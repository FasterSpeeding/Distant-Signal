# Tracked Trains on the Home Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Blockers:** None. Every dependency this plan needs is already live and unmodified-suitable: `getMyTrackedTrains()` (`frontend/lib/api.ts`), `TrackedTrainListItem` (`frontend/lib/types.ts`), and `GET /Train/mine` (`crates/api`) all already exist, are already exercised end-to-end by `frontend/app/track/mine/page.tsx`, and need zero changes for this plan. This plan is ready to implement today.

**Goal:** Give a logged-in user with at least one tracked train a reason to notice it from their actual landing page. Add a condensed "Your Tracked Trains" section to the bottom of the home page (`frontend/app/page.tsx`) — up to 5 rows, most-recently-tracked first, with a "View all" link to `/track/mine` — hidden entirely for a logged-out visitor or a logged-in visitor with nothing tracked. No backend change, no new API call shape, no new refresh mechanism: this reuses `getMyTrackedTrains()` (`GET /Train/mine`) exactly as `/track/mine` already calls it.

**Architecture:**

```
frontend/app/page.tsx   Promise.all([
                           getPreferences(),        existing, now run concurrently
                           getLineStatusForMode(),   existing, now run concurrently
                           getMyTrackedTrains(),     NEW -- reused unmodified from lib/api.ts
                         ])
                         trackedTrains = (result ?? []).slice(0, 5)
                         <Stack>
                           Your Lines            (unchanged)
                           Your Stations         (unchanged)
                           Your Tracked Trains   NEW -- rendered only if trackedTrains.length > 0
                             up to 5 rows, trackedAt DESC (server-ordered, not re-sorted)
                             "View all" -> /track/mine
                         </Stack>
        │ no-store, cookie-fwd (existing function, existing route, no backend change)
        ▼
GET /Train/mine   (crates/api, UNCHANGED)
```

