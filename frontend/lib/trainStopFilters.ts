import { codeStationLabel, normalizeLocationCode } from './stationLabel';
import { MAX_AVOIDED, MAX_VIAS } from './tripPlan';
import { codeList, orderedCodeList } from './tripPlanUrl';

/** The `/trains` boards' advanced stop filters (`GET /public/trains/search`
 * and `/public/trains/arrivals`: `calls_at`, `via`, `avoid`, `avoid_stop`;
 * docs/api-changelog.md, 2026-10-30): the planner's "Call at", "Pass
 * through", "Avoid completely" and "Don't stop at", applied to the part of
 * the train a passenger rides. Each list holds station codes, a bus stop's
 * or ferry terminal's `tiploc:` code, or (Call at and Pass through) a
 * station group, `group:LON`.
 *
 * Four more, for finding one particular train and never in the planner
 * (DS-213). About the route before the station (before the first of
 * two): "Starts at" (`origin`, one station code: the train's first
 * station, exactly), and "Previously calls at" and "Previously stops at"
 * (`previously_calls_at`, `previously_stops_at`): where the train has
 * already been, its origin counted or not. And "Terminates at"
 * (`terminates_at`, one station code: its last station, exactly). "Starts
 * at" moved here from the station board's main form, under the same
 * `origin` parameter, so an old link keeps its meaning (and now works
 * between two stations too). */
export interface TrainStopFilters {
  /** A passenger call at each, in order. */
  callsAt: string[];
  /** A call at or a pass of each, in order. */
  via: string[];
  /** Neither calls at nor runs through these. */
  avoid: string[];
  /** Runs through these, if at all, without stopping. */
  avoidStop: string[];
  /** Before the station: a passenger call at each, in order, where the
   * train starts included. */
  previouslyCallsAt: string[];
  /** Before the station: a passenger call at each, in order, after the
   * train started (its origin not counted). */
  previouslyStopsAt: string[];
  /** Starts here: none or one station code, as typed (`StationField`),
   * so it can be half-entered; `stopFilterErrors` refuses anything but a
   * 3-letter code. */
  startsAt: string[];
  /** Ends here: as `startsAt`. */
  terminatesAt: string[];
}

export const NO_STOP_FILTERS: TrainStopFilters = {
  callsAt: [],
  via: [],
  avoid: [],
  avoidStop: [],
  previouslyCallsAt: [],
  previouslyStopsAt: [],
  startsAt: [],
  terminatesAt: [],
};

/** The API's caps (`crates/api/src/routes/trains/stop_filters.rs`): Call
 * at, Pass through and the two "Previously" lists take the planner's via
 * cap, the avoid lists its avoid cap. */
export const MAX_CALLS_AT = MAX_VIAS;
export { MAX_AVOIDED, MAX_VIAS };

/** Each list's URL and API parameter: the two share the names. */
export const STOP_FILTER_PARAMS = [
  ['calls_at', 'callsAt'],
  ['via', 'via'],
  ['avoid', 'avoid'],
  ['avoid_stop', 'avoidStop'],
  ['previously_calls_at', 'previouslyCallsAt'],
  ['previously_stops_at', 'previouslyStopsAt'],
  ['origin', 'startsAt'],
  ['terminates_at', 'terminatesAt'],
] as const satisfies readonly (readonly [string, keyof TrainStopFilters])[];

/** A station code, the only thing `origin` takes. */
const STATION_CODE = /^[A-Za-z]{3}$/;

/** The filters a `/trains` URL holds, read tolerantly as the `/plan` page
 * reads its lists: anything malformed dropped, each list cut to its cap. */
export function parseStopFilterParams(params: URLSearchParams | null): TrainStopFilters {
  if (!params) return NO_STOP_FILTERS;
  const ordered = (param: string) => orderedCodeList(params.get(param) ?? undefined, MAX_CALLS_AT);
  const station = (param: string) => {
    const code = params.get(param)?.trim() ?? '';
    return STATION_CODE.test(code) ? [code.toUpperCase()] : [];
  };
  return {
    callsAt: ordered('calls_at'),
    via: orderedCodeList(params.get('via') ?? undefined, MAX_VIAS),
    avoid: codeList(params.get('avoid') ?? undefined, { allowStops: true, max: MAX_AVOIDED }),
    avoidStop: codeList(params.get('avoid_stop') ?? undefined, { allowStops: true, max: MAX_AVOIDED }),
    previouslyCallsAt: ordered('previously_calls_at'),
    previouslyStopsAt: ordered('previously_stops_at'),
    startsAt: station('origin'),
    terminatesAt: station('terminates_at'),
  };
}

/** Every set list as `[param, "A,B"]`, normalized, for the API query and
 * the address bar alike. */
export function stopFilterEntries(filters: TrainStopFilters): [string, string][] {
  const entries: [string, string][] = [];
  for (const [param, field] of STOP_FILTER_PARAMS) {
    const codes = filters[field].map(normalizeLocationCode).filter((code) => code.length > 0);
    if (codes.length > 0) entries.push([param, codes.join(',')]);
  }
  return entries;
}

