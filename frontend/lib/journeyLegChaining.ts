import dayjs from 'dayjs';
import { LONDON_TZ } from './londonWallClock';
import { stopDisplayArrival } from './stopTimes';
import type { JourneyDetail, JourneyLegDetail } from './types';

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
 * leg that might still resolve to nothing. `false` for a journey with no
 * legs at all (shouldn't occur in practice, but this mirrors the same
 * defensive posture `defaultJourneyTitle`/`trackAgainPrefill` already take
 * for the identical edge case rather than assuming the invariant holds). */
export function journeyCanAddLeg(journey: JourneyDetail): boolean {
  const lastLeg = journey.legs.at(-1) ?? null;
  return lastLeg !== null && lastLeg.trackedTrainState !== null;
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
 * RFC3339 instant, whatever kind of leg it is. Today every leg is a train
 * leg, read from its matched train's stop list: `null` while it has no
 * matched train, no stop list, no stop for the destination, or no arrival
 * time at it. A leg kind with its own planned arrival adds its branch here,
 * and `journeyPriorArrival` picks it up unchanged. */
export function legArrivalInstant(leg: JourneyLegDetail): string | null {
  const state = leg.trackedTrainState;
  const stops = state?.journeyStops ?? null;
  const destination = leg.destinationCrs ?? state?.scheduleDestinationCrs ?? null;
  if (state === null || stops === null || destination === null) return null;

  const origin = leg.originCrs ?? state.pinOriginCrs;
  const originIndex = origin === null ? -1 : stops.findIndex((stop) => sameCrs(stop.crs, origin));
  const stop = stops.slice(originIndex + 1).find((candidate) => sameCrs(candidate.crs, destination));
  return stop ? stopDisplayArrival(stop) : null;
}

function londonDateTime(instant: string): LondonDateTime | null {
  const london = dayjs(instant).tz(LONDON_TZ);
  if (!london.isValid()) return null;
  return { date: london.format('YYYY-MM-DD'), time: london.format('HH:mm') };
}

function sameCrs(a: string | null, b: string): boolean {
  return a !== null && a.toUpperCase() === b.toUpperCase();
}
