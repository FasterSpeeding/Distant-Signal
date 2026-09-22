'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Card, Stack, Text } from '@mantine/core';
import { TrainJourney } from './TrainJourney';
import { JourneyLegCandidates } from './JourneyLegCandidates';
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
    return (
      <Card withBorder>
        <Stack gap="sm">
          <Text fw={500}>
            {leg.originCrs ?? '?'} → {leg.destinationCrs ?? '?'}, {leg.serviceDate}
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

  return (
    <Card withBorder>
      <Stack gap="sm">
        <TrainJourney state={leg.trackedTrainState} />
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
