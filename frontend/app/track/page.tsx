import { Stack, Title, Text } from '@mantine/core';
import { TrackTrainForm } from '@/components/TrackTrainForm';

export default async function TrackPage({
  searchParams,
}: {
  searchParams: Promise<{ origin?: string | string[] }>;
}) {
  const { origin } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param (e.g.
  // `?origin=a&origin=b`) -- fall back to the first value rather than
  // letting `.toUpperCase()` throw on an array.
  const originParam = Array.isArray(origin) ? origin[0] : origin;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Track a Train</Title>
      <Text c="dimmed">
        Pin a specific train to see its live position, delay and next calling point as Network Rail
        reports it.
      </Text>
      <TrackTrainForm initialOrigin={originParam?.toUpperCase()} />
    </Stack>
  );
}
