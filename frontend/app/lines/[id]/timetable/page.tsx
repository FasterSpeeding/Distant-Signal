import Link from 'next/link';
import { notFound } from 'next/navigation';
import type { Metadata } from 'next';
import { Group, Paper, Stack, Text, Title, VisuallyHidden } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import { TextLink } from '@/components/TextLink';
import { ApiNotFoundError, getAllLines, getLineStatus, getLineTimetable } from '@/lib/api';
import { createLogger } from '@/lib/logger';
import { formatDate, londonDayKey, TIMES_IN_UK_LOCAL_TIME } from '@/lib/dateFormat';
import {
  directionLabel,
  directionTabs,
  formatApiMinute,
  formatClock,
  parseTimetableParams,
  stationName,
  timetableHref,
  timetableParamsForHref,
  timetableRowTimes,
  fromStationView,
  type TimetablePageParams,
} from '@/lib/lineTrains';
import type { LineCatalogueStation, LineTimetablePage } from '@/lib/types';
import { LineTrainRow } from '../LineTrainRow';
import classes from '../LineTrains.module.css';
import { TimetableMore, type TimetableQuery } from './TimetableMore';

const log = createLogger('app/lines/timetable');

// Dynamic: the default date is "today in London" and the rows carry live
// status.
export const revalidate = 0;

/** Trains per page. */
const TIMETABLE_PAGE_SIZE = 50;

/** The dates offered: as far back as line populations are kept (three
 * days) and the next day, which `schedule-reference` publishes ahead. */
const DAYS_BACK = 3;
const DAYS_AHEAD = 1;

/** The line's name for the heading: its status report's, else the line
 * list's (which always has the catalogue lines and the caller's own), else
 * the id (the timetable itself still decides whether there is anything to
 * show). */
async function resolveLineName(id: string): Promise<string> {
  try {
    const [report] = await getLineStatus([id], false);
    if (report?.name) return report.name;
  } catch {
    // No status row yet is routine; the line list knows the name.
  }
  try {
    return (await getAllLines()).find((line) => line.id === id)?.name ?? id;
  } catch (err) {
    log.warn('Could not resolve a line name; falling back to the id.', { line_id: id, error: err });
    return id;
  }
}

