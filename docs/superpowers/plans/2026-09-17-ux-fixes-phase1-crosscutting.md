# UX Fixes — Phase 1: Cross-cutting Fixes

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development
> to work this plan task-by-task, **in the order listed** (not reordered,
> not parallelized within this worktree — see below). Steps use checkbox
> (`- [ ]`) syntax for tracking.
>
> This is a standalone slice of the full plan at
> `docs/superpowers/plans/2026-09-17-full-service-ux-accessibility-fixes.md`
> (its Phase 1), split out so it can run in its own isolated worktree in
> parallel with that plan's Phase 0 and Phase 2 worktrees. **Unlike those
> two, this phase's 15 tasks are NOT independent of each other** — nearly
> all of them touch `frontend/app/layout.tsx` and/or `frontend/app/globals.css`,
> which is exactly why this phase runs as ONE sequential worktree rather
> than being split further: splitting it into more parallel worktrees would
> just produce constant merge conflicts on those two files. The
> subagent-driven-development skill already processes tasks one at a time
> within a single worktree — follow that as written, in the task order
> below (which already encodes the "Touches the same file as" dependencies
> noted per-task).
>
> This worktree does not depend on the parallel Phase 0 or Phase 2
> worktrees, and they don't depend on it — a later merge step reconciles
> all three. Phase 3 of the parent plan (a separate, later piece of work,
> not part of this worktree) depends on THIS phase landing first, since it
> adopts shared components this phase creates.

**Goal:** land the 15 highest-leverage findings from
`docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md`
§2 — patterns that each resolve UX/accessibility findings across 4-6 route
categories at once (a shrink-wrapped `<main>`, no mobile nav collapse, emoji
toggles rendering as tofu boxes in Chromium, bare-heading anonymous states,
unguarded text truncation, a missing skip link, a block-level inline-link
component, inconsistent status/label display, a hard WCAG 1.4.11 contrast
failure, unlabelled loading states, missing timezone/freshness context, two
incompatible "pick one" filter idioms, filter-chrome imbalance, Firefox
rendering gaps, and a cluster of minor chrome inconsistencies) — rather than
91 separate per-page patches.

**The design specs remain non-binding**, exactly as the source review
states in its own intro. Where a task below departs from a spec's stated
intent, it says so; do not "restore compliance" with a spec a task
explicitly departs from.

**Architecture:** almost entirely frontend (`frontend/app`, `frontend/components`,
`frontend/lib`). This phase introduces three new shared components/modules
that a later, separate piece of work (Phase 3, not part of this worktree)
adopts: a status-row primitive (Task 1.5), a mobile nav drawer (Task 1.2),
and a shared display-label layer (Task 1.8). Build these as genuinely
reusable — Phase 3 will import them, not reimplement them.

**Tech Stack:** Next.js 16 App Router + TypeScript, Mantine v9.5.2
(`@mantine/core`, `@mantine/charts`), Vitest 2 + `@testing-library/react` via
`frontend/test/render.tsx`'s `renderWithMantine`, Playwright 1.62
(`frontend/e2e/`).

**Specs:**
- `docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md`
  — §2 is the cross-cutting findings this plan implements; §5 is "protect
  this, don't regress it." Finding numbers (e.g. "§2.3") cited below mean
  that document's headings.
- `docs/superpowers/specs/2026-09-02-frontend-accessibility-audit-research.md`
  and `docs/superpowers/plans/2026-09-02-frontend-accessibility-fixes.md` —
  the previous accessibility pass. **Already landed and verified working**
  (review §5): the `<main>` landmark, `autoContrast`/`luminanceThreshold: 0.179`,
  the grape-7 filled-surface override, and gray-7 dimmed text are all live in
  `frontend/app/layout.tsx` / `frontend/lib/theme.ts` / `frontend/app/globals.css`
  today. Task 1.1 layers a *width* fix onto the *same* `<main>` `Container`
  the previous plan already landmarked — read that task's note before
  editing.

---

## Global Constraints

- **No change to `frontend/lib/severity.ts`'s `GROUP_COLOR` hue map.** The
  grape-theme spec's Non-goal (still binding per the previous plan) says the
  five severity hues carry meaning users already read at a glance. Nothing
  in this phase needs a hue change — Task 1.8's display-label layer changes
  *text labels*, not colours.
