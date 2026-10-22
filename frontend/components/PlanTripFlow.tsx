'use client';

import { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { Alert, Button, List, ListItem, Stack, Text } from '@mantine/core';
import { PlanTripForm } from './PlanTripForm';
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
import { apiQuery, timingSummary } from '@/lib/tripPlanTiming';
import { listPlan, mergePage, pageQuery, startList, type PageDirection, type PlanList } from '@/lib/tripPlanPaging';
import { captureScrollAnchor, restoreScrollAnchor, type ScrollAnchor } from '@/lib/scrollAnchor';
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

/** At most this many requests per "Earlier routes" or "Later routes"
 * click: a page whose journeys are all already shown (a delay that grew
 * between two requests) is followed by the next one, so a click always
 * adds something unless the day has run out. */
const MAX_REQUESTS_PER_PAGE = 3;

/** The attribute each itinerary card's wrapper carries for scroll
 * anchoring: `<segment>:<itinerary key>`. */
const ENTRY_ATTRIBUTE = 'data-plan-entry';

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
  // The visitor's search's own response; `list` is it with every earlier
  // and later page merged in (`lib/tripPlanPaging.ts`), `null` when it has
  // no journey to page from. The results show `list` when there is one.
  const [searchedPlan, setSearchedPlan] = useState<TripPlanResponse | null>(null);
  const [list, setList] = useState<PlanList | null>(null);
  const plan = useMemo(() => (list ? listPlan(list) : searchedPlan), [list, searchedPlan]);
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
  // The search the shown plan answers, for its summary line.
  const [answeredQuery, setAnsweredQuery] = useState<TripPlanQuery | null>(null);
  // "Earlier routes" / "Later routes" in flight, and its failure.
  const [paging, setPaging] = useState<PageDirection | null>(null);
  const [pagingError, setPagingError] = useState<{ direction: PageDirection; message: string } | null>(null);
  const resultsRef = useRef<HTMLDivElement>(null);
  const pendingAnchor = useRef<ScrollAnchor | null>(null);
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

  async function handleSearch(query: TripPlanQuery) {
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
    setSearchedPlan(null);
    setList(null);
    setAnsweredQuery(null);
    setPagingError(null);
    // Clears out the PREVIOUS search's resolved names immediately (not
    // just once the new lookup resolves) -- otherwise a code the new plan
    // never mentions could keep rendering the old plan's name for the
    // brief window before `getStationNames` below resolves.
    setStationNames(new Map());
    try {
      const sent = apiQuery(query);
      const result = await fetchTripPlan(sent);
      if (searchRequestId.current !== requestId) return; // superseded by a newer search
      setSearchedPlan(result);
      setList(startList(query, sent.date, result));
      setAnsweredQuery(query);
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

  // Earlier/later (spec 2026-10-19 §4.3): one page at a time, merged into
  // the list -- earlier above, later below -- so the whole span stays on
  // screen. Each page repeats the visitor's own search with only the time
  // changed (`pageQuery`).
  async function loadPage(direction: PageDirection) {
    if (!list || paging || searching) return;
    // A new search from the form while this runs wins: its id moves on.
    const requestId = searchRequestId.current;
    setPaging(direction);
    setPagingError(null);
    let current = list;
    const pages: TripPlanResponse[] = [];
    try {
      for (let request = 0; request < MAX_REQUESTS_PER_PAGE; request += 1) {
        const sent = apiQuery(pageQuery(current, direction));
        const result = await fetchTripPlan(sent);
        if (searchRequestId.current !== requestId) return;
        pages.push(result);
        const merged = mergePage(current, direction, result, sent.date);
        current = merged.list;
        if (merged.added > 0 || current.earlierDone || current.laterDone) break;
      }
      // Hold the card on screen still while earlier ones go in above it.
      if (direction === 'earlier') pendingAnchor.current = captureScrollAnchor(resultsRef.current, ENTRY_ATTRIBUTE);
      setList(current);
      void Promise.all(pages.map((page) => getStationNames(collectTripPlanStationCodes(page)))).then((found) => {
        if (searchRequestId.current !== requestId) return;
        setStationNames((names) => new Map([...names, ...found.flatMap((more) => [...more])]));
      });
    } catch (error) {
      if (searchRequestId.current !== requestId) return;
      setPagingError({
        direction,
        message:
          error instanceof TripPlanError
            ? error.message
            : describeFailure('load', direction === 'earlier' ? 'earlier routes' : 'later routes'),
      });
    } finally {
      setPaging(null);
    }
  }

  useLayoutEffect(() => {
    restoreScrollAnchor(resultsRef.current, pendingAnchor.current);
    pendingAnchor.current = null;
  }, [list]);

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
      {plan && answeredQuery && (
        <Text fw={600} data-testid="plan-timing-summary">
          {timingSummary(answeredQuery)}
        </Text>
      )}
      {plan && <ProvisionalTimetableNote provisional={plan.provisional} />}
      {list && (
        <PageControl
          direction="earlier"
          done={list.earlierDone}
          error={pagingError?.direction === 'earlier' ? pagingError.message : null}
          loading={paging === 'earlier'}
          disabled={paging !== null || searching}
          onLoad={() => void loadPage('earlier')}
        />
      )}
      {/* `ds-no-scroll-anchor`: the scroll position is held by
          `restoreScrollAnchor` alone, never twice. Only with a plan: an
          empty Stack still takes the outer Stack's gap, which put 16px
          under the form before any search. */}
      {plan && (
        <Stack gap="md" ref={resultsRef} className="ds-no-scroll-anchor">
          {plan.segments.map((segment, segmentIndex) => {
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
                  <div
                    key={list?.entries[itineraryIndex]?.key ?? itineraryIndex}
                    {...{
                      [ENTRY_ATTRIBUTE]: `${segmentIndex}:${list?.entries[itineraryIndex]?.key ?? itineraryIndex}`,
                    }}
                  >
                    <ItineraryOption
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
                  </div>
                ))}
              </Stack>
            );
          })}
        </Stack>
      )}
      {list && (
        <PageControl
          direction="later"
          done={list.laterDone}
          error={pagingError?.direction === 'later' ? pagingError.message : null}
          loading={paging === 'later'}
          disabled={paging !== null || searching}
          onLoad={() => void loadPage('later')}
        />
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

/** "Earlier routes" above the results or "Later routes" below them, or,
 * once a page in that direction came back empty, a line saying so. */
function PageControl({
  direction,
  done,
  error,
  loading,
  disabled,
  onLoad,
}: {
  direction: PageDirection;
  done: boolean;
  error: string | null;
  loading: boolean;
  disabled: boolean;
  onLoad: () => void;
}) {
  if (done) {
    return (
      <Text size="sm" c="dimmed">
        {direction === 'earlier' ? 'No earlier journeys on this day.' : 'No later journeys on this day.'}
      </Text>
    );
  }
  return (
    <Stack gap="xs" align="flex-start">
      {error && (
        <Alert color="red" w="100%">
          {error}
        </Alert>
      )}
      <Button variant="default" loading={loading} disabled={disabled} onClick={onLoad}>
        {direction === 'earlier' ? 'Earlier routes' : 'Later routes'}
      </Button>
    </Stack>
  );
}
