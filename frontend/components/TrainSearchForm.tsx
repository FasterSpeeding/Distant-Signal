'use client';

import { useState, type FormEvent } from 'react';
import { Alert, Autocomplete, Button, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Wire shape of `GET /public/trains/search`
 * (`crates/api/src/render.rs::destination_departure_json`). Deliberately
 * NOT `ScheduleDepartureRow` from `TrackTrainForm.tsx`, even though the two
 * overlap: this one carries `originCrs` (the station a train departs FROM,
 * which for a mid-route result is an intermediate stop, not the schedule's
 * first station) and a non-nullable `destinationCrs` (it is the search key,
 * so a row can only exist if it resolved). Like every CIF-derived row in
 * this app it carries NO operator and NO live running status -- the CIF
 * SCHEDULE feed has neither (see
 * docs/superpowers/specs/2026-09-07-train-listing-page-design.md §1.3), and
 * this list never fabricates them. */
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  originCrs: string;
  destinationCrs: string;
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
 * draws that 404-vs-empty-results distinction on purpose (Task 7) and
 * collapsing it here would waste it.
 *
 * `nextCursor` lives INSIDE the success variant rather than in its own
 * `useState`, so it cannot survive a state transition it does not belong
 * to: a fresh search, an error, or an unpublished response all discard it
 * automatically, and there is no way to render "Load more" next to an error
 * or next to page 1 of a search that has since been re-run. */
type Results =
  | { rows: TrainSearchRow[]; nextCursor: string | null }
  | 'unpublished'
  | 'error'
  | null;

/** Destination-first, whole-network train search -- the `/trains` page's
 * one interactive component
 * (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
 * Approach B).
 *
 * Deliberately NOT a replacement for `TrackTrainForm`, and it does not try
 * to be: this searches published CIF timetable data by destination and
 * cannot see a train that isn't in it (a station missing from
 * `stanox_crs`, a same-day amendment landing after the last CIF delivery).
 * Note that "the result list was truncated" is NOT on that list any more:
 * nothing is capped, and everything past the first page is reachable with
 * Load more. `/track`'s manual-entry form
 * remains the honest fallback for exactly those gaps, and this component
 * links to it explicitly rather than pretending they don't exist -- §4 of
 * the design doc is a direct "no" on full replacement.
 *
 * Filter set, and why it stops here: Destination is required (it is the
 * server-side bucket key); Origin and a From/To time range are optional.
 * There is no Operator filter -- CIF rows carry no operator field at all,
 * so the filter could only ever match nothing. There is no Date filter --
 * both of this app's schedule sources are "today only, server-side". Both
 * are explicit §6 non-goals, not omissions to fill in later.
 *
 * Fetches through the same-origin `/api/*` proxy, like every other Client
 * Component in this app (`API_BASE_URL` is server-only). */
export function TrainSearchForm({
  initialDestination = '',
  initialOrigin = '',
  attachTicketId,
}: {
  initialDestination?: string;
  initialOrigin?: string;
  attachTicketId?: number;
}) {
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [fromTime, setFromTime] = useState('');
  const [toTime, setToTime] = useState('');
  const [results, setResults] = useState<Results>(null);
  const [searching, setSearching] = useState(false);
  // Separate from `searching` on purpose: a "Load more" in flight must not
  // blank the rows already on screen the way `resultsContent`'s
  // `searching` branch does, and must not re-disable the Search button.
  const [loadingMore, setLoadingMore] = useState(false);

  const { suggestions: destinationSuggestions } = useSuggestions(destinationCrs, searchStations);
  const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);

  const destinationValid = CRS_PATTERN.test(destinationCrs.trim());
  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const fromValid = fromTime.trim() === '' || TIME_PATTERN.test(fromTime.trim());
  const toValid = toTime.trim() === '' || TIME_PATTERN.test(toTime.trim());
  const canSearch = destinationValid && originValid && fromValid && toValid && !searching;

  // Computed once per render rather than once per row: every result links
  // to the same calendar date, because this search is always "today"
  // (server-side) -- same construction and same browser-local-date
  // assumption `TrackTrainForm`'s own CIF branch already makes.
  const today = dayjs().format('YYYY-MM-DD');

  // The manual fallback keeps a standalone ticket's id attached to the
  // journey, so a user who bounces from /trains to /track doesn't silently
  // lose the ticket they were trying to link -- same `?ticketId=` param
  // `app/track/page.tsx` already reads.
  const manualHref = attachTicketId !== undefined ? `/track?ticketId=${attachTicketId}` : '/track';

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query -- the cursor is positional, not
   * self-describing, so dropping a filter here would silently widen the
   * search mid-scroll. */
  function searchParams() {
    const params = new URLSearchParams({ destination: destinationCrs.trim().toUpperCase() });
    if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
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
        // A real, distinct fact, not an error: no CIF publish has landed
        // for today at all. Never collapsed into "no matches".
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: TrainSearchResponse = await response.json();
      // REPLACE, not append -- a fresh search starts over. The mirror of
      // `handleLoadMore` below, and the two must not be merged.
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  /** Fetches the next page and APPENDS it.
   *
   * Only reachable when `results` is a success state carrying a non-null
   * `nextCursor`, so it re-reads that state at call time rather than
   * trusting a captured value -- which is also what makes a second click
   * use the SECOND response's cursor rather than the first's.
   *
   * A failed "Load more" deliberately does NOT blow away the rows already
   * on screen: it leaves them, drops the cursor so the button disappears,
   * and lets the user re-run the search if they want. Replacing a good
   * partial list with a full-width error would be a worse outcome than
   * showing fewer trains. */
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
    if (!destinationValid) {
      return (
        <Text size="sm" c="dimmed">
          Enter a destination station above to search for trains.
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
          Press Search to find trains to this destination.
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
          No scheduled timetable data is available for that destination today — it may not be a
          station this feed covers, or today&apos;s timetable may not have been published yet.
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
              <Group key={`${row.uid}-${row.originCrs}-${row.scheduled}`} justify="space-between" wrap="nowrap">
                <Text size="sm">
                  {row.scheduled} · {row.originCrs} → {row.destinationCrs}
                </Text>
                <Group gap="sm" wrap="nowrap">
                  {/* Always safe to render, unlike TrackTrainForm's LDBWS
                      branch: every row here carries a real CIF schedule UID
                      (it is the grouping pass's own key), which is exactly
                      what /train/[uid]/[date] is keyed on. */}
                  <TextLink href={`/train/${encodeURIComponent(row.uid)}/${today}`}>
                    View live status
                  </TextLink>
                  <TrackThisTrainButton
                    uid={row.uid}
                    date={today}
                    attachTicketId={attachTicketId}
                    size="xs"
                  />
                </Group>
              </Group>
            ))}
          </Stack>
        </ScrollArea>
        {/* Only when the server said there IS more. `nextCursor` is an
            explicit null on the last page, so this disappears on its own
            once the day is exhausted -- there is no separate "has more"
            flag to keep in sync. The plain "variant=default" Button is
            deliberate: the visual treatment of Load more is implementation
            /design-review, exactly as the rest of this page's styling is
            (addendum §6). */}
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
        label="Destination station"
        placeholder="e.g. Manchester or MAN"
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={destinationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={destinationCrs.length > 0 && !destinationValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <Autocomplete
        label="Departing from (optional)"
        placeholder="e.g. Euston or EUS"
        description="Any station on the train's route, not just where it started."
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
      <Group grow align="flex-start">
        {/* Plain TextInputs, not @mantine/dates pickers: these are a
            time-of-day range on a fixed (today) date, which DateTimePicker
            cannot express without also asking for a date this feature
            deliberately does not accept. */}
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
      <Group>
        <Button type="submit" disabled={!canSearch}>
          {searching ? 'Searching…' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        {resultsContent()}
      </Stack>
      {/* `component="div"`, not the default `<p>`: `TextLink` renders its
          own Mantine `<Text>` (a `<p>` by default), so wrapping it in an
          ordinary `<Text>` here would nest a `<p>` inside a `<p>` --
          invalid HTML and a React hydration warning (see the same fix's
          rationale in `TicketPanel.tsx`). This is the one spot in this
          component that needs prose text and an inline `TextLink` in the
          same line, so the wrapper tag changes instead of the content. */}
      <Text size="sm" c="dimmed" component="div">
        Can&apos;t find your train? <TextLink href={manualHref}>Track it manually</TextLink> by
        entering its origin station and departure time.
      </Text>
    </Stack>
  );
}
