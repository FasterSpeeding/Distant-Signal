import { Box, Group, Text, VisuallyHidden } from '@mantine/core';
import { CardTitleButton, ClickableCard, StaticCard } from './ClickableCard';
import { StatusRow } from './StatusRow';
import { PlatformBadge } from './PlatformBadge';
import { stationLabel } from '@/lib/stationLabel';
import { CANCELLED_LABEL } from '@/lib/serviceStatus';
import { DelayBadge } from './DelayBadge';
import { StateBadge } from './StateBadge';

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

/** The row's status badge, in the app's shared light-variant, sentence-case
 * delay pattern (`JourneyTimeline.tsx`'s `delayBadge`, `TrainJourney.tsx`'s
 * "Delay:" badge), worded by `lib/serviceStatus.ts`: "12 min late" orange,
 * "3 min early" teal, "On time" green, "Cancelled" red. `tt="none"` for the same reason `TrainJourney.tsx`
 * gives -- Mantine's default 11px uppercase measured borderline, and these
 * are phrases with units. Every light-variant text colour here is pinned
 * in `app/globals.css` to >= 5.5:1 on its own tint (orange #a5370b 5.61:1,
 * teal #076b4c 5.65:1, green #226c30 5.62:1, red #ad2424 5.68:1); the
 * dark scheme's light variants invert to a pale shade-0 text on a deep
 * tint. Each state is a different word, never colour alone. */
function statusBadge(row: ScheduleRowData) {
  if (row.isCancelled) {
    return <StateBadge tone="critical">{CANCELLED_LABEL}</StateBadge>;
  }
  return <DelayBadge delayMinutes={row.delayMinutes} />;
}

/** A single reusable schedule/departure row -- one Darwin/LDBWS live
 * departure, with its cancellation/delay status AND its Darwin platform
 * (current, and whether it's changed from what was first announced --
 * `PlatformBadge`), for use in any list view of live departures. Built on
 * `StatusRow` for the same shrink-safe title/trailing layout every other
 * row in this codebase uses (WCAG 2.5.3).
 *
 * Interactive only when BOTH `onSelect` is given AND the row isn't
 * cancelled -- a cancelled service was never selectable there either
 * (nothing to track), and this component keeps that same rule rather than
 * becoming a second, divergently-behaved copy of it. An interactive row is
 * a `ClickableCard` whose title is a real `<button>` stretched over the
 * card (named by the time, destination and operator); a cancelled row in
 * the same picker is the same box, flat. Without `onSelect` it is a plain
 * row. */
export function ScheduleRow({ row, onSelect }: { row: ScheduleRowData; onSelect?: () => void }) {
  const clickable = onSelect !== undefined && !row.isCancelled;
  // A cancelled row is NOT dimmed: the `opacity: 0.6` it used to carry
  // pulled body text down to ~4.3:1 on the dark body (under 4 on a dark
  // Card) -- below AA on exactly the row a traveller most needs to read.
  // Instead, the same cancelled treatment `PlatformBadge` uses: the time is
  // struck through, the red "Cancelled" badge says it in words, and a
  // visually hidden "(cancelled)" puts it in the row text itself, since a
  // screen reader announces neither the strike-through nor the colour.
  // Text stays full body colour in both schemes (21:1 light, 8.2:1+ dark).
  // A `span` (drawn as a block): it goes inside the row's `<button>`,
  // which may only hold phrasing content.
  const title = (
    <Text component="span" display="block" size="sm" data-cancelled={row.isCancelled || undefined}>
      {row.isCancelled ? (
        <>
          <s>{row.scheduled}</s>
          <VisuallyHidden> (cancelled)</VisuallyHidden>
        </>
      ) : (
        row.scheduled
      )}{' '}
      · {stationLabel(row.destinationCrs, row.destinationName)}
      {row.operator ? ` · ${row.operator}` : ''}
    </Text>
  );
  const trailing = (
    <Group gap="xs" wrap="nowrap">
      <PlatformBadge
        platform={row.platform}
        plannedPlatform={row.plannedPlatform}
        platformChanged={row.platformChanged}
      />
      {statusBadge(row)}
    </Group>
  );

  if (clickable) {
    return (
      <ClickableCard padding="xs">
        <StatusRow title={<CardTitleButton onClick={onSelect}>{title}</CardTitleButton>} trailing={trailing} />
      </ClickableCard>
    );
  }
  const statusRow = <StatusRow title={title} trailing={trailing} />;
  if (onSelect !== undefined) {
    // A cancelled row in a picker: the same box as its neighbours, flat.
    return <StaticCard padding="xs">{statusRow}</StaticCard>;
  }
  // `Box`, not `Group`: this wraps `StatusRow` (itself a `Group
  // justify="space-between"`) as its ONLY child, and a flex child doesn't
  // stretch to its container's full width by default.
  return <Box>{statusRow}</Box>;
}
