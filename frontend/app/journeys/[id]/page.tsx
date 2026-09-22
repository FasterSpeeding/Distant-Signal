import { Group, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourney, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { AddJourneyLegButton } from '@/components/AddJourneyLegButton';
import { JourneyLegCard } from '@/components/JourneyLegCard';
import { JourneyStatusBadge } from '@/components/JourneyStatusBadge';
import { LastUpdated } from '@/components/LastUpdated';
import { LoginLink } from '@/components/LoginLink';
import { ShareJourneyButton } from '@/components/ShareJourneyButton';
import { formatDate } from '@/lib/dateFormat';
import { legDestinationArrivalLabel, legEndpointName } from '@/lib/journeyLegLabel';
import { routeLabel } from '@/lib/stationLabel';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

export const revalidate = 0;

/** 2026-09-22 UX review finding M18: the fallback `<h1>` for a journey
 * with no `customName` used to be the flat, generic "Tracked journey" --
 * the spec's own rename-the-journey pattern is noted as deferred in the
 * code, but until it lands the title should default to the route, the
 * same "route is the identity" fix I16/2.7 makes for each leg's own card
 * (09-17 review recommended the identical default for the single-train
 * page). Spans the WHOLE journey -- the first leg's own origin to the
 * last leg's own destination -- not just the first leg, so a multi-leg
 * journey's title still reads as one trip. Name resolution reuses
 * `legEndpointName` (see its own doc comment) so this never disagrees
 * with what each leg's own card/table already shows for the same
 * station. */
function defaultJourneyTitle(journey: JourneyDetail): string {
  const firstLeg = journey.legs[0];
  if (!firstLeg) return 'Tracked journey';
  const lastLeg = journey.legs.at(-1) ?? firstLeg;
  const originName = legEndpointName(
    firstLeg.originCrs,
    firstLeg.trackedTrainState?.journeyStops,
    firstLeg.trackedTrainState,
    'origin',
  );
  const destinationName = legEndpointName(
    lastLeg.destinationCrs,
    lastLeg.trackedTrainState?.journeyStops,
    lastLeg.trackedTrainState,
    'destination',
  );
  const route = routeLabel(firstLeg.originCrs, originName, lastLeg.destinationCrs, destinationName);
  return `${route}, ${formatDate(firstLeg.serviceDate)}`;
}

/** One short status phrase per leg, for the multi-leg rollup summary line
 * below (2026-09-22 UX review finding I12/2.6). Deliberately NOT built on
 * `lib/journeyStatus.ts::legStatusGroup` -- that function picks a single
 * WORST-status label per leg for ranking purposes (and, per that module's
 * own doc comment, is the one piece of this fix intentionally left
 * untouched here), where this wants the same plain facts
 * (`JourneyStatusBadge`'s own header badge already reduces the whole
 * journey to its single worst leg) restated per leg instead of collapsed
 * to one. "on time", not "on track" -- aligned with `JourneyTimeline.tsx`/
 * `TrainJourney.tsx`'s own existing per-stop/per-leg delay badges, the
 * same M22/2.11 terminology fix `JourneyStatusBadge.tsx`'s own label map
 * makes. */
function legSummaryPhrase(leg: JourneyLegDetail): string {
  if (leg.trackedTrainState === null) return 'needs a train picked';
  const state = leg.trackedTrainState;
  if (state.status === 'cancelled') return 'cancelled';
  if (state.status === 'awaiting_activation' || state.status === null) return 'awaiting first report';
  if (state.delayMinutes !== null && state.delayMinutes > 0) return `${state.delayMinutes}m late`;
  return 'on time';
}

/** The slim connector drawn between two consecutive legs (2026-09-22 UX
 * review finding I12/2.6: "the journey is not drawn as a chain"). Reads
 * the DEPARTING leg's own destination row -- never the underlying train's
 * terminus, which may run on past the traveller's own change point --
 * for both the station name and its best-known arrival time. Renders a
 * plain station name with no time when nothing better is known yet (an
 * unmatched or not-yet-reported leg), rather than hiding the connector
 * outright: even without a live buffer computation (explicitly deferred
 * to Phase 3 by the design doc), naming the change point at all is what
 * turns two stacked cards into one journey. */
function LegConnector({ leg }: { leg: JourneyLegDetail }) {
  const stops = leg.trackedTrainState?.journeyStops;
  const stationName =
    legEndpointName(leg.destinationCrs, stops, leg.trackedTrainState, 'destination') ?? leg.destinationCrs;
  if (!stationName) return null;
  const arrival = legDestinationArrivalLabel(leg.destinationCrs, stops);
  return (
    <Text size="xs" c="dimmed" pl="sm">
      ↓ Change at {stationName}
      {arrival ? ` — ${arrival}` : ''}
    </Text>
  );
}

/** `/journeys/[id]` -- design doc §4. One card per leg. No editable
 * header, no skip badge, no platform column -- all explicitly deferred,
 * see this plan's own Non-goals for the reasoning behind each. A
 * share-to-group button DOES exist (Task 8), but only for the journey's
 * owner (`journey.isOwner`) -- a non-owning group member reaches this
 * page via `journey_readable_by`'s group-shared read path and gets
 * neither that button nor the leg-level owner-only controls
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
  const multiLeg = journey.legs.length > 1;
  // `revalidate = 0` above means this page is a live, uncached fetch on
  // every request -- so "the instant this request's own `getJourney()`
  // call returned" genuinely IS when every fact on this page (the delay
  // figure, the last-reported location, every leg's status) was current
  // as of, not an invented number. 2026-09-22 UX review finding I24/2.12:
  // nothing on this page previously said when "22m late" was measured,
  // and a journey page is MORE time-critical than a single train page --
  // a stale leg-1 ETA silently invalidates a leg-2 pick.
  const fetchedAt = new Date().toISOString();

  return (
    <Stack p="lg" gap="md">
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
              produce a dead end. */}
          {journey.isOwner && (
            <AddJourneyLegButton journeyId={journey.id} priorDestinationCrs={priorDestinationCrs} />
          )}
          {journey.isOwner && <ShareJourneyButton journeyId={journey.id} />}
        </Group>
      </Group>
      {/* 2026-09-22 UX review finding I12/2.6: the header badge above shows
          only the single WORST leg's status -- `LEG_STATUS_RANK` puts
          `unmatched` above `delayed`, so "Needs a train picked" from one
          leg fully hid a real 22-minute delay on another, matched leg.
          This line restates every leg's own status so a delay is never
          invisible above the fold. Single-leg journeys skip this line --
          the header badge already says the whole story when there's only
          one leg to summarise. */}
      {multiLeg && (
        <Text size="sm" c="dimmed">
          {journey.legs.map((leg, index) => `Leg ${index + 1} ${legSummaryPhrase(leg)}`).join(' · ')}
        </Text>
      )}
      <LastUpdated timestamp={fetchedAt} label="Data as of" />
      {journey.legs.map((leg, index) => (
        // 2026-09-22 UX review finding I12/2.6: "the journey is not drawn
        // as a chain" -- two cards with a bare 16px gap and no
        // relationship between them. The "Leg N of M" label (multi-leg
        // journeys only -- a single-leg journey has nothing to number)
        // plus the connector after every leg but the last are what turn a
        // list of unrelated cards into one readable journey.
        // `id="leg-{id}"` (2026-09-22 UX review finding I15/2.3): the
        // scroll target `JourneyStatusBadge`'s own anchor points at when
        // the worst status is `unmatched` -- see that component's doc
        // comment.
        <Stack key={leg.id} id={`leg-${leg.id}`} gap={4}>
          {multiLeg && (
            <Text size="xs" fw={700} c="dimmed" tt="uppercase">
              Leg {index + 1} of {journey.legs.length}
            </Text>
          )}
          <JourneyLegCard
            journeyId={journey.id}
            leg={leg}
            isOwner={journey.isOwner}
            isOnlyLeg={journey.legs.length === 1}
          />
          {index < journey.legs.length - 1 && <LegConnector leg={leg} />}
        </Stack>
      ))}
    </Stack>
  );
}
