# Collated UX / accessibility / usability review — operator overview & journey tracking (2026-09-22)

Single unified report collating four independent screenshot-based reviews of the
two new features. No application code was changed in producing this document.

## Source reviews

| Tag | Document | Coverage |
|---|---|---|
| **[SDL]** | `2026-09-22-ux-review-status-dashboard-lines.md` | `/status`, `/lines`, `/lines/[id]`, `/lines/[id]/history` |
| **[OH]** | `2026-09-22-ux-review-operators-homepage.md` | `/operators`, `/operators/[code]/history`, `/network/history`, homepage pinning |
| **[JC]** | `2026-09-22-ux-review-journey-creation-flow.md` | `/track` (both modes), departure picker, candidate list, `/track/mine`, `/groups` |
| **[JD]** | `2026-09-22-ux-review-journey-detail.md` | `/journeys/[id]` — multi-leg (`167`) and single-leg (`168`) |

All screenshots are in `frontend/e2e/screenshots/output/` (`manifest.ndjson`).
Mobile = 390×844, tablet = 768×1024, desktop = 1440×900; Chromium unless a
filename says otherwise. The code behind the captures is the
`integration-preview` / `worktree-agent-aa9d33a8a0709d13a` branches, not `main`.

**Severity scale.** Critical = blocks the feature's purpose, misleads a
traveller acting on it, or fails a WCAG A/AA criterion on a primary path.
Important = real confusion, a missing headline capability, or an AA failure
users will hit. Minor = polish or a small-blast-radius regression of a house
convention. [SDL] notes the two earlier app reviews use *serious / moderate /
minor*; read Critical ≈ serious, Important ≈ moderate.

---

## 1. Executive summary

Both features are visually finished and, on the surface, in good shape: they
reuse the app's page shell, heading scale, `StatusBadge`, `PinToggle` and
`SegmentedControl`; nothing overflows horizontally at 390px on the operator
pages; Firefox is pixel-equivalent to Chromium everywhere it was captured; and
the badge-contrast and honest-copy work from the two previous reviews has
largely carried forward. The single-leg journey page in particular reads as
"the train page plus one button", which is exactly right.

The problems are structural rather than cosmetic, and they cluster in two
places. First, **two of the most expensive new things that were built cannot be
reached at all**: the operator and network history pages have no inbound link
anywhere in the UI, and a journey created by time-window search vanishes the
moment the user leaves its page — no list, no nav entry, no URL they'd
remember. Second, **the journey detail page is not yet trustworthy**: it
contradicts itself about where the train is (on both the delayed *and* the
on-time journey), and the candidate trains it offers for an unmatched leg are
labelled with the train's own origin and terminus rather than with when it
leaves and arrives at the stations the traveller actually cares about — so the
one decision the feature exists to support cannot be made from the information
shown.

Underneath the individual findings sit three repeating patterns worth fixing at
the root rather than four times over: new components re-implement row and card
shapes the app already has (`StatusRow`, `LineStatusCard`, the `/trains` result
row) and each re-implementation silently drops a rule the original encodes;
groups of identical buttons ship with identical accessible names; and several
control groups ship with no accessible name at all. Finally, roughly six
findings are entangled with fabricated seed data rather than product defects —
notably the entirely-invisible "Change train" action — so the seed needs
extending before the next review round can say anything about them.

**Counts: 4 Critical, 28 Important, 24 Minor** (plus 5 pre-existing issues
noted but not attributed to these features). The four source reviews produced
roughly 95 raw observations; merging duplicates across themes reduced them to
the 56 entries below.

---

## 2. Critical findings

### C1 · A time-window journey is unreachable once you leave its page
**Source:** [JC] §2.5, §2.6, rec. 1.
**Screenshots:** `track-window-search-candidates.png` (`/journeys/169`),
`track-mine-authenticated.png`, `track-mine-journeys.png`.

`GET /Journeys/mine` is wired into `lib/api.ts` (`getMyJourneys()`,
`:660`) and has **no caller**. `/track/mine` builds its list from `trains` and
`shared` — i.e. train subscriptions — and an unmatched leg has
`train_subscription_id IS NULL`, so it can never produce a row. `navLinks.ts`
has no journey entry. The journey page itself has no breadcrumb or "← My
journeys" link; the user arrived by a `router.push`. Close the tab and the
journey is gone unless the user memorised `/journeys/169`.

Every other private object in this app — trains, tickets, groups, custom lines
— has a list page. This one does not, and its status ("needs a train picked")
is precisely the state that requires the user to come back.

A telling artefact: [JC] found `track-mine-authenticated.png` and
`track-mine-journeys.png` are **byte-identical** (`cmp` reports no difference)
even though the manifest describes one as the journeys list. The page has no
journey concept at all.

**Recommendation.** Call `getMyJourneys()` from `/track/mine` and render
unmatched journeys as rows carrying the "Needs a train picked" badge, linking to
`/journeys/[id]`; or add a `/journeys` list page and link it from `navLinks.ts`
and from `/track/mine`. Add a "← My journeys" link above the journey `<h1>`.
See also I22 (the same object currently has two detail pages).

### C2 · The operator and network history pages have no inbound link anywhere
**Source:** [OH] §2.1, §3.1, rec. 1.
**Screenshots:** `operators-list-desktop.png`,
`operators-list-authenticated-desktop.png`, `operator-history-gr-desktop.png`,
`network-history-desktop.png`.

`/operators/[code]/history` and `/network/history` exist and render well, but a
repo-wide grep finds **no `href` pointing at either** outside their own
`basePath` constants. `OperatorStatusCard.tsx` documents being deliberately not
a link, and commit `e19af671` removed a dead operator-detail link — but the
history route shipped in the same phase and nothing replaced the link. The only
interactive element on an operator card is the pin star.

The back-links make this sharper, not softer: the operator history page says
"Back to operators", implying a forward path that doesn't exist; the network
history page says "Back to all lines" and points at `/lines`, which has never
heard of it.

**Recommendation.** Add a "History" link to `OperatorStatusCard`'s footer row
(`/operators/{code}/history`) and an "N lines" link to `/lines` filtered by
operator; link `/network/history` from `/status` (and fix its back-link to
match whichever page becomes its parent). **Add both routes to e2e nav/link
coverage so an orphaned route fails CI.** Making the card a link also resolves
half of I1.

### C3 · The journey page contradicts itself about where the train is
**Source:** [JD] §2.1, rec. 1.
**Screenshots:** all five — `journey-167-{mobile,tablet,desktop,firefox-desktop}.png`,
`journey-168-desktop.png`.

On `journey-167-desktop.png` the summary block says "Last reported: Doncaster
(departure)"; three lines below, the caption says "Last reported at Doncaster —
that report couldn't be matched to a timetabled stop, so no position is shown on
the line."; and **Doncaster is the third row of the table directly beneath**.
The progress line carries no marker on any shot. `journey-168-desktop.png` does
the identical thing with Woking — and there it is *worse*, because a traveller
looking at a green "On track" badge is told in the same card that the system
cannot place their train.

Compounding it: every row reads "est.", including stops the summary says the
train has already departed, so the "Actual / est." column never shows an actual.

This is the exact failure the 09-17 review §3.6 rated *serious*. The
recommended hedged copy was adopted verbatim (`JourneyProgress.tsx:241-246`) —
but the *trigger condition* was never fixed, and it now fires in a case the copy
explicitly says cannot be true.

**Likely cause** (needs verification against real data, see §5): `lastReachedIndex`
is −1 while `lastReportedLocation` resolves to a name present in `journeyStops`
— a TIPLOC/CRS/name key mismatch between the movement overlay and the stop list,
or a seed that set `lastReportedLocation` without `actual*` on the matching stop.

**Recommendation.** Never render the "couldn't be matched" caption when
`lastReportedLocation` string-matches a row label in the same card — fall back
to placing the marker by name, or drop the disclaimer. When the summary says
"(departure)" from a listed stop, that row and prior rows should show actual
times. Highlight the "Next calling point" node on the line regardless, since
that is the one position fact the page is confident about.

### C4 · Candidate trains are described by the train's route, not the traveller's leg
**Source:** [JD] §2.2, rec. 2 (Critical); independently [JC] §2.5, rec. 4
(logged there as three Important findings — enrichment, codes, accessible names).
**Screenshots:** `journey-167-desktop.png`, `-tablet.png`, `-mobile.png`,
`-firefox-desktop.png` (second card); `track-window-search-candidates.png`.

A **YRK → NCL** leg offers three candidates labelled "19:00 · KGX → EDB",
"20:00 · KGX → EDB", "21:00 · KGX → EDB". The traveller is due into York at
est. 18:22 and needs York → Newcastle. Nothing says when each train leaves
York, when it reaches Newcastle, or whether "19:00" is a King's Cross time or a
York time — and a 19:00 KGX departure reaches York around 21:00, *after* the
20:00 one would have left. The decision the window-search feature exists to
support cannot be made from what is on screen.

