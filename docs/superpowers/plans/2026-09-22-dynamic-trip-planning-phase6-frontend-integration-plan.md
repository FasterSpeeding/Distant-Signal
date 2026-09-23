# Plan: Dynamic Trip Planning — Phase 6: Frontend Integration

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 6 of the six-phase breakdown in
`docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md` §8: a
third mode on the existing `JourneyCreationFlow` — "Plan a route for me" —
that collects origin/destination/optional ordered waypoints/date/results
preference, calls Phase 5's `GET /Trips/plan`, lets the visitor compare and
pick an itinerary per segment, then commits every train leg of the chosen
itinerary through the **existing, unmodified** `POST /Journeys` +
`POST /Journeys/{id}/legs` calls — landing in the exact same per-leg summary
view `JourneyCreationFlow` already renders once a journey exists. No backend
change beyond one small frontend-proxy allowlist entry (Task 1).

**Architecture:** `/journeys/new`/`JourneyCreationFlow` already exists and is
this app's primary tracking entry point (design spec §0.8/§5.3) — this phase
adds a mode selector to it, not a new page. Five new frontend files:
`lib/tripPlan.ts` (query building + fetch, Task 2's sibling to `lib/types.ts`'s
new wire types), `components/PlanTripForm.tsx` (the input form, mirroring
`TrackTrainForm`'s own Autocomplete/`SegmentedControl` conventions, Task 3),
`components/ItineraryOption.tsx` (renders one itinerary's legs for
comparison/selection, Task 4), `components/PlanTripFlow.tsx` (orchestrates
form → results → per-segment pick → the multi-leg creation sequence, Task
5), and a small edit to `JourneyCreationFlow.tsx` itself (the mode selector,
Task 6).

**Tech stack:** Next.js/React/Mantine (`frontend`), vitest + Testing
Library.

**Spec:** `docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md`
§5.1, §5.3, §8 Phase 6. Depends on Phase 5
(`2026-09-22-dynamic-trip-planning-phase5-planning-api-plan.md`) for the
exact `GET /Trips/plan` response shape this phase's TypeScript types must
mirror field-for-field.

---

## Judgment calls this plan makes (read before Task 1)

1. **The frontend same-origin proxy (`frontend/app/api/[...path]/route.ts`)
   needs a new allowlist entry for `/Trips/...` — verified directly this
   pass, not assumed.** That file's own `ROOT_MOUNTED_PREFIXES` set
   (`route.ts:54`) and its `isAllowed` check (`route.ts:79-85`) currently
   only recognize `Train`, `Journeys`, `JourneyTemplates` — every other
   root-mounted route added since this allowlist was introduced has needed
   exactly this one-line addition (its own comment, `route.ts:26-53`, says
   so explicitly for each prior addition). Phase 5's `GET /Trips/plan` is
   mounted the same way (`main.rs`'s `.merge(routes::trips::router())`,
   not nested under `/public`), so without this change every
   `fetch('/api/Trips/plan?...')` the new frontend code makes would be
   rejected with `400 invalid path` before ever reaching `api`. This is a
   frontend-proxy change, not a backend change, which is why it lives in
   this phase rather than Phase 5.
2. **A picked itinerary's multi-leg creation is done entirely inside
   `PlanTripFlow.tsx`'s own submit handler, sequentially, calling
   `POST /Journeys` once then `POST /Journeys/{id}/legs` once per remaining
   train leg — never routed through `JourneyCreationFlow`'s own
   `handleLegOneCreated`/`handleLegAdded` state machine mid-sequence.**
   `JourneyCreationFlow`'s existing "Add a leg" flow is a deliberately
   incremental, one-user-click-per-leg design (`JourneyCreationFlow.tsx`'s
   own doc comment: "chain further legs on one `POST
   /Journeys/{id}/legs` call at a time"). A planner-sourced itinerary is
   different: the visitor has already reviewed and confirmed the WHOLE
   route before clicking "Track this journey," so re-surfacing an
   "Add a leg" button per hop would be a worse experience than what a human
   building the same multi-leg journey by hand goes through today —
   exactly the case the design spec's own §5.1 point 1 calls out ("no
   candidate-browsing step for the user at all... a strictly better
   creation path"). `PlanTripFlow` therefore creates every leg itself, in
   one sequence, and calls `JourneyCreationFlow`'s existing `onCreated`
   prop **exactly once**, after the sequence finishes (successfully or with
   a named partial failure) — `onCreated`'s own existing contract (a bare
   `journeyId`, triggering a refetch) needs no change at all to support
   this; `JourneyCreationFlow.tsx` itself is touched only to add the mode
   selector (Task 6), not to learn about multi-leg planning.
3. **A `TransferLeg` (a fixed-link walk/tube/bus hop) never becomes a
   `journey_legs` row — it is presentation-only, shown as an inline "walk
   from X to Y, N minutes" divider between the train legs either side of
   it.** The design spec's own §5.1 point 5 explicitly leaves "does a
   cross-station walk get its own minimal `journey_legs`-adjacent record,
   or stay presentation-only" as an unresolved design decision "for the
   implementation-planning stage" — this plan resolves it for v1 as
   presentation-only, because inventing a new leg concept/schema is real,
   separately-scoped backend work this phase's own Non-goals (and Phase
   5's) explicitly defer; nothing about "walk 5 minutes" is trackable
   (delay/cancellation-notifiable) in the same sense a real train leg is,
   so there is no correctness loss in not persisting it as its own row —
   only a display concern, handled entirely client-side by
   `ItineraryOption.tsx`.
4. **An itinerary with ZERO train legs at all (a pure fixed-link-only
   "journey" — e.g. "just walk from Euston to King's Cross") is shown but
   its "Track this journey" action is disabled, with an explanatory
   message, rather than attempting a nonsensical zero-leg `POST /Journeys`
   call.** `journey_legs`' own schema is fundamentally "board a specific
   train" (design spec §5.1 point 5's own framing) — there is no leg at all
   to create for a walk with no train on either side of it. This is a real,
   if rare, output shape CSA/RAPTOR can legitimately produce (Phase 3/4's
   own test coverage for exactly this case), so the UI must handle it
   honestly, not silently break.

---

## Non-goals

- **No backend change beyond the proxy allowlist entry** (Judgment Call 1).
- **No new `journey_legs`-adjacent record for a walking transfer** — see
  Judgment Call 3; this stays an open design question for a later pass, per
  the design spec's own §5.1 point 5 and §7 Open Question 5.
- **No unordered-waypoint / "find the best order" UI** — matches the
  design spec's own §4 scope; waypoints are entered and searched in the
  order the visitor types them.
- **No re-optimization across a picked itinerary's own multiple segments**
  (a segment's own itinerary choice does not affect what the next
  segment's search considers) — matches Phase 5's own `plan_via_waypoints`
  design.
- **No saving a planned itinerary as a reusable/recurring template** —
  named future work in the design spec's own §7 Open Question 4, explicitly
  out of scope here.

## Global Constraints

- **File scope.** Created:
  `frontend/lib/tripPlan.ts` (new),
  `frontend/lib/tripPlan.test.ts` (new),
  `frontend/components/PlanTripForm.tsx` (new),
  `frontend/components/PlanTripForm.test.tsx` (new),
  `frontend/components/ItineraryOption.tsx` (new),
  `frontend/components/ItineraryOption.test.tsx` (new),
  `frontend/components/PlanTripFlow.tsx` (new),
  `frontend/components/PlanTripFlow.test.tsx` (new).
  Modified: `frontend/app/api/[...path]/route.ts`,
  `frontend/lib/types.ts`,
  `frontend/components/JourneyCreationFlow.tsx`.
  No other file changes — `TrackTrainForm.tsx`, `AddJourneyLegButton.tsx`,
  and every backend file are untouched by this phase.
- **Testing.** `npm test -- <file>` (vitest) per changed test file, then a
  full `npm test`, `npm run lint`, `npx tsc --noEmit`, and `npm run build`
  before considering this phase done — matching `.github/workflows/ci.yml`'s
  `frontend` job's exact four steps. **UI verification**: per this repo's
  standing practice for a change with no automated end-to-end coverage of
  its own yet, start the dev stack and manually verify in a real browser —
  plan a real route with a real change of trains, confirm both `results`
  modes render, confirm a picked itinerary's legs actually appear as real
  tracked legs on `/journeys/{id}` afterward.
- **Wire-shape fidelity.** Every field name in the new `lib/types.ts`
  additions (Task 2) must match Phase 5's actual shipped
  `PlannedLeg`/`PlannedItinerary`/response JSON **exactly** — re-read that
  phase's own `crates/api/src/data/trip_planning_itinerary.rs` and
  `crates/api/src/routes/trips.rs` directly before writing Task 2's types,
  do not rely on this document's own paraphrase of them.

## Review Focus

- **A `results=options` response where a segment's `cappedByMaxChanges` is
  `true`** — the UI must say so ("a faster route exists with more changes
  than shown"), not silently show only the within-cap set with no
  explanation (Phase 5's own honest-disclosure design would be wasted if
  the frontend drops the flag).
- **A multi-waypoint plan where one segment has zero itineraries** (no
  route found for that hop) — the "Track this journey" action must stay
  disabled and say which segment has no option, not let the visitor
  proceed with an incomplete selection.
- **The zero-train-leg (fixed-link-only) itinerary case** — see Judgment
  Call 4; must not silently attempt `POST /Journeys` with no train
  identity.
- **A partial failure partway through the multi-leg creation sequence**
  (leg 1 succeeds, leg 2's `POST /Journeys/{id}/legs` fails) — the visitor
  must be told plainly which leg failed and see a way forward (the journey
  that DOES exist, with its one committed leg, per the design spec's own
  "must not strand the user" posture already established for
  `JourneyCreationFlow`'s own refetch-failure handling).
- **A `GET /Trips/plan` 404 (no CIF data published for the chosen date)
  or 400 (invalid CRS/results value)** — both must render as a clear,
  distinct message a visitor can act on (pick a different date; fix a
  typo'd station), not a generic "something went wrong."

---

## Task 1: Frontend proxy allowlist — permit `/Trips/...`

**Files:**
- Modify: `frontend/app/api/[...path]/route.ts`

- [ ] **Step 1: Add `'Trips'` to `ROOT_MOUNTED_PREFIXES`**

```typescript
const ROOT_MOUNTED_PREFIXES = new Set(['Train', 'Journeys', 'JourneyTemplates', 'Trips']);
```

- [ ] **Step 2: Add the matching `isAllowed` check** (`/Trips/plan` is
  always called with a trailing segment, unlike `Journeys`/`JourneyTemplates`,
  which also need their own bare-path check — `Trips` needs only the
  `startsWith` form):

```typescript
  const isAllowed =
    target.pathname.startsWith('/public/') ||
    target.pathname.startsWith('/Train/') ||
    target.pathname === '/Journeys' ||
    target.pathname.startsWith('/Journeys/') ||
    target.pathname === '/JourneyTemplates' ||
    target.pathname.startsWith('/JourneyTemplates/') ||
    target.pathname.startsWith('/Trips/');
```

- [ ] **Step 3: Update this file's own explanatory comment** (`route.ts:26-53`)
  to add one sentence naming `/Trips/...` alongside the existing three,
  matching that comment's own established per-addition pattern.

- [ ] **Step 4: Verify** — no existing test in this repo directly exercises
  this proxy file's routing table in isolation (confirmed by `grep -rn
  "ROOT_MOUNTED_PREFIXES\|resolveTargetPath" frontend/**/*.test.ts`,
  expected empty); Task 2's own `lib/tripPlan.test.ts` and this phase's
  manual UI verification step are what exercise this change for real.
  Run:

```bash
npx tsc --noEmit
```

  Expected: no new type errors.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/api/[...path]/route.ts
git commit -m "frontend: allow the same-origin proxy to reach GET /Trips/plan"
```

---

## Task 2: Wire types + fetch helper

**Files:**
- Modify: `frontend/lib/types.ts`
- Create: `frontend/lib/tripPlan.ts`
- Create: `frontend/lib/tripPlan.test.ts`

**Interfaces:**
- Produces: `TripPlanLeg`, `TripPlanItinerary`, `TripPlanSegment`,
  `TripPlanResponse` (types), `buildTripPlanQuery`, `fetchTripPlan`
  (functions) — consumed by Task 3/4/5.

- [ ] **Step 1: Add the wire types to `frontend/lib/types.ts`**, near
  `JourneyDetail` — **re-verify every field name against Phase 5's actual
  shipped `PlannedLeg`/`PlannedItinerary` `#[derive(Serialize)]` structs
  and `routes::trips::get_trip_plan`'s JSON response before writing this**
  (per this plan's own Global Constraints):

```typescript
/** One leg of a `GET /Trips/plan` itinerary
 * (`crates/api/src/data/trip_planning_itinerary.rs::PlannedLeg`,
 * camelCase, discriminated by `kind`). A `transfer` leg has no train
 * identity at all -- it is a walk/tube/bus/ferry hop with no
 * corresponding `journey_legs` row ever created for it (see this plan's
 * own Judgment Call 3). */
export type TripPlanLeg =
  | {
      kind: 'train';
      trainUid: string;
      serviceDate: string; // "YYYY-MM-DD"
      originCrs: string | null;
      destinationCrs: string | null;
      scheduledDeparture: string; // "HH:MM:SS"
      scheduledArrival: string;
      arrivalDayOffset: number;
    }
  | {
      kind: 'transfer';
      mode: string;
      originCrs: string | null;
      destinationCrs: string | null;
      minutes: number;
    };

/** One candidate itinerary for one segment
 * (`crates/api/src/data/trip_planning_itinerary.rs::PlannedItinerary`).
 * `exceedsRecommendedChanges` is only ever present for `results=fastest`
 * (CSA has no interchange-count cap of its own, see that Rust module's
 * own doc comment) -- absent (not `false`) for a `results=options` entry. */
export interface TripPlanItinerary {
  legs: TripPlanLeg[];
  changeCount: number;
  totalDurationMinutes: number;
  exceedsRecommendedChanges?: boolean;
}

/** One origin->destination hop of a (possibly multi-waypoint) plan
 * (`routes::trips::get_trip_plan`'s own `"segments"` array entry). */
export interface TripPlanSegment {
  originCrs: string;
  destinationCrs: string;
  itineraries: TripPlanItinerary[];
  cappedByMaxChanges: boolean;
}

/** `GET /Trips/plan`'s full response. */
export interface TripPlanResponse {
  results: 'fastest' | 'options';
  segments: TripPlanSegment[];
}
```

- [ ] **Step 2: Write `lib/tripPlan.ts`**

```typescript
import type { TripPlanResponse } from './types';

export interface TripPlanQuery {
  originCrs: string;
  destinationCrs: string;
  /** Ordered, e.g. `['YRK', 'NCL']` -- entered order, never reordered. */
  waypointCrs: string[];
  date: string; // "YYYY-MM-DD"
  departAfter?: string; // "HH:MM"
  results: 'fastest' | 'options';
}

/** Builds `GET /Trips/plan`'s query string from a [`TripPlanQuery`] --
 * pure and independently testable, matching this codebase's own
 * "small pure helper, tested separately from the fetch call" convention
 * (e.g. `lib/trackAgainPrefill.ts`'s own `trackAgainHref`). */
export function buildTripPlanQuery(query: TripPlanQuery): string {
  const params = new URLSearchParams({
    origin: query.originCrs.trim().toUpperCase(),
    destination: query.destinationCrs.trim().toUpperCase(),
    date: query.date,
    results: query.results,
  });
  const waypoints = query.waypointCrs.map(c => c.trim().toUpperCase()).filter(c => c.length > 0);
  if (waypoints.length > 0) {
    params.set('waypoints', waypoints.join(','));
  }
  if (query.departAfter) {
    params.set('departAfter', `${query.departAfter}:00`);
  }
  return params.toString();
}

export class TripPlanError extends Error {
  constructor(
    message: string,
    public status: number
  ) {
    super(message);
  }
}

/** Calls `GET /api/Trips/plan` (the same-origin proxy, Task 1) and returns
 * the parsed response, or throws [`TripPlanError`] with the backend's own
 * plain-text error body as its message -- both `400` (bad CRS/results
 * value) and `404` (no CIF data published for this date yet) are real,
 * distinct, user-actionable outcomes (this plan's own Review Focus), so
 * the caller can render each differently rather than one generic failure
 * message. */
export async function fetchTripPlan(query: TripPlanQuery): Promise<TripPlanResponse> {
  const response = await fetch(`/api/Trips/plan?${buildTripPlanQuery(query)}`);
  if (!response.ok) {
    const body = await response.text();
    throw new TripPlanError(body || 'Could not plan this trip.', response.status);
  }
  return response.json();
}
```

- [ ] **Step 3: Write `lib/tripPlan.test.ts`**

```typescript
import { describe, expect, it, vi, afterEach } from 'vitest';
import { buildTripPlanQuery, fetchTripPlan, TripPlanError } from './tripPlan';

describe('buildTripPlanQuery', () => {
  it('builds a minimal query with no waypoints or departAfter', () => {
    const query = buildTripPlanQuery({
      originCrs: 'eus',
      destinationCrs: 'mkc',
      waypointCrs: [],
      date: '2026-09-23',
      results: 'fastest',
    });
    const params = new URLSearchParams(query);
    expect(params.get('origin')).toBe('EUS');
    expect(params.get('destination')).toBe('MKC');
    expect(params.get('results')).toBe('fastest');
    expect(params.has('waypoints')).toBe(false);
    expect(params.has('departAfter')).toBe(false);
  });

  it('joins ordered waypoints with a comma, uppercased', () => {
    const query = buildTripPlanQuery({
      originCrs: 'EUS',
      destinationCrs: 'EDB',
      waypointCrs: ['york', 'ncl'],
      date: '2026-09-23',
      results: 'options',
    });
    expect(new URLSearchParams(query).get('waypoints')).toBe('YORK,NCL');
  });

  it('filters out blank waypoint entries', () => {
    const query = buildTripPlanQuery({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: ['', '  ', 'YRK'],
      date: '2026-09-23',
      results: 'fastest',
    });
    expect(new URLSearchParams(query).get('waypoints')).toBe('YRK');
  });

  it('appends :00 to a departAfter HH:MM value', () => {
    const query = buildTripPlanQuery({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-09-23',
      departAfter: '08:30',
      results: 'fastest',
    });
    expect(new URLSearchParams(query).get('departAfter')).toBe('08:30:00');
  });
});

describe('fetchTripPlan', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('returns the parsed response on success', async () => {
    const body = { results: 'fastest', segments: [] };
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(body) } as Response)
    );
    const result = await fetchTripPlan({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-09-23',
      results: 'fastest',
    });
    expect(result).toEqual(body);
  });

  it('throws TripPlanError with the backend message and status on failure', async () => {
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue({ ok: false, status: 404, text: () => Promise.resolve('no schedule data published') } as Response)
    );
    await expect(
      fetchTripPlan({ originCrs: 'EUS', destinationCrs: 'MKC', waypointCrs: [], date: '2099-01-01', results: 'fastest' })
    ).rejects.toMatchObject(new TripPlanError('no schedule data published', 404));
  });
});
```

- [ ] **Step 4: Run the tests**

```bash
npm test -- tripPlan.test.ts
```

  Expected: all 6 pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/tripPlan.ts frontend/lib/tripPlan.test.ts
git commit -m "frontend: add GET /Trips/plan wire types and fetch helper"
```

---

## Task 3: `PlanTripForm.tsx` — the input form

**Files:**
- Create: `frontend/components/PlanTripForm.tsx`
- Create: `frontend/components/PlanTripForm.test.tsx`

**Interfaces:**
- Produces: `PlanTripForm` component, `onSubmit(query: TripPlanQuery):
  void` prop.

- [ ] **Step 1: Read `TrackTrainForm.tsx`'s existing Origin `Autocomplete`
  block (around line 1081-1156, per this plan's own research pass) in full
  before writing this component**, so the station-suggestion wiring
  (`searchStations`, debounce/abort-signal handling, the exact
  `Autocomplete` props used) is mirrored precisely rather than
  reconstructed from memory — copy that pattern's shape for this form's own
  Origin/Destination/each-waypoint fields.

- [ ] **Step 2: Write the component** — origin, destination, an
  add/remove-able ordered list of waypoint fields, a date picker, an
  optional "depart after" time field, a `results` `SegmentedControl`
  (`Fastest` / `Compare options`, mirroring `TrackTrainForm`'s own
  `SegmentedControl` convention for its `mode` toggle), and a submit
  button:

```typescript
'use client';

import { useState } from 'react';
import { Autocomplete, Button, Group, SegmentedControl, Stack, TextInput, ActionIcon } from '@mantine/core';
import { DateInput, TimeInput } from '@mantine/dates';
import { IconPlus, IconX } from '@tabler/icons-react';
import { searchStations } from '@/lib/suggestions';
import type { TripPlanQuery } from '@/lib/tripPlan';

/** The input side of "Plan a route for me" (design spec §5.3) -- collects
 * origin, destination, ordered optional waypoints, a date, an optional
 * earliest-departure time, and a fastest/options preference, then hands a
 * ready-to-fetch [`TripPlanQuery`] to its caller (`PlanTripFlow`, Task 5).
 * Mirrors `TrackTrainForm.tsx`'s own station-autocomplete and
 * `SegmentedControl` conventions rather than reinventing them -- see this
 * task's own Step 1. */
export function PlanTripForm({ onSubmit }: { onSubmit: (query: TripPlanQuery) => void }) {
  const [originCrs, setOriginCrs] = useState('');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [waypoints, setWaypoints] = useState<string[]>([]);
  const [date, setDate] = useState<Date | null>(new Date());
  const [departAfter, setDepartAfter] = useState('');
  const [results, setResults] = useState<'fastest' | 'options'>('fastest');

  function addWaypoint() {
    setWaypoints(current => [...current, '']);
  }

  function updateWaypoint(index: number, value: string) {
    setWaypoints(current => current.map((existing, i) => (i === index ? value : existing)));
  }

  function removeWaypoint(index: number) {
    setWaypoints(current => current.filter((_, i) => i !== index));
  }

  function handleSubmit() {
    if (!originCrs.trim() || !destinationCrs.trim() || !date) return;
    onSubmit({
      originCrs: originCrs.trim(),
      destinationCrs: destinationCrs.trim(),
      waypointCrs: waypoints,
      date: date.toISOString().slice(0, 10),
      departAfter: departAfter || undefined,
      results,
    });
  }

  const canSubmit = originCrs.trim().length > 0 && destinationCrs.trim().length > 0 && date !== null;

  return (
    <Stack gap="md">
      <Autocomplete
        label="From"
        placeholder="Station name or CRS code"
        value={originCrs}
        onChange={setOriginCrs}
        data={[]}
        // See this task's Step 1 -- wire this exactly like
        // TrackTrainForm.tsx's own Origin Autocomplete, including its
        // debounced searchStations(query, signal) call and abort-signal
        // cleanup, rather than the placeholder `data={[]}` above.
      />
      <Autocomplete
        label="To"
        placeholder="Station name or CRS code"
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={[]}
      />
      {waypoints.map((waypoint, index) => (
        <Group key={index} gap="xs">
          <TextInput
            style={{ flex: 1 }}
            label={index === 0 ? 'Via (optional, in order)' : undefined}
            placeholder="Station name or CRS code"
            value={waypoint}
            onChange={event => updateWaypoint(index, event.currentTarget.value)}
          />
          <ActionIcon color="red" variant="subtle" mt={index === 0 ? 24 : 0} onClick={() => removeWaypoint(index)} aria-label="Remove this waypoint">
            <IconX size={16} />
          </ActionIcon>
        </Group>
      ))}
      <Button variant="subtle" leftSection={<IconPlus size={16} />} onClick={addWaypoint} style={{ alignSelf: 'flex-start' }}>
        Add a waypoint
      </Button>
      <DateInput label="Date" value={date} onChange={setDate} minDate={new Date()} />
      <TimeInput label="Depart after (optional)" value={departAfter} onChange={event => setDepartAfter(event.currentTarget.value)} />
      <SegmentedControl
        value={results}
        onChange={value => setResults(value as 'fastest' | 'options')}
        data={[
          { label: 'Fastest', value: 'fastest' },
          { label: 'Compare options', value: 'options' },
        ]}
      />
      <Button disabled={!canSubmit} onClick={handleSubmit}>
        Find routes
      </Button>
    </Stack>
  );
}
```

- [ ] **Step 3: Write `PlanTripForm.test.tsx`**

```typescript
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { PlanTripForm } from './PlanTripForm';

describe('PlanTripForm', () => {
  it('disables Find routes until both origin and destination are entered', () => {
    render(<PlanTripForm onSubmit={vi.fn()} />);
    expect(screen.getByText('Find routes')).toBeDisabled();
    fireEvent.change(screen.getByLabelText('From'), { target: { value: 'EUS' } });
    expect(screen.getByText('Find routes')).toBeDisabled();
    fireEvent.change(screen.getByLabelText('To'), { target: { value: 'MKC' } });
    expect(screen.getByText('Find routes')).not.toBeDisabled();
  });

  it('adds and removes waypoint fields', () => {
    render(<PlanTripForm onSubmit={vi.fn()} />);
    expect(screen.queryByPlaceholderText('Station name or CRS code', { selector: 'input' })).toBeTruthy();
    fireEvent.click(screen.getByText('Add a waypoint'));
    const waypointInputs = screen.getAllByPlaceholderText('Station name or CRS code');
    // From + To + 1 waypoint = 3 inputs sharing this placeholder.
    expect(waypointInputs.length).toBe(3);
    fireEvent.click(screen.getByLabelText('Remove this waypoint'));
    expect(screen.getAllByPlaceholderText('Station name or CRS code').length).toBe(2);
  });

  it('calls onSubmit with a well-formed query, including entered-order waypoints', () => {
    const onSubmit = vi.fn();
    render(<PlanTripForm onSubmit={onSubmit} />);
    fireEvent.change(screen.getByLabelText('From'), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: 'EDB' } });
    fireEvent.click(screen.getByText('Add a waypoint'));
    fireEvent.change(screen.getAllByPlaceholderText('Station name or CRS code')[2], { target: { value: 'YRK' } });
    fireEvent.click(screen.getByText('Find routes'));
    expect(onSubmit).toHaveBeenCalledWith(
      expect.objectContaining({ originCrs: 'EUS', destinationCrs: 'EDB', waypointCrs: ['YRK'], results: 'fastest' })
    );
  });
});
```

- [ ] **Step 4: Run the tests**

```bash
npm test -- PlanTripForm.test.tsx
```

- [ ] **Step 5: Commit**

```bash
git add frontend/components/PlanTripForm.tsx frontend/components/PlanTripForm.test.tsx
git commit -m "frontend: add PlanTripForm, the trip-planning input form"
```

---

## Task 4: `ItineraryOption.tsx` — render and select one itinerary

**Files:**
- Create: `frontend/components/ItineraryOption.tsx`
- Create: `frontend/components/ItineraryOption.test.tsx`

- [ ] **Step 1: Write the component**

```typescript
import { Badge, Card, Group, Radio, Stack, Text } from '@mantine/core';
import type { TripPlanItinerary, TripPlanLeg } from '@/lib/types';

function legSummary(leg: TripPlanLeg): string {
  if (leg.kind === 'train') {
    const from = leg.originCrs ?? '?';
    const to = leg.destinationCrs ?? '?';
    return `${leg.scheduledDeparture.slice(0, 5)} ${from} → ${to} ${leg.scheduledArrival.slice(0, 5)}`;
  }
  const from = leg.originCrs ?? '?';
  const to = leg.destinationCrs ?? '?';
  return `Walk/transfer (${leg.mode}) ${from} → ${to}, ${leg.minutes} min`;
}

/** One selectable itinerary card -- design spec §5.3's "route-summary
 * display for both `'fastest'` and `'options'` responses." Disabled with an
 * explanatory message when the itinerary has no train leg at all (this
 * plan's own Judgment Call 4 -- there is nothing a `POST /Journeys` call
 * could create for a walk with no train on either side of it). */
export function ItineraryOption({
  itinerary,
  selected,
  onSelect,
}: {
  itinerary: TripPlanItinerary;
  selected: boolean;
  onSelect: () => void;
}) {
  const hasTrainLeg = itinerary.legs.some(leg => leg.kind === 'train');

  return (
    <Card withBorder>
      <Group justify="space-between" wrap="wrap">
        <Radio
          checked={selected}
          onChange={onSelect}
          disabled={!hasTrainLeg}
          label={
            <Stack gap={4}>
              {itinerary.legs.map((leg, index) => (
                <Text key={index} size="sm">
                  {legSummary(leg)}
                </Text>
              ))}
            </Stack>
          }
        />
        <Stack gap={4} align="flex-end">
          <Text size="sm">{itinerary.totalDurationMinutes} min</Text>
          <Badge color={itinerary.changeCount === 0 ? 'green' : 'blue'}>
            {itinerary.changeCount} {itinerary.changeCount === 1 ? 'change' : 'changes'}
          </Badge>
          {itinerary.exceedsRecommendedChanges && (
            <Text size="xs" c="orange">
              More changes than usually recommended
            </Text>
          )}
        </Stack>
      </Group>
      {!hasTrainLeg && (
        <Text size="xs" c="dimmed" mt="xs">
          This route needs no train — there is nothing to track.
        </Text>
      )}
    </Card>
  );
}
```

- [ ] **Step 2: Write the tests**

```typescript
import { render, screen, fireEvent } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ItineraryOption } from './ItineraryOption';
import type { TripPlanItinerary } from '@/lib/types';

