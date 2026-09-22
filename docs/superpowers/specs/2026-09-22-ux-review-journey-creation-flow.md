# UX / accessibility review — journey creation and search flow (2026-09-22)

Scope: the `/track` form's two modes ("Pick a departure" / "Search a time
window"), the live departure picker's platform + delay badges, the
candidate-list state of `/journeys/[id]` reached after a window search, the
`/track/mine` list, and the `/groups` empty state. Eleven screenshots under
`frontend/e2e/screenshots/output/`, all Chromium, all authenticated; the
manifest is `frontend/e2e/screenshots/output/manifest.ndjson`.

The code that produced these screenshots is **not** on `main`. It is the
`integration-preview` branch checked out at
`/home/coder/Distant-Signal/.integration-worktree` (commit `c84dfb4e`). File
paths below that start with `.integration-worktree/` refer to that checkout;
paths without the prefix are identical on `main` and there. Where a claim
depends on code rather than pixels, the file and line are cited so the
collating reviewer can verify it.

The bar applied is the one the codebase already set for itself:
`2026-09-17-full-service-ux-accessibility-usability-review.md` (§2.5 shrink
guards, §2.9 no internal codes in copy, §2.10 24px targets, §2.13 one idiom
for "pick exactly one", §3.6's Track/Train findings) and
`2026-09-02-frontend-ui-ux-review.md` (F3 raw CRS codes, F6 two-step commit,
F9 pickers that offer invalid values, F13 unbounded pending states). The
feature spec is `2026-09-22-journey-tracking-design.md`, especially §2.3 and
§4.

---

## 1. Summary

The new mode toggle is a competent piece of UI — a real `radiogroup`, a clear
selected state, both modes share the Origin field so switching loses nothing,
and the submit label changes with the mode. It does **not** look like a
broken form. The four time-window fields have plain-English descriptions,
and the platform badge does the WCAG 1.4.1 thing correctly by naming both
platforms in text.

The problems are around the edges of that form, and one of them is
structural:

1. **Critical — a window-search journey is unreachable once you leave its
   page.** `GET /Journeys/mine` is wired into `lib/api.ts` but has no caller;
   `/track/mine` still lists tracked *trains* only (an unmatched leg has no
   train subscription, so it can never appear there); there is no nav entry
   and no URL for the mode. Close the tab after
   `track-window-search-candidates.png` and the journey is gone unless you
   remember `/journeys/169`.
2. **Important — the toggle is unlabelled and its labels describe
   mechanics, not the user's situation.** Nothing on the page says *why* there
   are two options; a screen reader announces an unnamed radiogroup; "Search a
   time window" is rail-app vocabulary, and the sibling "Add a leg" modal
   already uses the better pair ("I know the train").
3. **Important — every time-window field says "(optional)" but at least one
   is required**, and the rule surfaces only as a post-submit error.
4. **Important — the candidate list hides the one thing the user just
   typed (the window), shows no arrival times, uses raw codes and an ISO
   date, and gives four identical "Track this train" buttons.**
5. **Important — the "Platform 2 (changed from 1)" badge is orange-6 on a
   pale-orange tint, roughly 2.6:1 for 11px bold text** (estimated from the
   Mantine token; measure with axe). The neighbouring filled "+11 MIN" badge
   is fine.

Nothing here contradicts the spec's §2.3 decision (manual pick, persisted
window, "Change train"); the findings are about how that decision is
surfaced.

---

## 2. Findings by theme

Severity scale: **Critical** (blocks the feature's purpose or a WCAG A/AA
criterion on a primary path), **Important** (real confusion or an AA
failure a user will hit), **Minor** (polish, or a regression of a stated
house convention with a small blast radius).

### 2.1 The pick-vs-window mode toggle

Screenshots: `track-pick-mode-desktop.png`, `track-pick-mode-tablet.png`,
`track-pick-mode-mobile.png`, `track-window-mode-desktop.png`,
`track-window-mode-tablet.png`, `track-window-mode-mobile.png`.
Code: `.integration-worktree/frontend/components/TrackTrainForm.tsx:998-1009`.

