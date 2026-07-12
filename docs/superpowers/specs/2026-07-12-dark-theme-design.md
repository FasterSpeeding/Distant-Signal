# Dark Theme Support — Design

Sub-project 3 of 3 (see also: `2026-07-11-operator-station-autocomplete-design.md`,
`2026-07-12-edit-custom-lines-design.md`, both shipped). Independent of
the other two — smallest of the three.

## Goals

- Let a user switch between light, dark, and system (`prefers-color-scheme`)
  themes, persisted across visits, with a toggle in the site nav.

## Non-goals

- No custom Mantine theme (colors, spacing, fonts) — this is scheme
  switching only, not a redesign. Confirmed during planning: every color
  reference in the app already goes through Mantine theme tokens
  (`c="dimmed"`/`c="blue"`/`color="yellow"`/etc.) or Mantine CSS
  variables (`var(--mantine-color-default-border)`), with zero hardcoded
  hex/named colors anywhere in `frontend/` — so there is no separate
  "audit every component for hardcoded light-mode colors" phase this
  design needs to account for.
- No server-side/backend persistence of the choice — Mantine's default
  `colorSchemeManager` (localStorage-backed) is sufficient for a
  single-user personal instance; no `Preferences`/DB change.

## Implementation

`frontend/app/layout.tsx`: both `<ColorSchemeScript />` (line 15) and
`<MantineProvider>` (line 18) gain `defaultColorScheme="auto"` — they
must match, or the SSR-rendered markup (from `ColorSchemeScript`, which
runs before hydration to set the initial `data-mantine-color-scheme`
attribute and avoid a flash of the wrong theme) can disagree with what
`MantineProvider` initializes to on the client.

New `frontend/components/ThemeToggle.tsx` (Client Component): reads the
current scheme via Mantine's `useMantineColorScheme()` (`colorScheme`:
`'light' | 'dark' | 'auto'`, the raw preference — not resolved) and
`useComputedColorScheme('light')` (the resolved `'light' | 'dark'`,
for icon choice when the preference is `'auto'`). Clicking cycles
`light → dark → auto → light`. Rendered as a Mantine `ActionIcon` with
an `aria-label` stating the current mode, matching this app's existing
icon-button pattern (`PinToggle`'s ★/☆ `ActionIcon`).

Wired into the nav `Group` in `app/layout.tsx`, alongside "All Lines"/
"Station Lookup" — the only existing header chrome, and where a
site-wide control belongs.

## Testing

`frontend/components/ThemeToggle.test.tsx`, following the established
`PinToggle.test.tsx`/`DeleteLineButton.test.tsx` pattern (render inside a
real `<MantineProvider>`, no mocking of Mantine's hooks — they run
against real context/localStorage, which jsdom provides). Covers: initial
icon per starting scheme, and that each click advances to the next
scheme in the light → dark → auto → light cycle.
