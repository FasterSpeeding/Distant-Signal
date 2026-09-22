# UX / accessibility / usability review — journey detail view (`/journeys/[id]`)

**Date:** 2026-09-22
**Scope:** the journey detail page only, as captured against a live instance in
`frontend/e2e/screenshots/output/`:

| File | URL | Viewport | Browser | Case |
|---|---|---|---|---|
| `journey-167-mobile.png` | `/journeys/167` | 390×844 | Chromium | Delayed, 2 legs (1 matched + 1 unmatched) |
| `journey-167-tablet.png` | `/journeys/167` | 768×1024 | Chromium | same |
| `journey-167-desktop.png` | `/journeys/167` | 1440×900 | Chromium | same |
| `journey-167-firefox-desktop.png` | `/journeys/167` | 1440×900 | Firefox | same, cross-browser |
| `journey-168-desktop.png` | `/journeys/168` | 1440×900 | Chromium | On time, 1 leg |

Code references are to the build that produced these shots, which lives in
`.integration-worktree/frontend/` (the main tree has no `app/journeys/` yet):
`app/journeys/[id]/page.tsx`, `components/JourneyLegCard.tsx`,
`components/JourneyLegCandidates.tsx`, `components/JourneyStatusBadge.tsx`,
`components/JourneyTimeline.tsx`, `components/JourneyProgress.tsx`,
`lib/journeyStatus.ts`. The bar applied is the one set by
`2026-09-17-full-service-ux-accessibility-usability-review.md` (cited below as
"the 09-17 review") and `2026-09-02-frontend-ui-ux-review.md` ("the 09-02
review"); the design intent is `2026-09-22-journey-tracking-design.md` §3/§4.

Severity scale: **Critical** = a traveller relying on this page to catch a train
would be misled or unable to act; **Important** = materially confusing,
inaccessible, or missing a headline capability; **Minor** = polish.

---

## 1. Summary

The page is clean, consistent across three viewports and two browsers, and the
badge-contrast work from the 09-17 review has carried over intact (every badge in
these shots clears AA). The single-leg case (`journey-168-desktop.png`) does
**not** feel over-engineered — it is the old train page plus one "Add a leg"
button and one rollup badge.

But as a *journey* page it does not yet do the one thing that distinguishes a
journey from a train. Across all five shots:

1. **Both journeys contradict themselves about where the train is** (Critical).
   "Last reported: Doncaster (departure)" sits three lines above "Last reported
   at Doncaster — that report couldn't be matched to a timetabled stop", while
   *Doncaster is the third row of the table directly beneath*. The on-time
   journey does the identical thing with Woking. The progress line has no marker
   on any shot. This is the exact "page disagrees with itself" failure the 09-17
   review (§3.6, first item) already flagged — the copy was fixed as asked; the
   trigger condition was not.
2. **The unmatched leg's candidates don't answer the question the traveller
   has** (Critical). A YRK → NCL leg offers "19:00 · KGX → EDB" ×3 — the
   *train's* origin, terminus and (presumably) origin departure, not when it
   leaves York or reaches Newcastle. The traveller is due into York at est. 18:22
   and cannot tell which of the three they can catch. All three buttons are named
   "Track this train" with no distinguishing accessible name.
3. **The two legs are not presented as a chain** (Important). No "Leg 1 / Leg 2",
   no connector, no "arrive York 18:22 → next leg departs …", and the header
   rollup ("Needs a train picked") hides the 22-minute delay entirely because
   `unmatched` outranks `delayed`.

Two headline features the spec commits to are invisible in every shot: the
**"Change train"** action (the code exists but is gated on a persisted search
window the seeded leg does not have — and nothing on the card hints that
swapping is possible) and the **station-skip badge** (rendered only as an inline
pill deep in the table, never in the summary, never on the line, and — a real
gap — never in the journey rollup: `legStatusGroup` ignores `legSkip`).

---

## 2. Findings by theme

### 2.1 · Critical · The train's position contradicts itself on every shot

**Seen in:** all five files. `journey-167-desktop.png` summary block: "Last
reported: Doncaster (departure)" / "Delay: 22m late" / "Next calling point:
York"; then a six-node line with no marker; then the caption "Last reported at
Doncaster — that report couldn't be matched to a timetabled stop, so no position
is shown on the line."; then a table whose third row is **Doncaster 17:35 /
est. 17:57**. `journey-168-desktop.png` is identical in shape with Woking
(second row of its table).

**Why it matters.** The 09-17 review §3.6 called this pattern *serious*: "the
page's single job is to say where the train is, and it disagrees with itself."
The recommended copy was adopted verbatim (`JourneyProgress.tsx:241-246`), but
it now fires in a case the copy explicitly says cannot be true — the reported
location *is* a timetabled stop and the table proves it. For the on-time,
single-leg journey this is worse than on the delayed one: a traveller with a
green "On track" badge is told, in the same card, that the system cannot place
their train. Every row is also "est." — including KGX, Peterborough and
Doncaster, which the summary says the train has already departed — so
"Actual / est." never shows an actual.

**Likely cause (needs verification against real data, not the seed):**
`lastReachedIndex` is −1 while `lastReportedLocation` resolves to a name that
is in `journeyStops` — a key mismatch (TIPLOC vs CRS vs name) between the
movement overlay and the stop list, or a seed that populated
`lastReportedLocation` without `actual*` on the matching stop. Either way the
UI should defend against it.

**Recommend.**
- Never render the "couldn't be matched" caption when `lastReportedLocation`
  string-matches a row label in the same card; fall back to placing the marker
  by name, or say "Last reported at Doncaster" with no disclaimer.
- When the summary says "(departure)" from a listed stop, that row (and prior
  rows) should show actual times, not "est.". If the seed cannot produce that,
  the seed is not representative and these screenshots under-test the page.
- Surface "Next calling point: York" *on the line* (highlight the York node)
  even when the marker is absent — it is the one piece of position data the
  page is confident about.

### 2.2 · Critical · The unmatched leg's candidate rows describe the train, not the leg

**Seen in:** `journey-167-desktop.png`, `-tablet.png`, `-mobile.png`,
`-firefox-desktop.png` — second card: "YRK → NCL, 2026-09-22 / Searching for a
train to track — pick one below." then three rows "19:00 · KGX → EDB",
"20:00 · KGX → EDB", "21:00 · KGX → EDB", each with a purple "Track this train".

**What a traveller sees.** They are on leg 1, due into York est. 18:22, and need
York → Newcastle. The choices are labelled by King's Cross departure and
Edinburgh terminus. Nothing says when each leaves York or arrives at Newcastle,
whether 19:00 is a KGX time or a YRK time, or whether the 19:00 is even
reachable (a 19:00 KGX departure reaches York around 21:00 — after the 20:00
one would have left). `CandidateRow` carries `destinationArrival`
(`JourneyLegCandidates.tsx:16`) and it is not rendered.

**Accessibility.** Three buttons with the identical accessible name "Track this
train" (`JourneyLegCandidates.tsx:119-121`): a screen-reader user tabbing the
list hears three indistinguishable controls. The 09-17 review's precedent for
list actions is a per-row `aria-label` that names the row.

**Copy.** The open-leg intro "Searching for a train to track — pick one below"
and the loading state "Searching for candidate trains…"
(`JourneyLegCandidates.tsx:88-91`) are near-identical sentences for opposite
states (still loading vs. ready to choose). The first reads as a spinner.

**Codes.** "YRK → NCL" and "KGX → EDB" are bare CRS codes as the only label —
the 09-02 review's F3 finding, unchanged. Leg 1's own title one card up uses
"London Kings Cross (KGX) → York (YRK), 22 Sept 2026", so the page is
inconsistent with itself as well as with precedent (see also 2.9).

**Recommend.**
- Row copy: "**dep. York 19:12 → arr. Newcastle 20:05** · 19:00 from London
  Kings Cross to Edinburgh" — leg-scoped times first and bold, train identity
  dimmed second. If the candidate endpoint cannot return the leg-origin
  departure, that is a backend gap worth closing before this ships; it is the
  whole value of the window search.
- `aria-label="Track the 19:12 from York to Newcastle"` per button.
- Change the intro to "Pick the train for this leg:" (imperative, not
  progressive), keep "Searching…" only for the loading state.
- Show station names (with codes in parentheses if space allows), matching the
  matched card.

### 2.3 · Important · Matched vs. unmatched legs: the action-required state is visually the quietest thing on the page

**Seen in:** `journey-167-desktop.png` and `-mobile.png`.

The matched card is dense: headcode, route, three summary lines, an orange
badge, a diagram, a caption, a six-row table. The unmatched card — the one that
*needs the user to do something* — is a plain white card with a plain-weight
title, one line of dimmed grey text, and three rows. Nothing in its border,
background, title weight, icon or colour says "action required". The only
signal is the small blue "Needs a train picked" badge in the page header, which
on mobile is ~800 px above the card it refers to, does not say *which* leg, and
is not a link. Its `Tooltip` repeats the badge's own text
(`JourneyStatusBadge.tsx:27-31`) — a hover-only tooltip on a non-focusable
element that adds nothing.

The purple `Track this train` buttons do help — primary colour is reserved for
this action and it is the only saturated control on the page — so a sighted
user scrolling will find it. But a user who lands, reads the delayed first leg,
and stops there could miss that leg 2 has no train yet.

**Recommend.**
- Give the open-leg card an accent: `withBorder` plus a left border / light
  blue surface (`--mantine-color-blue-light`) and a leading `IconAlertCircle`,
  title "**Leg 2 · York → Newcastle — pick a train**".
- Make the header badge an anchor that scrolls to the first unmatched leg, and
  say how many: "1 leg needs a train".
- Drop the redundant tooltip, or replace it with useful detail ("Leg 2 has no
  train picked yet").

### 2.4 · Important · No "Change train" affordance exists on the matched leg, and nothing suggests one could

**Seen in:** the first card of every `journey-167-*.png` file. The card ends
after the Edinburgh row with ~30 px of empty space and no control of any kind —
no button, no kebab, no link, nothing on hover. The page header has only the
badge and "Add a leg".

**Verified in code.** `JourneyLegCard.tsx:95-112` does render a
`size="xs" variant="default"` "Change train" toggle, but only when
`hasWindow && isOwner`. The account is the owner (it sees "Add a leg"), so the
seeded leg was created without any of `departAfter/departBefore/arriveAfter/
arriveBefore` — i.e. the screenshots exercised the *pin/known-train* path, and
the window-search path the feature exists for was never captured. The
`track-window-search-candidates` shot of `/journeys/169` in the same manifest
shows an *open* windowed leg, but no shot shows a *matched* windowed leg, so
the button has zero screenshot coverage.

**UX assessment.** Even where it does render, the placement is weak: a
default-variant xs button *below* the timetable, after a six-row table, is the
last thing a user sees, and it toggles to "Cancel" in place with the candidate
list appearing beneath — a long card gets longer. For a leg without a window,
the spec says delete-and-recreate; the UI offers neither (no delete either —
`page.tsx`'s own doc comment: "no delete … explicitly deferred"), so a
traveller who picked the wrong train has no exit from this page at all.

**Recommend (placement).** The card header. Put the leg's actions on the right of
the title row — "Train P9E010 …… [Change train] [⋯]" — the same top-right
position the page header uses for "Add a leg", so both levels follow one rule:
*actions live at the top-right of the thing they act on*. On mobile the row
wraps and the button lands under the title, still above the fold of the card.
For a no-window leg, show the same button and route it to a "pick a different
train" flow (re-open the picker seeded from the leg's origin/destination/date),
or at minimum show a "Remove leg" so the user is not stuck. Add a matched,
windowed leg to the screenshot seed.

### 2.5 · Important · The station-skip badge has a place to go but would be invisible in practice

**Seen in:** no shot renders one (none of the seeded legs is skipping a stop).

**Where it would go.** `JourneyTimeline.tsx:257-261`: a `size="sm"` red
"Skipped" pill inline after the station name, in the table only. That is a
reasonable spot in isolation — it sits on the row it describes.

**Why it would be missed.**
- The table is the *last* element of the card. On `journey-167-mobile.png` it
  starts ~600 px into the page; a traveller scanning the summary block ("Last
  reported / Delay / Next calling point") gets no hint.
- The rollup never changes. `lib/journeyStatus.ts::legStatusGroup` reads only
  `status` and `delayMinutes`; `leg.legSkip` is not consulted. A leg whose
  destination is being skipped still rolls up as **"On track"** — the spec
  (§3) says a skipped leg should outrank both delayed and unmatched. This is a
  correctness gap, not just a visibility one.
- The progress line does not mark it either.
- Without the badge, nothing looks *wrong*: the row still shows an "est." time
  (`estimated` is only suppressed for `stopStatus === 'Skipped'`, not for the
  leg-scoped `isSkippedOnLeg`, `JourneyTimeline.tsx:243`), so a skipped York
  would read "York 18:00 est. 18:22" — a confident, wrong ETA.

**Recommend.** Treat a skip like a cancellation for display purposes:
(1) rank `legSkip` as `severe` in `legStatusGroup` with label "Not stopping at
York"; (2) render a yellow/red `Alert` at the top of the leg card — "This train
is no longer calling at York. You'll need a different train for this leg." —
with the Change-train action inside it; (3) keep the inline table badge, and
suppress the "est." time on that row exactly as the `stopStatus` path already
does. Seed one skipping leg so this is screenshot-covered.

### 2.6 · Important · The journey is not drawn as a chain, and the rollup hides the delay

**Seen in:** `journey-167-desktop.png`, `-tablet.png`, `-mobile.png`.

Two cards stacked with a 16 px gap and no relationship between them. No "Leg 1
of 2", no connector, no interchange line. Leg 1 ends with York est. 18:22; leg
2 is York → Newcastle; the page never draws the line between them. The spec
defers the *live* connection buffer to Phase 3, and its scheduled-times divider
only applies between two *matched* legs — but "Leg N" labelling and a visual
connector cost nothing and are what makes two cards read as one journey.

The header badge reads "Needs a train picked" for a journey whose only running
train is **22 minutes late**. Per `LEG_STATUS_RANK` `unmatched (3) > delayed
(2)`, so the delay is invisible above the fold. A traveller's first question
("am I late?") is answered only by scrolling. The spec asked for a worst-status
badge; it did not ask for the other statuses to be suppressed.

**Recommend.** A one-line summary under the `h1`: "**Leg 1** 22m late · **Leg
2** needs a train" (or two badges). Number the cards ("Leg 1 · London Kings
Cross → York"). Between cards, a slim connector: "Change at York — arrive est.
18:22". Even without live buffer maths, showing the arrival next to the change
point turns two cards into a journey.

### 2.7 · Important · The matched card is about the train, not the leg

**Seen in:** every `journey-167-*.png`, first card.

- Title is the headcode "Train P9E010"; the route and date are dimmed beneath.
  The 09-17 review §3.6 already asked for this inversion on the train page.
  In a list of legs, the leg (route + time) is the identity; the headcode is
  the footnote.
- The diagram and table cover the train's whole diagram (KGX → **Edinburgh**),
  with KGX and Edinburgh in bold as origin/terminus — but the traveller gets
  off at **York**, which is a plain-weight row indistinguishable from
  Peterborough. Newcastle and Edinburgh rows are noise for this leg.
- The terminus row (Edinburgh; Southampton Central on 168) has **no times at
  all**. On `journey-168-desktop.png` that is the traveller's own destination:
  the single number they most want ("when do I get in?") is blank while three
  intermediate stops have times. Likely a fixture with arrival-only data that
  `scheduledDeparture ?? scheduledArrival` should already handle — verify
  against real data; if real data also lacks it, that is a Critical.

**Recommend.** Title "London Kings Cross → York · 16:00" with "Train P9E010"
dimmed. Bold the leg's own origin and destination rows and add a small "You
get off here" marker on the destination row; dim (or collapse behind "3 more
stops to Edinburgh") rows after the leg destination. Label the diagram's end
nodes with the leg's endpoints or highlight the York node.

### 2.8 · Important · Mobile: clipped label and an unhinted horizontal scroll

**Seen in:** `journey-167-mobile.png`.

- The right-hand diagram label renders as "Edinburgł" — the last glyph is
  clipped at the card edge. The end-node label is centred on a node that sits
  at the container edge, so half the label overflows.
- The table's fourth header reads "Del" — the Delay and Platform columns are
  cut off at the card edge. `TableScrollContainer minWidth={420}`
  (`JourneyTimeline.tsx:93`) exceeds the ~326 px available inside the card at
  390 px, so the table scrolls horizontally, but there is no shadow, fade or
  scrollbar visible in the capture to say so. The 09-17 review §2.5 treated
  truncated status content on mobile as *serious*; this is the same family.
- "Actual / est." wraps its cell contents to two lines ("est. / 16:22") on
  every row, doubling the table's height.

**Recommend.** Drop the Platform column below `sm` (it is empty for all but the
origin by design — see 2.10) and shorten "Actual / est." to "Actual"; that
brings the natural width under 390. Pad the diagram's end labels inward
(`text-align` toward the centre, or `padding-inline` on the container). If a
scroll container must remain, add the standard edge-fade.

### 2.9 · Minor · Inconsistent formats across the two cards

**Seen in:** `journey-167-desktop.png`.
Matched: "London Kings Cross (KGX) → York (YRK), **22 Sept 2026**".
Unmatched: "**YRK → NCL, 2026-09-22**" (`JourneyLegCard.tsx:52` interpolates
`serviceDate` raw). Same page, same field, two date formats and two naming
conventions. Use the matched card's format everywhere.

### 2.10 · Minor · Two empty columns in every table

**Seen in:** all five files. Delay and Platform are empty on every row of both
journeys. Platform is documented as origin-only (`JourneyTimeline.tsx:298-303`)
so it will *usually* be 80 % empty; Delay is `null` per stop while the train
is 22 m late and every "est." is +22 — the data is contradictory (the est. is
propagated *from* the delay that the Delay column says is unknown). Two blank
columns read as "broken", and they cost the mobile width in 2.8. Hide a column
when no row has data; and if per-stop delay is null but train-level delay is
known, either propagate it or drop the column.

### 2.11 · Minor · "On track" vs. "On time", one line apart

**Seen in:** `journey-168-desktop.png`. Header badge "On track"; leg badge "On
time". Two phrases for one state, adjacent. The journey-level word need not
differ; "On time" for both, or "All legs on time" if the distinction is wanted.

### 2.12 · Minor · No freshness timestamp

**Seen in:** all five files. Nothing says when "22m late" was last updated —
the 09-17 review §2.12 asked for `LastUpdated` under the train summary block;
the journey page inherits the gap. A journey page is more time-critical than a
single train page (a stale leg-1 ETA silently invalidates the leg-2 pick).

### 2.13 · Minor · Known, pre-existing chrome issue still present

The blank square button in the header (between the accessibility icon and the
avatar) in every shot is the theme toggle emoji rendering as tofu — 09-17
review §2.3, not new to this feature. Noted only so the collated report does
not attribute it to journey tracking.

---

## 3. Per-screenshot notes

- **`journey-167-desktop.png`** — reference case for 2.1–2.7. Layout is
  otherwise well proportioned: 1100 px content column, cards full width, header
  actions top-right. The empty band under the Edinburgh row is the unrendered
  Change-train slot.
- **`journey-167-firefox-desktop.png`** — pixel-equivalent to Chromium apart
  from font rendering; no layout, badge or table differences. Cross-browser
  parity holds.
- **`journey-167-tablet.png`** — the best of the five: table fits without
  scrolling, labels are not clipped, both cards are visible in one viewport
  with the candidate list immediately readable. Nothing tablet-specific to
  flag.
- **`journey-167-mobile.png`** — 2.8 (clipped "Edinburgh", cut "Del" column,
  no scroll hint). Header wraps gracefully: two-line `h1`, then badge + "Add a
  leg" on their own row. Candidate rows fit on one line each with the button
  right-aligned — the unmatched card degrades well; the matched card does not.
- **`journey-168-desktop.png`** — the single-leg case reads naturally: it is
  the old train page with one extra button and one badge, which is exactly
  right. Its problems are inherited (2.1 Woking contradiction, 2.7 blank
  destination time, 2.10 empty columns) rather than journey-induced.

---

## 4. What works well

- **Badge contrast holds.** The light-variant overrides in `globals.css:157-160`
  (orange `#bb3e0d`, green `#267b37`) plus Mantine's native blue-light
  (`#1864ab` on `#d0ebff`, 4.93:1) mean "22m late", "On time", "On track" and
  "Needs a train picked" all clear AA at 11 px; `tt="none"` is applied to the
  delay badge as the 09-17 review asked.
- **Primary colour discipline.** Grape is used only for "Track this train" —
  the one action that changes state — while "Add a leg" and the (unseen)
  "Change train" are neutral. The eye goes to the right control.
- **Honest data treatment.** "est." is dimmed and italic, empty cells stay
  empty rather than showing invented placeholders, and the "couldn't be
  matched" copy itself is the exact hedged wording the previous review asked
  for (its *trigger* is the problem, 2.1).
- **Responsive header.** The title / badge / action row reflows cleanly at
  every width without truncation — the failure mode the 09-17 review §2.5
  warned about is avoided here.
- **Cross-browser and cross-viewport consistency.** Nothing Chromium-specific;
  the tablet capture is clean.
- **Not over-engineered for one leg.** `journey-168-desktop.png` is
  recognisably the same page a single-train user already knows.

---

## 5. Recommendations (actionable, in priority order)

1. **Fix the position contradiction (2.1).** Never show "couldn't be matched
   to a timetabled stop" when the reported location is a row of the same
   table; place the marker by name as a fallback; highlight the "Next calling
   point" node on the line regardless. Verify whether `lastReachedIndex = −1`
   with a listed `lastReportedLocation` occurs on real data or only in the seed.
2. **Make candidate rows leg-scoped (2.2).** Show departure from the leg's
   origin and arrival at the leg's destination first; the train's own
   origin/terminus second and dimmed; render `destinationArrival`. Give each
   "Track this train" a row-specific `aria-label`. Replace "Searching for a
   train to track — pick one below" with an imperative.
3. **Mark the open leg as needing action (2.3).** Accent border/surface,
   leading icon, "Leg N · Origin → Destination — pick a train" title; make the
   header badge an anchor that scrolls to it and says how many legs need one.
4. **Put "Change train" in the card header, top-right (2.4)** — same rule as
   "Add a leg" at page level — and give no-window legs a "Remove leg" so a
   wrong pick is recoverable. Seed a matched, windowed leg so the control is
   screenshot-covered.
5. **Rank skips in the rollup and surface them in the summary (2.5).**
   `legStatusGroup` must consult `legSkip`; add a leg-top `Alert` ("This train
   is no longer calling at York"); suppress the "est." time on the skipped row.
6. **Draw the chain (2.6).** Number legs, add a "Change at York — arrive est.
   18:22" connector, and put a per-leg summary line under the `h1` so a delay
   is never hidden behind "Needs a train picked".
7. **Make the card about the leg (2.7).** Route-and-time as the title, headcode
   dimmed; bold and mark the leg's own destination row; dim rows past it;
   ensure the terminus row always has an arrival time.
8. **Mobile table and diagram (2.8, 2.10).** Hide the Platform column below
   `sm`, rename "Actual / est." to "Actual", pad the diagram's end labels; hide
   any column with no data in any row.
9. **Consistency and freshness (2.9, 2.11, 2.12).** One date/name format for
   both card titles; one word for "on time" at both levels; `LastUpdated` under
   the summary block.

Verification each fix should carry: re-shoot `/journeys/167` at 390 px and
1440 px plus one new seed (matched + windowed + skipping leg) so 2.4 and 2.5
stop being untested claims.
