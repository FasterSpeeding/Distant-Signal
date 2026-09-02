# Standalone Ticket Entry Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move the standalone ("no `trackingId`") case of `TicketEntryForm` off the bottom of `/track/mine` (where it renders inline, collapsed by default, below the trains list and the unattached-tickets section) onto its own dedicated `/track/mine/add-ticket` route, replacing the inline form with a second `TextLink` entry point beside the existing "Track a new train" link, pre-expanding the form on the new page via a new optional `defaultOpen` prop, and adding a page-level "Back to My Trains & Tickets" link. `TicketPanel.tsx`'s two `trackingId`-scoped call sites (attaching a ticket to an already-tracked train) are untouched.

**Architecture:**

```
frontend/components/TicketEntryForm.tsx      MODIFIED: new optional
                                              `defaultOpen?: boolean` prop
                                              seeds `open`'s initial
                                              `useState` value (was
                                              hardcoded `false`). No other
                                              change. (Task 1)
        │
frontend/app/track/mine/add-ticket/page.tsx  NEW: Server Component,
                                              getSession() proactive gate;
                                              !authenticated ->
                                              AutoOpenLoginPrompt (reused
                                              from ../AutoOpenLoginPrompt,
                                              NOT LoginLink -- see Status
                                              note); else Stack p="lg"
                                              gap="md", Title "Add a
                                              ticket", Back TextLink, and
                                              <TicketEntryForm label="Add a
                                              ticket" defaultOpen />
                                              (Task 2, depends on Task 1)
        │
frontend/app/track/mine/page.tsx             MODIFIED: inline
                                              <TicketEntryForm /> removed;
                                              entry-point Group gains a
                                              second TextLink, "Add a
                                              ticket" -> /track/mine/add-
                                              ticket (Task 3)
        │
        ▼
Final verification: vitest run + tsc --noEmit + next build (Task 4)
```

