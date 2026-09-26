import type { TripPlanResponse } from './types';

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
  }
  return Array.from(codes);
}

export interface TripPlanQuery {
  originCrs: string;
  destinationCrs: string;
  /** Ordered, e.g. `['YRK', 'NCL']` -- entered order, never reordered. */
  waypointCrs: string[];
  date: string; // "YYYY-MM-DD"
  departAfter?: string; // "HH:MM"
  results: 'fastest' | 'options';
}

/** Builds `GET /Trips/plan`'s query string from a [`TripPlanQuery`] --
 * pure and independently testable, matching this codebase's own
 * "small pure helper, tested separately from the fetch call" convention
 * (e.g. `lib/trackAgainPrefill.ts`'s own `trackAgainHref`). */
export function buildTripPlanQuery(query: TripPlanQuery): string {
  const params = new URLSearchParams({
    origin: query.originCrs.trim().toUpperCase(),
    destination: query.destinationCrs.trim().toUpperCase(),
    date: query.date,
    results: query.results,
  });
  const waypoints = query.waypointCrs.map(c => c.trim().toUpperCase()).filter(c => c.length > 0);
  if (waypoints.length > 0) {
    params.set('waypoints', waypoints.join(','));
  }
  if (query.departAfter) {
    params.set('departAfter', `${query.departAfter}:00`);
  }
  return params.toString();
}

export class TripPlanError extends Error {
  constructor(
    message: string,
    public status: number
  ) {
    super(message);
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
    if (response.status !== 400 && response.status !== 404) {
      console.error(`fetchTripPlan: request failed with ${response.status}`, body);
      throw new TripPlanError('Something went wrong planning this trip. Please try again.', response.status);
    }
    throw new TripPlanError(body || 'Could not plan this trip.', response.status);
  }
  return response.json();
}
