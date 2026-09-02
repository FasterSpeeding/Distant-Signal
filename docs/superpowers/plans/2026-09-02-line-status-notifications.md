# Line-Status Notifications Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Web Push notifications for (a) a pinned line's severity-tier change and (b) a tracked train's `status`/`delay_minutes` transition, delivered via a new poll-based `notifier` binary that watermark-polls `line_status_history` and `train_movement_events`, reusing `pinned_lines`/`tracked_trains` as subscription scope with no new preferences table, and a single global "Enable notifications" Tier-2 control on the home page.

**Architecture:** A new `crates/notifier` binary (mirroring `aggregator`'s poll-loop shape) polls two tables by a persisted `id` watermark, computes severity-tier/status transitions via pure, DB-free decision functions, joins the surviving candidates against `pinned_lines`/`tracked_trains` and a new `push_subscriptions` table, and sends via the `web-push` crate (VAPID, no `reqwest` dependency, sidestepping `crates/api`'s pinned `reqwest 0.12`). A new `crates/api/src/routes/notifications.rs` exposes the two endpoints the frontend needs to create a subscription (`GET /public/notifications/vapid-public-key`, `POST /public/notifications/subscribe`). A new `NotificationsToggle` component on the home page performs the browser-side `PushManager.subscribe()` flow, following `PinToggle.tsx`'s established Tier-2 `useNeedsLogin()`/`LoginPromptModal` shape exactly. One task in this plan (Task 9) adds the `push`/`notificationclick` handlers this feature needs to whatever service worker file the sibling PWA-service-worker effort produces — see **Global Constraints** below, this is a hard, unresolved cross-plan dependency, not a soft one.

**Tech Stack:** Rust/Axum/sqlx (existing workspace conventions), a new `web-push` crate dependency (`pimeys/rust-web-push`, currently `0.11.0` on crates.io, confirmed by a fresh search this session — default HTTP client `isahc`, `hyper-client` feature swaps to a `hyper`-based client, no `reqwest` dependency either way; requires OpenSSL at build time regardless of HTTP-client feature). Next.js App Router + TypeScript + Vitest (existing frontend conventions, no new npm dependency — `PushManager`/`Notification` are browser-native Web APIs). Helm chart + Docker, mirroring `aggregator`'s no-HTTP-surface, `replicas: 1` deployment shape.

**Spec:** `docs/superpowers/specs/2026-09-02-line-status-notifications-design.md` — read in full before starting; this plan carries its Decisions into concrete tasks and does not restate its research. Cross-references below to "Decision N" refer to that document.

**Status note — every citation below independently re-confirmed against this worktree's current source, not trusted blind from the spec (this worktree's branch was merged forward to main tip `50dd6f8` before this plan was written, specifically so these citations are accurate as of today, 2026-09-02):**

