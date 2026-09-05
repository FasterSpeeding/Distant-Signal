//! The four TfL-shaped read endpoints: `/Line/Mode/{modes}/Status`,
//! `/Line/{ids}/Status`, `/StopPoint/{crs}/Disruption`,
//! `/Line/{id}/Status/{from}/to/{to}`. Unauthenticated, matching TfL's own
//! public API — including its URL scheme: `main.rs` merges this crate's
//! `router()` directly onto the top-level router (unprefixed), not
//! nested under `/public` like `routes::public_router()`'s other routes,
//! so paths match DESIGN.md's `GET /Line/Mode/national-rail/Status` etc.
//! exactly (see the comment on `routes::public_router()` for why nesting
//! under `/public` would break TfL-client compatibility, and why
//! `/public/health`/`/private/*` still need to keep their prefixes).
//!
//! The last route's handler takes `Path<(String, DateTime<Utc>,
//! DateTime<Utc>)>` — a tuple with a non-primitive `chrono::DateTime<Utc>`
//! in a multi-segment `Path` extraction. This was flagged up front as
//! possibly not compiling/parsing correctly; verified with a standalone
//! probe (a throwaway route + `tower::ServiceExt::oneshot` request, not
//! kept in this file) that it does compile and correctly parses RFC3339
//! timestamps from the URL path, both percent-encoded (`00%3A00%3A00Z`)
//! and literal (`00:00:00Z`, what `curl` actually sends). No workaround
//! needed — kept the brief's original code as-is.

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Json, Router as AxumRouter};
use chrono::{DateTime, Utc};
use common::{LineStatus, LineStatusReport, Severity};
use serde::Deserialize;
use serde_json::Value;

use crate::app::{App, Router};
use crate::auth::OptionalAuthenticatedUser;
use crate::data::custom_lines;
use crate::data::queries;
use crate::render::to_tfl_shape;

pub fn router() -> Router {
    AxumRouter::new()
        .route(
            "/Line/Mode/{mode}/Status",
            axum::routing::get(get_mode_status),
        )
        .route("/Line/{ids}/Status", axum::routing::get(get_line_status))
        .route(
            "/StopPoint/{crs}/Disruption",
            axum::routing::get(get_stop_point_disruption),
        )
        .route(
            "/Line/{id}/Status/{from}/to/{to}",
            axum::routing::get(get_line_status_history),
        )
        .route(
            "/Line/{id}/Stats/{from}/to/{to}",
            axum::routing::get(get_line_daily_stats),
        )
        .route(
            "/Line/{id}/Stats/HalfHourly/{from}/to/{to}",
            axum::routing::get(get_line_half_hourly_stats),
        )
        .route(
            "/Line/{id}/Stats/Hourly/{from}/to/{to}",
            axum::routing::get(get_line_hourly_stats),
        )
        .route(
            "/Line/{id}/Stats/SixHourly/{from}/to/{to}",
            axum::routing::get(get_line_six_hourly_stats),
        )
        // Decision 4 scaffolding -- siblings of the two routes above,
        // reading line_status_{daily,half_hourly}_coverage_stats instead.
        // Always return `[]` today: nothing writes those tables until a
        // future full-coverage producer exists. See
        // docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
        // Decision 4.
        .route(
            "/Line/{id}/Stats/Coverage/{from}/to/{to}",
            axum::routing::get(get_line_daily_coverage_stats),
        )
        .route(
            "/Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}",
            axum::routing::get(get_line_half_hourly_coverage_stats),
        )
}

#[derive(Debug, Deserialize)]
pub struct DetailQuery {
    #[serde(default)]
    pub detail: bool,
}

fn to_report(row: queries::LineStatusRow) -> LineStatusReport {
    LineStatusReport {
        id: row.id,
        name: row.name,
        mode_name: row.mode_name,
        operators: row.operators,
        statuses: row.statuses,
    }
}

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
fn overlay_for(
    row: &queries::LineStatusRow,
    tfl_rows: &[queries::LineStatusRow],
) -> Option<Vec<LineStatus>> {
    let tfl_id = common::tfl_line_id_for_nr(&row.id)?;
    tfl_rows
        .iter()
        .find(|r| r.id == tfl_id)
        .map(|r| r.statuses.clone())
}

fn rows_to_json(rows: Vec<queries::LineStatusRow>, detail: bool) -> Vec<Value> {
    rows.into_iter()
        .map(|row| {
            let computed_at = row.computed_at;
            let report = to_report(row);
            to_tfl_shape(&report, computed_at, detail)
        })
        .collect()
}

/// Drops any row whose id is a private custom line the caller doesn't own.
/// Catalogue/TfL rows (no `custom-` prefix) are always kept untouched.
/// `user` is `None` for an anonymous caller -- every custom-line row is
/// dropped for them, since an anonymous caller can never be the owner of
/// anything.
async fn filter_private_custom_rows(
    pool: &sqlx::PgPool,
    rows: Vec<queries::LineStatusRow>,
    user: &Option<crate::auth::AuthenticatedUser>,
) -> anyhow::Result<Vec<queries::LineStatusRow>> {
    let custom_ids: Vec<String> = rows
        .iter()
        .filter(|r| r.id.starts_with("custom-"))
        .map(|r| r.id.clone())
        .collect();
    if custom_ids.is_empty() {
        return Ok(rows);
    }
    let owners = custom_lines::owners_for_ids(pool, &custom_ids).await?;
    Ok(rows
        .into_iter()
        .filter(|row| {
            let Some(owner) = owners.get(&row.id) else {
                return true; // not a custom-line row after all (shouldn't happen given the prefix check, but never drop on a lookup miss)
            };
            match (user, owner) {
                (Some(caller), Some(owner_id)) => &caller.id == owner_id,
                _ => false,
            }
        })
        .collect())
}

/// Every mode this deployment has data for. `national-rail` is written by
/// the aggregator from Knowledgebase incidents and LDBWS samples; the other
/// five are written by `crates/poller-tfl` via `/private/tfl-line-status`.
///
/// The list is closed rather than "anything in the database" so that a
/// typo, or a real TfL mode this app deliberately does not ingest (`bus`,
/// `river-bus`, `cable-car`), gets a 400 that names the problem instead of
/// an empty array that reads as "no disruption anywhere".
const SUPPORTED_MODES: [&str; 6] = [
    "national-rail",
    "tube",
    "dlr",
    "overground",
    "elizabeth-line",
    "tram",
];

/// Splits and validates TfL's comma-separated `{modes}` path segment.
/// Comma-separated modes are TfL's own contract for this URL — mimicking it
/// is the whole point of these four endpoints — and it lets the frontend
/// fetch every displayed line in one request rather than six.
fn parse_modes(raw: &str) -> Result<Vec<String>, String> {
    let modes: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .map(str::to_string)
        .collect();

    if modes.is_empty() {
        return Err("no mode given".to_string());
    }
    if let Some(unsupported) = modes
        .iter()
        .find(|mode| !SUPPORTED_MODES.contains(&mode.as_str()))
    {
        return Err(format!("unsupported mode: {unsupported}"));
    }
    Ok(modes)
}

