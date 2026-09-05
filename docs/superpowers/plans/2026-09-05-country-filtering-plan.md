# Plan: Country Filtering (GB / Northern Ireland / Republic of Ireland) on `/lines`

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **Both tasks below are normal, buildable, testable, mergeable
> implementation work in *this* repo, today** — unlike some recent plans in
> this repo with a Part 2 that depends on infrastructure this repo can't
> reach, there is no such split here. The one thing that makes this plan
> unusual is stated plainly in the design spec and repeated here: the
> surface this plan wires the filter into (`/lines`) has **no real non-GB
> data today, and may not for a long time** (Ireland Tier C — the thing
> that would ever populate a non-GB `LineStatusReport` row — is currently
> no-go; see "Why this is still real, mergeable work" below). Both tasks'
> test plans account for that directly: the "second country" path is
> exercised against synthetic data manufactured in the test itself, not
> against anything a real poller produces.

**Goal:** implement
`docs/superpowers/specs/2026-09-05-country-filtering-design.md` ("the
design spec") in full, for its one implemented surface (`AllLinesTable.tsx`
/ the `/lines` page) — a client-side-derived country value per line-status
row, and a self-hiding multi-select filter control over it, with zero
backend/API changes, matching the design spec's Decisions 1–5 exactly.

**Architecture:** one small, additive change to `frontend/lib/modes.ts`
(Task 1) — a `Country` type, an (initially empty) `MODE_TO_COUNTRY` lookup
table, and two pure derivation functions (`countryForMode`,
`countryForReport`) — mirroring that file's existing
`MERGED_TFL_LINE_IDS`/`TFL_TO_NR_LINE_ID` precedent exactly, per the design
spec's Decision 3. One change to `frontend/app/lines/AllLinesTable.tsx`
(Task 2) wires that derivation into the row-building logic already there,
computes which countries are actually present in the current render
(mirroring `operatorOptions`'s existing self-populating pattern), and
renders a `Chip.Group multiple` filter control — gated to only appear once
more than one country is present — that AND-combines with the existing
operator filter, per Decision 4/5.

**Design doc:**
`docs/superpowers/specs/2026-09-05-country-filtering-design.md` — its
Decisions 1–5 and Non-goals are authoritative for reasoning this plan does
not repeat. This plan makes the one call that spec's own Non-goals section
explicitly left open (the UI affordance — see Judgment Call 1 below) and
resolves how to test a feature whose real data doesn't exist yet (Judgment
Call 2).

---

## Why this is still real, mergeable work despite "nothing to filter" today

The design spec's own §8 Open Question 1 states plainly that this
surface "may have nothing to filter... for a long time, or ever." That is
true and does not change here. What it does **not** mean is that this
plan's tasks are speculative or untestable:

- The `MODE_TO_COUNTRY` lookup table (Task 1) is real, tested code today —
  it is simply an *empty* table today (no `modeName` this app emits maps
  to anything but the implicit `Gb` default), which is itself the correct,
  verifiable state per Decision 2/3 ("GB is never an explicit tag... GB is
  simply the absence of an Ireland-mapped mode"). Its derivation functions
  (`countryForMode`/`countryForReport`) are fully exercised by unit tests
  today via an **injectable lookup table parameter** (Judgment Call 2) —
  not left untested until real data exists.
- The filter control and its self-hiding gate (Task 2) are real,
  shippable UI today. Its "only GB present → hidden" branch is exercised
  against this repo's own real, current fixture data (`AllLinesTable.test.tsx`'s
  existing `lines`/`reports` — always `national-rail` today). Its "two
  countries present → shown, filters correctly" branch is exercised
  against **synthetic** fixture data plus one narrowly-scoped partial
  module mock of `MODE_TO_COUNTRY` (Judgment Call 2) — a real, passing,
  CI-enforced test, not a manual-only check.

