import { addCalendarDays, londonCalendarDay, londonDayEndIso } from './londonWallClock';
import { legArrivalInstant, legDepartureInstant, legExpectedDepartureInstant } from './journeyLegChaining';
import { journeyListItemStatusGroup, worstLegStatus, type LegStatusGroup } from './journeyStatus';
import { isUntrackedLeg } from './untrackedLeg';
import type { JourneyDetail, JourneyLegDetail, JourneyListItem, JourneyStop } from './types';

/** The home page's journey sections (logged in): the journey under way
 * now, then the ones still to come. `GET /Journeys/mine` names a journey
 * and its current leg but carries no departure or arrival time, so the
 * journeys that could be under way or still to come today are read in full
 * (`GET /Journeys/{id}`) and timed from their legs, with the live delay and
 * cancellation the journey page itself shows (`lib/journeyLegChaining.ts`).
 * A journey starting on a later day is upcoming from its date alone. */

/** Where a journey is, relative to `now`. */
export type JourneyPhase = 'upcoming' | 'inProgress' | 'past';

/** How many journeys the home page reads in full: the ones that start
 * today or yesterday (an overnight journey). A handful at most in practice;
 * the cap keeps one busy day from becoming a burst of detail reads. */
export const MAX_HOME_JOURNEY_DETAILS = 6;

/** How many upcoming journeys the home page lists before "View all". */
export const MAX_UPCOMING_JOURNEYS = 5;

/** A journey the home page shows, with its full detail when it was read. */
export interface HomeJourney {
  item: JourneyListItem;
  detail: JourneyDetail | null;
  /** The first leg's expected departure, when known. */
  departure: string | null;
}

/** The journey shown as current, and any others under way at once. */
export interface HomeJourneySplit {
  current: (HomeJourney & { detail: JourneyDetail }) | null;
  /** Other journeys under way at the same time, soonest arriving first. */
  alsoInProgress: (HomeJourney & { detail: JourneyDetail })[];
  /** Journeys whose first departure is still to come, soonest first. */
  upcoming: HomeJourney[];
}

/** The headline status of a journey on the home page: the WHOLE journey's,
 * never only its current train's. Read in full, that is the worst of every
 * leg (`worstLegStatus`, what the journey page's own badge shows). A row
 * not read in full (a journey on a later day) has only its current leg to
 * go on (`journeyListItemStatusGroup`), which for a journey not yet under
 * way is its first leg. The one place both sections take a status from, so
 * a whole-journey summary on `GET /Journeys/mine` replaces the fallback
 * here alone. */
export function homeJourneyStatusGroup(journey: HomeJourney): LegStatusGroup {
  return (journey.detail && worstLegStatus(journey.detail.legs)) ?? journeyListItemStatusGroup(journey.item);
}

function firstServiceDate(item: JourneyListItem): string {
  return item.firstServiceDate ?? item.serviceDate;
}

/** The list rows worth reading in full: a journey that starts today, or
 * started yesterday and whose current leg is not before yesterday (one
 * still running past midnight). Newest travel first, capped. */
export function homeJourneyDetailIds(items: readonly JourneyListItem[], today: string): number[] {
  const yesterday = addCalendarDays(today, -1);
  return items
    .filter((item) => {
      const first = firstServiceDate(item);
      return first <= today && first >= yesterday && item.serviceDate >= yesterday;
    })
    .sort((a, b) => b.serviceDate.localeCompare(a.serviceDate) || b.id - a.id)
    .slice(0, MAX_HOME_JOURNEY_DETAILS)
    .map((item) => item.id);
}

function isBefore(instant: string | null, now: Date): boolean {
  return instant !== null && Date.parse(instant) <= now.getTime();
}

/** How long past its timetabled arrival a train leg with no live data at
 * all (no arrival report, no running delay) still counts as under way: a
 * train Network Rail hasn't reported on is not dropped the minute it is
 * due. The same figure, under the same name, as the api's travel order
 * (`TRAVEL_ARRIVAL_GRACE_MINUTES` in crates/api/src/data/travel_order.rs),
 * so the home page and `GET /Journeys/mine`'s order agree on when a
 * journey is over. Change both together. */