/** How many lists are set. */
export function countStopFilters(filters: TrainStopFilters): number {
  return STOP_FILTER_PARAMS.filter(([, field]) => filters[field].length > 0).length;
}

/** Each set list's parameter name: the usage-count steps. */
export function setStopFilterParams(filters: TrainStopFilters): (typeof STOP_FILTER_PARAMS)[number][0][] {
  return STOP_FILTER_PARAMS.filter(([, field]) => filters[field].length > 0).map(([param]) => param);
}

/** One line naming the set filters, for the collapsed section: "Call at
 * Swindon · Avoid DID". */
export function stopFiltersSummary(filters: TrainStopFilters, names = new Map<string, string>()): string {
  const list = (codes: string[]) => codes.map((code) => names.get(code) ?? code).join(', ');
  const parts: string[] = [];
  if (filters.startsAt.length) parts.push(`Starts at ${list(filters.startsAt)}`);
  if (filters.previouslyCallsAt.length) parts.push(`Previously calls at ${list(filters.previouslyCallsAt)}`);
  if (filters.previouslyStopsAt.length) parts.push(`Previously stops at ${list(filters.previouslyStopsAt)}`);
  if (filters.terminatesAt.length) parts.push(`Terminates at ${list(filters.terminatesAt)}`);
  if (filters.callsAt.length) parts.push(`Call at ${list(filters.callsAt)}`);
  if (filters.via.length) parts.push(`Pass through ${list(filters.via)}`);
  if (filters.avoid.length) parts.push(`Avoid ${list(filters.avoid)}`);
  if (filters.avoidStop.length) parts.push(`Don't stop at ${list(filters.avoidStop)}`);
  return parts.join(' · ');
}

export type StopFilterErrors = Partial<Record<keyof TrainStopFilters, string>>;

/** The client-side checks, mirroring the API's 400s
 * (`ParsedStopFilters::check_conflicts`) and the planner's own
 * (`advancedOptionErrors`), so the visitor sees them on the field:
 * an avoided searched station, a Call at, Pass through or "Previously"
 * entry that is one, the same station twice in a row, and a list that
 * contradicts another. The "Previously" lists look before the station and
 * the avoid lists after it, so the two only clash on the arrivals board
 * (`arrivals`), where every list looks before it; so does "Starts at"
 * (the API's `400` there too), and "Terminates at" only on the departures
 * board, where the lists look on to the end of the train. Each must be a
 * station code, and may be the station searched: a train starting where
 * it departs, or ending where it arrives, is the common case on those
 * boards, and elsewhere it is a circular working. */
export function stopFilterErrors(
  filters: TrainStopFilters,
  {
    stations,
    names,
    arrivals = false,
    between = false,
  }: {
    /** The searched stations with what the visitor calls them: `[code,
     * 'where you board']`. */
    stations: [string, string][];
    names: Map<string, string>;
    arrivals?: boolean;
    between?: boolean;
  },
): StopFilterErrors {
  const label = (code: string) => codeStationLabel(code, names.get(code));
  const roleOf = (code: string) => stations.find(([station]) => normalizeLocationCode(station) === code)?.[1];
  const errors: StopFilterErrors = {};

  for (const field of ['avoid', 'avoidStop'] as const) {
    const searched = filters[field].find((code) => roleOf(code));
    if (searched !== undefined) {
      errors[field] = `${label(searched)} is ${roleOf(searched)}, so it can't be avoided.`;
    }
  }
  const avoided = new Set(filters.avoid);
  const notStopped = new Set([...filters.avoid, ...filters.avoidStop]);
  const stopClash = `"Avoid completely" or "Don't stop at": a train can't both stop there and not`;
  const earlierBlocked = arrivals ? notStopped : new Set<string>();
  const ordered = [
    ['callsAt', notStopped, stopClash],
    ['via', avoided, `"Avoid completely": a train can't both pass through it and avoid it`],
    ['previouslyCallsAt', earlierBlocked, stopClash],
    ['previouslyStopsAt', earlierBlocked, stopClash],
  ] as const;
  for (const [field, blocked, clash] of ordered) {
    for (const [index, code] of filters[field].entries()) {
      const role = roleOf(code);
      if (role) {
        errors[field] = `${label(code)} is ${role}: list a station elsewhere on the route.`;
        break;
      }
      if (blocked.has(code)) {
        errors[field] = `${label(code)} is also in ${clash}.`;
        break;
      }
      if (index > 0 && filters[field][index - 1] === code) {
        errors[field] = `${label(code)} is listed twice in a row: list it once.`;
        break;
      }
    }
  }
  const ends = [
    ['startsAt', arrivals, 'start'],
    ['terminatesAt', !arrivals && !between, 'end'],
  ] as const;
  for (const [field, clashes, verb] of ends) {
    const code = filters[field][0]?.trim().toUpperCase();
    if (code === undefined) continue;
    if (!STATION_CODE.test(code)) {
      errors[field] = 'Must be a 3-letter station code';
    } else if (clashes && notStopped.has(code)) {
      errors[field] =
        `${label(code)} is also in "Avoid completely" or "Don't stop at": a train can't both ${verb} there and not.`;
    }
  }
  return errors;
}
