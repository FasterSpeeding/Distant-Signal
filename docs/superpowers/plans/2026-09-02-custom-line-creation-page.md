# Custom Line Creation Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move `CustomLineForm`'s create mode off the bottom of `/lines` (where it renders inline, always expanded, under every row of the table) onto its own dedicated `/lines/new` route, replacing the inline form with a `TextLink` entry point beside `/lines`' own `<Title order={1}>`, and remove the now-dead submit-reset workaround `CustomLineForm.tsx`'s create-mode success branch carries specifically because it used to `router.push` back to its own route.

**Architecture:**

```
frontend/app/lines/new/page.tsx        NEW: Center > Stack maw=480, Title
                                        "New custom line", <CustomLineForm
                                        cancelHref="/lines" />  (Task 1)
        │
frontend/app/lines/page.tsx            MODIFIED: inline <CustomLineForm />
                                        section removed; Group(Title +
                                        TextLink "New custom line" ->
                                        /lines/new) replaces the bare Title
                                        (Task 2)
        │
frontend/app/lines/CustomLineForm.tsx  MODIFIED: create-mode's manual
                                        setSubmitting(false)/field-clear
                                        workaround removed -- both modes
                                        now collapse onto one
                                        `router.push(existingLine ? ... :
                                        '/lines')`, since create only ever
                                        renders at /lines/new now, a route
                                        genuinely different from '/lines'
                                        (Task 3)
        │
frontend/app/lines/CustomLineForm.test.tsx
                                        MODIFIED: the test locking in the
                                        removed workaround is deleted; the
                                        create-mode 401 test's pathname
                                        mock/assertion move from '/lines'
                                        to '/lines/new' (Task 3)
        │
        ▼
Final verification: vitest run + tsc --noEmit + next build (Task 4)
```

Tasks 1-3 touch disjoint files and can be dispatched in parallel; Task 4
must run last, after all three land.

**Tech Stack:** Next.js App Router (Server + Client Components), TypeScript,
Mantine v9 (`Group`, `Stack`, `Center`, `Title`), Vitest +
`@testing-library/react` + `renderWithMantine` (`frontend/test/render.tsx`)
for tests — no new frontend dependency, no backend change.

**Spec:** `docs/superpowers/specs/2026-09-02-custom-line-creation-page-design.md`
— read in full before starting; this plan does not restate its research,
only carries its Decisions into concrete tasks. Cross-references below to
"Decision N" refer to that document.