**What it is.** A full-width Mantine `SegmentedControl` directly under the
Origin field, options "Pick a departure" / "Search a time window". The
selected segment is a white pill on a grey track. Mantine 9.5.2 renders it
as `role="radiogroup"` with native radio inputs (verified in
`frontend/node_modules/@mantine/core/esm/components/SegmentedControl/SegmentedControl.mjs:74,119`),
so arrow-key navigation and radio semantics come for free.

**Is it obviously a mode switch, or a broken form?** A mode switch. Across
all six captures the selected state is unambiguous, the control is the same
width as the inputs around it, and the form below visibly changes shape
(pick: Scheduled departure + Now / Destination (optional) / Operator
(optional) / picker text; window: Destination / Date / 2×2 time grid). The
submit label follows the mode ("Track this train" vs "Search for a train",
`TrackTrainForm.tsx:1178-1186`), which is the single best cue on the page
that window mode does not immediately track anything. The Origin field
sitting *above* the toggle correctly signals "this part is shared".

**Important — the group has no visible or accessible label.** There is no
heading, legend, or `aria-label` on the `SegmentedControl`
(`TrackTrainForm.tsx:1002-1009` passes only `value`/`onChange`/`data`). A
sighted user sees two pills and has to infer from their wording that they
are alternatives; a screen-reader user lands on "radiogroup, Pick a
departure, radio button, 1 of 2, checked" with no name for the group. This
is the *only* discovery path for window mode (the mode is not URL-addressable
— confirmed: `.integration-worktree/frontend/app/track/page.tsx:47-63` reads
only `origin` and `ticketId`), so the label is load-bearing. One line above
the control — "How do you want to find the train?" — rendered as the group's
label fixes both halves.

**Important — the labels name the mechanism, not the user's situation.**
"Pick a departure" and "Search a time window" are both verbs the *form*
performs. The distinction a passenger actually has in their head is "I know
which train I'm getting" versus "I'll be on something between about 6 and 7".
The sibling `AddJourneyLegButton` modal
(`.integration-worktree/frontend/components/AddJourneyLegButton.tsx:164-167`)
already uses "I know the train" for its second option — so the codebase has
two different labels for the same choice, and the better one is on the less
visible control. "Time window" is also planner jargon; "(optional)" fields
below it then have to explain what a window is by example.

**Important — the page never introduces the second mode.** The `<h1>` is
"Track a Train" and the dimmed intro reads "Pin a specific train to see its
live position…" (`track/page.tsx:71`; identical in every capture). That
sentence is true of pick mode only. A user who arrives to find a train they
half-know is told this page is for *specific* trains and has no reason to
look at the toggle's second pill. The metadata description
(`track/page.tsx:38`) has the same gap.

**Important — not linkable.** Because the mode lives only in `useState`
(`TrackTrainForm.tsx:348`), nothing else in the app can send a user straight
to window mode: not the candidate list's own "Search manually" fallback
(`JourneyLegCandidates.tsx:104` links to bare `/track`, which lands on
*pick* mode), not the add-ticket page, not a help link, not a bookmark. The
sibling `?origin=` param shows the pattern is already established
(`track/page.tsx:50-56`); `?mode=window` is one line.

**Minor — switching modes is silent.** The toggle swaps roughly 400px of
form beneath it with no announcement. Sighted users see it; screen-reader
users get nothing until they tab forward. A visually-hidden `aria-live`
"Showing time-window search" is cheap. Not blocking.

**Minor — target height.** Segments are ~32px tall at every width
(`track-pick-mode-mobile.png`). Above the 24px floor the 09-17 review set
(§2.10) but below its 44px recommendation for primary actions; this is the
primary fork in the flow.

### 2.2 The time-window fields

Screenshots: `track-window-mode-desktop.png`, `track-window-mode-tablet.png`,
`track-window-mode-mobile.png`.
Code: `TrackTrainForm.tsx:1012-1085`, `TimeFilterInput.tsx`.

**What works.** Each of the four fields has a one-line description in plain
English, and the earliest-departure description names the origin once one
is entered ("Only trains at KGX at or after this time",
`TrackTrainForm.tsx:1049`). Half-typed times block submission with an
explanatory message rather than silently vanishing (`TimeFilterInput.tsx:145-171`,
`windowIncompleteTimes`). The submit button is never disabled, so an invalid
press produces an `Alert` rather than a dead button (`TrackTrainForm.tsx:694-713`).

