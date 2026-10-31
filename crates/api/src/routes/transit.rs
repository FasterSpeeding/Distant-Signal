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
//! **Disruptions** (phase 2, spec §10, §11): a network's lines and a line
//! carry `status`, a line its `situations`, a stop its `notices`, and
//! `/transit/{network}/situations` lists the current and upcoming ones; all
//! from the published situations (`ds_store::transit::disruptions`), shown
//! only while they are under 30 minutes old ("unavailable" after, `none`
//! while the network has none).
//!
//! Path segments: `{network}` and `{line}` are local ids (`metrolink`,
//! `blue`), `{stop}` a station's ATCO code (`9400ZZMAPIC`). Bodies carry the
//! public line id (`metrolink:blue`).

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use std::collections::BTreeSet;

use chrono::{DateTime, NaiveDate, Timelike, Utc};
use common::transit::{is_id, is_stop_id};
use ds_store::transit::{disruptions, read};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};

/// `/transit/networks`.
const NETWORKS_CACHE: &str = "public, max-age=300";
/// A network (its lines and their status), a line, a stop (its notices),
/// the situations, a board.
const SHORT_CACHE: &str = "public, max-age=60";
/// A timetable.
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
        .route("/transit/rail-links", axum::routing::get(rail_links))
        .route("/transit/{network}", axum::routing::get(get_network))
        .route(
            "/transit/{network}/lines/{line}",
            axum::routing::get(get_line),
        )
        .route(
            "/transit/{network}/lines/{line}/timetable",
            axum::routing::get(get_line_timetable),
        )
        .route(
            "/transit/{network}/situations",
            axum::routing::get(get_situations),
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
    // `tripPlanning` (DS-164): whether `/Trips/plan` plans with the network
    // (`API_TRIP_PLAN_TRANSIT_NETWORKS`), so the planner offers its stops.
    // Absent while that list is empty, so the body is as before.
    let planner = crate::data::transit_planning::enabled_networks();
    let mut body = to_json(&networks)?;
    if !planner.is_empty()
        && let Some(list) = body.as_array_mut()
    {
        for (entry, network) in list.iter_mut().zip(&networks) {
            entry["tripPlanning"] = Value::Bool(planner.contains(&network.id));
        }
    }
    reply(NETWORKS_CACHE, json!({ "networks": body }))
}

#[derive(Debug, Deserialize)]
struct RailLinksQuery {
    crs: Option<String>,
}

/// `/transit/rail-links?crs=MAN`: the transit stops of every served network
/// linked to that National Rail station (spec §12.1), for its station
/// page. Empty while no network is served, or for a CRS with no link.
async fn rail_links(State(app): State<App>, Query(query): Query<RailLinksQuery>) -> Reply {
    let crs = query.crs.unwrap_or_default().to_ascii_uppercase();
    if crs.len() != 3 || !crs.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(bad_request("crs must be three letters"));
    }
    let networks = enabled(&app);
    let links = if networks.is_empty() {
        Vec::new()
    } else {
        read::rail_links_for_crs(&app.database, &networks, &crs)
            .await
            .map_err(internal_error)?
    };
    reply(LONG_CACHE, json!({ "links": to_json(&links)? }))
}

/// Where `network`'s line status comes from: `tfl` (`TfL`'s own line
/// status, the `TfL` design §7: no situations, so pages link to the line's
/// `/lines/tfl-<line>` page for the detail) or `situations` (SIRI-SX).
fn status_source(network: &str) -> &'static str {
    if common::transit::status_from_tfl(network) {
        "tfl"
    } else {
        "situations"
    }
}

/// The situations' state for `network` now (spec §10.2's staleness rule).
async fn status_state(
    app: &App,
    network: &str,
) -> Result<(disruptions::StatusState, Option<DateTime<Utc>>), (StatusCode, String)> {
    let as_of = disruptions::situations_as_of(&app.database, network)
        .await
        .map_err(internal_error)?;
    Ok((disruptions::state_at(as_of, Utc::now()), as_of))
}

