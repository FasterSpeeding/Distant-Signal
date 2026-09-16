//! CRUD + permissions for shared groups (`groups`, `group_members`,
//! `group_trains`, `group_invite_links`). See
//! docs/superpowers/specs/2026-09-11-shared-groups-design.md.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::users;

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
    owner_username: Option<String>,
    member_count: i64,
    role: String,
}

/// `None` unless `user_id` is a member of `group_id` -- this app's
/// universal "exists but not yours" 404 convention (never `403` for
/// "doesn't exist or isn't yours" -- see `train_tracking::tracked_train_owner`).
///
/// The inner join below on `role = 'owner'` assumes every group always has
/// exactly one owner row. HAZARD for whoever adds user deletion: the
/// migration's `group_members.user_id ON DELETE CASCADE` removes the
/// owner's own membership row directly if their `users` row is ever
/// deleted, bypassing `remove_member`'s ownership-transfer/group-deletion
/// logic entirely -- this join then finds no owner row and 404s for every
/// remaining member forever (an unreadable-but-not-deleted group). No
/// user-deletion feature exists today, so this is latent, not an active
/// bug; a future one should either run `remove_member`-equivalent logic
/// before deleting the user, or otherwise repair/reassign ownership as
/// part of that deletion.
pub async fn get_group_detail(
    pool: &PgPool,
    group_id: &str,
    user_id: &str,
) -> Result<Option<GroupDetail>> {
    let row: Option<GroupDetailRow> = sqlx::query_as(
        "SELECT g.id, g.name, \
                owner_m.user_id AS owner_id, owner_u.name AS owner_name, \
                owner_u.username AS owner_username, \
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
        // Same helper as the member list and shared-train attribution:
        // this field also goes to every member of the group including
        // plain ones, so it gets the identical name-else-username,
        // never-an-email, blank-is-not-a-label treatment.
        owner_name: users::display_label(r.owner_name, r.owner_username),
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

/// Deliberately no `email` field, and the query below deliberately does
/// not select one: nothing about a member list needs an email address, and
/// a column that is never read can never be leaked by a later edit.
#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupMemberRow {
    user_id: String,
    name: Option<String>,
    username: Option<String>,
    role: String,
    joined_at: DateTime<Utc>,
}

/// Deliberately no `email` field, and `display_name` is deliberately never
/// an email either: `GET /groups/{id}/members` returns this to every
/// current member, and a joined-via-link member has no other relationship
/// with the rest of the group -- shipping their verified email to everyone
/// (as a field of its own, OR quietly as the `displayName` fallback for a
/// member whose IdP sent no name) leaks more than the feature needs. The
/// fallback is their `username` instead. `None` means "neither on file",
/// which the frontend renders as its own generic "A member" placeholder.
/// Same rule, same helper (`users::display_label`), as
/// `GroupTrain.added_by_name` one struct away.
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
            // `users::display_label`, not a bare `row.name`: a
            // blank-but-present name ("" -- what an IdP with no name on
            // file for the user actually sends) is not a label, and used
            // to render as an empty member row. See that function's own
            // doc comment, and `display_name_collapse_tests` below.
            display_name: users::display_label(row.name, row.username),
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
        "SELECT gm.user_id, u.name, u.username, gm.role, gm.joined_at \
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
    //
    // `custom_line_group_grants` is DELIBERATELY not cleaned up here, a
    // considered divergence from this cleanup rather than an omission --
    // see docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md
    // §2.7. The cleanup below exists because a shared train's underlying
    // `train_subscriptions` row is itself private to the departing member
    // and may vanish the moment they untrack it. Neither premise holds for
    // a granted custom line: leaving a group changes nothing about
    // `custom_lines.user_id` (ownership is fixed and has no transfer path
    // at all), and the remaining members' access was granted deliberately
    // by the owner and does not depend on the owner's continued presence.
    // Auto-revoking here would also silently drop the grant on a
    // leave-and-rejoin, with nothing restoring it. The owner keeps three
    // independent ways to revoke at any time: `remove_custom_line_grant`
    // as the granter (which, per that function and its route, works even
    // after they leave), any current `admin`/`owner` removing it, or
    // deleting the line outright (the FK cascades every grant everywhere).
    // `remove_member_does_not_touch_custom_line_group_grants_even_when_the_departing_member_is_the_grantor`
    // pins this.
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
            sqlx::query(
                "UPDATE group_members SET role = 'owner' WHERE group_id = $1 AND user_id = $2",
            )
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
pub async fn rotate_invite_link(
    pool: &PgPool,
    group_id: &str,
    user_id: &str,
) -> Result<InviteLink> {
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
    added_by_username: Option<String>,
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
            // The sharer's `users.name`, else their `users.username` --
            // never the raw internal user_id, and never their email (see
            // `users::display_label`: attribution shown to a whole group
            // is not the place to reveal one member's email address).
            // Shared with the member list via that same helper so the two
            // can't drift on what counts as a usable name -- a blank one
            // doesn't; see `display_name_collapse_tests` below.
            added_by_name: users::display_label(row.added_by_name, row.added_by_username),
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
                gt.added_by, u.name AS added_by_name, u.username AS added_by_username \
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

#[derive(Debug, Clone, sqlx::FromRow)]
struct SharedTrainRow {
    group_id: String,
    group_name: String,
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
    added_by_username: Option<String>,
}

/// One train shared into one group the CALLER is a member of -- `GroupTrain`
/// (the per-group `GET /groups/{id}/trains` shape) plus the two fields that
/// only make sense once rows from several groups land in one list:
/// `group_id`/`group_name`, the "from <group>" attribution
/// `/track/mine` tags each row with.
///
/// Inherits `GroupTrain`'s hard privacy constraint verbatim (spec §4's
/// "Never shown" list): no ticket field, no `notificationsEnabled`, no
/// exact `trackedAt`. This shape is strictly MORE exposed than
/// `GroupTrain` -- it reaches a member without them opening the group at
/// all -- so no future edit may widen it past what the group detail page
/// already shows the same member.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedTrain {
    pub group_id: String,
    pub group_name: String,
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

impl From<SharedTrainRow> for SharedTrain {
    fn from(row: SharedTrainRow) -> Self {
        SharedTrain {
            group_id: row.group_id,
            group_name: row.group_name,
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
            // Same "name, else username, else nothing -- and never an
            // email" collapse `GroupTrain::from` already applies, via the
            // same `users::display_label`; see its own comment. This shape
            // is strictly more exposed than `GroupTrain` (it reaches a
            // member without them opening the group at all), so it is the
            // last place that should be laxer about it.
            added_by_name: users::display_label(row.added_by_name, row.added_by_username),
        }
    }
}

/// Every train shared into ANY group `user_id` belongs to, EXCLUDING the
/// ones they tracked themselves. This is what lets `/track/mine` show a
/// group-shared train alongside the caller's own tracked trains, tagged
/// with where it came from, instead of hiding it behind
/// `/groups/{id}` -- the whole point of sharing a train being that the
/// people you shared it with actually see it.
///
/// `ts.user_id <> $1` is the deliberate half of that: the caller's OWN
/// subscriptions already occupy a row each in
/// `train_tracking::list_tracked_trains_for_user`'s half of the same list
/// (with their own rename/ticket controls, which a shared row
/// deliberately has none of), and a train they shared into two groups
/// would otherwise render three times on one page. Note the asymmetry
/// this leaves: a train shared WITH the caller is tagged, while one the
/// caller shared OUT carries no group tag on `/track/mine` at all --
/// `TrackedTrainListItem::shared_group_count` is already on that row's
/// wire shape, but nothing on this page renders it today. Tagging the
/// outgoing direction too is a deliberate follow-up, not something this
/// query is hiding.
///
/// One row per (group, train) pair, NOT per train: a train shared into two
/// groups the caller is in is genuinely two attributions, and collapsing
/// that server-side would throw away one of the two group names the page
/// tags the row with. The frontend merges them back into one row carrying
/// both tags.
///
/// No permission check beyond the `group_members me` join, which IS the
/// check: a row can only appear here via a group the caller is currently a
/// member of, so this needs no `get_member_role` gate of its own (and,
/// unlike `list_group_trains`, has no single group id to gate on).
///
/// Ordered newest-shared-first and capped at the same
/// [`MINE_LIST_LIMIT`](crate::data::train_tracking::MINE_LIST_LIMIT) the
/// caller's own half of the list uses -- but note the cap counts (group,
/// train) PAIRS here, where the own half counts trains, so a train shared
/// into three of the caller's groups spends three of the hundred. Those
/// three pairs are NOT ordered together either -- shares into different
/// groups happen at different times -- so at the far edge of the cap a
/// merged row can lose one of its `from <group>` tags, and if every one
/// of a train's pairs falls past the cut, the row itself (ordinary
/// truncation, exactly as the own half drops its 101st train). Accepted
/// rather than solved
/// with a windowed subquery: the same "100 is a round number, not a
/// researched one, revisit when real usage exists" posture
/// `MINE_LIST_LIMIT` itself is documented with, and nothing today is
/// remotely near it. `gt.added_at` is an ordering key
/// only and is never selected into the response (spec §4 forbids exposing
/// a share's exact timestamp), with `gt.train_subscription_id` breaking
/// ties so the order is total and stable across calls.
pub async fn list_shared_trains_for_user(pool: &PgPool, user_id: &str) -> Result<Vec<SharedTrain>> {
    let rows: Vec<SharedTrainRow> = sqlx::query_as(
        "SELECT g.id AS group_id, g.name AS group_name, \
                gt.train_subscription_id, \
                ts.pin_origin_crs, ts.pin_destination_crs, \
                so.name AS pin_origin_name, sd.name AS pin_destination_name, \
                ts.pin_scheduled_departure, ts.service_date, ts.resolution_status, \
                tr.train_uid, cs.status, cs.delay_minutes, ts.custom_name, \
                gt.added_by, u.name AS added_by_name, u.username AS added_by_username \
         FROM group_members me \
         JOIN groups g ON g.id = me.group_id \
         JOIN group_trains gt ON gt.group_id = g.id \
         JOIN train_subscriptions ts ON ts.id = gt.train_subscription_id \
         JOIN users u ON u.id = gt.added_by \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
         LEFT JOIN stations so ON so.crs = UPPER(ts.pin_origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(ts.pin_destination_crs) \
         WHERE me.user_id = $1 AND ts.user_id <> $1 \
         ORDER BY gt.added_at DESC, gt.train_subscription_id DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(crate::data::train_tracking::MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(SharedTrain::from).collect())
}

// ---------------------------------------------------------------------------
// Custom-line group grants (`custom_line_group_grants`). See
// docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md and
// docs/superpowers/plans/2026-09-15-custom-line-group-sharing.md.
//
// A grant conveys READ access only, and nothing in this section is ever
// consulted by `custom_lines::update_custom_line`/`delete_custom_line` --
// those remain gated purely on `user_id = caller.id`, exactly as before
// this feature. The read side lives in
// `custom_lines::readable_custom_line_ids`, not here.
// ---------------------------------------------------------------------------

/// Grants `group_id` read access to one of the CALLER'S OWN custom lines.
///
/// Ownership is enforced at the APPLICATION layer, the exact
/// `WHERE id = $1 AND user_id = $2` shape [`add_train_to_group`] already
/// uses for trains and `train_tracking.rs` uses for tickets. This is the
/// deliberate restriction of design §2.3, worth stating plainly: a group's
/// `admin`/`owner` has NO special standing over a custom line they don't
/// own and cannot force a member's private line into the group's shared
/// view. Only the line's own owner can choose to share it.
///
/// Returns `false` if `line_id` doesn't exist or isn't owned by `user_id`
/// -- the route maps both, indistinguishably, to `404` with `get_line`'s
/// own "custom line not found" message, never `403` and never `400`.
///
/// Idempotent: re-granting an already-granted line is a silent no-op
/// (`ON CONFLICT (group_id, line_id) DO NOTHING`), matching
/// [`add_train_to_group`].
///
/// The caller must ALSO be a current member of the group -- that half is
/// the route's `require_member` gate, not this function's job.
///
/// The ownership check and the insert share one transaction, unlike
/// [`add_train_to_group`]'s two separate statements. Not for safety --
/// `custom_lines` has no ownership-transfer path at all, so the check can
/// never become MORE permissive between the two -- but because
/// `custom_line_group_grants.line_id` carries a real FK: an owner deleting
/// the line from a second tab in that window would otherwise turn a clean
/// `404` into an FK-violation `500`.
pub async fn grant_custom_line(
    pool: &PgPool,
    group_id: &str,
    line_id: &str,
    user_id: &str,
) -> Result<bool> {
    let mut tx = pool.begin().await?;
    let owned: Option<(String,)> =
        sqlx::query_as("SELECT id FROM custom_lines WHERE id = $1 AND user_id = $2 FOR UPDATE")
            .bind(line_id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    if owned.is_none() {
        tx.rollback().await?;
        return Ok(false);
    }

    sqlx::query(
        "INSERT INTO custom_line_group_grants (group_id, line_id, granted_by, granted_at) \
         VALUES ($1, $2, $3, NOW()) \
         ON CONFLICT (group_id, line_id) DO NOTHING",
    )
    .bind(group_id)
    .bind(line_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Revokes a grant. `caller_can_manage` should be the route's own
/// already-resolved `GroupRole::can_manage()` -- an `admin`/`owner` may
/// remove ANY grant; anyone else may only remove one THEY granted (design
/// §2.4, identical in shape to [`remove_train_from_group`]).
///
/// Removing a grant never touches `custom_lines` itself: the line, its
/// content and its ownership are completely unaffected, and only the
/// group's VISIBILITY into it changes. Because access is resolved live on
/// every read ([`crate::data::custom_lines::readable_custom_line_ids`]),
/// the revocation takes effect on the very next request with nothing else
/// to invalidate (design §2.5).
///
/// Note the sharer branch checks `granted_by = $3` against the caller's own
/// id and NOT against current group membership, which is what lets a
/// granter who has since left the group still revoke their own grant
/// (design §2.7) -- the route for this deliberately does not
/// `require_member` for exactly that reason.
///
/// Returns `false` if no matching row was deleted (unknown grant, or a
/// non-manager targeting someone else's) -- the route maps that to `404`.
pub async fn remove_custom_line_grant(
    pool: &PgPool,
    group_id: &str,
    line_id: &str,
    user_id: &str,
    caller_can_manage: bool,
) -> Result<bool> {
    let result = if caller_can_manage {
        sqlx::query("DELETE FROM custom_line_group_grants WHERE group_id = $1 AND line_id = $2")
            .bind(group_id)
            .bind(line_id)
            .execute(pool)
            .await?
    } else {
        sqlx::query(
            "DELETE FROM custom_line_group_grants \
             WHERE group_id = $1 AND line_id = $2 AND granted_by = $3",
        )
        .bind(group_id)
        .bind(line_id)
        .bind(user_id)
        .execute(pool)
        .await?
    };
    Ok(result.rows_affected() > 0)
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct GroupCustomLineRow {
    line_id: String,
    line_name: String,
    granted_by: String,
    granted_by_name: Option<String>,
    granted_by_username: Option<String>,
}

/// A custom line granted into a group, as `GET /groups/{id}/lines/custom`
/// returns it.
///
/// Deliberately carries only the line's identity and its attribution --
/// no stations, operators, headcode filters or status. Not because those
/// are secret from a granted member (they explicitly are not, design
/// §3.5: full detail or nothing), but because the group page reads them
/// through the ordinary `/lines/{id}` and `GET /Line/{ids}/Status` routes
/// this feature already widened, rather than duplicating a second,
/// parallel status-rendering path into the groups module that would then
/// have to be kept in sync with the first.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupCustomLine {
    pub line_id: String,
    pub line_name: String,
    pub granted_by: String,
    pub granted_by_name: Option<String>,
}

impl From<GroupCustomLineRow> for GroupCustomLine {
    fn from(row: GroupCustomLineRow) -> Self {
        GroupCustomLine {
            line_id: row.line_id,
            line_name: row.line_name,
            granted_by: row.granted_by,
            // `users::display_label` -- the sharer's `users.name`, else
            // their `users.username`, else nothing. Never the raw internal
            // user id, and never their email: attribution shown to a whole
            // group is not the place to reveal one member's email address.
            // Shared with `GroupTrain::from` and the member list via that
            // same helper so none of them can drift on what counts as a
            // usable name (a blank one doesn't).
            granted_by_name: users::display_label(row.granted_by_name, row.granted_by_username),
        }
    }
}

/// Every custom line granted into `group_id`, oldest-granted first. No
/// permission check here -- the route's own `require_member` call gates "is
/// the caller even a member", exactly as it does for
/// [`list_group_trains`].
pub async fn list_group_custom_lines(
    pool: &PgPool,
    group_id: &str,
) -> Result<Vec<GroupCustomLine>> {
    let rows: Vec<GroupCustomLineRow> = sqlx::query_as(
        "SELECT g.line_id, cl.name AS line_name, \
                g.granted_by, u.name AS granted_by_name, u.username AS granted_by_username \
         FROM custom_line_group_grants g \
         JOIN custom_lines cl ON cl.id = g.line_id \
         JOIN users u ON u.id = g.granted_by \
         WHERE g.group_id = $1 \
         ORDER BY g.granted_at, g.line_id",
    )
    .bind(group_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(GroupCustomLine::from).collect())
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SharedCustomLineRow {
    group_id: String,
    group_name: String,
    line_id: String,
    line_name: String,
    granted_by: String,
    granted_by_name: Option<String>,
    granted_by_username: Option<String>,
}

/// One custom line granted into one group the CALLER is a member of --
/// [`GroupCustomLine`] plus the two fields that only make sense once rows
/// from several groups land in one list (`group_id`/`group_name`, the
/// "from <group>" tag the home page renders).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedCustomLine {
    pub group_id: String,
    pub group_name: String,
    pub line_id: String,
    pub line_name: String,
    pub granted_by: String,
    pub granted_by_name: Option<String>,
}

impl From<SharedCustomLineRow> for SharedCustomLine {
    fn from(row: SharedCustomLineRow) -> Self {
        SharedCustomLine {
            group_id: row.group_id,
            group_name: row.group_name,
            line_id: row.line_id,
            line_name: row.line_name,
            granted_by: row.granted_by,
            // Same `users::display_label` collapse `GroupCustomLine::from`
            // applies -- see its own comment. This shape is strictly MORE
            // exposed (it reaches a member on the home page without them
            // opening the group at all), so it must never be laxer.
            granted_by_name: users::display_label(row.granted_by_name, row.granted_by_username),
        }
    }
}

/// Every custom line granted into ANY group `user_id` belongs to,
/// EXCLUDING the ones they own themselves. This is what lets the home page
/// show a group-shared custom line alongside the caller's own pinned
/// lines, tagged with where it came from -- the exact shape (and the exact
/// reasoning) [`list_shared_trains_for_user`] already established for
/// trains.
///
/// `cl.user_id <> $1` is the deliberate half of that, parallel to that
/// function's `ts.user_id <> $1`: the caller's OWN custom lines already
/// reach the home page through `pinned_lines`/`GET /public/lines`, with
/// their own edit controls (which a shared row deliberately has none of),
/// and a line they granted into two groups would otherwise render three
/// times on one page.
///
/// One row per (group, line) pair, NOT per line: a line granted into two
/// groups the caller is in is genuinely two attributions, and collapsing
/// that server-side would throw away one of the two group names the page
/// tags the row with. The frontend merges them back into one row carrying
/// both tags (`frontend/lib/sharedCustomLines.ts`).
///
/// No permission check beyond the `group_members me` join, which IS the
/// check: a row can only appear here via a group the caller is currently a
/// member of, so a non-member simply gets nothing rather than a `404`, and
/// there is no single group id to gate on anyway.
///
/// `granted_at` is an ordering key only and is never selected into the
/// response, with `line_id` breaking ties so the order is total and stable
/// across calls.
pub async fn list_shared_custom_lines_for_user(
    pool: &PgPool,
    user_id: &str,
) -> Result<Vec<SharedCustomLine>> {
    let rows: Vec<SharedCustomLineRow> = sqlx::query_as(
        "SELECT g.id AS group_id, g.name AS group_name, \
                gr.line_id, cl.name AS line_name, \
                gr.granted_by, u.name AS granted_by_name, u.username AS granted_by_username \
         FROM group_members me \
         JOIN groups g ON g.id = me.group_id \
         JOIN custom_line_group_grants gr ON gr.group_id = g.id \
         JOIN custom_lines cl ON cl.id = gr.line_id \
         JOIN users u ON u.id = gr.granted_by \
         WHERE me.user_id = $1 AND cl.user_id <> $1 \
         ORDER BY gr.granted_at DESC, gr.line_id DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(SharedCustomLine::from).collect())
}

/// A group a custom line is shared into, as the line's OWNER sees it on
/// their own edit page ("Shared with: Family, Commute Buddies").
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineGroupRef {
    pub id: String,
    pub name: String,
}

/// Every group `line_id` is currently granted into, alphabetically --
/// **empty unless `caller_id` owns the line.**
///
/// This is the one query in this module whose output is a description of
/// somebody's group memberships, so the owner-only rule is enforced HERE,
/// in the `EXISTS` clause, and not only by `routes::lines::get_line`
/// calling it behind an `if is_owner`. Both guards say the same thing; a
/// future refactor that loses the caller-side one still cannot make this
/// return anything to a non-owner. That is what satisfies design §3.5's
/// privacy goal: a fellow group member must never learn which OTHER
/// groups the owner has also shared this line into.
///
/// For the owner it deliberately lists EVERY group the line is granted
/// into, including one they have since LEFT (which design §2.7 explicitly
/// allows to keep its grant). Scoping this to the owner's current
/// memberships -- as the design sketched -- would hide a live grant from
/// the one person whose data it is and who is entitled to revoke it
/// (`remove_custom_line_grant`'s granter branch works after they leave,
/// precisely so they can). That would be the real privacy failure, not a
/// protection.
pub async fn groups_shared_with_line(
    pool: &PgPool,
    line_id: &str,
    caller_id: &str,
) -> Result<Vec<LineGroupRef>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT g.id, g.name FROM custom_line_group_grants gr \
         JOIN groups g ON g.id = gr.group_id \
         WHERE gr.line_id = $1 \
           AND EXISTS ( \
             SELECT 1 FROM custom_lines cl \
             WHERE cl.id = gr.line_id AND cl.user_id = $2 \
           ) \
         ORDER BY g.name, g.id",
    )
    .bind(line_id)
    .bind(caller_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, name)| LineGroupRef { id, name })
        .collect())
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
        cleanup(
            &pool,
            &["TEST-GROUPS-DETAIL-OWNER", "TEST-GROUPS-DETAIL-OUTSIDER"],
        )
        .await;
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
        let train_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM group_trains WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count group_trains");
        assert_eq!(
            train_count.0, 0,
            "group_trains row should have cascaded away"
        );

        // Assert group_invite_links cascaded
        let link_count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM group_invite_links WHERE group_id = $1")
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
        cleanup(
            &pool,
            &["TEST-GROUPS-PROMOTE-OWNER", "TEST-GROUPS-PROMOTE-MEMBER"],
        )
        .await;
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
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
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
        assert_eq!(
            remaining_shared.0, 0,
            "the departed member's shared train should be pulled"
        );

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
        cleanup(
            &pool,
            &["TEST-GROUPS-REMOVE-OWNER-2", "TEST-GROUPS-REMOVE-MEMBER-2"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_transfers_ownership_to_the_longest_standing_admin_when_the_owner_leaves \
                -- --ignored`"]
    async fn remove_member_transfers_ownership_to_the_longest_standing_admin_when_the_owner_leaves()
    {
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
    async fn remove_member_transfers_ownership_to_the_longest_standing_member_when_no_admin_exists()
    {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-TRANSFER2-OWNER").await;
        seed_user(&pool, "TEST-GROUPS-TRANSFER2-MEMBER").await;
        let group_id = create_group(&pool, "Transfer Test 2", "TEST-GROUPS-TRANSFER2-OWNER")
            .await
            .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
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
        cleanup(
            &pool,
            &[
                "TEST-GROUPS-TRANSFER2-OWNER",
                "TEST-GROUPS-TRANSFER2-MEMBER",
            ],
        )
        .await;
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
        assert_eq!(
            active.token, second.token,
            "only the newest link should be active"
        );

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

        assert!(
            revoke_invite_link(&pool, &group_id)
                .await
                .expect("first revoke")
        );
        assert!(
            !revoke_invite_link(&pool, &group_id)
                .await
                .expect("second revoke is a no-op")
        );
        assert_eq!(
            get_active_invite_link(&pool, &group_id)
                .await
                .expect("query"),
            None
        );

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

        assert_eq!(
            resolve_invite_link(&pool, token).await.expect("query"),
            None
        );

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

        assert_eq!(
            resolve_invite_link(&pool, &link.token)
                .await
                .expect("query"),
            None
        );

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
        cleanup(
            &pool,
            &["TEST-GROUPS-JOIN-OWNER-3", "TEST-GROUPS-JOIN-JOINER-3"],
        )
        .await;
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
        cleanup(
            &pool,
            &[
                "TEST-GROUPS-ADDTRAIN-OWNER-1",
                "TEST-GROUPS-ADDTRAIN-STRANGER-1",
            ],
        )
        .await;
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
        let group_id = create_group(
            &pool,
            "Remove Train Test 2",
            "TEST-GROUPS-REMOVETRAIN-OWNER-2",
        )
        .await
        .expect("create group");
        for member in [
            "TEST-GROUPS-REMOVETRAIN-SHARER-2",
            "TEST-GROUPS-REMOVETRAIN-BYSTANDER-2",
        ] {
            sqlx::query(
                "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
            )
            .bind(&group_id)
            .bind(member)
            .execute(&pool)
            .await
            .expect("seed member");
        }
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-REMOVETRAIN-SHARER-2").await;
        add_train_to_group(
            &pool,
            &group_id,
            train_id,
            "TEST-GROUPS-REMOVETRAIN-SHARER-2",
        )
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
        let group_id = create_group(
            &pool,
            "Remove Train Test 3",
            "TEST-GROUPS-REMOVETRAIN-OWNER-3",
        )
        .await
        .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GROUPS-REMOVETRAIN-SHARER-3")
        .execute(&pool)
        .await
        .expect("seed member");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-REMOVETRAIN-SHARER-3").await;
        add_train_to_group(
            &pool,
            &group_id,
            train_id,
            "TEST-GROUPS-REMOVETRAIN-SHARER-3",
        )
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
            &[
                "TEST-GROUPS-REMOVETRAIN-OWNER-3",
                "TEST-GROUPS-REMOVETRAIN-SHARER-3",
            ],
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

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_shared_trains_for_user_returns_other_members_trains_tagged_with_their_group \
                -- --ignored`"]
    async fn list_shared_trains_for_user_returns_other_members_trains_tagged_with_their_group() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-SHAREDMINE-SHARER").await;
        seed_user(&pool, "TEST-GROUPS-SHAREDMINE-VIEWER").await;
        let group_id = create_group(&pool, "Shared Mine Test", "TEST-GROUPS-SHAREDMINE-SHARER")
            .await
            .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GROUPS-SHAREDMINE-VIEWER")
        .execute(&pool)
        .await
        .expect("seed member");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-SHAREDMINE-SHARER").await;
        add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-SHAREDMINE-SHARER")
            .await
            .expect("add");

        let shared = list_shared_trains_for_user(&pool, "TEST-GROUPS-SHAREDMINE-VIEWER")
            .await
            .expect("list shared trains");
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].train_subscription_id, train_id);
        assert_eq!(shared[0].group_id, group_id);
        assert_eq!(shared[0].group_name, "Shared Mine Test");
        assert_eq!(shared[0].added_by, "TEST-GROUPS-SHAREDMINE-SHARER");
        assert_eq!(
            shared[0].added_by_name.as_deref(),
            Some("TEST-GROUPS-SHAREDMINE-SHARER"),
            "the sharer's display name is what the tag on the row renders"
        );
        assert_eq!(shared[0].pin_origin_crs.as_deref(), Some("WOK"));

        // The sharer's OWN list never repeats their own train back at them
        // -- it's already in their `list_tracked_trains_for_user` half of
        // the same page.
        let sharers_own = list_shared_trains_for_user(&pool, "TEST-GROUPS-SHAREDMINE-SHARER")
            .await
            .expect("list the sharer's shared trains");
        assert!(
            sharers_own.is_empty(),
            "a caller's own tracked train must never come back as a shared one, got {sharers_own:?}"
        );

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
                "TEST-GROUPS-SHAREDMINE-SHARER",
                "TEST-GROUPS-SHAREDMINE-VIEWER",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_shared_trains_for_user_never_leaks_a_group_the_caller_is_not_in \
                -- --ignored`"]
    async fn list_shared_trains_for_user_never_leaks_a_group_the_caller_is_not_in() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-SHAREDLEAK-SHARER").await;
        seed_user(&pool, "TEST-GROUPS-SHAREDLEAK-STRANGER").await;
        let group_id = create_group(&pool, "Shared Leak Test", "TEST-GROUPS-SHAREDLEAK-SHARER")
            .await
            .expect("create group");
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-SHAREDLEAK-SHARER").await;
        add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-SHAREDLEAK-SHARER")
            .await
            .expect("add");

        let stranger = list_shared_trains_for_user(&pool, "TEST-GROUPS-SHAREDLEAK-STRANGER")
            .await
            .expect("list shared trains");
        assert!(
            stranger.is_empty(),
            "a non-member must see nothing from this group, got {stranger:?}"
        );

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
                "TEST-GROUPS-SHAREDLEAK-SHARER",
                "TEST-GROUPS-SHAREDLEAK-STRANGER",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_shared_trains_for_user_returns_one_row_per_group_a_train_is_shared_into \
                -- --ignored`"]
    async fn list_shared_trains_for_user_returns_one_row_per_group_a_train_is_shared_into() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GROUPS-SHAREDTWO-SHARER").await;
        seed_user(&pool, "TEST-GROUPS-SHAREDTWO-VIEWER").await;
        let train_id = seed_train_subscription(&pool, "TEST-GROUPS-SHAREDTWO-SHARER").await;
        let mut group_ids = Vec::new();
        for name in ["Shared Two A", "Shared Two B"] {
            let group_id = create_group(&pool, name, "TEST-GROUPS-SHAREDTWO-SHARER")
                .await
                .expect("create group");
            sqlx::query(
                "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
            )
            .bind(&group_id)
            .bind("TEST-GROUPS-SHAREDTWO-VIEWER")
            .execute(&pool)
            .await
            .expect("seed member");
            add_train_to_group(&pool, &group_id, train_id, "TEST-GROUPS-SHAREDTWO-SHARER")
                .await
                .expect("add");
            group_ids.push(group_id);
        }

        let shared = list_shared_trains_for_user(&pool, "TEST-GROUPS-SHAREDTWO-VIEWER")
            .await
            .expect("list shared trains");
        assert_eq!(
            shared.len(),
            2,
            "one row per (group, train) pair, so the frontend can tag the \
             merged row with BOTH group names"
        );
        let mut names: Vec<&str> = shared.iter().map(|t| t.group_name.as_str()).collect();
        names.sort_unstable();
        assert_eq!(names, vec!["Shared Two A", "Shared Two B"]);
        assert!(
            shared.iter().all(|t| t.train_subscription_id == train_id),
            "both rows describe the same underlying subscription"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        for group_id in &group_ids {
            sqlx::query("DELETE FROM groups WHERE id = $1")
                .bind(group_id)
                .execute(&pool)
                .await
                .ok();
        }
        cleanup(
            &pool,
            &[
                "TEST-GROUPS-SHAREDTWO-SHARER",
                "TEST-GROUPS-SHAREDTWO-VIEWER",
            ],
        )
        .await;
    }

    // -----------------------------------------------------------------
    // Custom-line group grants.
    // -----------------------------------------------------------------

    /// Seeds one custom line owned by `user_id` through the real write
    /// path (`insert_custom_line`), so the row -- and the `pinned_lines`
    /// row it creates alongside -- is exactly what the app would produce.
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

    /// Groups first (cascading their members, trains, invite links and
    /// grants), then each user's custom lines and pins, then the users --
    /// `groups.created_by` and `custom_line_group_grants.granted_by` both
    /// reference `users(id)` with no cascade, so users must go last.
    async fn cleanup_lines_groups_and_users(pool: &PgPool, group_ids: &[&str], user_ids: &[&str]) {
        for id in group_ids {
            sqlx::query("DELETE FROM groups WHERE id = $1")
                .bind(id)
                .execute(pool)
                .await
                .ok();
        }
        for id in user_ids {
            sqlx::query("DELETE FROM custom_lines WHERE user_id = $1")
                .bind(id)
                .execute(pool)
                .await
                .ok();
            sqlx::query("DELETE FROM pinned_lines WHERE user_id = $1")
                .bind(id)
                .execute(pool)
                .await
                .ok();
        }
        cleanup(pool, user_ids).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                grant_custom_line_rejects_a_line_the_caller_does_not_own -- --ignored \
                --test-threads=1`"]
    async fn grant_custom_line_rejects_a_line_the_caller_does_not_own() {
        // A group's OWNER has no standing whatsoever over a member's
        // private custom line (design §2.3). Proves the refusal really
        // refused, not merely returned `false`: the table is queried
        // directly afterwards.
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-ADD-GOWNER-1").await;
        seed_user(&pool, "TEST-GRANT-ADD-STRANGER-1").await;
        let group_id = create_group(&pool, "Grant Add Test 1", "TEST-GRANT-ADD-GOWNER-1")
            .await
            .expect("create group");
        let line_id = seed_custom_line(&pool, "TEST-GRANT-ADD-STRANGER-1", "Grant Add 1").await;

        let granted = grant_custom_line(&pool, &group_id, &line_id, "TEST-GRANT-ADD-GOWNER-1")
            .await
            .expect("grant attempt");
        assert!(!granted);
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM custom_line_group_grants WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count grants");
        assert_eq!(count.0, 0, "the refused grant must not have been written");

        cleanup_lines_groups_and_users(
            &pool,
            &[&group_id],
            &["TEST-GRANT-ADD-GOWNER-1", "TEST-GRANT-ADD-STRANGER-1"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                grant_custom_line_is_idempotent -- --ignored --test-threads=1`"]
    async fn grant_custom_line_is_idempotent() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-IDEM-OWNER").await;
        let group_id = create_group(&pool, "Grant Idem Test", "TEST-GRANT-IDEM-OWNER")
            .await
            .expect("create group");
        let line_id = seed_custom_line(&pool, "TEST-GRANT-IDEM-OWNER", "Grant Idem").await;

        assert!(
            grant_custom_line(&pool, &group_id, &line_id, "TEST-GRANT-IDEM-OWNER")
                .await
                .expect("first grant")
        );
        assert!(
            grant_custom_line(&pool, &group_id, &line_id, "TEST-GRANT-IDEM-OWNER")
                .await
                .expect("second grant is a no-op, not an error")
        );
        let count: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM custom_line_group_grants WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count grants");
        assert_eq!(count.0, 1);

        cleanup_lines_groups_and_users(&pool, &[&group_id], &["TEST-GRANT-IDEM-OWNER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_custom_line_grant_a_non_manager_can_only_remove_their_own -- --ignored \
                --test-threads=1`"]
    async fn remove_custom_line_grant_a_non_manager_can_only_remove_their_own() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-RM-GOWNER").await;
        seed_user(&pool, "TEST-GRANT-RM-SHARER").await;
        seed_user(&pool, "TEST-GRANT-RM-BYSTANDER").await;
        let group_id = create_group(&pool, "Grant Remove Test", "TEST-GRANT-RM-GOWNER")
            .await
            .expect("create group");
        for member in ["TEST-GRANT-RM-SHARER", "TEST-GRANT-RM-BYSTANDER"] {
            sqlx::query(
                "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
            )
            .bind(&group_id)
            .bind(member)
            .execute(&pool)
            .await
            .expect("seed member");
        }
        let line_id = seed_custom_line(&pool, "TEST-GRANT-RM-SHARER", "Grant Remove").await;
        grant_custom_line(&pool, &group_id, &line_id, "TEST-GRANT-RM-SHARER")
            .await
            .expect("grant");

        // A plain member who didn't grant it: refused.
        assert!(
            !remove_custom_line_grant(
                &pool,
                &group_id,
                &line_id,
                "TEST-GRANT-RM-BYSTANDER",
                false,
            )
            .await
            .expect("bystander remove attempt")
        );
        // The granter themselves, still a plain member: allowed.
        assert!(
            remove_custom_line_grant(&pool, &group_id, &line_id, "TEST-GRANT-RM-SHARER", false)
                .await
                .expect("sharer remove")
        );

        cleanup_lines_groups_and_users(
            &pool,
            &[&group_id],
            &[
                "TEST-GRANT-RM-GOWNER",
                "TEST-GRANT-RM-SHARER",
                "TEST-GRANT-RM-BYSTANDER",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_custom_line_grant_allows_a_manager_to_remove_anyones -- --ignored \
                --test-threads=1`"]
    async fn remove_custom_line_grant_allows_a_manager_to_remove_anyones() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-RM2-GOWNER").await;
        seed_user(&pool, "TEST-GRANT-RM2-SHARER").await;
        let group_id = create_group(&pool, "Grant Remove Test 2", "TEST-GRANT-RM2-GOWNER")
            .await
            .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GRANT-RM2-SHARER")
        .execute(&pool)
        .await
        .expect("seed member");
        let line_id = seed_custom_line(&pool, "TEST-GRANT-RM2-SHARER", "Grant Remove 2").await;
        grant_custom_line(&pool, &group_id, &line_id, "TEST-GRANT-RM2-SHARER")
            .await
            .expect("grant");

        assert!(
            remove_custom_line_grant(&pool, &group_id, &line_id, "TEST-GRANT-RM2-GOWNER", true)
                .await
                .expect("manager remove")
        );
        // The line itself is completely untouched -- only the group's
        // visibility into it changed (design §2.4).
        let still_there: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM custom_lines WHERE id = $1")
            .bind(&line_id)
            .fetch_one(&pool)
            .await
            .expect("count lines");
        assert_eq!(still_there.0, 1);

        cleanup_lines_groups_and_users(
            &pool,
            &[&group_id],
            &["TEST-GRANT-RM2-GOWNER", "TEST-GRANT-RM2-SHARER"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                remove_member_does_not_touch_custom_line_group_grants_even_when_the_departing_member_is_the_grantor \
                -- --ignored --test-threads=1`"]
    async fn remove_member_does_not_touch_custom_line_group_grants_even_when_the_departing_member_is_the_grantor()
     {
        // The direct proof of design §2.7, and the one test here that
        // actively DISPROVES a `group_trains`-shaped intuition rather than
        // confirming one -- so it asserts both halves in the same
        // transaction's aftermath: the departed member's shared TRAIN is
        // pulled (existing behaviour, unchanged) while their granted LINE
        // survives.
        //
        // The reasoning, not just the outcome: `group_trains`' cleanup
        // exists because a shared train's underlying subscription is the
        // departing member's own private row and can vanish the moment
        // they untrack it. A custom line's ownership is completely
        // unaffected by anyone leaving a group -- there is no ownership
        // event here at all, and no successor rule, because
        // `custom_lines.user_id` has no transfer path of any kind. The
        // remaining members' access was a deliberate, still-standing
        // choice by an owner who retains three ways to revoke it.
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-LEAVE-GOWNER").await;
        seed_user(&pool, "TEST-GRANT-LEAVE-SHARER").await;
        let group_id = create_group(&pool, "Grant Leave Test", "TEST-GRANT-LEAVE-GOWNER")
            .await
            .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GRANT-LEAVE-SHARER")
        .execute(&pool)
        .await
        .expect("seed member");
        let line_id = seed_custom_line(&pool, "TEST-GRANT-LEAVE-SHARER", "Grant Leave").await;
        grant_custom_line(&pool, &group_id, &line_id, "TEST-GRANT-LEAVE-SHARER")
            .await
            .expect("grant");
        let train_id = seed_train_subscription(&pool, "TEST-GRANT-LEAVE-SHARER").await;
        add_train_to_group(&pool, &group_id, train_id, "TEST-GRANT-LEAVE-SHARER")
            .await
            .expect("share a train too");

        let outcome = remove_member(&pool, &group_id, "TEST-GRANT-LEAVE-SHARER")
            .await
            .expect("remove member");
        assert_eq!(outcome, RemoveMemberOutcome::Removed { new_owner: None });

        let grants: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM custom_line_group_grants WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count grants");
        assert_eq!(
            grants.0, 1,
            "the departed grantor's custom-line grant must SURVIVE (§2.7)"
        );
        let trains: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM group_trains WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count trains");
        assert_eq!(
            trains.0, 0,
            "control: the departed member's shared train is still pulled, unchanged"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(train_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_lines_groups_and_users(
            &pool,
            &[&group_id],
            &["TEST-GRANT-LEAVE-GOWNER", "TEST-GRANT-LEAVE-SHARER"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_group_cascades_custom_line_group_grants -- --ignored --test-threads=1`"]
    async fn delete_group_cascades_custom_line_group_grants() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-GDEL-OWNER").await;
        let group_id = create_group(&pool, "Grant Group Delete", "TEST-GRANT-GDEL-OWNER")
            .await
            .expect("create group");
        let line_id = seed_custom_line(&pool, "TEST-GRANT-GDEL-OWNER", "Grant Group Delete").await;
        grant_custom_line(&pool, &group_id, &line_id, "TEST-GRANT-GDEL-OWNER")
            .await
            .expect("grant");

        assert!(delete_group(&pool, &group_id).await.expect("delete group"));
        let grants: (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM custom_line_group_grants WHERE group_id = $1")
                .bind(&group_id)
                .fetch_one(&pool)
                .await
                .expect("count grants");
        assert_eq!(grants.0, 0, "grants should cascade with the group");
        // The line itself outlives the group it was shared into.
        let lines: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM custom_lines WHERE id = $1")
            .bind(&line_id)
            .fetch_one(&pool)
            .await
            .expect("count lines");
        assert_eq!(lines.0, 1);

        cleanup_lines_groups_and_users(&pool, &[&group_id], &["TEST-GRANT-GDEL-OWNER"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_shared_custom_lines_for_user_excludes_own_lines_and_other_peoples_groups \
                -- --ignored --test-threads=1`"]
    async fn list_shared_custom_lines_for_user_excludes_own_lines_and_other_peoples_groups() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-SHARED-SHARER").await;
        seed_user(&pool, "TEST-GRANT-SHARED-VIEWER").await;
        seed_user(&pool, "TEST-GRANT-SHARED-STRANGER").await;
        let group_id = create_group(&pool, "Grant Shared Test", "TEST-GRANT-SHARED-SHARER")
            .await
            .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-GRANT-SHARED-VIEWER")
        .execute(&pool)
        .await
        .expect("seed member");
        let sharer_line =
            seed_custom_line(&pool, "TEST-GRANT-SHARED-SHARER", "Grant Shared A").await;
        let viewer_line =
            seed_custom_line(&pool, "TEST-GRANT-SHARED-VIEWER", "Grant Shared B").await;
        grant_custom_line(&pool, &group_id, &sharer_line, "TEST-GRANT-SHARED-SHARER")
            .await
            .expect("grant the sharer's line");
        grant_custom_line(&pool, &group_id, &viewer_line, "TEST-GRANT-SHARED-VIEWER")
            .await
            .expect("grant the viewer's own line");

        let shared = list_shared_custom_lines_for_user(&pool, "TEST-GRANT-SHARED-VIEWER")
            .await
            .expect("list");
        assert_eq!(
            shared.len(),
            1,
            "the viewer's OWN granted line must not come back as a shared one, got {shared:?}"
        );
        assert_eq!(shared[0].line_id, sharer_line);
        assert_eq!(shared[0].group_id, group_id);
        assert_eq!(shared[0].group_name, "Grant Shared Test");
        assert_eq!(shared[0].line_name, "Grant Shared A");
        assert_eq!(
            shared[0].granted_by_name.as_deref(),
            Some("TEST-GRANT-SHARED-SHARER")
        );

        let stranger = list_shared_custom_lines_for_user(&pool, "TEST-GRANT-SHARED-STRANGER")
            .await
            .expect("list for a stranger");
        assert!(
            stranger.is_empty(),
            "a non-member must see nothing from this group, got {stranger:?}"
        );

        cleanup_lines_groups_and_users(
            &pool,
            &[&group_id],
            &[
                "TEST-GRANT-SHARED-SHARER",
                "TEST-GRANT-SHARED-VIEWER",
                "TEST-GRANT-SHARED-STRANGER",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_group_custom_lines_and_groups_shared_with_line_round_trip -- --ignored \
                --test-threads=1`"]
    async fn list_group_custom_lines_and_groups_shared_with_line_round_trip() {
        let pool = connect().await;
        seed_user(&pool, "TEST-GRANT-LIST-OWNER").await;
        let group_a = create_group(&pool, "Grant List A", "TEST-GRANT-LIST-OWNER")
            .await
            .expect("create group A");
        let group_b = create_group(&pool, "Grant List B", "TEST-GRANT-LIST-OWNER")
            .await
            .expect("create group B");
        let line_id = seed_custom_line(&pool, "TEST-GRANT-LIST-OWNER", "Grant List Line").await;
        for group_id in [&group_a, &group_b] {
            grant_custom_line(&pool, group_id, &line_id, "TEST-GRANT-LIST-OWNER")
                .await
                .expect("grant");
        }

        let in_a = list_group_custom_lines(&pool, &group_a)
            .await
            .expect("list");
        assert_eq!(in_a.len(), 1);
        assert_eq!(in_a[0].line_id, line_id);
        assert_eq!(in_a[0].line_name, "Grant List Line");
        assert_eq!(in_a[0].granted_by, "TEST-GRANT-LIST-OWNER");
        assert!(in_a[0].granted_by_name.is_some());

        let groups = groups_shared_with_line(&pool, &line_id, "TEST-GRANT-LIST-OWNER")
            .await
            .expect("groups for line");
        let names: Vec<&str> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["Grant List A", "Grant List B"]);

        // Owner-only at the QUERY level, not just at the route's own
        // `if is_owner` branch: a fellow member of one of these groups
        // must never learn which OTHER groups the line reaches.
        seed_user(&pool, "TEST-GRANT-LIST-MEMBER").await;
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_a)
        .bind("TEST-GRANT-LIST-MEMBER")
        .execute(&pool)
        .await
        .expect("seed member");
        assert!(
            groups_shared_with_line(&pool, &line_id, "TEST-GRANT-LIST-MEMBER")
                .await
                .expect("groups for a non-owner")
                .is_empty(),
            "a granted member must get nothing back from this query"
        );

        cleanup_lines_groups_and_users(
            &pool,
            &[&group_a, &group_b],
            &["TEST-GRANT-LIST-OWNER", "TEST-GRANT-LIST-MEMBER"],
        )
        .await;
    }
}

#[cfg(test)]
mod custom_line_grant_wire_shape_tests {
    use super::*;

    /// Pins `GroupCustomLine`'s exact JSON key set, for the same reason
    /// `group_train_json_never_includes_ticket_or_notification_fields`
    /// pins `GroupTrain`'s: this shape crosses a privacy boundary (it
    /// reaches every member of a group, for a line only one of them owns),
    /// so a field added here by accident is exactly the failure that
    /// matters. In particular it must never grow a `grantedByEmail` field,
    /// or any other raw-email one: `GroupCustomLine::from` collapses the
    /// sharer down to `users::display_label` (name, else username, else
    /// nothing) precisely so that an email address can never become one
    /// member's label shown to the rest of a group.
    #[test]
    fn group_custom_line_json_is_identity_and_attribution_only() {
        let value = serde_json::to_value(GroupCustomLine {
            line_id: "custom-my-commute".to_string(),
            line_name: "My Commute".to_string(),
            granted_by: "user-1".to_string(),
            granted_by_name: Some("Alex".to_string()),
        })
        .expect("serialize");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["grantedBy", "grantedByName", "lineId", "lineName"]
        );
    }

    /// `SharedCustomLine` is `GroupCustomLine` plus exactly
    /// `groupId`/`groupName` -- this shape is strictly MORE exposed (it
    /// reaches a member on the home page without them opening the group at
    /// all), so no future edit may widen it past what the group page
    /// already shows the same member.
    #[test]
    fn shared_custom_line_json_is_group_custom_line_plus_group_attribution() {
        let value = serde_json::to_value(SharedCustomLine {
            group_id: "group-1".to_string(),
            group_name: "Family".to_string(),
            line_id: "custom-my-commute".to_string(),
            line_name: "My Commute".to_string(),
            granted_by: "user-1".to_string(),
            granted_by_name: Some("Alex".to_string()),
        })
        .expect("serialize");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "grantedBy",
                "grantedByName",
                "groupId",
                "groupName",
                "lineId",
                "lineName",
            ]
        );
    }

    /// The same `users::display_label` collapse every other attribution
    /// field in this module applies: `name`, else `username`, else
    /// nothing -- a blank is not a name, and an email address is never a
    /// label shown to the rest of a group, whichever claim it arrived in.
    #[test]
    fn granted_by_name_falls_back_to_username_and_never_to_an_email() {
        let row = |name: Option<&str>, username: Option<&str>| GroupCustomLineRow {
            line_id: "custom-x".to_string(),
            line_name: "X".to_string(),
            granted_by: "user-1".to_string(),
            granted_by_name: name.map(str::to_string),
            granted_by_username: username.map(str::to_string),
        };
        assert_eq!(
            GroupCustomLine::from(row(Some("Alex"), Some("alex"))).granted_by_name,
            Some("Alex".to_string())
        );
        assert_eq!(
            GroupCustomLine::from(row(None, Some("alex"))).granted_by_name,
            Some("alex".to_string())
        );
        assert_eq!(
            GroupCustomLine::from(row(Some("   "), Some("alex"))).granted_by_name,
            Some("alex".to_string()),
            "a blank name is not a name"
        );
        assert_eq!(
            GroupCustomLine::from(row(Some("alex@example.com"), Some("alex"))).granted_by_name,
            Some("alex".to_string()),
            "an email address in the name claim must not become the label"
        );
        assert_eq!(
            GroupCustomLine::from(row(Some("alex@example.com"), None)).granted_by_name,
            None,
            "with no usable non-email label at all, nothing is shown"
        );
        assert_eq!(GroupCustomLine::from(row(None, None)).granted_by_name, None);
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
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
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
mod shared_train_wire_shape_tests {
    use super::*;

    fn shared_train() -> SharedTrain {
        SharedTrain {
            group_id: "group-1".to_string(),
            group_name: "Family".to_string(),
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
        }
    }

    /// Pins the exact JSON keys `SharedTrain` serializes to, for the same
    /// reason `group_train_json_never_includes_ticket_or_notification_fields`
    /// pins `GroupTrain`'s: spec §4's "Never shown" list (tickets,
    /// `notificationsEnabled`, exact `trackedAt`) is a hard constraint, and
    /// this shape reaches a member on `/track/mine` without them opening
    /// the group at all. Identical to `GroupTrain`'s key set plus exactly
    /// `groupId`/`groupName`.
    #[test]
    fn shared_train_json_is_group_train_plus_group_attribution_and_nothing_else() {
        let value = serde_json::to_value(shared_train()).expect("serialize");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "addedBy",
                "addedByName",
                "customName",
                "delayMinutes",
                "groupId",
                "groupName",
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

    /// `added_by_name` collapses `name`, else `username`, else nothing --
    /// the "shared by <who>" half of the row's attribution, and the same
    /// collapse (`users::display_label`) `GroupTrain::from` applies one
    /// struct away. Never the sharer's email: this row reaches a member
    /// who never even opened the group, so if anything it is the LAST
    /// place that should be laxer than the group detail page.
    #[test]
    fn added_by_name_falls_back_to_username_then_to_nothing_but_never_an_email() {
        let row = |name: Option<&str>, username: Option<&str>| SharedTrainRow {
            group_id: "group-1".to_string(),
            group_name: "Family".to_string(),
            train_subscription_id: 1,
            pin_origin_crs: None,
            pin_destination_crs: None,
            pin_origin_name: None,
            pin_destination_name: None,
            pin_scheduled_departure: None,
            service_date: "2026-09-11".parse().unwrap(),
            resolution_status: "pending".to_string(),
            train_uid: None,
            status: None,
            delay_minutes: None,
            custom_name: None,
            added_by: "user-1".to_string(),
            added_by_name: name.map(str::to_string),
            added_by_username: username.map(str::to_string),
        };

        assert_eq!(
            SharedTrain::from(row(Some("Alex"), Some("alex"))).added_by_name,
            Some("Alex".to_string())
        );
        assert_eq!(
            SharedTrain::from(row(None, Some("alex"))).added_by_name,
            Some("alex".to_string())
        );
        // Blank, not just absent -- what an IdP with no name on file for
        // the sharer actually sends, and what used to render this row's
        // attribution as "Shared by " with nothing after it.
        assert_eq!(
            SharedTrain::from(row(Some("  "), Some("alex"))).added_by_name,
            Some("alex".to_string())
        );
        assert_eq!(SharedTrain::from(row(None, None)).added_by_name, None);
        assert_eq!(
            SharedTrain::from(row(Some("alex@example.com"), Some("alex@example.com")))
                .added_by_name,
            None
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
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["displayName", "joinedAt", "role", "userId"]);
    }
}

/// The `users.name` -> display-label collapse both member rows and
/// shared-train attribution go through.
///
/// An identity provider that has no name on file for a user does not
/// necessarily OMIT the `name` claim -- Authentik (whose stock `profile`
/// scope mapping this deployment attaches) returns its `User.name`
/// attribute verbatim, and that attribute defaults to the EMPTY STRING --
/// and `users.name` is written straight from that claim
/// (`data::users::upsert_user`). So a blank name is a real, stored value
/// these two `From` impls have to cope with, not a hypothetical, and
/// `Some("")` is `Some`: it used to be handed to the frontend as the whole
/// label, where `?? 'A member'` doesn't fire either because `""` isn't
/// null. Result: a member row that is just a role badge, and a
/// "Shared by " with nothing after it.
#[cfg(test)]
mod display_name_collapse_tests {
    use super::*;

    fn member_row(name: Option<&str>, username: Option<&str>) -> GroupMemberRow {
        GroupMemberRow {
            user_id: "user-1".to_string(),
            name: name.map(str::to_string),
            username: username.map(str::to_string),
            role: "member".to_string(),
            joined_at: "2026-09-11T00:00:00Z".parse().unwrap(),
        }
    }

    fn train_row(name: Option<&str>, username: Option<&str>) -> GroupTrainRow {
        GroupTrainRow {
            train_subscription_id: 1,
            pin_origin_crs: None,
            pin_destination_crs: None,
            pin_origin_name: None,
            pin_destination_name: None,
            pin_scheduled_departure: None,
            service_date: "2026-09-11".parse().unwrap(),
            resolution_status: "pending".to_string(),
            train_uid: None,
            status: None,
            delay_minutes: None,
            custom_name: None,
            added_by: "user-1".to_string(),
            added_by_name: name.map(str::to_string),
            added_by_username: username.map(str::to_string),
        }
    }

    #[test]
    fn a_real_name_is_the_members_display_name() {
        let member = GroupMember::from(member_row(Some("Ada Rider"), Some("ada")));
        assert_eq!(member.display_name.as_deref(), Some("Ada Rider"));
    }

    #[test]
    fn a_padded_name_is_trimmed_for_a_member() {
        let member = GroupMember::from(member_row(Some("  Ada Rider  "), None));
        assert_eq!(member.display_name.as_deref(), Some("Ada Rider"));
    }

    /// The reported bug, at the member-list end: a blank name is not a
    /// label. It falls through to the username, and -- with neither on
    /// file -- to `None`, NOT to `Some("")`. `None` is what lets the
    /// frontend's own "A member"/"a member" placeholder fire.
    #[test]
    fn a_blank_name_falls_through_to_the_username_for_a_member() {
        let member = GroupMember::from(member_row(Some(""), Some("ada")));
        assert_eq!(member.display_name.as_deref(), Some("ada"));

        let member = GroupMember::from(member_row(Some("   "), Some("ada")));
        assert_eq!(member.display_name.as_deref(), Some("ada"));
    }

    #[test]
    fn no_name_and_no_username_collapses_to_none_for_a_member() {
        assert_eq!(
            GroupMember::from(member_row(Some(""), Some(""))).display_name,
            None
        );
        assert_eq!(GroupMember::from(member_row(None, None)).display_name, None);
    }

    #[test]
    fn a_real_name_is_the_shared_train_attribution() {
        let train = GroupTrain::from(train_row(Some("Ada Rider"), Some("ada")));
        assert_eq!(train.added_by_name.as_deref(), Some("Ada Rider"));
    }

    #[test]
    fn a_blank_name_falls_through_to_the_username_for_shared_train_attribution() {
        let train = GroupTrain::from(train_row(Some(""), Some("ada")));
        assert_eq!(train.added_by_name.as_deref(), Some("ada"));

        let train = GroupTrain::from(train_row(Some("\t\n"), Some("ada")));
        assert_eq!(train.added_by_name.as_deref(), Some("ada"));
    }

    #[test]
    fn no_name_and_no_username_collapses_to_none_for_shared_train_attribution() {
        assert_eq!(
            GroupTrain::from(train_row(Some(""), None)).added_by_name,
            None
        );
        assert_eq!(GroupTrain::from(train_row(None, None)).added_by_name, None);
    }

    /// The privacy half of this fix, end to end: neither of these two "who
    /// is this person" labels may ever be a member's email address.
    /// Neither query selects `users.email` any more, AND an email arriving
    /// through the fields they DO select -- an IdP that puts an address in
    /// `name` or `preferred_username`, which is legal and common -- is
    /// declined by `users::display_label` rather than rendered. What the
    /// group sees instead is the frontend's generic placeholder.
    #[test]
    fn a_member_is_never_attributed_by_email() {
        for row in [
            member_row(Some(""), Some("rider@example.com")),
            member_row(Some("rider@example.com"), None),
            member_row(Some("rider@example.com"), Some("rider@example.com")),
        ] {
            let member = GroupMember::from(row);
            assert_eq!(member.display_name, None);
            let json = serde_json::to_string(&member).expect("serialize");
            assert!(!json.contains('@'), "member JSON leaked an email: {json}");
        }

        for row in [
            train_row(Some(""), Some("rider@example.com")),
            train_row(Some("rider@example.com"), None),
        ] {
            let train = GroupTrain::from(row);
            assert_eq!(train.added_by_name, None);
            let json = serde_json::to_string(&train).expect("serialize");
            assert!(!json.contains('@'), "train JSON leaked an email: {json}");
        }
    }
}
