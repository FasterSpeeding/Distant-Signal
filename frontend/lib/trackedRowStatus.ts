import { journeyListItemStatusGroup, trackedTrainListStatusGroup, type LegStatusGroup } from './journeyStatus';
import { isTimetableOnly } from './serviceMode';
import type { JourneyListItem, ServiceMode, ServiceModeFields, TrackingPhaseFields } from './types';

/** What one `/track/mine` row says about its status, for a train tracked on
 * its own and a journey alike. A `LegStatusGroup` reads exactly as
 * `JourneyStatusGroupBadge` words it; the other four exist only once a row
 * is finished, or for a bus or ferry:
 *  - `completed`: the train (or the journey's last leg) arrived;
 *  - `noReport`: its day has passed and Network Rail never reported it;
 *  - `noTrainPicked`: its day has passed and no train was ever picked;
 *  - `legCancelled`: a multi-leg journey's current leg is cancelled. The
 *    list row only carries the current leg, so it can't tell one cancelled
 *    leg from all of them; "Leg cancelled" is true either way;
 *  - `timetableOnly`: a bus or ferry, which Network Rail never reports. */
export type TrackedRowStatusKind =
  LegStatusGroup | 'completed' | 'noReport' | 'noTrainPicked' | 'legCancelled' | 'timetableOnly';

export interface TrackedRowStatus {
  kind: TrackedRowStatusKind;
  /** Arrived, done, or its day is over. Finished rows sit under their own
   * heading; a cancelled one keeps its red badge there. */
  finished: boolean;
  /** The delay at the passenger's own stop, for the delay badge. */
  delayMinutes: number | null;
  delayProvisional: boolean;
  /** For `timetableOnly`: which kind of service it is. */
  serviceMode: ServiceMode | null;
}

/** The fields a tracked-train row needs: `TrackedTrainListItem`,
 * `GroupTrain` and `SharedGroupTrain` all satisfy it. */
export type TrackedTrainRowInput = {
  serviceDate: string;
  resolutionStatus: string;
  status: string | null;
  delayMinutes: number | null;
  delayProvisional?: boolean;
} & ServiceModeFields &
  TrackingPhaseFields;

/** Shared tail of both mappings: a row from a past day that never
 * completed keeps any problem it had (cancelled, not in the timetable, not
 * stopping) and stops asking for anything else. */
function finish(group: LegStatusGroup, status: string | null, pastDay: boolean, multiLeg: boolean) {
  const cancelled = group === 'severe';
  let kind: TrackedRowStatusKind = cancelled && multiLeg ? 'legCancelled' : group;
  let finished = pastDay || group === 'untrackedDone';
  if (status === 'completed' && !cancelled) {
    kind = 'completed';
    finished = true;
  } else if (pastDay) {
    if (group === 'awaiting' || group === 'planned') kind = 'noReport';
    if (group === 'unmatched') kind = 'noTrainPicked';
    if (group === 'untrackedPlanned' || group === 'untrackedInProgress') kind = 'untrackedDone';
  }
  return { kind, finished };
}

/** A train tracked on its own. `today` is the London day ("2026-10-21",
 * `londonDayKey`). */
export function trackedTrainRowStatus(train: TrackedTrainRowInput, today: string): TrackedRowStatus {
  const pastDay = train.serviceDate < today;
  const base = {
    delayMinutes: train.delayMinutes,
    delayProvisional: train.delayProvisional ?? false,
    serviceMode: train.serviceMode ?? null,
  };
  if (isTimetableOnly(train)) {
    return { ...base, kind: 'timetableOnly', finished: pastDay };
  }
  return { ...base, ...finish(trackedTrainListStatusGroup(train), train.status, pastDay, false) };
}

/** One row of `GET /Journeys/mine`. The row describes the journey's
 * current leg (the earliest one not yet completed, or the last when all
 * are), so a `completed` status means the whole journey is. */
export function journeyRowStatus(journey: JourneyListItem, today: string): TrackedRowStatus {
  const pastDay = journey.serviceDate < today;
  const multiLeg = (journey.legCount ?? 1) > 1;
  return {
    delayMinutes: journey.delayMinutes,
    delayProvisional: journey.delayProvisional ?? false,
    serviceMode: null,
    ...finish(journeyListItemStatusGroup(journey), journey.status, pastDay, multiLeg),
  };
}
