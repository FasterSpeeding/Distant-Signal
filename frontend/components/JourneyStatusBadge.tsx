import { Badge, Tooltip } from '@mantine/core';
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
