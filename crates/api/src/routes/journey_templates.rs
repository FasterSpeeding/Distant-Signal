//! `/JourneyTemplates/...`: durable, reusable journey templates (Phase B).
//! See docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
//! and
//! docs/superpowers/plans/2026-09-22-reusable-journeys-phaseB-durable-templates-plan.md.
//! Every route here requires an authenticated session (`AuthenticatedUser`),
//! same posture as `routes::journeys`. Materialization
//! (`POST .../materialize`) is manual/synchronous only in this phase -- no
//! scheduler, no automated sweep; that's Phase C's job, not this file's.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use common::TimeWindow;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::journey_templates::{self, TemplateLegInput};
use crate::data::journeys;
use crate::data::train_tracking;

pub fn router() -> Router {
    Router::new()
        .route(
            "/JourneyTemplates",
            axum::routing::post(post_journey_template),
        )
        .route(
            "/JourneyTemplates/mine",
            axum::routing::get(get_my_journey_templates),
        )
        .route(
            "/JourneyTemplates/{template_id}",
            axum::routing::get(get_journey_template)
                .put(put_journey_template)
                .delete(delete_journey_template),
        )
        .route(
            "/JourneyTemplates/{template_id}/materialize",
            axum::routing::post(post_materialize_journey_template),
        )
}

/// One manually-entered template leg on the wire -- shared by
/// `CreateJourneyTemplateRequest::Manual` and `PutJourneyTemplateRequest`.
/// Field-for-field the `window`-mode shape of `journeys::CreateJourneyLegRequest`
/// minus `service_date` (a template leg is date-less) and minus `mode`
/// itself (there's only one leg shape here, no pin/knownTrain equivalent
/// for a template -- see this plan's Judgment Call 2 for why that's fine).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TemplateLegRequest {
    origin_crs: String,
    destination_crs: String,
    #[serde(default)]
    depart_window: TimeWindow,
    #[serde(default)]
    arrive_window: TimeWindow,
}

/// `POST /JourneyTemplates`'s two mutually-exclusive creation shapes --
/// mirrors `journeys::CreateJourneyLegRequest`'s own tagged-enum shape and
/// serde attributes exactly (same `rename_all_fields` requirement, same
/// reasoning).
#[derive(Debug, Deserialize)]
#[serde(
    tag = "mode",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum CreateJourneyTemplateRequest {
    /// Build a new template directly from caller-supplied legs. See this
    /// plan's Judgment Call 5: the frontend built in this plan never
    /// actually sends this variant (no "start from nothing" UI ships in
    /// Phase B) -- it exists because the task brief asks for both "create"
    /// and "promote," and because `PutJourneyTemplateRequest` needs this
    /// exact leg-list shape anyway.
    Manual {
        #[serde(default)]
        custom_name: Option<String>,
        legs: Vec<TemplateLegRequest>,
    },
    /// Promotes an existing journey the caller OWNS (not merely
    /// group-shared-with -- see this plan's Judgment Call 3) into a
    /// durable template. Reads the journey's own legs
    /// (`journeys::list_legs_for_journey`) and copies each one's
    /// origin/destination/window verbatim, no `service_date`, no train
    /// binding. This is §6 item 2's "Make this a template" button.
    FromJourney {
        #[serde(default)]
        custom_name: Option<String>,
        journey_id: i64,
    },
}

