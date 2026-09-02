//! `/public/stations`, `/public/tocs`: type-ahead search over reference
//! data. Unauthenticated, read-only — same `public_router()` pattern as
//! `lines.rs`. See
//! docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::app::{App, Router};
use crate::data::reference::{self, Suggestion};

/// Caps how many rows a single type-ahead request can return. 20 is
/// plenty for a dropdown the user is actively narrowing by typing more.
const SUGGESTION_LIMIT: i64 = 20;

pub fn router() -> Router {
    Router::new()
        .route("/stations", axum::routing::get(search_stations))
        .route("/tocs", axum::routing::get(search_tocs))
        .route("/tocs/all", axum::routing::get(list_all_tocs))
}

#[derive(Debug, Deserialize)]
struct SearchQuery {
    #[serde(default)]
    q: String,
}

async fn search_stations(
    State(app): State<App>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Suggestion>>, (StatusCode, String)> {
    let Some(q) = sanitize_query(&query.q) else {
        return Ok(Json(Vec::new()));
    };
    let results = reference::search_stations(&app.database, q, SUGGESTION_LIMIT)
        .await
        .map_err(internal_error)?;
    Ok(Json(results))
}

async fn search_tocs(
    State(app): State<App>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Suggestion>>, (StatusCode, String)> {
    let Some(q) = sanitize_query(&query.q) else {
        return Ok(Json(Vec::new()));
    };
    let results = reference::search_tocs(&app.database, q, SUGGESTION_LIMIT)
        .await
        .map_err(internal_error)?;
    Ok(Json(results))
}

async fn list_all_tocs(
    State(app): State<App>,
) -> Result<Json<Vec<Suggestion>>, (StatusCode, String)> {
    let results = reference::get_all_tocs(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(results))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "reference search failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

/// Trims `raw`; returns `None` if the result is empty. Used to skip
/// querying the DB entirely for a type-ahead request with no search text
/// yet (e.g. the field was just focused, or the user cleared it).
fn sanitize_query(raw: &str) -> Option<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_query_trims_whitespace() {
        assert_eq!(sanitize_query("  wok  "), Some("wok"));
    }

    #[test]
    fn sanitize_query_rejects_empty_or_whitespace_only() {
        assert_eq!(sanitize_query(""), None);
        assert_eq!(sanitize_query("   "), None);
    }

    #[test]
    fn sanitize_query_passes_through_non_whitespace_unchanged() {
        assert_eq!(sanitize_query("SW"), Some("SW"));
    }
}
