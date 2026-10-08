//! `/public/freshness`: how fresh the six data sources feeding the status
//! API are (stations reference data, TOC reference data, the raw incidents
//! feed, the `TfL` line-status feed, the CIF SCHEDULE feed pushed by
//! `schedule-ingest`, and the Network Rail CORPUS extract that
//! `schedule-ingest` loads through `/private/corpus-locations`). Unauthenticated, read-only — same `public_router()`
//! pattern as `reference.rs`. Reads the same values as the `last_*_fetch`
//! queries the private poller-startup endpoints call
//! (`crates/api/src/routes/ingest.rs`), but in one query
//! (`queries::data_freshness`) -- this is a public read of the same
//! underlying data, just aimed at the frontend instead of poller backoff.
//! Station-samples is deliberately omitted: it's per-station polling data,
//! not one of the six sources this endpoint reports on.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new().route("/freshness", axum::routing::get(get_freshness))
}

#[derive(Debug, Serialize, PartialEq)]
pub struct DataFreshness {
    pub stations: Option<DateTime<Utc>>,
    pub tocs: Option<DateTime<Utc>>,
    pub incidents: Option<DateTime<Utc>>,
    /// When `TfL` line status last landed. Unlike its three siblings this is
    /// not a poller-fed raw table but the `computed_at` of the TfL-owned
    /// `line_status` rows themselves — for this source, ingest and
    /// computation are the same event.
    pub tfl: Option<DateTime<Utc>>,
    /// When a CIF SCHEDULE feed delivery was last recorded by
    /// `schedule-ingest`'s push to `/private/schedule-feed-ingests`. Its own
    /// new source, not previously reported by this endpoint.
    pub schedule_feed: Option<DateTime<Utc>>,
    /// The `delivered_at` (the delivered file's own mtime) of the newest
    /// Network Rail CORPUS extract loaded into `corpus_locations`
    /// (`MAX(corpus_deliveries.delivered_at)`, the same read as
    /// `data::corpus::last_corpus_delivery`). `None` until the first load,
    /// and for as long as `scheduleFeed.corpus.enabled` stays off.
    pub corpus: Option<DateTime<Utc>>,
}

async fn get_freshness(
    State(app): State<App>,
) -> Result<Json<DataFreshness>, (StatusCode, String)> {
    let [stations, tocs, incidents, tfl, schedule_feed, corpus] =
        queries::data_freshness(&app.database)
            .await
            .map_err(internal_error)?;
    Ok(Json(DataFreshness {
        stations,
        tocs,
        incidents,
        tfl,
        schedule_feed,
        corpus,
    }))
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "used as a map_err callback, which passes the error by value"
)]
fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    if let Some(unavailable) = crate::unavailable::response_for(&err) {
        return unavailable;
    }
    tracing::error!(error = ?err, "data freshness query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn serializes_missing_data_as_null() {
        let freshness = DataFreshness {
            stations: None,
            tocs: None,
            incidents: None,
            tfl: None,
            schedule_feed: None,
            corpus: None,
        };
        let json = serde_json::to_value(&freshness).unwrap();
        assert!(json["stations"].is_null());
        assert!(json["tocs"].is_null());
        assert!(json["incidents"].is_null());
        assert!(json["tfl"].is_null());
        assert!(json["schedule_feed"].is_null());
        assert!(json["corpus"].is_null());
    }

    #[test]
    fn round_trips_a_present_timestamp() {
        let ts = Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap();
        let freshness = DataFreshness {
            stations: Some(ts),
            tocs: None,
            incidents: None,
            tfl: None,
            schedule_feed: Some(ts),
            corpus: Some(ts),
        };
        let json = serde_json::to_value(&freshness).unwrap();
        let roundtripped: DateTime<Utc> = json["stations"].as_str().unwrap().parse().unwrap();
        assert_eq!(roundtripped, ts);
        let schedule_feed_roundtripped: DateTime<Utc> =
            json["schedule_feed"].as_str().unwrap().parse().unwrap();
        assert_eq!(schedule_feed_roundtripped, ts);
        let corpus_roundtripped: DateTime<Utc> = json["corpus"].as_str().unwrap().parse().unwrap();
        assert_eq!(corpus_roundtripped, ts);
    }
}

/// Database-gated, one throwaway database per test (`#[sqlx::test]`): the
/// CORPUS case loads through `replace_corpus_locations`, which replaces the
/// WHOLE `corpus_locations` table, so it must never share a database that
/// may hold a real extract.
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::Request;
    use chrono::TimeZone;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use super::*;
    use crate::app::{App, AppState};
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::corpus::{CorpusLocation, replace_corpus_locations};

    /// Copied from `routes::reference::db_tests::test_app` (colocated
    /// per-file, like its siblings). Every field an inert placeholder except
    /// `database` -- this route touches nothing else on `App`.
    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            migration_database_url: None,
            redis_url: "redis://127.0.0.1:0".to_string(),
            redis_password: None,
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
            internal_oauth_group_corpus: "svc-corpus-ingest".to_string(),
            internal_oauth_group_mcp: "srv-ds-mcp".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
            chatbot_access: crate::data::config::ChatbotAccessMode::Group,
            admin_group: String::new(),
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
            metrics_port: 9091,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
            background_loops: true,
            incidents_row_heartbeat: true,
            past_travel_retention_days: 548,
            stale_push_subscription_days: 365,
            inactive_account_retention_days: 0,
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

    async fn get_freshness_json(pool: &PgPool) -> serde_json::Value {
        let app: axum::Router = Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/freshness")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    #[sqlx::test(migrations = "../ds-store/migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn corpus_is_null_before_any_delivery(pool: PgPool) {
        let json = get_freshness_json(&pool).await;
        assert!(
            json.get("corpus").is_some(),
            "corpus key must always be present: {json}"
        );
        assert!(json["corpus"].is_null(), "no delivery yet: {json}");
    }

    #[sqlx::test(migrations = "../ds-store/migrations")]
    #[ignore = "needs DATABASE_URL (a role that can create databases)"]
    async fn corpus_reports_the_newest_delivery(pool: PgPool) {
        let older = Utc.with_ymd_and_hms(2026, 8, 1, 3, 0, 0).unwrap();
        let newer = Utc.with_ymd_and_hms(2026, 9, 1, 3, 0, 0).unwrap();
        let location = CorpusLocation {
            nlc: "559500".to_string(),
            stanox: Some("87219".to_string()),
            tiploc: Some("CLPHMJN".to_string()),
            crs: Some("CLJ".to_string()),
            uic: None,
            nlc_desc: Some("CLAPHAM JUNCTION LONDON".to_string()),
            nlc_desc16: None,
        };
        // Loaded newest first: the field is the newest delivery, not the
        // most recent load.
        for delivered_at in [newer, older] {
            replace_corpus_locations(
                &pool,
                delivered_at,
                "CORPUSExtract.json.gz",
                std::slice::from_ref(&location),
            )
            .await
            .unwrap();
        }

        let json = get_freshness_json(&pool).await;
        let corpus: DateTime<Utc> = json["corpus"].as_str().unwrap().parse().unwrap();
        assert_eq!(corpus, newer);
        assert_eq!(
            crate::data::corpus::refresh_last_delivery_metric(&pool)
                .await
                .unwrap(),
            Some(newer)
        );
    }
}
