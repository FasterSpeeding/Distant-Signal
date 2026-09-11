//! CRUD + permissions for shared groups (`groups`, `group_members`,
//! `group_trains`, `group_invite_links`). See
//! docs/superpowers/specs/2026-09-11-shared-groups-design.md.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// The three-tier role stored in `group_members.role` -- see the design
/// doc's §2.1 for why there are three tiers, not two: the creator is a
/// PERMANENT `owner`, distinct from a promotable `admin`, so an `admin`
/// can never remove/demote/act on the `owner` row itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupRole {
    Owner,
    Admin,
    Member,
}

impl GroupRole {
    fn from_db(raw: &str) -> Self {
        match raw {
            "owner" => GroupRole::Owner,
            "admin" => GroupRole::Admin,
            _ => GroupRole::Member,
        }
    }

    /// `admin`/`owner` share every day-to-day management power (§3):
    /// invite-link management, member removal, renaming.
    pub fn can_manage(self) -> bool {
        matches!(self, GroupRole::Owner | GroupRole::Admin)
    }

    /// Promoting a member to `admin` and deleting the group outright are
    /// the two actions reserved for `owner` alone (§3).
    pub fn is_owner(self) -> bool {
        matches!(self, GroupRole::Owner)
    }
}

#[cfg(test)]
mod role_tests {
    use super::*;

    #[test]
    fn owner_and_admin_can_manage_but_member_cannot() {
        assert!(GroupRole::Owner.can_manage());
        assert!(GroupRole::Admin.can_manage());
        assert!(!GroupRole::Member.can_manage());
    }

    #[test]
    fn only_owner_is_owner() {
        assert!(GroupRole::Owner.is_owner());
        assert!(!GroupRole::Admin.is_owner());
        assert!(!GroupRole::Member.is_owner());
    }

    #[test]
    fn from_db_maps_every_known_value_and_defaults_unknown_to_member() {
        assert_eq!(GroupRole::from_db("owner"), GroupRole::Owner);
        assert_eq!(GroupRole::from_db("admin"), GroupRole::Admin);
        assert_eq!(GroupRole::from_db("member"), GroupRole::Member);
        // The DB's own CHECK constraint (migration Task 1) already rejects
        // anything else at write time; this defends read-side decoding
        // against a value this crate didn't write (a manual DB edit, a
        // future migration bug) rather than panicking on it.
        assert_eq!(GroupRole::from_db("something-else"), GroupRole::Member);
    }
}

pub async fn create_group(pool: &PgPool, name: &str, user_id: &str) -> Result<String> {
    let id = crate::auth::generate_session_token();
    let mut tx = pool.begin().await?;
    sqlx::query("INSERT INTO groups (id, name, created_by, created_at) VALUES ($1, $2, $3, NOW())")
        .bind(&id)
        .bind(name)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, joined_at) VALUES ($1, $2, 'owner', NOW())",
    )
    .bind(&id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupSummary {
    pub id: String,
    pub name: String,
    pub role: GroupRole,
    pub member_count: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupSummaryRow {
    id: String,
    name: String,
    role: String,
    member_count: i64,
}

pub async fn list_groups_for_user(pool: &PgPool, user_id: &str) -> Result<Vec<GroupSummary>> {
    let rows: Vec<GroupSummaryRow> = sqlx::query_as(
        "SELECT g.id, g.name, gm.role, \
                (SELECT COUNT(*) FROM group_members gm2 WHERE gm2.group_id = g.id) AS member_count \
         FROM groups g \
         JOIN group_members gm ON gm.group_id = g.id \
         WHERE gm.user_id = $1 \
         ORDER BY g.created_at",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| GroupSummary {
            id: r.id,
            name: r.name,
            role: GroupRole::from_db(&r.role),
            member_count: r.member_count,
        })
        .collect())
}

/// The calling user's role in a group, or `None` if they aren't a member
/// (including a group that doesn't exist at all) -- every permission
/// check in `crate::routes::groups` funnels through this, mirroring
/// `train_tracking::tracked_train_owner`'s "one lookup, many call sites"
/// shape.
pub async fn get_member_role(
    pool: &PgPool,
    group_id: &str,
    user_id: &str,
) -> Result<Option<GroupRole>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2")
            .bind(group_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(role,)| GroupRole::from_db(&role)))
}

#[derive(Debug, Clone)]
pub struct GroupDetail {
    pub id: String,
    pub name: String,
    pub owner_id: String,
    pub owner_name: Option<String>,
    pub member_count: i64,
    pub role: GroupRole,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupDetailRow {
    id: String,
    name: String,
    owner_id: String,
    owner_name: Option<String>,
    member_count: i64,
    role: String,
}

/// `None` unless `user_id` is a member of `group_id` -- this app's
/// universal "exists but not yours" 404 convention (never `403` for
/// "doesn't exist or isn't yours" -- see `train_tracking::tracked_train_owner`).
pub async fn get_group_detail(
    pool: &PgPool,
    group_id: &str,
    user_id: &str,
) -> Result<Option<GroupDetail>> {
    let row: Option<GroupDetailRow> = sqlx::query_as(
        "SELECT g.id, g.name, \
                owner_m.user_id AS owner_id, owner_u.name AS owner_name, \
                (SELECT COUNT(*) FROM group_members gm2 WHERE gm2.group_id = g.id) AS member_count, \
                caller.role AS role \
         FROM groups g \
         JOIN group_members caller ON caller.group_id = g.id AND caller.user_id = $2 \
         JOIN group_members owner_m ON owner_m.group_id = g.id AND owner_m.role = 'owner' \
         JOIN users owner_u ON owner_u.id = owner_m.user_id \
         WHERE g.id = $1",
    )
    .bind(group_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|r| GroupDetail {
        id: r.id,
        name: r.name,
        owner_id: r.owner_id,
        owner_name: r.owner_name,
        member_count: r.member_count,
        role: GroupRole::from_db(&r.role),
    }))
}

