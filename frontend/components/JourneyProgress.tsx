'use client';

import { useEffect, useRef } from 'react';
import { Box, Stack, Text, Tooltip } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import { journeyStopLabel } from './JourneyTimeline';
import type { JourneyStatus, JourneyStop, ResolutionStatus } from '@/lib/types';

const NODE_SLOT_WIDTH = 56;

/** The one place an Origin/Terminate node's diameter is defined --
 * `nodeDiameter` and `NODE_CIRCLE_SLOT` both derive from this so the two
 * can never silently drift apart. */
const ENDPOINT_DIAMETER = 18;
const INTERMEDIATE_DIAMETER = 12;

/** Every node's circle -- whatever kind, whatever renders below it (an
 * always-visible label for an endpoint, nothing for a bare intermediate
 * node) -- is centered inside a slot of this fixed height, anchored to the
 * top of its flex item (`.journeyProgressLine`'s `align-items: flex-start`
 * in `globals.css`). That keeps every circle's vertical center at the same
 * offset (`NODE_CIRCLE_SLOT / 2`) regardless of the varying total height
 * Task 3's endpoint labels (and Task 6's `mayHaveArrived` glyph) introduce
 * below the circle. Set equal to `ENDPOINT_DIAMETER` -- the largest circle
 * -- so no circle ever overflows its own slot. The connecting line's own
 * vertical offset (`.journeyProgressLine::before`'s `top` in `globals.css`)
 * reads this same value back via the `--journey-progress-node-slot` CSS
 * custom property set below, rather than duplicating the number, so the
 * two can't desync either. */
const NODE_CIRCLE_SLOT = ENDPOINT_DIAMETER;

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
  return kind === 'Origin' || kind === 'Terminate' ? ENDPOINT_DIAMETER : INTERMEDIATE_DIAMETER;
}

type NodeState = 'reached' | 'marker' | 'not-reached' | 'cancelled-remaining';

/** `index <= lastIndex` (and `lastIndex !== -1`) is "reached"; the stop
 * exactly at `lastIndex` is additionally the "you are here" marker; every
 * later index is "not yet reached" -- or, on a cancelled journey,
 * `'cancelled-remaining'` instead, so it reads visibly differently from
 * "just hasn't got there yet" (spec Decision 5). `lastIndex === -1` means
 * nothing has been confirmed, so every node takes the `index > lastIndex`
 * branch and no marker exists -- this falls out of the comparison rather
 * than needing a special case, including for a cancelled journey with no
 * confirmed movement at all: every stop is already `'cancelled-remaining'`
 * there too. */
