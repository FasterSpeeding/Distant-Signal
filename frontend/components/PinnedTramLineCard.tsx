import { Text } from '@mantine/core';
import { StatusCard } from '@/components/StatusCard';
import { TransitLineSwatch } from '@/components/TransitLineSwatch';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { type PinnedTramLine } from '@/lib/transit';

/** A pinned tram line on the home page, in "Your lines" beside the
 * National Rail lines' `LineStatusCard`s and built the same way
 * (`StatusCard`): its colour and name (the card's link) with its status
 * badge when the network has a disruptions feed, then its network. */
export function PinnedTramLineCard({ pinned }: { pinned: PinnedTramLine }) {
  const { network, networkName, line } = pinned;
  return (
    <StatusCard
      href={`/transit/${network}/lines/${line.lineId}`}
      title={
        <>
          <TransitLineSwatch line={line} inline />
          {line.name}
        </>
      }
      badge={<TransitStatusBadge status={line.status} />}
    >
      <Text size="xs" c="dimmed">
        {networkName}
      </Text>
    </StatusCard>
  );
}
