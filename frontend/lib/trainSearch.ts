import { formatWeekdayShortDate } from '@/lib/dateFormat';
import { addCalendarDays, nowInLondon } from '@/lib/londonWallClock';
import type { ServiceModeFields, TrainSearchResult } from '@/lib/types';

/** The train search's filter patterns and result state, for
 * `TrainSearchForm` and `TrainSearchResults`. */

/** The exact `"HH:MM"` shape the four time filters put on the wire
 * (`from`/`to`/`arrival_from`/`arrival_to`, all parsed server-side by
 * `crates/api/src/routes/trains.rs`'s `normalize_time`).
 *
 * Deliberately KEPT now that those four fields are `@mantine/dates`'
 * `TimeInput` (a native `<input type="time">`) rather than a free-text
 * `TextInput`, because that control is NOT equivalent to this check --
 * it is narrower in one direction and wider in the other.
 *
 * Narrower: a native time input's value sanitization drops outright
 * nonsense (`"25:99"`) to `""` before it can ever reach `onChange`, so
 * that class of input no longer produces an error at all, it simply never
 * lands. Wider: the values it DOES accept are "valid HTML time strings",
 * which include a seconds component -- `"09:00:30"` survives sanitization
 * untouched even though `step` is 60 and no seconds segment is offered in
 * the UI. `"HH:MM:SS"` is not what this API parses, so that is the case
 * this pattern still genuinely catches, as an inline error rather than a
 * 400 from the server.
 *
 * It is also the guard for a browser with no `type="time"` support at all,
 * where the control degrades to a plain text field with no sanitization
 * whatsoever. */
export const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

/** The URL params `TrainSearchForm` owns; `ticketId` and anything else in
 * the URL are left alone. */
export const TRAIN_URL_PARAMS = [
  'station',
  'origin',
  'stops_at',
  'date',
  'from',
  'to',
  'arrival_from',
  'arrival_to',
  'from_day_offset',
  'to_day_offset',
  'arrival_from_day_offset',
  'arrival_to_day_offset',
  'operator',
] as const;

/** A two-character ATOC operator code, as `operator=` takes them. */
const OPERATOR_CODE = /^[A-Z0-9]{2}$/;

/** `?operator=` read tolerantly: comma-separated, trimmed, uppercased, with
 * repeats and anything that is not a code dropped and, given the known
 * operators, codes not among them dropped too, so a stale or hand-edited
 * link narrows to what can still be searched rather than failing with a
 * `400`. */
export function parseOperatorParam(raw: string | null | undefined, known?: ReadonlySet<string>): string[] {
  const codes: string[] = [];
  for (const entry of (raw ?? '').split(',')) {
    const code = entry.trim().toUpperCase();
    if (!OPERATOR_CODE.test(code) || codes.includes(code)) continue;
    if (known && known.size > 0 && !known.has(code)) continue;
    codes.push(code);
  }
  return codes;
}

/** `?*_day_offset=`: `1` is the morning after the searched date, and
 * anything else (absent, `0`, junk) the date itself. */
export function parseNextDayParam(raw: string | null | undefined): boolean {
  return raw?.trim() === '1';
}

/** `"HH:MM"` as minutes past midnight, `null` for anything else. */
export function clockMinutes(time: string): number | null {
  const trimmed = time.trim();
  if (!TIME_PATTERN.test(trimmed)) return null;
  const [hours, minutes] = trimmed.split(':').map(Number);
  return (hours ?? 0) * 60 + (minutes ?? 0);
}

/** Whether a time window's upper bound (Latest departure, Latest arrival)
 * falls after the searched date's midnight. It does when the visitor says
 * so (`ownNextDay`), and it must when it is earlier than its lower bound:
 * "from 22:00 to 01:30" can only mean 01:30 the next morning. `implied`
 * marks that second case, where the choice is not the visitor's to make.
 * The API applies the same rule to a bound sent without its day
 * (`crates/api/src/routes/trains.rs`, `resolve_window`). */
export function upperBoundNextDay(
  upper: string,
  ownNextDay: boolean,
  lower: string,
  lowerNextDay: boolean,
): { nextDay: boolean; implied: boolean } {
  const upperMinutes = clockMinutes(upper);
  const lowerMinutes = clockMinutes(lower);
  const implied =
    upperMinutes !== null && lowerMinutes !== null && upperMinutes < lowerMinutes + (lowerNextDay ? 1440 : 0);
  return { nextDay: ownNextDay || implied, implied };
}

