//! `GET /public/island-of-ireland/stations`, `/lines`: read-only listing of
//! the Iarnród Éireann (and, once built, NIR) station/line catalogue.
//! Unauthenticated, read-only, no pagination -- the whole catalogue is a
//! few hundred rows at most. See
//! docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md's
//! Judgment Call #5 for why this route exists at all (verifiability, not a
//! frontend feature -- nothing in `frontend/` consumes this).

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use common::island_of_ireland::{
    IslandOfIrelandLineDefinition, IslandOfIrelandNetwork, IslandOfIrelandStation,
};
use serde::Deserialize;

use crate::app::{App, Router};
use crate::data::island_of_ireland;

pub fn router() -> Router {
    Router::new()
        .route(
            "/island-of-ireland/stations",
            axum::routing::get(list_stations),
        )
        .route("/island-of-ireland/lines", axum::routing::get(list_lines))
}

#[derive(Debug, Deserialize)]
struct NetworkFilter {
    #[serde(default)]
    network: Option<String>,
}

fn parse_network(
    raw: &Option<String>,
) -> Result<Option<IslandOfIrelandNetwork>, (StatusCode, String)> {
    match raw.as_deref() {
        None => Ok(None),
        Some("republic-of-ireland") => Ok(Some(IslandOfIrelandNetwork::RepublicOfIreland)),
        Some("northern-ireland") => Ok(Some(IslandOfIrelandNetwork::NorthernIreland)),
        Some(other) => Err((
            StatusCode::BAD_REQUEST,
            format!("unrecognized network filter: {other}"),
        )),
    }
}

async fn list_stations(
    State(app): State<App>,
    Query(filter): Query<NetworkFilter>,
) -> Result<Json<Vec<IslandOfIrelandStation>>, (StatusCode, String)> {
    let network = parse_network(&filter.network)?;
    let stations = island_of_ireland::list_stations(&app.database, network)
        .await
        .map_err(internal_error)?;
    Ok(Json(stations))
}

async fn list_lines(
    State(app): State<App>,
    Query(filter): Query<NetworkFilter>,
) -> Result<Json<Vec<IslandOfIrelandLineDefinition>>, (StatusCode, String)> {
    let network = parse_network(&filter.network)?;
    let lines = island_of_ireland::list_lines(&app.database, network)
        .await
        .map_err(internal_error)?;
    Ok(Json(lines))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "island-of-ireland catalogue query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_network_accepts_both_known_values_and_none() {
        assert_eq!(parse_network(&None).unwrap(), None);
        assert_eq!(
            parse_network(&Some("republic-of-ireland".to_string())).unwrap(),
            Some(IslandOfIrelandNetwork::RepublicOfIreland)
        );
        assert_eq!(
            parse_network(&Some("northern-ireland".to_string())).unwrap(),
            Some(IslandOfIrelandNetwork::NorthernIreland)
        );
    }

    #[test]
    fn parse_network_rejects_unknown_values() {
        assert!(parse_network(&Some("mars".to_string())).is_err());
    }
}
