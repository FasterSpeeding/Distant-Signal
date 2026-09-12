# Schematic Journey Progress Visualization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a compact, schematic "you are here" line diagram (`JourneyProgress.tsx`) above the existing `JourneyTimeline` table, so a reader can see roughly where a train is along its route at a glance, without scanning the detailed table.

**Architecture:** A new `"use client"` Mantine-primitives-only component reads the exact same `TrainJourneyState.journeyStops` array `JourneyTimeline` already reads, plus four more `TrainJourneyState` fields (`resolutionStatus`, `status`, `trainUid`, `mayHaveArrived`) needed to render the full status-matrix captions. It derives the "you are here" position client-side, once, from the highest-index stop with a confirmed `actualArrival`/`actualDeparture` — never from `estimatedArrival`/`estimatedDeparture`, never interpolated. It renders additively, directly above `JourneyTimeline`, behind the identical `{state.journeyStops && ...}` guard.

**Tech Stack:** Next.js App Router, React Client Component, TypeScript, Mantine v9 (`Box`, `Stack`, `Text`, `Tooltip` — no charting/SVG library), one small addition to `frontend/app/globals.css` for the connecting-line pseudo-element, Vitest + `@testing-library/react` (`frontend/test/render.tsx`'s `renderWithMantine`), run via `npx vitest run <path>` / `npm test` from `frontend/`.

**Spec:** `docs/superpowers/specs/2026-09-12-journey-progress-visualization-design.md`

## Global Constraints

- **Zero new backend work.** No new route, no new field on `JourneyStop`/`TrainJourneyState`, no new derivation in `crates/api`. Confirmed directly against `crates/api/src/data/journey.rs`: `JourneyStop` (its `struct` definition and `build_journey_stops`/`apply_delay_estimates`) already carries every field below, serialized camelCase; `CallingPointKind` (`crates/schedule-query/src/records.rs`) serializes as `"Origin" | "Intermediate" | "Terminate"`, matching `JourneyStopKind` on the wire.
- **`JourneyStop` fields this component reads, verbatim from `frontend/lib/types.ts:376-403`:**
  ```ts
  export type JourneyStopKind = 'Origin' | 'Intermediate' | 'Terminate';

  export interface JourneyStop {
    crs: string | null;
    name: string | null;
    tiploc: string | null;
    kind: JourneyStopKind | null;
    scheduledArrival: string | null;    // RFC3339
    scheduledDeparture: string | null;  // RFC3339
    actualArrival: string | null;       // RFC3339 -- a REAL confirmed TRUST event
    actualDeparture: string | null;     // RFC3339 -- a REAL confirmed TRUST event
    estimatedArrival: string | null;    // RFC3339 -- current-delay PROPAGATION only
    estimatedDeparture: string | null;
    lastEventType: string | null;       // "ARRIVAL" | "DEPARTURE" | "PASS"
    variationStatus: string | null;
    delayMinutes: number | null;
  }
  ```
- **`TrainJourneyState` fields this component's extra props are sourced from, verbatim from `frontend/lib/types.ts:357-358, 440-495`:** `resolutionStatus: ResolutionStatus` where `ResolutionStatus = 'pending' | 'schedule_matched' | 'resolved' | 'unresolved'`; `status: JourneyStatus | null` where `JourneyStatus = 'awaiting_activation' | 'en_route' | 'cancelled' | 'completed'`; `trainUid: string | null`; `journeyStops: JourneyStop[] | null`; `mayHaveArrived: boolean`.
- **Prop-shape resolution (an ambiguity in the spec, resolved here, before Task 1):** the spec's Decision 1 illustrative snippet passes only `stops={state.journeyStops}` into `JourneyProgress`. But Decision 5's caption/aria-label text ("Matched to train {trainUid}…", "Cancelled…", "Arrived at…") cannot be derived from `stops` alone: `schedule_matched` and `resolved`+`awaiting_activation` produce *identical* `stops`/`lastReachedIndex` shapes (populated stops, `lastReachedIndex === -1`) but need different caption wording, and only `resolutionStatus` distinguishes them; `cancelled`/`completed`/`mayHaveArrived` are likewise not recoverable from `stops` alone. This plan's `JourneyProgress` therefore takes a wider prop set than Decision 1's snippet literally shows, while inventing no field names — every extra prop is copied verbatim off `TrainJourneyState`:
  ```ts
  interface JourneyProgressProps {
    stops: JourneyStop[];
    resolutionStatus: ResolutionStatus;
    status: JourneyStatus | null;
    trainUid: string | null;
    mayHaveArrived: boolean;
  }
  ```
  `TrainJourney.tsx`'s wiring becomes `<JourneyProgress stops={state.journeyStops} resolutionStatus={state.resolutionStatus} status={state.status} trainUid={state.trainUid} mayHaveArrived={state.mayHaveArrived} />`, still gated on the identical `{state.journeyStops && ...}` non-null guard Decision 1 specifies.
- **The full status/resolution decision table (spec Decision 5), copied verbatim:**

| Backend state | `journeyStops` | `lastReachedIndex` | `JourneyProgress` renders |
|---|---|---|---|
| `pending` / `unresolved` | `null` | n/a | **Nothing** — the `{state.journeyStops && ...}` guard means the component isn't mounted at all. `StatusMessage`'s existing "Waiting to hear from Network Rail" / "Couldn't be matched" text is the only thing shown, unchanged. |
| `schedule_matched` | populated (always) | `-1` (no movement data exists yet) | The full line of hollow "not yet reached" nodes, origin/terminus labeled, **no "you are here" marker at all** — not a marker parked at the origin. A caption: *"Scheduled route shown — live tracking hasn't started yet."* |
| `resolved` + `awaiting_activation` | populated | `-1` | Same rendering as `schedule_matched` above — the caption instead reads *"Matched to train {trainUid} — waiting for its first movement report."* |
| `resolved` + `en_route`, `mayHaveArrived === false` | populated | ≥ 0 (typically) | The main case: full line, "you are here" marker at `lastReachedIndex`, everything past it hollow. |
| `resolved` + `en_route`, `mayHaveArrived === true` | populated | ≥ 0 | Same marker placement as above (it is **not** moved to the terminus). An additional small warning glyph on the marker itself (not a color change) makes the diagram consistent with the text alert already shown above it. |
| `resolved` + `cancelled` | populated | ≥ -1 (whatever was last confirmed before cancellation, possibly `-1`) | Marker frozen at `lastReachedIndex` exactly as computed. Every node from `lastReachedIndex + 1` onward is rendered in a distinct **cancelled** hollow style (a dashed/greyed-out ring rather than the plain "not yet reached" ring). A caption echoes the red "Cancelled" `Alert`. |
| `resolved` + `completed` | populated | Normally the final index (the terminus has a confirmed `actualArrival`) | Marker at the terminus, rendered with the same "reached" styling as any other node. Caption echoes the green "Arrived" `Alert`. |
| Neither source has anything for this `train_uid`/date | `null` | n/a | Same as `pending`/`unresolved` — component doesn't mount. |

  **Cardinal rule restated:** the marker only ever sits on an index where `lastReachedIndex` says a confirmed event exists, or it doesn't render at all. No state advances it, ages it forward, or infers a "probably at the next stop by now" position.
- **No new dependency, no SVG, no charting library.** Plain Mantine primitives (`Box`, `Stack`, `Text`, `Tooltip`) + one small addition to `frontend/app/globals.css` for the connecting-line pseudo-element, matching this codebase's existing `.issueRow`/`.prideSparkle` global-class convention (there are no CSS Modules in this frontend — confirmed by `find frontend -iname '*.module.css'` returning nothing).
- **`"use client"` from the first line of `JourneyProgress.tsx`, from Task 1 onward** — Decision 7 requires it for `scrollIntoView`/`matchMedia`, both browser-only APIs introduced in Task 4; `TrainJourney`/`JourneyTimeline`/the page chain stay Server Components untouched, matching `AutoRefresh.tsx`/`PinToggle.tsx`'s existing "one interactive leaf inside an otherwise-static tree" pattern.
- **Test framework:** Vitest, colocated `*.test.tsx`, `frontend/test/render.tsx`'s `renderWithMantine`, the `stop()` fixture-builder pattern from `frontend/components/JourneyTimeline.test.tsx:7-24` (reused verbatim, redeclared locally per test file — this codebase has no shared test-fixtures module). Hover interactions use `fireEvent.mouseEnter`/`screen.findByText` (matching `LineDefinitionTooltip.test.tsx`'s established pattern) — there is no `@testing-library/user-event` dependency in this repo. Rerendering the *same* mounted instance under `renderWithMantine` requires re-wrapping the new tree in `<MantineProvider theme={theme}>` explicitly (`IssueList.test.tsx:680-682`'s documented gotcha: RTL's `rerender` replaces the whole tree, and `renderWithMantine`'s `MantineProvider` wrapper is not preserved across a bare `rerender` call).
- **`prefers-reduced-motion` precedent:** no existing JS call site in this frontend does a live `window.matchMedia('(prefers-reduced-motion: reduce)')` check — `PrideToggle.tsx`'s own reduced-motion handling is CSS-only (`@media (prefers-reduced-motion: no-preference)` in `globals.css`). `frontend/vitest.setup.ts` polyfills `window.matchMedia` to always report `matches: false`; a test needing `matches: true` must locally override it with `vi.spyOn(window, 'matchMedia').mockImplementation(...)`. jsdom also has no `Element.prototype.scrollIntoView` implementation — tests that need it must stub `window.HTMLElement.prototype.scrollIntoView = vi.fn()`.
- **Accessibility (spec Decision 6):** outer container carries `role="img"` + an `aria-label` that textually restates what the diagram shows, one string per Decision 5 row (exact strings defined in Task 5). Individual node circles carry `aria-hidden="true"` (decorative — visible station-name text sits alongside them, not inside the hidden element); the Tooltip-trigger wrapper for a bare intermediate node is NOT aria-hidden and remains keyboard-focusable (`tabIndex={0}` + its own `aria-label`). The "you are here" halo is a shape/shadow difference, never a color-only signal.

---

## File Structure

- **Create:** `frontend/components/JourneyProgress.tsx` — the new component: `JourneyProgressProps`, `lastReachedIndex`, `nodeState`, `delayState`/`DELAY_COLOR`, `circleStyle`, `progressCopy`, the auto-scroll `useEffect`, and the `JourneyProgress`/`JourneyProgressNode` render functions. One file, mirroring `JourneyTimeline.tsx`'s own scope (a single component file owning its own rendering plus small colocated helpers — the spec's own "Explicitly out of scope" section rules out a shared `lib/` utility for now).
- **Create:** `frontend/components/JourneyProgress.test.tsx` — Vitest coverage for every node state, the full decision table, and the accessibility assertions.
- **Modify:** `frontend/components/TrainJourney.tsx` — render `<JourneyProgress .../>` directly above `<JourneyTimeline .../>`, gated on the identical `{state.journeyStops && ...}` guard.
- **Modify:** `frontend/components/TrainJourney.test.tsx` — extend coverage to assert `JourneyProgress` mounts (or doesn't) and shows the right caption in representative states.
- **Modify:** `frontend/app/globals.css` — add the `.journeyProgressLine`/`.journeyProgressLine::before` connecting-line rule.

## Task 1: `JourneyProgress` skeleton — one hollow node per stop, sized by kind, wired in

**Files:**
- Create: `frontend/components/JourneyProgress.tsx`
- Test: `frontend/components/JourneyProgress.test.tsx`
- Modify: `frontend/components/TrainJourney.tsx`
- Test: `frontend/components/TrainJourney.test.tsx`
- Modify: `frontend/app/globals.css`

**Interfaces:**
- Consumes: `JourneyStop`, `JourneyStopKind`, `ResolutionStatus`, `JourneyStatus` from `@/lib/types`.
- Produces: `JourneyProgressProps` and `export function JourneyProgress(props: JourneyProgressProps)` — the signature every later task in this plan extends internally without changing. Also `lastReachedIndex(stops: JourneyStop[]): number` (not exported — colocated, matching the spec's "8-line array walk" framing), reused by Tasks 2 and 4.

- [ ] **Step 1: Write the failing skeleton test**

Create `frontend/components/JourneyProgress.test.tsx`:

```tsx
import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { JourneyProgress } from './JourneyProgress';
import type { JourneyStop } from '@/lib/types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: 'RDG',
    name: 'Reading',
    tiploc: null,
    kind: 'Intermediate',
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    ...overrides,
  };
}

describe('JourneyProgress', () => {
  it('renders one node per stop, in order', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Origin' }),
          stop({ crs: 'CLJ', name: 'Clapham Junction', kind: 'Intermediate' }),
          stop({ crs: 'WOK', name: 'Woking', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(container.querySelectorAll('[data-journey-node]')).toHaveLength(3);
  });

  it('renders Origin and Terminate nodes at a larger diameter than an Intermediate node', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', kind: 'Origin' }),
          stop({ crs: 'CLJ', kind: 'Intermediate' }),
          stop({ crs: 'WOK', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect((nodes[0] as HTMLElement).style.width).toBe('18px');
    expect((nodes[1] as HTMLElement).style.width).toBe('12px');
    expect((nodes[2] as HTMLElement).style.width).toBe('18px');
  });

  it('renders no nodes and does not crash for an empty stops array', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(container.querySelectorAll('[data-journey-node]')).toHaveLength(0);
    expect(screen.getByRole('img')).toBeInTheDocument();
  });

  it('carries a role="img" and a stop-count aria-label before any marker logic exists', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'WAT', kind: 'Origin' }), stop({ crs: 'WOK', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByRole('img', { name: 'Journey progress: 2 stops' })).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `frontend/`): `npx vitest run components/JourneyProgress.test.tsx`
Expected: FAIL — `Cannot find module './JourneyProgress'`.

- [ ] **Step 3: Add the connecting-line CSS**

Add to `frontend/app/globals.css` (append after the `.issueRow*` block, matching that block's own comment-then-rule style):

```css
/* Schematic "you are here" journey progress diagram --
   `components/JourneyProgress.tsx`. The horizontal connecting line is a
   `::before` pseudo-element rather than an SVG path (spec:
   docs/superpowers/specs/2026-09-12-journey-progress-visualization-design.md
   -- "Existing visual/diagram conventions checked": no SVG/charting
   precedent exists anywhere else in this frontend, so none is introduced
   here either). `top: 9px` is half of the larger (18px) Origin/Terminate
   node diameter, so the line meets every node's vertical center. */
.journeyProgressLine {
  position: relative;
  display: flex;
  align-items: flex-start;
}

.journeyProgressLine::before {
  content: '';
  position: absolute;
  left: 0;
  right: 0;
  top: 9px;
  height: 2px;
  background: var(--mantine-color-gray-4);
  z-index: 0;
}
```

- [ ] **Step 4: Write the minimal component**

Create `frontend/components/JourneyProgress.tsx`:

```tsx
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

/** Schematic, index-spaced (NOT time/distance-proportional) "you are here"
 * line diagram, additive to `JourneyTimeline` -- see
 * docs/superpowers/specs/2026-09-12-journey-progress-visualization-design.md.
 * Rendered directly above `JourneyTimeline` in `TrainJourney.tsx`, behind
 * the identical `{state.journeyStops && ...}` guard. */
export function JourneyProgress({ stops }: JourneyProgressProps) {
  const ariaLabel = `Journey progress: ${stops.length} stop${stops.length === 1 ? '' : 's'}`;

  return (
    <Box role="img" aria-label={ariaLabel} style={{ overflowX: 'auto' }}>
      <Box className="journeyProgressLine" style={{ minWidth: stops.length * NODE_SLOT_WIDTH }}>
        {stops.map((stop, index) => {
          const diameter = nodeDiameter(stop.kind);
          return (
            <Box
              key={`${stop.crs ?? 'unknown'}-${index}`}
              style={{ flex: `0 0 ${NODE_SLOT_WIDTH}px`, display: 'flex', justifyContent: 'center' }}
            >
              <Box
                data-journey-node
                aria-hidden="true"
                style={{
                  width: diameter,
                  height: diameter,
                  borderRadius: '50%',
                  border: '2px solid var(--mantine-color-gray-5)',
                  backgroundColor: 'transparent',
                  zIndex: 1,
                }}
              />
            </Box>
          );
        })}
      </Box>
    </Box>
  );
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: PASS — all 4 tests.

- [ ] **Step 6: Wire `JourneyProgress` into `TrainJourney.tsx`**

In `frontend/components/TrainJourney.tsx`, add the import and render call:

```tsx
import { JourneyProgress } from './JourneyProgress';
```

```tsx
export function TrainJourney({ state }: { state: TrainJourneyState }) {
  return (
    <Stack gap="sm">
      <StatusMessage state={state} />
      {state.resolutionStatus === 'resolved' && <JourneyDetails state={state} />}
      {state.journeyStops && (
        <JourneyProgress
          stops={state.journeyStops}
          resolutionStatus={state.resolutionStatus}
          status={state.status}
          trainUid={state.trainUid}
          mayHaveArrived={state.mayHaveArrived}
        />
      )}
      {state.journeyStops && <JourneyTimeline stops={state.journeyStops} />}
    </Stack>
  );
}
```

- [ ] **Step 7: Write the failing `TrainJourney` mount/no-mount tests**

Add to `frontend/components/TrainJourney.test.tsx`, inside `describe('TrainJourney', ...)`:

```tsx
  it('renders JourneyProgress whenever journeyStops is present (schedule_matched)', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'schedule_matched',
          trainUid: 'X12345',
          journeyStops: [
            {
              crs: 'RDG',
              name: 'Reading',
              tiploc: null,
              kind: 'Origin',
              scheduledArrival: null,
              scheduledDeparture: '2026-09-08T08:00:00Z',
              actualArrival: null,
              actualDeparture: null,
              estimatedArrival: null,
              estimatedDeparture: null,
              lastEventType: null,
              variationStatus: null,
              delayMinutes: null,
            },
          ],
        })}
      />,
    );
    expect(screen.getByRole('img', { name: /Journey progress/ })).toBeInTheDocument();
  });

  it('renders no JourneyProgress for pending, even if journeyStops were somehow non-null', () => {
    renderWithMantine(
      <TrainJourney state={baseState({ resolutionStatus: 'pending', journeyStops: null })} />,
    );
    expect(screen.queryByRole('img', { name: /Journey progress/ })).not.toBeInTheDocument();
  });
```

- [ ] **Step 8: Run the test to verify it fails, then implement (already done in Step 6), then verify it passes**

Run: `npx vitest run components/TrainJourney.test.tsx`
Expected: since Step 6 already wired the component in, this should PASS immediately — if it fails, re-check Step 6's edit landed before running.

- [ ] **Step 9: Run the full frontend test suite**

Run (from `frontend/`): `npm test`
Expected: PASS, no regressions.

- [ ] **Step 10: Commit**

```bash
git add frontend/components/JourneyProgress.tsx frontend/components/JourneyProgress.test.tsx frontend/components/TrainJourney.tsx frontend/components/TrainJourney.test.tsx frontend/app/globals.css
git commit -m "feat: add JourneyProgress skeleton, wired above JourneyTimeline"
```

## Task 2: "You are here" marker + reached/not-reached node states + delay coloring

**Files:**
- Modify: `frontend/components/JourneyProgress.tsx`
- Test: `frontend/components/JourneyProgress.test.tsx`

**Interfaces:**
- Consumes: `lastReachedIndex`, `nodeDiameter`, `JourneyProgressProps` from Task 1.
- Produces: `type NodeState = 'reached' | 'marker' | 'not-reached'`, `nodeState(index, lastIndex): NodeState`, `type DelayState = 'on-time' | 'late' | 'early' | 'unknown'`, `delayState(delayMinutes): DelayState`, `DELAY_COLOR: Record<DelayState, string>`, `circleStyle(state, delay): React.CSSProperties` — all reused and extended by Task 6.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/JourneyProgress.test.tsx`, inside `describe('JourneyProgress', ...)`:

```tsx
  it('marks the highest-index stop with a confirmed actualArrival/actualDeparture as the marker', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({
            crs: 'CLJ',
            kind: 'Intermediate',
            actualArrival: '2026-09-12T08:20:00Z',
            actualDeparture: '2026-09-12T08:21:00Z',
          }),
          stop({ crs: 'WOK', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[0]).toHaveAttribute('data-node-state', 'reached');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'marker');
    expect(nodes[2]).toHaveAttribute('data-node-state', 'not-reached');
  });

  it('renders no marker at all when nothing has been confirmed yet (lastReachedIndex === -1)', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'WAT', kind: 'Origin' }), stop({ crs: 'WOK', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(Array.from(nodes).every((n) => n.getAttribute('data-node-state') === 'not-reached')).toBe(true);
  });

  it('colors a reached node green/orange/teal by delayMinutes, and gray when delayMinutes is unknown', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z', delayMinutes: 0 }),
          stop({ crs: 'B', kind: 'Intermediate', actualArrival: '2026-09-12T08:10:00Z', delayMinutes: 4 }),
          stop({ crs: 'C', kind: 'Intermediate', actualArrival: '2026-09-12T08:20:00Z', delayMinutes: -2 }),
          stop({ crs: 'D', kind: 'Terminate', actualArrival: '2026-09-12T08:30:00Z', delayMinutes: null }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[0]).toHaveAttribute('data-delay-state', 'on-time');
    expect(nodes[1]).toHaveAttribute('data-delay-state', 'late');
    expect(nodes[2]).toHaveAttribute('data-delay-state', 'early');
    // nodes[3] is also the marker (last confirmed index) -- delay unknown.
    expect(nodes[3]).toHaveAttribute('data-delay-state', 'unknown');
  });

  it('a PASS event (both actualArrival and actualDeparture set to the same instant) still counts that stop as reached', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin' }),
          stop({
            crs: 'B',
            kind: 'Intermediate',
            actualArrival: '2026-09-12T08:10:00Z',
            actualDeparture: '2026-09-12T08:10:00Z',
          }),
          stop({ crs: 'C', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'marker');
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: FAIL — every node currently renders `data-node-state`/`data-delay-state` as `undefined` (neither attribute exists yet).

- [ ] **Step 3: Implement marker/reached/delay logic**

In `frontend/components/JourneyProgress.tsx`, add below `nodeDiameter`:

```tsx
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
```

Replace the `JourneyProgress` function body's node-mapping to compute and pass state:

```tsx
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: PASS — all tests from Task 1 and Task 2.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/JourneyProgress.tsx frontend/components/JourneyProgress.test.tsx
git commit -m "feat: add you-are-here marker and delay-colored node states"
```

## Task 3: Tooltip-on-demand for bare nodes; always-visible origin/terminus labels

**Files:**
- Modify: `frontend/components/JourneyProgress.tsx`
- Test: `frontend/components/JourneyProgress.test.tsx`

**Interfaces:**
- Consumes: `nodeState`, `delayState`, `circleStyle`, `DELAY_COLOR`, `nodeDiameter` from Task 2.
- Produces: `function JourneyProgressNode(props): JSX.Element` — the extracted per-node renderer Task 6 extends further with the cancelled/may-have-arrived visuals.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/JourneyProgress.test.tsx`, inside `describe('JourneyProgress', ...)`:

```tsx
  it('always shows the origin and terminus station names as visible text, but not an intermediate node\'s', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Origin' }),
          stop({ crs: 'CLJ', name: 'Clapham Junction', kind: 'Intermediate' }),
          stop({ crs: 'WOK', name: 'Woking', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('London Waterloo')).toBeInTheDocument();
    expect(screen.getByText('Woking')).toBeInTheDocument();
    expect(screen.queryByText('Clapham Junction')).not.toBeInTheDocument();
  });

  it('reveals an intermediate node\'s name and scheduled time via Tooltip on hover', async () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Origin' }),
          stop({
            crs: 'CLJ',
            name: 'Clapham Junction',
            kind: 'Intermediate',
            scheduledArrival: '2026-09-12T08:15:00Z',
          }),
          stop({ crs: 'WOK', name: 'Woking', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const trigger = screen.getByLabelText(/Clapham Junction/);
    fireEvent.mouseEnter(trigger);
    // 2026-09-12 is within BST (UTC+1) -- 08:15Z renders as 09:15 London
    // time, same `formatTime`/Europe-London posture `JourneyTimeline` uses.
    expect(await screen.findByText('09:15')).toBeInTheDocument();
  });

  it('reveals a bare node\'s name even with no scheduled time known', async () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Origin' }),
          stop({ crs: 'CLJ', name: 'Clapham Junction', kind: 'Intermediate' }),
          stop({ crs: 'WOK', name: 'Woking', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    const trigger = screen.getByLabelText('Clapham Junction');
    fireEvent.mouseEnter(trigger);
    expect(await screen.findByText('Clapham Junction')).toBeInTheDocument();
  });
```

Add the `fireEvent` import to the top of the test file (combine with the existing `screen` import):

```tsx
import { fireEvent, screen } from '@testing-library/react';
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: FAIL — no station name is rendered anywhere yet (Task 1/2's node is a bare, unlabeled circle for every stop, including origin/terminus).

- [ ] **Step 3: Extract `JourneyProgressNode` with label/Tooltip rules**

Replace `frontend/components/JourneyProgress.tsx`'s imports and the `JourneyProgress`/node-rendering section:

```tsx
'use client';

import { Box, Stack, Text, Tooltip } from '@mantine/core';
import { formatTime } from '@/lib/dateFormat';
import type { JourneyStatus, JourneyStop, ResolutionStatus } from '@/lib/types';
```

Replace the `stops.map(...)` block inside `JourneyProgress` with a call to the new `JourneyProgressNode`:

```tsx
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

  if (isEndpoint) {
    return (
      <Stack gap={4} align="center" style={{ flex: `0 0 ${NODE_SLOT_WIDTH}px` }}>
        {circle}
        <Text size="xs" fw={700} ta="center">
          {label}
        </Text>
      </Stack>
    );
  }

  return (
    <Box style={{ flex: `0 0 ${NODE_SLOT_WIDTH}px`, display: 'flex', justifyContent: 'center' }}>
      <Tooltip
        label={
          <Stack gap={2}>
            <Text size="xs">{label}</Text>
            {scheduled && <Text size="xs">{formatTime(scheduled)}</Text>}
          </Stack>
        }
        events={{ hover: true, focus: true, touch: true }}
      >
        <Box tabIndex={0} aria-label={label}>
          {circle}
        </Box>
      </Tooltip>
    </Box>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: PASS — all tests from Tasks 1-3.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/JourneyProgress.tsx frontend/components/JourneyProgress.test.tsx
git commit -m "feat: add origin/terminus labels and per-node Tooltip"
```

## Task 4: Horizontal auto-scroll to the marker, with `prefers-reduced-motion`

**Files:**
- Modify: `frontend/components/JourneyProgress.tsx`
- Test: `frontend/components/JourneyProgress.test.tsx`

**Interfaces:**
- Consumes: `lastReachedIndex`, `JourneyProgressNode` from Tasks 1-3.
- Produces: the `useEffect`-driven auto-scroll behavior, keyed on `lastIndex`, that no later task needs to touch again.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/JourneyProgress.test.tsx`, a new `describe` block alongside the existing one:

```tsx
describe('JourneyProgress auto-scroll', () => {
  beforeEach(() => {
    window.HTMLElement.prototype.scrollIntoView = vi.fn();
  });

  it('scrolls the marker node into view, centered, on mount when a marker exists', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledWith(
      expect.objectContaining({ inline: 'center', behavior: 'smooth' }),
    );
  });

  it('does not call scrollIntoView when there is no marker (lastReachedIndex === -1)', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(window.HTMLElement.prototype.scrollIntoView).not.toHaveBeenCalled();
  });

  it('uses an instant jump, not smooth scrolling, when the viewer prefers reduced motion', () => {
    vi.spyOn(window, 'matchMedia').mockImplementation(
      (query: string) =>
        ({
          matches: query === '(prefers-reduced-motion: reduce)',
          media: query,
          onchange: null,
          addListener: vi.fn(),
          removeListener: vi.fn(),
          addEventListener: vi.fn(),
          removeEventListener: vi.fn(),
          dispatchEvent: vi.fn(),
        }) as unknown as MediaQueryList,
    );

    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledWith(
      expect.objectContaining({ behavior: 'auto' }),
    );
  });

  it('re-scrolls when lastReachedIndex advances on rerender (a new confirmed event moved the marker)', () => {
    const { rerender } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Intermediate' }),
          stop({ crs: 'C', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    (window.HTMLElement.prototype.scrollIntoView as ReturnType<typeof vi.fn>).mockClear();

    rerender(
      <MantineProvider theme={theme}>
        <JourneyProgress
          stops={[
            stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
            stop({ crs: 'B', kind: 'Intermediate', actualArrival: '2026-09-12T08:15:00Z' }),
            stop({ crs: 'C', kind: 'Terminate' }),
          ]}
          resolutionStatus="resolved"
          status="en_route"
          trainUid="C1"
          mayHaveArrived={false}
        />
      </MantineProvider>,
    );
    expect(window.HTMLElement.prototype.scrollIntoView).toHaveBeenCalledTimes(1);
  });
});
```

Add the extra imports needed at the top of the test file:

```tsx
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: FAIL — `scrollIntoView` is never called (no effect exists yet).

- [ ] **Step 3: Implement the auto-scroll effect**

In `frontend/components/JourneyProgress.tsx`, add `useEffect`/`useRef` to the imports:

```tsx
import { useEffect, useRef } from 'react';
```

Thread a ref array through `JourneyProgress` and into `JourneyProgressNode` (which attaches it to the node's decorative circle -- the same DOM element `scrollIntoView` is called on):

```tsx
export function JourneyProgress({ stops }: JourneyProgressProps) {
  const lastIndex = lastReachedIndex(stops);
  const ariaLabel = `Journey progress: ${stops.length} stop${stops.length === 1 ? '' : 's'}`;
  const nodeRefs = useRef<Array<HTMLDivElement | null>>([]);

  useEffect(() => {
    if (lastIndex === -1) return;
    const node = nodeRefs.current[lastIndex];
    if (!node) return;
    const reduceMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
    node.scrollIntoView({ inline: 'center', behavior: reduceMotion ? 'auto' : 'smooth' });
  }, [lastIndex]);

  return (
    <Box role="img" aria-label={ariaLabel} style={{ overflowX: 'auto' }}>
      <Box className="journeyProgressLine" style={{ minWidth: stops.length * NODE_SLOT_WIDTH }}>
        {stops.map((stop, index) => (
          <JourneyProgressNode
            key={`${stop.crs ?? 'unknown'}-${index}`}
            stop={stop}
            index={index}
            lastIndex={lastIndex}
            nodeRef={(el) => {
              nodeRefs.current[index] = el;
            }}
          />
        ))}
      </Box>
    </Box>
  );
}
```

Update `JourneyProgressNode`'s signature and its circle's `ref`:

```tsx
function JourneyProgressNode({
  stop,
  index,
  lastIndex,
  nodeRef,
}: {
  stop: JourneyStop;
  index: number;
  lastIndex: number;
  nodeRef: (el: HTMLDivElement | null) => void;
}) {
  const diameter = nodeDiameter(stop.kind);
  const state = nodeState(index, lastIndex);
  const delay = delayState(stop.delayMinutes);
  const isEndpoint = stop.kind === 'Origin' || stop.kind === 'Terminate';
  const label = stop.name ?? stop.crs ?? 'Unknown location';
  const scheduled = stop.scheduledArrival ?? stop.scheduledDeparture;

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

  // ...rest unchanged from Task 3 (isEndpoint branch, Tooltip branch).
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: PASS — all tests from Tasks 1-4.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/JourneyProgress.tsx frontend/components/JourneyProgress.test.tsx
git commit -m "feat: auto-scroll to the marker, respecting prefers-reduced-motion"
```

## Task 5: Full decision-table captions and `aria-label` text, exhaustively

**Files:**
- Modify: `frontend/components/JourneyProgress.tsx`
- Test: `frontend/components/JourneyProgress.test.tsx`
- Modify: `frontend/components/TrainJourney.test.tsx`

**Interfaces:**
- Consumes: `lastReachedIndex`, `JourneyProgressProps` from Task 1.
- Produces: `interface ProgressCopy { caption: string; ariaLabel: string }` and `progressCopy(stops, lastIndex, resolutionStatus, status, trainUid, mayHaveArrived): ProgressCopy` — the single source of truth for every row of the Global Constraints decision table, reused unchanged by Task 6.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/JourneyProgress.test.tsx`, a new `describe` block:

```tsx
describe('JourneyProgress decision-table captions and aria-labels', () => {
  it('schedule_matched: no marker, "scheduled route" caption and aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="schedule_matched"
        status={null}
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText("Scheduled route shown — live tracking hasn't started yet.")).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: scheduled route shown, live tracking not yet started' }),
    ).toBeInTheDocument();
  });

  it('resolved + awaiting_activation: no marker, "matched to train" caption naming trainUid', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="awaiting_activation"
        trainUid="C21373"
        mayHaveArrived={false}
      />,
    );
    expect(
      screen.getByText('Matched to train C21373 — waiting for its first movement report.'),
    ).toBeInTheDocument();
    expect(
      screen.getByRole('img', {
        name: 'Journey progress: matched to train C21373, waiting for first movement report',
      }),
    ).toBeInTheDocument();
  });

  it('resolved + en_route, mayHaveArrived false: "Currently at X" caption and matching aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Currently at Alpha.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: currently at Alpha, stop 1 of 2' }),
    ).toBeInTheDocument();
  });

  it('resolved + en_route, mayHaveArrived true: same caption, aria-label notes the inference', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate', actualArrival: '2026-09-12T09:00:00Z' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={true}
      />,
    );
    expect(screen.getByText('Currently at Bravo.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: currently at Bravo (may have arrived), stop 2 of 2' }),
    ).toBeInTheDocument();
  });

  it('resolved + cancelled, with a confirmed marker: "Cancelled — last confirmed at X" caption/aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Cancelled — last confirmed at Alpha.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: cancelled, last confirmed at Alpha, stop 1 of 2' }),
    ).toBeInTheDocument();
  });

  it('resolved + cancelled, before any confirmed movement: a distinct caption/aria-label', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Cancelled — no movement was ever confirmed.')).toBeInTheDocument();
    expect(
      screen.getByRole('img', { name: 'Journey progress: cancelled before any confirmed movement' }),
    ).toBeInTheDocument();
  });

  it('resolved + completed: "Arrived at X" caption/aria-label naming the terminus', () => {
    renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Terminate', actualArrival: '2026-09-12T09:00:00Z' }),
        ]}
        resolutionStatus="resolved"
        status="completed"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(screen.getByText('Arrived at Bravo.')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Journey progress: arrived at Bravo' })).toBeInTheDocument();
  });

  it('empty stops array: "Not yet started" caption/aria-label, defensively (not reachable via the real TrainJourney guard, but must not crash)', () => {
    renderWithMantine(
      <JourneyProgress stops={[]} resolutionStatus="resolved" status="en_route" trainUid="C1" mayHaveArrived={false} />,
    );
    expect(screen.getByText('Not yet started.')).toBeInTheDocument();
    expect(screen.getByRole('img', { name: 'Journey progress: not yet started' })).toBeInTheDocument();
  });
});
```

Also update Task 1's now-superseded aria-label test (it asserted the placeholder stop-count wording that this task replaces). In the existing `describe('JourneyProgress', ...)` block, replace:

```tsx
  it('carries a role="img" and a stop-count aria-label before any marker logic exists', () => {
```

with:

```tsx
  it('carries a role="img" and a "currently at" aria-label once a marker exists', () => {
```

and change its body's final assertion from `screen.getByRole('img', { name: 'Journey progress: 2 stops' })` to:

```tsx
    expect(
      screen.getByRole('img', { name: /Journey progress: matched to train/ }),
    ).toBeInTheDocument();
```

(this fixture has no `actualArrival`/`actualDeparture` anywhere, so `lastReachedIndex === -1`; add `resolutionStatus="resolved"` and `status="awaiting_activation"` — already present in that test's props — so the new wording is deterministic.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: FAIL — no caption `Text` renders at all yet, and the container's `aria-label` is still the Task-1 stop-count placeholder.

- [ ] **Step 3: Implement `progressCopy` and wire it in**

In `frontend/components/JourneyProgress.tsx`, add above the `export function JourneyProgress` line:

```tsx
interface ProgressCopy {
  caption: string;
  ariaLabel: string;
}

/** One pair of strings for every row of the status/resolution decision
 * table -- see this plan's Global Constraints for the table copied
 * verbatim from the spec. `caption` is the always-visible `Text` shown
 * under the diagram; `ariaLabel` is the `role="img"` container's textual
 * restatement (spec Decision 6). They are independent strings, not one
 * string reused twice, because the aria-label states the exact stop
 * position ("stop N of Total") a sighted caption doesn't need spelled
 * out. */
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
  const markerName =
    lastIndex >= 0 ? (stops[lastIndex].name ?? stops[lastIndex].crs ?? 'Unknown location') : null;
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
    const terminusName = stops[total - 1].name ?? stops[total - 1].crs ?? 'Unknown location';
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
```

Update `JourneyProgress` to compute and render it:

```tsx
export function JourneyProgress({ stops, resolutionStatus, status, trainUid, mayHaveArrived }: JourneyProgressProps) {
  const lastIndex = lastReachedIndex(stops);
  const nodeRefs = useRef<Array<HTMLDivElement | null>>([]);
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
      <Box role="img" aria-label={ariaLabel} style={{ overflowX: 'auto' }}>
        <Box className="journeyProgressLine" style={{ minWidth: stops.length * NODE_SLOT_WIDTH }}>
          {stops.map((stop, index) => (
            <JourneyProgressNode
              key={`${stop.crs ?? 'unknown'}-${index}`}
              stop={stop}
              index={index}
              lastIndex={lastIndex}
              nodeRef={(el) => {
                nodeRefs.current[index] = el;
              }}
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: PASS — all tests from Tasks 1-5.

- [ ] **Step 5: Add integration tests to `TrainJourney.test.tsx`**

Add to `frontend/components/TrainJourney.test.tsx`, inside `describe('TrainJourney', ...)`:

```tsx
  it('schedule_matched with journeyStops: JourneyProgress shows the matching "scheduled route" caption', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'schedule_matched',
          trainUid: 'X12345',
          journeyStops: [
            {
              crs: 'RDG',
              name: 'Reading',
              tiploc: null,
              kind: 'Origin',
              scheduledArrival: null,
              scheduledDeparture: '2026-09-08T08:00:00Z',
              actualArrival: null,
              actualDeparture: null,
              estimatedArrival: null,
              estimatedDeparture: null,
              lastEventType: null,
              variationStatus: null,
              delayMinutes: null,
            },
          ],
        })}
      />,
    );
    expect(screen.getByText("Scheduled route shown — live tracking hasn't started yet.")).toBeInTheDocument();
  });

  it('resolved + cancelled with journeyStops: JourneyProgress caption names the last confirmed stop', () => {
    renderWithMantine(
      <TrainJourney
        state={baseState({
          resolutionStatus: 'resolved',
          trainUid: 'C21373',
          status: 'cancelled',
          lastReportedLocation: 'Surbiton',
          journeyStops: [
            {
              crs: 'WAT',
              name: 'London Waterloo',
              tiploc: null,
              kind: 'Origin',
              scheduledArrival: null,
              scheduledDeparture: '2026-08-28T18:32:00Z',
              estimatedArrival: null,
              estimatedDeparture: null,
              actualArrival: null,
              actualDeparture: '2026-08-28T18:32:00Z',
              lastEventType: 'DEPARTURE',
              variationStatus: 'ON TIME',
              delayMinutes: 0,
            },
            {
              crs: 'SUR',
              name: 'Surbiton',
              tiploc: null,
              kind: 'Intermediate',
              scheduledArrival: '2026-08-28T18:50:00Z',
              scheduledDeparture: null,
              estimatedArrival: null,
              estimatedDeparture: null,
              actualArrival: '2026-08-28T18:52:00Z',
              actualDeparture: null,
              lastEventType: 'ARRIVAL',
              variationStatus: 'LATE',
              delayMinutes: 2,
            },
            {
              crs: 'WOK',
              name: 'Woking',
              tiploc: null,
              kind: 'Terminate',
              scheduledArrival: '2026-08-28T19:10:00Z',
              scheduledDeparture: null,
              estimatedArrival: null,
              estimatedDeparture: null,
              actualArrival: null,
              actualDeparture: null,
              lastEventType: null,
              variationStatus: null,
              delayMinutes: null,
            },
          ],
        })}
      />,
    );
    expect(screen.getByText('Cancelled — last confirmed at Surbiton.')).toBeInTheDocument();
  });
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `npx vitest run components/TrainJourney.test.tsx components/JourneyProgress.test.tsx`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/components/JourneyProgress.tsx frontend/components/JourneyProgress.test.tsx frontend/components/TrainJourney.test.tsx
git commit -m "feat: exhaustive decision-table captions and aria-labels"
```

## Task 6: Cancelled-node dashed styling, `mayHaveArrived` glyph, final accessibility check

**Files:**
- Modify: `frontend/components/JourneyProgress.tsx`
- Test: `frontend/components/JourneyProgress.test.tsx`

**Interfaces:**
- Consumes: `nodeState`, `circleStyle`, `JourneyProgressNode`, `progressCopy` from Tasks 2, 3, 5.
- Produces: final shape of `NodeState` (`'reached' | 'marker' | 'not-reached' | 'cancelled-remaining'`) and `nodeState`'s final 3-argument signature — nothing downstream in this plan extends it further.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/JourneyProgress.test.tsx`, inside `describe('JourneyProgress', ...)`:

```tsx
  it('cancelled: nodes after the frozen marker render in a distinct cancelled style, not the plain not-yet-reached style', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Intermediate' }),
          stop({ crs: 'C', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(nodes[0]).toHaveAttribute('data-node-state', 'marker');
    expect(nodes[1]).toHaveAttribute('data-node-state', 'cancelled-remaining');
    expect(nodes[2]).toHaveAttribute('data-node-state', 'cancelled-remaining');
  });

  it('cancelled before any confirmed movement: every node is cancelled-remaining, none is a marker', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[stop({ crs: 'A', kind: 'Origin' }), stop({ crs: 'B', kind: 'Terminate' })]}
        resolutionStatus="resolved"
        status="cancelled"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(Array.from(nodes).every((n) => n.getAttribute('data-node-state') === 'cancelled-remaining')).toBe(true);
  });

  it('completed: no node anywhere carries the cancelled-remaining style', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate', actualArrival: '2026-09-12T09:00:00Z' }),
        ]}
        resolutionStatus="resolved"
        status="completed"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const nodes = container.querySelectorAll('[data-journey-node]');
    expect(Array.from(nodes).some((n) => n.getAttribute('data-node-state') === 'cancelled-remaining')).toBe(false);
  });

  it('mayHaveArrived: the marker carries a distinguishing badge attribute, with its fill color unaffected', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z', delayMinutes: 0 }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={true}
      />,
    );
    expect(container.querySelector('[data-may-have-arrived="true"]')).toBeInTheDocument();
    const marker = container.querySelector('[data-node-state="marker"]');
    expect(marker).toHaveAttribute('data-delay-state', 'on-time');
  });

  it('mayHaveArrived false: no badge renders anywhere', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', kind: 'Origin', actualDeparture: '2026-09-12T08:00:00Z' }),
          stop({ crs: 'B', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    expect(container.querySelector('[data-may-have-arrived]')).not.toBeInTheDocument();
  });

  it('every decorative node circle is aria-hidden, but an intermediate node\'s focusable Tooltip trigger is not', () => {
    const { container } = renderWithMantine(
      <JourneyProgress
        stops={[
          stop({ crs: 'A', name: 'Alpha', kind: 'Origin' }),
          stop({ crs: 'B', name: 'Bravo', kind: 'Intermediate' }),
          stop({ crs: 'C', name: 'Charlie', kind: 'Terminate' }),
        ]}
        resolutionStatus="resolved"
        status="en_route"
        trainUid="C1"
        mayHaveArrived={false}
      />,
    );
    const circles = container.querySelectorAll('[data-journey-node]');
    circles.forEach((circle) => expect(circle).toHaveAttribute('aria-hidden', 'true'));
    const trigger = screen.getByLabelText('Bravo');
    expect(trigger).not.toHaveAttribute('aria-hidden');
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: FAIL — `nodeState` has no `'cancelled-remaining'` case yet (cancelled trains currently render `'not-reached'` past the marker), and no `data-may-have-arrived` attribute exists anywhere.

- [ ] **Step 3: Implement cancelled styling and the `mayHaveArrived` glyph**

In `frontend/components/JourneyProgress.tsx`, replace `NodeState`/`nodeState`/`circleStyle`:

```tsx
type NodeState = 'reached' | 'marker' | 'not-reached' | 'cancelled-remaining';

/** Extends Task 2's version with one more branch: an unreached stop on a
 * cancelled journey is `'cancelled-remaining'`, not the plain
 * `'not-reached'` -- so it reads visibly differently from "just hasn't got
 * there yet" (spec Decision 5). This falls out of composition rather than
 * needing a special case for `lastIndex === -1` + cancelled: every stop is
 * `index > lastIndex` when nothing has been confirmed, so every stop
 * already takes this branch. */
function nodeState(index: number, lastIndex: number, status: JourneyStatus | null): NodeState {
  if (lastIndex === -1 || index > lastIndex) {
    return status === 'cancelled' ? 'cancelled-remaining' : 'not-reached';
  }
  if (index === lastIndex) return 'marker';
  return 'reached';
}

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
```

Update every call site of `nodeState(index, lastIndex)` to `nodeState(index, lastIndex, status)` — both inside `JourneyProgressNode` and its call from `JourneyProgress`. Thread `status`/`mayHaveArrived` into `JourneyProgressNode`'s props:

```tsx
export function JourneyProgress({ stops, resolutionStatus, status, trainUid, mayHaveArrived }: JourneyProgressProps) {
  // ...unchanged from Task 5 above this line...
  return (
    <Stack gap="xs">
      <Box role="img" aria-label={ariaLabel} style={{ overflowX: 'auto' }}>
        <Box className="journeyProgressLine" style={{ minWidth: stops.length * NODE_SLOT_WIDTH }}>
          {stops.map((stop, index) => (
            <JourneyProgressNode
              key={`${stop.crs ?? 'unknown'}-${index}`}
              stop={stop}
              index={index}
              lastIndex={lastIndex}
              status={status}
              mayHaveArrived={mayHaveArrived}
              nodeRef={(el) => {
                nodeRefs.current[index] = el;
              }}
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
  const label = stop.name ?? stop.crs ?? 'Unknown location';
  const scheduled = stop.scheduledArrival ?? stop.scheduledDeparture;
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
        {circle}
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
        <Box tabIndex={0} aria-label={label}>
          {circle}
        </Box>
      </Tooltip>
      {glyph}
    </Box>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run components/JourneyProgress.test.tsx`
Expected: PASS — all tests from Tasks 1-6.

- [ ] **Step 5: Run the full frontend test suite**

Run (from `frontend/`): `npm test`
Expected: PASS, no regressions in `JourneyTimeline.test.tsx`, `TrainJourney.test.tsx`, or anywhere else — this plan never modified `JourneyTimeline.tsx`, `JourneyDetails`, `StatusMessage`, or any backend `resolutionStatus`/`status` computation.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/JourneyProgress.tsx frontend/components/JourneyProgress.test.tsx
git commit -m "feat: cancelled-node dashed style, mayHaveArrived glyph, accessibility check"
```

---

## Self-Review

**1. Spec coverage:**

| Spec section/decision | Task |
| --- | --- |
| Corrections §1-3 (no new backend, `JourneyTimeline` already a table, `TrainJourney`/`TrainJourneyPanel` already composed) | Global Constraints (backend-field confirmation); File Structure (no `crates/` files touched anywhere) |
| "Not TD/berth-level tracking, and not GPS" framing | `lastReachedIndex`'s doc comment (Task 1) — states the constraint explicitly, tied to the train-tracking design doc's rejection of TD |
| "What data already reaches the frontend" — exact `JourneyStop` shape, `PASS` sets both `actual*` fields, no `currentStopIndex` on the wire, `journeyStops` can be `null` | Global Constraints (field list, verbatim types); Task 2 (`lastReachedIndex`/PASS-event test); Task 1 (`{state.journeyStops && ...}` guard, unchanged) |
| Decision 1: new additive component, composed above `JourneyTimeline`, both pages via `TrainJourneyPanel` | Task 1 (component creation + wiring); no page-level changes anywhere in this plan (both `app/train/...` pages funnel through the unchanged `TrainJourneyPanel`) |
| Decision 2: `lastReachedIndex` derivation, colocated not `lib/`-level | Task 1 (defines it in `JourneyProgress.tsx`, not a new module) |
| Decision 3: node states table (reached/marker/not-reached/origin-terminus sizing), tooltip-on-demand, no permanent per-stop labels | Task 2 (reached/marker/not-reached + delay coloring); Task 1 (endpoint sizing); Task 3 (tooltip + origin/terminus labels) |
| Decision 4: horizontal scroll container, auto-scroll-to-marker, no compression | Task 1 (`overflow-x: auto` Box, index-proportional `NODE_SLOT_WIDTH`); Task 4 (auto-scroll effect) |
| Decision 5: full status/resolution decision table, all 8 rows | Global Constraints (table copied verbatim); Task 5 (`progressCopy`, one test per row); Task 6 (cancelled-remaining visual, mayHaveArrived glyph) |
| Decision 6: accessibility — `role="img"`/`aria-label`, `aria-hidden` nodes except focusable Tooltip triggers, color-never-only-signal, `prefers-reduced-motion` | Task 1 (`role="img"` base); Task 5 (full `aria-label` text); Task 3/6 (aria-hidden circles + focusable Tooltip trigger, tested explicitly in Task 6); Task 2 (halo is a shape/shadow difference, not a color-only signal); Task 4 (reduced-motion) |
| Decision 7: Client Component boundary, `"use client"` only on this file | Global Constraints + Task 1 (`"use client"` from the first line); Task 4 (the actual browser-API usage that requires it) |
| Data refresh: no new mechanism | Global Constraints/Non-goals — no polling/refresh code added anywhere in this plan |
| Non-goals: no interpolation, no map, no new backend, no per-stop ETA mechanism, no compression, no replacement of `JourneyTimeline` | Enforced structurally: `lastReachedIndex` never reads `estimated*` (Task 1's doc comment states this explicitly); no distance/geography data anywhere; File Structure never modifies `JourneyTimeline.tsx`/`JourneyDetails`/`StatusMessage`/any Rust file |
| Testing approach's own bullet list (marker correctness, no-marker state, cancelled, completed, mayHaveArrived, origin/terminus sizing, one aria-label test per decision-table row, tooltip reveal, scrollIntoView mocked + reduced-motion, empty array) | Every bullet maps 1:1 to a test in Tasks 1, 2, 3, 4, 5, or 6 above |

No gaps found.

**2. Placeholder scan:** No "TBD"/"implement later"/"add appropriate handling" language anywhere in the tasks above; every step carries literal code or a literal command to run. No step says "similar to Task N" without repeating the actual code — Tasks 3, 4, 5, and 6 each show the complete, current version of every function they modify (`JourneyProgress`, `JourneyProgressNode`, `nodeState`, `circleStyle`), not a diff description.

**3. Type consistency:** `JourneyProgressProps` is declared once in Task 1 and never renamed. `lastReachedIndex`, `nodeDiameter`, `nodeState`, `delayState`, `DELAY_COLOR`, `circleStyle`, `progressCopy`, `JourneyProgressNode` are named identically everywhere they're referenced across Tasks 1-6 — the only signature changes are additive-parameter extensions called out explicitly at the point they happen (`nodeState` gains a third `status` parameter in Task 6; every call site of it is updated in that same task's Step 3). `JourneyProgress`'s own public signature (`{ stops, resolutionStatus, status, trainUid, mayHaveArrived }`) is fixed from Task 1's `JourneyProgressProps` interface and never changes; `TrainJourney.tsx`'s Task 1 wiring already matches its final shape and is never revisited.

---

**Plan complete and saved to `docs/superpowers/plans/2026-09-12-journey-progress-visualization.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
