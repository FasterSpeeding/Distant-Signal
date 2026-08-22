# Elizabeth Line Duplicate-Entry Merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stop "Elizabeth line" appearing as two unrelated rows (one NR-sourced with real `sample_stats`, one TfL-sourced without) on `/public/lines` and the line detail page, by suppressing the TfL row from the public line list and overlaying its status onto the NR row's detail view as a labeled secondary field.

**Architecture:** A small, explicit, literal id-mapping table in `common` (`"tfl-elizabeth" → "elizabeth-line"`) drives two read-time-only changes: `crates/api/src/routes/lines.rs::list_lines` filters the mapped TfL row out of `/public/lines`, and `crates/api/src/routes/line_status.rs::get_line_status` fetches the mapped TfL row's current statuses alongside any requested NR row and attaches them under a new `tflStatus` JSON field via a new `render.rs` function. No schema change, no write-path change, no touch to `line_status_history` or either poller. The frontend gains one optional field on `LineStatusReport` and one new labeled section on the line detail page.

**Tech Stack:** Rust 2024 edition (axum 0.8, sqlx 0.8), PostgreSQL 16, Next.js 16 App Router + Mantine v9 + Vitest.

**Spec:** `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md` — Area 1 only. Areas 2–4 of that spec are out of scope for this plan.

## Global Constraints

- **Read-time only. Never write a merged/synthesized row into `line_status_history`, and never modify `upsert_tfl_line_status` or the aggregator's write path.** This is the spec's Area 1 hard constraint, verbatim. No task in this plan touches `crates/api/src/data/queries.rs::upsert_tfl_line_status`, `crates/aggregator/`, or the `line_status_history` table in any way — the merge is entirely in the two read-side handlers named above.
- **Never reuse the aggregator's volatile reason-annotation pattern** (`crates/aggregator/src/aggregation.rs:879`'s `Some(format!("live samples show: {reason}"))`, and the similar pattern at line 759) for anything this plan produces. That pattern — sample-derived text appended into `reason`, changing every aggregation cycle even when nothing about the underlying disruption changed — is the confirmed root cause of a separate, currently-open line-history-duplication bug. This plan's overlay never constructs new `reason` text at all: it passes TfL's own `LineStatus.reason` values through byte-for-byte under the new `tflStatus` field. Task 3 includes a regression test asserting exactly that (see below).
- **The mapping table has exactly one entry today.** `TFL_TO_NR_LINE_ID: &[(&str, &str)] = &[("tfl-elizabeth", "elizabeth-line")]` in `crates/common/src/lib.rs`, next to the existing `TFL_LINE_ID_PREFIX`/`TFL_OPERATOR` constants. Area 2 of the spec (Overground, not in scope here) will extend it to seven entries once six new NR line definitions exist — the two lookup functions this plan adds (`nr_line_id_for_tfl`, `tfl_line_id_for_nr`) are written generically over the whole table so that future extension needs no code change beyond adding rows.
- **`common::LineStatusReport`/`LineStatus` (the shared wire/storage type) are not modified.** The overlay is assembled entirely in `crates/api`'s render/route layer, which already builds the public JSON shape independently of the stored type (see `render.rs`'s own module doc comment: "Deliberately independent of any `#[serde(rename)]` on the stored types"). Adding a field to the shared struct would also touch the poller's payload shape and the DB-storage serialization path (`upsert_tfl_line_status` serializes `report.statuses`, not the whole report, so this wouldn't strictly break storage — but it's needless surface area for a field only the API layer ever populates, and every other overlay decision in this plan follows the same "stay in the API crate" boundary).
- **Testing conventions, matching this repo's existing pattern** (see `docs/superpowers/plans/2026-08-22-tfl-line-status-integration.md`'s Global Constraints for the full survey): Rust pure functions get a `#[cfg(test)] mod tests` at the bottom of the same file; anything needing Postgres is `#[ignore]`d and this repo has exactly one such test in the whole codebase, so this plan avoids adding a DB-backed test by decomposing every piece of new logic into a pure, directly-testable function and wiring it into the async handler last. Frontend: `components/`/`lib/` files get a `.test.ts(x)` sibling; **no file under `app/` has a component test** — `frontend/app/lines/[id]/page.tsx` (Task 5) is a Server Component and is verified by hand against the running dev stack, matching how every other page in this app is checked.
- **Every command in this plan runs from the repo root** unless it says `cd frontend`. Rust tests are `cargo test -p <crate>`.
- **Commit after each task.**

