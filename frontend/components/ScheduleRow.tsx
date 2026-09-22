import { Badge, Box, Group, Text } from '@mantine/core';
import { StatusRow } from './StatusRow';
import { PlatformBadge } from './PlatformBadge';
import { stationLabel } from '@/lib/stationLabel';

/** One live LDBWS departure-board row's display data -- deliberately a
 * plain data shape, not `DepartureRow` (`TrackTrainForm.tsx`'s own wire
 * type for `GET /public/stations/{crs}/departures`) itself, so this
 * component has no dependency on that route's exact field names and can
 * be reused by any future list view backed by a differently-shaped live
 * source (a station departure-board page, a search-results list) without
 * a wire-type coupling. */
export interface ScheduleRowData {
  /** A stable React key for the caller's own list rendering -- not
   * rendered by this component itself. Darwin's `serviceId` is the usual
   * choice (`DepartureRow.serviceId`). */
  key: string;
  scheduled: string; // "HH:MM"
  destinationCrs: string;
  /** Resolved station name for `destinationCrs`, via a server-side batched
   * `stations` lookup (2026-09-22 UX review follow-up -- this picker used
   * to show the bare CRS code with no name at all). `null` when the code
   * has no reference row; `stationLabel` (below) falls back to the code
   * itself in that case, the same convention this app uses everywhere a
   * name might not resolve. */
  destinationName: string | null;
  /** `undefined` (not rendered) rather than `''`, for a source with no
   * operator field at all (e.g. a CIF-derived row) -- same "omit, don't
   * fabricate" posture as `PlatformBadge`'s own `null` handling. */
  operator?: string;
  isCancelled: boolean;
  delayMinutes: number;
  platform: string | null;
  plannedPlatform: string | null;
  platformChanged: boolean;
}

function statusBadge(row: ScheduleRowData) {
  if (row.isCancelled) return <Badge color="red">Cancelled</Badge>;
  if (row.delayMinutes > 0) return <Badge color="orange">+{row.delayMinutes} min</Badge>;
  return <Badge color="green">On time</Badge>;
}

/** A single reusable schedule/departure row -- one Darwin/LDBWS live
 * departure, with its cancellation/delay status AND its Darwin platform
 * (current, and whether it's changed from what was first announced --
 * `PlatformBadge`), for use in any list view of live departures. Built on
 * `StatusRow` for the same shrink-safe title/trailing layout every other
 * row in this codebase uses (WCAG 2.5.3).
 *
 * Interactive (a `role="button"`, keyboard-activatable row, mirroring
 * `TrackTrainForm.tsx`'s picker rows) only when BOTH `onSelect` is given
 * AND the row isn't cancelled -- a cancelled service was never selectable
 * there either (nothing to track), and this component keeps that same
 * rule rather than becoming a second, divergently-behaved copy of it. */
export function ScheduleRow({ row, onSelect }: { row: ScheduleRowData; onSelect?: () => void }) {
  const clickable = onSelect !== undefined && !row.isCancelled;
  const title = (
    <Text size="sm" style={{ opacity: row.isCancelled ? 0.6 : 1 }}>
      {row.scheduled} · {stationLabel(row.destinationCrs, row.destinationName)}
      {row.operator ? ` · ${row.operator}` : ''}
    </Text>
  );
  const trailing = (
    <Group gap="xs" wrap="nowrap">
      <PlatformBadge platform={row.platform} plannedPlatform={row.plannedPlatform} platformChanged={row.platformChanged} />
      {statusBadge(row)}
    </Group>
  );

  return (
    // `Box`, not `Group`: this wraps `StatusRow` (itself a `Group
    // justify="space-between"`) as its ONLY child, and a flex child
    // doesn't stretch to its container's full width by default -- a `Group`
    // wrapper here would let `StatusRow`'s own "space-between" collapse
    // down to content width instead of spanning the row. `Box` is a plain
    // block element, so it's full-width by default, same as this
    // component's own `role="button"` predecessor at
    // `TrackTrainForm.tsx`'s picker rows.
    <Box
      role={clickable ? 'button' : undefined}
      tabIndex={clickable ? 0 : undefined}
      onClick={clickable ? onSelect : undefined}
      onKeyDown={
        clickable
          ? (event) => {
              if (event.key === 'Enter' || event.key === ' ') onSelect?.();
            }
          : undefined
      }
      style={{ cursor: clickable ? 'pointer' : 'default' }}
    >
      <StatusRow title={title} trailing={trailing} />
    </Box>
  );
}
