//! Tracked-train pin creation and lookup. Query functions are kept thin
//! (see `crates/api/src/data/queries.rs`'s module docs for why this crate
//! prefers runtime-checked `sqlx::query` over the `query!` macro family);
//! `validate_pin` is factored out so the one piece of actual logic here is
//! testable without a database.

use chrono::{DateTime, Utc};
use common::{TicketEntryRequest, TrackPinRequest, TrackedTrainRef, TrainMovementEventMessage};
use serde::Serialize;
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

pub async fn create_ticket(
    pool: &PgPool,
    tracked_train_id: i64,
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

/// The public read-model for a ticket, returned directly as JSON by
/// `GET /Train/{trackingId}/tickets` (Task 3). Never leaks `user_id` --
/// same posture as `TrackedTrainState`.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedTrainTicket {
    pub id: i64,
    pub tracked_train_id: i64,
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
