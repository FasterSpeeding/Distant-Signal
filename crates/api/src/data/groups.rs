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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMember {
    pub user_id: String,
    pub name: Option<String>,
    pub email: Option<String>,
    pub role: GroupRole,
    pub joined_at: DateTime<Utc>,
}

impl From<GroupMemberRow> for GroupMember {
    fn from(row: GroupMemberRow) -> Self {
        GroupMember {
            user_id: row.user_id,
            name: row.name,
            email: row.email,
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
}
