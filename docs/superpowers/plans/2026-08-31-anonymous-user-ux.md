# Anonymous User UX Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the two remaining gaps the design spec identified for anonymous/first-time visitors, after its two most concrete findings were already shipped as standalone fixes ahead of this plan (see Status note below): the home page (`/`) is still useless for a logged-out visitor — its "Your Lines"/"Your Stations" sections are built entirely from `getPreferences()`, which is always empty for anyone who has never pinned anything, so an anonymous visitor sees only two lines of placeholder text — and the app has three independent, structurally-drifted implementations of the same "show a control, prompt on the real 401" pattern (`PinToggle`, `TrackTrainForm`, and the now-fixed `CustomLineForm`/`DeleteLineButton`), which the spec recommends consolidating so a fourth feature doesn't reinvent it a fourth way. This plan also formalizes the spec's server-side "ownership-probe" pattern (`TicketPanel`'s three-way branch) in its own code comments, so the next auth-tied feature copies it by name rather than re-deriving it.

**Architecture:**

```
frontend/app/page.tsx                NEW: getSession() branch; anonymous
                                        visitor gets an explainer, a "right
                                        now" widget built from allReports
                                        (already fetched here), and a CTA
                                        row with a proactive login link.
                                        Logged-in behavior unchanged.
frontend/app/page.test.tsx           NEW file (no test coverage exists
                                        for this page today)

frontend/components/useNeedsLogin.ts  NEW: shared client-side 401 handler,
                                        extracted from the pattern
                                        PinToggle/TrackTrainForm/
                                        CustomLineForm/DeleteLineButton
                                        each hand-rolled independently.
frontend/components/LoginPrompt.tsx   NEW: shared `<TextLink
                                        href="/api/auth/login">Log in to
                                        {verb}</TextLink>` rendering, same
                                        shape all four existing call sites
                                        already converged on by hand.
frontend/app/lines/CustomLineForm.tsx   MODIFY: use useNeedsLogin/LoginPrompt
frontend/components/DeleteLineButton.tsx MODIFY: use useNeedsLogin/LoginPrompt

frontend/components/TicketPanel.tsx   MODIFY: doc comment only -- names
                                        its three-way branch "the
                                        ownership-probe pattern" explicitly,
                                        as the copy-this reference for the
                                        next auth-tied feature (e.g. the
                                        not-yet-built tracked-trains-list).
```

