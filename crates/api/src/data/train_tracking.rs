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
        "INSERT INTO train_subscriptions \
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

/// Creates a subscription for an identity that is ALREADY fully known
/// (the NR-primary path, docs/superpowers/specs/2026-09-06-shared-train-identity-design.md
/// §4) -- no `pending`/`schedule_matched` waypoint at all, unlike
/// `create_pin`'s legacy CRS+time flow. `pin_*` columns are sourced live
/// from the `trains` row itself via `INSERT ... SELECT`, and come back
/// `NULL` if that row has no schedule data yet (the accepted gap named in
/// the design spec's §1) -- safe since Task 20's own migration relaxed
/// their `NOT NULL` constraint.
///
/// Deliberately does NOT also write the matched `train_uid` onto this row's
/// own (legacy) `tracked_trains.train_uid` column, even though `trains.train_uid`
/// is right there in the same `SELECT`. Two different users tracking the
/// SAME real train via this endpoint -- this endpoint's own headline
/// scenario -- would both resolve to the identical `(train_uid,
/// service_date)` pair; `tracked_trains` still carries a real
/// `UNIQUE (train_uid, service_date) WHERE train_uid IS NOT NULL` index
/// (`tracked_trains_resolved_identity`,
/// `20260828120000_train_tracking.sql:85`) left over from the old
/// one-row-per-physical-train assumption the shared-`trains` table (Task 1)
/// was built specifically to retire -- the SECOND user's `INSERT` would hit
/// that constraint and fail outright. That index is scheduled to be dropped
/// entirely in Task 22 (alongside the whole legacy `train_uid` column it
/// guards), not before -- narrowing it here would be a bigger, separately-
/// reviewable change than this task's own scope. As of Task 21,
/// `trust-consumer`'s `by_train_uid` direct-Activation-match fast path
/// (Task 16) DOES see NR-primary subscriptions created by this function --
/// `list_active_tracked_trains` now reads `train_uid` via a `LEFT JOIN
/// trains` rather than this table's own (never-written-by-this-function)
/// column, and `trains_id` (unlike `train_uid`) IS written here,
/// immediately, so no further change to this function was needed for that
/// join to pick this row up automatically. See
/// `list_active_tracked_trains_surfaces_train_uid_for_every_subscriber_sharing_a_trains_id`
/// (this module's own `db_tests`) for the end-to-end proof, including for
/// TWO subscribers sharing one `trains_id` -- this endpoint's own headline
/// scenario. A bare-`train_uid`-no-schedule-data row (the design's own
/// accepted §1 gap) still has no route to trust-consumer's CRS+time
/// heuristic (nothing to match against), but the `by_train_uid` fast path
/// now covers it too.
///
/// Two more downstream correctness fixes this task's own migration made
/// necessary, neither mentioned in the original brief text, both verified
/// by tracing every reader of `tracked_trains.pin_origin_crs`/
/// `pin_scheduled_departure`: `common::TrackedTrainRef`/this file's own
/// `TrackedTrainRow` had those two fields typed as plain `String`/
/// `DateTime<Utc>` (never `Option`) -- with the columns now nullable, a
/// bare-`train_uid`-no-schedule NR-primary row would fail to decode via
/// `list_active_tracked_trains` (used by `trust-consumer`'s periodic
/// reference reload), erroring that ENTIRE reload, not just this one row.
/// Both are now `Option` (see their own doc comments). Second,
/// `list_pending_pins_for_schedule_match`'s sweep was guarded only on
/// `WHERE train_uid IS NULL AND resolution_status = 'pending'` -- since
/// this function never sets `train_uid`, a fresh NR-primary row (still
/// `'pending'` by this table's own `DEFAULT`) would be swept up by the
/// periodic schedule-match job despite already having a known `trains_id`,
/// hitting the exact same `PendingSchedulePin` decode failure for a
/// bare-`train_uid`-no-schedule row. Its `WHERE` clause now also requires
/// `trains_id IS NULL` -- a no-op for every legacy row (which never has one
/// without also having the other) and the correct exclusion for this new
/// NR-primary shape.
///
/// **Idempotent by `(user_id, trains_id)`.** A second call for the same
/// user and the same shared train returns that user's EXISTING subscription
/// id rather than inserting another row -- so a user who clicks "Track this
/// train" twice (a repeat click, a back-navigate, a second tab) ends up
/// with one subscription, one `/track/mine` entry and one notification
/// stream, and is navigated to the same `/train/by-id/{trackingId}` both
/// times.
///
/// Scoped to ONE user: two different users tracking the same physical train
/// still get two independent subscriptions, which is this endpoint's own
/// headline scenario and the entire reason the shared `trains` table
/// exists.
///
/// Deliberately NOT backed by a `UNIQUE (user_id, trains_id)` index, and
/// this is a considered rejection rather than an oversight. Four unrelated
/// paths already do a bare `UPDATE train_subscriptions SET trains_id = $2
/// WHERE id = $1` (`data/schedule_matching.rs`'s `attempt_schedule_match`,
/// `data/trust_event_backlog_match.rs` in two places, and this file's own
/// live-resolution write), and `create_pin` above has never deduplicated
/// legacy CRS+time pins -- so a user holding two legacy pins that later
/// resolve to the same physical train is reachable today, and a global
/// unique index would turn each of those `UPDATE`s into a hard failure
/// inside schedule matching, backlog matching and live TRUST resolution.
///
/// The honest residual limitation, stated rather than papered over: under
/// READ COMMITTED, two genuinely simultaneous in-flight calls can both
/// observe no existing row and both insert. This closes the ordinary
/// repeat-click case, not a true concurrent double-submit; the frontend
/// closes that one the way every other mutating control in this app does,
/// by disabling the button while its request is in flight
/// (`frontend/components/TrackThisTrainButton.tsx`).
pub async fn create_subscription_for_train(
    pool: &PgPool,
    trains_id: i64,
    user_id: &str,
) -> anyhow::Result<i64> {
    // One statement, not a SELECT-then-INSERT round trip: the `inserted`
    // CTE's `NOT EXISTS (SELECT 1 FROM existing)` guard means the INSERT
    // never fires when a subscription is already there, and the final
    // UNION ALL yields exactly one row either way -- so `fetch_one` still
    // errors (RowNotFound) for a `trains_id` that names no `trains` row,
    // exactly as the previous plain `INSERT ... SELECT` did.
    let row: (i64,) = sqlx::query_as(
        "WITH existing AS ( \
             SELECT id FROM train_subscriptions \
             WHERE user_id = $1 AND trains_id = $2 \
             ORDER BY id LIMIT 1 \
         ), \
         inserted AS ( \
             INSERT INTO train_subscriptions \
                 (user_id, trains_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs) \
             SELECT $1, tr.id, tr.service_date, tr.origin_crs, tr.scheduled_departure, tr.destination_crs \
             FROM trains tr \
             WHERE tr.id = $2 AND NOT EXISTS (SELECT 1 FROM existing) \
             RETURNING id \
         ) \
         SELECT id FROM existing UNION ALL SELECT id FROM inserted",
    )
    .bind(user_id)
    .bind(trains_id)
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
    /// `Option`, not `String` -- as of Task 20's `create_subscription_for_train`
    /// (the NR-primary path), a row can legitimately have `NULL` here (the
    /// design spec's own accepted §1 gap: a bare `train_uid` with no
    /// schedule match yet). See `common::TrackedTrainRef::pin_origin_crs`'s
    /// own doc comment -- this row shape's whole reason to exist is
    /// carrying that value through to it unchanged.
    pin_origin_crs: Option<String>,
    /// See `pin_origin_crs`'s own doc comment on this same struct.
    pin_scheduled_departure: Option<DateTime<Utc>>,
    resolution_status: String,
    train_uid: Option<String>,
    train_id: Option<String>,
    trains_id: Option<i64>,
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
            trains_id: row.trains_id,
        }
    }
}

/// What `trust-consumer` needs for its periodic reference reload (Task
/// 14): pending pins to attempt resolving, and already-resolved ones to
/// recognize incoming TRUST messages against, after a restart or on its
/// periodic reload. "Active" excludes `completed`/`cancelled` rows in
/// `train_current_state` and `unresolved` rows in `tracked_trains` --
/// there is nothing further for trust-consumer to do with either.
///
/// `train_uid`/`train_id` now come from a `LEFT JOIN trains`, not
/// `tracked_trains`' own (as of this task, no-longer-written) columns --
/// this is the change that finally lets an NR-primary subscription
/// (Task 20's `create_subscription_for_train`, which sets `trains_id`
/// immediately but deliberately never touches this table's own legacy
/// `train_uid` column) populate `trust-consumer`'s `by_train_uid`
/// direct-match fast path (Task 16) at all, including for a SECOND
/// subscriber sharing the same physical train's `trains_id` -- exactly
/// the scenario the unique `tracked_trains_resolved_identity` index made
/// impossible to express via `tracked_trains.train_uid` itself. `LEFT
/// JOIN`, not `JOIN`: a row whose `trains_id` is still `NULL` (no
/// schedule/backlog/live match has ever run) must still come back, just
/// with `train_uid`/`train_id` both `None` -- `TrackedTrainRow`/
/// `TrackedTrainRef` already type both fields `Option` for exactly this
/// reason.
pub async fn list_active_tracked_trains(pool: &PgPool) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let rows = sqlx::query_as::<_, TrackedTrainRow>(
        "SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tr.train_uid, tr.train_id, tt.trains_id \
         FROM train_subscriptions tt \
         LEFT JOIN trains tr ON tr.id = tt.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
         WHERE tt.resolution_status != 'unresolved' \
           AND (cs.status IS NULL OR cs.status NOT IN ('completed', 'cancelled'))",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(TrackedTrainRef::from).collect())
}

