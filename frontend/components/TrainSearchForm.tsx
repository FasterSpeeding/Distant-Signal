'use client';

import { useId, useMemo, useState, type SubmitEvent } from 'react';
import { Autocomplete, Button, Group, Loader, Stack, Text, VisuallyHidden } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { nowInLondon } from '@/lib/londonWallClock';
import { TextLink } from './TextLink';
import { TimeFilterInput } from './TimeFilterInput';
import { TrainSearchResults } from './TrainSearchResults';
import { ListFilterBar, SearchIcon, type ListNoun } from './ListFilterBar';
import type { TrainSearchDates, TrainSearchPage } from '@/lib/types';
import { searchDateBounds, searchDateDescription } from '@/lib/searchDates';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { useLiveSearch } from '@/lib/useLiveSearch';
import { replaceUrlParams, useUrlFilterSync } from '@/lib/useUrlFilterSync';
import {
  hasRows,
  TIME_PATTERN,
  TRAIN_URL_PARAMS,
  type TrainSearchResults as TrainSearchResultsState,
} from '@/lib/trainSearch';

const TRAIN_NOUN: ListNoun = { one: 'train', other: 'trains' };

const CRS_PATTERN = /^[A-Za-z]{3}$/;
/** Calling-point-first, whole-network train search -- the `/trains` page's
 * one interactive component.
 *
 * Two shapes (docs/style-guide.md, "List search and filters"): the full
 * form with a Search button until the first search, then the compact
 * sticky bar, with Station in the search slot and every other field behind
 * "Filters". From then on every change searches live (`useLiveSearch`),
 * and the URL follows through `history.replaceState`. Generalizes the earlier destination-first
 * search into "any station the train calls at" as the primary, required
 * key, with "departing from" (the schedule's TRUE origin) and "stops at"
 * (one OTHER calling point on the route, not necessarily the destination) as
 * independent, optional filters layered on top -- see
 * docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md and
 * docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md.
 *
 * Deliberately NOT a replacement for `TrackTrainForm`, and it does not try
 * to be: this searches published CIF timetable data by calling point and
 * cannot see a train that isn't in it (a station missing from
 * `stanox_crs`, a same-day amendment landing after the last CIF delivery).
 * `/track`'s manual-entry form remains the honest fallback for exactly
 * those gaps, and this component links to it explicitly.
 *
 * Filter set, and why it stops here: Station is required (it is the
 * server-side search key). Origin, Stops at, Date and an "Earliest
 * departure"/"Latest departure" time range are all optional. There is no
 * Operator filter (a non-goal for now): rows do carry the schedule's
 * `operator` (ATOC code, `null` when unknown), which the result rows show,
 * but the search itself is not keyed on it. Date defaults to today and is
 * bounded to a roughly week-either-side window
 * (`crates/api/src/routes/trains.rs::SEARCH_WINDOW_FORWARD_DAYS`/
 * `SEARCH_WINDOW_BACKWARD_DAYS`) -- see
 * docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md.
 *
 * "Earliest departure"/"Latest departure" are deliberately NOT named
 * "From"/"To": that wording collided with the separate "Departing from"
 * station field above it -- a place and a time range sharing the word
 * "from" -- which live testing found genuinely confusing (a time range
 * read as if it were about the "Departing from" station rather than about
 * Station). Every one of these four time fields now also carries a
 * `description` naming the specific station it is scoped to, for the same
 * reason.
 *
 * "Stops at (optional)" replaced the earlier single-valued "Terminating at"
 * (the schedule's TRUE destination): structurally the same single-station
 * `Autocomplete` (the same `useSuggestions`/`searchStations` pair the other
 * single-station fields above use), but a train matches as long as it calls
 * at this station, true destination or not -- PROVIDED that call falls
 * LATER in the journey than Station (2026-09-22; this used to be a
 * same-station-only rule, so naming the same station in both fields was the
 * only way to ask "does this working come BACK here" -- a loop/circular
 * service -- but a later-only reading is now what "Stops at" means for
 * every station named, loop or not: a search for "stops at X" should only
 * return trains you can actually ride from Station to X). See
 * docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md. An
 * earlier version of this field accepted several stations at once (a
 * Mantine `TagsInput`, matching ALL of them); that shipped and was then
 * deliberately scoped back down to exactly one station.
 *
 * A second, independent time-range pair -- "Earliest arrival"/"Latest
 * arrival" -- filters on when the train reaches Stops at's named calling
 * point, as opposed to "Earliest departure"/"Latest departure" above, which
 * stay scoped to Station. It only renders once Stops at is filled in,
 * mirroring the reveal condition the earlier "Terminating at" field used
 * for the same time-range pair.
 *
 * Fetches through the same-origin `/api/*` proxy, like every other Client
 * Component in this app (`API_BASE_URL` is server-only).
 *
 * `operatorNames` (ATOC code to name, from the page's TOC list, as on
 * `StationTimetable`) names each result row's operator; without it the
 * row's details line carries no operator.
 *
 * `searchDates` (`GET /public/trains/search/dates`, read by the page on the
 * server) bounds the date picker to what the search accepts; `null` or
 * absent (the read failed) falls back to a week either side of today, the
 * API's static window -- see `lib/searchDates.ts`. */
