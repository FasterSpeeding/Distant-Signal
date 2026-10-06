/** Formatting for a line's `upcoming` notes (`GET /Line/{ids}/Status`,
 * docs/api-changelog.md 2026-10-06): disruptions announced for the line
 * that have not started yet. Network time, so Europe/London throughout,
 * like `dateFormat`. */
import type { UpcomingDisruption } from './types';
import { formatTime } from './dateFormat';

/** "Sun 11 Oct" */
const DAY = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  weekday: 'short',
  day: 'numeric',
  month: 'short',
});

function isLondonMidnight(iso: string): boolean {
  return formatTime(iso) === '00:00';
}

function day(value: Date | string): string {
  return DAY.format(value instanceof Date ? value : new Date(value));
}

/** When an upcoming disruption happens, in UK local time.
 *
 * Whole London days (midnight to midnight, `to` exclusive) read as days:
 * "Sun 11 Oct", or "Mon 12 Oct to Wed 14 Oct". Anything else reads with
 * times: "Fri 16 Oct, 05:00 to Fri 16 Oct, 23:30", or "from Fri 16 Oct,
 * 05:00" with no stated end. */
export function formatUpcomingWhen(note: Pick<UpcomingDisruption, 'from' | 'to'>): string {
  const { from, to } = note;
  if (to !== null && isLondonMidnight(from) && isLondonMidnight(to)) {
    // `to` is exclusive: the last day is the one just before it.
    const lastDay = day(new Date(Date.parse(to) - 1));
    const firstDay = day(from);
    return firstDay === lastDay ? firstDay : `${firstDay} to ${lastDay}`;
  }
  const start = `${day(from)}, ${formatTime(from)}`;
  if (to === null) {
    return `from ${start}`;
  }
  return `${start} to ${day(to)}, ${formatTime(to)}`;
}