/// Writes one TRUST-derived event into the SHARED, per-physical-train
/// tables. Callable for ANY `trains_id`, regardless of whether any
/// `tracked_trains` row (subscription) references it at all -- this is
/// the primary write path once trust-backlog-consumer becomes the primary
/// movement-event writer (Task 14), and it's also what `upsert_train_event`
/// below now delegates to for the legacy per-subscription path.
/// `event.tracked_train_id` is ignored here on purpose -- this function's
/// entire point is to not require one.
///
/// **Event-time monotonicity guard on the `train_current_state` write.**
/// This function is the one place `trust-consumer`'s live write path and
/// `trust-backlog-consumer`'s `ingest_shared_movement` write path converge
/// (see `crates/api/src/data/trust_event_backlog.rs`) -- both are
/// independent, continuously-running processes that can write the same
/// `trains_id` in either order, with no coordination between them. Without
/// a guard, whichever process's `UPDATE` commits last always wins,
/// regardless of which one actually carries the more recent real-world
/// event -- a lagging writer's stale update can silently overwrite a
/// fresher one. Full analysis, including why this is a real (not
/// theoretical) production risk and why the guard lives here rather than
/// in either caller:
/// `docs/superpowers/specs/2026-09-07-shared-train-status-write-race-design.md`
/// (Option C).
///
/// The mechanism is **event-time monotonicity, not wall-clock/commit
/// order**: `event_time` below is `COALESCE(event.actual_timestamp,
/// event.planned_timestamp)` -- the incoming event's own real-world
/// timestamp, not `NOW()` (which `updated_at` already records, and which
/// provides no protection at all since it always advances forward
/// regardless of which writer produced it). The `ON CONFLICT DO UPDATE`'s
/// `WHERE EXCLUDED.event_time IS NULL OR EXCLUDED.event_time >=
/// train_current_state.event_time OR train_current_state.event_time IS
/// NULL` clause makes the whole `UPDATE` a no-op whenever the incoming
/// event is OLDER than what's already stored, independent of commit order
/// between the two writers -- so this is NOT dead code or a redundant
/// restatement of the `ON CONFLICT` target; it is the actual fix.
///
/// **A no-timestamp incoming event always applies, but never regresses a
/// known `event_time`.** Not every message this function is called for
/// actually carries a timestamp -- e.g. a Cancellation with a missing or
/// malformed `canx_timestamp` (both `crates/trust-consumer/src/process.rs`
/// and `crates/trust-backlog-consumer/src/process.rs` build a
/// Cancellation's `actual_timestamp` from
/// `canx_timestamp.as_deref().and_then(parse_epoch_millis)`, `None` on
/// either a missing or an unparseable value, with `planned_timestamp`
/// always `None` for that message shape) -- so `event_time` here can
/// legitimately be `NULL` even once the stored row's `event_time` is
/// already known. The first `EXCLUDED.event_time IS NULL` branch exists
/// specifically for that case: an event we have no real-world time for is
/// still the best information available and must still apply (matching
/// this function's pre-guard behaviour for exactly that case), rather than
/// being silently and PERMANENTLY dropped -- without this branch, `NULL >=
/// x` is SQL's UNKNOWN, `train_current_state.event_time IS NULL` is FALSE
/// once a real event_time is stored, and `UNKNOWN OR FALSE` never
/// satisfies `WHERE`, so every future write to that `trains_id` (including
/// ones that DO carry a real, newer timestamp) would silently no-op
/// forever -- worse than having no guard at all. Symmetrically, `SET
/// event_time = COALESCE(EXCLUDED.event_time, train_current_state.event_time)`
/// (rather than a bare `EXCLUDED.event_time`) means a no-timestamp write
/// updates every other column normally but leaves the stored `event_time`
/// exactly as it was -- if it instead clobbered `event_time` back to
/// `NULL`, that would re-open the `train_current_state.event_time IS NULL`
/// branch for every subsequent call, permanently defeating the guard for
/// this `trains_id` after just one no-timestamp event.
pub async fn upsert_train_movement(
    pool: &PgPool,
    trains_id: i64,
    event: &TrainMovementEventMessage,
) -> anyhow::Result<()> {
    let event_time = event.actual_timestamp.or(event.planned_timestamp);

    sqlx::query(
        "INSERT INTO train_movement_events \
            (trains_id, dedup_key, msg_type, event_type, loc_stanox, loc_crs, \
             planned_timestamp, actual_timestamp, variation_status, raw_body) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (trains_id, dedup_key) WHERE trains_id IS NOT NULL DO NOTHING",
    )
    .bind(trains_id)
    .bind(&event.dedup_key)
    .bind(&event.msg_type)
    .bind(&event.event_type)
    .bind(&event.loc_stanox)
    .bind(&event.loc_crs)
    .bind(event.planned_timestamp)
    .bind(event.actual_timestamp)
    .bind(&event.variation_status)
    .bind(&event.raw_body)
    .execute(pool)
    .await?;

    sqlx::query(
        "INSERT INTO train_current_state \
            (trains_id, status, last_reported_location, last_event_type, \
             delay_minutes, next_calling_point, eta_next, eta_source, event_time, updated_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW()) \
         ON CONFLICT (trains_id) WHERE trains_id IS NOT NULL DO UPDATE SET \
            status                  = EXCLUDED.status, \
            last_reported_location  = EXCLUDED.last_reported_location, \
            last_event_type         = EXCLUDED.last_event_type, \
            delay_minutes            = EXCLUDED.delay_minutes, \
            next_calling_point       = EXCLUDED.next_calling_point, \
            eta_next                 = EXCLUDED.eta_next, \
            eta_source               = EXCLUDED.eta_source, \
            event_time               = COALESCE(EXCLUDED.event_time, train_current_state.event_time), \
            updated_at               = NOW() \
         WHERE EXCLUDED.event_time IS NULL \
            OR EXCLUDED.event_time >= train_current_state.event_time \
            OR train_current_state.event_time IS NULL",
    )
    .bind(trains_id)
    .bind(&event.status)
    .bind(&event.last_reported_location)
    .bind(&event.last_event_type)
    .bind(event.delay_minutes)
    .bind(&event.next_calling_point)
    .bind(event.eta_next)
    .bind(&event.eta_source)
    .bind(event_time)
    .execute(pool)
    .await?;

    Ok(())
}

/// Legacy per-subscription resolution flip only -- as of this task, it no
/// longer writes `train_movement_events`/`train_current_state` itself
/// (that's `upsert_train_movement`'s job now). Advances
/// `tracked_trains.resolution_status`, mirrors the resolution onto the
/// shared `trains` row (same dual-write Task 5 introduced), and returns
/// the resolved `trains_id` so the caller can feed the same event into
/// `upsert_train_movement`. Returns `None` only in the accepted-gap case:
/// no `trains_id` was already known AND this call carries no
/// `resolved_train_uid` either (this process never saw the Activation) --
/// the pin still flips to `'resolved'` for this user's own tracking
/// purposes, but no shared `trains` row can be created or updated without
/// a known identity.
///
/// As of Task 22 (Step D's final cutover), this `UPDATE` writes ONLY
/// `resolution_status` -- `tracked_trains.train_uid`/`train_id`/
/// `resolved_at` no longer exist as columns at all (dropped by this same
/// task's migration), so the old `train_uid = COALESCE($2, train_uid),
/// train_id = $3, ..., resolved_at = NOW()` write this UPDATE used to do
/// is gone entirely, not merely stopped. This is also the direct fix for
/// the risk Task 21's review flagged: that old per-subscription
/// `train_uid` write could collide with `tracked_trains_resolved_identity`
/// (a `UNIQUE (train_uid, service_date) WHERE train_uid IS NOT NULL`
/// index) the moment two subscribers shared one physical train (Task 20's
/// own headline scenario) and a process restart re-delivered an Activation
/// for the second one -- both writes would race to set the same
/// `(train_uid, service_date)` pair on two different `tracked_trains` rows.
/// That index is dropped by this same migration, and this UPDATE no longer
/// attempts the write that could have hit it -- the shared identity link
/// lives exclusively on `trains_id` from here on.
///
/// Because the returned row no longer carries a `train_uid` column to fall
/// back on, `trains_id` derivation below now reads directly off THIS
/// call's own `resolved_train_uid` parameter instead of a value the
/// `UPDATE` read back post-write. A previously-schedule-matched pin no
/// longer needs that fallback anyway: schedule matching (Task 3) already
/// links `trains_id` directly on `tracked_trains` the moment it succeeds,
/// so `existing_trains_id` (the `trains_id` column itself) is already
/// `Some` by the time any live-TRUST resolution reaches this function for
/// such a pin.
async fn flip_legacy_resolution(
    pool: &PgPool,
    tracked_train_id: i64,
    resolved_train_uid: Option<&str>,
    resolved_train_id: &str,
) -> anyhow::Result<Option<i64>> {
    let row: Option<(Option<i64>, chrono::NaiveDate)> = sqlx::query_as(
        "UPDATE train_subscriptions SET resolution_status = 'resolved' \
         WHERE id = $1 RETURNING trains_id, service_date",
    )
    .bind(tracked_train_id)
    .fetch_optional(pool)
    .await?;
    let Some((existing_trains_id, service_date)) = row else {
        return Ok(None);
    };

    let trains_id = match (existing_trains_id, resolved_train_uid) {
        (Some(id), _) => Some(id),
        (None, Some(train_uid)) => {
            let id =
                crate::data::trains::find_or_create_train(pool, train_uid, service_date).await?;
            sqlx::query("UPDATE train_subscriptions SET trains_id = $2 WHERE id = $1")
                .bind(tracked_train_id)
                .bind(id)
                .execute(pool)
                .await?;
            Some(id)
        }
        (None, None) => None,
    };
    if let Some(id) = trains_id {
        crate::data::trains::mark_train_resolved(pool, id, resolved_train_id).await?;
    }
    Ok(trains_id)
}

/// Idempotent, same overall contract as before this task: resolves the pin
/// (if `resolved_train_id` is `Some`) and writes the shared movement/
/// current-state tables. As of this task, that write ALWAYS goes through
/// [`upsert_train_movement`], keyed on `trains_id` -- never directly on
/// `tracked_train_id` -- so an event for a subscription whose identity is
/// still entirely unknown (no schedule/backlog match ever ran, and this
/// call itself carries no `resolved_train_uid`) has nothing to key a
/// shared-table write on and is dropped with a warning, matching the
/// accepted gap `flip_legacy_resolution` documents.
pub async fn upsert_train_event(
    pool: &PgPool,
    event: &TrainMovementEventMessage,
) -> anyhow::Result<()> {
    let resolved_trains_id = match &event.resolved_train_id {
        Some(train_id) => {
            flip_legacy_resolution(
                pool,
                event.tracked_train_id,
                event.resolved_train_uid.as_deref(),
                train_id,
            )
            .await?
        }
        None => None,
    };

    let trains_id = match resolved_trains_id {
        Some(id) => Some(id),
        None => sqlx::query_scalar::<_, Option<i64>>(
            "SELECT trains_id FROM train_subscriptions WHERE id = $1",
        )
        .bind(event.tracked_train_id)
        .fetch_optional(pool)
        .await?
        .flatten(),
    };

    match trains_id {
        Some(trains_id) => upsert_train_movement(pool, trains_id, event).await?,
        None => {
            tracing::warn!(
                tracked_train_id = event.tracked_train_id,
                "no trains_id known yet for this subscription; movement event dropped \
                 from the shared store until its identity is resolved"
            );
        }
    }

    Ok(())
}

/// As of this task, this ONLY flips `resolution_status` -- every schedule
/// column this used to also write now lives exclusively on the shared
/// `trains` row (`schedule_matching::attempt_schedule_match`'s own
/// `find_or_create_train_with_schedule_match` call, Task 3). Guarded on
/// `trains_id IS NULL` rather than the old `train_uid IS NULL` -- since
/// Task 8's read cutover, `tracked_trains.train_uid` is no longer the
/// signal anything trusts for "has this pin been schedule-matched yet."
/// Still safe to call from BOTH the synchronous pin-creation path and the
/// periodic sweep without a race clobbering a row that has since moved on
/// (a live TRUST Movement resolved it first -- setting `trains_id` via
/// `flip_legacy_resolution` -- or an earlier sweep tick already matched
/// it) -- `rows_affected() == 0` in either of those cases is not an error,
/// just a no-op, which is why this returns `bool` rather than erroring on
/// zero rows affected.
pub async fn apply_schedule_match(pool: &PgPool, tracked_train_id: i64) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE train_subscriptions SET resolution_status = 'schedule_matched' \
         WHERE id = $1 AND trains_id IS NULL AND resolution_status = 'pending'",
    )
    .bind(tracked_train_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Row shape for `list_pending_pins_for_schedule_match`'s query -- every
/// still-`pending`, never-schedule-matched row, the periodic sweep's own
/// input set (Decision 3's "also run this same attempt periodically").
///
/// `pin_origin_crs`/`pin_scheduled_departure` are `Option`, not bare
/// `String`/`DateTime<Utc>`, even though the query below already filters
/// NULLs out of its `WHERE` clause: `train_subscriptions.trains_id` is
/// `ON DELETE SET NULL` (`20260906100000_trains.sql:38`), so a still-
/// `pending` NR-primary subscription (`create_subscription_for_train`,
/// whose `pin_*` columns are `NULL` by design until a schedule match ever
/// happens -- the design's own accepted §1 gap) can have its `trains_id`
/// nulled out from under it once `aggregator::queries::prune_trains`
/// deletes the `trains` row it pointed at, landing it right back in this
/// query's result set with NULL `pin_*` columns. Decoding those as
/// non-`Option` used to make sqlx error on EVERY row of EVERY sweep tick,
/// forever, the moment a single row reached that state -- not just fail to
/// process that one row. See
/// `list_pending_pins_for_schedule_match_excludes_a_pruned_nr_primary_row_with_null_pins`
/// in this module's own `db_tests`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PendingSchedulePin {
    pub id: i64,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: Option<String>,
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
}

