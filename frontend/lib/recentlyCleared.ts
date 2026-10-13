import { formatTime } from './dateFormat';
import type { RecentlyClearedIncident } from './types';

/** The clearing summary without National Rail's "CLEARED:" prefix: the
 * section heading already says so. Left as is when nothing would remain. */
export function clearedTitle(summary: string): string {
  const title = summary.replace(/^\s*cleared\s*:\s*/i, '').trim();
  return title === '' ? summary : title;
}

/** "Was expected until 22:30" when the last live text stated an end that
 * hasn't passed yet at `now` (the incident cleared early), "Was expected
 * until the end of the day" for that phrasing, otherwise null. */
export function wasExpectedUntil(
  item: Pick<RecentlyClearedIncident, 'expectedUntil' | 'expectedUntilEndOfDay'>,
  now: number,
): string | null {
  if (item.expectedUntilEndOfDay) return 'Was expected until the end of the day';
  if (item.expectedUntil !== null && Date.parse(item.expectedUntil) > now) {
    return `Was expected until ${formatTime(item.expectedUntil)}`;
  }
  return null;
}