- **Every §5 "working well" item in the source review is a regression bar,
  not a suggestion.** Before landing any task that touches a file
  `StatusBadge`, `ConnectivityMonitor`, or any other component the review's
  §5 names, re-read that paragraph and re-verify the property it credits
  still holds.
- **Severity labels below follow the source review's normalised vocabulary**
  (blocker/serious/moderate/minor/nitpick) and are not to be re-rounded.
  Where the review notes a specific WCAG success criterion (1.4.11, 2.5.3,
  2.4.1, 1.3.1/4.1.2, 1.4.1), keep that citation — see Task 1.9 in
  particular, the one *hard* numeric WCAG failure in the whole sweep
  (1.69:1, needs 3:1) — do not downgrade it because its severity label
  elsewhere says "moderate."
- **Design-decision tasks are marked `[DESIGN DECISION — confirm before or during implementation]`.**
  Each proposes a concrete direction (so you're never blocked), but flag it
  clearly in your task report as a judgment call, not a settled bug fix.
- **Testing/build commands** (run from `frontend/`): `npm test` (Vitest),
  `npm run build`, `npm run test:e2e` (needs `E2E_BASE_URL` or local `npm run dev`).
  No task in this phase touches Rust/`cargo test`.
- **Task order below is load-bearing, not incidental** — it already
  encodes every "touches the same file as" dependency between these 15
  tasks. Do not reorder.

---

## Phase 1 — Cross-cutting fixes

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
      installed source" discipline the previous accessibility plan used for
      this exact component).
- [ ] Do **not** apply per-page `maw={640}`/`maw={480}` typography
      constraints as a *replacement* for this fix — that's separate,
      page-specific work belonging to a later piece of work, not this task.
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
- [ ] Verify in both Chromium and Firefox at 1440×900 — Gecko's ~4% narrower
      text metrics mean the wrap threshold is font-stack-dependent; don't
      just eyeball Chromium.
- [ ] Build this as a genuinely reusable nav-drawer component — a later,
      separate piece of work adopts it; don't inline the drawer logic
      directly into `layout.tsx` in a way that can't be imported.

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
- [ ] Keep both components' existing `aria-label`s — they're already
      correct; this is a sighted-user rendering fix only.
- [ ] Verify in Chromium specifically (the failure mode is Chromium-without-
      colour-emoji-font); a before/after screenshot in `next build` (not
      `next dev`, since dev builds carry indicator overlays that can
      overlap captured content).

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
      content, not as the only content.
- [ ] Copy differs per route and already exists in each modal's own
      call-site text — reuse it, don't write four new sentences.
- [ ] This is one shared *pattern*, four small per-route edits — fine to
      do as one task/commit since none of the four files import from each
      other.

### Task 1.5: `wrap="nowrap"` shrink-guard convention (§2.5)

**Severity:** serious (WCAG 2.5.3 for the button case, per the review).
**Blast radius:** confirmed instances on 3 routes, latent risk at 30
`wrap="nowrap"` sites across `frontend/app`/`frontend/components`.
**Files:** `frontend/app/page.tsx:770`, `frontend/app/groups/[id]/page.tsx`
(`SharedTrainRow` around line 251, and `SharedCustomLineRow` — same defect,
not yet triggered), `frontend/app/track/mine/page.tsx:232`. Reference the
two sites that already document the convention correctly:
`frontend/components/LineStatusCard.tsx:17-23`, `frontend/components/IssueList.tsx:342`.

**Do this task together with Task 1.8 (next)** — both touch
`app/page.tsx` and `app/groups/[id]/page.tsx`; this task establishes the
shared `StatusRow` component that Task 1.8 then feeds a label through.
Either combine them into one commit, or land this one first.

- [ ] Fix the three known instances: `style={{ flexShrink: 0 }}` on the
      badge/button, `lineClamp` on the title/label next to it.
- [ ] Extract a shared "title-plus-status row" primitive (new component,
      `frontend/components/StatusRow.tsx`) that gets the shrink rule right
      once, modeled on `LineStatusCard.tsx`'s existing documented pattern.
      Have `app/page.tsx`, `SharedTrainRow`, `SharedCustomLineRow` and
      `track/mine/page.tsx` adopt it rather than each carrying its own
      `Group wrap="nowrap"`.
- [ ] Add a lint rule or render test asserting any `Group wrap="nowrap"`
      containing a `Badge` gives that badge a shrink guard.
- [ ] Build `StatusRow` as genuinely reusable — a later, separate piece of
      work (Phase 3) adopts it further.

