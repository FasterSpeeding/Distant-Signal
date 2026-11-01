import { formatTime } from './dateFormat';
import { labelPlaceEnds, placeNamesAlike, RAIL_PLACE, stopPlaceKind, type PlaceKind } from './placeKinds';
import type { JourneyLegDetail, TransitMode, UntrackedLegFields, UntrackedStatus } from './types';

/** An untracked leg: a Tube ride, a bus, a walk, anything we have no live
 * or timetable data for (spec
 * docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
 * §2). Every place that reads `trackedTrainState === null` as "needs a
 * train picked" must ask this first. */
export function isUntrackedLeg(leg: Pick<UntrackedLegFields, 'kind'>): boolean {
  return leg.kind === 'untracked';
}

/** A train leg whose train hasn't been picked yet (an open window search). */
export function isOpenTrainLeg(leg: Pick<JourneyLegDetail, 'kind' | 'trackedTrainState'>): boolean {
  return !isUntrackedLeg(leg) && leg.trackedTrainState === null;
}

/** Every mode, in the order the form offers them, with its label. */
export const TRANSIT_MODES: readonly { value: TransitMode; label: string }[] = [
  { value: 'tube', label: 'Tube' },
  { value: 'dlr', label: 'DLR' },
  { value: 'bus', label: 'Bus' },
  { value: 'tram', label: 'Tram' },
  { value: 'metro', label: 'Metro' },
  { value: 'rail', label: 'Train (not tracked)' },
  { value: 'walk', label: 'Walk' },
  { value: 'transfer', label: 'Transfer' },
  { value: 'ferry', label: 'Ferry' },
  { value: 'taxi', label: 'Taxi' },
  { value: 'cycle', label: 'Cycle' },
  { value: 'other', label: 'Other' },
];

/** The modes whose places can be picked from TfL's stops
 * (`GET /tfl/stops`), with the stop modes each searches: its own, or for
 * `metro` (the planner's generic METRO transfer) DLR, tram and Tube. */
const TFL_STOP_SEARCH: Partial<Record<TransitMode, readonly string[]>> = {
  tube: ['tube'],
  dlr: ['dlr'],
  tram: ['tram'],
  metro: ['dlr', 'tram', 'tube'],
};

/** Whether `mode`'s places come from the TfL stop lookup. */
export function hasTflStops(mode: TransitMode): boolean {
  return TFL_STOP_SEARCH[mode] !== undefined;
}

/** Whether an untracked `mode` leg can carry `stopId` as a place's stop,
 * as `POST /Journeys/itinerary` checks it (`journeys::attach_untracked_stops`):
 * a trip-planner network's tram station (`9400ZZMAPIC`) on a tram or bus
 * leg, any other (TfL) stop on a leg whose places come from the TfL stop
 * lookup. A walk or transfer to a stop has none: the stop's name is its
 * place. */
export function canHaveStopId(mode: TransitMode, stopId: string): boolean {
  return stopId.startsWith('9400') ? mode === 'tram' || mode === 'bus' : hasTflStops(mode);
}

/** The TfL stop modes a `mode` leg's place fields search (none for a mode
 * without TfL stops). */
export function tflStopSearchModes(mode: TransitMode): readonly string[] {
  return TFL_STOP_SEARCH[mode] ?? [];
}

/** A mode's label; an unknown mode from a newer backend reads "Other". */
export function transitModeLabel(mode: TransitMode | null | undefined): string {
  return TRANSIT_MODES.find((entry) => entry.value === mode)?.label ?? 'Other';
}

/** The card heading: the mode, and the line when there is one ("Tube:
 * Victoria line", "Walk"). */
export function untrackedLegTitle(leg: Pick<UntrackedLegFields, 'transitMode' | 'lineName'>): string {
  const mode = transitModeLabel(leg.transitMode);
  return leg.lineName ? `${mode}: ${leg.lineName}` : mode;
}

/** The fields of a leg that say what kind of station its ends are. */
type PlaceLeg = Pick<JourneyLegDetail, 'originName' | 'destinationName'> &
  Partial<
    Pick<
      JourneyLegDetail,
      'kind' | 'originCrs' | 'destinationCrs' | 'transitMode' | 'originStopId' | 'destinationStopId'
    >
  >;

