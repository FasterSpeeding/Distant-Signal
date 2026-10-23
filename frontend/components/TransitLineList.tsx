import { ColorSwatch, Group, Stack, Text } from '@mantine/core';
import { CardTitleLink, ClickableCard } from '@/components/ClickableCard';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { lineColour } from '@/lib/transit';
import type { TransitLine, TransitLineStatus } from '@/lib/types';

/** A network's lines as compact cards that wrap (seven Metrolink lines
 * fill no grid evenly), each a link to its line page, marked by its
 * colour, with its status when the network has a disruptions feed. */
export function TransitLineList({
  network,
  lines,
}: {
  network: string;
  lines: (TransitLine & { status?: TransitLineStatus })[];
}) {
  return (
    <Group gap="sm">
      {lines.map((line) => (
        <ClickableCard key={line.id} padding="sm">
          <Stack gap={6} align="flex-start">
            <Group gap="xs" wrap="nowrap">
              <ColorSwatch color={lineColour(line)} size={14} withShadow={false} aria-hidden="true" />
              <Text fw={600}>
                <CardTitleLink href={`/transit/${network}/lines/${line.lineId}`}>{line.name}</CardTitleLink>
              </Text>
            </Group>
            <TransitStatusBadge status={line.status} />
          </Stack>
        </ClickableCard>
      ))}
    </Group>
  );
}
