import { Stack, Title, Text } from '@mantine/core';
import { TrackTrainForm } from '@/components/TrackTrainForm';

export default async function TrackPage({
  searchParams,
}: {
  searchParams: Promise<{ origin?: string }>;
}) {
  const { origin } = await searchParams;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Track a Train</Title>
      <Text c="dimmed">
        Pin a specific train to see its live position, delay and next calling point as Network Rail
        reports it.
      </Text>
      <TrackTrainForm initialOrigin={origin?.toUpperCase()} />
    </Stack>
  );
}
