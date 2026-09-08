//! `GET /public/trains/search` -- calling-point-first, whole-network,
//! CIF-SCHEDULE-derived train search. Backs the `/trains` listing page.
//! Generalizes the earlier destination-first search
//! (docs/superpowers/specs/2026-09-07-train-listing-page-design.md) into a
//! calling-point-first one -- see
//! docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
//!
//! Named `trains` (plural), deliberately distinct from this crate's
//! `routes::train` (singular), which serves the authenticated, per-train
//! `/Train/...` family. This module is a public, unauthenticated READ over
//! published timetable data and shares no state, auth model or types with
//! that one.
//!
//! Reads `schedule_destination_departures` directly
//! (`queries::search_schedule_calling_point_departures`) as a bounded index
//! range scan (over `schedule_destination_departures_calling_point_idx`)
//! with a keyset cursor. This is a publish-then-poll read of a table
//! `schedule-reference` writes when a CIF delivery lands -- never a
//! synchronous call into that service.
//!
//! **v1 filter set, and why it stops here.** `station` (any calling
//! point -- boarding or alighting) is required; `origin` (the schedule's
//! TRUE first calling point) and `destination` (the schedule's TRUE final
//! calling point) are both optional, independent filters, along with the
//! `from`/`to` time range. There is deliberately NO operator filter: the
//! CIF SCHEDULE feed's operator field is parsed-but-undecoded everywhere in
//! this codebase, so a CIF-derived row has no operator to filter on at all.
//! There is deliberately NO date parameter: like
//! `get_station_schedule_departures`, this is "always today, server-side".
//!
//! **This route owns the `now`-forward boundary**, which is the whole point
//! of the storage shape behind it. The publish stores the entire rail day
//! uncapped, because it fires once per CIF delivery -- roughly daily -- so
//! a publish-time filter would freeze at whatever the clock read when the
//! delivery landed. Evaluating `now` here means a search at 18:00 is
//! correct at 18:00.
//!
//! **Pagination is a keyset cursor, not an offset.** `limit` bounds one
//! page; `after` carries the last row of the previous page.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::CallingPointDepartureCursor;
use crate::render::calling_point_departure_json;

/// Page size when the caller does not ask for one.
const DEFAULT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected.
///
/// This is an unauthenticated, unmetered public route over a table with
/// ~377,000 rows per day. Without a ceiling, `?limit=1000000` is a free
/// full-table scan and a large response for any anonymous caller. 200 is
/// four default pages -- generous for any real client.
///
/// An over-large `limit` is clamped, not rejected -- the honest answer is
/// "here are 200, with a cursor for the rest." A `limit` that is zero,
/// negative or unparseable IS malformed and does 400.
///
/// That's an asymmetry with `from`, `to`, `destination`, `origin` and
/// `station`, which all 400 on a bad value instead of silently ignoring it:
/// clamping `limit` can only ever return FEWER rows than the caller asked
/// for, and it's still a safe, honest answer because there's a cursor for
/// the rest. Silently dropping a malformed filter like `destination` would
/// do the opposite -- it would return MORE rows than the caller asked for,
/// under a filter the caller thinks is still applied. That reads as a
/// broken search, not a rejected input, so those fields 400 instead of
/// clamping or ignoring.
const MAX_SEARCH_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
struct TrainSearchParams {
    /// Required. A 3-letter CRS code; the search is keyed on ANY station a
    /// train calls at -- boarding or alighting, including where it starts
    /// or ends -- not just where it departs from or terminates.
    station: String,
    /// Optional. Filters to schedules whose TRUE origin (their first
    /// calling point) is this CRS -- NOT "any calling point along the
    /// route", which is what `station` above already answers. See
    /// `schedule_query::DestinationDeparture`'s own doc comment for the
    /// `origin_crs`-vs-`true_origin_crs` distinction this filters on.
    origin: Option<String>,
    /// Optional. Filters to schedules whose TRUE destination (their final
    /// calling point) is this CRS.
    destination: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on scheduled departure.
    /// Narrows the `now`-forward window; it can never widen it backwards.
    from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound.
    to: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on the time the train
    /// ARRIVES at `destination` -- a SEPARATE filter from `from`/`to`
    /// above, which stay scoped to `station`. Requires `destination` to
    /// be set; see this route's own validation below for why an
    /// arrival-time filter with nothing named to arrive at 400s instead
    /// of being silently ignored, mirroring `MAX_SEARCH_LIMIT`'s own
    /// doc comment's reasoning for the other filters. See
    /// docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.
    destination_from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound. Same `destination`
    /// requirement as `destination_from`.
    destination_to: Option<String>,
    /// Optional page size, 1..=`MAX_SEARCH_LIMIT`, defaulting to
    /// `DEFAULT_SEARCH_LIMIT`. Values above the maximum are clamped, not
    /// rejected; zero, negative and unparseable values are a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`. See `decode_cursor`.
    after: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/trains/search", axum::routing::get(get_trains_search))
}