const trainItinerary: TripPlanItinerary = {
  legs: [
    {
      kind: 'train',
      trainUid: 'C11052',
      serviceDate: '2026-09-23',
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      scheduledDeparture: '08:00:00',
      scheduledArrival: '08:50:00',
      arrivalDayOffset: 0,
    },
  ],
  changeCount: 0,
  totalDurationMinutes: 50,
};

const fixedLinkOnlyItinerary: TripPlanItinerary = {
  legs: [{ kind: 'transfer', mode: 'TUBE', originCrs: 'EUS', destinationCrs: 'KGX', minutes: 5 }],
  changeCount: 0,
  totalDurationMinutes: 5,
};

describe('ItineraryOption', () => {
  it('renders a train leg summary and allows selection', () => {
    const onSelect = vi.fn();
    render(<ItineraryOption itinerary={trainItinerary} selected={false} onSelect={onSelect} />);
    expect(screen.getByText(/08:00 EUS → MKC 08:50/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('radio'));
    expect(onSelect).toHaveBeenCalled();
  });

  it('flags an itinerary that exceeds the recommended change count', () => {
    render(
      <ItineraryOption
        itinerary={{ ...trainItinerary, changeCount: 3, exceedsRecommendedChanges: true }}
        selected={false}
        onSelect={vi.fn()}
      />
    );
    expect(screen.getByText('More changes than usually recommended')).toBeInTheDocument();
  });

  it('disables selection for a fixed-link-only itinerary with no train leg', () => {
    render(<ItineraryOption itinerary={fixedLinkOnlyItinerary} selected={false} onSelect={vi.fn()} />);
    expect(screen.getByRole('radio')).toBeDisabled();
    expect(screen.getByText('This route needs no train — there is nothing to track.')).toBeInTheDocument();
  });
});
```

- [ ] **Step 3: Run the tests, commit**

```bash
npm test -- ItineraryOption.test.tsx
git add frontend/components/ItineraryOption.tsx frontend/components/ItineraryOption.test.tsx
git commit -m "frontend: add ItineraryOption, a selectable itinerary summary card"
```

---

## Task 5: `PlanTripFlow.tsx` — orchestration + multi-leg creation

**Files:**
- Create: `frontend/components/PlanTripFlow.tsx`
- Create: `frontend/components/PlanTripFlow.test.tsx`

**Interfaces:**
- Produces: `PlanTripFlow` component, `onCreated(result: CreateJourneyResponse):
  void` prop — the exact same prop shape `TrackTrainForm`'s own `onCreated`
  already has, per this plan's Judgment Call 2.

- [ ] **Step 1: Write the component**

```typescript
'use client';

import { useState } from 'react';
import { Alert, Button, Stack, Text } from '@mantine/core';
import { PlanTripForm } from './PlanTripForm';
import { ItineraryOption } from './ItineraryOption';
import { fetchTripPlan, TripPlanError, type TripPlanQuery } from '@/lib/tripPlan';
import type { CreateJourneyResponse, TripPlanItinerary, TripPlanResponse } from '@/lib/types';

interface SegmentSelection {
  itinerary: TripPlanItinerary | null;
}

/** "Plan a route for me" (design spec §5.3): the form (Task 3) → results
 * comparison (Task 4, per segment) → the multi-leg creation sequence
 * (this task) → hands off to `JourneyCreationFlow`'s existing
 * `handleLegOneCreated`-driven view via `onCreated`, called EXACTLY ONCE
 * after this component's own creation sequence finishes -- see this
 * plan's own Judgment Call 2 for why the sequence lives here, not spread
 * across `JourneyCreationFlow`'s incremental "Add a leg" flow. */
export function PlanTripFlow({ onCreated }: { onCreated: (result: CreateJourneyResponse) => void }) {
  const [plan, setPlan] = useState<TripPlanResponse | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [selections, setSelections] = useState<SegmentSelection[]>([]);
  const [creating, setCreating] = useState(false);
  const [creationError, setCreationError] = useState<string | null>(null);

  async function handleSearch(query: TripPlanQuery) {
    setPlanError(null);
    setPlan(null);
    try {
      const result = await fetchTripPlan(query);
      setPlan(result);
      setSelections(result.segments.map(() => ({ itinerary: null })));
    } catch (error) {
      setPlanError(error instanceof TripPlanError ? error.message : 'Could not plan this trip. Please try again.');
    }
  }

  function selectItinerary(segmentIndex: number, itinerary: TripPlanItinerary) {
    setSelections(current => current.map((selection, i) => (i === segmentIndex ? { itinerary } : selection)));
  }

  const allSegmentsSelected =
    plan !== null && selections.length === plan.segments.length && selections.every(s => s.itinerary !== null);

  async function handleTrackJourney() {
    if (!allSegmentsSelected) return;
    setCreating(true);
    setCreationError(null);

    // Every TRAIN leg across every selected segment, in order -- a
    // TransferLeg never becomes a journey_legs row (this plan's own
    // Judgment Call 3).
    const trainLegs = selections.flatMap(selection =>
      (selection.itinerary?.legs ?? []).filter((leg): leg is Extract<typeof leg, { kind: 'train' }> => leg.kind === 'train')
    );

    if (trainLegs.length === 0) {
      setCreationError('This route needs no train — there is nothing to track.');
      setCreating(false);
      return;
    }

    try {
      const firstLeg = trainLegs[0];
      const createResponse = await fetch('/api/Journeys', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          leg: { mode: 'knownTrain', trainUid: firstLeg.trainUid, serviceDate: firstLeg.serviceDate },
        }),
      });
      if (!createResponse.ok) {
        throw new Error(await createResponse.text());
      }
      const created: CreateJourneyResponse = await createResponse.json();

      for (let i = 1; i < trainLegs.length; i += 1) {
        const leg = trainLegs[i];
        const addResponse = await fetch(`/api/Journeys/${created.journeyId}/legs`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ mode: 'knownTrain', trainUid: leg.trainUid, serviceDate: leg.serviceDate }),
        });
        if (!addResponse.ok) {
          // Partial success: leg 1..i already exist. Tell the visitor
          // exactly which leg failed, then still hand off -- the journey
          // that DOES exist must not strand them (this plan's own Review
          // Focus, matching JourneyCreationFlow's own established
          // refetch-failure posture).
          setCreationError(
            `Tracked ${i} of ${trainLegs.length} legs. Adding leg ${i + 1} failed: ${await addResponse.text()}. ` +
              'You can add it manually from the journey page.'
          );
          onCreated(created);
          return;
        }
      }

      onCreated(created);
    } catch (error) {
      setCreationError(error instanceof Error ? error.message : 'Could not create this journey. Please try again.');
    } finally {
      setCreating(false);
    }
  }

  return (
    <Stack gap="md">
      <PlanTripForm onSubmit={handleSearch} />
      {planError && (
        <Alert color="red" title="Couldn't plan this trip">
          {planError}
        </Alert>
      )}
      {plan &&
        plan.segments.map((segment, segmentIndex) => (
          <Stack key={segmentIndex} gap="xs">
            <Text fw={600}>
              {segment.originCrs} → {segment.destinationCrs}
            </Text>
            {segment.itineraries.length === 0 && (
              <Alert color="yellow">No route found for {segment.originCrs} → {segment.destinationCrs}.</Alert>
            )}
            {segment.cappedByMaxChanges && (
              <Text size="xs" c="orange">
                A faster route exists with more changes than shown below.
              </Text>
            )}
            {segment.itineraries.map((itinerary, itineraryIndex) => (
              <ItineraryOption
                key={itineraryIndex}
                itinerary={itinerary}
                selected={selections[segmentIndex]?.itinerary === itinerary}
                onSelect={() => selectItinerary(segmentIndex, itinerary)}
              />
            ))}
          </Stack>
        ))}
      {creationError && (
        <Alert color="red" title="Some legs could not be created">
          {creationError}
        </Alert>
      )}
      {plan && (
        <Button disabled={!allSegmentsSelected || creating} loading={creating} onClick={() => void handleTrackJourney()}>
          Track this journey
        </Button>
      )}
    </Stack>
  );
}
```

- [ ] **Step 2: Write `PlanTripFlow.test.tsx`** — mocks `fetch` directly
  (matching `lib/tripPlan.test.ts`'s own `vi.stubGlobal('fetch', ...)`
  convention):

```typescript
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { describe, expect, it, vi, afterEach } from 'vitest';
import { PlanTripFlow } from './PlanTripFlow';
import type { TripPlanResponse } from '@/lib/types';

