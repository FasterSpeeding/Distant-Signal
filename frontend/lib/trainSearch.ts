import { nowInLondon } from '@/lib/londonWallClock';
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