No backend changes. No changes to `PinToggle.tsx` or `TrackTrainForm.tsx` themselves (the spec explicitly does not require retrofitting the two already-correct implementations). No changes to `frontend/app/layout.tsx` (see Global Constraints — the nav-bar recommendation this spec makes is conditioned on a feature that doesn't exist yet).

**Tech Stack:** Next.js App Router + TypeScript + Mantine v9, Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`) — frontend-only, same stack as every prior frontend-only plan in this repo. No Rust/backend crate is touched by this plan.

**Spec:** `docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` — read in full before starting; this plan does not restate its research, only carries its decisions into concrete tasks. Cross-references below to "§Section" refer to that document.

**Status note (verified 2026-09-01, before writing this plan):** both concrete fixes the spec's own "Corrections" section flagged as pre-existing bugs (not part of this plan's scope) are confirmed **already shipped on `main`**, independently of this plan:

- **Custom-line ownership gating.** `frontend/components/DeleteLineButton.tsx` and `frontend/app/lines/CustomLineForm.tsx` both now have a `needsLogin` state set specifically on a `401` response, rendering `<TextLink href="/api/auth/login" underline="always">Log in to delete/create a line</TextLink>` — no more raw backend rejection text shown to the user. `frontend/app/lines/[id]/page.tsx` now gates the Edit/Delete buttons on `isCustom && isOwner`, not `isCustom` alone; `isOwner` comes from `CustomLineDetail.isOwner` (`frontend/lib/types.ts`), computed server-side by `crates/api/src/routes/lines.rs`'s `is_owner()` (5 existing unit tests) via `OptionalAuthenticatedUser`. This is also, incidentally, the one backend addition §Policy's Tier 3 section proposed (a boolean ownership flag on `GET /public/lines/{id}`) — already live, not something this plan needs to add.
- **History-page retention honesty.** `frontend/app/lines/[id]/history/page.tsx` computes `retentionShortfallDays` (`frontend/lib/history.ts`) and renders a yellow `Alert` titled "Some of this range isn't available" naming the actual retention window, rather than silently truncating a 30-day preset.

Also verified while scoping this plan: the **tracked-trains-list feature has not landed** — `crates/api/src/routes/train.rs::router()` has no `/Train/mine` route, `frontend/app/track/` has only `page.tsx` (no `mine/` subdirectory), and `frontend/app/layout.tsx` has no `TrackedTrainsNavItem`. The spec's nav-bar recommendation for that entry (§Nav bar: "once it exists... hidden entirely when logged out") is therefore not actionable yet — see Global Constraints. Its own plan, `docs/superpowers/plans/2026-08-31-tracked-trains-list.md`, already specifies exactly this shape in its own Task 5 (Decision 4: "hidden entirely when logged out, not shown-with-a-login-prompt"), so no separate follow-up doc is needed once that plan runs — this plan just confirms it's still pending, not silently done.

## Global Constraints

- **No backend changes anywhere in this plan.** The one backend addition the spec proposed (`isOwner` on `GET /public/lines/{id}`) is already shipped (see Status note). Every remaining recommendation in the spec is explicitly grounded in data the frontend already has in hand — no task in this plan may add a new API route, modify a Rust file, or add a database query.
- **The home page's "right now" widget uses `allReports`, already fetched unconditionally by the existing `getLineStatusForMode(DISPLAYED_MODES_PARAM)` call in `frontend/app/page.tsx` — no new fetch, no new endpoint.** Reuse `severityRank`/`worstStatus` (`frontend/lib/severity.ts`), both already imported in this file. Per §Home page redesign's own "Explicitly not proposed" note, do **not** add a most-disrupted-*stations* widget — there is no bulk disruption endpoint for stations (`getStopPointDisruption(crs)` is per-station only), and building one is out of scope.
- **Logged-in behavior on `/` must not change.** Pinned lines/stations render exactly as they do today, including the existing zero-pins empty-state text — the spec explicitly does not require a separate empty-state redesign for a logged-in visitor with no pins (§Home page redesign, "Optionally... no separate empty-state redesign is proposed for that narrower case"). Only the anonymous branch is new.
- **`getSession()` on `/` must use the same defensive `.catch()` fallback already established in `app/layout.tsx` and `TicketPanel.tsx`** (`.catch(() => ({ authenticated: false, id: null, email: null, name: null }))`) — an auth-status glitch degrades to "treat as anonymous," never a broken homepage.
- **Proactive Tier-2 prompting is scoped to the home page only in this plan.** §Policy's own recommended default (Open Question 1) is "proactive only where session is already in hand" — the home page qualifies because this plan's own Task 1 adds a `getSession()` call there for the anonymous/logged-in branch anyway, so surfacing "Log in to pin your lines and stations" proactively costs nothing extra. Do **not** add a new `getSession()` call to `/lines` or `/stations/[crs]` purely to decorate `PinToggle`'s star proactively — those pages don't fetch session today, and the spec is explicit that adding one solely for this purpose is new cost, not a settled decision. If a future task wants to revisit that tradeoff, it's a separate, later decision, not implied by this plan.
- **The shared `useNeedsLogin`/`LoginPrompt` extraction (Task 2) is not a mandatory refactor of `PinToggle.tsx` or `TrackTrainForm.tsx`.** Per §Reusable pattern: those two are already correct, independent implementations; the spec's explicit ask is for *new* Tier-2 controls and the already-fixed `CustomLineForm`/`DeleteLineButton` to use the shared piece, not for a repo-wide rewrite. Do not modify `PinToggle.tsx` or `TrackTrainForm.tsx` in this plan.
- **No `app/layout.tsx` changes in this plan.** The spec's nav-bar section (§Nav bar) makes no recommendation for the three existing links ("No other nav changes are implied by this audit") and its one substantive recommendation (a session-gated "My Tracked Trains" entry) is explicitly conditioned on the tracked-trains-list feature existing, which it doesn't yet (see Status note). Do not add a placeholder nav item, disabled link, or speculative `TrackedTrainsNavItem` component — when that feature's own plan runs, its own Task 5 already implements the correct shape.
- **No changes to `/lines/[id]/history`** — confirmed Tier 1, no gate needed, already correct (see Status note).
- **Do not touch the already-shipped `isOwner` gating logic** in `crates/api/src/routes/lines.rs`, `frontend/lib/types.ts`, or `frontend/app/lines/[id]/page.tsx` — verifying it is Task 3 of this plan's final step, not modifying it. (A *separate*, larger, not-yet-executed plan — `docs/superpowers/plans/2026-08-31-private-custom-lines-and-tracked-trains.md` — proposes eventually *removing* `isOwner` entirely in favor of full backend enforcement; that plan is out of scope here and this plan must not pre-empt or partially implement any of its steps.)
- **`PinToggle`'s documented last-write-wins race is out of scope** (spec's Open Questions item 4) — orthogonal to auth UX, not touched by any task here.
- **Testing convention:** colocated `*.test.ts`/`*.test.tsx`, Vitest, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`) — matching this repo's existing convention across every prior frontend plan. Every task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures.

