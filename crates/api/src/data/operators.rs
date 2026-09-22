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
        // Defensive guard (final whole-branch review, Fix 12): if `tocs`
        // ever somehow contained a literal `atoc_code = "TfL"` row
        // (implausible from the real RDM feed, but not structurally
        // prevented by this table), this loop must not emit a second
        // `code: "TfL"` entry alongside the synthetic one appended below --
        // two rows with the same code would be a duplicate React `key` on
        // the frontend list and make `operator_rollup`'s `.find()` silently
        // return only the first. The synthetic row below is always the sole
        // source of a `"TfL"` entry.
        if toc.code == common::TFL_OPERATOR {
            continue;
        }
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
        .filter(|row| operator_matches_tfl_rollup(&row.operators))
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

/// Whether one row's `operators` list should fold into the synthetic
/// `"TfL"` rollup built by [`all_operator_rollups`]: literally `"TfL"`
/// itself, or any of [`TFL_ADJACENT_OPERATORS`] (a London Overground /
/// Elizabeth line catalogue row that ALSO counts toward "TfL"'s rollup --
/// see that constant's own doc comment for the one-directional reasoning).
///
/// Extracted as a plain, synchronous function -- unlike
/// [`all_operator_rollups`], which needs a `PgPool` -- specifically so this,
/// one of the two genuinely novel pieces of logic in this module, can be
/// unit tested directly rather than only reachable through a DB-gated
/// integration test. See this module's `tfl_rollup_matching_tests` below,
/// and this plan's closing "Note for Phase 4" on why this guarantee matters
/// beyond this phase.
fn operator_matches_tfl_rollup(operators: &[String]) -> bool {
    operators.iter().any(|op| op == common::TFL_OPERATOR)
        || operators
            .iter()
            .any(|op| TFL_ADJACENT_OPERATORS.contains(&op.as_str()))
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
            .filter(|line| is_unmerged_tfl_line(&line.id))
            .map(|line| line.id),
    );
    queries::line_status_for_ids(pool, &ids).await
}

/// Whether a TfL line id has no NR catalogue counterpart it should be
/// merged into instead -- the same exclusion [`public_line_status_rows`]'s
/// own doc comment describes, and the inverse of
/// `routes::lines::is_merged_into_nr_line`, which applies the identical
/// check on `/public/lines` for the same reason (never double-count a
/// merged TfL line alongside its NR catalogue counterpart).
///
/// Extracted as a plain, synchronous wrapper around
/// `common::nr_line_id_for_tfl` -- the second of this module's two novel
/// pieces of logic, and the one this plan's closing "Note for Phase 4"
/// explicitly says a later phase depends on -- so it can be unit tested
/// directly without a `queries::TflLineSummary` or a database. See this
/// module's `unmerged_tfl_line_tests` below.
fn is_unmerged_tfl_line(line_id: &str) -> bool {
    common::nr_line_id_for_tfl(line_id).is_none()
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
    use common::{
        DataQuality, LineStatus, SampleAvailability, SampleStats, Severity, ValidityPeriod,
    };

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
        assert_eq!(
            build_rollup("SW".to_string(), "South Western Railway".to_string(), &[]),
            None
        );
    }

    #[test]
    fn worst_severity_uses_severity_rank_not_raw_discriminant_ordering() {
        // Diverted (discriminant 21) is numerically higher than MinorDelays
        // (discriminant 9) but is genuinely more severe -- the exact
        // non-monotonic case severity_rank exists to get right. A `.min()`
        // over raw `Severity` would pick MinorDelays; this must pick
        // Diverted.
        let a = row(
            "line-a",
            &["SW"],
            vec![status(Severity::MinorDelays, "minor", None)],
        );
        let b = row(
            "line-b",
            &["SW"],
            vec![status(Severity::Diverted, "diverted", None)],
        );
        let rollup = build_rollup(
            "SW".to_string(),
            "South Western Railway".to_string(),
            &[&a, &b],
        )
        .unwrap();
        assert_eq!(rollup.worst_severity, Severity::Diverted);
        assert_eq!(rollup.reason, "diverted");
    }

    #[test]
    fn line_ids_and_computed_at_reflect_every_matching_line() {
        let mut a = row(
            "line-a",
            &["SW"],
            vec![status(Severity::GoodService, "", None)],
        );
        a.computed_at = Utc::now() - chrono::Duration::hours(2);
        let b = row(
            "line-b",
            &["SW"],
            vec![status(Severity::GoodService, "", None)],
        );
        let rollup = build_rollup(
            "SW".to_string(),
            "South Western Railway".to_string(),
            &[&a, &b],
        )
        .unwrap();
        assert_eq!(
            rollup.line_ids,
            vec!["line-a".to_string(), "line-b".to_string()]
        );
        // Oldest, not newest -- Judgment Call in this module's own doc
        // comment on `computed_at`.
        assert_eq!(rollup.computed_at, Some(a.computed_at));
    }

    #[test]
    fn sample_stats_merge_across_lines_using_the_representative_status_per_line() {
        let a = row(
            "line-a",
            &["SW"],
            vec![status(
                Severity::GoodService,
                "",
                Some(SampleStats {
                    total: 10,
                    delayed: 1,
                    cancelled: 0,
                    skipped: 0,
                    avg_delay_minutes: 2.0,
                }),
            )],
        );
        let b = row(
            "line-b",
            &["SW"],
            vec![status(
                Severity::GoodService,
                "",
                Some(SampleStats {
                    total: 5,
                    delayed: 0,
                    cancelled: 0,
                    skipped: 0,
                    avg_delay_minutes: 0.0,
                }),
            )],
        );
        let rollup = build_rollup(
            "SW".to_string(),
            "South Western Railway".to_string(),
            &[&a, &b],
        )
        .unwrap();
        let stats = rollup.sample_stats.unwrap();
        assert_eq!(stats.total, 15);
        assert_eq!(stats.delayed, 1);
    }

    #[test]
    fn a_line_with_no_stats_on_any_status_contributes_no_sample_stats() {
        let a = row(
            "line-a",
            &["SW"],
            vec![status(Severity::GoodService, "", None)],
        );
        let rollup =
            build_rollup("SW".to_string(), "South Western Railway".to_string(), &[&a]).unwrap();
        assert_eq!(rollup.sample_stats, None);
    }
}