const singleSegmentPlan: TripPlanResponse = {
  results: 'fastest',
  segments: [
    {
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      cappedByMaxChanges: false,
      itineraries: [
        {
          legs: [
            {
              kind: 'train',
              trainUid: 'C11052',
              serviceDate: '2026-09-23',
              originCrs: 'EUS',
              destinationCrs: 'MKC',
              scheduledDeparture: '08:00:00',
              scheduledArrival: '08:50:00',
              arrivalDayOffset: 0,
            },
          ],
          changeCount: 0,
          totalDurationMinutes: 50,
        },
      ],
    },
  ],
};

describe('PlanTripFlow', () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('creates the journey via POST /api/Journeys after picking the only itinerary', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: () => Promise.resolve(singleSegmentPlan) } as Response)
      .mockResolvedValueOnce({
        ok: true,
        json: () => Promise.resolve({ journeyId: 42, legId: 1, trackingId: 7, resolutionStatus: null }),
      } as Response);
    vi.stubGlobal('fetch', fetchMock);

    const onCreated = vi.fn();
    render(<PlanTripFlow onCreated={onCreated} />);

    fireEvent.change(screen.getByLabelText('From'), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText(/EUS → MKC/);
    fireEvent.click(screen.getByRole('radio'));
    fireEvent.click(screen.getByText('Track this journey'));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith(expect.objectContaining({ journeyId: 42 })));
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/Journeys',
      expect.objectContaining({
        method: 'POST',
        body: JSON.stringify({ leg: { mode: 'knownTrain', trainUid: 'C11052', serviceDate: '2026-09-23' } }),
      })
    );
  });

  it('shows a plain-text error and does not offer the button when the plan request fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue({ ok: false, status: 404, text: () => Promise.resolve('no schedule data published') } as Response)
    );
    render(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('From'), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: 'MKC' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('no schedule data published');
    expect(screen.queryByText('Track this journey')).not.toBeInTheDocument();
  });

  it('names the failing segment when a segment has no itineraries', async () => {
    const noRoutePlan: TripPlanResponse = {
      results: 'fastest',
      segments: [{ originCrs: 'EUS', destinationCrs: 'ZZZ', itineraries: [], cappedByMaxChanges: false }],
    };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue({ ok: true, json: () => Promise.resolve(noRoutePlan) } as Response));
    render(<PlanTripFlow onCreated={vi.fn()} />);
    fireEvent.change(screen.getByLabelText('From'), { target: { value: 'EUS' } });
    fireEvent.change(screen.getByLabelText('To'), { target: { value: 'ZZZ' } });
    fireEvent.click(screen.getByText('Find routes'));

    await screen.findByText('No route found for EUS → ZZZ.');
  });
});
```

- [ ] **Step 3: Run the tests**

```bash
npm test -- PlanTripFlow.test.tsx
```

- [ ] **Step 4: Commit**

```bash
git add frontend/components/PlanTripFlow.tsx frontend/components/PlanTripFlow.test.tsx
git commit -m "frontend: add PlanTripFlow, the plan-then-track orchestration component"
```

---

## Task 6: Wire the third mode into `JourneyCreationFlow.tsx`

**Files:**
- Modify: `frontend/components/JourneyCreationFlow.tsx`

- [ ] **Step 1: Add a mode selector**, shown only while `journeyId === null`
  (the existing first branch), defaulting to `'known'` so nothing changes
  for a visitor who takes no action — direct extension of the existing
  `if (journeyId === null) { return <TrackTrainForm onCreated={...} />; }`
  branch:

```typescript
  const [entryMode, setEntryMode] = useState<'known' | 'plan'>('known');

  if (journeyId === null) {
    return (
      <Stack gap="md">
        <SegmentedControl
          value={entryMode}
          onChange={value => setEntryMode(value as 'known' | 'plan')}
          data={[
            { label: 'I know my route', value: 'known' },
            { label: 'Plan a route for me', value: 'plan' },
          ]}
        />
        {entryMode === 'known' ? (
          <TrackTrainForm onCreated={handleLegOneCreated} />
        ) : (
          <PlanTripFlow onCreated={handleLegOneCreated} />
        )}
      </Stack>
    );
  }