**Status note — every citation below re-confirmed directly against this
worktree's current source (not trusted blind from the spec), after
fast-forward-merging `main` (this branch was 15 commits behind and did not
yet have the spec file):** `frontend/app/lines/page.tsx` (30 lines, read in
full) matches the spec's own citation exactly — `<Title order={1}>All
Lines</Title>` at line 20, the `<Stack gap="md"><Title order={2}>New
Custom Line</Title><CustomLineForm /></Stack>` section at lines 24-27.
`frontend/app/lines/[id]/edit/page.tsx` (35 lines, read in full) matches
the spec's citation exactly — the `<Center><Stack p="lg" gap="md"
maw={480} w="100%">` chrome at lines 28-34, `<CustomLineForm
existingLine={line} cancelHref={...} />` at line 31, no `export const
revalidate = 0` anywhere in the file (confirmed relevant below).
`frontend/app/track/mine/page.tsx`'s entry-point `Group` is at lines
76-79 exactly as cited (`<Group justify="space-between"
align="baseline"><Title order={1}>My Trains &amp; Tickets</Title><TextLink
href="/track">Track a new train</TextLink></Group>`).
`frontend/app/lines/CustomLineForm.tsx` (276 lines, read in full):
`handleSubmit` spans lines 95-162; the edit-mode branch is lines 129-133;
the create-mode branch (the workaround) is lines 134-157, with its stale
comment at lines 135-148 and the six manual-reset calls at lines 149-155,
exactly matching the spec's citations.
`frontend/app/lines/CustomLineForm.test.tsx` (314 lines, read in full):
the create-mode 401 test (`'a 401 on create shows a login prompt worded
for creating, not editing'`) spans lines 242-262, with
`mockUsePathname.mockReturnValue('/lines')` at line 250 and the
`return_to=%2Flines` assertion at line 261; the workaround-locking test
(`'a successful create resets the submit button out of its loading state
and clears the form'`) plus its explanatory comment spans lines 264-296 —
all exactly matching the spec's citations, nothing has moved.
**New finding this plan's own verification pass surfaced, not called out
by the spec:** `frontend/app/track/page.tsx` (the destination
`/track/mine`'s own "Track a new train" link points at) is a real,
already-shipped precedent for a *static* route with *no* server-side data
fetch and *no* `export const revalidate = 0` — confirmed by grepping
`export const revalidate` across `frontend/app`, which finds it on every
page that fetches data server-side (`app/lines/page.tsx`, `app/page.tsx`,
`app/track/mine/page.tsx`, `app/incidents/[id]/page.tsx`,
`app/lines/[id]/page.tsx`, `app/lines/[id]/history/page.tsx`) but not on
`app/track/page.tsx`. Task 1's new `/lines/new/page.tsx` is the same
shape as `app/track/page.tsx` (static route, no dynamic segment, no
server-side fetch — the spec's own Error handling section confirms this:
"it takes no dynamic segment and fetches nothing server-side before
rendering") — so it must **not** get `export const revalidate = 0`
either. `frontend/lib/types.ts` already declares `LineStatus.sampleAvailability`
as a required field (confirmed: `AllLinesTable.test.tsx`'s own fixtures
already carry `sampleAvailability: { state: 'no-coverage' }` on every
`LineStatus` literal) — an unrelated, already-landed change from a
different plan, noted here only because any new `LineStatus`/`LineStatusReport`
test fixture this plan's own test tasks add must include it too, or
`tsc --noEmit`/`next build` will fail the type check.

## Global Constraints

- **No backend change, anywhere.** `crates/api/src/routes/lines.rs`,
  `custom_lines::slugify`, and the `POST`/`PUT /api/lines` endpoints are
  untouched — Decision 1's collision analysis (a custom line id is always
  `custom-<slug>`, so `"new"` can never collide) is a read-only finding,
  not something this plan implements.
- **`CustomLineForm`'s edit mode, its `existingLine`/`cancelHref` props,
  and `/lines/[id]/edit/page.tsx` are completely untouched.** They are the
  precedent this plan copies, not something being redesigned (design's
  "Explicitly out of scope").
- **No new component and no new prop.** `CustomLineForm` already has
  everything the create page needs (`cancelHref?: string`, existing
  `useNeedsLogin`/`LoginLink` 401 handling) — Task 1 only changes *where*
  it's mounted, never its own code, except for Task 3's unrelated
  cleanup inside `handleSubmit`.
- **No proactive session/auth gating anywhere in this plan.** Decision 3:
  the `TextLink` entry point on `/lines` renders unconditionally for every
  visitor, logged in or not — do not add a `getSession()` call to
  `frontend/app/lines/page.tsx` or `frontend/app/lines/new/page.tsx` to
  decorate it. `CustomLineForm`'s existing reactive `useNeedsLogin`/`LoginLink`
  401 handling on submit is the only auth surface this feature needs, and
  it is already implemented — nothing in this plan touches it except
  Task 3's unrelated navigation cleanup.
- **Copy is fixed by Decision 2: "New custom line"**, sentence case, used
  verbatim both as the `TextLink`'s text on `/lines` and as the
  `<Title order={1}>` on `/lines/new`. Do not substitute a verb-phrase
  variant (e.g. "Create a new line") — the design considered and rejected
  that in favor of reusing this app's existing name for the feature.
- **The entry-point `Group` uses exactly `justify="space-between"
  align="baseline"`**, matching `/track/mine/page.tsx:76` verbatim — not a
  new layout choice to design.
- **`/lines/new/page.tsx` must NOT have `export const revalidate = 0`.**
  Unlike `/lines/page.tsx` (which fetches four things server-side and
  needs it to avoid a failed prerender against the `api` service at build
  time — see that file's own precedent, `app/track/mine/page.tsx:13-17`'s
  comment), the new page fetches nothing server-side, matching
  `app/track/page.tsx`'s existing, already-shipped shape (Status note
  above). Getting this wrong doesn't break `next build` today (an
  over-cautious `revalidate = 0` is harmless) but is a real deviation from
  this app's established convention that a reviewer should flag.
