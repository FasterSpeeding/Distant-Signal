import { Badge, Group, Stack, Text } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { JourneyStop } from '@/lib/types';

/** Renders the full scheduled timetable as the primary structure of the
 * train detail page, with live actual-vs-scheduled data overlaid per stop
 * once available -- see
 * docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §4.
 * Rendered whenever `TrainJourneyState.journeyStops` is non-null,
 * independent of `resolutionStatus`/`status` -- this is the restructuring
 * the design doc's §4 describes: the timeline is no longer nested inside
 * one branch of the status switch. */
export function JourneyTimeline({ stops }: { stops: JourneyStop[] }) {
  return (
    <Stack gap={4} role="list" aria-label="Journey timeline">
      {stops.map((stop, index) => (
        <JourneyStopRow key={`${stop.crs ?? 'unknown'}-${index}`} stop={stop} />
      ))}
    </Stack>
  );
}

function delayBadge(delayMinutes: number | null) {
  if (delayMinutes === null) return null;
  if (delayMinutes === 0) {
    return (
      <Badge color="green" variant="light">
        On time
      </Badge>
    );
  }
  if (delayMinutes > 0) {
    return (
      <Badge color="orange" variant="light">
        {delayMinutes}m late
      </Badge>
    );
  }
  return (
    <Badge color="teal" variant="light">
      {Math.abs(delayMinutes)}m early
    </Badge>
  );
}

function JourneyStopRow({ stop }: { stop: JourneyStop }) {
  const label = stop.name ?? stop.crs ?? 'Unknown location';
  const scheduled = stop.scheduledDeparture ?? stop.scheduledArrival;
  const actual = stop.actualDeparture ?? stop.actualArrival;
  const reached = actual !== null;

  return (
    <Group gap="xs" role="listitem" wrap="nowrap">
      <Text fw={stop.kind === 'Origin' || stop.kind === 'Terminate' ? 700 : 400} c={reached ? undefined : 'dimmed'}>
        {label}
      </Text>
      {scheduled && (
        <Text size="sm" c="dimmed">
          {formatTime(scheduled)}
        </Text>
      )}
      {actual && (
        <Text size="sm">{formatTime(actual)}</Text>
      )}
      {delayBadge(stop.delayMinutes)}
    </Group>
  );
}