async fn get_mode_status(
    State(app): State<App>,
    Path(modes): Path<String>,
    Query(query): Query<DetailQuery>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let modes = parse_modes(&modes).map_err(|message| (StatusCode::BAD_REQUEST, message))?;

    let rows = queries::line_status_for_modes(&app.database, &modes)
        .await
        .map_err(internal_error)?;
    let rows = filter_private_custom_rows(&app.database, rows, &user)
        .await
        .map_err(internal_error)?;

    Ok(Json(rows_to_json(rows, query.detail)))
}

async fn get_line_status(
    State(app): State<App>,
    Path(ids): Path<String>,
    Query(query): Query<DetailQuery>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let ids: Vec<String> = ids.split(',').map(|s| s.to_string()).collect();

    let rows = queries::line_status_for_ids(&app.database, &ids)
        .await
        .map_err(internal_error)?;
    let rows = filter_private_custom_rows(&app.database, rows, &user)
        .await
        .map_err(internal_error)?;

    if rows.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no matching line(s): {}", ids.join(",")),
        ));
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
                crate::render::to_tfl_shape_with_overlay(
                    &report,
                    computed_at,
                    query.detail,
                    overlay.as_deref(),
                )
            })
            .collect(),
    ))
}

/// `[]` is a real, meaningful response here -- every line covering this
/// station currently reports Good Service -- so it must never also be what
/// "we have no line coverage for this station at all" looks like. Those are
/// different facts (a curated-catalogue gap vs. a genuinely quiet station)
/// and this handler used to collapse them into the identical empty array,
/// silently telling a user looking up an uncovered station "no disruptions"
/// exactly as confidently as it tells a user looking up a fully-covered,
/// genuinely-fine one. See
/// docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md
/// ("Problem statement") for the original write-up of this exact gap.
///
/// Fixed the same way this file's sibling `get_line_status` already
/// distinguishes "no matching line(s)" from "matched, nothing to report":
/// a 404 with a message that names the CRS, rather than a shape change to
/// the success body. This keeps every real 200 response (an array of
/// TfL-shaped reports, possibly empty because every covering line is fine)
/// completely unchanged, and reuses the exact `ApiNotFoundError` path the
/// frontend already has for "this call came back not-found" -- no new
/// wrapper type, no new response shape, on either side.
async fn get_stop_point_disruption(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let matching_line_ids: Vec<String> = app
        .config
        .lines
        .iter()
        .filter(|line| line.has_station(&crs))
        .map(|line| line.id.clone())
        .collect();

    if matching_line_ids.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no line coverage for stop point: {crs}"),
        ));
    }

    let rows = queries::line_status_for_ids(&app.database, &matching_line_ids)
        .await
        .map_err(internal_error)?;

    let disruptions: Vec<Value> = rows
        .into_iter()
        .flat_map(|row| {
            let computed_at = row.computed_at;
            let statuses: Vec<LineStatus> = row
                .statuses
                .into_iter()
                .filter(|s| s.severity != Severity::GoodService)
                .collect();
            let report = LineStatusReport {
                id: row.id,
                name: row.name,
                mode_name: row.mode_name,
                operators: row.operators,
                statuses,
            };
            if report.statuses.is_empty() {
                None
            } else {
                Some(to_tfl_shape(&report, computed_at, true))
            }
        })
        .collect();

    Ok(Json(disruptions))
}

async fn get_line_status_history(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
    OptionalAuthenticatedUser(user): OptionalAuthenticatedUser,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    if id.starts_with("custom-") {
        let owners = custom_lines::owners_for_ids(&app.database, std::slice::from_ref(&id))
            .await
            .map_err(internal_error)?;
        let owned_by_caller = match (&user, owners.get(&id)) {
            (Some(caller), Some(Some(owner_id))) => &caller.id == owner_id,
            _ => false,
        };
        if !owned_by_caller {
            return Ok(Json(vec![])); // identical shape to a genuinely unknown id -- this route has never distinguished the two.
        }
    }

    let history = queries::line_status_history_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        history
            .into_iter()
            .map(|(computed_at, statuses)| {
                let report = LineStatusReport {
                    id: id.clone(),
                    name: String::new(),
                    mode_name: String::new(),
                    operators: vec![],
                    statuses,
                };
                to_tfl_shape(&report, computed_at, true)
            })
            .collect(),
    ))
}

/// Derives the four rate/average fields from one stored rollup row,
/// guarding every division against a zero denominator -- a day CAN have
/// total: 0 if every contributing cycle itself had total: 0 (rare given
/// min_sample_size, not impossible). Pure so it's unit-testable without a
/// database.
fn daily_stats_to_json(row: queries::DailyStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| {
        if row.total > 0 {
            numerator as f64 / row.total as f64
        } else {
            0.0
        }
    };

    serde_json::json!({
        "day": row.day,
        "sampleCycles": row.sample_cycles,
        "total": row.total,
        "delayed": row.delayed,
        "cancelled": row.cancelled,
        "skipped": row.skipped,
        "avgDelayMinutes": avg_delay_minutes,
        "delayRate": rate(row.delayed),
        "cancellationRate": rate(row.cancelled),
        "skipRate": rate(row.skipped),
    })
}

async fn get_line_daily_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::daily_stats_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(rows.into_iter().map(daily_stats_to_json).collect()))
}

/// Half-hourly sibling of `daily_stats_to_json` -- identical
/// rate-derivation logic, `halfHourStart` (an ISO instant) in place of
/// `day`. Originally `hourly_stats_to_json` emitting `hourStart`; renamed
/// alongside the table/route when the bucket size was halved -- see git
/// history for the hourly-era version.
fn half_hourly_stats_to_json(row: queries::HalfHourlyStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| {
        if row.total > 0 {
            numerator as f64 / row.total as f64
        } else {
            0.0
        }
    };

    serde_json::json!({
        "halfHourStart": row.half_hour_start,
        "sampleCycles": row.sample_cycles,
        "total": row.total,
        "delayed": row.delayed,
        "cancelled": row.cancelled,
        "skipped": row.skipped,
        "avgDelayMinutes": avg_delay_minutes,
        "delayRate": rate(row.delayed),
        "cancellationRate": rate(row.cancelled),
        "skipRate": rate(row.skipped),
    })
}

async fn get_line_half_hourly_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::half_hourly_stats_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter().map(half_hourly_stats_to_json).collect(),
    ))
}

/// Sub-daily sibling of `half_hourly_stats_to_json` -- identical
/// rate-derivation logic, `bucketStart` in place of `halfHourStart`. A
/// distinct field name is deliberate: reusing "halfHourStart" here would
/// misname a 1-hour or 6-hour bucket's start instant. Backs BOTH new
/// sub-daily routes (`get_line_hourly_stats`/`get_line_six_hourly_stats`)
/// -- they share this one function the same way they share
/// `queries::sub_daily_stats_for_range` itself (Decision 2 of
/// docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md).
fn sub_daily_stats_to_json(row: queries::HalfHourlyStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| {
        if row.total > 0 {
            numerator as f64 / row.total as f64
        } else {
            0.0
        }
    };

    serde_json::json!({
        "bucketStart": row.half_hour_start,
        "sampleCycles": row.sample_cycles,
        "total": row.total,
        "delayed": row.delayed,
        "cancelled": row.cancelled,
        "skipped": row.skipped,
        "avgDelayMinutes": avg_delay_minutes,
        "delayRate": rate(row.delayed),
        "cancellationRate": rate(row.cancelled),
        "skipRate": rate(row.skipped),
    })
}

