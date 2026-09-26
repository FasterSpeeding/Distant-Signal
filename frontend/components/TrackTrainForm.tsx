'use client';

import { useEffect, useId, useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import {
  Alert,
  Autocomplete,
  Button,
  Group,
  SegmentedControl,
  SimpleGrid,
  Stack,
  Text,
  VisuallyHidden,
} from '@mantine/core';
import { DateTimePicker, DatePickerInput } from '@mantine/dates';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { ScheduleRow } from './ScheduleRow';
import { TextLink } from './TextLink';
import { TrackDestinationModal } from './TrackDestinationModal';
import { TimeFilterInput } from './TimeFilterInput';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { useGroupSummaries } from '@/lib/useGroupSummaries';
import { shareTrackedTrainToGroup } from '@/lib/shareTrackedTrain';
import { suggestionAutocompleteProps } from '@/lib/suggestionAutocomplete';
import { stationLabel } from '@/lib/stationLabel';
import { nowInLondon, londonWallClockToUtc, LONDON_TZ } from '@/lib/londonWallClock';
import type { CreateJourneyResponse } from '@/lib/types';

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
function ldbwsDayOffset(scheduled: string, now: dayjs.Dayjs): 0 | 1 {
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
function resolveLdbwsDepartureDate(scheduled: string, now: dayjs.Dayjs): string {
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
 * picked. */
function matchesScheduledDeparture(
  rowScheduled: string,
  scheduledDeparture: string | null,
  rowDayOffset = 0,
): boolean {
  if (scheduledDeparture === null) return true;
  const [hh, mm] = rowScheduled.split(':');
  const date = nowInLondon().add(rowDayOffset, 'day').format('YYYY-MM-DD');
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
  /** Same server-side batched-lookup enrichment as `DepartureRow`'s own
   * `destinationName` -- see its doc comment. `null` both when
   * `destinationCrs` itself is `null` and when it resolved to no
   * `stations` row. */
  destinationName: string | null;
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
 * creating a tracked train for. Once `submitTrack`'s `POST /Journeys`
 * (`pin`-mode leg) call succeeds, this form makes one best-effort follow-up
 * call, `POST /Train/tickets/{attachTicketId}/attach`, before navigating to
 * the new journey's page -- if that call fails for any reason (network
 * blip, the ticket having since been attached elsewhere), tracking the
 * train has ALREADY succeeded and this form still navigates on; the ticket
 * just stays standalone and attachable later from the merged trains/tickets
 * list, rather than the whole flow failing over a non-essential follow-up.
 * `submitWindow`'s open-window leg has no bound train at all yet, so it has
 * no equivalent ticket-attach step.
 *
 * Journey tracking Phase 1 (this task) moved both submit paths off the
 * legacy `POST /Train/track` route onto `POST /Journeys`: `submitTrack`
 * posts a `pin`-mode leg (the pre-existing "pick a departure"/manual-entry
 * flow, field-for-field identical to the old `TrackPinRequest`), and the
 * new `submitWindow` posts a `window`-mode leg for the `SegmentedControl`'s
 * second option -- an open time-window search with no train chosen yet
 * (the journey view, Task 17, is where a candidate gets picked next). Both
 * go through the same same-origin `/api/Journeys` proxy (Client Components
 * can't read the server-only `API_BASE_URL` env var `lib/api.ts` relies on
 * -- same reasoning as `PinToggle`). Mirrors `PinToggle`'s `needsLogin` 401
 * pattern, with one deliberate difference: a 401 here does NOT reset the
 * form. `PinToggle` can afford to forget its click (there was no typed
 * input to lose); a form with real typed input is worth protecting, so
 * every field stays exactly as typed while the login prompt renders
 * alongside it (Decision 4, "no navigation away").
 *
 * Shared-groups follow-up: `useGroupSummaries()` decides, once on mount,
 * whether this user belongs to any group. Zero groups (the majority case,
 * and every anonymous visitor) leaves `handleSubmit`'s form `onSubmit`
 * calling `submitTrack(null)`/`submitWindow(null)` (per `mode`) directly --
 * no prompt, no behavior change from before this feature existed. One or
 * more groups instead makes `handleSubmit` open `TrackDestinationModal`
 * first and defer the actual submit to its `onConfirm`, which calls
 * whichever of the two matches the current `mode` -- see that component's
 * own doc comment for why reusing it unmodified (rather than a second,
 * divergent submit path) is safe. Sharing the new pin into the chosen
 * group (`shareTrackedTrainToGroup`) is a further best-effort follow-up
 * `submitTrack` performs once the track call itself has already
 * succeeded, same swallow-every-failure posture as the `attachTicketId`
 * block right below it -- `submitWindow` has no such follow-up yet (see
 * that function's own doc comment on why `groupId` is currently unused
 * there). */
export function TrackTrainForm({
  initialOrigin = '',
  initialDestination = '',
  attachTicketId,
  initialMode = 'pick',
  initialServiceDate,
  initialDepartAfter = '',
  initialDepartBefore = '',
  initialArriveAfter = '',
  initialArriveBefore = '',
  onCreated,
}: {
  initialOrigin?: string;
  // "Track this journey again" (docs/superpowers/plans/2026-09-22-reusable-journeys-phaseA-track-again-plan.md)
  // is the first caller of these five props -- pre-fills the Destination
  // field (pin mode and window mode read/write the SAME `destinationCrs`
  // state, see that state declaration's own comment -- only one mode's
  // rendering of it is ever visible at a time, driven by `initialMode`),
  // and the window-mode time bounds. All five are inert, ordinary
  // `useState` initial values, same as `initialOrigin` already is -- no
  // new prop changes this component's submit behaviour.
  initialDestination?: string;
  attachTicketId?: number;
  // Review §2.1/I21: the mode toggle used to live only in `useState`, so
  // nothing in the app could send a user straight to window mode -- not
  // even `JourneyLegCard`'s own "Edit search" link. `track/page.tsx` reads
  // this off `?mode=window`, the same pattern its `?origin=` already uses.
  initialMode?: 'pick' | 'window';
  /** "YYYY-MM-DD" -- window mode's own service-date field
   * (`windowServiceDate`, below), which otherwise always defaults to
   * today. `undefined` (the default) preserves that exact pre-existing
   * behaviour. `CreateJourneyLegFromTicketButton.tsx`
   * (`/track?serviceDate=...`) is the first caller of this prop: a
   * ticket's proposed leg is dated by the ticket's own extracted departure
   * date, not necessarily today. Deliberately has no pin-mode equivalent --
   * pin mode's `scheduledDeparture` is a single full date+time field with
   * its own "now" default, not a bare date, so there is nothing for this
   * prop to feed there. */
  initialServiceDate?: string;
  /** "HH:MM" -- same value contract `TimeFilterInput`'s own `onChange`
   * already uses for `departFrom`/`departTo`/`arriveFrom`/`arriveTo`. */
  initialDepartAfter?: string;
  initialDepartBefore?: string;
  initialArriveAfter?: string;
  initialArriveBefore?: string;
  /** `JourneyCreationFlow.tsx`'s (the `/journeys/new` continuous
   * multi-leg creation page) only caller of this prop: this form is
   * reused there verbatim as the "leg 1" step, but that page wants to
   * stay put afterwards and offer an inline "Add a leg" rather than being
   * pushed away to `/journeys/{id}` the instant leg 1 exists -- the way
   * every OTHER caller of this form (`/track` itself, chief among them)
   * still wants, and gets, by leaving this prop unset. When given, this
   * replaces `submitTrack`/`submitWindow`'s own final `router.push` with a
   * call to this instead; every other success side effect (the
   * ticket-attach/group-share follow-ups below) is unaffected. `undefined`
   * (the default) preserves this component's exact pre-existing
   * behaviour -- the redirect fires unconditionally, same as before this
   * prop existed. */
  onCreated?: (result: CreateJourneyResponse) => void;
}) {
  const router = useRouter();
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [operator, setOperator] = useState('');
  // Defaults to "now" (the repo owner's own stated expectation), not
  // `null` -- computed once via lazy `useState` initializer, in the exact
  // Europe/London-wall-clock `'YYYY-MM-DD HH:mm:ss'` string shape the "Now"
  // button (below) and `pickDeparture`/`pickCifDeparture` already construct,
  // so it round-trips through `handleSubmit`'s own parsing identically to a
  // value the user picked by hand. `nowInLondon()`, not `dayjs()` -- this
  // value feeds `handleSubmit`'s `serviceDate`/`scheduled_departure`
  // computation directly, so it must be pinned to the London wall clock this
  // whole app treats train times as being in, not whatever zone the
  // visitor's own browser happens to be in (2026-09-26 review, finding M8;
  // contrast the trip planner's "Depart after" field, an ordinary filter
  // input correctly left in plain browser-local time).
  const [scheduledDeparture, setScheduledDeparture] = useState<string | null>(() =>
    nowInLondon().format('YYYY-MM-DD HH:mm:ss'),
  );
  // Darwin's own explicit skipped-calling-point snapshot for whichever
  // live departure-board row the user picked (`pickDeparture`, below) --
  // a live-board pick is the only source that ever has this signal at all
  // (the CIF-picker/manual-entry paths never do). `submitTrack`'s `pin`-mode
  // leg forwards this to `POST /Journeys` (`CreateJourneyLegRequest::Pin`'s
  // own `skippedStations` field, `crates/api/src/routes/journeys.rs`), the
  // same value the legacy `TrackPinRequest` this form used to submit
  // carried under the same name.
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

  // Window-search mode -- the `SegmentedControl`'s second option. Reuses
  // `TimeFilterInput`'s existing before/after convention verbatim (per the
  // journey-tracking design doc's own §0.5 direction) rather than inventing
  // a new one for leg-window entry.
  const [mode, setMode] = useState<'pick' | 'window'>(initialMode);
  const modeLabelId = useId();
  // Destination station is genuinely the SAME field in both modes -- one
  // `destinationCrs` state (declared above, alongside `originCrs`), not a
  // second `windowDestinationCrs` copy. It used to be a second copy: two
  // independently-typed `useState`s that both happened to be seeded from
  // `initialDestination`, so a value typed into one mode's Destination
  // field silently vanished the moment the user switched to the other --
  // toggling the `SegmentedControl` looks like "my typing got cleared"
  // even though neither `setState` call is ever cleared by the toggle
  // itself (2026-09-23 state-persistence-across-mode-toggle fix). Unifying
  // the state is what makes the window-mode Autocomplete below able to
  // reuse `destinationSuggestions`/`destinationSuggestionsLoading`
  // (declared with `destinationCrs` above) instead of needing its own
  // dedicated `useSuggestions` instance -- with one shared value there is
  // only one thing for a suggestions dropdown to be driven by, so the
  // reason a second hook instance existed (see this comment's own former
  // text, in git blame) no longer applies.
  //
  // Origin (`originCrs`) already worked this way from the start -- see
  // that field's own "Origin is the one field both modes share" comment
  // just above the `SegmentedControl` in the JSX below -- this brings
  // Destination in line with it. `scheduledDeparture`/window
  // date-and-time-bounds/`operator` stay mode-scoped, deliberately NOT
  // unified: they have no equivalent meaning in the other mode (window
  // mode has no single scheduled-departure instant to fill, pick mode has
  // no depart/arrive-window bounds, and `operator` only narrows pick
  // mode's already-fetched departure-board picker -- window mode has no
  // candidate list of its own to narrow at all, see `submitWindow`'s own
  // doc comment).
  // Review §2.2/M13: seeded with today's real date, not `null` -- a `null`
  // value rendered the field as empty with "Today" as a grey PLACEHOLDER,
  // which reads as "nothing selected" even though `submitWindow` (below)
  // already resolves a `null` to today. Pin-mode's own `scheduledDeparture`
  // field, twenty pixels away in the other branch, shows its default as a
  // real value for the same reason -- this brings window mode in line with
  // it. Still `clearable` (below), and `submitWindow`'s `?? nowInLondon()...`
  // fallback stays as defence if a caller ever clears it back to `null`.
  // `nowInLondon()`, not `dayjs()` -- this is a `serviceDate` sent straight
  // to the backend (same reasoning as `scheduledDeparture`'s own default
  // just above; 2026-09-26 review, finding M8).
  const [windowServiceDate, setWindowServiceDate] = useState<string | null>(
    () => initialServiceDate ?? nowInLondon().format('YYYY-MM-DD'),
  );
  const [departFrom, setDepartFrom] = useState(initialDepartAfter);
  const [departTo, setDepartTo] = useState(initialDepartBefore);
  const [arriveFrom, setArriveFrom] = useState(initialArriveAfter);
  const [arriveTo, setArriveTo] = useState(initialArriveBefore);
  // Same half-entered-time bookkeeping TrainSearchForm.tsx's own four
  // TimeFilterInput fields already need -- see that component's own
  // `incompleteTimes` doc comment for the full reasoning (a native
  // `<input type="time">` reports a half-entered value as `''`,
  // indistinguishable from untouched).
  const [windowIncompleteTimes, setWindowIncompleteTimes] = useState({
    departFrom: false,
    departTo: false,
    arriveFrom: false,
    arriveTo: false,
  });
  // Renamed from `windowDestinationValid` now that Destination is shared
  // state (see the comment above `windowServiceDate`) -- this is no longer
  // a window-mode-only computation, even though it is currently only
  // CONSUMED by window mode's own validation/copy below (pin mode's
  // Destination field stays optional with no equivalent validity gate,
  // unchanged).
  const destinationValid = CRS_PATTERN.test(destinationCrs.trim());
  const windowTimesComplete = !Object.values(windowIncompleteTimes).some(Boolean);
  const windowHasABound =
    departFrom.trim() !== '' || departTo.trim() !== '' || arriveFrom.trim() !== '' || arriveTo.trim() !== '';
  const canSubmitWindow =
    originValid && destinationValid && windowTimesComplete && windowHasABound && !submitting;

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
   * edit before tracking. Combines the departure's `"HH:MM"` with its REAL
   * Europe/London calendar date, via `resolveLdbwsDepartureDate`, into the
   * exact `'YYYY-MM-DD HH:mm:ss'` string shape `scheduledDeparture` already
   * expects -- same construction as the "Now" button above
   * (`nowInLondon().format('YYYY-MM-DD HH:mm:ss')`). `resolveLdbwsDepartureDate`
   * itself converts whatever `now` it's given to Europe/London internally
   * (see that function's own doc comment), so the plain `dayjs()` passed
   * below is fine as an absolute instant -- what would NOT be fine is
   * reading a calendar date or combining it with `row.scheduled` (a London
   * wall-clock reading) anywhere else without going through it.
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
   * Adds `row.dayOffset` days to *today's Europe/London* date
   * (`nowInLondon()`, not `dayjs()` -- 2026-09-26 review, finding M8: this
   * feeds `scheduledDeparture`/`serviceDate` directly, so "today" must mean
   * the London calendar day, not the visitor's own browser zone), rather
   * than always assuming "today" the way `pickDeparture` (LDBWS, below) still
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
    const date = nowInLondon().add(row.dayOffset ?? 0, 'day').format('YYYY-MM-DD');
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
      // `scheduledDeparture` is the DateTimePicker's own bare-wall-clock
      // string, `'YYYY-MM-DD HH:mm:ss'` (@mantine/dates' `assign-time.mjs`
      // formats it via `date.format('YYYY-MM-DD HH:mm:ss')`) -- not ISO
      // 8601, and with no zone/offset of its own. Every value that ever
      // lands in this state (the "now" default, the "Now" button, a picked
      // live-departure-board row, a picked CIF schedule row -- see each of
      // their own doc comments) is built to represent an Europe/London
      // wall-clock reading, matching the departure times themselves (a
      // train's scheduled departure is always stated in Europe/London terms
      // in this app, `lib/dateFormat.ts`'s own stated convention), NOT
      // whatever zone the visitor's own browser happens to be in.
      //
      // `service_date` is `serviceDate`'s first 10 characters, read directly
      // off the raw string rather than round-tripped through `Date` --
      // that's already the London calendar date the string represents, and
      // a round-trip would risk exactly the kind of local-midnight/DST
      // day-off-by-one this comment is about avoiding for the timestamp
      // below.
      //
      // `scheduled_departure`, unlike `service_date`, DOES need the real UTC
      // instant, so it can't just take the string as-is -- but a bare
      // `new Date(scheduledDeparture.replace(' ', 'T'))` (this function's own
      // shape until the 2026-09-26 "Repeater Signal" review, finding M8)
      // parses a zone-less date-time string in the HOST's own timezone per
      // the ECMAScript spec -- the browser's, for a value built client-side.
      // That is only ever correct for a visitor whose own browser happens to
      // be on Europe/London; anyone else (a UK-based visitor travelling
      // abroad, someone tracking a train for a UK-based relative from a
      // different zone) got the wrong instant, silently -- close enough most
      // of the day to go unnoticed, but capable of computing a `/train/
      // {uid}/{date}` link a whole day off `serviceDate` near the London
      // midnight boundary. `londonWallClockToUtc` (`lib/londonWallClock.ts`)
      // is the fix: it explicitly resolves the string against Europe/London
      // (via `dayjs`'s `utc`/`timezone` plugins) rather than relying on
      // whatever zone the runtime happens to be in.
      const serviceDate = scheduledDeparture.slice(0, 10);
      const departure = londonWallClockToUtc(scheduledDeparture);
      const body = {
        customName: null,
        leg: {
          mode: 'pin' as const,
          originCrs: originCrs.trim().toUpperCase(),
          scheduledDeparture: departure.toISOString(),
          serviceDate,
          ...(destinationCrs.trim() ? { destinationCrs: destinationCrs.trim().toUpperCase() } : {}),
          ...(operator.trim() ? { operator: operator.trim() } : {}),
          ...(skippedStations.length > 0 ? { skippedStations } : {}),
          // Integration note (2026-09-22): the Darwin platform snapshot the
          // departure-board picker captures (`pickDeparture`) used to ride
          // on the legacy `POST /Train/track` body as
          // `platform`/`planned_platform`. `POST /Journeys` replaced that
          // route, so the same two values now travel inside the pin-mode
          // LEG, in this endpoint's camelCase convention -- see
          // `CreateJourneyLegRequest::Pin` in
          // `crates/api/src/routes/journeys.rs`, which forwards them into
          // the very same `common::TrackPinRequest` fields the old route
          // filled. Dropping them here would silently lose the origin
          // calling point's platform on every newly pinned journey.
          ...(platform !== null ? { platform } : {}),
          ...(plannedPlatform !== null ? { plannedPlatform } : {}),
        },
      };

      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        const result: CreateJourneyResponse = await response.json();
        if (attachTicketId !== undefined && result.trackingId !== null) {
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed -- see this block's own comment above.
          }
        }
        if (groupId !== null && result.trackingId !== null) {
          // Still shares the underlying train_subscriptions row via the
          // EXISTING group_trains table, unchanged -- journey-level
          // sharing is Phase 4 (see this plan's own Non-goals).
          await shareTrackedTrainToGroup(groupId, result.trackingId);
        }
        if (onCreated) {
          onCreated(result);
        } else {
          router.push(`/journeys/${result.journeyId}`);
        }
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

  /** The window-search mode's own submit -- posts a `window`-mode leg to
   * `POST /Journeys` instead of a `pin`-mode one, creating an `'unmatched'`
   * leg with no train bound yet (the journey view, Task 17, is where a
   * candidate gets picked next). Reuses `TrackDestinationModal`'s
   * Personal-vs-group prompt the same way `submitTrack` does -- see
   * `handleSubmit`'s own doc comment -- but `groupId` is currently a
   * placeholder here: sharing an open/unmatched leg into a group at
   * creation time has no real precedent yet (there is no train to share),
   * and this plan's Non-goals explicitly defer journey-level group sharing
   * to Phase 4. The parameter is kept, unused, purely so `handleSubmit`'s
   * dispatch to either `submitTrack`/`submitWindow` can share one call
   * shape without a branch on arity. */
  async function submitWindow(groupId: string | null) {
    if (!canSubmitWindow) return;
    setSubmitting(true);
    needsLoginState.reset();
    setFieldError(null);
    try {
      const body = {
        customName: null,
        leg: {
          mode: 'window' as const,
          originCrs: originCrs.trim().toUpperCase(),
          destinationCrs: destinationCrs.trim().toUpperCase(),
          serviceDate: windowServiceDate ?? nowInLondon().format('YYYY-MM-DD'),
          departWindow: { after: departFrom || null, before: departTo || null },
          arriveWindow: { after: arriveFrom || null, before: arriveTo || null },
        },
      };
      const response = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (response.ok) {
        const result: CreateJourneyResponse = await response.json();
        // No trackingId yet (an open leg has no train bound) -- so no
        // ticket-attach or group-share follow-up is possible here, unlike
        // submitTrack/pin mode. The journey view itself (Task 17) is where
        // a candidate gets picked next.
        void groupId; // reserved for a future group-share-on-window-search follow-up
        if (onCreated) {
          onCreated(result);
        } else {
          router.push(`/journeys/${result.journeyId}`);
        }
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (response.status === 400) {
        const text = await response.text();
        setFieldError(text || "Couldn't search for a train. Try again.");
        return;
      }
      setFieldError("Couldn't search for a train. Try again.");
    } catch {
      setFieldError("Couldn't search for a train. Try again.");
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
    if (mode === 'window') {
      if (!originValid || !destinationValid || !windowHasABound) {
        setFieldError(
          !originValid || !destinationValid
            ? 'Enter a valid origin and destination station before searching.'
            : 'Enter at least one earliest/latest departure or arrival time to search a window.',
        );
        return;
      }
      // Review §2.2/M14: "no visible earliest > latest check" -- both were
      // previously checked only for PRESENCE, never for order, so
      // "earliest 18:00, latest 09:00" reached the backend unexamined.
      // Plain string comparison is safe here: both are always `"HH:MM"`
      // (`TimeFilterInput`'s own value contract), which sorts
      // lexicographically identical to chronologically within one day.
      if (departFrom && departTo && departFrom > departTo) {
        setFieldError('Latest departure must be after earliest departure.');
        return;
      }
      if (arriveFrom && arriveTo && arriveFrom > arriveTo) {
        setFieldError('Latest arrival must be after earliest arrival.');
        return;
      }
      setFieldError(null);
      if (groups.length > 0) {
        setDestinationPromptOpened(true);
        return;
      }
      void submitWindow(null);
      return;
    }
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
                destinationName: row.destinationName,
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
                  {/* `stationLabel` -- same "Name (CODE)", bare-code-fallback
                      convention as everywhere else in this app (item 5,
                      2026-09-22 UX review follow-up): this used to always
                      show the raw code alone. */}
                  {row.destinationCrs ? ` · ${stationLabel(row.destinationCrs, row.destinationName)}` : ''}
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
                    `pickCifDeparture` itself). `nowInLondon()`, not
                    `dayjs()` -- `service_date` is a London calendar day, so
                    "today" here must mean London's, not the visitor's own
                    browser zone (2026-09-26 review, finding M8: this is the
                    exact `/train/{uid}/{date}` link that finding's own
                    motivating example describes). */}
                <TextLink
                  href={`/train/${encodeURIComponent(row.uid)}/${nowInLondon()
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
    <Stack gap="md">
      {/* Review §2.1/I21: the page's own intro used to be static copy
          owned by `track/page.tsx` ("Pin a specific train…"), true of pick
          mode only -- a visitor who switched to window mode was still told
          the page was for a *specific* train. Lives here, not there, so it
          can react to the client-side mode toggle instead of only the
          initial `?mode=` a page load saw. The ticket-attach case keeps its
          own mode-independent copy from the caller (`track/page.tsx`),
          since attaching a ticket only ever follows the pin-mode path. */}
      {attachTicketId === undefined && (
        <Text c="dimmed">
          {mode === 'window'
            ? "Not sure which train yet? Tell us roughly when you're travelling — Origin, Destination and at least one of the times below — and we'll show you the matches to choose from. You can change your pick later."
            : 'Pin a specific train to see its live position, delay and next calling point as Network Rail reports it.'}
        </Text>
      )}
      <Stack gap="md" component="form" onSubmit={handleSubmit} maw={640}>
      <Autocomplete
        label="Origin station"
        placeholder="e.g. Woking or WOK"
        value={originCrs}
        onChange={setOriginCrs}
        onBlur={() => setOriginTouched(true)}
        {...suggestionAutocompleteProps(originSuggestions, {
          query: originCrs,
          loading: originSuggestionsLoading,
          noMatchMessage: 'No matching stations',
        })}
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
      {/* Origin is the one field both modes share -- every leg shape
          (`pin`/`knownTrain`/`window`) needs an `originCrs`, so it stays
          above the mode switch rather than being duplicated inside each
          branch. */}
      {/* Review §2.1/I21: "Pick a departure"/"Search a time window" named
          the FORM's own mechanism, not the traveller's situation -- and the
          sibling `AddJourneyLegButton` modal already used the clearer "I
          know the train" for the equivalent choice, so the codebase had two
          different vocabularies for one decision. Both controls now agree:
          "I know the train" / "Search a time window".

          A visible label wired as the radiogroup's own name. Without it
          a screen reader announced "radiogroup, Pick a departure, radio
          button, 1 of 2, checked" -- the group itself unnamed -- and this
          toggle is the main discovery path for window mode (`?mode=window`
          deep-links into it, but nothing links there), so the name is
          load-bearing (2026-09-22 UX review, I9/P4). Same `Text id` +
          `aria-labelledby` shape
          `HistoryRangePicker`'s "Period" control already uses; the
          question is phrased from the traveller's situation rather than
          from the form's mechanism. */}
      <Text id={modeLabelId} size="xs" fw={600} c="dimmed">
        How do you want to find the train?
      </Text>
      <SegmentedControl
        aria-labelledby={modeLabelId}
        value={mode}
        onChange={(value) => setMode(value as 'pick' | 'window')}
        data={[
          { label: 'I know the train', value: 'pick' },
          { label: 'Search a time window', value: 'window' },
        ]}
      />
      {/* Review §2.1/M23: the toggle swaps ~400px of form beneath it with
          no announcement -- sighted users see it happen, screen-reader
          users get nothing until they tab forward into different fields.
          `VisuallyHidden` keeps this out of the visual layout entirely; the
          visible cue (the fields themselves changing) is unaffected. */}
      <VisuallyHidden role="status" aria-live="polite">
        {mode === 'window' ? 'Showing time-window search.' : 'Showing pick-a-departure search.'}
      </VisuallyHidden>
      {mode === 'window' ? (
        <>
          <Autocomplete
            label="Destination station"
            placeholder="e.g. Reading or RDG"
            // `destinationCrs`/`setDestinationCrs`/`destinationSuggestions`
            // -- the SAME state and suggestions hook the pin-mode
            // Destination field below uses, not a separate window-mode
            // copy. See the state-declaration comment above
            // `windowServiceDate` for why: a value typed here now survives
            // switching back to "I know the train" mode, and vice versa.
            value={destinationCrs}
            onChange={setDestinationCrs}
            {...suggestionAutocompleteProps(destinationSuggestions, {
              query: destinationCrs,
              loading: destinationSuggestionsLoading,
              noMatchMessage: 'No matching stations',
            })}
            error={
              destinationCrs.length > 0 && !destinationValid
                ? 'Must be a 3-letter CRS code'
                : null
            }
            // NOT the native `required` attribute -- same reasoning as the
            // Origin field's own comment above: a native `required` field
            // would let the browser's own constraint validation intercept
            // the submit event before `handleSubmit` ever runs (confirmed
            // live -- jsdom enforces this too), silently replacing this
            // form's own explanatory `fieldError` message with (at best) a
            // native validation bubble instead. `handleSubmit`'s own
            // `!destinationValid` check already owns this
            // validation.
          />
          <DatePickerInput
            label="Date"
            // Review §2.2/M13: `windowServiceDate` is seeded with today's
            // real date above (not `null`), so this now shows an actual
            // value ("22 Sept 2026") the way pin-mode's own default
            // departure does -- "Today" stays only as the placeholder for
            // if the field is ever cleared back to empty.
            placeholder="Today"
            value={windowServiceDate}
            onChange={setWindowServiceDate}
            clearable
            // Bug: this `clearable` field was the one place in the app
            // missing the `clearButtonProps` aria-label every other
            // `clearable` field already carries (see e.g.
            // `TrainSearchForm.tsx`'s own "Clear the date" and
            // `IncidentSearchForm.tsx`'s comment on the same pattern).
            // Mantine's `clearable` renders an icon-only close button with
            // no accessible name of its own, which axe-core flags as a
            // critical `button-name` violation -- caught by the
            // accessibility suite's `/track, departure picker populated`
            // and route-sweep cases once this field had a value to clear.
            clearButtonProps={{ 'aria-label': 'Clear the date' }}
          />
          {/* Review §2.2/I17: all four fields below say "(optional)" in
              their own label, which is individually true but collectively
              misleading -- `windowHasABound` (used by `handleSubmit`
              above) refuses to submit unless at least one of the four is
              set, and until now that rule surfaced only as a post-submit
              error. This states it up front instead of relabelling the
              fields (which would have to explain "optional, but not all
              four of you" some other way). */}
          <Text size="sm">At least one of the four times below is required to search.</Text>
          <SimpleGrid cols={{ base: 1, sm: 2 }}>
            <TimeFilterInput
              label="Earliest departure (optional)"
              name="earliest departure"
              description={`Only trains leaving ${originValid ? originCrs.trim().toUpperCase() : 'the origin above'} at or after this time.`}
              value={departFrom}
              onChange={setDepartFrom}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, departFrom: v }))}
              error={null}
            />
            <TimeFilterInput
              label="Latest departure (optional)"
              name="latest departure"
              description={`Only trains leaving ${originValid ? originCrs.trim().toUpperCase() : 'the origin above'} at or before this time.`}
              value={departTo}
              onChange={setDepartTo}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, departTo: v }))}
              error={null}
            />
          </SimpleGrid>
          <SimpleGrid cols={{ base: 1, sm: 2 }}>
            <TimeFilterInput
              label="Earliest arrival (optional)"
              name="earliest arrival"
              description={`Only trains reaching ${destinationValid ? destinationCrs.trim().toUpperCase() : 'the destination above'} at or after this time.`}
              value={arriveFrom}
              onChange={setArriveFrom}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, arriveFrom: v }))}
              error={null}
            />
            <TimeFilterInput
              label="Latest arrival (optional)"
              name="latest arrival"
              description={`Only trains reaching ${destinationValid ? destinationCrs.trim().toUpperCase() : 'the destination above'} at or before this time.`}
              value={arriveTo}
              onChange={setArriveTo}
              onIncompleteChange={(v) => setWindowIncompleteTimes((c) => ({ ...c, arriveTo: v }))}
              error={null}
            />
          </SimpleGrid>
        </>
      ) : (
        <>
          {/* Review §2.3/M12: `wrap="nowrap"` plus the button's own
              `flexShrink: 0` -- without them this `Group` wraps at 390px,
              orphaning "Now" alone on its own row under a full-width
              picker (the same §2.5 shrink-guard idiom `StatusRow` already
              centralises for badge/text rows). */}
          <Group align="flex-end" gap="xs" wrap="nowrap">
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
              // Same reasoning as the Origin field above -- a cleared
              // departure is validated by `handleSubmit` itself now, not by
              // native `required` constraint validation.
              style={{ flexGrow: 1 }}
            />
            {/* `@mantine/dates`' own `presets` prop (9.5.2) only ever assigns a
                *date* (`DatePickerPreset['value']` is a bare `DateStringValue`,
                like `DatePicker`'s "Today"/"Yesterday" presets) -- it has no
                way to also fill in a time-of-day, so it can't produce "right
                now" on its own; a plain Button next to the picker is the clean
                fit here instead. `nowInLondon().format('YYYY-MM-DD HH:mm:ss')`
                deliberately matches the exact bare-wall-clock string shape
                the picker itself produces (`assign-time.mjs`'s own
                `date.format('YYYY-MM-DD HH:mm:ss')`) -- see this file's own
                `handleSubmit` comment on why that shape, not an ISO string,
                is required to avoid an around-midnight day-off-by-one, and
                why it's anchored to Europe/London rather than `dayjs()`'s
                host zone (2026-09-26 review, finding M8). */}
            <Button
              variant="default"
              style={{ flexShrink: 0 }}
              onClick={() => setScheduledDeparture(nowInLondon().format('YYYY-MM-DD HH:mm:ss'))}
            >
              Now
            </Button>
          </Group>
          <Autocomplete
            label="Destination station (optional)"
            placeholder="e.g. Woking or WOK"
            value={destinationCrs}
            onChange={setDestinationCrs}
            {...suggestionAutocompleteProps(destinationSuggestions, {
              query: destinationCrs,
              loading: destinationSuggestionsLoading,
              noMatchMessage: 'No matching stations',
            })}
          />
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
        </>
      )}
      {fieldError && (
        <Alert color="red" title={mode === 'window' ? "Couldn't search for a train" : "Couldn't track this train"}>
          {fieldError}
        </Alert>
      )}
      <Group>
        {/* Disabled only while a submit is in flight -- see
            `handleSubmit`'s own doc comment (Task 3.6.14) for why an
            invalid-but-not-yet-submitted form no longer disables this
            button at all. */}
        <Button type="submit" disabled={submitting}>
          {mode === 'window'
            ? submitting
              ? 'Searching…'
              : 'Search for a train'
            : submitting
              ? 'Tracking…'
              : 'Track this train'}
        </Button>
      </Group>
      </Stack>
      <TrackDestinationModal
        opened={destinationPromptOpened}
        groups={groups}
        onClose={() => setDestinationPromptOpened(false)}
        onConfirm={(groupId) => void (mode === 'window' ? submitWindow(groupId) : submitTrack(groupId))}
      />
      {/* Review §2.5/M19: the modal used to say "Log in to track this
          train" unconditionally, even in window mode -- where nothing is
          tracked yet (a window search doesn't have a chosen train). */}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        {mode === 'window' ? 'Log in to search for a train.' : 'Log in to track this train.'}
      </LoginPromptModal>
    </Stack>
  );
}
