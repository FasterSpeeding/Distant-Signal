# Collapsible Station Timetable Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a collapsed-by-default "Scheduled departures" accordion section to the station page (`frontend/app/stations/[crs]/page.tsx`) that lists CIF-schedule-derived departures for that station, cross-linked to each train's live status page.

**Architecture:** A new `'use client'` component, `frontend/components/StationTimetable.tsx`, wraps a single Mantine `Accordion`/`AccordionItem` (collapsed by default, `keepMounted={false}`) around a fetch-on-expand data view. It calls the existing, unchanged `GET /api/trains/search?station=<crs>` proxy route (same envelope, same row shape, same pagination as `TrainSearchForm.tsx` already uses) with no other query parameters, and is rendered once from `StationDisruptionPage` near the bottom of the page. No backend code changes at all.

**Tech Stack:** Next.js (App Router) Client Component, TypeScript, Mantine v9 (`Accordion`, `Text`, `Button`, `Alert`, `Stack`, `Group`), Vitest + `@testing-library/react` for frontend tests (`frontend/test/render.tsx`'s `renderWithMantine` helper), `vi.stubGlobal('fetch', ...)` mocking, dayjs for the once-per-render "today" date.

**Spec:** `docs/superpowers/specs/2026-09-12-station-timetable-design.md`

## Global Constraints

- **No new backend routes, query parameters, indexes, or migrations.** The feature consumes `GET /public/trains/search?station=<crs>` (mounted at `/trains/search` in `crates/api/src/routes/trains.rs`, reached from the browser through the same-origin proxy at `/api/trains/search?station=<crs>`) completely unchanged (spec §0.3, §2).
- **Query parameter shape:** exactly one parameter is sent by this feature: `station=<CRS>`, uppercased (matching `normalize_crs`). No `date`, `from`, `to`, `origin`, or `stops_at` are ever sent by this component (spec §2, Non-goals).
- **Pagination:** keyset pagination via the response envelope `{ results: TrainSearchRow[], nextCursor: string | null }`. "Load more" re-issues the request with `after=<nextCursor>` and the same `station` value appended, and appends new rows to the existing list rather than replacing them. `nextCursor` is an explicit `null` (never omitted) on the last page. Mirrors `TrainSearchForm.tsx::handleLoadMore` (`frontend/components/TrainSearchForm.tsx:285-334`).
- **Response/state distinction:** `404` means "nothing published for this day" (copy: "isn't available yet"); `200` with an empty `results` array means "published, but no matches" (copy: "No scheduled departures found for the rest of today."). These must never be collapsed into one state (spec §3.3, §0.4).
- **No headcode, no operator column, ever**, for this CIF-derived data (spec §0.5).
- **No calling-points list, no station-name resolution** — bare CRS codes only, matching `TrainSearchForm.tsx` exactly (spec §3.2).
- **Cross-link every row** to `/train/{uid}/{date}` via `TextLink`, `date` computed once per render as `dayjs().format('YYYY-MM-DD')` (today; there is no date picker on this component) — spec §0.6, §3.2.
- **Collapsed via Mantine `Accordion`, `keepMounted={false}`** on the `AccordionPanel`, exactly matching `IssueList.tsx`'s existing convention (`frontend/components/IssueList.tsx:331`) — this is what makes "collapsed by default" mean "not rendered" rather than "visually hidden" (spec Decision 3-4).
- **Fetch fresh on every expand, no cross-expand cache** — because the panel unmounts on collapse, component-local state is naturally discarded; no `localStorage`/SWR/react-query layer is introduced (spec Decision 5).
- **Default window is the route's own `now`-forward-on-today default** — omit `from`/`to`/`date` entirely; do not add any client-side wall-clock windowing (spec Decision 6).
- **Test framework:** Vitest, file `frontend/components/StationTimetable.test.tsx`, following `IssueList.test.tsx`'s `screen.queryByText(...).not.toBeInTheDocument()` convention for genuine non-rendering and `TrainSearchForm.test.tsx`'s `vi.stubGlobal('fetch', ...)`/`searchCallUrl` helpers for asserting exact request URLs. Run via `npm test` (`vitest run`) from `frontend/`.
- **No new backend tests required** — spec §5.1 confirms the exact request shape this feature issues is already covered by existing, passing `db_tests` in `crates/api/src/routes/trains.rs` (`trains_search_with_no_explicit_from_still_defaults_to_the_now_floor_on_today`, `trains_search_omitting_date_still_defaults_to_today`, `trains_search_renders_camel_case_rows_with_trimmed_time_and_station_attached`, `trains_search_nothing_published_for_today_is_a_404`, `trains_search_published_day_with_no_matches_is_200_with_an_empty_results_array`, `trains_search_paginates_with_a_cursor_and_after_continues_from_it`). No Rust changes are part of this plan.

---

## File Structure

- **Create:** `frontend/components/StationTimetable.tsx` — the new client component, its wire-shape types (`TrainSearchRow`, `TrainSearchResponse`, `Results`), and all fetch/render logic. One file, mirroring `TrainSearchForm.tsx`'s own scope (a single component file owning its own state and rendering).
- **Create:** `frontend/components/StationTimetable.test.tsx` — Vitest coverage for every state and interaction.
- **Modify:** `frontend/app/stations/[crs]/page.tsx` — render `<StationTimetable crs={crs} />` after the existing "Sample stats by operator" section (the last section on the page today, `page.tsx:253-273`).
- **Modify:** `frontend/app/stations/[crs]/page.test.tsx` — add coverage asserting the new section renders on the station page.

## Task 1: `StationTimetable` skeleton — collapsed accordion, no data yet

**Files:**
- Create: `frontend/components/StationTimetable.tsx`
- Test: `frontend/components/StationTimetable.test.tsx`

**Interfaces:**
- Consumes: `Accordion`/`AccordionItem`/`AccordionControl`/`AccordionPanel` from `@mantine/core` (same import shape as `frontend/components/IssueList.tsx:4-17`).
- Produces: `export function StationTimetable({ crs }: { crs: string })` — the component every later task extends and that Task 4 renders from the station page.

- [ ] **Step 1: Write the failing test for collapsed-by-default rendering**

Create `frontend/components/StationTimetable.test.tsx`:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { StationTimetable } from './StationTimetable';

describe('StationTimetable', () => {
  it('renders collapsed by default: the control is present, but no fetch happens and no panel content is in the document', () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<StationTimetable crs="RDG" />);

    expect(screen.getByRole('button', { name: 'Scheduled departures' })).toBeInTheDocument();
    expect(screen.queryByText('Loading scheduled departures…')).not.toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run (from `frontend/`): `npx vitest run components/StationTimetable.test.tsx`
Expected: FAIL — `Cannot find module './StationTimetable'` (the component does not exist yet).

- [ ] **Step 3: Write the minimal component**

Create `frontend/components/StationTimetable.tsx`:

```tsx
'use client';

import { Accordion, AccordionControl, AccordionItem, AccordionPanel, Stack, Text } from '@mantine/core';

/** Collapsed-by-default "Scheduled departures" section for the station page
 * (`frontend/app/stations/[crs]/page.tsx`), listing CIF-schedule-derived
 * departures for `crs` via the existing, unchanged
 * `GET /public/trains/search?station=<crs>` route -- see
 * docs/superpowers/specs/2026-09-12-station-timetable-design.md.
 *
 * `keepMounted={false}` on the panel matches `IssueList.tsx`'s own
 * documented reasoning verbatim: Mantine v9 keeps a collapsed panel's
 * content mounted (via the Activity API) purely visually hidden by
 * default, which `screen.queryByText` (and a screen reader in "not
 * visible" mode, inconsistently) can still find. `keepMounted={false}`
 * makes "collapsed by default" actually mean "not rendered" until first
 * expanded.
 *
 * One `AccordionItem`, not `multiple`: there is only one section here,
 * unlike `IssueList.tsx`'s per-status accordion. */
export function StationTimetable({ crs }: { crs: string }) {
  return (
    <Accordion keepMounted={false}>
      <AccordionItem value="scheduled-departures">
        <AccordionControl>Scheduled departures</AccordionControl>
        <AccordionPanel>
          <Stack gap="xs">
            <Text size="sm" c="dimmed">
              Loading scheduled departures…
            </Text>
          </Stack>
        </AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `npx vitest run components/StationTimetable.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/StationTimetable.tsx frontend/components/StationTimetable.test.tsx
git commit -m "feat: add collapsed StationTimetable skeleton"
```

## Task 2: Fetch on first expand — loading, success, empty, and unpublished states

**Files:**
- Modify: `frontend/components/StationTimetable.tsx`
- Test: `frontend/components/StationTimetable.test.tsx`

**Interfaces:**
- Consumes: `StationTimetable({ crs })` from Task 1.
- Produces: the `TrainSearchRow`/`TrainSearchResponse`/`Results` types and `resultsContent()` rendering function that Task 3 (pagination, re-fetch) and Task 4 (escape hatch, wiring) build on. `Results` is:

```ts
type Results =
  | { rows: TrainSearchRow[]; nextCursor: string | null }
  | 'unpublished'
  | 'error'
  | null;
```

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/StationTimetable.test.tsx` (keep Task 1's `describe`/first `it`; add these alongside it, plus the shared fixtures above the `describe` block):

```tsx
/** Builds a `GET /public/trains/search` response body -- same envelope
 * shape `TrainSearchForm.test.tsx::searchBody` builds against the same
 * route. */
function searchBody(
  rows: Array<{
    uid: string;
    scheduled: string;
    stationCrs: string;
    originCrs: string | null;
    destinationCrs: string | null;
  }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({
    results: rows.map((row) => ({ destinationArrival: null, destinationArrivalDayOffset: 0, ...row })),
    nextCursor,
  });
}

const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', stationCrs: 'RDG', originCrs: 'PAD', destinationCrs: 'BRI' },
  { uid: 'C10002', scheduled: '10:05', stationCrs: 'RDG', originCrs: 'WAT', destinationCrs: 'EXD' },
];

function expand() {
  return screen.getByRole('button', { name: 'Scheduled departures' });
}
```

Then add these test cases inside the existing `describe('StationTimetable', ...)` block:

```tsx
  it('fetches exactly once, to /api/trains/search?station=<CRS> uppercased, on first expand', async () => {
    const fetchMock = vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 })));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="rdg" />);

    fireEvent.click(expand());

    await waitFor(() => expect(fetchMock).toHaveBeenCalledTimes(1));
    expect(fetchMock).toHaveBeenCalledWith('/api/trains/search?station=RDG');
  });

  it('shows a loading state between expand and the fetch resolving', () => {
    const fetchMock = vi.fn(() => new Promise(() => {})); // never resolves
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(screen.getByText('Loading scheduled departures…')).toBeInTheDocument();
  });

  it('renders one row per result, with time/origin/destination and a link to the live status page for today', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(await screen.findByText('08:22 · PAD → RDG → BRI')).toBeInTheDocument();
    expect(screen.getByText('10:05 · WAT → RDG → EXD')).toBeInTheDocument();
    const links = screen.getAllByRole('link', { name: 'View live status' });
    const today = new Date().toISOString().slice(0, 10);
    expect(links[0]).toHaveAttribute('href', `/train/C10001/${today}`);
  });

  it('renders a "?" placeholder when origin or destination is unresolved', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(() =>
        Promise.resolve(
          new Response(
            searchBody([{ uid: 'C99999', scheduled: '09:00', stationCrs: 'RDG', originCrs: null, destinationCrs: 'BRI' }]),
            { status: 200 },
          ),
        ),
      ),
    );
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(await screen.findByText('09:00 · ? → RDG → BRI')).toBeInTheDocument();
  });

  it('shows the "no matches today" copy for a 200 with an empty results array', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody([]), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(await screen.findByText('No scheduled departures found for the rest of today.')).toBeInTheDocument();
  });

  it('shows the "not available yet" copy on a 404, distinct from the empty-results copy', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response('not found', { status: 404 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText("Today's scheduled timetable data isn't available yet."),
    ).toBeInTheDocument();
    expect(screen.queryByText('No scheduled departures found for the rest of today.')).not.toBeInTheDocument();
  });

  it('shows an error alert on a non-2xx, non-404 response', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response('boom', { status: 500 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText("Couldn't load the scheduled departures right now."),
    ).toBeInTheDocument();
  });

  it('shows an error alert when fetch itself throws', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.reject(new Error('network down'))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText("Couldn't load the scheduled departures right now."),
    ).toBeInTheDocument();
  });
```

Also add the two missing imports at the top of the test file:

```tsx
import { fireEvent, waitFor } from '@testing-library/react';
```

(combine with the existing `screen` import from `@testing-library/react`).

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/StationTimetable.test.tsx`
Expected: FAIL on every new test (no fetch is wired up yet; the component always renders the static "Loading…" text and never reads `crs` or calls `fetch`).

- [ ] **Step 3: Implement fetch-on-expand with all four response states**

Replace the contents of `frontend/components/StationTimetable.tsx`:

```tsx
'use client';

import { useState } from 'react';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Alert,
  Group,
  Stack,
  Text,
} from '@mantine/core';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';

/** Wire shape of `GET /public/trains/search`
 * (`crates/api/src/render.rs::calling_point_departure_json`), reused
 * verbatim from `TrainSearchForm.tsx`'s own (unexported, so redeclared
 * here) row/envelope types -- same route, same response, no new fields.
 * `originCrs`/`destinationCrs` are both nullable: either can be
 * unresolved for a real published schedule. This component never sends
 * `date`, so `destinationArrival`/`destinationArrivalDayOffset` are kept
 * for shape-fidelity with the wire response but unused here, same as
 * `TrainSearchForm.tsx`'s own posture for the latter field. */
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  stationCrs: string;
  originCrs: string | null;
  destinationCrs: string | null;
  destinationArrival: string | null;
  destinationArrivalDayOffset: number;
}

/** The envelope `GET /public/trains/search` returns -- not a bare array,
 * since the backend paginates a whole day's results. `nextCursor` is an
 * explicit `null` on the last page, never omitted. */
interface TrainSearchResponse {
  results: TrainSearchRow[];
  nextCursor: string | null;
}

/** Four mutually-exclusive states, checked top to bottom by
 * `resultsContent` below -- `'unpublished'` and an empty `rows` array are
 * genuinely different facts (spec §0.4/§3.3: a station outside this
 * feed's coverage must not read the same as "nothing's running right
 * now") and must not be collapsed into one copy. */
type Results =
  | { rows: TrainSearchRow[]; nextCursor: string | null }
  | 'unpublished'
  | 'error'
  | null;

/** Today's date, computed once per render for every row's live-status
 * link -- there is no date picker on this stripped-down view (spec
 * §3.2). */
function today(): string {
  return dayjs().format('YYYY-MM-DD');
}

export function StationTimetable({ crs }: { crs: string }) {
  const [expanded, setExpanded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [results, setResults] = useState<Results>(null);

  async function loadFirstPage() {
    setLoading(true);
    setResults(null);
    try {
      const response = await fetch(`/api/trains/search?station=${crs.toUpperCase()}`);
      if (response.status === 404) {
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setLoading(false);
    }
  }

  function handleChange(value: string | null) {
    const nowExpanded = value !== null;
    setExpanded(nowExpanded);
    if (nowExpanded) {
      void loadFirstPage();
    }
  }

  function resultsContent() {
    if (loading) {
      return (
        <Text size="sm" c="dimmed">
          Loading scheduled departures…
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Couldn&apos;t load">
          Couldn&apos;t load the scheduled departures right now.
        </Alert>
      );
    }
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          Today&apos;s scheduled timetable data isn&apos;t available yet.
        </Text>
      );
    }
    if (results === null) {
      return null;
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No scheduled departures found for the rest of today.
        </Text>
      );
    }
    const displayDate = today();
    return (
      <Stack gap="xs">
        {results.rows.map((row) => (
          <Group key={`${row.uid}-${row.scheduled}`} justify="space-between" wrap="nowrap">
            <Text size="sm">
              {row.scheduled} · {row.originCrs ?? '?'} → {row.stationCrs} → {row.destinationCrs ?? '?'}
            </Text>
            <TextLink href={`/train/${encodeURIComponent(row.uid)}/${displayDate}`}>
              View live status
            </TextLink>
          </Group>
        ))}
      </Stack>
    );
  }

  return (
    <Accordion keepMounted={false} onChange={handleChange}>
      <AccordionItem value="scheduled-departures">
        <AccordionControl>Scheduled departures</AccordionControl>
        <AccordionPanel>
          <Stack gap="xs">{resultsContent()}</Stack>
        </AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}
```

Note: `expanded` is tracked but not yet read anywhere in this task's rendering — Task 3 uses it to detect the collapse-then-re-expand transition. Keeping it here now (rather than adding it in Task 3) avoids a later step needing to re-touch the `handleChange` signature.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run components/StationTimetable.test.tsx`
Expected: PASS — all 8 tests (Task 1's + this task's 7).

- [ ] **Step 5: Commit**

```bash
git add frontend/components/StationTimetable.tsx frontend/components/StationTimetable.test.tsx
git commit -m "feat: fetch scheduled departures on first expand"
```

## Task 3: "Load more" pagination and fresh fetch on re-expand

**Files:**
- Modify: `frontend/components/StationTimetable.tsx`
- Test: `frontend/components/StationTimetable.test.tsx`

**Interfaces:**
- Consumes: `TrainSearchRow`, `TrainSearchResponse`, `Results`, `resultsContent()`, `loadFirstPage()`, `handleChange()` from Task 2.
- Produces: `handleLoadMore()` and the re-fetch-on-re-expand behavior that Task 4's escape-hatch/wiring task does not need to know about internally (it only renders `<StationTimetable crs={crs} />`).

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/StationTimetable.test.tsx`, alongside the existing fixtures:

```tsx
const PAGE_TWO = [
  { uid: 'C10003', scheduled: '11:40', stationCrs: 'RDG', originCrs: 'PAD', destinationCrs: 'BRI' },
];
```

And these test cases inside `describe('StationTimetable', ...)`:

```tsx
  it('shows Load more when nextCursor is non-null, and none when it is null', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE, null), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);
    fireEvent.click(expand());
    await screen.findByText('08:22 · PAD → RDG → BRI');
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('Load more fetches with after=<cursor> and station unchanged, and appends rather than replaces', async () => {
    const fetchMock = vi.fn((input: RequestInfo | URL) => {
      const url = String(input);
      if (url.includes('after=CURSOR1')) {
        return Promise.resolve(new Response(searchBody(PAGE_TWO, null), { status: 200 }));
      }
      return Promise.resolve(new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }));
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('11:40 · PAD → RDG → BRI')).toBeInTheDocument();
    expect(screen.getByText('08:22 · PAD → RDG → BRI')).toBeInTheDocument();
    expect(screen.getByText('10:05 · WAT → RDG → EXD')).toBeInTheDocument();
    expect(fetchMock).toHaveBeenNthCalledWith(2, '/api/trains/search?station=RDG&after=CURSOR1');
    await waitFor(() => expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument());
  });

  it('collapse then re-expand issues a fresh fetch rather than reusing the previous result set', async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(new Response(searchBody(PAGE_ONE), { status: 200 }))
      .mockResolvedValueOnce(new Response(searchBody(PAGE_TWO), { status: 200 }));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());
    await screen.findByText('08:22 · PAD → RDG → BRI');

    fireEvent.click(expand()); // collapse
    fireEvent.click(expand()); // re-expand

    await screen.findByText('11:40 · PAD → RDG → BRI');
    expect(screen.queryByText('08:22 · PAD → RDG → BRI')).not.toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/StationTimetable.test.tsx`
Expected: FAIL — "Load more" never renders (Task 2's `resultsContent` has no such button), and collapsing/re-expanding does not clear `results`, so the second test's `after=CURSOR1` assertion and the third test's fresh-fetch assertion both fail.

- [ ] **Step 3: Implement Load more and re-fetch-on-re-expand**

In `frontend/components/StationTimetable.tsx`:

Add a `loadingMore` state and a `handleLoadMore` function alongside `loadFirstPage`:

```tsx
  const [loadingMore, setLoadingMore] = useState(false);

  async function handleLoadMore() {
    if (results === null || results === 'error' || results === 'unpublished') return;
    if (results.nextCursor === null || loadingMore) return;
    setLoadingMore(true);
    try {
      const response = await fetch(
        `/api/trains/search?station=${crs.toUpperCase()}&after=${results.nextCursor}`,
      );
      if (!response.ok) {
        setResults((current) =>
          current !== null && current !== 'error' && current !== 'unpublished'
            ? { rows: current.rows, nextCursor: null }
            : current,
        );
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: [...current.rows, ...body.results], nextCursor: body.nextCursor }
          : current,
      );
    } catch {
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: current.rows, nextCursor: null }
          : current,
      );
    } finally {
      setLoadingMore(false);
    }
  }
