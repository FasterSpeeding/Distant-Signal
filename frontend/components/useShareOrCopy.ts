'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

/** `idle`, `copied` for a couple of seconds after a clipboard write, or
 * `failed` when the clipboard refused (permission denied, insecure
 * context). A native share sheet leaves it `idle`: the OS gives its own
 * feedback. */
export type ShareOrCopyState = 'idle' | 'copied' | 'failed';

/** How long "Copied" stays up: long enough to read, short enough that the
 * control doesn't feel stuck. */
export const COPIED_RESET_MS = 2000;

/** The app's one copy-to-clipboard routine, with an optional Web Share
 * first.
 *
 * With `share: true`, `navigator.share` runs where it exists (most mobile
 * browsers), falling back to the clipboard elsewhere (desktop Firefox has
 * never shipped it). A user cancelling the share sheet rejects with an
 * `AbortError`: that is them changing their mind, so nothing else happens.
 * Any other rejection falls back to the clipboard, since some copy of the
 * link beats a dead click. `AbortError` is matched by `.name`, not
 * `instanceof Error`: a `DOMException` doesn't extend `Error` in every
 * engine (jsdom's doesn't).
 *
 * The feature check runs per call, not once at module load, so it stays
 * right if a test or polyfill swaps `navigator.share` between renders. */
export function useShareOrCopy({ share = false, shareTitle }: { share?: boolean; shareTitle?: string } = {}) {
  const [state, setState] = useState<ShareOrCopyState>('idle');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  const run = useCallback(
    async (value: string, title: string | undefined = shareTitle) => {
      if (timer.current) clearTimeout(timer.current);
      if (share && typeof navigator.share === 'function') {
        try {
          await navigator.share({ url: value, ...(title !== undefined && { title }) });
          setState('idle');
          return;
        } catch (err) {
          if (err && typeof err === 'object' && 'name' in err && err.name === 'AbortError') return;
          // Fall through to the clipboard.
        }
      }
      try {
        await navigator.clipboard.writeText(value);
        setState('copied');
        timer.current = setTimeout(() => setState('idle'), COPIED_RESET_MS);
      } catch {
        setState('failed');
      }
    },
    [share, shareTitle],
  );

  return { state, run };
}
