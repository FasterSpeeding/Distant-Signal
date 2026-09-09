'use client';

import { useState, type FormEvent } from 'react';
import { Alert, Autocomplete, Button, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import { DatePickerInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Mirrors the backend's exact window --
 * `crates/api/src/routes/trains.rs::SEARCH_WINDOW_FORWARD_DAYS`/
 * `SEARCH_WINDOW_BACKWARD_DAYS` -- so the picker can never construct a
 * request the server will 400. Computed once per render from `dayjs()`,
 * consistent with this file's existing `today` computation just below. */
function dateWindow() {
  return {
    minDate: dayjs().subtract(7, 'day').format('YYYY-MM-DD'),
    maxDate: dayjs().add(7, 'day').format('YYYY-MM-DD'),
  };
}

/** Wire shape of `GET /public/trains/search`
 * (`crates/api/src/render.rs::calling_point_departure_json`).
 * `stationCrs` is the required calling-point search key, echoed back on
 * every row. `originCrs`/`destinationCrs` are both nullable: they are the
 * schedule's TRUE origin/destination, independent optional filters, and
 * either can be unresolved for a real published schedule (see
 * `schedule_query::DestinationDeparture`'s own doc comment). Like every
 * CIF-derived row in this app it carries NO operator and NO live running
 * status. */
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  stationCrs: string;
  originCrs: string | null;
  destinationCrs: string | null;
  destinationArrival: string | null;
}

/** The envelope `GET /public/trains/search` returns. Not a bare array: it
 * has to carry `nextCursor`, because the backend publishes and stores the
 * whole day uncapped and hands it back a page at a time. `nextCursor` is an
 * explicit `null` on the last page, never omitted. */
interface TrainSearchResponse {
  results: TrainSearchRow[];
  nextCursor: string | null;
}

/** Exactly one of five mutually-exclusive states, checked top to bottom by
 * `resultsContent` below. `'unpublished'` and an empty `rows` array are
 * genuinely different facts and get different copy -- the backend route
 * draws that 404-vs-empty-results distinction on purpose and collapsing it
 * here would waste it.
 *
 * `nextCursor` lives INSIDE the success variant rather than in its own
 * `useState`, so it cannot survive a state transition it does not belong
 * to: a fresh search, an error, or an unpublished response all discard it
 * automatically. */
type Results =
  | { rows: TrainSearchRow[]; nextCursor: string | null }
  | 'unpublished'
  | 'error'
  | null;

/** Calling-point-first, whole-network train search -- the `/trains` page's
 * one interactive component. Generalizes the earlier destination-first
 * search into "any station the train calls at" as the primary, required
 * key, with "departing from" (the schedule's TRUE origin) and "terminating
 * at" (the schedule's TRUE destination) as independent, optional filters
 * layered on top -- see
 * docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
 *
 * Deliberately NOT a replacement for `TrackTrainForm`, and it does not try
 * to be: this searches published CIF timetable data by calling point and
 * cannot see a train that isn't in it (a station missing from
 * `stanox_crs`, a same-day amendment landing after the last CIF delivery).
 * `/track`'s manual-entry form remains the honest fallback for exactly
 * those gaps, and this component links to it explicitly.
 *
 * Filter set, and why it stops here: Station is required (it is the
 * server-side search key). Origin, Destination, Date and a From/To time
 * range are all optional. There is no Operator filter -- CIF rows carry
 * no operator field at all, an explicit non-goal, not an omission to fill
 * in later. Date defaults to today and is bounded to a roughly week-either-
 * side window (`crates/api/src/routes/trains.rs::SEARCH_WINDOW_FORWARD_DAYS`/
 * `SEARCH_WINDOW_BACKWARD_DAYS`) -- see
 * docs/superpowers/specs/2026-09-09-trains-search-multi-day-design.md.
 *
 * A second, independent time-range pair -- "Arrival from"/"Arrival to" --
 * filters on when the train reaches Destination, as opposed to From/To
 * above, which stay scoped to Station. It only renders once Destination is
 * filled in: an arrival-time filter with nothing named to arrive at is
 * ambiguous, and the backend 400s exactly that combination (see
 * `crates/api/src/routes/trains.rs`'s own validation), so this component
 * never lets the caller construct it. See
 * docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.
 *
 * Fetches through the same-origin `/api/*` proxy, like every other Client
 * Component in this app (`API_BASE_URL` is server-only). */
export function TrainSearchForm({
  initialStation = '',
  initialOrigin = '',
  initialDestination = '',
  initialDate = '',
  attachTicketId,
}: {
  initialStation?: string;
  initialOrigin?: string;
  initialDestination?: string;
  initialDate?: string;
  attachTicketId?: number;
}) {
  const [stationCrs, setStationCrs] = useState(initialStation);
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [dateValue, setDateValue] = useState<string | null>(initialDate || null);
  const [fromTime, setFromTime] = useState('');
  const [toTime, setToTime] = useState('');
  const [destinationArrivalFrom, setDestinationArrivalFrom] = useState('');
  const [destinationArrivalTo, setDestinationArrivalTo] = useState('');
  const [results, setResults] = useState<Results>(null);
  const [searching, setSearching] = useState(false);
  // Separate from `searching` on purpose: a "Load more" in flight must not
  // blank the rows already on screen the way `resultsContent`'s
  // `searching` branch does, and must not re-disable the Search button.
  const [loadingMore, setLoadingMore] = useState(false);

  const { suggestions: stationSuggestions } = useSuggestions(stationCrs, searchStations);
  const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);
  const { suggestions: destinationSuggestions } = useSuggestions(destinationCrs, searchStations);

  const stationValid = CRS_PATTERN.test(stationCrs.trim());
  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const destinationValid = destinationCrs.trim() === '' || CRS_PATTERN.test(destinationCrs.trim());
  const fromValid = fromTime.trim() === '' || TIME_PATTERN.test(fromTime.trim());
  const toValid = toTime.trim() === '' || TIME_PATTERN.test(toTime.trim());
  const destinationArrivalFromValid =
    destinationArrivalFrom.trim() === '' || TIME_PATTERN.test(destinationArrivalFrom.trim());
  const destinationArrivalToValid =
    destinationArrivalTo.trim() === '' || TIME_PATTERN.test(destinationArrivalTo.trim());
  const canSearch =
    stationValid &&
    originValid &&
    destinationValid &&
    fromValid &&
    toValid &&
    destinationArrivalFromValid &&
    destinationArrivalToValid &&
    !searching;

  // Every result links to the DATE THAT WAS ACTUALLY SEARCHED -- not
  // always today, now that a search can target a different day.
  const searchedDate = dateValue || dayjs().format('YYYY-MM-DD');

  const manualHref = attachTicketId !== undefined ? `/track?ticketId=${attachTicketId}` : '/track';

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query. */
  function searchParams() {
    const params = new URLSearchParams({ station: stationCrs.trim().toUpperCase() });
    if (dateValue) params.set('date', dateValue);
    if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
    if (destinationCrs.trim()) {
      params.set('destination', destinationCrs.trim().toUpperCase());
      // Gated on destination being set, not just on the fields having
      // values: this is what makes clearing Destination drop any
      // previously-entered arrival-time filter, without needing to also
      // clear destinationArrivalFrom/To state -- the fields themselves
      // unmount (see the conditional render below) but their state is
      // deliberately remembered in case Destination is filled back in.
      if (destinationArrivalFrom.trim()) params.set('destination_from', destinationArrivalFrom.trim());
      if (destinationArrivalTo.trim()) params.set('destination_to', destinationArrivalTo.trim());
    }
    if (fromTime.trim()) params.set('from', fromTime.trim());
    if (toTime.trim()) params.set('to', toTime.trim());
    return params;
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSearch) return;
    setSearching(true);
    try {
      const response = await fetch(`/api/trains/search?${searchParams().toString()}`);
      if (response.status === 404) {
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  async function handleLoadMore() {
    if (results === null || results === 'error' || results === 'unpublished') return;
    if (results.nextCursor === null || loadingMore) return;

    setLoadingMore(true);
    try {
      const params = searchParams();
      params.set('after', results.nextCursor);
      const response = await fetch(`/api/trains/search?${params.toString()}`);
      if (!response.ok) {
        setResults((current) =>
          current !== null && current !== 'error' && current !== 'unpublished'
            ? { rows: current.rows, nextCursor: null }
            : current,
        );
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: [...current.rows, ...body.results], nextCursor: body.nextCursor }
          : current,
      );
    } catch {
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: current.rows, nextCursor: null }
          : current,
      );
    } finally {
      setLoadingMore(false);
    }
  }

  function resultsContent() {
    if (!stationValid) {
      return (
        <Text size="sm" c="dimmed">
          Enter a station above to search for trains that call there.
        </Text>
      );
    }
    if (searching) {
      return (
        <Text size="sm" c="dimmed">
          Searching…
        </Text>
      );
    }
    if (results === null) {
      return (
        <Text size="sm" c="dimmed">
          Press Search to find trains that call at this station.
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t search for trains right now. Try again.
        </Alert>
      );
    }
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          {dateValue
            ? `Scheduled timetable data for ${dateValue} isn't available yet — it may not have been published, or that station may not be one this feed covers.`
            : "Today's scheduled timetable data isn't available yet — it may not have been published, or that station may not be one this feed covers."}
        </Text>
      );
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No scheduled trains match those filters right now.
        </Text>
      );
    }
    return (
      <>
        <Text size="sm" c="dimmed">
          These are from the scheduled timetable, not live running information, and may be up to 30
          minutes out of date. Open a train to see its live status.
        </Text>
        <ScrollArea mah={420} offsetScrollbars>
          <Stack gap="xs">
            {results.rows.map((row) => (
              <Group key={`${row.uid}-${row.scheduled}`} justify="space-between" wrap="nowrap">
                <Text size="sm">
                  {row.scheduled} · {row.originCrs ?? '?'} → {row.stationCrs} → {row.destinationCrs ?? '?'}
                </Text>
                <Group gap="sm" wrap="nowrap">
                  <TextLink href={`/train/${encodeURIComponent(row.uid)}/${searchedDate}`}>
                    View live status
                  </TextLink>
                  <TrackThisTrainButton
                    uid={row.uid}
                    date={searchedDate}
                    attachTicketId={attachTicketId}
                    size="xs"
                  />
                </Group>
              </Group>
            ))}
          </Stack>
        </ScrollArea>
        {results.nextCursor !== null && (
          <Group>
            <Button
              variant="default"
              size="xs"
              onClick={handleLoadMore}
              disabled={loadingMore}
              loading={loadingMore}
            >
              Load more
            </Button>
          </Group>
        )}
      </>
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
        data={stationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = stationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={stationCrs.length > 0 && !stationValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <DatePickerInput
        label="Date (optional)"
        placeholder="Today"
        description="Search a different day, up to a week either side of today."
        value={dateValue}
        onChange={setDateValue}
        minDate={dateWindow().minDate}
        maxDate={dateWindow().maxDate}
        clearable
      />
      <Autocomplete
        label="Departing from (optional)"
        placeholder="e.g. Paddington or PAD"
        description="Where the journey actually begins."
        value={originCrs}
        onChange={setOriginCrs}
        data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = originSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
      />
      <Autocomplete
        label="Terminating at (optional)"
        placeholder="e.g. Manchester or MAN"
        description="Where the journey ends."
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={destinationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={destinationCrs.length > 0 && !destinationValid ? 'Must be a 3-letter CRS code' : null}
      />
      <Group grow align="flex-start">
        <TextInput
          label="From (optional)"
          placeholder="09:00"
          value={fromTime}
          onChange={(event) => setFromTime(event.currentTarget.value)}
          error={fromTime.length > 0 && !fromValid ? 'Must be a time like 09:00' : null}
        />
        <TextInput
          label="To (optional)"
          placeholder="12:00"
          value={toTime}
          onChange={(event) => setToTime(event.currentTarget.value)}
          error={toTime.length > 0 && !toValid ? 'Must be a time like 12:00' : null}
        />
      </Group>
      {destinationCrs.trim() !== '' && (
        <Group grow align="flex-start">
          <TextInput
            label="Arrival from (optional)"
            placeholder="09:00"
            description="When the train reaches Terminating at above -- separate from From/To, which are about Station above."
            value={destinationArrivalFrom}
            onChange={(event) => setDestinationArrivalFrom(event.currentTarget.value)}
            error={
              destinationArrivalFrom.length > 0 && !destinationArrivalFromValid
                ? 'Must be a time like 09:00'
                : null
            }
          />
          <TextInput
            label="Arrival to (optional)"
            placeholder="09:30"
            value={destinationArrivalTo}
            onChange={(event) => setDestinationArrivalTo(event.currentTarget.value)}
            error={
              destinationArrivalTo.length > 0 && !destinationArrivalToValid
                ? 'Must be a time like 09:30'
                : null
            }
          />
        </Group>
      )}
      <Group>
        <Button type="submit" disabled={!canSearch}>
          {searching ? 'Searching…' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        {resultsContent()}
      </Stack>
      <Text size="sm" c="dimmed" component="div">
        Can&apos;t find your train? <TextLink href={manualHref}>Track it manually</TextLink> by
        entering its origin station and departure time.
      </Text>
    </Stack>
  );
}
