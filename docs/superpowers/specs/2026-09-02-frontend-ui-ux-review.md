# Frontend UI/UX Review — Screenshot Audit

**Status: review only, not a fix plan and not an audit re-run.** Written to
the same rigor as
`docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md`
(structural template: Goal, Method, findings with verified citations, explicit
out-of-scope and open-questions sections). No code was changed to produce
this document. Every claim below cites the specific screenshot file it was
observed in; nothing is asserted from captions or memory alone.

## Goal

Review the actual rendered UI of the live Distant Signal deployment
(`https://konata.fox-prometheus.ts.net`, captured 2026-09-02 as 73
screenshots) against two yardsticks at once:

1. **The design specs the pages were built from** (`docs/superpowers/specs/*-design.md`)
   — did the implementation land what the spec intended, and where it
   didn't, is the deviation purposeful or a miss?
2. **Fresh critical judgment** — is the UI actually good, regardless of what
   any spec says?

**The specs are not binding requirements, and this review does not treat
them as such.** A spec records a decision made at one point in time with the
information available then. Several findings below explicitly recommend
going *beyond* or *against* what a spec decided — the station-search
ranking (the autocomplete spec's "plain substring is enough" non-goal is
demonstrably not enough on the real data, §F1), the add-ticket page width
(a deliberate spec decision that produced a worse-looking page than the
convention it declined to copy, §D3), and the logged-in-empty home page
(the anonymous-UX spec called it "arguably fine"; on the real rendered
pages it is clearly worse than fine, §F2). Where the rendered UI matches a
spec *and* the result is good, that's noted as such; where it matches a
spec and the result is still weak, this review says so rather than hiding
behind conformance. The reverse also holds: several things the specs
required were verified as correctly delivered and are credited explicitly
(§C), because a review that only lists problems misrepresents an app that
is, overall, in decent visual shape.

## Method

- **Source material:** all 73 PNGs under the audit scratchpad's
  `screenshots-2026-09-02/` directory, plus its `INDEX.md` manifest. Every
  file was opened and looked at as an image (very tall full-page captures
  were additionally cropped and re-read at higher zoom with a small
  Pillow script; the nav icon cluster was inspected at 4× zoom).
- **Specs read against the screenshots** (the ones that actually govern
  what was captured — not every spec in the repo):
  `2026-08-18-grape-theme-design.md`, `2026-07-12-dark-theme-design.md`,
  `2026-08-31-anonymous-user-ux-design.md`,
  `2026-09-02-modal-login-prompt-design.md`,
  `2026-09-02-custom-line-creation-page-design.md`,
  `2026-07-11-operator-station-autocomplete-design.md`,
  `2026-08-31-line-history-graphics-design.md`,
  `2026-09-02-line-history-chart-fixes-design.md`,
  `2026-09-02-trend-chart-granularity-design.md`,
  `2026-08-31-incident-detail-page-design.md`,
  `2026-08-31-tracked-trains-list-design.md`,
  `2026-09-01-tracked-trains-home-page-design.md`,
  `2026-09-02-standalone-ticket-entry-page-design.md`, and (for
  cross-reference only) `2026-09-02-frontend-accessibility-audit-research.md`
  and `2026-09-02-line-history-list-spamminess-research.md`.
- **Capture-integrity caveats found during review** (verified by md5, not
  guessed — the capture session was fighting the auto-navigation bug and
  it shows). These matter because anyone re-using this screenshot set
  should not trust these filenames:
  - `home-loggedin-populated-desktop.png` is **byte-identical** to
    `home-loggedout-desktop.png` (both show the logged-out home). The real
    populated dashboard evidence is `home-loggedin-populated-tablet.png`,
    plus two *mislabeled* files that actually show it:
    `line-history-empty-desktop.png` (really the populated home, desktop)
    and `connect-claude-error-mobile.png` (really the populated home,
    mobile).
  - `track-form-desktop.png` and `track-form-mobile.png` actually show the
    `/chat` "not allowed" page (desktop and mobile respectively;
    `track-form-mobile.png` is byte-identical to both
    `chat-loggedin-notallowed-*.png` files, which are themselves identical
    390px captures). The Track-a-Train form itself is only evidenced at
    mobile width, via `track-datepicker-desktop.png` and
    `track-form-error-desktop.png` (both actually 390px wide).
  - `train-by-id-mobile.png` is byte-identical to
    `line-detail-custom-mobile.png`; `station-detail-york-mobile.png`
    actually shows the add-ticket form; `track-mine-with-ticket-desktop.png`
    is byte-identical to its mobile counterpart.
  - Net effect: **no desktop capture exists** of the Track-a-Train form or
    the with-ticket `/track/mine` page (tablet stands in), **no mobile
    capture exists** of the station detail or `/train/by-id` pages, and
    **no capture at all exists** of dark mode, the Trends tab at mobile
    width, or the matched-train journey page. See Open questions.
  - One rendering caveat: in full-page captures the modal overlay only
    dims the viewport-sized region (`position: fixed`), so the undimmed
    table beneath `login-prompt-modal-desktop.png` is a capture artifact,
    not an overlay bug — deliberately **not** flagged as a finding.