```

Update `handleChange` so a collapse clears `results` (making the next expand a genuinely fresh fetch rather than relying on `keepMounted={false}` alone to discard state — `results`/`loading`/`loadingMore` live in the parent `StationTimetable` component, not the `AccordionPanel`'s own subtree, so they are NOT unmounted by `keepMounted={false}`):

```tsx
  function handleChange(value: string | null) {
    const nowExpanded = value !== null;
    setExpanded(nowExpanded);
    if (nowExpanded) {
      void loadFirstPage();
    } else {
      setResults(null);
    }
  }
```

Add the "Load more" button to `resultsContent()`, after the rows `Stack` in the `results.rows.length === 0` check's sibling branch (replace the final `return` of the rows branch):

```tsx
    const displayDate = today();
    return (
      <Stack gap="xs">
        <Stack gap="xs">
          {results.rows.map((row) => (
            <Group key={`${row.uid}-${row.scheduled}`} justify="space-between" wrap="nowrap">
              <Text size="sm">
                {row.scheduled} · {row.originCrs ?? '?'} → {row.stationCrs} → {row.destinationCrs ?? '?'}
              </Text>
              <TextLink href={`/train/${encodeURIComponent(row.uid)}/${displayDate}`}>
                View live status
              </TextLink>
            </Group>
          ))}
        </Stack>
        {results.nextCursor !== null && (
          <Group>
            <Button variant="default" size="xs" onClick={handleLoadMore} disabled={loadingMore} loading={loadingMore}>
              Load more
            </Button>
          </Group>
        )}
      </Stack>
    );
