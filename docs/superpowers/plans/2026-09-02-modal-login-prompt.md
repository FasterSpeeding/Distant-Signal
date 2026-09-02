# Modal Login Prompt Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the inline `LoginLink` text prompt with a new, shared `LoginPromptModal` component at the five call sites the design names (`PinToggle`, `CustomLineForm`, `TrackTrainForm`, `TicketEntryForm`, and `/track/mine`'s page-level 401 prompt), extract a `useLoginHref()` hook so the `return_to`-building logic isn't duplicated between `LoginLink` and the new modal, fold the three hand-rolled `needsLogin` `useState`s onto the shared `useNeedsLogin()` hook while their files are already being touched, and reclassify the "My Trains & Tickets" nav item from Tier 3 (hidden when logged out) to always-visible-with-a-modal-on-the-real-401 — while leaving the other seven `LoginLink`/`useNeedsLogin` call sites (`DeleteLineButton`, `DeleteTrainButton`, `TicketPanel`, both train detail pages, the home page CTA, `AuthStatus`) completely untouched, per the design's own per-site reasoning.

**Architecture:**

```
frontend/components/useLoginHref.ts        NEW — return_to href builder
  │                                         (Task 1; LoginLink.tsx refactored
  │                                          onto it, behavior unchanged)
  ▼
frontend/components/LoginPromptModal.tsx   NEW — thin controlled Modal wrapper
  (Task 2)                                  (opened/onClose/children only;
  │                                          useNeedsLogin() itself untouched)
  ├──────────────┬──────────────┬──────────────────┬─────────────────┐
  ▼              ▼              ▼                  ▼                 ▼
PinToggle.tsx  CustomLineForm  TrackTrainForm  TicketEntryForm   track/mine/
(Task 3,       .tsx (Task 4,   .tsx (Task 5,   .tsx (Task 6,     page.tsx +
 + useNeedsLogin (already uses  + useNeedsLogin  + useNeedsLogin  NEW AutoOpen-
 adoption)      useNeedsLogin,  adoption)        adoption)        LoginPrompt.tsx
                render swap     (Task 8, via a new
                only)                            sibling Client
                                                  Component file)

frontend/app/layout.tsx ──────────────────────────────────────────────
  TrackedTrainsNavItem: drops getSession()/null-branch/Suspense,
  becomes an unconditional <TextLink> (Task 7 — independent of every
  other task, touches no shared file)

Task 9: final verification (vitest + tsc + next build), all tasks landed
```

**Tech Stack:** Next.js App Router (Server + Client Components) + TypeScript + Mantine v9.5.2 (`Modal`, `Button`, both already used identically by `DeleteLineButton.tsx`/`DeleteTrainButton.tsx`) — no new npm package, no new Cargo/Rust involvement (this is a frontend-only plan). Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`) for every test.

**Spec:** `docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md` — read in full before starting; this plan does not restate its research, only carries its Decisions into concrete tasks. Cross-references below to "Decision N" refer to that document.

**IMPORTANT — branch from `main`, not from this worktree:** This plan was written from a worktree (`worktree-agent-a2cca8ac08ea935b8`) that forked from `main` well before the spec above was merged (`main`'s `2cfed49`) and is now dozens of commits behind — `git log HEAD..main --oneline` currently shows 25+ commits this worktree lacks, including a real, substantive rewrite of `frontend/app/lines/CustomLineForm.tsx`'s submit-success branch (the old `existingLine ? ... : ...` two-branch reset dance is gone; both branches now just `router.push(...)`, since `/lines/new` no longer renders this form inline). **Every citation below was independently verified by reading `main`'s current content directly (`git show main:<path>`), not this worktree's own files, and not trusted blind from the spec** (which itself was written against `main` too, and several of its own line-number citations — e.g. `CustomLineForm.tsx:244–248` — are already off by ~20 lines against current `main`; corrected citations are used throughout below). **Do not start Task 1 from this worktree's current `HEAD`** — branch/rebase onto `main` first, or this plan's line numbers and diffs will not apply cleanly.

## Global Constraints

- **`useNeedsLogin()`'s public shape does not change, anywhere in this plan.** `{ needsLogin, reset, markNeedsLogin }` (`frontend/components/useNeedsLogin.ts:21-31`) stays exactly as-is — Decision 1. No task modifies `useNeedsLogin.ts` or `useNeedsLogin.test.ts`. Confirmed unchanged reference: `useNeedsLogin.test.ts:1-20`'s three assertions.
- **Seven call sites are explicitly out of scope — no task's file list may include them:** `frontend/components/DeleteLineButton.tsx` + `.test.tsx`, `frontend/components/DeleteTrainButton.tsx` + `.test.tsx` (Decision 4: both already render their `needsLogin` prompt *inside* their own existing confirm `Modal` — nesting a second `Modal` is rejected complexity), `frontend/components/TicketPanel.tsx`, `frontend/app/train/by-id/[trackingId]/page.tsx`, `frontend/app/train/[uid]/[date]/page.tsx` (all three genuine Tier-3 whole-content-replace prompts), `frontend/app/page.tsx` (proactive CTA, not a reaction to a failed action), `frontend/components/AuthStatus.tsx` (static nav "Log in", not tied to any attempted action).
- **`LoginPromptModal`'s title is the fixed string `"Log in required"` everywhere it's used — never a per-call-site prop.** Decision 2: only `children` (body prose) varies per call site; a distinct `title` prop was considered and rejected as redundant.
- **Every `LoginPromptModal` login button uses the `<Link href={...} style={{ textDecoration: 'none' }}><Button>Log in</Button></Link>` wrapping pattern — never Mantine's `component={Link}` polymorphic prop.** Decision 3; `CustomLineForm.tsx`'s existing Cancel button (`app/lines/CustomLineForm.tsx:236-240`) is the established precedent, and this convention applies regardless of Server/Client Component type (per `app/layout.tsx:145-154`'s own comment on why).
- **`useLoginHref()` is the single source of truth for the `return_to`-building calculation.** `LoginLink` and `LoginPromptModal` both consume it; neither may independently call `usePathname()`/`useSearchParams()` and rebuild the string itself (Decision 1, Open Question 2).
- **No task renames or changes the wire/URL shape of `/api/auth/login?return_to=...`.** Only where the calculation lives moves (into `useLoginHref()`); the resulting string is byte-identical to today's `LoginLink.tsx:35-38` output for the same inputs.
- **`LoginPromptModal` renders unconditionally at every migrated call site** (mirroring `DeleteLineButton.tsx:66-83`'s own `<Modal opened={opened} onClose={close}>` usage) — no call site wraps it in its own `{needsLogin && ...}` guard; Mantine's `Modal` already no-ops visually when `opened` is `false`.
- **Mantine's `Modal` close (×) button has no default accessible name** — confirmed by reading `@mantine/core`'s shipped source this session (`ModalBaseCloseButton.cjs`/`CloseButton.cjs`: no `aria-label` default anywhere in the chain; `Modal`'s own `defaultProps` never sets `closeButtonProps`). This is a real gap the spec's Open Question 4 didn't check (it deferred to Mantine's own docs rather than the installed package's source). `LoginPromptModal` must pass `closeButtonProps={{ 'aria-label': 'Close' }}` explicitly — both for basic accessibility (an icon-only, unlabeled button otherwise) and so `LoginPromptModal.test.tsx` (Task 2) has a reliable way to assert the close button fires `onClose` without relying on `Escape`/backdrop-click timing quirks in jsdom.
- **Testing convention:** colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npx vitest run` from `frontend/`). Every task's verification step runs the relevant test file(s) via `npx vitest run <path>`; Task 9 runs the full suite plus `npx tsc --noEmit` plus `npm run build`.
- **Parallelizable tasks:** Task 7 (`app/layout.tsx`) depends on nothing in this plan — it touches no file any other task touches and doesn't consume `LoginPromptModal`/`useLoginHref` at all, so it can be dispatched at any point, even before Task 1. Tasks 3, 4, 5, 6, and 8 each depend only on Task 2 (transitively Task 1) and touch disjoint files — parallelizable once Task 2 lands. Task 9 depends on every other task having landed.