export function TrainSearchForm({
  initialStation = '',
  initialOrigin = '',
  initialStopsAt = '',
  initialDate = '',
  initialFrom = '',
  initialTo = '',
  initialArrivalFrom = '',
  initialArrivalTo = '',
  attachTicketId,
  operatorNames,
  searchDates,
}: {
  initialStation?: string | undefined;
  initialOrigin?: string | undefined;
  initialStopsAt?: string | undefined;
  initialDate?: string | undefined;
  initialFrom?: string | undefined;
  initialTo?: string | undefined;
  initialArrivalFrom?: string | undefined;
  initialArrivalTo?: string | undefined;
  attachTicketId?: number | undefined;
  operatorNames?: Readonly<Record<string, string>> | undefined;
  searchDates?: TrainSearchDates | null | undefined;
}) {
  const operatorLookup = useMemo(
    () => (operatorNames ? new Map(Object.entries(operatorNames)) : undefined),
    [operatorNames],
  );
  const [stationCrs, setStationCrs] = useState(initialStation);
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [stopsAt, setStopsAt] = useState(initialStopsAt);
  const [dateValue, setDateValue] = useState<string | null>(initialDate || null);
  const [fromTime, setFromTime] = useState(initialFrom);
  const [toTime, setToTime] = useState(initialTo);
  const [arrivalFrom, setArrivalFrom] = useState(initialArrivalFrom);
  const [arrivalTo, setArrivalTo] = useState(initialArrivalTo);
  /** Which of the four time filters are currently HALF-entered -- an hour
   * segment filled in with the minutes left blank, or vice versa.
   *
   * This cannot be read off the four value states above, and that is the
   * whole reason it exists: a native `<input type="time">` reports a
   * half-entered time as `''`, exactly like an untouched one (see
   * `TimeFilterInput`'s own doc comment). Without this, a field visibly
   * reading "09:--" would be silently dropped by `searchParams()`' own
   * `.trim()` gating and the search would run as though the caller had
   * never set that filter at all. `TimeFilterInput` reports the state up
   * through `onIncompleteChange`; `canSearch` below refuses to search on
   * it. */
  const [incompleteTimes, setIncompleteTimes] = useState({
    from: false,
    to: false,
    arrivalFrom: false,
    arrivalTo: false,
  });
  const [results, setResults] = useState<TrainSearchResultsState>(null);
  // The RAW `dateValue` submitted with the last search that came back
  // 404/unpublished, captured with the request -- separate from `results`
  // because the `'unpublished'` variant carries no payload of its own to
  // carry a `date` field on. Only ever read by the 'unpublished' branch of
  // `TrainSearchResults`; never read live from `dateValue` there, for the
  // same reason `results`' own `date` field exists (see `Results`' doc
  // comment).
  const [unpublishedDate, setUnpublishedDate] = useState<string | null>(null);
  /** Whether the search runs live. Off until the first Search press (or a
   * URL that already names a station): before that the full form is shown,
   * and a half-filled form must not start searching under the visitor. From
   * then on the form is the compact bar and every change searches. */
  const [live, setLive] = useState(() => CRS_PATTERN.test(initialStation.trim()));
  // Separate from the search's own pending flag on purpose: a "Load more"
  // in flight must not dim the rows already on screen.
  const [loadingMore, setLoadingMore] = useState(false);

  const stationErrorId = useId();
  const { suggestions: stationSuggestions, loading: stationSuggestionsLoading } = useSuggestions(
    stationCrs,
    searchStations,
  );
  const { suggestions: originSuggestions, loading: originSuggestionsLoading } = useSuggestions(
    originCrs,
    searchStations,
  );
  const { suggestions: stopsAtSuggestions, loading: stopsAtSuggestionsLoading } = useSuggestions(
    stopsAt,
    searchStations,
  );

  const stationValid = CRS_PATTERN.test(stationCrs.trim());
  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const stopsAtValid = stopsAt.trim() === '' || CRS_PATTERN.test(stopsAt.trim());
  const fromValid = fromTime.trim() === '' || TIME_PATTERN.test(fromTime.trim());
  const toValid = toTime.trim() === '' || TIME_PATTERN.test(toTime.trim());
  const arrivalFromValid = arrivalFrom.trim() === '' || TIME_PATTERN.test(arrivalFrom.trim());
  const arrivalToValid = arrivalTo.trim() === '' || TIME_PATTERN.test(arrivalTo.trim());
  /** Gated on `stopsAt` for the arrival pair, mirroring exactly what
   * `searchParams()` itself does with `arrival_from`/`arrival_to`: those
   * two fields unmount when Stops at is cleared, and their state is
   * deliberately remembered in case it is filled back in, so a half-entered
   * arrival time left behind by a since-removed Stops at must not go on
   * blocking a search it can no longer contribute a filter to. */
  const timesComplete =
    !incompleteTimes.from &&
    !incompleteTimes.to &&
    (stopsAt.trim() === '' || (!incompleteTimes.arrivalFrom && !incompleteTimes.arrivalTo));
  const filtersValid =
    stationValid &&
    originValid &&
    stopsAtValid &&
    fromValid &&
    toValid &&
    arrivalFromValid &&
    arrivalToValid &&
    timesComplete;

  const manualHref = attachTicketId !== undefined ? `/track?ticketId=${attachTicketId}` : '/track';
  // Anchored to Europe/London (FE-4), like the server's own window, so a
  // visitor whose device isn't on UK time gets the edges the server
  // enforces rather than a 400 at either end.
  const today = nowInLondon().format('YYYY-MM-DD');
  const dateBounds = searchDateBounds(searchDates, today);

  // What the "Earliest/Latest departure" and "Earliest/Latest arrival"
  // fields' `description`s name as the station they are scoped to --
  // preferring the actual entered CRS (so the helper text is concrete, e.g.
  // "RDG") and falling back to naming the Station field when nothing valid
  // has been entered yet, so the sentence still reads naturally.
  const stationDisplay = stationValid ? stationCrs.trim().toUpperCase() : 'Station';
  const stopsAtDisplay = stopsAtValid && stopsAt.trim() ? stopsAt.trim().toUpperCase() : 'Stops at';

  /** The current filter set as query parameters. Shared by the search and
   * by "Load more" so that page 2 is unambiguously a continuation of page
   * 1's query. */
  function searchParams() {
    const params = new URLSearchParams({ station: stationCrs.trim().toUpperCase() });
    if (dateValue) params.set('date', dateValue);
    if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
    if (stopsAt.trim()) {
      params.set('stops_at', stopsAt.trim().toUpperCase());
      // Gated on stops_at being set, not just on the fields having values:
      // this is what makes clearing Stops at drop any previously-entered
      // arrival-time filter from the actual request, without needing to
      // also clear arrivalFrom/To state -- the fields themselves unmount
      // (see the conditional render below) but their state is deliberately
      // remembered in case Stops at is filled back in.
      if (arrivalFrom.trim()) params.set('arrival_from', arrivalFrom.trim());
      if (arrivalTo.trim()) params.set('arrival_to', arrivalTo.trim());
    }
    if (fromTime.trim()) params.set('from', fromTime.trim());
    if (toTime.trim()) params.set('to', toTime.trim());
    return params;
  }

  const query = filtersValid ? searchParams().toString() : null;

  /** `GET /public/trains/search`'s page 1 for `searched`, run by
   * `useLiveSearch`, which aborts it if the filters change first. */
  const { pending } = useLiveSearch(live ? query : null, async (searched, signal) => {
    // The date that was actually searched, read from the request rather
    // than the live picker, which can move while the request is out.
    const submittedDateValue = new URLSearchParams(searched).get('date');
    // A function, not `signal.aborted` inline: it changes across the awaits.
    const superseded = () => signal.aborted;
    try {
      const response = await fetch(`/api/trains/search?${searched}`, { signal });
      if (superseded()) return;
      if (response.status === 404) {
        setResults('unpublished');
        setUnpublishedDate(submittedDateValue);
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body = (await response.json()) as TrainSearchPage;
      if (superseded()) return;
      setResults({
        rows: body.results,
        nextCursor: body.nextCursor,
        date: submittedDateValue ?? '',
        loadMoreFailed: false,
        provisional: body.provisional === true,
      });
    } catch {
      if (!superseded()) setResults('error');
    }
  });

  // Keeps this /trains history entry's URL on the search on screen, so
  // following a result to `/train/[uid]/[date]` and pressing Back restores
  // both the filters and the results (see
  // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
  // §3.1). `history.replaceState` through `useUrlFilterSync`, not
  // `router.replace`, which re-ran this page's server render (refetching
  // the operator list and search dates) on every search. Only once the
  // search is live: a form nobody has searched with yet leaves the URL
  // alone. `ticketId` and any other param are kept.
  useUrlFilterSync(live && query !== null ? query : null, (current, state) =>
    state === null
      ? current
      : replaceUrlParams(current, TRAIN_URL_PARAMS, Object.fromEntries(new URLSearchParams(state))),
  );

  function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    if (filtersValid) setLive(true);
  }

  async function handleLoadMore() {
    if (!hasRows(results)) return;
    if (results.nextCursor === null || loadingMore) return;

    // Page 1's actual raw date (`''` meaning "none picked"), not whatever
    // the picker shows right now -- the caller may have moved `dateValue`
    // after searching but before pressing "Load more". `searchParams()`
    // itself would read live `dateValue`, so its own `date` handling is
    // overridden below to match page 1 exactly: present with page 1's
    // value, or absent, never a stale-vs-live mismatch either way. This
    // keeps this request unambiguously "page 2 of the same search".
    const pageOneDate = results.date;
    // The exact result-set object this page is a continuation of. A fresh
    // search can resolve first and leave this response describing a result
    // set that is no longer on screen; the live search always installs a
    // BRAND NEW object, so identity is all that is needed to spot that.
    // Without this check the stale page would append its rows to (and stamp
    // its cursor, or its failure, onto) somebody else's search.
    const pagedFrom = results;
    setLoadingMore(true);
    try {
      const params = searchParams();
      if (pageOneDate) {
        params.set('date', pageOneDate);
      } else {
        params.delete('date');
      }
      params.set('after', results.nextCursor);
      const response = await fetch(`/api/trains/search?${params.toString()}`);
      if (!response.ok) {
        // The cursor is kept, not nulled: this page just failed to load, and
        // the reader gets a named error plus a working retry rather than a
        // list that quietly stops one page short of the end.
        setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
        return;
      }
      const body = (await response.json()) as TrainSearchPage;
      setResults((current) =>
        current === pagedFrom
          ? {
              rows: [...current.rows, ...body.results],
              nextCursor: body.nextCursor,
              date: current.date,
              loadMoreFailed: false,
              provisional: current.provisional,
            }
          : current,
      );
    } catch {
      setResults((current) => (current === pagedFrom ? { ...current, loadMoreFailed: true } : current));
    } finally {
      setLoadingMore(false);
    }
  }

  const stationError = stationCrs.length > 0 && !stationValid ? 'Must be a 3-letter station code' : null;
  const stationAutocompleteProps = suggestionAutocompleteProps(stationSuggestions, {
    query: stationCrs,
    loading: stationSuggestionsLoading,
    noMatchMessage: 'No matching stations',
  });

  // Inside the filters popover the option lists and the date picker render
  // in place (`withinPortal: false`): a portalled dropdown counts as outside
  // the popover, so picking an option closed the whole panel, and it would
  // sit outside the phone sheet's focus trap.
  const inPanel = live;
  const comboboxProps = inPanel ? { comboboxProps: { withinPortal: false } } : {};

  /** Every field but Station: the rest of the full form, then the panel. */
  const otherFields = (
    <>
      {/* `clearButtonProps`: Mantine's `clearable` clear button ships with
          no accessible name, so axe's `button-name` fires (critical) as
          soon as a date is picked. */}
      <DatePickerInput
        valueFormat="D MMM YYYY"
        label="Date (optional)"
        placeholder="Today"
        description={searchDateDescription(dateBounds, today)}
        value={dateValue}
        onChange={setDateValue}
        minDate={dateBounds.minDate}
        maxDate={dateBounds.maxDate}
        clearable
        clearButtonProps={{ 'aria-label': 'Clear the date' }}
        {...(inPanel && { popoverProps: { withinPortal: false } })}
      />
      <Autocomplete
        label="Departing from (optional)"
        placeholder="e.g. Paddington or PAD"
        description="Where the journey actually begins."
        value={originCrs}
        onChange={setOriginCrs}
        {...suggestionAutocompleteProps(originSuggestions, {
          query: originCrs,
          loading: originSuggestionsLoading,
          noMatchMessage: 'No matching stations',
        })}
        {...comboboxProps}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter station code' : null}
      />
      <Autocomplete
        label="Stops at (optional)"
        placeholder="e.g. Reading or RDG"
        description="A station this train reaches later in its journey than Station, its destination included."
        value={stopsAt}
        onChange={setStopsAt}
        {...suggestionAutocompleteProps(stopsAtSuggestions, {
          query: stopsAt,
          loading: stopsAtSuggestionsLoading,
          noMatchMessage: 'No matching stations',
        })}
        {...comboboxProps}
        error={stopsAt.length > 0 && !stopsAtValid ? 'Must be a 3-letter station code' : null}
      />
      {/* All four time filters are `TimeFilterInput` -- a native
       * `<input type="time">` with a clock button that opens the platform
       * time picker and a clear button. See `TimeFilterInput`'s own doc
       * comment. A native time input's value is already exactly the
       * `"HH:MM"` these four fields put on the wire, and an emptied field is
       * `""`, so clearing a field drops its query param. No `placeholder`:
       * a native time input paints its own `--:--` segment hints. */}
      <Group grow align="flex-start">
        <TimeFilterInput
          clockHint
          label="Earliest departure (optional)"
          name="earliest departure"
          description={`Only trains at ${stationDisplay} at or after this time.`}
          value={fromTime}
          onChange={setFromTime}
          onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, from: incomplete }))}
          error={fromTime.length > 0 && !fromValid ? 'Must be a time like 09:00' : null}
        />
        <TimeFilterInput
          label="Latest departure (optional)"
          name="latest departure"
          description={`Only trains at ${stationDisplay} at or before this time.`}
          value={toTime}
          onChange={setToTime}
          onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, to: incomplete }))}
          error={toTime.length > 0 && !toValid ? 'Must be a time like 09:00' : null}
        />
      </Group>
      {stopsAt.trim() !== '' && (
        <Group grow align="flex-start">
          <TimeFilterInput
            label="Earliest arrival (optional)"
            name="earliest arrival"
            description={`Only trains reaching ${stopsAtDisplay} at or after this time.`}
            value={arrivalFrom}
            onChange={setArrivalFrom}
            onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, arrivalFrom: incomplete }))}
            error={arrivalFrom.length > 0 && !arrivalFromValid ? 'Must be a time like 09:00' : null}
          />
          <TimeFilterInput
            label="Latest arrival (optional)"
            name="latest arrival"
            description={`Only trains reaching ${stopsAtDisplay} at or before this time.`}
            value={arrivalTo}
            onChange={setArrivalTo}
            onIncompleteChange={(incomplete) => setIncompleteTimes((c) => ({ ...c, arrivalTo: incomplete }))}
            error={arrivalTo.length > 0 && !arrivalToValid ? 'Must be a time like 09:00' : null}
          />
        </Group>
      )}
    </>
  );

  const activeCount =
    (dateValue ? 1 : 0) +
    (originCrs.trim() ? 1 : 0) +
    (stopsAt.trim() ? 1 : 0) +
    (fromTime.trim() ? 1 : 0) +
    (toTime.trim() ? 1 : 0) +
    (stopsAt.trim() && arrivalFrom.trim() ? 1 : 0) +
    (stopsAt.trim() && arrivalTo.trim() ? 1 : 0);

  function clearPanelFilters() {
    setDateValue(null);
    setOriginCrs('');
    setStopsAt('');
    setFromTime('');
    setToTime('');
    setArrivalFrom('');
    setArrivalTo('');
    setIncompleteTimes({ from: false, to: false, arrivalFrom: false, arrivalTo: false });
  }

  // Known only once the last page is in: the search api has no total.
  const matchCount = !pending && hasRows(results) && results.nextCursor === null ? results.rows.length : null;

  const resultsSection = (
    <Stack gap="xs" mih={72}>
      <TrainSearchResults
        stationValid={stationValid}
        searching={pending && results === null}
        pending={pending}
        results={results}
        unpublishedDate={unpublishedDate}
        loadingMore={loadingMore}
        onLoadMore={() => void handleLoadMore()}
        operatorLookup={operatorLookup}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );

  // `inline`/`underline="always"`: this link sits mid-sentence in body
  // text, so colour alone can't be what marks it (WCAG 1.4.1), and it must
  // render as a `<span>`, not `TextLink`'s default `<p>`.
  const manualFallback = (
    <Text size="sm" c="dimmed">
      Can&apos;t find your train?{' '}
      <TextLink href={manualHref} inline underline="always">
        Track it manually
      </TextLink>{' '}
      by entering its origin station and departure time.
    </Text>
  );

  if (live) {
    return (
      <Stack gap="md">
        <ListFilterBar
          search={
            <Autocomplete
              aria-label="Station"
              placeholder="Station, e.g. Reading or RDG"
              value={stationCrs}
              onChange={setStationCrs}
              {...stationAutocompleteProps}
              // Only the red outline here: the bar is one fixed-height row,
              // so the message itself goes below it.
              error={stationError !== null}
              aria-describedby={stationError ? stationErrorId : undefined}
              leftSection={
                pending ? <Loader size={14} aria-hidden="true" data-testid="list-filter-pending" /> : <SearchIcon />
              }
              flex="1 1 auto"
              miw={0}
            />
          }
          noun={TRAIN_NOUN}
          matchCount={matchCount}
          pending={pending}
          activeCount={activeCount}
          onClearFilters={clearPanelFilters}
          panelWidth={380}
          scrollablePanel
        >
          {otherFields}
        </ListFilterBar>
        {stationError && (
          <Text size="sm" c="red" id={stationErrorId}>
            {stationError}
          </Text>
        )}
        {/* No count to announce (the search api has no total), but a
            screen reader still hears that a search is running. */}
        <VisuallyHidden role="status">{pending ? 'Searching' : ''}</VisuallyHidden>
        {resultsSection}
        {manualFallback}
      </Stack>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <Autocomplete
        label="Station"
        placeholder="e.g. Reading or RDG"
        description="Any station this train calls at along its route."
        value={stationCrs}
        onChange={setStationCrs}
        {...stationAutocompleteProps}
        error={stationError}
        required
      />
      {otherFields}
      <Group>
        <Button type="submit" disabled={!filtersValid}>
          Search
        </Button>
      </Group>
      {resultsSection}
      {manualFallback}
    </Stack>
  );
}
