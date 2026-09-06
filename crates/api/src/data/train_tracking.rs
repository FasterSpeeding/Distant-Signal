//! Tracked-train pin creation and lookup. Query functions are kept thin
//! (see `crates/api/src/data/queries.rs`'s module docs for why this crate
//! prefers runtime-checked `sqlx::query` over the `query!` macro family);
//! `validate_pin` is factored out so the one piece of actual logic here is
//! testable without a database.

use chrono::{DateTime, Utc};
use common::{
    CUSTOM_NAME_MAX_LENGTH, TicketEntryRequest, TrackPinRequest, TrackedTrainRef,
    TrainMovementEventMessage,
};
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

/// These messages are USER-FACING COPY, not developer diagnostics. There is
/// no error envelope anywhere in this API (`crates/api/src/routes/train.rs`
/// returns `(StatusCode::BAD_REQUEST, String)` as plain text), and
/// `frontend/components/TrackTrainForm.tsx` renders the body verbatim as
/// the form's error `Alert` -- so a snake_case field name written here
/// becomes a snake_case field name on a user's screen
/// (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F5).
pub fn validate_pin(pin: &TrackPinRequest, now: DateTime<Utc>) -> Result<(), String> {
    if pin.origin_crs.trim().is_empty() {
        return Err("Enter the station you're departing from.".to_string());
    }
    if pin.origin_crs.len() != 3 {
        return Err(
            "That doesn't look like a station code — CRS codes are three letters, like WOK \
             or EUS."
                .to_string(),
        );
    }
    if now - pin.scheduled_departure > MAX_PIN_AGE {
        // Interpolated from MAX_PIN_AGE, not typed as prose, so this message
        // can never drift from the constant it describes.
        return Err(format!(
            "That departure time is more than {} hours ago — trains can only be tracked \
             within {} hours of departure.",
            MAX_PIN_AGE.num_hours(),
            MAX_PIN_AGE.num_hours(),
        ));
    }
    Ok(())
}

/// `user_id` is the authenticated caller's id (the OIDC `sub`, per
/// `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 1) --
/// resolved by the route handler's `AuthenticatedUser` extractor
/// (`crates/api/src/routes/train.rs::post_track`, below), never taken from
/// the request body itself.
pub async fn create_pin(
    pool: &PgPool,
    pin: &TrackPinRequest,
    user_id: &str,
) -> anyhow::Result<i64> {
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
const TICKET_SOURCES: [&str; 4] = [
    "manual",
    "pkpass-semantics",
    "pkpass-heuristic",
    "pdf-heuristic",
];

/// This is the actual mechanism behind "review before save" for the
/// `.pkpass`/PDF ingestion tiers (Tasks 6-9), not merely a data-quality
/// nicety: neither of those formats can ever recover a real CRS code (both
/// only ever give station NAMES, e.g. "Kings Cross" -- see
/// `crates/api/src/data/ticket_extraction.rs`'s module doc). Rejecting a
/// non-3-letter value here means a `PartialTicket` preview resubmitted
/// unedited is *guaranteed* to fail this check, forcing a human to correct
/// it into a real code before anything is ever saved.
/// Same user-facing-copy posture as [`validate_pin`]'s doc comment.
pub fn validate_ticket_entry(entry: &TicketEntryRequest) -> Result<(), String> {
    if !TICKET_SOURCES.contains(&entry.source.as_str()) {
        // Not a `{TICKET_SOURCES:?}` Debug dump of the array (that used to
        // render e.g. `source must be one of ["manual", "pkpass-semantics",
        // "pkpass-heuristic", "pdf-heuristic"]` verbatim to a user). This
        // path is unreachable from the app's own form, which always
        // supplies `source` itself, so listing the valid values buys a
        // direct-API caller nothing a 400 doesn't already tell them.
        return Err("That's not a recognised ticket source.".to_string());
    }
    if let Some(crs) = &entry.origin_crs
        && crs.len() != 3
    {
        return Err(
            "That doesn't look like a station code — CRS codes are three letters, like WOK \
             or EUS."
                .to_string(),
        );
    }
    if let Some(crs) = &entry.destination_crs
        && crs.len() != 3
    {
        return Err(
            "That doesn't look like a destination station code — CRS codes are three \
             letters, like WOK or EUS."
                .to_string(),
        );
    }
    Ok(())
}

/// Normalizes a raw `customName` request field into what should actually be
/// written: `None` if the field was absent/JSON-`null`, or if what's left
/// after trimming is empty (this is "clear the custom name," not an error —
/// see the design spec's Decision 1), or `Some(trimmed)` otherwise, bounded
/// by [`CUSTOM_NAME_MAX_LENGTH`]. Both rename routes (`crates/api/src/routes/train.rs`)
/// call this before writing, so the trim-and-normalize step lives in exactly
/// one place rather than being duplicated between the tracked-train and
/// ticket routes. Same user-facing-copy posture as [`validate_pin`]'s doc
/// comment: this message is rendered verbatim in `RenameTrainButton`/
/// `RenameTicketButton`'s error text, so it carries no internal field names.
pub fn validate_custom_name(name: Option<&str>) -> Result<Option<String>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > CUSTOM_NAME_MAX_LENGTH {
        return Err(format!(
            "That name is too long — custom names can be at most {CUSTOM_NAME_MAX_LENGTH} \
             characters."
        ));
    }
    Ok(Some(trimmed.to_string()))
}