- `crates/common/src/lib.rs:108-131`'s `severity_rank` and `:362-370`'s `LineStatusReport::worst_severity()` — re-confirmed: `worst_severity()` uses raw `Severity` `.min()` (line 368, `.map(|s| s.severity).min()`), **not** `severity_rank`. The spec's central finding stands unchanged. `severity_rank_tests` at lines 134-181 already encodes the exact `Diverted`/`PartClosed`-vs-`MinorDelays` discrepancy the spec describes.
- `crates/aggregator/src/queries.rs:260-311`'s `write_line_status` and `crates/api/src/data/queries.rs:333-382`'s `upsert_tfl_line_status` — re-confirmed as two structurally-identical, textually-separate `line_status_history` writers, each with its own `changed`/`tfl_statuses_changed` diff-and-insert. `line_status_history.statuses` is a JSONB `serde_json::to_value(&report.statuses)` of `Vec<common::LineStatus>` in both writers (`queries.rs:261`, `data/queries.rs:342` — same shape, `serde_json::from_value::<Vec<common::LineStatus>>` round-trips it directly), which is what Task 3 below deserializes to compute a row's worst `severity_rank`.
- `crates/api/src/data/preferences.rs` (read in full): `list_pinned_line_ids` (lines 7-13), `replace_pinned_lines` (lines 41-53+). Schema confirmed unchanged from the spec's citation: `pinned_lines(user_id TEXT, line_id TEXT, pinned_at TIMESTAMPTZ, PRIMARY KEY (user_id, line_id))`.
- `crates/api/migrations/20260828120000_train_tracking.sql` (read in full): `tracked_trains(id BIGSERIAL PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id), ...)`, `train_movement_events(id BIGSERIAL PRIMARY KEY, tracked_train_id BIGINT NOT NULL REFERENCES tracked_trains(id) ON DELETE CASCADE, ...)`, `train_current_state(tracked_train_id BIGINT PRIMARY KEY REFERENCES tracked_trains(id) ON DELETE CASCADE, status TEXT NOT NULL DEFAULT 'awaiting_activation' CHECK (status IN ('awaiting_activation','en_route','cancelled','completed')), delay_minutes INTEGER, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), ...)`. Each `tracked_trains` row has exactly **one** owning `user_id` — unlike a line (which many users can pin), there is no fan-out to reason about for trains. This fact simplifies Task 4's train decision logic below (see that task's note).
- `crates/trust-consumer/src/journey.rs:1-52` (read in full): `apply_movement`/`apply_cancellation` are pure, produce a `DerivedState { status, delay_minutes, .. }`. `train_current_state` is the only place a train's *current* derived status/delay lives — there is no per-event derived-state history table, only the raw `train_movement_events` rows plus the one upserted-in-place `train_current_state` row. Task 4 designs around this directly (see that task's note on why trains don't need a Decision-3-style "no preceding row" guard).
- `crates/api/src/app.rs` (read in full): `AppState { config, database: PgPool, redis, oidc }`, `pub type App = Arc<AppState>`, `pub type Router = axum::Router<App>`. `crates/api/src/auth.rs:176-198`: `AuthenticatedUser { id, email, name }`, a real `FromRequestParts<App>` extractor, `401` on no/invalid session — the exact extractor Task 2's `POST /public/notifications/subscribe` uses.
- `crates/api/src/routes/mod.rs` (read in full): `public_router()` merges `health`/`freshness`/`history_retention`/`incidents`/`lines`/`preferences`/`reference`/`auth` routers under `/public` (nested in `crates/api/src/main.rs:60`). Task 2 adds a `notifications` module to this exact list.
- `crates/api/src/routes/preferences.rs:1-24` and `frontend/components/PinToggle.tsx`/`useNeedsLogin.ts` (both read in full): the established Tier-2 shape Task 8's `NotificationsToggle` follows exactly — `useNeedsLogin()`'s `{ needsLogin, reset(), markNeedsLogin() }`, a 401-triggered `LoginPromptModal`, fetch via the same-origin `/api/*` proxy (`frontend/app/api/[...path]/route.ts`, confirmed it already forwards any `/public/...` path with no change needed for Task 2's two new routes).
- `charts/distant-signal/templates/aggregator-deployment.yaml`, `docker/aggregator.Dockerfile`, and `docker-compose.yml:257-273`'s `aggregator:` service block (all read in full): the exact "no HTTP surface, `replicas: 1`, plain `rust:1.88-bookworm` builder, `ca-certificates`-only runtime" shape Task 7 mirrors for `notifier`. `charts/distant-signal/templates/secret.yaml` and `_helpers.tpl`'s `internalTokenSecretName`/`internalTokenSecretKey` pattern (lines 186-196) is the exact shape Task 7 mirrors for the two new VAPID key secret entries.
- `crates/api/migrations/` (directory listing): 19 files, most recent `20260901150000_stanox_crs.sql`. Task 1's new migration is named `20260902100000_notifications.sql`, next in sequence.
- **`web-push` crate, freshly re-checked this session** (the spec's own Open question 2 asked for this): `0.11.0` on crates.io, maintained by `pimeys` (`github.com/pimeys/rust-web-push`). Confirmed: no `reqwest` dependency in either HTTP-client configuration (default `isahc`, or `hyper-client` feature) — the spec's Decision 5 crate-choice risk does not materialize. Confirmed: requires OpenSSL at build time regardless of HTTP-client feature (this workspace already needs `libssl-dev` at build time for `sqlx`'s `tls-native-tls` feature and `trust-consumer`'s own builder stage, so this is not a new class of build dependency, just one more crate needing it). **This plan chooses the `hyper-client` feature over the isahc default** — isahc is a libcurl wrapper, and pulling in libcurl as a second, separate C HTTP stack (beyond OpenSSL, which every service already needs) for one crate is avoidable; `hyper-client` needs no new system library beyond what `native-tls`/OpenSSL already requires. Exact method/type names in Task 6 below (`WebPushMessageBuilder`, `VapidSignatureBuilder`, `ContentEncoding::Aes128Gcm`, `HyperWebPushClient`) are per this session's search of the crate's own docs — **re-verify the exact call shape against `docs.rs/web-push/0.11.0` at the start of Task 6**, per the spec's own posture that this crate's API has moved with semver-breaking changes before.

## Global Constraints

- **Hard, unresolved cross-plan dependency: the service worker.** This entire feature is inert without a registered service worker capable of handling `push` events — there is no other browser API that delivers a notification to a closed/backgrounded tab. As of this plan's writing, `docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md` exists on `main` (confirmed, `50dd6f8`), but **no plan or implementation for it does** — confirmed by listing `docs/superpowers/plans/` for `*pwa-service-worker*`/`*service-worker*` (zero matches beyond the unrelated `2026-09-01-pwa-manifest.md`) and by confirming no `frontend/public/sw.js` or equivalent file exists anywhere in this worktree. **A sibling plan for that spec is being written concurrently, by a different worker, in parallel with this one.** Task 9 below (the only task in this plan that touches a service-worker file) is **blocked** on that sibling plan's implementation landing first — do not attempt Task 9 until a real service-worker file exists on `main`. Every other task in this plan (Tasks 1-8, 10) is independent of the service worker and can proceed regardless. **A human must sequence the two merges**: this plan's Tasks 1-8 can merge in any order relative to the sibling, but Task 9 must land strictly after the sibling's service-worker file exists, and Task 10 (end-to-end manual verification) must land after both.
- **SW push-handler contract, finalized by this plan (Task 9's exact deliverable, not open to reinterpretation at implementation time):** payload shape `{ title: string, body: string, url: string, tag: string }`, JSON-encoded in the push message body. Two required event-handler behaviours: a `push` listener that parses `event.data.json()` and calls `registration.showNotification(title, { body, tag, data: { url } })` (after an optional `clients.matchAll()` focused-tab check — skip showing the notification if a focused client already has `url` open, since `AutoRefresh` already covers that case within 30s); a `notificationclick` listener that calls `clients.openWindow(event.notification.data.url)`. This is a **contract Task 9 imposes on the sibling SW file**, not new SW infrastructure of its own — Task 9 adds exactly these two `addEventListener` blocks to whatever file the sibling plan creates, and touches nothing else in it (no registration, no caching strategy, no lifecycle code).
- **No `reqwest` version conflict.** `web-push 0.11.0` (either HTTP-client feature) has no `reqwest` dependency at all — confirmed fresh this session. `crates/notifier` is a new, independent binary crate regardless, so even a hypothetical conflict would not touch `crates/api`'s pinned `reqwest = "0.12"`.
- **`severity_rank`, never `worst_severity()`, anywhere in the notifier's decision logic** (Decision 2) — this is the one correctness detail most worth double-checking in review, since `worst_severity()` already exists and is easy to reach for by habit.
- **No new "notification preferences" table.** `pinned_lines`/`tracked_trains` are the subscription scope directly (Decision 5) — unpinning is the only "stop notifying me" mechanism. Do not add a `notification_preferences`-shaped table in any task.
- **No new npm dependency.** `PushManager`/`Notification`/`navigator.serviceWorker` are browser-native Web APIs; Task 8 adds no `package.json` entry.
- **Migrations only ever run from `crates/api`** (`crates/api/src/main.rs:95`, `sqlx::migrate!().run(&app.database)`) — `crates/notifier` never runs a migration itself, exactly like `crates/aggregator` today. Task 1's migration must be authored under `crates/api/migrations/`, not a new directory.
- **`crates/notifier` is a new, independent workspace member** — add it to the root `Cargo.toml`'s `[workspace] members` list (Task 5) the same way `crates/schedule-reference` was added most recently.

---

## File structure

```
crates/api/migrations/20260902100000_notifications.sql   NEW -- Task 1
  push_subscriptions, line_notification_state, train_notification_state, notifier_cursor

crates/api/src/data/notifications.rs                      NEW -- Task 2
  upsert_push_subscription(), list_subscriptions_for_user() [test-only helper]
crates/api/src/data/mod.rs                                 MODIFY -- Task 2 (+ pub mod notifications;)
crates/api/src/routes/notifications.rs                     NEW -- Task 2
  router(): GET /notifications/vapid-public-key, POST /notifications/subscribe
crates/api/src/routes/mod.rs                                MODIFY -- Task 2 (+ pub mod notifications; + .merge(notifications::router()))
crates/api/src/data/config.rs                                MODIFY -- Task 2 (+ vapid_public_key field, if not already read elsewhere)

crates/notifier/Cargo.toml                                  NEW -- Task 3 (scaffold) / Task 6 (web-push dep)
crates/notifier/src/main.rs                                  NEW -- Task 3 (scaffold+loop) / Task 6 (send wiring)
crates/notifier/src/config.rs                                 NEW -- Task 3
crates/notifier/src/decision.rs                                NEW -- Task 3
  is_severity_transition(), decide_user_notification(), NotifyDecision,
  train_severity_rank(), decide_train_notification()
crates/notifier/src/queries.rs                                  NEW -- Task 4
  read_cursor(), advance_cursor(), poll_line_candidates(), poll_train_candidates(),
  push_subscriptions_for_user(), upsert_line_notification_state(), upsert_train_notification_state(),
  delete_push_subscription()
crates/notifier/src/send.rs                                       NEW -- Task 6
  NotificationPayload{title,body,url,tag}, send_to_subscription()
Cargo.toml (workspace root)                                        MODIFY -- Task 3 (+ "crates/notifier" member)

docker/notifier.Dockerfile                                          NEW -- Task 7
charts/distant-signal/templates/notifier-deployment.yaml              NEW -- Task 7
charts/distant-signal/templates/secret.yaml                            MODIFY -- Task 7 (+ vapid-public-key, vapid-private-key)
charts/distant-signal/templates/_helpers.tpl                            MODIFY -- Task 7 (+ vapidPublicKeySecretName/Key, vapidPrivateKeySecretName/Key)
charts/distant-signal/values.yaml                                        MODIFY -- Task 7 (+ notifier: section)
docker-compose.yml                                                        MODIFY -- Task 7 (+ notifier: service)

frontend/components/NotificationsToggle.tsx                                NEW -- Task 8
frontend/components/NotificationsToggle.test.tsx                            NEW -- Task 8
frontend/app/page.tsx                                                        MODIFY -- Task 8 (+ <NotificationsToggle />)
frontend/app/page.test.tsx                                                    MODIFY -- Task 8 (mock extension)

<sibling SW file, path TBD by the sibling plan>                                MODIFY -- Task 9 (BLOCKED, see Global Constraints)
```

---

### Task 1: Migration — `push_subscriptions`, `line_notification_state`, `train_notification_state`, `notifier_cursor`

**Files:**
- Create: `crates/api/migrations/20260902100000_notifications.sql`

**Interfaces:**
- Produces: four new tables, exactly as Decision 5/Decision 3 of the spec specify. Every later task's SQL depends on these exact column names/types.
- **Depends on:** nothing — foundational.

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- Web Push notifications: per-device subscriptions, and per-(user, target)
-- last-notified state used for the notifier's escalate-now/cooldown
-- decision (Decision 5, docs/superpowers/specs/2026-09-02-line-status-notifications-design.md).
-- notifier_cursor is the watermark the crates/notifier poll loop advances
-- over line_status_history and train_movement_events (Decision 3).
-- -------------------------------------------------------------------------

CREATE TABLE push_subscriptions (
    id           BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id      TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    endpoint     TEXT        NOT NULL UNIQUE,
    p256dh       TEXT        NOT NULL,
    auth         TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX push_subscriptions_user_id ON push_subscriptions (user_id);

CREATE TABLE line_notification_state (
    user_id                      TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    line_id                      TEXT        NOT NULL,
    last_notified_severity_rank  SMALLINT    NOT NULL,
    last_notified_at             TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, line_id)
);

CREATE TABLE train_notification_state (
    user_id                       TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tracked_train_id              BIGINT      NOT NULL REFERENCES tracked_trains(id) ON DELETE CASCADE,
    last_notified_status          TEXT        NOT NULL,
    last_notified_delay_minutes   INTEGER,
    last_notified_at              TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, tracked_train_id)
);

-- name is either 'line_status_history' or 'train_movement_events'. Rows
-- are upserted by the notifier itself on first use (see
-- crates/notifier/src/queries.rs's read_cursor) rather than seeded here,
-- so this migration only needs to declare the shape.
CREATE TABLE notifier_cursor (
    name               TEXT   PRIMARY KEY,
    last_processed_id  BIGINT NOT NULL DEFAULT 0
);
```

- [ ] **Step 2: Run the migration locally against the dev database**

Run (from repo root, with the local Postgres up per `docker-compose.yml`): `cd crates/api && sqlx migrate run --database-url "$DATABASE_URL"` (or start the `api` service, which runs `sqlx::migrate!()` on boot — either is fine; the api service already does this automatically for every other migration in this repo).
Expected: migration applies with no error; `\d push_subscriptions`, `\d line_notification_state`, `\d train_notification_state`, `\d notifier_cursor` in `psql` show the four tables above.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260902100000_notifications.sql
git commit -m "Add push_subscriptions/line_notification_state/train_notification_state/notifier_cursor tables"
```

---

### Task 2: `api` routes — VAPID public key + subscribe endpoint

**Files:**
- Create: `crates/api/src/data/notifications.rs`
- Create: `crates/api/src/routes/notifications.rs`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/src/routes/mod.rs`
- Modify: `crates/api/src/data/config.rs`
- Test: inline `#[cfg(test)]` modules in both new files, `#[ignore]`d DB tests for the data-layer functions (mirroring `crates/api/src/data/users.rs`'s `session_round_trip_creates_looks_up_and_deletes` precedent, per the spec's own Testing section)

**Interfaces:**
- Produces: `data::notifications::upsert_push_subscription(pool, user_id, endpoint, p256dh, auth) -> anyhow::Result<()>` (an `ON CONFLICT (endpoint) DO UPDATE SET user_id = EXCLUDED.user_id, p256dh = EXCLUDED.p256dh, auth = EXCLUDED.auth, last_seen_at = NOW()` upsert — see the spec's Open question 5 on why re-ownership must be handled this way); `routes::notifications::router() -> Router`.
- Consumed by: `crates/api/src/routes/mod.rs`'s `public_router()` (this task wires the merge in).
- **Depends on:** Task 1's migration (the `push_subscriptions` table must exist).

- [ ] **Step 1: Add the VAPID public key to `ServiceArguments`**

In `crates/api/src/data/config.rs`, find the existing `ServiceArguments` clap struct (same file `crates/api/src/data/train_tracking.rs`'s doc comment references for the crate's config conventions) and add:

```rust
/// The VAPID public key `crates/notifier` signs push messages with —
/// handed to the browser's `PushManager.subscribe({ applicationServerKey })`
/// call unchanged. The matching PRIVATE key lives only in `crates/notifier`'s
/// own config (Task 5) -- `api` never needs it, since `api` only stores
/// subscriptions, it never sends to them.
#[arg(long, env)]
pub vapid_public_key: String,
```

(Match the exact `#[arg(long, env)]` style already used by neighboring fields in this struct — read the file first to place this consistently with its existing field ordering/doc-comment conventions.)

- [ ] **Step 2: Write the failing data-layer test**

Create `crates/api/src/data/notifications.rs`:

```rust
//! `push_subscriptions`: one row per browser/device a user has granted
//! push permission on. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Decision 5 and Open question 5 (endpoint re-ownership).

use anyhow::Result;
use sqlx::PgPool;

/// `ON CONFLICT (endpoint)`, not `(user_id, endpoint)`: the Push API's
/// `endpoint` is already a globally unique per-device-registration URL, so
/// this is also how a shared device correctly re-points its one endpoint
/// row at whichever user is currently logged in and re-subscribes (Open
/// question 5).
pub async fn upsert_push_subscription(
    pool: &PgPool,
    user_id: &str,
    endpoint: &str,
    p256dh: &str,
    auth: &str,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, created_at, last_seen_at) \
         VALUES ($1, $2, $3, $4, NOW(), NOW()) \
         ON CONFLICT (endpoint) DO UPDATE SET \
           user_id = EXCLUDED.user_id, p256dh = EXCLUDED.p256dh, auth = EXCLUDED.auth, last_seen_at = NOW()",
    )
    .bind(user_id)
    .bind(endpoint)
    .bind(p256dh)
    .bind(auth)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for #[ignore]d DB tests");
        PgPoolOptions::new().max_connections(2).connect(&url).await.expect("connect")
    }

    #[tokio::test]
    #[ignore]
    async fn upsert_creates_then_reassigns_ownership_on_conflict() {
        let pool = test_pool().await;
        // Requires two pre-existing users -- 'test-user-a'/'test-user-b' --
        // seeded the same way users.rs's own #[ignore]d tests expect a
        // fixture user to exist; follow that file's exact setup convention.
        upsert_push_subscription(&pool, "test-user-a", "https://push.example/ep1", "p256dh-a", "auth-a")
            .await
            .expect("first insert");
        let owner: String = sqlx::query_scalar("SELECT user_id FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep1")
            .fetch_one(&pool)
            .await
            .expect("read back");
        assert_eq!(owner, "test-user-a");

        // Same endpoint, different user -- re-subscription on a shared device.
        upsert_push_subscription(&pool, "test-user-b", "https://push.example/ep1", "p256dh-b", "auth-b")
            .await
            .expect("second insert (conflict path)");
        let owner: String = sqlx::query_scalar("SELECT user_id FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep1")
            .fetch_one(&pool)
            .await
            .expect("read back after conflict");
        assert_eq!(owner, "test-user-b", "re-subscribing must re-own the row, not create a duplicate");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep1")
            .fetch_one(&pool)
            .await
            .expect("count");
        assert_eq!(count, 1, "ON CONFLICT must update in place, not insert a second row");

        sqlx::query("DELETE FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep1")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
```

- [ ] **Step 3: Register the new data module**

In `crates/api/src/data/mod.rs`, add alongside the existing alphabetical list:

```rust
pub mod notifications;
```

- [ ] **Step 4: Write the route handlers**

Create `crates/api/src/routes/notifications.rs`:

```rust
//! `/public/notifications`: the two endpoints the browser-side push
//! subscribe flow needs. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Decision 6 for the frontend flow this serves.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::notifications;

pub fn router() -> Router {
    Router::new()
        .route("/notifications/vapid-public-key", axum::routing::get(get_vapid_public_key))
        .route("/notifications/subscribe", axum::routing::post(post_subscribe))
}

/// Unauthenticated on purpose -- the browser needs this key BEFORE it has
/// established any session-gated call, to construct the
/// `PushManager.subscribe({ applicationServerKey })` call itself (Decision
/// 6). It is public key material; there is nothing to protect here.
async fn get_vapid_public_key(State(app): State<App>) -> String {
    app.config.vapid_public_key.clone()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SubscribeRequest {
    endpoint: String,
    keys: SubscribeKeys,
}

#[derive(Debug, Deserialize)]
struct SubscribeKeys {
    p256dh: String,
    auth: String,
}

/// Authenticated (Decision 6: a 401 here is what the frontend's
/// `useNeedsLogin()` reacts to). Body shape matches the Push API's own
/// `PushSubscription.toJSON()` output directly, so the frontend can pass
/// it through with no reshaping.
async fn post_subscribe(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(body): Json<SubscribeRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    notifications::upsert_push_subscription(&app.database, &user.id, &body.endpoint, &body.keys.p256dh, &body.keys.auth)
        .await
        .map_err(|err| {
            tracing::error!(error = ?err, "failed to upsert push subscription");
            (StatusCode::INTERNAL_SERVER_ERROR, "failed to save subscription".to_string())
        })?;
    Ok(StatusCode::NO_CONTENT)
}
```

- [ ] **Step 5: Wire the router in**

In `crates/api/src/routes/mod.rs`, add to the `pub mod` list:

```rust
pub mod notifications;
```

And add `.merge(notifications::router())` to `public_router()`'s existing chain (alongside `.merge(preferences::router())` etc.).

- [ ] **Step 6: Run the tests**

Run: `cargo test -p api notifications` (unit-level; the `#[ignore]`d DB test needs `DATABASE_URL` and is run separately, matching this repo's existing convention — `cargo test -p api -- --ignored notifications` against a real local Postgres).
Expected: compiles; the `#[ignore]`d test passes when run against a live local database with the fixture users present.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/notifications.rs crates/api/src/routes/notifications.rs crates/api/src/data/mod.rs crates/api/src/routes/mod.rs crates/api/src/data/config.rs
git commit -m "Add /public/notifications/vapid-public-key and /public/notifications/subscribe"
```

---

### Task 3: `crates/notifier` scaffold + pure decision logic (Decisions 2, 3, 4)

**Files:**
- Create: `crates/notifier/Cargo.toml`
- Create: `crates/notifier/src/config.rs`
- Create: `crates/notifier/src/decision.rs`
- Create: `crates/notifier/src/main.rs`
- Modify: `Cargo.toml` (workspace root)
- Test: inline `#[cfg(test)]` module in `decision.rs`

**Interfaces:**
- Produces: `decision::NotifyDecision { Skip, NotifyNow }`, `decision::is_severity_transition(previous_rank: Option<u8>, new_rank: u8) -> bool`, `decision::decide_user_notification(previous_rank: u8, new_rank: u8, last_notified_rank: Option<u8>, last_notified_at: Option<DateTime<Utc>>, now: DateTime<Utc>, cooldown: Duration) -> NotifyDecision`, `decision::train_severity_rank(status: &str, delay_minutes: Option<i32>, delay_threshold_minutes: i32) -> u8`, `decision::decide_train_notification(previous_rank: u8, new_rank: u8) -> NotifyDecision`. Every one of these is consumed by Task 4's `queries.rs`/the eventual `main.rs` cycle.
- **Depends on:** nothing — this task's deliverable is entirely pure functions plus a crate skeleton that compiles and loops without doing anything real yet (Task 4 fills in the actual polling).

**Design notes carried into this task** (read before writing code — these are real decisions this plan is making, going slightly beyond the spec's own sketch, and later tasks depend on them):

- The spec's own Testing section sketches a single decision function taking `(previous_severity_rank, new_severity_rank, last_notified_severity_rank, last_notified_at, now)`. This plan splits that into two layers because the spec's own Architecture section (step 2 vs. step 4) already describes two genuinely different checks: a **table-level** "did this line's own history actually transition tiers at all" filter (`is_severity_transition`, Decision 2/3 combined — `previous_rank: None` means no preceding history row exists for this `line_id`, i.e. Decision 3's cold-start guard; `Some(p) if p == new_rank` means no real tier change, Decision 2), and a **per-user** "given a real transition happened, does this specific user get notified now or on a cooldown" decision (`decide_user_notification`, Decision 5). The first check runs once per candidate history row; the second runs once per `(user, line)` pin on top of a row that already passed the first check.
- **For trains, this plan deliberately does NOT mirror the two-layer split.** `tracked_trains` has exactly one owning `user_id` per row (Status note above) — there is no fan-out, so "the table's own previous state" and "this user's last-notified state" are the same thing by construction. `train_severity_rank` maps the train's derived state onto the same 0..N rank shape lines use (`0` = normal, `1` = delayed at/above `delay_threshold_minutes`, `2` = cancelled — cancellation is always worse than any delay, matching the spec's Decision 4 framing of a discrete cancellation event as a stronger signal than a delay reading). `decide_train_notification` is intentionally simpler than the line version — **escalation only** (`new_rank > previous_rank` → `NotifyNow`, anything else → `Skip`), with no de-escalation/cooldown branch. This is a deliberate scope choice: the spec's Decision 4 "what's covered" list only names a transition *into* `cancelled` and delay *crossing* the threshold — both are one-directional, "got worse" events — it never asks for a "your train recovered" notification, so there is nothing for a cooldown-gated de-escalation branch to do. Flagged here explicitly as a plan-level decision, not an oversight, in case Task 10's manual verification or later real-usage feedback suggests otherwise.
- **Trains also don't need a Decision-3-style "no preceding row skips" cold-start guard**, for a different reason than the "no fan-out" one above: Decision 3's guard exists because `write_line_status`'s `None => true` branch writes a `line_status_history` row for **every** line simultaneously on a fresh deployment (a real bulk-backfill artifact). No equivalent bulk-backfill exists for `train_movement_events` — a `tracked_trains` row only ever comes from one real user action, one train at a time, never a backfill. So this plan treats a train with no prior `train_notification_state` row as `previous_rank = 0` ("normal") rather than skipping it outright — meaning a user who starts tracking an *already*-cancelled or *already*-delayed train **does** get one notification the first time the notifier observes it. This is treated as correct, desired behaviour for tracked trains (unlike the line case, where "notify every pinner of every line on first deploy" would be a real notification storm), not a gap.

- [ ] **Step 1: Add `crates/notifier` to the workspace**

In the root `Cargo.toml`'s `[workspace] members` list, add `"crates/notifier"` (after `"crates/schedule-reference"`, matching the list's existing append-at-end convention).

- [ ] **Step 2: Write `Cargo.toml`**

Create `crates/notifier/Cargo.toml`:

```toml
[package]
name = "notifier"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
serde_json = "1.0.149"
sqlx = { version = "0.8.6", features = ["chrono", "json", "macros", "postgres", "runtime-tokio", "tls-native-tls"] }
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros", "time"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
```

(`web-push` is added in Task 6, not here — this task's scaffold has no send capability yet, matching this plan's task-by-task testability.)

- [ ] **Step 3: Write the failing decision-logic tests**

Create `crates/notifier/src/decision.rs`, tests first:

```rust
//! Pure severity/status-transition decision logic -- no I/O, no database.
//! See docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Decisions 2, 3, 4, 5 and this plan's Task 3 design notes for why lines
//! and trains use two different-shaped decision functions.

use chrono::{DateTime, Duration, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifyDecision {
    Skip,
    NotifyNow,
}

/// Table-level filter, run once per new `line_status_history` row before
/// any per-user join. `previous_rank = None` means no preceding history
/// row exists for this `line_id` at all (Decision 3's cold-start guard --
/// must not be treated as "changed from nothing").
pub fn is_severity_transition(previous_rank: Option<u8>, new_rank: u8) -> bool {
    match previous_rank {
        None => false,
        Some(previous) => previous != new_rank,
    }
}

/// Per-user decision, called only for a row that already passed
/// `is_severity_transition`. `previous_rank`/`new_rank` are the line's own
/// objective transition (shared across every user pinning this line);
/// `last_notified_rank`/`last_notified_at` are this specific user's own
/// notification history for this line.
pub fn decide_user_notification(
    previous_rank: u8,
    new_rank: u8,
    last_notified_rank: Option<u8>,
    last_notified_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    cooldown: Duration,
) -> NotifyDecision {
    if last_notified_rank == Some(new_rank) {
        // Idempotency guard (Decision 3's note): this user's own state
        // already matches where the line ended up, regardless of cursor
        // position -- do not re-notify.
        return NotifyDecision::Skip;
    }
    let escalated = new_rank > previous_rank;
    if escalated {
        return NotifyDecision::NotifyNow;
    }
    match last_notified_at {
        Some(t) if now - t < cooldown => NotifyDecision::Skip,
        _ => NotifyDecision::NotifyNow,
    }
}

/// Maps a tracked train's derived state onto the same rank shape lines
/// use. Cancellation always outranks any delay reading.
pub fn train_severity_rank(status: &str, delay_minutes: Option<i32>, delay_threshold_minutes: i32) -> u8 {
    if status == "cancelled" {
        2
    } else if delay_minutes.unwrap_or(0) >= delay_threshold_minutes {
        1
    } else {
        0
    }
}

/// Escalation-only (see this plan's Task 3 design notes for why trains
/// don't get a de-escalation/cooldown branch).
pub fn decide_train_notification(previous_rank: u8, new_rank: u8) -> NotifyDecision {
    if new_rank > previous_rank { NotifyDecision::NotifyNow } else { NotifyDecision::Skip }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_prior_history_row_is_never_a_transition() {
        assert!(!is_severity_transition(None, 4));
        assert!(!is_severity_transition(None, 0));
    }

    #[test]
    fn same_rank_is_not_a_transition() {
        assert!(!is_severity_transition(Some(3), 3));
    }

    #[test]
    fn a_real_rank_change_is_a_transition() {
        assert!(is_severity_transition(Some(0), 4));
        assert!(is_severity_transition(Some(4), 0));
    }

    #[test]
    fn escalation_notifies_immediately_even_during_an_active_cooldown() {
        let now = Utc::now();
        let last_notified_at = Some(now - Duration::minutes(5)); // well inside a 20-min cooldown
        let decision = decide_user_notification(0, 4, Some(0), last_notified_at, now, Duration::minutes(20));
        assert_eq!(decision, NotifyDecision::NotifyNow);
    }

    #[test]
    fn deescalation_is_skipped_during_an_active_cooldown() {
        let now = Utc::now();
        let last_notified_at = Some(now - Duration::minutes(5));
        let decision = decide_user_notification(4, 0, Some(4), last_notified_at, now, Duration::minutes(20));
        assert_eq!(decision, NotifyDecision::Skip);
    }

    #[test]
    fn deescalation_notifies_once_the_cooldown_has_elapsed() {
        let now = Utc::now();
        let last_notified_at = Some(now - Duration::minutes(25));
        let decision = decide_user_notification(4, 0, Some(4), last_notified_at, now, Duration::minutes(20));
        assert_eq!(decision, NotifyDecision::NotifyNow);
    }

    #[test]
    fn a_first_ever_notification_for_this_user_is_not_gated_by_any_cooldown() {
        let now = Utc::now();
        let decision = decide_user_notification(0, 3, None, None, now, Duration::minutes(20));
        assert_eq!(decision, NotifyDecision::NotifyNow);
    }

    #[test]
    fn already_notified_this_exact_resulting_state_is_skipped() {
        let now = Utc::now();
        // last_notified_rank already equals new_rank -- e.g. two
        // consecutive equal transitions, or a watermark replay.
        let decision = decide_user_notification(0, 4, Some(4), Some(now - Duration::hours(1)), now, Duration::minutes(20));
        assert_eq!(decision, NotifyDecision::Skip);
    }

    #[test]
    fn train_cancelled_outranks_any_delay() {
        assert_eq!(train_severity_rank("cancelled", Some(2), 15), 2);
        assert!(train_severity_rank("cancelled", None, 15) > train_severity_rank("en_route", Some(999), 15));
    }

    #[test]
    fn train_delay_below_threshold_is_normal_rank() {
        assert_eq!(train_severity_rank("en_route", Some(14), 15), 0);
        assert_eq!(train_severity_rank("en_route", None, 15), 0);
    }

    #[test]
    fn train_delay_at_or_above_threshold_is_rank_one() {
        assert_eq!(train_severity_rank("en_route", Some(15), 15), 1);
        assert_eq!(train_severity_rank("en_route", Some(45), 15), 1);
    }

    #[test]
    fn train_escalation_notifies_deescalation_does_not() {
        assert_eq!(decide_train_notification(0, 1), NotifyDecision::NotifyNow);
        assert_eq!(decide_train_notification(0, 2), NotifyDecision::NotifyNow);
        assert_eq!(decide_train_notification(1, 0), NotifyDecision::Skip);
        assert_eq!(decide_train_notification(2, 1), NotifyDecision::Skip);
        assert_eq!(decide_train_notification(1, 1), NotifyDecision::Skip);
    }

    #[test]
    fn a_newly_tracked_already_delayed_train_does_notify_once() {
        // Status note: no cold-start guard for trains -- previous_rank=0
        // (no prior train_notification_state row) is the correct baseline,
        // not a skip.
        assert_eq!(decide_train_notification(0, 1), NotifyDecision::NotifyNow);
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p notifier`
Expected: all 13 tests PASS (this crate has no DB dependency yet, so no `#[ignore]`d subset).

- [ ] **Step 5: Write `config.rs`**

Create `crates/notifier/src/config.rs`:

```rust
use clap::Parser;

/// CLI/env configuration for the `notifier` service.
#[derive(Debug, Parser)]
pub struct Config {
    #[arg(long, env)]
    pub database_url: String,

    /// How often the notifier polls line_status_history/train_movement_events.
    /// DESIGN.md-style "reasonable round number, revisit with real usage"
    /// posture -- not independently load-tested, matching the spec's own
    /// framing of the cooldown/threshold constants below.
    #[arg(long, env, default_value_t = 60)]
    pub poll_interval_secs: u64,

    /// Decision 5: how long a de-escalation/lateral notification is
    /// suppressed after the last one sent to this user for this line.
    #[arg(long, env, default_value_t = 20)]
    pub cooldown_minutes: i64,

    /// Decision 4: the delay, in minutes, at or above which a tracked
    /// train's delay reading becomes notify-worthy.
    #[arg(long, env, default_value_t = 15)]
    pub train_delay_threshold_minutes: i32,

    /// VAPID keys, PEM-encoded EC private key (`openssl ecparam -genkey
    /// -name prime256v1`) and the matching uncompressed public key --
    /// wired into web-push's VapidSignatureBuilder in Task 6. Fails fast
    /// at startup if either is empty (Task 6), matching this repo's
    /// existing "refuse to start on a missing required secret" posture
    /// (crates/api/src/app.rs's internal_token `ensure!`).
    #[arg(long, env)]
    pub vapid_private_key: String,
    #[arg(long, env)]
    pub vapid_public_key: String,
    /// The `mailto:` or `https:` VAPID "subject" contact, required by the
    /// Web Push protocol's own VAPID spec (RFC 8292) so a push service can
    /// reach the sender if a subscription is being abused.
    #[arg(long, env)]
    pub vapid_subject: String,

    #[arg(long, env, default_value = "info")]
    pub log_level: String,
}
```

- [ ] **Step 6: Write a skeleton `main.rs`**

Create `crates/notifier/src/main.rs`:

```rust
//! `notifier`: polls line_status_history/train_movement_events by
//! watermark and sends Web Push notifications for real severity/status
//! transitions on a user's pinned lines/tracked trains. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md.

mod config;
mod decision;

use std::time::Duration;

use clap::Parser;
use config::Config;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    let config = Config::parse();

    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level)).init();

    let pool = PgPoolOptions::new().max_connections(5).connect(&config.database_url).await?;

    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    loop {
        interval.tick().await;
        tracing::debug!("notifier cycle placeholder -- queries.rs and send.rs wired in Tasks 4/6");
        let _ = &pool; // silences unused-var warning until Task 4 uses it
    }
}
```

(This intentionally does nothing real yet — Task 4 replaces the loop body with the actual poll cycle. The point of this step is a compiling, runnable binary with the pure decision module in place before any I/O is added.)

- [ ] **Step 7: Build check**

Run: `cargo build -p notifier`
Expected: PASS (compiles clean; `cargo clippy -p notifier` should also be clean, matching this workspace's existing lint posture for every other crate).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/notifier/
git commit -m "Scaffold crates/notifier with pure severity/status-transition decision logic"
```

---

### Task 4: Notifier DB queries — watermark polling, candidate joins, state upserts

**Files:**
- Create: `crates/notifier/src/queries.rs`
- Modify: `crates/notifier/src/main.rs`
- Test: inline `#[cfg(test)]` module, `#[ignore]`d DB tests (needs a live Postgres with the schema from Task 1)

**Interfaces:**
- Consumes: `decision::{is_severity_transition, decide_user_notification, train_severity_rank, decide_train_notification, NotifyDecision}` (Task 3).
- Produces: `queries::read_cursor(pool, name) -> Result<i64>`, `queries::advance_cursor(pool, name, new_value) -> Result<()>`, `queries::LineCandidate { id, line_id, new_rank, previous_rank: Option<u8> }`, `queries::poll_line_candidates(pool, since_id) -> Result<Vec<LineCandidate>>`, `queries::TrainCandidate { tracked_train_id, user_id, new_rank, previous_rank }`, `queries::poll_train_candidates(pool, since_id, delay_threshold_minutes) -> Result<(Vec<TrainCandidate>, i64)>` (returns candidates plus the max event id seen, for cursor advancement), `queries::pinned_users_for_line(pool, line_id) -> Result<Vec<String>>`, `queries::line_notification_state(pool, user_id, line_id) -> Result<Option<(u8, DateTime<Utc>)>>`, `queries::upsert_line_notification_state(pool, user_id, line_id, rank, at) -> Result<()>`, `queries::upsert_train_notification_state(pool, user_id, tracked_train_id, status, delay_minutes, at) -> Result<()>`, `queries::push_subscriptions_for_user(pool, user_id) -> Result<Vec<PushSubscriptionRow>>` (`PushSubscriptionRow { id, endpoint, p256dh, auth }`), `queries::delete_push_subscription(pool, id) -> Result<()>`.
- Consumed by: Task 6's `main.rs` cycle wiring.
- **Depends on:** Task 1 (schema), Task 3 (decision functions, `main.rs` skeleton).

- [ ] **Step 1: Write `queries.rs` — cursor read/advance**

```rust
//! Watermark polling and candidate joins over line_status_history /
//! train_movement_events. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Architecture section for the full per-cycle shape this implements.

use chrono::{DateTime, Utc};
use common::{LineStatus, severity_rank};
use sqlx::{PgPool, Row};

use crate::decision::train_severity_rank;

/// Upserts a zero row on first use -- the migration declares the table's
/// shape but deliberately does not seed rows (Task 1), so the first ever
/// poll cycle for a given `name` creates its own starting-at-zero cursor
/// here.
pub async fn read_cursor(pool: &PgPool, name: &str) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO notifier_cursor (name, last_processed_id) VALUES ($1, 0) \
         ON CONFLICT (name) DO UPDATE SET name = EXCLUDED.name \
         RETURNING last_processed_id",
    )
    .bind(name)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn advance_cursor(pool: &PgPool, name: &str, new_value: i64) -> anyhow::Result<()> {
    sqlx::query("UPDATE notifier_cursor SET last_processed_id = $1 WHERE name = $2")
        .bind(new_value)
        .bind(name)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 2: Add the line-candidate poll**

```rust
pub struct LineCandidate {
    pub id: i64,
    pub line_id: String,
    pub new_rank: u8,
    /// Always a real value by construction: only pushed below after
    /// `is_severity_transition` has already required `previous_rank` to be
    /// `Some` (Decision 3's cold-start guard already filtered out the
    /// `None` case) -- `u8`, not `Option<u8>`, so callers never need to
    /// unwrap an invariant that's already been proven true.
    pub previous_rank: u8,
}

fn worst_rank(statuses: &[LineStatus]) -> u8 {
    statuses.iter().map(|s| severity_rank(s.severity)).min().unwrap_or(0)
}

/// One correlated subquery per row to find "the immediately preceding
/// line_status_history row for this same line_id" (Decision 3's guard --
/// NULL previous_statuses means none exists). This workspace's existing
/// data-volume scale ("single trusted personal instance", per DESIGN.md)
/// doesn't justify a window-function rewrite for this; revisit if line
/// count/history volume ever grows enough to matter.
pub async fn poll_line_candidates(pool: &PgPool, since_id: i64) -> anyhow::Result<Vec<LineCandidate>> {
    let rows = sqlx::query(
        "SELECT h.id, h.line_id, h.statuses AS statuses, \
                (SELECT h2.statuses FROM line_status_history h2 \
                   WHERE h2.line_id = h.line_id AND h2.id < h.id \
                   ORDER BY h2.id DESC LIMIT 1) AS previous_statuses \
         FROM line_status_history h \
         WHERE h.id > $1 \
         ORDER BY h.id",
    )
    .bind(since_id)
    .fetch_all(pool)
    .await?;

    let mut candidates = Vec::new();
    for row in rows {
        let id: i64 = row.try_get("id")?;
        let line_id: String = row.try_get("line_id")?;
        let statuses_json: serde_json::Value = row.try_get("statuses")?;
        let statuses: Vec<LineStatus> = serde_json::from_value(statuses_json)?;
        let new_rank = worst_rank(&statuses);

        let previous_rank = match row.try_get::<Option<serde_json::Value>, _>("previous_statuses")? {
            None => None,
            Some(previous_json) => {
                let previous_statuses: Vec<LineStatus> = serde_json::from_value(previous_json)?;
                Some(worst_rank(&previous_statuses))
            }
        };

        if crate::decision::is_severity_transition(previous_rank, new_rank) {
            // Safe: is_severity_transition returning true already requires
            // previous_rank to be Some (its None branch always returns
            // false) -- see the LineCandidate.previous_rank field comment.
            candidates.push(LineCandidate { id, line_id, new_rank, previous_rank: previous_rank.expect("checked by is_severity_transition") });
        }
    }
    Ok(candidates)
}
```

- [ ] **Step 3: Add the train-candidate poll**

```rust
pub struct TrainCandidate {
    pub tracked_train_id: i64,
    pub user_id: String,
    pub new_rank: u8,
    pub previous_rank: u8,
}

/// Per Task 3's design notes: trains have exactly one owning user per
/// `tracked_trains` row, so there is no separate table-level-vs-per-user
/// split here -- `train_notification_state` doubles as both. Candidates
/// are found by touching `train_movement_events` for the watermark, but
/// the actual current status/delay come from `train_current_state`
/// (the only place a train's *current* derived state lives -- see this
/// plan's Status note on journey.rs). Returns candidates plus the max
/// event id seen (for cursor advancement even over trains this cycle
/// found no notify-worthy transition for).
pub async fn poll_train_candidates(
    pool: &PgPool,
    since_id: i64,
    delay_threshold_minutes: i32,
) -> anyhow::Result<(Vec<TrainCandidate>, i64)> {
    let touched = sqlx::query(
        "SELECT DISTINCT e.tracked_train_id, MAX(e.id) OVER () AS max_id \
         FROM train_movement_events e WHERE e.id > $1",
    )
    .bind(since_id)
    .fetch_all(pool)
    .await?;

    if touched.is_empty() {
        return Ok((Vec::new(), since_id));
    }
    let max_id: i64 = touched[0].try_get("max_id")?;

    let mut candidates = Vec::new();
    for row in &touched {
        let tracked_train_id: i64 = row.try_get("tracked_train_id")?;

        let current = sqlx::query(
            "SELECT t.user_id, s.status, s.delay_minutes \
             FROM tracked_trains t JOIN train_current_state s ON s.tracked_train_id = t.id \
             WHERE t.id = $1",
        )
        .bind(tracked_train_id)
        .fetch_optional(pool)
        .await?;
        let Some(current) = current else { continue }; // no current-state row yet -- nothing to compare

        let user_id: String = current.try_get("user_id")?;
        let status: String = current.try_get("status")?;
        let delay_minutes: Option<i32> = current.try_get("delay_minutes")?;
        let new_rank = train_severity_rank(&status, delay_minutes, delay_threshold_minutes);

        let previous = sqlx::query(
            "SELECT last_notified_status, last_notified_delay_minutes \
             FROM train_notification_state WHERE user_id = $1 AND tracked_train_id = $2",
        )
        .bind(&user_id)
        .bind(tracked_train_id)
        .fetch_optional(pool)
        .await?;
        let previous_rank = match previous {
            None => 0, // Task 3's design note: no cold-start guard for trains
            Some(previous) => {
                let previous_status: String = previous.try_get("last_notified_status")?;
                let previous_delay: Option<i32> = previous.try_get("last_notified_delay_minutes")?;
                train_severity_rank(&previous_status, previous_delay, delay_threshold_minutes)
            }
        };

        if crate::decision::decide_train_notification(previous_rank, new_rank) == crate::decision::NotifyDecision::NotifyNow {
            candidates.push(TrainCandidate { tracked_train_id, user_id, new_rank, previous_rank });
        }
    }
    Ok((candidates, max_id))
}
```

- [ ] **Step 4: Add the remaining join/state/subscription helpers**

```rust
pub async fn pinned_users_for_line(pool: &PgPool, line_id: &str) -> anyhow::Result<Vec<String>> {
    let rows = sqlx::query("SELECT user_id FROM pinned_lines WHERE line_id = $1")
        .bind(line_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(|row| Ok(row.try_get("user_id")?)).collect()
}

pub async fn line_notification_state(
    pool: &PgPool,
    user_id: &str,
    line_id: &str,
) -> anyhow::Result<Option<(u8, DateTime<Utc>)>> {
    let row = sqlx::query(
        "SELECT last_notified_severity_rank, last_notified_at FROM line_notification_state \
         WHERE user_id = $1 AND line_id = $2",
    )
    .bind(user_id)
    .bind(line_id)
    .fetch_optional(pool)
    .await?;
    Ok(match row {
        None => None,
        Some(row) => {
            let rank: i16 = row.try_get("last_notified_severity_rank")?;
            let at: DateTime<Utc> = row.try_get("last_notified_at")?;
            Some((rank as u8, at))
        }
    })
}

pub async fn upsert_line_notification_state(
    pool: &PgPool,
    user_id: &str,
    line_id: &str,
    rank: u8,
    at: DateTime<Utc>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO line_notification_state (user_id, line_id, last_notified_severity_rank, last_notified_at) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (user_id, line_id) DO UPDATE SET \
           last_notified_severity_rank = EXCLUDED.last_notified_severity_rank, last_notified_at = EXCLUDED.last_notified_at",
    )
    .bind(user_id)
    .bind(line_id)
    .bind(rank as i16)
    .bind(at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn upsert_train_notification_state(
    pool: &PgPool,
    user_id: &str,
    tracked_train_id: i64,
    status: &str,
    delay_minutes: Option<i32>,
    at: DateTime<Utc>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO train_notification_state \
           (user_id, tracked_train_id, last_notified_status, last_notified_delay_minutes, last_notified_at) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (user_id, tracked_train_id) DO UPDATE SET \
           last_notified_status = EXCLUDED.last_notified_status, \
           last_notified_delay_minutes = EXCLUDED.last_notified_delay_minutes, \
           last_notified_at = EXCLUDED.last_notified_at",
    )
    .bind(user_id)
    .bind(tracked_train_id)
    .bind(status)
    .bind(delay_minutes)
    .bind(at)
    .execute(pool)
    .await?;
    Ok(())
}

pub struct PushSubscriptionRow {
    pub id: i64,
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

pub async fn push_subscriptions_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<Vec<PushSubscriptionRow>> {
    let rows = sqlx::query("SELECT id, endpoint, p256dh, auth FROM push_subscriptions WHERE user_id = $1")
        .bind(user_id)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(PushSubscriptionRow {
                id: row.try_get("id")?,
                endpoint: row.try_get("endpoint")?,
                p256dh: row.try_get("p256dh")?,
                auth: row.try_get("auth")?,
            })
        })
        .collect()
}

/// Error handling: called on a 404/410 from the push service (Task 6) --
/// self-healing cleanup, mirroring users.rs's own "every write takes out
/// its own trash" posture cited by the spec.
pub async fn delete_push_subscription(pool: &PgPool, id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM push_subscriptions WHERE id = $1").bind(id).execute(pool).await?;
    Ok(())
}
```

- [ ] **Step 5: Write the idempotency/cursor-advancement integration test**

Append to `queries.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for #[ignore]d DB tests");
        PgPoolOptions::new().max_connections(2).connect(&url).await.expect("connect")
    }

    #[tokio::test]
    #[ignore]
    async fn a_second_poll_over_an_unchanged_table_finds_no_new_candidates() {
        let pool = test_pool().await;
        // Assumes a 'test-line' row already has at least one
        // line_status_history entry (seed the same way
        // crates/aggregator/src/queries.rs's own tests do, via direct
        // INSERTs against a disposable line_id, cleaned up after).
        let cursor_name = "line_status_history";
        let start = read_cursor(&pool, cursor_name).await.expect("read cursor");
        let first_pass = poll_line_candidates(&pool, start).await.expect("first poll");
        let max_id = first_pass.iter().map(|c| c.id).max().unwrap_or(start);
        advance_cursor(&pool, cursor_name, max_id).await.expect("advance");

        let second_pass = poll_line_candidates(&pool, max_id).await.expect("second poll");
        assert!(second_pass.is_empty(), "an unchanged table must produce zero new candidates on a repeat poll");
    }

    #[tokio::test]
    #[ignore]
    async fn push_subscriptions_round_trip_and_self_cleanup_on_delete() {
        // Mirrors users.rs's own session_round_trip_creates_looks_up_and_deletes
        // shape, per the spec's Testing section -- this is the automated
        // half of the 404/410 self-cleanup path Task 5/6 exercise for
        // real; Task 10's manual pass confirms the real HTTP 404/410
        // trigger, this confirms the DB side of "delete on expired" alone.
        let pool = test_pool().await;
        // Assumes a fixture user 'test-user-a' already exists, same
        // convention as Task 2's upsert test.
        sqlx::query(
            "INSERT INTO push_subscriptions (user_id, endpoint, p256dh, auth, created_at, last_seen_at) \
             VALUES ($1, $2, $3, $4, NOW(), NOW())",
        )
        .bind("test-user-a")
        .bind("https://push.example/ep-cleanup-test")
        .bind("p256dh")
        .bind("auth")
        .execute(&pool)
        .await
        .expect("seed subscription");

        let subscriptions = push_subscriptions_for_user(&pool, "test-user-a").await.expect("list");
        let seeded = subscriptions
            .iter()
            .find(|s| s.endpoint == "https://push.example/ep-cleanup-test")
            .expect("seeded row must be listed");

        delete_push_subscription(&pool, seeded.id).await.expect("delete");

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM push_subscriptions WHERE endpoint = $1")
            .bind("https://push.example/ep-cleanup-test")
            .fetch_one(&pool)
            .await
            .expect("count after delete");
        assert_eq!(remaining, 0, "delete_push_subscription must actually remove the row");
    }
}
```

- [ ] **Step 6: Run the tests**

Run: `cargo test -p notifier` (unit-only, no DB needed for compilation); `cargo test -p notifier -- --ignored` against a live local Postgres with Task 1's migration applied and at least one seeded `line_status_history` row for `a_second_poll_over_an_unchanged_table_finds_no_new_candidates` to exercise.
Expected: PASS.

- [ ] **Step 7: Wire `queries` into `main.rs`'s module list**

In `crates/notifier/src/main.rs`, add `mod queries;` alongside `mod config;`/`mod decision;`. Leave the loop body as Task 3's placeholder for now — Task 6 replaces it with the real cycle once `send.rs` exists.

- [ ] **Step 8: Commit**

```bash
git add crates/notifier/src/queries.rs crates/notifier/src/main.rs
git commit -m "Add notifier's watermark-poll queries, candidate joins, and notification-state upserts"
```

---

### Task 5: `web-push` dependency + VAPID send path

**Files:**
- Modify: `crates/notifier/Cargo.toml`
- Create: `crates/notifier/src/send.rs`

**Interfaces:**
- Produces: `send::NotificationPayload { title: String, body: String, url: String, tag: String }` (exactly the `{title, body, url, tag}` shape this plan's Global Constraints section fixes as the SW contract), `send::SendOutcome { Sent, Expired, TransientFailure }`, `send::send_to_subscription(vapid_private_key: &str, vapid_subject: &str, subscription: &queries::PushSubscriptionRow, payload: &NotificationPayload) -> SendOutcome`.
- Consumed by: Task 6's `main.rs` cycle wiring.
- **Depends on:** Task 4 (`PushSubscriptionRow`).

**Before writing code, verify the exact `web-push 0.11.0` API surface against `https://docs.rs/web-push/0.11.0`** — this plan's Status note flags this crate's API as having moved with semver-breaking changes before, and the exact symbols below are from this planning session's web search, not a direct read of the crate's source. If the pinned version's actual API differs from what's below, adapt this task's code to match the real signatures rather than forcing the crate to fit this sketch.