**Important — "(optional)" on all four, but the form refuses to submit
with none.** `windowHasABound` (`TrackTrainForm.tsx:380-383`) requires at
least one of the four; the only place this is stated is the post-submit
error "Enter at least one earliest/latest departure or arrival time to
search a window." (`TrackTrainForm.tsx:702`). The labels in every capture
say the opposite. This is the F9 pattern from the 09-02 review — the UI
documents a rule everywhere except in the control that enforces it — in
reverse: the labels actively promise the wrong thing. Either drop
"(optional)" and add a group-level sentence ("Give at least one time"), or
keep "(optional)" and say "at least one of these four" under the grid.

**Important — four fields for a one-field question.** The common cases are
"leaving after 18:00" or "arriving by 09:00"; the 2×2 grid asks the user to
evaluate four bounds every time. The spec's `TimeWindow {after, before}`
shape (§2.1) justifies the data model, not the form. A "Depart after / Depart
before / Arrive after / Arrive before" *selector* plus one time input, with
"add another bound" for the power case, would cover the same request shape
with a quarter of the reading. Recommendation, not a defect.

**Important (needs live verification) — the 12-hour "--:-- --" skeleton is
still rendered.** All three window captures show three segments with the
AM/PM slot, on a site where every displayed time is 24h. `TimeFilterInput.tsx:205-221`
sets `lang="en-GB"` specifically to stop this (Task 3.6.13), and the 09-17
review §3.6 already listed it as a defect on `/trains`. Either the fix does
not take effect in headless Chromium, or it is not applied on this branch;
either way the screenshots show the bug. A native `<input type="time">`
placeholder can't be set, so if `lang` proves unreliable the alternative
the 09-17 review named (text input, `placeholder="HH:MM"`,
`inputMode="numeric"`) is the fallback.

**Minor — the Date field shows "Today" as a placeholder, not a value.**
`DatePickerInput placeholder="Today"` (`TrackTrainForm.tsx:1038-1044`) with
`windowServiceDate` initially `null`, which `submitWindow` then resolves to
today (`:637`). So the field is *empty* but *means* today, and the grey
placeholder text is indistinguishable from a hint. Pick mode, twenty pixels
up in the other branch, shows its default as a real value ("22/09/2026
18:17"). Seed the value with today's date, or change the placeholder to
"Defaults to today".

**Minor — the same field is described three different ways.** "Only trains
at KGX at or after this time" / "Only trains at or before this time" (no
"at the origin", `:1058`) inside one grid; "Only trains departing at or
after this time" in `AddJourneyLegButton.tsx:193`. Pick one phrasing.

**Minor — no visible earliest > latest check.** Cannot be judged from
screenshots; flagging so it is tested. `handleSubmit` (`:697-705`) checks
only presence.

### 2.3 Mobile usability of the window form

Screenshot: `track-window-mode-mobile.png` (390×844). Compare
`track-pick-mode-mobile.png`.

**What works.** The pick-mode form, including its submit, fits inside one
390×844 viewport. The window-mode labels wrap cleanly ("Earliest departure"
/ "(optional)") rather than truncating, and the toggle's two labels fit on
one line each at ~13px.

**Minor — the 2×2 grid stays two-column at 390px and misaligns.** `Group
grow` (`TrackTrainForm.tsx:1045,1065`) keeps the pairs side-by-side at
~170px each. Because the two descriptions wrap to different line counts
("Only trains reaching the destination at or after this time." = 3 lines;
its sibling = 3 lines but different breaks), the two inputs in the arrival
row sit at different heights (y≈773 vs y≈787 in the capture). The
per-input clock `ActionIcon` is `size="md"` = 28px, under the 44px the 09-17
review asks for on tap-first controls (§2.10). The submit button lands at
y≈820-857, i.e. at the very edge of an 844px viewport before any keyboard
appears. Stacking to one column below `sm` (`SimpleGrid cols={{ base: 1,
sm: 2 }}`) fixes alignment and gives each field full width; it costs ~120px
of height, which is fine because the four fields would no longer need
three-line descriptions.

