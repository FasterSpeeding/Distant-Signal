//! Self-service account deletion and personal-data export (UK legal audit
//! LEG-4, 2026-09-27; UK GDPR Arts. 15, 17 and 20).
//!
//! Every table that holds personal data is keyed to `users(id)`, directly
//! or through a parent row, and every foreign key to `users(id)` has an
//! explicit `ON DELETE` action (migration
//! `20260927060000_users_fk_on_delete_actions.sql`). So deletion is one
//! `DELETE FROM users` plus the two things a cascade cannot express:
//!
//! 1. **Group ownership.** A group belongs to all of its members, not only
//!    to the person who created it. Before the user row goes, the user
//!    leaves every group through the same code path as a voluntary "Leave
//!    group" (`groups::remove_member_in_tx`): an owned group with other
//!    members is handed to the longest-standing admin (else member), and an
//!    owned group with no other members is deleted. `groups.created_by` is
//!    then set to NULL by its FK.
//! 2. **Share links naming the user's journeys.** `unlisted_links` is
//!    polymorphic (`resource_type`/`resource_id`, no FK to `journeys`), so
//!    links pointing at the user's journeys are deleted explicitly, in case
//!    anyone other than the user ever created one.
//!
//! See `docs/personal-data-retention.md` for the full per-table table.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::PgPool;

use crate::data::groups::{self, RemoveMemberOutcome};

/// Every table with a foreign key to `users(id)`. The export covers each
/// one, and `every_users_fk_table_is_exported` (a DB-gated test) fails if a
/// new table referencing `users(id)` is added without being added here and
/// to [`export_account`]. Child tables with no direct `users` FK
/// (`journey_legs`, `journey_template_legs`,
/// `journey_template_skipped_dates`) are exported nested under their
/// parents.
pub const USER_KEYED_TABLES: &[&str] = &[
    "custom_line_group_grants",
    "custom_lines",
    "group_invite_links",
    "group_journeys",
    "group_members",
    "group_trains",
    "groups",
    "journey_leg_notification_state",
    "journey_templates",
    "journeys",
    "line_notification_state",
    "pinned_lines",
    "pinned_operators",
    "pinned_stations",
    "push_subscriptions",
    "sessions",
    "tracked_train_tickets",
    "train_notification_state",
    "train_subscriptions",
    "unlisted_links",
];

/// The exact confirmation phrase `DELETE /public/account` requires in its
/// body (compared case-insensitively, surrounding whitespace ignored). The
/// frontend asks the user to type it.
pub const DELETE_ACCOUNT_CONFIRMATION: &str = "delete my account";

/// What [`delete_account`] did to the groups the user belonged to, for the
/// route's log line and the tests.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDeletion {
    /// Groups the user left that carry on for their other members.
    pub groups_left: u32,
    /// Of those, groups the user owned whose ownership was handed over.
    pub groups_transferred: u32,
    /// Groups the user owned alone, which were deleted.
    pub groups_deleted: u32,
}

