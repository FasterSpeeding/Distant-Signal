import { searchKey } from './stationSearch';
import { TRANSIT_STOP_ID } from './transitIds';
import type { TransitStop } from './types';

/** Rows "Find a stop" lists at most; past that, its last row opens the
 * network's A to Z filtered by the same text. */
export const STOP_FINDER_LIMIT = 8;

/** A stop the finder can list: its id and name, and its lines' public ids
 * (`metrolink:blue`), to name them under it. */
export type FinderStop = Pick<TransitStop, 'id' | 'name' | 'lines'>;

/** 0 exact, 1 name prefix, 2 a word starting with the query, 3 anything
 * else that holds every word typed. `null` for no match. */
function matchTier(name: string, needle: string, words: readonly string[]): number | null {
  if (!words.every((word) => name.includes(word))) return null;
  if (name === needle) return 0;
  if (name.startsWith(needle)) return 1;
  const nameWords = name.split(' ');
  if (nameWords.some((word) => word.startsWith(needle))) return 2;
  return 3;
}

/** The stops whose names hold every word of `query` (in any order,
 * ignoring case and punctuation: `kings cross` finds `King's Cross St.
 * Pancras`), best first: exact name, name prefix, word prefix, then the
 * rest, each tier A to Z. `[]` for a blank query. */
export function rankStops<T extends FinderStop>(stops: readonly T[], query: string): T[] {
  const needle = searchKey(query);
  if (!needle) return [];
  const words = needle.split(' ');
  return stops
    .flatMap((stop) => {
      const name = searchKey(stop.name);
      const tier = matchTier(name, needle, words);
      return tier === null ? [] : [{ stop, tier, name }];
    })
    .sort((a, b) => a.tier - b.tier || a.name.localeCompare(b.name, 'en-GB'))
    .map(({ stop }) => stop);
}

/** "Blue, Green and Red": the lines a stop is on, named from the network's
 * lines (`metrolink:blue` to "Blue"), in the order given. Lines the
 * network doesn't name are left out; `''` when none are named. */
export function stopLineNames(stop: FinderStop, lineNames: Readonly<Record<string, string>>): string {
  const names = stop.lines.flatMap((id) => {
    const name = lineNames[id];
    return name ? [name] : [];
  });
  if (names.length <= 1) return names.join('');
  return `${names.slice(0, -1).join(', ')} and ${names[names.length - 1] ?? ''}`;
}

/** The A to Z page, filtered by `query` when given:
 * `/transit/metrolink/stops?q=picc`. */
export function stopIndexHref(network: string, query?: string): string {
  const base = `/transit/${encodeURIComponent(network)}/stops`;
  const q = query?.trim();
  return q ? `${base}?${new URLSearchParams({ q }).toString()}` : base;
}

const loaded = new Map<string, Promise<FinderStop[]>>();

/** The network's stations (`GET /api/transit/{network}/stops`), fetched
 * once per page load on first use and shared by every finder on the page.
 * A failed load is forgotten, so the next attempt retries. */
export function loadFinderStops(network: string): Promise<FinderStop[]> {
  const cached = loaded.get(network);
  if (cached) return cached;
  const request = fetchStops(network);
  loaded.set(network, request);
  request.catch(() => loaded.delete(network));
  return request;
}

/** Test hook: forget every loaded list. */
export function resetFinderStops(): void {
  loaded.clear();
}

async function fetchStops(network: string): Promise<FinderStop[]> {
  const response = await fetch(`/api/transit/${encodeURIComponent(network)}/stops`);
  if (!response.ok) throw new Error(`stops ${response.status}`);
  const body = (await response.json()) as { stops?: unknown };
  if (!Array.isArray(body.stops)) throw new Error('stops: no list');
  return (body.stops as unknown[]).flatMap((value) => {
    if (typeof value !== 'object' || value === null) return [];
    const stop = value as Partial<TransitStop>;
    if (typeof stop.id !== 'string' || typeof stop.name !== 'string' || !TRANSIT_STOP_ID.test(stop.id)) return [];
    const lines = Array.isArray(stop.lines) ? stop.lines.filter((l): l is string => typeof l === 'string') : [];
    return [{ id: stop.id, name: stop.name, lines }];
  });
}
