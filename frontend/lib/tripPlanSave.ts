import dayjs from 'dayjs';
import { LONDON_TZ } from './londonWallClock';
import { isTiplocCode } from './stationLabel';
import type { TransitMode, TripPlanItinerary, TripPlanLeg } from './types';

type TrainLeg = Extract<TripPlanLeg, { kind: 'train' }>;
type TransferLeg = Extract<TripPlanLeg, { kind: 'transfer' }>;

/** One leg of a `POST /Journeys/itinerary` body: a train leg, as before,
 * or (spec 2026-10-20 §2, follow-up) an untracked leg for a planner
 * transfer such as a cross-London Tube ride. */
export type ItineraryLegBody =
  | { trainUid: string; serviceDate: string; originCrs?: string; destinationCrs?: string }
  | {
      kind: 'untracked';
      transitMode: TransitMode;
      originName?: string;
      destinationName?: string;
      originStopId?: string;
      destinationStopId?: string;
      plannedDeparture: string;
      plannedArrival: string;
    };

/** What an untracked leg takes from a transfer: its mode, place names
 * and, for a Tube or metro transfer, the TfL stops at each end. */
export interface TransferAsUntracked {
  transitMode: TransitMode;
  originName?: string;
  destinationName?: string;
  originStopId?: string;
  destinationStopId?: string;
}

const TRANSFER_MODES: Record<string, TransitMode> = {
  WALK: 'walk',
  TUBE: 'tube',
  METRO: 'metro',
  BUS: 'bus',
  FERRY: 'ferry',
};

/** The ONE place a planner transfer becomes an untracked leg's mode and
 * place names: the fixed-link mode (WALK, TUBE, ...) as a journey mode, and
 * each end's station name (else its code). Where `/Trips/plan` names the
 * TfL stop at an end (`originStop`/`destinationStop`, on TUBE and METRO
 * transfers; docs/superpowers/specs/2026-10-20-tfl-stop-lookup-design.md
 * §5), that end is the stop: its name and its id. `null` for a change
 * within one station (`TRANSFER`), which isn't a journey of its own. */
export function transferAsUntracked(
  leg: TransferLeg,
  nameFor: (code: string) => string | undefined,
): TransferAsUntracked | null {
  const transitMode = TRANSFER_MODES[leg.mode.toUpperCase()];
  if (transitMode === undefined) return null;
  const place = (code: string | null) => {
    if (!code || isTiplocCode(code)) return undefined;
    return nameFor(code) ?? code;
  };
  const originName = leg.originStop?.name ?? place(leg.originCrs);
  const destinationName = leg.destinationStop?.name ?? place(leg.destinationCrs);
  const originStopId = leg.originStop?.id;
  const destinationStopId = leg.destinationStop?.id;
  return {
    transitMode,
    ...(originName ? { originName } : {}),
    ...(destinationName ? { destinationName } : {}),
    ...(originStopId ? { originStopId } : {}),
    ...(destinationStopId ? { destinationStopId } : {}),
  };
}

/** A train leg's departure from its boarding call or arrival at its
 * alighting call, as a UTC instant: the PUBLIC time the planner planned on
 * (`schedule_query::Connection`'s times are public, falling back to
 * working), with its own day offset, falling back to the working time and
 * its offset only when the leg has no public time. On its service date. */
function trainInstant(leg: TrainLeg, end: 'departure' | 'arrival'): dayjs.Dayjs | null {
  const publicTime = end === 'departure' ? leg.publicDeparture : leg.publicArrival;
  const publicOffset = end === 'departure' ? leg.publicDepartureDayOffset : leg.publicArrivalDayOffset;
  const [time, offset] = publicTime
    ? [publicTime, publicOffset ?? 0]
    : end === 'departure'
      ? [leg.scheduledDeparture, leg.departureDayOffset ?? 0]
      : [leg.scheduledArrival, leg.arrivalDayOffset];
  const instant = dayjs.tz(`${leg.serviceDate} ${time}`, LONDON_TZ).add(offset, 'day');
  return instant.isValid() ? instant : null;
}

/** Every leg of the chosen itineraries to save, in order: the train legs
 * `trainLegsForTracking` (lib/tripPlan.ts) would pick (a train ridden
 * through a waypoint stays one leg), with each Tube, walk, bus or ferry transfer between them as an
 * untracked leg. A transfer starts when the train before it arrives (the
 * planner walks straight off it; public time, else working) and lasts its
 * minutes; one with no train
 * before it ends when the next train leaves. A transfer with neither, or a
 * change within a station, is left out. */
export function itineraryLegsForSaving(
  itineraries: TripPlanItinerary[],
  nameFor: (code: string) => string | undefined,
): ItineraryLegBody[] {
  // The same merge as `trainLegsForTracking`: a segment that rides on in
  // the previous segment's train extends that leg instead of adding one.
  const sequence: (TrainLeg | TransferLeg)[] = [];
  for (const itinerary of itineraries) {
    itinerary.legs.forEach((leg, index) => {
      const previous = sequence.at(-1);
      if (
        leg.kind === 'train' &&
        index === 0 &&
        itinerary.continuesPreviousTrain &&
        previous?.kind === 'train' &&
        previous.trainUid === leg.trainUid &&
        previous.serviceDate === leg.serviceDate
      ) {
        sequence[sequence.length - 1] = {
          ...previous,
          destinationCrs: leg.destinationCrs,
          scheduledArrival: leg.scheduledArrival,
          arrivalDayOffset: leg.arrivalDayOffset,
          bookedArrivalPlatform: leg.bookedArrivalPlatform,
        };
        return;
      }
      sequence.push(leg);
    });
  }

  const legs: ItineraryLegBody[] = [];
  sequence.forEach((leg, index) => {
    if (leg.kind === 'train') {
      legs.push({
        trainUid: leg.trainUid,
        serviceDate: leg.serviceDate,
        ...(leg.originCrs && !isTiplocCode(leg.originCrs) ? { originCrs: leg.originCrs } : {}),
        ...(leg.destinationCrs && !isTiplocCode(leg.destinationCrs) ? { destinationCrs: leg.destinationCrs } : {}),
      });
      return;
    }
    const untracked = transferAsUntracked(leg, nameFor);
    if (untracked === null) return;
    const before = sequence
      .slice(0, index)
      .reverse()
      .find((l): l is TrainLeg => l.kind === 'train');
    const after = sequence.slice(index + 1).find((l): l is TrainLeg => l.kind === 'train');
    const minutes = Math.max(leg.minutes, 0);
    let departure: dayjs.Dayjs | null = null;
    if (before) departure = trainInstant(before, 'arrival');
    else if (after) departure = trainInstant(after, 'departure')?.subtract(minutes, 'minute') ?? null;
    if (departure === null) return;
    legs.push({
      kind: 'untracked',
      ...untracked,
      plannedDeparture: departure.toISOString(),
      plannedArrival: departure.add(minutes, 'minute').toISOString(),
    });
  });
  return legs;
}
