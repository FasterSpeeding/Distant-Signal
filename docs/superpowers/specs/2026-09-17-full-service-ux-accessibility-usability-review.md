# Full-service UX / accessibility / usability review — 2026-09-17 screenshot sweep

**Status: review only. Not a fix plan, not an audit re-run, and not
approved for implementation.** No code was changed to produce this
document. It consolidates six independent thematic reviews of one
874-screenshot sweep into a single prioritised read, written to the same
rigour as `2026-09-02-frontend-ui-ux-review.md` and
`2026-09-17-accessibility-section-ux-review.md`: every claim cites the
screenshot or source file it was observed in, and where this document
goes beyond what the six source reviews could establish, it says so.

**The design specs are not binding requirements, and this review does not
treat them as such.** A spec records a decision made at one point with the
information available then. Several findings below explicitly recommend
going *against* what a spec decided — the archive's "press Search first"
precedent (§3.3), the station page's "no new headings" constraint (§3.5),
the "reuse `ShareButton` verbatim" instruction for group invite links
(§3.2), the add-ticket page's upload-tab default (§3.6), and the
detail-page decision to keep operators and `isCleared` off the incident
page (§3.3). Where the rendered UI matches a spec *and* the result is
good, that is credited (§5). Where it matches a spec and the result is
still weak, this review says so rather than hiding behind conformance.

---

## 1. Summary

### What was reviewed

The 2026-09-17 full-site screenshot sweep, indexed by
`shots-tmp/full-sweep/manifest.json`:

| Dimension | Coverage |
|---|---|
| Screenshots | 874 |
| Routes | 23 (`/` through `/trains`; every page in the app) |
| States | 6 — `default`, `authenticated`, `dark-mode`, `offline`, `api-error`, `loading` |
| Devices | 5 — `mobile-iphone14` (390×664 @3×), `tablet-ipad-mini` (768×1024), `desktop-1280x800`, `desktop-1440x900`, `desktop-1920x1080` |
| Browsers | 2 — Chromium, Firefox |

Six reviewers each took one thematic slice — Home/Chat/Sign-in,
Groups/Sharing, Incidents, Lines, Stations, Track/Train — sampled
representatively from that slice (26–55 images each, ~250 total), read
the general design docs (`DESIGN.md`, the frontend-design, dark-theme and
accessibility-audit specs) plus the route-specific specs under
`docs/superpowers/specs/`, and opened the frontend source wherever the
pixels raised a question the pixels could not answer. Their raw output is
preserved at `shots-tmp/full-sweep/reviews/0{1..6}-*.md`; this document
supersedes it for reading purposes.

### What this consolidation adds

The six slices were written independently and in parallel, so the same
defect frequently appears in three, four or six of them under six
different names. Collating them changes the shape of the result
substantially:

- **Sixteen findings are site-wide, not page-specific** (§2). Several were
  reported as separate per-page complaints by up to six reviewers; at
  least three of them are a single line of CSS or a single shared
  component. §2 is the most valuable section in this document and is
  deliberately placed first.
- **Two claims that individual reviewers could only flag as suspicions
  have been resolved against the source** and are now stated as fact: the
  `<main>` shrink-wrap (§2.1) and the malformed train fixture (§4.2).
- **One fixture-corruption finding is larger than reported** — 11 of 31
  accessibility fixtures are affected, not 9 (§4.3).
- Severity vocabulary is normalised across all six slices to
  **blocker / serious / moderate / minor / nitpick**. Individual reviews
  used axe's scale, their own scale, or both; where a reviewer's label and
  the merged evidence disagreed, the merged label is used and the change
  is noted.

### Sweep methodology caveats — read once, applies everywhere

Four artefacts of *how the sweep was captured* recur in every slice. They
are recorded here once and are **not** repeated as findings below.

1. **`api-error` and `loading` are visually identical to `default` on
   almost every route, and this is correct.** The capture tool mocks
   `/api/**` in the browser; every one of these routes fetches
   server-side, in a React Server Component, so a client-side route mock
   never gets a chance to fire. On `/stations/KGX` the two captures are
   byte-identical to each other. This is the documented limitation of the
   harness, not a page bug. Its one real consequence is a coverage gap:
   the app's genuine loading and error states — which do exist, in
   `IncidentSearchForm`, `StationTimetable`, `TrainSearchForm` and the
   `/track` departure picker — were never exercised by this sweep at all
   (§6). The one positive signal it *does* give is real: no route makes an
   unguarded client `/api/**` fetch on first paint.
2. **The sweep ran against `next dev`.** The round black "N" dev indicator
   overlaps the footer in most captures, the "Any line" select on
   `/incidents` mobile and the invite input on `/groups/[id]` mobile, and
   shows a red "1 Issue" / "2 Issues" pill on some `/lines/[id]/edit` and
   offline captures. Hydration timing and bundle size in these shots are
   therefore not representative. Re-run against a production build with
   `devIndicators: false`.
3. **Some `authenticated` captures are not authenticated.**
   `stations/authenticated__desktop-1440x900__chromium.png` is
   byte-identical to its `api-error` sibling and its nav reads "Log in";
   `incidents-id/authenticated__desktop-1440x900__chromium.png` shows
   `layout.tsx:324`'s `<Suspense fallback={<Text c="dimmed">Log in</Text>}>`
   placeholder rather than a streamed `AuthNavItem`. The sweep should wait
   for the nav to settle before shooting.
4. **Some captures fired before hydration finished.** The "Enable
   notifications" button, the `/chat` and `/groups` login modals and the
   `/groups/[id]` absolute invite URL are present in some captures and
   absent in others at the same viewport and state. "Absent in one
   screenshot" must not be read as "never renders" — but the window in
   which it is absent is itself a finding (§2.11).

---

## 2. Site-wide and cross-cutting findings

Sixteen findings recur across page categories. Each names every slice that
observed it independently, because independent rediscovery is the evidence
that the cause is shared rather than local.

### 2.1 · serious · `<main>` shrink-wraps, so every page has a different left content edge

**Observed by:** Home/Chat (C-1), Groups (G1-2, G4-1), Track/Train (X6) —
and re-verified for this document.

At a single 1440×900 viewport the left edge of the page's own content
lands at a different x on every route, determined by that page's longest
line of text rather than by any design decision:

| Route | Left edge of `<h1>` |
|---|---|
| Nav brand (all routes) | x ≈ 168 |
| `/train/W12345/2026-09-17` | x ≈ 328 |
| `/` (anonymous) | x ≈ 236 |
| `/connect-claude` | x ≈ 313 |
| `/` (authenticated) | x ≈ 476 |
| `/groups` (authenticated) | x ≈ 560 |
| `/chat/callback` | x ≈ 572 |
| `/chat` | x ≈ 681 |

Screens: `home/default__desktop-1440x900__chromium.png`,
`home/authenticated__desktop-1440x900__chromium.png`,
`groups/authenticated__desktop-1440x900__chromium.png`,
`train-uid-date/default__desktop-1440x900__chromium.png`,
`chat/default__desktop-1440x900__chromium.png`.

The Groups slice reported the same defect as three page-level problems —
a 320px list, a 250px create form and a 600px detail page, "three sibling
pages with three different content widths determined by their longest
text, not by design" — and Track/Train reported it as a fourth, "three
different form widths on three sibling forms" (`/track` ~714px, `/trains`
~865px, `/track/mine/add-ticket` ~412px). Those are not four findings.
They are one.

**Root cause (confirmed in source).** `app/globals.css:989` sets
`body { display: flex; flex-direction: column; min-height: 100vh }` for the
sticky footer. `app/layout.tsx:350` renders
`<Container component="main" size="lg" px={0} style={{ flex: 1 }}>`.
Mantine's `Container` is `max-width` + `margin-inline: auto`; inside a
flex column, auto inline margins **override** `align-items: stretch`, so
`<main>` resolves to `width: max-content` capped at the `size="lg"` value
and then centres itself. The nav escapes this only because its own
`Container` is wrapped in a full-bleed `Box`. Both the `body` rule and the
`flex: 1` carry long doc comments explaining the sticky footer; neither
anticipated the interaction.

**Recommendation.** Add `w="100%"` to the `main` `Container` (or
`align-self: stretch` on `main` in `globals.css`). One line, and it fixes
all 23 routes. Then add a Playwright assertion that `main`'s bounding
width equals the nav container's, so it cannot regress. **Do not** apply
the Groups slice's per-page `maw={640}` / `maw={480}` recommendations as
written — with the root cause fixed, a deliberate measure on the groups
list and the create form is still worth having, but as a typography
decision rather than as a workaround.

### 2.2 · serious · No mobile navigation collapse; the header eats 22–35 % of the first phone viewport

**Observed by:** Home/Chat (C-2), Groups (X1), Stations (A-3),
Track/Train (X1).

The nav is one flat `Group` that wraps. On an iPhone 14 it is three rows
anonymous and four rows logged in (brand, two rows of links, then the icon
row, then name + Log out) — roughly 500px of a 2301px anonymous home page,
~700px of the 3147px authenticated one, ~35 % of the 390×844 first
viewport on `/groups/[id]`, and on `/track/mine` the first content pixel
sits at y ≈ 300. Stations noted its own pages are the worst hit, because a
three-line `<h1>` ("Disruptions at London Kings Cross (KGX)") follows the
header. "Groups", the seventh link, is orphaned on its own row beside the
icon buttons.

Screens: `home/default__mobile-iphone14__chromium.png`,
`home/authenticated__mobile-iphone14__chromium.png`,
`groups-id/authenticated__mobile-iphone14__chromium.png`,
`track-mine/authenticated__mobile-iphone14__chromium.png`,
`stations/default__mobile-iphone14__chromium.png`.

**Spec context.** `2026-07-07-frontend-design.md` and
`2026-08-31-anonymous-user-ux-design.md` describe the nav's *contents*;
nothing specifies mobile behaviour, and the nav has since grown from four
links to ten items. The spec is silent, not opposed.

**Recommendation.** Mantine `Burger` + `Drawer` under `sm`
(`hiddenFrom="sm"` on the link group, `visibleFrom="sm"` on the burger).
Keep brand, theme toggle and Log in / avatar in the bar; everything else
in the drawer.

### 2.3 · serious · Theme and pride toggles are emoji glyphs and render as tofu in Chromium

**Observed by:** all six slices (Home/Chat C-3, Groups X3, Incidents
capture caveat, Lines X1, Stations A-6, Track/Train X3).

`components/ThemeToggle.tsx:64` renders `'🌙' : '☀️'`;
`components/PrideToggle.tsx:48-56` renders `🏳️‍🌈`, `🏳️‍⚧️` and a plain `🏳️`.
In every Chromium capture in the sweep the pride toggle is two empty boxes
`▯▯` and the theme toggle is a thin monochrome text-presentation sun;
Firefox renders both as colour emoji. Whether a user sees an icon or a
blank square depends entirely on whether their OS ships a colour-emoji
font — headless/CI Chromium, many Linux desktops, locked-down Windows
images, kiosk browsers and older Android WebViews do not. `☀️` half-works
because U+2600 has a monochrome fallback in most fonts; U+1F319 and the
ZWJ flag sequences do not.

Screens: any Chromium capture in the sweep, e.g.
`train-uid-date/default__desktop-1440x900__chromium.png` (both boxes
visible at x ≈ 1189), compared with its `__firefox.png` sibling.

**Second defect in the same component, reported only by Home/Chat:** six
of the nine pride modes share the identical plain `🏳️` glyph, so six
consecutive clicks change the `aria-label` and change nothing on screen.
Even on a device with a full emoji font, the control gives no feedback
across most of its range.

Both controls have correct `aria-label`s (confirmed by the a11y audit), so
this is a sighted-user problem, not a screen-reader one.

**Recommendation.** Tabler `IconSun` / `IconMoon` / `IconSunMoon` for the
theme toggle — which also retires the "A" badge (§2.16). For pride, render
a small CSS-striped swatch inside the `ActionIcon` keyed by mode: the
stripe colours already exist in `globals.css` as
`body[data-pride='…']::before` gradients (lines 298–400), so this is
deterministic, font-independent, and actually distinguishes the nine
modes. If the emoji is a deliberate brand choice, bundle `Noto Color
Emoji` via `next/font/local` so rendering stops depending on the OS.

