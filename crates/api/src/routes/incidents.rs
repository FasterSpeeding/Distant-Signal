//! `GET /public/incidents/{incidentId}` -- a single Knowledgebase incident's
//! full detail: description, affected stations, every validity period,
//! which lines currently report it, and its own change history.
//! Unauthenticated, matching every other read in `public_router()` -- see
//! docs/superpowers/specs/2026-08-31-incident-detail-page-design.md's
//! "Public read-route convention" finding: every field this returns is
//! already fully public today via `GET /Line/{ids}/Status?detail=true`.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new()
        .route("/incidents", axum::routing::get(search_incidents))
        .route("/incidents/{incidentId}", axum::routing::get(get_incident))
}

/// `knowledgebase-incident-{incidentId}` is the ONLY provenance-string
/// format that names a real `incidents` row -- see the design spec's
/// Correction 1. Reconstructing it here (rather than storing/returning the
/// bare incident_id as `disruption.source`) is what lets
/// `lines_currently_reporting_incident` reach into `line_status.statuses`'
/// JSONB and find this exact incident.
fn knowledgebase_source(incident_id: &str) -> String {
    format!("knowledgebase-incident-{incident_id}")
}

/// Page size when the caller does not ask for one. Same numeric value as
/// `routes::trains::DEFAULT_SEARCH_LIMIT` -- no reason for this route's
/// page size to differ -- but declared as its own constant, not shared,
/// since the two routes have no reason to be coupled (Decision 4 of the
/// design spec).
const DEFAULT_INCIDENT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected --
/// same rationale and same numeric value as
/// `routes::trains::MAX_SEARCH_LIMIT`.
const MAX_INCIDENT_SEARCH_LIMIT: i64 = 200;

/// `#[serde(deny_unknown_fields)]` for the same reason
/// `trains.rs::TrainSearchParams` has it -- see this crate's established
/// posture: a misspelled filter name must 400, not silently no-op. See
/// Correction 3 of the design spec.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IncidentSearchParams {
    /// Optional. Comma-separated ATOC codes, e.g. `operator=SW,VT` --
    /// matches this codebase's existing multi-value convention
    /// (`GET /Line/{ids}/Status`'s `ids`), not repeated query keys.
    /// Matches if the incident's `operators` array overlaps this set at
    /// all (an "any of" filter).
    operator: Option<String>,
    /// Optional. A single catalogue line id (`app.config.lines`, never a
    /// custom line). Resolved server-side to that line's own station list,
    /// then applied as a station-overlap filter -- see this route's own
    /// doc comment on `search_incidents` for the named approximation this
    /// implies. An id that doesn't resolve in `app.config.lines` is a
    /// `400` ("unknown line"), not a 404 or a silently-empty result.
    line: Option<String>,
    /// Optional, RFC3339. Inclusive lower bound on `first_seen_at`.
    from: Option<String>,
    /// Optional, RFC3339. Inclusive upper bound on `first_seen_at`.
    to: Option<String>,
    /// Optional. `true` = planned works only, `false` = unplanned only,
    /// omitted = either.
    planned: Option<bool>,
    /// Optional. `true` = cleared only, `false` = active only, omitted =
    /// either. Deliberately not a hidden default filter.
    cleared: Option<bool>,
    /// Optional. Inclusive lower bound on the raw `priority` integer. No
    /// documented "major"/"minor" mapping exists -- this is a raw numeric
    /// range over an unexplained feed value.
    priority_min: Option<i32>,
    /// Optional. Inclusive upper bound. A `400` if both bounds are given
    /// and `priority_min > priority_max`.
    priority_max: Option<i32>,
    /// Optional page size, 1..=`MAX_INCIDENT_SEARCH_LIMIT`. Over-large is
    /// clamped, not rejected; non-positive or unparseable is a `400`.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`.
    after: Option<String>,
}

/// Parses and bounds the page size -- same shape as
/// `routes::trains::normalize_limit`, kept as its own local copy since the
/// two routes' constants are deliberately not shared.
fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_INCIDENT_SEARCH_LIMIT);
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
    Ok(parsed.min(MAX_INCIDENT_SEARCH_LIMIT))
}

/// Parses a caller-supplied RFC3339 timestamp for `from`/`to`.
fn normalize_rfc3339(label: &str, raw: &str) -> Result<chrono::DateTime<chrono::Utc>, (StatusCode, String)> {
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                format!("{label} must be an RFC3339 timestamp"),
            )
        })
}

