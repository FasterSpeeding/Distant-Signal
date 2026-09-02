'use client';

import { Stack, Title, Text, Button } from '@mantine/core';

export default function Error({
  error,
  reset,
}: {
  error: Error & { digest?: string };
  reset: () => void;
}) {
  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": this is the page's top-level heading, so
          it must be the h1 -- a 404/500 with no h1 at all fired axe's
          `page-has-heading-one` on every not-found template in the app
          (docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md).
          `size="h2"` keeps the rendered size exactly as it was; only the
          tag changes. These render inside the root layout's <main>
          Container, so they need no landmarking of their own. */}
      <Title order={1} size="h2">Couldn&apos;t load status data</Title>
      <Text c="dimmed">{error.message}</Text>
      <Button onClick={reset} w="fit-content">
        Try again
      </Button>
    </Stack>
  );
}
