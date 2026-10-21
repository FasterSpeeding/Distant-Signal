import { Group } from '@mantine/core';
import { DelayBadge } from './DelayBadge';
import { ServiceModeBadge } from './ServiceModeBadge';
import { StateBadge, type StateTone } from './StateBadge';
import type { JourneySummaryGroup } from '@/lib/journeyStatus';
import type { TrackedRowStatus } from '@/lib/trackedRowStatus';
import { UNTRACKED_STATUS_LABEL } from '@/lib/untrackedLeg';

// 2026-09-22 UX review finding M22/2.11: "On track" here and "On time" on
// `TrainJourney.tsx`'s own per-leg delay badge (and `JourneyTimeline.tsx`'s
// per-stop one) named the identical state two different ways, one line
// apart on `journey-168-desktop.png`. "On time" is the app's own more
// established term for this state -- it's what both of those other,
// pre-existing badges already say -- so this rollup badge aligns to it
// rather than the other way round.
const LABEL: Record<JourneySummaryGroup, string> = {
  // An untracked leg's clock-only state (spec 2026-10-20 §2.2). It only
  // heads a journey with no train legs (`LEG_STATUS_RANK`).
  untrackedDone: UNTRACKED_STATUS_LABEL.done,
  untrackedPlanned: UNTRACKED_STATUS_LABEL.planned,
  untrackedInProgress: UNTRACKED_STATUS_LABEL.inProgress,
  good: 'On time',
  // A timetabled train whose first report isn't due yet (spec §4).
  planned: 'Planned',
  awaiting: 'Awaiting first report',
  // A train with no timetable match and no report (spec §3).
  noTimetable: 'Not in the timetable',
  unmatched: 'Needs a train picked',
  delayed: 'Delayed',
  // Deliberately NOT folded into `severe`/"Cancelled": the train IS
  // running, it just isn't calling where this journey needs it to, and
  // labelling that "Cancelled" would trade one wrong summary for another.
  // Its own group and label, ranked between `unmatched` and `severe` --
  // see `lib/journeyStatus.ts`'s `LEG_STATUS_RANK`.
  skipped: 'Not stopping at your station',
  severe: 'Cancelled',
  // Some train legs cancelled, not all (2026-10-21 journey summary).
  partCancelled: 'Part cancelled',
  connectionAtRisk: 'Tight connection',
  connectionMissed: 'Connection missed',
};

const TONE: Record<JourneySummaryGroup, StateTone> = {
  untrackedDone: 'neutral',
  untrackedPlanned: 'neutral',
  untrackedInProgress: 'neutral',
  good: 'good',
  // Gray, like "Awaiting first report": nothing is wrong yet, and blue is
  // kept for planned works.
  planned: 'neutral',
  awaiting: 'neutral',
  // Grape: the traveller should check the train's UID or date.
  noTimetable: 'action',
  // Grape, not blue: this is the one state that asks the user to act
  // (pick a train), so it takes the app's action colour. Blue means a
  // planned or changed arrangement (planned closures, a changed platform)
  // -- docs/style-guide.md "Semantic roles". Yellow/orange are already
  // "Delayed"/"Not stopping" in this same rollup, and two facts must not
  // share a hue here.
  unmatched: 'action',
  delayed: 'caution',
  // Orange, not red: red is already "Cancelled" one row below, and two
  // different facts must not share a hue here (the same rule the 09-22
  // review's I20 applies to the platform/delay badge pair).
  skipped: 'late',
  severe: 'critical',
  // Red like "Cancelled": a train the traveller planned on isn't running.
  partCancelled: 'critical',
  // Yellow like "Delayed": still possible, at risk.
  connectionAtRisk: 'caution',
  // Orange like a late train or a skipped stop: the plan no longer works.
  connectionMissed: 'late',
};

/** The badge for an ALREADY-CLASSIFIED status group -- the one place a
 * `JourneySummaryGroup` becomes a label and a colour. No `Tooltip`: it
 * would only restate the badge's own visible text (2026-09-22 UX review,
 * M17) and a `Tooltip` on a non-focusable `Badge` is keyboard-unreachable
 * anyway. Re-exported from `JourneyStatusBadge.tsx`, its old home. */
export function JourneyStatusGroupBadge({ group }: { group: JourneySummaryGroup }) {
  return <StateBadge tone={TONE[group]}>{LABEL[group]}</StateBadge>;
}

/** The one status badge for a journey or a tracked train, wherever it is
 * listed or headed (`lib/trackedRowStatus.ts` does the classifying). The
 * same status reads the same either way: the journey rollup's words and
 * colours (`JourneyStatusGroupBadge`), with the delay badge standing in
 * for "On time"/"Delayed" once the delay at the final destination is
 * known, so a late arrival says by how much.
 *
 * A finished row says how it ended: "Completed" (with its final delay),
 * "No report received" or "No train picked", all gray. Cancelled stays
 * red. "Part cancelled" (red) sits beside the final delay, so a journey
 * that lost a train but still got there on time says both. */
export function TrackedRowStatusBadge({ status }: { status: TrackedRowStatus }) {
  const { kind, delayMinutes, delayProvisional } = status;
  const delay =
    delayMinutes === null ? null : <DelayBadge delayMinutes={delayMinutes} provisional={delayProvisional} />;
  switch (kind) {
    case 'timetableOnly':
      return (
        <Group gap={6} wrap="nowrap">
          <ServiceModeBadge mode={status.serviceMode} />
          <StateBadge tone="neutral">Timetabled only</StateBadge>
        </Group>
      );
    case 'completed':
      return (
        <Group gap={6} wrap="nowrap">
          <StateBadge tone="neutral">Completed</StateBadge>
          {delay}
        </Group>
      );
    case 'partCancelled':
      return (
        <Group gap={6} wrap="nowrap">
          <JourneyStatusGroupBadge group={kind} />
          {delay}
        </Group>
      );
    case 'noReport':
      return <StateBadge tone="neutral">No report received</StateBadge>;
    case 'noTrainPicked':
      return <StateBadge tone="neutral">No train picked</StateBadge>;
    case 'good':
    case 'delayed':
      return delay ?? <JourneyStatusGroupBadge group={kind} />;
    default:
      return <JourneyStatusGroupBadge group={kind} />;
  }
}
