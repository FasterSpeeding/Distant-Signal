import { cookies } from 'next/headers';
import type {
  LineStatusReport,
  LineStatusHistoryEntry,
  LineDailyStats,
  LineHalfHourlyStats,
  LineHourlyStats,
  LineSixHourlyStats,
  LineDailyCoverageStats,
  LineHalfHourlyCoverageStats,
  OperatorDailyStats,
  OperatorHalfHourlyStats,
  OperatorHourlyStats,
  OperatorSixHourlyStats,
  NetworkDailyStats,
  NetworkHalfHourlyStats,
  NetworkHourlyStats,
  NetworkSixHourlyStats,
  Preferences,
  LineSummary,
  CustomLineDetail,
  LineDefinitionSummary,
  DataFreshness,
  HistoryRetention,
  Suggestion,
  SessionInfo,
  TrackedTrainState,
  PublicTrainState,
  LineTrainEntry,
  TrackedTrainListItem,
  TrackedTrainTicket,
  DelayRepayEstimateResponse,
  TicketListItem,
  IncidentDetail,
  StationOperatorSampleStats,
  StationAccessibilityData,
  GroupSummary,
  GroupDetail,
  GroupMember,
  GroupTrain,
  SharedGroupTrain,
  GroupCustomLine,
  SharedGroupCustomLine,
  GroupJourney,
  SharedGroupJourney,
  GroupJoinPreview,
  OperatorSummary,
  JourneyDetail,
  JourneyListItem,
  JourneyTemplateListItem,
  JourneyTemplateDetail,
} from './types';

/** Thrown when the API responds 404 — lets callers distinguish "genuinely
 * not found" from other failures (network errors, 500s, etc.). */
export class ApiNotFoundError extends Error {}

/** Thrown when the API responds 401 -- lets callers distinguish "not logged
 * in at all" from `ApiNotFoundError`'s "doesn't exist / isn't yours"
 * (which stays deliberately indistinguishable from each other, per this
 * app's 401-vs-404 convention -- see
 * docs/superpowers/specs/2026-08-31-private-custom-lines-and-tracked-trains-design.md). */
export class ApiUnauthorizedError extends Error {}

/** Thrown when the API responds 403 -- so far only `GET
 * /public/chatbot/access` (embedded-chatbot-option-b plan, Task 2) uses
 * this status, for a real, logged-in user who simply isn't allowlisted.
 * Deliberately its own error type rather than collapsing into the generic
 * `Error` fallback below: `getChatbotAccess()` needs to tell this apart
 * from `ApiUnauthorizedError` ("no session at all") to render the right of
 * two different page states. */
export class ApiForbiddenError extends Error {}

function baseUrl(): string {
  const url = process.env.API_BASE_URL;
  if (!url) {
    throw new Error('API_BASE_URL environment variable is not set');
  }
  return url;
}

/** The single place a non-ok response becomes an exception. Shared by
 * `fetchJson` and `getPreferences` (which needs its own fetch, but must
 * fail identically for every status it does *not* special-case) so the two
 * paths can't drift on which statuses map to `ApiNotFoundError`. */
function errorForResponse(url: string, response: Response): Error {
  const message = `API request to ${url} failed: ${response.status} ${response.statusText}`;
  if (response.status === 404) return new ApiNotFoundError(message);
  if (response.status === 401) return new ApiUnauthorizedError(message);
  if (response.status === 403) return new ApiForbiddenError(message);
  return new Error(message);
}

async function fetchJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<T>;
}

/** The session cookie's own name -- must match `SESSION_COOKIE_NAME` in
 * `app/connect-claude/authorize/route.ts` and `crates/api/src/auth.rs`. */
const SESSION_COOKIE_NAME = 'distant_signal_session';

/** Builds the `fetch`/`fetchJson` `init` fragment that forwards ONLY the
 * incoming request's session cookie to the backend -- a Server Component's
 * own `fetch` never inherits any cookie automatically. Returns `{}` (no
 * `Cookie` header at all) when the visitor has no session cookie, matching
 * every existing cookie-forwarding call site's own conditional shape.
 *
 * Deliberately forwards only the one named cookie rather than the whole
 * incoming jar (`(await cookies()).toString()`, this function's own earlier
 * behaviour, and the shape every call site below used to duplicate inline)
 * -- Signal Box Audit, flib Low finding: "the entire cookie jar is forwarded
 * on every SSR fetch". The backend only ever needs the session cookie to
 * authenticate the caller; any OTHER cookie a future feature sets on this
 * origin (an A/B flag, a consent banner, ...) has no business leaving the
 * frontend pod for the backend's own separate origin, and forwarding the raw
 * `Cookie` header would ship it there unconditionally. */
async function cookieForwardInit(): Promise<RequestInit> {
  const session = (await cookies()).get(SESSION_COOKIE_NAME)?.value;
  return session ? { headers: { Cookie: `${SESSION_COOKIE_NAME}=${session}` } } : {};
}