---

### Task 1: `useLoginHref()` extraction + `LoginLink` refactor

**Files:**
- Create: `frontend/components/useLoginHref.ts`
- Create: `frontend/components/useLoginHref.test.ts`
- Modify: `frontend/components/LoginLink.tsx`

**Interfaces:**
- Produces: `useLoginHref(): string` — a Client-Component-only hook returning the full `/api/auth/login?return_to=...` href for the current page.
- Consumed by: `LoginLink.tsx` (this task, internally) and `LoginPromptModal.tsx` (Task 2).
- **Depends on:** nothing — foundational.

Current `LoginLink.tsx` (full file, `frontend/components/LoginLink.tsx:1-44`) builds the href inline at lines 35-38:

```tsx
  const pathname = usePathname();
  const search = useSearchParams().toString();
  const returnTo = search ? `${pathname}?${search}` : pathname;
  const href = `/api/auth/login?return_to=${encodeURIComponent(returnTo)}`;
```

- [ ] **Step 1: Create `frontend/components/useLoginHref.ts`**

```typescript
'use client';

import { usePathname, useSearchParams } from 'next/navigation';

/** Builds the `/api/auth/login?return_to=...` href from the current page's
 * path + query string. Extracted out of `LoginLink.tsx` (see that file's own
 * doc comment on why a URL fragment can never be captured this way) so
 * `LoginPromptModal` doesn't duplicate or reimplement this three-line
 * calculation — see
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md Decision 1. */
export function useLoginHref(): string {
  const pathname = usePathname();
  const search = useSearchParams().toString();
  const returnTo = search ? `${pathname}?${search}` : pathname;
  return `/api/auth/login?return_to=${encodeURIComponent(returnTo)}`;
}
```

- [ ] **Step 2: Write `frontend/components/useLoginHref.test.ts`**

Mirrors the URL-encoding assertions already in `LoginLink.test.tsx:14-52`, via `renderHook` instead of a rendered component:

```typescript
import { describe, it, expect, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useLoginHref } from './useLoginHref';

const mockUsePathname = vi.fn();
const mockUseSearchParams = vi.fn();
vi.mock('next/navigation', () => ({
  usePathname: () => mockUsePathname(),
  useSearchParams: () => mockUseSearchParams(),
}));

describe('useLoginHref', () => {
  it('returns return_to = pathname alone when there is no query string', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    const { result } = renderHook(() => useLoginHref());
    expect(result.current).toBe('/api/auth/login?return_to=%2Flines%2Fsome-line');
  });

  it('appends pathname + query string, URL-encoded, when a query string is present', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams('tab=history'));
    const { result } = renderHook(() => useLoginHref());
    expect(result.current).toBe('/api/auth/login?return_to=%2Flines%2Fsome-line%3Ftab%3Dhistory');
  });

  it('renders the root path correctly', () => {
    mockUsePathname.mockReturnValue('/');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    const { result } = renderHook(() => useLoginHref());
    expect(result.current).toBe('/api/auth/login?return_to=%2F');
  });
});
```

- [ ] **Step 3: Run the new test**

Run: `cd frontend && npx vitest run components/useLoginHref.test.ts`
Expected: PASS (3 tests).

- [ ] **Step 4: Refactor `LoginLink.tsx` to consume `useLoginHref()`**

Replace the full file:

```tsx
'use client';

import { useLoginHref } from './useLoginHref';
import { TextLink } from './TextLink';

/** Wraps `TextLink` with the shared `return_to`-bearing login href (see
 * `useLoginHref.ts`), so `GET /auth/callback`
 * (`crates/api/src/routes/auth.rs`) can send the user back here instead of
 * always to `SSO_POST_LOGIN_REDIRECT_URL`. See
 * docs/superpowers/specs/2026-08-31-dynamic-post-login-redirect-design.md's
 * Design → Where the return path is captured.
 *
 * A separate Client Component rather than adding these hooks to `TextLink`
 * itself: `usePathname()`/`useSearchParams()` (inside `useLoginHref`) are
 * Client-Component-only hooks, and `TextLink` must stay server-renderable
 * (see its own doc comment) since most of its call sites are Server
 * Components. This mirrors the existing `AuthStatus.tsx` embeds
 * `LogoutButton.tsx` pattern -- a small interactive Client Component leaf
 * inside a server-rendered tree.
 *
 * Deliberately cannot capture a URL fragment (`#...`) -- a fragment is
 * never sent to the server on any HTTP request, by construction of the
 * URL/HTTP specs; there is no mechanism here or anywhere else that could
 * round-trip one through a full-page OIDC redirect. Known, accepted
 * limitation -- see the design spec's Open Questions. */
export function LoginLink({
  children,
  underline,
}: {
  children: React.ReactNode;
  underline?: 'hover' | 'always';
}) {
  const href = useLoginHref();
  return (
    <TextLink href={href} underline={underline}>
      {children}
    </TextLink>
  );
}
```

- [ ] **Step 5: Run `LoginLink`'s existing tests unmodified to confirm no behavior change**

Run: `cd frontend && npx vitest run components/LoginLink.test.tsx`
Expected: PASS, all 4 existing assertions in `LoginLink.test.tsx:1-53` unchanged — the rendered `href` output is byte-identical, only where the calculation lives moved. Do **not** edit `LoginLink.test.tsx` in this task.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/useLoginHref.ts frontend/components/useLoginHref.test.ts frontend/components/LoginLink.tsx
git commit -m "Extract useLoginHref() out of LoginLink, shared with the new LoginPromptModal"
```

---

### Task 2: New `LoginPromptModal` component

