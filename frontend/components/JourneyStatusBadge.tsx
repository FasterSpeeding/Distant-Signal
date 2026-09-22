import { Badge } from '@mantine/core';
import { worstLegStatus, type LegStatusGroup } from '@/lib/journeyStatus';
import type { JourneyLegDetail } from '@/lib/types';

const LABEL: Record<LegStatusGroup, string> = {
  good: 'On track',
  awaiting: 'Awaiting first report',
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

const COLOR: Record<LegStatusGroup, string> = {
  good: 'green',
  awaiting: 'gray',
  unmatched: 'blue',
  delayed: 'yellow',
  // Orange, not red: red is already "Cancelled" one row below, and two
  // different facts must not share a hue here (the same rule the 09-22
  // review's I20 applies to the platform/delay badge pair).
  skipped: 'orange',
  severe: 'red',
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
  return (
    <Badge color={COLOR[group]} variant="light" tt="none">
      {LABEL[group]}
    </Badge>
  );
}

/** The journey-level summary badge, per spec §3. Renders nothing for a
 * journey with no legs (should not occur in practice).
 *
 * Review §2.5/M17: this used to wrap the `Badge` in a `Tooltip` whose
 * `label` was the exact same string the badge already renders visibly --
 * announcing nothing a sighted user didn't already have, while also being
 * unreachable by keyboard (a `Tooltip` needs a focusable child to trigger
 * on focus, and a bare `Badge` isn't one). Dropped rather than reworded:
 * there is no second fact about journey status worth adding here that
 * wouldn't duplicate what `JourneyLegCard`'s own per-leg badges already
 * say. */
export function JourneyStatusBadge({ legs }: { legs: JourneyLegDetail[] }) {
  const worst = worstLegStatus(legs);
  if (worst === null) return null;
  return (
    <Badge color={COLOR[worst]} variant="light" tt="none">
      {LABEL[worst]}
    </Badge>
  );
}
