import { Fragment } from 'react';
import { Stack, Text, Title } from '@mantine/core';
import { notFound, redirect } from 'next/navigation';
import type { Metadata } from 'next';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitDisruptions } from '@/components/TransitDisruptions';
import { TransitLineList } from '@/components/TransitLineList';
import { TransitStopFinder } from '@/components/TransitStopFinder';
import { getTransitNetwork, getTransitSituations, getTransitStops } from '@/lib/api';
import { pageMetadata, previewImageAlt } from '@/lib/pageMetadata';
import { orNotFound, TRANSIT_ID } from '@/lib/transit';
import { stopIndexHref } from '@/lib/transitStopFinder';
import { groupLinesByMode, networkModesPhrase } from '@/lib/transitModes';

interface Params {
  params: Promise<{ network: string }>;
  /** `?q=`, the old in-page "Find a stop": sent on to the A to Z. */
  searchParams?: Promise<{ q?: string | string[] }>;
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network } = await params;
  if (!TRANSIT_ID.test(network)) notFound();
  const detail = await orNotFound(getTransitNetwork(network));
  const modes = networkModesPhrase(detail);
  const description = `${detail.name}: ${modes ? `${modes} lines` : 'lines'}, stops and scheduled times.`;
  return pageMetadata(detail.name, description, {
    path: `/transit/${encodeURIComponent(network)}`,
    imageAlt: previewImageAlt(detail.name),
  });
}

/** `/transit/[network]`: a network's lines (with their status, when the
 * network has a disruptions feed or, for TfL, TfL's own status), grouped
 * by mode when it has several, its disruptions, and its stops. The TfL
 * network has no disruptions list (its status is TfL's, detailed on each
 * line's `/lines/tfl-<line>` page) and links the TfL lines that are
 * National Rail lines here (the TfL transit design §8.2).
 *
 * Stops are a type-ahead finder (`TransitStopFinder`), not a list: the
 * page neither fetches nor renders the network's stations (356 for TfL),
 * and the finder loads them on first use. A line's page lists its stops in
 * order, and `/transit/[network]/stops` has the A to Z. */
export default async function TransitNetworkPage({ params, searchParams }: Params) {
  const { network } = await params;
  const { q } = (await searchParams) ?? {};
  if (!TRANSIT_ID.test(network)) notFound();
  const search = Array.isArray(q) ? q[0] : q;
  if (search?.trim()) redirect(stopIndexHref(network, search));
  const [detail, situations] = await Promise.all([
    orNotFound(getTransitNetwork(network)),
    getTransitSituations(network).catch(() => null),
  ]);
  const fromTfl = detail.statusSource === 'tfl';
  const groups = groupLinesByMode(detail.lines);
  const lineNames = Object.fromEntries(detail.lines.map((line) => [line.id, line.name]));
  // The disruptions name their stops: only then are the stations fetched
  // (from the data cache, server-side; none of it reaches the browser).
  const namesStops =
    !fromTfl && !!situations && [...situations.current, ...situations.upcoming].some((s) => s.stops.length > 0);
  const stops = namesStops ? await getTransitStops(network).catch(() => []) : [];
  const stopNames = Object.fromEntries(stops.map((stop) => [stop.id, stop.name]));
  const hasFeed = (detail.status?.state ?? 'none') !== 'none';
  // Only the TfL network has live data (TfL's arrivals, on each stop's
  // `/tfl/stops/{id}` page); any other says it has none, in its own modes.
  const modes = networkModesPhrase(detail);
  const notice = fromTfl
    ? "Scheduled times from TfL's timetable, and TfL's line status. Each stop's page has live arrivals."
    : `Scheduled times from the published timetable. We don't have live ${modes ? `${modes} ` : ''}times.`;
  return (
    <Stack p="lg" gap="lg">
      <Stack gap={4}>
        <Title order={1}>{detail.name}</Title>
        <Text c="dimmed">{notice}</Text>
      </Stack>

      {groups.map((group) => (
        <Stack key={group.mode ?? 'lines'} gap="sm">
          <SectionTitle>{group.label ?? 'Lines'}</SectionTitle>
          <TransitLineList network={network} lines={group.lines} label={group.label ?? 'Lines'} />
        </Stack>
      ))}

      {detail.nationalRailLines && detail.nationalRailLines.length > 0 && (
        <Stack gap="xs">
          <SectionTitle>Also run by TfL</SectionTitle>
          {/* Links in a sentence, not cards: they lead off this network, so
              they stay under its own lines. */}
          <Text size="sm" c="dimmed">
            On the National Rail network, with live trains and timetables there:{' '}
            {detail.nationalRailLines.map((line, index, all) => (
              <Fragment key={line.id}>
                <TextLink href={`/lines/${line.id}`} inline underline="always">
                  {line.name}
                </TextLink>
                {index < all.length - 2 ? ', ' : index === all.length - 2 ? ' and ' : '.'}
              </Fragment>
            ))}
          </Text>
        </Stack>
      )}

      {!fromTfl && (
        <TransitDisruptions
          situations={situations}
          failed={hasFeed && situations === null}
          empty="No disruptions reported."
          lineNames={lineNames}
          network={network}
          stopNames={stopNames}
        />
      )}

      <Stack gap="sm">
        <SectionTitle>Stops</SectionTitle>
        <TransitStopFinder network={network} networkName={detail.name} lineNames={lineNames} />
        <Text size="sm" c="dimmed">
          Each line&apos;s page lists its stops in order.{' '}
          {/* Not prefetched: the A to Z is the full list this page leaves out. */}
          <TextLink href={stopIndexHref(network)} inline underline="always" prefetch={false}>
            All stops, A to Z
          </TextLink>
        </Text>
      </Stack>

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
