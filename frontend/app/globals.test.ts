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

describe('TextLink underline affordance', () => {
  // `textDecoration: 'none'` left these links distinguished from the text
  // around them by colour alone (WCAG 1.4.1). The rules live here rather
  // than inline because `:hover`/`:focus-visible` can't be expressed as a
  // style object; `TextLink` opts in via `data-text-link`.
  it('underlines every TextLink on hover and on keyboard focus', () => {
    const rule = css.match(/a\[data-text-link\]:hover,\s*a\[data-text-link\]:focus-visible\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('text-decoration: underline');
  });

  it('underlines always-on TextLinks unconditionally', () => {
    const rule = css.match(/a\[data-text-link=['"]always['"]\]\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('text-decoration: underline');
  });

  it('draws the underline in the link colour rather than the inherited text colour', () => {
    // The `<a>` itself has no `color`; the colour lives on the `Text`
    // inside it, and a decoration is painted in the *decorating* element's
    // colour, so without this the underline would come out body-black.
    expect(css).toContain('text-decoration-color: var(--mantine-color-anchor)');
  });
});

describe('status badge truncation opt-out', () => {
  // Mantine's Badge root carries `overflow: hidden` + `text-overflow:
  // ellipsis`, which clipped "Good Service" to "G…" in the All Lines table
  // at 390px — colour alone then carried the status (WCAG 1.4.1). It also
  // collapses the badge's min-content contribution to zero, which is what
  // let a flex row squeeze the badge past its own width and paint it over
  // the date range on the line detail page.
  it('opts status badges out of overflow clipping, root and label', () => {
    const rule = css.match(/\[data-status-badge\][\s\S]*?\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('overflow: visible');
    expect(rule![0]).toContain('text-overflow: clip');
  });
});

describe('collapsed issue row layout', () => {
  it('lays the row out as a single flex line by default', () => {
    const rule = css.match(/\.issueRow\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('display: flex');
    expect(rule![0]).toContain('justify-content: space-between');
  });

  it('stacks the row into two lines below the sm breakpoint', () => {
    const query = css.match(
      /@media \(max-width: \$mantine-breakpoint-sm\)\s*\{[\s\S]*?\n\}/,
    );
    expect(query).not.toBeNull();
    expect(query![0]).toContain('.issueRow {');
    expect(query![0]).toContain('flex-direction: column');
  });

  it('lets the reason wrap to two clamped lines on mobile instead of truncating to nothing', () => {
    expect(css).toContain('-webkit-line-clamp: 2');
  });

  it('never lets the severity badge shrink out of the row', () => {
    const rule = css.match(/\.issueRow__badge\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('flex-shrink: 0');
  });
});
