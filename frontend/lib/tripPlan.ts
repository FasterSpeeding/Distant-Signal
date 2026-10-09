import { isGroupCode, normalizeLocationCode } from './stationLabel';
import type {
  TripPlanItinerary,
  TripPlanLeg,
  TripPlanNoResultReason,
  TripPlanResponse,
  TripPlanViaSatisfied,
  TripPlanWaypointSatisfied,
} from './types';
import { describeFailure } from './failure';
import { createLogger } from './logger';

const log = createLogger('lib/tripPlan');

/** A code a station-name lookup can resolve: not a station group
 * (`group:LON`) nor a choice of several (`KGX|EUS`). */
function isStationCode(code: string): boolean {
  return !isGroupCode(code) && !code.includes('|');
}

/** Every distinct, non-null CRS code a `GET /Trips/plan` response mentions
 * -- each segment's own origin/destination endpoints, AND every leg (train
 * or transfer) of every one of its itineraries, since a leg's own
 * origin/destination can genuinely differ from its segment's (boarding a
 * Birmingham->Glasgow service at Crewe, alighting at Preston -- see
 * `PlanTripFlow.tsx`'s own `handleTrackJourney` doc comment for the real
 * example this covers). `TripPlanSegment`/`TripPlanLeg` carry bare codes
 * only, unlike every OTHER station-bearing response in this app (which
 * already comes back with a `*Name` sibling field resolved server-side),
 * so `PlanTripFlow` uses this to know which codes it needs to resolve
 * itself, via `lib/suggestions.ts`'s `getStationNames`. Pure and
 * independently testable, matching `buildTripPlanQuery`'s own "small pure
 * helper" convention below. */
export function collectTripPlanStationCodes(response: TripPlanResponse): string[] {
  const codes = new Set<string>();
  for (const segment of response.segments) {
    codes.add(segment.originCrs);
    codes.add(segment.destinationCrs);
    for (const itinerary of segment.itineraries) {
      for (const leg of itinerary.legs) {
        if (leg.originCrs) codes.add(leg.originCrs);
        if (leg.destinationCrs) codes.add(leg.destinationCrs);
      }
    }
    for (const value of segment.noResultReason?.values ?? []) codes.add(value);
  }
  for (const via of response.via ?? []) codes.add(via);
  // The members a journey used for a group via or waypoint (2026-10-07).
  for (const journey of response.journeys ?? []) {
    for (const hit of [...(journey.viaSatisfiedBy ?? []), ...(journey.waypointSatisfiedBy ?? [])]) {
      if (hit.matchedCrs) codes.add(hit.matchedCrs);
    }
  }
  return Array.from(codes).filter(isStationCode);
}

export interface TripPlanQuery {
  originCrs: string;
  destinationCrs: string;
  /** Ordered, e.g. `['YRK', 'NCL']` -- entered order, never reordered. An
   * entry may be a station group, `group:LON` (a stop at ANY member). */
  waypointCrs: string[];
  date: string; // "YYYY-MM-DD"
  departAfter?: string | undefined; // "HH:MM"
  /** "HH:MM"; the backend rejects it together with `departAfter`. */
  arriveBy?: string | undefined;
  /** Never call at or pass through these stations. */
  avoidCrs?: string[] | undefined;
  /** Pass these, but never call at them. */
  avoidStopCrs?: string[] | undefined;
  /** Never board, alight or change at these (staying aboard is fine). */
  avoidChangeCrs?: string[] | undefined;
  /** Pass through these stations, in order, calling there or not: a CRS, a
   * bus stop's or ferry terminal's `tiploc:` code, or a station group
   * (`group:LON`, passed by ANY member). At most [`MAX_VIAS`]. */
  viaCrs?: string[] | undefined;
  /** 0 to [`MAX_CHANGES_LIMIT`]; absent means the API's default
   * ([`DEFAULT_MAX_CHANGES`]). */
  maxChanges?: number | undefined;
  results: 'fastest' | 'options';
}

/** `GET /Trips/plan`'s own limits (`crates/api/src/routes/trips.rs`:
 * `MAX_VIAS`, `MAX_AVOIDED`; `trip_planning_itinerary::MAX_CHANGES_LIMIT`
 * and `DEFAULT_MAX_CHANGES`). The form enforces them so a visitor meets
 * them as a disabled control, not a 400. */
export const MAX_VIAS = 3;
export const MAX_AVOIDED = 8;
export const MAX_CHANGES_LIMIT = 6;
export const DEFAULT_MAX_CHANGES = 2;

/** The advanced options the planner form groups behind its "Advanced"
 * disclosure. */
export type TripPlanAdvancedOptions = Pick<
  TripPlanQuery,
  'viaCrs' | 'avoidCrs' | 'avoidStopCrs' | 'avoidChangeCrs' | 'maxChanges'
