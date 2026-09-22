//! Journey tracking (Phase 1): a `journeys` row groups one-or-more
//! `journey_legs`, each either bound to a real `train_subscriptions` row
//! (a "matched" leg) or an open time-window search waiting for a manual
//! pick (an "unmatched" leg). See
//! docs/superpowers/specs/2026-09-22-journey-tracking-design.md and
//! docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md.
//!
//! **Phase 1 never creates more than one leg per journey** -- every
//! journey-CREATION function below (`create_journey_with_pin_leg`,
//! `create_journey_with_known_train_leg`, `create_journey_with_window_leg`)
//! always passes `leg_order = 1`. **Phase 2 (design doc §3) adds a leg to an
//! EXISTING journey instead** -- `add_known_train_leg_to_journey` and
//! `add_window_leg_to_journey`, both computing `leg_order = MAX(...) + 1` via
//! the shared `owned_next_leg_order` helper. The schema itself (`leg_order`,
//! `UNIQUE (journey_id, leg_order)`) was already multi-leg-shaped from Phase
//! 1 onward, per the design doc's own reasoning for not folding leg fields
//! onto `train_subscriptions` (§1.1) -- so Phase 2 needed no schema change,
//! only the new writers below.

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use common::TimeWindow;
use serde::Serialize;
use sqlx::PgPool;

/// Mirrors `train_tracking::tracked_train_owner` exactly -- `None` for "no
/// journey with that id"; the route layer maps both that and a mismatch to
/// `404`, never `403` (this codebase's universal ownership convention).
/// Not currently called anywhere in this plan's own routes (every route
/// below folds its ownership check directly into a `JOIN`/`WHERE` instead,
/// per the same convention) -- kept as a small, independently useful,
/// independently testable primitive, matching `tracked_train_owner`'s own
/// role in `train_tracking.rs`.
pub async fn journey_owner(pool: &PgPool, journey_id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT user_id FROM journeys WHERE id = $1")
        .bind(journey_id)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(id,)| id))
}

/// One `journey_legs` row, scoped to a caller's ownership of its parent
/// journey -- folds the ownership check directly into the `WHERE`/`JOIN`
/// (this codebase's established convention, e.g.
/// `train_tracking::delete_tracked_train`'s `WHERE id = $1 AND user_id =
/// $2`) rather than a separate `journey_owner` lookup followed by an
/// unscoped read. Backs both `GET .../candidates` (reads the window/CRS
/// fields to search with, Task 9) and `POST .../train` (confirms ownership
/// before writing, Task 7).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyLegRow {
    pub id: i64,
    pub journey_id: i64,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub service_date: NaiveDate,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
    pub arrive_after: Option<NaiveTime>,
    pub arrive_before: Option<NaiveTime>,
    pub train_subscription_id: Option<i64>,
    pub match_mode: String,
}

