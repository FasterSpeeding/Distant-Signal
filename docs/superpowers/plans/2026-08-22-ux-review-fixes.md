# UX Review Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Work through the 10-item punch list in the 2026-08-21 screenshot UX review — fixing the responsive layout failures that make the product's core content unreadable on a phone, the US date formatting in a UK rail app, the incoherent Active/Upcoming issue bucketing, the false "no disruptions" all-clear on an unknown CRS, the unbounded history page, and a set of layout/content/copy defects — without regressing anything the review called out as a deliberate improvement over spec.

**Architecture:** Four kinds of change, in dependency order. (1) Shared CSS groundwork in `app/globals.css` (asserted by `app/globals.test.ts`, the pattern this repo already uses for CSS that can't be expressed as a style object) plus a `data-status-badge` hook on `StatusBadge` — reused by the issue row, the All Lines table and the history timeline. (2) New pure, unit-tested `lib/` modules — `lib/validity.ts` (issue bucketing), `lib/dateFormat.ts` (one locale decision for the whole app), `lib/sampleStats.ts`, `lib/history.ts` (history collapsing + range resolution), `lib/stationIssues.ts` (cross-line de-duplication) — so the behaviour lives in testable functions and the Server Components stay thin. (3) Prop-shape changes to `IssueList` (`now`, then `items`) threaded from the Server Component pages. (4) Layout chrome in `app/layout.tsx`.

**Tech Stack:** Next.js 16 App Router (Server + Client Components), Mantine v9.4.1 (`@mantine/core`, `@mantine/dates`), TypeScript, Vitest + React Testing Library, `postcss-preset-mantine` + `postcss-simple-vars`.

## Global Constraints

- **Rationale lives here, not in a separate spec file.** Most of this punch list is layout/CSS, locale formatting, copy and bug fixes — too small to earn a design doc. The two tasks that involve a genuine design tradeoff (Task 8, the history page; Task 10, station-page de-duplication) each carry a **Decision** note stating what was chosen and why. Everything else is justified inline in the task description.

- **Do not revert what the review called a purposeful, good deviation from spec.** Specifically: the status/avg-delay/cancelled columns and sorting on `/lines` (which `2026-07-09-frontend-personalization-design.md` said should be "deliberately brief"); the Edit/Delete gating and red destructive styling on `/lines/{id}`; the labelled filter chips and filled/outline chip states in `IssueList`; the click/tap-to-open freshness and line-definition tooltips; the pin toggle's two-state styling and stateful tooltip label. Several tasks build *on* these; none undo them.

- **Two review findings were checked against the real source and are deliberately NOT planned.** See "Findings corrected or dropped" at the end of this document before starting — do not open work on them.

- **Hydration safety is a hard requirement, and two of these fixes are hydration fixes.** This app already fought this class of bug twice (`ThemeToggle`, `LastUpdated` — both carry long comments about it). `new Date(x).toLocaleDateString()` with no explicit locale/timezone, which `IssueList` and the history page both use today, renders `5/10/2026` on the server (Node in this container resolves to `en-US`/`UTC` — verified) and something else in a UK browser, so it is *already* a live server/client mismatch as well as a correctness bug. Every date in this plan goes through an explicit `Intl.DateTimeFormat('en-GB', { timeZone: 'Europe/London', … })`. Likewise, issue bucketing must not call `Date.now()` inside a Client Component: the current instant is stamped once by the Server Component and passed down as a `now: number` prop, and `AutoRefresh` (`router.refresh()` every 30s) re-stamps it.

- **`IssueList`'s props change twice, on purpose.** Task 4 adds `now: number`; Task 10 replaces `statuses: LineStatus[]` with `items: IssueItem[]`. Both are mechanical, both are covered by `IssueList.test.tsx`, and both call sites (`app/lines/[id]/page.tsx`, `app/stations/[crs]/page.tsx`) are updated in the same task. This is deliberate rather than doing one big prop change up front, so Phase 0 stays shippable on its own. Callbacks/functions can never be added as `IssueList` props — it is a Client Component receiving props from Server Components, so everything crossing that boundary must be serializable.

- **Responsive behaviour is done in CSS, not `useMediaQuery`.** `@mantine/hooks`' `useMediaQuery` returns `false` during SSR, which would reintroduce exactly the hydration problem above. Use `app/globals.css` media queries (`$mantine-breakpoint-sm` etc. are wired up in `postcss.config.cjs`) or Mantine's `visibleFrom`/`hiddenFrom` Box props, which are implemented as `display: none !important` inside a `<style>` tag `MantineProvider` renders on both server and client (verified in `node_modules/@mantine/core/esm/core/MantineProvider/MantineClasses/MantineClasses.mjs`).

- **Testing `visibleFrom`/`hiddenFrom` and media queries in jsdom.** jsdom does not evaluate media queries, so both the mobile and desktop variants of a responsive element are present in the test DOM. Assert on the *class* (`mantine-visible-from-sm`) or use `getAllByText`, never `getByText` alone, on elements that have a responsive twin. Where the behaviour is purely a stylesheet rule, assert it in `app/globals.test.ts` against the raw CSS source (this file already does this for the link-colour and underline rules) — assertions must match the **unprocessed** source, i.e. `$mantine-breakpoint-sm`, not `48em`.

- **Test-file inventory, verified.** Every file in `components/` and `lib/` has a `.test.ts(x)` sibling **except** `lib/types.ts`, `lib/suggestions.ts` and `lib/useSuggestions.ts` (the last has a test but no… — re-confirmed: `useSuggestions.test.ts` exists, `suggestions.ts` and `types.ts` have none). **No file under `app/` has a component test** — the only `app/` test is `app/globals.test.ts`, which reads CSS text. Server Component pages are therefore verified by hand against the running stack, exactly as `2026-07-10-outage-page-redesign.md` did. New `lib/` modules in this plan all get tests; new/changed `app/` pages do not.

- **Commit after each task**, using the command given in the task's final step.

---

## Phase 0 — Bugs a user can hit today

### Task 1: Stop status badges being reduced to colour alone, and give the issue row a mobile layout

**Files:**
- Modify: `frontend/components/StatusBadge.tsx`
- Modify: `frontend/app/globals.css`
- Modify: `frontend/app/globals.test.ts`
- Modify: `frontend/components/StatusBadge.test.tsx`

**Interfaces:**
- Produces: a `data-status-badge` attribute on every `StatusBadge` root, and the `.issueRow*` class family in `app/globals.css`. Consumed by Task 2 (`IssueList`), Task 3 (`AllLinesTable`) and Task 8 (history timeline).

This is the shared groundwork for the two worst findings in the review, which have the same root cause. Mantine's `Badge` root sets `overflow: hidden; text-overflow: ellipsis` (verified in `node_modules/@mantine/core/styles/Badge.css`, class `m_347db0ec`). The visible consequence is `Good Service` rendering as `G…` in the All Lines table at 390px. The *invisible* consequence is worse: an element with `overflow: hidden` contributes a min-content size of zero, so any flex or table container under width pressure is free to squeeze the badge to nothing — which is also half of why the issue row collapses. Severity is this product's primary information and colour must not be the only thing carrying it.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/StatusBadge.test.tsx`:

```typescript
  it('marks the badge so it can be opted out of Mantine\'s label truncation', () => {
    const { container } = renderWithMantine(<StatusBadge severity={10} />);
    expect(container.querySelector('[data-status-badge]')).not.toBeNull();
  });
```

Add to `frontend/app/globals.test.ts`:

```typescript
describe('status badge truncation opt-out', () => {
  // Mantine's Badge root carries `overflow: hidden` + `text-overflow:
  // ellipsis`, which clipped "Good Service" to "G…" in the All Lines table
  // at 390px — colour alone then carried the status (WCAG 1.4.1). It also
  // collapses the badge's min-content contribution to zero, which is what
  // let a flex row squeeze the badge past its own width and paint it over
  // the date range on the line detail page.
  it('opts status badges out of overflow clipping, root and label', () => {
    const rule = css.match(/\[data-status-badge\][\s\S]*?\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('overflow: visible');
    expect(rule![0]).toContain('text-overflow: clip');
  });
});

describe('collapsed issue row layout', () => {
  it('lays the row out as a single flex line by default', () => {
    const rule = css.match(/\.issueRow\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('display: flex');
    expect(rule![0]).toContain('justify-content: space-between');
  });

  it('stacks the row into two lines below the sm breakpoint', () => {
    const query = css.match(
      /@media \(max-width: \$mantine-breakpoint-sm\)\s*\{[\s\S]*?\n\}/,
    );
    expect(query).not.toBeNull();
    expect(query![0]).toContain('.issueRow {');
    expect(query![0]).toContain('flex-direction: column');
  });

  it('lets the reason wrap to two clamped lines on mobile instead of truncating to nothing', () => {
    expect(css).toContain('-webkit-line-clamp: 2');
  });

  it('never lets the severity badge shrink out of the row', () => {
    const rule = css.match(/\.issueRow__badge\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('flex-shrink: 0');
  });
});
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && npx vitest run app/globals.test.ts components/StatusBadge.test.tsx`
Expected: the 5 new tests FAIL (no `data-status-badge` attribute, no `.issueRow` rules); every pre-existing test in both files still PASSES.

- [ ] **Step 3: Implement**

Replace `frontend/components/StatusBadge.tsx` entirely with:

```tsx
import { Badge } from '@mantine/core';
import { severityColor, severityLabel } from '@/lib/severity';

/** `data-status-badge` is the hook for the rule in `app/globals.css` that
 * opts this badge out of Mantine's `overflow: hidden` +
 * `text-overflow: ellipsis` — see that rule's comment. Kept as a plain
 * data attribute rather than a `className` so a call site can still pass
 * its own `className` without having to remember to merge this one in. */
export function StatusBadge({ severity }: { severity: number }) {
  return (
    <Badge color={severityColor(severity)} variant="filled" data-status-badge>
      {severityLabel(severity)}
    </Badge>
  );
}
```

Append to `frontend/app/globals.css`:

```css
/* Mantine's Badge root sets `overflow: hidden` + `text-overflow: ellipsis`
   (see `@mantine/core/styles/Badge.css`, `.m_347db0ec`). Two things went
   wrong with that here. Visibly, the All Lines table at 390px rendered
   "Good Service" as "G…" and "Severe Delays" as "SE…", leaving colour as
   the only carrier of the status — a WCAG 1.4.1 failure on the page's main
   content. Less visibly, `overflow: hidden` makes an element's min-content
   contribution zero, so a flex row or a table column under width pressure
   is free to squeeze the badge below its own width; on the line detail
   page that let the severity badge (pinned with `flex-shrink: 0`) overflow
   its group and paint straight over the validity dates.

   Both roots are the same override. The descendant half targets Mantine's
   `.m_5add502a` label span, which repeats the same three properties. */
[data-status-badge],
[data-status-badge] :where(*) {
  overflow: visible;
  text-overflow: clip;
}

/* Collapsed issue row — `components/IssueList.tsx`, and reused by the
   history timeline. Previously a single Mantine `Group` with
   `wrap="nowrap"`: severity badge + reason + validity dates + source
   badge, with only the reason allowed to shrink. That has no viable layout
   at 390px — the intrinsic width of badge + dates + source badge already
   exceeds the viewport, so the reason truncated to nothing and the badge
   overflowed onto the dates. Below `sm` the row becomes two stacked lines
   instead. Plain classes rather than Mantine `Group`/`Box` props because
   the breakpoint behaviour has to be a media query, not a style object. */
.issueRow {
  display: flex;
  width: 100%;
  min-width: 0;
  align-items: center;
  justify-content: space-between;
  gap: var(--mantine-spacing-xs);
}

.issueRow__main {
  display: flex;
  min-width: 0;
  align-items: center;
  gap: var(--mantine-spacing-xs);
}

/* Classification and provenance are fixed; the reason is the only thing
   that gives way on a wide screen. */
.issueRow__badge {
  flex-shrink: 0;
}

.issueRow__meta {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: var(--mantine-spacing-xs);
  white-space: nowrap;
}

.issueRow__reason {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

@media (max-width: $mantine-breakpoint-sm) {
  .issueRow {
    flex-direction: column;
    align-items: flex-start;
  }

  .issueRow__main {
    width: 100%;
    flex-wrap: wrap;
  }

  /* Two wrapped lines of reason beats one line of "Go…". There is room for
     them once the row is allowed to stack, and the clamp keeps a
     paragraph-long planned-works description from turning every row into a
     wall — the expanded panel carries the full text. */
  .issueRow__reason {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    white-space: normal;
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run app/globals.test.ts components/StatusBadge.test.tsx`
Expected: PASS — all tests in both files.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/StatusBadge.tsx frontend/components/StatusBadge.test.tsx frontend/app/globals.css frontend/app/globals.test.ts
git commit -m "Stop status badges truncating, and add a mobile issue-row layout"
```

---

### Task 2: Apply the two-line issue row to `IssueList`

**Files:**
- Modify: `frontend/components/IssueList.tsx`
- Modify: `frontend/components/IssueList.test.tsx`

**Interfaces:** No prop changes. Consumes the `.issueRow*` classes from Task 1.

Punch-list item 1, the worst finding in the sweep (`line-detail_standard-severe-delays__mobile-390x844.png`, `line-detail_custom-with-data__mobile-390x844.png`, `station-detail_with-disruptions__mobile-390x844.png`). Replaces the nested Mantine `Group`s inside `AccordionControl` with the class-based row, and deletes the inline `flexShrink`/`minWidth` style objects they carried (those properties now live in the stylesheet, where the media query can override them).

- [ ] **Step 1: Write the failing test**

Add to `frontend/components/IssueList.test.tsx`:

```typescript
  it('marks up the collapsed row so it can stack on narrow viewports', () => {
    const { container } = renderWithMantine(<IssueList statuses={[minorNow]} />);
    const row = container.querySelector('.issueRow');
    expect(row).not.toBeNull();
    expect(row!.querySelector('.issueRow__badge')).not.toBeNull();
    expect(row!.querySelector('.issueRow__reason')).not.toBeNull();
    expect(row!.querySelector('.issueRow__meta')).not.toBeNull();
  });

  it('does not pin the row with an inline nowrap that a media query cannot override', () => {
    const { container } = renderWithMantine(<IssueList statuses={[minorNow]} />);
    const row = container.querySelector('.issueRow') as HTMLElement;
    expect(row.style.flexWrap).toBe('');
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/IssueList.test.tsx`
Expected: the 2 new tests FAIL (`.issueRow` is not in the DOM); the pre-existing tests PASS.

- [ ] **Step 3: Implement**

In `frontend/components/IssueList.tsx`, replace the whole `<AccordionControl>…</AccordionControl>` block (currently a `Group justify="space-between" wrap="nowrap"` containing two nested `Group`s, a `Box`, a `Text truncate` and a `Badge`) with:

```tsx
            <AccordionControl>
              {/*
                The badges are the row's classification and provenance, so
                they hold their width; the reason is the only element that
                gives way. All of that — and the two-line stack below `sm` —
                lives in `app/globals.css`'s `.issueRow*` rules rather than
                in Mantine `Group` props, because a breakpoint cannot be
                expressed as a style object and the previous inline
                `flexShrink`/`minWidth`/`wrap="nowrap"` values would have
                outranked any media query that tried.
              */}
              <div className="issueRow">
                <div className="issueRow__main">
                  <div className="issueRow__badge">
                    <StatusBadge severity={status.statusSeverity} />
                  </div>
                  <Text size="sm" className="issueRow__reason">
                    {status.reason}
                  </Text>
                </div>
                <div className="issueRow__meta">
                  <Text size="xs" c="dimmed">
                    {formatValiditySummary(status)}
                  </Text>
                  {/*
                    Explicit gray: without a `color`, Mantine falls back to
                    theme.primaryColor (grape), making this read as branded
                    or interactive. It's provenance, not brand — gray is
                    already how `informational` severity is treated in
                    lib/severity.ts's GROUP_COLOR.
                  */}
                  <Badge variant="outline" size="sm" color="gray">
                    {DATA_QUALITY_LABELS[status.dataQuality]}
                  </Badge>
                </div>
              </div>
            </AccordionControl>
```

Then remove `Box` and `Group` from the `@mantine/core` import list **only if** nothing else in the file still uses them — as of this task `Group` is still used by both `ChipGroup` rows, so only `Box` becomes unused. Remove `Box`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run components/IssueList.test.tsx`
Expected: PASS — the full file, new and pre-existing.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/IssueList.tsx frontend/components/IssueList.test.tsx
git commit -m "Give the collapsed issue row a two-line layout below the sm breakpoint"
```

---

### Task 3: Give Status priority over the numeric columns on All Lines at mobile widths

**Files:**
- Modify: `frontend/app/lines/AllLinesTable.tsx`
- Create: `frontend/app/lines/AllLinesTable.test.tsx`

**Interfaces:** No prop changes. Depends on Task 1's badge fix (without it, restoring the column's width still leaves the label ellipsized).

Punch-list item 2. Task 1 stops the badge clipping, but at 390px five columns still do not fit — the fix has to also drop the two numeric columns and re-surface their numbers under the line name. `TableTh`/`TableTd` extend `BoxProps` (verified in `node_modules/@mantine/core/lib/components/Table/Table.components.d.ts`), so `visibleFrom`/`hiddenFrom` are available on them.

Note this file has **no** test today; this task creates the first one. It is a Client Component, so it renders under `renderWithMantine` without any server plumbing.

- [ ] **Step 1: Write the failing test**

Create `frontend/app/lines/AllLinesTable.test.tsx`:

```typescript
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AllLinesTable } from './AllLinesTable';
import type { LineStatusReport, LineSummary } from '@/lib/types';

const lines: LineSummary[] = [
  { id: 'northern', name: 'Northern', category: 'operator', operators: ['NT'], source: 'catalogue' },
];

const reports: LineStatusReport[] = [
  {
    $type: 'NRStatus.LineStatusReport',
    id: 'northern',
    name: 'Northern',
    modeName: 'national-rail',
    operators: ['NT'],
    computedAt: '2026-08-21T12:00:00Z',
    lineStatuses: [
      {
        statusSeverity: 6,
        statusSeverityDescription: 'Severe Delays',
        reason: 'Signalling failure',
        dataQuality: 'ldbws-inferred',
        validityPeriods: [{ fromDate: '2026-08-21T09:00:00Z', toDate: null, isNow: true }],
        sampleStats: { total: 10, delayed: 4, cancelled: 2, skipped: 0, avgDelayMinutes: 7.5 },
      },
    ],
  },
];

function renderTable() {
  return renderWithMantine(
    <AllLinesTable lines={lines} reports={reports} pinnedLineIds={[]} tocs={[]} />,
  );
}

describe('AllLinesTable responsive columns', () => {
  it('renders the full status label, never an ellipsized stub', () => {
    renderTable();
    expect(screen.getByText('Severe Delays')).toBeInTheDocument();
  });

  it('hides the numeric columns below the sm breakpoint', () => {
    const { container } = renderTable();
    const hidden = Array.from(container.querySelectorAll('.mantine-visible-from-sm'));
    const text = hidden.map((el) => el.textContent);
    expect(text.some((t) => t?.includes('Avg Delay'))).toBe(true);
    expect(text.some((t) => t?.includes('Cancelled'))).toBe(true);
  });

  it('re-surfaces the numbers under the line name for the widths that lose the columns', () => {
    const { container } = renderTable();
    const mobileOnly = container.querySelector('.mantine-hidden-from-sm');
    expect(mobileOnly).not.toBeNull();
    expect(mobileOnly!.textContent).toContain('7.5 min');
    expect(mobileOnly!.textContent).toContain('20%');
  });

  it('says so explicitly when a line has no sample data', () => {
    const { container } = renderWithMantine(
      <AllLinesTable lines={lines} reports={[]} pinnedLineIds={[]} tocs={[]} />,
    );
    expect(container.querySelector('.mantine-hidden-from-sm')!.textContent).toContain('No sample data');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run app/lines/AllLinesTable.test.tsx`
Expected: FAIL — no `mantine-visible-from-sm`/`mantine-hidden-from-sm` elements exist yet. (The first test, "renders the full status label", should already PASS thanks to Task 1 — it is a regression guard, not a new behaviour.)

- [ ] **Step 3: Implement**

In `frontend/app/lines/AllLinesTable.tsx`, add `Text` is already imported. Change the two numeric headers to be desktop-only:

```tsx
            <TableTh {...headerProps('avgDelay')} visibleFrom="sm">
              Avg Delay{sortIndicator('avgDelay', sort)}
            </TableTh>
            <TableTh {...headerProps('cancelled')} visibleFrom="sm">
              Cancelled{sortIndicator('cancelled', sort)}
            </TableTh>
```

Change the name cell to carry a mobile-only stat line, and make the two numeric cells match their headers:

```tsx
              <TableTd>
                <TextLink href={`/lines/${line.id}`}>{line.name}</TextLink>
                {/* At 390px five columns cannot all fit, and the one that
                    was losing was Status — the page's whole point — while
                    two numeric columns kept their width. Below `sm` the
                    numbers move here instead of disappearing; `visibleFrom`/
                    `hiddenFrom` are Mantine's `display: none` classes,
                    emitted by MantineProvider on server and client alike,
                    so this is SSR-safe (unlike `useMediaQuery`). */}
                <Text size="xs" c="dimmed" hiddenFrom="sm">
                  {stats
                    ? `Avg ${stats.avgDelayMinutes.toFixed(1)} min · ${cancelledPct}% cancelled`
                    : 'No sample data'}
                </Text>
              </TableTd>
              <TableTd>{worst ? <StatusBadge severity={worst.statusSeverity} /> : null}</TableTd>
              <TableTd visibleFrom="sm">
                {/* …unchanged body… */}
              </TableTd>
              <TableTd visibleFrom="sm">
                {/* …unchanged body… */}
              </TableTd>
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run app/lines/AllLinesTable.test.tsx`
Expected: PASS — all 4 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/AllLinesTable.tsx frontend/app/lines/AllLinesTable.test.tsx
git commit -m "Give Status priority over numeric columns on All Lines at mobile widths"
```

---

### Task 4: Make every issue land in a bucket a human can explain

**Files:**
- Create: `frontend/lib/validity.ts`
- Create: `frontend/lib/validity.test.ts`
- Modify: `frontend/components/IssueList.tsx`
- Modify: `frontend/components/IssueList.test.tsx`
- Modify: `frontend/app/lines/[id]/page.tsx`
- Modify: `frontend/app/stations/[crs]/page.tsx`

**Interfaces:**
- Produces: `periodIsActive`, `periodIsUpcoming`, `bucketFor`, `governingPeriod` in `lib/validity.ts`. Consumed by `IssueList` here and by Task 6's validity formatting.
- Changes: `IssueList({ statuses, now })` — `now` is epoch milliseconds, stamped by the Server Component.

Punch-list item 4. The review guessed at the cause; the real one is confirmed in the backend source and is worth stating precisely because the fix depends on it. `crates/poller-incidents/src/schema.rs:97` sets `is_now: vp.end_time.is_none()` — the flag means "**open-ended**", not "covers now". The aggregator's `validity_for_output` (`crates/aggregator/src/aggregation.rs:158`) then correctly *selects* the period covering now via `period_covers_now`, but copies the poller's flag through untouched. So every in-progress planned work with a known end date arrives at the frontend as `isNow: false`, and `IssueList`'s `isActive` (which reads `isNow` and nothing else) buckets it nowhere: hence `All (3) / Active (0) / Upcoming (0)` on Northern, West Barnes Drain and every Woking section. `isUpcoming` has a second, independent bug — it inspects only `validityPeriods[0]`, which stopped being safe when multi-period extraction landed (`2026-08-21-multi-period-extraction-design.md`).

Fixing this in the backend would be the deeper fix but is out of scope for a frontend plan; the frontend fix (fall back to the dates when `isNow` is false) is correct regardless of what the backend later does with the flag, because `isNow: true` remains sufficient-but-not-necessary for "active".

Counts must also add up, which they cannot with three buckets: a period that has already ended is neither active nor upcoming. A fourth **Ended** segment appears only when something is actually in it, so the common case still shows three.

- [ ] **Step 1: Write the failing test**

Create `frontend/lib/validity.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { bucketFor, governingPeriod, periodIsActive, periodIsUpcoming } from './validity';
import type { LineStatus, ValidityPeriod } from './types';

const NOW = Date.parse('2026-08-21T12:00:00Z');
const HOUR = 3_600_000;

function period(overrides: Partial<ValidityPeriod> = {}): ValidityPeriod {
  return { fromDate: new Date(NOW - HOUR).toISOString(), toDate: null, isNow: false, ...overrides };
}

function status(periods: ValidityPeriod[]): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Engineering works',
    dataQuality: 'planned',
    validityPeriods: periods,
  };
}

describe('periodIsActive', () => {
  it('trusts isNow when it is set', () => {
    expect(periodIsActive(period({ fromDate: new Date(NOW + HOUR).toISOString(), isNow: true }), NOW)).toBe(true);
  });

  it('treats a period that started in the past and has not ended as active, despite isNow being false', () => {
    // The exact shape the backend produces for in-progress planned works:
    // `is_now` is derived from "has no end time", so a dated window that
    // spans right now still arrives false.
    const spanning = period({
      fromDate: new Date(NOW - 5 * HOUR).toISOString(),
      toDate: new Date(NOW + 5 * HOUR).toISOString(),
      isNow: false,
    });
    expect(periodIsActive(spanning, NOW)).toBe(true);
  });

  it('treats an open-ended period that started in the past as active', () => {
    expect(periodIsActive(period({ toDate: null }), NOW)).toBe(true);
  });

  it('does not treat a finished period as active', () => {
    const ended = period({
      fromDate: new Date(NOW - 5 * HOUR).toISOString(),
      toDate: new Date(NOW - HOUR).toISOString(),
    });
    expect(periodIsActive(ended, NOW)).toBe(false);
  });

  it('does not treat a future period as active', () => {
    expect(periodIsActive(period({ fromDate: new Date(NOW + HOUR).toISOString() }), NOW)).toBe(false);
  });

  it('falls back to active rather than silently dropping an unparseable toDate', () => {
    expect(periodIsActive(period({ toDate: 'not a date' }), NOW)).toBe(true);
  });
});

describe('periodIsUpcoming', () => {
  it('is true only for a period that has not started', () => {
    expect(periodIsUpcoming(period({ fromDate: new Date(NOW + HOUR).toISOString() }), NOW)).toBe(true);
    expect(periodIsUpcoming(period(), NOW)).toBe(false);
  });
});

describe('bucketFor', () => {
  it('buckets an in-progress planned work as active', () => {
    const spanning = period({
      fromDate: new Date(NOW - HOUR).toISOString(),
      toDate: new Date(NOW + HOUR).toISOString(),
    });
    expect(bucketFor(status([spanning]), NOW)).toBe('active');
  });

  it('buckets a wholly future window as upcoming', () => {
    const future = period({
      fromDate: new Date(NOW + HOUR).toISOString(),
      toDate: new Date(NOW + 2 * HOUR).toISOString(),
    });
    expect(bucketFor(status([future]), NOW)).toBe('upcoming');
  });

  it('buckets a wholly past window as ended', () => {
    const past = period({
      fromDate: new Date(NOW - 2 * HOUR).toISOString(),
      toDate: new Date(NOW - HOUR).toISOString(),
    });
    expect(bucketFor(status([past]), NOW)).toBe('ended');
  });

  it('checks every period, not just the first', () => {
    const past = period({
      fromDate: new Date(NOW - 3 * HOUR).toISOString(),
      toDate: new Date(NOW - 2 * HOUR).toISOString(),
    });
    const future = period({ fromDate: new Date(NOW + HOUR).toISOString() });
    expect(bucketFor(status([past, future]), NOW)).toBe('upcoming');
  });

  it('treats a status carrying no validity information at all as active', () => {
    // Matches the aggregator's own fallback for an incident with no
    // validity period (`from_date: now, to_date: None, is_now: true`) —
    // "we do not know when" must not read as "it is over".
    expect(bucketFor(status([]), NOW)).toBe('active');
  });
});

describe('governingPeriod', () => {
  it('prefers the period covering now', () => {
    const past = period({
      fromDate: new Date(NOW - 3 * HOUR).toISOString(),
      toDate: new Date(NOW - 2 * HOUR).toISOString(),
    });
    const current = period({ fromDate: new Date(NOW - HOUR).toISOString() });
    expect(governingPeriod(status([past, current]), NOW)).toBe(current);
  });

  it('falls back to the soonest future period', () => {
    const later = period({ fromDate: new Date(NOW + 5 * HOUR).toISOString() });
    const sooner = period({ fromDate: new Date(NOW + HOUR).toISOString() });
    expect(governingPeriod(status([later, sooner]), NOW)).toBe(sooner);
  });

  it('returns undefined when there is nothing to describe', () => {
    expect(governingPeriod(status([]), NOW)).toBeUndefined();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/validity.test.ts`
Expected: FAIL to resolve — `./validity` does not exist.

- [ ] **Step 3: Implement `lib/validity.ts`**

Create `frontend/lib/validity.ts`:

```typescript
import type { LineStatus, ValidityPeriod } from './types';

/** Every issue lands in exactly one of these, so the tab counts add up. */
export type IssueBucket = 'active' | 'upcoming' | 'ended';

/** `isNow` does NOT mean "this period covers now".
 *
 * `crates/poller-incidents/src/schema.rs` builds it as
 * `is_now: vp.end_time.is_none()` — i.e. "open-ended". The aggregator's
 * `validity_for_output` then does pick the period covering now (via
 * `period_covers_now`) but copies that flag through untouched, so every
 * in-progress planned work with a known end date reaches the frontend as
 * `isNow: false`. Reading only the flag is what produced the incoherent
 * "All (3) / Active (0) / Upcoming (0)" on the line and station pages.
 *
 * So: `isNow` is treated as sufficient but not necessary, with the dates
 * as the real test. An unparseable `toDate` resolves to "still active"
 * rather than silently dropping the issue out of every bucket — the same
 * bias towards surfacing rather than hiding that the rest of this
 * component uses. */
export function periodIsActive(period: ValidityPeriod, now: number): boolean {
  if (period.isNow) return true;
  const from = Date.parse(period.fromDate);
  if (Number.isNaN(from) || from > now) return false;
  if (period.toDate === null) return true;
  const to = Date.parse(period.toDate);
  return Number.isNaN(to) || to >= now;
}

export function periodIsUpcoming(period: ValidityPeriod, now: number): boolean {
  if (periodIsActive(period, now)) return false;
  const from = Date.parse(period.fromDate);
  return !Number.isNaN(from) && from > now;
}

/** Checks every period, not just `validityPeriods[0]` — the previous code
 * read only the first, which stopped being safe once incidents could carry
 * several periods (see
 * `docs/superpowers/specs/2026-08-21-multi-period-extraction-design.md`). */
export function bucketFor(status: LineStatus, now: number): IssueBucket {
  // No validity information at all is not the same as "it's over": the
  // aggregator's own fallback for an incident without a validity period is
  // an open-ended one starting now, so match that.
  if (status.validityPeriods.length === 0) return 'active';
  if (status.validityPeriods.some((period) => periodIsActive(period, now))) return 'active';
  if (status.validityPeriods.some((period) => periodIsUpcoming(period, now))) return 'upcoming';
  return 'ended';
}

/** The period a collapsed row should describe: whichever one covers now,
 * else the soonest one still to come, else the earliest on record. */
export function governingPeriod(status: LineStatus, now: number): ValidityPeriod | undefined {
  const active = status.validityPeriods.find((period) => periodIsActive(period, now));
  if (active) return active;

  const upcoming = status.validityPeriods
    .filter((period) => periodIsUpcoming(period, now))
    .sort((a, b) => Date.parse(a.fromDate) - Date.parse(b.fromDate));
  if (upcoming.length > 0) return upcoming[0];

  return [...status.validityPeriods].sort((a, b) => Date.parse(a.fromDate) - Date.parse(b.fromDate))[0];
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run lib/validity.test.ts`
Expected: PASS — all tests.

- [ ] **Step 5: Write the failing `IssueList` tests**

In `frontend/components/IssueList.test.tsx`, add a shared `NOW` constant near the existing fixtures and thread `now={NOW}` through every `renderWithMantine(<IssueList … />)` call (the existing fixtures build their dates from `Date.now()`, so define `const NOW = Date.now();` and reuse it). Then add:

```typescript
  it('counts an in-progress dated window as Active even though isNow is false', () => {
    // The exact shape that produced "All (1) / Active (0) / Upcoming (0)".
    const inProgress: LineStatus = {
      statusSeverity: 4,
      statusSeverityDescription: 'Planned Closure',
      reason: 'Station improvement work',
      dataQuality: 'planned',
      validityPeriods: [
        {
          fromDate: new Date(NOW - 86400000).toISOString(),
          toDate: new Date(NOW + 86400000).toISOString(),
          isNow: false,
        },
      ],
    };
    renderWithMantine(<IssueList statuses={[inProgress]} now={NOW} />);
    expect(screen.getByText('Active (1)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (0)')).toBeInTheDocument();
  });

  it('makes the tab counts add up', () => {
    const ended: LineStatus = {
      statusSeverity: 9,
      statusSeverityDescription: 'Minor Delays',
      reason: 'Finished works',
      dataQuality: 'planned',
      validityPeriods: [
        {
          fromDate: new Date(NOW - 2 * 86400000).toISOString(),
          toDate: new Date(NOW - 86400000).toISOString(),
          isNow: false,
        },
      ],
    };
    renderWithMantine(<IssueList statuses={[...all, ended]} now={NOW} />);
    expect(screen.getByText('All (4)')).toBeInTheDocument();
    expect(screen.getByText('Active (2)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (1)')).toBeInTheDocument();
    expect(screen.getByText('Ended (1)')).toBeInTheDocument();
  });

  it('hides the Ended tab entirely when nothing has ended', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.queryByText(/^Ended/)).not.toBeInTheDocument();
  });
```

- [ ] **Step 6: Run tests to verify they fail**

Run: `cd frontend && npx vitest run components/IssueList.test.tsx`
Expected: the 3 new tests FAIL (`Active (0)` is rendered, there is no Ended segment); the rest PASS.

- [ ] **Step 7: Implement in `IssueList`**

In `frontend/components/IssueList.tsx`:

Replace the local `isUpcoming`/`isActive`/`sortRank` helpers with the shared module:

```tsx
import { bucketFor, governingPeriod, type IssueBucket } from '@/lib/validity';
```

```tsx
type ActiveFilter = 'all' | IssueBucket;

const BUCKET_SORT_RANK: Record<IssueBucket, number> = { active: 0, upcoming: 1, ended: 2 };
```

Change the component signature and the derived state:

```tsx
export function IssueList({ statuses, now }: { statuses: LineStatus[]; now: number }) {
```

```tsx
  // `now` is stamped once by the Server Component page and passed in,
  // rather than read from `Date.now()` here. That keeps the server-rendered
  // markup and the client's pre-hydration render byte-identical — the same
  // constraint `LastUpdated` and `ThemeToggle` document — while still
  // letting the buckets depend on real dates. `AutoRefresh`'s 30s
  // `router.refresh()` re-stamps it, so it does not go stale.
  const buckets = useMemo(
    () => new Map(statuses.map((status) => [status, bucketFor(status, now)])),
    [statuses, now],
  );
```

(add `useMemo` to the `react` import).

Landing-tab initialiser, keeping its existing "never recompute" semantics:

```tsx
  const [activeFilter, setActiveFilter] = useState<ActiveFilter>(() =>
    statuses.some((status) => bucketFor(status, now) === 'active') ? 'active' : 'all',
  );
```

Counts and segments:

```tsx
  const activeCount = chipFiltered.filter((s) => buckets.get(s) === 'active').length;
  const upcomingCount = chipFiltered.filter((s) => buckets.get(s) === 'upcoming').length;
  const endedCount = chipFiltered.filter((s) => buckets.get(s) === 'ended').length;

  const filtered = chipFiltered
    .filter((status) => activeFilter === 'all' || buckets.get(status) === activeFilter)
    .sort(compareByUrgency);
```

`compareByUrgency`/`sortRank` now read the bucket map:

```tsx
  function sortRank(status: LineStatus): number {
    return BUCKET_SORT_RANK[buckets.get(status) ?? 'ended'];
  }
```

and `earliestFromDate` becomes `governingPeriod(status, now)`-based:

```tsx
  function earliestFromDate(status: LineStatus): number {
    const period = governingPeriod(status, now);
    return period ? Date.parse(period.fromDate) : Infinity;
  }
```

`SegmentedControl` data becomes conditional — an empty Ended bucket contributes no segment, so the common case is unchanged:

```tsx
        <SegmentedControl
          value={activeFilter}
          onChange={(value) => setActiveFilter(value as ActiveFilter)}
          data={[
            { label: `All (${chipFiltered.length})`, value: 'all' },
            { label: `Active (${activeCount})`, value: 'active' },
            { label: `Upcoming (${upcomingCount})`, value: 'upcoming' },
            // Only offered when something is actually in it. Three buckets
            // could never make the counts add up — an issue whose window
            // has closed is neither active nor upcoming — and "3 issues,
            // 0 active, 0 upcoming" is exactly the nonsense this fixes.
            ...(endedCount > 0 ? [{ label: `Ended (${endedCount})`, value: 'ended' }] : []),
          ]}
        />
```

`formatValiditySummary`/`formatFullValidity` take `now` and use `governingPeriod` instead of `validityPeriods[0]`; their date formatting is corrected separately in Task 6, so for now keep the existing `toLocaleDateString`/`toLocaleString` calls and change only which period they read.

Extend `emptyStateMessage`'s `tab` union to include `'ended'` and give it a lead sentence (`'Nothing on this line has finished recently.'`) plus `endedCount` in the sibling-selection logic.

- [ ] **Step 8: Update the two call sites**

In `frontend/app/lines/[id]/page.tsx`, before the `return`:

```tsx
  // Stamped server-side so IssueList's buckets don't depend on a
  // `Date.now()` that differs between the SSR pass and hydration. Fresh on
  // every request (this route is dynamic) and re-stamped by AutoRefresh.
  const now = Date.now();
```

and `<IssueList statuses={report.lineStatuses} now={now} />`.

Do the same in `frontend/app/stations/[crs]/page.tsx`, passing the single `now` to every per-line `IssueList`.

- [ ] **Step 9: Run the full suite**

Run: `cd frontend && npm test`
Expected: PASS — everything, including the new `lib/validity.test.ts` and the updated `IssueList.test.tsx`.

- [ ] **Step 10: Commit**

```bash
git add frontend/lib/validity.ts frontend/lib/validity.test.ts frontend/components/IssueList.tsx frontend/components/IssueList.test.tsx frontend/app/lines/\[id\]/page.tsx frontend/app/stations/\[crs\]/page.tsx
git commit -m "Bucket in-progress dated issues as Active, and make the tab counts add up"
```

---

### Task 5: Make an unknown CRS say "Station not found" instead of issuing an all-clear

**Files:**
- Modify: `frontend/app/stations/[crs]/page.tsx`
- Create: `frontend/app/stations/[crs]/not-found.tsx`

**Interfaces:** None shared. Uses the existing `getStationName` (which already returns `null` for "the search endpoint came back with no exact match").

Punch-list item 5. Confirmed against source: `/stations/ZZZ` renders `Disruptions at ZZZ` + `No disruptions affecting this station.` + a working pin button, because `resolveStationName` swallows *both* "no such station" (`getStationName` → `null`) and "the lookup failed" (a thrown error) into the same `null`, and `getStopPointDisruption` returns `[]` for an unknown code rather than 404ing. Telling a user there are no disruptions at a station that does not exist is worse than an error, and pinning a non-existent CRS writes junk into their preferences.

The two `null` cases must be separated: a genuine miss should 404, but an API hiccup must keep falling back to the bare code (the current, deliberate behaviour — the disruption data is what matters most on this page). A malformed code is rejected before any request is made.

- [ ] **Step 1: Implement**

Replace the `resolveStationName` helper in `frontend/app/stations/[crs]/page.tsx` with:

```tsx
/** Three outcomes, not two. The previous version collapsed "there is no
 * such station" and "the name lookup failed" into a single `null`, so the
 * page could not tell them apart — and rendered a cheerful "No disruptions
 * affecting this station." for `/stations/ZZZ`, complete with a working pin
 * button. An unknown code must 404; a lookup that merely failed must still
 * keep falling back to the bare CRS, since the disruption data is what
 * this page is actually for. */
type StationLookup =
  | { outcome: 'found'; name: string }
  | { outcome: 'unknown' }
  | { outcome: 'unavailable' };

/** Every CRS code is exactly three letters, so a malformed one is answered
 * without troubling the API at all. */
const CRS_PATTERN = /^[A-Za-z]{3}$/;

async function lookupStation(crs: string): Promise<StationLookup> {
  if (!CRS_PATTERN.test(crs)) return { outcome: 'unknown' };
  try {
    const name = await getStationName(crs);
    return name === null ? { outcome: 'unknown' } : { outcome: 'found', name };
  } catch {
    return { outcome: 'unavailable' };
  }
}
```

And the page body:

```tsx
export default async function StationDisruptionPage({
  params,
}: {
  params: Promise<{ crs: string }>;
}) {
  const { crs } = await params;

  // Deliberately awaited before the disruption fetch rather than in
  // parallel with it: `getStationName` is an hour-cached reference lookup,
  // so the serialization costs nothing in the common case, and an unknown
  // code should 404 without ever asking for its (empty) disruption list.
  const lookup = await lookupStation(crs);
  if (lookup.outcome === 'unknown') {
    notFound();
  }

  const [reports, preferences] = await Promise.all([getStopPointDisruption(crs), getPreferences()]);
  const heading = lookup.outcome === 'found' ? `${lookup.name} (${crs})` : crs;
  const now = Date.now();
  // …unchanged render…
```

Add `import { notFound } from 'next/navigation';` at the top.

- [ ] **Step 2: Add the not-found boundary**

Create `frontend/app/stations/[crs]/not-found.tsx`, mirroring `app/lines/[id]/not-found.tsx` but with the way out that Task 13 also adds to its sibling:

```tsx
import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function StationNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Station not found</Title>
      <Text c="dimmed">
        No National Rail station matches that code. Station codes are three letters, like WOK or EUS.
      </Text>
      <TextLink href="/stations" underline="always">
        Look up a station
      </TextLink>
    </Stack>
  );
}
```

- [ ] **Step 3: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

Check all four cases:
- `http://localhost:3000/stations/ZZZ` → "Station not found", **no pin button**.
- `http://localhost:3000/stations/zzzz` → "Station not found", with no API request for it (visible in the frontend container log).
- `http://localhost:3000/stations/WOK` → unchanged, disruptions and pin toggle present.
- A real station with no current disruptions → still the "No disruptions affecting this station." message, *not* a 404.

- [ ] **Step 4: Commit**

```bash
git add frontend/app/stations/\[crs\]/page.tsx frontend/app/stations/\[crs\]/not-found.tsx
git commit -m "404 unknown station codes instead of reporting no disruptions"
```

---

## Phase 1 — Correctness and trust

### Task 6: One locale decision for the whole app (en-GB, Europe/London)

**Files:**
- Create: `frontend/lib/dateFormat.ts`
- Create: `frontend/lib/dateFormat.test.ts`
- Modify: `frontend/components/LastUpdated.tsx`
- Modify: `frontend/components/IssueList.tsx`
- Modify: `frontend/components/IssueList.test.tsx`
- Modify: `frontend/app/lines/[id]/history/page.tsx`

**Interfaces:**
- Produces: `formatDate`, `formatDateTime`, `formatTime`, `londonDayKey` in `lib/dateFormat.ts`. Consumed here and by Task 8.

Punch-list item 3, and it is more than cosmetic on two counts. First, `5/10/2026` in a UK rail context will be read as 5 October; the app renders that today on the line detail page, the station page and the history page. Second, `new Date(x).toLocaleDateString()` with no arguments takes the *runtime's* locale and timezone — verified in this container, Node resolves to `en-US`/`UTC` and produces `5/10/2026`, while a UK browser would produce `10/05/2026` from the same input. That is a live server/client hydration mismatch, of exactly the class `LastUpdated` and `ThemeToggle` already carry comments about, hiding inside what looks like a formatting nit.

`LastUpdated` already got this right (`Intl.DateTimeFormat('en-GB', { timeZone: 'Europe/London', … })`, per `2026-07-15-last-updated-indicators-design.md`). This task promotes that one-off into the app's single formatting module and routes the stragglers through it, so a future call site cannot quietly reintroduce the problem. `formatTime` and `londonDayKey` exist for Task 8.

- [ ] **Step 1: Write the failing test**

Create `frontend/lib/dateFormat.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { formatDate, formatDateTime, formatTime, londonDayKey } from './dateFormat';

describe('formatDate', () => {
  it('renders an unambiguous UK date, never M/D/YYYY', () => {
    expect(formatDate('2026-05-10T00:00:00Z')).toBe('10 May 2026');
    expect(formatDate('2026-10-11T00:00:00Z')).toBe('11 Oct 2026');
  });

  it('is independent of the runtime locale and timezone', () => {
    // Node in CI/containers resolves to en-US/UTC; a UK browser does not.
    // A bare `toLocaleDateString()` therefore produced different markup on
    // the server and the client — a hydration mismatch as well as a
    // correctness bug.
    expect(Intl.DateTimeFormat().resolvedOptions().locale).not.toBe('en-GB');
    expect(formatDate('2026-05-10T00:00:00Z')).toBe('10 May 2026');
  });
});

describe('formatDateTime', () => {
  it('renders a 24-hour UK date-time with no seconds', () => {
    expect(formatDateTime('2026-08-19T18:56:01Z')).toBe('19 Aug 2026, 19:56');
  });
});

describe('formatTime', () => {
  it('renders a 24-hour London wall-clock time', () => {
    expect(formatTime('2026-08-19T18:56:01Z')).toBe('19:56');
  });
});

describe('londonDayKey', () => {
  it('keys by the London calendar day, not the UTC one', () => {
    // 23:30 UTC on 19 Aug is 00:30 on 20 Aug in British Summer Time.
    expect(londonDayKey('2026-08-19T23:30:00Z')).toBe('2026-08-20');
    expect(londonDayKey('2026-08-19T12:00:00Z')).toBe('2026-08-19');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/dateFormat.test.ts`
Expected: FAIL to resolve — `./dateFormat` does not exist.

- [ ] **Step 3: Implement**

Create `frontend/lib/dateFormat.ts`:

```typescript
/** The app's single locale/timezone decision.
 *
 * This is a UK rail product, so dates are en-GB and times are London
 * wall-clock. Both are stated explicitly on every formatter rather than
 * left to the runtime: `new Date(x).toLocaleDateString()` follows whatever
 * locale and timezone the *process* has, which is en-GB/Europe-London in a
 * British browser but en-US/UTC in the Node process rendering the page —
 * so the line detail, station and history pages were simultaneously
 * showing Americans' dates to UK users ("5/10/2026" for 10 May) and
 * emitting different server and client markup for the same timestamp. The
 * same reasoning `LastUpdated` documents for its own formatter; this module
 * is where that formatter now lives, so there is one place to get it wrong.
 *
 * Formatters are module-level constants because constructing an
 * `Intl.DateTimeFormat` is comparatively expensive and these are called
 * once per rendered row. */
const DATE = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  dateStyle: 'medium',
});

/** No `timeStyle: 'medium'`: seconds on a status timestamp are noise — the
 * aggregator recomputes every few minutes. */
const DATE_TIME = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  dateStyle: 'medium',
  timeStyle: 'short',
});

const TIME = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  timeStyle: 'short',
});

/** `en-CA` is the shortest route to a stable YYYY-MM-DD key; the point here
 * is the `Europe/London` timezone, not the locale — grouping history by the
 * UTC day would split a British summer evening across two headings. */
const DAY_KEY = new Intl.DateTimeFormat('en-CA', {
  timeZone: 'Europe/London',
  year: 'numeric',
  month: '2-digit',
  day: '2-digit',
});

function asDate(value: string | Date): Date {
  return value instanceof Date ? value : new Date(value);
}

/** "10 May 2026" */
export function formatDate(value: string | Date): string {
  return DATE.format(asDate(value));
}

/** "19 Aug 2026, 19:56" */
export function formatDateTime(value: string | Date): string {
  return DATE_TIME.format(asDate(value));
}

/** "19:56" */
export function formatTime(value: string | Date): string {
  return TIME.format(asDate(value));
}

/** "2026-08-20" — the London calendar day, for grouping. */
export function londonDayKey(value: string | Date): string {
  return DAY_KEY.format(asDate(value));
}
```

- [ ] **Step 4: Route the existing call sites through it**

In `frontend/components/LastUpdated.tsx`, delete the local `EXACT_TIME_FORMAT` constant and use the shared helper:

```tsx
import { formatDateTime } from '@/lib/dateFormat';
```
```tsx
  const exact = formatDateTime(date);
```

In `frontend/components/IssueList.tsx`, rewrite the two validity formatters (which Task 4 already switched to `governingPeriod`):

```tsx
import { formatDate, formatDateTime } from '@/lib/dateFormat';
```

```tsx
function formatValiditySummary(status: LineStatus, now: number): string {
  const period = governingPeriod(status, now);
  if (!period) return '';
  if (periodIsActive(period, now)) return 'Now';
  const from = formatDate(period.fromDate);
  return period.toDate ? `${from} – ${formatDate(period.toDate)}` : `From ${from}`;
}

function formatFullValidity(status: LineStatus, now: number): string {
  const period = governingPeriod(status, now);
  if (!period) return '';
  const from = formatDateTime(period.fromDate);
  return period.toDate ? `${from} – ${formatDateTime(period.toDate)}` : `${from} – ongoing`;
}
```

Note the `'Now'` summary now keys off `periodIsActive` rather than the raw `isNow` flag, so an in-progress dated window says "Now" on the row as well as counting under Active — the two must not disagree.

In `frontend/app/lines/[id]/history/page.tsx`, replace `new Date(entry.computedAt).toLocaleString()` with `formatDateTime(entry.computedAt)`. (Task 8 rewrites this file; this keeps the intermediate commit correct.)

- [ ] **Step 5: Update `IssueList` tests**

Existing assertions that match on locale-formatted dates need updating to the en-GB strings. Add a guard:

```typescript
  it('formats validity dates as unambiguous UK dates', () => {
    const dated: LineStatus = {
      statusSeverity: 4,
      statusSeverityDescription: 'Planned Closure',
      reason: 'Station improvement work',
      dataQuality: 'planned',
      validityPeriods: [
        { fromDate: '2026-05-10T00:00:00Z', toDate: '2026-10-11T00:00:00Z', isNow: false },
      ],
    };
    renderWithMantine(<IssueList statuses={[dated]} now={Date.parse('2026-12-01T00:00:00Z')} />);
    expect(screen.getByText('10 May 2026 – 11 Oct 2026')).toBeInTheDocument();
  });
```

- [ ] **Step 6: Run the full suite**

Run: `cd frontend && npm test`
Expected: PASS. `LastUpdated.test.tsx`'s existing SSR-determinism regression test must still pass unchanged — it asserts the pre-mount output is a fixed absolute time, and the string it produces is identical because the format options are the same ones moved out of that file.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/dateFormat.ts frontend/lib/dateFormat.test.ts frontend/components/LastUpdated.tsx frontend/components/IssueList.tsx frontend/components/IssueList.test.tsx frontend/app/lines/\[id\]/history/page.tsx
git commit -m "Format every date as en-GB London time via one shared module"
```

---

## Phase 2 — Layout structure

### Task 7: Add a site-wide content max-width

**Files:**
- Modify: `frontend/app/layout.tsx`

**Interfaces:** None. Purely additive chrome; every page keeps its own `p="lg"`.

Punch-list item 8 and viewport issue 5. At 1920px a row's name sits at x≈30 with its status badge at x≈870 and its pin at x≈1780; the operator filter input stretches to ~1880px and the three-word All/Active/Upcoming control becomes a 1880px bar. Mantine's `Container` at `size="lg"` is 1140px (verified in `@mantine/core/styles/Container.css`), inside the review's suggested 1100–1200px band.

The nav goes inside a container of the same width as well, so the site title lines up with the page heading beneath it instead of hugging the viewport edge while the content is centred — a header that stays full-bleed over centred content looks like a bug in its own right. `px={0}` on the container because every page already supplies its own `p="lg"`; letting `Container` add its default `md` inline padding on top would put 40px of gutter on a 390px screen.

- [ ] **Step 1: Implement**

In `frontend/app/layout.tsx`, add `Box, Container` to the `@mantine/core` import, and replace the nav `<Group …>` element and the bare `{children}` with:

```tsx
          {/* No max-width anywhere meant a 1920px viewport put a line's
              name at x≈30, its status badge at x≈870 and its pin at
              x≈1780 — the row stopped being scannable as a row. `lg` is
              1140px. The border stays on a full-bleed Box so the rule still
              spans the window while the nav's contents line up with the
              page content below it. `px={0}`: every page already applies
              its own `p="lg"`, and Container's default `md` inline padding
              on top of that is 40px of gutter on a 390px screen. */}
          <Box
            component="nav"
            style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}
          >
            <Container size="lg" px={0}>
              <Group justify="space-between" px="lg" py="md">
                <Link href="/" style={{ textDecoration: 'none', color: 'inherit' }}>
                  <Text fw={700}>National Rail Line Status</Text>
                </Link>
                <Group gap="lg">
                  <TextLink href="/lines">All Lines</TextLink>
                  <TextLink href="/stations">Station Lookup</TextLink>
                  <Suspense fallback={<ActionIcon variant="subtle" aria-label="Data freshness" disabled loading />}>
                    <DataFreshnessNavItem />
                  </Suspense>
                  <ThemeToggle />
                </Group>
              </Group>
            </Container>
          </Box>
          <Container size="lg" px={0}>
            {children}
          </Container>
```

Keep the existing long comment about `<Link>` wrapping `Text` rather than `component={Link}` — it explains a real Server-Component constraint and still applies.

- [ ] **Step 2: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

At 1920px check `/lines`, `/lines/{id}` and `/`: content is centred at ~1140px, the operator `MultiSelect` and the `SegmentedControl` are no longer full-bleed, and the nav's title/links sit on the same left and right edges as the page content. At 390px confirm horizontal padding is unchanged from before this task (24px, from each page's `p="lg"`) — not doubled.

- [ ] **Step 3: Commit**

```bash
git add frontend/app/layout.tsx
git commit -m "Constrain page and nav content to a 1140px centred container"
```

---

### Task 8: Turn the history page into a readable timeline

**Files:**
- Create: `frontend/lib/history.ts`
- Create: `frontend/lib/history.test.ts`
- Modify: `frontend/app/lines/[id]/history/page.tsx`
- Modify: `frontend/app/lines/[id]/history/HistoryRangePicker.tsx`
- Create: `frontend/app/lines/[id]/history/HistoryRangePicker.test.tsx`

**Interfaces:**
- Produces: `collapseHistory`, `groupSpansByDay`, `resolveRange` and the `HistorySpan`/`HistoryDay`/`ResolvedRange` types in `lib/history.ts`. Consumed by the page.
- Changes: `HistoryRangePicker({ lineId, preset, from, to })`.

Punch-list item 7, covering all four sub-findings: 34,659px desktop / ~46,000px mobile pages, oldest-first ordering, a blank default state, and quick-range buttons whose selected state is imperceptible.

> **Decision — collapse into spans, group by day, newest first; not pagination.** The underlying data is a step function sampled every 5–15 minutes: `HistoryResults` renders one block per aggregator recompute, so a 30-day window is thousands of blocks of near-identical text describing a handful of actual state changes. Pagination would page through the redundancy rather than remove it, and would make "when did this line last go bad" *harder* to answer, not easier. Collapsing runs of consecutive recomputes that carry an identical set of statuses into a single span ("Minor Delays, 20:14–21:45") attacks the redundancy at source and turns the page into the timeline it should always have been; day headings give a scannable skeleton, and newest-first puts the answer to the page's main question at the top instead of 34,000px below the fold. Pagination is deliberately left out — if collapsing ever fails to bound the page for a badly flapping line, day sections can become collapsible without changing the data model, which pagination would.

The presets also move into the URL as `?range=7d`/`?range=30d` rather than absolute instants, so the default state needs no redirect, a shared link stays "the last 7 days" rather than freezing a moment, and the picker can highlight the active preset from the URL alone.

- [ ] **Step 1: Write the failing test**

Create `frontend/lib/history.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { collapseHistory, groupSpansByDay, resolveRange } from './history';
import type { LineStatusHistoryEntry } from './types';

function entry(computedAt: string, statuses: Array<[number, string]>): LineStatusHistoryEntry {
  return {
    $type: 'NRStatus.LineStatusReport',
    id: 'northern',
    name: 'Northern',
    modeName: 'national-rail',
    operators: ['NT'],
    computedAt,
    lineStatuses: statuses.map(([statusSeverity, reason]) => ({
      statusSeverity,
      statusSeverityDescription: 'x',
      reason,
      dataQuality: 'ldbws-inferred' as const,
      validityPeriods: [],
    })),
  };
}

describe('collapseHistory', () => {
  it('collapses consecutive recomputes carrying identical statuses into one span', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:00:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:10:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:20:00Z', [[9, 'Minor delays']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].samples).toBe(3);
    expect(spans[0].from).toBe('2026-08-19T18:00:00Z');
    expect(spans[0].to).toBe('2026-08-19T18:20:00Z');
  });

  it('starts a new span when the status set changes', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:00:00Z', [[10, 'Good Service']]),
      entry('2026-08-19T18:10:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:20:00Z', [[10, 'Good Service']]),
    ]);
    expect(spans).toHaveLength(3);
  });

  it('ignores the order statuses happen to arrive in within one entry', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:00:00Z', [[9, 'A'], [4, 'B']]),
      entry('2026-08-19T18:10:00Z', [[4, 'B'], [9, 'A']]),
    ]);
    expect(spans).toHaveLength(1);
  });

  it('sorts oldest-first before collapsing, so a span is always a real contiguous run', () => {
    const spans = collapseHistory([
      entry('2026-08-19T18:20:00Z', [[9, 'Minor delays']]),
      entry('2026-08-19T18:00:00Z', [[9, 'Minor delays']]),
    ]);
    expect(spans).toHaveLength(1);
    expect(spans[0].from).toBe('2026-08-19T18:00:00Z');
  });

  it('reports the worst severity in the span, by true rank', () => {
    // 4 (Planned Closure) is numerically lower but less severe than 6.
    const spans = collapseHistory([entry('2026-08-19T18:00:00Z', [[4, 'A'], [6, 'B']])]);
    expect(spans[0].severity).toBe(6);
  });

  it('returns nothing for no entries', () => {
    expect(collapseHistory([])).toEqual([]);
  });
});

