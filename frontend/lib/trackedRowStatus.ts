import {
  journeyDetailSummary,
  journeyListItemSummary,
  summariseJourney,
  trackedTrainListStatusGroup,
  type JourneySummary,
  type JourneySummaryGroup,
} from './journeyStatus';
import { isTimetableOnly } from './serviceMode';
import type { JourneyLegDetail, JourneyListItem, ServiceMode, ServiceModeFields, TrackingPhaseFields } from './types';

/** What a journey's or a tracked train's status badge says, wherever it is
 * shown (the Track list, the journey page header, the home page). A
 * `JourneySummaryGroup` reads as `JourneyStatusGroupBadge` words it; the
 * others say how a finished row ended, or that it is a bus or ferry:
 *  - `completed`: arrived (every leg, for a journey);
 *  - `noReport`: its day has passed and Network Rail never reported it;
 *  - `noTrainPicked`: its day has passed and no train was ever picked;
 *  - `timetableOnly`: a bus or ferry, which Network Rail never reports. */
export type TrackedRowStatusKind = JourneySummaryGroup | 'completed' | 'noReport' | 'noTrainPicked' | 'timetableOnly';

export interface TrackedRowStatus {
  kind: TrackedRowStatusKind;
  /** Arrived, done, or its day is over. Finished rows sit under their own
   * heading; a cancelled one keeps its red badge there. */
  finished: boolean;
  /** The delay at the final destination (a lone train's: at the
   * traveller's stop), for the delay badge. */
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

/** Problems that stay on a finished row: it didn't simply arrive. */
const STILL_FLAGGED: ReadonlySet<JourneySummaryGroup> = new Set(['severe', 'partCancelled', 'noTimetable', 'skipped']);

/** A journey summary as a badge. A finished journey says "Completed"
 * unless something still flags it; one from a past day that never
 * finished stops asking for anything ("No report received", "No train
 * picked") but keeps a problem it had. */
export function journeySummaryRowStatus(summary: JourneySummary, pastDay: boolean): TrackedRowStatus {
  const { group } = summary;
  let kind: TrackedRowStatusKind = group;
  const finished = pastDay || summary.phase === 'completed';
  if (summary.phase === 'completed' && !STILL_FLAGGED.has(group)) {
    kind = 'completed';
  } else if (pastDay) {
    if (group === 'awaiting' || group === 'planned') kind = 'noReport';
    if (group === 'unmatched') kind = 'noTrainPicked';
    if (group === 'untrackedPlanned' || group === 'untrackedInProgress' || group === 'untrackedDone') {
      kind = 'completed';
    }
  }
  return {
    kind,
    finished,
    delayMinutes: summary.finalArrivalDelayMinutes,
    delayProvisional: summary.finalArrivalDelayProvisional,
    serviceMode: null,
  };
}

/** A train tracked on its own, read as a one-leg journey by the same rule
 * (`summariseJourney`), so it and a journey on that train say the same.
 * `today` is the London day ("2026-10-21", `londonDayKey`). */
export function trackedTrainRowStatus(train: TrackedTrainRowInput, today: string): TrackedRowStatus {
  const pastDay = train.serviceDate < today;
  if (isTimetableOnly(train)) {
    return {
      kind: 'timetableOnly',
      finished: pastDay,
      delayMinutes: train.delayMinutes,
      delayProvisional: train.delayProvisional ?? false,
      serviceMode: train.serviceMode ?? null,
    };
  }
  const cancelled = train.status === 'cancelled';
  const summary = summariseJourney({
    currentLegGroup: trackedTrainListStatusGroup(train),
    otherLegGroups: [],
    legCount: 1,
    trainLegCount: 1,
    cancelledLegCount: cancelled ? 1 : 0,
    unmatchedLegCount: 0,
    finishedLegCount: train.status === 'completed' ? 1 : 0,
    startedLegCount: train.status === 'en_route' || train.status === 'completed' ? 1 : 0,
    finalArrivalDelayMinutes: cancelled ? null : train.delayMinutes,
    finalArrivalDelayProvisional: train.delayProvisional ?? false,
    connection: null,
  });
  return journeySummaryRowStatus(summary, pastDay);
}

/** One row of `GET /Journeys/mine`, by its whole-journey summary
 * (`journeyListItemSummary`). Past means the current leg's day is over. */
export function journeyRowStatus(journey: JourneyListItem, today: string): TrackedRowStatus {
  return journeySummaryRowStatus(journeyListItemSummary(journey), journey.serviceDate < today);
}

/** The journey page header's status, from every leg. `null` with no legs. */
export function journeyDetailRowStatus(legs: readonly JourneyLegDetail[]): TrackedRowStatus | null {
  const summary = journeyDetailSummary(legs);
  return summary === null ? null : journeySummaryRowStatus(summary, false);
}
