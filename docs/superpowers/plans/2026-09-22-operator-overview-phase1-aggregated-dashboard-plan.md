# Plan: Operator Overview Phase 1 — All-Lines Aggregated Dashboard

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement section **A** of
`docs/superpowers/specs/2026-09-22-operator-overview-design.md` (the approved
spec) — a new `/status` page giving a single, glanceable "network health"
view: five severity-bucket counter tiles (each linking to `/lines`
pre-filtered to that bucket), an unbounded worst-lines-first list, and a
mode/country breakdown. This is **Phase 1 of 4** in that spec's own phased
delivery plan; Phases 2 (per-line drill-down), 3 (operators list + pinning)
and 4 (historical views) are out of scope here and are being planned/executed
separately — see Non-goals.

**Architecture:** entirely frontend, entirely additive, no backend/API
changes (verified below, not just assumed from the spec). The whole feature
is a client-side reduce over the same `GET /Line/Mode/{mode}/Status` payload
`app/page.tsx` and `app/lines/page.tsx` already fetch — no new endpoint, no
new wire type. Build order:

1. **`lib/severity.ts`** (Task 1) — the spec says to reuse this file's
   `SeverityGroup`/`GROUP_RANK`, but as written today neither is actually
   `export`ed (verified — see Judgment Call 2). This task exports them and
   adds the two small accessors every later task needs: `severityGroup()`
   (a status number → its bucket) and `isSeverityGroup()` (a query-string
   value → a validated bucket), plus a single-sourced label/order table so
   the dashboard's tiles and `AllLinesTable`'s new filter chips can never
   say something different for the same bucket.
2. **`lib/networkStatusOverview.ts`** (Task 2, new file) — a pure,
   independently-tested module that takes the already-fetched
   `LineStatusReport[]` and produces bucket counts, the unbounded
   worst-first list, and the mode/country breakdowns. No React, no fetch —
   mirrors `lib/sampleStats.ts`'s own "pure derivation, `*.test.ts`
   alongside it" shape.
3. **`app/status/page.tsx`** (Task 3, new page) — a thin Server Component:
   fetch the reports (same `withStaleFallback` cache key `app/page.tsx` and
   `app/lines/page.tsx` already use, so the entry is shared), call Task 2's
   builder, render the five tiles + list + breakdown. Page-local render
   functions only (no new `components/` file), mirroring `app/page.tsx`'s
   own `RightNowModule`, which is deliberately local until "a third page
   wants it."
4. **`AllLinesTable.tsx`** (Task 4) — add a fourth, AND-combined filter
   dimension (status group), seeded from an optional `initialStatusGroup`
   prop, so a tile's link actually narrows the table it points at.
5. **`app/lines/page.tsx`** (Task 5) — accept `searchParams`, parse and
   validate a `?statusGroup=` value, pass it through as the new prop.
6. **Nav integration** (Task 6) — add `/status` to
   `PRIMARY_NAV_DESTINATIONS` so the page is actually discoverable, plus the
   two hardcoded label-list updates and the careful re-verification
   `e2e/nav.spec.ts`'s own pixel-exact layout assertions require (see
   Judgment Call 5 — this is the one place in this plan with real
   regression risk that isn't caught by `vitest`/`tsc`/`next build` alone).
7. **Full-suite verification** (Task 7).

**Tech stack:** Next.js 16 (App Router, Server Components), React 19,
Mantine 9, TypeScript, Vitest + Testing Library, Playwright (e2e, not
CI-gated — see Global Constraints).

**Spec:** `docs/superpowers/specs/2026-09-22-operator-overview-design.md`,
section **A** — authoritative for the architectural decision that this is
frontend-only with no new backend endpoint. This plan does not re-litigate
that; it only resolves what the spec explicitly left open for "the
implementation plan" (route location, exact bucket vocabulary/labels, query
param shape, nav placement).

---

## Judgment calls this plan makes (read before Task 1)

1. **Route: `frontend/app/status/page.tsx`, not a redesigned `/lines`
   header.** The spec named both as options. `AllLinesTable.tsx` already
   carries four independent concerns (name search, operator filter, country
   filter, four-way sort) across a 125-row table; folding a network-wide
   dashboard into its header would grow that file's scope rather than give
   the dashboard its own reviewable surface. A dedicated route also matches
   how every other top-level feature area in this app is delivered
   (`/lines`, `/stations`, `/trains`, `/incidents` are each their own route,
   reachable from nav) — see Task 6.

2. **`SeverityGroup`/`GROUP_RANK` are not exported today — this plan exports
   them rather than inventing new vocabulary.** Verified directly:
   `frontend/lib/severity.ts:3` (`type SeverityGroup = ...`) and
   `frontend/lib/severity.ts:48` (`const GROUP_RANK: Record<SeverityGroup,
   number> = ...`) both lack the `export` keyword — only the five functions
   in that file (`severityColor`, `isGoodSeverity`, `severityLabel`,
   `severityRank`, `worstStatus`) are exported, confirmed by
   `frontend/lib/severity.test.ts:2`'s import line, which imports only those
   four testable functions and never the type or the const. So the spec's
   "reusing `severity.ts`'s `SeverityGroup`/`GROUP_RANK`" is correct about
   *where the concept lives* but not yet true about *what's reachable from
   outside the file* — Task 1 closes that gap by adding `export` to both,
   plus a `severityGroup(severity): SeverityGroup` accessor (the
   bucket-membership counterpart of the already-exported `severityRank`),
   since every consumer this plan adds needs to know which bucket a status
   falls into, not just how two statuses rank against each other.

3. **Bucket vocabulary and labels come from the actual `SeverityGroup`
   values (`good`/`informational`/`planned`/`mild`/`severe`), not from the
   five-bucket wording ("Good / Minor / Severe / Suspended-or-worse /
   Planned") used to describe this task.** Those two lists don't match:
   `severity.ts` has no `'suspended-or-worse'` group (`Suspended`,
   `Severe Delays`, `Rail Replacement`, `Diverted`, etc. are all one
   `'severe'` bucket — see `SEVERITY_TABLE`, `frontend/lib/severity.ts:5-32`),
   and it does have an `'informational'` group (Special Service, Exit Only,
   No Step Free Access, TfL Service Closed/Information) that the task's
   paraphrase omits entirely. Per this task's own framing, the spec (which
   explicitly says "the five severity buckets already defined in
   `severity.ts`'s `SeverityGroup`") is authoritative over a prose
   description of it — so this plan builds five tiles for the five real
   groups, with display labels chosen to read naturally: **Good Service,
   Informational, Planned, Minor Disruption, Severe Disruption** (Task 1's
   `SEVERITY_GROUP_LABELS`).

4. **Query param: `?statusGroup=<good|informational|planned|mild|severe>`,
   one-directional only.** A tile links to `/lines?statusGroup=severe`;
   `AllLinesTable` reads it once, as an initial `useState` seed (Task 4),
   exactly mirroring `IncidentSearchForm`'s own `initialOperator`/
   `initialLine`/`initialFrom`/`initialTo` props
   (`frontend/components/IncidentSearchForm.tsx:108-138`) — a prop consumed
   only at construction time, never re-read after mount. This plan does
   **not** add two-way URL sync for `AllLinesTable`'s pre-existing operator/
   country/name/sort filters — that would be a much larger change to a
   component this plan is trying to touch minimally, and the spec's own
   §A only asks for the counters to *link somewhere pre-filtered*, not for
   `/lines` to become fully shareable-by-URL.

5. **`app/page.tsx` is left untouched.** Its `notGoodServiceSummary`
   (`frontend/app/page.tsx:144-156`) and `RightNowModule`
   (`frontend/app/page.tsx:549-625`) are page-local (unexported) functions
   embedded in an 819-line file with its own large test suite
   (`app/page.test.tsx`). This plan's new `lib/networkStatusOverview.ts`
   (Task 2) reimplements the same worst-first sort as a small, independently
   tested, exported function, rather than reaching into `app/page.tsx` to
   extract and share it — refactoring that file is a real but separate
   cleanup opportunity, deliberately left for later so this phase doesn't
   carry regression risk on a page outside its own scope.

6. **"By country" breakdown self-hides below two countries present; "by
   mode" does not.** `AllLinesTable.tsx` already establishes this exact
   self-hiding precedent for its own country filter (`countryOptions.length
   > 1` gate, `frontend/app/lines/AllLinesTable.tsx:317`) because
   `lib/modes.ts`'s `MODE_TO_COUNTRY` table is deliberately empty today (see
   that file's own doc comment) — every report is `'Gb'` until a real
   non-GB poller exists. The dashboard's country breakdown mirrors that:
   rendered only once `Object.keys(overview.byCountry).length > 1`. The mode
   breakdown (National Rail vs TfL) always renders, since both buckets are
   real and populated today.

7. **The worst-first list excludes `'good'`-group lines, matching
   `notGoodServiceSummary`'s own scope** (`frontend/app/page.tsx:144-156`
   filters to `severityRank(...) > severityRank(10)`). "Unbounded" removes
   `RIGHT_NOW_LIMIT`'s five-row cap (`frontend/app/page.tsx:134`), not the
   good-service exclusion — a list of ~140 lines, most saying "Good
   Service," is not what "worst-lines-first" is asking for.

