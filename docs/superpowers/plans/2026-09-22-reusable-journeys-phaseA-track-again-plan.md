# Plan: Phase A — "Track this journey again"

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase A of
`docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md`
(§2.1, §6 item 1, §8) end to end: a **"Track this journey again"** button on
`/journeys/[id]` that pre-fills `/track`'s existing creation form from the
current journey's own already-fetched first leg, so re-tracking a route
travelled before doesn't mean retyping origin/destination/time-window
criteria from scratch. **No backend change of any kind** — every
byte touched by this plan lives under `frontend/`. Phases B (durable
templates) and C (recurrence), also confirmed in scope by the spec's §7
Open Question #1 resolution, are separate, independently-planned pieces of
work; nothing here builds toward their schema.

**Architecture:** four small, mostly-independent pieces, landed in an order
that keeps every intermediate commit buildable:

1. A new pure module, `frontend/lib/trackAgainPrefill.ts` (Task 1),
   extracts "what would a fresh `/track` visit need to reproduce this
   journey's first leg's criteria" from an already-fetched `JourneyDetail`
   — a `{mode, origin, destination, departAfter, departBefore, arriveAfter,
   arriveBefore}` record, plus a thin `trackAgainHref` wrapper that turns
   that into a `/track?...` query string. Framework-free and independently
   testable, deliberately **not** inlined into the button component — see
   this plan's own closing section on why Phase B likely wants to import
   the same extraction function.
2. `TrackTrainForm.tsx` (Task 2) currently has a real, confirmed gap: its
   only pre-fill props are `initialOrigin` and `initialMode`
   (`frontend/components/TrackTrainForm.tsx:286-298`) — there is **no**
   `initialDestination` or window-bounds pre-fill of any kind, despite the
   design spec's own task brief assuming otherwise. `JourneyLegCard.tsx`'s
   existing "Edit search" link says so explicitly in its own comment:
   *"Doesn't restore the destination/date/times too (no query-param
   contract for those yet)"* (`frontend/components/JourneyLegCard.tsx:190-191`).
   This task is what finally builds that contract: five new optional props
   (`initialDestination`, `initialDepartAfter`, `initialDepartBefore`,
   `initialArriveAfter`, `initialArriveBefore`), wired into the same
   `useState(initialX)` pattern `initialOrigin` already uses.
3. `/track`'s own page (`frontend/app/track/page.tsx`, Task 3) reads five
   matching new query params and threads them through to `TrackTrainForm`
   — the server-side half of the same contract, extending the existing
   `?origin=`/`?mode=`/`?ticketId=` convention rather than inventing a
   parallel one.
