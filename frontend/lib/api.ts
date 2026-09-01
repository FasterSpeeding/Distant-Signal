import { cookies } from 'next/headers';
import type {
  LineStatusReport,
  LineStatusHistoryEntry,
  LineDailyStats,
  Preferences,
  LineSummary,
  CustomLineDetail,
  LineDefinitionSummary,
  DataFreshness,
  HistoryRetention,
  Suggestion,
  SessionInfo,
  TrackedTrainState,
  TrackedTrainListItem,
  TrackedTrainTicket,
  DelayRepayEstimateResponse,
  TicketListItem,
  IncidentDetail,
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
  return new Error(message);
}

async function fetchJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<T>;
}

/** Builds the `fetch`/`fetchJson` `init` fragment that forwards the
 * incoming request's session cookie to the backend -- a Server
 * Component's own `fetch` never inherits it automatically. Returns `{}`
 * (no `Cookie` header at all) when the visitor has no cookies, matching
 * every existing cookie-forwarding call site's own conditional shape. */
async function cookieForwardInit(): Promise<RequestInit> {
  const cookieHeader = (await cookies()).toString();
  return cookieHeader ? { headers: { Cookie: cookieHeader } } : {};
}

export async function getLineStatusForMode(mode: string): Promise<LineStatusReport[]> {
  const url = `${baseUrl()}/Line/Mode/${mode}/Status`;
  return fetchJson<LineStatusReport[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

export async function getLineStatus(ids: string[], detail: boolean): Promise<LineStatusReport[]> {
  const idsParam = ids.join(',');
  const query = detail ? '?detail=true' : '';
  const url = `${baseUrl()}/Line/${idsParam}/Status${query}`;
  return fetchJson<LineStatusReport[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

export async function getStopPointDisruption(crs: string): Promise<LineStatusReport[]> {
  return fetchJson<LineStatusReport[]>(`${baseUrl()}/StopPoint/${crs}/Disruption`, {
    cache: 'no-store',
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
  const url = `${baseUrl()}/Line/${id}/Status/${from}/to/${to}`;
  return fetchJson<LineStatusHistoryEntry[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

/** `GET /Line/{id}/Stats/{from}/to/{to}` -- the new daily rollup route.
 * `from`/`to` are `YYYY-MM-DD` calendar days (the route's own path segments
 * are `NaiveDate`, not RFC3339 instants -- see the backend plan's Task 4).
 * Same public, no-store, no-cookie-forwarding shape as
 * `getLineStatusHistory`. */
export async function getLineDailyStats(
  id: string,
  from: string,
  to: string,
): Promise<LineDailyStats[]> {
  return fetchJson<LineDailyStats[]>(
    `${baseUrl()}/Line/${id}/Stats/${from}/to/${to}`,
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
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401) {
    return { pinnedLines: [], pinnedStations: [] };
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
 * `fetchJson` instead of needing its own 401-tolerant branch. */
export async function getSession(): Promise<SessionInfo> {
  const cookieHeader = (await cookies()).toString();
  return fetchJson<SessionInfo>(`${baseUrl()}/public/auth/session`, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
}

export async function getAllLines(): Promise<LineSummary[]> {
  const url = `${baseUrl()}/public/lines`;
  return fetchJson<LineSummary[]>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
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
  const url = `${baseUrl()}/public/lines/${id}`;
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
  const url = `${baseUrl()}/public/lines/${id}/definition`;
  return fetchJson<LineDefinitionSummary>(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
}

export async function getDataFreshness(): Promise<DataFreshness> {
  return fetchJson<DataFreshness>(`${baseUrl()}/public/freshness`, {
    cache: 'no-store',
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

export async function getTrackedTrainByUidAndDate(uid: string, date: string): Promise<TrackedTrainState> {
  const url = `${baseUrl()}/Train/by-uid/${encodeURIComponent(uid)}/${encodeURIComponent(date)}`;
  const response = await fetch(url, {
    cache: 'no-store',
    ...(await cookieForwardInit()),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<TrackedTrainState>;
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
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TrackedTrainListItem[]>;
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
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
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
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
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
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TicketListItem[]>;
}

/** `GET /public/incidents/{incidentId}`. Public, unauthenticated read — no
 * cookie forwarding needed, same plain `fetchJson` pattern as
 * `getLineDefinition`/`getCustomLine`. Throws `ApiNotFoundError` on a 404
 * (via `errorForResponse`, same as every other `fetchJson` caller) —
 * `app/incidents/[id]/page.tsx` catches it and calls `notFound()`,
 * identical to `/lines/[id]`'s existing pattern. */
export async function getIncident(incidentId: string): Promise<IncidentDetail> {
  return fetchJson<IncidentDetail>(`${baseUrl()}/public/incidents/${encodeURIComponent(incidentId)}`, {
    cache: 'no-store',
  });
}
