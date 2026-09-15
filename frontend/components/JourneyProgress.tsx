'use client';

import { Box, Stack, Text, Tooltip } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { JourneyStatus, JourneyStop, ResolutionStatus } from '@/lib/types';

const NODE_SLOT_WIDTH = 56;

/** The largest node diameter (`nodeDiameter`'s Origin/Terminate case).
 * Every node's circle -- whatever kind, whatever renders below it (an
 * always-visible label for an endpoint, nothing for a bare intermediate
 * node) -- is centered inside a slot of this fixed height, anchored to the
 * top of its flex item (`.journeyProgressLine`'s `align-items: flex-start`
 * in `globals.css`). That keeps every circle's vertical center at the same
 * offset (`NODE_CIRCLE_SLOT / 2`, matching the connecting line's own
 * `top: 9px`) regardless of the varying total height Task 3's endpoint
 * labels (and Task 6's `mayHaveArrived` glyph) introduce below the circle. */
const NODE_CIRCLE_SLOT = 18;

/** See this component's extra props beyond `{ stops }`: `resolutionStatus`,
 * `status`, `trainUid`, and `mayHaveArrived` are needed to render the full
 * status-matrix captions in `progressCopy` (added in a later task) --
 * `schedule_matched` and `resolved`+`awaiting_activation` produce
 * IDENTICAL `stops`/`lastReachedIndex` shapes but need different caption
 * text, and only `resolutionStatus` distinguishes them. See
 * docs/superpowers/plans/2026-09-12-journey-progress-visualization.md's
 * Global Constraints for the full reasoning. */
interface JourneyProgressProps {
  stops: JourneyStop[];
  resolutionStatus: ResolutionStatus;
  status: JourneyStatus | null;
  trainUid: string | null;
  mayHaveArrived: boolean;
}

/** The last scheduled calling point with a confirmed reported event -- an
 * ARRIVAL, DEPARTURE, or PASS message TRUST has already sent, already
 * merged into `JourneyStop.actualArrival`/`actualDeparture`. This is the
 * ONLY thing "you are here" is allowed to mean in this component.
 *
 * This deliberately walks `actualArrival`/`actualDeparture` ONLY -- never
 * `estimatedArrival`/`estimatedDeparture`, which are a forward propagation
 * of the train's current overall delay onto stops nothing has confirmed
 * yet, not a report of anything Network Rail said happened there.
 *
 * `docs/superpowers/specs/2026-08-28-train-tracking-design.md` already
 * investigated and explicitly rejected Train Describer (TD) / berth-level
 * physical position tracking as a non-goal for this app, reasoning that
 * TRUST's schedule-location events are the right granularity for "where is
 * this train relative to its stops." This function -- and the marker it
 * drives -- is a restatement of that decision, not a new instance of it: it
 * NEVER returns a value that implies the train is between two stops, NEVER
 * advances on a timer, and NEVER changes except when a new confirmed event
 * actually arrives on the next data refresh. A future edit must not
 * "smooth" or animate the marker between two confirmed indices -- there is
 * no data behind such a position, by design, and adding one would silently
 * reopen the TD/GPS non-goal in a different visual form. `-1` (no stop
 * confirmed yet) is a legitimate, common return value, not an error case. */
function lastReachedIndex(stops: JourneyStop[]): number {
  for (let i = stops.length - 1; i >= 0; i--) {
    if (stops[i].actualArrival !== null || stops[i].actualDeparture !== null) {
      return i;
    }
  }
  return -1;
}

function nodeDiameter(kind: JourneyStop['kind']): number {
  return kind === 'Origin' || kind === 'Terminate' ? 18 : 12;
}

type NodeState = 'reached' | 'marker' | 'not-reached';

/** `index <= lastIndex` (and `lastIndex !== -1`) is "reached"; the stop
 * exactly at `lastIndex` is additionally the "you are here" marker; every
 * later index is "not yet reached". `lastIndex === -1` means nothing has
 * been confirmed, so every node is "not yet reached" and no marker exists
 * -- this falls out of the comparison rather than needing a special case. */
function nodeState(index: number, lastIndex: number): NodeState {
  if (lastIndex === -1 || index > lastIndex) return 'not-reached';
  if (index === lastIndex) return 'marker';
  return 'reached';
}

type DelayState = 'on-time' | 'late' | 'early' | 'unknown';

/** Reuses `JourneyTimeline.tsx`'s exact three-way delay convention
 * (green/orange/teal) -- see its own `delayBadge` -- plus one honest
 * addition this component needs that the badge never has to express: a
 * reached stop whose `delayMinutes` is `null` (no `scheduledArrival`/
 * `scheduledDeparture` to diff against). Defaulting that to "on time"
 * would fabricate a fact nothing confirmed; `'unknown'` renders a neutral
 * gray instead. */
