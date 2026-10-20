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
the colour theme picker on `/account/preferences` (`ThemePreference`) offers
match device, light and dark. `primaryShade` stays at
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
| Severity text | `--ds-severity-<group>-text` (`severityGroupTextColor`) | green `#267b37`, gray 7, blue 9, yellow `#a85700`, red 9 | shade 4 of each | Same hue as the group's `StatusBadge`; each ≥5.2:1 on a card |
| Light-variant text | `--mantine-color-*-light-color` | yellow `#a85700`, orange `#bb3e0d`, green `#267b37`, teal `#087a57` | shade 0 | Off-palette hexes, each ≥4.6:1 on its shade-1 background |

App-owned tokens are prefixed `--ds-`. The overrides use
`html:root[data-mantine-color-scheme=…]` so they beat MantineProvider's
later-injected `:root` block on specificity.

### Semantic roles

| Colour | Means | Used for |
| --- | --- | --- |
| green | Good service, on time, arrived, **success** | Severity `good`; "On time"; "Arrived"; every success Alert ("Saved.", "Ticket saved", "Added to …") |
| gray | Informational, neutral, unknown | Severity `informational`; unchanged platform; "Awaiting first report"; provenance badges; "Affected line"/"Affected station" tags; the trip planner's "N changes" |
| blue | **A planned or changed arrangement, not a fault** | Severity `planned` (Planned Closure, Part Closure); incident "Planned work"; a changed platform |
| yellow | Minor disruption, caution, partial data | Severity `mild`; journey leg "Delayed"; "Some of this range isn't available" |
| orange | Running late, skipped stop, draft | "12 min late"; "Not stopping at your station"; draft legal page |
| teal | Early, or a live board source | "3 min early"; ETA "Live departure board" |
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
- Every badge is sentence case with `tt="none"`, the severity
  `StatusBadge` included ("Minor delays", via `severityBadgeLabel`):
  "12 min late", "Not stopping at your station", "Planned work", "AI
  summary". There is deliberately no
  theme-level Badge default (`lib/theme.ts` explains why).