/// `PUT /JourneyTemplates/{id}`'s body -- full-resource replace (Judgment
/// Calls 1 and 4). Same `TemplateLegRequest` shape as the `Manual` create
/// variant; no `mode` tag needed since there's only one shape for an edit.
///
/// The six recurrence fields (`days_of_week` through `auto_commit_rule`)
/// are deliberately REQUIRED here, unlike `custom_name`'s
/// `#[serde(default)]` -- this is a full-resource-replace endpoint (every
/// save resends the whole `legs` list too), so making these required means
/// a caller can never accidentally wipe out a template's recurrence config
/// by omitting them from a plain name/leg edit. The frontend always
/// populates every field from the currently-loaded template before
/// submitting.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PutJourneyTemplateRequest {
    #[serde(default)]
    custom_name: Option<String>,
    legs: Vec<TemplateLegRequest>,
    days_of_week: Option<i16>,
    active: bool,
    starts_on: Option<NaiveDate>,
    ends_on: Option<NaiveDate>,
    default_match_mode: String,
    auto_commit_rule: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MaterializeTemplateRequest {
    /// Required, no server-side default -- matches
    /// `journeys::CreateJourneyLegRequest`'s own convention that every
    /// leg-creation wire type takes an explicit `serviceDate`, never an
    /// implicit "today". The frontend's "Run now" button (Task 9) defaults
    /// its own date field to today client-side and lets the user change
    /// it before submitting, the same "user is present and picks a date"
    /// posture the design doc's §1 describes for the whole "reusable"
    /// trigger mechanism.
    service_date: NaiveDate,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CreateJourneyTemplateResponse {
    template_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MaterializeTemplateResponse {
    journey_id: i64,
    leg_ids: Vec<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyTemplateLegDetailResponse {
    id: i64,
    origin_crs: Option<String>,
    origin_name: Option<String>,
    destination_crs: Option<String>,
    destination_name: Option<String>,
    depart_after: Option<NaiveTime>,
    depart_before: Option<NaiveTime>,
    arrive_after: Option<NaiveTime>,
    arrive_before: Option<NaiveTime>,
}

/// `GET /JourneyTemplates/{id}`'s response. `daysOfWeek`/`active`/
/// `startsOn`/`endsOn`/`defaultMatchMode`/`autoCommitRule` are all real,
/// round-tripped fields (so a future Phase C edit UI has somewhere to
/// read/write) -- this plan's own frontend (Task 8) renders NONE of them
/// as editable controls; see that task's own scope note.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JourneyTemplateDetailResponse {
    id: i64,
    custom_name: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    days_of_week: Option<i16>,
    active: bool,
    starts_on: Option<NaiveDate>,
    ends_on: Option<NaiveDate>,
    default_match_mode: String,
    auto_commit_rule: Option<String>,
    legs: Vec<JourneyTemplateLegDetailResponse>,
}

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "journey template request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to {operation}"),
        )
    }
}

fn to_template_leg_input(
    leg: TemplateLegRequest,
) -> Result<TemplateLegInput, (StatusCode, String)> {
    journey_templates::validate_template_leg(
        &leg.origin_crs,
        &leg.destination_crs,
        &leg.depart_window,
        &leg.arrive_window,
    )
    .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    Ok(TemplateLegInput {
        origin_crs: Some(leg.origin_crs.trim().to_ascii_uppercase()),
        destination_crs: Some(leg.destination_crs.trim().to_ascii_uppercase()),
        depart_after: leg.depart_window.after,
        depart_before: leg.depart_window.before,
        arrive_after: leg.arrive_window.after,
        arrive_before: leg.arrive_window.before,
    })
}