Once (if) Ireland Tier C ever ships a poller that emits a non-`Gb`
`modeName`, the only future change needed on this surface is adding that
one real string to `MODE_TO_COUNTRY` — the type, the derivation, the UI,
and the self-hiding gate are already built, reviewed, and tested by the
time that day comes. This is the "ready the day it's needed" framing the
design spec itself asked for (§8 Open Question 1's closing line).

---

## Judgment calls this plan makes (read before Task 1)

The design spec's own Non-goals (§7) named the UI affordance as an
explicit, undecided implementation-time choice. This plan's own brief also
flagged that a synthetic-data test strategy needs a concrete mechanism, not
just a description. Both are resolved here, against real, current code —
not guessed at:

1. **UI affordance: `Chip.Group multiple` (Mantine's `ChipGroup`/`Chip`),
   not `MultiSelect` and not `SegmentedControl`.** All three exist as real
   precedent somewhere in this codebase today, so this is a choice between
   three things this repo already does, not an invented fourth pattern:
   - `MultiSelect` (`AllLinesTable.tsx:169-178`, the operator filter) is
     built for a searchable dropdown over dozens of codes. A 2–3-value
     filter has nothing to search, and a closed-by-default dropdown hides
     every option behind a click for a value set small enough that all of
     them could be visible at a glance. This is the design spec's own
     framing (§4: "given only 2-3 values are ever possible, a segmented
     control/`Chip.Group` may read better").
   - `SegmentedControl` (`components/IssueList.tsx:293-306`, the
     active/upcoming/ended filter) is **single-select only** — Mantine's
     `SegmentedControl` has no `multiple` mode. That's a real, structural
     downgrade here: a genuinely useful country-filter query is "show me
     everything on the island of Ireland" (`NorthernIreland` +
     `RepublicOfIreland` both selected, `Gb` excluded) — impossible with a
     control that only ever holds one value at a time. Rejected on
     capability, not just taste.
   - **`Chip.Group multiple`** (`components/IssueList.tsx:256-291`, the
     existing `severityFilter`/`sourceFilter` controls) is exact, current,
     in-repo precedent for this precise shape of problem: a small,
     always-fully-visible, independently-toggleable set of values, held as
     `useState<string[]>`, AND-combined with sibling filters over the same
     row set, with a `chipRowLabel`-shaped facet label ("Country — showing
     all" / "Country — N selected") stating explicitly that an empty
     selection filters nothing (that file's own comment at `:127-130`
     explains why this matters: "an empty selection filters nothing, which
     is not the same as filtering to nothing"). Task 2 copies this pattern
     structurally, including the `role="group"`/`aria-labelledby` wiring
     `IssueList.tsx` already uses (`Chip`/`ChipGroup` carry no built-in
     accessible group label on their own).
2. **Testing the "two countries present" path without real data: a
   narrowly-scoped `vi.mock('@/lib/modes', ...)` using `importOriginal`,
   plus an injectable-table parameter on the pure derivation functions
   themselves.** Two distinct mechanisms, matched to the two things being
   tested:
   - **Task 1's own unit tests** (`modes.test.ts`) test `countryForMode`/
     `countryForReport` directly, so they don't need to mock anything —
     both functions take the lookup table as an optional parameter
     (default: the real, exported `MODE_TO_COUNTRY`), so a test can pass a
     literal synthetic table (e.g. `{ 'island-of-ireland-nir':
     'NorthernIreland' }`) inline and assert the derivation logic itself
     works, without that string ever being claimed as real. This also
     means `MODE_TO_COUNTRY` doesn't need to be guessed-and-populated with
     unconfirmed values to be testable — a real anti-pattern this repo
     explicitly avoids elsewhere (`poller-ldbws/src/config.rs`'s
     `ldbws_base_url` doc comment: "deliberately has no default... must be
     supplied out of band once confirmed, not guessed"; the Ireland
     rail-support plan's own §8/Non-goals make the identical point about
     not committing to unconfirmed `modeName` strings — see
     `docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md:196`,
     which cites `island-of-ireland-*` as the *illustrative*, not decided,
     prefix). This plan's own tests use that same `island-of-ireland-*`
     shape purely as a realistic-looking synthetic label, not a claim
     about the real future value.
   - **Task 2's component test** (`AllLinesTable.test.tsx`) can't use the
     parameter trick, because `AllLinesTable.tsx` itself calls
     `countryForReport(report)` with its default (real) table — the
     component has no prop for injecting a table, and shouldn't grow one
     just for tests (that would be test-only API surface leaking into
     production code, which this repo avoids). Instead, one file-scoped
     `vi.mock('@/lib/modes', async (importOriginal) => ...)` — the exact
     `importOriginal` partial-mock pattern this repo already uses
     (`components/AutoRefresh.test.tsx:15-18`, `components/
     ConnectivityMonitor.test.tsx`, `app/incidents/[id]/page.test.tsx`) —
     merges one synthetic key onto the real (today: empty)
     `MODE_TO_COUNTRY`, keeping every other export of `modes.ts` real and
     unmocked. This affects every test in that file, but harmlessly: no
     existing fixture's `modeName` collides with the synthetic key, so
     every pre-existing test's behavior is unchanged; only the two new
     country-filter tests construct a report using that key.
3. **A line with no matching `LineStatusReport` defaults to `Gb`, not
   "unknown."** `AllLinesTable`'s `rows` memo (`:99-115`) already handles a
   line with no report (`report` is `undefined` — see the `swr` fixture in
   the existing test file, `:70`); `worst`/`stats`/`representative` are
   already `undefined`/`null` for it, but the row still renders. Since
   country is only derivable from a report's `modeName`, and every
   `LineSummary` this app can produce today comes from the GB catalogue,
   TfL, or a custom GB line (design spec §4, citing `list_lines`,
   `crates/api/src/routes/lines.rs:132-190` — no Ireland-sourced
   `LineSummary` exists or is reachable through this endpoint at all,
   design spec §8 Open Question 3), defaulting a report-less row to `Gb`
   is not a guess — it's the only value that's ever been true for such a
   row, and matches Decision 2's own framing (GB is "not tagged Ireland,"
   not a field that needs to be present to be true).

---

## Non-goals

- **Any change to `frontend/app/page.tsx`** (the home dashboard), even
  though it also reads `LineStatusReport[]` (`worstSeverityAcrossReports`,
  `representativeStatusAcrossReports`, `:38-76`). That surface has **no
  filter control of any kind today** — not even the operator filter this
  plan's country filter mirrors — so adding one there would be new UI
  invention, not extending an existing pattern, and is out of scope per
  the design spec's own exclusive focus on `AllLinesTable.tsx` (§4/§6).
  Noted for a future reader, not solved here.
- **Any change to `frontend/app/stations/StationSearchForm.tsx`** or any
  Ireland station-search surface. Per design spec §6/§7, explicitly
  deferred there and not re-opened here.
- **Populating `MODE_TO_COUNTRY` with any real non-`Gb` entry.** No such
  `modeName` exists yet (Ireland Tier C is no-go — Ireland spec §5/§7); see
  Judgment Call 2 above for why this plan is still fully testable without
  one.
- **Any backend/`api`/`common::` change.** Matches the design spec's own
  Non-goals (§7) exactly — everything in this plan is `frontend/` only.
- **URL persistence for the country filter.** Matches the operator
  filter's own precedent and Decision 4's explicit call.
- **Folding country into the existing operator filter**, or vice versa.
  Decision 4 requires they stay two independent, AND-combined filters.
- **An implementation plan for Ireland Tier C.** Not this plan's call to
  make (Ireland spec §7's own no-go stands); this plan only readies the
  frontend for the day that decision changes, per the design spec's own
  framing.

## Global Constraints

- **No `lines/*.toml` changes anywhere in this plan.** This is a pure
  `frontend/` change; nothing here touches the GB catalogue.
- **No new API route, query parameter, or response field.** Matches the
  design spec's Decision 3/Non-goals exactly — country is derived entirely
  client-side from data `AllLinesTable` already receives as props.
- **`MODE_TO_COUNTRY` stays a plain, hand-maintained `Record<string,
  Country>`** — no attempt to generate it, no attempt to make it
  exhaustive over `DISPLAYED_MODES` (Decision 2/3: GB is the *absence* of
  an entry, not a `Gb` value present for every mode; an exhaustive map
  would misrepresent that framing and would need updating every time
  `DISPLAYED_MODES` grows, for a fact — "still GB" — that never changes).
- **`AllLinesTable.tsx` grows no new prop.** The country derivation and
  filter state are entirely internal to the component, exactly like the
  existing operator filter — no call site (`frontend/app/lines/page.tsx`)
  needs to change.
- **Testing.** Frontend: `npm test` (`vitest run`, per
  `frontend/package.json`'s `"test"` script) after each task, plus this
  repo's own standing practice for UI changes — start the dev server
  (`npm run dev` inside `frontend/`) and manually verify in a real browser,
  not just jsdom, before considering Task 2 done. Concretely: (a) confirm
  the country control is **absent** on the real `/lines` page today (the
  real, current, all-GB state); (b) temporarily hardcode a second entry
  into `MODE_TO_COUNTRY` (e.g. `{ 'national-rail': 'NorthernIreland' }` —
  deliberately wrong/silly, just to force two distinct `country` values
  onto real rows) to confirm the control renders and filters correctly
  against the real page, then **revert that temporary edit before
  committing** — it must never reach a real commit.
- **File scope.** Modified: `frontend/lib/modes.ts`, `frontend/lib/
  modes.test.ts`, `frontend/app/lines/AllLinesTable.tsx`,
  `frontend/app/lines/AllLinesTable.test.tsx`. No other file changes in
  this plan.

---

## Task 1: `Country` type, `MODE_TO_COUNTRY`, `countryForMode`/`countryForReport`

**Files:** modify `frontend/lib/modes.ts`, `frontend/lib/modes.test.ts`.

Independent, first task — pure data/derivation, no UI, nothing else in the
codebase depends on it yet.

- [ ] **Step 1: Add the `Country` type and `MODE_TO_COUNTRY` to
  `frontend/lib/modes.ts`.** Append after the existing
  `MERGED_TFL_LINE_IDS` constant (current end of file, `:35-43`):

```typescript
/** The three jurisdictions this app can ever attribute a line-status row
 * to. Mirrors `IslandOfIrelandNetwork` in `crates/common/src/lib.rs`
 * (`NorthernIreland`/`RepublicOfIreland`) exactly, with `Gb` added as the
 * implicit "not tagged Ireland" default rather than a fourth variant
 * anywhere in `common::` -- see
 * docs/superpowers/specs/2026-09-05-country-filtering-design.md Decision 1
 * and Decision 2 for why GB is never an explicit backend tag. */
export type Country = 'Gb' | 'NorthernIreland' | 'RepublicOfIreland';

/** Maps a `LineStatusReport.modeName` to the country it belongs to.
 * Deliberately empty today: every mode this app can currently emit a
 * report for (`DISPLAYED_MODES`, above) is GB, and GB is never listed here
 * explicitly -- a `modeName` absent from this table is `Gb` by
 * construction (see Decision 2). This table gains entries only once a
 * real non-GB poller exists and its real `modeName` value(s) are known --
 * see docs/superpowers/specs/2026-09-05-country-filtering-design.md
 * Decision 3 and §8 Open Question 2, and
 * docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md:196 for
 * why `island-of-ireland-*` is illustrative, not a committed value, and
 * must not be guessed at here. Mirrors `MERGED_TFL_LINE_IDS`'s own shape:
 * a small, hand-maintained lookup over a mode/id this app already emits,
 * not a new backend field. */
export const MODE_TO_COUNTRY: Record<string, Country> = {};

/** Derives a country from a raw `modeName` string. `table` defaults to the
 * real `MODE_TO_COUNTRY` but is overridable so this derivation can be unit
 * tested against a synthetic mapping before any real non-GB `modeName`
 * exists (see the design spec's Judgment Call 2 in
 * docs/superpowers/plans/2026-09-05-country-filtering-plan.md) --
 * production call sites should never pass `table` explicitly. */
export function countryForMode(modeName: string, table: Record<string, Country> = MODE_TO_COUNTRY): Country {
  return table[modeName] ?? 'Gb';
}

/** `countryForMode`, keyed off a `LineStatusReport`/`LineStatusHistoryEntry`
 * directly -- `Pick<..., 'modeName'>` rather than the full report type so a
 * caller (or a test) doesn't need to fabricate every other required field
 * just to derive a country. */
export function countryForReport(
  report: Pick<import('./types').LineStatusReport, 'modeName'>,
  table: Record<string, Country> = MODE_TO_COUNTRY,
): Country {
  return countryForMode(report.modeName, table);
}
```

  (The inline `import('./types').LineStatusReport` avoids adding a new
  top-of-file `import type` line purely for one `Pick<...>` reference;
  either is fine stylistically in this codebase — use a normal top-level
  `import type { LineStatusReport } from './types';` instead if that reads
  more consistently with this file's existing conventions once you're
  looking at the real file rather than this plan's excerpt.)

- [ ] **Step 2: Unit tests in `frontend/lib/modes.test.ts`.** Append a new
  `describe` block, following that file's existing style (plain
  `describe`/`it`, no component rendering):

```typescript
describe('MODE_TO_COUNTRY / countryForMode / countryForReport', () => {
  it('MODE_TO_COUNTRY has no real entries yet -- Ireland Tier C has not shipped', () => {
    // Deliberately asserts emptiness, not just "doesn't throw" -- a
    // regression here would mean someone guessed at an unconfirmed
    // modeName value (see this table's own doc comment for why that must
    // not happen before a real poller exists).
    expect(MODE_TO_COUNTRY).toEqual({});
  });

  it('defaults every currently-displayed mode to Gb', () => {
    for (const mode of DISPLAYED_MODES) {
      expect(countryForMode(mode)).toBe('Gb');
    }
  });

  it('defaults an unrecognised modeName to Gb', () => {
    expect(countryForMode('some-mode-nobody-has-invented-yet')).toBe('Gb');
  });

  it('maps a modeName present in an injected table to its country', () => {
    // No real modeName maps to a non-Gb country today -- this exercises
    // the derivation logic itself against a synthetic table so it isn't
    // left untested until Ireland Tier C ships (see
    // docs/superpowers/plans/2026-09-05-country-filtering-plan.md
    // Judgment Call 2). The key is illustrative only, matching
    // docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md:196's
    // own "island-of-ireland-*" framing -- not a claim about a real value.
    const syntheticTable: Record<string, Country> = { 'island-of-ireland-nir': 'NorthernIreland' };
    expect(countryForMode('island-of-ireland-nir', syntheticTable)).toBe('NorthernIreland');
    expect(countryForMode('national-rail', syntheticTable)).toBe('Gb');
  });

  it('countryForReport derives from a report\'s modeName the same way', () => {
    const syntheticTable: Record<string, Country> = { 'island-of-ireland-roi': 'RepublicOfIreland' };
    expect(countryForReport({ modeName: 'island-of-ireland-roi' }, syntheticTable)).toBe('RepublicOfIreland');
    expect(countryForReport({ modeName: 'national-rail' }, syntheticTable)).toBe('Gb');
  });
});
```

  Add `Country`, `MODE_TO_COUNTRY`, `countryForMode`, `countryForReport` to
  this file's existing top import line (`import { DISPLAYED_MODES,
  DISPLAYED_MODES_PARAM, MERGED_TFL_LINE_IDS } from './modes';`).

- [ ] **Step 3: Verify**

```bash
cd frontend
npm test -- modes.test.ts
```

  Confirm all new and pre-existing `modes.test.ts` cases pass, and that
  `npx tsc --noEmit` (or `npm run build`, if that's this repo's real
  type-check gate — check `frontend/package.json`'s scripts if `tsc` isn't
  wired directly) reports no type errors from the new exports.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/modes.ts frontend/lib/modes.test.ts
git commit -m "frontend: add Country type, MODE_TO_COUNTRY lookup, countryForMode/countryForReport"
```

---

## Task 2: Self-hiding country filter in `AllLinesTable.tsx`

**Files:** modify `frontend/app/lines/AllLinesTable.tsx`,
`frontend/app/lines/AllLinesTable.test.tsx`.

Depends on Task 1 (imports `Country`/`MODE_TO_COUNTRY`/`countryForReport`
from `frontend/lib/modes.ts`). Wires the derivation into the existing
row-building logic and adds the `Chip.Group multiple` control (Judgment
Call 1), gated on more than one country being present (Decision 5).

- [ ] **Step 1: Imports.** Current imports (`:1-22`):

```typescript
import { useMemo, useState } from 'react';
import {
  Stack,
  Table,
  TableThead,
  TableTbody,
  TableTr,
  TableTh,
  TableTd,
  Text,
  MultiSelect,
  UnstyledButton,
  Tooltip,
} from '@mantine/core';
```

  Change to:

```typescript
import { useId, useMemo, useState } from 'react';
import {
  Chip,
  ChipGroup,
  Group,
  Stack,
  Table,
  TableThead,
  TableTbody,
  TableTr,
  TableTh,
  TableTd,
  Text,
  MultiSelect,
  UnstyledButton,
  Tooltip,
} from '@mantine/core';
```

  and add, alongside the existing `frontend/lib/severity`/`sampleStats`
  imports:

```typescript
import { countryForReport, type Country } from '@/lib/modes';
```

- [ ] **Step 2: `COUNTRY_LABELS` constant.** Add after
  `expandOperatorForFiltering` (current `:61-63`), before the
  `AllLinesTable` function declaration:

```typescript
const COUNTRY_LABELS: Record<Country, string> = {
  Gb: 'GB',
  NorthernIreland: 'Northern Ireland',
  RepublicOfIreland: 'Republic of Ireland',
};

/** Mirrors `chipRowLabel` in `components/IssueList.tsx:131-133` --
 * duplicated rather than imported since that function isn't exported and
 * this is a one-line, component-local concern in both places (same as
 * `expandOperatorForFiltering` being its own component-local helper
 * rather than shared). States explicitly that an empty selection filters
 * nothing, not "filters to nothing" -- the exact confusion that file's own
 * comment names. */
function countryChipLabel(selected: number): string {
  return selected === 0 ? 'Country — showing all' : `Country — ${selected} selected`;
}
```

- [ ] **Step 3: State.** Add alongside the existing
  `selectedOperators`/`sort` state (`:76-77`):

```typescript
const [selectedOperators, setSelectedOperators] = useState<string[]>([]);
const [selectedCountries, setSelectedCountries] = useState<Country[]>([]);
const [sort, setSort] = useState<SortState | null>(null);
const countryLabelId = useId();
```

- [ ] **Step 4: Derive `country` per row, and the present-countries set.**
  Current `rows` memo (`:99-115`):

```typescript
const rows = useMemo(
  () =>
    lines.map((line) => {
      const report = reportsById.get(line.id);
      const worst = report ? worstStatus(report) : undefined;
      const representative = representativeStatus(report?.lineStatuses ?? []);
      const stats = representative?.fullCoverageStats ?? representative?.sampleStats;
      const cancelledPct = cancelledPercent(stats);
      return { line, worst, stats, cancelledPct, representative };
    }),
  [lines, reportsById],
);
```

  Add `country`, derived from the same `report` lookup already in scope
  (`undefined` report → `Gb`, per Judgment Call 3 above — no separate
  branch needed since `countryForReport` only ever reads `.modeName`, so
  guard on `report` itself):

```typescript
const rows = useMemo(
  () =>
    lines.map((line) => {
      const report = reportsById.get(line.id);
      const worst = report ? worstStatus(report) : undefined;
      const representative = representativeStatus(report?.lineStatuses ?? []);
      const stats = representative?.fullCoverageStats ?? representative?.sampleStats;
      const cancelledPct = cancelledPercent(stats);
      // A line with no report at all has no modeName to derive a country
      // from; every such line today is GB by construction (see this
      // plan's Judgment Call 3 -- no non-GB LineSummary is reachable
      // through GET /public/lines yet), so it defaults the same way
      // countryForReport itself defaults an unrecognised modeName.
      const country: Country = report ? countryForReport(report) : 'Gb';
      return { line, worst, stats, cancelledPct, representative, country };
    }),
  [lines, reportsById],
);

// Mirrors operatorOptions's own "derive the option set from what's
// actually present" pattern (:88-97), over `rows` rather than raw `lines`
// since country is only knowable once a line is joined to its report.
// This is also this feature's self-hiding gate (Decision 4/5): a length
// of 1 (today, always exactly ['Gb']) means the filter control below does
// not render at all.
const countryOptions = useMemo(() => Array.from(new Set(rows.map((row) => row.country))).sort(), [rows]);
```

- [ ] **Step 5: AND-combine with the operator filter.** Current
  `filteredRows` memo (`:117-124`):

```typescript
const filteredRows = useMemo(() => {
  if (selectedOperators.length === 0) return rows;
  const expandedSelection = new Set(selectedOperators.flatMap(expandOperatorForFiltering));
  return rows.filter((row) => row.line.operators.some((op) => expandedSelection.has(op)));
}, [rows, selectedOperators]);
```

  Change to:

```typescript
const filteredRows = useMemo(() => {
  let result = rows;
  if (selectedOperators.length > 0) {
    // Expand the selection (e.g. "TfL" -> "TfL"/"LO"/"XR"), not each row's
    // own `operators` -- unchanged from before this task.
    const expandedSelection = new Set(selectedOperators.flatMap(expandOperatorForFiltering));
    result = result.filter((row) => row.line.operators.some((op) => expandedSelection.has(op)));
  }
  // AND-combined with the operator filter, not folded into it (Decision
  // 4): operator and country answer different questions, and a line has
  // exactly one country but potentially several operators.
  if (selectedCountries.length > 0) {
    result = result.filter((row) => selectedCountries.includes(row.country));
  }
  return result;
}, [rows, selectedOperators, selectedCountries]);
```

- [ ] **Step 6: Render the control.** After the existing `<MultiSelect
  .../>` block (`:169-178`), before `<Table>`:

```tsx
<MultiSelect
  label="Filter by operator"
  placeholder="All operators"
  data={operatorOptions}
  value={selectedOperators}
  onChange={setSelectedOperators}
  searchable
  clearable
  clearButtonProps={{ 'aria-label': 'Clear operator filter' }}
/>
{/* Self-hiding per Decision 4/5: with fewer than two countries present
    (today, always exactly ['Gb']) there is nothing meaningful to filter
    by, and a one-option control is worse than no control at all -- see
    docs/superpowers/specs/2026-09-05-country-filtering-design.md §5. */}
{countryOptions.length > 1 && (
  <Stack gap={4}>
    <Text id={countryLabelId} size="xs" fw={600} c="dimmed">
      {countryChipLabel(selectedCountries.length)}
    </Text>
    <ChipGroup multiple value={selectedCountries} onChange={(value) => setSelectedCountries(value as Country[])}>
      <Group gap="xs" role="group" aria-labelledby={countryLabelId}>
        {countryOptions.map((country) => (
          <Chip key={country} value={country} size="xs" variant={selectedCountries.includes(country) ? 'filled' : 'outline'}>
            {COUNTRY_LABELS[country]}
          </Chip>
        ))}
      </Group>
    </ChipGroup>
  </Stack>
)}
```

- [ ] **Step 7: Tests in `frontend/app/lines/AllLinesTable.test.tsx`.**

  First, add the file-scoped partial mock (near the existing
  `vi.mock('next/navigation', ...)` at the top of the file), and the new
  imports it needs:

```typescript
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, within, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AllLinesTable, expandOperatorForFiltering } from './AllLinesTable';
import type { LineStatusReport, LineSummary, Suggestion } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/lines',
  useSearchParams: () => new URLSearchParams(''),
}));

// Injects exactly one synthetic non-GB modeName mapping, on top of the
// real (today: empty) MODE_TO_COUNTRY -- see
// docs/superpowers/plans/2026-09-05-country-filtering-plan.md Judgment
// Call 2 for why this is the only way to exercise the "two countries
// present" path without a real Ireland Tier C poller. Mirrors the
// importOriginal partial-mock pattern already used in
// components/AutoRefresh.test.tsx:15-18. Harmless to every pre-existing
// test in this file: none of their fixtures use this modeName.
vi.mock('@/lib/modes', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/modes')>();
  return {
    ...actual,
    MODE_TO_COUNTRY: { ...actual.MODE_TO_COUNTRY, 'synthetic-ni-mode': 'NorthernIreland' },
  };
});
```

  Then, add a new `describe` block (anywhere after the existing ones,
  matching this file's own ordering-by-topic style):

```typescript
describe('AllLinesTable country filter', () => {
  it('does not render when every present line resolves to a single country (today\'s real state)', () => {
    renderTable(); // existing top-of-file fixture: every report is modeName: 'national-rail' -> Gb only
    expect(screen.queryByText(/^Country/)).not.toBeInTheDocument();
  });

  it('renders once a second country is present, and filters correctly', async () => {
    const countryLines: LineSummary[] = [
      { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['VT'], source: 'catalogue' },
      { id: 'synthetic-ni-line', name: 'Synthetic NI Line', category: 'Regional', operators: ['NI'], source: 'catalogue' },
    ];
    const countryReports: LineStatusReport[] = [
      report({ id: 'wcml', name: 'West Coast Main Line' }), // modeName defaults to 'national-rail' -> Gb
      report({ id: 'synthetic-ni-line', name: 'Synthetic NI Line', modeName: 'synthetic-ni-mode' }), // -> NorthernIreland, via the mock above
    ];
    renderWithMantine(
      <AllLinesTable lines={countryLines} reports={countryReports} pinnedLineIds={[]} tocs={[]} />,
    );

    expect(screen.getByText('Country — showing all')).toBeInTheDocument();
    const gbChip = screen.getByRole('button', { name: 'GB' });
    const niChip = screen.getByRole('button', { name: 'Northern Ireland' });

    fireEvent.click(niChip);
    expect(screen.getByText('Synthetic NI Line')).toBeInTheDocument();
    expect(screen.queryByText('West Coast Main Line')).not.toBeInTheDocument();
    expect(screen.getByText('Country — 1 selected')).toBeInTheDocument();

    fireEvent.click(gbChip);
    expect(screen.getByText('Synthetic NI Line')).toBeInTheDocument();
    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
    expect(screen.getByText('Country — 2 selected')).toBeInTheDocument();

    fireEvent.click(niChip); // toggle off, leaving only GB selected
    expect(screen.queryByText('Synthetic NI Line')).not.toBeInTheDocument();
    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
  });

  it('AND-combines with the operator filter rather than replacing it', async () => {
    const countryLines: LineSummary[] = [
      { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['VT'], source: 'catalogue' },
      { id: 'gwr', name: 'Great Western Railway', category: 'Long Distance', operators: ['GW'], source: 'catalogue' },
      { id: 'synthetic-ni-line', name: 'Synthetic NI Line', category: 'Regional', operators: ['VT'], source: 'catalogue' },
    ];
    const countryReports: LineStatusReport[] = [
      report({ id: 'wcml', name: 'West Coast Main Line' }),
      report({ id: 'gwr', name: 'Great Western Railway' }),
      report({ id: 'synthetic-ni-line', name: 'Synthetic NI Line', modeName: 'synthetic-ni-mode' }),
    ];
    renderWithMantine(
      <AllLinesTable lines={countryLines} reports={countryReports} pinnedLineIds={[]} tocs={[]} />,
    );

    // Filter to operator "VT" (matches wcml and synthetic-ni-line) AND
    // country "Gb" (matches wcml and gwr) -- intersection is wcml alone.
    const operatorInput = screen.getByRole('combobox', { name: 'Filter by operator' });
    fireEvent.click(operatorInput);
    fireEvent.click(await screen.findByRole('option', { name: 'VT' }));
    fireEvent.click(screen.getByRole('button', { name: 'GB' }));

    expect(screen.getByText('West Coast Main Line')).toBeInTheDocument();
    expect(screen.queryByText('Great Western Railway')).not.toBeInTheDocument();
    expect(screen.queryByText('Synthetic NI Line')).not.toBeInTheDocument();
  });
});
```

  (`report(...)` here is the existing helper already defined at the top of
  this test file, `:20-29` — no change needed there since it already
  defaults `modeName: 'national-rail'` and accepts an override via
  `overrides`.)

  Note on `screen.getByRole('button', { name: 'GB' })`: confirm during
  implementation which accessible role Mantine's `Chip` actually renders
  (`checkbox` in `multiple` mode is also plausible depending on the
  installed Mantine version) — adjust the role in these queries to match
  whatever a quick `screen.debug()`/`logRoles` run shows, rather than
  guessing; this is exactly the kind of thing this repo's own existing
  operator-filter tests are careful about (see the `readonly`-attribute
  comment at `AllLinesTable.test.tsx:112-116` for the same "verify against
  the real rendered output, don't assume" discipline).

- [ ] **Step 8: Verify**

```bash
cd frontend
npm test -- AllLinesTable.test.tsx
npm test  # full suite -- confirm the new @/lib/modes mock doesn't leak into or break any other test file
```

  Then, per this repo's own standing practice for UI changes (also
  captured in this plan's Global Constraints): start the dev server and
  check by eye.

```bash
cd frontend
npm run dev
```

  - Visit `/lines`. Confirm **no** country filter control renders anywhere
    on the page (today's real, all-GB state) — this is the primary thing
    to verify, since it's the state every real user sees today and the
    one place a bug here would be silently invisible in code review.
  - Temporarily edit `frontend/lib/modes.ts`'s `MODE_TO_COUNTRY` to
    `{ 'national-rail': 'NorthernIreland' }` (deliberately wrong — this
    just forces two distinct `country` values onto real, already-fetched
    rows so the control has something to render). Reload `/lines`.
    Confirm: the "Country" chip row now appears with two chips (labels
    depend on which real lines/reports load in dev — some will show `GB`,
    none will meaningfully show `Northern Ireland` correctly-labeled data,
    but the control's presence, chip toggling, and row filtering should
    all work); toggle chips and confirm the table narrows as expected.
  - **Revert the temporary `MODE_TO_COUNTRY` edit** before moving on —
    confirm with `git diff frontend/lib/modes.ts` that it's clean before
    the commit in Step 9.

- [ ] **Step 9: Commit**

```bash
git add frontend/app/lines/AllLinesTable.tsx frontend/app/lines/AllLinesTable.test.tsx
git commit -m "frontend: add a self-hiding country filter to AllLinesTable, AND-combined with the operator filter"
```

---

## References

- `docs/superpowers/specs/2026-09-05-country-filtering-design.md` — the
  approved design spec this plan implements in full.
- `docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md` §5/§7
  — Tier C no-go, the reason this surface has no real non-GB data today.
- `docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md:196,281`
  — confirms no Tier C plan exists yet and that `island-of-ireland-*` is
  illustrative, not a committed `modeName` value; cited in Judgment Call 2
  and this plan's own test comments as the reason not to guess a real
  value into `MODE_TO_COUNTRY`.
- `frontend/lib/modes.ts:35-43` — `MERGED_TFL_LINE_IDS`, the structural
  precedent Task 1's `MODE_TO_COUNTRY` mirrors.
- `frontend/app/lines/AllLinesTable.tsx:52-63,76,88-97,99-115,117-124,169-178`
  — the operator `MultiSelect`/`operatorOptions`/`rows`/`filteredRows`,
  the structural precedent Task 2 extends.
- `frontend/components/IssueList.tsx:127-133,152-153,254-291,293-306` —
  `chipRowLabel`, `severityFilter`/`sourceFilter`'s `ChipGroup multiple`
  usage (Judgment Call 1's chosen precedent), and `SegmentedControl`
  (Judgment Call 1's rejected alternative, with the reason).
- `frontend/components/AutoRefresh.test.tsx:15-18` — the `importOriginal`
  partial-mock pattern Task 2's test file reuses for `@/lib/modes`.
- `frontend/lib/types.ts:113-128` — `LineStatusReport`, confirming
  `modeName: string` is the real, current field name this plan's
  derivation functions read.
