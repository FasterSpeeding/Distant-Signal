import { ColorSwatch, Group, Text } from '@mantine/core';
import { CardLink } from '@/components/CardLink';
import { lineColour } from '@/lib/transit';
import type { TransitLine } from '@/lib/types';

/** A network's lines as compact cards that wrap (seven Metrolink lines
 * fill no grid evenly), each a link to its line page, marked by its
 * colour. */
export function TransitLineList({ network, lines }: { network: string; lines: TransitLine[] }) {
  return (
    <Group gap="sm">
      {lines.map((line) => (
        <CardLink key={line.id} href={`/transit/${network}/lines/${line.lineId}`} padding="sm">
          <Group gap="xs" wrap="nowrap">
            <ColorSwatch color={lineColour(line)} size={14} withShadow={false} aria-hidden="true" />
            <Text fw={600}>{line.name}</Text>
          </Group>
        </CardLink>
      ))}
    </Group>
  );
}