8. **Custom lines are included in the counts/list exactly as they are
   today, with no special-casing added.** `getLineStatusForMode` already
   forwards the caller's session cookie (`frontend/lib/api.ts:96-102`,
   `cookieForwardInit`), so a logged-in caller's own private custom lines
   are already included in `allReports` for both `app/page.tsx` and
   `app/lines/page.tsx`. `/status` fetches the same way and inherits the
   same scoping — this plan does not add or remove anything here, it only
   preserves existing behavior.

9. **Nav placement uses the label "Status", not "Network Status" or
   "Network Health".** `components/AppNavBar.tsx`'s own doc comment
   (`frontend/components/AppNavBar.tsx:90-131`) records measured slack
   budgets as tight as **42px** at the `md` (992px) breakpoint for the
   logged-in bar, and `e2e/nav.spec.ts` asserts an *exact* pixel height
   (`toBe(61)`) at 1440px and 390px plus a single-row layout at 992/991px —
   this is real, previously-fixed layout fragility, not theoretical. A
   short single-word label minimizes the risk of tipping the bar back into
   wrapping; Task 6 still requires a real-browser re-verification pass
   rather than assuming the shorter label is automatically safe.

---

## Non-goals

- **No backend/API changes of any kind.** Confirmed by reading
  `frontend/lib/api.ts` in full: `getLineStatusForMode` already returns
  every line's current report in one call, and nothing this plan needs is
  missing from it. The spec's own §A explicitly frames a server-computed
  `GET /public/status/overview` as optional/not-required for Phase 1 — this
  plan does not add it.
- **Phases 2, 3 and 4 of the spec** (per-line drill-down completion,
  operators list + pinning, historical/trend views) — out of scope, planned
  and executed separately. If a later phase needs something from this one
  (e.g. Phase 3's operator rollup reusing `lib/networkStatusOverview.ts`'s
  bucketing), that is a forward dependency for that phase's own plan to
  state, not something wired up here.
- **No two-way URL sync of `AllLinesTable`'s pre-existing filters**
  (operator, country, name, sort) — see Judgment Call 4.
- **No refactor of `app/page.tsx`** (`notGoodServiceSummary`/
  `RightNowModule`) — see Judgment Call 5.
- **No new `components/` file** for the dashboard's tiles/rows — page-local
  functions in `app/status/page.tsx`, matching `RightNowModule`'s own
  documented precedent for staying local until a second page wants it.
- **No changes to `StatusBadge`, `StatusRow`, `LineStatusCard`, `IssueList`,
  or `lib/sampleStats.ts`** — none of them are touched or needed by this
  page (the dashboard shows counts and worst-status labels via
  `severityLabel`, not per-line sample-stat summaries).
- **No changes to `crates/*`** (Rust backend) — this is a frontend-only
  plan, confirmed necessary by Task 0 below.

## Global Constraints

- **File scope.** Modified:
  `frontend/lib/severity.ts`,
  `frontend/lib/severity.test.ts`,
  `frontend/app/lines/AllLinesTable.tsx`,
  `frontend/app/lines/AllLinesTable.test.tsx`,
  `frontend/app/lines/page.tsx`,
  `frontend/app/lines/page.test.tsx`,
  `frontend/lib/navLinks.ts`,
  `frontend/e2e/nav.spec.ts`.
  New:
  `frontend/lib/networkStatusOverview.ts`,
  `frontend/lib/networkStatusOverview.test.ts`,
  `frontend/app/status/page.tsx`,
  `frontend/app/status/page.test.tsx`.
  No other file changes, and specifically no `crates/*` changes.
- **Testing (this repo's actual CI, `.github/workflows/ci.yml`'s `frontend`
  job, lines 246-272).** From `frontend/`:
  - `npx tsc --noEmit` (the strict typecheck CI runs — there is no eslint
    config in this repo, so this *is* "lint" here).
  - `npm test` (vitest run) for the full suite; `npm test -- <path>` for a
    single changed file while iterating.
  - `npm run build` (`next build`, plus this repo's
    `scripts/stamp-sw-version.mjs` postbuild step) before considering the
    frontend tasks done.
  - **`npx playwright test e2e/nav.spec.ts` is NOT part of CI** — confirmed
    by reading `.github/workflows/ci.yml` in full: the `frontend` job has no
    `test:e2e` step (its own comment, lines 241-245, says so explicitly).
    Task 6 still requires running it manually against a real dev server,
    because that spec is the only thing in this repo that would catch a
    nav-bar wrap regression, and this plan is knowingly touching the exact
    array (`PRIMARY_NAV_DESTINATIONS`) that layout was tuned against.
- **Do not touch `navHeight`'s expected pixel values (`61`) in
  `e2e/nav.spec.ts`** to make a test pass — if adding the nav link causes a
  wrap, the fix is a shorter label or different placement, not editing the
  assertion. That number is a real, previously-measured invariant (see
  `components/AppNavBar.tsx:30-58`), not an arbitrary constant.

---

## Task 0: Confirm no backend change is needed (verification only, no diff)

Already done as part of writing this plan, recorded here so the claim is
checked rather than assumed:

- [x] Read `frontend/lib/api.ts` in full. `getLineStatusForMode(mode)`
  (`frontend/lib/api.ts:96-102`) calls `GET /Line/Mode/{mode}/Status` and
  returns `LineStatusReport[]` — every line's full current status, exactly
  the payload `app/page.tsx` and `app/lines/page.tsx` already fetch via the
  identical call. Nothing this plan needs (severity, mode, operators,
  country-derivable `modeName`) is missing from that type
  (`frontend/lib/types.ts:143-158`).
- [x] Read `frontend/lib/severity.ts`, `frontend/lib/modes.ts` in full —
  `severityRank`/`worstStatus`/`countryForReport`/`DISPLAYED_MODES_PARAM`/
  `MERGED_TFL_LINE_IDS` all exist and do exactly what the spec says.