---

### Task 1: Home page — anonymous "right now" widget and proactive login CTA

**Files:**
- Modify: `frontend/app/page.tsx`
- Create: `frontend/app/page.test.tsx` (no test file exists for this page today)

**Interfaces:**
- Consumes: `getSession` (`frontend/lib/api.ts`, already used elsewhere), `severityRank`/`worstStatus` (`frontend/lib/severity.ts`, already imported in this file), `MERGED_TFL_LINE_IDS`/`DISPLAYED_MODES_PARAM` (`frontend/lib/modes.ts`, already imported).
- No new exports — this is a page component, not a shared library.

Per §Home page redesign: branch on `session.authenticated`. The anonymous branch replaces today's two empty-state blocks with (1) a one-line explainer, (2) the "right now" widget, (3) a CTA row. The logged-in branch is byte-for-byte what's on `main` today.

- [ ] **Step 1: Add the session fetch**

```tsx
import { getLineStatusForMode, getPreferences, getSession, getStationName, getStopPointDisruption } from '@/lib/api';
```

At the top of `DashboardPage`, alongside the existing `getPreferences()`/`getLineStatusForMode()` calls:

```tsx
export default async function DashboardPage() {
  // Same defensive fallback as app/layout.tsx and TicketPanel.tsx: an
  // auth-status glitch degrades to "treat as anonymous", not a broken
  // homepage. See docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md
  // §Home page redesign.
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  const preferences = await getPreferences();
  const allReports = await getLineStatusForMode(DISPLAYED_MODES_PARAM);
  // ... pinnedLineReports/pinnedStationEntries unchanged below
```

- [ ] **Step 2: Add the "right now" widget's data computation**

Add near the existing `worstSeverityAcrossReports`/`sampleStatsAcrossReports` helpers:

```tsx
/** Anonymous-visitor "right now" widget data (§Home page redesign). Built
 * entirely from `allReports`, already fetched unconditionally by this page
 * for the pinned-lines section -- no new endpoint. Excludes
 * `MERGED_TFL_LINE_IDS` the same way `pinnedLineReports` already does,
 * since those ids are folded into their National Rail counterpart's row
 * everywhere a line list is built directly from reports rather than from
 * `/public/lines` -- counting them separately would double-count the same
 * real-world line. */
function notGoodServiceSummary(reports: LineStatusReport[]) {
  const affected = reports
    .filter((report) => !MERGED_TFL_LINE_IDS.includes(report.id))
    .filter((report) => severityRank(worstStatus(report).statusSeverity) > severityRank(10))
    // Same worst-first-then-alphabetical sort the pinned section already
    // uses (§Home page redesign: "same sort the pinned section already
    // does").
    .sort((a, b) => {
      const rankDiff = severityRank(worstStatus(b).statusSeverity) - severityRank(worstStatus(a).statusSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });
  return { count: affected.length, worst: affected.slice(0, 5) };
}
```

