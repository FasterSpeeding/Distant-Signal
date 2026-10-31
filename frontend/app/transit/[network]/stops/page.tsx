import { Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { SearchableList } from '@/components/SearchableList';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { getTransitNetwork, getTransitStops } from '@/lib/api';
import { pageMetadata, previewImageAlt } from '@/lib/pageMetadata';
import { orNotFound, TRANSIT_ID, transitStopHref } from '@/lib/transit';

interface Params {
  params: Promise<{ network: string }>;
  /** `?q=`, "Find a stop" (also where the network page's finder submits). */
  searchParams?: Promise<{ q?: string | string[] }>;
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network } = await params;
  if (!TRANSIT_ID.test(network)) notFound();
  const detail = await orNotFound(getTransitNetwork(network));
  const title = `${detail.name} stops`;
  return pageMetadata(title, `Every ${detail.name} stop, A to Z, with its scheduled departures.`, {
    path: `/transit/${encodeURIComponent(network)}/stops`,
    imageAlt: previewImageAlt(title),
  });
}

/** `/transit/[network]/stops`: every station of a network, A to Z, with
 * "Find a stop" filtering in place (`?q=`). The network page has only a
 * type-ahead finder, so the full list lives here: one link away for
 * browsing, and where the finder's form lands without JavaScript. */
export default async function TransitStopsPage({ params, searchParams }: Params) {
  const { network } = await params;
  const { q } = (await searchParams) ?? {};
  if (!TRANSIT_ID.test(network)) notFound();
  const [detail, stops] = await Promise.all([
    orNotFound(getTransitNetwork(network)),
    orNotFound(getTransitStops(network)),
  ]);
  return (
    <Stack p="lg" gap="lg">
      <Stack gap={4}>
        <Title order={1}>{detail.name} stops</Title>
        <TextLink href={`/transit/${encodeURIComponent(network)}`}>{detail.name}</TextLink>
      </Stack>

      <Stack gap="sm">
        <SectionTitle>A to Z</SectionTitle>
        {stops.length === 0 ? (
          <Text c="dimmed">No stops in the timetable.</Text>
        ) : (
          <SearchableList
            searchLabel="Find a stop"
            noun={{ one: 'stop', other: 'stops' }}
            titlePrefix={`${detail.name} stops`}
            initialQuery={Array.isArray(q) ? q[0] : q}
            layout="list"
            listLabel="Stops"
            items={stops.map((stop) => ({
              key: stop.id,
              text: stop.name,
              // Not prefetched: a few hundred stop links scrolling past would
              // each fetch a route.
              node: (
                <TextLink href={transitStopHref(network, stop.id)} prefetch={false}>
                  {stop.name}
                </TextLink>
              ),
            }))}
          />
        )}
      </Stack>

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
