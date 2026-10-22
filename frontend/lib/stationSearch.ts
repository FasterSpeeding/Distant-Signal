import { searchStations, tflStopDetail } from './suggestions';
import { TRANSIT_ID, TRANSIT_STOP_ID } from './transitIds';
import type { Suggestion, TflStop, TransitNetwork, TransitStop } from './types';

/** Rows the merged station search shows at most, as `/stations` alone. */
export const STATION_SEARCH_LIMIT = 20;

/** `/stations`' search (DS-134): National Rail stations, then any TfL Tube,
 * DLR and tram stops (`GET /public/tfl/stops`, empty until poller-tfl's
 * stops job has run) and any served transit network's stops (`GET
 * /public/transit/{network}/stops`, none while `api.transitNetworks` is
 * empty), so a stop with no National Rail station, such as Uxbridge, still
 * turns up. Each extra row carries its `place`, so the dropdown names its
 * mode and the form knows where it leads. Merged by match quality (exact
 * name, prefix, word prefix, substring), National Rail first within a
 * tier. Only `/stations` uses this: every other picker hands its code to a
 * station board or the planner, which a Tube-only stop has none of. A
 * failed TfL or transit lookup is no extra rows, never a failed search. */
export async function searchStationsAndStops(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  if (!q.trim()) return [];
  const [rail, tfl, transit] = await Promise.all([
    searchStations(q, signal),
    searchTflStopPlaces(q, signal).catch(rethrowAbort),
    searchTransitStopPlaces(q, signal).catch(rethrowAbort),
  ]);
  return rankStationResults(q, [...rail, ...tfl, ...transit]).slice(0, STATION_SEARCH_LIMIT);
}

function rethrowAbort(err: unknown): Suggestion[] {
  if (err instanceof DOMException && err.name === 'AbortError') throw err;
  return [];
}

async function searchTflStopPlaces(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  const response = await fetch(`/api/tfl/stops?${new URLSearchParams({ q }).toString()}`, signal ? { signal } : {});
  if (!response.ok) return [];
  const body: unknown = await response.json();
  if (!Array.isArray(body)) return [];
  return body.filter(isTflStop).map((stop) => ({
    code: `tfl:${stop.id}`,
    name: stop.name,
    detail: tflStopDetail(stop),
    place: { kind: 'tfl', stop },
  }));
}

async function searchTransitStopPlaces(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  const needle = searchKey(q);
  if (!needle) return [];
  const init = signal ? { signal } : {};
  const response = await fetch('/api/transit/networks', init);
  if (!response.ok) return [];
  const body = (await response.json()) as { networks?: unknown };
  const networks = arrayOf(body.networks)
    .filter((n): n is TransitNetwork => hasStrings(n, 'id', 'name'))
    .filter((n) => TRANSIT_ID.test(n.id));
  const perNetwork = await Promise.all(
    networks.map(async (network): Promise<Suggestion[]> => {
      const stopsResponse = await fetch(`/api/transit/${encodeURIComponent(network.id)}/stops`, init);
      if (!stopsResponse.ok) return [];
      const stopsBody = (await stopsResponse.json()) as { stops?: unknown };
      return arrayOf(stopsBody.stops)
        .filter((stop): stop is TransitStop => hasStrings(stop, 'id', 'name'))
        .filter((stop) => TRANSIT_STOP_ID.test(stop.id) && searchKey(stop.name).includes(needle))
        .map((stop) => ({
          code: `transit:${network.id}:${stop.id}`,
          name: stop.name,
          detail: transitDetail(network),
          place: { kind: 'transit', network: network.id, stopId: stop.id },
        }));
    }),
  );
  return perNetwork.flat();
}

/** `Metrolink tram stop`; the mode is display text from the network row. */
function transitDetail(network: TransitNetwork): string {
  const mode = typeof network.mode === 'string' && network.mode ? ` ${network.mode}` : '';
  return `${network.name}${mode} stop`;
}

function arrayOf(value: unknown): unknown[] {
  return Array.isArray(value) ? (value as unknown[]) : [];
}

function hasStrings(value: unknown, ...keys: string[]): boolean {
  if (typeof value !== 'object' || value === null) return false;
  const record = value as Record<string, unknown>;
  return keys.every((key) => typeof record[key] === 'string');
}

function isTflStop(value: unknown): value is TflStop {
  if (typeof value !== 'object' || value === null) return false;
  const stop = value as Partial<TflStop>;
  return (
    typeof stop.id === 'string' &&
    typeof stop.name === 'string' &&
    Array.isArray(stop.modes) &&
    Array.isArray(stop.lines)
  );
}

/** Lower case, `&` as `and`, punctuation dropped, spaces collapsed: the
 * shape `common::tfl_stops::search_key` matches on, so `kings cross`
 * finds `King's Cross`. */
export function searchKey(text: string): string {
  return text
    .toLowerCase()
    .replace(/&/g, ' and ')
    .replace(/[^a-z0-9 ]+/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** 0 exact (name or code), 1 name prefix, 2 a word starting with the
 * query, 3 anything else the server matched. */
function matchTier(suggestion: Suggestion, needle: string): number {
  const name = searchKey(suggestion.name);
  if (name === needle || (!suggestion.place && suggestion.code.toLowerCase() === needle)) return 0;
  if (name.startsWith(needle)) return 1;
  if (name.split(' ').some((word) => word.startsWith(needle))) return 2;
  return 3;
}

/** Stable: within a tier, the order given (National Rail first). */
export function rankStationResults(q: string, results: Suggestion[]): Suggestion[] {
  const needle = searchKey(q);
  return results
    .map((suggestion, index) => ({ suggestion, index, tier: matchTier(suggestion, needle) }))
    .sort((a, b) => a.tier - b.tier || a.index - b.index)
    .map(({ suggestion }) => suggestion);
}