- [ ] **Step 1: Add the dependency**

In `crates/notifier/Cargo.toml`, add:

```toml
web-push = { version = "0.11", default-features = false, features = ["hyper-client"] }
```

(`hyper-client`, not the `isahc` default — see this plan's Status note on why: avoids pulling in libcurl as a second C HTTP stack when this workspace's other services already standardize on OpenSSL/`native-tls` for the one system dependency they need.)

- [ ] **Step 2: Write `send.rs`**

```rust
//! Sends one Web Push message to one subscription, VAPID-signed. See
//! docs/superpowers/specs/2026-09-02-line-status-notifications-design.md's
//! Error handling section for the 404/410-vs-transient-failure split this
//! implements.

use serde::Serialize;
use web_push::{
    ContentEncoding, SubscriptionInfo, SubscriptionKeys, VapidSignatureBuilder,
    WebPushClient, WebPushMessageBuilder,
};

use crate::queries::PushSubscriptionRow;

/// Exactly the SW contract this plan's Global Constraints section fixes.
/// Any change to this shape must be reflected in Task 9's push-handler
/// code -- they are two hand-written halves of the same wire contract.
#[derive(Debug, Serialize)]
pub struct NotificationPayload {
    pub title: String,
    pub body: String,
    pub url: String,
    pub tag: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SendOutcome {
    Sent,
    /// 404/410 from the push service -- caller must delete the subscription.
    Expired,
    /// Anything else (5xx, timeout, etc.) -- caller logs and moves on, no
    /// retry queue (Error handling section, spec).
    TransientFailure,
}

pub async fn send_to_subscription(
    vapid_private_key: &str,
    vapid_subject: &str,
    subscription: &PushSubscriptionRow,
    payload: &NotificationPayload,
) -> SendOutcome {
    let subscription_info = SubscriptionInfo {
        endpoint: subscription.endpoint.clone(),
        keys: SubscriptionKeys { p256dh: subscription.p256dh.clone(), auth: subscription.auth.clone() },
    };

    let mut signature_builder = match VapidSignatureBuilder::from_pem(vapid_private_key.as_bytes(), &subscription_info) {
        Ok(builder) => builder,
        Err(err) => {
            tracing::error!(error = ?err, "invalid VAPID private key"); // startup-time fail-fast (Task 6) should prevent this in practice
            return SendOutcome::TransientFailure;
        }
    };
    signature_builder.add_claim("sub", vapid_subject);
    let signature = match signature_builder.build() {
        Ok(sig) => sig,
        Err(err) => {
            tracing::error!(error = ?err, "failed to build VAPID signature");
            return SendOutcome::TransientFailure;
        }
    };

    let body = match serde_json::to_vec(payload) {
        Ok(body) => body,
        Err(err) => {
            tracing::error!(error = ?err, "failed to serialize notification payload");
            return SendOutcome::TransientFailure;
        }
    };

    let mut message_builder = WebPushMessageBuilder::new(&subscription_info);
    message_builder.set_payload(ContentEncoding::Aes128Gcm, &body);
    message_builder.set_vapid_signature(signature);

    let message = match message_builder.build() {
        Ok(message) => message,
        Err(err) => {
            tracing::error!(error = ?err, "failed to build web push message");
            return SendOutcome::TransientFailure;
        }
    };

    let client = web_push::HyperWebPushClient::new();
    // Two bounded retries on a transient failure, per the spec's Error
    // handling section -- no dead-letter queue, no retry-after-restart
    // mechanism; a genuinely persistent change is picked up again next
    // cycle since notification_state is only updated on send success.
    for attempt in 0..3 {
        match client.send(message.clone()).await {
            Ok(_) => return SendOutcome::Sent,
            Err(web_push::WebPushError::EndpointNotValid) | Err(web_push::WebPushError::EndpointNotFound) => {
                return SendOutcome::Expired;
            }
            Err(err) => {
                tracing::warn!(error = ?err, attempt, "web push send failed, retrying");
            }
        }
    }
    SendOutcome::TransientFailure
}
```