**Minor — the "Now" button orphans onto its own row in pick mode at
390px** (`track-pick-mode-mobile.png`, y≈500-540): `Group align="flex-end"`
wraps, leaving a lone 68px button under a full-width picker. Pre-existing on
`main`, but the window branch's new layout makes the contrast visible.
`wrap="nowrap"` with the button `flexShrink: 0` (the §2.5 idiom) keeps it
beside the picker.

**Minor — at 1440px, every input is ~1100px wide** (`track-window-mode-desktop.png`,
`track-pick-mode-desktop.png`): a five-character time in a 540px box, a
three-letter station code in an 1100px box. Groups already uses `maw={640}`
(`app/groups/page.tsx:89`) for exactly this reason; the form should too.

### 2.4 The departure picker's platform and delay badges

Screenshot: `track-pick-departures-platform-badge.png` (`/track?origin=KGX`,
1440×900). Code: `.integration-worktree/frontend/components/PlatformBadge.tsx`,
`ScheduleRow.tsx`.

The single picker row reads "18:30 · EDB · GR" with, right-aligned, a pale
orange "Platform 2 (changed from 1)" badge and a filled orange "+11 MIN"
badge.

**What works.** The platform badge carries the whole fact in text —
current platform *and* the one it changed from — so it survives colour
blindness, greyscale, and a screen reader; `tt="none"` keeps it readable.
The `data-platform-changed` attribute means tests assert the state rather
than the hue. The row is built on `StatusRow`, so the shrink guard the 09-17
review demanded (§2.5) is inherited rather than re-implemented. The row is
a `role="button"` with `tabIndex` and Enter/Space handling; a cancelled row
is correctly not clickable. The filled "+11 MIN" badge is dark text on
orange (~8:1) — legible.

**Important (measure) — the changed-platform badge fails AA text
contrast.** `PlatformBadge.tsx:38` uses `variant="light"` with
`color="orange"`. Mantine's light variant in the light scheme paints the
text in `orange-6` (`#fd7e14`) over a 10% orange tint; `#fd7e14` on white
is ≈2.6:1, and the tint does not help. The badge is 11px, 700 weight —
"small text" under WCAG 1.4.3, which needs 4.5:1. Estimated, not measured:
run axe on this capture. The same token pair is what made the 09-17 review
call the dark-mode "4M LATE" badge borderline (§3.6). Fix at the source —
`variant="light"` with `autoContrast` does not change the *text* colour;
use `color="orange.8"`/`orange.9` for the text (Mantine `c` prop on the
Badge) or switch the badge to `variant="outline"` with a dark orange, and
apply the same fix to the yellow "22M LATE" pill on `/track/mine` (§2.6
below), which is worse.

**Important — two adjacent orange badges encode two unrelated facts.**
Platform-changed (orange light) sits 8px from delayed (orange filled). At a
glance the row reads "two orange warnings" and the user has to read both to
learn one is "where to stand" and the other "how late". Colour is not the
only cue (the text is), so this is not a 1.4.1 failure — but the choice of
the *same* hue for *different* categories is the reverse of what the app
does everywhere else (green/orange/red map to one severity axis). A neutral
or blue platform badge with a small platform glyph, reserving orange for
lateness, restores the convention. If orange must stay for "changed", put
the platform badge on its own line under the title on small widths.

**Minor — raw codes in the row title.** "EDB · GR" is exactly the §2.9
class the 09-17 review lists; the `/track/mine` rows twenty pixels away on
another page now say "York (YRK)". The picker has always done this on
`main`; the new `ScheduleRow` data shape (`ScheduleRow.tsx:12-28`) carries
only `destinationCrs`/`operator` codes, so it inherits the debt rather than
paying it. Worth fixing while the component is new: add optional
`destinationName`/`operatorName` and render "18:30 · Edinburgh (EDB) · LNER".

**Minor — density untested.** One row in the capture. At ten rows (the
documented LDBWS default) each with two badges, the right column will be
~220px of badges per row on desktop and will crush the title on mobile.
`StatusRow`'s shrink rule protects the *badges*; the title gets
`lineClamp`. Needs a mobile capture with a populated board.

