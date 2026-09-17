# UX Fixes — Phase 2: Fixture Data Corruption Cleanup

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development
> to work this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for
> tracking.
>
> This is a standalone slice of the full plan at
> `docs/superpowers/plans/2026-09-17-full-service-ux-accessibility-fixes.md`
> (its Phase 2), split out so it can run in its own isolated worktree in
> parallel with that plan's Phase 0 and Phase 1. Fully independent of both —
> a test-fixture data-quality bug, not application code.

**Goal:** fix literal `$34`/`$35`/`$36`/bare-`.` React-Server-Component
de-duplication-reference artifacts that leaked into 11 of 31 station
accessibility fixture files and are rendering as real station info, plus a
related `isEmptyRenderable` gap for punctuation-only values.

**Architecture:** frontend test fixtures + one shared rendering-path helper
function. No backend changes.

**Tech Stack:** Next.js 16 App Router + TypeScript, Vitest 2.

**Specs:**
- `docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md`
  §4.3 — the finding this task fixes.
- `docs/superpowers/specs/2026-09-17-full-service-ux-accessibility-usability-review.md`
  §3.5 — names the related `N/A`-as-a-fact finding fixed alongside.

---

## Global Constraints

- Fix the fixtures from the real source (`GET /public/stations/{crs}/accessibility`
  or the database) — **not** from a rendered page, which is how these
  artifacts leaked in originally.
- `frontend/test/fixtures/accessibility/*.json` fixtures back real regression
  tests (including one asserting "no raw node across 31 real payloads") —
  after this fix, that assertion must still pass AND now actually mean what
  it claims.
- Testing: `npm test` from `frontend/` must pass.

---

## Phase 2 — Fixture data corruption cleanup

### Task 2.1: Fix the `$34`/`$35`/`$36` RSC-reference artifacts in fixtures

**Severity:** serious (fixture). **Files:** the 11 affected files under
`frontend/test/fixtures/accessibility/` (`BAL.json` ×3, `BHM.json` ×2,
`BTN.json` ×4, `EDB.json` ×1, `EUS.json` ×4, `HUL.json` ×1, `KGX.json` ×3,
`LDS.json` ×1, `MAN.json` ×1, `STP.json` ×1, `WVH.json` ×1 — confirmed via
`grep -rlE '"\$[0-9a-f]{1,3}"' frontend/test/fixtures/accessibility/`), and
`.devdata/seed.sql` (carries `$34`–`$38` and `$e` — note: `.devdata/` is a
local dev scratch dir for this session's screenshot sweep, not tracked by
git; fix it too if present, but the tracked fixture files are the actual
deliverable).

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
      `frontend/lib` for this function) — the review notes `N/A` is real feed
      junk, not a scraping artifact, so this is a second, smaller fix in the
      same area but a different cause; land both together since they touch
      the same rendering path.
- [ ] Verify with `npm test` that "no raw node across 31 real payloads" (the
      existing regression test) and every other fixture-driven assertion
      still pass, and now actually mean what they claim to mean.