/// Renders a keyset cursor for the wire: base64url-without-padding of
/// `"{RFC3339 first_seen_at}|{incident_id}"` -- same opaque-token shape as
/// `routes::trains::encode_cursor`. Not signed: it names a public row on
/// an unauthenticated route.
fn encode_cursor(cursor: &queries::IncidentSearchCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}",
        cursor.first_seen_at.to_rfc3339(),
        cursor.incident_id
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never
/// silently dropped -- dropping it would restart the caller at page 1
/// while their UI appended the response as page 2, duplicating rows.
fn decode_cursor(raw: &str) -> Result<queries::IncidentSearchCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [first_seen_at, incident_id] = parts.as_slice() else {
        return Err(invalid());
    };
    let first_seen_at = chrono::DateTime::parse_from_rfc3339(first_seen_at)
        .map_err(|_| invalid())?
        .with_timezone(&chrono::Utc);
    Ok(queries::IncidentSearchCursor {
        first_seen_at,
        incident_id: (*incident_id).to_string(),
    })
}

/// Renders one `IncidentSummaryRow` as camelCase JSON -- the `IncidentSummary`
/// shape `frontend/lib/types.ts` declares.
fn incident_summary_json(row: &queries::IncidentSummaryRow) -> Value {
    json!({
        "incidentId": row.incident_id,
        "summary": row.summary,
        "operators": row.operators,
        "affectedStations": row.affected_stations,
        "priority": row.priority,
        "isPlanned": row.is_planned,
        "isCleared": row.is_cleared,
        "firstSeenAt": row.first_seen_at.to_rfc3339(),
        "fetchedAt": row.fetched_at.to_rfc3339(),
    })
}

/// `GET /public/incidents` -- see
/// docs/superpowers/specs/2026-09-12-incident-archive-design.md Decisions
/// 1-5. Unauthenticated, per this file's own established public-read
/// convention. The `line` filter is a named approximation (station-overlap
/// against the resolved catalogue line's own stations) -- it catches the
/// matcher's `StationHit`/`ExclusiveSegment`/`SharedSegment` tiers but
/// misses `KeywordOnly`/`OperatorOnly`; see Correction 1 of the design spec
/// and this route's own frontend copy (Decision 6) for where that
/// limitation must stay visible to a user, not just documented here.
async fn search_incidents(
    State(app): State<App>,
    Query(params): Query<IncidentSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let operators: Option<Vec<String>> = params.operator.as_deref().and_then(|raw| {
        let list: Vec<String> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if list.is_empty() { None } else { Some(list) }
    });

    let affected_stations: Option<Vec<String>> = match params
        .line
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(line_id) => {
            let Some(line) = app.config.lines.iter().find(|l| l.id == line_id) else {
                return Err((StatusCode::BAD_REQUEST, "unknown line".to_string()));
            };
            Some(line.stations.iter().map(|s| s.crs.clone()).collect())
        }
        None => None,
    };

    let from = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_rfc3339("from", s))
        .transpose()?;
    let to = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_rfc3339("to", s))
        .transpose()?;

    if let (Some(from_bound), Some(to_bound)) = (from, to) {
        if from_bound > to_bound {
            return Err((
                StatusCode::BAD_REQUEST,
                "from must not be after to".to_string(),
            ));
        }
    }

    if let (Some(min), Some(max)) = (params.priority_min, params.priority_max) {
        if min > max {
            return Err((
                StatusCode::BAD_REQUEST,
                "priority_min must not exceed priority_max".to_string(),
            ));
        }
    }

    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    let page = queries::search_incidents(
        &app.database,
        operators,
        affected_stations,
        params.planned,
        params.cleared,
        params.priority_min,
        params.priority_max,
        from,
        to,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error)?;

    Ok(Json(json!({
        "results": page.results.iter().map(incident_summary_json).collect::<Vec<Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
    })))
}

