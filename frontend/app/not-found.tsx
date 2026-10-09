import { Group, Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { TextLink } from '@/components/TextLink';

export const metadata: Metadata = {
  title: 'Page not found',
  robots: { index: false },
};

/** The app-wide 404, for any URL no route matches. Without it Next falls
 * back to its own built-in "404 | This page could not be found." screen:
 * inline-styled, off-theme, and with no way back into the app. Same shape
 * as the per-segment not-found pages (app/stations/[crs]/not-found.tsx and
 * friends), which still take over inside their own segments. */
export default function NotFound() {
  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": see app/error.tsx's fuller comment on this
          same pattern -- page-level h1, rendered size unchanged. */}
      <Title order={1} size="h2">
        Page not found
      </Title>
      <Text c="dimmed">
        There&apos;s no page at this address. The link may be mistyped, or the page may have moved.
      </Text>
      <Group gap="lg">
        <TextLink href="/" underline="always">
          Go to the home page
        </TextLink>
        <TextLink href="/lines" underline="always">
          Browse all lines
        </TextLink>
        <TextLink href="/stations" underline="always">
          Look up a station
        </TextLink>
      </Group>
    </Stack>
  );
}
