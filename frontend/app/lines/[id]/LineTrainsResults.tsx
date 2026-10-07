import Link from 'next/link';
import { Group, Paper, Stack, Text, VisuallyHidden } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import { ApiNotFoundError, getLineTrainsSummary, searchTrainsBetween } from '@/lib/api';
import type { LineCatalogueStation, LineTrainSummary, LineTrainsSummary, TrainSearchResult } from '@/lib/types';
import { TextLink } from '@/components/TextLink';
import { ServiceModeBadge } from '@/components/ServiceModeBadge';
import { isTimetableOnly, serviceNoun } from '@/lib/serviceMode';
import { LastUpdated } from '@/components/LastUpdated';
import { formatDate, TIMES_IN_UK_LOCAL_TIME } from '@/lib/dateFormat';
import {
  addCounts,
  DAY_MINUTES,
  directionLabel,
  directionTabs,
  formatApiMinute,
  formatClock,
  groupPatterns,
  groupShared,
  hubStations,
  lineHref,
  lineTimeMinute,
  lineTimetableLink,
  londonMinuteOfDay,
  paramsForHref,
  previousDate,
  previousDayWindow,
  resolveWindow,
  shiftToNextDay,
  sortByLineTime,
  splitRunning,
  stationName,
  type LinePageParams,
  type TrainWindow,
} from '@/lib/lineTrains';
import { LineTrainRow } from './LineTrainRow';
import classes from './LineTrains.module.css';

const NO_PARAMS: LinePageParams = { dir: null, at: null, from: null, to: null, view: null };

/** The trains a station pair search can list in one go. */
const PAIR_LIMIT = 60;

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <Paper withBorder p="md">
      <Text c="dimmed">{children}</Text>
    </Paper>
  );
}

/** The direction chips: plain links (no JavaScript), the current one
 * marked `aria-current`. */
