import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { MantineProvider, DEFAULT_THEME, mergeThemeOverrides, type MantineTheme } from '@mantine/core';
import { theme } from './theme';

// The real merged theme the provider actually resolves `variantColorResolver`
// against (DEFAULT_THEME with this app's overrides layered on), rather than a
// hand-rolled stand-in that could quietly diverge from it. `mergeThemeOverrides`
// is typed to return `MantineThemeOverride` (every field optional, since it's
// also used to merge a *partial* override) rather than `MantineTheme` -- the
// cast is honest, not a workaround: DEFAULT_THEME is itself a complete
// MantineTheme, and merging overrides onto a complete theme can't drop
// required fields, only widen some of their types in the merge helper's own
// signature.
const mergedTheme = mergeThemeOverrides(DEFAULT_THEME, theme) as MantineTheme;

// Per the grape-theme spec's Testing section: colour is largely not
// unit-testable and shouldn't be asserted shade by shade (that's a job for
// the visual/contrast verification pass, not this suite). What IS worth
// locking down here is that the app's real `MantineProvider` actually
// *receives* this theme — asserting `theme.primaryColor === 'grape'`
// against `createTheme({ primaryColor: 'grape' })` in the same file proved
// nothing (it can't fail short of someone deliberately editing the value
// right next to the assertion). Rendering under the real provider and
// reading back the CSS variable it emits catches the regression that
// actually matters: a `MantineProvider` that silently lost its `theme`
// prop (e.g. via a bad refactor of a `renderWithMantine`-style helper).
//
// `--mantine-color-anchor` only resolves (`getMergedVariables` /
// `removeDefaultVariables`) when it differs from Mantine's built-in
// default theme, so this also only works because grape isn't the default
// primary colour — see the provider-loses-its-theme case below, where the
// variable comes back unset for exactly that reason.
describe('theme', () => {
  it('is threaded through to the rendered MantineProvider', () => {
    render(
      <MantineProvider theme={theme}>
        <div>probe</div>
      </MantineProvider>,
    );
    // Mantine's `MantineCssVariables` writes a `<style>` block targeting
    // `:root`, so — unlike ordinary inherited custom properties, which
    // jsdom's computed-style implementation doesn't cascade to descendant
    // elements — this only resolves on `document.documentElement` itself.
    const anchorColor = getComputedStyle(document.documentElement).getPropertyValue('--mantine-color-anchor');
    expect(anchorColor).toBe('var(--mantine-color-grape-6)');
  });

  it('resolves a different anchor colour when the provider has no theme at all', () => {
    // Sanity check that the assertion above is actually discriminating: a
    // provider that never got `theme={theme}` renders under Mantine's
    // default (blue) theme instead, so a bug that drops the prop would
    // fail the test above rather than passing it vacuously.
    render(
      <MantineProvider>
        <div>probe</div>
      </MantineProvider>,
    );
    const anchorColor = getComputedStyle(document.documentElement).getPropertyValue('--mantine-color-anchor');
    expect(anchorColor).not.toBe('var(--mantine-color-grape-6)');
  });

  it('sets autoContrast and the AA-derived luminance threshold', () => {
    expect(theme.autoContrast).toBe(true);
    expect(theme.luminanceThreshold).toBe(0.179);
  });

  it("resolves filled grape to white text rather than autoContrast's black", () => {
    // Calls the resolver directly -- reading the rendered colour back out of
    // jsdom isn't reliable for Mantine's CSS variables (see this file's
    // existing note about :root-scoped custom properties).
    const resolved = theme.variantColorResolver!({ color: 'grape', variant: 'filled', theme: mergedTheme });
    expect(resolved.color).toBe('var(--mantine-color-white)');
  });

  it.each(['green', 'yellow', 'orange'])('leaves filled %s to autoContrast', (color) => {
    const resolved = theme.variantColorResolver!({ color, variant: 'filled', theme: mergedTheme });
    expect(resolved.color).not.toBe('var(--mantine-color-white)');
    expect(resolved.color).not.toContain('light-dark');
    expect(resolved.color).not.toContain('--ds-');
  });

  // The destructive confirm button (`ConfirmActionButton`): white on red 9
  // in both schemes, from the tokens in app/globals.css, rather than
  // autoContrast's black on red 6/8. app/globals.test.ts measures them.
  it('sends filled red to the --ds-filled-red-* tokens', () => {
    const resolved = theme.variantColorResolver!({ color: 'red', variant: 'filled', theme: mergedTheme });
    expect(resolved.background).toBe('var(--ds-filled-red-fill)');
    expect(resolved.hover).toBe('var(--ds-filled-red-fill-hover)');
    expect(resolved.color).toBe('var(--ds-filled-red-text)');
  });

  it("leaves non-filled red on Mantine's own variables", () => {
    for (const variant of ['light', 'outline', 'subtle']) {
      const resolved = theme.variantColorResolver!({ color: 'red', variant, theme: mergedTheme });
      expect(resolved.background).not.toContain('--ds-filled-red');
      expect(resolved.color).not.toContain('--ds-filled-red');
    }
  });

  // Regression coverage for a real bug found during this plan's Task 6
  // visual pass: none of Badge/Button/Chip's calls into
  // theme.variantColorResolver (Mantine's own components) pass a
  // colorScheme field, so autoContrast's label-colour decision always
  // evaluates shade 6's luminance -- even when the rendered background is
  // shade 8 (dark scheme). For gray/blue that means autoContrast always
  // picks black (correct for shade 6, needed for the light scheme), but
  // black on their actual shade-8 dark-mode background is 1.83:1 (gray) /
  // 4.18:1 (blue) -- both fail AA, regressing an already-passing dark-mode
  // pairing (white was 11.51:1 / 5.02:1). See lib/theme.ts's
  // SCHEME_BLIND_FILLED_COLORS comment for the full derivation.
  // Pink and indigo: black on shade 8 is 3.71:1 for both.
  it.each(['gray', 'blue', 'pink', 'indigo'])(
    "pins filled %s to a scheme-aware light-dark() rather than autoContrast's scheme-blind black",
    (color) => {
      const resolved = theme.variantColorResolver!({ color, variant: 'filled', theme: mergedTheme });
      expect(resolved.color).toBe('light-dark(var(--mantine-color-black), var(--mantine-color-white))');
    },
  );

  // Mantine's default is hover only, so a keyboard user never opened a
  // tooltip (components/Tooltip.test.tsx renders the effect).
  it('opens every tooltip on focus as well as hover', () => {
    expect(theme.components?.Tooltip?.defaultProps).toEqual({
      events: { hover: true, focus: true, touch: false },
    });
  });
});
