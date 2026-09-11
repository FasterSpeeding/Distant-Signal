//! `/groups`: shared groups for train tracking -- named groups with
//! join-link-based membership, letting a tracked train be shared (custom
//! name + live status, never tickets) with other group members. Mounted
//! under the existing session-authenticated `public_router()` (final path
//! `/public/groups/...`), never the internal-token-gated `private_router()`
//! -- see docs/superpowers/specs/2026-09-11-shared-groups-design.md §5.
//!
//! Permission failures here use `403 Forbidden`, NOT this crate's usual
//! 404-never-403 ownership convention (`train_tracking::tracked_train_owner`'s
//! own doc comment). That convention exists to hide WHETHER a resource
//! exists at all from someone with no legitimate claim to know. A group
//! member who lacks `admin`/`owner` permission already knows the group
//! exists (they can see it, they're a member of it) -- hiding that via
//! `404` would be actively confusing, not protective. This mirrors
//! `ChatbotAuthorizedUser`'s own `403` precedent (`crates/api/src/auth.rs`):
//! "a resolved, real user who simply isn't in the group is a genuinely
//! different case... not an ownership check hiding a secret resource."
//! `404` is still used here for "not a member at all" (the group may or
//! may not exist; either way, this caller has no legitimate claim to know
//! which) and for "no member/train with that id" lookups.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::groups::{self, GroupRole};

pub fn router() -> Router {
    Router::new()
        .route("/groups", axum::routing::post(create_group).get(list_groups))
        .route(
            "/groups/{id}",
            axum::routing::get(get_group).put(rename_group).delete(delete_group),
        )
        .route("/groups/{id}/members", axum::routing::get(list_members))
        .route(
            "/groups/{id}/members/{user_id}",
            axum::routing::delete(remove_member),
        )
        .route(
            "/groups/{id}/members/{user_id}/promote",
            axum::routing::post(promote_member),
        )
        .route(
            "/groups/{id}/invite-link",
            axum::routing::post(create_invite_link).delete(revoke_invite_link),
        )
        // "join" here is a literal path segment at the SAME position as
        // `/groups/{id}`'s dynamic `{id}` -- matchit resolves the literal
        // route first, the same precedence already proven for
        // `/Train/mine` vs `/Train/{tracking_id}`
        // (`routes::train::tests::literal_route_wins_over_same_position_dynamic_route`).
        // A group whose real id happened to be the literal string "join"
        // is unreachable via `/groups/{id}` as a result -- acceptable,
        // since `groups::create_group`'s ids are 32 random bytes,
        // base64url-encoded (`auth::generate_session_token`), so "join"
        // can never actually be generated.
        .route(
            "/groups/join/{token}",
            axum::routing::get(get_join_preview).post(post_join),
        )
        .route(
            "/groups/{id}/trains",
            axum::routing::get(list_group_trains_route).post(add_group_train),
        )
        .route(
            "/groups/{id}/trains/{train_subscription_id}",
            axum::routing::delete(remove_group_train),
        )
}

/// Shared permission gate: `404` if the caller isn't a member of
/// `group_id` at all (this app's universal "exists but not yours"
/// convention -- see this file's own module doc), `403` if they're a
/// member but `predicate` rejects their role. `predicate` is one of
/// `GroupRole::can_manage`/`GroupRole::is_owner`. Returns the caller's own
/// role on success, since several call sites need it again afterwards.
async fn require_role(
    app: &App,
    group_id: &str,
    user_id: &str,
    predicate: fn(GroupRole) -> bool,
) -> Result<GroupRole, (StatusCode, String)> {
    let role = groups::get_member_role(&app.database, group_id, user_id)
        .await
        .map_err(internal_error("check group membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))?;
    if !predicate(role) {
        return Err((
            StatusCode::FORBIDDEN,
            "you don't have permission to do that in this group".to_string(),
        ));
    }
    Ok(role)
}

/// Shared 500 mapper for every route in this file, mirroring
/// `routes::train::internal_error`'s own shape exactly.
fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        tracing::error!(error = ?err, operation, "group request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to {operation}"),
        )
    }
}

