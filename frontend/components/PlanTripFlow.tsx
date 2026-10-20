'use client';

import { useMemo, useRef, useState } from 'react';
import { Alert, Button, Group, List, ListItem, Stack, Text } from '@mantine/core';
import { PlanTripForm, type PlanTripFormHandle } from './PlanTripForm';
import { ItineraryOption } from './ItineraryOption';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import {
  collectTripPlanStationCodes,
  fetchTripPlan,
  isTooLargeSearchError,
  noResultHint,
  trainLegsForTracking,
  TripPlanError,
  type TripPlanQuery,
} from '@/lib/tripPlan';
import { planPageSearch, type PlanFormInitial } from '@/lib/tripPlanUrl';
import { apiQuery, earlierQuery, laterQuery, timingMode, timingSummary, type TimingMode } from '@/lib/tripPlanTiming';
import { getStationNames } from '@/lib/suggestions';
import { itineraryLegsForSaving } from '@/lib/tripPlanSave';
import { groupLabels, useStationGroups } from '@/lib/stationGroups';
import { codeRouteLabel, codeStationLabel, isGroupCode } from '@/lib/stationLabel';
import { ProvisionalTimetableNote } from './ProvisionalTimetableNote';
import type { CreateJourneyResponse, TrainSearchDates, TripPlanItinerary, TripPlanResponse } from '@/lib/types';
import { describeFailure, failureFromResponse } from '@/lib/failure';

/** A failure already worded for the visitor (`describeFailure` or the API's own sentence). */
class JourneyCreationError extends Error {}

