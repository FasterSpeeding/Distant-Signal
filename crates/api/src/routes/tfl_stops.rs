//! `/public/tfl/stops`, `/public/tfl/stops/by-crs` and
//! `/public/tfl/stops/rail-crs`: `TfL`'s Tube, DLR and tram stops (docs/superpowers/specs/2026-10-20-tfl-stop-lookup-design.md
//! §3). Unauthenticated and read-only, like `/stations` (`reference.rs`):
//! the stop list is public data. Empty until poller-tfl's stops job has run
//! (`pollers.tfl.stops.enabled`).

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use common::tfl_stops::is_stop_id;

use crate::app::{App, Router};
use crate::data::tfl_stops::{self, TflStopSummary};

/// Rows a type-ahead search returns at most, as `/stations`.
const SUGGESTION_LIMIT: i64 = 20;

/// CRS codes one `/tfl/stops/by-crs` request may ask about, and stop ids
/// one `/tfl/stops/rail-crs` request may.
const MAX_CRS: usize = 20;

pub fn router() -> Router {
    Router::new()
        .route("/tfl/stops", axum::routing::get(search_stops))
        .route("/tfl/stops/by-crs", axum::routing::get(stops_by_crs))
        .route("/tfl/stops/rail-crs", axum::routing::get(rail_crs_by_stop))
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
    /// `tube,dlr,tram` (default: all three).
    modes: Option<String>,
}

/// `GET /public/tfl/stops?q=&modes=`: up to 20 stops, best match first
/// (`tfl_stops::search`). A blank `q` is `[]`; an unknown mode a 400.
async fn search_stops(
    State(app): State<App>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<TflStopSummary>>, (StatusCode, String)> {
    let modes = tfl_stops::parse_modes(query.modes.as_deref())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    let q = query.q.trim();
    if q.is_empty() {
        return Ok(Json(Vec::new()));
    }
    // A type-ahead query, not a document: anything longer is a typo or a probe.
    let q: String = q.chars().take(100).collect();
    tfl_stops::search(&app.database, &q, &modes, SUGGESTION_LIMIT)
        .await
        .map(Json)
        .map_err(|err| internal_error(&err))
}

#[derive(Debug, Deserialize)]
struct ByCrsQuery {
    /// One or more CRS codes, comma-separated (`EUS,KGX`).
    crs: Option<String>,
    modes: Option<String>,
}

/// `GET /public/tfl/stops/by-crs?crs=EUS[,KGX][&modes=]`: each National
/// Rail station's `TfL` stops, best first, by CRS (`tfl_stops::stops_for_crs`:
/// overrides, the hub crosswalk, then the nearest stop within 300 m). A code
/// with no stop is absent. 400 without `crs`, with a code that is not three
/// letters, or with more than 20.
async fn stops_by_crs(
    State(app): State<App>,
    Query(query): Query<ByCrsQuery>,
) -> Result<Json<BTreeMap<String, Vec<TflStopSummary>>>, (StatusCode, String)> {
    let modes = tfl_stops::parse_modes(query.modes.as_deref())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    let codes = parse_crs_list(query.crs.as_deref())?;
    tfl_stops::stops_for_crs(&app.database, &codes, &modes)
        .await
        .map(Json)
        .map_err(|err| internal_error(&err))
}

#[derive(Debug, Deserialize)]
struct RailCrsQuery {
    /// One or more stop ids, comma-separated (`940GZZLUEUS,940GZZLUVIC`).
    ids: Option<String>,
}

/// `GET /public/tfl/stops/rail-crs?ids=940GZZLUEUS[,...]`: each stop's
/// National Rail stations by CRS, best first
/// (`tfl_stops::rail_crs_for_stops`: overrides, the hub crosswalk, then the
/// nearest station within 300 m), so a train leg can follow a Tube leg from
/// the station the Tube leg ends at. A stop with no station is absent. 400
/// without `ids`, with an id that is not a stop id, or with more than 20.
async fn rail_crs_by_stop(
    State(app): State<App>,
    Query(query): Query<RailCrsQuery>,
) -> Result<Json<BTreeMap<String, Vec<String>>>, (StatusCode, String)> {
    let ids = parse_stop_id_list(query.ids.as_deref())?;
    tfl_stops::rail_crs_for_stops(&app.database, &ids)
        .await
        .map(Json)
        .map_err(|err| internal_error(&err))
}

fn parse_stop_id_list(raw: Option<&str>) -> Result<Vec<String>, (StatusCode, String)> {
    let bad = || {
        (
            StatusCode::BAD_REQUEST,
            format!("ids must be up to {MAX_CRS} comma-separated TfL stop ids."),
        )
    };
    let raw = raw
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .ok_or_else(bad)?;
    let ids: Vec<String> = raw.split(',').map(|id| id.trim().to_owned()).collect();
    if ids.len() > MAX_CRS || !ids.iter().all(|id| is_stop_id(id)) {
        return Err(bad());
    }
    Ok(ids)
}

fn parse_crs_list(raw: Option<&str>) -> Result<Vec<String>, (StatusCode, String)> {
    let bad = || {
        (
            StatusCode::BAD_REQUEST,
            format!("crs must be up to {MAX_CRS} comma-separated three-letter station codes."),
        )
    };
    let raw = raw
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .ok_or_else(bad)?;
    let codes: Vec<String> = raw
        .split(',')
        .map(|code| code.trim().to_ascii_uppercase())
        .collect();
    if codes.len() > MAX_CRS
        || codes
            .iter()
            .any(|code| code.len() != 3 || !code.bytes().all(|b| b.is_ascii_alphanumeric()))
    {
        return Err(bad());
    }
    Ok(codes)
}

fn internal_error(err: &anyhow::Error) -> (StatusCode, String) {
    if let Some(unavailable) = crate::unavailable::response_for(err) {
        return unavailable;
    }
    tracing::error!(error = ?err, "TfL stops read failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crs_lists_are_three_letter_codes() {
        assert_eq!(parse_crs_list(Some(" eus, KGX ")).unwrap(), ["EUS", "KGX"]);
        assert!(parse_crs_list(None).is_err());
        assert!(parse_crs_list(Some("")).is_err());
        assert!(parse_crs_list(Some("EUST")).is_err());
        assert!(parse_crs_list(Some("EU%")).is_err());
        let many = vec!["EUS"; MAX_CRS + 1].join(",");
        assert!(parse_crs_list(Some(&many)).is_err());
    }

    #[test]
    fn stop_id_lists_are_naptan_ids() {
        assert_eq!(
            parse_stop_id_list(Some(" 940GZZLUEUS,940GZZLUVIC ")).unwrap(),
            ["940GZZLUEUS", "940GZZLUVIC"]
        );
        assert!(parse_stop_id_list(None).is_err());
        assert!(parse_stop_id_list(Some(" ")).is_err());
        assert!(parse_stop_id_list(Some("940gzzlueus")).is_err());
        assert!(parse_stop_id_list(Some("940GZZLUEUS,")).is_err());
        assert!(parse_stop_id_list(Some("940GZZ%UEUS")).is_err());
        let many = vec!["940GZZLUEUS"; MAX_CRS + 1].join(",");
        assert!(parse_stop_id_list(Some(&many)).is_err());
    }

    #[test]
    fn router_builds_without_a_route_conflict() {
        let _router: Router = crate::routes::public_router();
    }
}
