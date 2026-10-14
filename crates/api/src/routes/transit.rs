//! `/public/transit/...`: transit networks that are neither National Rail
//! nor `TfL`, Manchester Metrolink first (docs/superpowers/specs/2026-10-09-
//! transit-networks-design.md §11). Read-only and unauthenticated, from the
//! network's active timetable version (`ds_store::transit::read`).
//!
//! **Dark by default.** A network is served only while it is listed in
//! `API_TRANSIT_NETWORKS` (chart `api.transitNetworks`, empty by default);
//! otherwise every route for it answers 404, and `/transit/networks` is an
//! empty list, which the frontend reads to hide its pages.
//!
//! **Scheduled, never live.** A board carries `"realtime": false` and
//! `"kind": "scheduled"`, and its rows have no expected time, delay,
//! cancellation or status field (spec §2, C6): a timetable says nothing
//! about what ran.
//!
//! Path segments: `{network}` and `{line}` are local ids (`metrolink`,
//! `blue`), `{stop}` a station's ATCO code (`9400ZZMAPIC`). Bodies carry the
//! public line id (`metrolink:blue`).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use chrono::{NaiveDate, Timelike};
use common::transit::{is_id, is_stop_id};
use ds_store::transit::read;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};

/// `/transit/networks`.
const NETWORKS_CACHE: &str = "public, max-age=300";
/// A network (its lines), a board.
const SHORT_CACHE: &str = "public, max-age=60";
/// A line, a stop, a timetable.
const MEDIUM_CACHE: &str = "public, max-age=300";
/// The stop list.
const LONG_CACHE: &str = "public, max-age=3600";

/// A board's default and largest page.
const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 100;
/// The line timetable's default window.
const DEFAULT_WINDOW_S: i32 = 3 * 3600;

