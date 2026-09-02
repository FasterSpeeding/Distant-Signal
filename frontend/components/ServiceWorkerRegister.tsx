'use client';

import { useEffect } from 'react';
import { useMounted } from '@mantine/hooks';

/** Side-effect-only Client Component (renders nothing), mounted once in
 * RootLayout alongside AutoRefresh/ColorSchemeMeta -- the same
 * established "root-scope side effect, no new provider" shape those two
 * already use. See
 * docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md
 * Decision 4.
 *
 * register('/sw.js') with no explicit `scope` option is deliberate, not
 * an omission -- registering from a root-level path gives the service
 * worker the maximum possible scope (the whole origin), which both
 * sw.js's own allowlist (needs visibility into every navigation) and the
 * sibling line-status-notifications spec's future notificationclick
 * handler (needs a registration scope covering any in-app deep link)
 * require.
 *
 * A registration failure (unsupported browser, a sw.js fetch failure, a
 * syntax error in a bad deploy) is caught and swallowed -- same
 * degrade-quietly shape AuthNavItem/DataFreshnessNavItem (app/layout.tsx)
 * already use for a failed fetch in a root layout with no route-level
 * error.tsx. A broken registration must never break the page it's
 * mounted on; the app functions identically to today (no offline
 * support, no asset precaching) if this fails.
 *
 * `loadedAt`: a fresh ISO timestamp passed down from RootLayout -- a
 * Server Component, which re-executes on every navigation AND every
 * AutoRefresh-triggered router.refresh(). Recording
 * localStorage['lastSuccessfulLoadAt'] here, keyed on this prop actually
 * changing, is what gives offline.html's own "Last connected around X
 * ago" line a real, per-successful-load signal: if the request behind a
 * navigation/refresh had failed (genuinely offline), this component would
 * never receive a new `loadedAt` value in the first place, so the stored
 * timestamp only ever advances on an actual successful server response --
 * the same "no fabricated fallback" posture LastUpdated.tsx already takes
 * for its own timestamp. */
export function ServiceWorkerRegister({ loadedAt }: { loadedAt: string }) {
  const mounted = useMounted();

  useEffect(() => {
    if (!mounted) return;
    if ('serviceWorker' in navigator) {
      navigator.serviceWorker.register('/sw.js').catch(() => {
        // Swallowed -- see doc comment above.
      });
    }
  }, [mounted]);

  useEffect(() => {
    if (!mounted) return;
    try {
      window.localStorage.setItem('lastSuccessfulLoadAt', loadedAt);
    } catch {
      // localStorage can throw (private browsing / storage blocked) --
      // offline.html's own read of this key already tolerates a missing
      // value, so a failed write here is a silent no-op, not an error.
    }
  }, [mounted, loadedAt]);

  return null;
}