/// Deletes `user_id` and all of their personal data in one transaction.
/// Returns `Ok(None)` if there is no such user (already deleted).
///
/// The user row is locked first (`FOR UPDATE`), so a concurrent request on
/// one of the user's still-live sessions that inserts a new row keyed to
/// them waits on its FK check and then fails, rather than leaving an
/// orphan behind this transaction.
pub async fn delete_account(pool: &PgPool, user_id: &str) -> Result<Option<AccountDeletion>> {
    let mut tx = pool.begin().await?;
    let locked: Option<(String,)> = sqlx::query_as("SELECT id FROM users WHERE id = $1 FOR UPDATE")
        .bind(user_id)
        .fetch_optional(&mut *tx)
        .await?;
    if locked.is_none() {
        tx.rollback().await?;
        return Ok(None);
    }

    let mut outcome = AccountDeletion::default();
    // Sorted, so two deletions touching the same groups take the `groups`
    // row locks (`remove_member_in_tx`'s `FOR UPDATE`) in the same order.
    let group_ids: Vec<String> = sqlx::query_scalar(
        "SELECT group_id FROM group_members WHERE user_id = $1 ORDER BY group_id",
    )
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    for group_id in group_ids {
        match groups::remove_member_in_tx(&mut tx, &group_id, user_id, true).await? {
            RemoveMemberOutcome::Removed { new_owner } => {
                outcome.groups_left += 1;
                if new_owner.is_some() {
                    outcome.groups_transferred += 1;
                }
            }
            RemoveMemberOutcome::GroupDeleted => outcome.groups_deleted += 1,
            // Raced with a concurrent leave: nothing left to do.
            RemoveMemberOutcome::NotAMember => {}
            // Unreachable with `remover_is_target = true`.
            RemoveMemberOutcome::OwnerCannotBeRemoved => {
                anyhow::bail!("remove_member refused a self-removal from group {group_id}")
            }
        }
    }

    sqlx::query(
        "DELETE FROM unlisted_links \
         WHERE resource_type = 'journey' \
           AND resource_id IN (SELECT id::text FROM journeys WHERE user_id = $1)",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    // Cascades to every other table in `USER_KEYED_TABLES` (and on to
    // their children); `groups.created_by` is set to NULL.
    sqlx::query("DELETE FROM users WHERE id = $1")
        .bind(user_id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;
    Ok(Some(outcome))
}

/// Everything `export_account` returns: every row of personal data held
/// about the user, one key per category. Rows are each table's own columns
/// (`to_jsonb`, so snake_case column names), minus the few columns listed
/// in `omitted`, which are security credentials rather than information
/// about the user.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountExport {
    pub format_version: u32,
    pub exported_at: DateTime<Utc>,
    pub account: Value,
    pub sessions: Value,
    pub push_subscriptions: Value,
    pub pinned_lines: Value,
    pub pinned_stations: Value,
    pub pinned_operators: Value,
    pub custom_lines: Value,
    pub custom_line_group_grants: Value,
    pub tracked_trains: Value,
    pub tickets: Value,
    pub journeys: Value,
    pub journey_templates: Value,
    pub group_memberships: Value,
    pub group_shared_trains: Value,
    pub group_shared_journeys: Value,
    pub share_links: Value,
    pub group_invite_links: Value,
    pub notification_state: NotificationStateExport,
    pub omitted: Vec<&'static str>,
    pub notes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStateExport {
    pub trains: Value,
    pub lines: Value,
    pub journey_legs: Value,
}

/// Runs `inner` (a query yielding one `jsonb` column per row, with `$1`
/// bound to the user id) and returns its rows as a JSON array.
async fn rows(pool: &PgPool, inner: &str, user_id: &str) -> Result<Value> {
    let sql = format!("SELECT COALESCE(jsonb_agg(q.j), '[]'::jsonb) FROM ({inner}) AS q(j)");
    Ok(sqlx::query_scalar(&sql)
        .bind(user_id)
        .fetch_one(pool)
        .await?)
}

/// Builds the user's personal-data export (UK GDPR Arts. 15 and 20).
/// Returns `Ok(None)` if there is no such user.
pub async fn export_account(pool: &PgPool, user_id: &str) -> Result<Option<AccountExport>> {
    let account: Option<Value> =
        sqlx::query_scalar("SELECT to_jsonb(u) FROM users u WHERE id = $1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    let Some(account) = account else {
        return Ok(None);
    };

    Ok(Some(AccountExport {
        format_version: 1,
        exported_at: Utc::now(),
        account,
        sessions: rows(
            pool,
            "SELECT to_jsonb(s) - 'id' - 'refresh_token' FROM sessions s \
             WHERE user_id = $1 ORDER BY created_at",
            user_id,
        )
        .await?,
        push_subscriptions: rows(
            pool,
            "SELECT to_jsonb(p) - 'p256dh' - 'auth' FROM push_subscriptions p \
             WHERE user_id = $1 ORDER BY id",
            user_id,
        )
        .await?,
        pinned_lines: rows(
            pool,
            "SELECT to_jsonb(p) FROM pinned_lines p WHERE user_id = $1 ORDER BY pinned_at",
            user_id,
        )
        .await?,
        pinned_stations: rows(
            pool,
            "SELECT to_jsonb(p) FROM pinned_stations p WHERE user_id = $1 ORDER BY pinned_at",
            user_id,
        )
        .await?,
        pinned_operators: rows(
            pool,
            "SELECT to_jsonb(p) FROM pinned_operators p WHERE user_id = $1 ORDER BY pinned_at",
            user_id,
        )
        .await?,
        custom_lines: rows(
            pool,
            "SELECT to_jsonb(c) FROM custom_lines c WHERE user_id = $1 ORDER BY created_at",
            user_id,
        )
        .await?,
        custom_line_group_grants: rows(
            pool,
            "SELECT to_jsonb(g) FROM custom_line_group_grants g \
             WHERE granted_by = $1 ORDER BY granted_at",
            user_id,
        )
        .await?,
        tracked_trains: rows(
            pool,
            "SELECT to_jsonb(t) FROM train_subscriptions t \
             WHERE user_id = $1 ORDER BY service_date, id",
            user_id,
        )
        .await?,
        tickets: rows(
            pool,
            "SELECT to_jsonb(t) FROM tracked_train_tickets t WHERE user_id = $1 ORDER BY id",
            user_id,
        )
        .await?,
        journeys: rows(
            pool,
            "SELECT to_jsonb(j) || jsonb_build_object('legs', ( \
                 SELECT COALESCE(jsonb_agg(to_jsonb(l) ORDER BY l.leg_order), '[]'::jsonb) \
                 FROM journey_legs l WHERE l.journey_id = j.id)) \
             FROM journeys j WHERE user_id = $1 ORDER BY id",
            user_id,
        )
        .await?,
        journey_templates: rows(
            pool,
            "SELECT to_jsonb(t) || jsonb_build_object( \
                 'legs', (SELECT COALESCE(jsonb_agg(to_jsonb(l) ORDER BY l.leg_order), '[]'::jsonb) \
                          FROM journey_template_legs l WHERE l.template_id = t.id), \
                 'skipped_dates', (SELECT COALESCE(jsonb_agg(to_jsonb(s) ORDER BY s.service_date), '[]'::jsonb) \
                          FROM journey_template_skipped_dates s WHERE s.template_id = t.id)) \
             FROM journey_templates t WHERE user_id = $1 ORDER BY id",
            user_id,
        )
        .await?,
        // Only the user's own membership rows and each group's name --
        // other members' names are their personal data, not this user's.
        group_memberships: rows(
            pool,
            "SELECT jsonb_build_object( \
                 'group_id', g.id, 'group_name', g.name, 'role', m.role, \
                 'joined_at', m.joined_at, 'group_created_by_you', g.created_by IS NOT DISTINCT FROM $1) \
             FROM group_members m JOIN groups g ON g.id = m.group_id \
             WHERE m.user_id = $1 ORDER BY m.joined_at",
            user_id,
        )
        .await?,
        group_shared_trains: rows(
            pool,
            "SELECT to_jsonb(g) FROM group_trains g WHERE added_by = $1 ORDER BY added_at",
            user_id,
        )
        .await?,
        group_shared_journeys: rows(
            pool,
            "SELECT to_jsonb(g) FROM group_journeys g WHERE added_by = $1 ORDER BY added_at",
            user_id,
        )
        .await?,
        share_links: rows(
            pool,
            "SELECT to_jsonb(u) - 'token_hash' FROM unlisted_links u \
             WHERE created_by = $1 ORDER BY created_at",
            user_id,
        )
        .await?,
        group_invite_links: rows(
            pool,
            "SELECT to_jsonb(i) - 'token_hash' FROM group_invite_links i \
             WHERE created_by = $1 ORDER BY created_at",
            user_id,
        )
        .await?,
        notification_state: NotificationStateExport {
            trains: rows(
                pool,
                "SELECT to_jsonb(n) FROM train_notification_state n WHERE user_id = $1",
                user_id,
            )
            .await?,
            lines: rows(
                pool,
                "SELECT to_jsonb(n) FROM line_notification_state n WHERE user_id = $1",
                user_id,
            )
            .await?,
            journey_legs: rows(
                pool,
                "SELECT to_jsonb(n) FROM journey_leg_notification_state n WHERE user_id = $1",
                user_id,
            )
            .await?,
        },
        omitted: vec![
            "sessions.id and sessions.refresh_token: hashed session credentials",
            "push_subscriptions.p256dh and push_subscriptions.auth: your browser's push encryption keys",
            "share_links.token_hash and group_invite_links.token_hash: hashed link credentials",
        ],
        notes: vec![
            "Your login identity (name, username, email) comes from the single sign-on provider; \
             it holds its own copy, which this export does not include.",
            "Uploaded tickets are read in memory and never stored; only the ticket details listed \
             under `tickets` are kept.",
            "Database backups are encrypted and kept for 7 days.",
        ],
    }))
}

#[cfg(test)]
pub(crate) mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    pub(crate) async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn exec(pool: &PgPool, sql: &str, binds: &[&str]) {
        let mut query = sqlx::query(sql);
        for bind in binds {
            query = query.bind(*bind);
        }
        query
            .execute(pool)
            .await
            .unwrap_or_else(|err| panic!("{sql}: {err}"));
    }

    async fn scalar_i64(pool: &PgPool, sql: &str, binds: &[&str]) -> i64 {
        let mut query = sqlx::query_scalar::<_, i64>(sql);
        for bind in binds {
            query = query.bind(*bind);
        }
        query
            .fetch_one(pool)
            .await
            .unwrap_or_else(|err| panic!("{sql}: {err}"))
    }

    pub(crate) async fn seed_user(pool: &PgPool, id: &str) {
        exec(
            pool,
            "INSERT INTO users (id, email, name, username) VALUES ($1, $1 || '@example.com', 'Name ' || $1, $1)",
            &[id],
        )
        .await;
    }

    /// Removes any leftovers from a previous failed run of a test using
    /// the `prefix` user ids.
    pub(crate) async fn cleanup(pool: &PgPool, prefix: &str) {
        let pattern = format!("{prefix}%");
        exec(pool, "DELETE FROM groups WHERE id LIKE $1", &[&pattern]).await;
        exec(pool, "DELETE FROM users WHERE id LIKE $1", &[&pattern]).await;
    }

    /// Gives `user` one or more rows in EVERY table keyed to users(id)
    /// (and every child table), as if they had used every feature:
    /// sessions, push, pins, a custom line granted to a group, a tracked
    /// train with a ticket and notification state, a standalone ticket, a
    /// journey with a leg and a share link, a template with a leg and a
    /// skipped date, three groups (one owned with another member, one owned
    /// alone, one owned by `other` that `user` joined) with invite links
    /// and shared trains/journeys.
    pub(crate) async fn seed_every_feature(pool: &PgPool, user: &str, other: &str) {
        let u = &[user][..];
        seed_user(pool, user).await;
        seed_user(pool, other).await;
        exec(pool, "INSERT INTO sessions (id, user_id, expires_at) VALUES ('sess-' || $1, $1, NOW() + INTERVAL '1 day')", u).await;
        exec(pool, "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth) VALUES ($1, 'https://push.example/' || $1, 'k', 'a')", u).await;
        exec(
            pool,
            "INSERT INTO pinned_lines (user_id, line_id) VALUES ($1, 'line-' || $1)",
            u,
        )
        .await;
        exec(
            pool,
            "INSERT INTO pinned_stations (user_id, crs) VALUES ($1, 'KGX')",
            u,
        )
        .await;
        exec(
            pool,
            "INSERT INTO pinned_operators (user_id, operator_code) VALUES ($1, 'GR')",
            u,
        )
        .await;
        exec(pool, "INSERT INTO line_notification_state (user_id, line_id, last_notified_severity_rank, last_notified_at) VALUES ($1, 'line-' || $1, 1, NOW())", u).await;
        exec(pool, "INSERT INTO custom_lines (id, name, user_id, stations) VALUES ('cl-' || $1, 'My line', $1, '{KGX}')", u).await;

        exec(pool, "INSERT INTO train_subscriptions (user_id, service_date, resolution_status) VALUES ($1, CURRENT_DATE, 'pending')", u).await;
        let sub = format!(
            "(SELECT id FROM train_subscriptions WHERE user_id = '{user}' ORDER BY id LIMIT 1)"
        );
        exec(pool, &format!("INSERT INTO tracked_train_tickets (tracked_train_id, user_id, source) VALUES ({sub}, $1, 'manual')"), u).await;
        exec(
            pool,
            "INSERT INTO tracked_train_tickets (user_id, source) VALUES ($1, 'manual')",
            u,
        )
        .await;
        exec(pool, &format!("INSERT INTO train_notification_state (user_id, tracked_train_id, last_notified_status, last_notified_at) VALUES ($1, {sub}, 'on_time', NOW())"), u).await;

        exec(
            pool,
            "INSERT INTO journey_templates (user_id, custom_name) VALUES ($1, 'Commute')",
            u,
        )
        .await;
        let tmpl = format!("(SELECT id FROM journey_templates WHERE user_id = '{user}' LIMIT 1)");
        exec(pool, &format!("INSERT INTO journey_template_legs (template_id, leg_order, origin_crs, destination_crs) VALUES ({tmpl}, 0, 'KGX', 'YRK')"), &[]).await;
        exec(pool, &format!("INSERT INTO journey_template_skipped_dates (template_id, service_date) VALUES ({tmpl}, CURRENT_DATE)"), &[]).await;
        exec(pool, &format!("INSERT INTO journeys (user_id, custom_name, source_template_id) VALUES ($1, 'Trip', {tmpl})"), u).await;
        let journey = format!("(SELECT id FROM journeys WHERE user_id = '{user}' LIMIT 1)");
        exec(pool, &format!("INSERT INTO journey_legs (journey_id, leg_order, service_date, train_subscription_id) VALUES ({journey}, 0, CURRENT_DATE, {sub})"), &[]).await;
        let leg = format!("(SELECT id FROM journey_legs WHERE journey_id = {journey} LIMIT 1)");
        exec(pool, &format!("INSERT INTO journey_leg_notification_state (user_id, journey_leg_id, last_notified_skipped, last_notified_at) VALUES ($1, {leg}, false, NOW())"), u).await;
        exec(pool, &format!("INSERT INTO unlisted_links (token_hash, resource_type, resource_id, created_by) VALUES ('tok-' || $1, 'journey', {journey}::text, $1)"), u).await;

        // Owned, with another member (an admin who should inherit it).
        let shared = format!("{user}-g-shared");
        let alone = format!("{user}-g-alone");
        let joined = format!("{user}-g-joined");
        for (group, owner) in [(&shared, user), (&alone, user), (&joined, other)] {
            exec(
                pool,
                "INSERT INTO groups (id, name, created_by) VALUES ($1, $1, $2)",
                &[group, owner],
            )
            .await;
            exec(
                pool,
                "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'owner')",
                &[group, owner],
            )
            .await;
        }
        exec(
            pool,
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'admin')",
            &[&shared, other],
        )
        .await;
        exec(
            pool,
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
            &[&joined, user],
        )
        .await;
        for group in [&shared, &joined] {
            exec(pool, "INSERT INTO group_invite_links (token_hash, group_id, created_by, expires_at) VALUES ('inv-' || $1, $1, $2, NOW() + INTERVAL '1 day')", &[group, user]).await;
            exec(pool, &format!("INSERT INTO group_trains (group_id, train_subscription_id, added_by) VALUES ($1, {sub}, $2)"), &[group, user]).await;
            exec(pool, &format!("INSERT INTO group_journeys (group_id, journey_id, added_by) VALUES ($1, {journey}, $2)"), &[group, user]).await;
            exec(pool, "INSERT INTO custom_line_group_grants (group_id, line_id, granted_by) VALUES ($1, 'cl-' || $2, $2)", &[group, user]).await;
        }
    }

    /// Every (table, column) with a foreign key to users(id), from the
    /// live catalog -- so a table added later is checked automatically.
    async fn users_fk_columns(pool: &PgPool) -> Vec<(String, String)> {
        sqlx::query_as(
            "SELECT c.conrelid::regclass::text, a.attname::text \
             FROM pg_constraint c \
             JOIN pg_attribute a ON a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey) \
             WHERE c.contype = 'f' AND c.confrelid = 'users'::regclass \
             ORDER BY 1, 2",
        )
        .fetch_all(pool)
        .await
        .expect("list users FKs")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
    async fn every_users_fk_table_is_exported() {
        let pool = connect().await;
        let mut tables: Vec<String> = users_fk_columns(&pool)
            .await
            .into_iter()
            .map(|(table, _)| table)
            .collect();
        // Sort in Rust: the query's ORDER BY uses the database collation,
        // which orders e.g. "groups"/"group_trains" differently from byte order.
        tables.sort();
        tables.dedup();
        let mut expected: Vec<String> = USER_KEYED_TABLES.iter().map(|t| t.to_string()).collect();
        expected.sort();
        assert_eq!(
            tables, expected,
            "USER_KEYED_TABLES (and export_account, delete_account and \
             docs/personal-data-retention.md) must cover exactly the tables with a FK to users(id)"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
    async fn deleting_a_user_who_used_every_feature_leaves_no_rows_referencing_them() {
        let pool = connect().await;
        let (user, other) = ("acct-del-user", "acct-del-other");
        cleanup(&pool, "acct-del-").await;
        seed_every_feature(&pool, user, other).await;

        // Sanity: the seed really did touch every users(id) FK column.
        let fk_columns = users_fk_columns(&pool).await;
        for (table, column) in &fk_columns {
            let n = scalar_i64(
                &pool,
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1"),
                &[user],
            )
            .await;
            assert!(n > 0, "seed left {table}.{column} empty for the user");
        }
        let journey_ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM journeys WHERE user_id = $1")
                .bind(user)
                .fetch_all(&pool)
                .await
                .unwrap();

        let outcome = delete_account(&pool, user)
            .await
            .expect("delete succeeds")
            .expect("user existed");
        assert_eq!(
            outcome,
            AccountDeletion {
                groups_left: 2,
                groups_transferred: 1,
                groups_deleted: 1,
            }
        );

        for (table, column) in &fk_columns {
            let n = scalar_i64(
                &pool,
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1"),
                &[user],
            )
            .await;
            assert_eq!(n, 0, "{table}.{column} still references the deleted user");
        }
        assert_eq!(
            scalar_i64(&pool, "SELECT COUNT(*) FROM users WHERE id = $1", &[user]).await,
            0
        );
        // Child tables with no users FK went with their parents.
        for (sql, what) in [
            (
                "SELECT COUNT(*) FROM journey_legs WHERE journey_id = ANY($1)",
                "journey_legs",
            ),
            (
                "SELECT COUNT(*) FROM unlisted_links WHERE resource_type = 'journey' AND resource_id = ANY($1::bigint[]::text[])",
                "journey share links",
            ),
        ] {
            let n: i64 = sqlx::query_scalar(sql)
                .bind(&journey_ids)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert_eq!(n, 0, "{what} survived");
        }
        let orphan_template_legs = scalar_i64(
            &pool,
            "SELECT COUNT(*) FROM journey_template_legs l \
             WHERE NOT EXISTS (SELECT 1 FROM journey_templates t WHERE t.id = l.template_id)",
            &[],
        )
        .await;
        assert_eq!(orphan_template_legs, 0);

        // The shared group carries on under the other member, now owner.
        let shared = format!("{user}-g-shared");
        let role: String = sqlx::query_scalar(
            "SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2",
        )
        .bind(&shared)
        .bind(other)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(role, "owner");
        let created_by: Option<String> =
            sqlx::query_scalar("SELECT created_by FROM groups WHERE id = $1")
                .bind(&shared)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(created_by, None);
        // The group only the user was in is gone; the other user's group stays.
        let alone = format!("{user}-g-alone");
        let joined = format!("{user}-g-joined");
        assert_eq!(
            scalar_i64(
                &pool,
                "SELECT COUNT(*) FROM groups WHERE id = $1",
                &[&alone]
            )
            .await,
            0
        );
        assert_eq!(
            scalar_i64(
                &pool,
                "SELECT COUNT(*) FROM group_members WHERE group_id = $1",
                &[&joined]
            )
            .await,
            1
        );

        // Idempotent.
        assert!(delete_account(&pool, user).await.unwrap().is_none());
        cleanup(&pool, "acct-del-").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api -- --ignored --test-threads=1`"]
    async fn export_contains_every_category() {
        let pool = connect().await;
        let (user, other) = ("acct-exp-user", "acct-exp-other");
        cleanup(&pool, "acct-exp-").await;
        seed_every_feature(&pool, user, other).await;

        let export = export_account(&pool, user)
            .await
            .expect("export succeeds")
            .expect("user exists");
        let json = serde_json::to_value(&export).unwrap();
        let object = json.as_object().unwrap();

        assert_eq!(json["account"]["id"], user);
        for key in [
            "sessions",
            "pushSubscriptions",
            "pinnedLines",
            "pinnedStations",
            "pinnedOperators",
            "customLines",
            "customLineGroupGrants",
            "trackedTrains",
            "tickets",
            "journeys",
            "journeyTemplates",
            "groupMemberships",
            "groupSharedTrains",
            "groupSharedJourneys",
            "shareLinks",
            "groupInviteLinks",
        ] {
            let rows = object[key]
                .as_array()
                .unwrap_or_else(|| panic!("{key} is not an array"));
            assert!(!rows.is_empty(), "export category {key} is empty");
        }
        for key in ["trains", "lines", "journeyLegs"] {
            assert!(
                !json["notificationState"][key]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "notificationState.{key} is empty"
            );
        }
        assert_eq!(json["tickets"].as_array().unwrap().len(), 2);
        assert_eq!(json["groupMemberships"].as_array().unwrap().len(), 3);
        assert_eq!(json["journeys"][0]["legs"].as_array().unwrap().len(), 1);
        assert_eq!(
            json["journeyTemplates"][0]["legs"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            json["journeyTemplates"][0]["skipped_dates"]
                .as_array()
                .unwrap()
                .len(),
            1
        );

        // Credentials are left out; other users' data is not included.
        let text = json.to_string();
        for secret in [
            "sess-acct-exp-user",
            "tok-acct-exp-user",
            "inv-acct-exp-user",
        ] {
            assert!(!text.contains(secret), "export leaked {secret}");
        }
        assert!(json["sessions"][0].get("id").is_none());
        assert!(json["pushSubscriptions"][0].get("auth").is_none());
        assert!(json["pushSubscriptions"][0].get("p256dh").is_none());
        assert!(
            !text.contains("Name acct-exp-other"),
            "export leaked another member's name"
        );

        assert!(
            export_account(&pool, "acct-exp-nobody")
                .await
                .unwrap()
                .is_none()
        );
        cleanup(&pool, "acct-exp-").await;
    }
}