Task 1 must land before Task 2 (Task 2's new page passes `defaultOpen`,
which doesn't exist until Task 1 adds it). Task 3 has no compile-time
dependency on Task 2, but should land after it so the new link it adds
points at a route that already exists rather than 404ing between merges,
same reasoning the custom-line-creation-page precedent used for its own
Task 1/Task 2 ordering.

**Tech Stack:** Next.js App Router (Server + Client Components), TypeScript,
Mantine v9 (`Group`, `Stack`, `Title`), Vitest + `@testing-library/react` +
`renderWithMantine` (`frontend/test/render.tsx`) for tests — no new
frontend dependency, no backend change.

**Spec:** `docs/superpowers/specs/2026-09-02-standalone-ticket-entry-page-design.md`
— read in full before starting; this plan carries its Decisions into
concrete tasks, correcting the parts that have gone stale (see Status
note). Cross-references below to "Decision N" refer to that document.

**Status note — real, material drift found and corrected, not a blind
carry-forward of the spec's own citations:** This worktree's branch was 56
commits behind `main` and had none of its own unique commits, so it was
fast-forward-merged onto `main` (`git merge --ff-only main`) before any of
the reads below, exactly as the custom-line-creation-page precedent did
for its own equivalent drift. Two things changed underneath the spec
between when it was written and current `main`, confirmed by reading
every file below fresh after the fast-forward:

1. **A `LoginPromptModal` migration landed** (`docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md`,
   `docs/superpowers/plans/2026-09-02-modal-login-prompt.md`, merged as
   `50dd6f8`). This is the single biggest correction to the spec's own
   "Current relevant state": the spec's Decision 3 says the new page
   should render `<Title>` + `LoginLink` (`components/LoginLink.tsx`) for
   an unauthenticated visitor, "the same login-nudge shape `/track/mine`
   itself uses." **That shape no longer exists.** `/track/mine`'s own
   `trains === null` branch (`app/track/mine/page.tsx:50–58`, read in
   full) now renders `<AutoOpenLoginPrompt>` (`app/track/mine/AutoOpenLoginPrompt.tsx`,
   29 lines, read in full — a small Client Component, colocated in this
   same route directory, that holds `useState(true)` and renders a
   pre-opened, fully-controlled `LoginPromptModal`), not `LoginLink`. This
   plan's Task 2 reuses `AutoOpenLoginPrompt` directly (relative import
   `../AutoOpenLoginPrompt` from the new page's own sibling directory) —
   not `LoginLink` — because the new page's proactive-gate situation is
   now structurally identical to `/track/mine`'s own (a whole-page
   `getSession`/`getMyTrackedTrains`-null prompt with no click event to
   open a modal from, per that component's own doc comment), and reusing
   the just-established sibling-page convention is more consistent than
   reintroducing the older, now-superseded inline-text shape the spec
   assumed. `TicketEntryForm.tsx` itself is also affected but not in a way
   that changes this plan's own work: its reactive 401-on-submit handling
   (spec's Decision 3 "what does not change") now renders
   `<LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>`
   (`components/TicketEntryForm.tsx:378–380`, using the shared
   `useNeedsLogin()` hook from `components/useNeedsLogin.ts`) instead of a
   hand-rolled `needsLogin` boolean + inline `LoginLink` — still exactly
   unmodified by this plan, just confirmed to still be true that no task
   here touches it.
2. **`TicketPanel.tsx` was *not* migrated** by that same landed work — its
   own `getSession()` proactive check and `<LoginLink underline="always">Log
   in to attach a ticket to this journey</LoginLink>` nudge
   (`components/TicketPanel.tsx:38–60`, read in full) are unchanged from
   what the spec's own Current relevant state already described (the
   modal-login-prompt design spec's own Decision 4 explicitly kept
   `TicketPanel` on its Tier-3 whole-panel-replacement shape, not a
   client-control migration). `TicketPanel.tsx`'s two `TicketEntryForm`
   call sites are otherwise unchanged: `trackingId={trackingId}
   label="Add a ticket for this journey"` (zero-tickets branch, now line
   75) and `trackingId={trackingId} label="Add another ticket"` (has-
   tickets branch, now line 100) — both one line lower than the spec's own
   citation (74/98) because of an unrelated `DeleteTicketButton` import
   added above them by other recent work, not a structural change. This
   plan's scoping decision on `TicketPanel` (below) stands unmodified from
   the spec's own conclusion.

Every other file citation in the spec's own "Current relevant state" was
independently re-verified directly against this worktree's current source
post-fast-forward and found accurate (module-relative line-number drift of
a line or two from unrelated nearby edits, cited exactly as re-verified
throughout this plan, not copied blind from the spec): `app/track/mine/page.tsx`
is 269 lines; its entry-point `Group` is at lines 77–80 (`justify=
"space-between" align="baseline"`, `Title` then `TextLink href="/track"`);
`<TicketEntryForm label="Add a ticket" />` is at line 114.
`components/TicketEntryForm.tsx` is 441 lines; the signature
(`{ trackingId, label }: { trackingId?: number; label: string }`) is at
line 55; `const [open, setOpen] = useState(false);` is at line 57; the
`if (!open)` collapsed-button branch is at line 277; the
`trackingId === undefined` standalone-success branch starts at line 201.
`next.config.mjs`'s `/track/tickets` → `/track/mine` redirect (exact-match
`source`, not a `:path*` prefix) is unchanged, confirmed at lines 27–35;
`find frontend/app/track -type f` still returns only `page.tsx`,
`page.test.tsx`, `mine/page.tsx`, `mine/page.test.tsx`, `mine/AutoOpenLoginPrompt.tsx`
— no existing `add-ticket/` directory to collide with. `app/track/page.tsx`
(`TrackTrainForm` in a plain, unconstrained `<Stack p="lg" gap="md">`, no
`Center`/`maw`) is unchanged. `app/layout.tsx`'s nav-bar link back to
`/track/mine` is unchanged in destination, though its own implementation
changed for an unrelated reason (Decision 6 of the modal-login-prompt
spec: `TrackedTrainsNavItem`, `app/layout.tsx:111–115`, is now a plain,
unconditional `<TextLink href="/track/mine">My Trains &amp; Tickets</TextLink>`
with no `getSession()` call of its own — not something this plan touches
or depends on).

**Scoping decision on `TicketPanel.tsx` (re-confirmed, not just
inherited):** the spec's "Explicitly out of scope" section already
concluded `TicketPanel.tsx`'s two `trackingId`-scoped instances stay
completely untouched — no route change, no `defaultOpen`, no gating
change — because attaching a ticket to an *already-tracked, specific*
train is a narrower, different context from `/track/mine/add-ticket`'s "I
don't yet know which train this is for" case. Re-checking that conclusion
against `TicketPanel.tsx`'s current real shape (point 2 above) changes
nothing about it: `TicketPanel` still isn't part of the `LoginPromptModal`
migration either, so there is no new "should this adopt the modal too"
question this plan needs to resolve — it is exactly as untouched by the
surrounding session's other work as it is by this plan. This plan does not
modify `frontend/components/TicketPanel.tsx` at all.

