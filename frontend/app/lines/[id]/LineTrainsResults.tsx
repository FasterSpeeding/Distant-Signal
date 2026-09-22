import { Group, Paper, Stack, Text } from '@mantine/core';
import { ApiNotFoundError, getLineTrains } from '@/lib/api';
import type { LineTrainEntry } from '@/lib/types';
import { routeLabel } from '@/lib/stationLabel';
import { TextLink } from '@/components/TextLink';

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

/** `id`/`date` are both required (not defaulted here) -- `date` in
 * particular is deliberately the caller's own, already-computed
 * `londonDayKey`, not left to `getLineTrains`'s own UTC default, so the
 * date this panel fetches and the date every row's `/train/{uid}/{date}`
 * link points at can never disagree (see `LineDetailPage`'s own comment on
 * this). Rendered inside a `<Suspense>` on `/lines/[id]` (`page.tsx`) --
 * same rationale as `HalfHourlyTrendsResults`: Suspense catches
 * *suspension*, not errors, so every failure branch below must resolve to
 * real markup rather than throw, or a backend outage would blank the whole
 * line page instead of just this panel. */
export async function LineTrainsResults({ id, date }: { id: string; date: string }) {
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

  return (
    <Stack gap="xs">
      {sorted.map((train) => {
        const live = train.liveStatus;
        const scheduledTime = firstScheduledTime(train).slice(0, 5) || '?';
        const isCancelled = live?.status === 'cancelled';
        return (
          <Group key={train.uid} justify="space-between" wrap="nowrap">
            <Text size="sm">
              {scheduledTime}
              {' · '}
              {live ? (
                routeLabel(live.originCrs, live.originName, live.destinationCrs, live.destinationName)
              ) : (
                <Text span c="dimmed">
                  Scheduled — not live yet
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
                  <Text span c="dimmed">
                    {' '}
                    · {live.delayMinutes}m late
                  </Text>
                )
              )}
            </Text>
            <TextLink href={`/train/${encodeURIComponent(train.uid)}/${date}`}>View live status</TextLink>
          </Group>
        );
      })}
    </Stack>
  );
}
