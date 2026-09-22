import { Group, Paper, Stack, Text } from '@mantine/core';
import { ApiNotFoundError, getLineTrains } from '@/lib/api';
import type { LineTrainEntry } from '@/lib/types';
import { routeLabel, UNKNOWN_STATION_LABEL } from '@/lib/stationLabel';
import { TextLink } from '@/components/TextLink';
import { StatusRow } from '@/components/StatusRow';
import { LastUpdated } from '@/components/LastUpdated';
import { formatDate, TIMES_IN_UK_LOCAL_TIME } from '@/lib/dateFormat';

/** Every entry's schedule-side first calling point's own booked time
 * ("HH:MM:SS", CIF/UK-local) -- both this row's displayed time and the
 * basis of `scheduleSortKey` below, uniformly whether or not this row has a
 * `liveStatus` yet. Deliberately NOT `entry.liveStatus?.scheduledDeparture`
 * (an RFC3339 instant, only present once a live `trains` row exists, and --
 * for a line whose trains originate off-line -- the whole service's ORIGIN
 * departure time, not necessarily the time THIS line's own first calling
 * point is reached): using it here would let the displayed time disagree
 * with this row's own sort position, which always uses the schedule side.
 * The schedule (`callingPoints`) is this route's backbone -- present on
 * every entry regardless of live-status coverage -- so it is the only
 * source that treats every row the same way. */
function firstScheduledTime(entry: LineTrainEntry): string {
  const first = entry.callingPoints?.[0];
  return first?.booked_departure ?? first?.booked_arrival ?? '';
}

/** Sort key for `firstScheduledTime`, prefixed with `day_offset` so a
 * post-midnight first calling point (`day_offset: 1`, e.g. "00:20:00")
 * sorts after same-day departures rather than lexically to the top of the
 * day. */
function scheduleSortKey(entry: LineTrainEntry): string {
  const dayOffset = entry.callingPoints?.[0]?.day_offset ?? 0;
  return `${dayOffset}${firstScheduledTime(entry)}`;
}

/** `scheduleSortKey`'s own format ("{dayOffset}HH:MM:SS") for "right now",
 * so the two are directly `localeCompare`-able -- the partition `nowSortKey`
 * exists for (below). Always prefixed `0`: this page shows one rail day at
 * a time and "now" is always read as falling within that day's own
 * same-day portion, same simplification `scheduleSortKey`'s day-offset
 * comment already accepts for a genuine post-midnight service (a visitor
 * loading this page in the small hours of a day whose first calling point
 * is itself day_offset 1 is a rarer edge case than the one this fixes). */
function nowSortKey(now: Date): string {
  const parts = new Intl.DateTimeFormat('en-GB', {
    timeZone: 'Europe/London',
    hourCycle: 'h23',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
  }).formatToParts(now);
  const get = (type: string) => parts.find((p) => p.type === type)?.value ?? '00';
  return `0${get('hour')}:${get('minute')}:${get('second')}`;
}

/** The route name for a row that HAS a live record (`live` is non-null) --
 * see the exported component's own branch for the "no live record yet"
 * case, which keeps its separate, honest "Scheduled — not live yet" copy
 * unchanged.
 *
 * Previously this called `routeLabel(live.originCrs, live.originName, …)`
 * directly: a live record with no schedule match of its own has
 * `originCrs: null`, so `routeLabel` returned its bare
 * `UNKNOWN_STATION_LABEL` fallback as the row's ENTIRE label -- eleven of
 * twelve rows in one capture (2026-09-22 UX review §4.1). The schedule side
 * (`entry.scheduleOriginCrs`/`scheduleDestinationCrs`, resolved
 * server-side from `callingPoints`' first/last TIPLOC — see
 * `crates/api/src/render.rs`'s `ScheduleRouteEndpoints`) is present on
 * every entry regardless of live-status coverage, so it is the base label;
 * the live origin/destination only *upgrades* it, and only when BOTH ends
 * resolved. If neither side names a station, the row still identifies the
 * train by its UID rather than printing the bare fallback string as
 * user-facing copy. */
function liveRowRouteLabel(entry: LineTrainEntry): string {
  const live = entry.liveStatus;
  if (live?.originCrs && live?.destinationCrs) {
    return routeLabel(live.originCrs, live.originName, live.destinationCrs, live.destinationName);
  }
  const scheduleLabel = routeLabel(
    entry.scheduleOriginCrs,
    entry.scheduleOriginName,
    entry.scheduleDestinationCrs,
    entry.scheduleDestinationName,
  );
  return scheduleLabel === UNKNOWN_STATION_LABEL ? `Train ${entry.uid}` : scheduleLabel;
}

/** One row's worth of rendering -- shared between the "upcoming" list and
 * the collapsed "earlier today" list below, so the two can never drift in
 * shape. Reuses `StatusRow` for its shrink guard (2026-09-22 UX review
 * §4.3: a `Group wrap="nowrap"` row with no `flexShrink: 0` on the trailing
 * link let a long route name squeeze "View live status" down to nothing at
 * narrow widths) and passes a row-specific `aria-label` on the link (same
 * review: 12 identical "View live status" links have no distinguishing
 * accessible name of their own). */
