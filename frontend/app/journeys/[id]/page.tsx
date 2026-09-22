import { Group, Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourney, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { JourneyLegCard } from '@/components/JourneyLegCard';
import { LoginLink } from '@/components/LoginLink';
import { ShareJourneyButton } from '@/components/ShareJourneyButton';

export const revalidate = 0;

/** `/journeys/[id]` -- design doc §4. One card per leg (Phase 1: always
 * exactly one, see `crates/api/src/data/journeys.rs`'s own module doc
 * comment). No editable header, no delete, no share-to-group button, no
 * skip badge, no platform column -- all explicitly deferred, see this
 * plan's own Non-goals for the reasoning behind each. */
export default async function JourneyDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  if (!/^\d+$/.test(id)) {
    notFound();
  }

  let journey;
  try {
    journey = await getJourney(Number(id));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    // Same distinct "log in, this might be yours" posture
    // app/train/by-id/[trackingId]/page.tsx already takes for the
    // identical 401-vs-404 split, for the same reason: this page has no
    // public sibling content to fall back to.
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Someone&apos;s tracked journey — log in to see it</Title>
          <LoginLink underline="always">Log in to view this journey</LoginLink>
        </Stack>
      );
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between" align="baseline">
        <Title order={1}>{journey.customName ?? 'Tracked journey'}</Title>
        <Group gap="xs">
          <ShareJourneyButton journeyId={journey.id} />
        </Group>
      </Group>
      {journey.legs.map((leg) => (
        <JourneyLegCard key={leg.id} journeyId={journey.id} leg={leg} />
      ))}
    </Stack>
  );
}