- [ ] **Step 3: Build check**

Run: `cargo build -p notifier`
Expected: PASS. If the real `web-push 0.11.0` API differs from the symbols above (`SubscriptionInfo`/`SubscriptionKeys`/`VapidSignatureBuilder`/`WebPushMessageBuilder`/`ContentEncoding`/`HyperWebPushClient`/`WebPushError` variant names), fix this file to match what actually compiles against the pinned version — do not silently downgrade to an older crate version to make the sketch above compile as-is.

- [ ] **Step 4: Commit**

```bash
git add crates/notifier/Cargo.toml crates/notifier/src/send.rs
git commit -m "Add web-push (hyper-client) dependency and VAPID send path"
```

---

### Task 6: Wire the full poll cycle in `main.rs` + startup fail-fast

**Files:**
- Modify: `crates/notifier/src/main.rs`
- Test: `#[ignore]`d DB integration test

**Interfaces:**
- Consumes: everything from Tasks 3-5 (`decision`, `queries`, `send`).
- Produces: the complete `run_cycle(pool, cooldown, train_delay_threshold_minutes, vapid_private_key, vapid_subject) -> anyhow::Result<()>` function, the real deliverable of this whole crate.
- **Depends on:** Tasks 3, 4, 5.

- [ ] **Step 1: Add the VAPID startup fail-fast check**

