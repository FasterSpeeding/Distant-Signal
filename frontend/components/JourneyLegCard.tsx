'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Card, Stack, Text } from '@mantine/core';
import { TrainJourney } from './TrainJourney';
import { JourneyLegCandidates } from './JourneyLegCandidates';
import type { JourneyLegDetail } from '@/lib/types';

/** One leg's card on `/journeys/[id]` (design doc §4). Two branches:
 *
 * - **Open** (`trackedTrainState === null`): the search parameters plus an
 *   unconditional `JourneyLegCandidates`.
 * - **Matched**: `TrainJourney` (reused unmodified, per the design doc's
 *   own explicit direction) plus, ONLY when the leg has a persisted window
 *   (`departAfter`/`departBefore`/`arriveAfter`/`arriveBefore` -- any
 *   non-null), a "Change train" toggle that reveals the SAME
 *   `JourneyLegCandidates` component, re-scoped to this leg (its own
 *   persisted window drives what the backend searches -- see
 *   `crates/api/src/routes/journeys.rs::get_leg_candidates`). A leg with
 *   no window (a `pin`/`knownTrain`-mode leg) has no window to re-search
 *   and so gets no "Change train" action -- swapping it means
 *   delete-and-recreate, same as today's single-train tracking, per the
 *   design doc's own 2026-09-22 addendum. */
export function JourneyLegCard({ journeyId, leg }: { journeyId: number; leg: JourneyLegDetail }) {
  const router = useRouter();
  const [changingTrain, setChangingTrain] = useState(false);
  const hasWindow =
    leg.departAfter !== null ||
    leg.departBefore !== null ||
    leg.arriveAfter !== null ||
    leg.arriveBefore !== null;

  if (leg.trackedTrainState === null) {
    return (
      <Card withBorder>
        <Stack gap="sm">
          <Text fw={500}>
            {leg.originCrs ?? '?'} → {leg.destinationCrs ?? '?'}, {leg.serviceDate}
          </Text>
          <Text size="sm" c="dimmed">
            Searching for a train to track — pick one below.
          </Text>
          <JourneyLegCandidates
            journeyId={journeyId}
            legId={leg.id}
            serviceDate={leg.serviceDate}
            onPicked={() => router.refresh()}
          />
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
        {hasWindow && (
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
