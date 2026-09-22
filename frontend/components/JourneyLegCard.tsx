'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Card, Group, Stack, Text } from '@mantine/core';
import { TrainJourney } from './TrainJourney';
import { JourneyLegCandidates } from './JourneyLegCandidates';
import { RemoveJourneyLegButton } from './RemoveJourneyLegButton';
import { legRouteAndTime } from '@/lib/journeyLegLabel';
import { formatDate } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import type { JourneyLegDetail } from '@/lib/types';

/** One leg's card on `/journeys/[id]` (design doc §4). Two branches:
 *
 * - **Open** (`trackedTrainState === null`): the search parameters plus,
 *   for the journey's OWNER only, a `JourneyLegCandidates` picker -- a
 *   non-owning group member (reachable here via `journey_readable_by`'s
 *   group-shared read path, see this plan's I1 final-review finding) sees
 *   a plain "waiting for the owner" message instead, since picking a train
 *   for someone else's leg is a 404 the backend correctly refuses.
 * - **Matched**: `TrainJourney` (reused unmodified, per the design doc's
 *   own explicit direction) plus, ONLY when the leg has a persisted window
 *   (`departAfter`/`departBefore`/`arriveAfter`/`arriveBefore` -- any
 *   non-null) AND the viewer is the journey's owner, a "Change train"
 *   toggle that reveals the SAME `JourneyLegCandidates` component,
 *   re-scoped to this leg (its own persisted window drives what the
 *   backend searches -- see
 *   `crates/api/src/routes/journeys.rs::get_leg_candidates`). A leg with
 *   no window (a `pin`/`knownTrain`-mode leg) has no window to re-search
 *   and so gets no "Change train" action -- swapping it means
 *   delete-and-recreate, same as today's single-train tracking, per the
 *   design doc's own 2026-09-22 addendum. It instead gets a "Remove leg"
 *   action (2026-09-22 UX review finding I14/2.4) so a wrong pick is at
 *   least recoverable, backed by `DELETE
 *   /Journeys/{journeyId}/legs/{legId}` (`crates/api/src/routes/journeys.rs::delete_journey_leg`).
 *
 * Both the "Change train" toggle and "Remove leg" live in the card's OWN
 * title row, top-right -- 2026-09-22 UX review finding I14/2.4's own
 * recommendation: "actions live at the top-right of the thing they act
 * on", the same rule `page.tsx`'s header already applies to "Add a leg".
 * The previous placement (a plain `xs` button below the whole six-row
 * timetable) had it as the very last thing on the card and zero visual
 * weight. */
