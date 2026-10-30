import { addCalendarDays, londonCalendarDay, londonDayEndIso } from './londonWallClock';
import { legArrivalInstant, legDepartureInstant, legExpectedDepartureInstant } from './journeyLegChaining';
import { journeyDetailSummary, journeyListItemSummary, type JourneySummaryGroup } from './journeyStatus';
import { viewerCanPickTrain, type TrackedRowStatus } from './trackedRowStatus';
import { splitTrainRows } from './upcomingPast';
import { isUntrackedLeg } from './untrackedLeg';
import type {
  JourneyDetail,
  JourneyLegDetail,
  JourneyListItem,
  JourneyStop,
  LateDepartureReliance,
  TrackedTrainListItem,
  TrackedTrainState,
} from './types';

/** The home page's journey sections (logged in): the journey under way
 * now, then the ones still to come. A train tracked on its own (no
 * journey) is a one-leg journey here, as on the Journeys page
 * (`lib/upcomingPast.ts`), so it can be either. `GET /Journeys/mine` names a journey
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

/** How many journeys shared with the caller, and not followed, the home
 * page lists under "Shared with you" (DS-199), and so reads in full at
 * most: a short glance, with the rest on the Journeys page. */
export const MAX_SHARED_JOURNEYS = 3;

/** A journey the home page shows, with its full detail when it was read. */
export interface HomeJourney {
  item: JourneyListItem;
  detail: JourneyDetail | null;
  /** The first leg's expected departure, when known. */
  departure: string | null;
}

/** A train tracked on its own (`GET /Train/mine`, in no journey), shown
 * as a one-leg journey with the Track list's own badge status
 * (`trackedTrainRowStatus`). */
export interface HomeLoneTrain {
  train: TrackedTrainListItem;
  status: TrackedRowStatus;
  /** Its departure from the traveller's stop, when known: the live one
   * (`legExpectedDepartureInstant`) when read in full, else the
   * timetabled one from its list row. */
  departure: string | null;
  /** The train as a one-leg journey's leg (`loneTrainLeg`), when its state
   * was read in full; `null` for one on another day, past the cap, or
   * whose read failed, which goes by its list row alone. */
  leg: JourneyLegDetail | null;
}

/** One row of the home page's journey sections: a journey, or a train
 * tracked on its own (`isLoneTrain`). */
export type HomeEntry = HomeJourney | HomeLoneTrain;

/** Whether a home entry is a train tracked on its own. */
export function isLoneTrain(entry: HomeEntry): entry is HomeLoneTrain {
  return 'train' in entry;
}

/** The entry under way shown as current: a journey read in full, or a
 * lone train. */
export type HomeCurrent = (HomeJourney & { detail: JourneyDetail }) | HomeLoneTrain;

/** The journey shown as current, and any others under way at once. */
export interface HomeJourneySplit {
  current: HomeCurrent | null;
  /** Other journeys under way at the same time, soonest arriving first. */
  alsoInProgress: HomeCurrent[];
  /** Journeys whose first departure is still to come, soonest first. */
  upcoming: HomeEntry[];
}

/** The headline status of a journey on the home page: the WHOLE journey's
 * summary (`summariseJourney`), the rule the journey page's own badge and
 * the Journeys list use. Read in full, that includes a missed or tight
 * connection (the api's `connection` on each leg, `journeyDetailSummary`).
 * A row not read in full (a journey on a later day) goes by its list row
 * (`journeyListItemSummary`), which has no connection to judge. */
export function homeJourneyStatusGroup(journey: HomeJourney): JourneySummaryGroup {
  const summary = journey.detail ? journeyDetailSummary(journey.detail.legs) : null;
  return (summary ?? journeyListItemSummary(journey.item)).group;
}

function firstServiceDate(item: JourneyListItem): string {
  return item.firstServiceDate ?? item.serviceDate;
}

/** What the home page reads in full: journeys (`GET /Journeys/{id}`) and
 * lone trains (`GET /Train/{id}`, what the train's own by-id page reads). */
export interface HomeDetailReads {
  journeyIds: number[];
  trainIds: number[];
}

/** One row `homeDetailReads` might read; `departure` in ms. */
interface Candidate {
  kind: 'journey' | 'train';
  id: number;
  day: string;
  departure: number;
}

/** The rows worth reading in full, under one cap
 * (`MAX_HOME_JOURNEY_DETAILS`) shared by both kinds:
 *
 * - a journey that starts today, or started yesterday and whose current
 *   leg is not before yesterday (one still running past midnight);
 * - a lone train dated today or yesterday that the api hasn't called
 *   travelled (the Journeys page's Upcoming half, `splitTrainRows`): one
 *   it has is over by the same arrival rule, so reading it would change
 *   nothing.
 *
 * Over the cap, the soonest are kept: today's before yesterday's, then by
 * departure. A journey's list row has no departure time, so it counts from
 * the start of its day, as the api's own travel order does for a time it
 * doesn't know (`travel_order::day_start_sql`); then journeys before
 * trains, newest first. */