**Files:**
- Create: `frontend/components/LoginPromptModal.tsx`
- Create: `frontend/components/LoginPromptModal.test.tsx`

**Interfaces:**
- Consumes: `useLoginHref()` (Task 1).
- Produces: `LoginPromptModal({ opened: boolean; onClose: () => void; children: React.ReactNode })` — a Client Component; consumed by Tasks 3, 4, 5, 6 (directly) and Task 8 (via the new `AutoOpenLoginPrompt` wrapper).
- **Depends on:** Task 1.

`DeleteLineButton.tsx:66-83` and `DeleteTrainButton.tsx:76-93` are the direct precedent for this component's own `Modal` usage (controlled `opened`/`onClose`, `Group justify="end" mt="md"` footer).

- [ ] **Step 1: Write the failing test, `frontend/components/LoginPromptModal.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginPromptModal } from './LoginPromptModal';

const mockUsePathname = vi.fn();
const mockUseSearchParams = vi.fn();
vi.mock('next/navigation', () => ({
  usePathname: () => mockUsePathname(),
  useSearchParams: () => mockUseSearchParams(),
}));

describe('LoginPromptModal', () => {
  beforeEach(() => {
    mockUsePathname.mockReturnValue('/lines');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
  });

  it('renders nothing interactive when opened is false', () => {
    renderWithMantine(
      <LoginPromptModal opened={false} onClose={vi.fn()}>
        Log in to pin this line.
      </LoginPromptModal>,
    );
    expect(screen.queryByText('Log in to pin this line.')).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in' })).not.toBeInTheDocument();
  });

  it('renders the fixed title, the body children, and a Log in link with the correct return_to href when opened', () => {
    renderWithMantine(
      <LoginPromptModal opened={true} onClose={vi.fn()}>
        Log in to pin this line.
      </LoginPromptModal>,
    );
    expect(screen.getByText('Log in required')).toBeInTheDocument();
    expect(screen.getByText('Log in to pin this line.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines',
    );
  });

  it('calls onClose when the close button fires', () => {
    const onClose = vi.fn();
    renderWithMantine(
      <LoginPromptModal opened={true} onClose={onClose}>
        Log in to pin this line.
      </LoginPromptModal>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run it to confirm it fails**

Run: `cd frontend && npx vitest run components/LoginPromptModal.test.tsx`
Expected: FAIL — `Cannot find module './LoginPromptModal'`.

- [ ] **Step 3: Write `frontend/components/LoginPromptModal.tsx`**

```tsx
'use client';

import Link from 'next/link';
import { Button, Group, Modal, Text } from '@mantine/core';
import { useLoginHref } from './useLoginHref';

/** Thin, fully-controlled presentational wrapper -- mirrors
 * `DeleteLineButton.tsx:66-83`/`DeleteTrainButton.tsx:76-93`'s own
 * `<Modal opened={opened} onClose={close}>` usage. Every migrated call
 * site renders this unconditionally; Mantine's `Modal` already no-ops
 * visually when `opened` is `false`, so callers don't need their own
 * `{needsLogin && ...}` guard the way the inline `LoginLink` version
 * required. `children` is call-site-specific body prose (mirrors
 * `LoginLink`'s existing flexibility) -- there is deliberately no `verb`
 * prop; the title is a fixed constant, never a prop. See
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
 * Decisions 1-3.
 *
 * `closeButtonProps={{ 'aria-label': 'Close' }}`: Mantine's `Modal` close
 * button has no default accessible name (confirmed by reading the
 * installed `@mantine/core` source -- neither `ModalBaseCloseButton` nor
 * the underlying `CloseButton` sets one), so this is set explicitly for
 * basic accessibility, not just test convenience. */
export function LoginPromptModal({
  opened,
  onClose,
  children,
}: {
  opened: boolean;
  onClose: () => void;
  children: React.ReactNode;
}) {
  const href = useLoginHref();
  return (
    <Modal opened={opened} onClose={onClose} title="Log in required" closeButtonProps={{ 'aria-label': 'Close' }}>
      <Text>{children}</Text>
      <Group justify="end" mt="md">
        {/* Plain `<Link>` wrapping `Button`, not `component={Link}` on the
            Mantine polymorphic prop -- established convention regardless of
            Server/Client boundary, see `CustomLineForm.tsx:236-240`'s own
            Cancel button and this design's Decision 3. */}
        <Link href={href} style={{ textDecoration: 'none' }}>
          <Button>Log in</Button>
        </Link>
      </Group>
    </Modal>
  );
}
```

- [ ] **Step 4: Run the test to confirm it passes**

Run: `cd frontend && npx vitest run components/LoginPromptModal.test.tsx`
Expected: PASS (3 tests).

- [ ] **Step 5: Commit**

```bash
git add frontend/components/LoginPromptModal.tsx frontend/components/LoginPromptModal.test.tsx
git commit -m "Add LoginPromptModal, a thin controlled Modal wrapper around useLoginHref"
```

---

### Task 3: Migrate `PinToggle.tsx` (adopt `useNeedsLogin`)

**Files:**
- Modify: `frontend/components/PinToggle.tsx`
- Modify: `frontend/components/PinToggle.test.tsx`

**Interfaces:**
- Consumes: `useNeedsLogin()` (`frontend/components/useNeedsLogin.ts`, unchanged), `LoginPromptModal` (Task 2).
- **Depends on:** Task 2.

Current `PinToggle.tsx` (full file, `frontend/components/PinToggle.tsx:1-129`) hand-rolls `const [needsLogin, setNeedsLogin] = useState(false);` at line 46, resets it at line 62 (`setNeedsLogin(false)` inside `toggle()`), sets it at lines 78 and 94 (the two independent 401 branches — the `GET /api/preferences` read and the `PUT` write), and renders it inline at line 125: `{needsLogin && <LoginLink underline="always">Log in to pin</LoginLink>}`.

- [ ] **Step 1: Update the component's imports and state**

Replace:
```tsx
import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Group, Tooltip } from '@mantine/core';
import { LoginLink } from './LoginLink';
import type { Preferences } from '@/lib/types';
```
with:
```tsx
import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Group, Tooltip } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import type { Preferences } from '@/lib/types';
```

Replace (`PinToggle.tsx:39-46`):
```tsx
export function PinToggle({ kind, id, initiallyPinned }: { kind: PinKind; id: string; initiallyPinned: boolean }) {
  const router = useRouter();
  const [pinned, setPinned] = useState(initiallyPinned);
  const [busy, setBusy] = useState(false);
  // Set on a 401 from either request below, cleared at the start of every
  // fresh attempt. Surfaces *why* the click did nothing, instead of the
  // dead-click silence this replaced (see the comment further down).
  const [needsLogin, setNeedsLogin] = useState(false);
```
with:
```tsx
export function PinToggle({ kind, id, initiallyPinned }: { kind: PinKind; id: string; initiallyPinned: boolean }) {
  const router = useRouter();
  const [pinned, setPinned] = useState(initiallyPinned);
  const [busy, setBusy] = useState(false);
  // Set on a 401 from either request below, reset at the start of every
  // fresh attempt. Surfaces *why* the click did nothing, instead of the
  // dead-click silence this replaced (see the comment further down).
  const needsLoginState = useNeedsLogin();
```

- [ ] **Step 2: Swap the reset/set calls inside `toggle()`**

`setNeedsLogin(false)` (`PinToggle.tsx:62`) → `needsLoginState.reset()`.
Both `setNeedsLogin(true)` occurrences (`PinToggle.tsx:78`, inside the prefs-`GET` 401 branch, and `:94`, inside the `PUT` 401 branch) → `needsLoginState.markNeedsLogin()`.

- [ ] **Step 3: Swap the render**

Replace (`PinToggle.tsx:110-127`)'s closing:
```tsx
      {needsLogin && <LoginLink underline="always">Log in to pin</LoginLink>}
    </Group>
  );
}
```
with:
```tsx
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to pin this {kind}.
      </LoginPromptModal>
    </Group>
  );
}
```

- [ ] **Step 4: Update `PinToggle.test.tsx`'s login-prompt assertions**

Replace the two `findByRole('link', { name: 'Log in to pin' })` assertions (`PinToggle.test.tsx:136-147`, `:151-164`):

```tsx
  it('a 401 from the preferences read shows the login prompt modal, linking to /api/auth/login', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async () => new Response('no session', { status: 401 }));

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    expect(screen.queryByText('Log in to pin this line.')).not.toBeInTheDocument();

    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

    expect(await screen.findByText('Log in to pin this line.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines',
    );
  });

  // A 401 on the PUT (read succeeded, write didn't) must surface the same
  // prompt — the anonymous-visitor case can fail at either step.
  it('a 401 from the PUT also shows the login prompt modal', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation(async (url) => {
      if (url === '/api/preferences') {
        return new Response(JSON.stringify({ pinnedLines: [], pinnedStations: [] }), { status: 200 });
      }
      return new Response('no session', { status: 401 });
    });

    renderWithMantine(<PinToggle kind="line" id="wcml" initiallyPinned={false} />);
    fireEvent.click(screen.getByLabelText('Pin (currently not pinned)'));

    expect(await screen.findByText('Log in to pin this line.')).toBeInTheDocument();
  });