#[cfg(test)]
mod tfl_rollup_matching_tests {
    use super::*;

    fn ops(codes: &[&str]) -> Vec<String> {
        codes.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_row_carrying_the_literal_tfl_code_matches() {
        assert!(operator_matches_tfl_rollup(&ops(&["TfL"])));
    }

    #[test]
    fn a_row_carrying_london_overground_matches() {
        assert!(operator_matches_tfl_rollup(&ops(&["LO"])));
    }

    #[test]
    fn a_row_carrying_elizabeth_line_matches() {
        assert!(operator_matches_tfl_rollup(&ops(&["XR"])));
    }

    #[test]
    fn a_row_carrying_a_real_non_tfl_adjacent_operator_does_not_match() {
        // "SW" (South Western Railway) is a real ATOC code with no TfL
        // adjacency at all -- the negative case this predicate must get
        // right, or every real operator would silently fold into "TfL".
        assert!(!operator_matches_tfl_rollup(&ops(&["SW"])));
    }

    #[test]
    fn a_row_with_several_operators_matches_if_any_one_is_tfl_adjacent() {
        assert!(operator_matches_tfl_rollup(&ops(&["SW", "XR"])));
    }
}

#[cfg(test)]
mod unmerged_tfl_line_tests {
    use super::*;

    #[test]
    fn a_tfl_line_with_an_nr_counterpart_is_not_unmerged() {
        // Real example from `common::TFL_TO_NR_LINE_ID`, the same fixture
        // `routes::lines::is_merged_into_nr_line`'s own tests use --
        // `tfl-elizabeth` merges into the NR catalogue's `elizabeth-line`.
        assert!(!is_unmerged_tfl_line("tfl-elizabeth"));
    }

    #[test]
    fn an_overground_tfl_line_with_an_nr_counterpart_is_not_unmerged() {
        assert!(!is_unmerged_tfl_line("tfl-mildmay"));
    }

    #[test]
    fn a_tfl_line_with_no_nr_counterpart_is_unmerged() {
        // Real example with no entry in `common::TFL_TO_NR_LINE_ID` -- the
        // same fixture `routes::lines::is_merged_into_nr_line`'s own
        // negative-case test uses.
        assert!(is_unmerged_tfl_line("tfl-northern"));
    }
}

#[cfg(test)]
mod tfl_adjacent_operators_drift_guard_tests {
    use super::*;

    #[test]
    fn matches_the_frontends_own_copy_of_this_list() {
        // Guards exactly the kind of duplication
        // `crates/common/src/lib.rs`'s `severity_rank_tests::
        // rank_matches_the_frontends_group_table` already guards for a
        // different pair of duplicated constants: this Rust-side array has
        // no shared-constant bridge to
        // `frontend/app/lines/AllLinesTable.tsx`'s own
        // `TFL_ADJACENT_OPERATORS`, so drift between the two must be a test
        // failure here rather than a silently divergent operator rollup. If
        // this ever needs to change, update
        // `frontend/app/lines/AllLinesTable.tsx`'s `TFL_ADJACENT_OPERATORS`
        // in lockstep.
        assert_eq!(TFL_ADJACENT_OPERATORS, ["LO", "XR"]);
    }
}
