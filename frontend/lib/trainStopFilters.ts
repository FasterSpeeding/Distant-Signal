import { codeStationLabel, normalizeLocationCode } from './stationLabel';
import { MAX_AVOIDED, MAX_VIAS } from './tripPlan';
import { codeList, orderedCodeList } from './tripPlanUrl';

/** The `/trains` boards' advanced stop filters (`GET /public/trains/search`
 * and `/public/trains/arrivals`: `calls_at`, `via`, `avoid`, `avoid_stop`;
 * docs/api-changelog.md, 2026-10-30): the planner's "Call at", "Pass
 * through", "Avoid completely" and "Don't stop at", applied to the part of
 * the train a passenger rides. Each list holds station codes, a bus stop's
 * or ferry terminal's `tiploc:` code, or (Call at and Pass through) a
 * station group, `group:LON`. */
export interface TrainStopFilters {
  /** A passenger call at each, in order. */
  callsAt: string[];
  /** A call at or a pass of each, in order. */
  via: string[];
  /** Neither calls at nor runs through these. */
  avoid: string[];
  /** Runs through these, if at all, without stopping. */
  avoidStop: string[];
}

export const NO_STOP_FILTERS: TrainStopFilters = { callsAt: [], via: [], avoid: [], avoidStop: [] };

/** The API's caps (`crates/api/src/routes/trains/stop_filters.rs`): Call
 * at and Pass through take the planner's via cap, the avoid lists its
 * avoid cap. */
export const MAX_CALLS_AT = MAX_VIAS;
export { MAX_AVOIDED, MAX_VIAS };

/** Each list's URL and API parameter: the two share the names. */
export const STOP_FILTER_PARAMS = [
  ['calls_at', 'callsAt'],
  ['via', 'via'],
  ['avoid', 'avoid'],
  ['avoid_stop', 'avoidStop'],
] as const satisfies readonly (readonly [string, keyof TrainStopFilters])[];

/** The filters a `/trains` URL holds, read tolerantly as the `/plan` page
 * reads its lists: anything malformed dropped, each list cut to its cap. */
export function parseStopFilterParams(params: URLSearchParams | null): TrainStopFilters {
  if (!params) return NO_STOP_FILTERS;
  return {
    callsAt: orderedCodeList(params.get('calls_at') ?? undefined, MAX_CALLS_AT),
    via: orderedCodeList(params.get('via') ?? undefined, MAX_VIAS),
    avoid: codeList(params.get('avoid') ?? undefined, { allowStops: true, max: MAX_AVOIDED }),
    avoidStop: codeList(params.get('avoid_stop') ?? undefined, { allowStops: true, max: MAX_AVOIDED }),
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
 * an avoided searched station, a Call at or Pass through that is one, the
 * same station twice in a row, and a list that contradicts another. */
export function stopFilterErrors(
  filters: TrainStopFilters,
  {
    stations,
    names,
  }: {
    /** The searched stations with what the visitor calls them: `[code,
     * 'where you board']`. */
    stations: [string, string][];
    names: Map<string, string>;
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
  const ordered = [
    ['callsAt', notStopped, `"Avoid completely" or "Don't stop at": a train can't both stop there and not`],
    ['via', avoided, `"Avoid completely": a train can't both pass through it and avoid it`],
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
  return errors;
}