>;

/** How many of the advanced options are set: each non-empty list counts
 * once, as does a `maxChanges` other than the default. */
export function countAdvancedOptions(options: TripPlanAdvancedOptions): number {
  const lists = [options.viaCrs, options.avoidCrs, options.avoidStopCrs, options.avoidChangeCrs];
  const setLists = lists.filter((list) => (list ?? []).length > 0).length;
  return setLists + (options.maxChanges !== undefined ? 1 : 0);
}

const LIST_PARAMS = [
  ['waypoints', 'waypointCrs'],
  ['via', 'viaCrs'],
  ['avoid', 'avoidCrs'],
  ['avoidStop', 'avoidStopCrs'],
  ['avoidChange', 'avoidChangeCrs'],
] as const;

/** Builds `GET /Trips/plan`'s query string from a [`TripPlanQuery`] --
 * pure and independently testable, matching this codebase's own
 * "small pure helper, tested separately from the fetch call" convention
 * (e.g. `lib/trackAgainPrefill.ts`'s own `trackAgainHref`). */
export function buildTripPlanQuery(query: TripPlanQuery): string {
  const params = new URLSearchParams({
    origin: normalizeLocationCode(query.originCrs),
    destination: normalizeLocationCode(query.destinationCrs),
    date: query.date,
    results: query.results,
  });
  setListParams(params, query);
  if (query.departAfter) {
    params.set('departAfter', `${query.departAfter}:00`);
  }
  if (query.arriveBy) {
    params.set('arriveBy', `${query.arriveBy}:00`);
  }
  if (query.maxChanges !== undefined) {
    params.set('maxChanges', String(query.maxChanges));
  }
  return params.toString();
}

/** Every list parameter (`waypoints`, `via`, the avoid lists): normalized,
 * empties dropped, comma-joined, and omitted when empty. Shared with the
 * `/plan` page URL (`lib/tripPlanUrl.ts`) so the two never disagree. */
export function setListParams(params: URLSearchParams, query: Partial<TripPlanQuery>): void {
  for (const [param, field] of LIST_PARAMS) {
    const codes = (query[field] ?? []).map(normalizeLocationCode).filter((c) => c.length > 0);
    if (codes.length > 0) params.set(param, codes.join(','));
  }
}

type TrainLeg = Extract<TripPlanLeg, { kind: 'train' }>;

/** Every train leg of the chosen itineraries, in order, for tracking -- a
 * transfer never becomes a journey leg. A segment whose itinerary
 * `continuesPreviousTrain` rides on in the same train through the
 * waypoint, so its first leg extends the previous one rather than
 * becoming a second leg for the same train. */
export function trainLegsForTracking(itineraries: TripPlanItinerary[]): TrainLeg[] {
  const legs: TrainLeg[] = [];
  for (const itinerary of itineraries) {
    itinerary.legs.forEach((leg, index) => {
      if (leg.kind !== 'train') return;
      const previous = legs[legs.length - 1];
      if (
        index === 0 &&
        itinerary.continuesPreviousTrain &&
        previous?.trainUid === leg.trainUid &&
        previous.serviceDate === leg.serviceDate
      ) {
        legs[legs.length - 1] = {
          ...previous,
          destinationCrs: leg.destinationCrs,
          scheduledArrival: leg.scheduledArrival,
          arrivalDayOffset: leg.arrivalDayOffset,
          bookedArrivalPlatform: leg.bookedArrivalPlatform,
        };
        return;
      }
      legs.push(leg);
    });
  }
  return legs;
}

/** What the trip planner shows for a 503 (docs/api-changelog.md,
 * 2026-10-06): Distant Signal is temporarily unavailable, not "no route". */
export const TRIP_PLAN_UNAVAILABLE_MESSAGE = 'Journey planning is unavailable right now. Try again in a minute.';

export class TripPlanError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.status = status;
  }
}

/** Calls `GET /api/Trips/plan` (the same-origin proxy, Task 1) and returns
 * the parsed response, or throws [`TripPlanError`] with the backend's own
 * plain-text error body as its message -- both `400` (bad CRS/results
 * value) and `404` (no CIF data published for this date yet) are real,
 * distinct, user-actionable outcomes (this plan's own Review Focus), so
 * the caller can render each differently rather than one generic failure
 * message.
 *
 * Every OTHER status (a 500, a gateway timeout, an unreachable backend, ...)
 * is not a backend-authored, user-actionable message -- it can be Next's own
 * raw proxy error text (an HTML error page fragment, a stack trace line),
 * which `PlanTripFlow` would otherwise show verbatim in its alert (Signal
 * Box Audit, flib Low finding: "non-400/404 error bodies are shown to the
 * user verbatim"). That looks broken and can leak implementation details, so
 * those statuses get a generic, honest message instead -- the raw body is
 * still logged to the console for debugging, just not shown to the visitor. */
