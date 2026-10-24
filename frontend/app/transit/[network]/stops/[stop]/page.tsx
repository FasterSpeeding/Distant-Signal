import { Group, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitStopNotices } from '@/components/TransitDisruptions';
import { StopRailLinks } from '@/components/TransitRailLinks';
import { TransitStopBoard } from '@/components/TransitStopBoard';
import { getTransitDepartures, getTransitNetwork, getTransitNetworks, getTransitStop } from '@/lib/api';
import { formatDate } from '@/lib/dateFormat';
import { pageMetadata } from '@/lib/pageMetadata';
import { localLineId, orNotFound, TRANSIT_ID, TRANSIT_STOP_ID } from '@/lib/transit';

interface Params {
  params: Promise<{ network: string; stop: string }>;
}

interface Search {
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

async function load(network: string, stop: string) {
  if (!TRANSIT_ID.test(network) || !TRANSIT_STOP_ID.test(stop)) notFound();
  return Promise.all([orNotFound(getTransitNetwork(network)), orNotFound(getTransitStop(network, stop))]);
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network, stop } = await params;
  const [detail, stopDetail] = await load(network, stop);
  return pageMetadata(`${stopDetail.name}, ${detail.name}`, `Scheduled tram departures from ${stopDetail.name}.`);
}

/** `/transit/[network]/stops/[stop]`: a tram stop's scheduled departures
 * and the lines that call there. `?date=` picks another day, `?cursor=`
 * the next page. */
export default async function TransitStopPage({ params, searchParams }: Params & Search) {
  const { network, stop } = await params;
  const search = (await searchParams) ?? {};
  const date = single(search.date);
  const cursor = single(search.cursor);
  const [[detail, stopDetail], networks] = await Promise.all([load(network, stop), getTransitNetworks()]);
  // Only a network the trip planner routes over can be planned from
  // (`tripPlanning`, on the networks list; transit trip planning §5.2).
  const plannable = (detail.tripPlanning ?? networks.find((n) => n.id === network)?.tripPlanning) === true;
  const board = await orNotFound(
    getTransitDepartures(network, stop, {
      date: date && DATE.test(date) ? date : undefined,
      cursor: cursor && cursor.length <= 256 ? cursor : undefined,
    }),
  );
  const lineNames = new Map(detail.lines.map((line) => [line.id, line]));
  const base = `/transit/${network}/stops/${stop}`;
  const laterHref = board.nextCursor
    ? `${base}?${new URLSearchParams({ date: board.date, cursor: board.nextCursor }).toString()}`
    : null;

  return (
    <Stack p="lg" gap="lg">
      <Stack gap={4}>
        <Group justify="space-between">
          <Title order={1}>{stopDetail.name}</Title>
          {plannable && (
            <TextLink href={`/plan?${new URLSearchParams({ origin: stopDetail.id }).toString()}`}>
              Plan a journey from here
            </TextLink>
          )}
        </Group>
        <Group gap="xs" c="dimmed" fz="sm">
          <TextLink href={`/transit/${network}`}>{detail.name}</TextLink>
          {stopDetail.lines.map((id) => {
            const line = lineNames.get(id);
            return (
              <TextLink key={id} href={`/transit/${network}/lines/${localLineId(id)}`}>
                {line?.name ?? localLineId(id)}
              </TextLink>
            );
          })}
        </Group>
      </Stack>

      <StopRailLinks links={stopDetail.railLinks} />

      <TransitStopNotices notices={stopDetail.notices} />

      <Stack gap="sm" id="departures">
        <SectionTitle>Departures</SectionTitle>
        <Text size="sm">{formatDate(board.date)}</Text>
        <TransitStopBoard board={board} laterHref={laterHref} />
        <Group>
          <TextLink href={`${base}?date=${nextDay(board.date)}`}>Next day</TextLink>
        </Group>
      </Stack>

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
