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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_builds_without_panicking() {
        let _ = router();
    }
}
