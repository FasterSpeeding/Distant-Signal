import { Group, Paper, Stack, Text } from '@mantine/core';
import { ApiNotFoundError, getLineTrains } from '@/lib/api';
import type { LineTrainEntry } from '@/lib/types';
import { formatTime } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import { TextLink } from '@/components/TextLink';

/** Every entry's schedule-side first calling point's own booked time --
 * used as the sort key for every row uniformly, whether or not that row
 * has a `liveStatus` yet. Deliberately NOT `entry.liveStatus?.scheduledDeparture`
 * (an RFC3339 instant, only present once a live `trains` row exists): mixing
 * that format with the schedule's own bare "HH:MM:SS" CIF time within one
 * sort would compare two different string shapes against each other. The
 * schedule (`callingPoints`) is this route's backbone -- present on every
 * entry regardless of live-status coverage -- so it is the only sort key
 * that treats every row the same way. */
function firstScheduledTime(entry: LineTrainEntry): string {
  const first = entry.callingPoints?.[0];
  return first?.booked_departure ?? first?.booked_arrival ?? '';
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

  const sorted = [...trains].sort((a, b) => firstScheduledTime(a).localeCompare(firstScheduledTime(b)));

  return (
    <Stack gap="xs">
      {sorted.map((train) => {
        const live = train.liveStatus;
        const scheduledTime = live?.scheduledDeparture
          ? formatTime(live.scheduledDeparture)
          : firstScheduledTime(train).slice(0, 5) || '?';
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
              {live?.delayMinutes != null && live.delayMinutes > 0 && (
                <Text span c="dimmed">
                  {' '}
                  · {live.delayMinutes}m late
                </Text>
              )}
            </Text>
            <TextLink href={`/train/${encodeURIComponent(train.uid)}/${date}`}>View live status</TextLink>
          </Group>
        );
      })}
    </Stack>
  );
}