- [x] Read `frontend/app/lines/AllLinesTable.tsx` and
  `frontend/app/lines/page.tsx` in full — confirmed the spec's claim that
  `AllLinesTable`'s filter state is local `useState`
  (`frontend/app/lines/AllLinesTable.tsx:149-156`) and that
  `AllLinesPage` takes no `searchParams` today
  (`frontend/app/lines/page.tsx:76`) — both addressed by Tasks 4-5.
- [x] Confirmed no existing "stat tile" component exists under
  `frontend/components/` (directory listing checked) — Task 3 builds tiles
  as page-local JSX rather than reaching for a nonexistent shared primitive.

No further action for this task; it exists only to record the check.

---

## Task 1: `lib/severity.ts` — export the group vocabulary, add accessors

**Files:** modify `frontend/lib/severity.ts`, `frontend/lib/severity.test.ts`.

Independent, first task. Nothing else in this plan compiles without it.

- [ ] **Step 1: Export the existing type and const.** Change
  `frontend/lib/severity.ts:3`:

```ts
export type SeverityGroup = 'good' | 'informational' | 'planned' | 'mild' | 'severe';
```

  and `frontend/lib/severity.ts:48`:

```ts
export const GROUP_RANK: Record<SeverityGroup, number> = {
```

  (Both statements are otherwise unchanged — this is purely adding the
  `export` keyword to each, per Judgment Call 2. No other line in either
  declaration changes.)

- [ ] **Step 2: Add `severityGroup`, directly below the existing
  `severityRank` function** (after `frontend/lib/severity.ts:78`, before
  `worstStatus`):

```ts
/** The `SeverityGroup` a raw severity number belongs to (see
 * `SEVERITY_TABLE` above) -- the bucket-membership counterpart of
 * `severityRank`'s numeric ordering. Added for the network-wide dashboard's
 * five-bucket breakdown (`lib/networkStatusOverview.ts`) and
 * `AllLinesTable`'s own status-group filter, both of which need to know
 * WHICH group a status falls into, not just how it ranks against another
 * one. Same unknown-severity fallback as `severityRank`: an unrecognized
 * number is treated as `'informational'`. */
export function severityGroup(severity: number): SeverityGroup {
  return SEVERITY_TABLE[severity]?.group ?? 'informational';
}
```

- [ ] **Step 3: Add the shared label table and rank-ordered list**, directly
  below `GROUP_RANK` (after its closing brace, before `severityColor`):

```ts
/** Display copy for each `SeverityGroup`, single-sourced so the network
 * dashboard's counter tiles (`app/status/page.tsx`) and `AllLinesTable`'s
 * status-group filter chips can never say something different for the same
 * bucket. */
export const SEVERITY_GROUP_LABELS: Record<SeverityGroup, string> = {
  good: 'Good Service',
  informational: 'Informational',
  planned: 'Planned',
  mild: 'Minor Disruption',
  severe: 'Severe Disruption',
};

/** Every `SeverityGroup`, ordered best-to-worst by `GROUP_RANK` -- the
 * iteration order for the dashboard's five counter tiles and
 * `AllLinesTable`'s filter chips, so both render in one deliberate order
 * rather than relying on object-key iteration order. */
export const SEVERITY_GROUPS_BY_RANK: readonly SeverityGroup[] = [
  'good',
  'informational',
  'planned',
  'mild',
  'severe',
];
```

- [ ] **Step 4: Add `isSeverityGroup`, a query-param type guard**, directly
  below `SEVERITY_GROUPS_BY_RANK`:

```ts
/** Narrows an untyped query-string value (e.g. `/lines?statusGroup=severe`)
 * to a real `SeverityGroup`, or `false` for anything else -- an unknown/
 * missing/malformed value must fall back to "no filter" rather than being
 * silently treated as some specific bucket. Used by `app/lines/page.tsx` to
 * validate `searchParams.statusGroup` before handing it to `AllLinesTable`
 * as `initialStatusGroup`. */
export function isSeverityGroup(value: string | undefined): value is SeverityGroup {
  return value !== undefined && (SEVERITY_GROUPS_BY_RANK as readonly string[]).includes(value);
}
```

- [ ] **Step 5: Add unit tests**, in `frontend/lib/severity.test.ts` (new
  `describe` blocks, alongside the existing ones — add `severityGroup` and
  `isSeverityGroup` to the top import line, `frontend/lib/severity.test.ts:2`):

```ts
import { isGoodSeverity, isSeverityGroup, severityColor, severityGroup, severityLabel, worstStatus } from './severity';
```

```ts
describe('severityGroup', () => {
  it('groups every NR severity the same way severityColor already does', () => {
    expect(severityGroup(10)).toBe('good');        // Good Service
    expect(severityGroup(9)).toBe('mild');          // Minor Delays
    expect(severityGroup(7)).toBe('mild');          // Reduced Service
    expect(severityGroup(2)).toBe('severe');        // Suspended
    expect(severityGroup(21)).toBe('severe');       // Diverted
    expect(severityGroup(4)).toBe('planned');       // Planned Closure
    expect(severityGroup(0)).toBe('informational'); // Special Service
  });

  it('groups the five TfL-only codes the same way severityColor already does', () => {
    expect(severityGroup(25)).toBe('good');          // No Issues
    expect(severityGroup(24)).toBe('mild');          // Issues Reported
    expect(severityGroup(23)).toBe('severe');        // Not Running
    expect(severityGroup(22)).toBe('informational'); // Service Closed
    expect(severityGroup(26)).toBe('informational'); // Information
  });

  it('falls back to informational for an unrecognized value, matching severityRank', () => {
    expect(severityGroup(999)).toBe('informational');
  });
});

describe('isSeverityGroup', () => {
  it('accepts every real SeverityGroup value', () => {
    for (const group of ['good', 'informational', 'planned', 'mild', 'severe']) {
      expect(isSeverityGroup(group)).toBe(true);
    }
  });

  it('rejects an unrecognized string', () => {
    expect(isSeverityGroup('extremely-bad')).toBe(false);
  });

  it('rejects undefined (no query param at all)', () => {
    expect(isSeverityGroup(undefined)).toBe(false);
  });
});
```

- [ ] **Step 6: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- lib/severity.test.ts
```

  Expected: typecheck clean, all `severity.test.ts` tests (existing plus new)
  pass.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/severity.ts frontend/lib/severity.test.ts
git commit -m "frontend: export SeverityGroup/GROUP_RANK, add severityGroup/isSeverityGroup and shared bucket labels"
```

---

## Task 2: `lib/networkStatusOverview.ts` — pure bucketing/sorting logic (new file)

**Files:** create `frontend/lib/networkStatusOverview.ts`,
`frontend/lib/networkStatusOverview.test.ts`.

Depends on Task 1 (`severityGroup`, `SeverityGroup`). Depends on the existing
`frontend/lib/modes.ts` (`MERGED_TFL_LINE_IDS`, `countryForReport`, `Country`)
and `frontend/lib/severity.ts` (`severityRank`, `worstStatus`) — no changes
needed to either, both already export everything used here.

- [ ] **Step 1: Write the file**

