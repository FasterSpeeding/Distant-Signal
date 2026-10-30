'use client';

import { useEffect, useId, useRef } from 'react';
import { Box, Flex, Stack, Text, Tooltip, UnstyledButton } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import { journeyStopLabel, type JourneyEndpointNames } from './JourneyTimeline';
import { PlatformBadge } from './PlatformBadge';
import { DELAY_TONE_COLOR } from './DelayBadge';
import { WarningIcon } from './WarningIcon';
import { stopDisplayTime } from '@/lib/stopTimes';
import { trainPosition } from '@/lib/trainPosition';
import { legProgressIndex, type LegPhase, type LegStopRange } from '@/lib/legStopRange';
import type { JourneyStatus, JourneyStop, ResolutionStatus, TrackingPhase } from '@/lib/types';

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
 * Global Constraints for the full reasoning.
 *
 * `lastReportedLocation` is the SAME field `TrainJourney.tsx`'s
 * `JourneyDetails` already prints as "Last reported: X" -- passed through
 * here (Task 3.6.1) so a `status === 'en_route'` train whose last movement
 * report didn't match any timetabled stop (`lastReachedIndex` still `-1`)
 * can say where it actually was last confirmed, instead of reusing the
 * "waiting for its first movement report" copy that belongs to a train
 * with nothing confirmed at all. */
interface JourneyProgressProps {
  stops: JourneyStop[];
  /** `planned` (spec 2026-10-20 §4) captions an unreported train as
   * planned rather than waiting. Optional: other callers pass nothing. */
  trackingPhase?: TrackingPhase | null;
  resolutionStatus: ResolutionStatus;
  status: JourneyStatus | null;
  trainUid: string | null;
  mayHaveArrived: boolean;
  lastReportedLocation: string | null;
  // See `JourneyTimeline.tsx`'s own doc comment on `JourneyEndpointNames` --
  // the same tracked-pin origin/destination seed, threaded through here so
  // this diagram's endpoint labels/captions (`journeyStopLabel`, below)
  // never disagree with `JourneyTimeline`'s about the same stop. Optional
  // for the same reason it is there: a caller with no pin data on hand
  // still compiles, and just gets the by-index fallback.
  endpointNames?: JourneyEndpointNames;
  /** The journey leg's stops within `stops` (`legStopRange`): the diagram
   * then shows only those, from where the traveller boards to where they
   * alight, and its position and captions are relative to them. `null` or
   * omitted (a train page, or a leg whose stops weren't found) shows the
   * whole calling pattern. */
  legRange?: LegStopRange | null;
}

/** Origin and Terminate are the two node kinds that print their station
 * name as always-visible text (spec Decision 3), which is why they get a
 * bigger circle AND a wider layout slot (`.journeyProgressNode--endpoint`
 * in globals.css). One predicate for both so a future third "endpoint-ish"
 * kind can't end up sized one way and slotted the other. */
function isEndpoint(kind: JourneyStop['kind']): boolean {
  return kind === 'Origin' || kind === 'Terminate';
}

/** Whether a node is drawn as an endpoint (a bigger circle with its name
 * printed). On a journey leg's clipped diagram that is the boarding and
 * alighting stops, whatever kind of call they are on the train's own
 * route; otherwise the train's own Origin and Terminate. */
function nodeIsEndpoint(stop: JourneyStop, index: number, total: number, clipped: boolean): boolean {
  if (clipped) return index === 0 || index === total - 1;
  return isEndpoint(stop.kind);
}

function nodeDiameter(endpoint: boolean): number {
  return endpoint ? ENDPOINT_DIAMETER : INTERMEDIATE_DIAMETER;
}

/** How many of the train's stops a journey leg leaves out on one side:
 * before the traveller boards, or after they get off. */
interface LegContinuation {
  side: 'before' | 'after';
  count: number;
}

function stopCount(count: number): string {
  return `${count} ${count === 1 ? 'stop' : 'stops'}`;
}

/** The screen-reader restatement of a clipped leg's stubs: where the
 * train starts, if before the boarding stop, and where it runs on to, if
 * past the alighting stop. `null` when it does neither (no stubs drawn). */
