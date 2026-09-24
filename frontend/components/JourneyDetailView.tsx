import { Group, Stack, Text, Title } from '@mantine/core';
import { AddJourneyLegButton } from './AddJourneyLegButton';
import { DeleteJourneyButton } from './DeleteJourneyButton';
import { JourneyLegCard } from './JourneyLegCard';
import { JourneyStatusBadge } from './JourneyStatusBadge';
import { LastUpdated } from './LastUpdated';
import { SaveAsTemplateButton } from './SaveAsTemplateButton';
import { ShareJourneyButton } from './ShareJourneyButton';
import { ShareJourneyLinkButton } from './ShareJourneyLinkButton';
import { TrackJourneyAgainButton } from './TrackJourneyAgainButton';
import { formatDate } from '@/lib/dateFormat';
import { journeyCanAddLeg, journeyPriorDestinationCrs } from '@/lib/journeyLegChaining';
import { legDestinationArrivalLabel, legEndpointName } from '@/lib/journeyLegLabel';
import { routeLabel } from '@/lib/stationLabel';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

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
 * station, falling back to the leg's own server-resolved
 * `originName`/`destinationName` (`stations` reference lookup) -- which
 * is the ONLY name available for a journey whose first leg is still
 * open, since an unmatched leg has neither `journeyStops` nor a pin for
 * `legEndpointName` to read. Bare CRS codes remain the last resort;
 * never a fabricated name. */
function defaultJourneyTitle(journey: JourneyDetail): string {
  const firstLeg = journey.legs[0];
  if (!firstLeg) return 'Tracked journey';
  const lastLeg = journey.legs.at(-1) ?? firstLeg;
  const originName =
    legEndpointName(
      firstLeg.originCrs,
      firstLeg.trackedTrainState?.journeyStops,
      firstLeg.trackedTrainState,
      'origin',
    ) ?? firstLeg.originName;
  const destinationName =
    legEndpointName(
      lastLeg.destinationCrs,
      lastLeg.trackedTrainState?.journeyStops,
      lastLeg.trackedTrainState,
      'destination',
    ) ?? lastLeg.destinationName;
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

/** The reusable body of `/journeys/[id]` -- extracted verbatim (2026-09-23
 * unlisted-links plan, Task 5) from `app/journeys/[id]/page.tsx` so a
 * later `/journeys/shared/[token]` page (Task 6, same plan) can render the
 * identical journey detail view for an anonymous share-link viewer, minus
 * the page-specific "Back to my trains & journeys" link -- there is
 * nothing for an anonymous viewer to go back to, so that link stays
 * page-specific and is NOT part of this component. Every owner-only
 * control below (`AddJourneyLegButton`, `ShareJourneyButton`,
 * `ShareJourneyLinkButton`, `SaveAsTemplateButton`) stays gated on
 * `journey.isOwner`, same as before this extraction -- a share-link
 * viewer is never the owner, so those simply won't render there.
 *
 * Renders a Fragment, not its own `Stack`, so it stays a transparent set
 * of direct children of whichever `<Stack gap="md">` the caller wraps it
 * in (both current callers use `p="lg" gap="md"`, matching this view's
 * former in-page position exactly) -- the caller's flex `gap` therefore
 * still applies between every one of this view's top-level elements
 * exactly as it did before the extraction. */
export function JourneyDetailView({
  journey,
  fetchedAt,
  origin,
}: {
  journey: JourneyDetail;
  fetchedAt: string;
  origin: string;
}) {
  // Review §2.5/M16: "Add a leg" used to be offered at the same visual
  // weight as the status badge even while the CURRENT leg still needs a
  // train picked -- there is nothing to chain a new leg onto yet, and it
  // competed for attention with the one action that actually matters on
  // this page. Hidden until the last leg is matched. Both helpers now live
  // in `lib/journeyLegChaining.ts`, shared with `JourneyCreationFlow.tsx`'s
  // own inline "Add a leg" during initial creation -- see that module's own
  // doc comment for why this moved out of being private to this page.
  const priorDestinationCrs = journeyPriorDestinationCrs(journey);
  const canAddLeg = journeyCanAddLeg(journey);
  const multiLeg = journey.legs.length > 1;

  return (
    <>
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
          {journey.isOwner && (
            <ShareJourneyLinkButton journeyId={journey.id} shareLink={journey.shareLink} origin={origin} />
          )}
          {journey.isOwner && <SaveAsTemplateButton journeyId={journey.id} />}
          {/* Feature request: "journeys should be mutable ... you should
              be able to ... delete parts or even the whole journey."
              Owner-only, same reasoning as every other action above --
              `DELETE /Journeys/{id}` 404s a non-owner identically to a
              nonexistent journey, so offering this to a shared-group
              viewer (or an anonymous share-link viewer, now that this
              view is shared between both) would only be a dead end.
              Placed last among the owner-only controls, immediately
              before the always-visible `TrackJourneyAgainButton`, so the
              one destructive action on this page sits at the end of the
              owner-only run rather than between two non-destructive
              ones. */}
          {journey.isOwner && <DeleteJourneyButton journeyId={journey.id} />}
          {/* Deliberately NOT gated on journey.isOwner -- see
              docs/superpowers/plans/2026-09-22-reusable-journeys-phaseA-track-again-plan.md's
              Judgment Call 7. Placed last so the owner-only controls
              above stay visually adjacent to each other. */}
          <TrackJourneyAgainButton journey={journey} />
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
    </>
  );
}