`CandidateRow` already carries `destinationArrival`
(`JourneyLegCandidates.tsx:16`) and does not render it. There is no operator, no
duration, no calling-pattern hint and no "view live status" link — where the
spec §2.3 said to reuse `TrainSearchForm`'s result rendering, `JourneyLegCandidates`
is a fresh minimal list instead (see I25). `track-window-search-candidates.png`
shows the same shape on the creation path: four rows "18:00 · KGX → EDB" …
"21:00 · KGX → EDB" differing only in the hour.

Three additional defects ride on the same rows, each independently reported:
- **All buttons share one accessible name** ("Track this train" ×3 on `/journeys/167`,
  ×4 on `/journeys/169`); the row text is a sibling `<Text>`, not part of the
  name. See I26.
- **Bare CRS codes and an ISO date** ("YRK → NCL, 2026-09-22",
  `JourneyLegCard.tsx:52`) on a page whose *first card* already renders
  "London Kings Cross (KGX) → York (YRK), 22 Sept 2026". See I27.
- **"Searching for a train to track — pick one below"** describes a spinner, not
  a list of four ready choices. See M15.

**Recommendation.** Row copy: "**dep. York 19:12 → arr. Newcastle 20:05** ·
19:00 from London Kings Cross to Edinburgh" — leg-scoped times first and bold,
train identity dimmed second. Give each button
`aria-label="Track the 19:12 from York to Newcastle"`. Use station names with
codes in parentheses, matching the matched card. **If the candidate endpoint
cannot return the leg-origin departure and leg-destination arrival, that is a
backend gap to close before this ships** — it is the entire value of the window
search.

---

## 3. Important findings

### I1 · Affordance mismatch: cards that are links don't look it, cards that look like links aren't
**Source:** [SDL] §2.1 + [OH] §2.2, §4.2 — two halves of one problem.
**Screenshots:** `status-dashboard-{mobile,tablet,desktop}.png`,
`operators-list-desktop.png`, `homepage-operator-overview-authenticated-desktop.png`.

On `/status`, all five severity counter tiles and all five "Lines to watch" rows
*are* `next/link`s, styled `{ textDecoration: 'none', color: 'inherit' }` with
no chevron, hover cue or link colour — ten identical white cards. The app's
established affordance for "this goes somewhere" is grape link text, visible on
the very same screenshots (`/lines` names, "View history", "Back to line").
The dashboard is the one page whose entire purpose is drill-down and its
drill-down controls are its least link-like elements.