- **Testing convention: every route/component with real behavior gets a
  colocated `*.test.tsx`, Vitest + `@testing-library/react` +
  `renderWithMantine`.** This repo already has 7 `page.test.tsx` files,
  including two siblings in this exact `app/lines/` tree
  (`app/lines/[id]/page.test.tsx`, `app/lines/[id]/history/page.test.tsx`)
  — `/lines/page.tsx` and `/lines/[id]/edit/page.tsx` lacking one today is
  a pre-existing gap, not the norm this plan should continue. Task 1 and
  Task 2 each add a new colocated `page.test.tsx`; `/lines/[id]/edit`
  stays out of scope (Explicitly out of scope, above) so no test is added
  for it here.
- **A component under test that calls `useRouter()` (`CustomLineForm`
  does, unconditionally, at its top level) throws outside a real Next.js
  App Router tree.** Every test file that mounts `CustomLineForm` directly
  or indirectly (via a page that renders it, or via `AllLinesTable`'s
  embedded `PinToggle`) must mock `next/navigation`'s `useRouter` — see
  `CustomLineForm.test.tsx`'s and `AllLinesTable.test.tsx`'s own top-of-file
  mocks for the exact shape to copy per call site.
- **Parallelizable tasks:** Tasks 1, 2, and 3 touch disjoint files and can
  be dispatched to separate subagents in parallel. Task 4 (final
  verification) must run last, after all three land — it is the only task
  that needs the full, combined tree to be internally consistent (e.g.
  `/lines/page.tsx` no longer importing `CustomLineForm` at all, `/lines/new/page.tsx`
  existing as a real route).

---

### Task 1: New `/lines/new` page

**Files:**
- Create: `frontend/app/lines/new/page.tsx`
- Create: `frontend/app/lines/new/page.test.tsx`

**Interfaces:**
- Consumes: `CustomLineForm` (`frontend/app/lines/CustomLineForm.tsx`,
  unchanged export, `{ existingLine?, cancelHref? }` props) — mounted with
  no `existingLine` (create mode) and `cancelHref="/lines"`.
- Produces: the `/lines/new` route itself. Nothing downstream in this plan
  imports from this file — Task 2's `TextLink` only needs the URL string
  `/lines/new`, not any export from this module.
- **Depends on:** nothing — this is the first, independent task.

- [ ] **Step 1: Write the failing test**

Create `frontend/app/lines/new/page.test.tsx`:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import NewCustomLinePage from './page';

// CustomLineForm calls useRouter() from next/navigation unconditionally
// at the top of its component body -- throws outside a real Next.js App
// Router tree. Same workaround CustomLineForm.test.tsx's own top-of-file
// mock uses; usePathname/useSearchParams are included too since a real
// 401 on this page would mount LoginLink, which needs them, even though
// no test below exercises that path (CustomLineForm.test.tsx already
// covers 401 handling directly).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/lines/new',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('NewCustomLinePage', () => {
  it('renders the "New custom line" heading and mounts CustomLineForm in create mode with cancelHref="/lines"', () => {
    renderWithMantine(<NewCustomLinePage />);

    expect(screen.getByRole('heading', { name: 'New custom line', level: 1 })).toBeInTheDocument();
    // Create mode, not edit: the Name field is present and the submit
    // button reads "Create line" (CustomLineForm's own create-vs-edit
    // label, unchanged).
    expect(screen.getByLabelText('Name')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Create line' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Cancel' })).toHaveAttribute('href', '/lines');
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run app/lines/new/page.test.tsx`
Expected: FAIL — `frontend/app/lines/new/page.tsx` does not exist yet, so
the import in the test file fails to resolve.

- [ ] **Step 3: Write the page**

Create `frontend/app/lines/new/page.tsx`:

```tsx
import { Center, Stack, Title } from '@mantine/core';
import { CustomLineForm } from '../CustomLineForm';

