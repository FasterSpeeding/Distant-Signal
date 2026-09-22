# UX / accessibility / usability review — status dashboard, lines list, line detail & history (2026-09-22)

Screenshot-based review of the operator-overview feature's `/status`, `/lines`,
`/lines/[id]` and `/lines/[id]/history` surfaces. Screenshots are the ones in
`frontend/e2e/screenshots/output/` listed in `manifest.ndjson` (all
unauthenticated; mobile = 390×844, tablet = 768×1024, desktop = 1440×900;
Chromium unless stated). Where a screenshot alone couldn't settle a question
(is that card a link? where does "Unknown station" come from?) the feature's
source in the `integration-preview` worktree was read for confirmation, and
the file is cited. No code was changed.

**Severity scale.** Critical / Important / Minor, as requested. The app's
two earlier reviews (`2026-09-17-full-service-ux-accessibility-usability-review.md`,
`2026-09-02-frontend-ui-ux-review.md`) use *serious / moderate / minor*;
read Critical ≈ serious and Important ≈ moderate when collating. The bar
applied throughout is the one those reviews already set for this app —
colour never carries status alone, no status badge ever truncates, every
empty state has a way out, copy is quantified and free of engineering
vocabulary, ≥24px touch targets, times say when the data was last updated.

---

## 1. Summary