```

Add `Button` to the `@mantine/core` import list at the top of the file.

- [ ] **Step 4: Run tests to verify they pass**

Run: `npx vitest run components/StationTimetable.test.tsx`
Expected: PASS — all tests from Tasks 1-3.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/StationTimetable.tsx frontend/components/StationTimetable.test.tsx
git commit -m "feat: add Load more pagination and fresh fetch on re-expand"
```

## Task 4: Disclaimer copy, escape hatch, and wiring into the station page

**Files:**
- Modify: `frontend/components/StationTimetable.tsx`
- Modify: `frontend/app/stations/[crs]/page.tsx`
- Test: `frontend/components/StationTimetable.test.tsx`
- Test: `frontend/app/stations/[crs]/page.test.tsx`

**Interfaces:**
- Consumes: `StationTimetable({ crs })` from Task 3 (final component shape).
- Produces: nothing further downstream — this is the last task.

- [ ] **Step 1: Write the failing tests**

Add to `frontend/components/StationTimetable.test.tsx`, inside `describe('StationTimetable', ...)`:

```tsx
  it('shows a disclaimer above the rows once expanded with results', async () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    fireEvent.click(expand());

    expect(
      await screen.findByText(/These are from the scheduled timetable, not live running information/),
    ).toBeInTheDocument();
  });

  it('offers a link to the full /trains search, prefilled with this station, regardless of expand state', () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    expect(screen.getByRole('link', { name: /Search a different day or filter/ })).toHaveAttribute(
      'href',
      '/trains?station=RDG',
    );
  });

  it('notes that trains terminating at this station will not appear, and that no headcode/operator is shown', () => {
    vi.stubGlobal('fetch', vi.fn(() => Promise.resolve(new Response(searchBody(PAGE_ONE), { status: 200 }))));
    renderWithMantine(<StationTimetable crs="RDG" />);

    expect(
      screen.getByText(/only departures from this station -- trains that terminate here won't be listed/i),
    ).toBeInTheDocument();
  });
```

