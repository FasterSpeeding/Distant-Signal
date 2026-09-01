import { Group, Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function IncidentNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Incident not found</Title>
      <Text c="dimmed">
        No incident matches that ID. It may have been mistyped, or this app may never have ingested it — there is
        no retention/prune job on incident records today, so this is indistinguishable from a very old, no-longer-tracked one.
      </Text>
      <Group gap="lg">
        <TextLink href="/" underline="always">
          Back to your dashboard
        </TextLink>
      </Group>
    </Stack>
  );
}