#[cfg(test)]
mod custom_name_tests {
    use super::*;

    #[test]
    fn a_well_formed_name_is_trimmed_and_kept() {
        assert_eq!(
            validate_custom_name(Some("  My commute  ")),
            Ok(Some("My commute".to_string()))
        );
    }

    #[test]
    fn none_input_is_kept_as_none() {
        // JSON `customName` omitted or explicitly `null` -- the route's
        // Option<String> deserializes both to None.
        assert_eq!(validate_custom_name(None), Ok(None));
    }

    #[test]
    fn an_empty_string_clears_rather_than_errors() {
        assert_eq!(validate_custom_name(Some("")), Ok(None));
    }

    #[test]
    fn a_whitespace_only_string_clears_rather_than_errors() {
        // Decision 1's own guard: whitespace-only can't masquerade as "a
        // custom name is set" and permanently hide the useful default.
        assert_eq!(validate_custom_name(Some("   ")), Ok(None));
    }

    #[test]
    fn exactly_at_the_cap_is_accepted() {
        let name = "a".repeat(CUSTOM_NAME_MAX_LENGTH);
        assert_eq!(validate_custom_name(Some(&name)), Ok(Some(name)));
    }

    #[test]
    fn one_over_the_cap_is_rejected() {
        let name = "a".repeat(CUSTOM_NAME_MAX_LENGTH + 1);
        assert!(validate_custom_name(Some(&name)).is_err());
    }

    #[test]
    fn the_cap_counts_unicode_scalar_values_not_bytes() {
        // "café" x 25 = 100 chars but more than 100 UTF-8 bytes (each 'é' is
        // 2 bytes) -- proves this counts chars(), not len(), matching the
        // "100 characters" wording in the error message.
        let name = "café".repeat(25);
        assert_eq!(name.chars().count(), 100);
        assert!(name.len() > 100);
        assert_eq!(validate_custom_name(Some(&name)), Ok(Some(name)));
    }

