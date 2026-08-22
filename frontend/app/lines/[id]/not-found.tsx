import { Group, Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function LineNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Line not found</Title>
      <Text c="dimmed">No line matches that ID.</Text>
      {/* The page previously had no link anywhere on it — a genuine dead
          end reachable from a stale bookmark or a deleted custom line. */}
      <Group gap="lg">
        <TextLink href="/lines" underline="always">
          Browse all lines
        </TextLink>
        <TextLink href="/" underline="always">
          Back to your dashboard
        </TextLink>
      </Group>
    </Stack>
  );
}