pub async fn get_owned_leg(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<JourneyLegRow>> {
    let row = sqlx::query_as::<_, JourneyLegRow>(
        "SELECT jl.id, jl.journey_id, jl.origin_crs, jl.destination_crs, jl.service_date, \
                jl.depart_after, jl.depart_before, jl.arrive_after, jl.arrive_before, \
                jl.train_subscription_id, jl.match_mode \
         FROM journey_legs jl \
         JOIN journeys j ON j.id = jl.journey_id \
         WHERE jl.id = $1 AND jl.journey_id = $2 AND j.user_id = $3",
    )
    .bind(leg_id)
    .bind(journey_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

async fn insert_journey(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
) -> anyhow::Result<i64> {
    let (id,): (i64,) =
        sqlx::query_as("INSERT INTO journeys (user_id, custom_name) VALUES ($1, $2) RETURNING id")
            .bind(user_id)
            .bind(custom_name)
            .fetch_one(pool)
            .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
async fn insert_leg(
    pool: &PgPool,
    journey_id: i64,
    leg_order: i32,
    origin_crs: Option<&str>,
    destination_crs: Option<&str>,
    service_date: NaiveDate,
    train_subscription_id: Option<i64>,
    match_mode: &str,
    depart_after: Option<NaiveTime>,
    depart_before: Option<NaiveTime>,
    arrive_after: Option<NaiveTime>,
    arrive_before: Option<NaiveTime>,
) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_legs \
            (journey_id, leg_order, origin_crs, destination_crs, service_date, \
             train_subscription_id, match_mode, \
             depart_after, depart_before, arrive_after, arrive_before) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         RETURNING id",
    )
    .bind(journey_id)
    .bind(leg_order)
    .bind(origin_crs)
    .bind(destination_crs)
    .bind(service_date)
    .bind(train_subscription_id)
    .bind(match_mode)
    .bind(depart_after)
    .bind(depart_before)
    .bind(arrive_after)
    .bind(arrive_before)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

/// Shared by Phase 2's "add a leg to an EXISTING journey" functions below.
/// `Ok(None)` if `journey_id` doesn't exist or isn't owned by `user_id`
/// (404-never-403, same posture as every other ownership check in this
/// app); `Ok(Some(next_leg_order))` otherwise, `next_leg_order` being
/// `MAX(leg_order) + 1` for this journey (`1` if it somehow has none yet,
/// though that can't happen in practice since every journey is created
/// with a leg).
///
/// **Race, stated rather than hidden**: under READ COMMITTED, two
/// simultaneous calls for the SAME journey can both read the same
/// `MAX(leg_order)` and both attempt to insert the same value -- the
/// schema's own `UNIQUE (journey_id, leg_order)` constraint rejects the
/// second with a constraint-violation error, surfaced as a plain
/// `anyhow::Error` (mapped to the route's existing 500 path). This closes
/// the ordinary repeat-click case (the frontend disables its submit
/// button while a request is in flight, Task 7), not a true concurrent
/// double-submit from two different tabs -- same accepted-limitation
/// posture as `train_tracking::create_subscription_for_train`'s own doc
/// comment.
async fn owned_next_leg_order(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<i32>> {
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM journeys WHERE id = $1 AND user_id = $2")
            .bind(journey_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
    if owned.is_none() {
        return Ok(None);
    }
    let next_leg_order: (i32,) = sqlx::query_as(
        "SELECT COALESCE(MAX(leg_order), 0) + 1 FROM journey_legs WHERE journey_id = $1",
    )
    .bind(journey_id)
    .fetch_one(pool)
    .await?;
    Ok(Some(next_leg_order.0))
}

/// Creates a one-leg journey around a legacy CRS+time GUESS pin
/// (`train_tracking::create_pin`, unchanged) -- the `pin` mode of
/// `POST /Journeys` (`crates/api/src/routes/journeys.rs`), replacing
/// `TrackTrainForm.tsx`'s direct `POST /Train/track` call (design doc
/// §7.2). Two sequential inserts, not one transaction -- see this plan's
/// own Judgment Call 5 for why (`create_pin` is typed to take `&PgPool`,
/// not a transaction handle; widening its signature is out of this
/// phase's scope).
///
/// The new leg's `origin_crs`/`destination_crs` are the pin's OWN
/// `origin_crs`/`destination_crs` (the latter may be `None` -- optional on
/// `TrackPinRequest`), never re-derived from anywhere else.
/// `match_mode = 'manual'` immediately: the leg is bound to a real
/// `train_subscriptions` row from birth (even though that row's own
/// `resolution_status` may still be `'pending'`), exactly mirroring how
/// Task 2's own historical backfill treats every pre-existing row --
/// "already bound to a real train, no window to re-open."
/// `depart_*`/`arrive_*` stay `NULL`: no window was ever searched.
///
/// Returns `(journey_id, leg_id, tracking_id)` -- the route layer
/// (`crates/api/src/routes/journeys.rs::post_journey`) still needs
/// `tracking_id` to run the same best-effort schedule/backlog match
/// attempts `post_track` already makes for a bare pin.
pub async fn create_journey_with_pin_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    pin: &common::TrackPinRequest,
) -> anyhow::Result<(i64, i64, i64)> {
    let journey_id = insert_journey(pool, user_id, custom_name).await?;
    let tracking_id = crate::data::train_tracking::create_pin(pool, pin, user_id).await?;
    let leg_id = insert_leg(
        pool,
        journey_id,
        1,
        Some(pin.origin_crs.as_str()),
        pin.destination_crs.as_deref(),
        pin.service_date,
        Some(tracking_id),
        "manual",
        None,
        None,
        None,
        None,
    )
    .await?;
    Ok((journey_id, leg_id, tracking_id))
}

/// Creates a one-leg journey around an ALREADY-known train identity
/// (`train_tracking::create_subscription_for_train`, unchanged) -- the
/// `knownTrain` mode of `POST /Journeys`, replacing
/// `TrackThisTrainButton.tsx`'s direct
/// `POST /Train/by-uid/{uid}/{date}/track` call.
///
/// `origin_crs`/`destination_crs` are read back off the RESULTING
/// `train_subscriptions` row's own `pin_origin_crs`/`pin_destination_crs`
/// -- populated live from the `trains` row by
/// `create_subscription_for_train` itself, `NULL` if that row has no
/// schedule data yet (the same accepted gap named on
/// `TrackedTrainState::pin_origin_crs`'s own doc comment,
/// `crates/api/src/data/train_tracking.rs`, and the reason Task 1's
/// migration made these two columns nullable -- see this plan's own
/// Judgment Call 1). Never independently supplied by the caller: the
/// caller only ever has a bare `(trainUid, serviceDate)` for this mode.
/// `match_mode = 'manual'`, `depart_*`/`arrive_*` `NULL` -- same reasoning
/// as [`create_journey_with_pin_leg`].
pub async fn create_journey_with_known_train_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    trains_id: i64,
    service_date: NaiveDate,
) -> anyhow::Result<(i64, i64, i64)> {
    let journey_id = insert_journey(pool, user_id, custom_name).await?;
    let tracking_id =
        crate::data::train_tracking::create_subscription_for_train(pool, trains_id, user_id)
            .await?;
    let pins: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT pin_origin_crs, pin_destination_crs FROM train_subscriptions WHERE id = $1",
    )
    .bind(tracking_id)
    .fetch_optional(pool)
    .await?;
    let (origin_crs, destination_crs) = pins.unwrap_or((None, None));
    let leg_id = insert_leg(
        pool,
        journey_id,
        1,
        origin_crs.as_deref(),
        destination_crs.as_deref(),
        service_date,
        Some(tracking_id),
        "manual",
        None,
        None,
        None,
        None,
    )
    .await?;
    Ok((journey_id, leg_id, tracking_id))
}

/// Adds a direct, already-known-train leg to an EXISTING journey (Phase 2,
/// spec §3) -- the `leg_order = max(...) + 1` sibling of
/// [`create_journey_with_known_train_leg`], minus the `insert_journey`
/// call (the journey already exists). Same
/// `train_tracking::create_subscription_for_train` + read-back-the-pin-CRS
/// logic as that function, unchanged.
///
/// Returns `Ok(None)` for "no such journey, or not this caller's" (route
/// maps to 404). Returns `Ok(Some((leg_id, tracking_id)))` on success --
/// `tracking_id` is needed by the route layer to run the same
/// `enrich_shared_train` best-effort enrichment `post_journey`'s own
/// `KnownTrain` arm and `post_leg_train` already run for every new
/// `train_subscriptions` row.
pub async fn add_known_train_leg_to_journey(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    trains_id: i64,
    service_date: NaiveDate,
) -> anyhow::Result<Option<(i64, i64)>> {
    let Some(next_leg_order) = owned_next_leg_order(pool, journey_id, user_id).await? else {
        return Ok(None);
    };
    let tracking_id =
        crate::data::train_tracking::create_subscription_for_train(pool, trains_id, user_id)
            .await?;
    let pins: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT pin_origin_crs, pin_destination_crs FROM train_subscriptions WHERE id = $1",
    )
    .bind(tracking_id)
    .fetch_optional(pool)
    .await?;
    let (origin_crs, destination_crs) = pins.unwrap_or((None, None));
    let leg_id = insert_leg(
        pool,
        journey_id,
        next_leg_order,
        origin_crs.as_deref(),
        destination_crs.as_deref(),
        service_date,
        Some(tracking_id),
        "manual",
        None,
        None,
        None,
        None,
    )
    .await?;
    Ok(Some((leg_id, tracking_id)))
}

/// User-facing validation for a window-search leg's request fields --
/// same posture as `train_tracking::validate_pin`'s own doc comment (this
/// message is rendered verbatim by the frontend). Requires real 3-letter
/// CRS codes for both ends (mirroring `routes::trains::normalize_crs`'s own
/// check, duplicated here rather than reached across the
/// `data`/`routes` module boundary -- that helper is private to
/// `routes/trains.rs`) and AT LEAST ONE of the four window bounds set.
///
/// The "at least one bound" rule closes a real, otherwise-silent
/// ambiguity: `journey_legs.depart_after`/`.../arrive_before` being
/// non-null is literally the signal `GET /Journeys/{id}` uses to decide
/// whether a matched leg offers "Change train" (design doc §2.3/§4). A
/// window search with all four bounds left blank would be a legitimate
/// "any train, any time" search, but would leave every one of those four
/// columns NULL -- indistinguishable, on every later read, from a leg that
/// was never window-searched at all (a `pin`/`knownTrain`-mode leg).
/// Requiring one bound closes that ambiguity outright. See this plan's own
/// Judgment Call 4 for the full reasoning.
pub fn validate_window_leg(
    origin_crs: &str,
    destination_crs: &str,
    depart_window: &TimeWindow,
    arrive_window: &TimeWindow,
) -> Result<(), String> {
    if origin_crs.trim().len() != 3 {
        return Err(
            "Enter a valid origin station — CRS codes are three letters, like WOK or EUS."
                .to_string(),
        );
    }
    if destination_crs.trim().len() != 3 {
        return Err(
            "Enter a valid destination station — CRS codes are three letters, like WOK or \
             EUS."
                .to_string(),
        );
    }
    if depart_window.is_empty() && arrive_window.is_empty() {
        return Err(
            "Enter at least one earliest/latest departure or arrival time to search a window \
             — or pick a specific known departure instead."
                .to_string(),
        );
    }
    Ok(())
}

/// Creates a one-leg journey around an OPEN time-window search -- the
/// `window` mode of `POST /Journeys` (design doc §2.1, §9's revised Phase
/// 1 scope). No `train_subscriptions` row at all yet: `train_subscription_id`
/// is `NULL`, `match_mode = 'unmatched'`, and the caller is expected to
/// browse `GET /Journeys/{journeyId}/legs/{legId}/candidates` and commit
/// one via `POST /Journeys/{journeyId}/legs/{legId}/train` next (Tasks 9,
/// 11).
///
/// Caller must have already run `origin_crs`/`destination_crs`/
/// `depart_window`/`arrive_window` through [`validate_window_leg`] -- this
/// function does no validation of its own, matching this codebase's
/// established "route validates, data layer writes" split (e.g.
/// `train_tracking::rename_tracked_train`'s own doc comment).
#[allow(clippy::too_many_arguments)]
pub async fn create_journey_with_window_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    origin_crs: &str,
    destination_crs: &str,
    service_date: NaiveDate,
    depart_window: TimeWindow,
    arrive_window: TimeWindow,
) -> anyhow::Result<(i64, i64)> {
    let journey_id = insert_journey(pool, user_id, custom_name).await?;
    let leg_id = insert_leg(
        pool,
        journey_id,
        1,
        Some(origin_crs),
        Some(destination_crs),
        service_date,
        None,
        "unmatched",
        depart_window.after,
        depart_window.before,
        arrive_window.after,
        arrive_window.before,
    )
    .await?;
    Ok((journey_id, leg_id))
}

