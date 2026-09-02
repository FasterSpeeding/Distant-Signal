import { createTheme, defaultVariantColorsResolver, type VariantColorsResolver } from '@mantine/core';

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
// not change. That non-goal wins. The scoped `variantColorResolver`
// override (grape-only) this comment used to point to as future work is
// below: `variantColorResolver` pins filled grape's text to white on the
// grape-7 background `app/globals.css` substitutes for the light scheme,
// and `autoContrast`/`luminanceThreshold` below handle every other
// colour's filled-surface contrast instead. Note this is still a *strict
// improvement* over the current default primary (blue 6 vs white is
// 3.56:1) — not a regression.

/** `autoContrast`'s label-colour decision is, empirically, colour-scheme
 * BLIND for any colour rendered without an explicit shade suffix (every
 * `StatusBadge` and the incident-page `Badge`, per Decision 2 of this
 * plan's write-up): none of Badge/Button/Chip/etc.'s calls to
 * `theme.variantColorResolver` in `@mantine/core`'s source
 * (`components/Badge/Badge.cjs` and its siblings) pass a `colorScheme`
 * field, so `defaultVariantColorsResolver`'s own call to
 * `parseThemeColor({ color, theme })` always resolves the shade via
 * `getPrimaryShade(theme, colorScheme || "light")` -- `colorScheme` is
 * `undefined` there, so this is unconditionally `"light"`, i.e. shade 6,
 * *regardless of which scheme is actually active*. The rendered
 * *background* still correctly tracks scheme (it goes through the
 * `--mantine-color-<name>-filled` CSS variable, which Mantine's
 * MantineCssVariables component generates separately per scheme), so the
 * text-colour decision and the background it's painted on can end up
 * computed against two different shades.
 *
 * For six of this app's eight filled colours that's harmless: their
 * shade-6 and shade-8 luminances sit on the *same* side of 0.179, so
 * "decide using shade 6, render on shade 8" still lands on the right
 * answer. Verified exhaustively for gray/red/green/blue/yellow/orange at
 * shade 8 with this exact WCAG formula: gray 8 white 11.51 vs black 1.83,
 * red 8 white 4.51 vs black 4.65, green 8 white 3.45 vs black 6.10, blue 8
 * white 5.02 vs black 4.18, yellow 8 white 2.48 vs black 8.46, orange 8
 * white 3.58 vs black 5.87. Red/green/yellow/orange all agree with the
 * shade-6-derived choice (black) either way, or both options clear AA
 * (red). Gray and blue do NOT: shade 6 is light enough that `autoContrast`
 * always picks black (correct for the shade-6 background light renders),
 * but shade 8 is dark enough that black on it is 1.83:1 (gray) / 4.18:1
 * (blue) -- both fail AA, undoing an already-passing pairing (dark mode
 * used to be unconditionally white before this plan, at 11.51/5.02).
 *
 * `variantColorResolver`'s input carries no `colorScheme` field either
 * (`VariantColorsResolverInput`'s shape, confirmed against
 * `default-variant-colors-resolver.d.ts`), so a resolver branch can't
 * query it directly -- but CSS's own `light-dark()` can: Mantine already
 * sets `:root { color-scheme: var(--mantine-color-scheme) }`
 * (`@mantine/core/styles.css`), so `light-dark(black, white)` resolves per
 * the *actual* active scheme at paint time, sidestepping the JS-side
 * blindness entirely. Confirmed live in both schemes against the Task 6
 * preview route before this fix (dark gray/blue both black, 1.83:1/4.18:1)
 * and after (white, 11.51:1/5.02:1) -- see the plan's Task 6 report. */
const SCHEME_BLIND_FILLED_COLORS = new Set(['gray', 'blue']);

/** Pins filled grape surfaces to white text, overriding `autoContrast`
 * for this one colour.
 *
 * Necessary because the light scheme substitutes grape 7 for grape 6 as
 * the filled background at the CSS layer (`app/globals.css` --
 * `--mantine-color-grape-filled`, the same grape-7 the anchor colour was
 * moved to in the grape-theme spec, so the light scheme has one grape
 * rather than two). `autoContrast` runs in JS against
 * `theme.colors.grape[6]` and cannot see that substitution: unaided it
 * would evaluate grape 6 (L 0.2109, above the 0.179 threshold), choose
 * BLACK, and paint it on a grape-7 background for 4.29:1 -- a fail, and
 * the wrong colour. White on grape 7 is 4.85:1, and white on grape 8
 * (the dark scheme's filled shade, not overridden) is 5.82:1, so pinning
 * white is correct in both schemes.
 *
 * Also pins gray/blue filled surfaces via `light-dark()` -- see
 * `SCHEME_BLIND_FILLED_COLORS` above for why. Every other colour keeps
 * going through plain `autoContrast`, which is what fixes the severity
 * badges and the red delete buttons. */
const variantColorResolver: VariantColorsResolver = (input) => {
  const resolved = defaultVariantColorsResolver(input);
  if (input.color === 'grape' && input.variant === 'filled') {
    return { ...resolved, color: 'var(--mantine-color-white)' };
  }
  if (SCHEME_BLIND_FILLED_COLORS.has(input.color ?? '') && input.variant === 'filled') {
    return { ...resolved, color: 'light-dark(var(--mantine-color-black), var(--mantine-color-white))' };
  }
  return resolved;
};

export const theme = createTheme({
  primaryColor: 'grape',

  // `autoContrast` picks black or white text per filled surface by
  // comparing the background's WCAG relative luminance against
  // `luminanceThreshold`. Mantine's default threshold (0.3) is too high to
  // be safe: it correctly flips yellow and green badges to black text but
  // leaves red (3.28:1), gray (3.32:1), blue (3.56:1) and orange (2.57:1
  // before the flip) on white text, all short of AA's 4.5:1.
  //
  // 0.179 is derived, not tuned. Solving 4.5:1 for each branch:
  //   black text clears AA  <=>  (L + 0.05)/0.05 >= 4.5  <=>  L >= 0.1750
  //   white text clears AA  <=>  1.05/(L + 0.05) >= 4.5  <=>  L <= 0.1833
  // Those windows overlap, so ANY threshold in [0.1750, 0.1833] makes
  // autoContrast provably AA-correct for every possible background --
  // there is no colour it can get wrong, PROVIDED the decision and the
  // rendered background are evaluated against the same shade. 0.179 is the
  // balance point where both branches floor at ~4.58:1. Verified across
  // all eight palette colours this app uses at shades 6/7/8: 24 of 24
  // pass, minimum 4.65:1. See app/globals.test.ts, which asserts this
  // exhaustively.
  //
  // That proviso is not automatic: see SCHEME_BLIND_FILLED_COLORS above
  // for the two colours (gray, blue) where Mantine's own filled-variant
  // callers evaluate this decision against shade 6 unconditionally, even
  // when the rendered background is shade 8 (dark scheme) -- both are
  // pinned via `light-dark()` in variantColorResolver rather than left to
  // autoContrast.
  //
  // This is the "deliberate decision against the Non-goals" that
  // docs/superpowers/specs/2026-08-18-grape-theme-design.md:156-162 parked.
  // The evidence it was waiting for is
  // docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md:
  // white text fails AA on ALL FIVE of GROUP_COLOR's hues, not just the
  // amber one that review guessed at. lib/severity.ts is untouched -- a
  // yellow badge is still yellow, it just has readable text now.
  autoContrast: true,
  luminanceThreshold: 0.179,

  variantColorResolver,
});
