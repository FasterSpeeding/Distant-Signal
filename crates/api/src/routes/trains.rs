//! `GET /public/trains/search` -- destination-first, whole-network,
//! CIF-SCHEDULE-derived train search. Backs the `/trains` listing page
//! (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
//! Approach B).
//!
//! Named `trains` (plural), deliberately distinct from this crate's
//! `routes::train` (singular), which serves the authenticated, per-train
//! `/Train/...` family. This module is a public, unauthenticated READ over
//! published timetable data and shares no state, auth model or types with
//! that one.
//!
//! Reads `schedule_destination_departures` directly
//! (`queries::search_schedule_destination_departures`) as a bounded index
//! range scan with a keyset cursor. This is a publish-then-poll read of a
//! table `schedule-reference` writes when a CIF delivery lands -- never a
//! synchronous call into that service, per the design doc's §6.
//!
//! **v1 filter set, and why it stops here.** `destination` is required;
//! `origin` and the `from`/`to` time range are optional. There is
//! deliberately NO operator filter: the CIF SCHEDULE feed's operator field
//! is parsed-but-undecoded everywhere in this codebase, so a CIF-derived row
//! has no operator to filter on at all (design doc §1.3/§6). There is
//! deliberately NO date parameter: like `get_station_schedule_departures`,
//! this is "always today, server-side" (design doc §6).
//!
//! **This route owns the `now`-forward boundary**, which is the whole point
//! of the storage shape behind it. The publish stores the entire rail day
//! uncapped (`crates/schedule-reference`'s
//! `publish_schedule_destination_departures` passes `NaiveTime::MIN`),
//! because it fires once per CIF delivery -- roughly daily -- so a
//! publish-time filter would freeze at whatever the clock read when the
//! delivery landed. Evaluating `now` here means a search at 18:00 is
//! correct at 18:00. See
//! docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
//! §1.3 and §3.
//!
//! **Pagination is a keyset cursor, not an offset.** `limit` bounds one
//! page; `after` carries the last row of the previous page. Both exist
//! because there is no cap anywhere else in this pipeline: a busy
//! destination genuinely has thousands of trains in a day, and the honest
//! way to show them is a page at a time rather than a silent truncation.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::DestinationDepartureCursor;
use crate::render::destination_departure_json;

/// Page size when the caller does not ask for one.
///
/// 50 rather than the old flat 100: this is now a PAGE, not the whole
/// answer, so it is sized for a first screenful plus room to scroll, and
/// "Load more" covers the rest at no extra cost (the next page is another
/// `LIMIT`-bounded index range scan, not a re-scan).
const DEFAULT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected.
///
/// **Why a ceiling exists at all,** now that nothing else in this pipeline
/// caps anything: this is an unauthenticated, unmetered public route over a
/// table with ~377,000 rows per day. Without a ceiling, `?limit=1000000`
/// is a free full-table scan and a ~20MB response for any anonymous
/// caller. 200 is four default pages -- generous for any real client,
/// including one that wants to render a whole morning at once.
///
/// **Why clamp rather than 400:** an over-large `limit` is not a malformed
/// input, it is an over-eager one, and the honest answer is "here are 200,
/// with a cursor for the rest" rather than an error. That is the opposite
/// call from `normalize_time`/`normalize_crs`, which DO 400 -- because
/// there, silently dropping a filter would return MORE rows than the caller
/// asked for, whereas clamping a limit only ever returns fewer, with a
/// cursor saying so. A `limit` that is zero, negative or unparseable IS
/// malformed and does 400.
const MAX_SEARCH_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
struct TrainSearchParams {
    /// Required. A 3-letter CRS code; the search is keyed on it.
    destination: String,
    /// Optional. Matches the calling point a train departs FROM, which for
    /// a mid-route result is an intermediate station, not the schedule's
    /// own first station -- see `schedule_query::DestinationDeparture`'s
    /// own doc comment.
    origin: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on scheduled departure.
    /// Narrows the `now`-forward window; it can never widen it backwards
    /// (a train that has already departed is not a search result).
    from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound.
    to: Option<String>,
    /// Optional page size, 1..=`MAX_SEARCH_LIMIT`, defaulting to
    /// `DEFAULT_SEARCH_LIMIT`. Values above the maximum are clamped, not
    /// rejected; zero, negative and unparseable values are a `400`.
    ///
    /// Typed `Option<String>` rather than `Option<i64>` deliberately: with
    /// `Option<i64>`, `?limit=abc` fails inside axum's `Query` extractor
    /// and produces its generic deserialization error, which names neither
    /// the field nor the expectation. Parsing it here keeps every 400 on
    /// this route self-describing, exactly as `from`/`to` already are.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`. See `decode_cursor`.
    after: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/trains/search", axum::routing::get(get_trains_search))
}

