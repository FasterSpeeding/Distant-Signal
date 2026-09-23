'use client';

import { useState } from 'react';
import { Alert, Button, Stack, Text } from '@mantine/core';
import { PlanTripForm } from './PlanTripForm';
import { ItineraryOption } from './ItineraryOption';
import { fetchTripPlan, TripPlanError, type TripPlanQuery } from '@/lib/tripPlan';
import type { CreateJourneyResponse, TripPlanItinerary, TripPlanResponse } from '@/lib/types';

interface SegmentSelection {
  itinerary: TripPlanItinerary | null;
}

/** "Plan a route for me" (design spec §5.3): the form (Task 3) → results
 * comparison (Task 4, per segment) → the multi-leg creation sequence
 * (this task) → hands off to `JourneyCreationFlow`'s existing
 * `handleLegOneCreated`-driven view via `onCreated`, called EXACTLY ONCE
 * after this component's own creation sequence finishes -- see this
 * plan's own Judgment Call 2 for why the sequence lives here, not spread
 * across `JourneyCreationFlow`'s incremental "Add a leg" flow. */
export function PlanTripFlow({ onCreated }: { onCreated: (result: CreateJourneyResponse) => void }) {
  const [plan, setPlan] = useState<TripPlanResponse | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [selections, setSelections] = useState<SegmentSelection[]>([]);
  const [creating, setCreating] = useState(false);
  const [creationError, setCreationError] = useState<string | null>(null);

  async function handleSearch(query: TripPlanQuery) {
    setPlanError(null);
    setPlan(null);
    try {
      const result = await fetchTripPlan(query);
      setPlan(result);
      setSelections(result.segments.map(() => ({ itinerary: null })));
    } catch (error) {
      setPlanError(error instanceof TripPlanError ? error.message : 'Could not plan this trip. Please try again.');
    }
  }

  function selectItinerary(segmentIndex: number, itinerary: TripPlanItinerary) {
    setSelections(current => current.map((selection, i) => (i === segmentIndex ? { itinerary } : selection)));
  }

  const allSegmentsSelected =
    plan !== null && selections.length === plan.segments.length && selections.every(s => s.itinerary !== null);

  async function handleTrackJourney() {
    if (!allSegmentsSelected) return;
    setCreating(true);
    setCreationError(null);

    // Every TRAIN leg across every selected segment, in order -- a
    // TransferLeg never becomes a journey_legs row (this plan's own
    // Judgment Call 3).
    const trainLegs = selections.flatMap(selection =>
      (selection.itinerary?.legs ?? []).filter((leg): leg is Extract<typeof leg, { kind: 'train' }> => leg.kind === 'train')
    );

    if (trainLegs.length === 0) {
      setCreationError('This route needs no train — there is nothing to track.');
      setCreating(false);
      return;
    }

    try {
      const firstLeg = trainLegs[0];
      // Known, pre-existing backend limitation (not introduced or fixed by
      // this task -- see `KnownTrain` in `CreateJourneyLegRequest`,
      // `crates/api/src/routes/journeys.rs`, and
      // `crates/api/src/data/journeys.rs`'s `create_subscription_for_train`/
      // `add_known_train_leg_to_journey`): a `knownTrain`-mode leg request
      // only ever carries `trainUid`/`serviceDate`, never an origin/
      // destination of its own. The backend derives the committed leg's
      // `origin_crs`/`destination_crs` from the matched train's OWN full
      // schedule (`trains.origin_crs`/`destination_crs` -- the whole
      // working's start/end), not from wherever this traveller actually
      // boards or alights. `firstLeg.originCrs`/`firstLeg.destinationCrs`
      // (from `GET /Trips/plan`) are this leg's real boarding/alighting
      // points and can legitimately differ -- e.g. boarding a
      // London->Edinburgh service at York and alighting at Newcastle -- but
      // there is no field on this request to send them, so they are
      // silently discarded here. The train UID + service date (the only
      // fields that matter for live delay/cancellation tracking) ARE
      // committed correctly; only display/station-skip-detection metadata
      // on the resulting `journey_legs` row can end up describing the
      // train's full route instead of this leg's own. Same gap
      // `AddJourneyLegButton.tsx`'s existing `knownTrain` submission has
      // always had -- not new or specific to trip planning, and out of
      // scope for this task to fix (would require changing the
      // `KnownTrain` request shape itself).
      const createResponse = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: firstLeg.trainUid, serviceDate: firstLeg.serviceDate },
        }),
      });
      if (!createResponse.ok) {
        throw new Error(await createResponse.text());
      }
      const created: CreateJourneyResponse = await createResponse.json();

      for (let i = 1; i < trainLegs.length; i += 1) {
        const leg = trainLegs[i];
        // Same pre-existing `knownTrain` CRS-derivation gap as the initial
        // `POST /Journeys` call above -- `leg.originCrs`/`leg.destinationCrs`
        // are this subsequent leg's own real boarding/alighting points, but
        // `POST /Journeys/{id}/legs` (`AddJourneyLegRequest::KnownTrain`)
        // has nowhere to accept them either, so the committed row's
        // origin/destination will again reflect this train's own full
        // route rather than this leg's.
        const addResponse = await fetch(`/api/Journeys/${created.journeyId}/legs`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ mode: 'knownTrain', trainUid: leg.trainUid, serviceDate: leg.serviceDate }),
        });
        if (!addResponse.ok) {
          // Partial success: leg 1..i already exist. Tell the visitor
          // exactly which leg failed, then still hand off -- the journey
          // that DOES exist must not strand them (this plan's own Review
          // Focus, matching JourneyCreationFlow's own established
          // refetch-failure posture).
          setCreationError(
            `Tracked ${i} of ${trainLegs.length} legs. Adding leg ${i + 1} failed: ${await addResponse.text()}. ` +
              'You can add it manually from the journey page.'
          );
          onCreated(created);
          return;
        }
      }

      onCreated(created);
    } catch (error) {
      setCreationError(error instanceof Error ? error.message : 'Could not create this journey. Please try again.');
    } finally {
      setCreating(false);
    }
  }

  return (
    <Stack gap="md">
      <PlanTripForm onSubmit={handleSearch} />
      {planError && (
        <Alert color="red" title="Couldn't plan this trip">
          {planError}
        </Alert>
      )}
      {plan &&
        plan.segments.map((segment, segmentIndex) => (
          <Stack key={segmentIndex} gap="xs">
            <Text fw={600}>
              {segment.originCrs} → {segment.destinationCrs}
            </Text>
            {segment.itineraries.length === 0 && (
              <Alert color="yellow">No route found for {segment.originCrs} → {segment.destinationCrs}.</Alert>
            )}
            {segment.cappedByMaxChanges && (
              <Text size="xs" c="orange">
                A faster route exists with more changes than shown below.
              </Text>
            )}
            {segment.itineraries.map((itinerary, itineraryIndex) => (
              <ItineraryOption
                key={itineraryIndex}
                itinerary={itinerary}
                selected={selections[segmentIndex]?.itinerary === itinerary}
                onSelect={() => selectItinerary(segmentIndex, itinerary)}
              />
            ))}
          </Stack>
        ))}
      {creationError && (
        <Alert color="red" title="Some legs could not be created">
          {creationError}
        </Alert>
      )}
      {plan && (
        <Button disabled={!allSegmentsSelected || creating} loading={creating} onClick={() => void handleTrackJourney()}>
          Track this journey
        </Button>
      )}
    </Stack>
  );
}
