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
 * That London pinning is the rule for **network-time** values — anything
 * that states a fact about the rail network's own clock (a train's
 * scheduled/actual departure, an incident's validity window, a line's
 * London-day history buckets). It has exactly one documented exception:
 * `formatLocalDateTime` below, for **viewer-relative** values — a record of
 * something the viewer themselves did in this app, where their own clock is
 * the one that answers the question. Today that is one call site,
 * `components/TicketSummary.tsx`'s "Added {time}". Reasoning and the full
 * call-site categorisation live in
 * docs/superpowers/specs/2026-09-02-client-local-timezone-display-research.md
 * (Finding 1 and its Recommendation). If you are about to reach for
 * `formatLocalDateTime` for anything a train, an incident or the aggregator
 * did, you want `formatDateTime` instead.
 *
 * Formatters are module-level constants because constructing an
 * `Intl.DateTimeFormat` is comparatively expensive and these are called
 * once per rendered row. (`formatLocalDateTime` deliberately is not — see
 * its own comment.) */
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

/** "19 Aug 2026, 19:56" in **the host's own timezone** — `formatDateTime`'s
 * options minus the `timeZone` key.
 *
 * The missing `timeZone` is the whole point and is load-bearing: per
 * ECMA-402, omitting it resolves to the host environment's own IANA zone
 * (the same value `Intl.DateTimeFormat().resolvedOptions().timeZone`
 * returns). Because that is a real zone identifier and not a frozen offset,
 * it handles the viewer's own DST transitions, which any "detect the offset
 * and apply it" approach cannot. Do not "fix" this by passing
 * `timeZone: Intl.DateTimeFormat().resolvedOptions().timeZone` — that is a
 * redundant round-trip to the value the omission already produces.
 *
 * It follows that this is **only correct in a browser, post-mount**. In the
 * Node process rendering a page server-side the host zone is the
 * container's UTC, not the viewer's, so calling this from a Server
 * Component — or from a Client Component's first, pre-hydration render —
 * emits different server and client markup for the same instant: exactly
 * the bug class this module's header exists to prevent.
 * `components/LocalDateTime.tsx` is the sanctioned way to use it; it gates
 * the call behind `useMounted()` and falls back to `formatDateTime` before
 * mount. Nothing else should call this directly.
 *
 * Constructed per call rather than hoisted to a module-level constant like
 * the four above: a constant would capture the host zone at module-load
 * time, which on the server is the container's UTC, cached process-wide for
 * every subsequent render — and it would make the local path untestable,
 * since a test that flips `process.env.TZ` could never observe the change
 * through a formatter built at import time. The cost is negligible at the
 * one call site's real scale (post-mount only, once per ticket row, on a
 * page showing a handful of one user's tickets).
 *
 * See docs/superpowers/specs/2026-09-02-client-local-timezone-display-research.md. */
export function formatLocalDateTime(value: string | Date): string {
  return new Intl.DateTimeFormat('en-GB', {
    dateStyle: 'medium',
    timeStyle: 'short',
  }).format(asDate(value));
}

/** "19:56" */
export function formatTime(value: string | Date): string {
  return TIME.format(asDate(value));
}

/** "2026-08-20" — the London calendar day, for grouping. */
export function londonDayKey(value: string | Date): string {
  return DAY_KEY.format(asDate(value));
}