/// Each line carries `status` (`disruptions::LineStatusView`): its
/// statuses while the situations are fresh, "unavailable" with the as-of
/// time once they are over 30 minutes old, `none` while the network has
/// no situations at all.
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
    let (state, as_of) = status_state(&app, &network).await?;
    let stored = disruptions::stored_line_statuses(&app.database, &network)
        .await
        .map_err(internal_error)?;
    let mut body = to_json(&summary)?;
    // The TfL network (the TfL design §8.2): TfL's lines that are National
    // Rail catalogue lines here (the Overground and the Elizabeth line),
    // which its page links to rather than timetabling again.
    if common::transit::status_from_tfl(&network) {
        let catalogue: Vec<Value> = common::TFL_TO_NR_LINE_ID
            .iter()
            .filter_map(|(_, nr)| app.config.lines.iter().find(|line| line.id == *nr))
            .map(|line| json!({ "id": line.id, "name": line.name }))
            .collect();
        body["nationalRailLines"] = Value::Array(catalogue);
    }
    let mut lines_json = Vec::with_capacity(lines.len());
    for line in &lines {
        let mut entry = to_json(line)?;
        entry["status"] = to_json(&disruptions::line_view(
            state,
            as_of,
            stored.get(&line.line_id),
        ))?;
        lines_json.push(entry);
    }
    body["lines"] = Value::Array(lines_json);
    body["status"] = json!({ "state": state, "asOf": as_of });
    body["statusSource"] = json!(status_source(&network));
    reply(SHORT_CACHE, body)
}

#[derive(Debug, Deserialize)]
struct LineQuery {
    /// The frequency summary's date (default: today in London).
    date: Option<NaiveDate>,
}

/// The line, its `status`, the current and upcoming `situations` that
/// concern it (those changing its status, and stop notices for its
/// stations), and its `frequency` on `?date=` (the `TfL` design §8.3).
async fn get_line(
    State(app): State<App>,
    Path((network, line)): Path<(String, String)>,
    Query(query): Query<LineQuery>,
) -> Reply {
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
    let (state, as_of) = status_state(&app, &network).await?;
    let stored = disruptions::stored_line_statuses(&app.database, &network)
        .await
        .map_err(internal_error)?;
    let situations = disruptions::stored_situations(&app.database, &network)
        .await
        .map_err(internal_error)?;
    let stations: BTreeSet<String> = detail
        .directions
        .iter()
        .flat_map(|d| d.stations.iter().map(|s| s.id.clone()))
        .collect();
    let view = disruptions::situations_view(&network, &situations, state, as_of, Utc::now(), |s| {
        disruptions::concerns(s, &line, &stations)
    });
    let date = query
        .date
        .unwrap_or_else(|| super::london_now().date_naive());
    let frequency = read::line_frequency(&app.database, &network, &line, date)
        .await
        .map_err(internal_error)?;
    let mut body = to_json(&detail)?;
    body["status"] = to_json(&disruptions::line_view(state, as_of, stored.get(&line)))?;
    body["situations"] = to_json(&view)?;
    body["frequency"] = to_json(&frequency)?;
    body["statusSource"] = json!(status_source(&network));
    reply(SHORT_CACHE, body)
}

/// `/transit/{network}/situations`: every current and upcoming situation.
async fn get_situations(State(app): State<App>, Path(network): Path<String>) -> Reply {
    check_network(&app, &network)?;
    if read::get_network(&app.database, &network)
        .await
        .map_err(internal_error)?
        .is_none()
    {
        return Err(not_found("transit network"));
    }
    let (state, as_of) = status_state(&app, &network).await?;
    let situations = disruptions::stored_situations(&app.database, &network)
        .await
        .map_err(internal_error)?;
    let view =
        disruptions::situations_view(&network, &situations, state, as_of, Utc::now(), |_| true);
    reply(SHORT_CACHE, to_json(&view)?)
}

