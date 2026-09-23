'use client';

import { useRef, useState } from 'react';
import { Alert, Button, Stack, Text } from '@mantine/core';
import { PlanTripForm } from './PlanTripForm';
import { ItineraryOption } from './ItineraryOption';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
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
 * across `JourneyCreationFlow`'s incremental "Add a leg" flow.
 *
 * Final-review fix (C1): a FULL success calls `onCreated` immediately --
 * there's nothing left for the visitor to read first. A PARTIAL failure
 * (leg 1..i committed, leg i+1 didn't) instead stores the already-created
 * journey in `pendingResult` and renders a "Continue to your journey"
 * button rather than calling `onCreated` right away. `onCreated` is
 * `JourneyCreationFlow`'s `handleLegOneCreated`, which flips `journeyId`
 * from `null` to a real id in the SAME state-update batch that would have
 * set `creationError` here -- calling it immediately unmounted this
 * component (and its about-to-render error Alert) before the visitor
 * could ever see which leg failed, landing them instead on
 * `JourneyCreationFlow`'s single-leg "success" view with legs 2+ silently
 * gone. Deferring the call until the visitor acknowledges the message
 * keeps `onCreated` called exactly once overall, per Judgment Call 2 --
 * just later. */
export function PlanTripFlow({ onCreated }: { onCreated: (result: CreateJourneyResponse) => void }) {
  const [plan, setPlan] = useState<TripPlanResponse | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [selections, setSelections] = useState<SegmentSelection[]>([]);
  const [creating, setCreating] = useState(false);
  const [creationError, setCreationError] = useState<string | null>(null);
  const [pendingResult, setPendingResult] = useState<CreateJourneyResponse | null>(null);
  const [searching, setSearching] = useState(false);
  const needsLoginState = useNeedsLogin();
  // Monotonic guard against a stale `GET /Trips/plan` response landing
  // after a newer search was already issued (I2) -- `GET /Trips/plan` can
  // take several seconds (a full day of schedule connections, run through
  // pathfinding), and nothing here hard-blocks re-issuing `handleSearch`
  // while an earlier one is still in flight. Each call captures its own
  // id; a response is only applied if it's still the most recent one by
  // the time it resolves, so an older, slower response can never clobber
  // a newer one that happened to finish first.
  const searchRequestId = useRef(0);

  async function handleSearch(query: TripPlanQuery) {
    const requestId = (searchRequestId.current += 1);
    setSearching(true);
    setPlanError(null);
    setPlan(null);
    try {
      const result = await fetchTripPlan(query);
      if (searchRequestId.current !== requestId) return; // superseded by a newer search
      setPlan(result);
      setSelections(result.segments.map(() => ({ itinerary: null })));
    } catch (error) {
      if (searchRequestId.current !== requestId) return;
      setPlanError(error instanceof TripPlanError ? error.message : 'Could not plan this trip. Please try again.');
    } finally {
      if (searchRequestId.current === requestId) setSearching(false);
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
    needsLoginState.reset();

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
      // I1: `GET /Trips/plan` is deliberately unauthenticated, so an
      // anonymous visitor can plan a full route with no friction -- but
      // `POST /Journeys` requires a session, and returns a plain-text
      // `401`/"no session" a raw-error Alert would render verbatim and
      // confusingly. Mirrors `TrackTrainForm.tsx`'s `submitTrack`/
      // `submitWindow` -- detect the 401 before the generic `!ok` branch
      // below and show the same login-prompt UI they already use instead.
      if (createResponse.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
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
        //
        // This inner try/catch (not just an `if (!addResponse.ok)` check)
        // is load-bearing: `fetch` itself can reject -- a genuine network
        // exception (a dropped connection, an aborted request), distinct
        // from an HTTP error status, which resolves normally -- and by the
        // time we're in this loop `created` already names a REAL journey
        // that exists server-side (leg 1 already succeeded). Funnelling
        // both failure shapes into the same partial-failure branch below
        // means a network blip here gets the exact same "tell them which
        // leg failed, still hand off the journey that DOES exist" treatment
        // an HTTP error already got -- review finding: the previous version
        // let a thrown exception here escape to the OUTER catch, which
        // reports a generic failure and never calls `onCreated`, stranding
        // the visitor even though their journey (with leg 1 already
        // tracked) is sitting right there.
        try {
          const addResponse = await fetch(`/api/Journeys/${created.journeyId}/legs`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ mode: 'knownTrain', trainUid: leg.trainUid, serviceDate: leg.serviceDate }),
          });
          // I1: a session can in principle expire mid-sequence too -- same
          // 401 handling as the initial `POST /Journeys` call above, but
          // leg 1..i already exist server-side by this point, so this
          // still tells the visitor which leg failed (same partial-failure
          // treatment as any other add-leg failure) AND prompts login,
          // rather than losing that context behind a login-only message.
          if (addResponse.status === 401) {
            needsLoginState.markNeedsLogin();
            setCreationError(
              `Tracked ${i} of ${trainLegs.length} legs. Your session expired before leg ${i + 1} could be added. ` +
                'Log in, then add it manually from the journey page.'
            );
            setPendingResult(created);
            return;
          }
          if (!addResponse.ok) {
            throw new Error(await addResponse.text());
          }
        } catch (legError) {
          // Partial success: leg 1..i already exist. Tell the visitor
          // exactly which leg failed, then still hand off -- the journey
          // that DOES exist must not strand them (this plan's own Review
          // Focus, matching JourneyCreationFlow's own established
          // refetch-failure posture). Covers both an HTTP-error response
          // (the `throw` above, whose `.message` is the backend's own
          // plain-text body) and a genuine network exception (whose
          // `.message` is the browser's own, e.g. "Failed to fetch").
          const reason = legError instanceof Error ? legError.message : 'a network error';
          setCreationError(
            `Tracked ${i} of ${trainLegs.length} legs. Adding leg ${i + 1} failed: ${reason}. ` +
              'You can add it manually from the journey page.'
          );
          // C1: do NOT call `onCreated` here -- see this component's own
          // doc comment. Store the already-created journey so the
          // "Continue to your journey" button (rendered below once
          // `creationError`/`pendingResult` are both set) can hand off
          // once the visitor has actually seen which leg failed.
          setPendingResult(created);
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
      <PlanTripForm onSubmit={handleSearch} searching={searching} />
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
      {/* C1: once a partial failure has happened, `pendingResult` holds
          the journey that DOES already exist server-side. This button is
          the visitor's own deliberate acknowledgement of the message
          above -- `onCreated` fires only when they click it, never
          automatically, so it's never called in the same tick as (and
          before) the error message could render. Replaces "Track this
          journey" entirely rather than sitting alongside it: re-running
          `handleTrackJourney` from here would re-POST leg 1 as a second,
          duplicate journey. */}
      {pendingResult ? (
        <Button onClick={() => onCreated(pendingResult)}>Continue to your journey</Button>
      ) : (
        plan && (
          <Button disabled={!allSegmentsSelected || creating} loading={creating} onClick={() => void handleTrackJourney()}>
            Track this journey
          </Button>
        )
      )}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this journey.
      </LoginPromptModal>
    </Stack>
  );
}