- The three functional bugs the same audit found (auto-navigation hijack,
  `/connect-claude` authenticated 500, stations-lookup wrong navigation)
  are **not re-investigated** here — see Explicitly out of scope.

Findings are organized **by theme, ranked roughly by product impact**,
rather than page-by-page: most of what's wrong here is a pattern repeated
across several pages (raw codes, two-step selects, leaked internal
strings), and a per-page organization would state each one three times.
Page-specific one-offs are gathered in §F14. Verdict vocabulary:
**real issue** (worth fixing), **purposeful deviation** (implementation or
spec chose differently on purpose, and that's fine — or at least
documented), **matches intent** (credited in §C).

## Findings — real issues worth fixing

### F1. Station search ranking: typing "York" does not surface York

**Seen:** `stations-autocomplete-desktop.png` — the `/stations` combobox
with "York" fully typed shows, as its top four suggestions: "BYK — Bentley
(South Yorkshire)", "BLE — Bramley (West Yorkshire)", "CLN — Chapeltown
(South Yorkshire)", "CPY — Clapham (North Yorkshire)". York itself (YRK) —
the exact-name match — is not visible at all; every visible hit matches
only on the "(…Yorkshire)" county suffix. The INDEX also notes (not
separately screenshotted) that a single character shows an unfiltered A–Z
list.

**Design intent:** `2026-07-11-operator-station-autocomplete-design.md`,
Non-goals: "No fuzzy/typo-tolerant matching (no `pg_trgm`) — plain
substring `ILIKE` on code or name is enough for a list this size."

**Verdict: real issue — the spec's judgment call is contradicted by its own
feature's rendered output.** No fuzziness is needed, but *ranking* is:
plain unordered `ILIKE` makes the single most likely query in the UK rail
network ("York", also a substring of ~40 Yorkshire station names) bury its
own answer. Recommended direction: keep the substring match, add a
three-tier `ORDER BY` — exact CRS/code match first, name-prefix match
second, everything else after, alphabetical within tiers. This is a
one-query change, not a search engine. The same ordering should apply to
the CRS combobox on `/lines/new` (same backend), where
`lines-new-autocomplete-desktop.png` happens to look fine only because
"KGX" is a code query.

### F2. Logging in makes the home page *worse* until you pin something

**Seen:** `home-loggedout-desktop.png` / `home-loggedout-mobile.png` — an
anonymous visitor gets a genuinely useful page: tagline, "Right now" module
("89 lines not at Good Service right now:") with the five worst lines as
clickable status cards, and a CTA row. `home-loggedin-desktop.png` /
`home-loggedin-mobile.png` — the same visitor, freshly logged in with no
pins yet, gets: a floating "Enable notifications" button, two empty
sections ("You haven't pinned any lines yet…", "You haven't pinned any
stations yet…"), and nothing else. The live-status content they were
looking at seconds earlier is gone.

**Design intent:** `2026-08-31-anonymous-user-ux-design.md` §Home page
redesign — the "Right now" module was designed for the *anonymous* branch,
and the spec explicitly said the logged-in zero-pin case is "arguably fine
for them specifically". The implementation matches the spec exactly.

**Verdict: real issue, spec conformance notwithstanding.** The spec's
"arguably fine" was a guess; the rendered pair of pages settles the
argument the other way. Logging in is the single action the app most wants
a visitor to take, and its immediate reward is a blank page — the exact
"useless home page" problem that spec set out to fix, reintroduced for the
first session after signup. Recommended direction: render the "Right now"
module for logged-in users whenever they have zero pinned lines (the data
is already fetched on this page unconditionally — the spec itself
documents that `allReports` is pulled on every load), keeping the empty
"Your Lines/Stations" prompts above it as one-liners.

### F3. Raw CRS codes as primary user-facing labels

**Seen:** `home-loggedin-populated-tablet.png` — "Your Tracked Trains"
rows read, in their entirety, "WAT — 2 Sept 2026 · 14:00" and "KGX — 2 Sept
2026 · 16:53". `track-mine-populated-desktop.png` /
`track-mine-populated-mobile.png` — same bare "KGX" card on `/track/mine`.
`lines-new-filled-desktop.png` / `-mobile.png` and `line-edit-*.png` —
station chips are "YRK ×" "KGX ×". `train-by-id-desktop.png` — "KGX · 2
Sept 2026".

**Design intent:** `2026-09-01-tracked-trains-home-page-design.md`
Decision 1 specifies "route (`origin → destination` or bare origin)" — so
bare-origin *codes* are technically within intent, and pre-match trains
genuinely have no destination yet. No spec anywhere *requires* codes-only
display; meanwhile the autocomplete spec exists precisely because "users
don't need to already know ATOC/CRS codes by heart".

**Verdict: real issue (consistency-of-principle, not spec violation).**
The app spent a whole spec making sure users never have to *type* codes,
then displays codes back to them as the only label on the dashboard's most
personal section. "London Kings Cross" vs "KGX" is the difference between
a glanceable dashboard and a lookup exercise for anyone who doesn't
commute through that station daily. Station names are already in the
frontend's reach (the same reference data the autocomplete queries).
Recommended direction: show "London Kings Cross (KGX)" or name-only in
tracked-train rows and ticket route lines ("PAD → RDG" in
`track-mine-with-ticket-tablet.png` has the same problem); keep bare codes
in the compact chips on the line form if space demands, but add
`title`/tooltip names there.

### F4. Mobile users cannot pin from the All Lines table at all

**Seen:** `lines-loggedout-mobile.png`, `lines-loggedin-pinned-mobile.png`
— at 390px the table folds Avg Delay/Cancelled into a sub-line under the
name (good, see §C4) but the **Pin column is dropped entirely**; there is
no star anywhere in a full-page capture of ~130 rows, including on the
row (CrossCountry) that is actually pinned. At 768px
(`lines-loggedin-pinned-tablet.png`) and 1280px
(`lines-loggedin-pinned-desktop.png`) the Pin column is present, with a
clearly-differentiated filled-yellow star and an "Unpin (currently
pinned)" tooltip.

