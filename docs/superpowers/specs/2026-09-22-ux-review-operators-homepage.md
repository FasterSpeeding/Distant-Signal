# UX / accessibility / usability review — operators list, operator & network history, homepage pinning (2026-09-22)

Scope: the `/operators` list page (logged out, logged in, Firefox), the
`/operators/[code]/history` and `/network/history` Trends pages, and the
homepage's "Your Lines / Your Stations / Your Operators" pinning sections,
as captured in `frontend/e2e/screenshots/output/` (all 1440×900, Chromium
unless noted; see `manifest.ndjson`). The feature code reviewed lives on
branch `worktree-agent-aa9d33a8a0709d13a` (HEAD `e19af671`); file paths
below are relative to that tree. Design intent is taken from
`docs/superpowers/specs/2026-09-22-operator-overview-design.md` (§C, §D, §E);
the bar applied is the one already set by the 2026-09-02 and 2026-09-17
reviews (colour never alone, no engineering vocabulary in copy, one idiom
for "pick exactly one", 24/44 px targets, labelled controls).

Screenshots are viewport captures, not full-page, so anything below 900 px
(the rate chart's lower half on both history pages, the LNER operator card
on the logged-in homepage) was checked against source rather than pixels.

## 1. Summary

The new pages are visually consistent with the rest of the app: same page
shell, heading scale and padding as `stations-desktop.png`, the same
`StatusBadge` (colour + uppercase text, never colour alone), the same
`PinToggle` (44 px, `aria-label` that states action and state, tooltip),
and an operator card that is a faithful sibling of `LineStatusCard`. The
logged-out homepage requirement ("nothing shown unless something is
pinned") is met and reads as intentional, because the anonymous page is
byte-for-byte the existing "Right now" homepage. Cross-browser rendering in
Firefox is identical apart from the pre-existing font fallback.

The problems are structural rather than cosmetic:

- **Critical — both history pages are unreachable from the UI.** Nothing
  links to `/operators/[code]/history` or `/network/history`; the operator
  card is deliberately not a link and carries no history link. The two most
  expensive new pages can only be reached by typing a URL.
- **Important — the operator card is a dead end that looks clickable.** It
  has the exact shape of `LineStatusCard`, which *is* a link everywhere
  else, so users (especially on the logged-in homepage, where line cards
  and operator cards sit 300 px apart) will click it and nothing happens.
  There is also no way to see which of an operator's lines is driving the
  rollup.
- **Important — the granularity control has no label**, visible or
  programmatic, and its selected-state styling differs from the "Period"
  control directly above it (2026-09-17 review §2.13 already asked for one
  idiom here).
- **Important — the Trends explanatory copy is a five-line methodology
  paragraph** with `--` for dashes, ISO axis dates and, on the network page,
  a missing space ("running.Rates"). Inherited from `/lines/[id]/history`,
  but the operator/network pages make it longer, and this is the first time
  it appears on a page without a Timeline tab to balance it.

Everything else is Minor. A closing recommendations list is in §5.

## 2. `/operators` list — `operators-list-desktop.png`, `operators-list-authenticated-desktop.png`, `operators-list-desktop-firefox.png`

### 2.1 · Critical · No route from the list to an operator's history, lines, or anything else

`operators-list-desktop.png` shows nine cards whose only interactive element
is the pin star. `frontend/components/OperatorStatusCard.tsx` documents this
as a deliberate call ("deliberately NOT wrapped in a `Link` … there is no
`/operators/[code]` detail page in this phase"), and commit `e19af671`
removed a "dead operator-detail link". But `/operators/[code]/history`
*does* exist in this phase (`operator-history-gr-desktop.png`), and a
repo-wide grep finds no `href` pointing at it or at `/network/history`
outside their own `basePath` constants. The history page's own "Back to
operators" link (`app/operators/[code]/history/page.tsx:76`) implies a
forward path that does not exist.

Net effect for a user: `/operators` answers "how is LNER doing right now?"
and then offers nothing — not the lines that make up the rollup, not the
trend, not the incidents. For the spec's own framing (§C: the list is the
entry point to "a filtered `/lines`" and §D: operator history is Phase 4's
deliverable) this is the single largest gap in the shipped UI.

**Recommend:** give every card a "History" text link in the footer row
(left of the star, or beside "Updated 8m ago") pointing at
`/operators/{code}/history`, and a "N lines" link pointing at `/lines`
pre-filtered by operator (the `AllLinesTable` operator filter already
exists client-side; if it cannot yet take a query param, link to `/lines`
unfiltered rather than nowhere). Link `/network/history` from the `/status`
dashboard and from `/lines` (its own back-link already claims `/lines` as
its parent). Add both new routes to whatever e2e nav/link coverage exists
so an orphaned page fails CI.

### 2.2 · Important · The card has a link's shape but no link's behaviour

Same shape, border, shadow and content layout as `LineStatusCard`
(compare the two rows of cards in
`homepage-operator-overview-authenticated-desktop.png`: "Your Lines" cards
are links to `/lines/{id}`; "Your Operators" cards are inert). The reason
text is clamped to three lines with an ellipsis ("Services may be delayed by
up to 40 minutes, revised or…") and there is no way to read the rest — on
`LineStatusCard` the click-through *is* the "read more". Hover gives no
pointer change (nothing to hover). This is a false affordance on the list
page and an inconsistency on the homepage.

**Recommend:** either make the card a link (to the history page for now, to
a detail page later) or visibly differentiate it — drop the shadow, or add
an explicit "History" / "Lines" link row so the card reads as a summary with
actions rather than as a button.

### 2.3 · Important · The rollup hides what it is rolling up

"London North Eastern Railway — SEVERE DELAYS — Severe delays between London
Kings Cross and Peterborough after a signalling failure…" is a single
line's reason text presented as the operator's status, with no indication
that it is the worst of *n* lines or which line it is. For an operator
running a dozen lines (spec §C says single-digit to low teens; the homepage
source says the full list is 25–40 operators) a passenger cannot tell
whether the whole network is in trouble or one branch is. ScotRail's card is
the clearest example: "PLANNED CLOSURE — Planned engineering work will close
the line between Edinburgh and Kirkcaldy next weekend" is one Fife Circle
closure presented as ScotRail's status, while "No delay/cancellation data
available for this operator" sits beneath it.

**Recommend:** add a one-line qualifier under the reason in the same dimmed
`size="xs"` style as the stats line: "Worst of 4 lines · LNER East Coast
Main Line" (line name linked). When all lines are Good Service, "4 lines,
all running normally". This is the same "say the scope" principle the
2026-09-17 review applied to station rows (F11: "station-page rows don't say
which line they belong to").

### 2.4 · Minor · Pin star drifts vertically between cards in a row

The footer row is not anchored to the card bottom, so the star sits at
different heights across a row (Avanti at y≈292 vs CrossCountry/GWR at
y≈333; Southeastern vs SWR similar). Scanning a row for "which of these are
pinned" means hunting. `LineStatusCard` on the homepage has the same
non-anchored footer, but it has no star, so the drift is invisible there.

**Recommend:** `Card` as a flex column with the footer `Group` on
`marginTop: 'auto'` (or `Stack` with `justify="space-between"` and
`h="100%"`), so the stars align.

### 2.5 · Minor · Logged-out state shows a pin control that cannot pin

`operators-list-desktop.png` (logged out) and
`operators-list-authenticated-desktop.png` (logged in) are pixel-identical
except for two filled stars. The manifest describes the logged-out shot as
"without any pinning UI available", but the star is present; only its
tooltip/`aria-label` ("Pin — needs an account") and the click-time
`LoginPromptModal` differ. This matches `/lines` and `/stations/[crs]`
exactly (`PinToggle`'s `needsAccountHint`), so it is consistent with
precedent — recorded here because the manifest expectation and the
rendering disagree, not as a defect. If the project ever revisits the
anonymous-pin affordance, do it once in `PinToggle` for all three kinds.

### 2.6 · Minor · No intro line, no ordering cue, no filter

`stations-desktop.png` opens with a one-sentence description under its
`<h1>`; `/operators` goes straight from "Operators" to cards. Cards are
alphabetical, whereas the homepage's pinned lines are explicitly sorted
worst-first ("a dashboard should lead with what needs attention",
`app/page.tsx`). Nine cards are fine alphabetically; 25–40 will not be.

**Recommend:** one dimmed sentence ("Every operator this app tracks, with
its worst current line status and today's delay and cancellation figures";
the page's own `<meta description>` already says this) and worst-first
ordering, or a small "Sort: Worst first / A–Z" control reusing the
`SegmentedControl` idiom.

### 2.7 · Minor · Nav placement

"Operators" is the last primary item, after "Incident Archive"
(`lib/navLinks.ts:34`). It is a sibling of "All Lines" and "Station Lookup"
(three ways to browse status), so it belongs beside them. Cosmetic, but
cheap to fix now while the item is new.

### 2.8 · Works well

- Status communication is exactly the app's established idiom: coloured
  badge + uppercase text, plus a plain-language sentence, plus "Avg delay
  8.7 min · 2% cancelled" in the same format `LineStatusCard` uses. No new
  vocabulary, no colour-only encoding.
- "No delay/cancellation data available for this operator." is honest and
  in the same slot as the stats line, so the card does not collapse.
- Pin toggle: same component as elsewhere, 44 px, distinct hue *and* fill
  for pinned vs unpinned, `aria-label` states action and current state,
  tooltip mirrors it. Keyboard and screen-reader users get the same
  information as sighted users.
- Firefox (`operators-list-desktop-firefox.png`) is identical apart from
  the known font fallback (2026-09-17 §2.15) and 1 px of nav-height
  difference. Nothing regressed cross-browser.
- Titles wrap to two lines cleanly ("London North Eastern Railway") with
  the badge staying top-right.

## 3. History pages — `operator-history-gr-desktop.png`, `network-history-desktop.png`

### 3.1 · Critical (shared with §2.1) · Unreachable

See §2.1. Additionally, `network-history-desktop.png`'s back-link says
"Back to all lines" and points at `/lines`, which does not link here. A
back-link to a page that has never heard of you is disorienting; if the
network view is conceptually the `/status` dashboard's history, say "Back to
status" and link from `/status`.

### 3.2 · Important (a11y) · Granularity control is unlabelled

`operator-history-gr-desktop.png`: "Period" has a visible bold label and
(`HistoryRangePicker.tsx:120-123`) `aria-labelledby`. The row beneath it —
"Hourly / 6-hourly / Daily" — has neither
(`app/lines/[id]/history/GranularityControl.tsx:60-63`: no `aria-label`, no
`aria-labelledby`, no visible `Text` label above it). A screen-reader user
tabbing in hears three radio options with no group name; a sighted user gets
the only hint from the dimmed helper text *below* the control ("30 min is
not shown for this range…"), which explains an absent option before the
present ones are named.

The two controls also use different selected-state treatments — "7 days"
is a grape-filled segment, "Daily" is a white segment with a border on a
grey track. They read as two unrelated widgets rather than two settings of
one chart. The 2026-09-17 review's §2.13 recommendation was specifically
"make the presets a `SegmentedControl` labelled 'Period', with
`color="grape"`" — the Period half has been done; the granularity half has
not been brought to match.

**Recommend:** add a visible "Granularity" (or "Show per") label with
`aria-labelledby`, and give the control `color="grape"` so both rows share
one idiom. This fix lands on `/lines/[id]/history` too, since the component
is shared.

### 3.3 · Important · Methodology copy reads as a developer note

Both pages carry a five-line dimmed paragraph (source:
`TrendsResults.tsx:27-30` `HONESTY_COPY`, plus one appended sentence per
page). Observed problems, all visible in the two screenshots:

- Double hyphens as dashes throughout: "day -- not a share of poll cycles",
  "30 min is not shown for this range -- it's wider than…". The rest of the
  app uses a real em dash ("Live UK rail line status, train tracking, and
  Delay Repay support — pin the lines…"). `--` is the source-comment idiom
  leaking into UI text.
- **`network-history-desktop.png` has "simultaneously running.Rates shown"
  with no space.** The current source (`NetworkTrendsResults.tsx:77`) has
  `{HONESTY_COPY[granularity]} Rates shown` with a space, so either the
  running instance predates a fix or JSX whitespace was collapsed; verify
  against the deployed build rather than assuming the source is what
  shipped.
- "poll cycles", "coverage", "rollup", "catalogue line" are engineering
  vocabulary (2026-09-17 §2.9 class). A passenger does not know what a poll
  cycle is.
- Axis dates are ISO ("2026-09-15") where the rest of the app prints "15
  Sep" style; the volume axis ticks are 0 / 65 / 130 / 195 / 260 (operator)
  — an odd step that suggests nice-number rounding is off.
- The rate chart's y-axis is cut off by the viewport at ~9 % (operator) and
  ~4.5 % (network); the delay-rate line hovers between 10 % and 17 %, so
  the chart reads as "one line, no others" until you scroll. Checked in
  source: cancellation and skip rates *are* plotted with distinct dash
  patterns (`TrendsCharts.tsx:157-158`) and would appear lower down, so the
  legend is not colour-only. Not a defect, but a reason to put the charts
  before the paragraph.

**Recommend:** keep one sentence in place ("Each train is counted once per
day, by the status it had when first seen. Days with too little data are
left blank.") and move the remaining explanation into a collapsed
`<details>`/Mantine `Spoiler` titled "How these rates are calculated";
replace `--` with `—`; format axis dates with the app's existing
date-formatting helper; and put the charts above the explanation so the data
leads. Since the copy is shared, this also improves `/lines/[id]/history`.

### 3.4 · Minor · No "what is this a rollup of" line near the title

"History: London North Eastern Railway" gives no sense of scope until the
last sentence of the paragraph ("summed across every line this operator
runs"). Put "4 lines" (linked) directly under the title, mirroring §2.3.
For the network page, "Every National Rail line this app tracks (TfL not
included)" under the title says in one line what the paragraph's last
sentence says in two.

### 3.5 · Minor · Title formats differ from the line history page

`/lines/[id]/history` is presumably titled by line name; the new pages use
"History: {name}" and "Network history". Minor, but pick one pattern
("LNER — History" vs "History: LNER") across all three.

### 3.6 · Works well

- Period presets (7 / 30 / Custom…) are exactly the 2026-09-17 §2.13
  recommendation, correctly reused.
- The "30 min is not shown for this range" note, and the (unseen here)
  yellow retention-shortfall `Alert`, are honest about data limits instead
  of drawing a misleading flat line — the right call, and consistent with
  the existing line history page.
- The trains-counted bar chart above the rate chart gives the rate its
  denominator; that is genuinely useful context rather than a developer
  dump. With ~260 trains/day (operator) and ~2 000/day (network) the bars
  make sample size legible at a glance.
- Skeleton fallback (`Skeleton height={320}`) while the Suspense boundary
  resolves — no unlabelled blank (2026-09-17 §2.11 class avoided).

## 4. Homepage — `homepage-desktop.png`, `homepage-operator-overview-logged-out-desktop.png`, `homepage-operator-overview-authenticated-desktop.png`

### 4.1 · Works well · "Nothing until pinned" reads as intentional, not empty

`homepage-operator-overview-logged-out-desktop.png` is pixel-identical to
the pre-feature baseline `homepage-desktop.png`: tagline, "Right now" with
five worst lines, three CTA links, footer. There is no empty "Your
Operators" stub and no hint that anything is missing — an anonymous visitor
sees a complete, useful page. The requirement is met and it does not feel
broken. The logged-in branch also generalises the existing
`bothPinnedSectionsEmpty` guard to `allPinnedSectionsEmpty`
(`app/page.tsx:453-467`), so a fresh account with nothing pinned still gets
"Right now" first — the 2026-09-02 F2 fix was carried forward, not
regressed.

### 4.2 · Important · Operator cards on the homepage are inert next to line cards that are links

Same finding as §2.2 but sharper here, because the two card types are 300
px apart and identical in shape. "LNER East Coast Main Line" (Your Lines,
top) navigates on click; "London North Eastern Railway" (Your Operators,
bottom) does not. Users will learn "cards are links" from the first section
and be wrong by the third.

### 4.3 · Minor · Only one of the three card types carries a pin control

"Your Operators" cards show a filled star (SWR at y≈874), "Your Lines" cards
show none (`LineStatusCard` takes no `PinToggle`). The star is useful
(unpin from the dashboard) but its presence on one section and absence on
the other looks like an oversight. Either add unpin to line cards on the
homepage too, or drop it from the operator card when rendered there (the
component already takes `pinned` as a prop; a `showPin` prop is one line).

### 4.4 · Minor · Copy not updated for the third pin kind

- Anonymous tagline: "pin the lines and stations you care about once you're
  logged in."
- Anonymous CTA: "Log in to pin your lines and stations" (`app/page.tsx:293`).
- Anonymous quick links: "Browse all lines · Look up a station" — no
  "Browse operators".

Each should say "lines, stations and operators" / add a "Browse operators"
link. Small, but they are the only places the anonymous homepage could
advertise that the new page exists.

### 4.5 · Minor · The same disruption is printed three times

`homepage-operator-overview-authenticated-desktop.png`: the LNER signalling
failure appears as the LNER ECML line card, as "Avg delay 18.4 min · 8%
cancelled" on the Kings Cross station row, and again in full as the LNER
operator card. This is expected for a demo user who pinned all three, and
the source already de-duplicates TfL lines against "Your Lines"; consider
the same courtesy for an operator whose *only* affected line is already
pinned (a short "See LNER East Coast Main Line above" instead of repeating
the paragraph), or at least keep the operator card's reason to one clamped
line on the homepage.

### 4.6 · Minor (pre-existing) · Heading levels

"Your Lines" is `<h1>`, "Your Stations" / "Your Operators" / "Right now"
are `<h2>` (`app/page.tsx:471,504,551,647`). The new section copies the
existing convention, so it is consistent; the convention itself gives the
page an `<h1>` that is one of three peers. Not introduced by this feature —
noted so the collated report can decide whether to fold it into the
existing homepage findings.

## 5. Consistency with `/stations` and the rest of the app

`stations-desktop.png` vs the three new pages: same header/nav, same `<h1>`
weight and size, same `p="lg"` content inset, same footer. The new pages
are built from `Card`, `StatusBadge`, `LastUpdated`, `PinToggle`,
`SegmentedControl` and `TrendsCharts`, all already in use on `/lines`,
`/lines/[id]` and `/lines/[id]/history`. Nothing looks bolted on; if
anything `/operators` is *more* filled-in than `/stations`, which is mostly
white space below its form. The one place the new work is *less* consistent
than its neighbours is the interaction model (cards that are not links,
§2.2/§4.2), not the visual language.

## 6. Recommendations (actionable, in priority order)

1. **Link the history pages.** Add a "History" link to `OperatorStatusCard`'s
   footer (`/operators/{code}/history`) and link `/network/history` from
   `/status` and `/lines`; fix the network page's back-link to point at
   whichever of those becomes its parent. Add both routes to nav/link e2e
   coverage. (§2.1, §3.1 — Critical)
2. **Resolve the card's affordance.** Either make `OperatorStatusCard` a
   link (history page today, detail page later) or remove the link-like
   shadow and add an explicit action row. Apply the same choice on the
   homepage. (§2.2, §4.2 — Important)
3. **Say what the rollup covers.** "Worst of N lines · {line name}" (linked)
   under the reason on the card, and "N lines" under the history page
   title. (§2.3, §3.4 — Important/Minor)
4. **Label the granularity control** with a visible "Granularity" text and
   `aria-labelledby`, and give it `color="grape"` to match "Period".
   Shared component, so `/lines/[id]/history` benefits. (§3.2 — Important)
5. **Rewrite the Trends copy.** One plain sentence inline, the rest in a
   collapsed "How these rates are calculated"; `—` not `--`; human date
   axis labels; charts above the explanation. Verify the deployed build for
   the "running.Rates" missing space. (§3.3 — Important)
6. **Anchor the card footer** so pin stars align across a row. (§2.4 — Minor)
7. **Homepage copy and links:** mention operators in the tagline and
   log-in CTA; add "Browse operators" to the anonymous quick links.
   (§4.4 — Minor)
8. **Pin control parity on the homepage:** either both card types have one
   or neither does. (§4.3 — Minor)
9. **`/operators` page framing:** one-line intro, worst-first ordering (or a
   sort control) before the list grows past ~10 operators; move "Operators"
   next to "All Lines" in the nav. (§2.6, §2.7 — Minor)
