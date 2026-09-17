# Full-Service UX / Accessibility / Usability Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development
> (recommended, since most tasks below are independent and dispatchable in
> parallel) or superpowers:executing-plans to work this plan task-by-task.
> Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Phase 0 must be done first, and gates part of Phase 3.** Task 0.2 (fix
> the seed fixture's `calling_points` shape) is a precondition for trusting
> *any* further finding about the train timetable/progress UI — the review
> itself says the current screenshots of that feature are a degraded
> fallback path, not the real feature. Do not implement a UI fix for
> anything in §3.6's timetable/progress findings until Task 0.1/0.2 land and
> the train routes are re-captured (Phase 4, Task 4.3).
>
> **Phase 1 (cross-cutting) should land before Phase 3 (per-area).** Several
> per-area findings are symptoms of a Phase 1 root cause (nav wrap, `wrap="nowrap"`
> shrink guards, `TextLink` block-level rendering, the display-label layer).
> Fixing a per-area symptom first means re-touching the same file once Phase 1
> lands the shared fix — do the shared component once, first.
>
> **Phase 2 (fixture corruption) is fully independent** of everything else
> and can be done in parallel with Phase 0/1 by a different worker — nothing
> in this plan depends on it except Task 4.3's re-capture (which wants clean
> fixtures, not just a correct `calling_points` shape).
>
> **Within Phase 1 and Phase 3, tasks are independent of each other unless a
> "Depends on" / "Touches the same file as" note says otherwise** — see each
> task's Files list before parallelizing two of them.

**Goal:** turn `docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md`
(16 cross-cutting findings, ~90 per-area findings, 9 correctness bugs, 1
fixture-corruption finding, 16 coverage gaps) into landed fixes, without
conflating four different kinds of work the review itself keeps separate:
suspect *correctness* bugs that need live-data verification before a fix is
even chosen (§4.1/§4.2), a test-fixture data-quality bug that has nothing to
do with application code (§4.3), high-leverage *cross-cutting* UI patterns
that each resolve findings across 4–6 route categories at once (§2), and
genuinely *page-specific* findings (§3). A fifth category — coverage gaps
(§6) — is not a fix at all; it is a follow-up capture task, kept as its own
phase rather than silently dropped.

**The design specs remain non-binding**, exactly as the review states in its
own intro. Five places name a spec whose stated intent this plan
deliberately does not restore: the incident archive's "press Search first"
precedent (§3.3, Task 3.3.2), the station page's "no new headings"
constraint (§3.5, Task 3.5.2), the "reuse `ShareButton` verbatim" instruction
for the group invite link (§3.2, Task 3.2.2), the add-ticket page's
upload-tab default (§3.6, Task 3.6.6 — flagged as a design decision, not a
settled deviation), and the incident detail page's Decision 6 keeping
`operators`/`isCleared` off the page (§3.3, Task 3.3.1). Where a task departs
from a spec, its entry says so and cites the spec; nowhere in this plan
should a departure be described as "restoring compliance" or reverted back
toward the spec.

