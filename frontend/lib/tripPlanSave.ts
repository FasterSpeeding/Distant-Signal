import dayjs from 'dayjs';
import { LONDON_TZ } from './londonWallClock';
import { transferLineName } from './displayLabels';
import { isTiplocCode } from './stationLabel';
import type { TransitMode, TripPlanItinerary, TripPlanLeg, TripPlanTransitLeg } from './types';

type TrainLeg = Extract<TripPlanLeg, { kind: 'train' }>;
type TransferLeg = Extract<TripPlanLeg, { kind: 'transfer' }>;
/** A leg with its own times: a train, or a tram (or its replacement bus). */
type TimedLeg = TrainLeg | TripPlanTransitLeg;

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
      lineName?: string;
      originStopId?: string;
      destinationStopId?: string;
      plannedDeparture: string;
      plannedArrival: string;
    };

/** What an untracked leg takes from a transfer: its mode, place names,
 * line where the planner knows it and, for a Tube or metro transfer, the
 * TfL stops at each end. */
export interface TransferAsUntracked {
  transitMode: TransitMode;
  originName?: string;
  destinationName?: string;
  lineName?: string;
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

/** A `TRANSFER` link's mode: `transfer` when its two ends are different
 * stations, `undefined` for a change within one station (both ends the
 * same code, or neither known). */
function crossStationTransferMode(leg: TransferLeg): TransitMode | undefined {
  if (leg.mode.toUpperCase() !== 'TRANSFER') return undefined;
  const origin = leg.originCrs?.toUpperCase() ?? null;
  const destination = leg.destinationCrs?.toUpperCase() ?? null;
  return origin === destination ? undefined : 'transfer';
}

/** The ONE place a planner transfer becomes an untracked leg's mode and
 * place names: the fixed-link mode (WALK, TUBE, ...) as a journey mode, and
 * each end's station name (else its code). Where `/Trips/plan` names the
 * TfL stop at an end (`originStop`/`destinationStop`, on TUBE and METRO
 * transfers; docs/superpowers/specs/2026-10-20-tfl-stop-lookup-design.md
 * §5), that end is the stop: its name and its id. The transfer's `line`
 * (2026-10-21) is the leg's line name, as people write it ("Victoria
 * line"). A `TRANSFER` link between two different stations (Euston to
 * Victoria, 19 minutes: often a Tube hop the link doesn't say is one) is a
 * `transfer` leg; `null` for a `TRANSFER` within one station (the same
 * code at both ends, its minutes only the interchange time), which isn't a
 * journey of its own. */
export function transferAsUntracked(
  leg: TransferLeg,
  nameFor: (code: string) => string | undefined,
): TransferAsUntracked | null {
  const transitMode = TRANSFER_MODES[leg.mode.toUpperCase()] ?? crossStationTransferMode(leg);
  if (transitMode === undefined) return null;
  const place = (code: string | null) => {
    if (!code || isTiplocCode(code)) return undefined;
    return nameFor(code) ?? code;
  };
  // A walk to or from a tram stop (transit trip planning §8.1) names the
  // stop at its tram end, which has no station code.
  const walk = leg.transitWalk;
  const originTram = walk && !walk.towardsStop ? walk.stop : undefined;
  const destinationTram = walk?.towardsStop ? walk.stop : undefined;
  const originName = leg.originStop?.name ?? originTram?.name ?? place(leg.originCrs);
  const destinationName = leg.destinationStop?.name ?? destinationTram?.name ?? place(leg.destinationCrs);
  const originStopId = leg.originStop?.id ?? originTram?.id;
  const destinationStopId = leg.destinationStop?.id ?? destinationTram?.id;
  const lineName = transferLineName(leg.mode, leg.line);
  return {
    transitMode,
    ...(originName ? { originName } : {}),
    ...(destinationName ? { destinationName } : {}),
    ...(lineName ? { lineName } : {}),
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

/** A transit leg's departure or arrival as a UTC instant: its own
 * scheduled time, with its day offset, on its service date. */
function transitInstant(leg: TripPlanTransitLeg, end: 'departure' | 'arrival'): dayjs.Dayjs | null {
  const [time, offset] =
    end === 'departure'
      ? [leg.scheduledDeparture, leg.departureDayOffset]
      : [leg.scheduledArrival, leg.arrivalDayOffset];
  const instant = dayjs.tz(`${leg.serviceDate} ${time}`, LONDON_TZ).add(offset, 'day');
  return instant.isValid() ? instant : null;
}

function timedInstant(leg: TimedLeg, end: 'departure' | 'arrival'): dayjs.Dayjs | null {
  return leg.kind === 'train' ? trainInstant(leg, end) : transitInstant(leg, end);
}

/** A tram ride (transit trip planning §11.2) as an untracked leg: a tram,
 * or a bus for a replacement bus, its line, its stations by name and id,
 * and its own scheduled times. */
function transitAsUntracked(leg: TripPlanTransitLeg): ItineraryLegBody | null {
  const departure = transitInstant(leg, 'departure');
  const arrival = transitInstant(leg, 'arrival');
  if (departure === null || arrival === null) return null;
  const lineName = leg.line?.name;
  return {
    kind: 'untracked',
    transitMode: leg.serviceMode === 'replacementBus' ? 'bus' : 'tram',
    originName: leg.originStop.name,
    destinationName: leg.destinationStop.name,
    ...(lineName ? { lineName } : {}),
    originStopId: leg.originStop.id,
    destinationStopId: leg.destinationStop.id,
    plannedDeparture: departure.toISOString(),
    plannedArrival: arrival.toISOString(),
  };
}

/** Every leg of the chosen itineraries to save, in order: the train legs
 * `trainLegsForTracking` (lib/tripPlan.ts) would pick (a train ridden
 * through a waypoint stays one leg), with each Tube, walk, bus or ferry transfer between them as an
 * untracked leg. A transfer starts when the train before it arrives (the
 * planner walks straight off it; public time, else working), or when the
 * transfer before it ends, and lasts its minutes; one with no train
 * before it ends when the next train leaves (less any transfers after it). A transfer with neither, or a
 * change within a station, is left out. A tram ride is an untracked leg
 * with its own scheduled times, and times the transfers next to it as a
 * train does. */
export function itineraryLegsForSaving(
  itineraries: TripPlanItinerary[],
  nameFor: (code: string) => string | undefined,
): ItineraryLegBody[] {
  // The same merge as `trainLegsForTracking`: a segment that rides on in
  // the previous segment's train extends that leg instead of adding one.
  const sequence: TripPlanLeg[] = [];
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
    if (leg.kind === 'transit') {
      const untracked = transitAsUntracked(leg);
      if (untracked !== null) legs.push(untracked);
      return;
    }
    const untracked = transferAsUntracked(leg, nameFor);
    if (untracked === null) return;
    const timed = (l: TripPlanLeg) => l.kind !== 'transfer';
    const beforeIndex = sequence.findLastIndex((l, i) => i < index && timed(l));
    const afterIndex = sequence.findIndex((l, i) => i > index && timed(l));
    const before = beforeIndex >= 0 ? (sequence[beforeIndex] as TimedLeg) : undefined;
    const after = afterIndex >= 0 ? (sequence[afterIndex] as TimedLeg) : undefined;
    const minutes = Math.max(leg.minutes, 0);
    // Transfers in a row (the Northern line, then the Victoria line) follow
    // one another: each starts when the one before it ends.
    const transferMinutes = (from: number, to: number) =>
      sequence.slice(from, to).reduce((sum, l) => sum + (l.kind === 'transfer' ? Math.max(l.minutes, 0) : 0), 0);
    let departure: dayjs.Dayjs | null = null;
    if (before)
      departure = timedInstant(before, 'arrival')?.add(transferMinutes(beforeIndex + 1, index), 'minute') ?? null;
    else if (after)
      departure = timedInstant(after, 'departure')?.subtract(transferMinutes(index, afterIndex), 'minute') ?? null;
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