/// Adds an open time-window-search leg to an EXISTING journey (Phase 2,
/// spec §3) -- the `leg_order = max(...) + 1` sibling of
/// [`create_journey_with_window_leg`], minus the `insert_journey` call.
/// Caller must have already run the four `&str`/`TimeWindow` arguments
/// through [`validate_window_leg`] -- this function does no validation of
/// its own, matching this file's established "route validates, data layer
/// writes" split.
///
/// Returns `Ok(None)` for "no such journey, or not this caller's" (route
/// maps to 404). Returns `Ok(Some(leg_id))` on success -- an `'unmatched'`
/// leg with no `train_subscription_id`, exactly like a window-mode
/// journey's own first leg.
#[allow(clippy::too_many_arguments)]
pub async fn add_window_leg_to_journey(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    origin_crs: &str,
    destination_crs: &str,
    service_date: NaiveDate,
    depart_window: TimeWindow,
    arrive_window: TimeWindow,
) -> anyhow::Result<Option<i64>> {
    let Some(next_leg_order) = owned_next_leg_order(pool, journey_id, user_id).await? else {
        return Ok(None);
    };
    let leg_id = insert_leg(
        pool,
        journey_id,
        next_leg_order,
        Some(origin_crs),
        Some(destination_crs),
        service_date,
        None,
        "unmatched",
        depart_window.after,
        depart_window.before,
        arrive_window.after,
        arrive_window.before,
    )
    .await?;
    Ok(Some(leg_id))
}