### 2.5 The candidate list after a window search

Screenshot: `track-window-search-candidates.png` (`/journeys/169`,
1440×900). Code: `.integration-worktree/frontend/app/journeys/[id]/page.tsx`,
`components/JourneyLegCard.tsx:47-74`, `components/JourneyLegCandidates.tsx`.

The page shows `<h1>` "Tracked journey", a blue "Needs a train picked"
badge, an "Add a leg" button, and one card: "KGX → EDB, 2026-09-22",
"Searching for a train to track — pick one below.", then four rows "18:00 ·
KGX → EDB" … "21:00 · KGX → EDB", each with a filled "Track this train"
button.

**What works.** Each candidate has a real `<Button>` (not a clickable div),
with a `loading` state and the sibling buttons disabled during a pick
(`JourneyLegCandidates.tsx:119`); a failed pick shows an `Alert`; the
zero-result state links out ("Search manually"). The 401 branch of the page
("Someone's tracked journey — log in to see it", `page.tsx:45`) matches the
posture the 09-17 review asked for on the by-id train page. The status badge
copy "Needs a train picked" is human. The whole state is honest — nothing
pretends a match exists.

**Critical — this page is the only place the journey exists, and nothing
links to it.** See §2.6 for the `/track/mine` half. From this page's own
point of view: no breadcrumb, no "← My journeys", and the nav has no journey
entry (`.integration-worktree/frontend/lib/navLinks.ts` — no match for
"journey"). The user got here by a `router.push` (`TrackTrainForm.tsx:654`).
If they tap "Status" in the header, the journey is unreachable except via
browser history. Every other private object in this app — trains, tickets,
groups, custom lines — has a list page; this one does not, and its status
("needs a train picked") is precisely the one that needs the user to come
back.

**Important — the window the user just entered is not shown.** The card
header is origin, destination, date (`JourneyLegCard.tsx:51-53`). The
`departAfter`/`departBefore`/`arriveAfter`/`arriveBefore` values are on
`leg` (they drive `hasWindow` at `:41-45`) but are never rendered. Spec §4
explicitly says the open-leg card shows "the search parameters (origin,
destination, windows)". Without it the user cannot tell why the 17:30 they
expected is absent, cannot verify a typo ("did I say 18:00 or 08:00?"), and
has no "edit search" path — the list is the whole page. Show "Departing
KGX after 18:00 on Tue 22 Sept" and an "Edit search" affordance (which,
given §2.1's finding, currently has nowhere to link to).

**Important — the rows give nothing to choose between.** "18:00 · KGX →
EDB" ×4 differ only in the hour. `CandidateRow.destinationArrival` is in
the wire type (`JourneyLegCandidates.tsx:16`) and not rendered; there is no
operator, no duration, no calling-pattern hint. The spec's §2.3 said to
reuse `TrainSearchForm`'s result rendering ("same list UI as `/trains`
search results"); `JourneyLegCandidates` is a fresh minimal list instead,
and `/trains` rows show arrival and a "View live status" link this list
lacks. A passenger deciding between the 18:00 and the 19:00 wants "arr
22:20 · 4h 20m · LNER" and a way to peek at the train.

**Important — four buttons with the identical accessible name.** Screen
readers listing controls hear "Track this train" four times; the row text
is a sibling `<Text>`, not part of the button's name. `aria-label={`Track
the ${row.scheduled} to ${destination}`}` on each button, or wrapping each
row in a `role="group"` with `aria-label`, fixes it. (WCAG 2.4.6 /
2.5.3 adjacent — the visible label is fine; the *distinguishing* name is
missing.)

**Important — raw codes and an ISO date, on a page whose sibling list
already fixed both.** "KGX → EDB, 2026-09-22" (`JourneyLegCard.tsx:52`) is
the exact §2.9 / F3 pattern; `/track/mine` in the same session
(`track-mine-journeys.png`) renders "London Kings Cross (KGX) → York (YRK),
22 Sept 2026 · 17:00" using `routeLabel` and `formatDate`. Reuse them. The
candidate rows repeat the codes (`JourneyLegCandidates.tsx:117`).

