//! Tracked-train pin creation and lookup. Query functions are kept thin
//! (see `crates/api/src/data/queries.rs`'s module docs for why this crate
//! prefers runtime-checked `sqlx::query` over the `query!` macro family);
//! `validate_pin` is factored out so the one piece of actual logic here is
//! testable without a database.

use chrono::{DateTime, Utc};
use common::{TicketEntryRequest, TrackPinRequest, TrackedTrainRef, TrainMovementEventMessage};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::delay_repay_rules;

/// A pin more than this far in the past is almost certainly a stale
/// frontend view (the user was looking at a departure board snapshot from
/// much earlier) rather than a real tracking request -- reject it rather
/// than create a `tracked_trains` row trust-consumer can never resolve
/// (TRUST's Train Movements feed is a live stream, not a historical
/// lookup; a pin for a service that ran days ago will sit 'pending'
/// forever). A pin arbitrarily far in the future is fine -- "track before
/// it even starts running" is an explicit design goal.
const MAX_PIN_AGE: chrono::Duration = chrono::Duration::hours(6);

/// Caps `list_tracked_trains_for_user`'s response size. No retention or
/// pruning job exists anywhere in this codebase for `tracked_trains`
/// (grepped for `DELETE FROM tracked_trains`/`prune`/`expire`/`retention`
/// -- only `ON DELETE CASCADE` foreign keys and unrelated matches turned
/// up), so this table grows without bound for as long as a user keeps
/// tracking trains, and this cap is the only bound on one HTTP response.
/// `100` is a reasonable-sounding round number, not a researched or
/// load-tested figure -- this codebase has no real-world data yet on how
/// many trains a typical user tracks over their account's lifetime. If
/// usage patterns show this is too low or unnecessarily high, revisit it
/// once real usage exists -- same posture `MAX_PIN_AGE` already took.
/// See docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md's
/// Open Questions 1-2 (also: no pagination/"load more" is designed for
/// what falls past this cap).
const MINE_LIST_LIMIT: i64 = 100;

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

/// Every allowed value of `tracked_train_tickets.source` -- kept in one
/// place (this constant, not repeated string literals) so this app-layer
/// check and the migration's own CHECK constraint (Task 1) can't silently
/// drift apart; if they ever do, the DB constraint is the backstop.
const TICKET_SOURCES: [&str; 4] = ["manual", "pkpass-semantics", "pkpass-heuristic", "pdf-heuristic"];

