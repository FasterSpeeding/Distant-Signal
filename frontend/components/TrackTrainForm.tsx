'use client';

import { useEffect, useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Autocomplete, Badge, Button, Group, ScrollArea, Stack, Text } from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import type { TrackPinRequest, TrackPinResponse } from '@/lib/types';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
const OPERATOR_PATTERN = /^[A-Za-z]{2}$/;

/** True unless `destinationCrs` looks like a resolved 3-letter code AND
 * the row's own destination doesn't case-insensitively match it. While
 * the field still holds partial/typed-name text (or is empty), every row
 * matches -- there is nothing on a row to honestly match partial text
 * against (rows carry a CRS code, never a station name). A `null` row
 * destination (CIF only) never matches an *active* filter: "unknown" is
 * not "assume it matches". See
 * docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
 * Decision 1. */
function matchesDestination(rowDestinationCrs: string | null, destinationCrs: string): boolean {
  const trimmed = destinationCrs.trim();
  if (!CRS_PATTERN.test(trimmed)) return true;
  return rowDestinationCrs !== null && rowDestinationCrs.toUpperCase() === trimmed.toUpperCase();
}

/** Same idea for Operator, LDBWS rows only -- CIF rows have no `operator`
 * field at all (the CIF SCHEDULE feed doesn't carry one), so call sites
 * for CIF rows never call this at all, exempting those rows from the
 * Operator filter entirely rather than having them always fail it (which
 * would silently defeat the whole point of the CIF fallback). See the
 * design doc's Decision 1, "CIF/Operator schema asymmetry". */
function matchesOperator(rowOperator: string, operator: string): boolean {
  const trimmed = operator.trim();
  if (!OPERATOR_PATTERN.test(trimmed)) return true;
  return rowOperator.toUpperCase() === trimmed.toUpperCase();
}

/** True unless `scheduledDeparture` is resolved AND the row's own
 * departure time is strictly before it -- filters out departures that
 * have already passed relative to whatever the user has typed/picked,
 * additive alongside `matchesDestination`/`matchesOperator`. Applies to
 * BOTH sources identically (unlike the Destination/Operator split): both
 * `DepartureRow.scheduled` and `ScheduleDepartureRow.scheduled` are the
 * same `"HH:MM"` shape, and neither row type carries its own date (both
 * pickers are always "today", server-side). Combines the row's `"HH:MM"`
 * with *today's* browser-local date into the exact same
 * `'YYYY-MM-DD HH:mm:ss'` string shape `scheduledDeparture` itself holds
 * -- same construction `pickDeparture`/`pickCifDeparture`/the "Now" button
 * already use -- so the two can be compared with a plain string comparison
 * rather than round-tripping through `Date`/UTC (this format sorts
 * lexicographically identical to chronologically, and a round-trip through
 * `Date` risks exactly the kind of local-midnight/DST day-off-by-one this
 * file's `handleSubmit` comment already warns about). A `null`
 * `scheduledDeparture` (not yet resolved) never filters -- matches every
 * row, same "unknown means don't filter" posture as the other two
 * matchers. */
function matchesScheduledDeparture(rowScheduled: string, scheduledDeparture: string | null): boolean {
  if (scheduledDeparture === null) return true;
  const [hh, mm] = rowScheduled.split(':');
  const today = dayjs().format('YYYY-MM-DD');
  const rowDateTime = `${today} ${hh}:${mm}:00`;
  return rowDateTime >= scheduledDeparture;
}

/** Wire shape of `GET /public/stations/{crs}/departures`
 * (`crates/api/src/render.rs::station_departure_json`) -- camelCase
 * mirror of `common::StationDeparture`'s own fields, minus `headcode`
 * (always `None` at the source, never carried through). See
 * docs/superpowers/specs/2026-09-03-trip-search-design.md Decision 2/5. */
interface DepartureRow {
  serviceId: string;
  operator: string;
  destinationCrs: string;
  scheduled: string;
  estimated: string;
  isCancelled: boolean;
  delayMinutes: number;
  cancelReason: string | null;
  delayReason: string | null;
  skippedStations: string[];
}

/** Wire shape of `GET /public/stations/{crs}/schedule-departures`
 * (`crates/api/src/render.rs::schedule_departure_json`) -- deliberately
 * NOT `DepartureRow`: no `operator`, no live running-status fields at all
 * (`isCancelled`/`delayMinutes`/`estimated`/`cancelReason`/`delayReason`),
 * because the CIF SCHEDULE feed genuinely has none of that -- see
 * docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
 * Decision 2/5. `destinationCrs` is nullable: `null` when the terminating
 * TIPLOC has no `stanox_crs` row (a real, if rare, gap). */