In `crates/notifier/src/main.rs`'s `main()`, immediately after `Config::parse()`:

```rust
anyhow::ensure!(!config.vapid_private_key.is_empty(), "vapid_private_key (--vapid-private-key / VAPID_PRIVATE_KEY) must not be empty");
anyhow::ensure!(!config.vapid_public_key.is_empty(), "vapid_public_key (--vapid-public-key / VAPID_PUBLIC_KEY) must not be empty");
anyhow::ensure!(!config.vapid_subject.is_empty(), "vapid_subject (--vapid-subject / VAPID_SUBJECT) must not be empty");
```

(Matches `crates/api/src/app.rs`'s existing `ensure!(!config.internal_token.is_empty(), ...)` posture, per the spec's Error handling section: "fail fast... rather than silently no-op every cycle.")

- [ ] **Step 2: Replace the placeholder loop body with `run_cycle`**

```rust
mod config;
mod decision;
mod queries;
mod send;

use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use config::Config;
use send::{NotificationPayload, SendOutcome, send_to_subscription};
use sqlx::PgPool;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    let config = Config::parse();

    anyhow::ensure!(!config.vapid_private_key.is_empty(), "vapid_private_key (--vapid-private-key / VAPID_PRIVATE_KEY) must not be empty");
    anyhow::ensure!(!config.vapid_public_key.is_empty(), "vapid_public_key (--vapid-public-key / VAPID_PUBLIC_KEY) must not be empty");
    anyhow::ensure!(!config.vapid_subject.is_empty(), "vapid_subject (--vapid-subject / VAPID_SUBJECT) must not be empty");

    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level)).init();

    let pool = sqlx::postgres::PgPoolOptions::new().max_connections(5).connect(&config.database_url).await?;

    let cooldown = chrono::Duration::minutes(config.cooldown_minutes);
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    loop {
        interval.tick().await;
        let result = run_cycle(
            &pool,
            cooldown,
            config.train_delay_threshold_minutes,
            &config.vapid_private_key,
            &config.vapid_subject,
        )
        .await;
        if let Err(err) = result {
            tracing::error!(error = ?err, "notifier cycle failed; will retry next interval");
        }
    }
}

async fn run_cycle(
    pool: &PgPool,
    cooldown: chrono::Duration,
    train_delay_threshold_minutes: i32,
    vapid_private_key: &str,
    vapid_subject: &str,
) -> anyhow::Result<()> {
    let now = Utc::now();

    // --- Lines (Decision 2/3/5) ---
    let line_cursor_start = queries::read_cursor(pool, "line_status_history").await?;
    let line_candidates = queries::poll_line_candidates(pool, line_cursor_start).await?;
    let line_max_id = line_candidates.iter().map(|c| c.id).max().unwrap_or(line_cursor_start);

    for candidate in &line_candidates {
        let user_ids = queries::pinned_users_for_line(pool, &candidate.line_id).await?;
        for user_id in user_ids {
            let state = queries::line_notification_state(pool, &user_id, &candidate.line_id).await?;
            let (last_notified_rank, last_notified_at) = match state {
                Some((rank, at)) => (Some(rank), Some(at)),
                None => (None, None),
            };
            let decision = decision::decide_user_notification(
                candidate.previous_rank,
                candidate.new_rank,
                last_notified_rank,
                last_notified_at,
                now,
                cooldown,
            );
            if decision != decision::NotifyDecision::NotifyNow {
                continue;
            }

            let payload = NotificationPayload {
                title: "Line status changed".to_string(),
                body: format!("{} has a new status.", candidate.line_id), // Task 10 note below on richer copy
                url: format!("/lines/{}", candidate.line_id),
                tag: format!("line-{}", candidate.line_id),
            };
            if send_to_all_subscriptions(pool, &user_id, &payload, vapid_private_key, vapid_subject).await? {
                queries::upsert_line_notification_state(pool, &user_id, &candidate.line_id, candidate.new_rank, now).await?;
            }
        }
    }
    queries::advance_cursor(pool, "line_status_history", line_max_id).await?;

    // --- Trains (Decision 4) ---
    let train_cursor_start = queries::read_cursor(pool, "train_movement_events").await?;
    let (train_candidates, train_max_id) =
        queries::poll_train_candidates(pool, train_cursor_start, train_delay_threshold_minutes).await?;

    for candidate in &train_candidates {
        let (status, delay_minutes) = current_train_state(pool, candidate.tracked_train_id).await?;
        let payload = NotificationPayload {
            title: if status == "cancelled" { "Your train was cancelled".to_string() } else { "Your train is delayed".to_string() },
            body: match delay_minutes {
                Some(minutes) if status != "cancelled" => format!("Now running about {minutes} minutes late."),
                _ => "Check the latest status.".to_string(),
            },
            url: format!("/track/{}", candidate.tracked_train_id),
            tag: format!("train-{}", candidate.tracked_train_id),
        };
        if send_to_all_subscriptions(pool, &candidate.user_id, &payload, vapid_private_key, vapid_subject).await? {
            queries::upsert_train_notification_state(pool, &candidate.user_id, candidate.tracked_train_id, &status, delay_minutes, now)
                .await?;
        }
    }
    queries::advance_cursor(pool, "train_movement_events", train_max_id).await?;

    Ok(())
}

async fn current_train_state(pool: &PgPool, tracked_train_id: i64) -> anyhow::Result<(String, Option<i32>)> {
    use sqlx::Row;
    let row = sqlx::query("SELECT status, delay_minutes FROM train_current_state WHERE tracked_train_id = $1")
        .bind(tracked_train_id)
        .fetch_one(pool)
        .await?;
    Ok((row.try_get("status")?, row.try_get("delay_minutes")?))
}

/// Sends to every device this user has subscribed on (Decision 5's
/// per-user, not per-subscription, fan-out). Returns true if at least one
/// send succeeded (or the user has zero subscriptions -- see below) --
/// callers use this to decide whether to update notification_state.
///
/// A user with zero push_subscriptions rows still counts as "handled" (not
/// a failure) -- notification_state still advances so a later real
/// subscription doesn't immediately fire a backlog of stale transitions.
async fn send_to_all_subscriptions(
    pool: &PgPool,
    user_id: &str,
    payload: &NotificationPayload,
    vapid_private_key: &str,
    vapid_subject: &str,
) -> anyhow::Result<bool> {
    let subscriptions = queries::push_subscriptions_for_user(pool, user_id).await?;
    if subscriptions.is_empty() {
        return Ok(true);
    }
    let mut any_ok = false;
    for subscription in &subscriptions {
        match send_to_subscription(vapid_private_key, vapid_subject, subscription, payload).await {
            SendOutcome::Sent => any_ok = true,
            SendOutcome::Expired => {
                queries::delete_push_subscription(pool, subscription.id).await?;
            }
            SendOutcome::TransientFailure => {
                tracing::warn!(user_id, endpoint = %subscription.endpoint, "transient push send failure, will retry next real transition");
            }
        }
    }
    Ok(any_ok)
}
```

