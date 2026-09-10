import {
  Badge,
  Table,
  TableScrollContainer,
  TableThead,
  TableTbody,
  TableTr,
  TableTh,
  TableTd,
  Text,
} from '@mantine/core';
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
 * than better. `TableScrollContainer` keeps a long station name from
 * forcing the whole page to scroll horizontally on a narrow screen --
 * the table scrolls in its own box instead.
 *
 * Flat `TableThead`/`TableTr`/... named exports, not the `Table.Thead`
 * dot-notation compound API -- this component is rendered from a Server
 * Component chain (`page.tsx` -> `TrainJourney.tsx` -> here, none of them
 * carrying `"use client"`), and the compound API pulls in a
 * `"use client"`-tainted import chain that resolves to `undefined` at
 * runtime in that context -- the same reason `AllLinesTable.tsx` and
 * `lines/[id]/history/page.tsx` use the flat exports instead. */
export function JourneyTimeline({ stops }: { stops: JourneyStop[] }) {
  return (
    <TableScrollContainer minWidth={420}>
      <Table verticalSpacing={6} horizontalSpacing="sm" aria-label="Journey timeline">
        <TableThead>
          <TableTr>
            <TableTh>Station</TableTh>
            <TableTh>Scheduled</TableTh>
            <TableTh>Actual / est.</TableTh>
            <TableTh>Delay</TableTh>
          </TableTr>
        </TableThead>
        <TableTbody>
          {stops.map((stop, index) => (
            <JourneyStopRow key={`${stop.crs ?? 'unknown'}-${index}`} stop={stop} />
          ))}
        </TableTbody>
      </Table>
    </TableScrollContainer>
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
    <TableTr>
      <TableTd>
        <Text
          fw={stop.kind === 'Origin' || stop.kind === 'Terminate' ? 700 : 400}
          c={reached ? undefined : 'dimmed'}
        >
          {label}
        </Text>
      </TableTd>
      <TableTd>
        {scheduled && (
          <Text size="sm" c="dimmed">
            {formatTime(scheduled)}
          </Text>
        )}
      </TableTd>
      <TableTd>
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
      </TableTd>
      <TableTd>{delayBadge(stop.delayMinutes)}</TableTd>
    </TableTr>
  );
}