### 2.4 · serious · Four private routes render a bare heading until (or unless) a client-side login modal opens

**Observed by:** Home/Chat (CH-1), Groups (G1-1), Track/Train (M1, A1).

`AutoOpenLoginPrompt` is the only logged-out content on `/chat`,
`/groups`, `/track/mine` and `/track/mine/add-ticket` (confirmed: those
four `page.tsx` files are its only importers). It is a client component
whose modal opens on mount. The server HTML is a heading and nothing else.

Across the sweep, the modal was caught open in **one** state per route —
`offline` — and missed in the other four. On `/track/mine`, `default`,
`dark-mode`, `api-error` and `loading` all show the `<h1>` over ~700px of
empty gradient. Whether the modal mounted late or the harness dismissed
it, the result is exactly the state a real visitor lands in the moment
they press Escape, and the state every visitor sees before hydration, on a
slow connection, or with JS disabled. Search engines and link unfurlers
see an empty page.

Screens: `track-mine/default__desktop-1440x900__chromium.png`,
`track-mine/api-error__…`, `track-mine/loading__…`,
`groups/default__desktop-1440x900__chromium.png`,
`chat/default__desktop-1440x900__chromium.png` (all bare) versus
`track-mine/offline__desktop-1440x900__chromium.png`,
`groups/offline__…`, `chat/offline__…` (modal open).

**The app already contains the fix.** `/train/by-id/[trackingId]` renders
a server-side `<h1>` plus an inline underlined "Log in to view this
tracked train", visible in all five states including dark mode, with no
dependence on a client modal
(`train-by-id-trackingId/default__desktop-1440x900__chromium.png`).
`/groups/[id]` and `/groups/join/[token]` do the same. Three routes do it
right; four do not.

**Spec context.** `AutoOpenLoginPrompt.tsx:16-21` records the modal-only
approach as "a deliberate, accepted simplification (Decision 6's Open
Question 1)", and the modal-login spec's own D1 dissent predicted the
empty-page-behind-a-dismissed-modal outcome. The sweep reproduces the
dissent in pixels on four routes. **Revisit the accepted simplification.**

**Recommendation.** Render the same sentence and a `LoginLink` inline in
the server markup on all four routes, and keep the auto-open modal as a
progressive enhancement rather than as the content. The modal itself is
good and should be kept (§5). One shared change; the copy differs per
route and already exists in each modal.

### 2.5 · serious · `wrap="nowrap"` with no shrink guard truncates labels and status badges on mobile

**Observed by:** Home/Chat (H-3), Groups (G2-2), Track/Train (M2).

Three reviewers found three instances of one pattern, in three components
written by different authors:

- `app/page.tsx:770` — tracked-train status badges clipped to their first
  two characters, "EN …" and "4M…", on the authenticated home
  (`home/authenticated__mobile-iphone14__chromium.png`).
- `app/groups/[id]/page.tsx` (`SharedTrainRow`) — "Remove from group"
  rendered as "**Remove fr**", overflowing the card edge, in **both**
  browsers (`groups-id/authenticated__mobile-iphone14__chromium.png`,
  `…__firefox.png`). Groups notes `SharedCustomLineRow` carries the same
  `wrap="nowrap"` and will fail identically as soon as a line is shared.
- `app/track/mine/page.tsx:232` — "EN ROUTE" and "4M LATE" collapse to
  ~20px chips showing "E" / "4" (Chromium) or "I" / "4" (Firefox)
  (`track-mine/authenticated__mobile-iphone14__chromium.png`).

The mechanism is identical every time: `Group justify="space-between"
wrap="nowrap"` around a variable-length `Text` and a `Badge`/`Button` with
no `flexShrink: 0`; Mantine's `Badge` truncates with `overflow: hidden`,
the long title wins the space contest, and the element carrying the row's
only status information loses it.

**Severity note.** Groups correctly identifies the button case as an
accessibility defect and not merely a cosmetic one: a control whose
visible label is clipped no longer matches its accessible name (WCAG
2.5.3). The badge cases are worse in practice — mobile is the natural
device for "is my train late", and on mobile the answer is a single
letter.

**This is a convention problem, not three bugs.** There are 30
`wrap="nowrap"` sites in `frontend/app` and `frontend/components`, about a
third of them with `justify="space-between"`. Two of them —
`LineStatusCard.tsx:17-23` and `IssueList.tsx:342` — carry explicit doc
comments about which element is allowed to shrink and why. The house
convention exists and is documented; it simply is not applied at every
site.

**Recommendation.** Fix the three known instances with
`style={{ flexShrink: 0 }}` on the badge/button and `lineClamp` on the
title. Then extract the row shape into one shared primitive (a
title-plus-status row that gets the shrink rule right once), and add a
lint rule or a render test asserting that a `Group wrap="nowrap"`
containing a `Badge` gives the badge a shrink guard. Never let a status
badge truncate.

### 2.6 · serious (a11y) · No skip link; 10+ focusable nav items precede `<main>` on every route

**Observed by:** Home/Chat (C-6). Site-wide by construction.

Keyboard users tab through All Lines, Station Lookup, Find a Train,
Incident Archive, My Trains & Tickets, (Groups), info, theme, pride and
Log in on every page load before reaching content.
`2026-09-02-frontend-accessibility-audit-research.md` never planned a skip
link, so this is unaddressed rather than regressed.

**Recommendation.** A visually-hidden-until-focused "Skip to content" link
as the first child of `<body>`, targeting the existing `<main>`
(`app/layout.tsx:350`). Cheap, and WCAG 2.4.1.

### 2.7 · moderate · The authenticated desktop nav wraps to two rows at 1440px

**Observed by:** all six slices (Home/Chat C-2, Groups X2, Incidents A9,
Lines X2, Stations B-18, Track/Train X2).

Adding "Groups", the user's display name and "Log out" pushes "Distant
Signal" onto a row of its own and drops the whole nav to row two, moving
the sleeper rule and all page content down ~40px. The anonymous nav at the
same width fits on one row, so **the header changes height the moment
someone logs in** and the two states read as different applications. In
Firefox it fits on one row at 1440 because Gecko's text metrics are ~4%
narrower — the layout is sitting exactly on the wrap threshold, which
means real users will see it break or not break depending on their font
stack.

Screens: compare `track/default__desktop-1440x900__chromium.png` with
`track/authenticated__desktop-1440x900__chromium.png`; also
`groups/authenticated__desktop-1440x900__chromium.png`,
`lines/authenticated__desktop-1440x900__chromium.png`,
`incidents/authenticated__desktop-1440x900__chromium.png`,
`stations-crs/authenticated__desktop-1440x900__chromium.png`.

**Recommendation.** At ≥ `md`, give the header a fixed height and move
"My Trains & Tickets", "Groups" and "Log out" under an account menu keyed
by an avatar, rather than letting the bar `flex-wrap`. Dropping the
display name from the bar and shortening "My Trains & Tickets" to "My
Trains" buys ~150px on its own if a smaller change is wanted first. This
is the same work as §2.2 and should be done with it.

### 2.8 · moderate · `TextLink` is block-level, so every mid-sentence link breaks onto its own line

**Observed by:** Track/Train (D6, S3); the underline symptom also by
Home/Chat (C-7). Confirmed in source for this document.

`components/TextLink.tsx` renders `<Link><Text>{children}</Text></Link>`,
and Mantine's `Text` is a `<p>`. Every call site that puts a link inside a
sentence therefore produces three lines where one was intended:

> This is the public view of this service. Track it above to get updates, or
> **Find a train**
> going somewhere else.

