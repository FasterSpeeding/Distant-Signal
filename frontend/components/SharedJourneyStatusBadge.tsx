import { JourneyStatusGroupBadge } from './JourneyStatusBadge';
import { TrackedTrainStatusBadge } from './TrackedTrainStatusBadge';
import { isUntrackedLeg } from '@/lib/untrackedLeg';
import type { LegStatusGroup } from '@/lib/journeyStatus';
import type { GroupJourney, UntrackedStatus } from '@/lib/types';

const UNTRACKED_GROUP: Record<UntrackedStatus, LegStatusGroup> = {
  planned: 'untrackedPlanned',
  inProgress: 'untrackedInProgress',
  done: 'untrackedDone',
};

/** The status badge for a journey shared into a group (`GroupJourney`),
 * which summarises one leg: the journey's first train leg, else its first
 * leg. Reads the same as the owner's own pages:
 *
 * - a journey of untracked legs only (walk, Tube) shows that leg's
 *   clock-only state ("Planned", "In progress (not live tracked)",
 *   "Done");
 * - a train leg with no train picked yet asks for one, in the action
 *   colour, like the owner's journey list;
 * - a picked train gets the tracked-train badge, with its
 *   `trackingPhase` and `serviceMode` ("Planned", "Not in the timetable",
 *   "Bus · Timetabled only").
 *
 * It used to treat every row as a train and coalesce a missing
 * `resolutionStatus` to `unresolved`, so a walk-and-Tube journey, or one
 * still searching for its train, read as a red "Unmatched". */
export function SharedJourneyStatusBadge({ journey }: { journey: GroupJourney }) {
  if (isUntrackedLeg(journey)) {
    return <JourneyStatusGroupBadge group={UNTRACKED_GROUP[journey.untrackedStatus ?? 'planned']} />;
  }
  if (journey.resolutionStatus === null) {
    return <JourneyStatusGroupBadge group="unmatched" />;
  }
  return (
    <TrackedTrainStatusBadge
      train={{
        resolutionStatus: journey.resolutionStatus,
        status: journey.status,
        delayMinutes: journey.delayMinutes,
        serviceMode: journey.serviceMode,
        liveTracking: journey.liveTracking,
        trackingPhase: journey.trackingPhase ?? null,
        firstReportExpectedAt: journey.firstReportExpectedAt ?? null,
      }}
    />
  );
}
