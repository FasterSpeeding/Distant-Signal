import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';

// Vitest runs with `frontend/` as its root (see vitest.config.ts).
const css = readFileSync('app/globals.css', 'utf8');

// Mantine's palette, read off `@mantine/core/styles.css`. Only the shades
// these assertions actually name are listed.
const GRAPE_4 = '#da77f2';
const GRAPE_6 = '#be4bdb';
const GRAPE_7 = '#ae3ec9';
const WHITE = '#ffffff';
const DARK_7 = '#242424'; // `--mantine-color-body` in the dark scheme

// WCAG 2.1 relative luminance and contrast ratio. Colour can't usefully be
// asserted shade by shade in a unit test, but "does this pair clear AA for
// body text" is a pure function of two hex values, so it can be.
function luminance(hex: string): number {
  const channels = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const [r, g, b] = channels.map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const AA_BODY_TEXT = 4.5;

describe('link colour', () => {
  it('confirms the shade the light scheme was moved off actually failed AA', () => {
    expect(contrast(GRAPE_6, WHITE)).toBeLessThan(AA_BODY_TEXT);
  });

  it('clears AA for body text in the light scheme', () => {
    expect(contrast(GRAPE_7, WHITE)).toBeGreaterThanOrEqual(AA_BODY_TEXT);
  });

  it('overrides --mantine-color-anchor to grape 7 for the light scheme only', () => {
    // Mantine resolves the anchor colour to `primaryColor`-6 in light and
    // -4 in dark; only the light half fails AA, so only the light half is
    // overridden. The selector must outweigh Mantine's own
    // `:root[data-mantine-color-scheme='light']` block, which the provider
    // injects into <body> — later in document order than this stylesheet,
    // so equal specificity would lose.
    const rule = css.match(/html:root\[data-mantine-color-scheme=['"]light['"]\]\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('--mantine-color-anchor: var(--mantine-color-grape-7)');
    expect(css).not.toContain("data-mantine-color-scheme='dark'");
  });

  it('leaves the dark scheme alone, where grape 4 already clears AA', () => {
    expect(contrast(GRAPE_4, DARK_7)).toBeGreaterThanOrEqual(AA_BODY_TEXT);
  });
});
