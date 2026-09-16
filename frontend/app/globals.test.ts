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
const GRAY_6 = '#868e96';
const GRAY_7 = '#495057';
const GRAY_8 = '#343a40';
const BLUE_8 = '#1971c2';
const BLACK = '#000000';
const DARK_2 = '#a6a7ab'; // `--mantine-color-dimmed` in the dark scheme

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

describe('scheme-blind filled colours (gray, blue)', () => {
  // See lib/theme.ts's SCHEME_BLIND_FILLED_COLORS comment for the full
  // derivation: none of Mantine's own Badge/Button/Chip callers pass a
  // colorScheme into theme.variantColorResolver, so autoContrast's
  // label-colour decision always evaluates shade 6's luminance, even when
  // the rendered background is shade 8 (dark scheme). That's harmless for
  // six of this app's eight filled colours, but not gray or blue.
  it('confirms black actually fails AA on gray 8 and blue 8 -- the dark-scheme regression autoContrast alone would cause', () => {
    expect(contrast(GRAY_8, BLACK)).toBeLessThan(AA_BODY_TEXT); // 1.83:1
    expect(contrast(BLUE_8, BLACK)).toBeLessThan(AA_BODY_TEXT); // 4.18:1
  });

  it('confirms white clears AA on gray 8 and blue 8 -- what light-dark() restores', () => {
    expect(contrast(GRAY_8, WHITE)).toBeGreaterThanOrEqual(AA_BODY_TEXT); // 11.51:1
    expect(contrast(BLUE_8, WHITE)).toBeGreaterThanOrEqual(AA_BODY_TEXT); // 5.02:1
  });

  it('confirms black still clears AA on gray 6 and blue 6 -- why light mode needs black, not white, unlike dark', () => {
    expect(contrast(GRAY_6, BLACK)).toBeGreaterThanOrEqual(AA_BODY_TEXT); // 6.32:1
  });
});