/* ---------------------------------------------------------------------------
 * Path-segment encoding invariant (security)
 *
 * EVERY caller-supplied `string` spliced into a backend URL below -- path
 * segment or query value -- goes through `encodeURIComponent`. This is not
 * cosmetic. These helpers run in Server Components, most of them forward the
 * visitor's own session cookie (`cookieForwardInit`), and their arguments
 * come overwhelmingly from Next.js dynamic route params -- which Next
 * *decodes* before a page component ever sees them. So a visitor-crafted URL
 * like `/lines/..%2F..%2Fmetrics%3F` arrives here as the literal string
 * `../../metrics?`, and an unencoded `${id}` lets `fetch` resolve it away
 * from the intended route entirely: `new URL('http://api' + '/StopPoint/' +
 * '../../metrics?' + '/Disruption')` is `http://api/metrics?/Disruption`.
 * That is a confused deputy -- the frontend pod reaching the backend's own
 * unauthenticated operational endpoints from inside the cluster, with the
 * visitor's cookie attached, on the visitor's say-so.
 *
 * `encodeURIComponent` closes it because it percent-encodes `/`, `?`, `#`
 * and `..`'s separators, so the segment can only ever *be* a segment.
 *
 * Helpers whose id parameter is typed `number` (`getTrackedTrainById`,
 * `getJourney`, `getJourneyTemplate`, `getTicketsForTrackedTrain`,
 * `getDelayRepayEstimate`) are deliberately left uninterpolated-but-unencoded:
 * a `number` cannot carry a `/`, `?` or `.`-pair, and every call site coerces
 * with `Number(...)` first (worst case `NaN`, a harmless 4-char segment). If
 * one of those signatures ever widens to `string`, it must gain
 * `encodeURIComponent` at the same time.
 * ------------------------------------------------------------------------- */

