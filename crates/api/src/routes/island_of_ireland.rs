//! `GET /public/island-of-ireland/stations`, `/lines`: read-only listing of
//! the Iarnród Éireann (and, once built, NIR) station/line catalogue.
//! `GET /public/island-of-ireland/stations/{id}/departures`: raw
//! pass-through of the live `island_of_ireland_station_samples` board
//! (Tier B). Unauthenticated, read-only, no pagination -- the whole
//! catalogue is a few hundred rows at most. See
//! docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md's
//! Judgment Call #5 for why these routes exist at all (verifiability, not a
//! frontend feature -- nothing in `frontend/` consumes this).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use common::island_of_ireland::{
    IslandOfIrelandDeparture, IslandOfIrelandLineDefinition, IslandOfIrelandNetwork,
    IslandOfIrelandStation,
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
        .route(
            "/island-of-ireland/stations/{id}/departures",
            axum::routing::get(get_station_departures),
        )
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

/// 404 when `island_of_ireland_station_samples` has no row for `id` at
/// all -- identical honesty split to
/// `routes::departures::get_station_departures`. `200 []` is the same
/// "row exists, board is genuinely empty right now" fact that route
/// already draws.
async fn get_station_departures(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, String)> {
    let Some(sample) = island_of_ireland::latest_station_sample(&app.database, &id)
        .await
        .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("no sample data collected for island-of-ireland station: {id}"),
        ));
    };

    Ok(Json(sample.departures.iter().map(departure_json).collect()))
}

/// Hand-built camelCase JSON, matching `render::station_departure_json`'s
/// established convention for a public departures endpoint even though
/// this route's own producer (Task B4) writes snake_case internally.
fn departure_json(d: &IslandOfIrelandDeparture) -> serde_json::Value {
    serde_json::json!({
        "trainCode": d.train_code,
        "origin": d.origin,
        "destination": d.destination,
        "scheduledArrival": d.scheduled_arrival,
        "scheduledDeparture": d.scheduled_departure,
        "expectedArrival": d.expected_arrival,
        "expectedDeparture": d.expected_departure,
        "lateMinutes": d.late_minutes,
        "status": d.status,
        "dueInMinutes": d.due_in_minutes,
    })
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

    #[test]
    fn departure_json_maps_every_field_to_camel_case() {
        let departure = IslandOfIrelandDeparture {
            train_code: "A101".to_string(),
            origin: "Belfast".to_string(),
            destination: "Dublin Connolly".to_string(),
            scheduled_arrival: None,
            scheduled_departure: Some("06:00".to_string()),
            expected_arrival: None,
            expected_departure: Some("06:00".to_string()),
            late_minutes: 0,
            status: "On Time".to_string(),
            due_in_minutes: Some(5),
        };
        let json = departure_json(&departure);
        assert_eq!(
            json,
            serde_json::json!({
                "trainCode": "A101",
                "origin": "Belfast",
                "destination": "Dublin Connolly",
                "scheduledArrival": null,
                "scheduledDeparture": "06:00",
                "expectedArrival": null,
                "expectedDeparture": "06:00",
                "lateMinutes": 0,
                "status": "On Time",
                "dueInMinutes": 5,
            })
        );
    }
}
