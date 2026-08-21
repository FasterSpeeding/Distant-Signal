'use client';

import { useRouter } from 'next/navigation';
import { useInterval } from '@mantine/hooks';

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
 * `RELATIVE_TIME_TICK_MS` in `LastUpdated.tsx`. */
export function AutoRefresh() {
  const router = useRouter();
  useInterval(() => router.refresh(), REFRESH_INTERVAL_MS, { autoInvoke: true });
  return null;
}