**Minor — "Searching for a train to track" describes the wrong thing.**
The list is static; nothing is searching (auto-mode is deferred per §2.3).
The sentence primes the F13 problem the 09-02 review named — an unbounded
"we're working on it" with no progress — when the honest state is "we found
four; you choose". "4 trains match your search. Pick the one you'll be on —
we'll track it live from then on." also answers "what happens when I click".

**Minor — nothing says what happens on pick.** After "Track this train"
the card silently re-renders as a matched leg with a "Change train" button
(`JourneyLegCard.tsx:95-99`). The §2.3 decision (window persisted, swap
later) is a real reassurance — "you can change your mind" — and it is
invisible until after the irreversible-looking click. One clause in the
intro sentence covers it.

**Minor — "Add a leg" is offered before the first leg has a train, at
the same weight as the status badge.** The modal it opens
(`AddJourneyLegButton.tsx:159-245`, not screenshotted but one click from
this capture) uses bare `TextInput`s labelled "Origin CRS", "Destination
CRS", "Service date" (placeholder `YYYY-MM-DD`) and "Train UID" — the
station `Autocomplete`, `DatePickerInput` and `TimeFilterInput`s that
`TrackTrainForm` uses fifty lines away are not reused. This is the regression
the 09-17 review §3.6 called out on the ticket form ("Origin CRS code" as a
label) reappearing in a new component. Also its second mode asks for a
*train UID*, which no passenger knows. Recommend hiding "Add a leg" until
the current leg is matched (or at least demoting it), and rebuilding the
modal's fields from `TrackTrainForm`'s.

**Minor — the status badge's tooltip repeats its own text**
(`JourneyStatusBadge.tsx:27-31`), and a `Tooltip` on a non-focusable
`Badge` is unreachable by keyboard. Drop the tooltip or make it say
something the badge does not ("Pick a train below to start live tracking").

**Minor — generic `<h1>` with no rename.** "Tracked journey" is the
fallback for a null `customName` (`page.tsx:60`); the spec §4 rename pattern
is noted as deferred in the code comment. Until it lands, default the title
to the route ("London Kings Cross → Edinburgh, 22 Sept") as the 09-17
review recommended for the train page.

**Minor — login-modal copy.** In window mode the submit says "Search for a
train" but the 401 modal says "Log in to track this train."
(`TrackTrainForm.tsx:1195`).

### 2.6 `/track/mine` — the list the journey should appear in

Screenshots: `track-mine-authenticated.png`, `track-mine-journeys.png`.
These two files are byte-identical (`cmp` reports no difference), although
the manifest describes one as "tracked trains and tickets" and the other as
"the journeys list" — a small tell that the page has no journey concept.
Code: `.integration-worktree/frontend/app/track/mine/page.tsx` — unchanged
from `main` (`git diff main -- frontend/app/track/mine/page.tsx` is empty).

**What works.** The row titles now resolve both ends — "London Kings Cross
(KGX) → York (YRK), 22 Sept 2026 · 17:00" — which closes the "mixed label"
half of 09-17 §2.9. Status pills are not truncated at 1440px, and the kebab
menu the 09-17 review asked for (§3.6, "Stop tracking on the list") is
present.

**Critical (same root as §2.5) — open journeys cannot appear here.** The
list is built from `trains` and `shared` (`page.tsx:130-178`), i.e. train
subscriptions. A window-mode leg has `train_subscription_id IS NULL` until
picked, so it has no row to render. `getMyJourneys()` exists
(`.integration-worktree/frontend/lib/api.ts:660`) and is called from
nowhere. The result for the user: search a window, close the tab, and the
journey is not in "My Trains & Tickets", not in the nav, not in the home
dashboard. The fix is not cosmetic: either this page lists journeys (with
"Needs a train picked" rows linking to `/journeys/[id]`) or a `/journeys`
list page exists and is linked from the nav and from here.

