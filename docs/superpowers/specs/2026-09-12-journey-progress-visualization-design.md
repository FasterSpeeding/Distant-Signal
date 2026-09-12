# "You are here" journey progress visualization — design

**Status: design proposal, not approved for implementation.** Written to
the same rigor as
`docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md`
(the doc this proposal sits directly on top of — it adds a second,
schematic view over the exact same `JourneyStop[]` data that design
introduced, it does not change what data exists) and
`docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md`
(structure/tone precedent). No implementation plan is included; that is a
separate, later step in this repo's process.

## Required reading consumed in full before this document was written

`frontend/components/JourneyTimeline.tsx`,
`frontend/components/JourneyTimeline.test.tsx`,
`frontend/components/TrainJourney.tsx`,
`frontend/components/TrainJourney.test.tsx`,
`frontend/components/TrainJourneyPanel.tsx`, `frontend/lib/types.ts` (the
`JourneyStop`/`TrainJourneyState` section),
`crates/api/src/data/journey.rs` (whole file, including `db_tests`),
`docs/superpowers/specs/2026-08-28-train-tracking-design.md` (whole doc,
its Non-goals and "TD (Train Describer)" section in particular),
`docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md`
(whole doc), `docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md`
(whole doc — this is the design that made per-stop data exist at all),
`docs/superpowers/specs/2026-07-12-dark-theme-design.md`.

## Corrections to the brief's assumptions