/// Parses a caller-supplied `"HH:MM"` into a real `NaiveTime`.
fn normalize_time(label: &str, raw: &str) -> Result<chrono::NaiveTime, (StatusCode, String)> {
    chrono::NaiveTime::parse_from_str(raw, "%H:%M").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("{label} must be a time of day in HH:MM form"),
        )
    })
}

/// Validates and uppercases a CRS code.
fn normalize_crs(label: &str, raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be a 3-letter CRS code"),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Parses and bounds the page size.
fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_SEARCH_LIMIT);
    };
    let parsed: i64 = raw.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        )
    })?;
    if parsed < 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        ));
    }
    Ok(parsed.min(MAX_SEARCH_LIMIT))
}

/// Renders a keyset cursor for the wire: base64url-without-padding of
/// `"HH:MM:SS|train_uid"`. Two parts, not three: `origin_crs` (the station
/// being searched) is now the fixed equality filter for the whole query,
/// not a value that varies within one page, so it carries no ordering
/// information and doesn't belong in the cursor.
///
/// Base64 makes the value visibly OPAQUE, matching this crate's established
/// posture for other cursors in this codebase. Not signed: the cursor names
/// a public timetable row on an unauthenticated route.
fn encode_cursor(cursor: &CallingPointDepartureCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}",
        cursor.scheduled.format("%H:%M:%S"),
        cursor.train_uid
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never
/// silently ignored -- ignoring it would restart the caller at page 1
/// while their UI appended the result as page 2, duplicating every row.
fn decode_cursor(raw: &str) -> Result<CallingPointDepartureCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [scheduled, train_uid] = parts.as_slice() else {
        return Err(invalid());
    };
    let scheduled =
        chrono::NaiveTime::parse_from_str(scheduled, "%H:%M:%S").map_err(|_| invalid())?;
    Ok(CallingPointDepartureCursor {
        scheduled,
        train_uid: (*train_uid).to_string(),
    })
}

