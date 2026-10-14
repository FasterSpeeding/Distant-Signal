'use client';

import { useEffect, useMemo, useState, type SubmitEvent } from 'react';
import { usePathname, useRouter } from 'next/navigation';
import { Autocomplete, Button, Group, Stack, Text } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import { nowInLondon } from '@/lib/londonWallClock';
import { TextLink } from './TextLink';
import { TimeFilterInput } from './TimeFilterInput';
import { TrainSearchResults } from './TrainSearchResults';
import type { TrainSearchDates, TrainSearchPage } from '@/lib/types';
import { searchDateBounds, searchDateDescription } from '@/lib/searchDates';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { ButtonSpinner } from '@/components/LoadingText';
import { hasRows, TIME_PATTERN, type TrainSearchResults as TrainSearchResultsState } from '@/lib/trainSearch';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
/** Calling-point-first, whole-network train search -- the `/trains` page's
 * one interactive component. Generalizes the earlier destination-first
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
  const router = useRouter();
  const operatorLookup = useMemo(
    () => (operatorNames ? new Map(Object.entries(operatorNames)) : undefined),
    [operatorNames],
  );
  const pathname = usePathname();
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
  // 404/unpublished, captured at submit time -- separate from `results`
  // because the `'unpublished'` variant carries no payload of its own to
  // carry a `date` field on. Only ever read by the 'unpublished' branch of
  // `TrainSearchResults`; never read live from `dateValue` there, for the
  // same reason `results`' own `date` field exists (see `Results`' doc
  // comment).
  const [unpublishedDate, setUnpublishedDate] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  // Separate from `searching` on purpose: a "Load more" in flight must not
  // blank the rows already on screen the way `TrainSearchResults`'
  // `searching` branch does, and must not re-disable the Search button.
  const [loadingMore, setLoadingMore] = useState(false);

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
  const canSearch =
    stationValid &&
    originValid &&
    stopsAtValid &&
    fromValid &&
    toValid &&
    arrivalFromValid &&
    arrivalToValid &&
    timesComplete &&
    !searching;

  const manualHref = attachTicketId !== undefined ? `/track?ticketId=${attachTicketId}` : '/track';
  // Anchored to Europe/London (FE-4), like the server's own window, so a
  // visitor whose device isn't on UK time gets the edges the server
  // enforces rather than a 400 at either end.
  const today = nowInLondon().format('YYYY-MM-DD');
  const dateBounds = searchDateBounds(searchDates, today);

  // What the "Earliest/Latest departure" and "Earliest/Latest arrival"
  // fields' `description`s name as the station they are scoped to --
  // preferring the actual entered CRS (so the helper text is concrete, e.g.
  // "RDG") and falling back to naming the field above it when nothing valid
  // has been entered yet, so the sentence still reads naturally.
  const stationDisplay = stationValid ? stationCrs.trim().toUpperCase() : 'Station above';
  const stopsAtDisplay = stopsAtValid && stopsAt.trim() ? stopsAt.trim().toUpperCase() : 'Stops at above';

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query. */
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

  /** The actual `GET /public/trains/search` call for page 1, shared by the
   * explicit Search button (`handleSubmit`) and the auto-run-on-mount
   * effect below, so a search fired either way behaves identically -- same
   * loading state, same 404-vs-error handling, same `Results` shape
   * installed on success. Mirrors `IncidentSearchForm.tsx`'s own identically
   * named, identically shared `runSearch` -- see
   * docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
   * §3.3. */
  async function runSearch() {
    setSearching(true);
    // Captured synchronously, BEFORE the `await` below -- this is what makes
    // it "the date that was actually searched" rather than a live read that
    // could reflect a picker change the caller made while the request was
    // in flight. Mirrors what `searchParams()` itself just read into the
    // request that's about to go out.
    const submittedDateValue = dateValue;
    try {
      const response = await fetch(`/api/trains/search?${searchParams().toString()}`);
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
      setResults({
        rows: body.results,
        nextCursor: body.nextCursor,
        date: submittedDateValue || '',
        loadMoreFailed: false,
        provisional: body.provisional === true,
      });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    if (!canSearch) return;
    // Keep this /trains history entry's URL in sync with the last search
    // that actually ran, so following a result to `/train/[uid]/[date]` and
    // pressing Back can restore both the form and the results, rather than
    // re-delivering the stale URL /trains first loaded with (see
    // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
    // §3.1). `replace`, not `push`: this should keep /trains a single
    // history entry whose URL stays current, not add a new Back-button stop
    // on every search -- mirrors `StationSearchForm.tsx`'s own use of
    // `next/navigation`, for a different navigation shape. Deliberately NOT
    // done from the mount effect below (§3.3) -- only an explicit search
    // press writes to the URL, matching how this component already only
    // wrote a *fetch* URL from `searchParams()`, never from a mount-time
    // default.
    router.replace(`${pathname}?${searchParams().toString()}`, { scroll: false });
    await runSearch();
  }

  // Restores not just the form's fields (the `initialX` props above already
  // do that) but the RESULTS too, when this page is entered with a station
  // already in the URL -- most notably after Back-navigation from
  // `/train/[uid]/[date]` once `handleSubmit` above has kept the URL
  // current. Structurally identical to `IncidentSearchForm.tsx`'s own
  // mount-only auto-run effect: an empty dependency array, deliberately NOT
  // re-run on every later filter edit (that stays the Search button's own
  // job), gated on `initialStation` specifically -- the one *required*
  // precondition this form's own `stationValid`/`canSearch` already treat
  // specially -- rather than "any initial value is present". See
  // docs/superpowers/specs/2026-09-22-train-search-state-persistence-design.md
  // §3.3.
  //
  // Known, accepted limitation (§3.4): if the visitor had pressed "Load
  // more" one or more times before leaving, only PAGE 1 of that result set
  // is reconstructed here -- `nextCursor` is server-issued opaque pagination
  // state with no representation in `searchParams()`/the URL at all, so
  // there is nothing for this effect to resume from. This re-runs the FIRST
  // page of the same search, not a resume of exactly where "Load more" had
  // gotten to -- a correct, if smaller, restoration rather than a wrong one,
  // and not a bug to fix in this pass.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- mount-only restore of the URL's search (see above)
    if (CRS_PATTERN.test(initialStation.trim())) void runSearch();
    // Intentionally empty: this is a mount-only effect, not one that tracks
    // the filter state it reads -- see the comment above.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- mount-only effect (see above)
  }, []);

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
    // The exact result-set object this page is a continuation of. Search is
    // not disabled while a page is in flight, so a fresh search can resolve
    // first and leave this response describing a result set that is no
    // longer on screen; `handleSubmit` always installs a BRAND NEW object,
    // so identity is all that is needed to spot that. Without this check the
    // stale page would append its rows to (and stamp its cursor, or its
    // failure, onto) somebody else's search.
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

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <Autocomplete
        label="Station"
        placeholder="e.g. Reading or RDG"
        description="Any station this train calls at along its route."
        value={stationCrs}
        onChange={setStationCrs}
        {...suggestionAutocompleteProps(stationSuggestions, {
          query: stationCrs,
          loading: stationSuggestionsLoading,
          noMatchMessage: 'No matching stations',
        })}
        error={stationCrs.length > 0 && !stationValid ? 'Must be a 3-letter station code' : null}
        required
      />
      {/* `clearButtonProps`: Mantine's `clearable` clear button ships with
          no accessible name, so axe's `button-name` fires (critical) as
          soon as a date is picked. See `components/IncidentSearchForm.tsx`'s
          own note and `app/lines/AllLinesTable.tsx` for the same fix. */}
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
        error={stopsAt.length > 0 && !stopsAtValid ? 'Must be a 3-letter station code' : null}
      />
      {/* All four time filters are `TimeFilterInput` -- a native
       * `<input type="time">` with a clock button that opens the platform
       * time picker and a clear button -- not the free-text `TextInput`
       * they used to be, which required the caller to TYPE "HH:MM"
       * correctly with nothing but a "09:00" placeholder to go on. See
       * `TimeFilterInput`'s own doc comment for why the two buttons have to
       * be supplied explicitly rather than left to the native control, and
       * why `@mantine/dates`' `TimePicker` isn't used instead. Typing is
       * not taken away: the segments still accept digits and arrow keys.
       *
       * Value shape is unchanged, which is the whole point: a native time
       * input's value is already exactly the `"HH:MM"` (`step` is 60, so
       * never seconds) these four fields have always put on the wire as
       * `from`/`to`/`arrival_from`/`arrival_to`, and an emptied field is
       * `""` exactly as before -- so `searchParams()`, the optional-filter
       * `.trim()` gating and `TIME_PATTERN` all keep working untouched, and
       * clearing a field still drops its query param.
       *
       * `DateTimePicker`/`DatePickerInput` (this app's existing date
       * controls, including the "Date (optional)" field just above) are
       * deliberately NOT what's used here: those carry a calendar, and
       * these four are time-of-day filters ON a date chosen separately --
       * a second calendar per field would be four more ways to disagree
       * with the one date the search actually runs against.
       *
       * No `placeholder`: a native time input renders its own `--:--`
       * segment hints and never paints a `placeholder`, so the old
       * "09:00"/"12:00" hints would show on no browser that supports the
       * control -- and the format they used to teach is now taught by the
       * control's own segments instead. (They WOULD still render on a
       * browser old enough to degrade `type="time"` to a text box; that is
       * the only thing dropping them costs, against keeping four props
       * that are invisible everywhere else.) The four error strings now
       * all name the same "09:00" example rather than each echoing its own
       * removed placeholder. */}
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
      <Group>
        <Button type="submit" disabled={!canSearch} leftSection={searching ? <ButtonSpinner /> : undefined}>
          {searching ? 'Searching' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        <TrainSearchResults
          stationValid={stationValid}
          searching={searching}
          results={results}
          unpublishedDate={unpublishedDate}
          loadingMore={loadingMore}
          onLoadMore={() => void handleLoadMore()}
          operatorLookup={operatorLookup}
          attachTicketId={attachTicketId}
        />
      </Stack>
      {/* `inline`/`underline="always"`: this link sits mid-sentence in body
          text rather than in a nav or beside a heading, so (a) colour alone
          can't be what marks it (WCAG 1.4.1) and (b) it must render as a
          `<span>`, not `TextLink`'s default `<p>`, or the rest of the
          sentence gets forced onto its own line. See `TextLink`'s own doc
          comment. */}
      <Text size="sm" c="dimmed">
        Can&apos;t find your train?{' '}
        <TextLink href={manualHref} inline underline="always">
          Track it manually
        </TextLink>{' '}
        by entering its origin station and departure time.
      </Text>
    </Stack>
  );
}
