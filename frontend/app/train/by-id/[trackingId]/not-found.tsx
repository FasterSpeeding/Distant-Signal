import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function TrackedTrainByIdNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Tracked train not found</Title>
      <Text c="dimmed">No tracking pin matches that id.</Text>
      <TextLink href="/track" underline="always">
        Track a train
      </TextLink>
    </Stack>
  );
}
