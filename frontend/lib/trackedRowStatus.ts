import {
  journeyDetailSummary,
  journeyListItemSummary,
  summariseJourney,
  trackedTrainListStatusGroup,
  type JourneySummary,
  type JourneySummaryGroup,
  type LegStatusGroup,
} from './journeyStatus';
import { isTimetableOnly } from './serviceMode';
import { isUntrackedLeg } from './untrackedLeg';
import type {
  GroupJourney,
  JourneyLegDetail,
  JourneyListItem,
  ListCancellation,
  ServiceMode,
  ServiceModeFields,
  TrackingPhaseFields,
  UntrackedStatus,
} from './types';

/** What a journey's or a tracked train's status badge says, wherever it is
 * shown (the Track list, the journey page header, the home page). A
 * `JourneySummaryGroup` reads as `JourneyStatusGroupBadge` words it; the
 * others say how a finished row ended, or that it is a bus or ferry:
 *  - `completed`: arrived (every leg, for a journey);
 *  - `noReport`: its day has passed and it never arrived (no report, or
 *    reports that stopped short of the arrival);
 *  - `noTrainPicked`: its day has passed and no train was ever picked;
 *  - `noTrainPickedYet`: a train leg still has no train, on a journey
 *    shared with the viewer that they don't follow, so picking one is not
 *    theirs to do (`viewerCanPickTrain`);
 *  - `timetableOnly`: a bus or ferry, which Network Rail never reports. */
export type TrackedRowStatusKind =
  JourneySummaryGroup | 'completed' | 'noReport' | 'noTrainPicked' | 'noTrainPickedYet' | 'timetableOnly';

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
  /** For `severe` ("Cancelled") and `partCancelled`: the row's own train's
   * cancellation (DS-181), when it is the whole story (a lone train, or a
   * journey with one train leg), so the badge can say "Terminates at
   * York" and give the reason. Absent or `null` otherwise. */
  cancellation?: ListCancellation | null;
}

/** The fields a tracked-train row needs: `TrackedTrainListItem`,
 * `GroupTrain` and `SharedGroupTrain` all satisfy it. */
export type TrackedTrainRowInput = {
  serviceDate: string;
  resolutionStatus: string;
  status: string | null;
  delayMinutes: number | null;
  delayProvisional?: boolean;
  cancellation?: ListCancellation | null;
} & ServiceModeFields &
  TrackingPhaseFields;

/** Problems that stay on a finished row: it didn't simply arrive. */
const STILL_FLAGGED: ReadonlySet<JourneySummaryGroup> = new Set(['severe', 'partCancelled', 'noTimetable', 'skipped']);

/** A journey summary as a badge. A finished journey says "Completed"
 * unless something still flags it; one from a past day that never
 * finished stops asking for anything ("No report received", "No train
 * picked") but keeps a problem it had. */
