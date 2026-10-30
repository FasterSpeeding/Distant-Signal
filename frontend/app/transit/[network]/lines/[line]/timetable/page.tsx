import { Group, Stack, Text, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { TextLink } from '@/components/TextLink';
import { TransitAttributionNote } from '@/components/TransitAttributionNote';
import { TransitLineTimetable } from '@/components/TransitLineTimetable';
import { getTransitLine, getTransitLineTimetable, getTransitNetwork } from '@/lib/api';
import { formatDate } from '@/lib/dateFormat';
import { pageMetadata, previewImageAlt } from '@/lib/pageMetadata';
import { orNotFound, TRANSIT_ID } from '@/lib/transit';
import type { TransitDirection } from '@/lib/types';

interface Params {
  params: Promise<{ network: string; line: string }>;
}

interface Search {
  searchParams?: Promise<Record<string, string | string[] | undefined>>;
}

const DATE = /^\d{4}-\d{2}-\d{2}$/;
const HOUR = 3600;
/** The window shown, and how far "Earlier"/"Later" move it. */
const WINDOW = 3 * HOUR;
const DAY = 24 * HOUR;

function single(value: string | string[] | undefined): string | undefined {
  return Array.isArray(value) ? value[0] : value;
}

/** `HH:MM` as seconds after midnight. */
function seconds(hhmm: string | undefined): number | undefined {
  const match = hhmm ? /^(\d{2}):(\d{2})$/.exec(hhmm) : null;
  if (!match) return undefined;
  const value = Number(match[1]) * HOUR + Number(match[2]) * 60;
  return value < DAY ? value : undefined;
}

function hhmm(value: number): string {
  return `${String(Math.floor(value / HOUR)).padStart(2, '0')}:${String(Math.floor((value % HOUR) / 60)).padStart(2, '0')}`;
}

/** Today in the UK, and the start of the current hour less one hour. */
function londonNow(): { date: string; fromSeconds: number } {
  const parts = new Intl.DateTimeFormat('en-GB', {
    timeZone: 'Europe/London',
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    hourCycle: 'h23',
  }).formatToParts(new Date());
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? '00';
  const hour = Number(get('hour'));
  return { date: `${get('year')}-${get('month')}-${get('day')}`, fromSeconds: Math.max(0, hour - 1) * HOUR };
}

async function load(network: string, line: string) {
  if (!TRANSIT_ID.test(network) || !TRANSIT_ID.test(line)) notFound();
  return Promise.all([orNotFound(getTransitNetwork(network)), orNotFound(getTransitLine(network, line))]);
}

export async function generateMetadata({ params }: Params): Promise<Metadata> {
  const { network, line } = await params;
  const [, lineDetail] = await load(network, line);
  return pageMetadata(`${lineDetail.name} timetable`, `Scheduled ${lineDetail.name} times, stop by stop.`, {
    path: `/transit/${encodeURIComponent(network)}/lines/${encodeURIComponent(line)}/timetable`,
    imageAlt: previewImageAlt(`${lineDetail.name} timetable`),
  });
}

/** `/transit/[network]/lines/[line]/timetable`: one direction's scheduled
 * trams in a three-hour window. `?date=`, `?direction=` and `?from=`
 * (`HH:MM`) pick what is shown; the links move between them. */
export default async function TransitTimetablePage({ params, searchParams }: Params & Search) {
  const { network, line } = await params;
  const search = (await searchParams) ?? {};
  const [detail, lineDetail] = await load(network, line);
  const now = londonNow();
  const dateParam = single(search.date);
  const date = dateParam && DATE.test(dateParam) ? dateParam : now.date;
  const direction: TransitDirection = single(search.direction) === 'inbound' ? 'inbound' : 'outbound';
  const from = seconds(single(search.from)) ?? (date === now.date ? now.fromSeconds : 6 * HOUR);
  const to = Math.min(from + WINDOW, DAY);
  const timetable = await orNotFound(
    getTransitLineTimetable(network, line, { date, direction, from: hhmm(from), to: hhmm(to) }),
  );

  const base = `/transit/${network}/lines/${line}/timetable`;
  const href = (changes: { direction?: TransitDirection; from?: number }) =>
    `${base}?${new URLSearchParams({
      date,
      direction: changes.direction ?? direction,
      from: hhmm(changes.from ?? from),
    }).toString()}`;
  const other = lineDetail.directions.find((d) => d.direction !== direction);
  const current = lineDetail.directions.find((d) => d.direction === direction);

  return (
    <Stack p="lg" gap="lg">
      <Stack gap={4}>
        <Title order={1}>{lineDetail.name} timetable</Title>
        <Group gap="xs" c="dimmed" fz="sm">
          <TextLink href={`/transit/${network}/lines/${line}`}>{lineDetail.name}</TextLink>
          <TextLink href={`/transit/${network}`}>{detail.name}</TextLink>
        </Group>
      </Stack>

      <Stack gap={4}>
        <Text fw={600}>
          {current?.headsign ? `Towards ${current.headsign}` : 'This direction'}, {formatDate(date)}, {hhmm(from)}–
          {hhmm(to === DAY ? DAY - 60 : to)}
        </Text>
        <Group gap="md">
          {other && (
            <TextLink href={href({ direction: other.direction })}>
              {other.headsign ? `Towards ${other.headsign}` : 'Other direction'}
            </TextLink>
          )}
          {from > 0 && <TextLink href={href({ from: Math.max(0, from - WINDOW) })}>Earlier</TextLink>}
          {to < DAY && <TextLink href={href({ from: Math.min(from + WINDOW, DAY - WINDOW) })}>Later</TextLink>}
        </Group>
      </Stack>

      <TransitLineTimetable network={network} timetable={timetable} />

      <TransitAttributionNote network={detail} />
    </Stack>
  );
}
