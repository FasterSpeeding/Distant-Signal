import { matchingTransitStations, rankStationResults } from './stationSearch';
import { tflStopPlaceName } from './stationLabel';
import { fetchTflStops, searchPlannerLocations, tflStopDetail } from './suggestions';
import { transitNetworkName, transitStopPlaceName } from './transitStops';
import type { Suggestion } from './types';

/** The journey planner's From and To search (DS-135): the stations, bus
 * stops and ferry terminals of `searchPlannerLocations`, then TfL's Tube,
 * DLR and tram stops (`GET /public/tfl/stops`), merged by match quality as
 * `/stations` merges them. A TfL stop's `code` is its stop id
 * (`940GZZLUUXB`), which `GET /Trips/plan` takes as an origin or
 * destination: it plans from or to a nearby National Rail station and adds
 * a Tube or walk leg. Only From and To offer stops; "Call at", "Pass
 * through" and the avoid lists stay stations.
 *
 * After the TfL stops come the stations of every transit network the
 * planner routes over (`tripPlanning: true` on
 * `GET /public/transit/networks`), named `Piccadilly (Metrolink)`, whose
 * `code` is the station's id (`9400ZZMAPIC`): the plan rides the tram from
 * or to it. A failed stop lookup is no extra rows. */
export async function searchPlannerEnds(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  if (!q.trim()) return [];
  const [places, stops, transit] = await Promise.all([
    searchPlannerLocations(q, signal),
    searchTflStopEnds(q, signal).catch(noRowsUnlessAborted),
    searchTransitStationEnds(q, signal).catch(noRowsUnlessAborted),
  ]);
  return rankStationResults(q, [...places, ...stops, ...transit]);
}

function noRowsUnlessAborted(err: unknown): Suggestion[] {
  if (err instanceof DOMException && err.name === 'AbortError') throw err;
  return [];
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

async function searchTransitStationEnds(q: string, signal?: AbortSignal): Promise<Suggestion[]> {
  const matches = await matchingTransitStations(q, signal, { tripPlanningOnly: true });
  return matches.map(({ network, stop }) => ({
    code: stop.id,
    name: transitStopPlaceName(stop.name, transitNetworkName(network.id, network.name)),
    detail: `${network.name} tram stop`,
  }));
}
