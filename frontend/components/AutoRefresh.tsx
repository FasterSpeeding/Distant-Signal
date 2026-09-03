'use client';

import { useEffect, useRef } from 'react';
import { useRouter } from 'next/navigation';
import { useDocumentVisibility, useInterval } from '@mantine/hooks';

const REFRESH_INTERVAL_MS = 30_000;

/** Side-effect-only component (renders nothing) mounted once in the root
 * layout so every page keeps its data live while left open, rather than
 * only updating on a full navigation. `router.refresh()` re-runs the
 * current route's server-side data fetches (picking up the live
 * `cache: 'no-store'` responses from `lib/api.ts`) while preserving client
 * state such as scroll position — unlike a hard reload.
 *
 * 30s matches the cadence already implied elsewhere in the app: the former
 * `next: { revalidate: 30 }` windows on the line-status/freshness fetches
 * (now `no-store`, refreshed instead by this interval), and
 * `RELATIVE_TIME_TICK_MS` in `LastUpdated.tsx`.
 *
 * The interval is paused while the document is hidden (design spec
 * Decision 4): polling a backgrounded tab every 30s spends the visitor's
 * battery and the backend's capacity on data nobody is looking at, and
 * during an outage it also piles up failures nobody is there to see. */
export function AutoRefresh() {
  const router = useRouter();
  const visibility = useDocumentVisibility();
  const interval = useInterval(() => router.refresh(), REFRESH_INTERVAL_MS);

  // `useInterval` returns a fresh object literal `{ start, stop, toggle,
  // active }` on every render (verified in
  // node_modules/@mantine/hooks/esm/use-interval/use-interval.mjs), so it
  // must NOT go in the dependency array: the effect would re-run every
  // render and fire router.refresh() in a loop. `useRouter()` is likewise
  // not guaranteed to be referentially stable. Both are therefore read
  // through a ref kept current on each render -- the same technique
  // useInterval itself uses internally for its `intervalValueRef` -- and
  // the effect keys on `visibility` alone, which is the only input that
  // should actually start or stop the timer.
  const latest = useRef({ router, interval });
  latest.current = { router, interval };

  // Mount is not a transition. The immediate refresh below is for a
  // visitor coming *back* to a tab that went stale while hidden; firing it
  // on the first run too would re-fetch a page the server rendered
  // milliseconds earlier, roughly doubling render load per full page load
  // -- including against an already-failing backend, which is the opposite
  // of what pausing while hidden is for (design spec Decision 4, which
  // says "on transitioning back to 'visible'"). The previous
  // `{ autoInvoke: true }` did not do this: it only started the timer, it
  // never invoked the callback.
  const firstRun = useRef(true);

  useEffect(() => {
    if (visibility !== 'visible') {
      firstRun.current = false;
      latest.current.interval.stop();
      return undefined;
    }
    if (firstRun.current) {
      firstRun.current = false;
    } else {
      // Refresh once immediately on becoming visible rather than making a
      // returning visitor wait up to 30s: whatever is on screen is by
      // definition as stale as the time they spent away.
      latest.current.router.refresh();
    }
    latest.current.interval.start();
    return () => latest.current.interval.stop();
  }, [visibility]);

  return null;
}