export async function getLineStatusForMode(mode: string): Promise<LineStatusReport[]> {
  // `mode` may be a comma-separated list (`'national-rail,tube,tram'`), the
  // comma being this route's own separator -- so it's encoded per-element and
  // rejoined, exactly like `getLineStatus`'s `ids` below. See that function's
  // comment, and the encoding-invariant note above.
  const modeParam = mode
    .split(',')
    .map((m) => encodeURIComponent(m))
    .join(',');
  const url = `${baseUrl()}/Line/Mode/${modeParam}/Status`;
  return fetchJson<LineStatusReport[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

export async function getLineStatus(ids: string[], detail: boolean): Promise<LineStatusReport[]> {
  // Each id is encoded individually and the commas re-added afterwards: the
  // comma is this route's own multi-id separator (`/Line/{ids}/Status`), so
  // encoding the joined string would encode the separators too and the
  // backend would see one absurd single id. Encoding per-id keeps the
  // separator meaningful while still making each id un-escapable.
  const idsParam = ids.map((id) => encodeURIComponent(id)).join(',');
  const query = detail ? '?detail=true' : '';
  const url = `${baseUrl()}/Line/${idsParam}/Status${query}`;
  return fetchJson<LineStatusReport[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

export async function getStopPointDisruption(crs: string): Promise<LineStatusReport[]> {
  return fetchJson<LineStatusReport[]>(`${baseUrl()}/StopPoint/${encodeURIComponent(crs)}/Disruption`, {
    cache: 'no-store',
  });
}

/** `GET /public/stations/{crs}/sample-stats` -- per-(station, operator)
 * delay/cancellation stats, computed on demand by
 * `crates/api/src/routes/station_stats.rs`. Public, unauthenticated read,
 * same `no-store` convention as `getStopPointDisruption`. Throws
 * `ApiNotFoundError` on a 404 (station isn't part of live sampling at
 * all) via `errorForResponse`, same as every other `fetchJson` caller --
 * `fetchStationSampleStats` in `app/stations/[crs]/page.tsx` catches it. */
export async function getStationSampleStats(crs: string): Promise<StationOperatorSampleStats[]> {
  return fetchJson<StationOperatorSampleStats[]>(`${baseUrl()}/public/stations/${encodeURIComponent(crs)}/sample-stats`, {
    cache: 'no-store',
  });
}

/** `GET /public/stations/{crs}/accessibility` -- filtered RDM station
 * facilities/accessibility data (design spec Decisions 3-4). Cached for an
 * hour, same convention as `getStationName`/`getAllTocs`: the underlying
 * feed's own documented poll interval is 24 hours
 * (`crates/poller-stations/src/main.rs`), so this is reference data, not a
 * live feed, and does not warrant `cache: 'no-store'`. Throws
 * `ApiNotFoundError` on a 404 (no `stations` row for this CRS at all) via
 * `errorForResponse`, same as every other `fetchJson` caller --
 * `fetchStationAccessibility` in `app/stations/[crs]/page.tsx` catches it
 * and renders it as a different sentence from a `200 {}`. */
export async function getStationAccessibility(crs: string): Promise<StationAccessibilityData> {
  return fetchJson<StationAccessibilityData>(`${baseUrl()}/public/stations/${encodeURIComponent(crs)}/accessibility`, {
    next: { revalidate: 3600 },
  });
}

/** Resolves a CRS code to its station name, for display (e.g. station
 * disruption page headings). `/public/stations` is the same substring
 * type-ahead search backing the autocomplete fields — not an exact-match
 * lookup — so this filters its results for the row whose `code` equals
 * `crs` exactly. Returns `null` (rather than throwing) when no such row
 * comes back, so callers can fall back to displaying the bare code.
 * Cached for an hour rather than `no-store` like the live feeds around it:
 * this is reference data that changes on the order of years, and every
 * render of `/stations/[crs]` would otherwise pay a round-trip for a
 * heading. */
export async function getStationName(crs: string): Promise<string | null> {
  const results = await fetchJson<Suggestion[]>(`${baseUrl()}/public/stations?q=${encodeURIComponent(crs)}`, {
    next: { revalidate: 3600 },
  });
  const match = results.find((s) => s.code.toUpperCase() === crs.toUpperCase());
  return match ? match.name : null;
}

export async function getLineStatusHistory(
  id: string,
  from: string,
  to: string,
): Promise<LineStatusHistoryEntry[]> {
  const url = `${baseUrl()}/Line/${encodeURIComponent(id)}/Status/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`;
  return fetchJson<LineStatusHistoryEntry[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

/** `GET /Line/{id}/Stats/{from}/to/{to}` -- the new daily rollup route.
 * `from`/`to` are `YYYY-MM-DD` calendar days (the route's own path segments
 * are `NaiveDate`, not RFC3339 instants -- see the backend plan's Task 4).
 * Same public, no-store, cookie-forwarding shape as
 * `getLineStatusHistory` -- and for the same reason: the backend gates a
 * `custom-` line id on the caller's session, so without the forward the
 * owner of a private line would see an empty chart on their own line's
 * history page. */
export async function getLineDailyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineDailyStats[]> {
  return fetchJson<LineDailyStats[]>(
    `${baseUrl()}/Line/${encodeURIComponent(id)}/Stats/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store', ...(await cookieForwardInit()) },
  );
}

/** `GET /Line/{id}/Stats/HalfHourly/{from}/to/{to}` -- the half-hourly
 * rollup route (30-minute buckets). Unlike `getLineDailyStats`, `from`/
 * `to` are passed straight through as RFC3339 instants (no `londonDayKey`
 * conversion) -- the route's own path segments are `DateTime<Utc>`, not
 * `NaiveDate`, since a 30-minute bucket has no calendar-day analog to
 * round-trip through (Decision 6 of
 * docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md,
 * written for the original 1-hour bucket -- the reasoning is unchanged at
 * 30 minutes). Same public, no-store, cookie-forwarding shape as
 * `getLineDailyStats`/`getLineStatusHistory`. Originally
 * `getLineHourlyStats` calling `/Stats/Hourly/...`; renamed alongside the
 * backend route when the bucket size was halved -- see git history for
 * the hourly-era version. */
export async function getLineHalfHourlyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineHalfHourlyStats[]> {
  return fetchJson<LineHalfHourlyStats[]>(
    `${baseUrl()}/Line/${encodeURIComponent(id)}/Stats/HalfHourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store', ...(await cookieForwardInit()) },
  );
}

/** `GET /Line/{id}/Stats/Hourly/{from}/to/{to}` -- the 1-hour sub-daily
 * rollup route (Decision 2 of
 * docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md).
 * Same RFC3339-instant/public/no-store/cookie-forwarding shape as
 * `getLineHalfHourlyStats`. */
export async function getLineHourlyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineHourlyStats[]> {
  return fetchJson<LineHourlyStats[]>(
    `${baseUrl()}/Line/${encodeURIComponent(id)}/Stats/Hourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store', ...(await cookieForwardInit()) },
  );
}

/** `GET /Line/{id}/Stats/SixHourly/{from}/to/{to}` -- the 6-hour sub-daily
 * rollup route, sibling of `getLineHourlyStats`. */
export async function getLineSixHourlyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineSixHourlyStats[]> {
  return fetchJson<LineSixHourlyStats[]>(
    `${baseUrl()}/Line/${encodeURIComponent(id)}/Stats/SixHourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store', ...(await cookieForwardInit()) },
  );
}

/** `GET /Line/{id}/Stats/Coverage/{from}/to/{to}` -- the full-coverage
 * sibling of `getLineDailyStats` (Decision 4). Same `YYYY-MM-DD`/no-store/
 * cookie-forwarding shape. Always resolves `[]` today: no full-coverage
 * producer exists yet to populate `line_status_daily_coverage_stats`. See
 * docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
 * Decision 4. */
export async function getLineDailyCoverageStats(
  id: string,
  from: string,
  to: string,
): Promise<LineDailyCoverageStats[]> {
  return fetchJson<LineDailyCoverageStats[]>(
    `${baseUrl()}/Line/${encodeURIComponent(id)}/Stats/Coverage/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store', ...(await cookieForwardInit()) },
  );
}

/** `GET /Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}` -- the
 * full-coverage sibling of `getLineHalfHourlyStats`. Added for backend
 * symmetry with the daily route above (Decision 4); no frontend chart
 * consumes it yet -- see `CoverageTrendsResults.tsx`'s own doc comment for
 * why only the daily series has a chart in this pass. */
export async function getLineHalfHourlyCoverageStats(
  id: string,
  from: string,
  to: string,
): Promise<LineHalfHourlyCoverageStats[]> {
  return fetchJson<LineHalfHourlyCoverageStats[]>(
    `${baseUrl()}/Line/${encodeURIComponent(id)}/Stats/Coverage/HalfHourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store', ...(await cookieForwardInit()) },
  );
}

/** `GET /public/operators/{code}/stats/{from}/to/{to}` -- the operator-scoped
 * daily Trends rollup (Phase 4). Public, unauthenticated -- no
 * `cookieForwardInit()`, matching `getHistoryRetention`/`getDataFreshness`'s
 * own precedent for a genuinely public endpoint, unlike the per-line
 * `getLineDailyStats` family (which forwards cookies because a `custom-`
 * id might be in play; this route's line-id set never includes one). */
export async function getOperatorDailyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorDailyStats[]> {
  return fetchJson<OperatorDailyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store' },
  );
}

/** Half-hourly sibling of `getOperatorDailyStats` -- `from`/`to` are RFC3339
 * instants, same reasoning as `getLineHalfHourlyStats`. */
export async function getOperatorHalfHourlyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorHalfHourlyStats[]> {
  return fetchJson<OperatorHalfHourlyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/half-hourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store' },
  );
}

/** 1-hour sub-daily sibling, mirrors `getLineHourlyStats`. */
export async function getOperatorHourlyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorHourlyStats[]> {
  return fetchJson<OperatorHourlyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/hourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store' },
  );
}

/** 6-hour sub-daily sibling, mirrors `getLineSixHourlyStats`. */
export async function getOperatorSixHourlyStats(
  code: string,
  from: string,
  to: string,
): Promise<OperatorSixHourlyStats[]> {
  return fetchJson<OperatorSixHourlyStats[]>(
    `${baseUrl()}/public/operators/${encodeURIComponent(code)}/stats/six-hourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store' },
  );
}

/** `GET /public/network/stats/{from}/to/{to}` -- the whole-network
 * (catalogue National Rail lines only -- see this plan's Judgment Call 5
 * for why TfL lines never contribute) daily Trends rollup. */
export async function getNetworkDailyStats(from: string, to: string): Promise<NetworkDailyStats[]> {
  return fetchJson<NetworkDailyStats[]>(`${baseUrl()}/public/network/stats/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`, {
    cache: 'no-store',
  });
}

export async function getNetworkHalfHourlyStats(
  from: string,
  to: string,
): Promise<NetworkHalfHourlyStats[]> {
  return fetchJson<NetworkHalfHourlyStats[]>(
    `${baseUrl()}/public/network/stats/half-hourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store' },
  );
}

export async function getNetworkHourlyStats(from: string, to: string): Promise<NetworkHourlyStats[]> {
  return fetchJson<NetworkHourlyStats[]>(`${baseUrl()}/public/network/stats/hourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`, {
    cache: 'no-store',
  });
}

export async function getNetworkSixHourlyStats(
  from: string,
  to: string,
): Promise<NetworkSixHourlyStats[]> {
  return fetchJson<NetworkSixHourlyStats[]>(
    `${baseUrl()}/public/network/stats/six-hourly/${encodeURIComponent(from)}/to/${encodeURIComponent(to)}`,
    { cache: 'no-store' },
  );
}

/** The only endpoint in this file that is *per-user* rather than shared,
 * so the only one that needs both of the following. Deliberately not routed
 * through `fetchJson`:
 *
 * 1. **Cookie forwarding.** This runs in a Server Component, and a Server
 *    Component's own `fetch` carries none of the browser's cookies — it is
 *    a fresh server-to-server request, not a continuation of the incoming
 *    one. Without explicitly re-attaching the incoming request's `Cookie`
 *    header, `/public/preferences` (which requires an authenticated user)
 *    would never see a logged-in visitor's session and would 401 even for
 *    them. `cookies()` from `next/headers` is what reads that incoming
 *    header. (The browser-initiated path — `components/PinToggle.tsx` —
 *    doesn't need this: it goes through the same-origin `/api/*` proxy,
 *    which the browser attaches cookies to itself and which forwards them
 *    on.)
 * 2. **401 tolerance.** An anonymous visitor has no preferences, and that
 *    is a perfectly normal state: the home dashboard, All Lines and every
 *    station page must still render for them, just with nothing pinned.
 *    A 401 here therefore means "no preferences", not "this page is
 *    broken". This tolerance is scoped to this endpoint alone — `fetchJson`
 *    still throws on 401 for everything else, where an unexpected 401 is a
 *    genuine failure worth surfacing.
 *
 * Every other non-ok status still throws, via the same `errorForResponse`
 * `fetchJson` uses. */
export async function getPreferences(): Promise<Preferences> {
  const url = `${baseUrl()}/public/preferences`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401) {
    return { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<Preferences>;
}

/** Per-user like `getPreferences`, so it needs the same cookie forwarding
 * (see that function's comment for the full explanation of why a Server
 * Component's own `fetch` doesn't automatically carry the incoming
 * request's cookies). Unlike `/public/preferences`, though,
 * `/public/auth/session` never 401s — an anonymous visitor gets a normal
 * 200 with `authenticated: false` — so this can go through the shared
 * `fetchJson` instead of needing its own 401-tolerant branch.
 *
 * Because it never 401s, EVERY rejection this throws is a genuine failure
 * to answer the question at all (network error, timeout, or a 5xx --
 * including a real DB error during the session lookup now that
 * `OptionalAuthenticatedUser` correctly propagates one instead of
 * collapsing it into `Ok(None)`), never a confirmed "not logged in". Do
 * not let a caller's `.catch()` treat that the same as a confirmed
 * `authenticated: false` -- see `getSessionOrLoggedOut()` below, which is
 * what every "degrade rather than crash" caller in this app should call
 * instead of writing its own `.catch()` around this. */
export async function getSession(): Promise<SessionInfo> {
  return fetchJson<SessionInfo>(`${baseUrl()}/public/auth/session`, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

/** The shape `getSession()` returns for a visitor with no session, and the
 * fallback `getSessionOrLoggedOut()` below degrades to when the session
 * check fails outright. Exported so every caller that needs a "render as
 * logged out" value -- confirmed or not -- shares this one object rather
 * than each hand-rolling its own copy (`app/layout.tsx`'s `<Suspense>`
 * fallback, rendered before the check has even started, is the one
 * legitimate reason to reference this directly instead of going through
 * `getSessionOrLoggedOut()`). */
export const LOGGED_OUT_SESSION: SessionInfo = {
  authenticated: false,
  id: null,
  email: null,
  name: null,
};

/** The one place `getSession()`'s failure mode is turned into a UI-safe
 * fallback -- every page/component below that used to write its own
 * `getSession().catch(() => ({ authenticated: false, ... }))` (or collapse
 * a rejection into `null`/`true` further down the chain) now calls this
 * instead. That used to be entirely silent: a network error, a timeout, or
 * a backend 5xx (see `getSession()`'s own doc comment above for why the
 * last of those just became newly reachable) rendered EXACTLY the same nav
 * link and page state as a visitor who was genuinely, confirmedly never
 * logged in -- no log line, no distinct state, anywhere. A logged-in user
 * whose session check merely had a bad moment saw themselves logged out,
 * indistinguishable from someone who never signed in at all.
 *
 * The fix keeps the same fail-safe UI (there is no positively-confirmed
 * identity to show, so degrading to the logged-out nav/page state is still
 * the only safe default -- this is not a redesign of the auth UI) but adds
 * the one thing that was missing: a `console.error` so the failure leaves
 * an actual trace in server logs instead of vanishing, matching this
 * codebase's existing pattern for a tolerated-but-unexpected fetch failure
 * (see e.g. `app/lines/[id]/history/page.tsx`'s `console.error` on a failed
 * history load, or `app/error.tsx`'s on an unhandled render error). */
export async function getSessionOrLoggedOut(): Promise<SessionInfo> {
  try {
    return await getSession();
  } catch (err) {
    console.error('getSession() failed; rendering as logged out, but this is NOT a confirmed logged-out state', err);
    return LOGGED_OUT_SESSION;
  }
}

/** `/chat`'s own three-state page-load gate (embedded-chatbot-option-b
 * plan, Task 5 Step 1 -- `getChatbotAccess()`'s own design note there
 * flagged extending `errorForResponse` with a real `ApiForbiddenError`
 * over a string-return sketch as the choice to make once one exists; it
 * now does, see above). Extends `getMyTrackedTrains()`'s own `null`-means-
 * "not logged in" convention with the allowlist's own third state, since
 * two booleans (`ApiUnauthorizedError`/`ApiForbiddenError`) collapse
 * neither into the other, unlike every other 401-only gate in this file. */
export async function getChatbotAccess(): Promise<'allowed' | 'unauthenticated' | 'forbidden'> {
  try {
    await fetchJson(`${baseUrl()}/public/chatbot/access`, {
      cache: 'no-store',
      ...(await cookieForwardInit()),
    });
    return 'allowed';
  } catch (err) {
    if (err instanceof ApiUnauthorizedError) return 'unauthenticated';
    if (err instanceof ApiForbiddenError) return 'forbidden';
    // Any other failure (network error, 5xx) is treated the same as "not
    // available" -- same fail-closed posture orchestrator/'s own
    // checkChatbotAccess (Task 3) takes for an ambiguous response: never
    // render the chat UI on an answer this page can't positively confirm
    // as "allowed".
    return 'forbidden';
  }
}

export async function getAllLines(): Promise<LineSummary[]> {
  const url = `${baseUrl()}/public/lines`;
  return fetchJson<LineSummary[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

/** Every operator with at least one currently-tracked public line
 * (`crates/api/src/routes/operators.rs`'s `list_operators`) -- real ATOC
 * codes from `tocs` plus a synthetic `"TfL"` row, each with a rolled-up
 * worst status + merged sample stats. Unauthenticated and caller-identity-
 * independent (unlike `getAllLines`, which appends the caller's own custom
 * lines) -- no cookie forwarding needed. `cache: 'no-store'`, same as
 * `getAllLines`/`getLineStatusForMode`: this is live status data, not
 * slow-changing reference data. */
export async function getAllOperators(): Promise<OperatorSummary[]> {
  const url = `${baseUrl()}/public/operators`;
  return fetchJson<OperatorSummary[]>(url, { cache: 'no-store' });
}

/** Single-operator rollup (`crates/api/src/routes/operators.rs`'s
 * `get_operator`) -- same shape as one row of `getAllOperators()`, fetched
 * directly rather than filtering the full list client-side, for a page that
 * only needs one operator's rollup (e.g. the operator history page's line
 * count). Unauthenticated, same as `getAllOperators`. Throws
 * `ApiNotFoundError` on a 404 -- an unknown code, or a real `tocs` code with
 * zero currently-matching lines (Judgment Call 4, same omission as the
 * list). */
export async function getOperator(code: string): Promise<OperatorSummary> {
  const url = `${baseUrl()}/public/operators/${encodeURIComponent(code)}`;
  return fetchJson<OperatorSummary>(url, { cache: 'no-store' });
}

/** Every TOC (code + name), for resolving a fixed known set of operator
 * codes up front (e.g. the All Lines operator filter) rather than
 * type-ahead searching one at a time. Cached for an hour like
 * `getStationName` — this is reference data that barely changes. */
export async function getAllTocs(): Promise<Suggestion[]> {
  return fetchJson<Suggestion[]>(`${baseUrl()}/public/tocs/all`, {
    next: { revalidate: 3600 },
  });
}

/** Deliberately collapses a `401` into `ApiNotFoundError` alongside the
 * genuine `404`, unlike `getTrackedTrainById`/`getTrackedTrainByUidAndDate`
 * below -- Decision 8 in the design spec. On `/lines/[id]`, "not logged in"
 * and "logged in but not the owner" should render identically (both just a
 * 404): this is a public catalogue page most visitors have no reason to
 * think they own, unlike the single-purpose tracked-train page, so there's
 * no case here worth a distinct "please log in, this might be yours"
 * prompt. */
export async function getCustomLine(id: string): Promise<CustomLineDetail> {
  const url = `${baseUrl()}/public/lines/${encodeURIComponent(id)}`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401 || response.status === 404) {
    throw new ApiNotFoundError(`API request to ${url} failed: ${response.status}`);
  }
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<CustomLineDetail>;
}

export async function getLineDefinition(id: string): Promise<LineDefinitionSummary> {
  const url = `${baseUrl()}/public/lines/${encodeURIComponent(id)}/definition`;
  return fetchJson<LineDefinitionSummary>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

/** `GET /public/lines/{id}/trains?date=` -- every scheduled UID on line
 * `id` for one rail day, each paired with its live status where one
 * already exists (`crates/api/src/routes/lines.rs`'s `get_line_trains`).
 * `date` is `"YYYY-MM-DD"`; when omitted the backend defaults to its own
 * UTC "today" (`resolve_schedule_date`) -- callers that build a link from
 * this response (e.g. `/train/{uid}/{date}`) should always pass `date`
 * explicitly instead, so the fetched day and the link's day can never
 * disagree (see `LineTrainsResults`'s own use of `londonDayKey`, this
 * app's stated London-calendar-day convention, `lib/dateFormat.ts`).
 * 404s (`ApiNotFoundError`) when there is no CIF-derived schedule
 * population for this `(id, date)` -- an unpublished catalogue line, a
 * custom line, a TfL line (neither ever has one at all -- see
 * `get_line_schedule`'s own doc comment), or a rail day not yet published,
 * all indistinguishable from here, same as `getLineDefinition`'s sibling
 * route just above. */
export async function getLineTrains(id: string, date?: string): Promise<LineTrainEntry[]> {
  const query = date ? `?date=${encodeURIComponent(date)}` : '';
  const url = `${baseUrl()}/public/lines/${encodeURIComponent(id)}/trains${query}`;
  return fetchJson<LineTrainEntry[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

/** `init` is optional and additive so existing call sites are unchanged.
 * It exists for `app/layout.tsx`, which awaits this call before emitting
 * any HTML and therefore needs to bound it -- see that call site's comment
 * for why an unbounded wait there is worse than no call at all. */
export async function getDataFreshness(init?: Pick<RequestInit, 'signal'>): Promise<DataFreshness> {
  return fetchJson<DataFreshness>(`${baseUrl()}/public/freshness`, {
    cache: 'no-store',
    ...init,
  });
}

/** How many days of `line_status_history` the backend actually retains —
 * see `lib/types.ts`'s `HistoryRetention` doc. Fetched by the
 * `/lines/[id]/history` page so it can tell the user honestly when a
 * requested range extends beyond what's actually kept. */
export async function getHistoryRetention(): Promise<HistoryRetention> {
  return fetchJson<HistoryRetention>(`${baseUrl()}/public/history-retention`, {
    cache: 'no-store',
  });
}

export async function getTrackedTrainById(id: number): Promise<TrackedTrainState> {
  const url = `${baseUrl()}/Train/${id}`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<TrackedTrainState>;
}

/** `GET /Train/by-uid/{uid}/{date}` -- PUBLIC and UNSCOPED, and typed as
 * the `PublicTrainState` it actually returns.
 *
 * This used to be `getTrackedTrainByUidAndDate`, declared as returning
 * `TrackedTrainState` and casting the body straight into it with no
 * checking of any kind. That was wrong on both counts once the route
 * became public: the response has an entirely different shape, and its
 * `trainsId` (then named `id`) was being read by the calling page as a
 * `trackingId` for the `/Train/{trackingId}` rename/delete/ticket routes,
 * which key off `train_subscriptions.id` -- a different `BIGSERIAL` space
 * that also starts at 1.
 *
 * No cookies are forwarded: there is no per-caller component to this
 * response, and forwarding a session cookie would only imply otherwise.
 * A 404 (no known train for that uid/date) still surfaces as
 * `ApiNotFoundError` via `errorForResponse`; a 401 is not a reachable
 * outcome for this route at all. */
export async function getPublicTrainByUidAndDate(
  uid: string,
  date: string,
): Promise<PublicTrainState> {
  const url = `${baseUrl()}/Train/by-uid/${encodeURIComponent(uid)}/${encodeURIComponent(date)}`;
  const response = await fetch(url, { cache: 'no-store' });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<PublicTrainState>;
}

/** `GET /Train/mine`. Returns `null` on `401` (not logged in) --
 * deliberately not `ApiNotFoundError`, matching `getTicketsForTrackedTrain`'s
 * precedent of treating "no session" as an expected outcome, not a
 * failure. Unlike that function, there is no second, distinct 404-shaped
 * outcome to also collapse into `null` here -- there's no id in this
 * route's path to be wrong about, so a 401 from this one call is the
 * complete, unambiguous signal. `app/track/mine/page.tsx` does NOT need a
 * separate `getSession()` call the way `TicketPanel` does. */
export async function getMyTrackedTrains(): Promise<TrackedTrainListItem[] | null> {
  const url = `${baseUrl()}/Train/mine`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TrackedTrainListItem[]>;
}

/** `GET /Journeys/{id}` -- same error-mapping contract as
 * `getTrackedTrainById` immediately above: throws `ApiNotFoundError` on a
 * 404 (doesn't exist, or isn't this caller's -- indistinguishable, per
 * this app's 404-never-403 convention) and `ApiUnauthorizedError` on a
 * 401 (not logged in at all) via `errorForResponse`, so
 * `app/journeys/[id]/page.tsx` can render the same two distinct page
 * states `app/train/by-id/[trackingId]/page.tsx` already does. */
export async function getJourney(id: number): Promise<JourneyDetail> {
  const url = `${baseUrl()}/Journeys/${id}`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyDetail>;
}

/** `GET /Journeys/shared/{token}` -- genuinely unauthenticated (no cookie
 * needed, though harmless if sent): resolves an unlisted share-link
 * token to the same `JourneyDetail` shape `getJourney` returns, with
 * `isOwner: false` and `shareLink: null` always. Throws `ApiNotFoundError`
 * for an unknown, expired, or revoked token -- same contract
 * `getGroupJoinPreview` already uses for its own token-not-found case. */
export async function getJourneyByShareToken(token: string): Promise<JourneyDetail> {
  const url = `${baseUrl()}/Journeys/shared/${encodeURIComponent(token)}`;
  const response = await fetch(url, { cache: 'no-store' });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyDetail>;
}

/** `GET /Journeys/mine` -- `null` on a `401`, same "not logged in" signal
 * `getMyTrackedTrains` already uses (no id in this route's path to
 * disambiguate a second way). Not consumed by any page in this plan (see
 * this plan's own Non-goals: no `/journeys/mine` list page yet) --
 * implemented now, independently testable, for a follow-up list page to
 * consume later without a backend change. */
export async function getMyJourneys(): Promise<JourneyListItem[] | null> {
  const url = `${baseUrl()}/Journeys/mine`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyListItem[]>;
}

/** `GET /JourneyTemplates/mine` -- same `null`-on-401 "not logged in"
 * contract as `getMyJourneys` (no id in this route's path to disambiguate
 * a second way). */
export async function getMyJourneyTemplates(): Promise<JourneyTemplateListItem[] | null> {
  const url = `${baseUrl()}/JourneyTemplates/mine`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyTemplateListItem[]>;
}

/** `GET /JourneyTemplates/{id}` -- same error-mapping contract as
 * `getJourney`: throws `ApiNotFoundError` on a 404 (doesn't exist, or
 * isn't this caller's — templates have no group-shared read path in
 * Phase B, unlike a journey) and `ApiUnauthorizedError` on a 401, so
 * `app/journeys/templates/[id]/page.tsx` can render the same two distinct
 * page states `app/journeys/[id]/page.tsx` already does. */
export async function getJourneyTemplate(id: number): Promise<JourneyTemplateDetail> {
  const url = `${baseUrl()}/JourneyTemplates/${id}`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<JourneyTemplateDetail>;
}

/** Per-user, session-gated ticket list for one tracked train
 * (`GET /Train/{trackingId}/tickets`). Same cookie-forwarding pattern as
 * `getPreferences`/`getSession` (a Server Component's own fetch does not
 * inherit the incoming request's cookies). Returns `null` on BOTH `401`
 * and `404` -- deliberately not thrown as `ApiNotFoundError`, since "you're
 * not the owner of this pin" is an expected, common outcome for a public,
 * shareable tracked-train page (every non-owner viewer hits this), not an
 * exceptional one. This collapses two different real conditions (not
 * logged in at all vs. logged in but not the owner) into one `null` --
 * `components/TicketPanel.tsx` tells them apart itself by separately
 * calling the existing `getSession()` first, since widening this
 * function's own signature would depart from the design spec's own
 * hand-written contract for it. */
export async function getTicketsForTrackedTrain(trackingId: number): Promise<TrackedTrainTicket[] | null> {
  const url = `${baseUrl()}/Train/${trackingId}/tickets`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401 || response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TrackedTrainTicket[]>;
}

/** Per-ticket Delay Repay estimate
 * (`GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`). Same
 * cookie-forwarding and null-on-401/404 shape as
 * `getTicketsForTrackedTrain` above -- called only from within the "you
 * own this pin" branch `TicketPanel` has already established (see that
 * component), so a `null` here in practice means the specific ticket id
 * didn't resolve under this tracking id, a narrower condition than the
 * top-level list's 401/404 split -- `TicketPanel` treats it as "no
 * estimate to show for this ticket" rather than failing the whole page. */
export async function getDelayRepayEstimate(
  trackingId: number,
  ticketId: number,
): Promise<DelayRepayEstimateResponse | null> {
  const url = `${baseUrl()}/Train/${trackingId}/tickets/${ticketId}/delay-repay`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401 || response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<DelayRepayEstimateResponse>;
}

/** `GET /Train/tickets/mine`. Returns `null` on `401` (not logged in) --
 * deliberately not `ApiNotFoundError`, matching `getTicketsForTrackedTrain`'s
 * precedent of treating "no session" as an expected, first-class outcome.
 * Unlike that function, there is no second, distinct 404-shaped outcome to
 * also collapse into `null` here -- no id in this route's path to be
 * wrong about, so a 401 from this one call is the complete signal (same
 * reasoning as `getTrackedTrainById`'s sibling list route, `getMyTrackedTrains`).
 * Called from the merged `app/track/mine/page.tsx` (Part B of the
 * upload-first ticket-tracking plan -- `/track/tickets` used to call this
 * directly, but now just redirects there), which does NOT need a separate
 * `getSession()` call the way `TicketPanel` does. */
export async function getMyTickets(): Promise<TicketListItem[] | null> {
  const url = `${baseUrl()}/Train/tickets/mine`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TicketListItem[]>;
}

/** `GET /public/incidents/{incidentId}`. Never *requires* a session, but it
 * does forward the incoming request's cookies, exactly like `getAllLines()`
 * and for the same reason: the response's `currentlyAffectsLines` reads
 * `line_status`, which holds private custom-line rows, so the backend gates
 * those rows on who is asking (`routes::incidents::get_incident`). A Server
 * Component's own `fetch` carries none of the browser's cookies (see
 * `getPreferences`'s comment for the full explanation), so without this the
 * page would always call the backend anonymously and a logged-in owner
 * would lose their OWN custom line from "Currently affects" — the
 * functional half of the same bug the backend gate fixes.
 *
 * Still throws `ApiNotFoundError` on a 404 (via `errorForResponse`, same as
 * every other `fetchJson` caller) — `app/incidents/[id]/page.tsx` catches it
 * and calls `notFound()`, identical to `/lines/[id]`'s existing pattern. */
export async function getIncident(incidentId: string): Promise<IncidentDetail> {
  return fetchJson<IncidentDetail>(`${baseUrl()}/public/incidents/${encodeURIComponent(incidentId)}`, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

/** `GET /public/groups` -- the current user's own groups. `null` on a
 * `401`, matching `getMyTrackedTrains()`'s own "no id in the path, no
 * second party to disambiguate" null-on-401 convention -- there is
 * nothing else this route's `401` could mean besides "not logged in."
 *
 * `init?.signal`, matching `getDataFreshness`'s own optional-`signal`
 * shape: `RootLayout` (`app/layout.tsx`) calls this once per render,
 * unawaited alongside the freshness fetch, to hydrate
 * `GroupSummariesProvider` (`lib/useGroupSummaries.tsx`) -- and, being
 * awaited before any HTML is emitted, needs the same bounded timeout
 * `getDataFreshness` already has so a black-holed backend can't hang first
 * paint on this fetch instead. */
export async function getMyGroups(init?: Pick<RequestInit, 'signal'>): Promise<GroupSummary[] | null> {
  const url = `${baseUrl()}/public/groups`;
  const response = await fetch(url, { cache: 'no-store', ...init, ...(await cookieForwardInit()) });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<GroupSummary[]>;
}

/** `GET /public/groups/{id}` -- has an id in its path, so (unlike
 * `getMyGroups`) a `401` here is a genuine, if narrow, session-lapse case
 * and is thrown via `fetchJson`, matching `getTrackedTrainById`'s own
 * convention rather than `getMyTrackedTrains`'s null-on-401 one. */
export async function getGroup(id: string): Promise<GroupDetail> {
  const url = `${baseUrl()}/public/groups/${encodeURIComponent(id)}`;
  return fetchJson<GroupDetail>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

export async function getGroupMembers(id: string): Promise<GroupMember[]> {
  const url = `${baseUrl()}/public/groups/${encodeURIComponent(id)}/members`;
  return fetchJson<GroupMember[]>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

export async function getGroupTrains(id: string): Promise<GroupTrain[]> {
  const url = `${baseUrl()}/public/groups/${encodeURIComponent(id)}/trains`;
  return fetchJson<GroupTrain[]>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

/** `GET /public/groups/shared-trains` -- every train OTHER members have
 * shared into any group the caller belongs to (never the caller's own
 * tracked trains, which `getMyTrackedTrains()` already returns), each
 * tagged with the group it came from and who shared it. Feeds
 * `/track/mine`, which renders these alongside the caller's own rows.
 *
 * `null` on a `401`, exactly like `getMyGroups`/`getMyTrackedTrains` above
 * and for the same reason: no id in the path, so a `401` can only ever
 * mean "not logged in". `/track/mine` still keys its whole logged-out
 * branch off `getMyTrackedTrains()` alone (see that page's own comment) --
 * this one's `null` is folded into `[]` there, since "anonymous" is
 * already answered by the time it's read. */
export async function getSharedGroupTrains(): Promise<SharedGroupTrain[] | null> {
  const url = `${baseUrl()}/public/groups/shared-trains`;
  const response = await fetch(url, { cache: 'no-store', ...(await cookieForwardInit()) });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<SharedGroupTrain[]>;
}

/** `GET /public/groups/{id}/lines/custom` -- the custom lines members have
 * shared into this group. Any current member may read it; a non-member
 * gets the group's usual `404`. Throws on a `401`, like `getGroupTrains`
 * and for the same reason (there is an id in the path). */
export async function getGroupCustomLines(id: string): Promise<GroupCustomLine[]> {
  const url = `${baseUrl()}/public/groups/${encodeURIComponent(id)}/lines/custom`;
  return fetchJson<GroupCustomLine[]>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

/** `GET /public/groups/shared-custom-lines` -- every custom line OTHER
 * members have shared into any group the caller belongs to (never the
 * caller's own, which already reach them through `/public/lines`), each
 * tagged with the group it came from and who shared it. Feeds the home
 * page's "Lines shared with you" section.
 *
 * `null` on a `401`, exactly like `getSharedGroupTrains`/`getMyGroups`
 * above and for the same reason: no id in the path, so a `401` can only
 * ever mean "not logged in". */
export async function getSharedGroupCustomLines(): Promise<SharedGroupCustomLine[] | null> {
  const url = `${baseUrl()}/public/groups/shared-custom-lines`;
  const response = await fetch(url, { cache: 'no-store', ...(await cookieForwardInit()) });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<SharedGroupCustomLine[]>;
}

/** `GET /public/groups/{id}/journeys` -- the journeys shared into this
 * group. Any current member may read it; a non-member gets the group's
 * usual `404`. Throws on a `401`, like `getGroupTrains`/`getGroupCustomLines`
 * and for the same reason (there is an id in the path). */
export async function getGroupJourneys(id: string): Promise<GroupJourney[]> {
  const url = `${baseUrl()}/public/groups/${encodeURIComponent(id)}/journeys`;
  return fetchJson<GroupJourney[]>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

/** `GET /public/groups/shared-journeys` -- every journey OTHER members
 * have shared into any group the caller belongs to (never the caller's
 * own). Not called by any page yet -- see this feature's plan, Judgment
 * Call 4 -- built now for parity with `getSharedGroupTrains`/
 * `getSharedGroupCustomLines`. `null` on a `401`, same reasoning as those
 * two: no id in the path, so a `401` can only ever mean "not logged in". */
export async function getSharedGroupJourneys(): Promise<SharedGroupJourney[] | null> {
  const url = `${baseUrl()}/public/groups/shared-journeys`;
  const response = await fetch(url, { cache: 'no-store', ...(await cookieForwardInit()) });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<SharedGroupJourney[]>;
}

/** `GET /public/groups/join/{token}` -- unauthenticated on the backend
 * (see `routes::groups::get_join_preview`'s own doc comment), so this
 * needs no cookie forwarding either; a not-yet-logged-in visitor can see
 * the join preview before being sent through login. */
export async function getGroupJoinPreview(token: string): Promise<GroupJoinPreview> {
  const url = `${baseUrl()}/public/groups/join/${encodeURIComponent(token)}`;
  return fetchJson<GroupJoinPreview>(url, { cache: 'no-store' });
}
