//! Tracked-train pin creation and lookup. Query functions are kept thin
//! (see `crates/api/src/data/queries.rs`'s module docs for why this crate
//! prefers runtime-checked `sqlx::query` over the `query!` macro family);
//! `validate_pin` is factored out so the one piece of actual logic here is
//! testable without a database.

use chrono::{DateTime, Utc};
use common::{CUSTOM_NAME_MAX_LENGTH, TicketEntryRequest, TrackPinRequest};
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
/// `pub(crate)` only so `data::groups::list_shared_trains_for_user` can
/// cap ITS half of the same `/track/mine` list with the same figure --
/// that list now renders this list plus the group-shared one, and two
/// independently-chosen caps on one page's rows would be a silent
/// divergence waiting to happen.
pub(crate) const MINE_LIST_LIMIT: i64 = 100;

/// These messages are USER-FACING COPY, not developer diagnostics. There is
/// no error envelope anywhere in this API (`crates/api/src/routes/train.rs`
/// returns `(StatusCode::BAD_REQUEST, String)` as plain text), and
/// `frontend/components/TrackTrainForm.tsx` renders the body verbatim as
/// the form's error `Alert` -- so a `snake_case` field name written here
/// becomes a `snake_case` field name on a user's screen
/// (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F5).
pub fn validate_pin(pin: &TrackPinRequest, now: DateTime<Utc>) -> Result<(), String> {
    if pin.origin_crs.trim().is_empty() {
        return Err("Enter the station you're departing from.".to_string());
    }
    if !crate::routes::is_crs_code(&pin.origin_crs) {
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
    // API-6: a pin beyond the published timetable can never match, and
    // used to sit in both 300 s sweeps until its date came round (a pin
    // dated 2090 was swept for ever).
    let london_today = now.with_timezone(&chrono_tz::Europe::London).date_naive();
    if pin.service_date > london_today + chrono::Duration::days(PIN_MAX_DAYS_AHEAD)
        || pin.scheduled_departure - now > chrono::Duration::days(PIN_MAX_DAYS_AHEAD + 1)
    {
        return Err(format!(
            "That departure is too far ahead — trains can be tracked up to {PIN_MAX_DAYS_AHEAD} \
             days before they run."
        ));
    }
    if let Some(crs) = &pin.destination_crs
        && !crate::routes::is_crs_code(crs)
    {
        return Err(
            "That doesn't look like a destination station code — CRS codes are three \
             letters, like WOK or EUS."
                .to_string(),
        );
    }
    // Free text, not an ATOC code: `TrackTrainForm` sends whatever the user
    // typed in its Operator box, and the column is stored but never read.
    if let Some(operator) = &pin.operator {
        crate::routes::validate_short_text("The operator", operator, PIN_OPERATOR_MAX_CHARS)?;
    }
    for platform in [&pin.platform, &pin.planned_platform].into_iter().flatten() {
        crate::routes::validate_short_text("The platform", platform, PIN_PLATFORM_MAX_CHARS)?;
    }
    crate::routes::validate_code_list(
        "Skipped stations",
        &pin.skipped_stations,
        PIN_MAX_SKIPPED_STATIONS,
        "three-letter station codes",
        crate::routes::is_crs_code,
    )?;
    Ok(())
}

// Moved to ds_store::tracking (ingest architecture plan 1A.9)
pub(crate) use ds_store::tracking::PIN_MAX_DAYS_AHEAD;
/// Operator names from the track form, e.g. "South Western Railway".
const PIN_OPERATOR_MAX_CHARS: usize = 64;
/// Darwin platforms are short ("1", "10A", "13-14").
const PIN_PLATFORM_MAX_CHARS: usize = 8;
/// A long-distance service has well under 64 calling points.
const PIN_MAX_SKIPPED_STATIONS: usize = 64;

/// At most this many of a user's subscriptions may be dated today or
/// later when they add a new one (API-6). Journeys, templates and groups
/// already had caps; tracked-train pins had none, and every pending pin
/// costs both 300 s sweeps a match attempt. Checked at the three user-facing
/// ways to add one (`POST /Train/track`, `POST /Train/track-by-uid` for a
/// train the user doesn't already track, and a pin-mode journey), count
/// then insert with the same accepted small race as the other caps.
/// Template materialisation and known-train journey legs are bounded by the
/// journey and template caps instead, but their rows still count here.
pub const MAX_FUTURE_PINS_PER_USER: i64 = 100;

/// The count [`MAX_FUTURE_PINS_PER_USER`] is checked against.
pub async fn count_future_subscriptions_for_user<'c, E>(
    executor: E,
    user_id: &str,
) -> anyhow::Result<i64>
where
    E: sqlx::PgExecutor<'c>,
{
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM train_subscriptions \
         WHERE user_id = $1 AND service_date >= CURRENT_DATE",
    )
    .bind(user_id)
    .fetch_one(executor)
    .await?;
    Ok(count)
}

/// How far back `POST /Train/by-uid/{uid}/{date}/track` accepts a
/// `service_date` (2026-10-01 review). Matches the 7 days
/// `GET /public/trains/search` lets a user look back
/// (`routes::trains::SEARCH_WINDOW_BACKWARD_DAYS`), so any train a user
/// can find there they can still track; well inside both `trains`
/// retention tiers (14 days untracked, 30 tracked), so the row this
/// creates is never one `aggregator` prunes straight away.
pub(crate) const TRACK_BY_UID_MAX_DAYS_BEHIND: i64 = 7;

/// At most this many of a user's subscriptions may be dated in the
/// recent past (`[today - TRACK_BY_UID_MAX_DAYS_BEHIND, today)`) when they
/// track another past-dated train by uid (2026-10-01 review).
/// [`MAX_FUTURE_PINS_PER_USER`] counts only `service_date >= today`, so
/// before this a past-dated by-uid request had no cap at all and one user
/// could mint unlimited subscriptions and shared `trains` rows. Same value
/// and the same accepted count-then-insert race as that cap.
pub const MAX_RECENT_PAST_PINS_PER_USER: i64 = 100;

/// The count [`MAX_RECENT_PAST_PINS_PER_USER`] is checked against:
/// `user_id`'s subscriptions with `earliest <= service_date < today`.
pub async fn count_recent_past_subscriptions_for_user<'c, E>(
    executor: E,
    user_id: &str,
    earliest: chrono::NaiveDate,
    today: chrono::NaiveDate,
) -> anyhow::Result<i64>
where
    E: sqlx::PgExecutor<'c>,
{
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM train_subscriptions \
         WHERE user_id = $1 AND service_date >= $2 AND service_date < $3",
    )
    .bind(user_id)
    .bind(earliest)
    .bind(today)
    .fetch_one(executor)
    .await?;
    Ok(count)
}

/// The user-facing 400 body for [`MAX_RECENT_PAST_PINS_PER_USER`].
pub fn recent_past_pin_cap_message() -> String {
    format!(
        "You're already tracking {MAX_RECENT_PAST_PINS_PER_USER} trains from the past \
         {TRACK_BY_UID_MAX_DAYS_BEHIND} days, which is the maximum. Remove some to make room."
    )
}

/// Whether `user_id` already tracks the shared train `(train_uid,
/// service_date)`, so `create_subscription_for_train` would hand back that
/// subscription rather than insert one. Looked up without creating the
/// `trains` row, so a capped user can't mint rows either.
pub async fn user_tracks_train_uid<'c, E>(
    executor: E,
    user_id: &str,
    train_uid: &str,
    service_date: chrono::NaiveDate,
) -> anyhow::Result<bool>
where
    E: sqlx::PgExecutor<'c>,
{
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS ( \
             SELECT 1 FROM train_subscriptions s JOIN trains t ON t.id = s.trains_id \
             WHERE s.user_id = $1 AND t.train_uid = $2 AND t.service_date = $3 \
         )",
    )
    .bind(user_id)
    .bind(train_uid)
    .bind(service_date)
    .fetch_one(executor)
    .await?;
    Ok(exists)
}

/// The user-facing 400 body for [`MAX_FUTURE_PINS_PER_USER`].
pub fn pin_cap_message() -> String {
    format!(
        "You're already tracking {MAX_FUTURE_PINS_PER_USER} upcoming trains, which is the \
         maximum. Remove some to make room."
    )
}