/** `YYYY-MM-DD` plus `days`, as a calendar date (no time zone involved). */
function addDays(date: string, days: number): string {
  const d = new Date(`${date}T12:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return d.toISOString().slice(0, 10);
}

/** The date choices, each labelled relative to today. */
function dateOptions(today: string): { value: string; label: string }[] {
  const options: { value: string; label: string }[] = [];
  for (let offset = -DAYS_BACK; offset <= DAYS_AHEAD; offset += 1) {
    const value = addDays(today, offset);
    const long = formatDate(`${value}T12:00:00Z`);
    const relative = offset === 0 ? 'Today' : offset === 1 ? 'Tomorrow' : offset === -1 ? 'Yesterday' : null;
    options.push({ value, label: relative ? `${relative} (${long})` : long });
  }
  return options;
}

export async function generateMetadata({ params }: { params: Promise<{ id: string }> }): Promise<Metadata> {
  const { id } = await params;
  if (!/^[a-z0-9-]+$/.test(id)) notFound();
  const title = `Timetable: ${await resolveLineName(id)} — Distant Signal`;
  return { title, openGraph: { title, type: 'website' } };
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <Paper withBorder p="md">
      <Text c="dimmed">{children}</Text>
    </Paper>
  );
}

/** The filters: a plain GET form, so it works without JavaScript. The
 * direction is kept (the chips change it); the cursor is not (a new filter
 * starts from the top). */
function Filters({
  id,
  params,
  date,
  today,
  stations,
}: {
  id: string;
  params: TimetablePageParams;
  date: string;
  today: string;
  stations: LineCatalogueStation[];
}) {
  const options = stations.filter((s, i) => stations.findIndex((o) => o.crs === s.crs) === i);
  return (
    <form action={`/lines/${encodeURIComponent(id)}/timetable`} method="get" className={classes.picker}>
      {params.dir && <input type="hidden" name="dir" value={params.dir} />}
      <label>
        <Text span size="sm" display="block">
          Date
        </Text>
        <select name="date" defaultValue={date}>
          {dateOptions(today).map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      </label>
      <label>
        <Text span size="sm" display="block">
          From
        </Text>
        <select name="from" defaultValue={params.from ?? ''}>
          <option value="">Any station</option>
          {options.map((s) => (
            <option key={s.crs} value={s.crs}>
              {s.name ?? s.crs}
            </option>
          ))}
        </select>
      </label>
      <label>
        <Text span size="sm" display="block">
          To
        </Text>
        <select name="to" defaultValue={params.to ?? ''}>
          <option value="">Any station</option>
          {options.map((s) => (
            <option key={s.crs} value={s.crs}>
              {s.name ?? s.crs}
            </option>
          ))}
        </select>
      </label>
      <label>
        <Text span size="sm" display="block">
          From time
        </Text>
        <input
          type="time"
          name="at"
          defaultValue={params.at !== null && params.at < 24 * 60 ? formatApiMinute(params.at) : ''}
        />
      </label>
      <label>
        <Text span size="sm" display="block">
          Trains
        </Text>
        <select name="scope" defaultValue={params.scope}>
          <option value="line">This line&apos;s trains</option>
          <option value="shared">Other trains along it</option>
        </select>
      </label>
      <button type="submit" className={classes.chip}>
        Show timetable
      </button>
    </form>
  );
}

/** Direction chips, as on the line page: plain links, the current one
 * `aria-current`, counts for the whole day under the other filters. */
function DirectionChips({ id, params, page }: { id: string; params: TimetablePageParams; page: LineTimetablePage }) {
  const tabs = directionTabs({ line: page.counts[params.scope] ?? {} }, page.stations, page.scopeApplied);
  if (tabs.length === 0) return null;
  return (
    <nav aria-label="Direction">
      <ul className={classes.chips}>
        {tabs.map((tab) => (
          <li key={tab.dir ?? 'all'}>
            <Link
              className={classes.chip}
              href={timetableHref(id, { ...timetableParamsForHref(params), dir: tab.dir })}
              aria-current={params.dir === tab.dir ? 'page' : undefined}
            >
              {tab.label}{' '}
              <span className={classes.count}>
                {tab.count}
                <VisuallyHidden> trains</VisuallyHidden>
              </span>
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}

type Loaded = { page: LineTimetablePage } | { error: 'unpublished' | 'unavailable' };

/** `/lines/{id}/timetable`: the line's full day, cursor-paged
 * (docs/superpowers/specs/2026-10-06-line-page-trains-design.md §5).
 *
 * URL parameters: `date`, `dir`, `from`, `to` (stations), `at` (first
 * time listed), `scope` (`line` or `shared`), `after` (a cursor, for the
 * no-JavaScript next-page link). The first page is server-rendered; "Load
 * more" appends the next ones in place. */
export default async function LineTimetablePage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams?: Promise<Record<string, string | string[] | undefined>>;
}) {
  const { id } = await params;
  if (!/^[a-z0-9-]+$/.test(id)) notFound();
  const tp = parseTimetableParams((await searchParams) ?? {});
  // eslint-disable-next-line react-hooks/purity -- server component: one timestamp per request
  const now = Date.now();
  const today = londonDayKey(new Date(now));
  const date = tp.date ?? today;
  const query: TimetableQuery = {
    date,
    dir: tp.dir,
    from: tp.from,
    to: tp.from === tp.to ? null : tp.to,
    at: tp.at === null ? null : formatApiMinute(tp.at),
    scope: tp.scope,
    limit: TIMETABLE_PAGE_SIZE,
  };

  const [name, loaded] = await Promise.all([
    resolveLineName(id),
    getLineTimetable(id, { ...query, after: tp.after })
      .then((page): Loaded => ({ page }))
      .catch((err: unknown): Loaded => {
        if (!(err instanceof ApiNotFoundError)) log.warn('Timetable fetch failed.', { line_id: id, error: err });
        return { error: err instanceof ApiNotFoundError ? 'unpublished' : 'unavailable' };
      }),
  ]);

  const base = timetableParamsForHref(tp);
  const heading = (
    <>
      <TextLink href={`/lines/${encodeURIComponent(id)}`} underline="always">
        ← Back to {name}
      </TextLink>
      <Title order={1}>Timetable: {name}</Title>
    </>
  );

  if ('error' in loaded) {
    return (
      <Stack p="lg" gap="md">
        {heading}
        <Empty>
          {loaded.error === 'unpublished'
            ? `No timetable is published for this line on ${formatDate(`${date}T12:00:00Z`)}.`
            : 'This timetable isn’t available right now. Please try again shortly.'}
        </Empty>
      </Stack>
    );
  }
  const { page } = loaded;
  const stations = page.stations;
  const fromName = page.from ? stationName(page.from, stations) : null;
  const toName = page.to ? stationName(page.to, stations) : null;
  const total = Object.values(page.counts[tp.scope] ?? {}).reduce<number>((sum, n) => sum + (n ?? 0), 0);
  const dirName = tp.dir ? directionLabel(tp.dir, stations) : null;
  const what = [
    tp.scope === 'shared' ? 'Other trains running along this line' : 'This line’s trains',
    fromName && toName ? `from ${fromName} to ${toName}` : fromName ? `calling at ${fromName}` : null,
    !fromName && toName ? `calling at ${toName}` : null,
    dirName ? `${dirName.charAt(0).toLowerCase()}${dirName.slice(1)}` : null,
  ]
    .filter(Boolean)
    .join(', ');
  const timeMeaning = fromName
    ? `Times are departures from ${fromName}`
    : 'Times are when each train is due on the line';
  const nextHref = page.nextCursor ? timetableHref(id, { ...base, after: page.nextCursor }) : null;

  return (
    <Stack p="lg" gap="md">
      {heading}
      <Text size="sm" c="dimmed">
        {formatDate(`${date}T12:00:00Z`)} · {timeMeaning} · {TIMES_IN_UK_LOCAL_TIME}
      </Text>
      <Filters id={id} params={tp} date={date} today={today} stations={stations} />
      <DirectionChips id={id} params={tp} page={page} />

      <Stack gap="xs">
        <Group justify="space-between" gap="xs" wrap="wrap">
          <SectionTitle order={2}>
            {what}
            {tp.at !== null ? ` from ${formatClock(tp.at)}` : ''}
          </SectionTitle>
          {(tp.at !== null || tp.after !== null) && (
            <TextLink href={timetableHref(id, { ...base, at: null })}>Show the whole day</TextLink>
          )}
        </Group>
        {page.scopeApplied && total > 0 && (
          <Text size="sm" c="dimmed">
            {total} train{total === 1 ? '' : 's'} that day{dirName ? ', all directions' : ''}.
          </Text>
        )}
        {!page.scopeApplied && (
          <Text size="sm" c="dimmed">
            This day&apos;s timetable doesn&apos;t say which trains are this line&apos;s own, so every train calling
            here is listed.
          </Text>
        )}
        {page.trains.length === 0 ? (
          <Empty>
            No trains match these filters.{' '}
            <TextLink href={timetableHref(id, { date: tp.date })} inline underline="always">
              Clear the filters
            </TextLink>
          </Empty>
        ) : (
          <>
            <ul className={classes.list} aria-label="Trains">
              {page.trains.map((train) => {
                const { time, arrival } = timetableRowTimes(train);
                return (
                  <LineTrainRow
                    key={train.uid}
                    train={fromStationView(train, page.from)}
                    date={date}
                    stations={stations}
                    timeOverride={time}
                    arrival={arrival}
                  />
                );
              })}
            </ul>
            <TimetableMore
              id={id}
              query={query}
              initialCursor={page.nextCursor}
              stations={stations}
              nextHref={nextHref}
            />
          </>
        )}
      </Stack>
    </Stack>
  );
}
