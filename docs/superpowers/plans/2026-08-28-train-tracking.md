# Individual Train Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Execution-order dependency with another in-flight plan:** This plan's
> Task 1 migration (`crates/api/migrations/20260828120000_train_tracking.sql`)
> creates `tracked_trains` with a `user_id TEXT NOT NULL REFERENCES users(id)`
> column (added by the ownership-coordination pass described below) — that
> FK target, the `users` table, does not exist until
> `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 1
> migration (`crates/api/migrations/20260828090000_user_accounts.sql`) has
> run. **That plan's Task 1 must apply before this plan's Task 1.** The
> timestamp prefixes already sort correctly (`20260828090000` <
> `20260828120000`), so running both migration sets in filename order is
> sufficient — this note exists so neither plan is executed out of order by
> two workers unaware of the other. Tasks 1, 3, and 5 below carry
> pointers back to this note at the exact points ownership changed
> anything from this plan's original (pre-coordination) shape. See that
> plan's own matching note at its top.

**Goal:** Let a user pin a specific `(train_uid, service_date)` for tracking, ingest Network Rail's TRUST movement feed (via RDM's Kafka Train Movements product) filtered to exactly the currently-tracked trains, persist an immutable event log plus a denormalized current-state row per tracked train, and serve that state — blended with a best-effort Darwin/LDBWS ETA where available — through two new public API routes.

**Architecture:** A new long-running crate, `crates/trust-consumer`, holds a persistent Kafka consumer-group connection to RDM's Train Movements topic — the first service in this codebase that is not a cron-style poller. It resolves each pinned train's identity from TRUST's Activation message, appends Movement/Cancellation/Change-of-Origin/Change-of-Identity events for trains it has resolved, derives position-in-journey and a naive propagated ETA, and writes through to `crates/api` over the same `X-Internal-Token`-gated ingest pattern every existing poller uses. `crates/api` gains two new tables (`tracked_trains`, `train_movement_events`) plus a denormalized `train_current_state` table, a public pin-creation/read route pair, and a private ingest route for `trust-consumer`. `crates/aggregator` is **not** touched — per-train journeys are a different read/write shape than its per-line `aggregate()` loop, and the design doc is explicit that this feature must not be folded into it.

**Tech Stack:** Rust/sqlx/axum/tokio (`crates/api`, new `crates/trust-consumer`), PostgreSQL, `rdkafka` (new dependency, SASL/SSL Kafka consumer client), `async-trait` (new dependency, for a `dyn`-compatible feed abstraction), `reqwest` for the write-through HTTP calls, `chrono`/`chrono-tz` for local-time schedule-window handling (reused from `crates/enricher`'s existing pattern).

**Spec:** `docs/superpowers/specs/2026-08-28-train-tracking-design.md` — read in full before starting; this plan does not restate its research, only its resulting decisions.

## Prerequisites this plan assumes exist before Task 1 can be *run*

This plan can be written, reviewed, and every task's code can be written and unit-tested (via the fake feed described in Task 9) without any of the following. But Task 14's Kafka-integration step, and any real deployment, are blocked until a human has completed, out of band:

1. **RDM Train Movements product subscription approved** for this app's RDM account. The design doc found no SLA for approval lag — start this early, it gates everything downstream of Task 14.
2. **Cost tier confirmed** directly in the RDM catalogue (requires a logged-in account) before committing to always-on ingestion.
3. **Network Rail Infrastructure Limited licence sign-off**, separate and distinct from the existing NRE Ts&Cs review that covers this app's four existing RDM feeds. Must confirm the current wording of the no-logo/no-"official" clause before Task 17's attribution line ships.
4. **Exact Kafka connection details from the live RDM catalogue entry**: broker hostname(s), topic name, consumer-group semantics, and SASL mechanism (PLAIN vs SCRAM) for the Train Movements product. None of this was confirmed in the design doc's research pass.

Every one of these is modeled the same way this codebase already models `poller-tocs`/`poller-incidents`' unconfirmed RDM endpoint paths: a required, no-default, env-only config value with a doc comment stating the gap, so a missing value fails loudly at startup instead of silently misconfiguring (see Task 7).

## Global Constraints

- **Config is env-only via `clap`'s `env` feature**, matching every existing crate (`crates/poller-ldbws/src/config.rs`). No config files, no hardcoded defaults for anything that depends on prerequisite 4 above.
- **Migration ordering.** Timestamp-prefixed SQL under `crates/api/migrations/`; the next one must sort after the existing `20260822120000_line_status_source.sql`. This plan's migration is `20260828120000_train_tracking.sql`.
- **Cross-plan dependency: `users` must exist first.** `tracked_trains` is
  born with a `NOT NULL` owner (see Task 1) rather than getting the same
  nullable-retrofit treatment `custom_lines`/`pinned_lines`/
  `pinned_stations` needed elsewhere in this codebase — those three tables
  predate any user model and have real rows in a live schema already;
  `tracked_trains` has neither, as of this writing, so there's no
  retrofit question for it. That means this plan's Task 1 migration has a
  hard dependency on `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s
  Task 1 migration (`20260828090000_user_accounts.sql`, creating `users`)
  having already run — see the note at the top of this document.
