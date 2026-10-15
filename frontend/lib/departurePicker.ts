import dayjs from 'dayjs';
import { addCalendarDays, nowInLondon, londonToday, LONDON_TZ } from '@/lib/londonWallClock';
import type { BoardCallingPoint, LineTrainSummaryLive } from '@/lib/types';

/** The track form's departure picker: the two wire shapes it lists (the
 * live board and the timetable), the filters that narrow them, and the
 * date arithmetic that turns a bare "HH:MM" into a London wall-clock
 * departure. Split out of `TrackTrainForm.tsx`, which owns the form state;
 * `DeparturePicker.tsx` renders the rows. */

export const CRS_PATTERN = /^[A-Za-z]{3}$/;
const OPERATOR_PATTERN = /^[A-Za-z]{2}$/;

/** How many days past London's today a pin's service date may be. Mirrors
 * the backend's `PIN_MAX_DAYS_AHEAD` (`crates/ds-store/src/tracking.rs`,
 * 28 since 2026-10-08, the timetable's publish window), which rejects
 * anything later with the same copy as `pinTooFarMessage`. */
export const PIN_MAX_DAYS_AHEAD = 28;

/** The last London calendar day a pin may be for, `'YYYY-MM-DD'`. */
export function lastPinDate(): string {
  return addCalendarDays(londonToday(), PIN_MAX_DAYS_AHEAD);
}

export const pinTooFarMessage = `That departure is too far ahead. Trains can be tracked up to ${PIN_MAX_DAYS_AHEAD} days before they run.`;

/** True unless `destinationCrs` looks like a resolved 3-letter code AND
 * the row's own destination doesn't case-insensitively match it. While
 * the field still holds partial/typed-name text (or is empty), every row
 * matches -- there is nothing on a row to honestly match partial text
 * against (rows carry a CRS code, never a station name). A `null` row
 * destination (CIF only) never matches an *active* filter: "unknown" is
 * not "assume it matches". See
 * docs/superpowers/specs/2026-09-04-track-a-train-picker-refactor-design.md
 * Decision 1. */
