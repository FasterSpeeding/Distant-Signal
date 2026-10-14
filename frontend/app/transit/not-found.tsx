import { Stack, Text, Title } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

/** An unknown tram network, line or stop, or a network we don't show yet. */
export default function TransitNotFound() {
  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": see app/error.tsx. */}
      <Title order={1} size="h2">
        Not found
      </Title>
      <Text c="dimmed">We don&apos;t have a tram network, line or stop at this address.</Text>
      <TextLink href="/lines" underline="always">
        See all lines
      </TextLink>
    </Stack>
  );
}