Nothing here fails a WCAG success criterion outright on the evidence
available, and several things are done well: the status badges are the
existing `StatusBadge` (filled, `autoContrast`, colour + uppercase text),
the mobile layouts reflow without page-level horizontal overflow, headings
are correctly levelled, filter state is spelled out in text ("Status — Severe
Disruption"), and the generic error boundary that the broken LNER history
page fell back to is a genuinely good fallback.

The findings cluster into four themes:

1. **The dashboard's primary controls don't look like controls.** The five
   severity counter tiles and every "Lines to watch" row are `<Link>`s, but
   they are styled `color: inherit; text-decoration: none` with no chevron,
   hover cue or link colour — five identical white cards. The tiles also
   carry no severity colour or icon at all, so "2 Severe Disruption" is
   visually indistinguishable from "3 Good Service"; the worst bucket is
   last in reading order and, on mobile, orphaned alone on a third row.
   (Important.)
2. **The new "Trains running today" panel leads with a placeholder.** On
   tablet and desktop, 11 of the 12 visible rows read `HH:MM · Unknown
   station` — the live record's origin is unresolved, so `routeLabel()`
   emits its fallback as the row's only content. The schedule-side calling
   points are present on every entry and are what the row's time already
   comes from; they should name the route. (Important, arguably the
   single biggest usability gap in this set.)
3. **`/lines` on mobile hides the Pin column behind an unsignalled
   sideways scroll**, and the status-group chips don't update the URL, so
   a visitor arriving from a dashboard tile at `/lines?statusGroup=severe`
   and tapping "All statuses" is left with a URL that disagrees with the
   page. (Important.)
4. **Small copy/semantics problems** that contradict the app's own
   quantified-honest-copy convention: a "By mode" card that says
   `ALL GOOD SERVICE` for `0 lines tracked`; a yellow "5 affected" badge
   when two of the five are Severe; no last-updated time anywhere on a
   page that says "right now"; an empty history state with no way out.

---

## 2. Status dashboard — `/status`

Screenshots: `status-dashboard-mobile.png`, `status-dashboard-tablet.png`,
`status-dashboard-desktop.png`, `status-dashboard-desktop-firefox.png`.
Source: `frontend/app/status/page.tsx` (integration-preview worktree).

### 2.1 · Important · Counter tiles and "Lines to watch" rows have no link affordance

All three viewports show five bordered white cards (count + dimmed label)
and five bordered white rows (name + badge). In the source every one of
them is a `next/link` wrapping a Mantine `Card` with
`style={{ textDecoration: 'none', color: 'inherit' }}`
(`SeverityCounterTile`, `WorstLinesSection`). Nothing in the rendered
output says so: no link colour, no chevron/arrow, no "View lines" text, no
visible hover or pressed state (screenshots are static, but there is no
styling in the source for one either). The app's established affordance
for "this takes you somewhere" is grape link text (`/lines` line names,
`View history`, `View live status`, `Back to line` — all visible in the
sibling screenshots). The dashboard is the one page whose entire purpose
is drill-down, and its drill-down controls are its least link-like
elements.

Two knock-ons worth naming:

- **Accessible name.** The tile link's name is its text content, e.g.
  "3 Good Service" or "0 Informational". That is a count, not a purpose.
  The `0 Informational` tile links to a `/lines` table filtered to zero
  rows (`status-dashboard-*`; contrast `lines-filtered-severe-desktop.png`
  for what a filtered result looks like) — an empty destination behind a
  control that doesn't say it's a control.
- **Focus indicator (needs live verification).** A raw `<a>` (inline)
  wrapping a block `Card` is exactly the shape where the browser's default
  `:focus-visible` outline fragments or vanishes. Mantine's focus ring is
  applied to Mantine interactive components, not to a bare anchor. Keyboard
  users may get no visible focus on the page's ten primary links. Worth an
  axe/keyboard pass before this ships.

**Recommendation.** Give the tile a visible affordance the app already
uses: either render the label as a grape `TextLink`-style line ("View 2
lines →") under the count, or make the whole card a Mantine
`UnstyledButton`/`Card component={Link}` with a hover border-colour change
and Mantine's focus ring, plus an `aria-label` of the form "2 lines with
Severe Disruption — view in All Lines". Do the same for the worst-lines
rows, or — simpler — reuse `LineStatusCard`, which is already a link with
the right shrink rules and carries the sample summary and last-updated
that these rows currently drop (see 2.4). Don't link a zero-count tile, or
render it as a muted non-link with "none" in place of "0".

### 2.2 · Important · Tiles carry no severity signal; worst bucket is last and orphaned on mobile

The five tiles are text-only and identical in treatment. Everywhere else
in this app a severity is shown as colour **and** text together
(`StatusBadge`; the 2026-09-17 review §5 records the measured contrast
work that made that safe). Here it is text only — which passes the
colour-alone test trivially, but at the cost of glanceability: the page's
own metadata promises "how many are running a Good Service versus facing
disruption … at a glance", and at a glance all five tiles read as the
same thing.

Ordering compounds it. `SEVERITY_GROUPS_BY_RANK` is ascending, so the
strip reads Good → Informational → Planned → Minor → Severe. A traveller
asking "is anything wrong?" reads left-to-right/top-to-bottom and reaches
the answer last. On mobile (`status-dashboard-mobile.png`, 2-column grid)
"2 Severe Disruption" sits alone on a third row, bottom-left, ~330px below
the heading; on tablet (`status-dashboard-tablet.png`, 3-column) it is the
last cell of a 3+2 layout. Five items in a 2- or 3-column grid always
leaves an orphan; which tile is orphaned is a choice.

**Recommendation.** Keep the text label (it is the accessible half), and
add the severity's existing badge colour as a non-sole cue — a coloured
left border or top rule, or a small `StatusBadge`-coloured dot before the
label — so the strip reads as the same palette the badges below use.
Order worst-first (Severe, Minor, Planned, Informational, Good), matching
`worstFirst` in the list below it; if the orphan can't be avoided, make it
"Good Service" rather than "Severe Disruption". Alternatively at `base`
use a single-column list of five rows (count, label, coloured cue) —
shorter than the current 3-row grid and with no orphan.

### 2.3 · Important · "By mode" cards mis-state the situation in two ways

`status-dashboard-tablet.png` / `-desktop.png` / `-desktop-firefox.png`,
bottom section (`ModeCard`):

- **TfL: `ALL GOOD SERVICE` (green) over `0 lines tracked`.** An empty set
  is not "all good service"; it is "nothing tracked". Rendering a green
  good-service badge for zero lines is the kind of false reassurance the
  app's copy elsewhere is careful to avoid ("You haven't pinned any
  stations yet", "No trains are scheduled on this line today"). It also
  contradicts the page's own subtitle, "8 lines tracked across National
  Rail and TfL right now", when there are no TfL lines. Show a neutral
  "No lines tracked" state (grey, no badge) or hide the card, and make the
  subtitle honest about which modes are present.
- **National Rail: `5 AFFECTED` in yellow** when two of those five are
  Severe (`SEVERE DELAYS`, `PART SUSPENDED` are visible in the list
  directly above). Yellow is the app's *Minor Disruption* colour (see
  `MINOR DELAYS` on the same screenshot), so the card's colour understates
  the list it summarises. The text is present so this is not colour-alone,
  but it is the wrong colour. Colour the badge by the *worst* severity in
  the slice (reuse `worstStatus` over `reports`, which the card already
  computes for `affected`), or drop the colour and show "5 of 8 affected"
  as plain text.
- **Contrast (needs measurement).** These two badges are Mantine
  `variant="light"` (coloured text on a 10%-tint background). The app's
  status badges are `filled` + `autoContrast` and were measured in the
  2026-09-17 review; `light` yellow (yellow-8 text on a pale yellow tint)
  was not, and at ~11px bold uppercase it is likely in the 3–4:1 range,
  below AA for normal text. Either measure it or switch to the same
  `StatusBadge` mechanism the rest of the page uses.

### 2.4 · Minor · "Lines to watch" rows reinvent `StatusRow`/`LineStatusCard` and drop information

The row is `Group justify="space-between"` (wrapping allowed) around a
`Text` and a `StatusBadge`. On mobile (`status-dashboard-mobile.png`) the
LNER row's badge wraps to a second line while the Northern row's fits on
one, so row heights vary — a fair trade (nothing truncates, which is the
house rule from 2026-09-17 §2.5), but the app already has `StatusRow`
(documented shrink rules, consistent height) and `LineStatusCard` (same
link + badge, plus the sample summary and last-updated) for exactly this
row shape. Compared with the homepage's `RightNowModule` card that the
design spec §A said to mirror, these rows lose "21 of 48 sampled services
delayed…" and the freshness line. A traveller deciding between two
Severe lines gets no basis to choose.

### 2.5 · Important · The page says "right now" but never says when

Subtitle: "8 lines tracked across National Rail and TfL right now."
No timestamp, no "updated 2 min ago", no timezone — on any viewport. The
page is served through `withStaleFallback`, so "right now" can legitimately
be several minutes old. The 2026-09-17 review §2.12 raised precisely this
("nothing ever says when the data was last updated") as moderate across
the app; the new page inherits the gap rather than fixing it. Add the same
last-updated line `LineStatusCard` already shows, once, under the subtitle.

### 2.6 · Minor · Firefox parity

`status-dashboard-desktop-firefox.png` is layout-identical to Chromium.
The only differences are the already-documented ones (2026-09-17 §2.15:
dashed nav rule degrades to a slightly heavier line; system typeface).
Nothing new.

### Working well on `/status`

- Heading structure (`h1` Network Status → `h2` Lines to watch → `h2` By
  mode) is correct and the mobile hamburger nav keeps the first viewport
  mostly content (a clear improvement over the 22–35% header of §2.2).
- The list badges are the real `StatusBadge`: filled, uppercase text,
  autoContrast-checked colours; `PART SUSPENDED`/`SEVERE DELAYS` share one
  red and `MINOR DELAYS` is a clearly different yellow. Keep.
- Nothing overflows horizontally at 390px; the grid degrades 5 → 3 → 2
  columns sensibly (ordering aside, 2.2).
- The "By country" section self-hides when only one country exists rather
  than showing a one-row breakdown — the right call.

---

## 3. All lines table — `/lines`

Screenshots: `lines-unfiltered-mobile.png`, `lines-unfiltered-tablet.png`,
`lines-unfiltered-desktop.png`, `lines-filtered-severe-desktop.png`.
Source: `frontend/app/lines/AllLinesTable.tsx`, `frontend/app/lines/page.tsx`.

### 3.1 · Important · Mobile hides the Pin column behind an unsignalled horizontal scroll

`lines-unfiltered-mobile.png` shows two columns, Name and Status, with the
`PART SUSPENDED` badge flush against the right edge and no Pin column. The
source explains why: the mobile table is Name + Status + Pin inside a
`TableScrollContainer minWidth={420}` on a 390px viewport, so the Pin
column (and the badge's right margin) sit ~30px off-screen, reachable only
by swiping the table sideways. Nothing signals that: no edge fade, no
visible scrollbar, no cut-off header cell. The 2026-09-02 review's F4
("Mobile users cannot pin from the All Lines table at all") was recorded
as fixed in 2026-09-17 §5 ("keeps the pin star"); in this capture the star
is not visible, so a mobile visitor who doesn't discover the scroll is back
to F4. The `TableScrollContainer` is the right tool for a *wide* table; it
is the wrong tool for a table that is 30px too wide because of one fixed
column.

**Recommendation.** Make the mobile layout fit 390px without scrolling:
move the pin star into the Name cell (right-aligned on the name's first
line), or shrink the Pin column to the icon's 44px hit area with no header
text, or let the badge sit under the name on its own line as the numeric
sub-line already does. If the scroll container must stay, add the standard
scroll affordance (a right-edge fade or a visible "swipe for more" cue) —
but fitting is better.

### 3.2 · Important · URL and chip state diverge after arriving from a dashboard tile

`lines-filtered-severe-desktop.png` is `/lines?statusGroup=severe` and
correctly shows the "Severe Disruption" chip filled, the label reading
"Status — Severe Disruption", and two rows. But `statusGroup` only *seeds*
client state (`initialStatusGroup` → `useState`; the comment in
`page.tsx:19-28` says so deliberately). Consequences for the dashboard →
lines journey the feature exists to provide:

- Tap the "Severe Disruption" tile, then tap "All statuses": the table
  shows all lines, the URL still says `?statusGroup=severe`. Refresh or
  share, and the severe filter comes back.
- Browser Back from a chip change does nothing (no history entry), while
  Back from the tile link returns to `/status` — two different meanings of
  Back on the same control set.
- The page `<title>` is always "All Lines" even when the tile link lands
  on a two-row filtered view; the code comment acknowledges this and
  argues that filtered links "are not the link people paste". With the
  dashboard now generating those links on every visit, that assumption no
  longer holds.

**Recommendation.** Mirror the chip to the URL with `router.replace`
(`scroll: false`), read it with `useSearchParams`, and let the title
reflect it. This is a few lines and removes the drift entirely.

### 3.3 · Minor · Filtered view gives no count and no way back

`lines-filtered-severe-desktop.png`: two rows, no "2 of 125 lines" line,
no "Clear filter" near the table. The house convention (2026-09-17 §5
"Copy is honest and specific where it counts") is quantified — "Showing
the first 5 — 7 more lines are not at Good Service". Add "2 lines with
Severe Disruption · Show all" above the table. Also worth handling the
zero-row case the `0 Informational` tile produces (2.1): "No lines are
Informational right now" with the "All statuses" chip re-offered.

### 3.4 · Minor (a11y, measure) · Filter chips are `size="xs"`

The status and country chips are Mantine `Chip size="xs"` (~23px tall in
Mantine v7). The 2026-09-17 review §2.10 set a ≥24px floor for tap
targets, 44px on primary ones. Six chips in a row at 390px
(`lines-unfiltered-mobile.png`, "Status — showing all") are the page's
primary filter; `size="sm"` (28px) costs one more wrap on mobile and
clears the floor.

### 3.5 · Minor · The `▲`/`↕` sort glyphs are the only sort-state indicator

`Name ▲` vs `Status ↕`: fine visually, but confirm `aria-sort` is set on
the `<th>` (the `UnstyledButton`-in-`th` comment suggests keyboard
sorting was considered; `aria-sort` is the remaining half).

### Working well on `/lines`

- The filter label spells the selection out in words ("Status — Severe
  Disruption" / "Status — showing all") so the filled chip is never the
  only cue. This is exactly the standard the app set for itself; keep it.
- The mobile Name cell folds Avg Delay / Cancelled into a dimmed sub-line
  ("Avg delay 14.0 min · 41% cancelled"). Clear and compact.
- Line names are grape links — the correct affordance the dashboard
  should borrow (2.1).
- Status badges do not truncate at any width (§2.5 rule honoured; the
  screenshots show full `PART SUSPENDED`).

---

## 4. Line detail — `/lines/lner-ecml`

Screenshots: `line-detail-lner-ecml-mobile.png`, `-tablet.png`,
`-desktop.png`. New content is the "Trains running today" panel
(`frontend/app/lines/[id]/LineTrainsResults.tsx`); the rest of the page is
pre-existing and was reviewed on 2026-09-17 §3.4.

### 4.1 · Important · "Trains running today" rows read `HH:MM · Unknown station`

On tablet and desktop, rows 06:00 through 16:00 — eleven of the twelve
visible — render as `06:00 · Unknown station`, `07:00 · Unknown station`,
…, `16:00 · Unknown station · 22m late`. Only rows with no live record
yet get the honest `Scheduled — not live yet`. Mechanism, from the source:
the row calls `routeLabel(live.originCrs, live.originName, …)`; when the
live record has no schedule match `originCrs` is null and `routeLabel`
returns `UNKNOWN_STATION_LABEL` (`lib/stationLabel.ts:21`) as the *entire*
label. So the panel's main content is a fallback string.

For a traveller this is the panel failing at its one job — "which train
is this?" — while looking like data. It also violates the vocabulary rule
(2026-09-17 §2.9, 2026-09-02 F5: engineering/fallback strings reaching
user copy), and it's worse than the sibling `/train` page, whose
"Unknown location" rows were just removed (`bcf7ea9d`). Note the panel's
own comment: the schedule (`callingPoints`) "is present on every entry
regardless of live-status coverage" — the time shown already comes from
`callingPoints[0]`. The first and last calling points' names are right
there.

Also ambiguous: `06:00` is the booked departure from *this line's* first
calling point, but the row never says which station, so "06:00 · Unknown
station" reads as "departs somewhere unknown at 06:00".

**Recommendation.** Build the route label from the schedule side (first
calling point → last calling point) and only *upgrade* to the live
origin/destination when both resolve. Never print "Unknown station" as a
whole row label; if the schedule is also nameless, print the headcode/UID
("Train 1A23") so the row still identifies something. Consider whether
this should be Critical: if the "no schedule match yet" state is the
common one during the day, the panel is unusable most of the time it's
looked at.

### 4.2 · Important · Past trains first, no anchor to "now", no cap

Captured at 18:09 local; the list starts at 06:00 and on desktop the
first twelve rows are all departed trains (`line-detail-lner-ecml-desktop.png`
shows 06:00–16:00 before the fold). A traveller on a Severe-delays line at
18:09 wants the next departure, and has to scroll past the day. The list
is uncapped (a main line can have 100+ services), so on mobile the
"Recent trends" charts below become effectively unreachable. Options, in
increasing effort: separate "Departed" (collapsed) from "Upcoming"; sort
upcoming first with "N earlier trains" collapsed; cap at the next 10 with
"Show all N".

### 4.3 · Minor · Delay is de-emphasised; every link says the same thing

- `· 22m late` is `c="dimmed"` — the same grey as "Scheduled — not live
  yet". Cancelled is `c="red"`. The one live fact a traveller cares about
  is styled as the least important text on the row. Use `c="red"` for
  cancellations (as now) and the app's existing delay colour with the
  text kept, matching `StatusBadge`'s colour + text pairing; or reuse
  `EtaBadge`/`TrackedTrainStatusBadge` which already encode this.
- Twelve identical `View live status` links. In a screen-reader links list
  that is 12 × the same name (WCAG 2.4.4 is met via context, 2.4.9 is
  not). Simplest fix: make the time + route the link text and drop the
  trailing link; or give each an `aria-label` "View live status for the
  06:00 to Edinburgh".
- The row is `Group wrap="nowrap"` with a `Text` and a `TextLink` and no
  `flexShrink: 0` on the link — the exact shape 2026-09-17 §2.5 flagged.
  Not observed to break in these captures (the mobile capture happened to
  have only short "Scheduled — not live yet" rows) but a long real route
  name plus "· 22m late" at 390px will squeeze the link into "View live /
  status". Reuse `StatusRow` or add the shrink guard.

### 4.4 · Minor · The panel has no context line

No date ("Monday 22 September"), no count ("48 trains"), no
last-updated, no timezone — the same §2.12 gap as 2.5. A one-line dimmed
caption under the heading fixes all four.

### Working well on `/lines/[id]`

- The three data-unavailable states ("No scheduled train data is
  available for this line today", "Today's trains aren't available right
  now", "No trains are scheduled on this line today") are distinct,
  honest and never throw — the panel fails on its own rather than
  blanking the page. This is the pattern 5.1 asks the history page to
  adopt.
- Mobile header: title wraps to two lines, then ⓘ / share / badge on one
  row; the `SEVERE DELAYS` badge is full-width-safe and never truncates.
- Tablet and desktop are consistent; nothing new overflows.
- Pre-existing issues visible here (issue-row chevron ~16px, ⓘ ~20px,
  "Filter" chrome outweighing a single issue) are already in the
  2026-09-17 review (§2.10, §2.14) and are not re-counted.

---

## 5. Line history — `/lines/[id]/history`

### 5.1 · The LNER capture is the app's generic error boundary — reviewed as a fallback

`line-history-lner-ecml-desktop.png` (and its two retries) shows
`frontend/app/error.tsx`, the app's only error boundary, because of a
confirmed data bug in this line's history. Assessed as a fallback UX:

**Good, and hold the line on it:**
- A real `h1` ("Something went wrong") — the boundary explicitly fixed
  the axe `page-has-heading-one` failure the 2026-09-02 audit found.
- No raw error message; the `digest` is shown as "Reference: 1858812559"
  — copyable, meaningful to support, meaningless to attackers. This is
  the F5 fix from 2026-09-02 holding.
- Honest, hedged copy ("It *may* be a temporary problem with the live
  data feeds") — it doesn't over-promise that retrying will work.
- A primary `Try again` button and a text link, both keyboard-reachable;
  auto-reset when connectivity returns (source, lines 55–61).
- Nav and footer survive, so the visitor is not stranded.

**Important · A single results panel failing blanks the whole route.**
The page title ("History: LNER East Coast Main Line"), the "Back to line"
link, the Period control and the Timeline/Trends tabs all disappear; the
visitor loses their place and the one obvious recovery ("go back to the
line") is not offered. The same feature's `LineTrainsResults` and the
`HalfHourlyTrendsResults` pair on `/lines/[id]` deliberately "resolve to
real markup rather than throw" so a failure is scoped to the panel. The
Timeline results do not. Add the same catch-and-render-a-Paper pattern
to `HistoryResults` (or a route-level `error.tsx` under
`lines/[id]/history/` that keeps the header and offers "Back to line").

**Minor · "Back to your dashboard" assumes a logged-in user.** The capture
is anonymous; there is no "your dashboard", and the link goes to `/`. "Back
to the home page" is true for everyone. The boundary is shared, so the
copy has to work on every route — this one currently doesn't.

**Minor · Copy attributes a deterministic bug to "live data feeds".** With
a per-route boundary (above) the message could be specific ("Couldn't load
this line's history"); as the global fallback the hedge is acceptable.

### 5.2 · CrossCountry — the working page, with an empty Timeline

`line-history-cross-country-desktop-check.png`.

**Minor · The empty state has no way out and slightly engineering-y copy.**
"No history entries in that range." is honest but ends there. The house
standard (2026-09-17 §5) is "every empty state with a way out". The
control that would change the answer — the Period segmented control — is
40px above, and nothing points at it; nor does the copy say what
"history" means or why a line that is currently `MINOR DELAYS` has no
entries in 7 days (retention? no change?). Suggest: "No status changes
recorded for CrossCountry in the last 7 days. Try 30 days, or see the
Trends tab for delay and cancellation rates." — and link "30 days" and
"Trends" so the way out is one click.

**Minor · Full-width segmented control.** Three options stretched across
1100px at desktop puts the labels ~370px apart; a `w="fit-content"` or
`maw` keeps the group readable as one control. Selected state is filled
grape with white text plus the label, so it is not colour-alone. Fine on
narrower widths (not captured, but the same `SegmentedControl` compresses
without clipping on `/incidents` per §5).

**Working well:** "Back to line" is a real underlined link placed as a
breadcrumb above the `h1`; the Period control carries an
`aria-labelledby` (source) and a visible "Period" label; the Tabs are
Mantine Tabs with a visible selected underline; the page never shows a
raw empty table or a spinner-forever.

---

## 6. Cross-cutting notes

- **Consistency across viewports** is good on all three pages: no
  page-level horizontal overflow at 390px anywhere in this set, grids
  step down sensibly, and the mobile hamburger nav is a clear improvement
  over the sweep on 2026-09-17. The one reflow that misfires is the
  `/lines` table's scoped scroll (3.1).
- **Colour + text** is honoured everywhere a `StatusBadge` is used. The
  two places that stray are both on `/status`: tiles with *no* colour
  (2.2, a glanceability cost, not an a11y failure) and "By mode" badges
  with the *wrong* colour and an unmeasured `light` variant (2.3).
- **Last-updated / timezone** (2026-09-17 §2.12) is still absent on every
  new surface: `/status` (2.5), the trains panel (4.4).
- **Reuse.** The feature reimplements three row shapes the app already
  has documented primitives for (`StatusRow`, `LineStatusCard`,
  `EtaBadge`/`TrackedTrainStatusBadge`); each reimplementation dropped a
  rule the primitive encodes (shrink guard, hover/focus, freshness line).
  The 2026-09-17 review's "this is a convention problem, not three bugs"
  applies again.

---

## 7. Recommendations (actionable, in priority order)

1. **Trains panel: build the route label from the schedule** (`callingPoints`
   first → last) and only override with live origin/destination when both
   resolve; never render "Unknown station" as a whole label. — 4.1, Important
2. **Dashboard tiles and rows: make links look like links.** Card-as-link
   with hover/focus styling (or a visible grape "View N lines →" line),
   descriptive `aria-label`s, no link on zero counts, verify keyboard focus
   ring. — 2.1, Important
3. **Dashboard tiles: add a non-sole severity cue and order worst-first**
   (coloured rule/dot per tile using the badge palette; Severe first;
   avoid orphaning Severe on mobile). — 2.2, Important
4. **`/lines` mobile: fit the table at 390px** (pin star into the Name cell
   or a 44px icon-only column) instead of a 420px scroll container with no
   scroll cue. — 3.1, Important
5. **`/lines`: mirror the status chip to the URL** (`router.replace`,
   `scroll:false`) and let `<title>` reflect the filter. — 3.2, Important
6. **"By mode": neutral state for zero lines** ("No lines tracked", no
   green badge); colour the "N affected" badge by the worst severity in the
   slice or drop colour; measure or replace the `light`-variant badges. —
   2.3, Important
7. **History page: scope failures to the results panel** (catch and render
   a `Paper`, as `LineTrainsResults` does) so the header, Back link, Period
   and Tabs survive; make the global boundary's link "Back to the home
   page". — 5.1, Important / Minor
8. **Add one last-updated line** under the `/status` subtitle and under
   "Trains running today", using the freshness line `LineStatusCard`
   already renders. — 2.5, 4.4
9. **Trains panel: upcoming-first with departed trains collapsed, and a
   cap with "Show all N".** — 4.2, Important
10. **Trains panel row polish:** delay in colour + text (not dimmed);
    time+route as the link (drop 12× "View live status"); shrink guard on
    the trailing link or reuse `StatusRow`. — 4.3, Minor
11. **Filtered `/lines`: quantified caption + clear-filter link**, and a
    zero-row message. — 3.3, Minor
12. **History empty state with a way out** (link "30 days" and "Trends"
    from the copy); constrain the Period control's width. — 5.2, Minor
13. **Chips to `size="sm"`** to clear the 24px floor; confirm `aria-sort` on
    sortable headers. — 3.4, 3.5, Minor
14. **Reuse `LineStatusCard` for "Lines to watch"** so the rows regain the
    sample summary and freshness line and inherit the documented shrink
    rules. — 2.4, Minor