- **`X-Internal-Token` auth gate.** Every `trust-consumer` → `api` call (the private ingest route and the pending-pins reference read) goes through the same shared-secret header every poller uses (`crates/api/src/auth.rs`'s `require_internal_token`, mounted on `private_router()`). The two new public *read* routes (Task 5) are **not** behind this gate and stay fully unauthenticated/unscoped — see Task 5's own note on why (a deliberate shareable-link posture, not a leftover from before accounts existed). Pin *creation* (Task 3) differs: per the account-system design doc's coordination fix to this plan (see the note at the top of this document), `POST /Train/track` requires a resolved session (`AuthenticatedUser`, from `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 6) and stamps the new row's `user_id` from it — it is still mounted under `public_router()` (not `private_router()`; `X-Internal-Token` is for machine-to-machine poller/consumer calls, not browser sessions), just no longer anonymous within that router. `/public/preferences`'s own doc comment, cited in this bullet in an earlier draft of this plan, is stale for the same reason as of that account-system plan's Task 10 — it is also now session-gated, not a still-valid "unauthenticated user-facing action" precedent.
- **The tracked-train filter is exactly one thing, and no task may broaden it.** A TRUST message is stored if and only if its `train_id` resolves to a currently-tracked `(train_uid, date)` — full stop. No task in this plan may reintroduce the curated line/station catalogue (`dedup_sample_stations`, `crates/api/src/data/samples.rs`) as a secondary allowlist; the design doc explicitly rejected that as "silently track every train touching a curated line." The *ingest-side read* volume is still the full national feed (Kafka has no server-side content filter) — only the *write/storage* volume is narrowed.
- **Crate naming: `trust-consumer`, not `poller-trust`.** The design doc flags this as a real, deliberately-unresolved tension. This plan resolves it: `poller-*` names a specific shape this feature does not have — a stateless cron loop making periodic HTTP GETs. `trust-consumer` (a) names the data source (TRUST) the way `poller-ldbws`/`poller-tocs` name theirs, (b) uses "consumer" deliberately, matching the Kafka consumer-group terminology the whole feed is built on, and (c) reads visibly differently from the `poller-*` family so nobody mistakes it for one at a glance — signaling the operational difference the design doc calls out (reconnect/backoff, offset management, a different health-check shape) rather than hiding it behind a familiar prefix out of habit.
- **No CIF SCHEDULE ingestion in this pass.** The design doc's goal 2 describes seeding a tracked train's full scheduled calling-point list from CIF "independent of whether TRUST has activated the train yet." This app has no CIF integration today — DESIGN.md §3 lists CIF SCHEDULE as "optional, post-v1... Not required for v1," and pulling it in is a data-source addition this plan does not take on. **This plan narrows goal 2 accordingly**: a tracked train's calling-point list is built up *incrementally*, one entry per Movement event actually observed on the feed, not pre-seeded. Practical effect: "next scheduled calling point" is only ever "the next one TRUST has told us about a train reaching," never a full advance itinerary. This is an honest v1 limitation, not a bug — a future CIF-ingestion pass (its own design/plan) is the natural way to close it, exactly as the design doc's Non-goals anticipate for TD.
- **Darwin↔TRUST correlation lives in `crates/api`, not `trust-consumer`.** The design doc leaves this as an implementation-level choice. This plan puts the best-effort `(date, station CRS, scheduled time)` blend at **read time**, inside the `GET /Train/...` handler (Task 6) — not baked into `trust-consumer`'s write path. Reasoning: it keeps `trust-consumer` free of a dependency on `station_samples` (owned by `poller-ldbws`/`crates/api`, a different write path entirely), keeps the heuristic swappable/improvable without touching the Kafka consumer, and matches this codebase's existing preference for keeping enrichment steps as thin, independently-replaceable layers over a materialized base (the same shape `crates/aggregator`'s `apply_extraction` took over the incident pipeline).
- **Idempotency.** RDM Kafka delivery is at-least-once. Every `train_movement_events` row carries a `dedup_key` (computed by `trust-consumer`, see Task 13) under a `UNIQUE (tracked_train_id, dedup_key)` constraint; the write path is `INSERT ... ON CONFLICT (tracked_train_id, dedup_key) DO NOTHING`. A message replayed after a consumer restart must never appear twice in the event log or double-apply to `train_current_state`.
- **Unconfirmed TRUST message types (`0005`, `0008`).** Per this codebase's "no invented API details" convention, `trust-consumer`'s schema parser (Task 8) treats any `msg_type` outside the five confirmed types as an `Unknown(String)` variant, logged at `debug` and dropped — never a hard parse error, never guessed at.
- **Retention.** `train_movement_events` rows older than a configurable window (default 90 days, `RETENTION_DAYS`) are pruned periodically by `trust-consumer`, mirroring `crates/aggregator`'s existing `history_retention_days`/`prune_removed_lines` pattern. `tracked_trains` and `train_current_state` rows are kept indefinitely — cheap, and per the design doc, the more useful long-term record.
- **`dataQuality`-style provenance stays explicit, not collapsed.** Any ETA field this feature emits carries an `eta_source` of `trust-propagated` or `darwin-estimated` — never a single unlabeled number — extending DESIGN.md §5.5's existing philosophy (`knowledgebase`/`planned`/`ldbws-inferred`/reserved `trust-inferred`) rather than inventing a parallel one.
- **`crates/aggregator` is not modified by any task in this plan.** No task may add train-tracking logic to `aggregate()`, `LoadedIncident`, or any file under `crates/aggregator/`.
- New dependencies introduced by this plan: `rdkafka` (in `trust-consumer`; SASL/SSL Kafka client), `async-trait` (in `trust-consumer`; makes the feed abstraction `dyn`-compatible), `axum` (in `trust-consumer`; the health endpoint — Task 7).
- Internal wire types shared between `trust-consumer` and `crates/api` (`crates/common`) use plain `snake_case` field names, matching `IncidentMessage`/`StationSample`. Public-facing JSON returned by the two new `/Train/...` routes uses `#[serde(rename_all = "camelCase")]`, matching `PreferencesResponse`/`CustomLineDetail`.

---

### Task 1: Database schema migration

**Files:**
- Create: `crates/api/migrations/20260828120000_train_tracking.sql`

**Interfaces:**
- Produces: `tracked_trains`, `train_movement_events`, `train_current_state` tables. Consumed by Task 3 (writes to `tracked_trains`), Task 4 (writes to all three), Task 5 (reads all three), Task 6 (reads `train_current_state`).
- **Depends on:** the `users` table from
  `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 1
  (`crates/api/migrations/20260828090000_user_accounts.sql`) —
  `tracked_trains.user_id` references it. That migration must be applied
  first; see the note at the top of this document.

- [ ] **Step 1: Write the migration**

Create `crates/api/migrations/20260828120000_train_tracking.sql`:

```sql
-- -------------------------------------------------------------------------
-- Individual train tracking: user-pinned (train_uid, date) journeys,
-- sourced from Network Rail's TRUST movement feed via trust-consumer.
-- See docs/superpowers/specs/2026-08-28-train-tracking-design.md and
-- docs/superpowers/plans/2026-08-28-train-tracking.md.
--
-- IMPORTANT: this migration depends on the `users` table from
-- docs/superpowers/plans/2026-08-28-user-accounts-sso.md's Task 1
-- (crates/api/migrations/20260828090000_user_accounts.sql), which must
-- apply first -- see the note at the top of
-- docs/superpowers/plans/2026-08-28-train-tracking.md. Unlike
-- custom_lines/pinned_lines/pinned_stations (which predate any user model
-- and needed a nullable-or-truncate ownership retrofit once accounts were
-- added), tracked_trains has no migration applied anywhere as of the
-- account-system design's writing, so it ships with a NOT NULL owner from
-- birth -- see that design doc's Data model section.
--
-- Three tables, mirroring the design doc's "What gets stored":
--   tracked_trains       -- one row per pin, owned by the user who created
--                           it (see user_id below). Starts 'pending'
--                           (train_uid unknown -- all we have is what the
--                           user was looking at on a departure board) and
--                           moves to 'resolved' once trust-consumer binds
--                           it to a TRUST Activation, or 'unresolved' if no
--                           Activation is ever matched to it.
--   train_movement_events -- immutable, append-only event log. One row per
--                           TRUST message matched to a resolved tracked
--                           train. dedup_key + the UNIQUE constraint below
--                           is what makes at-least-once Kafka delivery safe
--                           to write blindly (INSERT ... ON CONFLICT DO
--                           NOTHING).
--   train_current_state   -- denormalized "where is it right now" row,
--                           mirroring line_status being a materialized
--                           table the aggregator writes rather than
--                           something recomputed per request (DESIGN.md
--                           §4). One row per tracked_trains row, upserted
--                           on every event.
-- -------------------------------------------------------------------------

CREATE TABLE tracked_trains (
    id BIGSERIAL PRIMARY KEY,

    -- The user who created this pin (see Task 3). NOT NULL from birth --
    -- unlike custom_lines/pinned_lines/pinned_stations, this table has no
    -- pre-existing unowned rows to accommodate. Reads stay public (Task
    -- 5's GET routes are unscoped, matching a shareable-tracking-link
    -- posture -- see that task's note); only creation is owned.
    -- TEXT, not BIGINT/UUID: docs/superpowers/plans/2026-08-28-user-accounts-sso.md's
    -- Task 1 defines users.id as the bare OIDC `sub` claim stored verbatim
    -- (a TEXT primary key, matching this schema's existing natural-key
    -- convention -- incidents.incident_id, custom_lines.id, stations.crs).
    -- Keep this column's type in sync with that table if it ever changes.
    user_id TEXT NOT NULL REFERENCES users(id),

    -- Pin-time criteria: what the user was actually looking at. origin_crs
    -- + scheduled_departure + service_date is the best-effort key
    -- trust-consumer resolves against incoming Activation/Movement
    -- messages (see Task 10) -- there is no CIF lookup available to do
    -- this exactly, per this plan's Global Constraints.
    service_date        DATE NOT NULL,
    pin_origin_crs       TEXT NOT NULL,
    pin_scheduled_departure TIMESTAMPTZ NOT NULL,
    pin_destination_crs  TEXT,
    pin_operator         TEXT,

    -- Populated once resolved.
    train_uid  TEXT,
    train_id   TEXT,  -- TRUST's own daily identifier, the join key for
                       -- every subsequent message on this train.

    resolution_status TEXT NOT NULL DEFAULT 'pending'
        CHECK (resolution_status IN ('pending', 'resolved', 'unresolved')),

    tracked_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    resolved_at TIMESTAMPTZ
);

CREATE INDEX tracked_trains_user_id ON tracked_trains (user_id);

-- Only meaningful once resolved -- a resolved (train_uid, service_date)
-- pair must be unique (re-pinning an already-tracked train should not
-- create a second parallel event log for it). Multiple *pending* rows with
-- NULL train_uid are fine and expected; Postgres treats NULLs as distinct
-- for uniqueness purposes, so this constraint doesn't block them.
CREATE UNIQUE INDEX tracked_trains_resolved_identity
    ON tracked_trains (train_uid, service_date)
    WHERE train_uid IS NOT NULL;

-- trust-consumer's reference-reload query (Task 4) filters on this.
CREATE INDEX tracked_trains_resolution_status ON tracked_trains (resolution_status);

CREATE TABLE train_movement_events (
    id BIGSERIAL PRIMARY KEY,
    tracked_train_id BIGINT NOT NULL REFERENCES tracked_trains(id) ON DELETE CASCADE,

    dedup_key TEXT NOT NULL,  -- see Task 13 -- stable across redelivery of
                              -- the same real-world TRUST message.
    msg_type   TEXT NOT NULL, -- '0001'..'0007' (never '0005'/'0008' --
                              -- unconfirmed types are dropped before this
                              -- table, see Task 8).
    event_type TEXT,          -- ARRIVAL / DEPARTURE / PASS, Movement only.
    loc_stanox TEXT,
    loc_crs    TEXT,          -- best-effort STANOX->CRS translation; NULL
                              -- if untranslatable.
    planned_timestamp TIMESTAMPTZ,
    actual_timestamp  TIMESTAMPTZ,
    variation_status  TEXT,
    raw_body   JSONB NOT NULL, -- full message body, verbatim, for anything
                              -- this schema doesn't model explicitly.

    received_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    UNIQUE (tracked_train_id, dedup_key)
);

CREATE INDEX train_movement_events_tracked_train
    ON train_movement_events (tracked_train_id, received_at);

CREATE TABLE train_current_state (
    tracked_train_id BIGINT PRIMARY KEY REFERENCES tracked_trains(id) ON DELETE CASCADE,

    status TEXT NOT NULL DEFAULT 'awaiting_activation'
        CHECK (status IN ('awaiting_activation', 'en_route', 'cancelled', 'completed')),

    last_reported_location TEXT,
    last_event_type        TEXT,
    delay_minutes           INTEGER,
    next_calling_point      TEXT,

    -- trust-propagated (naive forward delay propagation, always available
    -- once en route) vs darwin-estimated (blended in at read time by
    -- crates/api -- see Task 6). Never both at once: this column reflects
    -- what trust-consumer itself last computed, which Task 6 may override
    -- transiently in its response without writing back here.
    eta_next   TIMESTAMPTZ,
    eta_source TEXT CHECK (eta_source IN ('trust-propagated', 'darwin-estimated')),

    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
```

- [ ] **Step 2: Run the API crate test suite to confirm nothing broke**

Run: `cargo test -p api`
Expected: PASS — three new tables, no existing query touches them.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260828120000_train_tracking.sql
git commit -m "Add tracked_trains, train_movement_events, train_current_state tables"
```

---

### Task 2: `common` wire types

**Files:**
- Modify: `crates/common/src/lib.rs`

**Interfaces:**
- Produces: `TrackPinRequest` (pin-creation payload), `TrainMovementEventMessage` (trust-consumer → api ingest payload). Consumed by Task 3 (`TrackPinRequest`), Task 4 (`TrainMovementEventMessage`, both sides), Task 14 (`trust-consumer` constructs `TrainMovementEventMessage`).

- [ ] **Step 1: Add the two structs**

Add to `crates/common/src/lib.rs`, near `StationSample`/`IncidentMessage`:

```rust
/// Pin-creation payload for `POST /Train/track` (`crates/api/src/routes/train.rs`).
/// Deliberately does NOT include `train_uid` -- per the design doc's
/// Tracking semantics, the pinned service is only ever known by what a
/// departure-board view already has (RDM's ephemeral `serviceID`-adjacent
/// fields), never by a durable train identity at pin time. Resolution to
/// `(train_uid, service_date)` happens later, out of band, once
/// trust-consumer observes a matching TRUST Activation (see
/// docs/superpowers/plans/2026-08-28-train-tracking.md Task 10).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackPinRequest {
    pub service_date: chrono::NaiveDate,
    pub origin_crs: String,
    pub scheduled_departure: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_crs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
}

/// One TRUST-derived event for a tracked train, as `trust-consumer` posts
/// it to `POST /private/train-events`. Carries both the raw event (for the
/// immutable log, `train_movement_events`) and trust-consumer's own
/// derived current-state fields (for `train_current_state`) in the same
/// message -- denormalize-on-write, per this plan's Global Constraints.
///
/// `resolved_train_uid`/`resolved_train_id` are only `Some` on the one
/// message that resolves a pending pin (i.e. the Activation-derived
/// event); every subsequent event for the same tracked train carries them
/// as `None`, since the binding doesn't change again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainMovementEventMessage {
    pub tracked_train_id: i64,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_train_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_train_id: Option<String>,

    pub dedup_key: String,
    pub msg_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc_stanox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loc_crs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planned_timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_timestamp: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variation_status: Option<String>,
    pub raw_body: serde_json::Value,

    // Derived current-state fields, computed by trust-consumer (Tasks
    // 11-12) and written straight through to train_current_state.
    pub status: String, // "awaiting_activation" | "en_route" | "cancelled" | "completed"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reported_location: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_minutes: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_calling_point: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_next: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_source: Option<String>, // "trust-propagated", set by trust-consumer.
                                    // "darwin-estimated" is only ever
                                    // produced at read time (Task 6), never
                                    // written back by trust-consumer.
}
```

- [ ] **Step 2: Confirm the workspace still builds**

Run: `cargo build --workspace`
Expected: PASS — additive, unused by anything yet.

- [ ] **Step 3: Commit**

```bash
git add crates/common/src/lib.rs
git commit -m "Add TrackPinRequest and TrainMovementEventMessage wire types"
```

---

### Task 3: Pin creation — `POST /Train/track`

**Files:**
- Create: `crates/api/src/data/train_tracking.rs`
- Create: `crates/api/src/routes/train.rs`
- Modify: `crates/api/src/data/mod.rs` (add `pub mod train_tracking;`)
- Modify: `crates/api/src/routes/mod.rs` (mount `train::router()` in `public_router()`)

**Interfaces:**
- Produces: `POST /Train/track` (requires a resolved session — see below).
  `fn validate_pin(pin: &TrackPinRequest) -> Result<(), String>` (pure,
  unit-tested). `async fn create_pin(pool, pin, user_id: &str) -> Result<i64>`.
  Consumed by frontend (out of scope here) and Task 5's read routes (same
  `tracked_trains` rows).
- **Depends on:** `crate::auth::AuthenticatedUser`, the session extractor
  from `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 6
  (`crates/api/src/auth.rs`). That plan's auth routes/session middleware
  must be implemented before this task can compile and actually gate
  anything — see the note at the top of this document. Unlike
  `custom_lines`/`preferences` (public product content plus a pre-existing
  public-by-design posture the account-system design explicitly narrows),
  `tracked_trains` has no unauthenticated precedent to preserve: this
  route was never shipped anonymous, so requiring a session here from the
  start is a pure addition, not a breaking behavior change to anything
  live.

- [ ] **Step 1: Write the failing test for `validate_pin`**

Create `crates/api/src/data/train_tracking.rs`:

```rust
//! Tracked-train pin creation and lookup. Query functions are kept thin
//! (see `crates/api/src/data/queries.rs`'s module docs for why this crate
//! prefers runtime-checked `sqlx::query` over the `query!` macro family);
//! `validate_pin` is factored out so the one piece of actual logic here is
//! testable without a database.

use chrono::{DateTime, Utc};
use common::TrackPinRequest;
use sqlx::PgPool;

/// A pin more than this far in the past is almost certainly a stale
/// frontend view (the user was looking at a departure board snapshot from
/// much earlier) rather than a real tracking request -- reject it rather
/// than create a `tracked_trains` row trust-consumer can never resolve
/// (TRUST's Train Movements feed is a live stream, not a historical
/// lookup; a pin for a service that ran days ago will sit 'pending'
/// forever). A pin arbitrarily far in the future is fine -- "track before
/// it even starts running" is an explicit design goal.
const MAX_PIN_AGE: chrono::Duration = chrono::Duration::hours(6);

pub fn validate_pin(pin: &TrackPinRequest, now: DateTime<Utc>) -> Result<(), String> {
    if pin.origin_crs.trim().is_empty() {
        return Err("origin_crs must not be empty".to_string());
    }
    if pin.origin_crs.len() != 3 {
        return Err("origin_crs must be a 3-letter CRS code".to_string());
    }
    if now - pin.scheduled_departure > MAX_PIN_AGE {
        return Err("scheduled_departure is too far in the past to track".to_string());
    }
    Ok(())
}

/// `user_id` is the authenticated caller's id (the OIDC `sub`, per
/// `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 1) --
/// resolved by the route handler's `AuthenticatedUser` extractor
/// (`crates/api/src/routes/train.rs::post_track`, below), never taken from
/// the request body itself.
pub async fn create_pin(pool: &PgPool, pin: &TrackPinRequest, user_id: &str) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO tracked_trains \
            (user_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs, pin_operator) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id",
    )
    .bind(user_id)
    .bind(pin.service_date)
    .bind(&pin.origin_crs)
    .bind(pin.scheduled_departure)
    .bind(&pin.destination_crs)
    .bind(&pin.operator)
    .fetch_one(pool)
    .await?;

    Ok(row.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(origin_crs: &str, scheduled_departure: DateTime<Utc>) -> TrackPinRequest {
        TrackPinRequest {
            service_date: scheduled_departure.date_naive(),
            origin_crs: origin_crs.to_string(),
            scheduled_departure,
            destination_crs: None,
            operator: None,
        }
    }

    #[test]
    fn a_well_formed_near_term_pin_is_valid() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let departure: DateTime<Utc> = "2026-06-15T13:00:00Z".parse().unwrap();
        assert!(validate_pin(&pin("WAT", departure), now).is_ok());
    }

    #[test]
    fn a_future_pin_is_valid() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let departure: DateTime<Utc> = "2026-06-20T18:32:00Z".parse().unwrap();
        assert!(validate_pin(&pin("WAT", departure), now).is_ok());
    }

    #[test]
    fn an_empty_origin_crs_is_rejected() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        assert!(validate_pin(&pin("", now), now).is_err());
    }

    #[test]
    fn a_non_three_letter_crs_is_rejected() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        assert!(validate_pin(&pin("WATERLOO", now), now).is_err());
    }

    #[test]
    fn a_stale_departure_is_rejected() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let departure: DateTime<Utc> = "2026-06-15T02:00:00Z".parse().unwrap(); // 10h ago
        assert!(validate_pin(&pin("WAT", departure), now).is_err());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p api validate_pin`
Expected: PASS — implementation and tests were written together (same posture the reference incident-extraction plan's `text_hash`/`combine` steps used for atomic pure-function additions).

- [ ] **Step 3: Wire `train_tracking` into `data/mod.rs`**

In `crates/api/src/data/mod.rs`, add `pub mod train_tracking;` alongside the existing `pub mod custom_lines;`/`pub mod preferences;`.

- [ ] **Step 4: Write the route**

Create `crates/api/src/routes/train.rs`:

```rust
//! `/Train/...`: individual train tracking. Pin *creation* requires an
//! authenticated session (`AuthenticatedUser`, from
//! docs/superpowers/plans/2026-08-28-user-accounts-sso.md's Task 6) --
//! every tracked train has a real owner from birth, per that plan's
//! coordination fix to this one. State *reads* (Task 5) stay
//! unauthenticated/unscoped -- see that task's note on why this isn't a
//! strict "everything private" posture. Mounted directly (not under
//! `/public`) to match the design doc's sketched URL shape for the
//! eventual frontend page.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use common::TrackPinRequest;
use serde::Serialize;

use crate::app::{App, Router};
use crate::auth::AuthenticatedUser;
use crate::data::train_tracking;

pub fn router() -> Router {
    Router::new().route("/Train/track", axum::routing::post(post_track))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackPinResponse {
    tracking_id: i64,
    resolution_status: &'static str,
}

async fn post_track(
    State(app): State<App>,
    user: AuthenticatedUser,
    Json(pin): Json<TrackPinRequest>,
) -> Result<Json<TrackPinResponse>, (StatusCode, String)> {
    train_tracking::validate_pin(&pin, Utc::now()).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let tracking_id = train_tracking::create_pin(&app.database, &pin, &user.id)
        .await
        .map_err(internal_error)?;

    Ok(Json(TrackPinResponse { tracking_id, resolution_status: "pending" }))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "failed to create train tracking pin");
    (StatusCode::INTERNAL_SERVER_ERROR, "failed to create tracking pin".to_string())
}
```

- [ ] **Step 5: Mount it**

In `crates/api/src/routes/mod.rs`, add `pub mod train;` and add `.merge(train::router())` to `public_router()`'s builder chain, alongside the existing `.merge(preferences::router())`.

- [ ] **Step 6: Confirm the workspace builds and the full API test suite passes**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 7: Manually verify against a live dev stack**

Requires a resolved session (`AuthenticatedUser`), same as the
account-system plan's own manual-verification steps — either a real
completed login, or a hand-inserted test `users`/`sessions` row matching
that plan's Task 5 DB-test pattern:

```bash
docker compose --env-file dev.env up --build -d api postgres
psql "$DATABASE_URL" -c "INSERT INTO users (id) VALUES ('TEST-USER') ON CONFLICT DO NOTHING"
psql "$DATABASE_URL" -c "INSERT INTO sessions (id, user_id, expires_at) VALUES ('$(echo -n manual-test-token | sha256sum | cut -d' ' -f1)', 'TEST-USER', NOW() + INTERVAL '1 hour')"

curl -s -X POST http://localhost:8080/Train/track \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"serviceDate":"2026-08-28","originCrs":"WAT","scheduledDeparture":"2026-08-28T18:32:00Z"}'
```

Wait — `TrackPinRequest` uses plain `snake_case` (it's a `common` wire type per this plan's Global Constraints), so the body must use `service_date`/`origin_crs`/`scheduled_departure`:

```bash
curl -s -X POST http://localhost:8080/Train/track \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"service_date":"2026-08-28","origin_crs":"WAT","scheduled_departure":"2026-08-28T18:32:00Z"}'
```

Expected: `{"trackingId":1,"resolutionStatus":"pending"}` (response body itself IS camelCase — only the request body is snake_case, per the split this plan's Global Constraints call out), and `SELECT user_id FROM tracked_trains WHERE id = 1` shows `TEST-USER`. Retry without the `Cookie` header and confirm `401`. Clean up: `psql "$DATABASE_URL" -c "DELETE FROM tracked_trains WHERE id = 1; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"`.

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/data/train_tracking.rs crates/api/src/data/mod.rs crates/api/src/routes/train.rs crates/api/src/routes/mod.rs
git commit -m "Add POST /Train/track pin creation"
```

---

### Task 4: Private ingest — `POST /private/train-events`, `GET /private/tracked-trains`

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`
- Modify: `crates/api/src/routes/ingest.rs`

**Interfaces:**
- Consumes: `TrainMovementEventMessage` (Task 2).
- Produces: `async fn upsert_train_event(pool, event: &TrainMovementEventMessage) -> Result<()>` (idempotent — resolves the pin if `resolved_train_uid` is set, inserts the event row with `ON CONFLICT DO NOTHING`, upserts `train_current_state`). `async fn list_active_tracked_trains(pool) -> Result<Vec<TrackedTrainRef>>` (pending + resolved-but-not-completed rows, for `trust-consumer`'s periodic reference reload — Task 14). Consumed by Task 14.
- **Ownership check (no change needed):** both routes here are mounted
  under `private_router()`, gated by `require_internal_token` — they're
  `trust-consumer`'s own machine-to-machine calls, never a browser
  request, so there's no "which user is asking" question for either
  handler to answer. `upsert_train_event`/`list_active_tracked_trains`
  operate on `tracked_trains` rows by internal id, not by owner, and stay
  that way after the account-system coordination fix — a tracked train's
  `user_id` (Task 1) is set once at creation (Task 3) and never
  re-checked by this task's ingest path.

- [ ] **Step 1: Add `TrackedTrainRef` and the two query functions**

Add to `crates/api/src/data/train_tracking.rs`:

```rust
use common::TrainMovementEventMessage;

