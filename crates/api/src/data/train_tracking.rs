//! Tracked-train pin creation and lookup. Query functions are kept thin
//! (see `crates/api/src/data/queries.rs`'s module docs for why this crate
//! prefers runtime-checked `sqlx::query` over the `query!` macro family);
//! `validate_pin` is factored out so the one piece of actual logic here is
//! testable without a database.

use chrono::{DateTime, Utc};
use common::{TrackPinRequest, TrackedTrainRef, TrainMovementEventMessage};
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
