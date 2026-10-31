import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  ColorSwatch,
  Group,
  Stack,
  Text,
  Title,
} from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { ButtonLink } from '@/components/ButtonLink';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { PinToggle } from '@/components/PinToggle';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitDisruptions } from '@/components/TransitDisruptions';
import { TransitFrequency } from '@/components/TransitFrequency';
import { TransitLineDiagram } from '@/components/TransitLineDiagram';
import { TransitStatusBadge } from '@/components/TransitStatusBadge';
import { getPreferences, getTransitLine, getTransitNetwork } from '@/lib/api';
import { formatDate } from '@/lib/dateFormat';
import { pageMetadata, previewImageAlt } from '@/lib/pageMetadata';
import { lineColour, orNotFound, TRANSIT_ID } from '@/lib/transit';
import { buildLineDiagram } from '@/lib/transitLineDiagram';
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
 * last departures, its stops drawn once as a route strip with its
 * branches (`TransitLineDiagram`; trains run both ways along it), and links
 * to its timetable each way and by branch. The star pins it (`statusLineId`: its public id,
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
  const diagram = buildLineDiagram(lineDetail);
  const timetableHref = (query: Record<string, string>) =>
    `/transit/${network}/lines/${line}/timetable?${new URLSearchParams(query).toString()}`;
  const branches = lineDetail.branches ?? [];
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

      {diagram.stops.length > 0 && (
        <Stack gap="sm">
          <SectionTitle>Stops</SectionTitle>
          <Text size="sm" c="dimmed">
            {diagram.from && diagram.to && diagram.from.id !== diagram.to.id
              ? `${diagram.from.name} to ${diagram.to.name}, both ways. `
              : 'Both ways. '}
            {branches.length > 1 || diagram.stops.some((stop) => stop.spurs.length > 0)
              ? 'Each branch hangs off the stop where it splits or joins.'
              : ''}
          </Text>
          {/* Direction is a choice of timetable, not a second list of stops. */}
          <Group gap="md" component="ul" className="ds-plain-list" aria-label="Timetable each way">
            {lineDetail.directions.map((direction) => (
              <li key={direction.direction}>
                <TextLink href={timetableHref({ direction: direction.direction })} underline="always">
                  {`Timetable: ${headingOf(direction).replace(/^Towards/, 'towards')}`}
                </TextLink>
              </li>
            ))}
          </Group>
          <TransitLineDiagram
            network={network}
            diagram={diagram}
            colour={lineColour(lineDetail)}
            label={`${lineDetail.name} stops${diagram.from && diagram.to ? `, ${diagram.from.name} to ${diagram.to.name}` : ''}`}
            idPrefix="line-diagram"
          />
          {branches.length > 1 && (
            <Accordion variant="separated" chevronPosition="left">
              <AccordionItem value="branches">
                <AccordionControl>
                  <Text fw={500}>Timetables by branch</Text>
                  <Text size="xs" c="dimmed">
                    {branches.length} branches
                  </Text>
                </AccordionControl>
                <AccordionPanel>
                  <Stack component="ul" gap={4} className="ds-plain-list">
                    {branches.map((branch) => (
                      <li key={branch.id}>
                        <TextLink href={timetableHref({ direction: branch.direction, branch: branch.id })}>
                          {branch.name}
                        </TextLink>{' '}
                        <Text span size="xs" c="dimmed">
                          {branch.stations.length} stops, {branch.sharePercent}% of trips
                        </Text>
                      </li>
                    ))}
                  </Stack>
                </AccordionPanel>
              </AccordionItem>
            </Accordion>
          )}
        </Stack>
      )}
      {lineDetail.directions.length === 0 && <Text c="dimmed">Nothing runs on this line in the timetable.</Text>}

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