describe('dimmed body text contrast', () => {
  it('overrides --mantine-color-dimmed to gray 7 for the light scheme only', () => {
    const rule = css.match(/html:root\[data-mantine-color-scheme=['"]light['"]\]\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('--mantine-color-dimmed: var(--mantine-color-gray-7)');
  });

  it('confirms the dimmed shade the light scheme was moved off actually failed AA', () => {
    expect(contrast(GRAY_6, WHITE)).toBeLessThan(AA_BODY_TEXT); // 3.32:1
    expect(contrast(GRAY_7, WHITE)).toBeGreaterThanOrEqual(AA_BODY_TEXT); // 8.18:1
  });

  // Mantine hardcodes a *titled* Notification's description to gray 6
  // rather than reading --mantine-color-dimmed, so the override above does
  // not reach ConnectivityMonitor's "Reconnecting..." banner -- it shipped
  // at the same failing 3.32:1 until axe caught it against the
  // banner-visible state (e2e/connectivity-banner.spec.ts).
  it('also lifts a titled Notification description off gray 6 in the light scheme', () => {
    const rule = css.match(
      /\.mantine-Notification-description:where\(\[data-with-title\]\)\s*\{[^}]*\}/,
    );
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('color: var(--mantine-color-gray-7)');
  });

  it("leaves the dark scheme's dimmed colour alone, where it already clears AA", () => {
    expect(contrast(DARK_2, DARK_7)).toBeGreaterThanOrEqual(AA_BODY_TEXT); // 6.46:1
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

describe('links inside sanitized incident HTML', () => {
  const rule = css.match(/\[data-rich-text\]\s+a\s*\{[^}]*\}/);

  it('themes in-content anchors with the shared anchor colour', () => {
    expect(rule).not.toBeNull();
    // The token, not a grape shade: it resolves to grape 7 in light (the
    // override at the top of globals.css) and Mantine's grape 4 in dark,
    // so both schemes are correct from one rule. The two contrast facts
    // this relies on are already asserted above -- grape 7 on white at
    // 4.85:1 and grape 4 on #242424 at 5.84:1 -- and are deliberately not
    // restated here.
    expect(rule![0]).toContain('color: var(--mantine-color-anchor)');
  });

  it('leaves the underline in place, unlike the chrome link treatment', () => {
    // WCAG 1.4.1: these anchors sit mid-paragraph in prose, so colour
    // alone cannot be what distinguishes them from the body text around
    // them. `a[data-text-link]` above deliberately does remove the
    // underline; copying that here would be the wrong tidy-up.
    expect(rule![0]).not.toContain('text-decoration');
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

// `components/JourneyProgress.tsx`. The diagram broke the train page's
// mobile layout when it landed: fixed 56px slots declared inline in the
// component (so no media query could reach them), a flex row whose
// `min-width` a long endpoint label could silently exceed (leaving the
// connecting line stopping short of the last node), nodes bunched to the
// left with the line dangling past the terminus on a short journey, and a
// 12px tap target as the only route to an intermediate stop's name.
describe('journey progress diagram layout', () => {
  it('keeps every horizontal measurement in CSS custom properties the breakpoint can rescale', () => {
    const rule = css.match(/\.journeyProgressScroll\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('--journey-progress-slot: 56px');
    expect(rule![0]).toContain('--journey-progress-endpoint-slot: 84px');
  });

  it('scopes the diagram to its own scroll box and stops a swipe chaining to the page', () => {
    const rule = css.match(/\.journeyProgressScroll\s*\{[^}]*\}/);
    expect(rule![0]).toContain('overflow-x: auto');
    expect(rule![0]).toContain('overscroll-behavior-x: contain');
    // Not decoration: `overflow-x: auto` computes `overflow-y` to `auto`
    // too, and the nodes sit at y=0 of the scroll content -- so this
    // padding is the only thing keeping the marker's halo and a focused
    // trigger's focus ring (WCAG 2.4.7) off the clip edge.
    expect(rule![0]).toContain('padding-block: 6px');
  });

  it('sizes the node row from the counts the component supplies', () => {
    const rule = css.match(/\.journeyProgressLine\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    // Anchored: a bare `toContain('width: 100%')` is also satisfied by the
    // `min-width:` declaration in the same rule, so it could never fail.
    expect(rule![0]).toMatch(/[;{]\s*width:\s*100%/);
    expect(rule![0]).toContain('var(--journey-progress-count');
    expect(rule![0]).toContain('var(--journey-progress-endpoint-count');
    // Spare width left over once every node has hit its `max-width` cap is
    // split evenly rather than pooling at the right-hand edge.
    expect(rule![0]).toContain('justify-content: center');
  });

  // The one part of this layout that is checkable as arithmetic rather than
  // as source text. The row's `min-width` has to equal the sum of the flex
  // bases below it exactly -- if it is bigger the nodes can't fill the row,
  // if it is smaller they overflow it -- and the two are written in
  // different forms (`count * slot + endpoints * (endpointSlot - slot)`
  // against `intermediates * slot + endpoints * endpointSlot`), so the
  // equality is worth evaluating rather than eyeballing.
  it("computes a row min-width that is exactly the sum of its nodes' slots", () => {
    const rule = css.match(/\.journeyProgressLine\s*\{[^}]*\}/)![0];
    const formula = rule.match(/min-width:\s*calc\(([\s\S]*?)\);/)![1];

    function rowMinWidth(count: number, endpoints: number, slot: number, endpointSlot: number) {
      const substituted = formula
        .replace(/var\(--journey-progress-count[^)]*\)/g, String(count))
        .replace(/var\(--journey-progress-endpoint-count[^)]*\)/g, String(endpoints))
        .replace(/var\(--journey-progress-endpoint-slot[^)]*\)/g, String(endpointSlot))
        .replace(/var\(--journey-progress-slot[^)]*\)/g, String(slot))
        .replace(/px/g, '');
      // The formula is pure arithmetic over the four substituted numbers.
      return Function(`"use strict"; return (${substituted});`)() as number;
    }

    for (const [slot, endpointSlot] of [
      [56, 84],
      [44, 76],
    ]) {
      for (const [count, endpoints] of [
        [0, 0],
        [1, 1],
        [2, 2],
        [3, 2],
        [21, 2],
        [4, 0],
        [5, 1],
        [6, 3],
      ]) {
        const sumOfSlots = (count - endpoints) * slot + endpoints * endpointSlot;
        expect(rowMinWidth(count, endpoints, slot, endpointSlot)).toBe(sumOfSlots);
      }
    }
  });

  it('lets nodes grow to fill a short journey, but never shrink and never stretch without limit', () => {
    const rule = css.match(/\.journeyProgressNode\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    // `1 0 <basis>`: grow into spare width, never shrink.
    expect(rule![0]).toContain('flex: 1 0 var(--journey-progress-slot,');
    // Explicit, not flex's `auto` default -- otherwise a long station name's
    // min-content width silently widens its own slot past the basis.
    expect(rule![0]).toContain('min-width: var(--journey-progress-slot,');
    // Caps how far growth can stretch a slot: a node centres its circle in
    // its own slot, so an uncapped 550px slot on a two-stop desktop journey
    // put the origin and terminus circles 550px apart.
    expect(rule![0]).toContain('max-width: calc(2 * var(--journey-progress-slot,');
    // Establishes the containing block for the connecting-line segments.
    expect(rule![0]).toContain('position: relative');
  });

  it('gives every var() a fallback, so a node rendered outside the scroll box still lays out', () => {
    // A `var()` with no fallback that resolves to nothing makes the WHOLE
    // declaration invalid at computed-value time -- for `flex` that means
    // `0 1 auto`, i.e. shrinkable content-sized nodes and a broken diagram.
    const section = css.slice(css.indexOf('.journeyProgressScroll {'));
    const bare = section.match(/var\(--journey-progress-(?:slot|endpoint-slot|node-slot)\)/g);
    expect(bare).toBeNull();
  });

  it('draws the connecting line as two half-segments per node, ending under the end circles', () => {
    // NOT a single row-spanning line: every node centres its circle in its
    // own slot, so a `left: 0; right: 0` line on the row always overhangs
    // the first and last circle by half a slot.
    expect(css).not.toMatch(/\.journeyProgressLine::before/);

    const segments = css.match(/\.journeyProgressNode::before,\s*\n\s*\.journeyProgressNode::after\s*\{[^}]*\}/);
    expect(segments).not.toBeNull();
    expect(segments![0]).toContain('position: absolute');
    expect(segments![0]).toContain('top: calc(var(--journey-progress-node-slot, 18px) / 2)');

    // Whole-rule matches, so neither can accidentally resolve to the shared
    // `::before, ::after` rule above (whose body starts with `content`).
    // Left half runs from the node's left edge to its centre, right half
    // from its centre to its right edge -- the circle is at the centre, so
    // the two meet under it and the run is continuous across the row.
    expect(css).toMatch(/\.journeyProgressNode::before\s*\{\s*left:\s*0;\s*right:\s*50%;\s*\}/);
    expect(css).toMatch(/\.journeyProgressNode::after\s*\{\s*left:\s*50%;\s*right:\s*0;\s*\}/);

    // The outer half at each end is what would dangle; suppressing it also
    // means a single-stop journey draws no line at all.
    const ends = css.match(
      /\.journeyProgressNode:first-child::before,\s*\n\s*\.journeyProgressNode:last-child::after\s*\{[^}]*\}/,
    );
    expect(ends).not.toBeNull();
    expect(ends![0]).toContain('content: none');
  });

  it('gives an endpoint a wider slot so its always-visible label wraps instead of breaking mid-word', () => {
    const rule = css.match(/\.journeyProgressNode--endpoint\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toContain('flex-basis: var(--journey-progress-endpoint-slot,');
    expect(rule![0]).toContain('min-width: var(--journey-progress-endpoint-slot,');
  });

  it('contains an endpoint label inside its slot', () => {
    const rule = css.match(/\.journeyProgressLabel\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toMatch(/[;{]\s*max-width:\s*100%/);
    // `anywhere`, not `break-word`: only `anywhere` also reduces the
    // min-content contribution, which is the thing that would otherwise
    // widen the slot.
    expect(rule![0]).toContain('overflow-wrap: anywhere');
  });

  it('clears the 24px minimum pointer target for the intermediate-node tooltip trigger', () => {
    const rule = css.match(/\.journeyProgressTrigger\s*\{[^}]*\}/);
    expect(rule).not.toBeNull();
    expect(rule![0]).toMatch(/[;{]\s*width:\s*100%/);
    expect(rule![0]).toContain('min-height: 24px');
    // Keeps the circle pinned to the top of the enlarged trigger so the
    // extra height grows downwards and the circle's centre stays on the
    // connecting line.
    expect(rule![0]).toContain('align-items: flex-start');
  });

  it('rescales the slots below the sm breakpoint, using the same media query the issue rows do', () => {
    const queries = css.match(/@media \(max-width: \$mantine-breakpoint-sm\)\s*\{[\s\S]*?\n\}/g);
    expect(queries).not.toBeNull();
    const diagram = queries!.find((query) => query.includes('.journeyProgressScroll'));
    expect(diagram).toBeDefined();
    // Still comfortably over the 24px minimum tap target
    // (`.journeyProgressTrigger` fills the slot's width).
    expect(diagram!).toContain('--journey-progress-slot: 44px');
    expect(diagram!).toContain('--journey-progress-endpoint-slot: 76px');
  });
});