export function legContinuationDescription(
  origin: { name: string; count: number } | null,
  terminus: { name: string; count: number } | null,
): string | null {
  const starts = origin ? `starts at ${origin.name}, ${stopCount(origin.count)} before you board` : null;
  const continues = terminus ? `continues to ${terminus.name}, ${stopCount(terminus.count)} after you get off` : null;
  if (starts && continues) return `The train ${starts}, and ${continues}.`;
  if (starts) return `The train ${starts}.`;
  if (continues) return `The train ${continues}.`;
  return null;
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
function nodeState(index: number, lastIndex: number, status: JourneyStatus | null, stop: JourneyStop): NodeState {
  // A backend that marks each stop (DS-181) says exactly which calls are
  // cancelled: a terminated-short train keeps the stops it still serves,
  // and a section cancelled between two stations shows even before a
  // later reached stop. An older one only has the train-wide status.
  if (stop.cancelled === true) return 'cancelled-remaining';
  if (lastIndex === -1 || index > lastIndex) {
    if (stop.cancelled === false) return 'not-reached';
    return status === 'cancelled' ? 'cancelled-remaining' : 'not-reached';
  }
  if (index === lastIndex) return 'marker';
  return 'reached';
}

type DelayState = 'on-time' | 'late' | 'early' | 'unknown';

/** Uses `DelayBadge`'s delay-to-colour table (`DELAY_TONE_COLOR`:
 * green/orange/teal), so a marker and a badge for the same stop agree, plus
 * one honest
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
  'on-time': DELAY_TONE_COLOR.onTime,
  late: DELAY_TONE_COLOR.late,
  early: DELAY_TONE_COLOR.early,
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
  lastReportedLocation: string | null,
  endpointNames: JourneyEndpointNames | undefined,
  /** `true` when `lastIndex` came from `reportedLocationIndex` rather than
   * from the per-stop `actual*` overlay -- the evidence is the same
   * movement report, but the wording stays the summary block's own ("last
   * reported at X") rather than claiming the stronger "currently at X",
   * which this app reserves for a stop the overlay itself confirmed. */
  markerFromReportedLocation: boolean,
  trackingPhase: TrackingPhase | null = null,
  /** A journey leg's progress (`legRange`): where the train is relative to
   * the leg, and the name of the stop it was last at (outside the leg's
   * stops when `phase` is `before`). `null` for the whole train. */
  leg: { phase: LegPhase; trainAt: string | null } | null = null,
): ProgressCopy {
  const terminus = stops[stops.length - 1];
  if (terminus === undefined) {
    return { caption: 'Not yet started.', ariaLabel: 'Journey progress: not yet started' };
  }

  const total = stops.length;
  // The leg's stops only: at or past the alighting stop the leg is done,
  // whatever the train does next; short of the boarding stop it hasn't
  // started, even though the train has.
  if (leg?.phase === 'after') {
    const alightName = journeyStopLabel(terminus, total - 1, total, endpointNames);
    return {
      caption: `Arrived at ${alightName}.`,
      ariaLabel: `Journey progress: arrived at ${alightName}`,
    };
  }
  if (leg?.phase === 'before' && leg.trainAt) {
    const board = stops[0];
    const boardName = board ? journeyStopLabel(board, 0, total, endpointNames) : null;
    if (status === 'cancelled') {
      return {
        caption: `Cancelled before reaching ${boardName}. Last confirmed at ${leg.trainAt}.`,
        ariaLabel: `Journey progress: cancelled before reaching ${boardName}, last confirmed at ${leg.trainAt}`,
      };
    }
    return {
      caption: `Not started. The train is on its way to ${boardName}, last reported at ${leg.trainAt}.`,
      ariaLabel: `Journey progress: not yet started, train on its way to ${boardName}, last reported at ${leg.trainAt}`,
    };
  }
  const markerStop = lastIndex >= 0 ? stops[lastIndex] : undefined;
  const markerName = markerStop ? journeyStopLabel(markerStop, lastIndex, total, endpointNames) : null;
  const stopNumber = lastIndex + 1;

  if (status === 'cancelled') {
    if (lastIndex === -1) {
      return {
        caption: 'Cancelled. No movement was ever confirmed.',
        ariaLabel: 'Journey progress: cancelled before any confirmed movement',
      };
    }
    return {
      caption: `Cancelled. Last confirmed at ${markerName}.`,
      ariaLabel: `Journey progress: cancelled, last confirmed at ${markerName}, stop ${stopNumber} of ${total}`,
    };
  }

  if (status === 'completed') {
    const terminusName = journeyStopLabel(terminus, total - 1, total, endpointNames);
    return {
      caption: `Arrived at ${terminusName}.`,
      ariaLabel: `Journey progress: arrived at ${terminusName}`,
    };
  }

  if (lastIndex === -1) {
    // Its first report isn't due yet: the timetable is the whole story.
    if (trackingPhase === 'planned' && status !== 'en_route') {
      return {
        caption: 'Planned. The timetabled route is shown.',
        ariaLabel: 'Journey progress: planned, timetabled route shown',
      };
    }
    if (resolutionStatus === 'schedule_matched') {
      return {
        caption: "Scheduled route shown. Live tracking hasn't started yet.",
        ariaLabel: 'Journey progress: scheduled route shown, live tracking not yet started',
      };
    }
    // `status === 'en_route'` here means Network Rail HAS confirmed this
    // train is running -- something was reported -- but that report never
    // matched a timetabled calling point (a passing point, junction, or
    // schedule mismatch), so `lastReachedIndex` still came back `-1`. That
    // is a materially different fact from "nothing has been confirmed yet"
    // (the `awaiting_activation` branch below) and must not reuse its
    // caption -- see Task 3.6.1: reusing it here directly contradicted a
    // summary above that already shows a confirmed delay/ETA for the same
    // train. The marker rule itself is untouched: `lastIndex === -1` still
    // means no marker is drawn (`nodeState`) -- only this caption differs.
    if (status === 'en_route') {
      if (lastReportedLocation) {
        return {
          caption: `Last reported at ${lastReportedLocation}. That report couldn't be matched to a timetabled stop, so no position is shown on the line.`,
          ariaLabel: `Journey progress: last reported at ${lastReportedLocation}, not matched to a timetabled stop, no position shown`,
        };
      }
      return {
        caption:
          "Confirmed en route, but its last movement report couldn't be matched to a timetabled stop, so no position is shown on the line.",
        ariaLabel: 'Journey progress: confirmed en route, not matched to a timetabled stop, no position shown',
      };
    }

    // resolved + awaiting_activation: a real train_uid is matched, nothing
    // has been confirmed at all -- the same copy StatusMessage uses for
    // awaiting_activation.
    return {
      caption: 'Matched to a timetabled train. Waiting for its first movement report.',
      ariaLabel: 'Journey progress: matched to a timetabled train, waiting for its first movement report',
    };
  }

  // en_route with a confirmed marker.
  if (markerFromReportedLocation) {
    return {
      caption: `Last reported at ${markerName}.`,
      ariaLabel: `Journey progress: last reported at ${markerName}, stop ${stopNumber} of ${total}`,
    };
  }
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
export function JourneyProgress({
  stops: allStops,
  trackingPhase = null,
  resolutionStatus,
  status,
  trainUid,
  mayHaveArrived,
  lastReportedLocation,
  endpointNames,
  legRange = null,
}: JourneyProgressProps) {
  const clipped = legRange !== null;
  const stops = legRange ? allStops.slice(legRange.start, legRange.end + 1) : allStops;
  // "May have arrived" is about the train's terminus: on a leg that gets
  // off earlier it says nothing about the leg's own last stop.
  const markerMayHaveArrived = mayHaveArrived && (!legRange || legRange.end === allStops.length - 1);
  // Two sources for the ONE marker index, in strict precedence order. The
  // per-stop `actual*`/PASS overlay wins whenever it has anything at all;
  // only when it is empty does the summary block's own
  // `lastReportedLocation` get to name a listed stop instead (C3 -- see
  // `reportedLocationIndex`). `-1` still means "no marker", but now only
  // when NEITHER source confirmed a calling point, which is the state the
  // "couldn't be matched to a timetabled stop" caption below actually
  // describes.
  //
  // Found on the train's WHOLE calling pattern, then (on a journey leg)
  // mapped onto the leg's own stops by `legProgressIndex`: before the
  // boarding stop nothing on the leg is reached, and at or past the
  // alighting stop all of it is.
  const position = trainPosition(allStops, lastReportedLocation, endpointNames);
  const legProgress = legRange ? legProgressIndex(position.index, legRange) : null;
  // A report (not a confirmed call) naming the alighting stop itself says
  // where the train was, not that the leg is over: it reads "last
  // reported at" like any other reported stop.
  const legPhase =
    legProgress?.phase === 'after' && position.fromReport && position.index === legRange?.end
      ? 'on'
      : (legProgress?.phase ?? null);
  const markerFromReportedLocation = position.fromReport && legPhase !== 'after';
  const lastIndex = legProgress ? legProgress.index : position.index;
  const beforeStop = legPhase === 'before' && position.index >= 0 ? allStops[position.index] : undefined;
  const trainAt = beforeStop ? journeyStopLabel(beforeStop, position.index, allStops.length, endpointNames) : null;
  // A clipped leg whose train also calls before the boarding stop or after
  // the alighting stop: a short dashed stub past that end of the line, a
  // "+n stops" note under that end's label, and the same in words as the
  // group's accessible description. Counts match `JourneyTimeline`'s
  // "Earlier stops"/"Later stops" toggles.
  const stopsBefore = legRange ? legRange.start : 0;
  const stopsAfter = legRange ? allStops.length - 1 - legRange.end : 0;
  const trainOrigin = allStops[0];
  const trainTerminus = allStops[allStops.length - 1];
  const continuationDescription = legContinuationDescription(
    stopsBefore > 0 && trainOrigin
      ? { name: journeyStopLabel(trainOrigin, 0, allStops.length, endpointNames), count: stopsBefore }
      : null,
    stopsAfter > 0 && trainTerminus
      ? {
          name: journeyStopLabel(trainTerminus, allStops.length - 1, allStops.length, endpointNames),
          count: stopsAfter,
        }
      : null,
  );
  const descriptionId = useId();
  function continuation(index: number): LegContinuation | null {
    if (index === 0 && stopsBefore > 0) return { side: 'before', count: stopsBefore };
    if (index === stops.length - 1 && stopsAfter > 0) return { side: 'after', count: stopsAfter };
    return null;
  }
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const nodeRefs = useRef<(HTMLDivElement | null)[]>([]);
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
  const { caption, ariaLabel } = progressCopy(
    stops,
    lastIndex,
    resolutionStatus,
    status,
    trainUid,
    markerMayHaveArrived,
    lastReportedLocation,
    endpointNames,
    markerFromReportedLocation,
    trackingPhase,
    legPhase ? { phase: legPhase, trainAt } : null,
  );

  // Scrolls THIS diagram's own horizontal scroll box, and only it --
  // deliberately not `node.scrollIntoView(...)`, which was the original
  // implementation and is what broke the train page on a phone.
  // `scrollIntoView` scrolls *every* scrollable ancestor of the node,
  // the document included, and its `block` option defaults to `'start'`
  // when omitted. On a narrow screen the diagram sits below the fold, so on
  // mount -- and again on every poll refresh that advanced the marker --
  // the whole page jumped down to put a 12px circle at the top of the
  // viewport; `inline: 'center'` could drag the document sideways too.
  // `Element.scrollTo` on the one element we name can't move anything else.
  useEffect(() => {
    if (lastIndex === -1) return;
    const container = scrollRef.current;
    const node = nodeRefs.current[lastIndex];
    if (!container || !node) return;
    // Measured rather than derived from `offsetLeft`: the node's
    // `offsetParent` is `.journeyProgressLine` (it's `position: relative`),
    // not the scroll box, so `offsetLeft` would silently exclude the scroll
    // box's own padding. The delta between the two centers needs no such
    // assumption.
    const nodeRect = node.getBoundingClientRect();
    const containerRect = container.getBoundingClientRect();
    const delta = nodeRect.left + nodeRect.width / 2 - (containerRect.left + containerRect.width / 2);
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    container.scrollTo({
      left: container.scrollLeft + delta,
      behavior: reduceMotion ? 'auto' : 'smooth',
    });
    // `stops.length` as well as `lastIndex`: a poll refresh that picks up a
    // schedule alteration can add or remove calling points without moving
    // the marker's index, which slides the marker to a different x offset
    // under a `lastIndex`-only dependency and leaves it off-screen.
  }, [lastIndex, stops.length]);

  const endpointCount = stops.filter((stop, index) => nodeIsEndpoint(stop, index, stops.length, clipped)).length;

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
          bullet. `group` keeps (a) -- the same per-state label, announced
          on entering the container -- while letting (b) actually work. A
          group name is announced at group entry rather than inline like an
          `img`'s, and a low-verbosity screen reader may skip it, so the
          `caption` `Text` rendered as this box's sibling below is the
          always-present, always-read restatement of the same fact; the two
          are deliberately redundant. Do NOT change this back to `img`
          without also making the triggers non-focusable; the two halves
          must agree. */}
      <Box
        ref={scrollRef}
        className="journeyProgressScroll"
        data-journey-progress-scroll
        role="group"
        aria-label={ariaLabel}
        aria-describedby={continuationDescription ? descriptionId : undefined}
        // eslint-disable-next-line react/forbid-component-props -- stop counts known only at render time, as CSS variables
        style={{
          // Counts only -- every px measurement the diagram uses lives in
          // `.journeyProgressScroll`'s own custom properties in
          // globals.css, so the `max-width: $mantine-breakpoint-sm` block
          // there can rescale the whole diagram for a phone. A media query
          // can't reach into a React style object, which is exactly why
          // the old inline `minWidth: stops.length * NODE_SLOT_WIDTH`
          // could never have a mobile value.
          '--journey-progress-count': String(stops.length),
          '--journey-progress-endpoint-count': String(endpointCount),
        }}
      >
        <Box
          className="journeyProgressLine"
          // eslint-disable-next-line react/forbid-component-props -- hands the NODE_CIRCLE_SLOT constant to the stylesheet as a CSS variable
          style={{
            // Read back by `.journeyProgressLine::before`'s `top` in
            // globals.css, instead of that rule hardcoding half of
            // `NODE_CIRCLE_SLOT` as its own separate literal -- so the
            // connecting line's vertical position can't silently drift
            // out of sync with the circle-centering slot it's meant to
            // bisect.
            '--journey-progress-node-slot': `${NODE_CIRCLE_SLOT}px`,
          }}
        >
          {/* eslint-disable-next-line react-hooks/refs -- nodeRefSetter reads a ref only to reuse cached callback refs (see nodeRefSetters) */}
          {stops.map((stop, index) => (
            <JourneyProgressNode
              key={`${stop.crs ?? 'unknown'}-${index}`}
              stop={stop}
              index={index}
              total={stops.length}
              lastIndex={lastIndex}
              status={status}
              mayHaveArrived={markerMayHaveArrived}
              endpoint={nodeIsEndpoint(stop, index, stops.length, clipped)}
              endpointNames={endpointNames}
              nodeRef={nodeRefSetter(index)}
              continuation={continuation(index)}
            />
          ))}
        </Box>
      </Box>
      {/* Hidden, but still read as the group's description through
          `aria-describedby`: shown, it would repeat what the stubs and
          their "+n stops" notes already say. */}
      {continuationDescription && (
        <span id={descriptionId} hidden>
          {continuationDescription}
        </span>
      )}
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
  total,
  lastIndex,
  status,
  mayHaveArrived,
  endpoint,
  endpointNames,
  nodeRef,
  continuation = null,
}: {
  stop: JourneyStop;
  index: number;
  total: number;
  lastIndex: number;
  status: JourneyStatus | null;
  mayHaveArrived: boolean;
  endpoint: boolean;
  endpointNames: JourneyEndpointNames | undefined;
  nodeRef: (el: HTMLDivElement | null) => void;
  /** Set on a clipped leg's boarding or alighting node when the train also
   * calls beyond it on that side. */
  continuation?: LegContinuation | null;
}) {
  const diameter = nodeDiameter(endpoint);
  const state = nodeState(index, lastIndex, status, stop);
  const delay = delayState(stop.delayMinutes);
  const label = journeyStopLabel(stop, index, total, endpointNames);
  // Same departure-first precedence as `JourneyTimeline.tsx`'s own
  // `scheduled` (see `journeyStopLabel`'s doc comment) -- this tooltip
  // shows the exact same time that stop's row shows in the table below.
  const scheduled = stopDisplayTime(stop);
  const isMarker = state === 'marker';

  const circle = (
    <Box
      ref={nodeRef}
      data-journey-node
      data-node-state={state}
      data-delay-state={delay}
      aria-hidden="true"
      // eslint-disable-next-line react/forbid-component-props -- size and colours come from the stop's state at render time (circleStyle)
      style={{
        width: diameter,
        height: diameter,
        borderRadius: '50%',
        // The `zIndex` below is what keeps the connecting line
        // (absolutely-positioned `.journeyProgressNode::before`/`::after`
        // in globals.css, painted with the z-index:0 group) from drawing
        // straight across the middle of every circle. It already applied
        // without `position`, but only by way of a Flexbox special case --
        // this circle is a flex item of `circleSlot`, and a flex item's
        // `z-index` creates a stacking context even when it is statically
        // positioned (CSS Flexbox 1 section 4.3, Flex Item Z-Ordering).
        // `position: relative` makes the z-index apply under the ordinary
        // rule instead, so the circle keeps painting above the line even if
        // `circleSlot` ever stops being a flex container.
        position: 'relative',
        zIndex: 1,
        ...circleStyle(state, delay),
      }}
    />
  );

  const circleSlot = (
    <Flex h={NODE_CIRCLE_SLOT} align="center" justify="center">
      {circle}
    </Flex>
  );

  // Not a color change (spec Decision 5's own stated reason: color alone
  // must never be the only signal, and the marker's fill already encodes
  // delay) -- a small glyph next to the marker, consistent with the
  // "May have arrived" `Alert` already shown above this diagram by
  // `TrainJourney.tsx`'s `StatusMessage`.
  const glyph =
    isMarker && mayHaveArrived ? (
      // An inline SVG (`WarningIcon`), not the literal "⚠" character, which
      // some platforms paint as a full-colour emoji that ignores the theme
      // entirely. Yellow's light-variant text token -- the colour of the
      // "May have arrived" `Alert` it echoes -- clears the 3:1 non-text
      // floor in both schemes (#964d00: 6.25:1 on white; Mantine's dark
      // yellow light-color on the dark body well above that). Still
      // `aria-hidden`: the progress group's own `aria-label` already says
      // "may have arrived" in words.
      <Box
        aria-hidden="true"
        data-may-have-arrived="true"
        lh={1}
        display="flex"
        c="var(--mantine-color-yellow-light-color)"
      >
        <WarningIcon size={12} />
      </Box>
    ) : null;

  // Both node kinds are the SAME plain `Box` carrying the SAME
  // `.journeyProgressNode` class (an endpoint adds a modifier for its wider
  // slot) -- deliberately not a Mantine `Stack` for one and a `Box` for the
  // other. The column/centering/gap layout and, crucially, the flex sizing
  // that has to respond to globals.css's mobile breakpoint now live in one
  // stylesheet rule for both, so the two kinds can't drift and no inline
  // `flex` shorthand races the media query.
  if (endpoint) {
    return (
      <Box
        className="journeyProgressNode journeyProgressNode--endpoint"
        data-journey-node-slot="endpoint"
        data-leg-continues={continuation?.side}
      >
        {circleSlot}
        {glyph}
        <Text size="xs" fw={700} ta="center" className="journeyProgressLabel">
          {label}
        </Text>
        {/* `aria-hidden`: the group's description says it in full. */}
        {continuation && (
          <Text size="xs" c="dimmed" ta="center" aria-hidden="true" data-leg-continues-note>
            +{stopCount(continuation.count)}
          </Text>
        )}
        {/* Compact badge, not the full `ScheduleRow` list-row component --
            this is a dense schematic diagram, not a list, so
            `PlatformBadge` alone (already the smallest, colour+text unit
            that convention has) fits without crowding the line. `null`
            for every stop except (today) the ORIGIN -- see
            `JourneyStop.platform`'s own doc comment -- and `PlatformBadge`
            itself renders nothing for a `null` platform, so a Terminate
            endpoint (which never has one yet) simply shows no badge, no
            layout gap reserved for it. */}
        <PlatformBadge
          platform={stop.platform}
          plannedPlatform={stop.plannedPlatform}
          platformChanged={stop.platformChanged}
          platformStatus={stop.platformStatus}
          bookedPlatform={stop.bookedPlatform}
          platformBasis={stop.platformBasis}
        />
      </Box>
    );
  }

  return (
    <Box className="journeyProgressNode" data-journey-node-slot="intermediate">
      <Tooltip
        label={
          <Stack gap={2}>
            <Text size="xs">{label}</Text>
            {scheduled && <Text size="xs">{formatTime(scheduled)}</Text>}
            {/* See `JourneyTimeline.tsx`'s own `skipCaption` for the
                confidence-hedged wording convention this mirrors --
                intentionally the SAME two strings, so this diagram's
                tooltip never disagrees with the table row below it about
                how confidently to word the same stop. */}
            {stop.stopStatus === 'Skipped' && (
              <Text size="xs" c="dimmed">
                {stop.skipSource === 'Trust' ? 'Does not appear to have stopped here' : 'Did not stop here'}
              </Text>
            )}
          </Stack>
        }
        events={{ hover: true, focus: true, touch: true }}
      >
        {/* Wraps the same `circleSlot` used for an endpoint node -- rather
            than a hand-duplicated copy of its style -- so the two node
            kinds can never drift out of alignment with each other. The
            focusable/labelled Tooltip trigger itself is this outer
            element. `.journeyProgressTrigger` stretches it to the full slot
            width and floors its height at 24px: the circle it wraps is 12px
            across, and a 12px tap target is the only route to an
            intermediate stop's name on a touch screen.

            A real button, not a bare `tabIndex={0}` div: the same
            "focusable element with a real role and its own `aria-label`"
            shape `LineDefinitionTooltip.tsx`'s `ActionIcon` trigger
            already uses, which is the existing codebase pattern spec
            Decision 6 points at. A focusable div has role `generic`, and
            an `aria-label` on a `generic` element names nothing, so that
            shape would leave the trigger focusable-but-silent even now
            that the container is a `group`.

            `UnstyledButton` (already used by `AllLinesTable.tsx`) rather
            than a hand-rolled `component="button"` style reset, because it
            carries Mantine's `.mantine-focus-auto` class -- so a focused
            node shows the SAME focus ring as every other control in the
            app, not the UA default -- plus the `appearance`/
            `-webkit-tap-highlight-color` resets a hand-rolled one keeps
            forgetting (without the latter, iOS Safari paints a grey tap
            box around the 12px circle). Its layout still comes entirely
            from `.journeyProgressTrigger`, which is declared after
            Mantine's own styles and so wins on source order.

            `aria-describedby` for the tooltip body itself is added by
            Mantine/floating-ui's `useRole(..., { role: 'tooltip' })` while
            it's open. There is deliberately no `onClick` and pressing
            Enter/Space does nothing -- the tooltip is already open from
            focus, and hover/focus/touch are the only things that reveal
            it. That inert-on-activation trigger is the accepted tradeoff
            of the APG tooltip pattern (a focusable element must have a
            real role, and `button` is the one screen readers announce
            reliably), not an unfinished handler. */}
        <UnstyledButton type="button" aria-label={label} className="journeyProgressTrigger">
          {circleSlot}
        </UnstyledButton>
      </Tooltip>
      {glyph}
    </Box>
  );
}