**Tech Stack:** Next.js App Router + TypeScript + Mantine v9, Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`). Entirely frontend; no Rust/backend work in this plan.

**Spec:** `docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md` — read in full before starting; this plan does not restate its research, only carries its decisions into concrete tasks. Cross-references below to "Decision N" / "Finding N" / "Corrections" refer to that document.

**Status note:** confirmed by direct inspection while writing this plan (2026-09-01), not assumed from the spec:

- `frontend/app/page.tsx` (read in full) is an async Server Component, `export const revalidate = 0`, with **no `getSession()` call anywhere** — confirmed both by reading the whole file and by `git log --oneline -- frontend/app/page.tsx`, whose most recent commit is `d268d75` ("Fix dashboard TfL/NR duplicate pin..."), well before this spec was written; the anonymous-user-ux spec's own home-page redesign (a session-branching page) has **not** landed, exactly as the design spec's Corrections section states.
- The page currently renders exactly **two** sections, both inside one top-level `<Stack p="lg" gap="xl">`: "Your Lines" (`Title order={1}`, `SimpleGrid` of `LineStatusCard`s or dimmed empty-state text linking to `/lines`) and "Your Stations" (`Title order={2}`, `Stack` of `Card`-wrapped `Link`s or dimmed empty-state text linking to `/stations`). No third section of any kind exists today.
- Data fetching today is **not** fully concurrent: `const preferences = await getPreferences();` then `const allReports = await getLineStatusForMode(DISPLAYED_MODES_PARAM);` are two sequential top-level awaits (only `pinnedStationEntries`'s own inner `Promise.all(...)` over per-station reads is concurrent). Decision 3 of the design spec calls for fetching `getMyTrackedTrains()` "via `Promise.all` alongside `getPreferences()` and `getLineStatusForMode(...)`, not awaited sequentially after them" — implementing that decision as written means restructuring these two existing sequential awaits into a `Promise.all` alongside the new fetch. This is a real, if small, change to already-existing code, called out explicitly here so it isn't missed. There is no data dependency between the three calls (`preferences` is only read afterward, to filter `allReports`), so this restructuring is safe.
- `getMyTrackedTrains()` (`frontend/lib/api.ts`, read in full): `GET /Train/mine`, cookie-forwarded, `cache: 'no-store'`, returns `null` on `401`, `TrackedTrainListItem[]` on `200`. Already used unmodified by `frontend/app/track/mine/page.tsx`. **No changes needed or made to this function.**
- `TrackedTrainListItem` (`frontend/lib/types.ts`, read in full) already has every field this plan's row needs: `id`, `serviceDate`, `pinOriginCrs`, `pinDestinationCrs`, `pinScheduledDeparture`, `resolutionStatus`, `trainUid`, `status`, `delayMinutes`, `trackedAt`. **No type changes needed.**
- `frontend/app/track/mine/page.tsx` (read in full) has a local, unexported `TrackedTrainListRow` + `RowStatusBadge` + `STATUS_LABELS` (lines 58–143). The design spec's Testing/Explicitly-out-of-scope sections both treat extracting these into a shared component as a reasonable but **non-mandated** implementation-time choice ("the home page's row can equally well be a smaller, independently-written variant if that turns out cleaner in practice"). This plan takes that option: it writes a small, page-local, trimmed variant in `app/page.tsx` rather than modifying `track/mine/page.tsx` or creating a new shared component file — keeping this plan's footprint to one file plus its test, matching the design's "no new visual language, no extraction mandated" framing.
- No `frontend/app/page.test.tsx` exists today (confirmed: file not found) — this plan's test file is new coverage for `app/page.tsx`, not an extension of an existing one, and (per Global Constraints below) is scoped only to the new section's outcomes, not full regression coverage of the pre-existing two sections.
- `AutoRefresh` (`frontend/components/AutoRefresh.tsx`, mounted once in `app/layout.tsx`) already re-runs this page's `no-store` fetches every 30s, with no per-route opt-out — this plan adds no new refresh mechanism, per Decision 5.

## Global Constraints

- **No backend changes.** No task in this plan may touch `crates/api` or any other Rust crate, or add a database migration. `GET /Train/mine` and `list_tracked_trains_for_user` are reused exactly as they exist today (Decision 3's explicit rejection of a `?limit=` parameter).
- **No changes to `frontend/lib/api.ts` or `frontend/lib/types.ts`.** `getMyTrackedTrains()` and `TrackedTrainListItem` are reused completely unmodified — both already have everything this plan needs.
- **No changes to `frontend/app/track/mine/page.tsx` or `frontend/app/layout.tsx`.** The nav entry to `/track/mine` (`TrackedTrainsNavItem`) already exists and needs no change; this plan does not extract or refactor that page's local row-rendering helpers (see Status note).
- **Cap is exactly 5 rows, done client-side via `.slice(0, 5)` after the fetch — no new query parameter.** The backend query is already ordered `tracked_at DESC`; "first 5 of the response" already is "5 most recently tracked" (Decision 1/3). Do not re-sort.
- **The section is either fully present or entirely absent — no loading skeleton, no partial/placeholder state, no empty-state text of its own.** Render nothing (no heading, no card, no text) when the post-slice list is empty, whether that's because `getMyTrackedTrains()` returned `null` (logged out) or `[]` (logged in, nothing tracked) — both collapse to the same "don't render this section" outcome (Decision 4). Do not add a "you haven't tracked anything yet" nudge here; that copy already exists on `/track/mine`.
- **No `getSession()` call added to `app/page.tsx`.** `getMyTrackedTrains()`'s `null`-on-401 return is the complete "not logged in" signal, exactly as `/track/mine` already relies on with no separate session check (Decision 3/5).
- **No new refresh mechanism, polling interval, or client-side "check now" affordance.** This section is an ordinary part of the existing `no-store` Server Component read, already covered by the global 30s `AutoRefresh`. Do not add a per-route opt-out or a faster refresh path.
- **Placement is fixed: a third `Stack` section, `Title order={2}`, directly below "Your Stations," inside the page's existing single top-level `Stack`.** Not a sidebar, not above "Your Lines" — Decision 2 is explicit about this and it is not this plan's call to revisit.
- **No "active only" or any other status filter on which rows appear.** The 5 rows are whatever `trackedAt DESC` gives, unfiltered (Decision 1) — do not attempt to hide cancelled, completed-looking, or stale trains; the backend has no reliable signal for that (Finding 1 of the parent spec, restated in this spec's Current relevant state).
- **Row content and visual language:** route (`origin → destination` or bare origin), `serviceDate` + `pinScheduledDeparture` (via the existing `formatDate`/`formatTime` from `frontend/lib/dateFormat.ts`), and the same status/delay badge treatment `/track/mine`'s `RowStatusBadge` already uses (`resolutionStatus` vs. `status` + `delayMinutes` branching, same colors/labels). Do not invent new copy, colors, or badge semantics — mirror the existing sibling page's row, written as a page-local variant per the Status note.
- **Do not build the shared-component extraction of `TrackedTrainListRow`/`RowStatusBadge`.** Explicitly out of scope per the design spec; this plan's Task 1 writes a small, independent, page-local version instead (see Status note).
- **Test scope is the new section's outcomes only** (per the design spec's own Testing section): `getMyTrackedTrains()` returning `null` (section absent), `[]` (section absent), and a populated list (section present, capped at 5 even when more than 5 are returned, "View all" link present and pointing at `/track/mine`, rows in the order returned — no client-side re-sorting). Do not attempt full regression coverage of the pre-existing "Your Lines"/"Your Stations" sections as part of this plan; assert only that they still render (headings present) alongside the new section, not their full existing behavior.
- **Testing convention:** colocated `*.test.tsx`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`). The task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures.

