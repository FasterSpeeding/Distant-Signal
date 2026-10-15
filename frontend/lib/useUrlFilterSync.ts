'use client';

import { useEffect, useRef } from 'react';

/** How long the URL waits for typing to settle before it is rewritten.
 * Safari throttles `history.replaceState` (and throws past the limit), so
 * one call per keystroke is not safe. */
export const URL_SYNC_DELAY_MS = 300;

/** `url` with the params in `keys` replaced by `values` (an array appends
 * one param per entry; empty strings, nulls and empty arrays remove the
 * param), keeping any other params and the hash. Returns a
 * path, not an absolute URL, for `history.replaceState`. */
export function replaceUrlParams(
  url: string,
  keys: readonly string[],
  values: Readonly<Record<string, string | readonly string[] | null | undefined>>,
): string {
  const parsed = new URL(url);
  const params = parsed.searchParams;
  // A single value already in the URL is replaced where it stands, so a
  // shared link's params keep their order; everything else is removed and
  // appended.
  for (const key of keys) {
    const value = values[key];
    if (typeof value !== 'string' || value === '' || !params.has(key) || params.getAll(key).length > 1) {
      params.delete(key);
    }
  }
  for (const [key, value] of Object.entries(values)) {
    if (value === null || value === undefined) continue;
    if (typeof value === 'string') {
      if (value !== '') params.set(key, value);
    } else {
      for (const entry of value) params.append(key, entry);
    }
  }
  const search = params.toString();
  return `${parsed.pathname}${search ? `?${search}` : ''}${parsed.hash}`;
}

/** Keeps a list's filter state in the address bar, so refresh, share and
 * Back all see what is on screen (docs/style-guide.md, "List search and
 * filters").
 *
 * `history.replaceState`, not `router.replace`: Next.js keeps
 * `useSearchParams` in sync with it, and it does not re-run the page's
 * server render the way `router.replace` does (which refetched everything
 * the page loads on every change and could reshuffle the list under the
 * visitor's finger). Replace, not push: a filter change is not a new place
 * to go Back to. Debounced, and skipped on mount so landing on a page never
 * rewrites its own URL.
 *
 * `state` is compared by value (`JSON.stringify`), so an object literal
 * built on every render is fine; `toUrl` turns the current URL and state
 * into the next path (usually through `replaceUrlParams`). */
export function useUrlFilterSync<T>(
  state: T,
  toUrl: (currentUrl: string, state: T) => string,
  { delayMs = URL_SYNC_DELAY_MS }: { delayMs?: number } = {},
): void {
  const key = JSON.stringify(state);
  const latest = useRef({ state, toUrl });
  useEffect(() => {
    latest.current = { state, toUrl };
  });
  const syncedKeyRef = useRef<string | null>(null);
  useEffect(() => {
    if (syncedKeyRef.current === null) {
      syncedKeyRef.current = key;
      return;
    }
    if (syncedKeyRef.current === key) return;
    const timer = window.setTimeout(() => {
      syncedKeyRef.current = key;
      const next = latest.current.toUrl(window.location.href, latest.current.state);
      try {
        // `null` state: Next.js's patched `replaceState` then copies its
        // own router state across and syncs `useSearchParams`, which it
        // skips for a state object that already carries its markers.
        window.history.replaceState(null, '', next);
      } catch {
        // Over the browser's replaceState rate limit: the list is still
        // right, only the address bar lags until the next change.
      }
    }, delayMs);
    return () => window.clearTimeout(timer);
  }, [key, delayMs]);
}