4. A new component, `TrackJourneyAgainButton.tsx` (Task 4), renders the
   button itself (`router.push` to `trackAgainHref(journey)`, or nothing at
   all if there's no origin to reproduce), and Task 5 wires it into
   `/journeys/[id]`'s existing header action row
   (`frontend/app/journeys/[id]/page.tsx:172-189`), alongside
   `AddJourneyLegButton`/`ShareJourneyButton`.

**Tech stack:** Next.js/React/Mantine (`frontend`), vitest + Testing
Library. No Rust, no migration, no route — this plan touches zero files
under `crates/`.

**Spec:** `docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md`
— authoritative for the overall three-phase shape (§8) and for Phase A's
own scope (§2.1: *"a pure frontend affordance… no new API surface, no new
table"*). This plan resolves the concrete judgment calls the spec leaves to
"a planning pass" for Phase A specifically (which query params, which leg,
how a multi-mode leg's criteria map onto `TrackTrainForm`'s two submission
modes) — verified against the CURRENT worktree, not the spec's own
citations, which predate the journey-tracking UX-review fix cycle (see
Judgment Call 1 below for the concrete place the spec's assumption turned
out to be stale).

---

## Judgment calls this plan makes (read before Task 1)

1. **The spec's task brief assumed `initialX`-style pre-fill props on
   `TrackTrainForm` had already been "extended during the journey-tracking
   + UX-review work." Verified false — extending them is this plan's own
   Task 2, not prior art.** Read `frontend/components/TrackTrainForm.tsx`
   in full: its props are exactly `initialOrigin`, `attachTicketId`,
   `initialMode` (lines 286-298) — no destination, no window-bounds
   pre-fill. The strongest confirmation is `JourneyLegCard.tsx`'s own
   "Edit search" link (line 190-194), added during that very UX-review
   cycle, which explicitly says it *can't* restore destination/date/times
   because *"no query-param contract for those yet"*. This plan is what
   builds that contract — Tasks 2 and 3 are new ground, not a rewiring of
   something that already existed.

2. **Multi-leg journeys: pre-fill ONLY the first leg, not the whole
   shape.** Verified `TrackTrainForm`'s `submitTrack`/`submitWindow`
   (`TrackTrainForm.tsx:547-697`) each `POST /Journeys` with exactly one
   leg — this component, and the `/track` page that hosts it, have no
   mechanism to create a multi-leg journey in one submission. Multi-leg
   chaining exists only via `AddJourneyLegButton`'s separate modal, called
   AFTER a journey already exists (`frontend/components/AddJourneyLegButton.tsx`).
   There is therefore no honest way for a single click on
   `/journeys/[id]` to reproduce a three-leg trip's full shape through
   `/track` alone — doing so would need either a new multi-leg-aware
   creation endpoint (a backend change, out of Phase A's explicit "no
   backend change" scope) or a sequence of `/track` + N ×
   `AddJourneyLegButton` calls the user would have to drive by hand anyway
   (no faster than just re-entering each leg). Pre-filling leg 1 and
   leaving the rest for the user to re-add via the existing "Add a leg"
   flow is the only shape that's both honest about what `/track` can do
   and actually saves the user typing for the leg that matters most (the
   one they're about to search from). Phase B's durable template (a real
   `journey_templates`/`journey_template_legs` row pair) is the right place
   for true whole-shape duplication — see this plan's closing section.

3. **Which of `JourneyLegDetail`'s two shapes (a searched window, or a
   direct pin/knownTrain pick) maps to which of `TrackTrainForm`'s two
   modes: derived from the same `hasWindow` boolean `JourneyLegCard.tsx`
   already computes (`JourneyLegCard.tsx:125-129`), not from `matchMode`.**
   Verified in `crates/api/src/data/journeys.rs`: a `pin`/`knownTrain`-mode
   leg always has `depart_after`/`depart_before`/`arrive_after`/
   `arrive_before` `NULL` (`create_journey_with_pin_leg`'s and
   `create_journey_with_known_train_leg`'s own doc comments, lines 226 and
   274-275); a `window`-mode leg always has at least one of those four set
   (`validate_window_leg`'s own requirement). `match_mode` alone can't
   distinguish them — a window-mode leg that's since been matched also
   reads `match_mode: 'manual'`, identically to a `pin`/`knownTrain` leg,
   while its window fields stay populated forever (they're never cleared on
   match, `journeys.rs:522`'s `UPDATE` only ever touches
   `train_subscription_id`/`match_mode`). So: any of the four window fields
   non-null → route to `mode=window` with destination + the populated
   subset of the four bounds pre-filled; all four null → route to
   `mode=pick` with destination pre-filled (if known) and the departure
   time left at `TrackTrainForm`'s own default ("now" — see Judgment Call
   4).

4. **Service date / departure time is deliberately NEVER carried over from
   the old leg — both modes keep their own existing "default to
   today/now" behaviour untouched.** The old leg's `serviceDate` describes
   a journey already travelled (that's the whole reason the button exists);
   reusing it verbatim would silently create a new tracking pin dated in
   the past. `windowServiceDate` already defaults to `dayjs().format(...)`
   (`TrackTrainForm.tsx:387`) and `scheduledDeparture` already defaults to
   "now" (`TrackTrainForm.tsx:309-311`) — both correct defaults for "again"
   with zero new code. Only the ROUTE/WINDOW-SHAPE criteria (origin,
   destination, the four time-of-day bounds) are pre-filled; the calendar
   date is always left to whatever the form already does when nothing is
   passed.

5. **Operator is not pre-filled — there is nothing to read it from.**
   Checked `JourneyLegDetail`/`TrackedTrainState`/`TrainJourneyState`
   (`frontend/lib/types.ts:597-828`) field-by-field: none of the three
   carries an `operator` string on the wire today (the underlying
   `train_subscriptions.pin_operator` column is written at creation time
   but never read back into any journey-detail response). Not a regression
   this plan introduces — there's genuinely nothing client-side to carry
   forward, and re-picking a train via the pick-mode departures picker
   shows the operator per-row anyway.

6. **Live, single-service-day signals — `skippedStations`, `platform`,
   `plannedPlatform` — are never reused.** These describe Darwin's
   departure-board snapshot for the ORIGINAL specific service
   (`TrackTrainForm.tsx:312-328`'s own doc comments); they have no meaning
   for a different day's service and aren't exposed on `JourneyLegDetail`
   in the first place, so there's nothing to carry even if it were wanted.

7. **The button is shown to every viewer of the page, not gated on
   `journey.isOwner` — unlike `AddJourneyLegButton`/`ShareJourneyButton`,
   which both mutate the journey being viewed and are correctly
   owner-gated (a non-owner's attempt 404s server-side, per
   `app/journeys/[id]/page.tsx:178-183`'s own comment).** "Track this
   journey again" does neither: it only reads client-side data the page
   already fetched (available to owner and shared-group viewer alike, per
   `journey_readable_by`) and, on click, creates an entirely INDEPENDENT
   new journey for whoever is currently logged in, via the same
   ownership-blind `POST /Journeys` every other `/track` visit already
   uses. A group member who's been shown a shared journey and wants to
   travel the same route themselves is a real, unobjectionable use of this
   button — there's no backend permission it could bypass, since the write
   it eventually makes is a plain, already-public creation call scoped to
   whoever is logged in when they submit the resulting form. `TrackTrainForm`'s
   own existing `useNeedsLogin`/`LoginPromptModal` machinery already covers
   "not logged in at all" for that eventual submit, unchanged by this plan.

8. **No gating on the first leg's own match status.** The button stays
   available whether the journey's first leg is matched, still an open
   search, or (per the spec's own confirmed gap) has failed to find
   anything — the button's whole value is re-submitting the same criteria,
   which is arguably MORE useful when the original search hasn't found a
   match yet. No `canAddLeg`-style conditional (contrast
   `app/journeys/[id]/page.tsx:150`, which is about a different, structural
   concern — whether there's a "next stop" to chain a NEW leg onto, not
   whether this journey is worth repeating).

9. **Button placement: last in the header action row, after
   `ShareJourneyButton`, not gated alongside the two owner-only
   controls.** `AddJourneyLegButton`/`ShareJourneyButton` are both
   conditionally rendered on `journey.isOwner` (`page.tsx:184-187`) and are
   both about managing THIS journey; `TrackJourneyAgainButton` is
   unconditional and about starting something new. Keeping the two
   owner-gated controls adjacent and putting the universal one last avoids
   interleaving a control that's always present between two that sometimes
   aren't.

---

## Non-goals

- **No backend change whatsoever** — no migration, no new route, no
  change to `crates/api`, `crates/common`, or any other crate. Verified
  nothing this plan needs is missing server-side: `GET /Journeys/{id}`
  already returns every field Task 1 reads (`frontend/lib/types.ts:804-828`).
- **No multi-leg shape reproduction.** Only the journey's first leg's
  criteria are pre-filled — see Judgment Call 2. A user wanting the rest of
  a multi-leg trip back still uses "Add a leg" by hand, same as today.
- **No operator pre-fill** (Judgment Call 5), **no live-signal reuse**
  (Judgment Call 6), **no service-date/departure-time carry-over**
  (Judgment Call 4).
- **No durable template entity.** No `journey_templates` table, no
  "save this as reusable" button, no `/journeys/templates` list — that's
  Phase B, a separate plan, separately scoped.
- **No recurrence, no day-of-week picker, no auto-commit matching.**
  That's Phase C, a separate plan.
- **No new affordance anywhere else in the app.** Not added to
  `/track/mine`'s list rows, `/trains`, or the single-train detail page —
  scoped strictly to `/journeys/[id]`, per spec §6 item 1 / §8 Phase A.
- **No change to `AddJourneyLegButton`/`ShareJourneyButton`'s own
  behaviour or gating.**
- **No change to the `/track?ticketId=`/`?mode=` contract's existing
  semantics** — this plan only ADDS new, independent query params
  alongside the two that already exist.

## Global Constraints

- **File scope.** New:
  `frontend/lib/trackAgainPrefill.ts`,
  `frontend/lib/trackAgainPrefill.test.ts`,
  `frontend/components/TrackJourneyAgainButton.tsx`,
  `frontend/components/TrackJourneyAgainButton.test.tsx`.
  Modified:
  `frontend/components/TrackTrainForm.tsx`,
  `frontend/components/TrackTrainForm.test.tsx`,
  `frontend/app/track/page.tsx`,
  `frontend/app/track/page.test.tsx`,
  `frontend/app/journeys/[id]/page.tsx`,
  `frontend/app/journeys/[id]/page.test.tsx`.
  No other file changes — in particular, **nothing under `crates/`**.
- **Testing.** This repo's actual frontend CI job
  (`.github/workflows/ci.yml`, the job with `working-directory: frontend`,
  lines 280-307) runs, in order: `npm ci`, `npm run lint` (eslint),
  `npx tsc --noEmit`, `npm test` (vitest), `npm run build` (next build).
  Run `npm test -- <file>` for each changed/new test file as you land it
  (Tasks 1-5's own Verify steps), then run the full sequence —
  `npm run lint && npx tsc --noEmit && npm test && npm run build` — once
  at the end (Task 6) as the final gate, matching CI exactly. No Rust
  commands apply to this plan at all (Global Constraints' file scope has
  zero `crates/` entries).
  **UI verification** (this repo's standing practice for a change with no
  Playwright e2e coverage added — `frontend/e2e/` is untouched by this
  plan): start the dev stack, open a real matched (pin-mode) journey's
  detail page, click "Track this journey again", confirm `/track` opens in
  pick mode with Origin and Destination pre-filled and Scheduled departure
  defaulted to now; then open a real journey whose leg was created via a
  time-window search, click the button, confirm `/track` opens in window
  mode with Origin, Destination and whichever of the four time bounds were
  actually set pre-filled, and Date defaulted to today. Folded into Task
  6's own Verify step, not a separate task.

---

## Task 1: `frontend/lib/trackAgainPrefill.ts` — pure criteria extraction

**Files:** create `frontend/lib/trackAgainPrefill.ts`,
`frontend/lib/trackAgainPrefill.test.ts`.

Independent — no dependency on Tasks 2-5, and nothing later depends on this
file compiling except Task 4.

- [ ] **Step 1: Write the module.**

```ts
import type { JourneyDetail, JourneyLegDetail } from './types';

/** What a fresh `/track` visit would need to reproduce one leg's own
 * search criteria -- the shared shape `trackAgainHref` (below) turns into
 * a query string, and the shape Phase B's "promote this journey to a
 * template" feature (docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
 * §6 item 2) is expected to reuse directly when it builds a
 * `journey_template_legs` row instead of a URL -- see this plan's own
 * closing note. Deliberately has no `serviceDate`/departure-time field at
 * all: "again" always means a NEW occurrence, so the date/time is always
 * left to whatever `/track`'s own form already defaults to (today/now),
 * never carried over from the journey being repeated. */
export interface TrackAgainPrefill {
  mode: 'pick' | 'window';
  origin: string | null;
  destination: string | null;
  /** "HH:MM", only ever populated when `mode === 'window'`. */
  departAfter: string | null;
  departBefore: string | null;
  arriveAfter: string | null;
  arriveBefore: string | null;
}

/** `"HH:MM:SS" | null` -> `"HH:MM" | null` -- `journey_legs.depart_after`
 * etc. are wall-clock bounds, no timezone conversion needed, just trimming
 * the seconds a passenger never entered. Same trick
 * `JourneyLegCard.tsx`'s own (private) `formatWindowTime` already applies
 * to the same four fields for display -- not imported from there since
 * it's a one-line slice and importing a display helper from a card
 * component into a data-extraction module would be the wrong direction of
 * coupling. */
function toHHMM(value: string | null): string | null {
  return value ? value.slice(0, 5) : null;
}

/** Extracts "again" criteria from a journey's FIRST leg only -- see this
 * plan's Judgment Call 2 for why only the first leg, not the whole
 * multi-leg shape. Returns `null` only for the defensive case of a
 * journey with zero legs (shouldn't happen in practice -- removing a
 * journey's only leg removes the journey itself, per
 * `RemoveJourneyLegButton.tsx` -- but `JourneyDetailPage`'s own
 * `defaultJourneyTitle` guards the identical case the same way, so this
 * matches established house style rather than assuming the invariant
 * holds).
 *
 * `mode` is derived from whether ANY of the leg's four window bounds is
 * set, not from `matchMode` -- a matched window-mode leg keeps its window
 * fields populated forever (they're never cleared on match), so this is
 * the only reliable signal for "was this originally a time-window search,
 * or a direct pin/known-train pick" -- see Judgment Call 3. `origin`
 * prefers the leg's own `originCrs`, falling back to the matched train's
 * own `pinOriginCrs` for the rare case a `knownTrain`-mode leg's own
 * `origin_crs` was `null` at creation time (the bound `trains` row had no
 * schedule data yet) -- same fallback shape `lib/journeyLegLabel.ts`'s
 * `legEndpointName` already uses for the analogous name-resolution
 * problem. `destination` follows the identical pattern. */
export function trackAgainPrefill(journey: JourneyDetail): TrackAgainPrefill | null {
  const firstLeg: JourneyLegDetail | undefined = journey.legs[0];
  if (!firstLeg) return null;

  const hasWindow =
    firstLeg.departAfter !== null ||
    firstLeg.departBefore !== null ||
    firstLeg.arriveAfter !== null ||
    firstLeg.arriveBefore !== null;

  const origin = firstLeg.originCrs ?? firstLeg.trackedTrainState?.pinOriginCrs ?? null;
  const destination =
    firstLeg.destinationCrs ?? firstLeg.trackedTrainState?.pinDestinationCrs ?? null;

  return {
    mode: hasWindow ? 'window' : 'pick',
    origin,
    destination,
    departAfter: hasWindow ? toHHMM(firstLeg.departAfter) : null,
    departBefore: hasWindow ? toHHMM(firstLeg.departBefore) : null,
    arriveAfter: hasWindow ? toHHMM(firstLeg.arriveAfter) : null,
    arriveBefore: hasWindow ? toHHMM(firstLeg.arriveBefore) : null,
  };
}

/** `trackAgainPrefill`, encoded as a `/track` query string --
 * `TrackJourneyAgainButton.tsx`'s only dependency on this module. Returns
 * `null` whenever there's no origin to reproduce at all (the rare gap
 * noted on `trackAgainPrefill`'s own `origin` field) -- never a link to a
 * form that would open with nothing usefully filled in, same "never a
 * dead-end control" posture `ShareJourneyButton.tsx` takes for zero
 * groups. `destination` and the four window bounds are each added only
 * when actually known -- an absent query param and an empty string mean
 * the same thing to `TrackPage`'s own `Array.isArray(x) ? x[0] : x`
 * unwrapping, but omitting genuinely-unknown fields keeps the resulting
 * URL honest and short rather than papering it with empty `=`s. */
export function trackAgainHref(journey: JourneyDetail): string | null {
  const prefill = trackAgainPrefill(journey);
  if (!prefill || !prefill.origin) return null;

  const params = new URLSearchParams();
  params.set('mode', prefill.mode);
  params.set('origin', prefill.origin);
  if (prefill.destination) params.set('destination', prefill.destination);
  if (prefill.mode === 'window') {
    if (prefill.departAfter) params.set('departAfter', prefill.departAfter);
    if (prefill.departBefore) params.set('departBefore', prefill.departBefore);
    if (prefill.arriveAfter) params.set('arriveAfter', prefill.arriveAfter);
    if (prefill.arriveBefore) params.set('arriveBefore', prefill.arriveBefore);
  }
  return `/track?${params.toString()}`;
}
```

- [ ] **Step 2: Write the test file.**

```ts
import { describe, it, expect } from 'vitest';
import { trackAgainPrefill, trackAgainHref } from './trackAgainPrefill';
import type { JourneyDetail, JourneyLegDetail, TrackedTrainState } from './types';

function trackedState(overrides: Partial<TrackedTrainState> = {}): TrackedTrainState {
  return {
    id: 1,
    serviceDate: '2026-09-22',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinOriginName: null,
    pinDestinationName: null,
    resolutionStatus: 'resolved',
    trainUid: 'P9E010',
    trainId: null,
    status: 'en_route',
    lastReportedLocation: null,
    lastEventType: null,
    delayMinutes: 0,
    nextCallingPoint: null,
    etaNext: null,
    etaSource: null,
    scheduleDestinationCrs: 'EDB',
    scheduleDestinationName: null,
    scheduleCallingPoints: null,
    journeyStops: null,
    mayHaveArrived: false,
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

function leg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'YRK',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    trackedTrainState: trackedState(),
    legSkip: null,
    ...overrides,
  };
}

function journey(legs: JourneyLegDetail[]): JourneyDetail {
  return { id: 1, customName: null, createdAt: '2026-09-22T00:00:00Z', legs, isOwner: true };
}

describe('trackAgainPrefill', () => {
  it('reads pick mode for a leg with no window bounds', () => {
    const result = trackAgainPrefill(journey([leg()]));
    expect(result).toEqual({
      mode: 'pick',
      origin: 'KGX',
      destination: 'YRK',
      departAfter: null,
      departBefore: null,
      arriveAfter: null,
      arriveBefore: null,
    });
  });

  it('reads window mode when any window bound is set, even on an unmatched leg', () => {
    const result = trackAgainPrefill(
      journey([
        leg({
          departAfter: '08:00:00',
          departBefore: '10:00:00',
          arriveAfter: null,
          arriveBefore: null,
          matchMode: 'unmatched',
          trackedTrainState: null,
        }),
      ]),
    );
    expect(result).toEqual({
      mode: 'window',
      origin: 'KGX',
      destination: 'YRK',
      departAfter: '08:00',
      departBefore: '10:00',
      arriveAfter: null,
      arriveBefore: null,
    });
  });

  it('reads window mode for a MATCHED window leg too (match_mode does not decide this)', () => {
    // A window-mode leg keeps its window fields set forever, even after
    // being matched -- matchMode alone can't distinguish this from a
    // pin/knownTrain leg. See Judgment Call 3.
    const result = trackAgainPrefill(
      journey([leg({ arriveAfter: '17:00:00', arriveBefore: '19:00:00' })]),
    );
    expect(result?.mode).toBe('window');
    expect(result?.arriveAfter).toBe('17:00');
    expect(result?.arriveBefore).toBe('19:00');
  });

  it('falls back to the matched train pin CRS when the leg row itself has no origin/destination', () => {
    const result = trackAgainPrefill(
      journey([
        leg({
          originCrs: null,
          destinationCrs: null,
          trackedTrainState: trackedState({ pinOriginCrs: 'PAD', pinDestinationCrs: 'RDG' }),
        }),
      ]),
    );
    expect(result?.origin).toBe('PAD');
    expect(result?.destination).toBe('RDG');
  });

  it('uses only the FIRST leg of a multi-leg journey', () => {
    const result = trackAgainPrefill(
      journey([leg({ originCrs: 'KGX', destinationCrs: 'YRK' }), leg({ id: 2, originCrs: 'YRK', destinationCrs: 'EDB' })]),
    );
    expect(result?.origin).toBe('KGX');
    expect(result?.destination).toBe('YRK');
  });

  it('returns null for a journey with no legs at all', () => {
    expect(trackAgainPrefill(journey([]))).toBeNull();
  });
});

describe('trackAgainHref', () => {
  it('builds a pick-mode URL with origin and destination only', () => {
    expect(trackAgainHref(journey([leg()]))).toBe('/track?mode=pick&origin=KGX&destination=YRK');
  });

  it('builds a window-mode URL with only the bounds that were actually set', () => {
    const href = trackAgainHref(
      journey([leg({ departAfter: '08:00:00', arriveBefore: null, matchMode: 'unmatched', trackedTrainState: null })]),
    );
    expect(href).toBe('/track?mode=window&origin=KGX&destination=YRK&departAfter=08%3A00');
  });

  it('returns null when there is no origin to reproduce', () => {
    const href = trackAgainHref(
      journey([leg({ originCrs: null, destinationCrs: null, trackedTrainState: null, matchMode: 'unmatched' })]),
    );
    expect(href).toBeNull();
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- trackAgainPrefill
```

  Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/trackAgainPrefill.ts frontend/lib/trackAgainPrefill.test.ts
git commit -m "frontend: add trackAgainPrefill, pure journey-leg criteria extraction for /track pre-fill"
```

---

## Task 2: `TrackTrainForm.tsx` — five new pre-fill props

**Files:** modify `frontend/components/TrackTrainForm.tsx`,
`frontend/components/TrackTrainForm.test.tsx`.

Independent of Task 1. Nothing later in this plan depends on this task's
tests, but Task 3 depends on these props existing.

- [ ] **Step 1: Extend the component's props** (`TrackTrainForm.tsx:286-298`):

```tsx
export function TrackTrainForm({
  initialOrigin = '',
  initialDestination = '',
  attachTicketId,
  initialMode = 'pick',
  initialDepartAfter = '',
  initialDepartBefore = '',
  initialArriveAfter = '',
  initialArriveBefore = '',
}: {
  initialOrigin?: string;
  // "Track this journey again" (docs/superpowers/plans/2026-09-22-reusable-journeys-phaseA-track-again-plan.md)
  // is the first caller of these five props -- pre-fills BOTH the
  // pin-mode Destination field and the window-mode Destination field from
  // the same value (only one is ever visible at a time, driven by
  // `initialMode`), and the window-mode time bounds. All five are inert,
  // ordinary `useState` initial values, same as `initialOrigin` already
  // is -- no new prop changes this component's submit behaviour.
  initialDestination?: string;
  attachTicketId?: number;
  initialMode?: 'pick' | 'window';
  /** "HH:MM" -- same value contract `TimeFilterInput`'s own `onChange`
   * already uses for `departFrom`/`departTo`/`arriveFrom`/`arriveTo`. */
  initialDepartAfter?: string;
  initialDepartBefore?: string;
  initialArriveAfter?: string;
  initialArriveBefore?: string;
}) {
```

- [ ] **Step 2: Seed `destinationCrs`** (pin mode) from `initialDestination`
  (`TrackTrainForm.tsx:301`):

```tsx
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
```

- [ ] **Step 3: Seed `windowDestinationCrs`** (window mode) from the same
  prop (`TrackTrainForm.tsx:366`):

```tsx
  const [windowDestinationCrs, setWindowDestinationCrs] = useState(initialDestination);
```

- [ ] **Step 4: Seed the four window-bound fields**
  (`TrackTrainForm.tsx:388-391`):

```tsx
  const [departFrom, setDepartFrom] = useState(initialDepartAfter);
  const [departTo, setDepartTo] = useState(initialDepartBefore);
  const [arriveFrom, setArriveFrom] = useState(initialArriveAfter);
  const [arriveTo, setArriveTo] = useState(initialArriveBefore);
```

  Not touched: `windowServiceDate` (`TrackTrainForm.tsx:387`) and
  `scheduledDeparture` (`TrackTrainForm.tsx:309-311`) keep their existing
  today/now defaults unconditionally — see Judgment Call 4. No prop is
  added for either.

- [ ] **Step 5: Add tests**, alongside the existing `initialOrigin`
  coverage (`TrackTrainForm.test.tsx`, near line 169's
  `'pre-fills the origin field from initialOrigin'`):

```tsx
  it('pre-fills the pin-mode destination field from initialDestination', () => {
    renderWithMantine(<TrackTrainForm initialOrigin="WAT" initialDestination="RDG" />);
    expect(screen.getByLabelText('Destination station (optional)')).toHaveValue('RDG');
  });

  it('pre-fills the window-mode destination and time-bound fields together', () => {
    renderWithMantine(
      <TrackTrainForm
        initialMode="window"
        initialOrigin="WAT"
        initialDestination="RDG"
        initialDepartAfter="08:00"
        initialArriveBefore="10:00"
      />,
    );
    expect(screen.getByLabelText('Destination station')).toHaveValue('RDG');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('08:00');
    expect(screen.getByLabelText('Latest arrival (optional)')).toHaveValue('10:00');
    // Bounds that weren't passed stay empty, not "undefined" leaking through.
    expect(screen.getByLabelText('Latest departure (optional)')).toHaveValue('');
    expect(screen.getByLabelText('Earliest arrival (optional)')).toHaveValue('');
  });

  it('leaves scheduled departure and window date at their own today/now defaults regardless of the new props', () => {
    renderWithMantine(
      <TrackTrainForm initialOrigin="WAT" initialDestination="RDG" initialDepartAfter="08:00" />,
    );
    // Judgment Call 4: no initialServiceDate/initialScheduledDeparture prop
    // exists at all -- "again" never carries the old date/time forward.
    const picker = screen.getByLabelText(/Scheduled departure/) as HTMLInputElement;
    expect(picker.value).not.toBe('');
  });
```

- [ ] **Step 6: Verify**

```bash
cd frontend && npm test -- TrackTrainForm
```

  Expected: existing tests still pass (every new prop defaults to `''`,
  identical to the pre-existing unconditional empty-string state) and the
  three new tests pass.

- [ ] **Step 7: Commit**

```bash
git add frontend/components/TrackTrainForm.tsx frontend/components/TrackTrainForm.test.tsx
git commit -m "frontend: add initialDestination/initialDepartAfter/../initialArriveBefore pre-fill props to TrackTrainForm"
```

---

## Task 3: `/track` page — the query-param half of the contract

**Files:** modify `frontend/app/track/page.tsx`,
`frontend/app/track/page.test.tsx`.

Depends on Task 2's new prop names.

- [ ] **Step 1: Extend the `searchParams` type and destructuring**
  (`app/track/page.tsx:47-72`):

```tsx
export default async function TrackPage({
  searchParams,
}: {
  searchParams: Promise<{
    origin?: string | string[];
    ticketId?: string | string[];
    mode?: string | string[];
    destination?: string | string[];
    departAfter?: string | string[];
    departBefore?: string | string[];
    arriveAfter?: string | string[];
    arriveBefore?: string | string[];
  }>;
}) {
  const { origin, ticketId, mode, destination, departAfter, departBefore, arriveAfter, arriveBefore } =
    await searchParams;
  // Same "repeated query param -> first value" unwrapping `origin`/
  // `ticketId`/`mode` already use just below -- applied uniformly to the
  // five new params `TrackJourneyAgainButton`/`trackAgainHref`
  // (docs/superpowers/plans/2026-09-22-reusable-journeys-phaseA-track-again-plan.md)
  // introduce, so a malformed/repeated value degrades the same way a
  // malformed `?origin=` already does rather than throwing.
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const departAfterParam = Array.isArray(departAfter) ? departAfter[0] : departAfter;
  const departBeforeParam = Array.isArray(departBefore) ? departBefore[0] : departBefore;
  const arriveAfterParam = Array.isArray(arriveAfter) ? arriveAfter[0] : arriveAfter;
  const arriveBeforeParam = Array.isArray(arriveBefore) ? arriveBefore[0] : arriveBefore;
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;
  const modeParam = Array.isArray(mode) ? mode[0] : mode;
  const initialMode = modeParam === 'window' ? 'window' : 'pick';
```

  (This keeps every existing line's own ordering and comments intact —
  only the destructuring, the six `const originParam`-style lines, and the
  type widen; `attachTicketId`/`modeParam`/`initialMode` are unchanged.)

- [ ] **Step 2: Pass the new params through to `TrackTrainForm`**
  (`app/track/page.tsx:102-106`):

```tsx
      <TrackTrainForm
        initialOrigin={originParam?.toUpperCase()}
        initialDestination={destinationParam?.toUpperCase()}
        attachTicketId={attachTicketId}
        initialMode={initialMode}
        initialDepartAfter={departAfterParam}
        initialDepartBefore={departBeforeParam}
        initialArriveAfter={arriveAfterParam}
        initialArriveBefore={arriveBeforeParam}
      />
```

  `.toUpperCase()` on `destinationParam` mirrors `originParam`'s own
  existing treatment immediately above it — both are CRS codes, same
  normalization. The four time params are passed through verbatim (already
  plain "HH:MM" strings on the wire from `trackAgainHref`, and
  `TrackTrainForm`'s own fields tolerate an empty/malformed value exactly
  as they already do for anything a user might type by hand).

- [ ] **Step 3: Add tests**, alongside the existing `?mode=window` coverage
  (`app/track/page.test.tsx`, after the `'starts the form in window mode
  for ?mode=window'` test):

```tsx
  it('pre-fills destination and window bounds for ?mode=window&destination=&departAfter=...', async () => {
    renderWithMantine(
      await TrackPage({
        searchParams: Promise.resolve({
          mode: 'window',
          origin: 'wat',
          destination: 'rdg',
          departAfter: '08:00',
          arriveBefore: '10:00',
        }),
      }),
    );

    expect(screen.getByLabelText('Destination station')).toHaveValue('RDG');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('08:00');
    expect(screen.getByLabelText('Latest arrival (optional)')).toHaveValue('10:00');
  });

  it('pre-fills the pin-mode destination for a plain ?destination= with no ?mode=', async () => {
    renderWithMantine(
      await TrackPage({ searchParams: Promise.resolve({ origin: 'wat', destination: 'rdg' }) }),
    );

    expect(screen.getByLabelText('Destination station (optional)')).toHaveValue('RDG');
  });
```

- [ ] **Step 4: Verify**

```bash
cd frontend && npm test -- app/track/page
```

  Expected: existing tests still pass, both new tests pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/track/page.tsx frontend/app/track/page.test.tsx
git commit -m "frontend: thread ?destination=/?departAfter=/.../?arriveBefore= through to TrackTrainForm"
```

---

## Task 4: `TrackJourneyAgainButton.tsx` — the button itself

**Files:** create `frontend/components/TrackJourneyAgainButton.tsx`,
`frontend/components/TrackJourneyAgainButton.test.tsx`.

Depends on Task 1 (`trackAgainHref`). Independent of Tasks 2-3 compiling
(this component only builds a URL string and navigates to it — it doesn't
care what `/track` does with the query params until Task 5 wires it onto a
real page next to a real `/track`), but obviously has no visible effect
until Task 3 has landed.

- [ ] **Step 1: Write the component.**

```tsx
'use client';

import { useRouter } from 'next/navigation';
import { Button } from '@mantine/core';
import { trackAgainHref } from '@/lib/trackAgainPrefill';
import type { JourneyDetail } from '@/lib/types';

/** "Track this journey again" (design doc §2.1/§6 item 1/§8 Phase A) --
 * pre-fills `/track` from this journey's own already-fetched FIRST leg
 * (see `lib/trackAgainPrefill.ts`'s own doc comment for why only the
 * first leg, and this plan's Judgment Call 2). Pure client-side
 * navigation, no fetch, no mutation of the journey being viewed at all --
 * unlike `AddJourneyLegButton`/`ShareJourneyButton`, both of which DO
 * mutate it and are correctly owner-gated by their caller
 * (`app/journeys/[id]/page.tsx`), this component takes no `isOwner` prop
 * and is meant to be shown to every viewer -- see this plan's Judgment
 * Call 7 for why that's the right call here specifically.
 *
 * Renders nothing at all when `trackAgainHref` returns `null` -- the rare
 * case of a journey whose first leg has no origin to reproduce at all
 * (`trackAgainPrefill.ts`'s own doc comment) -- same "never a dead-end
 * control" posture `ShareJourneyButton.tsx` already takes for zero
 * groups, rather than rendering a button that would open `/track` with
 * nothing usefully filled in. */
export function TrackJourneyAgainButton({ journey }: { journey: JourneyDetail }) {
  const router = useRouter();
  const href = trackAgainHref(journey);
  if (href === null) return null;

  return (
    <Button variant="default" size="xs" onClick={() => router.push(href)}>
      Track this journey again
    </Button>
  );
}
```

- [ ] **Step 2: Write the test file.**

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackJourneyAgainButton } from './TrackJourneyAgainButton';
import type { JourneyDetail, JourneyLegDetail } from '@/lib/types';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/journeys/167',
  useSearchParams: () => new URLSearchParams(''),
}));

function leg(overrides: Partial<JourneyLegDetail> = {}): JourneyLegDetail {
  return {
    id: 1,
    originCrs: 'KGX',
    originName: null,
    destinationCrs: 'YRK',
    destinationName: null,
    serviceDate: '2026-09-22',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    matchMode: 'manual',
    trackedTrainState: null,
    legSkip: null,
    ...overrides,
  };
}

function journey(legs: JourneyLegDetail[], isOwner = true): JourneyDetail {
  return { id: 167, customName: null, createdAt: '2026-09-22T00:00:00Z', legs, isOwner };
}

describe('TrackJourneyAgainButton', () => {
  beforeEach(() => pushMock.mockClear());

  it('navigates to a pick-mode /track URL for a leg with no window', () => {
    renderWithMantine(<TrackJourneyAgainButton journey={journey([leg()])} />);
    fireEvent.click(screen.getByRole('button', { name: 'Track this journey again' }));
    expect(pushMock).toHaveBeenCalledWith('/track?mode=pick&origin=KGX&destination=YRK');
  });

  it('navigates to a window-mode /track URL for a leg with window bounds', () => {
    renderWithMantine(
      <TrackJourneyAgainButton
        journey={journey([leg({ departAfter: '08:00:00', matchMode: 'unmatched' })])}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Track this journey again' }));
    expect(pushMock).toHaveBeenCalledWith('/track?mode=window&origin=KGX&destination=YRK&departAfter=08%3A00');
  });

  it('renders for a non-owner (shared-group viewer) too -- see Judgment Call 7', () => {
    renderWithMantine(<TrackJourneyAgainButton journey={journey([leg()], false)} />);
    expect(screen.getByRole('button', { name: 'Track this journey again' })).toBeInTheDocument();
  });

  it('renders nothing when there is no origin to reproduce', () => {
    renderWithMantine(
      <TrackJourneyAgainButton
        journey={journey([leg({ originCrs: null, destinationCrs: null })])}
      />,
    );
    expect(screen.queryByRole('button', { name: 'Track this journey again' })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npm test -- TrackJourneyAgainButton
```

  Expected: all tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/components/TrackJourneyAgainButton.tsx frontend/components/TrackJourneyAgainButton.test.tsx
git commit -m "frontend: add TrackJourneyAgainButton"
```

---

## Task 5: Wire the button onto `/journeys/[id]`

**Files:** modify `frontend/app/journeys/[id]/page.tsx`,
`frontend/app/journeys/[id]/page.test.tsx`.

Depends on Task 4.

- [ ] **Step 1: Import the component**, alongside this file's other
  component imports (`app/journeys/[id]/page.tsx:4-9`):

```tsx
import { AddJourneyLegButton } from '@/components/AddJourneyLegButton';
import { JourneyLegCard } from '@/components/JourneyLegCard';
import { JourneyStatusBadge } from '@/components/JourneyStatusBadge';
import { LastUpdated } from '@/components/LastUpdated';
import { LoginLink } from '@/components/LoginLink';
import { ShareJourneyButton } from '@/components/ShareJourneyButton';
import { TrackJourneyAgainButton } from '@/components/TrackJourneyAgainButton';
```

- [ ] **Step 2: Render it last in the header action group**
  (`app/journeys/[id]/page.tsx:184-188`):

```tsx
          {journey.isOwner && canAddLeg && (
            <AddJourneyLegButton journeyId={journey.id} priorDestinationCrs={priorDestinationCrs} />
          )}
          {journey.isOwner && <ShareJourneyButton journeyId={journey.id} />}
          {/* Deliberately NOT gated on journey.isOwner -- see
              docs/superpowers/plans/2026-09-22-reusable-journeys-phaseA-track-again-plan.md's
              Judgment Call 7. Placed last so the two owner-only controls
              above stay visually adjacent to each other. */}
          <TrackJourneyAgainButton journey={journey} />
```

- [ ] **Step 3: Add tests**, alongside the existing "Add a leg" gating
  suite (`app/journeys/[id]/page.test.tsx`, near its
  `describe('JourneyDetailPage "Add a leg" gating (M16)', ...)` block):

```tsx
describe('JourneyDetailPage "Track this journey again" button', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  it('renders for the owner', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney({ isOwner: true }));
    renderWithMantine(await renderPage());
    expect(screen.getByRole('button', { name: 'Track this journey again' })).toBeInTheDocument();
  });

  it('renders for a non-owner shared-group viewer too', async () => {
    vi.mocked(api.getJourney).mockResolvedValue(baseJourney({ isOwner: false }));
    renderWithMantine(await renderPage());
    expect(screen.getByRole('button', { name: 'Track this journey again' })).toBeInTheDocument();
  });
});
```

  (`baseJourney`/`renderPage` are this test file's own existing helpers,
  `app/journeys/[id]/page.test.tsx:114-127` — no new helper needed.)

- [ ] **Step 4: Verify**

```bash
cd frontend && npm test -- "app/journeys/\[id\]/page"
```

  Expected: every pre-existing test in this file still passes (confirmed
  no test asserts an exact button count in the header — only
  `getByRole('button', { name: 'Add a leg' })` by name, at
  `page.test.tsx:351` — so adding a third, always-rendered button doesn't
  collide with anything) and both new tests pass.

- [ ] **Step 5: Commit**

```bash
git add "frontend/app/journeys/[id]/page.tsx" "frontend/app/journeys/[id]/page.test.tsx"
git commit -m "frontend: add \"Track this journey again\" to the journey detail page header"
```

---

## Task 6: Full verification sweep

**Files:** none (verification only — no diff to commit).

- [ ] **Step 1: Full automated suite**, matching CI's frontend job exactly
  (`.github/workflows/ci.yml` lines 298-307):

```bash
cd frontend
npm run lint
npx tsc --noEmit
npm test
npm run build
```

  Expected: all four pass clean.

- [ ] **Step 2: Manual UI verification** (per this plan's Global
  Constraints — no Playwright coverage added for this feature):

  1. Start the dev stack (`npm run dev` in `frontend/`, with a real `api`
     backend running per this repo's own local-dev setup).
  2. Open a real journey whose first leg is a direct pin/known-train pick
     (matched, no window) at `/journeys/{id}`.
  3. Click **"Track this journey again"**. Confirm `/track` opens with
     **pick mode** selected, Origin and Destination pre-filled to that
     leg's route, and Scheduled departure defaulted to the current
     date/time (NOT the original journey's old date).
  4. Open a real journey whose first leg was created via a time-window
     search (matched or still unmatched — either exercises the same
     `hasWindow` path).
  5. Click **"Track this journey again"**. Confirm `/track` opens with
     **window mode** selected, Origin/Destination pre-filled, whichever
     of the four time bounds that leg actually had set are pre-filled
     (the others empty), and Date defaulted to today.
  6. Confirm the button is visible and functions identically when viewing
     a journey shared into a group you're a member of but don't own (a
     non-`isOwner` view) — see Judgment Call 7.

- [ ] **Step 3: No commit** — this task only verifies Tasks 1-5's already-
  committed diffs together; nothing new to stage.

---

## Handoff notes for Phase B (durable templates)

Phase B's "promote an existing journey to a template" entry point (spec
§6 item 2) is expected to need the exact same "what does this journey's
leg actually need to be reproduced" extraction this plan builds in Task 1
— worth flagging explicitly for whoever plans that phase:

- **`trackAgainPrefill(journey)`** (`frontend/lib/trackAgainPrefill.ts`)
  already returns a structured `{mode, origin, destination, departAfter,
  departBefore, arriveAfter, arriveBefore}` record per leg-criteria-set,
  independent of how it gets used — Task 1's own doc comment calls this
  out. Phase B's "promote" button can most likely call this SAME function
  (or a trivial per-leg generalization of it — this plan's version only
  ever reads `journey.legs[0]`, per Judgment Call 2) to build each
  `journey_template_legs` row's `origin_crs`/`destination_crs`/
  `depart_after`/.../`arrive_before` fields, rather than re-deriving the
  "which of the four window fields means this was a window-mode leg, not
  a pin" logic (Judgment Call 3) a second time.
- Unlike this phase, Phase B's promotion almost certainly DOES want the
  WHOLE multi-leg shape, not just leg 1 (Judgment Call 2 is specific to
  what `/track`'s single-leg creation flow can express — a
  `journey_templates` row has no such limit, since it defines its own
  `journey_template_legs` directly rather than routing through `/track`
  at all). Generalizing `trackAgainPrefill` to map over every leg rather
  than reading only `journey.legs[0]` should be a small, backward-
  compatible change if Phase B's planning pass wants to reuse it that way.
- The one thing Phase B should NOT reuse as-is: `trackAgainHref`'s
  URL-building (Task 1's second export) is specific to `/track`'s query
  param contract (Task 3) and has no analogue for a `POST
  /JourneyTemplates` request body — only `trackAgainPrefill` itself (the
  structured record, not the URL string) is the reusable part.
