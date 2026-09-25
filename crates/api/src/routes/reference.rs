//! `/public/stations`, `/public/tocs`: type-ahead search over reference
//! data. Unauthenticated, read-only — same `public_router()` pattern as
//! `lines.rs`. See
//! docs/superpowers/specs/2026-07-11-operator-station-autocomplete-design.md.
//!
//! Also hosts `/public/stations/{crs}/accessibility`, a single-station read
//! of the curated facilities/accessibility slice of `stations.accessibility`
//! — the same `stations` table this module already owns every read of, so
//! it lives here rather than in a new sibling of `station_stats.rs` (which
//! is a derived computation over a different table). See
//! docs/superpowers/specs/2026-09-12-station-accessibility-design.md.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::app::{App, Router};
use crate::data::reference::{self, NearbyStation, Suggestion};

/// Caps how many rows a single type-ahead request can return. 20 is
/// plenty for a dropdown the user is actively narrowing by typing more.
const SUGGESTION_LIMIT: i64 = 20;

/// Default row count for `/stations/nearby` when the caller doesn't supply
/// `limit`. Deliberately much smaller than [`SUGGESTION_LIMIT`]: these are
/// physical distances a person might actually travel to, not a type-ahead
/// dropdown, so a short list of the genuinely-nearest handful reads better
/// than a long one.
const NEARBY_DEFAULT_LIMIT: i64 = 8;

/// Upper bound on `limit` for `/stations/nearby`, to keep an abusive
/// caller-supplied value from turning the full-table Haversine scan into a
/// full-table response.
const NEARBY_MAX_LIMIT: i64 = 50;

pub fn router() -> Router {
    Router::new()
        .route("/stations", axum::routing::get(search_stations))
        .route(
            "/stations/{crs}/accessibility",
            axum::routing::get(get_station_accessibility),
        )
        .route("/stations/nearby", axum::routing::get(get_nearby_stations))
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

#[derive(Debug, Deserialize)]
struct NearbyQuery {
    lat: Option<String>,
    lon: Option<String>,
    limit: Option<String>,
}

/// `GET /public/stations/nearby?lat=&lon=&limit=` -- nearest stations to a
/// point, for the frontend's "near me" feature. Unauthenticated and
/// read-only, same posture as `search_stations`/`search_tocs` above.
///
/// `lat`/`lon` are required and validated as plausible coordinates
/// (`normalize_coordinate`); a missing or invalid value is a `400` naming
/// the field, matching `trains.rs`'s "malformed input 400s, it is never
/// silently ignored" posture rather than this module's own
/// empty-query-means-empty-results convention for the type-ahead routes
/// (that convention exists because an empty search box is an expected,
/// frequent state; a "near me" request with no coordinates at all is a
/// caller bug, not a normal empty state).
async fn get_nearby_stations(
    State(app): State<App>,
    Query(query): Query<NearbyQuery>,
) -> Result<Json<Vec<NearbyStation>>, (StatusCode, String)> {
    let Some(lat_raw) = query.lat.as_deref() else {
        return Err((StatusCode::BAD_REQUEST, "lat is required".to_string()));
    };
    let Some(lon_raw) = query.lon.as_deref() else {
        return Err((StatusCode::BAD_REQUEST, "lon is required".to_string()));
    };
    let lat = normalize_coordinate("lat", lat_raw, -90.0, 90.0)?;
    let lon = normalize_coordinate("lon", lon_raw, -180.0, 180.0)?;
    let limit = normalize_nearby_limit(query.limit.as_deref())?;

    let results = reference::nearest_stations(&app.database, lat, lon, limit)
        .await
        .map_err(internal_error)?;
    Ok(Json(results))
}

/// Parses `raw` as a finite `f64` within `[min, max]`; anything else
/// (unparseable, `NaN`/infinite, or out of the plausible coordinate range)
/// is a `400` naming the field, matching `trains.rs::normalize_crs`/
/// `normalize_time`'s "malformed input 400s" convention.
fn normalize_coordinate(
    label: &str,
    raw: &str,
    min: f64,
    max: f64,
) -> Result<f64, (StatusCode, String)> {
    let trimmed = raw.trim();
    let parsed: f64 = trimmed
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, format!("{label} must be a number")))?;
    if !parsed.is_finite() || parsed < min || parsed > max {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be between {min} and {max}"),
        ));
    }
    Ok(parsed)
}