### Task 1.6: Skip link (§2.6)

**Severity:** serious (a11y), WCAG 2.4.1. **Blast radius:** every route, one
component.
**Files:** `frontend/app/layout.tsx` (first child of `<body>`).

- [ ] Add a visually-hidden-until-focused "Skip to content" link as the
      first child of `<body>`, targeting the existing `<main>` landmark
      (`layout.tsx:350`, `id="main"` or similar if `Container` doesn't
      already expose one).
- [ ] Independent of every other task in this phase — safe to land at any
      point in the sequence; doing it here keeps `layout.tsx` edits grouped
      near Tasks 1.1/1.2's own `layout.tsx` changes.

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
      word boundaries.
- [ ] At the in-prose call sites, also set `underline="always"` — colour is
      currently the only cue for a mid-sentence link (WCAG 1.4.1), and the
      component's own doc comment already says `'always'` is for exactly
      this case.
- [ ] Do **not** attempt to audit all 126 call sites — only the ones named
      above. A grep for `<TextLink>` wrapped in a sentence can catch more
      later as a follow-up, but is out of scope here.

### Task 1.8: Shared display-label layer — status/TOC/CRS/category (§2.9)

**Severity:** moderate. **Blast radius:** every page category (Home, Groups,
Lines, Track, Incidents, Stations — six of six).
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

**Land after or combined with Task 1.5** (same files: `app/page.tsx`,
`app/groups/[id]/page.tsx`).

- [ ] Extract `TrackedTrainStatusBadge` out of `app/page.tsx` into a shared
      component; have `app/groups/[id]/page.tsx`'s shared-train card import
      it instead of printing `en_route` verbatim, so the two pages agree on
      status display.
- [ ] Add a TOC-code → operator-name lookup (`poller-tocs` already holds
      names and codes) and apply it at `/lines/[id]`'s "Operators" row and
      wherever an ATOC code is otherwise shown raw (`GR`, `e.g. SW`
      placeholders on `/lines/new`, `/lines/[id]/edit`, `/track`).
- [ ] Fix `routeLabel()` in `stationLabel.ts` so it resolves both ends
      through the same station lookup the autocomplete uses, rather than
      falling back to a bare code only on the destination when its name is
      null (today: "London Kings Cross (KGX) → EDB" mixes forms in one
      string — either resolve both or render both as codes, but not one of
      each).
- [ ] A category-enum → label map for `/lines/[id]`'s "Category" ("main-line" → a human label).
- [ ] Content fixes, each a one-line string change: "Knowledgebase" →
      "National Rail incident messages", "recompute" → "status change",
      "fetched" → "updated from National Rail", "propagated" → "Estimate
      (Network Rail)" (keep the long form in a tooltip and a
      `VisuallyHidden` span).
- [ ] Unlabelled CRS pills on `/incidents/[id]` — give them a visible label
      or wrap them so they read as station identifiers, not floating codes.
- [ ] Bare CRS as a destination on `/`, `/track/mine`, both train pages —
      resolved as part of the `routeLabel()` fix above, not a separate change.