/** A fresh `Idempotency-Key` for one "Track this journey" click. */
function idempotencyKey(): string {
  return typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

interface SegmentSelection {
  itinerary: TripPlanItinerary | null;
}

/** `form` with `paged`'s date and time (and so its mode). */
function withPagedTiming(form: TripPlanQuery, paged: TripPlanQuery): TripPlanQuery {
  const { departAfter: _depart, arriveBy: _arrive, dayOffset: _offset, acrossMidnight: _across, ...rest } = form;
  return {
    ...rest,
    date: paged.date,
    ...(paged.arriveBy ? { arriveBy: paged.arriveBy } : { departAfter: paged.departAfter }),
  };
}

/** A plan with the visitor's search that produced it, and the service date
 * that search was sent as (`apiQuery`: an arrive-by time after midnight is
 * the previous day's trains). */
interface AnsweredSearch {
  query: TripPlanQuery;
  serviceDate: string;
  plan: TripPlanResponse;
}

/** "Plan a route for me" (design spec §5.3): the form (Task 3) → results
 * comparison (Task 4, per segment) → one `POST /Journeys/itinerary` for
 * every train leg of the chosen itineraries → hands off to
 * `JourneyCreationFlow`'s `handleLegOneCreated`-driven view via
 * `onCreated`, called exactly once, after the journey exists.
 *
 * The save is atomic (2026-10-14, decision D-15): the journey and all its
 * legs are created in one transaction, so either everything is tracked or
 * nothing is. This replaced a `POST /Journeys` followed by one
 * `POST /Journeys/{id}/legs` per later leg, whose partial failures needed
 * a "Continue to your journey" hand-off. */
export function PlanTripFlow({
  onCreated,
  initialOriginCrs,
  initialQuery,
  syncUrl = false,
  searchDates,
}: {
  onCreated: (result: CreateJourneyResponse) => void;
  /** Pre-fills the From field (`/plan?from=CRS`). */
  initialOriginCrs?: string | undefined;
  /** A search restored from `/plan`'s query string (`lib/tripPlanUrl.ts`). */
  initialQuery?: PlanFormInitial | undefined;
  /** `/plan` only: write each search into the address bar
   * (`history.replaceState`, no navigation), so it can be shared and
   * reopens the same form. Off on `/journeys/new`, whose query means
   * something else. */
  syncUrl?: boolean;
  /** The date picker's range -- see `PlanTripForm`. */
  searchDates?: TrainSearchDates | null | undefined;
}) {
  const [plan, setPlan] = useState<TripPlanResponse | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  // The options-mode search-size 400: shown as advice, not the raw message
  // (which is the formula the API checks).
  const [tooLarge, setTooLarge] = useState(false);
  // CRS -> full name, resolved client-side once `plan` loads -- unlike
  // every OTHER station-bearing response in this app, `GET /Trips/plan`
  // never sends a `*Name` sibling field of its own (see
  // `lib/tripPlan.ts`'s `collectTripPlanStationCodes` doc comment), so
  // this component resolves every code the plan mentions itself
  // (`lib/suggestions.ts`'s `getStationNames`) and hands the result to
  // both the segment/no-route summaries below and each `ItineraryOption`.
  // A code with no resolved name is simply absent from the map --
  // `codeRouteLabel` already falls back to the bare code for that case,
  // same "degraded lookup, not broken display" contract as every other
  // station label in this app.
  const [resolvedNames, setStationNames] = useState<Map<string, string>>(new Map());
  // Plus the station groups' labels: a segment may start or end at a
  // group "Call at" stop (`group:LON`, 2026-10-07).
  const groups = useStationGroups(
    plan !== null &&
      [...plan.segments.flatMap((segment) => [segment.originCrs, segment.destinationCrs]), ...(plan.via ?? [])].some(
        isGroupCode,
      ),
  );
  const stationNames = useMemo(() => new Map([...groupLabels(groups), ...resolvedNames]), [groups, resolvedNames]);
  const [selections, setSelections] = useState<SegmentSelection[]>([]);
  const [creating, setCreating] = useState(false);
  const [creationError, setCreationError] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  // The search the shown plan answers (for its summary line), and the last
  // one that found a journey: earlier/later page from the plan on screen,
  // or from this one when the page on screen is empty.
  const [answered, setAnswered] = useState<AnsweredSearch | null>(null);
  const [pagingAnchor, setPagingAnchor] = useState<AnsweredSearch | null>(null);
  // A paged search's timing, pushed into the form's fields.
  const [formTiming, setFormTiming] = useState<
    { date: string; mode: TimingMode; time: string; seq: number } | undefined
  >(undefined);
  const needsLoginState = useNeedsLogin();
  // Monotonic guard against a stale `GET /Trips/plan` response landing
  // after a newer search was already issued (I2) -- `GET /Trips/plan` can
  // take several seconds (a full day of schedule connections, run through
  // pathfinding). Each call captures its own id; a response is only
  // applied if it's still the most recent one by the time it resolves, so
  // an older, slower response can never clobber a newer one that happened
  // to finish first. Kept as defense-in-depth even now that `handleSearch`
  // itself (below) and `PlanTripForm`'s own `disabled` (see its `searching`
  // doc comment) both refuse to START a second search while one is already
  // in flight: this is what still protects data correctness if either of
  // those guards is ever bypassed (e.g. a future caller invoking
  // `handleSearch` directly), exactly the layered-guard posture
  // `TrainSearchForm.tsx`'s `handleLoadMore` already uses (its own
  // `loadingMore` in-flight check PLUS its `pagedFrom` stale-response
  // identity check, together, not either alone).
  const searchRequestId = useRef(0);

  async function handleSearch(query: TripPlanQuery, paged = false) {
    // Whole-branch final-review finding: data correctness was already
    // covered by the `searchRequestId` guard below (a stale response can
    // never overwrite a newer one), but nothing stopped a second click
    // while a search was already in flight from firing another real
    // `GET /Trips/plan` -- wasted backend pathfinding work on every extra
    // click, not a correctness bug, but real, redundant work all the same.
    // `PlanTripForm`'s submit button is now also disabled on `searching`
    // (see its own doc comment), which is what makes a rapid re-click a
    // no-op in practice; this early return mirrors `TrainSearchForm.tsx`'s
    // `handleLoadMore` guarding itself on `loadingMore` in addition to
    // `LoadMoreControl`'s own `disabled`/`loading` prop, so the no-op holds
    // even if `handleSearch` is ever reachable some other way than that
    // one button.
    if (searching) return;
    const requestId = (searchRequestId.current += 1);
    setSearching(true);
    setPlanError(null);
    setTooLarge(false);
    if (syncUrl) {
      window.history.replaceState(null, '', `${window.location.pathname}?${planPageSearch(query)}`);
    }
    setPlan(null);
    setAnswered(null);
    // A new search from the form starts paging afresh.
    if (!paged) setPagingAnchor(null);
    // Clears out the PREVIOUS search's resolved names immediately (not
    // just once the new lookup resolves) -- otherwise a code the new plan
    // never mentions could keep rendering the old plan's name for the
    // brief window before `getStationNames` below resolves.
    setStationNames(new Map());
    try {
      const sent = apiQuery(query);
      const result = await fetchTripPlan(sent);
      if (searchRequestId.current !== requestId) return; // superseded by a newer search
      setPlan(result);
      const search = { query, serviceDate: sent.date, plan: result };
      setAnswered(search);
      if ((result.journeys ?? []).length > 0) setPagingAnchor(search);
      setSelections(result.segments.map(() => ({ itinerary: null })));
      // Best-effort, non-blocking: the plan itself is already fully
      // renderable with bare codes (`codeRouteLabel`'s own fallback), so
      // this resolves in the background rather than delaying `searching`
      // going back to `false` -- a slow/failed name lookup must never
      // hold up the actual route results.
      void getStationNames(collectTripPlanStationCodes(result)).then((names) => {
        if (searchRequestId.current === requestId) setStationNames(names);
      });
    } catch (error) {
      if (searchRequestId.current !== requestId) return;
      if (error instanceof TripPlanError && isTooLargeSearchError(error)) {
        setTooLarge(true);
        return;
      }
      setPlanError(error instanceof TripPlanError ? error.message : describeFailure('plan', 'this trip'));
    } finally {
      if (searchRequestId.current === requestId) setSearching(false);
    }
  }

  function selectItinerary(segmentIndex: number, itinerary: TripPlanItinerary) {
    // With waypoints, the segments' itineraries are parts of whole
    // journeys (`plan.journeys`): choosing one part chooses its journey.
    const index = plan?.segments[segmentIndex]?.itineraries.indexOf(itinerary) ?? -1;
    if (plan && plan.segments.length > 1 && plan.journeys && index >= 0 && index < plan.journeys.length) {
      setSelections(plan.segments.map((segment) => ({ itinerary: segment.itineraries[index] ?? null })));
      return;
    }
    setSelections((current) => current.map((selection, i) => (i === segmentIndex ? { itinerary } : selection)));
  }

  const allSegmentsSelected =
    plan !== null && selections.length === plan.segments.length && selections.every((s) => s.itinerary !== null);

  async function handleTrackJourney() {
    if (!allSegmentsSelected) return;
    setCreating(true);
    setCreationError(null);
    needsLoginState.reset();

    // Every leg across every selected segment, in order: each train leg,
    // and each Tube, walk, bus or ferry transfer between them as an
    // untracked leg with its planned times (spec 2026-10-20 §2, follow-up;
    // until then a transfer was dropped). The itinerary still needs a train.
    const chosen = selections.flatMap((selection) => (selection.itinerary ? [selection.itinerary] : []));
    const trainLegs = trainLegsForTracking(chosen);
    const legsToSave = itineraryLegsForSaving(chosen, (code) => stationNames.get(code));

    if (trainLegs.length === 0) {
      setCreationError('This route needs no train, so there is nothing to track.');
      setCreating(false);
      return;
    }

    try {
      // One request for the whole itinerary (`POST /Journeys/itinerary`,
      // decision D-15 of the 2026-10-14 delegated-access spec): the
      // journey and every train leg are created in one transaction, so a
      // failure can no longer leave a journey with only some of its legs.
      //
      // Each train leg's `originCrs`/`destinationCrs` (from `GET /Trips/plan`)
      // are its real boarding and alighting points, which can differ from
      // the matched train's own full route (boarding a Birmingham to
      // Glasgow service at Crewe and leaving it at Preston). A `null` end,
      // or a bus stop's `tiploc:` end (no CRS a journey leg can store),
      // means "no override", so the key is omitted rather than sent as
      // `null`.
      //
      // The Idempotency-Key makes a retried request (a double click, a
      // dropped response) return the journey the first one made instead
      // of a second copy.
      const response = await fetch('/api/Journeys/itinerary', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json', 'Idempotency-Key': idempotencyKey() },
        body: JSON.stringify({ legs: legsToSave }),
      });
      // I1: `GET /Trips/plan` is deliberately unauthenticated, so an
      // anonymous visitor can plan a full route with no friction, but
      // saving needs a session, and the plain-text 401 would read badly in
      // an Alert. Show the login prompt instead.
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (!response.ok) {
        throw new JourneyCreationError(await failureFromResponse('create', 'this journey', response));
      }
      onCreated((await response.json()) as CreateJourneyResponse);
    } catch (error) {
      setCreationError(
        error instanceof JourneyCreationError ? error.message : describeFailure('create', 'this journey'),
      );
    } finally {
      setCreating(false);
    }
  }

  // Earlier/later: one request each, from the plan on screen, or from the
  // last one with a journey when this page is empty (spec §4.3).
  const pagingFrom = answered && (answered.plan.journeys ?? []).length > 0 ? answered : pagingAnchor;
  const earlier = pagingFrom ? earlierQuery(pagingFrom.query, pagingFrom.serviceDate, pagingFrom.plan) : null;
  const later = pagingFrom ? laterQuery(pagingFrom.query, pagingFrom.serviceDate, pagingFrom.plan) : null;

  const formHandle = useRef<PlanTripFormHandle>(null);

  function page(paged: TripPlanQuery) {
    if (searching) return;
    // The time comes from the results on screen; everything else from the
    // form as it is now, so a From or To changed since that search is the
    // one paged (and re-run), not the stale one.
    const query = withPagedTiming(formHandle.current?.currentQuery() ?? paged, paged);
    setFormTiming((current) => ({
      date: query.date,
      mode: timingMode(query),
      time: query.arriveBy ?? query.departAfter ?? '',
      seq: (current?.seq ?? 0) + 1,
    }));
    void handleSearch(query, true);
  }

  const hintFor = (reason: Parameters<typeof noResultHint>[0]) =>
    noResultHint(reason, (code) => codeStationLabel(code, stationNames.get(code)));

  return (
    <Stack gap="md">
      <PlanTripForm
        onSubmit={handleSearch}
        searching={searching}
        initialOriginCrs={initialOriginCrs}
        initial={initialQuery}
        searchDates={searchDates}
        timing={formTiming}
        handleRef={formHandle}
      />
      {planError && (
        <Alert color="red" title="Couldn't plan this trip">
          {planError}
        </Alert>
      )}
      {tooLarge && (
        <Alert color="red" title="Couldn't plan this trip: the search is too large">
          <Text size="sm">
            Comparing options over this many stops, vias and changes is more than the planner can do at once. Try one of
            these:
          </Text>
          <List size="sm" mt={4}>
            <ListItem>Switch Results to Fastest, which has no such limit.</ListItem>
            <ListItem>Remove some stops to call at, or some &quot;Pass through&quot; stations.</ListItem>
            <ListItem>Lower &quot;Most changes&quot; in Advanced options.</ListItem>
          </List>
        </Alert>
      )}
      {plan && answered && (
        <Text fw={600} data-testid="plan-timing-summary">
          {timingSummary(answered.query)}
        </Text>
      )}
      {plan && <ProvisionalTimetableNote provisional={plan.provisional} />}
      {plan?.segments.map((segment, segmentIndex) => {
        // Computed once per segment -- reused for both the heading and
        // the "no route found" alert below, so the two can never drift
        // out of sync with each other's formatting.
        const segmentLabel = codeRouteLabel(
          segment.originCrs,
          stationNames.get(segment.originCrs),
          segment.destinationCrs,
          stationNames.get(segment.destinationCrs),
        );
        return (
          <Stack key={segmentIndex} gap="xs">
            <Text fw={600}>{segmentLabel}</Text>
            {segment.itineraries.length === 0 && (
              <Alert color="yellow">
                No route found for {segmentLabel}.
                {segment.noResultReason && <Text size="sm">{segment.noResultReason.message}</Text>}
                {segment.noResultReason && hintFor(segment.noResultReason) && (
                  <Text size="sm" mt={4}>
                    {hintFor(segment.noResultReason)}
                  </Text>
                )}
              </Alert>
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
                stationNames={stationNames}
                viaPasses={plan.journeys?.[itineraryIndex]?.viaSatisfiedBy?.filter(
                  (via) => via.segment === segmentIndex,
                )}
                waypointStops={plan.journeys?.[itineraryIndex]?.waypointSatisfiedBy?.filter(
                  (stop) => stop.segment === segmentIndex,
                )}
                groups={groups}
              />
            ))}
          </Stack>
        );
      })}
      {plan && (earlier ?? later) && (
        <Group gap="sm">
          {earlier && (
            <Button variant="default" disabled={searching} onClick={() => page(earlier)}>
              Earlier routes
            </Button>
          )}
          {later && (
            <Button variant="default" disabled={searching} onClick={() => page(later)}>
              Later routes
            </Button>
          )}
        </Group>
      )}
      {creationError && (
        <Alert color="red" title="Couldn't track this journey">
          {creationError}
        </Alert>
      )}
      {plan && (
        <Button
          disabled={!allSegmentsSelected || creating}
          loading={creating}
          onClick={() => void handleTrackJourney()}
        >
          Track this journey
        </Button>
      )}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this journey.
      </LoginPromptModal>
    </Stack>
  );
}