---

## Task 1: TfL-to-NR line-id mapping in `common`

**Files:**
- Modify: `crates/common/src/lib.rs`

**Interfaces:**
- Produces: `common::nr_line_id_for_tfl(tfl_line_id: &str) -> Option<&'static str>`, `common::tfl_line_id_for_nr(nr_line_id: &str) -> Option<&'static str>`. Consumed by Task 2 (`lines.rs`) and Task 4 (`line_status.rs`).

- [ ] **Step 1: Write the failing tests**

Append to `crates/common/src/lib.rs`, near the existing `tfl_severity_tests` module:

```rust
#[cfg(test)]
mod tfl_nr_merge_tests {
    use super::*;

    #[test]
    fn elizabeth_line_tfl_id_maps_to_its_nr_counterpart() {
        assert_eq!(nr_line_id_for_tfl("tfl-elizabeth"), Some("elizabeth-line"));
    }

    #[test]
    fn elizabeth_line_nr_id_maps_back_to_its_tfl_counterpart() {
        assert_eq!(tfl_line_id_for_nr("elizabeth-line"), Some("tfl-elizabeth"));
    }

    #[test]
    fn a_tfl_line_with_no_nr_counterpart_has_no_mapping() {
        // The overwhelming majority of TfL lines -- e.g. the Northern line,
        // which collides in *name* with an NR catalogue line but has no
        // shared-infrastructure NR counterpart the way Elizabeth line does.
        assert_eq!(nr_line_id_for_tfl("tfl-northern"), None);
    }

    #[test]
    fn an_nr_line_with_no_tfl_counterpart_has_no_mapping() {
        assert_eq!(tfl_line_id_for_nr("waterloo-main-line"), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p common tfl_nr_merge_tests`
Expected: FAIL with "cannot find function `nr_line_id_for_tfl`" (and likewise for `tfl_line_id_for_nr`).

- [ ] **Step 3: Implement the mapping table and lookup functions**

Add to `crates/common/src/lib.rs`, directly below the existing `TFL_LINE_ID_PREFIX` constant (around line 233):

```rust
/// Maps a TfL line id (already `TFL_LINE_ID_PREFIX`-namespaced) to the NR
/// catalogue line id covering the same railway, for the small set of lines
/// where a TfL-sourced `line_status` row and an NR/Darwin-sourced one exist
/// independently for what is, to a passenger, one railway. See
/// `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
/// Area 1. Elizabeth line is the only entry today; Overground will add six
/// more once NR line definitions exist for it (that spec's Area 2, not yet
/// done) -- `nr_line_id_for_tfl`/`tfl_line_id_for_nr` are written generically
/// over this table so extending it needs no code change beyond a new row.
const TFL_TO_NR_LINE_ID: &[(&str, &str)] = &[("tfl-elizabeth", "elizabeth-line")];

/// The NR catalogue line id a TfL line's status should be merged into for
/// display, or `None` if this TfL line has no NR counterpart (true for
/// every TfL line except the ones in `TFL_TO_NR_LINE_ID`).
pub fn nr_line_id_for_tfl(tfl_line_id: &str) -> Option<&'static str> {
    TFL_TO_NR_LINE_ID
        .iter()
        .find(|(tfl, _)| *tfl == tfl_line_id)
        .map(|(_, nr)| *nr)
}

/// The TfL line id whose status should be overlaid onto this NR catalogue
/// line id's detail view, or `None` if this NR line has no TfL counterpart.
pub fn tfl_line_id_for_nr(nr_line_id: &str) -> Option<&'static str> {
    TFL_TO_NR_LINE_ID
        .iter()
        .find(|(_, nr)| *nr == nr_line_id)
        .map(|(tfl, _)| *tfl)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p common tfl_nr_merge_tests`
Expected: PASS, 4 tests.

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "feat: add TfL-to-NR line-id mapping for Elizabeth line merge"
```

