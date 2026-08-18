# Grape Theme — Design

Adopt Mantine's `grape` as the app's brand/primary colour, replacing the
implicit Mantine default (`blue`).

Prompted by the 2026-08-18 GUI/UX review, which found the app has no
visual identity of its own — it renders as stock Mantine. Several findings
in that review (unlabelled chips, indistinguishable pin states, ⓘ triggers
that read as broken glyphs) are being fixed separately; this spec covers
only the colour system.

## Goals

- Give the app a deliberate brand colour (`grape`) applied through the
  theme, so it is set in one place rather than sprinkled per-component.
- Keep the **status/severity palette semantic and unchanged**. Rail status
  colour is information, not decoration.
- Leave the app fully legible in both light and dark appearance.

## Non-goals

- No change to `lib/severity.ts`'s `GROUP_COLOR` map. Green/gray/blue/
  yellow/red carry meaning users already read at a glance, and the review
  singled the status badges out as one of the things that works.
- No custom colour tuples, no hand-mixed shades. Use Mantine's shipped
  `grape` scale as-is; a bespoke palette is a much larger commitment
  (contrast tuning across 10 shades × 2 appearances) for no current need.
- No typography, spacing, or radius changes. Colour only.
- No dark-mode redesign. Dark already works (verified on the running app);
  this must not regress it.

## Current state

`app/layout.tsx` renders `<MantineProvider defaultColorScheme="auto">`
with **no `theme` prop at all**. Every colour in the app is therefore
either a Mantine default (primary = `blue`) or hardcoded at the call site.

Two consequences matter here:

1. There is no theme object to extend — this spec creates the first one.
2. Six call sites hardcode `c="blue"` rather than referencing the primary
   colour:

   | File | Line | Element |
   |---|---|---|
   | `app/layout.tsx` | 61 | "All Lines" nav link |
   | `app/layout.tsx` | 64 | "Station Lookup" nav link |
   | `app/page.tsx` | 47 | "Browse all lines" |
   | `app/page.tsx` | 67 | "Look up a station" |
   | `app/lines/page.tsx` | 43 | line name in the All Lines table |
   | `app/lines/[id]/page.tsx` | 86 | "View history" |

## The collision this must avoid

`GROUP_COLOR.planned` is **`blue`** (`lib/severity.ts`). Today that is
invisible as a problem, because primary is also blue — everything blue,
links and planned-status alike.

Setting `primaryColor: 'grape'` **without** fixing the six hardcoded
`c="blue"` call sites produces the worst of both worlds:

- Buttons, active chips, and focus rings turn grape (theme-driven).
- Every link stays blue (hardcoded).
- Blue now means *only* "planned closure" in the status system — except
  it also still means "this is a link", in the same viewport, at the same
  time. On `/lines`, a blue line name sits in a table beside blue
  `PLANNED` badges.

**Requirement: the six call sites above must be converted in the same
change that sets the primary colour.** They should follow the theme
(`c="primary"` or the equivalent Mantine token) rather than naming a
colour, so the next palette change is a one-line edit.

After that conversion, blue belongs exclusively to `planned` and the
semantic split is cleaner than it is today.

## Design

### Theme object

Introduce a theme module (suggested: `frontend/lib/theme.ts`) exporting a
`createTheme({ primaryColor: 'grape' })`, and pass it to `MantineProvider`
in `app/layout.tsx`.

It goes in its own module rather than inline in the layout because
`layout.tsx` is a Server Component and the theme is shared with
`vitest.setup.ts` — see Testing below.

### Primary shade

Mantine's default `primaryShade` is `{ light: 6, dark: 8 }`. Grape shade 6
is `#be4bdb`; shade 8 is `#9c36b5`.

Keep the default unless contrast testing (below) says otherwise. If white
text on grape 6 fails for small text, prefer raising the light shade to 7
over introducing a custom tuple.

### What must NOT become grape

- **Status badges.** `StatusBadge` passes `severityColor(...)` explicitly,
  so it is already immune to the primary change. Verify, don't assume.
- **Destructive actions.** The delete button and its confirm modal are red
  and must stay red — the review called this flow out as correctly
  cautious.
- **Anything conveying data.** If a future chart or meter needs colour,
  it does not inherit brand grape by default.

## Risks

**Grape vs. red at badge size, for red-green colour blindness.** Grape is
magenta-leaning purple. Under protanopia/deuteranopia, purple and red
converge more than blue and red do. Today's `severe` red badges sit
beside blue links; after this change they sit beside grape UI. Since the
severity badges always carry a **text label** ("Severe Delays", "Minor
Delays"), status is never conveyed by colour alone and this is a
degradation in scanning speed, not a loss of information. Accept, but
check it deliberately rather than discovering it later.

**Contrast in dark mode.** Grape 8 (`#9c36b5`) as a filled background with
white text is darker than blue 8; confirm it still clears 4.5:1. The
review also flagged existing contrast concerns (dimmed grey body text,
white-on-amber badges) as *unmeasured* — this is a good moment to run an
automated pass rather than trust eyeballs on either.

## Testing

`vitest.setup.ts` already wraps components in a `MantineProvider` (it
appears in the grep for `MantineProvider`). It must be updated to pass the
same theme object, or every component test renders under a different
palette than production — the class of bug where tests pass and the app
looks wrong.

Colour is largely not unit-testable and should not be asserted shade by
shade. Worth one test that the provider receives the theme with
`primaryColor: 'grape'`; beyond that, verify visually.

## Verification

Re-shoot the screenshot set from the 2026-08-18 review (the driver script
and manifest are in that session's scratchpad; `playwright-core` against
the bundled Chromium at `~/.cache/ms-playwright/chromium-1228/`, since the
Playwright MCP server is pinned to a system Chrome that isn't installed
here). Compare before/after on:

- `/lines` — the blue-link vs blue-`PLANNED`-badge collision described
  above is most visible here.
- `/lines/[id]` — status badge, source chips, and "View history" link
  together.
- Home in both light and dark.

Run an automated contrast check (axe or similar) rather than judging by
eye, and record the numbers.

## Open question

Whether `planned` should stay blue at all once blue is no longer the
brand colour. Keeping it is the smaller change and is assumed here. But
`planned` is arguably the odd member of the severity scale — it is a
schedule fact, not a severity — and the review separately noted that a
line whose only issues are planned currently presents as disrupted. If
that lands differently, revisit the whole `GROUP_COLOR` map as its own
piece of work, not as a rider on this one.
