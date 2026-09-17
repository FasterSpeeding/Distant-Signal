# UX Fixes — Phase 0: Verification Tasks

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development
> to work this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for
> tracking.
>
> This is a standalone slice of the full plan at
> `docs/superpowers/plans/2026-09-17-full-service-ux-accessibility-fixes.md`
> (its Phase 0), split out so it can run in its own isolated worktree in
> parallel with that plan's Phase 1 and Phase 2. These are NOT UI fixes —
> each is "find out what's actually true first," per the source review's own
> framing, most urgently for Task 0.1, where the review explicitly says its
> own evidence (fixture data) is not trustworthy until Task 0.2 lands.
>
> **Task 0.2 should land before Task 0.1** — a corrected fixture removes one
> possible confound (a fixture bug masquerading as a formatting bug) before
> spending time reproducing against production data. Tasks 0.3, 0.4, 0.5,
> 0.6 are independent of 0.1/0.2 and of each other — do them in any order
> after or alongside 0.1/0.2.
>
> **This worktree does not depend on the parallel Phase 1 or Phase 2
> worktrees**, and nothing in Phase 1/Phase 2 depends on this one landing
> first — a later merge step reconciles all three. If a task here finds a
> real bug whose fix belongs in a file Phase 1 is also touching (flagged
> per-task below), file it as a note in your final report rather than
> touching that file yourself — the parent session will sequence it.

**Goal:** resolve six correctness questions the source review could only
raise, not settle, from static screenshots alone: a suspected BST/UTC
time-offset bug, a fixture wire-shape mismatch that means the train
timetable-overlay feature was never actually exercised by the screenshot
sweep, a dropped-whitespace rendering bug, a Suspense-never-resolves report,
an unhandled promise rejection, and an unexplained per-route colour
inconsistency.

**Architecture:** mostly Rust backend (`crates/api/src/data/journey.rs`) and
frontend investigation; two small frontend defensive fixes.

**Tech Stack:** Next.js 16 App Router + TypeScript, Vitest 2, Rust
(`crates/api`), `cargo test`.

**Specs:**
- `docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md`
  §4 — the nine correctness bugs; this plan implements verification/fixes
  for six of them (§4.1, §4.2, §4.4, §4.7, §4.8, §4.9). The other three
  (§4.5, §4.6) are fixed directly in the parent plan's Phase 3, and §4.3 is
  the sibling Phase 2 fixture-corruption plan.

---

## Global Constraints

- **Do not fix anything about train timetable/progress *content* speculatively.**
  Task 0.1/0.2's whole point is that the review's own evidence for those
  findings may be an artifact of the fixture bug — establish the truth
  first, then file a scoped follow-up rather than guessing at a UI fix here.
- **Testing/build commands** (run from `frontend/`): `npm test` (Vitest).
  Task 0.2 additionally needs `cargo test` in `crates/api` for its seed-shape
  assertion — this is the one task in this phase that touches Rust.
- Where a task's outcome is "no code fix needed, close the finding" — that
  is a valid, complete outcome. Do not invent a fix for a finding that
  didn't reproduce.

---

## Phase 0 — Verification tasks

### Task 0.1: Reproduce the BST/UTC train-timetable offset against real data

**Severity:** serious, needs live verification (review §4.1).
**Depends on:** Task 0.2 landing first (see note above).
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
      `crates/api` data, not `.devdata/seed.sql`). If no real deployment is
      reachable from this worktree, use the corrected fixture from Task 0.2
      as the best available substitute and note that limitation in your
      report.
- [ ] Compare the header's departure/ETA against the timetable row for the
      same instant, the same way the review did (`train-uid-date` capture).
- [ ] If the offset reproduces: it's a real bug in the API → wire → parse
      path (most likely `RawCallingPoint`'s naive-time handling in
      `journey.rs`, or a `london_to_utc` call missing on one of the two
      code paths) — **do not fix it in this worktree**; report the exact
      layer identified so the parent session can file it into the parent
      plan's Phase 3 Track/Train section (Task 3.6.0).
- [ ] If it does not reproduce: the offset was entirely an artifact of Task
      0.2's fixture mismatch (naive London wall-clock times fed as
      already-zoned UTC instants) — record that finding; no application fix
      needed.
- [ ] Either way, add the two regression tests the review recommends: a
      backend unit test asserting
      `calling_points[0].scheduled_departure == pin_scheduled_departure` for
      a known train (`crates/api/src/data/journey.rs` or its test module),
      and a frontend test rendering one instant through both the header and
      the table asserting the two strings are equal (colocate with
      `frontend/app/train/[uid]/[date]/page.test.tsx` or `JourneyTimeline.test.tsx`).
      These tests are the deliverable of this task even if the underlying
      bug isn't fixed here.

### Task 0.2: Fix the seed fixture's `calling_points` wire-shape mismatch

