# Dynamic Color-Scheme Meta Tag Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make this app emit a `<meta name="color-scheme" content="light|dark">` tag that always reflects the app's own currently-resolved theme, so Dark Reader (and similar extensions) can use it as an opt-out signal instead of double-dark-theming an already-dark page.

**Architecture:** A new `viewport` export on `frontend/app/layout.tsx` renders the tag server-side with a fixed `'light'` default (the same pre-mount fallback `ThemeToggle`/`useComputedColorScheme('light')` already uses); a new tiny Client Component, `ColorSchemeMeta`, mounted once inside `MantineProvider` alongside `AutoRefresh`, imperatively keeps that tag's `content` attribute in sync with the resolved theme after mount, via `useComputedColorScheme('light')` + `useMounted()` — the exact same gated-`useEffect`-DOM-mutation shape `PrideToggle.tsx` already uses for `document.body.dataset.pride`. Nothing renders the tag itself in JSX a second time, so there is no new hydration-mismatch surface.

**Tech Stack:** Next.js 16 App Router `Viewport` metadata API (`export const viewport: Viewport`, no new dependency); Mantine v9 `useComputedColorScheme`/`useMounted` (already in use elsewhere in this codebase); Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper for tests.

**Spec:** `docs/superpowers/specs/2026-09-01-dynamic-color-scheme-meta-design.md` — read in full before starting; this plan carries its Decisions into concrete tasks and does not restate its research. Note: the spec's own header currently reads "Status: design proposal, not approved" — this plan proceeds on the explicit instruction that its shape is to be implemented; if that status line is still unresolved when this plan is executed, confirm with whoever requested implementation before landing Task 3 (the user-visible wiring step), not before Tasks 1–2 (both inert in isolation — an unused `viewport` export and an unmounted component change nothing about the live app).

**Status note — every citation below re-confirmed directly against this worktree's current source, not trusted from the spec:** `frontend/app/layout.tsx` (157 lines): `metadata: Metadata` export at lines 16–20, `import type { Metadata } from 'next';` at line 5, `<html {...mantineHtmlProps}>` at line 92, `<ColorSchemeScript defaultColorScheme="auto" />` at line 94, `<MantineProvider theme={theme} defaultColorScheme="auto">` at line 97, `<AutoRefresh />` at line 98, `import { AutoRefresh } from '@/components/AutoRefresh';` at line 11 — all match the spec's own citations exactly, no drift. `frontend/components/ThemeToggle.tsx` (68 lines): `useComputedColorScheme('light')` at line 36, `useMounted()` at line 37 — confirmed. `frontend/components/PrideToggle.tsx` (168 lines): the `document.body.dataset.pride = mode` effect is at lines 130–133 exactly as the spec cites. `frontend/components/AutoRefresh.tsx` (23 lines): its own doc comment "Side-effect-only component (renders nothing) mounted once in the root layout" is at line 8, `return null;` at line 22. `frontend/app/layout.test.tsx` (35 lines): imports only `{ TrackedTrainsNavItem } from './layout';` (line 4) today — no test of `metadata` or any other export exists yet. `frontend/components/ThemeToggle.test.tsx` (116 lines): `renderWithProvider` helper at lines 9–11 (`renderWithMantine(<ThemeToggle />, { defaultColorScheme: 'auto' })`), the SSR/localStorage test seeds `localStorage.setItem('mantine-color-scheme-value', 'dark')` at line 75. `frontend/test/render.tsx` (26 lines): `renderWithMantine(ui, options)` at lines 18–26, `options.defaultColorScheme` passed straight through to `MantineProvider`. `frontend/vitest.setup.ts`: the `matchMedia` polyfill (lines 13–27) always reports `matches: false` (no dark preference), so `'auto'` resolves to `'light'` in every test unless `localStorage['mantine-color-scheme-value']` is seeded first. `frontend/package.json`'s `scripts` has `"test": "vitest run"` and `"build": "next build"` (both run from `frontend/`); there is no `lint` script in this package, so no task below adds a lint step. No barrel/index file re-exports components — every component is imported directly by path (`@/components/AutoRefresh`, confirmed as the only import site via grep), so `ColorSchemeMeta` will be imported the same way, with no additional export-surface file to update.

## Global Constraints