type Reply = Result<([(header::HeaderName, &'static str); 1], Json<Value>), (StatusCode, String)>;

pub fn router() -> Router {
    Router::new()
        .route("/transit/networks", axum::routing::get(list_networks))
        .route("/transit/{network}", axum::routing::get(get_network))
        .route(
            "/transit/{network}/lines/{line}",
            axum::routing::get(get_line),
        )
        .route(
            "/transit/{network}/lines/{line}/timetable",
            axum::routing::get(get_line_timetable),
        )
        .route("/transit/{network}/stops", axum::routing::get(list_stops))
        .route(
            "/transit/{network}/stops/{stop}",
            axum::routing::get(get_stop),
        )
        .route(
            "/transit/{network}/stops/{stop}/departures",
            axum::routing::get(get_departures),
        )
}

/// The networks this api serves (`API_TRANSIT_NETWORKS`, empties dropped).
fn enabled(app: &App) -> Vec<String> {
    app.config
        .transit_networks
        .iter()
        .map(|n| n.trim().to_owned())
        .filter(|n| !n.is_empty())
        .collect()
}

fn not_found(what: &str) -> (StatusCode, String) {
    (StatusCode::NOT_FOUND, format!("no such {what}"))
}

/// 404 unless `network` is served.
fn check_network(app: &App, network: &str) -> Result<(), (StatusCode, String)> {
    if is_id(network) && enabled(app).iter().any(|n| n == network) {
        Ok(())
    } else {
        Err(not_found("transit network"))
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used as a map_err callback, which passes the error by value"
)]
fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    if let Some(unavailable) = crate::unavailable::response_for(&err) {
        return unavailable;
    }
    tracing::error!(error = ?err, "transit query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "every handler returns it as its Reply"
)]
fn reply(cache: &'static str, body: Value) -> Reply {
    Ok(([(header::CACHE_CONTROL, cache)], Json(body)))
}

fn to_json<T: serde::Serialize>(value: &T) -> Result<Value, (StatusCode, String)> {
    serde_json::to_value(value).map_err(|err| internal_error(err.into()))
}

async fn list_networks(State(app): State<App>) -> Reply {
    let networks = read::list_networks(&app.database, &enabled(&app))
        .await
        .map_err(internal_error)?;
    reply(NETWORKS_CACHE, json!({ "networks": to_json(&networks)? }))
}

async fn get_network(State(app): State<App>, Path(network): Path<String>) -> Reply {
    check_network(&app, &network)?;
    let Some(summary) = read::get_network(&app.database, &network)
        .await
        .map_err(internal_error)?
    else {
        return Err(not_found("transit network"));
    };
    let lines = read::list_lines(&app.database, &network)
        .await
        .map_err(internal_error)?;
    let mut body = to_json(&summary)?;
    body["lines"] = to_json(&lines)?;
    reply(SHORT_CACHE, body)
}

async fn get_line(State(app): State<App>, Path((network, line)): Path<(String, String)>) -> Reply {
    check_network(&app, &network)?;
    if !is_id(&line) {
        return Err(not_found("line"));
    }
    let Some(detail) = read::get_line(&app.database, &network, &line)
        .await
        .map_err(internal_error)?
    else {
        return Err(not_found("line"));
    };
    reply(MEDIUM_CACHE, to_json(&detail)?)
}

#[derive(Debug, Deserialize)]
struct TimetableQuery {
    date: Option<NaiveDate>,
    direction: Option<String>,
    /// `HH:MM`.
    from: Option<String>,
    to: Option<String>,
}

/// `HH:MM` (to 47:59, a timetable's day) as seconds after midnight.
fn parse_hhmm(text: &str) -> Option<i32> {
    let (h, m) = text.split_once(':')?;
    if !(1..=2).contains(&h.len()) || m.len() != 2 {
        return None;
    }
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    ((0..48).contains(&h) && (0..60).contains(&m)).then_some(h * 3600 + m * 60)
}

fn bad_request(what: &str) -> (StatusCode, String) {
    (StatusCode::BAD_REQUEST, what.to_owned())
}

async fn get_line_timetable(
    State(app): State<App>,
    Path((network, line)): Path<(String, String)>,
    Query(query): Query<TimetableQuery>,
) -> Reply {
    check_network(&app, &network)?;
    if !is_id(&line) {
        return Err(not_found("line"));
    }
    let now = super::london_now();
    let date = query.date.unwrap_or_else(|| now.date_naive());
    let direction = query.direction.unwrap_or_else(|| "outbound".to_owned());
    if !["outbound", "inbound"].contains(&direction.as_str()) {
        return Err(bad_request("direction must be outbound or inbound"));
    }
    let now_s = i32::try_from(now.num_seconds_from_midnight()).unwrap_or(0);
    let from_s = match query.from.as_deref() {
        Some(text) => parse_hhmm(text).ok_or_else(|| bad_request("from must be HH:MM"))?,
        // An hour ago, on the hour.
        None => (now_s - 3600).max(0) / 3600 * 3600,
    };
    let to_s = match query.to.as_deref() {
        Some(text) => parse_hhmm(text).ok_or_else(|| bad_request("to must be HH:MM"))?,
        None => from_s + DEFAULT_WINDOW_S,
    };
    if to_s <= from_s || to_s - from_s > 24 * 3600 {
        return Err(bad_request("to must be after from, within a day"));
    }
    let Some(grid) = read::line_timetable(
        &app.database,
        &network,
        &line,
        date,
        &direction,
        from_s,
        to_s,
    )
    .await
    .map_err(internal_error)?
    else {
        return Err(not_found("line"));
    };
    let mut body = to_json(&grid)?;
    body["realtime"] = json!(false);
    body["kind"] = json!("scheduled");
    reply(MEDIUM_CACHE, body)
}

async fn list_stops(State(app): State<App>, Path(network): Path<String>) -> Reply {
    check_network(&app, &network)?;
    let stops = read::list_stops(&app.database, &network)
        .await
        .map_err(internal_error)?;
    reply(LONG_CACHE, json!({ "stops": to_json(&stops)? }))
}

async fn get_stop(State(app): State<App>, Path((network, stop)): Path<(String, String)>) -> Reply {
    check_network(&app, &network)?;
    if !is_stop_id(&stop) {
        return Err(not_found("stop"));
    }
    let Some(detail) = read::get_stop(&app.database, &network, &stop)
        .await
        .map_err(internal_error)?
    else {
        return Err(not_found("stop"));
    };
    reply(MEDIUM_CACHE, to_json(&detail)?)
}

#[derive(Debug, Deserialize)]
struct DeparturesQuery {
    date: Option<NaiveDate>,
    /// `HH:MM`; default now (today) or midnight (another date).
    after: Option<String>,
    limit: Option<i64>,
    cursor: Option<String>,
}

async fn get_departures(
    State(app): State<App>,
    Path((network, stop)): Path<(String, String)>,
    Query(query): Query<DeparturesQuery>,
) -> Reply {
    check_network(&app, &network)?;
    if !is_stop_id(&stop) {
        return Err(not_found("stop"));
    }
    let now = super::london_now();
    let today = now.date_naive();
    let date = query.date.unwrap_or(today);
    let after_s = match query.after.as_deref() {
        Some(text) => parse_hhmm(text)
            .filter(|s| *s < 24 * 3600)
            .ok_or_else(|| bad_request("after must be HH:MM"))?,
        None if date == today => {
            i32::try_from(now.num_seconds_from_midnight()).unwrap_or(0) / 60 * 60
        }
        None => 0,
    };
    let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    if query.cursor.as_deref().is_some_and(|c| c.len() > 256) {
        return Err(bad_request("cursor too long"));
    }
    let Some(board) = read::stop_departures(
        &app.database,
        &network,
        &stop,
        date,
        after_s,
        query.cursor.as_deref(),
        limit,
    )
    .await
    .map_err(internal_error)?
    else {
        return Err(not_found("stop"));
    };
    let mut body = to_json(&board)?;
    body["realtime"] = json!(false);
    body["kind"] = json!("scheduled");
    reply(SHORT_CACHE, body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hhmm_parses_to_a_timetable_day() {
        assert_eq!(parse_hhmm("07:05"), Some(7 * 3600 + 300));
        assert_eq!(parse_hhmm("25:00"), Some(25 * 3600));
        for bad in ["7", "07:60", "48:00", "x:00", "007:00"] {
            assert_eq!(parse_hhmm(bad), None, "{bad}");
        }
    }
}

/// The routes against a live database, with a seeded test network
/// (`ds_store::test_support::transit`).
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use ds_store::test_support::transit::{A, cleanup, seed};
    use tower::ServiceExt;

    use super::*;

    const NETWORK: &str = "test-api";

    fn app(pool: sqlx::PgPool, networks: &[&str]) -> App {
        let app = crate::test_support::inert_app(pool);
        let mut state = std::sync::Arc::try_unwrap(app).unwrap_or_else(|_| unreachable!());
        state.config.transit_networks = networks.iter().map(|n| (*n).to_owned()).collect();
        std::sync::Arc::new(state)
    }

    async fn get(app: &App, uri: &str) -> (StatusCode, Option<String>, Value) {
        let router: axum::Router = Router::new().merge(router()).with_state(app.clone());
        let response = router
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let cache = response
            .headers()
            .get(header::CACHE_CONTROL)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap();
        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, cache, body)
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL (a migrated database)"]
    async fn every_route_is_404_until_the_network_is_served() {
        let pool = ds_store::test_support::connect().await;
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        seed(&pool, NETWORK, today).await;
        let dark = app(pool.clone(), &[]);
        let (status, _, body) = get(&dark, "/transit/networks").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "networks": [] }));
        for uri in [
            format!("/transit/{NETWORK}"),
            format!("/transit/{NETWORK}/lines/blue"),
            format!("/transit/{NETWORK}/lines/blue/timetable"),
            format!("/transit/{NETWORK}/stops"),
            format!("/transit/{NETWORK}/stops/{A}"),
            format!("/transit/{NETWORK}/stops/{A}/departures"),
        ] {
            assert_eq!(get(&dark, &uri).await.0, StatusCode::NOT_FOUND, "{uri}");
        }
        cleanup(&pool, NETWORK).await;
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL (a migrated database)"]
    async fn a_served_network_answers_with_scheduled_data() {
        let pool = ds_store::test_support::connect().await;
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        seed(&pool, NETWORK, today).await;
        let lit = app(pool.clone(), &[NETWORK, "other"]);

        let (status, cache, body) = get(&lit, "/transit/networks").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache.as_deref(), Some(NETWORKS_CACHE));
        assert_eq!(body["networks"][0]["id"], NETWORK);
        assert_eq!(body["networks"].as_array().unwrap().len(), 1);

        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}")).await;
        assert_eq!(body["lines"][0]["id"], format!("{NETWORK}:blue"));
        assert_eq!(body["attribution"]["licence"], "OGL-UK-3.0");

        let (status, _, body) = get(&lit, &format!("/transit/{NETWORK}/lines/blue")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["directions"][0]["stations"][0]["id"], A);
        // The path takes the local id, not the public one.
        let (status, _, _) = get(&lit, &format!("/transit/{NETWORK}/lines/{NETWORK}:blue")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, cache, body) = get(
            &lit,
            &format!("/transit/{NETWORK}/stops/{A}/departures?date={today}&after=00:00"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache.as_deref(), Some(SHORT_CACHE));
        assert_eq!(body["realtime"], false);
        assert_eq!(body["kind"], "scheduled");
        let row = body["departures"][0].as_object().unwrap();
        for absent in [
            "expected",
            "expectedDeparture",
            "delay",
            "cancelled",
            "status",
        ] {
            assert!(!row.contains_key(absent), "a scheduled row has no {absent}");
        }
        assert_eq!(row["tripId"], "T2450");

        let (status, _, body) = get(
            &lit,
            &format!("/transit/{NETWORK}/lines/blue/timetable?date={today}&from=06:00&to=09:00"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["trips"].as_array().unwrap().len(), 2);
        assert_eq!(body["realtime"], false);

        let (status, _, _) = get(
            &lit,
            &format!("/transit/{NETWORK}/lines/blue/timetable?direction=up"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _, body) = get(&lit, &format!("/transit/{NETWORK}/stops")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["stops"].as_array().unwrap().len(), 2);
        let (status, _, _) = get(&lit, &format!("/transit/{NETWORK}/stops/nope")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        cleanup(&pool, NETWORK).await;
    }
}