export function JourneyLegCard({
  journeyId,
  leg,
  isOwner,
  isOnlyLeg,
}: {
  journeyId: number;
  leg: JourneyLegDetail;
  isOwner: boolean;
  /** Whether this is the journey's only remaining leg -- threaded from
   * `page.tsx`'s own `journey.legs.length`, and passed straight through to
   * `RemoveJourneyLegButton` so it can warn (and, on success, redirect
   * instead of refresh) when removing this leg also removes the whole
   * journey. See that component's own doc comment. */
  isOnlyLeg: boolean;
}) {
  const router = useRouter();
  const [changingTrain, setChangingTrain] = useState(false);
  const hasWindow =
    leg.departAfter !== null ||
    leg.departBefore !== null ||
    leg.arriveAfter !== null ||
    leg.arriveBefore !== null;

  if (leg.trackedTrainState === null) {
    // 2026-09-22 UX review finding 2.9: the matched card's title reads
    // "London Kings Cross (KGX) → York (YRK), 22 Sept 2026"
    // (`legRouteAndTime`/`formatDate`); this used to interpolate
    // `leg.serviceDate` raw ("YRK → NCL, 2026-09-22") -- two date formats
    // and two naming conventions on the same page. No `journeyStops` or
    // pin exists yet for an open leg, so `routeLabel` still falls back to
    // bare CRS codes here -- an honest degradation, not fabricated names
    // -- but the DATE format and the "Name (CODE)" convention now match
    // the matched card everywhere they can.
    const route = routeLabel(leg.originCrs, null, leg.destinationCrs, null);
    return (
      <Card
        withBorder
        // 2026-09-22 UX review finding I15/2.3: "the action-required
        // state is visually the quietest thing on the page" -- a plain
        // white card with a plain-weight title was indistinguishable from
        // inert content. A left accent border + a tinted surface + a
        // bold title are the three non-colour-alone cues the review
        // asked for (an icon would be a fourth, but `@tabler/icons-react`
        // isn't a project dependency -- see `InfoIcon.tsx`'s own doc
        // comment -- and three cues already clear WCAG 1.4.1).
        style={{ borderLeft: '4px solid var(--mantine-color-blue-6)' }}
        bg="var(--mantine-color-blue-light)"
      >
        <Stack gap="sm">
          <Text fw={700}>
            Pick a train — {route}, {formatDate(leg.serviceDate)}
          </Text>
          {isOwner ? (
            <>
              <Text size="sm" c="dimmed">
                Searching for a train to track — pick one below.
              </Text>
              <JourneyLegCandidates
                journeyId={journeyId}
                legId={leg.id}
                serviceDate={leg.serviceDate}
                onPicked={() => router.refresh()}
              />
            </>
          ) : (
            <Text size="sm" c="dimmed">
              Waiting for the owner to pick a train.
            </Text>
          )}
        </Stack>
      </Card>
    );
  }

  const state = leg.trackedTrainState;

  // Leg-scoped skip signal (§5.2) -- at most the leg's own origin/destination
  // CRS, sourced from Task 4's `legSkip` wire field. `TrainJourney`/
  // `JourneyTimeline` treat `undefined` and `[]` identically, but this is
  // always a concrete (possibly empty) array here since `leg.legSkip` is
  // only `null` when there's nothing to report.
  const skippedCrs = [
    leg.legSkip?.originSkipped ? leg.originCrs : null,
    leg.legSkip?.destinationSkipped ? leg.destinationCrs : null,
  ].filter((crs): crs is string => crs !== null);

  const title = legRouteAndTime(leg.originCrs, leg.destinationCrs, state.journeyStops, state);

  return (
    <Card withBorder>
      <Stack gap="sm">
        <Group justify="space-between" align="flex-start" wrap="wrap" gap="xs">
          <Stack gap={2}>
            <Text fw={600} size="lg">
              {title}
            </Text>
            {state.trainUid && (
              <Text size="xs" c="dimmed">
                Train {state.trainUid}
              </Text>
            )}
          </Stack>
          {/* Two independent conditions, both required: `hasWindow` is the
              Phase 2 "this leg is still an open window, so a train can be
              picked for it" test, and `isOwner` is Phase 4's sharing gate --
              a shared-group viewer sees the leg but must not be offered a
              "Change train"/"Remove leg" action the API would reject
              anyway. */}
          {isOwner && (
            <Group gap="xs">
              {hasWindow ? (
                <Button size="xs" variant="default" onClick={() => setChangingTrain((c) => !c)}>
                  {changingTrain ? 'Cancel' : 'Change train'}
                </Button>
              ) : (
                <RemoveJourneyLegButton journeyId={journeyId} legId={leg.id} isOnlyLeg={isOnlyLeg} />
              )}
            </Group>
          )}
        </Group>
        <TrainJourney
          state={state}
          skippedCrs={skippedCrs}
          legDestinationCrs={leg.destinationCrs}
          suppressTrainUidHeading
        />
        {hasWindow && isOwner && changingTrain && (
          <JourneyLegCandidates
            journeyId={journeyId}
            legId={leg.id}
            serviceDate={leg.serviceDate}
            onPicked={() => {
              setChangingTrain(false);
              router.refresh();
            }}
          />
        )}
      </Stack>
    </Card>
  );
}
