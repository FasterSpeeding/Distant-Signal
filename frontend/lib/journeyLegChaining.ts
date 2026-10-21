import dayjs from 'dayjs';
import { LONDON_TZ } from './londonWallClock';
import { stopDisplayArrival, stopDisplayTime } from './stopTimes';
import { isUntrackedLeg } from './untrackedLeg';
import type { JourneyDetail, JourneyLegDetail, JourneyStop } from './types';

/** A London calendar date (`'YYYY-MM-DD'`) and wall-clock time (`'HH:MM'`). */
export interface LondonDateTime {
  date: string;
  time: string;
}

/** What a caller wanting to chain another leg onto a journey needs to know
 * about its CURRENT last leg -- shared by `app/journeys/[id]/page.tsx` (the
 * "Add a leg" button on an already-created journey) and
 * `JourneyCreationFlow.tsx` (the same button, offered inline during initial
 * creation -- see that component's own doc comment for the design). Factored
 * out of the page rather than left as private, duplicated logic in two
 * places, which would otherwise be free to drift apart on exactly this "can
 * the user chain another leg on right now" judgment call.
 *
 * The station a next leg's own origin should default to -- the CURRENT
 * last leg's own `destinationCrs`, falling back to its matched train's
 * `scheduleDestinationCrs` for the case a `knownTrain`/`window`-mode leg's
 * own `destination_crs` was never set at creation time (only the bound
 * train's own schedule knows it). `null` for a journey with no legs at all,
 * or when neither source has a destination yet. */
export function journeyPriorDestinationCrs(journey: JourneyDetail): string | null {
  const lastLeg = journey.legs.at(-1) ?? null;
  return lastLeg?.destinationCrs ?? lastLeg?.trackedTrainState?.scheduleDestinationCrs ?? null;
}

/** Whether a next leg can be chained onto this journey right now. There is
 * nothing to chain a new leg onto until the CURRENT last leg has a real
 * matched train (`trackedTrainState !== null`) -- an unmatched leg has no
 * settled destination/arrival time yet for a following leg's origin to key
 * off, and `POST /Journeys/{id}/legs` has no way to validate a chain onto a
 * leg that might still resolve to nothing. An untracked leg (a Tube ride, a
 * walk) is complete as entered: its planned arrival is what the next leg
 * starts from (spec 2026-10-20 §2.3). `false` for a journey with no
 * legs at all (shouldn't occur in practice, but this mirrors the same
 * defensive posture `defaultJourneyTitle`/`trackAgainPrefill` already take
 * for the identical edge case rather than assuming the invariant holds). */
export function journeyCanAddLeg(journey: JourneyDetail): boolean {
  const lastLeg = journey.legs.at(-1) ?? null;
  return lastLeg !== null && (isUntrackedLeg(lastLeg) || lastLeg.trackedTrainState !== null);
}

/** The CURRENT last leg's destination as words, for a next untracked leg's
 * "from" field: the station or place name the API resolved, else its
 * station code. `null` with no last leg or no known destination. */
export function journeyPriorDestinationName(journey: JourneyDetail): string | null {
  const lastLeg = journey.legs.at(-1) ?? null;
  if (lastLeg === null) return null;
  return (
    lastLeg.destinationName ?? lastLeg.trackedTrainState?.scheduleDestinationName ?? journeyPriorDestinationCrs(journey)
  );
}

/** When the CURRENT last leg's train is timetabled to reach that leg's
 * destination, in Europe/London time: what the "Add a leg" dialog
 * pre-fills a next leg's service date and earliest departure from, the
 * same way `journeyPriorDestinationCrs` pre-fills its origin. A leg that
 * arrives after midnight lands on the next day's date.
 *
 * The time is the PUBLIC (passenger timetable) arrival at that stop,
 * falling back to the working one only when the API sent no public times
 * at all (`stopDisplayArrival`). Deliberately the timetabled time, not a
 * live estimate or actual: the next train is chosen from the timetable
 * too. The stop is looked for after the leg's own origin, so a route that
 * calls at the same station twice picks the call this leg gets off at.
 *
 * `null` (the dialog keeps its usual defaults) when there is no last leg
 * or its arrival isn't known (`legArrivalInstant`). */
export function journeyPriorArrival(journey: JourneyDetail): LondonDateTime | null {
  const lastLeg = journey.legs.at(-1) ?? null;
  const arrival = lastLeg === null ? null : legArrivalInstant(lastLeg);
  return arrival === null ? null : londonDateTime(arrival);
}

/** When one leg is timetabled to reach its own destination, as an
 * RFC3339 instant, whatever kind of leg it is. An untracked leg (spec
 * 2026-10-20 §2.3) gives its planned arrival. A train leg is read from its
 * matched train's stop list: `null` while it has no matched train, no stop
 * list, no stop for the destination, or no arrival time at it.
 * `journeyPriorArrival` and the connectors between legs read this, so a
 * rail leg added after a Tube leg starts from the Tube leg's arrival. */