- [ ] **Step 3: Build check**

Run: `cargo build -p notifier && cargo clippy -p notifier`
Expected: PASS, no warnings.

- [ ] **Step 4: Write the end-to-end `#[ignore]`d cycle test**

Append a test module to `main.rs` (or a `tests/` integration test file, matching whichever this repo's existing binaries prefer — check `crates/aggregator/src/main.rs`'s own `#[cfg(test)]` module for the convention and mirror it) asserting: seed a `pinned_lines` row + two `line_status_history` rows for the same `line_id` at different ranks + a `push_subscriptions` row for that user; run `run_cycle` once; assert `line_notification_state` now has a row with the expected rank; run `run_cycle` again with no new data; assert no panic and the state is unchanged (idempotent). This test will fail on the real HTTP send (no real push endpoint), which is expected — assert on the DB side effects (`line_notification_state`), not on send success, following the spec's Testing section's own "the crate's real send path is not unit-testable meaningfully" posture. Structure the test to seed a `push_subscriptions.endpoint` pointing at an obviously-invalid URL and treat a `TransientFailure`/error outcome as acceptable, asserting only that `run_cycle` itself returns `Ok(())` and doesn't panic.

- [ ] **Step 5: Run it**

Run: `cargo test -p notifier -- --ignored` against a live local Postgres.
Expected: PASS (with the caveat in Step 4 about not asserting on real send success).

- [ ] **Step 6: Commit**

```bash
git add crates/notifier/src/main.rs
git commit -m "Wire the full notifier poll cycle: line and train transitions to Web Push sends"
```

---

### Task 7: Docker + Helm chart deployment

**Files:**
- Create: `docker/notifier.Dockerfile`
- Create: `charts/distant-signal/templates/notifier-deployment.yaml`
- Modify: `charts/distant-signal/templates/secret.yaml`
- Modify: `charts/distant-signal/templates/_helpers.tpl`
- Modify: `charts/distant-signal/values.yaml`
- Modify: `docker-compose.yml`