```ts
import type { LineStatusReport } from './types';
import { severityGroup, severityRank, worstStatus, type SeverityGroup } from './severity';
import { countryForReport, MERGED_TFL_LINE_IDS, type Country } from './modes';

/** Drops the TfL ids that are folded into their National Rail
 * counterpart's own row everywhere a line list is built directly from
 * reports rather than from `/public/lines` (`MERGED_TFL_LINE_IDS`'s own doc
 * comment) -- the exact same exclusion `app/page.tsx`'s
 * `notGoodServiceSummary` already applies, so this dashboard's counts never
 * double-count the same real-world line (e.g. the Elizabeth line reported
 * once under `tfl-elizabeth` AND once under its NR-merged id). */
function realReports(reports: LineStatusReport[]): LineStatusReport[] {
  return reports.filter((report) => !MERGED_TFL_LINE_IDS.includes(report.id));
}

/** Worst-first, then alphabetical -- the exact comparator
 * `notGoodServiceSummary` (`app/page.tsx:144-156`) already uses for its own
 * "Right now" list, reimplemented here as a small, independently-tested,
 * exported function rather than reaching into that page-local one (see this
 * plan's Judgment Call 5 for why `app/page.tsx` itself is left untouched). */
function compareWorstFirst(a: LineStatusReport, b: LineStatusReport): number {
  const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
  return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
}

export interface NetworkStatusOverview {
  /** How many lines fall into each severity bucket right now (deduplicated
   * per Task 2's `realReports`). */
  counts: Record<SeverityGroup, number>;
  /** Every line's report, deduplicated. Not surfaced directly by the
   * dashboard, but used to derive `counts`/`byMode`/`byCountry` and offered
   * here so a caller can compute a total without re-deriving the same
   * exclusion. */
  totalLines: number;
  /** Every affected line (i.e. not in the `'good'` bucket), worst-first
   * then alphabetical -- unbounded, unlike `app/page.tsx`'s
   * `RIGHT_NOW_LIMIT`-capped list (see this plan's Judgment Call 7). */
  worstFirst: LineStatusReport[];
  /** National Rail vs TfL, folding all five TfL-published modes
   * (`tube`/`dlr`/`overground`/`elizabeth-line`/`tram`) into one bucket --
   * mirrors `AllLinesTable`'s own "TfL" operator-filter folding
   * (`expandOperatorForFiltering`), applied to mode instead of operator. */
  byMode: { nationalRail: LineStatusReport[]; tfl: LineStatusReport[] };
  /** `countryForReport`'s three jurisdictions, present as a key only for a
   * country this snapshot actually has a line in -- so a caller doesn't
   * have to invent copy for an always-empty bucket (today, in practice,
   * always just `{ Gb: [...] }`, since `MODE_TO_COUNTRY` is still empty --
   * see `lib/modes.ts`'s own doc comment). */
  byCountry: Partial<Record<Country, LineStatusReport[]>>;
}

/** Builds the whole network-status dashboard's data from an already-fetched
 * `GET /Line/Mode/{mode}/Status` response -- the same payload
 * `app/page.tsx` and `app/lines/page.tsx` already fetch, no new endpoint.
 * Pure and synchronous: no fetch, no React, so `app/status/page.tsx` (Task
 * 3) is a thin Server Component that just calls this and renders the
 * result. */
export function buildNetworkStatusOverview(reports: LineStatusReport[]): NetworkStatusOverview {
  const real = realReports(reports);

  const counts: Record<SeverityGroup, number> = {
    good: 0,
    informational: 0,
    planned: 0,
    mild: 0,
    severe: 0,
  };
  const byMode: NetworkStatusOverview['byMode'] = { nationalRail: [], tfl: [] };
  const byCountry: NetworkStatusOverview['byCountry'] = {};

  for (const report of real) {
    const group = severityGroup(worstStatus(report).statusSeverity);
    counts[group] += 1;

    if (report.modeName === 'national-rail') {
      byMode.nationalRail.push(report);
    } else {
      byMode.tfl.push(report);
    }

    const country = countryForReport(report);
    (byCountry[country] ??= []).push(report);
  }

  const worstFirst = real
    .filter((report) => severityGroup(worstStatus(report).statusSeverity) !== 'good')
    .sort(compareWorstFirst);

  return { counts, totalLines: real.length, worstFirst, byMode, byCountry };
}
```

- [ ] **Step 2: Write the test file**

```ts
import { describe, it, expect } from 'vitest';
import { buildNetworkStatusOverview } from './networkStatusOverview';
import type { LineStatus, LineStatusReport } from './types';

function status(overrides: Partial<LineStatus> & { statusSeverity: number }): LineStatus {
  return {
    statusSeverityDescription: 'x',
    reason: '',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
    fullCoverageAvailability: { state: 'not-enabled' },
    ...overrides,
  };
}

function report(overrides: Partial<LineStatusReport> & { id: string; name: string }): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    modeName: 'national-rail',
    operators: [],
    lineStatuses: [],
    computedAt: '2026-09-22T09:00:00Z',
    ...overrides,
  };
}

describe('buildNetworkStatusOverview', () => {
  it('buckets each line by its worst status, into the five real SeverityGroup values', () => {
    const reports = [
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 10 })] }), // good
      report({ id: 'b', name: 'B', lineStatuses: [status({ statusSeverity: 9 })] }),  // mild
      report({ id: 'c', name: 'C', lineStatuses: [status({ statusSeverity: 2 })] }),  // severe
      report({ id: 'd', name: 'D', lineStatuses: [status({ statusSeverity: 4 })] }),  // planned
      report({ id: 'e', name: 'E', lineStatuses: [status({ statusSeverity: 0 })] }),  // informational
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.counts).toEqual({ good: 1, mild: 1, severe: 1, planned: 1, informational: 1 });
    expect(overview.totalLines).toBe(5);
  });

  it('excludes MERGED_TFL_LINE_IDS so a merged line is never counted twice', () => {
    const reports = [
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line (TfL)', lineStatuses: [status({ statusSeverity: 23 })] }),
      report({ id: 'elizabeth-line', name: 'Elizabeth line', modeName: 'elizabeth-line', lineStatuses: [status({ statusSeverity: 10 })] }),
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.totalLines).toBe(1);
    expect(overview.counts.good).toBe(1);
    expect(overview.counts.severe).toBe(0);
  });

  it('sorts worstFirst by severity rank descending, then alphabetically, and excludes good-service lines', () => {
    const reports = [
      report({ id: 'wcml', name: 'West Coast Main Line', lineStatuses: [status({ statusSeverity: 9 })] }), // mild
      report({ id: 'gwr', name: 'Great Western Railway', lineStatuses: [status({ statusSeverity: 2 })] }),  // severe
      report({ id: 'ecml', name: 'East Coast Main Line', lineStatuses: [status({ statusSeverity: 2 })] }),  // severe
      report({ id: 'good', name: 'Good Line', lineStatuses: [status({ statusSeverity: 10 })] }),
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.worstFirst.map((r) => r.id)).toEqual(['ecml', 'gwr', 'wcml']);
  });

  it('splits National Rail from every TfL-published mode', () => {
    const reports = [
      report({ id: 'nr', name: 'NR', modeName: 'national-rail' }),
      report({ id: 'tube', name: 'Tube', modeName: 'tube' }),
      report({ id: 'dlr', name: 'DLR', modeName: 'dlr' }),
    ];
    const overview = buildNetworkStatusOverview(reports);
    expect(overview.byMode.nationalRail.map((r) => r.id)).toEqual(['nr']);
    expect(overview.byMode.tfl.map((r) => r.id).sort()).toEqual(['dlr', 'tube']);
  });

  it('groups by country via countryForReport, keying only countries actually present', () => {
    const reports = [report({ id: 'nr', name: 'NR', modeName: 'national-rail' })];
    const overview = buildNetworkStatusOverview(reports);
    expect(Object.keys(overview.byCountry)).toEqual(['Gb']);
  });

  it('returns all-zero counts and empty lists for no reports', () => {
    const overview = buildNetworkStatusOverview([]);
    expect(overview.counts).toEqual({ good: 0, informational: 0, planned: 0, mild: 0, severe: 0 });
    expect(overview.worstFirst).toEqual([]);
    expect(overview.totalLines).toBe(0);
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- lib/networkStatusOverview.test.ts
```

  Expected: typecheck clean, all new tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/networkStatusOverview.ts frontend/lib/networkStatusOverview.test.ts
