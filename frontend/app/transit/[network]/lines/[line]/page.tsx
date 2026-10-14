import { ColorSwatch, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { ButtonLink } from '@/components/ButtonLink';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { PinToggle } from '@/components/PinToggle';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitDisruptions } from '@/components/TransitDisruptions';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { getPreferences, getTransitLine, getTransitNetwork } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';
import { lineColour, orNotFound, TRANSIT_ID } from '@/lib/transit';
import type { Preferences } from '@/lib/types';

interface Params {
  params: Promise<{ network: string; line: string }>;
}

// Per-user data fails closed, as on the station page: an outage or no
// session reads as nothing pinned.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };

async function load(network: string, line: string) {
  if (!TRANSIT_ID.test(network) || !TRANSIT_ID.test(line)) notFound();
  return Promise.all([orNotFound(getTransitNetwork(network)), orNotFound(getTransitLine(network, line))]);
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network, line } = await params;
  const [detail, lineDetail] = await load(network, line);
  return pageMetadata(`${lineDetail.name}, ${detail.name}`, `${lineDetail.name} stops and scheduled tram times.`);
}

/** `/transit/[network]/lines/[line]`: a tram line's status and disruptions
 * (when the network has a disruptions feed), its stops in order, each
 * way, and a link to its timetable. The star pins it (its public id,
 * `metrolink:blue`), as for a National Rail line. */
export default async function TransitLinePage({ params }: Params) {
  const { network, line } = await params;
  const [[detail, lineDetail], preferences] = await Promise.all([
    load(network, line),
    getPreferences().catch(() => NO_PREFERENCES),
  ]);
  return (
    <Stack p="lg" gap="lg">
      <Stack gap={4}>
        <Group gap="sm" wrap="nowrap">
          <ColorSwatch color={lineColour(lineDetail)} size={18} withShadow={false} aria-hidden="true" />
          <Title order={1}>{lineDetail.name}</Title>
          <PinToggle kind="line" id={lineDetail.id} initiallyPinned={preferences.pinnedLines.includes(lineDetail.id)} />
        </Group>
        <TextLink href={`/transit/${network}`}>{detail.name}</TextLink>
      </Stack>

      {lineDetail.status && lineDetail.status.state !== 'none' && (
        <Group>
          <TransitStatusBadge status={lineDetail.status} />
        </Group>
      )}

      <Group>
        <ButtonLink href={`/transit/${network}/lines/${line}/timetable`} variant="light">
          Timetable
        </ButtonLink>
      </Group>

      <TransitDisruptions situations={lineDetail.situations} empty="Nothing is affecting this line right now." />

      <SimpleGrid cols={{ base: 1, sm: lineDetail.directions.length > 1 ? 2 : 1 }} spacing="lg">
        {lineDetail.directions.map((direction) => (
          <Stack key={direction.direction} gap="sm">
            <SectionTitle>
              {direction.headsign
                ? `Towards ${direction.headsign}`
                : direction.direction === 'outbound'
                  ? 'One way'
                  : 'The other way'}
            </SectionTitle>
            <Stack component="ol" gap={4} className="ds-plain-list">
              {direction.stations.map((station) => (
                <li key={station.id}>
                  <TextLink href={`/transit/${network}/stops/${station.id}`}>{station.name}</TextLink>
                </li>
              ))}
            </Stack>
          </Stack>
        ))}
      </SimpleGrid>
      {lineDetail.directions.length === 0 && <Text c="dimmed">No trams on this line in the timetable.</Text>}

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