/// Binds (or re-binds) a leg to a real train working -- the `manual`-mode
/// commit route's data function (design doc §2.3), reused UNCHANGED for
/// both a leg's first pick and any later "Change train" re-pick: this is
/// always a plain `UPDATE`, never a new `journey_legs` row, per the
/// 2026-09-22 addendum's explicit decision that the leg's OLD
/// `train_subscription_id` is simply orphaned from the leg once
/// overwritten -- left exactly as today's `delete_tracked_train`/re-pin
/// flows already leave an unreferenced row, no extra cleanup here. The
/// leg's `depart_*`/`arrive_*` window is deliberately left untouched by
/// this `UPDATE` -- it is what makes a later "Change train" possible at
/// all (design doc §1.1/§2.3).
///
/// Ownership-scoped via the same `journeys j` join `get_owned_leg` uses,
/// folded directly into the `UPDATE` (not re-derived from a prior read
/// alone) -- same paranoia as every other ownership-scoped write in this
/// codebase. Returns `true` if a row was updated, `false` for "no such
/// leg, or not this caller's" (the route maps this to `404`, never `403`).
pub async fn set_leg_train_subscription(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
    train_subscription_id: i64,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE journey_legs SET train_subscription_id = $1, match_mode = 'manual' \
         FROM journeys j \
         WHERE journey_legs.id = $2 AND journey_legs.journey_id = $3 \
           AND j.id = journey_legs.journey_id AND j.user_id = $4",
    )
    .bind(train_subscription_id)
    .bind(leg_id)
    .bind(journey_id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// One row of `GET /Journeys/mine` -- deliberately lighter than the full
/// `GET /Journeys/{id}` detail (`routes::journeys::JourneyDetailResponse`),
/// mirroring `TrackedTrainListItem`'s own "list is lighter than detail"
/// split. This surfaces a single leg's fields directly, not a nested array
/// -- the "current leg" (the earliest leg that isn't already `completed`,
/// or the journey's LAST leg if every leg is `completed`). A genuine
/// multi-leg rollup (design doc §3's "worst status across legs" idiom) is a
/// later phase's job.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyListItem {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub leg_id: i64,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub match_mode: String,
    pub train_subscription_id: Option<i64>,
    pub resolution_status: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
}

