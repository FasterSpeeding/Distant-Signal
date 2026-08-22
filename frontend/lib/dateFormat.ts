/** The app's single locale/timezone decision.
 *
 * This is a UK rail product, so dates are en-GB and times are London
 * wall-clock. Both are stated explicitly on every formatter rather than
 * left to the runtime: `new Date(x).toLocaleDateString()` follows whatever
 * locale and timezone the *process* has, which is en-GB/Europe-London in a
 * British browser but en-US/UTC in the Node process rendering the page —
 * so the line detail, station and history pages were simultaneously
 * showing Americans' dates to UK users ("5/10/2026" for 10 May) and
 * emitting different server and client markup for the same timestamp. The
 * same reasoning `LastUpdated` documents for its own formatter; this module
 * is where that formatter now lives, so there is one place to get it wrong.
 *
 * Formatters are module-level constants because constructing an
 * `Intl.DateTimeFormat` is comparatively expensive and these are called
 * once per rendered row. */
const DATE = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  dateStyle: 'medium',
});

/** No `timeStyle: 'medium'`: seconds on a status timestamp are noise — the
 * aggregator recomputes every few minutes. */
const DATE_TIME = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  dateStyle: 'medium',
  timeStyle: 'short',
});

const TIME = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  timeStyle: 'short',
});

/** `en-CA` is the shortest route to a stable YYYY-MM-DD key; the point here
 * is the `Europe/London` timezone, not the locale — grouping history by the
 * UTC day would split a British summer evening across two headings. */
const DAY_KEY = new Intl.DateTimeFormat('en-CA', {
  timeZone: 'Europe/London',
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
});

function asDate(value: string | Date): Date {
  return value instanceof Date ? value : new Date(value);
}

/** "10 May 2026" */
export function formatDate(value: string | Date): string {
  return DATE.format(asDate(value));
}

/** "19 Aug 2026, 19:56" */
export function formatDateTime(value: string | Date): string {
  return DATE_TIME.format(asDate(value));
}

/** "19:56" */
export function formatTime(value: string | Date): string {
  return TIME.format(asDate(value));
}

/** "2026-08-20" — the London calendar day, for grouping. */
export function londonDayKey(value: string | Date): string {
  return DAY_KEY.format(asDate(value));
}
