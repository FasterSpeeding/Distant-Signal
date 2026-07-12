# Dark Theme Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user switch between light, dark, and system themes via a
toggle in the site nav, persisted across visits.

**Architecture:** Mantine's built-in color-scheme machinery
(`defaultColorScheme="auto"` on both `ColorSchemeScript` and
`MantineProvider`, localStorage-persisted by default) does almost all of
the work, since every color in this app already goes through Mantine
theme tokens. The only new code is a small `ThemeToggle` Client
Component that cycles light → dark → auto on click, rendered in the
existing nav.

**Tech Stack:** Next.js/React/Mantine v9/vitest.

## Global Constraints

- Spec: `docs/superpowers/specs/2026-07-12-dark-theme-design.md`.
- No custom Mantine theme, no per-component color audit — confirmed
  during planning that every color reference in `frontend/` already goes
  through Mantine theme tokens or CSS variables (spec's Non-goals).
- No backend/`Preferences` change — Mantine's default
  `colorSchemeManager` (localStorage) is sufficient.
- `ColorSchemeScript`'s `defaultColorScheme` and `MantineProvider`'s
  `defaultColorScheme` must be set to the same value (`"auto"`) — a
  mismatch reintroduces the flash-of-wrong-theme `ColorSchemeScript`
  exists to prevent.
- `jsdom`'s `matchMedia` is already polyfilled in
  `frontend/vitest.setup.ts` to always report no dark preference
  (`matches: false`) — this is pre-existing, not something this plan
  adds. It means `'auto'` always resolves to `'light'` in tests; account
  for that in test assertions rather than trying to mock a "system is
  dark" scenario.
- This repo's `docker-compose.yml` (unrelated prior work, since merged to
  `master`) requires a Compose profile — every service needs
  `--profile prod` or `--profile dev`; there's no profile-less default.
  Task 3's verification commands account for this.

---

## File Structure

- Create: `frontend/components/ThemeToggle.tsx` — cycling toggle button.
- Create: `frontend/components/ThemeToggle.test.tsx`.
- Modify: `frontend/app/layout.tsx` — `defaultColorScheme="auto"` on both
  `ColorSchemeScript` and `MantineProvider`; render `<ThemeToggle />` in
  the nav.

---

### Task 1: ThemeToggle component

**Files:**
- Create: `frontend/components/ThemeToggle.tsx`
- Create: `frontend/components/ThemeToggle.test.tsx`

**Interfaces:**
- Produces: `export function ThemeToggle()` — no props. Task 2 renders it
  in the nav.

- [ ] **Step 1: Write `frontend/components/ThemeToggle.tsx`**

```tsx
'use client';

import { ActionIcon, useComputedColorScheme, useMantineColorScheme } from '@mantine/core';
import type { MantineColorScheme } from '@mantine/core';

const NEXT_SCHEME: Record<MantineColorScheme, MantineColorScheme> = {
  light: 'dark',
  dark: 'auto',
  auto: 'light',
};

/** Cycles light -> dark -> auto -> light on click. The icon reflects the
 * *resolved* appearance (`useComputedColorScheme`) rather than the raw
 * preference, so picking "auto" shows whichever of sun/moon actually
 * matches the system right now instead of a generic third icon; the
 * `aria-label` states the raw preference (including "auto" itself) so
 * it's still clear which of the three states is selected. */
export function ThemeToggle() {
  const { colorScheme, setColorScheme } = useMantineColorScheme();
  const computedColorScheme = useComputedColorScheme('light');

  return (
    <ActionIcon
      variant="outline"
      onClick={() => setColorScheme(NEXT_SCHEME[colorScheme])}
      aria-label={`Theme: ${colorScheme}. Click to switch.`}
    >
      {computedColorScheme === 'dark' ? '🌙' : '☀️'}
    </ActionIcon>
  );
}
```

- [ ] **Step 2: Write the failing tests**

Create `frontend/components/ThemeToggle.test.tsx`:

```tsx
import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { ThemeToggle } from './ThemeToggle';

function renderWithProvider() {
  return render(
    <MantineProvider defaultColorScheme="auto">
      <ThemeToggle />
    </MantineProvider>,
  );
}

describe('ThemeToggle', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('starts on auto (the default) with a label stating so', () => {
    renderWithProvider();
    expect(screen.getByLabelText('Theme: auto. Click to switch.')).toBeInTheDocument();
  });

  it('cycles auto -> light -> dark -> auto on repeated clicks', () => {
    renderWithProvider();
    const button = screen.getByRole('button');

    fireEvent.click(button);
    expect(screen.getByLabelText('Theme: light. Click to switch.')).toBeInTheDocument();

    fireEvent.click(button);
    expect(screen.getByLabelText('Theme: dark. Click to switch.')).toBeInTheDocument();

    fireEvent.click(button);
    expect(screen.getByLabelText('Theme: auto. Click to switch.')).toBeInTheDocument();
  });

  it('shows the sun icon when resolved to light, moon when resolved to dark', () => {
    renderWithProvider();
    const button = screen.getByRole('button');
    // matchMedia is polyfilled (vitest.setup.ts) to always report no dark
    // preference, so "auto" resolves to light here.
    expect(button).toHaveTextContent('☀️');

    fireEvent.click(button); // -> light
    expect(button).toHaveTextContent('☀️');

    fireEvent.click(button); // -> dark
    expect(button).toHaveTextContent('🌙');
  });
});
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `npm --prefix frontend test -- ThemeToggle`
Expected: FAIL — `./ThemeToggle` module doesn't exist yet (if Step 1
wasn't done first) or, if it was, this step is a formality confirming
the test file itself is wired up correctly. Since the task brief already
gives you Step 1's code, do Step 1 first, then run this to confirm
Step 2's tests pass against it, matching the RED/GREEN evidence this
project's plans record.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npm --prefix frontend test -- ThemeToggle`
Expected: all 3 tests pass.