**Architecture:** almost entirely frontend (`frontend/app`, `frontend/components`,
`frontend/lib`); two tasks touch `crates/api` (Task 0.2's seed/fixture shape
fix touches `.devdata/seed.sql` only, not Rust source, but Task 0.2 also adds
a backend seed-shape test per the review's own recommendation in §4.1). No
new backend features. Several Phase 1 tasks introduce one new shared
component each (a status-row primitive for §2.5, a mobile nav drawer for
§2.2/§2.7, a shared display-label layer for §2.9) that Phase 3 tasks then
adopt rather than re-solving locally — that adoption is called out per task.

**Tech Stack:** Next.js 16 App Router + TypeScript + Mantine v9.5.2
(`@mantine/core`, `@mantine/charts`), Vitest 2 + `@testing-library/react` via
`frontend/test/render.tsx`'s `renderWithMantine`, Playwright 1.62
(`frontend/e2e/`, `E2E_BASE_URL`-driven), and `frontend/e2e/screenshot-sweep/`
— the same tool that produced the 874-screenshot sweep this review is based
on, reused narrower in Phase 4 (`node run.ts --routes=... --states=...`, see
its `README.md`).

**Specs:**
- `docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md`
  — the review this plan implements. §2 is cross-cutting findings, §3 is
  per-area, §4 is correctness bugs, §5 is "protect this, don't regress it",
  §6 is coverage gaps. Section/finding numbers (e.g. "§2.3", "§4.2") are
  cited throughout this plan and mean *that* document's headings.
- `docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md`
  and `docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md` —
  the previous accessibility pass. **Already landed and verified working**
  (review §5): the `<main>` landmark, `autoContrast`/`luminanceThreshold: 0.179`,
  the grape-7 filled-surface override, and gray-7 dimmed text are all live in
  `frontend/app/layout.tsx` / `frontend/lib/theme.ts` / `frontend/app/globals.css`
  today. Nothing in this plan should re-touch that mechanism except Task 1.1,
  which layers a *width* fix onto the *same* `<main>` `Container` the
  previous plan already landmarked — read that task's note before editing.
- Route-specific specs the review names as places it recommends departing
  from: `2026-08-31-station-catalogue-completeness-research.md`-adjacent
  `2026-09-12-station-accessibility-design.md` (§3.5's "no new headings"),
  `2026-09-12-incident-archive-design.md` (§3.3's "press Search first"),
  `2026-09-11-shared-groups.md` / `2026-09-15-custom-line-group-sharing.md`
  (§3.2's "reuse ShareButton verbatim"), `2026-09-02-standalone-ticket-entry-page-design.md`
  (§3.6's upload-tab default), `2026-08-31-incident-detail-page-design.md`
  (§3.3's Decision 6).

---

## Global Constraints

- **No change to `frontend/lib/severity.ts`'s `GROUP_COLOR` hue map.** The
  grape-theme spec's Non-goal (still binding per the previous plan) says the
  five severity hues carry meaning users already read at a glance. Nothing
  in this plan needs a hue change — §2.9's display-label layer changes *text
  labels*, not colours.
- **Every §5 "working well" item is a regression bar, not a suggestion.**
  Before landing any Phase 1/3 task that touches a file `StatusBadge`,
  `ConnectivityMonitor`, `JourneyProgress`, `JourneyTimeline`, `StationAccessibilitySection`,
  or `TrainSearchForm` also touches, re-read review §5's paragraph naming that
  component and re-verify the property it credits still holds (e.g. "the
  progress line shows *no* marker when nothing is confirmed" must survive
  Task 3.6.1's fix to the *caption* logic).
- **Severity labels in this plan follow the review's normalised vocabulary**
  (blocker/serious/moderate/minor/nitpick) and are not re-rounded. Where the
  review notes a specific WCAG success criterion (1.4.11, 2.5.3, 2.4.1,
  1.3.1/4.1.2, 1.4.1), this plan keeps that citation rather than folding it
  into a generic "a11y" tag — see Task 1.9 in particular, which is the one
  *hard* numeric WCAG failure in the whole sweep (1.69:1, needs 3:1).
- **Design-decision tasks are marked `[DESIGN DECISION — confirm before or during implementation]`.**
  These are places the review itself frames as judgment calls, not bugs. This
  plan proposes a concrete direction for each (so an agentic worker is never
  blocked), but a human sanity check is worth a message before or shortly
  after landing, not a blocking approval gate.
- **Testing/build commands** (run from `frontend/`): `npm test` (Vitest),
  `npm run build`, `npm run test:e2e` (needs `E2E_BASE_URL` or local `npm run dev`
  per `playwright.config.ts`). No task in Phases 0–3 requires `cargo test`
  except Task 0.2, which touches `crates/api`'s test suite for the seed-shape
  assertion.

---

## Phase 0 — Verification tasks (must happen before the rest of §3.6/§4 can be scoped correctly)

These are not UI fixes. Each is "find out what's actually true first," per
the review's own framing — most urgently for §4.1, where the review
explicitly says its own evidence (fixture data) is not trustworthy because
of §4.2.

### Task 0.1: Reproduce the BST/UTC train-timetable offset against real data

**Severity:** serious, needs live verification (review §4.1).
**Depends on:** Task 0.2 should land first — a corrected fixture removes one
possible confound (a fixture bug masquerading as a formatting bug) before
this task spends time on production data.
**Files (read/instrument only, no fix yet):**
- `frontend/lib/dateFormat.ts` (confirmed correct: pins `Europe/London` at
  both call sites — the divergence is upstream of formatting)
- `crates/api/src/data/journey.rs` (`RawCallingPoint`, `london_to_utc`)
- `crates/api/src/data/eta_blend.rs` (`london_to_utc`, if separate from
  `journey.rs`)
- `frontend/app/train/[uid]/[date]/page.tsx` (header instant), `frontend/components/JourneyTimeline.tsx` (per-row instant)

**Steps:**
- [ ] Pick one real, non-fixture train with a schedule match and calling
      points populated in a real deployment (or a `next dev` pointed at real
      `crates/api` data, not `.devdata/seed.sql`).
- [ ] Compare the header's departure/ETA against the timetable row for the
      same instant, the same way the review did (`train-uid-date` capture).
- [ ] If the offset reproduces: it's a real bug in the API → wire → parse
      path (most likely `RawCallingPoint`'s naive-time handling in
      `journey.rs`, or a `london_to_utc` call missing on one of the two
      code paths) — file it as a Task 3.6.0 fix (add to Phase 3 Track/Train,
      see that section) with the exact layer identified.
- [ ] If it does not reproduce: the offset was entirely an artifact of
      Task 0.2's fixture mismatch (naive London wall-clock times fed as
      already-zoned UTC instants) — close §4.1 with that finding recorded,
      no application fix needed.
- [ ] Either way, add the two tests the review recommends regardless of
      outcome: a backend unit test asserting
      `calling_points[0].scheduled_departure == pin_scheduled_departure` for
      a known train (`crates/api/src/data/journey.rs` or its test module),
      and a frontend test rendering one instant through both the header and
      the table asserting the two strings are equal (colocate with
      `frontend/app/train/[uid]/[date]/page.test.tsx` or `JourneyTimeline.test.tsx`).

### Task 0.2: Fix the seed fixture's `calling_points` wire-shape mismatch

**Severity:** serious (fixture), review §4.2.
**Independent of Task 0.1's outcome** — this needs fixing regardless, since
`.devdata/seed.sql` currently produces `could not build journey stops` server
logs (`crates/api/src/routes/train.rs:966`) and every train-page capture in
the sweep is of a degraded fallback, not the timetable-overlay feature.
**Files:**
- `.devdata/seed.sql` (the `calling_points` JSON blob at/around line 85,
  `INSERT INTO trains` at line 82)
- `.devdata/gen_seed.py` if the seed is generated rather than hand-written —
  check which before editing
- `crates/api/src/data/journey.rs` (`RawCallingPoint`, `build_journey_stops`
  — read-only, to confirm the target shape)

**Steps:**
- [ ] Rewrite the seed's `calling_points` to `RawCallingPoint`'s actual
      camelCase shape: `tiploc`, `kind`, `bookedArrival`/`bookedDeparture` as
      **naive London wall-clock** `NaiveTime` values (not zoned instants),
      `dayOffset`. Use real TIPLOCs that join against `stanox_crs` (the
      review names the join-key normalisation trap in `journey.rs`'s doc
      comment — trimmed + uppercased on both sides).
- [ ] Add a seed/integration test asserting `build_journey_stops` returns
      `Some` for the fixture train (as the review recommends) — likely in
      `crates/api`'s existing seed-driven test suite; grep for how other
      seed-shape assertions are structured there first.
- [ ] Run `cargo test` in `crates/api` (the one Phase 0/Phase 3 task that
      touches Rust — everything else in this plan is frontend-only).
- [ ] Note for Phase 4: this fixture fix is what makes Task 4.3 (re-capture
      the two train routes) meaningful — do not re-capture before this
      lands.

### Task 0.3: Investigate the dropped-whitespace-after-JSX-interpolation bug

**Severity:** moderate, needs verification (review §4.4).
**Independent** of every other Phase 0 task.
**Files:**
- `frontend/components/ReliabilityDigest.tsx:158` (`{rollup.eligibleCount} may have`)
- `frontend/components/DelayRepayEstimate.tsx:57` (`({delayMinutes} minutes)`)

**Steps:**
- [ ] Confirm the running/deployed build is on current `main` (the review
      flags this because neither file has changed since 2026-09-15/2026-08-30,
      so if the bug reproduces on a fresh build from current source, it's a
      toolchain/minifier issue rather than a stale deploy).
- [ ] If it reproduces on a fresh build: this is not a copy bug, it's a build
      pipeline issue (JSX-whitespace-collapsing in some minification step) —
      file it separately from this plan's frontend-copy tasks, since the fix
      is likely in build tooling/Next.js config, not component code.
- [ ] Regardless of root cause, make the space explicit at both call sites
      (`{count}{' '}may`, `({delayMinutes}{' '}minutes)`) and add a render
      test asserting the exact string — this is a one-line, zero-risk
      defensive fix independent of whatever the toolchain investigation
      finds, and should land even if the toolchain issue takes longer to
      root-cause.

### Task 0.4: Reproduce the mobile-Chromium Suspense-never-resolves report

**Severity:** moderate, needs verification (review §4.7, cross-referenced §2.11).
**Files:**
- `frontend/app/lines/[id]/page.tsx:470-489` (the two "Recent trends" Suspense
  boundaries)
- `frontend/components/HalfHourlyTrendsResults.tsx` (the wrapped chart
  component — check for a `matchMedia`/`ResizeObserver` dependency that could
  behave differently on mobile emulation)

**Steps:**
- [ ] Reproduce manually in Chrome device mode (iPhone 14 viewport) against
      a real deployment — load `/lines/[id]`, both light and dark, and watch
      whether the "Recent trends" skeletons resolve.
- [ ] If it resolves normally in manual testing: this was the sweep's own
      capture-timing artifact (full-page screenshot fired before streamed
      chunks flushed on slower mobile emulation) — no code fix, just note it
      resolved in Task 4.1's re-capture writeup.
- [ ] If it does not resolve: this is a real hydration bug in the chart
      wrapper — diagnose using superpowers:systematic-debugging and file as
      an addition to Task 1.10 (which already touches this exact Suspense
      boundary for the unrelated "no accessible loading state" finding —
      fix both in the same pass if this reproduces, to avoid re-touching the
      file twice).

### Task 0.5: Investigate the unhandled rejection on `/lines/[id]/edit` under a mocked network

**Severity:** minor, needs investigation (review §4.8).
**Files:**
- `frontend/app/lines/[id]/not-found.tsx` (the page rendered — a private
  line's edit route, anonymous or non-owner)
- `frontend/components/LineDefinitionTooltip.tsx` if it exists, or wherever
  the `/definition` fetch the review suspects lives — grep for it before
  assuming the file name

**Steps:**
- [ ] Confirm which client-side call fires on the 404 template under a
      mocked `/api/**` (the review's best guess: a session probe or the
      `LineDefinitionTooltip` fetch, which the private-lines spec says is
      *meant* to fail silently).
- [ ] Wrap the offending fetch in a `.catch()` so a swallowed failure stays
      swallowed rather than becoming an unhandled rejection — this is a
      one-line defensive fix once the call site is found.

### Task 0.6: Investigate `LoginLink`'s route-dependent colour

**Severity:** minor, unexplained (review §4.9). Lowest priority in Phase 0 —
do last, or skip if time-constrained, since both colours pass AA and the
only issue is the inconsistency itself.
**Files:** `frontend/components/AuthStatus.tsx` (renders `LoginLink`)

**Steps:**
- [ ] Compare `/lines`, `/lines/[id]`, `/lines/new` (grape "Log in") against
      `/lines/[id]/history` (dark grey "Log in") — find what differs about
      that one route's render tree (a wrapping `Text c=` prop, a CSS
      selector specificity difference, a different `AuthStatus` call site).
- [ ] Once understood, make the colour resolution explicit and consistent —
      either value passes AA, so this is about removing an unexplained
      per-route difference, not about picking a "correct" colour.

---

## Phase 1 — Cross-cutting fixes (§2)

Ordered by severity, then by blast radius (how many route categories a fix
touches) — per the brief's instruction that a cross-cutting fix resolving
findings across 4-6 areas outranks an equally-labelled single-page finding.
**Land these before Phase 3** so per-area work doesn't re-touch shared
components. Tasks in this phase are independent of each other except where
a "Touches the same file as" note says otherwise.

### Task 1.1: Fix `<main>`'s shrink-wrap (review §2.1)

**Severity:** serious. **Blast radius:** all 23 routes, one line.
**Files:** `frontend/app/layout.tsx:350`, `frontend/app/globals.css` (the
`body { display:flex; flex-direction:column; min-height:100vh }` rule
carrying the sticky-footer doc comment, currently around line 1009), a new
Playwright assertion in `frontend/e2e/`.

- [ ] Add `w="100%"` to the existing `<Container component="main" size="lg" px={0} style={{ flex: 1 }}>`
      at `layout.tsx:350` (or `align-self: stretch` on the `main` selector in
      `globals.css` if the prop doesn't thread through cleanly — verify
      against Mantine's `Container` styles first, same "confirm against
      installed source" discipline the previous plan used for this exact
      component).
- [ ] **Do not** apply the Groups slice's per-page `maw={640}`/`maw={480}`
      as a *replacement* for this fix — the review is explicit that those
      are separate typography decisions worth having on top of the root-cause
      fix, not instead of it. Leave that as a Phase 3 Groups task (3.2.7) if
      wanted.
- [ ] Add a Playwright assertion that `<main>`'s bounding width equals the
      nav container's, so this cannot regress silently.
- [ ] Screenshot-diff `/`, `/train/[uid]/[date]`, `/groups/[id]` before/after
      at desktop-1440x900 to confirm all three converge to one content edge.

### Task 1.2: Mobile nav collapse + desktop nav wrap (§2.2 + §2.7, explicitly the same work per the review)

**Severity:** serious (§2.2) + moderate (§2.7). **Blast radius:** every route,
both mobile and the authenticated desktop nav.
**Files:** `frontend/app/layout.tsx` (the nav `Group`s around lines 286-320),
a new `frontend/components/AppNavDrawer.tsx` (or similar — no existing
component to extend).

- [ ] Add a Mantine `Burger` + `Drawer` under `sm`: `hiddenFrom="sm"` on the
      current link `Group`, `visibleFrom="sm"` on the burger. Keep brand,
      theme toggle and Log in/avatar in the bar at every width; move All
      Lines / Station Lookup / Find a Train / Incident Archive / My Trains &
      Tickets / Groups into the drawer.
- [ ] At `≥md`, stop the bar from `flex-wrap`: give the header a fixed
      height and move "My Trains & Tickets", "Groups" and "Log out" under an
      account menu keyed by an avatar (Mantine `Menu`), so the anonymous and
      authenticated bars render at the same height. If a smaller first step
      is wanted, dropping the display name from the bar and shortening "My
      Trains & Tickets" to "My Trains" buys ~150px with a two-line diff —
      land that first if the account-menu work needs more design time.
- [ ] Verify in both Chromium and Firefox at 1440×900 — the review notes
      Gecko's ~4% narrower text metrics mean the wrap threshold is
      font-stack-dependent; don't just eyeball Chromium.

### Task 1.3: Replace emoji theme/pride toggles with deterministic icons (§2.3, resolves §2.16's "A" badge for free)

**Severity:** serious. **Blast radius:** every route (nav is global), all
Chromium users without a colour-emoji font.
**Files:** `frontend/components/ThemeToggle.tsx`, `frontend/components/PrideToggle.tsx`,
`frontend/app/globals.css:298-400` (existing `body[data-pride='…']::before`
stripe gradients — reuse these, don't invent new colours).

- [ ] `ThemeToggle`: replace the `'🌙' : '☀️'` glyphs with Tabler `IconSun` /
      `IconMoon` / `IconSunMoon` (the last one for "auto"). This retires the
      12px "A" `Indicator` badge entirely — `IconSunMoon` is already visually
      distinct from the other two, so the badge's whole reason for existing
      (auto→light can look like a no-op) goes away. Delete the badge and its
      doc comment along with the emoji.
- [ ] `PrideToggle`: render a small CSS-striped swatch inside the
      `ActionIcon`, keyed by mode, using the gradients already defined in
      `globals.css:298-400` — deterministic and font-independent, and
      actually distinguishes all nine modes (today six of nine share one
      plain-flag glyph that gives no visual feedback across most of the
      control's range).
- [ ] Keep both components' existing `aria-label`s — the review confirms
      they're already correct; this is a sighted-user rendering fix only.
- [ ] Verify in Chromium specifically (the failure mode is Chromium-without-
      colour-emoji-font); a before/after screenshot in `next build` (not
      `next dev`, since the review's methodology caveat #2 says the sweep
      itself ran against dev and isn't representative of hydration/bundle
      timing — irrelevant here, but keep the habit).

### Task 1.4: Inline login-prompt content on the four bare-heading private routes (§2.4)

**Severity:** serious. **Blast radius:** 4 routes (`/chat`, `/groups`,
`/track/mine`, `/track/mine/add-ticket`) — but high-value because it fixes
what search engines/unfurlers/pre-hydration visitors see.
**Files:** `frontend/app/groups/AutoOpenLoginPrompt.tsx`, `frontend/app/track/mine/AutoOpenLoginPrompt.tsx`,
`frontend/app/chat/page.tsx`, `frontend/app/groups/page.tsx`, `frontend/app/track/mine/page.tsx`,
`frontend/app/track/mine/add-ticket/page.tsx`. **Reference the already-correct
pattern** at `frontend/app/train/by-id/[trackingId]/page.tsx` and
`frontend/app/groups/[id]/page.tsx` (server-rendered `<h1>` + inline
underlined "Log in to view..." + `LoginLink`, no client-modal dependency).

- [ ] On each of the four routes, render the same sentence-plus-`LoginLink`
      pattern the two correct routes already use, in server markup, for the
      logged-out branch.
- [ ] Keep the auto-open modal as a progressive enhancement on top of that
      content, not as the only content — this is the explicit "revisit the
      accepted simplification" the review asks for against
      `AutoOpenLoginPrompt.tsx:16-21`'s own comment citing "Decision 6's Open
      Question 1."
- [ ] Copy differs per route and already exists in each modal's own
      call-site text — reuse it, don't write four new sentences.
- [ ] This is one shared *pattern*, four small per-route edits — safe to
      parallelize across the four files once the pattern is agreed, since
      none of the four files import from each other.

### Task 1.5: `wrap="nowrap"` shrink-guard convention (§2.5)

**Severity:** serious (WCAG 2.5.3 for the button case, per the review).
**Blast radius:** confirmed instances on 3 routes, latent risk at 30
`wrap="nowrap"` sites across `frontend/app`/`frontend/components`.
**Files:** `frontend/app/page.tsx:770`, `frontend/app/groups/[id]/page.tsx`
(`SharedTrainRow` around line 251, and `SharedCustomLineRow` — same defect,
not yet triggered), `frontend/app/track/mine/page.tsx:232`. Reference the
two sites that already document the convention correctly:
`frontend/components/LineStatusCard.tsx:17-23`, `frontend/components/IssueList.tsx:342`.

- [ ] Fix the three known instances: `style={{ flexShrink: 0 }}` on the
      badge/button, `lineClamp` on the title/label next to it.
- [ ] Extract a shared "title-plus-status row" primitive (new component,
      e.g. `frontend/components/StatusRow.tsx`) that gets the shrink rule
      right once, modeled on `LineStatusCard.tsx`'s existing documented
      pattern. Have `app/page.tsx`, `SharedTrainRow`, `SharedCustomLineRow`
      and `track/mine/page.tsx` adopt it rather than each carrying its own
      `Group wrap="nowrap"` — this also directly serves §2.9 (see Task 1.8:
      the status label itself should route through the same shared
      component so `/groups/[id]` and `/track/mine` agree on how a status
      renders, not just that it doesn't truncate).
- [ ] Add a lint rule or render test asserting any `Group wrap="nowrap"`
      containing a `Badge` gives that badge a shrink guard — the review notes
      this convention is already documented at two sites but not enforced.
- [ ] **Touches the same files as Task 1.8** (`app/page.tsx`, `app/groups/[id]/page.tsx`,
      `app/track/mine/page.tsx`) — do this task first since it establishes
      the shared `StatusRow` component Task 1.8 then feeds a label through,
      or coordinate as one combined change if dispatched to the same worker.

### Task 1.6: Skip link (§2.6)

**Severity:** serious (a11y), WCAG 2.4.1. **Blast radius:** every route, one
component.
**Files:** `frontend/app/layout.tsx` (first child of `<body>`).

- [ ] Add a visually-hidden-until-focused "Skip to content" link as the
      first child of `<body>`, targeting the existing `<main>` landmark
      (`layout.tsx:350`, `id="main"` or similar if `Container` doesn't
      already expose one).
- [ ] Independent of every other task in this plan — safe to land alone,
      any time.

### Task 1.7: `TextLink` inline rendering (§2.8)

**Severity:** moderate. **Blast radius:** 126 call sites, but the fix is
component-level plus a handful of call-site prop additions.
**Files:** `frontend/components/TextLink.tsx`, `frontend/app/globals.css`
(new `text-decoration-skip-ink: none` rule on `a[data-text-link]`), plus the
specific in-prose call sites the review names: the home-page tagline
sentence, `train/[uid]/[date]/page.tsx`'s "Find a train" sentence, and
`trains/page.tsx`'s "Track it manually" sentence.

- [ ] Give `TextLink` a `component="span"`/inline render path (it currently
      wraps `<Text>`, which Mantine renders as `<p>` — block-level, breaking
      any mid-sentence usage onto its own line).
- [ ] Apply the inline path at in-prose call sites only — positional links
      (nav items, actions beside a heading) keep the current render.
- [ ] Set `text-decoration-skip-ink: none` on `a[data-text-link]` in
      `globals.css` so the underline is continuous rather than gapped at
      word boundaries (this is also what's causing the Home/Chat C-7 visible
      gaps, likely the same root cause per the review).
- [ ] At the in-prose call sites, also set `underline="always"` — colour is
      currently the only cue for a mid-sentence link (WCAG 1.4.1), and the
      component's own doc comment already says `'always'` is for exactly
      this case.
- [ ] Do **not** attempt to audit all 126 call sites in this task — only the
      ones the review specifically names as broken. A grep for `<TextLink>`
      wrapped in a sentence (adjacent to plain `<Text>`/string siblings) can
      catch more later as a follow-up, but is out of scope here.

### Task 1.8: Shared display-label layer — status/TOC/CRS/category (§2.9)

**Severity:** moderate. **Blast radius:** every page category (the review's
own table lists Home, Groups, Lines, Track, Incidents, Stations — six of
six).
**Files:** new `frontend/lib/displayLabels.ts` (or extend `frontend/lib/stationLabel.ts`,
which already holds `routeLabel()` — see below), `frontend/app/page.tsx:809`
(`TrackedTrainStatusBadge`, currently a local, unexported function — extract
to `frontend/components/TrackedTrainStatusBadge.tsx`), `frontend/app/groups/[id]/page.tsx`
(the `en_route` literal in the shared-train card), `frontend/app/lines/[id]/page.tsx`
("Category"/"Operators" raw enum/ATOC-code display), `frontend/lib/stationLabel.ts:26-38`
(`routeLabel()` — fix the origin-name+code / destination-code-only mixing),
`frontend/app/incidents/page.tsx` and `frontend/app/incidents/[id]/page.tsx`
("Knowledgebase", "matcher", "reprocessing pass", "Last fetched", unlabelled
CRS pills), `frontend/app/lines/[id]/history/page.tsx` ("recomputes").

- [ ] Extract `TrackedTrainStatusBadge` out of `app/page.tsx` into a shared
      component; have `app/groups/[id]/page.tsx`'s shared-train card import
      it instead of printing `en_route` verbatim, so the two pages agree on
      status display (this is the review's explicit recommendation — "the
      groups page should reuse `TrackedTrainStatusBadge`").
- [ ] Add a TOC-code → operator-name lookup (per the review, `poller-tocs`
      already holds names and codes) and apply it at `/lines/[id]`'s
      "Operators" row and wherever an ATOC code is otherwise shown raw
      (`GR`, `e.g. SW` placeholders on `/lines/new`, `/lines/[id]/edit`,
      `/track`).
- [ ] Fix `routeLabel()` in `stationLabel.ts` so it resolves both ends
      through the same station lookup the autocomplete uses, rather than
      falling back to a bare code only on the destination when its name is
      null (today: "London Kings Cross (KGX) → EDB" mixes forms in one
      string — either resolve both or render both as codes, but not one of
      each).
- [ ] A category-enum → label map for `/lines/[id]`'s "Category" ("main-line" → a human label).
- [ ] Content fixes named by the review, each a one-line string change:
      "Knowledgebase" → "National Rail incident messages", "recompute" →
      "status change", "fetched" → "updated from National Rail",
      "propagated" → "Estimate (Network Rail)" (keep the long form in a
      tooltip and a `VisuallyHidden` span, per the review).
- [ ] Unlabelled CRS pills on `/incidents/[id]` — give them a visible label
      or wrap them so they read as station identifiers, not floating codes.
- [ ] Bare CRS as a destination on `/`, `/track/mine`, both train pages —
      resolved as part of the `routeLabel()` fix above, not a separate change.
- [ ] **Touches the same files as Task 1.5** (`app/page.tsx`,
      `app/groups/[id]/page.tsx`) — sequence after or combine with 1.5, per
      that task's note.

### Task 1.9: WCAG 1.4.11 hard failure — chip close-button contrast, plus broader touch-target padding (§2.10)

**Severity:** moderate-labelled by the review's own severity vocabulary, but
**do not round this down**: the chip `×` on `/lines/[id]/edit` measures
1.69:1 against a 3:1 WCAG 1.4.11 threshold — the only hard numeric SC
failure in the entire sweep, and it's on the control users tap most on that
form.
**Files:** `frontend/app/lines/CustomLineForm.tsx:194-217` (shared by
`/lines/new` and `/lines/[id]/edit`), plus a shared touch-target utility
applied across the icon-button set named by the review: `frontend/components/PinToggle.tsx`,
`frontend/components/ShareButton.tsx`, `frontend/components/InfoIcon.tsx`,
`frontend/components/IssueList.tsx` (accordion chevron), `frontend/components/GroupInviteLinkCard.tsx`
(share `ActionIcon`), `frontend/app/incidents/page.tsx` (date-preset buttons,
segmented segments, date-clear `×`), `frontend/components/JourneyProgress.tsx`
or `JourneyTimeline.tsx` (progress-line node buttons).

- [ ] **Fix the contrast failure first and separately** — the `CloseButton`
      inside the filled grape `Badge` at `CustomLineForm.tsx:209` currently
      keeps its default grey icon colour rather than the badge's white text
      (4.85:1, already used for the label). Set it to `white` explicitly.
      This one change is the single highest-priority item in this task.
- [ ] Then pad the hit areas: a single utility (`::before` inset, or a
      shared `ActionIcon` size default) applied to the icon-button set gets
      everything to ≥24px CSS, with 44px on primary-action ones (pin star,
      share). Keep the glyph sizes as-is — this is a hit-area change, not a
      visual redesign.
- [ ] Specific instances named by the review: the chip `×` (~10px, fixed
      above), incident date-preset buttons (~30px) and date-clear `×`
      (~20px), `/lines/[id]`'s share `ActionIcon` (28px), ⓘ (~20px), issue-row
      chevron (~16px), `/stations/[crs]`'s pin star and share (~28px), train
      pages' progress-line nodes (~14px `<button>`s — the only way to learn
      an intermediate stop's name on touch) and share button (32px).

### Task 1.10: Reserve slots for client-only values, labelled loading states (§2.11)

**Severity:** moderate. **Blast radius:** 4 instances across Home, Groups, Lines.
**Files:** `frontend/components/NotificationsToggle.tsx:50-52`, `frontend/components/GroupInviteLinkCard.tsx`
(the `window.location.origin` `useEffect` and `share()`'s early return),
`frontend/app/lines/[id]/page.tsx:470-489` (the two "Recent trends" `Suspense`
boundaries), `frontend/app/lines/[id]/history/page.tsx:153-217` (Timeline
panel `Suspense`).

- [ ] `NotificationsToggle`: render the button always, `disabled` until
      `supported` resolves, so it doesn't pop in after hydration and split
      the mobile hero between `<h1>` and tagline.
- [ ] `GroupInviteLinkCard`: compute the invite URL server-side from a
      `NEXT_PUBLIC_SITE_URL` / request host rather than `window.location.origin`
      in a `useEffect` — this also fixes the "Share is inert while
      `origin === ''`" bug and the "bare path is uncopyable" bug in one
      change. **Note:** this is the same finding as Task 3.2.2's "reuse
      `ShareButton` verbatim" deviation — do this task's server-side-origin
      fix first, since Task 3.2.2 depends on the URL being real before it
      can meaningfully add expiry copy beside it.
- [ ] Give every `Suspense` fallback named above a visible "Loading trends…"
      / "Loading history…" line inside a `role="status"` region with
      `aria-busy`, sized to the *empty* state rather than the populated one
      (a 560px skeleton that resolves to two lines of "not enough data" is a
      large layout shift today).
- [ ] **If Task 0.4 confirmed a real hydration bug** (not just a capture
      timing artifact) in the mobile-Chromium trends boundary, fix that
      alongside this task's fallback-labelling change, in the same file, in
      the same pass.

### Task 1.11: Timezone label + `LastUpdated` on the train page (§2.12)

**Severity:** moderate. **Blast radius:** Incidents, Lines history, Track/Train.
**Files:** `frontend/app/incidents/[id]/page.tsx` ("First seen"/"Last
fetched"), `frontend/app/lines/[id]/history/page.tsx` (per-row times),
`frontend/app/train/[uid]/[date]/page.tsx` and `frontend/app/train/by-id/[trackingId]/page.tsx`
(summary block), `frontend/components/LastUpdated.tsx` (exists, not
currently used on the train page), `frontend/components/ConnectivityMonitor.tsx`
(offline notification body — surface the same freshness timestamp inline
there too).

- [ ] Append "Times in UK local time" once per section rather than
      per-row, at each of the three locations named above.
- [ ] Add `<LastUpdated>` under the train summary block ("Updated 20:41 ·
      refreshes every 30 s") — the review notes this is the app's one
      genuinely live view and currently has no freshness indicator at all.
- [ ] Surface the same freshness timestamp inline in `ConnectivityMonitor`'s
      offline-notification body, so "showing the last update" (§5's credited
      copy) names an actual time rather than being an unverifiable claim.

### Task 1.12: One idiom for "pick exactly one" (§2.13)

**Severity:** moderate. **Blast radius:** Incidents archive, Lines history.
**`[DESIGN DECISION — confirm before or during implementation]`** for the
archive's interaction-model change specifically (see below).
**Files:** `frontend/app/incidents/page.tsx` (`IncidentSearchForm.tsx`'s date
presets + segmented control), `frontend/app/lines/[id]/history/page.tsx`
(same pattern).

- [ ] Replace the filled/light preset-pill-row-above-a-`SegmentedControl`
      pattern with a single `SegmentedControl` (or `Chip.Group`) labelled
      "Period", `color="grape"` so the selected segment uses the brand fill
      in both light and dark (today dark mode makes both controls markedly
      less legible — grape text on dark-grape fill, grey-on-grey).
- [ ] **Design decision:** on the history page, collapse to
      `7 days / 30 days / Custom…`, showing the date picker and its
      "Show history" submit only when "Custom" is chosen. This changes the
      page from "three controls for one value, two of which apply
      immediately and one of which needs a submit" to one interaction model
      — a real UX improvement per the review, but it's a scoped interaction
      change worth a quick look before shipping, not just a style fix. It
      also removes the undefined state where a hand-edited date leaves no
      preset highlighted, and recovers ~90 CSS px on mobile.
- [ ] Same treatment on `/incidents` (Type: All/Planned work/Real-time,
      Status: All/Active/Cleared) — these currently have no visible caption
      at all (also see Task 1.13's a11y note on these same two controls,
      §3.3's serious a11y finding — fix both together since it's the same
      `SegmentedControl` pair).

### Task 1.13: `IssueList` filter chrome vs. content imbalance (§2.14)

**Severity:** moderate. **Blast radius:** `/lines/[id]`, `/stations/[crs]`
(same component), plus the form-level version on `/incidents`.
**Files:** `frontend/components/IssueList.tsx`, `frontend/app/incidents/page.tsx`
(`IncidentSearchForm.tsx`'s Priority block).

- [ ] Collapse the chip filters (Severity, Source) behind a "Filter"
      disclosure when there are ≤3 issues.
- [ ] Only render source chips for sources actually present in the loaded
      report — the filters are client-side over already-loaded data, so the
      set is known; don't show all five source chips when only two sources
      exist in this report.
- [ ] Keep the All/Active/Upcoming segmented control always visible — it
      carries counts and is useful standalone.
- [ ] On `/incidents`, collapse the "Priority (raw feed value — meaning
      undocumented)" block from two side-by-side labels each with the
      caveat repeated, plus a third footnote repeat, to one
      `Input.Wrapper label="Priority range"` with Min/Max fields and the
      single existing footnote as its description — or move it behind a
      "More filters" disclosure. **Note:** the review is explicit that
      Archive spec Decision 3 (priority stays raw and honestly labelled) is
      right in intent; this fix is about *execution* (saying the caveat once
      vs. three times), not about reverting the honesty.

### Task 1.14: Firefox sleeper-rule + font-delivery verification (§2.15)

**Severity:** minor. **Blast radius:** every route, Firefox only.
**Files:** `frontend/app/globals.css` (the dashed "sleeper" divider rule —
currently likely a `border`/`background` with dot/dash styling that Gecko
renders sparsely), font loading (check `frontend/app/layout.tsx` or a
`next/font` config for whether the app's rounder sans is actually served as
a webfont).

- [ ] Replace the divider with a `repeating-linear-gradient` with an
      explicit `background-size`, or an inline SVG pattern — both render
      identically in Chromium and Firefox, unlike the current rule.
- [ ] Investigate whether the app's typeface is delivered as a webfont at
      all — the review notes *every* Firefox capture renders in a
      Helvetica/Arial-class fallback while Chromium renders the intended
      font, which "likely" means the font isn't being served as a webfont
      and Firefox is showing what a Windows/Android user without it
      installed sees, not a Firefox bug. **Verify this first** — if
      confirmed, add the font via `next/font/local` (or equivalent) so
      delivery no longer depends on the host having it installed. This also
      explains §2.7's knife-edge nav-wrap behaviour (Gecko's ~4% narrower
      fallback metrics), so fixing font delivery may reduce risk in Task 1.2
      as a side effect — do this task first if sequencing with 1.2.

### Task 1.15: Chrome-control consistency cluster (§2.16, remaining items after §2.3 already resolves the "A" badge)

**Severity:** minor (aggregate — "individually trivial items that together
make the shared chrome look unfinished," per the review). **Blast radius:**
nav auth controls, footer, several anonymous-CTA pages, offline banner.
**Files:** `frontend/components/AuthStatus.tsx` (Log in/Log out sizing),
`frontend/components/OpenDataAttribution.tsx` (footer link sizing),
`frontend/components/ConnectivityMonitor.tsx` (offline banner copy + mobile
width), `frontend/app/groups/join/[token]/page.tsx`, `frontend/app/connect-claude/page.tsx`,
`frontend/app/groups/page.tsx`, `frontend/app/lines/new/page.tsx`, `frontend/app/track/page.tsx`
(anonymous CTA weighting).

- [ ] Normalize auth-control sizing: "Log in" (16px text link), "Log out"
      (~12px bold button), footer's "powered by NationalRail" (12px
      underlined link, ~16px hit height) — pick one consistent size/weight
      treatment for text-link-styled actions in the chrome.
- [ ] `[DESIGN DECISION — confirm before or during implementation]`: the
      anonymous call-to-action is routinely the weakest element on the page
      it matters most on — `/groups/join/[token]` (the page an invitee
      reaches *by definition* anonymous, where logging in *is* the join
      step) shows an underlined text link where the authenticated equivalent
      is a filled full-width button. Same inversion on `/connect-claude`,
      `/groups`, and implicitly `/lines/new`, `/track`, both train pages.
      Proposed direction: promote the anonymous CTA to the same filled
      button treatment plus a `title`/helper note that it needs an account
      (matching the existing anonymous-pin-star pattern the review credits
      in §3.4) — worth a quick look since "make every anonymous CTA a filled
      button" is a visual-weight decision across ~6 pages, not a single
      component fix.
- [ ] Offline banner: rewrite copy on form pages (`/track`, `/trains`,
      `/track/mine/add-ticket`, `/lines/new`, `/lines/[id]/edit`) where "Can't
      reach live data right now — showing the last update." is untrue (there
      is no update to show on a form) — on those routes say something about
      entry safety instead (e.g. "Can't reach the server right now — your
      entries are safe until you submit."). Give the fixed wrapper
      `width: min(calc(100vw - 32px), 480px)` so it doesn't shrink to ~200px
      and wrap to four lines on a 390px phone.

---

## Phase 2 — Fixture data corruption cleanup (§4.3)

Fully independent of Phases 0/1/3 — a data-quality bug in test fixtures, not
application code. Can be dispatched to a separate worker at any point.

### Task 2.1: Fix the `$34`/`$35`/`$36` RSC-reference artifacts in fixtures

**Severity:** serious (fixture). **Files:** the 11 affected files under
`frontend/test/fixtures/accessibility/` (`BAL.json` ×3, `BHM.json` ×2,
`BTN.json` ×4, `EDB.json` ×1, `EUS.json` ×4, `HUL.json` ×1, `KGX.json` ×3,
`LDS.json` ×1, `MAN.json` ×1, `STP.json` ×1, `WVH.json` ×1 — confirmed via
`grep -rlE '"\$[0-9a-f]{1,3}"' frontend/test/fixtures/accessibility/`), and
`.devdata/seed.sql` (carries `$34`–`$38` and `$e`).

- [ ] Re-capture or hand-fix each affected fixture's `notes` values from
      `GET /public/stations/{crs}/accessibility` directly, or from the
      database — **not from a rendered page**, which is how these React
      Server Component de-duplication references leaked into the fixtures in
      the first place.
- [ ] Also fix the punctuation-only artifacts noted alongside (a lone `.`
      line preceding one of the `$3x` values).
- [ ] Add a fixture lint asserting no string value in
      `frontend/test/fixtures/accessibility/*.json` matches
      `/^\$[0-9a-f]{1,3}$/` — a cheap regression guard (a Vitest test file
      reading all 31 fixtures, or a small standalone script wired into `npm test`).
- [ ] Independently of the artifact fix, treat a scalar that is only
      punctuation (`.`, `-`, `N/A`) as empty in `isEmptyRenderable` (grep
      `frontend/lib` for this function) — the review notes `N/A` in §3.5 is
      real feed junk, not a scraping artifact, so this is a second, smaller
      fix in the same area but a different cause; land both together since
      they touch the same rendering path.
- [ ] Verify with `npm test` that "no raw node across 31 real payloads" (the
      existing regression test the review names) and every other
      fixture-driven assertion still pass, and now actually mean what they
      claim to mean.

---

## Phase 3 — Per-area fixes (§3)

Everything already promoted to Phase 1 is omitted here (cross-referenced
instead, per the review's own §3 framing). Land Phase 1 first where a task
below notes it depends on a Phase 1 component.

### 3.1 Home, Chat and Sign-in

**Files touched in this section:** `frontend/app/chat/callback/page.tsx`,
`frontend/app/chat/page.tsx`, `frontend/app/page.tsx`, `frontend/app/connect-claude/page.tsx`,
`frontend/components/ChatPanel.tsx`.

- [ ] **Task 3.1.1 (serious):** `/chat/callback` state handling —
      `app/chat/callback/page.tsx:51-60` keeps "Connecting…" as the `<h1>` in
      both success and error branches and renders `err.message` verbatim.
      Switch the heading with state (Loader → "Connected, taking you to
      Chat…" → "Couldn't connect"); on error, one plain sentence + a primary
      "Back to Chat" button + the raw message in a collapsed `<details>`;
      add a ~20s timeout on the `auth()` exchange; use `Alert color="red"`
      with an icon for `role="alert"` (WCAG 1.4.1 — currently colour-only).
- [ ] **Task 3.1.2 (serious):** the logged-in-but-not-allowed `/chat` state
      ("Chat — Not available for your account yet.") is a dead end — add a
      sentence explaining what chat is and a link to `/connect-claude`,
      which works for every logged-in user today. Spec context: the dual-mode
      design's 200-not-404 choice is correct and unchanged; this is adding
      the missing next-step sentence the 2026-09-02 review already asked
      for (its F8).
- [ ] **Task 3.1.3 (moderate):** `/chat` is undiscoverable — wrap
      `getChatbotAccess()` in the same `Suspense` as `GroupsNavItem`
      (`app/layout.tsx`) so allow-listed users get a nav item.
- [ ] **Task 3.1.4 (moderate):** authenticated dashboard reads as a stack of
      empty prompts — `app/page.tsx`'s "Your Lines"/"Your Stations" empty
      states each double a "Browse all lines/stations" link 40px above its
      own identical link. Keep one of the two links per section when empty;
      order "Right now" first when both pinned sections are empty.
- [ ] **Task 3.1.5 (moderate, flagged for Phase 4):** no way to start the
      MCP connection from `/chat` — `ChatPanel.tsx:87-92` returns early with
      a "reconnect from the Chat page" error when no MCP token is stored,
      and the only `auth()` call lives in `/chat/callback`. **This needs the
      Phase 4 coverage-gap capture first** (item 9 in §6 — an allow-listed
      chat user was never captured), since the sweep had no allow-listed
      user and this finding is "from source, not screenshots." Do not build
      a UI fix here speculatively; confirm the actual first-time-user flow
      is broken (vs. some button existing that the sweep simply didn't
      reach) before designing the fix.
- [ ] **Task 3.1.6 (minor):** `/connect-claude` polish — `CopyButton` with
      `aria-label="Copy connector URL"` for the long connector URL;
      `color="grape" variant="light"` + `IconInfoCircle` instead of Mantine's
      default blue `Alert` (the grape-theme spec reserves blue for `planned`
      severity — same fix applies to `ChatPanel.tsx:270`'s `bg="blue.0"`);
      "Click **+**" → "Click the **+** button"; em dash for "--".
- [ ] **Task 3.1.7 (minor):** brand appears twice above the fold on mobile
      ("Distant Signal" in nav + as `<h1>`, ~100px apart) — keep the `<h1>`
      for the outline, change its text to "Live UK rail status" with the
      existing tagline. **Sequence after Task 1.2** (mobile nav collapse) —
      that task changes the nav's own vertical footprint, so measure the
      "100px apart" gap again after 1.2 lands before deciding this is still
      worth doing on its own.

### 3.2 Groups and Sharing

**Files:** `frontend/app/groups/[id]/page.tsx`, `frontend/app/groups/new/page.tsx`,
`frontend/app/groups/join/[token]/page.tsx`, `frontend/components/GroupInviteLinkCard.tsx`,
`frontend/components/PromoteMemberButton.tsx`, `frontend/components/JoinGroupButton.tsx`.

- [ ] **Task 3.2.1 (serious) `[DESIGN DECISION — confirm before or during implementation]`:**
      "Delete group" and "Leave group" are visually identical (same red
      outline variant, 12px apart) despite wildly different blast radii.
      Proposed direction: keep "Leave group" as the red outline button;
      demote "Delete group" to a subtle red text button in a "Danger zone" at
      the foot of the page, or behind a "…" menu beside Rename; stack header
      actions full-width with `gap="sm"` on `xs`. Also check the Leave
      modal's owner branch states "X will become the owner" explicitly —
      ownership transfer is the one consequence a departing owner wouldn't
      guess. This is flagged as a design decision because "which of two
      destructive actions gets visual precedence" is a judgment call the
      review itself frames as ambiguous, not a clear bug.
- [ ] **Task 3.2.2 (moderate):** invite link never says it expires (spec §2.3
      gives every link a 7-day life; nothing in the UI says so). Add
      "Expires 24 Sept" beside the input; add helper text that "Regenerate"
      invalidates the old link. **Depends on Task 1.10's server-side invite-URL
      fix landing first** — expiry copy beside a URL that's currently
      wrong/uncopyable is lower value than fixing the URL first. **Deviates
      from spec:** §6 says "reuse `ShareButton`'s copy/Web Share pattern
      verbatim" — followed literally, this is the cause of the bug, because
      `ShareButton` shares the *current* page (no server origin needed) while
      the invite card shares a *different* URL. Get the URL from the server
      instead, per Task 1.10.
- [ ] **Task 3.2.3 (moderate, also review §4.6 correctness bug — fix here,
      not in Phase 0, since the fix is obvious once identified):** an
      existing member (most often the group's own owner, testing their
      invite link) is offered "Join group" because `/groups/join/[token]`
      only calls the unauthenticated preview. When `session.authenticated`,
      probe membership and render "You're already in {name} — Open group";
      make `JoinGroupButton` treat a 409/already-member response as success
      routing to the group, not an error alert.
- [ ] **Task 3.2.4 (moderate):** mobile member rows split awkwardly (name /
      badge / two stacked ~36px action buttons directly on top of each
      other) and `PromoteMemberButton.tsx` fires immediately with no toast.
      On `xs`, name+badge on one line, actions on their own full-width line
      (or a per-row "…" `Menu`); keep Remove red, Promote neutral; give
      Promote a lightweight confirmation or success notice.
- [ ] **Task 3.2.5 (minor):** navigation/identity gaps — add a "← Groups"
      `TextLink` above the detail-page `<h1>`; add a "(you)" marker in the
      member list; drop section `h2`s to `Title order={2} size="h4"`
      (matching `/lines/[id]`'s existing pattern) since they render at
      near-`h1` scale on mobile; add a hover/focus style or trailing chevron
      to the group card, currently a bare `<Link>` with
      `textDecoration:none; color:inherit` and no clickability signal.
- [ ] **Task 3.2.6 (minor):** `/groups/new` says nothing about what happens
      next — add "You'll get an invite link to share as soon as it's
      created." Consider enabling the submit button with client-side
      validation instead of a disabled grey-on-grey-in-dark-mode button.
- [ ] **Task 3.2.7 (minor):** anonymous handling differs across
      `/groups` (auto-open modal), `/groups/[id]`, `/groups/join/[token]`
      (inline links) — resolved automatically once Task 1.4 lands (moves
      `/groups` to the inline pattern too).
- [ ] **Task 3.2.8 (deferred from Phase 1's Task 1.1 note):** the review's
      Groups-specific `maw={640}`/`maw={480}` typography suggestion for the
      list and create-form pages, as a deliberate measure *on top of* Task
      1.1's root-cause fix, not instead of it.

### 3.3 Incidents

**Files:** `frontend/app/incidents/page.tsx` (`IncidentSearchForm.tsx`),
`frontend/app/incidents/[id]/page.tsx`.

- [ ] **Task 3.3.1 (serious):** the incident detail page never says who's
      affected or whether it's over — `page.tsx:95-173` renders neither
      `incident.operators` nor `incident.isCleared`, though both are in the
      API response and both appear on the archive rows. Add an at-a-glance
      strip under the title: Status (Active/Cleared, reusing the archive
      rows' existing badge), Operators by name (feeds off Task 1.8's
      TOC-name lookup), Affected stations labelled with names *and* codes,
      Planned/Real-time; when `isCleared`, replace "Currently affects" with
      "This incident has been cleared." **Deviates from spec:** Detail-page
      Decision 6 keeps NLP/severity fields off the page (still right); but
      operators and `isCleared` aren't extraction fields — they're already
      on the row and already public, so showing them here isn't the same
      deviation Decision 6 guarded against.
- [ ] **Task 3.3.2 (serious) — reverses a spec decision, not flagged as a
      design decision since the review states the direction plainly:** the
      archive lands on an empty form despite a default filter already being
      applied (30-day floor pre-filled, but `IncidentSearchForm.tsx:245-250`
      starts `results` at `null`). Run the search once on mount whenever the
      initial filter set is non-empty (always true, because of the 30-day
      floor); keep the explicit Search button for re-querying. At desktop
      width, put results above or beside the filters (the form is ~700px
      tall before any incident is visible). **Deviates from spec:** Archive
      Decision 6 copies `TrainSearchForm`'s "press Search first" shape —
      keep the *mechanics*, drop that one part of the precedent, since a
      train search needs input and an archive search does not (the 30-day
      floor's own stated purpose was to make the first view useful).
- [ ] **Task 3.3.3 (serious, a11y — WCAG 1.3.1/4.1.2):** the two
      `SegmentedControl`s (`IncidentSearchForm.tsx:473-490`) have no name,
      visibly or programmatically. Wrap each in `Input.Wrapper` labelled
      "Type" and "Status" — **do this together with Task 1.12's visual fix
      to these same two controls**, since both touch the identical JSX.
- [ ] **Task 3.3.4 (moderate):** "History" renders as a heading with nothing
      under it when `incident.history` is empty (`page.tsx:151-161` has no
      empty branch) — add "No changes recorded since this incident was first
      seen."
- [ ] **Task 3.3.5 (moderate):** priority-block sizing — resolved by Task
      1.13, cross-referenced here.
- [ ] **Task 3.3.6 (moderate, a11y):** "Validity", "Currently affects",
      "History" are `Text fw={500}` (`page.tsx:122,133,152`), not real
      headings — promote to `Title order={2} size="h5"` (visually
      identical, fixes the document outline).
- [ ] **Task 3.3.7 (moderate):** mobile title consumes ~40% of the first
      viewport — `size="h2"` or a responsive `fz`, consider `lineClamp` with
      the full text as the `title` attribute.
- [ ] **Task 3.3.8 (minor):** no back link on the detail page — add a small
      "← Incident Archive" `TextLink` above the title (this page is likely
      to be reached from a shared URL with no history to go back to).
- [ ] **Task 3.3.9 (minor):** "To (optional)" field balance
      (`placeholder="Any"`); constrain the form to `maw` ~720px at 1440
      (partly resolved by Task 1.1's `<main>` fix, verify remaining gap
      after that lands); move the share `ActionIcon` onto the badge's row
      rather than its own orphaned row.
- [ ] **Task 3.3.10 (flagged for Phase 4, not a Phase 3 fix):** "badge soup"
      on result rows (§3.3's own "Flagged pre-emptively, unverified"
      finding) — the sweep never captured a populated `/incidents` results
      state at all (§6 item 1). **Do not design a fix here.** Phase 4, Task
      4.2 captures the results state first; only then does this become a
      real, verified finding with a scoped fix (the review's own tentative
      direction if it does confirm: keep only Active/Cleared filled, render
      operators as names, fold Planned/Real-time into a prefix, move station
      codes to the detail page).

### 3.4 Lines

**Files:** `frontend/app/lines/AllLinesTable.tsx`, `frontend/app/lines/CustomLineForm.tsx`,
`frontend/app/lines/[id]/page.tsx`, `frontend/app/lines/[id]/history/page.tsx`,
`frontend/app/lines/[id]/not-found.tsx`, `frontend/app/lines/new/page.tsx`.

- [ ] **Task 3.4.1 (serious):** the Status column is blank for ~120 of ~125
      rows on "All Lines" — `AllLinesTable.tsx:318` renders `null` when
      there's no worst status, giving no way to distinguish "good service,
      nothing to report" from "no data" from "not computed yet." Render a
      grey outline "NO DATA" badge driven by the same `sampleUnavailableReason`
      helper the numeric columns already use (`lib/sampleStats.ts`), with the
      reason as its tooltip — grey, so it doesn't touch the severity
      palette (a stated Non-goal), and sort-by-status gains a defined
      bucket.
- [ ] **Task 3.4.2 (moderate):** 125 rows, no text filter, catalogue
      ordering — add a type-ahead "Find a line" input beside the operator
      filter (matching the existing Station Lookup pattern) and a default
      sort by name, or grouping by operator with sticky headers. Sticky
      filter bar on mobile.
- [ ] **Task 3.4.3 (moderate):** neither custom-line form (`/lines/new`,
      `/lines/[id]/edit`, both via `CustomLineForm.tsx`) says what a custom
      line is — add one dimmed paragraph under the `<h1>` explaining what it
      creates, that it's private, and that it appears in the user's All
      Lines table.
- [ ] **Task 3.4.4 (moderate):** station list has no empty state and no
      order-matters indication — "No stations yet — add at least two, in
      travel order." under the input; number the chips (1 KGX, 2 EUS) or
      render as a vertical ordered list with drag handles (per DESIGN.md
      §5.1's ordered-list domain model). Add `withAsterisk` to Name (it's
      currently validated as required with no visual mark).
- [ ] **Task 3.4.5 (minor):** the two "Not enough … data" empty-state boxes
      on `/lines/[id]` read almost identically with no heading between them
      — when both series are empty, render one combined box; when only one
      is, show that section's heading (verify the half-hourly-coverage
      spec's intended "Full coverage" `h3` is actually rendering — the
      review flags this as worth confirming since Task 1.10's Suspense fix
      touches the same area).
- [ ] **Task 3.4.6 (minor):** mobile header stacking orphans the ⓘ icon on
      its own row — group ⓘ and share right-aligned on the badge's row.
- [ ] **Task 3.4.7 (minor):** sort headers always show `↕`, never active
      direction — show `↑`/`↓` on the active column, set `aria-sort` on its
      `<th>`; add `white-space: nowrap` so "Avg Delay ↕" doesn't wrap its
      glyph.
- [ ] **Task 3.4.8 (minor):** wrapped row names on mobile read as two
      separate items — `lh={1.3}` on the name link.
- [ ] **Task 3.4.9 (minor):** timeline copy redundancy ("GOOD SERVICE Good
      Service 21:34") — when reason text equals the severity label, render
      "No incidents reported" or omit it.
- [ ] **Task 3.4.10 (minor):** anonymous 404 on `/lines/[id]/edit` offers
      "Back to your dashboard" (anonymous has none) — change to "Go to the
      home page", neutral for both; add a third "Log in" link for anonymous
      visitors only, so a session-expired owner has a way back in without
      leaking the line's existence.
- [ ] **Task 3.4.11 (minor):** no Delete on the edit page — add a secondary
      "Delete line…" text link at the foot of the form, opening the same
      confirmation modal already on the detail page.
- [ ] **Task 3.4.12 (minor):** dark mode loses "Show advanced options" as a
      link (near-white on near-white) — use grape-4 (5.70:1, the nav-link
      colour). Use the same affordance class for "Show"/"Hide" states.
- [ ] **Task 3.4.13 (minor):** anonymous users get no hint pinning/creating
      needs an account — add a `title` on the pin star ("Pin — needs an
      account"); show a one-line `Alert` on the custom-line form ("You'll be
      asked to log in when you save — your entries are kept.") and verify
      form state actually survives the OIDC round-trip.
- [ ] **Task 3.4.14 (flagged for Phase 4):** "Show advanced options" expanded
      state and the edit form's "Hide advanced options" state were never
      captured (§6 item 11) — confirm the affordance-class question (Task
      3.4.12) against a real expanded/collapsed pair before considering it
      closed.

### 3.5 Stations

**Files:** `frontend/app/stations/page.tsx`, `frontend/app/stations/[crs]/page.tsx`,
`frontend/components/StationAccessibilitySection.tsx`.

- [ ] **Task 3.5.1 (serious):** the page is named for disruptions and is 85%
      accessibility content with no way to jump to it. Rename the `<h1>` to
      "London Kings Cross (KGX)" (matching `generateMetadata`'s existing
      title form); add a compact "On this page" jump row (Disruptions ·
      Departures · Stats · Accessibility & facilities); give each section an
      `id` so `/stations/KGX#accessibility` is linkable. **Deviates from
      spec:** 09-12's Decision 8 placed accessibility last — keep that
      placement, only the page's *framing* (title + navigation) changes.
      This is cheaper than the 09-17 review's tabs proposal and gets most of
      the benefit, per this review's own recommendation.
- [ ] **Task 3.5.2 (serious, a11y) `[DESIGN DECISION — confirm before or
      during implementation, but the review itself has already overridden
      the standing constraint]`:** one `h2` for a 4,500px section — the four
      group titles and twelve key labels in `StationAccessibilitySection.tsx:487,496`
      are bold `<p>`s, not headings. Promote the four group titles to
      `<Title order={3} size="sm">` and the twelve keys to `order={4}`,
      holding visual size via `size` (no visual change, semantic-only). This
      explicitly reverses the "no new headings" constraint the 09-16
      structured-rendering plan set to keep the just-merged axe sweep green
      during the rendering rewrite — the review states plainly that
      constraint was right to make then and is wrong to keep now, since an
      h2→h3 step is exactly what `heading-order` exists to encourage and a
      4,500px section with one heading fails the people it's for. Flagged as
      a design decision anyway because it's a genuine site-structure change
      (new heading levels app-wide need the same end-to-end verification
      Decision 7 of the previous accessibility plan did) — add
      `getByRole('heading', { level: 3 })` assertions and re-verify no skip
      is introduced against the page's existing `<h1>` → `<h2>` chain.
- [ ] **Task 3.5.3 (serious):** every group leads with whatever the feed
      serialised first — the single most important fact (step-free
      category) is buried below less important items. Implement a stable
      kind-sort (sentences/booleans → facilities → tokens → times/contacts →
      collections), then an at-a-glance strip (step-free category,
      assistance hours/phone, accessible toilet/Changing Places, lift count,
      Blue Badge bays) — do the sort and de-duplication (Task 3.5.4) first,
      then the strip.
- [ ] **Task 3.5.4 (serious):** about a third of the section prints twice
      (Help points/Staff help/tactile-warnings sentences appear at two
      different y-offsets, once under a different group). Merge
      `staffAssistance` and `helpAndSupport` into one group; de-duplicate
      deep-equal nodes across the section before rendering.
- [ ] **Task 3.5.5 (moderate) `[DESIGN DECISION — confirm before or during
      implementation]`:** the nine disclosure/accordion controls say only
      "N items" and look like dividers, hiding exactly the question a reader
      has ("which lift serves platform 8"). Proposed direction: show the
      qualifier on screen, not only in `aria-label` ("13 toilet locations",
      "9 lifts", "11 platforms"); inline any collection of ≤3 items (six of
      the nine here); strip the accordion chrome so the control sits in the
      text column at its label's indent; never render "1 item" as an
      accordion. Flagged as a design decision because redesigning an
      existing, axe-covered disclosure affordance (09-16 §4.5 reused
      `Disclosure` unchanged, justified by that coverage) is a genuine UI
      redesign, not a copy fix — worth a quick look at the proposed
      direction before landing broadly.
- [ ] **Task 3.5.6 (moderate):** booleans render in two different visual
      languages (glyph+500-weight vs. plain label/value rows), and the
      important ones get the quiet one. One boolean rendering for
      `available` and its siblings; colour the glyph (green-7/red-7 light,
      green-4/red-4 dark, matching `StatusBadge`'s existing resolution); fold
      "Not available" facilities into one dimmed line per group.
- [ ] **Task 3.5.7 (moderate):** rich-text rhythm uneven against plain lines
      — `[data-rich-text] p { margin-bottom: var(--mantine-spacing-xs) }`,
      `[data-rich-text] :last-child { margin-bottom: 0 }`; raise label/value
      `Stack gap` from 2 to 4-6px.
- [ ] **Task 3.5.8 (moderate):** phone numbers in prose aren't tappable, and
      the same number appears three ways within 100px — linkify UK phone
      patterns in text nodes of the sanitizer's DOM output; drop a facility's
      free-text duplicate when it exactly equals the contact's
      `primaryTelephoneNumber`.
- [ ] **Task 3.5.9 (moderate):** raw URLs as link text, and `N/A` presented
      as a fact — when anchor text equals its `href`, replace with the host
      ("nationalrail.co.uk ↗") and keep the full URL in `title`; set
      `overflow-wrap: anywhere` on `[data-rich-text] a`; treat `N/A`, `-`,
      `.` and empty-after-trim as empty in `isEmptyRenderable` (**same fix
      as Phase 2 Task 2.1's `isEmptyRenderable` change — do these together**,
      one PR, since they're the identical function).
- [ ] **Task 3.5.10 (moderate):** prose runs the full ~1,100px container —
      `max-width: 70ch` on the section's text column.
- [ ] **Task 3.5.11 (moderate, aggregate):** label defects — "Car parks"
      printed twice in a row; "Atm"/"Cctv available"/"Cctv"/"Wifi" need a
      five-word acronym map; "Drop off pick up" rendered as a tick line
      purely by accident of shape; "Station accessibility" is an empty bold
      label (first line of the whole section); "Location: Next to Waitrose"
      inconsistent formatting vs. other "Location" rows; "Step free
      category" / "Category:" says "category" twice; "Names"/"Lifts info"
      relabelled to words a traveller would use.
- [ ] **Task 3.5.12 (minor):** "Search a different day or filter →" link
      floats outside its accordion — render inside the panel after the rows,
      or right-align in the control row.
- [ ] **Task 3.5.13 (minor):** `/stations` is empty below the search form —
      show pinned stations (logged in), recently viewed (localStorage), or a
      handful of major termini as plain links.
- [ ] **Task 3.5.14 (flagged for Phase 4):** "select a suggestion, then press
      Look up" two-step — the 2026-09-02 review's F6 asked for
      navigate-on-select; static captures can't show whether that landed.
      Worth a five-second check in the running app rather than a Phase 3
      code change assumed necessary.

### 3.6 Track and Train

**Files:** `frontend/components/JourneyProgress.tsx`, `frontend/components/JourneyTimeline.tsx`,
`frontend/components/TicketEntryForm.tsx`, `frontend/app/track/mine/add-ticket/page.tsx`,
`frontend/app/track/mine/page.tsx`, `frontend/app/track/page.tsx`, `frontend/app/trains/page.tsx`,
`frontend/components/TimeFilterInput.tsx`, `frontend/components/DelayRepayEstimate.tsx`,
`frontend/components/ReliabilityDigest.tsx`.

**Depends on Phase 0 Tasks 0.1/0.2**: do not fix anything about the
timetable/progress *content* (Tasks 3.6.2, 3.6.3) until the fixture is
corrected and, ideally, Task 4.3's re-capture confirms these findings
against the real feature rather than its degraded fallback. Task 3.6.1 (the
state-machine bug) is independent of the fixture question — it's a logic bug
in the caption-selection condition, reproducible on real data at any
junction/passing-point regardless of fixture quality — fix it now.

- [ ] **Task 3.6.1 (serious, also review §4.5 correctness bug — the fix is
      clear enough to do directly, not deferred to Phase 0):**
      `JourneyProgress.tsx:206-221` selects the `awaiting_activation` caption
      whenever `lastReachedIndex === -1`, regardless of `status`, so a train
      with a confirmed `en_route` status whose last movement report didn't
      match a timetabled stop gets the same "waiting for its first movement
      report" caption as a train with literally nothing confirmed — directly
      contradicting the summary above it ("Delay: 4M LATE", "Next calling
      point: … ETA …"). Add a branch for
      `lastIndex === -1 && status === 'en_route'` (or more precisely,
      whatever signal distinguishes "confirmed en route, no timetable match"
      from "truly nothing confirmed") — "Last reported at York — that
      report couldn't be matched to a timetabled stop, so no position is
      shown on the line." and the matching `aria-label`. **Keep the marker
      rule exactly as it is** (§5 credits "no marker until confirmed" as
      correct) — only the caption logic changes.
- [ ] **Task 3.6.2 (moderate — gated on Phase 0 Task 0.2):** "Unknown
      location" on every timetable row and both diagram labels —
      `journeyStopLabel` falls back `name → crs → 'Unknown location'`
      (`JourneyTimeline.tsx:67-69`). Resolve `tiploc → crs → name`
      server-side before building `journeyStops` (in `crates/api/src/data/journey.rs`,
      touching the shared TIPLOC/CRS join `journey.rs`'s own doc comment
      describes) and never drop a row for a missing tiploc — emit `name: null`
      and let the client label it by index ("Stop 3"); seed first/last row
      labels from the pin's origin/destination; if *every* stop is unnamed,
      collapse to a single dimmed line ("13 stops — station names
      unavailable"). **This finding is fixture-amplified per the review** —
      confirm against real data (post Task 0.2) before assuming the current
      severity/frequency is representative.
- [ ] **Task 3.6.3 (moderate — gated on Phase 0 Task 0.1):** six-hour-stale
      ETA in the present tense under a "may have arrived" alert. When
      `mayHaveArrived` is true, render "Was due at Edinburgh Waverley 14:34
      (no arrival report received)" instead of the present-tense ETA badge.
      Confirm this isn't itself a symptom of Task 0.1's timezone bug before
      implementing — an ETA that looks "stale" partly because it's
      mis-zoned is a different bug than one that's genuinely stale.
- [ ] **Task 3.6.4 (serious):** the manual ticket form
      (`TicketEntryForm.tsx:323-353`) has no placeholders, no helper text, no
      required marks — reuse `/track`'s station `Autocomplete` for
      origin/destination (labelled "Origin station"/"Destination station",
      not "CRS code" — the picker-refactor spec removed that label from
      `/track` for exactly this reason and this form never got the memo);
      give Operator the same treatment as Task 1.8; give Ticket type a
      placeholder or a `Select` if the backend has an enum; `withAsterisk`
      on required fields, "(optional)" text on optional ones.
- [ ] **Task 3.6.5 (moderate):** add-ticket page never says which train the
      ticket attaches to, or that it doesn't need one yet — add one dimmed
      sentence under the title: "Save the ticket now; you can attach it to a
      tracked train afterwards, or we'll try to match it for you."
- [ ] **Task 3.6.6 (moderate) `[DESIGN DECISION — confirm before or during
      implementation]`:** upload paths (pkpass/PDF dropzones — the
      drag-and-drop spec's whole investment) are hidden one tab away behind
      "Manual entry," the spec's stated default. Two options, either
      acceptable per the review: (a) make the upload tab the default (manual
      stays one tap away; both upload paths already land the user back on
      pre-filled manual fields), or (b) put a single compact dropzone
      *above* the tabs so the choice is visible without a tab switch.
      Proposed direction: (b) — it doesn't require deciding which entry
      method is "primary" and gets the discoverability win either way.
      Flagged because this reverses a deliberate spec default, not a bug.
- [ ] **Task 3.6.7 (minor):** no "stop tracking" affordance on the list; "Delete"
      is the wrong word on the detail page (users don't delete trains, they
      stop tracking them). Add an overflow kebab on the row (Rename / Stop
      tracking — also relieves Task 1.5's space contest on that same row);
      relabel the detail-page button "Stop tracking," keeping red outline +
      confirm modal.
- [ ] **Task 3.6.8 (minor):** date/time stated twice in the same ticket
      card — print the dimmed `when` line only when `train.customName` is
      set.
- [ ] **Task 3.6.9 (minor):** train page's title duplicated ("Train
      W12345" `<h1>` immediately followed by "Train W12345" again) —
      suppress the subtitle when it equals the `<h1>`, or make the `<h1>` the
      route and the subtitle the uid.
- [ ] **Task 3.6.10 (minor):** `/train/by-id/[trackingId]`'s anonymous title
      is the internal subscription id ("Tracking Train 1") — "Tracked
      train" as the `<h1>` instead (or "Someone's tracked train — log in to
      see it" if the route can't disclose more pre-auth); if Share already
      copies the canonical URL, say so in its tooltip.
- [ ] **Task 3.6.11 (minor):** Delay Repay disclaimer appears three times
      across two adjacent pages (~120 words/page) — keep one full disclaimer
      at the card level (`ReliabilityDigest.tsx`); reduce the per-ticket one
      (`DelayRepayEstimate.tsx`) to "This app never submits a claim on your
      behalf" + the link. Fix the "--" → em dash here too (same file as Task
      0.3's whitespace fix — do together).
- [ ] **Task 3.6.12 (minor):** dark-mode alert/badge weighting on train page
      — use `--mantine-color-yellow-light`/`-light-color` tokens for the
      alert so `variant="light"` semantics survive into dark; bump the "4M
      LATE" badge to `size="sm"` or drop uppercase to ease the contrast
      budget (currently ~3.5:1, borderline for 11px uppercase text).
- [ ] **Task 3.6.13 (minor):** `/trains`' `TimeFilterInput` renders the
      12-hour "--:-- --" native-`<input type="time">` skeleton in an en-US
      browser locale on a UK 24h site — switch to Mantine `TimeInput` with
      explicit 24h, or a text input with `placeholder="HH:MM"` and
      `inputMode="numeric"` (matching the multi-day spec's own description).
- [ ] **Task 3.6.14 (minor):** `/track`'s disabled "Track this train" is
      near-invisible in dark mode (light grey on slightly-lighter grey,
      under 2:1) — `variant="light"` or a bordered outline so shape survives
      even disabled, or don't disable it at all and let submit trigger the
      existing field-error `Alert`.
- [ ] **Task 3.6.15 (minor):** add-ticket tab strip wraps at 390px, two rows
      read as two different components — shorter labels ("Manual",
      ".pkpass", "PDF") or a `fullWidth` `SegmentedControl`.
- [ ] **Task 3.6.16 (nitpick):** "Your reliability" ordering when the only
      train is EN ROUTE but its ticket already has a delay estimate; two
      adjacent `/trains` fields sharing one placeholder example.

---

## Phase 4 — Coverage-gap follow-up capture (§6)

Not fixes. A targeted re-run of `frontend/e2e/screenshot-sweep/` with
narrower `--routes=`/`--states=` flags to actually see the states/
interactions the original static sweep couldn't reach — several Phase 3
tasks above (3.1.5, 3.3.10, 3.4.14, 3.5.14) explicitly wait on this rather
than presuming a fix for something unverified.

### Task 4.1: Re-run the full sweep against a production build

**Depends on:** nothing structurally, but most valuable *after* Phase 1
lands, so the re-capture reflects the fixed chrome rather than needing a
third pass.
- [ ] Build with `next build` / `devIndicators: false` (removes the dev "N"
      indicator artifacts the review's methodology caveat #2 flags as
      overlapping content in several captures).
- [ ] Add a settle step that waits for the streamed nav to resolve before
      shooting (methodology caveat #3 — some "authenticated" captures in
      the original sweep weren't actually authenticated because the nav
      hadn't settled).
- [ ] Re-run `node run.ts` with the full route/state/device/browser matrix
      the original sweep used, per `frontend/e2e/screenshot-sweep/README.md`.

### Task 4.2: Targeted interactive-state captures

Using `--routes=` / `--states=` to scope narrowly (per `README.md`'s CLI
flags), script each of the following (the review's own §6 script notes are
the acceptance criteria):

- [ ] `/incidents` with results — load, click Search. (Feeds Task 3.3.10.)
- [ ] `/trains` with results — Station=KGX submit; repeat with Stops at=YRK;
      repeat under the `api-error` mock.
- [ ] `/track`'s departure picker with rows — type an origin before
      capturing.
- [ ] `/lines/[id]/history`'s Trends tab — `?tab=trends` at all three
      widths, light and dark.
- [ ] `/stations/[crs]`'s "Scheduled departures" accordion — click the
      control before capturing `loading` and `api-error`.
- [ ] Authenticated dark-mode `/groups/[id]`.
- [ ] An expired/invalid invite token on `/groups/join/[token]`.
- [ ] A plain-member and an admin view of `/groups/[id]`, plus a
      long-named group with 6+ members.
- [ ] An allow-listed chat user on `/chat` (feeds Task 3.1.5).
- [ ] `/lines/[id]` for a custom line (Edit + red Delete) and its delete
      confirmation modal.
- [ ] Either custom-line form with "Show advanced options" expanded, and
      the edit form's "Hide advanced options" state (feeds Task 3.4.14).
- [ ] An expanded incident row on `/lines/[id]`.
- [ ] `/track/mine/add-ticket`'s upload-tab dropzone and its post-save
      "Ticket saved" alert.
- [ ] An open autocomplete dropdown anywhere in the app.
- [ ] Hover and focus states (tooltips on "—" cells, focus rings on pin
      stars, sort-header affordances, whether `/lines/[id]`'s issue row is
      wholly clickable or only its chevron is) — the review is explicit that
      "no focus-ring finding" in the original sweep must not be read as
      "focus rings are fine."

### Task 4.3: Re-capture the two train routes against the corrected fixture

**Depends on:** Phase 0 Task 0.2 (fixture fix) and Phase 2 (fixture
corruption cleanup, if it touches any of the same fixture files — check for
overlap before running).
- [ ] Re-capture `/train/[uid]/[date]` and `/train/by-id/[trackingId]`.
- [ ] Re-examine §3.6 and §4.1's findings against the corrected fixture —
      several (Task 3.6.2's "Unknown location," Task 0.1's timezone
      question) may look different or disappear entirely once the real
      timetable-overlay feature is actually being exercised instead of its
      degraded fallback.

---

## Final Verification (after all phases land)

- [ ] `npm test && npm run build` from `frontend/` — must pass.
- [ ] `cargo test` in `crates/api` — for Task 0.2's seed-shape assertion.
- [ ] Re-run the Task 1.x-era live-axe check (`frontend/e2e/accessibility.spec.ts`,
      from the previous accessibility plan) against every route touched by
      Phase 1/3 — confirm zero new `color-contrast`, `landmark-one-main`,
      `region`, `heading-order`, `page-has-heading-one` violations, and
      specifically confirm the WCAG 1.4.11 chip-contrast fix (Task 1.9) and
      the new headings introduced by Task 3.5.2 don't create a fresh skip.
- [ ] Walk review §5's "what is working well" list one more time against
      the final state — this plan should not be able to point at a single
      regression in that list. Pay particular attention to `JourneyProgress`'s
      "no marker until confirmed" rule (touched by Task 3.6.1) and
      `StationAccessibilitySection`'s "no raw JSON anywhere" property
      (touched by Tasks 3.5.2-3.5.11).
- [ ] Confirm the two documents Task 0.1/0.2 and Phase 2 leave "resolved
      not open": review §4.1/§4.2's "must be resolved before the timetable
      overlay reaches real users" and §4.3's fixture-corruption note should
      both point at completed work, not remain open questions in a spec
      nobody circled back to.
