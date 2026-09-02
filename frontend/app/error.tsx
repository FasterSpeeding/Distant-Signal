'use client';

import { useEffect, useRef } from 'react';
import { Stack, Title, Text, Button } from '@mantine/core';
import { useConnectivity } from '@/components/ConnectivityMonitor';

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  const { disconnected } = useConnectivity();

  // Next's own ErrorBoundaryHandler only clears a tripped error when the
  // *pathname* changes (getDerivedStateFromProps in
  // node_modules/next/dist/client/components/error-boundary.js), and
  // AutoRefresh's router.refresh() never changes the pathname -- so before
  // this effect existed, a visitor who hit this page during a backend
  // outage stayed on it until they manually clicked "Try again" or
  // navigated away, no matter how long ago the backend came back. `reset`
  // is a prop only this component receives; no sibling elsewhere in the
  // tree can call it, which is why this fix can only live here. See
  // docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md
  // Decision 6.
  //
  // Only ever on a true -> false transition, never on the initial render.
  // A genuine (non-connectivity) Server Component bug renders this page
  // with `disconnected` already false; calling reset() there would
  // re-render the segment, throw again, and loop the boundary forever.
  // Tracking the previous value makes "the connection came back" the only
  // trigger.
  const wasDisconnected = useRef(disconnected);
  useEffect(() => {
    if (wasDisconnected.current && !disconnected) {
      reset();
    }
    wasDisconnected.current = disconnected;
  }, [disconnected, reset]);

  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": this is the page's top-level heading, so
          it must be the h1 -- a 404/500 with no h1 at all fired axe's
          `page-has-heading-one` on every not-found template in the app
          (docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md).
          `size="h2"` keeps the rendered size exactly as it was; only the
          tag changes. These render inside the root layout's <main>
          Container, so they need no landmarking of their own. */}
      <Title order={1} size="h2">
        {disconnected ? 'Trying to reconnect…' : "Couldn't load status data"}
      </Title>
      {disconnected ? (
        <Text c="dimmed">
          Can&apos;t reach live data right now. This page will come back on its own as soon as the
          connection returns.
        </Text>
      ) : (
        <Text c="dimmed">{error.message}</Text>
      )}
      {/* Kept in both states: the auto-reset above only fires on a
          reconnect, so a visitor looking at a non-connectivity error still
          needs a way to retry, and one looking at the reconnecting state
          shouldn't be forced to wait if they'd rather poke it. */}
      <Button onClick={reset} w="fit-content">
        Try again
      </Button>
    </Stack>
  );
}