/// Most-recently-created journey first, capped at the same
/// `train_tracking::MINE_LIST_LIMIT` `GET /Train/mine` already uses --
/// `pub(crate)` on that constant already permits this cross-module read
/// (see its own doc comment, which anticipates exactly this: "any list
/// this list's own cap should agree with"). Each journey is shown with its
/// "current leg" -- the earliest leg that isn't already `completed`, or the
/// journey's LAST leg (highest `leg_order`) if every leg is `completed`.
pub async fn list_journeys_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<JourneyListItem>> {
    let rows = sqlx::query_as::<_, JourneyListItem>(
        "WITH ranked_legs AS ( \
             SELECT jl.id, jl.journey_id, jl.leg_order, jl.origin_crs, jl.destination_crs, \
                    jl.match_mode, jl.train_subscription_id, \
                    ts.resolution_status, cs.status, cs.delay_minutes, \
                    ROW_NUMBER() OVER ( \
                        PARTITION BY jl.journey_id \
                        ORDER BY (cs.status IS DISTINCT FROM 'completed') DESC, \
                                 CASE WHEN cs.status = 'completed' \
                                      THEN -jl.leg_order ELSE jl.leg_order END ASC \
                    ) AS rn \
             FROM journey_legs jl \
             LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
             LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
         ) \
         SELECT j.id, j.custom_name, j.created_at, \
                rl.id AS leg_id, rl.origin_crs, rl.destination_crs, rl.match_mode, \
                rl.train_subscription_id, rl.resolution_status, rl.status, rl.delay_minutes \
         FROM journeys j \
         JOIN ranked_legs rl ON rl.journey_id = j.id AND rl.rn = 1 \
         WHERE j.user_id = $1 \
         ORDER BY j.created_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(crate::data::train_tracking::MINE_LIST_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneySummaryRow {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

pub async fn get_owned_journey_summary(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<JourneySummaryRow>> {
    let row = sqlx::query_as::<_, JourneySummaryRow>(
        "SELECT id, custom_name, created_at FROM journeys WHERE id = $1 AND user_id = $2",
    )
    .bind(journey_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Every leg of a journey, `leg_order` ascending -- Phase 1 callers only
/// ever see one row (this module's own doc comment), but this is already
/// shaped for a later phase's longer result. Deliberately NOT
/// ownership-scoped on its own (unlike [`get_owned_leg`]) -- every real
/// caller (`routes::journeys::get_journey`, Task 12) already confirmed the
/// journey's ownership via [`get_owned_journey_summary`] one call earlier
/// in the same request, so re-checking here would be a redundant query,
/// not a real safety gain.
pub async fn list_legs_for_journey(pool: &PgPool, journey_id: i64) -> anyhow::Result<Vec<JourneyLegRow>> {
    let rows = sqlx::query_as::<_, JourneyLegRow>(
        "SELECT id, journey_id, origin_crs, destination_crs, service_date, \
                depart_after, depart_before, arrive_after, arrive_before, \
                train_subscription_id, match_mode \
         FROM journey_legs WHERE journey_id = $1 ORDER BY leg_order",
    )
    .bind(journey_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
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
        sqlx::query(
            "DELETE FROM journey_legs WHERE journey_id IN (SELECT id FROM journeys WHERE user_id = $1)",
        )
        .bind(user_id)
        .execute(pool)
        .await
        .expect("cleanup fixture journey_legs");
        sqlx::query("DELETE FROM journeys WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journeys");
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

    fn fixture_pin(origin_crs: &str) -> common::TrackPinRequest {
        common::TrackPinRequest {
            service_date: "2026-09-22".parse().unwrap(),
            origin_crs: origin_crs.to_string(),
            scheduled_departure: "2026-09-22T09:00:00Z".parse().unwrap(),
            destination_crs: Some("EDB".to_string()),
            operator: None,
            // `TrackPinRequest::skipped_stations` has `#[serde(default)]`
            // for deserialization only -- a direct struct literal (as
            // here) still needs the field supplied. "No known skip" is
            // exactly what an empty list means, per that field's own doc
            // comment.
            skipped_stations: Vec::new(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_pin_leg -- --ignored --test-threads=1`"]
    async fn create_journey_with_pin_leg_wraps_a_pin_in_a_one_row_journey() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-PIN").await;

        let (journey_id, leg_id, tracking_id) =
            create_journey_with_pin_leg(&pool, "TEST-JOURNEY-PIN", None, &fixture_pin("KGX"))
                .await
                .expect("create journey with pin leg");

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-PIN")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.origin_crs.as_deref(), Some("KGX"));
        assert_eq!(leg.destination_crs.as_deref(), Some("EDB"));
        assert_eq!(leg.train_subscription_id, Some(tracking_id));
        assert_eq!(leg.match_mode, "manual");
        assert_eq!(leg.depart_after, None);

        cleanup_user(&pool, "TEST-JOURNEY-PIN").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_window_leg -- --ignored --test-threads=1`"]
    async fn create_journey_with_window_leg_creates_an_unmatched_leg() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-WINDOW").await;

        let depart_window = common::TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: Some("10:00:00".parse().unwrap()),
        };
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-WINDOW",
            Some("Commute"),
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            depart_window,
            common::TimeWindow::default(),
        )
        .await
        .expect("create journey with window leg");

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-WINDOW")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, None);
        assert_eq!(leg.match_mode, "unmatched");
        assert_eq!(leg.depart_after, Some("08:00:00".parse().unwrap()));
        assert_eq!(leg.depart_before, Some("10:00:00".parse().unwrap()));

        cleanup_user(&pool, "TEST-JOURNEY-WINDOW").await;
    }

    #[test]
    fn validate_window_leg_rejects_an_all_blank_window() {
        let err = validate_window_leg(
            "WAT",
            "RDG",
            &common::TimeWindow::default(),
            &common::TimeWindow::default(),
        )
        .unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn validate_window_leg_accepts_one_bound_set() {
        let depart_window = common::TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: None,
        };
        assert!(validate_window_leg("WAT", "RDG", &depart_window, &common::TimeWindow::default()).is_ok());
    }

    #[test]
    fn validate_window_leg_messages_carry_no_internal_field_names() {
        let messages = [
            validate_window_leg("W", "RDG", &common::TimeWindow::default(), &common::TimeWindow::default())
                .unwrap_err(),
            validate_window_leg(
                "WAT",
                "RDG",
                &common::TimeWindow::default(),
                &common::TimeWindow::default(),
            )
            .unwrap_err(),
        ];
        for message in messages {
            assert!(!message.is_empty());
            assert!(!message.contains('_'), "user-facing copy leaked an identifier: {message}");
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    async fn set_leg_train_subscription_binds_an_unmatched_leg_and_is_reusable_for_change_train() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create window leg");

        // First pick.
        let first_tracking_id =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("WAT"), "TEST-JOURNEY-COMMIT")
                .await
                .expect("seed first candidate subscription");
        let updated = set_leg_train_subscription(
            &pool,
            journey_id,
            leg_id,
            "TEST-JOURNEY-COMMIT",
            first_tracking_id,
        )
        .await
        .expect("commit first pick");
        assert!(updated);

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-COMMIT")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, Some(first_tracking_id));
        assert_eq!(leg.match_mode, "manual");
        // The window survives the first commit -- this is what makes
        // "Change train" possible at all.
        assert_eq!(leg.depart_after, Some("08:00:00".parse().unwrap()));

        // "Change train" re-pick -- same route, same function, an UPDATE
        // not a new leg.
        let second_tracking_id =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("WAT"), "TEST-JOURNEY-COMMIT")
                .await
                .expect("seed second candidate subscription");
        let updated = set_leg_train_subscription(
            &pool,
            journey_id,
            leg_id,
            "TEST-JOURNEY-COMMIT",
            second_tracking_id,
        )
        .await
        .expect("commit re-pick");
        assert!(updated);

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-COMMIT")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, Some(second_tracking_id));
        assert_eq!(leg.depart_after, Some("08:00:00".parse().unwrap()));

        cleanup_user(&pool, "TEST-JOURNEY-COMMIT").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    async fn set_leg_train_subscription_a_non_owner_cannot_bind_someone_elses_leg() {
        let pool = connect().await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT-OWNER").await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT-OTHER").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT-OWNER",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create window leg");
        let tracking_id =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("WAT"), "TEST-JOURNEY-COMMIT-OTHER")
                .await
                .expect("seed candidate subscription");

        let updated = set_leg_train_subscription(
            &pool,
            journey_id,
            leg_id,
            "TEST-JOURNEY-COMMIT-OTHER",
            tracking_id,
        )
        .await
        .expect("attempt bind as non-owner");
        assert!(!updated);

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-COMMIT-OWNER")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, None);

        cleanup_user(&pool, "TEST-JOURNEY-COMMIT-OWNER").await;
        cleanup_user(&pool, "TEST-JOURNEY-COMMIT-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_journeys_for_user -- --ignored --test-threads=1`"]
    async fn list_journeys_for_user_picks_the_earliest_non_completed_leg() {
        let pool = connect().await;
        let user_id = "TEST-LIST-JOURNEYS-MULTI-LEG";
        seed_user(&pool, user_id).await;

        // Create a journey and add two legs
        let journey_id = insert_journey(&pool, user_id, Some("Multi-leg journey"))
            .await
            .expect("insert journey");

        // Insert a dummy train to use for marking legs as completed
        // Use ON CONFLICT DO NOTHING to handle re-runs and get the existing train
        sqlx::query(
            "INSERT INTO trains (train_uid, service_date) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(format!("TEST-TRAIN-{}-1", user_id))
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .execute(&pool)
        .await
        .expect("insert test train 1");

        let trains_id_1: i64 =
            sqlx::query_scalar("SELECT id FROM trains WHERE train_uid = $1 AND service_date = $2")
                .bind(format!("TEST-TRAIN-{}-1", user_id))
                .bind("2026-09-22".parse::<NaiveDate>().unwrap())
                .fetch_one(&pool)
                .await
                .expect("get trains_id_1");

        // Leg 1: Create with a train subscription that points to trains_id_1
        let tracking_id_1 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO train_subscriptions (user_id, trains_id, service_date) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(user_id)
        .bind(trains_id_1)
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .fetch_one(&pool)
        .await
        .expect("insert train subscription 1");

        let _leg_id_1 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, \
                                        train_subscription_id, match_mode) \
             VALUES ($1, 1, $2, $3, $4, $5, 'manual') \
             RETURNING id",
        )
        .bind(journey_id)
        .bind("KGX")
        .bind("EDB")
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .bind(tracking_id_1)
        .fetch_one(&pool)
        .await
        .expect("insert leg 1");

        // Mark leg 1 as completed
        sqlx::query("DELETE FROM train_current_state WHERE trains_id = $1")
            .bind(trains_id_1)
            .execute(&pool)
            .await
            .expect("clear existing train state");
        sqlx::query(
            "INSERT INTO train_current_state (trains_id, status, delay_minutes) \
             VALUES ($1, 'completed', 0)",
        )
        .bind(trains_id_1)
        .execute(&pool)
        .await
        .expect("mark leg 1 as completed");

        // Leg 2: Create an unmatched leg (no train subscription yet)
        let leg_id_2 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, match_mode) \
             VALUES ($1, 2, $2, $3, $4, 'unmatched') \
             RETURNING id",
        )
        .bind(journey_id)
        .bind("EDB")
        .bind("GLG")
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .fetch_one(&pool)
        .await
        .expect("insert leg 2");

        // Call list_journeys_for_user and verify that leg 2 is returned
        let journeys = list_journeys_for_user(&pool, user_id)
            .await
            .expect("list journeys");

        // Should return at least one journey (the one we just created)
        assert!(!journeys.is_empty(), "Expected at least one journey");

        // Find our journey
        let our_journey = journeys
            .iter()
            .find(|j| j.id == journey_id)
            .expect("Our journey should be in the list");

        // Verify that leg 2's data is shown (earliest non-completed leg)
        assert_eq!(our_journey.leg_id, leg_id_2, "Should show leg 2's ID");
        assert_eq!(
            our_journey.origin_crs.as_deref(),
            Some("EDB"),
            "Should show leg 2's origin"
        );
        assert_eq!(
            our_journey.destination_crs.as_deref(),
            Some("GLG"),
            "Should show leg 2's destination"
        );
        assert_eq!(
            our_journey.match_mode, "unmatched",
            "Should show leg 2's match_mode"
        );

        // Verify that the journey appears exactly once
        let journey_count = journeys.iter().filter(|j| j.id == journey_id).count();
        assert_eq!(
            journey_count, 1,
            "Journey should appear exactly once in the list"
        );

        // Now test: mark leg 2 as also completed, and verify it shows leg 2 (the last leg)
        sqlx::query(
            "INSERT INTO trains (train_uid, service_date) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(format!("TEST-TRAIN-{}-2", user_id))
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .execute(&pool)
        .await
        .expect("insert test train 2");

        let trains_id_2: i64 =
            sqlx::query_scalar("SELECT id FROM trains WHERE train_uid = $1 AND service_date = $2")
                .bind(format!("TEST-TRAIN-{}-2", user_id))
                .bind("2026-09-22".parse::<NaiveDate>().unwrap())
                .fetch_one(&pool)
                .await
                .expect("get trains_id_2");

        let tracking_id_2 = sqlx::query_scalar::<_, i64>(
            "INSERT INTO train_subscriptions (user_id, trains_id, service_date) VALUES ($1, $2, $3) RETURNING id",
        )
        .bind(user_id)
        .bind(trains_id_2)
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .fetch_one(&pool)
        .await
        .expect("insert train subscription 2");

        sqlx::query("UPDATE journey_legs SET train_subscription_id = $1, match_mode = 'manual' WHERE id = $2")
            .bind(tracking_id_2)
            .bind(leg_id_2)
            .execute(&pool)
            .await
            .expect("update leg 2 with subscription");

        sqlx::query("DELETE FROM train_current_state WHERE trains_id = $1")
            .bind(trains_id_2)
            .execute(&pool)
            .await
            .expect("clear existing train state for leg 2");
        sqlx::query(
            "INSERT INTO train_current_state (trains_id, status, delay_minutes) \
             VALUES ($1, 'completed', 0)",
        )
        .bind(trains_id_2)
        .execute(&pool)
        .await
        .expect("mark leg 2 as completed");

        // Now both legs are completed, should still show leg 2 (the last one)
        let journeys = list_journeys_for_user(&pool, user_id)
            .await
            .expect("list journeys after all legs completed");

        let our_journey = journeys
            .iter()
            .find(|j| j.id == journey_id)
            .expect("Our journey should still be in the list");

        assert_eq!(
            our_journey.leg_id, leg_id_2,
            "When all legs completed, should show last leg (leg 2)"
        );

        // Clean up test trains (cleanup_user doesn't remove trains since they're shared)
        sqlx::query("DELETE FROM train_current_state WHERE trains_id IN ($1, $2)")
            .bind(trains_id_1)
            .bind(trains_id_2)
            .execute(&pool)
            .await
            .expect("cleanup train_current_state");
        sqlx::query("DELETE FROM trains WHERE id IN ($1, $2)")
            .bind(trains_id_1)
            .bind(trains_id_2)
            .execute(&pool)
            .await
            .expect("cleanup trains");

        cleanup_user(&pool, user_id).await;
    }

    /// Small helper for the `add_*_leg_to_journey` tests below -- none of
    /// them can use [`JourneyLegRow`] to see `leg_order` (that struct
    /// deliberately omits it, same as every other read in this file), so
    /// they read it back directly the same way
    /// `list_journeys_for_user_picks_the_earliest_non_completed_leg` above
    /// already reads other raw columns not exposed by this file's own
    /// structs.
    async fn leg_order_of(pool: &PgPool, leg_id: i64) -> i32 {
        sqlx::query_scalar("SELECT leg_order FROM journey_legs WHERE id = $1")
            .bind(leg_id)
            .fetch_one(pool)
            .await
            .expect("read leg_order")
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_window_leg_to_journey -- --ignored --test-threads=1`"]
    async fn add_window_leg_to_journey_assigns_incrementing_leg_order() {
        let pool = connect().await;
        let user_id = "TEST-JOURNEY-ADD-WINDOW";
        seed_user(&pool, user_id).await;

        let (journey_id, first_leg_id) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create initial window leg");
        assert_eq!(leg_order_of(&pool, first_leg_id).await, 1);

        let depart_window = common::TimeWindow {
            after: Some("12:00:00".parse().unwrap()),
            before: Some("14:00:00".parse().unwrap()),
        };
        let second_leg_id = add_window_leg_to_journey(
            &pool,
            journey_id,
            user_id,
            "RDG",
            "EDB",
            "2026-09-22".parse().unwrap(),
            depart_window,
            common::TimeWindow::default(),
        )
        .await
        .expect("add second window leg")
        .expect("journey is owned");
        assert_eq!(leg_order_of(&pool, second_leg_id).await, 2);

        let second_leg = get_owned_leg(&pool, journey_id, second_leg_id, user_id)
            .await
            .expect("read second leg")
            .expect("leg exists");
        assert_eq!(second_leg.match_mode, "unmatched");
        assert_eq!(second_leg.train_subscription_id, None);
        assert_eq!(second_leg.depart_after, Some("12:00:00".parse().unwrap()));
        assert_eq!(second_leg.depart_before, Some("14:00:00".parse().unwrap()));

        let third_leg_id = add_window_leg_to_journey(
            &pool,
            journey_id,
            user_id,
            "EDB",
            "GLG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("16:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("add third window leg")
        .expect("journey is owned");
        assert_eq!(leg_order_of(&pool, third_leg_id).await, 3);

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_known_train_leg_to_journey -- --ignored --test-threads=1`"]
    async fn add_known_train_leg_to_journey_sets_train_subscription_and_manual_mode() {
        let pool = connect().await;
        let user_id = "TEST-JOURNEY-ADD-KNOWN";
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        let (journey_id, first_leg_id) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            service_date,
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create initial window leg");
        assert_eq!(leg_order_of(&pool, first_leg_id).await, 1);

        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-TRAIN-ADD-KNOWN", service_date)
                .await
                .expect("seed fixture train");

        let (leg_id, tracking_id) =
            add_known_train_leg_to_journey(&pool, journey_id, user_id, trains_id, service_date)
                .await
                .expect("add known-train leg")
                .expect("journey is owned");
        assert_eq!(leg_order_of(&pool, leg_id).await, 2);

        let leg = get_owned_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, Some(tracking_id));
        assert_eq!(leg.match_mode, "manual");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_known_train_leg_to_journey -- --ignored --test-threads=1`"]
    async fn add_leg_to_journey_a_non_owner_cannot_add_a_leg_to_someone_elses_journey() {
        let pool = connect().await;
        let owner_id = "TEST-JOURNEY-ADD-LEG-OWNER";
        let other_id = "TEST-JOURNEY-ADD-LEG-OTHER";
        seed_user(&pool, owner_id).await;
        seed_user(&pool, other_id).await;

        let (journey_id, _leg_id) = create_journey_with_window_leg(
            &pool,
            owner_id,
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("create window leg for owner");

        let window_result = add_window_leg_to_journey(
            &pool,
            journey_id,
            other_id,
            "RDG",
            "EDB",
            "2026-09-22".parse().unwrap(),
            common::TimeWindow {
                after: Some("12:00:00".parse().unwrap()),
                before: None,
            },
            common::TimeWindow::default(),
        )
        .await
        .expect("attempt add window leg as non-owner");
        assert_eq!(window_result, None);

        // `trains_id` is never dereferenced: ownership is checked first,
        // before this function ever touches `train_subscriptions`.
        let known_train_result = add_known_train_leg_to_journey(
            &pool,
            journey_id,
            other_id,
            i64::MAX,
            "2026-09-22".parse().unwrap(),
        )
        .await
        .expect("attempt add known-train leg as non-owner");
        assert_eq!(known_train_result, None);

        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, other_id).await;
    }
}