#[derive(Debug, Deserialize)]
struct TimetableQuery {
    date: Option<NaiveDate>,
    direction: Option<String>,
    /// `HH:MM`.
    from: Option<String>,
    to: Option<String>,
    /// A branch of the line (`branches[].id`, the `TfL` design §8.3).
    branch: Option<String>,
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
        read::TimetableWindow {
            date,
            direction: &direction,
            from_s,
            to_s,
            branch: query.branch.as_deref(),
        },
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

/// A stop id as the network's tables spell it. The `tfl` network's
/// stations are `TfL`'s stops, whose pages and ids are the stop areas
/// (`940GZZLUKSX`); the tables hold the access nodes (`9400ZZLUKSX`). Both
/// spellings are taken (the `TfL` design §8.4).
fn table_stop_id(network: &str, stop: &str) -> String {
    match stop.strip_prefix("940G") {
        Some(rest) if common::transit::status_from_tfl(network) => format!("9400{rest}"),
        _ => stop.to_owned(),
    }
}

#[derive(Debug, Deserialize)]
struct StopQuery {
    /// The `firstLast` date (default: today in London).
    date: Option<NaiveDate>,
}

/// A station, its platforms, lines and rail links, its `notices`, and
/// `firstLast`: its first and last departures on `?date=` per line and
/// direction.
async fn get_stop(
    State(app): State<App>,
    Path((network, stop)): Path<(String, String)>,
    Query(query): Query<StopQuery>,
) -> Reply {
    check_network(&app, &network)?;
    if !is_stop_id(&stop) {
        return Err(not_found("stop"));
    }
    let stop = table_stop_id(&network, &stop);
    let Some(detail) = read::get_stop(&app.database, &network, &stop)
        .await
        .map_err(internal_error)?
    else {
        return Err(not_found("stop"));
    };
    // `notices`: the current situations naming this station (a lift out
    // of service, a stop closed), while the situations are fresh.
    let (state, as_of) = status_state(&app, &network).await?;
    let situations = disruptions::stored_situations(&app.database, &network)
        .await
        .map_err(internal_error)?;
    let view = disruptions::situations_view(&network, &situations, state, as_of, Utc::now(), |s| {
        s.stop_ids.iter().any(|id| id == &stop)
    });
    let date = query
        .date
        .unwrap_or_else(|| super::london_now().date_naive());
    let first_last = read::stop_first_last(&app.database, &network, &stop, date)
        .await
        .map_err(internal_error)?
        .unwrap_or_default();
    let mut body = to_json(&detail)?;
    body["notices"] = to_json(&view.current)?;
    body["firstLast"] = to_json(&first_last)?;
    reply(SHORT_CACHE, body)
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
    let stop = table_stop_id(&network, &stop);
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
    use ds_store::test_support::transit::{A, B, cleanup, seed};
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
        assert_eq!(body["mode"], "tram");
        assert_eq!(body["statusLineId"], format!("{NETWORK}:blue"));
        assert_eq!(body["statusSource"], "situations");

        // The frequency summary (the TfL design §8.3): outbound from A at
        // 06:00, 07:00, 23:50 and 00:10; inbound from B at 07:30.
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/lines/blue?date={today}")).await;
        let frequency = &body["frequency"];
        assert_eq!(frequency["outsideHorizon"], false);
        let outbound = &frequency["directions"][0];
        assert_eq!(outbound["direction"], "outbound");
        assert_eq!(outbound["station"]["id"], A);
        assert_eq!(
            (outbound["first"].as_str(), outbound["last"].as_str()),
            (Some("06:00"), Some("00:10"))
        );
        let late = outbound["bands"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["band"] == "late")
            .unwrap();
        assert_eq!(late["departures"], 2);
        assert_eq!(late["minGapMinutes"], 20);
        assert_eq!(frequency["directions"][1]["station"]["id"], B);
        assert_eq!(frequency["directions"][1]["first"], "07:30");
        let far = today.checked_add_days(chrono::Days::new(30)).unwrap();
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/lines/blue?date={far}")).await;
        assert_eq!(body["frequency"]["outsideHorizon"], true);
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

    /// The `TfL` design §7, §8.2: the `tfl` network's lines take `TfL`'s own
    /// status (`tfl-<line>` rows, `ingest_freshness('tfl')`), pin by that
    /// id, have no situations, and the network lists `TfL`'s National Rail
    /// catalogue lines.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL (a migrated database)"]
    async fn tfl_lines_take_tfls_own_status() {
        let pool = ds_store::test_support::connect().await;
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        seed(&pool, "tfl", today).await;
        let owner = ds_store::test_support::schedule_fixture_pool(&pool).await;
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ('tfl-blue', 'Blue', 'tube', '{TfL}', \
               '[{\"severity\": 9, \"reason\": \"Signal failure\", \"validity\": \
                 {\"from_date\": \"2026-10-14T00:00:00Z\", \"to_date\": null, \"is_now\": true}}]', 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET statuses = EXCLUDED.statuses, source = 'tfl'",
        )
        .execute(&owner)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO ingest_freshness (source, fetched_at) VALUES ('tfl', now()) \
             ON CONFLICT (source) DO UPDATE SET fetched_at = now()",
        )
        .execute(&owner)
        .await
        .unwrap();
        let mut lit = app(pool.clone(), &["tfl"]);
        std::sync::Arc::get_mut(&mut lit).unwrap().config.lines =
            common::config::parse_lines(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lines"))
                .unwrap();

        let (status, _, body) = get(&lit, "/transit/tfl").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["statusSource"], "tfl");
        assert_eq!(body["status"]["state"], "current");
        let line = &body["lines"][0];
        assert_eq!(line["statusLineId"], "tfl-blue");
        assert_eq!(line["status"]["state"], "current");
        assert_eq!(line["status"]["statuses"][0]["description"], "Minor Delays");
        let national_rail: Vec<&str> = body["nationalRailLines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| l["id"].as_str().unwrap())
            .collect();
        assert!(
            national_rail.contains(&"elizabeth-line"),
            "{national_rail:?}"
        );
        assert!(
            national_rail.contains(&"overground-mildmay"),
            "{national_rail:?}"
        );

        // TfL's stop-area spelling reaches the station (the TfL design §8.4),
        // with its first and last departures of the day.
        let (status, _, body) = get(
            &lit,
            &format!("/transit/tfl/stops/940GZZTTAAA?date={today}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["id"], A);
        let first_last = &body["firstLast"][0];
        assert_eq!(first_last["line"]["id"], "tfl:blue");
        assert_eq!(first_last["direction"], "outbound");
        assert_eq!(first_last["towards"], "Bbb");
        assert_eq!(
            (first_last["first"].as_str(), first_last["last"].as_str()),
            (Some("06:00"), Some("00:10"))
        );
        let (status, _, body) = get(
            &lit,
            &format!("/transit/tfl/stops/940GZZTTAAA/departures?date={today}&after=06:00"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["departures"][0]["tripId"], "T0600");
        // Another network keeps its own spelling only.
        let (_, _, body) = get(&lit, "/transit/tfl/lines/blue").await;
        assert_eq!(body["statusSource"], "tfl");
        assert_eq!(body["branches"][0]["id"], "outbound-1");
        assert_eq!(body["status"]["statuses"][0]["reason"], "Signal failure");
        assert!(body["situations"]["current"].as_array().unwrap().is_empty());

        sqlx::query("DELETE FROM line_status WHERE line_id = 'tfl-blue'")
            .execute(&owner)
            .await
            .unwrap();
        cleanup(&pool, "tfl").await;
    }

    /// Plan 5.1: a National Rail station's linked tram stops, only from
    /// served networks; and the stop's own link names the station.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL (a migrated database)"]
    async fn rail_links_come_from_served_networks_only() {
        let pool = ds_store::test_support::connect().await;
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        seed(&pool, NETWORK, today).await;

        let lit = app(pool.clone(), &[NETWORK]);
        let (status, cache, body) = get(&lit, "/transit/rail-links?crs=ZZT").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache.as_deref(), Some(LONG_CACHE));
        let links = body["links"].as_array().unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0]["network"], NETWORK);
        assert_eq!(links[0]["stopId"], A);
        assert_eq!(links[0]["kind"], "interchange");
        // Lower case is accepted; a CRS with no link is an empty list.
        let (_, _, body) = get(&lit, "/transit/rail-links?crs=zzt").await;
        assert_eq!(body["links"].as_array().unwrap().len(), 1);
        let (_, _, body) = get(&lit, "/transit/rail-links?crs=KGX").await;
        assert_eq!(body, json!({ "links": [] }));
        let (status, _, _) = get(&lit, "/transit/rail-links?crs=KGXX").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/stops/{A}")).await;
        assert_eq!(body["railLinks"][0]["crs"], "ZZT");
        assert!(
            body["railLinks"][0]
                .as_object()
                .unwrap()
                .contains_key("stationName")
        );

        // Dark: nothing, not a 404 (the station page asks for every CRS).
        let dark = app(pool.clone(), &[]);
        let (status, _, body) = get(&dark, "/transit/rail-links?crs=ZZT").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, json!({ "links": [] }));
        cleanup(&pool, NETWORK).await;
    }