git commit -m "frontend: add buildNetworkStatusOverview, the pure bucketing/sorting logic for the network status dashboard"
```

---

## Task 3: `app/status/page.tsx` — the dashboard page (new route)

**Files:** create `frontend/app/status/page.tsx`,
`frontend/app/status/page.test.tsx`.

Depends on Task 1 (labels/order) and Task 2 (`buildNetworkStatusOverview`).
Independent of Tasks 4-5 (the tiles link to `/lines?statusGroup=...`
regardless of whether that page can act on it yet — but Task 4/5 must land
before this feature is fully useful end-to-end).

- [ ] **Step 1: Write the page**

```tsx
import { Badge, Card, Group, SimpleGrid, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { getLineStatusForMode } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
import { buildNetworkStatusOverview, type NetworkStatusOverview } from '@/lib/networkStatusOverview';
import { SEVERITY_GROUP_LABELS, SEVERITY_GROUPS_BY_RANK, worstStatus } from '@/lib/severity';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import type { Country, LineStatusReport } from '@/lib/types';

// Same rationale as every other dynamic route in this app (see
// app/lines/page.tsx's own comment): without this, `next build` treats the
// route as eligible for static generation and tries to prerender it, which
// fails since the `api` service only exists on the compose network at
// runtime.
export const revalidate = 0;

const METADATA_TITLE = 'Network Status — Distant Signal';
const METADATA_DESCRIPTION =
  'A live, network-wide snapshot of every National Rail and TfL line this app tracks: how many are running a Good Service versus facing disruption or a planned closure right now, which lines need attention most, and how that breaks down by mode and by country.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

const COUNTRY_LABELS: Record<Country, string> = {
  Gb: 'GB',
  NorthernIreland: 'Northern Ireland',
  RepublicOfIreland: 'Republic of Ireland',
};

export default async function NetworkStatusPage() {
  // Deliberately the SAME cache key `app/page.tsx`/`app/lines/page.tsx`
  // already use (`lineStatusForMode:${DISPLAYED_MODES_PARAM}`) -- it is the
  // same request, so all three pages share one stale-fallback cache entry.
  const allReports = await withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () =>
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
  );
  const overview = buildNetworkStatusOverview(allReports);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="xs">
        <Title order={1}>Network Status</Title>
        <Text c="dimmed">
          {overview.totalLines} line{overview.totalLines === 1 ? '' : 's'} tracked across National Rail
          and TfL right now.
        </Text>
      </Stack>

      <SimpleGrid cols={{ base: 2, sm: 3, lg: 5 }} spacing="md">
        {SEVERITY_GROUPS_BY_RANK.map((group) => (
          <SeverityCounterTile key={group} group={group} count={overview.counts[group]} />
        ))}
      </SimpleGrid>

      <WorstLinesSection worstFirst={overview.worstFirst} />

      <Stack gap="md">
        <Title order={2}>By mode</Title>
        <SimpleGrid cols={{ base: 1, sm: 2 }} spacing="md">
          <ModeCard label="National Rail" reports={overview.byMode.nationalRail} />
          <ModeCard label="TfL" reports={overview.byMode.tfl} />
        </SimpleGrid>
      </Stack>

      {/* Self-hides below two countries present -- see this plan's Judgment
          Call 6: `MODE_TO_COUNTRY` is still empty today (lib/modes.ts's own
          doc comment), so every report is 'Gb' and a one-entry breakdown
          would say nothing a visitor doesn't already know. Mirrors
          AllLinesTable's own identical self-hiding country filter. */}
      {Object.keys(overview.byCountry).length > 1 && (
        <Stack gap="md">
          <Title order={2}>By country</Title>
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {(Object.entries(overview.byCountry) as [Country, LineStatusReport[]][]).map(([country, reports]) => (
              <ModeCard key={country} label={COUNTRY_LABELS[country]} reports={reports} />
            ))}
          </SimpleGrid>
        </Stack>
      )}
    </Stack>
  );
}

/** One of the five top-strip counters. Links to `/lines` pre-filtered to
 * this bucket (Task 4/5 make that filter real) -- the query param name and
 * values are `SeverityGroup` itself, so this link and `AllLinesTable`'s own
 * filter chips can never drift on what a value means. Page-local, not a new
 * `components/` file: this is the only page that renders a "count + label"
 * tile today, mirroring `app/page.tsx`'s own `RightNowModule` precedent for
 * staying local until a second page wants the same shape. */
function SeverityCounterTile({
  group,
  count,
}: {
  group: (typeof SEVERITY_GROUPS_BY_RANK)[number];
  count: number;
}) {
  return (
    <Link href={`/lines?statusGroup=${group}`} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder shadow="sm" padding="lg">
        <Stack gap={4} align="center">
          <Text size="xl" fw={700}>
            {count}
          </Text>
          <Text size="sm" c="dimmed" ta="center">
            {SEVERITY_GROUP_LABELS[group]}
          </Text>
        </Stack>
      </Card>
    </Link>
  );
}

/** The unbounded worst-lines-first list -- the same "affected lines, worst
 * first" content `app/page.tsx`'s `RightNowModule` shows, but without its
 * `RIGHT_NOW_LIMIT` cap (this plan's Judgment Call 7): this page's whole
 * point is the full picture, not a five-row teaser. */
function WorstLinesSection({ worstFirst }: { worstFirst: LineStatusReport[] }) {
  return (
    <Stack gap="md">
      <Title order={2}>Lines to watch</Title>
      {worstFirst.length === 0 ? (
        <Text>Every line is running a Good Service.</Text>
      ) : (
        <Stack gap="xs">
          {worstFirst.map((report) => (
            <Link key={report.id} href={`/lines/${report.id}`} style={{ textDecoration: 'none', color: 'inherit' }}>
              <Card withBorder>
                <Group justify="space-between">
                  <Text fw={600}>{report.name}</Text>
                  <StatusBadge severity={worstStatus(report).statusSeverity} />
                </Group>
              </Card>
            </Link>
          ))}
        </Stack>
      )}
    </Stack>
  );
}

/** One row of the mode/country breakdown: a label plus how many of its
 * lines are affected (not Good Service) out of how many total. Reused for
 * both the mode breakdown (National Rail/TfL) and the country breakdown
 * (GB/NI/RoI) -- both answer the exact same question ("how is this slice of
 * the network doing"), just sliced a different way. */
