//! `GET /Trips/plan` -- the read-only journey-planning endpoint. See
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §5.2
//! and
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase5-planning-api-plan.md's
//! own Judgment Call 1 for why this lives under a new `/Trips` prefix, not
//! `/Journeys/*`. Unauthenticated, read-only -- computing a hypothetical
//! itinerary commits nothing and belongs to no user, matching
//! `reference::nearest_stations`'s own public/read-only posture, not
//! `routes::journeys`'s authenticated-write one.

use std::sync::LazyLock;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;

use crate::app::App;
use crate::data::{trip_planning, trip_planning_itinerary};

/// Hard cap on `?waypoints=`, enforced before any database read (2026-09-25
/// review, High 4b). `plan_via_waypoints` solves one INDEPENDENT pathfinding
/// search per consecutive pair of stops, so `n` waypoints means `n + 1` full
/// searches over the whole day's connections graph -- and nothing capped `n`.
/// `?waypoints=YRK,YRK,YRK,...` repeated a few hundred times is a legal query
/// string that turns one unauthenticated GET into hundreds of graph searches.
///
/// 8 is generous for a real itinerary (a ten-leg cross-country trip with
/// eight intermediate stops the traveller specifically asked to route via)
/// while keeping the worst case a small multiple of the single-leg cost, not
/// an unbounded one.
const MAX_WAYPOINTS: usize = 8;

/// Global cap on trip plans being computed at once (2026-09-25 review, High
/// 4c). This is a per-process concurrency gate, NOT a per-IP rate limit --
/// stated plainly because the two are often conflated: it bounds how much of
/// this box's CPU one endpoint can hold at any instant, and says nothing
/// about how often any single caller may ask.
///
/// A plain `tokio::sync::Semaphore` with `try_acquire`, rather than a tower
/// layer: `tower::limit::ConcurrencyLimitLayer` QUEUES excess requests
/// (unbounded, since axum awaits readiness through `oneshot`) instead of
/// shedding them, which converts a CPU flood into a memory flood plus
/// ever-growing latency, and `tower_governor` (a real per-IP limiter) is not
/// a dependency of this workspace and would need trusted client-IP
/// extraction to be meaningful behind this app's Ingress. Shedding with a 503
/// is the honest behaviour for work this expensive: the caller learns
/// immediately, and every other route -- healthcheck included -- keeps its
/// worker threads.
///
/// 4 permits: enough that ordinary interactive use never sees a 503 (a plan
/// takes well under a second), small enough that the blocking pool retains
/// ample capacity for the PDF parser and sqlx's own work.
static PLAN_SLOTS: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(4));

