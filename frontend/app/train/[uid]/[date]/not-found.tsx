import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function TrackedTrainByUidNotFound() {
  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": see app/error.tsx's fuller comment on this
          same pattern -- page-level h1, rendered size unchanged. */}
      <Title order={1} size="h2">Tracked train not found</Title>
      <Text c="dimmed">No resolved tracked train matches that train and date.</Text>
      <TextLink href="/track" underline="always">
        Track a train
      </TextLink>
    </Stack>
  );
}
