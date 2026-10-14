import { Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitLineList } from '@/components/TransitLineList';
import { getTransitNetwork, getTransitStops } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';
import { orNotFound, TRANSIT_ID } from '@/lib/transit';

interface Params {
  params: Promise<{ network: string }>;
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network } = await params;
  if (!TRANSIT_ID.test(network)) notFound();
  const detail = await orNotFound(getTransitNetwork(network));
  return pageMetadata(detail.name, `${detail.name}: lines, stops and scheduled tram times.`);
}

/** `/transit/[network]`: a tram network's lines and stops. */
export default async function TransitNetworkPage({ params }: Params) {
  const { network } = await params;
  if (!TRANSIT_ID.test(network)) notFound();
  const [detail, stops] = await Promise.all([
    orNotFound(getTransitNetwork(network)),
    orNotFound(getTransitStops(network)),
  ]);
  return (
    <Stack p="lg" gap="lg">
      <Stack gap={4}>
        <Title order={1}>{detail.name}</Title>
        <Text c="dimmed">Scheduled times from the published timetable. We don&apos;t have live tram times.</Text>
      </Stack>

      <Stack gap="sm">
        <SectionTitle>Lines</SectionTitle>
        <TransitLineList network={network} lines={detail.lines} />
      </Stack>

      <Stack gap="sm">
        <SectionTitle>Stops</SectionTitle>
        {stops.length === 0 ? (
          <Text c="dimmed">No stops in the timetable.</Text>
        ) : (
          <Stack component="ul" gap={4} className="ds-plain-list">
            {stops.map((stop) => (
              <li key={stop.id}>
                <TextLink href={`/transit/${network}/stops/${stop.id}`}>{stop.name}</TextLink>
              </li>
            ))}
          </Stack>
        )}
      </Stack>

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
