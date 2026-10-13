import dayjs from 'dayjs';
import type { TrainSearchDates } from './types';
import { formatDate } from './dateFormat';

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
 * how far back and ahead it reaches, leaving out a direction it doesn't
 * reach at all (never "up to 0 days back").
 *
 * `today` is the browser's London day and `bounds` the server's, so the two
 * can disagree: a browser clock that is wrong, or a page left open past
 * midnight. When `today` falls outside `bounds` a count relative to it
 * would be wrong, so the range reads as dates ("5 Oct 2026 to 19 Oct
 * 2026"). */
export function searchDateDescription(bounds: DateBounds, today: string): string {
  if (today < bounds.minDate || today > bounds.maxDate) {
    if (bounds.minDate === bounds.maxDate) {
      return `Only ${formatDate(bounds.minDate)} can be searched.`;
    }
    return `Search a day from ${formatDate(bounds.minDate)} to ${formatDate(bounds.maxDate)}.`;
  }
  const back = dayjs(today).diff(dayjs(bounds.minDate), 'day');
  const ahead = dayjs(bounds.maxDate).diff(dayjs(today), 'day');
  if (back === FALLBACK_SEARCH_WINDOW_DAYS && ahead === FALLBACK_SEARCH_WINDOW_DAYS) {
    return 'Search a different day, up to a week either side of today.';
  }
  if (back === 0 && ahead === 0) {
    return 'Only today can be searched.';
  }
  if (back === 0) {
    return `Search a later day, up to ${dayCount(ahead)} ahead.`;
  }
  if (ahead === 0) {
    return `Search an earlier day, up to ${dayCount(back)} back.`;
  }
  return `Search a different day, up to ${dayCount(back)} back and ${dayCount(ahead)} ahead.`;
}
