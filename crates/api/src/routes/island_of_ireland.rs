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
use axum::http::{StatusCode, header};
use common::island_of_ireland::{
    IslandOfIrelandDeparture, IslandOfIrelandLineDefinition, IslandOfIrelandNetwork,
    IslandOfIrelandStation,
};
use serde::Deserialize;

use crate::app::{App, Router};
use crate::data::island_of_ireland;

/// `Cache-Control` for the two whole-catalogue dumps below
/// (`list_stations`/`list_lines`) -- same rationale and value as
/// `routes::stanox_crs::STANOX_CRS_CACHE_CONTROL` (see that constant's doc
/// comment): a slow-changing reference catalogue served in full to
/// anonymous callers with no cache header at all is cheap bandwidth/DB
/// amplification (finding "Whole-table dumps served uncached to anonymous
/// callers"). Deliberately NOT applied to `get_station_departures` below --
/// that route is a live departure board (`island_of_ireland_station_samples`),
/// re-polled continuously, not a reference table, so caching it would show
/// stale live data instead of merely saving a cheap query.
const CATALOGUE_CACHE_CONTROL: &str = "public, max-age=3600";

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
) -> Result<
    (
        [(header::HeaderName, &'static str); 1],
        Json<Vec<IslandOfIrelandStation>>,
    ),
    (StatusCode, String),
> {
    let network = parse_network(&filter.network)?;
    let stations = island_of_ireland::list_stations(&app.database, network)
        .await
        .map_err(internal_error)?;
    Ok((
        [(header::CACHE_CONTROL, CATALOGUE_CACHE_CONTROL)],
        Json(stations),
    ))
}

async fn list_lines(
    State(app): State<App>,
    Query(filter): Query<NetworkFilter>,
) -> Result<
    (
        [(header::HeaderName, &'static str); 1],
        Json<Vec<IslandOfIrelandLineDefinition>>,
    ),
    (StatusCode, String),
> {
    let network = parse_network(&filter.network)?;
    let lines = island_of_ireland::list_lines(&app.database, network)
        .await
        .map_err(internal_error)?;
    Ok((
        [(header::CACHE_CONTROL, CATALOGUE_CACHE_CONTROL)],
        Json(lines),
    ))
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

/// HTTP-layer tests exercised against a live database -- mirrors
/// `routes::stanox_crs::db_tests`'s `test_app` helper (colocated per-file
/// rather than shared, matching this crate's established convention -- see
/// that module's own doc comment for the same reasoning stated the first
/// time this was copied).
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    fn test_app(pool: PgPool) -> App {
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
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
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

    /// Regression for "Whole-table dumps served uncached to anonymous
    /// callers": both whole-catalogue routes must carry a public,
    /// positive-max-age `Cache-Control`, independent of whether the
    /// underlying tables have any rows -- an empty catalogue is still a
    /// catalogue dump, not a per-caller answer.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                island_of_ireland_catalogue_routes_set_cache_control \
                -- --ignored --test-threads=1`"]
    async fn island_of_ireland_catalogue_routes_set_cache_control() {
        let pool = connect().await;
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));

        for uri in ["/island-of-ireland/stations", "/island-of-ireland/lines"] {
            let response = router
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "uri: {uri}");
            assert_eq!(
                response
                    .headers()
                    .get(header::CACHE_CONTROL)
                    .and_then(|v| v.to_str().ok()),
                Some(CATALOGUE_CACHE_CONTROL),
                "uri {uri} must carry a public, positive-max-age Cache-Control"
            );
        }
    }
}
