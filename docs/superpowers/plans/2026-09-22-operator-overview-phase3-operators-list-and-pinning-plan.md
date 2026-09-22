# Plan: Operator Overview Phase 3 — Operators List + Operator Pinning

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 3 of
`docs/superpowers/specs/2026-09-22-operator-overview-design.md` (sections C
and E, shipped together per that spec's own phasing recommendation): a new
`GET /public/operators` (+ `GET /public/operators/{code}`) rollup endpoint,
a `pinned_operators` table mirroring `pinned_lines`/`pinned_stations`
exactly, the matching data/route/frontend plumbing, a new
`frontend/app/operators/page.tsx` list page, and a third "Your Operators"
homepage section — structurally identical to "Your Lines"/"Your Stations".

**Scope discipline:** this plan is Phase 3 ONLY. It does not touch Phase 1
(all-lines dashboard, §A), Phase 2 (per-line drill-down, §B), or Phase 4
(historical operator/network views, §D) — those are separate, parallel
efforts. This plan's Task 3 (the operator→lines resolution primitive) is
the one piece Phase 4 will need to build on; see this plan's closing
"Note for Phase 4" section for exactly what it can reuse.

**Architecture:** backend before frontend, same ordering as every other
plan in this repo, since the frontend depends on the new endpoint/columns
existing.

- Task 1 adds one small, genuinely reusable pure function to
  `crates/common` (`merge_sample_stats`) — the "combine several already-
  computed `SampleStats` into one" arithmetic the spec asks C's rollup to
  reuse from `compute_sample_stats`'s averaging logic, extended to operate
  over multiple lines' stats rather than raw departures (no existing
  function does this; `compute_sample_stats` takes raw `StationDeparture`s,
  not pre-computed `SampleStats`, so this is new, small, and pure).
- Task 2 is the `pinned_operators` migration — a byte-for-byte mirror of
  `pinned_lines`/`pinned_stations` as they exist TODAY (already carrying
  the `user_id` ownership column; there is no pre-ownership era to
  retrofit here, since this table is new).
- Task 3 is the actual new logic: `crates/api/src/data/operators.rs`,
  computing "every ATOC code from `tocs` (+ synthetic `TfL`) → its public
  lines → worst severity + merged sample stats," reusing
  `queries::tfl_line_summaries`/`queries::line_status_for_ids` (zero new
  SQL queries needed for status data — see that task's own reasoning for
  why the existing `line_status_for_ids` primitive is exactly the query
  this needs) and `common::severity_rank`/`common::nr_line_id_for_tfl`.
- Task 4 mirrors `preferences.rs`'s `list_pinned_line_ids`/
  `replace_pinned_lines` for operators, literally copying the pattern with
  the column renamed.
- Task 5 is the new `routes/operators.rs` (two GETs), reusing
  `crate::render::sample_stats_json` (`pub(crate)`, already exists) for
  the wire shape — **not** a derived `#[serde(rename_all = "camelCase")]`
  struct embedding `common::SampleStats` directly, which would leak
  `avg_delay_minutes` (snake_case) onto the wire one level down, since
  `rename_all` on an outer struct does not recurse into a nested type that
  has no rename attribute of its own. This is the same reason
  `routes/station_stats.rs`/`crates/api/src/render.rs` hand-build JSON via
  `serde_json::json!` instead of deriving `Serialize` — verified directly
  against that file, not assumed.
- Task 6 extends `routes/preferences.rs`: `PreferencesResponse.pinnedOperators`,
  `PUT /preferences/pinned-operators`, and a `filter_known_pinned_operators`
  read-time hygiene filter mirroring `filter_known_pinned_lines`.
