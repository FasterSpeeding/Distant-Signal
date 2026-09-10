import { Badge, Table, Text } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { JourneyStop } from '@/lib/types';

/** Renders the full scheduled timetable as the primary structure of the
 * train detail page, with live actual-vs-scheduled data overlaid per stop
 * once available -- see
 * docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §4.
 * Rendered whenever `TrainJourneyState.journeyStops` is non-null,
 * independent of `resolutionStatus`/`status` -- this is the restructuring
 * the design doc's §4 describes: the timeline is no longer nested inside
 * one branch of the status switch.
 *
 * A real `Table`, not a stack of flex rows -- a calling-point list is
 * genuinely tabular data (one station per row, the same four facts about
 * each), and a stack of independently-sized `Group`s can't keep a column
 * aligned down the list once row content varies in length, which is
 * exactly what adding a variable-width estimated time made worse rather
 * than better. `Table.ScrollContainer` keeps a long station name from
 * forcing the whole page to scroll horizontally on a narrow screen --
 * the table scrolls in its own box instead. */
export function JourneyTimeline({ stops }: { stops: JourneyStop[] }) {
  return (
    <Table.ScrollContainer minWidth={420}>
      <Table verticalSpacing={6} horizontalSpacing="sm" aria-label="Journey timeline">
        <Table.Thead>
          <Table.Tr>
            <Table.Th>Station</Table.Th>
            <Table.Th>Scheduled</Table.Th>
            <Table.Th>Actual / est.</Table.Th>
            <Table.Th>Delay</Table.Th>
          </Table.Tr>
        </Table.Thead>
        <Table.Tbody>
          {stops.map((stop, index) => (
            <JourneyStopRow key={`${stop.crs ?? 'unknown'}-${index}`} stop={stop} />
          ))}
        </Table.Tbody>
      </Table>
    </Table.ScrollContainer>
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
  const estimated = stop.estimatedDeparture ?? stop.estimatedArrival;
  const reached = actual !== null;

  return (
    <Table.Tr>
      <Table.Td>
        <Text
          fw={stop.kind === 'Origin' || stop.kind === 'Terminate' ? 700 : 400}
          c={reached ? undefined : 'dimmed'}
        >
          {label}
        </Text>
      </Table.Td>
      <Table.Td>
        {scheduled && (
          <Text size="sm" c="dimmed">
            {formatTime(scheduled)}
          </Text>
        )}
      </Table.Td>
      <Table.Td>
        {actual ? (
          <Text size="sm">{formatTime(actual)}</Text>
        ) : (
          estimated && (
            // Visually distinguished from a confirmed actual time -- muted
            // and italic, with an explicit "est." prefix, never rendered
            // for a stop that already has a real reported time above.
            <Text size="sm" c="dimmed" fs="italic">
              est. {formatTime(estimated)}
            </Text>
          )
        )}
      </Table.Td>
      <Table.Td>{delayBadge(stop.delayMinutes)}</Table.Td>
    </Table.Tr>
  );
}
