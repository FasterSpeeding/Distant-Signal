'use client';

import { useEffect, useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Autocomplete, Button, Group, Stack, Text } from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { ScheduleRow } from './ScheduleRow';
import { TextLink } from './TextLink';
import { TrackDestinationModal } from './TrackDestinationModal';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { useGroupSummaries } from '@/lib/useGroupSummaries';
import { shareTrackedTrainToGroup } from '@/lib/shareTrackedTrain';
import { noMatchOptionContent, withNoMatchPlaceholder } from '@/lib/autocompleteNoMatch';
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

/** How many hours a same-day combination of an LDBWS `DepartureRow.scheduled`
 * "HH:MM" with TODAY's date can land before real wall-clock "now" before
 * `resolveLdbwsDepartureDate` (below) judges it to actually be tomorrow
 * instead. `DepartureRow` carries no date/day-offset field at all (see its
 * own doc comment), so the only signal available to tell "a genuinely
 * same-day departure that's already left" apart from "a departure that's
 * rolled past local midnight and is really tomorrow" is how far in the past
 * the naive same-day combination lands. `crates/poller-ldbws/src/main.rs`'s
 * `fetch_departures`/`fetch_departures_once` calls Darwin's
 * `GetDepBoardWithDetails` with only `numRows` set -- no `timeOffset`/
 * `timeWindow` override -- so every row on the board is governed by
 * Darwin's own default near-term look-ahead window (on the order of ~2
 * hours): a live board never shows a departure that has already left, and
 * never reaches further into the future than that window either. `4`
 * comfortably clears that ~2-hour window with margin for clock skew between
 * the browser and Darwin/the poller and for the board being briefly stale
 * by the time it's rendered/clicked, while staying well short of a full
 * 24h so a genuinely-stale same-day row is never misread as "tomorrow"
 * just because it's a few hours old. Mirrors the reasoning
 * `poller-ldbws::schema::compute_delay_minutes` already applies to these
 * same `std`/`etd` fields for its own midnight-wraparound handling (see
 * that function's doc comment) -- same bare-"HH:MM"-no-date shape, same
 * data source, same underlying fix, just applied client-side here since
 * `DepartureRow` has no day-offset field to compute server-side. */
const LDBWS_PAST_THRESHOLD_HOURS = 4;

/** How many days past `now`'s own calendar date an LDBWS `DepartureRow`'s
 * bare `"HH:MM"` `scheduled` genuinely falls on: `0` (today) unless
 * combining it with today's date would land more than
 * `LDBWS_PAST_THRESHOLD_HOURS` hours in the PAST relative to `now`, in
 * which case it's `1` (tomorrow) -- see that constant's own doc comment for
 * why that threshold. Only ever corrects forward, never backward: a live
 * upcoming-departures board never shows an already-passed departure, so
 * there is no valid scenario where a row is really YESTERDAY relative to
 * `now` -- unlike CIF's `dayOffset` (a known fact carried on the row
 * itself), this is inferred, and inference only ever needs to go one
 * direction here. Deliberately compares against `now` (real wall-clock
 * time), not whatever `scheduledDeparture` the user may have already typed
 * -- the two are unrelated: this answers "what calendar day is this row
 * really on", not "has the user's chosen time already passed it". */
function ldbwsDayOffset(scheduled: string, now: dayjs.Dayjs): 0 | 1 {
  const [hh, mm] = scheduled.split(':');
  const sameDay = dayjs(`${now.format('YYYY-MM-DD')} ${hh}:${mm}:00`);
  return now.diff(sameDay, 'hour', true) > LDBWS_PAST_THRESHOLD_HOURS ? 1 : 0;
}

/** Resolves the real calendar date (`'YYYY-MM-DD'`) an LDBWS
 * `DepartureRow.scheduled` "HH:MM" falls on, relative to `now` -- `now.add(
 * ldbwsDayOffset(scheduled, now), 'day')`, formatted. See `ldbwsDayOffset`'s
 * own doc comment for the reasoning; this is the piece `pickDeparture` and
 * the LDBWS branch of `matchesScheduledDeparture`'s filtering both need,
 * factored out so they can't drift out of sync with each other. */
function resolveLdbwsDepartureDate(scheduled: string, now: dayjs.Dayjs): string {
  return now.add(ldbwsDayOffset(scheduled, now), 'day').format('YYYY-MM-DD');
}