async fn get_line_hourly_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::sub_daily_stats_for_range(&app.database, &id, from, to, 60)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(sub_daily_stats_to_json).collect(),
    ))
}

async fn get_line_six_hourly_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::sub_daily_stats_for_range(&app.database, &id, from, to, 360)
        .await
        .map_err(internal_error)?;
    Ok(Json(
        rows.into_iter().map(sub_daily_stats_to_json).collect(),
    ))
}

/// Full-coverage sibling of `daily_stats_to_json` -- identical
/// rate-derivation logic, `resolvedWindows` in place of `sampleCycles`.
/// See docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
/// Decision 4.
fn daily_coverage_stats_to_json(row: queries::DailyCoverageStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| {
        if row.total > 0 {
            numerator as f64 / row.total as f64
        } else {
            0.0
        }
    };

    serde_json::json!({
        "day": row.day,
        "resolvedWindows": row.resolved_windows,
        "total": row.total,
        "delayed": row.delayed,
        "cancelled": row.cancelled,
        "skipped": row.skipped,
        "avgDelayMinutes": avg_delay_minutes,
        "delayRate": rate(row.delayed),
        "cancellationRate": rate(row.cancelled),
        "skipRate": rate(row.skipped),
    })
}

async fn get_line_daily_coverage_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::daily_coverage_stats_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter().map(daily_coverage_stats_to_json).collect(),
    ))
}

/// Half-hourly sibling of `daily_coverage_stats_to_json`, `halfHourStart`
/// in place of `day` -- mirrors `half_hourly_stats_to_json`'s own
/// relationship to `daily_stats_to_json`.
fn half_hourly_coverage_stats_to_json(row: queries::HalfHourlyCoverageStatsRow) -> Value {
    let avg_delay_minutes = if row.running_count > 0 {
        row.delay_minutes_sum / row.running_count as f64
    } else {
        0.0
    };
    let rate = |numerator: i64| {
        if row.total > 0 {
            numerator as f64 / row.total as f64
        } else {
            0.0
        }
    };

    serde_json::json!({
        "halfHourStart": row.half_hour_start,
        "resolvedWindows": row.resolved_windows,
        "total": row.total,
        "delayed": row.delayed,
        "cancelled": row.cancelled,
        "skipped": row.skipped,
        "avgDelayMinutes": avg_delay_minutes,
        "delayRate": rate(row.delayed),
        "cancellationRate": rate(row.cancelled),
        "skipRate": rate(row.skipped),
    })
}

