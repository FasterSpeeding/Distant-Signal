import { StateBadge, type StateTone } from './StateBadge';
import { worstLegStatus, type LegStatusGroup } from '@/lib/journeyStatus';
import { isOpenTrainLeg, UNTRACKED_STATUS_LABEL } from '@/lib/untrackedLeg';
import type { JourneyLegDetail } from '@/lib/types';

// 2026-09-22 UX review finding M22/2.11: "On track" here and "On time" on
// `TrainJourney.tsx`'s own per-leg delay badge (and `JourneyTimeline.tsx`'s
// per-stop one) named the identical state two different ways, one line
// apart on `journey-168-desktop.png`. "On time" is the app's own more
// established term for this state -- it's what both of those other,
// pre-existing badges already say -- so this rollup badge aligns to it
// rather than the other way round.
const LABEL: Record<LegStatusGroup, string> = {
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
};

const TONE: Record<LegStatusGroup, StateTone> = {
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
};

/** The badge for an ALREADY-CLASSIFIED status group -- the one place a
 * `LegStatusGroup` becomes a label and a colour. Split out of
 * `JourneyStatusBadge` below so `/track/mine`'s journey rows, which
 * classify from `GET /Journeys/mine`'s flat row shape rather than from a
 * `JourneyLegDetail[]`, render the identical badge instead of a second
 * copy of `LABEL`/`COLOR` that is free to drift from this one. No
 * `Tooltip`: it would only restate the badge's own visible text (2026-09-22
 * UX review, M17) and a `Tooltip` on a non-focusable `Badge` is
 * keyboard-unreachable anyway. */
export function JourneyStatusGroupBadge({ group }: { group: LegStatusGroup }) {
  return <StateBadge tone={TONE[group]}>{LABEL[group]}</StateBadge>;
}

/** The journey-level summary badge, per spec §3. Renders nothing for a
 * journey with no legs (should not occur in practice).
 *
 * Review §2.5/M17: this used to wrap the `Badge` in a `Tooltip` whose
 * `label` was the exact same string the badge already renders visibly --
 * announcing nothing a sighted user didn't already have, while also being
 * unreachable by keyboard (a `Tooltip` needs a focusable child to trigger
 * on focus, and a bare `Badge` isn't one). Dropped rather than reworded,
 * per the review's own first-choice recommendation: there is no second
 * fact about journey status worth adding here that wouldn't duplicate
 * what `JourneyLegCard`'s own per-leg badges already say.
 *
 * Finding I15/2.3: when the worst status is `unmatched`, this used to say
 * the flat "Needs a train picked" with no indication of WHICH leg or HOW
 * MANY -- on mobile the badge sits roughly 800px above the card it refers
 * to. It now (a) counts the unmatched legs and (b) becomes a same-page
 * anchor to the FIRST one, landing on `id="leg-{legId}"`
 * (`app/journeys/[id]/page.tsx` sets that id on every leg's own wrapper).
 * A plain `<a href="#...">` -- no client JS needed for the scroll itself,
 * the browser's native same-page anchor behaviour is exactly what's
 * wanted here. */
export function JourneyStatusBadge({ legs }: { legs: JourneyLegDetail[] }) {
  const worst = worstLegStatus(legs);
  if (worst === null) return null;

  if (worst === 'unmatched') {
    // An untracked leg (Tube, bus, walk) never has a train to pick.
    const unmatchedLegs = legs.filter(isOpenTrainLeg);
    const firstUnmatchedLegId = unmatchedLegs[0]?.id;
    const label = unmatchedLegs.length === 1 ? '1 leg needs a train' : `${unmatchedLegs.length} legs need a train`;
    if (firstUnmatchedLegId === undefined) {
      return <StateBadge tone={TONE.unmatched}>{label}</StateBadge>;
    }
    return (
      <StateBadge href={`#leg-${firstUnmatchedLegId}`} tone={TONE.unmatched}>
        {label}
      </StateBadge>
    );
  }

  return <StateBadge tone={TONE[worst]}>{LABEL[worst]}</StateBadge>;
}