**Important — two nouns for one object.** The create flow's heading is
"Track a Train", the destination page is "Tracked journey" at
`/journeys/169`, and the list is "My Trains & Tickets" with "Track a new
train". A matched journey then links from this list to `/train/...`
(`page.tsx:210-213`), not to the `/journeys/...` page the user was shown
after creating it — so the same object has two detail pages depending on
which door you enter by. Pick "journey" or "train" for the user-facing
noun; the spec's §7.2 backward-compatibility section is about the API, not
the vocabulary.

**Minor — pre-existing contradictions still present.** "Your reliability —
Track a train and check back once it's finished running" while both rows
are EN ROUTE (09-17 §3.6 nitpick). The "22M LATE" pill is yellow light
variant — yellow-6 text on a yellow tint — which is the same contrast
problem as §2.4 and worse (`#fab005` on white ≈ 1.9:1). Not new, but the
platform badge fix should be made in one place for both.

### 2.7 `/groups` empty state

Screenshot: `groups-empty-state.png` (1440×900). Code:
`frontend/app/groups/page.tsx:88-98` (unchanged from `main`).

`<h1>` "Groups", a grape "Create group" text link, and one dimmed line:
"You're not in any groups yet. Create one to share tracked trains with
other people."

**Does it guide the user?** Partly. It names the action (create) and the
value (share tracked trains), and it does not fabricate a sample group. That
is more than "nothing here". It stops short in four ways:

**Minor — two links for the same action, 40px apart, in two styles.**
"Create group" (grape `TextLink`, header) and "Create one" (plain Next
`<Link>`, black underline, inline) both go to `/groups/new`. In an empty
state there is one thing to do; make it one filled `Button` in the body and
drop the header link when the list is empty (the header link earns its
place once there are rows to scroll past).

**Minor — the header CTA floats in the middle of the screen.** `maw={640}`
on the `Stack` (a 09-17 §3.2 recommendation, applied) plus
`justify="space-between"` puts "Create group" at x≈720 on a 1440px viewport
— aligned to nothing visible. Left-aligning the header actions under the
title, or giving the empty state its own centred block, avoids the orphan.

**Minor — no path for the more common case: being invited.** Most people
join a group someone else made. "…or ask a group member for their invite
link" is one clause and pre-empts the "how do I join my family's group"
question. The 09-17 review's §3.2 invite-link findings make the same
observation from the other side.

**Minor — "tracked trains" undersells what groups share now.** The page's
own metadata (`page.tsx:44-45`) says "tracked trains and custom lines", and
this branch adds `ShareJourneyButton`. The empty state is where a new user
learns what a group is for; say "share journeys and custom lines".

**Minor — the only guidance on the page is dimmed.** `c="dimmed"` on the
one sentence that tells the user what to do. Empty-state copy is content,
not metadata; body colour.

---

## 3. What works well (keep these)

- The mode switch uses one idiom, a `SegmentedControl` with a real
  `radiogroup`, rather than the mixed pill-button/segmented pattern the
  09-17 review flagged (§2.13). Selected state is unmistakable at every
  width and the Origin field is correctly shared above it.
