import { ColorSwatch, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { ButtonLink } from '@/components/ButtonLink';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { PinToggle } from '@/components/PinToggle';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitDisruptions } from '@/components/TransitDisruptions';
import { TransitBranches } from '@/components/TransitBranches';
import { TransitFrequency } from '@/components/TransitFrequency';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { getPreferences, getTransitLine, getTransitNetwork } from '@/lib/api';
import { formatDate } from '@/lib/dateFormat';
import { pageMetadata, previewImageAlt } from '@/lib/pageMetadata';
import { lineColour, orNotFound, TRANSIT_ID, transitStopHref } from '@/lib/transit';
import { branchesHeading, directionHeading } from '@/lib/transitModes';
import type { Preferences } from '@/lib/types';

interface Params {
  params: Promise<{ network: string; line: string }>;
}

interface Search {
  /** `?date=` (`YYYY-MM-DD`): the day "How often it runs" describes. */
  searchParams?: Promise<{ date?: string | string[] }>;
}

const DATE = /^\d{4}-\d{2}-\d{2}$/;

// Per-user data fails closed, as on the station page: an outage or no
// session reads as nothing pinned.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };

async function load(network: string, line: string, date?: string) {
  if (!TRANSIT_ID.test(network) || !TRANSIT_ID.test(line)) notFound();
  return Promise.all([orNotFound(getTransitNetwork(network)), orNotFound(getTransitLine(network, line, date))]);
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network, line } = await params;
  const [detail, lineDetail] = await load(network, line);
  const title = `${lineDetail.name}, ${detail.name}`;
  return pageMetadata(title, `${lineDetail.name} stops, how often it runs and scheduled times.`, {
    path: `/transit/${encodeURIComponent(network)}/lines/${encodeURIComponent(line)}`,
    imageAlt: previewImageAlt(title),
  });
}

/** `/transit/[network]/lines/[line]`: a line's status and disruptions
 * (when the network has a disruptions feed; for TfL, TfL's own status and
 * a link to its line page), how often it runs today and its first and
 * last departures, its stops in order, each way, and a link to its
 * timetable. The star pins it (`statusLineId`: its public id,
 * `metrolink:blue`, or TfL's line, `tfl-victoria`), as for a National
 * Rail line. */
export default async function TransitLinePage({ params, searchParams }: Params & Search) {
  const { network, line } = await params;
  const dateParam = (await searchParams)?.date;
  const date = typeof dateParam === 'string' && DATE.test(dateParam) ? dateParam : undefined;
  const [[detail, lineDetail], preferences] = await Promise.all([
    load(network, line, date),
    getPreferences().catch(() => NO_PREFERENCES),
  ]);
  const fromTfl = lineDetail.statusSource === 'tfl';
  // A direction with several branches lists them, each with its stations
  // (the TfL transit design §8.3); one with a single branch keeps its list.
  const branchesOf = (direction: string) =>
    (lineDetail.branches ?? []).filter((branch) => branch.direction === direction);
  const headingOf = (direction: (typeof lineDetail.directions)[number]) => {
    const branches = branchesOf(direction.direction);
    return branches.length > 1 ? branchesHeading(branches) : directionHeading(direction);
  };
  const pinId = lineDetail.statusLineId ?? lineDetail.id;
  const stopNames = Object.fromEntries(
    lineDetail.directions.flatMap((direction) => direction.stations.map((station) => [station.id, station.name])),
  );
  return (
    <Stack p="lg" gap="lg">
      <Stack gap={4}>
        <Group gap="sm" wrap="nowrap">
          <ColorSwatch color={lineColour(lineDetail)} size={18} withShadow={false} aria-hidden="true" />
          <Title order={1}>{lineDetail.name}</Title>
          <PinToggle kind="line" id={pinId} initiallyPinned={preferences.pinnedLines.includes(pinId)} />
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
        {fromTfl && (
          <ButtonLink href={`/lines/${encodeURIComponent(pinId)}`} variant="subtle">
            Status details
          </ButtonLink>
        )}
      </Group>

      {!fromTfl && (
        <TransitDisruptions
          situations={lineDetail.situations}
          empty="Nothing is affecting this line right now."
          network={network}
          stopNames={stopNames}
        />
      )}

      {lineDetail.frequency && (
        <TransitFrequency
          frequency={lineDetail.frequency}
          title={date ? `How often it runs on ${formatDate(date)}` : undefined}
          headings={Object.fromEntries(
            lineDetail.directions.map((direction) => [direction.direction, headingOf(direction)]),
          )}
        />
      )}

      <SimpleGrid cols={{ base: 1, sm: lineDetail.directions.length > 1 ? 2 : 1 }} spacing="lg">
        {lineDetail.directions.map((direction) => (
          <Stack key={direction.direction} gap="sm">
            <SectionTitle>{headingOf(direction)}</SectionTitle>
            {branchesOf(direction.direction).length > 1 ? (
              <TransitBranches network={network} line={line} branches={branchesOf(direction.direction)} />
            ) : (
              <Stack component="ol" gap={4} className="ds-plain-list">
                {direction.stations.map((station) => (
                  <li key={station.id}>
                    <TextLink href={transitStopHref(network, station.id)}>{station.name}</TextLink>
                  </li>
                ))}
              </Stack>
            )}
          </Stack>
        ))}
      </SimpleGrid>
      {lineDetail.directions.length === 0 && <Text c="dimmed">Nothing runs on this line in the timetable.</Text>}

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