---

### Task 1: Frontend — add the "Your Tracked Trains" section to the home page

**Files:**
- Modify: `frontend/app/page.tsx`
- Create: `frontend/app/page.test.tsx`

**Interfaces:**
- Consumes: `getMyTrackedTrains` (`frontend/lib/api.ts`, unmodified), `TrackedTrainListItem` (`frontend/lib/types.ts`, unmodified), `formatDate`/`formatTime` (`frontend/lib/dateFormat.ts`, unmodified).
- Produces: no new exports — `DashboardPage`'s rendered output gains a third, conditionally-rendered section. Two new page-local, unexported helpers: `TrackedTrainSummaryRow`, `TrackedTrainStatusBadge`.

- [ ] **Step 1: Restructure the top-level fetches into a `Promise.all`, adding `getMyTrackedTrains()`**

In `frontend/app/page.tsx`, change:

```tsx
export default async function DashboardPage() {
  const preferences = await getPreferences();

  // Every displayed mode, not just national-rail: a pinned TfL line would
  // otherwise be silently missing from "Your Lines".
  const allReports = await getLineStatusForMode(DISPLAYED_MODES_PARAM);
```

to:

```tsx
export default async function DashboardPage() {
  // Concurrent, independent fetches -- getMyTrackedTrains() has no data
  // dependency on preferences or line status (its own null-on-401 return is
  // the complete "not logged in" signal; no getSession() call needed here,
  // mirroring /track/mine's own established reasoning), so serializing it
  // after the other two would only add latency for no reason. Mirrors this
  // page's existing pinnedStationEntries Promise.all precedent below. Per
  // docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md
  // Decision 3.
  const [preferences, allReports, myTrackedTrains] = await Promise.all([
    getPreferences(),
    // Every displayed mode, not just national-rail: a pinned TfL line would
    // otherwise be silently missing from "Your Lines".
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
    getMyTrackedTrains(),
  ]);
```

Add `getMyTrackedTrains` to the existing `@/lib/api` import:

```tsx
import { getLineStatusForMode, getMyTrackedTrains, getPreferences, getStationName, getStopPointDisruption } from '@/lib/api';
```

Add a new import for the date helpers, and widen the existing type-only import:

```tsx
import { formatDate, formatTime } from '@/lib/dateFormat';
import type { LineStatusReport, TrackedTrainListItem } from '@/lib/types';
```

Add `Badge` to the existing `@mantine/core` import:

```tsx
import { Badge, Stack, Title, SimpleGrid, Text, Group, Card } from '@mantine/core';
```

- [ ] **Step 2: Slice to 5, after the existing `pinnedStationEntries` block**

Directly after the `pinnedStationEntries` `Promise.all(...)` block (before the `return (`), add:

```tsx
  // null (not logged in) collapses to [] -- the same "hide entirely"
  // treatment a logged-in user with zero tracked trains gets (Decision 4 of
  // the design spec). slice(0, 5) of an already trackedAt-DESC-ordered
  // response is "5 most recently tracked" with no client-side re-sort
  // needed (Decision 1/3) -- the backend query is already ordered that way.
  const trackedTrains = (myTrackedTrains ?? []).slice(0, 5);
```