export async function fetchTripPlan(query: TripPlanQuery): Promise<TripPlanResponse> {
  const response = await fetch(`/api/Trips/plan?${buildTripPlanQuery(query)}`);
  if (!response.ok) {
    const body = await response.text();
    if (response.status === 503) {
      // api could not reach its database, or is shedding load: retryable.
      log.warn('fetchTripPlan: api temporarily unavailable (503)', { body });
      throw new TripPlanError(TRIP_PLAN_UNAVAILABLE_MESSAGE, 503);
    }
    if (response.status !== 400 && response.status !== 404) {
      log.error('fetchTripPlan: request failed', { status: response.status, body });
      throw new TripPlanError(describeFailure('plan', 'this trip', response.status), response.status);
    }
    throw new TripPlanError(body || describeFailure('plan', 'this trip', response.status), response.status);
  }
  return (await response.json()) as TripPlanResponse;
}

/** The options-mode search-size 400 (docs/api-changelog.md, 2026-10-06):
 * `... is too large a search (...); use fewer waypoints or vias, a lower
 * maxChanges, or results=fastest`. */
export function isTooLargeSearchError(error: TripPlanError): boolean {
  return error.status === 400 && error.message.includes('too large a search');
}

/** What the planner suggests under a segment's `noResultReason`, for the
 * constraints a visitor set in the form's "Advanced options" (and the
 * change cap); `null` for the rest, whose own message already says it all.
 * `name` turns a code into what the visitor saw in the picker. */
export function noResultHint(reason: TripPlanNoResultReason, name: (code: string) => string): string | null {
  const names = reason.values.map(name).join(', ');
  switch (reason.constraint) {
    case 'via':
      return (
        `Try removing ${names || 'a via'} from "Pass through", or making it a stop you call at. ` +
        'A train running through a small station without stopping is only seen at timing points, ' +
        'so a via there may need a train that calls.'
      );
    case 'maxChanges':
      return 'Allow more changes in Advanced options, or switch Results to Fastest.';
    case 'avoid':
      return `Try removing ${names || 'a station'} from "Avoid completely".`;
    case 'avoidStop':
      return `Try removing ${names || 'a station'} from "Don't stop at".`;
    case 'avoidChange':
      return `Try removing ${names || 'a station'} from "Don't change at".`;
    case 'avoidCombined':
      return 'Try removing some of the stations in Advanced options: together they rule out every route.';
    default:
      return null;
  }
}

/** The station a journey used for a via or waypoint that may be a choice
 * (2026-10-07): the station's own label, plus which choice it satisfied when
 * that was a group ("London Kings Cross (one of the London Terminals)") or
 * several stations. `stationName` labels a code; `groupName` names a
 * group's code (`undefined` when the groups are not loaded). */
export function satisfiedStationLabel(
  hit: { crs: string; matchedCrs?: string | null },
  stationName: (code: string) => string,
  groupName: (code: string) => string | undefined,
): string {
  const matched = hit.matchedCrs ?? hit.crs;
  const station = stationName(matched);
  if (matched === hit.crs) return station;
  if (isGroupCode(hit.crs)) return `${station} (one of the ${groupName(hit.crs) ?? hit.crs})`;
  return `${station} (one of ${hit.crs.split('|').map(stationName).join(', ')})`;
}

/** Where a journey stopped for one of the visitor's "Call at" stops that
 * was a choice: "Stops at London Kings Cross (one of the London Terminals)";
 * at an end of the trip, "Starts at" / "Ends at". `station` is
 * [`satisfiedStationLabel`]'s. */
export function waypointSatisfiedLabel(stop: Pick<TripPlanWaypointSatisfied, 'how'>, station: string): string {
  switch (stop.how) {
    case 'origin':
      return `Starts at ${station}`;
    case 'destination':
      return `Ends at ${station}`;
    case 'walk':
      return `Walks to ${station}`;
    case 'call':
      return `Stops at ${station}`;
  }
}

/** How a route passed one of the visitor's "Pass through" stations, as the
 * itinerary card says it: "Passes through Stafford without stopping" vs
 * "Calls at Stafford". */
export function viaSatisfiedLabel(via: Pick<TripPlanViaSatisfied, 'crs' | 'how'>, name: string | undefined): string {
  const station = name ?? via.crs;
  switch (via.how) {
    case 'pass':
      return `Passes through ${station} without stopping`;
    case 'call':
      return `Calls at ${station}`;
    case 'walk':
      return `Goes via ${station} on foot`;
  }
}