- [ ] Build the display-label layer as genuinely reusable — a later,
      separate piece of work (Phase 3) adopts it further (e.g. the manual
      ticket-entry form's Operator field).

### Task 1.9: WCAG 1.4.11 hard failure — chip close-button contrast, plus broader touch-target padding (§2.10)

**Severity:** the source review's own vocabulary labels this "moderate",
but **do not round this down**: the chip `×` on `/lines/[id]/edit` measures
1.69:1 against a 3:1 WCAG 1.4.11 threshold — the only hard numeric SC
failure in the entire review, and it's on the control users tap most on that
form.
**Files:** `frontend/app/lines/CustomLineForm.tsx:194-217` (shared by
`/lines/new` and `/lines/[id]/edit`), plus a shared touch-target utility
applied across the icon-button set: `frontend/components/PinToggle.tsx`,
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
- [ ] Specific instances: the chip `×` (~10px, fixed above), incident
      date-preset buttons (~30px) and date-clear `×` (~20px), `/lines/[id]`'s
      share `ActionIcon` (28px), ⓘ (~20px), issue-row chevron (~16px),
      `/stations/[crs]`'s pin star and share (~28px), train pages'
      progress-line nodes (~14px `<button>`s — the only way to learn an
      intermediate stop's name on touch) and share button (32px).

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
      change.
- [ ] Give every `Suspense` fallback named above a visible "Loading trends…"
      / "Loading history…" line inside a `role="status"` region with
      `aria-busy`, sized to the *empty* state rather than the populated one
      (a 560px skeleton that resolves to two lines of "not enough data" is a
      large layout shift today).
- [ ] A parallel worktree (this plan's Phase 0, Task 0.4) is separately
      investigating whether the mobile-Chromium "Recent trends" boundary has
      a real hydration bug beyond the labelling gap. If your own testing
      while implementing this task also notices the skeleton never
      resolving on mobile Chromium, fix it here (you own this file); note
      it in your report either way so the parent session can reconcile with
      Phase 0's findings.

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
      refreshes every 30 s") — this is the app's one genuinely live view and
      currently has no freshness indicator at all.
- [ ] Surface the same freshness timestamp inline in `ConnectivityMonitor`'s
      offline-notification body, so "showing the last update" names an
      actual time rather than being an unverifiable claim.

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
      "Show history" submit only when "Custom" is chosen. This is a scoped
      interaction change worth flagging in your report — it also removes
      the undefined state where a hand-edited date leaves no preset
      highlighted, and recovers ~90 CSS px on mobile.
- [ ] Same treatment on `/incidents` (Type: All/Planned work/Real-time,
      Status: All/Active/Cleared) — these currently have no visible caption
      at all. **Note:** the `/incidents` two `SegmentedControl`s also need a
      programmatic label (`Input.Wrapper`) — that specific a11y fix belongs
      to a later, separate piece of work (Phase 3, Task 3.3.3) since it's a
      page-specific WCAG finding, but since you're touching this exact JSX
      for the visual fix, adding `Input.Wrapper` with "Type"/"Status" labels
      here too is more efficient than a second pass — do it if convenient.

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
      "More filters" disclosure. This is about *execution* (saying the
      caveat once vs. three times), not about removing it.

### Task 1.14: Firefox sleeper-rule + font-delivery verification (§2.15)

**Severity:** minor. **Blast radius:** every route, Firefox only.
**Files:** `frontend/app/globals.css` (the dashed "sleeper" divider rule —
currently likely a `border`/`background` with dot/dash styling that Gecko
renders sparsely), font loading (check `frontend/app/layout.tsx` or a
`next/font` config for whether the app's rounder sans is actually served as
a webfont).

**Do this task before Task 1.2 if you're able to reorder locally for your
own convenience** — font-delivery affects Gecko's text metrics, which is
also what makes Task 1.2's nav-wrap threshold knife-edge in Firefox. If
you've already landed Task 1.2 by the time you reach this task, that's
fine; just re-verify Task 1.2's Firefox nav-wrap check after this task's
font fix lands, in case metrics shifted.

- [ ] Replace the divider with a `repeating-linear-gradient` with an
      explicit `background-size`, or an inline SVG pattern — both render
      identically in Chromium and Firefox, unlike the current rule.
- [ ] Investigate whether the app's typeface is delivered as a webfont at
      all — every Firefox capture in the source review renders in a
      Helvetica/Arial-class fallback while Chromium renders the intended
      font, which likely means the font isn't being served as a webfont and
      Firefox is showing what a Windows/Android user without it installed
      sees, not a Firefox bug. **Verify this first** — if confirmed, add the
      font via `next/font/local` (or equivalent) so delivery no longer
      depends on the host having it installed.

### Task 1.15: Chrome-control consistency cluster (§2.16, remaining items after §2.3 already resolves the "A" badge)

**Severity:** minor (aggregate — individually trivial items that together
make the shared chrome look unfinished). **Blast radius:** nav auth
controls, footer, several anonymous-CTA pages, offline banner.
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
      button treatment plus a `title`/helper note that it needs an account —
      flag this in your report as a visual-weight decision across ~6 pages,
      not a single component fix.
- [ ] Offline banner: rewrite copy on form pages (`/track`, `/trains`,
      `/track/mine/add-ticket`, `/lines/new`, `/lines/[id]/edit`) where "Can't
      reach live data right now — showing the last update." is untrue (there
      is no update to show on a form) — on those routes say something about
      entry safety instead (e.g. "Can't reach the server right now — your
      entries are safe until you submit."). Give the fixed wrapper
      `width: min(calc(100vw - 32px), 480px)` so it doesn't shrink to ~200px
      and wrap to four lines on a 390px phone.