/** The day after `searchedDate` (`"YYYY-MM-DD"`), as "Sat 24 Oct": the
 * calendar day a time after its midnight falls on. */
export function nextDateLabel(searchedDate: string): string {
  // Noon UTC is the same calendar day in London all year.
  return formatWeekdayShortDate(`${addCalendarDays(searchedDate, 1)}T12:00:00Z`);
}

/** A search's rows split at the searched date's midnight: those leaving on
 * the date itself, then those leaving after its midnight (`dayOffset` 1 or
 * more). The API returns them in train-day order, so the split is one
 * index. */
export function splitAtMidnight<Row extends Pick<TrainSearchResult, 'dayOffset'>>(
  rows: readonly Row[],
): { sameDay: Row[]; afterMidnight: Row[] } {
  const index = rows.findIndex((row) => (row.dayOffset ?? 0) >= 1);
  if (index === -1) return { sameDay: [...rows], afterMidnight: [] };
  return { sameDay: rows.slice(0, index), afterMidnight: rows.slice(index) };
}

/** The track button's noun for a search row. */
export function trackNoun(row: ServiceModeFields): 'train' | 'bus' | 'ferry' {
  if (row.serviceMode === 'ferry') return 'ferry';
  if (row.serviceMode === 'bus' || row.serviceMode === 'replacementBus') return 'bus';
  return 'train';
}

/** Exactly one of five mutually-exclusive states, checked top to bottom by
 * `TrainSearchResults` (the component). `'unpublished'` and an empty `rows` array are
 * genuinely different facts and get different copy -- the backend route
 * draws that 404-vs-empty-results distinction on purpose and collapsing it
 * here would waste it.
 *
 * `nextCursor` lives INSIDE the success variant rather than in its own
 * `useState`, so it cannot survive a state transition it does not belong
 * to: a fresh search, an error, or an unpublished response all discard it
 * automatically. `date` lives here for the same reason: it is the RAW
 * `dateValue` that was submitted with THIS search -- `''` when no date was
 * picked, exactly mirroring the condition `searchParams()` itself uses to
 * decide whether to include a `date` query param at all -- captured once at
 * submit time and carried forward untouched by `handleLoadMore`. Never
 * re-read live from the `dateValue` picker state at render time, which can
 * drift out from under already-displayed rows if the caller moves the date
 * picker without pressing Search again. See `resolvedDate` for turning this
 * into an actual calendar date for a link or an API call.
 *
 * `loadMoreFailed` rides along for the same reason: it records that the LAST
 * "Load more" press errored, so the footer can say so rather than let the
 * list read as finished, and a fresh search clears it automatically by
 * replacing this whole object. The cursor is deliberately kept on failure --
 * it is still valid, so the retry the footer offers is a real one. */
export type TrainSearchResults =
  | {
      rows: TrainSearchResult[];
      nextCursor: string | null;
      date: string;
      loadMoreFailed: boolean;
      provisional: boolean;
    }
  | 'unpublished'
  | 'error'
  | null;

/** Narrows `Results` to the "has rows" branch -- factored out for the same
 * reason `StationTimetable.tsx`'s identical helper is: `handleLoadMore`'s
 * early-return guard plus each of its functional `setResults` updaters need
 * this exact three-way check, and spelling it out at every call site invited
 * the copies to drift apart. */
export function hasRows(results: TrainSearchResults): results is {
  rows: TrainSearchResult[];
  nextCursor: string | null;
  date: string;
  loadMoreFailed: boolean;
  provisional: boolean;
} {
  return results !== null && results !== 'error' && results !== 'unpublished';
}

/** Turns a `Results` success variant's raw `date` (`''` meaning "no date was
 * picked, defaulted to today") into an actual `"YYYY-MM-DD"` to put in a
 * link or hand to `TrackThisTrainButton`. Only the `''` branch reads the
 * clock, and only to name the SAME "no date picked" default `searchParams()`
 * itself would have applied at submit time -- it does not reintroduce a live
 * read of `dateValue`. */
export function resolvedDate(rawDate: string): string {
  return rawDate || nowInLondon().format('YYYY-MM-DD');
}