/// Parses a caller-supplied `"HH:MM"` into a real `NaiveTime`, which is
/// what the query now compares against a real `TIME` column. (Under the
/// JSONB bucket this returned a `"HH:MM:SS"` string for a lexicographic
/// comparison; there is no text comparison left to line up with.)
///
/// `Err` (a 400) rather than silently ignoring an unparseable value: a
/// dropped filter would return MORE trains than asked for, which reads as a
/// broken search rather than a rejected input.
fn normalize_time(label: &str, raw: &str) -> Result<chrono::NaiveTime, (StatusCode, String)> {
    chrono::NaiveTime::parse_from_str(raw, "%H:%M").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("{label} must be a time of day in HH:MM form"),
        )
    })
}

/// Validates and uppercases a CRS code. Rejecting rather than passing a
/// malformed value through matters here because a non-CRS `destination`
/// would otherwise be reported as an ordinary empty result, which
/// misreports a caller error as a data gap.
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

/// Parses and bounds the page size. Over-large values are CLAMPED to
/// `MAX_SEARCH_LIMIT`; zero, negative and unparseable values are a `400`.
/// See `MAX_SEARCH_LIMIT`'s own doc comment for why those two inputs are
/// treated differently.
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
/// `"HH:MM:SS|train_uid|origin_crs"`.
///
/// Base64 makes the value visibly OPAQUE. That is the point of encoding it
/// at all -- the three components are the trailing columns of
/// `schedule_destination_departures`' primary key, an internal detail that
/// must stay free to change, and a bare readable string invites a client to
/// build one by hand and depend on it. It is not a security measure and
/// deliberately is not signed: the cursor names a public timetable row on
/// an unauthenticated route, so tampering can only reposition a reader
/// within data they may already read in full.
///
/// `URL_SAFE_NO_PAD` is this crate's established engine
/// (`crates/api/src/auth.rs:122-123`), and needs no percent-encoding in a
/// query string.
fn encode_cursor(cursor: &DestinationDepartureCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}|{}",
        cursor.scheduled.format("%H:%M:%S"),
        cursor.train_uid,
        cursor.origin_crs
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never a
/// silently-ignored one: ignoring it would restart the caller at page 1
/// while their UI appended the result as page 2, duplicating every row on
/// screen. That is the same reasoning `normalize_time` gives for rejecting
/// an unparseable time rather than dropping the filter.
///
/// `train_uid` and `origin_crs` are passed through as-is rather than
/// validated further: they are compared for ordering only, so a nonsense
/// value yields an empty page rather than anything unsafe, and the query is
/// parameterized.
fn decode_cursor(raw: &str) -> Result<DestinationDepartureCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [scheduled, train_uid, origin_crs] = parts.as_slice() else {
        return Err(invalid());
    };
    let scheduled =
        chrono::NaiveTime::parse_from_str(scheduled, "%H:%M:%S").map_err(|_| invalid())?;
    Ok(DestinationDepartureCursor {
        scheduled,
        train_uid: (*train_uid).to_string(),
        origin_crs: (*origin_crs).to_string(),
    })
}