The brief describes the status quo and the required backend work in terms
that don't match what's actually in the tree today. Recording these
up front, per this repo's own "corrections" precedent
(`2026-08-29-train-tracking-frontend-design.md`'s "Corrections to the
brief's assumptions" section):

1. **The current rendering is already a structured table, not "a text
   list."** `frontend/components/JourneyTimeline.tsx` renders a real
   Mantine `Table` — one row per calling point, four columns (station,
   scheduled, actual/est., delay badge) — inside a `TableScrollContainer`.
   It is not schematic/spatial (no notion of "where along the route,"
   just top-to-bottom rows) and that's the real gap this spec addresses,
   but it is materially richer than "a text list" already, and any new
   component sits alongside it, not as a rescue from a plain-text
   rendering.
2. **The per-stop data this spec needs already exists on the wire —
   no new backend work is required.** The brief frames this as something
   to "derive purely from already-collected `train_movement_events`," as
   if that derivation is still to be designed. It already happened:
   `crates/api/src/data/journey.rs`'s `build_journey_stops` +
   `apply_delay_estimates` (introduced by
   `2026-09-08-journey-timetable-overlay-design.md`, now shipped) already
   merges the booked timetable with the latest confirmed
   `train_movement_events` row per location into `JourneyStop[]`,
   serialized as `TrainJourneyState.journeyStops` and consumed today by
   `JourneyTimeline.tsx`. This spec is a **frontend-only, additive**
   design: a second component reading the exact same
   `state.journeyStops` array `JourneyTimeline` already reads, no new API
   route, no new field, no new backend derivation. (The "current stop index"
   this component needs is derived client-side from the existing array —
   see Decision 2 — not something the wire shape is missing.)
3. **`TrainJourney.tsx`/`TrainJourneyPanel.tsx` already compose several
   independent pieces** (a `StatusMessage` switch, a `JourneyDetails`
   fallback summary, `JourneyTimeline`, and — one level up, in
   `TrainJourneyPanel` — `RealTimeTrainsLink`). The brief describes this
   loosely as one or two files; in practice there are four/five
   composed pieces already, and the new component is a sixth, not a
   replacement for any existing one.

## This is not TD/berth-level tracking, and not GPS

Stated explicitly and early, because this codebase's own design doc for
individual train tracking
(`docs/superpowers/specs/2026-08-28-train-tracking-design.md`) already
investigated and **explicitly rejected** Train Describer (TD) / berth-level
physical position as a non-goal ("Non-goals (this pass)": *"Train Describer
(TD) / berth-level physical position. Investigated and deliberately not
recommended for v1"*), with a full section reasoning through why TRUST's
schedule-location events are the right granularity for "where is this train
relative to its stops" and TD's sub-station stepping data is a materially
harder system this app doesn't need. This spec does not reopen that
decision, and does not depend on anything TD would have provided.

The "you are here" marker this spec designs is placed at **the last
scheduled calling point with a confirmed reported event** — an `ARRIVAL`,
`DEPARTURE`, or `PASS` message TRUST has already sent, already stored in
`train_movement_events`, already merged into `JourneyStop.actualArrival`/
`actualDeparture` by code that shipped in the timetable-overlay design.
That is a real, already-happened, discrete event at a named station — not:

- **Not live signalling-derived position.** No TD/berth data is read,
  requested, or implied. The marker never claims to know where the train
  is *between* two calling points.
- **Not GPS or any other continuous positioning.** There is no notion of
  "73% of the way to the next stop" or a moving dot. The marker sits on a
  discrete station node, or it doesn't render at all.
- **Not interpolated or predicted.** The marker does not advance on a
  timer, does not creep forward as the scheduled/estimated time for the
  next stop approaches, and does not move until a new confirmed event
  actually arrives and the page re-fetches (see "Data refresh" below).
  Between two polls, the marker is static.

Every decision below that touches "where is the marker" is a restatement
of this constraint, not a new instance of it.

## Problem

`JourneyTimeline`'s table is precise but not *legible at a glance*: to
answer "roughly where is this train right now, how far along its route,"
a reader has to scan down a column of times looking for where the actual
column stops being populated. For a long journey (see "How long is a
journey" below) that's a lot of scrolling and scanning for a question that
a schematic line-diagram answers in one glance, the way a metro map's "you
are here" dot does. This spec adds a compact, schematic, non-map
progress diagram that answers exactly that one question, positioned above
the existing detailed table.

## What data already reaches the frontend (the honesty ceiling)

Everything this component can honestly show is already enumerated in
`JourneyStop` (`frontend/lib/types.ts`, mirroring
`crates/api/src/data/journey.rs`'s `JourneyStop`):

```ts
export interface JourneyStop {
  crs: string | null;
  name: string | null;
  tiploc: string | null;
  kind: JourneyStopKind | null;       // 'Origin' | 'Intermediate' | 'Terminate'
  scheduledArrival: string | null;    // RFC3339
  scheduledDeparture: string | null;  // RFC3339
  actualArrival: string | null;       // RFC3339 -- a REAL confirmed TRUST event
  actualDeparture: string | null;     // RFC3339 -- a REAL confirmed TRUST event
  estimatedArrival: string | null;    // RFC3339 -- current-delay PROPAGATION, only while actual* is null
  estimatedDeparture: string | null;
  lastEventType: string | null;       // "ARRIVAL" | "DEPARTURE" | "PASS"
  variationStatus: string | null;
  delayMinutes: number | null;        // only once an actual time exists for that stop
}
```

Three load-bearing facts about this shape, confirmed by reading
`journey.rs`, that the visual design must respect:

1. **`actualArrival`/`actualDeparture` are the only fields backed by a
   confirmed TRUST message.** `estimatedArrival`/`estimatedDeparture` are
   `apply_delay_estimates`'s forward propagation of the train's *current
   overall delay* onto every stop that hasn't been confirmed yet — real
   arithmetic on a real number, but not a report of anything Network Rail
   said happened at that specific stop. A `PASS` event sets **both**
   `actualArrival` and `actualDeparture` to the same instant (confirmed by
   `build_journey_stops_overlays_a_pass_event_setting_both_actual_times`).
2. **"Position" is not a field — it's derivable, deterministically, from
   the array.** There is no `currentStopIndex` on the wire. The last
   confirmed position is: the highest-index stop in `journeyStops` where
   `actualArrival !== null || actualDeparture !== null`. This mirrors
   exactly what `may_have_arrived` already does server-side (it looks at
   `stops.last()`'s `estimated_arrival`, i.e. the same array, the same
   ordering guarantee) — this spec does the equivalent walk for "last
   reached," client-side, over data already on the page.
3. **`journeyStops` can be `null`.** Per
   `2026-09-08-journey-timetable-overlay-design.md` §1, this happens for
   `pending`/`unresolved` tracked trains (no `train_uid` yet — genuinely
   nothing to draw, not a loading state) and, more rarely, for a
   `resolved` train that matched neither `trains.calling_points` nor any
   `schedule_destination_departures` row (a real train that isn't itself a
   CIF-published schedule that day). `JourneyTimeline` already guards on
   this (`{state.journeyStops && <JourneyTimeline .../>}`); the new
   component uses the identical guard — see Decision 1.

## How long is a journey (spot check)

`2026-09-08-journey-timetable-overlay-design.md`'s own Open Questions #3
already flags this from static reading: *"a handful of very long
InterCity/sleeper services can have 20-30+ stops."* `journey.rs`'s own
test fixtures mostly exercise 2-3 stop journeys (short, synthetic test
schedules — `RDG + SLO + synthetic WAT terminus` is a typical fixture
shape), which confirms the *mechanism* handles arbitrary lengths but
doesn't itself demonstrate a long real one. Treat "needs to handle 25+
stops without becoming useless" as a real design constraint, not a rare
edge case — this governs Decision 4 (horizontal, evenly-spaced, scrolling)
below.

## Existing visual/diagram conventions checked

Grepped `frontend/` for any existing hand-rolled diagram, SVG chart, or
schematic component: none exists. The only `<svg>`-touching files are
icon components (`InfoIcon.tsx`) and Next's generated `app/icon.svg` —
no precedent for a custom line/node diagram anywhere in this codebase.
Charting elsewhere (`@mantine/charts`/`recharts`, used for line-history
trend graphics per `2026-08-31-line-history-graphics-design.md`) is a
time-series axis chart, not a schematic map, and pulling in a charting
library for a dozen dots on a line would be pure overhead. **Decision:
no new dependency.** Build the diagram from plain Mantine primitives
(`Box`, `Group`, `Text`, `Tooltip`) and CSS (flex layout, a CSS
background-image or pseudo-element for the connecting line), matching
every other visual element in this app, rather than introducing SVG
authoring or a diagramming library where nothing already establishes that
pattern. This also keeps dark-theme correctness free: every color comes
from Mantine's existing `c="dimmed"` / `color="orange"` / CSS-variable
convention (`2026-07-12-dark-theme-design.md`'s stated rule — "zero
hardcoded colors"), the same convention `JourneyTimeline`'s `delayBadge`
already follows, so this component inherits dark-mode support for free
rather than needing its own pass.

## Design decisions

### 1. New component: `JourneyProgress.tsx`, additive to `TrainJourney.tsx`

A new `frontend/components/JourneyProgress.tsx`, taking the same prop
shape `JourneyTimeline` does (`{ stops: JourneyStop[] }`), rendered in
`TrainJourney.tsx` **directly above** `JourneyTimeline`, gated on the
identical `state.journeyStops` non-null guard:

```tsx
export function TrainJourney({ state }: { state: TrainJourneyState }) {
  return (
    <Stack gap="sm">
      <StatusMessage state={state} />
      {state.resolutionStatus === 'resolved' && <JourneyDetails state={state} />}
      {state.journeyStops && <JourneyProgress stops={state.journeyStops} />}
      {state.journeyStops && <JourneyTimeline stops={state.journeyStops} />}
    </Stack>
  );
}
```

This is a strict **addition**, not a replacement or a toggle: both render
whenever a stop list exists. `JourneyProgress` answers "roughly where is
it, at a glance"; `JourneyTimeline` answers "exactly what happened at each
stop, with times." They read the identical array — no prop drilling
divergence, no risk of the two disagreeing about what data is available,
since neither computes anything the other doesn't already have access to.
No existing component is removed, modified in its own rendering logic, or
hidden behind a feature flag; this mirrors how `JourneyTimeline` itself
was added *alongside* `JourneyDetails` rather than replacing it in the
2026-09-08 design.

Rendered on both pages that already share `TrainJourney` via
`TrainJourneyPanel.tsx` — `app/train/[uid]/[date]/page.tsx` (public,
`PublicTrainState`-adapted) and `app/train/by-id/[trackingId]/page.tsx`
(owned subscription, `TrackedTrainState`) — with **no page-level changes
required**, since both already funnel through the one shared
`TrainJourneyPanel` → `TrainJourney` call. No new route, no new page.

### 2. Deriving "current position" client-side, once, in one place

A small pure helper, colocated in `JourneyProgress.tsx` (not a new
`lib/` module — this is an 8-line array walk, not a shared utility any
other component needs yet):

```ts
function lastReachedIndex(stops: JourneyStop[]): number {
  for (let i = stops.length - 1; i >= 0; i--) {
    if (stops[i].actualArrival !== null || stops[i].actualDeparture !== null) {
      return i;
    }
  }
  return -1; // nothing confirmed yet -- see Decision 5's schedule_matched/awaiting_activation rows
}
```

This walks `actualArrival`/`actualDeparture` **only** — never
`estimatedArrival`/`estimatedDeparture` — matching the "confirmed events
only" framing above. `-1` is a legitimate, common result (every tracked
train starts here): no node satisfies the "you are here" condition in
Decision 3's table (it requires `lastReachedIndex !== -1`), so every node
simply renders as "not yet reached," and Decision 5 adds the caption copy
for that case on top — this falls out of composition, not a special case
bolted on afterward.

### 3. Visual language: a horizontal schematic line of nodes

One row: a thin horizontal line (a CSS `::before`/background strip, not an
SVG path) with one circular node per stop, evenly spaced along it — spacing
is **positional (index-based), not time-proportional**. Two adjacent stops
40 minutes apart and two adjacent stops 4 minutes apart get the same
on-screen gap. This is a deliberate schematic-map choice (the same one
the London Underground diagram makes vs. a geographically accurate map):
the question this component answers is "how far along the *sequence* is
the train," not "how far along the *clock*," and even spacing keeps a
disproportionately long inter-stop gap (a fast, sparse InterCity leg) from
squashing the rest of the diagram into unreadable clutter. `dataviz`-style
proportional-distance encoding was considered and rejected — this app has
no concept of physical distance between calling points anywhere in its
schema, so a distance-proportional diagram would need invented data, which
is exactly the "don't fabricate" posture `eta_blend`/`resolve.rs` already
established elsewhere in this codebase.

Per-node states, using only fields already established above:

| State | Condition | Rendering |
|---|---|---|
| **Reached** | index ≤ `lastReachedIndex` | Filled circle. Fill color from the stop's `delayMinutes` at that index, reusing `JourneyTimeline`'s existing three-way convention exactly (`green` on-time / `orange` late / `teal` early) — no new color language invented for this component. |
| **You are here** | index === `lastReachedIndex` (and `lastReachedIndex !== -1`) | The reached-circle styling above, **plus** a distinct outer ring/halo (a wider, higher-contrast border — CSS `outline` or a second concentric circle, not a different color, so it stays legible against any of the three delay colors) and a small text label under the diagram: `"Currently at {name}"`. This is the "you are here" marker; there is at most one per render. |
| **Not yet reached** | index > `lastReachedIndex` | Hollow circle (outline only, `c="dimmed"` border color, transparent fill) — no delay color, since there is nothing confirmed to color it by. This is the same "not yet reached" visual idea `JourneyTimeline`'s dimmed/muted text already uses for an unreached stop, translated into the node's fill state instead of text color. |
| **Origin / Terminate** | `kind === 'Origin'` or `kind === 'Terminate'` | Rendered at a visibly larger node radius than an `Intermediate` stop, same convention `JourneyTimeline` already applies via `fw={700}` for the two endpoints — carried into this component as a size difference instead of a font-weight difference, since there's no text weight on a circle. |

Station names are **not** printed under every node by default — with
20-30+ evenly-spaced nodes, a label under each one collides or gets
truncated into uselessness on any realistic viewport width. Instead:

- The **origin** and **terminus** names are always printed (start/end of
  the line, where collision risk is lowest and orientation matters most).
- The **"you are here"** node's name is always printed (its own label
  under the diagram, per the table above) — this is the one name a reader
  actually needs to answer "where is it."
- Every other node is a bare, unlabeled circle; a `Tooltip` (Mantine's
  existing tooltip primitive, already used this way elsewhere —
  `LineDefinitionTooltip.tsx`, the `schedule_matched` "As scheduled" badge
  in `TrainJourney.tsx` itself) reveals that node's name and scheduled
  time on hover/focus, keeping the information available without
  permanently occupying screen space. This matches `JourneyTimeline`'s own
  posture of using `Tooltip` for supplementary detail rather than always-
  visible text.

### 4. Long journeys: horizontal scroll, not compression, with auto-scroll to "you are here"

Reusing exactly the precedent `JourneyTimeline` already set for the same
problem: `TableScrollContainer` wraps the table so a long list scrolls
within its own box instead of forcing the whole page to scroll
horizontally. `JourneyProgress` wraps its node row in a plain `Box` with
`overflow-x: auto` (the same mechanism `TableScrollContainer` itself uses
under the hood) rather than inventing a second scrolling primitive.
Evenly-spaced nodes (Decision 3) means the row's total width grows
linearly with stop count — a 30-stop journey produces a wide strip, which
is fine precisely because it scrolls in its own container rather than
being squeezed to fit.

**Auto-scroll on mount/update:** the "at a glance" value of this component
degrades badly if a 30-stop journey's "you are here" node is off-screen to
the right and the reader has to know to scroll for it. On mount, and again
whenever `lastReachedIndex` changes between renders (i.e. a new confirmed
event moved the marker), the component calls the current-position node's
`scrollIntoView({ inline: 'center', behavior: 'smooth' })` inside a
`useEffect` keyed on `lastReachedIndex`. This requires `"use client"` —
see Decision 7. If `lastReachedIndex === -1` (journey not yet started, no
marker to center on), the scroll defaults to the origin end (`inline:
'start'`), which is already the natural resting scroll position of a
freshly-rendered `overflow-x: auto` container, so no explicit scroll call
is needed for that case.

No compression, collapsing, or "show only nearby stops" treatment is
proposed — explicitly a non-goal (see below). Scrolling is the same
answer this codebase already gave to the identical long-list problem one
component over; reusing it here is the smaller, more consistent design
than inventing a second strategy for what is structurally the same
problem.

### 5. Degraded / unresolved states: what the diagram does when there's nothing (yet) to point at

Every branch here composes with `StatusMessage`'s existing per-state copy
in `TrainJourney.tsx` (Decision 1 renders `JourneyProgress` unconditionally
alongside it, not as a replacement for any of that copy):

| Backend state | `journeyStops` | `lastReachedIndex` | `JourneyProgress` renders |
|---|---|---|---|
| `pending` / `unresolved` | `null` | n/a | **Nothing** — the `{state.journeyStops && ...}` guard means the component isn't mounted at all. `StatusMessage`'s existing "Waiting to hear from Network Rail" / "Couldn't be matched" text is the only thing shown, unchanged. This is the honest answer per the design doc precedent's §1 reasoning: there is no `train_uid`, so there is no schedule to draw a line for, and guessing one would risk showing a different train's route under a confident-looking diagram. |
| `schedule_matched` | populated (always, per `2026-09-08`'s §1) | `-1` (no movement data exists yet) | The full line of hollow "not yet reached" nodes, origin/terminus labeled, **no "you are here" marker at all** — not a marker parked at the origin, which would visually claim "confirmed to have started" when nothing has been confirmed. A small caption above or below the diagram: *"Scheduled route shown — live tracking hasn't started yet."* This mirrors `StatusMessage`'s own "As scheduled" badge copy for this exact state, applied to the diagram instead of just the text summary. |
| `resolved` + `awaiting_activation` | populated | `-1` | Same rendering as `schedule_matched` above (identical reasoning — a real `train_uid` exists but zero confirmed movement events do) — the caption instead reads *"Matched to train {trainUid} — waiting for its first movement report."*, matching `StatusMessage`'s own copy for this state. |
| `resolved` + `en_route`, `mayHaveArrived === false` | populated | ≥ 0 (typically) | The main case: full line, "you are here" marker at `lastReachedIndex`, everything past it hollow. |
| `resolved` + `en_route`, `mayHaveArrived === true` | populated | ≥ 0 | Same marker placement as above (it is **not** moved to the terminus — the marker only ever sits on a confirmed stop, and `mayHaveArrived` is explicitly an inference about time elapsed, not a new confirmed event). An additional small warning glyph/outline on the marker itself (not a color change — reusing `StatusMessage`'s existing `color="yellow"` "May have arrived" `Alert` wording, echoed here as a compact badge on the node) makes the *diagram* consistent with the *text* alert already shown above it, rather than the two disagreeing about how confident the page is. |
| `resolved` + `cancelled` | populated | ≥ -1 (whatever was last confirmed before cancellation, possibly `-1` if cancelled before any movement was ever reported) | Marker frozen at `lastReachedIndex` exactly as computed (matches `journey.rs`'s own `apply_cancellation`, which explicitly preserves last-known location rather than clearing it — confirmed by that function's own test, `cancellation_preserves_last_known_location`, cited in `2026-08-29-train-tracking-frontend-design.md`). Every node from `lastReachedIndex + 1` onward is rendered in a distinct **cancelled** hollow style (e.g. a dashed/greyed-out ring rather than the plain "not yet reached" ring), so a cancelled train's remaining stops read visibly differently from "just hasn't got there yet." A caption echoes `StatusMessage`'s existing red "Cancelled" `Alert`. |
| `resolved` + `completed` | populated | Normally the final index (the terminus has a confirmed `actualArrival`) | Marker at the terminus, rendered with the same "reached" styling as any other node — no separate "finished" visual identity is invented beyond what "reached, and it's the last node" already communicates unambiguously. Caption echoes `StatusMessage`'s existing green "Arrived" `Alert`. |
| Neither source has anything for this `train_uid`/date | `null` | n/a | Same as `pending`/`unresolved` — component doesn't mount. `JourneyDetails`'s existing denormalized-summary fallback remains the only movement rendering, unchanged, per `2026-09-08`'s own named gap. |

The **cardinal rule** underlying every row above, restated because it's
the one a future edit is most likely to accidentally violate: **the
marker only ever sits on an index where `lastReachedIndex` says a
confirmed event exists, or it doesn't render at all.** No state advances
it, ages it forward, or infers a "probably at the next stop by now"
position. That is precisely the TD/GPS-style interpolation this spec
explicitly is not doing (see the framing section above).

### 6. Accessibility: the diagram is a visual summary, not the only copy of the information

A row of circles connected by a line conveys nothing to a screen reader
without help, and this app already has accessibility precedent to follow
(`aria-label="Journey timeline"` on `JourneyTimeline`'s own `Table`,
`2026-09-02-frontend-accessibility-audit-research.md`'s existing pass over
this codebase). Concretely:

- The outer container carries `role="img"` and an `aria-label` that states
  the same fact the visual marker states: `aria-label="Journey progress:
  currently at {name}, {n} of {total} stops"` (or, for the `-1`/no-marker
  states, `"Journey progress: not yet started"` / `"...cancelled, last
  confirmed at {name}"` etc., one string per row of the Decision 5 table).
  This single string is a complete, honest textual restatement of what the
  diagram shows — a screen reader user gets the "at a glance" answer
  without needing to parse per-node `Tooltip`s.
- Individual nodes are `aria-hidden="true"` (decorative once the
  container-level label already states the answer) **except** the
  per-node `Tooltip` trigger targets remain independently
  focusable/readable for a sighted keyboard user who wants the
  intermediate-stop detail — matching `Tooltip`'s existing accessible
  pattern elsewhere in this codebase, not inventing a new one.
- Color is never the only signal: the "you are here" halo/ring is a shape
  difference, not just a color difference (colorblind-safe by
  construction, and consistent with `delayBadge`'s own text labels
  ("Xm late"/"On time") never relying on color alone either).
- `prefers-reduced-motion`: the `scrollIntoView({ behavior: 'smooth' })`
  call in Decision 4 respects it — check
  `window.matchMedia('(prefers-reduced-motion: reduce)').matches` and pass
  `behavior: 'auto'` (instant jump, no animation) when it's set, following
  the exact precedent `PrideToggle.tsx` already established in this
  codebase for the identical media query.

### 7. Client vs. server component boundary

`JourneyTimeline` and `TrainJourney` are pure Server Components today (no
`"use client"` anywhere in that chain — the doc comment on
`JourneyTimeline.tsx` calls this out explicitly, since it's the reason
that file avoids Mantine's compound `Table.Thead` API). `JourneyProgress`
needs `scrollIntoView` and a `prefers-reduced-motion` check, both
browser-only APIs, so it **must** be a Client Component
(`"use client"` at the top of `JourneyProgress.tsx`). This is a narrower
boundary than making the whole `TrainJourney` chain a Client Component:
only `JourneyProgress.tsx` itself carries the directive, matching how
`AutoRefresh.tsx` and `PinToggle.tsx` are each small, self-contained
Client Components mounted from otherwise-Server-Component pages — the
established pattern for "one interactive/browser-API-dependent leaf inside
an otherwise-static tree," not a new one invented for this feature. The
node styling (colors, shapes, layout) itself needs no client-side
JavaScript at all — only the auto-scroll behavior does — so the component
still server-renders its full visual content on first paint; the
`useEffect` only adjusts scroll position after hydration.

## Data refresh

No new refresh mechanism. `JourneyProgress` re-renders whenever its parent
`TrainJourney`/page does, which is driven by the existing global
`AutoRefresh` (`app/layout.tsx`, 30s interval, `cache: 'no-store'` Server
Component fetches) exactly as `JourneyTimeline` already relies on — see
`2026-08-29-train-tracking-frontend-design.md` Decision 5, unchanged by
this spec. A new confirmed event moving `lastReachedIndex` forward appears
on the next 30s refresh, the same latency every other live figure on this
page already has.

## Non-goals

- **No real-time interpolation or "moving" marker.** The marker sits on a
  discrete node or doesn't render; it never animates along the line
  between two confirmed stops, never advances on a timer, and never shows
  partial progress toward the next stop. This is the direct, load-bearing
  consequence of the framing section above.
- **No prediction of position between reported points.** The diagram never
  infers "probably somewhere between X and Y by now" from elapsed time —
  that is exactly the TD/berth-level or GPS-style capability this
  codebase's own design doc rejected, and this spec does not resurrect it
  in a different visual form.
- **No map.** No geography, no track layout, no distance-proportional
  spacing (Decision 3 is explicit that spacing is index-based, not
  distance- or time-based). This is a schematic line diagram in the London
  Underground sense, not a geographic one.
- **No new backend work.** No new route, no new field on `JourneyStop`/
  `TrainJourneyState`, no new derivation in `crates/api`. Every piece of
  data this component needs already ships today (see "Corrections" above).
- **No per-stop ETA beyond what already exists.** `estimatedArrival`/
  `estimatedDeparture` are used only to color a not-yet-reached node's
  `Tooltip` detail if desired at implementation time (optional, not
  load-bearing to the design) — this spec does not add a second ETA
  mechanism, matching `2026-09-08`'s own Decision 4 on the exact same
  point for `JourneyTimeline`.
- **No compression/collapsing of long journeys.** Decision 4's scrolling
  answer is the only long-journey treatment proposed; a collapsed/
  "expand to see all stops" interaction is explicitly left as a future
  fast-follow if scrolling proves insufficient in practice, not designed
  here (matching `2026-09-08`'s own Open Questions #3, which flagged this
  exact tension and left it unresolved for the same reason).
- **No replacement of `JourneyTimeline`.** Both render; neither is
  deprecated, hidden behind a toggle, or slated for removal.
- **No changes to `StatusMessage`'s copy/branching logic**, `JourneyDetails`,
  `EtaBadge`, or any backend `resolutionStatus`/`status` semantics. This
  spec is additive to `TrainJourney.tsx`'s render tree only.

## Testing approach

Following this repo's established Vitest convention (colocated
`*.test.tsx`, `renderWithMantine`, the same `stop()` fixture builder
pattern `JourneyTimeline.test.tsx` already uses — reused verbatim, not
reinvented):

- `components/JourneyProgress.test.tsx`:
  - `lastReachedIndex`-equivalent behavior: given a stops array with a mix
    of reached/unreached, the correct node carries the "you are here"
    marker (assert via the marker's distinguishing `aria`/test-id, not
    pixel position).
  - No stops reached (`lastReachedIndex === -1`): no "you are here" marker
    renders; the container's `aria-label` reflects "not yet started"
    phrasing, not a stale/default marker position.
  - Cancelled journey: marker frozen at the last confirmed stop, remaining
    nodes carry the distinct cancelled styling (assert a distinguishing
    class/attribute, not a specific color value, to avoid a brittle
    color-string assertion).
  - Completed journey: marker at the final node, reached-styling only (no
    cancelled/dimmed nodes anywhere).
  - `mayHaveArrived` true: marker still at `lastReachedIndex` (not the
    terminus), plus the inference badge/glyph present.
  - Origin/Terminate nodes render at the distinguishing larger size;
    intermediate nodes don't.
  - Container `role="img"` + `aria-label` text matches the expected string
    for each of the Decision 5 table's rows — one test per row, mirroring
    how `TrainJourney.test.tsx` already has one test per state-table row
    for `StatusMessage`.
  - Per-node `Tooltip` reveals the right station name and scheduled time
    on hover/focus for a non-endpoint, non-marker node.
  - `scrollIntoView` is called (mocked, since jsdom doesn't implement
    layout) with `inline: 'center'` when a marker exists, and is a no-op /
    `inline: 'start'` equivalent when it doesn't; a mocked
    `matchMedia('(prefers-reduced-motion: reduce)')` returning `matches:
    true` results in `behavior: 'auto'` being passed instead of
    `'smooth'`.
  - Not rendered at all when `stops` is an empty array is **not** a case
    this component needs to handle specially — `TrainJourney.tsx`'s
    existing `{state.journeyStops && ...}` guard only checks non-null, not
    non-empty, so add one explicit test confirming an empty array renders
    a sensible empty/"not yet started" state rather than crashing (the
    same `lastReachedIndex === -1` path other empty-history cases already
    exercise).
- `components/TrainJourney.test.tsx`: extend the existing per-state-table
  tests to also assert `JourneyProgress` mounts (or doesn't) in the right
  states, mirroring how that file already asserts `JourneyTimeline`'s
  presence per state.
- No backend/Rust tests are needed — this spec adds no Rust code.

## Explicitly out of scope

- A `lib/`-level shared "current position" utility — the derivation is
  small enough to stay local to `JourneyProgress.tsx` until a second
  consumer needs the same logic (matching this codebase's general
  no-premature-abstraction posture).
- Any change to how `mayHaveArrived`, `resolutionStatus`, or `status` are
  computed server-side.
- A user preference/toggle to hide the diagram. Nothing else on this page
  has a per-element visibility toggle (matching `AutoRefresh`'s own
  no-per-route-opt-out precedent, cited above); not introduced here either.
- Printing every stop's name permanently (Decision 3) — tooltip-on-demand
  only, for the reasons given there.
- Any distance/geography data model addition. If a future feature ever
  wants a geographically accurate map, that is an entirely different,
  much larger design (real track/geography data this app has never
  ingested) and not a natural evolution of this component.

## Open questions / risks

1. **Real-world node density at 25-30+ stops is unverified by static
   reading** — whether evenly-spaced nodes at that count remain legible
   even with horizontal scrolling, or whether a denser packing/virtualized
   rendering becomes necessary, can only be judged against a real long
   journey once this ships. `journey.rs`'s own test fixtures are all
   short, synthetic 2-3 stop schedules, so this can't be validated from
   existing fixtures alone — recommend spot-checking against a real
   InterCity/sleeper `train_uid` in a staging environment before
   considering this done.
2. **Tooltip-on-hover is a weak mobile affordance** — Mantine `Tooltip`
   generally needs a `Tooltip.Floating`/tap-to-open treatment or an
   explicit `withinPortal`/click trigger on touch devices to be reachable
   without a mouse. This spec assumes standard `Tooltip` behavior already
   used elsewhere in this app (`LineDefinitionTooltip.tsx`) is "good
   enough" by not diverging from precedent, but doesn't independently
   verify that precedent is itself mobile-friendly — flagged as inherited
   risk, not introduced by this design.
3. **Whether `estimatedArrival`/`estimatedDeparture` should surface at all
   in the per-node tooltip** (Decision 3's optional detail) was not fully
   resolved here — showing a propagated estimate next to a real scheduled
   time in the same tooltip risks visually conflating "confirmed" and
   "guessed" the exact way `EtaBadge`'s `etaSource` badge was built to
   avoid at the top level. If implemented, the tooltip should carry the
   same `trust-propagated`/estimate-vs-actual visual distinction
   `EtaBadge` already established, not a new unlabeled number — left as an
   implementation-time detail rather than a blocking design question.
4. **This spec does not investigate whether Mantine's `Tooltip` chain
   pulls in the same client-only import cost `JourneyTimeline.tsx`'s doc
   comment warns about for `Table.Thead`'s compound API** (the reason that
   component uses flat `TableThead` exports instead). Since
   `JourneyProgress.tsx` is already a Client Component (Decision 7), that
   specific constraint doesn't apply to it — `Tooltip`'s compound-API cost
   only matters inside a Server Component chain — but this is worth a
   sanity check at implementation time given how directly the sibling
   component was bitten by an analogous issue.

