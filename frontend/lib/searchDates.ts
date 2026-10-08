import dayjs from 'dayjs';
import type { TrainSearchDates } from './types';

/** The `/trains` date picker's bounds when `GET /public/trains/search/dates`
 * could not be read: the API's own static window
 * (`crates/api/src/routes/trains.rs`'s `SEARCH_WINDOW_BACKWARD_DAYS`/
 * `SEARCH_WINDOW_FORWARD_DAYS`), which it accepts whatever is published. */
export const FALLBACK_SEARCH_WINDOW_DAYS = 7;

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

/** The inclusive `"YYYY-MM-DD"` bounds a date picker offers. */
export interface DateBounds {
  minDate: string;
  maxDate: string;
}

/** The `/trains` date picker's bounds: the search's own accepted range
 * (`from`/`to`) when `dates` is a usable response, otherwise
 * `FALLBACK_SEARCH_WINDOW_DAYS` either side of `today` (London,
 * `"YYYY-MM-DD"`). A malformed or inverted range counts as unusable. */
export function searchDateBounds(
  dates: Pick<TrainSearchDates, 'from' | 'to'> | null | undefined,
  today: string,
): DateBounds {
  if (dates && DATE_PATTERN.test(dates.from) && DATE_PATTERN.test(dates.to) && dates.from <= dates.to) {
    return { minDate: dates.from, maxDate: dates.to };
  }
  const base = dayjs(today);
  return {
    minDate: base.subtract(FALLBACK_SEARCH_WINDOW_DAYS, 'day').format('YYYY-MM-DD'),
    maxDate: base.add(FALLBACK_SEARCH_WINDOW_DAYS, 'day').format('YYYY-MM-DD'),
  };
}

function dayCount(days: number): string {
  return days === 1 ? '1 day' : `${days} days`;
}

/** The date field's description for `bounds`, relative to `today`:
 * "up to a week either side of today" for the fallback window, otherwise
 * how far back and ahead it reaches. */
export function searchDateDescription(bounds: DateBounds, today: string): string {
  const back = Math.max(0, dayjs(today).diff(dayjs(bounds.minDate), 'day'));
  const ahead = Math.max(0, dayjs(bounds.maxDate).diff(dayjs(today), 'day'));
  if (back === FALLBACK_SEARCH_WINDOW_DAYS && ahead === FALLBACK_SEARCH_WINDOW_DAYS) {
    return 'Search a different day, up to a week either side of today.';
  }
  return `Search a different day, up to ${dayCount(back)} back and ${dayCount(ahead)} ahead.`;
}