/// What `trust-consumer` needs to know about each active tracked train:
/// pending pins to attempt resolving, and already-resolved ones to
/// recognize incoming TRUST messages against, after a restart or on its
/// periodic reload (see Task 14). "Active" excludes `completed`/`cancelled`
/// rows in `train_current_state` and `unresolved` rows in `tracked_trains`
/// -- there is nothing further for trust-consumer to do with either.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TrackedTrainRef {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub train_id: Option<String>,
}

pub async fn list_active_tracked_trains(pool: &PgPool) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let rows = sqlx::query_as::<_, TrackedTrainRef>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, tt.train_id \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         WHERE tt.resolution_status != 'unresolved' \
           AND (cs.status IS NULL OR cs.status NOT IN ('completed', 'cancelled'))",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn upsert_train_event(pool: &PgPool, event: &TrainMovementEventMessage) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    if let (Some(train_uid), Some(train_id)) = (&event.resolved_train_uid, &event.resolved_train_id) {
        sqlx::query(
            "UPDATE tracked_trains \
             SET train_uid = $2, train_id = $3, resolution_status = 'resolved', resolved_at = NOW() \
             WHERE id = $1",
        )
        .bind(event.tracked_train_id)
        .bind(train_uid)
        .bind(train_id)
        .execute(&mut *tx)
        .await?;
    }

    sqlx::query(
        "INSERT INTO train_movement_events \
            (tracked_train_id, dedup_key, msg_type, event_type, loc_stanox, loc_crs, \
             planned_timestamp, actual_timestamp, variation_status, raw_body) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (tracked_train_id, dedup_key) DO NOTHING",
    )
    .bind(event.tracked_train_id)
    .bind(&event.dedup_key)
    .bind(&event.msg_type)
    .bind(&event.event_type)
    .bind(&event.loc_stanox)
    .bind(&event.loc_crs)
    .bind(event.planned_timestamp)
    .bind(event.actual_timestamp)
    .bind(&event.variation_status)
    .bind(&event.raw_body)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "INSERT INTO train_current_state \
            (tracked_train_id, status, last_reported_location, last_event_type, \
             delay_minutes, next_calling_point, eta_next, eta_source, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW()) \
         ON CONFLICT (tracked_train_id) DO UPDATE SET \
            status                  = EXCLUDED.status, \
            last_reported_location  = EXCLUDED.last_reported_location, \
            last_event_type         = EXCLUDED.last_event_type, \
            delay_minutes            = EXCLUDED.delay_minutes, \
            next_calling_point       = EXCLUDED.next_calling_point, \
            eta_next                 = EXCLUDED.eta_next, \
            eta_source               = EXCLUDED.eta_source, \
            updated_at               = NOW()",
    )
    .bind(event.tracked_train_id)
    .bind(&event.status)
    .bind(&event.last_reported_location)
    .bind(&event.last_event_type)
    .bind(event.delay_minutes)
    .bind(&event.next_calling_point)
    .bind(event.eta_next)
    .bind(&event.eta_source)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
```

Note: the `train_current_state` upsert always writes on every event, even a Kafka-redelivered duplicate the `train_movement_events` INSERT just silently dropped via `ON CONFLICT DO NOTHING` — writing the same current-state values twice is harmless (idempotent by construction, not merely by dedup), so this doesn't need to be conditioned on whether the event insert actually inserted a row.

- [ ] **Step 2: Add the two routes**

In `crates/api/src/routes/ingest.rs`, add to `router()`:

```rust
        .route("/train-events", axum::routing::post(post_train_events))
        .route("/tracked-trains", axum::routing::get(get_active_tracked_trains))
```

And the handlers:

```rust
async fn post_train_events(
    State(app): State<App>,
    Json(events): Json<Vec<common::TrainMovementEventMessage>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    for event in &events {
        queries_train_tracking::upsert_train_event(&app.database, event)
            .await
            .map_err(internal_error)?;
    }
    Ok(Json(UpsertResponse { upserted: events.len() as u64 }))
}

async fn get_active_tracked_trains(
    State(app): State<App>,
) -> Result<Json<Vec<crate::data::train_tracking::TrackedTrainRef>>, (StatusCode, String)> {
    let rows = crate::data::train_tracking::list_active_tracked_trains(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(rows))
}
```

(Add `use crate::data::train_tracking as queries_train_tracking;` — or just call `crate::data::train_tracking::upsert_train_event` fully qualified as shown in the second handler; either is fine, pick whichever the surrounding file's existing import style favors once you're editing it.) Also add `#[derive(Serialize)]` to `TrackedTrainRef` in `train_tracking.rs` (the ingest route returns it as JSON, unlike Task 3's DB-only reads) — go back and add `Serialize` to its `derive` list now.

- [ ] **Step 3: Confirm the workspace builds and tests pass**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 4: Manually verify with `curl` against a live dev stack**

```bash
docker compose --env-file dev.env up --build -d api postgres
source dev.env
curl -s -X POST http://localhost:8080/Train/track \
  -H "Content-Type: application/json" \
  -d '{"service_date":"2026-08-28","origin_crs":"WAT","scheduled_departure":"2026-08-28T18:32:00Z"}'
# note the returned trackingId, then:
curl -s http://localhost:8080/private/tracked-trains -H "x-internal-token: $INTERNAL_TOKEN"
curl -s -X POST http://localhost:8080/private/train-events \
  -H "x-internal-token: $INTERNAL_TOKEN" -H "Content-Type: application/json" \
  -d '[{"tracked_train_id":1,"resolved_train_uid":"C21373","resolved_train_id":"221832406","dedup_key":"activation-1","msg_type":"0001","raw_body":{},"status":"en_route"}]'
psql "$DATABASE_URL" -c "SELECT resolution_status, train_uid FROM tracked_trains WHERE id = 1"
psql "$DATABASE_URL" -c "SELECT status FROM train_current_state WHERE tracked_train_id = 1"
```

Expected: `tracked_trains.resolution_status` is `resolved`, `train_current_state.status` is `en_route`. Re-POST the same event body and confirm `SELECT count(*) FROM train_movement_events WHERE dedup_key = 'activation-1'` is still `1`, not `2`. Clean up: `psql "$DATABASE_URL" -c "DELETE FROM tracked_trains WHERE id = 1"` (cascades).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/train_tracking.rs crates/api/src/routes/ingest.rs
git commit -m "Add private train-events ingest and active-tracked-trains reference routes"
```

---

### Task 5: Public reads — `GET /Train/{trackingId}`, `GET /Train/by-uid/{uid}/{date}`

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `GET /Train/{trackingId}` and `GET /Train/by-uid/{train_uid}/{date}` (both public). Consumed by Task 6 (wraps the same read with ETA blending) and, eventually, a follow-up frontend (out of scope here).

**Ownership: reads deliberately stay public/unscoped, only creation is
owned.** `tracked_trains` now has a real `user_id` (Task 1), but neither
read route added here checks it, and `TrackedTrainState` (below) never
serializes `user_id` into the response — so nothing about who owns a
tracked train leaks to a caller either way. This is a deliberate choice,
not an oversight: `trackingId` is an opaque, hard-to-guess-in-bulk
(sequential, but not enumerable at any real scale without also brute-
forcing valid ids against a live service) numeric handle, and the natural
v1 product shape for "here's the status of the train I'm tracking" is a
shareable link — the same posture package-tracking numbers or a
calendar's "view this event" link take, not a login-walled personal
inbox. This is a real, considered deviation from a strict "everything
about a tracked train is private to its owner" default, and differs from
the account-system design doc's own reasoning for `custom_lines` (public
because a custom line's *computed status* is genuinely public product
content, feeding the same aggregator as catalogue lines) — here the
justification is narrower and different: nothing product-relevant is
gained by hiding a resolved train's live position from someone who
already has the specific tracking id or `(train_uid, date)` pair, and
gating reads would break the plausible "text a friend the link to your
train" use case for no real privacy benefit, since `train_uid`/
`service_date` are public transit facts, not personal data. If a future
UI need arises (e.g. "my tracked trains" list, requiring enumeration
rather than single-id lookup), *that* listing endpoint should require
`AuthenticatedUser` and scope to `user_id` — the two single-lookup GETs
here are a narrower case that doesn't need it. Revisit this call if the
frontend design pass (out of scope for this plan) surfaces a real
"someone else can page through my activity" concern that opaque
random-lookup ids alone don't already mitigate.

Note on route shape: the design doc's architecture sketch shows `GET /Train/{uid}/{date}` as the primary read path, but its own "Tracking semantics" text is explicit that `train_uid` is *not* known at pin time. This plan resolves that tension (flagged as an open question in the design doc's Open Questions/tension list) by making the tracking-id-keyed route the primary one — it works in every resolution state, including `pending`, which the sketch's uid/date path structurally cannot (there is no uid yet) — and keeping the uid/date path as a secondary convenience lookup for once a tracking has resolved, matching the sketch's illustrative shape without depending on it.

- [ ] **Step 1: Add the read query functions**

Add to `crates/api/src/data/train_tracking.rs`:

```rust
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TrackedTrainState {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub train_id: Option<String>,
    pub status: Option<String>,
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    pub delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
    pub eta_next: Option<DateTime<Utc>>,
    pub eta_source: Option<String>,
}

const TRACKED_TRAIN_STATE_SELECT: &str = "\
    SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
           tt.resolution_status, tt.train_uid, tt.train_id, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source \
    FROM tracked_trains tt \
    LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id";

pub async fn get_by_tracking_id(pool: &PgPool, id: i64) -> anyhow::Result<Option<TrackedTrainState>> {
    let row = sqlx::query_as::<_, TrackedTrainState>(&format!("{TRACKED_TRAIN_STATE_SELECT} WHERE tt.id = $1"))
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn get_by_uid_and_date(
    pool: &PgPool,
    train_uid: &str,
    service_date: chrono::NaiveDate,
) -> anyhow::Result<Option<TrackedTrainState>> {
    let row = sqlx::query_as::<_, TrackedTrainState>(&format!(
        "{TRACKED_TRAIN_STATE_SELECT} WHERE tt.train_uid = $1 AND tt.service_date = $2"
    ))
    .bind(train_uid)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 2: Add the two routes**

In `crates/api/src/routes/train.rs`, extend `router()`:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id))
        .route("/Train/by-uid/{train_uid}/{date}", axum::routing::get(get_by_uid_and_date))
}
```

```rust
use axum::extract::Path;
use chrono::NaiveDate;

async fn get_by_tracking_id(
    State(app): State<App>,
    Path(tracking_id): Path<i64>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error)?;
    state
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "no tracked train with that id".to_string()))
}

async fn get_by_uid_and_date(
    State(app): State<App>,
    Path((train_uid, date)): Path<(String, NaiveDate)>,
) -> Result<Json<train_tracking::TrackedTrainState>, (StatusCode, String)> {
    let state = train_tracking::get_by_uid_and_date(&app.database, &train_uid, date)
        .await
        .map_err(internal_error)?;
    state
        .map(Json)
        .ok_or((StatusCode::NOT_FOUND, "no resolved tracked train for that uid/date".to_string()))
}
```

Add `#[serde(rename_all = "camelCase")]` to `TrackedTrainState`'s derive list in `train_tracking.rs` — it's returned directly as public JSON, unlike `TrackedTrainRef` (private, poller-to-poller shape).

- [ ] **Step 3: Confirm the workspace builds and tests pass**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 4: Manually verify**

```bash
curl -s http://localhost:8080/Train/1
curl -s http://localhost:8080/Train/by-uid/C21373/2026-08-28
```

Expected: first returns the pending/resolved state for tracking id 1 (or 404 if Task 4's manual cleanup already ran — re-create a pin first); second returns 404 until a tracked train with that exact `train_uid`/`service_date` exists.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/train_tracking.rs crates/api/src/routes/train.rs
git commit -m "Add GET /Train/{trackingId} and GET /Train/by-uid/{uid}/{date} reads"
```

---

### Task 6: Best-effort Darwin/LDBWS ETA blending at read time

**Files:**
- Create: `crates/api/src/data/eta_blend.rs`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Consumes: `TrackedTrainState` (Task 5), `station_samples` (existing table, owned by `poller-ldbws`/`crates/api`'s existing ingest path).
- Produces: `fn find_darwin_eta(samples: &[StationDeparture], pin_origin_crs: &str, pin_destination_crs: Option<&str>, next_calling_point: Option<&str>) -> Option<DateTime<Utc>>` (pure, unit-tested — the heuristic correlation). Wired into both `GET /Train/...` handlers: if this returns `Some`, the response's `eta_next`/`eta_source` are overridden to `darwin-estimated` before serializing, without writing back to `train_current_state` (per this plan's Global Constraints — the blend is read-time-only).

- [ ] **Step 1: Write the failing tests**

Create `crates/api/src/data/eta_blend.rs`:

```rust
//! Best-effort Darwin/TRUST correlation, applied at read time only (see
//! this plan's Global Constraints for why it doesn't live in
//! `trust-consumer`). Keyed on `(origin CRS, destination CRS)` matching a
//! currently-sampled `StationDeparture`'s `destination_crs` against the
//! tracked train's pin/next-calling-point -- deliberately NOT a guaranteed
//! join. See docs/superpowers/specs/2026-08-28-train-tracking-design.md's
//! Open Questions #5.

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use common::StationDeparture;

/// Looks for a live `StationDeparture` sampled at `pin_origin_crs` whose
/// `destination_crs` matches either the tracked train's pinned destination
/// or its currently-known next calling point, and returns Darwin's own
/// estimated time for it if that departure isn't cancelled. `estimated` is
/// either `"On time"`, `"Cancelled"`, or an `"HH:MM"` string (see
/// `common::StationDeparture`'s doc comment) -- only the `"HH:MM"` case
/// yields a concrete ETA; `"On time"` has no better estimate to offer than
/// what trust-consumer's own propagation already computed, so this
/// function returns `None` for it rather than fabricating a value from the
/// scheduled time.
pub fn find_darwin_eta(
    samples: &[StationDeparture],
    pin_destination_crs: Option<&str>,
    next_calling_point: Option<&str>,
    service_date: NaiveDate,
) -> Option<DateTime<Utc>> {
    let target_destination = pin_destination_crs.or(next_calling_point)?;

    let matched = samples
        .iter()
        .find(|d| !d.is_cancelled && d.destination_crs.eq_ignore_ascii_case(target_destination))?;

    let time = NaiveTime::parse_from_str(&matched.estimated, "%H:%M").ok()?;
    Utc.from_local_datetime(&service_date.and_time(time)).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn departure(destination_crs: &str, estimated: &str, is_cancelled: bool) -> StationDeparture {
        StationDeparture {
            service_id: "test".to_string(),
            operator: "SW".to_string(),
            destination_crs: destination_crs.to_string(),
            scheduled: "18:32".to_string(),
            estimated: estimated.to_string(),
            is_cancelled,
            delay_minutes: 0,
            cancel_reason: None,
            delay_reason: None,
            headcode: None,
            skipped_stations: vec![],
        }
    }

    #[test]
    fn no_target_destination_means_no_darwin_eta() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        assert_eq!(find_darwin_eta(&[departure("WOK", "18:40", false)], None, None, date), None);
    }

    #[test]
    fn matches_by_pinned_destination_and_parses_hhmm() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("WOK", "18:41", false)];
        let eta = find_darwin_eta(&samples, Some("WOK"), None, date);
        assert_eq!(eta, Some("2026-08-28T18:41:00Z".parse().unwrap()));
    }

    #[test]
    fn falls_back_to_next_calling_point_when_no_pinned_destination() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("SUR", "18:45", false)];
        let eta = find_darwin_eta(&samples, None, Some("SUR"), date);
        assert_eq!(eta, Some("2026-08-28T18:45:00Z".parse().unwrap()));
    }

    #[test]
    fn a_cancelled_departure_never_matches() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("WOK", "18:41", true)];
        assert_eq!(find_darwin_eta(&samples, Some("WOK"), None, date), None);
    }

    #[test]
    fn on_time_yields_no_concrete_eta_to_prefer_over_trust() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("WOK", "On time", false)];
        assert_eq!(find_darwin_eta(&samples, Some("WOK"), None, date), None);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p api find_darwin_eta`
Expected: PASS.

- [ ] **Step 3: Wire `eta_blend` into `data/mod.rs` and the two read routes**

Add `pub mod eta_blend;` to `crates/api/src/data/mod.rs`.

In `crates/api/src/routes/train.rs`, after fetching `state` in both `get_by_tracking_id` and `get_by_uid_and_date` and before wrapping it in `Json`, insert the blend (factor this into one shared helper to avoid duplicating it across both handlers):

```rust
async fn blend_darwin_eta(app: &App, mut state: train_tracking::TrackedTrainState) -> train_tracking::TrackedTrainState {
    let Some(destination) = state.pin_destination_crs.as_deref().or(state.next_calling_point.as_deref()) else {
        return state;
    };
    let Ok(samples) = crate::data::queries::latest_station_sample(&app.database, &state.pin_origin_crs).await else {
        return state;
    };
    if let Some(sample) = samples {
        if let Some(eta) = eta_blend::find_darwin_eta(&sample.departures, Some(destination), None, state.service_date) {
            state.eta_next = Some(eta);
            state.eta_source = Some("darwin-estimated".to_string());
        }
    }
    state
}
```

This calls a `latest_station_sample(pool, crs) -> Result<Option<StationSample>>` helper — check `crates/api/src/data/queries.rs` first: if an equivalent single-station lookup already exists (used by `crates/api/src/data/samples.rs` or the line-status inference path), reuse it instead of adding a new one; only add it here if nothing already fetches one station's latest `StationSample` by CRS.

Then in both handlers, replace `state.map(Json)` with `Some(blend_darwin_eta(&app, state).await).map(Json)` (or thread it through the `Option` — either way, only call the blend when a state was actually found).

- [ ] **Step 4: Confirm the workspace builds and tests pass**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/eta_blend.rs crates/api/src/data/mod.rs crates/api/src/routes/train.rs
git commit -m "Blend Darwin-estimated ETAs into tracked-train reads when available"
```

