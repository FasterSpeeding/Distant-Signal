import { createTheme } from '@mantine/core';

// The app's single brand-colour decision (see
// docs/superpowers/specs/2026-08-18-grape-theme-design.md). Shared between
// `app/layout.tsx` (a Server Component, which can't inline a `createTheme`
// call and keep it identical to what tests render under) and
// `vitest.setup.ts` / component test files, so production and tests never
// diverge on palette.
//
// `primaryShade` is kept at Mantine's default ({ light: 6, dark: 8 })
// rather than raised to 7 as the design spec's "Primary shade" section
// suggests for contrast. Computed via WCAG 2.1 relative luminance, white
// text on grape 6 (#be4bdb) is 4.02:1 — short of the 4.5:1 body-text
// threshold (grape 7, #ae3ec9, would clear it at 4.85:1). But
// `primaryShade` is not scoped to `primaryColor`: Mantine's
// `getPrimaryShade()` feeds the shade-selection formula for every named
// colour's `-filled`/`-outline`/`-text` CSS variables (see
// `get-css-color-variables.mjs`), so raising it here would also shift the
// rendered shade of every `StatusBadge` (green/gray/blue/yellow/red) —
// directly violating this spec's non-goal that the severity palette must
// not change. That non-goal wins; see the theme task report for the
// measured numbers and the recommendation to revisit this as a scoped
// `variantColorResolver` override (grape-only) rather than a global
// `primaryShade` bump, if the 4.02:1 gap needs closing later. Note this is
// still a *strict improvement* over the current default primary (blue 6
// vs white is 3.56:1) — not a regression.
export const theme = createTheme({ primaryColor: 'grape' });