export function homeDetailReads(
  items: readonly JourneyListItem[],
  trains: readonly TrackedTrainListItem[],
  today: string,
): HomeDetailReads {
  const yesterday = addCalendarDays(today, -1);
  const journeys: Candidate[] = items
    .filter((item) => {
      const first = firstServiceDate(item);
      return first <= today && first >= yesterday && item.serviceDate >= yesterday;
    })
    .map((item) => ({ kind: 'journey', id: item.id, day: item.serviceDate, departure: Number.NEGATIVE_INFINITY }));
  const lone: Candidate[] = splitTrainRows(trains, today)
    .upcoming.map(({ train }) => train)
    .filter((train) => train.serviceDate <= today && train.serviceDate >= yesterday)
    .map((train) => ({
      kind: 'train',
      id: train.id,
      day: train.serviceDate,
      departure: train.pinScheduledDeparture ? Date.parse(train.pinScheduledDeparture) : Number.POSITIVE_INFINITY,
    }));
  const kept = [...journeys, ...lone]
    .sort(
      (a, b) =>
        b.day.localeCompare(a.day) ||
        (a.departure === b.departure ? 0 : a.departure < b.departure ? -1 : 1) ||
        (a.kind === b.kind ? 0 : a.kind === 'journey' ? -1 : 1) ||
        b.id - a.id,
    )
    .slice(0, MAX_HOME_JOURNEY_DETAILS);
  return {
    journeyIds: kept.filter((read) => read.kind === 'journey').map((read) => read.id),
    trainIds: kept.filter((read) => read.kind === 'train').map((read) => read.id),
  };
}

/** A lone train as the one train leg of a journey, from its own state
 * (`GET /Train/{id}`), so the journey rules (`legIsOver`, `legEndMs`,
 * `legExpectedDepartureInstant`) read it exactly as they read a journey's
 * train leg: its boarding and alighting stops are the train's pin. */
export function loneTrainLeg(train: TrackedTrainListItem, state: TrackedTrainState): JourneyLegDetail {
  return {
    id: train.id,
    kind: 'train',
    originCrs: train.pinOriginCrs ?? state.pinOriginCrs,
    originName: train.pinOriginName ?? state.pinOriginName,
    destinationCrs: train.pinDestinationCrs ?? state.pinDestinationCrs,
    destinationName: train.pinDestinationName ?? state.pinDestinationName,
    serviceDate: train.serviceDate,
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    windowSearched: false,
    trackedTrainState: state,
    legSkip: null,
  };
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
  return legsPhase(detail.legs, now);
}