**Severity:** serious (fixture), review §4.2.
**Independent of Task 0.1's outcome** — this needs fixing regardless, since
`.devdata/seed.sql` currently produces `could not build journey stops` server
logs (`crates/api/src/routes/train.rs:966`) and every train-page capture in
the original sweep is of a degraded fallback, not the real timetable-overlay
feature.
**Files:**
- `.devdata/seed.sql` (the `calling_points` JSON blob at/around line 85,
  `INSERT INTO trains` at line 82) — note this file is local dev scratch,
  not git-tracked; if it's absent in this worktree, recreate the minimal
  seed insert needed to exercise `build_journey_stops`, or scope this task
  to the `crates/api` test fixture instead and note the gap.
- `.devdata/gen_seed.py` if the seed is generated rather than hand-written —
  check which before editing.
- `crates/api/src/data/journey.rs` (`RawCallingPoint`, `build_journey_stops`
  — read-only, to confirm the target shape).

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
- [ ] Run `cargo test` in `crates/api`.

### Task 0.3: Investigate the dropped-whitespace-after-JSX-interpolation bug

**Severity:** moderate, needs verification (review §4.4).
**Independent** of every other task in this phase.
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
      report it separately, since the fix is likely in build tooling/Next.js
      config, not component code, and is outside this plan's scope.
- [ ] Regardless of root cause, make the space explicit at both call sites
      (`{count}{' '}may`, `({delayMinutes}{' '}minutes)`) and add a render
      test asserting the exact string — this is a one-line, zero-risk
      defensive fix independent of whatever the toolchain investigation
      finds, and should land even if the toolchain issue takes longer to
      root-cause.

### Task 0.4: Reproduce the mobile-Chromium Suspense-never-resolves report

**Severity:** moderate, needs verification (review §4.7, cross-referenced §2.11).
**Files (read/investigate only — do not fix the Suspense fallback UI here,
that belongs to the parallel Phase 1 worktree's Task 1.10):**
- `frontend/app/lines/[id]/page.tsx:470-489` (the two "Recent trends" Suspense
  boundaries)
- `frontend/components/HalfHourlyTrendsResults.tsx` (the wrapped chart
  component — check for a `matchMedia`/`ResizeObserver` dependency that could
  behave differently on mobile emulation)

**Steps:**
- [ ] Reproduce manually in Chrome device mode (iPhone 14 viewport) against
      a real deployment — load `/lines/[id]`, both light and dark, and watch
      whether the "Recent trends" skeletons resolve.
- [ ] If it resolves normally in manual testing: this was the original
      sweep's own capture-timing artifact (full-page screenshot fired before
      streamed chunks flushed on slower mobile emulation) — no code fix,
      just record that it resolved.
- [ ] If it does not resolve: this is a real hydration bug in the chart
      wrapper — diagnose using superpowers:systematic-debugging, but **do
      not fix it in this worktree** (it touches the same file/Suspense
      boundary as Phase 1's Task 1.10) — report the exact root cause found
      so the parent session can route it into that task.

### Task 0.5: Fix the unhandled rejection on `/lines/[id]/edit` under a mocked network

**Severity:** minor, needs investigation (review §4.8).
**Files:**
- `frontend/app/lines/[id]/not-found.tsx` (the page rendered — a private
  line's edit route, anonymous or non-owner)
- `frontend/components/LineDefinitionTooltip.tsx` if it exists, or wherever
  the `/definition` fetch the review suspects lives — grep for it before
  assuming the file name.

**Steps:**
- [ ] Confirm which client-side call fires on the 404 template under a
      mocked `/api/**` (the review's best guess: a session probe or the
      `LineDefinitionTooltip` fetch, which the private-lines spec says is
      *meant* to fail silently).
- [ ] Wrap the offending fetch in a `.catch()` so a swallowed failure stays
      swallowed rather than becoming an unhandled rejection — this is a
      one-line defensive fix, safe to land directly in this worktree since
      it's isolated to `frontend/app/lines/[id]/not-found.tsx` /
      whatever component owns the fetch, not a file Phase 1 touches.

### Task 0.6: Investigate `LoginLink`'s route-dependent colour

**Severity:** minor, unexplained (review §4.9). Lowest priority in this
phase — do last, or skip if time-constrained, since both colours pass AA
and the only issue is the inconsistency itself.
**Files:** `frontend/components/AuthStatus.tsx` (renders `LoginLink`)

**Steps:**
- [ ] Compare `/lines`, `/lines/[id]`, `/lines/new` (grape "Log in") against
      `/lines/[id]/history` (dark grey "Log in") — find what differs about
      that one route's render tree (a wrapping `Text c=` prop, a CSS
      selector specificity difference, a different `AuthStatus` call site).
- [ ] Once understood, make the colour resolution explicit and consistent —
      either value passes AA, so this is about removing an unexplained
      per-route difference, not about picking a "correct" colour.
