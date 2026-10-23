import { rankStationResults } from './stationSearch';
import { tflStopPlaceName } from './stationLabel';
import { fetchTflStops, searchPlannerLocations, tflStopDetail } from './suggestions';
import type { Suggestion } from './types';

/** The journey planner's From and To search (DS-135): the stations, bus
 * stops and ferry terminals of `searchPlannerLocations`, then TfL's Tube,
 * DLR and tram stops (`GET /public/tfl/stops`), merged by match quality as
 * `/stations` merges them. A TfL stop's `code` is its stop id
 * (`940GZZLUUXB`), which `GET /Trips/plan` takes as an origin or
 * destination: it plans from or to a nearby National Rail station and adds
 * a Tube or walk leg. Only From and To offer stops; "Call at", "Pass
 * through" and the avoid lists stay stations. A failed stop lookup is no
 * extra rows. */
export async function searchPlannerEnds(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  if (!q.trim()) return [];
  const [places, stops] = await Promise.all([
    searchPlannerLocations(q, signal),
    searchTflStopEnds(q, signal).catch((err: unknown) => {
      if (err instanceof DOMException && err.name === 'AbortError') throw err;
      return [];
    }),
  ]);
  return rankStationResults(q, [...places, ...stops]);
}

/** Every TfL mode the planner takes a stop of. */
const TFL_MODES = ['tube', 'dlr', 'tram'] as const;

async function searchTflStopEnds(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  const stops = await fetchTflStops(q, TFL_MODES, signal);
  return stops.map((stop) => ({
    code: stop.id,
    name: tflStopPlaceName(stop),
    detail: tflStopDetail(stop),
  }));
}