export function matchesDestination(rowDestinationCrs: string | null, destinationCrs: string): boolean {
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
export function matchesOperator(rowOperator: string, operator: string): boolean {
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
 * really on", not "has the user's chosen time already passed it".
 *
 * `now` is converted to Europe/London (`.tz(LONDON_TZ)`) before either its
 * calendar date is read or the row's `"HH:MM"` is combined with it --
 * `scheduled` is Darwin's own Europe/London wall-clock reading, so "today"
 * must mean the London calendar day, and the combined same-day guess must
 * be interpreted in that same zone, not whatever zone `now` itself happens
 * to carry (`dayjs()`'s host zone, if a caller passes one unconverted). This
 * makes the function correct regardless of what `now` the caller passes in
 * -- see `lib/londonWallClock.ts`'s own doc comment (2026-09-26 "Repeater
 * Signal" review, finding M8) for the bug this closes: a visitor outside
 * the UK combining today's BROWSER-local date with a London-wall-clock
 * `"HH:MM"` could misjudge a near-midnight row's real day by up to a whole
 * day. */
export function ldbwsDayOffset(scheduled: string, now: dayjs.Dayjs): 0 | 1 {
  const [hh, mm] = scheduled.split(':');
  const nowLondon = now.tz(LONDON_TZ);
  const sameDay = dayjs.tz(`${nowLondon.format('YYYY-MM-DD')} ${hh}:${mm}:00`, LONDON_TZ);
  return nowLondon.diff(sameDay, 'hour', true) > LDBWS_PAST_THRESHOLD_HOURS ? 1 : 0;
}

/** Resolves the real calendar date (`'YYYY-MM-DD'`) an LDBWS
 * `DepartureRow.scheduled` "HH:MM" falls on, relative to `now` -- `now.add(
 * ldbwsDayOffset(scheduled, now), 'day')`, formatted. See `ldbwsDayOffset`'s
 * own doc comment for the reasoning; this is the piece `pickDeparture` and
 * the LDBWS branch of `matchesScheduledDeparture`'s filtering both need,
 * factored out so they can't drift out of sync with each other.
 *
 * `now` is converted to Europe/London before its calendar date is read, same
 * reasoning and same fix as `ldbwsDayOffset` immediately above (whose own
 * internal London conversion this relies on for the day-offset itself). */
export function resolveLdbwsDepartureDate(scheduled: string, now: dayjs.Dayjs): string {
  return now.tz(LONDON_TZ).add(ldbwsDayOffset(scheduled, now), 'day').format('YYYY-MM-DD');
}

/** True unless `scheduledDeparture` is resolved AND the row's own
 * departure time is strictly before it -- filters out departures that
 * have already passed relative to whatever the user has typed/picked,
 * additive alongside `matchesDestination`/`matchesOperator`. Applies to
 * BOTH sources identically (unlike the Destination/Operator split): both
 * `DepartureRow.scheduled` and `ScheduleDepartureRow.scheduled` are the
 * same `"HH:MM"` shape. Combines the row's `"HH:MM"` with `rowDayOffset`
 * days past *today's Europe/London* date (`nowInLondon()`, not `dayjs()`'s
 * host-zone "today" -- 2026-09-26 review, finding M8: `scheduled` is itself
 * a London wall-clock reading, so "today" must mean the London calendar day
 * or this comparison drifts by a day near midnight for a visitor outside
 * the UK) into the exact same `'YYYY-MM-DD HH:mm:ss'` string shape
 * `scheduledDeparture` itself holds
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
 * picked.
 *
 * `baseDate` (`'YYYY-MM-DD'`) is the day `rowDayOffset` counts from,
 * London's today by default. The CIF call site passes the row's own
 * `serviceDate` (see `CifPickerRow`), which is what its `dayOffset` is
 * relative to. */
export function matchesScheduledDeparture(
  rowScheduled: string,
  scheduledDeparture: string | null,
  rowDayOffset = 0,
  baseDate: string = londonToday(),
): boolean {
  if (scheduledDeparture === null) return true;
  const [hh, mm] = rowScheduled.split(':');
  const date = addCalendarDays(baseDate, rowDayOffset);
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
 * rendering (`DeparturePicker`, `'ldbws'` branch) has no "View live status"
 * link even though the `'cif'` branch does. */
export interface DepartureRow {
  serviceId: string;
  operator: string;
  destinationCrs: string;
  /** Resolved via a batched `stations` lookup, added server-side
   * (`render::station_departure_json`) so this picker can show a name
   * instead of a bare CRS code -- 2026-09-22 UX review follow-up. `null`
   * when the code has no `stations` reference row, same "fall back to the
   * bare code" convention `stationLabel` applies everywhere else in this
   * app. */
  destinationName: string | null;
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
  // LDBWS Retail Service ID and the service's subsequent calling points
  // (see `BoardCallingPoint` in `lib/types.ts`). Optional: older backends
  // and older stored boards don't send them. Not rendered by the picker.
  rsid?: string | null;
  callingPoints?: BoardCallingPoint[];
}

/** Wire shape of `GET /public/stations/{crs}/schedule-departures`
 * (`crates/api/src/render.rs::schedule_departure_json`) -- deliberately
 * NOT `DepartureRow`: no `operator`, no live running-status fields at all
 * (`isCancelled`/`delayMinutes`/`estimated`/`cancelReason`/`delayReason`),
 * because the CIF SCHEDULE feed genuinely has none of that -- see
 * docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
 * Decision 2/5. (Since 2026-10-07 the server joins the train's own live
 * state, when it has one, into the optional `live` object below.)
 * `destinationCrs` is nullable: `null` when the terminating
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
 * both use this to compute the row's REAL departure calendar date instead of
 * always assuming "today" -- see their own doc comments.
 *
 * It never moves the row's SERVICE date: every row in one response belongs
 * to the same CIF service date (the bucket's, `Picker`'s `serviceDate`), and
 * the `/train/{uid}/{date}` link is keyed on that, not on the departure's
 * own day. Optional because an `api` pod from before the field existed
 * (rolled independently of `frontend`) omits it; absent means `0`. */
export interface ScheduleDepartureRow {
  uid: string;
  scheduled: string;
  /** The public (timetable) departure, shown in place of `scheduled` (the
   * working-timetable time, kept as WTT for one release). `null`/absent
   * until the next schedule publish. */
  publicDeparture?: string | null;
  /** Absent from an older api pod mid-rollout; see `pickCifDeparture`. */
  dayOffset?: number;
  destinationCrs: string | null;
  /** Same server-side batched-lookup enrichment as `DepartureRow`'s own
   * `destinationName` -- see its doc comment. `null` both when
   * `destinationCrs` itself is `null` and when it resolved to no
   * `stations` row. */
  destinationName: string | null;
  /** The schedule's true origin CRS, `null` when it does not resolve
   * (2026-10-07, docs/api-changelog.md). */
  originCrs?: string | null;
  /** `originCrs`'s station name, `null` when unknown. */
  originName?: string | null;
  /** The line summary's compact live status for this train today; `null`
   * when it has none. */
  live?: LineTrainSummaryLive | null;
}

/** `'unavailable'` replaces the old `'not-sampled'` name: it now means
 * neither the LDBWS live board NOR the CIF-derived timetable had data for
 * this station -- see Decision 3/5.
 *
 * Each CIF row carries the `serviceDate` of the bucket it was fetched from
 * (`CifPickerRow`). */
export type Picker =
  { source: 'ldbws'; rows: DepartureRow[] } | { source: 'cif'; rows: CifPickerRow[] } | 'unavailable' | null;

/** A CIF picker row plus its CIF service date: the London date of the
 * `schedule-departures` bucket it came from (`?date=`, or the server's own
 * `london_today()` when absent). A row's own departure day is
 * `serviceDate + dayOffset`; its `/train/{uid}/{date}` link uses
 * `serviceDate`. Captured at fetch time rather than re-read from the clock
 * at render, so a picker fetched at 23:59 doesn't relabel every row a day
 * later once London midnight passes. Between 00:00 and 02:00 the picker
 * also holds rows from yesterday's bucket, see `fetchCifPickerRows`. */
export type CifPickerRow = ScheduleDepartureRow & { serviceDate: string };

/** London hour before which the CIF picker also shows yesterday's services
 * still due here -- this codebase's 02:00 rail-day rollover
 * (`crates/common/src/rail_day.rs`). Until then, a service that left its
 * origin last evening can still be on its way to this station. */
const RAIL_DAY_ROLLOVER_HOUR = 2;

/** The departure's own London wall-clock reading, `'YYYY-MM-DD HH:mm:00'`
 * -- the same string shape `scheduledDeparture` holds. */
export function cifDepartureDateTime(row: CifPickerRow): string {
  const [hh, mm] = row.scheduled.split(':');
  return `${addCalendarDays(row.serviceDate, row.dayOffset ?? 0)} ${hh}:${mm}:00`;
}

/** The `schedule-departures` rows for `crs`, each tagged with its service
 * date. Resolves to `'unavailable'` on today's `404`, `null` on any other
 * failure of today's request -- the picker's existing contract.
 *
 * Between London 00:00 and 02:00 it also asks for yesterday's bucket
 * (`?date=`) and adds its rows that depart after midnight (`dayOffset >= 1`)
 * and not before now: last evening's services still due here, which
 * today's bucket can't contain (a CIF service is dated by its origin day).
 * Those rows keep yesterday as their service date. That request is
 * best-effort: any failure just leaves them out. An `api` pod from before
 * `?date=` ignores the parameter and answers with today's bucket, so an
 * answer identical to today's is discarded rather than relabelled.
 *
 * The merged rows are put in departure order. Duplicates (same uid, service
 * date and departure) are dropped; a service calling here twice keeps both
 * calls, since those differ in departure. */
export async function fetchCifPickerRows(
  crs: string,
  signal: AbortSignal,
): Promise<CifPickerRow[] | 'unavailable' | null> {
  const now = nowInLondon();
  const today = now.format('YYYY-MM-DD');
  const yesterday = addCalendarDays(today, -1);
  const base = `/api/stations/${crs}/schedule-departures`;
  const yesterdayRequest: Promise<ScheduleDepartureRow[] | null> =
    now.hour() < RAIL_DAY_ROLLOVER_HOUR
      ? fetch(`${base}?date=${yesterday}`, { signal })
          .then((res) => (res.ok ? (res.json() as Promise<unknown>) : null))
          .then((body) => (Array.isArray(body) ? (body as ScheduleDepartureRow[]) : null))
          .catch(() => null)
      : Promise.resolve(null);
  const [todayRes, yesterdayRows] = await Promise.all([fetch(base, { signal }), yesterdayRequest]);
  if (todayRes.status === 404) {
    return 'unavailable';
  }
  if (!todayRes.ok) {
    return null;
  }
  const todayRows = (await todayRes.json()) as ScheduleDepartureRow[];
  const rows: CifPickerRow[] = todayRows.map((row) => ({ ...row, serviceDate: today }));
  if (yesterdayRows === null || JSON.stringify(yesterdayRows) === JSON.stringify(todayRows)) {
    return rows;
  }
  const nowWallClock = now.format('YYYY-MM-DD HH:mm:00');
  const stillDue = yesterdayRows
    .map((row): CifPickerRow => ({ ...row, serviceDate: yesterday }))
    .filter((row) => (row.dayOffset ?? 0) >= 1 && cifDepartureDateTime(row) >= nowWallClock);
  const seen = new Set<string>();
  return [...stillDue, ...rows]
    .filter((row) => {
      const key = `${row.uid}|${row.serviceDate}|${cifDepartureDateTime(row)}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .sort((a, b) => cifDepartureDateTime(a).localeCompare(cifDepartureDateTime(b)));
}
