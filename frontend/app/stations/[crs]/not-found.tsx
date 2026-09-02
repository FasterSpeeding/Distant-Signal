import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function StationNotFound() {
  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": see app/error.tsx's fuller comment on this
          same pattern -- page-level h1, rendered size unchanged. */}
      <Title order={1} size="h2">Station not found</Title>
      <Text c="dimmed">
        No National Rail station matches that code. Station codes are three letters, like WOK or EUS.
      </Text>
      <TextLink href="/stations" underline="always">
        Look up a station
      </TextLink>
    </Stack>
  );
}