async fn get_incident(
    State(app): State<App>,
    Path(incident_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let Some(incident) = queries::incident_by_id(&app.database, &incident_id)
        .await
        .map_err(internal_error)?
    else {
        return Err((StatusCode::NOT_FOUND, "incident not found".to_string()));
    };
    let history = queries::incident_history_for_id(&app.database, &incident_id)
        .await
        .map_err(internal_error)?;
    let source = knowledgebase_source(&incident_id);
    let lines = queries::lines_currently_reporting_incident(&app.database, &source)
        .await
        .map_err(internal_error)?;

    Ok(Json(to_incident_detail_json(incident, history, lines)))
}

/// Renders `serde_json::Value` field-by-field via `json!()`, exactly like
/// `crates/api/src/render.rs::status_to_json` -- deliberately NOT
/// `#[derive(Serialize)] #[serde(rename_all = "camelCase")]` on a struct
/// that embeds `validity_periods` directly, because `rename_all` is not
/// inherited into a nested type. See this plan's Status note Correction A
/// and Global Constraints for the concrete failure mode that would produce
/// (a response that's camelCase at the top level but snake_case inside
/// every validity period). Pure function, no I/O -- unit-testable without
/// a database, matching `to_tfl_shape`'s own testable shape in `render.rs`.
fn to_incident_detail_json(
    incident: queries::IncidentRow,
    history: Vec<queries::IncidentHistoryRow>,
    lines: Vec<queries::IncidentLineRefRow>,
) -> Value {
    json!({
        "incidentId": incident.incident_id,
        "summary": incident.summary,
        "description": incident.description,
        "operators": incident.operators,
        "affectedStations": incident.affected_stations,
        "priority": incident.priority,
        "validityPeriods": render_validity_periods(&incident.validity_periods),
        "isPlanned": incident.is_planned,
        "isCleared": incident.is_cleared,
        "firstSeenAt": incident.first_seen_at.to_rfc3339(),
        "fetchedAt": incident.fetched_at.to_rfc3339(),
        "currentlyAffectsLines": lines.iter().map(|l| json!({
            "id": l.line_id,
            "name": l.name,
        })).collect::<Vec<_>>(),
        "history": history.iter().map(|h| json!({
            "summary": h.summary,
            "description": h.description,
            "operators": h.operators,
            "affectedStations": h.affected_stations,
            "priority": h.priority,
            "validityPeriods": render_validity_periods(&h.validity_periods),
            "isPlanned": h.is_planned,
            "isCleared": h.is_cleared,
            "recordedAt": h.recorded_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })
}

/// `validity_periods` comes back from `queries::incident_by_id`/
/// `incident_history_for_id` as raw `serde_json::Value` (the column's own
/// stored JSONB, snake-case field names -- `from_date`/`to_date`/`is_now`,
/// since `common::ValidityPeriod` has no `rename_all`). Deserializes into
/// the real Rust type first, rather than this function re-implementing
/// JSONB field access by hand. A malformed row does NOT fail loudly: it
/// degrades silently to an empty validity-periods array, with a
/// `tracing::warn!` as the only signal.
fn render_validity_periods(raw: &Value) -> Value {
    let periods: Vec<common::ValidityPeriod> = match serde_json::from_value(raw.clone()) {
        Ok(periods) => periods,
        Err(err) => {
            tracing::warn!(error = ?err, "failed to deserialize validity_periods JSONB, rendering as empty");
            Vec::new()
        }
    };
    Value::Array(
        periods
            .into_iter()
            .map(|p| {
                json!({
                    "fromDate": p.from_date.to_rfc3339(),
                    "toDate": p.to_date.map(|d| d.to_rfc3339()),
                    "isNow": p.is_now,
                })
            })
            .collect(),
    )
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "incident lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn sample_incident() -> queries::IncidentRow {
        queries::IncidentRow {
            incident_id: "12345".to_string(),
            summary: "Signal failure at Woking".to_string(),
            description: "<p>Delays expected</p>".to_string(),
            operators: vec!["VT".to_string()],
            affected_stations: vec!["WOK".to_string(), "WAT".to_string()],
            priority: 3,
            validity_periods: serde_json::json!([
                {"from_date": "2026-08-30T09:00:00Z", "to_date": null, "is_now": true}
            ]),
            is_planned: false,
            is_cleared: false,
            first_seen_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            fetched_at: Utc.with_ymd_and_hms(2026, 8, 31, 10, 15, 0).unwrap(),
        }
    }

    #[test]
    fn renders_top_level_fields_as_camel_case() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        assert_eq!(json["incidentId"], "12345");
        assert_eq!(json["summary"], "Signal failure at Woking");
        assert_eq!(json["affectedStations"][0], "WOK");
        assert_eq!(json["isPlanned"], false);
        assert_eq!(json["isCleared"], false);
    }

    #[test]
    fn validity_periods_render_as_camel_case_not_snake_case() {
        // The direct regression test for Correction A -- proves this
        // function does not fall back to a derived Serialize impl that
        // would leak `from_date`/`to_date`/`is_now` through unrenamed.
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        let period = &json["validityPeriods"][0];
        assert_eq!(period["fromDate"], "2026-08-30T09:00:00+00:00");
        assert!(period["toDate"].is_null());
        assert_eq!(period["isNow"], true);
        assert!(
            period.get("from_date").is_none(),
            "must not leak the raw snake_case JSONB field name"
        );
    }

    #[test]
    fn currently_affects_lines_is_empty_array_not_null_when_no_lines_match() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        assert!(json["currentlyAffectsLines"].is_array());
        assert_eq!(json["currentlyAffectsLines"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn history_renders_every_entry_newest_first_order_preserved() {
        let history = vec![
            queries::IncidentHistoryRow {
                summary: "v2".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 2,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 31, 9, 0, 0).unwrap(),
            },
            queries::IncidentHistoryRow {
                summary: "v1".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 1,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            },
        ];
        let json = to_incident_detail_json(sample_incident(), history, vec![]);
        assert_eq!(json["history"][0]["summary"], "v2");
        assert_eq!(json["history"][1]["summary"], "v1");
    }

    #[test]
    fn currently_affects_lines_renders_id_and_name() {
        let lines = vec![queries::IncidentLineRefRow {
            line_id: "south-western".to_string(),
            name: "South Western Main Line".to_string(),
        }];
        let json = to_incident_detail_json(sample_incident(), vec![], lines);
        assert_eq!(json["currentlyAffectsLines"][0]["id"], "south-western");
        assert_eq!(
            json["currentlyAffectsLines"][0]["name"],
            "South Western Main Line"
        );
    }

    #[test]
    fn knowledgebase_source_matches_the_exact_format_correction_1_verified() {
        assert_eq!(
            knowledgebase_source("12345"),
            "knowledgebase-incident-12345"
        );
    }
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
    use crate::auth::internal_oauth::ServiceTokenVerifier;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Local to this test module, matching `routes::trains`'s own
    /// `test_app` -- this codebase's convention is one small
    /// per-route-test-module copy of this fixture builder, not a shared
    /// helper (grepped: `stanox_crs.rs`, `departures.rs`,
    /// `station_stats.rs`, `trains.rs`, `chatbot.rs`, `groups.rs`,
    /// `ingest.rs`, `train.rs` each define their own).
    fn test_app(pool: PgPool, lines: Vec<common::LineDefinition>) -> App {
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
            lines: LineCatalogue(lines),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
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
            internal_oauth_verifier: ServiceTokenVerifier::new(
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

    async fn delete_fixtures(pool: &PgPool) {
        sqlx::query("DELETE FROM incidents WHERE incident_id LIKE 'route-test-%'")
            .execute(pool)
            .await
            .expect("cleanup fixture incidents rows");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_incident(
        pool: &PgPool,
        incident_id: &str,
        operators: &[&str],
        affected_stations: &[&str],
        priority: i32,
        is_planned: bool,
        is_cleared: bool,
    ) {
        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, priority, \
                 is_planned, is_cleared) \
             VALUES ($1, $2, '', $3, $4, $5, $6, $7)",
        )
        .bind(incident_id)
        .bind(format!("Fixture incident {incident_id}"))
        .bind(operators)
        .bind(affected_stations)
        .bind(priority)
        .bind(is_planned)
        .bind(is_cleared)
        .execute(pool)
        .await
        .expect("seed fixture incidents row");
    }

    async fn get(pool: &PgPool, lines: Vec<common::LineDefinition>, uri: &str) -> (StatusCode, String) {
        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone(), lines));
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

    fn fixture_line() -> common::LineDefinition {
        common::LineDefinition {
            id: "test-line".to_string(),
            name: "Test Line".to_string(),
            mode: "train".to_string(),
            category: "main".to_string(),
            operators: vec!["VT".to_string()],
            stations: vec![
                common::Station {
                    crs: "WAT".to_string(),
                    tiploc: None,
                    role: "principal".to_string(),
                    segment: None,
                },
                common::Station {
                    crs: "WOK".to_string(),
                    tiploc: None,
                    role: "principal".to_string(),
                    segment: None,
                },
            ],
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: std::collections::HashMap::new(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_with_no_filters_returns_every_seeded_row_as_200() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-1", &["VT"], &["WAT"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents").await;
        assert_eq!(status, StatusCode::OK);
        assert!(!results(&body).is_empty());
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_operator_param_is_comma_parsed_and_matches_on_overlap() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-2", &["VT", "SW"], &["WAT"], 1, false, false).await;
        seed_incident(&pool, "route-test-3", &["GW"], &["PAD"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents?operator=SW,XX").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let ids: Vec<&str> = rows.iter().map(|r| r["incidentId"].as_str().unwrap()).collect();
        assert_eq!(ids, vec!["route-test-2"]);
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_unknown_line_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?line=does-not-exist").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("line"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_known_line_resolves_to_station_overlap_and_excludes_a_no_overlap_incident()
     {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        // Matches test-line via WOK (one of the line's own stations).
        seed_incident(&pool, "route-test-4", &["VT"], &["WOK"], 1, false, false).await;
        // Same operator as the line, but NO shared station -- the shape a
        // real OperatorOnly-only matcher hit would have. Must be excluded:
        // this is the concrete proof the line filter's approximation
        // misses that tier, per Correction 1 of the design spec.
        seed_incident(&pool, "route-test-5", &["VT"], &["ZZZ"], 1, false, false).await;

        let (status, body) = get(&pool, vec![fixture_line()], "/incidents?line=test-line").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let ids: Vec<&str> = rows.iter().map(|r| r["incidentId"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            vec!["route-test-4"],
            "only the station-overlap match must be returned: {ids:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_priority_min_greater_than_max_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?priority_min=5&priority_max=1").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("priority"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_from_after_to_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(
            &pool,
            vec![],
            "/incidents?from=2026-01-02T00:00:00Z&to=2026-01-01T00:00:00Z",
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("from"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_malformed_after_cursor_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("after"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_rejects_an_unrecognized_query_parameter_instead_of_silently_ignoring_it()
     {
        let pool = connect().await;
        let (status, _) = get(&pool, vec![], "/incidents?operater=SW").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-6", &["VT"], &["WAT"], 1, false, false).await;

        let (status, _) = get(&pool, vec![], "/incidents?limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, vec![], "/incidents?limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("limit"), "400 body should name the field: {body}");

        let (status, _) = get(&pool, vec![], "/incidents?limit=99999").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is clamped to MAX_INCIDENT_SEARCH_LIMIT, never rejected"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-7", &["VT"], &["WAT"], 1, false, false).await;

        let (status, body) = get(&pool, vec![], "/incidents?limit=100").await;
        assert_eq!(status, StatusCode::OK);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_paginates_with_a_cursor_and_after_continues_from_it() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-8", &["VT"], &["WAT"], 1, false, false).await;
        seed_incident(&pool, "route-test-9", &["VT"], &["WAT"], 1, false, false).await;

        let (status, first) = get(&pool, vec![], "/incidents?limit=1").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&first).len(), 1);
        let cursor = next_cursor(&first).expect("a second page exists");

        let (status, second) = get(&pool, vec![], &format!("/incidents?limit=1&after={cursor}")).await;
        assert_eq!(status, StatusCode::OK);
        let second_rows = results(&second);
        assert_eq!(second_rows.len(), 1);
        assert_ne!(
            second_rows[0]["incidentId"], results(&first)[0]["incidentId"],
            "`after` must continue from the cursor, not restart at page 1"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_renders_camel_case_rows_with_no_leaked_snake_case_fields() {
        let pool = connect().await;
        delete_fixtures(&pool).await;
        seed_incident(&pool, "route-test-10", &["VT"], &["WAT"], 3, true, false).await;

        let (status, body) = get(&pool, vec![], "/incidents").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let row = rows
            .iter()
            .find(|r| r["incidentId"] == "route-test-10")
            .expect("fixture row present");
        assert_eq!(row["summary"], "Fixture incident route-test-10");
        assert_eq!(row["priority"], 3);
        assert_eq!(row["isPlanned"], true);
        assert_eq!(row["isCleared"], false);
        assert!(row.get("first_seen_at").is_none(), "no stray snake_case field");
        assert!(row.get("is_planned").is_none(), "no stray snake_case field");
        assert!(row.get("description").is_none(), "list rows never include description");
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search -- --ignored --test-threads=1`"]
    async fn incident_search_published_with_no_matches_is_200_with_an_empty_results_array() {
        let pool = connect().await;
        let (status, body) = get(&pool, vec![], "/incidents?operator=ZZ_NO_SUCH_OPERATOR").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an unmatched filter is a 200 with an empty results array, never a 404 -- there is \
             no 'unpublished' concept for this table"
        );
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
    }
}