**Design intent:** no spec governs the responsive column set;
`2026-08-31-anonymous-user-ux-design.md` treats pinning as a first-class
Tier-2 action and the modal-login-prompt spec routes its logged-out case
through this exact control.

**Verdict: real issue.** Pinning is the app's core personalization loop
(the home page is built from it), and on phones — where a glanceable
pinned dashboard is *most* valuable — the primary pinning surface has no
entry point; a mobile user's only route to a pinned line is via each
station page's pin button. Also, a pinned row is visually identical to an
unpinned one at mobile width, so existing pins are invisible. Recommended
direction: keep the star at all widths (it needs ~44px; the folded layout
has the room), or failing that, add pin to a row-tap action sheet.

### F5. Internal strings leak into user-facing copy on error/metadata paths

**Seen:**
- `track-form-error-desktop.png` — inline alert: "Couldn't track this
  train — **scheduled_departure** is too far in the past to track" (raw
  snake_case field name, verbatim backend text).
- `BUG-connect-claude-error-desktop.png` — the shared error page renders
  "Minified React error #130; visit https://react.dev/errors/130?args[]=…"
  as its body copy. (The crash itself is out of scope; the *error page
  design* — piping `error.message` straight to users — is not.)
- `line-detail-crosscountry-expanded-incident-desktop.png` — expanded
  incident footer: "Source: knowledgebase-incident-EC354602568440DB82B2835903B7A5FE".

**Design intent:** `2026-08-31-anonymous-user-ux-design.md`'s Tier-2
contract explicitly demands "never a generic error string" for auth
failures, and the codebase fixed exactly this class of leak once already
(the `"no session"` raw-body finding, §Correction 3). No spec blesses any
of these three leaks.