function ModeCard({ label, reports }: { label: string; reports: LineStatusReport[] }) {
  const affected = reports.filter((report) => worstStatus(report).statusSeverity !== 10 && worstStatus(report).statusSeverity !== 25).length;
  return (
    <Card withBorder padding="lg">
      <Stack gap={4}>
        <Group justify="space-between">
          <Text fw={600}>{label}</Text>
          <Badge color={affected === 0 ? 'green' : 'yellow'} variant="light">
            {affected === 0 ? 'All Good Service' : `${affected} affected`}
          </Badge>
        </Group>
        <Text size="sm" c="dimmed">
          {reports.length} line{reports.length === 1 ? '' : 's'} tracked
        </Text>
      </Stack>
    </Card>
  );
}
```

  Note on `ModeCard`'s "affected" check: it inlines the two known "good"
  severities (`10`/`25`) rather than importing `isGoodSeverity` from
  `lib/severity.ts` purely to keep this task's diff self-contained --
  **prefer `isGoodSeverity(worstStatus(report).statusSeverity)`** from
  `@/lib/severity` instead when implementing this step (it already exists
  and does exactly this), i.e. write:

```tsx
import { isGoodSeverity, SEVERITY_GROUP_LABELS, SEVERITY_GROUPS_BY_RANK, worstStatus } from '@/lib/severity';
// ...
const affected = reports.filter((report) => !isGoodSeverity(worstStatus(report).statusSeverity)).length;
```

  (Both read the same table under the hood; using the existing exported
  helper avoids duplicating the "10 or 25 means good" knowledge a second
  time in this file.)

- [ ] **Step 2: Write the test file**

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import NetworkStatusPage, { metadata } from './page';
import * as api from '@/lib/api';
import { __resetStaleCacheForTests } from '@/lib/liveDataCache';
import type { LineStatus, LineStatusReport } from '@/lib/types';

vi.mock('@/lib/api');
vi.mock('next/headers', () => ({
  cookies: async () => ({ toString: () => '', get: () => undefined }),
}));

function status(overrides: Partial<LineStatus> & { statusSeverity: number }): LineStatus {
  return {
    statusSeverityDescription: 'x',
    reason: '',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    sampleAvailability: { state: 'no-coverage' },
    fullCoverageAvailability: { state: 'not-enabled' },
    ...overrides,
  };
}

function report(overrides: Partial<LineStatusReport> & { id: string; name: string }): LineStatusReport {
  return {
    $type: 'DistantSignal.LineStatusReport',
    modeName: 'national-rail',
    operators: [],
    lineStatuses: [],
    computedAt: '2026-09-22T09:00:00Z',
    ...overrides,
  };
}

beforeEach(() => {
  __resetStaleCacheForTests();
  vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
});

describe('NetworkStatusPage', () => {
  it('renders one counter tile per severity group, each linking to /lines?statusGroup=<group>', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 2 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());

    for (const [group, count] of [
      ['good', '0'],
      ['informational', '0'],
      ['planned', '0'],
      ['mild', '0'],
      ['severe', '1'],
    ]) {
      const link = screen.getByRole('link', { name: new RegExp(count) });
      expect(link).toBeTruthy();
      void group;
    }
    expect(screen.getByRole('link', { name: /Severe Disruption/ })).toHaveAttribute(
      'href',
      '/lines?statusGroup=severe',
    );
  });

  it('shows the good-service empty state when nothing is affected', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'a', name: 'A', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('Every line is running a Good Service.')).toBeInTheDocument();
  });

  it('lists affected lines worst-first, unbounded (no five-row cap)', async () => {
    const reports = Array.from({ length: 8 }, (_, i) =>
      report({ id: `line-${i}`, name: `Line ${i}`, lineStatuses: [status({ statusSeverity: 2 })] }),
    );
    vi.mocked(api.getLineStatusForMode).mockResolvedValue(reports);
    renderWithMantine(await NetworkStatusPage());
    for (const r of reports) {
      expect(screen.getByRole('link', { name: new RegExp(r.name) })).toBeInTheDocument();
    }
  });

  it('excludes a merged TfL id from both the counts and the mode breakdown', async () => {
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line (TfL)', lineStatuses: [status({ statusSeverity: 10 })] }),
    ]);
    renderWithMantine(await NetworkStatusPage());
    expect(screen.getByText('0 lines tracked across National Rail and TfL right now.')).toBeInTheDocument();
  });

  it('still renders the page shell when the status fetch fails outright (no stale entry yet)', async () => {
    vi.mocked(api.getLineStatusForMode).mockRejectedValue(new Error('connect ECONNREFUSED'));
    await expect(NetworkStatusPage()).rejects.toThrow();
    // Documents current behavior: unlike app/lines/page.tsx (which wraps
    // getAllLines in withStaleFallback and has a prior successful render to
    // fall back to), a cold cache with no prior success still throws to
    // app/error.tsx, same as every other withStaleFallback call site on a
    // cold cache.
  });
});

describe('metadata', () => {
  it('titles the page after its own heading', () => {
    expect(metadata.title).toBe('Network Status — Distant Signal');
  });

  it('mirrors title/description into openGraph and twitter', () => {
    expect(metadata.openGraph).toMatchObject({ title: metadata.title, type: 'website' });
    expect(metadata.twitter).toMatchObject({ card: 'summary', title: metadata.title });
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- app/status/page.test.tsx
```

  Expected: typecheck clean, all tests pass. (If the first test's loop-based
  assertion proves awkward against Testing Library's query semantics,
  replace it with five explicit `getByRole('link', { name: ... })` calls,
  one per bucket — the intent, not the exact loop shape, is what matters.)

- [ ] **Step 4: Commit**

```bash
git add frontend/app/status/page.tsx frontend/app/status/page.test.tsx
git commit -m "frontend: add /status, the all-lines aggregated network dashboard"
```

---

## Task 4: `AllLinesTable.tsx` — status-group filter

**Files:** modify `frontend/app/lines/AllLinesTable.tsx`,
`frontend/app/lines/AllLinesTable.test.tsx`.

Depends on Task 1 (`severityGroup`, `SeverityGroup`, `SEVERITY_GROUP_LABELS`,
`SEVERITY_GROUPS_BY_RANK`). Independent of Task 3.

- [ ] **Step 1: Extend the import from `@/lib/severity`**
  (`frontend/app/lines/AllLinesTable.tsx:26`):

```ts
import { worstStatus, severityRank, severityGroup, SEVERITY_GROUP_LABELS, SEVERITY_GROUPS_BY_RANK, type SeverityGroup } from '@/lib/severity';
```

- [ ] **Step 2: Add the prop**, in the component's props type
  (`frontend/app/lines/AllLinesTable.tsx:133-148`):

```tsx
export function AllLinesTable({
  lines,
  reports,
  pinnedLineIds,
  tocs,
  viewerIsAnonymous = false,
  initialStatusGroup,
}: {
  lines: LineSummary[];
  reports: LineStatusReport[];
  pinnedLineIds: string[];
  tocs: Suggestion[];
  viewerIsAnonymous?: boolean;
  /** Seeds the new status-group filter below from a query param (e.g.
   * `/lines?statusGroup=severe`, followed from `app/status/page.tsx`'s
   * counter tiles) -- consumed only as this `useState`'s initial value,
   * never re-read after mount, exactly like `IncidentSearchForm`'s own
   * `initialOperator`/`initialLine`/`initialFrom`/`initialTo` props. */
  initialStatusGroup?: SeverityGroup;
}) {
```

- [ ] **Step 3: Add the filter's state**, alongside the existing filter
  state (`frontend/app/lines/AllLinesTable.tsx:149-157`):

```tsx
  const [selectedOperators, setSelectedOperators] = useState<string[]>([]);
  const [selectedCountries, setSelectedCountries] = useState<Country[]>([]);
  const [statusGroupFilter, setStatusGroupFilter] = useState<SeverityGroup | null>(initialStatusGroup ?? null);
  const [nameQuery, setNameQuery] = useState('');
  const [sort, setSort] = useState<SortState | null>({ field: 'name', direction: 'asc' });
  const countryLabelId = useId();
  const statusLabelId = useId();
```

