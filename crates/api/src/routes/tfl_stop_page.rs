//! `GET /public/tfl/stops/{id}` and `GET /public/tfl/stops/{id}/arrivals`:
//! one `TfL` Tube, DLR or tram stop and its live arrivals, for the
//! `/tfl/stops/{id}` page (DS-136). Unauthenticated and read-only, like the
//! stop search beside them (`tfl_stops.rs`). The arrivals come from `TfL`
//! on demand through a short per-stop cache (`data::tfl_arrivals`) and share
//! the `/Train/by-uid/*` rate limit (`crate::rate_limit::classify`).

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use common::tfl_stops::{TflStopLine, is_stop_id};

use crate::app::{App, Router};
use crate::data::tfl_arrivals::{self, StopArrivals};
use crate::data::tfl_stops;

pub fn router() -> Router {
    Router::new()
        .route("/tfl/stops/{id}", axum::routing::get(get_stop))
        .route("/tfl/stops/{id}/arrivals", axum::routing::get(get_arrivals))
}

/// A National Rail station at the stop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RailStation {
    pub crs: String,
    /// `null` when the station has no reference row.
    pub name: Option<String>,
}

/// `GET /public/tfl/stops/{id}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TflStopDetail {
    pub id: String,
    pub name: String,
    pub modes: Vec<String>,
    pub lines: Vec<TflStopLine>,
    /// The National Rail stations at this stop, best first
    /// (`tfl_stops::rail_crs_for_stops`: the termini overrides, the hub
    /// crosswalk, then the nearest station within 300 m). Often empty.
    pub rail_stations: Vec<RailStation>,
}

/// The 404 every unknown or malformed id gets.
fn not_found(id: &str) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("no TfL stop {id}"))
}

/// `GET /public/tfl/stops/{id}`: the stop, its lines and its National Rail
/// stations. 404 for an id that is not a stop in `tfl_stops`.
async fn get_stop(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<TflStopDetail>, (StatusCode, String)> {
    if !is_stop_id(&id) {
        return Err(not_found(&id));
    }
    let ids = [id.clone()];
    let mut stops = tfl_stops::stops_by_id(&app.database, &ids)
        .await
        .map_err(|err| internal_error(&err))?;
    let Some(stop) = stops.remove(&id) else {
        return Err(not_found(&id));
    };
    let codes = tfl_stops::rail_crs_for_stops(&app.database, &ids)
        .await
        .map_err(|err| internal_error(&err))?
        .remove(&id)
        .unwrap_or_default();
    let names = crate::data::queries::station_names_for_crs_batch(&app.database, &codes)
        .await
        .map_err(|err| internal_error(&err))?;
    let rail_stations = codes
        .into_iter()
        .map(|crs| RailStation {
            name: names.get(&crs).cloned(),
            crs,
        })
        .collect();
    Ok(Json(TflStopDetail {
        id: stop.id,
        name: stop.name,
        modes: stop.modes,
        lines: stop.lines,
        rail_stations,
    }))
}

/// The body of an arrivals 503: the shape of every other 503 this api
/// answers (`crate::unavailable`), with its own `error` code, and a
/// message fit to show a traveller.
#[derive(Debug, Serialize)]
struct TflUnavailable {
    error: &'static str,
    retryable: bool,
    message: &'static str,
}

pub const TFL_UNAVAILABLE_MESSAGE: &str =
    "Live arrivals from TfL aren't available right now. Please try again in a minute.";

fn tfl_unavailable() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(TflUnavailable {
            error: "tfl_unavailable",
            retryable: true,
            message: TFL_UNAVAILABLE_MESSAGE,
        }),
    )
        .into_response()
}

