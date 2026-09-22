import { Badge } from '@mantine/core';
import { worstLegStatus, type LegStatusGroup } from '@/lib/journeyStatus';
import type { JourneyLegDetail } from '@/lib/types';

const LABEL: Record<LegStatusGroup, string> = {
  good: 'On track',
  awaiting: 'Awaiting first report',
  unmatched: 'Needs a train picked',
  delayed: 'Delayed',
  severe: 'Cancelled',
};

const COLOR: Record<LegStatusGroup, string> = {
  good: 'green',
  awaiting: 'gray',
  unmatched: 'blue',
  delayed: 'yellow',
  severe: 'red',
};

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
