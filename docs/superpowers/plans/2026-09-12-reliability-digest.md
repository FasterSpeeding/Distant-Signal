# Personal Reliability / Delay Repay Digest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `ReliabilityDigest` card to `/track/mine` that summarizes a logged-in user's own tracked-journey punctuality (on-time %, average delay, worst journeys) and a Delay Repay eligibility rollup (counts per compensation band, never a currency total), computed entirely client-side from the two arrays the page already fetches.

**Architecture:** Two pure TypeScript functions (`frontend/lib/reliabilityDigest.ts`) — `computePunctualitySummary(trains, today)` and `computeDelayRepayRollup(tickets)` — operate on the already-fetched `TrackedTrainListItem[]`/`TicketListItem[]`. A new presentational component (`frontend/components/ReliabilityDigest.tsx`) composes their output into a `PunctualitySection` and a `DelayRepaySection`, and is rendered once from `frontend/app/track/mine/page.tsx`, gated on the same `nothingToShow` check the page already computes. No backend change, no new fetch, no new route.

**Tech Stack:** Next.js (App Router) Server Component (`page.tsx` stays a Server Component; `ReliabilityDigest` is a plain, non-`'use client'` presentational component — it takes already-fetched props, same as `TrackedTrainListRow`), TypeScript, Mantine v9 (`Card`, `Stack`, `Group`, `Text`, `Alert`, `Title`), Vitest (`frontend/lib/reliabilityDigest.test.ts`, plain, no DOM) + Vitest/`@testing-library/react` (`frontend/components/ReliabilityDigest.test.tsx`, mirroring `DelayRepayEstimate.test.tsx`'s structure and `frontend/test/render.tsx`'s `renderWithMantine` helper).

**Spec:** `docs/superpowers/specs/2026-09-12-reliability-digest-design.md`

## Global Constraints

- **No backend work at all.** No new migration, no new Rust route, no new SQL, no change to `crates/api/src/data/delay_repay_rules.rs`. Every number in this feature is derived from `GET /Train/mine` (`getMyTrackedTrains()`) and `GET /Train/tickets/mine` (`getMyTickets()`), both already fetched by `frontend/app/track/mine/page.tsx` in one `Promise.all` (spec Decision 5, Non-goals).
- **No currency total, ever.** The Delay Repay rollup reports **counts per `${scheme}-${bandMinutes}` band**, never an averaged or summed percentage/amount — `tracked_train_tickets`'s own migration forbids storing fare/price data, and `DelayRepayEstimate.percentage` is a percentage of an unknown fare (spec Correction 1, Decision 3).
- **Punctuality population** (spec Decision 1), exactly:
  ```ts
  train.resolutionStatus === 'resolved' && train.delayMinutes !== null && train.serviceDate < today
  ```
  `today` is a `"YYYY-MM-DD"` string computed once per request from `londonDayKey(new Date())` (`frontend/lib/dateFormat.ts`) — this is a network-time value (a rail service date), so it uses that module's London-pinned formatter, not the viewer's local clock.