/// Every row the periodic schedule-match sweep should retry: still
/// `pending` AND still lacking a `trains_id` -- a `schedule_matched` row
/// (or one resolved via live TRUST) already has one and is excluded, same
/// as a `resolved`/`unresolved` row. As of this task, `trains_id IS NULL`
/// is the ONLY identity guard here (the old, redundant `train_uid IS NULL`
/// was dropped along with `apply_schedule_match`'s own write of that
/// column -- `resolution_status = 'pending'` alone already excluded every
/// `schedule_matched` row, since `apply_schedule_match` always flips both
/// together in the same `UPDATE`).
///
/// `trains_id IS NULL` is a no-op for every legacy row (which never has a
/// `trains_id` without also going through `apply_schedule_match`/live
/// resolution first, both of which also flip `resolution_status` away from
/// `'pending'`), but load-bearing as of Task 20's NR-primary path
/// (`create_subscription_for_train`): that function sets `trains_id`
/// immediately but leaves `resolution_status` at its `'pending'` default,
/// so such a row would otherwise be swept into a schedule-match attempt
/// that exists only to *discover* a `trains_id`, which this row already
/// has -- pointlessly.
///
/// **Re-examined for review finding I1, and deliberately left as it is.**
/// The question was whether this should instead pick up NR-primary rows
/// whose linked `trains` row still lacks schedule data. It should not, for
/// a concrete reason rather than a stylistic one: this sweep's only tool is
/// `attempt_schedule_match`, which is keyed on `(origin CRS, departure
/// time)` -- exactly the pair such a row does not have and cannot derive.
/// Widening the `WHERE` would select rows the sweep can do nothing with.
///
/// **Re-examined again for the finding that `trains_id IS NULL` alone is
/// NOT sufficient to guarantee non-NULL `pin_*` columns.** `trains_id` is
/// `ON DELETE SET NULL` (`20260906100000_trains.sql:38`): once
/// `aggregator::queries::prune_trains` deletes a `trains` row, any
/// still-`pending` subscription pointing at it (an NR-primary row that
/// never got a schedule match, e.g. a train that never ran) has its
/// `trains_id` nulled out and reappears in this very query's result --
/// now indistinguishable, by `trains_id` alone, from a fresh NR-primary
/// row, but with `pin_origin_crs`/`pin_scheduled_departure` NULL. The
/// `WHERE` clause below now also excludes those explicitly (rather than
/// relying solely on `PendingSchedulePin`'s fields being `Option` to avoid
/// a decode error), since this sweep has nothing to do with such a row
/// either way -- same reasoning as the paragraph above, just reached via a
/// different route into this table's state space. See
/// `list_pending_pins_for_schedule_match_excludes_a_pruned_nr_primary_row_with_null_pins`.
///
/// What those rows get instead:
/// * LIVE data -- `trust-consumer` sees them through
///   `list_active_tracked_trains`' `LEFT JOIN trains`, matches their
///   `train_uid` straight off an Activation, and their movements flow
///   normally. Proven end to end by
///   `an_nr_primary_subscription_receives_live_movement_events` in this
///   module's own `db_tests`, not assumed.
/// * HISTORICAL data -- `routes::train::enrich_shared_train` replays the
///   retained `trust_event_backlog` at track time, and uses the replayed
///   origin departure's own `(CRS, time)` to run a real schedule match
///   (`schedule_matching::attempt_schedule_match_for_shared_train`).
///
/// The one residual gap, named rather than hidden: a train tracked
/// NR-primary that has NOT yet run (nothing in the backlog) acquires
/// schedule data only if something else supplies a `(CRS, time)` for it
/// later -- another subscriber's legacy pin, or a re-track. Live TRUST
/// still resolves its `train_id` and movements; it is only origin/
/// destination/calling points that stay `NULL`. Closing that needs a
/// schedule lookup keyed on `train_uid` alone, which no index in this
/// codebase supports today; out of scope for this fix.
pub async fn list_pending_pins_for_schedule_match(
    pool: &PgPool,
) -> anyhow::Result<Vec<PendingSchedulePin>> {
    let rows = sqlx::query_as::<_, PendingSchedulePin>(
        "SELECT id, service_date, pin_origin_crs, pin_scheduled_departure \
         FROM train_subscriptions WHERE trains_id IS NULL AND resolution_status = 'pending' \
         AND pin_origin_crs IS NOT NULL AND pin_scheduled_departure IS NOT NULL",
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
    /// `Option`, not `String`. `20260906130000_nullable_pin_columns.sql`
    /// dropped this column's `NOT NULL`, and
    /// `create_subscription_for_train` (the NR-primary path behind
    /// `POST /Train/by-uid/{uid}/{date}/track`) sources it via
    /// `INSERT ... SELECT` from the linked `trains` row -- which leaves it
    /// `NULL` whenever that row has no schedule data yet. That is the
    /// DEFAULT outcome of the new endpoint, not an edge case, so decoding
    /// this as a bare `String` made `GET /Train/{trackingId}` fail outright
    /// ("unexpected null; try decoding as an Option") for exactly the
    /// subscriptions this whole redesign exists to create. See
    /// `get_by_tracking_id_returns_a_row_with_null_pins_for_an_nr_primary_subscription`.
    pub pin_origin_crs: Option<String>,
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
    /// The matched schedule's own terminus CRS (Decision 3 step 4 of
    /// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md),
    /// `None` until schedule-matched (or if the terminus TIPLOC never
    /// resolved to a CRS -- see `schedule_matching::attempt_schedule_match`).
    pub schedule_destination_crs: Option<String>,
    /// See `pin_origin_name`'s own doc comment -- same
    /// `LEFT JOIN stations` mechanism, joined on `schedule_destination_crs`.
    pub schedule_destination_name: Option<String>,
    /// Opaque JSONB relay of the matched entry's calling points, already
    /// camelCase-shaped at write time
    /// (`schedule_matching::ScheduleCallingPointDto`) -- this crate does
    /// not deserialize it again on the way out.
    pub schedule_calling_points: Option<serde_json::Value>,
    pub status: Option<String>,
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    pub delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
    pub eta_next: Option<DateTime<Utc>>,
    pub eta_source: Option<String>,
    pub custom_name: Option<String>,
    /// The shared `trains` row's own surrogate key, needed to read
    /// `train_movement_events` for this train's live overlay -- internal
    /// plumbing for `routes::train`'s journey-stops attachment, never sent
    /// to the frontend (this struct already uses a *different* `id` for
    /// the tracking id, so exposing a second, differently-scoped `id`-like
    /// field on the wire would repeat exactly the confusion
    /// `PublicTrainState::trains_id`'s own doc comment describes).
    #[serde(skip_serializing)]
    pub trains_id: Option<i64>,
    /// The merged scheduled-timetable + live-overlay stop list -- `None`
    /// until `train_uid` is known, or if neither backing source has
    /// anything for this train (see
    /// docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md
    /// §1). Populated by `routes::train`'s handlers AFTER this struct is
    /// read from the DB (same "read row, then overlay a computed field"
    /// pattern `blend_darwin_eta` already uses for `eta_next`/`eta_source`
    /// on this same struct) -- never selected directly by
    /// `TRACKED_TRAIN_STATE_SELECT`, hence `#[sqlx(skip)]`. NOT
    /// `#[sqlx(default)]`: the derived `FromRow` impl still emits a
    /// `try_get::<#ty, _>(..)` call for a `#[sqlx(default)]` field and only
    /// swallows the resulting `ColumnNotFound` error at runtime, so it
    /// still requires `JourneyStop: sqlx::Type<Postgres> +
    /// sqlx::Decode<Postgres>` at compile time -- a bound this
    /// never-decoded, pure computed/serialization type deliberately
    /// doesn't satisfy. `#[sqlx(skip)]` instead emits a bare
    /// `Default::default()` with no such bound, which is what "this column
    /// is never in the SELECT" actually needs.
    #[sqlx(skip)]
    pub journey_stops: Option<Vec<crate::data::journey::JourneyStop>>,
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
// `LEFT JOIN trains tr`: Step C of
// docs/superpowers/specs/2026-09-06-shared-train-identity-design.md §2 --
// train_uid/train_id/schedule_destination_crs/schedule_calling_points come
// from the shared trains row -- `tracked_trains`' own duplicate columns
// (train_uid/train_id/schedule_*) no longer exist at all as of Task 22's
// migration, so `tr` is now the ONLY possible source for any of these
// four fields. `cs` joins on `trains_id` (Step D, Task 11) -- new writes
// go through `upsert_train_movement`, keyed on `trains_id` alone, so the
// old `cs.tracked_train_id = tt.id` join would silently stop seeing fresh
// current-state rows for any train resolved after that task landed.
//
// Until Task 22, `train_id` here was `COALESCE(tr.train_id, tt.train_id)`
// -- a fallback to `tracked_trains`' own directly-written column for the
// design spec's accepted gap (§2 Step B "Named edge case": a row resolved
// via live TRUST alone, where `train_uid` was never learned at all, so
// `trains_id` stays permanently NULL and `tr` can never surface anything
// for that row). Task 22's migration drops `tt.train_id` entirely, and
// `flip_legacy_resolution` (the only writer of that column) stopped
// writing it in the same task -- the fallback's source is gone on both
// ends, so this now reads `tr.train_id` alone. This WIDENS the accepted
// gap: a live-TRUST-only resolution with no known `train_uid` now loses
// `train_id` too, not just the three fields that were already NULL in
// that scenario (`train_uid`/`schedule_destination_crs`/
// `schedule_calling_points`) -- see
// `a_resolution_with_no_known_train_uid_leaves_trains_id_null`'s own
// updated assertions for the concrete, tested shape of that gap.
const TRACKED_TRAIN_STATE_SELECT: &str = "\
    SELECT tt.id, tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, \
           so.name AS pin_origin_name, sd.name AS pin_destination_name, \
           tt.resolution_status, tr.train_uid, tr.train_id, \
           tr.destination_crs AS schedule_destination_crs, ssd.name AS schedule_destination_name, \
           tr.calling_points AS schedule_calling_points, \
           tr.id AS trains_id, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.next_calling_point, cs.eta_next, cs.eta_source, \
           tt.custom_name \
    FROM train_subscriptions tt \
    LEFT JOIN trains tr ON tr.id = tt.trains_id \
    LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
    LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs) \
    LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs) \
    LEFT JOIN stations ssd ON ssd.crs = UPPER(tr.destination_crs)";

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
    /// `Option`, not `String` -- see `TrackedTrainState::pin_origin_crs`'s
    /// own doc comment for the full reasoning. `GET /Train/mine` returns
    /// EVERY one of a user's subscriptions, so a single NR-primary
    /// subscription with no schedule data yet used to fail the whole list
    /// request, not merely omit itself from it.
    pub pin_origin_crs: Option<String>,
    pub pin_destination_crs: Option<String>,
    /// See `TrackedTrainState::pin_origin_name`'s doc comment -- same
    /// `LEFT JOIN stations` mechanism, same `None`-means-no-reference-row
    /// contract.
    pub pin_origin_name: Option<String>,
    pub pin_destination_name: Option<String>,
    /// `Option` for the same reason as `pin_origin_crs` directly above --
    /// the two columns were relaxed together by
    /// `20260906130000_nullable_pin_columns.sql` and are written (or not)
    /// together by `create_subscription_for_train`.
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
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
                tt.pin_scheduled_departure, tt.resolution_status, tr.train_uid, \
                cs.status, cs.delay_minutes, tt.tracked_at, tt.custom_name \
         FROM train_subscriptions tt \
         LEFT JOIN trains tr ON tr.id = tt.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
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
        "{TRACKED_TRAIN_STATE_SELECT} WHERE tr.train_uid = $1 AND tt.service_date = $2"
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
    let result = sqlx::query("DELETE FROM train_subscriptions WHERE id = $1 AND user_id = $2")
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
    let result = sqlx::query(
        "UPDATE train_subscriptions SET custom_name = $1 WHERE id = $2 AND user_id = $3",
    )
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
    let row: Option<(String,)> =
        sqlx::query_as("SELECT user_id FROM train_subscriptions WHERE id = $1")
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
///
/// `cs` joins on `trains_id`, not `tracked_train_id` -- same Step D
/// re-point (Task 11) as `TRACKED_TRAIN_STATE_SELECT`/
/// `list_tracked_trains_for_user` above: `upsert_train_movement` only ever
/// writes `train_current_state.trains_id`, so a `tracked_train_id` join
/// here would silently stop seeing fresh current-state rows for any train
/// resolved after this task landed, exactly the staleness those two
/// queries' own re-point avoids.
///
/// `LEFT JOIN trains tr ON tr.id = tt.trains_id` for `train_uid`: added by
/// Task 22, replacing a direct `tt.train_uid` read -- that column no
/// longer exists as of this task's migration, same Step C cutover
/// `TRACKED_TRAIN_STATE_SELECT`/`list_tracked_trains_for_user` already
/// made for their own `train_uid` field.
pub async fn list_tickets_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<TicketListItem>> {
    let rows = sqlx::query_as::<_, TicketListRow>(
        "SELECT t.id, t.tracked_train_id, t.operator, t.ticket_type, t.origin_crs, t.destination_crs, \
                so.name AS origin_name, sd.name AS destination_name, \
                t.source, t.created_at, \
                tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tr.train_uid, \
                cs.status, cs.delay_minutes, t.custom_name \
         FROM tracked_train_tickets t \
         LEFT JOIN train_subscriptions tt ON tt.id = t.tracked_train_id \
         LEFT JOIN trains tr ON tr.id = tt.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
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
        sqlx::query("DELETE FROM train_subscriptions WHERE user_id = $1")
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
            "INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
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
                "INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
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
    // As of Task 22, `tracked_trains` no longer has its own `train_uid`
    // column at all -- a schedule-matched pin's identity link lives
    // exclusively on `trains_id` (Task 3 already sets that directly, the
    // moment a schedule match succeeds). This test used to seed a raw
    // `tracked_trains.train_uid` value with `trains_id` left NULL, proving
    // the old per-row `train_uid = COALESCE($2, train_uid)` write preserved
    // it; that scenario can no longer be constructed (or occur in
    // production) once the column is gone, so this test now seeds the
    // schedule-matched identity the real way -- a pre-existing `trains_id`
    // link -- and proves the SAME end-to-end guarantee survives through
    // `flip_legacy_resolution`'s `(Some(id), _) => Some(id)` branch: a
    // later event carrying only `resolved_train_id` (no
    // `resolved_train_uid`) still resolves the pin and still shows the
    // earlier-linked `train_uid` via the joined `trains` row.
    async fn upsert_train_event_with_only_resolved_train_id_resolves_via_the_existing_trains_id_link()
     {
        let pool = connect().await;
        let user_id = "TEST-UPSERT-SCHEDULE-MATCHED";
        seed_user(&pool, user_id).await;
        let service_date: chrono::NaiveDate = "2026-09-05".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "C88888", service_date)
            .await
            .expect("find_or_create_train for the schedule-matched identity");
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, trains_id, resolution_status) \
             VALUES ($1, $2, $3, $4, $5, 'schedule_matched') RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("EUS")
        .bind("2026-09-05T18:15:00Z".parse::<DateTime<Utc>>().unwrap())
        .bind(trains_id) // already schedule-matched and linked, no train_id yet
        .fetch_one(&pool)
        .await
        .expect("seed schedule-matched tracked_trains row, already linked to a trains_id");

        let mut event = fixture_event(tracked_train_id, "dedup-only-train-id");
        event.resolved_train_uid = None; // not re-supplied by this event
        event.resolved_train_id = Some("221832406".to_string());

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracked_train_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.resolution_status, "resolved");
        assert_eq!(state.train_id, Some("221832406".to_string()));
        assert_eq!(
            state.train_uid,
            Some("C88888".to_string()),
            "the schedule-matched identity, already linked via trains_id, must still be visible \
             through the joined trains row even though this event supplied no resolved_train_uid"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
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

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(state.resolution_status, "resolved");
        assert_eq!(state.train_uid, Some("C21373".to_string()));
        assert_eq!(state.train_id, Some("221832406".to_string()));

        // Both fields resolve here, so the Step A dual-write fires and
        // creates a row in the shared `trains` table -- clean it up (see
        // the comment on the sibling test above for the convention).
        sqlx::query("DELETE FROM trains WHERE train_uid = 'C21373'")
            .execute(&pool)
            .await
            .ok();
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

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert train event");

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
        // Task 11 behavior change (intentional, per this task's own doc
        // comment on upsert_train_event): before this task, the
        // movement/current-state writes happened unconditionally, keyed
        // directly on tracked_train_id. As of this task, that write always
        // goes through upsert_train_movement, keyed on trains_id -- and this
        // fixture's identity is entirely unknown (never resolved, so
        // tracked_trains.trains_id is still NULL, and this event itself
        // carries no resolved_train_uid/resolved_train_id either). There is
        // nothing to key a shared-table write on, so the event is dropped
        // (with a warning) rather than written, matching the accepted gap
        // flip_legacy_resolution documents. state.status comes back None,
        // not "en_route", because no train_current_state row was created at
        // all -- TRACKED_TRAIN_STATE_SELECT's LEFT JOIN cs ON cs.trains_id =
        // tt.trains_id finds nothing to join against.
        assert_eq!(state.status, None);

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_live_resolution_with_a_known_train_uid_dual_writes_the_shared_trains_row -- --ignored --test-threads=1`"]
    async fn a_live_resolution_with_a_known_train_uid_dual_writes_the_shared_trains_row() {
        let pool = connect().await;
        let user_id = "TEST-LIVE-RESOLUTION-DUAL-WRITE";
        sqlx::query(
            "INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING",
        )
        .bind(user_id)
        .bind("live-resolution@example.com")
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("seed fixture user");

        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind("WAT")
        .bind(service_date.and_hms_opt(18, 32, 0).unwrap().and_utc())
        .fetch_one(&pool)
        .await
        .expect("seed tracked_trains row");

        let event = TrainMovementEventMessage {
            tracked_train_id,
            resolved_train_uid: Some("TEST-LIVE-UID".to_string()),
            resolved_train_id: Some("221832406".to_string()),
            dedup_key: "test-live-dual-write-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("WAT".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("WAT".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: None,
            eta_next: None,
            eta_source: None,
        };

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert_train_event");

        let (trains_id,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM train_subscriptions WHERE id = $1")
                .bind(tracked_train_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id");
        let trains_id = trains_id.expect("a resolution with a known train_uid must set trains_id");

        let (train_uid, train_id): (String, Option<String>) =
            sqlx::query_as("SELECT train_uid, train_id FROM trains WHERE id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back the shared trains row");
        assert_eq!(train_uid, "TEST-LIVE-UID");
        assert_eq!(train_id, Some("221832406".to_string()));

        sqlx::query("DELETE FROM train_subscriptions WHERE user_id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TEST-LIVE-UID'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_resolution_with_no_known_train_uid_leaves_trains_id_null -- --ignored --test-threads=1`"]
    async fn a_resolution_with_no_known_train_uid_leaves_trains_id_null() {
        // The brief's accepted gap: a live-TRUST resolution that sets
        // resolved_train_id but never learned a train_uid at all (no prior
        // schedule match linked a trains_id on the row, and this event
        // carries no resolved_train_uid either) must NOT dual-write onto the
        // shared `trains` table -- tracked_trains.trains_id must stay NULL.
        // This is distinct from the
        // upsert_train_event_with_only_resolved_train_id_resolves_via_the_existing_trains_id_link
        // test above, which seeds a row that already has a trains_id linked
        // from a prior schedule match (so the dual-write correctly *does*
        // fire for that case).
        //
        // As of Task 22, this gap is WIDER than it used to be:
        // `tracked_trains` no longer has its own `train_id` column for
        // `TRACKED_TRAIN_STATE_SELECT` to fall back to (that COALESCE, and
        // the write that fed it, are both gone -- see
        // `flip_legacy_resolution`'s own doc comment). A resolution that
        // never learns a train_uid now loses BOTH `train_uid` and
        // `train_id` on the read model, not just `train_uid` -- there is no
        // longer anywhere else in this schema for the real TRUST train_id
        // to be recorded once `trains_id` can't be linked.
        let pool = connect().await;
        let user_id = "TEST-RESOLUTION-NO-KNOWN-TRAIN-UID";
        seed_user(&pool, user_id).await;
        // No prior schedule match -- this pin was never linked to a
        // trains_id, unlike the sibling test's fixture.
        let tracking_id = seed_tracked_train(&pool, user_id).await;

        let mut event = fixture_event(tracking_id, "dedup-no-known-train-uid");
        event.resolved_train_uid = None; // never learned -- the accepted gap
        event.resolved_train_id = Some("221832406".to_string());

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(
            state.resolution_status, "resolved",
            "the pin itself must still resolve"
        );
        assert_eq!(
            state.train_id, None,
            "no trains row was ever linked, so the joined read model has nowhere left to read \
             train_id from -- this widened gap is the direct, accepted consequence of Task 22 \
             dropping tracked_trains' own train_id column and flip_legacy_resolution's write to it"
        );
        assert_eq!(
            state.train_uid, None,
            "train_uid was never known, so it must stay NULL"
        );

        let (trains_id,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM train_subscriptions WHERE id = $1")
                .bind(tracking_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id");
        assert_eq!(
            trains_id, None,
            "no train_uid was ever known, so the Step A dual-write must not fire and trains_id must stay NULL"
        );

        let leaked: Vec<(i64,)> = sqlx::query_as("SELECT id FROM trains WHERE train_id = $1")
            .bind("221832406")
            .fetch_all(&pool)
            .await
            .expect("check for leaked trains rows");
        assert!(
            leaked.is_empty(),
            "no trains row should have been created for this train_id when train_uid was never known, found: {leaked:?}"
        );

        cleanup_user(&pool, user_id).await;
    }

    // --- Step C cutover: reads now come from the joined `trains` row -----
    //
    // A prior test here (`get_by_tracking_id_reads_identity_from_the_joined_
    // trains_row_not_tracked_trains_own_stale_column`) proved the read
    // trusted the joined `trains` row over tracked_trains' own (deliberately
    // seeded-wrong) `train_uid` column. As of Task 22, that column no longer
    // exists at all -- there is no longer any "own stale column" left to
    // seed or to accidentally read from, so the invariant that test proved
    // is now structurally guaranteed by the schema itself. Removed rather
    // than kept as dead weight.

    /// Legacy-pin regression guard: a pin still stuck at `trains_id IS NULL`
    /// (never resolved -- Task 7's backfill only ever touches
    /// `train_uid IS NOT NULL` rows, so a genuinely-`pending` pin never gets
    /// one) must still come back with every `trains`-derived field `None`,
    /// via the `LEFT JOIN` -- not be silently excluded, which an inadvertent
    /// `JOIN`/`INNER JOIN` would do.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_by_tracking_id_a_legacy_unresolved_pin_with_no_trains_id_still_returns_the_row \
                -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_a_legacy_unresolved_pin_with_no_trains_id_still_returns_the_row() {
        let pool = connect().await;
        let user_id = "TEST-STEP-C-LEFT-JOIN";
        seed_user(&pool, user_id).await;
        let tracking_id = seed_tracked_train(&pool, user_id).await;

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("get_by_tracking_id")
            .expect("row must still be returned even with trains_id NULL");
        assert_eq!(state.resolution_status, "pending");
        assert_eq!(
            state.train_uid, None,
            "no trains row to join against -- must be None, not an error or a missing row"
        );
        assert_eq!(state.train_id, None);
        assert_eq!(state.schedule_destination_crs, None);
        assert_eq!(state.schedule_calling_points, None);

        cleanup_user(&pool, user_id).await;
    }

    // The one-off Step D production backfill job that used to live here
    // (`run_backfill_pass` + `run_step_d_backfill_of_movement_tables`,
    // batching `trains_id` onto pre-existing `train_movement_events`/
    // `train_current_state` rows keyed by their now-dropped
    // `tracked_train_id` columns) already ran, exactly once, against this
    // environment -- Task 22's own Step 1 dry-run count of 0 confirms
    // nothing was left for it to do. Its own SQL joined on
    // `tme.tracked_train_id`/`cs.tracked_train_id`, both dropped by this
    // task's migration, so it can never run again; removed rather than
    // left as permanently-broken dead code.

    // --- Task 11: split upsert_train_event into upsert_train_movement +
    // flip_legacy_resolution --------------------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                upsert_train_movement_writes_a_row_for_a_trains_id_with_no_subscriber_at_all \
                -- --ignored --test-threads=1`"]
    async fn upsert_train_movement_writes_a_row_for_a_trains_id_with_no_subscriber_at_all() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "NOSUB-UID", service_date)
            .await
            .expect("find_or_create_train");
        // Deliberately: no tracked_trains row is ever created for this trains_id.

        let event = TrainMovementEventMessage {
            tracked_train_id: 0, // unused by upsert_train_movement -- see its own doc comment
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-nosub-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("WAT".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("WAT".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: None,
            eta_next: None,
            eta_source: None,
        };

        upsert_train_movement(&pool, trains_id, &event)
            .await
            .expect("upsert_train_movement for an unsubscribed train");

        let (status,): (String,) =
            sqlx::query_as("SELECT status FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect(
                    "a current-state row must exist for this trains_id even with zero subscribers",
                );
        assert_eq!(status, "en_route");

        // Also verify the movement-event row itself landed, trains_id-keyed
        // -- the whole point of this function's split from
        // upsert_train_event. As of Task 22, `train_movement_events` no
        // longer has a `tracked_train_id` column at all to assert `NULL`
        // on -- "with no tracked_train_id at all" is now structurally
        // guaranteed by the schema itself, not just this row's own value.
        let (dedup_key,): (String,) =
            sqlx::query_as("SELECT dedup_key FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect(
                    "a movement-event row must exist for this trains_id even with zero subscribers",
                );
        assert_eq!(dedup_key, "test-nosub-dedup");

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                upsert_train_movement_is_idempotent_on_a_redelivered_dedup_key \
                -- --ignored --test-threads=1`"]
    async fn upsert_train_movement_is_idempotent_on_a_redelivered_dedup_key() {
        // Same real code path called twice against non-reset state (not a
        // duplicated inline copy) -- proving ON CONFLICT (trains_id, dedup_key)
        // actually dedups the movement-event insert for this new,
        // trains_id-only write path, the same guarantee upsert_train_event
        // already had for the legacy tracked_train_id-keyed path.
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "IDEMPOTENT-UID", service_date)
                .await
                .expect("find_or_create_train");

        let mut event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-idempotent-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("WAT".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("WAT".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: None,
            eta_next: None,
            eta_source: None,
        };

        upsert_train_movement(&pool, trains_id, &event)
            .await
            .expect("first upsert_train_movement call");
        // A redelivered Kafka message: same dedup_key, but the current-state
        // fields have moved on (a later, real-world snapshot of the same
        // train) -- proving the event-row dedup and the current-state
        // upsert are independent concerns, exactly as upsert_train_event's
        // own doc comment already established for the legacy path.
        event.status = "en_route".to_string();
        event.delay_minutes = Some(5);
        event.last_reported_location = Some("CLJ".to_string());
        upsert_train_movement(&pool, trains_id, &event)
            .await
            .expect("second, redelivered upsert_train_movement call");

        let rows: Vec<(i64,)> = sqlx::query_as(
            "SELECT id FROM train_movement_events WHERE trains_id = $1 AND dedup_key = $2",
        )
        .bind(trains_id)
        .bind("test-idempotent-dedup")
        .fetch_all(&pool)
        .await
        .expect("read back movement-event rows");
        assert_eq!(
            rows.len(),
            1,
            "the redelivered event must be deduped, not inserted a second time"
        );

        let (delay_minutes, last_reported_location): (Option<i32>, Option<String>) = sqlx::query_as(
            "SELECT delay_minutes, last_reported_location FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("read back current-state row");
        assert_eq!(
            delay_minutes,
            Some(5),
            "current-state upsert must still apply the second call's fresher values"
        );
        assert_eq!(last_reported_location, Some("CLJ".to_string()));

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    // --- Option C: event-time monotonicity guard, see
    // docs/superpowers/specs/2026-09-07-shared-train-status-write-race-design.md
    // -----------------------------------------------------------------------

    /// The direct discriminating proof for the guard `upsert_train_movement`'s
    /// own doc comment describes: `trust-consumer` and `trust-backlog-consumer`
    /// both call this same function for the same `trains_id`, with no
    /// coordination between them, so an OLDER event can genuinely reach
    /// Postgres AFTER a NEWER one already wrote the row (one process lagging
    /// behind the other -- see the design doc's §2). Before the `WHERE
    /// EXCLUDED.event_time >= train_current_state.event_time OR
    /// train_current_state.event_time IS NULL` guard existed, this second,
    /// commit-order-later call would blindly win and regress the row back to
    /// stale data; this test proves it no longer does, on
    /// `status`/`last_reported_location`/`delay_minutes`/`event_time` alike.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                an_out_of_order_event_does_not_regress_current_state \
                -- --ignored --test-threads=1`"]
    async fn an_out_of_order_event_does_not_regress_current_state() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "OUT-OF-ORDER-UID", service_date)
                .await
                .expect("find_or_create_train");

        let newer_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-out-of-order-newer-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("ARRIVAL".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("MKC".to_string()),
            planned_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("MKC".to_string()),
            last_event_type: Some("ARRIVAL".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("BHM".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &newer_event)
            .await
            .expect("the newer event's own write must succeed");

        // An OLDER event (an earlier actual_timestamp), arriving SECOND --
        // e.g. a lagging trust-consumer catching up on a Movement
        // trust-backlog-consumer's own faster path already superseded.
        let older_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-out-of-order-older-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("72410".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            raw_body: serde_json::json!({}),
            status: "cancelled".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(99),
            next_calling_point: Some("CRE".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &older_event)
            .await
            .expect("the older event's call must succeed (a guarded no-op is not an error)");

        let (status, last_reported_location, delay_minutes, event_time): (
            String,
            Option<String>,
            Option<i32>,
            Option<DateTime<Utc>>,
        ) = sqlx::query_as(
            "SELECT status, last_reported_location, delay_minutes, event_time \
             FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("read back current-state row");

        assert_eq!(
            status, "en_route",
            "the older event's status must not have overwritten the newer event's"
        );
        assert_eq!(
            last_reported_location,
            Some("MKC".to_string()),
            "the older event's location must not have overwritten the newer event's"
        );
        assert_eq!(
            delay_minutes,
            Some(0),
            "the older event's delay_minutes must not have overwritten the newer event's"
        );
        assert_eq!(
            event_time,
            Some("2026-09-06T19:45:00Z".parse::<DateTime<Utc>>().unwrap()),
            "event_time itself must still reflect the newer event, not have regressed either"
        );

        // Also directly verify the movement-event ROWS themselves both
        // landed -- the guard is scoped to the train_current_state upsert
        // only, never to train_movement_events' own append-only insert.
        let movement_events_count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count train_movement_events");
        assert_eq!(
            movement_events_count, 2,
            "both events' own movement-event rows must still be recorded regardless of the \
             current-state guard"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Same-shape companion to the out-of-order test above, proving the new
    /// guard does NOT interfere with the existing, expected in-order case:
    /// each call carries a `event_time` newer than (or equal to) the last,
    /// so every call's write must still apply normally, exactly as before
    /// this guard existed.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                in_order_events_still_update_current_state_normally \
                -- --ignored --test-threads=1`"]
    async fn in_order_events_still_update_current_state_normally() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "IN-ORDER-UID", service_date)
                .await
                .expect("find_or_create_train");

        let first_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-in-order-first-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("72410".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("MKC".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &first_event)
            .await
            .expect("first, in-order event must succeed");

        // A genuinely NEWER event, arriving second -- the ordinary,
        // overwhelmingly common case this guard must not disturb.
        let second_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-in-order-second-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("ARRIVAL".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("MKC".to_string()),
            planned_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:45:00Z".parse().unwrap()),
            variation_status: Some("LATE".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("MKC".to_string()),
            last_event_type: Some("ARRIVAL".to_string()),
            delay_minutes: Some(3),
            next_calling_point: Some("BHM".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &second_event)
            .await
            .expect("second, newer event must succeed");

        let (status, last_reported_location, delay_minutes, event_time): (
            String,
            Option<String>,
            Option<i32>,
            Option<DateTime<Utc>>,
        ) = sqlx::query_as(
            "SELECT status, last_reported_location, delay_minutes, event_time \
             FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("read back current-state row");

        assert_eq!(
            status, "en_route",
            "the in-order case is unaffected by the new guard"
        );
        assert_eq!(last_reported_location, Some("MKC".to_string()));
        assert_eq!(delay_minutes, Some(3));
        assert_eq!(
            event_time,
            Some("2026-09-06T19:45:00Z".parse::<DateTime<Utc>>().unwrap())
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Regression test for a critical bug review found in this guard's
    /// first version: `WHERE EXCLUDED.event_time >=
    /// train_current_state.event_time OR train_current_state.event_time IS
    /// NULL`, with no `EXCLUDED.event_time IS NULL` branch, silently and
    /// PERMANENTLY blocked every future write to a `trains_id` once (a) its
    /// stored `event_time` had become non-NULL, and (b) a later incoming
    /// event itself carried `event_time = NULL` (a real, reachable case --
    /// see this function's own doc comment on Cancellations with a missing
    /// or malformed `canx_timestamp`). `NULL >= x` is SQL's UNKNOWN, `... IS
    /// NULL` is FALSE once a real value is stored, and `UNKNOWN OR FALSE`
    /// never satisfies `WHERE` -- so the whole `ON CONFLICT DO UPDATE`
    /// became a no-op, worse than the pre-fix blind-overwrite behaviour it
    /// replaced (which at least always applied). This test proves a
    /// no-timestamp event still applies normally even against a row whose
    /// `event_time` is already known.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_null_event_time_event_still_applies_even_with_a_known_stored_event_time \
                -- --ignored --test-threads=1`"]
    async fn a_null_event_time_event_still_applies_even_with_a_known_stored_event_time() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "NULL-EVENT-TIME-APPLIES-UID",
            service_date,
        )
        .await
        .expect("find_or_create_train");

        // First, a normal, well-timed event -- establishes a known, non-NULL
        // stored event_time.
        let timed_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-null-event-time-timed-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("72410".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("MKC".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &timed_event)
            .await
            .expect("the first, well-timed event must succeed");

        // A CANCELLATION with a missing/malformed canx_timestamp -- both
        // planned_timestamp and actual_timestamp are None, exactly as
        // trust-consumer/trust-backlog-consumer's own Cancellation
        // construction produces for that case. Before the fix, this call
        // would have silently no-op'd (INSERT 0 0) instead of applying.
        let no_timestamp_cancellation = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-null-event-time-cancel-dedup".to_string(),
            msg_type: "0002".to_string(),
            event_type: None,
            loc_stanox: None,
            loc_crs: None,
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "cancelled".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("MKC".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &no_timestamp_cancellation)
            .await
            .expect("the no-timestamp event's own call must succeed");

        let (status,): (String,) =
            sqlx::query_as("SELECT status FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back current-state row");
        assert_eq!(
            status, "cancelled",
            "a no-timestamp event must still apply normally, even against a row whose \
             event_time is already known -- this is the direct regression proof for the bug \
             review found in this guard's first version"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Companion to the regression test above, proving the fix's OTHER half:
    /// a no-timestamp write must not clobber the stored `event_time` back to
    /// `NULL` (which would re-open the `train_current_state.event_time IS
    /// NULL` branch and permanently defeat the guard for this `trains_id`
    /// after just one no-timestamp event). Proves both halves directly: (1)
    /// `event_time` survives the no-timestamp write unchanged, and (2) a
    /// SUBSEQUENT, genuinely-stale, well-timed write is still correctly
    /// guarded (blocked) afterwards -- i.e. the guard keeps working, not
    /// merely that the column value looks right in isolation.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_null_event_time_write_does_not_clobber_the_stored_event_time \
                -- --ignored --test-threads=1`"]
    async fn a_null_event_time_write_does_not_clobber_the_stored_event_time() {
        let pool = connect().await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "NULL-EVENT-TIME-NO-CLOBBER-UID",
            service_date,
        )
        .await
        .expect("find_or_create_train");

        let timed_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-no-clobber-timed-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("72410".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T19:15:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("MKC".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &timed_event)
            .await
            .expect("the first, well-timed event must succeed");

        let no_timestamp_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-no-clobber-no-timestamp-dedup".to_string(),
            msg_type: "0002".to_string(),
            event_type: None,
            loc_stanox: None,
            loc_crs: None,
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: None,
            raw_body: serde_json::json!({}),
            status: "cancelled".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("MKC".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &no_timestamp_event)
            .await
            .expect("the no-timestamp event's own call must succeed");

        let (event_time_after_no_timestamp_write,): (Option<DateTime<Utc>>,) =
            sqlx::query_as("SELECT event_time FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back current-state row");
        assert_eq!(
            event_time_after_no_timestamp_write,
            Some("2026-09-06T19:15:00Z".parse::<DateTime<Utc>>().unwrap()),
            "a no-timestamp write must not clobber the stored event_time back to NULL"
        );

        // Now a genuinely STALE, well-timed event (older than the row's
        // still-intact stored event_time) -- must still be correctly
        // guarded (blocked), proving the earlier no-timestamp write didn't
        // permanently defeat the guard for this trains_id.
        let stale_timed_event = TrainMovementEventMessage {
            tracked_train_id: 0,
            resolved_train_uid: None,
            resolved_train_id: None,
            dedup_key: "test-no-clobber-stale-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("11111".to_string()),
            loc_crs: Some("XXX".to_string()),
            planned_timestamp: Some("2026-09-06T18:00:00Z".parse().unwrap()),
            actual_timestamp: Some("2026-09-06T18:00:00Z".parse().unwrap()),
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("XXX".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("YYY".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_movement(&pool, trains_id, &stale_timed_event)
            .await
            .expect("the stale event's own call must succeed (a guarded no-op is not an error)");

        let (status, last_reported_location, event_time): (
            String,
            Option<String>,
            Option<DateTime<Utc>>,
        ) = sqlx::query_as(
            "SELECT status, last_reported_location, event_time \
             FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("read back current-state row");
        assert_eq!(
            status, "cancelled",
            "the stale, well-timed event must still be guarded (blocked) after the intervening \
             no-timestamp write -- the guard must not have been permanently defeated"
        );
        assert_eq!(last_reported_location, Some("EUS".to_string()));
        assert_eq!(
            event_time,
            Some("2026-09-06T19:15:00Z".parse::<DateTime<Utc>>().unwrap()),
            "event_time itself must still reflect the last real timestamp, unaffected by \
             either the no-timestamp write or the subsequently-blocked stale write"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                upsert_train_event_delegates_to_upsert_train_movement_for_an_already_resolved_pin \
                -- --ignored --test-threads=1`"]
    async fn upsert_train_event_delegates_to_upsert_train_movement_for_an_already_resolved_pin() {
        // A pin already resolved (trains_id known from an earlier message),
        // receiving a plain follow-up event that itself carries neither
        // resolved_train_uid nor resolved_train_id. upsert_train_event must
        // still look up the existing trains_id and delegate the shared-table
        // write to upsert_train_movement -- the legacy per-subscription path
        // and the shared path must end up writing the exact same row.
        let pool = connect().await;
        let user_id = "TEST-DELEGATES-ALREADY-RESOLVED";
        seed_user(&pool, user_id).await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "DELEGATE-UID", service_date)
                .await
                .expect("find_or_create_train");
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, trains_id, \
                 resolution_status) \
             VALUES ($1, $2, 'EUS', $3, $4, 'resolved') RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(service_date.and_hms_opt(19, 15, 0).unwrap().and_utc())
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("seed an already-resolved tracked_trains row");

        let mut event = fixture_event(tracked_train_id, "test-delegate-dedup");
        event.resolved_train_uid = None;
        event.resolved_train_id = None; // no fresh resolution info on this event

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert_train_event");

        let (status,): (String,) =
            sqlx::query_as("SELECT status FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect(
                    "upsert_train_event must delegate to upsert_train_movement, keyed on trains_id",
                );
        assert_eq!(status, "en_route");

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracked_train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Task 17's own reason `list_active_tracked_trains` needed to start
    /// selecting `tt.trains_id`: trust-consumer's `apply_reference_reload`
    /// seeds `trains_id_by_tracked_train_id` straight off the field on
    /// `TrackedTrainRef` this query returns -- if the query silently
    /// dropped it, no forwarding signal (Task 17) could ever be built for
    /// this ref, even though the pin is otherwise fully wired up
    /// (`resolved`, with a real `trains_id`).
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_active_tracked_trains -- --ignored --test-threads=1`"]
    async fn list_active_tracked_trains_carries_the_trains_id_through_for_a_resolved_ref() {
        let pool = connect().await;
        let user_id = "TEST-LIST-ACTIVE-TRAINS-ID-USER";
        seed_user(&pool, user_id).await;
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-LIST-ACTIVE-TRAINS-ID-UID",
            "2026-09-06".parse().unwrap(),
        )
        .await
        .expect("find_or_create_train");

        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, \
                 trains_id, resolution_status) \
             VALUES ($1, $2, 'WAT', $3, $4, 'resolved') \
             RETURNING id",
        )
        .bind(user_id)
        .bind("2026-09-06".parse::<chrono::NaiveDate>().unwrap())
        .bind("2026-09-06T19:15:00Z".parse::<DateTime<Utc>>().unwrap())
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("seed a resolved tracked_trains row with a real trains_id");

        let refs = list_active_tracked_trains(&pool)
            .await
            .expect("list_active_tracked_trains");
        let seeded = refs
            .into_iter()
            .find(|r| r.id == tracked_train_id)
            .expect("the seeded ref should be active (resolved, no current-state row)");
        assert_eq!(
            seeded.trains_id,
            Some(trains_id),
            "trains_id must round-trip through list_active_tracked_trains, not be dropped"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracked_train_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_inherits_known_schedule_data \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_inherits_known_schedule_data() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-TRACK";
        seed_user(&pool, user_id).await;

        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();
        let scheduled_departure: chrono::DateTime<chrono::Utc> =
            "2026-09-06T19:15:00Z".parse().unwrap();
        let trains_id = sqlx::query_scalar::<_, i64>(
            "INSERT INTO trains (train_uid, service_date, origin_crs, scheduled_departure) \
             VALUES ('TEST-NR-PRIMARY-UID', $1, 'EUS', $2) RETURNING id",
        )
        .bind(service_date)
        .bind(scheduled_departure)
        .fetch_one(&pool)
        .await
        .expect("seed a trains row with known schedule data");

        let tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("create_subscription_for_train");

        let (row_trains_id, pin_origin_crs, pin_scheduled_departure): (
            Option<i64>,
            String,
            chrono::DateTime<chrono::Utc>,
        ) = sqlx::query_as(
            "SELECT trains_id, pin_origin_crs, pin_scheduled_departure FROM train_subscriptions \
             WHERE id = $1",
        )
        .bind(tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back the new subscription");
        assert_eq!(row_trains_id, Some(trains_id));
        assert_eq!(pin_origin_crs, "EUS");
        assert_eq!(pin_scheduled_departure, scheduled_departure);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// The `NULL`-pins case the design spec's own §1 accepts as a gap: a
    /// bare `train_uid` `trains` row with no schedule data at all (a caller
    /// that only ever knew NR's identity for the train, never a CRS/time).
    /// Proves `create_subscription_for_train` doesn't error or silently
    /// substitute a default -- it faithfully carries the `trains` row's own
    /// `NULL` schedule columns straight through, which only works at all
    /// because this task's own migration dropped `pin_origin_crs`/
    /// `pin_scheduled_departure`'s `NOT NULL` constraint.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_allows_null_pins_for_a_bare_uid_with_no_schedule_data \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_allows_null_pins_for_a_bare_uid_with_no_schedule_data() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-TRACK-NULL-PINS";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-NULL-PINS-UID",
            "2026-09-06".parse().unwrap(),
        )
        .await
        .expect("seed a bare trains row with no schedule data");

        let tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("create_subscription_for_train must succeed even with no schedule data");

        let (row_trains_id, pin_origin_crs, pin_scheduled_departure): (
            Option<i64>,
            Option<String>,
            Option<chrono::DateTime<chrono::Utc>>,
        ) = sqlx::query_as(
            "SELECT trains_id, pin_origin_crs, pin_scheduled_departure FROM train_subscriptions \
             WHERE id = $1",
        )
        .bind(tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back the new subscription");
        assert_eq!(row_trains_id, Some(trains_id));
        assert_eq!(
            pin_origin_crs, None,
            "no schedule match yet -> NULL pin, not a default"
        );
        assert_eq!(pin_scheduled_departure, None);

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Explicit idempotency check. This test previously asserted the
    /// OPPOSITE -- that two calls produced two rows -- which was an honest
    /// record of the behaviour at the time, not a requirement. A
    /// train-listing feature turned that behaviour into a real user-facing
    /// bug (a "Track this train" button a user can click twice), so the
    /// function was fixed and this test inverted alongside it. See this
    /// task's own header, which also records why a
    /// `UNIQUE (user_id, trains_id)` index was rejected in favour of this
    /// in-function fix.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_called_twice_returns_the_same_subscription \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_called_twice_returns_the_same_subscription() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-TRACK-TWICE";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-TWICE-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        let first_tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("first create_subscription_for_train call");
        let second_tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("second create_subscription_for_train call, same trains_id and user_id");

        assert_eq!(
            first_tracking_id, second_tracking_id,
            "a repeat call for the same (trains_id, user_id) must return the EXISTING \
             subscription, not create a second one"
        );

        let (row_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM train_subscriptions WHERE trains_id = $1 AND user_id = $2",
        )
        .bind(trains_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("count subscriptions for this (trains_id, user_id) pair");
        assert_eq!(row_count, 1, "exactly one row must exist after two calls");

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(first_tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Discriminating counterpart: idempotency is scoped to ONE user. Two
    /// different users tracking the same physical train is this endpoint's
    /// own headline scenario (the whole point of the shared `trains` table)
    /// and must still produce two independent subscriptions.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_is_not_shared_between_users \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_is_not_shared_between_users() {
        let pool = connect().await;
        let user_a = "TEST-NR-PRIMARY-SHARED-A";
        let user_b = "TEST-NR-PRIMARY-SHARED-B";
        seed_user(&pool, user_a).await;
        seed_user(&pool, user_b).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-SHARED-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        let a = create_subscription_for_train(&pool, trains_id, user_a)
            .await
            .expect("user A subscribes");
        let b = create_subscription_for_train(&pool, trains_id, user_b)
            .await
            .expect("user B subscribes to the same train");

        assert_ne!(
            a, b,
            "two users tracking one train must still get two independent subscriptions"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id IN ($1, $2)")
            .bind(a)
            .bind(b)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_a).await;
        cleanup_user(&pool, user_b).await;
    }

    /// Guards the one thing the existing-row-wins CTE could plausibly get
    /// wrong: the returned id must be the row that actually exists, usable
    /// as a real tracking id, not a stale/duplicated value. Reads the row
    /// back through the same ownership query every `/Train/{trackingId}`
    /// route uses.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_repeat_call_returns_a_usable_tracking_id \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_repeat_call_returns_a_usable_tracking_id() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-USABLE-ID";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-USABLE-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("first call");
        let returned = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("second call");

        let owner = tracked_train_owner(&pool, returned)
            .await
            .expect("ownership lookup");
        assert_eq!(
            owner,
            Some(user_id.to_string()),
            "the returned id must resolve to a real, caller-owned subscription"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(returned)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Final whole-branch review re-review finding: proves
    /// `list_pending_pins_for_schedule_match` survives the state
    /// `aggregator::queries::prune_trains` can eventually put a still-
    /// `pending` NR-primary subscription into. `train_subscriptions.trains_id`
    /// is `ON DELETE SET NULL` (`20260906100000_trains.sql:38`), so deleting
    /// the `trains` row a bare-`train_uid`-no-schedule-data subscription
    /// (the same fixture shape as
    /// `create_subscription_for_train_allows_null_pins_for_a_bare_uid_with_no_schedule_data`)
    /// points at reproduces the exact post-prune state: `trains_id` NULL,
    /// `resolution_status` still `'pending'` (the cascade never touches
    /// it), AND `pin_origin_crs`/`pin_scheduled_departure` NULL (they were
    /// never written in the first place -- this subscription never had
    /// schedule data). Before this fix, `PendingSchedulePin` decoding those
    /// two columns as non-`Option` made this query error out entirely the
    /// moment ANY row reached this state, not just fail to process that one
    /// row -- poisoning every tick of the periodic sweep for every
    /// still-pending subscription in the table, forever. Asserts here that
    /// the call succeeds and this row is excluded rather than surfaced for
    /// a schedule-match attempt it has no CRS/time to make.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_pending_pins_for_schedule_match_excludes_a_pruned_nr_primary_row_with_null_pins \
                -- --ignored --test-threads=1`"]
    async fn list_pending_pins_for_schedule_match_excludes_a_pruned_nr_primary_row_with_null_pins()
     {
        let pool = connect().await;
        let user_id = "TEST-PRUNED-NR-PRIMARY-SWEEP";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-PRUNED-NR-PRIMARY-UID",
            "2026-09-06".parse().unwrap(),
        )
        .await
        .expect("seed a bare trains row with no schedule data");

        let tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("create_subscription_for_train must succeed even with no schedule data");

        // Simulate `aggregator::queries::prune_trains` deleting the linked
        // `trains` row: the FK's own `ON DELETE SET NULL`
        // (`20260906100000_trains.sql:38`) is what actually nulls out
        // `trains_id` here, not a manual UPDATE -- this is the real
        // mechanism responsible for the bug, not a stand-in for it.
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .expect("delete the trains row to trigger its ON DELETE SET NULL cascade");

        let (row_trains_id, row_status, row_origin, row_departure): (
            Option<i64>,
            String,
            Option<String>,
            Option<DateTime<Utc>>,
        ) = sqlx::query_as(
            "SELECT trains_id, resolution_status, pin_origin_crs, pin_scheduled_departure \
             FROM train_subscriptions WHERE id = $1",
        )
        .bind(tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back the subscription after the trains row is gone");
        assert_eq!(
            row_trains_id, None,
            "ON DELETE SET NULL should have nulled out trains_id"
        );
        assert_eq!(
            row_status, "pending",
            "resolution_status is untouched by the FK cascade"
        );
        assert_eq!(row_origin, None, "this subscription never had schedule data");
        assert_eq!(row_departure, None);

        let pending = list_pending_pins_for_schedule_match(&pool)
            .await
            .expect(
                "must not error even though a pruned NR-primary row with NULL pin columns is \
                 present in the table",
            );
        assert!(
            !pending.iter().any(|row| row.id == tracking_id),
            "a row with NULL pin_origin_crs/pin_scheduled_departure must be excluded, not \
             surfaced for a schedule-match attempt it cannot make"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracking_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Task 21's own headline scenario, proven end-to-end rather than
    /// merely claimed: two DIFFERENT users tracking the SAME physical
    /// train via `create_subscription_for_train` (Task 20's NR-primary
    /// path, which deliberately never wrote either row's own legacy
    /// `tracked_trains.train_uid` column even back when that column still
    /// existed -- see that function's doc comment) must BOTH come back
    /// from `list_active_tracked_trains` with a real `train_uid`, sourced
    /// via the `LEFT JOIN trains` Task 21 added. Before that task, both
    /// rows would have surfaced `train_uid: None` here -- neither was
    /// directly asserted by an existing test at the time, which is why
    /// that task's brief called for a new one rather than trusting the
    /// doc comment's own claim. As of Task 22, the column this test used
    /// to also assert was never written on either row no longer exists at
    /// all -- that belt-and-braces check is now structurally guaranteed by
    /// the schema itself, so it has been removed rather than kept as a
    /// query against a column that can never come back.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_active_tracked_trains_surfaces_train_uid_for_every_subscriber_sharing_a_trains_id \
                -- --ignored --test-threads=1`"]
    async fn list_active_tracked_trains_surfaces_train_uid_for_every_subscriber_sharing_a_trains_id()
     {
        let pool = connect().await;
        let first_user_id = "TEST-SHARED-TRAINS-ID-USER-1";
        let second_user_id = "TEST-SHARED-TRAINS-ID-USER-2";
        seed_user(&pool, first_user_id).await;
        seed_user(&pool, second_user_id).await;

        let train_uid = "TEST-SHARED-TRAINS-ID-UID";
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            train_uid,
            "2026-09-06".parse().unwrap(),
        )
        .await
        .expect("find_or_create_train");

        let first_tracking_id = create_subscription_for_train(&pool, trains_id, first_user_id)
            .await
            .expect("first subscriber tracks this train");
        let second_tracking_id = create_subscription_for_train(&pool, trains_id, second_user_id)
            .await
            .expect("second subscriber tracks the SAME physical train");

        // A prior version of this test also belt-and-braces-confirmed
        // neither row's own legacy `tracked_trains.train_uid` column was
        // ever written directly. As of Task 22, that column no longer
        // exists at all -- the check is now structurally guaranteed by the
        // schema itself, so it has been removed rather than kept as a
        // query against a column that can never come back.
        let refs = list_active_tracked_trains(&pool)
            .await
            .expect("list_active_tracked_trains");
        let first_ref = refs
            .iter()
            .find(|r| r.id == first_tracking_id)
            .expect("first subscriber's row must be active");
        let second_ref = refs
            .iter()
            .find(|r| r.id == second_tracking_id)
            .expect("second subscriber's row must be active");

        assert_eq!(
            first_ref.train_uid,
            Some(train_uid.to_string()),
            "the FIRST subscriber must get a by_train_uid-matchable entry, sourced via the \
             trains join, despite its own tracked_trains.train_uid column never being written"
        );
        assert_eq!(
            second_ref.train_uid,
            Some(train_uid.to_string()),
            "the SECOND subscriber sharing the same trains_id must ALSO get a \
             by_train_uid-matchable entry -- this is the Task 20 gap Task 21 exists to close"
        );
        assert_eq!(first_ref.trains_id, Some(trains_id));
        assert_eq!(second_ref.trains_id, Some(trains_id));

        sqlx::query("DELETE FROM train_subscriptions WHERE id IN ($1, $2)")
            .bind(first_tracking_id)
            .bind(second_tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, first_user_id).await;
        cleanup_user(&pool, second_user_id).await;
    }

    // --- Task 22: Step D's final cutover -- live-TRUST resolution proven
    // end-to-end AFTER the legacy column drop ---------------------------

    /// Task 22's own required proof: a live-TRUST resolution (the real
    /// `upsert_train_event` -> `flip_legacy_resolution` path trust-consumer
    /// and trust-backlog-consumer both call) must still succeed end-to-end
    /// once `tracked_trains.train_uid`/`train_id`/`resolved_at` and the
    /// other four retired legacy columns are physically gone. Seeds a
    /// fresh pin with `seed_tracked_train` (already updated by this same
    /// task to write only columns that still exist), runs the real
    /// resolution path, and asserts on the ordinary, still-present
    /// `resolution_status`/`trains_id` columns and the joined read model --
    /// nowhere in this test does any SQL of its own reference any of the
    /// seven `tracked_trains` columns or the two `tracked_train_id`
    /// columns this task's migration drops.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_live_trust_resolution_still_succeeds_end_to_end_after_the_legacy_column_drop \
                -- --ignored --test-threads=1`"]
    async fn a_live_trust_resolution_still_succeeds_end_to_end_after_the_legacy_column_drop() {
        let pool = connect().await;
        let user_id = "TEST-POST-DROP-RESOLUTION";
        seed_user(&pool, user_id).await;
        let tracking_id = seed_tracked_train(&pool, user_id).await;

        let mut event = fixture_event(tracking_id, "dedup-post-drop-resolution");
        event.resolved_train_uid = Some("TEST-POST-DROP-UID".to_string());
        event.resolved_train_id = Some("TEST-POST-DROP-TRAIN-ID".to_string());

        upsert_train_event(&pool, &event)
            .await
            .expect("a live-TRUST resolution must still succeed after the column drop");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("get_by_tracking_id")
            .expect("tracked train exists");
        assert_eq!(
            state.resolution_status, "resolved",
            "the pin must still flip to resolved via the post-drop flip_legacy_resolution"
        );
        assert_eq!(
            state.train_uid,
            Some("TEST-POST-DROP-UID".to_string()),
            "identity must be visible via the joined trains row, not any dropped tracked_trains column"
        );
        assert_eq!(state.train_id, Some("TEST-POST-DROP-TRAIN-ID".to_string()));
        assert_eq!(
            state.status,
            Some("en_route".to_string()),
            "upsert_train_movement's own shared-table write must also still fire, keyed on the \
             freshly dual-written trains_id"
        );

        let (trains_id,): (Option<i64>,) =
            sqlx::query_as("SELECT trains_id FROM train_subscriptions WHERE id = $1")
                .bind(tracking_id)
                .fetch_one(&pool)
                .await
                .expect("read back trains_id");
        let trains_id = trains_id.expect("the resolution must have linked a real trains_id");

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// The direct fix for the risk Task 21's review flagged, proven rather
    /// than merely argued in a doc comment: before this task,
    /// `flip_legacy_resolution`'s `UPDATE` wrote
    /// `tracked_trains.train_uid = COALESCE($2, train_uid)` per
    /// subscription row -- once two DIFFERENT subscribers shared one
    /// physical train (Task 20's own headline scenario) and each one's
    /// pin was independently resolved via live TRUST (e.g. a process
    /// restart re-delivering the same Activation and causing both
    /// subscribers' pins to resolve against the same `train_uid` and
    /// `service_date`), the SECOND subscriber's `UPDATE` would collide
    /// with `tracked_trains_resolved_identity`'s
    /// `UNIQUE (train_uid, service_date) WHERE train_uid IS NOT NULL`
    /// index and fail outright.
    ///
    /// This test seeds exactly that scenario -- two independent, still-
    /// `pending` pins for two different users, sharing the same
    /// `service_date` (both via `seed_tracked_train`) -- and resolves BOTH
    /// with the identical `resolved_train_uid`/`resolved_train_id`, back
    /// to back, with no cleanup in between. Both calls must succeed: the
    /// index itself is dropped by this task's migration, and
    /// `flip_legacy_resolution` no longer attempts the write that could
    /// have hit it in the first place -- the shared identity link now
    /// lives exclusively on `trains_id`, and `find_or_create_train`'s own
    /// `ON CONFLICT (train_uid, service_date) DO UPDATE` makes a second
    /// resolution against the same identity a safe, idempotent no-op.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                two_subscribers_sharing_a_physical_train_each_resolve_via_live_trust_without_a_unique_constraint_collision \
                -- --ignored --test-threads=1`"]
    async fn two_subscribers_sharing_a_physical_train_each_resolve_via_live_trust_without_a_unique_constraint_collision()
     {
        let pool = connect().await;
        let first_user_id = "TEST-POST-DROP-COLLISION-USER-1";
        let second_user_id = "TEST-POST-DROP-COLLISION-USER-2";
        seed_user(&pool, first_user_id).await;
        seed_user(&pool, second_user_id).await;

        let first_tracking_id = seed_tracked_train(&pool, first_user_id).await;
        let second_tracking_id = seed_tracked_train(&pool, second_user_id).await;

        let mut first_event = fixture_event(first_tracking_id, "dedup-post-drop-collision-1");
        first_event.resolved_train_uid = Some("TEST-POST-DROP-COLLISION-UID".to_string());
        first_event.resolved_train_id = Some("TEST-POST-DROP-COLLISION-TRAIN-ID".to_string());
        upsert_train_event(&pool, &first_event)
            .await
            .expect("the FIRST subscriber's live-TRUST resolution must succeed");

        // Same train_uid, same service_date (both fixtures share
        // seed_tracked_train's hardcoded "2026-09-02") -- exactly the
        // collision shape Task 21's review flagged. Before this task, this
        // second call's own UPDATE would have hit
        // tracked_trains_resolved_identity's UNIQUE constraint.
        let mut second_event = fixture_event(second_tracking_id, "dedup-post-drop-collision-2");
        second_event.resolved_train_uid = Some("TEST-POST-DROP-COLLISION-UID".to_string());
        second_event.resolved_train_id = Some("TEST-POST-DROP-COLLISION-TRAIN-ID".to_string());
        upsert_train_event(&pool, &second_event).await.expect(
            "the SECOND subscriber sharing the same physical train must ALSO resolve, with \
                 no unique-constraint collision -- this is the direct proof of Task 21's fix",
        );

        let (first_status, first_trains_id): (String, Option<i64>) = sqlx::query_as(
            "SELECT resolution_status, trains_id FROM train_subscriptions WHERE id = $1",
        )
        .bind(first_tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back first subscriber's row");
        let (second_status, second_trains_id): (String, Option<i64>) = sqlx::query_as(
            "SELECT resolution_status, trains_id FROM train_subscriptions WHERE id = $1",
        )
        .bind(second_tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back second subscriber's row");

        assert_eq!(first_status, "resolved");
        assert_eq!(second_status, "resolved");
        let trains_id = first_trains_id.expect("first subscriber must have a linked trains_id");
        assert_eq!(
            second_trains_id,
            Some(trains_id),
            "both subscribers must end up linked to the exact SAME shared trains row"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, first_user_id).await;
        cleanup_user(&pool, second_user_id).await;
    }

    /// Fix 2 (review finding C2), read path 1 of 2: `GET /Train/mine`.
    ///
    /// `create_subscription_for_train` against a `trains` row with NO
    /// schedule data leaves `pin_origin_crs`/`pin_scheduled_departure`
    /// `NULL` -- the DEFAULT outcome of `POST /Train/by-uid/{uid}/{date}/track`,
    /// not an edge case. Before this fix, `TrackedTrainListItem` typed both
    /// as non-`Option`, so sqlx failed to decode the row and the ENTIRE list
    /// request errored out -- one such subscription hid every other train
    /// the user had. This asserts the read succeeds and reports the pins as
    /// `None`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_tracked_trains_for_user_reads_a_subscription_with_null_pins \
                -- --ignored --test-threads=1`"]
    async fn list_tracked_trains_for_user_reads_a_subscription_with_null_pins() {
        let pool = connect().await;
        let user_id = "TEST-NULL-PIN-MINE-LIST";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NULL-PIN-LIST-UID",
            "2026-09-06".parse().unwrap(),
        )
        .await
        .expect("seed a bare trains row with no schedule data at all");
        let tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("create_subscription_for_train");

        let items = list_tracked_trains_for_user(&pool, user_id)
            .await
            .expect("GET /Train/mine's read must not error on a NULL-pin subscription");

        assert_eq!(
            items.len(),
            1,
            "the NULL-pin subscription must be listed, not skipped"
        );
        let item = &items[0];
        assert_eq!(item.id, tracking_id);
        assert_eq!(
            item.pin_origin_crs, None,
            "no schedule data -> None, not a default"
        );
        assert_eq!(item.pin_scheduled_departure, None);
        assert_eq!(item.pin_origin_name, None, "no CRS to join stations on");
        assert_eq!(
            item.train_uid,
            Some("TEST-NULL-PIN-LIST-UID".to_string()),
            "the shared trains row's identity is still readable"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Fix 2 (review finding C2), read path 2 of 2: the
    /// `TRACKED_TRAIN_STATE_SELECT`-backed read behind
    /// `GET /Train/{trackingId}`. Same NULL-pin scenario as the test above;
    /// before this fix `TrackedTrainState::pin_origin_crs` was a bare
    /// `String` and this call failed with "unexpected null; try decoding as
    /// an Option".
    ///
    /// `get_by_uid_and_date` (this module's other
    /// `TRACKED_TRAIN_STATE_SELECT` caller) is deliberately NOT covered
    /// here: it no longer has a single caller anywhere in the workspace --
    /// `GET /Train/by-uid/{uid}/{date}` reads
    /// `trains::get_public_train_state` as of Task 19. See this fix's
    /// report for that as a flagged, deliberate non-removal.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                get_by_tracking_id_reads_a_subscription_with_null_pins \
                -- --ignored --test-threads=1`"]
    async fn get_by_tracking_id_reads_a_subscription_with_null_pins() {
        let pool = connect().await;
        let user_id = "TEST-NULL-PIN-STATE-READ";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NULL-PIN-STATE-UID",
            "2026-09-06".parse().unwrap(),
        )
        .await
        .expect("seed a bare trains row with no schedule data at all");
        let tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("create_subscription_for_train");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("GET /Train/{trackingId}'s read must not error on a NULL-pin subscription")
            .expect("the subscription exists, so a row must come back");

        assert_eq!(state.id, tracking_id);
        assert_eq!(
            state.pin_origin_crs, None,
            "no schedule data -> None, not a default"
        );
        assert_eq!(state.pin_destination_crs, None);
        assert_eq!(state.pin_origin_name, None);
        assert_eq!(
            state.train_uid,
            Some("TEST-NULL-PIN-STATE-UID".to_string()),
            "the shared trains row's identity is still readable"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Fix 4 (review finding I1), the LIVE half -- the half the review
    /// asked to be confirmed by test rather than assumed broken.
    ///
    /// An NR-primary subscription (no pin, no schedule data, `trains_id`
    /// set from birth) is reachable by `trust-consumer`: it comes back from
    /// `list_active_tracked_trains` WITH its `train_uid` (via the `LEFT
    /// JOIN trains`), which is what populates that crate's
    /// `Reference::by_train_uid` direct-Activation-match fast path. This
    /// test then feeds the exact `TrainMovementEventMessage` that fast path
    /// produces back through `upsert_train_event` -- the real ingest entry
    /// point -- and asserts the full live outcome lands:
    ///
    /// * the subscription flips to `'resolved'`;
    /// * the SHARED `trains` row gets TRUST's own `train_id`/`resolved_at`;
    /// * `train_movement_events`/`train_current_state` are written, keyed
    ///   on `trains_id`.
    ///
    /// So live data flows correctly for this shape today, and Fix 4's
    /// production change is scoped to the BACKLOG gap alone (a train that
    /// has already run, whose live TRUST window has closed) -- see
    /// `routes::train::enrich_shared_train`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                an_nr_primary_subscription_receives_live_movement_events \
                -- --ignored --test-threads=1`"]
    async fn an_nr_primary_subscription_receives_live_movement_events() {
        let pool = connect().await;
        let user_id = "TEST-NR-LIVE-FLOW";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-NR-LIVE-UID", service_date)
                .await
                .expect("seed a bare trains row with no schedule data");
        let tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("create_subscription_for_train");

        // Step 1: trust-consumer's own reference reload must be able to SEE
        // this subscription's train_uid at all -- without this, its
        // `by_train_uid` fast path can never match an Activation for it.
        let refs = list_active_tracked_trains(&pool)
            .await
            .expect("list_active_tracked_trains");
        let this_ref = refs
            .iter()
            .find(|r| r.id == tracking_id)
            .expect("the NR-primary subscription must be in the active reference set");
        assert_eq!(
            this_ref.train_uid,
            Some("TEST-NR-LIVE-UID".to_string()),
            "train_uid must reach trust-consumer's by_train_uid map"
        );
        assert_eq!(this_ref.trains_id, Some(trains_id));
        assert_eq!(
            this_ref.pin_origin_crs, None,
            "and it genuinely has no pin for the CRS+time heuristic to use"
        );

        // Step 2: the event trust-consumer posts once its Activation match
        // is confirmed by the first live Movement.
        let event = common::TrainMovementEventMessage {
            tracked_train_id: tracking_id,
            resolved_train_uid: Some("TEST-NR-LIVE-UID".to_string()),
            resolved_train_id: Some("TEST-NR-LIVE-TRAINID".to_string()),
            dedup_key: "test-nr-live-dedup-1".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("12345".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("EUS".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("MKC".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_event(&pool, &event)
            .await
            .expect("upsert_train_event for a live NR-primary movement");

        let (resolution_status,): (String,) =
            sqlx::query_as("SELECT resolution_status FROM train_subscriptions WHERE id = $1")
                .bind(tracking_id)
                .fetch_one(&pool)
                .await
                .expect("read back the subscription");
        assert_eq!(resolution_status, "resolved");

        let (train_id, resolved_at): (Option<String>, Option<DateTime<Utc>>) =
            sqlx::query_as("SELECT train_id, resolved_at FROM trains WHERE id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back the shared trains row");
        assert_eq!(train_id, Some("TEST-NR-LIVE-TRAINID".to_string()));
        assert!(resolved_at.is_some());

        let (event_count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count movement events");
        assert_eq!(event_count, 1);

        let (last_location, status): (Option<String>, String) = sqlx::query_as(
            "SELECT last_reported_location, status FROM train_current_state WHERE trains_id = $1",
        )
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("the shared current-state row must exist");
        assert_eq!(last_location, Some("EUS".to_string()));
        assert_eq!(status, "en_route");

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Fix 5 (review finding I6), the database half.
    ///
    /// `trust-consumer` now fans one TRUST movement out to one
    /// `TrainMovementEventMessage` per subscription sharing the train (see
    /// `nr_primary_subscriptions_resolve_from_a_live_activation_and_movement`
    /// in `crates/trust-consumer/src/process.rs`, which proves it produces
    /// exactly those two messages). This is the other end of that: feeding
    /// both through `upsert_train_event` -- the real ingest path -- must
    /// flip BOTH subscriptions to `'resolved'`, and must NOT double-write
    /// the shared movement row they both describe.
    ///
    /// Both messages carry the SAME `dedup_key`, because they describe one
    /// real-world event; `upsert_train_movement`'s
    /// `ON CONFLICT (trains_id, dedup_key) DO NOTHING` is what collapses
    /// them to a single stored row.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                two_subscribers_sharing_one_train_both_resolve_from_one_movement \
                -- --ignored --test-threads=1`"]
    async fn two_subscribers_sharing_one_train_both_resolve_from_one_movement() {
        let pool = connect().await;
        let first_user = "TEST-SHARED-RESOLVE-A";
        let second_user = "TEST-SHARED-RESOLVE-B";
        cleanup_user(&pool, first_user).await;
        cleanup_user(&pool, second_user).await;
        seed_user(&pool, first_user).await;
        seed_user(&pool, second_user).await;
        let service_date: chrono::NaiveDate = "2026-09-06".parse().unwrap();

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-SHARED-RESOLVE-UID",
            service_date,
        )
        .await
        .expect("seed the shared trains row");
        let first_id = create_subscription_for_train(&pool, trains_id, first_user)
            .await
            .expect("first subscription");
        let second_id = create_subscription_for_train(&pool, trains_id, second_user)
            .await
            .expect("second subscription, same physical train");
        assert_ne!(first_id, second_id);

        // Exactly the two messages trust-consumer's fan-out produces for
        // one Activation+Movement cycle: same dedup_key, same resolution
        // signal, different tracked_train_id.
        let event_for = |tracked_train_id: i64| common::TrainMovementEventMessage {
            tracked_train_id,
            resolved_train_uid: Some("TEST-SHARED-RESOLVE-UID".to_string()),
            resolved_train_id: Some("TEST-SHARED-RESOLVE-TRAINID".to_string()),
            dedup_key: "test-shared-resolve-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("WAT".to_string()),
            planned_timestamp: None,
            actual_timestamp: None,
            variation_status: Some("ON TIME".to_string()),
            raw_body: serde_json::json!({}),
            status: "en_route".to_string(),
            last_reported_location: Some("WAT".to_string()),
            last_event_type: Some("DEPARTURE".to_string()),
            delay_minutes: Some(0),
            next_calling_point: Some("WOK".to_string()),
            eta_next: None,
            eta_source: None,
        };
        upsert_train_event(&pool, &event_for(first_id))
            .await
            .expect("ingest the first subscriber's copy");
        upsert_train_event(&pool, &event_for(second_id))
            .await
            .expect("ingest the second subscriber's copy");

        let statuses: Vec<(i64, String)> = sqlx::query_as(
            "SELECT id, resolution_status FROM train_subscriptions WHERE id = ANY($1) ORDER BY id",
        )
        .bind(vec![first_id, second_id])
        .fetch_all(&pool)
        .await
        .expect("read back both subscriptions");
        assert_eq!(
            statuses,
            vec![
                (first_id.min(second_id), "resolved".to_string()),
                (first_id.max(second_id), "resolved".to_string()),
            ],
            "BOTH subscribers sharing one physical train must resolve, not just one"
        );

        let (event_count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM train_movement_events WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count movement events");
        assert_eq!(
            event_count, 1,
            "one real-world event, one stored row -- the shared dedup key must collapse the \
             fan-out rather than duplicating it per subscriber"
        );

        let (state_count,): (i64,) =
            sqlx::query_as("SELECT count(*) FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("count current-state rows");
        assert_eq!(state_count, 1);

        let (train_id,): (Option<String>,) =
            sqlx::query_as("SELECT train_id FROM trains WHERE id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read back the shared trains row");
        assert_eq!(train_id, Some("TEST-SHARED-RESOLVE-TRAINID".to_string()));

        sqlx::query("DELETE FROM train_subscriptions WHERE id = ANY($1)")
            .bind(vec![first_id, second_id])
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, first_user).await;
        cleanup_user(&pool, second_user).await;
    }
}