/** True unless `scheduledDeparture` is resolved AND the row's own
 * departure time is strictly before it -- filters out departures that
 * have already passed relative to whatever the user has typed/picked,
 * additive alongside `matchesDestination`/`matchesOperator`. Applies to
 * BOTH sources identically (unlike the Destination/Operator split): both
 * `DepartureRow.scheduled` and `ScheduleDepartureRow.scheduled` are the
 * same `"HH:MM"` shape. Combines the row's `"HH:MM"` with `rowDayOffset`
 * days past *today's* browser-local date into the exact same
 * `'YYYY-MM-DD HH:mm:ss'` string shape `scheduledDeparture` itself holds
 * -- same construction `pickDeparture`/`pickCifDeparture`/the "Now" button
 * already use -- so the two can be compared with a plain string comparison
 * rather than round-tripping through `Date`/UTC (this format sorts
 * lexicographically identical to chronologically, and a round-trip through
 * `Date` risks exactly the kind of local-midnight/DST day-off-by-one this
 * file's `handleSubmit` comment already warns about). A `null`
 * `scheduledDeparture` (not yet resolved) never filters -- matches every
 * row, same "unknown means don't filter" posture as the other two
 * matchers.
 *
 * `rowDayOffset` defaults to `0`. Two call sites, two different ways of
 * arriving at a value: the CIF call site passes a real, known
 * `row.dayOffset` -- see `ScheduleDepartureRow.dayOffset`'s own doc comment
 * for why CIF rows, and only CIF rows, can carry one straight from the
 * server. The LDBWS call site has no such field to read (`DepartureRow`
 * carries no day-offset concept at all -- Darwin has no CIF-schedule
 * linkage to derive one from), so it instead passes an INFERRED value, via
 * `ldbwsDayOffset` -- see that function's own doc comment for the
 * midnight-wraparound heuristic behind it. Without a nonzero offset from
 * either source, a genuinely-future post-midnight row (CIF `dayOffset: 1`,
 * or an LDBWS row `ldbwsDayOffset` infers as tomorrow -- e.g. `00:07`
 * viewed at 23:50) would compare as "today 00:07", read as already-passed
 * relative to a same-day `scheduledDeparture`, and be silently filtered out
 * of the picker entirely -- never even reachable to click, regardless of
 * how `pickCifDeparture`/`pickDeparture` themselves compute the date once
 * picked. */
function matchesScheduledDeparture(
  rowScheduled: string,
  scheduledDeparture: string | null,
  rowDayOffset = 0,
): boolean {
  if (scheduledDeparture === null) return true;
  const [hh, mm] = rowScheduled.split(':');
  const date = dayjs().add(rowDayOffset, 'day').format('YYYY-MM-DD');
  const rowDateTime = `${date} ${hh}:${mm}:00`;
  return rowDateTime >= scheduledDeparture;
}

/** Wire shape of `GET /public/stations/{crs}/departures`
 * (`crates/api/src/render.rs::station_departure_json`) -- camelCase
 * mirror of `common::StationDeparture`'s own fields, minus `headcode`
 * (always `None` at the source, never carried through). See
 * docs/superpowers/specs/2026-09-03-trip-search-design.md Decision 2/5.
 * Deliberately carries no `uid`: LDBWS/Darwin has no concept of the CIF
 * schedule UID the public `/train/[uid]/[date]` page is keyed on --
 * `serviceId` below is Darwin's own RID-based identifier, a different
 * scheme entirely, not a substitute. That's why this source's row
 * rendering (`pickerContent`, `'ldbws'` branch) has no "View live status"
 * link even though the `'cif'` branch does. */
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
  // See `JourneyStop.platform`/`plannedPlatform`/`platformChanged`'s own
  // doc comments in `lib/types.ts` -- same fields, same meaning, this
  // route's own copy of them (`station_departure_json`).
  platform: string | null;
  plannedPlatform: string | null;
  platformChanged: boolean;
}