- At most three badges in a row, in the order severity (`StatusBadge`),
  state (`StateBadge`), then "AI summary" (`AiGeneratedBadge`). `BadgeRow`
  takes one badge per slot, so a fourth or a different order can't be
  written. Lines, operators and stations an item affects are dimmed text,
  operators by name, not pills; so are provenance (data quality, a
  ticket's source) and group roles. Don't badge what the severity already
  says (no "Real-time" badge).

## Spacing, radius and elevation

- Spacing scale: xs 10 · sm 12 · md 16 · lg 20 · xl 32 px. Pages pad with
  `p="lg"`. Stacks use `gap="md"` between page blocks, `gap="lg"` between
  section cards, `gap="xs"` or `"sm"` inside a card.
- Radius: xs 2 · sm 4 · md 8 (default) · lg 16 · xl 32 px. Cards, buttons,
  inputs and alerts are 8px; badges are pills. Nothing overrides radius.
- Elevation: borders do the work. `shadow="sm"` only on a card that is
  itself a link (`CardLink`, `LineStatusCard`, the linked /status tiles);
  a card holding its own controls (`OperatorStatusCard`) is flat. No
  coloured left-border accents (the open-leg card's is its one,
  deliberate, non-colour-only exception; see Accessibility). Mantine drops
  shadows in dark mode.

### Design tokens

`tokens` in `frontend/lib/theme.ts` names the app's own values on top of
Mantine's theme. Spread them; don't repeat the numbers.

| Token | Value | Use |
| --- | --- | --- |
| `tokens.card.static` | `withBorder` | A card that only holds content or its own controls |
| `tokens.card.link` | `withBorder`, `shadow="sm"` | A card that is one link (`CardLink`, `LineStatusCard`) |
| `tokens.statusTile` | gap 4, value xl/700, label sm | `StatusTile`, a count and its label (`/status` top strip) |
| `tokens.statusCard` | padding lg, title clamp 2, reason clamp 3 | `StatusCard` (lines, operators, the /status mode cards) |
| `tokens.iconButton.field` | 36px | An `ActionIcon` beside a 36px input (`CopyableLink`) |
| `tokens.iconButton.pageAction` | 44px | Page-level pin and share (`PinToggle`, `ShareButton`, review §2.10) |

A new shared value goes here first, with a comment saying where it is
used, rather than as a literal in a component.

## Layout and page templates

- App shell: skip link → `nav aria-label="Main"` → `<main id="main-content">`
  → footer. Sticky footer via `body { display:flex; min-height:100vh }` and
  `main { flex: 1 }`.
- Nav and main share `Container size="lg" px={0}` (1140px max) so content
  edges line up with the bar. The bar is 60px minimum, `px="lg" py="md"`,
  with a bottom border and a dashed "sleeper" rule (20px dash, 16px gap,
  text colour at 20%).
- The bar starts with the wordmark (`Wordmark`: the favicon's signal arm on
  its grape tile, 24px, then "Distant Signal" in 16px bold). Its links are
  Track, Plan a journey, Status, Lines, Operators, Stations and Incidents
  (`lib/navLinks.ts`). Track is the one tracking entry: it opens the Track
  page (`/track/mine`) and stays marked current on `/track`, `/trains` and
  `/journeys`. No settings live in the bar: the theme and pride flag are
  on `/account/preferences`, reached from the account menu, the drawer and
  the footer.
- One exception: a logged-out visitor at `md` and up gets a gear
  (`PreferencesNavIcon`, labelled and tooltipped "Preferences") beside
  "Log in". It is a link to the settings page, not a control, and they
  have no account menu or drawer to reach it otherwise. Logged in, the
  account menu does that job, so the gear is left out.
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
| sm | 48em · 768px | Issue rows stack and clamp to 2 lines; the /lines filters open as a bottom sheet instead of a popover and its table cells drop to 6px side padding; journey-progress slots shrink 56→44px |
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
  tab)"; never write "↗" into link text. No text arrows on links: a back
  link says "Back to groups", a forward link says what it opens ("Next
  trains", "Full day's timetable").
- **ConfirmActionButton** (`components/ConfirmActionButton.tsx`): every
  "are you sure?" action (delete, remove, leave, stop tracking, promote,
  demote). One trigger style: `size="xs"`, red outline for `tone="danger"`
  (the default), default variant for a reversible `tone="neutral"` change.
  The modal's confirm button is named "Confirm <actionName>" ("Confirm
  delete ticket"), so it never shares a name with the trigger and still
  contains its visible text. A 401 shows `loginPrompt` ("Log in to delete
  this ticket") as a `LoginControl` link; any other failure reads through
  `failureFromResponse`/`describeFailure`. The `Delete*Button`/`Remove*Button`
  components are thin wrappers that pass the request, copy and
  `onSuccess`. Never hand-roll a confirm `Modal`.
- **LoginControl** (`components/LoginControl.tsx`): the one login link,
  carrying `return_to` and `prefetch={false}`. `variant="link"` (default)
  for "Log in to …" under a failed action or in the nav;
  `variant="button"` for a page whose one anonymous action is logging in
  (`/groups/join`, `/groups`, `/account`), followed by the LEG-1 consent
  note.
- **CardLink** (`components/CardLink.tsx`): a whole card that is one link
  (home page rows, `/groups`, `/status` tiles). It takes
  `tokens.card.link`; `[data-card-link]`/`[data-card]` in `globals.css`
  keep the text its own colour, turn the border grape on hover and focus,
  and draw the focus ring on the card. A heading link inside a card that
  holds other controls is a plain `<Link data-card-link>` around just the
  heading. Never `style={{ textDecoration: 'none', color: 'inherit' }}`.
- **ButtonLink** (`components/ButtonLink.tsx`): an in-app link drawn as a
  button ("Create a group", "Edit line", "Back to chat"). One `<a>`, never
  a `<button>` inside a `<Link>`; usable from Server Components.
- **CopyableLink** (`components/CopyableLink.tsx`) and **useShareOrCopy**:
  every copy-to-clipboard value (invite and share links, the MCP server
  URL and install commands, the connector URL). A read-only field, a 36px
  outline `ActionIcon` ("Copy MCP server URL", or "Share invite link" with
  `share`, which tries the Web Share API first), and a polite live region
  reading "Copied." or "Couldn't copy. Select the text and copy it
  yourself." `ShareButton` uses the same hook at the 44px page-action size.
- **DelayBadge** (`components/DelayBadge.tsx`): the one delay badge, worded
  by `lib/serviceStatus.ts` ("12 min late" orange, "3 min early" teal, "On
  time" green). `DELAY_TONE_COLOR` is the one delay-to-colour table;
  `JourneyProgress`'s stop markers read it too.
  In a narrow table column (`compact`) it reads "+12 min" or "−3 min"
  below `sm`, with the full words kept for screen readers (`PhoneLabel`).
- **Nearby stations** (`app/stations/NearbyStationList.tsx`): the "Use
  my location" results under a "Nearest stations" label, in
  `ServiceRowList`'s bordered, divided list. Each row is one link to the
  station page, at least 44px tall, with hover, active and an inset focus
  ring: "Woking (WOK)" on the left at 500, the distance in its own
  right-hand column, sm, dimmed, tabular figures, always "0.4 km" (km to
  one decimal place). The link is named "Woking (WOK), 0.4 km away".
- **Route text**: a route reads "KGX to YRK" on screen and to a screen
  reader alike. `lib/stationLabel.ts` (`routeLabel`, `codeRouteLabel`)
  joins the ends with "to", so its strings go straight into JSX, titles,
  `aria-label`s and `Select` options. Never "→" in copy.
- **Journey names**: one rule, `lib/journeyName.ts`, wherever a journey
  is listed, headed or previewed (the Track list, the journey page, group
  rows, templates, share-link previews, Connected apps). The name is the
  owner's custom name, verbatim. Without one it is the whole route and the
  first day: the FIRST leg's origin to the LAST leg's destination,
  "London Kings Cross (KGX) to Edinburgh (EDB), 17 Sep 2026", through
  `routeLabel`, so both ends fall back to codes together ("KGX to EDB").
  Never the current leg's or the first leg's destination. A secondary
  line, `journeyPreviewLine`, brings back the route and date a custom name
  hides and adds "3 legs" for a multi-leg journey, joined by " · ". The
  journey page shows the route and date under a custom name; its per-leg
  summary already counts the legs. Only a date, no time: until a leg has
  a train its time is a window, and a name must not change shape when a
  leg resolves. Unresolved, untracked and cancelled legs name the journey
  like any other (a leg end with a name but no code shows its name); the
  status badge says what is wrong. No legs: "Journey with no legs yet". A
  template has no day, so its name is the route alone. A share-link
  preview leaves the custom name out (it is the owner's private label) and
  uses the default name. Push notifications name a journey "'Custom
  name'" or "your journey to Edinburgh" and a leg by its position and
  route, "Leg 2 of your journey to Edinburgh is delayed", stations by name
  ("York to Newcastle"), or codes on both ends when either name is
  unknown.
- **Buttons**: Mantine Button sm, 36px, 14px/600, radius 8. Filled grape
  for the primary action, light for secondary, default for neutral, red
  filled for destructive (text via `autoContrast`). Icon buttons are
  `ActionIcon` subtle or outline with 16px SVGs.
- **StatusBadge**: filled, sentence case ("Part suspended"), severity colour; text colour by
  `autoContrast` at threshold 0.179; gray and blue use `light-dark()` so
  dark mode gets white.
- **PlatformBadge**: light, sentence case. Unchanged gray; changed blue with
  the old platform in words; cancelled red, struck through, with a hidden
  "(cancelled)" and a `title` tooltip. A null platform renders nothing.
  `compact` (under a Platform column header) reads "4" or "9 (was 6)"
  below `sm`, the column header "Plat", both spoken in full.
- **Badges are never cut short.** Mantine's Badge ellipsis is off for
  `StatusBadge`, `DelayBadge` and `PlatformBadge` (`globals.css`): a
  badge is as wide as its words, and a layout too narrow for them shortens
  the words (`compact`) or moves the badge, never trims it.
- **Calling-point table** (`JourneyTimeline`): below `sm` the delay sits
  under the Actual time instead of in its own column, cells have 6px side
  padding, and the table only scrolls sideways (inside its own box) when a
  station name leaves no other way.
- **StatusCard** (`components/StatusCard.tsx`): the one status card. A
  name (600, clamped to 2 lines) with its `StatusBadge` on a row that never
  wraps, the reason (sm dimmed, clamped to 3), detail lines, then a footer
  pinned to the bottom edge. With `href` the whole card is one link
  (`CardLink`, raised); without, it is flat because it holds its own
  controls. `LineStatusCard`, `OperatorStatusCard` and the /status mode
  cards are built on it. A mode card's badge is the worst severity in the
  slice, and the count is a sentence ("2 of 120 lines affected.").
- **StatusTile** (`components/StatusTile.tsx`): a count over its label,
  the /status strip. Below `sm` each tile is one compact row (count, label,
  "View lines") in a single column; from `sm` up, five across. Five tiles
  never fill a two-column grid, so there is no two-column layout. Labels
  are sentence case (`severityGroupLabel`). `severity` colours a linked
  tile's count in its group's hue (`--ds-severity-<group>-text`), so the
  strip reads red to green like the badges; the label still names the
  group, and a zero tile stays dimmed. Colour goes on the count, never a
  left-border accent. From `sm` up, count and label sit at the top of every
  tile, so they line up across the row whether or not a tile has the
  "View lines" cue, which drops to the bottom edge.
- **StateBadge** and **BadgeRow** (`components/StateBadge.tsx`): every
  badge that states a thing's state, light variant, sentence case, coloured
  by a semantic `tone` (neutral gray, good green, caution yellow, late
  orange, early teal, planned blue, critical red, action grape), never by a
  colour picked at the call site. `href` makes it an in-page anchor. It
  takes no ref, so a `Tooltip` wraps it in a `<span>`.
- **Wordmark** (`components/Wordmark.tsx`) and `lib/brand.ts`: the signal
  arm (one geometry, checked against `app/icon.svg`) beside the name.
  `data-site-title` (the pride hook) stays on the name only.
- **Link previews** (`lib/previewImage.tsx`): a route's `opengraph-image.tsx`
  renders its own title on the brand template (the bare arm and the name,
  the title below, on a grape 7 to 9 gradient). Static pages pass their
  `metadata`; a line or station passes its own `generateMetadata`, so the
  two can't disagree. Titles never come from the request. Every other
  route falls back to `public/opengraph-image.png`, the template's no-title
  form. A route with its own image passes `OWN_PREVIEW_IMAGE` to
  `pageMetadata`: an explicit card image would otherwise win over it.
- **Preferences** (`app/account/preferences/page.tsx`): open to everyone,
  never a redirect. Each section is a `PreferencesSection` card.
  - Device-local, no account needed: Appearance (colour theme, pride
    flag), saved in this browser and applied before the first paint
    (Mantine's `ColorSchemeScript`, `PRIDE_MODE_SCRIPT`).
  - Account-backed: notifications, and the Claude API key for accounts
    that can use "Ask about trains". Logged out, these give way to one
    "Account settings" card with a log in link back to the page, never a
    permission prompt that then fails.
  - Logging in or out leaves the device-local settings as they are. The
    account keeps no copy, so there is nothing to merge or overwrite.
  - Linked from the drawer (everyone), the account menu (logged in), the
    bar's gear (logged out, `md` and up) and the footer (everyone).
    `/chat` links here instead of holding the key itself.
- **Page skeletons** (`components/PageSkeleton.tsx`): a route whose server
  fetch holds up the first paint gets a `loading.tsx` built from
  `PageSkeleton` pieces (`SkeletonPageHeader`, `SkeletonText`,
  `SkeletonCardGrid`, `SkeletonRows`, `SkeletonStops`, `SkeletonSection`),
  drawn in the page's real padding, gap, width and card shapes so nothing
  jumps when the data lands. One `role="status"` `aria-busy` region per
  page, named by a visually hidden plain-word label ("Loading line"), with
  everything drawn `aria-hidden`; headings are bars, never real h1s.
  In-page Suspense fallbacks keep using `LoadingPlaceholder`. Today: home
  (in the `(home)` route group, because a root `app/loading.tsx` would wrap
  every route), `/lines/[id]`, both train routes (`TrainPageSkeleton`) and
  `/track/mine`. Never add `app/loading.tsx`.
- **Station accessibility** (`StationAccessibilitySection`): the section
  opens with its key facts as a definition list (`<dl class="keyFacts">`,
  term above value on a phone and beside it from `xs` up): step-free access
  ("Category A: step-free access to all platforms"), staff assistance, the
  assistance phone, ramps, wheelchairs, toilets, lifts and Blue Badge bays,
  with Yes or No for booleans. Assistance meeting points are numbered from
  1 and named by location ("Meeting point 1: Customer Information Point"),
  never the feed's zero-based name. A note over 280 characters starts
  collapsed behind a "Notes" disclosure whose accessible name includes its
  field. Shorter notes stay inline.
- **Charts** (`lib/chartTheme.ts`): every chart takes its series colours
  from `CHART_COLORS`, the `--ds-chart-*` variables in `globals.css`. Late
  is orange, cancelled red, skipped stops yellow and a plain count grape;
  each clears 3:1 in both themes. Lines are straight (`CHART_CURVE`), never
  smoothed. Numbers use en-GB separators and the site's units ("80,000",
  "12.5%", "2.5 min"). Ticks read "15 Sep" or "14:30", tooltip headings
  "15 Sep 2026" or "15 Sep, 14:30". Series sharing an axis also differ by
  dash pattern. Sparse buckets are a gray band, never a fake point.
- **Section card**: see [Heading rules](#heading-rules); `Stack gap="xs"` or
  `"sm"` with `align="flex-start"` so buttons don't stretch.
- **Alerts** (light variant): red for errors (title "Couldn't …" or "…
  failed", `role="alert"`), yellow for partial data, green for success,
  grape for product notices and information. No blue alerts.
- **LastUpdated / DataFreshnessInfo**: relative time re-rendered every 30s,
  exact London time in a tooltip; SSR shows the absolute time until mounted.
- **List search and filters** (`components/ListFilterBar.tsx`): one
  row, search on the left and a default-variant "Filters" button on the
  right with a count badge of active filters. Search is never hidden
  behind the button. The filters open in a `Popover` from `sm` up and a
  bottom `Drawer` below it, both a dialog named "Filters" ending in "Clear
  filters" and "Show *n* lines" (or "Show lines" while the count is not
  known). Escape closes an open option list first, then the panel, and
  focus returns to the button. Variants:
  - *Sticky* (the default), for a page's main list: /lines, /incidents,
    /trains (after the first search), /operators, a tram network's stops.
    Transparent until it sticks, then the body colour and a bottom rule.
    It bleeds over the page's `lg` side padding, so its container must
    have that padding. A table's headers stick under it (`stickyHeader`,
    offset by `--list-filter-bar-height`), so such a table sits in no
    scroll container and has to fit a phone on its own.
  - *Search only* (no panel): /operators' "Find an operator" and "Find a
    stop", through `SearchableList`, which filters server-rendered items
    in place.
  - *Filters only, not sticky*, inside a section: the line and station
    issue lists, with the All/Active/Upcoming tabs inline as `trailing`.
    No button when there is nothing to narrow.
  - *Another main control*: /trains puts Station in the search slot. It
    shows the full form until the first search, then the bar.

  Filter state lives in the URL through `useUrlFilterSync`
  (`history.replaceState`, debounced), never `router.replace`, which
  re-runs the page's server render. Keys inside a section are prefixed
  (`issueTab`, `tflIssueSeverity`) so they can't clash with the page's
  own. A server-built link or GET form back to the same page carries them
  through `KeepIssueFilters.tsx`, which reads the live URL (the line page's
  train links and station picker). A server-backed list (/incidents, /trains) applies changes live
  through `useLiveSearch`: one request once typing settles, the previous
  one aborted, a spinner in place of the search glyph and the rows dimmed,
  never blanked. The tab title names what is filtered
  (`useFilteredDocumentTitle`: `Lines: Great Western Railway, "west"`). A
  filtered list says how many rows are left with a way back
  (`ListResultSummary`: "3 lines match. Show all lines"), announced from a
  hidden `role="status"`. Not hide-on-scroll.
- **Form fields**: Mantine `TextInput`/`Autocomplete`: label 14/500,
  description xs dimmed, error in `--mantine-color-error`.
- **Footer credits** (`OpenDataAttribution.tsx`): credit wording is licence
  text; copy it exactly, including "NationalRail" as one word.
- **Pride mode** (`PrideMode.tsx`): decorative, off by default. Each
  flag's stripes are one `--ds-pride-<flag>` variable in `globals.css`,
  read by the page bar, nav bar, site name and the picker's swatch. The nav
  mounts `PrideModeApplier`, which applies the saved mode with no control,
  and `PrideSparkles`, the flag's three-emoji sparkle scatter around the
  wordmark (`aria-hidden`, floating, static under reduced motion).
  `PrideModeSelect` on `/account/preferences` picks the flag, with a live
  preview beside it: the swatch in a round badge with the flag's spinning
  ring and sparkles, the old nav toggle's look. All of it follows
  `body[data-pride]`, so it shows logged in or out.

## States

| State | Pattern | Example copy |
| --- | --- | --- |
| Loading | A page whose fetch holds up the first paint: a `loading.tsx` skeleton of its real layout (`PageSkeleton`). Inside a page: `Skeleton` inside Suspense (`LoadingPlaceholder`); inline, `LoadingText` (a plain word beside an `aria-hidden` `Loader`) in `role="status" aria-busy` where it is the region's content. A busy button keeps a word label ("Saving") with `leftSection={<ButtonSpinner />}`. No "…". The nav's fallback is the full logged-out bar. | "Loading trends", "Searching" |
| Empty | Dimmed text saying what would appear, with a route out. No illustrations. | "Nothing is affecting this line right now." |
| Error (page) | `app/error.tsx`: h1 at h2 size, dimmed cause, "Try again", home link, xs "Reference:" digest | "Couldn't load this page" |
| Error (inline) | Red Alert, title names the failed action | "Couldn't save this ticket" |
| Not found | h1 at h2 size, cause, three always-underlined way-out links | "There's no page at this address." |
| Stale / offline | Bottom-centre `Notification loading`, `role="status" aria-live="polite"`. The offline page (`public/offline.html`) shows the last successful load as "Last connected 4 min ago." | "Reconnecting" |
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
- Relative time: "just now", "4 min ago", "3 hr ago", "2 days ago"
  (`lib/relativeTime.ts`), with the exact time in a tooltip. A page with
  many cards states "Updated" once, not per card.
- Delays: light badges, sentence case, "*n* min late" / "*n* min early" /
  "On time". A line that runs no trains says "No trains running", not
  zero figures. Journey rollups rank good < awaiting < delayed < unmatched <
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
- A toggle has a fixed accessible name and states its value separately.
  A choice of several is a labelled radio group (the colour theme: a
  `SegmentedControl` named "Colour theme").
- Sort indicators are SVG chevrons, `aria-hidden`, with `aria-sort` on
  the header; no ▲▼↕ glyphs.
- Email addresses render through `EmailLink`, which server-renders no
  address so Cloudflare's Email Obfuscation leaves it alone.
- Checked by `e2e/accessibility.spec.ts`.

## Writing

Copy rules are checked in CI: ESLint's `no-restricted-syntax` copy rules
(`frontend/eslint.config.mjs`) and `scripts/check-copy.py` (banned
phrases, Title Case headings, meta descriptions, pages without metadata).

### Who we write for

Commuters first, with the detail on hand. Plain language everywhere.
Enthusiast detail (headcode, UID, sample counts, data sources) stays
available but labelled and secondary: small dimmed text, a tooltip or a
"How this is calculated" disclosure, never the headline.

- A train is named by its departure time from its origin and its route:
  "08:42 Woking to London Waterloo" (`lib/trainName.ts`). The headcode and
  UID go underneath in small text, labelled: "Headcode 1S00 · UID W12345".
  A train search row shows the headcode the same way on its details line
  ("From Reading · Headcode 1S00").
- Never lead with an internal identifier ("Train W12345", "Tracking Train
  42").

### Tone

Terse and factual, like a departure board. Say what is true and what to
do next. No chatter, no apologies, no exclamation marks, no marketing
words, no em dashes: write two sentences, or use a comma, a colon or
parentheses.
Second person ("your trains"), and "we" or "Distant Signal" for the site,
never "this app".

- Sentence case for every heading, nav label, button, badge and page
  title. The only Title Case is an official status name named in running
  text: "7 more lines are not at Good Service" (`severityLabel` in
  `lib/severity.ts`). On a badge the same status
  is sentence case ("Good service", `severityBadgeLabel`).
- A page's h1 is its nav label: Track, Plan a journey, Status, Lines,
  Operators, Stations, Incidents, Ask about trains, Preferences. A page
  linked from a hub takes the link's words ("Find a train").
- Helper text is one line. If the detail matters, put it behind a "How
  this is calculated" disclosure.
- Meta descriptions: one plain sentence, 20 words or fewer, at most 160
  characters (`lib/pageMetadata.ts`).

### Failures

Every failure reads "Couldn't <verb> <noun>." plus what to do next, built
by `describeFailure(verb, noun, status)` in `lib/failure.ts`:

| Case | Copy |
| --- | --- |
| Default, 5xx, network | "Couldn't load this train. Try again." |
| 401 | "Couldn't save this ticket. Log in and try again." |
| 403 | "Couldn't delete this group. You don't have access." |
| 404 | "Couldn't load this journey. It may have been removed." |
| 409 | "Couldn't rename this train. It changed in the meantime. Reload and try again." |

Never show a raw response body, a status code, `error.message`,
"Something went wrong", "Request failed" or "Please try again". The one
exception: a 400/403/409/422 whose body is one of the API's own sentences
(`failureFromResponse`), shown as a sentence.

### Terminology

| Say | Meaning | Don't say |
| --- | --- | --- |
| train | What a commuter catches; named by time and route | service (in headings), Train {UID} |
| service | A timetabled run, in explanations only ("a timetabled service") | schedule, CIF row |
| headcode | The 4-character signalling ID ("1S00"), secondary text only | identity, train ID |
| UID | The timetable's 6-character ID ("W12345"), secondary text only | id, uid |
| operator | A train company, by name ("South Western Railway") | TOC, operator code alone |
| track | Follow a train or journey live | pin a train |
| pin | Keep a line, station or operator on your home page | track a line |
| log in / log out | Account access | sign in, sign-in |
| Delay Repay | The compensation scheme, two words, capitalised | Delay-Repay, delay repay |
| platform | Where the train calls | track, stand |
| calling point | A station a train stops at | stop (in tables), TIPLOC |
| station code | The 3-letter code ("WOK") | CRS, CRS code |
| ID | An identifier, capitalised | id, Id |
| AI summary | The one label on AI-shaped content | AI, AI-generated |
| saved in this browser | Local storage | localStorage |
| National Rail live board / Network Rail train reports | Data sources | Darwin, LDBWS, TRUST, Knowledgebase |

### Units and dates

| Thing | Format | Example |
| --- | --- | --- |
| Minutes | number, space, "min" | "4 min late", "Avg delay 2.5 min" |
| Relative time | `lib/relativeTime.ts` | "just now", "4 min ago", "3 hr ago", "2 days ago" |
| Clock time | 24-hour, UK time | "08:42" |
| Date | `lib/dateFormat.ts`, D MMM YYYY; three-letter months ("Sep", never en-GB Intl's "Sept") | "9 Oct 2026", "17 Sep 2026" |
| Date pickers | `valueFormat="D MMM YYYY"` on every `DatePickerInput` | "9 Oct 2026" |
| 24-hour hint | Once per form, on its first time field (`clockHint`) | "24-hour clock, e.g. 19:00." |

### Banned phrases

On screen, never: "—" (em dash), "…" (ellipsis), "→", "←", "↗", "--", "Something went wrong", "Please try again",
"Could not", "Request failed", "this app", "ingest", "aggregator",
"full coverage", "TOC", "deployment", "allowlist", "localStorage",
"tracking pin", "propagated", "extraction", "sign in", "signed in",
"sign-in", "CRS", "CRS code", "--:--", "seamless", "effortless",
"powerful", "revolutionary", "supercharge", "unlock", "game-changer",
"cutting-edge", "best-in-class", "delightful", "magic".
`scripts/check-copy.py` holds the list. The em dash is banned in every
string the app holds, not only the ones the checker recognises as copy,
and ESLint flags it in any string literal or JSX text. So is the ellipsis.

### Other rules

- Use rail terms people know ("platform", "calling point", "Delay
  Repay"). UK spelling.
- Be honest about uncertainty: "(no arrival report received)", "Estimate
  (Network Rail)".
- Credits use each licence's exact wording; the site calls itself "an
  independent, unofficial service" and uses no provider logos.
- No em dash "—" anywhere in UI copy, and never "--". Rewrite the
  sentence: "Log in (needs a Distant Signal account)", "Cancelled. Last
  confirmed at Woking.", "Country: showing all".
- An en dash "–" only for ranges: "08:00–09:00", "15–29 minute delays".
- A code with its name is "Name (CODE)" ("Woking (WOK)", `stationLabel`),
  or code first with a middle dot where the picker shows codes first
  ("WOK · Woking", `codeStationLabel`, operator pickers). A label and its
  value take a colon ("Lifts: Available").
- An empty table cell: words where there is room ("No data", "None"). In
  a narrow numeric column an en dash "–" keeps the column scannable; mark
  it `aria-hidden` with a visually hidden "No data" beside it
  (`NO_VALUE` in `app/lines/AllLinesTable.tsx`). A missing time on a
  service row is the same en dash with a visually hidden "No time"
  (`NO_TIME` in `lib/dateFormat.ts`), never "--:--".
- No ellipsis "…" in copy. In-progress text is a plain word and a spinner
  ("Saving", "Loading trains"); a menu item that opens a dialog is just
  its action ("Delete line"); a select option is a noun ("Custom dates").
  Text cut short by CSS (`text-overflow: ellipsis`) is fine: that is
  layout, not copy.
- No engineering vocabulary on screen: no raw enum values, field names,
  bare codes, ISO dates or `error.message`. Map through
  `lib/displayLabels.ts` and `lib/stationLabel.ts`.
- Show the working: "2 of 10 sampled services delayed" beats "Some delays".

## Do and don't

- **Do** `describeFailure()` for every failure. **Don't** show a response
  body, a status code or `error.message`.
- **Do** `pageMetadata(title, description)` for a page's metadata.
  **Don't** append " · Distant Signal" by hand; the root template does
  (`fullTitle()` where a title must carry it).
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
  `var(--mantine-spacing-*)`); app CSS tokens are `--ds-*`, app JS tokens
  are `tokens` in `lib/theme.ts`.
- **No inline `style` props.** ESLint's `react/forbid-dom-props` and
  `react/forbid-component-props` reject `style` in `app/`, `components/`
  and `lib/` (tests are exempt). In order of preference use: a shared
  component; a Mantine style prop (`miw={0}`, `flex={1}`, `mt="auto"`,
  `maw="70ch"`, `fz`, `lh`, `c`, `display`); a token; one of the small
  helpers in `globals.css` (`.ds-nowrap`, `.ds-pre-wrap`,
  `.ds-break-anywhere`, `.ds-plain-list`, `.ds-self-start`,
  `.ds-pass-through`, `.ds-clickable`, `.ds-column`); or a named class
  there. The allowlist is per line, an
  `// eslint-disable-next-line react/forbid-component-props -- <why>` for a
  value only known at render time that CSS can't express. Today there are
  three, all in `JourneyProgress`: the stop counts as CSS variables, the
  node-slot CSS variable, and each node circle's state-dependent size and
  colours. One file is exempt as a whole, in `eslint.config.mjs`:
  `lib/previewImage.tsx`, whose JSX next/og renders to a PNG and which can
  only be styled with style objects.
- Global CSS in `app/globals.css`, as BEM-ish classes or `data-*` hooks
  (`[data-text-link]`, `[data-card-link]`, `[data-status-badge]`).
- Theme config is one object in `lib/theme.ts`, shared by the app and tests.
- Icons are hand-inlined 16px Feather-style SVGs (`stroke-width 2`,
  `currentColor`, `aria-hidden`); no icon library *(convention)*. No
  Unicode glyphs as functional icons.
- Prettier 3: `printWidth: 120`, `singleQuote: true`.

## Visual regression

`frontend/e2e/visual.spec.ts` screenshots home, Status, Lines, a line,
Stations, a station, Trains (empty and searched), a train, Incidents, the
plan form (Depart at and Arrive by), Account (logged in),
Preferences (logged out, logged in and with a pride flag) and the tram
pages at 390 and 1366px, light and dark, plus the /lines filter bar's
stuck and open states and the /incidents filters open: 96 images in `frontend/e2e/visual-baselines/`. It runs against the seeded e2e
stack (`frontend/e2e/fixtures/seed.sql`), in its own Playwright project
(`visual`), which the ordinary e2e run skips.

- **Deterministic.** The browser clock is fixed (2026-09-17T11:00Z, the
  seed's day), reduced motion is on and animations, transitions and the
  caret are off, and each shot waits for the network to go quiet and fonts
  to load. Text tied to the server's own clock (freshness lines, "4 min
  ago", date-and-time stamps, date and time inputs, the trains date hint)
  is masked.
- **Threshold.** A shot fails when more than 1% of its pixels differ by more
  than Playwright's 0.2 colour tolerance. Font anti-aliasing between runs
  moves a few dozen pixels at most.
- **Always in the pinned container.** `scripts/visual-tests.py` runs the
  spec inside the Playwright image matching the locked `@playwright/test`
  (`mcr.microsoft.com/playwright:v<version>-noble`), locally and in CI, so
  fonts and rendering match. Outside it every image differs.
- **CI.** The `frontend-e2e` job runs it after the e2e specs, on every PR
  and push that touches `frontend/` (`scripts/paths-changed.py` decides,
  and says "changed" when it can't tell). Failed runs upload the
  expected, actual and diff images in the Playwright report artifact.

To run it locally, start the api and `npm run build && npm run start` the
frontend with the `frontend-e2e` job's environment, then
`uv run scripts/visual-tests.py` (set `E2E_BASE_URL` and the job's other
`E2E_*` variables). Diffs land in `frontend/test-results/`.

To update the baselines after an intended visual change, run
`uv run scripts/visual-tests.py --update -- --update-snapshots=all`, look at
every changed image in `frontend/e2e/visual-baselines/`, and commit them
with the change. A plain `--update` rewrites only the images over the
threshold. Bumping `@playwright/test` changes the image, so regenerate them
all then.

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

Wave 1 site polish (9 Oct 2026): sentence case and h1s matching nav
labels; a title template and short meta descriptions; `describeFailure`;
the plain-language terminology above; trains named by time and route;
minimal badges; "4 min ago"; the CI copy guardrails.

Handled on other branches at the same time: `ScheduleRow` delay format and
contrast, unlabelled loading skeletons, cookies/privacy heading levels, and
Unicode glyph icons in `ShareJourneyLinkButton`, `GroupInviteLinkCard` and
`JourneyProgress`.

Round 2, phase 1 (12 Oct 2026): `ConfirmActionButton` replaces 15
hand-copied confirm modals (promoting a member now confirms, like
demoting); `LoginControl` merges `LoginLink` and `LoginButton`;
`CardLink`, `ButtonLink` and `CopyableLink`/`useShareOrCopy` replace the
copied link-card style, `<Link><Button>` nesting and five copy
implementations; `DELAY_TONE_COLOR` is the one delay colour table; design
tokens in `lib/theme.ts`; the inline `style` ban (99 to 3); visual
regression screenshots in CI; "station code" and "log in" everywhere,
including API error text; no "--:--".

Round 2, phases 2 and 3 (14 Oct 2026): `/account/preferences` holds the
colour theme (out of the nav), the pride flag picker, notifications (off
the home page) and the Claude API key (out of `/chat`); one Track entry in
the nav replaces "Track a journey", "Trains" and "My trains & tickets",
with Plan a journey in the bar from `md` up and `/trains` titled "Find a
train"; the signal-arm wordmark and per-page link previews; `StatusCard`,
`StatusTile`, `StateBadge` and `BadgeRow`; sentence-case severity group
and impact labels; station accessibility key facts; themed charts; page
skeletons; and the largest forms split into smaller modules.