Add to `frontend/app/stations/[crs]/page.test.tsx`, inside `describe('StationDisruptionPage -- outage behaviour', ...)` (reusing that block's existing `beforeEach` mocks, which already stub every `lib/api` call the page makes):

```tsx
  it('renders the collapsed Scheduled departures section', async () => {
    await renderPage();
    expect(screen.getByRole('button', { name: 'Scheduled departures' })).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `npx vitest run components/StationTimetable.test.tsx app/stations/[crs]/page.test.tsx`
Expected: FAIL — the disclaimer/escape-hatch/terminating-trains copy does not exist yet in `StationTimetable.tsx`, and `page.tsx` does not render `StationTimetable` at all yet.

- [ ] **Step 3: Add the disclaimer, the terminating-trains note, and the escape hatch**

In `frontend/components/StationTimetable.tsx`, add `TextLink`-adjacent copy. Update the top-level render to always show the escape hatch (outside the `AccordionPanel`, so it is present regardless of expand state) and add the disclaimer/limitation copy inside the panel, above the rows-or-state content:

```tsx
  return (
    <Stack gap="xs">
      <Accordion keepMounted={false} onChange={handleChange}>
        <AccordionItem value="scheduled-departures">
          <AccordionControl>Scheduled departures</AccordionControl>
          <AccordionPanel>
            <Stack gap="xs">
              <Text size="sm" c="dimmed">
                These are from the scheduled timetable, not live running information, and may be up to 30
                minutes out of date. Open a train to see its live status.
              </Text>
              <Text size="sm" c="dimmed">
                This list shows only departures from this station -- trains that terminate here won&apos;t
                be listed, and neither headcode nor operator is available for scheduled-timetable rows.
              </Text>
              {resultsContent()}
            </Stack>
          </AccordionPanel>
        </AccordionItem>
      </Accordion>
      <TextLink href={`/trains?station=${crs.toUpperCase()}`}>
        Search a different day or filter →
      </TextLink>
    </Stack>
  );
```

Remove the now-redundant `<Stack gap="xs">{resultsContent()}</Stack>` wrapper that Task 2 put directly in `AccordionPanel` (replaced above), and add `Stack` stays imported (already is). No other imports change.

- [ ] **Step 4: Run the component tests to verify they pass**

Run: `npx vitest run components/StationTimetable.test.tsx`
Expected: PASS.

- [ ] **Step 5: Wire `StationTimetable` into the station page**

In `frontend/app/stations/[crs]/page.tsx`, add the import:

```tsx
import { StationTimetable } from '@/components/StationTimetable';
```

Then render it as the last section of the page, after the "Sample stats by operator" `Stack` block (after line 273's closing `</Stack>`, before the page's own closing `</Stack>` on line 274-275):

```tsx
      <Divider />
      <StationTimetable crs={crs} />
    </Stack>
  );
}
```

(This replaces the existing final two lines, `    </Stack>\n  );\n}`, adding one `<Divider />` and the `<StationTimetable crs={crs} />` call immediately before the page's closing tags.)

- [ ] **Step 6: Run the page test to verify it passes**

Run: `npx vitest run "app/stations/[crs]/page.test.tsx"`
Expected: PASS, including the new "renders the collapsed Scheduled departures section" test.

- [ ] **Step 7: Run the full frontend test suite**

Run (from `frontend/`): `npm test`
Expected: PASS, no regressions in any other file (in particular `IssueList.test.tsx`, which also uses `Accordion`/`keepMounted={false}` and shares no state with this component, and `TrainSearchForm.test.tsx`, whose route this feature reuses read-only).

- [ ] **Step 8: Commit**

```bash
git add frontend/components/StationTimetable.tsx frontend/components/StationTimetable.test.tsx frontend/app/stations/[crs]/page.tsx frontend/app/stations/[crs]/page.test.tsx
git commit -m "feat: wire StationTimetable into the station page with disclaimer and escape hatch"
```

---

## Self-Review

**1. Spec coverage:**

| Spec decision/section | Task |
| --- | --- |
| §0.3 / Decision 1: no new backend route/query param/migration | Global Constraints + every task reuses `/api/trains/search?station=` unchanged; no `crates/` files touched anywhere in this plan |
| §2: exact response envelope/row shape, 404-vs-empty, `now`-forward default, keyset pagination | Task 2 (types, 404/empty split, `now`-forward via omitting `from`/`to`), Task 3 (pagination) |
| Decision 3-4: `Accordion`/`keepMounted={false}` | Task 1 |
| Decision 5: fetch fresh on every expand, no cross-expand cache | Task 3 Step 3 (`handleChange`'s `else` branch clears `results` on collapse) |
| Decision 6: `now`-forward/today/paginated, no fixed window | Task 2 (`loadFirstPage` sends only `station=`), Task 3 (Load more via `nextCursor`/`after`) |
| §3.2: row fields (time, origin, destination, uid link), `?` placeholder, bare CRS codes, no calling points/headcode/operator | Task 2 (row rendering + `?` test), Task 4 (explicit limitation copy) |
| §0.6 / Decision 2: cross-link to `/train/{uid}/{date}` | Task 2 (`TextLink href={/train/${uid}/${displayDate}}`) |
| §3.3: Loading / Error / 404 / empty-200 / rows-with-Load-more states | Task 2 (loading, error, 404, empty, rows), Task 3 (Load more) |
| §3.4: escape hatch to `/trains?station=<crs>` | Task 4 |
| §0.4 / §0.5: inherited limitation (no terminating trains, no headcode/operator) surfaced in UI copy | Task 4's "This list shows only departures..." note |
| §5.2: the 10 enumerated frontend test cases | All 10 covered — collapsed/no-fetch (Task 1), single fetch on expand (Task 2), loading (Task 2), success+link (Task 2), empty-200 (Task 2), 404 (Task 2), error/thrown-fetch (Task 2), Load more mechanics (Task 3), collapse-then-re-expand fresh fetch (Task 3), escape-hatch link (Task 4) |
| §5.1: no new backend tests needed | Stated in Global Constraints; no backend files appear in File Structure or any task |

No gaps found.

**2. Placeholder scan:** No "TBD"/"implement later"/"add appropriate handling" language anywhere in the tasks above; every step carries the literal code or command to run. No task says "similar to Task N" without repeating the actual code.

**3. Type consistency:** `TrainSearchRow`/`TrainSearchResponse`/`Results` are declared once, in Task 2, and every later task (3, 4) references them by the same names and shapes with no renaming. `loadFirstPage`, `handleLoadMore`, `handleChange`, `resultsContent`, `today()` are named consistently across Tasks 2-4. The component's public interface, `StationTimetable({ crs }: { crs: string })`, is unchanged from Task 1 through Task 4 — Task 4 wires it into `page.tsx` with `<StationTimetable crs={crs} />`, matching that exact signature.

---

**Plan complete and saved to `docs/superpowers/plans/2026-09-12-station-timetable.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