describe('groupSpansByDay', () => {
  it('groups by London day, newest day first and newest span first within a day', () => {
    const spans = collapseHistory([
      entry('2026-08-19T10:00:00Z', [[10, 'Good Service']]),
      entry('2026-08-19T11:00:00Z', [[9, 'Minor delays']]),
      entry('2026-08-20T10:00:00Z', [[10, 'Good Service']]),
    ]);
    const days = groupSpansByDay(spans);
    expect(days.map((d) => d.day)).toEqual(['2026-08-20', '2026-08-19']);
    expect(days[1].spans[0].from).toBe('2026-08-19T11:00:00Z');
  });
});

describe('resolveRange', () => {
  const NOW = Date.parse('2026-08-21T12:00:00Z');

  it('defaults to the last 7 days when nothing is in the URL', () => {
    const range = resolveRange({}, NOW);
    expect(range.preset).toBe('7d');
    expect(Date.parse(range.to) - Date.parse(range.from)).toBe(7 * 86400000);
  });

  it('honours the 30-day preset', () => {
    expect(resolveRange({ range: '30d' }, NOW).preset).toBe('30d');
  });

  it('honours an explicit custom range and reports no preset', () => {
    const range = resolveRange(
      { from: '2026-08-01T00:00:00Z', to: '2026-08-05T00:00:00Z' },
      NOW,
    );
    expect(range.preset).toBeNull();
    expect(range.from).toBe('2026-08-01T00:00:00Z');
  });

  it('falls back to the default rather than erroring on junk', () => {
    expect(resolveRange({ from: 'nonsense', to: 'also nonsense' }, NOW).preset).toBe('7d');
    expect(resolveRange({ range: 'forever' }, NOW).preset).toBe('7d');
  });

  it('ignores a half-specified custom range', () => {
    expect(resolveRange({ from: '2026-08-01T00:00:00Z' }, NOW).preset).toBe('7d');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/history.test.ts`
Expected: FAIL to resolve — `./history` does not exist.

- [ ] **Step 3: Implement `lib/history.ts`**

Create `frontend/lib/history.ts`:

```typescript
import { londonDayKey } from './dateFormat';
import { severityRank } from './severity';
import type { LineStatus, LineStatusHistoryEntry } from './types';

const DAY_MS = 86_400_000;

export interface HistorySpan {
  /** Worst severity across the span's statuses, by true rank. */
  severity: number;
  statuses: LineStatus[];
  /** `computedAt` of the first recompute in the run. */
  from: string;
  /** `computedAt` of the last recompute in the run. */
  to: string;
  /** How many recomputes were collapsed into this span. */
  samples: number;
}

export interface HistoryDay {
  /** `YYYY-MM-DD`, London. */
  day: string;
  spans: HistorySpan[];
}

/** Identity of an entry's *state*, order-insensitive: two recomputes that
 * found the same set of statuses are the same state even if the aggregator
 * happened to emit them in a different order. Severity plus reason is
 * enough — `statusSeverityDescription` is a pure function of severity, and
 * validity periods on a historical snapshot move as `now` moves, which
 * would defeat the collapsing for no benefit. */
function stateSignature(entry: LineStatusHistoryEntry): string {
  return entry.lineStatuses
    .map((status) => `${status.statusSeverity} ${status.reason}`)
    .sort()
    .join('');
}

function worstSeverity(statuses: LineStatus[]): number {
  return statuses.reduce(
    (worst, status) => (severityRank(status.statusSeverity) > severityRank(worst) ? status.statusSeverity : worst),
    10,
  );
}

/** The aggregator recomputes every 5–15 minutes, so a 30-day history is
 * thousands of entries describing a handful of actual state changes — the
 * page came out 34,659px tall at desktop and ~46,000px at mobile, almost
 * all of it the same sentence repeated. Runs of consecutive recomputes with
 * an identical status set collapse into one span with its own start, end
 * and sample count. Entries are sorted oldest-first first, so a "span" is
 * always a genuinely contiguous run regardless of what order the API
 * returned them in. */
export function collapseHistory(entries: LineStatusHistoryEntry[]): HistorySpan[] {
  const ordered = [...entries].sort((a, b) => Date.parse(a.computedAt) - Date.parse(b.computedAt));

  const spans: HistorySpan[] = [];
  let signature: string | null = null;

  for (const entry of ordered) {
    const next = stateSignature(entry);
    const current = spans[spans.length - 1];
    if (current && next === signature) {
      current.to = entry.computedAt;
      current.samples += 1;
      continue;
    }
    signature = next;
    spans.push({
      severity: worstSeverity(entry.lineStatuses),
      statuses: entry.lineStatuses,
      from: entry.computedAt,
      to: entry.computedAt,
      samples: 1,
    });
  }

  return spans;
}

/** Newest day first, newest span first within a day — the page's main
 * question is "what happened recently", and the previous oldest-first
 * ordering put the most recent state 34,000px below the fold. Keyed on the
 * London calendar day so a summer evening doesn't split across two
 * headings at 23:00 UTC. */
export function groupSpansByDay(spans: HistorySpan[]): HistoryDay[] {
  const byDay = new Map<string, HistorySpan[]>();
  for (const span of spans) {
    const day = londonDayKey(span.from);
    const existing = byDay.get(day);
    if (existing) existing.push(span);
    else byDay.set(day, [span]);
  }

  return Array.from(byDay.entries())
    .sort(([a], [b]) => (a < b ? 1 : a > b ? -1 : 0))
    .map(([day, daySpans]) => ({
      day,
      spans: [...daySpans].sort((a, b) => Date.parse(b.from) - Date.parse(a.from)),
    }));
}

export type RangePreset = '7d' | '30d';

export interface ResolvedRange {
  /** ISO instant, inclusive. */
  from: string;
  /** ISO instant, inclusive. */
  to: string;
  /** `null` for an explicit custom range. */
  preset: RangePreset | null;
}

const PRESET_DAYS: Record<RangePreset, number> = { '7d': 7, '30d': 30 };

/** The page used to render nothing at all until the user picked two dates
 * — a blank screen with a disabled button. Presets now live in the URL as
 * `?range=7d`, so the no-parameters case is simply the 7-day preset and
 * needs no redirect, a shared link keeps meaning "the last 7 days" rather
 * than freezing an instant, and the picker can highlight the active preset
 * from the URL alone. Anything unparseable falls back to the default rather
 * than erroring: a mistyped query string should still show a useful page. */
export function resolveRange(
  params: { from?: string; to?: string; range?: string },
  now: number,
): ResolvedRange {
  const from = params.from ? Date.parse(params.from) : NaN;
  const to = params.to ? Date.parse(params.to) : NaN;
  if (!Number.isNaN(from) && !Number.isNaN(to) && from <= to) {
    return { from: params.from!, to: params.to!, preset: null };
  }

  const preset: RangePreset = params.range === '30d' ? '30d' : '7d';
  return {
    from: new Date(now - PRESET_DAYS[preset] * DAY_MS).toISOString(),
    to: new Date(now).toISOString(),
    preset,
  };
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run lib/history.test.ts`
Expected: PASS — all tests.

- [ ] **Step 5: Rewrite the page**

Replace `frontend/app/lines/[id]/history/page.tsx` entirely with:

```tsx
import { Suspense } from 'react';
import { Divider, Skeleton, Stack, Text, Title } from '@mantine/core';
import { getLineStatus, getLineStatusHistory } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { TextLink } from '@/components/TextLink';
import { collapseHistory, groupSpansByDay, resolveRange } from '@/lib/history';
import { formatDate, formatTime } from '@/lib/dateFormat';
import { HistoryRangePicker } from './HistoryRangePicker';

// Same `revalidate = 0` rationale as the other dynamic routes: without it
// Next.js may treat this route as eligible for static generation and try to
// prerender it at build time, which fails since the `api` service only
// exists on the compose network at runtime. Previously this page was
// implicitly dynamic because it rendered nothing without `searchParams`;
// now that it has a default range, say so explicitly.
export const revalidate = 0;

// …resolveLineName unchanged…

export default async function LineHistoryPage({
  params,
  searchParams,
}: {
  params: Promise<{ id: string }>;
  searchParams: Promise<{ from?: string; to?: string; range?: string }>;
}) {
  const { id } = await params;
  const query = await searchParams;

  const name = await resolveLineName(id);
  const range = resolveRange(query, Date.now());

  return (
    <Stack p="lg" gap="md">
      <TextLink href={`/lines/${id}`} underline="always">
        Back to line
      </TextLink>
      <Title order={1}>History: {name}</Title>
      <HistoryRangePicker lineId={id} preset={range.preset} from={range.from} to={range.to} />
      {/* The results are always rendered now, so without a Suspense
          boundary the whole page — picker included — would block on the
          history fetch, which is the slowest call in the app for a 30-day
          window. */}
      <Suspense key={`${range.from}-${range.to}`} fallback={<Skeleton height={240} />}>
        <HistoryResults id={id} from={range.from} to={range.to} />
      </Suspense>
    </Stack>
  );
}

async function HistoryResults({ id, from, to }: { id: string; from: string; to: string }) {
  const entries = await getLineStatusHistory(id, from, to);
  const spans = collapseHistory(entries);
  const days = groupSpansByDay(spans);

  if (days.length === 0) {
    return <Text c="dimmed">No history entries in that range.</Text>;
  }

  return (
    <Stack gap="lg">
      {/* Says out loud what the collapsing did, so a short page doesn't
          read as missing data. */}
      <Text size="sm" c="dimmed">
        {entries.length} status {entries.length === 1 ? 'recompute' : 'recomputes'} across {spans.length}{' '}
        {spans.length === 1 ? 'period' : 'periods'}, newest first.
      </Text>
      {days.map((day) => (
        <Stack key={day.day} gap="xs">
          <Title order={3} size="h5">
            {formatDate(day.spans[0].from)}
          </Title>
          <Divider />
          {day.spans.map((span) => (
            <div className="issueRow" key={span.from}>
              <div className="issueRow__main">
                <div className="issueRow__badge">
                  <StatusBadge severity={span.severity} />
                </div>
                <Text size="sm" className="issueRow__reason">
                  {span.statuses.map((status) => status.reason).filter(Boolean).join(' · ') || 'No reason given'}
                </Text>
              </div>
              <div className="issueRow__meta">
                <Text size="xs" c="dimmed">
                  {span.from === span.to
                    ? formatTime(span.from)
                    : `${formatTime(span.from)}–${formatTime(span.to)}`}
                </Text>
              </div>
            </div>
          ))}
        </Stack>
      ))}
    </Stack>
  );
}
```

Note the reuse of Task 1's `.issueRow` classes — the timeline row has the same shape as the collapsed issue row and inherits its two-line mobile behaviour for free.

- [ ] **Step 6: Write the failing picker test**

Create `frontend/app/lines/[id]/history/HistoryRangePicker.test.tsx`:

```typescript
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { HistoryRangePicker } from './HistoryRangePicker';

vi.mock('next/navigation', () => ({ useRouter: () => ({ push: vi.fn() }) }));

describe('HistoryRangePicker', () => {
  it('shows the active preset as filled and the inactive one as light', () => {
    renderWithMantine(
      <HistoryRangePicker
        lineId="northern"
        preset="30d"
        from="2026-07-22T12:00:00Z"
        to="2026-08-21T12:00:00Z"
      />,
    );
    expect(screen.getByRole('button', { name: 'Last 30 days' })).toHaveAttribute('data-variant', 'filled');
    expect(screen.getByRole('button', { name: 'Last 7 days' })).toHaveAttribute('data-variant', 'light');
  });

  it('marks the active preset for assistive technology too', () => {
    renderWithMantine(
      <HistoryRangePicker lineId="northern" preset="7d" from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.getByRole('button', { name: 'Last 7 days' })).toHaveAttribute('aria-pressed', 'true');
  });

  it('does not nag about picking dates once a range is already showing', () => {
    renderWithMantine(
      <HistoryRangePicker lineId="northern" preset="7d" from="2026-08-14T12:00:00Z" to="2026-08-21T12:00:00Z" />,
    );
    expect(screen.queryByText(/Pick both a start and end date/)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 7: Run test to verify it fails**

Run: `cd frontend && npx vitest run app/lines/\[id\]/history/HistoryRangePicker.test.tsx`
Expected: FAIL — the component does not take `preset`/`from`/`to` and both buttons are `variant="light"`.

- [ ] **Step 8: Implement the picker**

Replace `frontend/app/lines/[id]/history/HistoryRangePicker.tsx` entirely with:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { DatePickerInput } from '@mantine/dates';
import { Button, Group, Stack, Text } from '@mantine/core';
import type { RangePreset } from '@/lib/history';

function toCalendarDay(iso: string): string {
  return iso.slice(0, 10);
}

/** `preset`/`from`/`to` come from the page, which resolved them out of the
 * URL (see `lib/history.ts`'s `resolveRange`). The picker is now a display
 * of the range the results below actually cover, not an independent piece
 * of state — the two used to be able to disagree, and the quick-range
 * buttons looked identical whether or not their range was the one showing. */
export function HistoryRangePicker({
  lineId,
  preset,
  from,
  to,
}: {
  lineId: string;
  preset: RangePreset | null;
  from: string;
  to: string;
}) {
  const router = useRouter();
  const [value, setValue] = useState<[string | null, string | null]>([
    toCalendarDay(from),
    toCalendarDay(to),
  ]);

  function handleSearch() {
    const [start, end] = value;
    if (!start || !end) return;
    router.push(
      `/lines/${lineId}/history?from=${new Date(start).toISOString()}&to=${new Date(end).toISOString()}`,
    );
  }

  // Presets navigate by name, not by baked-in instants, so a shared link
  // keeps meaning "the last 7 days".
  function handlePreset(next: RangePreset) {
    router.push(`/lines/${lineId}/history?range=${next}`);
  }

  const bothEndsPicked = Boolean(value[0] && value[1]);

  function presetProps(candidate: RangePreset) {
    const selected = preset === candidate;
    return {
      // Filled vs light rather than two shades of the same tint: the
      // difference between "selected" and "not" was barely perceptible.
      variant: selected ? ('filled' as const) : ('light' as const),
      'aria-pressed': selected,
      onClick: () => handlePreset(candidate),
    };
  }

  return (
    <Stack gap="xs">
      <Group gap="sm">
        <Button {...presetProps('7d')}>Last 7 days</Button>
        <Button {...presetProps('30d')}>Last 30 days</Button>
      </Group>
      <Group align="end">
        <DatePickerInput
          type="range"
          label="Pick a date range"
          placeholder="Pick dates range"
          value={value}
          onChange={setValue}
          // The calendar gave no anchor for "where am I" — today rendered
          // exactly like every other day.
          highlightToday
        />
        <Button onClick={handleSearch} disabled={!bothEndsPicked}>
          Show history
        </Button>
      </Group>
      {/* Only while the user has genuinely half-picked a range. It used to
          sit under an empty page as the only thing on it. */}
      {!bothEndsPicked && (
        <Text size="sm" c="dimmed">
          Pick both a start and end date to continue.
        </Text>
      )}
    </Stack>
  );
}
```

- [ ] **Step 9: Run tests to verify they pass**

Run: `cd frontend && npx vitest run app/lines/\[id\]/history lib/history.test.ts`
Expected: PASS — all tests.

- [ ] **Step 10: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

Visit `http://localhost:3000/lines/{id}/history` with no query string: results for the last 7 days appear without any interaction, "Last 7 days" is filled, today is marked in the calendar. Click "Last 30 days": URL becomes `?range=30d`, that button becomes filled, and the page is a small number of day sections rather than tens of thousands of pixels — measure it (`document.body.scrollHeight` in devtools) and confirm it is now in the low thousands. Confirm the newest entry is at the top. Check the same at 390px.

- [ ] **Step 11: Commit**

```bash
git add frontend/lib/history.ts frontend/lib/history.test.ts frontend/app/lines/\[id\]/history/
git commit -m "Collapse line history into a newest-first day-grouped timeline"
```

---

## Phase 3 — Content and polish

### Task 9: Extract the shared sample-stats helper

**Files:**
- Create: `frontend/lib/sampleStats.ts`
- Create: `frontend/lib/sampleStats.test.ts`
- Modify: `frontend/app/lines/AllLinesTable.tsx`
- Modify: `frontend/components/LineStatusCard.tsx`
- Modify: `frontend/app/page.tsx`

**Interfaces:**
- Produces: `firstSampleStats(statuses)`, `cancelledPercent(stats)`, `formatSampleSummary(stats)` in `lib/sampleStats.ts`. Consumed by Tasks 10 and 11.

Groundwork for the rest of this phase. The same two lines — "find the first status carrying `sampleStats`" and "round `cancelled / total` to a percentage" — are currently written out four times (`AllLinesTable.sampleStatsFor`, `LineStatusCard`, `app/page.tsx`'s `sampleStatsAcrossReports`, `RepresentativeInfo`), and the station page is about to need a fifth. Consolidating first keeps Tasks 10 and 11 small.

- [ ] **Step 1: Write the failing test**

Create `frontend/lib/sampleStats.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { cancelledPercent, firstSampleStats, formatSampleSummary } from './sampleStats';
import type { LineStatus, SampleStats } from './types';

const stats: SampleStats = { total: 160, delayed: 142, cancelled: 8, skipped: 1, avgDelayMinutes: 12.44 };

function status(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    ...overrides,
  };
}

describe('firstSampleStats', () => {
  it('returns undefined when nothing carries stats', () => {
    expect(firstSampleStats([status()])).toBeUndefined();
  });

  it('returns the first status that carries stats', () => {
    expect(firstSampleStats([status(), status({ sampleStats: stats })])).toBe(stats);
  });
});

describe('cancelledPercent', () => {
  it('rounds to a whole percentage', () => {
    expect(cancelledPercent(stats)).toBe(5);
  });

  it('returns null rather than dividing by zero on an empty sample', () => {
    expect(cancelledPercent({ ...stats, total: 0 })).toBeNull();
  });

  it('returns null for missing stats', () => {
    expect(cancelledPercent(undefined)).toBeNull();
  });
});

describe('formatSampleSummary', () => {
  it('renders the one-line summary used across cards, rows and tables', () => {
    expect(formatSampleSummary(stats)).toBe('Avg delay 12.4 min · 5% cancelled');
  });

  it('says so when there is no sample data', () => {
    expect(formatSampleSummary(undefined)).toBe('No sample data');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/sampleStats.test.ts`
Expected: FAIL to resolve.

- [ ] **Step 3: Implement**

Create `frontend/lib/sampleStats.ts`:

```typescript
import type { LineStatus, SampleStats } from './types';

/** The aggregator attaches the same sample-derived stats to every status on
 * a line's report, so the first one found is representative of all of them
 * — the rationale `RepresentativeInfo` already documents, extracted here
 * because four call sites had independently reimplemented it. */
export function firstSampleStats(statuses: LineStatus[]): SampleStats | undefined {
  return statuses.find((status) => status.sampleStats)?.sampleStats;
}

/** `null` rather than 0 for an empty sample: "0% cancelled" out of nothing
 * is a claim the data doesn't support. */
export function cancelledPercent(stats: SampleStats | undefined): number | null {
  if (!stats || stats.total === 0) return null;
  return Math.round((stats.cancelled / stats.total) * 100);
}

export function formatSampleSummary(stats: SampleStats | undefined): string {
  if (!stats) return 'No sample data';
  const cancelled = cancelledPercent(stats);
  const delay = `Avg delay ${stats.avgDelayMinutes.toFixed(1)} min`;
  return cancelled === null ? delay : `${delay} · ${cancelled}% cancelled`;
}
```

- [ ] **Step 4: Adopt it at the existing call sites**

- `frontend/app/lines/AllLinesTable.tsx`: delete the local `sampleStatsFor`, import `firstSampleStats`/`cancelledPercent`/`formatSampleSummary`, and use `formatSampleSummary(stats)` for the `hiddenFrom="sm"` line added in Task 3.
- `frontend/components/LineStatusCard.tsx`: replace the inline `find`/percentage lines with `firstSampleStats(report.lineStatuses)` and render `formatSampleSummary(stats)`.
- `frontend/app/page.tsx`: rewrite `sampleStatsAcrossReports` to `reports.map((r) => firstSampleStats(r.lineStatuses)).find(Boolean)` and use `formatSampleSummary`.

Leave `RepresentativeInfo` alone — its sentence is a different, longer one that names `skipped` and `delayed`; forcing it through `formatSampleSummary` would lose information for no gain.

- [ ] **Step 5: Run the full suite**

Run: `cd frontend && npm test`
Expected: PASS. `LineStatusCard.test.tsx` and `AllLinesTable.test.tsx` may need their expected strings adjusted to the `formatSampleSummary` wording — update them, don't loosen them.

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/sampleStats.ts frontend/lib/sampleStats.test.ts frontend/app/lines/AllLinesTable.tsx frontend/components/LineStatusCard.tsx frontend/app/page.tsx frontend/components/LineStatusCard.test.tsx frontend/app/lines/AllLinesTable.test.tsx
git commit -m "Extract the shared sample-stats helpers into lib/sampleStats.ts"
```

---

### Task 10: De-duplicate the station page

**Files:**
- Create: `frontend/lib/stationIssues.ts`
- Create: `frontend/lib/stationIssues.test.ts`
- Modify: `frontend/components/IssueList.tsx`
- Modify: `frontend/components/IssueList.test.tsx`
- Modify: `frontend/app/stations/[crs]/page.tsx`
- Modify: `frontend/app/lines/[id]/page.tsx`

**Interfaces:**
- Produces: `IssueLineRef`, `IssueItem`, `statusKey`, `dedupeStationIssues` in `lib/stationIssues.ts`.
- Changes: `IssueList({ items, now })` replaces `IssueList({ statuses, now })`.

Punch-list item 9, station half. Confirmed in `station-detail_with-disruptions__desktop-1440x900.png`: Woking renders the *same three* disruptions three times over — once for Portsmouth Direct Line, once for South West Main Line, once for Alton Line — each with its own severity chip row, source chip row, three-segment tab bar and accordion. `/StopPoint/{crs}/Disruption` returns one report per line through the station, and an operator-wide incident lands identically on every one of them.

> **Decision — dedupe with "affects" attribution, keep a per-line status strip.** The review offered three options: one shared filter bar, collapsed per-line accordions, or dedupe with "affects: …" tags. Dedupe is the only one that removes the actual problem — the other two still make the reader compare three identical lists to discover they are identical. `2026-07-09-outage-page-redesign-design.md` deliberately chose per-line grouping to preserve which line each issue belongs to, and that concern is real; it is met more cheaply by carrying the set of affected lines *on each issue* than by cloning the whole list per line. A compact per-line strip at the top (name → linked to its line page, sample summary, status badge) keeps the "how is each line doing" view that grouping also provided, and gives the line headings the links the review asked for. Net effect on Woking: three filter blocks, three tab bars and nine rows become one filter block, one tab bar and three rows.

`RepresentativeInfo` is dropped from this page as a consequence: with the issues flattened, "the first `sampleStats` found across several different lines" is not representative of anything. Each line's own stats move into its strip row instead, which says strictly more.

- [ ] **Step 1: Write the failing test**

Create `frontend/lib/stationIssues.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { dedupeStationIssues } from './stationIssues';
import type { LineStatus, LineStatusReport } from './types';

function status(reason: string, severity = 9): LineStatus {
  return {
    statusSeverity: severity,
    statusSeverityDescription: 'Minor Delays',
    reason,
    dataQuality: 'planned',
    validityPeriods: [{ fromDate: '2026-05-10T00:00:00Z', toDate: '2026-10-11T00:00:00Z', isNow: false }],
  };
}

function report(id: string, name: string, statuses: LineStatus[]): LineStatusReport {
  return {
    $type: 'NRStatus.LineStatusReport',
    id,
    name,
    modeName: 'national-rail',
    operators: ['SW'],
    computedAt: '2026-08-21T12:00:00Z',
    lineStatuses: statuses,
  };
}

describe('dedupeStationIssues', () => {
  it('collapses an operator-wide issue reported on three lines into one item', () => {
    const shared = status('Berrylands Station Upgrade');
    const items = dedupeStationIssues([
      report('portsmouth-direct', 'Portsmouth Direct Line', [shared]),
      report('south-west-main', 'South West Main Line', [{ ...shared }]),
      report('alton', 'Alton Line', [{ ...shared }]),
    ]);
    expect(items).toHaveLength(1);
    expect(items[0].lines.map((l) => l.name)).toEqual([
      'Portsmouth Direct Line',
      'South West Main Line',
      'Alton Line',
    ]);
  });

  it('keeps genuinely different issues apart', () => {
    const items = dedupeStationIssues([
      report('a', 'A', [status('Signal failure')]),
      report('b', 'B', [status('Engineering works')]),
    ]);
    expect(items).toHaveLength(2);
  });

  it('does not merge issues that differ only in severity', () => {
    const items = dedupeStationIssues([
      report('a', 'A', [status('Same words', 9)]),
      report('b', 'B', [status('Same words', 6)]),
    ]);
    expect(items).toHaveLength(2);
  });

  it('preserves first-seen order', () => {
    const items = dedupeStationIssues([
      report('a', 'A', [status('First'), status('Second')]),
      report('b', 'B', [status('Second')]),
    ]);
    expect(items.map((i) => i.status.reason)).toEqual(['First', 'Second']);
  });

  it('does not list the same line twice for an issue reported twice on it', () => {
    const items = dedupeStationIssues([report('a', 'A', [status('Dup'), status('Dup')])]);
    expect(items).toHaveLength(1);
    expect(items[0].lines).toHaveLength(1);
  });

  it('returns nothing for no reports', () => {
    expect(dedupeStationIssues([])).toEqual([]);
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/stationIssues.test.ts`
Expected: FAIL to resolve.

- [ ] **Step 3: Implement `lib/stationIssues.ts`**

```typescript
import type { LineStatus, LineStatusReport } from './types';

export interface IssueLineRef {
  id: string;
  name: string;
}

/** An issue plus the lines it was reported on. `lines` is optional so the
 * line detail page — where every issue belongs to the one line named in the
 * heading — doesn't have to say so on every row. */
export interface IssueItem {
  status: LineStatus;
  lines?: IssueLineRef[];
}

/** Identity of an issue across reports. Reason alone is not enough (the
 * same words at two severities are two different situations) and the
 * validity window has to be in the key too, or a recurring closure would
 * merge with next month's. */
export function statusKey(status: LineStatus): string {
  return [
    status.statusSeverity,
    status.dataQuality,
    status.reason,
    status.validityPeriods.map((p) => `${p.fromDate}/${p.toDate ?? ''}/${p.isNow}`).join(';'),
  ].join(' ');
}

/** `/StopPoint/{crs}/Disruption` returns one report per line through the
 * station, and an operator-wide incident lands identically on all of them —
 * Woking rendered the same three disruptions, each behind its own filter
 * block and tab bar, three times over. Collapse identical statuses into one
 * item carrying every line it was reported on, so attribution survives
 * without the repetition. First-seen order is preserved; `IssueList` sorts
 * by urgency afterwards. */
export function dedupeStationIssues(reports: LineStatusReport[]): IssueItem[] {
  const byKey = new Map<string, IssueItem & { lines: IssueLineRef[] }>();

  for (const report of reports) {
    for (const status of report.lineStatuses) {
      const key = statusKey(status);
      const existing = byKey.get(key);
      if (!existing) {
        byKey.set(key, { status, lines: [{ id: report.id, name: report.name }] });
        continue;
      }
      if (!existing.lines.some((line) => line.id === report.id)) {
        existing.lines.push({ id: report.id, name: report.name });
      }
    }
  }

  return Array.from(byKey.values());
}
```

- [ ] **Step 4: Write the failing `IssueList` tests**

Change every `renderWithMantine(<IssueList statuses={…} now={NOW} />)` in `IssueList.test.tsx` to `items={….map((status) => ({ status }))}`, and add:

```typescript
  it('names the affected lines on a row reported on more than one', () => {
    renderWithMantine(
      <IssueList
        items={[
          {
            status: minorNow,
            lines: [
              { id: 'a', name: 'Portsmouth Direct Line' },
              { id: 'b', name: 'South West Main Line' },
            ],
          },
        ]}
        now={NOW}
      />,
    );
    expect(screen.getByText('2 lines')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Signal failure'));
    expect(screen.getByText(/Portsmouth Direct Line, South West Main Line/)).toBeInTheDocument();
  });

  it('says nothing about lines when an issue only affects one', () => {
    renderWithMantine(
      <IssueList items={[{ status: minorNow, lines: [{ id: 'a', name: 'Alton Line' }] }]} now={NOW} />,
    );
    expect(screen.queryByText(/lines$/)).not.toBeInTheDocument();
  });
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cd frontend && npx vitest run components/IssueList.test.tsx`
Expected: FAIL — `items` is not a prop.

- [ ] **Step 6: Implement in `IssueList`**

Change the signature and derive the statuses list once:

```tsx
export function IssueList({ items, now }: { items: IssueItem[]; now: number }) {
  const statuses = useMemo(() => items.map((item) => item.status), [items]);
  const linesByStatus = useMemo(
    () => new Map(items.map((item) => [item.status, item.lines ?? []])),
    [items],
  );
```

Everything downstream keeps working on `statuses`; only the row rendering changes. In `.issueRow__meta`, before the source badge:

```tsx
                  {(linesByStatus.get(status) ?? []).length > 1 && (
                    <Badge variant="outline" size="sm" color="gray">
                      {linesByStatus.get(status)!.length} lines
                    </Badge>
                  )}
```

and in `AccordionPanel`, above the validity line:

```tsx
                {(linesByStatus.get(status) ?? []).length > 1 && (
                  <Text size="sm" c="dimmed">
                    Affects: {linesByStatus.get(status)!.map((line) => line.name).join(', ')}
                  </Text>
                )}
```

- [ ] **Step 7: Rewrite the station page**

Replace the body of `frontend/app/stations/[crs]/page.tsx`'s render with (keeping Task 5's lookup logic above it):

```tsx
  const items = dedupeStationIssues(reports);
  // Worst first, then alphabetical — the previous order was whatever the
  // API iterated, which visibly differed between viewports on the same data.
  const orderedReports = [...reports].sort((a, b) => {
    const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
    return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
  });

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Disruptions at {heading}</Title>
        <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
      </Group>

      {reports.length === 0 && <Text c="dimmed">No disruptions affecting this station.</Text>}

      {orderedReports.length > 0 && (
        <>
          <Divider />
          {/* Per-line attribution, once — replacing three full copies of
              the same filter block, tab bar and issue list. The headings
              are links now, which the review asked for and the previous
              plain `Text` headings weren't. */}
          <Stack gap="xs">
            {orderedReports.map((report) => {
              const stats = firstSampleStats(report.lineStatuses);
              return (
                <Group key={report.id} justify="space-between" wrap="nowrap" gap="sm">
                  <Stack gap={0} style={{ minWidth: 0 }}>
                    <TextLink href={`/lines/${report.id}`}>{report.name}</TextLink>
                    <Text size="xs" c="dimmed">
                      {formatSampleSummary(stats)}
                    </Text>
                  </Stack>
                  <StatusBadge severity={worstStatus(report).statusSeverity} />
                </Group>
              );
            })}
          </Stack>
          <Divider />
          <IssueList items={items} now={now} />
        </>
      )}
    </Stack>
  );
```

Update imports: drop `RepresentativeInfo`, add `TextLink`, `severityRank`, `dedupeStationIssues`, `firstSampleStats`, `formatSampleSummary`.

- [ ] **Step 8: Update the line detail call site**

In `frontend/app/lines/[id]/page.tsx`:

```tsx
      {/* Every issue here belongs to the line already named in the heading,
          so no per-issue line attribution is needed — that's what the
          optional `lines` on IssueItem is for on the station page. */}
      <IssueList items={report.lineStatuses.map((status) => ({ status }))} now={now} />
```

- [ ] **Step 9: Run the full suite and verify**

Run: `cd frontend && npm test` — expected PASS.

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

Visit `http://localhost:3000/stations/WOK`: one filter block, one tab bar, three rows (not nine), each shared row carrying a "3 lines" badge that expands to name them; the three line names above are links that navigate; the section order is worst-first and identical at 390px, 834px and 1440px.

- [ ] **Step 10: Commit**

```bash
git add frontend/lib/stationIssues.ts frontend/lib/stationIssues.test.ts frontend/components/IssueList.tsx frontend/components/IssueList.test.tsx frontend/app/stations/\[crs\]/page.tsx frontend/app/lines/\[id\]/page.tsx
git commit -m "De-duplicate station issues across lines and link the line headings"
```

---

### Task 11: Clean up the home dashboard

**Files:**
- Modify: `frontend/components/LineStatusCard.tsx`
- Modify: `frontend/components/LineStatusCard.test.tsx`
- Modify: `frontend/app/page.tsx`

**Interfaces:** None shared. Uses `lib/sampleStats.ts` (Task 9) and `getStationName` (existing).

Punch-list item 9, home half, plus viewport issue 7. Three separate defects, all visible in `home_default__desktop-1440x900.png` and `home_default__mobile-390x844.png`:

1. **Reason text walls.** The West Barnes Drain and Milton Keynes Drain cards dump the full machine-flavoured reason — ten lines of "…reported more severe than automatically classified: reduced service; platform 2 closed…" — while sibling Good Service cards sit nearly empty at the same stretched height. `lineClamp={3}` puts the rest on the detail page where it belongs.
2. **Badge placement flips when the name wraps.** `Group justify="space-between"` defaults to `wrap="wrap"`, so a long name ("Elizabeth line Heathrow Branch") pushes the badge onto its own line on some cards and not others.
3. **Bare CRS codes and unstable order.** "Your Stations" lists EUS/WAT/MKC/RAY/VXH while the station page proves the app can resolve names; and the card order differs between captures minutes apart because `allReports.filter(...)` inherits whatever order `/Line/Mode/…/Status` returned.

- [ ] **Step 1: Write the failing test**

Add to `frontend/components/LineStatusCard.test.tsx`:

```typescript
  it('clamps a long reason rather than letting it fill the card', () => {
    const wall = 'Station improvement work: '.repeat(40);
    const { container } = renderWithMantine(
      <LineStatusCard report={{ ...report, lineStatuses: [{ ...report.lineStatuses[0], reason: wall }] }} />,
    );
    const reason = container.querySelector('[data-card-reason]') as HTMLElement;
    expect(reason.style.getPropertyValue('-webkit-line-clamp')).toBe('3');
  });

  it('keeps the status badge on the title row even when the name wraps', () => {
    const { container } = renderWithMantine(
      <LineStatusCard report={{ ...report, name: 'Elizabeth line Heathrow Branch' }} />,
    );
    const titleRow = container.querySelector('[data-card-title-row]') as HTMLElement;
    expect(titleRow.getAttribute('data-wrap')).toBe('nowrap');
  });
```

(adapt the fixture name to whatever `LineStatusCard.test.tsx` already builds).

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/LineStatusCard.test.tsx`
Expected: the 2 new tests FAIL; the pre-existing ones PASS.

- [ ] **Step 3: Implement the card**

In `frontend/components/LineStatusCard.tsx`:

```tsx
        {/* `wrap="nowrap"` with the name allowed to shrink: with Group's
            default wrapping, a long line name pushed the badge onto its own
            line on some cards and not others, so a grid of cards had no
            consistent place to look for status. */}
        <Group justify="space-between" wrap="nowrap" gap="xs" data-card-title-row>
          <Text fw={600} lineClamp={2} style={{ minWidth: 0 }}>
            {report.name}
          </Text>
          {/* StatusBadge opts out of Mantine's ellipsis (see globals.css),
              so it holds its full label here. */}
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
        {/* Three lines, not ten. These reasons run to whole paragraphs of
            machine-assembled text; the card's job is "is this line OK", and
            the detail page carries the rest. */}
        <Text size="sm" c="dimmed" lineClamp={3} data-card-reason>
          {worst.reason}
        </Text>
```

- [ ] **Step 4: Implement the page**

In `frontend/app/page.tsx`:

```tsx
  // The pinned set came out in whatever order `/Line/Mode/…/Status`
  // happened to return, which visibly differed between two captures minutes
  // apart. Worst first, then alphabetical: a dashboard should lead with
  // what needs attention, and must not reshuffle under the user.
  const pinnedLineReports = allReports
    .filter((report) => preferences.pinnedLines.includes(report.id))
    .sort((a, b) => {
      const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });

  const pinnedStationEntries = await Promise.all(
    preferences.pinnedStations.map(async (crs) => ({
      crs,
      // The station detail page already shows "London Kings Cross (KGX)";
      // there is no reason for the dashboard to show a bare code. Hour-cached
      // reference data (see `getStationName`), and a failure here falls back
      // to the code rather than taking the dashboard down.
      name: await getStationName(crs).catch(() => null),
      reports: await getStopPointDisruption(crs),
    })),
  );
```

and in the station card:

```tsx
                        <Text fw={600}>{name ? `${name} (${crs})` : crs}</Text>
```

Add `worstStatus` to the `@/lib/severity` import and `getStationName` to the `@/lib/api` import.

- [ ] **Step 5: Run the full suite and verify**

Run: `cd frontend && npm test` — expected PASS.

Rebuild and check `/` at 1440px and 390px: no card is a text wall, every badge sits on its title row, station rows read "Woking (WOK)", and two reloads produce the same card order.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/LineStatusCard.tsx frontend/components/LineStatusCard.test.tsx frontend/app/page.tsx
git commit -m "Clamp dashboard card text, stabilise card order, and name pinned stations"
```

---

### Task 12: Nav chrome cosmetics — tooltip placement and the auto-theme indicator

**Files:**
- Modify: `frontend/components/LineDefinitionTooltip.tsx`
- Modify: `frontend/components/LineDefinitionTooltip.test.tsx`
- Modify: `frontend/components/DataFreshnessInfo.tsx`
- Modify: `frontend/components/ThemeToggle.tsx`
- Modify: `frontend/components/ThemeToggle.test.tsx`

**Interfaces:** None. Presentational props only.

Punch-list item 10, first two entries. Two notes on what was actually found versus what the review reported:

- **Tooltip.** The overlap is real and reproducible (`line-detail_tooltip-open__tablet-834x1194.png`, `__mobile-390x844.png`): Mantine's `Tooltip` defaults to `position="top"`, and the "How this line is defined" trigger sits immediately below the nav, so the tooltip opens upward straight over it. The review's *diagnosis* — a z-index problem, "the site title renders through the tooltip" — is not supported by the code: the tooltip is portalled to `<body>` at `z-index: 300` and the nav has no stacking context at all, so it cannot lose that fight; the capture almost certainly caught the fade-in transition mid-way. The fix is therefore positional, not a z-index bump: open downward, into the content, where there is room. A modest `zIndex` is still set so the intent is explicit rather than relying on a default.
- **Theme toggle "A" badge.** The review reports it as "clipped by the header edge". Checking `home_default__mobile-390x844.png`, `lines_default__tablet-834x1194.png` and `line-detail_not-found__mobile-390x844.png` at pixel level, the badge renders complete in all three — it is *not* clipped. The real complaint holds though: a 14px filled bubble overhanging the icon's corner reads as a rendering accident. Moving it to `bottom-end` with a smaller size and larger offset tucks it into the button's corner where it reads as deliberate, and removes any possibility of it overhanging the nav's bounding box. Recording the correction here so nobody spends time hunting a clipping bug that isn't there.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/LineDefinitionTooltip.test.tsx`:

```typescript
  it('opens downward, into the page, rather than up over the nav', () => {
    // The trigger sits directly under the header; Mantine's default
    // `position="top"` put the tooltip on top of the site title on tablet
    // and over the whole nav on mobile.
    renderWithMantine(<LineDefinitionTooltip stations={['WOK']} operators={['SW']} />);
    fireEvent.click(screen.getByLabelText('How this line is defined'));
    expect(document.querySelector('[data-position="bottom"]')).not.toBeNull();
  });
```

Add to `frontend/components/ThemeToggle.test.tsx`:

```typescript
  it('tucks the auto-mode badge into the button corner instead of overhanging it', () => {
    const { container } = renderWithMantine(<ThemeToggle />, { defaultColorScheme: 'auto' });
    const indicator = container.querySelector('[data-position="bottom-end"]');
    expect(indicator).not.toBeNull();
  });
```

(If Mantine's rendered attribute names differ from `data-position`, assert against whatever the component actually emits — read the rendered DOM in the failing run before writing the final assertion, rather than guessing.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd frontend && npx vitest run components/LineDefinitionTooltip.test.tsx components/ThemeToggle.test.tsx`
Expected: the 2 new tests FAIL; all pre-existing tests in both files PASS.

- [ ] **Step 3: Implement**

`frontend/components/LineDefinitionTooltip.tsx` — on the `Tooltip`:

```tsx
      // The trigger sits immediately below the nav, so Mantine's default
      // `position="top"` opened the tooltip straight over the header (and
      // over the whole nav at 390px). Downward, into the content, is the
      // only direction with room. `zIndex` is stated rather than left to
      // the default so the intent is explicit — the tooltip is already
      // portalled to <body> and the nav creates no stacking context, so
      // this is belt-and-braces, not the actual fix.
      position="bottom"
      zIndex={400}
```

`frontend/components/DataFreshnessInfo.tsx` — same treatment, but `position="bottom-end"`: this trigger is in the nav's right-hand group, so a centred `bottom` tooltip would hang off the right edge of the viewport.

`frontend/components/ThemeToggle.tsx` — on the `Indicator`:

```tsx
    // `bottom-end` at a smaller size and larger offset, rather than the
    // default top-right overhang: as an overhanging 14px bubble it read as
    // a rendering artifact rather than a status indicator, and it was the
    // only element in the nav able to paint outside its own button.
    <Indicator
      label="A"
      size={12}
      offset={4}
      position="bottom-end"
      disabled={displayedScheme !== 'auto'}
      attributes={{ indicator: { 'aria-hidden': 'true' } }}
    >
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run components/LineDefinitionTooltip.test.tsx components/ThemeToggle.test.tsx components/DataFreshnessInfo.test.tsx`
Expected: PASS.

- [ ] **Step 5: Verify against the running stack**

Rebuild, then at 390px and 834px: tap the ⓘ next to a line name — the tooltip opens *below* it and never touches the nav. Tap the nav's ⓘ — it opens below and stays inside the right edge. In auto mode the "A" sits in the button's lower-right corner, fully inside the header.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/LineDefinitionTooltip.tsx frontend/components/LineDefinitionTooltip.test.tsx frontend/components/DataFreshnessInfo.tsx frontend/components/ThemeToggle.tsx frontend/components/ThemeToggle.test.tsx
git commit -m "Open tooltips downward and tuck the auto-theme badge into its button"
```

---

### Task 13: Make the All Lines sortable headers discoverable and operable

**Files:**
- Modify: `frontend/app/lines/AllLinesTable.tsx`
- Modify: `frontend/app/lines/AllLinesTable.test.tsx`

**Interfaces:** None shared.

Punch-list item 10, third entry. Two problems for the price of one. The review's: `sortIndicator` returns `null` for every column until one is clicked, so there is no affordance saying the headers are sortable at all (`lines_sorted-by-avg-delay__desktop-1440x900.png` vs the default capture). And one the screenshots couldn't show: `headerProps` puts `onClick` and `cursor: pointer` on a bare `<th>`, which is not focusable, not keyboard-operable, and announces nothing about sort state. A real `<button>` inside the header plus `aria-sort` fixes both.

- [ ] **Step 1: Write the failing test**

Add to `frontend/app/lines/AllLinesTable.test.tsx`:

```typescript
describe('AllLinesTable sorting affordance', () => {
  it('shows a sort glyph on every sortable header before anything is clicked', () => {
    renderTable();
    expect(screen.getAllByText('↕').length).toBeGreaterThanOrEqual(3);
  });

  it('makes the headers real buttons, so they are keyboard-operable', () => {
    renderTable();
    const nameHeader = screen.getByRole('button', { name: /Name/ });
    expect(nameHeader).toBeInTheDocument();
  });

  it('announces sort state via aria-sort', () => {
    renderTable();
    const header = screen.getByRole('columnheader', { name: /Name/ });
    expect(header).toHaveAttribute('aria-sort', 'none');
    fireEvent.click(screen.getByRole('button', { name: /Name/ }));
    expect(header).toHaveAttribute('aria-sort', 'ascending');
    fireEvent.click(screen.getByRole('button', { name: /Name/ }));
    expect(header).toHaveAttribute('aria-sort', 'descending');
  });
});
```

(add `fireEvent` to the `@testing-library/react` import).

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run app/lines/AllLinesTable.test.tsx`
Expected: the 3 new tests FAIL; pre-existing ones PASS.

- [ ] **Step 3: Implement**

In `frontend/app/lines/AllLinesTable.tsx`, replace `sortIndicator` and `headerProps` with:

```tsx
/** A neutral glyph on every sortable column, not just the active one:
 * without it there was no affordance at all until after a click, so the
 * headers looked like plain labels. `aria-hidden` because `aria-sort` on
 * the `<th>` carries the same information properly. */
function SortGlyph({ field, sort }: { field: SortField; sort: SortState | null }) {
  const active = sort?.field === field;
  return (
    <Text span size="xs" c="dimmed" aria-hidden>
      {' '}
      {active ? (sort!.direction === 'asc' ? '▲' : '▼') : '↕'}
    </Text>
  );
}

function ariaSort(field: SortField, sort: SortState | null): 'ascending' | 'descending' | 'none' {
  if (sort?.field !== field) return 'none';
  return sort.direction === 'asc' ? 'ascending' : 'descending';
}
```

and render each sortable header as:

```tsx
            {/* `UnstyledButton` inside the `<th>` rather than `onClick` on
                the `<th>` itself: a bare cell with a click handler is not
                focusable and cannot be triggered from the keyboard, which
                made the whole sorting feature mouse-only. */}
            <TableTh aria-sort={ariaSort('name', sort)}>
              <UnstyledButton onClick={() => toggleSort('name')} style={{ fontWeight: 'inherit' }}>
                Name
                <SortGlyph field="name" sort={sort} />
              </UnstyledButton>
            </TableTh>
```

Repeat for `status`, `avgDelay` (keeping `visibleFrom="sm"` from Task 3) and `cancelled`. Add `UnstyledButton` to the `@mantine/core` import; `Text` is already imported.

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run app/lines/AllLinesTable.test.tsx`
Expected: PASS — all tests, including Task 3's responsive-column ones.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/AllLinesTable.tsx frontend/app/lines/AllLinesTable.test.tsx
git commit -m "Make All Lines sortable headers discoverable and keyboard-operable"
```

---

### Task 14: Copy fixes and dead ends

**Files:**
- Modify: `frontend/app/stations/page.tsx`
- Modify: `frontend/app/lines/[id]/not-found.tsx`
- Modify: `frontend/components/IssueList.tsx`
- Modify: `frontend/components/IssueList.test.tsx`

**Interfaces:** None shared.

The last three findings, grouped because each is a few lines.

1. **The lookup page contradicts itself** (`stations-lookup_default-empty__desktop-1440x900.png`). The subtitle says "Enter a 3-letter CRS station code" while the field beneath it is labelled "Station name or CRS code" and resolves typed names through the autocomplete — `StationSearchForm.handleSearch` explicitly matches on code *or* name and carries a comment about the bug that used to make name search work only by accident. The sentence undersells work that has already been done.
2. **"Line not found" is a dead end** (`line-detail_not-found__desktop-1440x900.png`) — no link anywhere. One CTA fixes it, matching the station not-found page added in Task 5.
3. **Good Service says the same thing three times** (`line-detail_standard-good-service__desktop-1440x900.png`, and the tablet capture): a `GOOD SERVICE` header badge, then a filter block and a tab bar, then one accordion row whose entire content is a `GOOD SERVICE` badge and the words "Good Service". The issue list for a healthy line carries nothing the header doesn't. Suppress it — filters and tabs over a single non-issue are pure chrome.

- [ ] **Step 1: Write the failing test**

Add to `frontend/components/IssueList.test.tsx`:

```typescript
  it('replaces the filter chrome with one sentence when the line is simply fine', () => {
    const goodService: LineStatus = {
      statusSeverity: 10,
      statusSeverityDescription: 'Good Service',
      reason: 'Good Service',
      dataQuality: 'ldbws-inferred',
      validityPeriods: [{ fromDate: new Date(NOW).toISOString(), toDate: null, isNow: true }],
    };
    renderWithMantine(<IssueList items={[{ status: goodService }]} now={NOW} />);
    expect(screen.getByText('Good service — no issues reported on this line.')).toBeInTheDocument();
    expect(screen.queryByText(/^All \(/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Severity —/)).not.toBeInTheDocument();
  });

  it('still shows the full list when a Good Service status sits alongside a real issue', () => {
    const goodService: LineStatus = { ...minorNow, statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: 'Good Service' };
    renderWithMantine(
      <IssueList items={[goodService, minorNow].map((status) => ({ status }))} now={NOW} />,
    );
    expect(screen.getByText(/^All \(2\)/)).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/IssueList.test.tsx`
Expected: the 2 new tests FAIL (the chrome renders regardless).

- [ ] **Step 3: Implement**

In `frontend/components/IssueList.tsx`, immediately after the `statuses`/`linesByStatus` memos and *before* any other derived state:

```tsx
  // A line with nothing wrong doesn't need a filter UI. The old output was
  // a "GOOD SERVICE" header badge, two rows of filter chips, a three-tab
  // strip, and one expandable row whose entire content was a second
  // "GOOD SERVICE" badge and the words "Good Service" — three statements of
  // the same fact plus controls for narrowing a list of one non-issue.
  // Only when *every* status is Good Service: a line carrying both a good
  // service reading and a real disruption still needs the full list.
  const allGood = statuses.length > 0 && statuses.every((status) => status.statusSeverity === 10);
  if (allGood) {
    return <Text c="dimmed">Good service — no issues reported on this line.</Text>;
  }
```

In `frontend/app/stations/page.tsx`:

```tsx
      <Text c="dimmed">
        Search by station name or CRS code to see disruptions affecting lines through it.
      </Text>
```

In `frontend/app/lines/[id]/not-found.tsx`:

```tsx
import { Group, Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function LineNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Line not found</Title>
      <Text c="dimmed">No line matches that ID.</Text>
      {/* The page previously had no link anywhere on it — a genuine dead
          end reachable from a stale bookmark or a deleted custom line. */}
      <Group gap="lg">
        <TextLink href="/lines" underline="always">
          Browse all lines
        </TextLink>
        <TextLink href="/" underline="always">
          Back to your dashboard
        </TextLink>
      </Group>
    </Stack>
  );
}
```

- [ ] **Step 4: Run the full suite**

Run: `cd frontend && npm test`
Expected: PASS.

- [ ] **Step 5: Verify against the running stack**

Rebuild, then check: `/stations` subtitle matches its field label; `/lines/does-not-exist` offers two working links; a Good Service line's detail page shows one dimmed sentence where the filter block used to be; a line with a real disruption is unchanged.

- [ ] **Step 6: Run the e2e suite as a final gate**

Run: `cd frontend && npm run test:e2e`
Expected: PASS, or a clear report of which specs need updating for the new markup — do not leave a red e2e suite behind.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/stations/page.tsx frontend/app/lines/\[id\]/not-found.tsx frontend/components/IssueList.tsx frontend/components/IssueList.test.tsx
git commit -m "Fix lookup-page copy, the line not-found dead end, and Good Service duplication"
```

---

## Findings corrected or dropped after reading the source

Recorded so they are not re-opened.

- **Punch-list item 6, "blank mobile render of `/lines/{id}/edit`" — dropped, it is a capture artifact, not a bug.** The review flagged `line-edit_not-applicable__mobile-390x844.png` as 100% white and asked for one manual repro. Reading the code path: `app/lines/[id]/edit/page.tsx` calls `notFound()` when `getCustomLine` 404s for a catalogue line, and the nearest boundary is `app/lines/[id]/not-found.tsx` — the *same* component `/lines/{unknown-id}` renders. Two facts settle it. The desktop captures of the two routes are **byte-identical** (`md5sum` of `line-detail_not-found__desktop-1440x900.png` and `line-edit_not-applicable__desktop-1440x900.png` both `156cbd19…`), confirming they are the same render. And `line-detail_not-found__mobile-390x844.png` — that identical component, at that identical viewport — renders perfectly, header and all. A component cannot render correctly and blank at the same width in the same run; the blank frame was caught mid-navigation. No fix is planned. (Task 14 does add a CTA to that page, but for the dead-end finding, not this one.)

- **Punch-list item 10 / viewport issue 6, "theme-toggle A badge clipped by the header edge" — half-corrected.** The badge is not clipped in any capture inspected (`home_default__mobile-390x844.png`, `lines_default__tablet-834x1194.png`, `line-detail_not-found__mobile-390x844.png`). The "reads as a rendering accident" half of the complaint stands and is addressed in Task 12; the clipping half does not exist. Do not go looking for it.

- **Viewport issue 4, "tooltip z-index" — cause corrected, symptom real.** See Task 12's note: the tooltip is portalled at `z-index: 300` and the nav creates no stacking context, so this is a `position="top"` problem, not a layering one.

Not planned, deliberately:

- **Viewport issue 9, the operator-filter dropdown overlaying the table header.** That is how a Mantine `Combobox` dropdown works — an overlay, not a content-pusher. Pushing content instead would make the table jump on every keystroke. No change.
- **Review b.7, raw CRS codes in the line-definition tooltip.** Resolving ~20 codes to names needs either a bulk CRS→name endpoint (which does not exist — `/public/stations` is a substring type-ahead) or twenty separate requests per tooltip. Worth doing, but it is a backend change and belongs in its own plan.
- **Review c.4, no reorder affordance on the edit form's ordered station list.** Out of scope for a UX-fix pass; it is a new interaction, not a defect.