(`severityRank(10)` is `0`, the "good" group's rank — spelled out via the function rather than a bare `0` literal so the comparison reads self-explanatorily against `SEVERITY_TABLE`'s `10: 'Good Service'` entry.)

- [ ] **Step 3: Render the anonymous branch**

Replace the two existing `Stack` blocks' rendering with a branch — logged-in keeps exactly what's there today, anonymous gets the new content:

```tsx
  if (!session.authenticated) {
    const { count, worst } = notGoodServiceSummary(allReports);
    return (
      <Stack p="lg" gap="xl">
        <Stack gap="xs">
          <Title order={1}>Distant Signal</Title>
          <Text c="dimmed">
            Live UK rail line status, train tracking, and Delay Repay support -- pin the lines and
            stations you care about once you&apos;re logged in.
          </Text>
        </Stack>

        <Stack gap="md">
          <Title order={2}>Right now</Title>
          {count === 0 ? (
            <Text>Every line is running a Good Service.</Text>
          ) : (
            <>
              <Text>
                {count} line{count === 1 ? '' : 's'} not at Good Service right now:
              </Text>
              <Stack gap="xs">
                {worst.map((report) => (
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
            </>
          )}
        </Stack>

        <Group gap="lg">
          <TextLink href="/lines">Browse all lines</TextLink>
          <TextLink href="/stations">Look up a station</TextLink>
          {/* Proactive, not reactive -- session is already in hand on this
              page (see this task's own Step 1), so there's no reason to
              wait for a failed pin click the way PinToggle does elsewhere.
              §Policy's Tier-2 "proactive where session is already fetched"
              refinement. */}
          <TextLink href="/api/auth/login" underline="always">
            Log in to pin your lines and stations
          </TextLink>
        </Group>
      </Stack>
    );
  }

  // Logged-in branch: unchanged from today -- pinnedLineReports/
  // pinnedStationEntries computation and rendering exactly as on main.
```

Move the existing `pinnedLineReports`/`pinnedStationEntries` computation (currently right after the `getLineStatusForMode` call) to after this early return, since it's now only needed on the logged-in path — this also avoids computing `pinnedStationEntries` (which does a `Promise.all` of per-station fetches) for an anonymous visitor who has no pinned stations by construction.

**Implementation-time judgment calls, explicitly left open by this plan (the spec does not pin these down further):** exact widget heading text, whether to show all `worst` entries or cap at some other number (5 is a reasonable starting point mirroring similar caps elsewhere in this codebase — e.g. `MAX_PERIODS_PER_INCIDENT = 8`, `MINE_LIST_LIMIT = 100` — not a spec-mandated number), and the exact explainer copy. None of these are load-bearing; keep them consistent with this codebase's existing tone (compare `TicketPanel`'s and `TrackTrainForm`'s own copy for register).

- [ ] **Step 4: Write tests**

Create `frontend/app/page.test.tsx`, mocking `@/lib/api` and awaiting the async Server Component directly (the established technique — see `TicketPanel.test.tsx`, `frontend/app/track/mine/page.test.tsx` precedent in the tracked-trains-list plan):

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import DashboardPage from './page';
import * as api from '@/lib/api';
import type { LineStatusReport } from '@/lib/types';

vi.mock('@/lib/api');

function report(overrides: Partial<LineStatusReport> = {}): LineStatusReport {
  return {
    $type: 'x', id: 'bakerloo', name: 'Bakerloo', modeName: 'tube', operators: [],
    lineStatuses: [{ statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: '' } as never],
    computedAt: '2026-09-01T00:00:00Z',
    ...overrides,
  };
}

