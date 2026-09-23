import type { TripPlanResponse } from './types';

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
 * message. */
export async function fetchTripPlan(query: TripPlanQuery): Promise<TripPlanResponse> {
  const response = await fetch(`/api/Trips/plan?${buildTripPlanQuery(query)}`);
  if (!response.ok) {
    const body = await response.text();
    throw new TripPlanError(body || 'Could not plan this trip.', response.status);
  }
  return response.json();
}