## Global Constraints

- **No backend change, anywhere.** No `crates/api` route, migration, or
  handler is touched — this is a frontend-only route/layout move plus one
  small, additive component prop.
- **`TicketPanel.tsx` and both its `TicketEntryForm` call sites are
  completely untouched.** See the scoping decision above. Do not add
  `defaultOpen` to either call site, do not change `TicketPanel`'s own
  `getSession()`/`LoginLink` gating.
- **The new page's auth gate reuses `AutoOpenLoginPrompt`
  (`frontend/app/track/mine/AutoOpenLoginPrompt.tsx`), imported via the
  relative path `../AutoOpenLoginPrompt` from the new page's own file.**
  Do not import `LoginLink` for this purpose (that was the spec's original,
  now-stale proposal — see Status note) and do not create a second copy of
  `AutoOpenLoginPrompt`; it already takes `children: React.ReactNode` and
  has no dependency on which page renders it.
- **The proactive gate uses `getSession()` directly**
  (`frontend/lib/api.ts`'s `getSession(): Promise<SessionInfo>`), with the
  same `.catch(() => ({ authenticated: false, id: null, email: null, name:
  null }))` defensive fallback `TicketPanel.tsx:38–43` already uses for an
  identical purpose — not a second call to `getMyTrackedTrains()` purely
  for its side-channel 401 signal.
- **`export const revalidate = 0` is required on the new page.** This is
  the opposite conclusion from the custom-line-creation-page precedent's
  own `/lines/new` (which explicitly must *not* have it, because it
  fetches nothing server-side) — this new page calls `getSession()` server-
  side unconditionally, and every page in this app that does a real
  server-side fetch against the `api` service has this line (confirmed:
  `grep -rn "export const revalidate" frontend/app` returns it on
  `app/page.tsx`, `app/track/mine/page.tsx`, `app/lines/page.tsx`, and
  three dynamic-segment pages — `app/lines/new/page.tsx`, which fetches
  nothing, is the one page in that grep's absence list, and it says why in
  its own comment). Omitting it here would risk `next build` trying and
  failing to prerender this route statically against a compose-network-only
  `api` service.
- **Copy is fixed:** the new page's `<Title order={1}>` and the new
  `TextLink`'s text are both `"Add a ticket"` (Decision 2 of the spec —
  reuses the feature's already-established name verbatim, same reasoning
  the custom-line precedent used for "New custom line"). The page-level
  return link reads `"Back to My Trains &amp; Tickets"` and points at
  `/track/mine`. `AutoOpenLoginPrompt`'s body text on the new page is
  `"Log in to add a ticket."` — matches the established sentence shape
  `/track/mine`'s own instance uses (`"Log in to see the trains and
  tickets you're tracking."`, `app/track/mine/page.tsx:55`), ending in a
  period, not the spec's own un-punctuated draft phrasing.