- **On-time definition**: `delayMinutes <= 0` — matching `RowStatusBadge`'s existing convention in `frontend/app/track/mine/page.tsx:299-303` (`train.delayMinutes > 0` renders "late", else "On time"). This is explicitly **not** `common::Defaults::delay_threshold_minutes` (the unrelated 5-minute backend line-status threshold) — spec Correction 6.
- **Cancelled journeys** (`status === 'cancelled'`) are excluded from the delay/on-time arithmetic (`avgDelayMinutes`, `onTimePct`) entirely and reported as a separate count — never blended into "average delay" (spec Decision 1).
- **Delay Repay rollup population** (spec Decision 3), exactly: denominator is tickets where `trackedTrainId !== null && operator !== null` (`attachedTicketsWithOperator`); numerator is that same set filtered to `estimate !== null` (`eligibleCount`). A standalone ticket (`trackedTrainId === null`) is excluded from both. An attached ticket with `operator === null` is excluded from the denominator too — **not** counted as "0% eligible" (there is no "we know and it's zero" signal for it).
- **The rollup consumes `TicketListItem.estimate`** (already computed server-side by `build_ticket_list_item`, wired to `delay_repay_rules::estimate_delay_repay` exactly once) — it never calls `estimate_delay_repay` itself and adds no new call site into `delay_repay_rules.rs` (spec Correction 5).
- **Hedged copy carries `DelayRepayEstimate.tsx`'s disclaimer discipline forward verbatim**: the top-level disclaimer sentence ("not a guarantee of compensation and not proof you travelled...") is reproduced in full, never paraphrased; no second near-duplicate disclaimer is layered on top of it; no claim-performing language ("claim now", "submit", "get your refund") anywhere; no outbound `<a>`/claim link is rendered by the rollup component at all (spec Decision 4).
- **Test framework: Vitest.** `frontend/lib/reliabilityDigest.test.ts` (plain Vitest, no DOM) and `frontend/components/ReliabilityDigest.test.tsx` (Vitest + `@testing-library/react`, via `frontend/test/render.tsx`'s `renderWithMantine`). Run via `npm test` (`vitest run`) from `frontend/`. **No backend/DB-backed tests are needed anywhere in this plan** — there is no Rust change, so no `#[sqlx::test]`/`#[ignore]`-gated coverage applies.
- **No pagination, filtering, or date-range picker** on the digest — it reports over whatever `MINE_LIST_LIMIT`/`MINE_TICKETS_LIMIT` already returned to the page (spec Decision 1, Non-goals).

---

## File Structure

- **Create:** `frontend/lib/reliabilityDigest.ts` — pure helper module (no I/O), mirroring `frontend/lib/sampleStats.ts`'s/`frontend/lib/trackingName.ts`'s "pure functions over already-typed arrays" shape. Exports `isEligibleForPunctuality`, `computePunctualitySummary`, `computeDelayRepayRollup`, and their result types (`PunctualitySummary`, `DelayRepayRollup`).
- **Create:** `frontend/lib/reliabilityDigest.test.ts` — Vitest coverage for all three functions, including the three drift-risk tests called out in Self-Review below.
- **Create:** `frontend/components/ReliabilityDigest.tsx` — new presentational component, composing `PunctualitySection` and `DelayRepaySection`. New copy, not a reuse of `DelayRepayEstimate.tsx`'s JSX (spec Decision 4).
- **Create:** `frontend/components/ReliabilityDigest.test.tsx` — Vitest + Testing Library coverage, mirroring `frontend/components/DelayRepayEstimate.test.tsx`'s structure.
- **Modify:** `frontend/app/track/mine/page.tsx` — import `ReliabilityDigest`, render it once between the page header and the trains/empty-state block, gated on `!nothingToShow`.

## Task 1: `computePunctualitySummary` — pure punctuality aggregation

**Files:**
- Create: `frontend/lib/reliabilityDigest.ts`
- Test: `frontend/lib/reliabilityDigest.test.ts`

**Interfaces:**
- Consumes: `TrackedTrainListItem` (`frontend/lib/types.ts:541-564`) — specifically `resolutionStatus: ResolutionStatus`, `delayMinutes: number | null`, `serviceDate: string`, `status: JourneyStatus | null`, `id: number`, `trainUid: string | null`.
- Produces (used by Task 3):
  ```ts
  export function isEligibleForPunctuality(train: TrackedTrainListItem, today: string): boolean;

  export interface WorstJourney {
    trainId: number;
    trainUid: string | null;
    serviceDate: string;
    delayMinutes: number;
  }

  export interface PunctualitySummary {
    eligibleCount: number;
    onTimePct: number | null;      // null when eligibleCount === 0
    avgDelayMinutes: number | null; // null when eligibleCount === 0
    cancelledCount: number;
    worstJourneys: WorstJourney[]; // at most 5, descending delayMinutes, ties by most recent serviceDate
  }

  export function computePunctualitySummary(trains: TrackedTrainListItem[], today: string): PunctualitySummary;
  ```

- [ ] **Step 1: Write the failing tests for `isEligibleForPunctuality`**

Create `frontend/lib/reliabilityDigest.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { isEligibleForPunctuality, computePunctualitySummary, computeDelayRepayRollup } from './reliabilityDigest';
import type { TrackedTrainListItem, TicketListItem } from './types';

const TODAY = '2026-09-12';

function train(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-09-10',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinOriginName: 'London Kings Cross',
    pinDestinationName: 'York',
    pinScheduledDeparture: '2026-09-10T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 3,
    trackedAt: '2026-09-09T12:00:00Z',
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

describe('isEligibleForPunctuality', () => {
  it('excludes pending', () => {
    expect(isEligibleForPunctuality(train({ resolutionStatus: 'pending', delayMinutes: null }), TODAY)).toBe(false);
  });

  it('excludes schedule_matched', () => {
    expect(
      isEligibleForPunctuality(train({ resolutionStatus: 'schedule_matched', delayMinutes: null }), TODAY),
    ).toBe(false);
  });

  it('excludes unresolved', () => {
    expect(isEligibleForPunctuality(train({ resolutionStatus: 'unresolved', delayMinutes: null }), TODAY)).toBe(
      false,
    );
  });

  it('excludes resolved with null delayMinutes', () => {
    expect(isEligibleForPunctuality(train({ resolutionStatus: 'resolved', delayMinutes: null }), TODAY)).toBe(
      false,
    );
  });

  it('excludes a resolved row whose serviceDate is today', () => {
    expect(isEligibleForPunctuality(train({ serviceDate: TODAY }), TODAY)).toBe(false);
  });

  it('excludes a resolved row whose serviceDate is in the future', () => {
    expect(isEligibleForPunctuality(train({ serviceDate: '2026-09-13' }), TODAY)).toBe(false);
  });

  it('includes a resolved, non-null-delay row strictly before today', () => {
    expect(isEligibleForPunctuality(train({ serviceDate: '2026-09-11' }), TODAY)).toBe(true);
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend && npx vitest run lib/reliabilityDigest.test.ts`
Expected: FAIL — `reliabilityDigest.ts` does not exist yet ("Failed to resolve import").

- [ ] **Step 3: Write `isEligibleForPunctuality`**

Create `frontend/lib/reliabilityDigest.ts`:

```ts
import type { TrackedTrainListItem, TicketListItem } from './types';

/** The punctuality half's population: a train whose real identity was
 * resolved, that has a real recorded delay figure, and whose service date
 * has fully elapsed. `serviceDate < today` (not a `status === 'completed'`
 * check) is the honest proxy for "this journey has actually happened" --
 * `train_current_state.status` never reliably reaches `'completed'` in
 * practice (a pre-existing, already-documented gap; see
 * docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md Finding
 * 1). `today` is a "YYYY-MM-DD" string the caller computes once per
 * request via `londonDayKey(new Date())` (`lib/dateFormat.ts`), not
 * recomputed per row. See
 * docs/superpowers/specs/2026-09-12-reliability-digest-design.md Decision
 * 1. */
export function isEligibleForPunctuality(train: TrackedTrainListItem, today: string): boolean {
  return train.resolutionStatus === 'resolved' && train.delayMinutes !== null && train.serviceDate < today;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run lib/reliabilityDigest.test.ts`
Expected: PASS (7 tests)

- [ ] **Step 5: Commit**

```bash
cd frontend && git add lib/reliabilityDigest.ts lib/reliabilityDigest.test.ts
git commit -m "feat: add isEligibleForPunctuality for the reliability digest"
```

- [ ] **Step 6: Write the failing tests for `computePunctualitySummary`**

Append to `frontend/lib/reliabilityDigest.test.ts`:

```ts
describe('computePunctualitySummary', () => {
  it('returns an explicit no-data shape when nothing is eligible', () => {
    const summary = computePunctualitySummary([train({ resolutionStatus: 'pending', delayMinutes: null })], TODAY);
    expect(summary).toEqual({
      eligibleCount: 0,
      onTimePct: null,
      avgDelayMinutes: null,
      cancelledCount: 0,
      worstJourneys: [],
    });
  });

  it('on-time uses delayMinutes <= 0, matching RowStatusBadge, not a 5-minute threshold', () => {
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 0 }),
        train({ id: 2, serviceDate: '2026-09-10', delayMinutes: -2 }),
        train({ id: 3, serviceDate: '2026-09-10', delayMinutes: 1 }),
        train({ id: 4, serviceDate: '2026-09-10', delayMinutes: 4 }),
      ],
      TODAY,
    );
    expect(summary.eligibleCount).toBe(4);
    expect(summary.onTimePct).toBe(50); // 2 of 4 have delayMinutes <= 0
  });

  it('excludes cancelled journeys from avgDelayMinutes/onTimePct and counts them separately', () => {
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 0, status: 'completed' }),
        train({ id: 2, serviceDate: '2026-09-10', delayMinutes: 90, status: 'cancelled' }),
      ],
      TODAY,
    );
    expect(summary.eligibleCount).toBe(1);
    expect(summary.onTimePct).toBe(100);
    expect(summary.avgDelayMinutes).toBe(0);
    expect(summary.cancelledCount).toBe(1);
  });

  it('worstJourneys returns at most 5, sorted descending by delayMinutes, ties broken by most recent serviceDate', () => {
    const trains = [
      train({ id: 1, serviceDate: '2026-09-01', delayMinutes: 10 }),
      train({ id: 2, serviceDate: '2026-09-05', delayMinutes: 30 }),
      train({ id: 3, serviceDate: '2026-09-02', delayMinutes: 30 }), // same delay as id 2, older date
      train({ id: 4, serviceDate: '2026-09-06', delayMinutes: 5 }),
      train({ id: 5, serviceDate: '2026-09-07', delayMinutes: 45 }),
      train({ id: 6, serviceDate: '2026-09-08', delayMinutes: 2 }),
      train({ id: 7, serviceDate: '2026-09-09', delayMinutes: 1 }),
    ];
    const summary = computePunctualitySummary(trains, TODAY);
    expect(summary.worstJourneys).toHaveLength(5);
    expect(summary.worstJourneys.map((j) => j.trainId)).toEqual([5, 2, 3, 1, 4]);
  });

  it('a cancelled journey with a leftover delayMinutes can still appear in worstJourneys context but never skews the average', () => {
    // Guard against a regression where cancelled rows get silently folded
    // into the delay/on-time arithmetic via worstJourneys' own sort.
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 5, status: 'completed' }),
        train({ id: 2, serviceDate: '2026-09-10', delayMinutes: 200, status: 'cancelled' }),
      ],
      TODAY,
    );
    expect(summary.avgDelayMinutes).toBe(5);
    expect(summary.cancelledCount).toBe(1);
  });
});
```

- [ ] **Step 7: Run the tests to verify they fail**

Run: `cd frontend && npx vitest run lib/reliabilityDigest.test.ts`
Expected: FAIL — `computePunctualitySummary is not a function`.

- [ ] **Step 8: Implement `computePunctualitySummary`**

Append to `frontend/lib/reliabilityDigest.ts`:

```ts
export interface WorstJourney {
  trainId: number;
  trainUid: string | null;
  serviceDate: string;
  delayMinutes: number;
}

export interface PunctualitySummary {
  eligibleCount: number;
  onTimePct: number | null;
  avgDelayMinutes: number | null;
  cancelledCount: number;
  worstJourneys: WorstJourney[];
}

/** The punctuality half's whole aggregation. Reuses
 * `isEligibleForPunctuality`'s population for every figure -- there is no
 * separate population for `worstJourneys` (spec Decision 2). Cancelled
 * journeys are excluded from the delay/on-time arithmetic entirely and
 * counted separately (spec Decision 1's own extension of DESIGN.md
 * §5.5/§6's "delay rate and cancellation rate are two independent axes"
 * posture) -- a cancelled row's leftover `delayMinutes` never contributes
 * to `avgDelayMinutes` or `onTimePct`, but can still surface in
 * `worstJourneys` (it is still a real recorded delay figure on a real
 * tracked train; excluding it from *both* would silently drop a legitimate
 * "this journey was a mess" data point the user tracked). "On time" is
 * `delayMinutes <= 0`, matching `RowStatusBadge`'s existing convention on
 * this exact page, not `common::Defaults::delay_threshold_minutes` (spec
 * Correction 6). Returns an explicit no-data shape (`null`, never `NaN`
 * dressed up as `0`) when nothing is eligible. */
export function computePunctualitySummary(trains: TrackedTrainListItem[], today: string): PunctualitySummary {
  const eligible = trains.filter((t) => isEligibleForPunctuality(t, today));
  const cancelledCount = eligible.filter((t) => t.status === 'cancelled').length;
  const forArithmetic = eligible.filter((t) => t.status !== 'cancelled');

  const onTimePct =
    forArithmetic.length === 0
      ? null
      : Math.round((forArithmetic.filter((t) => (t.delayMinutes as number) <= 0).length / forArithmetic.length) * 100);

  const avgDelayMinutes =
    forArithmetic.length === 0
      ? null
      : forArithmetic.reduce((sum, t) => sum + (t.delayMinutes as number), 0) / forArithmetic.length;

  const worstJourneys: WorstJourney[] = [...eligible]
    .sort((a, b) => {
      const delayDiff = (b.delayMinutes as number) - (a.delayMinutes as number);
      if (delayDiff !== 0) return delayDiff;
      return b.serviceDate.localeCompare(a.serviceDate);
    })
    .slice(0, 5)
    .map((t) => ({
      trainId: t.id,
      trainUid: t.trainUid,
      serviceDate: t.serviceDate,
      delayMinutes: t.delayMinutes as number,
    }));

  return {
    eligibleCount: eligible.length,
    onTimePct,
    avgDelayMinutes,
    cancelledCount,
    worstJourneys,
  };
}
```

- [ ] **Step 9: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run lib/reliabilityDigest.test.ts`
Expected: PASS (all `isEligibleForPunctuality` + `computePunctualitySummary` tests)

- [ ] **Step 10: Commit**

```bash
cd frontend && git add lib/reliabilityDigest.ts lib/reliabilityDigest.test.ts
git commit -m "feat: add computePunctualitySummary for the reliability digest"
```

## Task 2: `computeDelayRepayRollup` — pure Delay Repay aggregation, counts only

**Files:**
- Modify: `frontend/lib/reliabilityDigest.ts`
- Modify: `frontend/lib/reliabilityDigest.test.ts`

**Interfaces:**
- Consumes: `TicketListItem` (`frontend/lib/types.ts:726-751`) — specifically `trackedTrainId: number | null`, `operator: string | null`, `estimate: DelayRepayEstimate | null` (`scheme: 'DR15' | 'DR30'`, `bandMinutes: number`).
- Produces (used by Task 3):
  ```ts
  export interface DelayRepayRollup {
    attachedTicketsWithOperator: number;
    eligibleCount: number;
    bandCounts: Record<string, number>; // key: `${scheme}-${bandMinutes}`, e.g. "DR15-30"
  }

  export function computeDelayRepayRollup(tickets: TicketListItem[]): DelayRepayRollup;
  ```

- [ ] **Step 1: Write the failing tests**

Append to `frontend/lib/reliabilityDigest.test.ts`:

```ts
function ticket(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: null,
    originCrs: 'KGX',
    destinationCrs: 'YRK',
    originName: null,
    destinationName: null,
    source: 'manual',
    createdAt: '2026-09-09T12:00:00Z',
    serviceDate: '2026-09-10',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinScheduledDeparture: '2026-09-10T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 35,
    estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'estimate disclaimer' },
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: 'route disclaimer',
    customName: null,
    ...overrides,
  };
}