    /// Plan 4.5: status and situations by the situations' freshness, the
    /// stop notices, and the situations listing's order.
    #[tokio::test]
    #[ignore = "requires DATABASE_URL (a migrated database)"]
    async fn status_follows_the_situations_freshness() {
        use ds_store::test_support::transit::{apply_situations, set_situations_as_of, situation};

        let pool = ds_store::test_support::connect().await;
        let today = NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        seed(&pool, NETWORK, today).await;
        let lit = app(pool.clone(), &[NETWORK]);

        // Missing: no status at all.
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}")).await;
        assert_eq!(body["status"]["state"], "none");
        assert_eq!(body["lines"][0]["status"]["state"], "none");
        let (status, cache, body) = get(&lit, &format!("/transit/{NETWORK}/situations")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cache.as_deref(), Some(SHORT_CACHE));
        assert_eq!(body["state"], "none");

        // Fresh: a delay on blue naming stop B, a later one, and an
        // upcoming one.
        let now = Utc::now();
        let mut upcoming = situation("S3", "blue", "suspended", None, now);
        upcoming.validity[0].start = now + chrono::TimeDelta::days(2);
        let mut later = situation("S2", "blue", "delayed", None, now);
        later.validity[0].start = now - chrono::TimeDelta::minutes(10);
        apply_situations(
            &pool,
            NETWORK,
            vec![
                situation("S1", "blue", "delayed", Some(B), now),
                later,
                upcoming,
            ],
            now,
        )
        .await;
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}")).await;
        assert_eq!(body["status"]["state"], "current");
        let line_status = &body["lines"][0]["status"];
        assert_eq!(line_status["state"], "current");
        assert_eq!(line_status["statuses"][0]["severity"], 9);
        assert_eq!(line_status["statuses"][0]["description"], "Minor Delays");
        assert_eq!(line_status["upcoming"][0]["situationId"], "S3");

        let (_, cache, body) = get(&lit, &format!("/transit/{NETWORK}/lines/blue")).await;
        assert_eq!(cache.as_deref(), Some(SHORT_CACHE));
        assert_eq!(body["status"]["state"], "current");
        let ids = |list: &Value| -> Vec<String> {
            list.as_array()
                .unwrap()
                .iter()
                .map(|s| s["id"].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(ids(&body["situations"]["current"]), ["S1", "S2"]);
        assert_eq!(ids(&body["situations"]["upcoming"]), ["S3"]);
        assert_eq!(
            body["situations"]["current"][0]["lines"][0],
            format!("{NETWORK}:blue")
        );
        assert_eq!(
            body["situations"]["current"][0]["advice"][0],
            "Allow extra time."
        );

        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/situations")).await;
        assert_eq!(ids(&body["current"]), ["S1", "S2"]);
        assert_eq!(ids(&body["upcoming"]), ["S3"]);

        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/stops/{B}")).await;
        assert_eq!(ids(&body["notices"]), ["S1"]);
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/stops/{A}")).await;
        assert!(body["notices"].as_array().unwrap().is_empty());

        // Stale: "unavailable" with its as-of time, and nothing listed.
        let stale = now - chrono::TimeDelta::minutes(45);
        set_situations_as_of(&pool, NETWORK, Some(stale)).await;
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}")).await;
        assert_eq!(body["status"]["state"], "unavailable");
        let line_status = &body["lines"][0]["status"];
        assert_eq!(line_status["state"], "unavailable");
        assert!(line_status["asOf"].is_string());
        assert!(line_status["statuses"].as_array().unwrap().is_empty());
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/situations")).await;
        assert_eq!(body["state"], "unavailable");
        assert!(body["current"].as_array().unwrap().is_empty());
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/stops/{B}")).await;
        assert!(body["notices"].as_array().unwrap().is_empty());

        set_situations_as_of(&pool, NETWORK, None).await;
        let (_, _, body) = get(&lit, &format!("/transit/{NETWORK}/lines/blue")).await;
        assert_eq!(body["status"]["state"], "none");

        // Dark: the situations route is 404 like the rest.
        let dark = app(pool.clone(), &[]);
        let (status, _, _) = get(&dark, &format!("/transit/{NETWORK}/situations")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        cleanup(&pool, NETWORK).await;
    }
}