- Tasks 7-13 are the frontend: wire type, `getAllOperators()`, `PinToggle`'s
  widened `PinKind`, a small `formatOperatorSampleSummary` helper (a
  **new**, small formatter — not a forced fit into `formatSampleSummary`'s
  `SampleStatsCarrier` shape, which needs a `sampleAvailability` this
  aggregate genuinely doesn't have one coherent value for; see Judgment
  Call 3), a new `OperatorStatusCard` component reused by both the list
  page and the homepage section (satisfying the spec's explicit "reuse the
  same card" instruction for E), the list page itself, and the homepage's
  new "Your Operators" section plus the `NO_PREFERENCES`/`Preferences`
  widening this change ripples out to (three call sites, not the two the
  task named — see Judgment Call 5).
- Task 14 is the full-stack verify.

**Tech stack:** Rust/axum/sqlx (`crates/api`, `crates/common`), Next.js/
React/Mantine (`frontend`), Postgres.

**Spec:** `docs/superpowers/specs/2026-09-22-operator-overview-design.md`
— authoritative for every architectural decision this plan implements
(schema shape, why custom lines are excluded from the public rollup, why
pinning reuses the existing pattern exactly, why C and E ship together).
This plan does not re-argue anything that spec already settled; it only
resolves the things left as genuine implementation-level judgment calls
(the spec's own Open Questions 1-3, plus a few new ones this plan's own
research surfaced — see below).

---

## Judgment calls this plan makes (read before Task 1)

1. **Wire shape for a rollup's sample stats: reuse `crate::render::sample_stats_json`
   via hand-built `serde_json::json!`, not a derived struct.** Verified
   `crates/api/src/render.rs:97-105`'s `sample_stats_json` exists
   specifically because `common::SampleStats` has no
   `#[serde(rename_all = "camelCase")]` of its own (it derives plain
   `Serialize`, fields `total`/`delayed`/`cancelled`/`skipped`/
   `avg_delay_minutes`) — every existing public route that embeds one
   (`routes/station_stats.rs`, and `render.rs`'s own `to_tfl_shape`) builds
   its JSON by hand for exactly this reason: a parent struct's own
   `rename_all` attribute does not recurse into a nested type's fields.
   `sample_stats_json` is `pub(crate)`, so `routes/operators.rs` (same
   crate) can call it directly — no export change needed. This plan's
   `OperatorRollup` (Task 3, a plain Rust struct with no `Serialize` derive
   at all) is deliberately NOT the wire type; `routes/operators.rs` (Task 5)
   converts it to a `serde_json::Value` by hand, the same way
   `station_stats.rs` converts its own internal rows.

2. **What "an operator" is, resolved exactly as the spec recommends
   (Open Question 2): every row of the `tocs` table (`reference::get_all_tocs`,
   already existing, unchanged) plus one synthetic `"TfL"` row —
   `common::TFL_OPERATOR`.** A line's real, literal ATOC code(s) come
   from the catalogue (`app.config.lines[].operators`) or, for a
   TfL-ingested `line_status` row, are always literally `["TfL"]` (verified
   in `crates/poller-tfl/src/schema.rs:106` and `main.rs:422,443,490` — every
   TfL `LineStatusReport` the poller constructs hardcodes
   `operators: vec!["TfL".to_string()]`, which `queries::upsert_tfl_line_status`
   writes straight into `line_status.operators` unmodified). So: a real ATOC
   code's matching-line set is exactly "catalogue lines whose `operators`
   contains this code" (TfL rows never carry anything but the literal
   string `"TfL"`, so they can never match a real code). The synthetic
   `"TfL"` row's matching-line set is the union of (a) every TfL-ingested
   `line_status` row **that has no NR catalogue counterpart** (`common::nr_line_id_for_tfl`
   returns `None` for its id — otherwise the same real-world railway would
   be double-counted once under its TfL row and once under its NR
   catalogue counterpart, which already carries a real code — see
   `routes/lines.rs`'s own `is_merged_into_nr_line`, which exists for this
   exact reason on `/public/lines`), and (b) every catalogue line whose
   `operators` contains `"LO"` or `"XR"` — the two TfL-adjacent codes
   `frontend/app/lines/AllLinesTable.tsx`'s `TFL_ADJACENT_OPERATORS`/
   `expandOperatorForFiltering` already fold into "TfL" for filtering
   purposes (London Overground catalogue lines carry `operators: ["LO"]`;
   the Elizabeth line catalogue entry carries `["XR"]`). This Rust-side
   constant (Task 3's own `TFL_ADJACENT_OPERATORS`) necessarily duplicates
   the frontend's array of two string literals — there is no
   Rust↔TypeScript shared-constant bridge anywhere in this codebase (the
   same documented gap
   `docs/superpowers/plans/2026-09-05-custom-tracking-names-plan.md`'s
   Judgment Call 1 names for `CUSTOM_NAME_MAX_LENGTH`), and the frontend's
   own copy is intentionally component-local (its own comment says so), so
   there is no existing shared home to lift either copy into. Flagged, not
   silently duplicated.

3. **An operator's aggregate sample stats get their own small formatter
   (`frontend/lib/operatorStats.ts`'s `formatOperatorSampleSummary`), not
   a forced fit into `formatSampleSummary`'s `SampleStatsCarrier` shape.**
   `SampleStatsCarrier` requires a `sampleAvailability: SampleAvailability`
   (`'no-coverage' | 'below-threshold' | 'available'`) and an optional
   `dataQuality`. Both are meaningful for ONE line's ONE status this cycle;
   neither has a coherent single value once several lines with
   independently-varying availability/quality are merged into one rollup
   (an operator running one line currently below-threshold and one line
   currently available has no honest single `SampleAvailability` to report).
   Rather than fabricate one, `OperatorRollup`/the wire `OperatorSummary`
   just carry `sampleStats: Option<SampleStats>` (`None` when no matching
   line has a representative status with any stats — see Task 3's
   `representative_sample_stats`) and a new formatter renders "No
   delay/cancellation data available for this operator." for `None`, with
   one special case: `code === 'TfL'` renders the exact same wording
   `sampleUnavailableReason`'s own TfL branch already uses ("Not measured
   by this app — status is TfL's own."), since a `None` TfL rollup is
   TRUE for the same reason a `None` sample stat is true per-line (TfL
   statuses never carry `sample_stats`, only `dataQuality: 'tfl'` — every
   real ATOC-only rollup's `None` case, by contrast, means "no matching
   line had a representative status with stats yet," a different, genuine
   "not measured" reason `formatOperatorSampleSummary`'s generic message
   covers honestly).

4. **A zero-line operator (a `tocs` row with no line in this catalogue at
   all — plausible for a TOC no longer running anything this app tracks)
   is omitted from `GET /public/operators` entirely, not rendered as an
   empty card.** `build_rollup` (Task 3) returns `None` for an empty
   `matching` slice, and `all_operator_rollups` only pushes `Some` results.
   `GET /public/operators/{code}` for such a code (or any code unknown to
   both `tocs` and `"TfL"`) still 404s (Task 5) — an operator with no
   current lines has nothing an operator page could usefully show, and a
   0-line, 0-status card would be confusing clutter on a list whose whole
   point is "operator X's current status." A **pinned** zero-line
   operator's code is NOT dropped from `preferences.pinnedOperators`
   itself by this decision (Task 6's `filter_known_pinned_operators`
   checks against the full `tocs` code set, not against
   `all_operator_rollups`'s filtered output) — it simply renders nothing
   in the homepage's "Your Operators" grid until that TOC runs a line
   again, the same "pinned but currently nothing to show" outcome a pinned
   line with no `line_status` row yet already gets on today's homepage
   (see `app/page.tsx`'s own comment on `pinnedLineReports`' filter).

5. **`Preferences`/`NO_PREFERENCES` widening touches THREE call sites, not
   the two the task named.** Grepped for every `Preferences`-shaped object
   literal in `frontend/` before starting (`grep -rn "pinnedLines: \[\]\|pinnedStations: \[\]\|: Preferences = {"`)
   and found a third `NO_PREFERENCES` constant at
   `frontend/app/stations/[crs]/page.tsx:51`, not just `app/page.tsx` and
   `app/lines/page.tsx`. All three are widened in Task 13. The same grep
   also surfaced that widening the `Preferences` interface is a **breaking
   TypeScript change for every existing test-mock literal typed against
   it** (`vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [...], pinnedStations: [...] })`,
   ~40 occurrences across `app/page.test.tsx`, `app/stations/[crs]/page.test.tsx`,
   `app/lines/page.test.tsx`, `lib/api.test.ts`, `components/PinToggle.test.tsx`)
   — a missing required property on an object literal assigned to a known
   interface type is always a compile error, not just an excess-property
   warning. Task 13 fixes all of them with one small scripted find/replace
   per file (every occurrence in this codebase has `pinnedStations: [...]`
   as the LAST property before a closing brace, so appending
   `, pinnedOperators: []` right after each `pinnedStations: [...]` match is
   safe and uniform), then leans on `npx tsc --noEmit` to catch anything
   the script missed.

6. **No `?codes=` batch query param on `GET /public/operators`.** The spec
   floated this as one option for the homepage section ("ideally exposed
   as a batchable `GET /public/operators?codes=SW,VT`"). Not built: there
   are only ~25-40 rows in `tocs` total, so `GET /public/operators`
   unfiltered is already a small, cheap response — the homepage section
   (Task 13) fetches the full list once (same call the `/operators` list
   page itself makes, both cached under the same `withStaleFallback` key)
   and filters client-side to `preferences.pinnedOperators`, the EXACT
   pattern `app/page.tsx`'s existing "Your Lines" section already uses
   against the (much larger, ~125-row) `allReports` list. Adding a query
   param would be a second way to ask the same question for no real
   benefit at this scale.

7. **No `frontend/app/operators/[code]/page.tsx` detail page in this
   phase**, even though `GET /public/operators/{code}` (Task 5) is built.
   The task's explicit frontend file list for Phase 3 names only
   `frontend/app/operators/page.tsx` — no detail route. Since there is
   nowhere for a per-operator "view more" link to go yet,
   `OperatorStatusCard` (Task 11) is a plain, non-`Link`-wrapped `Card`
   (unlike `LineStatusCard`, which wraps a `Link` to `/lines/{id}` — there
   is no operator-detail destination to link to today). The detail route
   is built anyway because it is cheap (shares 100% of Task 3's rollup
   logic) and is exactly what a future `/operators/[code]` page — or
   Phase 4 — can consume with zero backend changes; it just has no caller
   in this phase.

8. **`PUT /preferences/pinned-operators` validates nothing on write**,
   mirroring `put_pinned_lines` exactly (not `put_pinned_stations`, which
   has a light "exactly 3 characters" check) — because unlike a station
   CRS, an operator code has no single fixed length invariant to check up
   front (`"TfL"` is 3 characters, every real ATOC code is 2). Hygiene is
   enforced entirely on read, via `filter_known_pinned_operators` (Task 6),
   the same "filter on read, not on write" posture `pinned_lines` already
   uses.

---

## Non-goals

- **Phase 1 (§A, all-lines dashboard) and Phase 2 (§B, per-line
  drill-down) are untouched.** No task here modifies `app/page.tsx`'s
  `notGoodServiceSummary`/`RightNowModule`, `AllLinesTable.tsx`'s existing
  filters, or anything under `app/lines/[id]/`.
- **Phase 4 (§D, historical operator/network views) is not planned or
  built here.** See this plan's closing "Note for Phase 4" section for
  what it inherits.
- **No `operator_status`/materialized rollup table.** Per the spec's own
  §C recommendation, the rollup is computed live on each request — the
  line count per operator is small enough (single-digit to low-teens) that
  this is cheap. Not revisited unless traffic ever makes it a real problem
  (spec Open Question 5).
- **No `/operators/[code]` detail page** (Judgment Call 7).
- **No sort/filter UI on `/operators`** (no operator-column filter, no
  search box) — the list is small (~25-40 rows) and ships in the backend's
  own `tocs.name`-ordered, then-TfL-appended order; nothing in the task
  asked for `/lines`-style filtering here, and adding it would be scope
  creep against an already-large phase.
- **No change to `AllLinesTable.tsx`'s existing operator filter or
  `expandOperatorForFiltering`.** Task 3's Rust `TFL_ADJACENT_OPERATORS` is
  a new, separate copy (Judgment Call 2) — this plan does not touch or
  import from the frontend file.
- **No change to `app/api/[...path]/route.ts`.** Confirmed it is a
  generic catch-all proxying `/api/*` → `${API_BASE_URL}/public/*}` (for
  everything except the already-special-cased `/Train/...` prefix) with
  method/body/cookie passthrough — `/api/operators`,
  `/api/preferences/pinned-operators` need no new special-casing, the same
  conclusion `docs/superpowers/plans/2026-09-05-custom-tracking-names-plan.md`'s
  Non-goals reached for its own new `POST` routes.

## Global Constraints

- **Ownership convention.** `pinned_operators` writes are always
  `user_id`-scoped exactly like `pinned_lines`/`pinned_stations`
  (`replace_pinned_operators` deletes-then-inserts inside one transaction,
  scoped to `user_id`) — there is no separate "doesn't exist vs. isn't
  yours" distinction to make here at all (unlike custom lines/tracked
  trains), since a pin is pure per-user UI state with no shared ownership
  concept.
- **Public rollup excludes custom lines by construction, not by an
  explicit filter.** `data/operators.rs` (Task 3) never queries
  `custom_lines` and never receives a caller identity — the line-id
  universe it ever asks `line_status` about is `app.config.lines`' own ids
  unioned with `queries::tfl_line_summaries`' ids. A private custom line's
  id is in neither set, so it is structurally unreachable from this
  module, not merely filtered out after the fact. Do not "helpfully" widen
  this to accept a caller's own custom lines in a later task without
  re-reading the spec's §0 "Implication for C" and Open Question 1 first.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (matches
  `.github/workflows/ci.yml`'s `clippy` job, `--workspace --all-features`;
  `--all-targets` added locally per this repo's stricter contributor
  default, same as the custom-tracking-names plan's own Testing
  constraint), `cargo test --workspace` (ignored DB-gated tests skipped —
  `.github/workflows/ci.yml:219-220`), and
  `cargo test -p api -- --ignored --test-threads=1` for every DB-gated
  test this plan adds (`.github/workflows/ci.yml:229-230`'s exact
  invocation, against
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres`).
  Frontend: `npm test -- <file>` per changed test file
  (`.github/workflows/ci.yml:268-269`'s `npm test`), plus a full `npm test`,
  `npx tsc --noEmit`, and `npm run build` before considering the frontend
  tasks done (`.github/workflows/ci.yml:271-272`'s `npm run build`).
- **File scope.** Modified/created:
  `crates/common/src/lib.rs`,
  `crates/api/migrations/20260922090000_pinned_operators.sql` (new),
  `crates/api/src/data/mod.rs`,
  `crates/api/src/data/operators.rs` (new),
  `crates/api/src/data/preferences.rs`,
  `crates/api/src/routes/mod.rs`,
  `crates/api/src/routes/operators.rs` (new),
  `crates/api/src/routes/preferences.rs`,
  `frontend/lib/types.ts`,
  `frontend/lib/api.ts`,
  `frontend/lib/api.test.ts`,
  `frontend/lib/operatorStats.ts` (new),
  `frontend/lib/operatorStats.test.ts` (new),
  `frontend/components/PinToggle.tsx`,
  `frontend/components/PinToggle.test.tsx`,
  `frontend/components/OperatorStatusCard.tsx` (new),
  `frontend/components/OperatorStatusCard.test.tsx` (new),
  `frontend/app/operators/page.tsx` (new),
  `frontend/app/operators/page.test.tsx` (new),
  `frontend/app/page.tsx`,
  `frontend/app/page.test.tsx`,
  `frontend/app/lines/page.tsx`,
  `frontend/app/lines/page.test.tsx`,
  `frontend/app/stations/[crs]/page.tsx`,
  `frontend/app/stations/[crs]/page.test.tsx`.
  No other file changes.

---

## Task 1: `crates/common` — `merge_sample_stats`

**Files:** modify `crates/common/src/lib.rs`.

Independent, first task. Nothing else depends on this compiling except
Task 3.

- [ ] **Step 1: Add the function**, directly below `compute_sample_stats`
  (after its closing brace, before `#[cfg(test)] mod compute_sample_stats_tests`,
  `crates/common/src/lib.rs` around line 1325):

```rust
/// Combines several already-computed [`SampleStats`] (one per line) into a
/// single rolled-up `SampleStats` -- the aggregation
/// `api::data::operators::build_rollup` needs to turn "N lines' own
/// representative stats" into "one operator's rollup stats." Weights
/// `avg_delay_minutes` by each input's own running (non-cancelled) count,
/// the same denominator [`compute_sample_stats`] itself averages over, so a
/// multi-line rollup's average agrees with what re-running
/// `compute_sample_stats` over the union of every line's raw departures
/// would have produced -- not a naive unweighted per-line average, which
/// would let a low-volume line's number count as much as a high-volume
/// one's.
///
/// Returns `None` for an empty slice: "no lines had stats to combine" is a
/// real, distinct outcome from "the combined total was zero," and a caller
/// should render the former as "no data" rather than a claimed 0-delay,
/// 0%-cancelled figure.
pub fn merge_sample_stats(all: &[SampleStats]) -> Option<SampleStats> {
    if all.is_empty() {
        return None;
    }
    let total: usize = all.iter().map(|s| s.total).sum();
    let delayed: usize = all.iter().map(|s| s.delayed).sum();
    let cancelled: usize = all.iter().map(|s| s.cancelled).sum();
    let skipped: usize = all.iter().map(|s| s.skipped).sum();
    let running = total.saturating_sub(cancelled);
    let avg_delay_minutes = if running == 0 {
        0.0
    } else {
        all.iter()
            .map(|s| s.avg_delay_minutes * s.total.saturating_sub(s.cancelled) as f64)
            .sum::<f64>()
            / running as f64
    };

    Some(SampleStats {
        total,
        delayed,
        cancelled,
        skipped,
        avg_delay_minutes,
    })
}

#[cfg(test)]
mod merge_sample_stats_tests {
    use super::*;

    fn stats(total: usize, delayed: usize, cancelled: usize, skipped: usize, avg: f64) -> SampleStats {
        SampleStats {
            total,
            delayed,
            cancelled,
            skipped,
            avg_delay_minutes: avg,
        }
    }

    #[test]
    fn an_empty_slice_is_none() {
        assert_eq!(merge_sample_stats(&[]), None);
    }

    #[test]
    fn a_single_input_is_returned_unchanged() {
        let s = stats(10, 2, 1, 0, 4.5);
        assert_eq!(merge_sample_stats(&[s.clone()]), Some(s));
    }

    #[test]
    fn counts_sum_and_avg_delay_is_weighted_by_running_count() {
        // Line A: 8 running (10 total, 2 cancelled), avg delay 10.0.
        // Line B: 2 running (2 total, 0 cancelled), avg delay 0.0.
        // Weighted avg = (8*10.0 + 2*0.0) / 10 = 8.0, NOT the naive
        // unweighted (10.0 + 0.0) / 2 = 5.0 a per-line average would give.
        let a = stats(10, 5, 2, 1, 10.0);
        let b = stats(2, 0, 0, 0, 0.0);
        let merged = merge_sample_stats(&[a, b]).unwrap();
        assert_eq!(merged.total, 12);
        assert_eq!(merged.delayed, 5);
        assert_eq!(merged.cancelled, 2);
        assert_eq!(merged.skipped, 1);
        assert_eq!(merged.avg_delay_minutes, 8.0);
    }

    #[test]
    fn every_input_fully_cancelled_gives_zero_avg_delay_not_a_division_by_zero() {
        let a = stats(3, 0, 3, 0, 0.0);
        let merged = merge_sample_stats(&[a]).unwrap();
        assert_eq!(merged.avg_delay_minutes, 0.0);
    }
}
```

- [ ] **Step 2: Verify**

```bash
cargo test -p common merge_sample_stats
cargo build -p common
```

  Expected: all 4 new tests pass; crate builds clean.

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "common: add merge_sample_stats, combining several lines' SampleStats into one rollup"
```

---

## Task 2: Migration — `pinned_operators`

**Files:** create `crates/api/migrations/20260922090000_pinned_operators.sql`.

Independent of Task 1. `20260922090000` sorts after the latest existing
migration (`20260917090000_incidents_affected_lines.sql`, confirmed via
`ls crates/api/migrations | sort | tail`).

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- pinned_operators: per-user operator pins backing the homepage's "Your
-- Operators" section and /operators' PinToggle. See
-- docs/superpowers/specs/2026-09-22-operator-overview-design.md §C/§E and
-- docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md.
--
-- Exact mirror of pinned_lines/pinned_stations AS THEY EXIST TODAY -- i.e.
-- already carrying the user_id ownership column and composite primary key
-- 20260828100000_add_ownership.sql retrofitted onto those two tables. This
-- table has no "pre-ownership" era to retrofit: operator pinning is new as
-- of this migration, so it is created directly in its final shape.
--
-- operator_code is TEXT, not CHAR(2) (unlike pinned_stations.crs, always
-- exactly 3 characters): a pinned code is either a real ATOC code
-- (tocs.atoc_code, 2 characters) or the literal synthetic string "TfL" (3
-- characters, common::TFL_OPERATOR) -- the column's two valid shapes are
-- different lengths, so a fixed-width CHAR column would not fit both.
-- -------------------------------------------------------------------------

CREATE TABLE pinned_operators (
    user_id        TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    operator_code  TEXT        NOT NULL,
    pinned_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, operator_code)
);
```

- [ ] **Step 2: Verify.** Migrations run automatically against
  `DATABASE_URL` on `cargo test`/`cargo run` startup for this crate:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
psql "$DATABASE_URL" -c "\d pinned_operators"
```

  Expected: no `sqlx::migrate::MigrateError`; `\d pinned_operators` shows
  `user_id | text`, `operator_code | text`, `pinned_at | timestamptz`,
  primary key `(user_id, operator_code)`.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260922090000_pinned_operators.sql
git commit -m "api: add pinned_operators table"
```

---

## Task 3: `crates/api/src/data/operators.rs` — the rollup primitive

**Files:** create `crates/api/src/data/operators.rs`; modify
`crates/api/src/data/mod.rs`.

Depends on Task 1 (`common::merge_sample_stats`) and Task 2 only for the
crate to build/migrate together; this task's own logic needs neither
directly (it reads `line_status`/`tocs`, not `pinned_operators`).

This is the biggest net-new piece of Phase 3 — no existing backend
primitive resolves "operator → its lines" today (spec §0's own finding).
The design below reuses two existing query functions verbatim
(`queries::tfl_line_summaries`, `queries::line_status_for_ids`) rather than
writing any new SQL: the "public line universe" (catalogue ids ∪
unmerged-TfL ids) is exactly the id set `queries::line_status_for_ids`
already knows how to fetch in one query.

- [ ] **Step 1: Register the module** in `crates/api/src/data/mod.rs`
  (alphabetically, after `notifier_forward_queue`, before `preferences`):

```rust
pub mod notifier_forward_queue;
pub mod operators;
pub mod preferences;
```

- [ ] **Step 2: Write `crates/api/src/data/operators.rs`**

```rust
//! Per-operator rollups for `GET /public/operators` (+ `/{code}` detail):
//! for each real ATOC code (`tocs.atoc_code`) plus a synthetic `"TfL"` row,
//! find every PUBLIC line whose `operators` carries that code and roll up
//! worst severity + merged sample stats across them. See
//! docs/superpowers/specs/2026-09-22-operator-overview-design.md §C/§E and
//! docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md
//! (Judgment Calls 2 and 4 in particular).
//!
//! Custom lines are excluded BY CONSTRUCTION, not by an explicit filter:
//! the only line-id universe this module ever asks `line_status` about is
//! `app.config.lines`' own ids (the static catalogue) unioned with the ids
//! `queries::tfl_line_summaries` returns (TfL-ingested rows) -- a private
//! custom line's id is in neither set, so it is never looked up, never
//! joined, never summed. See the design spec's §0 "Implication for C" and
//! Open Question 1.

use std::collections::HashMap;

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::PgPool;

use crate::data::{queries, reference};

/// ATOC codes that are their own line's real operator code but should ALSO
/// fold into the synthetic `"TfL"` rollup -- mirrors
/// `frontend/app/lines/AllLinesTable.tsx`'s own
/// `TFL_ADJACENT_OPERATORS`/`expandOperatorForFiltering` exactly
/// (one-directional: a London Overground ("LO") or Elizabeth line ("XR")
/// catalogue line ALSO counts toward "TfL"'s rollup, but requesting "LO" or
/// "XR" on their own still means just that code). Necessarily a separate
/// Rust-side copy of that frontend array -- see this plan's Judgment
/// Call 2 for why no shared constant exists to pull this from instead.
const TFL_ADJACENT_OPERATORS: [&str; 2] = ["LO", "XR"];

/// One operator's rolled-up current status across every public line it
/// runs. `line_ids` is deliberately public on this type, not just an
/// internal detail -- it is the exact "which lines does operator X run"
/// primitive a future Phase 4 (per-operator historical trends) needs to
/// scope its own `line_status_daily_stats`/half-hourly queries; see this
/// plan's closing "Note for Phase 4."
#[derive(Debug, Clone, PartialEq)]
pub struct OperatorRollup {
    pub code: String,
    pub name: String,
    pub line_ids: Vec<String>,
    pub worst_severity: common::Severity,
    pub reason: String,
    pub sample_stats: Option<common::SampleStats>,
    /// The OLDEST `computed_at` across every matching line -- an aggregate
    /// is only as fresh as its stalest input, so this (not the newest) is
    /// the honest "how out of date could this rollup be" signal. Always
    /// `Some` in practice: [`build_rollup`] only ever returns `Some` for a
    /// non-empty `matching` slice, and every row `line_status_for_ids`
    /// returns carries a real `computed_at`.
    pub computed_at: Option<DateTime<Utc>>,
}

/// Every operator with at least one public line, in `tocs.name` order
/// (`reference::get_all_tocs`'s own `ORDER BY name`) plus a trailing
/// synthetic `"TfL"` row. An operator from `tocs` with zero matching lines
/// is omitted entirely -- see this plan's Judgment Call 4 for why an empty
/// rollup card would be worse than no card at all.
pub async fn all_operator_rollups(
    pool: &PgPool,
    catalogue_lines: &[common::LineDefinition],
) -> Result<Vec<OperatorRollup>> {
    let tocs = reference::get_all_tocs(pool).await?;
    let rows = public_line_status_rows(pool, catalogue_lines).await?;

    let mut out = Vec::with_capacity(tocs.len() + 1);
    for toc in &tocs {
        let matching: Vec<&queries::LineStatusRow> = rows
            .iter()
            .filter(|row| row.operators.iter().any(|op| op == &toc.code))
            .collect();
        if let Some(rollup) = build_rollup(toc.code.clone(), toc.name.clone(), &matching) {
            out.push(rollup);
        }
    }

    let tfl_matching: Vec<&queries::LineStatusRow> = rows
        .iter()
        .filter(|row| {
            row.operators.iter().any(|op| op == common::TFL_OPERATOR)
                || row
                    .operators
                    .iter()
                    .any(|op| TFL_ADJACENT_OPERATORS.contains(&op.as_str()))
        })
        .collect();
    if let Some(rollup) = build_rollup(
        common::TFL_OPERATOR.to_string(),
        common::TFL_OPERATOR.to_string(),
        &tfl_matching,
    ) {
        out.push(rollup);
    }

    Ok(out)
}

/// Single-operator version of [`all_operator_rollups`], for
/// `GET /public/operators/{code}`. Recomputes the whole list and picks one
/// out rather than a narrower query -- the full list is already small
/// (~25-40 rows) and this keeps the "how a code resolves to a rollup"
/// logic in exactly one place. Returns `None` for a code with zero
/// matching lines (same omission as the list) or one that resolves to
/// neither a real `tocs` row nor `"TfL"`.
pub async fn operator_rollup(
    pool: &PgPool,
    catalogue_lines: &[common::LineDefinition],
    code: &str,
) -> Result<Option<OperatorRollup>> {
    Ok(all_operator_rollups(pool, catalogue_lines)
        .await?
        .into_iter()
        .find(|r| r.code == code))
}

/// Fetches every `line_status` row for the "public" line universe this
/// module rolls operators up over: the static catalogue
/// (`app.config.lines`) plus TfL-ingested lines that have no NR catalogue
/// counterpart. A merged TfL row (e.g. `tfl-elizabeth`) is excluded here so
/// it is never double-counted alongside its catalogue counterpart, which
/// already carries the real ATOC-style code (`elizabeth-line`'s own
/// `operators: ["XR"]`) -- see `common::nr_line_id_for_tfl`, the same
/// exclusion `routes::lines::list_lines`'s `is_merged_into_nr_line` applies
/// on `/public/lines` for the identical reason. Private custom lines are
/// never in this universe at all -- their ids are in neither input set, so
/// this function never queries for them.
async fn public_line_status_rows(
    pool: &PgPool,
    catalogue_lines: &[common::LineDefinition],
) -> Result<Vec<queries::LineStatusRow>> {
    let tfl = queries::tfl_line_summaries(pool).await?;
    let mut ids: Vec<String> = catalogue_lines.iter().map(|l| l.id.clone()).collect();
    ids.extend(
        tfl.into_iter()
            .filter(|line| common::nr_line_id_for_tfl(&line.id).is_none())
            .map(|line| line.id),
    );
    queries::line_status_for_ids(pool, &ids).await
}

/// Rolls up one operator's matching lines into an [`OperatorRollup`], or
/// `None` if `matching` is empty (Judgment Call 4: a zero-line operator is
/// omitted, not emitted as an empty card).
fn build_rollup(
    code: String,
    name: String,
    matching: &[&queries::LineStatusRow],
) -> Option<OperatorRollup> {
    if matching.is_empty() {
        return None;
    }

    let mut worst_severity = common::Severity::GoodService;
    let mut reason = String::new();
    let mut representative_stats: Vec<common::SampleStats> = Vec::new();
    let mut computed_at: Option<DateTime<Utc>> = None;

    for row in matching {
        for status in &row.statuses {
            // `>=`, not `>`: on a tie, the LAST status encountered wins,
            // which is fine -- there is no meaningful ordering preference
            // between two equally-severe statuses from different lines.
            // Ranked via `common::severity_rank`, NOT a raw `Severity`
            // comparison/`.min()` -- `Severity`'s derived `Ord` sorts by
            // discriminant, which is non-monotonic with true severity (see
            // `severity_rank`'s own doc comment; `Diverted`/`PartClosed`
            // are numerically high but genuinely severe).
            if common::severity_rank(status.severity) >= common::severity_rank(worst_severity) {
                worst_severity = status.severity;
                reason = status.reason.clone();
            }
        }
        if let Some(stats) = representative_sample_stats(&row.statuses) {
            representative_stats.push(stats);
        }
        computed_at = Some(match computed_at {
            Some(existing) if existing <= row.computed_at => existing,
            _ => row.computed_at,
        });
    }

    Some(OperatorRollup {
        code,
        name,
        line_ids: matching.iter().map(|r| r.id.clone()).collect(),
        worst_severity,
        reason,
        sample_stats: common::merge_sample_stats(&representative_stats),
        computed_at,
    })
}

/// The same "which status on this line is representative" precedence
/// `frontend/lib/sampleStats.ts`'s `representativeStatus` already applies
/// per report -- ported here so an operator's rollup sums the SAME numbers
/// a single line's own card would show, not a second, disagreeing
/// selection. Prefers a status carrying `full_coverage_stats`, then one
/// carrying `sample_stats`, else `None` -- unlike the frontend version
/// (which falls back to "the first status regardless" so it always has
/// SOMETHING to render a `reason`/`dataQuality` from), this fallback only
/// feeds the numeric rollup, and there is nothing useful to average in
/// once neither stats field is present on any status.
fn representative_sample_stats(statuses: &[common::LineStatus]) -> Option<common::SampleStats> {
    statuses
        .iter()
        .find_map(|s| s.full_coverage_stats.clone())
        .or_else(|| statuses.iter().find_map(|s| s.sample_stats.clone()))
}

#[cfg(test)]
mod build_rollup_tests {
    use super::*;
    use common::{DataQuality, LineStatus, SampleAvailability, SampleStats, Severity, ValidityPeriod};

    fn validity() -> ValidityPeriod {
        ValidityPeriod {
            from_date: Utc::now(),
            to_date: None,
            is_now: true,
        }
    }

    fn status(severity: Severity, reason: &str, stats: Option<SampleStats>) -> LineStatus {
        LineStatus {
            severity,
            reason: reason.to_string(),
            validity: validity(),
            disruption: None,
            data_quality: DataQuality::Knowledgebase,
            sample_stats: stats,
            sample_availability: SampleAvailability::NoCoverage,
            full_coverage_stats: None,
            full_coverage_availability: common::FullCoverageAvailability::NotEnabled,
        }
    }

    fn row(id: &str, operators: &[&str], statuses: Vec<LineStatus>) -> queries::LineStatusRow {
        queries::LineStatusRow {
            id: id.to_string(),
            name: id.to_string(),
            mode_name: "national-rail".to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            statuses,
            computed_at: Utc::now(),
        }
    }

    #[test]
    fn an_empty_matching_slice_is_none() {
        assert_eq!(build_rollup("SW".to_string(), "South Western Railway".to_string(), &[]), None);
    }

    #[test]
    fn worst_severity_uses_severity_rank_not_raw_discriminant_ordering() {
        // Diverted (discriminant 21) is numerically higher than MinorDelays
        // (discriminant 9) but is genuinely more severe -- the exact
        // non-monotonic case severity_rank exists to get right. A `.min()`
        // over raw `Severity` would pick MinorDelays; this must pick
        // Diverted.
        let a = row("line-a", &["SW"], vec![status(Severity::MinorDelays, "minor", None)]);
        let b = row("line-b", &["SW"], vec![status(Severity::Diverted, "diverted", None)]);
        let rollup = build_rollup("SW".to_string(), "South Western Railway".to_string(), &[&a, &b]).unwrap();
        assert_eq!(rollup.worst_severity, Severity::Diverted);
        assert_eq!(rollup.reason, "diverted");
    }

    #[test]
    fn line_ids_and_computed_at_reflect_every_matching_line() {
        let mut a = row("line-a", &["SW"], vec![status(Severity::GoodService, "", None)]);
        a.computed_at = Utc::now() - chrono::Duration::hours(2);
        let b = row("line-b", &["SW"], vec![status(Severity::GoodService, "", None)]);
        let rollup = build_rollup("SW".to_string(), "South Western Railway".to_string(), &[&a, &b]).unwrap();
        assert_eq!(rollup.line_ids, vec!["line-a".to_string(), "line-b".to_string()]);
        // Oldest, not newest -- Judgment Call in this module's own doc
        // comment on `computed_at`.
        assert_eq!(rollup.computed_at, Some(a.computed_at));
    }

    #[test]
    fn sample_stats_merge_across_lines_using_the_representative_status_per_line() {
        let a = row(
            "line-a",
            &["SW"],
            vec![status(Severity::GoodService, "", Some(SampleStats { total: 10, delayed: 1, cancelled: 0, skipped: 0, avg_delay_minutes: 2.0 }))],
        );
        let b = row(
            "line-b",
            &["SW"],
            vec![status(Severity::GoodService, "", Some(SampleStats { total: 5, delayed: 0, cancelled: 0, skipped: 0, avg_delay_minutes: 0.0 }))],
        );
        let rollup = build_rollup("SW".to_string(), "South Western Railway".to_string(), &[&a, &b]).unwrap();
        let stats = rollup.sample_stats.unwrap();
        assert_eq!(stats.total, 15);
        assert_eq!(stats.delayed, 1);
    }

    #[test]
    fn a_line_with_no_stats_on_any_status_contributes_no_sample_stats() {
        let a = row("line-a", &["SW"], vec![status(Severity::GoodService, "", None)]);
        let rollup = build_rollup("SW".to_string(), "South Western Railway".to_string(), &[&a]).unwrap();
        assert_eq!(rollup.sample_stats, None);
    }
}
```

  Note: `common::LineStatus`/`ValidityPeriod`/`SampleAvailability`/
  `FullCoverageAvailability` field names above must match the real current
  struct definitions exactly (verified against
  `crates/common/src/lib.rs:335-399` while writing this plan) — if `cargo
  build` reports a field mismatch, re-check that file, not this plan; the
  struct shapes are copied from direct reads of the current source, not
  memorized.

- [ ] **Step 3: Verify**

```bash
cargo test -p api build_rollup_tests
cargo build -p api
```

  Expected: all 5 new tests pass; crate builds clean (this task adds no DB
  I/O of its own beyond calling two already-existing, already-tested query
  functions, so no `#[ignore]`d DB test is added here — Task 5's route
  tests are what exercise this module against a real database end to end).

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/mod.rs crates/api/src/data/operators.rs
git commit -m "api: add data::operators, the operator-to-lines rollup primitive"
```

---

## Task 4: `crates/api/src/data/preferences.rs` — pinned-operators data layer

**Files:** modify `crates/api/src/data/preferences.rs`.

Independent of Tasks 1/3; depends only on Task 2's migration for the
table to exist. Literally copies `list_pinned_line_ids`/
`replace_pinned_lines` with the column renamed, per the spec's own
instruction for E.

- [ ] **Step 1: Add `list_pinned_operator_codes`**, directly below
  `list_pinned_station_crs`:

```rust
pub async fn list_pinned_operator_codes(pool: &PgPool, user_id: &str) -> Result<Vec<String>> {
    let rows = sqlx::query(
        "SELECT operator_code FROM pinned_operators WHERE user_id = $1 ORDER BY pinned_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    rows.into_iter()
        .map(|row| Ok(row.try_get("operator_code")?))
        .collect()
}
```

- [ ] **Step 2: Add `replace_pinned_operators`**, directly below
  `replace_pinned_stations`:

```rust
/// Same replace-whole-set semantics as `replace_pinned_lines`/
/// `replace_pinned_stations` -- delete-all-then-insert-all in one
/// transaction, scoped to `user_id`.
pub async fn replace_pinned_operators(pool: &PgPool, user_id: &str, codes: &[String]) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM pinned_operators WHERE user_id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    for code in codes {
        sqlx::query(
            "INSERT INTO pinned_operators (user_id, operator_code, pinned_at) VALUES ($1, $2, NOW())",
        )
        .bind(user_id)
        .bind(code)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
```

- [ ] **Step 3: Add a DB-gated round-trip test**, in a new `#[cfg(test)] mod db_tests`
  at the bottom of the file (this file currently has no test module at
  all — confirmed by reading it in full):

```rust
#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                replace_pinned_operators_then_list_pinned_operator_codes_round_trips \
                -- --ignored`"]
    async fn replace_pinned_operators_then_list_pinned_operator_codes_round_trips() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-PINNED-OPERATORS-USER', 'test@example.com', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        replace_pinned_operators(&pool, "TEST-PINNED-OPERATORS-USER", &["SW".to_string(), "TfL".to_string()])
            .await
            .expect("pin operators");
        let codes = list_pinned_operator_codes(&pool, "TEST-PINNED-OPERATORS-USER")
            .await
            .expect("list pinned operator codes");
        assert_eq!(codes, vec!["SW".to_string(), "TfL".to_string()]);

        // A second replace fully supersedes the first set (delete-then-
        // insert, not merge).
        replace_pinned_operators(&pool, "TEST-PINNED-OPERATORS-USER", &["VT".to_string()])
            .await
            .expect("replace pinned operators");
        let codes = list_pinned_operator_codes(&pool, "TEST-PINNED-OPERATORS-USER")
            .await
            .expect("list pinned operator codes after replace");
        assert_eq!(codes, vec!["VT".to_string()]);

        sqlx::query("DELETE FROM pinned_operators WHERE user_id = 'TEST-PINNED-OPERATORS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-PINNED-OPERATORS-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");
    }
}
```

- [ ] **Step 4: Verify**

```bash
cargo build -p api
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api replace_pinned_operators_then_list_pinned_operator_codes_round_trips -- --ignored --test-threads=1
```

  Expected: builds clean; the DB-gated test passes against a real local
  Postgres.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/preferences.rs
git commit -m "api: add list_pinned_operator_codes/replace_pinned_operators"
```

---

## Task 5: `crates/api/src/routes/operators.rs` — the two new routes

**Files:** create `crates/api/src/routes/operators.rs`; modify
`crates/api/src/routes/mod.rs`.

Depends on Task 3 (`data::operators`). Resolves Judgment Call 1 (hand-built
JSON via `sample_stats_json`, not a derived struct) and Judgment Call 7
(detail route built, no frontend caller yet).

- [ ] **Step 1: Register the module** in `crates/api/src/routes/mod.rs`
  (alphabetically, after `notifications`, before `preferences`), and merge
  its router into `public_router()`:

```rust
pub mod notifications;
pub mod operators;
pub mod preferences;
```

```rust
        .merge(notifications::router())
        .merge(operators::router())
        .merge(preferences::router())
```

- [ ] **Step 2: Write `crates/api/src/routes/operators.rs`**

```rust
//! `GET /public/operators` (+ `/operators/{code}` detail): per-operator
//! status rollups backing the Phase 3 operators list page and operator
//! pinning. Unauthenticated -- same `public_router()` posture as
//! `lines.rs`/`reference.rs` (every real ATOC code, and the synthetic
//! `"TfL"` row, are public reference concepts; nothing here varies by
//! caller identity, unlike `preferences.rs`). See
//! docs/superpowers/specs/2026-09-22-operator-overview-design.md §C and
//! docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::operators;
use crate::render::sample_stats_json;

pub fn router() -> Router {
    Router::new()
        .route("/operators", axum::routing::get(list_operators))
        .route("/operators/{code}", axum::routing::get(get_operator))
}

async fn list_operators(State(app): State<App>) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rollups = operators::all_operator_rollups(&app.database, &app.config.lines)
        .await
        .map_err(internal_error)?;
    Ok(Json(rollups.iter().map(operator_rollup_json).collect()))
}

async fn get_operator(
    State(app): State<App>,
    Path(code): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let rollup = operators::operator_rollup(&app.database, &app.config.lines, &code)
        .await
        .map_err(internal_error)?;
    let Some(rollup) = rollup else {
        return Err((StatusCode::NOT_FOUND, "operator not found".to_string()));
    };
    Ok(Json(operator_rollup_json(&rollup)))
}

/// Hand-built camelCase JSON -- NOT a derived `#[serde(rename_all = "camelCase")]`
/// struct embedding `common::SampleStats` directly, which would leak
/// `avg_delay_minutes` (snake_case) one level down (a parent struct's
/// `rename_all` does not recurse into a nested type with no rename
/// attribute of its own). Same rationale, and the same reused
/// `sample_stats_json` helper, as `routes/station_stats.rs`'s own
/// `get_station_sample_stats` -- see this plan's Judgment Call 1.
fn operator_rollup_json(r: &operators::OperatorRollup) -> Value {
    let mut out = json!({
        "code": r.code,
        "name": r.name,
        "lineIds": r.line_ids,
        "worstSeverity": r.worst_severity as i32,
        "reason": r.reason,
        "computedAt": r.computed_at,
    });
    if let Some(stats) = &r.sample_stats {
        out["sampleStats"] = sample_stats_json(stats);
    }
    out
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "operators rollup query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}

#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Copied verbatim from `crates/api/src/routes/station_stats.rs`'s own
    /// `db_tests::test_app_with_lines_and_full_coverage_default` (that
    /// module's own doc comment: colocated per-file rather than shared,
    /// until a third file needs it too -- this is the second, not the
    /// third). Every field an inert placeholder except `database`/`lines`,
    /// which the caller supplies.
    fn test_app(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            redis_url: "redis://127.0.0.1:0".to_string(),
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
        };

        std::sync::Arc::new(AppState {
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Reserved `Z…` fixture-code namespace, matching this file's own
    /// `station_stats.rs` sibling test convention -- picks a `tocs` code
    /// unlikely to collide with a real ATOC code.
    fn gating_line(id: &str, operator: &str) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: vec![operator.to_string()],
            stations: vec![common::Station {
                crs: "ZZZ".to_string(),
                tiploc: None,
                role: "minor".to_string(),
                segment: None,
            }],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    async fn seed_toc(pool: &PgPool, code: &str, name: &str) {
        sqlx::query(
            "INSERT INTO tocs (atoc_code, name, legal_name, fetched_at) VALUES ($1, $2, $2, NOW()) \
             ON CONFLICT (atoc_code) DO UPDATE SET name = EXCLUDED.name",
        )
        .bind(code)
        .bind(name)
        .execute(pool)
        .await
        .expect("seed fixture toc");
    }

    async fn seed_line_status(pool: &PgPool, line_id: &str, operators: &[&str], statuses: serde_json::Value) {
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ($1, $1, 'national-rail', $2, $3, 'aggregator') \
             ON CONFLICT (line_id) DO UPDATE SET operators = EXCLUDED.operators, statuses = EXCLUDED.statuses",
        )
        .bind(line_id)
        .bind(operators)
        .bind(statuses)
        .execute(pool)
        .await
        .expect("seed fixture line_status row");
    }

    async fn cleanup(pool: &PgPool, toc_code: &str, line_id: &str) {
        sqlx::query("DELETE FROM tocs WHERE atoc_code = $1")
            .bind(toc_code)
            .execute(pool)
            .await
            .expect("cleanup fixture toc");
        sqlx::query("DELETE FROM line_status WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture line_status row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                list_operators_returns_a_rollup_for_a_toc_with_a_matching_line \
                -- --ignored --test-threads=1`"]
    async fn list_operators_returns_a_rollup_for_a_toc_with_a_matching_line() {
        let pool = connect().await;
        seed_toc(&pool, "ZA", "Z Test Rail").await;
        seed_line_status(
            &pool,
            "ztest-operators-line",
            &["ZA"],
            serde_json::json!([{
                "severity": 9, "reason": "minor delays", "validity": {"from_date": "2026-01-01T00:00:00Z", "to_date": null, "is_now": true},
                "data_quality": "knowledgebase", "sample_availability": {"state": "no-coverage"},
                "full_coverage_availability": {"state": "not-enabled"}
            }]),
        )
        .await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), vec![gating_line("ztest-operators-line", "ZA")]));
        let response = router
            .oneshot(Request::builder().uri("/operators").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let za = json
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["code"] == "ZA")
            .expect("ZA rollup present");
        assert_eq!(za["name"], "Z Test Rail");
        assert_eq!(za["lineIds"], serde_json::json!(["ztest-operators-line"]));
        assert_eq!(za["worstSeverity"], 9);

        cleanup(&pool, "ZA", "ztest-operators-line").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_operator_unknown_code_is_404 -- --ignored --test-threads=1`"]
    async fn get_operator_unknown_code_is_404() {
        let pool = connect().await;
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), vec![]));
        let response = router
            .oneshot(Request::builder().uri("/operators/ZZ-NOT-REAL").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                get_operator_a_toc_with_zero_matching_lines_is_404 -- --ignored --test-threads=1`"]
    async fn get_operator_a_toc_with_zero_matching_lines_is_404() {
        // Judgment Call 4: a real tocs row with no matching line is
        // omitted, same as an unknown code.
        let pool = connect().await;
        seed_toc(&pool, "ZB", "Z Ghost Rail").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), vec![]));
        let response = router
            .oneshot(Request::builder().uri("/operators/ZB").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        sqlx::query("DELETE FROM tocs WHERE atoc_code = 'ZB'").execute(&pool).await.expect("cleanup");
    }
}
```

  Note: the `statuses` JSONB fixture in the first test must deserialize
  into `Vec<common::LineStatus>` — verify the exact field names/shapes
  against `crates/common/src/lib.rs`'s current `LineStatus`/`ValidityPeriod`/
  `SampleAvailability`/`FullCoverageAvailability` derive attributes while
  implementing this step (same caveat as Task 3's Step 2) rather than
  trusting this plan's JSON literal blindly if the real struct has since
  changed shape.

- [ ] **Step 3: Verify**

```bash
cargo build -p api
cargo clippy -p api --all-features --all-targets -- -D warnings
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api -- --ignored --test-threads=1
```

  Expected: builds and lints clean; all three new DB-gated tests (plus
  every pre-existing DB-gated test) pass.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/mod.rs crates/api/src/routes/operators.rs
git commit -m "api: add GET /public/operators and GET /public/operators/{code}"
```

---

## Task 6: `crates/api/src/routes/preferences.rs` — pinned-operators routes

**Files:** modify `crates/api/src/routes/preferences.rs`.

Depends on Task 4 (`data::preferences`'s new functions). Resolves Judgment
Call 8 (no write-time validation, mirroring `put_pinned_lines`).

- [ ] **Step 1: Add the route**, in `router()`:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/preferences", axum::routing::get(get_preferences))
        .route(
            "/preferences/pinned-lines",
            axum::routing::put(put_pinned_lines),
        )
        .route(
            "/preferences/pinned-stations",
            axum::routing::put(put_pinned_stations),
        )
        .route(
            "/preferences/pinned-operators",
            axum::routing::put(put_pinned_operators),
        )
}
```

- [ ] **Step 2: Widen `PreferencesResponse`**:

```rust
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreferencesResponse {
    pinned_lines: Vec<String>,
    pinned_stations: Vec<String>,
    pinned_operators: Vec<String>,
}
```

- [ ] **Step 3: Add the import** (top of file, alongside the existing
  `use crate::data::{custom_lines, preferences, queries};`):

```rust
use crate::data::{custom_lines, preferences, queries, reference};
```

- [ ] **Step 4: Extend `get_preferences`**, adding the operators fetch/filter
  and threading it into the returned `PreferencesResponse` (insert after
  the existing `pinned_stations` computation, before the final `Ok(Json(...))`):

```rust
    let pinned_operator_codes = preferences::list_pinned_operator_codes(&app.database, &user.id)
        .await
        .map_err(internal_error)?;
    // Every real ATOC code plus the synthetic "TfL" row is "known" here --
    // unlike `filter_known_pinned_lines`, there is no ownership/visibility
    // distinction to make (an operator code is a public reference concept,
    // not a private or group-scoped resource), so this filter exists purely
    // to drop a stale/foreign code, the same hygiene role
    // `filter_existing_station_crs` plays for stations.
    let tocs = reference::get_all_tocs(&app.database)
        .await
        .map_err(internal_error)?;
    let known_operator_codes = tocs
        .into_iter()
        .map(|t| t.code)
        .chain(std::iter::once(common::TFL_OPERATOR.to_string()));
    let pinned_operators = filter_known_pinned_operators(pinned_operator_codes, known_operator_codes);

    Ok(Json(PreferencesResponse {
        pinned_lines,
        pinned_stations,
        pinned_operators,
    }))
```

- [ ] **Step 5: Add `put_pinned_operators`**, directly below
  `put_pinned_stations`:

```rust
async fn put_pinned_operators(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(codes): Json<Vec<String>>,
) -> Result<StatusCode, (StatusCode, String)> {
    preferences::replace_pinned_operators(&app.database, &user.id, &codes)
        .await
        .map_err(internal_error)?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 6: Add `filter_known_pinned_operators`**, directly below
  `filter_known_pinned_lines`:

```rust
/// Filters `pinned_codes` down to ones that still resolve to a real
/// operator -- a real `tocs` row's code, or the synthetic `"TfL"` row.
/// Unlike `filter_known_pinned_lines`, there is no per-caller visibility
/// distinction: every operator code is public reference data, so a single
/// flat known-codes set (not a caller-scoped one) is correct here.
fn filter_known_pinned_operators(
    pinned_codes: Vec<String>,
    known_codes: impl IntoIterator<Item = String>,
) -> Vec<String> {
    let known: HashSet<String> = known_codes.into_iter().collect();
    pinned_codes
        .into_iter()
        .filter(|code| known.contains(code))
        .collect()
}
```

- [ ] **Step 7: Add unit tests**, alongside the existing `mod tests` block:

```rust
    #[test]
    fn a_pinned_real_operator_code_survives_the_known_codes_filter() {
        let pinned = vec!["SW".to_string()];
        let result = filter_known_pinned_operators(pinned, vec!["SW".to_string(), "VT".to_string()]);
        assert_eq!(result, vec!["SW".to_string()]);
    }

    #[test]
    fn a_pinned_tfl_code_survives_the_known_codes_filter() {
        let pinned = vec!["TfL".to_string()];
        let result = filter_known_pinned_operators(pinned, vec!["TfL".to_string()]);
        assert_eq!(result, vec!["TfL".to_string()]);
    }

    #[test]
    fn a_pinned_code_unknown_to_every_source_is_dropped() {
        let pinned = vec!["ZZ".to_string()];
        let result = filter_known_pinned_operators(pinned, vec!["SW".to_string()]);
        assert!(result.is_empty());
    }
```

- [ ] **Step 8: Add a DB-gated round-trip test**, in the existing
  `mod db_tests` block, alongside its TfL-pinned-line regression test:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_pinned_operator_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip \
                -- --ignored`"]
    async fn a_pinned_operator_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ('TEST-PREFS-OPERATOR-USER', 'test@example.com', 'Test Rider') \
             ON CONFLICT (id) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed fixture user");

        sqlx::query(
            "INSERT INTO tocs (atoc_code, name, legal_name, fetched_at) VALUES ('ZP', 'Z Prefs Rail', 'Z Prefs Rail', NOW()) \
             ON CONFLICT (atoc_code) DO UPDATE SET name = EXCLUDED.name",
        )
        .execute(&pool)
        .await
        .expect("seed fixture toc");

        preferences::replace_pinned_operators(
            &pool,
            "TEST-PREFS-OPERATOR-USER",
            &["ZP".to_string(), "ZZ-UNKNOWN".to_string(), "TfL".to_string()],
        )
        .await
        .expect("pin operators");

        // This test's job is only the write/read round trip through the
        // real table (replace_pinned_operators -> list_pinned_operator_codes),
        // matching the scope
        // a_pinned_tfl_line_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip
        // has for pinned_lines. filter_known_pinned_operators (the "is this
        // code still real" hygiene step get_preferences applies on top of
        // this list) is deliberately NOT re-exercised here -- it needs no
        // database at all and is already covered by this file's own pure
        // unit tests in Step 7. So this list is expected to still contain
        // the unknown code -- that filtering happens one layer up, in the
        // route handler, not in list_pinned_operator_codes itself.
        let pinned_operator_codes = preferences::list_pinned_operator_codes(&pool, "TEST-PREFS-OPERATOR-USER")
            .await
            .expect("list pinned operator codes");

        sqlx::query("DELETE FROM pinned_operators WHERE user_id = 'TEST-PREFS-OPERATOR-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture pins");
        sqlx::query("DELETE FROM tocs WHERE atoc_code = 'ZP'")
            .execute(&pool)
            .await
            .expect("cleanup fixture toc");
        sqlx::query("DELETE FROM users WHERE id = 'TEST-PREFS-OPERATOR-USER'")
            .execute(&pool)
            .await
            .expect("cleanup fixture user");

        assert!(pinned_operator_codes.contains(&"ZP".to_string()));
        assert!(pinned_operator_codes.contains(&"TfL".to_string()));
        assert!(pinned_operator_codes.contains(&"ZZ-UNKNOWN".to_string()));
    }
```

- [ ] **Step 9: Verify**

```bash
cargo build -p api
cargo test -p api filter_known_pinned_operators
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api a_pinned_operator_is_still_returned_by_get_preferences_after_a_real_write_read_round_trip -- --ignored
```

  Expected: builds clean; unit tests pass; DB-gated test passes.

- [ ] **Step 10: Commit**

```bash
git add crates/api/src/routes/preferences.rs
git commit -m "api: add PUT /public/preferences/pinned-operators and thread pinnedOperators through GET /public/preferences"
```

---

## Task 7: `frontend/lib/types.ts` — `OperatorSummary` + widen `Preferences`

**Files:** modify `frontend/lib/types.ts`.

Independent of every backend task except for matching the wire shape Task
5 produces. No other file depends on this compiling except Tasks 8-13.

- [ ] **Step 1: Widen `Preferences`** (currently at line 335-338):

```typescript
export interface Preferences {
  pinnedLines: string[];
  pinnedStations: string[];
  pinnedOperators: string[];
}
```

- [ ] **Step 2: Add `OperatorSummary`**, directly below `LineSummary`:

```typescript
/** `GET /public/operators`'s per-item response shape (+ `GET
 * /public/operators/{code}`'s single-item shape) --
 * `crates/api/src/data/operators.rs`'s `OperatorRollup`, hand-serialized
 * camelCase by `crates/api/src/routes/operators.rs`'s `operator_rollup_json`.
 * `code` is either a real ATOC code (`tocs.atoc_code`) or the literal
 * synthetic string `"TfL"`. `lineIds` is the exact "which lines does this
 * operator run" set the rollup was computed from -- reusable by a future
 * per-operator detail/history view without a second request. `sampleStats`
 * is absent when no matching line had a representative status carrying
 * stats yet (always the case for the `"TfL"` row, which never carries
 * sample stats per-line either) -- render with
 * `lib/operatorStats.ts`'s `formatOperatorSampleSummary`, not
 * `lib/sampleStats.ts`'s `formatSampleSummary` (this type has no
 * `sampleAvailability`/`dataQuality` to satisfy `SampleStatsCarrier` with —
 * see docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md's
 * Judgment Call 3 for why). `computedAt` is `null` only in the
 * (never-constructed-in-practice) case of a rollup with zero matching
 * lines. */
export interface OperatorSummary {
  code: string;
  name: string;
  lineIds: string[];
  worstSeverity: number;
  reason: string;
  sampleStats?: SampleStats;
  computedAt: string | null;
}
```

- [ ] **Step 3: Verify**

```bash
cd frontend && npx tsc --noEmit
```

  Expected: fails, loudly, at every existing `Preferences`-typed literal
  missing `pinnedOperators` (every file listed in this plan's Judgment
  Call 5) — this is the EXPECTED, temporary state until Task 13 fixes
  every one of them. Do not attempt to "fix forward" here; this task's own
  job is only the type widening. Confirm the failures are exactly the
  missing-`pinnedOperators` shape (not some unrelated syntax error) before
  moving on.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/types.ts
git commit -m "frontend: add OperatorSummary, widen Preferences with pinnedOperators"
```

---

## Task 8: `frontend/lib/api.ts` — `getAllOperators()` + widen `getPreferences()`

**Files:** modify `frontend/lib/api.ts`; modify `frontend/lib/api.test.ts`.

Depends on Task 7 (the `OperatorSummary`/widened `Preferences` types).

- [ ] **Step 1: Add `getAllOperators`**, directly below `getAllLines`:

```typescript
/** Every operator with at least one currently-tracked public line
 * (`crates/api/src/routes/operators.rs`'s `list_operators`) -- real ATOC
 * codes from `tocs` plus a synthetic `"TfL"` row, each with a rolled-up
 * worst status + merged sample stats. Unauthenticated and caller-identity-
 * independent (unlike `getAllLines`, which appends the caller's own custom
 * lines) -- no cookie forwarding needed. `cache: 'no-store'`, same as
 * `getAllLines`/`getLineStatusForMode`: this is live status data, not
 * slow-changing reference data. */
export async function getAllOperators(): Promise<OperatorSummary[]> {
  const url = `${baseUrl()}/public/operators`;
  return fetchJson<OperatorSummary[]>(url, { cache: 'no-store' });
}
```

  (Add `OperatorSummary` to this file's existing `import type { ... } from './types'` line.)

- [ ] **Step 2: Widen `getPreferences()`'s 401 fallback** (line 317):

```typescript
  if (response.status === 401) {
    return { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };
  }
```

- [ ] **Step 3: Widen `frontend/lib/api.test.ts`**'s two `Preferences`-shaped
  mock literals (lines 403/405, 415/431):

```bash
cd frontend
perl -pi -e 's/pinnedStations: (\[[^\]]*\])/pinnedStations: $1, pinnedOperators: []/g' lib/api.test.ts
```

- [ ] **Step 4: Verify**

```bash
cd frontend
npm test -- lib/api.test.ts
npx tsc --noEmit
```

  Expected: `lib/api.test.ts` passes; `tsc` no longer reports errors for
  this file's own literals (other files' still-unfixed literals are
  expected to keep failing until Task 13 — same note as Task 7's Step 3).

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "frontend: add getAllOperators, widen getPreferences()'s fail-closed default"
```

---

## Task 9: `frontend/components/PinToggle.tsx` — widen `PinKind`

**Files:** modify `frontend/components/PinToggle.tsx`; modify
`frontend/components/PinToggle.test.tsx`.

Independent of every other frontend task except needing Task 7's widened
`Preferences` type to compile against.

- [ ] **Step 1: Widen `PinKind`** (line 10):

```typescript
type PinKind = 'line' | 'station' | 'operator';
```

- [ ] **Step 2: Widen the internal `key`/`endpoint` switch** (lines 101-102),
  from a binary ternary to a three-way `if`/`else if`/`else` (a ternary chain
  would work but reads worse at three branches — matches this file's own
  preference for readability over cleverness elsewhere):

```typescript
      const prefs: Preferences = await prefsResponse.json();
      let key: keyof Preferences;
      let endpoint: string;
      if (kind === 'line') {
        key = 'pinnedLines';
        endpoint = '/api/preferences/pinned-lines';
      } else if (kind === 'station') {
        key = 'pinnedStations';
        endpoint = '/api/preferences/pinned-stations';
      } else {
        key = 'pinnedOperators';
        endpoint = '/api/preferences/pinned-operators';
      }
      const current = prefs[key];
```

- [ ] **Step 3: Widen `frontend/components/PinToggle.test.tsx`**'s three
  `Preferences`-shaped mock literals:

```bash
cd frontend
perl -pi -e 's/pinnedStations: (\[[^\]]*\])/pinnedStations: $1, pinnedOperators: []/g' components/PinToggle.test.tsx
```

- [ ] **Step 4: Add a test for the new `kind`**, alongside this file's
  existing `kind="line"`/`kind="station"` test pairs (mirror the shape of
  whichever existing `kind="station"` toggle-and-PUT test is there,
  substituting `kind="operator"`, `id="SW"`, and asserting the PUT lands on
  `/api/preferences/pinned-operators` with the operator code in the body).

- [ ] **Step 5: Verify**

```bash
cd frontend
npm test -- components/PinToggle.test.tsx
npx tsc --noEmit
```

  Expected: all `PinToggle` tests (existing + new) pass.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/PinToggle.tsx frontend/components/PinToggle.test.tsx
git commit -m "frontend: widen PinToggle's PinKind to include 'operator'"
```

---

## Task 10: `frontend/lib/operatorStats.ts` — the aggregate-stats formatter

**Files:** create `frontend/lib/operatorStats.ts`; create
`frontend/lib/operatorStats.test.ts`.

Depends on Task 7 (`OperatorSummary`). Resolves Judgment Call 3.

- [ ] **Step 1: Write `frontend/lib/operatorStats.ts`**

```typescript
import type { OperatorSummary } from './types';

/** Renders an operator rollup's aggregate delay/cancellation figures, or a
 * hedge sentence when none are available. Deliberately NOT
 * `lib/sampleStats.ts`'s `formatSampleSummary` -- that function requires a
 * `SampleStatsCarrier` shape (`sampleAvailability`, optional `dataQuality`)
 * that has no single coherent value once several lines' independently-
 * varying availability/quality are merged into one rollup. See
 * docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md's
 * Judgment Call 3.
 *
 * The `"TfL"` special case matches `lib/sampleStats.ts`'s
 * `sampleUnavailableReason`'s own TfL wording exactly, for the same
 * underlying reason: TfL statuses never carry sample stats (their
 * `dataQuality` is `'tfl'`, not sample-derived), so a `"TfL"` rollup's
 * `sampleStats` is always absent, and that absence means the same thing
 * here as it does per-line. */
export function formatOperatorSampleSummary(operator: Pick<OperatorSummary, 'code' | 'sampleStats'>): string {
  if (!operator.sampleStats) {
    return operator.code === 'TfL'
      ? "Not measured by this app — status is TfL's own."
      : 'No delay/cancellation data available for this operator.';
  }
  const { total, cancelled, avgDelayMinutes } = operator.sampleStats;
  const cancelledPct = total > 0 ? Math.round((cancelled / total) * 100) : null;
  const delay = `Avg delay ${avgDelayMinutes.toFixed(1)} min`;
  return cancelledPct === null ? delay : `${delay} · ${cancelledPct}% cancelled`;
}
```

- [ ] **Step 2: Write `frontend/lib/operatorStats.test.ts`**

```typescript
import { describe, expect, it } from 'vitest';
import { formatOperatorSampleSummary } from './operatorStats';

describe('formatOperatorSampleSummary', () => {
  it('renders the TfL-specific hedge when a TfL rollup has no sample stats', () => {
    expect(formatOperatorSampleSummary({ code: 'TfL', sampleStats: undefined })).toBe(
      "Not measured by this app — status is TfL's own.",
    );
  });

  it('renders a generic hedge for any other operator with no sample stats', () => {
    expect(formatOperatorSampleSummary({ code: 'SW', sampleStats: undefined })).toBe(
      'No delay/cancellation data available for this operator.',
    );
  });

  it('renders delay and cancellation percentage when stats are present', () => {
    expect(
      formatOperatorSampleSummary({
        code: 'SW',
        sampleStats: { total: 20, delayed: 4, cancelled: 2, skipped: 0, avgDelayMinutes: 3.4 },
      }),
    ).toBe('Avg delay 3.4 min · 10% cancelled');
  });

  it('omits the cancellation clause when total is zero', () => {
    expect(
      formatOperatorSampleSummary({
        code: 'SW',
        sampleStats: { total: 0, delayed: 0, cancelled: 0, skipped: 0, avgDelayMinutes: 0 },
      }),
    ).toBe('Avg delay 0.0 min');
  });
});
```

- [ ] **Step 3: Verify**

```bash
cd frontend
npm test -- lib/operatorStats.test.ts
```

  Expected: all 4 tests pass.

- [ ] **Step 4: Commit**

```bash
git add frontend/lib/operatorStats.ts frontend/lib/operatorStats.test.ts
git commit -m "frontend: add formatOperatorSampleSummary"
```

---

## Task 11: `frontend/components/OperatorStatusCard.tsx`

**Files:** create `frontend/components/OperatorStatusCard.tsx`; create
`frontend/components/OperatorStatusCard.test.tsx`.

Depends on Tasks 9 (widened `PinToggle`) and 10
(`formatOperatorSampleSummary`). Reused by both Task 12's list page and
Task 13's homepage section, satisfying the spec's explicit "reuse the same
card" instruction for E.

- [ ] **Step 1: Write `frontend/components/OperatorStatusCard.tsx`**

```typescript
'use client';

import { Card, Group, Stack, Text } from '@mantine/core';
import { StatusBadge } from './StatusBadge';
import { LastUpdated } from './LastUpdated';
import { PinToggle } from './PinToggle';
import { formatOperatorSampleSummary } from '@/lib/operatorStats';
import type { OperatorSummary } from '@/lib/types';

/** Mirrors `LineStatusCard`'s shape (worst-status badge, reason, sample
 * summary, last-updated) -- deliberately NOT wrapped in a `Link` the way
 * `LineStatusCard` links to `/lines/{id}`: there is no
 * `/operators/[code]` detail page in this phase for a click to go to (see
 * docs/superpowers/plans/2026-09-22-operator-overview-phase3-operators-list-and-pinning-plan.md's
 * Judgment Call 7). Used by both `/operators`' list page and the
 * homepage's "Your Operators" section -- the same reuse
 * `LineStatusCard` already gets across `/lines`... no, across the
 * homepage's "Your Lines" section (the one other place a status card like
 * this renders). */
export function OperatorStatusCard({
  operator,
  pinned,
  needsAccountHint = false,
}: {
  operator: OperatorSummary;
  pinned: boolean;
  needsAccountHint?: boolean;
}) {
  return (
    <Card withBorder shadow="sm" padding="lg">
      <Stack gap="xs">
        <Group justify="space-between" wrap="nowrap" gap="xs" data-card-title-row>
          <Text fw={600} lineClamp={2} style={{ minWidth: 0 }}>
            {operator.name}
          </Text>
          <StatusBadge severity={operator.worstSeverity} />
        </Group>
        <Text
          size="sm"
          c="dimmed"
          lineClamp={3}
          data-card-reason
          style={{ display: '-webkit-box', WebkitBoxOrient: 'vertical', WebkitLineClamp: 3, overflow: 'hidden' }}
        >
          {operator.reason || 'No current disruption reported.'}
        </Text>
        <Text size="xs" c="dimmed">
          {formatOperatorSampleSummary(operator)}
        </Text>
        <Group justify="space-between" align="center" wrap="nowrap">
          {operator.computedAt ? (
            <LastUpdated timestamp={operator.computedAt} />
          ) : (
            <Text size="xs" c="dimmed">
              &nbsp;
            </Text>
          )}
          <PinToggle
            kind="operator"
            id={operator.code}
            initiallyPinned={pinned}
            needsAccountHint={needsAccountHint}
          />
        </Group>
      </Stack>
    </Card>
  );
}
```

- [ ] **Step 2: Write `frontend/components/OperatorStatusCard.test.tsx`**,
  mirroring `LineStatusCard.test.tsx`'s existing shape (render with
  `renderWithMantine`, assert the name/badge/reason/sample-summary text and
  the `PinToggle`'s pinned/unpinned state render correctly for a fixture
  `OperatorSummary`, including one fixture with `sampleStats: undefined`
  and `code: 'TfL'` to cover the TfL-hedge branch end to end through the
  real component, not just `operatorStats.test.ts`'s unit-level coverage).

- [ ] **Step 3: Verify**

```bash
cd frontend
npm test -- components/OperatorStatusCard.test.tsx
```

- [ ] **Step 4: Commit**

```bash
git add frontend/components/OperatorStatusCard.tsx frontend/components/OperatorStatusCard.test.tsx
git commit -m "frontend: add OperatorStatusCard"
```

---

## Task 12: `frontend/app/operators/page.tsx` — the list page

**Files:** create `frontend/app/operators/page.tsx`; create
`frontend/app/operators/page.test.tsx`.

Depends on Task 8 (`getAllOperators`), Task 11 (`OperatorStatusCard`), and
Task 13's `NO_PREFERENCES` widening being done FIRST is not required — this
task defines its own local `NO_PREFERENCES` constant from scratch (it is a
new file, not one of the three pre-existing ones Task 13 fixes), already in
the widened three-field shape from day one.

- [ ] **Step 1: Write `frontend/app/operators/page.tsx`**, mirroring
  `frontend/app/lines/page.tsx`'s exact structure (Server Component,
  `revalidate = 0`, static `metadata`, `Promise.all` fetch, `NO_PREFERENCES`
  fail-closed fallback, `viewerIsAnonymous` session hint):

```typescript
import { Stack, Text, SimpleGrid, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { getAllOperators, getPreferences, getSession } from '@/lib/api';
import { withStaleFallback } from '@/lib/liveDataCache';
import { OperatorStatusCard } from '@/components/OperatorStatusCard';
import type { Preferences } from '@/lib/types';

export const revalidate = 0;

const METADATA_TITLE = 'Operators — Distant Signal';
const METADATA_DESCRIPTION =
  'Every train operator this app tracks — National Rail TOCs and TfL — with its current worst status and aggregate delay/cancellation figures at a glance.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

// Fails closed to "nothing pinned" on a preferences-fetch failure, the
// exact shape a 401 already returns (design spec Decision 5) -- same
// posture as app/lines/page.tsx's own NO_PREFERENCES.
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };

export default async function OperatorsPage() {
  const [operators, preferences, viewerIsAnonymous] = await Promise.all([
    withStaleFallback('allOperators', () => getAllOperators()),
    getPreferences().catch(() => NO_PREFERENCES),
    getSession()
      .then((session) => !session.authenticated)
      .catch(() => true),
  ]);

  const pinnedSet = new Set(preferences.pinnedOperators);

  return (
    <Stack p="lg" gap="xl">
      <Title order={1}>Operators</Title>
      {operators.length === 0 ? (
        <Text c="dimmed">No operator status data available right now.</Text>
      ) : (
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
          {operators.map((operator) => (
            <OperatorStatusCard
              key={operator.code}
              operator={operator}
              pinned={pinnedSet.has(operator.code)}
              needsAccountHint={viewerIsAnonymous}
            />
          ))}
        </SimpleGrid>
      )}
    </Stack>
  );
}
```

- [ ] **Step 2: Write `frontend/app/operators/page.test.tsx`**, mirroring
  `frontend/app/lines/page.test.tsx`'s existing shape: mock
  `api.getAllOperators`/`api.getPreferences`/`api.getSession`, render the
  page, assert one card per returned operator, assert the empty-state
  sentence when `getAllOperators` resolves `[]`, and assert
  `getPreferences` rejecting/401-ing still renders the page (fails closed,
  no pins shown, not a broken page).

- [ ] **Step 3: Verify**

```bash
cd frontend
npm test -- app/operators/page.test.tsx
npx tsc --noEmit
```

- [ ] **Step 4: Commit**

```bash
git add frontend/app/operators/page.tsx frontend/app/operators/page.test.tsx
git commit -m "frontend: add /operators list page"
```

---

## Task 13: Homepage "Your Operators" section + remaining `NO_PREFERENCES` widenings

**Files:** modify `frontend/app/page.tsx`, `frontend/app/page.test.tsx`,
`frontend/app/lines/page.tsx`, `frontend/app/lines/page.test.tsx`,
`frontend/app/stations/[crs]/page.tsx`, `frontend/app/stations/[crs]/page.test.tsx`.

Depends on Tasks 7, 8, 11. This task closes out Judgment Call 5's full
blast radius: THREE `NO_PREFERENCES` constants (not two), plus every
test-mock literal across five files.

- [ ] **Step 1: Bulk-widen every remaining `Preferences`-shaped literal**
  this task does not need to hand-edit (every occurrence has
  `pinnedStations: [...]` as the last property before its closing brace —
  confirmed by direct inspection of every listed file while writing this
  plan):

```bash
cd frontend
for f in app/page.test.tsx "app/stations/[crs]/page.test.tsx" app/lines/page.test.tsx; do
  perl -pi -e 's/pinnedStations: (\[[^\]]*\])/pinnedStations: $1, pinnedOperators: []/g' "$f"
done
```

- [ ] **Step 2: Widen the three `NO_PREFERENCES` constants by hand**
  (the bulk script above deliberately does NOT touch the three non-test
  `page.tsx` files, so their `NO_PREFERENCES` consts and the homepage's new
  fetch/section logic can be edited together, in context, below):

  `frontend/app/lines/page.tsx:74`:

```typescript
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };
```

  `frontend/app/stations/[crs]/page.tsx:51`:

```typescript
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };
```

- [ ] **Step 3: `frontend/app/page.tsx` — fetch operators + compute the
  pinned subset.** Add `getAllOperators` to the existing imports, widen
  `NO_PREFERENCES` (line 94), and add one more concurrent fetch inside the
  existing `Promise.all` (after the `getSharedGroupCustomLines` entry):

```typescript
import {
  ApiNotFoundError,
  getAllOperators,
  getLineStatusForMode,
  getMyTrackedTrains,
  getPreferences,
  getSession,
  getSharedGroupCustomLines,
  getSharedGroupTrains,
  getStationName,
  getStopPointDisruption,
} from '@/lib/api';
import { OperatorStatusCard } from '@/components/OperatorStatusCard';
```

```typescript
const NO_PREFERENCES: Preferences = { pinnedLines: [], pinnedStations: [], pinnedOperators: [] };
```

```typescript
  const [preferences, allReports, myTrackedTrains, sharedGroupTrains, sharedCustomLines, allOperators] =
    await Promise.all([
      getPreferences().catch(() => NO_PREFERENCES),
      withStaleFallback(`lineStatusForMode:${DISPLAYED_MODES_PARAM}`, () =>
        getLineStatusForMode(DISPLAYED_MODES_PARAM),
      ),
      getMyTrackedTrains().catch(() => null),
      getSharedGroupTrains().catch(() => null),
      getSharedGroupCustomLines().catch(() => null),
      // Same public, unauthenticated, cheap-to-fetch-in-full list
      // `/operators` itself fetches (~25-40 rows) -- filtered down to the
      // caller's own pins below, rather than a per-code batch fetch. See
      // this plan's Judgment Call 6.
      withStaleFallback('allOperators', () => getAllOperators()).catch(() => []),
    ]);
```

- [ ] **Step 4: Compute `pinnedOperatorSummaries`**, directly below the
  existing `pinnedLineReports` computation, with the identical worst-first-
  then-alphabetical sort:

```typescript
  const pinnedOperatorSummaries = allOperators
    .filter((operator) => preferences.pinnedOperators.includes(operator.code))
    .sort((a, b) => {
      const rankDiff = severityRank(b.worstSeverity) - severityRank(a.worstSeverity);
      return rankDiff !== 0 ? rankDiff : a.name.localeCompare(b.name);
    });
```

- [ ] **Step 5: Generalize `bothPinnedSectionsEmpty` to `allPinnedSectionsEmpty`**
  (the correct three-way generalization the spec's own E section names as
  the right move "once a third section exists" — do this as a rename, not
  an addition, so there is exactly one such flag, not two disagreeing
  ones). Replace the existing declaration:

```typescript
  const allPinnedSectionsEmpty =
    pinnedLineReports.length === 0 && pinnedStationEntries.length === 0 && pinnedOperatorSummaries.length === 0;
```

  and update BOTH existing usages (the top-of-page and the
  Lines-empty-only placements) from `bothPinnedSectionsEmpty` to
  `allPinnedSectionsEmpty`:

```typescript
      {allPinnedSectionsEmpty && <RightNowModule summary={rightNow} />}
```

```typescript
      {!allPinnedSectionsEmpty && pinnedLineReports.length === 0 && (
        <RightNowModule summary={rightNow} />
      )}
```

- [ ] **Step 6: Add the "Your Operators" section**, directly after the
  existing "Your Stations" `</Stack>` and before the second
  `RightNowModule` placement block:

```typescript
      <Stack gap="md">
        <Group justify="space-between">
          <Title order={2}>Your Operators</Title>
          {pinnedOperatorSummaries.length > 0 && <TextLink href="/operators">Browse all operators</TextLink>}
        </Group>
        {pinnedOperatorSummaries.length === 0 ? (
          <Text c="dimmed">
            You haven&apos;t pinned any operators yet. <Link href="/operators">Browse all operators</Link> to pin
            some.
          </Text>
        ) : (
          <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
            {pinnedOperatorSummaries.map((operator) => (
              <OperatorStatusCard key={operator.code} operator={operator} pinned />
            ))}
          </SimpleGrid>
        )}
      </Stack>
```

  Note `pinned` is passed as a bare `true` (every card in THIS section is,
  by construction, one of the caller's own pins) — no `needsAccountHint`
  passed either, matching this page's other two pinned sections' cards
  (`LineStatusCard` takes no pin-related props at all on this page; the
  "Your Stations" cards render their own inline `PinToggle`-free `Card`).
  Unlike `/operators`' list page (Task 12), a logged-in viewer on THIS
  page's pinned section is never anonymous by construction (the whole
  branch is inside `if (!session.authenticated) { return ...anonymous... }`
  further up the file), so `needsAccountHint` would always be `false` here
  regardless — omitting it entirely relies on `OperatorStatusCard`'s own
  default (`= false`), which is correct.

- [ ] **Step 7: Widen `frontend/app/page.test.tsx`'s test mocks** for the
  new fetch, alongside its existing `beforeEach`/per-test
  `vi.mocked(api.getSharedGroupCustomLines).mockResolvedValue(...)` calls —
  add a default `vi.mocked(api.getAllOperators).mockResolvedValue([])`
  wherever those adjacent mocks are already set up, so every existing test
  that doesn't care about operators keeps passing with an empty operators
  list (mirroring how `getSharedGroupCustomLines`'s default is already
  `[]` for every test that isn't specifically about it). Add at least one
  NEW test asserting: (a) a pinned operator with a matching
  `getAllOperators` entry renders an `OperatorStatusCard` under "Your
  Operators"; (b) the empty-state sentence renders and links to
  `/operators` when `preferences.pinnedOperators` is empty; (c) with all
  three pinned sections empty, `allPinnedSectionsEmpty` still correctly
  puts `RightNowModule` at the top of the page (a direct regression check
  for Step 5's rename — the existing "both empty" test, if any, should be
  renamed/duplicated to cover the operators-empty leg of the new
  three-way `&&`).

- [ ] **Step 8: Verify**

```bash
cd frontend
npm test -- app/page.test.tsx app/lines/page.test.tsx "app/stations/[crs]/page.test.tsx"
npx tsc --noEmit
npm run build
```

  Expected: all tests pass; `tsc` reports zero errors anywhere in the
  project now (this is the step where every literal Task 7's Step 3
  flagged as temporarily broken must now be fixed); production build
  succeeds, including the new `/operators` route.

- [ ] **Step 9: Commit**

```bash
git add frontend/app/page.tsx frontend/app/page.test.tsx \
        frontend/app/lines/page.tsx frontend/app/lines/page.test.tsx \
        "frontend/app/stations/[crs]/page.tsx" "frontend/app/stations/[crs]/page.test.tsx"
git commit -m "frontend: add homepage 'Your Operators' section, generalize bothPinnedSectionsEmpty to allPinnedSectionsEmpty"
```

---

## Task 14: Full-stack verify

**Files:** none (verification only).

- [ ] **Step 1: Rust, plain**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo test --workspace
```

- [ ] **Step 2: Rust, DB-gated**

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres \
  cargo test -p api -- --ignored --test-threads=1
```

  Expected: every DB-gated test this plan added (Tasks 3-6), plus every
  pre-existing one, passes.

- [ ] **Step 3: Frontend**

```bash
cd frontend
npm test
npx tsc --noEmit
npm run build
```

- [ ] **Step 4: Manual, in a real browser** (this repo's standing practice
  for a change with meaningful new UI and no end-to-end test coverage).
  Start the dev stack, then:
  1. Visit `/operators` while logged out. Confirm a grid of operator
     cards renders, each with a name, a status badge, a reason/hedge line,
     a delay/cancellation summary (or the TfL-specific hedge on the "TfL"
     card), and a pin star showing the "needs an account" hint on hover.
  2. Log in. Pin two or three operators (including "TfL") from
     `/operators`. Confirm the star fills in immediately.
  3. Go to the homepage. Confirm a new "Your Operators" section appears,
     below "Your Stations", showing exactly the pinned operators, sorted
     worst-status-first.
  4. Unpin every pinned operator (from either `/operators` or by editing
     the pinned set some other way) and reload the homepage. Confirm
     "Your Operators" reverts to its empty-state sentence linking back to
     `/operators`.
  5. With every pin (lines, stations, AND operators) removed, confirm the
     "Right now" module renders at the very TOP of the homepage (the
     `allPinnedSectionsEmpty` case) — this is the concrete regression
     check for Task 13's `bothPinnedSectionsEmpty` → `allPinnedSectionsEmpty`
     rename.
  6. Hit `GET /public/operators/SW` (or any real ATOC code with a
     currently-tracked line) directly and confirm it 200s with the same
     shape as that code's entry in the list response; hit a nonsense code
     and confirm a 404.

---

## Self-review (spec coverage)

- §C's rollup computation (Open Questions 1/2) → Task 3
  (`data::operators`), Judgment Calls 2/4.
- §C's "which crate" / "no poller or aggregator change required" →
  confirmed: every new file in this plan is under `crates/api`/`crates/common`/
  `frontend`; nothing in `crates/aggregator`/`crates/poller-*` is touched.
- §C's frontend list page + card reuse recommendation → Tasks 11, 12.
- §E's backend mirror-of-`pinned_lines` → Tasks 2, 4, 6.
- §E's `PinToggle` widening → Task 9.
- §E's `Preferences`/`NO_PREFERENCES` widening (+ the third call site the
  spec didn't name) → Tasks 7, 13; Judgment Call 5.
- §E's homepage section, "reuse the same card," empty-state copy pattern →
  Task 13.
- Open Question 3 (access control) → confirmed: `/operators` and
  `/operators/{code}` are unauthenticated (Task 5), matching every other
  public catalogue surface; only the pinning routes are session-gated
  (Task 6), matching `pinned_lines`/`pinned_stations` exactly.
- "Ship C and E together, since E has no marginal cost once C exists" →
  reflected directly in this plan's task ordering (Tasks 3/5 build the
  rollup+routes; Tasks 4/6 add pinning on top of the same migration/module
  structure with genuinely small, mechanical diffs).

## Note for Phase 4

Phase 4 (§D's operator/network historical views) needs, per the spec's own
§D.2, "a future Phase 4 could reuse to get 'all lines for operator X'" —
that is exactly `crates/api/src/data/operators.rs`'s
`OperatorRollup.line_ids` (populated by `all_operator_rollups`/
`operator_rollup`, Task 3). A Phase 4 cross-line aggregation query over
`line_status_daily_stats`/`line_status_half_hourly_stats` scoped to one
operator can call `operators::operator_rollup(pool, &app.config.lines, code)`,
read `.line_ids` off the result, and `WHERE line_id = ANY($1)` against
those ids — no new resolution logic needed, and no double-counting risk
either (the TfL-merge exclusion in `public_line_status_rows` already
prevents a merged TfL line from appearing in a rollup's `line_ids`
alongside its NR catalogue counterpart). This plan does not build any of
Phase 4's actual aggregation queries, charts, or routes — this paragraph
only records the exact reusable surface Phase 3 leaves behind.
