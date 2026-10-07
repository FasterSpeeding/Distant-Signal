'use client';

import { useEffect, useState } from 'react';
import { Alert, Autocomplete, Button, Stack, Text } from '@mantine/core';
import { LoadMoreControl } from './LoadMoreControl';
import { ServiceRow, ServiceRowList } from './ServiceRow';
import { TextLink } from './TextLink';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { searchRowDetails, searchRowSummary, searchRowTime } from '@/lib/searchRow';
import { serviceNoun } from '@/lib/serviceMode';
import { dayOffsetMarker } from '@/lib/serviceStatus';
import type { TrainSearchResult } from '@/lib/types';

// Mirrors `lib/useSuggestions.ts`'s own (non-exported) `DEBOUNCE_MS = 250`
// constant -- this is a SEPARATE debounce, for the committed filter value
// that drives the candidates re-fetch below, not the Operator field's own
// suggestion-dropdown debounce (which lives inside `useSuggestions` itself,
// applied to `operator` independently). Same 250ms, so a fast typist sees
// both the dropdown and the re-fetch settle together rather than one
// visibly lagging the other.
const FILTER_DEBOUNCE_MS = 250;

/** Wire shape of `GET /Journeys/{id}/legs/{id}/candidates` -- the
 * envelope `GET /public/trains/search` returns
 * (`crates/api/src/render.rs::calling_point_departure_json`) PLUS the four
 * leg-scoped fields `crates/api/src/routes/journeys.rs::leg_candidate_json`
 * adds on top of it (2026-09-22 UX review, C4).
 *
 * The distinction is the whole point. `originCrs`/`destinationCrs`/
 * `destinationArrival` describe the TRAIN's own route -- for a
 * York->Newcastle leg riding a London->Edinburgh service they say
 * "KGX -> EDB", which is why every candidate used to render identically as
 * "19:00 · KGX → EDB" and the one decision this list exists to support
 * could not be made from it. `stationCrs`/`legOriginCrs`,
 * `legDestinationCrs` and `legDestinationArrival` describe the
 * TRAVELLER's leg: where they get on, where they get off, and when. */
interface CandidateRow extends TrainSearchResult {
  /** Echoed back by the backend; identical to `stationCrs`. */
  legOriginCrs: string;
  legDestinationCrs: string;
  /** Arrival at the leg's own destination, `"HH:MM"`. `null` when the
   * schedule records neither an arrival nor a booked departure there --
   * the row then shows no arrival rather than a guessed one. */
  legDestinationArrival: string | null;
  /** How many days past the service date `legDestinationArrival` falls --
   * `0` for the overwhelming majority, non-zero for an overnight leg. */
  legDestinationArrivalDayOffset: number;
  /** The PUBLIC (passenger timetable) arrival at the leg's destination,
   * `"HH:MM"`, shown in place of `legDestinationArrival` (working
   * timetable) when present. Optional: absent from an older backend. The
   * departure's public time is `publicDeparture`. */
  legPublicDestinationArrival?: string | null;
}

interface CandidatesResponse {
  results: CandidateRow[];
  nextCursor: string | null;
}

/** Exactly one of three mutually-exclusive states, mirroring
 * `TrainSearchForm.tsx`'s own `Results` type for the same reason: `hasRows`
 * below narrows to the success variant, and `nextCursor`/`loadMoreFailed`
 * live INSIDE it so a fresh fetch (a `journeyId`/`legId` change) or an error
 * discard them automatically rather than needing a separate `useState` each
 * that could survive a state transition it doesn't belong to. */
type Results =
  { rows: CandidateRow[]; nextCursor: string | null; loadMoreFailed: boolean } | 'loading' | 'error' | null;

/** Narrows `Results` to the "has rows" branch -- see `TrainSearchForm.tsx`'s
 * identical helper. `handleLoadMore`'s early-return guard plus each of its
 * functional `setResults` updaters need this exact check. */
function hasRows(
  results: Results,
): results is { rows: CandidateRow[]; nextCursor: string | null; loadMoreFailed: boolean } {
  return results !== null && results !== 'loading' && results !== 'error';
}

/** The open-leg candidate list + pick action -- design doc §2.2/§2.3/§4.
 * `onPicked` is called after a successful commit; the caller (a
 * `JourneyLegCard`, `frontend/components/JourneyLegCard.tsx`) decides what
 * to do next (typically `router.refresh()`). Paginates with `nextCursor`
 * exactly like `TrainSearchForm.tsx`'s own "Load more" does, against the
 * same `after=`-cursor query param the backend route already accepts
 * (`crates/api/src/routes/journeys.rs::get_leg_candidates`'s
 * `CandidatesParams::after`) -- this component used to leave `nextCursor`
 * on the floor entirely, which on a high-frequency corridor with more than
 * one operator's services interleaved by departure time (EUS-MKC is the
 * reported case) silently dropped whichever operator's trains happened to
 * sort past the default 50-row page, with no indication more results
 * existed. `LoadMoreControl` is shared with `TrainSearchForm.tsx` and
 * `IncidentSearchForm.tsx`, so this list gets the same three-state footer
 * (button / end-of-results line / failed-with-retry) for free. */
