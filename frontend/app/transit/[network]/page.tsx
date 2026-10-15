import { Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { SectionTitle } from '@/components/SectionTitle';
import { SearchableList } from '@/components/SearchableList';
import { TextLink } from '@/components/TextLink';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitDisruptions } from '@/components/TransitDisruptions';
import { TransitLineList } from '@/components/TransitLineList';
import { getTransitNetwork, getTransitSituations, getTransitStops } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';
import { orNotFound, TRANSIT_ID } from '@/lib/transit';

interface Params {
  params: Promise<{ network: string }>;
  /** `?q=`, "Find a stop". */
  searchParams?: Promise<{ q?: string | string[] }>;
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network } = await params;
  if (!TRANSIT_ID.test(network)) notFound();
  const detail = await orNotFound(getTransitNetwork(network));
  return pageMetadata(detail.name, `${detail.name}: lines, stops and scheduled tram times.`);
}

/** `/transit/[network]`: a tram network's lines (with their status, when
 * the network has a disruptions feed), its disruptions, and its stops. */
export default async function TransitNetworkPage({ params, searchParams }: Params) {
  const { network } = await params;
  const { q } = (await searchParams) ?? {};
  if (!TRANSIT_ID.test(network)) notFound();
  const [detail, stops, situations] = await Promise.all([
    orNotFound(getTransitNetwork(network)),
    orNotFound(getTransitStops(network)),
    getTransitSituations(network).catch(() => null),
  ]);
  const lineNames = Object.fromEntries(detail.lines.map((line) => [line.id, line.name]));
  const hasFeed = (detail.status?.state ?? 'none') !== 'none';
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

      <TransitDisruptions
        situations={situations}
        failed={hasFeed && situations === null}
        empty="No disruptions reported."
        lineNames={lineNames}
      />

      <Stack gap="sm">
        <SectionTitle>Stops</SectionTitle>
        {stops.length === 0 ? (
          <Text c="dimmed">No stops in the timetable.</Text>
        ) : (
          // The list is a client component so "Find a stop" can filter it in
          // place; the page stays a Server Component.
          <SearchableList
            searchLabel="Find a stop"
            noun={{ one: 'stop', other: 'stops' }}
            titlePrefix={`${detail.name} stops`}
            initialQuery={Array.isArray(q) ? q[0] : q}
            layout="list"
            items={stops.map((stop) => ({
              key: stop.id,
              text: stop.name,
              node: <TextLink href={`/transit/${network}/stops/${stop.id}`}>{stop.name}</TextLink>,
            }))}
          />
        )}
      </Stack>

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