```

And replace the non-401 negative assertion (`PinToggle.test.tsx:168-179`)'s final check:
```tsx
    expect(screen.queryByRole('link', { name: 'Log in to pin' })).not.toBeInTheDocument();
```
with:
```tsx
    expect(screen.queryByText('Log in to pin this line.')).not.toBeInTheDocument();
```

`PinToggle.test.tsx:116-130` (the "401 from the preferences read leaves the button usable and issues no PUT" test) needs no change — it never asserted on `LoginLink`'s rendered output.

- [ ] **Step 5: Run the tests**

Run: `cd frontend && npx vitest run components/PinToggle.test.tsx`
Expected: PASS (all 9 existing tests, 2 rewritten).

- [ ] **Step 6: Commit**

```bash
git add frontend/components/PinToggle.tsx frontend/components/PinToggle.test.tsx
git commit -m "Migrate PinToggle onto LoginPromptModal + shared useNeedsLogin"
```

---

### Task 4: Migrate `CustomLineForm.tsx`

**Files:**
- Modify: `frontend/app/lines/CustomLineForm.tsx`
- Modify: `frontend/app/lines/CustomLineForm.test.tsx`

**Interfaces:**
- Consumes: `LoginPromptModal` (Task 2). Already uses `useNeedsLogin()` (`app/lines/CustomLineForm.tsx:41`) — no hook-adoption work needed here, unlike Tasks 3/5/6.
- **Depends on:** Task 2.

This form already calls `useNeedsLogin()` (`app/lines/CustomLineForm.tsx:41`, `.reset()` at `:97`, `.markNeedsLogin()` at `:121`) — this task only swaps the render, the smallest of the four component migrations.

- [ ] **Step 1: Swap the import**

Replace (`CustomLineForm.tsx:9-10`):
```tsx
import { useNeedsLogin } from '@/components/useNeedsLogin';
import { LoginLink } from '@/components/LoginLink';
```
with:
```tsx
import { useNeedsLogin } from '@/components/useNeedsLogin';
import { LoginPromptModal } from '@/components/LoginPromptModal';
```

- [ ] **Step 2: Swap the render**

Replace (`CustomLineForm.tsx:221-225`):
```tsx
      {needsLoginState.needsLogin && (
        <LoginLink underline="always">
          Log in to {existingLine ? 'edit' : 'create'} a line
        </LoginLink>
      )}
```
with:
```tsx
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to {existingLine ? 'edit' : 'create'} a custom line.
      </LoginPromptModal>
