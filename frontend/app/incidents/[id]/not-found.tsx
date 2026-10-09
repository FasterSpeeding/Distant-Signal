import { Group, Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function IncidentNotFound() {
  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": see app/error.tsx's fuller comment on this
          same pattern -- page-level h1, rendered size unchanged. */}
      <Title order={1} size="h2">
        Incident not found
      </Title>
      <Text c="dimmed">No incident has that ID. Check the link, or search the incidents.</Text>
      <Group gap="lg">
        <TextLink href="/" underline="always">
          Back to your dashboard
        </TextLink>
      </Group>
    </Stack>
  );
}