function nodeState(index: number, lastIndex: number, status: JourneyStatus | null): NodeState {
  if (lastIndex === -1 || index > lastIndex) {
    return status === 'cancelled' ? 'cancelled-remaining' : 'not-reached';
  }
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
  if (state === 'cancelled-remaining') {
    return { border: '2px dashed var(--mantine-color-gray-5)', backgroundColor: 'transparent' };
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

interface ProgressCopy {
  caption: string;
  ariaLabel: string;
}

/** One pair of strings for every row of the status/resolution decision
 * table -- see this plan's Global Constraints for the table copied
 * verbatim from the spec. `caption` is the always-visible `Text` shown
 * under the diagram; `ariaLabel` is the diagram container's textual
 * restatement (spec Decision 6 -- the container is a labelled
 * `role="group"`, see `JourneyProgress`'s own comment for why that role
 * and not `role="img"`). They are independent strings, not one string
 * reused twice, because the aria-label states the exact stop position
 * ("stop N of Total") a sighted caption doesn't need spelled out. */
function progressCopy(
  stops: JourneyStop[],
  lastIndex: number,
  resolutionStatus: ResolutionStatus,
  status: JourneyStatus | null,
  trainUid: string | null,
  mayHaveArrived: boolean,
): ProgressCopy {
  if (stops.length === 0) {
    return { caption: 'Not yet started.', ariaLabel: 'Journey progress: not yet started' };
  }

  const total = stops.length;
  const markerName = lastIndex >= 0 ? journeyStopLabel(stops[lastIndex]) : null;
  const stopNumber = lastIndex + 1;

  if (status === 'cancelled') {
    if (lastIndex === -1) {
      return {
        caption: 'Cancelled — no movement was ever confirmed.',
        ariaLabel: 'Journey progress: cancelled before any confirmed movement',
      };
    }
    return {
      caption: `Cancelled — last confirmed at ${markerName}.`,
      ariaLabel: `Journey progress: cancelled, last confirmed at ${markerName}, stop ${stopNumber} of ${total}`,
    };
  }

  if (status === 'completed') {
    const terminusName = journeyStopLabel(stops[total - 1]);
    return {
      caption: `Arrived at ${terminusName}.`,
      ariaLabel: `Journey progress: arrived at ${terminusName}`,
    };
  }

  if (lastIndex === -1) {
    if (resolutionStatus === 'schedule_matched') {
      return {
        caption: "Scheduled route shown — live tracking hasn't started yet.",
        ariaLabel: 'Journey progress: scheduled route shown, live tracking not yet started',
      };
    }
    // resolved + awaiting_activation, or resolved + en_route with no
    // confirmed movement yet -- both mean "a real train_uid is matched,
    // nothing has been confirmed", the same copy StatusMessage uses for
    // awaiting_activation.
    return {
      caption: `Matched to train ${trainUid} — waiting for its first movement report.`,
      ariaLabel: `Journey progress: matched to train ${trainUid}, waiting for first movement report`,
    };
  }

  // en_route with a confirmed marker.
  return {
    caption: `Currently at ${markerName}.`,
    ariaLabel: mayHaveArrived
      ? `Journey progress: currently at ${markerName} (may have arrived), stop ${stopNumber} of ${total}`
      : `Journey progress: currently at ${markerName}, stop ${stopNumber} of ${total}`,
  };
}

/** Schematic, index-spaced (NOT time/distance-proportional) "you are here"
 * line diagram, additive to `JourneyTimeline` -- see
 * docs/superpowers/specs/2026-09-12-journey-progress-visualization-design.md.
 * Rendered directly above `JourneyTimeline` in `TrainJourney.tsx`, behind
 * the identical `{state.journeyStops && ...}` guard. */
export function JourneyProgress({ stops, resolutionStatus, status, trainUid, mayHaveArrived }: JourneyProgressProps) {
  const lastIndex = lastReachedIndex(stops);
  const nodeRefs = useRef<Array<HTMLDivElement | null>>([]);
  // One stable callback-ref per index, cached across renders, so a re-render
  // that doesn't change `stops.length` (e.g. a poll refresh with the same
  // stop count) doesn't hand every node a brand-new ref function -- React
  // would otherwise call the old one with `null` and the new one with the
  // element on every single render, for every node, for no reason.
  const nodeRefSetters = useRef<Map<number, (el: HTMLDivElement | null) => void>>(new Map());
  function nodeRefSetter(index: number) {
    let setter = nodeRefSetters.current.get(index);
    if (!setter) {
      setter = (el) => {
        nodeRefs.current[index] = el;
      };
      nodeRefSetters.current.set(index, setter);
    }
    return setter;
  }
  const { caption, ariaLabel } = progressCopy(stops, lastIndex, resolutionStatus, status, trainUid, mayHaveArrived);

  useEffect(() => {
    if (lastIndex === -1) return;
    const node = nodeRefs.current[lastIndex];
    if (!node) return;
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    node.scrollIntoView({ inline: 'center', behavior: reduceMotion ? 'auto' : 'smooth' });
  }, [lastIndex]);

  return (
    <Stack gap="xs">
      {/* `role="group"` + `aria-label`, NOT `role="img"`. Spec Decision 6
          asks for two things at once: (a) a single container-level string
          that restates what the diagram shows, and (b) per-node `Tooltip`
          triggers that "remain independently focusable/readable for a
          sighted keyboard user who wants the intermediate-stop detail".
          `role="img"` can only deliver (a): per ARIA, an `img`'s subtree is
          presentational, so every descendant -- including those focusable
          triggers -- is dropped from the accessibility tree. That made the
          triggers focusable but silent (a keyboard user tabs onto a node
          and a screen reader announces nothing), which is a WCAG 4.1.2
          Name/Role/Value failure and contradicts Decision 6's own second
          bullet. `group` keeps (a) -- the label is announced on entering
          the container, exactly as the `img` label was -- while letting
          (b) actually work. Do NOT change this back to `img` without also
          making the triggers non-focusable; the two halves must agree. */}
      <Box role="group" aria-label={ariaLabel} style={{ overflowX: 'auto' }}>
        <Box
          className="journeyProgressLine"
          style={
            {
              minWidth: stops.length * NODE_SLOT_WIDTH,
              // Read back by `.journeyProgressLine::before`'s `top` in
              // globals.css, instead of that rule hardcoding half of
              // `NODE_CIRCLE_SLOT` as its own separate literal -- so the
              // connecting line's vertical position can't silently drift
              // out of sync with the circle-centering slot it's meant to
              // bisect.
              '--journey-progress-node-slot': `${NODE_CIRCLE_SLOT}px`,
            } as React.CSSProperties
          }
        >
          {stops.map((stop, index) => (
            <JourneyProgressNode
              key={`${stop.crs ?? 'unknown'}-${index}`}
              stop={stop}
              index={index}
              lastIndex={lastIndex}
              status={status}
              mayHaveArrived={mayHaveArrived}
              nodeRef={nodeRefSetter(index)}
            />
          ))}
        </Box>
      </Box>
      <Text size="sm" c="dimmed">
        {caption}
      </Text>
    </Stack>
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
 * *trigger wrapper* -- a style-reset `<button>` -- carries its own
 * `aria-label` and stays keyboard focusable instead. An endpoint node has
 * no trigger and is not focusable at all: its name is already visible
 * text, so a tab stop there would announce something the reader can
 * already read. */
function JourneyProgressNode({
  stop,
  index,
  lastIndex,
  status,
  mayHaveArrived,
  nodeRef,
}: {
  stop: JourneyStop;
  index: number;
  lastIndex: number;
  status: JourneyStatus | null;
  mayHaveArrived: boolean;
  nodeRef: (el: HTMLDivElement | null) => void;
}) {
  const diameter = nodeDiameter(stop.kind);
  const state = nodeState(index, lastIndex, status);
  const delay = delayState(stop.delayMinutes);
  const isEndpoint = stop.kind === 'Origin' || stop.kind === 'Terminate';
  const label = journeyStopLabel(stop);
  // Same departure-first precedence as `JourneyTimeline.tsx`'s own
  // `scheduled` (see `journeyStopLabel`'s doc comment) -- this tooltip
  // shows the exact same time that stop's row shows in the table below.
  const scheduled = stop.scheduledDeparture ?? stop.scheduledArrival;
  const isMarker = state === 'marker';

  const circle = (
    <Box
      ref={nodeRef}
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

  // Not a color change (spec Decision 5's own stated reason: color alone
  // must never be the only signal, and the marker's fill already encodes
  // delay) -- a small glyph next to the marker, consistent with the
  // "May have arrived" `Alert` already shown above this diagram by
  // `TrainJourney.tsx`'s `StatusMessage`.
  const glyph =
    isMarker && mayHaveArrived ? (
      <Text aria-hidden="true" size="xs" data-may-have-arrived="true" style={{ lineHeight: 1 }}>
        ⚠
      </Text>
    ) : null;

  if (isEndpoint) {
    return (
      <Stack gap={4} align="center" style={{ flex: `0 0 ${NODE_SLOT_WIDTH}px` }}>
        {circleSlot}
        {glyph}
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
        {/* Wraps the same `circleSlot` used for an endpoint node -- rather
            than a hand-duplicated copy of its style -- so the two node
            kinds can never drift out of alignment with each other. The
            focusable/labelled Tooltip trigger itself is this outer
            element; it has no fixed size of its own and just inherits
            `circleSlot`'s height.

            A real `<button>` (style-reset to nothing, since the circle
            inside is the entire visual), not a bare `tabIndex={0}` div:
            the same "focusable element with a real role and its own
            `aria-label`" shape `LineDefinitionTooltip.tsx`'s `ActionIcon`
            trigger already uses, which is the existing codebase pattern
            spec Decision 6 points at. A focusable div has role `generic`,
            and an `aria-label` on a `generic` element names nothing, so
            that shape would leave the trigger focusable-but-silent even
            now that the container is a `group`. `aria-describedby` for the
            tooltip body itself is added by Mantine/floating-ui's
            `useRole(..., { role: 'tooltip' })` while it's open. There is
            deliberately no `onClick`: hover/focus/touch all reveal the
            same tooltip, and a keyboard press falls through to the
            Tooltip's own focus handling. */}
        <Box
          component="button"
          type="button"
          aria-label={label}
          style={{
            display: 'block',
            padding: 0,
            margin: 0,
            border: 'none',
            background: 'none',
            font: 'inherit',
            color: 'inherit',
            cursor: 'pointer',
          }}
        >
          {circleSlot}
        </Box>
      </Tooltip>
      {glyph}
    </Box>
  );
}