- **Route: `/track/mine/add-ticket`** (Decision 1 of the spec — unaffected
  by the drift above, still the correct call: unambiguous, reuses the
  entry point's own established name, no dynamic-segment collision risk).
- **The entry-point `Group` on `/track/mine` keeps its existing
  `justify="space-between" align="baseline"` unchanged**; the second
  `TextLink` is added inside a new inner `<Group gap="md">` wrapping both
  links, per Decision 2 of the spec.
- **Page layout: an unconstrained `<Stack p="lg" gap="md">`, no `Center`,
  no `maw`**, matching `/track/page.tsx`'s own existing layout for the
  sibling form (`TrackTrainForm`) this page sits beside conceptually
  (Decision 6 of the spec) — `TicketEntryForm` sets no `maw`/width
  constraint of its own for a page-level `maw` to line up with.
- **No change to `TicketEntryForm`'s upload logic, manual-entry fields,
  400/401/422/504/413 handling, `handleSubmit`'s standalone success
  branch, or the flat-vs-scoped `ticketsBasePath` routing.** The only
  change to this file across this whole plan is the new `defaultOpen`
  prop (Task 1).
- **Testing convention: colocated `*.test.tsx`, Vitest +
  `@testing-library/react` + `renderWithMantine`.** A component under test
  that renders `TicketEntryForm` (calls `useRouter()` unconditionally) or
  `AutoOpenLoginPrompt`/`LoginPromptModal` (calls `useLoginHref()`, i.e.
  `usePathname()`/`useSearchParams()`) needs `next/navigation` mocked —
  see `app/track/mine/page.test.tsx:17–21`'s own current mock for the
  exact shape to copy.

---

### Task 1: `TicketEntryForm.tsx` — add `defaultOpen` prop

**Files:**
- Modify: `frontend/components/TicketEntryForm.tsx`
- Modify: `frontend/components/TicketEntryForm.test.tsx`

**Interfaces:**
- Produces: `TicketEntryForm`'s props gain one new optional field:
  `{ trackingId?: number; label: string; defaultOpen?: boolean }`. Every
  existing call site (`TicketPanel.tsx:75,100`, and `app/track/mine/page.tsx:114`
  until Task 3 removes it) omits `defaultOpen` and keeps today's exact
  `useState(false)` behavior, since the prop defaults to `false` when
  omitted.
- **Depends on:** nothing — first, independent task. Task 2 depends on
  this task landing first (its new page passes `defaultOpen`).

- [ ] **Step 1: Write the failing test**

In `frontend/components/TicketEntryForm.test.tsx`, add a new test right
after the existing `'starts collapsed, showing only the entry-point
button'` test (current lines 90–94):

```tsx
  it('defaultOpen renders the manual-entry tab immediately, with no collapsed-button click needed', () => {
    renderWithMantine(<TicketEntryForm label="Add a ticket" defaultOpen />);
    expect(screen.getByLabelText('Operator')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add a ticket' })).not.toBeInTheDocument();
  });
```

This uses no `trackingId` (matching `/track/mine/add-ticket`'s real call
in Task 2) and `label="Add a ticket"` (matching that page's real label),
the mirror image of the existing `'starts collapsed'` test.

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run components/TicketEntryForm.test.tsx -t "defaultOpen renders the manual-entry tab immediately"`
Expected: FAIL — `TicketEntryForm` has no `defaultOpen` prop yet, so
`open` stays `false` and `getByLabelText('Operator')` throws (not found).

- [ ] **Step 3: Add the prop**

In `frontend/components/TicketEntryForm.tsx`, change the signature and
`open` state (currently):

```tsx
export function TicketEntryForm({ trackingId, label }: { trackingId?: number; label: string }) {
  const router = useRouter();
  const [open, setOpen] = useState(false);
```

to:

```tsx
export function TicketEntryForm({
  trackingId,
  label,
  defaultOpen,
}: {
  trackingId?: number;
  label: string;
  defaultOpen?: boolean;
}) {
  const router = useRouter();
  // Every existing call site omits this and keeps today's exact
  // collapsed-by-default behavior; only /track/mine/add-ticket passes
  // `defaultOpen` (a dedicated page whose entire reason for existing is
  // "add a ticket" has no competing content to protect the way
  // /track/mine's own trains list and unattached-tickets section do, so
  // forcing a click through a button reading the identical words the
  // page's own heading already committed to would be friction with
  // nothing behind it).
  const [open, setOpen] = useState(defaultOpen ?? false);
```

No other line in this file changes — `handleSubmit`, the collapsed-button
branch, the `savedStandaloneTicket` branch, and every other piece of state
are untouched. The existing Cancel button (`onClick={() => setOpen(false)}`)
and the standalone success path's "Done for now" button (also
`setOpen(false)`) both still collapse back to the `label` button regardless
of how the component started, exactly as before — `defaultOpen` only
seeds the *first* render's `open` value.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run components/TicketEntryForm.test.tsx`
Expected: PASS on the whole file — the new test passes, and every existing
test (in particular `'starts collapsed, showing only the entry-point
button'`, which omits `defaultOpen` entirely) still passes unchanged,
confirming the default (`false`) is preserved when the prop isn't given.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TicketEntryForm.tsx frontend/components/TicketEntryForm.test.tsx
git commit -m "Add optional defaultOpen prop to TicketEntryForm"
```

---

### Task 2: New `/track/mine/add-ticket` page

**Files:**
- Create: `frontend/app/track/mine/add-ticket/page.tsx`
- Create: `frontend/app/track/mine/add-ticket/page.test.tsx`

**Interfaces:**
- Consumes: `TicketEntryForm` (`frontend/components/TicketEntryForm.tsx`,
  Task 1's `{ trackingId?, label, defaultOpen? }` props) — mounted with no
  `trackingId` (standalone) and `defaultOpen`. `AutoOpenLoginPrompt`
  (`frontend/app/track/mine/AutoOpenLoginPrompt.tsx`, unchanged export,
  `{ children: React.ReactNode }`) via the relative import
  `../AutoOpenLoginPrompt`. `TextLink`
  (`frontend/components/TextLink.tsx`, unchanged, `{ href, children,
  underline? }`). `getSession` (`frontend/lib/api.ts`, unchanged,
  `(): Promise<SessionInfo>` where `SessionInfo = { authenticated:
  boolean; id: string | null; email: string | null; name: string | null
  }`).
- Produces: the `/track/mine/add-ticket` route itself. Nothing downstream
  in this plan imports from this file — Task 3's `TextLink` only needs the
  URL string `/track/mine/add-ticket`, not any export from this module.
- **Depends on:** Task 1 (`defaultOpen` must exist on `TicketEntryForm`
  before this page can pass it).

- [ ] **Step 1: Write the failing test**

Create `frontend/app/track/mine/add-ticket/page.test.tsx`:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import AddTicketPage from './page';
import * as api from '@/lib/api';

vi.mock('@/lib/api');
// AutoOpenLoginPrompt -> LoginPromptModal calls useLoginHref()
// (usePathname()/useSearchParams() under the hood), and the expanded
// TicketEntryForm calls useRouter() -- same workaround
// app/track/mine/page.test.tsx and TicketEntryForm.test.tsx use for the
// same reason (both hooks throw outside a real Next.js App Router tree).
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/track/mine/add-ticket',
  useSearchParams: () => new URLSearchParams(''),
}));

function session(authenticated: boolean) {
  return { authenticated, id: authenticated ? 'user-1' : null, email: null, name: null };
}

describe('AddTicketPage', () => {
  it('not logged in: shows an auto-opened login prompt modal, no form', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(false));
    renderWithMantine(await AddTicketPage());

    expect(screen.getByText('Log in required')).toBeInTheDocument();
    expect(screen.getByText('Log in to add a ticket.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrack%2Fmine%2Fadd-ticket',
    );
    expect(screen.queryByLabelText('Operator')).not.toBeInTheDocument();
  });

  it('logged in: shows the heading, a Back link, and TicketEntryForm expanded with no click needed', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    renderWithMantine(await AddTicketPage());

    expect(screen.getByRole('heading', { name: 'Add a ticket', level: 1 })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Back to My Trains & Tickets' })).toHaveAttribute(
      'href',
      '/track/mine',
    );
    // defaultOpen: the manual-entry fields are visible immediately, no
    // collapsed-button click required.
    expect(screen.getByLabelText('Operator')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Add a ticket' })).not.toBeInTheDocument();
  });

  it('the rendered TicketEntryForm has no trackingId: a save posts to the flat /api/Train/tickets route', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ ticketId: 1 }), { status: 200 })));
    renderWithMantine(await AddTicketPage());

    fireEvent.change(screen.getByLabelText('Operator'), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets', expect.objectContaining({ method: 'POST' }));
    });
    vi.unstubAllGlobals();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run app/track/mine/add-ticket/page.test.tsx`
Expected: FAIL — `frontend/app/track/mine/add-ticket/page.tsx` does not
exist yet, so the import fails to resolve.

- [ ] **Step 3: Write the page**

Create `frontend/app/track/mine/add-ticket/page.tsx`:

```tsx
import { Stack, Title } from '@mantine/core';
import { getSession } from '@/lib/api';
import { AutoOpenLoginPrompt } from '../AutoOpenLoginPrompt';
import { TextLink } from '@/components/TextLink';
import { TicketEntryForm } from '@/components/TicketEntryForm';

// See app/page.tsx's own `revalidate = 0` comment for the rationale: this
// route has no dynamic segment, and it fetches getSession() server-side
// below, so without this Next.js treats it as eligible for static
// generation and tries to prerender it during `next build`, which fails
// since the `api` service only exists on the compose network at runtime.
export const revalidate = 0;

/** `/track/mine/add-ticket` -- the standalone ("no tracked train yet")
 * case of `TicketEntryForm`, moved off the bottom of `/track/mine` onto
 * its own dedicated page per
 * docs/superpowers/specs/2026-09-02-standalone-ticket-entry-page-design.md.
 * `TicketPanel.tsx`'s two trackingId-scoped instances (attaching a ticket
 * to an already-tracked, specific train) are a different, narrower
 * context and are untouched by this page.
 *
 * Proactive `getSession()` gate, same defensive `.catch()` fallback
 * `TicketPanel.tsx` already uses for an identical purpose: `/track/mine`'s
 * own entry-point Group (including the link to this page) only ever
 * renders for a visitor `getMyTrackedTrains()` has already confirmed is
 * logged in, so this page keeps that promise rather than only discovering
 * "actually, you're not logged in" reactively at submit time -- e.g. a
 * session that expired between loading /track/mine and clicking through.
 * `AutoOpenLoginPrompt` is reused as-is from the sibling /track/mine route
 * (relative import) rather than duplicated -- it already takes arbitrary
 * `children` and has no dependency on which page renders it. */
export default async function AddTicketPage() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Add a ticket</Title>
        <AutoOpenLoginPrompt>Log in to add a ticket.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Add a ticket</Title>
      <TextLink href="/track/mine">Back to My Trains &amp; Tickets</TextLink>
      {/* defaultOpen: this page's entire reason for existing is already
          stated by the Title above, so there's no reason to make a
          visitor click a button that repeats it. */}
      <TicketEntryForm label="Add a ticket" defaultOpen />
    </Stack>
  );
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend && npx vitest run app/track/mine/add-ticket/page.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/track/mine/add-ticket/page.tsx frontend/app/track/mine/add-ticket/page.test.tsx
git commit -m "Add dedicated /track/mine/add-ticket page for standalone ticket entry"
```

---

### Task 3: `/track/mine/page.tsx` — remove inline form, add entry-point link

**Files:**
- Modify: `frontend/app/track/mine/page.tsx`
- Modify: `frontend/app/track/mine/page.test.tsx`

**Interfaces:**
- No longer consumes `TicketEntryForm` — that import is removed.
- Produces: nothing new consumed downstream in this plan;
  `/track/mine/add-ticket` is referenced only as a URL string
  (`href="/track/mine/add-ticket"`), not an import.
- **Depends on:** nothing at compile time (this file doesn't import from
  `add-ticket/page.tsx`). Land after Task 2 anyway so the link this task
  adds points at a route that actually exists.

- [ ] **Step 1: Update the test file first**

In `frontend/app/track/mine/page.test.tsx`, replace the existing test
`'renders the "Add a ticket" entry point'` (currently lines 231–236):

```tsx
  it('renders the "Add a ticket" entry point', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('button', { name: 'Add a ticket' })).toBeInTheDocument();
  });
```

with:

```tsx
  it('renders "Track a new train" and "Add a ticket" entry-point links beside the title', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([]);
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByRole('link', { name: 'Track a new train' })).toHaveAttribute('href', '/track');
    expect(screen.getByRole('link', { name: 'Add a ticket' })).toHaveAttribute(
      'href',
      '/track/mine/add-ticket',
    );
  });
```

This changes the assertion from a `button` query (today's entry point is
`TicketEntryForm`'s own collapsed button, rendered in-place) to a `link`
query with an `href` (a `TextLink` to the new page), and adds the
previously-missing assertion for "Track a new train"'s own `href`, a free
addition while this test is already being touched (no existing test in
this file covers it today).

Also update the top-of-file mock comment (currently lines 9–16), which
cites `TicketEntryForm` as a reason `useRouter()` needs mocking — that
reason goes away once this task removes the component from this page:

```tsx
// The not-logged-in prompt is AutoOpenLoginPrompt -> LoginPromptModal,
// which calls useLoginHref() (usePathname()/useSearchParams() under the
// hood) -- same stub AuthStatus.test.tsx and TicketPanel.test.tsx use for
// the same reason. This page also renders AttachTicketAction and
// DeleteTicketButton, both of which call useRouter() from next/navigation
// -- same workaround TicketPanel.test.tsx/TicketEntryForm.test.tsx use for
// the same reason (useRouter() throws outside an app router context).
```

(was: "This page also renders TicketEntryForm (the "Add a ticket" entry
point) and AttachTicketAction, both of which call useRouter()...")

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run app/track/mine/page.test.tsx -t "renders \"Track a new train\" and \"Add a ticket\""`
Expected: FAIL — the current page has no `href="/track/mine/add-ticket"`
link; `getByRole('link', { name: 'Add a ticket' })` throws (not found,
since today's "Add a ticket" is a `button`, not a `link`).

- [ ] **Step 3: Edit the page**

In `frontend/app/track/mine/page.tsx`, remove the now-unused import
(currently line 8):

```tsx
import { TicketEntryForm } from '@/components/TicketEntryForm';
```

Change the entry-point `Group` (currently lines 77–80):

```tsx
      <Group justify="space-between" align="baseline">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <TextLink href="/track">Track a new train</TextLink>
      </Group>
```

to:

```tsx
      <Group justify="space-between" align="baseline">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <Group gap="md">
          <TextLink href="/track">Track a new train</TextLink>
          <TextLink href="/track/mine/add-ticket">Add a ticket</TextLink>
        </Group>
      </Group>
```

And remove the inline form entirely (currently line 114):

```tsx
      <TicketEntryForm label="Add a ticket" />
```

No other line in this file changes — the trains list, the
unattached-tickets section, `TrackedTrainListRow`, `UnattachedTicketRow`,
and `RowStatusBadge` are all untouched.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd frontend && npx vitest run app/track/mine/page.test.tsx`
Expected: PASS on the whole file — in particular, confirm no other
existing test in this file depended on the inline form's presence (none
do; every other test asserts on trains/tickets content, not the ticket-
entry form).

- [ ] **Step 5: Commit**

```bash
git add frontend/app/track/mine/page.tsx frontend/app/track/mine/page.test.tsx
git commit -m "Move standalone ticket entry off /track/mine onto /track/mine/add-ticket, add entry-point link"
```

---

### Task 4: Final verification

**Files:** none (verification only, no edits).

**Interfaces:** none.
**Depends on:** Tasks 1, 2, and 3 all landed.

- [ ] **Step 1: Run the full frontend test suite**

Run: `cd frontend && npx vitest run`
Expected: PASS, with no new failures anywhere in the suite (not just the
files this plan touched). In particular confirm `TicketPanel.test.tsx`
passes unchanged (it renders `TicketEntryForm` with `trackingId` set,
never `defaultOpen`, so Task 1's new prop should have zero effect on it) —
`grep -rn "TicketEntryForm" frontend/components/TicketPanel.tsx` before
running is a useful sanity check that neither call site was accidentally
touched.

- [ ] **Step 2: Type-check the whole frontend project**

Run: `cd frontend && npx tsc --noEmit`
Expected: PASS — catches a missed import (e.g. `TicketEntryForm` still
imported unused in `app/track/mine/page.tsx`) or a props mismatch
`vitest run` alone might not surface.

- [ ] **Step 3: Production build**

Run: `cd frontend && npm run build`
Expected: PASS, including a successful prerender attempt of
`/track/mine/add-ticket` — since this route has `export const revalidate
= 0` (Global Constraints), Next.js should treat it as dynamic and not try
to statically prerender it against the `api` service at build time. If
the build instead fails trying to fetch from `api` for this route, that
means the `revalidate = 0` export was dropped or misplaced — a regression
against this plan's own Global Constraints, not an expected outcome.

- [ ] **Step 4: Manual click-through, if a dev server can be run in this
      environment**

If the execution environment supports running `frontend`'s dev server
against a live `api` backend, logged in as a real user: load `/track/mine`,
confirm both "Track a new train" and "Add a ticket" links render beside
the title and no ticket-entry form appears below the trains/tickets
content; click "Add a ticket", confirm `/track/mine/add-ticket` renders
with the manual-entry form already expanded (no click needed) and a
working "Back to My Trains & Tickets" link; submit a real standalone
ticket and confirm the "Find or track the train this ticket is for" /
"Done for now" next-step `Alert` appears in place (unchanged behavior,
Decision 5 of the spec) rather than any navigation happening. Also load
`/track/mine/add-ticket` while logged out (or via a private/incognito
window) and confirm the `LoginPromptModal` opens automatically with "Log
in to add a ticket." as its body text. If no live `api` backend is
available in this environment, skip this step and rely on Steps 1–3 plus
this plan's own task-level test coverage — do not claim this step passed
without actually running it.

- [ ] **Step 5: No commit for this task**

This task is verification-only; nothing here is staged or committed. If
any step fails, return to the relevant task (1–3), fix it there, and
re-run this task from Step 1.