/// `user_id` is the authenticated caller's id (the OIDC `sub`) --
/// resolved by the route handler's `AuthenticatedUser` extractor
/// (`crates/api/src/routes/train.rs::post_track`, below), never taken from
/// the request body itself.
///
/// Generic over `E: PgExecutor` (rather than `&PgPool`) -- same reason as
/// `aggregator::queries::record_daily_stats`'s own doc comment: it lets
/// `journeys::create_journey_with_pin_leg` call this with `&mut *tx` from
/// inside its own transaction (19-pass security/bug review, journeys area,
/// Medium finding 3), while every standalone caller (`routes::train::post_track`
/// and this module's own tests) keeps passing a bare `&PgPool` unchanged --
/// `&PgPool` implements `PgExecutor<'_>` too.
pub async fn create_pin<'c, E>(
    executor: E,
    pin: &TrackPinRequest,
    user_id: &str,
) -> anyhow::Result<i64>
where
    E: sqlx::PgExecutor<'c>,
{
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO train_subscriptions \
            (user_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs, \
             pin_operator, pin_skipped_stations, pin_platform, pin_planned_platform) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
         RETURNING id",
    )
    .bind(user_id)
    .bind(pin.service_date)
    .bind(&pin.origin_crs)
    .bind(pin.scheduled_departure)
    .bind(&pin.destination_crs)
    .bind(&pin.operator)
    .bind(&pin.skipped_stations)
    .bind(&pin.platform)
    .bind(&pin.planned_platform)
    .fetch_one(executor)
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
/// Before DB2-21, two genuinely simultaneous calls could both observe no
/// existing row and both insert; the advisory lock described below closes
/// that. The frontend still disables the button while its request is in
/// flight (`frontend/components/TrackThisTrainButton.tsx`).
/// Generic over `E: PgExecutor` (rather than `&PgPool`) -- same reason as
/// [`create_pin`]'s own doc comment: `journeys::create_journey_with_known_train_leg`
/// calls this with `&mut *tx` from inside its own transaction (19-pass
/// security/bug review, journeys area, Medium finding 3), while every
/// standalone caller keeps passing a bare `&PgPool` unchanged.
///
/// **Re-enables a previously-deactivated row (2026-09-26 review, Medium
/// finding 10).** `journeys::set_leg_train_subscription` ("Change train")
/// sets `notifications_enabled = FALSE` on a subscription once it becomes
/// unreferenced by any leg -- but that's scoped to the journey-leg context
/// it was meant for, not a permanent verdict on the underlying
/// `(user_id, trains_id)` pin. Before this fix, THIS function's idempotency
/// meant a disabled row, once created, could never be re-enabled: any
/// later, unrelated call for the same user and physical train (a brand-new
/// journey leg auto-matching to it, a standalone "Track this train", even a
/// direct re-pin) just got the same disabled row back unchanged, silently
/// never notifying again for a use case that has nothing to do with the
/// "Change train" action that disabled it. Reaching this function at all
/// (new call, existing row) IS the new use case re-establishing the pin, so
/// it re-enables notifications for it -- an `UPDATE ... WHERE
/// notifications_enabled = FALSE` alongside the existing lookup, in the
/// same statement, so a fresh call always returns a live subscription.
///
/// **Serialised per `(user_id, trains_id)` (DB2-21).** The statement runs
/// in its own (nested) transaction after
/// `common::pg::lock_user_train_subscription`, so a concurrent call for the
/// same pair waits and then sees the first call's row, instead of both
/// inserting. The lock is a separate statement on purpose: under READ
/// COMMITTED a statement's snapshot is taken when it starts, so a lock
/// taken inside the same statement would still read the old snapshot.
/// Inside a caller's transaction the lock is held until that commits.
/// Takes `Acquire` (a `&PgPool` or a `&mut` connection/transaction) rather
/// than any executor, since it needs a transaction of its own.
pub async fn create_subscription_for_train<'c, A>(
    conn: A,
    trains_id: i64,
    user_id: &str,
) -> anyhow::Result<i64>
where
    A: sqlx::Acquire<'c, Database = sqlx::Postgres>,
{
    let mut tx = conn.begin().await?;
    common::pg::lock_user_train_subscription(&mut tx, user_id, trains_id).await?;
    // One statement, not a SELECT-then-INSERT round trip: the `inserted`
    // CTE's `NOT EXISTS (SELECT 1 FROM existing)` guard means the INSERT
    // never fires when a subscription is already there, and the final
    // UNION ALL yields exactly one row either way -- so `fetch_one` still
    // errors (RowNotFound) for a `trains_id` that names no `trains` row,
    // exactly as the previous plain `INSERT ... SELECT` did.
    //
    // `reactivated` is a data-modifying CTE never read by the final SELECT
    // -- per Postgres's own documented behavior for `WITH`, a data-modifying
    // statement always runs to completion exactly once, regardless of
    // whether anything references its output, so this still executes on
    // every call. The `AND notifications_enabled = FALSE` guard makes it a
    // no-op write (no row touched) on the overwhelmingly common path where
    // the existing subscription is already enabled.
    let row: (i64,) = sqlx::query_as(
        "WITH existing AS ( \
             SELECT id FROM train_subscriptions \
             WHERE user_id = $1 AND trains_id = $2 \
             ORDER BY id LIMIT 1 \
         ), \
         reactivated AS ( \
             UPDATE train_subscriptions SET notifications_enabled = TRUE \
             WHERE id IN (SELECT id FROM existing) AND notifications_enabled = FALSE \
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
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
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
    // API-7: the two free-text fields went into TEXT columns unbounded.
    // They can hold an operator's full name from a `.pkpass`, so the bound
    // is the custom-name one, not an ATOC code shape.
    if let Some(operator) = &entry.operator {
        crate::routes::validate_short_text("The operator", operator, TICKET_TEXT_MAX_CHARS)?;
    }
    if let Some(ticket_type) = &entry.ticket_type {
        crate::routes::validate_short_text("The ticket type", ticket_type, TICKET_TEXT_MAX_CHARS)?;
    }
    Ok(())
}

/// Bound on a ticket's free-text `operator` and `ticket_type` (API-7).
const TICKET_TEXT_MAX_CHARS: usize = CUSTOM_NAME_MAX_LENGTH;

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
            current_departure_date: None,
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
            current_departure_date: None,
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
    fn oversized_or_control_character_free_text_is_rejected() {
        let mut long = entry(Some("KGX"), "manual");
        long.operator = Some("x".repeat(TICKET_TEXT_MAX_CHARS + 1));
        assert!(
            validate_ticket_entry(&long)
                .unwrap_err()
                .contains("too long")
        );

        let mut long_type = entry(Some("KGX"), "manual");
        long_type.ticket_type = Some("y".repeat(2 * 1024 * 1024));
        assert!(validate_ticket_entry(&long_type).is_err());

        let mut control = entry(Some("KGX"), "manual");
        control.ticket_type = Some("Anytime\u{0}Return".to_string());
        assert!(validate_ticket_entry(&control).is_err());

        let mut at_limit = entry(Some("KGX"), "manual");
        at_limit.operator = Some("é".repeat(TICKET_TEXT_MAX_CHARS));
        assert!(
            validate_ticket_entry(&at_limit).is_ok(),
            "the bound counts characters, not bytes"
        );
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
                current_departure_date: None,
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

// Moved to ds_store::tracking (ingest architecture plan 1A.9)
pub(crate) use ds_store::tracking::reopen_subscriptions_after_reinstatement;
pub use ds_store::tracking::{
    PendingBacklogPin, PendingSchedulePin, TrainEventsBatchOutcome, apply_schedule_match,
    list_active_tracked_trains, list_pending_pins_for_backlog_match,
    list_pending_pins_for_schedule_match, upsert_train_event, upsert_train_event_on,
    upsert_train_events_batch, upsert_train_movement, upsert_train_movement_on,
};

/// The public read-model for a tracked train, returned directly as JSON by
/// `crates/api/src/routes/train.rs`'s `GET /Train/{trackingId}` (via
/// `get_by_tracking_id` below). Unlike `TrackedTrainRow`/`TrackedTrainRef`
/// above (poller-facing, private), this never leaks the raw `user_id`
/// column -- see Task 5's brief for why that read deliberately stays
/// public/unscoped despite `tracked_trains` having a real owner.
///
/// This module used to have a second `TRACKED_TRAIN_STATE_SELECT` reader,
/// `get_by_uid_and_date`, keyed only on `(train_uid, service_date)` with no
/// `user_id` filter at all. `GET /Train/by-uid/{uid}/{date}` stopped calling
/// it as of Task 19 (it reads `trains::get_public_train_state` instead),
/// which left it with zero callers anywhere in the workspace -- confirmed
/// again by the 19-pass Signal Box Audit's trains-area Low finding. Unlike
/// `get_by_tracking_id`, that query could match more than one
/// `train_subscriptions` row (multiple users can each pin the same
/// `train_uid`/`service_date`), so `fetch_optional` would return whichever
/// one row Postgres happened to pick -- silently handing back that OTHER
/// user's `custom_name`/`shared_group_count` (both genuinely per-user,
/// despite this struct never exposing `user_id` itself) to whatever caller
/// eventually got reconnected to it. Deleted outright rather than fixed,
/// since dead code carrying a latent cross-user leak serves no purpose
/// sitting around waiting to be reconnected.
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
    /// The pin's own scheduled departure, `None` for an NR-primary
    /// subscription whose `trains` row had no schedule data to source it from
    /// (same reason as `pin_origin_crs` immediately above).
    ///
    /// Internal plumbing, never sent to the frontend -- hence
    /// `#[serde(skip_serializing)]`, the same posture
    /// `schedule_skipped_stations`/`schedule_platform` already take on this
    /// struct. It exists solely so `routes::train::blend_darwin_eta` can hand
    /// `eta_blend::find_darwin_eta` the instant that identifies WHICH service
    /// on the origin's departure board this pin actually is; without it that
    /// overlay could only match on destination, and published the next
    /// service's estimate once the tracked train left the board (the
    /// 2026-09-25 review's High 3 finding). The wire already carries this
    /// value on `TrackedTrainListItem` for `GET /Train/mine`, so nothing new
    /// is exposed by reading it here.
    #[serde(skip_serializing)]
    pub pin_scheduled_departure: Option<DateTime<Utc>>,
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
    /// The shared row's own captured Darwin skip snapshot
    /// (`trains.skipped_stations`) -- internal plumbing for
    /// `routes::train::attach_journey_stops`'s `journey::build_journey_stops`
    /// call, same never-sent-to-the-frontend posture as `trains_id` just
    /// below (the frontend already gets this signal per-stop, on each
    /// `JourneyStop`'s own `skipSource`).
    #[serde(skip_serializing)]
    pub schedule_skipped_stations: Vec<String>,
    /// The shared row's own captured origin-platform snapshot
    /// (`trains.platform`/`planned_platform`) -- same internal-plumbing
    /// posture as `schedule_skipped_stations` immediately above, fed to
    /// `journey::build_journey_stops` and never sent to the frontend
    /// directly (the origin `JourneyStop` carries it instead).
    #[serde(skip_serializing)]
    pub schedule_platform: Option<String>,
    #[serde(skip_serializing)]
    pub schedule_planned_platform: Option<String>,
    pub status: Option<String>,
    pub last_reported_location: Option<String>,
    pub last_event_type: Option<String>,
    /// The delay at the passenger's own stop, against the PUBLIC timetable
    /// (`data::stop_delay`, design doc §9 decision 2): the pin destination, measured once the train has reported there and forecast before then. `None` until
    /// known.
    pub delay_minutes: Option<i32>,
    /// What `delay_minutes` was measured against (`public`, `publicSchedule`
    /// or `working`, see `common::public_delay::DelayBasis`); `None` exactly
    /// when `delay_minutes` is.
    #[sqlx(skip)]
    pub delay_basis: Option<crate::data::stop_delay::DelayBasis>,
    /// `true` while `delay_minutes` is a forecast (the train has not reported
    /// at that stop yet).
    #[sqlx(skip)]
    pub delay_provisional: bool,
    /// TRUST's running delay against the working timetable
    /// (`train_current_state.delay_minutes`), internal: the input to the
    /// per-stop estimates and to the forecast above. Never serialized.
    #[serde(skip_serializing)]
    pub working_delay_minutes: Option<i32>,
    pub next_calling_point: Option<String>,
    pub eta_next: Option<DateTime<Utc>>,
    pub eta_source: Option<String>,
    pub custom_name: Option<String>,
    /// How many groups (`crate::data::groups`) this subscription is
    /// currently shared into, via `group_trains.train_subscription_id` --
    /// `0` if it isn't shared anywhere. Read via a correlated `COUNT(*)`
    /// subquery directly in `TRACKED_TRAIN_STATE_SELECT` below, the same
    /// "attach a count to the row that owns it" shape
    /// `groups::GroupSummary`/`groups::GroupDetail` already use for their
    /// own `member_count` (`SELECT ... (SELECT COUNT(*) FROM group_members
    /// gm2 WHERE gm2.group_id = g.id) AS member_count ...`) -- deliberately
    /// NOT a separate `count_groups_containing_train` query invoked per row
    /// from a route handler, which would turn `GET /Train/mine` (a
    /// potentially-many-row list) into an N+1. Exists so the frontend's
    /// delete-confirmation modal (`DeleteTrainButton`) can warn "this train
    /// is shared in N group(s)" without a second round-trip.
    pub shared_group_count: i64,
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
    /// Server-side replacement for the frontend's old client-only "may have
    /// finished" heuristic -- `true` once now is more than 15 minutes past
    /// the ESTIMATED arrival at `journey_stops`'s final calling point (see
    /// `journey::may_have_arrived`). Populated the same "read row, then
    /// overlay a computed field" way as `journey_stops` itself, by
    /// `routes::train::attach_journey_stops`, hence `#[sqlx(skip)]` here
    /// too -- defaults to `false` (never a stale true) whenever there are
    /// no `journey_stops` to compute it from at all (pending/unresolved, or
    /// neither backing schedule source had anything).
    #[sqlx(skip)]
    pub may_have_arrived: bool,
    /// `serviceMode` (`train`/`replacementBus`/`bus`/`ferry`) and
    /// `liveTracking` (`false` for a bus or ferry, which TRUST never
    /// reports), from `schedule_services`; filled after the read by
    /// `data::schedule_services::attach`. A train when unknown.
    #[sqlx(skip)]
    #[serde(flatten)]
    pub service: crate::data::schedule_services::ServiceModeFields,
    /// The operating company's ATOC code (for example `"SW"`), from the CIF
    /// schedule. Serialized as `operatorCode`. Filled after the read by
    /// `data::train_operator`, hence `#[sqlx(skip)]`. `None` when no single
    /// code is known; see that module's doc for when that happens.
    #[sqlx(skip)]
    pub operator_code: Option<String>,
    /// The `tocs` display name for `operator_code` (for example
    /// `"South Western Railway"`). Serialized as `operatorName`. `None` when
    /// `operator_code` is `None` or the code has no `tocs` row.
    #[sqlx(skip)]
    pub operator_name: Option<String>,
    /// Whether the train is cancelled: `status == "cancelled"`. Filled
    /// after the read by `data::train_reasons`, like every field below.
    #[sqlx(skip)]
    pub cancelled: bool,
    /// The TRUST cancellation reason code (e.g. `"TG"`), only while
    /// `cancelled` is true.
    #[sqlx(skip)]
    pub cancel_reason_code: Option<String>,
    /// `cancel_reason_code`'s delay attribution glossary text (e.g.
    /// `"Driver"`). `None` for a code the glossary lacks or a system code
    /// (`PD`, `ZW`). There is no delay-reason equivalent: TRUST carries none.
    #[sqlx(skip)]
    pub cancel_reason: Option<String>,
    /// The TRUST change-of-origin reason code, when the train's origin was
    /// changed.
    #[sqlx(skip)]
    pub change_of_origin_reason_code: Option<String>,
    /// Its glossary text, under the same rules as `cancel_reason`.
    #[sqlx(skip)]
    pub change_of_origin_reason: Option<String>,
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
           tt.pin_scheduled_departure, \
           so.name AS pin_origin_name, sd.name AS pin_destination_name, \
           tt.resolution_status, tr.train_uid, tr.train_id, \
           tr.destination_crs AS schedule_destination_crs, ssd.name AS schedule_destination_name, \
           tr.calling_points AS schedule_calling_points, \
           COALESCE(tr.skipped_stations, '{}') AS schedule_skipped_stations, \
           tr.platform AS schedule_platform, tr.planned_platform AS schedule_planned_platform, \
           tr.id AS trains_id, \
           cs.status, cs.last_reported_location, cs.last_event_type, \
           cs.delay_minutes, cs.delay_minutes AS working_delay_minutes, \
           cs.next_calling_point, cs.eta_next, cs.eta_source, \
           tt.custom_name, \
           (SELECT COUNT(*) FROM group_trains gt WHERE gt.train_subscription_id = tt.id) \
               AS shared_group_count \
    FROM train_subscriptions tt \
    LEFT JOIN trains tr ON tr.id = tt.trains_id \
    LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
    LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs)::bpchar \
    LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs)::bpchar \
    LEFT JOIN stations ssd ON ssd.crs = UPPER(tr.destination_crs)::bpchar";

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
    /// The delay at the passenger's own stop, against the PUBLIC timetable
    /// (`data::stop_delay`, design doc §9 decision 2): see `TrackedTrainState::delay_minutes`. `None` until
    /// known.
    pub delay_minutes: Option<i32>,
    /// What `delay_minutes` was measured against (`public`, `publicSchedule`
    /// or `working`, see `common::public_delay::DelayBasis`); `None` exactly
    /// when `delay_minutes` is.
    #[sqlx(skip)]
    pub delay_basis: Option<crate::data::stop_delay::DelayBasis>,
    /// `true` while `delay_minutes` is a forecast (the train has not reported
    /// at that stop yet).
    #[sqlx(skip)]
    pub delay_provisional: bool,
    /// TRUST's running delay against the working timetable
    /// (`train_current_state.delay_minutes`), internal: the input to the
    /// per-stop estimates and to the forecast above. Never serialized.
    #[serde(skip_serializing)]
    pub working_delay_minutes: Option<i32>,
    /// The shared `trains` row, internal: what the delay is read for.
    #[serde(skip_serializing)]
    pub trains_id: Option<i64>,
    pub tracked_at: DateTime<Utc>,
    pub custom_name: Option<String>,
    /// See `TrackedTrainState::shared_group_count`'s doc comment -- same
    /// correlated-subquery mechanism, same reason (letting
    /// `/train/[uid]/[date]`'s tracking overlay, which reads this list
    /// rather than `GET /Train/{trackingId}`, warn on delete too).
    pub shared_group_count: i64,
    /// `serviceMode` (`train`/`replacementBus`/`bus`/`ferry`) and
    /// `liveTracking` (`false` for a bus or ferry, which TRUST never
    /// reports), from `schedule_services`; filled after the read by
    /// `data::schedule_services::attach`. A train when unknown.
    #[sqlx(skip)]
    #[serde(flatten)]
    pub service: crate::data::schedule_services::ServiceModeFields,
}