pub fn router() -> crate::app::Router {
    crate::app::Router::new().route("/Trips/plan", axum::routing::get(get_trip_plan))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TripPlanParams {
    origin: String,
    destination: String,
    /// Comma-separated ordered CRS codes, e.g. `?waypoints=YRK,NCL`. Absent
    /// or empty means no waypoints -- a direct origin->destination plan.
    #[serde(default)]
    waypoints: Option<String>,
    date: NaiveDate,
    #[serde(default)]
    depart_after: Option<NaiveTime>,
    #[serde(default = "default_results")]
    results: String,
}

fn default_results() -> String {
    "fastest".to_string()
}

/// **Read this before adding work to this handler.** One request here reads
/// every `schedule_calling_points_full` row for the requested date (hundreds
/// of thousands), builds and sorts the whole day's connections graph in
/// memory, and then runs one graph search per leg. That is by far the most
/// expensive thing this unauthenticated API can be asked to do, so three
/// separate bounds apply, all of them load-bearing (2026-09-25 review, High
/// 4) and none of them a substitute for another:
///
/// 1. [`MAX_WAYPOINTS`], checked BEFORE any database read -- bounds how many
///    graph searches one request can ask for. Rejected requests cost a string
///    split, not a query.
/// 2. [`PLAN_SLOTS`], held across the whole read-plus-compute body -- bounds
///    how many of these can be in flight at once, so the peak is a few
///    graphs' worth of memory and a few threads, not one per connection.
/// 3. `spawn_blocking` around the graph build and the searches -- keeps
///    minutes of synchronous CPU off the tokio worker threads. Without it, a
///    handful of concurrent requests starved every async task in the process,
///    including `/public/health`, so the API looked dead rather than slow and
///    orchestration would restart a pod that was merely busy. Same treatment,
///    for the same reason, as `routes::train`'s PDF ticket parser.
async fn get_trip_plan(
    State(app): State<App>,
    Query(params): Query<TripPlanParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if params.results != "fastest" && params.results != "options" {
        return Err((
            StatusCode::BAD_REQUEST,
            "results must be 'fastest' or 'options'".to_string(),
        ));
    }

    let waypoints = parse_waypoints(params.waypoints.as_deref())?;

    // Acquired BEFORE the reads below, not just around the search: the
    // whole-day row read and the graph built from it are the memory half of
    // this endpoint's cost, and admitting unbounded concurrent requests to
    // that and only serialising the CPU afterwards would still let a few
    // callers exhaust this process's memory. `try_acquire`, so an overloaded
    // process sheds load immediately instead of queueing unboundedly.
    let Ok(_permit) = PLAN_SLOTS.try_acquire() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "too many trip plans are being computed right now; please retry in a moment"
                .to_string(),
        ));
    };

    let Some(calling_points) =
        trip_planning::fetch_calling_points_for_date(&app.database, params.date)
            .await
            .map_err(internal_error("read calling points"))?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "no CIF-derived schedule data has been published for {} yet",
                params.date
            ),
        ));
    };

    let interchange = trip_planning::fetch_interchange_data(&app.database)
        .await
        .map_err(internal_error("fetch interchange data"))?;

    // Everything CPU-bound in one hop onto the blocking pool: building the
    // connections graph (a sort over every calling point of the day) AND the
    // per-leg searches. Owned values are moved in rather than borrowed --
    // `spawn_blocking` needs `'static`, and both were built for this request
    // alone and are dropped when it ends.
    let date = params.date;
    let origin = params.origin.trim().to_ascii_uppercase();
    let destination = params.destination.trim().to_ascii_uppercase();
    let depart_after = params.depart_after.unwrap_or(NaiveTime::MIN);
    let results = params.results.clone();
    let segments = tokio::task::spawn_blocking(move || {
        let connections = trip_planning::build_connections(calling_points);
        trip_planning_itinerary::plan_via_waypoints(
            &connections,
            &interchange,
            date,
            &origin,
            &waypoints,
            &destination,
            depart_after,
            &results,
        )
    })
    .await
    .map_err(|join_err| {
        tracing::error!(error = ?join_err, "trip planning task panicked or was cancelled");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to plan trip".to_string(),
        )
    })?
    .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    Ok(Json(serde_json::json!({
        "results": params.results,
        "segments": segments.iter().map(|segment| serde_json::json!({
            "originCrs": segment.origin_crs,
            "destinationCrs": segment.destination_crs,
            "itineraries": segment.itineraries,
            "cappedByMaxChanges": segment.capped_by_max_changes,
        })).collect::<Vec<_>>(),
    })))
}

/// Splits, trims, uppercases and CAPS the `?waypoints=` list. Factored out of
/// the handler purely so the cap is testable without a database or an HTTP
/// stack -- the parsing itself is unchanged.
///
/// The cap is a 400 with a message naming the limit, not a silent truncation:
/// silently dropping waypoints would return an itinerary that answers a
/// different question than the one asked, which for a journey planner is
/// worse than an error.
fn parse_waypoints(raw: Option<&str>) -> Result<Vec<String>, (StatusCode, String)> {
    let waypoints: Vec<String> = raw
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_uppercase())
        .collect();

    if waypoints.len() > MAX_WAYPOINTS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "too many waypoints: {} given, at most {MAX_WAYPOINTS} allowed",
                waypoints.len()
            ),
        ));
    }
    Ok(waypoints)
}

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "trip plan request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to {operation}"),
        )
    }
}

