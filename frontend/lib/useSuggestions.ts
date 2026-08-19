'use client';

import { useEffect, useRef, useState } from 'react';
import type { Suggestion } from './types';

const DEBOUNCE_MS = 250;

/** Debounces `query` by 250ms, then calls `search(query, signal)`.
 * Aborts the in-flight request (via `signal`) whenever `query` changes
 * again before the previous call resolves, so a fast typist never has a
 * stale, slower response overwrite a newer one. Shared by every
 * operator/station autocomplete field. */
export function useSuggestions(
  query: string,
  search: (q: string, signal: AbortSignal) => Promise<Suggestion[]>,
): { suggestions: Suggestion[]; loading: boolean } {
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!query.trim()) {
      setSuggestions([]);
      setLoading(false);
      return;
    }

    const controller = new AbortController();
    setLoading(true);
    const timer = setTimeout(() => {
      search(query, controller.signal)
        .then((results) => {
          if (!controller.signal.aborted) {
            setSuggestions(results);
          }
        })
        .catch((err: unknown) => {
          if (!(err instanceof DOMException && err.name === 'AbortError')) {
            setSuggestions([]);
          }
        })
        .finally(() => {
          if (!controller.signal.aborted) {
            setLoading(false);
          }
        });
    }, DEBOUNCE_MS);

    return () => {
      clearTimeout(timer);
      controller.abort();
    };
  }, [query, search]);

  return { suggestions, loading };
}
