import { legDepartureInstant, londonDateTime } from './journeyLegChaining';
import type { JourneyLegDetail } from './types';

/** A train leg's window search, as "Edit search" on a journey leg holds it
 * once it is complete: the stations as codes, the London service date, and
 * up to four `'HH:MM'` bounds (at least one). What
 * `GET /Journeys/{id}/legs/{legId}/candidates` searches instead of the
 * leg's stored search (`legSearchQuery`), and what the pick saves as the
 * leg's own (`legSearchPickBody`), so the leg and its train always agree. */
export interface LegSearch {
  originCrs: string;
  destinationCrs: string;
  serviceDate: string; // 'YYYY-MM-DD'
  departAfter: string | null;
  departBefore: string | null;
  arriveAfter: string | null;
  arriveBefore: string | null;
}

/** The form's text for one search, before it is checked. */
export interface LegSearchDraft {
  origin: string;
  destination: string;
  serviceDate: string;
  departAfter: string;
  departBefore: string;
  arriveAfter: string;
  arriveBefore: string;
}

/** `'HH:MM'` from the API's `'HH:MM:SS'` (or `'HH:MM'`); `''` for none. */
function hhmm(time: string | null): string {
  return time ? time.slice(0, 5) : '';
}

/** The search a leg was made with, as the form's starting text: every
 * setting stored on the leg (its stations, date and all four time bounds;
 * a window leg stores nothing else). */
export function legSearchDraftFromLeg(
  leg: Pick<
    JourneyLegDetail,
    'originCrs' | 'destinationCrs' | 'serviceDate' | 'departAfter' | 'departBefore' | 'arriveAfter' | 'arriveBefore'
  >,
): LegSearchDraft {
  return {
    origin: leg.originCrs ?? '',
    destination: leg.destinationCrs ?? '',
    serviceDate: leg.serviceDate,
    departAfter: hhmm(leg.departAfter),
    departBefore: hhmm(leg.departBefore),
    arriveAfter: hhmm(leg.arriveAfter),
    arriveBefore: hhmm(leg.arriveBefore),
  };
}

/** "Change train" on a leg that was never window-searched (a pinned
 * train, "I know the train", a leg saved from the planner): a search made
 * from what the leg knows -- its stations (else its train's pin, else the
 * train's own route), its date, and trains from its train's timetabled
 * departure on. Every later train that day is listed, and "Edit search"
 * changes any of it. `null` for a window-searched leg (its own stored
 * search is used) or when the leg's stations aren't known. With no
 * departure time that day, the search starts at midnight. */
export function legSearchPrefill(leg: JourneyLegDetail): LegSearch | null {
  if (leg.windowSearched) return null;
  const state = leg.trackedTrainState;
  const origin = leg.originCrs ?? state?.pinOriginCrs ?? state?.journeyStops?.[0]?.crs ?? null;
  const destination = leg.destinationCrs ?? state?.pinDestinationCrs ?? state?.scheduleDestinationCrs ?? null;
  if (!origin || !destination) return null;
  const departure = legDepartureInstant(leg);
  const london = departure === null ? null : londonDateTime(departure);
  return {
    originCrs: origin.toUpperCase(),
    destinationCrs: destination.toUpperCase(),
    serviceDate: leg.serviceDate,
    departAfter: london !== null && london.date === leg.serviceDate ? london.time : '00:00',
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
  };
}

const TIME = /^\d{2}:\d{2}$/;
const DATE = /^\d{4}-\d{2}-\d{2}$/;

/** A complete search from the form's text, its stations already resolved
 * to codes (`resolveStationCode`), or `null` while it isn't one: both
 * stations, a date, well-formed times, at least one of them, and no
 * earliest after its latest (the same rules as adding a window leg,
 * `validate_window_leg`). */
export function legSearchFromDraft(
  draft: LegSearchDraft,
  stations: { origin: string | null; destination: string | null },
): LegSearch | null {
  const time = (value: string) => (value.trim() === '' ? null : value.trim());
  const bounds = {
    departAfter: time(draft.departAfter),
    departBefore: time(draft.departBefore),
    arriveAfter: time(draft.arriveAfter),
    arriveBefore: time(draft.arriveBefore),
  };
  const set = Object.values(bounds).filter((value): value is string => value !== null);
  if (stations.origin === null || stations.destination === null || !DATE.test(draft.serviceDate)) return null;
  if (set.length === 0 || !set.every((value) => TIME.test(value))) return null;
  if (bounds.departAfter && bounds.departBefore && bounds.departAfter > bounds.departBefore) return null;
  if (bounds.arriveAfter && bounds.arriveBefore && bounds.arriveAfter > bounds.arriveBefore) return null;
  return {
    originCrs: stations.origin,
    destinationCrs: stations.destination,
    serviceDate: draft.serviceDate,
    ...bounds,
  };
}

/** The candidates route's query for an edited search. */
export function legSearchQuery(search: LegSearch): URLSearchParams {
  const params = new URLSearchParams({
    origin: search.originCrs,
    destination: search.destinationCrs,
    serviceDate: search.serviceDate,
  });
  for (const key of ['departAfter', 'departBefore', 'arriveAfter', 'arriveBefore'] as const) {
    const value = search[key];
    if (value !== null) params.set(key, value);
  }
  return params;
}

/** The pick's `search` field: the window-leg fields, less the date (the
 * pick's own `serviceDate`). */
export function legSearchPickBody(search: LegSearch) {
  return {
    originCrs: search.originCrs,
    destinationCrs: search.destinationCrs,
    departWindow: { after: search.departAfter, before: search.departBefore },
    arriveWindow: { after: search.arriveAfter, before: search.arriveBefore },
  };
}
