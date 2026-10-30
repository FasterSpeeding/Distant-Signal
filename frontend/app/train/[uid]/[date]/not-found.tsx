import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

/** A UID and date with no train: a mistyped UID on the Trains section's
 * headcode-or-UID lookup lands here, so the way out is back to it. */
export default function TrainByUidNotFound() {
  return (
    <Stack p="lg" gap="md">
      {/* order={1}, size="h2": see app/error.tsx's fuller comment on this
          same pattern -- page-level h1, rendered size unchanged. */}
      <Title order={1} size="h2">
        Train not found
      </Title>
      <Text c="dimmed">We have no train with that UID on that date.</Text>
      <TextLink href="/trains" underline="always">
        Look up a train
      </TextLink>
    </Stack>
  );
}