- [ ] **Step 3: Render the third section**

Directly after the closing `</Stack>` of the existing "Your Stations" `Stack` (i.e. as the next sibling inside the page's top-level `<Stack p="lg" gap="xl">`, before that outer `Stack`'s own closing tag), add:

```tsx
      {trackedTrains.length > 0 && (
        <Stack gap="md">
          <Group justify="space-between">
            <Title order={2}>Your Tracked Trains</Title>
            <TextLink href="/track/mine">View all</TextLink>
          </Group>
          <Stack gap="xs">
            {trackedTrains.map((train) => (
              <TrackedTrainSummaryRow key={train.id} train={train} />
            ))}
          </Stack>
        </Stack>
      )}
```

`TextLink` is already imported on this page (used by "Your Lines"/"Your Stations"). No new import needed for it.

- [ ] **Step 4: Add the page-local row and badge helpers**

At the end of `frontend/app/page.tsx`, after `DashboardPage`'s closing brace, add:

```tsx
// Home-page-local mirror of /track/mine's own row shape
// (frontend/app/track/mine/page.tsx's TrackedTrainListRow/RowStatusBadge/
// STATUS_LABELS) -- same fields, same resolutionStatus-vs-status+
// delayMinutes branching, same words and colors. Deliberately NOT imported
// from that file or extracted into a shared component: per the design
// spec's Testing/Explicitly-out-of-scope sections, that extraction is a
// reasonable but non-mandated implementation-time choice, and this page
// having no import dependency on /track/mine's file keeps this change
// scoped to one file.
function TrackedTrainSummaryRow({ train }: { train: TrackedTrainListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- same logic as /track/mine's own row. The
  // resolved-with-null-trainUid fallback is defensive: the backend's own
  // resolution invariant means this shouldn't happen, but this component
  // doesn't assume it.
  const href =
    train.resolutionStatus === 'resolved' && train.trainUid
      ? `/train/${train.trainUid}/${train.serviceDate}`
      : `/train/by-id/${train.id}`;

  const route = train.pinDestinationCrs ? `${train.pinOriginCrs} → ${train.pinDestinationCrs}` : train.pinOriginCrs;

  return (
    <Link href={href} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder>
        <Stack gap={4}>
          <Group justify="space-between" wrap="nowrap">
            <Text fw={500}>{route}</Text>
            <TrackedTrainStatusBadge train={train} />
          </Group>
          <Text size="sm" c="dimmed">
            {formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}
          </Text>
        </Stack>
      </Card>
    </Link>
  );
}

// Short, human badge words -- copied verbatim from /track/mine's own
// STATUS_LABELS so the two pages never disagree about wording for the same
// underlying tokens. Falls back to the raw token itself for anything
// unlisted, so an unexpected value never disappears from the badge.
const STATUS_LABELS: Record<string, string> = {
  pending: 'Pending match',
  unresolved: 'Unmatched',
  awaiting_activation: 'Not yet started',
  en_route: 'En route',
  completed: 'Completed',
  cancelled: 'Cancelled',
};

function TrackedTrainStatusBadge({ train }: { train: TrackedTrainListItem }) {
  // pending/unresolved show the resolution status itself -- no journey
  // status exists yet for either. Once resolved, the journey status plus a
  // delay badge takes over. No "active only" filter and no attempt to
  // distinguish a genuinely-finished journey from one that's merely gone
  // quiet -- per Decision 1/Finding 1 of the design spec, the backend can't
  // honestly support that distinction today.
  if (train.resolutionStatus !== 'resolved') {
    return (
      <Badge color={train.resolutionStatus === 'unresolved' ? 'red' : 'gray'} variant="light">
        {STATUS_LABELS[train.resolutionStatus] ?? train.resolutionStatus}
      </Badge>
    );
  }
  return (
    <Group gap={6} wrap="nowrap">
      {train.status && (
        <Badge color={train.status === 'cancelled' ? 'red' : 'gray'} variant="light">
          {STATUS_LABELS[train.status] ?? train.status}
        </Badge>
      )}
      {train.delayMinutes !== null && (
        <Badge color={train.delayMinutes > 0 ? 'orange' : 'green'} variant="light">
          {train.delayMinutes > 0 ? `${train.delayMinutes}m late` : 'On time'}
        </Badge>
      )}
    </Group>
  );
}
```