    #[test]
    fn validation_messages_carry_no_internal_field_names() {
        // Same guard as validate_pin's/validate_ticket_entry's own tests --
        // this 400 body is rendered verbatim by RenameTrainButton/
        // RenameTicketButton's error text.
        let name = "a".repeat(CUSTOM_NAME_MAX_LENGTH + 1);
        let message = validate_custom_name(Some(&name)).unwrap_err();
        assert!(!message.is_empty());
        assert!(
            !message.contains('_'),
            "user-facing copy leaked an identifier: {message}"
        );
    }
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
            assert!(
                validate_ticket_entry(&entry(Some("KGX"), source)).is_ok(),
                "{source} should be valid"
            );
        }
    }

    #[test]
    fn an_unknown_source_is_rejected() {
        assert!(validate_ticket_entry(&entry(Some("KGX"), "barcode-decoded")).is_err());
    }

    #[test]
    fn validation_messages_carry_no_internal_field_names_or_debug_dumps() {
        // Same guard as train_tracking::tests -- these 400 bodies are
        // rendered verbatim by TicketEntryForm.tsx. The old
        // `format!("source must be one of {TICKET_SOURCES:?}")` failed this
        // twice over: both an identifier AND a Rust Debug array dump.
        let messages = [
            validate_ticket_entry(&entry(Some("KGX"), "barcode-decoded")).unwrap_err(),
            validate_ticket_entry(&entry(Some("Kings Cross"), "manual")).unwrap_err(),
            validate_ticket_entry(&TicketEntryRequest {
                operator: None,
                ticket_type: None,
                origin_crs: Some("KGX".to_string()),
                destination_crs: Some("Edinburgh Waverley".to_string()),
                source: "manual".to_string(),
            })
            .unwrap_err(),
        ];
        for message in messages {
            assert!(!message.is_empty(), "validation message must not be empty");
            assert!(
                !message.contains('_'),
                "user-facing copy leaked an identifier: {message}"
            );
            assert!(
                !message.contains('['),
                "user-facing copy leaked a Debug array dump: {message}"
            );
        }
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
pub async fn upsert_train_event(
    pool: &PgPool,
    event: &TrainMovementEventMessage,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;

    // Fires on `resolved_train_id.is_some()` ALONE now -- Decision 5 of
    // docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md,
    // the required companion to schedule-first matching: once a schedule
    // match can populate `train_uid` before ANY TRUST message arrives, the
    // resolving Movement's own `resolved_train_uid` is frequently `None`
    // (this process's `pending_activations` map is unrelated to a
    // schedule match), and the old two-field guard would leave a pin with
    // fully live, correct tracking data stuck at `schedule_matched`
    // forever. `train_uid` uses `COALESCE`, never a blind overwrite,
    // preserving whatever value a schedule match (or an earlier message)
    // already wrote. `resolved`'s own two-field INVARIANT (both
    // `train_uid` and `train_id` bound) is unchanged: this is still the
    // only write that ever sets `resolution_status = 'resolved'`, and it
    // never leaves `train_uid` NULL when it does (either freshly supplied
    // here, or already present from an earlier message/schedule match).
    if let Some(train_id) = &event.resolved_train_id {
        sqlx::query(
            "UPDATE tracked_trains \
             SET train_uid = COALESCE($2, train_uid), train_id = $3, \
                 resolution_status = 'resolved', resolved_at = NOW() \
             WHERE id = $1",
        )
        .bind(event.tracked_train_id)
        .bind(&event.resolved_train_uid)
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

/// Writes a successful schedule match (Decision 3 step 4 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md):
/// sets `train_uid` (never `train_id` -- that stays exclusively
/// TRUST-sourced, per this plan's Global Constraints) and moves
/// `resolution_status` to the new `'schedule_matched'` waypoint. Guarded
/// by `WHERE train_uid IS NULL AND resolution_status = 'pending'` so this
/// is safe to call from BOTH the synchronous pin-creation path and the
/// periodic sweep without a race clobbering a row that has since moved on
/// (a live TRUST Movement resolved it first, or an earlier sweep tick
/// already matched it) -- `rows_affected() == 0` in either of those cases
/// is not an error, just a no-op, which is why this returns `bool` rather
/// than erroring on zero rows affected.
pub async fn apply_schedule_match(
    pool: &PgPool,
    tracked_train_id: i64,
    train_uid: &str,
    matched_line_id: &str,
    schedule_calling_points: &serde_json::Value,
    schedule_destination_crs: Option<&str>,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE tracked_trains \
         SET train_uid = $2, resolution_status = 'schedule_matched', matched_line_id = $3, \
             schedule_calling_points = $4, schedule_destination_crs = $5, schedule_matched_at = NOW() \
         WHERE id = $1 AND train_uid IS NULL AND resolution_status = 'pending'",
    )
    .bind(tracked_train_id)
    .bind(train_uid)
    .bind(matched_line_id)
    .bind(schedule_calling_points)
    .bind(schedule_destination_crs)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Row shape for `list_pending_pins_for_schedule_match`'s query -- every
/// still-`pending`, never-schedule-matched row, the periodic sweep's own
/// input set (Decision 3's "also run this same attempt periodically").
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PendingSchedulePin {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_scheduled_departure: DateTime<Utc>,
}

/// Every row the periodic schedule-match sweep should retry: still
/// `pending` AND still lacking a `train_uid` -- a `schedule_matched` row
/// already has one and is excluded, same as a `resolved`/`unresolved` row.
pub async fn list_pending_pins_for_schedule_match(
    pool: &PgPool,
) -> anyhow::Result<Vec<PendingSchedulePin>> {
    let rows = sqlx::query_as::<_, PendingSchedulePin>(
        "SELECT id, service_date, pin_origin_crs, pin_scheduled_departure \
         FROM tracked_trains WHERE train_uid IS NULL AND resolution_status = 'pending'",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
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
    /// `None` whenever the `LEFT JOIN` below found no `stations` row for
    /// `pin_origin_crs` (an unrecognised code, or reference data that
    /// hasn't caught up) -- see Decision 3 of the plan this join
    /// implements. Every frontend consumer must fall back to the bare CRS
    /// code rather than assume this is always present.
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
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
    pub custom_name: Option<String>,
}

// `LEFT JOIN`, never `JOIN`: a CRS with no reference row (a code the
// stations feed doesn't carry) must still return the train, just with a
// `None` name.
//
// `UPPER(...)` is mandatory, not defensive tidiness. `pin_origin_crs`/
// `pin_destination_crs` are `TEXT` (`migrations/20260828120000_train_tracking.sql`)
// and `validate_pin` never normalises their case, while `stations.crs` is
// `CHAR(3)`. Without `UPPER`, a user who typed `kgx` would get `NULL` here
// and fall back to the bare code -- the exact outcome this join exists to
// remove, for the subset of users most likely to hit it (see Decision 3 of
// docs/superpowers/plans/2026-09-02-frontend-ux-review-fixes.md).
const TRACKED_TRAIN_STATE_SELECT: &str = "\
    SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
           so.name AS pin_origin_name, sd.name AS pin_destination_name, \
           tt.resolution_status, tt.train_uid, tt.train_id, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source, \
           tt.custom_name \
    FROM tracked_trains tt \
    LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
    LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs)";

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
    /// See `TrackedTrainState::pin_origin_name`'s doc comment -- same
    /// `LEFT JOIN stations` mechanism, same `None`-means-no-reference-row
    /// contract.
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub tracked_at: DateTime<Utc>,
    pub custom_name: Option<String>,
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
pub async fn list_tracked_trains_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<TrackedTrainListItem>> {
    // Same `LEFT JOIN stations ... ON so.crs = UPPER(...)` mechanism as
    // `TRACKED_TRAIN_STATE_SELECT` -- see its comment for why `UPPER` is
    // mandatory. This feeds the home dashboard, `/track/mine`, and
    // `AttachTicketAction`'s `Select` -- three of F3's six sites.
    let rows = sqlx::query_as::<_, TrackedTrainListItem>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
                so.name AS pin_origin_name, sd.name AS pin_destination_name, \
                tt.pin_scheduled_departure, tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes, tt.tracked_at, tt.custom_name \
         FROM tracked_trains tt \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs) \
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

pub async fn get_by_tracking_id(
    pool: &PgPool,
    id: i64,
) -> anyhow::Result<Option<TrackedTrainState>> {
    let row = sqlx::query_as::<_, TrackedTrainState>(&format!(
        "{TRACKED_TRAIN_STATE_SELECT} WHERE tt.id = $1"
    ))
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

/// Deletes a tracked train by id, scoped to the caller's ownership -- the
/// check is folded directly into the `WHERE` clause
/// (`WHERE id = $1 AND user_id = $2`), the same shape
/// `custom_lines::delete_custom_line` uses, rather than a separate
/// `tracked_train_owner` lookup followed by an unscoped delete. Unlike
/// `delete_custom_line` (which also has to clean up a `pinned_lines` row
/// with no FK of its own), nothing else needs deleting here: every other
/// row that references a `tracked_trains` id --
/// `train_movement_events`, `train_current_state` (both
/// `crates/api/migrations/20260828120000_train_tracking.sql`), and
/// `tracked_train_tickets` (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql`)
/// -- is declared `ON DELETE CASCADE`, so a single `DELETE FROM
/// tracked_trains` here is sufficient; Postgres does the rest inside the
/// same statement's transaction. Returns `true` if a row was deleted,
/// `false` if no tracked train with that id belongs to this caller
/// (doesn't exist, or belongs to someone else -- indistinguishable at this
/// layer, same as every other ownership check in this file; the route
/// handler maps `false` to `404`, never `403`).
pub async fn delete_tracked_train(pool: &PgPool, id: i64, user_id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM tracked_trains WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Renames (or clears, if `custom_name` is `None`) a tracked train's
/// display name, scoped to the caller's ownership -- same
/// `WHERE id = $1 AND user_id = $2` shape as [`delete_tracked_train`]
/// immediately above, folded directly into the `UPDATE` rather than a
/// separate ownership lookup first. Returns `true` if a row was updated,
/// `false` if no tracked train with that id belongs to this caller
/// (doesn't exist, or belongs to someone else -- indistinguishable at this
/// layer, same as every other ownership check in this file; the route
/// handler maps `false` to `404`, never `403`). The caller
/// (`crate::routes::train::post_tracked_train_name`) is responsible for
/// having already run `custom_name` through [`validate_custom_name`] --
/// this function does no validation of its own, matching
/// `attach_ticket_to_tracked_train`'s own "route validates, data layer
/// writes" division of responsibility.
pub async fn rename_tracked_train(
    pool: &PgPool,
    id: i64,
    user_id: &str,
    custom_name: Option<&str>,
) -> anyhow::Result<bool> {
    let result =
        sqlx::query("UPDATE tracked_trains SET custom_name = $1 WHERE id = $2 AND user_id = $3")
            .bind(custom_name)
            .bind(id)
            .bind(user_id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
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

    #[test]
    fn validation_messages_carry_no_internal_field_names() {
        // The 400 body is rendered verbatim as the form's error Alert
        // (frontend/components/TrackTrainForm.tsx), so a snake_case field
        // name here lands on screen. See the review's §F5. A cheap, durable
        // guard: no branch's message should ever contain `_`.
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let stale_departure: DateTime<Utc> = "2026-06-15T02:00:00Z".parse().unwrap();

        let messages = [
            validate_pin(&pin("", now), now).unwrap_err(),
            validate_pin(&pin("WATERLOO", now), now).unwrap_err(),
            validate_pin(&pin("WAT", stale_departure), now).unwrap_err(),
        ];
        for message in messages {
            assert!(!message.is_empty(), "validation message must not be empty");
            assert!(
                !message.contains('_'),
                "user-facing copy leaked an identifier: {message}"
            );
        }
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
pub async fn tracked_train_owner(
    pool: &PgPool,
    tracking_id: i64,
) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_id FROM tracked_trains WHERE id = $1")
        .bind(tracking_id)
        .fetch_optional(pool)
        .await?;
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
    /// See `TrackedTrainState::pin_origin_name`'s doc comment -- same
    /// `LEFT JOIN stations` mechanism, joined here on the TICKET's own
    /// `origin_crs`/`destination_crs`, not the pin route (which
    /// `TrackedTrainListItem` already covers).
    pub origin_name: Option<String>,
    pub destination_name: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub custom_name: Option<String>,
}

// `format!`ed into two callers below (`list_tickets_for_tracked_train`,
// `get_ticket_owned`), so the joins belong here once and both callers get
// them. The base table is aliased `t` so each caller's appended `WHERE`
// must qualify its columns -- see both callers.
const TICKET_SELECT: &str = "\
    SELECT t.id, t.tracked_train_id, t.operator, t.ticket_type, t.origin_crs, t.destination_crs, \
           so.name AS origin_name, sd.name AS destination_name, t.source, t.created_at, \
           t.custom_name \
    FROM tracked_train_tickets t \
    LEFT JOIN stations so ON so.crs = UPPER(t.origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(t.destination_crs)";

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
        "{TICKET_SELECT} WHERE t.tracked_train_id = $1 AND t.user_id = $2 ORDER BY t.created_at"
    ))
    .bind(tracking_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Used by the Delay Repay estimate route (Task 5), which needs a single
/// ticket by its own id, still scoped to the caller.
pub async fn get_ticket_owned(
    pool: &PgPool,
    ticket_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<TrackedTrainTicket>> {
    let row = sqlx::query_as::<_, TrackedTrainTicket>(&format!(
        "{TICKET_SELECT} WHERE t.id = $1 AND t.user_id = $2"
    ))
    .bind(ticket_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Deletes a ticket by id, scoped to the caller's ownership -- mirrors
/// `delete_tracked_train`'s own `WHERE id = $1 AND user_id = $2` shape
/// exactly (`crates/api/src/data/train_tracking.rs:413-420`). No join
/// needed, per `get_ticket_owned`'s own established precedent just above:
/// `tracked_train_tickets.user_id` is a direct, indexed column
/// (`tracked_train_tickets_user_id`,
/// `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:56`),
/// not transitive through the owning tracked train. Applies identically
/// whether the ticket is attached (`tracked_train_id: Some(_)`) or
/// standalone (`tracked_train_id: None`, per
/// `crates/api/migrations/20260901140000_standalone_tickets.sql`) -- the
/// `WHERE` clause never references that column, so there is nothing to
/// special-case. Nothing else needs deleting as a consequence: unlike a
/// tracked train, a ticket is a leaf in the FK graph -- nothing
/// `REFERENCES tracked_train_tickets` anywhere in this schema. Returns
/// `true` if a row was deleted, `false` if no ticket with that id belongs
/// to this caller (doesn't exist, or belongs to someone else --
/// indistinguishable at this layer, same as every other ownership check
/// in this file; the route handler maps `false` to `404`, never `403`).
pub async fn delete_ticket(pool: &PgPool, ticket_id: i64, user_id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM tracked_train_tickets WHERE id = $1 AND user_id = $2")
        .bind(ticket_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// Renames (or clears) a ticket's display name, scoped to the caller's
/// ownership -- mirrors [`rename_tracked_train`] exactly, and
/// [`delete_ticket`]'s own `WHERE id = $1 AND user_id = $2` shape
/// immediately above it (no join needed, per this table's own
/// ownership-redundancy design -- see [`delete_ticket`]'s doc comment).
/// Applies identically whether the ticket is attached or standalone, same
/// as `delete_ticket`. Returns `true`/`false` with the same "doesn't
/// exist, or isn't yours -- 404, never 403" contract as
/// [`rename_tracked_train`].
pub async fn rename_ticket(
    pool: &PgPool,
    ticket_id: i64,
    user_id: &str,
    custom_name: Option<&str>,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE tracked_train_tickets SET custom_name = $1 WHERE id = $2 AND user_id = $3",
    )
    .bind(custom_name)
    .bind(ticket_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
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
    /// Same `LEFT JOIN stations` mechanism as `TrackedTrainTicket`'s
    /// fields, joined on THIS ticket's own origin/destination -- not the
    /// pin route, which `TrackedTrainListItem` already covers, so this
    /// query deliberately does not add two more joins for
    /// `pin_origin_crs`/`pin_destination_crs`.
    origin_name: Option<String>,
    destination_name: Option<String>,
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
    custom_name: Option<String>,
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
    /// See `TicketListRow::origin_name`'s doc comment.
    pub origin_name: Option<String>,
    pub destination_name: Option<String>,
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
    pub custom_name: Option<String>,
}

/// Mirrors `routes/train.rs`'s `build_delay_repay_response` exactly (same
/// `match (operator, delay_minutes)` shape), so the two independently
/// computed estimates for the same `(ticket, tracked train)` pair can
/// never disagree.
fn build_ticket_list_item(row: TicketListRow) -> TicketListItem {
    let estimate = match (row.operator.as_deref(), row.delay_minutes) {
        (Some(operator), Some(delay_minutes)) => {
            delay_repay_rules::estimate_delay_repay(operator, delay_minutes)
        }
        _ => None,
    };
    let claim_url = row
        .operator
        .as_deref()
        .map(delay_repay_rules::claim_url_for)
        .unwrap_or(delay_repay_rules::GENERIC_CLAIM_URL);

    TicketListItem {
        id: row.id,
        tracked_train_id: row.tracked_train_id,
        operator: row.operator,
        ticket_type: row.ticket_type,
        origin_crs: row.origin_crs,
        destination_crs: row.destination_crs,
        origin_name: row.origin_name,
        destination_name: row.destination_name,
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
        custom_name: row.custom_name,
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
pub async fn list_tickets_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<TicketListItem>> {
    let rows = sqlx::query_as::<_, TicketListRow>(
        "SELECT t.id, t.tracked_train_id, t.operator, t.ticket_type, t.origin_crs, t.destination_crs, \
                so.name AS origin_name, sd.name AS destination_name, \
                t.source, t.created_at, \
                tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes, t.custom_name \
         FROM tracked_train_tickets t \
         LEFT JOIN tracked_trains tt ON tt.id = t.tracked_train_id \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         LEFT JOIN stations so ON so.crs = UPPER(t.origin_crs) \
         LEFT JOIN stations sd ON sd.crs = UPPER(t.destination_crs) \
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
            origin_name: Some("London Kings Cross".to_string()),
            destination_name: Some("Edinburgh Waverley".to_string()),
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
            custom_name: None,
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
            origin_name: Some("London Kings Cross".to_string()),
            destination_name: Some("Edinburgh Waverley".to_string()),
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
            custom_name: None,
        }
    }

    // Regression check that this mirrored implementation hasn't drifted
    // from routes/train.rs's build_delay_repay_response for the same
    // (operator, delay_minutes) pair -- see this function's own doc
    // comment for why the two must never disagree.
    #[test]
    fn matches_build_delay_repay_response_for_a_qualifying_dr30_delay() {
        let item = build_ticket_list_item(row(Some("LNER"), Some(45)));

        let estimate = item
            .estimate
            .expect("LNER + 45 minutes should clear the DR30 30-minute band");
        assert_eq!(estimate.scheme, "DR30");
        assert_eq!(estimate.percentage, 50);
        assert_eq!(
            item.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
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
        assert_eq!(
            item.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
        assert_eq!(item.disclaimer, delay_repay_rules::ROUTE_DISCLAIMER);
    }

    // A standalone ticket (Part A of this plan) must degrade gracefully,
    // not crash -- this is the direct regression check that
    // `list_tickets_for_user`'s `LEFT JOIN` and this function's `Option`
    // fields actually compose safely for a row with no owning tracked
    // train at all, not just no delay data yet.
    #[test]
    fn a_standalone_ticket_with_no_tracked_train_has_no_estimate_but_still_a_real_claim_link_and_disclaimer()
     {
        let item = build_ticket_list_item(standalone_row(Some("LNER")));

        assert_eq!(item.tracked_train_id, None);
        assert_eq!(item.service_date, None);
        assert_eq!(item.pin_origin_crs, None);
        assert_eq!(item.resolution_status, None);
        assert_eq!(item.status, None);
        assert_eq!(item.delay_minutes, None);
        assert_eq!(item.estimate, None);
        assert_eq!(
            item.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
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

    async fn seed_user(pool: &PgPool, user_id: &str) {
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind(format!("{user_id}@example.com"))
        .bind(user_id)
        .execute(pool)
        .await
        .expect("seed fixture user");
    }

    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM tracked_train_tickets WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tickets");
        sqlx::query("DELETE FROM tracked_trains WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tracked_trains");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    /// Minimal fixture row -- only the `NOT NULL` columns
    /// (`crates/api/migrations/20260828120000_train_tracking.sql:40-76`).
    async fn seed_tracked_train(pool: &PgPool, user_id: &str) -> i64 {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind("2026-09-02".parse::<chrono::NaiveDate>().unwrap())
        .bind("KGX")
        .bind("2026-09-02T09:00:00Z".parse::<DateTime<Utc>>().unwrap())
        .fetch_one(pool)
        .await
        .expect("insert fixture tracked_trains row");
        id
    }

    fn fixture_entry() -> TicketEntryRequest {
        TicketEntryRequest {
            operator: Some("LNER".to_string()),
            ticket_type: Some("single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "manual".to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_the_owner_can_delete_their_own_attached_ticket() {
        let pool = connect().await;
        seed_user(&pool, "TEST-TICKET-DELETE-OWNER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-TICKET-DELETE-OWNER").await;
        let ticket_id = create_ticket(
            &pool,
            Some(tracking_id),
            &fixture_entry(),
            "TEST-TICKET-DELETE-OWNER",
        )
        .await
        .expect("create fixture ticket");

        let deleted = delete_ticket(&pool, ticket_id, "TEST-TICKET-DELETE-OWNER")
            .await
            .expect("delete ticket");
        assert!(deleted);

        let gone = get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-OWNER")
            .await
            .expect("read ticket");
        assert!(
            gone.is_none(),
            "ticket row should be gone after the owner deletes it"
        );

        cleanup_user(&pool, "TEST-TICKET-DELETE-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_non_owner_cannot_delete_it_and_the_row_survives() {
        let pool = connect().await;
        seed_user(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
        seed_user(&pool, "TEST-TICKET-DELETE-OTHER").await;
        let ticket_id = create_ticket(
            &pool,
            None,
            &fixture_entry(),
            "TEST-TICKET-DELETE-REAL-OWNER",
        )
        .await
        .expect("create fixture ticket");

        let deleted = delete_ticket(&pool, ticket_id, "TEST-TICKET-DELETE-OTHER")
            .await
            .expect("delete ticket");
        assert!(!deleted);

        let still_there = get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-REAL-OWNER")
            .await
            .expect("read ticket");
        assert!(
            still_there.is_some(),
            "row should survive a non-owner's delete attempt"
        );

        cleanup_user(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
        cleanup_user(&pool, "TEST-TICKET-DELETE-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_nonexistent_id_returns_false() {
        let pool = connect().await;
        let deleted = delete_ticket(&pool, 99999999, "TEST-TICKET-DELETE-NOBODY")
            .await
            .expect("delete ticket");
        assert!(!deleted);
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_an_unattached_standalone_ticket_deletes_identically_to_an_attached_one()
    {
        let pool = connect().await;
        seed_user(&pool, "TEST-TICKET-DELETE-STANDALONE").await;
        // tracked_train_id: None -- a STANDALONE ticket. delete_ticket's
        // own WHERE clause never references this column, so this must
        // succeed identically to the attached case above.
        let ticket_id = create_ticket(
            &pool,
            None,
            &fixture_entry(),
            "TEST-TICKET-DELETE-STANDALONE",
        )
        .await
        .expect("create fixture ticket");

        let deleted = delete_ticket(&pool, ticket_id, "TEST-TICKET-DELETE-STANDALONE")
            .await
            .expect("delete ticket");
        assert!(deleted);

        let gone = get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-STANDALONE")
            .await
            .expect("read ticket");
        assert!(gone.is_none());

        cleanup_user(&pool, "TEST-TICKET-DELETE-STANDALONE").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_tracked_train -- --ignored --test-threads=1`"]
    async fn rename_tracked_train_the_owner_can_set_and_clear_a_custom_name() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TRAIN-OWNER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-RENAME-TRAIN-OWNER").await;

        let renamed = rename_tracked_train(
            &pool,
            tracking_id,
            "TEST-RENAME-TRAIN-OWNER",
            Some("My commute"),
        )
        .await
        .expect("rename tracked train");
        assert!(renamed);

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.custom_name, Some("My commute".to_string()));

        let cleared = rename_tracked_train(&pool, tracking_id, "TEST-RENAME-TRAIN-OWNER", None)
            .await
            .expect("clear custom name");
        assert!(cleared);

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.custom_name, None);

        cleanup_user(&pool, "TEST-RENAME-TRAIN-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_tracked_train -- --ignored --test-threads=1`"]
    async fn rename_tracked_train_a_non_owner_cannot_rename_it_and_the_row_survives() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TRAIN-REAL-OWNER").await;
        seed_user(&pool, "TEST-RENAME-TRAIN-OTHER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-RENAME-TRAIN-REAL-OWNER").await;

        let renamed = rename_tracked_train(
            &pool,
            tracking_id,
            "TEST-RENAME-TRAIN-OTHER",
            Some("Hijacked name"),
        )
        .await
        .expect("attempt rename as non-owner");
        assert!(!renamed);

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.custom_name, None);

        cleanup_user(&pool, "TEST-RENAME-TRAIN-REAL-OWNER").await;
        cleanup_user(&pool, "TEST-RENAME-TRAIN-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_ticket -- --ignored --test-threads=1`"]
    async fn rename_ticket_the_owner_can_set_and_clear_a_custom_name() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TICKET-OWNER").await;
        let ticket_id = create_ticket(&pool, None, &fixture_entry(), "TEST-RENAME-TICKET-OWNER")
            .await
            .expect("create fixture ticket");

        let renamed = rename_ticket(
            &pool,
            ticket_id,
            "TEST-RENAME-TICKET-OWNER",
            Some("Mum's ticket to Leeds"),
        )
        .await
        .expect("rename ticket");
        assert!(renamed);

        let ticket = get_ticket_owned(&pool, ticket_id, "TEST-RENAME-TICKET-OWNER")
            .await
            .expect("read ticket")
            .expect("ticket exists");
        assert_eq!(
            ticket.custom_name,
            Some("Mum's ticket to Leeds".to_string())
        );

        let cleared = rename_ticket(&pool, ticket_id, "TEST-RENAME-TICKET-OWNER", None)
            .await
            .expect("clear custom name");
        assert!(cleared);

        let ticket = get_ticket_owned(&pool, ticket_id, "TEST-RENAME-TICKET-OWNER")
            .await
            .expect("read ticket")
            .expect("ticket exists");
        assert_eq!(ticket.custom_name, None);

        cleanup_user(&pool, "TEST-RENAME-TICKET-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                rename_ticket -- --ignored --test-threads=1`"]
    async fn rename_ticket_a_non_owner_cannot_rename_it_and_the_row_survives() {
        let pool = connect().await;
        seed_user(&pool, "TEST-RENAME-TICKET-REAL-OWNER").await;
        seed_user(&pool, "TEST-RENAME-TICKET-OTHER").await;
        let ticket_id = create_ticket(
            &pool,
            None,
            &fixture_entry(),
            "TEST-RENAME-TICKET-REAL-OWNER",
        )
        .await
        .expect("create fixture ticket");

        let renamed = rename_ticket(
            &pool,
            ticket_id,
            "TEST-RENAME-TICKET-OTHER",
            Some("Hijacked name"),
        )
        .await
        .expect("attempt rename as non-owner");
        assert!(!renamed);

        cleanup_user(&pool, "TEST-RENAME-TICKET-REAL-OWNER").await;
        cleanup_user(&pool, "TEST-RENAME-TICKET-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_tracked_trains_for_user_resolves_the_station_name_join \
                -- --ignored`"]
    async fn list_tracked_trains_for_user_resolves_the_station_name_join() {
        // Proves the `LEFT JOIN stations ... ON so.crs = UPPER(...)` join
        // actually resolves -- and, critically, that it resolves for a
        // LOWER-CASE stored CRS, which is the case that silently breaks
        // without `UPPER` (Decision 3 of
        // docs/superpowers/plans/2026-09-02-frontend-ux-review-fixes.md).
        let pool = connect().await;
        let user_id = "TEST-STATION-NAME-JOIN-USER";
        seed_user(&pool, user_id).await;

        sqlx::query(
            "INSERT INTO stations (crs, name) VALUES ('ZQQ', 'Zedbury') \
             ON CONFLICT (crs) DO UPDATE SET name = EXCLUDED.name",
        )
        .execute(&pool)
        .await
        .expect("seed fixture station");

        async fn seed_with_origin(pool: &PgPool, user_id: &str, origin_crs: &str) -> i64 {
            let (id,): (i64,) = sqlx::query_as(
                "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
                 VALUES ($1, $2, $3, $4) RETURNING id",
            )
            .bind(user_id)
            .bind("2026-09-02".parse::<chrono::NaiveDate>().unwrap())
            .bind(origin_crs)
            .bind("2026-09-02T09:00:00Z".parse::<DateTime<Utc>>().unwrap())
            .fetch_one(pool)
            .await
            .expect("insert fixture tracked_trains row");
            id
        }

        let uppercase_id = seed_with_origin(&pool, user_id, "ZQQ").await;
        let lowercase_id = seed_with_origin(&pool, user_id, "zqq").await;
        // No `stations` row for this code at all -- proves the `LEFT JOIN`,
        // not `JOIN`, guarantee: the train must still come back, just with
        // `pin_origin_name: None`.
        let unrecognised_id = seed_with_origin(&pool, user_id, "ZZQ").await;

        let rows = list_tracked_trains_for_user(&pool, user_id)
            .await
            .expect("list tracked trains");
        let by_id = |id: i64| rows.iter().find(|r| r.id == id).expect("row present");

        assert_eq!(
            by_id(uppercase_id).pin_origin_name,
            Some("Zedbury".to_string())
        );
        assert_eq!(
            by_id(lowercase_id).pin_origin_name,
            Some("Zedbury".to_string()),
            "a lower-case stored CRS must still resolve a name -- this is exactly what UPPER() \
             guards against silently breaking"
        );
        assert_eq!(by_id(unrecognised_id).pin_origin_name, None);

        sqlx::query("DELETE FROM stations WHERE crs = 'ZQQ'")
            .execute(&pool)
            .await
            .expect("cleanup fixture station");
        cleanup_user(&pool, user_id).await;
    }

    // --- upsert_train_event's two-field guard (Task 9) -----------------------
    //
    // Audit performed before writing these: grepped
    // resolved_train_uid|resolved_train_id|resolution_status across
    // crates/api/ and crates/trust-consumer/, then read this module's own
    // test coverage end to end. Finding: no existing test anywhere calls
    // upsert_train_event at all -- the only matches live in
    // crates/trust-consumer/src/process.rs's test module, and every one of
    // them asserts what run_once *produces* on the outgoing
    // TrainMovementEventMessage, never what this function *does* with it
    // once posted. So relaxing the guard below cannot break an existing
    // test; these are the first direct tests for this function.

    fn fixture_event(tracked_train_id: i64, dedup_key: &str) -> common::TrainMovementEventMessage {
        common::TrainMovementEventMessage {
            tracked_train_id,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: dedup_key.to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("72410".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: Some("2026-09-05T18:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-05T18:15:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("CRE".to_string()),
            eta_next: None,
            eta_source: None,
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_train_event -- --ignored --test-threads=1`"]
    async fn upsert_train_event_with_only_resolved_train_id_resolves_and_preserves_the_existing_train_uid()
     {
        let pool = connect().await;
        let user_id = "TEST-UPSERT-SCHEDULE-MATCHED";
        seed_user(&pool, user_id).await;
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, train_uid, resolution_status) \
             VALUES ($1, $2, $3, $4, $5, 'schedule_matched') RETURNING id",
        )
        .bind(user_id)
        .bind("2026-09-05".parse::<chrono::NaiveDate>().unwrap())
        .bind("EUS")
        .bind("2026-09-05T18:15:00Z".parse::<DateTime<Utc>>().unwrap())
        .bind("C88888") // schedule-matched train_uid, no train_id yet
        .fetch_one(&pool)
        .await
        .expect("seed schedule-matched tracked_trains row");

        let mut event = fixture_event(tracked_train_id, "dedup-only-train-id");
        event.resolved_train_uid = None; // the exact gap this task closes
        event.resolved_train_id = Some("221832406".to_string());

        upsert_train_event(&pool, &event).await.expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracked_train_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.resolution_status, "resolved");
        assert_eq!(state.train_id, Some("221832406".to_string()));
        assert_eq!(
            state.train_uid,
            Some("C88888".to_string()),
            "the schedule-matched train_uid must survive, COALESCE-preserved, not overwritten with NULL"
        );

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_train_event -- --ignored --test-threads=1`"]
    async fn upsert_train_event_with_both_fields_still_sets_both_train_uid_and_train_id() {
        let pool = connect().await;
        let user_id = "TEST-UPSERT-BOTH-FIELDS";
        seed_user(&pool, user_id).await;
        let tracking_id = seed_tracked_train(&pool, user_id).await;

        let mut event = fixture_event(tracking_id, "dedup-both-fields");
        event.resolved_train_uid = Some("C21373".to_string());
        event.resolved_train_id = Some("221832406".to_string());

        upsert_train_event(&pool, &event).await.expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.resolution_status, "resolved");
        assert_eq!(state.train_uid, Some("C21373".to_string()));
        assert_eq!(state.train_id, Some("221832406".to_string()));

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_train_event -- --ignored --test-threads=1`"]
    async fn upsert_train_event_with_neither_field_leaves_resolution_status_and_train_uid_untouched()
     {
        let pool = connect().await;
        let user_id = "TEST-UPSERT-NEITHER-FIELD";
        seed_user(&pool, user_id).await;
        let tracking_id = seed_tracked_train(&pool, user_id).await;

        let event = fixture_event(tracking_id, "dedup-neither-field"); // both None, the default

        upsert_train_event(&pool, &event).await.expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(
            state.resolution_status, "pending",
            "resolution_status must not move without at least resolved_train_id"
        );
        assert_eq!(state.train_uid, None);
        assert_eq!(state.train_id, None);
        // The movement/current-state writes still happen unconditionally --
        // this guard only ever gates the tracked_trains UPDATE.
        assert_eq!(state.status, Some("en_route".to_string()));

        cleanup_user(&pool, user_id).await;
    }
}
