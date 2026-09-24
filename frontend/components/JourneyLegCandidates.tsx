'use client';

import { useEffect, useState } from 'react';
import { Alert, Autocomplete, Button, Group, Stack, Text } from '@mantine/core';
import { LoadMoreControl } from './LoadMoreControl';
import { StatusRow } from './StatusRow';
import { TextLink } from './TextLink';
import { searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';

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
interface CandidateRow {
  uid: string;
  /** The departure time at the leg's own ORIGIN -- already leg-scoped,
   * because the backend search keys its main row on that station. */
  scheduled: string;
  destinationCrs: string | null;
  originCrs: string | null;
  destinationArrival: string | null;
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
  /** The train's operating ATOC code (e.g. `"SW"`), added to
   * `render::calling_point_departure_json`'s `"operator"` key by the
   * 2026-09-24 journey-leg-operator-filter plan. `null` for a CIF-sourced
   * row with no known operator, same "unknown means null, not a guess"
   * posture as `destinationCrs`/`originCrs` above. */
  operator: string | null;
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
  | { rows: CandidateRow[]; nextCursor: string | null; loadMoreFailed: boolean }
  | 'loading'
  | 'error'
  | null;

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
    setResults('loading');
    const params = new URLSearchParams();
    if (committedOperator) params.set('operator', committedOperator);
    const query = params.toString();
    fetch(`/api/Journeys/${journeyId}/legs/${legId}/candidates${query ? `?${query}` : ''}`)
      .then((res) => (res.ok ? res.json() : Promise.reject(res)))
      .then((body: CandidatesResponse) => {
        if (!cancelled) setResults({ rows: body.results, nextCursor: body.nextCursor, loadMoreFailed: false });
      })
      .catch(() => {
        if (!cancelled) setResults('error');
      });
    return () => {
      cancelled = true;
    };
    // `committedOperator` joins `journeyId`/`legId` as a full reset trigger,
    // same as this plan's Task 6 brief requires: a changed filter drops any
    // existing rows/`nextCursor` and searches again from page 1, exactly
    // like a fresh `journeyId`/`legId` mount already does.
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
      const response = await fetch(
        `/api/Journeys/${journeyId}/legs/${legId}/candidates?${params.toString()}`,
      );
      if (!response.ok) {
        setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
        return;
      }
      const body: CandidatesResponse = await response.json();
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
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t load candidate trains right now. Try again.
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
          {results.rows.length === 1 ? 'matches' : 'match'} your search — pick the one you&apos;ll be on. You can
          change it later.
        </Text>
        {pickError && <Alert color="red">{pickError}</Alert>}
        {results.rows.map((row) => (
          <CandidateRowView
            key={`${row.uid}-${row.scheduled}`}
            row={row}
            serviceDate={serviceDate}
            picking={picking}
            onPick={() => pick(row.uid)}
          />
        ))}
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
    </Stack>
  );
}

/** How this leg reads on this train: when it leaves the station the
 * traveller boards at, and when it reaches the one they get off at.
 * `arr.` is omitted -- not rendered as "?" or filled in from the train's
 * terminus arrival -- when the schedule has no time for the leg's
 * destination. */
function legTimes(row: CandidateRow): string {
  const departure = `dep. ${row.legOriginCrs} ${row.scheduled}`;
  if (!row.legDestinationArrival) return departure;
  // A non-zero day offset is rare but real (an overnight leg). Saying so
  // is cheaper than letting "dep. 23:40 → arr. 02:15" read as a
  // four-hours-backwards journey.
  const nextDay = row.legDestinationArrivalDayOffset > 0 ? ' (next day)' : '';
  return `${departure} → arr. ${row.legDestinationCrs} ${row.legDestinationArrival}${nextDay}`;
}

/** One candidate, shaped like the `/trains` search-result row the design
 * spec §2.3 asked this list to reuse: leg-scoped times as the row title,
 * the train's own identity and route as dimmed secondary text, and a
 * "View live status" link beside the action (2026-09-22 UX review, C4 +
 * I25).
 *
 * Deliberately a SHAPE match rather than an import of that row: the
 * `/trains` row is inline JSX inside `TrainSearchForm` and its action is
 * `TrackThisTrainButton`, which creates a standalone train subscription.
 * This list's action commits the pick to a journey LEG
 * (`POST /Journeys/{id}/legs/{id}/train`) -- a different write against a
 * different resource -- so extracting a shared component would mean
 * parameterising it on its own primary action, which is most of what the
 * component is. What it DOES take from that row is everything the review
 * found missing: the arrival time, the per-row live-status link, and the
 * shrink-guarded title/trailing layout (`StatusRow`, WCAG 2.5.3).
 *
 * `aria-label` on the button, not just the sibling `Text`: three or four
 * buttons all named "Track this train" is N indistinguishable items in a
 * screen reader's control list (I26/P3, WCAG 2.4.9). The label CONTAINS
 * the visible text, so Label-in-Name (2.5.3) still holds. */
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
  const times = legTimes(row);
  return (
    <StatusRow
      align="flex-start"
      title={<Text size="sm" fw={500}>{times}</Text>}
      subtitle={
        // The train's own identity and full route, dimmed and second --
        // useful context ("it's the Edinburgh train"), but no longer the
        // only thing on the row, which was C4.
        <Text size="xs" c="dimmed">
          Train {row.uid}
          {row.originCrs && row.destinationCrs ? ` · ${row.originCrs} → ${row.destinationCrs}` : ''}
          {row.operator !== null ? ` · ${row.operator}` : ''}
        </Text>
      }
      trailing={
        <Group gap="sm" wrap="nowrap">
          <TextLink href={`/train/${encodeURIComponent(row.uid)}/${serviceDate}`} size="sm" ariaLabel={`View live status for the ${times}`}>
            View live status
          </TextLink>
          <Button
            size="xs"
            loading={picking === row.uid}
            disabled={picking !== null}
            onClick={onPick}
            aria-label={`Track this train — ${times}`}
          >
            Track this train
          </Button>
        </Group>
      }
    />
  );
}