- [ ] **Step 5: Run the full frontend test suite and type check**

Run: `npm --prefix frontend test && npm --prefix frontend run build`
Expected: all existing tests still pass; `next build` succeeds.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/ThemeToggle.tsx frontend/components/ThemeToggle.test.tsx
git commit -m "Add ThemeToggle: cycles light/dark/auto color scheme"
```

---

### Task 2: Wire dark theme into the app layout

**Files:**
- Modify: `frontend/app/layout.tsx`

**Interfaces:**
- Consumes: `ThemeToggle` (Task 1).

- [ ] **Step 1: Replace the full contents of `frontend/app/layout.tsx`**

```tsx
import '@/app/globals.css';
import { MantineProvider, ColorSchemeScript, mantineHtmlProps, Group, Text } from '@mantine/core';
import Link from 'next/link';
import type { Metadata } from 'next';
import { ThemeToggle } from '@/components/ThemeToggle';

export const metadata: Metadata = {
  title: 'National Rail Status',
  description: 'Line status for UK National Rail, TfL-style.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" {...mantineHtmlProps}>
      <head>
        <ColorSchemeScript defaultColorScheme="auto" />
      </head>
      <body>
        <MantineProvider defaultColorScheme="auto">
          <Group
            component="nav"
            justify="space-between"
            p="md"
            style={{ borderBottom: '1px solid var(--mantine-color-default-border)' }}
          >
            {/* Plain `<Link>` wrapping Mantine's `Text`, rather than
                `component={Link}` on a Mantine polymorphic prop: this file
                is a Server Component, and passing the `Link` component
                reference into a Mantine `component` prop from a Server
                Component previously broke `next build`'s Server/Client
                boundary serialization check (see LineStatusCard fix).
                `ThemeToggle` below doesn't hit this: it's imported and
                rendered as a plain JSX element (a Client Component child
                of this Server Component), not passed as a value into a
                Mantine `component` prop — a different, safe pattern. */}
            <Link href="/" style={{ textDecoration: 'none', color: 'inherit' }}>
              <Text fw={700}>National Rail Line Status</Text>
            </Link>
            <Group gap="lg">
              <Link href="/lines" style={{ textDecoration: 'none' }}>
                <Text c="blue">All Lines</Text>
              </Link>
              <Link href="/stations" style={{ textDecoration: 'none' }}>
                <Text c="blue">Station Lookup</Text>
              </Link>
              <ThemeToggle />
            </Group>
          </Group>
          {children}
        </MantineProvider>
      </body>
    </html>
  );
}
```

- [ ] **Step 2: Verify build and existing tests**

Run: `npm --prefix frontend run build && npm --prefix frontend test`
Expected: build succeeds, all tests pass (this file has no dedicated test
— no `app/layout.tsx` test exists today, and this task doesn't add one,
consistent with this codebase's established precedent of not testing
`app/*` layout/page files; the toggle's own behavior is already covered
by Task 1's tests).

- [ ] **Step 3: Commit**

```bash
git add frontend/app/layout.tsx
git commit -m "Wire dark theme support into the app layout"
```

---

### Task 3: End-to-end verification

**Files:** none (verification only).

- [ ] **Step 1: Bring up the stack**

This repo's `docker-compose.yml` requires a Compose profile (added after
this plan's spec was written — see `docker-compose.yml`'s header
comment) — every service needs `--profile prod` or `--profile dev`, there
is no profile-less default. Use prod (optimized build; either profile
works equally well for this verification, prod avoids a slower debug
compile):

Run: `docker compose --profile prod up -d postgres api frontend`, wait
for `api` and `frontend` to be reachable.

- [ ] **Step 2: Confirm the homepage renders the toggle with the expected default state**

```bash
curl -s http://localhost:3000/ | grep -o 'aria-label="Theme:[^"]*"'
```

Expected: `aria-label="Theme: auto. Click to switch."` (the default,
since no prior localStorage value exists for a fresh `curl` — though
note `ColorSchemeScript`'s initial `data-mantine-color-scheme` attribute
on `<html>` reflects the *resolved* scheme at SSR time based on the
`defaultColorScheme` prop, not a per-request preference; a real browser
with a stored preference would differ from this raw `curl` check, which
has no localStorage/cookies at all — this check only confirms the
server-rendered default, not the client-side persistence behavior
already covered by Task 1's tests).

- [ ] **Step 3: Confirm `data-mantine-color-scheme` is present on `<html>`**

```bash
curl -s http://localhost:3000/ | grep -o 'data-mantine-color-scheme="[^"]*"' | head -1
```

Expected: the attribute is present (confirms `ColorSchemeScript` and
`mantineHtmlProps` are wired correctly; exact value depends on the
`defaultColorScheme="auto"` resolution at SSR time).

- [ ] **Step 4: Full workspace verification**

Run: `cargo test --workspace && npm --prefix frontend test && npm --prefix frontend run build`
Expected: everything passes — final gate before considering the feature
done. (`cargo test --workspace` is included for parity with this
project's other plans' final gates, even though this feature touches no
Rust code — confirms nothing else regressed.)

- [ ] **Step 5: Bring the stack down**

```bash
docker compose --profile prod down
```