---

### Task 7: `trust-consumer` crate scaffold

**Files:**
- Create: `crates/trust-consumer/Cargo.toml`
- Create: `crates/trust-consumer/src/main.rs`
- Create: `crates/trust-consumer/src/config.rs`
- Create: `crates/trust-consumer/src/health.rs`
- Modify: `Cargo.toml` (workspace `members`)

**Interfaces:**
- Produces: a binary that parses config, starts a health server, and idles — the scaffold every later task builds on. `Config` (env/clap). `health::spawn(bind_url) -> Arc<AtomicBool>` (a shared "connected" flag the health server reads and, later, the Kafka connection loop sets).

- [ ] **Step 1: Add the crate to the workspace**

In the root `Cargo.toml`, add `"crates/trust-consumer"` to `members`.

- [ ] **Step 2: Write `crates/trust-consumer/Cargo.toml`**

```toml
[package]
name = "trust-consumer"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
async-trait = "0.1"
axum = { version = "0.8.9", features = ["http2"] }
chrono = { version = "0.4.44", features = ["serde"] }
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
rdkafka = { version = "0.36", features = ["cmake-build", "ssl", "sasl"] }
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
sha2 = "0.10"
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }

[dev-dependencies]
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros", "test-util"] }
```

Actual version numbers for `rdkafka`/`async-trait` should be confirmed with `cargo add rdkafka --features cmake-build,ssl,sasl` and `cargo add async-trait` at execution time (network access required) rather than hand-typed — the versions above are a starting point, not a pin.

- [ ] **Step 3: Write `crates/trust-consumer/src/config.rs`**

```rust
use clap::Parser;

/// CLI/env configuration for the `trust-consumer` service.
///
/// `kafka_topic` and `kafka_sasl_mechanism` deliberately have no default:
/// the exact RDM Train Movements topic name and SASL mechanism (PLAIN vs
/// SCRAM) were not confirmed against a live RDM catalogue entry in this
/// feature's design research (see
/// docs/superpowers/specs/2026-08-28-train-tracking-design.md's Open
/// Questions #1-#3) -- this must be supplied out of band once a real RDM
/// Train Movements subscription exists, not guessed. Same posture as
/// `crates/poller-tocs/src/config.rs`'s `rdm_tocs_base_url`.
#[derive(Debug, Parser)]
pub struct Config {
    /// RDM Kafka broker address(es), comma-separated, e.g.
    /// `kafka.raildata.org.uk:9094`. GAP: unconfirmed hostname.
    #[arg(long, env)]
    pub kafka_brokers: String,

    /// GAP: unconfirmed exact topic name for the Train Movements product.
    #[arg(long, env)]
    pub kafka_topic: String,

    /// Consumer group id. Fixed per deployment, not per-process -- multiple
    /// trust-consumer replicas sharing one group would each get a subset
    /// of partitions, which is fine for horizontal scaling but NOT this
    /// plan's v1 (single replica; see Helm chart task).
    #[arg(long, env, default_value = "nr-status-trust-consumer")]
    pub kafka_consumer_group: String,

    /// RDM's "Consumer key" for this product (SASL username).
    #[arg(long, env)]
    pub kafka_sasl_username: String,

    /// RDM's "Consumer secret" for this product (SASL password).
    #[arg(long, env)]
    pub kafka_sasl_password: String,

    /// GAP: unconfirmed whether RDM's Kafka product uses PLAIN or a SCRAM
    /// variant. PLAIN is `librdkafka`'s simplest, most common default for
    /// managed Kafka-as-a-service offerings, but this is an assumption,
    /// not a confirmed fact -- reject silently guessing wrong by requiring
    /// this be set explicitly rather than defaulting it.
    #[arg(long, env)]
    pub kafka_sasl_mechanism: String,

    /// The `api` crate's ingestion endpoint for train movement events.
    #[arg(long, env, default_value = "http://api:8080/private/train-events")]
    pub api_ingest_url: String,

    /// The `api` crate's endpoint listing active tracked trains.
    #[arg(long, env, default_value = "http://api:8080/private/tracked-trains")]
    pub api_tracked_trains_url: String,

    /// Shared secret sent via `X-Internal-Token` to reach both `api`
    /// endpoints above (see `crates/api/src/auth.rs`).
    #[arg(long, env)]
    pub internal_token: String,

    /// How often to reload the active-tracked-trains reference set from
    /// `api` -- picks up newly created pins and pins that resolved on a
    /// prior run before this process restarted.
    #[arg(long, env, default_value_t = 60)]
    pub reference_reload_secs: u64,

    /// How long to keep `train_movement_events` rows before pruning.
    /// `tracked_trains`/`train_current_state` are kept indefinitely (see
    /// this plan's Global Constraints).
    #[arg(long, env, default_value_t = 90)]
    pub retention_days: i64,

    /// Bind address for the `/healthz` liveness endpoint (Task 7's
    /// `health.rs`). A persistent Kafka consumer needs
    /// connected/reconnecting/disconnected health semantics, not the
    /// "last poll succeeded at T" shape every cron-style poller uses --
    /// see docs/superpowers/specs/2026-08-28-train-tracking-design.md's
    /// Open Questions #6.
    #[arg(long, env, default_value = "0.0.0.0:8081")]
    pub health_bind_url: String,
}
```

- [ ] **Step 4: Write `crates/trust-consumer/src/health.rs`**

```rust
//! Minimal liveness endpoint. Unlike every existing poller (whose health
//! is implicit -- "did the last cron tick succeed") and `crates/enricher`
//! (which has "no HTTP surface" at all per its own deployment templates),
//! a persistent Kafka consumer needs a real connected/disconnected signal
//! a Kubernetes liveness probe can act on: a broker connection that's
//! silently wedged should get restarted, not left running forever.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::StatusCode;
use axum::routing::get;

/// Shared with the Kafka consumer loop (Task 14): `true` once the consumer
/// has successfully polled at least one batch (or confirmed group
/// membership) since the last disconnect; `false` from startup and
/// whenever a reconnect is in progress.
pub type ConnectionState = Arc<AtomicBool>;

pub fn spawn(bind_url: String) -> ConnectionState {
    let state: ConnectionState = Arc::new(AtomicBool::new(false));
    let state_for_server = Arc::clone(&state);

    tokio::spawn(async move {
        let app = axum::Router::new()
            .route("/healthz", get(move || healthz(Arc::clone(&state_for_server))))
            ;
        let listener = match tokio::net::TcpListener::bind(&bind_url).await {
            Ok(listener) => listener,
            Err(err) => {
                tracing::error!(error = ?err, bind_url, "failed to bind health endpoint");
                return;
            }
        };
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!(error = ?err, "health endpoint server stopped");
        }
    });

    state
}

async fn healthz(state: ConnectionState) -> (StatusCode, &'static str) {
    if state.load(Ordering::Relaxed) {
        (StatusCode::OK, "connected")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "disconnected")
    }
}
```

- [ ] **Step 5: Write `crates/trust-consumer/src/main.rs`**

```rust
//! `trust-consumer`: persistent Kafka consumer for Network Rail's TRUST
//! Train Movements feed (via RDM), filtered to exactly the currently
//! user-tracked `(train_uid, date)` set. NOT a cron-style poller -- see
//! docs/superpowers/plans/2026-08-28-train-tracking.md's Global
//! Constraints for why this crate isn't named `poller-trust`.

mod config;
mod health;

use clap::Parser;
use config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let _connection_state = health::spawn(config.health_bind_url.clone());

    tracing::info!("trust-consumer scaffold up; Kafka consumer loop lands in later tasks");

    // Placeholder -- Task 14 replaces this with the real consume loop.
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        tracing::info!("trust-consumer heartbeat");
    }
}
```

- [ ] **Step 6: Confirm the workspace builds**