export const TRAVEL_ARRIVAL_GRACE_MINUTES = 30;

const MINUTE_MS = 60_000;

/** The stop a train leg gets off at, looked for after its own origin. */
function legAlightingStop(leg: JourneyLegDetail): JourneyStop | null {
  const state = leg.trackedTrainState;
  const stops = state?.journeyStops ?? null;
  const destination = leg.destinationCrs ?? state?.scheduleDestinationCrs ?? null;
  if (stops === null || destination === null) return null;
  const same = (crs: string | null, other: string) => crs !== null && crs.toUpperCase() === other.toUpperCase();
  const origin = leg.originCrs ?? state?.pinOriginCrs ?? null;
  const originIndex = origin === null ? -1 : stops.findIndex((stop) => same(stop.crs, origin));
  return stops.slice(originIndex + 1).find((stop) => same(stop.crs, destination)) ?? null;
}

/** When a leg ends, in ms since the epoch, by the one rule for "in
 * progress" against "past" on the home page:
 *
 * 1. a train's ACTUAL arrival at the traveller's alighting stop, when
 *    reported;
 * 2. else, with a live running delay, the timetabled arrival plus that
 *    delay (early running ignored);
 * 3. else the timetabled arrival plus `TRAVEL_ARRIVAL_GRACE_MINUTES`.
 *
 * The client copy of the api's `travel_order::train_arrived_sql`, which
 * orders `GET /Journeys/mine` by the same rule. (A day with no arrival
 * time at all is handled by `legIsOver`, as there.)
 *
 * An untracked leg ends at its planned arrival. `null` when nothing is
 * known (no train picked, no stop list). */
export function legEndMs(leg: JourneyLegDetail): number | null {
  if (isUntrackedLeg(leg)) return leg.plannedArrival ? Date.parse(leg.plannedArrival) : null;
  const stop = legAlightingStop(leg);
  if (stop?.actualArrival) return Date.parse(stop.actualArrival);
  const scheduled = legArrivalInstant(leg);
  if (scheduled === null) return null;
  const delay = leg.trackedTrainState?.delayMinutes ?? null;
  if (delay !== null) return Date.parse(scheduled) + Math.max(delay, 0) * MINUTE_MS;
  return Date.parse(scheduled) + TRAVEL_ARRIVAL_GRACE_MINUTES * MINUTE_MS;
}

/** Whether a leg is over (`legEndMs`). A train leg is over at once when
 * its train reports completed or an actual arrival at the alighting stop,
 * and never only because its timetabled arrival has passed: a late train
 * keeps the journey current until it really arrives. A cancelled train
 * that is the LAST leg ends the journey once its departure time has
 * passed (there is no onward leg to be on; before then the journey stays
 * upcoming, badged "Cancelled"); a cancelled earlier leg is over at its
 * timetabled arrival, so the journey moves on to the next leg. An
 * untracked leg is over by the clock. With nothing known, a leg is over
 * once its day is. */
export function legIsOver(leg: JourneyLegDetail, now: Date, isLast = false): boolean {
  if (isUntrackedLeg(leg)) return leg.untrackedStatus === 'done' || isBefore(leg.plannedArrival ?? null, now);
  const state = leg.trackedTrainState;
  if (state?.status === 'completed') return true;
  if (legAlightingStop(leg)?.actualArrival) return true;
  if (state?.status === 'cancelled') {
    return isBefore(isLast ? legDepartureInstant(leg) : legArrivalInstant(leg), now);
  }
  const end = legEndMs(leg);
  if (end !== null) return end <= now.getTime();
  return isBefore(londonDayEndIso(leg.serviceDate), now);
}

/** Whether a leg has started: its expected departure has passed, or its
 * train or untracked leg says it is moving or done. */