/// Pure, DB-free tests for this route's own query-param parsing -- no
/// `db_tests`-style `#[ignore]`/live-database dependency needed, since
/// `Query<TripPlanParams>::try_from_uri` is exactly the same deserialization
/// path axum's `FromRequestParts` impl for `Query` uses at request time (see
/// `axum::extract::query::Query::try_from_uri`, which
/// `from_request_parts` calls directly).
#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for a final-whole-branch-review finding: without
    /// `#[serde(rename_all = "camelCase")]` on `TripPlanParams`, this
    /// route's own documented wire contract (`?departAfter=17:00`, matching
    /// every other route in this crate) silently failed to populate
    /// `depart_after` at all -- `serde_urlencoded`'s `Query` extractor
    /// ignores unknown query keys rather than erroring, so the caller got
    /// no error and a plan silently searched from `00:00` instead of the
    /// requested time.
    #[test]
    fn depart_after_is_read_from_its_camel_case_wire_name() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=EUS&destination=MKC&\
                                     date=2026-09-23&departAfter=17:00"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(
            params.depart_after,
            Some(NaiveTime::from_hms_opt(17, 0, 0).unwrap()),
            "departAfter=17:00 must populate depart_after, not silently leave it None \
             (which the handler then defaults to NaiveTime::MIN, searching from 00:00)"
        );
    }

    /// The old, non-wire key must NOT work -- otherwise this test would
    /// pass for the wrong reason (e.g. some other default) rather than
    /// proving the camelCase rename is what did it.
    #[test]
    fn the_old_snake_case_key_no_longer_matches() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=EUS&destination=MKC&\
                                     date=2026-09-23&depart_after=17:00"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(
            params.depart_after, None,
            "depart_after (snake_case) is not this route's documented wire key; it must be \
             silently ignored just like any other unknown query key, not accidentally accepted"
        );
    }

    /// **The 2026-09-25 High 4b regression test**: `?waypoints=` had no count
    /// cap at all, and each waypoint costs a FULL extra pathfinding search
    /// over the whole day's connections graph (`plan_via_waypoints` solves one
    /// per consecutive pair of stops). A single unauthenticated GET carrying a
    /// few hundred repeated waypoints therefore bought a few hundred graph
    /// searches on one request -- the cheapest possible way to pin this
    /// process's CPU.
    ///
    /// Asserts the cap rejects with a 400 naming the limit, and -- crucially
    /// -- that it is checked at PARSE time, before any database read: this
    /// test never touches a pool.
    #[test]
    fn too_many_waypoints_are_rejected_before_any_database_read() {
        let raw = ["YRK"; MAX_WAYPOINTS + 1].join(",");
        let (status, message) =
            parse_waypoints(Some(&raw)).expect_err("more than the cap must be rejected");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            message.contains(&MAX_WAYPOINTS.to_string()),
            "the error must name the limit so a caller can fix the request: {message}"
        );
    }

    /// The other side of the cap: a generous-but-real itinerary is untouched,
    /// and the parsing (trim, uppercase, drop empties) still behaves exactly
    /// as it did before the cap existed.
    #[test]
    fn a_waypoint_list_at_the_cap_is_accepted_and_still_normalised() {
        let raw = ["yrk"; MAX_WAYPOINTS].join(" , ");
        let waypoints = parse_waypoints(Some(&raw)).expect("exactly the cap is allowed");
        assert_eq!(waypoints.len(), MAX_WAYPOINTS);
        assert!(waypoints.iter().all(|crs| crs == "YRK"));

        assert_eq!(
            parse_waypoints(None).expect("absent is fine"),
            Vec::<String>::new()
        );
        assert_eq!(
            parse_waypoints(Some(" ,, ")).expect("empty entries are dropped"),
            Vec::<String>::new()
        );
    }
}