On `/operators` the inverse: `OperatorStatusCard` has the exact shape, border
and shadow of `LineStatusCard`, which *is* a link everywhere else — but is
inert. Its reason text is clamped to three lines with an ellipsis ("…revised
or…") and there is no way to read the rest; on `LineStatusCard` the click-through
*is* the read-more. On `homepage-operator-overview-authenticated-desktop.png`
the two card types sit ~300px apart and are identical in shape: "Your Lines"
cards navigate, "Your Operators" cards don't. Users learn "cards are links"
from the first section and are wrong by the third.

Two knock-ons on the `/status` half: the tile link's accessible name is a count
("0 Informational"), not a purpose; and a bare `<a>` wrapping a block `Card` is
exactly the shape where `:focus-visible` fragments or vanishes — Mantine's focus
ring applies to Mantine interactive components, not bare anchors, so ten primary
links may have no visible keyboard focus. **Needs an axe/keyboard pass.**

**Recommendation.** One rule for both: a card that navigates gets Mantine's
`Card component={Link}` treatment with a hover border change, the Mantine focus
ring, and a descriptive `aria-label` ("2 lines with Severe Disruption — view in
All Lines"); a card that does not navigate loses the shadow and gains an explicit
action row. Don't link a zero-count tile — render it muted with "none" for "0".
For `/operators`, making the card link to its history page resolves C2 at the
same time.

### I2 · "Trains running today" rows read `HH:MM · Unknown station`
**Source:** [SDL] §4.1, rec. 1.
**Screenshots:** `line-detail-lner-ecml-tablet.png`, `-desktop.png`.

Eleven of the twelve visible rows render as "06:00 · Unknown station", "07:00 ·
Unknown station", … "16:00 · Unknown station · 22m late". The row calls
`routeLabel(live.originCrs, live.originName, …)`; with no schedule match
`originCrs` is null and `routeLabel` returns `UNKNOWN_STATION_LABEL`
(`lib/stationLabel.ts:21`) as the *entire* label. The panel's main content is a
fallback string — failing at its one job ("which train is this?") while looking
like data, and leaking a fallback constant into user copy (09-17 §2.9, 09-02 F5).
It is also worse than the sibling `/train` page, whose "Unknown location" rows
were just removed (`bcf7ea9d`).

The panel's own source comment notes `callingPoints` "is present on every entry
regardless of live-status coverage" — the displayed time already comes from
`callingPoints[0]`. The names are right there.

**Recommendation.** Build the route label from the schedule side (first calling
point → last calling point) and only *upgrade* to the live origin/destination
when both resolve. Never print "Unknown station" as a whole label; if the
schedule is also nameless, print the headcode/UID ("Train 1A23"). [SDL] notes
this may warrant Critical if the "no schedule match yet" state is common during
the day — see §5.

### I3 · Dashboard tiles carry no severity signal and are ordered best-first
**Source:** [SDL] §2.2, rec. 3.
**Screenshots:** `status-dashboard-mobile.png`, `-tablet.png`, `-desktop.png`.

Five text-only tiles, identical in treatment: "2 Severe Disruption" is visually
indistinguishable from "3 Good Service". Everywhere else in the app a severity
is colour **and** text together. This passes the colour-alone test trivially but
at the cost of the page's own promise ("…at a glance"). `SEVERITY_GROUPS_BY_RANK`
is ascending, so the strip reads Good → Informational → Planned → Minor →
Severe: a traveller asking "is anything wrong?" reaches the answer last. On
mobile "2 Severe Disruption" is orphaned alone on a third row, ~330px below the
heading.

**Recommendation.** Keep the text label (the accessible half) and add the
severity's existing badge colour as a non-sole cue — a coloured left border, top
rule or dot. Order worst-first, matching `worstFirst` in the list below. If an
orphan is unavoidable in a 2-/3-column grid, orphan "Good Service", not "Severe
Disruption".

### I4 · "By mode" cards mis-state the situation in two ways
**Source:** [SDL] §2.3, rec. 6.
**Screenshots:** `status-dashboard-tablet.png`, `-desktop.png`, `-desktop-firefox.png`.

- **TfL: green `ALL GOOD SERVICE` over `0 lines tracked`.** An empty set is not
  "all good service". This is exactly the false reassurance the app's copy
  elsewhere avoids, and it contradicts the page's own subtitle ("8 lines tracked
  across National Rail and TfL right now") when there are no TfL lines.
- **National Rail: `5 AFFECTED` in yellow** when two of the five are Severe
  (`SEVERE DELAYS` and `PART SUSPENDED` are visible in the list directly above).
  Yellow is the app's *Minor Disruption* colour, so the card's colour understates
  the list it summarises.
- Both badges are Mantine `variant="light"`, an unmeasured combination — see I19.

**Recommendation.** Neutral "No lines tracked" state (grey, no badge) for an
empty mode, and an honest subtitle. Colour the "N affected" badge by the *worst*
severity in the slice (the card already computes `worstStatus` over `reports`),
or drop the colour and show "5 of 8 affected" as plain text.

### I5 · `TableScrollContainer minWidth={420}` on a 390px viewport, twice, with no scroll cue
**Source:** [SDL] §3.1, rec. 4 + [JD] §2.8, rec. 8 — same idiom, two places.
**Screenshots:** `lines-unfiltered-mobile.png`, `journey-167-mobile.png`.

- **`/lines` mobile:** Name + Status + Pin inside a 420px scroll container on a
  390px viewport, so the **Pin column sits ~30px off-screen**, reachable only by
  swiping the table sideways. No edge fade, no visible scrollbar, no cut-off
  header cell. The 09-02 review's F4 ("Mobile users cannot pin from the All Lines
  table at all") was recorded as fixed in 09-17 §5; in this capture the star is
  not visible, so a mobile visitor who doesn't discover the scroll is back to F4.
- **Journey timeline:** the same container (`JourneyTimeline.tsx:93`) exceeds the
  ~326px available inside a card at 390px. The fourth header reads **"Del"** —
  the Delay and Platform columns are cut off — again with no shadow, fade or
  scrollbar. 09-17 §2.5 treated truncated status content on mobile as *serious*.
- Also on `journey-167-mobile.png`: the diagram's right-hand end label renders as
  **"Edinburgł"**, clipped at the card edge, because the label is centred on a
  node sitting at the container edge. And "Actual / est." wraps to two lines on
  every row, doubling table height.

`TableScrollContainer` is the right tool for a genuinely wide table; it is the
wrong tool for a table that is 30px too wide.

**Recommendation.** Make both fit. `/lines`: move the pin star into the Name
cell, or shrink the Pin column to a 44px icon-only hit area. Journey timeline:
drop the Platform column below `sm` (it is empty for all but the origin by
design — see M21) and shorten "Actual / est." to "Actual". Pad the diagram's end
labels inward. Where a scroll container must remain, add the standard edge-fade.

### I6 · `/lines` filter chips don't update the URL, so URL and page disagree
**Source:** [SDL] §3.2, rec. 5.
**Screenshot:** `lines-filtered-severe-desktop.png`.

`statusGroup` only *seeds* client state (`initialStatusGroup` → `useState`;
`page.tsx:19-28` says so deliberately). With the new dashboard generating
`?statusGroup=…` links on every visit, the consequences are now on the primary
path: tap the Severe tile then "All statuses" and the table shows everything
while the URL still says `severe` — refresh or share and the filter returns.
Browser Back from a chip change does nothing while Back from the tile returns to
`/status`: two meanings of Back on one control set. The `<title>` is always "All
Lines" even for a two-row filtered view; the code comment argues filtered links
"are not the link people paste", an assumption the dashboard invalidates.

**Recommendation.** Mirror the chip to the URL with `router.replace({ scroll: false })`,
read it with `useSearchParams`, and let the title reflect it.

### I7 · The trains panel leads with the past and is uncapped
**Source:** [SDL] §4.2, rec. 9.
**Screenshot:** `line-detail-lner-ecml-desktop.png`.

Captured at 18:09 local; the list starts at 06:00 and the first twelve rows are
all departed trains. A traveller on a Severe-delays line at 18:09 wants the next
departure and must scroll past the whole day. The list has no cap — a main line
can run 100+ services — so on mobile the "Recent trends" charts below become
effectively unreachable.

**Recommendation.** In increasing effort: separate "Departed" (collapsed) from
"Upcoming"; sort upcoming first with "N earlier trains" collapsed; or cap at the
next 10 with "Show all N".

### I8 · A single failing results panel blanks the entire history route
**Source:** [SDL] §5.1, rec. 7.
**Screenshot:** `line-history-lner-ecml-desktop.png` (and its two retries).

The capture is `frontend/app/error.tsx`, the app's only error boundary, reached
because of a confirmed data bug in this line's history. As a *fallback* the
boundary is genuinely good (real `<h1>`, no raw error, copyable "Reference:
1858812559", hedged copy, Try again + auto-reset, nav and footer survive — the
09-02 F5 and `page-has-heading-one` fixes holding). The problem is blast radius:
the page title, the "Back to line" link, the Period control and the
Timeline/Trends tabs all disappear, so the visitor loses their place and the one
obvious recovery is not offered.

The same feature's `LineTrainsResults` and `HalfHourlyTrendsResults` deliberately
"resolve to real markup rather than throw" so a failure is scoped to the panel.
`HistoryResults` does not.

**Recommendation.** Apply the same catch-and-render-a-`Paper` pattern to
`HistoryResults`, or add a route-level `error.tsx` under `lines/[id]/history/`
that keeps the header and offers "Back to line". See also M5 for the global
boundary's copy.

### I9 · Control groups ship with no accessible name
**Source:** [OH] §3.2, rec. 4 + [JC] §2.1, rec. 2; related [SDL] §3.5.
**Screenshots:** `operator-history-gr-desktop.png`, `network-history-desktop.png`,
`track-pick-mode-{desktop,tablet,mobile}.png`, `track-window-mode-*.png`.

- **Granularity control** (`GranularityControl.tsx:60-63`): no `aria-label`, no
  `aria-labelledby`, no visible `Text` label — while "Period" directly above it
  *has* a bold visible label and `aria-labelledby` (`HistoryRangePicker.tsx:120-123`).
  A screen-reader user hears three unnamed radio options. A sighted user's only
  hint is dimmed helper text *below* the control that explains an **absent**
  option before naming the present ones. The two controls also use different
  selected-state treatments — grape-filled for "7 days", white-on-grey for
  "Daily" — so they read as two unrelated widgets rather than two settings of one
  chart. 09-17 §2.13 asked for one idiom here; the Period half was done, the
  granularity half was not.
- **The `/track` mode toggle** (`TrackTrainForm.tsx:1002-1009`) passes only
  `value`/`onChange`/`data`: no heading, legend or `aria-label`. A screen reader
  announces "radiogroup, Pick a departure, radio button, 1 of 2, checked" with no
  group name. This toggle is the **only** discovery path for window mode (the
  mode is not URL-addressable, see I21), so the label is load-bearing.
- **Sortable headers** on `/lines`: `Name ▲` / `Status ↕` glyphs are the only
  sort-state indicator; confirm `aria-sort` is set on the `<th>`.

**Recommendation.** "Granularity" (or "Show per") visible label +
`aria-labelledby`, and `color="grape"` so both rows share one idiom — this lands
on `/lines/[id]/history` too, since the component is shared. For the toggle, a
visible "How do you want to find the train?" wired as the group's label. Add
`aria-sort` to sortable headers.

### I10 · The Trends methodology copy reads as a developer note
**Source:** [OH] §3.3, rec. 5.
**Screenshots:** `operator-history-gr-desktop.png`, `network-history-desktop.png`.

A five-line dimmed paragraph (`TrendsResults.tsx:27-30` `HONESTY_COPY` plus a
per-page sentence) with, all visible in the captures:
- **Double hyphens as dashes** ("day -- not a share of poll cycles") where the
  rest of the app uses a real em dash. The source-comment idiom leaking into UI.
- **`network-history-desktop.png` shows "simultaneously running.Rates shown"**
  with no space. The current source (`NetworkTrendsResults.tsx:77`) has the space
  — see §5, this needs verifying against the deployed build.
- **Engineering vocabulary**: "poll cycles", "coverage", "rollup", "catalogue
  line" (09-17 §2.9 class). A passenger does not know what a poll cycle is.
- **ISO axis dates** ("2026-09-15") where the rest of the app prints "15 Sep";
  volume axis ticks of 0/65/130/195/260 suggest nice-number rounding is off.
- The rate chart's y-axis is cut off by the viewport, so it reads as "one line,
  no others" until you scroll — a reason to put charts above the paragraph.
  (Checked in source: cancellation and skip rates *are* plotted with distinct
  dash patterns, `TrendsCharts.tsx:157-158`, so the legend is not colour-only.)

This paragraph is inherited from `/lines/[id]/history`, but the operator and
network pages make it longer and it is the first time it appears on a page
without a Timeline tab to balance it.

**Recommendation.** Keep one plain sentence inline ("Each train is counted once
per day, by the status it had when first seen. Days with too little data are
left blank."); move the rest into a collapsed "How these rates are calculated"
(`<details>` / Mantine `Spoiler`); `—` not `--`; the app's existing date
formatter on the axis; charts above the explanation. Shared copy, so
`/lines/[id]/history` benefits too.

### I11 · Rollups don't say what they are rolling up
**Source:** [OH] §2.3, §3.4, rec. 3.
**Screenshots:** `operators-list-desktop.png`, `operator-history-gr-desktop.png`,
`network-history-desktop.png`.

"London North Eastern Railway — SEVERE DELAYS — Severe delays between London
Kings Cross and Peterborough after a signalling failure…" is *one line's* reason
text presented as the operator's status, with no indication that it is the worst
of *n* lines or which line it is. ScotRail is the clearest case: "PLANNED
CLOSURE — Planned engineering work will close the line between Edinburgh and
Kirkcaldy next weekend" is one Fife Circle closure presented as ScotRail's
status, with "No delay/cancellation data available for this operator" beneath it.
A passenger cannot tell whether the whole network is in trouble or one branch is.
The history pages have the same gap: "History: London North Eastern Railway"
gives no sense of scope until the last sentence of the methodology paragraph.

Nine operators today; the homepage source says the full list is 25–40.

**Recommendation.** A one-line dimmed `size="xs"` qualifier under the reason:
"Worst of 4 lines · LNER East Coast Main Line" (line name linked); "4 lines, all
running normally" when everything is Good Service. On the history pages, "4
lines" (linked) directly under the title, and "Every National Rail line this app
tracks (TfL not included)" on the network page. Same "say the scope" principle
09-17 F11 applied to station rows.

### I12 · The journey rollup badge hides the delay, and the legs aren't drawn as a chain
**Source:** [JD] §2.6, rec. 6.
**Screenshots:** `journey-167-desktop.png`, `-tablet.png`, `-mobile.png`.

The header badge reads "Needs a train picked" for a journey whose only running
train is **22 minutes late** — `LEG_STATUS_RANK` puts `unmatched (3)` above
`delayed (2)`, so the delay is invisible above the fold and a traveller's first
question ("am I late?") is answered only by scrolling. The spec asked for a
worst-status badge; it did not ask for the other statuses to be suppressed.

Separately, the two legs are two cards stacked with a 16px gap and no
relationship: no "Leg 1 of 2", no connector, no interchange line. Leg 1 ends
with York est. 18:22; leg 2 is York → Newcastle; the page never draws the line
between them. The spec defers the *live* connection buffer to Phase 3, but "Leg
N" labelling and a visual connector cost nothing and are what make two cards
read as one journey.

**Recommendation.** A one-line summary under the `<h1>`: "**Leg 1** 22m late ·
**Leg 2** needs a train" (or two badges). Number the cards. Between cards, a slim
connector: "Change at York — arrive est. 18:22".

### I13 · A skipped leg still rolls up as "On track" — `legStatusGroup` ignores `legSkip`
**Source:** [JD] §2.5, rec. 5.
**Screenshots:** none — no seeded leg skips a stop (see §5).

Verified in code rather than pixels, and a correctness gap rather than a
visibility one: `lib/journeyStatus.ts::legStatusGroup` reads only `status` and
`delayMinutes`; `leg.legSkip` is never consulted. A leg whose *destination is
being skipped* still rolls up as **"On track"**, where the spec (§3) says a
skipped leg should outrank both delayed and unmatched.

The display side compounds it. The skip badge is a `size="sm"` red "Skipped"
pill inline in the table only (`JourneyTimeline.tsx:257-261`) — the table is the
last element of the card, ~600px into the page on mobile. The progress line
doesn't mark it. And the "est." time is only suppressed for
`stopStatus === 'Skipped'`, **not** for the leg-scoped `isSkippedOnLeg`
(`JourneyTimeline.tsx:243`), so a skipped York would read "York 18:00 est. 18:22"
— a confident, wrong ETA on a train that isn't stopping there.

**Recommendation.** (1) Rank `legSkip` as `severe` in `legStatusGroup` with the
label "Not stopping at York"; (2) render a yellow/red `Alert` at the top of the
leg card — "This train is no longer calling at York. You'll need a different
train for this leg." — with the Change-train action inside it; (3) keep the
inline table badge and suppress the "est." time on that row exactly as the
`stopStatus` path already does. Seed one skipping leg so this becomes
screenshot-covered.

### I14 · "Change train" is invisible, and a wrong pick has no exit
**Source:** [JD] §2.4, rec. 4. **Partly a seed artefact — see §5.**
**Screenshots:** every `journey-167-*.png`, first card (the ~30px empty band
under the Edinburgh row *is* the unrendered slot).

`JourneyLegCard.tsx:95-112` does render a `size="xs" variant="default"` "Change
train" toggle — but only when `hasWindow && isOwner`. The account is the owner
(it sees "Add a leg"), so the seeded leg was created without any of
`departAfter/departBefore/arriveAfter/arriveBefore`: the screenshots exercised
the pin/known-train path, and the window-search path the feature exists for was
never captured in a *matched* state. The control has **zero screenshot coverage**.

Two genuine product gaps remain regardless of the seed:
- Even where it renders, the placement is weak — a default-variant xs button
  *below* a six-row timetable is the last thing a user sees, and it toggles to
  "Cancel" in place with the candidate list appearing beneath, making a long card
  longer.
- For a leg *without* a window the spec says delete-and-recreate, but the UI
  offers neither change nor delete (`page.tsx` doc comment: "no delete …
  explicitly deferred"). **A traveller who picked the wrong train has no exit
  from this page at all.**

**Recommendation.** Move the leg's actions to the right of its title row —
"Train P9E010 …… [Change train] [⋯]" — the same top-right position the page
header uses for "Add a leg", so both levels follow one rule: *actions live at the
top-right of the thing they act on*. For a no-window leg, show the same button
routed to a re-pick flow seeded from the leg's origin/destination/date, or at
minimum a "Remove leg". Add a matched, windowed leg to the screenshot seed.

### I15 · The leg that needs action is the quietest thing on the page
**Source:** [JD] §2.3, rec. 3.
**Screenshots:** `journey-167-desktop.png`, `-mobile.png`.

The matched card is dense — headcode, route, three summary lines, an orange
badge, a diagram, a caption, a six-row table. The unmatched card, the one that
*needs the user to do something*, is a plain white card with a plain-weight
title, one line of dimmed grey text and three rows. Nothing in its border,
background, title weight, icon or colour says "action required". The only signal
is the small blue "Needs a train picked" badge in the page header, which on
mobile is ~800px above the card it refers to, doesn't say *which* leg, and isn't
a link. The purple "Track this train" buttons do help — grape is correctly
reserved for the one state-changing action — but a user who lands, reads the
delayed first leg and stops there could miss that leg 2 has no train.

**Recommendation.** `withBorder` plus a left border / light blue surface
(`--mantine-color-blue-light`) and a leading `IconAlertCircle`; title "**Leg 2 ·
York → Newcastle — pick a train**". Make the header badge an anchor that scrolls
to the first unmatched leg and says how many: "1 leg needs a train".

### I16 · The matched card is about the train, not the leg — and the terminus row has no times
**Source:** [JD] §2.7, rec. 7.
**Screenshots:** every `journey-167-*.png` first card; `journey-168-desktop.png`.

- The title is the headcode "Train P9E010", with route and date dimmed beneath.
  In a list of legs the leg (route + time) is the identity and the headcode is
  the footnote — 09-17 §3.6 already asked for this inversion on the train page.
- The diagram and table cover the *train's* whole diagram (KGX → **Edinburgh**),
  with KGX and Edinburgh bold as origin/terminus — but the traveller gets off at
  **York**, a plain-weight row indistinguishable from Peterborough. Newcastle and
  Edinburgh are noise for this leg.
- **The terminus row has no times at all.** On `journey-168-desktop.png` that
  terminus is the traveller's own destination (Southampton Central): the single
  number they most want — "when do I get in?" — is blank while three intermediate
  stops have times. Likely a fixture with arrival-only data that
  `scheduledDeparture ?? scheduledArrival` should already handle; **if real data
  also lacks it, this is Critical** (see §5).

**Recommendation.** Title "London Kings Cross → York · 16:00" with "Train P9E010"
dimmed. Bold the leg's own origin and destination rows, add a "You get off here"
marker on the destination row, and dim or collapse rows past it ("3 more stops to
Edinburgh"). Label the diagram's end nodes with the leg's endpoints, or highlight
the York node.

### I17 · Every time-window field says "(optional)" but at least one is required
**Source:** [JC] §2.2, rec. 3.
**Screenshots:** `track-window-mode-{desktop,tablet,mobile}.png`.

`windowHasABound` (`TrackTrainForm.tsx:380-383`) requires at least one of the
four fields; the only place this is stated is the post-submit error "Enter at
least one earliest/latest departure or arrival time to search a window."
(`:702`). The labels in every capture say the opposite. This is the 09-02 F9
pattern — the UI documents a rule everywhere except in the control that enforces
it — in reverse: **the labels actively promise the wrong thing.**

**Recommendation.** Either drop "(optional)" and add a group-level line ("Give at
least one of these times") under the grid, or keep "(optional)" and say "at least
one of these four".

### I18 · The window the user just typed is never shown back to them, and there's no edit path
**Source:** [JC] §2.5, rec. 4.
**Screenshots:** `track-window-search-candidates.png`, `journey-167-*.png`.

The open-leg card header is origin, destination, date only
(`JourneyLegCard.tsx:51-53`). The `departAfter`/`departBefore`/`arriveAfter`/
`arriveBefore` values are present on `leg` (they drive `hasWindow` at `:41-45`)
and are never rendered — even though spec §4 explicitly says the open-leg card
shows "the search parameters (origin, destination, windows)". The user cannot
tell why the 17:30 they expected is absent, cannot verify a typo ("did I say
18:00 or 08:00?"), and has no way to edit: the list is the whole page.

**Recommendation.** "Departing KGX after 18:00 on Tue 22 Sept" on the card, plus
an "Edit search" affordance. Note this currently has nowhere to link to — see
I21; `?mode=window` is a prerequisite.

### I19 · Mantine `variant="light"` badges fail AA text contrast (estimated — measure)
**Source:** [JC] §2.4, §2.6, rec. 5 + [SDL] §2.3. **Partly contradicted by [JD] — see §5.**
**Screenshots:** `track-pick-departures-platform-badge.png`,
`track-mine-authenticated.png`, `status-dashboard-desktop.png`.

- **"Platform 2 (changed from 1)"** — `PlatformBadge.tsx:38` uses
  `variant="light" color="orange"`. Mantine's light variant paints text in
  `orange-6` (`#fd7e14`) over a 10% tint; `#fd7e14` on white is **≈2.6:1** at
  11px/700 weight, where WCAG 1.4.3 needs 4.5:1.
- **"22M LATE"** on `/track/mine` is yellow-light — `#fab005` on white is
  **≈1.9:1**, worse. Pre-existing, but the same root.
- **`/status` "By mode" badges** are the same `light` variant and were never
  measured, unlike the app's `filled` + `autoContrast` `StatusBadge`s.

**Recommendation.** Fix at the token level, once: `variant="light"` with
`autoContrast` does *not* change the text colour, so set the text explicitly
(`orange.8`/`orange.9`, `yellow.9`) or switch to `variant="outline"` with a dark
hue. Run axe against the deployed build first — [JD] found `globals.css:157-160`
already overrides orange and green light variants (`#bb3e0d`, `#267b37`), so part
of this may already be fixed; yellow appears not to be covered.

### I20 · Two adjacent orange badges encode two unrelated facts
**Source:** [JC] §2.4.
**Screenshot:** `track-pick-departures-platform-badge.png`.

Platform-changed (orange light) sits 8px from delayed (orange filled). At a
glance the row reads "two orange warnings" and the user must read both to learn
one is "where to stand" and the other "how late". Colour is not the only cue (the
text carries both facts, which `PlatformBadge` gets right), so this is not a
1.4.1 failure — but using the *same hue* for *different categories* inverts what
the app does everywhere else, where green/orange/red map to one severity axis.

**Recommendation.** Give the platform badge a neutral or blue treatment with a
small platform glyph, reserving orange for lateness. If orange must stay for
"changed", move the platform badge to its own line under the title at small
widths.

### I21 · Window mode is under-introduced, jargon-labelled, and not linkable
**Source:** [JC] §2.1, rec. 2.
**Screenshots:** `track-pick-mode-*.png`, `track-window-mode-*.png`.

Three separate gaps around one control (its *missing accessible name* is I9):
- **The labels name the mechanism, not the user's situation.** "Pick a
  departure" / "Search a time window" are both verbs the *form* performs. The
  distinction in a passenger's head is "I know which train I'm getting" versus
  "I'll be on something between about 6 and 7". The sibling `AddJourneyLegButton`
  modal (`:164-167`) already uses "I know the train" — so the codebase has two
  labels for the same choice and the better one is on the less visible control.
- **The page never introduces the second mode.** The `<h1>` is "Track a Train"
  and the intro reads "Pin a specific train to see its live position…"
  (`track/page.tsx:71`), true of pick mode only. A user who arrives to find a
  train they half-know is told this page is for *specific* trains. The
  `<meta description>` has the same gap.
- **Not linkable.** The mode lives only in `useState` (`:348`);
  `track/page.tsx:47-63` reads only `origin` and `ticketId`. Nothing can send a
  user straight to window mode — not the candidate list's own "Search manually"
  fallback (`JourneyLegCandidates.tsx:104` links to bare `/track`, landing on
  *pick* mode), not the add-ticket page, not a bookmark. `?origin=` shows the
  pattern is already established.

**Recommendation.** Relabel "I know which train" / "I'm not sure yet — search by
time"; use the same two labels in `AddJourneyLegButton`; support `?mode=window`;
rewrite the intro and meta description to mention both paths.

### I22 · Two nouns for one object, and two detail pages depending on which door you enter
**Source:** [JC] §2.6, rec. 8.
**Screenshots:** `track-mine-authenticated.png`, `track-window-search-candidates.png`,
`journey-167-desktop.png`.

The create flow's heading is "Track a Train"; the destination page is "Tracked
journey" at `/journeys/169`; the list is "My Trains & Tickets" with "Track a new
train". Worse than vocabulary drift: a matched journey links **from the list to
`/train/...`** (`page.tsx:210-213`), not to the `/journeys/...` page the user was
shown after creating it — so the same object has two detail pages depending on
the door.

**Recommendation.** Pick "journey" or "train" for user-facing copy and rename
accordingly; route list rows to the journey page. The spec §7.2
backward-compatibility section is about the API, not the vocabulary. This
overlaps C1 — the list work lands both.

### I23 · Time inputs still render the 12-hour "--:-- --" skeleton (verify)
**Source:** [JC] §2.2, rec. 6. **Possible capture artefact — see §5.**
**Screenshots:** `track-window-mode-{desktop,tablet,mobile}.png`.

All three window captures show three input segments including an AM/PM slot, on
a site where every displayed time is 24h. `TimeFilterInput.tsx:205-221` sets
`lang="en-GB"` specifically to prevent this (Task 3.6.13), and the 09-17 review
§3.6 already listed it as a defect on `/trains`.

**Recommendation.** Confirm whether `lang="en-GB"` takes effect in headless
Chromium versus a real browser. A native `<input type="time">` placeholder cannot
be set, so if `lang` proves unreliable, fall back to the approach 09-17 named: a
text input with `placeholder="HH:MM"` and `inputMode="numeric"`.

### I24 · Nothing anywhere says when the data was last updated
**Source:** [SDL] §2.5 (Important), §4.4 (Minor), rec. 8 + [JD] §2.12 (Minor).
**Screenshots:** `status-dashboard-*.png`, `line-detail-lner-ecml-*.png`,
all five `journey-*.png`.

- `/status` subtitle says "8 lines tracked … **right now**" with no timestamp, no
  "updated 2 min ago", no timezone, on any viewport — and the page is served
  through `withStaleFallback`, so "right now" can legitimately be minutes old.
- The "Trains running today" panel has no date, count, last-updated or timezone.
- The journey page has nothing saying when "22m late" was measured — and a
  journey page is *more* time-critical than a single train page, because a stale
  leg-1 ETA silently invalidates the leg-2 pick.

09-17 §2.12 raised exactly this as moderate across the app; all three new
surfaces inherit the gap rather than fixing it. Raised to Important here because
it recurs on every new surface in both features.

**Recommendation.** Add the same `LastUpdated` line `LineStatusCard` already
renders: once under the `/status` subtitle, once under "Trains running today",
once under the journey summary block.

### I25 · New components re-implement existing primitives and drop the rules they encode
**Source:** [SDL] §2.4, §4.3, §6, rec. 14 (logged Minor) + [JC] §2.5, rec. 7 +
[JD] §2.2. Raised to Important here as a merged root cause.
**Screenshots:** `status-dashboard-mobile.png`, `line-detail-lner-ecml-*.png`,
`track-window-search-candidates.png`, `journey-167-desktop.png`.

Four instances, each dropping a different rule:
1. **"Lines to watch" rows** are a hand-rolled `Group justify="space-between"`
   around a `Text` and a `StatusBadge`, where `StatusRow` (documented shrink
   rules, consistent height) and `LineStatusCard` (same link + badge, plus sample
   summary and last-updated) already exist. Dropped: the sample summary ("21 of
   48 sampled services delayed…") and the freshness line — so a traveller
   choosing between two Severe lines has no basis to choose. Row heights also
   vary on mobile as badges wrap.
2. **"Trains running today" rows** are `Group wrap="nowrap"` with a `Text` and a
   `TextLink` and **no `flexShrink: 0` on the link** — the exact shape 09-17 §2.5
   flagged. Not observed to break only because the mobile capture happened to
   contain short rows; a long route name plus "· 22m late" at 390px will squeeze
   "View live / status" onto two lines.
3. **`JourneyLegCandidates`** is a fresh minimal list where spec §2.3 said to
   reuse `TrainSearchForm`'s result rendering. Dropped: arrival time, duration,
   operator and the per-row "View live status" link that `/trains` rows have —
   which is precisely the content gap that makes C4 Critical.
4. **`AddJourneyLegButton`'s modal** (`:159-245`) uses bare `TextInput`s labelled
   "Origin CRS", "Destination CRS", "Service date" (placeholder `YYYY-MM-DD`) and
   "Train UID" — the station `Autocomplete`, `DatePickerInput` and
   `TimeFilterInput` that `TrackTrainForm` uses fifty lines away are not reused.
   This is the 09-17 §3.6 ticket-form regression reappearing in a new component.
   (No passenger knows a train UID.)

The counter-example is in the same feature set: `ScheduleRow` *is* built on
`StatusRow`, so it inherits the shrink guard rather than copying it — proof the
primitive works when used.

**Recommendation.** Treat this as one convention fix, not four bugs. Reuse
`LineStatusCard` for "Lines to watch" (which also lands I24's freshness line);
reuse `StatusRow` for the trains panel; rebuild `JourneyLegCandidates` on the
`/trains` result row (which also lands C4); rebuild the Add-a-leg modal's fields
from `TrackTrainForm`'s. 09-17's verdict — "this is a convention problem, not
three bugs" — applies again.

### I26 · Groups of identical buttons share one accessible name
**Source:** [JD] §2.2 + [JC] §2.5 + [SDL] §4.3.
**Screenshots:** `journey-167-desktop.png` (×3), `track-window-search-candidates.png`
(×4), `line-detail-lner-ecml-desktop.png` (×12).

Three independent instances of one pattern:
- **"Track this train" ×3** on `/journeys/167` (`JourneyLegCandidates.tsx:119-121`).
- **"Track this train" ×4** on `/journeys/169`.
- **"View live status" ×12** in the trains panel.

In every case the distinguishing text is a sibling `<Text>`, not part of the
button's accessible name, so a screen-reader user listing controls hears N
indistinguishable items. WCAG 2.4.4 is met via context; 2.4.9 is not, and 2.4.6
is adjacent.

**Recommendation.** One rule: any repeated action in a list gets a row-specific
`aria-label` (`"Track the 19:12 from York to Newcastle"`, `"View live status for
the 06:00 to Edinburgh"`), or the row is wrapped in a `role="group"` with an
`aria-label`. Simplest alternative for the trains panel: make the time+route the
link and drop the trailing link entirely.

### I27 · Raw CRS codes and ISO dates in user-facing copy, next to code that already fixed them
**Source:** [JC] §2.4, §2.5 + [JD] §2.2, §2.9.
**Screenshots:** `track-pick-departures-platform-badge.png`,
`track-window-search-candidates.png`, `journey-167-desktop.png`.

- Picker rows read **"18:30 · EDB · GR"** — the new `ScheduleRow` data shape
  (`:12-28`) carries only `destinationCrs`/`operator` codes, inheriting the debt
  rather than paying it.
- The open-leg card reads **"KGX → EDB, 2026-09-22"** (`JourneyLegCard.tsx:52`
  interpolates `serviceDate` raw), and the candidate rows repeat the codes
  (`JourneyLegCandidates.tsx:117`).
- The Add-a-leg modal labels fields "Origin CRS" / "Destination CRS" / "Train UID".

What makes this Important rather than Minor is the proximity of the fix:
`/track/mine` **in the same session** renders "London Kings Cross (KGX) → York
(YRK), 22 Sept 2026 · 17:00" via `routeLabel` and `formatDate`, and the
journey page's *first card* uses that format while its *second card* does not.
This is 09-17 §2.9 / 09-02 F3, with the solution already in the tree.

**Recommendation.** Use `routeLabel` and `formatDate` everywhere. Add optional
`destinationName`/`operatorName` to `ScheduleRow` and render "18:30 · Edinburgh
(EDB) · LNER".

### I28 · Four time fields for a one-field question (enhancement)
**Source:** [JC] §2.2 — flagged Important but explicitly "a recommendation, not
a defect".
**Screenshots:** `track-window-mode-{desktop,tablet,mobile}.png`.

The common cases are "leaving after 18:00" or "arriving by 09:00"; the 2×2 grid
asks the user to evaluate four bounds every time. The spec's
`TimeWindow {after, before}` shape (§2.1) justifies the data model, not the form.

**Recommendation.** A "Depart after / Depart before / Arrive after / Arrive
before" selector plus one time input, with "add another bound" for the power
case. This would also dissolve I17 and most of M12 (the mobile grid alignment
problems disappear with one field).

---

## 4. Minor findings

**M1 · Filtered `/lines` gives no count and no way back.** [SDL] §3.3.
`lines-filtered-severe-desktop.png`: two rows, no "2 of 125 lines", no "Clear
filter". Add "2 lines with Severe Disruption · Show all" above the table, and
handle the zero-row case the `0 Informational` tile produces: "No lines are
Informational right now", with "All statuses" re-offered.

**M2 · Filter chips are below the tap-target floor.** [SDL] §3.4.
`lines-unfiltered-mobile.png`: Mantine `Chip size="xs"` ≈23px against the 09-17
§2.10 floor of 24px (44px for primary controls). Six chips in a row at 390px are
the page's primary filter; `size="sm"` (28px) costs one more wrap.

**M3 · Trains-panel row polish.** [SDL] §4.3, §4.4.
`line-detail-lner-ecml-desktop.png`: "· 22m late" is `c="dimmed"` — the same grey
as "Scheduled — not live yet" — so the one live fact a traveller cares about is
styled as the least important text on the row. Use colour + text as `EtaBadge` /
`TrackedTrainStatusBadge` already do. The panel also has no context line (date,
count, timezone, last-updated); one dimmed caption fixes all four.

**M4 · Line-history empty state has no way out; Period control is full-width.**
[SDL] §5.2. `line-history-cross-country-desktop-check.png`: "No history entries
in that range." is honest but terminal — the control that would change the answer
is 40px above and nothing points at it, nor does the copy say why a line
currently at `MINOR DELAYS` has no entries in 7 days. Suggest: "No status changes
recorded for CrossCountry in the last 7 days. Try **30 days**, or see the
**Trends** tab…" with both linked. Separately, three segmented options stretched
across 1100px put the labels ~370px apart; `w="fit-content"` keeps it reading as
one control.

**M5 · The global error boundary says "Back to your dashboard" to anonymous
users.** [SDL] §5.1. `line-history-lner-ecml-desktop.png` is an anonymous
capture; there is no "your dashboard" and the link goes to `/`. The boundary is
shared across every route, so the copy must work for everyone: "Back to the home
page".

**M6 · Operator card pin stars drift vertically across a row.** [OH] §2.4.
`operators-list-desktop.png`: the footer row isn't anchored to the card bottom
(Avanti y≈292 vs CrossCountry/GWR y≈333), so scanning a row for "which are
pinned" means hunting. `Card` as a flex column with the footer on
`marginTop: 'auto'` fixes it.

**M7 · `/operators` has no intro line, no ordering cue and no filter; nav
placement.** [OH] §2.6, §2.7. `stations-desktop.png` opens with a one-sentence
description under its `<h1>`; `/operators` goes straight from title to cards, and
they are alphabetical where the homepage's pinned lines are explicitly worst-first.
Nine cards are fine alphabetically; 25–40 will not be. Also, "Operators" is the
last nav item, after "Incident Archive", when it is a sibling of "All Lines" and
"Station Lookup" (`lib/navLinks.ts:34`).

**M8 · Homepage copy never mentions the third pin kind.** [OH] §4.4.
`homepage-operator-overview-logged-out-desktop.png`: the tagline says "pin the
lines and stations you care about", the CTA says "Log in to pin your lines and
stations" (`app/page.tsx:293`), and the quick links offer "Browse all lines ·
Look up a station" with no "Browse operators". These are the only places the
anonymous homepage could advertise that the new page exists.

**M9 · Pin control parity across homepage card types.** [OH] §4.3.
`homepage-operator-overview-authenticated-desktop.png`: "Your Operators" cards
show a filled star, "Your Lines" cards show none. Useful, but present on one
section and absent on the other it reads as an oversight. Either add unpin to
line cards or add a `showPin` prop and drop it from operator cards here.

**M10 · The same disruption is printed three times on the homepage.** [OH] §4.5.
The LNER signalling failure appears as the LNER ECML line card, in the Kings
Cross station row's stats, and again in full on the LNER operator card. Expected
for a demo user who pinned all three, and the source already de-duplicates TfL
lines against "Your Lines" — extend the same courtesy when an operator's only
affected line is already pinned ("See LNER East Coast Main Line above"), or clamp
the operator card's reason to one line on the homepage.

**M11 · History page title formats differ.** [OH] §3.5. The new pages use
"History: {name}" and "Network history" where `/lines/[id]/history` presumably
uses the line name. Pick one pattern across all three.

**M12 · Window-form mobile and desktop layout.** [JC] §2.3, rec. 9.
`track-window-mode-mobile.png`: the 2×2 grid stays two-column at 390px (`Group
grow`), and because the descriptions wrap to different break points the two
arrival inputs sit at different heights (y≈773 vs y≈787). The per-input clock
`ActionIcon` is `size="md"` = 28px, under the 44px asked for tap-first controls.
The submit lands at y≈820-857, the very edge of an 844px viewport before any
keyboard appears. `SimpleGrid cols={{ base: 1, sm: 2 }}` fixes alignment and
width. Separately, `track-pick-mode-mobile.png` shows the "Now" button orphaned
on its own row (`Group align="flex-end"` wrapping) — `wrap="nowrap"` plus
`flexShrink: 0` keeps it beside the picker. At 1440px every input is ~1100px
wide; `/groups` already uses `maw={640}` for exactly this reason.

**M13 · The Date field's default is a placeholder, not a value.** [JC] §2.2,
rec. 10. `DatePickerInput placeholder="Today"` with `windowServiceDate` initially
`null`, which `submitWindow` resolves to today (`:637`) — so the field is *empty*
but *means* today, and grey placeholder text is indistinguishable from a hint.
Pick mode twenty pixels away shows its default as a real value ("22/09/2026
18:17"). Seed the value, or say "Defaults to today".

**M14 · Three phrasings of one description, and no earliest > latest check.**
[JC] §2.2. "Only trains at KGX at or after this time" / "Only trains at or before
this time" (no "at the origin", `:1058`) inside one grid, and "Only trains
departing at or after this time" in `AddJourneyLegButton.tsx:193`. Separately,
`handleSubmit` (`:697-705`) checks only presence, not ordering — flagged so it
gets a test.

**M15 · Open-leg copy describes a spinner, and nothing says the pick is
reversible.** [JC] §2.5 + [JD] §2.2. "Searching for a train to track — pick one
below" is near-identical to the actual loading state "Searching for candidate
trains…" (`JourneyLegCandidates.tsx:88-91`) — two near-identical sentences for
opposite states, and the first primes the 09-02 F13 "unbounded pending" problem
when the honest state is "we found four; you choose". The §2.3 decision that a
pick can be swapped later is a real reassurance that is invisible until after the
irreversible-looking click. One sentence covers both: "4 trains match your
search. Pick the one you'll be on — you can change it later."

**M16 · "Add a leg" is offered before the first leg has a train.** [JC] §2.5,
rec. 7. Offered at the same visual weight as the status badge on a journey that
needs a train picked. Hide or demote it until the current leg is matched. (Its
modal's field quality is I25.)

**M17 · The journey status badge's tooltip repeats its own text and is
keyboard-unreachable.** [JC] §2.5 + [JD] §2.3. `JourneyStatusBadge.tsx:27-31`; a
`Tooltip` on a non-focusable `Badge` adds nothing and cannot be reached by
keyboard. Drop it, or make it say something the badge doesn't ("Leg 2 has no
train picked yet").

**M18 · Generic `<h1>` "Tracked journey".** [JC] §2.5. The fallback for a null
`customName` (`page.tsx:60`); the spec §4 rename pattern is noted as deferred in
the code. Until it lands, default the title to the route ("London Kings Cross →
Edinburgh, 22 Sept"), as 09-17 recommended for the train page.

**M19 · Login-modal copy contradicts the mode.** [JC] §2.5. In window mode the
submit says "Search for a train" but the 401 modal says "Log in to track this
train." (`TrackTrainForm.tsx:1195`).

**M20 · `/groups` empty state stops short in five ways.** [JC] §2.7, rec. 11.
`groups-empty-state.png`. It is honest and names its CTA — but: two links for the
same action 40px apart in two styles ("Create group" grape `TextLink` in the
header, "Create one" plain underlined inline); the header CTA floats at x≈720 on
a 1440px viewport, aligned to nothing; there is no path for the *more common*
case of being invited ("…or ask a group member for their invite link"); "tracked
trains" undersells what groups now share (the page's own metadata says "tracked
trains and custom lines", and this branch adds `ShareJourneyButton`); and the
only guidance on the page is `c="dimmed"` — empty-state copy is content, not
metadata.

**M21 · Two permanently empty columns in every journey table.** [JD] §2.10.
All five shots: Delay and Platform are empty on every row of both journeys.
Platform is origin-only by design (`JourneyTimeline.tsx:298-303`) so it will
usually be ~80% empty; Delay is `null` per stop while the train is 22m late and
every "est." is +22 — the data contradicts itself, since the est. is propagated
*from* the delay the Delay column calls unknown. Two blank columns read as
"broken" and they cost the mobile width in I5. Hide a column when no row has
data; propagate train-level delay or drop the column.

**M22 · "On track" vs "On time", one line apart.** [JD] §2.11.
`journey-168-desktop.png`: header badge "On track", leg badge "On time". Two
phrases for one state, adjacent. Use "On time" at both levels, or "All legs on
time" if a distinction is wanted.

**M23 · The mode switch is silent, and its segments are 32px.** [JC] §2.1.
The toggle swaps ~400px of form with no announcement; a visually-hidden
`aria-live` ("Showing time-window search") is cheap. Segments are ~32px tall at
every width — above the 24px floor but below the 44px recommended for primary
actions, and this is the primary fork in the flow.

**M24 · Departure-picker density is untested.** [JC] §2.4. The capture has one
row. At ten rows (the documented LDBWS default) each with two badges, the right
column is ~220px of badges per row on desktop and will crush the title on mobile.
`StatusRow`'s shrink rule protects the badges; the title gets `lineClamp`. Needs
a capture with a populated board at 390px.

### Pre-existing — noted, not attributed to these features

- **Homepage heading levels.** "Your Lines" is `<h1>` while "Your Stations" /
  "Your Operators" / "Right now" are `<h2>` (`app/page.tsx:471,504,551,647`). The
  new section copies the existing convention faithfully; the convention itself
  gives the page an `<h1>` that is one of three peers. [OH] §4.6.
- **Theme-toggle tofu.** The blank square button between the accessibility icon
  and the avatar in every journey shot is the theme-toggle emoji failing to
  render (09-17 §2.3). [JD] §2.13.
- **`/track/mine` reliability card copy.** "Track a train and check back once
  it's finished running" while both rows are EN ROUTE (09-17 §3.6). [JC] §2.6.
- **Anonymous pin affordance on `/operators`.** The manifest describes the
  logged-out shot as "without any pinning UI available", but the star is present —
  only its `aria-label` ("Pin — needs an account") and the click-time
  `LoginPromptModal` differ. This matches `/lines` and `/stations/[crs]` exactly,
  so it is consistent with precedent, not a defect; recorded because the manifest
  and the rendering disagree. [OH] §2.5.
- **Firefox parity.** `status-dashboard-desktop-firefox.png`,
  `operators-list-desktop-firefox.png` and `journey-167-firefox-desktop.png` are
  all layout-identical to Chromium apart from the known font fallback (09-17
  §2.15) and 1px of nav height. Nothing regressed cross-browser. [SDL] §2.6,
  [OH] §2.8, [JD] §3.

---

## 5. Disagreements, dependencies, and demo-data artefacts

### 5.1 Where the reviews disagree

**Badge contrast — [JC] vs [JD].** [JD] §4 states every badge in the journey
shots clears AA, citing `globals.css:157-160` light-variant overrides (orange
`#bb3e0d`, green `#267b37`) plus Mantine's native blue-light (4.93:1). [JC] §2.4
estimates `PlatformBadge`'s orange-light at ≈2.6:1 and `/track/mine`'s
yellow-light at ≈1.9:1. **These are not actually contradictory**: [JC] computed
from raw Mantine tokens, [JD] read the shipped override file, and the override
covers orange and green but apparently not yellow. **Resolution:** run axe
against the deployed build. The orange finding may already be fixed by
`globals.css`; the yellow "22M LATE" almost certainly is not, and the `/status`
"By mode" `light` badges ([SDL] §2.3) were never measured at all. Fix once at the
token layer either way (I19).

**"running.Rates" missing space — source vs running instance.** [OH] §3.3 found
the *current source* (`NetworkTrendsResults.tsx:77`) has the space, while
`network-history-desktop.png` renders without it. Either the running instance
predates a fix or JSX whitespace collapsed. **Verify against the deployed build
before filing a fix.**

**Mobile pin star on `/lines` — [SDL] vs the 09-17 review.** 09-02 F4 said mobile
users cannot pin from the All Lines table; 09-17 §5 recorded it fixed ("keeps the
pin star"). `lines-unfiltered-mobile.png` shows no star. Most likely the fix
landed and is now hidden by the 420px scroll container (I5) rather than having
regressed — but the user-visible outcome is identical to F4, so it should be
re-verified as part of I5, not assumed fixed.

**12-hour time skeleton — capture environment or product?** [JC] §2.2 notes
`lang="en-GB"` was added specifically to prevent this and cannot tell whether it
fails in headless Chromium or was never applied on this branch. Could be a
screenshot artefact. Verify in a real browser before changing the approach (I23).

### 5.2 Findings that are partly or wholly demo-data artefacts

The screenshot set used fabricated seed data. Six findings are entangled with it:

| Finding | Seed contribution | Genuine defect remaining |
|---|---|---|
| **I14 "Change train" invisible** | The seeded leg has no `departAfter`/etc., so `hasWindow` is false and the button never renders. **Entirely a seed limitation** — the control has zero screenshot coverage. | Yes: weak placement below a six-row table, and *no* recovery path at all (no change, no delete) for a no-window leg. |
| **I13 skip badge / rollup** | No seeded leg skips a stop, so the badge never renders in any shot. | Yes, and verified in code: `legStatusGroup` ignores `legSkip`, and `isSkippedOnLeg` doesn't suppress "est." — a confident wrong ETA. Independent of the seed. |
| **C3 position contradiction** | Plausibly a seed that set `lastReportedLocation` without `actual*` times on the matching stop. | Yes: the UI should defend against the inconsistent state regardless. Two fixes — defensive UI, *and* verify the data pipeline on real movements. |
| **I16 blank terminus row** | Likely a fixture with arrival-only data that `scheduledDeparture ?? scheduledArrival` should already handle. | **Escalate to Critical if real data also lacks it** — on `journey-168` that blank cell is the traveller's own arrival time. Check before deciding severity. |
| **I2 "Unknown station"** | Depends on how often live records lack a schedule match in production; the seed may over-represent it. | Yes: the fix (build the label from `callingPoints`, never print the fallback constant as a whole label) is correct regardless of frequency. |
| **I4 TfL "0 lines tracked"** | Zero TfL lines is a seed artefact. | Yes: the false-reassurance rendering (green `ALL GOOD SERVICE` over an empty set) will fire any time a mode has no lines. |

Two further seed/capture notes: `/operators`' nine alphabetical cards are fine as
captured but the homepage source says production holds 25–40, which is what makes
M7's ordering finding real; and the byte-identical
`track-mine-authenticated.png` / `track-mine-journeys.png` pair is a capture
artefact that happens to *prove* C1.

**Action:** extend the screenshot seed before the next review round with (a) a
matched **windowed** leg, (b) a leg **skipping** its destination, (c) a leg with
real `actual*` times on departed stops, (d) at least one TfL line, and (e) a
populated 10-row departure board at 390px. Also add dark-mode captures — none of
the 11 journey-creation captures is dark, and both contrast findings will differ
there.

### 5.3 Where one fix lands another

- **I1 (operator card affordance) → C2.** Making `OperatorStatusCard` link to its
  history page resolves the operator half of the unreachable-routes Critical. Do
  them as one change.
- **I25 (reuse the `/trains` result row) → C4 and I26.** Rebuilding
  `JourneyLegCandidates` on the existing result row delivers the arrival time,
  duration, operator and per-row link that C4 needs, and a row that already has a
  per-row link likely resolves the accessible-name collision too.
- **I22 (unify the noun, route list rows to `/journeys`) → C1.** The list work is
  the same work.
- **I28 (one selector + one time input) → I17 and most of M12.** Collapsing the
  2×2 grid dissolves the "(optional)" contradiction *and* the mobile alignment
  problem rather than patching both.
- **I25 (reuse `LineStatusCard` for "Lines to watch") → I24.** The primitive
  already renders the freshness line the hand-rolled row dropped.
- **I19 fixed at the token layer → three surfaces at once**: `PlatformBadge`,
  `TrackedTrainStatusBadge`, and the `/status` "By mode" badges.
- **I9 / I10 are shared components** — fixing the granularity label and the
  Trends copy also improves `/lines/[id]/history`, which is *already* reachable.
- **C2 raises the priority of I9, I10 and I11.** Those three findings are
  currently on pages nobody can navigate to; linking the pages makes them
  user-visible for the first time.
- **I18 (show the window) depends on I21 (`?mode=window`)** for its "Edit search"
  affordance to have anywhere to link.
- **I5 is one idiom in two places** — `/lines` and the journey timeline both use
  `TableScrollContainer minWidth={420}` against a 390px viewport. Fix the pattern,
  not the pages.

---

## 6. Cross-cutting patterns

These showed up in more than one themed review and are worth fixing at the root.

**P1 · Orphaned routes.** Two of the four reviews independently found a shipped
page or object with no inbound link: the operator/network history pages (C2) and
window-search journeys (C1). In both cases a *back*-link exists pointing at a
parent that has never heard of the child. The root fix is process, not code: add
link/nav coverage to the e2e suite so a route with no inbound `href` fails CI,
and treat "where does this link from?" as a checklist item when a route is added.

**P2 · Re-implementation drops the rule the primitive encodes.** Four instances
across three reviews (I25): `StatusRow`'s shrink guard, `LineStatusCard`'s
freshness line and sample summary, the `/trains` result row's content and per-row
link, and `TrackTrainForm`'s `Autocomplete`/`DatePickerInput` field set. Each
re-implementation looks fine in isolation and each loses something the original
learned the hard way — in the candidate-list case, enough to make a Critical
finding. 09-17 already called this "a convention problem, not three bugs"; it
recurred in both new features.

**P3 · Repeated actions share one accessible name.** Three instances (I26):
"Track this train" ×3, ×4, and "View live status" ×12, in two different features
by two different authors. The distinguishing text is always a sibling `<Text>`
rather than part of the name. Worth a lint rule or a shared list-row component
that takes a `rowLabel` and composes the `aria-label` for you.

**P4 · Control groups ship without accessible names.** Two instances (I9): the
granularity control and the `/track` mode toggle, plus possibly-missing
`aria-sort` on `/lines` headers. Both are Mantine `SegmentedControl`s where a
sibling control in the same view *does* have a label — so the fix is a convention
("every `SegmentedControl` takes a label"), not a one-off.

**P5 · Fallback strings and internal codes reaching user copy.** "Unknown
station" as an entire row label (I2), "KGX → EDB, 2026-09-22" (I27), "Origin CRS"
/ "Train UID" as field labels (I25), `--` as a dash and "poll cycles" as
vocabulary (I10). Every one of these has a solved sibling *in the same codebase*
— `routeLabel`, `formatDate`, `/track/mine`'s row titles, the app's em-dash copy.
The display-label layer 09-17 asked for exists and is landing; new components
keep being written beneath it rather than on top of it.

**P6 · Worst-status rollups lose the information they summarise.** The operator
card shows one line's reason as the operator's status with no scope (I11); the
journey badge shows "Needs a train picked" and suppresses a 22-minute delay
(I12); `legStatusGroup` doesn't consult `legSkip` at all (I13); the "By mode"
card colours "5 affected" yellow when two are Severe (I4). Four instances across
three reviews of the same shape: a rollup that is correct about its worst input
and silent about everything else. A shared rule would help — a rollup states its
scope ("worst of N"), and never hides a second status that the user needs to act
on.

**P7 · Nothing says when the data is from.** `/status` says "right now" with no
timestamp; the trains panel has no date or freshness; the journey page never says
when "22m late" was measured (I24). 09-17 §2.12 raised this app-wide as moderate;
all three new surfaces inherited it. `LastUpdated` exists and `LineStatusCard`
already uses it.

**P8 · 390px is 30px too narrow, twice, with no scroll cue.** Both `/lines` and
the journey timeline use `TableScrollContainer minWidth={420}` inside a 390px
viewport, hiding a Pin column and two table columns respectively behind an
unsignalled sideways swipe (I5). The container is the right tool for a genuinely
wide table and the wrong one for a near-miss.

**P9 · The app's own conventions are the bar, and they're mostly met.** Worth
recording: all four reviews independently confirm the house rules are holding
where the existing primitives are used — `StatusBadge` is colour + text
everywhere it appears, `PinToggle` is 44px with a state-naming `aria-label`,
badges don't truncate, empty states are honest and don't fabricate data,
`ScheduleRow` correctly inherits `StatusRow`, the error boundary shows a digest
rather than a stack trace, and Firefox is clean in all three feature areas. The
findings above are almost entirely about new code written *beside* those
conventions rather than the conventions failing.

---

## 7. Suggested order for a fix pass

1. **Unreachable routes (C1 + C2)** as one "no orphaned pages" change, including
   the e2e link coverage that stops it recurring. Linking `OperatorStatusCard` to
   its history page lands I1's operator half; the `/track/mine` journeys work
   lands I22.
2. **Leg-scoped candidate rows (C4)**, implemented by rebuilding
   `JourneyLegCandidates` on the existing `/trains` result row (I25) with
   per-row `aria-label`s (I26). Confirm first that the candidate endpoint can
   return leg-origin departure and leg-destination arrival — if it can't, that
   backend gap blocks the feature's value.
3. **The position contradiction (C3)**: the defensive UI fix, plus a check of
   whether `lastReachedIndex = −1` with a listed `lastReportedLocation` happens on
   real movement data or only in the seed.
4. **`legStatusGroup` must consult `legSkip` (I13)** — a small correctness fix
   with a badly wrong output ("On track" for a train that isn't stopping where you
   need it), plus suppressing the misleading "est." on a skipped row.
5. **The two root-cause batches**, each cheap relative to its spread: the
   `variant="light"` badge contrast fix at the token layer (I19, three surfaces),
   and accessible names for control groups and repeated buttons (I9 + I26, five
   surfaces).

Before the next review round, **extend the screenshot seed** per §5.2 — six
findings currently rest on data the captures cannot exercise, and two severity
calls (I16, I2) can't be settled without it.