interface ScheduleDepartureRow {
  uid: string;
  scheduled: string;
  destinationCrs: string | null;
}

/** `'unavailable'` replaces the old `'not-sampled'` name: it now means
 * neither the LDBWS live board NOR the CIF-derived timetable had data for
 * this station -- see Decision 3/5. */
type Picker =
  | { source: 'ldbws'; rows: DepartureRow[] }
  | { source: 'cif'; rows: ScheduleDepartureRow[] }
  | 'unavailable'
  | null;

/** The v1 entry point for individual train tracking -- a manual form, not
 * a per-departure "track this train" action, per
 * docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md
 * Decision 1 (no public API exposes individual departures today, so a
 * departure-row action can't be built). `initialOrigin` is set by
 * `/track`'s page when arriving via the "Track a train from here" link on
 * `/stations/[crs]` (Decision 1's honest station-page shortcut), OR from
 * `TicketEntryForm`'s own standalone-ticket "next step" link (Part A of the
 * upload-first plan) -- same mechanism, different origin.
 *
 * `attachTicketId`, when given, is a standalone ticket (created via
 * `POST /Train/tickets`, no tracked train yet) the caller is looking for/
 * creating a tracked train for. Once `POST /Train/track` succeeds, this
 * form makes one best-effort follow-up call,
 * `POST /Train/tickets/{attachTicketId}/attach`, before navigating to the
 * new pin's detail page -- if that call fails for any reason (network
 * blip, the ticket having since been attached elsewhere), tracking the
 * train has ALREADY succeeded and this form still navigates on; the ticket
 * just stays standalone and attachable later from the merged trains/tickets
 * list, rather than the whole flow failing over a non-essential follow-up.
 *
 * Submits through the same-origin `/api/Train/track` proxy (Client
 * Components can't read the server-only `API_BASE_URL` env var
 * `lib/api.ts` relies on -- same reasoning as `PinToggle`). Mirrors
 * `PinToggle`'s `needsLogin` 401 pattern, with one deliberate difference:
 * a 401 here does NOT reset the form. `PinToggle` can afford to forget its
 * click (there was no typed input to lose); a four-field form has real
 * input worth protecting, so all four fields stay exactly as typed while
 * the login prompt renders alongside them (Decision 4, "no navigation
 * away"). */
