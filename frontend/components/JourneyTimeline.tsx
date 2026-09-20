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

/** The tracked pin's origin/destination display names (Task 3.6.2) --
 * always derivable from `TrainJourneyState.pinOriginCrs`/`pinOriginName`/
 * `pinDestinationCrs`/`pinDestinationName` even when the server-side
 * TIPLOC->CRS->name join (`crates/api/src/data/journey.rs`) couldn't
 * resolve a particular calling point's own name/CRS -- the pin is what the
 * user tracked, not something read off the timetable. `null` on either
 * side is still a legitimate value (an NR-primary subscription with no
 * pinned destination, or one created before any schedule match resolved a
 * name for the pinned CRS); `journeyStopLabel` treats that exactly like
 * "no seed available" and falls through to its own generic fallback. */
export interface JourneyEndpointNames {
  originName: string | null;
  destinationName: string | null;
}

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
export function JourneyTimeline({
  stops,
  endpointNames,
}: {
  stops: JourneyStop[];
  endpointNames?: JourneyEndpointNames;
}) {
  const total = stops.length;
  // Genuinely degenerate case (Task 3.6.2 point 4): every stop -- including
  // the two endpoints, even after seeding them from the pin -- has nothing
  // to display. Rendering N rows of "Stop 1"/"Stop 2"/... would look like a
  // real, if terse, timetable; it isn't one, so this collapses to a single
  // honest line instead. `total > 0` guards a genuinely-empty list, which
  // `TrainJourney.tsx` never renders this component for anyway
  // (`{state.journeyStops && ...}`) but `every` on `[]` is vacuously `true`.
  const allUnnamed =
    total > 0 && stops.every((stop, index) => resolvedStopLabel(stop, index, total, endpointNames) === null);

  if (allUnnamed) {
    return (
      <Text size="sm" c="dimmed">
        {total} {total === 1 ? 'stop' : 'stops'} — station names unavailable
      </Text>
    );
  }

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
            <JourneyStopRow
              key={`${stop.crs ?? 'unknown'}-${index}`}
              stop={stop}
              index={index}
              total={total}
              endpointNames={endpointNames}
            />
          ))}
        </TableTbody>
      </Table>
    </TableScrollContainer>
  );
}

/** The calling-point display name for a `JourneyStop` if one can be
 * resolved without falling back to a generic by-index placeholder --
 * `null` means "nothing to show here", which is exactly the signal
 * `JourneyTimeline`'s all-unnamed collapse (above) needs and a
 * `journeyStopLabel` that always returns a string can't give it. */
function resolvedStopLabel(
  stop: JourneyStop,
  index: number,
  total: number,
  endpointNames: JourneyEndpointNames | undefined,
): string | null {
  if (stop.name) return stop.name;
  if (stop.crs) return stop.crs;
  if (index === 0 && endpointNames?.originName) return endpointNames.originName;
  if (index === total - 1 && endpointNames?.destinationName) return endpointNames.destinationName;
  return null;
}

/** The calling-point display name for a `JourneyStop`: its resolved name,
 * falling back to the bare CRS code, falling back (at the first/last row
 * only) to the tracked pin's own origin/destination name -- always known
 * even when the timetable data isn't (Task 3.6.2) -- and finally to a
 * by-INDEX placeholder ("Stop 3"), never the old "Unknown location"
 * string: a server-side TIPLOC/CRS join failure is real and rare now (see
 * `crates/api/src/data/journey.rs`'s `tiploc_key` doc comment), not
 * evidence the row itself is bogus, so it still deserves a row, just not a
 * one-size-fits-all label that reads as an error. Shared with
 * `JourneyProgress.tsx`, which reuses this exact fallback chain for its own
 * node labels/tooltips/captions -- see that file's own doc comments for why
 * it must match this one. `endpointNames`/`index`/`total` are optional-ish
 * (index/total have no sensible default, but `endpointNames` may be
 * omitted) purely so a caller with no pin data on hand still compiles. */
export function journeyStopLabel(
  stop: JourneyStop,
  index: number,
  total: number,
  endpointNames?: JourneyEndpointNames,
): string {
  return resolvedStopLabel(stop, index, total, endpointNames) ?? `Stop ${index + 1}`;
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

function JourneyStopRow({
  stop,
  index,
  total,
  endpointNames,
}: {
  stop: JourneyStop;
  index: number;
  total: number;
  endpointNames?: JourneyEndpointNames;
}) {
  const label = journeyStopLabel(stop, index, total, endpointNames);
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