function legHasStarted(leg: JourneyLegDetail, now: Date): boolean {
  if (isUntrackedLeg(leg) && leg.untrackedStatus && leg.untrackedStatus !== 'planned') return true;
  const status = leg.trackedTrainState?.status;
  if (status === 'en_route' || status === 'completed') return true;
  return isBefore(legExpectedDepartureInstant(leg), now);
}

/** Where a journey is: upcoming until its first departure (live) has
 * passed, past once its last leg is over (`legIsOver`), and in progress in
 * between. With no departure time known yet (a first leg with no train
 * picked), it is upcoming until its first day ends. */
export function journeyPhase(detail: JourneyDetail, now: Date): JourneyPhase {
  const last = detail.legs.at(-1);
  if (last === undefined || legIsOver(last, now, true)) return 'past';
  // Any leg, not only the first: a later leg reported moving means the
  // journey is under way even when the first leg's times are unknown.
  return detail.legs.some((leg) => legHasStarted(leg, now)) ? 'inProgress' : 'upcoming';
}

/** The leg the traveller is on or heading for now: the first leg not yet
 * over, else the last. */
export function currentLeg(detail: JourneyDetail, now: Date): JourneyLegDetail | null {
  return (
    detail.legs.find((leg, index) => !legIsOver(leg, now, index === detail.legs.length - 1)) ??
    detail.legs.at(-1) ??
    null
  );
}

/** When the journey is expected to end (its last leg's live arrival), for
 * picking the soonest-arriving of two journeys under way at once. */
function expectedEnd(detail: JourneyDetail): number {
  const last = detail.legs.at(-1);
  return (last && legEndMs(last)) ?? Number.POSITIVE_INFINITY;
}

function departureMs(journey: HomeJourney): number {
  return journey.departure === null ? Number.POSITIVE_INFINITY : Date.parse(journey.departure);
}

/** Splits the caller's journeys for the home page.
 *
 * - In progress: read in full and between first departure and last
 *   arrival (`journeyPhase`). More than one at once (a return trip
 *   overlapping, two people's plans): the one arriving soonest is the
 *   current journey, the rest are listed under it.
 * - Upcoming: a journey starting on a later day, or one read in full whose
 *   first departure is still to come. Soonest first: by first day, then
 *   departure time where known, then newest.
 * - Everything else (past, or not read when it should have been) is left
 *   to the Journeys page.
 *
 * `details` maps journey id to its detail; a missing entry is a journey
 * that wasn't read or whose read failed. One that starts today with no
 * detail is listed as upcoming rather than dropped: its date says no more,
 * and the Journeys page has the rest. */
export function splitHomeJourneys(
  items: readonly JourneyListItem[],
  details: ReadonlyMap<number, JourneyDetail>,
  now: Date,
): HomeJourneySplit {
  const today = londonCalendarDay(now.toISOString());
  const inProgress: (HomeJourney & { detail: JourneyDetail })[] = [];
  const upcoming: HomeJourney[] = [];

  for (const item of items) {
    const detail = details.get(item.id) ?? null;
    if (detail === null) {
      if (firstServiceDate(item) >= today) upcoming.push({ item, detail: null, departure: null });
      continue;
    }
    const first = detail.legs[0];
    const departure = first ? legExpectedDepartureInstant(first) : null;
    const phase = journeyPhase(detail, now);
    if (phase === 'inProgress') inProgress.push({ item, detail, departure });
    else if (phase === 'upcoming') upcoming.push({ item, detail, departure });
  }

  inProgress.sort((a, b) => expectedEnd(a.detail) - expectedEnd(b.detail) || b.item.id - a.item.id);
  upcoming.sort(
    (a, b) =>
      firstServiceDate(a.item).localeCompare(firstServiceDate(b.item)) ||
      departureMs(a) - departureMs(b) ||
      b.item.id - a.item.id,
  );
  const [current = null, ...alsoInProgress] = inProgress;
  return { current, alsoInProgress, upcoming };
}