// No `export const revalidate = 0` -- unlike `/lines/page.tsx` (which
// fetches four things server-side and needs it to avoid `next build`
// trying and failing to prerender against the `api` service, which only
// exists on the compose network at runtime -- see that page's own
// comment), this page fetches nothing server-side. Matches
// `app/track/page.tsx`'s existing shape: a static route with no dynamic
// segment and no server-side data fetch needs nothing here.
export default function NewCustomLinePage() {
  return (
    // `Center` plus a `maw` matching CustomLineForm's own `maw={480}`
    // keeps this chrome's width in lockstep with the form's, so the
    // heading lines up with the form's edges -- same reasoning as
    // `[id]/edit/page.tsx`'s own comment, which this page copies almost
    // verbatim (see that file for the precedent).
    <Center>
      <Stack p="lg" gap="md" maw={480} w="100%">
        <Title order={1}>New custom line</Title>
        <CustomLineForm cancelHref="/lines" />
      </Stack>
    </Center>
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend && npx vitest run app/lines/new/page.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/new/page.tsx frontend/app/lines/new/page.test.tsx
git commit -m "Add dedicated /lines/new page for custom line creation"
```

---

### Task 2: `/lines/page.tsx` — remove inline form, add entry-point link

**Files:**
- Modify: `frontend/app/lines/page.tsx`
- Create: `frontend/app/lines/page.test.tsx`

**Interfaces:**
- Consumes: `TextLink` (`frontend/components/TextLink.tsx`, unchanged
  export, `{ href, children, underline?, target?, rel? }` props) — new
  import this task adds. No longer consumes `CustomLineForm` — that import
  is removed.
- Produces: nothing new consumed downstream in this plan; `/lines/new` is
  referenced only as a URL string (`href="/lines/new"`), not an import.
- **Depends on:** nothing at compile time (this task's own file doesn't
  import from `/lines/new/page.tsx`). Land after Task 1 anyway so the
  link this task adds points at a route that actually exists, rather than
  leaving `/lines/new` a dead link on `main` for however long between
  merges.

- [ ] **Step 1: Write the failing test**

Create `frontend/app/lines/page.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AllLinesPage from './page';
import * as api from '@/lib/api';
import type { LineStatusReport, LineSummary, Suggestion, Preferences } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getAllLines: vi.fn(),
    getPreferences: vi.fn(),
    getLineStatusForMode: vi.fn(),
    getAllTocs: vi.fn(),
  };
});
// AllLinesTable renders a PinToggle per row, which calls useRouter() from
// next/navigation -- same workaround AllLinesTable.test.tsx itself uses
// (that hook throws outside a real Next.js App Router tree).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
}));

const lines: LineSummary[] = [
  { id: 'wcml', name: 'West Coast Main Line', category: 'Long Distance', operators: ['VT'], source: 'catalogue' },
];
const preferences: Preferences = { pinnedLines: [] };
const reports: LineStatusReport[] = [];
const tocs: Suggestion[] = [{ code: 'VT', name: 'Avanti West Coast' }];

async function renderPage() {
  return renderWithMantine(await AllLinesPage());
}

