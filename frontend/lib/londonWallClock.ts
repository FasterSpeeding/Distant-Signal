/** The reverse direction of `lib/dateFormat.ts`'s "single locale/timezone
 * decision": that module turns a real instant (a `Date`/ISO string) into a
 * London wall-clock *display* string; this module turns a London wall-clock
 * string a caller has already built (typically by combining "today" with an
 * `"HH:MM"` read off a live departure board or CIF schedule row, both of
 * which are themselves Europe/London wall-clock readings) back into the real
 * UTC instant it names.
 *
 * `new Date('YYYY-MM-DDTHH:mm:ss')` (no `Z`/offset suffix) cannot do this
 * correctly: per the ECMAScript spec, a date-time string with no offset is
 * parsed in the *host's own* timezone -- the browser's, for anything built
 * client-side. For a UK visitor that happens to coincide with Europe/London,
 * masking the bug; for anyone else (a UK-based rail enthusiast on holiday
 * abroad, someone tracking a train for a UK-based relative) it silently
 * applies the WRONG offset. Concretely: a visitor on UTC+2 picking "10:00"
 * intending the advertised 10:00 London departure has that string
 * mis-parsed as 10:00 in *their own* zone, i.e. 08:00Z instead of the
 * correct 09:00Z (BST) -- close enough most of the day to go unnoticed, but
 * genuinely wrong, and capable of shifting the derived `serviceDate` (and
 * therefore the `/train/{uid}/{date}` link built from it) to the wrong
 * calendar day entirely near the London midnight boundary. See the
 * 2026-09-26 "Repeater Signal" adversarial review, finding M8.
 *
 * The backend has long had the equivalent problem the other way around
 * (interpreting a wall-clock reading as a real instant) and the equivalent
 * fix -- `crates/common/src/rail_day.rs`'s `current_rail_day`/
 * `next_rail_day_boundary`, which explicitly resolve a Europe/London
 * wall-clock boundary via `chrono-tz` rather than a raw UTC offset. This
 * module is the frontend's counterpart, using `dayjs`'s own `utc`/`timezone`
 * plugins (dayjs is already a direct dependency, already imported by every
 * call site this fixes -- `components/TrackTrainForm.tsx` -- so this adds no
 * new library, just the two plugins needed to make it timezone-aware). */
import dayjs, { type Dayjs } from 'dayjs';
import utc from 'dayjs/plugin/utc';
import timezone from 'dayjs/plugin/timezone';

dayjs.extend(utc);
dayjs.extend(timezone);

/** The one IANA zone name this module ever resolves against -- see this
 * file's own header comment for why (this is a UK rail product; every
 * departure/arrival time it deals in is already stated in this zone by the
 * upstream feeds, regardless of where the visitor themselves is). */
export const LONDON_TZ = 'Europe/London';

/** "Now", anchored to Europe/London's own wall clock rather than the
 * browser's/Node process's host zone. Safe to call from either: the
 * returned `Dayjs` still represents the same real instant `dayjs()` would
 * (nothing about "now" itself is host-zone-dependent) -- what changes is
 * that `.format(...)`/`.add(...)` on the result read/manipulate it in
 * Europe/London terms, which is what every call site combining "today"
 * with a London-wall-clock `"HH:MM"` (a live departure board row, a CIF
 * schedule row) actually needs. */
export function nowInLondon(): Dayjs {
  return dayjs().tz(LONDON_TZ);
}

/** Parses `value` -- a bare `'YYYY-MM-DD HH:mm:ss'` (or `'...THH:mm:ss'`)
 * string with no zone/offset of its own, the exact shape
 * `components/TrackTrainForm.tsx`'s `DateTimePicker`/"Now" button/live-board
 * pickers all produce -- as an Europe/London wall-clock reading, and
 * returns the real UTC instant it names as a plain `Date`. Use this
 * anywhere such a string is about to become a `scheduled_departure` sent to
 * the backend; see this file's own header comment for why a bare `new
 * Date(value)` gets this wrong for any visitor outside the UK. */
export function londonWallClockToUtc(value: string): Date {
  return dayjs.tz(value, LONDON_TZ).toDate();
}