Visible in `train-uid-date/default__desktop-1440x900__chromium.png`,
`train-by-id-trackingId/authenticated__desktop-1440x900__chromium.png`
and `trains/default__desktop-1440x900__chromium.png` ("Can't find your
train? ⏎ Track it manually ⏎ by entering…"). There are **126 `TextLink`
call sites** across `frontend/app` and `frontend/components`; the
component's own doc comment shows it was designed for both positional
links (nav items, actions beside a heading) and in-prose links, and the
block-level render only hurts the second class.

Home/Chat's C-7 — "Log in to pin your lines and stations" shows visible
gaps in the underline at each word boundary, in both engines
(`home/default__mobile-iphone14__chromium.png`) — is the same component
and probably the same cause: the decoration is being drawn on an inner
span per word rather than on the anchor.

**Recommendation.** Give `TextLink` a `component="span"` / `inline` render
path, apply it at the in-prose call sites, and set
`text-decoration-skip-ink: none` on the `a[data-text-link]` rule in
`globals.css` so the underline is continuous. At these call sites also set
`underline="always"` — colour is currently the only cue for a mid-sentence
link (WCAG 1.4.1), which the component's own doc comment already says is
what `'always'` is for.

### 2.9 · moderate · Internal codes and engineering vocabulary reach user-facing copy on every page category

**Observed by:** all six slices.

This is the single most widely-distributed content problem in the app, and
the 2026-09-02 review's F3 and F5 already named the class. Every slice
found fresh instances:

| Leak | Where | Slice |
|---|---|---|
| `en_route` printed verbatim | `/groups/[id]` shared-train card | Groups G2-3 |
| `main-line` (TOML enum), `GR` (ATOC code) | `/lines/[id]` "Category" / "Operators" | Lines D2 |
| `e.g. SW` as an operator placeholder | `/lines/new`, `/lines/[id]/edit`, `/track` | Lines N2/E4, Track T1 |
| "Origin CRS code" / "Destination CRS code" as field labels | `/track/mine/add-ticket` | Track A2 |
| Bare CRS as a destination — "London Kings Cross (KGX) → **EDB**" | `/`, `/track/mine`, both train pages | Home H-5, Track M4 |
| Unlabelled CRS pills "KGX" "EUS" floating in the page | `/incidents/[id]` | Incidents D3 |
| "Knowledgebase" (×3), "matcher", "reprocessing pass", "before this filter was fixed" | `/incidents` helper copy | Incidents A5 |
| "2 status **recomputes** across 2 incidents" | `/lines/[id]/history` | Lines H4 |
| "NETWORK RAIL **PROPAGATED**" on the most-read line of the page | both train pages | Track D7 |
| "Last **fetched**" (the poller's word) | `/incidents/[id]` | Incidents D7 |
| "Lifts info", "Names", "Atm", "Cctv available", "Drop off pick up" | `/stations/[crs]` | Stations B-12 |

Two of these deserve separate emphasis:

- **The mixed route label is one helper, used in three places.**
  `routeLabel()` gives the origin a name *and* a code but the destination
  only a code when its name is null, producing "London Kings Cross (KGX) →
  EDB" in the `<h2>` of the only card on `/track/mine`, in the ticket
  line, on the home dashboard and again on both train pages. Falling back
  to the code is right; mixing the two forms in one string looks like a
  data error to a passenger. Either resolve both ends through the stations
  lookup the autocomplete already uses, or render both as codes.
- **ATOC codes are asked for as input, not just shown as output.** The
  operator field on three separate forms expects a two-letter TOC code
  from a passenger who knows "LNER" and "South Western Railway", while the
  station field beside it happily accepts a name. `poller-tocs` already
  holds names and codes.

**Recommendation.** A shared display-label layer — status enum → label
(the tracked-train status helper already exists; the groups page should
reuse `TrackedTrainStatusBadge` so `/groups/[id]` and `/track/mine`
agree), TOC code → operator name, CRS → station name, category enum →
label — applied at render rather than fixed string by string. Replace
"Knowledgebase" with "National Rail incident messages", "recompute" with
"status change", "fetched" with "updated from National Rail", and
"propagated" with "Estimate (Network Rail)" keeping the long form in the
tooltip *and* in a `VisuallyHidden` span.

### 2.10 · moderate (a11y) · Interactive targets below the 24px minimum on the controls users tap most

**Observed by:** Incidents (A8), Lines (D5, D6, E1), Stations (B-15),
Track/Train (D10, D12).

A consistent pattern of small hit areas on touch, concentrated on
icon-only controls:

- `/lines/[id]/edit` — the `×` inside each grape station chip is **~10 CSS
  px** and is dark grey (73,80,87) on grape-7 (174,62,201) at **1.69:1**.
  That is a hard **WCAG 1.4.11** non-text-contrast failure (3:1 required),
  and it is the only measured hard failure in the whole sweep. Cause: the
  chip is a filled `Badge` whose `CloseButton` keeps the default grey icon
  colour rather than the badge's white text. Fix: `white` (4.85:1 on
  grape-7, already used for the label) plus a 24px minimum hit area.
  (`lines-id-edit/authenticated__mobile-iphone14__chromium.png`)
- `/incidents` — date preset buttons ~30px, segmented segments ~32px, the
  date-clear "×" ~20px
  (`incidents/default__mobile-iphone14__chromium.png`).
- `/lines/[id]` — share `ActionIcon` 28px, the ⓘ ~20px, the issue-row
  expander chevron ~16px
  (`lines-id/default__mobile-iphone14__chromium.png`).
- `/stations/[crs]` — pin star and share ~28px, "the two actions a
  returning user taps most"
  (`stations-crs/default__mobile-iphone14__chromium.png`).
- Train pages — progress-line nodes are ~14px `<button>`s and are the only
  way to learn an intermediate stop's name on touch; share button 32px
  (`train-uid-date/default__mobile-iphone14__chromium.png`).

**Recommendation.** Keep the glyph sizes; pad the hit areas. A single
utility (`::before` inset, or a shared `ActionIcon` size default) applied
to the icon-button set gets everything to ≥24px, with 44px on the
primary-action ones. Fix the chip `×` contrast separately and first — it
is the one item here that fails a WCAG success criterion outright.

### 2.11 · moderate · Client-only values pop in after hydration, with no server-side placeholder and no labelled loading state

**Observed by:** Home/Chat (H-2), Groups (G2-4), Lines (D1, H3).

Four instances of one habit — computing a value in a `useEffect` and
rendering nothing (or an unlabelled grey box) until it resolves:

- **`/` — "Enable notifications" appears late and moves the hero.**
  `NotificationsToggle.tsx:50-52` sets `supported` in a `useEffect`, so the
  button pops in after hydration on the most-visited page in the app. On
  mobile it is inserted *between* the `<h1>` and the tagline, splitting the
  hero. Absent from `home/default__desktop-1440x900__chromium.png`, present
  in `home/default__mobile-iphone14__chromium.png`.
- **`/groups/[id]` — the invite link is a bare path, and Share is inert.**
  `GroupInviteLinkCard.tsx` builds the URL from `window.location.origin` in
  a `useEffect`, so all four authenticated captures show
  `/groups/join/fixture-invite-token-123456789` in the read-only input —
  which does not work if a user selects and copies it — and `share()`
  early-returns while `origin === ''`, so a click in that window silently
  does nothing.
- **`/lines/[id]` — the two "Recent trends" Suspense boundaries never
  resolved in mobile Chromium**, leaving two flat 280px grey blocks with no
  text, in both light and dark
  (`lines-id/default__mobile-iphone14__chromium.png`). Desktop, tablet and
  **Firefox mobile** all show the resolved "Not enough data" boxes, which
  leans towards a timing artefact but does not rule out a real hydration
  bug (§4.7).
- **`/lines/[id]/history` — the Timeline panel is an unlabelled 240px grey
  box** in the default desktop and tablet captures while every other state
  of the same URL shows the resolved list.

In every case the fallback is a bare `Skeleton` with no accessible name, no
visible text, and a height chosen for populated content rather than for
what actually resolves — a 560px hole that collapses to two lines of "not
enough data" is a large layout shift, and a screen reader hears nothing at
all while it is there.

**Recommendation.** (a) Reserve the slot server-side: render the
notifications button always, `disabled` until `supported` resolves; compute
the invite URL server-side from a `NEXT_PUBLIC_SITE_URL` / request host and
render Share `disabled` with a tooltip until it is real. (b) Give every
`Suspense` fallback a visible "Loading trends…" / "Loading history…" line
inside a `role="status"` region with `aria-busy`, and size the skeleton to
the *empty* state rather than the populated one.

### 2.12 · moderate · Times are printed without a timezone, and nothing ever says when the data was last updated

**Observed by:** Incidents (D7), Lines (H4, X4), Track/Train (D9).

`/incidents/[id]`'s "First seen / Last fetched", `/lines/[id]/history`'s
per-row "19:34" and the train pages' entire summary block all print bare
times with no zone marker. The history spec's timeline is explicitly
anchored on "London midnight", so the zone *is* Europe/London — but a
traveller abroad checking a UK line, or anyone reading around the clock
change, cannot tell.

More seriously, **no page in the app says how fresh its data is.** The
disconnect/reconnect spec's whole model is "showing the last update" — and
that promise is unverifiable, because nothing states when the last update
was. On the train detail page, which is the app's one genuinely live view,
there is no "as of 20:41" anywhere, so a user cannot tell whether "4M LATE"
is thirty seconds or three hours old. A `LastUpdated` component exists in
`frontend/components/` and is not on that page.

**Recommendation.** Append the zone once per section ("Times in UK local
time") rather than per row. Put `LastUpdated` under the train summary block
("Updated 20:41 · refreshes every 30 s"), and surface the same freshness
timestamp inline in the offline notification's body so "the last update"
names itself.

### 2.13 · moderate · Two different idioms for "pick exactly one" on the same page

**Observed by:** Incidents (A6, A7), Lines (H2).

Both the incident archive and the line history page stack date-preset pill
buttons — where the selected one is `filled` and the rest `light` — directly
above a `SegmentedControl` expressing the same one-of-N choice. Filled
versus light reads as primary/secondary *buttons*, not as a selection: a
first-time user reads "Last 7 days" as the action and "Last 30 days" as a
lesser action. The 2026-09-02 review made this point about the history page
(F14); it now applies to two pages.

Two aggravating factors, one per slice:

- **Dark mode makes it worse** (Incidents A7): unselected `light`-variant
  presets are grape text on a dark-grape fill, and the segmented control's
  selected segment is slightly-lighter grey on grey — both markedly less
  legible than in light mode
  (`incidents/dark-mode__desktop-1440x900__chromium.png`).
- **Three controls for one value** (Lines H2): presets apply immediately
  while the adjacent date-range picker needs a separate "Show history"
  submit, so one piece of state has two interaction models.

**Recommendation.** Make the presets a `SegmentedControl` (or `Chip.Group`)
labelled "Period", with `color="grape"` so the selected segment uses the
brand fill in both schemes. On the history page, make it
`7 days / 30 days / Custom…` and show the date picker and submit only when
"Custom" is chosen — which also removes the undefined state where a
hand-edited date leaves no preset highlighted and nothing explains why, and
recovers ~90 CSS px on mobile.

### 2.14 · moderate · `IssueList`'s filter chrome outweighs the content it filters, on two routes

**Observed by:** Lines (D3), Stations (B-17). Adjacent form-level instances:
Incidents (A1, A4).

The same component renders on `/lines/[id]` and `/stations/[crs]`: a
"Severity — showing all" row with one chip, a "Source — showing all" row
with five chips (Knowledgebase / LDBWS-inferred / Trust-inferred / Planned /
TfL), and an All/Active/Upcoming segmented control — approximately 450 CSS
px of filter UI above **one** incident on mobile. Stations flags it as "the
second thing a station-page reader scrolls past" on the way to the content
they came for (§3.5).

`/incidents` has the form-level version of the same imbalance (§3.3): a
~700px-tall filter form, in which the "Priority (raw feed value — meaning
undocumented)" block — the least usable filter on the page — is visually the
heaviest thing on it, with the caveat repeated three times.

**Recommendation.** Collapse the chip filters behind a "Filter" disclosure
when there are ≤3 issues, and only render source chips for sources actually
present in the loaded report (the filters are client-side over loaded data,
so the set is known). Keep the All/Active/Upcoming control always visible —
it carries counts, which is useful on its own.

### 2.15 · minor · Firefox: the dashed "sleeper" rule degrades, and the app's typeface is not being delivered

**Observed by:** Home/Chat (C-5), Groups (X4), Lines (X3), Stations (A-4,
A-5), Track/Train (T5).

Two separate cross-browser issues, both seen by most slices:

- **The sleeper rule.** Chromium draws an even dashed rule across the full
  width under the nav. Firefox desktop draws a sparse row of ticks for the
  first ~250px and nothing further; Firefox mobile draws small evenly spaced
  dots. Home/Chat additionally reports visible horizontal banding in the
  Firefox background wash (bands at roughly y=230, 360, 480, 610 on
  desktop), which is Gecko dithering the long
  `color-mix(… 6%, transparent)` gradient. The 2026-09-02 review called
  this divider part of the app's identity (C6), so a browser where it
  vanishes is worth ten minutes. A `repeating-linear-gradient` with an
  explicit `background-size`, or an inline SVG pattern, renders identically
  in both engines.
- **The typeface — worth verifying, possibly a real bug.** Groups and
  Stations independently note that *every* Firefox capture renders in a
  Helvetica/Arial-class face while Chromium renders the app's rounder sans.
  The likely explanation is that the font is not being served as a webfont
  at all and is relying on the host having it installed — in which case
  Firefox is not the anomaly; it is what a Windows or Android user sees.
  Confirm the webfont is actually delivered. This also explains §2.7's
  knife-edge wrap behaviour: Gecko's fallback metrics are ~4 % narrower.

### 2.16 · minor · Small, inconsistent and under-weighted chrome controls

A cluster of individually-trivial items that together make the shared
chrome look unfinished. Grouped here so they can be fixed in one pass
alongside §2.2 / §2.7.

- **The "A" (auto) badge on the theme toggle is illegible at phone size** —
  a 12px `Indicator` carrying an ~8px "A" over the button's bottom-right
  corner, reading as a smudge on the border
  (`home/default__mobile-iphone14__chromium.png`). `ThemeToggle.tsx`'s own
  comment records why the badge exists (auto→light can look like a no-op); a
  distinct `IconSunMoon` solves the same problem without a badge, so this
  resolves for free with §2.3. *(Home/Chat C-4)*
- **Auth controls are inconsistently sized** — "Log in" is a 16px text link
  while "Log out" is a ~12px bold button; the footer's "powered by
  NationalRail" is a 12px underlined link with a ~16px hit height.
  *(Home/Chat C-8)*
- **The anonymous call to action is routinely the weakest element on the
  page.** On `/groups/join/[token]` — the page an invitee reaches by
  definition anonymous, where logging in *is* the join step — the action is
  an underlined text link while the authenticated equivalent is a filled
  full-width button
  (`groups-join-token/default__desktop-1440x900__chromium.png`). The same
  inversion appears on `/connect-claude` (Home/Chat CC-2), `/groups`
  (G1-3), and implicitly on `/lines/new`, `/track` and both train pages,
  where a prominent filled primary button gives no hint that it needs an
  account (Lines N4, Track T2/D11). *(Groups G3-2, G1-3; Home/Chat CC-2;
  Lines L6/N4; Track T2/D11)*
- **The offline banner's copy is wrong on form pages, and it shrink-wraps
  on phones.** "Can't reach live data right now — showing the last update."
  is honest and tight on a data page, and untrue on `/track`, `/trains`,
  `/track/mine/add-ticket`, `/lines/new` and `/lines/[id]/edit`, where
  there is no update to show; the thing the user needs to know there is
  whether their entries are safe. On a 390px phone the `Notification`
  shrinks to ~200px and the body wraps to four lines
  (`stations-crs/offline__mobile-iphone14__chromium.png`); give the fixed
  wrapper `width: min(calc(100vw - 32px), 480px)`. *(Lines X4, Track X4,
  Stations B-16)*

---

## 3. Per-area findings

Everything promoted to §2 is omitted here and cross-referenced instead.
What remains is genuinely page-specific.

### 3.1 Home, Chat and Sign-in — `/`, `/chat`, `/chat/callback`, `/connect-claude`

**serious — `/chat/callback` hangs on "Connecting…", or shows a raw error
under a heading that still says "Connecting…".** Most captures show
"Connecting… / Finishing sign-in to the rail data service." with no spinner
and nothing else; `chat-callback/api-error__desktop-1440x900__chromium.png`
and `…/offline__…` show the post-hydration result — "Connecting…" still as
the `<h1>`, and beneath it, in red, "No authorization code was present in
the callback URL." with no link, no button, and no explanation.
`app/chat/callback/page.tsx:55-60` keeps the title in both branches and
`:51` renders `err.message` verbatim, so SDK and network error strings reach
the user unfiltered. **Recommend:** switch the heading with the state
("Connecting…" with a `Loader` → "Connected, taking you to Chat…" →
"Couldn't connect"); on error give one plain sentence, a primary "Back to
Chat" button, and the raw message in a collapsed `<details>`; add a ~20s
timeout on the `auth()` exchange so a stalled token endpoint does not leave
"Connecting…" forever. The error is also colour-only — a Mantine
`Alert color="red"` with an icon gives it `role="alert"` too (WCAG 1.4.1).

**serious — the logged-in-but-not-allowed `/chat` state is a dead end.**
"Chat — Not available for your account yet." in dimmed text, nothing else
(`chat/authenticated__desktop-1440x900__chromium.png`).
`2026-09-02-embedded-chatbot-dual-mode-design.md` deliberately wants a 200
with a plain "not available" state rather than a 404 — correct, the
feature's existence is not secret — but the 2026-09-02 UI review's F8
already asked for one more sentence and a link out, and it is still not
there. **Recommend:** say what chat is, and point to `/connect-claude`,
which works for every logged-in user today. That turns a dead end into a
route to the same capability.

**moderate — `/chat` is undiscoverable.** No nav item on any route;
allow-listed users have to know the URL. `getChatbotAccess()` can be
wrapped in the same `Suspense` as `GroupsNavItem`.

**moderate — the authenticated dashboard reads as a stack of empty
prompts.** "Your Lines — You haven't pinned any lines yet. Browse all lines
to pin some." with a *second* "Browse all lines" link in the heading row
40px above; the same doubling for stations; then "Right now", which is the
only section with content (`home/authenticated__desktop-1440x900__chromium.png`).
**Recommend:** when a section is empty, keep one of the two links, not
both; and order "Right now" first when both pinned sections are empty —
the F2 fix's own rationale was "don't reward login with a blank page".

**moderate — no way to start the MCP connection from `/chat`** (from source,
not screenshots; the sweep had no allow-listed user).
`ChatPanel.tsx:87-92` returns early with a "reconnect from the Chat page"
error when no MCP token is stored, and the only `auth()` call is in
`/chat/callback` — so a first-time allowed user appears to have no button
that begins the OAuth flow. Flagged for the follow-up sweep (§6).

**minor — `/connect-claude` polish.** The connector URL will be a long
HTTPS string the user must retype into Claude's UI; add a `CopyButton`
with `aria-label="Copy connector URL"`. The plan-requirement callout is
Mantine's default blue `Alert`, on a page with no other status colour —
`2026-08-18-grape-theme-design.md` reserves blue for `planned` severity, so
`color="grape" variant="light"` with `IconInfoCircle` is a better fit (the
same note applies to `ChatPanel.tsx:270`'s `bg="blue.0"`). "Click **+**"
would read better as "Click the **+** button", and `--` should be an em
dash.

**minor — the brand appears twice above the fold.** "Distant Signal" in the
nav and again as the 34px `<h1>`, ~100px apart on mobile. Keeping the
`<h1>` for the document outline is right; it does not need to *say* the
brand. "Live UK rail status" with the existing tagline would earn the
space. Only worth doing alongside §2.2.

### 3.2 Groups and Sharing — `/groups`, `/groups/[id]`, `/groups/join/[token]`, `/groups/new`

**serious — "Delete group" and "Leave group" are visually identical and sit
12px apart.** Three buttons on the heading row: "Rename" (default), "Delete
group" (red outline), "Leave group" (red outline) — same variant, size,
weight, colour and height
(`groups-id/authenticated__desktop-1440x900__chromium.png`); on mobile they
wrap onto one row with the same 12px gap. The blast radii are not
comparable: Leave affects one person (and for the owner silently transfers
ownership to the longest-standing member, per spec §2.1); Delete destroys
the group, every member's access, every shared train and the invite link.
Both are behind confirm modals with distinct titles and `aria-label`s —
good, and a mis-tap is recoverable — but the modal is doing all the work
the button weighting should share. **Recommend:** keep "Leave group" as the
red outline button; demote "Delete group" to a subtle red text button in a
"Danger zone" at the foot of the page, or behind a "…" menu beside Rename;
stack the header actions full-width with `gap="sm"` on `xs`. Also check the
Leave modal's owner branch says "X will become the owner" — ownership
transfer is the one consequence a departing owner would not guess. *(Spec
§6 groups Rename/Delete/Leave as "the group's own header controls", but
describes what exists, not where it sits — this is a layout deviation the
spec does not forbid.)*

**moderate — the invite link never says it expires.** Spec §2.3 gives every
link a 7-day life and the UI says nothing; "Expires 24 Sept" beside the
input would pre-empt the "my link stopped working" support question.
Relatedly, "Regenerate" does not say the old link dies — a manager who
regenerates to "refresh" it will invalidate a link already sent to family.
Helper text under the buttons, or a tooltip. (Revoke being red-outline
against Regenerate's default is already the right distinction.) *(The
absolute-URL half of this finding is §2.11. Spec §6 says "reuse
`ShareButton`'s copy/Web Share pattern verbatim" — followed literally,
which is the cause: `ShareButton` shares the *current* page so it never
needs a server-side origin; the invite card shares a **different** URL and
should get it from the server. Deviate.)*

**moderate — an existing member is offered "Join group".** The
authenticated captures are the group's own owner, and they still get "Join
Fixture Family?" with a Join button
(`groups-join-token/authenticated__desktop-1440x900__chromium.png`),
because the page only calls the unauthenticated preview. The person most
likely to click an invite link is the one who created it, checking it
works. **Recommend:** when `session.authenticated`, probe membership and
render "You're already in Fixture Family — Open group"; and make
`JoinGroupButton` treat a 409/already-member response as success, routing
to the group rather than showing an error alert.

**moderate — mobile member rows split awkwardly and Promote has no
confirmation.** "Rider Fixture" on one line, the MEMBER badge on the next,
and on the right "Promote to admin" stacked ~20px directly above "Remove",
both ~36px tall and well inside a thumb
(`groups-id/authenticated__mobile-iphone14__chromium.png`). Remove has a
confirm modal; `PromoteMemberButton.tsx` fires immediately with no toast —
reversible via Demote, but silent. **Recommend:** on `xs`, name + badge on
one line and the actions on their own full-width line (or a per-row "…"
`Menu`); keep Remove red and Promote neutral; give Promote a lightweight
confirmation or at least a success notice.

**minor — navigation and identity gaps.** No breadcrumb or "← Groups" above
the `<h1>` on the detail page. No "(you)" marker in the member list, which
becomes impossible to resolve once the `memberLabel` placeholder path ("A
member (#a1b2c3)") is in play. Section `h2`s ("Members", "Shared trains",
"Shared custom lines") render at nearly `h1` scale on mobile —
`Title order={2} size="h4"`, the pattern `/lines/[id]` already uses, keeps
the outline and fixes the scale. The group card is wrapped in a `<Link>`
with `textDecoration: none; color: inherit`, so nothing signals it is
clickable — a trailing chevron or a `Card` hover/focus style.

**minor — `/groups/new` says nothing about what happens next.** A heading,
one field and a disabled button. One dimmed line — "You'll get an invite
link to share as soon as it's created." — matches what the form actually
does. The disabled submit is grey-on-grey in dark mode; enabling it and
validating client-side would avoid the low-contrast state entirely.

**minor — anonymous handling differs across three adjacent routes.**
`/groups` auto-opens the login modal; `/groups/[id]` and
`/groups/join/[token]` render inline links. Resolving §2.4 towards the
inline pattern settles this too. (`/groups/[id]`'s anonymous branch
correctly does *not* leak the group name — see §5.)

### 3.3 Incidents — `/incidents`, `/incidents/[id]`

**serious — the incident detail page never says who is affected or whether
it is over.** The page renders a title, "REAL-TIME", a share button, one
paragraph, two unlabelled pills ("KGX", "EUS"), Validity, "Currently
affects — Not currently reported on any tracked line.", an empty History
heading, and First seen / Last fetched
(`incidents-id/default__desktop-1440x900__chromium.png`). `page.tsx:95-173`
renders neither `incident.operators` nor `incident.isCleared` at all,
though both are in the API response and both appear on the archive rows.
The real reader is an anxious commuter arriving from a line page or a
shared link asking "does this affect my train, and is it still happening?"
— and the operator (LNER) is discoverable only by reading the prose, while
whether the incident is cleared is not shown anywhere. A cleared incident
with an "ongoing" validity period — common, per the stale-incident spec —
will look live. "REAL-TIME" + "ongoing" + "Not currently reported on any
tracked line" reads as a contradiction to anyone who does not know what
"tracked line" means. **Recommend:** an at-a-glance strip under the title —
Status (Active/Cleared, the badge the archive rows already use), Operators
by name, Affected stations labelled with names *and* codes, Planned /
Real-time — and when `isCleared`, replace the "Currently affects" section
with "This incident has been cleared." *(Detail-page spec Decision 6 keeps
NLP/severity fields off the page; still reasonable. But operators and
`isCleared` are not extraction fields — they are already on the row and
already public. Deviate.)*

**serious — the archive lands on an empty form even though a default filter
is already applied.** The "30 days" preset is highlighted and From is
pre-filled with 18 Aug 2026, yet the results area says "Press Search to
browse incidents across the network."; `IncidentSearchForm.tsx:245-250`
confirms `results` starts `null`. A visitor who clicks "Incident Archive"
in the nav gets a title, a paragraph, nine controls and no incidents — and
on a phone that Search button is two full screens down. **Recommend:** run
the search once on mount whenever the initial filter set is non-empty (it
always is, because of the 30-day floor), keeping the explicit Search button
for re-querying; and at desktop width put results above or beside the
filters, since the form is ~700px tall before a single incident can be
seen. *(Archive spec Decision 6 copies `TrainSearchForm`'s shape. Keep the
mechanics; drop the "press Search first" part of the precedent — a train
search needs input, an archive does not. The spec's own reason for the
30-day floor was to make the first view useful.)*

**serious (a11y) — the two segmented controls have no name, visibly or
programmatically.** Two full-width rows reading "All | Planned work |
Real-time" and "All | Active | Cleared", with no caption above either;
neither `SegmentedControl` (`IncidentSearchForm.tsx:473-490`) has an
`aria-label` or a wrapping label, while every other control on the form
does. Sighted users see two rows both beginning with "All" and must infer
the axis; screen-reader users get a radiogroup with no accessible name at
all (WCAG 1.3.1 / 4.1.2). **Recommend:** wrap each in `Input.Wrapper`
labelled "Type" and "Status", matching the label-plus-description pattern
the operator and line selects already use.

**moderate — "History" renders as a heading with nothing under it.**
`page.tsx:151-161` maps over `incident.history` with no empty branch. The
detail-page spec assumed history "always has at least the first-seen
snapshot"; the fixture demonstrates it does not, and a freshly ingested
real incident hits the same path between ingest and first change.
**Recommend:** "No changes recorded since this incident was first seen."

**moderate — the priority block is the heaviest thing on the form, for the
least usable filter.** Two bold "Priority (raw feed value — meaning
undocumented)" labels side by side, each with a "Minimum/Maximum,
inclusive." sub-label, then a third repeat of the caveat as a footnote; on
mobile each label wraps to three lines, making the priority block taller
than the operator and line selects combined. The honesty is right; saying
it three times reads as an apology in bold. **Recommend:** one
`Input.Wrapper label="Priority range"` containing "Min" and "Max", with the
single existing footnote as its description — or move it behind a "More
filters" disclosure. *(Archive spec Decision 3 — priority stays raw and
honestly labelled — is right in intent and over-applied in execution.)*

**moderate (a11y) — section headings on the detail page are not headings.**
"Validity", "Currently affects" and "History" are `Text fw={500}`
(`page.tsx:122,133,152`), so the document outline is a lone `<h1>` and
nothing else. `Title order={2} size="h5"` is visually identical.

**moderate — the mobile title consumes ~40 % of the first viewport.** A
ten-word summary wraps to four lines at full `h1` size before the badge
appears (`incidents-id/default__mobile-iphone14__chromium.png`), and
Knowledgebase summaries are routinely twice this length. The 2026-09-02
review flagged this (F14) and it is unchanged. `size="h2"` or a responsive
`fz`, and consider `lineClamp` with the full text as `title`.

**minor — the detail page is a dead end.** No back link or breadcrumb; the
only exits are the global nav. This is the page most likely to be reached
from a shared URL with no history to go back to. A small "← Incident
Archive" `TextLink` above the title.

**minor — small things.** "To (optional)" is an empty bordered box with no
placeholder while "From" has a value and a clear button, so the pair looks
unbalanced (`placeholder="Any"`). At 1440 the selects and number inputs
stretch the full 1100px container — a `maw` of ~720px would read as a form
rather than a table (this is partly §2.1). The share `ActionIcon` sits alone
on its own row between the badge and the body, because
`Group justify="space-between"` wraps once the `h1` spans the container;
put it on the badge's row.

**Flagged pre-emptively, unverified — "badge soup" on the result rows.**
`IncidentSearchForm.tsx:349-390` renders, per row, Planned/Real-time
(filled blue/orange), Active/Cleared (filled green/grey), one grape outline
badge per operator code, up to N blue outline line badges plus "+N more",
and one grey outline badge per station code. On an operator-wide incident
that is 8–10 pills in four colours, of which only Active/Cleared answers a
commuter's question — the exact pattern F11 called out on station pages,
and the impact-type spec's own Open Question 5. No screenshot exists (§6).
**Capture the results state before shipping further**, then likely: keep
only Active/Cleared filled, render operators as names, fold Planned/
Real-time into a prefix, and move station codes to the detail page.

### 3.4 Lines — `/lines`, `/lines/[id]`, `/lines/[id]/edit`, `/lines/[id]/history`, `/lines/new`

**serious — the Status column is blank for ~120 of ~125 rows.** On the
"All Lines" page of a rail-status product, only c2c and LNER ECML carry a
badge; every other row has an empty Status cell and "—" in both numeric
columns, on every viewport and both schemes. `AllLinesTable.tsx:318`
renders `null` when there is no worst status. A user cannot distinguish
"good service, nothing to report" from "we have no data for this line" from
"not computed yet" — three states the coverage spec explicitly set out to
disambiguate *for the numeric columns*, while Status got no equivalent.
Sorting by Status is meaningless when 96 % of cells are null. The mobile
sub-line ("No sample data") partially rescues phones; desktop gives nothing
at all. *Fixture caveat:* the fixture has only two line reports, so most
production rows will carry real statuses — but the empty-cell path is what
every line hits between aggregator runs, on a fresh deploy, and for the
~70–95 catalogue lines the coverage-gap analysis says do not exist yet.
**Recommend:** never render an empty Status cell. Render a grey outline
"NO DATA" badge driven by the same `sampleUnavailableReason` helper the
numeric columns already use, with the reason as its tooltip — grey, so the
severity palette (a stated non-goal) is untouched, and sort-by-status gains
a defined bucket.

**moderate — 125 rows, no text filter, catalogue ordering.** The mobile
capture is 27,279px tall (≈70 screens); desktop is 5,094px. The only filter
is a "Filter by operator" select, and names are in catalogue order, so
"Grand Central" sits between "EMR Rural Branches" and "Great Northern" and
the four CrossCountry lines are split between the top and bottom of the
table. Once you scroll past row 3 on mobile the filter is gone.
**Recommend:** a type-ahead "Find a line" input beside the operator filter
(matching the existing Station Lookup pattern) and a default sort by name —
or grouping by operator with sticky headers, which the operator filter
already implies is the mental model. A sticky filter bar on mobile.

**moderate — neither custom-line form says what a custom line is.**
`/lines/new` is an `<h1>`, then Name / Operators / Add station, and nothing
else. A user arriving from the "New custom line" link has no idea this
creates a personal route that then receives the same live status
computation as catalogue lines, that it is private to them, or that it will
appear in their All Lines table. `/lines/[id]/edit` shares the component
and the gap, and has no "private" note either even though privacy is now a
hard property of every custom line. **Recommend:** one dimmed paragraph
under the `<h1>`, shared by both pages: what it is, how it works, who can
see it. *(The operators-field and ATOC-placeholder half of this cluster is
§2.9.)*

**moderate — the station list has no empty state and no indication that
order matters.** Below "Add station" there is nothing until a station is
added; the edit page shows chips in insertion order (`KGX × EUS ×`). The
domain model is an *ordered* list from one end of the route to the other
(DESIGN.md §5.1), but the UI never says so and offers no reordering.
**Recommend:** "No stations yet — add at least two, in travel order." under
the input, and number the chips (1 KGX, 2 EUS) or render them as a vertical
ordered list with drag handles. If order genuinely does not affect matching
for custom lines, say "order doesn't matter" instead — either way, say
something. No field is marked required, yet "Name is required." is a
validation error (`CustomLineForm.tsx:99`); add `withAsterisk`.

**minor — the line detail page's two empty-state boxes say almost the same
thing.** "Not enough sampled data yet for this line." immediately followed
by "Not enough full-coverage data yet for this line." with no heading
between them. The half-hourly-coverage spec intended a "Full coverage"
`h3` above the second block and it is not visible in the capture — worth
confirming, since the accessibility fix plan depends on it for heading
order. **Recommend:** when both series are empty, render one box ("Not
enough data yet for this line — trends appear once live departures have
been sampled for a few hours."); when only one is, show that section's
heading so the sampled-versus-full-coverage difference is explained.

**minor — mobile header stacking orphans the ⓘ icon** on a row by itself
between the two-line `h1` and the share/badge row, where it reads as a
stray glyph. Put ⓘ and share in one right-aligned group on the badge's row.

**minor — sort headers show `↕` on every column, never the active
direction.** Name, Status, Avg Delay and Cancelled all show the same
bidirectional glyph regardless of the current sort, so sighted users cannot
see which column is sorted or which way. Show `↑`/`↓` on the active column
and set `aria-sort` on its `<th>`. ("Avg Delay ↕" also wraps its glyph onto
a second line at 1440 — `white-space: nowrap` on the header group.)

**minor — wrapped row names on mobile read as two separate items.** The
second line of "South Wales Main / Line" sits ~1.6× line-height below the
first while the sub-line "No sample data" sits tight beneath it. `lh={1.3}`
on the name link, letting the `Stack gap="xs"` provide the separation.

**minor — timeline copy on the history page.** The Good Service row reads
`GOOD SERVICE  Good Service  21:34`: the badge and the reason say the same
thing. When reason text equals the severity label, render "No incidents
reported" or omit it. *(The "recomputes" and zone halves are §2.9 and
§2.12.)*

**minor — the anonymous 404 on `/lines/[id]/edit` offers "Back to your
dashboard".** An anonymous visitor has no dashboard; the link goes to `/`.
The copy in `app/lines/[id]/not-found.tsx` was written for the stale-bookmark
case, which is a logged-in scenario. "Go to the home page" is neutral for
both. The private-lines spec also notes that an owner whose session expired
lands here with no way back in — a third "Log in" link, rendered for
anonymous only, closes that without leaking the line's existence.

**minor — there is no Delete on the edit page.** The edit spec deliberately
put Delete on the detail page, which is defensible, but a user who
navigated here to "manage" the line has to go back to find it. A secondary
"Delete line…" text link at the foot of the form, opening the same
confirmation modal.

**minor — dark mode loses "Show advanced options" as a link.** Light:
grape-8 at 7.07:1, obviously interactive. Dark: near-white (248,240,252),
indistinguishable from body text. Use grape-4 (5.70:1, the nav-link
colour). Per the 2026-09-02 review's open question, use the same affordance
class for both the "Show" and "Hide" states.

**minor — anonymous users see 125 pin stars and "New custom line" with no
hint that either needs an account.** The spec chose this deliberately
("Tier-2 public entry, gated completion") so anonymous visitors discover the
features — keep the behaviour, but a `title` on the star ("Pin — needs an
account") means the first click is not a surprise. Relatedly, a user who
fills in five stations and then hits the login wall may lose the work: show
a one-line `Alert` ("You'll be asked to log in when you save — your entries
are kept.") and make sure the form state actually survives the OIDC
round-trip, so the promise is true.

**nitpick — "Recent trends (last 24 hours)"** over-promises while the empty
state is showing, since the rolling window includes the in-progress hour and
may hold far less than 24h early in a line's life.

### 3.5 Stations — `/stations`, `/stations/[crs]`

The station detail page is where the "render by shape, not as JSON" effort
landed, and the sweep confirms that effort paid off (§5). These findings are
about what it has not yet reached. Almost all of them were *predicted* by
`2026-09-17-accessibility-section-ux-review.md` from a text facsimile; this
sweep confirms them with pixels, so they should now be treated as confirmed
rather than as proposals. Recommendations below cite that review's P-numbers.

**serious — the page is named for disruptions and is 85 % accessibility
content, with no way to jump to it.** The `<h1>` is "Disruptions at London
Kings Cross (KGX)". On mobile the "Accessibility & facilities" heading is at
~1,120 CSS px and the section then runs to the footer at ~6,750 — 83 % of
the page; on desktop it starts at y=703 and runs to 5,180, 87 %. Before
reaching it a reader scrolls past a one-incident list, two rows of filter
chips (§2.14), a segmented control, a "Sample stats" section that says it
has no data, and a collapsed timetable. There is no in-page navigation, no
anchor, and the section heading is visually the same weight as "Sample stats
by operator". For a wheelchair user or a blind traveller who opened this
page to answer "is it step-free and how do I get help", the page's own title
says they are in the wrong place. **Recommend (B-1):** rename the `<h1>` to
"London Kings Cross (KGX)" — `generateMetadata`'s title already uses that
form; add a compact "On this page" jump row (Disruptions · Departures ·
Stats · Accessibility & facilities); give each section an `id` so
`/stations/KGX#accessibility` is linkable from elsewhere in the app. This is
cheaper than the 09-17 review's tabs proposal and gets most of the benefit.
*(09-12 Decision 8 put accessibility last as a fourth section. Keep the
placement; the page's **framing** is what does not survive contact with the
real payload.)*

**serious (a11y) — one `h2` for a 4,500px section.** The four group titles
("Step-free access & assistance", "Facilities", "Platform & station
facilities", "Getting here") and the twelve key labels are bold `<p>`s
(`StationAccessibilitySection.tsx:487,496`). What a screen-reader user gets
instead is nine `role="region"` landmarks named "Passenger assistance: 3
items", "Locations: 13 items", "Lifts info: 9 items" — made unique for axe,
but not a substitute for structure. **Recommend (B-2):** promote the four
group titles to `<Title order={3} size="sm">` and, if the twelve keys keep
their own labels, `order={4}`, holding the visual size via `size`. Add
`getByRole('heading', { level: 3 })` assertions. *(**This review disagrees
with the plan's "no new headings" global constraint.** It was the right call
to keep the just-merged axe sweep green during the rendering rewrite, and it
is the wrong call to keep: an `h2` → `h3` step is exactly what
`heading-order` exists to encourage, and a 4,500px section with one heading
fails the people the section is for.)*

**serious — every group leads with whatever the feed serialised first, and
the single most important fact is buried.** The first group renders, in
order: "Station accessibility" (a bare label with nothing under it),
"Induction loop", "Passenger assistance ▸ 3 items", and only *then*
"Step free category / Category: A, Compliant step-free access to all
platform(s)". Under "Lifts — Available" the "9 items" disclosure comes
before the sentence "There are lifts"; under "Car parks" the totals come
*after* the "2 items" disclosure; "Platforms ▸ 11 items" precedes "There are
tactile warnings on all platforms in use". On mobile the Category A line is
a full screen below the section heading. **Recommend (P1.1 + P1.2):** a
stable kind-sort — sentences and booleans, then facilities, then tokens,
then times and contacts, then collections — which is shape-only and needs no
key knowledge; then an at-a-glance strip (step-free category, assistance
hours and phone, accessible toilet / Changing Places, lift count, Blue Badge
bays), which is the one place key-name knowledge is justified. Do the strip
*after* the sort and the de-duplication below.

**serious — about a third of the section is printed twice.** "Help points —
Available" plus three paragraphs, "Staff help — Available", "Induction loops
are only in Help Points", "Announcements are made both visually and audibly",
"There are tactile warnings on all platforms in use", the Departure/
Announcements/Arrival chips and the "Yes - from …" chips all appear once at
y≈800–1,700 and again at y≈3,170–4,030 in
`stations-crs/default__desktop-1440x900__chromium.png`. The second copies sit
in the *third* group, 2,400px below the first, under "Help and support"; the
consecutive pair at y≈3,770/3,790 is the same sentence twice, once indented
and once not, which reads as a rendering fault. **Recommend (P2):** merge
`staffAssistance` and `helpAndSupport` into one group and de-duplicate
deep-equal nodes across the section before rendering. Both shape-only.

**moderate — the disclosure controls say nothing and look like dividers.**
Nine collections, each a control reading "3 items" / "13 items" / "9 items" /
"1 item". Each is ~50px tall with the chevron indented ~48px, sits on a
full-width rule that runs *under* the surrounding block's indentation, and
has more vertical padding than any heading on the page. The question a
reader actually has — *which lift serves platform 8*, *which toilet has a
Changing Places bench* — is behind a label that does not name the thing.
**Recommend (B-6):** show the qualifier on screen, not only in `aria-label`
("13 toilet locations", "9 lifts", "11 platforms"); inline any collection of
≤3 items (six of the nine here); strip the accordion chrome so the control
sits in the text column at its label's indent; never render "1 item" as an
accordion. *(09-16 §4.5 reused `Disclosure` unchanged, justified by axe
coverage. Keep the mechanism; the chrome and the "N items" label should go.)*

**moderate — booleans speak two languages and the important ones get the
quiet one.** "✓ Showers — Available" and "✓ Toilets — Available" get a glyph
and 500-weight, while directly beneath, "Accessible toilets available: Yes",
"Baby changing available: Yes" and "Changing places toilets available: Yes"
are plain label/value rows. "Sheltered waiting available: Yes" sits between
"✗ Seating area — Not available" and "✓ Waiting facility — Available". The
glyphs are 14px monochrome strokes; in the transport-links run the two
crosses and four ticks are indistinguishable at a scan. **Recommend (P4):**
one boolean rendering for `available` and its sibling booleans; colour the
glyph (green-7/red-7 light, green-4/red-4 dark, exactly as `StatusBadge`
already resolves) while keeping the words; fold "Not available" facilities
into one dimmed line per group.

**moderate — rich-text rhythm is uneven against plain lines.** The three
paragraphs of the step-free note are separated by ~20px each and the plain
sentence that follows sits 2px below the last one; under "Helpline —
Available" the number floats ~20px above "Contact". Predicted by the 09-17
review as "[not visually verified]"; real at every width. **Recommend:**
`[data-rich-text] p { margin-bottom: var(--mantine-spacing-xs) }`,
`[data-rich-text] :last-child { margin-bottom: 0 }`, and raise the
label/value `Stack gap` from 2 to 4–6px.

**moderate — phone numbers inside prose are not tappable, and the same
number appears three ways within 100px.** Under "Helpline — Available":
`0800 022 3720` as plain text, then "Contact / Phone: 0800 022 3720" as a
`tel:` link, then "…you can call 0800 022 3720…" as plain text. On the
device this page is most likely to be read on, two of the three copies of
the assistance number cannot be tapped. **Recommend (P6):** linkify UK phone
patterns in text nodes of the sanitizer's DOM output, and when a facility's
free text is exactly the contact's `primaryTelephoneNumber`, drop the
duplicate.

**moderate — raw URLs as link text, and `N/A` presented as a fact.** "More
details / at https://www.nationalrail.co.uk/posters/KGX.pdf" — Chromium
mobile cannot break inside the URL so it pushes "at" onto its own line and
the URL fills the column; Firefox breaks after the slash instead. Nothing
overflows, which is the important thing, but a 45-character URL is not a
link label. Separately, "Underground — Available" is followed by the
sentence `N/A`. **Recommend:** when an anchor's text equals its `href`,
replace the text with the host ("nationalrail.co.uk ↗") and keep the full
URL in `title`; set `overflow-wrap: anywhere` on `[data-rich-text] a`
regardless; and treat `N/A`, `-`, `.` and empty-after-trim as empty in
`isEmptyRenderable` — they are feed placeholders, not information.

**moderate — prose runs the full 1,100px container.** The "Shops" note and
the "Customer help points are available at the platforms and at the taxi
rank…" line run ~150 characters per line, identical at 1920, while
everything else in the section is 20–60 characters wide, so the eye loses
the line on the few long ones. `max-width: 70ch` on the section's text
column; chips and tick lines are unaffected.

**moderate in aggregate — label defects.** "Car parks" printed twice in a
row (the group entry label, then the `carParks.carParks` key). "Atm", "Cctv
available", "Cctv", "Wifi" — a five-word acronym map fixes all four. "Drop
off pick up — Available" is the only top-level key in "Getting here"
rendered as a tick line rather than a label, purely because that object
happens to carry `available`. "Station accessibility" is a bold label with
nothing under it — the first line of the whole section is an empty heading.
"Location: Next to Waitrose" is bold-inline while every other "Location" is
a block label over an indented value. "Step free category" then "Category:"
says "category" twice for one value. "Names" labels the ticket-barrier chips
and "Lifts info" labels the lifts; neither is a word a traveller would use.

**minor — the "Search a different day or filter →" link floats outside the
accordion it belongs to**, separated by the control's own bottom rule, so it
reads as a page-level action unrelated to the timetable. Render it inside
the panel after the rows, or right-align it in the control row.

**minor — `/stations` is empty below the form.** At every width the form
ends ~250px into the content area and the rest is blank — ~1,300px of
nothing on tablet. Below it, show pinned stations (when logged in), recently
viewed stations (localStorage), or a handful of major termini as plain
links. It costs nothing, makes repeat lookups one tap, and gives the app's
most common entry point a reason to exist beyond the input.

**minor, unverified — the "select a suggestion, then press Look up"
two-step.** The 2026-09-02 review's F6 recommended navigating on suggestion
select; the static captures cannot show whether that landed, and the
disabled "Look up" button is still present at rest. Worth a five-second
check in the running app.

### 3.6 Track and Train — `/track`, `/track/mine`, `/track/mine/add-ticket`, `/train/[uid]/[date]`, `/train/by-id/[trackingId]`, `/trains`

**serious — the train page tells four different stories about the same
train at once.** In one viewport: a yellow alert "May have arrived"; "Last
reported: York (departure)", "Delay: 4M LATE", "Next calling point:
Edinburgh Waverley, ETA 14:34"; a progress diagram with **no** marker; and
the caption "Matched to train W12345 — waiting for its first movement
report." The caption is flatly contradicted by the summary above it: a
movement report *has* arrived. `components/JourneyProgress.tsx:206-221`
falls through to the `awaiting_activation` wording whenever
`lastReachedIndex` is −1, even when `status === 'en_route'`, and the code
comment explicitly conflates the two. Here nothing was confirmed *against
the timetable*, which is a different situation from nothing having been
confirmed *at all* — and real data will produce the same split whenever a
movement is reported at a location that is not in the schedule (junctions,
passing points, a missing tiploc). The page's single job is to say where the
train is, and it disagrees with itself. **Recommend:** add a branch for
`lastIndex === -1 && lastReportedLocation != null` — "Last reported at York
— that report couldn't be matched to a timetabled stop, so no position is
shown on the line." (and the same for the `aria-label`). **Keep the marker
rule exactly as it is**; only the copy is wrong. See also §4.5.

**serious — the manual ticket form has no placeholders, no helper text and
no required marks.** Four bare inputs: Operator, Ticket type, Origin CRS
code, Destination CRS code
(`components/TicketEntryForm.tsx:323-353` — labels only). On the same site,
`/track` and `/trains` say "e.g. Woking or WOK" and explain every field.
Here the user has to guess whether "Operator" wants "LNER" or "GR", whether
"Ticket type" is free text or an enum, and what a CRS code even is — the
picker-refactor spec removed "CRS code" from `/track`'s labels precisely
because the field accepts names too, and this form kept it. Nothing marks
which fields are mandatory. This is the low-friction fallback for the two
upload paths, and it is currently the highest-friction form on the slice.
**Recommend:** reuse `/track`'s station `Autocomplete` for origin and
destination, labelled "Origin station" / "Destination station"; give
Operator the same treatment as §2.9; give Ticket type a placeholder ("e.g.
Off-Peak Single") or a `Select` if the backend has an enum; mark required
fields with `withAsterisk` and optional ones with "(optional)" text, as the
sibling forms do.

**moderate — "Unknown location" on every timetable row and both diagram
labels, including the origin and terminus the header names two lines
above.** `journeyStopLabel` falls back `name → crs → 'Unknown location'`
(`components/JourneyTimeline.tsx:67-69`) and the fixture's stops have
neither. The timetable is 100 % placeholders — worse than not showing it —
and the diagram labels both ends of the journey "Unknown location", the
worst possible label for a "you are here" line. The overlay spec
deliberately hides the timetable for `pending`/`unresolved` states to avoid
"a confident-looking timetable UI" over guessed data; the same logic says a
timetable with zero resolvable names should degrade to a count ("13 stops —
station names unavailable") rather than a wall of placeholders.
**Recommend:** resolve `tiploc → crs → name` server-side before building
`journeyStops` and never drop a row for a missing tiploc — emit it with
`name: null` and let the client label it by index ("Stop 3"); seed the first
and last row labels from the pin's origin and destination; and if *every*
stop is unnamed, collapse the table to a single dimmed line. *(This is
fixture-amplified — see §4.2 — but the fallback strategy is the finding.)*

**moderate — a six-hour-stale ETA presented in the present tense, directly
under an alert saying the train may have arrived.** "Next calling point:
Edinburgh Waverley — ETA 14:34" is still framed as upcoming. `mayHaveArrived`
is computed server-side from the ETA, so the page *knows* the ETA has passed.
**Recommend:** when `mayHaveArrived` is true, render "Was due at Edinburgh
Waverley 14:34 (no arrival report received)" and drop the present-tense ETA
badge framing.

**moderate — the add-ticket page never says which train the ticket attaches
to, or that it does not need one yet.** The whole reason this page exists,
per the standalone-ticket spec, is that a ticket can be saved *before* a
tracked train exists and attached later. Nothing on screen says so; a
first-time user will wonder where "which journey?" went. One dimmed sentence
under the title: "Save the ticket now; you can attach it to a tracked train
afterwards, or we'll try to match it for you." The post-save Alert
reportedly does this, but the sweep never captured it (§6) and the promise
belongs before the form too.

**moderate — the upload paths are hidden behind secondary tabs, on the page
the upload-first plan created.** "Manual entry" is the default tab per spec;
the pkpass and PDF dropzones — the low-effort path, and the feature the
drag-and-drop spec invested in — are one click away and do not appear in any
capture. A user arriving with an e-ticket on their phone sees four empty text
boxes first. **This is a deliberate spec decision, so this is a
recommendation to revisit it, not a defect.** Either make the upload tab the
default (manual stays one tap away, and both upload paths already land the
user back on the pre-filled manual fields), or put a single compact dropzone
*above* the tabs so the choice is visible without a tab switch.

**minor — no "stop tracking" affordance on the list, and "Delete" is the
wrong word on the detail page.** The train card offers Rename; the ticket
offers Rename + Delete; the train itself can only be removed from its detail
page. Users will look for "remove" on the list. And on the detail page,
users do not *delete* trains, they stop tracking them — sitting next to "Add
to group", "Delete" also reads as though it might delete the group.
**Recommend:** an overflow kebab on the row with Rename / Stop tracking
(which also relieves §2.5's space contest), and relabel the detail-page
button "Stop tracking", keeping the red outline and the confirm modal.

**minor — the date and time are stated twice in the same card.** The default
display name is "London Kings Cross (KGX) → EDB, 17 Sept 2026 · 10:00" and
the dimmed line directly beneath is "17 Sept 2026 · 10:00"; on mobile that is
four lines of title followed by the same date again. The duplication is only
*needed* when a custom name replaces the default — print the dimmed `when`
line only when `train.customName` is set.

**minor — the train page's title is duplicated.** `<h1>` "Train W12345" and,
immediately below in body weight, "Train W12345" again. On the owner view the
second line is the custom-name slot and on the public view the
headcode/uid, but with the fixture they are byte-identical, so it reads as a
rendering bug. Suppress the subtitle when it equals the `h1` — or make the
`h1` the route ("London Kings Cross → Edinburgh Waverley") and the subtitle
the uid, which is what users actually search by.

**minor — `/train/by-id/[trackingId]`'s anonymous title is "Tracking Train
1"** — the internal subscription id, meaningless to someone who followed a
shared link, and different from what they will see after logging in ("Train
W12345"). The 2026-09-02 review's F13 already asked for this. "Tracked
train" as the `h1`, or "Someone's tracked train — log in to see it" if the
route cannot disclose anything pre-auth. Relatedly, nothing on the owner page
says "this is your private link; share the canonical one instead" — if Share
already copies the canonical URL, say so in its tooltip.

**minor — the Delay Repay disclaimer appears three times across two adjacent
pages, ~120 words per page.** `/track/mine`'s card-level "Your reliability"
block ends with a four-sentence disclaimer, the ticket block repeats a
near-identical three-sentence one, and the owner train page carries a third —
all before the single actionable link. The tickets-list spec consciously
accepts "never collapsed" and the 2026-09-02 review graded this real but
minor; the *card-level* one is the new addition, and it now makes the page
read as mostly legal text. Keep one full disclaimer at the card level and
reduce the per-ticket one to "This app never submits a claim on your behalf"
plus the link. (`--` should be an em dash in `DelayRepayEstimate`.)

**minor — dark-mode alert and badge weighting on the train page.** The "May
have arrived" alert becomes a solid saturated brown block with white text,
far heavier than the pale-yellow light-mode tint, and the "4M LATE" badge is
orange on dark-red-brown at roughly 3.5:1 — readable for the alert,
borderline for 11px uppercase text. Use the
`--mantine-color-yellow-light` / `-light-color` tokens so `variant="light"`
semantics survive into dark, and bump the badge to `size="sm"` or drop the
uppercase so the contrast budget is easier.

**minor — `/trains`' time inputs render the 12-hour "--:-- --" skeleton.**
`TimeFilterInput` uses a native `<input type="time">`, whose format follows
the *browser's* locale — the sweep browser is en-US, so a UK rail site where
every displayed time is 24h shows AM/PM, and the empty state gives no hint of
the expected format. Mantine `TimeInput` with explicit 24h, or a text input
with `placeholder="HH:MM"` and `inputMode="numeric"` (which is what the
multi-day spec describes anyway).

**minor — `/track`'s disabled "Track this train" is near-invisible in dark
mode** — light grey on slightly-lighter grey against `dark-7`, well under
2:1. WCAG exempts disabled controls, but this is the *only* action on the
page and a user cannot tell whether it is a button that will enable or
decoration. Use `variant="light"` or a bordered outline so the shape
survives, or do not disable it at all and let submit trigger the field-error
`Alert` the picker-refactor spec already provides.

**minor — the add-ticket tab strip wraps at 390px** with "Upload PDF
e-ticket" alone on row two, and the active-tab underline on row one runs
under only "Manual entry" while the separator under row two spans the full
width, so the two rows read as two different components. Shorter labels
("Manual", ".pkpass", "PDF") or a `SegmentedControl` with `fullWidth`.

**nitpick — "Your reliability" says "check back once it's finished running"
while the only train is marked EN ROUTE and its ticket already has a
delay-based estimate.** The card's headline promise is empty while its
sub-panel is populated. Ordering, not correctness.

**nitpick — two adjacent `/trains` fields share the placeholder "e.g.
Reading or RDG"** (Station and Stops at). Harmless, but a different example
for "Stops at" would reinforce that they are different questions, which the
description text is working hard to explain.

---

## 4. Correctness bugs found incidentally

These are not UX findings. They are things that look like they may simply be
*wrong*, and they should be triaged separately from the design feedback
above.

### 4.1 · serious (needs live verification) · A one-hour offset between the train page's header and its own timetable

`train-uid-date/default__desktop-1440x900__chromium.png` shows, for a single
train, in a single viewport:

| Value | Header / summary | Timetable row |
|---|---|---|
| Origin departure | 17 Sept 2026 · **10:00** | **09:00** |
| Terminus | ETA **14:34** | est. **13:34** |

The seed (`.devdata/seed.sql:84`) stores
`pin_scheduled_departure = '2026-09-17T09:00:00Z'` and
`eta_next = '2026-09-17T13:34:00Z'`, and the `calling_points` blob stores the
**same instants** (`plannedDeparture: "2026-09-17T09:00:00Z"`,
`plannedArrival: "2026-09-17T13:30:00Z"`). London was UTC+1 on 2026-09-17.
So the header is rendering those instants **correctly** in Europe/London,
and the timetable is rendering the identical instants as raw UTC clock
values — a clean one-hour error, the signature of a BST/UTC mismatch.

`lib/dateFormat.ts` pins `timeZone: 'Europe/London'` for both call sites, so
the divergence is not in the formatter: the *values* reaching
`JourneyTimeline` have already lost or gained the zone somewhere in the
API → wire → parse path. Static analysis cannot say which layer, and §4.2
means this particular fixture is not a trustworthy witness.

**A one-hour error on a rail timetable is worse than no timetable**, so this
must be resolved before the timetable overlay reaches real users.
**Recommend:** (a) reproduce against real, non-fixture data first; (b) add a
backend unit test asserting
`calling_points[0].scheduled_departure == pin_scheduled_departure` for a
known train; (c) add a frontend test that renders one instant through both
the header and the table and asserts the two strings are equal.

### 4.2 · serious (fixture) · The train fixture's `calling_points` blob does not match the wire shape the API deserializes

**New to this consolidation.** The Track/Train reviewer was told the fixture
has "a `calling_points` row missing a `tiploc`". The mismatch is larger than
that. `crates/api/src/data/journey.rs:23-40` deserializes each calling point
as:

```rust
struct RawCallingPoint { tiploc: String, kind: CallingPointKind,
                         booked_arrival: Option<NaiveTime>,
                         booked_departure: Option<NaiveTime>,
                         #[serde(default)] day_offset: u8 }   // camelCase wire
```

— i.e. `tiploc` / `kind` / `bookedArrival` / `bookedDeparture` / `dayOffset`,
where the two times are **naive London wall-clock** values that
`london_to_utc` then converts. `.devdata/seed.sql:85` writes instead:

```json
{"crs":"KGX","name":"London Kings Cross",
 "plannedArrival":null,"plannedDeparture":"2026-09-17T09:00:00Z"}
```

No `tiploc`, no `kind`, no `bookedArrival`/`bookedDeparture`, and full
zoned instants where naive times are expected.

**Consequences.** This is why the server logs `could not build journey
stops` (`routes/train.rs:966`), why every timetable row and both diagram
labels read "Unknown location" (§3.6), and it is the most likely
contributor to §4.1. More importantly, **it means the timetable-overlay and
progress-visualization features were never actually exercised by this
sweep** — the reviewed screenshots are of a degraded fallback path, not of
the feature.

**Recommend:** rewrite the seed's `calling_points` to the real
`ScheduleCallingPointDto` shape, with the TIPLOCs needed for the
`stanox_crs` join, and add a seed test that asserts
`build_journey_stops` returns `Some` for the fixture train. Then re-capture
the two train routes and re-examine §3.6 and §4.1 against a fixture that
represents production.

### 4.3 · serious (fixture) · RSC de-duplication references rendered as station information

Three `notes` values on `/stations/KGX` render as the literal strings `$34`,
`$35` and `$36`, one of them preceded by a line containing only `.` —
visible under "Staff help — Available" (desktop y≈1,730–1,760), "First class
— Available" (y≈2,440) and the second "Staff help" (y≈3,900), identically in
dark mode, at tablet and mobile widths, and in Firefox. The renderer is doing
the right thing with the data it was given: these are React Server Component
de-duplication references left over from scraping a production page's flight
payload, exactly the artifact
`2026-09-16-structured-accessibility-rendering-design.md` §1.3 warned about.

**The scope is larger than the Stations review reported.** It named nine
files; a grep for `/^\$[0-9a-f]{1,3}$/` across
`frontend/test/fixtures/accessibility/` matches **11 of 31**: `BAL` (3),
`BHM` (2), `BTN` (4), `EDB` (1), `EUS` (4), `HUL` (1), `KGX` (3), `LDS` (1),
`MAN` (1), `STP` (1), `WVH` (1) — `EUS` and `EDB` were missed. `.devdata/seed.sql`
carries `$34`–`$38` and `$e`.

**Why it matters beyond the screenshot:** the "no raw node across 31 real
payloads" regression test and every other fixture-driven assertion run on
strings that are not real feed values. The tests still prove the
classifier's *shape* logic, but the fixtures cannot be trusted as "what the
feed says".

**Recommend:** (1) re-capture the eleven affected fixtures and the seed from
`GET /public/stations/{crs}/accessibility` directly, or from the database —
not from the page; (2) add a fixture lint asserting no string value matches
`/^\$[0-9a-f]{1,3}$/`; (3) independently of the artifact, treat a scalar
that is only punctuation (`.`, `-`, `N/A`) as empty in `isEmptyRenderable`
— see §3.5, where `N/A` is real feed junk rather than a scraping artifact.

### 4.4 · moderate · Whitespace dropped after an interpolated number in the two sentences that carry the money disclaimer

`/track/mine` renders "**0may have qualified**" and "**(4minutes)**" at every
viewport (`track-mine/authenticated__desktop-1440x900__chromium.png`,
`…__tablet-ipad-mini__chromium.png`). The source *does* contain the space —
`components/ReliabilityDigest.tsx:158` (`{rollup.eligibleCount} may have`)
and `components/DelayRepayEstimate.tsx:57` (`({delayMinutes} minutes)`) —
and neither file has changed since 2026-09-15 / 2026-08-30, so the deployed
build is dropping whitespace that follows a JSX interpolation. **Recommend:**
make the space explicit (`{count}{' '}may`), add a render test asserting the
exact string, and separately find out *why* the build eats it — confirm the
running container is on current `main`, and if it is, this is a toolchain
issue that deserves its own ticket, because it will recur anywhere the
pattern appears.

### 4.5 · moderate · The progress caption is selected by the wrong condition

Cross-referenced from §3.6. `JourneyProgress.tsx:206-221` selects the
`awaiting_activation` copy whenever `lastReachedIndex === -1`, regardless of
`status`, and its own comment records the conflation ("both mean … nothing
has been confirmed"). That is a state-machine bug, not a wording preference:
"nothing confirmed against the timetable" and "nothing confirmed at all" are
different states and the page asserts the second when only the first is
true. It will reproduce on real data at junctions, passing points and any
stop whose tiploc does not join.

### 4.6 · moderate · An existing member is offered "Join group"

Cross-referenced from §3.2. `/groups/join/[token]` only calls the
unauthenticated preview, so it cannot know the viewer is already a member —
and the person most likely to open an invite link is the one who created it.
Worth fixing on both sides: probe membership when authenticated, and make
`JoinGroupButton` treat a 409/already-member response as success rather than
as an error.

### 4.7 · moderate (needs verification) · Two Suspense boundaries never resolved in mobile Chromium

`lines-id/default__mobile-iphone14__chromium.png` and its `dark-mode`
sibling show the "Recent trends" section as two flat 280px grey skeletons
with no text; the page is 4,728px tall because of them. Every other capture
of the same URL — desktop, tablet, and **Firefox mobile** (3,411px tall) —
shows the two resolved "Not enough … data" boxes. Two readings: a timing
artefact (the full-page capture fired before the streamed boundaries flushed
on slower mobile emulation), or a real bug where the streamed chunks do not
hydrate under mobile Chromium — a `matchMedia` or `ResizeObserver`
dependency in the chart wrapper, for instance. Both light and dark mobile
Chromium hit it and Firefox mobile did not, which leans towards the timing
explanation but does not settle it. **Reproduce manually in Chrome device
mode.** Regardless of cause, the unlabelled oversized skeleton is a real
finding (§2.11).

### 4.8 · minor (investigate) · An unhandled rejection on `/lines/[id]/edit` under a mocked network

`lines-id-edit/offline__desktop-1440x900__chromium.png`, `…api-error…` and
`…loading…` show the Next dev indicator carrying a red "1 Issue" badge; the
anonymous default capture does not. The page is the 404 template, so
something on it still makes a client call that throws when `/api/**` is
mocked — probably the session probe, or the `LineDefinitionTooltip`
`/definition` fetch, which the private-lines spec says is *meant to fail
silently*. A swallowed fetch should be `catch`ed, not left to bubble into an
unhandled rejection.

### 4.9 · minor (unexplained) · `LoginLink` resolves a different colour on one route

On `/lines`, `/lines/[id]` and `/lines/new` the nav's "Log in" is grape
(4.52:1 on the lavender ground). On `/lines/[id]/history` the same component
(`components/AuthStatus.tsx` → `LoginLink`) renders dark grey at 7.62:1
(`lines-id-history/default__desktop-1440x900__chromium.png`,
`…tablet-ipad-mini…`). Either colour passes AA; the inconsistency is the
issue, and an unexplained per-route colour resolution is worth understanding
before it surfaces somewhere that matters.

---

## 5. What is working well

Every reviewer was asked to report positives, and they are not incidental —
several are the direct, verified payoff of earlier planned work. These should
be protected against regression.

**Accessibility and contrast work landed, and it holds.** The
accessibility audit's worst finding (white on yellow at 1.86:1) is gone.
Severity badges are black on green-6 at **8.90:1** and black on yellow-6 at
**11.28:1** in light; black on green-8 at **6.10:1** and black on orange at
**8.46:1** in dark — the `autoContrast` / `luminanceThreshold: 0.179`
mechanism in `lib/theme.ts` is doing exactly what the plan said. The
Real-Time badge on `/incidents/[id]` is dark-on-orange, not the white-on-
orange 2.57:1 pairing the audit measured. Dimmed text was darkened to gray-7
(7.9:1). Grape-7 anchors clear AA on the lavender ground (4.68:1 light,
5.70:1 dark), with the measured numbers recorded in code comments and
covered by unit tests. Three slices independently confirmed dark mode is
coherent end to end with no light-mode leftovers and no contrast
regressions.

**The disconnect/reconnect design is delivered.** The offline notification
is bottom-centre with a spinner, plain honest copy ("Can't reach live data
right now — showing the last update."), `role="status"` + `aria-live="polite"`,
non-dismissable by design; it never blanks the page, never reflows content,
and correctly stacks *under* the login modal. Five slices verified it
independently. Its copy is shorter than the spec's and better for it. (The
form-page wording and the mobile width are §2.16.)

**The station page's "render by shape, not as JSON" rewrite paid off.**
There is no raw JSON anywhere on the page at any width or scheme — a week
ago 96 % of keys were `<Code>` blocks. Opening times read the way a human
writes them ("Mon–Fri, 05:00–01:36"), week-ordered and en-dashed. Contact
records are `tel:` links. Sanitized rich text keeps its structure and links
in the theme's anchor colour rather than browser blue. No horizontal
overflow anywhere in 6,750 CSS px of mobile page. Empty states are honest
and consistent across all four sections.

**The train pages honour their honesty rules.** The progress line shows *no*
"you are here" marker because no stop has a confirmed actual time — the
cardinal rule from the progress spec's Decisions 2 and 5. The timetable
prints "est." in italics and never a fabricated actual; the Delay column is
empty rather than guessed; the ETA carries its provenance badge; the "May
have arrived" alert explicitly says "this is an inference, not a confirmed
status from Network Rail". The timetable is a real `<table>` with an
`aria-label` inside a `TableScrollContainer`, the progress diagram is a
`role="group"` with a descriptive label, and its caption is always-visible
text rather than a tooltip. "View on Real Time Trains ↗" is a credible
external escape hatch for a product that admits its own data can lag.

**`/trains` is the best form in the app** and should be the model for the
others: every field has a label, a one-line description explaining what it
means *for the search*, an example placeholder in the "name or code"
pattern, and an explicit "(optional)" suffix; search is disabled until the
required field is filled *and* the page says why. The just-merged "Stops at"
loop-service behaviour is discoverable from the form copy itself.

**Destructive actions are handled with care.** Every one on `/groups/[id]`
has a confirm modal with a specific title ("Delete Fixture Family?",
"Remove Rider Fixture from this group?") and a red confirm button with an
explicit `aria-label`; additive actions are neutral `default` variant, so
red genuinely means destructive. Role-gated controls are computed with the
same predicates the backend uses.

**Privacy and non-disclosure are correct.** `/groups/[id]` does not leak the
group name to a non-member. Private custom lines 404 cleanly for non-owners
including on `/edit`, making "not logged in", "not the owner" and "doesn't
exist" indistinguishable — a security-correct outcome, well handled. The
join page's privacy sentence ("your other tracked trains and tickets stay
private") turns a hard constraint into user-facing reassurance at the moment
it matters, and confirm-before-join is never a silent auto-join.

**Copy is honest and specific where it counts.** "1 line not at Good Service
right now:", "Showing the first 5 — 7 more lines are not at Good Service",
"You haven't pinned any stations yet" — plain, quantified, every empty state
with a way out. The incident archive is honest about what its filters mean
(overlap not exact match; the priority caveat; matcher-attribution rather
than promised exactness), which is the right instinct for a data product.
`/connect-claude`'s authenticated page is the best-written page in the app.
"Group not found" matches the 404-not-403 convention without being evasive.

**The login modal pattern itself is good** — a fixed title, one
call-site-specific sentence, one primary action carrying `return_to`, an
accessible close with a visible focus ring. §2.4 is about *also* having
content behind it, not about replacing it.

**Mobile reflow is genuinely good in most places.** `/lines` folds its
numeric columns into a sub-line and keeps the pin star (the 2026-09-02
review's F4 is fixed). `/incidents` has no horizontal overflow at 390px and
its segmented controls compress without clipping labels. `/stations/[crs]`
manages 6,750 CSS px with no overflow. Add-ticket's convergence on a narrow
centred column looks finished at 1440 and 1920. Create and edit line forms
are one component and look it.

**Code-comment discipline is unusually strong and is worth protecting.**
The `ScrollArea` clipping comment in `IncidentSearchForm.tsx:268-289`, the
`tiploc_key` padding-trap comment, `LineStatusCard`'s and `IssueList`'s
shrink-rule comments, the `landmark-one-main` rationale in `layout.tsx`, and
`ThemeToggle`'s note on why the auto badge exists all explain *why* rather
than *what*. Several findings in this review were diagnosable in minutes
purely because of them.

---

## 6. Known coverage gaps — worth a targeted follow-up capture

The sweep is broad (874 images, every route, every state, five viewports,
two browsers) but it is a *static* sweep: it loads a URL and shoots. Nothing
is clicked, typed, expanded or submitted. As a result, the parts of the app
the specs spend most of their words on were largely not reviewed at all.
Collected here so one follow-up sweep can close them.

**Highest value — the sweep never captured a populated result set anywhere:**

1. **`/incidents` with results.** Every capture shows the empty "Press
   Search to browse incidents" state. The result rows — a summary link, a
   timestamp, Planned/Real-time, Active/Cleared, per-operator badges, line
   badges plus "+N more" and per-station badges — went entirely unreviewed,
   and the "badge soup" concern in §3.3 is therefore a prediction, not a
   finding. *Script: load the page, click Search.*
2. **`/trains` with results.** Row shape, per-row "Track this train",
   LDBWS-vs-CIF badge rules, "Load more", empty-results copy and the
   arrival-time pair that appears only when "Stops at" is filled — none of
   it captured. *Script: Station = KGX, submit; repeat with Stops at = YRK;
   repeat under the `api-error` mock.*
3. **`/track`'s departure picker with rows.** The picker-refactor spec
   defines six states ("Checking for departures…", "Couldn't load
   departures…", and so on) and the sweep captured none of them, because the
   picker only fetches once an origin is typed. *Script: type an origin.*
4. **`/lines/[id]/history`'s Trends tab.** Every history capture shows the
   Timeline tab. The rate chart, average-delay chart, legend, dash patterns,
   gap bands, the 30-min/1-h/6-h/daily `SegmentedControl` and its disabled
   tiers, chart reflow at 390px, and whether the shortfall banner and the
   disabled-preset tooltip landed — the whole 2026-08-31 / 09-02 / 09-05
   chart spec lineage — are unverified. The brief itself flags charts and
   history as the highest-risk area for mobile breakage. *Script:
   `?tab=trends` (or a click) at all three widths, light and dark.*
5. **`/stations/[crs]`'s "Scheduled departures" accordion.** Collapsed in
   every capture. It is the *only* client-fetching component on that route,
   so its loading text, its error `Alert`, its 404 copy and its
   empty-results copy — the four states the sweep's `loading` and
   `api-error` scenarios exist to capture — appear nowhere, which is why
   those two captures are byte-identical to each other. *Script: click the
   control before capturing `loading` and `api-error`.*

**States and identities the sweep did not reach:**

6. An **authenticated dark-mode** capture of `/groups/[id]` — the detail
   page's red outline buttons, dimmed attribution and role badges were never
   seen on a dark background.
7. An **expired or invalid invite token** on `/groups/join/[token]`. The
   source renders good, honest copy with a next step; with a 7-day expiry
   this is a common landing and it is unverified.
8. A **plain-member** and an **admin** view of `/groups/[id]`, plus a group
   with a long name and 6+ members. Every authenticated capture is the
   owner of a two-member group.
9. An **allow-listed chat user**, to see the working `/chat` UI and to
   confirm §3.1's suspicion that a first-time allowed user has no control
   that starts the MCP OAuth flow.
10. `/lines/[id]` **for a custom line** (Edit + red Delete, and the empty
    "Operators:" row), and the **delete confirmation modal**. Only the
    catalogue line was captured.
11. Either custom-line form with **"Show advanced options" expanded**, and
    the edit form's "Hide advanced options" state — the open question about
    the disclosure changing affordance class between states is still open.
12. An **expanded incident row** on `/lines/[id]` (sanitized-HTML links, the
    F7 blue-link issue).
13. `/track/mine/add-ticket`'s **upload-tab dropzone** and its **post-save
    "Ticket saved" alert** — the latter reportedly carries the "attach it to
    a train later" promise §3.6 asks for.
14. An **open autocomplete dropdown** anywhere in the app.

**Structural gaps a static sweep cannot close:**

15. **No hover or focus states anywhere** — tooltips on the "—" cells, focus
    rings on the pin stars, sort-header affordances, whether the `/lines/[id]`
    issue row is wholly clickable or only its 16px chevron is. Nobody should
    read "no focus-ring finding" as "focus rings are fine".
16. **Re-run against a production build** with the dev indicator disabled
    and with a settle step that waits for the streamed nav to resolve — see
    the caveats in §1. And re-capture the two train routes **after** §4.2 is
    fixed, since the current ones show a degraded fallback path rather than
    the feature.