async fn get_line_half_hourly_coverage_stats(
    State(app): State<App>,
    Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let rows = queries::half_hourly_coverage_stats_for_range(&app.database, &id, from, to)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter()
            .map(half_hourly_coverage_stats_to_json)
            .collect(),
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "line status query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_mode_still_works() {
        assert_eq!(
            parse_modes("national-rail").unwrap(),
            vec!["national-rail".to_string()]
        );
    }

    #[test]
    fn every_tfl_mode_this_app_ingests_is_accepted() {
        // The gate used to be `if mode != "national-rail" { 400 }`, which
        // made every ingested TfL line unreachable through this endpoint —
        // and it is the endpoint both frontend list pages are built on.
        let modes = parse_modes("tube,dlr,overground,elizabeth-line,tram").unwrap();
        assert_eq!(modes.len(), 5);
        assert!(modes.contains(&"elizabeth-line".to_string()));
    }

    #[test]
    fn whitespace_and_empty_segments_are_tolerated() {
        assert_eq!(
            parse_modes("tube, dlr,").unwrap(),
            vec!["tube".to_string(), "dlr".to_string()]
        );
    }

    #[test]
    fn an_unsupported_mode_is_named_in_the_error() {
        // `bus` and `river-bus` are real TfL modes this app deliberately
        // does not ingest, so "no results" would be a misleading answer.
        let err = parse_modes("tube,bus").unwrap_err();
        assert!(
            err.contains("bus"),
            "error should name the offending mode: {err}"
        );
    }

    #[test]
    fn an_empty_mode_list_is_rejected_rather_than_matching_everything() {
        assert!(parse_modes("").is_err());
        assert!(parse_modes(",,").is_err());
    }

    use chrono::Utc;
    use common::{DataQuality, SampleAvailability, Severity, ValidityPeriod};

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
            validity: ValidityPeriod {
                from_date: Utc::now(),
                to_date: None,
                is_now: true,
            },
            disruption: None,
            data_quality: DataQuality::Tfl,
            sample_stats: None,
            sample_availability: SampleAvailability::NoCoverage,
            full_coverage_stats: None,
            full_coverage_availability: common::FullCoverageAvailability::NotEnabled,
        }
    }

    #[test]
    fn tfl_ids_to_overlay_includes_only_rows_with_a_tfl_counterpart() {
        let rows = vec![row("elizabeth-line", vec![]), row("northern", vec![])];
        assert_eq!(tfl_ids_to_overlay(&rows), vec!["tfl-elizabeth".to_string()]);
    }

    #[test]
    fn tfl_ids_to_overlay_covers_multiple_merged_lines_at_once() {
        // Area 2 -- see docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md.
        // Proves this is genuinely table-driven across more than one entry,
        // not just Elizabeth line's single row.
        let rows = vec![
            row("elizabeth-line", vec![]),
            row("overground-mildmay", vec![]),
            row("northern", vec![]),
        ];
        assert_eq!(
            tfl_ids_to_overlay(&rows),
            vec!["tfl-elizabeth".to_string(), "tfl-mildmay".to_string()]
        );
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

    fn daily_stats_row(
        total: i64,
        delayed: i64,
        cancelled: i64,
        skipped: i64,
        running_count: i64,
        delay_minutes_sum: f64,
    ) -> queries::DailyStatsRow {
        queries::DailyStatsRow {
            day: chrono::NaiveDate::from_ymd_opt(2026, 8, 15).unwrap(),
            sample_cycles: 12,
            total,
            delayed,
            cancelled,
            skipped,
            running_count,
            delay_minutes_sum,
        }
    }

    #[test]
    fn daily_stats_to_json_computes_rates_for_a_normal_row() {
        let row = daily_stats_row(100, 10, 5, 2, 95, 190.0);
        let json = daily_stats_to_json(row);

        assert_eq!(json["day"], serde_json::json!("2026-08-15"));
        assert_eq!(json["sampleCycles"], serde_json::json!(12));
        assert_eq!(json["total"], serde_json::json!(100));
        assert_eq!(json["delayed"], serde_json::json!(10));
        assert_eq!(json["cancelled"], serde_json::json!(5));
        assert_eq!(json["skipped"], serde_json::json!(2));
        assert_eq!(json["avgDelayMinutes"], serde_json::json!(2.0)); // 190.0 / 95
        assert_eq!(json["delayRate"], serde_json::json!(0.1)); // 10 / 100
        assert_eq!(json["cancellationRate"], serde_json::json!(0.05)); // 5 / 100
        assert_eq!(json["skipRate"], serde_json::json!(0.02)); // 2 / 100
    }

    #[test]
    fn daily_stats_to_json_zero_total_and_running_count_never_produces_nan_or_infinity() {
        let row = daily_stats_row(0, 0, 0, 0, 0, 0.0);
        let json = daily_stats_to_json(row);

        for field in [
            "avgDelayMinutes",
            "delayRate",
            "cancellationRate",
            "skipRate",
        ] {
            let value = json[field]
                .as_f64()
                .unwrap_or_else(|| panic!("{field} should be a JSON number"));
            assert!(value.is_finite(), "{field} should be finite, got {value}");
            assert_eq!(
                value, 0.0,
                "{field} should be exactly 0.0 for a zero-denominator row"
            );
        }
    }

    #[tokio::test]
    async fn get_line_daily_stats_route_mounts_and_parses_naive_date_path_segments() {
        // Mirrors this file's module doc comment: the sibling
        // `/Line/{id}/Status/{from}/to/{to}` route needed a throwaway-router
        // probe to confirm a multi-segment `Path` extraction with a
        // non-primitive type actually parses from the URL. This route swaps
        // `DateTime<Utc>` for `chrono::NaiveDate` (Open judgment call #4),
        // so the same risk applies and is checked the same way -- except
        // this probe is kept, using the exact route string `router()`
        // registers, rather than discarded.
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        async fn probe(
            Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
        ) -> String {
            format!("{id}|{from}|{to}")
        }

        let app: axum::Router =
            axum::Router::new().route("/Line/{id}/Stats/{from}/to/{to}", axum::routing::get(probe));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/2026-08-01/to/2026-08-31")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(body, "northern|2026-08-01|2026-08-31");
    }

    fn half_hourly_stats_row(
        total: i64,
        delayed: i64,
        cancelled: i64,
        skipped: i64,
        running_count: i64,
        delay_minutes_sum: f64,
    ) -> queries::HalfHourlyStatsRow {
        queries::HalfHourlyStatsRow {
            half_hour_start: "2026-08-15T14:00:00Z".parse().unwrap(),
            sample_cycles: 12,
            total,
            delayed,
            cancelled,
            skipped,
            running_count,
            delay_minutes_sum,
        }
    }

    #[test]
    fn half_hourly_stats_to_json_computes_rates_for_a_normal_row() {
        let row = half_hourly_stats_row(100, 10, 5, 2, 95, 190.0);
        let json = half_hourly_stats_to_json(row);

        assert_eq!(
            json["halfHourStart"],
            serde_json::json!("2026-08-15T14:00:00Z")
        );
        assert_eq!(json["avgDelayMinutes"], serde_json::json!(2.0));
        assert_eq!(json["delayRate"], serde_json::json!(0.1));
    }

    #[test]
    fn half_hourly_stats_to_json_zero_total_never_produces_nan_or_infinity() {
        let row = half_hourly_stats_row(0, 0, 0, 0, 0, 0.0);
        let json = half_hourly_stats_to_json(row);
        for field in [
            "avgDelayMinutes",
            "delayRate",
            "cancellationRate",
            "skipRate",
        ] {
            let value = json[field].as_f64().unwrap();
            assert!(value.is_finite());
            assert_eq!(value, 0.0);
        }
    }

    #[tokio::test]
    async fn both_stats_routes_coexist_and_route_to_the_correct_handler() {
        // The real risk this test exists for: `/Line/{id}/Stats/{from}/to/{to}`
        // (NaiveDate) and `/Line/{id}/Stats/HalfHourly/{from}/to/{to}` (DateTime<Utc>)
        // share a path prefix with a dynamic segment at the exact position the
        // new route's literal "HalfHourly" segment occupies. Two throwaway probe
        // handlers, mounted the same way the daily route's own precedent probe
        // (`get_line_daily_stats_route_mounts_and_parses_naive_date_path_segments`,
        // above) does, confirm axum's router sends each URL to the right one
        // rather than assuming it from matchit's documented priority rules.
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        async fn daily_probe(
            Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
        ) -> String {
            format!("daily:{id}|{from}|{to}")
        }
        async fn half_hourly_probe(
            Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
        ) -> String {
            format!("half-hourly:{id}|{from}|{to}")
        }

        let app: axum::Router = axum::Router::new()
            .route(
                "/Line/{id}/Stats/{from}/to/{to}",
                axum::routing::get(daily_probe),
            )
            .route(
                "/Line/{id}/Stats/HalfHourly/{from}/to/{to}",
                axum::routing::get(half_hourly_probe),
            );

        let daily_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/2026-08-01/to/2026-08-31")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(daily_response.status(), StatusCode::OK);
        let daily_body = axum::body::to_bytes(daily_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(daily_body.to_vec()).unwrap(),
            "daily:northern|2026-08-01|2026-08-31"
        );

        let half_hourly_response = app
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/HalfHourly/2026-08-31T00:00:00Z/to/2026-09-01T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            half_hourly_response.status(),
            StatusCode::OK,
            "the HalfHourly route must not be shadowed by the sibling NaiveDate route"
        );
        let half_hourly_body = axum::body::to_bytes(half_hourly_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(half_hourly_body.to_vec()).unwrap(),
            "half-hourly:northern|2026-08-31 00:00:00 UTC|2026-09-01 00:00:00 UTC",
        );
    }

    #[test]
    fn sub_daily_stats_to_json_uses_bucket_start_not_half_hour_start() {
        let row = half_hourly_stats_row(100, 10, 5, 2, 95, 190.0);
        let json = sub_daily_stats_to_json(row);

        assert_eq!(
            json["bucketStart"],
            serde_json::json!("2026-08-15T14:00:00Z")
        );
        assert!(
            json.get("halfHourStart").is_none(),
            "must not also expose the half-hour-specific field name"
        );
        assert_eq!(json["avgDelayMinutes"], serde_json::json!(2.0));
        assert_eq!(json["delayRate"], serde_json::json!(0.1));
    }

    #[test]
    fn sub_daily_stats_to_json_zero_total_never_produces_nan_or_infinity() {
        let row = half_hourly_stats_row(0, 0, 0, 0, 0, 0.0);
        let json = sub_daily_stats_to_json(row);
        for field in [
            "avgDelayMinutes",
            "delayRate",
            "cancellationRate",
            "skipRate",
        ] {
            let value = json[field].as_f64().unwrap();
            assert!(value.is_finite());
            assert_eq!(value, 0.0);
        }
    }

    #[tokio::test]
    async fn hourly_and_six_hourly_routes_are_not_shadowed_by_the_daily_or_half_hourly_routes() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        async fn hourly_probe(
            Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
        ) -> String {
            format!("hourly:{id}|{from}|{to}")
        }
        async fn six_hourly_probe(
            Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
        ) -> String {
            format!("six-hourly:{id}|{from}|{to}")
        }
        async fn daily_probe(
            Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
        ) -> String {
            format!("daily:{id}|{from}|{to}")
        }

        let app: axum::Router = axum::Router::new()
            .route(
                "/Line/{id}/Stats/{from}/to/{to}",
                axum::routing::get(daily_probe),
            )
            .route(
                "/Line/{id}/Stats/Hourly/{from}/to/{to}",
                axum::routing::get(hourly_probe),
            )
            .route(
                "/Line/{id}/Stats/SixHourly/{from}/to/{to}",
                axum::routing::get(six_hourly_probe),
            );

        let hourly_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/Hourly/2026-08-31T00:00:00Z/to/2026-09-01T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(hourly_response.status(), StatusCode::OK);
        let hourly_body = axum::body::to_bytes(hourly_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(hourly_body.to_vec()).unwrap(),
            "hourly:northern|2026-08-31 00:00:00 UTC|2026-09-01 00:00:00 UTC"
        );

        let six_hourly_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/SixHourly/2026-08-31T00:00:00Z/to/2026-09-01T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(six_hourly_response.status(), StatusCode::OK);

        let daily_response = app
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/2026-08-01/to/2026-08-31")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            daily_response.status(),
            StatusCode::OK,
            "the daily NaiveDate route must still work alongside the two new literal-segment routes"
        );
    }

    // --- Decision 4 scaffolding: line_status_{daily,half_hourly}_coverage_stats routes ---

    fn daily_coverage_stats_row(
        total: i64,
        delayed: i64,
        cancelled: i64,
        skipped: i64,
        running_count: i64,
        delay_minutes_sum: f64,
    ) -> queries::DailyCoverageStatsRow {
        queries::DailyCoverageStatsRow {
            day: chrono::NaiveDate::from_ymd_opt(2026, 9, 3).unwrap(),
            resolved_windows: 12,
            total,
            delayed,
            cancelled,
            skipped,
            running_count,
            delay_minutes_sum,
        }
    }

    #[test]
    fn daily_coverage_stats_to_json_computes_rates_for_a_normal_row() {
        let row = daily_coverage_stats_row(100, 10, 5, 2, 95, 190.0);
        let json = daily_coverage_stats_to_json(row);

        assert_eq!(json["day"], serde_json::json!("2026-09-03"));
        assert_eq!(json["resolvedWindows"], serde_json::json!(12));
        assert_eq!(json["total"], serde_json::json!(100));
        assert_eq!(json["delayed"], serde_json::json!(10));
        assert_eq!(json["cancelled"], serde_json::json!(5));
        assert_eq!(json["skipped"], serde_json::json!(2));
        assert_eq!(json["avgDelayMinutes"], serde_json::json!(2.0)); // 190.0 / 95
        assert_eq!(json["delayRate"], serde_json::json!(0.1)); // 10 / 100
        assert_eq!(json["cancellationRate"], serde_json::json!(0.05)); // 5 / 100
        assert_eq!(json["skipRate"], serde_json::json!(0.02)); // 2 / 100
    }

    #[test]
    fn daily_coverage_stats_to_json_zero_total_and_running_count_never_produces_nan_or_infinity() {
        let row = daily_coverage_stats_row(0, 0, 0, 0, 0, 0.0);
        let json = daily_coverage_stats_to_json(row);

        for field in [
            "avgDelayMinutes",
            "delayRate",
            "cancellationRate",
            "skipRate",
        ] {
            let value = json[field]
                .as_f64()
                .unwrap_or_else(|| panic!("{field} should be a JSON number"));
            assert!(value.is_finite(), "{field} should be finite, got {value}");
            assert_eq!(
                value, 0.0,
                "{field} should be exactly 0.0 for a zero-denominator row"
            );
        }
    }

    fn half_hourly_coverage_stats_row(
        total: i64,
        delayed: i64,
        cancelled: i64,
        skipped: i64,
        running_count: i64,
        delay_minutes_sum: f64,
    ) -> queries::HalfHourlyCoverageStatsRow {
        queries::HalfHourlyCoverageStatsRow {
            half_hour_start: "2026-09-03T14:00:00Z".parse().unwrap(),
            resolved_windows: 12,
            total,
            delayed,
            cancelled,
            skipped,
            running_count,
            delay_minutes_sum,
        }
    }

    #[test]
    fn half_hourly_coverage_stats_to_json_computes_rates_for_a_normal_row() {
        let row = half_hourly_coverage_stats_row(100, 10, 5, 2, 95, 190.0);
        let json = half_hourly_coverage_stats_to_json(row);

        assert_eq!(
            json["halfHourStart"],
            serde_json::json!("2026-09-03T14:00:00Z")
        );
        assert_eq!(json["resolvedWindows"], serde_json::json!(12));
        assert_eq!(json["avgDelayMinutes"], serde_json::json!(2.0));
        assert_eq!(json["delayRate"], serde_json::json!(0.1));
    }

    #[test]
    fn half_hourly_coverage_stats_to_json_zero_total_never_produces_nan_or_infinity() {
        let row = half_hourly_coverage_stats_row(0, 0, 0, 0, 0, 0.0);
        let json = half_hourly_coverage_stats_to_json(row);
        for field in [
            "avgDelayMinutes",
            "delayRate",
            "cancellationRate",
            "skipRate",
        ] {
            let value = json[field].as_f64().unwrap();
            assert!(value.is_finite());
            assert_eq!(value, 0.0);
        }
    }

    async fn coverage_probe(
        Path((id, from, to)): Path<(String, chrono::NaiveDate, chrono::NaiveDate)>,
    ) -> String {
        format!("coverage-daily:{id}|{from}|{to}")
    }

    async fn coverage_half_hourly_probe(
        Path((id, from, to)): Path<(String, DateTime<Utc>, DateTime<Utc>)>,
    ) -> String {
        format!("coverage-half-hourly:{id}|{from}|{to}")
    }

    #[tokio::test]
    async fn coverage_stats_route_paths_parse_the_expected_path_segments() {
        // A lighter-weight, hand-rolled-router version of the test above,
        // proving the exact path strings `router()` registers for the two
        // new routes parse their path segments correctly -- mirrors
        // get_line_daily_stats_route_mounts_and_parses_naive_date_path_segments's
        // own probe pattern.
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app: axum::Router = axum::Router::new()
            .route(
                "/Line/{id}/Stats/Coverage/{from}/to/{to}",
                axum::routing::get(coverage_probe),
            )
            .route(
                "/Line/{id}/Stats/Coverage/HalfHourly/{from}/to/{to}",
                axum::routing::get(coverage_half_hourly_probe),
            );

        let daily_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/Coverage/2026-08-01/to/2026-08-31")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(daily_response.status(), StatusCode::OK);
        let daily_body = axum::body::to_bytes(daily_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(daily_body.to_vec()).unwrap(),
            "coverage-daily:northern|2026-08-01|2026-08-31"
        );

        let half_hourly_response = app
            .oneshot(
                Request::builder()
                    .uri("/Line/northern/Stats/Coverage/HalfHourly/2026-08-31T00:00:00Z/to/2026-09-01T00:00:00Z")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(half_hourly_response.status(), StatusCode::OK);
        let half_hourly_body = axum::body::to_bytes(half_hourly_response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            String::from_utf8(half_hourly_body.to_vec()).unwrap(),
            "coverage-half-hourly:northern|2026-08-31 00:00:00 UTC|2026-09-01 00:00:00 UTC",
        );
    }
}