export function TrackTrainForm({
  initialOrigin = '',
  attachTicketId,
}: {
  initialOrigin?: string;
  attachTicketId?: number;
}) {
  const router = useRouter();
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState('');
  const [operator, setOperator] = useState('');
  // Defaults to "now" (the repo owner's own stated expectation), not
  // `null` -- computed once via lazy `useState` initializer, in the exact
  // local-wall-clock `'YYYY-MM-DD HH:mm:ss'` string shape the "Now" button
  // (below) and `pickDeparture`/`pickCifDeparture` already construct, so
  // it round-trips through `handleSubmit`'s own parsing identically to a
  // value the user picked by hand.
  const [scheduledDeparture, setScheduledDeparture] = useState<string | null>(() =>
    dayjs().format('YYYY-MM-DD HH:mm:ss'),
  );
  const [submitting, setSubmitting] = useState(false);
  const needsLoginState = useNeedsLogin();
  const [fieldError, setFieldError] = useState<string | null>(null);

  const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);
  const { suggestions: destinationSuggestions } = useSuggestions(destinationCrs, searchStations);
  const { suggestions: operatorSuggestions } = useSuggestions(operator, searchTocs);
  const [originTouched, setOriginTouched] = useState(false);
  const [picker, setPicker] = useState<Picker>(null);
  // Initialized from `initialOrigin` (not `false`) so a form mounted with
  // an already-valid pre-filled origin shows "Checking for departures…"
  // on the very first paint rather than flashing the `picker === null`
  // "couldn't load" sentence for one render before the effect below runs.
  // Per docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
  // Decision 5.
  const [pickerLoading, setPickerLoading] = useState(() => CRS_PATTERN.test(initialOrigin.trim()));

  const originValid = CRS_PATTERN.test(originCrs.trim());
  const canSubmit = originValid && scheduledDeparture !== null && !submitting;

  // Fetch the live departures picker whenever the origin resolves to a
  // syntactically valid CRS -- same same-origin `/api/*` proxy pattern
  // `searchStations`/`searchTocs` already use (client-safe, no `baseUrl()`
  // import). Per docs/superpowers/specs/2026-09-03-trip-search-design.md
  // Decision 4. Falls back to the CIF-derived schedule-departures picker on
  // a 404, per
  // docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
  // Decision 3.
  useEffect(() => {
    if (!originValid) {
      setPicker(null);
      setPickerLoading(false);
      return;
    }
    const controller = new AbortController();
    const crs = originCrs.trim().toUpperCase();
    setPickerLoading(true);

    fetch(`/api/stations/${crs}/departures`, { signal: controller.signal })
      .then((res) => {
        if (res.status === 404) {
          // Fallback ONLY on 404 -- an LDBWS network blip or 500 must NOT
          // silently swap in the CIF picker; `!res.ok` still maps to `null`
          // exactly as today, leaving the picker absent rather than
          // switching sources on an error condition. Per
          // docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
          // Decision 3.
          return fetch(`/api/stations/${crs}/schedule-departures`, { signal: controller.signal }).then(
            (cifRes) => {
              if (cifRes.status === 404) {
                setPickerLoading(false);
                return setPicker('unavailable');
              }
              if (!cifRes.ok) {
                setPickerLoading(false);
                return setPicker(null);
              }
              return cifRes.json().then((rows: ScheduleDepartureRow[]) => {
                setPickerLoading(false);
                setPicker({ source: 'cif', rows });
              });
            },
          );
        }
        if (!res.ok) {
          setPickerLoading(false);
          return setPicker(null);
        }
        return res.json().then((rows: DepartureRow[]) => {
          setPickerLoading(false);
          setPicker({ source: 'ldbws', rows });
        });
      })
      .catch(() => {
        // Aborted (superseded by a newer origin change) or a genuine
        // network blip -- either way, leave prior `picker` state, same
        // posture as `useSuggestions`. Only flip `pickerLoading` off for a
        // genuine failure of *this* request; an aborted one is about to be
        // superseded by a new effect run that has already set it back to
        // `true`, and unconditionally clearing it here would race that.
        if (!controller.signal.aborted) setPickerLoading(false);
      });
    return () => controller.abort();
  }, [originCrs, originValid]);

  /** Fills Destination/Operator/Scheduled-departure from a picked, real
   * live departure -- without submitting, so the user can still review/
   * edit before tracking. Combines the departure's `"HH:MM"` with *today's*
   * browser-local date into the exact `'YYYY-MM-DD HH:mm:ss'` string shape
   * `scheduledDeparture` already expects -- same construction as the "Now"
   * button above (`dayjs().format('YYYY-MM-DD HH:mm:ss')`), and the same
   * browser-local-date assumption it already makes (not Europe/London
   * specifically) -- not a new limitation this picker introduces. */
  function pickDeparture(row: DepartureRow) {
    setDestinationCrs(row.destinationCrs);
    setOperator(row.operator);
    const [hh, mm] = row.scheduled.split(':');
    const today = dayjs().format('YYYY-MM-DD');
    setScheduledDeparture(`${today} ${hh}:${mm}:00`);
  }

  /** CIF-derived sibling of `pickDeparture` -- fills only
   * Destination/Scheduled-departure. `operator` is left exactly as the user
   * already typed it, never cleared, never guessed -- the CIF SCHEDULE feed
   * has no operator field at all (Decision 2). If `row.destinationCrs` is
   * `null` (the terminating TIPLOC has no `stanox_crs` row), the existing
   * Destination field is left untouched too, for the same "never guess,
   * never clobber with a blank" reason. */
  function pickCifDeparture(row: ScheduleDepartureRow) {
    if (row.destinationCrs !== null) setDestinationCrs(row.destinationCrs);
    const [hh, mm] = row.scheduled.split(':');
    const today = dayjs().format('YYYY-MM-DD');
    setScheduledDeparture(`${today} ${hh}:${mm}:00`);
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSubmit || scheduledDeparture === null) return;
    setSubmitting(true);
    needsLoginState.reset();
    setFieldError(null);
    try {
      // `scheduledDeparture` is the DateTimePicker's own local-wall-clock
      // string, `'YYYY-MM-DD HH:mm:ss'` (@mantine/dates' `assign-time.mjs`
      // formats it via `date.format('YYYY-MM-DD HH:mm:ss')`) -- not ISO
      // 8601. Its first 10 characters are already the local calendar date
      // the user picked, so `service_date` is read directly off the raw
      // string rather than round-tripped through `Date`/UTC, which would
      // give the wrong day for any departure in the first hour after local
      // midnight while the local UTC offset is positive (e.g. BST). The
      // space-separated form also isn't one of the ECMAScript-guaranteed-
      // parseable date formats (only a `T` separator is), so it's
      // normalized to `'YYYY-MM-DDTHH:mm:ss'` before being handed to `Date`
      // for the (correctly UTC) `scheduled_departure` field.
      const serviceDate = scheduledDeparture.slice(0, 10);
      const departure = new Date(scheduledDeparture.replace(' ', 'T'));
      const body: TrackPinRequest = {
        service_date: serviceDate,
        origin_crs: originCrs.trim().toUpperCase(),
        scheduled_departure: departure.toISOString(),
        ...(destinationCrs.trim() ? { destination_crs: destinationCrs.trim().toUpperCase() } : {}),
        ...(operator.trim() ? { operator: operator.trim() } : {}),
      };

      const response = await fetch('/api/Train/track', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        const result: TrackPinResponse = await response.json();
        if (attachTicketId !== undefined) {
          // Best-effort: tracking the train has already succeeded above --
          // don't let a failure here (network blip, the ticket having
          // since been attached elsewhere) block navigating to the new
          // pin. The ticket just stays attachable later if this fails.
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed -- see this block's own comment.
          }
        }
        router.push(`/train/by-id/${result.trackingId}`);
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (response.status === 400) {
        const text = await response.text();
        setFieldError(text || "Couldn't create the tracking pin. Try again.");
        return;
      }
      setFieldError("Couldn't create the tracking pin. Try again.");
    } catch {
      setFieldError("Couldn't create the tracking pin. Try again.");
    } finally {
      setSubmitting(false);
    }
  }

  /** The picker container's content, in the priority order documented in
   * docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
   * Decision 4 -- exactly one of six mutually-exclusive states, checked
   * top to bottom. Rows are filtered by `matchesDestination`/
   * `matchesOperator` (Decision 1), and additionally by
   * `matchesScheduledDeparture` (both sources -- see that function's own
   * doc comment) before rendering; a source whose *unfiltered* result was
   * already empty (state 5 below) is distinguished from one that had rows
   * but none survived filtering (the two new sentences inside the
   * `'ldbws'`/`'cif'` branches) -- different honest meanings, different
   * copy. */
  function pickerContent() {
    if (!originValid) {
      return (
        <Text size="sm" c="dimmed">
          Enter an origin station above to see upcoming departures.
        </Text>
      );
    }
    if (pickerLoading) {
      return (
        <Text size="sm" c="dimmed">
          Checking for departures…
        </Text>
      );
    }
    if (picker === null) {
      return (
        <Text size="sm" c="dimmed">
          Couldn&apos;t load departures for this station right now — enter the details below.
        </Text>
      );
    }
    if (picker === 'unavailable') {
      return (
        <Text size="sm" c="dimmed">
          No departure information is available for this station — enter the details below.
        </Text>
      );
    }
    if (picker.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No live departures currently on the board for this station right now.
        </Text>
      );
    }
    if (picker.source === 'ldbws') {
      const filtered = picker.rows.filter(
        (row) =>
          matchesDestination(row.destinationCrs, destinationCrs) &&
          matchesOperator(row.operator, operator) &&
          matchesScheduledDeparture(row.scheduled, scheduledDeparture),
      );
      if (filtered.length === 0) {
        return (
          <Text size="sm" c="dimmed">
            No upcoming departures match the destination and/or operator you&apos;ve entered.
          </Text>
        );
      }
      return (
        <ScrollArea mah={220} offsetScrollbars>
          <Stack gap="xs">
            {filtered.map((row) => {
              const clickable = !row.isCancelled;
              const badge = row.isCancelled ? (
                <Badge color="red">Cancelled</Badge>
              ) : row.delayMinutes > 0 ? (
                <Badge color="orange">+{row.delayMinutes} min</Badge>
              ) : (
                <Badge color="green">On time</Badge>
              );
              return (
                <Group
                  key={row.serviceId}
                  justify="space-between"
                  wrap="nowrap"
                  role={clickable ? 'button' : undefined}
                  tabIndex={clickable ? 0 : undefined}
                  onClick={clickable ? () => pickDeparture(row) : undefined}
                  onKeyDown={
                    clickable
                      ? (event) => {
                          if (event.key === 'Enter' || event.key === ' ') pickDeparture(row);
                        }
                      : undefined
                  }
                  style={{ cursor: clickable ? 'pointer' : 'default', opacity: clickable ? 1 : 0.6 }}
                >
                  <Text size="sm">
                    {row.scheduled} · {row.destinationCrs} · {row.operator}
                  </Text>
                  {badge}
                </Group>
              );
            })}
          </Stack>
        </ScrollArea>
      );
    }
    // picker.source === 'cif' -- Operator never filters this source
    // (Decision 1's CIF/Operator asymmetry): `matchesOperator` is simply
    // never called here.
    const filtered = picker.rows.filter(
      (row) =>
        matchesDestination(row.destinationCrs, destinationCrs) &&
        matchesScheduledDeparture(row.scheduled, scheduledDeparture),
    );
    return (
      <>
        <Text size="sm" c="dimmed">
          Live departure boards aren&apos;t available for this station. Showing the scheduled timetable instead
          — this is not live running information and may be up to 30 minutes out of date.
        </Text>
        {filtered.length === 0 ? (
          <Text size="sm" c="dimmed">
            No upcoming scheduled departures match the destination you&apos;ve entered.
          </Text>
        ) : (
          <ScrollArea mah={220} offsetScrollbars>
            <Stack gap="xs">
              {filtered.map((row) => (
                <Group
                  key={row.uid}
                  justify="space-between"
                  wrap="nowrap"
                  role="button"
                  tabIndex={0}
                  onClick={() => pickCifDeparture(row)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') pickCifDeparture(row);
                  }}
                  style={{ cursor: 'pointer' }}
                >
                  <Text size="sm">
                    {row.scheduled}
                    {row.destinationCrs ? ` · ${row.destinationCrs}` : ''}
                  </Text>
                </Group>
              ))}
            </Stack>
          </ScrollArea>
        )}
      </>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <Autocomplete
        label="Origin station"
        placeholder="e.g. Woking or WOK"
        value={originCrs}
        onChange={setOriginCrs}
        onBlur={() => setOriginTouched(true)}
        data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = originSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={originTouched && originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <Group align="flex-end" gap="xs">
        <DateTimePicker
          label="Scheduled departure"
          placeholder="Pick date and time"
          value={scheduledDeparture}
          onChange={setScheduledDeparture}
          // The backend rejects a departure more than 6 hours in the past
          // (`crates/api/src/data/train_tracking.rs`'s `MAX_PIN_AGE`) --
          // this hint is here so a rejection is rare rather than the
          // user's first encounter with the rule, per Decision 1.
          description="Must be within the last 6 hours, or any time in the future"
          required
          style={{ flexGrow: 1 }}
        />
        {/* `@mantine/dates`' own `presets` prop (9.5.2) only ever assigns a
            *date* (`DatePickerPreset['value']` is a bare `DateStringValue`,
            like `DatePicker`'s "Today"/"Yesterday" presets) -- it has no
            way to also fill in a time-of-day, so it can't produce "right
            now" on its own; a plain Button next to the picker is the clean
            fit here instead. `dayjs().format('YYYY-MM-DD HH:mm:ss')`
            deliberately matches the exact local-wall-clock string shape
            the picker itself produces (`assign-time.mjs`'s own
            `date.format('YYYY-MM-DD HH:mm:ss')`) -- see this file's own
            `handleSubmit` comment on why that shape, not an ISO string,
            is required to avoid an around-local-midnight day-off-by-one. */}
        <Button variant="default" onClick={() => setScheduledDeparture(dayjs().format('YYYY-MM-DD HH:mm:ss'))}>
          Now
        </Button>
      </Group>
      <Autocomplete
        label="Destination station (optional)"
        placeholder="e.g. Woking or WOK"
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={destinationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
      />
      <Autocomplete
        label="Operator (optional)"
        placeholder="e.g. SW"
        value={operator}
        onChange={setOperator}
        data={operatorSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = operatorSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
      />
      {/* Always present -- never absent from the DOM, per
          docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
          Decision 4. `mih={72}` blunts the size jump between the
          one/two-line text states; row-list states remain bounded by
          `ScrollArea`'s own `mah={220}` and can legitimately grow past
          the minimum. */}
      <Stack gap="xs" mih={72}>
        {pickerContent()}
      </Stack>
      {fieldError && (
        <Alert color="red" title="Couldn't track this train">
          {fieldError}
        </Alert>
      )}
      <Group>
        <Button type="submit" disabled={!canSubmit}>
          {submitting ? 'Tracking…' : 'Track this train'}
        </Button>
      </Group>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this train.
      </LoginPromptModal>
    </Stack>
  );
}