/// This is the actual mechanism behind "review before save" for the
/// `.pkpass`/PDF ingestion tiers (Tasks 6-9), not merely a data-quality
/// nicety: neither of those formats can ever recover a real CRS code (both
/// only ever give station NAMES, e.g. "Kings Cross" -- see
/// `crates/api/src/data/ticket_extraction.rs`'s module doc). Rejecting a
/// non-3-letter value here means a `PartialTicket` preview resubmitted
/// unedited is *guaranteed* to fail this check, forcing a human to correct
/// it into a real code before anything is ever saved.
pub fn validate_ticket_entry(entry: &TicketEntryRequest) -> Result<(), String> {
    if !TICKET_SOURCES.contains(&entry.source.as_str()) {
        return Err(format!("source must be one of {TICKET_SOURCES:?}"));
    }
    if let Some(crs) = &entry.origin_crs
        && crs.len() != 3
    {
        return Err("origin_crs must be a 3-letter CRS code".to_string());
    }
    if let Some(crs) = &entry.destination_crs
        && crs.len() != 3
    {
        return Err("destination_crs must be a 3-letter CRS code".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod ticket_entry_tests {
    use super::*;

    fn entry(origin_crs: Option<&str>, source: &str) -> TicketEntryRequest {
        TicketEntryRequest {
            operator: Some("LNER".to_string()),
            ticket_type: Some("single".to_string()),
            origin_crs: origin_crs.map(str::to_string),
            destination_crs: Some("EDB".to_string()),
            source: source.to_string(),
        }
    }

    #[test]
    fn a_well_formed_manual_entry_is_valid() {
        assert!(validate_ticket_entry(&entry(Some("KGX"), "manual")).is_ok());
    }

    #[test]
    fn missing_optional_fields_are_valid() {
        let entry = TicketEntryRequest {
            operator: None,
            ticket_type: None,
            origin_crs: None,
            destination_crs: None,
            source: "manual".to_string(),
        };
        assert!(validate_ticket_entry(&entry).is_ok());
    }

    #[test]
    fn a_station_name_instead_of_a_crs_code_is_rejected() {
        // Exactly the "Kings Cross" vs "KGX" case this check exists for --
        // see this function's doc comment.
        assert!(validate_ticket_entry(&entry(Some("Kings Cross"), "manual")).is_err());
    }

    #[test]
    fn every_declared_source_is_accepted() {
        for source in TICKET_SOURCES {
            assert!(validate_ticket_entry(&entry(Some("KGX"), source)).is_ok(), "{source} should be valid");
        }
    }

    #[test]
    fn an_unknown_source_is_rejected() {
        assert!(validate_ticket_entry(&entry(Some("KGX"), "barcode-decoded")).is_err());
    }
}

/// Row shape for `list_active_tracked_trains`'s query -- identical fields
/// to `common::TrackedTrainRef`, but with `sqlx::FromRow` derived, since
/// that derive can't live on `TrackedTrainRef` itself (`crates/common` has
/// no `sqlx` dependency at all). Private: nothing outside this function
/// needs it. See `crates/api/src/data/queries.rs`'s `TflLineSummaryRow`/
/// `row_to_report` for the precedent this mirrors.
#[derive(Debug, Clone, sqlx::FromRow)]
struct TrackedTrainRow {
    id: i64,
    service_date: chrono::NaiveDate,
    pin_origin_crs: String,
    pin_scheduled_departure: DateTime<Utc>,
    resolution_status: String,
    train_uid: Option<String>,
    train_id: Option<String>,
}

impl From<TrackedTrainRow> for TrackedTrainRef {
    fn from(row: TrackedTrainRow) -> Self {
        TrackedTrainRef {
            id: row.id,
            service_date: row.service_date,
            pin_origin_crs: row.pin_origin_crs,
            pin_scheduled_departure: row.pin_scheduled_departure,
            resolution_status: row.resolution_status,
            train_uid: row.train_uid,
            train_id: row.train_id,
        }
    }
}

/// What `trust-consumer` needs for its periodic reference reload (Task
/// 14): pending pins to attempt resolving, and already-resolved ones to
/// recognize incoming TRUST messages against, after a restart or on its
/// periodic reload. "Active" excludes `completed`/`cancelled` rows in
/// `train_current_state` and `unresolved` rows in `tracked_trains` --
/// there is nothing further for trust-consumer to do with either.
pub async fn list_active_tracked_trains(pool: &PgPool) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let rows = sqlx::query_as::<_, TrackedTrainRow>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, tt.train_id \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         WHERE tt.resolution_status != 'unresolved' \
           AND (cs.status IS NULL OR cs.status NOT IN ('completed', 'cancelled'))",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TrackedTrainRef::from).collect())
}

/// Idempotent: resolves the pin (if `resolved_train_uid`/`resolved_train_id`
/// are `Some`), inserts the event row with `ON CONFLICT DO NOTHING`
/// (dedup'd on `(tracked_train_id, dedup_key)` -- a Kafka-redelivered
/// message is silently dropped here), and upserts `train_current_state`.
/// The `train_current_state` upsert always writes on every event, even a
/// redelivered duplicate the event insert just dropped -- writing the same
/// current-state values twice is harmless (idempotent by construction, not
/// merely by dedup), so this doesn't need to be conditioned on whether the
/// event insert actually inserted a row.
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

/// The public read-model for a tracked train, returned directly as JSON by
/// `crates/api/src/routes/train.rs`'s `GET /Train/{trackingId}` and
/// `GET /Train/by-uid/{train_uid}/{date}`. Unlike `TrackedTrainRow`/
/// `TrackedTrainRef` above (poller-facing, private), this never leaks
/// `user_id` -- see Task 5's brief for why these two reads deliberately
/// stay public/unscoped despite `tracked_trains` having a real owner.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
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

/// A user's own tracked-train list, lighter than `TrackedTrainState`
/// (Decision 1 of the design spec) -- excludes live movement detail
/// (`train_id`, `last_reported_location`, `last_event_type`,
/// `next_calling_point`, `eta_next`, `eta_source`), which belongs on the
/// single-train detail page, not a multi-row list. `pin_scheduled_departure`
/// is new here -- no other existing route selects it (Finding 4 of the
/// design spec). Lives API-crate-side only, same as `TrackedTrainState`/
/// `TrackedTrainTicket` -- never sent between Rust services, only
/// serialized to JSON for the frontend.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedTrainListItem {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub tracked_at: DateTime<Utc>,
}

/// A user's own tracked trains, most-recently-tracked first (`tracked_at
/// DESC`, deliberately NOT `pin_scheduled_departure` -- a train pinned a
/// month in advance would otherwise sit ahead of one pinned five minutes
/// ago for a service that's delayed right now, which is very likely the
/// one thing the caller actually wants to check on; see Decision 2 of the
/// design spec), capped at `MINE_LIST_LIMIT` rows. No status-based
/// filtering -- `train_current_state.status` can never actually reach
/// `'completed'` in this codebase today (a separate, already-flagged gap
/// in `crates/trust-consumer/src/journey.rs`, not fixed here), so an
/// "active only" filter would silently do almost nothing while implying
/// curation that isn't happening; this function intentionally does not
/// attempt one.
pub async fn list_tracked_trains_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<Vec<TrackedTrainListItem>> {
    let rows = sqlx::query_as::<_, TrackedTrainListItem>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
                tt.pin_scheduled_departure, tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes, tt.tracked_at \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         WHERE tt.user_id = $1 \
         ORDER BY tt.tracked_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

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

/// Returns the owning `user_id` for a tracked train, or `None` if no such
/// tracked train exists. `POST /Train/{trackingId}/tickets` (Task 3) uses
/// this to answer "does this tracked train exist AND belong to the caller"
/// before creating a ticket against it (there's no existing ticket row yet
/// to filter by, unlike the read paths below). A mismatch or missing
/// tracked train both map to the same `404` at the route layer -- never
/// `403` -- matching `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s
/// existing "exists but not yours" convention.
pub async fn tracked_train_owner(pool: &PgPool, tracking_id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT user_id FROM tracked_trains WHERE id = $1").bind(tracking_id).fetch_optional(pool).await?;
    Ok(row.map(|(id,)| id))
}

/// `tracked_train_id: None` creates a STANDALONE ticket -- one uploaded/
/// entered before the user has found or created the tracked train it's
/// for (see `docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md`'s
/// extraction limits: no date/time is ever recovered from a `.pkpass`/PDF,
/// so extraction alone can never uniquely identify a specific tracked
/// train). The route layer decides which: `post_ticket`
/// (`/Train/{trackingId}/tickets`) always passes `Some(tracking_id)` after
/// its own ownership check; `post_standalone_ticket` (`/Train/tickets`)
/// always passes `None`. See `attach_ticket_to_tracked_train` below for how
/// a standalone ticket later gets a `tracked_train_id`.
pub async fn create_ticket(
    pool: &PgPool,
    tracked_train_id: Option<i64>,
    entry: &TicketEntryRequest,
    user_id: &str,
) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO tracked_train_tickets \
            (tracked_train_id, user_id, operator, ticket_type, origin_crs, destination_crs, source) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id",
    )
    .bind(tracked_train_id)
    .bind(user_id)
    .bind(&entry.operator)
    .bind(&entry.ticket_type)
    .bind(&entry.origin_crs)
    .bind(&entry.destination_crs)
    .bind(&entry.source)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// Attaches a standalone ticket (`create_ticket`'s `tracked_train_id: None`
/// case) to a tracked train, once the caller has found or created the one
/// this ticket is actually for. The route layer (`post_attach_ticket`) is
/// responsible for verifying the tracked train belongs to `user_id` and
/// that the ticket isn't already attached before calling this -- this
/// function still filters on `user_id` and `tracked_train_id IS NULL`
/// itself as defense in depth (never trust a caller-supplied id alone,
/// same posture every other write in this file takes), so it's also safe
/// to call on its own. Returns `true` if a row was actually updated (i.e.
/// the ticket existed, belonged to `user_id`, and was still unattached);
/// `false` covers every other case (no such ticket, not this caller's, or
/// already attached to something) without needing to distinguish them here
/// -- the route layer already knows which applies from its own prior
/// reads.
pub async fn attach_ticket_to_tracked_train(
    pool: &PgPool,
    ticket_id: i64,
    tracking_id: i64,
    user_id: &str,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE tracked_train_tickets \
         SET tracked_train_id = $1 \
         WHERE id = $2 AND user_id = $3 AND tracked_train_id IS NULL",
    )
    .bind(tracking_id)
    .bind(ticket_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// The public read-model for a ticket, returned directly as JSON by
/// `GET /Train/{trackingId}/tickets` (Task 3). Never leaks `user_id` --
/// same posture as `TrackedTrainState`. `tracked_train_id` is `Option<i64>`
/// -- `None` for a standalone ticket that hasn't been attached to a tracked
/// train yet (see `create_ticket`'s doc comment); every row this struct's
/// own `list_tickets_for_tracked_train` returns is guaranteed non-null by
/// that query's own `WHERE tracked_train_id = $1` filter (a NULL column
/// value can never equal a bound `i64`), but `get_ticket_owned` (used by
/// the Delay Repay route and the attach route) has no such filter and can
/// legitimately return `None` here.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedTrainTicket {
    pub id: i64,
    pub tracked_train_id: Option<i64>,
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

const TICKET_SELECT: &str = "\
    SELECT id, tracked_train_id, operator, ticket_type, origin_crs, destination_crs, source, created_at \
    FROM tracked_train_tickets";

/// Filters directly on `(tracked_train_id, user_id)` -- no join needed,
/// per this table's own ownership-redundancy design (see Task 1's migration
/// comment). A caller who doesn't own `tracking_id` gets an empty list,
/// identical to "you own it but have no tickets yet" -- Task 3's route
/// additionally checks `tracked_train_owner` first so the two cases are
/// distinguished at the HTTP layer (404 vs 200 []).
pub async fn list_tickets_for_tracked_train(
    pool: &PgPool,
    tracking_id: i64,
    user_id: &str,
) -> anyhow::Result<Vec<TrackedTrainTicket>> {
    let rows = sqlx::query_as::<_, TrackedTrainTicket>(&format!(
        "{TICKET_SELECT} WHERE tracked_train_id = $1 AND user_id = $2 ORDER BY created_at"
    ))
    .bind(tracking_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Used by the Delay Repay estimate route (Task 5), which needs a single
/// ticket by its own id, still scoped to the caller.
pub async fn get_ticket_owned(pool: &PgPool, ticket_id: i64, user_id: &str) -> anyhow::Result<Option<TrackedTrainTicket>> {
    let row = sqlx::query_as::<_, TrackedTrainTicket>(&format!("{TICKET_SELECT} WHERE id = $1 AND user_id = $2"))
        .bind(ticket_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Caps `list_tickets_for_user`'s response size. No retention/pruning job
/// exists anywhere in this codebase for `tracked_train_tickets` either
/// (grepped for `prune`/`retention`/`expire`/`DELETE FROM tracked_train_tickets`
/// -- only `ON DELETE CASCADE` and unrelated matches turned up), so this
/// table grows without bound for as long as a user keeps adding tickets,
/// and this cap is the only bound on one HTTP response. `100` matches
/// `MINE_LIST_LIMIT`'s proposed figure for the sibling tracked-trains list,
/// for consistency -- not independently researched or load-tested. See
/// docs/superpowers/specs/2026-08-31-tickets-list-design.md's Open
/// Question 1 (also: no pagination/"load more" is designed for what falls
/// past this cap).
const MINE_TICKETS_LIMIT: i64 = 100;

/// Physical columns selected by `list_tickets_for_user`'s query -- private,
/// exists only to satisfy `sqlx::FromRow`. `TicketListItem` (below) is the
/// public shape, built from this plus a pure computation -- same
/// two-struct pattern this file already uses for `TrackedTrainRow` /
/// `TrackedTrainRef`. `tracked_train_id` and every `tracked_trains`/
/// `train_current_state`-sourced column are `Option` -- a standalone
/// ticket (`create_ticket`'s `tracked_train_id: None` case) has no owning
/// tracked train yet, so this query's `LEFT JOIN` (not `JOIN`) to
/// `tracked_trains` can leave every one of them `NULL` for that row.
#[derive(Debug, Clone, sqlx::FromRow)]
struct TicketListRow {
    id: i64,
    tracked_train_id: Option<i64>,
    operator: Option<String>,
    ticket_type: Option<String>,
    origin_crs: Option<String>,
    destination_crs: Option<String>,
    source: String,
    created_at: DateTime<Utc>,
    service_date: Option<chrono::NaiveDate>,
    pin_origin_crs: Option<String>,
    pin_destination_crs: Option<String>,
    pin_scheduled_departure: Option<DateTime<Utc>>,
    resolution_status: Option<String>,
    train_uid: Option<String>,
    status: Option<String>,
    delay_minutes: Option<i32>,
}

/// A user's own tickets, across every tracked train they have -- the
/// cross-train counterpart to `TrackedTrainTicket` (which is scoped to one
/// tracked train). Carries the ticket's own six fields (unchanged from
/// `TrackedTrainTicket`) plus enough of the owning tracked train's context
/// (route, date, live delay) to make a row useful without clicking
/// through, plus a Delay Repay estimate computed inline -- see
/// `build_ticket_list_item` for why that's a pure computation, not a
/// second query per row. The last four fields are deliberately named and
/// shaped to match `DelayRepayEstimateResponse` exactly, field-for-field,
/// so the frontend can pass a `TicketListItem` straight into the
/// already-reviewed `<DelayRepayEstimate>` component with no adapter.
///
/// `tracked_train_id` and every train-context field
/// (`serviceDate`/`pinOriginCrs`/.../`status`) are `Option` -- all `None`
/// together for a standalone ticket with no tracked train attached yet
/// (see `create_ticket`'s doc comment). `estimate`/`delayMinutes` are
/// already `None` in that case too, by construction: `build_ticket_list_item`
/// only ever computes a real estimate from a `(operator, delay_minutes)`
/// pair, and a standalone row has no `delay_minutes` to pair with. This is
/// the same "graceful `None`, never a crash" behavior
/// `get_delay_repay_estimate`'s route already guarantees for an attached
/// ticket whose train hasn't resolved/reported a delay yet -- a standalone
/// ticket is just the same case with `None` for one more reason. `claimUrl`
/// and `disclaimer` are still always populated, per that same route's
/// invariant.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketListItem {
    pub id: i64,
    pub tracked_train_id: Option<i64>,
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub service_date: Option<chrono::NaiveDate>,
    pub pin_origin_crs: Option<String>,
    pub pin_destination_crs: Option<String>,
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
    pub resolution_status: Option<String>,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub estimate: Option<delay_repay_rules::DelayRepayEstimate>,
    pub claim_url: String,
    pub disclaimer: &'static str,
}

/// Mirrors `routes/train.rs`'s `build_delay_repay_response` exactly (same
/// `match (operator, delay_minutes)` shape), so the two independently
/// computed estimates for the same `(ticket, tracked train)` pair can
/// never disagree.
fn build_ticket_list_item(row: TicketListRow) -> TicketListItem {
    let estimate = match (row.operator.as_deref(), row.delay_minutes) {
        (Some(operator), Some(delay_minutes)) => delay_repay_rules::estimate_delay_repay(operator, delay_minutes),
        _ => None,
    };
    let claim_url = row.operator.as_deref().map(delay_repay_rules::claim_url_for).unwrap_or(delay_repay_rules::GENERIC_CLAIM_URL);

    TicketListItem {
        id: row.id,
        tracked_train_id: row.tracked_train_id,
        operator: row.operator,
        ticket_type: row.ticket_type,
        origin_crs: row.origin_crs,
        destination_crs: row.destination_crs,
        source: row.source,
        created_at: row.created_at,
        service_date: row.service_date,
        pin_origin_crs: row.pin_origin_crs,
        pin_destination_crs: row.pin_destination_crs,
        pin_scheduled_departure: row.pin_scheduled_departure,
        resolution_status: row.resolution_status,
        train_uid: row.train_uid,
        status: row.status,
        delay_minutes: row.delay_minutes,
        estimate,
        claim_url: claim_url.to_string(),
        disclaimer: delay_repay_rules::ROUTE_DISCLAIMER,
    }
}

/// A user's own tickets, across every tracked train they have,
/// most-recently-added first. No join needed for ownership (`WHERE
/// t.user_id = $1` on `tracked_train_tickets` alone, per this table's own
/// ownership-redundancy design -- Finding 1 of the design spec) -- the
/// joins to `tracked_trains`/`train_current_state` exist purely to pull in
/// enough train context for a useful row (route, date, live delay) and to
/// let `build_ticket_list_item` compute each row's Delay Repay estimate
/// inline, with no per-ticket follow-up query.
///
/// `LEFT JOIN` (not `JOIN`) to `tracked_trains`: `tracked_train_id` is now
/// nullable (`20260901140000_standalone_tickets.sql`) -- a standalone
/// ticket with no owning tracked train yet must still appear in this list
/// (that's the whole point of surfacing it, so a user can find/attach one),
/// so an inner join here would silently drop exactly the rows this
/// upload-first flow exists to show. `LEFT JOIN` to `train_current_state`
/// matches every other query in this file that reads it: a `pending`/
/// just-resolved tracked train legitimately has no `train_current_state`
/// row yet, same as before.
pub async fn list_tickets_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<Vec<TicketListItem>> {
    let rows = sqlx::query_as::<_, TicketListRow>(
        "SELECT t.id, t.tracked_train_id, t.operator, t.ticket_type, t.origin_crs, t.destination_crs, \
                t.source, t.created_at, \
                tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes \
         FROM tracked_train_tickets t \
         LEFT JOIN tracked_trains tt ON tt.id = t.tracked_train_id \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         WHERE t.user_id = $1 \
         ORDER BY t.created_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(MINE_TICKETS_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(build_ticket_list_item).collect())
}

#[cfg(test)]
mod ticket_list_tests {
    use super::*;

    fn row(operator: Option<&str>, delay_minutes: Option<i32>) -> TicketListRow {
        TicketListRow {
            id: 1,
            tracked_train_id: Some(1),
            operator: operator.map(str::to_string),
            ticket_type: Some("Off-Peak Day Single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "manual".to_string(),
            created_at: "2026-08-29T12:00:00Z".parse().unwrap(),
            service_date: Some("2026-08-29".parse().unwrap()),
            pin_origin_crs: Some("KGX".to_string()),
            pin_destination_crs: Some("EDB".to_string()),
            pin_scheduled_departure: Some("2026-08-29T09:00:00Z".parse().unwrap()),
            resolution_status: Some("resolved".to_string()),
            train_uid: Some("A12345".to_string()),
            status: Some("late".to_string()),
            delay_minutes,
        }
    }

    /// A standalone ticket (never attached to a tracked train, per
    /// `20260901140000_standalone_tickets.sql`) -- every `tracked_trains`/
    /// `train_current_state`-sourced column is `None`, the way
    /// `list_tickets_for_user`'s `LEFT JOIN` actually leaves them for such
    /// a row.
    fn standalone_row(operator: Option<&str>) -> TicketListRow {
        TicketListRow {
            id: 2,
            tracked_train_id: None,
            operator: operator.map(str::to_string),
            ticket_type: Some("Off-Peak Day Single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "pkpass-semantics".to_string(),
            created_at: "2026-08-29T12:00:00Z".parse().unwrap(),
            service_date: None,
            pin_origin_crs: None,
            pin_destination_crs: None,
            pin_scheduled_departure: None,
            resolution_status: None,
            train_uid: None,
            status: None,
            delay_minutes: None,
        }
    }

    // Regression check that this mirrored implementation hasn't drifted
    // from routes/train.rs's build_delay_repay_response for the same
    // (operator, delay_minutes) pair -- see this function's own doc
    // comment for why the two must never disagree.
    #[test]
    fn matches_build_delay_repay_response_for_a_qualifying_dr30_delay() {
        let item = build_ticket_list_item(row(Some("LNER"), Some(45)));

        let estimate = item.estimate.expect("LNER + 45 minutes should clear the DR30 30-minute band");
        assert_eq!(estimate.scheme, "DR30");
        assert_eq!(estimate.percentage, 50);
        assert_eq!(item.claim_url, "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert_eq!(item.delay_minutes, Some(45));
    }

    #[test]
    fn no_operator_yields_no_estimate_but_still_a_real_claim_link_and_disclaimer() {
        let item = build_ticket_list_item(row(None, Some(45)));

        assert_eq!(item.estimate, None);
        assert_eq!(item.claim_url, delay_repay_rules::GENERIC_CLAIM_URL);
        assert_eq!(item.disclaimer, delay_repay_rules::ROUTE_DISCLAIMER);
    }

    #[test]
    fn no_delay_data_yields_no_estimate_but_claim_url_and_disclaimer_are_still_populated() {
        let item = build_ticket_list_item(row(Some("LNER"), None));

        assert_eq!(item.estimate, None);
        assert_eq!(item.delay_minutes, None);
        assert_eq!(item.claim_url, "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert_eq!(item.disclaimer, delay_repay_rules::ROUTE_DISCLAIMER);
    }

    // A standalone ticket (Part A of this plan) must degrade gracefully,
    // not crash -- this is the direct regression check that
    // `list_tickets_for_user`'s `LEFT JOIN` and this function's `Option`
    // fields actually compose safely for a row with no owning tracked
    // train at all, not just no delay data yet.
    #[test]
    fn a_standalone_ticket_with_no_tracked_train_has_no_estimate_but_still_a_real_claim_link_and_disclaimer() {
        let item = build_ticket_list_item(standalone_row(Some("LNER")));

        assert_eq!(item.tracked_train_id, None);
        assert_eq!(item.service_date, None);
        assert_eq!(item.pin_origin_crs, None);
        assert_eq!(item.resolution_status, None);
        assert_eq!(item.status, None);
        assert_eq!(item.delay_minutes, None);
        assert_eq!(item.estimate, None);
        assert_eq!(item.claim_url, "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert_eq!(item.disclaimer, delay_repay_rules::ROUTE_DISCLAIMER);
    }

    #[test]
    fn a_standalone_ticket_with_no_operator_still_gets_the_generic_claim_url() {
        let item = build_ticket_list_item(standalone_row(None));

        assert_eq!(item.estimate, None);
        assert_eq!(item.claim_url, delay_repay_rules::GENERIC_CLAIM_URL);
        assert_eq!(item.disclaimer, delay_repay_rules::ROUTE_DISCLAIMER);
    }
}