---

## Task 2: Suppress the merged TfL row from `/public/lines`

**Files:**
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Consumes: `common::nr_line_id_for_tfl(tfl_line_id: &str) -> Option<&'static str>` (Task 1).
- Produces: `is_merged_into_nr_line(tfl_line_id: &str) -> bool`, a private pure helper — not consumed outside this file, but named so a future reviewer can find the suppression logic by name.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of `crates/api/src/routes/lines.rs` (after `catalogue_and_custom_line_summaries_are_not_suffixed`):

```rust
#[test]
fn a_tfl_line_with_an_nr_counterpart_is_suppressed() {
    assert!(is_merged_into_nr_line("tfl-elizabeth"));
}

#[test]
fn a_tfl_line_with_no_nr_counterpart_is_not_suppressed() {
    assert!(!is_merged_into_nr_line("tfl-northern"));
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api routes::lines::tests`
Expected: FAIL with "cannot find function `is_merged_into_nr_line`".

- [ ] **Step 3: Implement the filter**

Add this function above `tfl_display_name` (around line 144 today) in `crates/api/src/routes/lines.rs`:

```rust
/// Whether a TfL line's summary should be omitted from `/public/lines`
/// because an NR/Darwin-sourced line already covers the same railway and is
/// shown in its place, carrying this TfL line's status as a secondary field
/// on its detail view instead (`crates/api/src/routes/line_status.rs::get_line_status`).
/// See `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
/// Area 1.
fn is_merged_into_nr_line(tfl_line_id: &str) -> bool {
    common::nr_line_id_for_tfl(tfl_line_id).is_some()
}
```

Then change `list_lines` (around lines 123–132) from:

```rust
    let tfl = queries::tfl_line_summaries(&app.database)
        .await
        .map_err(internal_error)?;
    out.extend(tfl.into_iter().map(|line| LineSummary {
        id: line.id,
        name: tfl_display_name(&line.name),
        category: line.mode_name,
        operators: vec![common::TFL_OPERATOR.to_string()],
        source: "tfl",
    }));
```

to:

```rust
    let tfl = queries::tfl_line_summaries(&app.database)
        .await
        .map_err(internal_error)?;
    out.extend(
        tfl.into_iter()
            .filter(|line| !is_merged_into_nr_line(&line.id))
            .map(|line| LineSummary {
                id: line.id,
                name: tfl_display_name(&line.name),
                category: line.mode_name,
                operators: vec![common::TFL_OPERATOR.to_string()],
                source: "tfl",
            }),
    );
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api routes::lines::tests`
Expected: PASS, all tests in the module including the two new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/lines.rs
git commit -m "fix: suppress TfL Elizabeth line row from /public/lines in favor of its NR counterpart"
```

---

## Task 3: Render-layer overlay JSON shape

**Files:**
- Modify: `crates/api/src/render.rs`

**Interfaces:**
- Consumes: `common::LineStatus` (existing), `common::LineStatusReport` (existing).
- Produces: `to_tfl_shape_with_overlay(report: &LineStatusReport, computed_at: DateTime<Utc>, detail: bool, tfl_overlay: Option<&[LineStatus]>) -> Value`. Consumed by Task 4 (`line_status.rs`). The JSON it adds: a top-level `"tflStatus"` array (same per-status shape `status_to_json` already produces for `lineStatuses`), present only when `tfl_overlay` is `Some`.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `crates/api/src/render.rs` (after `sample_stats_omitted_when_absent`):