/// Scaffolding copied verbatim from `routes::journeys::db_tests`'s own
/// `test_app`/`test_router`/`connect` (that module's own comment notes it
/// was itself copied from `routes::train::db_tests` -- the same
/// cross-file-duplication convention this crate uses throughout
/// `crates/api/src/routes/*.rs`'s own `db_tests` modules rather than a
/// shared crate-visible fixture). `test_app`/`test_router`/`connect` below
/// are that same fixture, trimmed to only what this file's tests actually
/// use: no `seed_session`/`post_json` (this endpoint is unauthenticated
/// and GET-only), no `cleanup_user` (this endpoint has no per-user state to
/// clean up -- only the ad-hoc `schedule_calling_points_full`/`stanox_crs`
/// rows the seeded-connection test inserts and deletes itself).
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use super::*;
    use crate::app::AppState;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Every `ServiceArguments` field filled with an inert placeholder --
    /// this route doesn't read `config.lines` (or any other config field)
    /// at all. Copied from `routes::journeys::db_tests::test_app`'s own
    /// empty-index default.
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
            // `Client::open` only parses the URL, never opens a socket --
            // this route never touches Redis at all.
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

    /// The real `trips::router()`, mounted unprefixed exactly as `main.rs`
    /// does, turned into a `tower::Service` a test can drive with
    /// `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .with_state(app)
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Issues a GET against `router` and returns `(status, parsed JSON
    /// body)`. This route always returns either a JSON object body or a
    /// plain-text `(StatusCode, String)` error body, so wrapping the latter
    /// as a JSON string lets every case share one return shape -- same
    /// convention as `routes::journeys::db_tests::request`.
    async fn get(router: axum::Router, uri: String) -> (StatusCode, Value) {
        let req = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn an_unresolvable_origin_crs_is_a_clear_error() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        // Seed at least one calling-point row for the date, so this fails
        // on CRS resolution specifically, not on the "no schedule data
        // published for this date" 404 path.
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANZZZ', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANZZZ', 1, 'MILTNKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=ZZZ&destination=MKC&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(
            body.as_str().unwrap().contains("ZZZ"),
            "error must name the unresolvable CRS: {body:?}"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLANZZZ'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_date_with_no_published_schedule_data_is_a_clear_404() {
        let pool = connect().await;
        let router = test_router(test_app(pool));
        let (status, body) = get(
            router,
            "/Trips/plan?origin=EUS&destination=MKC&date=2099-01-01".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.as_str().unwrap().contains("2099-01-01"));
    }

    /// The end-to-end half of the 2026-09-25 High 4b fix: the cap is enforced
    /// by the real route, and it fires BEFORE the "no schedule data published
    /// for this date" read -- proven by using a date nothing is published for
    /// (which would otherwise 404) and asserting a 400 instead. A rejected
    /// request must not have cost a whole-day row read.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_waypoint_flood_is_rejected_before_the_date_is_even_looked_up() {
        let pool = connect().await;
        let router = test_router(test_app(pool));
        let flood = ["YRK"; MAX_WAYPOINTS + 40].join(",");
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=EUS&destination=MKC&date=2099-01-01&waypoints={flood}"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a waypoint flood must be rejected outright, not planned: {body:?}"
        );
        let message = body.as_str().expect("plain-text error body");
        assert!(
            message.contains("waypoints"),
            "the error must say what was wrong: {message}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn an_invalid_results_value_is_a_clear_400() {
        let pool = connect().await;
        let router = test_router(test_app(pool));
        let (status, body) = get(
            router,
            "/Trips/plan?origin=EUS&destination=MKC&date=2026-09-23&results=quickest".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let message = body.as_str().unwrap();
        assert!(message.contains("fastest"));
        assert!(message.contains("options"));
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn plan_via_waypoints_names_the_failing_segment() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANWP', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANWP', 1, 'MILTNKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANWP-EUS', 'EUS', 'EUSTON', 'LONDON EUSTON', 1), \
                    ('TESTPLANWP-MKC', 'MKC', 'MILTNKC', 'MILTON KEYNES CENTRAL', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=EUS&destination=MKC&waypoints=ZZZ&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(
            body.as_str().unwrap().contains("EUS -> ZZZ"),
            "error must name the failing segment: {body:?}"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLANWP'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANWP-%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// The sibling of `plan_via_waypoints_names_the_failing_segment` above:
    /// that test's failing segment happens to be the FIRST one
    /// (`EUS -> ZZZ`), so it doesn't exercise "an earlier segment resolved
    /// fine, and the error names the LATER one that didn't" -- a distinct
    /// case (`plan_via_waypoints`'s per-segment loop must keep going past
    /// a successful segment and still attribute the eventual failure
    /// correctly, not just report failure-at-index-0 correctly). Here
    /// `origin=EUS, waypoints=MKC, destination=ZZZ` makes the FIRST
    /// segment `EUS -> MKC` (both resolve -- no error), and the SECOND
    /// `MKC -> ZZZ` the one that fails.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn plan_via_waypoints_names_the_failing_segment_when_an_earlier_one_succeeded() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANWP2', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANWP2', 1, 'MILTNKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANWP2-EUS', 'EUS', 'EUSTON', 'LONDON EUSTON', 1), \
                    ('TESTPLANWP2-MKC', 'MKC', 'MILTNKC', 'MILTON KEYNES CENTRAL', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=EUS&waypoints=MKC&destination=ZZZ&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        let message = body.as_str().unwrap();
        assert!(
            message.contains("MKC -> ZZZ"),
            "error must name the SECOND, failing segment, not the first (which resolved fine): {message:?}"
        );
        assert!(
            !message.contains("EUS -> MKC"),
            "the first segment resolved fine and must not be reported as the failure: {message:?}"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLANWP2'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANWP2-%'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_real_seeded_connection_is_found_end_to_end() {
        // Regression test for a final-whole-branch-review finding: the
        // origin/destination CRS codes here (and their underlying TIPLOCs)
        // must be entirely synthetic, not real ones like EUS/MKC. On any
        // database that already has real EUS/MKC reference data (i.e. any
        // real deployment), a search keyed on the real EUSTON/MILTNKC
        // TIPLOCs would match every real service touching them on this
        // date too, not just the one seeded below -- and a real, faster
        // service could easily beat the seeded one, breaking the
        // `trainUid == "TESTPLAN1"` assertion. Using synthetic TIPLOCs that
        // no real schedule data could ever reference keeps the search
        // space provably isolated.
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLAN1', 0, 'TESTPL1O', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLAN1', 1, 'TESTPL1D', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLAN-ZZA', 'ZZA', 'TESTPL1O', 'TEST PLAN ORIGIN', 1), \
                    ('TESTPLAN-ZZB', 'ZZB', 'TESTPL1D', 'TEST PLAN DESTINATION', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=ZZA&destination=ZZB&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segments = body["segments"].as_array().expect("segments array");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0]["originCrs"], "ZZA");
        assert_eq!(segments[0]["destinationCrs"], "ZZB");
        let itineraries = segments[0]["itineraries"]
            .as_array()
            .expect("itineraries array");
        assert_eq!(itineraries.len(), 1);
        assert_eq!(itineraries[0]["legs"][0]["trainUid"], "TESTPLAN1");

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLAN1'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLAN-ZZ%'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn options_mode_excludes_results_over_the_cap_but_flags_when_capped() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        // Within-cap route: 2 changes (3 legs), ORIGIN -> A -> B -> DEST.
        // Over-cap-but-faster route: 3 changes (4 legs),
        // ORIGIN -> P -> Q -> R -> DEST, arriving strictly before the
        // within-cap route -- so `options` mode must exclude it from
        // `itineraries` but flag `cappedByMaxChanges: true`.
        //
        // Regression test for a final-whole-branch-review finding: ORIGIN
        // and DEST here are entirely synthetic CRS codes/TIPLOCs, not real
        // ones like EUS/MKC -- same reasoning as
        // `a_real_seeded_connection_is_found_end_to_end` above: real
        // background data for a real CRS/TIPLOC could add extra
        // within-cap routes and break the `itineraries.len() == 1`
        // assertion below.
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANCAPA', 0, 'TESTPCEO', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANCAPA', 1, 'TESTPLA', 'terminate', '08:15:00', NULL, 0), \
                    ($1, 'TESTPLANCAPB', 0, 'TESTPLA', 'origin', NULL, '08:20:00', 0), \
                    ($1, 'TESTPLANCAPB', 1, 'TESTPLB', 'terminate', '08:35:00', NULL, 0), \
                    ($1, 'TESTPLANCAPC', 0, 'TESTPLB', 'origin', NULL, '08:40:00', 0), \
                    ($1, 'TESTPLANCAPC', 1, 'TESTPCMD', 'terminate', '09:00:00', NULL, 0), \
                    ($1, 'TESTPLANFASTA', 0, 'TESTPCEO', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANFASTA', 1, 'TESTPLP', 'terminate', '08:10:00', NULL, 0), \
                    ($1, 'TESTPLANFASTB', 0, 'TESTPLP', 'origin', NULL, '08:15:00', 0), \
                    ($1, 'TESTPLANFASTB', 1, 'TESTPLQ', 'terminate', '08:20:00', NULL, 0), \
                    ($1, 'TESTPLANFASTC', 0, 'TESTPLQ', 'origin', NULL, '08:25:00', 0), \
                    ($1, 'TESTPLANFASTC', 1, 'TESTPLR', 'terminate', '08:30:00', NULL, 0), \
                    ($1, 'TESTPLANFASTD', 0, 'TESTPLR', 'origin', NULL, '08:35:00', 0), \
                    ($1, 'TESTPLANFASTD', 1, 'TESTPCMD', 'terminate', '08:45:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANCAP-ZZC', 'ZZC', 'TESTPCEO', 'TEST CAP ORIGIN', 1), \
                    ('TESTPLANCAP-ZZD', 'ZZD', 'TESTPCMD', 'TEST CAP DESTINATION', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        // No `stanox_crs` row (and so no `change_time_by_tiploc` entry) for
        // any of the intermediate TIPLOCs (TESTPLA/B, TESTPLP/Q/R) --
        // `minimum_change_time` falls back to its own
        // `DEFAULT_CHANGE_TIME` (5 minutes,
        // `schedule_query::interchange::DEFAULT_CHANGE_TIME`) for a TIPLOC
        // with no MSN record at all, which every gap below is seeded to
        // exactly meet.

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=ZZC&destination=ZZD&date={date}&results=options"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segments = body["segments"].as_array().expect("segments array");
        assert_eq!(segments.len(), 1);
        let itineraries = segments[0]["itineraries"]
            .as_array()
            .expect("itineraries array");
        for itinerary in itineraries {
            assert!(
                itinerary["changeCount"].as_u64().unwrap()
                    <= u64::from(trip_planning_itinerary::MAX_CHANGES),
                "{itinerary:?}"
            );
        }
        // The genuinely-within-cap 2-change route must actually be found
        // and returned -- `capped` is computed independently of
        // `within_cap` in `plan_segment`, so a regression that wrongly
        // drops the within-cap route would still leave `cappedByMaxChanges`
        // true and pass the loop above with an empty `itineraries` unless
        // this is asserted explicitly.
        assert_eq!(
            itineraries.len(),
            1,
            "the within-cap 2-change route must be found: {body:?}"
        );
        assert_eq!(itineraries[0]["changeCount"], 2, "{body:?}");
        assert_eq!(
            segments[0]["cappedByMaxChanges"], true,
            "a strictly faster, over-cap itinerary exists and must be flagged: {body:?}"
        );

        for uid in [
            "TESTPLANCAPA",
            "TESTPLANCAPB",
            "TESTPLANCAPC",
            "TESTPLANFASTA",
            "TESTPLANFASTB",
            "TESTPLANFASTC",
            "TESTPLANFASTD",
        ] {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = $1")
                .bind(uid)
                .execute(&pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANCAP-%'")
            .execute(&pool)
            .await
            .ok();
    }
}
