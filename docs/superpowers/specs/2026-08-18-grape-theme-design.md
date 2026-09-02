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
2. Seven call sites hardcode `c="blue"` rather than referencing the
   primary colour:

   | File | Line | Element |
   |---|---|---|
   | `app/layout.tsx` | 61 | "All Lines" nav link |
   | `app/layout.tsx` | 64 | "Station Lookup" nav link |
   | `app/page.tsx` | 47 | "Browse all lines" |
   | `app/page.tsx` | 67 | "Look up a station" |
   | `app/lines/page.tsx` | 43 | line name in the All Lines table |
   | `app/lines/[id]/page.tsx` | 86 | "View history" |
   | `app/lines/[id]/history/page.tsx` | 32 | "Back to line" |

   Re-grep before starting (`grep -rn 'c="blue"' app components --include=*.tsx`)
   — this list has already grown once, on the same day it was written, when
   the history-page fix added a link. Every new link in this codebase is
   currently born hardcoded, which is the underlying problem this spec fixes.

   **Status: done.** All seven sites were converted in the change that
   introduced `lib/theme.ts`, to `c="var(--mantine-color-anchor)"` (Mantine's
   anchor colour, which follows `theme.primaryColor`). `grep -rn 'c="blue"'
   app components --include=*.tsx` now returns nothing.

## The collision this must avoid

`GROUP_COLOR.planned` is **`blue`** (`lib/severity.ts`) and unchanged by
this spec (see Non-goals). Before this change, that was invisible as a
problem only because primary was *also* blue — links and planned-status
badges were both blue, and nothing distinguished "this is a link" from
"this is a planned closure".

Setting `primaryColor: 'grape'` **without** fixing the seven hardcoded
`c="blue"` call sites would have produced the worst of both worlds:
buttons/chips/focus rings turn grape (theme-driven) while every link stays
blue (hardcoded), so blue would mean "planned closure" *and* "this is a
link" in the same viewport at the same time.

**That did not happen.** All seven call sites (see Current state) were
converted in the same change that set the primary colour, so links now
follow `primaryColor` (grape) while `GROUP_COLOR.planned` stays blue,
untouched, as the Non-goals require. `StatusBadge` was never at risk in
the first place — it passes `color={severityColor(...)}` explicitly
(`components/StatusBadge.tsx`) and so was always immune to the primary-colour
swap. The separation this spec set out to achieve — link colour and
planned-severity colour no longer sharing a hue — is in place. Verify on
any page where a link sits beside a `StatusBadge`: `/lines/[id]` ("View
history") or `/lines/[id]/history` ("Back to line"); see Verification
below.

One thing this change didn't touch, and needed a separate fix: the
data-quality/provenance badge in `components/IssueList.tsx` (~line 292,
`{DATA_QUALITY_LABELS[status.dataQuality]}`) had no `color` prop, so it
fell back to `theme.primaryColor` — meaning it went from blue to grape
along with everything else, reading as branded/interactive rather than as
neutral metadata. Commit `79d2176` ("Give the data-quality badge an
explicit gray colour") fixed this with `color="gray"`, matching how
`informational` severity is already treated in `GROUP_COLOR`. That is
done, not open.

> **Footnote — what the first draft of this section got wrong:** it
> misidentified the badge. The blue "PLANNED" pill visible in the
> 2026-08-18 review screenshots, sitting beside blue links, was assumed to
> be a `GROUP_COLOR.planned` severity badge. It wasn't — it was the
> data-quality badge described above. The mix-up was easy to make:
> provenance (`DATA_QUALITY_LABELS.planned`, "how we learned about the
> issue") renders as "PLANNED", while severity (`GROUP_COLOR.planned`, "a
> planned closure") renders as "PLANNED CLOSURE" / "PART CLOSURE" — two
> different concepts that both surface the word "Planned" as an uppercase
> badge. Check the `color` prop a badge actually receives, not its label
> text, before concluding which map its colour comes from.

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

**Status: resolved, but not the way this section originally advised.** The
first draft said "if white text on grape 6 fails for small text, prefer
raising the light shade to 7". Grape 6 on white measures **4.02:1** — it
does fail AA's 4.5:1 for body text — but raising `primaryShade` turned out
to be the wrong instrument, and was rejected on evidence:

`primaryShade` is **not scoped to `primaryColor`**. Mantine's
`get-css-color-variables.mjs` feeds `getPrimaryShade()` into the
`-filled`/`-outline`/`-text` variables of *every* colour in `theme.colors`,
not just the primary. Raising it to 7 would therefore have shifted
`--mantine-color-red-filled`, `-yellow-filled` and the rest — which is
exactly what `StatusBadge` (`variant="filled"`) consumes. That would have
broken this spec's own Non-goal that the severity palette must not change.
See the reasoning comment in `frontend/lib/theme.ts`.

What shipped instead is a scoped override in `app/globals.css`:
`--mantine-color-anchor: var(--mantine-color-grape-7)` for the light scheme
only (grape 7 = `#ae3ec9` = **4.85:1**, clears AA). Nothing but Mantine's
`Anchor` and the `c="var(--mantine-color-anchor)"` call sites read that
variable, so links are fixed without touching `primaryShade`, `StatusBadge`,
or filled buttons. The dark scheme is left alone — Mantine resolves anchor
to shade 4 there, and grape 4 on the dark body is 5.84:1.

`app/globals.test.ts` encodes the WCAG contrast formula and asserts these
ratios directly, so the thresholds are checked rather than asserted by
eye.

**Resolved** by
`docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md` (Task
4): `autoContrast: true` with a derived `luminanceThreshold: 0.179` is
that deliberate decision against the Non-goals, made once the
accessibility audit gave the evidence this section asked for — white text
fails AA on all five of `GROUP_COLOR`'s hues, not just the amber one this
review guessed at. `lib/severity.ts`'s hues are untouched; badges just
gained (mostly) black text. Filled grape specifically goes to grape 7 via
a scoped `variantColorResolver`, the same instrument this section
predicted (:161-162 of the original draft) — white-on-grape-6's 4.02:1 is
now white-on-grape-7 at 4.85:1.

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
white text is darker than blue 8; confirmed at **5.82:1** (clears AA) —
see `app/globals.test.ts`'s `filled-surface contrast under autoContrast`
suite. The review also flagged existing contrast concerns (dimmed grey
body text, white-on-amber badges) as *unmeasured*; both are now measured
and fixed by `docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md`
(Task 4 for the badges, Task 5 for dimmed text: gray 6 at 3.32:1 → gray 7
at 8.18:1 in the light scheme). That same plan's Task 6 additionally
found and fixed a related dark-mode-only regression `autoContrast` alone
would have introduced for the gray/blue severity badges specifically (not
predicted by this section) — see `lib/theme.ts`'s
`SCHEME_BLIND_FILLED_COLORS` comment.

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

- `/lines/[id]` — the worst-status `StatusBadge` beside the "View
  history" link, and (inside `IssueList`) each issue's severity badge
  beside its gray data-quality badge, all in one view. When the worst
  status is a planned closure this is where a blue `StatusBadge` sits
  next to a now-grape link.
- `/lines/[id]/history` — per-entry status badges beside the "Back to
  line" link.
- Home (`/`) in both light and dark — "Browse all lines"/"Look up a
  station" links alongside pinned-station status badges.

(`/lines` renders no `StatusBadge` at all — its table is
Name/Category/Operators/Pin only — so it never showed this collision and
isn't useful for this comparison.)

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
