import { ColorSwatch, Group, Stack, Text } from '@mantine/core';
import { CardLink } from '@/components/CardLink';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { lineColour, type PinnedTramLine } from '@/lib/transit';

/** A pinned tram line on the home page: its name and network, its status
 * when the network has a disruptions feed, the whole card a link to its
 * line page. */
export function PinnedTramLineCard({ pinned }: { pinned: PinnedTramLine }) {
  const { network, networkName, line } = pinned;
  return (
    <CardLink href={`/transit/${network}/lines/${line.lineId}`}>
      <Stack gap={6} align="flex-start">
        <Group gap="xs" wrap="nowrap">
          <ColorSwatch color={lineColour(line)} size={14} withShadow={false} aria-hidden="true" />
          <Text fw={600}>{line.name}</Text>
        </Group>
        <Text size="sm" c="dimmed">
          {networkName}
        </Text>
        <TransitStatusBadge status={line.status} />
      </Stack>
    </CardLink>
  );
}