impl crate::data::stop_delay::PublicDelayFields for TrackedTrainState {
    fn delay_target(&self) -> Option<crate::data::stop_delay::StopDelayTarget> {
        crate::data::stop_delay::target(
            self.trains_id,
            self.train_uid.as_deref(),
            self.service_date,
            self.pin_destination_crs.as_deref(),
            self.working_delay_minutes,
        )
    }

    fn set_public_delay(&mut self, delay: Option<crate::data::stop_delay::StopDelay>) {
        (self.delay_minutes, self.delay_basis, self.delay_provisional) =
            crate::data::stop_delay::split(delay);
    }
}

impl crate::data::stop_delay::PublicDelayFields for TrackedTrainListItem {
    fn delay_target(&self) -> Option<crate::data::stop_delay::StopDelayTarget> {
        crate::data::stop_delay::target(
            self.trains_id,
            self.train_uid.as_deref(),
            self.service_date,
            self.pin_destination_crs.as_deref(),
            self.working_delay_minutes,
        )
    }

    fn set_public_delay(&mut self, delay: Option<crate::data::stop_delay::StopDelay>) {
        (self.delay_minutes, self.delay_basis, self.delay_provisional) =
            crate::data::stop_delay::split(delay);
    }
}

/// A user's own tracked trains, most-recently-tracked first (`tracked_at
/// DESC`, deliberately NOT `pin_scheduled_departure` -- a train pinned a
/// month in advance would otherwise sit ahead of one pinned five minutes
/// ago for a service that's delayed right now, which is very likely the
/// one thing the caller actually wants to check on; see Decision 2 of the
/// design spec), capped at `MINE_LIST_LIMIT` rows. No status-based
/// filtering: `train_current_state.status` CAN reach `'completed'` now
/// (`trust_schema::journey::apply_movement`'s confirmed-terminus-ARRIVAL
/// check), but only when `trains.destination_crs` already happened to be
/// known as that event was ingested -- which the
/// backlog-replay-before-schedule-match ordering in
/// `routes::train::enrich_shared_train` often means it wasn't, and nothing
/// re-derives it afterwards. `journey::apply_confirmed_arrival` closes that
/// for the two single-train read routes by reading the finished journey off
/// its own timeline, but deliberately not for this batched list (see that
/// function's own doc comment), so a long-finished train can still show
/// here as `'en_route'`. An "active only" filter would therefore silently
/// do almost nothing while implying curation that isn't happening; this
/// function intentionally does not attempt one.
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
                cs.status, cs.delay_minutes, cs.delay_minutes AS working_delay_minutes, \
                tr.id AS trains_id, tt.tracked_at, tt.custom_name, \
                (SELECT COUNT(*) FROM group_trains gt WHERE gt.train_subscription_id = tt.id) \
                    AS shared_group_count \
         FROM train_subscriptions tt \
         LEFT JOIN trains tr ON tr.id = tt.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
         LEFT JOIN stations so ON so.crs = UPPER(tt.pin_origin_crs)::bpchar \
         LEFT JOIN stations sd ON sd.crs = UPPER(tt.pin_destination_crs)::bpchar \
         WHERE tt.user_id = $1 \
         ORDER BY tt.tracked_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    let mut rows = rows;
    crate::data::stop_delay::apply_public_delays(pool, &mut rows).await?;
    crate::data::schedule_services::attach(pool, &mut rows).await;
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
    let mut rows: Vec<TrackedTrainState> = row.into_iter().collect();
    crate::data::stop_delay::apply_public_delays(pool, &mut rows).await?;
    Ok(rows.pop())
}