describe('DashboardPage', () => {
  it('anonymous, all lines good: shows the no-disruption message, not a raw empty state', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in to pin your lines and stations' })).toHaveAttribute(
      'href', '/api/auth/login',
    );
  });

  it('anonymous, a line disrupted: lists it, worst-first', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'central', name: 'Central', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '' } as never] }),
      report(),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/1 line not at Good Service right now/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Central/ })).toHaveAttribute('href', '/lines/central');
  });

  it('anonymous: merged TfL counterpart ids are excluded from the widget', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([
      report({ id: 'tfl-elizabeth', name: 'Elizabeth line', lineStatuses: [{ statusSeverity: 6, statusSeverityDescription: 'Severe Delays', reason: '' } as never] }),
    ]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByText(/Every line is running a Good Service/)).toBeInTheDocument();
  });

  it('logged in: renders the existing pinned-lines/pinned-stations behavior, not the anonymous branch', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'u1', email: 'a@b.com', name: 'A' });
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: ['central'], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report({ id: 'central', name: 'Central' })]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('heading', { name: 'Your Lines' })).toBeInTheDocument();
    expect(screen.queryByText(/Right now/)).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in to pin your lines and stations' })).not.toBeInTheDocument();
  });

  it('logged in, an auth glitch (getSession rejects): degrades to the anonymous branch, not a crash', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('boom'));
    vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
    vi.mocked(api.getLineStatusForMode).mockResolvedValue([report()]);
    renderWithMantine(await DashboardPage());
    expect(screen.getByRole('link', { name: 'Log in to pin your lines and stations' })).toBeInTheDocument();
  });
});
```

- [ ] **Step 5: Run the test suite**

Run (from `frontend/`): `npm test -- page.test.tsx` — scope further (e.g. `npm test -- app/page.test.tsx`) if this glob also matches a nested `page.test.tsx` elsewhere in the tree.
Expected: all new tests PASS.

- [ ] **Step 6: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS. `npm run build` matters here specifically because `frontend/app/page.tsx` sets `export const revalidate = 0` for exactly the reason its own existing comment gives (prerendering would fail without a live `api` service) — confirm this task hasn't disturbed that.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/page.tsx frontend/app/page.test.tsx
git commit -m "Give anonymous visitors a real home page: right-now widget and proactive login CTA"
```

---

### Task 2: Extract the shared client-side "needsLogin" pattern, retrofit `CustomLineForm`/`DeleteLineButton`

**Files:**
- Create: `frontend/components/useNeedsLogin.ts`
- Create: `frontend/components/useNeedsLogin.test.ts`
- Create: `frontend/components/LoginPrompt.tsx`
- Create: `frontend/components/LoginPrompt.test.tsx`
- Modify: `frontend/app/lines/CustomLineForm.tsx`
- Modify: `frontend/components/DeleteLineButton.tsx`
- Modify (tests, if the behavior-visible output changes): `frontend/app/lines/CustomLineForm.test.tsx`, `frontend/components/DeleteLineButton.test.tsx`

**Interfaces:**
- Produces: `useNeedsLogin()` returning `{ needsLogin, markNeedsLogin, reset }` (or equivalent minimal shape — exact hook API is an implementation-time call, see Step 1), `<LoginPrompt verb="..." />` rendering `<TextLink href="/api/auth/login" underline="always">Log in to {verb}</TextLink>`.
- Consumed by: `CustomLineForm.tsx`, `DeleteLineButton.tsx` (this task). Available for any future Tier-2 control per §Reusable pattern — not retrofitted onto `PinToggle.tsx`/`TrackTrainForm.tsx` in this plan (see Global Constraints).

Per §Reusable pattern: four independent implementations of the same shape already exist (`PinToggle`, `TrackTrainForm`, and the two already-fixed `CustomLineForm`/`DeleteLineButton`) — this is exactly the drift the spec flags as the reason to extract, since the fourth implementation is *already* a slightly different hand-roll of the same three lines (`useState(false)`, check `response.status === 401`, render a `TextLink`) rather than a shared piece. This task converges the two most recently added (and least "load-bearing," lowest-risk-to-touch) implementations onto a shared piece, establishing it as the thing new code reaches for.