/** The legs either side of an untracked leg in its journey, which can say
 * what its ends are: a train leg arrives at, or leaves from, a National
 * Rail station; a Tube ride's end is a Tube stop. */
export interface UntrackedLegNeighbours {
  previous?: PlaceLeg | null | undefined;
  next?: PlaceLeg | null | undefined;
}

/** The modes whose places are TfL stops of that mode. */
const STOP_MODE_OF: Partial<Record<string, string>> = { tube: 'tube', dlr: 'dlr', tram: 'tram' };

function rideKind(mode: string | null | undefined): PlaceKind | undefined {
  const stopMode = STOP_MODE_OF[mode ?? ''];
  return stopMode ? { kind: 'stop', modes: [stopMode] } : undefined;
}

/** What kind of station `neighbour`'s `end` is, if it says. */
function neighbourEndKind(
  neighbour: PlaceLeg | null | undefined,
  end: 'origin' | 'destination',
): PlaceKind | undefined {
  if (!neighbour) return undefined;
  const crs = end === 'origin' ? neighbour.originCrs : neighbour.destinationCrs;
  if (!isUntrackedLeg(neighbour)) return crs ? RAIL_PLACE : undefined;
  const stopId = end === 'origin' ? neighbour.originStopId : neighbour.destinationStopId;
  return stopPlaceKind({ id: stopId ?? null }) ?? rideKind(neighbour.transitMode);
}

/** An untracked leg's place names, each end saying what kind of station it
 * is when the two would otherwise read alike (DS-239): a leg saved before
 * the planner named them, "Vauxhall to Vauxhall", reads "Vauxhall
 * (National Rail) to Vauxhall (Tube)". Worked out when shown, never
 * stored: from each end's stop id (or a Tube ride's own mode), else the
 * leg either side of it (the train before a walk arrives at the National
 * Rail station, the Tube ride after it leaves from the Tube station). An
 * end nothing tells is as saved, as are names that differ. */
export function untrackedLegPlaceNames(
  leg: PlaceLeg,
  neighbours: UntrackedLegNeighbours = {},
): { originName: string | null; destinationName: string | null } {
  const originName = leg.originName ?? null;
  const destinationName = leg.destinationName ?? null;
  if (!originName || !destinationName || !placeNamesAlike(originName, destinationName)) {
    return { originName, destinationName };
  }
  const ownKind = (id: string | null | undefined): PlaceKind | undefined =>
    stopPlaceKind({ id: id ?? null }) ?? rideKind(leg.transitMode);
  const [origin, destination] = labelPlaceEnds(
    { name: originName, kind: ownKind(leg.originStopId) ?? neighbourEndKind(neighbours.previous, 'destination') },
    { name: destinationName, kind: ownKind(leg.destinationStopId) ?? neighbourEndKind(neighbours.next, 'origin') },
  );
  return { originName: origin, destinationName: destination };
}

/** "Euston to Victoria", one end, or `null` with neither. Free text, so
 * never a station code. Names that read alike say what each end is where
 * the leg or `neighbours` tell ([`untrackedLegPlaceNames`]). */
export function untrackedLegPlaces(leg: PlaceLeg, neighbours: UntrackedLegNeighbours = {}): string | null {
  const { originName, destinationName } = untrackedLegPlaceNames(leg, neighbours);
  if (originName && destinationName) return `${originName} to ${destinationName}`;
  if (originName) return `From ${originName}`;
  if (destinationName) return `To ${destinationName}`;
  return null;
}

/** "08:05 to 08:20" in UK time, or `null` without both times. */
export function untrackedLegTimes(leg: Pick<UntrackedLegFields, 'plannedDeparture' | 'plannedArrival'>): string | null {
  if (!leg.plannedDeparture || !leg.plannedArrival) return null;
  return `${formatTime(leg.plannedDeparture)} to ${formatTime(leg.plannedArrival)}`;
}

/** Each clock-only state's words (spec §2.2). */
export const UNTRACKED_STATUS_LABEL: Record<UntrackedStatus, string> = {
  planned: 'Planned',
  inProgress: 'In progress (not live tracked)',
  done: 'Done',
};