```

- [ ] **Step 3: Update `CustomLineForm.test.tsx`'s login-prompt assertions**

Replace the two `findByRole('link', ...)` assertions (`CustomLineForm.test.tsx:225-240` and `:242-262`):

```tsx
  it('a 401 on save shows the login prompt modal instead of the raw backend error text', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('no session', { status: 401 });
      }
      return new Response('[]', { status: 200 });
    });

    mockUsePathname.mockReturnValue('/lines/my-line/edit');
    renderWithProvider({ existingLine });
    fireEvent.click(screen.getByRole('button', { name: 'Save changes' }));

    expect(await screen.findByText('Log in to edit a custom line.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines%2Fmy-line%2Fedit',
    );
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
  });

  it('a 401 on create shows a login prompt worded for creating, not editing', async () => {
    vi.mocked(fetch).mockImplementation(async (url) => {
      if (typeof url === 'string' && url.startsWith('/api/lines')) {
        return new Response('no session', { status: 401 });
      }
      return new Response('[]', { status: 200 });
    });

    mockUsePathname.mockReturnValue('/lines/new');
    renderWithProvider();
    fireEvent.change(screen.getByLabelText('Name'), { target: { value: 'My Commute' } });
    const stationInput = screen.getByRole('combobox', { name: 'Add station (CRS code)' });
    fireEvent.change(stationInput, { target: { value: 'WOK' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.change(stationInput, { target: { value: 'CLJ' } });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    fireEvent.click(screen.getByRole('button', { name: 'Create line' }));

    expect(await screen.findByText('Log in to create a custom line.')).toBeInTheDocument();
  });
```

And replace the non-401 negative assertion (`CustomLineForm.test.tsx:264-279`)'s final check:
```tsx
    expect(screen.queryByRole('link', { name: /Log in to/ })).not.toBeInTheDocument();
```
with:
```tsx
    expect(screen.queryByRole('link', { name: 'Log in' })).not.toBeInTheDocument();
```

- [ ] **Step 4: Run the tests**

Run: `cd frontend && npx vitest run app/lines/CustomLineForm.test.tsx`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/lines/CustomLineForm.tsx frontend/app/lines/CustomLineForm.test.tsx
git commit -m "Migrate CustomLineForm's login prompt onto LoginPromptModal"
```

---

### Task 5: Migrate `TrackTrainForm.tsx` (adopt `useNeedsLogin`)

**Files:**
- Modify: `frontend/components/TrackTrainForm.tsx`
- Modify: `frontend/components/TrackTrainForm.test.tsx`

**Interfaces:**
- Consumes: `useNeedsLogin()`, `LoginPromptModal` (Task 2).
- **Depends on:** Task 2.

Current `TrackTrainForm.tsx` (full file, `frontend/components/TrackTrainForm.tsx:1-203`) hand-rolls `const [needsLogin, setNeedsLogin] = useState(false);` at line 56, resets it at line 66, sets it at line 118 (the single 401 branch), and renders it inline inside a `Group` with the submit button at lines 190-199 (block at 194-198).

- [ ] **Step 1: Update imports and state**

Replace (`TrackTrainForm.tsx:1-8`):
```tsx
'use client';

import { useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, TextInput } from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import dayjs from 'dayjs';
import { LoginLink } from './LoginLink';
```
with:
```tsx
'use client';

import { useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, TextInput } from '@mantine/core';
import { DateTimePicker } from '@mantine/dates';
import dayjs from 'dayjs';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
```

Replace (`TrackTrainForm.tsx:55-56`):
```tsx
  const [submitting, setSubmitting] = useState(false);
  const [needsLogin, setNeedsLogin] = useState(false);
```
with:
```tsx
  const [submitting, setSubmitting] = useState(false);
  const needsLoginState = useNeedsLogin();
```

- [ ] **Step 2: Swap the reset/set calls in `handleSubmit`**

`setNeedsLogin(false)` (`TrackTrainForm.tsx:66`) → `needsLoginState.reset()`.
`setNeedsLogin(true)` (`TrackTrainForm.tsx:118`, the sole 401 branch: `if (response.status === 401) { setNeedsLogin(true); return; }`) → `needsLoginState.markNeedsLogin()`.

- [ ] **Step 3: Swap the render**

Replace (`TrackTrainForm.tsx:190-199`):
```tsx
      <Group>
        <Button type="submit" disabled={!canSubmit}>
          {submitting ? 'Tracking…' : 'Track this train'}
        </Button>
        {needsLogin && (
          <LoginLink underline="always">
            Log in to track this train
          </LoginLink>
        )}
      </Group>
    </Stack>
  );
}
```
with:
```tsx
      <Group>
        <Button type="submit" disabled={!canSubmit}>
          {submitting ? 'Tracking…' : 'Track this train'}
        </Button>
      </Group>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this train.
      </LoginPromptModal>
    </Stack>
  );
}
```

(Moved out of the `Group` — a dialog overlay has no layout relationship to the button that triggered it, unlike the inline text it replaces. The component's own doc comment's "no navigation away" invariant, `TrackTrainForm.tsx:37-42`, is unaffected: `LoginPromptModal` overlays the page without unmounting the form or resetting any of its four typed fields.)

- [ ] **Step 4: Update `TrackTrainForm.test.tsx`'s login-prompt assertion**

Replace the `findByRole('link', ...)` assertion (`TrackTrainForm.test.tsx:96-114`):

```tsx
  it('on a 401, shows the login prompt modal and preserves the typed field values', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<TrackTrainForm initialOrigin="WAT" />);
    fireEvent.change(screen.getByLabelText(/Destination CRS code/), { target: { value: 'WOK' } });
    fireEvent.change(screen.getByLabelText(/Scheduled departure/), {
      target: { value: '2026-08-28 18:32:00' },
    });
    fireEvent.click(screen.getByRole('button', { name: /Track this train/ }));

    expect(await screen.findByText('Log in to track this train.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrack',
    );
    // Unlike PinToggle's toggle-and-forget click, the form's own input
    // must survive a 401 -- Decision 4's explicit "preserve typed values"
    // call.
    expect(screen.getByLabelText(/Origin CRS code/)).toHaveValue('WAT');
    expect(screen.getByLabelText(/Destination CRS code/)).toHaveValue('WOK');
  });
```

- [ ] **Step 5: Run the tests**

Run: `cd frontend && npx vitest run components/TrackTrainForm.test.tsx`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/TrackTrainForm.tsx frontend/components/TrackTrainForm.test.tsx
git commit -m "Migrate TrackTrainForm onto LoginPromptModal + shared useNeedsLogin"
```

---

### Task 6: Migrate `TicketEntryForm.tsx` (adopt `useNeedsLogin`)

**Files:**
- Modify: `frontend/components/TicketEntryForm.tsx`
- Modify: `frontend/components/TicketEntryForm.test.tsx`

**Interfaces:**
- Consumes: `useNeedsLogin()`, `LoginPromptModal` (Task 2).
- **Depends on:** Task 2.

Current `TicketEntryForm.tsx` (full file, `frontend/components/TicketEntryForm.tsx:1-424`) hand-rolls `const [needsLogin, setNeedsLogin] = useState(false);` at line 71, shared by **two** independent async flows: `handleUpload` (resets at line 131, sets at line 149 on a 401) and `handleSubmit` (resets at line 182, sets at line 218 on a 401). Rendered inline inside a `Group` at lines 368-380 (block at 375-379).

- [ ] **Step 1: Update imports and state**

Replace (`TicketEntryForm.tsx:1-8`):
```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, FileInput, Group, Stack, Tabs, TextInput, Text } from '@mantine/core';
import { LoginLink } from './LoginLink';
import { TextLink } from './TextLink';
```
with:
```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, FileInput, Group, Stack, Tabs, TextInput, Text } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { TextLink } from './TextLink';
```

Replace (`TicketEntryForm.tsx:69-71`):
```tsx
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [needsLogin, setNeedsLogin] = useState(false);
```
with:
```tsx
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();
```

- [ ] **Step 2: Swap the reset/set calls in both `handleUpload` and `handleSubmit`**

In `handleUpload`: `setNeedsLogin(false)` (`TicketEntryForm.tsx:131`) → `needsLoginState.reset()`; `setNeedsLogin(true)` (`:149`, the `if (response.status === 401)` branch) → `needsLoginState.markNeedsLogin()`.

In `handleSubmit`: `setNeedsLogin(false)` (`TicketEntryForm.tsx:182`) → `needsLoginState.reset()`; `setNeedsLogin(true)` (`:218`, the `if (response.status === 401)` branch) → `needsLoginState.markNeedsLogin()`.

Both flows keep sharing the one `needsLoginState` — same shared-state shape as the hand-rolled version, unchanged behavior (a 401 from either the upload or the final submit surfaces the identical prompt).

- [ ] **Step 3: Swap the render**

Replace (`TicketEntryForm.tsx:368-380`):
```tsx
      <Group>
        <Button onClick={handleSubmit} disabled={submitting || !originValid || !destinationValid}>
          {submitting ? 'Saving…' : 'Save ticket'}
        </Button>
        <Button variant="subtle" onClick={() => setOpen(false)}>
          Cancel
        </Button>
        {needsLogin && (
          <LoginLink underline="always">
            Log in to save this ticket
          </LoginLink>
        )}
      </Group>
    </Stack>
  );
}
```
with:
```tsx
      <Group>
        <Button onClick={handleSubmit} disabled={submitting || !originValid || !destinationValid}>
          {submitting ? 'Saving…' : 'Save ticket'}
        </Button>
        <Button variant="subtle" onClick={() => setOpen(false)}>
          Cancel
        </Button>
      </Group>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to save this ticket.
      </LoginPromptModal>
    </Stack>
  );
}
```

- [ ] **Step 4: Update `TicketEntryForm.test.tsx`'s two login-prompt assertions**

Replace the manual-submit 401 assertion (`TicketEntryForm.test.tsx:82-91`):
```tsx
  it('manual submit: on a 401, shows the login prompt modal and preserves typed fields', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.change(screen.getByLabelText('Operator'), { target: { value: 'LNER' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save ticket' }));

    expect(await screen.findByText('Log in to save this ticket.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrain%2Fby-id%2F1',
    );
    expect(screen.getByLabelText('Operator')).toHaveValue('LNER');
  });
```

And the upload 401 assertion (`TicketEntryForm.test.tsx:199-206`):
```tsx
  it('a 401 during upload shows the login prompt modal, same as the final-submit 401 handling', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));
    openForm();
    fireEvent.click(screen.getByRole('tab', { name: 'Upload .pkpass' }));
    const file = new File(['fake'], 'ticket.pkpass', { type: 'application/octet-stream' });
    fireEvent.change(getPkpassFileInput(), { target: { files: [file] } });
    expect(await screen.findByText('Log in to save this ticket.')).toBeInTheDocument();
  });
