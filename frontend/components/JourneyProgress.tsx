'use client';

import { Box } from '@mantine/core';
import type { JourneyStatus, JourneyStop, ResolutionStatus } from '@/lib/types';

const NODE_SLOT_WIDTH = 56;

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
        {stops.map((stop, index) => {
          const diameter = nodeDiameter(stop.kind);
          const state = nodeState(index, lastIndex);
          const delay = delayState(stop.delayMinutes);
          return (
            <Box
              key={`${stop.crs ?? 'unknown'}-${index}`}
              style={{ flex: `0 0 ${NODE_SLOT_WIDTH}px`, display: 'flex', justifyContent: 'center' }}
            >
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
            </Box>
          );
        })}
      </Box>
    </Box>
  );
}