/// HTTP-layer tests for this task's three ownership-filtered routes
/// (`get_mode_status`, `get_line_status`, `get_line_status_history`), plus
/// one regression test for `get_stop_point_disruption` confirming it's
/// genuinely untouched. Follows the `db_tests` convention Task 4
/// established in `crate::routes::lines::db_tests` (`test_app`/
/// `test_router`/`seed_session` built by hand against a real `App`,
/// exercised through the real `axum::Router` via `tower::ServiceExt::oneshot`)
/// -- kept as this file's own colocated copy rather than importing
/// `lines.rs`'s private helpers, per that module's own doc comment ("promote
/// only once a second file actually duplicates this setup" -- this is that
/// second file, but the helpers stay `pub(self)` to each file until/unless a
/// third file needs them too).
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::hash_session_token;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::custom_lines::{self, NewCustomLine};
    use crate::data::users::insert_session;

    /// Every `ServiceArguments` field filled with an inert placeholder
    /// except `lines`, which the caller supplies -- the one field a
    /// catalogue-id test actually needs to vary. Copied from
    /// `crate::routes::lines::db_tests::test_app` -- see this module's own
    /// doc comment for why it's not shared cross-file.
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
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            metrics_enabled: false,
            defaults_file: None,
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
            // `Client::open` only parses the URL, never opens a socket --
            // see `AppState::redis`'s doc comment. None of this file's
            // routes touch Redis at all.
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
        })
    }

    /// The real `line_status::router()`, merged unprefixed exactly as
    /// `main.rs` does (see this file's module doc comment for why these
    /// four routes are deliberately not nested under `/public`), turned
    /// into a `tower::Service` a test can drive with `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .with_state(app)
    }

    /// Seeds a real, resolvable session for `user_id` (creating the user
    /// if it doesn't already exist) and returns the *raw* token -- send it
    /// as `Cookie: distant_signal_session=<raw>`, never the hash `sessions`
    /// actually stores.
    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");

        let raw_token = format!("test-raw-session-token-for-{user_id}");
        insert_session(pool, &hash_session_token(&raw_token), user_id, 14)
            .await
            .expect("seed fixture session");
        raw_token
    }

    /// Deletes a fixture user and everything that cascades from it
    /// (`sessions`, owned `custom_lines`, `pinned_lines`).
    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Seeds (or updates) one `line_status` row directly -- this file's
    /// routes read straight from that table, so tests don't need a real
    /// aggregator/poller run to populate it. `statuses_json` is raw JSONB
    /// text; fields `common::LineStatus` marks `#[serde(default)]`
    /// (`data_quality`, `disruption`, `sample_stats`) can be omitted.
    async fn seed_line_status(pool: &PgPool, id: &str, mode_name: &str, statuses_json: &str) {
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, computed_at) \
             VALUES ($1, $2, $3, '{}', $4::jsonb, NOW()) \
             ON CONFLICT (line_id) DO UPDATE SET \
                mode_name = EXCLUDED.mode_name, statuses = EXCLUDED.statuses, computed_at = EXCLUDED.computed_at",
        )
        .bind(id)
        .bind(format!("Test {id}"))
        .bind(mode_name)
        .bind(statuses_json)
        .execute(pool)
        .await
        .expect("seed fixture line_status row");
    }

    async fn cleanup_line_status(pool: &PgPool, id: &str) {
        sqlx::query("DELETE FROM line_status WHERE line_id = $1")
            .bind(id)
            .execute(pool)
            .await
            .expect("cleanup fixture line_status row");
    }

    /// Seeds one `line_status_history` row -- `get_line_status_history`
    /// reads from this table, never `line_status` itself.
    async fn seed_line_status_history(pool: &PgPool, id: &str, statuses_json: &str) {
        sqlx::query(
            "INSERT INTO line_status_history (line_id, statuses, computed_at) VALUES ($1, $2::jsonb, NOW())",
        )
        .bind(id)
        .bind(statuses_json)
        .execute(pool)
        .await
        .expect("seed fixture line_status_history row");
    }

    async fn cleanup_line_status_history(pool: &PgPool, id: &str) {
        sqlx::query("DELETE FROM line_status_history WHERE line_id = $1")
            .bind(id)
            .execute(pool)
            .await
            .expect("cleanup fixture line_status_history row");
    }

    /// A minimal but valid catalogue line fixture -- same shape
    /// `lines::db_tests::test_catalogue_line` uses.
    fn test_catalogue_line(id: &str, name: &str) -> common::LineDefinition {
        common::LineDefinition {
            id: id.to_string(),
            name: name.to_string(),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: vec!["SW".to_string()],
            stations: vec![
                common::Station {
                    crs: "WOK".to_string(),
                    tiploc: None,
                    role: "major".to_string(),
                    segment: None,
                },
                common::Station {
                    crs: "CLJ".to_string(),
                    tiploc: None,
                    role: "major".to_string(),
                    segment: None,
                },
            ],
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

    /// One `LineStatus` with an active (non-`GoodService`) severity, as
    /// literal JSONB text for `seed_line_status`/`seed_line_status_history`.
    fn active_status_json() -> &'static str {
        r#"[{"severity":9,"reason":"Test disruption","validity":{"from_date":"2024-01-01T00:00:00Z","to_date":null,"is_now":true}}]"#
    }

    /// Issues a request against `router`, optionally with a session
    /// cookie, and returns `(status, parsed JSON body)`. Shared by every
    /// request helper below -- all four routes in this file return either
    /// a JSON array/object body or a plain-text `(StatusCode, String)`
    /// error body, so wrapping the latter as a JSON string lets every case
    /// share one return shape.
    async fn request(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(uri);
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    fn ids_in(body: &Value) -> Vec<String> {
        body.as_array()
            .expect("response body should be a JSON array")
            .iter()
            .filter_map(|entry| entry.get("id").and_then(Value::as_str).map(str::to_string))
            .collect()
    }

    // --- get_mode_status -----------------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_mode_status_catalogue_rows_always_show_custom_row_only_when_owned -- --ignored`"]
    async fn get_mode_status_catalogue_rows_always_show_custom_row_only_when_owned() {
        let pool = connect().await;

        let owner_token = seed_session(&pool, "TEST-MODE-STATUS-OWNER").await;
        let other_token = seed_session(&pool, "TEST-MODE-STATUS-OTHER").await;

        seed_line_status(&pool, "test-mode-status-catalogue", "national-rail", "[]").await;

        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Mode Status Custom Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-MODE-STATUS-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_status(&pool, &custom.id, "national-rail", "[]").await;

        let catalogue_line =
            test_catalogue_line("test-mode-status-catalogue", "Test Mode Status Catalogue");

        // Anonymous: catalogue row present, custom row absent.
        let anon_router = test_router(test_app(pool.clone(), vec![catalogue_line.clone()]));
        let (anon_status, anon_body) = request(
            anon_router,
            "/Line/Mode/national-rail/Status".to_string(),
            None,
        )
        .await;
        assert_eq!(anon_status, StatusCode::OK);
        let anon_ids = ids_in(&anon_body);
        assert!(anon_ids.contains(&"test-mode-status-catalogue".to_string()));
        assert!(!anon_ids.contains(&custom.id));

        // Non-owner: catalogue row present, custom row still absent.
        let other_router = test_router(test_app(pool.clone(), vec![catalogue_line.clone()]));
        let (other_status, other_body) = request(
            other_router,
            "/Line/Mode/national-rail/Status".to_string(),
            Some(&other_token),
        )
        .await;
        assert_eq!(other_status, StatusCode::OK);
        let other_ids = ids_in(&other_body);
        assert!(other_ids.contains(&"test-mode-status-catalogue".to_string()));
        assert!(!other_ids.contains(&custom.id));

        // Owner: catalogue row present, custom row present too.
        let owner_router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let (owner_status, owner_body) = request(
            owner_router,
            "/Line/Mode/national-rail/Status".to_string(),
            Some(&owner_token),
        )
        .await;
        assert_eq!(owner_status, StatusCode::OK);
        let owner_ids = ids_in(&owner_body);
        assert!(owner_ids.contains(&"test-mode-status-catalogue".to_string()));
        assert!(owner_ids.contains(&custom.id));

        cleanup_line_status(&pool, "test-mode-status-catalogue").await;
        cleanup_line_status(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-MODE-STATUS-OWNER").await;
        cleanup_user(&pool, "TEST-MODE-STATUS-OTHER").await;
    }

    // --- get_line_status --------------------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_status_drops_a_not_owned_custom_row_but_keeps_the_catalogue_row -- --ignored`"]
    async fn get_line_status_drops_a_not_owned_custom_row_but_keeps_the_catalogue_row() {
        let pool = connect().await;

        seed_session(&pool, "TEST-LINE-STATUS-OWNER").await;
        seed_line_status(&pool, "test-line-status-catalogue", "national-rail", "[]").await;
        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Line Status Not Owned".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LINE-STATUS-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_status(&pool, &custom.id, "national-rail", "[]").await;

        let catalogue_line =
            test_catalogue_line("test-line-status-catalogue", "Test Line Status Catalogue");
        let router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let uri = format!("/Line/test-line-status-catalogue,{}/Status", custom.id);
        let (status, body) = request(router, uri, None).await;

        assert_eq!(status, StatusCode::OK);
        let ids = ids_in(&body);
        assert_eq!(ids, vec!["test-line-status-catalogue".to_string()]);

        cleanup_line_status(&pool, "test-line-status-catalogue").await;
        cleanup_line_status(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-LINE-STATUS-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_status_a_lone_not_owned_custom_id_404s_like_an_unknown_id -- --ignored`"]
    async fn get_line_status_a_lone_not_owned_custom_id_404s_like_an_unknown_id() {
        let pool = connect().await;

        seed_session(&pool, "TEST-LINE-STATUS-LONE-OWNER").await;
        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Line Status Lone Not Owned".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LINE-STATUS-LONE-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_status(&pool, &custom.id, "national-rail", "[]").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let uri = format!("/Line/{}/Status", custom.id);
        let (status, body) = request(router, uri, None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        // The exact same message an unknown id already produces -- no new
        // branch, no new status code (this task's brief, Step 3).
        assert_eq!(
            body,
            Value::String(format!("no matching line(s): {}", custom.id))
        );

        cleanup_line_status(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-LINE-STATUS-LONE-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_status_the_owner_gets_their_own_custom_row -- --ignored`"]
    async fn get_line_status_the_owner_gets_their_own_custom_row() {
        let pool = connect().await;

        let owner_token = seed_session(&pool, "TEST-LINE-STATUS-REAL-OWNER").await;
        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Line Status Owned".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-LINE-STATUS-REAL-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_status(&pool, &custom.id, "national-rail", "[]").await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let uri = format!("/Line/{}/Status", custom.id);
        let (status, body) = request(router, uri, Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(ids_in(&body), vec![custom.id.clone()]);

        cleanup_line_status(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-LINE-STATUS-REAL-OWNER").await;
    }

    // --- get_line_status_history -------------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_status_history_an_owned_custom_id_returns_real_history -- --ignored`"]
    async fn get_line_status_history_an_owned_custom_id_returns_real_history() {
        let pool = connect().await;

        let owner_token = seed_session(&pool, "TEST-HISTORY-OWNER").await;
        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test History Owned".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-HISTORY-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_status_history(&pool, &custom.id, active_status_json()).await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let uri = format!(
            "/Line/{}/Status/2000-01-01T00:00:00Z/to/2100-01-01T00:00:00Z",
            custom.id
        );
        let (status, body) = request(router, uri, Some(&owner_token)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().map(Vec::len), Some(1));

        cleanup_line_status_history(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-HISTORY-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_status_history_a_not_owned_custom_id_returns_empty -- --ignored`"]
    async fn get_line_status_history_a_not_owned_custom_id_returns_empty() {
        let pool = connect().await;

        seed_session(&pool, "TEST-HISTORY-NOT-OWNER").await;
        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test History Not Owned".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-HISTORY-NOT-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_status_history(&pool, &custom.id, active_status_json()).await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let uri = format!(
            "/Line/{}/Status/2000-01-01T00:00:00Z/to/2100-01-01T00:00:00Z",
            custom.id
        );
        // No session cookie -- an anonymous caller can never own anything.
        let (status, body) = request(router, uri, None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, Value::Array(vec![]));

        cleanup_line_status_history(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-HISTORY-NOT-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_status_history_an_unknown_id_still_returns_empty_unchanged -- --ignored`"]
    async fn get_line_status_history_an_unknown_id_still_returns_empty_unchanged() {
        let pool = connect().await;

        // Regression: this route has never distinguished "unknown id" from
        // "empty history" -- confirms this task didn't change that for a
        // genuinely-unknown, non-custom-prefixed id.
        let router = test_router(test_app(pool.clone(), vec![]));
        let uri = "/Line/totally-unknown-line/Status/2000-01-01T00:00:00Z/to/2100-01-01T00:00:00Z"
            .to_string();
        let (status, body) = request(router, uri, None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, Value::Array(vec![]));
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_status_history_a_catalogue_id_is_unaffected -- --ignored`"]
    async fn get_line_status_history_a_catalogue_id_is_unaffected() {
        let pool = connect().await;

        seed_line_status_history(&pool, "test-history-catalogue", active_status_json()).await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let uri =
            "/Line/test-history-catalogue/Status/2000-01-01T00:00:00Z/to/2100-01-01T00:00:00Z"
                .to_string();
        // No session at all -- a catalogue id was never gated by ownership
        // and still isn't.
        let (status, body) = request(router, uri, None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().map(Vec::len), Some(1));

        cleanup_line_status_history(&pool, "test-history-catalogue").await;
    }

    // --- get_stop_point_disruption -----------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_stop_point_disruption_never_returns_a_custom_line -- --ignored`"]
    async fn get_stop_point_disruption_never_returns_a_custom_line() {
        let pool = connect().await;

        // No prior test in this crate covers "never returns a custom line"
        // for this route (checked before adding this one -- see this
        // task's brief). `get_stop_point_disruption` is untouched by this
        // task: its candidate line ids come from `app.config.lines` only,
        // never from `custom_lines`/`owners_for_ids` -- this proves that
        // invariant holds even when a private custom line's `line_status`
        // row shares a station (WOK) with, and has an active disruption
        // alongside, a real catalogue line.
        seed_session(&pool, "TEST-STOP-POINT-OWNER").await;
        seed_line_status(
            &pool,
            "test-stop-point-catalogue",
            "national-rail",
            active_status_json(),
        )
        .await;
        let custom = custom_lines::insert_custom_line(
            &pool,
            NewCustomLine {
                name: "Test Stop Point Custom Line".to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            "TEST-STOP-POINT-OWNER",
        )
        .await
        .expect("insert fixture custom line");
        seed_line_status(&pool, &custom.id, "national-rail", active_status_json()).await;

        let catalogue_line =
            test_catalogue_line("test-stop-point-catalogue", "Test Stop Point Catalogue");
        let router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let (status, body) = request(router, "/StopPoint/WOK/Disruption".to_string(), None).await;

        assert_eq!(status, StatusCode::OK);
        let ids = ids_in(&body);
        assert!(ids.contains(&"test-stop-point-catalogue".to_string()));
        assert!(
            !ids.contains(&custom.id),
            "a custom line must never appear in a StopPoint/Disruption response: {ids:?}"
        );

        cleanup_line_status(&pool, "test-stop-point-catalogue").await;
        cleanup_line_status(&pool, &custom.id).await;
        cleanup_user(&pool, "TEST-STOP-POINT-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_stop_point_disruption_a_station_with_no_line_coverage_404s_rather_than_looking_like_good_service -- --ignored`"]
    async fn get_stop_point_disruption_a_station_with_no_line_coverage_404s_rather_than_looking_like_good_service()
     {
        // The regression this task exists for (see
        // docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md
        // "Problem statement"): a CRS that isn't listed on any catalogue
        // line's `stations` used to come back `200 []`, byte-for-byte
        // identical to a station every one of whose covering lines is
        // running a confirmed Good Service. No catalogue line at all here
        // (an empty `lines` vec is the simplest way to guarantee zero
        // coverage for any CRS, real or not) -- this must now 404, naming
        // the CRS, mirroring `get_line_status`'s own established
        // "no matching line(s)" 404 for an analogous "nothing matched" case
        // in this same file.
        let pool = connect().await;

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = request(router, "/StopPoint/RAY/Disruption".to_string(), None).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("no line coverage for stop point: RAY".to_string())
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_stop_point_disruption_a_covered_station_with_only_good_service_still_200s_with_an_empty_array -- --ignored`"]
    async fn get_stop_point_disruption_a_covered_station_with_only_good_service_still_200s_with_an_empty_array()
     {
        // The other half of the same distinction: a station that genuinely
        // IS covered, by a line whose current statuses are all Good Service
        // (`"[]"`, the same "no active statuses" fixture shape
        // `get_mode_status_...`'s own test above uses), must keep its
        // existing `200 []` -- this task changes what "no coverage" looks
        // like, not what "covered and fine" looks like.
        let pool = connect().await;

        seed_line_status(&pool, "test-stop-point-good-service", "national-rail", "[]").await;

        let catalogue_line = test_catalogue_line(
            "test-stop-point-good-service",
            "Test Stop Point Good Service",
        );
        let router = test_router(test_app(pool.clone(), vec![catalogue_line]));
        let (status, body) = request(router, "/StopPoint/WOK/Disruption".to_string(), None).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, Value::Array(vec![]));

        cleanup_line_status(&pool, "test-stop-point-good-service").await;
    }

    // --- GET /Line/{id}/Stats/Coverage{,/HalfHourly}/{from}/to/{to} (Decision 4 scaffolding) ---

    async fn cleanup_daily_coverage_stats_row(pool: &PgPool, line_id: &str) {
        sqlx::query("DELETE FROM line_status_daily_coverage_stats WHERE line_id = $1")
            .bind(line_id)
            .execute(pool)
            .await
            .expect("cleanup fixture line_status_daily_coverage_stats row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_daily_coverage_stats_an_unknown_line_returns_an_empty_array -- --ignored`"]
    async fn get_line_daily_coverage_stats_an_unknown_line_returns_an_empty_array() {
        // Mirrors daily_stats_for_range's own "empty vec for an unknown
        // line_id, no error" contract -- this route never 404s.
        let pool = connect().await;
        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = request(
            router,
            "/Line/test-coverage-unknown-line/Stats/Coverage/2026-08-01/to/2026-08-31".to_string(),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, Value::Array(vec![]));
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_line_daily_coverage_stats_renders_a_seeded_row_with_correct_camel_case_and_rates -- --ignored`"]
    async fn get_line_daily_coverage_stats_renders_a_seeded_row_with_correct_camel_case_and_rates()
    {
        // Proves the whole round trip: a real row in
        // line_status_daily_coverage_stats, read by
        // daily_coverage_stats_for_range, rendered by
        // daily_coverage_stats_to_json, served through the real router --
        // including the one place `resolvedWindows`'s exact camelCase name
        // actually gets proven end to end, not just asserted against a
        // hand-built Value in the pure unit test above.
        const LINE_ID: &str = "TEST-COVERAGE-DAILY-ROUTE";
        let pool = connect().await;
        cleanup_daily_coverage_stats_row(&pool, LINE_ID).await;

        sqlx::query(
            "INSERT INTO line_status_daily_coverage_stats \
                (line_id, day, resolved_windows, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES ($1, '2026-08-15', 12, 100, 10, 5, 2, 95, 190.0)",
        )
        .bind(LINE_ID)
        .execute(&pool)
        .await
        .expect("seed fixture line_status_daily_coverage_stats row");

        let router = test_router(test_app(pool.clone(), vec![]));
        let (status, body) = request(
            router,
            format!("/Line/{LINE_ID}/Stats/Coverage/2026-08-01/to/2026-08-31"),
            None,
        )
        .await;

        cleanup_daily_coverage_stats_row(&pool, LINE_ID).await;

        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("response body should be an array");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row["day"], Value::String("2026-08-15".to_string()));
        assert_eq!(row["resolvedWindows"], serde_json::json!(12));
        assert_eq!(row["total"], serde_json::json!(100));
        assert_eq!(row["delayed"], serde_json::json!(10));
        assert_eq!(row["avgDelayMinutes"], serde_json::json!(2.0));
        assert_eq!(row["delayRate"], serde_json::json!(0.1));
        assert!(
            row.get("sampleCycles").is_none(),
            "must not leak the sample-stats field name"
        );
    }
}
