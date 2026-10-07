/**
 * The one status vocabulary every service list and badge in the app uses
 * (user decision, 2026-10-07):
 *
 * - "Cancelled";
 * - "12 min late", or "Exp. 12 min late" for a provisional (predicted,
 *   not yet observed) delay;
 * - "3 min early" -- early running is said, never folded into "On time";
 * - "On time";
 * - "Timetable only" for a bus or ferry, which is never reported live;
 * - "Scheduled" when nothing live is known yet.
 *
 * Callers keep their own layout (a coloured text, a light badge, a run-in
 * note); only the words and the tone come from here.
 */
import type { ServiceMode } from './types';

export const CANCELLED_LABEL = 'Cancelled';
export const ON_TIME_LABEL = 'On time';
export const SCHEDULED_LABEL = 'Scheduled';
export const TIMETABLE_ONLY_LABEL = 'Timetable only';

export type DelayTone = 'late' | 'early' | 'onTime';
export type LiveTone = 'cancelled' | DelayTone | 'timetable' | 'none';

/** "12 min late", "Exp. 12 min late", "3 min early" or "On time" for a
 * delay in whole minutes (negative is early). */
export function delayLabel(delayMinutes: number, provisional = false): { text: string; tone: DelayTone } {
  const prefix = provisional ? 'Exp. ' : '';
  if (delayMinutes > 0) return { text: `${prefix}${delayMinutes} min late`, tone: 'late' };
  if (delayMinutes < 0) return { text: `${prefix}${-delayMinutes} min early`, tone: 'early' };
  return { text: ON_TIME_LABEL, tone: 'onTime' };
}

/** The compact live state a service row carries: the line summary's
 * `live`, and the same object on `/public/trains/search` and
 * `/schedule-departures` rows. */
export interface ServiceLiveState {
  status: string | null;
  delayMinutes: number | null;
  delayProvisional: boolean;
  cancelled: boolean;
}

/** A row's status text and tone: cancelled, timetable only (a bus or a
 * ferry), late, early, on time, or "Scheduled" when nothing live is known
 * yet. */
export function liveStatusLabel(
  live: ServiceLiveState | null | undefined,
  serviceMode: ServiceMode | null | undefined,
): { text: string; tone: LiveTone } {
  if (live && (live.cancelled || live.status === 'cancelled')) return { text: CANCELLED_LABEL, tone: 'cancelled' };
  if (serviceMode !== undefined && serviceMode !== null && serviceMode !== 'train')
    return { text: TIMETABLE_ONLY_LABEL, tone: 'timetable' };
  if (live && live.delayMinutes !== null) return delayLabel(live.delayMinutes, live.delayProvisional);
  return { text: SCHEDULED_LABEL, tone: 'none' };
}

/** A day offset as the short marker shown after a time ("+1"), and the
 * words a screen reader hears instead ("next day"); `null` for the same
 * day. */
export function dayOffsetMarker(dayOffset: number | null | undefined): { short: string; spoken: string } | null {
  if (dayOffset === null || dayOffset === undefined || dayOffset <= 0) return null;
  return { short: `+${dayOffset}`, spoken: dayOffset === 1 ? 'next day' : `${dayOffset} days later` };
}
