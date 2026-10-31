import { Alert, Badge, Group, SimpleGrid, Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { ButtonLink } from '@/components/ButtonLink';
import { LastUpdated } from '@/components/LastUpdated';
import { LineStatusCard } from '@/components/LineStatusCard';
import { LinkSentence } from '@/components/LinkSentence';
import { SectionTitle } from '@/components/SectionTitle';
import { TflArrivalsBoard } from '@/components/TflArrivalsBoard';
import { TflStopTimetable } from '@/components/TflStopTimetable';
import {
  ApiNotFoundError,
  getLineStatus,
  getTflStop,
  getTflStopArrivals,
  getTransitDepartures,
  getTransitNetworks,
  getTransitStop,
} from '@/lib/api';
import { createLogger } from '@/lib/logger';
import { pageMetadata, previewImageAlt } from '@/lib/pageMetadata';
import { stationLabel } from '@/lib/stationLabel';
import { journeyFromTflStopHref, TFL_LINE_ID, TFL_STOP_ID, tflLineHref, tflModeBadge } from '@/lib/tflStops';
import type { LineStatusReport, TflStopArrivals, TflStopDetail, TransitBoard, TransitStopDetail } from '@/lib/types';

const log = createLogger('app/tfl/stops/[id]');

/** National Rail stations named at most: a hub's own, not every one near. */
const MAX_RAIL_STATIONS = 3;

interface Params {
  params: Promise<{ id: string }>;
}

interface Search {
  /** The timetable's day and page (`?date=`, `?cursor=`). */
  searchParams?: Promise<{ date?: string | string[]; cursor?: string | string[] }>;
}

const DATE = /^\d{4}-\d{2}-\d{2}$/;

function single(value: string | string[] | undefined): string | undefined {
  return Array.isArray(value) ? value[0] : value;
}

/** The day after `date` (`YYYY-MM-DD`). */
function nextDay(date: string): string {
  const day = new Date(`${date}T12:00:00Z`);
  day.setUTCDate(day.getUTCDate() + 1);
  return day.toISOString().slice(0, 10);
}

/** The TfL network's timetable at the stop (the TfL transit design §8.4):
 * its station and the scheduled board for `date` (default today) from
 * `cursor`, while the api serves the network and the stop is in its
 * timetable; null otherwise, or on any failure (live arrivals stay the
 * page's point). */
async function loadTimetable(
  id: string,
  date: string | undefined,
  cursor: string | undefined,
): Promise<{ station: TransitStopDetail; board: TransitBoard } | null> {
  try {
    const networks = await getTransitNetworks();
    if (!networks.some((network) => network.id === 'tfl')) return null;
    const [station, board] = await Promise.all([
      getTransitStop('tfl', id, date),
      getTransitDepartures('tfl', id, { date, cursor, limit: '10' }),
    ]);
    return { station, board };
  } catch (err) {
    if (!(err instanceof ApiNotFoundError)) {
      log.warn('TfL timetable unavailable', { stopId: id, error: String(err) });
    }
    return null;
  }
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
  return pageMetadata(stop.name, `Live arrivals and line status at ${stop.name} (${modesPhrase(stop)}).`, {
    path: `/tfl/stops/${encodeURIComponent(stop.id)}`,
    imageAlt: previewImageAlt(stop.name),
  });
}

/** `/tfl/stops/[id]` (DS-136): a TfL Tube, DLR or tram stop. Its live
 * arrivals by line and platform (from TfL, through the api's short cache),
 * its timetable while the TfL network is served (first and last trains,
 * scheduled departures; `?date=` and `?cursor=` page it), its lines'
 * status, the National Rail station there if there is one, and a way to
 * start a journey leg from it. Server-rendered; the root layout's
 * `AutoRefresh` refreshes it every 30 s. */
export default async function TflStopPage({ params, searchParams }: Params & Search) {
  const { id } = await params;
  const search = (await searchParams) ?? {};
  const dateParam = single(search.date);
  const cursorParam = single(search.cursor);
  const stop = await loadStop(id);
  const [arrivals, lineStatus, timetable] = await Promise.all([
    loadArrivals(stop.id),
    loadLineStatus(stop),
    loadTimetable(
      stop.id,
      dateParam && DATE.test(dateParam) ? dateParam : undefined,
      cursorParam && cursorParam.length <= 256 ? cursorParam : undefined,
    ),
  ]);
  const base = `/tfl/stops/${encodeURIComponent(stop.id)}`;
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
        <LinkSentence
          lead={rail.length > 1 ? 'National Rail stations here: ' : 'National Rail station here: '}
          links={rail.map((station) => ({
            href: `/stations/${encodeURIComponent(station.crs)}`,
            label: stationLabel(station.crs, station.name),
          }))}
        />
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

      {timetable && (
        <TflStopTimetable
          firstLast={timetable.station.firstLast ?? []}
          dated={dateParam !== undefined && DATE.test(dateParam)}
          board={timetable.board}
          laterHref={
            timetable.board.nextCursor
              ? `${base}?${new URLSearchParams({ date: timetable.board.date, cursor: timetable.board.nextCursor }).toString()}#timetable`
              : null
          }
          nextDayHref={`${base}?date=${nextDay(timetable.board.date)}#timetable`}
        />
      )}

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
          <LinkSentence
            lead={lineStatus.length > 0 ? 'Also here: ' : 'Lines here: '}
            links={unreported.map((line) => ({ href: tflLineHref(line.id), label: line.name }))}
          />
        </Stack>
      )}
    </Stack>
  );
}
