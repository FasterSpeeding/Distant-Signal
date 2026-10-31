import { isTiplocCode, normalizeLocationCode, parseGroupCode } from './stationLabel';
import { operatorList } from './plannerOperators';
import {
  MAX_AVOIDED,
  MAX_CHANGES_LIMIT,
  MAX_VIAS,
  operatorFilterParam,
  setListParams,
  type TripPlanQuery,
} from './tripPlan';

/** The `/plan` page's own query string: the last search, so the address bar
 * can be shared or bookmarked and reopens the same form. It uses the same
 * parameter names as `GET /Trips/plan` (`origin`, `destination`, `date`,
 * `departAfter` or `arriveBy`, `results`, `waypoints`, `via`, `avoid`, `avoidStop`,
 * `avoidChange`, `maxChanges`, and `operator` or `avoidOperator`), but times stay `HH:MM`, as the form shows
 * them. `origin` alone is what a station page's "Plan a journey from here"
 * link sends. */
export function planPageSearch(query: TripPlanQuery): string {
  const params = new URLSearchParams();
  if (query.originCrs.trim()) params.set('origin', normalizeLocationCode(query.originCrs));
  if (query.destinationCrs.trim()) params.set('destination', normalizeLocationCode(query.destinationCrs));
  params.set('date', query.date);
  // One time, never both: the API rejects the pair.
  if (query.arriveBy) params.set('arriveBy', query.arriveBy);
  else if (query.departAfter) params.set('departAfter', query.departAfter);
  if (query.results !== 'fastest') params.set('results', query.results);
  setListParams(params, query);
  if (query.maxChanges !== undefined) params.set('maxChanges', String(query.maxChanges));
  const operatorFilter = operatorFilterParam(query);
  if (operatorFilter) params.set(...operatorFilter);
  return params.toString();
}

/** What a `/plan` URL restores into the form. Every field is optional:
 * anything absent or malformed is dropped (the form keeps its own default),
 * never passed on to the API to be rejected there. */
export type PlanFormInitial = Partial<TripPlanQuery>;

type RawSearchParams = Record<string, string | string[] | undefined>;

const CRS = /^[A-Za-z]{3}$/;
const TIPLOC_CODE = /^tiploc:[A-Za-z0-9]{1,7}$/i;
/** A NaPTAN stop as From or To (transit trip planning §5.2): a tram
 * station's access-node code (`9400ZZMAPIC`) or a stop area
 * (`940GZZCRWMB`, a London tram or other TfL stop). */
const NAPTAN_CODE = /^940[0G][A-Z0-9]{4,16}$/i;
const DATE = /^\d{4}-\d{2}-\d{2}$/;
const TIME = /^([01]\d|2[0-3]):[0-5]\d$/;

function first(value: string | string[] | undefined): string | undefined {
  return Array.isArray(value) ? value[0] : value;
}

/** A station's CRS, or (`allowStops`) a bus stop's or ferry terminal's
 * `tiploc:` code, or (`allowGroups`) a station group's `group:` code, or
 * (`allowNaptan`, From and To only) a NaPTAN stop code, normalized; `null`
 * for anything else. */
function locationCode(
  raw: string | undefined,
  allowStops: boolean,
  allowGroups = false,
  allowNaptan = false,
): string | null {
  const code = raw?.trim() ?? '';
  if (CRS.test(code)) return code.toUpperCase();
  if (allowNaptan && NAPTAN_CODE.test(code)) return code.toUpperCase();
  if (allowStops && TIPLOC_CODE.test(code) && isTiplocCode(code)) return normalizeLocationCode(code);
  if (allowGroups) return parseGroupCode(code);
  return null;
}

export function codeList(
  raw: string | undefined,
  { allowStops, allowGroups = false, max }: { allowStops: boolean; allowGroups?: boolean; max: number },
): string[] {
  const codes: string[] = [];
  for (const part of (raw ?? '').split(',')) {
    const code = locationCode(part, allowStops, allowGroups);
    if (code && !codes.includes(code)) codes.push(code);
  }
  return codes.slice(0, max);
}

/** Vias keep their order and may repeat, just not twice in a row; a
 * station's CRS or a bus stop's or ferry terminal's `tiploc:` code, as the
 * avoid lists, or a station group (`group:LON`). At most `max`. Also the
 * `/trains` boards' `calls_at` and `via` (`lib/trainStopFilters.ts`). */
export function orderedCodeList(raw: string | undefined, max = MAX_VIAS): string[] {
  const vias: string[] = [];
  for (const part of (raw ?? '').split(',')) {
    const code = locationCode(part, true, true);
    if (code && vias[vias.length - 1] !== code) vias.push(code);
  }
  return vias.slice(0, max);
}

/** Reads a `/plan` URL back into the form's starting values. */
export function parsePlanSearchParams(raw: RawSearchParams): PlanFormInitial {
  const initial: PlanFormInitial = {};
  const origin = locationCode(first(raw.origin), true, false, true);
  if (origin) initial.originCrs = origin;
  const destination = locationCode(first(raw.destination), true, false, true);
  if (destination) initial.destinationCrs = destination;
  const date = first(raw.date);
  if (date && DATE.test(date)) initial.date = date;
  // `arriveBy` wins when a hand-edited URL has both: the form only ever
  // writes one.
  const arriveBy = first(raw.arriveBy);
  const departAfter = first(raw.departAfter);
  if (arriveBy && TIME.test(arriveBy)) initial.arriveBy = arriveBy;
  else if (departAfter && TIME.test(departAfter)) initial.departAfter = departAfter;
  const results = first(raw.results);
  if (results === 'fastest' || results === 'options') initial.results = results;
  const waypoints = codeList(first(raw.waypoints), { allowStops: true, allowGroups: true, max: 20 });
  if (waypoints.length > 0) initial.waypointCrs = waypoints;
  const via = orderedCodeList(first(raw.via));
  if (via.length > 0) initial.viaCrs = via;
  const avoid = codeList(first(raw.avoid), { allowStops: true, max: MAX_AVOIDED });
  if (avoid.length > 0) initial.avoidCrs = avoid;
  const avoidStop = codeList(first(raw.avoidStop), { allowStops: true, max: MAX_AVOIDED });
  if (avoidStop.length > 0) initial.avoidStopCrs = avoidStop;
  const avoidChange = codeList(first(raw.avoidChange), { allowStops: true, max: MAX_AVOIDED });
  if (avoidChange.length > 0) initial.avoidChangeCrs = avoidChange;
  const maxChanges = first(raw.maxChanges);
  if (maxChanges && /^\d{1,2}$/.test(maxChanges) && Number(maxChanges) <= MAX_CHANGES_LIMIT) {
    initial.maxChanges = Number(maxChanges);
  }
  // One operator filter: `operator` (only these) wins over a hand-added
  // `avoidOperator`, which the API would reject alongside it.
  const only = operatorList(first(raw.operator));
  const avoidOperators = operatorList(first(raw.avoidOperator));
  if (only.length > 0) {
    initial.operatorMode = 'only';
    initial.operators = only;
  } else if (avoidOperators.length > 0) {
    initial.operatorMode = 'avoid';
    initial.operators = avoidOperators;
  }
  return initial;
}
