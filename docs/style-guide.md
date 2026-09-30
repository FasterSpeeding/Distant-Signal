# Distant Signal style guide

How the Distant Signal frontend looks, reads and behaves. This is the
versioned, plain-text copy of the illustrated guide published at
<https://claude.ai/code/artifact/5fa075dc-fa7e-43b9-a94b-2a43393ac77d>,
which was derived from the code at `e79c8ed2` on 29 Sep 2026. The
illustrated guide has rendered specimens of every component; this file has
the rules. Where they disagree, this file is newer: it includes the
consistency fixes made after the guide was written (see
[Changes since the illustrated guide](#changes-since-the-illustrated-guide)).

Every value here is the product's own: Mantine 9 with a grape primary, the
system font stack, and the WCAG AA overrides in `frontend/app/globals.css`.
Rules marked *(convention)* are patterns in the code, not decisions written
down anywhere else.

## Principles

- **Mantine first.** Everything is a Mantine 9 component styled through
  theme tokens. The brand is one decision, `primaryColor: 'grape'`; fonts,
  spacing and radius are Mantine defaults. (`lib/theme.ts`)
- **AA or it doesn't ship.** Contrast is measured, not eyeballed. Where a
  Mantine default falls under 4.5:1, `globals.css` swaps the shade and
  `globals.test.ts` asserts it.
- **Colour means status.** Grape is the brand and the only accent. Green,
  yellow, orange, red, blue, teal and gray carry meaning about trains and
  lines, never decoration.
- **Honest data.** Show when data was fetched, say where it came from, and
  omit a value rather than invent one. Every feed gets its licence-mandated
  credit.

## Colour

The colour scheme follows the OS by default (`defaultColorScheme="auto"`);
the nav's theme toggle cycles light → dark → auto. `primaryShade` stays at
Mantine's default (light 6, dark 8) because raising it would shift every
status badge too. Instead `globals.css` substitutes grape 7 for links and
filled grape in the light scheme only.

### Grape

| Shade | Hex | Use |
| --- | --- | --- |
| 4 | `#da77f2` | Dark scheme links (5.84:1 on `#242424`) |
| 6 | `#be4bdb` | Body wash at 6% only; never text (4.02:1 on white fails) |
| 7 | `#ae3ec9` | Light scheme links, filled buttons, focus ring, skip link (white 4.85:1) |
| 8 | `#9c36b5` | Light filled hover; dark filled (white 5.82:1) |
| 9 | `#862e9c` | Dark filled hover; light-variant text on grape 1 (5.59:1) |

### Surface and text tokens

| Role | Token | Light | Dark | Note |
| --- | --- | --- | --- | --- |
| Page background | `--mantine-color-body` | `#ffffff` | dark 7 `#242424` | Plus a grape 6 wash at 6% fading out by 70vh |
| Text | `--mantine-color-text` | `#000000` | dark 0 `#c9c9c9` | |
| Dimmed text | `--mantine-color-dimmed` | gray 7 `#495057` | dark 1 `#b8b8b8` | Overridden from gray 6 (3.32:1) and dark 2 (4.04:1) |
| Placeholder | `--mantine-color-placeholder` | gray 7 | dark 1 | Same override as dimmed |
| Card / surface | Card bg | `#ffffff` | dark 6 `#2e2e2e` | Border gray 3 `#dee2e6` / dark 4 `#424242` |
| Link | `--mantine-color-anchor` | grape 7 | grape 4 | Light overridden from grape 6 |
| Filled grape | `--mantine-color-grape-filled` | grape 7, hover 8 | grape 8, hover 9 | Text pinned white by `variantColorResolver` |
| Error text | `--ds-color-error-text` | red 9 | red 4 | App-owned token |
| Good / bad icon | `--ds-color-good-icon`, `--ds-color-bad-icon` | green 7 / red 7 | green 4 / red 4 | Non-text glyphs |
| Light-variant text | `--mantine-color-*-light-color` | yellow `#a85700`, orange `#bb3e0d`, green `#267b37`, teal `#087a57` | shade 0 | Off-palette hexes, each ≥4.6:1 on its shade-1 background |

App-owned tokens are prefixed `--ds-`. The overrides use
`html:root[data-mantine-color-scheme=…]` so they beat MantineProvider's
later-injected `:root` block on specificity.

### Semantic roles

| Colour | Means | Used for |
| --- | --- | --- |
| green | Good service, on time, arrived, **success** | Severity `good`; "On time"; "Arrived"; every success Alert ("Saved.", "Ticket saved", "Added to …") |
| gray | Informational, neutral, unknown | Severity `informational`; unchanged platform; "Awaiting first report"; provenance badges; "Affected line"/"Affected station" tags; the trip planner's "N changes" |
| blue | **A planned or changed arrangement — not a fault** | Severity `planned` (Planned Closure, Part Closure); incident "Planned Work"; a changed platform |
| yellow | Minor disruption, caution, partial data | Severity `mild`; journey leg "Delayed"; "Some of this range isn't available" |
| orange | Running late, skipped stop, draft | "12m late"; "Not stopping at your station"; "Real-Time" incidents; draft legal page |
| teal | Early, or a live Darwin source | "3m early"; ETA "Live departure board" |
| red | Severe, cancelled, errors, destructive actions | Severity `severe`; cancelled trains and platforms; every error Alert; delete buttons |
| grape | Brand, primary action, links, product notices, **"you need to act"** | Buttons, links, focus; informational Alerts (Delay Repay estimate, API-key note, "Creating a line needs an account"); "Needs a train picked" and the open-leg card |

Blue used to carry five meanings; see the decision recorded in
`docs/superpowers/specs/2026-08-18-grape-theme-design.md` ("Open
question", 2026-09-29). Chart series colours (`TrendsCharts`) are data
encodings, not status, and are outside this table.

## Typography

One family: the Mantine system stack, for body and headings. No web fonts
are loaded anywhere, including the standalone pages. Monospace uses
Mantine's `ui-monospace` stack. Headings are weight 700; card titles 600,
list-row titles 500.

| Size | px / line-height | Example |
| --- | --- | --- |
| h1 | 34 / 1.3 | Account & data |
| h2 | 26 / 1.35 | Page not found |
| h3 | 22 / 1.4 | Download my data |
| h4 | 18 / 1.45 | a subsection |
| md | 16 / 1.55 | Body copy |
| sm | 14 / 1.45 | Supporting copy inside cards |
| xs | 12 / 1.4 | Timestamps, credits, freshness rows (dimmed) |
| badge | 11 / 700 | Platform 4 |

- Heading level and visual size are set separately. Pick the level for the
  outline; the size follows from it (see [Heading rules](#heading-rules)).
- Badges keep Mantine's uppercase only for short fixed labels, chiefly the
  severity `StatusBadge`. Any badge whose label is a phrase or carries units
  gets `tt="none"`: "12m late", "Not stopping at your station",
  "Platform 6 (changed from 4)", "Live departure board", "Pending match".
  There is deliberately no theme-level Badge default (`lib/theme.ts`
  explains why): the case is chosen per label.

## Spacing, radius and elevation

- Spacing scale: xs 10 · sm 12 · md 16 · lg 20 · xl 32 px. Pages pad with
  `p="lg"`. Stacks use `gap="md"` between page blocks, `gap="lg"` between
  section cards, `gap="xs"` or `"sm"` inside a card.
- Radius: xs 2 · sm 4 · md 8 (default) · lg 16 · xl 32 px. Cards, buttons,
  inputs and alerts are 8px; badges are pills. Nothing overrides radius.
- Elevation: borders do the work. `shadow="sm"` only on clickable cards
  (`LineStatusCard`) *(convention)*. Mantine drops shadows in dark mode.

## Layout and page templates

- App shell: skip link → `nav aria-label="Main"` → `<main id="main-content">`
  → footer. Sticky footer via `body { display:flex; min-height:100vh }` and
  `main { flex: 1 }`.
- Nav and main share `Container size="lg" px={0}` (1140px max) so content
  edges line up with the bar. The bar is 60px minimum, `px="lg" py="md"`,
  with a bottom border and a dashed "sleeper" rule (20px dash, 16px gap,
  text colour at 20%).
- Primary links show at `md` (62em) and up; below that they move into the
  burger `Drawer` (`AppNavDrawer.tsx`).

### Page widths

| Width | Use |
| --- | --- |
| 1140px | Dashboards and lists (status, lines, trains, stations) |
| 640px | Single-column flow pages: `<Stack p="lg" maw={640}>` (`/account`, forms) |
| 480px | Short creation forms: `/groups/new`, `/lines/new`, `/lines/[id]/edit` |
| 760px | Long reading pages: `READING_PAGE_WIDTH` from `components/LegalPage.tsx`, used by `LegalPage` and `/attribution` |
| 280px | Tooltip max width (`DataFreshnessInfo`) |

### Heading rules

- Exactly one h1 per page, `<Title order={1}>` at full h1 size (34px).
- Everything below the h1 uses **`SectionTitle`** (`components/SectionTitle.tsx`),
  which derives the size from the level so the two can't drift:

  | Level | Tag | Drawn at |
  | --- | --- | --- |
  | Section (`order` 2, default) | h2 | h3 size, 22px |
  | Subsection (`order={3}`) | h3 | h4 size, 18px |
  | Sub-subsection (`order={4}`) | h4 | h5 size, 16px |

  This applies on every page, dense detail pages included. Never leave an
  `order={2}` unsized: at 26px it reads as a second page title on mobile.
- Section cards are `<Card withBorder component="section" aria-labelledby=…>`
  with a `SectionTitle` carrying the matching `id`.
- Error and not-found pages use `<Title order={1} size="h2">`, dimmed
  explanation, then a way out. The standalone offline page matches.
- Never skip heading levels. A small bold label naming a group is still a
  real heading, `<Title order={3} size="sm">`, not bold `Text`.
- Chart titles inside `TrendsCharts` are sized `h6` by that component and
  are not section headings.

### Breakpoints

| Token | Width | What changes |
| --- | --- | --- |
| xs | 36em · 576px | Group header actions and member rows stack; standalone-page buttons go full width |
| sm | 48em · 768px | Issue rows stack and clamp to 2 lines; lines filter bar becomes sticky; journey-progress slots shrink 56→44px |
| md | 62em · 992px | Nav links move from the bar into the drawer |
| lg / xl | 75em / 88em | No custom behaviour |

## Components

- **TextLink** (`components/TextLink.tsx`): every text link. `underline="hover"`
  (default) for positional links; `underline="always"` plus `inline` for a
  link in running text. Hrefs with a URL scheme (`https:`, `mailto:`)
  render a plain `<a>` instead of `next/link`. `tone="inherit"` is only for
  credit and licence links inside dimmed or statement text (footer credits,
  `NationalRailCredit`, `/attribution`): the link keeps the surrounding
  colour and is marked by its underline alone. Don't hand-roll
  `<a style={{ color: 'inherit' }}>`. `external` opens a new tab and marks
  it with the `ExternalLinkIcon` SVG plus visually hidden "(opens in a new
  tab)"; never write "↗" into link text. Decorative text arrows ("← Groups")
  go in `<span aria-hidden="true">`.
- **Route arrows** (`components/RouteArrow.tsx`): "KGX → YRK" keeps its
  arrow on screen but reads "KGX to YRK": the arrow is aria-hidden and a
  `VisuallyHidden` "to" stands in. Render route strings from
  `lib/stationLabel.ts` through `<RouteText>`, write `<RouteArrow />` in
  place of a literal "→" in JSX, and use `spokenRoute()` where only plain
  text fits (`aria-label`, titles). `Select` options take
  `renderOption` with `RouteText`.
- **Buttons**: Mantine Button sm, 36px, 14px/600, radius 8. Filled grape
  for the primary action, light for secondary, default for neutral, red
  filled for destructive (text via `autoContrast`). Icon buttons are
  `ActionIcon` subtle or outline with 16px SVGs.
- **StatusBadge**: filled, uppercase, severity colour; text colour by
  `autoContrast` at threshold 0.179; gray and blue use `light-dark()` so
  dark mode gets white.
- **PlatformBadge**: light, sentence case. Unchanged gray; changed blue with
  the old platform in words; cancelled red, struck through, with a hidden
  "(cancelled)" and a `title` tooltip. A null platform renders nothing.
- **LineStatusCard**: `Card withBorder shadow="sm" padding="lg"`; the whole
  card is the link. Title 600 clamps to 2 lines, reason to 3.
- **Section card**: see [Heading rules](#heading-rules); `Stack gap="xs"` or
  `"sm"` with `align="flex-start"` so buttons don't stretch.
- **Alerts** (light variant): red for errors (title "Couldn't …" or "…
  failed", `role="alert"`), yellow for partial data, green for success,
  grape for product notices and information. No blue alerts.
- **LastUpdated / DataFreshnessInfo**: relative time re-rendered every 30s,
  exact London time in a tooltip; SSR shows the absolute time until mounted.
- **Form fields**: Mantine `TextInput`/`Autocomplete`: label 14/500,
  description xs dimmed, error in `--mantine-color-error`.
- **Footer credits** (`OpenDataAttribution.tsx`): credit wording is licence
  text; copy it exactly, including "NationalRail" as one word.
- **Pride mode** (`PrideToggle.tsx`): decorative, off by default. Each
  flag's stripes are one `--ds-pride-<flag>` variable in `globals.css`,
  read by the page bar, nav bar, site title and the toggle's swatch.

## States

| State | Pattern | Example copy |
| --- | --- | --- |
| Loading | `Skeleton` inside Suspense; `Loader size="sm"` inline. Pair with visible "Loading…" in `role="status" aria-busy`. The nav's fallback is the full logged-out bar. | "Loading…" |
| Empty | Dimmed text saying what would appear, with a route out. No illustrations. | "Nothing is affecting this line right now." |
| Error (page) | `app/error.tsx`: h1 at h2 size, dimmed cause, "Try again", home link, xs "Reference:" digest | "This page couldn't be loaded…" |
| Error (inline) | Red Alert, title names the failed action | "Couldn't save this ticket" |
| Not found | h1 at h2 size, cause, three always-underlined way-out links | "There's no page at this address…" |
| Stale / offline | Bottom-centre `Notification loading`, `role="status" aria-live="polite"`. The offline page (`public/offline.html`) shows the last successful load as "Last connected 4m ago." | "Reconnecting…" |
| Cancelled | Red badge "Cancelled", red Alert on the train page, struck-through platform | "Cancelled: the train no longer calls at this platform" |
| Unknown | Omit the element or say so plainly; never fabricate | "No status yet" · "(no arrival report received)" |

### Standalone pages

`public/offline.html` has no React runtime and no network, so it inlines
Mantine `DEFAULT_THEME` values plus the `globals.css` overrides as `--ds-*`
tokens, with a comment naming each source. `public/offline.test.ts` checks
every token against `DEFAULT_THEME`, forbids other colour literals and
network resources, and checks the relative-time wording. It has the brand
bar (a `header`, not a `nav`), the 8px Button-sm metrics and the grape wash.

## Data display

- Railway times render in Europe/London via `lib/dateFormat.ts`: `08:42`,
  `29 Sep 2026`, `29 Sep 2026, 08:42`, short date `29 Sep`. 24-hour clock.
  Say "Times in UK local time" where it matters.
- Only instants personal to the viewer (`LocalDateTime`) use the browser
  zone, after mount.
- Relative time: "just now", "4m ago", "3h ago", "2d ago"
  (`lib/relativeTime.ts`), with the exact time in a tooltip.
- Delays: light badges, sentence case, "*n*m late" / "*n*m early" / "On
  time". Journey rollups rank good < awaiting < delayed < unmatched <
  skipped < cancelled.
- Severity labels are TfL's own Title Case names; unknown codes read
  "Unknown" in gray (`lib/severity.ts`).

## Accessibility

- 4.5:1 for all text, including badge labels and placeholders. Filled
  surfaces pick text via `autoContrast` at `luminanceThreshold: 0.179`.
- Focus: Mantine's 2px outline in filled primary, 2px offset.
- Not colour alone: every status badge has a text label; cancelled
  platforms are struck through and say "(cancelled)"; changed platforms
  spell out the old number; the open-leg card has a border, tint, bold
  title and glyph.
- Landmarks: `main`, `nav aria-label="Main"`, footer
  `nav aria-label="Site information"`; sections use `aria-labelledby`.
- New-tab links (`TextLink external`, and every link in sanitized feed
  HTML) say "(opens in a new tab)" in their accessible name.
- `VisuallyHidden` repeats tooltip-only information. Icon buttons always
  have `aria-label`; decorative SVGs are `aria-hidden`.
- Modals and drawers get `aria-label="Close"` from the theme; passing
  `closeButtonProps` replaces it, so repeat the label.
- Motion only under `prefers-reduced-motion: no-preference`.
- Small icons get a 24px hit area (`.iconHitArea24`). Tooltips open on
  hover, focus and touch.
- Checked by `e2e/accessibility.spec.ts`.

## Writing

- Plain, direct, second person. Say what happened and what to do next.
- Failures start with "Couldn't". No apologies.
- Headings use sentence case. Nav labels are short nouns; some are Title
  Case *(convention)*.
- Use rail terms people know ("platform", "calling point", "Delay Repay").
  UK spelling.
- Be honest about uncertainty: "(no arrival report received)", "Estimate
  (Network Rail)".
- Credits use each licence's exact wording; the site calls itself "an
  independent, unofficial service" and uses no provider logos.
- Em dash "—", never "--"; ellipsis "…".
- No engineering vocabulary on screen: no raw enum values, field names,
  bare codes, ISO dates or `error.message`. Map through
  `lib/displayLabels.ts` and `lib/stationLabel.ts`.
- Show the working: "2 of 10 sampled services delayed" beats "Some delays".

## Do and don't

- **Do** `<Text c="dimmed" size="xs">` for secondary text. **Don't**
  `c="gray.6"` or a hex grey: gray 6 is 3.32:1 on white.
- **Do** use `TextLink` for every text link. **Don't** hand-roll anchors.
- **Do** pair every coloured badge with words that carry the meaning.
  **Don't** use red for anything that isn't cancelled, severe, an error or
  destructive. **Don't** use blue for anything that isn't planned or
  changed.
- **Do** format times with `lib/dateFormat.ts`. **Don't** call
  `toLocaleTimeString()` in rendered components (server UTC vs browser).
- **Don't** put grape 6 text on white, or white-less text on filled grape.

## Code conventions

- Style with Mantine props and CSS variables (`var(--mantine-color-*)`,
  `var(--mantine-spacing-*)`); app tokens are `--ds-*`.
- Global CSS in `app/globals.css`, as BEM-ish classes or `data-*` hooks
  (`[data-text-link]`, `[data-status-badge]`).
- Theme config is one object in `lib/theme.ts`, shared by the app and tests.
- Icons are hand-inlined 16px Feather-style SVGs (`stroke-width 2`,
  `currentColor`, `aria-hidden`); no icon library *(convention)*. No
  Unicode glyphs as functional icons.
- Prettier 3: `printWidth: 120`, `singleQuote: true`.

## Changes since the illustrated guide

Resolved on the style-guide consistency branch (29 Sep 2026):

- Section headings: `SectionTitle` sets one size per level app-wide
  (previously unsized 26px, h3, h4 and h5 for the same h2).
- Badges: `JourneyTimeline`, `TrackedTrainStatusBadge`, `EtaBadge` and
  `ItineraryOption` badges are sentence case.
- Blue narrowed to planned/changed arrangements (see Semantic roles).
- Success is always green ("Ticket saved" was blue).
- `offline.html` uses the real tokens, brand bar, 8px radius and "4m ago".
- Credit and contact links go through `TextLink`.
- 760px is `READING_PAGE_WIDTH`, defined once.
- Pride flag gradients are defined once in `globals.css`.

Handled on other branches at the same time: `ScheduleRow` delay format and
contrast, unlabelled loading skeletons, cookies/privacy heading levels, and
Unicode glyph icons in `ShareJourneyLinkButton`, `GroupInviteLinkCard` and
`JourneyProgress`.