- [ ] **Step 1: Write `useNeedsLogin`**

```ts
'use client';

import { useState } from 'react';

/** Shared client-side "show a control to everyone, prompt on the real
 * 401" state (§Policy Tier 2 /
 * docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md
 * §Reusable pattern). `PinToggle.tsx`, `TrackTrainForm.tsx`, and
 * (pre-extraction) `CustomLineForm.tsx`/`DeleteLineButton.tsx` each
 * hand-rolled this same three-line shape independently -- once already
 * slightly differently (CustomLineForm/DeleteLineButton's original,
 * pre-fix version had no needsLogin handling at all). This hook exists
 * purely so the shape can't drift again, not because the previous
 * hand-rolled versions were broken.
 *
 * Deliberately minimal: does not wrap the fetch call itself (each call
 * site's request shape differs too much -- a DELETE, a POST with a JSON
 * body, a PUT against a whole list -- to usefully share that part). Just
 * the state a caller resets at the start of every attempt and sets when a
 * response comes back 401. */
export function useNeedsLogin() {
  const [needsLogin, setNeedsLogin] = useState(false);

  return {
    needsLogin,
    /** Call at the start of every fresh attempt, before the request. */
    reset: () => setNeedsLogin(false),
    /** Call when a response comes back 401. */
    markNeedsLogin: () => setNeedsLogin(true),
  };
}
```

- [ ] **Step 2: Write `LoginPrompt`**

```tsx
import { TextLink } from './TextLink';

/** The shared "you need to log in to do that" affordance next to a Tier-2
 * control (§Reusable pattern of
 * docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md). `verb`
 * is inserted directly after "Log in to " -- pass a bare verb phrase
 * ("pin", "create a line", "delete a line"), not a full sentence. */
export function LoginPrompt({ verb }: { verb: string }) {
  return (
    <TextLink href="/api/auth/login" underline="always">
      Log in to {verb}
    </TextLink>
  );
}
```

- [ ] **Step 3: Retrofit `DeleteLineButton.tsx`**

Replace its own `useState(false)` / `setNeedsLogin` calls with `useNeedsLogin()`, and its inline `<TextLink href="/api/auth/login" ...>Log in to delete a line</TextLink>` with `<LoginPrompt verb="delete a line" />`. The `401` branch becomes `needsLoginState.markNeedsLogin()`; the top of `handleDelete` becomes `needsLoginState.reset()` (alongside the existing `setError(null)`). No behavior change — same DOM output, same accessible text ("Log in to delete a line"), only the state/rendering plumbing moves.

- [ ] **Step 4: Retrofit `CustomLineForm.tsx`**

Same mechanical swap: its own `needsLogin` state and inline `TextLink` become `useNeedsLogin()` and `<LoginPrompt verb="create/edit a line" />` (match whatever verb text is already there today — confirm the exact existing copy before changing it, since this form is shared between create and edit).

- [ ] **Step 5: Confirm existing tests still pass unmodified**

Since Steps 3–4 are a pure internal refactor with no DOM/behavior change, `CustomLineForm.test.tsx` and `DeleteLineButton.test.tsx` (which assert on rendered text/attributes, not implementation details) should require no edits. Run them explicitly to confirm rather than assuming:

Run (from `frontend/`): `npm test -- CustomLineForm.test.tsx DeleteLineButton.test.tsx`
Expected: PASS, unmodified.

If either test file directly imports/inspects internal state (rather than asserting on rendered output), it will need a small update to match — this is a signal the test was over-coupled to implementation, worth a one-line note in the commit if so, not a reason to change this task's approach.

- [ ] **Step 6: Write tests for the two new shared pieces**

`useNeedsLogin.test.ts`: a minimal test component using the hook, asserting `needsLogin` starts `false`, becomes `true` after `markNeedsLogin()`, and returns to `false` after `reset()`.