- [ ] **Step 4: Add the label helper**, alongside `countryChipLabel`
  (after `frontend/app/lines/AllLinesTable.tsx:113`):

```ts
/** Mirrors `countryChipLabel` immediately above -- single-select, not
 * multi (a line has exactly one worst status right now, so "2 selected"
 * would never be a meaningful state the way it is for operator/country). */
function statusChipLabel(selected: SeverityGroup | null): string {
  return selected ? `Status — ${SEVERITY_GROUP_LABELS[selected]}` : 'Status — showing all';
}
```

- [ ] **Step 5: Add the filtering step**, in `filteredRows`
  (`frontend/app/lines/AllLinesTable.tsx:212-237`, inserted after the
  country-filter block and before `return result;`):

```ts
    // AND-combined with the other three filters, same posture: a
    // severity-group filter answers a different question (what state is
    // the line in right now) than operator/country/name do. `row.worst` is
    // `undefined` for a line with no computed status at all ("NO DATA" in
    // the table below) -- such a row cannot match any specific bucket, and
    // is only ever visible under "All statuses".
    if (statusGroupFilter) {
      result = result.filter(
        (row) => row.worst !== undefined && severityGroup(row.worst.statusSeverity) === statusGroupFilter,
      );
    }
    return result;
  }, [rows, selectedOperators, selectedCountries, statusGroupFilter, nameQuery]);
```

  (Note the `useMemo` dependency array on the closing line above also gains
  `statusGroupFilter` — it is currently `[rows, selectedOperators,
  selectedCountries, nameQuery]` at `frontend/app/lines/AllLinesTable.tsx:237`.)

- [ ] **Step 6: Add the chip row**, directly below the `TextInput`/
  `MultiSelect` `Group` and above the existing country `ChipGroup` block
  (i.e. inserted between `frontend/app/lines/AllLinesTable.tsx:311` and
  `:312`), unconditionally rendered (unlike the country block, which
  self-hides below two countries — the status filter is always meaningful):

```tsx
        </Group>
        <Stack gap={4}>
          <Text id={statusLabelId} size="xs" fw={600} c="dimmed">
            {statusChipLabel(statusGroupFilter)}
          </Text>
          <ChipGroup
            value={statusGroupFilter ?? ''}
            onChange={(value) => setStatusGroupFilter(value === '' ? null : (value as SeverityGroup))}
          >
            <Group gap="xs" role="group" aria-labelledby={statusLabelId}>
              <Chip value="" size="xs" variant={statusGroupFilter === null ? 'filled' : 'outline'}>
                All statuses
              </Chip>
              {SEVERITY_GROUPS_BY_RANK.map((group) => (
                <Chip
                  key={group}
                  value={group}
                  size="xs"
                  variant={statusGroupFilter === group ? 'filled' : 'outline'}
                >
                  {SEVERITY_GROUP_LABELS[group]}
                </Chip>
              ))}
            </Group>
          </ChipGroup>
        </Stack>
        {countryOptions.length > 1 && (
```

  (The last line above is the existing conditional at
  `frontend/app/lines/AllLinesTable.tsx:317` — shown here only to make the
  insertion point unambiguous; it is otherwise unchanged.)

- [ ] **Step 7: Add tests**, in `frontend/app/lines/AllLinesTable.test.tsx`
  (new `describe` block, using this file's existing `lines`/`reports`
  fixtures — `wcml` is Minor Delays/mild, `gwr` is Suspended/severe, `swr`
  has no status in the existing fixture, confirm against the file's own
  fixtures before writing exact expectations):

```tsx
describe('status-group filter', () => {
  it('shows only lines whose worst status is in the selected group', () => {
    renderWithMantine(
      <AllLinesTable lines={lines} reports={reports} pinnedLineIds={[]} tocs={[]} />,
    );
    fireEvent.click(screen.getByRole('button', { name: /Severe Disruption/ }));
    expect(screen.getByRole('link', { name: 'Great Western Railway' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'West Coast Main Line' })).not.toBeInTheDocument();
  });

  it('seeds the filter from initialStatusGroup and filters on first render', () => {
    renderWithMantine(
      <AllLinesTable
        lines={lines}
        reports={reports}
        pinnedLineIds={[]}
        tocs={[]}
        initialStatusGroup="mild"
      />,
    );
    expect(screen.getByRole('link', { name: 'West Coast Main Line' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Great Western Railway' })).not.toBeInTheDocument();
  });

  it('"All statuses" clears the filter back to every line', () => {
    renderWithMantine(
      <AllLinesTable lines={lines} reports={reports} pinnedLineIds={[]} tocs={[]} initialStatusGroup="mild" />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'All statuses' }));
    expect(screen.getByRole('link', { name: 'West Coast Main Line' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Great Western Railway' })).toBeInTheDocument();
  });
});
```

  A Mantine `Chip` renders as a `<label>` wrapping a checkbox-like input, not
  a `role="button"` — **verify the actual accessible role/name Testing
  Library reports for this repo's Mantine version before finalizing these
  three tests** (this file's existing country-filter tests, if any exist
  further down, are the closest precedent to copy the exact query pattern
  from — check them first rather than guessing the role).

