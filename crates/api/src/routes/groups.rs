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
        .route(
            "/groups",
            axum::routing::post(create_group).get(list_groups),
        )
        .route(
            "/groups/{id}",
            axum::routing::get(get_group)
                .put(rename_group)
                .delete(delete_group),
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
        // Another literal segment at `/groups/{id}`'s dynamic position,
        // resolved ahead of it by exactly the same matchit precedence
        // `/groups/join/{token}` above already relies on (and which
        // `tests::shared_trains_literal_route_wins_over_same_position_dynamic_id_route`
        // pins). Group ids are 32 random base64url bytes, so no real group
        // can ever be shadowed by this path.
        .route(
            "/groups/shared-trains",
            axum::routing::get(list_shared_trains_route),
        )
        // A third literal segment at `/groups/{id}`'s dynamic position,
        // resolved ahead of it by the same matchit precedence the two
        // above already rely on (and which
        // `tests::shared_custom_lines_literal_route_wins_over_same_position_dynamic_id_route`
        // pins). Group ids are 32 random base64url bytes, so no real group
        // can ever be shadowed by this path.
        .route(
            "/groups/shared-custom-lines",
            axum::routing::get(list_shared_custom_lines_route),
        )
        .route(
            "/groups/{id}/trains",
            axum::routing::get(list_group_trains_route).post(add_group_train),
        )
        .route(
            "/groups/{id}/trains/{train_subscription_id}",
            axum::routing::delete(remove_group_train),
        )
        // Custom-line group grants. A distinct `.../lines/custom` sub-path
        // rather than a bare `.../lines`: a future `group_lines`
        // (catalogue/TfL sharing, designed but not built) has genuinely
        // different add-permission semantics -- "any member, any known
        // public line" versus "the line's own owner, only" -- and keeping
        // each handler answering exactly one unambiguous question is worth
        // more than a shorter path. See
        // docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md §3.3.
        .route(
            "/groups/{id}/lines/custom",
            axum::routing::get(list_group_custom_lines_route).post(add_custom_line_grant),
        )
        .route(
            "/groups/{id}/lines/custom/{line_id}",
            axum::routing::delete(remove_custom_line_grant_route),
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

/// Shared "is the caller even a member at all" gate: `404` otherwise, else
/// the caller's own role -- the same 404-never-403 convention `require_role`
/// uses, minus the extra `predicate` check, for the handlers where mere
/// membership is the entire permission model (listing members, listing/
/// adding/removing group trains, and the shared half of `remove_member`
/// that every self-leave takes) and any FURTHER enforcement (ownership of
/// the individual train, sharer-vs-manager, self-vs-someone-else) happens
/// past this point rather than via a `predicate` here.
async fn require_member(
    app: &App,
    group_id: &str,
    user_id: &str,
) -> Result<GroupRole, (StatusCode, String)> {
    groups::get_member_role(&app.database, group_id, user_id)
        .await
        .map_err(internal_error("check group membership"))?
        .ok_or((StatusCode::NOT_FOUND, "no group with that id".to_string()))
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
    /// Present only when `owner_name` is absent -- the distinguishing
    /// suffix for the generic placeholder; see
    /// `groups::GroupMember.display_tag`.
    owner_tag: Option<String>,
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
        owner_tag: detail.owner_tag,
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
    require_member(&app, &group_id, &user.id).await?;

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
    let caller_role = require_member(&app, &group_id, &user.id).await?;

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
    require_member(&app, &group_id, &user.id).await?;

    let trains = groups::list_group_trains(&app.database, &group_id)
        .await
        .map_err(internal_error("list group trains"))?;
    Ok(Json(trains))
}

/// `GET /groups/shared-trains` -- every train shared into ANY group the
/// caller belongs to, minus the ones they tracked themselves, so
/// `/track/mine` can list a group-shared train alongside the caller's own
/// with a "from <group>"/"shared by <who>" tag on it.
///
/// No group id in the path and so no `require_member` gate: the caller's
/// own membership rows ARE the scope of
/// `groups::list_shared_trains_for_user`'s query (see its doc comment), so
/// a non-member simply gets nothing rather than a `404`. Membership in
/// zero groups, and membership in groups with nothing shared into them,
/// are both an empty array -- the same "no signal about groups you can't
/// see" posture the rest of this file keeps.
async fn list_shared_trains_route(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<groups::SharedTrain>>, (StatusCode, String)> {
    let trains = groups::list_shared_trains_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list shared trains"))?;
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
    require_member(&app, &group_id, &user.id).await?;

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
    let role = require_member(&app, &group_id, &user.id).await?;

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

// ---------------------------------------------------------------------------
// Custom-line group grants. See
// docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md.
// ---------------------------------------------------------------------------

/// `GET /groups/{id}/lines/custom` -- every custom line granted into this
/// group. Any current member may see the list (design §3.3); mere
/// membership is the whole permission model, so `require_member` is the
/// entire gate, exactly as for `list_group_trains_route`.
async fn list_group_custom_lines_route(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
) -> Result<Json<Vec<groups::GroupCustomLine>>, (StatusCode, String)> {
    require_member(&app, &group_id, &user.id).await?;

    let lines = groups::list_group_custom_lines(&app.database, &group_id)
        .await
        .map_err(internal_error("list group custom lines"))?;
    Ok(Json(lines))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddCustomLineGrantRequest {
    line_id: String,
}

/// `POST /groups/{id}/lines/custom` -- share one of the caller's OWN
/// custom lines into this group.
///
/// Two independent gates, in this order: the caller must be a current
/// member (`require_member`, `404` otherwise -- a non-member has no
/// legitimate claim to know the group exists), and they must OWN the line
/// (`groups::grant_custom_line`'s own `WHERE id = $1 AND user_id = $2`).
/// The second is deliberately not relaxed for an `admin`/`owner` of the
/// group: a group's management structure has no standing over a member's
/// private custom line, and letting it decide that line's visibility would
/// be the first ownership exception this resource has ever had (design
/// §2.3).
///
/// "No such line" and "exists, but isn't yours" are the same `404` with
/// `get_line`'s own message, never `403` and never `400` -- an outside
/// caller must not be able to tell the two apart here any more than they
/// can at `GET /public/lines/{id}`.
async fn add_custom_line_grant(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(group_id): Path<String>,
    Json(req): Json<AddCustomLineGrantRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    require_member(&app, &group_id, &user.id).await?;

    let granted = groups::grant_custom_line(&app.database, &group_id, &req.line_id, &user.id)
        .await
        .map_err(internal_error("grant custom line to group"))?;
    if !granted {
        return Err((StatusCode::NOT_FOUND, "custom line not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `DELETE /groups/{id}/lines/custom/{lineId}` -- revoke a grant. The
/// member who granted it, or any `admin`/`owner` of the group (design
/// §2.4, the same sharer-or-manager rule `remove_group_train` uses).
///
/// This is the ONE route in this file that deliberately does not call
/// `require_member`, and the reason is design §2.7: a grant deliberately
/// SURVIVES its granter leaving the group, and that decision is only
/// defensible because the departed owner keeps the ability to revoke it
/// themselves. `require_member` would `404` them and quietly strip that
/// ability away, leaving a line shared into a group its owner can no
/// longer reach. So: resolve the caller's role as an `Option`, treat "not
/// a member" as simply "not a manager", and let
/// `remove_custom_line_grant`'s own `granted_by = $3` branch be the gate.
///
/// This leaks nothing. A caller who is neither a manager of the group nor
/// the granter deletes zero rows and gets `404 "no shared custom line with
/// that id"` -- identical to what a member targeting an unknown grant
/// gets, and identical to what a total stranger guessing a group id gets.
async fn remove_custom_line_grant_route(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((group_id, line_id)): Path<(String, String)>,
) -> Result<StatusCode, (StatusCode, String)> {
    let role = groups::get_member_role(&app.database, &group_id, &user.id)
        .await
        .map_err(internal_error("check group membership"))?;

    let removed = groups::remove_custom_line_grant(
        &app.database,
        &group_id,
        &line_id,
        &user.id,
        role.is_some_and(GroupRole::can_manage),
    )
    .await
    .map_err(internal_error("remove custom line grant"))?;
    if !removed {
        return Err((
            StatusCode::NOT_FOUND,
            "no shared custom line with that id".to_string(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /groups/shared-custom-lines` -- every custom line OTHER members
/// have granted into any group the caller belongs to, minus the caller's
/// own lines, each tagged with the group it came from and who shared it.
/// Feeds the home page's "Lines shared with you" section.
///
/// No group id in the path and so no `require_member` gate, for exactly
/// the reason `list_shared_trains_route` has none: the caller's own
/// membership rows ARE the scope of
/// `groups::list_shared_custom_lines_for_user`'s query, so a non-member
/// simply gets nothing rather than a `404`. Membership in zero groups and
/// membership in groups with nothing shared into them are both an empty
/// array -- the same "no signal about groups you can't see" posture the
/// rest of this file keeps.
async fn list_shared_custom_lines_route(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<groups::SharedCustomLine>>, (StatusCode, String)> {
    let lines = groups::list_shared_custom_lines_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list shared custom lines"))?;
    Ok(Json(lines))
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
            .route(
                "/groups/join/{token}",
                axum::routing::get(|| async { "join" }),
            )
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

    /// The same precedence check for `/groups/shared-trains`, the second
    /// literal segment this file registers at `/groups/{id}`'s dynamic
    /// position -- same hand-rolled two-route shape as the `join` test
    /// directly above (driving this file's real `router()` would need a
    /// whole `App`, which `db_tests` below builds and which this
    /// database-free routing question has no need of).
    #[tokio::test]
    async fn shared_trains_literal_route_wins_over_same_position_dynamic_id_route() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = axum::Router::new()
            .route(
                "/groups/shared-trains",
                axum::routing::get(|| async { "shared" }),
            )
            .route("/groups/{id}", axum::routing::get(|| async { "dynamic" }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/groups/shared-trains")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"shared");
    }

    /// The same precedence check for `/groups/shared-custom-lines`, the
    /// third literal segment this file registers at `/groups/{id}`'s
    /// dynamic position. Same hand-rolled two-route shape as its two
    /// siblings above, for the same reason.
    #[tokio::test]
    async fn shared_custom_lines_literal_route_wins_over_same_position_dynamic_id_route() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = axum::Router::new()
            .route(
                "/groups/shared-custom-lines",
                axum::routing::get(|| async { "shared-lines" }),
            )
            .route("/groups/{id}", axum::routing::get(|| async { "dynamic" }));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/groups/shared-custom-lines")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&body[..], b"shared-lines");
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

    /// Seeds one bare tracked train owned by `user_id` and returns its id
    /// -- same minimal column set `crate::data::groups::db_tests`' own
    /// helper of this name inserts, for the same reason (nothing under
    /// test here reads anything but the pin's origin).
    async fn seed_train_subscription(pool: &PgPool, user_id: &str) -> i64 {
        let row: (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, CURRENT_DATE, 'WOK', NOW()) RETURNING id",
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .expect("seed a tracked train");
        row.0
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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                shared_trains_returns_a_fellow_members_shared_train_and_401s_without_a_session \
                -- --ignored --test-threads=1`"]
    async fn shared_trains_returns_a_fellow_members_shared_train_and_401s_without_a_session() {
        let pool = connect().await;
        seed_session(&pool, "TEST-ROUTE-GROUPS-SHARED-SHARER").await;
        let viewer_token = seed_session(&pool, "TEST-ROUTE-GROUPS-SHARED-VIEWER").await;

        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Test Shared Trains",
            "TEST-ROUTE-GROUPS-SHARED-SHARER",
        )
        .await
        .expect("create fixture group");
        seed_membership(
            &pool,
            &group_id,
            "TEST-ROUTE-GROUPS-SHARED-VIEWER",
            "member",
        )
        .await;
        let train_id = seed_train_subscription(&pool, "TEST-ROUTE-GROUPS-SHARED-SHARER").await;
        crate::data::groups::add_train_to_group(
            &pool,
            &group_id,
            train_id,
            "TEST-ROUTE-GROUPS-SHARED-SHARER",
        )
        .await
        .expect("share fixture train");

        let router = test_router(test_app(pool.clone()));

        // The literal route really is reachable through the REAL router
        // (not just matchit in the abstract), and the viewer really does
        // receive a train they never tracked themselves, tagged with the
        // group it came from and who shared it.
        let (status, body) = request(
            router.clone(),
            "/groups/shared-trains".to_string(),
            Some(&viewer_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array of shared trains");
        assert_eq!(rows.len(), 1, "got {body:?}");
        assert_eq!(
            rows[0].get("trainSubscriptionId").and_then(Value::as_i64),
            Some(train_id)
        );
        assert_eq!(
            rows[0].get("groupId").and_then(Value::as_str),
            Some(group_id.as_str())
        );
        assert_eq!(
            rows[0].get("groupName").and_then(Value::as_str),
            Some("Route Test Shared Trains")
        );
        assert_eq!(
            rows[0].get("addedByName").and_then(Value::as_str),
            Some("TEST-ROUTE-GROUPS-SHARED-SHARER")
        );
        assert!(
            rows[0].get("tickets").is_none() && rows[0].get("notificationsEnabled").is_none(),
            "spec §4's never-shown fields must not appear on this route either"
        );

        // No session at all: `AuthenticatedUser` refuses before any of the
        // above can be reached -- the frontend's own `null`-on-401
        // "anonymous visitor" signal depends on this being a 401, not an
        // empty 200.
        let (anon_status, _anon_body) =
            request(router, "/groups/shared-trains".to_string(), None).await;
        assert_eq!(anon_status, StatusCode::UNAUTHORIZED);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .expect("cleanup fixture train");
        cleanup(
            &pool,
            &group_id,
            &[
                "TEST-ROUTE-GROUPS-SHARED-SHARER",
                "TEST-ROUTE-GROUPS-SHARED-VIEWER",
            ],
        )
        .await;
    }

    // ------------------------------------------------------------------
    // Custom-line group grants. The permission questions below are ones a
    // data-layer test cannot answer, because they are about which callers
    // the HANDLER lets reach the query at all.
    // ------------------------------------------------------------------

    /// Seeds one custom line owned by `user_id` through the real write
    /// path, so the row is exactly what `POST /public/lines` produces.
    async fn seed_custom_line(pool: &PgPool, user_id: &str, name: &str) -> String {
        crate::data::custom_lines::insert_custom_line(
            pool,
            crate::data::custom_lines::NewCustomLine {
                name: name.to_string(),
                operators: vec!["SW".to_string()],
                stations: vec!["WOK".to_string(), "CLJ".to_string()],
                headcode_prefixes: vec![],
                destination_crs_filter: vec![],
            },
            user_id,
        )
        .await
        .expect("seed a custom line")
        .id
    }

    async fn count_grants(pool: &PgPool, group_id: &str) -> i64 {
        let row: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM custom_line_group_grants WHERE group_id = $1")
                .bind(group_id)
                .fetch_one(pool)
                .await
                .expect("count grants");
        row.0
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_custom_line_grant_refuses_a_line_the_caller_does_not_own -- --ignored \
                --test-threads=1`"]
    async fn add_custom_line_grant_refuses_a_line_the_caller_does_not_own() {
        // The group's OWNER -- maximally privileged inside the group --
        // attempting to share a plain member's private line. 404, and no
        // row written: a group's management structure has no standing over
        // a member's custom line (design §2.3).
        let pool = connect().await;
        let owner_token = seed_session(&pool, "TEST-ROUTE-GRANT-GOWNER-1").await;
        seed_session(&pool, "TEST-ROUTE-GRANT-MEMBER-1").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Grant Test 1",
            "TEST-ROUTE-GRANT-GOWNER-1",
        )
        .await
        .expect("create fixture group");
        seed_membership(&pool, &group_id, "TEST-ROUTE-GRANT-MEMBER-1", "member").await;
        let line_id = seed_custom_line(&pool, "TEST-ROUTE-GRANT-MEMBER-1", "Route Grant 1").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = post_json(
            router,
            format!("/groups/{group_id}/lines/custom"),
            Some(&owner_token),
            Some(serde_json::json!({ "lineId": line_id })),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            body,
            Value::String("custom line not found".to_string()),
            "the same message GET /public/lines/{{id}} uses, so 'no such line' and \
             'exists, not yours' stay indistinguishable"
        );
        assert_eq!(count_grants(&pool, &group_id).await, 0);

        cleanup(
            &pool,
            &group_id,
            &["TEST-ROUTE-GRANT-GOWNER-1", "TEST-ROUTE-GRANT-MEMBER-1"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_custom_line_grant_refuses_a_caller_who_is_not_a_member -- --ignored \
                --test-threads=1`"]
    async fn add_custom_line_grant_refuses_a_caller_who_is_not_a_member() {
        // Owning the line is not enough -- you have to be in the group.
        let pool = connect().await;
        seed_session(&pool, "TEST-ROUTE-GRANT-GOWNER-2").await;
        let outsider_token = seed_session(&pool, "TEST-ROUTE-GRANT-OUTSIDER-2").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Grant Test 2",
            "TEST-ROUTE-GRANT-GOWNER-2",
        )
        .await
        .expect("create fixture group");
        let line_id = seed_custom_line(&pool, "TEST-ROUTE-GRANT-OUTSIDER-2", "Route Grant 2").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = post_json(
            router,
            format!("/groups/{group_id}/lines/custom"),
            Some(&outsider_token),
            Some(serde_json::json!({ "lineId": line_id })),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no group with that id".to_string()));
        assert_eq!(count_grants(&pool, &group_id).await, 0);

        cleanup(
            &pool,
            &group_id,
            &["TEST-ROUTE-GRANT-GOWNER-2", "TEST-ROUTE-GRANT-OUTSIDER-2"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_custom_line_grant_route_gates_on_sharer_or_manager -- --ignored \
                --test-threads=1`"]
    async fn remove_custom_line_grant_route_gates_on_sharer_or_manager() {
        let pool = connect().await;
        seed_session(&pool, "TEST-ROUTE-GRANT-RM-GOWNER").await;
        let sharer_token = seed_session(&pool, "TEST-ROUTE-GRANT-RM-SHARER").await;
        let bystander_token = seed_session(&pool, "TEST-ROUTE-GRANT-RM-BYSTANDER").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Grant Remove",
            "TEST-ROUTE-GRANT-RM-GOWNER",
        )
        .await
        .expect("create fixture group");
        for member in [
            "TEST-ROUTE-GRANT-RM-SHARER",
            "TEST-ROUTE-GRANT-RM-BYSTANDER",
        ] {
            seed_membership(&pool, &group_id, member, "member").await;
        }
        let line_id = seed_custom_line(&pool, "TEST-ROUTE-GRANT-RM-SHARER", "Route Grant RM").await;
        crate::data::groups::grant_custom_line(
            &pool,
            &group_id,
            &line_id,
            "TEST-ROUTE-GRANT-RM-SHARER",
        )
        .await
        .expect("seed fixture grant");

        let router = test_router(test_app(pool.clone()));
        let (status, _) = delete_request(
            router.clone(),
            format!("/groups/{group_id}/lines/custom/{line_id}"),
            Some(&bystander_token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(
            count_grants(&pool, &group_id).await,
            1,
            "the refused revoke must not have taken effect"
        );

        let (status, _) = delete_request(
            router,
            format!("/groups/{group_id}/lines/custom/{line_id}"),
            Some(&sharer_token),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(count_grants(&pool, &group_id).await, 0);

        cleanup(
            &pool,
            &group_id,
            &[
                "TEST-ROUTE-GRANT-RM-GOWNER",
                "TEST-ROUTE-GRANT-RM-SHARER",
                "TEST-ROUTE-GRANT-RM-BYSTANDER",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_custom_line_grant_route_still_works_for_a_granter_who_left_the_group \
                -- --ignored --test-threads=1`"]
    async fn remove_custom_line_grant_route_still_works_for_a_granter_who_left_the_group() {
        // The HTTP-level proof of design §2.7's load-bearing claim: the
        // grant survives its granter leaving, and that is only acceptable
        // because the granter can still revoke it afterwards. A
        // `require_member` gate on this route would silently break exactly
        // that -- which is why this handler deliberately has none.
        let pool = connect().await;
        seed_session(&pool, "TEST-ROUTE-GRANT-LEFT-GOWNER").await;
        let sharer_token = seed_session(&pool, "TEST-ROUTE-GRANT-LEFT-SHARER").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Grant Left",
            "TEST-ROUTE-GRANT-LEFT-GOWNER",
        )
        .await
        .expect("create fixture group");
        seed_membership(&pool, &group_id, "TEST-ROUTE-GRANT-LEFT-SHARER", "member").await;
        let line_id =
            seed_custom_line(&pool, "TEST-ROUTE-GRANT-LEFT-SHARER", "Route Grant Left").await;
        crate::data::groups::grant_custom_line(
            &pool,
            &group_id,
            &line_id,
            "TEST-ROUTE-GRANT-LEFT-SHARER",
        )
        .await
        .expect("seed fixture grant");

        crate::data::groups::remove_member(&pool, &group_id, "TEST-ROUTE-GRANT-LEFT-SHARER")
            .await
            .expect("the sharer leaves the group");
        assert_eq!(
            count_grants(&pool, &group_id).await,
            1,
            "precondition (§2.7): the grant survives the granter leaving"
        );

        let router = test_router(test_app(pool.clone()));
        let (status, _) = delete_request(
            router,
            format!("/groups/{group_id}/lines/custom/{line_id}"),
            Some(&sharer_token),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        assert_eq!(count_grants(&pool, &group_id).await, 0);

        cleanup(
            &pool,
            &group_id,
            &[
                "TEST-ROUTE-GRANT-LEFT-GOWNER",
                "TEST-ROUTE-GRANT-LEFT-SHARER",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                custom_line_grant_list_routes_are_member_scoped -- --ignored --test-threads=1`"]
    async fn custom_line_grant_list_routes_are_member_scoped() {
        let pool = connect().await;
        let sharer_token = seed_session(&pool, "TEST-ROUTE-GRANT-LIST-SHARER").await;
        let viewer_token = seed_session(&pool, "TEST-ROUTE-GRANT-LIST-VIEWER").await;
        let stranger_token = seed_session(&pool, "TEST-ROUTE-GRANT-LIST-STRANGER").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Route Grant List",
            "TEST-ROUTE-GRANT-LIST-SHARER",
        )
        .await
        .expect("create fixture group");
        seed_membership(&pool, &group_id, "TEST-ROUTE-GRANT-LIST-VIEWER", "member").await;
        let line_id =
            seed_custom_line(&pool, "TEST-ROUTE-GRANT-LIST-SHARER", "Route Grant List").await;
        crate::data::groups::grant_custom_line(
            &pool,
            &group_id,
            &line_id,
            "TEST-ROUTE-GRANT-LIST-SHARER",
        )
        .await
        .expect("seed fixture grant");

        let router = test_router(test_app(pool.clone()));

        // A member sees the group's granted lines.
        let (status, body) = request(
            router.clone(),
            format!("/groups/{group_id}/lines/custom"),
            Some(&viewer_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1, "got {body:?}");
        assert_eq!(
            rows[0].get("lineId").and_then(Value::as_str),
            Some(line_id.as_str())
        );
        assert_eq!(
            rows[0].get("grantedByName").and_then(Value::as_str),
            Some("TEST-ROUTE-GRANT-LIST-SHARER")
        );

        // A non-member gets the group's usual 404, not an empty list.
        let (status, _) = request(
            router.clone(),
            format!("/groups/{group_id}/lines/custom"),
            Some(&stranger_token),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);

        // The caller-scoped list: the viewer gets the shared line tagged
        // with its group; the sharer does NOT get their own line back;
        // the stranger gets nothing; an anonymous caller gets 401.
        let (status, body) = request(
            router.clone(),
            "/groups/shared-custom-lines".to_string(),
            Some(&viewer_token),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let rows = body.as_array().expect("an array");
        assert_eq!(rows.len(), 1, "got {body:?}");
        assert_eq!(
            rows[0].get("groupName").and_then(Value::as_str),
            Some("Route Grant List")
        );
        assert_eq!(
            rows[0].get("lineName").and_then(Value::as_str),
            Some("Route Grant List")
        );

        let (_, body) = request(
            router.clone(),
            "/groups/shared-custom-lines".to_string(),
            Some(&sharer_token),
        )
        .await;
        assert_eq!(
            body.as_array().map(Vec::len),
            Some(0),
            "a caller's own custom line must never come back as a shared one: {body:?}"
        );

        let (_, body) = request(
            router.clone(),
            "/groups/shared-custom-lines".to_string(),
            Some(&stranger_token),
        )
        .await;
        assert_eq!(body.as_array().map(Vec::len), Some(0), "got {body:?}");

        let (status, _) = request(router, "/groups/shared-custom-lines".to_string(), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        cleanup(
            &pool,
            &group_id,
            &[
                "TEST-ROUTE-GRANT-LIST-SHARER",
                "TEST-ROUTE-GRANT-LIST-VIEWER",
                "TEST-ROUTE-GRANT-LIST-STRANGER",
            ],
        )
        .await;
    }
}
