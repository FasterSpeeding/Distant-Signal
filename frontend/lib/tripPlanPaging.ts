import type { TripPlanQuery } from './tripPlan';
import { calendarAt, serviceMinutes, withTiming } from './tripPlanTiming';
import type { TripPlanItinerary, TripPlanJourney, TripPlanLeg, TripPlanResponse } from './types';

/** "Earlier routes" or "Later routes". */
export type PageDirection = 'earlier' | 'later';

/** One whole journey in the accumulated list: its part of each segment
 * (`segments[s].itineraries[j]`), its `journeys[j]` summary, and its times
 * as minutes from the FIRST search's service date's midnight, so journeys
 * from pages on other service dates (an overnight search, a later page
 * after midnight) compare directly. */
export interface PlanEntry {
  key: string;
  parts: TripPlanItinerary[];
  journey: TripPlanJourney;
  departure: number;
  arrival: number;
}

/** The planner's results as the visitor sees them: the first search's
 * page, with each earlier page prepended and each later page appended.
 * See docs/superpowers/specs/2026-10-19-trips-plan-arrive-by-design.md §4.3. */
export interface PlanList {
  /** The response the visitor's own search returned: segment headings,
   * `cappedByMaxChanges` and the rest of the plan-level fields. */
  base: TripPlanResponse;
  /** The visitor's search, which every page repeats with only its time changed. */
  query: TripPlanQuery;
  /** The service date `query` was sent as (`apiQuery`), which every
   * entry's minutes count from. */
  serviceDate: string;
  entries: PlanEntry[];
  /** The latest departure any page has reported: "Later" departs after it. */
  laterFrom: number;
  /** The earliest arrival any page has reported: "Earlier" arrives before it. */
  earlierBefore: number;
  /** A page in that direction came back empty: nothing more on this day. */
  earlierDone: boolean;
  laterDone: boolean;
  /** Any page's timetable is provisional. */
  provisional: boolean;
}

function legKey(leg: TripPlanLeg): string {
  if (leg.kind === 'transfer') {
    return `${leg.mode}:${leg.originCrs ?? ''}>${leg.destinationCrs ?? ''}`;
  }
  if (leg.kind === 'transit') {
    // A tram has no UID: its line, stops and timetabled departure.
    const departure = `${leg.scheduledDeparture}+${leg.departureDayOffset}`;
    return `${leg.network}:${leg.line?.id ?? leg.serviceMode}@${leg.serviceDate}:${leg.originStop.id}>${leg.destinationStop.id}@${departure}`;
  }
  // The timetabled departure, never the live one: a delay that changes
  // between two pages must not make the same train look new.
  const departure = `${leg.scheduledDeparture}+${leg.departureDayOffset ?? 0}`;
  return `${leg.trainUid}@${leg.serviceDate}:${leg.originCrs ?? ''}>${leg.destinationCrs ?? ''}@${departure}`;
}

/** A stable identity for one whole journey: every train's UID, service
 * date, boarding and alighting point and timetabled departure, and every
 * transfer, in order, segment by segment. Two pages' copies of the same
 * journey have the same key whatever their live times say. */
export function itineraryKey(parts: readonly TripPlanItinerary[]): string {
  return parts.map((part) => part.legs.map(legKey).join(' ')).join(' | ');
}

function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(`${to}T00:00:00Z`) - Date.parse(`${from}T00:00:00Z`)) / 86_400_000);
}

/** `plan`'s whole journeys as entries, timed from `listDate`'s midnight
 * (`serviceDate` is the date `plan` was asked for). `null` when the
 * segments and `journeys` don't line up (an older backend): such a plan
 * can't be paged. */
export function pageEntries(plan: TripPlanResponse, serviceDate: string, listDate: string): PlanEntry[] | null {
  const journeys = plan.journeys ?? [];
  if (plan.segments.length === 0 || plan.segments.some((segment) => segment.itineraries.length !== journeys.length)) {
    return null;
  }
  const shift = daysBetween(listDate, serviceDate) * 1440;
  return journeys.map((journey, j) => {
    const parts = plan.segments.flatMap((segment) => segment.itineraries.slice(j, j + 1));
    return {
      key: itineraryKey(parts),
      parts,
      journey,
      departure: shift + serviceMinutes(journey.departure),
      arrival: shift + serviceMinutes(journey.arrival),
    };
  });
}

