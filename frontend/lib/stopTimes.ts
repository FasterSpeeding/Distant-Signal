import { formatTime } from './dateFormat';
import type { JourneyStop } from './types';

/** The time a passenger timetable shows for a stop: the PUBLIC departure,
 * else the public arrival (a set-down-only stop has no public departure),
 * falling back to the working-timetable `scheduled*` only when the API sent
 * no public time at all (a schedule stored before public times were, until
 * the next schedule publish). Departure-first, the same precedence the train
 * page has always used. See
 * docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md. */
export function stopDisplayTime(stop: JourneyStop): string | null {
  if (hasPublicTimes(stop)) return stop.publicDeparture ?? stop.publicArrival ?? null;
  return stop.scheduledDeparture ?? stop.scheduledArrival;
}

/** The public arrival at a stop (`null` at a pick-up-only stop, which has
 * none), falling back to the working one only when the API sent no public
 * time at all (see `stopDisplayTime`). */
export function stopDisplayArrival(stop: JourneyStop): string | null {
  return hasPublicTimes(stop) ? (stop.publicArrival ?? null) : stop.scheduledArrival;
}

function hasPublicTimes(stop: JourneyStop): boolean {
  return stop.publicArrival != null || stop.publicDeparture != null;
}

/** A passing point: the train runs through without stopping. Shown only in
 * the detailed working-timetable view, never as a stop. */
export function isPassingPoint(stop: JourneyStop): boolean {
  return stop.workingPass != null && stop.scheduledArrival === null && stop.scheduledDeparture === null;
}

/** The passenger-facing direction notes for a stop, in reading order:
 * "Set down only" (you may get off, not on), "Pick up only" (on, not off)
 * and "Request stop". Origin/terminus direction is implied by being the
 * end of the line, so only intermediate calls are labelled; a stop the API
 * says nothing about (an older response) gets no label. */
export function stopDirectionLabels(stop: JourneyStop): string[] {
  const labels: string[] = [];
  if (stop.kind === 'Intermediate' && !isPassingPoint(stop)) {
    if (stop.canBoard === false && stop.canAlight === true) labels.push('Set down only');
    if (stop.canAlight === false && stop.canBoard === true) labels.push('Pick up only');
  }
  if (stop.requestStop === true) labels.push('Request stop');
  return labels;
}

/** A working-timetable (WTT) time as staff timetables print it: `HH:MM`, plus
 * `½` when the time is on the half-minute (`:30` seconds on the wire). */
export function formatWorkingTime(value: string): string {
  const seconds = new Date(value).getUTCSeconds();
  return seconds >= 30 ? `${formatTime(value)}½` : formatTime(value);
}

/** The same, read aloud: "20:50 and a half". A screen reader would
 * otherwise say "20:50 one half", or skip the glyph. */
export function workingTimeAccessibleLabel(value: string): string {
  const seconds = new Date(value).getUTCSeconds();
  return seconds >= 30 ? `${formatTime(value)} and a half` : formatTime(value);
}