/// `GET /public/tfl/stops/{id}/arrivals`: `TfL`'s live arrivals at the
/// stop, grouped by line and platform (`tfl_arrivals::group_arrivals`).
/// 404 for an id that is not a stop in `tfl_stops` (checked before `TfL` is
/// asked, so only real stops are fetched or cached); 503 with
/// `{"error":"tfl_unavailable",...}` when `TfL` can't be had (down, slow,
/// rate limiting us, or arrivals turned off).
async fn get_arrivals(
    State(app): State<App>,
    Path(id): Path<String>,
) -> Result<Json<StopArrivals>, Response> {
    if !is_stop_id(&id) {
        return Err(not_found(&id).into_response());
    }
    let known = tfl_stops::stops_by_id(&app.database, std::slice::from_ref(&id))
        .await
        .map_err(|err| internal_error(&err).into_response())?;
    if !known.contains_key(&id) {
        return Err(not_found(&id).into_response());
    }
    let snapshot = app
        .tfl_arrivals
        .get(&id)
        .await
        .map_err(|_| tfl_unavailable())?;
    Ok(Json(tfl_arrivals::group_arrivals(
        &id,
        &snapshot,
        chrono::Utc::now(),
    )))
}

fn internal_error(err: &anyhow::Error) -> (StatusCode, String) {
    if let Some(unavailable) = crate::unavailable::response_for(err) {
        return unavailable;
    }
    tracing::error!(error = ?err, "TfL stop read failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn router_builds_without_a_route_conflict() {
        let _router: crate::app::Router = crate::routes::public_router();
    }
}

#[cfg(test)]
mod db_tests {
    use std::sync::Arc;
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use sqlx::PgPool;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;
    use crate::data::tfl_arrivals::tests::{FakeSource, RUSSELL_SQUARE};
    use crate::data::tfl_arrivals::{FetchError, TflArrivals, TflPrediction, parse_predictions};

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    const STOPS: &str = "'940GZZPGTSA', '940GZZPGTSB'";

    async fn cleanup(pool: &PgPool) {
        for sql in [
            format!("DELETE FROM tfl_stop_crs WHERE stop_id IN ({STOPS})"),
            format!("DELETE FROM tfl_stops WHERE stop_id IN ({STOPS})"),
            "DELETE FROM stations WHERE crs IN ('Z9P')".to_owned(),
        ] {
            sqlx::query(&sql).execute(pool).await.unwrap();
        }
    }

    /// A Tube stop hub-linked to station Z9P, and a tram stop with no
    /// station anywhere near.
    async fn seed(pool: &PgPool) {
        cleanup(pool).await;
        for sql in [
            "INSERT INTO tfl_stops (stop_id, name, search_name, modes, lines, latitude, longitude, fetched_at) \
             VALUES ('940GZZPGTSA', 'Pagetest Square', 'pagetest square', '{tube}', \
             '[{\"id\": \"piccadilly\", \"name\": \"Piccadilly\"}]', 52.9, 1.9, NOW()), \
             ('940GZZPGTSB', 'Pagetest Tram', 'pagetest tram', '{tram}', \
             '[{\"id\": \"tram\", \"name\": \"Tram\"}]', 52.1, 1.1, NOW())",
            "INSERT INTO stations (crs, name, latitude, longitude) VALUES ('Z9P', 'Pagetest Central', 52.95, 1.95)",
            "INSERT INTO tfl_stop_crs (stop_id, crs, source) VALUES ('940GZZPGTSA', 'Z9P', 'hub')",
        ] {
            sqlx::query(sql)
                .execute(pool)
                .await
                .unwrap_or_else(|err| panic!("{sql}: {err}"));
        }
    }

    fn app_with(pool: PgPool, arrivals: TflArrivals) -> App {
        let app = crate::test_support::inert_app(pool);
        let mut state = Arc::try_unwrap(app).expect("a fresh app has one owner");
        state.tfl_arrivals = Arc::new(arrivals);
        Arc::new(state)
    }

