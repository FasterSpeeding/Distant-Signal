'use client';

import { useState } from 'react';
import { useSearchParams } from 'next/navigation';

/** The page's query params as the address bar has them when the list
 * mounts, or `null` outside the app router (unit tests), where the caller
 * falls back to its `initial*` props.
 *
 * A list's filters are seeded from here rather than from the page's server
 * props because of Back. The filters are written with
 * `history.replaceState` (`useUrlFilterSync`), which Next.js follows for
 * `useSearchParams` but not for the server-rendered page it has cached:
 * after following a result and pressing Back, the router restores the page
 * as it was first rendered, with the props of the URL it was first opened
 * at, while the address bar (and this) has the filters the visitor last
 * set. Read once, on mount: later URL writes are this list's own. */
export function useInitialSearchParams(): URLSearchParams | null {
  // Typed non-null, but `null` outside the app router's context.
  const params = useSearchParams() as ReturnType<typeof useSearchParams> | null;
  const [snapshot] = useState(() => (params ? new URLSearchParams(params.toString()) : null));
  return snapshot;
}

/** `params.get(key)`, as `undefined` when absent, for an `initial*` prop. */
export function paramValue(params: URLSearchParams, key: string): string | undefined {
  return params.get(key) ?? undefined;
}
