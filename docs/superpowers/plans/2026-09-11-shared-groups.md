# Shared Groups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let users create named groups, join them via a shareable link, and share individual tracked trains (custom name + live status, never tickets) into a group so other members can see them.

**Architecture:** Four new Postgres tables (`groups`, `group_members`, `group_trains`, `group_invite_links`), a single `crates/api/src/data/groups.rs` data-access module (mirroring `train_tracking.rs`'s one-file-per-feature-area convention), a single `crates/api/src/routes/groups.rs` route module merged into the existing session-authenticated `public_router()`, and five new Next.js pages plus a handful of small Client Components following this codebase's established button/modal/`useNeedsLogin` conventions.

**Tech Stack:** Rust (axum, sqlx/Postgres), Next.js App Router (React Server Components + Mantine), Vitest.

**Spec:** `docs/superpowers/specs/2026-09-11-shared-groups-design.md`

## Global Constraints

- Four new tables, not three: `groups`, `group_members`, `group_trains`, `group_invite_links` (spec §2).
- Three-tier role (`owner`/`admin`/`member`) on `group_members.role`. The creator is a **permanent** `owner`, inserted in the same transaction as the group — never removable or demotable by an `admin` (spec §2.1, §3).
- `group_trains` is a join table (`(group_id, train_subscription_id)` composite PK), not a `group_id` FK on `train_subscriptions` — a train can be shared into more than one group (spec §2.2).
- Departed-member cleanup is automatic: removing a `group_members` row deletes that user's `group_trains` rows (`added_by = <user>`) in the *same transaction* (spec §2.2, decided).
- Last-owner-leaves-a-non-empty-group auto-promotes the longest-standing remaining `admin` (by `joined_at`), or failing that the longest-standing remaining `member`, to `owner`. An owner leaving an otherwise-empty group deletes the group instead (spec §2.1, decided).
- Invite links are reusable and rotatable, default 7-day expiry; regenerating revokes the old one and mints a new one in the same call (spec §2.3, decided).
- `/groups/join/{token}` is confirm-before-join — never joins on `GET`, only on an explicit `POST` from a rendered confirm page (spec §2.3, decided).
- Tickets are **never** shared under any circumstance — no `group_trains`/group-detail query may join `tracked_train_tickets`, and no group-trains response type may carry a ticket field (spec §4, hard constraint).
- A shared train's display carries attribution ("shared by {member}") and the tracker's computed default name, but never `notifications_enabled` or the exact `tracked_at` timestamp (spec §4, decided).
- All 15 new HTTP routes are mounted under the existing session-authenticated `public_router()` (final path `/public/groups/...`), never the internal-token-gated `private_router()` (spec §5).
- New nav item "Groups", top-level, alongside "All Lines"/"Station Lookup"/"Find a Train"/"My Trains & Tickets" in `frontend/app/layout.tsx`, visible only to authenticated users (spec §6, decided).
- DB-backed backend tests are written `#[ignore]`d, following `crates/api/src/data/custom_lines.rs`'s `db_tests` convention. Run them with:
  `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`
  (run `sqlx migrate run` against that database first so this plan's new migration is applied).
- Frontend tests run with `npm test` (`vitest run`) from `frontend/`.
- No new dependency in `Cargo.toml`/`package.json` — every primitive needed (`auth::generate_session_token`, `auth::hash_session_token` is not needed here since invite tokens are stored verbatim as the table's own primary key, unlike session tokens) already exists.

---

## Task 1: Migration for all four tables

**Files:**
- Create: `crates/api/migrations/20260911090000_shared_groups.sql`

**Interfaces:**
- Produces: tables `groups(id, name, created_by, created_at)`, `group_members(group_id, user_id, role, joined_at)`, `group_trains(group_id, train_subscription_id, added_by, added_at)`, `group_invite_links(token, group_id, created_by, created_at, expires_at, revoked_at)`. Every later task's SQL depends on these exact column names/types.

- [ ] **Step 1: Write the migration file**

```sql
-- -------------------------------------------------------------------------
-- Shared groups: named groups with join-link-based membership. A tracked
-- train added to a group becomes visible to other group members (custom
-- name + live status, never tickets). See
-- docs/superpowers/specs/2026-09-11-shared-groups-design.md.
--
-- Four tables, not three:
--   groups              -- one row per group. `id` is a short random id,
--                          the same shape as auth::generate_session_token()
--                          (base64url of 32 random bytes), doubling as a
--                          non-guessable URL identifier.
--   group_members        -- membership + role. Three tiers, not two: the
--                          creator is a PERMANENT `owner`, inserted in the
--                          same transaction as the group -- never
--                          removable/demotable by a co-`admin` (§2.1).
--   group_trains          -- a JOIN TABLE (not a `group_id` FK on
--                          train_subscriptions) so one train can be shared
--                          into more than one group at once (§2.2). This
--                          repo already paid the cost of under-modeling an
--                          analogous relationship once for
--                          tracked_train_tickets
--                          (20260901140000_standalone_tickets.sql) -- a
--                          join table avoids repeating that here.
--   group_invite_links     -- reusable, rotatable join links. `token` reuses
--                          auth::generate_session_token()'s own opaque,
--                          high-entropy shape directly as the primary key
--                          (unlike sessions.id, this is NOT hashed --
--                          the token doubles as a bearer capability meant
--                          to be shared verbatim via a URL, and a group's
--                          own membership list is the actual access-control
--                          boundary once someone has joined, not secrecy of
--                          this table's contents).
-- -------------------------------------------------------------------------

CREATE TABLE groups (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    created_by  TEXT NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE group_members (
    group_id    TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id     TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role        TEXT NOT NULL DEFAULT 'member' CHECK (role IN ('owner', 'admin', 'member')),
    joined_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, user_id)
);

-- "Which groups is user X in" (GET /groups) -- the PK's leading column
-- (group_id) doesn't cover this.
CREATE INDEX group_members_user_id ON group_members (user_id);

CREATE TABLE group_trains (
    group_id               TEXT   NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    train_subscription_id  BIGINT NOT NULL REFERENCES train_subscriptions(id) ON DELETE CASCADE,
    added_by               TEXT   NOT NULL REFERENCES users(id),
    added_at               TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, train_subscription_id)
);

-- Untracking a train removes it from every group it was shared into
-- immediately, automatically, with no application code needed -- the same
-- cascade pattern already used for train_movement_events/train_current_state.
-- (This comment documents the FK above; ON DELETE CASCADE is already part
-- of the column definition.)

CREATE INDEX group_trains_train_subscription_id ON group_trains (train_subscription_id);

CREATE TABLE group_invite_links (
    token       TEXT PRIMARY KEY,
    group_id    TEXT NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    created_by  TEXT NOT NULL REFERENCES users(id),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at  TIMESTAMPTZ NOT NULL,
    revoked_at  TIMESTAMPTZ
);

-- "The group's currently active link" (GET /groups/{id}, POST rotate,
-- DELETE revoke) is always looked up by group_id first.
CREATE INDEX group_invite_links_group_id ON group_invite_links (group_id);
```

- [ ] **Step 2: Apply the migration against the local test database and confirm it succeeds**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test sqlx migrate run --source crates/api/migrations`
Expected: output includes `Applied 20260911090000/migrate shared groups`.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260911090000_shared_groups.sql
git commit -m "Add migration for the four shared-groups tables"
```

---

## Task 2: Data layer — group CRUD + role model

**Files:**
- Create: `crates/api/src/data/groups.rs`
- Modify: `crates/api/src/data/mod.rs` (add `pub mod groups;`)

**Interfaces:**
- Consumes: `crate::auth::generate_session_token() -> String` (`crates/api/src/auth.rs`).
- Produces (consumed by Tasks 3-9):
  - `pub enum GroupRole { Owner, Admin, Member }` with `fn can_manage(self) -> bool`, `fn is_owner(self) -> bool`, `fn from_db(raw: &str) -> Self`, deriving `Debug, Clone, Copy, PartialEq, Eq, Serialize`.
  - `pub async fn create_group(pool: &PgPool, name: &str, user_id: &str) -> Result<String>`
  - `pub async fn list_groups_for_user(pool: &PgPool, user_id: &str) -> Result<Vec<GroupSummary>>`
  - `pub struct GroupSummary { pub id: String, pub name: String, pub role: GroupRole, pub member_count: i64 }` (`Serialize`, camelCase)
  - `pub async fn get_member_role(pool: &PgPool, group_id: &str, user_id: &str) -> Result<Option<GroupRole>>`
  - `pub struct GroupDetail { pub id: String, pub name: String, pub owner_id: String, pub owner_name: Option<String>, pub member_count: i64, pub role: GroupRole }`
  - `pub async fn get_group_detail(pool: &PgPool, group_id: &str, user_id: &str) -> Result<Option<GroupDetail>>`
  - `pub async fn rename_group(pool: &PgPool, group_id: &str, new_name: &str) -> Result<bool>`
  - `pub async fn delete_group(pool: &PgPool, group_id: &str) -> Result<bool>`

- [ ] **Step 1: Write the module skeleton, the role enum, and its unit tests**

```rust
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
```

- [ ] **Step 2: Run the unit tests to confirm they pass**

Run: `cargo test -p api groups::role_tests`
Expected: 3 tests pass.

- [ ] **Step 3: Add `create_group`, `list_groups_for_user`, and their `db_tests`**

```rust
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
```

```rust
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
}
```

- [ ] **Step 4: Run the new db_tests against the local test database**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api create_group_inserts_the_creator_as_a_permanent_owner list_groups_for_user_returns_only_the_callers_own_groups -- --ignored`
Expected: both tests pass.

- [ ] **Step 5: Add `get_member_role`, `GroupDetail`/`get_group_detail`, `rename_group`, `delete_group`**

```rust
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
```

- [ ] **Step 6: Add db_tests for the Step 5 functions**

```rust
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

        let deleted = delete_group(&pool, &group_id).await.expect("delete group");
        assert!(deleted);

        let role = get_member_role(&pool, &group_id, "TEST-GROUPS-DELETE-OWNER")
            .await
            .expect("query");
        assert_eq!(role, None, "membership row should have cascaded away");

        cleanup(&pool, &["TEST-GROUPS-DELETE-OWNER"]).await;
    }
```

- [ ] **Step 7: Wire the module into `crates/api/src/data/mod.rs`**

Find the alphabetically-sorted `pub mod` list in `crates/api/src/data/mod.rs` and add `pub mod groups;` between `pub mod eta_blend;` and `pub mod island_of_ireland;` (alphabetical order, matching every other entry in that file).

- [ ] **Step 8: Run every test in the new module**

Run: `cargo test -p api groups::`
Expected: the 3 role tests pass; the `db_tests` show as `ignored` (no `DATABASE_URL` set in this run).

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/data/groups.rs crates/api/src/data/mod.rs
git commit -m "Add group CRUD data-layer functions and the three-tier role model"
```

---

## Task 3: Data layer — membership mutations + permission edge cases

**Files:**
- Modify: `crates/api/src/data/groups.rs` (append)

**Interfaces:**
- Consumes: `GroupRole` (Task 2).
- Produces (consumed by Task 7):
  - `pub struct GroupMember { pub user_id: String, pub name: Option<String>, pub email: Option<String>, pub role: GroupRole, pub joined_at: DateTime<Utc> }` (`Serialize`, camelCase)
  - `pub async fn list_members(pool: &PgPool, group_id: &str) -> Result<Vec<GroupMember>>`
  - `pub async fn promote_to_admin(pool: &PgPool, group_id: &str, target_user_id: &str) -> Result<bool>`
  - `pub enum RemoveMemberOutcome { NotAMember, Removed { new_owner: Option<String> }, GroupDeleted }`
  - `pub async fn remove_member(pool: &PgPool, group_id: &str, target_user_id: &str) -> Result<RemoveMemberOutcome>`

- [ ] **Step 1: Write `GroupMember`/`list_members` and their db_test**

```rust
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
```

- [ ] **Step 2: Run a quick compile check**

Run: `cargo check -p api`
Expected: compiles with no errors (unused-function warnings are fine at this point — `list_members` has no caller yet).

- [ ] **Step 3: Write `promote_to_admin` and its db_tests**

```rust
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
```

```rust
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
```

- [ ] **Step 4: Run the promote tests**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api promote_to_admin -- --ignored`
Expected: both tests pass.

- [ ] **Step 5: Write `RemoveMemberOutcome`/`remove_member`, implementing departed-member cleanup, ownership transfer, and group-deletion-on-last-leave**

```rust
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
```

- [ ] **Step 6: Run a quick compile check**

Run: `cargo check -p api`
Expected: compiles with no errors.

- [ ] **Step 7: Write the decided-edge-case db_tests**

```rust
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
```

- [ ] **Step 8: Run every new test in this task**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api remove_member -- --ignored`
Expected: all 5 tests pass.

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/data/groups.rs
git commit -m "Add membership mutations: promote, remove/leave with ownership transfer and departed-member cleanup"
```

---

## Task 4: Data layer — invite-link generation, validation, and consumption

**Files:**
- Modify: `crates/api/src/data/groups.rs` (append)

**Interfaces:**
- Consumes: `crate::auth::generate_session_token() -> String`.
- Produces (consumed by Tasks 2/6 for `GET /groups/{id}` and Task 8's routes):
  - `pub struct InviteLink { pub token: String, pub expires_at: DateTime<Utc> }` (`Serialize`, camelCase)
  - `pub async fn rotate_invite_link(pool: &PgPool, group_id: &str, user_id: &str) -> Result<InviteLink>`
  - `pub async fn revoke_invite_link(pool: &PgPool, group_id: &str) -> Result<bool>`
  - `pub async fn get_active_invite_link(pool: &PgPool, group_id: &str) -> Result<Option<InviteLink>>`
  - `pub struct JoinPreview { pub group_id: String, pub group_name: String, pub member_count: i64 }` (`Serialize`, camelCase)
  - `pub async fn resolve_invite_link(pool: &PgPool, token: &str) -> Result<Option<JoinPreview>>`
  - `pub async fn consume_invite_link(pool: &PgPool, token: &str, user_id: &str) -> Result<Option<String>>`

- [ ] **Step 1: Write `InviteLink`/`rotate_invite_link`/`revoke_invite_link`/`get_active_invite_link`**

```rust
#[derive(Debug, Clone, Serialize)]
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
```

- [ ] **Step 2: Write db_tests for rotate/revoke/get_active**

```rust
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
```

- [ ] **Step 3: Run these two tests**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api rotate_invite_link revoke_invite_link -- --ignored`
Expected: both pass.

- [ ] **Step 4: Write `JoinPreview`/`resolve_invite_link`/`consume_invite_link`**

```rust
#[derive(Debug, Clone, Serialize)]
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
```

- [ ] **Step 5: Write db_tests covering expiry, revocation, and idempotent join**

```rust
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
```

- [ ] **Step 6: Run the full invite-link test set**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api invite_link resolve_invite_link consume_invite_link -- --ignored`
Expected: all 6 tests from this task pass.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/groups.rs
git commit -m "Add invite-link generation, rotation, revocation, resolution, and consumption"
```

---

## Task 5: Data layer — group-trains CRUD + ownership check + attribution

**Files:**
- Modify: `crates/api/src/data/groups.rs` (append)

**Interfaces:**
- Produces (consumed by Task 9's routes):
  - `pub struct GroupTrain { train_subscription_id: i64, pin_origin_crs, pin_destination_crs, pin_origin_name, pin_destination_name: Option<String>, pin_scheduled_departure: Option<DateTime<Utc>>, service_date: NaiveDate, resolution_status: String, train_uid: Option<String>, status: Option<String>, delay_minutes: Option<i32>, custom_name: Option<String>, added_by: String, added_by_name: Option<String> }` (`Serialize`, camelCase) — all `pub`.
  - `pub async fn add_train_to_group(pool: &PgPool, group_id: &str, train_subscription_id: i64, user_id: &str) -> Result<bool>`
  - `pub async fn remove_train_from_group(pool: &PgPool, group_id: &str, train_subscription_id: i64, user_id: &str, caller_can_manage: bool) -> Result<bool>`
  - `pub async fn list_group_trains(pool: &PgPool, group_id: &str) -> Result<Vec<GroupTrain>>`

- [ ] **Step 1: Write `add_train_to_group` and its db_tests**

```rust
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
```

```rust
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
```

- [ ] **Step 2: Run these two tests**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api add_train_to_group -- --ignored`
Expected: both pass.

- [ ] **Step 3: Write `remove_train_from_group` and its db_tests**

```rust
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
```

```rust
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
```

- [ ] **Step 4: Run these three tests**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api remove_train_from_group -- --ignored`
Expected: all 3 pass.

- [ ] **Step 5: Write `GroupTrain`/`list_group_trains`**

```rust
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
```

- [ ] **Step 6: Write a db_test proving list_group_trains works end to end, and a pinned wire-shape test for the ticket/notification constraint**

```rust
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
```

```rust
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
```

- [ ] **Step 7: Run every test added in this task**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api list_group_trains group_train_json -- --ignored` then `cargo test -p api group_train_json_never_includes_ticket_or_notification_fields` (the second, non-DB test runs without `DATABASE_URL`/`--ignored`).
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/data/groups.rs
git commit -m "Add group-trains CRUD: ownership-checked add, sharer-or-manager remove, attributed list"
```

---

## Task 6: Routes — group CRUD (`POST /groups`, `GET /groups`, `GET /groups/{id}`, `PUT /groups/{id}`, `DELETE /groups/{id}`)

**Files:**
- Create: `crates/api/src/routes/groups.rs`
- Modify: `crates/api/src/routes/mod.rs` (add `pub mod groups;` and merge `groups::router()` into `public_router()`)

**Interfaces:**
- Consumes: everything from `crate::data::groups` (Tasks 2-5); `crate::auth::AuthenticatedUser`.
- Produces (consumed by Tasks 7-9, which add more handlers to the same `router()` function in this same file):
  - `pub fn router() -> Router` (this task wires the 5 group-CRUD routes; Tasks 7-9 add more `.route(...)` calls to this same builder)
  - `async fn require_role(app: &App, group_id: &str, user_id: &str, predicate: fn(GroupRole) -> bool) -> Result<GroupRole, (StatusCode, String)>` — the shared permission gate every later task's handlers reuse.
  - `fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String)`

- [ ] **Step 1: Write the module doc comment, imports, `router()` stub, and the shared helpers**

```rust
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
```

- [ ] **Step 2: Run a compile check (handlers referenced by `router()` don't exist yet, so this is expected to fail)**

Run: `cargo check -p api`
Expected: fails with `cannot find function 'create_group' in this scope` (and similarly for `list_groups`/`get_group`/`rename_group`/`delete_group`) — confirms the wiring is correct and only the handlers themselves are missing.

- [ ] **Step 3: Write `validate_group_name`, `create_group`, `list_groups`**

```rust
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
```

- [ ] **Step 4: Write `get_group` (extending the response with the invite link for managers), `rename_group`, `delete_group`**

```rust
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
```

- [ ] **Step 5: Add a `router_builds_without_panicking` test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn router_builds_without_panicking() {
        let _ = router();
    }
}
```

- [ ] **Step 6: Run `cargo check -p api` and the new test**

Run: `cargo check -p api && cargo test -p api routes::groups::tests`
Expected: compiles clean; the router test passes.

- [ ] **Step 7: Wire the module into `crates/api/src/routes/mod.rs`**

In `crates/api/src/routes/mod.rs`, add `pub mod groups;` to the alphabetically-sorted `pub mod` list (between `pub mod freshness;` and `pub mod health;`), and add `.merge(groups::router())` to the `public_router()` builder (alongside the other `.merge(...)` calls, e.g. right after `.merge(auth::router())`):

```rust
pub mod groups;
```

```rust
        .merge(auth::router())
        .merge(groups::router())
        .merge(chatbot::router())
```

- [ ] **Step 8: Run the full api test suite (non-DB tests only) to confirm nothing else broke**

Run: `cargo test -p api`
Expected: all non-`#[ignore]`d tests pass, including the new `routes::groups::tests::router_builds_without_panicking`.

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/routes/groups.rs crates/api/src/routes/mod.rs
git commit -m "Add group CRUD routes: create, list, detail (with invite link for managers), rename, delete"
```

---

## Task 7: Routes — membership (`GET .../members`, `DELETE .../members/{userId}`, `POST .../members/{userId}/promote`)

**Files:**
- Modify: `crates/api/src/routes/groups.rs` (append handlers, add three `.route(...)` calls to the existing `router()`)

**Interfaces:**
- Consumes: `groups::list_members`, `groups::promote_to_admin`, `groups::remove_member`/`RemoveMemberOutcome`, `groups::get_member_role` (Task 3); `require_role`/`internal_error` (Task 6).

- [ ] **Step 1: Add the three routes to `router()`**

```rust
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
```

- [ ] **Step 2: Write `list_members`**

```rust
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
```

- [ ] **Step 3: Write `remove_member` — self-leave always allowed, removing someone else requires `can_manage`, and an `admin` can never target the `owner`**

```rust
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
```

- [ ] **Step 4: Write `promote_member` — `owner` only, and only ever a plain `member` can be promoted**

```rust
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
```

- [ ] **Step 5: Run `cargo check -p api`**

Run: `cargo check -p api`
Expected: compiles clean.

- [ ] **Step 6: Run the full non-DB test suite**

Run: `cargo test -p api`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/groups.rs
git commit -m "Add membership routes: list, remove/leave (owner-protected), promote (owner-only)"
```

---

## Task 8: Routes — invite-link + join (`POST/DELETE .../invite-link`, `GET/POST /groups/join/{token}`)

**Files:**
- Modify: `crates/api/src/routes/groups.rs` (append handlers, add two `.route(...)` calls to the existing `router()`)

**Interfaces:**
- Consumes: `groups::rotate_invite_link`, `groups::revoke_invite_link`, `groups::resolve_invite_link`, `groups::consume_invite_link` (Task 4); `require_role`/`internal_error` (Task 6).

- [ ] **Step 1: Add the two routes to `router()` — note the literal-vs-dynamic precedence needed for `/groups/join/{token}` vs `/groups/{id}`**

```rust
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
}
```

- [ ] **Step 2: Write `create_invite_link` and `revoke_invite_link`**

```rust
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
```

- [ ] **Step 3: Write `get_join_preview` (unauthenticated) and `post_join`**

```rust
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
```

- [ ] **Step 4: Add a literal-vs-dynamic precedence test, mirroring `routes::train`'s own**

```rust
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
```

- [ ] **Step 5: Run `cargo check -p api` and the new test**

Run: `cargo check -p api && cargo test -p api routes::groups::tests`
Expected: compiles clean; both router tests pass.

- [ ] **Step 6: Run the full non-DB test suite**

Run: `cargo test -p api`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/groups.rs
git commit -m "Add invite-link rotate/revoke routes and the confirm-before-join preview/consume routes"
```

---

## Task 9: Routes — group-trains (`GET/POST /groups/{id}/trains`, `DELETE /groups/{id}/trains/{trainSubscriptionId}`)

**Files:**
- Modify: `crates/api/src/routes/groups.rs` (append handlers, add two `.route(...)` calls to the existing `router()`)

**Interfaces:**
- Consumes: `groups::list_group_trains`, `groups::add_train_to_group`, `groups::remove_train_from_group`, `groups::get_member_role` (Task 5); `internal_error` (Task 6).

- [ ] **Step 1: Add the two routes to `router()`**

```rust
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
```

- [ ] **Step 2: Write `list_group_trains_route` and `add_group_train`**

```rust
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
```

- [ ] **Step 3: Write `remove_group_train`**

```rust
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
```

- [ ] **Step 4: Run `cargo check -p api` and the full non-DB test suite**

Run: `cargo check -p api && cargo test -p api`
Expected: compiles clean; all tests pass. This is the last backend task — every one of the 15 routes from the spec's §5 API table now exists.

- [ ] **Step 5: Run every `#[ignore]`d db_test added across Tasks 2-9 together, to confirm no cross-task interference**

Run: `DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1`
Expected: every `groups`-related db_test passes (alongside every other pre-existing db_test in the crate).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/groups.rs
git commit -m "Add group-trains routes: list, add (ownership-checked), remove (sharer-or-manager)"
```

---

## Task 10: Frontend — wire types + server-side read functions

**Files:**
- Modify: `frontend/lib/types.ts` (append new interfaces)
- Modify: `frontend/lib/api.ts` (add imports + 5 new functions)

**Interfaces:**
- Consumes: the JSON shapes Tasks 6-9 produce.
- Produces (consumed by Tasks 11-14):
  - `GroupRole = 'owner' | 'admin' | 'member'`
  - `interface GroupInviteLink { token: string; expiresAt: string }`
  - `interface GroupSummary { id: string; name: string; role: GroupRole; memberCount: number }`
  - `interface GroupDetail { id: string; name: string; ownerId: string; ownerName: string | null; memberCount: number; role: GroupRole; inviteLink: GroupInviteLink | null }`
  - `interface GroupMember { userId: string; name: string | null; email: string | null; role: GroupRole; joinedAt: string }`
  - `interface GroupTrain { trainSubscriptionId: number; pinOriginCrs: string | null; pinDestinationCrs: string | null; pinOriginName: string | null; pinDestinationName: string | null; pinScheduledDeparture: string | null; serviceDate: string; resolutionStatus: string; trainUid: string | null; status: string | null; delayMinutes: number | null; customName: string | null; addedBy: string; addedByName: string | null }`
  - `interface GroupJoinPreview { groupId: string; groupName: string; memberCount: number }`
  - `getMyGroups(): Promise<GroupSummary[] | null>`
  - `getGroup(id: string): Promise<GroupDetail>`
  - `getGroupMembers(id: string): Promise<GroupMember[]>`
  - `getGroupTrains(id: string): Promise<GroupTrain[]>`
  - `getGroupJoinPreview(token: string): Promise<GroupJoinPreview>`

- [ ] **Step 1: Append the new interfaces to `frontend/lib/types.ts`**

Add at the end of the file:

```typescript
// Shared groups -- see
// docs/superpowers/specs/2026-09-11-shared-groups-design.md. Mirrors
// crates/api/src/data/groups.rs's own wire shapes exactly.

export type GroupRole = 'owner' | 'admin' | 'member';

export interface GroupInviteLink {
  token: string;
  expiresAt: string; // RFC3339
}

export interface GroupSummary {
  id: string;
  name: string;
  role: GroupRole;
  memberCount: number;
}

export interface GroupDetail {
  id: string;
  name: string;
  ownerId: string;
  ownerName: string | null;
  memberCount: number;
  role: GroupRole;
  // `null` for a plain `member` -- the invite link is only ever included
  // for an `admin`/`owner` caller (see `routes::groups::get_group`'s own
  // doc comment).
  inviteLink: GroupInviteLink | null;
}

export interface GroupMember {
  userId: string;
  name: string | null;
  email: string | null;
  role: GroupRole;
  joinedAt: string; // RFC3339
}

/** A train shared into a group -- deliberately carries no ticket field and
 * no `notificationsEnabled`/exact-`trackedAt` field (spec §4's "Never
 * shown" list is a hard constraint on the backend response this mirrors). */
export interface GroupTrain {
  trainSubscriptionId: number;
  pinOriginCrs: string | null;
  pinDestinationCrs: string | null;
  pinOriginName: string | null;
  pinDestinationName: string | null;
  pinScheduledDeparture: string | null; // RFC3339
  serviceDate: string; // "YYYY-MM-DD"
  resolutionStatus: string;
  trainUid: string | null;
  status: string | null;
  delayMinutes: number | null;
  customName: string | null;
  addedBy: string;
  addedByName: string | null;
}

export interface GroupJoinPreview {
  groupId: string;
  groupName: string;
  memberCount: number;
}
```

- [ ] **Step 2: Run the TypeScript compiler to confirm the new types parse**

Run: `cd frontend && npx tsc --noEmit`
Expected: no new errors (the new interfaces aren't imported anywhere yet, so this only checks their own syntax).

- [ ] **Step 3: Add the 5 new functions to `frontend/lib/api.ts`**

Add `GroupSummary, GroupDetail, GroupMember, GroupTrain, GroupJoinPreview` to the existing `import type { ... } from './types';` block at the top of the file, then append the functions at the end of the file:

```typescript
/** `GET /public/groups` -- the current user's own groups. `null` on a
 * `401`, matching `getMyTrackedTrains()`'s own "no id in the path, no
 * second party to disambiguate" null-on-401 convention -- there is
 * nothing else this route's `401` could mean besides "not logged in." */
export async function getMyGroups(): Promise<GroupSummary[] | null> {
  const url = `${baseUrl()}/public/groups`;
  const response = await fetch(url, { cache: 'no-store', ...(await cookieForwardInit()) });
  if (response.status === 401) return null;
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<GroupSummary[]>;
}

/** `GET /public/groups/{id}` -- has an id in its path, so (unlike
 * `getMyGroups`) a `401` here is a genuine, if narrow, session-lapse case
 * and is thrown via `fetchJson`, matching `getTrackedTrainById`'s own
 * convention rather than `getMyTrackedTrains`'s null-on-401 one. */
export async function getGroup(id: string): Promise<GroupDetail> {
  const url = `${baseUrl()}/public/groups/${id}`;
  return fetchJson<GroupDetail>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

export async function getGroupMembers(id: string): Promise<GroupMember[]> {
  const url = `${baseUrl()}/public/groups/${id}/members`;
  return fetchJson<GroupMember[]>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

export async function getGroupTrains(id: string): Promise<GroupTrain[]> {
  const url = `${baseUrl()}/public/groups/${id}/trains`;
  return fetchJson<GroupTrain[]>(url, { cache: 'no-store', ...(await cookieForwardInit()) });
}

/** `GET /public/groups/join/{token}` -- unauthenticated on the backend
 * (see `routes::groups::get_join_preview`'s own doc comment), so this
 * needs no cookie forwarding either; a not-yet-logged-in visitor can see
 * the join preview before being sent through login. */
export async function getGroupJoinPreview(token: string): Promise<GroupJoinPreview> {
  const url = `${baseUrl()}/public/groups/join/${token}`;
  return fetchJson<GroupJoinPreview>(url, { cache: 'no-store' });
}
```

- [ ] **Step 4: Run the TypeScript compiler again**

Run: `cd frontend && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 5: Run the existing frontend test suite to confirm nothing broke**

Run: `cd frontend && npm test`
Expected: all existing tests still pass (nothing yet calls the new functions, so there's nothing new to test here — Tasks 11-14 add the call sites and their own tests).

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts
git commit -m "Add shared-groups wire types and server-side read functions"
```

---

## Task 11: Frontend — nav item + `/groups` list + `/groups/new` create flow

**Files:**
- Modify: `frontend/app/layout.tsx` (add `GroupsNavItem` + wire into the nav `Group`)
- Create: `frontend/app/groups/page.tsx`
- Create: `frontend/app/groups/AutoOpenLoginPrompt.tsx` (colocated, mirrors `frontend/app/track/mine/AutoOpenLoginPrompt.tsx`)
- Create: `frontend/app/groups/page.test.tsx`
- Create: `frontend/components/CreateGroupForm.tsx`
- Create: `frontend/components/CreateGroupForm.test.tsx`
- Create: `frontend/app/groups/new/page.tsx`

**Interfaces:**
- Consumes: `getMyGroups`, `getSession` (Task 10 / existing `lib/api.ts`); `LoginPromptModal`, `useNeedsLogin`, `TextLink` (existing components).
- Produces: `GroupsNavItem` (Server Component, no exported type needed elsewhere), `CreateGroupForm` (Client Component, no props).

- [ ] **Step 1: Add `GroupsNavItem` to `frontend/app/layout.tsx` and wire it into the nav**

Add this function next to `TrackedTrainsNavItem` (after its closing brace, around line 120):

```tsx
// New top-level nav item, alongside "All Lines"/"Station Lookup"/"Find a
// Train"/"My Trains & Tickets" (spec §6, decided). Visible only to
// authenticated users -- unlike `TrackedTrainsNavItem` (reclassified to
// always-visible, see that function's own doc comment above), a group has
// no useful anonymous-visitor landing state at all (an anonymous "Groups"
// click has nothing to show but a login prompt with zero context), so this
// stays gated the same way `AuthNavItem` gates on `getSession()` -- a
// separate async Server Component behind its own `<Suspense>` so a slow/
// failed session check can't block the rest of the shell.
async function GroupsNavItem() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  if (!session.authenticated) return null;
  return <TextLink href="/groups">Groups</TextLink>;
}
```

Then add it to the nav `Group`, right after `<TrackedTrainsNavItem />`:

```tsx
                    <TrackedTrainsNavItem />
                    <Suspense fallback={null}>
                      <GroupsNavItem />
                    </Suspense>
                    <DataFreshnessNavItem freshness={freshness} />
```

- [ ] **Step 2: Run the frontend build to confirm the layout still compiles and prerenders**

Run: `cd frontend && npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 3: Create the colocated login-prompt wrapper for `/groups`**

```tsx
'use client';

import { useState } from 'react';
import { LoginPromptModal } from '@/components/LoginPromptModal';

/** Colocated copy of `app/track/mine/AutoOpenLoginPrompt.tsx` for the
 * `/groups` list page -- same "a Server Component can't hold the
 * `useState` a controlled `LoginPromptModal` needs" reasoning, kept
 * page-local rather than shared cross-directory, matching this codebase's
 * existing colocation convention for this exact component. */
export function AutoOpenLoginPrompt({ children }: { children: React.ReactNode }) {
  const [opened, setOpened] = useState(true);
  return (
    <LoginPromptModal opened={opened} onClose={() => setOpened(false)}>
      {children}
    </LoginPromptModal>
  );
}
```

- [ ] **Step 4: Create `frontend/app/groups/page.tsx`**

```tsx
import { Badge, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyGroups } from '@/lib/api';
import { AutoOpenLoginPrompt } from './AutoOpenLoginPrompt';
import { TextLink } from '@/components/TextLink';
import type { GroupSummary } from '@/lib/types';

// See app/page.tsx's own `revalidate = 0` comment: no dynamic segment on
// this route, so without this Next.js tries to prerender it during `next
// build`, which fails since the `api` service only exists at runtime.
export const revalidate = 0;

/** `/groups` -- list of the current user's groups: name, member count,
 * role badge, "Create group" CTA (spec §6). */
export default async function GroupsPage() {
  const groups = await getMyGroups();

  if (groups === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Groups</Title>
        <AutoOpenLoginPrompt>Log in to see your groups.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>Groups</Title>
        <TextLink href="/groups/new">Create group</TextLink>
      </Group>
      {groups.length === 0 ? (
        <Text c="dimmed">
          You&apos;re not in any groups yet. <Link href="/groups/new">Create one</Link> to share tracked trains
          with other people.
        </Text>
      ) : (
        <Stack gap="xs">
          {groups.map((group) => (
            <GroupRow key={group.id} group={group} />
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function GroupRow({ group }: { group: GroupSummary }) {
  // Plain <Link> wrapping the Card, not `component={Link}` on the Mantine
  // polymorphic prop -- this is a Server Component, and passing `Link` as
  // a value into a Mantine `component` prop from one previously broke
  // `next build`'s Server/Client boundary check (see `app/layout.tsx`'s
  // own comment on its nav-bar `<Link>` for the same reasoning).
  return (
    <Link href={`/groups/${group.id}`} style={{ textDecoration: 'none', color: 'inherit' }}>
      <Card withBorder>
        <Group justify="space-between">
          <Text fw={500}>{group.name}</Text>
          <Group gap="xs">
            <Badge variant="light">
              {group.memberCount} member{group.memberCount === 1 ? '' : 's'}
            </Badge>
            <Badge variant="outline">{group.role}</Badge>
          </Group>
        </Group>
      </Card>
    </Link>
  );
}
```

- [ ] **Step 5: Write `frontend/app/groups/page.test.tsx`**

```tsx
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import GroupsPage from './page';
import { getMyGroups } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  getMyGroups: vi.fn(),
}));

vi.mock('next/navigation', () => ({
  usePathname: () => '/groups',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('GroupsPage', () => {
  it('shows a login prompt when getMyGroups returns null', async () => {
    vi.mocked(getMyGroups).mockResolvedValue(null);
    render(await GroupsPage());
    expect(await screen.findByText('Log in to see your groups.')).toBeInTheDocument();
  });

  it('shows an empty-state message with no groups', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([]);
    render(await GroupsPage());
    expect(screen.getByText(/not in any groups yet/)).toBeInTheDocument();
  });

  it('lists each group with its name, member count, and role badge', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([
      { id: 'grp-1', name: 'Family', role: 'owner', memberCount: 3 },
    ]);
    render(await GroupsPage());
    expect(screen.getByText('Family')).toBeInTheDocument();
    expect(screen.getByText('3 members')).toBeInTheDocument();
    expect(screen.getByText('owner')).toBeInTheDocument();
  });

  it('singularizes the member count for exactly one member', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([
      { id: 'grp-1', name: 'Solo', role: 'owner', memberCount: 1 },
    ]);
    render(await GroupsPage());
    expect(screen.getByText('1 member')).toBeInTheDocument();
  });
});
```

- [ ] **Step 6: Run the new test**

Run: `cd frontend && npx vitest run app/groups/page.test.tsx`
Expected: all 4 tests pass.

- [ ] **Step 7: Write `frontend/components/CreateGroupForm.tsx`**

```tsx
'use client';

import { useState, type FormEvent } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Stack, TextInput } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

/** `/groups/new`'s form -- creates a group, then immediately rotates its
 * first invite link (spec §6: "on success, immediately generate the
 * group's first invite link") before navigating to the new group's detail
 * page, where `GroupInviteLinkCard` (Task 12) displays it. Both calls go
 * through the same-origin `/api/*` proxy -- this is a Client Component and
 * can't read the server-only `API_BASE_URL` env var, same reasoning as
 * `TrackTrainForm`/`DeleteTrainButton`.
 *
 * The invite-link call is best-effort: the group itself has already been
 * created by the time it runs, so its failure must never block navigating
 * to the new group -- its own detail page's "Regenerate" control (visible
 * to the owner, Task 12) can create one later if this fails. */
export function CreateGroupForm() {
  const router = useRouter();
  const [name, setName] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch('/api/groups', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      const created: { id: string } = await response.json();
      try {
        await fetch(`/api/groups/${created.id}/invite-link`, { method: 'POST' });
      } catch {
        // Best-effort -- see this component's own doc comment.
      }
      router.push(`/groups/${created.id}`);
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <TextInput
        label="Group name"
        placeholder="e.g. Family or Commute crew"
        value={name}
        onChange={(event) => setName(event.currentTarget.value)}
        maxLength={100}
        required
        data-autofocus
      />
      {error && (
        <Alert color="red" title="Couldn't create this group">
          {error}
        </Alert>
      )}
      <Button type="submit" disabled={name.trim().length === 0 || submitting}>
        {submitting ? 'Creating…' : 'Create group'}
      </Button>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to create a group.
      </LoginPromptModal>
    </Stack>
  );
}
```

- [ ] **Step 8: Write `frontend/components/CreateGroupForm.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CreateGroupForm } from './CreateGroupForm';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/groups/new',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('CreateGroupForm', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('Create group is disabled with an empty name', () => {
    renderWithMantine(<CreateGroupForm />);
    expect(screen.getByRole('button', { name: 'Create group' })).toBeDisabled();
  });

  it('creates the group, rotates its invite link, and navigates to it', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/groups') {
        return Promise.resolve(new Response(JSON.stringify({ id: 'grp-1', name: 'Family' }), { status: 200 }));
      }
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<CreateGroupForm />);
    fireEvent.change(screen.getByLabelText('Group name'), { target: { value: 'Family' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create group' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups/grp-1'));
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/groups',
      expect.objectContaining({ method: 'POST', body: JSON.stringify({ name: 'Family' }) }),
    );
    expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'POST' });
  });

  it('navigates even if the follow-up invite-link call fails', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/groups') {
        return Promise.resolve(new Response(JSON.stringify({ id: 'grp-2', name: 'Crew' }), { status: 200 }));
      }
      return Promise.reject(new Error('network blip'));
    });

    renderWithMantine(<CreateGroupForm />);
    fireEvent.change(screen.getByLabelText('Group name'), { target: { value: 'Crew' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create group' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups/grp-2'));
  });

  it('a 401 on group creation shows a login prompt', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<CreateGroupForm />);
    fireEvent.change(screen.getByLabelText('Group name'), { target: { value: 'Family' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create group' }));

    await screen.findByRole('link', { name: 'Log in to create a group.' });
    expect(pushMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 9: Run the new test**

Run: `cd frontend && npx vitest run components/CreateGroupForm.test.tsx`
Expected: all 4 tests pass.

- [ ] **Step 10: Write `frontend/app/groups/new/page.tsx`**

```tsx
import { Stack, Title } from '@mantine/core';
import { CreateGroupForm } from '@/components/CreateGroupForm';

export default function NewGroupPage() {
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Create a group</Title>
      <CreateGroupForm />
    </Stack>
  );
}
```

- [ ] **Step 11: Run the full frontend test suite and the TypeScript compiler**

Run: `cd frontend && npx tsc --noEmit && npm test`
Expected: no type errors; all tests pass, including the new ones from Steps 5-9.

- [ ] **Step 12: Commit**

```bash
git add frontend/app/layout.tsx frontend/app/groups frontend/components/CreateGroupForm.tsx frontend/components/CreateGroupForm.test.tsx
git commit -m "Add Groups nav item, the groups list page, and the create-group flow"
```

---

## Task 12: Frontend — `/groups/{id}` detail page: members section + shared-trains list

**Files:**
- Create: `frontend/app/groups/[id]/page.tsx`
- Create: `frontend/app/groups/[id]/page.test.tsx`
- Create: `frontend/components/RemoveMemberButton.tsx` + `.test.tsx`
- Create: `frontend/components/PromoteMemberButton.tsx` + `.test.tsx`
- Create: `frontend/components/LeaveGroupButton.tsx` + `.test.tsx`
- Create: `frontend/components/GroupInviteLinkCard.tsx` + `.test.tsx`
- Create: `frontend/components/RemoveGroupTrainButton.tsx` + `.test.tsx`

**Interfaces:**
- Consumes: `getGroup`, `getGroupMembers`, `getGroupTrains`, `getSession`, `ApiNotFoundError` (Task 10 / existing `lib/api.ts`); `trackedTrainDisplayName` (existing `lib/trackingName.ts`); `useNeedsLogin`, `LoginPromptModal` (existing components).
- Produces: five new small Client Components (no exported types needed elsewhere — Task 13 imports none of them, it adds a sibling component instead).

- [ ] **Step 1: Write `RemoveMemberButton.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** `admin`/`owner`-only "Remove" control for one row in the members list,
 * via the same-origin `/api/*` proxy. Mirrors `DeleteTrainButton.tsx`'s
 * confirm-modal shape exactly. The backend's own `remove_member` handler
 * already refuses to target the `owner` row (`403`) regardless of what
 * this button does, but the caller (Task 12's page) additionally never
 * RENDERS this button for the owner's own row at all -- defense in depth,
 * not reliance on the backend alone. */
export function RemoveMemberButton({ groupId, userId, name }: { groupId: string; userId: string; name: string }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleRemove() {
    setRemoving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/members/${userId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setRemoving(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setRemoving(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Remove
      </Button>
      <Modal opened={opened} onClose={close} title={`Remove ${name} from this group?`}>
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to remove this member</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button color="red" onClick={handleRemove} loading={removing} aria-label="Confirm remove member">
            Remove
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

- [ ] **Step 2: Write `RemoveMemberButton.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RemoveMemberButton } from './RemoveMemberButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('RemoveMemberButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('DELETEs the member and refreshes on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<RemoveMemberButton groupId="grp-1" userId="user-2" name="Alex" />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove member' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-2', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('a 403 shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response("the group owner can't be removed", { status: 403 }));

    renderWithMantine(<RemoveMemberButton groupId="grp-1" userId="user-2" name="Alex" />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove member' }));

    await waitFor(() => {
      expect(screen.getByText("the group owner can't be removed")).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 3: Run the new test**

Run: `cd frontend && npx vitest run components/RemoveMemberButton.test.tsx`
Expected: both tests pass.

- [ ] **Step 4: Write `PromoteMemberButton.tsx` + `.test.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Text } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** `owner`-only "Promote to admin" control -- no confirm modal (unlike
 * `RemoveMemberButton`/`DeleteTrainButton`): promoting is non-destructive
 * and reversible in spirit (an owner can always remove an admin they
 * regret promoting), so a bare click is proportionate, matching this
 * app's existing "confirm only genuinely destructive actions" posture. */
export function PromoteMemberButton({ groupId, userId }: { groupId: string; userId: string }) {
  const router = useRouter();
  const [promoting, setPromoting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handlePromote() {
    setPromoting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/members/${userId}/promote`, { method: 'POST' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setPromoting(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Request failed.');
      setPromoting(false);
    }
  }

  return (
    <>
      <Button variant="subtle" size="xs" onClick={handlePromote} loading={promoting}>
        Promote to admin
      </Button>
      {error && <Text c="red">{error}</Text>}
      {needsLoginState.needsLogin && <LoginLink underline="always">Log in to promote this member</LoginLink>}
    </>
  );
}
```

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PromoteMemberButton } from './PromoteMemberButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('PromoteMemberButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('POSTs the promote request and refreshes on success', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ userId: 'user-2', role: 'admin' }), { status: 200 }));

    renderWithMantine(<PromoteMemberButton groupId="grp-1" userId="user-2" />);
    fireEvent.click(screen.getByRole('button', { name: 'Promote to admin' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-2/promote', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('a 409 shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('that member is already an admin or the owner', { status: 409 }));

    renderWithMantine(<PromoteMemberButton groupId="grp-1" userId="user-2" />);
    fireEvent.click(screen.getByRole('button', { name: 'Promote to admin' }));

    await waitFor(() => {
      expect(screen.getByText('that member is already an admin or the owner')).toBeInTheDocument();
    });
  });
});
```

- [ ] **Step 5: Run the new test**

Run: `cd frontend && npx vitest run components/PromoteMemberButton.test.tsx`
Expected: both tests pass.

- [ ] **Step 6: Write `LeaveGroupButton.tsx` + `.test.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Self-removal, always allowed for any member (spec §3) -- unlike
 * `RemoveMemberButton`, this targets the CURRENT user's own id, so it
 * needs no `admin`/`owner` gating at the call site: the page renders this
 * for every member's own row unconditionally. On success, navigates back
 * to `/groups` (there's no reason to stay on a group's own detail page
 * once you've left it) -- same "navigate away from a now-gone-to-you
 * resource" reasoning as `DeleteTrainButton`'s own redirect. */
export function LeaveGroupButton({ groupId, currentUserId }: { groupId: string; currentUserId: string }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [leaving, setLeaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleLeave() {
    setLeaving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/members/${currentUserId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setLeaving(false);
        return;
      }
      router.push('/groups');
    } catch {
      setError('Request failed.');
      setLeaving(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" onClick={open}>
        Leave group
      </Button>
      <Modal opened={opened} onClose={close} title="Leave this group?">
        <Text>
          You&apos;ll lose access to every train shared in this group, and any trains you&apos;ve shared into it
          will be removed for everyone else too.
        </Text>
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to leave this group</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={leaving}>
            Cancel
          </Button>
          <Button color="red" onClick={handleLeave} loading={leaving} aria-label="Confirm leave group">
            Leave
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LeaveGroupButton } from './LeaveGroupButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('LeaveGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('DELETEs the current user as a member and navigates to /groups', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<LeaveGroupButton groupId="grp-1" currentUserId="user-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Leave group' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm leave group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-1', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups'));
  });
});
```

- [ ] **Step 7: Run the new test**

Run: `cd frontend && npx vitest run components/LeaveGroupButton.test.tsx`
Expected: passes.

- [ ] **Step 8: Write `GroupInviteLinkCard.tsx` + `.test.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { ActionIcon, Button, Card, Group, Stack, Text, TextInput, Tooltip } from '@mantine/core';
import type { GroupInviteLink } from '@/lib/types';

/** Copy-to-clipboard / Web Share affordance for a group's invite link,
 * adapted from `ShareButton.tsx`'s own pattern (feature-detect
 * `navigator.share`, fall back to the clipboard, flip to "Copied!" for a
 * couple of seconds) -- parameterized by an explicit `url` rather than
 * `window.location.href`, since this shares a DIFFERENT page (the join
 * page) than the one it's rendered on. `admin`/`owner`-only: the caller
 * (Task 12's page) never renders this for a plain `member` at all,
 * matching `inviteLink` being `null` in that case on the wire already. */
const COPIED_LABEL = 'Copied!';
const COPIED_TIMEOUT_MS = 2000;

export function GroupInviteLinkCard({ groupId, inviteLink }: { groupId: string; inviteLink: GroupInviteLink | null }) {
  const router = useRouter();
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const url = inviteLink ? `${window.location.origin}/groups/join/${inviteLink.token}` : null;

  async function share() {
    if (!url) return;
    if (typeof navigator.share === 'function') {
      try {
        await navigator.share({ url, title: 'Join my group on Distant Signal' });
        return;
      } catch (err) {
        if (err && typeof err === 'object' && 'name' in err && err.name === 'AbortError') return;
      }
    }
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
      setTimeout(() => setCopied(false), COPIED_TIMEOUT_MS);
    } catch {
      // No more to do -- see ShareButton.tsx's own identical fallback.
    }
  }

  async function regenerate() {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/groups/${groupId}/invite-link`, { method: 'POST' });
      if (!response.ok) {
        setError('Could not create a new invite link.');
        setBusy(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Could not create a new invite link.');
      setBusy(false);
    }
  }

  async function revoke() {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/groups/${groupId}/invite-link`, { method: 'DELETE' });
      if (!response.ok) {
        setError('Could not revoke the invite link.');
        setBusy(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Could not revoke the invite link.');
      setBusy(false);
    }
  }

  return (
    <Card withBorder>
      <Stack gap="xs">
        <Text fw={500}>Invite link</Text>
        {url ? (
          <Group gap="xs" wrap="nowrap">
            <TextInput value={url} readOnly style={{ flexGrow: 1 }} />
            <Tooltip label={copied ? COPIED_LABEL : 'Share this link'}>
              <ActionIcon variant="outline" color="gray" onClick={share} aria-label="Share invite link">
                {copied ? '✓' : '⇪'}
              </ActionIcon>
            </Tooltip>
          </Group>
        ) : (
          <Text size="sm" c="dimmed">
            No active invite link.
          </Text>
        )}
        {error && <Text c="red">{error}</Text>}
        <Group gap="xs">
          <Button variant="default" size="xs" onClick={regenerate} loading={busy}>
            Regenerate
          </Button>
          {url && (
            <Button variant="outline" color="red" size="xs" onClick={revoke} loading={busy}>
              Revoke
            </Button>
          )}
        </Group>
      </Stack>
    </Card>
  );
}
```

- [ ] **Step 9: Write `GroupInviteLinkCard.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { GroupInviteLinkCard } from './GroupInviteLinkCard';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('GroupInviteLinkCard', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows "No active invite link" when there is none', () => {
    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={null} />);
    expect(screen.getByText('No active invite link.')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Revoke' })).not.toBeInTheDocument();
  });

  it('renders the full join URL built from the token', () => {
    renderWithMantine(
      <GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />,
    );
    expect(screen.getByDisplayValue(/\/groups\/join\/tok123$/)).toBeInTheDocument();
  });

  it('Regenerate POSTs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ token: 'new', expiresAt: '2026-09-19T00:00:00Z' }), { status: 200 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />);
    fireEvent.click(screen.getByRole('button', { name: 'Regenerate' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'POST' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('Revoke DELETEs and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<GroupInviteLinkCard groupId="grp-1" inviteLink={{ token: 'tok123', expiresAt: '2026-09-18T00:00:00Z' }} />);
    fireEvent.click(screen.getByRole('button', { name: 'Revoke' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });
});
```

- [ ] **Step 10: Run the new test**

Run: `cd frontend && npx vitest run components/GroupInviteLinkCard.test.tsx`
Expected: all 4 tests pass.

- [ ] **Step 11: Write `RemoveGroupTrainButton.tsx` + `.test.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Removes a shared train from a group -- the sharer, or any `admin`/
 * `owner`, may click this (the backend enforces which; the page renders
 * this for every row regardless, since a plain member CAN remove their
 * own shared train). Mirrors `DeleteTrainButton.tsx`'s confirm-modal shape. */
export function RemoveGroupTrainButton({ groupId, trainSubscriptionId }: { groupId: string; trainSubscriptionId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleRemove() {
    setRemoving(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/trains/${trainSubscriptionId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setRemoving(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setRemoving(false);
    }
  }

  return (
    <>
      <Button variant="subtle" color="red" size="xs" onClick={open}>
        Remove from group
      </Button>
      <Modal opened={opened} onClose={close} title="Remove this train from the group?">
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to remove this train</LoginLink>}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={removing}>
            Cancel
          </Button>
          <Button color="red" onClick={handleRemove} loading={removing} aria-label="Confirm remove train from group">
            Remove
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RemoveGroupTrainButton } from './RemoveGroupTrainButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('RemoveGroupTrainButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('DELETEs the shared train and refreshes on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<RemoveGroupTrainButton groupId="grp-1" trainSubscriptionId={42} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove from group' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove train from group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/trains/42', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });
});
```

- [ ] **Step 12: Run the new test**

Run: `cd frontend && npx vitest run components/RemoveGroupTrainButton.test.tsx`
Expected: passes.

- [ ] **Step 13: Write `frontend/app/groups/[id]/page.tsx`**

```tsx
import { Badge, Card, Divider, Group, Stack, Text, Title } from '@mantine/core';
import { getGroup, getGroupMembers, getGroupTrains, getSession, ApiNotFoundError } from '@/lib/api';
import { RemoveMemberButton } from '@/components/RemoveMemberButton';
import { PromoteMemberButton } from '@/components/PromoteMemberButton';
import { LeaveGroupButton } from '@/components/LeaveGroupButton';
import { GroupInviteLinkCard } from '@/components/GroupInviteLinkCard';
import { RemoveGroupTrainButton } from '@/components/RemoveGroupTrainButton';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import type { GroupMember, GroupTrain } from '@/lib/types';

export const revalidate = 0;

/** `/groups/{id}` -- group detail: members (with role badges, remove/
 * promote/leave/invite-link) and the trains shared into the group. The
 * "Add one of my trains" picker is added on top of this page in Task 13;
 * this task ships the full read + remove/promote/leave surface on its own. */
export default async function GroupDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;

  let group;
  try {
    [group] = await Promise.all([getGroup(id)]);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Group not found</Title>
          <Text c="dimmed">This group doesn&apos;t exist, or you&apos;re not a member of it.</Text>
        </Stack>
      );
    }
    throw err;
  }

  const [members, trains, session] = await Promise.all([
    getGroupMembers(id),
    getGroupTrains(id),
    getSession().catch(() => ({ authenticated: false, id: null, email: null, name: null })),
  ]);
  const currentUserId = session.authenticated ? session.id : null;
  const canManage = group.role === 'owner' || group.role === 'admin';

  return (
    <Stack p="lg" gap="lg">
      <Group justify="space-between" align="baseline">
        <Title order={1}>{group.name}</Title>
        {currentUserId && <LeaveGroupButton groupId={id} currentUserId={currentUserId} />}
      </Group>

      <Stack gap="sm">
        <Title order={2}>Members</Title>
        {members.map((member) => (
          <MemberRow key={member.userId} groupId={id} member={member} canManage={canManage} />
        ))}
        {canManage && <GroupInviteLinkCard groupId={id} inviteLink={group.inviteLink} />}
      </Stack>

      <Divider />

      <Stack gap="sm">
        <Title order={2}>Shared trains</Title>
        {trains.length === 0 ? (
          <Text c="dimmed">No trains have been shared into this group yet.</Text>
        ) : (
          trains.map((train) => <SharedTrainRow key={train.trainSubscriptionId} groupId={id} train={train} />)
        )}
      </Stack>
    </Stack>
  );
}

function MemberRow({ groupId, member, canManage }: { groupId: string; member: GroupMember; canManage: boolean }) {
  const label = member.name ?? member.email ?? 'A member';
  const isOwner = member.role === 'owner';
  return (
    <Group justify="space-between" wrap="nowrap">
      <Group gap="xs">
        <Text>{label}</Text>
        <Badge variant="outline">{member.role}</Badge>
      </Group>
      <Group gap="xs">
        {canManage && member.role === 'member' && <PromoteMemberButton groupId={groupId} userId={member.userId} />}
        {canManage && !isOwner && <RemoveMemberButton groupId={groupId} userId={member.userId} name={label} />}
      </Group>
    </Group>
  );
}

function SharedTrainRow({ groupId, train }: { groupId: string; train: GroupTrain }) {
  const displayName = trackedTrainDisplayName({
    customName: train.customName,
    pinOriginCrs: train.pinOriginCrs,
    pinOriginName: train.pinOriginName,
    pinDestinationCrs: train.pinDestinationCrs,
    pinDestinationName: train.pinDestinationName,
    serviceDate: train.serviceDate,
    pinScheduledDeparture: train.pinScheduledDeparture,
  });
  return (
    <Card withBorder>
      <Group justify="space-between" wrap="nowrap">
        <Stack gap={4}>
          <Text fw={500}>{displayName}</Text>
          <Text size="sm" c="dimmed">
            Shared by {train.addedByName ?? 'a member'}
            {train.status && ` · ${train.status}`}
            {train.delayMinutes !== null && train.delayMinutes > 0 && ` · ${train.delayMinutes}m late`}
          </Text>
        </Stack>
        <RemoveGroupTrainButton groupId={groupId} trainSubscriptionId={train.trainSubscriptionId} />
      </Group>
    </Card>
  );
}
```

- [ ] **Step 14: Write `frontend/app/groups/[id]/page.test.tsx`**

```tsx
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import GroupDetailPage from './page';
import { getGroup, getGroupMembers, getGroupTrains, getSession, ApiNotFoundError } from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getGroup: vi.fn(),
    getGroupMembers: vi.fn(),
    getGroupTrains: vi.fn(),
    getSession: vi.fn(),
  };
});

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn(), push: vi.fn() }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('GroupDetailPage', () => {
  it('shows a not-found message on ApiNotFoundError', async () => {
    vi.mocked(getGroup).mockRejectedValue(new ApiNotFoundError('404'));
    render(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-missing' }) }));
    expect(await screen.findByText('Group not found')).toBeInTheDocument();
  });

  it('renders the group name, members, and shared trains', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: 'Alex',
      memberCount: 2,
      role: 'owner',
      inviteLink: { token: 'tok', expiresAt: '2026-09-18T00:00:00Z' },
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', name: 'Alex', email: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
      { userId: 'user-2', name: 'Sam', email: null, role: 'member', joinedAt: '2026-09-02T00:00:00Z' },
    ]);
    vi.mocked(getGroupTrains).mockResolvedValue([
      {
        trainSubscriptionId: 42,
        pinOriginCrs: 'WOK',
        pinDestinationCrs: 'WAT',
        pinOriginName: 'Woking',
        pinDestinationName: 'London Waterloo',
        pinScheduledDeparture: '2026-09-11T08:00:00Z',
        serviceDate: '2026-09-11',
        resolutionStatus: 'resolved',
        trainUid: 'A12345',
        status: 'en_route',
        delayMinutes: 5,
        customName: null,
        addedBy: 'user-2',
        addedByName: 'Sam',
      },
    ]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    render(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));

    expect(screen.getByRole('heading', { name: 'Family' })).toBeInTheDocument();
    expect(screen.getByText('Alex')).toBeInTheDocument();
    expect(screen.getByText('Sam')).toBeInTheDocument();
    expect(screen.getByText(/Shared by Sam/)).toBeInTheDocument();
  });

  it('never renders a Remove button for the owner row', async () => {
    vi.mocked(getGroup).mockResolvedValue({
      id: 'grp-1',
      name: 'Family',
      ownerId: 'user-1',
      ownerName: 'Alex',
      memberCount: 1,
      role: 'owner',
      inviteLink: null,
    });
    vi.mocked(getGroupMembers).mockResolvedValue([
      { userId: 'user-1', name: 'Alex', email: null, role: 'owner', joinedAt: '2026-09-01T00:00:00Z' },
    ]);
    vi.mocked(getGroupTrains).mockResolvedValue([]);
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    render(await GroupDetailPage({ params: Promise.resolve({ id: 'grp-1' }) }));
    expect(screen.queryByRole('button', { name: 'Remove' })).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 15: Run the new test and the full frontend suite**

Run: `cd frontend && npx vitest run app/groups/'[id]'/page.test.tsx`
Expected: all 3 tests pass.

Run: `cd frontend && npx tsc --noEmit && npm test`
Expected: no type errors; every test (old and new) passes.

- [ ] **Step 16: Commit**

```bash
git add frontend/app/groups/[id] frontend/components/RemoveMemberButton.tsx frontend/components/RemoveMemberButton.test.tsx frontend/components/PromoteMemberButton.tsx frontend/components/PromoteMemberButton.test.tsx frontend/components/LeaveGroupButton.tsx frontend/components/LeaveGroupButton.test.tsx frontend/components/GroupInviteLinkCard.tsx frontend/components/GroupInviteLinkCard.test.tsx frontend/components/RemoveGroupTrainButton.tsx frontend/components/RemoveGroupTrainButton.test.tsx
git commit -m "Add the group detail page: members (remove/promote/leave/invite-link) and shared trains (list + remove)"
```

---

## Task 13: Frontend — "Add one of my trains" picker

**Files:**
- Create: `frontend/components/AddTrainToGroupButton.tsx`
- Create: `frontend/components/AddTrainToGroupButton.test.tsx`
- Modify: `frontend/app/groups/[id]/page.tsx` (render the new button in the "Shared trains" section header)

**Interfaces:**
- Consumes: existing `GET /api/Train/mine` proxy route (`TrackedTrainListItem[]`, already wired — see `frontend/lib/api.ts`'s `getMyTrackedTrains`/the proxy's `resolveTargetPath`'s `Train/...`-bare-passthrough rule); `trackedTrainDisplayName` (existing `lib/trackingName.ts`).
- Produces: `AddTrainToGroupButton` (Client Component, props `{ groupId: string; excludeTrainSubscriptionIds: number[] }`).

- [ ] **Step 1: Write `AddTrainToGroupButton.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Modal, Select, Text } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { trackedTrainDisplayName } from '@/lib/trackingName';
import type { TrackedTrainListItem } from '@/lib/types';

/** Picker sourced from the user's own `/track/mine` list (spec §6),
 * fetched lazily on open via the same-origin `/api/Train/mine` proxy
 * (already wired -- `Train/...` requests pass through bare, no
 * `/public/` prefix, per `app/api/[...path]/route.ts`'s own
 * `resolveTargetPath`). `excludeTrainSubscriptionIds` hides trains
 * already shared into this group -- re-adding one is harmless
 * (`groups::add_train_to_group` is idempotent) but offering it again in
 * the picker would be confusing. */
export function AddTrainToGroupButton({
  groupId,
  excludeTrainSubscriptionIds,
}: {
  groupId: string;
  excludeTrainSubscriptionIds: number[];
}) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [loading, setLoading] = useState(false);
  const [trains, setTrains] = useState<TrackedTrainListItem[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleOpen() {
    setError(null);
    setSelected(null);
    open();
    setLoading(true);
    try {
      const response = await fetch('/api/Train/mine');
      if (!response.ok) {
        setError('Could not load your tracked trains.');
        setLoading(false);
        return;
      }
      const all: TrackedTrainListItem[] = await response.json();
      setTrains(all.filter((t) => !excludeTrainSubscriptionIds.includes(t.id)));
      setLoading(false);
    } catch {
      setError('Could not load your tracked trains.');
      setLoading(false);
    }
  }

  async function handleAdd() {
    if (!selected) return;
    setSubmitting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/${groupId}/trains`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ trainSubscriptionId: Number(selected) }),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      close();
      router.refresh();
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <>
      <Button variant="default" size="xs" onClick={handleOpen}>
        Add one of my trains
      </Button>
      <Modal opened={opened} onClose={close} title="Share a tracked train with this group">
        {loading && <Text c="dimmed">Loading your tracked trains…</Text>}
        {!loading && trains !== null && trains.length === 0 && (
          <Text c="dimmed">Every train you&apos;re tracking is already shared into this group.</Text>
        )}
        {!loading && trains !== null && trains.length > 0 && (
          <Select
            label="Tracked train"
            placeholder="Pick one"
            data={trains.map((t) => ({ value: String(t.id), label: trackedTrainDisplayName(t) }))}
            value={selected}
            onChange={setSelected}
          />
        )}
        {error && <Alert color="red">{error}</Alert>}
        {needsLoginState.needsLogin && <LoginLink underline="always">Log in to share a train</LoginLink>}
        <Button mt="md" onClick={handleAdd} disabled={!selected} loading={submitting}>
          Add to group
        </Button>
      </Modal>
    </>
  );
}
```

- [ ] **Step 2: Write `AddTrainToGroupButton.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AddTrainToGroupButton } from './AddTrainToGroupButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

function mineResponse() {
  return new Response(
    JSON.stringify([
      {
        id: 1,
        serviceDate: '2026-09-11',
        pinOriginCrs: 'WOK',
        pinDestinationCrs: 'WAT',
        pinOriginName: 'Woking',
        pinDestinationName: 'London Waterloo',
        pinScheduledDeparture: '2026-09-11T08:00:00Z',
        resolutionStatus: 'resolved',
        trainUid: 'A12345',
        status: 'en_route',
        delayMinutes: 0,
        trackedAt: '2026-09-10T00:00:00Z',
        customName: null,
      },
      {
        id: 2,
        serviceDate: '2026-09-11',
        pinOriginCrs: 'CLJ',
        pinDestinationCrs: 'VIC',
        pinOriginName: 'Clapham Junction',
        pinDestinationName: 'London Victoria',
        pinScheduledDeparture: '2026-09-11T09:00:00Z',
        resolutionStatus: 'resolved',
        trainUid: 'B67890',
        status: 'en_route',
        delayMinutes: 0,
        trackedAt: '2026-09-10T00:00:00Z',
        customName: null,
      },
    ]),
    { status: 200 },
  );
}

describe('AddTrainToGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('fetches /api/Train/mine on open and excludes already-shared trains', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(mineResponse());

    renderWithMantine(<AddTrainToGroupButton groupId="grp-1" excludeTrainSubscriptionIds={[2]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add one of my trains' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalledWith('/api/Train/mine'));
    const select = await screen.findByLabelText('Tracked train');
    fireEvent.click(select);
    expect(within(screen.getByRole('dialog')).getByText(/Woking/)).toBeInTheDocument();
    expect(within(screen.getByRole('dialog')).queryByText(/Clapham Junction/)).not.toBeInTheDocument();
  });

  it('POSTs the chosen trainSubscriptionId and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/Train/mine') return Promise.resolve(mineResponse());
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<AddTrainToGroupButton groupId="grp-1" excludeTrainSubscriptionIds={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Add one of my trains' }));
    const select = await screen.findByLabelText('Tracked train');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText(/Woking/));
    fireEvent.click(screen.getByRole('button', { name: 'Add to group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/groups/grp-1/trains',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trainSubscriptionId: 1 }) }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });
});
```

- [ ] **Step 3: Run the new test**

Run: `cd frontend && npx vitest run components/AddTrainToGroupButton.test.tsx`
Expected: both tests pass.

- [ ] **Step 4: Wire the button into `frontend/app/groups/[id]/page.tsx`**

Add the import:

```tsx
import { AddTrainToGroupButton } from '@/components/AddTrainToGroupButton';
```

Change the "Shared trains" section header from:

```tsx
      <Stack gap="sm">
        <Title order={2}>Shared trains</Title>
        {trains.length === 0 ? (
```

to:

```tsx
      <Stack gap="sm">
        <Group justify="space-between" align="baseline">
          <Title order={2}>Shared trains</Title>
          <AddTrainToGroupButton
            groupId={id}
            excludeTrainSubscriptionIds={trains.map((t) => t.trainSubscriptionId)}
          />
        </Group>
        {trains.length === 0 ? (
```

- [ ] **Step 5: Run the full frontend suite and the TypeScript compiler**

Run: `cd frontend && npx tsc --noEmit && npm test`
Expected: no type errors; all tests (old and new) pass.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/AddTrainToGroupButton.tsx frontend/components/AddTrainToGroupButton.test.tsx frontend/app/groups/[id]/page.tsx
git commit -m "Add the 'Add one of my trains' picker to the group detail page"
```

---

## Task 14: Frontend — `/groups/join/{token}` confirm-and-join page

**Files:**
- Create: `frontend/components/JoinGroupButton.tsx`
- Create: `frontend/components/JoinGroupButton.test.tsx`
- Create: `frontend/app/groups/join/[token]/page.tsx`
- Create: `frontend/app/groups/join/[token]/page.test.tsx`

**Interfaces:**
- Consumes: `getGroupJoinPreview`, `getSession`, `ApiNotFoundError` (Task 10 / existing `lib/api.ts`); `LoginLink` (existing, captures `return_to` automatically via `useLoginHref`'s `usePathname()`/`useSearchParams()` — no extra plumbing needed for the OIDC-redirect-preserves-the-token requirement, since the confirm page's own URL IS `/groups/join/{token}`).
- Produces: `JoinGroupButton` (Client Component, props `{ token: string; groupId: string }`).

- [ ] **Step 1: Write `JoinGroupButton.tsx`**

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** The confirm page's explicit "Join" action (spec §2.3: "Confirm-before-
 * join, never silent auto-join" -- this component is that explicit
 * action). Handles a 401 the same way every other mutating control in
 * this app does, even though the page only renders this for an already-
 * authenticated visitor -- a session can still lapse between page load
 * and this click, the same narrow race `DeleteTrainButton`'s own doc
 * comment already names. */
export function JoinGroupButton({ token, groupId }: { token: string; groupId: string }) {
  const router = useRouter();
  const [joining, setJoining] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleJoin() {
    setJoining(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/groups/join/${token}`, { method: 'POST' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setJoining(false);
        return;
      }
      router.push(`/groups/${groupId}`);
    } catch {
      setError('Request failed.');
      setJoining(false);
    }
  }

  return (
    <>
      {error && <Alert color="red">{error}</Alert>}
      {needsLoginState.needsLogin ? (
        <LoginLink underline="always">Log in to join this group</LoginLink>
      ) : (
        <Button onClick={handleJoin} loading={joining}>
          Join group
        </Button>
      )}
    </>
  );
}
```

- [ ] **Step 2: Write `JoinGroupButton.test.tsx`**

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { JoinGroupButton } from './JoinGroupButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/groups/join/tok123',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('JoinGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('POSTs the join and navigates to the group', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ groupId: 'grp-1' }), { status: 200 }));

    renderWithMantine(<JoinGroupButton token="tok123" groupId="grp-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Join group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/join/tok123', { method: 'POST' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups/grp-1'));
  });

  it('a 401 shows a login prompt instead of joining', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<JoinGroupButton token="tok123" groupId="grp-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Join group' }));

    await screen.findByRole('link', { name: 'Log in to join this group' });
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('an expired/revoked token (404) shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('this invite link is invalid or has expired', { status: 404 }));

    renderWithMantine(<JoinGroupButton token="tok123" groupId="grp-1" />);
    fireEvent.click(screen.getByRole('button', { name: 'Join group' }));

    await waitFor(() => {
      expect(screen.getByText('this invite link is invalid or has expired')).toBeInTheDocument();
    });
    expect(pushMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 3: Run the new test**

Run: `cd frontend && npx vitest run components/JoinGroupButton.test.tsx`
Expected: all 3 tests pass.

- [ ] **Step 4: Write `frontend/app/groups/join/[token]/page.tsx`**

```tsx
import { Alert, Stack, Text, Title } from '@mantine/core';
import { getGroupJoinPreview, getSession, ApiNotFoundError } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { JoinGroupButton } from '@/components/JoinGroupButton';

export const revalidate = 0;

/** `/groups/join/{token}` -- confirm-before-join (spec §2.3): resolves the
 * token to a group preview (works whether or not the visitor is logged in
 * -- `getGroupJoinPreview` hits the backend's unauthenticated preview
 * route), then either shows the explicit Join button (already
 * authenticated) or a login link. `LoginLink` needs no extra plumbing to
 * preserve this token through the OIDC redirect: it captures the CURRENT
 * page's own path via `usePathname()` (`useLoginHref.ts`), and that path
 * already IS `/groups/join/{token}` -- logging in and landing back here
 * re-renders this exact page, now authenticated, ready for the same
 * explicit Join click (mirroring `validate_return_to`'s existing
 * return-to-any-same-origin-path mechanism, `crates/api/src/auth.rs`). */
export default async function JoinGroupPage({ params }: { params: Promise<{ token: string }> }) {
  const { token } = await params;

  let preview;
  try {
    preview = await getGroupJoinPreview(token);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Invite link not found</Title>
          <Alert color="red">This invite link is invalid or has expired. Ask the group for a new one.</Alert>
        </Stack>
      );
    }
    throw err;
  }

  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Join {preview.groupName}?</Title>
      <Text>
        {preview.memberCount} member{preview.memberCount === 1 ? '' : 's'} already in this group. Joining lets
        everyone in {preview.groupName} see any tracked train you choose to share into it — your other tracked
        trains and tickets stay private.
      </Text>
      {session.authenticated ? (
        <JoinGroupButton token={token} groupId={preview.groupId} />
      ) : (
        <LoginLink underline="always">Log in to join {preview.groupName}</LoginLink>
      )}
    </Stack>
  );
}
```

- [ ] **Step 5: Write `frontend/app/groups/join/[token]/page.test.tsx`**

```tsx
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import JoinGroupPage from './page';
import { getGroupJoinPreview, getSession, ApiNotFoundError } from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getGroupJoinPreview: vi.fn(),
    getSession: vi.fn(),
  };
});

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/groups/join/tok123',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('JoinGroupPage', () => {
  it('shows an invalid-link message on ApiNotFoundError', async () => {
    vi.mocked(getGroupJoinPreview).mockRejectedValue(new ApiNotFoundError('404'));
    render(await JoinGroupPage({ params: Promise.resolve({ token: 'bad-token' }) }));
    expect(await screen.findByText('Invite link not found')).toBeInTheDocument();
  });

  it('shows a login link when the visitor is not authenticated', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 3 });
    vi.mocked(getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });

    render(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
    expect(screen.getByRole('heading', { name: 'Join Family?' })).toBeInTheDocument();
    expect(await screen.findByRole('link', { name: 'Log in to join Family' })).toBeInTheDocument();
  });

  it('shows the explicit Join button when already authenticated', async () => {
    vi.mocked(getGroupJoinPreview).mockResolvedValue({ groupId: 'grp-1', groupName: 'Family', memberCount: 1 });
    vi.mocked(getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: 'Alex' });

    render(await JoinGroupPage({ params: Promise.resolve({ token: 'tok123' }) }));
    expect(screen.getByRole('button', { name: 'Join group' })).toBeInTheDocument();
    expect(screen.getByText('1 member already in this group.', { exact: false })).toBeInTheDocument();
  });
});
```

- [ ] **Step 6: Run the new test, the full frontend suite, and the TypeScript compiler**

Run: `cd frontend && npx vitest run app/groups/join/'[token]'/page.test.tsx`
Expected: all 3 tests pass.

Run: `cd frontend && npx tsc --noEmit && npm test`
Expected: no type errors; every test in the whole frontend suite passes. This is the last frontend task — every page/component in the spec's §6 frontend surface now exists.

- [ ] **Step 7: Run the frontend production build to confirm no `next build`-time regressions (e.g. the Server/Client boundary issues this plan's own comments call out)**

Run: `cd frontend && npm run build`
Expected: build succeeds.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/JoinGroupButton.tsx frontend/components/JoinGroupButton.test.tsx frontend/app/groups/join
git commit -m "Add the confirm-before-join page and its explicit Join action"
```

---

## Self-Review

**1. Spec coverage.**

- §2.1 `groups`/`group_members`, permanent owner inserted in the same transaction, three-tier role, owner-never-removable/demotable, last-owner-transfer, owner-alone-deletes-group → Task 1 (schema), Task 2 (`create_group`), Task 3 (`remove_member`'s full transfer/deletion logic + its 5 dedicated db_tests), Task 7 (route-level owner protection).
- §2.2 `group_trains` join table, `ON DELETE CASCADE`, app-layer ownership check, departed-member cleanup → Task 1 (schema), Task 5 (`add_train_to_group`'s ownership check), Task 3 (`remove_member`'s departed-cleanup, tested explicitly).
- §2.3 `group_invite_links`, reusable/rotatable, 7-day default expiry, rotate-revokes-old, confirm-before-join → Task 1 (schema), Task 4 (all invite-link data functions + expiry/revocation db_tests), Task 8 (routes), Task 14 (the confirm page itself, never joining on `GET`).
- §3 permissions table → every action in the table maps onto a specific handler: create (Task 6 `create_group`, any authenticated user), view (Task 6/7/9, membership-gated), add own train (Task 9 `add_group_train` + Task 5's ownership check), remove a train (Task 9 `remove_group_train` + Task 5's sharer-or-manager check), invite management (Task 8, `can_manage`-gated), remove another member (Task 7, `can_manage`-gated, owner-protected), promote (Task 7, `is_owner`-gated), leave (Task 7's self-removal branch, always allowed), rename (Task 6, `can_manage`-gated), delete group (Task 6, `is_owner`-gated).
- §4 what a member sees → Task 5's `GroupTrain`/`list_group_trains` (computed-default-name-ready fields, attribution via `added_by`/`added_by_name`, no ticket/notification/exact-timestamp fields, pinned by a dedicated wire-shape test) and Task 12's `SharedTrainRow` (renders `trackedTrainDisplayName` + "Shared by {name}").
- §5 API surface → all 15 method+path rows implemented across Tasks 6-9; confirmed against the literal spec table during route-wiring (Tasks 6, 8, 9).
- §6 frontend surface → nav item (Task 11), `/groups` (Task 11), `/groups/new` (Task 11), `/groups/{id}` members+invite-link+leave (Task 12), `/groups/{id}` shared trains+remove (Task 12), "Add one of my trains" (Task 13), `/groups/join/{token}` (Task 14).
- §7 alternatives considered — no action needed; these are rejected alternatives, not requirements.
- §8 non-goals — confirmed nothing in this plan adds a user-directory/lookup feature, per-group ticket-sharing config, member/group caps, or changes to the existing public `{uid}/{date}` page/`ShareButton`.

**2. Placeholder scan.** Searched the plan for "TBD", "similar to Task", "add appropriate", and bare prose-only steps — none found. Every code step above contains complete, concrete Rust/TypeScript/SQL, not a description of what to write. One placeholder-shaped risk was caught and fixed during drafting: Task 4/6 originally left "how does the frontend see the current invite link on a normal page load" unaddressed, since the spec's API table lists no `GET`-the-current-link route. Fixed by explicitly extending `GET /groups/{id}`'s response with `inviteLink` (Task 2's `GroupDetail`, Task 4's `get_active_invite_link`, Task 6's `get_group` handler) and calling out the extension in both the data-layer doc comment and this self-review, rather than silently inventing an unlisted route or leaving the frontend with no way to redisplay the link.

**3. Type/signature consistency.**

- `GroupRole` (`Owner`/`Admin`/`Member`, `can_manage`/`is_owner`) defined once in Task 2, reused verbatim by name in every later backend task (Tasks 3, 5, 6, 7, 8, 9) — no renaming or re-derivation anywhere.
- `groups::GroupSummary`/`GroupDetail`/`GroupMember`/`GroupTrain`/`InviteLink`/`JoinPreview` field names are used identically between their Task 2/3/4/5 definitions and their Task 6/7/8/9 route call sites (e.g. `detail.role.can_manage()`, `member.role`, `train.added_by_name`).
- Every backend camelCase wire field has a matching frontend `lib/types.ts` field in Task 10 (`memberCount`, `ownerId`, `ownerName`, `inviteLink`, `joinedAt`, `trainSubscriptionId`, `pinOriginCrs`, `addedByName`, etc.) — cross-checked line by line against Tasks 2-5's Rust struct definitions while writing Task 10.
- Route paths used by every frontend `fetch()` call (Tasks 11-14: `/api/groups`, `/api/groups/{id}/invite-link`, `/api/groups/{id}/members/{userId}`, `/api/groups/{id}/members/{userId}/promote`, `/api/groups/{id}/trains`, `/api/groups/{id}/trains/{trainSubscriptionId}`, `/api/groups/join/{token}`) match exactly the paths `router()` registers across Tasks 6-9 (the proxy's default `/public/` prefix applies transparently, per Task 6's module doc comment).
- `remove_member`'s `RemoveMemberOutcome` variants (`NotAMember`/`Removed`/`GroupDeleted`) are matched exhaustively in Task 7's `remove_member` handler with no `_ =>` catch-all, so a future added variant would fail to compile there rather than silently falling through.