`LoginPrompt.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginPrompt } from './LoginPrompt';

describe('LoginPrompt', () => {
  it('renders "Log in to {verb}" linking to /api/auth/login', () => {
    renderWithMantine(<LoginPrompt verb="pin" />);
    expect(screen.getByRole('link', { name: 'Log in to pin' })).toHaveAttribute('href', '/api/auth/login');
  });
});
```

- [ ] **Step 7: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/useNeedsLogin.ts frontend/components/useNeedsLogin.test.ts \
        frontend/components/LoginPrompt.tsx frontend/components/LoginPrompt.test.tsx \
        frontend/app/lines/CustomLineForm.tsx frontend/components/DeleteLineButton.tsx
git commit -m "Extract shared useNeedsLogin/LoginPrompt, retrofit CustomLineForm and DeleteLineButton"
```

---

### Task 3: Name the server-side "ownership-probe" pattern explicitly in `TicketPanel.tsx`

**Files:**
- Modify: `frontend/components/TicketPanel.tsx` (doc comment only — no behavior change)

**Interfaces:** none — this task changes no exported signature and no rendered output. Pure documentation, per §Reusable pattern's own recommendation: *"Recommend documenting this explicitly... (in code comments, the way `TicketPanel.tsx` already half does) so the next auth-tied feature copies it on purpose rather than reinventing a fourth branching scheme."*

`TicketPanel.tsx`'s doc comment already describes the three-way branch in detail (unauthenticated → prompt; authenticated-not-owner → nothing; authenticated-owner → content) but doesn't name it. This task adds the name so a future `grep -rn "ownership-probe pattern"` finds this component as the canonical reference.

- [ ] **Step 1: Add the naming sentence to the existing doc comment**

At the top of `TicketPanel.tsx`'s doc comment (which already exists and already explains the three-way branch — see current content), add one sentence identifying it by name, e.g. immediately before or after the existing "This composition... is how this plan resolves that gap" sentence:

```
 * This three-way branch (unauthenticated -> login prompt; authenticated,
 * not-owner -> render nothing; authenticated, owner -> real content) is
 * "the ownership-probe pattern" (see
 * docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md
 * §Reusable pattern) -- the reference shape for any future page/component
 * that needs to show owner-scoped content on an otherwise-public route.
 * Copy this composition, not `getTicketsForTrackedTrain`'s specific
 * signature.