describe('AllLinesPage', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    vi.mocked(api.getAllLines).mockResolvedValue(lines);
    vi.mocked(api.getPreferences).mockResolvedValue(preferences);
    vi.mocked(api.getLineStatusForMode).mockResolvedValue(reports);
    vi.mocked(api.getAllTocs).mockResolvedValue(tocs);
  });

  it('renders a "New custom line" link pointing at /lines/new, sharing a row with the page title', async () => {
    await renderPage();

    const link = screen.getByRole('link', { name: 'New custom line' });
    expect(link).toHaveAttribute('href', '/lines/new');
    const heading = screen.getByRole('heading', { name: 'All Lines', level: 1 });
    // Same "shared parent row" assertion style CustomLineForm.test.tsx
    // already uses for its Cancel/submit pairing.
    expect(link.parentElement).toBe(heading.parentElement);
  });

  it('no longer renders CustomLineForm inline on this page', async () => {
    await renderPage();

    expect(screen.queryByLabelText('Name')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Create line' })).not.toBeInTheDocument();
    expect(screen.queryByRole('heading', { name: 'New Custom Line' })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run app/lines/page.test.tsx`
Expected: FAIL — the current `page.tsx` has no "New custom line" link (so
the first test's `getByRole('link', { name: 'New custom line' })` throws),
and still renders the inline form (so the second test's
`queryByLabelText('Name')`/`queryByRole('button', { name: 'Create line' })`
assertions fail).

- [ ] **Step 3: Edit the page**

In `frontend/app/lines/page.tsx`, replace the full current contents
(30 lines) with:

```tsx
import { Group, Stack, Title } from '@mantine/core';
import { getAllLines, getAllTocs, getLineStatusForMode, getPreferences } from '@/lib/api';
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
import { TextLink } from '@/components/TextLink';
import { AllLinesTable } from './AllLinesTable';

export const revalidate = 0;

export default async function AllLinesPage() {
  const [lines, preferences, reports, tocs] = await Promise.all([
    getAllLines(),
    getPreferences(),
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
    getAllTocs(),
  ]);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Group justify="space-between" align="baseline">
          <Title order={1}>All Lines</Title>
          <TextLink href="/lines/new">New custom line</TextLink>
        </Group>
        <AllLinesTable lines={lines} reports={reports} pinnedLineIds={preferences.pinnedLines} tocs={tocs} />
      </Stack>
    </Stack>
  );
}
```

This removes the `CustomLineForm` import entirely (dead once the inline
section is gone) and the whole second `<Stack gap="md">...</Stack>`
section (the "New Custom Line" `order={2}` heading + inline form).
`AllLinesTable` and its own props are otherwise unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run app/lines/page.test.tsx app/lines/AllLinesTable.test.tsx`
Expected: PASS on both files — `AllLinesTable.test.tsx` mounts
`AllLinesTable` directly and is unaffected by this page-level change, run
here only as a quick regression check on the component this page still
renders.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/page.tsx frontend/app/lines/page.test.tsx
git commit -m "Move custom-line creation off /lines onto /lines/new, add entry-point link"
```

---

### Task 3: `CustomLineForm.tsx` — remove the dead submit-reset workaround

**Files:**
- Modify: `frontend/app/lines/CustomLineForm.tsx`
- Modify: `frontend/app/lines/CustomLineForm.test.tsx`

**Interfaces:**
- Produces: no interface change — `CustomLineForm`'s props
  (`{ existingLine?, cancelHref? }`) and exported name are unchanged; only
  `handleSubmit`'s internal navigation logic changes.
- **Depends on:** nothing at compile time (this file does not import
  `/lines/new/page.tsx` or `/lines/page.tsx`). Land after Task 1/2 anyway
  so the removed comment's replacement ("both modes navigate cross-route")
  is describing a true statement about the shipped app at the point this
  commit lands, not a future state.

- [ ] **Step 1: Update the test file first**

In `frontend/app/lines/CustomLineForm.test.tsx`:

1. Delete the test `'a successful create resets the submit button out of
   its loading state and clears the form'` together with its explanatory
   comment — currently lines 264-296 (the comment block starting `//
   Reproduces the "Create line gets stuck loading" bug:` through the
   test's closing `});`). This is Decision 4's premise-invalidated test:
   its own comment states the scenario is specifically "creating a line
   navigates back to `/lines`, the very route this form is already
   rendered on" — no longer true once create mode only ever renders at
   `/lines/new` (Task 1/2). Its `useRouter().push` mock is a no-op
   `vi.fn()` (top of file) that cannot distinguish "same route, no
   remount" from "different route, remounts" either way, so there is no
   way to keep this test meaningfully asserting anything once the
   scenario it names stops existing in the app.

2. In the test `'a 401 on create shows a login prompt worded for
   creating, not editing'` (currently lines 242-262), update the pathname
   mock to match the form's real new location:

```tsx
    mockUsePathname.mockReturnValue('/lines/new');
```

   (was `mockUsePathname.mockReturnValue('/lines');`, line 250)

   And update the corresponding assertion:

```tsx
    expect(loginLink).toHaveAttribute('href', '/api/auth/login?return_to=%2Flines%2Fnew');
```

   (was `'/api/auth/login?return_to=%2Flines'`, line 261)

   No other test in this file references the form's create-mode route.

- [ ] **Step 2: Run the tests to see the current state**

Run: `cd frontend && npx vitest run app/lines/CustomLineForm.test.tsx`
Expected: PASS — this step only edited test assertions/mocks, not
production code, so nothing here should fail yet; the pathname
mock/assertion pair was updated together, and the deleted test simply
stops existing. This step exists to confirm the test-file edit alone
didn't break anything unrelated before Step 3 touches production code.

- [ ] **Step 3: Edit `CustomLineForm.tsx`'s `handleSubmit`**

Replace the current `if (existingLine) { ... } else { ... }` block
(currently lines 129-157) with:

```tsx
      // Both create and edit now navigate to a route different from
      // wherever this form is rendered (`/lines/new` for create,
      // `/lines/{id}/edit` for edit) -- App Router remounts this
      // component on the way there either way, so `submitting` and every
      // field reset for free, with no manual work needed here.
      router.push(existingLine ? `/lines/${existingLine.id}` : '/lines');
```

This removes the six manual-reset calls (`setSubmitting(false)`,
`setName('')`, `setOperators([])`, `setStations([])`,
`setHeadcodePrefixes([])`, `setDestinationCrsFilter([])`,
`setAdvancedOpen(false)`) and the stale comment explaining them, which
would otherwise assert a false fact ("navigates back to `/lines` — the
same route this form already lives on") once create mode's only
remaining call site is `/lines/new`, a genuinely different route (Decision
4). Nothing else in `CustomLineForm.tsx` changes — `existingLine`'s own
edit-mode reasoning, `cancelHref`'s rendering, and every other branch of
`handleSubmit` are untouched.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run app/lines/CustomLineForm.test.tsx`
Expected: PASS — in particular, `'a 401 on save shows a login prompt
instead of the raw backend error text'` (edit mode) and `'a non-401
failure shows the raw backend error text, not a login prompt'` (edit
mode, currently lines 300-313) must still pass unchanged, confirming the
edit-mode branch's behavior is untouched by this collapse.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/CustomLineForm.tsx frontend/app/lines/CustomLineForm.test.tsx
git commit -m "Remove CustomLineForm's dead create-mode submit-reset workaround"
```

---

### Task 4: Final verification

**Files:** none (verification only, no edits).

**Interfaces:** none.
**Depends on:** Tasks 1, 2, and 3 all landed.

- [ ] **Step 1: Run the full frontend test suite**

Run: `cd frontend && npx vitest run`
Expected: PASS, with no new failures anywhere in the suite (not just the
files this plan touched) — in particular confirm no other test file
asserted on the removed inline form (`grep -rn "New Custom Line" frontend/app frontend/components`
before running is a useful sanity check; the only real hit should be the
copy string itself inside `CustomLineForm.tsx`'s create-mode label logic,
if any, or nothing at all).

- [ ] **Step 2: Type-check the whole frontend project**

Run: `cd frontend && npx tsc --noEmit`
Expected: PASS — this is the step that would catch a missed import (e.g.
`CustomLineForm` still imported somewhere unused) or a props mismatch
`vitest run` alone might not surface, per this repo's own established
practice of running both.

- [ ] **Step 3: Production build**

Run: `cd frontend && npm run build`
Expected: PASS, including a successful prerender of `/lines/new` as a
fully static route (no `revalidate = 0` needed, per this plan's Global
Constraints — if the build instead tries and fails to fetch from the
`api` service for this route, that means Task 1's page accidentally
introduced a server-side fetch, which is a regression against Decision 1
and the design's Error handling section, both of which say this route
fetches nothing).

- [ ] **Step 4: Manual click-through, if a dev server can be run in this
      environment**

If the execution environment supports running `frontend`'s dev server
(directly, or via this repo's own `run` skill/tooling) against a live
`api` backend: load `/lines`, confirm the "New custom line" link renders
beside the "All Lines" heading and no form appears below the table; click
it, confirm `/lines/new` renders the form with a working "Cancel" link
back to `/lines`; submit a real creation and confirm the browser lands
back on `/lines` with the new line visible in the table (this is the
scenario Task 3 changed — an actual cross-route remount, not the old
same-route non-remount). If no live `api` backend is available in this
environment, skip this step and rely on Steps 1-3 plus this plan's own
task-level test coverage — do not claim this step passed without actually
running it.

- [ ] **Step 5: No commit for this task**

This task is verification-only; nothing here is staged or committed. If
any step fails, return to the relevant task (1-3), fix it there, and
re-run this task from Step 1.
