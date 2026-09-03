'use client';

import { createContext, useContext, useEffect, useState } from 'react';
import { Notification } from '@mantine/core';
import { useMounted, useNetwork } from '@mantine/hooks';

/** Two consecutive failed `getDataFreshness()` calls (i.e. two AutoRefresh
 * cycles, ~60s) before the banner appears, one success to clear it. The
 * debounce exists so a single blipped request -- a pod restart, one
 * dropped packet -- never flashes a "Reconnecting..." banner at someone
 * whose app is working fine. Design spec Decision 2. */
const CONSECUTIVE_FAILURES_TO_TRIP = 2;

/** Non-throwing default deliberately: `app/error.tsx` renders inside a
 * tripped Next error boundary, and a context hook that threw when no
 * provider was found would turn a recoverable error page into an
 * unrecoverable one. "No provider" honestly means "we have no reason to
 * think anything is disconnected". */
export const ConnectivityContext = createContext<{ disconnected: boolean }>({
  disconnected: false,
});

export function useConnectivity() {
  return useContext(ConnectivityContext);
}

/** Wraps the app shell (not a sibling of it) so that `app/error.tsx` --
 * which renders *below* this point in the tree, inside Next's
 * ErrorBoundaryHandler -- can read the context. That containment is
 * load-bearing for the self-healing error page; verified against a real
 * dev server before this landed (plan Task 0): a Context update does reach
 * a component inside a tripped error boundary, and React's propagation is
 * not blocked by the intervening class component.
 *
 * Two independent signals, deliberately treated differently (Decision 2):
 * `backendReachable` comes from the server and is debounced two-strikes,
 * because a server round-trip can blip. `useNetwork().online` is a local
 * browser fact with no round-trip to be wrong about, so it applies
 * immediately with no debounce. */
export function ConnectivityMonitor({
  backendReachable,
  observedAt,
  children,
}: {
  backendReachable: boolean;
  /** A fresh value per server render -- RootLayout passes
   * `new Date().toISOString()`, the same trick it already uses for
   * `ServiceWorkerRegister loadedAt`.
   *
   * This is what makes the two-strikes counter count *observations*
   * rather than *transitions*, and it is load-bearing. RootLayout
   * re-executes on every AutoRefresh `router.refresh()`, so during a real
   * outage it re-renders with `backendReachable={false}` every 30s -- an
   * unchanged value. An effect keyed on `backendReachable` alone would
   * therefore never re-run after the first failure, the counter would
   * stick at 1, and the banner would never appear at all. Keying on this
   * nonce makes each server render one observed outcome. */
  observedAt: string;
  children: React.ReactNode;
}) {
  // Everything below reads browser-only state (navigator.onLine), so the
  // banner stays hidden until mount and the context provides the only
  // honest server value -- `{ disconnected: false }` -- until then. Same
  // pre-mount discipline as PrideToggle/ThemeToggle/ServiceWorkerRegister.
  // Design spec Decision 9.
  const mounted = useMounted();
  const { online } = useNetwork();
  const [failures, setFailures] = useState(0);

  useEffect(() => {
    setFailures((current) => (backendReachable ? 0 : current + 1));
    // Keyed on `observedAt`, not `backendReachable` -- see that prop's
    // doc comment. `backendReachable` is read inside the updater rather
    // than depended on, so a repeated identical value still counts.
  }, [observedAt]);

  const backendDown = failures >= CONSECUTIVE_FAILURES_TO_TRIP;
  const disconnected = mounted && (!online || backendDown);

  return (
    <ConnectivityContext.Provider value={{ disconnected }}>
      {children}
      {disconnected && (
        // zIndex 300 sits below DataFreshnessInfo's tooltip (400) and
        // below Mantine's Modal, so this never covers something the
        // visitor deliberately opened. Design spec Decision 8.
        <div
          style={{
            position: 'fixed',
            bottom: 16,
            left: '50%',
            transform: 'translateX(-50%)',
            zIndex: 300,
          }}
        >
          {/* Copy is connectivity-neutral on purpose: it is true whether
              the visitor's own device is offline or the backend is
              unreachable, so there is one message and no branching. */}
          <Notification
            loading
            withCloseButton={false}
            title="Reconnecting…"
            role="status"
            aria-live="polite"
          >
            Can&apos;t reach live data right now — showing the last update.
          </Notification>
        </div>
      )}
    </ConnectivityContext.Provider>
  );
}