describe('computeDelayRepayRollup', () => {
  it('a standalone ticket (trackedTrainId: null) is excluded from both numerator and denominator', () => {
    const rollup = computeDelayRepayRollup([ticket({ trackedTrainId: null, estimate: null })]);
    expect(rollup.attachedTicketsWithOperator).toBe(0);
    expect(rollup.eligibleCount).toBe(0);
  });

  it('an attached ticket with operator: null is excluded from the denominator, not counted as ineligible', () => {
    const rollup = computeDelayRepayRollup([ticket({ operator: null, estimate: null })]);
    expect(rollup.attachedTicketsWithOperator).toBe(0);
    expect(rollup.eligibleCount).toBe(0);
  });

  it('an attached ticket with a non-null operator but a null estimate counts toward the denominator only', () => {
    const rollup = computeDelayRepayRollup([ticket({ estimate: null })]);
    expect(rollup.attachedTicketsWithOperator).toBe(1);
    expect(rollup.eligibleCount).toBe(0);
  });

  it('bandCounts tallies one ticket in each of the three known bands', () => {
    const rollup = computeDelayRepayRollup([
      ticket({ id: 1, estimate: { scheme: 'DR15', bandMinutes: 15, percentage: 25, disclaimer: 'd' } }),
      ticket({ id: 2, estimate: { scheme: 'DR15', bandMinutes: 30, percentage: 50, disclaimer: 'd' } }),
      ticket({ id: 3, estimate: { scheme: 'DR30', bandMinutes: 60, percentage: 100, disclaimer: 'd' } }),
    ]);
    expect(rollup.attachedTicketsWithOperator).toBe(3);
    expect(rollup.eligibleCount).toBe(3);
    expect(rollup.bandCounts).toEqual({
      'DR15-15': 1,
      'DR15-30': 1,
      'DR30-60': 1,
    });
  });

  it('never produces a currency/percentage total field of any kind', () => {
    const rollup = computeDelayRepayRollup([ticket()]);
    expect(rollup).not.toHaveProperty('totalPercentage');
    expect(rollup).not.toHaveProperty('averagePercentage');
    expect(rollup).not.toHaveProperty('estimatedTotal');
    expect(rollup).not.toHaveProperty('total');
    expect(Object.keys(rollup).sort()).toEqual(['attachedTicketsWithOperator', 'bandCounts', 'eligibleCount']);
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend && npx vitest run lib/reliabilityDigest.test.ts`
Expected: FAIL — `computeDelayRepayRollup is not a function`.

- [ ] **Step 3: Implement `computeDelayRepayRollup`**

Append to `frontend/lib/reliabilityDigest.ts`:

```ts
export interface DelayRepayRollup {
  attachedTicketsWithOperator: number;
  eligibleCount: number;
  bandCounts: Record<string, number>;
}

/** The Delay Repay half's whole aggregation. Population is "tracked trains
 * with an attached ticket that has a non-null operator" -- NOT "all
 * tracked trains" (operator data only reliably exists via an attached
 * ticket; the NR-primary "track this train" flow never captures one, and
 * tracked-train read models carry no operator field at all -- spec
 * Correction 2). A standalone ticket (`trackedTrainId: null`) is excluded
 * from both numerator and denominator: there is no journey yet to have
 * been delayed on. An attached ticket with `operator: null` is excluded
 * from the denominator too, NOT counted as "0% eligible" -- this app has
 * no idea whether that operator would have paid out at all, and folding
 * "we don't know" into the same bucket as "we know and it's zero" would
 * misstate the denominator's own meaning (spec Decision 3).
 *
 * Consumes `TicketListItem.estimate` -- the already-serialized output of
 * `estimate_delay_repay`, computed once server-side by
 * `build_ticket_list_item` -- never calls `estimate_delay_repay` itself
 * (spec Correction 5). Deliberately computes NO average/blended
 * percentage and NO currency total: `percentage` is a percentage of an
 * unknown fare (this app never stores ticket prices), so averaging two
 * different tickets' percentages produces a number with no unit anyone
 * can act on (spec Correction 1, Decision 3). `bandCounts`' keys are
 * `${scheme}-${bandMinutes}` (e.g. "DR15-30") -- a plain count per band
 * actually observed, nothing more. */
export function computeDelayRepayRollup(tickets: TicketListItem[]): DelayRepayRollup {
  const attached = tickets.filter((t) => t.trackedTrainId !== null && t.operator !== null);
  const eligible = attached.filter((t) => t.estimate !== null);

  const bandCounts: Record<string, number> = {};
  for (const t of eligible) {
    const estimate = t.estimate!;
    const key = `${estimate.scheme}-${estimate.bandMinutes}`;
    bandCounts[key] = (bandCounts[key] ?? 0) + 1;
  }

  return {
    attachedTicketsWithOperator: attached.length,
    eligibleCount: eligible.length,
    bandCounts,
  };
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run lib/reliabilityDigest.test.ts`
Expected: PASS (all tests in the file)

- [ ] **Step 5: Commit**

```bash
cd frontend && git add lib/reliabilityDigest.ts lib/reliabilityDigest.test.ts
git commit -m "feat: add computeDelayRepayRollup, counts-per-band only, for the reliability digest"
```

## Task 3: `ReliabilityDigest` component — hedged copy, wired into `/track/mine`

**Files:**
- Create: `frontend/components/ReliabilityDigest.tsx`
- Create: `frontend/components/ReliabilityDigest.test.tsx`
- Modify: `frontend/app/track/mine/page.tsx`

**Interfaces:**
- Consumes: `computePunctualitySummary`, `computeDelayRepayRollup`, `PunctualitySummary`, `DelayRepayRollup`, `WorstJourney` (Tasks 1-2, `frontend/lib/reliabilityDigest.ts`); `TrackedTrainListItem`, `TicketListItem` (`frontend/lib/types.ts`); `londonDayKey` (`frontend/lib/dateFormat.ts`); `formatDate` (`frontend/lib/dateFormat.ts`).
- Produces: `export function ReliabilityDigest({ trains, tickets }: { trains: TrackedTrainListItem[]; tickets: TicketListItem[] })` — rendered once by `frontend/app/track/mine/page.tsx`.

- [ ] **Step 1: Write the failing tests for the zero-data states and hedged copy**

Create `frontend/components/ReliabilityDigest.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ReliabilityDigest } from './ReliabilityDigest';
import type { TrackedTrainListItem, TicketListItem } from '@/lib/types';

// Substring-equal to DelayRepayEstimate.test.tsx's own TOP_LEVEL_DISCLAIMER
// fixture -- this is the one mechanical guard against the two silently
// drifting apart if `delay_repay_rules::ROUTE_DISCLAIMER` is ever
// reworded. See docs/superpowers/specs/2026-09-12-reliability-digest-design.md
// Open questions 1.
const CARRIED_FORWARD_DISCLAIMER = 'not a guarantee of compensation and not proof you travelled';

function train(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-09-01',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinOriginName: 'London Kings Cross',
    pinDestinationName: 'York',
    pinScheduledDeparture: '2026-09-01T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 3,
    trackedAt: '2026-08-30T12:00:00Z',
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

function ticket(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: null,
    originCrs: 'KGX',
    destinationCrs: 'YRK',
    originName: null,
    destinationName: null,
    source: 'manual',
    createdAt: '2026-08-30T12:00:00Z',
    serviceDate: '2026-09-01',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinScheduledDeparture: '2026-09-01T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 35,
    estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'estimate disclaimer' },
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: 'route disclaimer',
    customName: null,
    ...overrides,
  };
}

describe('ReliabilityDigest', () => {
  it('zero eligible punctuality data: renders the check-back-later prose, never a 0%/0-minute figure', () => {
    renderWithMantine(<ReliabilityDigest trains={[]} tickets={[]} />);
    expect(screen.getByText(/check back once it's finished running/)).toBeInTheDocument();
    expect(screen.queryByText(/0%/)).not.toBeInTheDocument();
  });

  it('zero attached-tickets-with-operator: renders the attach-a-ticket prose, never a 0-of-0 figure', () => {
    renderWithMantine(<ReliabilityDigest trains={[]} tickets={[]} />);
    expect(screen.getByText(/Attach a ticket to one of your tracked trains/)).toBeInTheDocument();
    expect(screen.queryByText(/0 of 0/)).not.toBeInTheDocument();
  });

  it('with eligible data: shows an on-time percentage and eligible count', () => {
    renderWithMantine(
      <ReliabilityDigest
        trains={[train({ serviceDate: '2026-09-01', delayMinutes: 0 })]}
        tickets={[]}
      />,
    );
    expect(screen.getByText(/100%/)).toBeInTheDocument();
  });

  it('hedged-copy: carries the disclaimer forward verbatim, the aggregate-specific no-total sentence, no claim-performing language, and no outbound link', () => {
    renderWithMantine(
      <ReliabilityDigest
        trains={[train({ serviceDate: '2026-09-01' })]}
        tickets={[ticket()]}
      />,
    );
    expect(screen.getByText(new RegExp(CARRIED_FORWARD_DISCLAIMER))).toBeInTheDocument();
    expect(screen.getByText(/never stores ticket prices/)).toBeInTheDocument();
    expect(screen.queryByText(/claim now/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/submit/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/get your refund/i)).not.toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('never renders a currency figure ("£") anywhere', () => {
    renderWithMantine(
      <ReliabilityDigest
        trains={[train({ serviceDate: '2026-09-01' })]}
        tickets={[ticket()]}
      />,
    );
    expect(screen.queryByText(/£/)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend && npx vitest run components/ReliabilityDigest.test.tsx`
Expected: FAIL — `ReliabilityDigest.tsx` does not exist yet.

- [ ] **Step 3: Implement `ReliabilityDigest.tsx`**

Create `frontend/components/ReliabilityDigest.tsx`:

```tsx
import { Alert, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { computeDelayRepayRollup, computePunctualitySummary } from '@/lib/reliabilityDigest';
import { londonDayKey, formatDate } from '@/lib/dateFormat';
import type { TrackedTrainListItem, TicketListItem } from '@/lib/types';

/** New retrospective summary card for `/track/mine`, per
 * docs/superpowers/specs/2026-09-12-reliability-digest-design.md. Takes the
 * exact same two already-fetched arrays the rest of the page renders from
 * -- no fetch of its own, no new `Promise.all` member (Decision 5).
 * `today` is computed here, once per render, from `londonDayKey(new
 * Date())` -- this page already sets `revalidate = 0` for the same
 * "don't let this go stale mid-day" reason (see page.tsx's own comment on
 * that export). */
export function ReliabilityDigest({ trains, tickets }: { trains: TrackedTrainListItem[]; tickets: TicketListItem[] }) {
  const today = londonDayKey(new Date());
  const punctuality = computePunctualitySummary(trains, today);
  const rollup = computeDelayRepayRollup(tickets);

  return (
    <Card withBorder>
      <Stack gap="md">
        <Title order={2}>Your reliability</Title>
        <PunctualitySection summary={punctuality} />
        <DelayRepaySection rollup={rollup} />
      </Stack>
    </Card>
  );
}

function PunctualitySection({ summary }: { summary: ReturnType<typeof computePunctualitySummary> }) {
  if (summary.eligibleCount === 0) {
    return (
      <Text size="sm" c="dimmed">
        Track a train and check back once it&apos;s finished running to see your punctuality here.
      </Text>
    );
  }

  return (
    <Stack gap={4}>
      <Text>
        Of your last {summary.eligibleCount} tracked journey{summary.eligibleCount === 1 ? '' : 's'} with a
        recorded outcome, {summary.onTimePct}% were on time
        {summary.avgDelayMinutes !== null && ` (average delay ${summary.avgDelayMinutes.toFixed(1)} minutes)`}.
        {summary.cancelledCount > 0 &&
          ` ${summary.cancelledCount} journey${summary.cancelledCount === 1 ? ' was' : 's were'} cancelled and ${summary.cancelledCount === 1 ? "isn't" : "aren't"} counted in that figure.`}
      </Text>
      {summary.worstJourneys.length > 0 && (
        <Stack gap={2}>
          <Text size="sm" fw={500}>
            Your most delayed tracked journeys:
          </Text>
          {summary.worstJourneys.map((journey) => {
            const href = journey.trainUid
              ? `/train/${journey.trainUid}/${journey.serviceDate}`
              : `/train/by-id/${journey.trainId}`;
            return (
              <Group key={journey.trainId} gap="xs">
                <Link href={href}>{formatDate(journey.serviceDate)}</Link>
                <Text size="sm" c="dimmed">
                  {journey.delayMinutes}m late
                </Text>
              </Group>
            );
          })}
        </Stack>
      )}
    </Stack>
  );
}

// Human-readable labels for the known bands this app's own
// `delay_repay_rules.rs` ever produces -- `dr15_band`/`dr30_band` only ever
// return 15/30/60 as `bandMinutes`, so this table is exhaustive against
// today's rules, not a guess. An unlisted key (a future new band) falls
// back to the raw key itself, so a rules change never disappears silently.
const BAND_LABELS: Record<string, string> = {
  'DR15-15': '15–29 minute delays (25% of fare)',
  'DR15-30': '30–59 minute delays (50% of fare)',
  'DR15-60': '60+ minute delays (100% of fare)',
  'DR30-30': '30–59 minute delays (50% of fare)',
  'DR30-60': '60+ minute delays (100% of fare)',
};

function DelayRepaySection({ rollup }: { rollup: ReturnType<typeof computeDelayRepayRollup> }) {
  if (rollup.attachedTicketsWithOperator === 0) {
    return (
      <Text size="sm" c="dimmed">
        Attach a ticket to one of your tracked trains to see whether any of your journeys may have qualified for
        Delay Repay.
      </Text>
    );
  }

  const bandEntries = Object.entries(rollup.bandCounts);

  return (
    <Stack gap={4}>
      <Alert color="blue" title="Possible Delay Repay eligibility, across your tracked journeys" variant="light">
        Of the {rollup.attachedTicketsWithOperator} tracked journey
        {rollup.attachedTicketsWithOperator === 1 ? '' : 's'} with a ticket attached, {rollup.eligibleCount} may
        have qualified for a partial or full refund of that journey&apos;s fare under the operator&apos;s Delay
        Repay scheme.
      </Alert>
      {bandEntries.length > 0 && (
        <Stack gap={2}>
          {bandEntries.map(([key, count]) => (
            <Text key={key} size="sm">
              {count} journey{count === 1 ? '' : 's'} — {BAND_LABELS[key] ?? key}
            </Text>
          ))}
        </Stack>
      )}
      <Text size="sm">
        This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled
        — the same estimate already shown against each ticket below, just added up. It is a count of journeys,
        not a total amount: this app never stores ticket prices, so it has no fare figure to add up into a total
        refund value, and never will. Always verify eligibility and submit any claim directly with each operator
        — this app never submits a claim on your behalf, for one ticket or for all of them at once.
      </Text>
    </Stack>
  );
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run components/ReliabilityDigest.test.tsx`
Expected: PASS (all 6 tests)

- [ ] **Step 5: Commit the component**

```bash
cd frontend && git add components/ReliabilityDigest.tsx components/ReliabilityDigest.test.tsx
git commit -m "feat: add ReliabilityDigest component with hedged Delay Repay rollup copy"
```

- [ ] **Step 6: Write the failing test asserting the digest renders on `/track/mine`**

Find the existing page test file and add a case. First check whether it exists:

```bash
ls /workspaces/github-com-fasterspeeding-network-rail-status/frontend/app/track/mine/page.test.tsx
```

If it exists, add this test case to its existing `describe` block (matching its existing mocking pattern for `getMyTrackedTrains`/`getMyTickets` — read the file first to match its exact mock setup before writing the addition, since this plan cannot predict its exact existing structure). If it does not exist, create it following `frontend/app/stations/[crs]/page.test.tsx`'s structure for a Server Component test (render the awaited JSX returned by the async page function). At minimum, the new/added test must be:

```tsx
it('renders the reliability digest card when there is something to show', async () => {
  // Arrange mocks so getMyTrackedTrains()/getMyTickets() resolve to a
  // non-empty array each, matching this file's own existing mocking
  // pattern for those two functions.
  const page = await MyTrackedTrainsPage();
  renderWithMantine(page);
  expect(screen.getByText('Your reliability')).toBeInTheDocument();
});

it('does not render the digest card when nothingToShow (empty state)', async () => {
  // Arrange mocks so both resolve to [].
  const page = await MyTrackedTrainsPage();
  renderWithMantine(page);
  expect(screen.queryByText('Your reliability')).not.toBeInTheDocument();
});
```

- [ ] **Step 7: Run the page test to verify the new cases fail**

Run: `cd frontend && npx vitest run app/track/mine/page.test.tsx`
Expected: FAIL — `ReliabilityDigest`/"Your reliability" not found (component not wired in yet).

- [ ] **Step 8: Wire `ReliabilityDigest` into `frontend/app/track/mine/page.tsx`**

Add the import near the other component imports (after the `TicketSummary` import, `page.tsx:6`):

```tsx
import { ReliabilityDigest } from '@/components/ReliabilityDigest';
```

Then modify the return block (`page.tsx:78-122`) to render it between the header `Group` and the `nothingToShow` conditional:

```tsx
  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <Group gap="md">
          <TextLink href="/track">Track a new train</TextLink>
          <TextLink href="/track/mine/add-ticket">Add a ticket</TextLink>
        </Group>
      </Group>
      {!nothingToShow && <ReliabilityDigest trains={trains} tickets={tickets ?? []} />}
      {nothingToShow ? (
        <Text c="dimmed">
```

(The rest of the existing conditional body — the `Link` to `/track` and the trains/tickets lists — stays byte-for-byte unchanged; only the new line above the ternary is added.)

- [ ] **Step 9: Run the page test to verify it passes**

Run: `cd frontend && npx vitest run app/track/mine/page.test.tsx`
Expected: PASS

- [ ] **Step 10: Run the full frontend test suite to check for regressions**

Run: `cd frontend && npm test`
Expected: PASS, no regressions in any other suite.

- [ ] **Step 11: Commit**

```bash
cd frontend && git add app/track/mine/page.tsx app/track/mine/page.test.tsx
git commit -m "feat: wire ReliabilityDigest into /track/mine"
```

---

## Self-Review

**1. Spec coverage:**
- Correction 1 / "no currency total" → Task 2's `computeDelayRepayRollup` returns only `attachedTicketsWithOperator`/`eligibleCount`/`bandCounts` (no percentage/amount field), guarded by Task 2 Step 1's "never produces a currency/percentage total field of any kind" test (asserts the exact key set) and Task 3's "never renders a currency figure" test (asserts no `£` anywhere in rendered output).
- Correction 2 / narrowed eligibility population → Task 2's population is exactly `trackedTrainId !== null && operator !== null`, with explicit tests for the standalone-ticket exclusion and the null-operator-excluded-from-denominator case.
- Correction 6 / on-time definition → Task 1's `computePunctualitySummary` uses `delayMinutes <= 0`, with an explicit test naming the 5-minute threshold it must NOT use ("on-time uses delayMinutes <= 0, matching RowStatusBadge, not a 5-minute threshold").
- Decision 1 (population, cancelled-exclusion) → Task 1, `isEligibleForPunctuality` + `computePunctualitySummary`'s `forArithmetic` split, with dedicated tests.
- Decision 2 (worst 5, tie-break) → Task 1's `worstJourneys` sort + test.
- Decision 3 (rollup population, no average) → Task 2.
- Decision 4 (hedged copy, no claim link, no second disclaimer) → Task 3's component + hedged-copy test (disclaimer substring, no-total sentence, no claim-performing language, no `<a>` rendered).
- Decision 5 (placement, gating on `nothingToShow`, no new fetch) → Task 3 Steps 6-9.
- Decision 6 (naming: `lib/reliabilityDigest.ts`, `components/ReliabilityDigest.tsx`) → matches exactly.
- Testing approach's "backend: none needed" → stated explicitly in Global Constraints.

**2. Placeholder scan:** No "TBD"/"add appropriate handling"/"similar to Task N" phrasing anywhere; every step carries complete, runnable code. Task 3 Step 6 is the one step that says "read the file first" rather than giving fixed code — this is intentional, not a placeholder: the plan cannot predict the exact existing mock structure of a file it hasn't confirmed exists, but it gives the exact two test bodies that must be added regardless of how the surrounding mocks are wired.

**3. Type consistency:** `PunctualitySummary`, `WorstJourney`, `DelayRepayRollup` are defined once in Task 1/2 and referenced identically (same field names: `eligibleCount`, `onTimePct`, `avgDelayMinutes`, `cancelledCount`, `worstJourneys`, `attachedTicketsWithOperator`, `bandCounts`) in Task 3's component and tests — no renamed field anywhere. Function names (`isEligibleForPunctuality`, `computePunctualitySummary`, `computeDelayRepayRollup`) match the spec's own Decision 6 naming and are used consistently across all three tasks. `TrackedTrainListItem`/`TicketListItem` field names in every test fixture (`resolutionStatus`, `delayMinutes`, `serviceDate`, `status`, `trainUid`, `trackedTrainId`, `operator`, `estimate`) are copied verbatim from `frontend/lib/types.ts:541-751`.

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-09-12-reliability-digest.md`. Two execution options:

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