/// [`get_by_tracking_id`] for many ids in one query, keyed by id. An id with
/// no row is simply absent. Backs the journey detail
/// (`routes::journeys::build_journey_detail_response`), which used to call
/// `get_by_tracking_id` once per leg (DB review part 2, DB2-14).
pub async fn get_by_tracking_ids(
    pool: &PgPool,
    ids: &[i64],
) -> anyhow::Result<std::collections::HashMap<i64, TrackedTrainState>> {
    if ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows = sqlx::query_as::<_, TrackedTrainState>(&format!(
        "{TRACKED_TRAIN_STATE_SELECT} WHERE tt.id = ANY($1)"
    ))
    .bind(ids)
    .fetch_all(pool)
    .await?;
    let mut rows = rows;
    crate::data::stop_delay::apply_public_delays(pool, &mut rows).await?;
    Ok(rows.into_iter().map(|row| (row.id, row)).collect())
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
///
/// **Journey-leg cleanup (2026-09-26 review, Low finding 13).**
/// `journey_legs.train_subscription_id` is `ON DELETE SET NULL`
/// (`20260922090000_journeys.sql`), so this delete alone already un-links
/// any leg that pointed at this subscription -- but that FK only touches
/// `train_subscription_id`, never `journey_legs.match_mode`. Left alone, a
/// leg that was `'manual'` (a direct pin/known-train pick, or an earlier
/// "Change train" re-pick -- see `journeys::set_leg_train_subscription`)
/// or `'auto'` (a template sweep's own auto-commit --
/// `notifier::queries::auto_commit_leg_to_train`) would end up with NO
/// subscription attached but a `match_mode` column still claiming it's
/// resolved. Two concrete failures came out of that mismatch: (1)
/// `notifier::queries::unmatched_auto_legs_for_commit_check`, the template
/// auto-sweep, only ever selects `WHERE match_mode = 'unmatched'` -- an
/// orphaned `'auto'` leg from a `default_match_mode = 'auto'` template
/// would never be reconsidered for re-matching, silently going dark
/// instead of the sweep picking a fresh candidate the way it would for a
/// leg that had never matched at all; (2) `routes::journeys::build_journey_detail_response`
/// derives `trackedTrainState: None` purely from `train_subscription_id IS
/// NULL`, regardless of `match_mode`, so the frontend's `JourneyLegCard`
/// would render this leg as "Open" (offering `JourneyLegCandidates`) while
/// the DB still called it `'manual'`/`'auto'` -- a status column lying
/// about a leg the UI itself no longer treats as matched. Resetting
/// `match_mode` to `'unmatched'` here (scoped by the same ownership guard
/// as the delete below, so a not-this-caller's `id` touches nothing) makes
/// both facts agree again: the leg reads as genuinely unmatched everywhere,
/// exactly the state it would be in had it never been matched, and (for a
/// leg whose journey does trace back to an `'auto'` template) becomes
/// reachable by the auto-sweep again on its next cycle.
pub async fn delete_tracked_train(pool: &PgPool, id: i64, user_id: &str) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;

    // Ownership-scoped via the same `EXISTS` idiom
    // `journeys::set_leg_train_subscription` already uses: this only
    // matches `journey_legs` rows at all when `id` is a subscription that
    // truly belongs to `user_id`, so a not-this-caller's (or nonexistent)
    // `id` leaves every other user's `journey_legs` rows completely
    // untouched, exactly like the `DELETE` below.
    sqlx::query(
        "UPDATE journey_legs jl SET match_mode = 'unmatched' \
         WHERE jl.train_subscription_id = $1 \
           AND EXISTS ( \
               SELECT 1 FROM train_subscriptions ts WHERE ts.id = $1 AND ts.user_id = $2 \
           )",
    )
    .bind(id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;

    let result = sqlx::query("DELETE FROM train_subscriptions WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    let deleted = result.rows_affected() > 0;

    tx.commit().await?;
    Ok(deleted)
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
            skipped_stations: vec![],
            platform: None,
            planned_platform: None,
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
    fn a_pin_beyond_the_timetable_horizon_is_rejected() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let last_day: DateTime<Utc> = "2026-06-22T18:00:00Z".parse().unwrap();
        assert!(validate_pin(&pin("WAT", last_day), now).is_ok());
        let too_far: DateTime<Utc> = "2026-06-23T08:00:00Z".parse().unwrap();
        assert!(
            validate_pin(&pin("WAT", too_far), now)
                .unwrap_err()
                .contains("too far ahead")
        );
        let far_future: DateTime<Utc> = "2090-01-01T08:00:00Z".parse().unwrap();
        assert!(validate_pin(&pin("WAT", far_future), now).is_err());
        // A near-term departure with a far-off service_date is caught too.
        let mut mismatched = pin("WAT", now + chrono::Duration::hours(1));
        mismatched.service_date = "2090-01-01".parse().unwrap();
        assert!(validate_pin(&mismatched, now).is_err());
    }

    #[test]
    fn pin_fields_are_bounded_and_shaped() {
        let now: DateTime<Utc> = "2026-06-15T12:00:00Z".parse().unwrap();
        let ok = || pin("WAT", now + chrono::Duration::hours(1));
        let mut full = ok();
        full.destination_crs = Some("RDG".to_string());
        full.operator = Some("South Western Railway".to_string());
        full.platform = Some("13-14".to_string());
        full.planned_platform = Some("10A".to_string());
        full.skipped_stations = vec!["CLJ".to_string(), "wok".to_string()];
        assert!(validate_pin(&full, now).is_ok());

        let huge = "x".repeat(8 * 1024 * 1024);
        let cases: Vec<(&str, TrackPinRequest)> = vec![
            (
                "digit origin",
                TrackPinRequest {
                    origin_crs: "W1T".to_string(),
                    ..ok()
                },
            ),
            (
                "bad destination",
                TrackPinRequest {
                    destination_crs: Some("Reading".to_string()),
                    ..ok()
                },
            ),
            (
                "huge operator",
                TrackPinRequest {
                    operator: Some(huge.clone()),
                    ..ok()
                },
            ),
            (
                "long platform",
                TrackPinRequest {
                    platform: Some("123456789".to_string()),
                    ..ok()
                },
            ),
            (
                "long planned platform",
                TrackPinRequest {
                    planned_platform: Some(huge.clone()),
                    ..ok()
                },
            ),
            (
                "bad skipped",
                TrackPinRequest {
                    skipped_stations: vec![huge.clone()],
                    ..ok()
                },
            ),
            (
                "too many skipped",
                TrackPinRequest {
                    skipped_stations: vec!["CLJ".to_string(); 65],
                    ..ok()
                },
            ),
        ];
        for (what, bad) in cases {
            let err = validate_pin(&bad, now).expect_err(what);
            assert!(
                err.len() < 200,
                "{what}: the message must not echo the input"
            );
            assert!(
                !err.contains('_'),
                "{what}: user-facing copy leaked an identifier: {err}"
            );
        }
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
/// `403` -- the api-wide convention that "exists but not yours" looks
/// exactly like "does not exist", so ids cannot be probed.
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
            (tracked_train_id, user_id, operator, ticket_type, origin_crs, destination_crs, \
             current_departure_date, source) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id",
    )
    .bind(tracked_train_id)
    .bind(user_id)
    .bind(&entry.operator)
    .bind(&entry.ticket_type)
    .bind(&entry.origin_crs)
    .bind(&entry.destination_crs)
    .bind(entry.current_departure_date)
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
    /// See `ticket_extraction::PartialTicket::current_departure_date`'s own
    /// doc comment -- this is that same best-effort HINT, persisted
    /// verbatim by `create_ticket`. `GET /Train/tickets/{ticketId}/journey-leg-proposal`
    /// (`data::journey_leg_proposal::propose_window_leg`) is the one reader
    /// that does anything with it; every other existing reader of this
    /// struct (`TicketPanel.tsx`'s rendering, the Delay Repay route) simply
    /// ignores it, same as any other field it doesn't need.
    pub current_departure_date: Option<DateTime<Utc>>,
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
           so.name AS origin_name, sd.name AS destination_name, t.current_departure_date, \
           t.source, t.created_at, t.custom_name \
    FROM tracked_train_tickets t \
    LEFT JOIN stations so ON so.crs = UPPER(t.origin_crs)::bpchar \
    LEFT JOIN stations sd ON sd.crs = UPPER(t.destination_crs)::bpchar";

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
    /// `train_current_state.delay_minutes`: TRUST's running delay against
    /// the working timetable, the input to the projection below.
    delay_minutes: Option<i32>,
    custom_name: Option<String>,
    trains_id: Option<i64>,
    /// The train's terminus, the last fallback for where Delay Repay is
    /// measured (see [`TicketListRow::measured_at_crs`]).
    schedule_destination_crs: Option<String>,
    /// The delay against the public arrival at [`TicketListRow::measured_at_crs`],
    /// filled in by `list_tickets_for_user` after the read.
    #[sqlx(skip)]
    destination_delay: Option<crate::data::stop_delay::StopDelay>,
    /// Whether the train reached [`TicketListRow::measured_at_crs`]
    /// (`data::delay_repay_outcome`), filled in after the read.
    #[sqlx(skip)]
    outcome: Option<delay_repay_rules::Outcome>,
    /// The train's ATOC code from its schedule, filled in after the read.
    #[sqlx(skip)]
    atoc_code: Option<String>,
}

impl TicketListRow {
    /// Where this ticket's Delay Repay is measured: its own destination,
    /// else the tracked train's pin destination, else the train's terminus
    /// (the same order as `routes::train`'s `delay_repay_destination`).
    fn measured_at_crs(&self) -> Option<String> {
        [
            self.destination_crs.as_deref(),
            self.pin_destination_crs.as_deref(),
            self.schedule_destination_crs.as_deref(),
        ]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|crs| !crs.is_empty())
        .map(str::to_uppercase)
    }

    /// [`TicketListRow::measured_at_crs`]'s name, when it is the ticket's
    /// own destination (the only one this query names).
    fn measured_at_name(&self) -> Option<&str> {
        self.destination_crs
            .as_deref()
            .map(str::trim)
            .filter(|crs| !crs.is_empty())
            .and(self.destination_name.as_deref())
    }
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
    /// The Delay Repay fields, exactly as `GET .../delay-repay` serves
    /// them (`delay_repay_rules::assess`, flattened), so the two never
    /// disagree.
    #[serde(flatten)]
    pub delay_repay: delay_repay_rules::DelayRepayFields,
    pub custom_name: Option<String>,
}