function delayState(delayMinutes: number | null): DelayState {
  if (delayMinutes === null) return 'unknown';
  if (delayMinutes === 0) return 'on-time';
  return delayMinutes > 0 ? 'late' : 'early';
}

const DELAY_COLOR: Record<DelayState, string> = {
  'on-time': 'green',
  late: 'orange',
  early: 'teal',
  unknown: 'gray',
};

/** The "you are here" marker gets the same fill as any other reached node
 * PLUS a `boxShadow` halo -- a wider, higher-contrast ring, not a
 * different color, so it stays legible against any of the three delay
 * colors (spec Decision 3). */
function circleStyle(state: NodeState, delay: DelayState): React.CSSProperties {
  if (state === 'not-reached') {
    return { border: '2px solid var(--mantine-color-gray-5)', backgroundColor: 'transparent' };
  }
  const color = DELAY_COLOR[delay];
  const base: React.CSSProperties = {
    border: `2px solid var(--mantine-color-${color}-6)`,
    backgroundColor: `var(--mantine-color-${color}-6)`,
  };
  if (state === 'marker') {
    base.boxShadow = `0 0 0 3px var(--mantine-color-${color}-3)`;
  }
  return base;
}

/** Schematic, index-spaced (NOT time/distance-proportional) "you are here"
 * line diagram, additive to `JourneyTimeline` -- see
 * docs/superpowers/specs/2026-09-12-journey-progress-visualization-design.md.
 * Rendered directly above `JourneyTimeline` in `TrainJourney.tsx`, behind
 * the identical `{state.journeyStops && ...}` guard. */
export function JourneyProgress({ stops }: JourneyProgressProps) {
  const lastIndex = lastReachedIndex(stops);
  const ariaLabel = `Journey progress: ${stops.length} stop${stops.length === 1 ? '' : 's'}`;

  return (
    <Box role="img" aria-label={ariaLabel} style={{ overflowX: 'auto' }}>
      <Box className="journeyProgressLine" style={{ minWidth: stops.length * NODE_SLOT_WIDTH }}>
        {stops.map((stop, index) => (
          <JourneyProgressNode
            key={`${stop.crs ?? 'unknown'}-${index}`}
            stop={stop}
            index={index}
            lastIndex={lastIndex}
          />
        ))}
      </Box>
    </Box>
  );
}

/** Origin/Terminate always print their name as visible text next to the
 * node (spec Decision 3: "the origin and terminus names are always
 * printed"). Every other node is a bare circle; a `Tooltip` (matching
 * `LineDefinitionTooltip.tsx`'s existing hover/focus/touch pattern)
 * reveals its name and scheduled time on demand instead of permanently
 * occupying screen space -- with 20-30+ evenly-spaced nodes, a label under
 * each one collides or truncates into uselessness. The decorative circle
 * itself is always `aria-hidden`; for a bare node, the Tooltip's
 * *trigger wrapper* carries its own `aria-label` and stays keyboard
 * focusable instead. */
function JourneyProgressNode({
  stop,
  index,
  lastIndex,
}: {
  stop: JourneyStop;
  index: number;
  lastIndex: number;
}) {
  const diameter = nodeDiameter(stop.kind);
  const state = nodeState(index, lastIndex);
  const delay = delayState(stop.delayMinutes);
  const isEndpoint = stop.kind === 'Origin' || stop.kind === 'Terminate';
  const label = stop.name ?? stop.crs ?? 'Unknown location';
  const scheduled = stop.scheduledArrival ?? stop.scheduledDeparture;

  const circle = (
    <Box
      data-journey-node
      data-node-state={state}
      data-delay-state={delay}
      aria-hidden="true"
      style={{
        width: diameter,
        height: diameter,
        borderRadius: '50%',
        zIndex: 1,
        ...circleStyle(state, delay),
      }}
    />
  );

  const circleSlot = (
    <Box style={{ height: NODE_CIRCLE_SLOT, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
      {circle}
    </Box>
  );

  if (isEndpoint) {
    return (
      <Stack gap={4} align="center" style={{ flex: `0 0 ${NODE_SLOT_WIDTH}px` }}>
        {circleSlot}
        <Text size="xs" fw={700} ta="center">
          {label}
        </Text>
      </Stack>
    );
  }

  return (
    <Box
      style={{
        flex: `0 0 ${NODE_SLOT_WIDTH}px`,
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
      }}
    >
      <Tooltip
        label={
          <Stack gap={2}>
            <Text size="xs">{label}</Text>
            {scheduled && <Text size="xs">{formatTime(scheduled)}</Text>}
          </Stack>
        }
        events={{ hover: true, focus: true, touch: true }}
      >
        <Box tabIndex={0} aria-label={label} style={{ height: NODE_CIRCLE_SLOT, display: 'flex', alignItems: 'center' }}>
          {circle}
        </Box>
      </Tooltip>
    </Box>
  );
}