/// `POST /JourneyTemplates` -- create (`Manual`) or promote-from-journey
/// (`FromJourney`). Rejects an empty `legs` array with 400 for `Manual`
/// mode (see `data::journey_templates::create_template`'s own
/// non-empty-invariant doc comment); rejects a source journey with any
/// leg missing an `origin_crs`/`destination_crs` with 400 for `FromJourney`
/// mode (the "no schedule data yet" gap named in Task 1's migration
/// comment -- rather than silently dropping that leg or minting a
/// half-blank template leg, this fails loudly with an actionable message).
async fn post_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<CreateJourneyTemplateRequest>,
) -> Result<Json<CreateJourneyTemplateResponse>, (StatusCode, String)> {
    match body {
        CreateJourneyTemplateRequest::Manual { custom_name, legs } => {
            if legs.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "A template needs at least one leg.".to_string(),
                ));
            }
            journey_templates::validate_leg_count(legs.len())
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
            // 2026-09 Signal Box Audit Low finding: same unvalidated
            // custom_name write path `routes::journeys::post_journey` had
            // (see that fix's own doc comment) -- this one lives right
            // next to it in the sibling templates route, and was equally
            // missing the trim/length-cap every other custom-name write in
            // this codebase applies.
            let custom_name = train_tracking::validate_custom_name(custom_name.as_deref())
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
            let leg_inputs = legs
                .into_iter()
                .map(to_template_leg_input)
                .collect::<Result<Vec<_>, _>>()?;
            let template_id = journey_templates::create_template(
                &app.database,
                &user.id,
                custom_name.as_deref(),
                &leg_inputs,
            )
            .await
            .map_err(internal_error("create journey template"))?;
            Ok(Json(CreateJourneyTemplateResponse { template_id }))
        }
        CreateJourneyTemplateRequest::FromJourney {
            custom_name,
            journey_id,
        } => {
            // Same custom_name cap as the `Manual` arm above -- see its own
            // doc comment.
            let custom_name = train_tracking::validate_custom_name(custom_name.as_deref())
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
            // Ownership, not mere readability -- Judgment Call 3.
            let owner = journeys::journey_owner(&app.database, journey_id)
                .await
                .map_err(internal_error("check journey ownership"))?;
            if owner.as_deref() != Some(user.id.as_str()) {
                return Err((StatusCode::NOT_FOUND, "no journey with that id".to_string()));
            }

            let source_legs = journeys::list_legs_for_journey(&app.database, journey_id)
                .await
                .map_err(internal_error("read journey legs"))?;
            // Defense in depth alongside `Manual` mode's own check above:
            // a source journey's leg count isn't itself capped anywhere
            // today (each leg is added one `POST /Journeys/{id}/legs` call
            // at a time, never as a single array), but there's no reason to
            // let an already-oversized journey mint an equally-oversized
            // template here either.
            journey_templates::validate_leg_count(source_legs.len())
                .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
            if source_legs
                .iter()
                .any(|leg| leg.origin_crs.is_none() || leg.destination_crs.is_none())
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "At least one leg of this journey has no known origin/destination yet — \
                     try again once its train has schedule data."
                        .to_string(),
                ));
            }
            let leg_inputs: Vec<TemplateLegInput> = source_legs
                .into_iter()
                .map(|leg| TemplateLegInput {
                    origin_crs: leg.origin_crs,
                    destination_crs: leg.destination_crs,
                    depart_after: leg.depart_after,
                    depart_before: leg.depart_before,
                    arrive_after: leg.arrive_after,
                    arrive_before: leg.arrive_before,
                })
                .collect();

            let template_id = journey_templates::create_template(
                &app.database,
                &user.id,
                custom_name.as_deref(),
                &leg_inputs,
            )
            .await
            .map_err(internal_error("create journey template from journey"))?;
            Ok(Json(CreateJourneyTemplateResponse { template_id }))
        }
    }
}

async fn get_my_journey_templates(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<journey_templates::JourneyTemplateListItem>>, (StatusCode, String)> {
    let rows = journey_templates::list_templates_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list journey templates"))?;
    Ok(Json(rows))
}

async fn get_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
) -> Result<Json<JourneyTemplateDetailResponse>, (StatusCode, String)> {
    let template = journey_templates::get_owned_template(&app.database, template_id, &user.id)
        .await
        .map_err(internal_error("read journey template"))?
        .ok_or((
            StatusCode::NOT_FOUND,
            "no journey template with that id".to_string(),
        ))?;
    let legs = journey_templates::list_template_legs(&app.database, template_id)
        .await
        .map_err(internal_error("list journey template legs"))?;

    Ok(Json(JourneyTemplateDetailResponse {
        id: template.id,
        custom_name: template.custom_name,
        created_at: template.created_at,
        updated_at: template.updated_at,
        days_of_week: template.days_of_week,
        active: template.active,
        starts_on: template.starts_on,
        ends_on: template.ends_on,
        default_match_mode: template.default_match_mode,
        auto_commit_rule: template.auto_commit_rule,
        legs: legs
            .into_iter()
            .map(|leg| JourneyTemplateLegDetailResponse {
                id: leg.id,
                origin_crs: leg.origin_crs,
                origin_name: leg.origin_name,
                destination_crs: leg.destination_crs,
                destination_name: leg.destination_name,
                depart_after: leg.depart_after,
                depart_before: leg.depart_before,
                arrive_after: leg.arrive_after,
                arrive_before: leg.arrive_before,
            })
            .collect(),
    }))
}