```

Exact wording/placement is not load-bearing — the requirement is that the phrase "ownership-probe pattern" appears in this file's comments, pointing at the spec, so it's discoverable by name.

- [ ] **Step 2: Run the existing test suite unmodified**

Run (from `frontend/`): `npm test -- TicketPanel.test.tsx`
Expected: PASS, unmodified — this task changes no behavior.

- [ ] **Step 3: Commit**

```bash
git add frontend/components/TicketPanel.tsx
git commit -m "Name TicketPanel's three-way branch as the ownership-probe pattern in its doc comment"
```

---

### Task 4: Final policy consistency pass

**Files:** none modified — this task is a verification checklist, not a code change. If it finds a genuine gap, stop and flag it rather than silently patching it as a side effect of this task (a real gap found here is new scope, not something to sneak into a "verification" commit).

Re-walk the spec's own inventory table (§"Current relevant state") against the codebase as it stands after Tasks 1–3, confirming every surface still matches its assigned tier and that nothing regressed:

- [ ] **Step 1:** `/` — anonymous gets the new widget/CTA (Task 1); logged-in unchanged. Confirm by running Task 1's own test suite plus a manual `npm run dev` check of both states if a local `api` service is available (not required if unavailable — the automated tests already cover both branches).
- [ ] **Step 2:** `/lines`, `/stations/[crs]` — reads still fully public (Tier 1, untouched); `PinToggle` still reactive-only (Global Constraints — deliberately not touched); `CustomLineForm`'s create flow still shows `LoginPrompt` on a 401 (Task 2, retrofitted, not removed).
- [ ] **Step 3:** `/lines/[id]`, `/lines/[id]/edit` — Edit/Delete still gated on `isOwner`; `DeleteLineButton` still shows `LoginPrompt` on a 401 (Task 2).
- [ ] **Step 4:** `/lines/[id]/history` — untouched, still Tier 1.
- [ ] **Step 5:** `/track`, `/train/[uid]/[date]`, `/train/by-id/[trackingId]` — untouched; `TrackTrainForm` and `TicketPanel` unmodified in behavior (Task 3 changed only a comment).
- [ ] **Step 6:** Nav bar — still no anonymous-visible change; confirm no `TrackedTrainsNavItem` or similar was accidentally introduced.
- [ ] **Step 7:** Run the complete frontend suite once more as the final gate: `npm test && npm run build` from `frontend/`. Expected: PASS.

No commit for this task unless Step 7's run turns up something Tasks 1–3's own commits didn't already cover (in which case, fix it as part of whichever of those tasks it belongs to, not as a new ad hoc commit here).

---

## Sequencing notes

- Task 1 (home page) has no dependency on Tasks 2/3 and can be built first, last, or in parallel with them — it touches only `frontend/app/page.tsx`/`page.test.tsx`, disjoint from Task 2's files.
- Task 2 (shared pattern extraction) has no dependency on Task 1. It does depend internally on its own Steps 1–2 (create the hook/component) before Steps 3–4 (retrofit the two consumers).
- Task 3 (naming the ownership-probe pattern) is fully independent of Tasks 1–2 — pure documentation on a third, untouched file.
- Task 4 (final pass) should run last, after Tasks 1–3 are all complete, since it re-verifies their combined result.
- Recommended order: 1, 2, 3, 4 — but 1/2/3 could equally run in parallel (e.g. via `superpowers:subagent-driven-development` dispatching all three independent tasks concurrently) since none shares a file with another. This plan lists them sequentially only for reading order.

## Sequencing / scoping judgment calls made while writing this plan

The spec deliberately leaves some things as open questions or as "worth doing, not mandatory" rather than deciding them outright. Per this plan's own brief, these are carried forward rather than silently resolved:

1. **§Policy Open Question 1 (proactive vs. reactive Tier-2 prompting)** is carried forward exactly as the spec leaves it: this plan implements the proactive prompt only on the home page (where session is already fetched for other reasons), and explicitly does **not** extend it to `/lines`/`/stations/[crs]` (Global Constraints). This is the spec's own "recommended default," not a new decision by this plan.
2. **§Reusable pattern's extraction is framed as "worth extracting... not proposed as mandatory."** This plan's judgment call (Task 2) is to do the extraction now, but scope it narrowly to the two most recently touched, lowest-risk call sites (`CustomLineForm`/`DeleteLineButton`) rather than also retrofitting `PinToggle`/`TrackTrainForm` — the spec names exactly this scope as sufficient ("any new Tier-2 control, and the fix to `CustomLineForm.tsx`/`DeleteLineButton.tsx`, should use it"), so this isn't a broader judgment call so much as following the spec's own stated scope precisely.
3. **The nav-bar tracked-trains-list recommendation (§Nav bar)** is carried forward as genuinely not-yet-actionable, per the Status note above — this plan does not attempt a placeholder or partial implementation. This is a sequencing judgment call in the sense that a different plan could have chosen to stub something out now; this plan chose not to, since the referenced feature's own plan already specifies the correct shape and a stub here would just be extra code to keep in sync with a design this plan doesn't own.
4. **§Policy Open Question 2 (the `isOwner` backend addition's exact extractor/edge-case behavior)** turned out to be moot for this plan — it's already shipped (Status note) — so this plan makes no judgment call on it at all, only verifies the shipped behavior in Task 4.
5. **§Open Questions item 4 (`PinToggle`'s last-write-wins race)** is carried forward unresolved, exactly as the spec itself frames it — explicitly out of scope, not touched by any task here.
