'use client';

import { useEffect, useRef, useState } from 'react';

/** How long a server-backed list waits for the filters to settle before it
 * fetches: long enough that typing a word is one request, short enough to
 * feel live. */
export const LIVE_SEARCH_DELAY_MS = 350;

/** Runs a server-backed list's search whenever its filters change, for the
 * pages whose filters apply live (/incidents, /trains; docs/style-guide.md,
 * "List search and filters").
 *
 * `query` is the request's query string, or `null` when the filters can't
 * be searched (a half-typed station code, an inverted range). A new value
 * is fetched after `delayMs` (the first one at once), the request before it
 * is aborted, and `run` is handed an `AbortSignal` so a slow, superseded
 * response never lands over a newer one: `run` must check
 * `signal.aborted` after every `await` before it sets any state.
 *
 * `pending` is true from the moment the filters change until the matching
 * response is in, for a small indicator; the rows on screen stay until
 * then. The same query twice in a row is not refetched. */
export function useLiveSearch(
  query: string | null,
  run: (query: string, signal: AbortSignal) => Promise<void>,
  { delayMs = LIVE_SEARCH_DELAY_MS }: { delayMs?: number } = {},
): { pending: boolean } {
  const [pending, setPending] = useState(query !== null);
  const runRef = useRef(run);
  useEffect(() => {
    runRef.current = run;
  });
  const lastQueryRef = useRef<string | null>(null);
  const inFlightRef = useRef<AbortController | null>(null);
  const startedRef = useRef(false);

  useEffect(() => {
    if (query === null || query === lastQueryRef.current) {
      // Nothing new to fetch; still pending only if a request is out.
      setPending(inFlightRef.current !== null);
      return;
    }
    setPending(true);
    const delay = startedRef.current ? delayMs : 0;
    const timer = window.setTimeout(() => {
      startedRef.current = true;
      lastQueryRef.current = query;
      inFlightRef.current?.abort();
      const controller = new AbortController();
      inFlightRef.current = controller;
      void runRef.current(query, controller.signal).finally(() => {
        if (inFlightRef.current !== controller) return;
        inFlightRef.current = null;
        setPending(false);
      });
    }, delay);
    return () => window.clearTimeout(timer);
  }, [query, delayMs]);

  useEffect(() => () => inFlightRef.current?.abort(), []);

  return { pending };
}