**Verdict: real issue — same defect class, three more instances.** The
validation message should be written for humans ("That departure time is
more than 6 hours ago — trains can only be tracked within 6 hours of
departure"); `app/error.tsx` should show a fixed friendly sentence and log
the real error instead of rendering it; the source ID either belongs
behind the ⓘ affordance or formatted as a short reference, not a 32-hex
dump in body text.

### F6. The "pick a suggestion, then press a second button" pattern

**Seen:** three independent implementations of the same two-step commit:
- `lines-new-autocomplete-desktop.png` — selecting "KGX — London Kings
  Cross" only fills the text box; the station is not added until the
  separate "Add" button is clicked (and the open dropdown visually
  overlaps the "Show advanced options" control mid-flow).
- `stations-search-selected-desktop.png` — selecting York fills "YRK",
  then a separate "Look up" click is required to navigate.
- `track-mine-with-ticket-tablet.png` — "Attach to one of your tracked
  trains": pick from select, then click "Attach".

**Design intent:** no spec mandates two-step commit; the autocomplete spec
only chose the widget, not the interaction contract.

**Verdict: real issue (friction + learnability), with nuance per site.**
For the station lookup, choosing a station from the dropdown *is* the
whole intent — navigating on select (keeping "Look up" for typed-code
entry) would remove a step from the app's most common flow. For the
line-form station adder, auto-adding on suggestion-select is the expected
Mantine-tags behavior users will guess first; the current design invites
"typed it, selected it, hit Create, station silently missing". The Attach
select is the most defensible (attaching is consequential), but its
disabled-until-selected button gives no hint why it's disabled. At minimum
each should commit on select; if the two-step stays anywhere, the second
button needs to visibly activate with an affordance change stronger than
enabled/disabled gray.

### F7. Incident body copy renders default-blue links inside a grape app

**Seen:** `line-detail-crosscountry-expanded-incident-desktop.png`,
`incident-detail-desktop.png` / `-mobile.png`, and (best example, a
link-dense real incident) `BUG-auto-navigation-hijack-desktop.png` —
"Engineering work", "Journey Planner", "First Bus", "Stagecoach Buses",
"compensation" etc. inside knowledgebase-sourced incident text are
default browser blue with underline, while every app-chrome link on the
same pages ("View history", "View full incident details", "CrossCountry"
under Currently affects) is theme grape.

**Design intent:** `2026-08-18-grape-theme-design.md`'s entire premise is
that blue stopped being the link color precisely so blue could keep
meaning "planned" in badges — "blue would mean 'planned closure' *and*
'this is a link' in the same viewport" was the collision it set out to kill.
The sanitized incident HTML's anchors were simply never covered: the spec's
call-site inventory only tracked `c="blue"` props, and these anchors come
from external HTML, not Mantine props.

**Verdict: real issue — an honest gap in an otherwise completed spec, not
a purposeful deviation.** On `incident-detail-desktop.png` a blue "PLANNED
WORK" badge sits a few hundred pixels from blue body links: the exact
juxtaposition the grape spec was written to eliminate. Recommended
direction: a scoped CSS rule on the sanitized-content container
(`a { color: var(--mantine-color-anchor) }`), which also picks up the
AA-fixed grape-7 anchor variable for free.

### F8. `/chat` "not allowed" is a dead end with no explanation

**Seen:** `chat-loggedin-notallowed-desktop.png` (identical bytes at
mobile) — the entire authenticated-but-not-allowlisted page is a "Chat"
heading and one dimmed line: "Not available for your account yet." No
what, no why, no who-to-ask, no link anywhere.

**Design intent:** the embedded-chatbot specs
(`2026-09-02-embedded-chatbot-dual-mode-design.md` and companions) design
the auth/token plumbing, not this state's presentation — no governing
spec; general UX judgment.

**Verdict: real issue (small, cheap, user-facing).** A gated feature's
"you're not in" page is the *only* thing a non-allowlisted user will ever
see of the feature; one more sentence ("Chat is being rolled out
gradually — ask the instance admin for access") and a link back to the
dashboard would make it feel intentional rather than half-built. The
logged-out chat modal (`chat-loggedout-mobile.png`), by contrast, has
well-tailored copy ("Sign in to ask about live departures…") — the two
states were clearly not given the same care.

### F9. The departure picker happily selects times the form will reject

**Seen:** `track-datepicker-desktop.png` — the Mantine date/time picker
open over the Track-a-Train form, all past dates in September selectable
(nothing disabled before "today"); `track-form-error-desktop.png` — the
inevitable result: pick 08:00 that morning, submit, get the F5 error
("more than 6 hours in the past"). The form's own helper text states the
rule ("Must be within the last 6 hours, or any time in the future").

**Design intent:** `2026-08-28-train-tracking-design.md` /
`2026-08-29-train-tracking-frontend-design.md` define the 6-hour window as
a backend constraint; nothing specifies picker bounds. General UX
judgment.

**Verdict: real issue.** The constraint is known, stated in the UI, and
enforced — everywhere except the control that selects the value. Setting
`minDate`/disabled-hours to the same 6-hour rule turns a submit-fail loop
into prevention. Keep the server-side check as backstop (clock skew), but
the picker shouldn't offer times the page itself documents as invalid.

### F10. Advanced line-form fields are undocumented jargon

**Seen:** `lines-new-filled-desktop.png` / `-mobile.png` — expanding
advanced options reveals "Headcode prefixes" (placeholder "e.g. 1P") and
"Destination CRS filter" (placeholder "e.g. AON"), with no helper text at
all; the basic form's "Operators" placeholder is "e.g. SW".

**Design intent:** `2026-09-02-custom-line-creation-page-design.md` moved
the form to its own route (correctly implemented — see §C6) but did not
add field documentation; the autocomplete spec deliberately left headcodes
as free text. No spec addresses explaining these concepts.

**Verdict: real issue for anyone who isn't a rail-ops enthusiast.**
Headcodes and CRS destination filters are power-user features (fine), but
a one-line description under each ("Filter to trains whose ID starts with
these prefixes, e.g. 1P for certain express services") costs nothing and
is the difference between discoverable power and a field people are
scared to touch. The Operators field should also get autocomplete's TOC
names surfaced — "e.g. SW" assumes the user knows TOC codes, which is the
assumption the autocomplete spec was written to remove.

### F11. Incident rows drown in badge soup, and station-page rows don't say which line they belong to

**Seen:** `station-detail-york-desktop.png` / `-tablet.png` — each incident
row carries up to four pills plus a chevron: severity ("MINOR DELAYS"),
"RAIL REPLACEMENT BUS", "2 LINES", "PLANNED", plus "Now". Nine rows of
this reads as a wall of orange capsules; and none of the rows says *which*
of York's four listed lines it affects — "2 LINES" states a count without
naming them, and several rows (Basingstoke–Winchester, Cleethorpes–Grimsby)
visibly have nothing to do with York except sharing an operator.
`line-detail-crosscountry-desktop.png` has the same density pattern one
notch milder.

**Design intent:** the provenance/severity badge system comes from the
incident pipeline specs (`2026-09-01-disruption-impact-type-design.md`,
stale-incident handling etc.), which define *what* metadata exists, not
how much of it each list row must wear. General UX judgment.

**Verdict: real issue (hierarchy, not correctness).** Everything shown is
true; not everything true needs to be a pill on the collapsed row. The
severity badge and title earn their place; source provenance
("KNOWLEDGEBASE", "RAIL REPLACEMENT BUS" — which duplicates the severity
badge's own "RAIL REPLACEMENT" on some rows) and "PLANNED" could live
inside the expanded view, where `line-detail-crosscountry-expanded-incident-desktop.png`
already repeats them anyway. On station pages, replace "2 LINES" with the
actual line names (or add a per-row "affects: TransPennine Express" line)
— that's the one fact a station-page reader is actually triaging by.

### F12. The unattached-ticket card buries its action under legal copy, and seats Delete beside it

**Seen:** `track-mine-with-ticket-tablet.png` (widest available capture;
mobile identical in structure) — card order: route/type header, two full
paragraphs of disclaimer ("No delay data recorded yet…", "This is a rough,
community-sourced estimate… This app never submits a claim on your behalf
-- verify eligibility…"), an external claim link, and only then the actual
action ("Attach to one of your tracked trains" + Attach), with "Track a
new train for this ticket" and a red-outline **Delete** all on the same
control row at tablet width.

**Design intent:** `2026-08-31-tickets-list-design.md` /
`2026-08-29-journey-ticket-tracking-frontend-design.md` require the
Delay-Repay disclaimer (rightly — it's a legal-adjacent estimate). Its
*placement priority* is unspecified.

**Verdict: real issue (weight and grouping), minor severity.** The
disclaimer belongs on the card but not *above* the primary action while
the ticket has no delay data at all — at that point it disclaims an
estimate that isn't being shown. Show it collapsed/smaller until an
estimate exists. And the destructive Delete should not sit at equal
visual weight in the same row as the constructive actions; push it to the
card corner or behind the row's overflow. (Also home of the recurring
`--` double-hyphen typography — see F14.)

### F13. "Pending match" gives no sense of time or progress

**Seen:** `train-by-id-desktop.png` — spinner, "Waiting to hear from
Network Rail", "This train hasn't been matched to a live service yet. This
page updates automatically." Both trains tracked in the session stayed in
this state 10+ minutes (INDEX); whether that latency is normal is a
backend question, out of scope. The badge "PENDING MATCH" also appears
with no explanation on `track-mine-populated-*.png` and the home
dashboard.

**Design intent:** the train-tracking frontend spec defines the pending
state's existence, not its reassurance copy. General UX judgment.

**Verdict: real issue (expectation-setting).** The state is honest but
unbounded — nothing tells the user whether "yet" means 30 seconds or an
hour, so a long wait is indistinguishable from breakage. Add "tracking
since 16:53" and one sentence of expectation ("matching usually completes
within a few minutes of departure"), and make the heading say something
about the train ("Tracking: KGX departure, 2 Sept") instead of the
internal counter "Tracking Train 1".

### F14. Assorted polish (each small; listed once, several recur on multiple pages)

- **Cryptic nav icon:** the icon-button cluster (zoomed to 4× during
  review) is ⓘ, a sun-with-"A" (color-scheme auto toggle — novel but
  guessable), and a rounded square containing two vertical bars that does
  not read as anything (pause? columns?) — `home-loggedout-desktop.png`
  top-right, present on every page. Whatever it is, it needs a tooltip;
  if it's the PWA/install or data-freshness affordance, an icon with an
  established shape.
- **"Right now" list doesn't say what it's showing:**
  `home-loggedout-desktop.png` says "89 lines not at Good Service right
  now:" and then lists five with no "worst five — see all 89" framing;
  the reader must infer the list is a sample, and the three-link footer
  is the only route to the rest.
- **"Enable notifications" is the anonymous page's only button:** it
  out-weighs everything on `home-loggedout-mobile.png`, sits *above* the
  tagline that explains the app, and (per
  `2026-09-02-line-status-notifications-design.md`) triggers a
  browser-permission flow — an aggressive first ask for a visitor who
  hasn't been told what the product is yet. Demote below the fold or to a
  quieter variant for logged-out users.
- **Redundant status text:** pinned-card "GOOD SERVICE" badge with
  literal "Good Service" text directly beneath
  (`home-loggedin-populated-tablet.png`, `line-history-empty-desktop.png`
  [mislabeled populated home]).
- **Dangling empty label:** "Operators:" with no value on the custom
  line's detail page (`line-detail-custom-desktop.png` / `-mobile.png`)
  — hide the row when empty.
- **"Connecting…" heading over a failure message:**
  `chat-callback-desktop.png` / `-mobile.png` pairs an in-progress
  heading with "No authorization code was present in the callback URL."
  Swap the heading to "Connection failed" when the error branch renders.
- **Duplicated metadata:** incident History section shows "1 Sept 2026,
  13:23 / First seen" and then, immediately below the divider, "First
  seen: 1 Sept 2026, 13:23 / Last fetched: …"
  (`incident-detail-desktop.png`).
- **Chart-axis dates are ISO while the whole app writes "2 Sept 2026":**
  "2026-09-01" on the Trends x-axis
  (`BUG-stations-lookup-wrong-navigation-desktop.png`, which despite its
  name is the only Trends-tab capture).
- **"(last 24 hours)" heading over a ~4.5-hour axis:**
  `line-detail-crosscountry-desktop.png` — the window is 24h but only
  sampled hours plot, so the heading over-promises; either annotate the
  short span or extend the axis with the gap shading the chart-fixes spec
  already built.
- **Bezier smoothing overstates sparse data:** with ~8 half-hourly points
  (`line-detail-crosscountry-desktop.png`) the smoothed curves imply
  continuous measurements and visibly overshoot between points; with 2
  daily points (`BUG-stations-lookup…​.png`) it's a bare line. The
  explainer paragraph is careful about honesty; `curveType="linear"`
  would match its spirit better. (Beyond any spec — the chart-fixes spec
  fixed tooltips/legend/gaps, not interpolation.)
- **`--` as a dash** in user-facing prose, twice: the trends explainer
  ("that half hour -- not a share of poll cycles") and the ticket
  disclaimer ("on your behalf -- verify eligibility") — everywhere else
  the app correctly uses "—".
- **Add-ticket tab strip wraps** to two rows at 390px, with "Upload PDF
  e-ticket" alone on the second line (`add-ticket-filled-mobile.png`,
  `station-detail-york-mobile.png` [mislabeled]) — legible but scruffy;
  shorter labels ("Manual", ".pkpass", "PDF") would fit one row.
- **"Ticket saved" banner weights its two actions oddly**
  (`add-ticket-saved-mobile.png`): the situational link ("Find or track
  the train this ticket is for") is underlined and prominent while the
  common path ("Done for now") is a small text button — reasonable to
  nudge attach-now, but "Done for now" is barely recognizable as a
  control.
- **Desktop line-detail places the status badge at the far page edge**,
  a full content-width away from the line name it describes
  (`line-detail-crosscountry-desktop.png`); the tablet/mobile treatment
  (badge directly under/next to the h1,
  `line-detail-crosscountry-tablet.png`) associates better and could
  simply be kept at desktop too.
- **Huge h1 for long incident titles on mobile:**
  `incident-detail-mobile.png` — a 20-word incident title at full h1 scale
  consumes ~40% of the first viewport; clamp the title size on this route.
- **Header date-preset buttons ("Last 7 days" filled / "Last 30 days"
  light)** read as primary/secondary actions rather than an exclusive
  selected/unselected pair (`line-history-crosscountry-desktop.png`);
  a segmented control (like the All/Active/Upcoming group below it, which
  reads perfectly) would say "one of these is active" unambiguously.

## Findings — purposeful deviations and spec-conformant calls (fine as-is, or noted with a dissent)

### D1. Login-prompt modal matches its spec exactly

`login-prompt-modal-desktop.png` / `-mobile.png`, `chat-loggedout-mobile.png`,
`track-mine-loggedout-modal-desktop.png` / `-mobile.png` all show the
`2026-09-02-modal-login-prompt-design.md` design as decided: fixed "Log in
required" title, per-site body copy, single primary "Log in" button, close
X, no cancel button. Sizing and margins are comfortable at both widths.
**Matches intent.** One dissent, mirroring the spec's own admission: after
dismissing the auto-opened modal, `/track/mine` is a heading floating over
an empty page (visible behind the modal in
`track-mine-loggedout-modal-desktop.png`). The spec accepted that as "no
worse than today"; a single inline "Log in to see your trains and tickets"
line under the heading would make the dismissed state self-explanatory for
one sentence of cost.

### D2. Chart fixes landed as designed

Legend on the rate chart with per-series dash patterns (solid blue delay,
dashed red cancellation, dotted yellow skip), shaded gap bands for
low-coverage spans, bounded empty-state box, no right-edge dot clipping —
all visible across `line-detail-crosscountry-desktop.png` /
`-tablet.png` and `line-history-empty-mobile.png`, exactly per
`2026-09-02-line-history-chart-fixes-design.md`. The daily-buckets-on-
history / hourly-on-line-page split matches
`2026-09-02-trend-chart-granularity-design.md` (the two explainer
paragraphs differ per-page as designed). **Matches intent** — remaining
chart quibbles are the new ones in F14, not regressions of these specs.

### D3. Add-ticket page width: purposeful deviation, but I'd reverse it

`add-ticket-blank-desktop.png` / `add-ticket-filled-desktop.png` — four
text inputs stretched to ~1,100px wide. This is exactly what
`2026-09-02-standalone-ticket-entry-page-design.md` Decision 6 chose
(unconstrained `Stack`, deliberately *not* copying the custom-line form's
`Center`+`maw={480}`), so it is not an implementation miss. But the
rendered result argues against the decision: a 1,100px input for a
three-letter CRS code looks unfinished next to the tidy 480px column of
`lines-new-blank-loggedout-desktop.png` and `line-edit-desktop.png`, and
the app now has two competing desktop form layouts for no visible reason.
The spec's rationale ("TicketEntryForm has no internal width to line up
with") describes an accident of the component, not a design goal.
**Recommend:** give `TicketEntryForm` the same `maw` treatment and
converge on the narrow centered form as the app-wide convention (the
Track-a-Train form should follow; only mobile captures of it exist, where
the question is moot).

### D4. Custom line creation on its own route, edit-form parity

`lines-new-*.png` vs `line-edit-*.png`: same fields, same order, same
chips, same button placement, differing only in heading and submit label —
the reuse `2026-09-02-custom-line-creation-page-design.md` intended, with
no edit-only regressions spotted. The "New custom line" entry link sits
beside the All Lines heading per Decision 2. **Matches intent.** (The
`/lines/new` page renders for anonymous visitors with gating at submit —
Tier 2 per the anonymous-UX policy; the submit-time modal behavior wasn't
capturable this session, see Open questions.)

### D5. Anonymous home, gating tiers, and destructive-action colors

The anonymous "Right now" home is the anonymous-UX spec's design, working
(F2 is about the *logged-in* branch). Tier-1 reads are ungated everywhere
checked; `/track/mine` gates via modal per the reclassification in the
modal spec; Edit/Delete appear on the owned custom line
(`line-detail-custom-desktop.png`) with Delete in red, matching the grape
spec's "destructive stays red" rule; status badges kept their semantic
colors (grape appears nowhere it shouldn't in any capture). The
data-quality "PLANNED" pill renders gray-outline beside colored severity
badges as the grape spec's follow-up fix intended. **Matches intent.**

### D6. Tracked-trains dashboard section: placement and cap per spec

`home-loggedin-populated-tablet.png`: "Your Tracked Trains" is the third
section, compact rows, "View all" link — per
`2026-09-01-tracked-trains-home-page-design.md` Decisions 1–2. The
codes-only row content is within the spec's letter (bare origin permitted)
— the improvement asked for in F3 goes beyond the spec, not against the
implementation.

## Findings — genuinely good (verified, not vibes)

- **C1. Every 404/empty state has copy and a route out.**
  `line-notfound-mobile.png` ("Browse all lines / Back to your
  dashboard"), `station-notfound-mobile.png` (which even teaches the CRS
  format: "codes are three letters, like WOK or EUS"),
  `line-history-empty-mobile.png`, `line-detail-custom-*.png`'s bounded
  trends placeholder. Nothing dead-ends except `/chat` (F8).
- **C2. Honest data-transparency copy is a real differentiator.** "2 of 10
  sampled services delayed… avg 3.7 min late"
  (`line-detail-crosscountry-desktop.png`), "Too few live departures
  sampled to report a rate right now" (`home-loggedin-populated-tablet.png`),
  the trends methodology paragraphs. Most status apps assert; this one
  shows its working.
- **C3. The login-gating pattern is genuinely consistent** across pin,
  chat, and `/track/mine` (§D1) — same component, same title, adapted
  body copy. This was three divergent hand-rolled patterns two weeks ago
  per the anonymous-UX spec; the consolidation shipped.
- **C4. The All Lines table's mobile folding is the right call** —
  Name+stats stacked, badge right (`lines-loggedout-mobile.png`); rows
  wrap predictably across ~130 entries, and sortable headers survive.
  (Its one mobile failure is the missing Pin column, F4.)
- **C5. Incident long-form content renders with real structure** — bold
  sub-headings, bullet lists for bus routes, per-operator sections
  (`BUG-auto-navigation-hijack-desktop.png` is the best specimen), far
  from the worst-case wall-of-text the capture manifest worried about.
- **C6. The theme reads as an identity.** Grape chrome + semantic status
  colors + the dashed "sleeper" divider under the nav is recognizably
  *this app* on every one of the 73 captures — the 2026-08-18 "renders as
  stock Mantine" complaint is dead. The stock-Mantine feel survives only
  in the date/time picker (`track-datepicker-desktop.png`), which is fine
  for a v1.

## Explicitly out of scope

- **The three functional bugs** already under separate investigation:
  auto-navigation hijack, `/connect-claude` authenticated 500 (already
  fixed and merged per the task brief), and the stations-lookup
  wrong-navigation. This review used their evidence files only as extra
  page captures (`BUG-auto-navigation-hijack-desktop.png` for incident
  layout, `BUG-stations-lookup-wrong-navigation-desktop.png` for the
  Trends tab) and, in F5, critiqued the *error-page design* the crash
  happened to exercise — not the crash.
- **Everything the accessibility audit already covers in depth**
  (`2026-09-02-frontend-accessibility-audit-research.md`): the six
  contrast failures (including every white-on-yellow/green/red badge
  visible throughout these screenshots — deliberately not re-flagged
  per-page above), missing `<main>` landmark, heading-order skips, and
  missing `<h1>` on error/not-found templates. Where a finding here
  touches the same pixels (e.g. F7's link color), the recommendation was
  checked not to conflict with that audit's (it doesn't — the anchor
  variable is the AA-cleared grape-7).
- The `/lines` single-character-filter oddity and the logged-out
  login-link prefetch console error — noted in the capture manifest,
  functional rather than design, not re-investigated.

## Open questions / risks

1. **Dark mode was never captured.** All 73 screenshots are light-scheme.
   The dark-theme and grape-theme specs make specific dark-mode claims
   (anchor shade 4, grape-8 filled buttons) that this review could not
   check visually; F7's blue in-content links are very likely *worse* in
   dark mode (default dark-blue anchors on dark background). A follow-up
   dark-scheme capture pass is the single highest-value addition to this
   screenshot set.
2. **Missing states** (see Method's integrity list): populated home at
   true desktop labeling, Track-a-Train form at desktop, `/train/by-id` at
   mobile, station detail at mobile, Trends tab at mobile, the matched
   `/train/[uid]/[date]` journey page (never reached — both test trains
   stayed pending), and the anonymous `/lines/new` *submit* behavior
   (does `CustomLineForm` now raise the login modal per the modal spec's
   Decision 4, or still show raw text?). None could be verified here.
3. **The "Hide advanced options" appearance question:**
   `lines-new-filled-desktop.png` shows it as a full-width light-purple
   button while the collapsed "Show advanced options" renders as a plain
   centered text link (`lines-new-blank-loggedout-desktop.png`,
   `line-edit-desktop.png`). This may simply be a captured hover state of
   a subtle-variant button; if it's genuinely state-dependent styling,
   the two states of one disclosure control shouldn't change affordance
   class. Needs a 10-second check in the running app, which this
   reviewer couldn't do.
4. **Severity taxonomy of "RAIL REPLACEMENT" and "DIVERTED" as red-family
   badges** (`line-detail-crosscountry-desktop.png`,
   `line-history-crosscountry-desktop.png`): both render in the same
   pink-red weight class as "SEVERE DELAYS" while arguably being service
   *modes* rather than severities. Whether that's a deliberate ranking in
   `lib/severity.ts` or an accident of the GROUP_COLOR map deserves a
   look before anyone acts on F11's badge-diet recommendation.
5. **The station-page relevance question behind F11** (operator-wide
   incidents flooding a station page) may be partly a data/aggregation
   question (which incidents get attached to a line) rather than purely
   presentation — same territory as the line-history spamminess research,
   which should be read together with F11 before designing a fix.
