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
use crate::data::queries;
use crate::render::to_tfl_shape;

pub fn router() -> Router {
    AxumRouter::new()
        .route("/Line/Mode/{mode}/Status", axum::routing::get(get_mode_status))
        .route("/Line/{ids}/Status", axum::routing::get(get_line_status))
        .route("/StopPoint/{crs}/Disruption", axum::routing::get(get_stop_point_disruption))
        .route("/Line/{id}/Status/{from}/to/{to}", axum::routing::get(get_line_status_history))
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

fn rows_to_json(rows: Vec<queries::LineStatusRow>, detail: bool) -> Vec<Value> {
    rows.into_iter()
        .map(|row| {
            let computed_at = row.computed_at;
            let report = to_report(row);
            to_tfl_shape(&report, computed_at, detail)
        })
        .collect()
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
    if let Some(unsupported) = modes.iter().find(|mode| !SUPPORTED_MODES.contains(&mode.as_str())) {
        return Err(format!("unsupported mode: {unsupported}"));
    }
    Ok(modes)
}

async fn get_mode_status(
    State(app): State<App>,
    Path(modes): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let modes = parse_modes(&modes).map_err(|message| (StatusCode::BAD_REQUEST, message))?;

    let rows = queries::line_status_for_modes(&app.database, &modes)
        .await
        .map_err(internal_error)?;

    Ok(Json(rows_to_json(rows, query.detail)))
}

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
        return Ok(Json(vec![]));
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
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
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

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "line status query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_mode_still_works() {
        assert_eq!(parse_modes("national-rail").unwrap(), vec!["national-rail".to_string()]);
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
        assert_eq!(parse_modes("tube, dlr,").unwrap(), vec!["tube".to_string(), "dlr".to_string()]);
    }

    #[test]
    fn an_unsupported_mode_is_named_in_the_error() {
        // `bus` and `river-bus` are real TfL modes this app deliberately
        // does not ingest, so "no results" would be a misleading answer.
        let err = parse_modes("tube,bus").unwrap_err();
        assert!(err.contains("bus"), "error should name the offending mode: {err}");
    }

    #[test]
    fn an_empty_mode_list_is_rejected_rather_than_matching_everything() {
        assert!(parse_modes("").is_err());
        assert!(parse_modes(",,").is_err());
    }
}