function DirectionChips({ id, params, summary }: { id: string; params: LinePageParams; summary: LineTrainsSummary }) {
  const tabs = directionTabs(summary.counts, summary.stations, summary.scopeApplied);
  if (tabs.length === 0) return null;
  return (
    <nav aria-label="Direction">
      <ul className={classes.chips}>
        {tabs.map((tab) => (
          <li key={tab.dir ?? 'all'}>
            <Link
              className={classes.chip}
              href={lineHref(id, { ...paramsForHref(params), dir: tab.dir })}
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

/** "By time" / "By route" -- the same trains, listed or grouped by pattern. */
function ViewChips({ id, params }: { id: string; params: LinePageParams }) {
  const options = [
    { view: null, label: 'By time' },
    { view: 'routes', label: 'By route' },
  ] as const;
  return (
    <nav aria-label="Group trains">
      <ul className={classes.chips}>
        {options.map((option) => (
          <li key={option.label}>
            <Link
              className={classes.chip}
              href={lineHref(id, { ...paramsForHref(params), view: option.view })}
              aria-current={params.view === option.view ? 'page' : undefined}
            >
              {option.label}
            </Link>
          </li>
        ))}
      </ul>
    </nav>
  );
}

function plural(count: number): string {
  return `${count} train${count === 1 ? '' : 's'}`;
}

/** " (into tomorrow)" when a window starting today ends after midnight. */
function intoTomorrow(from: number, to: number): string {
  return to > DAY_MINUTES && from < DAY_MINUTES ? ' (into tomorrow)' : '';
}

/** The window in words, and the Earlier / Now / Later links -- desktop and
 * phone each get their own (the phone window is an hour, not two). */
function WindowNav({
  id,
  params,
  window,
  count,
  phoneCount,
}: {
  id: string;
  params: LinePageParams;
  window: TrainWindow;
  count: number;
  phoneCount: number;
}) {
  const base = paramsForHref(params);
  const at = (minute: number) => lineHref(id, { ...base, at: formatApiMinute(minute) });
  return (
    <Group justify="space-between" gap="xs" wrap="wrap">
      <Text size="sm">
        Due on the line {formatClock(window.from)}–
        <span className={classes.desktopOnly}>
          {formatClock(window.to)}
          {intoTomorrow(window.from, window.to)} · {plural(count)}
        </span>
        <span className={classes.phoneOnly}>
          {formatClock(window.phoneTo)}
          {intoTomorrow(window.from, window.phoneTo)} · {plural(phoneCount)}
        </span>
      </Text>
      <Group gap="md" wrap="wrap">
        <span className={classes.desktopOnly}>
          <TextLink href={at(window.earlierDesktop)} ariaLabel="Earlier trains">
            ← Earlier
          </TextLink>
        </span>
        <span className={classes.phoneOnly}>
          <TextLink href={at(window.earlierPhone)} ariaLabel="Earlier trains">
            ← Earlier
          </TextLink>
        </span>
        {params.at !== null && (
          <TextLink href={lineHref(id, { ...base, at: null })} ariaLabel="Trains due now">
            Now
          </TextLink>
        )}
        <span className={classes.desktopOnly}>
          <TextLink href={at(window.laterDesktop)} ariaLabel="Later trains">
            Later →
          </TextLink>
        </span>
        <span className={classes.phoneOnly}>
          <TextLink href={at(window.laterPhone)} ariaLabel="Later trains">
            Later →
          </TextLink>
        </span>
      </Group>
    </Group>
  );
}

/** A train's service date: the page's, or the previous day's for a
 * train that started before midnight (see `LineTrainsResults`). */
type DateFor = (train: LineTrainSummary) => string;

function TrainList({
  trains,
  dateFor,
  stations,
  window,
  label,
}: {
  trains: LineTrainSummary[];
  dateFor: DateFor;
  stations: LineCatalogueStation[];
  window?: TrainWindow;
  label: string;
}) {
  return (
    <ul className={classes.list} aria-label={label}>
      {trains.map((train) => {
        const minute = lineTimeMinute(train.lineDue);
        const beyondPhone = window !== undefined && minute !== null && minute >= window.phoneTo;
        const date = dateFor(train);
        return (
          <LineTrainRow
            key={`${date}-${train.uid}`}
            train={train}
            date={date}
            stations={stations}
            beyondPhone={beyondPhone}
          />
        );
      })}
    </ul>
  );
}

/** Trains grouped by stopping pattern, each an accordion (native
 * `<details>`, keyboard- and screen-reader-accessible without script). */
function PatternGroups({
  trains,
  dateFor,
  stations,
}: {
  trains: LineTrainSummary[];
  dateFor: DateFor;
  stations: LineCatalogueStation[];
}) {
  const groups = groupPatterns(trains);
  return (
    <Stack gap="xs">
      {groups.map((group) => (
        <details key={group.key} className={classes.details}>
          <summary>
            <Text span fw={500}>
              {group.route}
              {group.kind ? `, ${group.kind}` : ''}
            </Text>
            <Text span size="sm" c="dimmed">
              {' '}
              · {group.frequency} · show {group.trains.length} train{group.trains.length === 1 ? '' : 's'}
            </Text>
          </summary>
          <div className={classes.detailsBody}>
            <TrainList trains={group.trains} dateFor={dateFor} stations={stations} label={group.route} />
          </div>
        </details>
      ))}
    </Stack>
  );
}

/** "Also running along part of this line (N)": other routes' and other
 * operators' trains, grouped by operator and route, each train a time
 * linking to its page. Collapsed by default. */
function SharedGroup({
  trains,
  dateFor,
  operatorName,
}: {
  trains: LineTrainSummary[];
  dateFor: DateFor;
  operatorName: (code: string) => string;
}) {
  if (trains.length === 0) return null;
  const groups = groupShared(trains, operatorName);
  return (
    <details className={classes.details}>
      <summary>
        <Text span fw={500}>
          Also running along part of this line ({trains.length})
        </Text>
      </summary>
      <Stack gap="sm" className={classes.detailsBody}>
        {groups.map((group) => (
          <div key={group.key}>
            <Text size="sm" fw={500}>
              {group.operator} · {group.route}
            </Text>
            <ul className={classes.timeLinks}>
              {group.trains.map((train) => {
                const minute = lineTimeMinute(train.lineDue);
                const time = minute === null ? '--:--' : formatClock(minute);
                return (
                  <li key={`${dateFor(train)}-${train.uid}`}>
                    <TextLink
                      href={`/train/${encodeURIComponent(train.uid)}/${dateFor(train)}`}
                      ariaLabel={`${time} ${group.operator} ${serviceNoun(train.serviceMode).toLowerCase()} to ${train.destination?.name ?? train.destination?.crs ?? 'unknown'}`}
                    >
                      {time}
                    </TextLink>
                    {isTimetableOnly(train) && (
                      <>
                        {' '}
                        <ServiceModeBadge mode={train.serviceMode} />
                      </>
                    )}
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </Stack>
    </details>
  );
}

/** "Other trains at <station> →": the line's hubs, where the trains that
 * only touch the line (not listed here) call -- their station timetables. */
function HubLinks({ stations }: { stations: LineCatalogueStation[] }) {
  const hubs = hubStations(stations);
  if (hubs.length === 0) return null;
  return (
    <Group gap="md" wrap="wrap">
      {hubs.map((hub) => (
        <TextLink key={hub.crs} href={`/stations/${hub.crs}#departures`}>
          Other trains at {hub.name ?? hub.crs} →
        </TextLink>
      ))}
    </Group>
  );
}

/** The optional From/To station picker: a plain GET form, so it works
 * without JavaScript. */
function StationPicker({
  id,
  params,
  stations,
}: {
  id: string;
  params: LinePageParams;
  stations: LineCatalogueStation[];
}) {
  const options = stations.filter((s, i) => stations.findIndex((o) => o.crs === s.crs) === i);
  return (
    <form action={`/lines/${encodeURIComponent(id)}#trains`} method="get" className={classes.picker}>
      {params.at !== null && <input type="hidden" name="at" value={formatApiMinute(params.at)} />}
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
      <button type="submit" className={classes.chip}>
        Show trains
      </button>
    </form>
  );
}

/** A station-pair search row as a summary row, with the live status the
 * line summary has for the same train, when it has one. */
function pairRow(row: TrainSearchResult, live: Map<string, LineTrainSummary>): LineTrainSummary {
  const known = live.get(row.uid);
  return {
    uid: row.uid,
    operator: row.operator ?? null,
    serviceMode: row.serviceMode ?? known?.serviceMode ?? 'train',
    liveTracking: row.liveTracking ?? known?.liveTracking ?? null,
    scope: known?.scope ?? null,
    direction: known?.direction ?? null,
    lineDue: null,
    origin: row.originCrs ? { crs: row.originCrs, name: null } : null,
    destination: row.destinationCrs ? { crs: row.destinationCrs, name: row.destinationName ?? null } : null,
    onLineStops: [],
    live: known?.live ?? null,
  };
}

type PairRows = { rows: TrainSearchResult[] } | { error: 'unpublished' | 'unavailable' };

/** Trains calling at `from` and later at `to` in the window, from the
 * indexed station search (`GET /public/trains/search`). */
async function loadPairRows(date: string, pair: { from: string; to: string }, window: TrainWindow): Promise<PairRows> {
  try {
    const page = await searchTrainsBetween({
      station: pair.from,
      stopsAt: pair.to,
      date,
      from: formatClock(Math.min(window.from, DAY_MINUTES - 1)),
      to: window.to >= DAY_MINUTES ? '23:59' : formatClock(window.to),
      limit: PAIR_LIMIT,
    });
    return { rows: page.results };
  } catch (err) {
    return { error: err instanceof ApiNotFoundError ? 'unpublished' : 'unavailable' };
  }
}

function StationPairResults({
  id,
  date,
  params,
  pair,
  window,
  summary,
  result,
}: {
  id: string;
  date: string;
  params: LinePageParams;
  pair: { from: string; to: string };
  window: TrainWindow;
  summary: LineTrainsSummary;
  result: PairRows;
}) {
  const fromName = stationName(pair.from, summary.stations);
  const toName = stationName(pair.to, summary.stations);
  const clear = lineHref(id, { ...paramsForHref(params), from: null, to: null });
  if ('error' in result) {
    return (
      <Empty>
        {result.error === 'unpublished'
          ? 'No timetable is published for today yet.'
          : 'Trains between these stations aren’t available right now.'}{' '}
        <TextLink href={clear}>Show all trains on this line</TextLink>
      </Empty>
    );
  }
  const live = new Map<string, LineTrainSummary>();
  for (const train of [...summary.trains, ...(summary.running ?? [])]) live.set(train.uid, train);
  return (
    <Stack gap="xs">
      <Group justify="space-between" gap="xs">
        <SectionTitle order={3}>
          {fromName} to {toName}
        </SectionTitle>
        <TextLink href={clear}>Show all trains on this line</TextLink>
      </Group>
      <Text size="sm" c="dimmed">
        Every train calling at {fromName} and later at {toName}, any operator, leaving {fromName} between{' '}
        {formatClock(window.from)} and {formatClock(window.to)}. Times are departures from {fromName}.
      </Text>
      {result.rows.length === 0 ? (
        <Empty>
          No trains from {fromName} to {toName} in this window.
        </Empty>
      ) : (
        <ul className={classes.list} aria-label={`Trains from ${fromName} to ${toName}`}>
          {result.rows.map((row) => (
            <LineTrainRow
              key={row.uid}
              train={pairRow(row, live)}
              date={date}
              stations={summary.stations}
              timeOverride={(row.publicDeparture ?? row.scheduled ?? '--:--').slice(0, 5)}
              showStrip={false}
            />
          ))}
        </ul>
      )}
    </Stack>
  );
}

/** "Trains on this line" (docs/superpowers/specs/2026-10-06-line-page-trains-design.md).
 *
 * Fetches the slim summary view for a window around `params.at` (default:
 * now) -- the line's own trains (`scope=line`) in the main list, sorted by
 * their time on the line; trains that run part of it (`shared`) in a
 * collapsed group; trains that only touch it not at all, linked through
 * the hub stations' timetables instead. With no `at`, a "Running now"
 * section lists the line's trains between their first and last on-line
 * calls.
 *
 * `date` is the caller's London day (see `LineDetailPage`), so the fetch
 * and every row's `/train/{uid}/{date}` link agree. Rendered inside a
 * `<Suspense>`: every failure resolves to markup, never a throw.
 *
 * `now` is a prop so tests can pin the window. */
export async function LineTrainsResults({
  id,
  date,
  now = new Date(),
  params = NO_PARAMS,
  operatorName = (code: string) => code,
}: {
  id: string;
  date: string;
  now?: Date;
  params?: LinePageParams;
  operatorName?: (code: string) => string;
}) {
  const liveView = params.at === null;
  const nowMinute = londonMinuteOfDay(now);
  const window = resolveWindow(params.at ?? nowMinute);
  const pair = params.from && params.to && params.from !== params.to ? { from: params.from, to: params.to } : null;

  const direction = pair ? undefined : (params.dir ?? undefined);
  const at = liveView ? nowMinute : null;
  // Before 03:00 the previous service date's trains are still running
  // (they belong to the date they started on): its window, 24 h on, is
  // fetched alongside today's and merged in. Its failure only loses them.
  const previous = previousDayWindow(window, at);
  const yesterday = previousDate(date);
  const [current, earlier] = await Promise.allSettled([
    getLineTrainsSummary(id, {
      date,
      from: formatApiMinute(window.from),
      to: formatApiMinute(window.to),
      at: at === null ? undefined : formatApiMinute(at),
      direction,
    }),
    previous
      ? getLineTrainsSummary(id, {
          date: yesterday,
          from: formatApiMinute(previous.from),
          to: formatApiMinute(previous.to),
          at: previous.at === null ? undefined : formatApiMinute(previous.at),
          direction,
        })
      : Promise.resolve(null),
  ]);
  if (current.status === 'rejected') {
    if (current.reason instanceof ApiNotFoundError) {
      return <Empty>No scheduled train data is available for this line today.</Empty>;
    }
    return <Empty>Today&apos;s trains aren&apos;t available right now.</Empty>;
  }
  const prev = earlier.status === 'fulfilled' ? earlier.value : null;
  const prevTrains = new WeakSet<LineTrainSummary>();
  const fromPrev = (trains: LineTrainSummary[] | null | undefined) =>
    (trains ?? []).map((train) => {
      const shifted = shiftToNextDay(train);
      prevTrains.add(shifted);
      return shifted;
    });
  const summary: LineTrainsSummary = prev
    ? {
        ...current.value,
        counts: addCounts(current.value.counts, prev.counts),
        truncated: current.value.truncated || prev.truncated,
        trains: [...fromPrev(prev.trains), ...current.value.trains],
        running:
          current.value.running === null && prev.running === null
            ? null
            : sortByLineTime([...fromPrev(prev.running), ...(current.value.running ?? [])]),
      }
    : current.value;
  const dateFor: DateFor = (train) => (prevTrains.has(train) ? yesterday : date);

  const stations = summary.stations;
  const timetableLink = (
    <TextLink href={lineTimetableLink(id, params, window.from)}>Full day&apos;s timetable →</TextLink>
  );
  const header = (
    <Group gap="xs" justify="space-between" wrap="wrap">
      <Text size="xs" c="dimmed">
        {formatDate(now)} · times are when each train is due on the line · {TIMES_IN_UK_LOCAL_TIME}
      </Text>
      <LastUpdated timestamp={now.toISOString()} />
    </Group>
  );

  if (pair) {
    const result = await loadPairRows(date, pair, window);
    return (
      <Stack gap="sm">
        {header}
        <StationPicker id={id} params={params} stations={stations} />
        <StationPairResults
          id={id}
          date={date}
          params={params}
          pair={pair}
          window={window}
          summary={summary}
          result={result}
        />
        {timetableLink}
      </Stack>
    );
  }

  const own = sortByLineTime(summary.trains.filter((t) => t.scope !== 'shared'));
  const shared = summary.trains.filter((t) => t.scope === 'shared');
  const { running, upcoming } = splitRunning(own, liveView ? summary.running : null);
  const heading = params.dir ? directionLabel(params.dir, stations) : null;

  return (
    <Stack gap="sm">
      {header}
      <DirectionChips id={id} params={params} summary={summary} />
      <details className={classes.details}>
        <summary>
          <Text span size="sm">
            Pick stations
          </Text>
        </summary>
        <div className={classes.detailsBody}>
          <StationPicker id={id} params={params} stations={stations} />
        </div>
      </details>

      {running.length > 0 && (
        <Stack gap="xs">
          <SectionTitle order={3}>
            Running now{heading ? ` · ${heading}` : ''} ({running.length})
          </SectionTitle>
          <TrainList trains={running} dateFor={dateFor} stations={stations} label="Running now" />
        </Stack>
      )}

      <Stack gap="xs">
        <Group justify="space-between" gap="xs" wrap="wrap">
          <SectionTitle order={3}>
            {liveView ? 'Due on the line' : `Due on the line from ${formatClock(window.at)}`}
            {heading ? ` · ${heading}` : ''}
          </SectionTitle>
          <ViewChips id={id} params={params} />
        </Group>
        <WindowNav
          id={id}
          params={params}
          window={window}
          count={upcoming.length}
          phoneCount={upcoming.filter((t) => (lineTimeMinute(t.lineDue) ?? 0) < window.phoneTo).length}
        />
        {upcoming.length === 0 ? (
          <Empty>No more of this line&apos;s trains are due in this window.</Empty>
        ) : params.view === 'routes' ? (
          <PatternGroups trains={upcoming} dateFor={dateFor} stations={stations} />
        ) : (
          <TrainList
            trains={upcoming}
            dateFor={dateFor}
            stations={stations}
            window={window}
            label="Trains due on the line"
          />
        )}
        {timetableLink}
        {summary.truncated && (
          <Text size="sm" c="dimmed">
            Only the first {summary.trains.length} trains are shown; use Later for the rest.
          </Text>
        )}
        {!summary.scopeApplied && (
          <Text size="sm" c="dimmed">
            Today&apos;s timetable doesn&apos;t say which trains are this line&apos;s own yet, so every train calling
            here is listed.
          </Text>
        )}
      </Stack>

      <SharedGroup trains={shared} dateFor={dateFor} operatorName={operatorName} />
      <HubLinks stations={stations} />
    </Stack>
  );
}