function legsPhase(legs: readonly JourneyLegDetail[], now: Date): JourneyPhase {
  const last = legs.at(-1);
  if (last === undefined || legIsOver(last, now, true)) return 'past';
  // Any leg, not only the first: a later leg reported moving means the
  // journey is under way even when the first leg's times are unknown.
  return legs.some((leg) => legHasStarted(leg, now)) ? 'inProgress' : 'upcoming';
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

/** The next change that works only because the train changed onto is
 * running late (DS-141, `connection.reliesOnLateDeparture`), for the
 * current journey card: the change onto the current leg while its train
 * has not left yet (the traveller is waiting for it), else the change onto
 * the leg after it. `null` when neither relies on a late train. */
export function nextLateDepartureReliance(
  detail: JourneyDetail,
  leg: JourneyLegDetail,
  now: Date,
): LateDepartureReliance | null {
  const index = detail.legs.indexOf(leg);
  if (index < 0) return null;
  const departure = legExpectedDepartureInstant(leg);
  const waiting = index > 0 && departure !== null && Date.parse(departure) > now.getTime();
  if (waiting && leg.connection?.reliesOnLateDeparture) return leg.connection.reliesOnLateDeparture;
  return detail.legs[index + 1]?.connection?.reliesOnLateDeparture ?? null;
}

/** Where a lone train is, relative to `now`, when the api has not called
 * it travelled.
 *
 * Read in full (`leg`), exactly as a one-leg journey (`journeyPhase`):
 * under way once its live or actual departure has passed (or it reports
 * moving), over at its actual arrival, else the timetable plus its delay,
 * else plus `TRAVEL_ARRIVAL_GRACE_MINUTES` (`legIsOver`).
 *
 * Otherwise from its list row alone, which has no live departure or any
 * arrival time: the api's `travelled` already stands for `legIsOver`'s
 * arrival rule (`travel_order::train_arrived_sql`), and on top of that, as
 * `legIsOver` has it for a last leg, a completed train is over and a
 * cancelled one once it was due to leave. It is under way once it reports
 * moving or its timetabled departure has passed. */
export function loneTrainPhase(train: TrackedTrainListItem, leg: JourneyLegDetail | null, now: Date): JourneyPhase {
  if (leg !== null) return legsPhase([leg], now);
  const departed = isBefore(train.pinScheduledDeparture, now);
  if (train.status === 'completed' || (train.status === 'cancelled' && departed)) return 'past';
  return train.status === 'en_route' || departed ? 'inProgress' : 'upcoming';
}

/** When an entry under way is expected to end (its last leg's live
 * arrival, `legEndMs`), for picking the soonest-arriving of two under way
 * at once. A lone train known only by its list row has no arrival time:
 * it sorts after any entry whose end is known. */
function expectedEnd(entry: HomeCurrent): number {
  const last = isLoneTrain(entry) ? entry.leg : entry.detail.legs.at(-1);
  return (last && legEndMs(last)) ?? Number.POSITIVE_INFINITY;
}

function departureMs(entry: HomeEntry): number {
  return entry.departure === null ? Number.POSITIVE_INFINITY : Date.parse(entry.departure);
}

function startDay(entry: HomeEntry): string {
  return isLoneTrain(entry) ? entry.train.serviceDate : firstServiceDate(entry.item);
}

/** The last tie-break: journeys before lone trains, then newest first. */
function byKindThenNewest(a: HomeEntry, b: HomeEntry): number {
  const kind = Number(isLoneTrain(a)) - Number(isLoneTrain(b));
  const id = (entry: HomeEntry) => (isLoneTrain(entry) ? entry.train.id : entry.item.id);
  return kind || id(b) - id(a);
}

/** Splits the caller's journeys and lone trains for the home page.
 *
 * - In progress: a journey read in full and between first departure and
 *   last arrival (`journeyPhase`), or a lone train under way
 *   (`loneTrainPhase`). More than one at once (a return trip overlapping,
 *   two people's plans): the one arriving soonest is the current journey,
 *   the rest are listed under it. One with no known arrival (a lone train
 *   not read in full) comes after those with one; between two such, the
 *   one that left last is the one the traveller is likeliest on.
 * - Upcoming: a journey starting on a later day, or one read in full whose
 *   first departure is still to come, and a lone train still to come.
 *   Soonest first, journeys and trains together: by first day, then
 *   departure time where known, then journeys before trains, newest first.
 * - Everything else (past, or not read when it should have been) is left
 *   to the Journeys page.
 *
 * `details` maps journey id to its detail; a missing entry is a journey
 * that wasn't read or whose read failed. One that starts today with no
 * detail is listed as upcoming rather than dropped: its date says no more,
 * and the Journeys page has the rest. `trains` are the caller's trains in
 * no journey (`splitJourneyTrains`), in `GET /Train/mine`'s order, and
 * `trainStates` the ones read in full (`homeDetailReads`); a train missing
 * from it goes by its list row. */
export function splitHomeJourneys(
  items: readonly JourneyListItem[],
  details: ReadonlyMap<number, JourneyDetail>,
  trains: readonly TrackedTrainListItem[],
  now: Date,
  trainStates: ReadonlyMap<number, TrackedTrainState> = new Map(),
): HomeJourneySplit {
  const today = londonCalendarDay(now.toISOString());
  const inProgress: HomeCurrent[] = [];
  const upcoming: HomeEntry[] = [];

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

  // The Journeys page's own Upcoming half: `travelled`, or the fallback.
  for (const { train, status } of splitTrainRows(trains, today).upcoming) {
    const state = trainStates.get(train.id);
    const leg = state ? loneTrainLeg(train, state) : null;
    const entry: HomeLoneTrain = {
      train,
      status,
      departure: (leg && legExpectedDepartureInstant(leg)) ?? train.pinScheduledDeparture,
      leg,
    };
    const phase = loneTrainPhase(train, leg, now);
    if (phase === 'inProgress') inProgress.push(entry);
    else if (phase === 'upcoming') upcoming.push(entry);
  }

  inProgress.sort(
    (a, b) => expectedEnd(a) - expectedEnd(b) || departureMs(b) - departureMs(a) || byKindThenNewest(a, b),
  );
  upcoming.sort(
    (a, b) => startDay(a).localeCompare(startDay(b)) || departureMs(a) - departureMs(b) || byKindThenNewest(a, b),
  );
  const [current = null, ...alsoInProgress] = inProgress;
  return { current, alsoInProgress, upcoming };
}

/** Whether a `GET /Journeys/mine` row is a journey another member shared
 * into one of the caller's groups that the caller doesn't follow
 * (`include=shared`, DS-178): someone else's travel, which the home page
 * lists apart under "Shared with you" rather than among the caller's own.
 * A journey they follow is theirs to watch, and stays with their own. */
export function isSharedNotFollowed(item: JourneyListItem): boolean {
  return !viewerCanPickTrain(item);
}

/** The home page's "Shared with you" (DS-199): journeys shared with the
 * caller that they don't follow (`isSharedNotFollowed`) and that are under
 * way or still to come, by the same phase rules as their own
 * (`splitHomeJourneys`). Under way first, soonest arriving first, then
 * upcoming soonest first. `details` holds the ones read in full. */
export function sharedHomeJourneys(
  items: readonly JourneyListItem[],
  details: ReadonlyMap<number, JourneyDetail>,
  now: Date,
): HomeJourney[] {
  const { current, alsoInProgress, upcoming } = splitHomeJourneys(items, details, [], now);
  const entries: HomeEntry[] = [...(current === null ? [] : [current]), ...alsoInProgress, ...upcoming];
  return entries.filter((entry): entry is HomeJourney => !isLoneTrain(entry));
}
