'use client';

import { useEffect } from 'react';
import { Button, Group, Stack, Text, Title } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

/** The app's ONLY error boundary -- there is no `global-error.tsx` and no
 * per-route `error.tsx` (see `app/layout.tsx`, which works around that gap
 * with `.catch()` fallbacks rather than adding one). So its copy has to
 * work on every route, which is why the heading is no longer
 * "Couldn't load status data".
 *
 * `error.message` is deliberately NOT rendered. It used to be, and on the
 * `/connect-claude` crash it printed "Minified React error #130; visit
 * https://react.dev/errors/130?args[]=..." as the page's body copy
 * (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F5). The
 * message goes to the console instead; `digest` -- Next.js's opaque
 * server-error correlation hash, which exists precisely to be quoted by a
 * user -- is what gets shown, and only when there is one (client-side
 * render errors have none). */
export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  useEffect(() => {
    // The only reporting channel this app has. Previously nothing logged
    // the error at all, while the useless half of it was rendered.
    console.error('Unhandled error rendering a page', { digest: error.digest, error });
  }, [error]);

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
        Something went wrong
      </Title>
      <Text c="dimmed">
        This page couldn&apos;t be loaded. It may be a temporary problem with the live data
        feeds — try again in a moment.
      </Text>
      <Group>
        <Button onClick={reset} w="fit-content">
          Try again
        </Button>
        <TextLink href="/">Back to your dashboard</TextLink>
      </Group>
      {error.digest && (
        <Text size="xs" c="dimmed">
          Reference: {error.digest}
        </Text>
      )}
    </Stack>
  );
}
