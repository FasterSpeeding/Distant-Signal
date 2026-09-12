'use client';

import { createContext, useContext, type ReactNode } from 'react';
import type { GroupSummary } from './types';

/** `null`: an anonymous visitor -- mirrors `getMyGroups()`'s own
 * null-on-401 meaning (`lib/api.ts`) all the way through to this context,
 * rather than collapsing it into `[]` at the fetch boundary. `GroupSummary[]`
 * (possibly empty): an authenticated caller, these are their groups.
 *
 * Every current consumer (`useGroupSummaries`, below) still collapses both
 * `null` and `[]` to "nothing to offer" -- same as before this context
 * existed -- but keeping the distinction in the context itself, rather than
 * baking the collapse in earlier, means a future caller that genuinely needs
 * to tell "anonymous" apart from "a real, group-less member" doesn't have to
 * re-plumb this all the way back to `RootLayout`. */
interface GroupSummariesContextValue {
  groups: GroupSummary[] | null;
}

/** Non-throwing default, same reasoning as `ConnectivityMonitor.tsx`'s own
 * `ConnectivityContext`: a component rendered outside `GroupSummariesProvider`
 * (every test that doesn't explicitly wrap itself, for one) should degrade to
 * the same "nothing to offer" posture `useGroupSummaries`'s pre-context
 * fetch-failure/anonymous/zero-groups cases already shared, not throw. */
const GroupSummariesContext = createContext<GroupSummariesContextValue>({ groups: null });

/** Hydrates the group-share feature's data from a single SERVER-SIDE
 * `getMyGroups()` call made once in `RootLayout` (`app/layout.tsx`),
 * eliminating the cold-start race the previous per-component
 * `useEffect`-driven `GET /api/groups` fetch had: `TrackThisTrainButton`,
 * `TrackTrainForm`, and `AddToGroupButton` each read `groups` via
 * `useGroupSummaries()` below, and with this provider that value is present
 * at first paint/hydration -- there is no window between mount and fetch
 * completion during which a real click could observe a stale "zero groups"
 * default, because there never was a client-side fetch to wait on in the
 * first place.
 *
 * Deliberately holds no state of its own and performs no fetch: `groups` is
 * read fresh on every render, straight from the prop `RootLayout` passes.
 * `RootLayout` re-executes on every client-side navigation and every
 * `AutoRefresh`-triggered `router.refresh()` (see that component's own doc
 * comment, and the `loadedAt`/`observedAt` props it already threads through
 * for the same reason) -- so simply passing this render's value straight
 * through, rather than freezing the FIRST value this component ever saw in
 * a `useState` initializer, is what lets a membership change (e.g. joining a
 * group via `/groups/join/[token]`, then navigating elsewhere) reach this
 * context on the very next navigation/refresh instead of never at all. See
 * this feature's own follow-up doc/commit for the fuller reasoning on why no
 * *additional* client-only refresh mechanism (a dedicated `router.refresh()`
 * call, a re-fetch-on-focus effect, etc.) is needed on top of that. */
export function GroupSummariesProvider({
  groups,
  children,
}: {
  groups: GroupSummary[] | null;
  children: ReactNode;
}) {
  return <GroupSummariesContext.Provider value={{ groups }}>{children}</GroupSummariesContext.Provider>;
}

/** Reads the signed-in caller's groups from `GroupSummariesContext`
 * (populated by `GroupSummariesProvider`, wired into `RootLayout` from a
 * single server-side `getMyGroups()` call -- `app/layout.tsx`). Used by the
 * "track this train" group-share prompt (`TrackDestinationModal`, wired in
 * by `TrackThisTrainButton.tsx` and `TrackTrainForm.tsx`) and by
 * `AddToGroupButton.tsx`: those call sites need to know whether showing a
 * Personal-vs-group prompt (or the "Add to group" button at all) is
 * warranted, and need to know it correctly from the very first render --
 * see `GroupSummariesProvider`'s own doc comment for why reading from this
 * context (rather than this hook doing its own `useEffect` fetch, as it
 * used to) is what actually closes that race.
 *
 * `groups ?? []`: an anonymous visitor, a genuinely group-less member, and
 * (per `GroupSummariesProvider`'s failure posture -- ultimately
 * `getMyGroups()`'s own doc comment in `app/layout.tsx`) any fetch failure
 * all reach this context as `null`, and this hook's own contract -- unchanged
 * from before this context existed -- treats all three identically as
 * "nothing to offer". This is also why this hook still returns a plain
 * `GroupSummary[]`, never `null`: every existing call site already does
 * `groups.length === 0`/`groups.map(...)` unconditionally, and preserving
 * that exact contract is what let this fix land without touching any of
 * them. */
export function useGroupSummaries(): { groups: GroupSummary[] } {
  const { groups } = useContext(GroupSummariesContext);
  return { groups: groups ?? [] };
}