    async fn get(app: App, uri: &str) -> (StatusCode, Value) {
        let router: axum::Router = Router::new().merge(router()).with_state(app);
        let response = router
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()));
        (status, body)
    }

    /// The fixture's predictions moved to a minute and two from now.
    fn upcoming() -> Vec<TflPrediction> {
        let now = chrono::Utc::now();
        parse_predictions(RUSSELL_SQUARE.as_bytes())
            .unwrap()
            .into_iter()
            .take(2)
            .enumerate()
            .map(|(i, mut prediction)| {
                prediction.expected_arrival =
                    now + chrono::Duration::seconds(60 * (i64::try_from(i).unwrap() + 1));
                prediction
            })
            .collect()
    }

    #[tokio::test]
    #[ignore = "requires a live database (DATABASE_URL)"]
    async fn a_stop_has_its_lines_and_rail_station() {
        let pool = connect().await;
        seed(&pool).await;
        let app = app_with(pool.clone(), TflArrivals::disabled());

        let (status, body) = get(app.clone(), "/tfl/stops/940GZZPGTSA").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["name"], "Pagetest Square");
        assert_eq!(body["modes"], serde_json::json!(["tube"]));
        assert_eq!(body["lines"][0]["id"], "piccadilly");
        assert_eq!(
            body["railStations"],
            serde_json::json!([{"crs": "Z9P", "name": "Pagetest Central"}])
        );

        let (status, body) = get(app.clone(), "/tfl/stops/940GZZPGTSB").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["railStations"], serde_json::json!([]));

        for missing in ["/tfl/stops/940GZZNOPE00", "/tfl/stops/not-an-id"] {
            let (status, _) = get(app.clone(), missing).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{missing}");
        }
        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database (DATABASE_URL)"]
    async fn arrivals_come_grouped_and_cached() {
        let pool = connect().await;
        seed(&pool).await;
        let source = FakeSource::new(Ok(upcoming()));
        let app = app_with(
            pool.clone(),
            TflArrivals::new(
                source.clone(),
                Duration::from_secs(30),
                Duration::from_secs(10),
            ),
        );

        let (status, body) = get(app.clone(), "/tfl/stops/940GZZPGTSA/arrivals").await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["stopId"], "940GZZPGTSA");
        assert!(body["fetchedAt"].is_string());
        let groups = body["groups"].as_array().unwrap();
        assert_eq!(groups.len(), 2, "{body}");
        assert_eq!(groups[0]["platformName"], "Eastbound - Platform 1");
        assert_eq!(groups[0]["lineId"], "piccadilly");
        assert_eq!(groups[0]["arrivals"][0]["destination"], "Cockfosters");
        let secs = groups[0]["arrivals"][0]["timeToStationSecs"]
            .as_i64()
            .unwrap();
        assert!((100..=120).contains(&secs), "{secs}");
        assert_eq!(
            groups[1]["arrivals"][0]["destination"],
            "Heathrow Terminal 5"
        );

        let (status, _) = get(app.clone(), "/tfl/stops/940GZZPGTSA/arrivals").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            source.calls(),
            1,
            "the second request is served from the cache"
        );

        // An unknown stop is a 404 and never reaches TfL.
        let (status, _) = get(app.clone(), "/tfl/stops/940GZZNOPE00/arrivals").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = get(app.clone(), "/tfl/stops/nope/arrivals").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(source.calls(), 1);
        cleanup(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database (DATABASE_URL)"]
    async fn a_tfl_outage_is_a_friendly_503() {
        let pool = connect().await;
        seed(&pool).await;
        for arrivals in [
            TflArrivals::new(
                FakeSource::new(Err(FetchError::Status(502))),
                Duration::from_secs(30),
                Duration::from_secs(10),
            ),
            TflArrivals::new(
                FakeSource::new(Err(FetchError::Transport("timed out".to_owned()))),
                Duration::from_secs(30),
                Duration::from_secs(10),
            ),
            TflArrivals::disabled(),
        ] {
            let app = app_with(pool.clone(), arrivals);
            let (status, body) = get(app, "/tfl/stops/940GZZPGTSB/arrivals").await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
            assert_eq!(body["error"], "tfl_unavailable");
            assert_eq!(body["retryable"], true);
            assert_eq!(body["message"], TFL_UNAVAILABLE_MESSAGE);
        }
        cleanup(&pool).await;
    }
}
