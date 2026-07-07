//! The four TfL-shaped read endpoints: `/Line/Mode/{mode}/Status`,
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

async fn get_mode_status(
    State(app): State<App>,
    Path(mode): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    if mode != "national-rail" {
        return Err((StatusCode::BAD_REQUEST, format!("unsupported mode: {mode}")));
    }

    let rows = queries::line_status_for_mode(&app.database, &mode)
        .await
        .map_err(internal_error)?;

    Ok(Json(
        rows.into_iter().map(to_report).map(|r| to_tfl_shape(&r, query.detail)).collect(),
    ))
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

    Ok(Json(
        rows.into_iter().map(to_report).map(|r| to_tfl_shape(&r, query.detail)).collect(),
    ))
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
                Some(to_tfl_shape(&report, true))
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
                let mut json = to_tfl_shape(&report, true);
                json["computedAt"] = Value::String(computed_at.to_rfc3339());
                json
            })
            .collect(),
    ))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "line status query failed");
    (StatusCode::INTERNAL_SERVER_ERROR, "query failed".to_string())
}
