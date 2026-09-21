import type { NearbyStation, Suggestion } from './types';

/** Client-side fetch through the same-origin `/api/*` proxy
 * (`app/api/[...path]/route.ts`) — Client Components can't read the
 * server-only `API_BASE_URL`, so this can't go through `lib/api.ts`'s
 * `baseUrl()` like the server-rendered fetches do. Empty/whitespace `q`
 * short-circuits without a network call, mirroring the backend's own
 * empty-query short-circuit. */
export async function searchStations(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  if (!q.trim()) return [];
  const response = await fetch(`/api/stations?q=${encodeURIComponent(q)}`, { signal });
  if (!response.ok) return [];
  return response.json() as Promise<Suggestion[]>;
}

export async function searchTocs(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  if (!q.trim()) return [];
  const response = await fetch(`/api/tocs?q=${encodeURIComponent(q)}`, { signal });
  if (!response.ok) return [];
  return response.json() as Promise<Suggestion[]>;
}

/** Client-side fetch through the same-origin `/api/*` proxy, same shape as
 * `searchStations` above (see its doc comment for why this can't go
 * through `lib/api.ts`'s server-only `baseUrl()`). Unlike `searchStations`,
 * there is no "empty input" short-circuit -- `lat`/`lon` only ever reach
 * this function already resolved from a real
 * `navigator.geolocation.getCurrentPosition()` fix, so every call here is a
 * genuine lookup.
 *
 * Unlike `searchStations`/`searchTocs`, a failed request (network error or
 * non-2xx) THROWS rather than resolving to `[]`: those two back a passive,
 * ever-refetching type-ahead dropdown where a transient blip and "no
 * matches" are both fine to just show as empty, but this backs a single
 * explicit "Use my location" button click that already has its own
 * `.catch()`-driven error state (`StationSearchForm.tsx`'s `handleNearMe`)
 * -- collapsing a real backend/validation failure into an empty result
 * would render the calm "nothing found near you" copy for what is actually
 * an error, giving the user no reason to retry. */
export async function searchNearbyStations(
  lat: number,
  lon: number,
  signal?: AbortSignal,
): Promise<NearbyStation[]> {
  const params = new URLSearchParams({ lat: String(lat), lon: String(lon) });
  const response = await fetch(`/api/stations/nearby?${params.toString()}`, { signal });
  if (!response.ok) {
    throw new Error(`nearby station lookup failed: ${response.status}`);
  }
  return response.json() as Promise<NearbyStation[]>;
}