- [ ] **Step 5: Write `frontend/app/page.test.tsx`**

Create `frontend/app/page.test.tsx`, mocking `@/lib/api` and calling the async page function directly — the same "await the async Server Component, then render" technique `track/mine/page.test.tsx` established. Per Global Constraints, this covers only the new section's outcomes plus a check that the pre-existing two sections still render alongside it:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import DashboardPage from './page';
import * as api from '@/lib/api';
import type { TrackedTrainListItem } from '@/lib/types';

vi.mock('@/lib/api');

function item(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-08-31',
    pinOriginCrs: 'WAT',
    pinDestinationCrs: 'WOK',
    pinScheduledDeparture: '2026-08-31T18:32:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'C21373',
    status: 'en_route',
    delayMinutes: 4,
    trackedAt: '2026-08-31T12:00:00Z',
    ...overrides,
  };
}

// Minimal, shared stubs for the two pre-existing fetches this page also
// makes -- not under test here, just enough for the page to render without
// throwing.
beforeEach(() => {
  vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
  vi.mocked(api.getLineStatusForMode).mockResolvedValue([]);
});

describe('DashboardPage -- Your Tracked Trains section', () => {
  it('getMyTrackedTrains() returns null (logged out): section absent, other two sections unchanged', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your Lines' })).toBeInTheDocument();
    expect(screen.getByRole('heading', { name: 'Your Stations' })).toBeInTheDocument();
  });

  it('getMyTrackedTrains() returns [] (logged in, nothing tracked): section absent', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    renderWithMantine(await DashboardPage());
    expect(screen.queryByRole('heading', { name: 'Your Tracked Trains' })).not.toBeInTheDocument();
  });

  it('populated list: section present with a "View all" link to /track/mine', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Tracked Trains' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/track/mine');
  });

  it('more than 5 tracked trains: only the first 5 (as returned) are rendered', async () => {
    const trains = Array.from({ length: 7 }, (_, i) => item({ id: i + 1, pinOriginCrs: `T${i + 1}` }));
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(trains);
    renderWithMantine(await DashboardPage());
    for (let i = 1; i <= 5; i++) {
      expect(screen.getByText(new RegExp(`^T${i}`))).toBeInTheDocument();
    }
    expect(screen.queryByText(/^T6/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^T7/)).not.toBeInTheDocument();
  });

  it('rows render in the order getMyTrackedTrains returned them (no client-side re-sort)', async () => {
    const first = item({ id: 1, pinOriginCrs: 'WAT' });
    const second = item({ id: 2, pinOriginCrs: 'PAD' });
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([first, second]);
    renderWithMantine(await DashboardPage());

    const links = screen.getAllByRole('link');
    const originOrder = links
      .map((link) => link.textContent ?? '')
      .filter((text) => text.startsWith('WAT') || text.startsWith('PAD'));
    expect(originOrder).toEqual([expect.stringMatching(/^WAT/), expect.stringMatching(/^PAD/)]);
  });

  it('renders a delay badge for a resolved, delayed train', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item({ delayMinutes: 12 })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText('12m late')).toBeInTheDocument();
  });

  it('resolved train with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([item()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/C21373/2026-08-31');
  });

  it('pending train: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([
      item({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: /WAT → WOK/ })).toHaveAttribute('href', '/train/by-id/1');
  });
});
```

**Implementation-time verification note:** confirm the exact shape `getPreferences()`/`getLineStatusForMode()` are expected to resolve to (used only as harmless stubs here) by checking their current return types in `frontend/lib/types.ts`/`frontend/lib/api.ts` — adjust the `beforeEach` stub values if the actual shapes differ from `{ pinnedLines: [], pinnedStations: [] }` / `[]`.

- [ ] **Step 6: Run the frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS, including the new `page.test.tsx` suite, with no new failures anywhere else.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/page.tsx frontend/app/page.test.tsx
git commit -m "Add condensed Your Tracked Trains section to the home page"
```