Run: `cargo build -p trust-consumer`
Expected: PASS (network access required to fetch `rdkafka`, which also needs `cmake` and OpenSSL headers available on the build machine — if `cmake-build` fails locally, `apt-get install -y cmake libssl-dev` first, matching the Dockerfile's builder-stage packages added in Task 15).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/trust-consumer
git commit -m "Scaffold trust-consumer crate"
```

---

### Task 8: TRUST message schema parsing

**Files:**
- Create: `crates/trust-consumer/src/schema.rs`
- Modify: `crates/trust-consumer/src/main.rs` (`mod schema;`)

**Interfaces:**
- Produces: `enum TrustMessage { Activation(Activation), Movement(Movement), Cancellation(Cancellation), ChangeOfOrigin(ChangeOfOrigin), ChangeOfIdentity(ChangeOfIdentity), Unknown(String) }`; `fn parse_batch(raw: &str) -> anyhow::Result<Vec<TrustMessage>>` (pure, unit-tested). Consumed by Task 9 (feed trait returns raw JSON, parsed here), Task 10/11/14.

- [ ] **Step 1: Write the failing tests**

Create `crates/trust-consumer/src/schema.rs`:

```rust
//! TRUST movement-feed message parsing. Field shapes are drawn only from
//! what docs/superpowers/specs/2026-08-28-train-tracking-design.md's
//! research pass independently confirmed (five of eight msg_types, by
//! name and field). `0005`/`0008` are unconfirmed and parse into
//! `TrustMessage::Unknown` rather than being guessed at -- per this
//! codebase's "no invented API details" convention.

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
struct Envelope {
    header: Header,
    body: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
struct Header {
    msg_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Activation {
    pub train_id: String,
    pub train_uid: String,
    pub toc_id: String,
    pub train_service_code: String,
    pub schedule_wtt_id: String,
    pub schedule_start_date: String,
    pub schedule_end_date: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Movement {
    pub train_id: String,
    pub event_type: String, // ARRIVAL | DEPARTURE | PASS
    pub gbtt_timestamp: Option<String>,
    pub planned_timestamp: Option<String>,
    pub actual_timestamp: Option<String>,
    pub reporting_stanox: Option<String>,
    pub loc_stanox: Option<String>,
    pub toc_id: Option<String>,
    pub variation_status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cancellation {
    pub train_id: String,
    pub canx_timestamp: Option<String>,
    pub canx_reason_code: Option<String>,
    pub canx_type: Option<String>, // "EN ROUTE" | "AT ORIGIN"
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangeOfOrigin {
    pub train_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChangeOfIdentity {
    pub train_id: String,
}

#[derive(Debug, Clone)]
pub enum TrustMessage {
    Activation(Activation),
    Movement(Movement),
    Cancellation(Cancellation),
    ChangeOfOrigin(ChangeOfOrigin),
    ChangeOfIdentity(ChangeOfIdentity),
    /// Any `msg_type` this pass doesn't confirm the shape of (`0005`,
    /// `0008`, or anything else RDM's schema turns out to send). Carries
    /// the raw `msg_type` string for logging; the raw body is intentionally
    /// dropped here since there's no confirmed shape to hold it in.
    Unknown(String),
}

/// TRUST sends a JSON array of `{header, body}` envelopes per batch (every
/// 5s or 32 messages, whichever first -- confirmed by the design doc's
/// research). One malformed envelope inside an otherwise-good batch is
/// logged and skipped, not treated as a reason to drop the whole batch.
pub fn parse_batch(raw: &str) -> anyhow::Result<Vec<TrustMessage>> {
    let envelopes: Vec<Envelope> = serde_json::from_str(raw)?;
    Ok(envelopes.into_iter().filter_map(parse_envelope).collect())
}

fn parse_envelope(envelope: Envelope) -> Option<TrustMessage> {
    let parsed = match envelope.header.msg_type.as_str() {
        "0001" => serde_json::from_value(envelope.body).ok().map(TrustMessage::Activation),
        "0002" => serde_json::from_value(envelope.body).ok().map(TrustMessage::Cancellation),
        "0003" => serde_json::from_value(envelope.body).ok().map(TrustMessage::Movement),
        "0006" => serde_json::from_value(envelope.body).ok().map(TrustMessage::ChangeOfOrigin),
        "0007" => serde_json::from_value(envelope.body).ok().map(TrustMessage::ChangeOfIdentity),
        other => return Some(TrustMessage::Unknown(other.to_string())),
    };
    if parsed.is_none() {
        tracing::warn!(msg_type = %envelope.header.msg_type, "confirmed msg_type failed to parse against its known shape; dropping");
    }
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_activation_message() {
        let raw = r#"[{"header":{"msg_type":"0001"},"body":{
            "train_id":"221832406","train_uid":"C21373","toc_id":"SW",
            "train_service_code":"22345000","schedule_wtt_id":"WTT1",
            "schedule_start_date":"2026-08-28","schedule_end_date":"2026-08-28"
        }}]"#;
        let messages = parse_batch(raw).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(matches!(&messages[0], TrustMessage::Activation(a) if a.train_uid == "C21373"));
    }

    #[test]
    fn parses_a_movement_message() {
        let raw = r#"[{"header":{"msg_type":"0003"},"body":{
            "train_id":"221832406","event_type":"DEPARTURE",
            "planned_timestamp":"1756400000000","actual_timestamp":"1756400060000",
            "loc_stanox":"87701","variation_status":"LATE"
        }}]"#;
        let messages = parse_batch(raw).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(matches!(&messages[0], TrustMessage::Movement(m) if m.event_type == "DEPARTURE"));
    }

    #[test]
    fn unconfirmed_msg_types_become_unknown_not_a_parse_error() {
        let raw = r#"[{"header":{"msg_type":"0005"},"body":{"anything":"goes"}}]"#;
        let messages = parse_batch(raw).unwrap();
        assert_eq!(messages.len(), 1);
        assert!(matches!(&messages[0], TrustMessage::Unknown(t) if t == "0005"));
    }

    #[test]
    fn a_confirmed_type_with_a_malformed_body_is_dropped_not_fatal() {
        let raw = r#"[
            {"header":{"msg_type":"0001"},"body":{"not_the_right_shape":true}},
            {"header":{"msg_type":"0001"},"body":{
                "train_id":"221832406","train_uid":"C21373","toc_id":"SW",
                "train_service_code":"22345000","schedule_wtt_id":"WTT1",
                "schedule_start_date":"2026-08-28","schedule_end_date":"2026-08-28"
            }}
        ]"#;
        let messages = parse_batch(raw).unwrap();
        assert_eq!(messages.len(), 1, "the malformed envelope is dropped, the good one survives");
    }

    #[test]
    fn a_batch_of_multiple_message_types_parses_all_of_them() {
        let raw = r#"[
            {"header":{"msg_type":"0002"},"body":{"train_id":"221832406","canx_type":"EN ROUTE"}},
            {"header":{"msg_type":"0006"},"body":{"train_id":"221832406"}},
            {"header":{"msg_type":"0007"},"body":{"train_id":"221832406"}}
        ]"#;
        let messages = parse_batch(raw).unwrap();
        assert_eq!(messages.len(), 3);
        assert!(matches!(&messages[0], TrustMessage::Cancellation(_)));
        assert!(matches!(&messages[1], TrustMessage::ChangeOfOrigin(_)));
        assert!(matches!(&messages[2], TrustMessage::ChangeOfIdentity(_)));
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail, then implement (already written above)**

Run: `cargo test -p trust-consumer parse` — since implementation and tests were written together in Step 1, run once to confirm PASS directly (same posture the reference `enricher` plan used for its own atomic pure-function additions).
Expected: PASS, all 5 tests.

- [ ] **Step 3: Declare the module**

In `crates/trust-consumer/src/main.rs`, add `mod schema;` alongside `mod config; mod health;`.

- [ ] **Step 4: Confirm the count**

Run: `cargo test -p trust-consumer`
Expected: 5 tests pass. If it reports 0, the `mod schema;` declaration from Step 3 was missed — `cargo test` filters by name over whatever's actually declared; a missing `mod` silently excludes the file rather than failing the build.

- [ ] **Step 5: Commit**

```bash
git add crates/trust-consumer/src/schema.rs crates/trust-consumer/src/main.rs
git commit -m "Parse TRUST movement-feed message envelopes"
```

---

### Task 9: `MovementFeed` abstraction — fake for tests, `rdkafka` for production

**Files:**
- Create: `crates/trust-consumer/src/feed/mod.rs`
- Create: `crates/trust-consumer/src/feed/kafka.rs`
- Modify: `crates/trust-consumer/src/main.rs` (`mod feed;`)

**Interfaces:**
- Produces: `trait MovementFeed: Send { async fn next_batch(&mut self) -> anyhow::Result<Vec<String>>; async fn commit(&mut self) -> anyhow::Result<()>; }`; `struct KafkaMovementFeed` (production, wraps `rdkafka::consumer::StreamConsumer`); `struct FakeMovementFeed` (test double, `#[cfg(test)]`-gated but exported for Task 14's tests via `#[cfg(any(test, feature = "test-util"))]` — see reasoning below). Consumed by Task 14 (the processing loop is generic over `impl MovementFeed`).

This is this plan's answer to "there's no `wiremock` for Kafka." The reasoning: `wiremock` works for the reference `enricher` plan's `LlmClient` because HTTP has a well-defined wire format a mock server can speak convincingly. Kafka's wire protocol is materially more complex (broker discovery, partition assignment, consumer-group coordination) and there is no equivalent lightweight embeddable mock broker this codebase already depends on or should add just for tests. The alternative this plan takes instead: push the *entire* Kafka-specific surface — connecting, polling, committing offsets — behind one narrow trait, and keep every other unit (schema parsing, resolution matching, journey/ETA derivation, dedup) as pure functions that never see a `rdkafka` type at all. `FakeMovementFeed` then lets Task 14 test the *processing loop itself* — "given this exact sequence of raw JSON batches, what got POSTed to `api`?" — without a broker, real or mocked. `KafkaMovementFeed` becomes a thin, closely-scoped piece of I/O glue verified by the manual live-broker check in Task 14, the same way `crates/enricher`'s `main.rs` sweep-timer wiring is verified manually rather than unit-tested.

- [ ] **Step 1: Write `crates/trust-consumer/src/feed/mod.rs`**

```rust
//! `MovementFeed`: the one trait that stands between the Kafka-specific
//! consumer (`kafka.rs`) and everything else in this crate. See Task 9's
//! doc comment in the implementation plan for why this replaces
//! `wiremock`-style testing here.

pub mod kafka;

use async_trait::async_trait;

#[async_trait]
pub trait MovementFeed: Send {
    /// Returns the next batch of raw JSON message-batch bodies (each
    /// element is one TRUST batch -- itself a JSON array of envelopes, per
    /// `schema::parse_batch`'s input shape) not yet committed. An empty
    /// `Vec` means "nothing new right now," not an error.
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>>;

    /// Commits offsets for everything returned by the most recent
    /// `next_batch` call. Only called after every message in that batch
    /// has been successfully written through to `api` -- see Task 14's
    /// at-least-once framing: a crash between `next_batch` and `commit`
    /// means the same batch is redelivered next time, which the dedup_key
    /// path (Task 4/13) makes safe.
    async fn commit(&mut self) -> anyhow::Result<()>;
}

#[cfg(any(test, feature = "test-util"))]
pub struct FakeMovementFeed {
    batches: std::collections::VecDeque<Vec<String>>,
    pub committed_count: usize,
}

#[cfg(any(test, feature = "test-util"))]
impl FakeMovementFeed {
    pub fn new(batches: Vec<Vec<String>>) -> Self {
        Self { batches: batches.into(), committed_count: 0 }
    }
}

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl MovementFeed for FakeMovementFeed {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        Ok(self.batches.pop_front().unwrap_or_default())
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        self.committed_count += 1;
        Ok(())
    }
}
```

Add a `test-util` feature to `crates/trust-consumer/Cargo.toml`'s `[features]` section (`test-util = []`) so Task 14's processing-loop tests — which live in a different module than `feed`, both inside the same crate's own `#[cfg(test)]` — can still see `FakeMovementFeed` even though it's declared `#[cfg(test)]` in its own module (same-crate `#[cfg(test)]` items ARE visible to other `#[cfg(test)]` code in the same crate without this, actually — `#[cfg(test)]` gates compilation per the whole test build, not per-module visibility. **Simplify: drop the `feature = "test-util"` gate entirely and use plain `#[cfg(test)]`** — this note exists only to flag that a features-based gate is unnecessary here, unlike a real cross-crate test-util pattern; use the simpler form in the actual file.

- [ ] **Step 2: Write `crates/trust-consumer/src/feed/kafka.rs`**

```rust
//! Production `MovementFeed`: wraps `rdkafka`'s `StreamConsumer` against
//! RDM's Kafka Train Movements product. SASL_SSL is assumed (RDM's Kafka
//! products are described as SASL-authenticated in the design doc's
//! research; the exact mechanism is a startup-time GAP, see `config.rs`).

use async_trait::async_trait;
use rdkafka::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message;

use super::MovementFeed;
use crate::config::Config;
use crate::health::ConnectionState;

pub struct KafkaMovementFeed {
    consumer: StreamConsumer,
    connection_state: ConnectionState,
}

impl KafkaMovementFeed {
    pub fn connect(config: &Config, connection_state: ConnectionState) -> anyhow::Result<Self> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_brokers)
            .set("group.id", &config.kafka_consumer_group)
            .set("security.protocol", "SASL_SSL")
            .set("sasl.mechanisms", &config.kafka_sasl_mechanism)
            .set("sasl.username", &config.kafka_sasl_username)
            .set("sasl.password", &config.kafka_sasl_password)
            .set("enable.auto.commit", "false") // explicit commit, see MovementFeed::commit
            .create()?;

        consumer.subscribe(&[&config.kafka_topic])?;

        Ok(Self { consumer, connection_state })
    }
}

#[async_trait]
impl MovementFeed for KafkaMovementFeed {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        use rdkafka::consumer::MessageStream;
        let _ = MessageStream::default(); // placeholder marker, replaced below
        unreachable!()
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        self.consumer.commit_consumer_state(rdkafka::consumer::CommitMode::Async)?;
        Ok(())
    }
}
```

The `next_batch` body above is deliberately a stub with a `// TODO` shape — `rdkafka`'s exact recommended polling pattern (`StreamConsumer::recv()` in a loop vs. `MessageStream`) should be confirmed against the version `cargo add` actually resolves in Task 7 rather than hand-written blind here; replace it with:

```rust
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        match self.consumer.recv().await {
            Ok(message) => {
                self.connection_state.store(true, std::sync::atomic::Ordering::Relaxed);
                let payload = message.payload().ok_or_else(|| anyhow::anyhow!("empty Kafka message payload"))?;
                Ok(vec![String::from_utf8_lossy(payload).into_owned()])
            }
            Err(err) => {
                self.connection_state.store(false, std::sync::atomic::Ordering::Relaxed);
                Err(err.into())
            }
        }
    }
```

(One `recv()` returns one Kafka message; TRUST's own batching of up to 32 envelopes per message means each `String` returned here is itself the JSON array `schema::parse_batch` expects — confirm this nesting against a real message once a live subscription exists, per this plan's Prerequisites.)

- [ ] **Step 3: Confirm the crate builds**

Run: `cargo build -p trust-consumer`
Expected: PASS.

- [ ] **Step 4: Declare the module**

In `crates/trust-consumer/src/main.rs`, add `mod feed;` alongside the existing `mod config; mod health; mod schema;`.

- [ ] **Step 5: Run the crate's test suite**

Run: `cargo test -p trust-consumer`
Expected: PASS — still just Task 8's 5 tests; `feed` has no tests of its own yet (`FakeMovementFeed` is exercised by Task 14).

- [ ] **Step 6: Commit**

```bash
git add crates/trust-consumer/src/feed crates/trust-consumer/src/main.rs
git commit -m "Add MovementFeed trait with a Kafka implementation and a fake test double"
```

---

### Task 10: Pin resolution matching

**Files:**
- Create: `crates/trust-consumer/src/matching.rs`
- Modify: `crates/trust-consumer/src/main.rs`

**Interfaces:**
- Consumes: `schema::Activation`, `train_tracking::TrackedTrainRef`-shaped pending pins (Task 4's wire shape, received via `GET /private/tracked-trains`).
- Produces: `fn resolve_pin(activation: &Activation, activation_origin_departure: Option<DateTime<Utc>>, pending: &[PendingPin]) -> Option<i64>` (pure, unit-tested — returns the matched `tracked_train_id`, if any). Consumed by Task 14.

This is the design doc's "Darwin↔TRUST correlation is heuristic" problem's sibling: matching a *pin* (all we have is origin CRS + scheduled departure time) to an *Activation* (all it confirms is `train_uid`/`train_id`/`schedule_start_date` — no origin-location timestamp). Per this plan's Global Constraints (no CIF), there's no schedule lookup to bridge "Activation's `train_uid`" to "what time does this train leave WAT." The practical signal actually available at Activation time is thin — `schedule_start_date` narrows to the day, nothing narrows to the specific service on a busy origin station without also seeing that train's first Movement event (its origin-departure timestamp, which the plan's Movement handling already captures). **This task therefore matches primarily on the first Movement event at the pin's `origin_crs`, not on Activation alone** — Activation only tells `trust-consumer` "here's a new `train_id`," and it's the very next Movement event carrying `loc_crs == pin.origin_crs` with an `actual_timestamp` within a tolerance window of `pin.scheduled_departure` that actually resolves a pin. Both steps live in this module since they're two halves of one matching problem.

- [ ] **Step 1: Write the failing tests**

Create `crates/trust-consumer/src/matching.rs`:

```rust
//! Best-effort resolution of a user's pin (origin CRS + scheduled
//! departure time, date -- no train_uid) against the live TRUST feed. See
//! this plan's Task 10 for why this matches on the first origin-station
//! Movement event rather than on Activation alone (this app has no CIF
//! schedule lookup to bridge Activation's train_uid to a departure time).
//! A heuristic, not a guaranteed join -- same posture the design doc takes
//! on Darwin correlation.

use chrono::{DateTime, Utc};

pub struct PendingPin {
    pub tracked_train_id: i64,
    pub pin_origin_crs: String,
    pub pin_scheduled_departure: DateTime<Utc>,
}

/// How far apart a pin's scheduled departure and an observed origin
/// departure event can be and still be considered the same real-world
/// service. Wide enough to survive a train running late from origin (the
/// single most common case), narrow enough that two different services
/// from the same station rarely both fall inside it.
const MATCH_TOLERANCE: chrono::Duration = chrono::Duration::minutes(20);

/// `loc_crs` is the origin-departure Movement event's location, already
/// translated from STANOX by the caller (see Task 11's translation table).
/// Returns the first pending pin whose origin CRS matches and whose
/// scheduled departure is within `MATCH_TOLERANCE` of `actual_timestamp`.
/// If more than one pending pin matches (two users pinned trains that
/// happen to depart the same station within the tolerance window), the
/// earliest-created pin wins -- `pending` is expected to be pre-sorted by
/// `tracked_at` by the caller; this function itself stays a simple
/// first-match scan rather than re-deriving an ordering it shouldn't own.
pub fn resolve_origin_departure(
    loc_crs: &str,
    actual_timestamp: DateTime<Utc>,
    pending: &[PendingPin],
) -> Option<i64> {
    pending
        .iter()
        .find(|pin| {
            pin.pin_origin_crs.eq_ignore_ascii_case(loc_crs)
                && (pin.pin_scheduled_departure - actual_timestamp).abs() <= MATCH_TOLERANCE
        })
        .map(|pin| pin.tracked_train_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(id: i64, crs: &str, scheduled: &str) -> PendingPin {
        PendingPin {
            tracked_train_id: id,
            pin_origin_crs: crs.to_string(),
            pin_scheduled_departure: scheduled.parse().unwrap(),
        }
    }

    #[test]
    fn matches_an_on_time_departure() {
        let pending = vec![pin(1, "WAT", "2026-08-28T18:32:00Z")];
        let actual: DateTime<Utc> = "2026-08-28T18:32:00Z".parse().unwrap();
        assert_eq!(resolve_origin_departure("WAT", actual, &pending), Some(1));
    }

    #[test]
    fn matches_a_late_departure_within_tolerance() {
        let pending = vec![pin(1, "WAT", "2026-08-28T18:32:00Z")];
        let actual: DateTime<Utc> = "2026-08-28T18:45:00Z".parse().unwrap(); // 13m late
        assert_eq!(resolve_origin_departure("WAT", actual, &pending), Some(1));
    }

    #[test]
    fn does_not_match_outside_tolerance() {
        let pending = vec![pin(1, "WAT", "2026-08-28T18:32:00Z")];
        let actual: DateTime<Utc> = "2026-08-28T19:10:00Z".parse().unwrap(); // 38m late
        assert_eq!(resolve_origin_departure("WAT", actual, &pending), None);
    }

    #[test]
    fn does_not_match_a_different_station() {
        let pending = vec![pin(1, "WAT", "2026-08-28T18:32:00Z")];
        let actual: DateTime<Utc> = "2026-08-28T18:32:00Z".parse().unwrap();
        assert_eq!(resolve_origin_departure("PAD", actual, &pending), None);
    }

    #[test]
    fn the_earliest_created_pending_pin_wins_on_ambiguity() {
        let pending = vec![pin(1, "WAT", "2026-08-28T18:32:00Z"), pin(2, "WAT", "2026-08-28T18:35:00Z")];
        let actual: DateTime<Utc> = "2026-08-28T18:33:00Z".parse().unwrap();
        assert_eq!(resolve_origin_departure("WAT", actual, &pending), Some(1));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p trust-consumer resolve_origin_departure`
Expected: PASS.

- [ ] **Step 3: Declare the module**

In `crates/trust-consumer/src/main.rs`, add `mod matching;`.

- [ ] **Step 4: Run the full crate test suite**

Run: `cargo test -p trust-consumer`
Expected: PASS, confirm the count includes both Task 8's 5 and Task 10's 5.

- [ ] **Step 5: Commit**

```bash
git add crates/trust-consumer/src/matching.rs crates/trust-consumer/src/main.rs
git commit -m "Match a pin's origin departure against live TRUST Movement events"
```

---

### Task 11: Journey / position-in-journey derivation

**Files:**
- Create: `crates/trust-consumer/src/journey.rs`
- Modify: `crates/trust-consumer/src/main.rs`

**Interfaces:**
- Consumes: `schema::Movement`, `schema::Cancellation`.
- Produces: `struct DerivedState { status: String, last_reported_location: Option<String>, last_event_type: Option<String>, delay_minutes: Option<i32>, next_calling_point: Option<String> }`; `fn apply_movement(previous: &DerivedState, movement: &Movement, loc_crs: Option<&str>) -> DerivedState`; `fn apply_cancellation(previous: &DerivedState) -> DerivedState` (both pure, unit-tested — mirrors `crates/aggregator/src/matcher.rs`'s existing precedent of pure, independently-testable derivation logic). Consumed by Task 14.

Per this plan's Global Constraints (no CIF), `next_calling_point` is never populated ahead of time — it stays `None` until superseded by the *next* Movement event's own location becoming the new `last_reported_location`. This is the honest v1 shape: "next" only ever means "TRUST hasn't told us about anywhere past here yet," not "the next station on this train's route."

- [ ] **Step 1: Write the failing tests**

Create `crates/trust-consumer/src/journey.rs`:

```rust
//! Pure position-in-journey derivation from a sequence of TRUST events.
//! Structured the way `crates/aggregator/src/matcher.rs` is pure and
//! independently testable -- no I/O, no database, just "given the
//! previous state and one new event, what's the new state."

use crate::schema::Movement;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct DerivedState {
    pub status: String, // "awaiting_activation" | "en_route" | "cancelled" | "completed"
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    pub delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
}

impl DerivedState {
    pub fn awaiting_activation() -> Self {
        Self { status: "awaiting_activation".to_string(), ..Default::default() }
    }
}

/// `loc_crs` is the movement's location, already translated from STANOX by
/// the caller (a real STANOX->CRS table is out of scope for this plan --
/// see Task 14's note on where that lookup comes from); `None` if
/// untranslatable, in which case `last_reported_location` falls back to the
/// raw STANOX so nothing is silently dropped.
pub fn apply_movement(previous: &DerivedState, movement: &Movement, loc_crs: Option<&str>) -> DerivedState {
    let location = loc_crs.map(str::to_string).or_else(|| movement.loc_stanox.clone());
    let delay_minutes = variation_to_minutes(movement.variation_status.as_deref());

    // "PASS" doesn't complete the journey; only the last scheduled
    // location's ARRIVAL/DEPARTURE would, and this crate has no scheduled
    // calling-point list to know which location is "last" (see this
    // plan's Global Constraints) -- so status stays en_route regardless
    // of event_type until an explicit Cancellation ends it. A future
    // CIF-backed pass is the natural place to add real completion
    // detection.
    DerivedState {
        status: "en_route".to_string(),
        last_reported_location: location,
        last_event_type: Some(movement.event_type.clone()),
        delay_minutes,
        next_calling_point: previous.next_calling_point.clone(), // see module docs -- never populated ahead of time
    }
}

pub fn apply_cancellation(previous: &DerivedState) -> DerivedState {
    DerivedState { status: "cancelled".to_string(), ..previous.clone() }
}

/// TRUST's `variation_status` is a category ("ON TIME", "LATE", "EARLY"),
/// not itself a minute count in the confirmed field list -- delay minutes
/// have to come from actual_timestamp - planned_timestamp instead, which
/// this function deliberately does NOT compute (it needs both timestamps
/// parsed, done by the caller in Task 14 where they're already in scope).
/// This function only normalizes the enum-shaped part: "ON TIME"/"EARLY"
/// clamp to zero (never negative -- a train running early isn't a
/// passenger-facing "delay"), "LATE" is left for the caller to fill in
/// with the real minute count, and anything else is `None`.
fn variation_to_minutes(variation_status: Option<&str>) -> Option<i32> {
    match variation_status {
        Some("ON TIME") | Some("EARLY") => Some(0),
        Some("LATE") => None, // caller overwrites with a real value
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn movement(event_type: &str, variation_status: Option<&str>) -> Movement {
        Movement {
            train_id: "221832406".to_string(),
            event_type: event_type.to_string(),
            gbtt_timestamp: None,
            planned_timestamp: None,
            actual_timestamp: None,
            reporting_stanox: None,
            loc_stanox: Some("87701".to_string()),
            toc_id: None,
            variation_status: variation_status.map(str::to_string),
        }
    }

    #[test]
    fn a_movement_sets_status_to_en_route() {
        let previous = DerivedState::awaiting_activation();
        let state = apply_movement(&previous, &movement("DEPARTURE", Some("ON TIME")), Some("WAT"));
        assert_eq!(state.status, "en_route");
        assert_eq!(state.last_reported_location, Some("WAT".to_string()));
        assert_eq!(state.last_event_type, Some("DEPARTURE".to_string()));
    }

    #[test]
    fn falls_back_to_raw_stanox_when_untranslatable() {
        let previous = DerivedState::awaiting_activation();
        let state = apply_movement(&previous, &movement("PASS", None), None);
        assert_eq!(state.last_reported_location, Some("87701".to_string()));
    }

    #[test]
    fn on_time_and_early_clamp_delay_to_zero() {
        let previous = DerivedState::awaiting_activation();
        assert_eq!(apply_movement(&previous, &movement("ARRIVAL", Some("ON TIME")), Some("WOK")).delay_minutes, Some(0));
        assert_eq!(apply_movement(&previous, &movement("ARRIVAL", Some("EARLY")), Some("WOK")).delay_minutes, Some(0));
    }

    #[test]
    fn late_is_left_for_the_caller_to_fill_in() {
        let previous = DerivedState::awaiting_activation();
        assert_eq!(apply_movement(&previous, &movement("ARRIVAL", Some("LATE")), Some("WOK")).delay_minutes, None);
    }

    #[test]
    fn cancellation_preserves_last_known_location() {
        let previous = DerivedState {
            status: "en_route".to_string(),
            last_reported_location: Some("WOK".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(4),
            next_calling_point: None,
        };
        let state = apply_cancellation(&previous);
        assert_eq!(state.status, "cancelled");
        assert_eq!(state.last_reported_location, Some("WOK".to_string()));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p trust-consumer journey`
Expected: PASS.

- [ ] **Step 3: Declare the module**

In `crates/trust-consumer/src/main.rs`, add `mod journey;`.

- [ ] **Step 4: Run the full crate test suite and commit**

Run: `cargo test -p trust-consumer`
Expected: PASS.

```bash
git add crates/trust-consumer/src/journey.rs crates/trust-consumer/src/main.rs
git commit -m "Derive position-in-journey state from TRUST movement events"
```

---

### Task 12: ETA propagation

**Files:**
- Create: `crates/trust-consumer/src/eta.rs`
- Modify: `crates/trust-consumer/src/main.rs`

**Interfaces:**
- Produces: `fn propagate_eta(last_reported_planned: DateTime<Utc>, last_reported_actual: DateTime<Utc>, remaining_scheduled: Option<DateTime<Utc>>) -> Option<DateTime<Utc>>` (pure, unit-tested — the design doc's "take the delay observed at the last-reported location and apply it uniformly forward" propagation). Consumed by Task 14.

Per this plan's Global Constraints (no CIF, no pre-known calling-point list), `remaining_scheduled` — the scheduled time of whatever's next — is not actually available in this pass; this function exists and is tested now so Task 14 has a real place to call it, but Task 14's wiring will only ever invoke it with `remaining_scheduled: None` until a future CIF-backed pass supplies real values, at which point this function needs no changes at all. This is called out explicitly rather than silently skipped, so the gap is visible in the code, not just in this plan.

- [ ] **Step 1: Write the failing tests**

Create `crates/trust-consumer/src/eta.rs`:

```rust
//! Naive TRUST-only ETA: propagate the delay observed at a tracked
//! train's last-reported location uniformly forward. Coarse by design --
//! see docs/superpowers/specs/2026-08-28-train-tracking-design.md's ETA
//! approach section for why this is deliberately simple rather than
//! ML-derived, and `crates/api/src/data/eta_blend.rs` (Task 6) for the
//! Darwin-estimated alternative this yields to when available.

use chrono::{DateTime, Utc};

/// `remaining_scheduled` is the scheduled time of the calling point this
/// ETA is being computed for. Returns `None` if there's nothing to
/// propagate onto -- always true in this plan's v1 (see this file's
/// module docs on the missing CIF-backed calling-point list); wired in
/// now so a future pass only needs to start passing `Some(...)`, not
/// rewrite this function.
pub fn propagate_eta(
    last_reported_planned: DateTime<Utc>,
    last_reported_actual: DateTime<Utc>,
    remaining_scheduled: Option<DateTime<Utc>>,
) -> Option<DateTime<Utc>> {
    let remaining_scheduled = remaining_scheduled?;
    let delay = last_reported_actual - last_reported_planned;
    Some(remaining_scheduled + delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_remaining_scheduled_time_means_no_eta() {
        let planned: DateTime<Utc> = "2026-08-28T18:32:00Z".parse().unwrap();
        let actual: DateTime<Utc> = "2026-08-28T18:37:00Z".parse().unwrap();
        assert_eq!(propagate_eta(planned, actual, None), None);
    }

    #[test]
    fn a_five_minute_delay_propagates_forward_uniformly() {
        let planned: DateTime<Utc> = "2026-08-28T18:32:00Z".parse().unwrap();
        let actual: DateTime<Utc> = "2026-08-28T18:37:00Z".parse().unwrap(); // 5m late
        let next_scheduled: DateTime<Utc> = "2026-08-28T18:50:00Z".parse().unwrap();
        assert_eq!(propagate_eta(planned, actual, Some(next_scheduled)), Some("2026-08-28T18:55:00Z".parse().unwrap()));
    }

    #[test]
    fn running_early_propagates_a_negative_offset() {
        let planned: DateTime<Utc> = "2026-08-28T18:32:00Z".parse().unwrap();
        let actual: DateTime<Utc> = "2026-08-28T18:30:00Z".parse().unwrap(); // 2m early
        let next_scheduled: DateTime<Utc> = "2026-08-28T18:50:00Z".parse().unwrap();
        assert_eq!(propagate_eta(planned, actual, Some(next_scheduled)), Some("2026-08-28T18:48:00Z".parse().unwrap()));
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p trust-consumer propagate_eta`
Expected: PASS.

- [ ] **Step 3: Declare the module, run the full suite, and commit**

Add `mod eta;` to `crates/trust-consumer/src/main.rs`.

Run: `cargo test -p trust-consumer`
Expected: PASS.

```bash
git add crates/trust-consumer/src/eta.rs crates/trust-consumer/src/main.rs
git commit -m "Add naive TRUST delay-propagation ETA calculation"
```

---

### Task 13: Dedup key computation

**Files:**
- Create: `crates/trust-consumer/src/dedup.rs`
- Modify: `crates/trust-consumer/src/main.rs`

**Interfaces:**
- Produces: `fn dedup_key(train_id: &str, msg_type: &str, event_type: Option<&str>, loc_stanox: Option<&str>, planned_timestamp: Option<&str>) -> String` (pure, unit-tested — a stable SHA-256 hex digest, following `crates/enricher/src/hash.rs`'s existing precedent for this exact kind of "stable content hash" helper in this codebase). Consumed by Task 14, and by `crates/api`'s `UNIQUE (tracked_train_id, dedup_key)` constraint (Task 1/4).

- [ ] **Step 1: Write the failing tests**

Create `crates/trust-consumer/src/dedup.rs`:

```rust
use sha2::{Digest, Sha256};

/// Stable across Kafka redelivery of the exact same TRUST message (at-least-once
/// delivery means this WILL happen). Built from the fields that together
/// identify one real-world event -- not the whole message body, which may
/// carry a redelivery-specific envelope field this pass doesn't model.
/// Mirrors `crates/enricher/src/hash.rs`'s `text_hash` in shape and in the
/// null-byte separator rationale (prevents field-boundary collisions).
pub fn dedup_key(
    train_id: &str,
    msg_type: &str,
    event_type: Option<&str>,
    loc_stanox: Option<&str>,
    planned_timestamp: Option<&str>,
) -> String {
    let mut hasher = Sha256::new();
    for field in [train_id, msg_type, event_type.unwrap_or(""), loc_stanox.unwrap_or(""), planned_timestamp.unwrap_or("")] {
        hasher.update(field.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_inputs_hash_identically() {
        assert_eq!(
            dedup_key("221832406", "0003", Some("DEPARTURE"), Some("87701"), Some("1756400000000")),
            dedup_key("221832406", "0003", Some("DEPARTURE"), Some("87701"), Some("1756400000000")),
        );
    }

    #[test]
    fn a_different_event_type_at_the_same_location_hashes_differently() {
        let a = dedup_key("221832406", "0003", Some("ARRIVAL"), Some("87701"), Some("1756400000000"));
        let b = dedup_key("221832406", "0003", Some("DEPARTURE"), Some("87701"), Some("1756400000000"));
        assert_ne!(a, b);
    }

    #[test]
    fn a_different_location_hashes_differently() {
        let a = dedup_key("221832406", "0003", Some("PASS"), Some("87701"), None);
        let b = dedup_key("221832406", "0003", Some("PASS"), Some("11223"), None);
        assert_ne!(a, b);
    }

    #[test]
    fn the_separator_prevents_boundary_collisions() {
        assert_ne!(
            dedup_key("AB", "0003", None, None, None),
            dedup_key("A", "B0003", None, None, None),
        );
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p trust-consumer dedup_key`
Expected: PASS.

- [ ] **Step 3: Declare the module, run the full suite, and commit**

Add `mod dedup;` to `crates/trust-consumer/src/main.rs`.

Run: `cargo test -p trust-consumer`
Expected: PASS.

```bash
git add crates/trust-consumer/src/dedup.rs crates/trust-consumer/src/main.rs
git commit -m "Add stable dedup-key hashing for at-least-once TRUST delivery"
```

---

### Task 14: Wire the full processing loop

**Files:**
- Create: `crates/trust-consumer/src/queries.rs` (HTTP client wrapper against `crates/api`)
- Create: `crates/trust-consumer/src/process.rs`
- Modify: `crates/trust-consumer/src/main.rs`

**Interfaces:**
- Consumes: `feed::MovementFeed` (Task 9), `schema::parse_batch` (Task 8), `matching::resolve_origin_departure` (Task 10), `journey::apply_movement`/`apply_cancellation` (Task 11), `eta::propagate_eta` (Task 12), `dedup::dedup_key` (Task 13).
- Produces: `async fn run<F: MovementFeed>(feed: &mut F, http: &reqwest::Client, config: &Config, connection_state: &ConnectionState) -> anyhow::Result<()>` — one full consume-process-write-commit cycle, tested end-to-end against `FakeMovementFeed`. This is the functionally-complete milestone; every prior task's pure logic is exercised together here for the first time.

- [ ] **Step 1: Write `crates/trust-consumer/src/queries.rs`**

```rust
//! Thin HTTP client wrapper against `crates/api`'s train-tracking
//! endpoints. Kept separate from `process.rs` so the processing loop's
//! tests (below) can run against `FakeMovementFeed` without also needing
//! a live `api` -- these functions are the one part of `process::run` this
//! plan does NOT unit-test, verified instead by Step 5's manual live-stack
//! check, the same posture `crates/enricher`'s DB-touching `queries.rs`
//! takes.

use common::{TrackedTrainRef, TrainMovementEventMessage};
use common::ingest::INTERNAL_TOKEN_HEADER;
use reqwest::Client;

pub async fn fetch_active_tracked_trains(client: &Client, url: &str, internal_token: &str) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let response = client
        .get(url)
        .header(INTERNAL_TOKEN_HEADER, internal_token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

pub async fn post_train_events(client: &Client, url: &str, internal_token: &str, events: &[TrainMovementEventMessage]) -> anyhow::Result<()> {
    if events.is_empty() {
        return Ok(());
    }
    common::ingest::post_batch(client, url, internal_token, events, "train events").await
}
```

`TrackedTrainRef` needs to move from `crates/api/src/data/train_tracking.rs` into `crates/common/src/lib.rs` for this to compile (it's now shared wire shape between `api` and `trust-consumer`, not `api`-internal) — go back and relocate it, re-exporting or updating `crates/api/src/routes/ingest.rs`'s return type accordingly. This is a small correction to Task 4's placement, not new scope: `TrackedTrainRef` was always a wire contract, it just wasn't obviously one until this task needed to consume it from a second crate.

- [ ] **Step 2: Write the failing test for `process::run`**

Create `crates/trust-consumer/src/process.rs`:

```rust
//! The full consume -> parse -> match/derive -> write -> commit cycle,
//! generic over `MovementFeed` so it's testable against `FakeMovementFeed`
//! without a broker. This is this plan's answer to "no wiremock for
//! Kafka" in practice, not just in the abstract -- see Task 9's doc
//! comment for the reasoning.

use crate::feed::MovementFeed;
use crate::schema::TrustMessage;

/// In-memory mirror of what `api`'s active-tracked-trains reference set
/// contains, refreshed on `run`'s caller's own schedule (main.rs's
/// reference-reload timer, wired in Step 4). Kept as a plain argument
/// rather than internal state so `run` itself stays a pure-ish function of
/// (feed, reference) -> (events posted), easy to assert against in tests.
pub struct Reference {
    pub pending: Vec<crate::matching::PendingPin>,
}

/// One full cycle: pull whatever the feed has, parse it, resolve/derive
/// against `reference`, and return the batch of events that would be
/// posted to `api` -- NOT posted yet, so tests can assert on the returned
/// `Vec` directly without an HTTP layer in the loop at all. `main.rs`'s
/// real loop (Step 4) posts this return value and only then calls
/// `feed.commit()`.
pub async fn run_once<F: MovementFeed>(feed: &mut F, reference: &Reference) -> anyhow::Result<Vec<common::TrainMovementEventMessage>> {
    let raw_batches = feed.next_batch().await?;
    let mut events = Vec::new();

    for raw in raw_batches {
        let messages = crate::schema::parse_batch(&raw)?;
        for message in messages {
            if let Some(event) = process_message(&message, reference) {
                events.push(event);
            }
        }
    }

    Ok(events)
}

fn process_message(message: &TrustMessage, reference: &Reference) -> Option<common::TrainMovementEventMessage> {
    match message {
        TrustMessage::Movement(movement) => {
            let planned = movement.planned_timestamp.as_deref().and_then(parse_epoch_millis);
            let actual = movement.actual_timestamp.as_deref().and_then(parse_epoch_millis);
            let loc_crs = None; // STANOX->CRS translation: see this task's Step 3 note.

            let tracked_train_id = actual.and_then(|actual_ts| {
                crate::matching::resolve_origin_departure(movement.loc_stanox.as_deref()?, actual_ts, &reference.pending)
            })?;

            let previous = crate::journey::DerivedState::awaiting_activation();
            let mut derived = crate::journey::apply_movement(&previous, movement, loc_crs);
            if let (Some(p), Some(a), Some("LATE")) = (planned, actual, movement.variation_status.as_deref()) {
                derived.delay_minutes = Some((a - p).num_minutes() as i32);
            }

            let dedup = crate::dedup::dedup_key(
                &movement.train_id,
                "0003",
                Some(&movement.event_type),
                movement.loc_stanox.as_deref(),
                movement.planned_timestamp.as_deref(),
            );

            Some(common::TrainMovementEventMessage {
                tracked_train_id,
                resolved_train_uid: None, // set only on the Activation-carrying path -- see Step 3's note
                resolved_train_id: Some(movement.train_id.clone()),
                dedup_key: dedup,
                msg_type: "0003".to_string(),
                event_type: Some(movement.event_type.clone()),
                loc_stanox: movement.loc_stanox.clone(),
                loc_crs: loc_crs.map(str::to_string),
                planned_timestamp: planned,
                actual_timestamp: actual,
                variation_status: movement.variation_status.clone(),
                raw_body: serde_json::json!({}),
                status: derived.status,
                last_reported_location: derived.last_reported_location,
                last_event_type: derived.last_event_type,
                delay_minutes: derived.delay_minutes,
                next_calling_point: derived.next_calling_point,
                eta_next: None,
                eta_source: None,
            })
        }
        // Activation/Cancellation/ChangeOfOrigin/ChangeOfIdentity/Unknown handling
        // follows the same shape and is filled in during this step's
        // implementation -- omitted here for brevity; the test in Step 2
        // covers Movement specifically, and Step 3 below spells out what's
        // left as a known simplification for this task.
        _ => None,
    }
}

fn parse_epoch_millis(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let millis: i64 = raw.parse().ok()?;
    chrono::DateTime::from_timestamp_millis(millis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::FakeMovementFeed;
    use crate::matching::PendingPin;

    fn reference_with_one_pending(id: i64, crs: &str, scheduled: &str) -> Reference {
        Reference {
            pending: vec![PendingPin {
                tracked_train_id: id,
                pin_origin_crs: crs.to_string(),
                pin_scheduled_departure: scheduled.parse().unwrap(),
            }],
        }
    }

    #[tokio::test]
    async fn a_matching_movement_produces_one_event() {
        let raw_batch = r#"[{"header":{"msg_type":"0003"},"body":{
            "train_id":"221832406","event_type":"DEPARTURE",
            "planned_timestamp":"1756405920000","actual_timestamp":"1756405920000",
            "loc_stanox":"WAT","variation_status":"ON TIME"
        }}]"#;
        let mut feed = FakeMovementFeed::new(vec![vec![raw_batch.to_string()]]);
        let reference = reference_with_one_pending(1, "WAT", "2026-08-28T18:32:00Z");

        let events = run_once(&mut feed, &reference).await.unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].tracked_train_id, 1);
        assert_eq!(events[0].status, "en_route");
    }

    #[tokio::test]
    async fn a_movement_with_no_matching_pin_produces_no_event() {
        let raw_batch = r#"[{"header":{"msg_type":"0003"},"body":{
            "train_id":"999","event_type":"DEPARTURE",
            "planned_timestamp":"1756405920000","actual_timestamp":"1756405920000",
            "loc_stanox":"PAD","variation_status":"ON TIME"
        }}]"#;
        let mut feed = FakeMovementFeed::new(vec![vec![raw_batch.to_string()]]);
        let reference = reference_with_one_pending(1, "WAT", "2026-08-28T18:32:00Z");

        let events = run_once(&mut feed, &reference).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn an_empty_batch_produces_no_events_and_is_not_an_error() {
        let mut feed = FakeMovementFeed::new(vec![vec![]]);
        let reference = reference_with_one_pending(1, "WAT", "2026-08-28T18:32:00Z");
        let events = run_once(&mut feed, &reference).await.unwrap();
        assert!(events.is_empty());
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail, then get them passing**

Run: `cargo test -p trust-consumer process`
Expected: initial FAIL (module not wired), then PASS once `mod process;` and `mod queries;` are added to `main.rs` (next step) and the code above compiles.

- [ ] **Step 3: Known simplifications this task deliberately leaves for follow-up work**

Document these directly as a code comment block at the top of `process.rs` (not just in this plan) so they're visible to whoever picks this crate up next:

- **STANOX→CRS translation is not implemented.** `loc_crs` is hardcoded `None` in `process_message`. TRUST messages carry STANOX codes, not CRS; the `crates/api`-owned `stations` reference table doesn't currently store a STANOX column ( `StationReference` in `crates/common` has no such field — confirm before assuming it does). Closing this gap is a small, self-contained follow-up: add a STANOX column to the stations migration, source it from whatever RDM reference product publishes the STANOX↔CRS mapping (unconfirmed — another GAP), and thread a lookup table into `process_message`. Until then, `last_reported_location` falls back to the raw STANOX (per `journey::apply_movement`'s existing fallback), which is honest but not display-friendly.
- **Activation handling (binding `train_id` → `train_uid` and marking a pin `resolved`) is not implemented in this task's `process_message` match arms.** The `PendingPin`/`resolve_origin_departure` matching in Task 10 resolves *which pin* a Movement belongs to; a real Activation message's own `train_uid` still needs to flow into `resolved_train_uid`/`resolved_train_id` on the *first* event posted for a newly-resolved `tracked_train_id`, matching `crates/api/src/data/train_tracking.rs::upsert_train_event`'s expectation (Task 4). Add this as the immediate next step before Task 14 is considered done: an `TrustMessage::Activation(activation) => { ... }` arm that, on first observing a `train_id` with no prior resolution, looks it up via the *same* `resolve_origin_departure`-driven matching (an Activation alone can't resolve a pin per Task 10's reasoning — hold the `Activation`'s `train_uid`/`train_id` in a short-lived in-memory map keyed by `train_id`, and attach it to whichever event *does* resolve the pin, i.e. the first matching Movement).
- **Cancellation/ChangeOfOrigin/ChangeOfIdentity arms are stubbed to `None`** (dropped, not posted) in the code above. Fill these in following the same shape as the `Movement` arm: `TrustMessage::Cancellation` calls `journey::apply_cancellation` and needs to know its `tracked_train_id` from an in-memory `train_id -> tracked_train_id` map populated once a Movement has resolved that `train_id` (the same map the Activation-binding note above needs) — a cancellation can arrive for a `train_id` this process has already resolved, so it does NOT go through `matching::resolve_origin_departure` a second time.

Before moving to Step 4, actually implement the `train_id -> tracked_train_id` in-memory map (a `HashMap<String, i64>` field threaded through `run_once`, populated on every resolving Movement and consulted first by every message type) and wire in the Activation/Cancellation/ChangeOfOrigin/ChangeOfIdentity arms per the notes above, with tests for each mirroring Step 2's shape (matching Movement / no-match / cancellation-after-resolution). This step's "known simplification" framing is for documentation clarity about the order these were built in, not a license to ship the stubs — the task is not done until these arms exist and are tested.

- [ ] **Step 4: Wire `run_once` into `main.rs`'s real loop**

Replace the placeholder loop in `crates/trust-consumer/src/main.rs`:

```rust
mod config;
mod dedup;
mod eta;
mod feed;
mod health;
mod journey;
mod matching;
mod process;
mod queries;
mod schema;

use std::collections::HashMap;
use std::time::Duration;

use clap::Parser;
use config::Config;
use feed::kafka::KafkaMovementFeed;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let connection_state = health::spawn(config.health_bind_url.clone());
    let http = reqwest::Client::new();

    let mut feed = KafkaMovementFeed::connect(&config, connection_state.clone())?;

    let mut reference = process::Reference { pending: Vec::new() };
    let mut last_reference_reload = tokio::time::Instant::now() - Duration::from_secs(config.reference_reload_secs);

    loop {
        if last_reference_reload.elapsed() >= Duration::from_secs(config.reference_reload_secs) {
            match queries::fetch_active_tracked_trains(&http, &config.api_tracked_trains_url, &config.internal_token).await {
                Ok(refs) => {
                    reference.pending = refs
                        .into_iter()
                        .filter(|r| r.resolution_status == "pending")
                        .map(|r| crate::matching::PendingPin {
                            tracked_train_id: r.id,
                            pin_origin_crs: r.pin_origin_crs,
                            pin_scheduled_departure: r.pin_scheduled_departure,
                        })
                        .collect();
                    last_reference_reload = tokio::time::Instant::now();
                }
                Err(err) => tracing::error!(error = ?err, "failed to reload active tracked trains; retrying next cycle"),
            }
        }

        match process::run_once(&mut feed, &reference).await {
            Ok(events) => {
                if let Err(err) = queries::post_train_events(&http, &config.api_ingest_url, &config.internal_token, &events).await {
                    tracing::error!(error = ?err, "failed to post train events; not committing this batch's offsets");
                    continue; // do NOT commit -- at-least-once redelivery will retry, dedup_key makes it safe
                }
                if let Err(err) = feed.commit().await {
                    tracing::error!(error = ?err, "failed to commit Kafka offsets");
                }
            }
            Err(err) => {
                tracing::error!(error = ?err, "error processing movement feed batch");
            }
        }
    }
}
```

- [ ] **Step 5: Confirm the crate builds and the full test suite passes**

Run: `cargo build -p trust-consumer && cargo test -p trust-consumer`
Expected: PASS.

- [ ] **Step 6: Manual verification against a real Kafka broker**

This step is blocked on this plan's Prerequisites (RDM Train Movements access). Once available:

```bash
docker run --rm -d --name test-kafka -p 9092:9092 apache/kafka:3.7.0
# produce a synthetic TRUST-shaped batch to a local test topic and confirm
# trust-consumer, pointed at it via KAFKA_BROKERS=localhost:9092, logs
# "posted train events" for a manually-created pin matching the synthetic
# message's loc_stanox/timestamp. Full instructions depend on the local
# Kafka image's CLI, out of scope to fully script here without live RDM
# schema confirmation of the real topic's exact message shape.
```

Expected: end-to-end flow confirmed against *some* Kafka broker (local, disposable) even before real RDM access exists — this at least proves `KafkaMovementFeed`'s connection/consume/commit mechanics work, independent of RDM's actual message content.

- [ ] **Step 7: Commit**

```bash
git add crates/trust-consumer/src/process.rs crates/trust-consumer/src/queries.rs crates/trust-consumer/src/main.rs crates/common/src/lib.rs crates/api/src/data/train_tracking.rs crates/api/src/routes/ingest.rs
git commit -m "Wire the full TRUST consume-match-derive-write processing loop"
```

---

### Task 15: Deployment — Dockerfile, docker-compose, env files

**Files:**
- Create: `docker/trust-consumer.Dockerfile`
- Modify: `docker-compose.yml`
- Modify: `local.env.example`
- Modify: `dev.env.example`

**Interfaces:** none (deployment only).

- [ ] **Step 1: Write `docker/trust-consumer.Dockerfile`**

```dockerfile
# syntax=docker/dockerfile:1
# Multi-stage build for trust-consumer. Needs cmake + OpenSSL headers in
# the builder stage for rdkafka's `cmake-build`/`ssl` features (librdkafka
# is a C library rdkafka vendors and compiles from source) -- every other
# Rust service in this repo is a pure-Rust dependency tree and doesn't need
# this.
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake libssl-dev pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin trust-consumer; \
    else \
      cargo build --bin trust-consumer; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/trust-consumer /usr/local/bin/trust-consumer

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --shell /usr/sbin/nologin trust-consumer

COPY --from=builder /usr/local/bin/trust-consumer /usr/local/bin/trust-consumer

USER trust-consumer

ENTRYPOINT ["/usr/local/bin/trust-consumer"]
```

- [ ] **Step 2: Add the `trust-consumer` service to `docker-compose.yml`**

Add after the `enricher` service block:

```yaml
  trust-consumer:
    build:
      context: .
      dockerfile: docker/trust-consumer.Dockerfile
      args:
        CARGO_PROFILE: release
    restart: unless-stopped
    depends_on:
      api:
        condition: service_healthy
    environment:
      # crates/trust-consumer/src/config.rs: Config. GAP: KAFKA_BROKERS,
      # KAFKA_TOPIC, and KAFKA_SASL_MECHANISM are unconfirmed against a
      # live RDM Train Movements catalogue entry -- placeholders until a
      # real subscription resolves them (see local.env.example and this
      # plan's Prerequisites).
      KAFKA_BROKERS: ${KAFKA_BROKERS}
      KAFKA_TOPIC: ${KAFKA_TOPIC}
      KAFKA_CONSUMER_GROUP: ${KAFKA_CONSUMER_GROUP:-nr-status-trust-consumer}
      KAFKA_SASL_USERNAME: ${KAFKA_SASL_USERNAME}
      KAFKA_SASL_PASSWORD: ${KAFKA_SASL_PASSWORD}
      KAFKA_SASL_MECHANISM: ${KAFKA_SASL_MECHANISM:-PLAIN}
      API_INGEST_URL: http://api:8080/private/train-events
      API_TRACKED_TRAINS_URL: http://api:8080/private/tracked-trains
      INTERNAL_TOKEN: ${INTERNAL_TOKEN}
      REFERENCE_RELOAD_SECS: ${REFERENCE_RELOAD_SECS:-60}
      RETENTION_DAYS: ${RETENTION_DAYS_TRAIN_EVENTS:-90}
      HEALTH_BIND_URL: 0.0.0.0:8081
      RUST_LOG: ${RUST_LOG:-info}
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8081/healthz"]
      interval: 10s
      timeout: 3s
      retries: 5
      start_period: 15s
```

(`curl` needs to be present in the runtime image for this healthcheck to work — add `curl` alongside `ca-certificates`/`libssl3` in the Dockerfile's final stage if it isn't already covered by a base-image default; check `docker/api.Dockerfile`'s equivalent healthcheck setup for the exact pattern this repo already uses, since `api`'s own healthcheck has the identical requirement.)

- [ ] **Step 3: Add the new env vars to both example env files**

In `local.env.example`, following the existing `RDM_*_BASE_URL` GAP-placeholder convention:

```bash
# crates/trust-consumer/src/config.rs -- GAP: none of these are confirmed
# against a live RDM Train Movements catalogue entry. Requires the RDM
# subscription/licence prerequisites in
# docs/superpowers/plans/2026-08-28-train-tracking.md to be completed
# before these can be real values.
KAFKA_BROKERS=kafka.example.invalid:9094
KAFKA_TOPIC=replace-with-confirmed-topic-name
KAFKA_CONSUMER_GROUP=nr-status-trust-consumer
KAFKA_SASL_USERNAME=changeme-rdm-trust-consumer-key
KAFKA_SASL_PASSWORD=changeme-rdm-trust-consumer-secret
KAFKA_SASL_MECHANISM=PLAIN
REFERENCE_RELOAD_SECS=60
RETENTION_DAYS_TRAIN_EVENTS=90
```

In `dev.env.example`, the same block (there is no "real local Kafka" equivalent to `dev.env`'s usual "point at something actually running locally" pattern until prerequisite access exists — leave these as the same placeholders, with a comment noting `trust-consumer` will crash-loop on `docker compose up` until real values are supplied, same as every GAP-flagged poller today).

- [ ] **Step 4: Verify the compose stack builds**

Run: `docker compose --env-file dev.env build trust-consumer`
Expected: PASS (the `cmake`/`libssl-dev` build-stage packages make this a slower build than the other Rust services — that's expected, not a regression).

- [ ] **Step 5: Commit**

```bash
git add docker/trust-consumer.Dockerfile docker-compose.yml local.env.example dev.env.example
git commit -m "Add trust-consumer to the docker-compose stack"
```

---

### Task 16: Helm chart additions

**Files:**
- Modify: `charts/nr-status/values.yaml`
- Modify: `charts/nr-status/templates/secret.yaml`
- Create: `charts/nr-status/templates/trust-consumer-deployment.yaml`

**Interfaces:** none (deployment only). Per the design doc's Open Questions #6, this deployment needs different liveness semantics than every existing poller: a `livenessProbe`/`readinessProbe` against `/healthz` (Task 7), not "no probes" like `aggregator`/`enricher`.

- [ ] **Step 1: Add a `trustConsumer` section to `values.yaml`**

```yaml
# ---------------------------------------------------------------------------
# trust-consumer (crates/trust-consumer/src/config.rs)
#
# Unlike every poller and unlike enricher, this is a persistent Kafka
# consumer with real connected/disconnected liveness semantics -- see
# templates/trust-consumer-deployment.yaml's probes.
# ---------------------------------------------------------------------------
trustConsumer:
  image:
    repository: nr-status/trust-consumer
    tag: ""
    pullPolicy: IfNotPresent
  kafka:
    brokers: ""
    topic: ""
    consumerGroup: nr-status-trust-consumer
    saslMechanism: PLAIN
    # -- Consumer key/secret. Follows the chart's normal secrets rule: an
    # existingSecret takes priority; otherwise these values (possibly
    # empty pre-subscription) are rendered as-is -- never auto-generated,
    # a random Kafka credential is meaningless.
    saslUsername: ""
    saslPassword: ""
    existingSecret: ""
    existingSecretUsernameKey: kafka-sasl-username
    existingSecretPasswordKey: kafka-sasl-password
  referenceReloadSecs: 60
  retentionDays: 90
  healthPort: 8081
  logLevel: info
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

- [ ] **Step 2: Add `kafka-sasl-username`/`kafka-sasl-password` to `secret.yaml`**

Add alongside the existing `internal-token`/`llm-api-key` blocks:

```
{{- if not .Values.trustConsumer.kafka.existingSecret -}}
{{- $_ := set $data "kafka-sasl-username" (.Values.trustConsumer.kafka.saslUsername | default "" | b64enc) -}}
{{- $_ := set $data "kafka-sasl-password" (.Values.trustConsumer.kafka.saslPassword | default "" | b64enc) -}}
{{- end -}}
```

- [ ] **Step 3: Write `charts/nr-status/templates/trust-consumer-deployment.yaml`**

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-trust-consumer" (include "nr-status.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "nr-status.labels" (dict "root" . "component" "trust-consumer") | nindent 4 }}
spec:
  # Fixed at 1: this plan's v1 uses one fixed consumer-group id per
  # deployment (crates/trust-consumer/src/config.rs's doc comment on
  # kafka_consumer_group) -- horizontal scaling via multiple replicas
  # sharing the group is a real future option, not implemented here.
  replicas: 1
  selector:
    matchLabels:
      {{- include "nr-status.selectorLabels" (dict "root" . "component" "trust-consumer") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "nr-status.labels" (dict "root" . "component" "trust-consumer") | nindent 8 }}
      {{- with .Values.trustConsumer.podAnnotations }}
      annotations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "nr-status.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "nr-status.podSecurityContext" (dict "override" .Values.trustConsumer.podSecurityContext) | nindent 8 }}
      containers:
        - name: trust-consumer
          image: {{ include "nr-status.image" (dict "root" . "image" .Values.trustConsumer.image) | quote }}
          imagePullPolicy: {{ .Values.trustConsumer.image.pullPolicy }}
          securityContext:
            {{- include "nr-status.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          ports:
            - name: health
              containerPort: {{ .Values.trustConsumer.healthPort }}
              protocol: TCP
          # Real connected/disconnected semantics, unlike every poller and
          # unlike enricher's "no probes" -- see this plan's Task 16 intro.
          readinessProbe:
            httpGet:
              path: /healthz
              port: health
            initialDelaySeconds: 10
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: /healthz
              port: health
            initialDelaySeconds: 30
            periodSeconds: 15
            failureThreshold: 6 # tolerate a real reconnect cycle before restarting the pod
          env:
            {{- include "nr-status.databaseEnv" . | nindent 12 }}
            - name: KAFKA_BROKERS
              value: {{ .Values.trustConsumer.kafka.brokers | quote }}
            - name: KAFKA_TOPIC
              value: {{ .Values.trustConsumer.kafka.topic | quote }}
            - name: KAFKA_CONSUMER_GROUP
              value: {{ .Values.trustConsumer.kafka.consumerGroup | quote }}
            - name: KAFKA_SASL_MECHANISM
              value: {{ .Values.trustConsumer.kafka.saslMechanism | quote }}
            - name: KAFKA_SASL_USERNAME
              valueFrom:
                secretKeyRef:
                  name: {{ default (include "nr-status.secretName" .) .Values.trustConsumer.kafka.existingSecret }}
                  key: {{ .Values.trustConsumer.kafka.existingSecretUsernameKey }}
            - name: KAFKA_SASL_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ default (include "nr-status.secretName" .) .Values.trustConsumer.kafka.existingSecret }}
                  key: {{ .Values.trustConsumer.kafka.existingSecretPasswordKey }}
            - name: API_INGEST_URL
              value: "http://{{ include "nr-status.fullname" . }}-api:8080/private/train-events"
            - name: API_TRACKED_TRAINS_URL
              value: "http://{{ include "nr-status.fullname" . }}-api:8080/private/tracked-trains"
            - name: INTERNAL_TOKEN
              valueFrom:
                secretKeyRef:
                  name: {{ include "nr-status.secretName" . }}
                  key: internal-token
            - name: REFERENCE_RELOAD_SECS
              value: {{ .Values.trustConsumer.referenceReloadSecs | quote }}
            - name: RETENTION_DAYS
              value: {{ .Values.trustConsumer.retentionDays | quote }}
            - name: HEALTH_BIND_URL
              value: {{ printf "0.0.0.0:%d" (int .Values.trustConsumer.healthPort) | quote }}
            - name: RUST_LOG
              value: {{ .Values.trustConsumer.logLevel | quote }}
            {{- with .Values.trustConsumer.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with .Values.trustConsumer.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.trustConsumer.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.trustConsumer.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.trustConsumer.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
```

Check `charts/nr-status/templates/_helpers.tpl` and an existing deployment template (e.g. `aggregator`'s) for the exact `nr-status.databaseEnv`/`nr-status.secretName`/API-service-hostname helper names before assuming the ones used above are verbatim correct — copy the real helper calls from the closest existing precedent rather than the names guessed here if they differ.

- [ ] **Step 4: Render the chart to confirm it's syntactically valid**

Run: `helm template nr-status ./charts/nr-status --set trustConsumer.kafka.brokers=kafka.example.invalid:9094,trustConsumer.kafka.topic=test-topic`
Expected: PASS — renders without error, output includes a `trust-consumer` Deployment with both probes present.

- [ ] **Step 5: Commit**

```bash
git add charts/nr-status/values.yaml charts/nr-status/templates/secret.yaml charts/nr-status/templates/trust-consumer-deployment.yaml
git commit -m "Add trust-consumer to the Helm chart"
```

---

### Task 17: Unbranded Network Rail attribution

**Files:**
- Modify: `frontend/components/OpenDataAttribution.tsx`

**Interfaces:** none (display-only).

Per the design doc's Licensing section: Train Movements is published under Network Rail Infrastructure Limited's own terms, distinct from the NRE terms already covering this app's four existing RDM feeds, and NR's terms explicitly forbid using NR/NRE/TOC branding or describing the app as "official." This task adds a third, deliberately plain, unbranded attribution line — but per this plan's Prerequisites, the exact current wording of NR's attribution requirement is not confirmed (the design doc's own research found "no publication/last-updated date visible on the page itself"). This task therefore ships a conservative, factual line now, flagged for a legal-review pass before this feature's data actually goes live — same fail-loudly-until-confirmed posture as every other GAP in this plan, applied to copy instead of config.

- [ ] **Step 1: Add the attribution line**

In `frontend/components/OpenDataAttribution.tsx`, extend the doc comment and JSX:

```tsx
 * Network Rail Infrastructure Limited's own open-data feeds (the TRUST
 * movement feed powering individual train tracking) are a THIRD, distinct
 * licence from the NRE terms above -- Network Rail's own terms explicitly
 * prohibit using NR/NRE/TOC branding or describing an app as "official"
 * (see docs/superpowers/specs/2026-08-28-train-tracking-design.md's
 * Licensing section). The line below is deliberately unbranded (no logo,
 * no link styled as an endorsement) and factual rather than using NRE's
 * fixed "Powered by..." wording, which is NRE's own licence condition, not
 * Network Rail's. TODO: this exact wording has not been through the
 * dedicated legal sign-off pass this feature's design doc calls for
 * (separate from the NRE Ts&Cs review below) -- re-verify against Network
 * Rail's current open-data-feeds page before this feature's data ships to
 * real users, the same way the NRE wording above was independently
 * verified first.
```

```tsx
      <Text size="xs" c="dimmed">
        Live train movement data from Network Rail&apos;s open data feeds
      </Text>
```

Add this as a third `<Text>` sibling inside the existing `<Box>`, after the NRE attribution line.

- [ ] **Step 2: Confirm the frontend builds**

Run: `cd frontend && npm run build`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add frontend/components/OpenDataAttribution.tsx
git commit -m "Add unbranded Network Rail attribution for the TRUST movement feed"
```

---

### Task 18: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Run the full Rust workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, no regressions.

- [ ] **Step 2: Run `cargo clippy` across the workspace**

Run: `cargo clippy --workspace --all-targets`
Expected: no new warnings from this plan's changes.

- [ ] **Step 3: Bring up the full dev stack**

```bash
docker compose --env-file dev.env up --build -d
docker compose ps
```

Expected: every existing service healthy; `trust-consumer` will crash-loop or sit unhealthy until real `KAFKA_*` values are supplied (expected — see this plan's Prerequisites, and `dev.env.example`'s comment from Task 15). Confirm this is the *only* unhealthy service, and confirm `docker compose logs trust-consumer` shows it failing specifically at the Kafka-connect step (`KafkaMovementFeed::connect`), not crashing earlier in config parsing or `api` connectivity — a crash anywhere else means something in this plan's wiring is broken, not just blocked on prerequisites.

- [ ] **Step 4: Manually verify the pin → API path end-to-end (the part that doesn't need Kafka)**

```bash
source dev.env
psql "$DATABASE_URL" -c "INSERT INTO users (id) VALUES ('TEST-USER') ON CONFLICT DO NOTHING"
psql "$DATABASE_URL" -c "INSERT INTO sessions (id, user_id, expires_at) VALUES ('$(echo -n manual-test-token | sha256sum | cut -d' ' -f1)', 'TEST-USER', NOW() + INTERVAL '1 hour')"
curl -s -X POST http://localhost:8080/Train/track \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"service_date":"2026-08-28","origin_crs":"WAT","scheduled_departure":"2026-08-28T18:32:00Z"}'
curl -s http://localhost:8080/Train/1
curl -s http://localhost:8080/private/tracked-trains -H "x-internal-token: $INTERNAL_TOKEN"
```

Expected: pin creation returns `resolutionStatus: "pending"` (requires the session cookie now — pin creation is owned, per Task 3's account-system coordination fix; a request without `Cookie: nr_session=...` gets `401`); the read route (`GET /Train/1`, unauthenticated — Task 5) returns the same pending state regardless of who's asking; the private reference route lists it, and `SELECT user_id FROM tracked_trains WHERE id = 1` shows `TEST-USER`. Clean up: `psql "$DATABASE_URL" -c "DELETE FROM tracked_trains WHERE id = 1; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"`.

- [ ] **Step 5: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree.