/// Parses and bounds `limit` for `/stations/nearby`, mirroring
/// `trains.rs::normalize_limit`'s shape: absent means the default, zero/
/// negative/unparseable is a `400`, and anything above
/// [`NEARBY_MAX_LIMIT`] is silently capped rather than rejected (an
/// over-large limit isn't malformed input, just one this route won't fully
/// honour).
fn normalize_nearby_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(NEARBY_DEFAULT_LIMIT);
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
    Ok(parsed.min(NEARBY_MAX_LIMIT))
}

async fn list_all_tocs(
    State(app): State<App>,
) -> Result<Json<Vec<Suggestion>>, (StatusCode, String)> {
    let results = reference::get_all_tocs(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(results))
}

/// `GET /public/stations/{crs}/accessibility` -- see
/// docs/superpowers/specs/2026-09-12-station-accessibility-design.md
/// Decision 4. The response body **is** the filtered object; no hand-built
/// `json!()` reshaping is needed because every value forwarded is already
/// the RDM feed's own camelCase JSON, untouched -- unlike
/// `station_stats.rs`, there is no nested `common` struct being embedded
/// that could hit the camelCase/snake_case pitfall
/// `crates/api/src/routes/incidents.rs` documents.
///
/// `404` and `200 {}` are deliberately different answers: the former means
/// this app has no `stations` row for `crs` at all, the latter that the row
/// exists but published none of the twelve allowlisted facility keys.
async fn get_station_accessibility(
    State(app): State<App>,
    Path(crs): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match reference::station_accessibility(&app.database, &crs)
        .await
        .map_err(internal_error)?
    {
        Some(data) => Ok(Json(data)),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("no station reference data for: {crs}"),
        )),
    }
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "reference read failed");
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
    fn normalize_coordinate_accepts_a_value_within_range() {
        assert_eq!(normalize_coordinate("lat", "51.5", -90.0, 90.0), Ok(51.5));
        assert_eq!(
            normalize_coordinate("lon", "-0.14", -180.0, 180.0),
            Ok(-0.14)
        );
    }

    #[test]
    fn normalize_coordinate_trims_whitespace() {
        assert_eq!(
            normalize_coordinate("lat", "  51.5  ", -90.0, 90.0),
            Ok(51.5)
        );
    }

    #[test]
    fn normalize_coordinate_rejects_unparseable_input() {
        let (status, body) = normalize_coordinate("lat", "nope", -90.0, 90.0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            body.contains("lat"),
            "400 body should name the field: {body}"
        );
    }

    #[test]
    fn normalize_coordinate_rejects_out_of_range_latitude() {
        let (status, _) = normalize_coordinate("lat", "91", -90.0, 90.0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = normalize_coordinate("lat", "-91", -90.0, 90.0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn normalize_coordinate_rejects_out_of_range_longitude() {
        let (status, _) = normalize_coordinate("lon", "181", -180.0, 180.0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = normalize_coordinate("lon", "-181", -180.0, 180.0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn normalize_coordinate_rejects_non_finite_values() {
        let (status, _) = normalize_coordinate("lat", "NaN", -90.0, 90.0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = normalize_coordinate("lat", "inf", -90.0, 90.0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn normalize_nearby_limit_defaults_when_absent() {
        assert_eq!(normalize_nearby_limit(None), Ok(NEARBY_DEFAULT_LIMIT));
        assert_eq!(normalize_nearby_limit(Some("")), Ok(NEARBY_DEFAULT_LIMIT));
        assert_eq!(normalize_nearby_limit(Some("  ")), Ok(NEARBY_DEFAULT_LIMIT));
    }

    #[test]
    fn normalize_nearby_limit_passes_through_a_valid_value() {
        assert_eq!(normalize_nearby_limit(Some("3")), Ok(3));
    }

    #[test]
    fn normalize_nearby_limit_caps_at_the_maximum_instead_of_rejecting() {
        assert_eq!(normalize_nearby_limit(Some("1000")), Ok(NEARBY_MAX_LIMIT));
    }

    #[test]
    fn normalize_nearby_limit_rejects_zero_negative_or_unparseable() {
        assert_eq!(
            normalize_nearby_limit(Some("0")).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            normalize_nearby_limit(Some("-5")).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            normalize_nearby_limit(Some("nope")).unwrap_err().0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn sanitize_query_passes_through_non_whitespace_unchanged() {
        assert_eq!(sanitize_query("SW"), Some("SW"));
    }

    /// `/stations/{crs}/accessibility` sits one segment under the existing
    /// literal `/stations` route, and `matchit` panics at insert time on an
    /// overlapping path or on two routes using different parameter names at
    /// the same position (this crate's other `{crs}` routes --
    /// `station_stats.rs`, `departures.rs` -- all agree on that name).
    /// Building the router is therefore the whole assertion; the behavioural
    /// half lives in `db_tests` below, which ordinary `cargo test -p api`
    /// skips for want of a database. This one always runs.
    #[test]
    fn router_builds_without_a_route_conflict() {
        let _router: Router = router();
        // `public_router()` merges this module's routes with
        // `station_stats.rs`'s and `departures.rs`'s, which own the other
        // `/stations/{crs}/…` paths -- a cross-module conflict surfaces
        // there, not in `router()` alone, and is otherwise only covered by
        // database-gated tests.
        let _merged: Router = crate::routes::public_router();
    }
}

/// HTTP-layer tests for `/stations/{crs}/accessibility` exercised against a
/// live database -- mirrors `routes::station_stats::db_tests`' seed/assert/
/// delete pattern and its `test_app` helper for constructing a real `App`
/// around a live pool without needing every other part of
/// `AppState::init` (OIDC discovery, etc). Uses the reserved `Z…` fixture
/// CRS namespace (same convention as `data::reference::db_tests`) so
/// cleanup can never touch real data.
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

    /// Copied from `routes::station_stats::db_tests::test_app` (that
    /// module's own doc comment: colocated per-file rather than shared).
    /// Every field an inert placeholder except `database`, which the caller
    /// supplies -- this route touches nothing else on `App`.
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
            // Built from the same catalogue the real `AppState::init`
            // builds it from, so a test never gets a matcher that
            // disagrees with its own `config.lines`.
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

    async fn get(pool: &PgPool, uri: &str) -> (StatusCode, String) {
        let app: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    async fn seed(pool: &PgPool, crs: &str, name: &str, accessibility: serde_json::Value) {
        sqlx::query(
            "INSERT INTO stations (crs, name, accessibility) VALUES ($1, $2, $3) \
             ON CONFLICT (crs) DO UPDATE SET accessibility = EXCLUDED.accessibility",
        )
        .bind(crs)
        .bind(name)
        .bind(accessibility)
        .execute(pool)
        .await
        .expect("seed fixture station");
    }

    async fn delete_fixture(pool: &PgPool, crs: &str) {
        sqlx::query("DELETE FROM stations WHERE crs = $1")
            .bind(crs)
            .execute(pool)
            .await
            .expect("cleanup fixture station");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                station_accessibility_route -- --ignored --test-threads=1`"]
    async fn station_accessibility_route_404s_naming_the_crs_when_no_row_exists() {
        let pool = connect().await;
        delete_fixture(&pool, "ZFC").await;

        let (status, body) = get(&pool, "/stations/ZFC/accessibility").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.contains("ZFC"), "404 body should name the CRS: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                station_accessibility_route -- --ignored --test-threads=1`"]
    async fn station_accessibility_route_is_200_empty_object_when_the_row_has_no_allowlisted_keys()
    {
        let pool = connect().await;
        seed(
            &pool,
            "ZFD",
            "Fixture Quiet Station",
            serde_json::json!({ "ticketBuying": { "open": true } }),
        )
        .await;

        let (status, body) = get(&pool, "/stations/ZFD/accessibility").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "a row that exists but published nothing is 200 {{}}, not 404 -- the two absences \
             stay separately reachable: {body}"
        );
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json, serde_json::json!({}));

        delete_fixture(&pool, "ZFD").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                station_accessibility_route -- --ignored --test-threads=1`"]
    async fn station_accessibility_route_returns_the_filtered_object_unwrapped_verbatim() {
        let pool = connect().await;
        seed(
            &pool,
            "ZFE",
            "Fixture Facilities Station Two",
            serde_json::json!({
                "lifts": [{ "location": "Platform 1" }],
                "transportLinks": ["Bus", "Underground"],
                "stationMap": "https://example.invalid/map.png"
            }),
        )
        .await;

        let (status, body) = get(&pool, "/stations/ZFE/accessibility").await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "lifts": [{ "location": "Platform 1" }],
                "transportLinks": ["Bus", "Underground"]
            }),
            "the body is the filtered object directly, not wrapped in an envelope, and \
             stationMap (non-allowlisted) is absent: {json}"
        );

        delete_fixture(&pool, "ZFE").await;
    }

    /// The new route sits at `/stations/{crs}/accessibility`, one segment
    /// deeper than the pre-existing `/stations` type-ahead search. Asserted
    /// explicitly because adding a path-parameter route under an existing
    /// literal one is exactly the change that can shadow it in a router.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                station_accessibility_route -- --ignored --test-threads=1`"]
    async fn station_accessibility_route_does_not_shadow_the_stations_search_route() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/stations?q=zzzzzzzznomatch").await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json, serde_json::json!([]));
    }

    async fn seed_with_coords(pool: &PgPool, crs: &str, name: &str, lat: f64, lon: f64) {
        sqlx::query(
            "INSERT INTO stations (crs, name, latitude, longitude) VALUES ($1, $2, $3, $4) \
             ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name, \
             latitude = EXCLUDED.latitude, longitude = EXCLUDED.longitude",
        )
        .bind(crs)
        .bind(name)
        .bind(lat)
        .bind(lon)
        .execute(pool)
        .await
        .expect("seed fixture station with coordinates");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                nearby_stations_route -- --ignored --test-threads=1`"]
    async fn nearby_stations_route_returns_nearest_first() {
        let pool = connect().await;
        seed_with_coords(&pool, "ZGA", "Near Route Fixture", 51.3200, -0.5600).await;
        seed_with_coords(&pool, "ZGB", "Far Route Fixture", 52.4800, -1.9000).await;

        let (status, body) = get(&pool, "/stations/nearby?lat=51.3191&lon=-0.5610").await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        let codes: Vec<&str> = json
            .iter()
            .filter_map(|v| v.get("code").and_then(|c| c.as_str()))
            .filter(|c| *c == "ZGA" || *c == "ZGB")
            .collect();
        assert_eq!(codes, vec!["ZGA", "ZGB"], "nearest must come first: {body}");
        assert!(
            json.iter().all(|v| v.get("distanceKm").is_some()),
            "each row must carry a camelCase distanceKm: {body}"
        );

        delete_fixture(&pool, "ZGA").await;
        delete_fixture(&pool, "ZGB").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                nearby_stations_route -- --ignored --test-threads=1`"]
    async fn nearby_stations_route_excludes_stations_with_no_coordinates() {
        let pool = connect().await;
        seed(
            &pool,
            "ZGC",
            "No Coords Route Fixture",
            serde_json::json!({}),
        )
        .await;

        let (status, body) = get(&pool, "/stations/nearby?lat=51.3191&lon=-0.5610").await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert!(
            json.iter()
                .all(|v| v.get("code").and_then(|c| c.as_str()) != Some("ZGC")),
            "a station with null lat/lon must never appear: {body}"
        );

        delete_fixture(&pool, "ZGC").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                nearby_stations_route -- --ignored --test-threads=1`"]
    async fn nearby_stations_route_respects_and_caps_the_limit() {
        let pool = connect().await;
        for i in 0..3 {
            seed_with_coords(
                &pool,
                &format!("ZH{i}"),
                &format!("Limit Route Fixture {i}"),
                51.30 + f64::from(i) * 0.01,
                -0.50,
            )
            .await;
        }

        let (status, body) = get(&pool, "/stations/nearby?lat=51.30&lon=-0.50&limit=1").await;
        assert_eq!(status, StatusCode::OK, "body: {body}");
        let json: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(json.len(), 1, "limit=1 must return exactly 1 row: {body}");

        let (status, body) = get(&pool, "/stations/nearby?lat=51.30&lon=-0.50&limit=100000").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is capped, not rejected: {body}"
        );
        let json: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert!(
            json.len() <= NEARBY_MAX_LIMIT as usize,
            "must never exceed the cap: {body}"
        );

        for i in 0..3 {
            delete_fixture(&pool, &format!("ZH{i}")).await;
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                nearby_stations_route -- --ignored --test-threads=1`"]
    async fn nearby_stations_route_400s_on_missing_or_invalid_coordinates() {
        let pool = connect().await;

        let (status, body) = get(&pool, "/stations/nearby").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

        let (status, body) = get(&pool, "/stations/nearby?lat=51.3&lon=notanumber").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "body: {body}");

        let (status, body) = get(&pool, "/stations/nearby?lat=999&lon=-0.5").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "an out-of-range latitude must 400: {body}"
        );
    }
}