async fn get_trains_search(
    State(app): State<App>,
    Query(params): Query<TrainSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let station = normalize_crs("station", &params.station)?;
    let origin = params
        .origin
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("origin", s))
        .transpose()?;
    let destination = params
        .destination
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("destination", s))
        .transpose()?;
    let from_time = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("from", s))
        .transpose()?;
    let to_time = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("to", s))
        .transpose()?;
    let destination_from_time = params
        .destination_from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("destination_from", s))
        .transpose()?;
    let destination_to_time = params
        .destination_to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("destination_to", s))
        .transpose()?;
    // Same "malformed input 400s, it is never silently ignored" posture
    // this file's own doc comment already argues for `from`/`to`/
    // `destination`/`origin`/`station` (lines 68-76): a destination-
    // arrival filter with no destination to arrive AT is ambiguous
    // input, not a wider search, so this 400s rather than quietly acting
    // as though neither bound was set.
    if (destination_from_time.is_some() || destination_to_time.is_some()) && destination.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "destination_from and destination_to require destination to be set".to_string(),
        ));
    }
    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    // "Always today, server-side" -- no date parameter exists on this route
    // by design. `today` and `now` are deliberately read from ONE
    // `Utc::now().with_timezone(...)` call rather than two independent
    // `Utc::now()` calls -- see this same reasoning's original writeup in
    // this route's git history (baa4e75) for why two independent reads can
    // disagree about which calendar day it is around the UTC/London
    // midnight boundary during British Summer Time.
    let london_now = chrono::Utc::now().with_timezone(&chrono_tz::Europe::London);
    let today = london_now.date_naive();
    let now = london_now.time();
    let scheduled_from = match from_time {
        Some(from) => std::cmp::max(now, from),
        None => now,
    };

    let Some(page) = queries::search_schedule_calling_point_departures(
        &app.database,
        &station,
        today,
        scheduled_from,
        origin.as_deref(),
        destination.as_deref(),
        to_time,
        destination_from_time,
        destination_to_time,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error)?
    else {
        return Err((
            StatusCode::NOT_FOUND,
            "no CIF-derived schedule data has been published for today".to_string(),
        ));
    };

    Ok(Json(json!({
        "results": page
            .departures
            .iter()
            .map(|row| calling_point_departure_json(row, &station))
            .collect::<Vec<Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
    })))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "train search query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

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
        };

        std::sync::Arc::new(AppState {
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

    async fn delete_today(pool: &PgPool) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(chrono::Utc::now().date_naive())
            .execute(pool)
            .await
            .expect("cleanup today's schedule_destination_departures rows");
    }

    fn relative_times() -> (chrono::NaiveTime, chrono::NaiveTime, chrono::NaiveTime) {
        use chrono::Timelike;

        let now = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .time();
        let now = chrono::NaiveTime::from_hms_opt(now.hour(), now.minute(), 0)
            .expect("valid time from valid hour/minute");
        let past = chrono::NaiveTime::MIN;
        let (soon, soon_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(30));
        let (later, later_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(60));
        assert!(
            soon_wrapped == 0 && later_wrapped == 0 && now > past,
            "these tests need at least an hour before midnight and a moment after it; \
             re-run outside 23:00-00:01 Europe/London"
        );
        (past, soon, later)
    }

    /// Seeds today with rows all sharing ONE `origin_crs` (`station_crs`,
    /// the new required search key) but varying `destination_crs` and
    /// `true_origin_crs`, so the two optional filters can be exercised
    /// independently of the fixed station. One already-departed row (which
    /// the route must hide), and two future rows.
    async fn seed_today(pool: &PgPool, station_crs: &str) {
        delete_today(pool).await;
        let today = chrono::Utc::now().date_naive();
        let (past, soon, later) = relative_times();
        for (scheduled, train_uid, destination_crs, true_origin_crs) in [
            (past, "C10000", "WAT", Some("PAD")),
            (soon, "C10001", "WAT", Some("PAD")),
            (later, "C10002", "BRI", Some("SWA")),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(today)
            .bind(destination_crs)
            .bind(scheduled)
            .bind(train_uid)
            .bind(station_crs)
            .bind(true_origin_crs)
            .execute(pool)
            .await
            .expect("seed fixture row");
        }
    }

    async fn get(pool: &PgPool, uri: &str) -> (StatusCode, String) {
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = router
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    fn results(body: &str) -> Vec<Value> {
        let json: Value = serde_json::from_str(body).unwrap();
        assert!(
            json.is_object() && json.get("results").is_some() && json.get("nextCursor").is_some(),
            "the body is an envelope with exactly `results` and `nextCursor`: {json}"
        );
        json["results"].as_array().cloned().unwrap()
    }

    fn next_cursor(body: &str) -> Option<String> {
        let json: Value = serde_json::from_str(body).unwrap();
        json["nextCursor"].as_str().map(str::to_string)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_missing_station_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "station is required -- axum's Query extractor rejects the missing field"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_station_is_a_400_not_a_404() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=NOTACRS").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("station"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_time_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&from=half+past+eight").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("from"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_destination_from_without_destination_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&destination_from=09:00").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("destination"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_destination_to_without_destination_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search?station=ZRB&destination_to=09:00").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_destination_from_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(
            &pool,
            "/trains/search?station=ZRB&destination=WAT&destination_from=teatime",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("destination_from"),
            "400 body should name the field: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_after_cursor_is_a_400() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?station=ZRB&after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("after"), "400 body should name the field: {body}");

        let (status, _) = get(&pool, "/trains/search?station=ZRB&after=bm9uc2Vuc2U").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, _) = get(&pool, "/trains/search?station=ZRB&limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, "/trains/search?station=ZRB&limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("limit"), "400 body should name the field: {body}");

        let (status, body) = get(&pool, "/trains/search?station=ZRB&limit=99999").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is clamped to MAX_SEARCH_LIMIT, never rejected"
        );
        assert_eq!(results(&body).len(), 2, "the fixture only has two future rows");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_nothing_published_for_today_is_a_404() {
        let pool = connect().await;
        delete_today(&pool).await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains("today"),
            "the 404 is about today's publish, not about the station: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_unknown_station_on_a_published_day_is_200_and_empty() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRF").await;
        assert_eq!(status, StatusCode::OK);
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_published_day_with_no_matches_is_200_with_an_empty_results_array() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB&origin=ZZZ").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "published-but-unmatched is a 200 with an empty results array, never a 404"
        );
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_renders_camel_case_rows_with_trimmed_time_and_station_attached() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=zrb").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let (_, soon, _) = relative_times();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["uid"], "C10001");
        assert_eq!(
            rows[0]["scheduled"],
            soon.format("%H:%M").to_string(),
            "seconds trimmed"
        );
        assert_eq!(
            rows[0]["stationCrs"], "ZRB",
            "the lowercase query param is normalized and re-attached uppercase"
        );
        assert_eq!(rows[0]["originCrs"], "PAD");
        assert_eq!(rows[0]["destinationCrs"], "WAT");
        assert!(rows[0].get("origin_crs").is_none(), "no stray snake_case field");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_that_has_already_gone() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let uids: Vec<&str> = rows.iter().map(|r| r["uid"].as_str().unwrap()).collect();
        assert!(
            !uids.contains(&"C10000"),
            "an already-departed row must not be returned: {uids:?}"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_applies_origin_destination_and_time_filters_together() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (_, soon, later) = relative_times();
        let uri = format!(
            "/trains/search?station=ZRB&origin=SWA&destination=BRI&from={}&to={}",
            soon.format("%H:%M"),
            later.format("%H:%M")
        );
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "C10002");
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_filters_by_destination_arrival_independent_of_from_to() {
        let pool = connect().await;
        delete_today(&pool).await;
        let today = chrono::Utc::now().date_naive();
        let (_, soon, later) = relative_times();
        // Both rows share the SAME scheduled (station) time, `soon` -- so
        // only destination_from/destination_to, not from/to, can tell them
        // apart. Their destination_arrival values reuse `soon`/`later`
        // themselves (rather than adding further offsets) so this stays
        // inside relative_times()'s own "at least an hour before midnight"
        // guarantee.
        for (train_uid, destination_arrival) in [("C90001", soon), ("C90002", later)] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, destination_arrival) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7)",
            )
            .bind(today)
            .bind("WAT")
            .bind(soon)
            .bind(train_uid)
            .bind("ZRB")
            .bind(Option::<&str>::None)
            .bind(destination_arrival)
            .execute(&pool)
            .await
            .expect("seed fixture row");
        }

        let uri = format!(
            "/trains/search?station=ZRB&destination=WAT&destination_from={}&destination_to={}",
            later.format("%H:%M"),
            later.format("%H:%M"),
        );
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "C90002");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_origin_and_destination_are_independent_of_each_other() {
        // The load-bearing new-behavior test: origin=PAD alone must match
        // BOTH rows below, even though they go to different destinations --
        // proving `origin` doesn't silently also constrain `destination`.
        // `seed_today` only has one future PAD-origin row, so a fixture
        // built from it can't discriminate this: a one-row, one-destination
        // result is equally consistent with `origin` secretly also fixing
        // the destination. This test therefore seeds its own two-row,
        // same-origin/different-destination fixture inline, rather than
        // extending `seed_today` and disturbing the exact future-row counts
        // several other tests assert against that shared fixture.
        let pool = connect().await;
        delete_today(&pool).await;
        let today = chrono::Utc::now().date_naive();
        let (_, soon, later) = relative_times();
        for (scheduled, train_uid, destination_crs) in
            [(soon, "C20001", "WAT"), (later, "C20002", "BRI")]
        {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(today)
            .bind(destination_crs)
            .bind(scheduled)
            .bind(train_uid)
            .bind("ZRB")
            .bind(Some("PAD"))
            .execute(&pool)
            .await
            .expect("seed fixture row");
        }

        let (status, body) = get(&pool, "/trains/search?station=ZRB&origin=PAD").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(
            rows.len(),
            2,
            "both rows true-originate at PAD despite different destinations: {rows:?}"
        );
        let uids: std::collections::BTreeSet<&str> =
            rows.iter().map(|row| row["uid"].as_str().unwrap()).collect();
        assert_eq!(
            uids,
            std::collections::BTreeSet::from(["C20001", "C20002"]),
            "both PAD-origin rows must come back regardless of their differing destinations"
        );
        let destinations: std::collections::BTreeSet<&str> = rows
            .iter()
            .map(|row| row["destinationCrs"].as_str().unwrap())
            .collect();
        assert_eq!(
            destinations,
            std::collections::BTreeSet::from(["WAT", "BRI"]),
            "origin=PAD must not silently also constrain destination"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&body).len(), 2);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_paginates_with_a_cursor_and_after_continues_from_it() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, first) = get(&pool, "/trains/search?station=ZRB&limit=1").await;
        assert_eq!(status, StatusCode::OK);
        let first_rows = results(&first);
        assert_eq!(first_rows.len(), 1);
        assert_eq!(first_rows[0]["uid"], "C10001");
        let cursor = next_cursor(&first).expect("a second page exists, so a cursor is returned");

        let (status, second) = get(
            &pool,
            &format!("/trains/search?station=ZRB&limit=1&after={cursor}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let second_rows = results(&second);
        assert_eq!(second_rows.len(), 1);
        assert_eq!(
            second_rows[0]["uid"], "C10002",
            "`after` must continue from the cursor, not restart at page 1"
        );
        assert!(
            next_cursor(&second).is_none(),
            "the last page must not hand back a cursor"
        );

        delete_today(&pool).await;
    }

    fn london_is_currently_ahead_of_utc() -> bool {
        use chrono::Offset;

        chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .offset()
            .fix()
            .local_minus_utc()
            != 0
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_inside_the_utc_vs_london_gap() {
        let pool = connect().await;
        delete_today(&pool).await;

        let is_bst = london_is_currently_ahead_of_utc();
        let today = chrono::Utc::now().date_naive();
        let london_time = {
            use chrono::Timelike;

            let t = chrono::Utc::now()
                .with_timezone(&chrono_tz::Europe::London)
                .time();
            chrono::NaiveTime::from_hms_opt(t.hour(), t.minute(), 0)
                .expect("valid time from valid hour/minute")
        };
        let (gap_time, wrapped) =
            london_time.overflowing_sub_signed(chrono::Duration::minutes(20));
        assert!(
            wrapped == 0 && london_time >= chrono::NaiveTime::from_hms_opt(0, 20, 0).unwrap(),
            "this test needs at least 20 minutes since London midnight; re-run outside \
             00:00-00:20 Europe/London"
        );

        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(today)
        .bind("WAT")
        .bind(gap_time)
        .bind("C10099")
        .bind("ZRB")
        .bind(Option::<&str>::None)
        .execute(&pool)
        .await
        .expect("seed the gap-row fixture");

        let (status, body) = get(&pool, "/trains/search?station=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<String> = results(&body)
            .iter()
            .map(|row| row["uid"].as_str().unwrap().to_string())
            .collect();

        if is_bst {
            assert!(
                !uids.contains(&"C10099".to_string()),
                "during BST, a row scheduled 20 minutes before the correct London-local `now` \
                 has already departed and must be excluded; if this fails, `now` has regressed \
                 to bare UTC time: {uids:?}"
            );
        } else {
            // Not currently observing BST: `Europe::London` and UTC agree
            // outside BST, so there is no gap between the two clocks to pin
            // this fixture inside -- reverting the route to bare UTC would
            // compute the exact same `now` this test just ran against, and
            // the assertion above would pass either way. There is nothing
            // this test COULD discriminate in that window, so skipping the
            // core assertion here is a true no-op, not a flaky pass/fail or
            // a silently-lost coverage gap.
        }

        delete_today(&pool).await;
    }
}