- The submit label tracks the mode ("Search for a train" / "Track this
  train") and the error `Alert` title does too ("Couldn't search for a
  train" / "Couldn't track this train", `TrackTrainForm.tsx:1169`).
- Window-mode fields have per-field descriptions, and the first one names
  the entered origin dynamically. Half-entered times are caught with a
  message rather than dropped.
- `PlatformBadge` gets WCAG 1.4.1 right: the text names both platforms, the
  colour is secondary, and a test hook asserts state without relying on
  colour. Rendering nothing for an unknown platform ("never fabricate") is
  the right call.
- `ScheduleRow` builds on `StatusRow`, so the badge shrink guard the 09-17
  review asked to be centralised (§2.5) is inherited, not copied.
- Candidate rows use real buttons with `loading`/disabled-siblings/error
  states; the zero-result state offers a way out.
- `/track/mine` row titles now name both stations with codes in
  parentheses — the display-label layer the 09-17 review asked for is
  visibly landing.
- The journey page's 401 branch uses the "log in, this might be yours"
  posture rather than a 404.
- The groups empty state is honest and names its CTA; no fake data.
- The pick-mode form fits in one 390×844 viewport with its submit visible.

---

## 4. Recommendations (concrete, in priority order)

1. **Make open journeys reachable** (Critical, §2.5/§2.6). Call
   `getMyJourneys()` from `/track/mine` and render unmatched journeys as
   rows with the "Needs a train picked" badge linking to `/journeys/[id]`;
   or add `/journeys` and link it from `navLinks.ts` and from `/track/mine`.
   Add a "← My journeys" link above the journey `<h1>`.
2. **Label the mode toggle and make it linkable** (Important, §2.1). Add a
   visible group label ("How do you want to find the train?") wired as the
   `SegmentedControl`'s `aria-label`/`aria-labelledby`; relabel the options
   "I know which train" / "I'm not sure yet — search by time"; support
   `?mode=window` in `track/page.tsx` alongside `?origin=`; rewrite the
   page intro to mention both ("…pick it from the departure board, or tell
   us roughly when you're travelling and choose from the matches"); use the
   same two labels in `AddJourneyLegButton`.
3. **Fix the "(optional)" contradiction** (Important, §2.2). Either a
   group-level line "Give at least one of these times" under the grid with
   "(optional)" removed from the labels, or make the first field required
   by default. Consider collapsing to one selector + one time input with
   "add another bound".
4. **Show the window and enrich the candidates** (Important, §2.5). Render
   the persisted `departAfter`/… on the open-leg card in words; add arrival
   time, duration and operator to each candidate row (reuse `/trains`'
   result row as the spec intended); add an "Edit search" affordance; give
   each "Track this train" button a distinguishing `aria-label`; replace
   "Searching for a train to track" with "N trains match — pick the one
   you'll be on; you can change it later"; use `routeLabel`/`formatDate`
   for the header.
5. **Fix badge contrast at the token level** (Important, §2.4/§2.6). Measure
   `PlatformBadge`'s light-orange and `TrackedTrainStatusBadge`'s
   light-yellow with axe; move both to a text colour ≥4.5:1 (`orange.8`,
   `yellow.9`, or outline variants). Give the platform badge a non-severity
   colour so orange means "late" only.
6. **Verify the 24h time input** (Important, §2.2). Confirm whether
   `lang="en-GB"` takes effect in Chromium; the captures say it does not.
   Fall back to the text-input approach if not.
7. **Bring `AddJourneyLegButton`'s modal up to the form's standard**
   (Minor now, Important once "Add a leg" is prominent, §2.5). Reuse the
   station `Autocomplete`, `DatePickerInput`, and the same labels; hide or
   demote "Add a leg" until the current leg is matched; reconsider asking
   for a train UID at all.
8. **Unify the noun** (Important, §2.6). Decide "journey" or "train" for
   user-facing copy; rename "My Trains & Tickets" / "Track a new train"
   accordingly, and route list rows to the journey page.
9. **Mobile layout of the window grid** (Minor, §2.3). One column below
   `sm`; `wrap="nowrap"` + `flexShrink: 0` for the "Now" button; 44px clock
   targets; `maw={640}` on the form at all widths.
10. **Date default as a value, not a placeholder** (Minor, §2.2). Seed
    `windowServiceDate` with today or say "Defaults to today".
11. **Groups empty state** (Minor, §2.7). One filled "Create a group"
    button in the body; mention invite links; say "journeys and custom
    lines"; body colour, not dimmed; keep the header link only when rows
    exist.
12. **Small copy fixes** (Minor). Login modal in window mode; the
    reliability card's "check back once it's finished running" while trains
    are en route; the status badge's self-repeating tooltip; three
    phrasings of the same time-field description.

---

## 5. Coverage gaps worth a follow-up capture

- Window mode, mobile, with the keyboard open and a validation error shown.
- The departure picker with a populated (10-row) board at 390px, to check
  badge/title space contest.
- Dark mode for every screen here — none of the eleven captures is dark,
  and both contrast findings (§2.4, §2.6) will be different there.
- The `/journeys/[id]` page after a pick (matched leg + "Change train"),
  and with the "Add a leg" modal open.
- The `TrackDestinationModal` (personal vs group) prompt in window mode —
  `submitWindow` accepts a `groupId` it then discards
  (`TrackTrainForm.tsx:653`), so a user in a group is asked a question
  whose answer is ignored.