export function journeySummaryRowStatus(
  summary: JourneySummary,
  pastDay: boolean,
  cancellation: ListCancellation | null = null,
): TrackedRowStatus {
  const { group } = summary;
  let kind: TrackedRowStatusKind = group;
  const finished = pastDay || summary.phase === 'completed';
  if (summary.phase === 'completed' && !STILL_FLAGGED.has(group)) {
    kind = 'completed';
  } else if (pastDay) {
    // Its day is over and it never arrived: whatever we last heard (a
    // forecast delay, "On time", "Awaiting") is stale, so say no report
    // came rather than show it.
    if (group === 'awaiting' || group === 'planned' || group === 'good' || group === 'delayed') kind = 'noReport';
    if (group === 'unmatched') kind = 'noTrainPicked';
    if (group === 'untrackedPlanned' || group === 'untrackedInProgress' || group === 'untrackedDone') {
      kind = 'completed';
    }
  }
  // A forecast for a row that will never arrive is not shown.
  const staleForecast = pastDay && summary.phase !== 'completed' && summary.finalArrivalDelayProvisional;
  return {
    kind,
    finished,
    delayMinutes: staleForecast ? null : summary.finalArrivalDelayMinutes,
    delayProvisional: staleForecast ? false : summary.finalArrivalDelayProvisional,
    serviceMode: null,
    cancellation: kind === 'severe' || kind === 'partCancelled' ? cancellation : null,
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
  // Wholly or partly cancelled by the api's own `cancellation` (DS-181),
  // else by the status alone, as before.
  const kind = train.cancellation?.kind ?? (cancelled ? 'full' : null);
  const summary = summariseJourney({
    currentLegGroup: trackedTrainListStatusGroup(train),
    otherLegGroups: [],
    legCount: 1,
    trainLegCount: 1,
    cancelledLegCount: cancelled ? 1 : 0,
    fullyCancelledLegCount: kind === 'full' ? 1 : 0,
    partCancelledLegCount: kind === 'partial' ? 1 : 0,
    unmatchedLegCount: 0,
    finishedLegCount: train.status === 'completed' ? 1 : 0,
    startedLegCount: train.status === 'en_route' || train.status === 'completed' ? 1 : 0,
    finalArrivalDelayMinutes: cancelled ? null : train.delayMinutes,
    finalArrivalDelayProvisional: train.delayProvisional ?? false,
    connection: null,
  });
  return journeySummaryRowStatus(summary, pastDay, train.cancellation ?? null);
}

/** The cancellation a `GET /Journeys/mine` row's badge words (DS-181): its
 * current leg's train's, when that train is the journey's only train leg.
 * A journey with several train legs keeps its rollup ("Part cancelled",
 * "Cancelled") without one train's detail, which would read as the whole
 * journey's. */
export function journeyListItemCancellation(journey: JourneyListItem): ListCancellation | null {
  const trainLegs = journey.trainLegCount ?? journey.legCount ?? 1;
  return trainLegs === 1 ? (journey.cancellation ?? null) : null;
}

/** Whether the viewer is the one to pick a train for this journey's
 * open legs, and so should be asked to: their own journey, or one they
 * follow. A journey shared into one of their groups that they don't follow
 * is the owner's to fill in (DS-199). */
export function viewerCanPickTrain(journey: Pick<JourneyListItem, 'followedFrom' | 'sharedVia'>): boolean {
  return journey.followedFrom !== undefined || (journey.sharedVia ?? []).length === 0;
}

/** One row of `GET /Journeys/mine`, by its whole-journey summary
 * (`journeyListItemSummary`). Past means the current leg's day is over.
 * "Needs a train picked" asks only a viewer who can act on it
 * (`viewerCanPickTrain`); anyone else reads "No train picked yet". */
export function journeyRowStatus(journey: JourneyListItem, today: string): TrackedRowStatus {
  const status = journeySummaryRowStatus(
    journeyListItemSummary(journey),
    journey.serviceDate < today,
    journeyListItemCancellation(journey),
  );
  return status.kind === 'unmatched' && !viewerCanPickTrain(journey) ? { ...status, kind: 'noTrainPickedYet' } : status;
}

/** The journey page header's status, from every leg. `null` with no legs. */
export function journeyDetailRowStatus(legs: readonly JourneyLegDetail[]): TrackedRowStatus | null {
  const summary = journeyDetailSummary(legs);
  return summary === null ? null : journeySummaryRowStatus(summary, false);
}

const UNTRACKED_GROUP: Record<UntrackedStatus, LegStatusGroup> = {
  planned: 'untrackedPlanned',
  inProgress: 'untrackedInProgress',
  done: 'untrackedDone',
};

/** A one-leg summary for a leg that isn't a picked train. */
function singleLegStatus(group: LegStatusGroup, pastDay: boolean): TrackedRowStatus {
  const summary = summariseJourney({
    currentLegGroup: group,
    otherLegGroups: [],
    legCount: 1,
    trainLegCount: group === 'unmatched' ? 1 : 0,
    cancelledLegCount: 0,
    unmatchedLegCount: group === 'unmatched' ? 1 : 0,
    finishedLegCount: group === 'untrackedDone' ? 1 : 0,
    startedLegCount: group === 'untrackedInProgress' || group === 'untrackedDone' ? 1 : 0,
    finalArrivalDelayMinutes: null,
    finalArrivalDelayProvisional: false,
    connection: null,
  });
  return journeySummaryRowStatus(summary, pastDay);
}

/** Whether the viewer of a group page is the one to pick a train for a
 * shared journey: its owner (who shared it) or a member following it. The
 * same rule as `viewerCanPickTrain` on the Journeys and home lists (DS-199). */
export function viewerCanPickGroupJourneyTrain(
  journey: Pick<GroupJourney, 'addedBy' | 'following'>,
  currentUserId: string | null,
): boolean {
  return currentUserId !== null && (journey.addedBy === currentUserId || journey.following === true);
}

/** A journey shared into a group (`GroupJourney`), which carries one leg:
 * the first train leg, else the first leg. Read by the same rule as a lone
 * train (that leg as a one-leg journey), so a group row says what the
 * owner's own Track list says for the same state. "Needs a train picked"
 * asks only the owner or a follower (`viewerCanPickGroupJourneyTrain`);
 * any other member reads "No train picked yet". */
export function groupJourneyRowStatus(
  journey: GroupJourney,
  today: string,
  currentUserId: string | null,
): TrackedRowStatus {
  const pastDay = journey.serviceDate < today;
  if (isUntrackedLeg(journey)) return singleLegStatus(UNTRACKED_GROUP[journey.untrackedStatus ?? 'planned'], pastDay);
  if (journey.resolutionStatus === null) {
    const status = singleLegStatus('unmatched', pastDay);
    return status.kind === 'unmatched' && !viewerCanPickGroupJourneyTrain(journey, currentUserId)
      ? { ...status, kind: 'noTrainPickedYet' }
      : status;
  }
  return trackedTrainRowStatus(
    {
      serviceDate: journey.serviceDate,
      resolutionStatus: journey.resolutionStatus,
      status: journey.status,
      delayMinutes: journey.delayMinutes,
      delayProvisional: journey.delayProvisional ?? false,
      serviceMode: journey.serviceMode,
      liveTracking: journey.liveTracking,
      trackingPhase: journey.trackingPhase ?? null,
    },
    today,
  );
}