export function JourneyLegCandidates({
  journeyId,
  legId,
  serviceDate,
  onPicked,
}: {
  journeyId: number;
  legId: number;
  serviceDate: string;
  onPicked: () => void;
}) {
  const [results, setResults] = useState<Results>(null);
  const [picking, setPicking] = useState<string | null>(null);
  const [pickError, setPickError] = useState<string | null>(null);
  // Separate from the initial fetch's own 'loading' state on purpose, same
  // as TrainSearchForm.tsx's `loadingMore`/`searching` split: a "Load more"
  // in flight must not blank the rows already on screen.
  const [loadingMore, setLoadingMore] = useState(false);
  // Bug fix: a lapsed session (401 from the candidates fetch) used to fall
  // straight into the generic 'error' branch below ("Couldn't load
  // candidate trains right now. Try again.") with no login prompt -- and
  // since the session had actually expired, "Try again" could never
  // succeed. Mirrors `PinToggle.tsx`'s established `useNeedsLogin()` /
  // `LoginPromptModal` shape: reset at the start of every fresh attempt,
  // set on a real 401, surfaced as an unconditionally-rendered modal below
  // rather than folded into `body()`'s own state machine.
  const needsLoginState = useNeedsLogin();

  // Operator/TOC filter -- design doc's journey-leg-operator-filter plan,
  // Task 6. `operator` is the raw typed text (drives the Autocomplete's own
  // value AND its `useSuggestions` suggestion dropdown below);
  // `committedOperator` is the DEBOUNCED, trimmed-and-uppercased value that
  // actually drives the candidates fetch, so a fast typist doesn't fire one
  // request per keystroke. Reuses `TrackTrainForm.tsx`'s window-mode
  // Operator field pattern verbatim for the input/suggestions wiring
  // (`useSuggestions` + `searchTocs` + `suggestionAutocompleteProps`) --
  // that field has no debounced-refetch of its own to mirror (it only
  // filters an already-fetched in-memory picker), so the debounce here is
  // new, specific to this component's server-side filtering.
  const [operator, setOperator] = useState('');
  const [committedOperator, setCommittedOperator] = useState('');
  const { suggestions: operatorSuggestions, loading: operatorSuggestionsLoading } = useSuggestions(
    operator,
    searchTocs,
  );

  // Debounces `operator` -> `committedOperator`, mirroring
  // `lib/useSuggestions.ts`'s own 250ms debounce (`FILTER_DEBOUNCE_MS`,
  // above) but kept separate from it: this fires the candidates re-fetch,
  // not a suggestions lookup. Uppercased here (not left to the caller) --
  // ATOC codes are always stored upper-case (CIF is upper-case ASCII), and
  // the backend's `operator_atoc = ANY($N)` match is a case-SENSITIVE plain
  // Postgres `TEXT` comparison, so a lower-case typed value would silently
  // match nothing. Matches `TrackTrainForm.tsx`'s own `matchesOperator`
  // precedent of normalizing with `.toUpperCase()` before comparing.
  useEffect(() => {
    const timer = setTimeout(() => {
      setCommittedOperator(operator.trim().toUpperCase());
    }, FILTER_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [operator]);

  useEffect(() => {
    let cancelled = false;
    // eslint-disable-next-line react-hooks/set-state-in-effect -- shows the loading state for the fetch this effect starts
    setResults('loading');
    needsLoginState.reset();
    const params = new URLSearchParams();
    if (committedOperator) params.set('operator', committedOperator);
    const query = params.toString();
    fetch(`/api/Journeys/${journeyId}/legs/${legId}/candidates${query ? `?${query}` : ''}`)
      // The catch below tells a 401 apart by rejecting with the Response itself.
      // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- see above
      .then((res) => (res.ok ? res.json() : Promise.reject(res)))
      .then((body: CandidatesResponse) => {
        if (!cancelled) setResults({ rows: body.results, nextCursor: body.nextCursor, loadMoreFailed: false });
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        // A lapsed session, specifically -- distinguished from every other
        // failure (a 500, a network error, ...) so only THIS case gets the
        // login prompt instead of the plain "try again" message that can
        // never succeed for it.
        if (err instanceof Response && err.status === 401) {
          needsLoginState.markNeedsLogin();
        }
        setResults('error');
      });
    return () => {
      cancelled = true;
    };
    // `committedOperator` joins `journeyId`/`legId` as a full reset trigger,
    // same as this plan's Task 6 brief requires: a changed filter drops any
    // existing rows/`nextCursor` and searches again from page 1, exactly
    // like a fresh `journeyId`/`legId` mount already does. `needsLoginState`
    // is deliberately excluded: it's a fresh object from `useNeedsLogin()`
    // every render (its `reset`/`markNeedsLogin` callbacks aren't
    // memoized), so listing it would re-run this fetch on every render
    // instead of only on a real prop/filter change.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- needsLoginState is a fresh object every render (see above)
  }, [journeyId, legId, committedOperator]);

  // Mirrors `TrainSearchForm.tsx`'s own `handleLoadMore` almost verbatim:
  // the `pagedFrom` identity check guards against a page 2 response landing
  // after `journeyId`/`legId` has already changed underneath it (this
  // component's fetch effect above has no analogue of a "fresh search"
  // button, but a caller can still re-mount this with new props while a
  // "Load more" is in flight), and a failed page keeps its cursor rather
  // than nulling it, so the footer's retry is a real one.
  async function handleLoadMore() {
    if (!hasRows(results)) return;
    if (results.nextCursor === null || loadingMore) return;
    const pagedFrom = results;
    setLoadingMore(true);
    try {
      const params = new URLSearchParams({ after: results.nextCursor });
      if (committedOperator) params.set('operator', committedOperator);
      const response = await fetch(`/api/Journeys/${journeyId}/legs/${legId}/candidates?${params.toString()}`);
      if (!response.ok) {
        // Same 401-specific handling as the initial fetch above -- a
        // session that lapsed between page 1 and "Load more" gets the login
        // prompt too, not just a retry button that can never succeed.
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        }
        setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
        return;
      }
      const body = (await response.json()) as CandidatesResponse;
      setResults((current) =>
        current === pagedFrom
          ? { rows: [...current.rows, ...body.results], nextCursor: body.nextCursor, loadMoreFailed: false }
          : current,
      );
    } catch {
      setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
    } finally {
      setLoadingMore(false);
    }
  }

  async function pick(uid: string) {
    setPicking(uid);
    setPickError(null);
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/legs/${legId}/train`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ trainUid: uid, serviceDate }),
      });
      if (!response.ok) {
        setPickError("Couldn't track that train. Try again.");
        return;
      }
      onPicked();
    } catch {
      setPickError("Couldn't track that train. Try again.");
    } finally {
      setPicking(null);
    }
  }

  // The body below this filter -- one of the same six mutually-exclusive
  // states `hasRows`/`Results` already model, just factored out of the
  // top-level return so the Operator filter (below) can wrap ALL of them
  // uniformly, rather than being duplicated into every early-return branch.
  function body() {
    if (results === null || results === 'loading') {
      return (
        <Text size="sm" c="dimmed">
          Searching for candidate trains…
        </Text>
      );
    }
    if (results === 'error') {
      // A lapsed session gets its own message -- "Try again" is honest
      // advice for a transient failure, but actively misleading for a 401,
      // since retrying with the same expired session can never succeed.
      // The actual login prompt is the `LoginPromptModal` rendered
      // unconditionally below, same as `PinToggle.tsx`.
      return (
        <Alert color="red" title="Search failed">
          {needsLoginState.needsLogin
            ? 'Your session has expired. Log in again to see candidate trains.'
            : "Couldn't load candidate trains right now. Try again."}
        </Alert>
      );
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          {committedOperator
            ? `No scheduled trains from operator ${committedOperator} match this window.`
            : 'No scheduled trains match this window.'}{' '}
          <TextLink href="/track" inline underline="always">
            Search manually
          </TextLink>{' '}
          instead.
        </Text>
      );
    }
    return (
      <>
        {/* Review §2.5/M15: the intro line this list used to inherit from its
            caller ("Searching for a train to track — pick one below") was
            near-identical to the loading state above, for the opposite
            situation — an unbounded "still working" tone on a list that has
            already finished and is just waiting to be picked from. Also
            answers "what happens when I click", which used to be invisible
            until after the (irreversible-looking) click: a picked leg can
            still be changed later via "Change train" (`JourneyLegCard.tsx`).
            Counts rows LOADED so far, same as the count would read on a
            single-page result before pagination existed -- on a window with
            more than one page this undercounts the true total until "Load
            more" is pressed, which reads as "at least this many", never as a
            wrong number, the same posture `TrainSearchForm.tsx` takes by
            simply not stating a total at all. */}
        <Text size="sm" c="dimmed">
          {results.rows.length} train{results.rows.length === 1 ? '' : 's'}{' '}
          {results.rows.length === 1 ? 'matches' : 'match'} your search — pick the one you&apos;ll be on. You can change
          it later.
        </Text>
        <Text size="sm" c="dimmed">
          Times are when each train leaves the station you board at and reaches the one you get off at.
        </Text>
        {pickError && <Alert color="red">{pickError}</Alert>}
        <ServiceRowList aria-label="Candidate trains">
          {results.rows.map((row) => (
            <CandidateRowView
              key={`${row.uid}-${row.scheduled ?? ''}`}
              row={row}
              serviceDate={serviceDate}
              picking={picking}
              onPick={() => pick(row.uid)}
            />
          ))}
        </ServiceRowList>
        <LoadMoreControl
          hasMore={results.nextCursor !== null}
          loading={loadingMore}
          failed={results.loadMoreFailed}
          onLoadMore={handleLoadMore}
          endMessage="You've reached the end — no more candidate trains match this window."
        />
      </>
    );
  }

  return (
    <Stack gap="xs">
      {/* Deliberately rendered BEFORE the loading/error/empty-state checks
          inside `body()`, not after them like a typical filter control that
          only makes sense once there's a list to filter: a traveller
          filtering by operator on a busy corridor likely wants to set it
          before the first page even loads, rather than watch an unfiltered
          page 1 render and then re-fetch a moment later. It stays mounted,
          visible and interactive through every one of `body()`'s states
          (including 'loading' and 'error'), which is also what lets the
          debounced `committedOperator` effect above keep working even while
          `body()` is showing something other than the rows list. */}
      <Autocomplete
        label="Operator (optional)"
        placeholder="e.g. SW"
        value={operator}
        onChange={setOperator}
        {...suggestionAutocompleteProps(operatorSuggestions, {
          query: operator,
          loading: operatorSuggestionsLoading,
          noMatchMessage: 'No matching operators',
        })}
      />
      {body()}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to see candidate trains for this leg.
      </LoginPromptModal>
    </Stack>
  );
}

/** The leg's arrival at the traveller's destination, `HH:MM`: the public
 * time, else the working one; `undefined` -- not "?", and never the
 * train's terminus arrival -- when the schedule has no time there. */
function legArrival(row: CandidateRow): string | undefined {
  return (row.legPublicDestinationArrival ?? row.legDestinationArrival)?.slice(0, 5);
}

/** One candidate as a `ServiceRow`: the leg's own departure (from the
 * station the traveller boards at) as the row's time, the train's
 * destination, the leg's arrival at the traveller's own destination (with
 * a "+1" for an overnight leg), live status, and the train's identity and
 * origin as the dimmed second line (2026-09-22 UX review, C4). The
 * destination links to the train's page; the pick button sits in the
 * row's actions slot, beside the link rather than inside it.
 *
 * The button commits the pick to a journey LEG (`POST
 * /Journeys/{id}/legs/{id}/train`), not a standalone subscription like
 * `/trains`' `TrackThisTrainButton`, so the row's action stays this
 * component's own.
 *
 * `aria-label` on the button: three or four buttons all named "Track this
 * train" is N indistinguishable items in a screen reader's control list
 * (I26/P3, WCAG 2.4.9). The label CONTAINS the visible text, so
 * Label-in-Name (2.5.3) still holds. */
function CandidateRowView({
  row,
  serviceDate,
  picking,
  onPick,
}: {
  row: CandidateRow;
  serviceDate: string;
  picking: string | null;
  onPick: () => void;
}) {
  const train = searchRowSummary(row);
  const time = searchRowTime(row);
  const arrival = legArrival(row);
  const nextDay = dayOffsetMarker(row.legDestinationArrivalDayOffset);
  const destination = train.destination?.name ?? train.destination?.crs;
  const spoken = [
    destination ? `${time} to ${destination}` : time,
    arrival && `arriving ${arrival}${nextDay ? ` (${nextDay.spoken})` : ''}`,
  ]
    .filter(Boolean)
    .join(', ');
  const noun = serviceNoun(train.serviceMode).toLowerCase();
  const details = [`${serviceNoun(train.serviceMode)} ${row.uid}`, searchRowDetails(row), row.operator]
    .filter(Boolean)
    .join(' · ');
  return (
    <ServiceRow
      train={train}
      date={serviceDate}
      timeOverride={time}
      dayOffset={row.dayOffset}
      arrival={arrival}
      arrivalDayOffset={row.legDestinationArrivalDayOffset}
      details={details}
      actions={
        <Button
          size="xs"
          loading={picking === row.uid}
          disabled={picking !== null}
          onClick={onPick}
          aria-label={`Track this ${noun} — ${spoken}`}
        >
          Track this {noun}
        </Button>
      }
    />
  );
}