```

  Add the two new imports (`SegmentedControl` from `@mantine/core`,
  `PlanTripFlow` from `./PlanTripFlow`) alongside this file's existing
  ones.

- [ ] **Step 2: Manually verify in a real browser** (per this plan's own
  Global Constraints) — plan a real route with a real change of trains on
  the running dev stack, confirm both `results` modes render sensibly,
  pick an itinerary, confirm the resulting journey's legs appear correctly
  on `/journeys/{id}` afterward, exactly as if a human had picked each
  train candidate by hand.

- [ ] **Step 3: Run the full frontend CI sequence**

```bash
npm run lint
npx tsc --noEmit
npm test
npm run build
```

  Expected: all four pass clean — the exact four steps
  `.github/workflows/ci.yml`'s `frontend` job runs.

- [ ] **Step 4: Commit**

```bash
git add frontend/components/JourneyCreationFlow.tsx
git commit -m "frontend: add 'Plan a route for me' as a third JourneyCreationFlow mode"
```

---

## Self-review notes

- **Spec coverage**: §5.3's "third mode... origin, destination, optional
  waypoints, date, and a fastest/options toggle → candidate itineraries...
  → pick one → §5.1's creation sequence, reusing `JourneyCreationFlow`'s
  existing... state machine" is implemented end to end across Tasks 2-6.
  §5.1 point 5's open walking-transfer-leg question is resolved for v1 as
  presentation-only (Judgment Call 3), consistent with it being explicitly
  left open by the design spec itself.
- **Placeholder scan**: `PlanTripForm.tsx`'s own Autocomplete `data={[]}`
  is flagged explicitly, in both the task's own Step 1 and an inline code
  comment, as a placeholder the implementer MUST replace by mirroring
  `TrackTrainForm.tsx`'s real wiring — not silently shipped as-is; every
  other piece of logic in this phase is complete and tested.
- **Type consistency**: `TripPlanLeg`/`TripPlanItinerary`/`TripPlanSegment`/
  `TripPlanResponse` in `lib/types.ts` match `PlanTripFlow.tsx`'s own usage
  (`selection.itinerary?.legs`, `leg.kind === 'train'`) and
  `ItineraryOption.tsx`'s rendering exactly; `onCreated`'s
  `CreateJourneyResponse` type is the same one `TrackTrainForm.tsx`'s own
  `onCreated` prop already uses, confirmed via `frontend/lib/types.ts:880-885`.
- **Review Focus**: all five items have a directly corresponding test
  (`shows a plain-text error...`, `names the failing segment...`, the
  fixed-link-only disabled-radio test in `ItineraryOption.test.tsx`, the
  partial-failure `creationError` path in `PlanTripFlow.tsx`'s own
  `handleTrackJourney`, and the `cappedByMaxChanges` rendering in
  `PlanTripFlow.tsx`'s JSX).