/** Wire shape of `GET /public/stations/{crs}/schedule-departures`
 * (`crates/api/src/render.rs::schedule_departure_json`) -- deliberately
 * NOT `DepartureRow`: no `operator`, no live running-status fields at all
 * (`isCancelled`/`delayMinutes`/`estimated`/`cancelReason`/`delayReason`),
 * because the CIF SCHEDULE feed genuinely has none of that -- see
 * docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
 * Decision 2/5. `destinationCrs` is nullable: `null` when the terminating
 * TIPLOC has no `stanox_crs` row (a real, if rare, gap).
 *
 * `dayOffset` is how many calendar days past this route's own "today" (the
 * server always resolves this endpoint's `service_date` to today, see that
 * route's own doc comment) `scheduled` actually falls on -- mirrors
 * `schedule_query::CallingPoint::day_offset` verbatim. Needed because a
 * bare `"HH:MM"` string carries no day information of its own: a real
 * overnight CIF schedule can have a calling point booked at, say, `00:07`
 * that is genuinely TOMORROW relative to when the schedule itself started
 * (see `schedule_query::resolve`'s own `f49687_raw` doc comment for the
 * live-confirmed c2c Liverpool Street -> Shoeburyness example this exists
 * for). Almost always `0`. `pickCifDeparture` and `matchesScheduledDeparture`
 * both use this to compute the row's REAL calendar date instead of always
 * assuming "today" -- see their own doc comments. */
