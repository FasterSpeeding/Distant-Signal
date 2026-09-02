import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { theme } from '@/lib/theme';

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
  it('confirms the shade the light scheme (links, and now filled grape surfaces too) was moved off actually failed AA', () => {
    expect(contrast(GRAPE_6, WHITE)).toBeLessThan(AA_BODY_TEXT);
  });

  it('clears AA for body text -- and, via lib/theme.ts\'s variantColorResolver, for filled surfaces -- in the light scheme', () => {
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

describe('filled-surface contrast under autoContrast', () => {
  // Mantine's shipped palette (node_modules/@mantine/core/.../default-colors.ts)
  // for the eight colours this app renders filled, at shades 6/7/8 -- 6 is
  // where `filled` resolves in the light scheme, 8 in the dark scheme
  // (`primaryShade: { light: 6, dark: 8 }`, Mantine's own default), and 7
  // is the shade the light scheme substitutes in for grape specifically
  // (see app/globals.css and lib/theme.ts's variantColorResolver). All
  // three shades are checked for every colour, not just the ones actually
  // used that way today, because the point of a derived threshold (see
  // lib/theme.ts's comment) is that it's correct for colours this table
  // doesn't even need to enumerate -- 24 of 24 below is the same
  // exhaustive check done during planning, kept as a standing regression
  // net rather than a one-off calculation.
  const PALETTE: [name: string, shade: 6 | 7 | 8, hex: string][] = [
    ['gray', 6, '#868e96'],
    ['gray', 7, '#495057'],
    ['gray', 8, '#343a40'],
    ['red', 6, '#fa5252'],
    ['red', 7, '#f03e3e'],
    ['red', 8, '#e03131'],
    ['green', 6, '#40c057'],
    ['green', 7, '#37b24d'],
    ['green', 8, '#2f9e44'],
    ['blue', 6, '#228be6'],
    ['blue', 7, '#1c7ed6'],
    ['blue', 8, '#1971c2'],
    ['yellow', 6, '#fab005'],
    ['yellow', 7, '#f59f00'],
    ['yellow', 8, '#f08c00'],
    ['orange', 6, '#fd7e14'],
    ['orange', 7, '#f76707'],
    ['orange', 8, '#e8590c'],
    ['grape', 6, '#be4bdb'],
    ['grape', 7, '#ae3ec9'],
    ['grape', 8, '#9c36b5'],
    ['teal', 6, '#12b886'],
    ['teal', 7, '#0ca678'],
    ['teal', 8, '#099268'],
  ];

  // Mirrors Mantine's own autoContrast rule: black text when the
  // background's relative luminance exceeds theme.luminanceThreshold,
  // white otherwise. Kept as an independent reimplementation rather than
  // importing Mantine's -- the point is to check the THRESHOLD choice, and
  // a test that borrowed Mantine's implementation could only ever agree
  // with itself.
  function autoContrastText(bg: string, threshold: number): string {
    return luminance(bg) > threshold ? '#000000' : '#ffffff';
  }

  it.each(PALETTE)(
    '%s-%i clears AA for body text with the autoContrast-chosen label colour',
    (name, shade, hex) => {
      expect(contrast(hex, autoContrastText(hex, theme.luminanceThreshold!))).toBeGreaterThanOrEqual(AA_BODY_TEXT);
    },
  );

  it('keeps the threshold inside the window where BOTH branches clear AA', () => {
    // The real guarantee: any threshold in [0.1750, 0.1833] is AA-correct
    // for every possible background, so this asserts the derivation, not
    // just today's palette. See lib/theme.ts for the algebra.
    expect(theme.luminanceThreshold!).toBeGreaterThanOrEqual(4.5 * 0.05 - 0.05);
    expect(theme.luminanceThreshold!).toBeLessThanOrEqual(1.05 / 4.5 - 0.05);
  });

  it('pins filled grape to white text on the grape 7 the light scheme substitutes', () => {
    expect(contrast(GRAPE_7, WHITE)).toBeGreaterThanOrEqual(AA_BODY_TEXT);
    // The failure this guards: black on grape 7 is 4.33:1, which is what
    // autoContrast would choose unaided -- see lib/theme.ts's resolver.
    expect(contrast(GRAPE_7, '#000000')).toBeLessThan(AA_BODY_TEXT);
  });

  it("redirects the light scheme's grape filled background and hover to grape 7/8", () => {
    const rule = css.match(/html:root\[data-mantine-color-scheme=['"]light['"]\]\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('--mantine-color-grape-filled: var(--mantine-color-grape-7)');
    expect(rule![0]).toContain('--mantine-color-grape-filled-hover: var(--mantine-color-grape-8)');
    expect(rule![0]).toContain('--mantine-primary-color-contrast: var(--mantine-color-white)');
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

describe('background theming', () => {
  // The base wash: a very low-opacity brand-tinted gradient from the top of
  // the page. Asserts it's driven entirely by CSS custom properties
  // (`--mantine-color-grape-6`, `--mantine-color-text`), never a hardcoded
  // hex — which is what keeps it categorically unable to collide with
  // `lib/severity.ts`'s `GROUP_COLOR` hexes (the non-goal this whole file's
  // link-colour section above also has to respect).
  it('washes the body in a low-opacity, variable-driven gradient rather than a fixed colour', () => {
    // `body(?!\[)`: matches the base `body { ... }` rule but not
    // `body[data-pride='rainbow'] { ... }`/`body[data-pride='trans'] { ... }`,
    // which follow immediately after in the file and have their own
    // assertions below.
    const rule = css.match(/body(?!\[)\s*\{\s*background-image:[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('color-mix(in srgb, var(--mantine-color-grape-6)');
    expect(rule![0]).not.toMatch(/#[0-9a-f]{3,8}/i);
    // Single-digit percentage: this is meant to be barely perceptible, not
    // a colour statement in its own right.
    expect(rule![0]).toMatch(/color-mix\(in srgb, var\(--mantine-color-grape-6\) \d%/);
  });

  it("overrides the wash under rainbow pride mode with the same seven hexes the flag bars use, still at low opacity", () => {
    const barRule = css.match(/body\[data-pride='rainbow'\]::before\s*\{[^}]*background:[^;]*;/);
    const washRule = css.match(/body\[data-pride='rainbow'\]\s*\{\s*background-image:[^}]*\}/);
    expect(barRule).not.toBeNull();
    expect(washRule).not.toBeNull();

    const hexes = barRule![0].match(/#[0-9a-f]{6}/gi)!;
    expect(hexes.length).toBeGreaterThan(0);
    for (const hex of hexes) {
      expect(washRule![0].toLowerCase()).toContain(`color-mix(in srgb, ${hex.toLowerCase()}`);
    }
    expect(washRule![0]).toMatch(/\d%, transparent\)/);
  });

  it("overrides the wash under trans pride mode with the same hexes the flag bars use, still at low opacity", () => {
    const barRule = css.match(/body\[data-pride='trans'\]::before\s*\{[^}]*background:[^;]*;/);
    const washRule = css.match(/body\[data-pride='trans'\]\s*\{\s*background-image:[^}]*\}/);
    expect(barRule).not.toBeNull();
    expect(washRule).not.toBeNull();

    const hexes = [...new Set(barRule![0].match(/#[0-9a-f]{6}/gi)!.map((h) => h.toLowerCase()))];
    expect(hexes.length).toBeGreaterThan(0);
    for (const hex of hexes) {
      expect(washRule![0].toLowerCase()).toContain(`color-mix(in srgb, ${hex}`);
    }
    expect(washRule![0]).toMatch(/\d%, transparent\)/);
  });

  // The same wash/bar-hex-parity contract as rainbow/trans above, extended
  // to the six modes PrideToggle grew afterwards (nonbinary, bisexual,
  // pansexual, asexual, sapphic, lesbian) -- table-driven since it's the
  // exact same assertion shape repeated per mode rather than six
  // hand-written copies.
  it.each(['nonbinary', 'bisexual', 'pansexual', 'asexual', 'sapphic', 'lesbian'])(
    "overrides the wash under %s pride mode with the same hexes the flag bar uses, still at low opacity",
    (mode) => {
      const barRule = css.match(new RegExp(`body\\[data-pride='${mode}'\\]::before\\s*\\{[^}]*background:[^;]*;`));
      const washRule = css.match(new RegExp(`body\\[data-pride='${mode}'\\]\\s*\\{\\s*background-image:[^}]*\\}`));
      expect(barRule).not.toBeNull();
      expect(washRule).not.toBeNull();

      const hexes = [...new Set(barRule![0].match(/#[0-9a-f]{6}/gi)!.map((h) => h.toLowerCase()))];
      expect(hexes.length).toBeGreaterThan(0);
      for (const hex of hexes) {
        expect(washRule![0].toLowerCase()).toContain(`color-mix(in srgb, ${hex}`);
      }
      expect(washRule![0]).toMatch(/\d%, transparent\)/);
    },
  );

  it('gives nav an unconditional positioning context, not one scoped to pride mode', () => {
    // Regression guard: this used to be `body[data-pride='true'] nav { position: relative; }`,
    // the only consumer at the time (back when this toggle was a plain
    // on/off boolean, before it grew a third `'trans'` state). The
    // always-on nav divider below needs it too now, so it must not have
    // stayed pride-only under either mode's selector.
    expect(css).not.toMatch(/body\[data-pride='rainbow'\]\s*nav\s*\{\s*position:\s*relative;\s*\}/);
    expect(css).not.toMatch(/body\[data-pride='trans'\]\s*nav\s*\{\s*position:\s*relative;\s*\}/);
    const rule = css.match(/\bnav\s*\{\s*position:\s*relative;\s*\}/);
    expect(rule).not.toBeNull();
  });

  it('keeps the always-on dashed nav divider clear of the pride bar band so the two never overlap', () => {
    const divider = css.match(/nav::before\s*\{[^}]*\}/);
    const prideBar = css.match(/body\[data-pride='rainbow'\]\s*nav::after\s*\{[^}]*\}/);
    expect(divider).not.toBeNull();
    expect(prideBar).not.toBeNull();

    // The divider sits inside the nav box (positive offset from the
    // bottom); the pride bar sits outside it (negative). Different bands,
    // so pride mode layers a second effect rather than fighting this one.
    expect(divider![0]).toMatch(/bottom:\s*2px/);
    expect(prideBar![0]).toMatch(/bottom:\s*-3px/);
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