/// One row of `GET /Train/tickets/mine`. Its Delay Repay fields come from
/// `delay_repay_rules::assess`, as `routes/train.rs`'s
/// `build_delay_repay_response` does, so the two independently computed
/// estimates for the same `(ticket, tracked train)` pair can never
/// disagree.
fn build_ticket_list_item(row: TicketListRow) -> TicketListItem {
    let measured_at_crs = row.measured_at_crs();
    let delay_repay = delay_repay_rules::assess(delay_repay_rules::AssessInputs {
        operator: row.operator.as_deref(),
        ticket_type: row.ticket_type.as_deref(),
        atoc_code: row.atoc_code.as_deref(),
        delay: row.destination_delay,
        outcome: row.outcome,
        measured_at_crs: measured_at_crs.as_deref(),
        measured_at_name: row.measured_at_name(),
    });

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
        delay_repay,
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
                cs.status, cs.delay_minutes, t.custom_name, \
                tt.trains_id, tr.destination_crs AS schedule_destination_crs \
         FROM tracked_train_tickets t \
         LEFT JOIN train_subscriptions tt ON tt.id = t.tracked_train_id \
         LEFT JOIN trains tr ON tr.id = tt.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = tt.trains_id \
         LEFT JOIN stations so ON so.crs = UPPER(t.origin_crs)::bpchar \
         LEFT JOIN stations sd ON sd.crs = UPPER(t.destination_crs)::bpchar \
         WHERE t.user_id = $1 \
         ORDER BY t.created_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(MINE_TICKETS_LIMIT)
    .fetch_all(pool)
    .await?;
    let mut rows = rows;
    // One batched read for every attached ticket's destination delay; a
    // standalone ticket (no train) has no target and keeps `None`.
    let mut positions = Vec::new();
    let mut targets = Vec::new();
    for (index, row) in rows.iter().enumerate() {
        let target = row.service_date.and_then(|service_date| {
            crate::data::stop_delay::target(
                row.trains_id,
                row.train_uid.as_deref(),
                service_date,
                row.measured_at_crs().as_deref(),
                row.delay_minutes,
            )
        });
        if let Some(target) = target.filter(|target| target.stop_crs.is_some()) {
            positions.push(index);
            targets.push(target);
        }
    }
    if !targets.is_empty() {
        let delays = crate::data::stop_delay::stop_delays(pool, &targets).await?;
        let outcome_targets: Vec<crate::data::delay_repay_outcome::OutcomeTarget> = targets
            .into_iter()
            .map(|t| crate::data::delay_repay_outcome::OutcomeTarget {
                trains_id: t.trains_id,
                train_uid: t.train_uid,
                service_date: t.service_date,
                destination_crs: t.stop_crs.unwrap_or_default(),
            })
            .collect();
        let outcomes = crate::data::delay_repay_outcome::outcomes(pool, &outcome_targets).await?;
        for ((index, delay), outcome) in positions.into_iter().zip(delays).zip(outcomes) {
            rows[index].destination_delay = delay;
            rows[index].outcome = outcome;
        }
    }
    attach_atoc_codes(pool, &mut rows).await;
    Ok(rows.into_iter().map(build_ticket_list_item).collect())
}

