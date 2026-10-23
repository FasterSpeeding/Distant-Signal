import { Alert, Badge, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { ButtonLink } from '@/components/ButtonLink';
import { LastUpdated } from '@/components/LastUpdated';
import { LineStatusCard } from '@/components/LineStatusCard';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { TflArrivalsBoard } from '@/components/TflArrivalsBoard';
import { ApiNotFoundError, getLineStatus, getTflStop, getTflStopArrivals } from '@/lib/api';
import { createLogger } from '@/lib/logger';
import { pageMetadata } from '@/lib/pageMetadata';
import { stationLabel } from '@/lib/stationLabel';
import { journeyFromTflStopHref, TFL_LINE_ID, TFL_STOP_ID, tflLineHref, tflModeBadge } from '@/lib/tflStops';
import type { LineStatusReport, TflStopArrivals, TflStopDetail } from '@/lib/types';

const log = createLogger('app/tfl/stops/[id]');

/** National Rail stations named at most: a hub's own, not every one near. */
const MAX_RAIL_STATIONS = 3;

interface Params {
  params: Promise<{ id: string }>;
}

async function loadStop(id: string): Promise<TflStopDetail> {
  if (!TFL_STOP_ID.test(id)) notFound();
  try {
    return await getTflStop(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) notFound();
    throw err;
  }
}

/** The stop's arrivals, or null when TfL (or the api) couldn't give them:
 * the rest of the page still renders, with a note in their place. */
async function loadArrivals(id: string): Promise<TflStopArrivals | null> {
  try {
    return await getTflStopArrivals(id);
  } catch (err) {
    log.warn('TfL arrivals unavailable', { stopId: id, error: String(err) });
    return null;
  }
}

/** Each of the stop's lines' status (`/Line/tfl-{id},.../Status`), in the
 * stop's line order. A line the api has no status for is left out; a
 * failed fetch leaves them all out (the section then links each line). */
async function loadLineStatus(stop: TflStopDetail): Promise<LineStatusReport[]> {
  const ids = stop.lines.filter((line) => TFL_LINE_ID.test(line.id)).map((line) => `tfl-${line.id}`);
  if (ids.length === 0) return [];
  try {
    const reports = await getLineStatus(ids, false);
    const byId = new Map(reports.map((report) => [report.id, report]));
    return ids.flatMap((id) => byId.get(id) ?? []);
  } catch (err) {
    if (!(err instanceof ApiNotFoundError)) {
      log.warn('TfL line status unavailable', { stopId: stop.id, error: String(err) });
    }
    return [];
  }
}

/** "Underground", "DLR and Tram": the stop's modes, for its description. */
function modesPhrase(stop: TflStopDetail): string {
  const modes = stop.modes.map(tflModeBadge);
  return modes.length > 1 ? `${modes.slice(0, -1).join(', ')} and ${modes[modes.length - 1]}` : (modes[0] ?? 'TfL');
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { id } = await params;
  const stop = await loadStop(id);
  return pageMetadata(stop.name, `Live arrivals and line status at ${stop.name} (${modesPhrase(stop)}).`);
}

/** `/tfl/stops/[id]` (DS-136): a TfL Tube, DLR or tram stop. Its live
 * arrivals by line and platform (from TfL, through the api's short cache),
 * its lines' status, the National Rail station there if there is one, and
 * a way to start a journey leg from it. Server-rendered; the root layout's
 * `AutoRefresh` refreshes it every 30 s. */
export default async function TflStopPage({ params }: Params) {
  const { id } = await params;
  const stop = await loadStop(id);
  const [arrivals, lineStatus] = await Promise.all([loadArrivals(stop.id), loadLineStatus(stop)]);
  const reported = new Set(lineStatus.map((report) => report.id));
  const unreported = stop.lines.filter((line) => TFL_LINE_ID.test(line.id) && !reported.has(`tfl-${line.id}`));
  // The stations at the stop, best first (King's Cross St. Pancras has two).
  const rail = stop.railStations.slice(0, MAX_RAIL_STATIONS);

  return (
    <Stack p="lg" gap="lg">
      <Stack gap="xs">
        <Title order={1}>{stop.name}</Title>
        <Group gap="xs">
          {stop.modes.map((mode) => (
            <Badge key={mode} variant="light" tt="none">
              {tflModeBadge(mode)}
            </Badge>
          ))}
        </Group>
        {rail.length > 0 && (
          <Text size="sm">
            {rail.length > 1 ? 'National Rail stations here: ' : 'National Rail station here: '}
            {rail.map((station, index) => (
              <span key={station.crs}>
                {index > 0 && ', '}
                <TextLink href={`/stations/${encodeURIComponent(station.crs)}`} underline="always" inline>
                  {stationLabel(station.crs, station.name)}
                </TextLink>
              </span>
            ))}
          </Text>
        )}
        <Group>
          <ButtonLink href={journeyFromTflStopHref(stop.id)} variant="default">
            Add as a journey leg
          </ButtonLink>
        </Group>
      </Stack>

      <Stack gap="sm" component="section" aria-labelledby="live-arrivals">
        <SectionTitle id="live-arrivals">Live arrivals</SectionTitle>
        {arrivals ? (
          <>
            <LastUpdated timestamp={arrivals.fetchedAt} />
            <TflArrivalsBoard arrivals={arrivals} />
          </>
        ) : (
          <Alert color="red" title="Couldn't load live arrivals">
            TfL isn&apos;t answering right now. This page tries again every 30 seconds.
          </Alert>
        )}
      </Stack>

      {stop.lines.length > 0 && (
        <Stack gap="sm" component="section" aria-labelledby="line-status">
          <SectionTitle id="line-status">Line status</SectionTitle>
          {lineStatus.length > 0 && (
            <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
              {lineStatus.map((report) => (
                // One "Updated" on the page, the arrivals' (docs/style-guide.md,
                // "Data display").
                <LineStatusCard key={report.id} report={report} showUpdated={false} />
              ))}
            </SimpleGrid>
          )}
          {unreported.length > 0 && (
            <Text size="sm">
              {lineStatus.length > 0 ? 'Also here: ' : 'Lines here: '}
              {unreported.map((line, index) => (
                <span key={line.id}>
                  {index > 0 && ', '}
                  <TextLink href={tflLineHref(line.id)} underline="always" inline>
                    {line.name}
                  </TextLink>
                </span>
              ))}
            </Text>
          )}
        </Stack>
      )}
    </Stack>
  );
}