**Interfaces:** none new — this task only deploys the binary Task 6 produces.
**Depends on:** Task 6 (the binary must build); Task 2 (api's `VAPID_PUBLIC_KEY` env var must exist so the same secret value can be shared).

- [ ] **Step 1: Write `docker/notifier.Dockerfile`**

Mirror `docker/aggregator.Dockerfile` exactly (same `rust:1.88-bookworm` builder pin, same cache-mount shape, same `ca-certificates`-only runtime stage), but add `libssl-dev pkg-config` to the builder stage's `apt-get install` (needed at build time by `web-push`'s OpenSSL dependency, confirmed by this plan's Status note — `trust-consumer.Dockerfile`'s builder stage already does the same for its own, unrelated reason) and `libssl3` to the runtime stage (the `web-push` crate itself, unlike `sqlx`'s `tls-native-tls`, may dynamically link against OpenSSL at runtime depending on which TLS backend `hyper-client`'s own TLS stack resolves to — confirm via `ldd` against a locally built binary during this task, matching `trust-consumer.Dockerfile`'s own documented verification method, and drop the runtime package if `ldd` shows no such link):

```dockerfile
# syntax=docker/dockerfile:1
# Multi-stage build for the `notifier` service. Mirrors
# docker/aggregator.Dockerfile's shape exactly (no HTTP surface, same
# rust:1.88-bookworm builder pin, same cache-mount pattern) with one
# addition: libssl-dev/pkg-config in the builder stage, needed at build
# time by the web-push crate's OpenSSL dependency (confirmed by this
# plan's own Status note) -- the same requirement docker/trust-consumer.Dockerfile
# already documents for its own, unrelated reason (rdkafka's sasl2-sys).
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

RUN apt-get update \
    && apt-get install -y --no-install-recommends libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin notifier; \
    else \
      cargo build --bin notifier; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/notifier /usr/local/bin/notifier

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin notifier

COPY --from=builder /usr/local/bin/notifier /usr/local/bin/notifier

USER notifier

ENTRYPOINT ["/usr/local/bin/notifier"]
```

- [ ] **Step 2: Verify the runtime OpenSSL link (don't skip)**

Build the image, then run: `docker run --rm --entrypoint ldd <built-image-tag> /usr/local/bin/notifier | grep -i ssl`
If it shows no `libssl`/`libcrypto` link, remove `libssl3` from the runtime stage's `apt-get install` line (keep `libssl-dev`/`pkg-config` in the builder stage regardless — those are compile-time-only, matching `trust-consumer.Dockerfile`'s documented `libcurl4-openssl-dev`-without-a-runtime-counterpart precedent for the identical reasoning).

- [ ] **Step 3: Add VAPID key secret entries**

In `charts/distant-signal/templates/_helpers.tpl`, add two new helper pairs immediately after the existing `internalTokenSecretName`/`internalTokenSecretKey` pair (lines 186-196), same shape:

```
{{- define "distant-signal.vapidPublicKeySecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.notifier.vapid.existingSecret }}
{{- end }}

{{- define "distant-signal.vapidPublicKeySecretKey" -}}
{{- if .Values.notifier.vapid.existingSecret }}
{{- .Values.notifier.vapid.existingSecretPublicKeyKey }}
{{- else }}
{{- print "vapid-public-key" }}
{{- end }}
{{- end }}

{{- define "distant-signal.vapidPrivateKeySecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.notifier.vapid.existingSecret }}
{{- end }}

{{- define "distant-signal.vapidPrivateKeySecretKey" -}}
{{- if .Values.notifier.vapid.existingSecret }}
{{- .Values.notifier.vapid.existingSecretPrivateKeyKey }}
{{- else }}
{{- print "vapid-private-key" }}
{{- end }}
{{- end }}
```

In `charts/distant-signal/templates/secret.yaml`, add a new block (following the `kafka-sasl-username`/`kafka-sasl-password` block's exact "deliberately NOT auto-generated" shape, lines 72-81 — a random VAPID key pair would need to be a *matched* EC keypair, not two independent random strings, so this cannot use the postgres-password/internal-token `randAlphaNum` auto-gen pattern):

```
{{/* vapid-public-key / vapid-private-key: like kafka-sasl-*, deliberately
     NOT auto-generated -- these must be a real matched VAPID EC keypair
     (openssl ecparam -genkey -name prime256v1), not independent random
     strings. Rendered (possibly empty) whenever no existingSecret is
     configured, so the notifier pod's secretKeyRefs always resolve
     instead of wedging in CreateContainerConfigError -- notifier's own
     startup fail-fast (Task 6) is what actually catches an empty value,
     the same division of labour trustConsumer's kafka-sasl-* pair uses. */}}
{{- if not .Values.notifier.vapid.existingSecret -}}
{{- $_ := set $data "vapid-public-key" (.Values.notifier.vapid.publicKey | default "" | b64enc) -}}
{{- $_ := set $data "vapid-private-key" (.Values.notifier.vapid.privateKey | default "" | b64enc) -}}
{{- end -}}
```

- [ ] **Step 4: Add `notifier-deployment.yaml`**

Mirror `charts/distant-signal/templates/aggregator-deployment.yaml`'s exact shape (no HTTP ports/probes, `replicas: 1`, same `Recreate` strategy rationale — two notifier replicas would double-send every notification, an even more directly user-visible failure mode than aggregator's double-write risk):

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-notifier" (include "distant-signal.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "notifier") | nindent 4 }}
spec:
  # Fixed at 1 -- two replicas would double-send every push notification.
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      {{- include "distant-signal.selectorLabels" (dict "root" . "component" "notifier") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "distant-signal.labels" (dict "root" . "component" "notifier") | nindent 8 }}
      {{- with .Values.notifier.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "distant-signal.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "distant-signal.podSecurityContext" (dict "override" .Values.notifier.podSecurityContext) | nindent 8 }}
      containers:
        - name: notifier
          image: {{ include "distant-signal.image" (dict "root" . "image" .Values.notifier.image) | quote }}
          imagePullPolicy: {{ .Values.notifier.image.pullPolicy }}
          securityContext:
            {{- include "distant-signal.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          # No probes: exposes no HTTP surface, same as aggregator.
          env:
            {{- include "distant-signal.databaseEnv" . | nindent 12 }}
            - name: POLL_INTERVAL_SECS
              value: {{ .Values.notifier.pollIntervalSecs | quote }}
            - name: COOLDOWN_MINUTES
              value: {{ .Values.notifier.cooldownMinutes | quote }}
            - name: TRAIN_DELAY_THRESHOLD_MINUTES
              value: {{ .Values.notifier.trainDelayThresholdMinutes | quote }}
            - name: VAPID_SUBJECT
              value: {{ .Values.notifier.vapid.subject | quote }}
            - name: VAPID_PUBLIC_KEY
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.vapidPublicKeySecretName" . }}
                  key: {{ include "distant-signal.vapidPublicKeySecretKey" . }}
            - name: VAPID_PRIVATE_KEY
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.vapidPrivateKeySecretName" . }}
                  key: {{ include "distant-signal.vapidPrivateKeySecretKey" . }}
            - name: RUST_LOG
              value: {{ .Values.notifier.logLevel | quote }}
            {{- with .Values.notifier.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with .Values.notifier.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.notifier.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.notifier.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.notifier.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
```

- [ ] **Step 5: Add `values.yaml` entries**

In `charts/distant-signal/values.yaml`, add a `notifier:` section (immediately after the existing `aggregator:` block, matching its exact shape):

```yaml
notifier:
  image:
    repository: distant-signal/notifier
    tag: ""
    pullPolicy: IfNotPresent
  # -- Same "reasonable round number, not load-tested" posture as
  # aggregator.pollIntervalSecs.
  pollIntervalSecs: 60
  cooldownMinutes: 20
  trainDelayThresholdMinutes: 15
  vapid:
    # -- mailto: or https: contact, required by RFC 8292's VAPID "sub" claim.
    subject: ""
    # -- PEM EC private key / uncompressed public key (openssl ecparam
    # -genkey -name prime256v1). Never auto-generated -- see secret.yaml's
    # own comment on why this differs from postgres-password/internal-token.
    publicKey: ""
    privateKey: ""
    existingSecret: ""
    existingSecretPublicKeyKey: "vapid-public-key"
    existingSecretPrivateKeyKey: "vapid-private-key"
  logLevel: info
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

Also add the same `vapid.publicKey`/`existingSecret` wiring to the existing `api:` section's values (a new `api.vapidPublicKey` field pointed at the *same* secret key, since `crates/api`'s `ServiceArguments` (Task 2) also needs `VAPID_PUBLIC_KEY` — read to hand to the browser, never the private key) — add to `api-deployment.yaml`:

```yaml
            - name: VAPID_PUBLIC_KEY
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.vapidPublicKeySecretName" . }}
                  key: {{ include "distant-signal.vapidPublicKeySecretKey" . }}
```

(placed alongside `api-deployment.yaml`'s other `secretKeyRef`-sourced env entries; do not add `VAPID_PRIVATE_KEY` to `api`'s Deployment — `api` never sends push, only `notifier` does).

- [ ] **Step 6: Add the `notifier` service to `docker-compose.yml`**

Mirror the `aggregator:` block (lines 257-273) exactly, with notifier's own env vars:

```yaml
  notifier:
    build:
      context: .
      dockerfile: docker/notifier.Dockerfile
      args:
        CARGO_PROFILE: release
    restart: unless-stopped
    depends_on:
      api:
        condition: service_healthy
    environment:
      # crates/notifier/src/config.rs: Config
      DATABASE_URL: postgres://${POSTGRES_USER}:${POSTGRES_PASSWORD}@postgres:5432/${POSTGRES_DB}
      POLL_INTERVAL_SECS: ${POLL_INTERVAL_SECS_NOTIFIER:-60}
      COOLDOWN_MINUTES: ${COOLDOWN_MINUTES_NOTIFIER:-20}
      TRAIN_DELAY_THRESHOLD_MINUTES: ${TRAIN_DELAY_THRESHOLD_MINUTES_NOTIFIER:-15}
      VAPID_SUBJECT: ${VAPID_SUBJECT:-mailto:admin@example.invalid}
      VAPID_PUBLIC_KEY: ${VAPID_PUBLIC_KEY:-}
      VAPID_PRIVATE_KEY: ${VAPID_PRIVATE_KEY:-}
      RUST_LOG: ${RUST_LOG:-info}
```

Also add `VAPID_PUBLIC_KEY: ${VAPID_PUBLIC_KEY:-}` to the existing `api:` service block's `environment:` (same value, `api` never gets the private key).

- [ ] **Step 7: Local verification**

Run: `helm template charts/distant-signal --set notifier.vapid.subject=mailto:test@example.invalid --set notifier.vapid.publicKey=test --set notifier.vapid.privateKey=test | grep -A5 "kind: Deployment" | grep notifier` — confirms the chart renders without error and the new Deployment appears. Then `docker compose build notifier` locally to confirm the Dockerfile builds.
Expected: both PASS.

- [ ] **Step 8: Commit**

```bash
git add docker/notifier.Dockerfile charts/distant-signal/templates/notifier-deployment.yaml charts/distant-signal/templates/secret.yaml charts/distant-signal/templates/_helpers.tpl charts/distant-signal/values.yaml charts/distant-signal/templates/api-deployment.yaml docker-compose.yml
git commit -m "Deploy the notifier service: Dockerfile, Helm chart, docker-compose, and shared VAPID public key on api"
```

---

### Task 8: Frontend — "Enable notifications" Tier-2 control

**Files:**
- Create: `frontend/components/NotificationsToggle.tsx`
- Create: `frontend/components/NotificationsToggle.test.tsx`
- Modify: `frontend/app/page.tsx`
- Modify: `frontend/app/page.test.tsx` (if it mocks the page's child components — check the existing file's mock list before editing; extend it, following `TrendsResults.test.tsx`'s pattern of adding to an existing mock block rather than replacing it)

**Interfaces:**
- Produces: `export function NotificationsToggle()` — a self-contained client component, no props needed (it reads nothing page-specific; Decision 6's "single global toggle").
- **Depends on:** Task 2 (`GET /public/notifications/vapid-public-key`, `POST /public/notifications/subscribe` must exist for real manual verification; unit tests mock both, so this task's code/tests can be written in parallel with Task 2, but Task 10's manual smoke test needs both landed).

- [ ] **Step 1: Write the failing test**

Create `frontend/components/NotificationsToggle.test.tsx`:

```tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/renderWithMantine'; // match this repo's actual helper path/name -- confirm against PinToggle.test.tsx's own import before writing this
import { NotificationsToggle } from './NotificationsToggle';

describe('NotificationsToggle', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  it('renders nothing (or a disabled state) when the browser has no PushManager/serviceWorker support', () => {
    // jsdom has neither by default -- this is the real, unmocked baseline.
    renderWithMantine(<NotificationsToggle />);
    expect(screen.queryByRole('button', { name: /enable notifications/i })).not.toBeInTheDocument();
  });

  it('subscribes and shows LoginPromptModal on a 401 from the subscribe POST', async () => {
    const fakeRegistration = {
      pushManager: {
        subscribe: vi.fn().mockResolvedValue({
          endpoint: 'https://push.example/ep1',
          toJSON: () => ({ endpoint: 'https://push.example/ep1', keys: { p256dh: 'p', auth: 'a' } }),
        }),
      },
    };
    // @ts-expect-error -- test-only global stubs for Web APIs jsdom doesn't implement.
    global.navigator.serviceWorker = { ready: Promise.resolve(fakeRegistration) };
    // @ts-expect-error
    global.window.PushManager = function () {};
    // @ts-expect-error
    global.Notification = { requestPermission: vi.fn().mockResolvedValue('granted') };

    vi.spyOn(global, 'fetch')
      .mockResolvedValueOnce(new Response('test-vapid-key', { status: 200 })) // GET vapid-public-key
      .mockResolvedValueOnce(new Response(null, { status: 401 })); // POST subscribe

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByText(/log in to enable notifications/i)).toBeInTheDocument());
  });

  it('subscribes successfully on a 204 from the subscribe POST', async () => {
    const fakeRegistration = {
      pushManager: {
        subscribe: vi.fn().mockResolvedValue({
          endpoint: 'https://push.example/ep1',
          toJSON: () => ({ endpoint: 'https://push.example/ep1', keys: { p256dh: 'p', auth: 'a' } }),
        }),
      },
    };
    // @ts-expect-error
    global.navigator.serviceWorker = { ready: Promise.resolve(fakeRegistration) };
    // @ts-expect-error
    global.window.PushManager = function () {};
    // @ts-expect-error
    global.Notification = { requestPermission: vi.fn().mockResolvedValue('granted') };

    vi.spyOn(global, 'fetch')
      .mockResolvedValueOnce(new Response('test-vapid-key', { status: 200 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }));

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole('button', { name: /notifications enabled/i })).toBeInTheDocument());
  });
});
```

(This test file's exact import path for `renderWithMantine` and the precise jsdom-global-stubbing approach must be confirmed against `frontend/components/PinToggle.test.tsx`'s own real imports/setup before finalizing — read that file first, since it's this component's closest sibling and already solves "test a component wrapped in Mantine".)

- [ ] **Step 2: Run the test to verify it fails**

Run (from `frontend/`): `npm test -- NotificationsToggle`
Expected: FAIL — `./NotificationsToggle` module doesn't exist yet.

- [ ] **Step 3: Write the component**

Create `frontend/components/NotificationsToggle.tsx`:

```tsx
'use client';

import { useEffect, useState } from 'react';
import { Button } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

/** Global "Enable notifications" control (Decision 6) -- not per-line,
 * since Decision 5 reuses pinned_lines/tracked_trains directly as scope.
 * Renders for every visitor (Tier 2, per docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md),
 * gated on browser capability, not install state (Decision 1 -- Android
 * and every desktop browser get real push from a bare open tab; only iOS
 * requires Home Screen install, which this component makes no attempt to
 * detect or require). */
export function NotificationsToggle() {
  const [supported, setSupported] = useState(false);
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const needsLoginState = useNeedsLogin();

  useEffect(() => {
    setSupported('serviceWorker' in navigator && 'PushManager' in window);
  }, []);

  async function enable() {
    setBusy(true);
    needsLoginState.reset();
    try {
      const permission = await Notification.requestPermission();
      if (permission !== 'granted') {
        return;
      }

      const keyResponse = await fetch('/api/notifications/vapid-public-key');
      if (!keyResponse.ok) {
        return;
      }
      const vapidPublicKey = await keyResponse.text();

      // Resolves once whatever service worker the sibling PWA effort
      // registers is active -- this component makes no assumption about
      // that SW's own file location or scope.
      const registration = await navigator.serviceWorker.ready;
      const subscription = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: vapidPublicKey,
      });
      const subscriptionJson = subscription.toJSON();

      const subscribeResponse = await fetch('/api/notifications/subscribe', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ endpoint: subscriptionJson.endpoint, keys: subscriptionJson.keys }),
      });
      if (!subscribeResponse.ok) {
        if (subscribeResponse.status === 401) {
          needsLoginState.markNeedsLogin();
        }
        return;
      }
      setEnabled(true);
    } finally {
      setBusy(false);
    }
  }

  if (!supported) {
    return null;
  }

  return (
    <>
      <Button onClick={enable} disabled={busy || enabled} variant={enabled ? 'light' : 'filled'}>
        {enabled ? 'Notifications enabled' : 'Enable notifications'}
      </Button>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to enable notifications.
      </LoginPromptModal>
    </>
  );
}
```

`applicationServerKey` above is passed as the raw base64url VAPID public key string from the API response — confirm during this task whether the real Push API on the target browsers needs this pre-converted to a `Uint8Array` (some implementations require it; `PushManager.subscribe`'s spec technically accepts either a `BufferSource` or, per newer browser support, a base64url string directly). If a `Uint8Array` conversion proves necessary during Task 10's manual verification, add a small `urlBase64ToUint8Array` helper here at that point — do not add unverified conversion code speculatively now.

- [ ] **Step 4: Run the tests**

Run (from `frontend/`): `npm test -- NotificationsToggle`
Expected: PASS.

- [ ] **Step 5: Add the control to the home page**

In `frontend/app/page.tsx`, import and render `<NotificationsToggle />` near the existing `<Title order={1}>Distant Signal</Title>` header (around line 107 as currently structured — read the file first to place it in the same header `Group`/`Stack` the title already lives in, rather than as a new standalone section, matching Decision 6's "direct extension of the existing UI" framing).

- [ ] **Step 6: Check and extend `page.test.tsx`**

Read `frontend/app/page.test.tsx` first. If it mocks child components wholesale (matching this repo's established convention for page-composition tests, e.g. `TrendsResults.test.tsx`'s own approach), add a mock for `NotificationsToggle` (`vi.mock('@/components/NotificationsToggle', () => ({ NotificationsToggle: () => <div data-testid="notifications-toggle" /> }))`) so the page test doesn't need real `navigator.serviceWorker`/`PushManager` globals. Add one assertion that the mocked toggle renders on the page.

- [ ] **Step 7: Run the full frontend suite**

Run (from `frontend/`): `npm test && npm run build`
Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/NotificationsToggle.tsx frontend/components/NotificationsToggle.test.tsx frontend/app/page.tsx frontend/app/page.test.tsx
git commit -m "Add the global 'Enable notifications' Tier-2 control to the home page"
```

---

### Task 9: Service-worker `push`/`notificationclick` handlers — BLOCKED on the sibling SW implementation

**Files:**
- Modify: `<the sibling service-worker file — path not yet known>` (most likely `frontend/public/sw.js`, per this plan's Status note and the sibling spec's own file-convention framing, but confirm the actual path against whatever the sibling plan/implementation actually produced before starting this task)

**Interfaces:**
- Consumes: the `{ title, body, url, tag }` payload this plan's Task 5 (`send::NotificationPayload`) sends.
- **Depends on:** the sibling `docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md`'s own plan **and implementation** landing on `main` first (Global Constraints). **Do not start this task until a real service-worker file exists in this worktree/on `main`.**

**Before starting:** confirm the sibling implementation has landed — check `docs/superpowers/plans/` for a `*pwa-service-worker*` plan file and confirm it's marked complete, and confirm a real service-worker file now exists (`find frontend -iname "sw.js" -o -iname "service-worker*"` or equivalent). If neither is true yet, stop here and re-check later — do not invent a service-worker file to unblock this task, and do not skip ahead to Task 10 either (it also depends on this task).

- [ ] **Step 1: Locate the real service-worker file and its existing structure**

Read the sibling implementation's service-worker file in full. Note its existing `addEventListener` registrations (`install`, `activate`, `fetch`, etc.) and its general code style/module structure, so this task's additions match rather than clash stylistically.

- [ ] **Step 2: Add the `push` handler**

Add, without modifying any of the file's existing listeners:

```js
self.addEventListener('push', (event) => {
  if (!event.data) {
    return;
  }
  const { title, body, url, tag } = event.data.json();

  event.waitUntil(
    (async () => {
      // Skip showing a notification if a focused client already has this
      // exact URL open -- AutoRefresh already covers that case within its
      // existing 30s poll, so a push notification on top of an
      // already-visible, already-fresh tab would be redundant.
      const clientList = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
      const alreadyFocused = clientList.some((client) => client.focused && new URL(client.url).pathname === url);
      if (alreadyFocused) {
        return;
      }
      await self.registration.showNotification(title, { body, tag, data: { url } });
    })(),
  );
});
```

- [ ] **Step 3: Add the `notificationclick` handler**

```js
self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  event.waitUntil(self.clients.openWindow(event.notification.data.url));
});
```

- [ ] **Step 4: Verify no clash with the sibling's existing listeners**

Confirm the sibling implementation does not already register its own `push` or `notificationclick` listener (it shouldn't — those spec's own "Explicitly out of scope" for the SW spec should exclude push, per this plan's Global Constraints framing of the contract as this plan's own request — but verify directly rather than assuming). If it does, reconcile rather than adding a second, conflicting listener for the same event.

- [ ] **Step 5: Manual verification**

This is not meaningfully unit-testable (per this plan's Status note and the spec's own Testing section) — a jsdom test environment has no real `ServiceWorkerGlobalScope`/`PushEvent`. Defer to Task 10's real-device smoke test for actual verification of this handler.

- [ ] **Step 6: Commit**

```bash
git add <the sibling service-worker file>
git commit -m "Add push/notificationclick handlers for line-status and tracked-train notifications"
```

---

### Task 10: End-to-end manual verification (real device/browser)

**Files:** none — verification only, per the spec's own Testing section ("a manual, real-device smoke test... is the only thing that actually verifies the encryption/VAPID path end-to-end").
**Depends on:** every prior task, including Task 9.

- [ ] **Step 1: Generate real VAPID keys for a local/staging environment**

Run: `openssl ecparam -genkey -name prime256v1 -noout -out vapid_private.pem`, then derive the public key per `web-push`'s own documented process (its README/docs describe converting the PEM to the uncompressed base64url public key format `PushManager.subscribe` expects — follow that exactly, since a malformed public key will make `applicationServerKey` reject at the browser).

- [ ] **Step 2: Bring up the full local stack with real keys**

Set `VAPID_PUBLIC_KEY`/`VAPID_PRIVATE_KEY`/`VAPID_SUBJECT` env vars (or the equivalent Helm `--set` values), bring up `docker-compose.yml` (or the Helm chart in a real cluster) including the new `notifier` service, and run the frontend against it.

- [ ] **Step 3: Subscribe from a real browser**

On a real device/browser (Android Chrome or a desktop browser — no install required, per Decision 1; or an installed iOS PWA if testing that path specifically), log in, click "Enable notifications", grant the permission prompt, and confirm the subscribe POST succeeds (`enabled` state in the UI).

- [ ] **Step 4: Trigger a real or manually-forced severity transition**

Pin a line, then either wait for a real severity change or manually insert a `line_status_history` row at a different `severity_rank` for that line directly in the database (bypassing the aggregator, purely for this test) to force `notifier`'s next cycle to find a candidate. Confirm the OS-level push notification actually appears on the test device within one `notifier` poll interval.

- [ ] **Step 5: Click the notification**

Confirm `notificationclick` (Task 9) opens/focuses the browser to the line's page (`/lines/{id}`).

- [ ] **Step 6: Repeat for a tracked train**

Track a train, then manually insert/update a `train_current_state` row (or a real `train_movement_events` row via the trust-consumer ingest path, if a real TRUST feed connection is available for testing) to force a `cancelled` transition or a delay crossing 15 minutes. Confirm the notification appears with the expected copy and that clicking it opens `/track/{id}`.

- [ ] **Step 7: Verify the 404/410 self-cleanup path**

Revoke the browser's notification permission (or uninstall the PWA, for the iOS case) so the push service starts returning `410 Gone`. Force another transition. Confirm the `push_subscriptions` row is deleted (query the database directly) rather than the notifier repeatedly failing on it.

- [ ] **Step 8: Record the outcome**

No commit is produced by this task. If any step surfaces an unexpected result, do not silently patch it here — re-open the relevant earlier task (per superpowers:systematic-debugging) and fix it there, then re-run this task's steps from the start.

---

## Not in this plan

Carried forward from the spec's own "Explicitly out of scope," not silently dropped:

- **The service worker's own registration, scope, caching strategy, and lifecycle** — entirely the sibling plan's responsibility; Task 9 only adds the two event listeners this feature's contract requires.
- **"An incident affecting a tracked train"** in the literal route-matching sense (Decision 4) — no train-shaped `matcher.rs` counterpart exists or is built here; tracked-train notifications cover only the train's own resolved `status`/`delay_minutes`.
- **Per-line notification muting/preferences finer than "pinned or not"** (Decision 5) — no `notification_preferences` table.
- **Email or any non-push delivery channel.**
- **In-tab toast notifications** built from client-side diffing of `AutoRefresh`'s polled data — the SW's focused-client check (Task 9, Step 2) is the only "don't double-notify an open tab" logic this plan adds.
- **Retry queues / dead-letter handling** beyond Task 5's bounded 3-attempt retry.
- **Multi-tenant rate-limit tuning, A/B-testing cooldown windows, or configurability of the 20-minute cooldown / 15-minute delay threshold** beyond the single static config values Task 3's `Config` struct exposes.
- **A dedicated `/settings` page** — Task 8 places the one new control on the existing home page.
- **A Postgres `LISTEN`/`NOTIFY`-based trigger mechanism** — the watermark-poll design (Tasks 3-4) is deliberately simpler; revisit only if `notifier`'s poll-interval latency ever becomes a real, observed problem.
- **A `Uint8Array` conversion helper for `applicationServerKey`** unless Task 10's manual verification shows the target browsers actually require it (Task 8, Step 3's note) — not added speculatively.