```rust
fn overlay_status(reason: &str) -> LineStatus {
    LineStatus {
        severity: Severity::MinorDelays,
        reason: reason.to_string(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: None,
        data_quality: DataQuality::Tfl,
        sample_stats: None,
    }
}

#[test]
fn tfl_status_included_when_overlay_present() {
    let report = sample_report(None);
    let overlay = vec![overlay_status("Minor delays due to signalling")];
    let json = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
    assert_eq!(json["tflStatus"][0]["reason"], "Minor delays due to signalling");
    assert_eq!(json["tflStatus"][0]["dataQuality"], "tfl");
}

#[test]
fn tfl_status_omitted_when_overlay_absent() {
    let report = sample_report(None);
    let json = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, None);
    assert!(json.get("tflStatus").is_none());
}

#[test]
fn overlay_does_not_alter_the_primary_lineStatuses_field() {
    // The NR row's own statuses must render identically with or without an
    // overlay present -- the overlay is additive, never a merge into the
    // primary field.
    let report = sample_report(None);
    let without = to_tfl_shape(&report, sample_computed_at(), false);
    let overlay = vec![overlay_status("Some TfL text")];
    let with = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
    assert_eq!(without["lineStatuses"], with["lineStatuses"]);
}

#[test]
fn overlay_reason_text_is_stable_across_identical_calls() {
    // Regression guard for the hard constraint in
    // docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
    // Area 1: this function must never synthesize or annotate `reason`
    // text -- it passes the source LineStatus through verbatim. Two calls
    // with byte-identical input must produce byte-identical output,
    // unlike the aggregator's volatile sample-stats annotation pattern
    // that caused a separate line-history duplication bug.
    let report = sample_report(None);
    let overlay = vec![overlay_status("Severe delays between Paddington and Heathrow")];
    let first = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
    let second = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
    assert_eq!(first["tflStatus"], second["tflStatus"]);
    assert_eq!(first["tflStatus"][0]["reason"], "Severe delays between Paddington and Heathrow");
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api render::tests`
Expected: FAIL with "cannot find function `to_tfl_shape_with_overlay`".

- [ ] **Step 3: Implement the overlay function**

Add to `crates/api/src/render.rs`, directly after `to_tfl_shape` (around line 24):

