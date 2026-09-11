'use client';

import { useEffect, useState } from 'react';
import type { GroupSummary } from './types';

/** Fetches the signed-in caller's groups once on mount, via the same-origin
 * `/api/groups` proxy (`GET /public/groups`,
 * `crates/api/src/routes/groups.rs::list_groups`) -- Client Components can't
 * read the server-only `API_BASE_URL` env var `lib/api.ts` relies on, same
 * reasoning as every other same-origin `/api/*` call in this codebase
 * (`PinToggle`, `TrackTrainForm`, `TrackThisTrainButton`).
 *
 * Powers the "track this train" group-share prompt
 * (`TrackDestinationModal`, wired in by `TrackThisTrainButton.tsx` and
 * `TrackTrainForm.tsx`): those call sites need to know, before the user
 * even clicks "track", whether showing a Personal-vs-group prompt is
 * warranted at all. `groups` starts as `[]` and stays that way for an
 * anonymous visitor, a genuinely group-less user, or any non-200 response
 * (network blip, session hiccup) -- all three are treated identically as
 * "nothing to offer", per the feature's own "any non-200 means skip the
 * prompt, behave exactly as today" rule. This hook never surfaces a loading
 * flag its callers would need to gate the track button on: the fetch is a
 * single fast GET that in practice resolves long before a real click, and
 * gating the button on it would itself be a behavior change for the
 * (majority) zero-groups case this feature must leave untouched. */
export function useGroupSummaries(): { groups: GroupSummary[] } {
  const [groups, setGroups] = useState<GroupSummary[]>([]);

  useEffect(() => {
    let cancelled = false;
    fetch('/api/groups')
      .then((response) => (response.ok ? (response.json() as Promise<unknown>) : []))
      .then((result) => {
        // `Array.isArray`, not a bare cast: `TrackDestinationModal` calls
        // `.map` on this unconditionally (it's always mounted, just not
        // always `opened`), so a malformed/unexpected response body here
        // must resolve to "no groups", not a runtime crash.
        if (!cancelled) setGroups(Array.isArray(result) ? (result as GroupSummary[]) : []);
      })
      .catch(() => {
        // Deliberately swallowed -- see this hook's own doc comment: a
        // failed fetch here means "nothing to offer", not an error worth
        // surfacing.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return { groups };
}
