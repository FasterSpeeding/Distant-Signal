import { Badge, Tooltip } from '@mantine/core';
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

/** The journey-level summary badge, per spec §3. Renders nothing for a
 * journey with no legs (should not occur in practice). */
export function JourneyStatusBadge({ legs }: { legs: JourneyLegDetail[] }) {
  const worst = worstLegStatus(legs);
  if (worst === null) return null;
  return (
    <Tooltip label={LABEL[worst]}>
      <Badge color={COLOR[worst]} variant="light" tt="none">
        {LABEL[worst]}
      </Badge>
    </Tooltip>
  );
}