interface ScheduleDepartureRow {
  uid: string;
  scheduled: string;
  dayOffset: number;
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
 * away").
 *
 * Shared-groups follow-up: `useGroupSummaries()` decides, once on mount,
 * whether this user belongs to any group. Zero groups (the majority case,
 * and every anonymous visitor) leaves `handleSubmit`'s form `onSubmit`
 * calling `submitTrack(null)` directly -- no prompt, no behavior change
 * from before this feature existed. One or more groups instead makes
 * `handleSubmit` open `TrackDestinationModal` first and defer the actual
 * submit to its `onConfirm`, which calls this same `submitTrack` -- see
 * that component's own doc comment for why reusing it unmodified (rather
 * than a second, divergent submit path) is safe. Sharing the new pin into
 * the chosen group (`shareTrackedTrainToGroup`) is a further best-effort
 * follow-up performed only once the track call itself has already
 * succeeded, same swallow-every-failure posture as the `attachTicketId`
 * block right below it. */
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
  // Darwin's own explicit skipped-calling-point snapshot for whichever
  // live departure-board row the user picked (`pickDeparture`, below) --
  // carried through to the pin so the journey timeline can eventually key
  // its "Skipped" treatment off it (`common::TrackPinRequest.skipped_stations`'s
  // own doc comment). `[]` (never sent) until an LDBWS row is actually
  // picked -- the CIF-picker/manual-entry paths have no such signal at
  // all.
  const [skippedStations, setSkippedStations] = useState<string[]>([]);
  // Darwin's own platform snapshot for whichever live departure-board row
  // the user picked (`pickDeparture`, below) -- carried through to the pin
  // so the journey timeline's origin stop can eventually show it (see
  // `common::TrackPinRequest.platform`/`planned_platform`'s own doc
  // comments). `null` (never sent) until an LDBWS row is actually picked --
  // same posture as `skippedStations` immediately above.
  const [platform, setPlatform] = useState<string | null>(null);
  const [plannedPlatform, setPlannedPlatform] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const needsLoginState = useNeedsLogin();
  const [fieldError, setFieldError] = useState<string | null>(null);
  const { groups } = useGroupSummaries();
  const [destinationPromptOpened, setDestinationPromptOpened] = useState(false);

  const { suggestions: originSuggestions, loading: originSuggestionsLoading } = useSuggestions(
    originCrs,
    searchStations,
  );
  const { suggestions: destinationSuggestions, loading: destinationSuggestionsLoading } = useSuggestions(
    destinationCrs,
    searchStations,
  );
  const { suggestions: operatorSuggestions, loading: operatorSuggestionsLoading } = useSuggestions(
    operator,
    searchTocs,
  );
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
   * edit before tracking. Combines the departure's `"HH:MM"` with its
   * REAL browser-local calendar date, via `resolveLdbwsDepartureDate`, into
   * the exact `'YYYY-MM-DD HH:mm:ss'` string shape `scheduledDeparture`
   * already expects -- same construction as the "Now" button above
   * (`dayjs().format('YYYY-MM-DD HH:mm:ss')`), and the same
   * browser-local-date assumption it already makes (not Europe/London
   * specifically) -- not a new limitation this picker introduces.
   *
   * Not always literally "today": a row picked from a live board viewed
   * near local midnight can genuinely be tomorrow (e.g. viewing the board
   * at 23:50 and picking a `"00:07"` row -- a real, near-term, 17-minutes-
   * away departure, not something ~23h43m in the past) --
   * `resolveLdbwsDepartureDate`'s own doc comment explains the heuristic
   * this relies on to tell that case apart from a normal same-day pick. */
  function pickDeparture(row: DepartureRow) {
    setDestinationCrs(row.destinationCrs);
    setOperator(row.operator);
    const [hh, mm] = row.scheduled.split(':');
    const date = resolveLdbwsDepartureDate(row.scheduled, dayjs());
    setScheduledDeparture(`${date} ${hh}:${mm}:00`);
    setSkippedStations(row.skippedStations);
    setPlatform(row.platform);
    setPlannedPlatform(row.plannedPlatform);
  }

  /** CIF-derived sibling of `pickDeparture` -- fills only
   * Destination/Scheduled-departure. `operator` is left exactly as the user
   * already typed it, never cleared, never guessed -- the CIF SCHEDULE feed
   * has no operator field at all (Decision 2). If `row.destinationCrs` is
   * `null` (the terminating TIPLOC has no `stanox_crs` row), the existing
   * Destination field is left untouched too, for the same "never guess,
   * never clobber with a blank" reason.
   *
   * Adds `row.dayOffset` days to *today's* browser-local date, rather than
   * always assuming "today" the way `pickDeparture` (LDBWS, below) still
   * does -- a post-midnight CIF calling point (`dayOffset: 1`, e.g. `00:07`)
   * is genuinely TOMORROW relative to when the search itself ran, and
   * combining it with bare "today" would create a pin dated the WRONG
   * calendar day (see `ScheduleDepartureRow.dayOffset`'s own doc comment).
   * `pickDeparture` (LDBWS, above) has the analogous fix,
   * `resolveLdbwsDepartureDate` -- `DepartureRow` still carries no day-offset
   * FIELD (Darwin's live board has no CIF-schedule linkage to derive one
   * from), but unlike here, where the day offset is a known fact read
   * straight off the row, LDBWS instead INFERS it from a bounded-look-ahead
   * heuristic -- see that function's own doc comment for why that's
   * reliable for this specific data source. */
  function pickCifDeparture(row: ScheduleDepartureRow) {
    if (row.destinationCrs !== null) setDestinationCrs(row.destinationCrs);
    // The CIF SCHEDULE feed has no per-service skip signal at all (Decision
    // 2) -- clears any snapshot a previously-picked LDBWS row may have left
    // behind, so switching pickers can't carry a stale skip list onto a
    // different service. Same reasoning for platform: CIF has no live
    // platform signal either.
    setSkippedStations([]);
    setPlatform(null);
    setPlannedPlatform(null);
    const [hh, mm] = row.scheduled.split(':');
    // `?? 0`: defends against an old `api` pod (a separate Helm Deployment,
    // rolled independently of `frontend`) omitting `dayOffset` from the JSON
    // entirely during a rollout, which would otherwise reach dayjs as
    // `undefined` and produce an Invalid Date.
    const date = dayjs().add(row.dayOffset ?? 0, 'day').format('YYYY-MM-DD');
    setScheduledDeparture(`${date} ${hh}:${mm}:00`);
  }

  /** The real submit -- does the `POST /api/Train/track` call and every
   * follow-up, exactly as this form always has. `groupId` is `null` for the
   * plain personal-tracking flow (the ONLY value it's ever called with when
   * `groups` is empty, preserving today's exact behavior), or a chosen
   * group's id from `TrackDestinationModal`'s `onConfirm`. */
  async function submitTrack(groupId: string | null) {
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
        ...(skippedStations.length > 0 ? { skipped_stations: skippedStations } : {}),
        ...(platform !== null ? { platform } : {}),
        ...(plannedPlatform !== null ? { planned_platform: plannedPlatform } : {}),
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
        if (groupId !== null) {
          // Best-effort, same posture as the ticket-attach block above --
          // see shareTrackedTrainToGroup's own doc comment.
          await shareTrackedTrainToGroup(groupId, result.trackingId);
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

  /** The form's own `onSubmit` -- zero groups calls `submitTrack` directly
   * (today's exact behavior, unchanged); one or more groups opens
   * `TrackDestinationModal` instead and defers the real submit to its
   * `onConfirm`. See this component's own doc comment.
   *
   * Task 3.6.14: the submit `Button` below used to stay `disabled` the
   * whole time `!canSubmit` held (i.e. before a valid origin/departure was
   * entered at all), which Mantine renders as light-grey-on-slightly
   * -lighter-grey in dark mode -- under 2:1, and unfixable by a `variant`
   * change alone, since Mantine's own disabled style
   * (`@mantine/core/styles.css`'s `[data-disabled]` rule) unconditionally
   * overrides EVERY variant's background/text/border with that same pair
   * of tokens. Rather than hand-patching `--mantine-color-disabled(-color)`
   * app-wide for one button, this takes the plan's other sanctioned
   * fix: the button now only disables while a submit is actually in
   * flight (`submitting`, a brief, self-explanatory state with its own
   * "Tracking…" label) -- an invalid press instead falls through to here
   * and surfaces the same `fieldError` `Alert` a failed backend
   * validation already renders, so a click always gets a visible result
   * instead of silently doing nothing behind an near-invisible control. */
  function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (submitting) return;
    if (!originValid || scheduledDeparture === null) {
      setFieldError(
        !originValid
          ? 'Enter a valid origin station before tracking — pick one from the suggestions, or a 3-letter CRS code.'
          : 'Pick a scheduled departure before tracking.',
      );
      return;
    }
    setFieldError(null);
    if (groups.length > 0) {
      setDestinationPromptOpened(true);
      return;
    }
    void submitTrack(null);
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
   * copy.
   *
   * Neither row-list branch below is wrapped in a `ScrollArea`, and that is
   * load-bearing. Both used to be (`<ScrollArea mah={220}
   * offsetScrollbars>`), which did not scroll -- it hard-clipped. A Mantine
   * `ScrollArea` root is `position: relative; overflow: hidden`
   * (`@mantine/core/styles/ScrollArea.css`, `.m_d57069b5`) while its
   * viewport is `height: 100%`. With only `mah` on the root, the root's own
   * `height` stays `auto`, so that `100%` resolves to `auto` too (CSS 2.1
   * §10.5: a percentage height against a content-sized containing block
   * computes to `auto`): the viewport grew to its full content height and
   * so never overflowed *itself* -- nothing scrolled -- while the root
   * clamped to 220px and hid the rest behind `overflow: hidden`. Mantine's
   * own scrollbar is sized from `scrollHeight` vs `clientHeight`, equal
   * here, so it never appeared to hint at it either, and the native one is
   * suppressed (`scrollbar-width: none`).
   *
   * That is worse here than in the two list pages with the same defect
   * (`IncidentSearchForm.tsx`, since fixed, and `TrainSearchForm.tsx`):
   * these rows are `role="button"` pickers, not text. A row is ~30px of
   * pitch (a `size="sm"` line -- 14px at Mantine's `--mantine-line-height-
   * sm: 1.45`, so ~20px -- plus the `Stack`'s 10px `xs` gap), so the 220px
   * cap landed after about seven of them, and every row past that was
   * unselectable -- unreachable by pointer and by wheel, and reachable by
   * keyboard only into a dead end (a browser does scroll an `overflow:
   * hidden` box to reveal a focused descendant, which parked the box at an
   * offset the user had no gesture to undo, hiding the *earlier* rows
   * instead). Silently, too: the rows were in the DOM and in the a11y tree,
   * so nothing said a departure had been hidden.
   *
   * `ScrollArea.Autosize` IS the Mantine component that supports a max
   * height (it wraps the root in a `display: flex` / `flex: 1` /
   * `overflow: hidden` chain, which is what makes the root's height
   * definite), but a bounded scroller isn't wanted here anyway: this picker
   * is rendered in the page flow inside the form (see the `mih={72}`
   * `Stack` below), not in a popover or a dropdown, so there is no
   * containing box it has to fit. Both sources publish about 10 rows in
   * practice -- CIF by a hard cap (`schedule-reference`'s
   * `MAX_DEPARTURES_PER_STATION = 10`, truncated before publication, which
   * `schedule_network_departures`' own migration header records as
   * "next-10, now-forward-filtered"), LDBWS only by the DEFAULT of
   * `poller-ldbws`'s `--num-rows` flag, which an operator can raise (see
   * `crates/poller-ldbws/src/main.rs`, which explicitly contemplates a
   * configured value "much larger than 10"). So ~290px at worst today,
   * barely past the 220px cap it replaces -- and, unlike that cap, an
   * LDBWS board configured longer degrades into more page to scroll
   * rather than into hidden departures. */
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
      // `ldbwsDayOffset`, not a bare `0`: without it, a near-midnight row
      // that's really tomorrow (see `ldbwsDayOffset`'s own doc comment)
      // would compare as "today HH:MM", read as already-passed relative to
      // a same-day `scheduledDeparture`, and be silently filtered out here
      // before it could ever be reached to click -- the same exposure
      // `matchesScheduledDeparture`'s own doc comment already documents for
      // the CIF side.
      const now = dayjs();
      const filtered = picker.rows.filter(
        (row) =>
          matchesDestination(row.destinationCrs, destinationCrs) &&
          matchesOperator(row.operator, operator) &&
          matchesScheduledDeparture(row.scheduled, scheduledDeparture, ldbwsDayOffset(row.scheduled, now)),
      );
      if (filtered.length === 0) {
        return (
          <Text size="sm" c="dimmed">
            No upcoming departures match the destination and/or operator you&apos;ve entered.
          </Text>
        );
      }
      return (
        // Deliberately NOT wrapped in a `ScrollArea` -- see
        // `pickerContent`'s own doc comment for the full reasoning. This
        // branch's rows are `role="button"` pickers, so the clipping this
        // used to cause made them literally unselectable.
        //
        // Rows render via the shared `ScheduleRow` component (not
        // hand-rolled here) -- see that component's own doc comment for
        // the AA-contrast reasoning behind dimming only the row's TEXT,
        // never its status badge, for a non-clickable (cancelled) row.
        <Stack gap="xs" data-departure-picker-rows>
          {filtered.map((row) => (
            <ScheduleRow
              key={row.serviceId}
              row={{
                key: row.serviceId,
                scheduled: row.scheduled,
                destinationCrs: row.destinationCrs,
                operator: row.operator,
                isCancelled: row.isCancelled,
                delayMinutes: row.delayMinutes,
                platform: row.platform,
                plannedPlatform: row.plannedPlatform,
                platformChanged: row.platformChanged,
              }}
              onSelect={() => pickDeparture(row)}
            />
          ))}
        </Stack>
      );
    }
    // picker.source === 'cif' -- Operator never filters this source
    // (Decision 1's CIF/Operator asymmetry): `matchesOperator` is simply
    // never called here.
    const filtered = picker.rows.filter(
      (row) =>
        matchesDestination(row.destinationCrs, destinationCrs) &&
        matchesScheduledDeparture(row.scheduled, scheduledDeparture, row.dayOffset),
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
          // Deliberately NOT wrapped in a `ScrollArea` -- see
          // `pickerContent`'s own doc comment, and the LDBWS branch's own
          // copy of this note. Same `role="button"` rows, same defect.
          <Stack gap="xs" data-departure-picker-rows>
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
                {/* A secondary action, deliberately separate from the row's
                    own click-to-select behaviour above: this navigates to
                    the train's own public status page
                    (`/train/[uid]/[date]`) WITHOUT filling/submitting the
                    tracking form at all, for a visitor who just wants to
                    look, not track. `row.uid` is a real CIF schedule UID
                    here (unlike the LDBWS branch above, whose
                    `DepartureRow` carries no train UID at all -- Darwin's
                    `serviceId` is a different identifier scheme entirely,
                    and the public page is keyed on the CIF/TRUST one --
                    so that branch has nothing honest to link this action
                    to and doesn't render it).

                    `stopPropagation` on BOTH handlers, not just `onClick`:
                    this link sits inside the row's own `role="button"`
                    `onClick`/`onKeyDown`, so without it, either activation
                    path would ALSO select the row for tracking --
                    a plain click bubbles up to the row's `onClick`, and an
                    Enter/Space keydown on the focused link bubbles up to
                    the row's `onKeyDown` before the browser's own
                    synthesized click on the anchor even fires. The link
                    keeps its own native Enter-to-follow behaviour and
                    remains a normal, independently tab-reachable focus
                    stop -- only the *bubbling into the row* is stopped.

                    The linked date is `row.dayOffset` days past today, not
                    a single hoisted "today" shared by every row -- the
                    public train page is keyed by `(train_uid,
                    service_date)`, and a post-midnight row's real
                    service_date is tomorrow, not today (same reasoning as
                    `pickCifDeparture` itself). */}
                <TextLink
                  href={`/train/${encodeURIComponent(row.uid)}/${dayjs()
                    .add(row.dayOffset ?? 0, 'day')
                    .format('YYYY-MM-DD')}`}
                  onClick={(event) => event.stopPropagation()}
                  onKeyDown={(event) => event.stopPropagation()}
                >
                  View live status
                </TextLink>
              </Group>
            ))}
          </Stack>
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
        // `withNoMatchPlaceholder`: `Autocomplete` has no
        // `nothingFoundMessage` prop in this Mantine version -- see
        // `lib/autocompleteNoMatch.ts`.
        data={withNoMatchPlaceholder(
          originSuggestions.map((s) => ({ value: s.code, label: s.code })),
          'No matching stations',
          { active: originCrs.trim().length > 0 && !originSuggestionsLoading },
        )}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const placeholder = noMatchOptionContent(option.value, 'No matching stations');
          if (placeholder) return placeholder;
          const match = originSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={originTouched && originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
        // NOT the native `required` attribute (Task 3.6.14): an empty
        // origin is now validated by `handleSubmit` itself, which sets
        // `fieldError` and returns before ever calling `submitTrack` --
        // see that function's own doc comment. A native `required` field
        // would make the browser's own constraint validation intercept
        // the submit event before `handleSubmit` ever runs, silently
        // replacing that explanatory message with (at best) a native
        // validation bubble the button's near-invisible disabled state
        // was already standing in for.
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
          // Same reasoning as the Origin field just above -- a cleared
          // departure is validated by `handleSubmit` itself now, not by
          // native `required` constraint validation.
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
        data={withNoMatchPlaceholder(
          destinationSuggestions.map((s) => ({ value: s.code, label: s.code })),
          'No matching stations',
          { active: destinationCrs.trim().length > 0 && !destinationSuggestionsLoading },
        )}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const placeholder = noMatchOptionContent(option.value, 'No matching stations');
          if (placeholder) return placeholder;
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
      />
      <Autocomplete
        label="Operator (optional)"
        placeholder="e.g. SW"
        value={operator}
        onChange={setOperator}
        data={withNoMatchPlaceholder(
          operatorSuggestions.map((s) => ({ value: s.code, label: s.code })),
          'No matching operators',
          { active: operator.trim().length > 0 && !operatorSuggestionsLoading },
        )}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const placeholder = noMatchOptionContent(option.value, 'No matching operators');
          if (placeholder) return placeholder;
          const match = operatorSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
      />
      {/* Always present -- never absent from the DOM, per
          docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
          Decision 4. `mih={72}` blunts the size jump between the
          one/two-line text states; row-list states can legitimately grow
          past the minimum and are deliberately given no maximum -- see
          `pickerContent`'s own doc comment for why the `mah`-capped
          `ScrollArea` that used to bound them was a clip, not a scroller,
          and why nothing replaced it. */}
      <Stack gap="xs" mih={72}>
        {pickerContent()}
      </Stack>
      {fieldError && (
        <Alert color="red" title="Couldn't track this train">
          {fieldError}
        </Alert>
      )}
      <Group>
        {/* Disabled only while a submit is in flight -- see
            `handleSubmit`'s own doc comment (Task 3.6.14) for why an
            invalid-but-not-yet-submitted form no longer disables this
            button at all. */}
        <Button type="submit" disabled={submitting}>
          {submitting ? 'Tracking…' : 'Track this train'}
        </Button>
      </Group>
      <TrackDestinationModal
        opened={destinationPromptOpened}
        groups={groups}
        onClose={() => setDestinationPromptOpened(false)}
        onConfirm={(groupId) => void submitTrack(groupId)}
      />
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this train.
      </LoginPromptModal>
    </Stack>
  );
}