- [ ] **Step 8: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- app/lines/AllLinesTable.test.tsx
```

  Expected: typecheck clean, all tests (existing plus new) pass. If Step 7's
  `getByRole` queries need adjusting once run against the real DOM output,
  fix the queries, not the component.

- [ ] **Step 9: Commit**

```bash
git add frontend/app/lines/AllLinesTable.tsx frontend/app/lines/AllLinesTable.test.tsx
git commit -m "frontend: add a status-group filter to AllLinesTable, seedable via initialStatusGroup"
```

---

## Task 5: `app/lines/page.tsx` — read `?statusGroup=` from `searchParams`

**Files:** modify `frontend/app/lines/page.tsx`, `frontend/app/lines/page.test.tsx`.

Depends on Task 1 (`isSeverityGroup`) and Task 4 (`AllLinesTable`'s new
`initialStatusGroup` prop).

- [ ] **Step 1: Add the `searchParams` param and import**, changing
  `frontend/app/lines/page.tsx:3` and `:76`:

```tsx
import { getAllLines, getAllTocs, getLineStatusForMode, getPreferences, getSession } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
import { isSeverityGroup } from '@/lib/severity';
import type { Preferences } from '@/lib/types';
```

```tsx
export default async function AllLinesPage({
  searchParams,
}: {
  searchParams: Promise<{ statusGroup?: string | string[] }>;
}) {
  const { statusGroup } = await searchParams;
  const statusGroupParam = Array.isArray(statusGroup) ? statusGroup[0] : statusGroup;
  const initialStatusGroup = isSeverityGroup(statusGroupParam) ? statusGroupParam : undefined;

  const [lines, preferences, reports, tocs, viewerIsAnonymous] = await Promise.all([
```

  (Everything from `const [lines, ...] = await Promise.all([` through the end
  of that block, `frontend/app/lines/page.tsx:77-99`, is unchanged — only the
  function signature above it and the two new lines before it are added.)

- [ ] **Step 2: Pass the prop through**
  (`frontend/app/lines/page.tsx:111-117`):

```tsx
        <AllLinesTable
          lines={lines}
          reports={reports}
          pinnedLineIds={preferences.pinnedLines}
          tocs={tocs}
          viewerIsAnonymous={viewerIsAnonymous}
          initialStatusGroup={initialStatusGroup}
        />
```

- [ ] **Step 3: Add tests**, in `frontend/app/lines/page.test.tsx` (this
  page's own test currently calls `AllLinesPage()` with no arguments —
  `renderPage()`, `frontend/app/lines/page.test.tsx:51-53` — every call site
  needs a `searchParams` promise now):

```tsx
async function renderPage(searchParams: Record<string, string | string[]> = {}) {
  return renderWithMantine(await AllLinesPage({ searchParams: Promise.resolve(searchParams) }));
}
```

  (Update every existing `renderPage()` call in this file to use the new
  signature — since it now takes an optional default, `await renderPage()`
  with no arguments still works for every pre-existing test unchanged.)

```tsx
describe('statusGroup deep link', () => {
  it('pre-selects the AllLinesTable status filter from a valid ?statusGroup= value', async () => {
    await renderPage({ statusGroup: 'severe' });
    expect(screen.getByRole('button', { name: /Severe Disruption/ })).toHaveAttribute('aria-pressed', 'true');
  });

  it('ignores an unrecognized ?statusGroup= value rather than erroring', async () => {
    await renderPage({ statusGroup: 'not-a-real-group' });
    expect(screen.getByRole('heading', { name: 'All Lines', level: 1 })).toBeInTheDocument();
  });
});
```

  As with Task 4 Step 7, **confirm the actual rendered role/attribute a
  selected Mantine `Chip` exposes** (it may not literally be
  `aria-pressed="true"`) before finalizing this assertion — inspect the
  real rendered output (e.g. via `screen.debug()` while iterating) rather
  than guessing.

- [ ] **Step 4: Verify**

```bash
cd frontend && npx tsc --noEmit && npm test -- app/lines/page.test.tsx
```

  Expected: typecheck clean, all tests (existing, now updated for the new
  call signature, plus new) pass.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/page.tsx frontend/app/lines/page.test.tsx
git commit -m "frontend: read ?statusGroup= on /lines and seed AllLinesTable's new filter from it"
```

---

## Task 6: Nav integration — make `/status` discoverable

**Files:** modify `frontend/lib/navLinks.ts`, `frontend/e2e/nav.spec.ts`.

Depends on Task 3 (the route must exist). This is the one task in this plan
with real, previously-documented layout regression risk — see Judgment
Call 9 and the Global Constraints note on `e2e/nav.spec.ts`.

- [ ] **Step 1: Add the destination**, in `PRIMARY_NAV_DESTINATIONS`
  (`frontend/lib/navLinks.ts:24-34`), placed first (the network-wide
  overview precedes the full per-line table it links into):

```ts
export const PRIMARY_NAV_DESTINATIONS: readonly NavDestination[] = [
  { href: '/status', label: 'Status' },
  { href: '/lines', label: 'All Lines' },
  { href: '/stations', label: 'Station Lookup' },
  { href: '/trains', label: 'Find a Train' },
  { href: '/incidents', label: 'Incident Archive' },
];
```

  `frontend/lib/navLinks.test.ts` and `frontend/components/AppNavBar.test.tsx`
  both iterate `PRIMARY_NAV_DESTINATIONS` dynamically (verified: the latter's
  own `for (const destination of PRIMARY_NAV_DESTINATIONS)` at line 47 and
  123) and need no manual update. `frontend/components/AppNavDrawer.test.tsx`
  passes its own synthetic fixture array, not the real export (verified —
  its `destinations` prop is a local literal, not this import), so it also
  needs no change.

- [ ] **Step 2: Update `e2e/nav.spec.ts`'s two hardcoded label lists** — this
  file, unlike the unit tests above, hardcodes the destination set rather
  than importing it:

  `frontend/e2e/nav.spec.ts:118` (desktop, inline links test):

```ts
    for (const label of ['Status', 'All Lines', 'Station Lookup', 'Find a Train', 'Incident Archive']) {
```

  `frontend/e2e/nav.spec.ts:190-196` (phone, drawer test):

```ts
    for (const label of [
      'Status',
      'All Lines',
      'Station Lookup',
      'Find a Train',
      'Incident Archive',
      'My Trains & Tickets',
    ]) {
```

- [ ] **Step 3: Manually re-verify the nav bar's layout against a real
  running app.** This is not optional polish — `e2e/nav.spec.ts` asserts an
  *exact* pixel height (`toBe(61)`) at both 1440×900 and 390×844, and a
  single-row layout at the 992/991px breakpoint boundary, against measured
  slack as tight as 42px (`components/AppNavBar.tsx:90-131`'s own doc
  comment). Adding one nav item is exactly the kind of change that
  regressed this before.

```bash
cd frontend
npm run build && npm run start &   # or `npm run dev` against a real backend
npx playwright test e2e/nav.spec.ts --project=chromium --project=firefox-nav
```

  Expected: every assertion in `e2e/nav.spec.ts` still passes, in **both**
  browser projects (the file's own header explains why Firefox's narrower
  text metrics make it the binding case at 1440px). If the bar wraps or the
  height assertion fails:
  - Do **not** edit the `toBe(61)`/row-count expectations to make it pass —
    that number is a real, previously-measured invariant (Global
    Constraints).
  - Instead, shorten the label further, or reconsider whether `/status`
    belongs in `PRIMARY_NAV_DESTINATIONS` at all versus, e.g., linked only
    from `/lines`'s own header (mirroring how "Incident Archive" is linked
    from there today, `frontend/app/lines/page.tsx:106-109`) as a fallback
    if inline nav genuinely doesn't fit — re-measure per
    `components/AppNavBar.tsx`'s own documented method (drive the real app,
    quote the actual slack figures) rather than guessing.

  This step cannot be completed by this plan alone (it requires a running
  dev stack and a real browser) — record the actual measured outcome when
  executing this task.

- [ ] **Step 4: Verify (automated parts)**

```bash
cd frontend
npx tsc --noEmit
npm test -- lib/navLinks.test.ts components/AppNavBar.test.tsx components/AppNavDrawer.test.tsx
```

  Expected: typecheck clean, all three test files still pass unchanged
  (per Step 1's own verification that neither hardcodes the destination
  list in a way this change breaks).

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/navLinks.ts frontend/e2e/nav.spec.ts
git commit -m "frontend: add /status to the primary nav, update nav.spec.ts's hardcoded label lists"
```

---

## Task 7: Full-suite verification

**Files:** none (verification only).

- [ ] **Step 1: Typecheck the whole frontend**

```bash
cd frontend && npx tsc --noEmit
```

- [ ] **Step 2: Run the full Vitest suite**

```bash
cd frontend && npm test
```

  Expected: every test passes, including every file touched or added by
  Tasks 1-6 and every pre-existing test this plan did not intend to change
  (`app/page.test.tsx`, the rest of `AllLinesTable.test.tsx`, etc.).

- [ ] **Step 3: Production build**

```bash
cd frontend && npm run build
```

  Expected: builds clean, including the new `/status` static/dynamic route
  (with `revalidate = 0`, matching every other backend-dependent route in
  this app, it is excluded from prerendering the same way `/lines` and `/`
  already are).

- [ ] **Step 4: Confirm Task 6's e2e re-verification was actually recorded**
  (not re-run here — this is a reminder, not a new check): the plan is not
  complete until Task 6 Step 3's real-browser pass has been executed and its
  outcome (pass, or the label/placement adjustment it required) is recorded
  against that task.

No commit for this task — it is the final gate before considering Phase 1
done, not a change of its own.