```rust
/// Like `to_tfl_shape`, but attaches a second line's current statuses under
/// a `tflStatus` field when `tfl_overlay` is `Some`. Used only by the
/// single-line detail endpoint (`routes/line_status.rs::get_line_status`)
/// for lines with a TfL counterpart merged away from `/public/lines` --
/// see `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
/// Area 1. `tfl_overlay`'s statuses are rendered through the same
/// `status_to_json` as the primary line, unchanged, so this never
/// constructs new `reason` text -- see that spec's hard constraint.
pub fn to_tfl_shape_with_overlay(
    report: &LineStatusReport,
    computed_at: DateTime<Utc>,
    detail: bool,
    tfl_overlay: Option<&[LineStatus]>,
) -> Value {
    let mut out = to_tfl_shape(report, computed_at, detail);
    if let Some(statuses) = tfl_overlay {
        out["tflStatus"] = Value::Array(statuses.iter().map(|s| status_to_json(s, detail)).collect());
    }
    out
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api render::tests`
Expected: PASS, all tests including the 4 new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/render.rs
git commit -m "feat: add TfL status overlay to the line-status JSON render layer"
```

---

## Task 4: Wire the overlay into the single-line-status endpoint

**Files:**
- Modify: `crates/api/src/routes/line_status.rs`

**Interfaces:**
- Consumes: `common::nr_line_id_for_tfl`/`tfl_line_id_for_nr` (Task 1), `render::to_tfl_shape_with_overlay` (Task 3), `queries::LineStatusRow` (existing, all fields `pub`), `queries::line_status_for_ids` (existing).
- Produces: `tfl_ids_to_overlay(rows: &[queries::LineStatusRow]) -> Vec<String>`, `overlay_for(row: &queries::LineStatusRow, tfl_rows: &[queries::LineStatusRow]) -> Option<Vec<common::LineStatus>>` — both pure, private, tested directly; not consumed outside this file.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block at the bottom of `crates/api/src/routes/line_status.rs` (after `an_empty_mode_list_is_rejected_rather_than_matching_everything`):

```rust
use chrono::Utc;
use common::{DataQuality, Severity, ValidityPeriod};

fn row(id: &str, statuses: Vec<LineStatus>) -> queries::LineStatusRow {
    queries::LineStatusRow {
        id: id.to_string(),
        name: id.to_string(),
        mode_name: "test".to_string(),
        operators: vec![],
        statuses,
        computed_at: Utc::now(),
    }
}

fn a_status(reason: &str) -> LineStatus {
    LineStatus {
        severity: Severity::MinorDelays,
        reason: reason.to_string(),
        validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
        disruption: None,
        data_quality: DataQuality::Tfl,
        sample_stats: None,
    }
}

#[test]
fn tfl_ids_to_overlay_includes_only_rows_with_a_tfl_counterpart() {
    let rows = vec![row("elizabeth-line", vec![]), row("northern", vec![])];
    assert_eq!(tfl_ids_to_overlay(&rows), vec!["tfl-elizabeth".to_string()]);
}

#[test]
fn overlay_for_finds_the_matching_tfl_row() {
    let nr_row = row("elizabeth-line", vec![]);
    let tfl_rows = vec![row("tfl-elizabeth", vec![a_status("Minor delays")])];
    let overlay = overlay_for(&nr_row, &tfl_rows).unwrap();
    assert_eq!(overlay.len(), 1);
    assert_eq!(overlay[0].reason, "Minor delays");
}

#[test]
fn overlay_for_is_none_when_the_line_has_no_tfl_counterpart() {
    let nr_row = row("northern", vec![]);
    let tfl_rows = vec![row("tfl-elizabeth", vec![a_status("Minor delays")])];
    assert!(overlay_for(&nr_row, &tfl_rows).is_none());
}

#[test]
fn overlay_for_is_none_when_the_tfl_counterpart_row_is_missing() {
    // e.g. the TfL feed temporarily dropped the line and
    // upsert_tfl_line_status's prune already removed its row -- graceful
    // degradation, not an error.
    let nr_row = row("elizabeth-line", vec![]);
    assert!(overlay_for(&nr_row, &[]).is_none());
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api routes::line_status::tests`
Expected: FAIL with "cannot find function `tfl_ids_to_overlay`" (and `overlay_for`).

- [ ] **Step 3: Implement the pure helpers and wire them into the handler**

Add to `crates/api/src/routes/line_status.rs`, near `to_report`/`rows_to_json` (around line 48):

```rust
/// TfL line ids whose statuses should be fetched to overlay onto `rows` --
/// one per row that has a TfL counterpart per `common::tfl_line_id_for_nr`.
/// Pure so it's testable without a database.
fn tfl_ids_to_overlay(rows: &[queries::LineStatusRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| common::tfl_line_id_for_nr(&row.id))
        .map(String::from)
        .collect()
}

/// The TfL counterpart's statuses for one NR row, if it has one and that
/// row was actually found in `tfl_rows` (it may not be, if the TfL feed
/// dropped the line since the last poll -- see
/// `queries::upsert_tfl_line_status`'s prune). Pure so it's testable
/// without a database.
fn overlay_for(row: &queries::LineStatusRow, tfl_rows: &[queries::LineStatusRow]) -> Option<Vec<LineStatus>> {
    let tfl_id = common::tfl_line_id_for_nr(&row.id)?;
    tfl_rows.iter().find(|r| r.id == tfl_id).map(|r| r.statuses.clone())
}
```

Then change `get_line_status` (currently lines 120–136) from:

```rust
async fn get_line_status(
    State(app): State<App>,
    Path(ids): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let ids: Vec<String> = ids.split(',').map(|s| s.to_string()).collect();

    let rows = queries::line_status_for_ids(&app.database, &ids)
        .await
        .map_err(internal_error)?;

    if rows.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no matching line(s): {}", ids.join(","))));
    }

    Ok(Json(rows_to_json(rows, query.detail)))
}
```

to:

```rust
async fn get_line_status(
    State(app): State<App>,
    Path(ids): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let ids: Vec<String> = ids.split(',').map(|s| s.to_string()).collect();

    let rows = queries::line_status_for_ids(&app.database, &ids)
        .await
        .map_err(internal_error)?;

    if rows.is_empty() {
        return Err((StatusCode::NOT_FOUND, format!("no matching line(s): {}", ids.join(","))));
    }

    // Any requested row with an NR/TfL counterpart (currently just Elizabeth
    // line) gets that counterpart's status overlaid -- see
    // docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
    // Area 1. A second, separate query rather than a join: this only ever
    // runs for a handful of ids on a single-line detail fetch, and keeps
    // `line_status_for_ids` itself unchanged for every other caller.
    let overlay_ids = tfl_ids_to_overlay(&rows);
    let tfl_rows = if overlay_ids.is_empty() {
        vec![]
    } else {
        queries::line_status_for_ids(&app.database, &overlay_ids)
            .await
            .map_err(internal_error)?
    };

    Ok(Json(
        rows.into_iter()
            .map(|row| {
                let computed_at = row.computed_at;
                let overlay = overlay_for(&row, &tfl_rows);
                let report = to_report(row);
                crate::render::to_tfl_shape_with_overlay(&report, computed_at, query.detail, overlay.as_deref())
            })
            .collect(),
    ))
}
```

Note: `rows_to_json` (used by `get_mode_status`) is untouched — the overlay only applies to `get_line_status`, matching the spec's scope (the merge is a line-detail-page concern; the list/dashboard endpoints already stop showing the TfL row as a duplicate via Task 2's `/public/lines` fix, and `AllLinesTable.tsx` looks status up by iterating `lines` from `/public/lines`, so a TfL row still present in `line_status_for_modes`'s result set is simply never looked up once its `LineSummary` is gone).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api routes::line_status::tests`
Expected: PASS, all tests including the 4 new ones.

- [ ] **Step 5: Full crate build check**

Run: `cargo build -p api`
Expected: compiles clean (this task's handler change is the only non-test code touched).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/line_status.rs
git commit -m "feat: overlay TfL Elizabeth line status onto the NR line's detail response"
```

---

## Task 5: Frontend — surface `tflStatus` on the line detail page

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/app/lines/[id]/page.tsx`

**Interfaces:**
- Consumes: `LineStatus` (existing type in `types.ts`).
- Produces: `LineStatusReport.tflStatus?: LineStatus[]` — consumed by `page.tsx`'s render.

- [ ] **Step 1: Add the field to the TypeScript type**

In `frontend/lib/types.ts`, change:

```ts
export interface LineStatusReport {
  $type: string;
  id: string;
  name: string;
  modeName: string;
  operators: string[];
  lineStatuses: LineStatus[];
  computedAt: string;
}
```

to:

```ts
export interface LineStatusReport {
  $type: string;
  id: string;
  name: string;
  modeName: string;
  operators: string[];
  lineStatuses: LineStatus[];
  computedAt: string;
  /** Present only for a line with a TfL counterpart merged into it (see
   * docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
   * Area 1 -- Elizabeth line today). The counterpart's own current
   * statuses, rendered separately from `lineStatuses` on the line detail
   * page rather than merged into it, since only this report's own
   * `lineStatuses` carries real `sampleStats`. */
  tflStatus?: LineStatus[];
}
```

No change needed to `frontend/lib/api.ts`'s `getLineStatus` — it already returns `res.json()` typed as `Promise<LineStatusReport[]>` with no field-by-field mapping, so the new optional field passes through automatically.

- [ ] **Step 2: Render the secondary section on the line detail page**

In `frontend/app/lines/[id]/page.tsx`, change the end of the returned JSX from:

```tsx
      <RepresentativeInfo statuses={report.lineStatuses} />
      {/* Every issue here belongs to the line already named in the heading,
          so no per-issue line attribution is needed -- that's what the
          optional `lines` on IssueItem is for on the station page. */}
      <IssueList items={report.lineStatuses.map((status) => ({ status }))} now={now} />
    </Stack>
  );
}
```

to:

```tsx
      <RepresentativeInfo statuses={report.lineStatuses} />
      {/* Every issue here belongs to the line already named in the heading,
          so no per-issue line attribution is needed -- that's what the
          optional `lines` on IssueItem is for on the station page. */}
      <IssueList items={report.lineStatuses.map((status) => ({ status }))} now={now} />
      {report.tflStatus && report.tflStatus.length > 0 && (
        <Stack gap="xs">
          {/* This line has an NR counterpart merged into it (Elizabeth line
              today -- see docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
              Area 1) and this is TfL's own, separately-sourced view of the
              same railway. Kept visually distinct from the primary IssueList
              above rather than merged into one list, since only the primary
              side has real sampleStats and merging would blur that. */}
          <Text fw={500}>TfL also reports:</Text>
          <IssueList items={report.tflStatus.map((status) => ({ status }))} now={now} />
        </Stack>
      )}
    </Stack>
  );
}
```

`Stack` and `Text` are both already imported at the top of this file (line 2) — no import changes needed.

- [ ] **Step 3: Type-check and run the existing frontend test suite**

Run: `cd frontend && npx tsc --noEmit`
Expected: no errors.

Run: `cd frontend && npm test`
Expected: all existing tests pass unchanged (this task adds no new `.test.ts(x)` file — `page.tsx` is a Server Component under `app/`, and per this repo's established convention those are verified by hand against the running stack, not unit tested; see this plan's Global Constraints).

- [ ] **Step 4: Manual verification against the dev stack**

Bring up the stack (`docker compose --env-file dev.env up -d`), wait for both the NR aggregator and `poller-tfl` to have completed at least one cycle, then visit `/lines/elizabeth-line` and confirm:
- The page shows one "Elizabeth line" entry (not two) when navigating from `/lines`.
- The primary `IssueList` shows the NR-sourced status/sample stats as before.
- A "TfL also reports:" section appears below it with TfL's own current status for Elizabeth line, in a labeled secondary `IssueList`.
- Visiting `/lines/tfl-elizabeth` directly still works (a stale link degrades gracefully to a standalone, non-merged TfL-only view) but no longer appears as a link anywhere in the UI (confirm it's absent from `/lines`).
- Visit `/lines/northern` (a line with no TfL counterpart) and confirm no "TfL also reports:" section appears and behavior is otherwise unchanged.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/app/lines/\[id\]/page.tsx
git commit -m "feat: show TfL's Elizabeth line status as a secondary section on the line detail page"
```

---

## Self-review notes

- **Spec coverage:** Area 1's five design steps map onto tasks as: step 1 (mapping table) → Task 1; step 2 (suppress from `/public/lines`) → Task 2; step 3 (overlay onto detail response) → Tasks 3–4; step 4 (frontend) → Task 5; step 5 (history untouched) → enforced by omission, called out explicitly in Global Constraints and in Task 4's handler comment. The Area 1 hard constraint (no volatile reason text) → Task 3's `overlay_reason_text_is_stable_across_identical_calls` test. The spec's Testing Plan bullets for Area 1 all have a corresponding test in Tasks 1–4 above (list_lines suppression test, overlay-fetch present/absent tests, reason-stability regression test); the `line_status_history` bullet is satisfied by not touching that code path, not by a new test, since there is nothing to assert against that isn't already covered by the untouched, pre-existing history tests.
- **Placeholder scan:** none found — every step has literal code, not a description of code.
- **Type consistency:** `Option<Vec<common::LineStatus>>` is used consistently for an overlay's payload across Tasks 3 and 4 (`to_tfl_shape_with_overlay`'s `tfl_overlay: Option<&[LineStatus]>` parameter accepts the `.as_deref()` of `overlay_for`'s `Option<Vec<LineStatus>>` return); `tflStatus?: LineStatus[]` on the TypeScript side matches the JSON shape `to_tfl_shape_with_overlay` actually produces (an array of the same per-status shape as `lineStatuses`, via the existing `status_to_json`).

## Execution Handoff

Two execution options:

1. **Subagent-Driven (recommended)** — a fresh subagent per task, review between tasks, fast iteration. Requires the superpowers:subagent-driven-development skill.
2. **Inline Execution** — execute tasks in one session with checkpoints between them. Requires the superpowers:executing-plans skill.
