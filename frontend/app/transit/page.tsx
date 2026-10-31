import { Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { CardTitleLink, ClickableCard } from '@/components/ClickableCard';
import { getTransitNetworks } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';
import { transitNetworkSummary } from '@/lib/transitModes';

export const metadata: Metadata = pageMetadata(
  'Trams and metros',
  'Tram and metro networks: lines, stops and scheduled departures.',
  { path: '/transit' },
);

/** `/transit`: the tram and metro networks we show. Not found while there
 * are none (the default: `API_TRANSIT_NETWORKS` is empty), so nothing
 * about them is visible until a network is switched on. */
export default async function TransitPage() {
  const networks = await getTransitNetworks();
  if (networks.length === 0) notFound();
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Trams and metros</Title>
      <Text c="dimmed">Lines, stops and scheduled times from each network&apos;s published timetable.</Text>
      <Stack gap="sm">
        {networks.map((network) => {
          const summary = transitNetworkSummary(network.id);
          return (
            <ClickableCard key={network.id}>
              <Stack gap={2}>
                <Text fw={600}>
                  <CardTitleLink href={`/transit/${network.id}`}>{network.name}</CardTitleLink>
                </Text>
                {summary && (
                  <Text size="sm" c="dimmed">
                    {summary}
                  </Text>
                )}
              </Stack>
            </ClickableCard>
          );
        })}
      </Stack>
    </Stack>
  );
}
