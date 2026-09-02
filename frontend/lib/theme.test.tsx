import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { MantineProvider, DEFAULT_THEME, mergeThemeOverrides } from '@mantine/core';
import { theme } from './theme';

// The real merged theme the provider actually resolves `variantColorResolver`
// against (DEFAULT_THEME with this app's overrides layered on), rather than a
// hand-rolled stand-in that could quietly diverge from it.
const mergedTheme = mergeThemeOverrides(DEFAULT_THEME, theme);

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
    const anchorColor = getComputedStyle(document.documentElement).getPropertyValue(
      '--mantine-color-anchor',
    );
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
    const anchorColor = getComputedStyle(document.documentElement).getPropertyValue(
      '--mantine-color-anchor',
    );
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

  it('leaves every other colour to autoContrast', () => {
    const resolved = theme.variantColorResolver!({ color: 'red', variant: 'filled', theme: mergedTheme });
    expect(resolved.color).not.toBe('var(--mantine-color-white)');
  });
});
