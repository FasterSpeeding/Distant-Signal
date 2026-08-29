import { cookies } from 'next/headers';
import type {
  LineStatusReport,
  LineStatusHistoryEntry,
  Preferences,
  LineSummary,
  CustomLineDetail,
  LineDefinitionSummary,
  DataFreshness,
  Suggestion,
} from './types';

/** Thrown when the API responds 404 — lets callers distinguish "genuinely
 * not found" from other failures (network errors, 500s, etc.). */
export class ApiNotFoundError extends Error {}

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
  return response.status === 404 ? new ApiNotFoundError(message) : new Error(message);
}

async function fetchJson<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<T>;
}

export async function getLineStatusForMode(mode: string): Promise<LineStatusReport[]> {
  return fetchJson<LineStatusReport[]>(`${baseUrl()}/Line/Mode/${mode}/Status`, {
    cache: 'no-store',
  });
}

export async function getLineStatus(ids: string[], detail: boolean): Promise<LineStatusReport[]> {
  const idsParam = ids.join(',');
  const query = detail ? '?detail=true' : '';
  return fetchJson<LineStatusReport[]>(`${baseUrl()}/Line/${idsParam}/Status${query}`, {
    cache: 'no-store',
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
  return fetchJson<LineStatusHistoryEntry[]>(
    `${baseUrl()}/Line/${id}/Status/${from}/to/${to}`,
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

export async function getAllLines(): Promise<LineSummary[]> {
  return fetchJson<LineSummary[]>(`${baseUrl()}/public/lines`, { cache: 'no-store' });
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

export async function getCustomLine(id: string): Promise<CustomLineDetail> {
  return fetchJson<CustomLineDetail>(`${baseUrl()}/public/lines/${id}`, { cache: 'no-store' });
}

export async function getLineDefinition(id: string): Promise<LineDefinitionSummary> {
  return fetchJson<LineDefinitionSummary>(`${baseUrl()}/public/lines/${id}/definition`, { cache: 'no-store' });
}

export async function getDataFreshness(): Promise<DataFreshness> {
  return fetchJson<DataFreshness>(`${baseUrl()}/public/freshness`, {
    cache: 'no-store',
  });
}
