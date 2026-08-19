import type { Suggestion } from './types';

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