```

- [ ] **Step 5: Run the tests**

Run: `cd frontend && npx vitest run components/TicketEntryForm.test.tsx`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/TicketEntryForm.tsx frontend/components/TicketEntryForm.test.tsx
git commit -m "Migrate TicketEntryForm onto LoginPromptModal + shared useNeedsLogin"
```

---

### Task 7: `TrackedTrainsNavItem` Tier 3→2 reclassification

**Files:**
- Modify: `frontend/app/layout.tsx`
- Modify: `frontend/app/layout.test.tsx`

**Interfaces:**
- Produces: `TrackedTrainsNavItem()` — was `async`, now a plain sync function component; consumed by `RootLayout` in the same file.
- **Depends on:** nothing in this plan — independent of `LoginPromptModal`/`useLoginHref` entirely. Can be dispatched in parallel with every other task.

Per Decision 6: the real 401 gate stays exactly where it already lives (`/track/mine`'s own `getMyTrackedTrains()` null-on-401 check, Task 8) — this task only removes the now-redundant client-side `getSession()` gate on the nav link itself.

- [ ] **Step 1: Replace `TrackedTrainsNavItem` and its doc comment**

Replace (`app/layout.tsx:88-119`):
```tsx
// Same rationale as AuthNavItem/DataFreshnessNavItem: a separate async
// Server Component so <Suspense> can stream the session check in without
// blocking the rest of the shell. Unlike those two, this one renders
// nothing at all when logged out (Decision 4 of
// docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md) --
// this is a full nav-bar entry point to a page whose entire content is
// private to the viewer, not a section of an already-public page (the
// TicketPanel pattern), so showing it to every visitor and having it
// always resolve to a login nudge would be dead weight for the common
// case of an anonymous visitor. Guarded with the same .catch() shape as
// AuthNavItem/DataFreshnessNavItem: a root layout has no route-level
// error.tsx, so an unguarded getSession() here could take down every
// page's nav bar on an auth glitch -- the same historical bug class
// already fixed in TicketPanel.tsx, not repeated here.
//
// Labelled "My Trains & Tickets," not "My Tracked Trains," now that
// `/track/mine` is the single merged page for both (Part B of the
// upload-first ticket-tracking plan) -- the separate `MyTicketsNavItem`
// this file used to also render (pointing at the now-redirected
// `/track/tickets`) is gone; one nav entry for one merged page.
export async function TrackedTrainsNavItem() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  if (!session.authenticated) {
    return null;
  }
  return <TextLink href="/track/mine">My Trains &amp; Tickets</TextLink>;
}
```
with:
```tsx
// Reclassified from Tier 3 (hidden entirely when logged out) to
// always-visible, per
// docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
// Decision 6 -- a deliberate, named reversal of
// docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md's
// Decision 4, which chose "hidden entirely" specifically because at the
// time an anonymous click would have resolved to a bare inline sentence
// with nothing else on the page. Now that `/track/mine`'s own existing
// `getMyTrackedTrains()` null-on-401 gate (unchanged -- see
// `app/track/mine/page.tsx`) opens a real, actionable `LoginPromptModal`
// instead, "dead weight in the nav bar" no longer describes what a
// logged-out click produces, so this link is worth advertising rather
// than hiding.
//
// No `getSession()` call here any more, and no `Suspense` wrapper needed
// at the call site below -- the real gating already lives entirely on
// `/track/mine`'s own page, which has no id in its path to disambiguate
// (same reasoning that page's own doc comment already gives for not
// needing a second `getSession()` call of its own). Adding a second,
// client-side session check here just to decide what to render would be
// duplicate plumbing for a decision this nav item no longer needs to
// make.
//
// Labelled "My Trains & Tickets," not "My Tracked Trains," now that
// `/track/mine` is the single merged page for both (Part B of the
// upload-first ticket-tracking plan).
export function TrackedTrainsNavItem() {
  return <TextLink href="/track/mine">My Trains &amp; Tickets</TextLink>;
}
```

- [ ] **Step 2: Drop the now-unnecessary `Suspense` wrapper**

Replace (`app/layout.tsx:163-168`):
```tsx
                <Group gap="lg">
                  <TextLink href="/lines">All Lines</TextLink>
                  <TextLink href="/stations">Station Lookup</TextLink>
                  <Suspense fallback={null}>
                    <TrackedTrainsNavItem />
                  </Suspense>
```
with:
```tsx
                <Group gap="lg">
                  <TextLink href="/lines">All Lines</TextLink>
                  <TextLink href="/stations">Station Lookup</TextLink>
                  <TrackedTrainsNavItem />
```

(matching how the two `TextLink`s immediately above it already render as plain, non-`Suspense`-wrapped children — `Suspense` is still imported and still used by `DataFreshnessNavItem`/`AuthNavItem` below, so the `import { Suspense } from 'react';` line at `app/layout.tsx:2` stays.)

- [ ] **Step 3: Rewrite `layout.test.tsx`'s `TrackedTrainsNavItem` describe block**

Replace (`app/layout.test.tsx:13-35`):
```tsx
describe('TrackedTrainsNavItem', () => {
  it('hides "My Trains & Tickets" when logged out', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    renderWithMantine(await TrackedTrainsNavItem());
    // Not `toBeEmptyDOMElement()` on the container: MantineProvider injects
    // <style> tags into the render tree regardless, so the container is
    // never literally empty (see TicketPanel.test.tsx's 404 case for the
    // same established workaround on other `return null` components).
    expect(screen.queryByRole('link', { name: 'My Trains & Tickets' })).not.toBeInTheDocument();
  });

  it('shows "My Trains & Tickets", pointing at /track/mine, when logged in', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: 'a@b.com', name: 'Ada' });
    renderWithMantine(await TrackedTrainsNavItem());
    expect(screen.getByRole('link', { name: 'My Trains & Tickets' })).toHaveAttribute('href', '/track/mine');
  });

  it('degrades to hidden (not a thrown error) when getSession rejects', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('auth service unreachable'));
    renderWithMantine(await TrackedTrainsNavItem());
    expect(screen.queryByRole('link', { name: 'My Trains & Tickets' })).not.toBeInTheDocument();
  });
});
```
with:
```tsx
describe('TrackedTrainsNavItem', () => {
  it('renders "My Trains & Tickets" unconditionally, pointing at /track/mine', () => {
    // No session check here any more (Decision 6 of
    // docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md) --
    // the real login gate lives entirely on /track/mine's own page now,
    // covered by app/track/mine/page.test.tsx instead.
    renderWithMantine(<TrackedTrainsNavItem />);
    expect(screen.getByRole('link', { name: 'My Trains & Tickets' })).toHaveAttribute('href', '/track/mine');
  });
});
```

- [ ] **Step 4: Drop the now-dead `api` mock, if nothing else in the file needs it**

`layout.test.tsx:5,7` (`import * as api from '@/lib/api';` and `vi.mock('@/lib/api');`) exist only to support the `TrackedTrainsNavItem` describe block just rewritten — the file's other two describe blocks (`viewport.themeColor`/`viewport.colorScheme`, `metadata.appleWebApp`, `layout.test.tsx:37-75`) import only `viewport`/`metadata` from `./layout` and never touch `@/lib/api`. Remove both lines; the file's remaining top import becomes:
```tsx
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackedTrainsNavItem, viewport, metadata } from './layout';
```
(`vi` is no longer used anywhere else in this file once the `api` mock is gone — drop it from the `vitest` import too, per the snippet above.)

- [ ] **Step 5: Run the tests**

Run: `cd frontend && npx vitest run app/layout.test.tsx`
Expected: PASS (1 test in the rewritten describe block + the 2 unrelated `viewport`/`metadata` tests, unchanged).

- [ ] **Step 6: Commit**

```bash
git add frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Reclassify My Trains & Tickets nav item to always-visible (Tier 3 -> 2)"
```

---

### Task 8: `/track/mine`'s auto-opened `LoginPromptModal`

**Files:**
- Create: `frontend/app/track/mine/AutoOpenLoginPrompt.tsx`
- Modify: `frontend/app/track/mine/page.tsx`
- Modify: `frontend/app/track/mine/page.test.tsx`

**Interfaces:**
- Consumes: `LoginPromptModal` (Task 2).
- Produces: `AutoOpenLoginPrompt({ children: React.ReactNode })` — a small Client Component, consumed by `page.tsx` in the same directory.
- **Depends on:** Task 2.

**A real, small correction to the spec's own Decision 6 wording, found while verifying against source:** Decision 6 describes the new piece as "a local Client Component wrapper... inside this same file [`page.tsx`], the same way `TrackedTrainListRow`/`RowStatusBadge` are already local, non-exported helpers." That precedent doesn't transfer: `TrackedTrainListRow`/`RowStatusBadge` are plain server-rendered functions with no hooks, and Next.js's Server/Client Component boundary is a **whole-file** `'use client'` directive, not a per-function one. `page.tsx`'s default export must stay a Server Component — it directly `await`s `getMyTrackedTrains()`/`getMyTickets()`, both of which read server-only config (same reasoning `PinToggle.tsx`'s own doc comment gives for why *that* component proxies through `/api/*` instead of calling `lib/api.ts` directly). A `useState(true)` therefore cannot live inside `page.tsx` itself; the wrapper needs its own sibling file with `'use client'` at the top, imported into `page.tsx` — hence the new `AutoOpenLoginPrompt.tsx` file above, colocated in the same route folder (not exported outside it, matching the spirit of "local" the design intended).

- [ ] **Step 1: Create `frontend/app/track/mine/AutoOpenLoginPrompt.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { LoginPromptModal } from '@/components/LoginPromptModal';

/** `/track/mine`'s own page must stay a Server Component (it directly
 * awaits `getMyTrackedTrains()`/`getMyTickets()`), so it can't hold the
 * `useState` a controlled `LoginPromptModal` needs itself -- this small
 * sibling Client Component is the wrapper. See
 * docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md
 * Decision 6.
 *
 * Starts open (there is no click event on a Server Component to open it
 * from) and stays fully controlled to match `LoginPromptModal`'s own
 * contract exactly, the same as `DeleteLineButton`/`DeleteTrainButton`'s
 * controlled-`Modal` convention. Closing it (Escape, backdrop, or its own
 * close control) leaves the page showing just its heading -- a deliberate,
 * accepted simplification (Decision 6's Open Question 1), not a gap this
 * component tries to close. A fresh navigation back to `/track/mine`
 * reopens it, since `opened` re-initializes to `true` on every fresh
 * mount. */
export function AutoOpenLoginPrompt({ children }: { children: React.ReactNode }) {
  const [opened, setOpened] = useState(true);
  return (
    <LoginPromptModal opened={opened} onClose={() => setOpened(false)}>
      {children}
    </LoginPromptModal>
  );
}
```

- [ ] **Step 2: Swap `page.tsx`'s import and null branch**

Replace (`app/track/mine/page.tsx:1-11`)'s `LoginLink` import:
```tsx
import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyTrackedTrains, getMyTickets } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
```
with:
```tsx
import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyTrackedTrains, getMyTickets } from '@/lib/api';
import { AutoOpenLoginPrompt } from './AutoOpenLoginPrompt';
import { TextLink } from '@/components/TextLink';
```

Replace the null branch (`app/track/mine/page.tsx:49-58`):
```tsx
  if (trains === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <LoginLink underline="always">
          Log in to see the trains and tickets you&apos;re tracking
        </LoginLink>
      </Stack>
    );
  }
```
with:
```tsx
  if (trains === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Trains &amp; Tickets</Title>
        <AutoOpenLoginPrompt>
          Log in to see the trains and tickets you&apos;re tracking.
        </AutoOpenLoginPrompt>
      </Stack>
    );
  }
```

- [ ] **Step 3: Update `page.test.tsx`'s login-nudge test**

Replace (`app/track/mine/page.test.tsx:64-71`):
```tsx
  it('null (not logged in): shows a login nudge', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(
      screen.getByRole('link', { name: "Log in to see the trains and tickets you're tracking" }),
    ).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrack%2Fmine');
  });
```
with:
```tsx
  it('null (not logged in): shows an auto-opened login prompt modal', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue(null);
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await MyTrackedTrainsPage());
    expect(screen.getByText('Log in required')).toBeInTheDocument();
    expect(screen.getByText("Log in to see the trains and tickets you're tracking.")).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Ftrack%2Fmine',
    );
  });
```

This test's existing `vi.mock('next/navigation', ...)` stub (`page.test.tsx:16-20`) already fixes `usePathname` to `'/track/mine'` with an empty `useSearchParams()`, which is exactly what `AutoOpenLoginPrompt` → `LoginPromptModal` → `useLoginHref()` needs — no change needed there.

- [ ] **Step 4: Run the tests**

Run: `cd frontend && npx vitest run app/track/mine/page.test.tsx`
Expected: PASS (all 14 tests; only the one rewritten above changes behavior).

- [ ] **Step 5: Commit**

```bash
git add frontend/app/track/mine/AutoOpenLoginPrompt.tsx frontend/app/track/mine/page.tsx frontend/app/track/mine/page.test.tsx
git commit -m "Auto-open LoginPromptModal on /track/mine's existing null-on-401 gate"
```

---

### Task 9: Final verification

**Files:** none (verification only).

**Depends on:** Tasks 1-8, all landed.

- [ ] **Step 1: Run the full frontend test suite**

Run: `cd frontend && npx vitest run`
Expected: PASS, with zero new failures. In particular, confirm the following files needed **no** edits anywhere in this plan and still pass exactly as they did before Task 1, per the design's own Testing section claim:
- `frontend/components/useNeedsLogin.test.ts` (hook's contract untouched — Decision 1)
- `frontend/components/DeleteLineButton.test.tsx`
- `frontend/components/DeleteTrainButton.test.tsx`

- [ ] **Step 2: Run TypeScript's project-wide type check**

Run: `cd frontend && npx tsc --noEmit`
Expected: PASS, zero errors. This is where a missed call site (e.g. a leftover `LoginLink` import, or a `needsLogin`/`setNeedsLogin` reference from before the `useNeedsLogin()` migration) would surface as a compile error if any of Tasks 3, 5, or 6 left a stale reference.

- [ ] **Step 3: Run the production build**

Run: `cd frontend && npm run build`
Expected: PASS. This exercises the real Server/Client Component boundary — in particular, confirms `AutoOpenLoginPrompt.tsx`'s `'use client'` split (Task 8) actually satisfies Next.js's build-time boundary check the way `app/layout.tsx`'s own comment (`app/layout.tsx:145-154`) warns about for the `Link`-wrapping-`Button` pattern used throughout `LoginPromptModal` and every migrated call site.

- [ ] **Step 4: Manual spot-check (optional but recommended given this touches five distinct interactive surfaces)**

Start the dev stack (per this repo's own `docker compose`/dev conventions) and, logged out, exercise: pinning a line or station (`PinToggle`), creating a custom line (`CustomLineForm`), tracking a train (`TrackTrainForm`), saving a ticket (`TicketEntryForm`), and navigating to `/track/mine` from the nav bar — confirming each opens `LoginPromptModal` with the expected body text and a working "Log in" link, and that "My Trains & Tickets" is now visible in the nav bar while logged out (Task 7).

No commit for this task — verification only.

---

## Self-review notes

- **Spec coverage:** Decision 1 (hook unchanged, new component) → Tasks 2-8 all keep `useNeedsLogin.ts` untouched. Decision 2 (fixed title, `children` body) → Task 2's `LoginPromptModal` implementation + Global Constraints. Decision 3 (`Link`-wrapping-`Button`) → Task 2. Decision 4 (4 migrate, `DeleteLineButton`/`DeleteTrainButton`/`TicketPanel`/both train pages/`app/page.tsx`/`AuthStatus` stay inline) → Tasks 3-6 + Global Constraints' explicit exclusion list. Decision 5 (opportunistic `useNeedsLogin` adoption bundled into the same pass) → Tasks 3, 5, 6 each fold this in, per the task prompt's own explicit instruction to not treat it as separate. Decision 6 (Tier 3→2, nav simplification + auto-opened modal) → Tasks 7 and 8. The `useLoginHref()` extraction (Decision 1, Open Question 2) → Task 1. Testing section → each task's own test-file steps plus Task 9's confirmation that `useNeedsLogin.test.ts`/`DeleteLineButton.test.tsx`/`DeleteTrainButton.test.tsx` need no changes.
- **Placeholder scan:** every step above contains real, complete code (no "add appropriate handling," no "similar to Task N" without the actual diff).
- **Type consistency:** `LoginPromptModal({ opened: boolean; onClose: () => void; children: React.ReactNode })` is the one signature used identically by Tasks 3, 4, 5, 6 (directly) and Task 8 (via `AutoOpenLoginPrompt`, which forwards `children` through unchanged). `useLoginHref(): string` (Task 1) is the one signature consumed by both `LoginLink.tsx` (Task 1) and `LoginPromptModal.tsx` (Task 2).
- **Corrected citations vs. the spec:** the spec's own inventory table cited `PinToggle.tsx:125` (confirmed exact), but `CustomLineForm.tsx:244-248` (current: `221-225`, since `main` has since dropped the old create-mode manual-reset branch — see this plan's own "IMPORTANT" note above), `TrackTrainForm.tsx:194-198` (confirmed exact), `TicketEntryForm.tsx:375-379` (confirmed exact), and `app/track/mine/page.tsx:53-55` (confirmed exact, block is `49-58`). `app/layout.tsx:108-119` for `TrackedTrainsNavItem` and `:166-168` for its `Suspense` wrapper are both confirmed exact against `main`.
- **New finding beyond the spec:** Mantine's shipped `Modal` close button has no default accessible name (verified by reading `@mantine/core`'s installed source, not just its docs) — `LoginPromptModal` sets `closeButtonProps={{ 'aria-label': 'Close' }}` explicitly (Task 2, Global Constraints) to fix this and make it testable.
- **New finding beyond the spec:** Decision 6's "wrapper inside `page.tsx`, like `TrackedTrainListRow`/`RowStatusBadge`" doesn't transfer, since those helpers hold no hooks and `page.tsx` must stay a Server Component. Task 8 creates a sibling file instead and documents why.
