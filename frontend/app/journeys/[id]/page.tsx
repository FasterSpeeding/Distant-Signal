import { Group, Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourney, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { AddJourneyLegButton } from '@/components/AddJourneyLegButton';
import { JourneyLegCard } from '@/components/JourneyLegCard';
import { JourneyStatusBadge } from '@/components/JourneyStatusBadge';
import { LoginLink } from '@/components/LoginLink';
import { ShareJourneyButton } from '@/components/ShareJourneyButton';
import { TextLink } from '@/components/TextLink';
import { formatDate } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import type { JourneyDetail } from '@/lib/types';

/** "London Kings Cross → Edinburgh, 22 Sept 2026" -- the fallback `<h1>`
 * for a journey with no `customName` set (the spec §4 rename pattern is
 * still deferred, see the code comment at its one call site below).
 * Review §2.5/M18: "Tracked journey" named nothing about THIS journey; the
 * 09-17 review made the same "default the title to the route" call for the
 * single-train page, and the route is exactly the one fact every journey
 * already carries on its first leg. Uses the FIRST leg's origin and the
 * LAST leg's destination so a (currently hypothetical, Phase 1 is
 * always-one-leg -- see `crates/api/src/data/journeys.rs`'s own module
 * doc comment) multi-leg journey reads as one through-route rather than
 * just its first leg. `null` only for a journey with zero legs, which
 * should not occur in practice (`journeys.legs` is never empty by
 * construction) -- falls back to the old generic title rather than
 * rendering an empty `<h1>`. */
function defaultJourneyTitle(journey: JourneyDetail): string {
  const firstLeg = journey.legs[0];
  if (!firstLeg) return 'Tracked journey';
  const lastLeg = journey.legs.at(-1) ?? firstLeg;
  const route = routeLabel(firstLeg.originCrs, firstLeg.originName, lastLeg.destinationCrs, lastLeg.destinationName);
  return `${route}, ${formatDate(firstLeg.serviceDate)}`;
}

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
  // Review §2.5/M16: "Add a leg" used to be offered at the same visual
  // weight as the status badge even while the CURRENT leg still needs a
  // train picked -- there is nothing to chain a new leg onto yet, and it
  // competed for attention with the one action that actually matters on
  // this page. Hidden until the last leg is matched.
  const canAddLeg = lastLeg !== null && lastLeg.trackedTrainState !== null;

  return (
    <Stack p="lg" gap="md">
      {/* The way back out. This page is reached by a `router.push` from
          the create flow and had no breadcrumb at all, so closing the tab
          lost the journey unless the user had memorised `/journeys/169`
          (2026-09-22 UX review, C1). `/track/mine` now lists journeys, so
          this link has a real destination. */}
      <TextLink href="/track/mine" underline="always">
        Back to my trains &amp; journeys
      </TextLink>
      <Group justify="space-between" align="baseline">
        <Title order={1}>{journey.customName ?? defaultJourneyTitle(journey)}</Title>
        <Group gap="xs">
          {/* Phase 2's status badge is a pure read -- shown to every viewer,
              owner or shared-group member alike. */}
          <JourneyStatusBadge legs={journey.legs} />
          {/* Both ACTIONS are owner-only. "Share" was already gated by
              Phase 4; "Add leg" is gated here for the same reason -- POST
              /Journeys/{id}/legs answers 404 for a non-owner (see
              `post_journey_leg_a_journey_owned_by_someone_else_is_404_not_403`),
              so offering the button to a shared-group viewer would only
              produce a dead end. Also gated on `canAddLeg` (M16, above). */}
          {journey.isOwner && canAddLeg && (
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