async fn put_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
    Json(body): Json<PutJourneyTemplateRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    if body.legs.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "A template needs at least one leg.".to_string(),
        ));
    }
    journey_templates::validate_leg_count(body.legs.len())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    journey_templates::validate_template_recurrence(
        &body.default_match_mode,
        body.auto_commit_rule.as_deref(),
        body.days_of_week,
        body.starts_on,
        body.ends_on,
    )
    .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    // Same custom_name cap as `post_journey_template`'s own arms -- see
    // that fix's doc comment.
    let custom_name = train_tracking::validate_custom_name(body.custom_name.as_deref())
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    let leg_inputs = body
        .legs
        .into_iter()
        .map(to_template_leg_input)
        .collect::<Result<Vec<_>, _>>()?;
    let replaced = journey_templates::replace_template(
        &app.database,
        template_id,
        &user.id,
        custom_name.as_deref(),
        &leg_inputs,
        body.days_of_week,
        body.active,
        body.starts_on,
        body.ends_on,
        &body.default_match_mode,
        body.auto_commit_rule.as_deref(),
    )
    .await
    .map_err(internal_error("replace journey template"))?;
    if !replaced {
        return Err((
            StatusCode::NOT_FOUND,
            "no journey template with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let deleted = journey_templates::delete_template(&app.database, template_id, &user.id)
        .await
        .map_err(internal_error("delete journey template"))?;
    if !deleted {
        return Err((
            StatusCode::NOT_FOUND,
            "no journey template with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /JourneyTemplates/{id}/materialize` -- the manual "Run now"
/// trigger. Literally `data::journey_templates::materialize_template`
/// called synchronously from this handler, no scheduler involved -- see
/// that function's own doc comment, and this plan's closing section for
/// exactly this signature's reuse contract for Phase C.
async fn post_materialize_journey_template(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(template_id): Path<i64>,
    Json(body): Json<MaterializeTemplateRequest>,
) -> Result<Json<MaterializeTemplateResponse>, (StatusCode, String)> {
    let result = journey_templates::materialize_template(
        &app.database,
        template_id,
        &user.id,
        body.service_date,
    )
    .await
    .map_err(internal_error("materialize journey template"))?;
    let Some(materialized) = result else {
        return Err((
            StatusCode::NOT_FOUND,
            "no journey template with that id".to_string(),
        ));
    };
    Ok(Json(MaterializeTemplateResponse {
        journey_id: materialized.journey_id,
        leg_ids: materialized.leg_ids,
    }))
}

#[cfg(test)]
mod wire_format_tests {
    use super::{CreateJourneyTemplateRequest, PutJourneyTemplateRequest};

    #[test]
    fn manual_mode_deserializes_its_camel_case_fields() {
        let request: CreateJourneyTemplateRequest = serde_json::from_str(
            r#"{
                "mode": "manual",
                "customName": "Weekday commute",
                "legs": [
                    {"originCrs": "WAT", "destinationCrs": "RDG",
                     "departWindow": {"after": "08:00:00"}}
                ]
            }"#,
        )
        .expect("valid manual-mode request should deserialize");
        assert!(matches!(
            request,
            CreateJourneyTemplateRequest::Manual { .. }
        ));
    }

    #[test]
    fn from_journey_mode_deserializes_its_camel_case_fields() {
        let request: CreateJourneyTemplateRequest =
            serde_json::from_str(r#"{"mode": "fromJourney", "journeyId": 42}"#)
                .expect("valid fromJourney-mode request should deserialize");
        assert!(matches!(
            request,
            CreateJourneyTemplateRequest::FromJourney { .. }
        ));
    }

    #[test]
    fn put_request_deserializes_its_camel_case_fields() {
        let request: PutJourneyTemplateRequest = serde_json::from_str(
            r#"{"customName": "Renamed",
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}],
                "daysOfWeek": null,
                "active": true,
                "startsOn": null,
                "endsOn": null,
                "defaultMatchMode": "manual",
                "autoCommitRule": null}"#,
        )
        .expect("valid PUT request should deserialize");
        assert_eq!(request.legs.len(), 1);
    }
}

/// Scaffolding copied verbatim from `routes::journeys::db_tests` (same
/// cross-file duplication convention that module's own doc comment
/// describes) -- `test_app`/`test_router`/`seed_session`/`connect`/
/// `request`/`post_json`/`delete_request` are byte-for-byte identical to
/// that module's own copies, plus a `put_json` helper mirroring `post_json`
/// with `method("PUT")`. `cleanup_user` is the one adaptation: this module
/// additionally seeds/mutates `journey_templates`/`journey_template_legs`
/// rows (neither of which `routes::journeys::db_tests::cleanup_user` knows
/// about), so this copy also clears those, same FK-respecting order
/// `data::journey_templates::db_tests::cleanup_user` already established.
#[cfg(test)]
mod db_tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use crate::app::{App, AppState};
    use crate::auth::hash_session_token;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};
    use crate::data::users::insert_session;

    /// Every `ServiceArguments` field filled with an inert placeholder --
    /// this file's routes don't read `config.lines` at all. Copied from
    /// `routes::journeys::db_tests::test_app`'s own empty-index default.
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

    /// The real `journey_templates::router()`, mounted unprefixed exactly as
    /// `main.rs` does, turned into a `tower::Service` a test can drive with
    /// `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .with_state(app)
    }

    /// Same as `test_router`, but also merges `routes::journeys::router()`
    /// into the same router -- used by
    /// `post_materialize_journey_template_mints_a_journey_with_unmatched_legs`
    /// so that test can assert against the real downstream
    /// `GET /Journeys/{id}` shape rather than reimplementing it.
    fn test_router_with_journeys(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .merge(crate::routes::journeys::router())
            .with_state(app)
    }

    /// Seeds a real, resolvable session for `user_id` (creating the user if
    /// it doesn't already exist) and returns the *raw* token -- send it as
    /// `Cookie: distant_signal_session=<raw>`, never the hash `sessions`
    /// actually stores.
    async fn seed_session(pool: &PgPool, user_id: &str) -> String {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");

        let raw_token = format!("test-raw-session-token-for-{user_id}");
        insert_session(pool, &hash_session_token(&raw_token), user_id, 14)
            .await
            .expect("seed fixture session");
        raw_token
    }

    /// Deletes a fixture user and its fixtures. Same FK-respecting order as
    /// `routes::journeys::db_tests::cleanup_user`, plus this module's own
    /// `journey_templates`/`journey_template_legs` rows -- see this module's
    /// own doc comment above for why.
    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "DELETE FROM journey_template_legs WHERE template_id IN \
                (SELECT id FROM journey_templates WHERE user_id = $1)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture journey_template_legs rows");
        sqlx::query("DELETE FROM journey_templates WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journey_templates rows");
        sqlx::query(
            "DELETE FROM journey_legs WHERE journey_id IN \
                (SELECT id FROM journeys WHERE user_id = $1)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture journey_legs rows");
        sqlx::query("DELETE FROM journeys WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journeys rows");
        sqlx::query("DELETE FROM train_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tracked_trains rows");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Issues a GET against `router`, optionally with a session cookie, and
    /// returns `(status, parsed JSON body)`. Every route in this file
    /// returns either a JSON object body or a plain-text `(StatusCode,
    /// String)` error body, so wrapping the latter as a JSON string lets
    /// every case share one return shape.
    async fn request(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(uri);
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
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

    /// Issues a `POST` with a JSON body against `router`, optionally with a
    /// session cookie -- the write-path counterpart to `request` above
    /// (which only ever issues an empty-body `GET`). Same shared
    /// `(status, parsed body)` return shape, same plain-text-becomes-`Value::String`
    /// handling for a `(StatusCode, String)` error response.
    async fn post_json(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
        body: serde_json::Value,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .uri(uri)
            .method("POST")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder
            .body(Body::from(
                serde_json::to_vec(&body).expect("serialize request body"),
            ))
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

    /// Issues a `PUT` with a JSON body against `router`, optionally with a
    /// session cookie -- mirrors `post_json` exactly, `method("PUT")` swapped
    /// in for `method("POST")`.
    async fn put_json(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
        body: serde_json::Value,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder()
            .uri(uri)
            .method("PUT")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder
            .body(Body::from(
                serde_json::to_vec(&body).expect("serialize request body"),
            ))
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

    /// Issues a `DELETE` against `router`, optionally with a session
    /// cookie -- mirrors `routes::journeys::db_tests::delete_request`
    /// exactly (same empty-body-becomes-`Value::Null` handling: a success
    /// here is `204 No Content` with no body, so feeding that straight to
    /// `serde_json::from_slice` would fail to parse and mask a real body on
    /// any OTHER status).
    async fn delete_request(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method("DELETE").uri(uri);
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
            })
        };
        (status, value)
    }

    #[test]
    fn router_builds_without_panicking() {
        let _ = super::router();
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_template_manual_mode_creates_a_template -- --ignored --test-threads=1`"]
    async fn post_journey_template_manual_mode_creates_a_template() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-CREATE";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let (status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "customName": "Weekday commute",
                "legs": [
                    {"originCrs": "WAT", "destinationCrs": "RDG",
                     "departWindow": {"after": "08:00:00"}}
                ]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, body) = request(
            router,
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["customName"], "Weekday commute");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["originCrs"], "WAT");
        assert_eq!(legs[0]["destinationCrs"], "RDG");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_template_manual_mode_an_empty_legs_array_is_400 -- --ignored --test-threads=1`"]
    async fn post_journey_template_manual_mode_an_empty_legs_array_is_400() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-EMPTY-LEGS";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let (status, _body) = post_json(
            router,
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "legs": []
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        cleanup_user(&pool, user_id).await;
    }

    /// Low finding #6 (2026-09-25 review): before
    /// `journey_templates::validate_leg_count` existed, a `Manual`-mode
    /// `legs` array had no upper bound at all, so this would have been
    /// accepted and paid for as `MAX_LEGS_PER_TEMPLATE + 1` single-row
    /// inserts in one transaction. Pins down a clean `400` instead.
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_template_manual_mode_too_many_legs_is_400 -- --ignored --test-threads=1`"]
    async fn post_journey_template_manual_mode_too_many_legs_is_400() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-TOO-MANY-LEGS";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let legs: Vec<serde_json::Value> = (0
            ..=crate::data::journey_templates::MAX_LEGS_PER_TEMPLATE)
            .map(|_| serde_json::json!({ "originCrs": "WAT", "destinationCrs": "RDG" }))
            .collect();
        let (status, body) = post_json(
            router,
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "legs": legs
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "over-cap legs: {body:?}");

        cleanup_user(&pool, user_id).await;
    }

    /// Low finding #2 (2026-09-25 review): before this route validated
    /// `customName` at all, a multi-KB name reached `create_template`
    /// unchanged and would have been stored and rendered to every group
    /// member. Pins down that the same `common::CUSTOM_NAME_MAX_LENGTH`
    /// cap every other `custom_name` write path in this codebase already
    /// enforces (`train_tracking::validate_custom_name`) is now applied
    /// here too.
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_template_manual_mode_an_oversized_custom_name_is_400 -- --ignored \
                --test-threads=1`"]
    async fn post_journey_template_manual_mode_an_oversized_custom_name_is_400() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-LONG-NAME";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let oversized_name = "a".repeat(common::CUSTOM_NAME_MAX_LENGTH + 1);
        let (status, body) = post_json(
            router,
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "customName": oversized_name,
                "legs": [{ "originCrs": "WAT", "destinationCrs": "RDG" }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "oversized name: {body:?}");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_template_from_journey_promotes_an_owned_journey -- --ignored --test-threads=1`"]
    async fn post_journey_template_from_journey_promotes_an_owned_journey() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-PROMOTE";
        let token = seed_session(&pool, user_id).await;

        let (journey_id, _leg_id) = crate::data::journeys::create_journey_with_window_leg(
            &pool,
            user_id,
            Some("Source journey"),
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create source journey");

        let router = test_router(test_app(pool.clone()));
        let (status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "fromJourney",
                "customName": "Promoted",
                "journeyId": journey_id
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, body) = request(
            router,
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["originCrs"], "WAT");
        assert_eq!(legs[0]["destinationCrs"], "RDG");
        assert_eq!(legs[0]["departAfter"], "08:00:00");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_journey_template_from_journey_a_journey_owned_by_someone_else_is_404_not_403 -- --ignored --test-threads=1`"]
    async fn post_journey_template_from_journey_a_journey_owned_by_someone_else_is_404_not_403() {
        let pool = connect().await;
        let owner_id = "TEST-ROUTE-TEMPLATE-PROMOTE-OWNER";
        let bystander_id = "TEST-ROUTE-TEMPLATE-PROMOTE-BYSTANDER";
        let owner_token = seed_session(&pool, owner_id).await;
        let bystander_token = seed_session(&pool, bystander_id).await;

        let (journey_id, _leg_id) = crate::data::journeys::create_journey_with_window_leg(
            &pool,
            owner_id,
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow::default(),
            common::TimeWindow::default(),
        )
        .await
        .expect("create source journey");

        let router = test_router(test_app(pool.clone()));
        let (status, _body) = post_json(
            router,
            "/JourneyTemplates".to_string(),
            Some(&bystander_token),
            serde_json::json!({
                "mode": "fromJourney",
                "journeyId": journey_id
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        let _ = owner_token;
        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, bystander_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_journey_template_a_non_owner_gets_404 -- --ignored --test-threads=1`"]
    async fn get_journey_template_a_non_owner_gets_404() {
        let pool = connect().await;
        let owner_id = "TEST-ROUTE-TEMPLATE-GET-OWNER";
        let bystander_id = "TEST-ROUTE-TEMPLATE-GET-BYSTANDER";
        let owner_token = seed_session(&pool, owner_id).await;
        let bystander_token = seed_session(&pool, bystander_id).await;
        let router = test_router(test_app(pool.clone()));

        let (_status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "mode": "manual",
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}]
            }),
        )
        .await;
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, _body) = request(
            router,
            format!("/JourneyTemplates/{template_id}"),
            Some(&bystander_token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, bystander_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                put_journey_template_the_owner_can_replace_its_legs -- --ignored --test-threads=1`"]
    async fn put_journey_template_the_owner_can_replace_its_legs() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-PUT";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let (_status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "customName": "Original",
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}]
            }),
        )
        .await;
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, _body) = put_json(
            router.clone(),
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
            serde_json::json!({
                "customName": "Renamed",
                "legs": [{"originCrs": "EUS", "destinationCrs": "MAN"}],
                "daysOfWeek": null,
                "active": true,
                "startsOn": null,
                "endsOn": null,
                "defaultMatchMode": "manual",
                "autoCommitRule": null
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, body) = request(
            router,
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["customName"], "Renamed");
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0]["originCrs"], "EUS");
        assert_eq!(legs[0]["destinationCrs"], "MAN");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                put_journey_template_round_trips_every_recurrence_field -- --ignored --test-threads=1`"]
    async fn put_journey_template_round_trips_every_recurrence_field() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-PUT-RECURRENCE";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let (_status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "customName": "Original",
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}]
            }),
        )
        .await;
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, _body) = put_json(
            router.clone(),
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
            serde_json::json!({
                "customName": "Weekday commute",
                "legs": [{"originCrs": "EUS", "destinationCrs": "MAN"}],
                "daysOfWeek": 31,
                "active": false,
                "startsOn": "2026-10-01",
                "endsOn": "2026-12-31",
                "defaultMatchMode": "auto",
                "autoCommitRule": "nearest_to_now"
            }),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, body) = request(
            router,
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["daysOfWeek"], 31);
        assert_eq!(body["active"], false);
        assert_eq!(body["startsOn"], "2026-10-01");
        assert_eq!(body["endsOn"], "2026-12-31");
        assert_eq!(body["defaultMatchMode"], "auto");
        assert_eq!(body["autoCommitRule"], "nearest_to_now");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                put_journey_template_an_invalid_default_match_mode_is_400 -- --ignored --test-threads=1`"]
    async fn put_journey_template_an_invalid_default_match_mode_is_400() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-PUT-BAD-MODE";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let (_status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}]
            }),
        )
        .await;
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, _body) = put_json(
            router,
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
            serde_json::json!({
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}],
                "daysOfWeek": null,
                "active": true,
                "startsOn": null,
                "endsOn": null,
                "defaultMatchMode": "sometimes",
                "autoCommitRule": null
            }),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_journey_template_the_owner_can_delete_it -- --ignored --test-threads=1`"]
    async fn delete_journey_template_the_owner_can_delete_it() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-DELETE";
        let token = seed_session(&pool, user_id).await;
        let router = test_router(test_app(pool.clone()));

        let (_status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}]
            }),
        )
        .await;
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, _body) = delete_request(
            router.clone(),
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, _body) = request(
            router,
            format!("/JourneyTemplates/{template_id}"),
            Some(&token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_materialize_journey_template_mints_a_journey_with_unmatched_legs -- --ignored --test-threads=1`"]
    async fn post_materialize_journey_template_mints_a_journey_with_unmatched_legs() {
        let pool = connect().await;
        let user_id = "TEST-ROUTE-TEMPLATE-MATERIALIZE";
        let token = seed_session(&pool, user_id).await;
        let router = test_router_with_journeys(test_app(pool.clone()));

        let (_status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&token),
            serde_json::json!({
                "mode": "manual",
                "legs": [
                    {"originCrs": "WAT", "destinationCrs": "RDG"},
                    {"originCrs": "RDG", "destinationCrs": "BRI"}
                ]
            }),
        )
        .await;
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, materialized) = post_json(
            router.clone(),
            format!("/JourneyTemplates/{template_id}/materialize"),
            Some(&token),
            serde_json::json!({ "serviceDate": "2026-10-01" }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let journey_id = materialized["journeyId"]
            .as_i64()
            .expect("journeyId present");
        assert_eq!(
            materialized["legIds"]
                .as_array()
                .expect("legIds array")
                .len(),
            2
        );

        let (status, body) = request(router, format!("/Journeys/{journey_id}"), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let legs = body["legs"].as_array().expect("legs array");
        assert_eq!(legs.len(), 2);
        for leg in legs {
            assert_eq!(leg["matchMode"], "unmatched");
            assert_eq!(leg["serviceDate"], "2026-10-01");
        }

        sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey_legs");
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .expect("cleanup materialized journey");
        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                post_materialize_journey_template_a_non_owner_gets_404 -- --ignored --test-threads=1`"]
    async fn post_materialize_journey_template_a_non_owner_gets_404() {
        let pool = connect().await;
        let owner_id = "TEST-ROUTE-TEMPLATE-MAT-OWNER";
        let bystander_id = "TEST-ROUTE-TEMPLATE-MAT-BYSTANDER";
        let owner_token = seed_session(&pool, owner_id).await;
        let bystander_token = seed_session(&pool, bystander_id).await;
        let router = test_router(test_app(pool.clone()));

        let (_status, created) = post_json(
            router.clone(),
            "/JourneyTemplates".to_string(),
            Some(&owner_token),
            serde_json::json!({
                "mode": "manual",
                "legs": [{"originCrs": "WAT", "destinationCrs": "RDG"}]
            }),
        )
        .await;
        let template_id = created["templateId"].as_i64().expect("templateId present");

        let (status, _body) = post_json(
            router,
            format!("/JourneyTemplates/{template_id}/materialize"),
            Some(&bystander_token),
            serde_json::json!({ "serviceDate": "2026-10-01" }),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, bystander_id).await;
    }
}
