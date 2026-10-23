import { Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { CardTitleLink, ClickableCard } from '@/components/ClickableCard';
import { getTransitNetworks } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';

export const metadata: Metadata = pageMetadata('Trams', 'Tram networks: lines, stops and scheduled departures.');

/** `/transit`: the tram networks we show. Not found while there are none
 * (the default: `API_TRANSIT_NETWORKS` is empty), so nothing about trams is
 * visible until a network is switched on. */
export default async function TransitPage() {
  const networks = await getTransitNetworks();
  if (networks.length === 0) notFound();
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Trams</Title>
      <Text c="dimmed">Scheduled times from the published timetable. We don&apos;t have live tram times.</Text>
      <Stack gap="sm">
        {networks.map((network) => (
          <ClickableCard key={network.id}>
            <Text fw={600}>
              <CardTitleLink href={`/transit/${network.id}`}>{network.name}</CardTitleLink>
            </Text>
          </ClickableCard>
        ))}
      </Stack>
    </Stack>
  );
}