/// Same cap as `common::CUSTOM_NAME_MAX_LENGTH` -- no group-naming
/// precedent exists yet in this codebase, so this reuses the established
/// tracked-train/ticket custom-name limit rather than inventing a new one.
const MAX_GROUP_NAME_LENGTH: usize = 100;

/// Same user-facing-copy posture as `train_tracking::validate_pin`'s doc
/// comment: this message is rendered verbatim by the frontend's error
/// `Alert`, so it carries no internal field names.
fn validate_group_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Enter a name for this group.".to_string());
    }
    if trimmed.chars().count() > MAX_GROUP_NAME_LENGTH {
        return Err(format!(
            "That name is too long — group names can be at most {MAX_GROUP_NAME_LENGTH} \
             characters."
        ));
    }
    Ok(trimmed.to_string())
}

#[derive(Debug, Deserialize)]
struct CreateGroupRequest {
    name: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupIdentityResponse {
    id: String,
    name: String,
}

async fn create_group(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(req): Json<CreateGroupRequest>,
) -> Result<Json<GroupIdentityResponse>, (StatusCode, String)> {
    let name = validate_group_name(&req.name).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    let id = groups::create_group(&app.database, &name, &user.id)
        .await
        .map_err(internal_error("create group"))?;
    Ok(Json(GroupIdentityResponse { id, name }))
}

async fn list_groups(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<groups::GroupSummary>>, (StatusCode, String)> {
    let list = groups::list_groups_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list groups"))?;
    Ok(Json(list))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GroupDetailResponse {
    id: String,
    name: String,
    owner_id: String,
    owner_name: Option<String>,
    member_count: i64,
    role: GroupRole,
    // No dedicated `GET` route exists in the spec's API table for reading
    // the current invite link (§5 lists only the two mutating routes) --
    // this extends `GET /groups/{id}`'s own response to carry it instead
    // of inventing an unlisted new route, since the frontend detail page
    // (Task 12) needs to display the current link on every visit, not
    // only right after a rotate. `None` for a plain `member` (spec §6:
    // the invite link is "visible only to admin/owner").
    invite_link: Option<groups::InviteLink>,
}

async fn get_group(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<Json<GroupDetailResponse>, (StatusCode, String)> {
    let detail = groups::get_group_detail(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("read group"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))?;

    let invite_link = if detail.role.can_manage() {
        groups::get_active_invite_link(&app.database, &group_id)
            .await
            .map_err(internal_error("read invite link"))?
    } else {
        None
    };

    Ok(Json(GroupDetailResponse {
        id: detail.id,
        name: detail.name,
        owner_id: detail.owner_id,
        owner_name: detail.owner_name,
        member_count: detail.member_count,
        role: detail.role,
        invite_link,
    }))
}

#[derive(Debug, Deserialize)]
struct RenameGroupRequest {
    name: String,
}

async fn rename_group(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
    Json(req): Json<RenameGroupRequest>,
) -> Result<Json<GroupIdentityResponse>, (StatusCode, String)> {
    let name = validate_group_name(&req.name).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
    require_role(&app, &group_id, &user.id, GroupRole::can_manage).await?;

    let renamed = groups::rename_group(&app.database, &group_id, &name)
        .await
        .map_err(internal_error("rename group"))?;
    if !renamed {
        return Err((StatusCode::NOT_FOUND, "no group with that id".to_string()));
    }
    Ok(Json(GroupIdentityResponse { id: group_id, name }))
}

async fn delete_group(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_role(&app, &group_id, &user.id, GroupRole::is_owner).await?;

    let deleted = groups::delete_group(&app.database, &group_id)
        .await
        .map_err(internal_error("delete group"))?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "no group with that id".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn list_members(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<groups::GroupMember>>, (StatusCode, String)> {
    groups::get_member_role(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("check group membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))?;

    let members = groups::list_members(&app.database, &group_id)
        .await
        .map_err(internal_error("list members"))?;
    Ok(Json(members))
}

/// `DELETE /groups/{id}/members/{userId}` -- self-removal ("leave") is
/// always allowed for any member; removing someone ELSE requires
/// `admin`/`owner`, and can never target the `owner` row regardless of the
/// caller's own role (spec §3: "an admin can never remove the owner" --
/// and there is only ever one owner, so this also protects the owner from
/// a hypothetical second admin/owner-equivalent). The actual
/// ownership-transfer/departed-cleanup/group-deletion logic lives entirely
/// in `groups::remove_member` (Task 3); this handler only decides WHETHER
/// the removal is authorized.
async fn remove_member(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((group_id, target_user_id)): Path<(String, String)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let caller_role = groups::get_member_role(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("check group membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))?;

    let is_self = target_user_id == user.id;
    if !is_self {
        if !caller_role.can_manage() {
            return Err((
                StatusCode::FORBIDDEN,
                "you don't have permission to remove members from this group".to_string(),
            ));
        }
        let target_role = groups::get_member_role(&app.database, &group_id, &target_user_id)
            .await
            .map_err(internal_error("check target membership"))?;
        if target_role == Some(GroupRole::Owner) {
            return Err((
                StatusCode::FORBIDDEN,
                "the group owner can't be removed".to_string(),
            ));
        }
    }

    match groups::remove_member(&app.database, &group_id, &target_user_id)
        .await
        .map_err(internal_error("remove member"))?
    {
        groups::RemoveMemberOutcome::NotAMember => {
            Err((StatusCode::NOT_FOUND, "no member with that id".to_string()))
        }
        groups::RemoveMemberOutcome::Removed { .. } | groups::RemoveMemberOutcome::GroupDeleted => {
            Ok(StatusCode::NO_CONTENT)
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PromoteResponse {
    user_id: String,
    role: GroupRole,
}

async fn promote_member(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((group_id, target_user_id)): Path<(String, String)>,
) -> Result<Json<PromoteResponse>, (StatusCode, String)> {
    require_role(&app, &group_id, &user.id, GroupRole::is_owner).await?;

    let target_role = groups::get_member_role(&app.database, &group_id, &target_user_id)
        .await
        .map_err(internal_error("check target membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no member with that id".to_string()))?;
    if target_role != GroupRole::Member {
        return Err((
            StatusCode::CONFLICT,
            "that member is already an admin or the owner".to_string(),
        ));
    }

    let promoted = groups::promote_to_admin(&app.database, &group_id, &target_user_id)
        .await
        .map_err(internal_error("promote member"))?;
    if !promoted {
        return Err((StatusCode::NOT_FOUND, "no member with that id".to_string()));
    }
    Ok(Json(PromoteResponse {
        user_id: target_user_id,
        role: GroupRole::Admin,
    }))
}

async fn create_invite_link(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<Json<groups::InviteLink>, (StatusCode, String)> {
    require_role(&app, &group_id, &user.id, GroupRole::can_manage).await?;
    let link = groups::rotate_invite_link(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("create invite link"))?;
    Ok(Json(link))
}

async fn revoke_invite_link(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_role(&app, &group_id, &user.id, GroupRole::can_manage).await?;
    groups::revoke_invite_link(&app.database, &group_id)
        .await
        .map_err(internal_error("revoke invite link"))?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /groups/join/{token}` -- UNAUTHENTICATED (no `AuthenticatedUser`
/// extractor): resolving a join token to a group preview must work for a
/// visitor who isn't logged in yet, so the confirm-before-join page
/// (spec §2.3) can render "Join {group name}?" before sending them through
/// login. Never changes membership.
async fn get_join_preview(
    State(app): State<App>,
    Path(token): Path<String>,
) -> Result<Json<groups::JoinPreview>, (StatusCode, String)> {
    groups::resolve_invite_link(&app.database, &token)
        .await
        .map_err(internal_error("resolve invite link"))?
        .map(Json)
        .ok_or((
            StatusCode::NOT_FOUND,
            "this invite link is invalid or has expired".to_string(),
        ))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct JoinResponse {
    group_id: String,
}

/// `POST /groups/join/{token}` -- the actual join, requiring a real
/// session (spec §2.3: "Confirm-before-join, never silent auto-join" --
/// this is the explicit action the confirm page's Join button fires).
async fn post_join(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(token): Path<String>,
) -> Result<Json<JoinResponse>, (StatusCode, String)> {
    let group_id = groups::consume_invite_link(&app.database, &token, &user.id)
        .await
        .map_err(internal_error("join group"))?
        .ok_or((
            StatusCode::NOT_FOUND,
            "this invite link is invalid or has expired".to_string(),
        ))?;
    Ok(Json(JoinResponse { group_id }))
}

/// `_route` suffix avoids shadowing `groups::list_group_trains` while
/// still reading naturally at the call site (`groups::list_group_trains`
/// vs this file's own `list_group_trains_route`) -- same reasoning
/// `routes::train.rs`'s handlers apply when a handler and its data-layer
/// counterpart would otherwise share an identical bare name.
async fn list_group_trains_route(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<groups::GroupTrain>>, (StatusCode, String)> {
    groups::get_member_role(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("check group membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))?;

    let trains = groups::list_group_trains(&app.database, &group_id)
        .await
        .map_err(internal_error("list group trains"))?;
    Ok(Json(trains))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddGroupTrainRequest {
    train_subscription_id: i64,
}

/// Any current member may add one of their OWN tracked trains (spec §3);
/// `groups::add_train_to_group`'s own ownership check is what actually
/// enforces "their own" -- this handler only checks group membership.
async fn add_group_train(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
    Json(req): Json<AddGroupTrainRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    groups::get_member_role(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("check group membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))?;

    let added = groups::add_train_to_group(
        &app.database,
        &group_id,
        req.train_subscription_id,
        &user.id,
    )
    .await
    .map_err(internal_error("add train to group"))?;
    if !added {
        return Err((
            StatusCode::NOT_FOUND,
            "no tracked train with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /groups/{id}/trains/{trainSubscriptionId}` -- the sharer, or
/// any `admin`/`owner`, may remove a shared train (spec §3). The actual
/// sharer-or-manager check lives in `groups::remove_train_from_group`
/// (Task 5, given `role.can_manage()` computed here).
async fn remove_group_train(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((group_id, train_subscription_id)): Path<(String, i64)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let role = groups::get_member_role(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("check group membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))?;

    let removed = groups::remove_train_from_group(
        &app.database,
        &group_id,
        train_subscription_id,
        &user.id,
        role.can_manage(),
    )
    .await
    .map_err(internal_error("remove train from group"))?;
    if !removed {
        return Err((
            StatusCode::NOT_FOUND,
            "no shared train with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_builds_without_panicking() {
        let _ = router();
    }

    #[tokio::test]
    async fn join_literal_route_wins_over_same_position_dynamic_id_route() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = axum::Router::new()
            .route("/groups/join/{token}", axum::routing::get(|| async { "join" }))
            .route("/groups/{id}", axum::routing::get(|| async { "dynamic" }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/groups/join/some-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"join");
    }
}

/// Route-level permission tests for this file's handlers, driven through
/// the real `axum::Router` against a real database -- the layer
/// `crate::data::groups::db_tests` deliberately doesn't cover, since a
/// data-layer test can only prove what a query does, never which callers a
/// HANDLER lets reach it. This file is the most permission-dense route
/// module in the crate (15 handlers, three distinct role predicates), so
/// the three cases picked here are the ones where the handler's own gate,
/// not the data layer's, is the entire behavior: an admin is refused the
/// owner's row, a non-owner is refused promotion, and the invite link is
/// withheld from a plain member.
///
/// The `test_app`/`test_router`/`seed_session`/`connect`/`request`/
/// `post_json`/`delete_request` helpers below are this file's OWN copy of
/// the harness `crate::routes::train::db_tests` (and
/// `routes::lines`/`routes::line_status` before it) each keeps privately.
/// That duplication is the established convention here, not an oversight
/// -- see `train::db_tests`' own doc comment on why promoting them to a
/// shared test-support module is a separate, deliberate decision rather
/// than something to do while adding a file's first route tests.
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

    /// Every `ServiceArguments` field filled with an inert placeholder.
    /// None of this file's routes read `config` at all (they only ever
    /// touch `app.database`), so there is no caller-supplied variance to
    /// thread through and no `..with_x` variant of this helper.
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
        };

        std::sync::Arc::new(AppState {
            config,
            database: pool,
            // `Client::open` only parses the URL, never opens a socket --
            // see `AppState::redis`'s doc comment. No route in this file
            // touches Redis at all.
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

    /// This file's own `router()`, mounted unprefixed exactly as `main.rs`
    /// mounts it inside `public_router()`, turned into a `tower::Service` a
    /// test can drive with `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
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

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Adds `user_id` to `group_id` at `role` directly, the same way
    /// `crate::data::groups::db_tests` seeds non-owner memberships -- going
    /// through `consume_invite_link` would only ever produce a `member`,
    /// and `promote_to_admin` is one of the very things under test here.
    async fn seed_membership(pool: &PgPool, group_id: &str, user_id: &str, role: &str) {
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role, joined_at) \
             VALUES ($1, $2, $3, NOW())",
        )
        .bind(group_id)
        .bind(user_id)
        .bind(role)
        .execute(pool)
        .await
        .expect("seed fixture group membership");
    }

    /// Deletes the fixture group (cascading `group_members`,
    /// `group_trains`, and `group_invite_links`) and THEN its fixture
    /// users. Order matters: `groups.created_by` and
    /// `group_invite_links.created_by` reference `users(id)` with no
    /// `ON DELETE CASCADE` (see
    /// `crates/api/migrations/20260911090000_shared_groups.sql`), so
    /// deleting the users first would fail on a foreign-key violation
    /// while the group still existed.
    async fn cleanup(pool: &PgPool, group_id: &str, user_ids: &[&str]) {
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(pool)
            .await
            .expect("cleanup fixture group");
        for id in user_ids {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .expect("cleanup fixture user");
        }
    }

    /// Issues a `GET` against `router`, optionally with a session cookie,
    /// and returns `(status, parsed JSON body)`. A plain-text
    /// `(StatusCode, String)` error body is wrapped as a JSON string so
    /// every case shares one return shape -- same helper shape as
    /// `routes::train::db_tests::request`.
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
        read_response(router, req).await
    }

    /// Issues a `POST` with a JSON body -- the write-path counterpart to
    /// `request`. `body: None` sends an empty body, which is what the
    /// bodyless `POST` routes in this file (`promote`, `invite-link`,
    /// `join`) expect.
    async fn post_json(
        router: axum::Router,
        uri: String,
        raw_token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().uri(uri).method("POST");
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&value).expect("serialize request body"),
                ))
                .expect("build request"),
            None => builder.body(Body::empty()).expect("build request"),
        };
        read_response(router, req).await
    }

    /// Issues a `DELETE` against an arbitrary URI (unlike
    /// `routes::train::db_tests::delete_request`, which only ever targets
    /// `/Train/{id}` and so takes an `i64` -- this file has three distinct
    /// `DELETE` routes with different path shapes).
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
        read_response(router, req).await
    }

    /// Shared tail of the three request helpers above: drive the router,
    /// then read the body as JSON, falling back to a `Value::String` for a
    /// plain-text error body and `Value::Null` for an empty `204` one.
    async fn read_response(router: axum::Router, req: Request<Body>) -> (StatusCode, Value) {
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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_an_admin_attempting_to_remove_the_owner_is_403 -- --ignored \
                --test-threads=1`"]
    async fn remove_member_an_admin_attempting_to_remove_the_owner_is_403() {
        let pool = connect().await;
        seed_session(&pool, "TEST-ROUTE-GROUPS-RM-OWNER").await;
        let admin_token = seed_session(&pool, "TEST-ROUTE-GROUPS-RM-ADMIN").await;

        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Test Remove",
            "TEST-ROUTE-GROUPS-RM-OWNER",
        )
        .await
        .expect("create fixture group");
        seed_membership(&pool, &group_id, "TEST-ROUTE-GROUPS-RM-ADMIN", "admin").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_request(
            router,
            format!("/groups/{group_id}/members/TEST-ROUTE-GROUPS-RM-OWNER"),
            Some(&admin_token),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body,
            Value::String("the group owner can't be removed".to_string())
        );

        // The owner is still a member -- the 403 refused the write, it
        // didn't merely report one that had already happened.
        let owner_role =
            crate::data::groups::get_member_role(&pool, &group_id, "TEST-ROUTE-GROUPS-RM-OWNER")
                .await
                .expect("read owner role");
        assert_eq!(owner_role, Some(crate::data::groups::GroupRole::Owner));

        cleanup(
            &pool,
            &group_id,
            &["TEST-ROUTE-GROUPS-RM-OWNER", "TEST-ROUTE-GROUPS-RM-ADMIN"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                promote_member_a_non_owner_caller_is_403 -- --ignored --test-threads=1`"]
    async fn promote_member_a_non_owner_caller_is_403() {
        let pool = connect().await;
        seed_session(&pool, "TEST-ROUTE-GROUPS-PROMO-OWNER").await;
        let admin_token = seed_session(&pool, "TEST-ROUTE-GROUPS-PROMO-ADMIN").await;
        seed_session(&pool, "TEST-ROUTE-GROUPS-PROMO-MEMBER").await;

        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Test Promote",
            "TEST-ROUTE-GROUPS-PROMO-OWNER",
        )
        .await
        .expect("create fixture group");
        seed_membership(&pool, &group_id, "TEST-ROUTE-GROUPS-PROMO-ADMIN", "admin").await;
        seed_membership(&pool, &group_id, "TEST-ROUTE-GROUPS-PROMO-MEMBER", "member").await;

        let router = test_router(test_app(pool.clone()));
        // The caller is an `admin`, i.e. `can_manage()` -- this asserts the
        // handler gates on `is_owner` specifically, which is exactly the
        // distinction the frontend's promote control has to mirror.
        let (status, _body) = post_json(
            router,
            format!("/groups/{group_id}/members/TEST-ROUTE-GROUPS-PROMO-MEMBER/promote"),
            Some(&admin_token),
            None,
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);

        let target_role = crate::data::groups::get_member_role(
            &pool,
            &group_id,
            "TEST-ROUTE-GROUPS-PROMO-MEMBER",
        )
        .await
        .expect("read target role");
        assert_eq!(
            target_role,
            Some(crate::data::groups::GroupRole::Member),
            "the refused promotion must not have taken effect"
        );

        cleanup(
            &pool,
            &group_id,
            &[
                "TEST-ROUTE-GROUPS-PROMO-OWNER",
                "TEST-ROUTE-GROUPS-PROMO-ADMIN",
                "TEST-ROUTE-GROUPS-PROMO-MEMBER",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_group_a_plain_member_never_sees_the_invite_link -- --ignored \
                --test-threads=1`"]
    async fn get_group_a_plain_member_never_sees_the_invite_link() {
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-GROUPS-LINK-OWNER").await;
        let member_token = seed_session(&pool, "TEST-ROUTE-GROUPS-LINK-MEMBER").await;

        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Test Invite Link",
            "TEST-ROUTE-GROUPS-LINK-OWNER",
        )
        .await
        .expect("create fixture group");
        seed_membership(&pool, &group_id, "TEST-ROUTE-GROUPS-LINK-MEMBER", "member").await;
        crate::data::groups::rotate_invite_link(&pool, &group_id, "TEST-ROUTE-GROUPS-LINK-OWNER")
            .await
            .expect("rotate fixture invite link");

        let router = test_router(test_app(pool.clone()));

        let (member_status, member_body) = request(
            router.clone(),
            format!("/groups/{group_id}"),
            Some(&member_token),
        )
        .await;
        assert_eq!(member_status, StatusCode::OK);
        assert_eq!(
            member_body.get("inviteLink"),
            Some(&Value::Null),
            "a plain member must never receive the invite link"
        );

        // Positive control, in the same test and against the same group and
        // the same live link: without this, an unconditionally-null
        // `inviteLink` would pass the assertion above just as happily as a
        // correctly role-conditional one.
        let (owner_status, owner_body) =
            request(router, format!("/groups/{group_id}"), Some(&owner_token)).await;
        assert_eq!(owner_status, StatusCode::OK);
        let owner_link = owner_body
            .get("inviteLink")
            .expect("owner response carries an inviteLink field");
        assert!(
            owner_link.is_object(),
            "the owner must receive the invite link, got {owner_link:?}"
        );
        assert!(
            owner_link
                .get("token")
                .and_then(Value::as_str)
                .is_some_and(|token| !token.is_empty()),
            "the owner's invite link must carry a non-empty token"
        );

        cleanup(
            &pool,
            &group_id,
            &[
                "TEST-ROUTE-GROUPS-LINK-OWNER",
                "TEST-ROUTE-GROUPS-LINK-MEMBER",
            ],
        )
        .await;
    }
}