/// Each attached ticket's train's ATOC code (`data::train_operator`), one
/// read per service date among the rows. Best effort: a failed read leaves
/// the ticket's own operator text to decide the scheme.
async fn attach_atoc_codes(pool: &PgPool, rows: &mut [TicketListRow]) {
    let mut uids_by_date: std::collections::HashMap<chrono::NaiveDate, Vec<String>> =
        std::collections::HashMap::new();
    for row in rows.iter() {
        if let (Some(date), Some(uid)) = (row.service_date, &row.train_uid) {
            uids_by_date.entry(date).or_default().push(uid.clone());
        }
    }
    for (date, uids) in uids_by_date {
        match crate::data::train_operator::operators_for_trains(pool, &uids, date).await {
            Ok(operators) => {
                for row in rows.iter_mut() {
                    if row.service_date == Some(date)
                        && let Some(operator) =
                            row.train_uid.as_ref().and_then(|uid| operators.get(uid))
                    {
                        row.atoc_code = Some(operator.code.clone());
                    }
                }
            }
            Err(err) => {
                tracing::warn!(error = ?err, %date, "failed to read train operators for Delay Repay");
            }
        }
    }
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
            trains_id: Some(1),
            schedule_destination_crs: Some("ABD".to_string()),
            destination_delay: delay_minutes.map(|minutes| crate::data::stop_delay::StopDelay {
                minutes,
                basis: crate::data::stop_delay::DelayBasis::Public,
                provisional: false,
            }),
            outcome: None,
            atoc_code: None,
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
            trains_id: None,
            schedule_destination_crs: None,
            destination_delay: None,
            outcome: None,
            atoc_code: None,
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
            .delay_repay
            .estimate
            .expect("LNER + 45 minutes should clear the DR30 30-minute band");
        assert_eq!(estimate.scheme, "DR30");
        assert_eq!(estimate.percentage, Some(50));
        assert_eq!(
            item.delay_repay.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
        assert_eq!(item.delay_repay.delay_minutes, Some(45));
    }

    #[test]
    fn a_projected_destination_delay_is_provisional_and_names_where_it_was_measured() {
        let mut row = row(Some("Southeastern"), None);
        row.destination_delay = Some(crate::data::stop_delay::StopDelay {
            minutes: 30,
            basis: crate::data::stop_delay::DelayBasis::PublicSchedule,
            provisional: true,
        });
        let item = build_ticket_list_item(row);
        assert_eq!(item.delay_repay.delay_minutes, Some(30));
        assert!(item.delay_repay.provisional);
        assert_eq!(item.delay_repay.measured_at_crs.as_deref(), Some("EDB"));
        let estimate = item.delay_repay.estimate.unwrap();
        assert_eq!((estimate.band_minutes, estimate.provisional), (30, true));
    }

    #[test]
    fn no_operator_yields_no_estimate_but_still_a_real_claim_link_and_disclaimer() {
        let item = build_ticket_list_item(row(None, Some(45)));

        assert_eq!(item.delay_repay.estimate, None);
        assert_eq!(
            item.delay_repay.claim_url,
            delay_repay_rules::GENERIC_CLAIM_URL
        );
        assert_eq!(
            item.delay_repay.disclaimer,
            delay_repay_rules::ROUTE_DISCLAIMER
        );
    }

    #[test]
    fn no_delay_data_yields_no_estimate_but_claim_url_and_disclaimer_are_still_populated() {
        let item = build_ticket_list_item(row(Some("LNER"), None));

        assert_eq!(item.delay_repay.estimate, None);
        assert_eq!(item.delay_repay.delay_minutes, None);
        assert_eq!(
            item.delay_repay.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
        assert_eq!(
            item.delay_repay.disclaimer,
            delay_repay_rules::ROUTE_DISCLAIMER
        );
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
        assert_eq!(item.delay_repay.delay_minutes, None);
        assert_eq!(item.delay_repay.estimate, None);
        assert_eq!(
            item.delay_repay.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
        assert_eq!(
            item.delay_repay.disclaimer,
            delay_repay_rules::ROUTE_DISCLAIMER
        );
    }

    #[test]
    fn a_standalone_ticket_with_no_operator_still_gets_the_generic_claim_url() {
        let item = build_ticket_list_item(standalone_row(None));

        assert_eq!(item.delay_repay.estimate, None);
        assert_eq!(
            item.delay_repay.claim_url,
            delay_repay_rules::GENERIC_CLAIM_URL
        );
        assert_eq!(
            item.delay_repay.disclaimer,
            delay_repay_rules::ROUTE_DISCLAIMER
        );
    }

    /// The list serves the same 2026-10-07 fields as the route: a train
    /// that did not reach the destination names it (with the ticket's own
    /// station name), and the train's code decides an unrecognised operator.
    #[test]
    fn not_reached_and_the_trains_operator_code_reach_the_list() {
        let mut not_reached = row(Some("Trainline"), Some(70));
        not_reached.outcome = Some(delay_repay_rules::Outcome::NotReached);
        let item = build_ticket_list_item(not_reached);
        assert_eq!(
            item.delay_repay.outcome,
            Some(delay_repay_rules::Outcome::NotReached)
        );
        assert_eq!(item.delay_repay.delay_minutes, None);
        assert_eq!(item.delay_repay.estimate, None);
        assert_eq!(
            item.delay_repay.measured_at_name.as_deref(),
            Some("Edinburgh Waverley")
        );

        let mut row = row(Some("Trainline"), Some(45));
        row.atoc_code = Some("GR".to_string());
        let item = build_ticket_list_item(row);
        assert_eq!(item.delay_repay.estimate.unwrap().scheme, "DR30");
        assert_eq!(
            item.delay_repay.claim_url,
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
        let json = serde_json::to_value(build_ticket_list_item(standalone_row(None))).unwrap();
        assert!(json["claimUrl"].is_string() && json["disclaimer"].is_string());
        assert!(json.get("delayRepay").is_none(), "flattened");
    }
}

#[cfg(test)]
#[expect(
    clippy::items_after_statements,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::unnecessary_wraps,
    reason = "test code: fixtures sit next to their use; paired test values share names; scenario tests read top to bottom; fakes mirror the signatures they stand in for"
)]
mod db_tests {
    use super::*;
    use chrono::NaiveDate;
    use common::TrainMovementEventMessage;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// The database's own `CURRENT_DATE`, which is what every query under
    /// test compares `service_date` against. Seeding from
    /// `Utc::now().date_naive()` instead only agreed with it because the
    /// server happened to run in UTC (DB review 2026-09-27 B4); this holds
    /// whatever the server's or the session's `TimeZone` is.
    async fn db_today(pool: &PgPool) -> NaiveDate {
        sqlx::query_scalar("SELECT CURRENT_DATE")
            .fetch_one(pool)
            .await
            .expect("read CURRENT_DATE")
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
        .bind("2026-09-02".parse::<NaiveDate>().unwrap())
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
            current_departure_date: None,
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
        let deleted = delete_ticket(&pool, 99_999_999, "TEST-TICKET-DELETE-NOBODY")
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
                -- --ignored --test-threads=1`"]
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
            .bind("2026-09-02".parse::<NaiveDate>().unwrap())
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

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                shared_group_count_reflects_how_many_groups_a_train_is_shared_into \
                -- --ignored --test-threads=1`"]
    async fn shared_group_count_reflects_how_many_groups_a_train_is_shared_into() {
        // Exercises the correlated `(SELECT COUNT(*) FROM group_trains ...)`
        // subquery embedded directly in `TRACKED_TRAIN_STATE_SELECT` and
        // `list_tracked_trains_for_user`'s own query -- both
        // `get_by_tracking_id` and `list_tracked_trains_for_user` must agree
        // on the same count for the same subscription.
        let pool = connect().await;
        let user_id = "TEST-SHARED-GROUP-COUNT-USER";
        seed_user(&pool, user_id).await;

        let shared_id = seed_tracked_train(&pool, user_id).await;
        let unshared_id = seed_tracked_train(&pool, user_id).await;

        let group_a = crate::data::groups::create_group(&pool, "Group A", user_id)
            .await
            .expect("create group A");
        let group_b = crate::data::groups::create_group(&pool, "Group B", user_id)
            .await
            .expect("create group B");
        for group_id in [&group_a, &group_b] {
            sqlx::query(
                "INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at) \
                 VALUES ($1, $2, $3, NOW())",
            )
            .bind(group_id)
            .bind(shared_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("share the train into a group");
        }

        let shared_state = get_by_tracking_id(&pool, shared_id)
            .await
            .expect("read shared train")
            .expect("row present");
        assert_eq!(shared_state.shared_group_count, 2);

        let unshared_state = get_by_tracking_id(&pool, unshared_id)
            .await
            .expect("read unshared train")
            .expect("row present");
        assert_eq!(unshared_state.shared_group_count, 0);

        let list = list_tracked_trains_for_user(&pool, user_id)
            .await
            .expect("list tracked trains");
        let by_id = |id: i64| list.iter().find(|r| r.id == id).expect("row present");
        assert_eq!(by_id(shared_id).shared_group_count, 2);
        assert_eq!(by_id(unshared_id).shared_group_count, 0);

        sqlx::query("DELETE FROM groups WHERE id = ANY($1)")
            .bind(vec![group_a, group_b])
            .execute(&pool)
            .await
            .ok();
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

    fn fixture_event(tracked_train_id: i64, dedup_key: &str) -> TrainMovementEventMessage {
        TrainMovementEventMessage {
            tracked_train_id,
            resolved_train_uid: None,
            resolved_train_id: None,
            identity_date: None,
            dedup_key: dedup_key.to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("72410".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: Some("2026-09-05T18:15:00Z".parse().unwrap()),
            gbtt_timestamp: None,
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
        let service_date: NaiveDate = "2026-09-05".parse().unwrap();
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

    /// **Low finding #2 of the 2026-09-25 review, this fix's own regression
    /// test.** A Cancellation-derived event (`status == "cancelled"`) never
    /// carries `resolved_train_id` -- see `fixture_event`'s defaults, and
    /// `trust-consumer::process.rs`'s own `TrustMessage::Cancellation`
    /// handler, which this fixture mirrors -- so before this fix
    /// `resolution_status` stayed `'pending'` forever for a subscription
    /// whose train was cancelled before ever resolving. It must now flip to
    /// `'unresolved'`, the terminal "nothing left to do" value
    /// `list_active_tracked_trains`'s own doc comment already anticipated a
    /// future writer for.
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_train_event -- --ignored --test-threads=1`"]
    async fn upsert_train_event_with_status_cancelled_flips_a_pending_subscription_to_unresolved() {
        let pool = connect().await;
        let user_id = "TEST-UPSERT-CANCELLED-PENDING";
        seed_user(&pool, user_id).await;
        let tracking_id = seed_tracked_train(&pool, user_id).await;

        let mut event = fixture_event(tracking_id, "dedup-cancelled-pending");
        event.status = "cancelled".to_string();
        // A real Cancellation carries neither field -- see this test's own
        // doc comment.
        event.resolved_train_uid = None;
        event.resolved_train_id = None;

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(
            state.resolution_status, "unresolved",
            "a cancellation for a still-pending subscription must stop it being retried forever"
        );

        cleanup_user(&pool, user_id).await;
    }

    /// The companion guard: a subscription that had ALREADY resolved (its
    /// train departed, then was cancelled mid-journey) must not be
    /// downgraded to `'unresolved'` -- that would be strictly less
    /// informative (the train WAS found) and is exactly finding #1's
    /// "regress an already-advanced status" mistake played out on this
    /// column instead of `train_current_state.status`.
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                upsert_train_event -- --ignored --test-threads=1`"]
    async fn upsert_train_event_with_status_cancelled_does_not_downgrade_an_already_resolved_subscription()
     {
        let pool = connect().await;
        let user_id = "TEST-UPSERT-CANCELLED-RESOLVED";
        seed_user(&pool, user_id).await;
        let tracking_id = seed_tracked_train(&pool, user_id).await;
        sqlx::query("UPDATE train_subscriptions SET resolution_status = 'resolved' WHERE id = $1")
            .bind(tracking_id)
            .execute(&pool)
            .await
            .expect("seed an already-resolved subscription");

        let mut event = fixture_event(tracking_id, "dedup-cancelled-resolved");
        event.status = "cancelled".to_string();
        event.resolved_train_uid = None;
        event.resolved_train_id = None;

        upsert_train_event(&pool, &event)
            .await
            .expect("upsert train event");

        let state = get_by_tracking_id(&pool, tracking_id)
            .await
            .expect("read tracked train")
            .expect("tracked train exists");
        assert_eq!(
            state.resolution_status, "resolved",
            "an already-resolved subscription must not be downgraded by a later cancellation"
        );

        cleanup_user(&pool, user_id).await;
    }

    // --- H4 residual (2026-10-01): a Reinstatement reopens what a Cancellation closed ---

    fn at(raw: &str) -> Option<DateTime<Utc>> {
        Some(raw.parse().unwrap())
    }

    async fn resolution_status_of(pool: &PgPool, tracked_train_id: i64) -> String {
        sqlx::query_scalar("SELECT resolution_status FROM train_subscriptions WHERE id = $1")
            .bind(tracked_train_id)
            .fetch_one(pool)
            .await
            .expect("read resolution_status")
    }

    async fn is_listed_active(pool: &PgPool, tracked_train_id: i64) -> bool {
        list_active_tracked_trains(pool)
            .await
            .expect("list_active_tracked_trains")
            .iter()
            .any(|tracked| tracked.id == tracked_train_id)
    }

    /// Cancel -> trust-consumer restart (nothing in memory, so it never
    /// forwards the Reinstatement) -> the Reinstatement arrives only through
    /// trust-backlog-consumer's ingest -> the next Movement resolves the
    /// subscription. `backlog_knows_uid` is whether trust-backlog-consumer
    /// still had the train's Activation parked; without it the shared row
    /// is found by the `train_id` already written onto it.
    async fn reinstatement_after_restart_reopens_and_a_movement_resolves(backlog_knows_uid: bool) {
        let pool = connect().await;
        let user_id = if backlog_knows_uid {
            "TEST-H4-REOPEN-UID"
        } else {
            "TEST-H4-REOPEN-NOUID"
        };
        let train_uid = if backlog_knows_uid {
            "TH4RU1"
        } else {
            "TH4RN1"
        };
        let train_id = if backlog_knows_uid {
            "TH4RU1ID01"
        } else {
            "TH4RN1ID01"
        };
        sqlx::query("DELETE FROM trains WHERE train_uid = $1")
            .bind(train_uid)
            .execute(&pool)
            .await
            .expect("pre-clean trains");
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let today = db_today(&pool).await;

        let trains_id = crate::data::trains::find_or_create_train(&pool, train_uid, today)
            .await
            .expect("seed the shared trains row");
        crate::data::trains::mark_train_resolved(&pool, trains_id, train_id)
            .await
            .expect("seed its train_id, as the backlog Activation ingest does");
        let (tracked_train_id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, trains_id, \
                 resolution_status) \
             VALUES ($1, $2, 'EUS', $3, $4, 'schedule_matched') RETURNING id",
        )
        .bind(user_id)
        .bind(today)
        .bind(today.and_hms_opt(18, 15, 0).unwrap().and_utc())
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .expect("seed a schedule-matched subscription");

        // The Cancellation, through trust-consumer's live path.
        let mut cancel = fixture_event(tracked_train_id, "h4-reopen-cancel");
        cancel.msg_type = "0002".to_string();
        cancel.event_type = None;
        cancel.planned_timestamp = None;
        cancel.actual_timestamp = at("2026-09-05T18:05:00Z");
        cancel.status = "cancelled".to_string();
        upsert_train_event(&pool, &cancel).await.expect("cancel");
        assert_eq!(
            resolution_status_of(&pool, tracked_train_id).await,
            "unresolved"
        );
        assert!(!is_listed_active(&pool, tracked_train_id).await);

        // trust-consumer restarts here. It never sees this subscription
        // again (not listed), so only trust-backlog-consumer forwards the
        // Reinstatement.
        let reinstatement = common::TrustBacklogEventMessage {
            crs: None,
            train_uid: backlog_knows_uid.then(|| train_uid.to_string()),
            train_id: train_id.to_string(),
            service_date: today,
            msg_type: "0005".to_string(),
            event_type: None,
            planned_timestamp: None,
            actual_timestamp: at("2026-09-05T18:10:00Z"),
            variation_status: None,
            delay_minutes: None,
            dedup_key: format!("h4-reopen-reinstate-{user_id}"),
            gbtt_timestamp: None,
        };
        let results = crate::data::trust_event_backlog::ingest_shared_movements_batch(
            &pool,
            std::slice::from_ref(&reinstatement),
        )
        .await;
        assert!(results.iter().all(Result::is_ok), "{results:?}");
        assert_eq!(
            resolution_status_of(&pool, tracked_train_id).await,
            "schedule_matched",
            "the reinstatement restores the status the cancellation replaced"
        );
        let status: String =
            sqlx::query_scalar("SELECT status FROM train_current_state WHERE trains_id = $1")
                .bind(trains_id)
                .fetch_one(&pool)
                .await
                .expect("read train_current_state");
        assert_eq!(status, "en_route");
        assert!(
            is_listed_active(&pool, tracked_train_id).await,
            "listed again, so a restarted trust-consumer can claim it"
        );

        // The next Movement, as the restarted trust-consumer's pin claim
        // sends it.
        let mut movement = fixture_event(tracked_train_id, "h4-reopen-movement");
        movement.resolved_train_uid = Some(train_uid.to_string());
        movement.resolved_train_id = Some(train_id.to_string());
        movement.actual_timestamp = at("2026-09-05T18:20:00Z");
        upsert_train_event(&pool, &movement)
            .await
            .expect("movement");
        assert_eq!(
            resolution_status_of(&pool, tracked_train_id).await,
            "resolved"
        );

        cleanup_user(&pool, user_id).await;
        sqlx::query("DELETE FROM trains WHERE train_uid = $1")
            .bind(train_uid)
            .execute(&pool)
            .await
            .expect("clean trains");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_backlog_reinstatement -- --ignored --test-threads=1`"]
    async fn a_backlog_reinstatement_reopens_a_cancelled_subscription_after_a_restart() {
        reinstatement_after_restart_reopens_and_a_movement_resolves(true).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_backlog_reinstatement -- --ignored --test-threads=1`"]
    async fn a_backlog_reinstatement_without_a_parked_activation_still_reopens() {
        reinstatement_after_restart_reopens_and_a_movement_resolves(false).await;
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
                create_subscription_for_train_inherits_known_schedule_data \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_inherits_known_schedule_data() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-TRACK";
        seed_user(&pool, user_id).await;

        let service_date: NaiveDate = "2026-09-06".parse().unwrap();
        let scheduled_departure: DateTime<Utc> = "2026-09-06T19:15:00Z".parse().unwrap();
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
            DateTime<Utc>,
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
            Option<DateTime<Utc>>,
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

    /// DB2-21: a second call for the same `(user, train)` while the first
    /// call's transaction is still open must wait for it and then return
    /// the same row. Before the advisory lock it didn't wait: it saw no
    /// committed row and inserted a duplicate.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_serialises_concurrent_calls \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_serialises_concurrent_calls() {
        let pool = connect().await;
        let user_id = "TEST-DB2-21-CONCURRENT";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-DB2-21-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        let mut first_tx = pool.begin().await.unwrap();
        let first = create_subscription_for_train(&mut *first_tx, trains_id, user_id)
            .await
            .expect("first call");

        let second = tokio::spawn({
            let pool = pool.clone();
            async move { create_subscription_for_train(&pool, trains_id, user_id).await }
        });
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        assert!(
            !second.is_finished(),
            "the second call must wait for the first call's transaction"
        );
        first_tx.commit().await.unwrap();
        let second = second.await.unwrap().expect("second call");
        assert_eq!(second, first, "both calls must return the one subscription");

        let (rows,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM train_subscriptions WHERE user_id = $1 AND trains_id = $2",
        )
        .bind(user_id)
        .bind(trains_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(rows, 1);

        cleanup_user(&pool, user_id).await;
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
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

        // Today, so this test keeps proving what it claims: the row must be
        // excluded because its `pin_*` columns are NULL, NOT because the
        // 2026-09-25 Medium 7 `service_date` floor swept it out for an
        // unrelated reason.
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-PRUNED-NR-PRIMARY-UID",
            db_today(&pool).await,
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
        assert_eq!(
            row_origin, None,
            "this subscription never had schedule data"
        );
        assert_eq!(row_departure, None);

        let pending = list_pending_pins_for_schedule_match(&pool).await.expect(
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
        // Today, for `list_active_tracked_trains`' `service_date` floor (the
        // 2026-09-25 Medium 8 fix) -- `create_subscription_for_train` copies
        // this date onto both subscriptions.
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, train_uid, db_today(&pool).await)
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

    /// 2026-09-26 review, Medium finding 6: reproduces the cross-match
    /// residual this task's transaction fix closes. Two DIFFERENT physical
    /// trains (different `train_uid`s) end up reported under the SAME
    /// TRUST `train_id` for the SAME `service_date` -- a real-world
    /// cross-matching incident, not a contrived input (see
    /// `20260925221500_close_out_trains_train_id_service_date_collisions.sql`'s
    /// own header for the 2026-09-25 incident this exact shape came from).
    /// The second subscriber's own resolution hits
    /// `trains_train_id_service_date`'s unique index the moment
    /// `mark_train_resolved` tries to write the SAME `train_id` onto a
    /// SECOND `trains` row for that date.
    ///
    /// Before this task's fix, `flip_legacy_resolution` first committed
    /// `resolution_status = 'resolved'` on the second subscriber's row as
    /// its own independent statement, then attempted the colliding write
    /// separately -- so the second subscriber's row was left reading
    /// `'resolved'` forever, with `trains_id` still `NULL`: "resolved" but
    /// never actually claimed, and invisible to every sweep that only
    /// re-checks still-`'pending'` rows. This proves that can no longer
    /// happen: the second call must fail outright (surfacing the
    /// collision instead of burying it), and the second subscriber's row
    /// must come back completely unchanged -- still `'pending'`, still no
    /// `trains_id` -- so a later, correct resolution can still claim it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_cross_matched_second_resolution_does_not_strand_the_subscription_as_resolved_with_no_trains_id \
                -- --ignored --test-threads=1`"]
    async fn a_cross_matched_second_resolution_does_not_strand_the_subscription_as_resolved_with_no_trains_id()
     {
        let pool = connect().await;
        let first_user_id = "TEST-M6-COLLISION-USER-1";
        let second_user_id = "TEST-M6-COLLISION-USER-2";
        seed_user(&pool, first_user_id).await;
        seed_user(&pool, second_user_id).await;

        let first_tracking_id = seed_tracked_train(&pool, first_user_id).await;
        let second_tracking_id = seed_tracked_train(&pool, second_user_id).await;

        // First subscriber resolves cleanly to its own physical train.
        let mut first_event = fixture_event(first_tracking_id, "dedup-m6-collision-1");
        first_event.resolved_train_uid = Some("TEST-M6-UID-A".to_string());
        first_event.resolved_train_id = Some("TEST-M6-SHARED-TRAIN-ID".to_string());
        upsert_train_event(&pool, &first_event)
            .await
            .expect("the first subscriber's live-TRUST resolution must succeed");

        // Second subscriber resolves to a DIFFERENT physical train (a
        // different `train_uid`) but the SAME `train_id` and the same
        // `service_date` (both fixtures share `seed_tracked_train`'s
        // hardcoded "2026-09-02") -- the cross-match collision. This must
        // fail: `find_or_create_train` mints a fresh, distinct `trains` row
        // for `TEST-M6-UID-B` (a different `train_uid` never collides on
        // its own upsert), but the subsequent `mark_train_resolved` write
        // that stamps `TEST-M6-SHARED-TRAIN-ID` onto THAT row collides with
        // the first subscriber's row for the same `(train_id, service_date)`.
        let mut second_event = fixture_event(second_tracking_id, "dedup-m6-collision-2");
        second_event.resolved_train_uid = Some("TEST-M6-UID-B".to_string());
        second_event.resolved_train_id = Some("TEST-M6-SHARED-TRAIN-ID".to_string());
        let second_result = upsert_train_event(&pool, &second_event).await;
        assert!(
            second_result.is_err(),
            "a genuine cross-match collision must surface as an error, not be silently \
             swallowed"
        );

        // The heart of this fix: the second subscriber's OWN row must be
        // completely unaffected by its own failed resolution attempt --
        // never left reading 'resolved' with no trains_id to back it up.
        let (second_status, second_trains_id): (String, Option<i64>) = sqlx::query_as(
            "SELECT resolution_status, trains_id FROM train_subscriptions WHERE id = $1",
        )
        .bind(second_tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back second subscriber's row");
        assert_eq!(
            second_status, "pending",
            "a failed resolution must not leave the subscription stuck reading 'resolved'"
        );
        assert!(
            second_trains_id.is_none(),
            "a failed resolution must not leave a dangling trains_id link either"
        );

        // The doomed candidate `trains` row itself must not have survived
        // the rollback -- proving the INSERT inside the same failed
        // transaction was undone, not merely orphaned.
        let (uid_b_count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM trains WHERE train_uid = 'TEST-M6-UID-B'")
                .fetch_one(&pool)
                .await
                .expect("count TEST-M6-UID-B rows");
        assert_eq!(
            uid_b_count, 0,
            "the rolled-back transaction must not leave behind the candidate trains row either"
        );

        // The FIRST subscriber, unrelated to the second's failed attempt,
        // must remain completely unaffected.
        let (first_status, first_trains_id): (String, Option<i64>) = sqlx::query_as(
            "SELECT resolution_status, trains_id FROM train_subscriptions WHERE id = $1",
        )
        .bind(first_tracking_id)
        .fetch_one(&pool)
        .await
        .expect("read back first subscriber's row");
        assert_eq!(first_status, "resolved");
        let trains_id = first_trains_id.expect("first subscriber must have a linked trains_id");

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
    /// This module used to have a second `TRACKED_TRAIN_STATE_SELECT`
    /// caller, `get_by_uid_and_date`, which is why no equivalent test for it
    /// appears here. `GET /Train/by-uid/{uid}/{date}` reads
    /// `trains::get_public_train_state` as of Task 19, so that reader had no
    /// caller left anywhere in the workspace -- the Signal Box Audit's
    /// trains-area Low finding on it, so it was deleted outright rather than
    /// given a test.
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
        // Today: this test reads the subscription back through
        // `list_active_tracked_trains`, which applies a `service_date` floor as
        // of the 2026-09-25 Medium 8 fix.
        let service_date = db_today(&pool).await;

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
        let event = TrainMovementEventMessage {
            tracked_train_id: tracking_id,
            resolved_train_uid: Some("TEST-NR-LIVE-UID".to_string()),
            resolved_train_id: Some("TEST-NR-LIVE-TRAINID".to_string()),
            identity_date: None,
            dedup_key: "test-nr-live-dedup-1".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("12345".to_string()),
            loc_crs: Some("EUS".to_string()),
            planned_timestamp: None,
            gbtt_timestamp: None,
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
        let service_date: NaiveDate = "2026-09-06".parse().unwrap();

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
        let event_for = |tracked_train_id: i64| TrainMovementEventMessage {
            tracked_train_id,
            resolved_train_uid: Some("TEST-SHARED-RESOLVE-UID".to_string()),
            resolved_train_id: Some("TEST-SHARED-RESOLVE-TRAINID".to_string()),
            identity_date: None,
            dedup_key: "test-shared-resolve-dedup".to_string(),
            msg_type: "0003".to_string(),
            event_type: Some("DEPARTURE".to_string()),
            loc_stanox: Some("87212".to_string()),
            loc_crs: Some("WAT".to_string()),
            planned_timestamp: None,
            gbtt_timestamp: None,
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

    /// Fixture for `list_pending_pins_for_backlog_match`'s own tests below:
    /// a `train_subscriptions` row with every column that predicate cares
    /// about under direct caller control, rather than only what
    /// `seed_tracked_train`'s fixed-value insert offers.
    async fn seed_backlog_candidate_pin(
        pool: &PgPool,
        user_id: &str,
        service_date: NaiveDate,
        pin_origin_crs: Option<&str>,
        pin_scheduled_departure: Option<DateTime<Utc>>,
        resolution_status: &str,
        trains_id: Option<i64>,
    ) -> i64 {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO train_subscriptions \
                (user_id, service_date, pin_origin_crs, pin_scheduled_departure, \
                 resolution_status, trains_id) \
             VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
        )
        .bind(user_id)
        .bind(service_date)
        .bind(pin_origin_crs)
        .bind(pin_scheduled_departure)
        .bind(resolution_status)
        .bind(trains_id)
        .fetch_one(pool)
        .await
        .expect("insert fixture backlog-candidate row");
        id
    }

    /// A row that already advanced past `'pending'` (schedule-matched,
    /// resolved live, or given up on) has nothing left for a backlog match
    /// to do -- must never be re-selected.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_pending_pins_for_backlog_match_excludes_a_non_pending_row \
                -- --ignored --test-threads=1`"]
    async fn list_pending_pins_for_backlog_match_excludes_a_non_pending_row() {
        let pool = connect().await;
        let user_id = "TEST-BACKLOG-SWEEP-NON-PENDING";
        seed_user(&pool, user_id).await;

        let service_date = db_today(&pool).await;
        let id = seed_backlog_candidate_pin(
            &pool,
            user_id,
            service_date,
            Some("EUS"),
            Some("2026-09-09T18:15:00Z".parse().unwrap()),
            "schedule_matched",
            None,
        )
        .await;

        let pending = list_pending_pins_for_backlog_match(&pool)
            .await
            .expect("list_pending_pins_for_backlog_match");
        assert!(
            !pending.iter().any(|row| row.id == id),
            "a row already past 'pending' must not be re-selected for a backlog match retry"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// DB2-39: `schedule_matching::run_schedule_match_sweep` (only called
    /// from `main.rs`'s sweep loop) matches a pending pin whose schedule is
    /// published, leaves one that has no match pending, and reports the
    /// count. Synthetic CRS, TIPLOC, line and UID so it touches no real
    /// fixture rows.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                run_schedule_match_sweep_matches_a_pending_pin -- --ignored --test-threads=1`"]
    async fn run_schedule_match_sweep_matches_a_pending_pin() {
        let pool = connect().await;
        let user_id = "TEST-DB2-39-SWEEP";
        let line_id = "test-db2-39-line";
        let uid = "Z39001";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let service_date = db_today(&pool).await + chrono::Duration::days(3);
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TEST-DB2-39-STANOX', 'ZZQ', 'ZZQTEST', 'TEST DB2-39', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO schedule_line_population (line_id, service_date, population) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (line_id, service_date) DO UPDATE SET population = EXCLUDED.population",
        )
        .bind(line_id)
        .bind(service_date)
        .bind(serde_json::json!([{
            "uid": uid,
            "calling_points": [{
                "tiploc": "ZZQTEST",
                "kind": "Origin",
                "booked_arrival": null,
                "booked_departure": "08:15",
                "is_half_minute_arrival": false,
                "is_half_minute_departure": false
            }]
        }]))
        .execute(&pool)
        .await
        .expect("seed schedule_line_population");

        let departure =
            crate::data::eta_blend::london_to_utc(service_date.and_hms_opt(8, 15, 0).unwrap())
                .unwrap();
        let matching = seed_backlog_candidate_pin(
            &pool,
            user_id,
            service_date,
            Some("ZZQ"),
            Some(departure),
            "pending",
            None,
        )
        .await;
        let unmatched = seed_backlog_candidate_pin(
            &pool,
            user_id,
            service_date,
            Some("ZZQ"),
            Some(departure + chrono::Duration::hours(5)),
            "pending",
            None,
        )
        .await;

        let index =
            std::collections::HashMap::from([("ZZQ".to_string(), vec![line_id.to_string()])]);
        let matched = crate::data::schedule_matching::run_schedule_match_sweep(&pool, &index)
            .await
            .expect("sweep");
        assert_eq!(matched, 1);

        let state = |id: i64| {
            let pool = pool.clone();
            async move {
                sqlx::query_as::<_, (String, Option<String>)>(
                    "SELECT ts.resolution_status, tr.train_uid FROM train_subscriptions ts \
                     LEFT JOIN trains tr ON tr.id = ts.trains_id WHERE ts.id = $1",
                )
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap()
            }
        };
        assert_eq!(
            state(matching).await,
            ("schedule_matched".to_string(), Some(uid.to_string()))
        );
        assert_eq!(state(unmatched).await, ("pending".to_string(), None));

        cleanup_user(&pool, user_id).await;
        for (sql, bind) in [
            ("DELETE FROM trains WHERE train_uid = $1", uid),
            (
                "DELETE FROM schedule_line_population WHERE line_id = $1",
                line_id,
            ),
            (
                "DELETE FROM stanox_crs WHERE stanox = $1",
                "TEST-DB2-39-STANOX",
            ),
        ] {
            sqlx::query(sql)
                .bind(bind)
                .execute(&pool)
                .await
                .expect("cleanup");
        }
    }

    /// A tracked bus (or ferry) is timetable-only: it stays out of the
    /// active set trust-consumer matches against, and out of the
    /// reconciliation sweep's enrichment candidates, so neither re-selects
    /// it every tick. A tracked train on the same day stays in both.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                tracked_buses_are_excluded_from_the_live_sweeps -- --ignored --test-threads=1`"]
    async fn tracked_buses_are_excluded_from_the_live_sweeps() {
        let pool = connect().await;
        let user_id = "TEST-BUS-SWEEP";
        seed_user(&pool, user_id).await;
        let today = db_today(&pool).await;
        let bus_uid = "TBUSSWP1";
        let train_uid = "TBUSSWP2";
        sqlx::query("DELETE FROM trains WHERE train_uid IN ($1, $2)")
            .bind(bus_uid)
            .bind(train_uid)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO schedule_services (service_date, uid, mode, train_status, stp) \
             VALUES ($1, $2, 'bus', 'B', 'P'), ($1, $3, 'train', 'P', 'P') \
             ON CONFLICT (service_date, uid) DO UPDATE SET mode = EXCLUDED.mode",
        )
        .bind(today)
        .bind(bus_uid)
        .bind(train_uid)
        .execute(&pool)
        .await
        .unwrap();
        let bus_trains_id = crate::data::trains::find_or_create_train(&pool, bus_uid, today)
            .await
            .unwrap();
        let train_trains_id = crate::data::trains::find_or_create_train(&pool, train_uid, today)
            .await
            .unwrap();
        let bus_sub = create_subscription_for_train(&pool, bus_trains_id, user_id)
            .await
            .unwrap();
        let train_sub = create_subscription_for_train(&pool, train_trains_id, user_id)
            .await
            .unwrap();

        let refs = list_active_tracked_trains(&pool).await.unwrap();
        assert!(
            refs.iter().any(|r| r.id == train_sub),
            "the train stays active"
        );
        assert!(
            !refs.iter().any(|r| r.id == bus_sub),
            "a bus is never activated by TRUST, so it is not in the active set"
        );

        let candidates =
            crate::data::reconciliation::enrichment_candidate_ids_for_tests(&pool).await;
        assert!(candidates.contains(&train_trains_id));
        assert!(
            !candidates.contains(&bus_trains_id),
            "a bus is not re-selected by the enrichment sweep"
        );

        cleanup_user(&pool, user_id).await;
        sqlx::query("DELETE FROM trains WHERE train_uid IN ($1, $2)")
            .bind(bus_uid)
            .bind(train_uid)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_services WHERE uid IN ($1, $2)")
            .bind(bus_uid)
            .bind(train_uid)
            .execute(&pool)
            .await
            .ok();
    }
}