/** The list for a new search, or `null` when it found no journey (nothing
 * to page from) or can't be paged. */
export function startList(query: TripPlanQuery, serviceDate: string, plan: TripPlanResponse): PlanList | null {
  const entries = pageEntries(plan, serviceDate, serviceDate);
  if (!entries || entries.length === 0) return null;
  const unique = entries.filter((entry, i) => entries.findIndex((other) => other.key === entry.key) === i);
  return {
    base: plan,
    query,
    serviceDate,
    entries: unique,
    laterFrom: Math.max(...unique.map((entry) => entry.departure)),
    earlierBefore: Math.min(...unique.map((entry) => entry.arrival)),
    earlierDone: false,
    laterDone: false,
    provisional: plan.provisional === true,
  };
}

/** The next page's search: the visitor's search with only its time
 * changed. Sent through `apiQuery` like a typed time, so a time after
 * midnight searches the previous day's late trains too.
 *
 * - Later: depart after the latest departure reported so far, plus a
 *   minute. Every journey on it leaves after every one shown.
 * - Earlier: arrive by the earliest arrival reported so far, less a
 *   minute. The latest departures that make it; [`mergePage`] keeps those
 *   leaving before the earliest shown.
 *
 * The times are the response's `journeys[]` times: the ones the planner
 * searched on, live where it applied live data (DS-140), so the API's own
 * `departAfter`/`arriveBy` bound and the cursor agree and a late train
 * can't be found again from its timetabled minute. */
export function pageQuery(list: PlanList, direction: PageDirection): TripPlanQuery {
  return direction === 'later'
    ? withTiming(list.query, 'departAfter', calendarAt(list.serviceDate, list.laterFrom + 1))
    : withTiming(list.query, 'arriveBy', calendarAt(list.serviceDate, list.earlierBefore - 1));
}

/** `list` with a page in `direction` merged in, and how many journeys it
 * added. `plan` is the page, `serviceDate` the date it was asked for.
 *
 * - An earlier page is prepended, a later one appended, each in the API's
 *   order.
 * - A journey already in the list (by [`itineraryKey`]) is dropped.
 * - So is one inside the span already shown: an earlier page keeps only
 *   journeys leaving before the earliest shown, a later one only those
 *   leaving after the latest. Pages never interleave.
 * - The cursors move to the page's extremes even when nothing was added,
 *   so the next request always moves on.
 * - A page with no journey marks that direction done. */
export function mergePage(
  list: PlanList,
  direction: PageDirection,
  plan: TripPlanResponse,
  serviceDate: string,
): { list: PlanList; added: number } {
  const page = pageEntries(plan, serviceDate, list.serviceDate) ?? [];
  if (page.length === 0) {
    return { list: { ...list, [direction === 'earlier' ? 'earlierDone' : 'laterDone']: true }, added: 0 };
  }
  const seen = new Set(list.entries.map((entry) => entry.key));
  const earliest = Math.min(...list.entries.map((entry) => entry.departure));
  const latest = Math.max(...list.entries.map((entry) => entry.departure));
  const fresh = page.filter((entry) => {
    const outside = direction === 'earlier' ? entry.departure < earliest : entry.departure > latest;
    if (!outside || seen.has(entry.key)) return false;
    seen.add(entry.key);
    return true;
  });
  const next: PlanList = {
    ...list,
    entries: direction === 'earlier' ? [...fresh, ...list.entries] : [...list.entries, ...fresh],
    laterFrom:
      direction === 'later' ? Math.max(list.laterFrom, ...page.map((entry) => entry.departure)) : list.laterFrom,
    earlierBefore:
      direction === 'earlier'
        ? Math.min(list.earlierBefore, ...page.map((entry) => entry.arrival))
        : list.earlierBefore,
    provisional: list.provisional || plan.provisional === true,
  };
  return { list: next, added: fresh.length };
}

/** The list as one `TripPlanResponse`, shaped as the results view reads a
 * single page: `segments[s].itineraries[j]` and `journeys[j]` aligned. */
export function listPlan(list: PlanList): TripPlanResponse {
  return {
    ...list.base,
    provisional: list.provisional,
    segments: list.base.segments.map((segment, s) => ({
      ...segment,
      itineraries: list.entries.flatMap((entry) => entry.parts.slice(s, s + 1)),
      noResultReason: null,
    })),
    journeys: list.entries.map((entry) => entry.journey),
  };
}