function TrainRow({ train, date }: { train: LineTrainEntry; date: string }) {
  const live = train.liveStatus;
  const scheduledTime = firstScheduledTime(train).slice(0, 5) || '?';
  const isCancelled = live?.status === 'cancelled';
  const routeText = live ? liveRowRouteLabel(train) : 'Scheduled — not live yet';
  return (
    <StatusRow
      title={
        <Text size="sm" style={{ minWidth: 0 }}>
          {scheduledTime}
          {' · '}
          {live ? (
            routeText
          ) : (
            <Text span c="dimmed">
              {routeText}
            </Text>
          )}
          {isCancelled ? (
            <Text span c="red">
              {' '}
              · Cancelled
            </Text>
          ) : (
            live?.delayMinutes != null &&
            live.delayMinutes > 0 && (
              // Coloured, not dimmed: the one live fact a traveller cares
              // about was previously the same grey as "Scheduled — not live
              // yet" -- the least important text on the row. `orange`
              // matches `TrackedTrainStatusBadge`'s own delay colour.
              <Text span c="orange" fw={500}>
                {' '}
                · {live.delayMinutes}m late
              </Text>
            )
          )}
        </Text>
      }
      trailing={
        <TextLink
          href={`/train/${encodeURIComponent(train.uid)}/${date}`}
          ariaLabel={`View live status for the ${scheduledTime} · ${routeText}`}
        >
          View live status
        </TextLink>
      }
    />
  );
}

/** `id`/`date` are both required (not defaulted here) -- `date` in
 * particular is deliberately the caller's own, already-computed
 * `londonDayKey`, not left to `getLineTrains`'s own UTC default, so the
 * date this panel fetches and the date every row's `/train/{uid}/{date}`
 * link points at can never disagree (see `LineDetailPage`'s own comment on
 * this). Rendered inside a `<Suspense>` on `/lines/[id]` (`page.tsx`) --
 * same rationale as `HalfHourlyTrendsResults`: Suspense catches
 * *suspension*, not errors, so every failure branch below must resolve to
 * real markup rather than throw, or a backend outage would blank the whole
 * line page instead of just this panel.
 *
 * `now` is optional (defaults to the real current time) purely so tests can
 * pin the upcoming/departed split below to a deterministic moment; every
 * real caller lets it default -- `LineDetailPage` already computes its own
 * `now` once for `trendsRange`/`IssueList`, but this panel's date-only
 * `date` prop is already what keeps this fetch and its links in sync with
 * that page, and threading a second timestamp through for a purely
 * cosmetic ordering concern isn't worth the extra prop on every other
 * caller. */
export async function LineTrainsResults({ id, date, now = new Date() }: { id: string; date: string; now?: Date }) {
  let trains: LineTrainEntry[];
  try {
    trains = await getLineTrains(id, date);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      // The expected, common case for a custom or TfL line if this
      // component is ever reached for one despite page.tsx's own gate, and
      // the equally expected case for a catalogue line with no CIF
      // schedule population for this exact rail day yet (see
      // get_line_schedule's own 404 semantics) -- not a claim that
      // something is broken.
      return (
        <Paper withBorder p="md">
          <Text c="dimmed">No scheduled train data is available for this line today.</Text>
        </Paper>
      );
    }
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">Today&apos;s trains aren&apos;t available right now.</Text>
      </Paper>
    );
  }

  if (trains.length === 0) {
    return (
      <Paper withBorder p="md">
        <Text c="dimmed">No trains are scheduled on this line today.</Text>
      </Paper>
    );
  }

  const sorted = [...trains].sort((a, b) => scheduleSortKey(a).localeCompare(scheduleSortKey(b)));

  // Upcoming-first, departed collapsed (2026-09-22 UX review §4.2): a
  // traveller checking a Severe-delays line in the evening wants the next
  // departure, not the whole day scrolled from 06:00 -- and the list has no
  // cap, so on mobile the "Recent trends" charts below it became
  // effectively unreachable. `nowKey` is comparable against
  // `scheduleSortKey` directly (same "{dayOffset}HH:MM:SS" shape).
  const nowKey = nowSortKey(now);
  const upcoming = sorted.filter((train) => scheduleSortKey(train) >= nowKey);
  const departed = sorted.filter((train) => scheduleSortKey(train) < nowKey);

  return (
    <Stack gap="xs">
      {/* One dimmed caption covers date, count and timezone at once
          (2026-09-22 UX review §4.4 -- the panel previously had none of
          the three); `LastUpdated` alongside it covers the fourth
          (freshness) with the same component `LineStatusCard` already
          uses elsewhere. This panel has no per-request fetch timestamp of
          its own to hand it, but the page it's embedded in has no stale
          cache either (`getLineTrains` is fetched fresh every request,
          unlike `withStaleFallback`'s callers) -- so `now`, the moment
          this render started, is an honest freshness figure, not a
          borrowed one. */}
      <Group gap="xs" justify="space-between" wrap="wrap">
        <Text size="xs" c="dimmed">
          {formatDate(now)} · {sorted.length} train{sorted.length === 1 ? '' : 's'} scheduled today ·{' '}
          {TIMES_IN_UK_LOCAL_TIME}
        </Text>
        <LastUpdated timestamp={now.toISOString()} />
      </Group>

      {upcoming.length === 0 ? (
        <Text size="sm" c="dimmed">
          No more trains are scheduled on this line for the rest of today.
        </Text>
      ) : (
        upcoming.map((train) => <TrainRow key={train.uid} train={train} date={date} />)
      )}

      {departed.length > 0 && (
        <details>
          <summary>
            <Text span size="sm" c="dimmed">
              {departed.length} earlier train{departed.length === 1 ? '' : 's'} today
            </Text>
          </summary>
          <Stack gap="xs" pt="xs">
            {departed.map((train) => (
              <TrainRow key={train.uid} train={train} date={date} />
            ))}
          </Stack>
        </details>
      )}
    </Stack>
  );
}