/// `false` if no group has that id -- the route maps this to `404`.
/// Permission checking (only `admin`/`owner` may rename) is the caller's
/// job, same split as `custom_lines::update_custom_line`.
pub async fn rename_group(pool: &PgPool, group_id: &str, new_name: &str) -> Result<bool> {
    let result = sqlx::query("UPDATE groups SET name = $2 WHERE id = $1")
        .bind(group_id)
        .bind(new_name)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Deletes a group outright. `ON DELETE CASCADE` on `group_members`/
/// `group_trains`/`group_invite_links` (Task 1's migration) means nothing
/// else needs deleting here -- same "the FK graph does the cleanup" shape
/// as `train_tracking::delete_tracked_train`.
pub async fn delete_group(pool: &PgPool, group_id: &str) -> Result<bool> {
    let result = sqlx::query("DELETE FROM groups WHERE id = $1")
        .bind(group_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupMemberRow {
    user_id: String,
    name: Option<String>,
    email: Option<String>,
    role: String,
    joined_at: DateTime<Utc>,
}

/// Deliberately no `email` field: `GET /groups/{id}/members` returns this
/// to every current member, and a joined-via-link member has no other
/// relationship with the rest of the group -- shipping their verified
/// email to everyone alongside `name` leaks more than the feature needs.
/// Collapsed to `displayName` the same way `GroupTrain.added_by_name`
/// already collapses `name.or(email)` for attribution, one struct away.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    pub user_id: String,
    pub display_name: Option<String>,
    pub role: GroupRole,
    pub joined_at: DateTime<Utc>,
}

impl From<GroupMemberRow> for GroupMember {
    fn from(row: GroupMemberRow) -> Self {
        GroupMember {
            user_id: row.user_id,
            display_name: row.name.or(row.email),
            role: GroupRole::from_db(&row.role),
            joined_at: row.joined_at,
        }
    }
}

/// Every member of `group_id`, oldest-joined first (so the owner -- always
/// the earliest row, since they're inserted at group-creation time -- sorts
/// to the top). No permission check here: any current member may view the
/// full member list (spec §3) -- the route's own `get_member_role` call
/// gates "is the caller even a member at all."
pub async fn list_members(pool: &PgPool, group_id: &str) -> Result<Vec<GroupMember>> {
    let rows: Vec<GroupMemberRow> = sqlx::query_as(
        "SELECT gm.user_id, u.name, u.email, gm.role, gm.joined_at \
         FROM group_members gm \
         JOIN users u ON u.id = gm.user_id \
         WHERE gm.group_id = $1 \
         ORDER BY gm.joined_at",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(GroupMember::from).collect())
}

/// Promotes a plain `member` to `admin`. A no-op (`false`) if the target
/// isn't currently a plain `member` -- already `admin` or `owner`, or not
/// a member at all -- so this can never accidentally "promote" the owner
/// row itself (its `role` is never `'member'`). Permission checking (only
/// `owner` may promote, per spec §3) is the route's job.
pub async fn promote_to_admin(pool: &PgPool, group_id: &str, target_user_id: &str) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE group_members SET role = 'admin' \
         WHERE group_id = $1 AND user_id = $2 AND role = 'member'",
    )
    .bind(group_id)
    .bind(target_user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoveMemberOutcome {
    /// The target wasn't a member of this group at all -- the route maps
    /// this to `404`.
    NotAMember,
    /// The target was removed. `new_owner` is `Some` exactly when the
    /// removed member was the `owner` and the group still has members left
    /// (§2.1's auto-promotion), `None` otherwise.
    Removed { new_owner: Option<String> },
    /// The target was the `owner` and the only remaining member -- the
    /// whole group was deleted instead of leaving an ownerless, empty
    /// group (§2.1: "An owner attempting to leave a group with no other
    /// members present simply deletes the group instead").
    GroupDeleted,
}

/// Removes `target_user_id` from `group_id`, handling every decided edge
/// case from spec §2.1/§2.2 in one transaction:
///
/// 1. Departed-member cleanup: the target's `group_trains` rows
///    (`added_by = target_user_id`) are deleted in the SAME transaction as
///    the membership removal, so a departed member's shared train never
///    lingers attributed to someone no longer in the group.
/// 2. If the target is the `owner` and other members remain, ownership
///    transfers to the longest-standing remaining `admin` (by
///    `joined_at`), or if none exists, the longest-standing remaining
///    `member`.
/// 3. If the target is the `owner` and NO other members remain, the whole
///    group is deleted instead of leaving it ownerless-and-empty.
///
/// Permission checking (self-leave is always allowed; removing someone
/// else requires `admin`/`owner` and can never target the `owner`) is the
/// route's job (Task 7) -- this function only encodes what happens to the
/// DATA once a removal is authorized.
pub async fn remove_member(
    pool: &PgPool,
    group_id: &str,
    target_user_id: &str,
) -> Result<RemoveMemberOutcome> {
    let mut tx = pool.begin().await?;

    let target_role: Option<String> =
        sqlx::query_scalar("SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2")
            .bind(group_id)
            .bind(target_user_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some(target_role) = target_role else {
        tx.rollback().await?;
        return Ok(RemoveMemberOutcome::NotAMember);
    };

    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM group_members WHERE group_id = $1 AND user_id != $2",
    )
    .bind(group_id)
    .bind(target_user_id)
    .fetch_one(&mut *tx)
    .await?;

    if target_role == "owner" && remaining == 0 {
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        return Ok(RemoveMemberOutcome::GroupDeleted);
    }

    // Departed-member cleanup (§2.2, decided) -- same transaction as the
    // removal below.
    sqlx::query("DELETE FROM group_trains WHERE group_id = $1 AND added_by = $2")
        .bind(group_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await?;

    sqlx::query("DELETE FROM group_members WHERE group_id = $1 AND user_id = $2")
        .bind(group_id)
        .bind(target_user_id)
        .execute(&mut *tx)
        .await?;

    let mut new_owner = None;
    if target_role == "owner" {
        // Longest-standing remaining admin, or failing that, longest-
        // standing remaining member (§2.1). `(role = 'admin') DESC` sorts
        // every admin ahead of every member; `joined_at ASC` within each
        // group picks the earliest-joined (longest-standing) row.
        // `remaining > 0` (checked above) guarantees this finds a row.
        let successor: Option<(String,)> = sqlx::query_as(
            "SELECT user_id FROM group_members \
             WHERE group_id = $1 \
             ORDER BY (role = 'admin') DESC, joined_at ASC \
             LIMIT 1",
        )
        .bind(group_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((successor_id,)) = successor {
            sqlx::query("UPDATE group_members SET role = 'owner' WHERE group_id = $1 AND user_id = $2")
                .bind(group_id)
                .bind(&successor_id)
                .execute(&mut *tx)
                .await?;
            new_owner = Some(successor_id);
        }
    }

    tx.commit().await?;
    Ok(RemoveMemberOutcome::Removed { new_owner })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteLink {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

/// Every link expires 7 days after creation unless rotated (spec §2.3,
/// decided) -- a low-effort mitigation against an old, forgotten, still-
/// valid link being found and reused much later.
const INVITE_LINK_TTL: chrono::Duration = chrono::Duration::days(7);

#[derive(Debug, Clone, sqlx::FromRow)]
struct InviteLinkRow {
    token: String,
    expires_at: DateTime<Utc>,
}

/// Rotates the group's active invite link: revokes any currently-active
/// link and inserts a fresh one with a new 7-day expiry, in one
/// transaction (spec §2.3: "rotation and 'extend the window' are the same
/// action"). Permission checking (only `admin`/`owner`) is the route's job.
pub async fn rotate_invite_link(pool: &PgPool, group_id: &str, user_id: &str) -> Result<InviteLink> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE group_invite_links SET revoked_at = NOW() \
         WHERE group_id = $1 AND revoked_at IS NULL",
    )
    .bind(group_id)
    .execute(&mut *tx)
    .await?;

    let token = crate::auth::generate_session_token();
    let expires_at = Utc::now() + INVITE_LINK_TTL;
    sqlx::query(
        "INSERT INTO group_invite_links (token, group_id, created_by, created_at, expires_at) \
         VALUES ($1, $2, $3, NOW(), $4)",
    )
    .bind(&token)
    .bind(group_id)
    .bind(user_id)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(InviteLink { token, expires_at })
}

/// Revokes the group's active invite link with no replacement.
/// Idempotent: `false` if there was nothing active to revoke -- the route
/// still returns `204` either way (revoking an already-revoked/expired
/// link is not an error).
pub async fn revoke_invite_link(pool: &PgPool, group_id: &str) -> Result<bool> {
    let result = sqlx::query(
        "UPDATE group_invite_links SET revoked_at = NOW() \
         WHERE group_id = $1 AND revoked_at IS NULL",
    )
    .bind(group_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// The group's current active invite link, if any -- `None` once revoked
/// or past its `expires_at`. Surfaced on `GET /groups/{id}` for
/// `admin`/`owner` callers only (Task 6) -- there is no dedicated `GET`
/// route for this in the spec's API table (§5 lists only the two mutating
/// invite-link routes), so `GET /groups/{id}`'s own response is extended
/// to carry it; see this plan's self-review note on that extension.
pub async fn get_active_invite_link(pool: &PgPool, group_id: &str) -> Result<Option<InviteLink>> {
    let row: Option<InviteLinkRow> = sqlx::query_as(
        "SELECT token, expires_at FROM group_invite_links \
         WHERE group_id = $1 AND revoked_at IS NULL AND expires_at > NOW() \
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(group_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| InviteLink {
        token: r.token,
        expires_at: r.expires_at,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JoinPreview {
    pub group_id: String,
    pub group_name: String,
    pub member_count: i64,
}

/// Resolves a join token to a group preview for the confirm-before-join
/// page (spec §2.3) -- valid iff `revoked_at IS NULL AND expires_at >
/// NOW()`. Never changes membership; see `consume_invite_link` for the
/// actual join. Unauthenticated: the route calling this takes no
/// `AuthenticatedUser` at all, so a not-yet-logged-in visitor can see what
/// they're being asked to join before being sent through login.
pub async fn resolve_invite_link(pool: &PgPool, token: &str) -> Result<Option<JoinPreview>> {
    let row: Option<(String, String, i64)> = sqlx::query_as(
        "SELECT g.id, g.name, (SELECT COUNT(*) FROM group_members gm WHERE gm.group_id = g.id) \
         FROM group_invite_links l \
         JOIN groups g ON g.id = l.group_id \
         WHERE l.token = $1 AND l.revoked_at IS NULL AND l.expires_at > NOW()",
    )
    .bind(token)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(group_id, group_name, member_count)| JoinPreview {
        group_id,
        group_name,
        member_count,
    }))
}

/// Consumes a join token: adds the caller to `group_members` as a plain
/// `member` if the token is still valid, or no-ops if they're already a
/// member (e.g. the owner re-clicking their own link, or a double-submit)
/// -- `ON CONFLICT DO NOTHING` on the natural `(group_id, user_id)` PK.
/// Returns the joined `group_id`, or `None` if the token doesn't resolve
/// to a valid, unexpired, unrevoked link -- the route maps that to `404`.
pub async fn consume_invite_link(
    pool: &PgPool,
    token: &str,
    user_id: &str,
) -> Result<Option<String>> {
    let mut tx = pool.begin().await?;
    let group_id: Option<String> = sqlx::query_scalar(
        "SELECT group_id FROM group_invite_links \
         WHERE token = $1 AND revoked_at IS NULL AND expires_at > NOW()",
    )
    .bind(token)
    .fetch_optional(&mut *tx)
    .await?;
    let Some(group_id) = group_id else {
        tx.rollback().await?;
        return Ok(None);
    };

    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role, joined_at) \
         VALUES ($1, $2, 'member', NOW()) \
         ON CONFLICT (group_id, user_id) DO NOTHING",
    )
    .bind(&group_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(group_id))
}

/// Adds one of the caller's own tracked trains to a group. Ownership is
/// enforced at the APPLICATION layer, not the DB (spec §2.2) -- the exact
/// `WHERE id = $1 AND user_id = $2` shape `train_tracking.rs` already uses
/// for ticket ownership. Idempotent: re-adding an already-shared train is
/// a silent no-op (`ON CONFLICT DO NOTHING`), matching
/// `insert_custom_line`'s own idempotent-insert precedent.
///
/// Returns `false` if `train_subscription_id` doesn't exist or isn't
/// owned by `user_id` -- the route maps this to `404`, never `403`.
pub async fn add_train_to_group(
    pool: &PgPool,
    group_id: &str,
    train_subscription_id: i64,
    user_id: &str,
) -> Result<bool> {
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM train_subscriptions WHERE id = $1 AND user_id = $2")
            .bind(train_subscription_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    if owned.is_none() {
        return Ok(false);
    }

    sqlx::query(
        "INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at) \
         VALUES ($1, $2, $3, NOW()) \
         ON CONFLICT (group_id, train_subscription_id) DO NOTHING",
    )
    .bind(group_id)
    .bind(train_subscription_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(true)
}

/// Removes a shared train from a group. `caller_can_manage` should be the
/// route's own already-resolved `GroupRole::can_manage()` for this caller
/// -- an `admin`/`owner` may remove ANY shared train; anyone else may only
/// remove a train THEY added (spec §3: "The member who added it, or any
/// admin/owner"). Returns `false` if no matching row was deleted (unknown
/// id, or a non-manager targeting someone else's shared train) -- the
/// route maps that to `404`.
pub async fn remove_train_from_group(
    pool: &PgPool,
    group_id: &str,
    train_subscription_id: i64,
    user_id: &str,
    caller_can_manage: bool,
) -> Result<bool> {
    let result = if caller_can_manage {
        sqlx::query("DELETE FROM group_trains WHERE group_id = $1 AND train_subscription_id = $2")
            .bind(group_id)
            .bind(train_subscription_id)
            .execute(pool)
            .await?
    } else {
        sqlx::query(
            "DELETE FROM group_trains \
             WHERE group_id = $1 AND train_subscription_id = $2 AND added_by = $3",
        )
        .bind(group_id)
        .bind(train_subscription_id)
        .bind(user_id)
        .execute(pool)
        .await?
    };
    Ok(result.rows_affected() > 0)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupTrainRow {
    train_subscription_id: i64,
    pin_origin_crs: Option<String>,
    pin_destination_crs: Option<String>,
    pin_origin_name: Option<String>,
    pin_destination_name: Option<String>,
    pin_scheduled_departure: Option<DateTime<Utc>>,
    service_date: chrono::NaiveDate,
    resolution_status: String,
    train_uid: Option<String>,
    status: Option<String>,
    delay_minutes: Option<i32>,
    custom_name: Option<String>,
    added_by: String,
    added_by_name: Option<String>,
    added_by_email: Option<String>,
}

/// A shared train's display shape for `GET /groups/{id}/trains`. Carries
/// exactly the fields `frontend/lib/trackingName.ts`'s
/// `trackedTrainDisplayName` needs to compute the tracker's default name
/// the same way the tracker themselves would see it (spec §4: "never
/// stored, always computed"), plus live status and attribution.
///
/// Deliberately carries NO ticket field, and NO `notificationsEnabled`/
/// exact `trackedAt` field -- spec §4's "Never shown" list. This is a
/// hard constraint: no future edit to this struct or to `list_group_trains`'s
/// query may join `tracked_train_tickets` or select
/// `train_subscriptions.notifications_enabled`/`tracked_at`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupTrain {
    pub train_subscription_id: i64,
    pub pin_origin_crs: Option<String>,
    pub pin_destination_crs: Option<String>,
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
    pub service_date: chrono::NaiveDate,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub custom_name: Option<String>,
    pub added_by: String,
    pub added_by_name: Option<String>,
}

impl From<GroupTrainRow> for GroupTrain {
    fn from(row: GroupTrainRow) -> Self {
        GroupTrain {
            train_subscription_id: row.train_subscription_id,
            pin_origin_crs: row.pin_origin_crs,
            pin_destination_crs: row.pin_destination_crs,
            pin_origin_name: row.pin_origin_name,
            pin_destination_name: row.pin_destination_name,
            pin_scheduled_departure: row.pin_scheduled_departure,
            service_date: row.service_date,
            resolution_status: row.resolution_status,
            train_uid: row.train_uid,
            status: row.status,
            delay_minutes: row.delay_minutes,
            custom_name: row.custom_name,
            added_by: row.added_by,
            // Same "name, else email, else nothing" order AuthStatus.tsx
            // already uses for its own nav-bar label -- never the raw
            // internal user_id alone.
            added_by_name: row.added_by_name.or(row.added_by_email),
        }
    }
}

/// Every train shared into `group_id`, oldest-shared first. No permission
/// check here -- the route's own `get_member_role` call gates "is the
/// caller even a member." See `GroupTrain`'s own doc comment for the
/// hard ticket/notification-privacy constraint this query must never
/// violate.
pub async fn list_group_trains(pool: &PgPool, group_id: &str) -> Result<Vec<GroupTrain>> {
    let rows: Vec<GroupTrainRow> = sqlx::query_as(
        "SELECT gt.train_subscription_id, \
                ts.pin_origin_crs, ts.pin_destination_crs, \
                so.name AS pin_origin_name, sd.name AS pin_destination_name, \
                ts.pin_scheduled_departure, ts.service_date, ts.resolution_status, \
                tr.train_uid, cs.status, cs.delay_minutes, ts.custom_name, \
                gt.added_by, u.name AS added_by_name, u.email AS added_by_email \
         FROM group_trains gt \
         JOIN train_subscriptions ts ON ts.id = gt.train_subscription_id \
         JOIN users u ON u.id = gt.added_by \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
         LEFT JOIN stations so ON so.crs = UPPER(ts.pin_origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(ts.pin_destination_crs) \
         WHERE gt.group_id = $1 \
         ORDER BY gt.added_at",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(GroupTrain::from).collect())
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn seed_user(pool: &PgPool, id: &str) {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(id)
        .bind(format!("{id}@example.com"))
        .bind(id)
        .execute(pool)
        .await
        .expect("seed fixture user");
    }

    async fn cleanup(pool: &PgPool, user_ids: &[&str]) {
        for id in user_ids {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .ok();
        }
    }

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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_group_inserts_the_creator_as_a_permanent_owner -- --ignored`"]
    async fn create_group_inserts_the_creator_as_a_permanent_owner() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-CREATE-OWNER").await;

        let group_id = create_group(&pool, "Test Family", "TEST-GROUPS-CREATE-OWNER")
            .await
            .expect("create group");

        let role = get_member_role(&pool, &group_id, "TEST-GROUPS-CREATE-OWNER")
            .await
            .expect("read role")
            .expect("creator should be a member");
        assert_eq!(role, GroupRole::Owner);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-CREATE-OWNER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_groups_for_user_returns_only_the_callers_own_groups -- --ignored`"]
    async fn list_groups_for_user_returns_only_the_callers_own_groups() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-LIST-A").await;
        seed_user(&pool, "TEST-GROUPS-LIST-B").await;

        let group_a = create_group(&pool, "A's group", "TEST-GROUPS-LIST-A")
            .await
            .expect("create group A");
        let group_b = create_group(&pool, "B's group", "TEST-GROUPS-LIST-B")
            .await
            .expect("create group B");

        let a_groups = list_groups_for_user(&pool, "TEST-GROUPS-LIST-A")
            .await
            .expect("list A's groups");
        assert_eq!(a_groups.len(), 1);
        assert_eq!(a_groups[0].id, group_a);
        assert_eq!(a_groups[0].role, GroupRole::Owner);
        assert_eq!(a_groups[0].member_count, 1);

        sqlx::query("DELETE FROM groups WHERE id = ANY($1)")
            .bind(vec![group_a, group_b])
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-LIST-A", "TEST-GROUPS-LIST-B"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                get_group_detail_returns_none_for_a_non_member -- --ignored`"]
    async fn get_group_detail_returns_none_for_a_non_member() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-DETAIL-OWNER").await;
        seed_user(&pool, "TEST-GROUPS-DETAIL-OUTSIDER").await;
        let group_id = create_group(&pool, "Detail Test", "TEST-GROUPS-DETAIL-OWNER")
            .await
            .expect("create group");

        let detail = get_group_detail(&pool, &group_id, "TEST-GROUPS-DETAIL-OUTSIDER")
            .await
            .expect("query");
        assert!(detail.is_none());

        let owner_detail = get_group_detail(&pool, &group_id, "TEST-GROUPS-DETAIL-OWNER")
            .await
            .expect("query")
            .expect("owner should see the group");
        assert_eq!(owner_detail.owner_id, "TEST-GROUPS-DETAIL-OWNER");
        assert_eq!(owner_detail.member_count, 1);
        assert_eq!(owner_detail.role, GroupRole::Owner);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-DETAIL-OWNER", "TEST-GROUPS-DETAIL-OUTSIDER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_group_cascades_members_and_trains_and_invite_links -- --ignored`"]
    async fn delete_group_cascades_members_and_trains_and_invite_links() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-DELETE-OWNER").await;
        let group_id = create_group(&pool, "Delete Test", "TEST-GROUPS-DELETE-OWNER")
            .await
            .expect("create group");

        // Seed a train_subscriptions row for testing group_trains cascade
        let train_sub_id: (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, CURRENT_DATE, 'WOK', NOW()) \
             RETURNING id",
        )
        .bind("TEST-GROUPS-DELETE-OWNER")
        .fetch_one(&pool)
        .await
        .expect("seed train_subscription");

        // Add a group_trains row
        sqlx::query(
            "INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at) \
             VALUES ($1, $2, $3, NOW())",
        )
        .bind(&group_id)
        .bind(train_sub_id.0)
        .bind("TEST-GROUPS-DELETE-OWNER")
        .execute(&pool)
        .await
        .expect("add group_trains");

        // Create a group_invite_links row (token is base64-encoded random bytes, using UUID for test)
        let token = crate::auth::generate_session_token();
        sqlx::query(
            "INSERT INTO group_invite_links (token, group_id, created_by, created_at, expires_at) \
             VALUES ($1, $2, $3, NOW(), NOW() + INTERVAL '7 days')",
        )
        .bind(&token)
        .bind(&group_id)
        .bind("TEST-GROUPS-DELETE-OWNER")
        .execute(&pool)
        .await
        .expect("create group_invite_links");

        // Delete the group
        let deleted = delete_group(&pool, &group_id).await.expect("delete group");
        assert!(deleted);

        // Assert group_members cascaded
        let role = get_member_role(&pool, &group_id, "TEST-GROUPS-DELETE-OWNER")
            .await
            .expect("query");
        assert_eq!(role, None, "group_members row should have cascaded away");

        // Assert group_trains cascaded
        let train_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM group_trains WHERE group_id = $1",
        )
        .bind(&group_id)
        .fetch_one(&pool)
        .await
        .expect("count group_trains");
        assert_eq!(
            train_count.0, 0,
            "group_trains row should have cascaded away"
        );

        // Assert group_invite_links cascaded
        let link_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM group_invite_links WHERE group_id = $1",
        )
        .bind(&group_id)
        .fetch_one(&pool)
        .await
        .expect("count group_invite_links");
        assert_eq!(
            link_count.0, 0,
            "group_invite_links row should have cascaded away"
        );

        // Clean up train_subscriptions (cascade removed group_trains, but not train_subscriptions)
        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_sub_id.0)
            .execute(&pool)
            .await
            .ok();

        cleanup(&pool, &["TEST-GROUPS-DELETE-OWNER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                promote_to_admin_promotes_a_plain_member -- --ignored`"]
    async fn promote_to_admin_promotes_a_plain_member() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-PROMOTE-OWNER").await;
        seed_user(&pool, "TEST-GROUPS-PROMOTE-MEMBER").await;
        let group_id = create_group(&pool, "Promote Test", "TEST-GROUPS-PROMOTE-OWNER")
            .await
            .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GROUPS-PROMOTE-MEMBER")
        .execute(&pool)
        .await
        .expect("seed member");

        let promoted = promote_to_admin(&pool, &group_id, "TEST-GROUPS-PROMOTE-MEMBER")
            .await
            .expect("promote");
        assert!(promoted);
        let role = get_member_role(&pool, &group_id, "TEST-GROUPS-PROMOTE-MEMBER")
            .await
            .expect("query")
            .expect("still a member");
        assert_eq!(role, GroupRole::Admin);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-PROMOTE-OWNER", "TEST-GROUPS-PROMOTE-MEMBER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                promote_to_admin_is_a_noop_against_the_owner_row -- --ignored`"]
    async fn promote_to_admin_is_a_noop_against_the_owner_row() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-PROMOTE-OWNER-2").await;
        let group_id = create_group(&pool, "Promote Owner Test", "TEST-GROUPS-PROMOTE-OWNER-2")
            .await
            .expect("create group");

        // Attempting to "promote" the owner (e.g. a buggy caller replaying
        // an id) must never change their role -- structurally impossible
        // to reach via the real route (Task 7 gates this to owner-only and
        // never targets the caller's own row this way), but this pins the
        // data layer's own defense-in-depth independent of that.
        let promoted = promote_to_admin(&pool, &group_id, "TEST-GROUPS-PROMOTE-OWNER-2")
            .await
            .expect("promote attempt");
        assert!(!promoted);
        let role = get_member_role(&pool, &group_id, "TEST-GROUPS-PROMOTE-OWNER-2")
            .await
            .expect("query")
            .expect("still a member");
        assert_eq!(role, GroupRole::Owner);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-PROMOTE-OWNER-2"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_returns_not_a_member_for_an_unknown_user -- --ignored`"]
    async fn remove_member_returns_not_a_member_for_an_unknown_user() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVE-OWNER-1").await;
        let group_id = create_group(&pool, "Remove Test 1", "TEST-GROUPS-REMOVE-OWNER-1")
            .await
            .expect("create group");

        let outcome = remove_member(&pool, &group_id, "TEST-GROUPS-NEVER-A-MEMBER")
            .await
            .expect("remove attempt");
        assert_eq!(outcome, RemoveMemberOutcome::NotAMember);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-REMOVE-OWNER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_deletes_a_departed_members_shared_trains_in_the_same_transaction \
                -- --ignored`"]
    async fn remove_member_deletes_a_departed_members_shared_trains_in_the_same_transaction() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVE-OWNER-2").await;
        seed_user(&pool, "TEST-GROUPS-REMOVE-MEMBER-2").await;
        let group_id = create_group(&pool, "Remove Test 2", "TEST-GROUPS-REMOVE-OWNER-2")
            .await
            .expect("create group");
        sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')")
            .bind(&group_id)
            .bind("TEST-GROUPS-REMOVE-MEMBER-2")
            .execute(&pool)
            .await
            .expect("seed member");
        let train_id: (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, CURRENT_DATE, 'WOK', NOW()) RETURNING id",
        )
        .bind("TEST-GROUPS-REMOVE-MEMBER-2")
        .fetch_one(&pool)
        .await
        .expect("seed a tracked train for the member");
        sqlx::query(
            "INSERT INTO group_trains (group_id, train_subscription_id, added_by) VALUES ($1, $2, $3)",
        )
        .bind(&group_id)
        .bind(train_id.0)
        .bind("TEST-GROUPS-REMOVE-MEMBER-2")
        .execute(&pool)
        .await
        .expect("share the train into the group");

        let outcome = remove_member(&pool, &group_id, "TEST-GROUPS-REMOVE-MEMBER-2")
            .await
            .expect("remove member");
        assert_eq!(outcome, RemoveMemberOutcome::Removed { new_owner: None });

        let remaining_shared: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM group_trains WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count group_trains");
        assert_eq!(remaining_shared.0, 0, "the departed member's shared train should be pulled");

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id.0)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-REMOVE-OWNER-2", "TEST-GROUPS-REMOVE-MEMBER-2"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_transfers_ownership_to_the_longest_standing_admin_when_the_owner_leaves \
                -- --ignored`"]
    async fn remove_member_transfers_ownership_to_the_longest_standing_admin_when_the_owner_leaves() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-TRANSFER-OWNER").await;
        seed_user(&pool, "TEST-GROUPS-TRANSFER-ADMIN-OLD").await;
        seed_user(&pool, "TEST-GROUPS-TRANSFER-ADMIN-NEW").await;
        let group_id = create_group(&pool, "Transfer Test", "TEST-GROUPS-TRANSFER-OWNER")
            .await
            .expect("create group");
        // Two admins, inserted in a known joined_at order -- the OLDER one
        // must win, not insertion order into this test or role-assignment
        // order.
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role, joined_at) \
             VALUES ($1, $2, 'admin', NOW() - INTERVAL '2 days')",
        )
        .bind(&group_id)
        .bind("TEST-GROUPS-TRANSFER-ADMIN-OLD")
        .execute(&pool)
        .await
        .expect("seed older admin");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role, joined_at) \
             VALUES ($1, $2, 'admin', NOW() - INTERVAL '1 day')",
        )
        .bind(&group_id)
        .bind("TEST-GROUPS-TRANSFER-ADMIN-NEW")
        .execute(&pool)
        .await
        .expect("seed newer admin");

        let outcome = remove_member(&pool, &group_id, "TEST-GROUPS-TRANSFER-OWNER")
            .await
            .expect("owner leaves");
        assert_eq!(
            outcome,
            RemoveMemberOutcome::Removed {
                new_owner: Some("TEST-GROUPS-TRANSFER-ADMIN-OLD".to_string())
            }
        );
        let new_role = get_member_role(&pool, &group_id, "TEST-GROUPS-TRANSFER-ADMIN-OLD")
            .await
            .expect("query")
            .expect("still a member");
        assert_eq!(new_role, GroupRole::Owner);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(
            &pool,
            &[
                "TEST-GROUPS-TRANSFER-OWNER",
                "TEST-GROUPS-TRANSFER-ADMIN-OLD",
                "TEST-GROUPS-TRANSFER-ADMIN-NEW",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_transfers_ownership_to_the_longest_standing_member_when_no_admin_exists \
                -- --ignored`"]
    async fn remove_member_transfers_ownership_to_the_longest_standing_member_when_no_admin_exists() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-TRANSFER2-OWNER").await;
        seed_user(&pool, "TEST-GROUPS-TRANSFER2-MEMBER").await;
        let group_id = create_group(&pool, "Transfer Test 2", "TEST-GROUPS-TRANSFER2-OWNER")
            .await
            .expect("create group");
        sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')")
            .bind(&group_id)
            .bind("TEST-GROUPS-TRANSFER2-MEMBER")
            .execute(&pool)
            .await
            .expect("seed member");

        let outcome = remove_member(&pool, &group_id, "TEST-GROUPS-TRANSFER2-OWNER")
            .await
            .expect("owner leaves");
        assert_eq!(
            outcome,
            RemoveMemberOutcome::Removed {
                new_owner: Some("TEST-GROUPS-TRANSFER2-MEMBER".to_string())
            }
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-TRANSFER2-OWNER", "TEST-GROUPS-TRANSFER2-MEMBER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_deletes_the_whole_group_when_the_sole_owner_leaves_alone -- --ignored`"]
    async fn remove_member_deletes_the_whole_group_when_the_sole_owner_leaves_alone() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-SOLO-OWNER").await;
        let group_id = create_group(&pool, "Solo Test", "TEST-GROUPS-SOLO-OWNER")
            .await
            .expect("create group");

        let outcome = remove_member(&pool, &group_id, "TEST-GROUPS-SOLO-OWNER")
            .await
            .expect("owner leaves alone");
        assert_eq!(outcome, RemoveMemberOutcome::GroupDeleted);

        let detail = get_group_detail(&pool, &group_id, "TEST-GROUPS-SOLO-OWNER")
            .await
            .expect("query");
        assert!(detail.is_none(), "the group itself should be gone");

        cleanup(&pool, &["TEST-GROUPS-SOLO-OWNER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rotate_invite_link_revokes_the_previous_active_link -- --ignored`"]
    async fn rotate_invite_link_revokes_the_previous_active_link() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-INVITE-OWNER-1").await;
        let group_id = create_group(&pool, "Invite Test 1", "TEST-GROUPS-INVITE-OWNER-1")
            .await
            .expect("create group");

        let first = rotate_invite_link(&pool, &group_id, "TEST-GROUPS-INVITE-OWNER-1")
            .await
            .expect("first rotate");
        let second = rotate_invite_link(&pool, &group_id, "TEST-GROUPS-INVITE-OWNER-1")
            .await
            .expect("second rotate");
        assert_ne!(first.token, second.token);

        let active = get_active_invite_link(&pool, &group_id)
            .await
            .expect("query")
            .expect("should have an active link");
        assert_eq!(active.token, second.token, "only the newest link should be active");

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-INVITE-OWNER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                revoke_invite_link_is_idempotent_and_clears_the_active_link -- --ignored`"]
    async fn revoke_invite_link_is_idempotent_and_clears_the_active_link() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-INVITE-OWNER-2").await;
        let group_id = create_group(&pool, "Invite Test 2", "TEST-GROUPS-INVITE-OWNER-2")
            .await
            .expect("create group");
        rotate_invite_link(&pool, &group_id, "TEST-GROUPS-INVITE-OWNER-2")
            .await
            .expect("rotate");

        assert!(revoke_invite_link(&pool, &group_id).await.expect("first revoke"));
        assert!(!revoke_invite_link(&pool, &group_id).await.expect("second revoke is a no-op"));
        assert_eq!(get_active_invite_link(&pool, &group_id).await.expect("query"), None);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-INVITE-OWNER-2"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                resolve_invite_link_returns_none_for_an_expired_link -- --ignored`"]
    async fn resolve_invite_link_returns_none_for_an_expired_link() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-JOIN-OWNER-1").await;
        let group_id = create_group(&pool, "Join Test 1", "TEST-GROUPS-JOIN-OWNER-1")
            .await
            .expect("create group");
        let token = "test-expired-token";
        sqlx::query(
            "INSERT INTO group_invite_links (token, group_id, created_by, expires_at) \
             VALUES ($1, $2, $3, NOW() - INTERVAL '1 hour')",
        )
        .bind(token)
        .bind(&group_id)
        .bind("TEST-GROUPS-JOIN-OWNER-1")
        .execute(&pool)
        .await
        .expect("seed an expired link");

        assert_eq!(resolve_invite_link(&pool, token).await.expect("query"), None);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-JOIN-OWNER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                resolve_invite_link_returns_none_for_a_revoked_link -- --ignored`"]
    async fn resolve_invite_link_returns_none_for_a_revoked_link() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-JOIN-OWNER-2").await;
        let group_id = create_group(&pool, "Join Test 2", "TEST-GROUPS-JOIN-OWNER-2")
            .await
            .expect("create group");
        let link = rotate_invite_link(&pool, &group_id, "TEST-GROUPS-JOIN-OWNER-2")
            .await
            .expect("rotate");
        revoke_invite_link(&pool, &group_id).await.expect("revoke");

        assert_eq!(resolve_invite_link(&pool, &link.token).await.expect("query"), None);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-JOIN-OWNER-2"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                consume_invite_link_adds_the_caller_as_a_plain_member_and_is_idempotent \
                -- --ignored`"]
    async fn consume_invite_link_adds_the_caller_as_a_plain_member_and_is_idempotent() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-JOIN-OWNER-3").await;
        seed_user(&pool, "TEST-GROUPS-JOIN-JOINER-3").await;
        let group_id = create_group(&pool, "Join Test 3", "TEST-GROUPS-JOIN-OWNER-3")
            .await
            .expect("create group");
        let link = rotate_invite_link(&pool, &group_id, "TEST-GROUPS-JOIN-OWNER-3")
            .await
            .expect("rotate");

        let joined = consume_invite_link(&pool, &link.token, "TEST-GROUPS-JOIN-JOINER-3")
            .await
            .expect("consume")
            .expect("should resolve");
        assert_eq!(joined, group_id);
        let role = get_member_role(&pool, &group_id, "TEST-GROUPS-JOIN-JOINER-3")
            .await
            .expect("query")
            .expect("should be a member now");
        assert_eq!(role, GroupRole::Member);

        // Re-clicking the same link (double-submit, or the owner's own
        // link) must not error or duplicate the row.
        let joined_again = consume_invite_link(&pool, &link.token, "TEST-GROUPS-JOIN-JOINER-3")
            .await
            .expect("consume again")
            .expect("should still resolve");
        assert_eq!(joined_again, group_id);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-JOIN-OWNER-3", "TEST-GROUPS-JOIN-JOINER-3"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                consume_invite_link_returns_none_for_an_unknown_token -- --ignored`"]
    async fn consume_invite_link_returns_none_for_an_unknown_token() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-JOIN-JOINER-4").await;

        let joined = consume_invite_link(&pool, "not-a-real-token", "TEST-GROUPS-JOIN-JOINER-4")
            .await
            .expect("consume");
        assert_eq!(joined, None);

        cleanup(&pool, &["TEST-GROUPS-JOIN-JOINER-4"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_train_to_group_rejects_a_train_the_caller_does_not_own -- --ignored`"]
    async fn add_train_to_group_rejects_a_train_the_caller_does_not_own() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-ADDTRAIN-OWNER-1").await;
        seed_user(&pool, "TEST-GROUPS-ADDTRAIN-STRANGER-1").await;
        let group_id = create_group(&pool, "Add Train Test 1", "TEST-GROUPS-ADDTRAIN-OWNER-1")
            .await
            .expect("create group");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-ADDTRAIN-STRANGER-1").await;

        let added = add_train_to_group(
            &pool,
            &group_id,
            train_id,
            "TEST-GROUPS-ADDTRAIN-OWNER-1", // owns the GROUP, not the train
        )
        .await
        .expect("add attempt");
        assert!(!added);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-ADDTRAIN-OWNER-1", "TEST-GROUPS-ADDTRAIN-STRANGER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_train_to_group_is_idempotent -- --ignored`"]
    async fn add_train_to_group_is_idempotent() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-ADDTRAIN-OWNER-2").await;
        let group_id = create_group(&pool, "Add Train Test 2", "TEST-GROUPS-ADDTRAIN-OWNER-2")
            .await
            .expect("create group");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-ADDTRAIN-OWNER-2").await;

        assert!(
            add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-ADDTRAIN-OWNER-2")
                .await
                .expect("first add")
        );
        assert!(
            add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-ADDTRAIN-OWNER-2")
                .await
                .expect("second add is a no-op, not an error")
        );
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM group_trains WHERE group_id = $1")
            .bind(&group_id)
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count.0, 1);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-ADDTRAIN-OWNER-2"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_train_from_group_allows_the_sharer_to_remove_their_own_train -- --ignored`"]
    async fn remove_train_from_group_allows_the_sharer_to_remove_their_own_train() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVETRAIN-1").await;
        let group_id = create_group(&pool, "Remove Train Test 1", "TEST-GROUPS-REMOVETRAIN-1")
            .await
            .expect("create group");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-REMOVETRAIN-1").await;
        add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-REMOVETRAIN-1")
            .await
            .expect("add");

        let removed = remove_train_from_group(
            &pool,
            &group_id,
            train_id,
            "TEST-GROUPS-REMOVETRAIN-1",
            false, // plain member, but they ARE the sharer
        )
        .await
        .expect("remove");
        assert!(removed);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-REMOVETRAIN-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_train_from_group_denies_a_plain_member_removing_someone_elses_train \
                -- --ignored`"]
    async fn remove_train_from_group_denies_a_plain_member_removing_someone_elses_train() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVETRAIN-OWNER-2").await;
        seed_user(&pool, "TEST-GROUPS-REMOVETRAIN-SHARER-2").await;
        seed_user(&pool, "TEST-GROUPS-REMOVETRAIN-BYSTANDER-2").await;
        let group_id = create_group(&pool, "Remove Train Test 2", "TEST-GROUPS-REMOVETRAIN-OWNER-2")
            .await
            .expect("create group");
        for member in ["TEST-GROUPS-REMOVETRAIN-SHARER-2", "TEST-GROUPS-REMOVETRAIN-BYSTANDER-2"] {
            sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')")
                .bind(&group_id)
                .bind(member)
                .execute(&pool)
                .await
                .expect("seed member");
        }
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-REMOVETRAIN-SHARER-2").await;
        add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-REMOVETRAIN-SHARER-2")
            .await
            .expect("add");

        let removed = remove_train_from_group(
            &pool,
            &group_id,
            train_id,
            "TEST-GROUPS-REMOVETRAIN-BYSTANDER-2",
            false, // plain member, NOT the sharer
        )
        .await
        .expect("remove attempt");
        assert!(!removed);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(
            &pool,
            &[
                "TEST-GROUPS-REMOVETRAIN-OWNER-2",
                "TEST-GROUPS-REMOVETRAIN-SHARER-2",
                "TEST-GROUPS-REMOVETRAIN-BYSTANDER-2",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_train_from_group_allows_an_admin_to_remove_anyones_train -- --ignored`"]
    async fn remove_train_from_group_allows_an_admin_to_remove_anyones_train() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-REMOVETRAIN-OWNER-3").await;
        seed_user(&pool, "TEST-GROUPS-REMOVETRAIN-SHARER-3").await;
        let group_id = create_group(&pool, "Remove Train Test 3", "TEST-GROUPS-REMOVETRAIN-OWNER-3")
            .await
            .expect("create group");
        sqlx::query("INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')")
            .bind(&group_id)
            .bind("TEST-GROUPS-REMOVETRAIN-SHARER-3")
            .execute(&pool)
            .await
            .expect("seed member");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-REMOVETRAIN-SHARER-3").await;
        add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-REMOVETRAIN-SHARER-3")
            .await
            .expect("add");

        let removed = remove_train_from_group(
            &pool,
            &group_id,
            train_id,
            "TEST-GROUPS-REMOVETRAIN-OWNER-3",
            true, // owner, can_manage
        )
        .await
        .expect("remove");
        assert!(removed);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(
            &pool,
            &["TEST-GROUPS-REMOVETRAIN-OWNER-3", "TEST-GROUPS-REMOVETRAIN-SHARER-3"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_group_trains_returns_shared_trains_with_attribution -- --ignored`"]
    async fn list_group_trains_returns_shared_trains_with_attribution() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-LISTTRAINS-OWNER").await;
        let group_id = create_group(&pool, "List Trains Test", "TEST-GROUPS-LISTTRAINS-OWNER")
            .await
            .expect("create group");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-LISTTRAINS-OWNER").await;
        add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-LISTTRAINS-OWNER")
            .await
            .expect("add");

        let trains = list_group_trains(&pool, &group_id).await.expect("list");
        assert_eq!(trains.len(), 1);
        assert_eq!(trains[0].train_subscription_id, train_id);
        assert_eq!(trains[0].added_by, "TEST-GROUPS-LISTTRAINS-OWNER");
        assert!(trains[0].added_by_name.is_some());

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup(&pool, &["TEST-GROUPS-LISTTRAINS-OWNER"]).await;
    }
}

#[cfg(test)]
mod group_train_wire_shape_tests {
    use super::*;

    /// Pins the exact JSON keys `GroupTrain` serializes to. A future edit
    /// that tacks on a ticket-related or `notificationsEnabled`/exact-
    /// `trackedAt` field -- the spec §4 hard constraint -- fails this test
    /// immediately, rather than only being caught by manual review.
    #[test]
    fn group_train_json_never_includes_ticket_or_notification_fields() {
        let train = GroupTrain {
            train_subscription_id: 1,
            pin_origin_crs: Some("WOK".to_string()),
            pin_destination_crs: None,
            pin_origin_name: Some("Woking".to_string()),
            pin_destination_name: None,
            pin_scheduled_departure: None,
            service_date: "2026-09-11".parse().unwrap(),
            resolution_status: "pending".to_string(),
            train_uid: None,
            status: None,
            delay_minutes: None,
            custom_name: None,
            added_by: "user-1".to_string(),
            added_by_name: Some("Alex".to_string()),
        };
        let value = serde_json::to_value(&train).expect("serialize");
        let mut keys: Vec<&str> = value.as_object().expect("object").keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "addedBy",
                "addedByName",
                "customName",
                "delayMinutes",
                "pinDestinationCrs",
                "pinDestinationName",
                "pinOriginCrs",
                "pinOriginName",
                "pinScheduledDeparture",
                "resolutionStatus",
                "serviceDate",
                "status",
                "trainSubscriptionId",
                "trainUid",
            ]
        );
    }
}

#[cfg(test)]
mod group_member_wire_shape_tests {
    use super::*;

    /// Pins the exact JSON keys `GroupMember` serializes to. A future edit
    /// that re-adds a raw `email` field alongside `displayName` would leak
    /// a member's verified email to every other member of the group --
    /// this test fails immediately rather than only being caught by
    /// manual review, mirroring `group_train_json_never_includes_ticket_or_notification_fields`
    /// one struct away.
    #[test]
    fn group_member_json_never_includes_a_raw_email_field() {
        let member = GroupMember {
            user_id: "user-1".to_string(),
            display_name: Some("Alex".to_string()),
            role: GroupRole::Member,
            joined_at: "2026-09-11T00:00:00Z".parse().unwrap(),
        };
        let value = serde_json::to_value(&member).expect("serialize");
        let mut keys: Vec<&str> = value.as_object().expect("object").keys().map(String::as_str).collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["displayName", "joinedAt", "role", "userId"]);
    }
}