async fn get_trains_search(
    State(app): State<App>,
    Query(params): Query<TrainSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let destination = normalize_crs("destination", &params.destination)?;
    let origin = params
        .origin
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("origin", s))
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
    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    // "Always today, server-side" -- no date parameter exists on this route
    // by design (design doc §6). Same posture and same expression as
    // `routes::departures::get_station_schedule_departures`.
    let today = chrono::Utc::now().date_naive();

    // The `now`-forward boundary, evaluated HERE rather than at publish
    // time -- see this module's own doc comment. `from` can only narrow it
    // further, never reach back past it, so the effective lower bound is
    // the later of the two.
    //
    // Europe/London LOCAL time, not UTC, and that is load-bearing: the
    // stored `scheduled` values are London local civil time straight off
    // the CIF body (`schedule_query::DestinationDeparture::scheduled`'s own
    // doc comment says so explicitly), so comparing a UTC time-of-day
    // against them would be an hour wrong every British Summer Time.
    // `chrono_tz` is already a direct dependency of this crate and
    // `chrono_tz::Europe::London` is already used in
    // `crate::data::eta_blend` for the same reason -- no new dependency,
    // and no hardcoded offset.
    let now = chrono::Utc::now()
        .with_timezone(&chrono_tz::Europe::London)
        .time();
    let scheduled_from = match from_time {
        Some(from) => std::cmp::max(now, from),
        None => now,
    };

    let Some(page) = queries::search_schedule_destination_departures(
        &app.database,
        &destination,
        today,
        scheduled_from,
        origin.as_deref(),
        to_time,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error)?
    else {
        // 404 vs an empty `results` array is a real distinction here, not
        // pedantry -- but note WHICH distinction it now draws. Under the
        // flat table the probe is day-scoped, so this 404 means "no CIF
        // publish has landed for today at all", and an unknown or
        // train-less destination CRS gets a `200` with no results instead.
        // See the addendum's §3 and §7 item 3, and Task 5's Interfaces
        // block. The frontend renders different copy for each.
        return Err((
            StatusCode::NOT_FOUND,
            "no CIF-derived schedule data has been published for today".to_string(),
        ));
    };

    // An envelope, not a bare array, because a bare array has nowhere to
    // carry `nextCursor`. camelCase and hand-built with `json!()`, like
    // every other response in this crate.
    Ok(Json(json!({
        "results": page
            .departures
            .iter()
            .map(|row| destination_departure_json(row, &destination))
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

    /// Copied from `routes::departures::db_tests::test_app`, per that
    /// module's own doc comment ("colocated per-file rather than shared,
    /// until a third file needs it too"). Every field is an inert
    /// placeholder except `database`, which the caller supplies -- this
    /// route touches nothing else on `App`.
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

    /// Day-scoped, because the route's own existence probe is. Under the
    /// flat table there is no per-destination row to delete, and leaving
    /// another destination's rows behind for today would make the 404 test
    /// silently pass through to a `200`.
    async fn delete_today(pool: &PgPool) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(chrono::Utc::now().date_naive())
            .execute(pool)
            .await
            .expect("cleanup today's schedule_destination_departures rows");
    }

    /// One time in the past and two strictly in the future, relative to the
    /// route's own London-local `now`.
    ///
    /// Computed at runtime, deliberately. This route applies its
    /// `now`-forward filter at REQUEST time -- that is the entire point of
    /// the storage shape behind it -- so a fixed wall-clock fixture like
    /// "08:22" would pass in the morning and silently return nothing in the
    /// afternoon. Anything asserting on visible rows must therefore be
    /// relative.
    fn relative_times() -> (chrono::NaiveTime, chrono::NaiveTime, chrono::NaiveTime) {
        use chrono::Timelike;

        // Truncated to whole minutes. Real CIF SCHEDULE data is always an
        // exact minute (no seconds/microseconds component), and everything
        // this route round-trips through the wire is minute- or
        // second-granularity text (`from`/`to` are "HH:MM", the cursor is
        // "HH:MM:SS"). `chrono::Utc::now()` itself carries microseconds, so
        // seeding a fixture row with the untruncated value would insert
        // sub-second precision the route's own encodings can never
        // preserve -- a fixture-only mismatch (the cursor's "%H:%M:%S"
        // encoding silently drops it, then compares unequal to the
        // original row) that can never occur against real published data.
        // Truncating here keeps the fixture representative.
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

    /// Seeds today with: one already-departed row (which the route must
    /// hide), and two future rows from two different origins (which it must
    /// show, earliest first). The flat-shape successor to the original
    /// plan's two-element JSONB bucket.
    async fn seed_today(pool: &PgPool, destination_crs: &str) {
        delete_today(pool).await;
        let today = chrono::Utc::now().date_naive();
        let (past, soon, later) = relative_times();
        for (scheduled, train_uid, origin_crs) in [
            (past, "C10000", "EUS"),
            (soon, "C10001", "EUS"),
            (later, "C10002", "CRE"),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(today)
            .bind(destination_crs)
            .bind(scheduled)
            .bind(train_uid)
            .bind(origin_crs)
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

    /// `results` out of the envelope, asserting the envelope's own shape on
    /// the way through so every test that reads rows also proves the body
    /// is not a bare array.
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
    async fn trains_search_missing_destination_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "destination is required -- axum's Query extractor rejects the missing field"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_destination_is_a_400_not_a_404() {
        // The discriminating case: a caller error must not be reported as
        // a data gap.
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?destination=NOTACRS").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("destination"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_time_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&from=half+past+eight").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("from"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_after_cursor_is_a_400() {
        // A malformed cursor must NOT be silently ignored: ignoring it
        // restarts the caller at page 1 while their UI appends the result
        // as page 2, duplicating every row on screen. Two shapes are
        // checked -- not-base64 at all, and valid base64 whose payload has
        // the wrong number of parts.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?destination=ZRB&after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("after"), "400 body should name the field: {body}");

        // base64url of "nonsense" -- decodes cleanly, but is not a cursor.
        let (status, _) = get(&pool, "/trains/search?destination=ZRB&after=bm9uc2Vuc2U").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        // The asymmetry `MAX_SEARCH_LIMIT`'s doc comment argues, pinned:
        // an over-large limit is over-eager (clamp, and say so with a
        // cursor), a zero or unparseable one is malformed (400).
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, _) = get(&pool, "/trains/search?destination=ZRB&limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("limit"), "400 body should name the field: {body}");

        let (status, body) = get(&pool, "/trains/search?destination=ZRB&limit=99999").await;
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
        // NOTE the changed meaning of this 404, and the changed assertion
        // that follows from it. Under the flat table the existence probe is
        // scoped to the DAY, not the destination, so this says "no CIF
        // publish has landed for today at all" and no longer names a CRS.
        // The companion test below pins the other half of that split.
        //
        // This test needs today's table to be genuinely empty. Run it
        // against the local docker-compose database, not one a real
        // `schedule-reference` has published into.
        let pool = connect().await;
        delete_today(&pool).await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains("today"),
            "the 404 is about today's publish, not about the destination: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_unknown_destination_on_a_published_day_is_200_and_empty() {
        // The other half of the changed split, and the reason it is a
        // deliberate call rather than an accident: once today's timetable
        // IS published, "nothing goes to ZRF" is a real answer, not a
        // missing one. Do not "restore" this to a 404.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRF").await;
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
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&origin=ZZZ").await;
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
    async fn trains_search_renders_camel_case_rows_with_trimmed_time_and_the_destination_attached() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=zrb").await;
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
        assert_eq!(rows[0]["originCrs"], "EUS");
        assert_eq!(
            rows[0]["destinationCrs"], "ZRB",
            "the lowercase query param is normalized and re-attached uppercase"
        );
        assert!(rows[0].get("origin_crs").is_none(), "no stray snake_case field");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_that_has_already_gone() {
        // The `now`-forward filter, which now lives HERE rather than at
        // publish time. The fixture's 00:00 row is published and matches
        // every other predicate; it must not be returned.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB").await;
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
    async fn trains_search_applies_origin_and_time_filters_together() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (_, soon, later) = relative_times();
        let uri = format!(
            "/trains/search?destination=ZRB&origin=CRE&from={}&to={}",
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
    async fn trains_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&body).len(), 2);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted -- the frontend \
             checks it to decide whether to render Load more"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_paginates_with_a_cursor_and_after_continues_from_it() {
        // The end-to-end pagination contract Task 10's "Load more" button
        // depends on: page 1 returns a cursor, feeding that cursor back as
        // `after` returns the NEXT row (not a repeat, not a restart), and
        // the final page reports no cursor.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, first) = get(&pool, "/trains/search?destination=ZRB&limit=1").await;
        assert_eq!(status, StatusCode::OK);
        let first_rows = results(&first);
        assert_eq!(first_rows.len(), 1);
        assert_eq!(first_rows[0]["uid"], "C10001");
        let cursor = next_cursor(&first).expect("a second page exists, so a cursor is returned");

        let (status, second) = get(
            &pool,
            &format!("/trains/search?destination=ZRB&limit=1&after={cursor}"),
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
}
