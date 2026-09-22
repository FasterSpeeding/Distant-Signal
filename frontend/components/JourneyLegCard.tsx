'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Card, Group, Stack, Text } from '@mantine/core';
import { TrainJourney } from './TrainJourney';
import { JourneyLegCandidates } from './JourneyLegCandidates';
import { TextLink } from './TextLink';
import { formatDate } from '@/lib/dateFormat';
import { routeLabel, stationLabel } from '@/lib/stationLabel';
import type { JourneyLegDetail } from '@/lib/types';

/** `"HH:MM:SS"` (the wire shape of a persisted `NaiveTime` bound) →
 * `"HH:MM"`, matching every other displayed time in this app
 * (`lib/dateFormat.ts`'s `formatTime`) rather than leaking the seconds
 * component into copy nobody asked for. */
function formatBoundTime(value: string): string {
  return value.slice(0, 5);
}

/** "Departing York after 09:00, arriving Newcastle before 12:00." — the one
 * thing review §2.5/I18 found genuinely missing from the open-leg card: the
 * search criteria the user just typed, restated so they can tell why a
 * train they expected is absent (or spot a typo) without leaving the page.
 * `null` when the leg has no persisted window at all (shouldn't happen for
 * an open leg in practice — see this component's own doc comment — but
 * every `depart*`/`arrive*` field is nullable on the wire, so this stays
 * defensive rather than assuming). */
function windowCriteriaSummary(leg: JourneyLegDetail): string | null {
  const departClauses: string[] = [];
  if (leg.departAfter) departClauses.push(`after ${formatBoundTime(leg.departAfter)}`);
  if (leg.departBefore) departClauses.push(`before ${formatBoundTime(leg.departBefore)}`);
  const arriveClauses: string[] = [];
  if (leg.arriveAfter) arriveClauses.push(`after ${formatBoundTime(leg.arriveAfter)}`);
  if (leg.arriveBefore) arriveClauses.push(`before ${formatBoundTime(leg.arriveBefore)}`);
  if (departClauses.length === 0 && arriveClauses.length === 0) return null;

  const originName = leg.originCrs ? stationLabel(leg.originCrs, leg.originName) : 'the origin';
  const destinationName = leg.destinationCrs
    ? stationLabel(leg.destinationCrs, leg.destinationName)
    : 'the destination';
  const parts: string[] = [];
  if (departClauses.length > 0) parts.push(`departing ${originName} ${departClauses.join(' and ')}`);
  if (arriveClauses.length > 0) parts.push(`arriving ${destinationName} ${arriveClauses.join(' and ')}`);
  return `Looking for trains ${parts.join(', ')}.`;
}

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
 *   design doc's own 2026-09-22 addendum. */
export function JourneyLegCard({
  journeyId,
  leg,
  isOwner,
}: {
  journeyId: number;
  leg: JourneyLegDetail;
  isOwner: boolean;
}) {
  const router = useRouter();
  const [changingTrain, setChangingTrain] = useState(false);
  const hasWindow =
    leg.departAfter !== null ||
    leg.departBefore !== null ||
    leg.arriveAfter !== null ||
    leg.arriveBefore !== null;

  if (leg.trackedTrainState === null) {
    // Review §2.5/I27: use the same `routeLabel`/`formatDate` this leg's
    // OWN matched-state sibling (`TrainJourney`) and `/track/mine` already
    // render with, instead of interpolating raw CRS codes and an ISO
    // `serviceDate` straight into the header.
    const header = `${routeLabel(leg.originCrs, leg.originName, leg.destinationCrs, leg.destinationName)}, ${formatDate(leg.serviceDate)}`;
    const criteria = windowCriteriaSummary(leg);
    return (
      <Card withBorder>
        <Stack gap="sm">
          <Text fw={500}>{header}</Text>
          {/* Review §2.5/I18: the window the user just entered was never
              shown back to them -- the only place it lived was inside
              `hasWindow`'s own boolean check above, never rendered. */}
          {criteria && (
            <Group justify="space-between" wrap="wrap" gap="xs">
              <Text size="sm" c="dimmed">
                {criteria}
              </Text>
              {isOwner && (
                // Review §2.1/I21: `/track` now accepts `?mode=window`, so
                // this finally has somewhere honest to link to -- prefills
                // the origin the same way the station-page "Track a train
                // from here" link already does. Doesn't restore the
                // destination/date/times too (no query-param contract for
                // those yet); a real re-search, not a form round-trip.
                <TextLink href={`/track?mode=window&origin=${encodeURIComponent(leg.originCrs ?? '')}`}>
                  Edit search
                </TextLink>
              )}
            </Group>
          )}
          {isOwner ? (
            <JourneyLegCandidates
              journeyId={journeyId}
              legId={leg.id}
              serviceDate={leg.serviceDate}
              onPicked={() => router.refresh()}
            />
          ) : (
            <Text size="sm" c="dimmed">
              Waiting for the owner to pick a train.
            </Text>
          )}
        </Stack>
      </Card>
    );
  }

  // Leg-scoped skip signal (§5.2) -- at most the leg's own origin/destination
  // CRS, sourced from Task 4's `legSkip` wire field. `TrainJourney`/
  // `JourneyTimeline` treat `undefined` and `[]` identically, but this is
  // always a concrete (possibly empty) array here since `leg.legSkip` is
  // only `null` when there's nothing to report.
  const skippedCrs = [
    leg.legSkip?.originSkipped ? leg.originCrs : null,
    leg.legSkip?.destinationSkipped ? leg.destinationCrs : null,
  ].filter((crs): crs is string => crs !== null);

  return (
    <Card withBorder>
      <Stack gap="sm">
        <TrainJourney state={leg.trackedTrainState} skippedCrs={skippedCrs} />
        {/* Two independent conditions, both required: `hasWindow` is the
            Phase 2 "this leg is still an open window, so a train can be
            picked for it" test, and `isOwner` is Phase 4's sharing gate --
            a shared-group viewer sees the leg but must not be offered a
            "Change train" action the API would reject anyway. */}
        {hasWindow && isOwner && (
          <>
            <Button size="xs" variant="default" onClick={() => setChangingTrain((c) => !c)}>
              {changingTrain ? 'Cancel' : 'Change train'}
            </Button>
            {changingTrain && (
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
          </>
        )}
      </Stack>
    </Card>
  );
}
