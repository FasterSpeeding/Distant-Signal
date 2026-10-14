import { Badge } from '@mantine/core';
import { StatusBadge } from '@/components/StatusBadge';
import type { TransitLineStatus } from '@/lib/types';

/** A tram line's headline status: the first of its statuses ("Minor
 * delays", "No reported disruption"), or "Status unavailable" once the
 * network's disruptions are over 30 minutes old. Nothing while the network
 * has no disruptions feed (`state: none`), so a network served without one
 * shows no status at all. */
export function TransitStatusBadge({ status }: { status?: TransitLineStatus | null | undefined }) {
  if (!status || status.state === 'none') return null;
  const headline = status.state === 'current' ? status.statuses[0] : undefined;
  if (!headline) {
    return (
      <Badge color="gray" variant="light" tt="none">
        Status unavailable
      </Badge>
    );
  }
  return <StatusBadge severity={headline.severity} />;
}