- **No new dependency, anywhere.** Every change in this plan reuses hooks/types already used in this codebase (`useComputedColorScheme`, `useMounted`, Next's `Viewport` type) — no new npm package.
- **The `viewport` export's `colorScheme` value is exactly `'light'`, never `'light dark'` and never the deprecated `Metadata.colorScheme` field** (spec Decision 1). It is added *alongside* the existing `metadata` export, not merged into it.
- **`ColorSchemeMeta` must never render the `<meta>` tag in its own JSX.** The mutation is strictly imperative, inside a `useEffect` gated on `useMounted()`, mirroring `PrideToggle.tsx:130-133`'s exact pattern (spec Decision 2). Rendering the tag via JSX a second time was explicitly considered and rejected in the spec — it would fight the tag Next's `viewport` export already renders at the same `<head>` position and reintroduce the hydration-mismatch bug class already fixed in `ThemeToggle`/`PrideToggle`/`LastUpdated`.
- **The tag's `content` is always a single value (`'light'` or `'dark'`), never the multi-value `'light dark'` form** (spec Decision 4) — that multi-value form is specifically the one Dark Reader's own bug tracker found unreliable; recreating it in the new tag would defeat the point.
- **Do not touch `@mantine/core/styles.css`'s injected `:root { color-scheme: light dark; }` rule, or add any `globals.css` rule to override/suppress it** (spec Decision 3, Explicitly out of scope). That CSS property governs native browser UI only and is a separate signal from the meta tag; no task in this plan touches `globals.css` or any Mantine stylesheet.
- **Out of scope, per the spec — do not add in any task:** a blocking inline script to close the initial-paint timing gap (Decision 1's rejected alternative); a `theme-color` meta tag (`Viewport.themeColor`); handling for any extension other than Dark Reader; the `darkreader-lock` opt-out.
- **No SSR/hydration-mismatch `renderToString` test is planned for `ColorSchemeMeta` itself** (spec Decision 5, "Explicitly not attempted") — `viewport.colorScheme` is resolved by Next's separate metadata pipeline, not by rendering `RootLayout`'s function body, so there is no equivalent render-diff test to write. Do not add one.
- **Testing convention:** colocated `*.test.tsx` files, `@testing-library/react`, this repo's `renderWithMantine` helper (`frontend/test/render.tsx`), Vitest via `npm test` (run from `frontend/`). Every frontend task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures.
- **Parallelizable tasks:** Task 1 (`viewport` export) and Task 2 (`ColorSchemeMeta` component) touch disjoint files and depend on nothing but the current source — they can be dispatched to separate subagents in parallel. Task 3 (wiring) depends on both landing first, since it edits `layout.tsx` a second time (after Task 1's edit) and imports `ColorSchemeMeta` (from Task 2). Task 4 (manual verification) depends on Task 3.

---

### Task 1: `viewport` export on `layout.tsx`

**Files:**
- Modify: `frontend/app/layout.tsx:5` (import), `frontend/app/layout.tsx:16-20` (add export after `metadata`)
- Test: `frontend/app/layout.test.tsx`

**Interfaces:**
- Produces: `export const viewport: Viewport` from `frontend/app/layout.tsx`, shape `{ colorScheme: 'light' }`.
- Consumed by: Next's metadata pipeline directly (no code consumes this import elsewhere); Task 4's manual verification checks the tag it produces in a real browser.
- **Depends on:** nothing — foundational, and inert on its own (an unused static export changes nothing the app renders differently, since nothing yet reads or displays this tag's value beyond what Next itself does at SSR time).

- [ ] **Step 1: Write the failing test**

In `frontend/app/layout.test.tsx`, change the import on line 4 from:

```typescript
import { TrackedTrainsNavItem } from './layout';
```

to:

```typescript
import { TrackedTrainsNavItem, viewport } from './layout';
```

Add a new top-level `describe` block (this file currently has no test file-level import of `describe`/`it`/`expect` beyond `vitest`'s named imports at line 1, which already include them):

```typescript
describe('viewport', () => {
  it('defaults color-scheme to light for the pre-hydration SSR render', () => {
    // No route in this app defines its own viewport/metadata export
    // (confirmed by grep against frontend/app/ — this worktree's plan doc
    // for this feature cites the same check), so this root-level default
    // is the value Next actually renders for every page. 'light' matches
    // ThemeToggle's own pre-mount fallback (useComputedColorScheme('light')),
    // not a new, third opinion about what "unknown" means.
    expect(viewport.colorScheme).toBe('light');
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run (from `frontend/`): `npm test -- layout.test.tsx`
Expected: FAIL — `viewport` is not exported from `./layout` (TypeScript/module error), since the export doesn't exist yet.

- [ ] **Step 3: Add the `viewport` export**

In `frontend/app/layout.tsx`, change line 5 from:

```typescript
import type { Metadata } from 'next';
```

to:

```typescript
import type { Metadata, Viewport } from 'next';
```

Then add immediately after the existing `metadata` export (currently ending at line 20):

```typescript
export const viewport: Viewport = {
  colorScheme: 'light',
};
```

- [ ] **Step 4: Run the test to verify it passes**

Run (from `frontend/`): `npm test -- layout.test.tsx`
Expected: PASS, including the pre-existing `TrackedTrainsNavItem` tests (unaffected by this change).

- [ ] **Step 5: Commit**

```bash
git add frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Add a viewport export so layout.tsx renders <meta name=color-scheme content=light> on first paint"
```

---

### Task 2: `ColorSchemeMeta` component

**Files:**
- Create: `frontend/components/ColorSchemeMeta.tsx`
- Test: `frontend/components/ColorSchemeMeta.test.tsx`

**Interfaces:**
- Produces: `export function ColorSchemeMeta()` — a Client Component, renders `null`, no props.
- Consumed by: Task 3, which mounts `<ColorSchemeMeta />` inside `MantineProvider` in `frontend/app/layout.tsx`.
- **Depends on:** nothing — foundational, and inert on its own until Task 3 mounts it (an unmounted component changes nothing).

This component is modeled directly on two existing patterns, both re-confirmed by direct read (see this plan's Status note):
- `frontend/components/AutoRefresh.tsx`'s overall shape: "side-effect-only component (renders nothing), mounted once in the root layout" (its own doc comment, line 8) — the established slot for "one global, invisible, mount-once Client Component."
- `frontend/components/PrideToggle.tsx:130-133`'s exact imperative-DOM-mutation mechanism:
  ```typescript
  useEffect(() => {
    if (!mounted) return;
    document.body.dataset.pride = mode;
  }, [mode, mounted]);
  ```
  `ColorSchemeMeta` applies the same `useMounted()`-gated `useEffect` shape to a `<meta>` tag's `content` attribute instead of `body`'s dataset, using `useComputedColorScheme('light')` — the identical hook and fallback value `ThemeToggle.tsx:36` already uses — in place of `PrideToggle`'s own `localStorage`-derived `mode` state.

- [ ] **Step 1: Write the failing tests**

Create `frontend/components/ColorSchemeMeta.test.tsx`:

```typescript
import { describe, it, expect, afterEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ThemeToggle } from './ThemeToggle';
import { ColorSchemeMeta } from './ColorSchemeMeta';

function colorSchemeMetaTags() {
  return document.head.querySelectorAll('meta[name="color-scheme"]');
}

describe('ColorSchemeMeta', () => {
  afterEach(() => {
    // This component's whole job is to mutate document.head outside
    // React's own render tree — @testing-library/react's automatic
    // cleanup unmounts the component but never removes a tag it appended
    // itself, so each test must undo that by hand or the next test starts
    // from a dirty document.head.
    colorSchemeMetaTags().forEach((tag) => tag.remove());
  });

  it('creates the meta tag with content="light" once mounted, matching the default resolved scheme', () => {
    renderWithMantine(<ColorSchemeMeta />);
    const tags = colorSchemeMetaTags();
    expect(tags).toHaveLength(1);
    expect(tags[0]).toHaveAttribute('content', 'light');
  });

  it('sets content="dark" when the stored preference resolves to dark', () => {
    // Same persistence key ThemeToggle.test.tsx's own SSR test seeds
    // (mantine-color-scheme-value) — Mantine reads it directly on mount,
    // no provider prop needed to set this up.
    localStorage.setItem('mantine-color-scheme-value', 'dark');
    renderWithMantine(<ColorSchemeMeta />);
    const tags = colorSchemeMetaTags();
    expect(tags).toHaveLength(1);
    expect(tags[0]).toHaveAttribute('content', 'dark');
  });

  it('updates content as the resolved scheme changes, without ever duplicating the tag', () => {
    // Mounted alongside ThemeToggle, same as production (both sit inside
    // MantineProvider in app/layout.tsx) — clicking ThemeToggle's button
    // is what actually changes the resolved scheme; ColorSchemeMeta has no
    // UI of its own to drive this directly.
    renderWithMantine(
      <>
        <ColorSchemeMeta />
        <ThemeToggle />
      </>,
      { defaultColorScheme: 'auto' },
    );
    // matchMedia is polyfilled (vitest.setup.ts) to report no dark
    // preference, so 'auto' resolves to light first.
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'light');

    const button = screen.getByRole('button');
    fireEvent.click(button); // auto -> light
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'light');

    fireEvent.click(button); // light -> dark
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'dark');

    fireEvent.click(button); // dark -> auto
    expect(colorSchemeMetaTags()).toHaveLength(1);
    expect(colorSchemeMetaTags()[0]).toHaveAttribute('content', 'light');
  });

  it("reuses an already-present tag (as Next's SSR-rendered viewport tag would be) instead of creating a duplicate", () => {
    const existing = document.createElement('meta');
    existing.setAttribute('name', 'color-scheme');
    existing.setAttribute('content', 'light');
    document.head.appendChild(existing);

    localStorage.setItem('mantine-color-scheme-value', 'dark');
    renderWithMantine(<ColorSchemeMeta />);

    const tags = colorSchemeMetaTags();
    expect(tags).toHaveLength(1);
    expect(tags[0]).toBe(existing);
    expect(tags[0]).toHaveAttribute('content', 'dark');
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- ColorSchemeMeta.test.tsx`
Expected: FAIL — `ColorSchemeMeta` module doesn't exist yet.

- [ ] **Step 3: Write `ColorSchemeMeta.tsx`**

Create `frontend/components/ColorSchemeMeta.tsx`:

```typescript
'use client';

import { useComputedColorScheme } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { useEffect } from 'react';

/** Side-effect-only component (renders nothing), mounted once in the root
 * layout alongside AutoRefresh — keeps the <meta name="color-scheme"> tag
 * Next's `viewport` export renders at SSR time (see app/layout.tsx) in
 * sync with the app's actually-resolved theme after every client-side
 * change. Dark Reader's own maintainer named this single-value tag,
 * specifically, as the current opt-out signal it checks (see
 * docs/superpowers/specs/2026-09-01-dynamic-color-scheme-meta-design.md).
 *
 * Same useMounted()-gated imperative-DOM-mutation shape PrideToggle.tsx
 * already uses for document.body.dataset.pride, and the same
 * useComputedColorScheme('light') hook/fallback ThemeToggle.tsx already
 * uses — no new hook, no new gating pattern, no new fallback constant.
 *
 * Deliberately never renders the tag in this component's own JSX: doing so
 * would fight the tag Next's `viewport` export already renders at the same
 * <head> position, and would reintroduce the hydration-mismatch bug class
 * already fixed in ThemeToggle/PrideToggle/LastUpdated. Because this only
 * runs inside a useEffect body gated on mounted, it never fires before
 * hydration completes and never runs at all during SSR. */
export function ColorSchemeMeta() {
  const computedColorScheme = useComputedColorScheme('light');
  const mounted = useMounted();

  useEffect(() => {
    if (!mounted) return;
    let tag = document.querySelector('meta[name="color-scheme"]');
    if (!tag) {
      tag = document.createElement('meta');
      tag.setAttribute('name', 'color-scheme');
      document.head.appendChild(tag);
    }
    tag.setAttribute('content', computedColorScheme);
  }, [mounted, computedColorScheme]);

  return null;
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- ColorSchemeMeta.test.tsx`
Expected: PASS, all four tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/ColorSchemeMeta.tsx frontend/components/ColorSchemeMeta.test.tsx
git commit -m "Add ColorSchemeMeta: keeps <meta name=color-scheme> in sync with the resolved theme post-mount"
```

---

### Task 3: Wire `ColorSchemeMeta` into `layout.tsx`

**Files:**
- Modify: `frontend/app/layout.tsx:11` (import), `frontend/app/layout.tsx:98` (mount)

**Interfaces:**
- Consumes: `ColorSchemeMeta` from Task 2 (`@/components/ColorSchemeMeta`); the `viewport` export from Task 1 (no direct code dependency — both land in the same file, but Task 1's export and this task's JSX change are independent edits to `layout.tsx` that must be sequenced, not merged, since Task 1 already commits its own hunk).
- Produces: the fully wired feature — from this task onward, every server-rendered page emits the SSR tag (Task 1) and keeps it live post-mount (Task 2, now actually mounted).
- **Depends on:** Task 1 and Task 2 both landed first (this task edits `layout.tsx` a second time, after Task 1's edit, and needs `ColorSchemeMeta` to exist to import it).

There is no existing precedent in this codebase for rendering the full `RootLayout` (a Server Component that returns `<html>`/`<body>`) inside a Vitest/jsdom test — `layout.test.tsx` today only renders the exported `TrackedTrainsNavItem` function via `renderWithMantine`, never `RootLayout` itself. Per the spec's own Testing section ("Explicitly not attempted"), no such test is added here either; this task's verification is the full test suite (regression-checking Tasks 1–2's tests still pass unchanged) plus `next build`, which exercises Next's real metadata/viewport pipeline and would fail on a bad import or a Server/Client boundary mistake.

- [ ] **Step 1: Add the import**

In `frontend/app/layout.tsx`, add a new import line immediately after the existing `AutoRefresh` import (currently line 11):

```typescript
import { AutoRefresh } from '@/components/AutoRefresh';
import { ColorSchemeMeta } from '@/components/ColorSchemeMeta';
```

- [ ] **Step 2: Mount the component**

In `frontend/app/layout.tsx`, immediately after `<AutoRefresh />` (currently line 98, inside `<MantineProvider theme={theme} defaultColorScheme="auto">`):

```tsx
<MantineProvider theme={theme} defaultColorScheme="auto">
  <AutoRefresh />
  <ColorSchemeMeta />
```

Placement matches `AutoRefresh`'s own "one global, invisible, mount-once inside `MantineProvider`" slot (spec Decision 2) — not the nav `Group` where `ThemeToggle`/`PrideToggle` live, since this component has no UI and doesn't need to be inside the nav at all.

- [ ] **Step 3: Run the full frontend test suite**

Run (from `frontend/`): `npm test`
Expected: PASS — no new failures. This is a regression check (Tasks 1 and 2's own tests already passed in isolation); this step confirms the import/mount doesn't break anything elsewhere (e.g. a snapshot or a test that enumerates `layout.tsx`'s rendered children).

- [ ] **Step 4: Run the production build**

Run (from `frontend/`): `npm run build`
Expected: PASS — `next build` exercises the real Next.js metadata pipeline (`viewport` export) and the Server/Client component boundary (`ColorSchemeMeta` imported into a Server Component file), catching any type or boundary error `npm test` wouldn't.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/layout.tsx
git commit -m "Mount ColorSchemeMeta in the root layout, alongside AutoRefresh"
```

---

### Task 4: Manual verification (not unit-testable)

**Files:** none — this task runs the built app and inspects it directly; it makes no code changes.

**Interfaces:**
- Consumes: the fully wired feature from Task 3.
- **Depends on:** Task 3.

Per the spec's own Open questions/risks section (item 2): "No way to verify against a real installed Dark Reader instance from within this design pass... whoever implements this should ideally do a manual check with Dark Reader installed, in both this app's light and dark modes, before considering the feature validated." This is the step where that happens. None of Tasks 1–3's automated tests can substitute for it: Task 1's test only checks the static export's value, Task 2's tests run in jsdom (no real `<head>` rendered by an actual browser, no real extension present), and Task 3 has no dedicated test at all beyond the full suite + build passing.

- [ ] **Step 1: Build and run the app locally**

Run (from `frontend/`): `npm run build && npm run start`

- [ ] **Step 2: Inspect the tag in a real browser, without Dark Reader**

Open the running app. In DevTools' Elements panel, confirm `<head>` contains exactly one `<meta name="color-scheme" content="light">` on first load (default theme).

- [ ] **Step 3: Confirm the tag updates live**

Click `ThemeToggle` (the sun/moon icon in the nav) to cycle to dark. Re-inspect `<head>`: confirm the same tag's `content` attribute is now `"dark"` — not a second tag, the same node updated in place. Cycle back through `auto`/`light` and confirm it tracks each resolved value.

- [ ] **Step 4: Confirm behavior with Dark Reader installed**

Install the Dark Reader browser extension (if not already available). With the app in its own light mode, confirm Dark Reader does not additionally dark-theme the page (i.e. it reads the `content="light"` tag as a signal to leave the page alone). Switch the app to its own dark mode via `ThemeToggle` and confirm Dark Reader also backs off once `content="dark"` is set — per the spec's Recommendation section, this is a best-effort signal, not a guarantee, so the acceptance bar here is "Dark Reader visibly respects the tag in this manual check," not "this is proven to work for every visitor."

- [ ] **Step 5: Record the outcome**

No commit is produced by this task (no code changes). If manual verification surfaces an unexpected result (e.g. Dark Reader still double-themes despite the correct tag), do not silently ship — raise it against the spec's own Recommendation section (which already documents this as a known, non-guaranteed limitation) rather than treating it as a bug in Tasks 1–3's implementation, since the tag itself was already independently confirmed correct in Steps 2–3 above.

---
