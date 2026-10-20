import { formatTime } from './dateFormat';
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
  { value: 'bus', label: 'Bus' },
  { value: 'tram', label: 'Tram' },
  { value: 'rail', label: 'Train (not tracked)' },
  { value: 'walk', label: 'Walk' },
  { value: 'ferry', label: 'Ferry' },
  { value: 'taxi', label: 'Taxi' },
  { value: 'cycle', label: 'Cycle' },
  { value: 'other', label: 'Other' },
];

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

/** "Euston to Victoria", one end, or `null` with neither. Free text, so
 * never a station code. */
export function untrackedLegPlaces(leg: Pick<JourneyLegDetail, 'originName' | 'destinationName'>): string | null {
  if (leg.originName && leg.destinationName) return `${leg.originName} to ${leg.destinationName}`;
  if (leg.originName) return `From ${leg.originName}`;
  if (leg.destinationName) return `To ${leg.destinationName}`;
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