export function legArrivalInstant(leg: JourneyLegDetail): string | null {
  if (isUntrackedLeg(leg)) return leg.plannedArrival ?? null;
  const stop = legDestinationStop(leg);
  return stop ? stopDisplayArrival(stop) : null;
}

/** When one leg is now expected to reach its own destination: a train's
 * reported arrival there, else its estimate, else the timetable
 * (`legArrivalInstant`); an untracked leg's planned arrival. What the
 * connector between two legs measures the change from, so a late-running
 * train shows a tight or missed connection. */
export function legExpectedArrivalInstant(leg: JourneyLegDetail): string | null {
  if (isUntrackedLeg(leg)) return leg.plannedArrival ?? null;
  const stop = legDestinationStop(leg);
  return stop ? (stop.actualArrival ?? stop.estimatedArrival ?? stopDisplayArrival(stop)) : null;
}

/** The stop a train leg gets off at, looked for after the leg's own origin
 * so a route that calls at the same station twice picks the right call. */
function legDestinationStop(leg: JourneyLegDetail): JourneyStop | null {
  const state = leg.trackedTrainState;
  const stops = state?.journeyStops ?? null;
  const destination = leg.destinationCrs ?? state?.scheduleDestinationCrs ?? null;
  if (state === null || stops === null || destination === null) return null;

  const origin = leg.originCrs ?? state.pinOriginCrs;
  const originIndex = origin === null ? -1 : stops.findIndex((stop) => sameCrs(stop.crs, origin));
  return stops.slice(originIndex + 1).find((candidate) => sameCrs(candidate.crs, destination)) ?? null;
}

/** When one leg is timetabled to leave its own origin, as an RFC3339
 * instant: an untracked leg's planned departure, or a matched train's
 * timetabled (public, else working) departure from the leg's own origin.
 * `null` when not known. */
export function legDepartureInstant(leg: JourneyLegDetail): string | null {
  if (isUntrackedLeg(leg)) return leg.plannedDeparture ?? null;
  const stop = legOriginStop(leg);
  return stop ? stopDisplayTime(stop) : null;
}

/** When one leg now leaves its own origin: a train's reported departure
 * there, else its estimate, else the timetable (`legDepartureInstant`); an
 * untracked leg's planned departure. The departure counterpart of
 * `legExpectedArrivalInstant`, so a late-running first train keeps a
 * journey upcoming until it actually goes. */
export function legExpectedDepartureInstant(leg: JourneyLegDetail): string | null {
  if (isUntrackedLeg(leg)) return leg.plannedDeparture ?? null;
  const stop = legOriginStop(leg);
  return stop ? (stop.actualDeparture ?? stop.estimatedDeparture ?? stopDisplayTime(stop)) : null;
}

function legOriginStop(leg: JourneyLegDetail): JourneyStop | null {
  const state = leg.trackedTrainState;
  const stops = state?.journeyStops ?? null;
  const origin = leg.originCrs ?? state?.pinOriginCrs ?? null;
  if (stops === null || origin === null) return null;
  return stops.find((candidate) => sameCrs(candidate.crs, origin)) ?? null;
}

/** Whole minutes between one leg's expected arrival
 * (`legExpectedArrivalInstant`, so a live delay counts) and the next leg's
 * timetabled departure, whatever kind each is (spec 2026-10-20 §2.3):
 * negative when the next leg leaves before this one arrives, `null` when
 * either time isn't known. No minimum interchange time is assumed. */
export function connectionGapMinutes(leg: JourneyLegDetail, next: JourneyLegDetail): number | null {
  const arrival = legExpectedArrivalInstant(leg);
  const departure = legDepartureInstant(next);
  if (arrival === null || departure === null) return null;
  const minutes = dayjs(departure).diff(dayjs(arrival), 'minute', true);
  return Number.isFinite(minutes) ? Math.round(minutes) : null;
}

/** Where and when the journey's FIRST leg starts: what "Add a leg at the
 * start" (inserting before it, spec 2026-10-20 §2.3) seeds a new leg's
 * end from. `null` for a journey with no legs. */
export function journeyFirstLegStart(
  journey: JourneyDetail,
): { legId: number; place: string | null; crs: string | null; departure: LondonDateTime | null } | null {
  const first = journey.legs[0];
  if (first === undefined) return null;
  const crs = first.originCrs ?? first.trackedTrainState?.pinOriginCrs ?? null;
  const departure = legDepartureInstant(first);
  return {
    legId: first.id,
    place: first.originName ?? first.trackedTrainState?.pinOriginName ?? crs,
    crs,
    departure: departure === null ? null : londonDateTime(departure),
  };
}

function londonDateTime(instant: string): LondonDateTime | null {
  const london = dayjs(instant).tz(LONDON_TZ);
  if (!london.isValid()) return null;
  return { date: london.format('YYYY-MM-DD'), time: london.format('HH:mm') };
}

function sameCrs(a: string | null, b: string): boolean {
  return a !== null && a.toUpperCase() === b.toUpperCase();
}
