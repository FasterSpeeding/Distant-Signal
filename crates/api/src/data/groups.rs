//! CRUD + permissions for shared groups (`groups`, `group_members`,
//! `group_trains`, `group_invite_links`). See
//! docs/superpowers/specs/2026-09-11-shared-groups-design.md.

use anyhow::Result;
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
}
