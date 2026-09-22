import { Group, Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourney, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { AddJourneyLegButton } from '@/components/AddJourneyLegButton';
import { JourneyLegCard } from '@/components/JourneyLegCard';
import { JourneyStatusBadge } from '@/components/JourneyStatusBadge';
import { LoginLink } from '@/components/LoginLink';
import { ShareJourneyButton } from '@/components/ShareJourneyButton';

export const revalidate = 0;

/** `/journeys/[id]` -- design doc §4. One card per leg (Phase 1: always
 * exactly one, see `crates/api/src/data/journeys.rs`'s own module doc
 * comment). No editable header, no delete, no skip badge, no platform
 * column -- all explicitly deferred, see this plan's own Non-goals for the
 * reasoning behind each. A share-to-group button DOES exist (Task 8), but
 * only for the journey's owner (`journey.isOwner`) -- a non-owning group
 * member reaches this page via `journey_readable_by`'s group-shared read
 * path and gets neither that button nor the leg-level owner-only controls
 * (`JourneyLegCard`'s own `isOwner` gating). */
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

  const lastLeg = journey.legs.at(-1) ?? null;
  const priorDestinationCrs =
    lastLeg?.destinationCrs ?? lastLeg?.trackedTrainState?.scheduleDestinationCrs ?? null;

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between" align="baseline">
        <Title order={1}>{journey.customName ?? 'Tracked journey'}</Title>
        <Group gap="xs">
          {/* Phase 2's status badge is a pure read -- shown to every viewer,
              owner or shared-group member alike. */}
          <JourneyStatusBadge legs={journey.legs} />
          {/* Both ACTIONS are owner-only. "Share" was already gated by
              Phase 4; "Add leg" is gated here for the same reason -- POST
              /Journeys/{id}/legs answers 404 for a non-owner (see
              `post_journey_leg_a_journey_owned_by_someone_else_is_404_not_403`),
              so offering the button to a shared-group viewer would only
              produce a dead end. */}
          {journey.isOwner && (
            <AddJourneyLegButton journeyId={journey.id} priorDestinationCrs={priorDestinationCrs} />
          )}
          {journey.isOwner && <ShareJourneyButton journeyId={journey.id} />}
        </Group>
      </Group>
      {journey.legs.map((leg) => (
        <JourneyLegCard key={leg.id} journeyId={journey.id} leg={leg} isOwner={journey.isOwner} />
      ))}
    </Stack>
  );
}
