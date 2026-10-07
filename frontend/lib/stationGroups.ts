'use client';

import { useEffect, useState } from 'react';
import type { Suggestion } from './types';

/** One named station group `GET /Trips/plan` takes as `group:NAME` in
 * `via`, `waypoints` and the avoid lists: ANY of its members
 * (`GET /Trips/station-groups`, docs/api-changelog.md, 2026-10-07). The
 * code helpers (`isGroupCode`, `parseGroupCode`) live in
 * `lib/stationLabel.ts`, which server components import too. */
export interface StationGroup {
  /** `LON` */
  group: string;
  /** What a request sends: `group:LON`. */
  code: string;
  /** `London Terminals` */
  name: string;
  members: { crs: string; name: string | null }[];
}

/** How a picker offers a group: "Any of the London Terminals (18 stations)". */
export function groupOptionLabel(group: Pick<StationGroup, 'name' | 'members'>): string {
  const count = group.members.length;
  return `Any of the ${group.name} (${count} ${count === 1 ? 'station' : 'stations'})`;
}

/** The groups whose name or code matches what was typed (case-insensitive
 * substring), as suggestions a station picker can list first. Nothing for
 * an empty query, like the station search. */
export function matchingGroupSuggestions(query: string, groups: StationGroup[]): Suggestion[] {
  const q = query.trim().toLowerCase();
  if (!q) return [];
  return groups
    .filter((group) => group.name.toLowerCase().includes(q) || group.code.toLowerCase().includes(q))
    .map((group) => ({ code: group.code, name: groupOptionLabel(group) }));
}

/** Code -> picker label for every group, to seed a code -> name map. */
export function groupLabels(groups: StationGroup[]): Map<string, string> {
  return new Map(groups.map((group) => [group.code, groupOptionLabel(group)]));
}

/** `GET /api/Trips/station-groups`; `[]` on any failure (the pickers then
 * offer stations only). */
export async function fetchStationGroups(signal?: AbortSignal): Promise<StationGroup[]> {
  try {
    const response = await fetch('/api/Trips/station-groups', signal ? { signal } : {});
    if (!response.ok) return [];
    const body = (await response.json()) as { groups?: StationGroup[] };
    return Array.isArray(body.groups) ? body.groups : [];
  } catch {
    return [];
  }
}

let cached: Promise<StationGroup[]> | null = null;

/** The station groups, fetched at most once per page load while that
 * succeeds (they change only with a deploy, and the response is
 * cacheable); a failure is retried next time. */
export function loadStationGroups(): Promise<StationGroup[]> {
  cached ??= fetchStationGroups().then((found) => {
    if (found.length === 0) cached = null;
    return found;
  });
  return cached;
}

/** For tests: forget the loaded groups. */
export function resetStationGroupsCache(): void {
  cached = null;
}

/** `search`, with the station groups matching the query listed before its
 * results. The groups load on the first search, so a page that never
 * searches a group-capable picker never asks for them. */
export function withGroupSuggestions(
  search: (q: string, signal: AbortSignal) => Promise<Suggestion[]>,
): (q: string, signal: AbortSignal) => Promise<Suggestion[]> {
  return async (q, signal) => {
    const [groups, results] = await Promise.all([loadStationGroups(), search(q, signal)]);
    return [...matchingGroupSuggestions(q, groups), ...results];
  };
}

const NO_GROUPS: StationGroup[] = [];

/** The station groups once `needed` (some code on screen is a group's, so
 * it needs their names); a stable `[]` until then and until they arrive. */
export function useStationGroups(needed: boolean): StationGroup[] {
  const [groups, setGroups] = useState<StationGroup[]>(NO_GROUPS);
  const loaded = groups.length > 0;
  useEffect(() => {
    if (!needed || loaded) return;
    let active = true;
    void loadStationGroups().then((found) => {
      if (active && found.length > 0) setGroups(found);
    });
    return () => {
      active = false;
    };
  }, [needed, loaded]);
  return groups;
}
