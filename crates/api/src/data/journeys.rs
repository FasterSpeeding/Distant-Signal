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
/// `train_tracking::rename_tracked_train`'s `WHERE id = $2 AND user_id =
/// $3`) rather than a separate `journey_owner` lookup followed by an
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
    /// See [`insert_leg`]'s own doc comment on the `window_searched`
    /// parameter -- distinguishes a leg deliberately created with a fully
    /// open ("any train, any time") search window from one that was never
    /// window-searched at all, both of which otherwise look identical
    /// (`depart_*`/`arrive_*` all `NULL`).
    pub window_searched: bool,
    /// [`LEG_KIND_TRAIN`] or [`LEG_KIND_UNTRACKED`]: the train routes
    /// (candidates, pick a train) refuse an untracked leg.
    pub kind: String,
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
                jl.train_subscription_id, jl.match_mode, jl.window_searched, jl.kind \
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

/// Same shape as [`JourneyLegRow`] plus the two resolved station names --
/// `LEFT JOIN stations` on `origin_crs`/`destination_crs`, the same
/// mechanism `train_tracking::TrackedTrainState::pin_origin_name`/
/// `TrackedTrainListItem::origin_name` already use (see those fields' own
/// doc comments). `None` on either name has the same meaning it does
/// there: no reference row for that code, not "no leg". Backs
/// `GET /Journeys/{id}`'s open-leg card, which previously rendered bare
/// CRS codes ("KGX → EDB") while `GET /Train/mine`'s sibling rows resolved
/// full names -- see
/// docs/superpowers/specs/2026-09-22-ux-review-journey-creation-flow.md
/// §2.5/§2.9.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneyLegWithNamesRow {
    pub id: i64,
    pub journey_id: i64,
    pub origin_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_crs: Option<String>,
    pub destination_name: Option<String>,
    pub service_date: NaiveDate,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
    pub arrive_after: Option<NaiveTime>,
    pub arrive_before: Option<NaiveTime>,
    pub train_subscription_id: Option<i64>,
    pub match_mode: String,
    /// See [`JourneyLegRow::window_searched`]'s own doc comment -- same
    /// field, same meaning.
    pub window_searched: bool,
    /// See [`JourneyLegRow::kind`].
    pub kind: String,
    /// An untracked leg's own fields (spec 2026-10-20 §2.1), `None` on a
    /// train leg.
    pub transit_mode: Option<String>,
    pub origin_place: Option<String>,
    pub destination_place: Option<String>,
    pub line_name: Option<String>,
    pub planned_departure: Option<DateTime<Utc>>,
    pub planned_arrival: Option<DateTime<Utc>>,
    /// The `TfL` stops an untracked leg's places were picked from (spec
    /// 2026-10-20-tfl-stop-lookup-design.md §4), `None` for free text and
    /// on a train leg.
    pub origin_stop_id: Option<String>,
    pub destination_stop_id: Option<String>,
}

/// Generic over `E: PgExecutor` (rather than `&PgPool`) -- see
/// `train_tracking::create_pin`'s own doc comment for the pattern and why:
/// this lets the three `create_journey_with_*` functions below call this
/// with `&mut *tx` from inside one shared transaction (19-pass security/bug
/// review, journeys area, Medium finding 3), while `add_known_train_leg_to_journey`/
/// `add_window_leg_to_journey` (Phase 2, unaffected by that finding) keep
/// passing a bare `&PgPool` unchanged.
async fn insert_journey<'c, E>(
    executor: E,
    user_id: &str,
    custom_name: Option<&str>,
) -> anyhow::Result<i64>
where
    E: sqlx::PgExecutor<'c>,
{
    let (id,): (i64,) =
        sqlx::query_as("INSERT INTO journeys (user_id, custom_name) VALUES ($1, $2) RETURNING id")
            .bind(user_id)
            .bind(custom_name)
            .fetch_one(executor)
            .await?;
    Ok(id)
}

/// `window_searched` -- 19-pass security/bug review, journeys area, Medium
/// finding 1: `TRUE` for a leg created via a window search
/// (`create_journey_with_window_leg`/`add_window_leg_to_journey`), even one
/// with all four `depart_*`/`arrive_*` bounds left `None` (a deliberate
/// "any train, any time" search); `FALSE` for a `pin`/`knownTrain`-mode leg,
/// which never had a window to search at all. This is what lets
/// `JourneyLegCard.tsx`'s `hasWindow`/"Change train" gate tell those two
/// all-NULL-bounds cases apart -- see
/// `20260925090000_journey_legs_window_searched.sql`'s own header comment.
/// Same generic-executor reasoning as [`insert_journey`].
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
async fn insert_leg<'c, E>(
    executor: E,
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
    window_searched: bool,
) -> anyhow::Result<i64>
where
    E: sqlx::PgExecutor<'c>,
{
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_legs \
            (journey_id, leg_order, origin_crs, destination_crs, service_date, \
             train_subscription_id, match_mode, \
             depart_after, depart_before, arrive_after, arrive_before, window_searched) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) \
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
    .bind(window_searched)
    .fetch_one(executor)
    .await?;
    Ok(id)
}

/// Shared by Phase 2's "add a leg to an EXISTING journey" functions below.
/// `AddLegOutcome::NotFound` if `journey_id` doesn't exist or isn't owned
/// by `user_id` (404-never-403, same posture as every other ownership check
/// in this app); `AddLegOutcome::Added(next_leg_order)` otherwise,
/// `next_leg_order` being
/// `MAX(leg_order) + 1` for this journey (`1` if it somehow has none yet,
/// though that can't happen in practice since every journey is created
/// with a leg).
///
/// **Leg cap (2026-09-26 review, L10).** Also refuses once the journey
/// already has [`MAX_LEGS_PER_JOURNEY`] legs -- checked HERE, after the
/// ownership check, rather than as a route-level pre-count, so a caller
/// probing someone else's journey id still only ever sees the 404 path,
/// never a "full" answer that would confirm the journey exists. The cap
/// is exact now that the count runs under the journey row lock.
///
/// **Runs under the journey row's lock (DB2-17).** Takes the journey row
/// `FOR UPDATE` on the caller's transaction, the same lock `delete_leg`
/// takes, so two concurrent adds to one journey serialise: the second
/// reads the first's leg, gets the next `leg_order` and sees the true
/// count. Before, both computed the same `leg_order` on the pool, and the
/// loser hit `UNIQUE (journey_id, leg_order)` as a 500 (and the cap could
/// be overshot). Callers must insert the leg on the same transaction.
async fn owned_next_leg_order(
    tx: &mut sqlx::PgConnection,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<AddLegOutcome<i32>> {
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM journeys WHERE id = $1 AND user_id = $2 FOR UPDATE")
            .bind(journey_id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    if owned.is_none() {
        return Ok(AddLegOutcome::NotFound);
    }
    let (leg_count, next_leg_order): (i64, i32) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(MAX(leg_order), 0) + 1 FROM journey_legs WHERE journey_id = $1",
    )
    .bind(journey_id)
    .fetch_one(&mut *tx)
    .await?;
    if leg_count >= MAX_LEGS_PER_JOURNEY {
        return Ok(AddLegOutcome::AtCap);
    }
    Ok(AddLegOutcome::Added(next_leg_order))
}

/// [`owned_next_leg_order`], or, with `before_leg_id`, the `leg_order` of
/// that leg after moving it and every later leg one place down (spec
/// 2026-10-20 §2.3, "insert a leg before"): the new leg takes the freed
/// place. Same journey row lock, ownership check and cap; `NoSuchLeg` when
/// `before_leg_id` isn't one of this journey's legs.
///
/// The shift is two statements, through negative values, because
/// `UNIQUE (journey_id, leg_order)` is checked row by row: `leg_order + 1`
/// in one statement can collide with the next row before it moves. Gaps
/// left by deleted legs stay as they are; only the order matters. Every
/// reader numbers legs by their position in `leg_order` order (the journey
/// detail, the notifier's "leg 2 of 3", which counts legs at or before
/// it), so an inserted leg renumbers the later ones there too.
async fn owned_leg_order_at(
    tx: &mut sqlx::PgConnection,
    journey_id: i64,
    user_id: &str,
    before_leg_id: Option<i64>,
) -> anyhow::Result<AddLegOutcome<i32>> {
    let next = owned_next_leg_order(&mut *tx, journey_id, user_id).await?;
    let (AddLegOutcome::Added(_), Some(before_leg_id)) = (&next, before_leg_id) else {
        return Ok(next);
    };
    let target: Option<(i32,)> =
        sqlx::query_as("SELECT leg_order FROM journey_legs WHERE id = $1 AND journey_id = $2")
            .bind(before_leg_id)
            .bind(journey_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((target_order,)) = target else {
        return Ok(AddLegOutcome::NoSuchLeg);
    };
    sqlx::query(
        "UPDATE journey_legs SET leg_order = -(leg_order + 1) \
         WHERE journey_id = $1 AND leg_order >= $2",
    )
    .bind(journey_id)
    .bind(target_order)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "UPDATE journey_legs SET leg_order = -leg_order WHERE journey_id = $1 AND leg_order < 0",
    )
    .bind(journey_id)
    .execute(&mut *tx)
    .await?;
    Ok(AddLegOutcome::Added(target_order))
}

/// Locks journey `journey_id` (`FOR UPDATE`) if `user_id` owns it; a
/// missing or someone else's journey locks nothing (the caller's own
/// ownership check then reports it).
async fn lock_owned_journey(
    tx: &mut sqlx::PgConnection,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<()> {
    sqlx::query("SELECT id FROM journeys WHERE id = $1 AND user_id = $2 FOR UPDATE")
        .bind(journey_id)
        .bind(user_id)
        .execute(&mut *tx)
        .await?;
    Ok(())
}

/// How many legs one journey may have. Same value as
/// `journey_templates::MAX_LEGS_PER_TEMPLATE`, deliberately: a template
/// materializes 1:1 into a journey's legs, so the two caps answer the same
/// question ("how long can one trip plausibly be") and must agree. Before
/// this existed, `add_known_train_leg_to_journey`/`add_window_leg_to_journey`
/// would append legs forever -- each `knownTrain` leg also creating a live
/// `train_subscriptions` row the notifier fans out to.
pub const MAX_LEGS_PER_JOURNEY: i64 = 20;

/// How many journeys one user may have created by hand (`POST /Journeys`).
/// Template-materialized journeys (`source_template_id IS NOT NULL`) are
/// NOT counted: those are produced on a schedule by `notifier`'s own
/// materialization and are already bounded by
/// `journey_templates::MAX_JOURNEY_TEMPLATES_PER_USER` x days, and counting
/// them would make a recurring template silently eat a user's manual
/// allowance. Journeys are never auto-pruned, so this is sized as a
/// lifetime-of-account abuse bound (years of real two-trips-a-day manual
/// use), not a working-set limit -- a user who does reach it gets a clear
/// 400 telling them to delete old journeys.
pub const MAX_JOURNEYS_PER_USER: i64 = 1000;

/// Hand-created journeys `user_id` currently owns, for
/// [`MAX_JOURNEYS_PER_USER`] enforcement (see that constant's doc for why
/// template-materialized journeys are excluded).
pub async fn count_manual_journeys_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM journeys WHERE user_id = $1 AND source_template_id IS NULL",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Outcome of adding a leg to an EXISTING journey: the journey isn't there
/// (or isn't the caller's -- route maps to 404), it already has
/// [`MAX_LEGS_PER_JOURNEY`] legs (route maps to 400), or the leg was added.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddLegOutcome<T> {
    NotFound,
    AtCap,
    /// The leg to insert before isn't one of this journey's (route: 404).
    NoSuchLeg,
    Added(T),
}

impl<T> AddLegOutcome<T> {
    /// `Some` only for [`AddLegOutcome::Added`].
    pub fn into_added(self) -> Option<T> {
        match self {
            AddLegOutcome::Added(value) => Some(value),
            AddLegOutcome::NotFound | AddLegOutcome::AtCap | AddLegOutcome::NoSuchLeg => None,
        }
    }
}

/// Creates a one-leg journey around a legacy CRS+time GUESS pin
/// (`train_tracking::create_pin`, unchanged) -- the `pin` mode of
/// `POST /Journeys` (`crates/api/src/routes/journeys.rs`), replacing
/// `TrackTrainForm.tsx`'s direct `POST /Train/track` call (design doc
/// §7.2). All writes run inside one transaction -- see this function's own
/// body doc comment (19-pass security/bug review, journeys area, Medium
/// finding 3) for why: an earlier version ran two-then-three sequential,
/// non-transactional statements here (this plan's own original Judgment
/// Call 5, which cited `create_pin` being typed to take `&PgPool` rather
/// than a transaction handle as the reason not to), which could leave a
/// zero-leg `journeys` row behind on a mid-sequence failure.
/// `train_tracking::create_pin` is now generic over `PgExecutor` precisely
/// so it can run inside this function's own transaction -- see its own doc
/// comment.
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
    // One transaction, not three sequential statements -- 19-pass
    // security/bug review, journeys area, Medium finding 3: a failure
    // partway through (constraint violation, FK error, pool error) used to
    // leave a zero-leg `journeys` row behind (invisible on `/Journeys/mine`,
    // which inner-joins legs, but still reachable by id, with no UI path to
    // see or delete it). `materialize_template`
    // (`journey_templates.rs`) already established this pattern for its own
    // multi-statement write -- same `pool.begin()`/`tx.commit()` shape here.
    let mut tx = pool.begin().await?;
    let journey_id = insert_journey(&mut *tx, user_id, custom_name).await?;
    let tracking_id = crate::data::train_tracking::create_pin(&mut *tx, pin, user_id).await?;
    let leg_id = insert_leg(
        &mut *tx,
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
        false,
    )
    .await?;
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
    Ok((journey_id, leg_id, tracking_id))
}

/// Creates a one-leg journey around an ALREADY-known train identity
/// (`train_tracking::create_subscription_for_train`, unchanged) -- the
/// `knownTrain` mode of `POST /Journeys`, replacing
/// `TrackThisTrainButton.tsx`'s direct
/// `POST /Train/by-uid/{uid}/{date}/track` call.
///
/// `origin_crs`/`destination_crs` default to a read-back off the RESULTING
/// `train_subscriptions` row's own `pin_origin_crs`/`pin_destination_crs`
/// -- populated live from the `trains` row by
/// `create_subscription_for_train` itself, `NULL` if that row has no
/// schedule data yet (the same accepted gap named on
/// `TrackedTrainState::pin_origin_crs`'s own doc comment,
/// `crates/api/src/data/train_tracking.rs`, and the reason Task 1's
/// migration made these two columns nullable -- see this plan's own
/// Judgment Call 1).
///
/// `origin_crs_override`/`destination_crs_override` let the caller supply
/// the leg's OWN boarding/alighting point instead, independently per end --
/// a traveller can board a train partway through its real working, or
/// alight before its final stop, and that leg-specific point can legitimately
/// differ from the train's own full route (see the origin/destination-override
/// plan's own "Why" section's Birmingham→Glasgow/Crewe→Preston example).
/// Either field alone may be `Some` while the other stays `None`: a `Some`
/// override wins outright for that end, a `None` end falls back to the
/// pin-derived value exactly as before. Passing `None, None` reproduces
/// today's exact pin-derived behavior byte-for-byte -- every existing caller
/// of this function does exactly that. Caller must have already run both
/// through [`validate_known_train_overrides`] -- this function does no
/// validation of its own (this file's established "route validates, data
/// layer writes" split), and per that validator's own doc comment (the
/// origin/destination-override plan's Judgment Call 3), a supplied override
/// is never checked against the train's real
/// calling points here.
///
/// `match_mode = 'manual'`, `depart_*`/`arrive_*` `NULL` -- same reasoning
/// as [`create_journey_with_pin_leg`].
pub async fn create_journey_with_known_train_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    trains_id: i64,
    service_date: NaiveDate,
    origin_crs_override: Option<&str>,
    destination_crs_override: Option<&str>,
) -> anyhow::Result<(i64, i64, i64)> {
    // One transaction -- 19-pass security/bug review, journeys area, Medium
    // finding 3: beyond the general zero-leg-journey risk
    // [`create_journey_with_pin_leg`]'s own doc comment describes, THIS
    // function's non-transactional version had an extra failure mode: a
    // leg-insert failure after `create_subscription_for_train` had already
    // committed would orphan that `train_subscriptions` row too (reachable
    // via `/Train/{trackingId}`/`GET /Train/mine`, but never linked from
    // any journey). Wrapping the whole sequence rolls that back as well.
    let mut tx = pool.begin().await?;
    let created = insert_known_train_journey(
        &mut tx,
        user_id,
        custom_name,
        trains_id,
        service_date,
        origin_crs_override,
        destination_crs_override,
    )
    .await?;
    tx.commit().await?;
    Ok(created)
}

/// The body of [`create_journey_with_known_train_leg`] on the caller's
/// transaction: the journey, the user's subscription for the train (made
/// or reused) and the leg binding them. Returns `(journey_id, leg_id,
/// tracking_id)`.
async fn insert_known_train_journey(
    tx: &mut sqlx::PgConnection,
    user_id: &str,
    custom_name: Option<&str>,
    trains_id: i64,
    service_date: NaiveDate,
    origin_crs_override: Option<&str>,
    destination_crs_override: Option<&str>,
) -> anyhow::Result<(i64, i64, i64)> {
    let journey_id = insert_journey(&mut *tx, user_id, custom_name).await?;
    let tracking_id =
        crate::data::train_tracking::create_subscription_for_train(&mut *tx, trains_id, user_id)
            .await?;
    let pins: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT pin_origin_crs, pin_destination_crs FROM train_subscriptions WHERE id = $1",
    )
    .bind(tracking_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (pin_origin_crs, pin_destination_crs) = pins.unwrap_or((None, None));
    let origin_crs = origin_crs_override.map(str::to_string).or(pin_origin_crs);
    let destination_crs = destination_crs_override
        .map(str::to_string)
        .or(pin_destination_crs);
    let leg_id = insert_leg(
        &mut *tx,
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
        false,
    )
    .await?;
    crate::data::journey_revisions::record_plan_change(&mut *tx, journey_id, Utc::now()).await?;
    Ok((journey_id, leg_id, tracking_id))
}

/// What [`track_known_train_on_its_own`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainTrackedOnItsOwn {
    pub journey_id: i64,
    pub tracking_id: i64,
    /// `false` when a journey of the user's already used their
    /// subscription for the train, which is handed back instead.
    pub created: bool,
}

/// "Track this train" for a known train through the old
/// `POST /Train/by-uid/{uid}/{date}/track` (DS-124): a one-leg journey,
/// as `POST /Journeys` makes, unless one of the user's journeys already
/// has a leg on their subscription for this train. Then that subscription
/// and the first such journey come back and nothing is made, so the route
/// stays idempotent and never lists one train as two journeys.
///
/// Holds the `(user_id, trains_id)` lock `create_subscription_for_train`
/// takes for the lookup and the insert, so two concurrent calls make one
/// journey between them.
pub async fn track_known_train_on_its_own(
    pool: &PgPool,
    user_id: &str,
    trains_id: i64,
    service_date: NaiveDate,
) -> anyhow::Result<TrainTrackedOnItsOwn> {
    let mut tx = pool.begin().await?;
    common::pg::lock_user_train_subscription(&mut tx, user_id, trains_id).await?;
    let existing: Option<(i64, i64)> = sqlx::query_as(
        "SELECT ts.id, MIN(jl.journey_id) \
         FROM train_subscriptions ts \
         JOIN journey_legs jl ON jl.train_subscription_id = ts.id \
         JOIN journeys j ON j.id = jl.journey_id AND j.user_id = ts.user_id \
         WHERE ts.user_id = $1 AND ts.trains_id = $2 \
         GROUP BY ts.id ORDER BY ts.id LIMIT 1",
    )
    .bind(user_id)
    .bind(trains_id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((tracking_id, journey_id)) = existing {
        tx.commit().await?;
        return Ok(TrainTrackedOnItsOwn {
            journey_id,
            tracking_id,
            created: false,
        });
    }
    let (journey_id, _, tracking_id) =
        insert_known_train_journey(&mut tx, user_id, None, trains_id, service_date, None, None)
            .await?;
    tx.commit().await?;
    Ok(TrainTrackedOnItsOwn {
        journey_id,
        tracking_id,
        created: true,
    })
}

/// "Stop tracking" a train (`DELETE /Train/{trackingId}`, DS-124): deletes
/// the train's own journeys (the caller's one-leg journeys whose leg is
/// bound to this subscription), each as `DELETE /Journeys/{id}` does
/// (template occurrence tombstones, then [`delete_journey_row_releasing_trains`]),
/// which also releases the train. It stays only while a leg of another
/// journey (a multi-leg one) still uses it, and is kept for that journey.
/// A row no leg uses at all (written by an older binary during a rollout)
/// is deleted outright. Group shares of the deleted journeys, its tickets,
/// remaining `group_trains` shares and notification state go with them
/// (`ON DELETE CASCADE`).
///
/// Returns `false` when no subscription with that id is the caller's (the
/// route's `404`), `true` otherwise, whether or not the row itself went.
/// Locks the journeys first, then the `(user_id, trains_id)` pair, the
/// order [`delete_journey`] takes them in.
pub async fn stop_tracking_train(
    pool: &PgPool,
    tracking_id: i64,
    user_id: &str,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let owned: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT trains_id FROM train_subscriptions WHERE id = $1 AND user_id = $2")
            .bind(tracking_id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    let Some((trains_id,)) = owned else {
        return Ok(false);
    };
    // Every journey of the caller's with a leg on the train, locked as an
    // add-leg path locks its journey; then, of those, the ones that still
    // have that one leg alone. A leg added concurrently either committed
    // before the lock (the journey is multi-leg now and kept) or waits
    // and then finds no journey.
    sqlx::query(
        "SELECT j.id FROM journeys j \
         WHERE j.user_id = $2 AND j.id IN ( \
             SELECT jl.journey_id FROM journey_legs jl WHERE jl.train_subscription_id = $1) \
         ORDER BY j.id FOR UPDATE",
    )
    .bind(tracking_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    let own_journeys: Vec<i64> = sqlx::query_scalar(concat!(
        "SELECT jl.journey_id FROM journey_legs jl \
         JOIN journeys j ON j.id = jl.journey_id AND j.user_id = $2 \
         WHERE jl.train_subscription_id = $1 AND ",
        crate::data::train_tracking::only_leg_of_its_journey_sql!(),
        " ORDER BY jl.journey_id"
    ))
    .bind(tracking_id)
    .bind(user_id)
    .fetch_all(&mut *tx)
    .await?;
    for journey_id in own_journeys {
        record_template_occurrence_skips(&mut tx, journey_id).await?;
        delete_journey_row_releasing_trains(&mut tx, journey_id, user_id).await?;
    }
    if let Some(trains_id) = trains_id {
        common::pg::lock_user_train_subscription(&mut tx, user_id, trains_id).await?;
    }
    sqlx::query(
        "DELETE FROM train_subscriptions ts WHERE ts.id = $1 AND ts.user_id = $2 \
           AND NOT EXISTS (SELECT 1 FROM journey_legs jl WHERE jl.train_subscription_id = ts.id)",
    )
    .bind(tracking_id)
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(true)
}

/// Adds a direct, already-known-train leg to an EXISTING journey (Phase 2,
/// spec §3) -- the `leg_order = max(...) + 1` sibling of
/// [`create_journey_with_known_train_leg`], minus the `insert_journey`
/// call (the journey already exists). Same
/// `train_tracking::create_subscription_for_train` + read-back-the-pin-CRS
/// logic as that function, unchanged -- including the identical
/// `origin_crs_override`/`destination_crs_override` behavior: both
/// optional, either overridable independently of the other, `None, None`
/// reproducing today's exact pin-derived behavior, and (the
/// origin/destination-override plan's Judgment Call 3) no validation here
/// against the train's real calling
/// points -- caller must have already run both through
/// [`validate_known_train_overrides`]. See that function's sibling doc
/// comment on [`create_journey_with_known_train_leg`] for the full
/// reasoning; this function's override-selection logic is byte-identical.
///
/// Returns `AddLegOutcome::NotFound` for "no such journey, or not this
/// caller's" (route maps to 404), `AddLegOutcome::AtCap` once the journey
/// already has [`MAX_LEGS_PER_JOURNEY`] legs (route maps to 400), and
/// `AddLegOutcome::Added((leg_id, tracking_id))` on success --
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
    origin_crs_override: Option<&str>,
    destination_crs_override: Option<&str>,
) -> anyhow::Result<AddLegOutcome<(i64, i64)>> {
    add_known_train_leg_to_journey_at(
        pool,
        journey_id,
        user_id,
        trains_id,
        service_date,
        origin_crs_override,
        destination_crs_override,
        None,
    )
    .await
}

/// [`add_known_train_leg_to_journey`], placed before `before_leg_id` when
/// given (see [`owned_leg_order_at`]), else at the end.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
pub async fn add_known_train_leg_to_journey_at(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    trains_id: i64,
    service_date: NaiveDate,
    origin_crs_override: Option<&str>,
    destination_crs_override: Option<&str>,
    before_leg_id: Option<i64>,
) -> anyhow::Result<AddLegOutcome<(i64, i64)>> {
    let mut tx = pool.begin().await?;
    let next_leg_order =
        match owned_leg_order_at(&mut tx, journey_id, user_id, before_leg_id).await? {
            AddLegOutcome::Added(next) => next,
            AddLegOutcome::NotFound => return Ok(AddLegOutcome::NotFound),
            AddLegOutcome::AtCap => return Ok(AddLegOutcome::AtCap),
            AddLegOutcome::NoSuchLeg => return Ok(AddLegOutcome::NoSuchLeg),
        };
    // 2026-09 Signal Box Audit Low finding: `create_subscription_for_train`
    // used to run as its own bare statement against `pool`, committed
    // immediately, BEFORE the `insert_leg` call below that actually
    // attaches it to this leg. `journey_legs`'s own `UNIQUE (journey_id,
    // leg_order)` constraint -- the very race `owned_next_leg_order`'s own
    // doc comment already accepts as a known, stated limitation -- meant a
    // losing `insert_leg` surfaced as a plain `anyhow::Error` (the route's
    // blanket `internal_error` 500) with the subscription already
    // permanently committed and nothing in `journey_legs` pointing at it:
    // an orphaned `train_subscriptions` row, exactly the kind
    // `set_leg_train_subscription`'s own doc comment describes as NOT
    // inert (still `notifications_enabled`, still fanned out to by
    // `notifier::candidates_for_trains_id`). Running both statements
    // inside one transaction closes that: a losing `insert_leg` now rolls
    // the subscription INSERT back too, instead of leaving it stranded.
    // `create_subscription_for_train` is safe to run again on a retry
    // either way -- it's idempotent per `(user_id, trains_id)` (its own
    // doc comment). The transaction now starts before
    // `owned_next_leg_order`, which locks the journey row (DB2-17).
    let tracking_id =
        crate::data::train_tracking::create_subscription_for_train(&mut *tx, trains_id, user_id)
            .await?;
    let pins: Option<(Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT pin_origin_crs, pin_destination_crs FROM train_subscriptions WHERE id = $1",
    )
    .bind(tracking_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (pin_origin_crs, pin_destination_crs) = pins.unwrap_or((None, None));
    let origin_crs = origin_crs_override.map(str::to_string).or(pin_origin_crs);
    let destination_crs = destination_crs_override
        .map(str::to_string)
        .or(pin_destination_crs);
    let leg_id = insert_leg(
        &mut *tx,
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
        false,
    )
    .await?;
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
    Ok(AddLegOutcome::Added((leg_id, tracking_id)))
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
    // 2026-09 Signal Box Audit Low finding: this used to be
    // `origin_crs.trim().len() != 3` -- a BYTE-length check, not a
    // character check. `str::len` counts UTF-8 bytes, so a single
    // multi-byte character (e.g. "é" is 2 bytes, plenty of 3-byte
    // characters exist too) could pass this check while being nowhere
    // near a real 3-letter station code. Fixed to match
    // `routes::trains::normalize_crs`'s own check -- the established
    // convention for validating a CRS code everywhere else in this
    // codebase -- three ASCII alphabetic characters, case-insensitive
    // (normalization to uppercase happens downstream, same as there).
    if !is_three_letter_crs(origin_crs) {
        return Err(
            "Enter a valid origin station. Station codes are three letters, like WOK or EUS."
                .to_string(),
        );
    }
    if !is_three_letter_crs(destination_crs) {
        return Err(
            "Enter a valid destination station. Station codes are three letters, like WOK or \
             EUS."
                .to_string(),
        );
    }
    if depart_window.is_empty() && arrive_window.is_empty() {
        return Err(
            "Enter at least one earliest/latest departure or arrival time to search a window, \
             or pick a specific known departure instead."
                .to_string(),
        );
    }
    // 2026-09 Signal Box Audit Low finding: neither window's `after`/
    // `before` bounds were ever checked against each other. A backwards
    // window (`after` later than `before`) isn't merely odd input -- every
    // schedule query built from these bounds
    // (`queries::search_schedule_calling_point_departures`'s
    // `scheduled_from`/`to_time` pattern) is an inclusive range filter, so
    // `after > before` can never match any real calling point. The leg
    // would persist looking perfectly valid and just silently never
    // resolve a train, with no error surfaced to the user who typed it.
    // Reject it outright instead.
    if let (Some(after), Some(before)) = (depart_window.after, depart_window.before)
        && after > before
    {
        return Err(
            "The earliest departure time must not be later than the latest departure time."
                .to_string(),
        );
    }
    if let (Some(after), Some(before)) = (arrive_window.after, arrive_window.before)
        && after > before
    {
        return Err(
            "The earliest arrival time must not be later than the latest arrival time.".to_string(),
        );
    }
    Ok(())
}

/// A real 3-letter station code, ASCII-alphabetic only, case-insensitive --
/// same check as `routes::trains::normalize_crs` (that helper is private
/// to `routes/trains.rs`, so this is a small duplicate rather than a
/// cross-module reach). Deliberately a byte-length-agnostic character
/// check -- see [`validate_window_leg`]'s doc comment for why
/// `str::len() != 3` alone is not safe here.
fn is_three_letter_crs(crs: &str) -> bool {
    let trimmed = crs.trim();
    trimmed.chars().count() == 3 && trimmed.chars().all(|c| c.is_ascii_alphabetic())
}

/// User-facing validation for a `knownTrain`-mode leg's OPTIONAL
/// origin/destination overrides -- same check-and-message posture as
/// [`validate_window_leg`]/`journey_templates::validate_template_leg`
/// already take for a manually-typed CRS field (a real 3-letter station code
/// once trimmed), just applied to two `Option<&str>` fields instead of two
/// required `&str` fields: either, both, or neither may be `Some`, and each
/// supplied one is checked independently.
///
/// Deliberately does NOT check a supplied override against the underlying
/// train's real calling points -- not an oversight, a scoped decision (the
/// origin/destination-override plan's own Judgment Call 3). Both routes
/// resolve `trains_id` via `trains::find_or_create_train`, a bare
/// `(train_uid, service_date)`
/// identity upsert that does not populate `calling_points` (or even
/// `origin_crs`/`destination_crs`) for a brand-new `trains` row -- that data
/// arrives later, best-effort, via `routes::train::enrich_shared_train`,
/// which both routes call only AFTER they've already built and returned
/// their response. There is no real schedule data to validate an override
/// against synchronously, for a train identity seen for the first time, at
/// the point either route would need to run this check -- fetching it
/// synchronously just for this validation would add a new schedule lookup
/// neither route makes today, for every caller, not just the ones supplying
/// an override. This function therefore only confirms a *supplied* override
/// is well-formed, and defers real calling-point validation to a future
/// plan if it turns out to matter in practice.
pub fn validate_known_train_overrides(
    origin_crs: Option<&str>,
    destination_crs: Option<&str>,
) -> Result<(), String> {
    // 2026-09 Signal Box Audit Low finding: same byte-length-vs-character
    // bug as [`validate_window_leg`]'s own fix -- `.trim().len() != 3` is a
    // UTF-8 byte count, not a 3-letter check. Reuses the same
    // [`is_three_letter_crs`] helper for consistency.
    if origin_crs.is_some_and(|origin_crs| !is_three_letter_crs(origin_crs)) {
        return Err(
            "Enter a valid origin station. Station codes are three letters, like WOK or EUS."
                .to_string(),
        );
    }
    if destination_crs.is_some_and(|destination_crs| !is_three_letter_crs(destination_crs)) {
        return Err(
            "Enter a valid destination station. Station codes are three letters, like WOK or EUS."
                .to_string(),
        );
    }
    Ok(())
}

/// A real TRUST/CIF `train_uid` is always exactly this many characters --
/// see `docs/superpowers/specs/2026-08-28-train-tracking-design.md`'s
/// identifiers section (`"train_uid" (e.g. "C21373") -- the CIF/schedule
/// UID"`) and `trust_schema::schema::Activation`'s own fixture literals
/// (`"C21373"`, `"C88888"`, `"W34058"`, ...) -- every confirmed real-world
/// example is a 6-character alphanumeric code.
pub const TRAIN_UID_LENGTH: usize = 6;

/// User-facing shape guard for a caller-supplied `train_uid`, Signal Box
/// Audit 2026-09-25 Low finding #1: `KnownTrain`-mode leg creation
/// (`POST /Journeys`, `POST /Journeys/{id}/legs`) and manual leg-matching
/// (`POST /Journeys/{id}/legs/{legId}/train`) all take a bare
/// caller-typed `train_uid` string and feed it straight into
/// `trains::find_or_create_train`, which unconditionally upserts a row
/// into the GLOBAL, shared `trains` table for `(train_uid, service_date)`
/// -- there is no per-user scoping or moderation on that table at all.
/// Without this check, any authenticated user could mint unlimited garbage
/// `(train_uid, service_date)` rows by supplying arbitrary strings (a
/// multi-KB blob, SQL-looking text, anything), polluting a table every
/// other user's journeys/trains pages read from.
///
/// Deliberately just a SHAPE check (length + alphanumeric), not a
/// real-identity lookup: this route has no synchronous way to confirm a
/// `train_uid` corresponds to a real CIF schedule (the same reasoning
/// [`validate_known_train_overrides`]'s doc comment gives for not
/// validating overrides against real calling points), and legitimate
/// candidates only ever reach this field pre-filled from this app's own
/// `GET .../candidates` search results anyway -- this exists to reject
/// obviously-malformed input, not to second-guess a well-formed one.
///
/// `find_or_create_train` itself is intentionally NOT where this check
/// lives: it's also called with long, deliberately-not-6-character
/// synthetic identifiers by internal consumers (`trust-backlog-consumer`'s
/// bare-UID fallback path, this crate's own fixtures) that are not
/// caller-facing and must stay unvalidated. This is purely a boundary
/// check for the caller-facing routes in `routes::journeys`.
pub fn validate_train_uid(train_uid: &str) -> Result<(), String> {
    let trimmed = train_uid.trim();
    if trimmed.len() != TRAIN_UID_LENGTH || !trimmed.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(format!(
            "That doesn't look like a real train identifier. Train IDs are \
             {TRAIN_UID_LENGTH}-character codes like \"C21373\"."
        ));
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
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
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
    // One transaction -- 19-pass security/bug review, journeys area, Medium
    // finding 3; see [`create_journey_with_pin_leg`]'s own doc comment for
    // the full reasoning (a mid-sequence failure used to leave a zero-leg
    // `journeys` row behind).
    let mut tx = pool.begin().await?;
    let journey_id = insert_journey(&mut *tx, user_id, custom_name).await?;
    let leg_id = insert_leg(
        &mut *tx,
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
        // Always `true`: a `window`-mode leg is, by definition, a window
        // search -- see [`insert_leg`]'s own doc comment on this parameter.
        true,
    )
    .await?;
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
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
/// Returns `AddLegOutcome::NotFound` for "no such journey, or not this
/// caller's" (route maps to 404), `AddLegOutcome::AtCap` at
/// [`MAX_LEGS_PER_JOURNEY`] (400), and `AddLegOutcome::Added(leg_id)` on
/// success -- an `'unmatched'`
/// leg with no `train_subscription_id`, exactly like a window-mode
/// journey's own first leg.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
pub async fn add_window_leg_to_journey(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    origin_crs: &str,
    destination_crs: &str,
    service_date: NaiveDate,
    depart_window: TimeWindow,
    arrive_window: TimeWindow,
) -> anyhow::Result<AddLegOutcome<i64>> {
    add_window_leg_to_journey_at(
        pool,
        journey_id,
        user_id,
        origin_crs,
        destination_crs,
        service_date,
        depart_window,
        arrive_window,
        None,
    )
    .await
}

/// [`add_window_leg_to_journey`], placed before `before_leg_id` when given.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them"
)]
pub async fn add_window_leg_to_journey_at(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    origin_crs: &str,
    destination_crs: &str,
    service_date: NaiveDate,
    depart_window: TimeWindow,
    arrive_window: TimeWindow,
    before_leg_id: Option<i64>,
) -> anyhow::Result<AddLegOutcome<i64>> {
    let mut tx = pool.begin().await?;
    let next_leg_order =
        match owned_leg_order_at(&mut tx, journey_id, user_id, before_leg_id).await? {
            AddLegOutcome::Added(next) => next,
            AddLegOutcome::NotFound => return Ok(AddLegOutcome::NotFound),
            AddLegOutcome::AtCap => return Ok(AddLegOutcome::AtCap),
            AddLegOutcome::NoSuchLeg => return Ok(AddLegOutcome::NoSuchLeg),
        };
    let leg_id = insert_leg(
        &mut *tx,
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
        true,
    )
    .await?;
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
    Ok(AddLegOutcome::Added(leg_id))
}

/// `journey_legs.kind` for a leg on a train we track (every leg before
/// 2026-10-20).
pub const LEG_KIND_TRAIN: &str = "train";
/// `journey_legs.kind` for a leg we have no data for: a Tube ride, a bus, a
/// walk (spec 2026-10-20-untracked-legs-and-tracked-train-states-design.md
/// §2).
pub const LEG_KIND_UNTRACKED: &str = "untracked";

/// Every `transit_mode` an untracked leg may have, in the order the form
/// offers them. Checked here, not by the database (whose CHECK only bounds
/// the shape), so adding a mode needs no migration.
pub const TRANSIT_MODES: [&str; 11] = [
    "tube", "dlr", "bus", "tram", "metro", "rail", "walk", "ferry", "taxi", "cycle", "other",
];
/// The longest place name an untracked leg keeps (the migration's CHECK).
pub const UNTRACKED_PLACE_MAX_CHARS: usize = 100;
/// The longest line name an untracked leg keeps (the migration's CHECK).
pub const UNTRACKED_LINE_MAX_CHARS: usize = 60;
/// The longest an untracked leg may last (the migration's CHECK).
pub const UNTRACKED_MAX_DURATION: chrono::Duration = chrono::Duration::hours(24);

/// A validated untracked leg, ready to insert. Built only by
/// [`validate_untracked_leg`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UntrackedLeg {
    pub transit_mode: String,
    pub origin_place: Option<String>,
    pub destination_place: Option<String>,
    pub line_name: Option<String>,
    pub planned_departure: DateTime<Utc>,
    pub planned_arrival: DateTime<Utc>,
    /// The `TfL` stops its places were picked from (spec
    /// 2026-10-20-tfl-stop-lookup-design.md §4): `None` from
    /// [`validate_untracked_leg`], set by [`attach_untracked_stops`].
    pub origin_stop_id: Option<String>,
    pub destination_stop_id: Option<String>,
}

impl UntrackedLeg {
    /// The leg's `service_date`: the London calendar date of its planned
    /// departure, so every reader keyed on a leg's date (the template
    /// occurrence guard, the journey list, the export) treats it like any
    /// other leg of that day.
    pub fn service_date(&self) -> NaiveDate {
        london_date(self.planned_departure)
    }
}

fn london_date(instant: DateTime<Utc>) -> NaiveDate {
    instant
        .with_timezone(&chrono_tz::Europe::London)
        .date_naive()
}

/// An untracked leg as the wire sends it, before [`validate_untracked_leg`].
#[derive(Debug, Clone, Copy)]
pub struct UntrackedLegInput<'a> {
    pub transit_mode: &'a str,
    pub origin_name: Option<&'a str>,
    pub destination_name: Option<&'a str>,
    pub line_name: Option<&'a str>,
    pub planned_departure: Option<DateTime<Utc>>,
    pub planned_arrival: Option<DateTime<Utc>>,
}

/// Validates an untracked leg (spec §2.4): a known mode, both planned
/// times, arrival not before departure and at most
/// [`UNTRACKED_MAX_DURATION`] later, a departure within the tracking
/// horizon (at most `PIN_MAX_DAYS_AHEAD` days ahead, at most
/// `TRACK_BY_UID_MAX_DAYS_BEHIND` days back, both in London dates), and
/// text within its length. Text is trimmed; blank text is absent.
pub fn validate_untracked_leg(
    input: UntrackedLegInput<'_>,
    london_today: NaiveDate,
) -> Result<UntrackedLeg, String> {
    let transit_mode = input.transit_mode.trim().to_ascii_lowercase();
    if !TRANSIT_MODES.contains(&transit_mode.as_str()) {
        return Err(format!(
            "transitMode must be one of: {}.",
            TRANSIT_MODES.join(", ")
        ));
    }
    let (Some(planned_departure), Some(planned_arrival)) =
        (input.planned_departure, input.planned_arrival)
    else {
        return Err("An untracked leg needs a planned departure and arrival.".to_string());
    };
    if planned_arrival < planned_departure {
        return Err("The planned arrival must be at or after the planned departure.".to_string());
    }
    if planned_arrival - planned_departure > UNTRACKED_MAX_DURATION {
        return Err("An untracked leg can last at most 24 hours.".to_string());
    }
    let departure_date = london_date(planned_departure);
    if crate::data::train_tracking::beyond_tracking_horizon(departure_date, london_today) {
        return Err(crate::data::train_tracking::too_far_ahead_message());
    }
    if departure_date
        < london_today
            - chrono::Duration::days(crate::data::train_tracking::TRACK_BY_UID_MAX_DAYS_BEHIND)
    {
        return Err(format!(
            "That leg is too far in the past. An untracked leg can start at most {} days ago.",
            crate::data::train_tracking::TRACK_BY_UID_MAX_DAYS_BEHIND
        ));
    }
    let text = |value: Option<&str>, max: usize, what: &str| -> Result<Option<String>, String> {
        let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) else {
            return Ok(None);
        };
        if value.chars().count() > max {
            return Err(format!("{what} can be at most {max} characters."));
        }
        if value.chars().any(char::is_control) {
            return Err(format!(
                "{what} can't contain line breaks or control characters."
            ));
        }
        Ok(Some(value.to_string()))
    };
    Ok(UntrackedLeg {
        transit_mode,
        origin_place: text(input.origin_name, UNTRACKED_PLACE_MAX_CHARS, "A place name")?,
        destination_place: text(
            input.destination_name,
            UNTRACKED_PLACE_MAX_CHARS,
            "A place name",
        )?,
        line_name: text(input.line_name, UNTRACKED_LINE_MAX_CHARS, "The line")?,
        planned_departure,
        planned_arrival,
        origin_stop_id: None,
        destination_stop_id: None,
    })
}

/// Gives a validated untracked leg the `TfL` stops its places were picked
/// from (spec 2026-10-20-tfl-stop-lookup-design.md §4): `originStopId` and
/// `destinationStopId`, each optional. Each must be a stop in `tfl_stops`;
/// the inner `Err` is the 400's sentence otherwise. A place left blank
/// takes its stop's name. Blank ids are absent.
pub async fn attach_untracked_stops(
    pool: &PgPool,
    mut leg: UntrackedLeg,
    origin_stop_id: Option<&str>,
    destination_stop_id: Option<&str>,
) -> anyhow::Result<Result<UntrackedLeg, String>> {
    let clean = |id: Option<&str>| {
        id.map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    };
    let origin = clean(origin_stop_id);
    let destination = clean(destination_stop_id);
    for (field, id) in [
        ("originStopId", &origin),
        ("destinationStopId", &destination),
    ] {
        if id
            .as_deref()
            .is_some_and(|id| !common::tfl_stops::is_stop_id(id))
        {
            return Ok(Err(format!(
                "{field} must be a TfL stop id from GET /tfl/stops."
            )));
        }
    }
    let ids: Vec<String> = origin.iter().chain(destination.iter()).cloned().collect();
    if ids.is_empty() {
        return Ok(Ok(leg));
    }
    if !TFL_STOP_MODES.contains(&leg.transit_mode.as_str()) {
        return Ok(Err(format!(
            "Only a {} leg can have a stop id.",
            TFL_STOP_MODES.join(", ")
        )));
    }
    let known = crate::data::tfl_stops::stops_by_id(pool, &ids).await?;
    for (field, id, place) in [
        ("originStopId", &origin, &mut leg.origin_place),
        (
            "destinationStopId",
            &destination,
            &mut leg.destination_place,
        ),
    ] {
        let Some(id) = id else { continue };
        let Some(stop) = known.get(id) else {
            return Ok(Err(format!("{field} {id} is not a stop we know.")));
        };
        if place.is_none() {
            *place = Some(stop.name.chars().take(UNTRACKED_PLACE_MAX_CHARS).collect());
        }
    }
    leg.origin_stop_id = origin;
    leg.destination_stop_id = destination;
    Ok(Ok(leg))
}

/// The untracked-leg modes whose places may carry a `TfL` stop id.
/// `metro` is the planner's generic `METRO` transfer (DLR, tram or Tube).
pub const TFL_STOP_MODES: [&str; 4] = ["tube", "dlr", "tram", "metro"];

/// An untracked leg's state, from the clock alone (spec §2.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UntrackedStatus {
    /// Before its planned departure.
    Planned,
    /// Between its planned departure and arrival.
    InProgress,
    /// At or after its planned arrival.
    Done,
}

/// [`UntrackedStatus`] at `now`. A zero-length leg goes straight from
/// planned to done.
pub fn untracked_status(
    planned_departure: DateTime<Utc>,
    planned_arrival: DateTime<Utc>,
    now: DateTime<Utc>,
) -> UntrackedStatus {
    if now >= planned_arrival {
        UntrackedStatus::Done
    } else if now >= planned_departure {
        UntrackedStatus::InProgress
    } else {
        UntrackedStatus::Planned
    }
}

#[cfg(test)]
mod untracked_tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
    }

    fn input<'a>(departure: &str, arrival: &str) -> UntrackedLegInput<'a> {
        UntrackedLegInput {
            transit_mode: "tube",
            origin_name: Some(" Euston "),
            destination_name: Some(""),
            line_name: Some("Victoria line"),
            planned_departure: Some(at(departure)),
            planned_arrival: Some(at(arrival)),
        }
    }

    #[test]
    fn a_valid_leg_is_trimmed_and_dated_in_london() {
        // 23:30 UTC on the 20th is 00:30 BST on the 21st: the leg's
        // service date is London's.
        let leg = validate_untracked_leg(
            input("2026-10-20T23:30:00Z", "2026-10-20T23:50:00Z"),
            today(),
        )
        .unwrap();
        assert_eq!(leg.origin_place.as_deref(), Some("Euston"));
        assert_eq!(leg.destination_place, None, "blank is absent");
        assert_eq!(
            leg.service_date(),
            NaiveDate::from_ymd_opt(2026, 10, 21).unwrap()
        );
    }

    #[test]
    fn bad_legs_are_refused_with_sentences() {
        let refuse =
            |input: UntrackedLegInput<'_>| validate_untracked_leg(input, today()).unwrap_err();
        assert_eq!(
            refuse(input("2026-10-21T09:00:00Z", "2026-10-21T08:59:59Z")),
            "The planned arrival must be at or after the planned departure."
        );
        assert_eq!(
            refuse(input("2026-10-21T09:00:00Z", "2026-10-22T09:00:01Z")),
            "An untracked leg can last at most 24 hours."
        );
        assert!(
            validate_untracked_leg(
                input("2026-10-21T09:00:00Z", "2026-10-22T09:00:00Z"),
                today()
            )
            .is_ok(),
            "exactly 24 hours is allowed"
        );
        assert!(refuse(input("2026-11-18T09:00:00Z", "2026-11-18T09:10:00Z")).contains("days"));
        assert!(
            refuse(input("2026-10-12T09:00:00Z", "2026-10-12T09:10:00Z")).contains("7 days ago")
        );
        assert_eq!(
            refuse(UntrackedLegInput {
                planned_arrival: None,
                ..input("2026-10-21T09:00:00Z", "2026-10-21T09:10:00Z")
            }),
            "An untracked leg needs a planned departure and arrival."
        );
        assert!(
            refuse(UntrackedLegInput {
                transit_mode: "zeppelin",
                ..input("2026-10-21T09:00:00Z", "2026-10-21T09:10:00Z")
            })
            .starts_with("transitMode must be one of: tube, dlr, bus")
        );
        let long = "x".repeat(UNTRACKED_LINE_MAX_CHARS + 1);
        assert_eq!(
            refuse(UntrackedLegInput {
                line_name: Some(&long),
                ..input("2026-10-21T09:00:00Z", "2026-10-21T09:10:00Z")
            }),
            "The line can be at most 60 characters."
        );
        assert!(
            refuse(UntrackedLegInput {
                origin_name: Some("Euston\nVictoria"),
                ..input("2026-10-21T09:00:00Z", "2026-10-21T09:10:00Z")
            })
            .contains("line breaks")
        );
    }

    #[test]
    fn status_follows_the_clock_alone() {
        let departure = at("2026-10-21T09:00:00Z");
        let arrival = at("2026-10-21T09:20:00Z");
        let status = |now: &str| untracked_status(departure, arrival, at(now));
        assert_eq!(status("2026-10-21T08:59:59Z"), UntrackedStatus::Planned);
        assert_eq!(status("2026-10-21T09:00:00Z"), UntrackedStatus::InProgress);
        assert_eq!(status("2026-10-21T09:19:59Z"), UntrackedStatus::InProgress);
        assert_eq!(status("2026-10-21T09:20:00Z"), UntrackedStatus::Done);
        assert_eq!(
            untracked_status(departure, departure, departure),
            UntrackedStatus::Done,
            "a zero-length leg goes straight from planned to done"
        );
        assert_eq!(
            serde_json::to_value(UntrackedStatus::InProgress).unwrap(),
            "inProgress"
        );
    }
}

/// Inserts one untracked leg (spec §2.1): `kind = 'untracked'`, no
/// subscription, `match_mode = 'manual'` (nothing to search for or match,
/// so the notifier's template sweep never reads it), no window, no CRS
/// codes, `service_date` from the planned departure. Same generic-executor
/// reasoning as [`insert_journey`].
async fn insert_untracked_leg<'c, E>(
    executor: E,
    journey_id: i64,
    leg_order: i32,
    leg: &UntrackedLeg,
) -> anyhow::Result<i64>
where
    E: sqlx::PgExecutor<'c>,
{
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_legs \
            (journey_id, leg_order, service_date, match_mode, window_searched, kind, \
             transit_mode, origin_place, destination_place, line_name, \
             planned_departure, planned_arrival, origin_stop_id, destination_stop_id) \
         VALUES ($1, $2, $3, 'manual', FALSE, 'untracked', $4, $5, $6, $7, $8, $9, $10, $11) \
         RETURNING id",
    )
    .bind(journey_id)
    .bind(leg_order)
    .bind(leg.service_date())
    .bind(&leg.transit_mode)
    .bind(&leg.origin_place)
    .bind(&leg.destination_place)
    .bind(&leg.line_name)
    .bind(leg.planned_departure)
    .bind(leg.planned_arrival)
    .bind(&leg.origin_stop_id)
    .bind(&leg.destination_stop_id)
    .fetch_one(executor)
    .await?;
    Ok(id)
}

/// Creates a one-leg journey whose leg is untracked (`POST /Journeys`,
/// `mode: "untracked"`), in one transaction like every other
/// `create_journey_with_*`. Returns `(journey_id, leg_id)`.
pub async fn create_journey_with_untracked_leg(
    pool: &PgPool,
    user_id: &str,
    custom_name: Option<&str>,
    leg: &UntrackedLeg,
) -> anyhow::Result<(i64, i64)> {
    let mut tx = pool.begin().await?;
    let journey_id = insert_journey(&mut *tx, user_id, custom_name).await?;
    let leg_id = insert_untracked_leg(&mut *tx, journey_id, 1, leg).await?;
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
    Ok((journey_id, leg_id))
}

/// Adds an untracked leg to an EXISTING journey the caller owns (`POST
/// /Journeys/{id}/legs`, `mode: "untracked"`), under the journey row lock
/// and leg cap of [`owned_next_leg_order`], like every other add.
pub async fn add_untracked_leg_to_journey(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    leg: &UntrackedLeg,
) -> anyhow::Result<AddLegOutcome<i64>> {
    add_untracked_leg_to_journey_at(pool, journey_id, user_id, leg, None).await
}

/// [`add_untracked_leg_to_journey`], placed before `before_leg_id` when
/// given.
pub async fn add_untracked_leg_to_journey_at(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    leg: &UntrackedLeg,
    before_leg_id: Option<i64>,
) -> anyhow::Result<AddLegOutcome<i64>> {
    let mut tx = pool.begin().await?;
    let next_leg_order =
        match owned_leg_order_at(&mut tx, journey_id, user_id, before_leg_id).await? {
            AddLegOutcome::Added(next) => next,
            AddLegOutcome::NotFound => return Ok(AddLegOutcome::NotFound),
            AddLegOutcome::AtCap => return Ok(AddLegOutcome::AtCap),
            AddLegOutcome::NoSuchLeg => return Ok(AddLegOutcome::NoSuchLeg),
        };
    let leg_id = insert_untracked_leg(&mut *tx, journey_id, next_leg_order, leg).await?;
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
    Ok(AddLegOutcome::Added(leg_id))
}

/// What [`update_untracked_leg`] found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateUntrackedOutcome {
    /// No such leg, or not the caller's (route: 404).
    NotFound,
    /// A train leg: only an untracked leg is edited this way (route: 400).
    NotUntracked,
    Updated,
}

/// Replaces an untracked leg's mode, places, line and times in place (spec
/// 2026-10-20 §2.4, `PUT /Journeys/{id}/legs/{legId}`), keeping its id and
/// place in the journey. `service_date` follows the new departure.
/// Ownership is folded into the same statement; a train leg is refused, not
/// converted.
pub async fn update_untracked_leg(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
    leg: &UntrackedLeg,
) -> anyhow::Result<UpdateUntrackedOutcome> {
    let mut tx = pool.begin().await?;
    // The journey row first, as every other leg write takes it, so the
    // revision written below never waits on it while holding the leg.
    lock_owned_journey(&mut tx, journey_id, user_id).await?;
    let kind: Option<(String,)> = sqlx::query_as(
        "SELECT jl.kind FROM journey_legs jl JOIN journeys j ON j.id = jl.journey_id \
         WHERE jl.id = $1 AND jl.journey_id = $2 AND j.user_id = $3 FOR UPDATE OF jl",
    )
    .bind(leg_id)
    .bind(journey_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    match kind {
        None => return Ok(UpdateUntrackedOutcome::NotFound),
        Some((kind,)) if kind != LEG_KIND_UNTRACKED => {
            return Ok(UpdateUntrackedOutcome::NotUntracked);
        }
        Some(_) => {}
    }
    sqlx::query(
        "UPDATE journey_legs SET transit_mode = $2, origin_place = $3, destination_place = $4, \
             line_name = $5, planned_departure = $6, planned_arrival = $7, service_date = $8, \
             origin_stop_id = $9, destination_stop_id = $10 \
         WHERE id = $1",
    )
    .bind(leg_id)
    .bind(&leg.transit_mode)
    .bind(&leg.origin_place)
    .bind(&leg.destination_place)
    .bind(&leg.line_name)
    .bind(leg.planned_departure)
    .bind(leg.planned_arrival)
    .bind(leg.service_date())
    .bind(&leg.origin_stop_id)
    .bind(&leg.destination_stop_id)
    .execute(&mut *tx)
    .await?;
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
    Ok(UpdateUntrackedOutcome::Updated)
}

/// Binds (or re-binds) a leg to a real train working -- the `manual`-mode
/// commit route's data function (design doc §2.3), reused for both a leg's
/// first pick and any later "Change train" re-pick: this is always a plain
/// `UPDATE`, never a new `journey_legs` row. The leg's `depart_*`/
/// `arrive_*` window is deliberately left untouched by this `UPDATE` -- it
/// is what makes a later "Change train" possible at all (design doc
/// §1.1/§2.3).
///
/// 2026-09-22 addendum's original decision that the leg's OLD
/// `train_subscription_id` is simply orphaned once overwritten -- left
/// exactly as `delete_tracked_train`/re-pin flows already leave an
/// unreferenced row -- turned out to be a real bug, not merely a cosmetic
/// leftover: 2026-09 security/bug review finding (Medium). An orphaned
/// `train_subscriptions` row is NOT inert -- `notifier`'s
/// `candidates_for_trains_id` fans out to every row in `train_subscriptions`
/// for a given `trains_id` regardless of whether any `journey_legs` row
/// still references it, so a "Change train" re-pick left the OLD
/// subscription fully live: still `notifications_enabled`, still pushing
/// notifications for a train the user is no longer tracking via this leg.
/// That review's fix deactivated the old subscription (`notifications_enabled
/// = FALSE`) once no leg referenced it. That stopped the pushes but left the
/// row in `GET /Train/mine` as a silent train "tracked on its own", and it
/// equally silenced a row the user really did track on its own: one row can
/// serve both (`create_subscription_for_train` is idempotent per
/// `(user_id, trains_id)`). DS-93 records the difference
/// (`train_subscriptions.tracked_standalone`, set by the standalone track
/// routes) and now deletes a leg-only old subscription outright, after
/// moving its tickets and group shares to the new one, and leaves one the
/// user tracks on its own, or that another leg still uses, fully live. See
/// [`release_leg_subscription`].
///
/// Runs as one transaction (a read of the leg's CURRENT
/// `train_subscription_id`, ownership-scoped and row-locked via `FOR
/// UPDATE`, then the `UPDATE`, then the cleanup of the old subscription) --
/// mirrors `delete_leg`'s own read-then-write-in-one-transaction shape
/// immediately below, needed here for the same reason: the OLD value has
/// to be read before it's overwritten, and the ownership check must cover
/// both. Returns `true` if the leg was updated, `false` for "no such leg,
/// or not this caller's" (the route maps this to `404`, never `403`).
pub async fn set_leg_train_subscription(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
    train_subscription_id: i64,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let updated = set_leg_train_subscription_locked(
        &mut tx,
        journey_id,
        leg_id,
        user_id,
        train_subscription_id,
    )
    .await?;
    if updated {
        crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(updated)
}

/// The row-locked ownership-check-then-`UPDATE`-then-old-subscription-
/// cleanup core of [`set_leg_train_subscription`], factored out onto
/// an already-open transaction/connection -- 2026-09 security/bug review
/// (Repeater Signal) Medium finding M12: [`create_subscription_and_bind_leg`]
/// needs to run this SAME logic inside ITS OWN transaction, one that also
/// contains the `train_subscriptions` INSERT that must precede it, so
/// neither commits without the other. Caller commits/rolls back; this
/// function only ever leaves the transaction open, whichever `bool` it
/// returns.
async fn set_leg_train_subscription_locked(
    tx: &mut sqlx::PgConnection,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
    train_subscription_id: i64,
) -> anyhow::Result<bool> {
    // 2026-09 Signal Box Audit Low finding: this used to check ONLY that
    // `leg_id`/`journey_id` belong to `user_id`, never that
    // `train_subscription_id` does too. That made this an IDOR: nothing
    // here stopped user A's leg from being pointed at user B's
    // `train_subscriptions` row -- which would then surface user B's live
    // train position/notifications inside user A's journey UI, and hand
    // user A a `tracking_id` for a subscription they don't own. It was
    // "safe" only by accident -- every real caller (`post_leg_train`, via
    // `create_subscription_and_bind_leg`) always mints/reuses the
    // subscription for THIS SAME `user_id` immediately before calling in,
    // so `train_subscription_id` is always already this same user's. But
    // that's a property of today's real callers, not of this function --
    // any future caller that accepts a subscription id from elsewhere
    // (e.g. a "reuse an existing pin" shortcut) would silently
    // reintroduce the IDOR. The `EXISTS` clause below makes the ownership
    // check a property of the function itself: if `train_subscription_id`
    // isn't this `user_id`'s, the whole lookup comes back empty and this
    // returns `Ok(false)`, which the route maps to `404` -- same "no such
    // leg, or not this caller's" posture as every other failure mode
    // here, never leaking whether the subscription id itself exists.
    // The journey row is locked first, as `delete_leg` locks it, so the
    // revision this change writes never waits on it while holding the leg.
    lock_owned_journey(&mut *tx, journey_id, user_id).await?;
    let owned: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT jl.train_subscription_id FROM journey_legs jl \
         JOIN journeys j ON j.id = jl.journey_id \
         WHERE jl.id = $1 AND jl.journey_id = $2 AND j.user_id = $3 \
           AND EXISTS ( \
               SELECT 1 FROM train_subscriptions ts \
               WHERE ts.id = $4 AND ts.user_id = $3 \
           ) \
         FOR UPDATE OF jl",
    )
    .bind(leg_id)
    .bind(journey_id)
    .bind(user_id)
    .bind(train_subscription_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((old_train_subscription_id,)) = owned else {
        return Ok(false);
    };

    let result = sqlx::query(
        "UPDATE journey_legs SET train_subscription_id = $1, match_mode = 'manual' \
         WHERE journey_legs.id = $2 AND journey_legs.journey_id = $3",
    )
    .bind(train_subscription_id)
    .bind(leg_id)
    .bind(journey_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 0 {
        return Ok(false);
    }

    if let Some(old_id) = old_train_subscription_id
        && old_id != train_subscription_id
    {
        release_leg_subscription(&mut *tx, user_id, old_id, Some(train_subscription_id)).await?;
    }

    Ok(true)
}

/// Cleanup of a subscription a leg just stopped using, `old_id` (DS-93):
/// "Change train" (`move_to` is the leg's new subscription), "Remove leg"
/// ([`delete_leg`]) and deleting a whole journey
/// ([`delete_journey_row_releasing_trains`]), both with `move_to` `None`. Deletes it unless
/// another leg (any journey) still references it.
///
/// Since DS-124 a train tracked on its own is a one-leg journey of its
/// own, so a leg referencing the row is the only reason to keep it. DS-93's
/// `tracked_standalone` (a row tracked on its own besides its legs) is no
/// longer read or written: the tracked-trains-as-journeys migration
/// (`20261022090000`) made every such row a journey of its own. The flag
/// stays as it was for a code rollback until phase 3 drops the column.
///
/// With `move_to`, what the user attached moves there first: its tickets,
/// and its remaining `group_trains` shares (a share the new subscription
/// already has is kept once; a share of the train's journey stays on the
/// journey, which keeps the leg). Without one, they go with the row (`ON
/// DELETE CASCADE`). Its notification state always goes with it, which is
/// right: that was the old train's.
///
/// Takes the `(user_id, trains_id)` lock `create_subscription_for_train`
/// takes (`common::pg::lock_user_train_subscription`, also taken by the
/// notifier's copy) before deciding, so a concurrent "Track this train" or
/// leg bind that is about to hand back this same row either finishes first
/// (and is seen here as referenced) or waits and then creates a fresh row.
/// A pin with no train yet (`trains_id` NULL) is never handed back by that
/// lookup, so it needs no lock.
async fn release_leg_subscription(
    conn: &mut sqlx::PgConnection,
    user_id: &str,
    old_id: i64,
    move_to: Option<i64>,
) -> anyhow::Result<()> {
    let trains_id: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT trains_id FROM train_subscriptions WHERE id = $1 AND user_id = $2")
            .bind(old_id)
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?;
    let Some((trains_id,)) = trains_id else {
        return Ok(());
    };
    if let Some(trains_id) = trains_id {
        common::pg::lock_user_train_subscription(&mut *conn, user_id, trains_id).await?;
    }
    let keep: Option<(bool,)> = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM journey_legs jl WHERE jl.train_subscription_id = ts.id) \
         FROM train_subscriptions ts WHERE ts.id = $1 AND ts.user_id = $2 FOR UPDATE",
    )
    .bind(old_id)
    .bind(user_id)
    .fetch_optional(&mut *conn)
    .await?;
    if keep.is_none_or(|(keep,)| keep) {
        return Ok(());
    }
    if let Some(new_id) = move_to {
        sqlx::query(
            "UPDATE tracked_train_tickets SET tracked_train_id = $2 \
             WHERE tracked_train_id = $1 AND user_id = $3",
        )
        .bind(old_id)
        .bind(new_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
        sqlx::query(
            "INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at) \
             SELECT group_id, $2, added_by, added_at FROM group_trains \
             WHERE train_subscription_id = $1 \
             ON CONFLICT (group_id, train_subscription_id) DO NOTHING",
        )
        .bind(old_id)
        .bind(new_id)
        .execute(&mut *conn)
        .await?;
    }
    sqlx::query("DELETE FROM train_subscriptions WHERE id = $1 AND user_id = $2")
        .bind(old_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Creates (or reuses -- `create_subscription_for_train` is idempotent per
/// `(user_id, trains_id)`, its own doc comment) a `train_subscriptions` row
/// for `trains_id` and binds it to `leg_id`, ATOMICALLY -- 2026-09
/// security/bug review (Repeater Signal) Medium finding M12.
///
/// The bug this closes: `post_leg_train` (this function's one caller,
/// `routes/journeys.rs`) used to call `create_subscription_for_train`
/// against `&app.database` directly -- its own bare statement, committed
/// immediately -- and only THEN call `set_leg_train_subscription` in a
/// separate, later transaction to link that subscription to the leg. A
/// failure or lost race in that second call left the freshly minted
/// subscription permanently orphaned: reachable via `/Train/{trackingId}`/
/// `GET /Train/mine`, but linked from no `journey_legs` row at all --
/// `set_leg_train_subscription`'s own doc comment already names exactly
/// this as reachable ("Lost a race against a concurrent deletion of the
/// underlying leg between the read above and this write"). This is the
/// identical "subscription committed before its link" shape
/// [`add_known_train_leg_to_journey`]'s own doc comment already closed for
/// the leg-CREATION path (a brand new leg); this closes the same race for
/// the leg-REBIND path (an existing leg's first pick, or a later "Change
/// train") the same way: one transaction, subscription insert and leg
/// link, or neither -- via [`set_leg_train_subscription_locked`], the
/// same row-locked ownership-check-then-`UPDATE` core
/// `set_leg_train_subscription` itself runs, just inside THIS function's
/// own transaction instead of its own.
///
/// Returns `Ok(None)` for "no such leg, or not this caller's" -- same
/// posture as `set_leg_train_subscription` (the route maps it to `404`,
/// never `403`); the freshly-created-or-reused subscription is rolled
/// back right along with it, so a losing race never leaves one behind.
/// Returns `Ok(Some(tracking_id))` on success.
pub async fn create_subscription_and_bind_leg(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
    trains_id: i64,
) -> anyhow::Result<Option<i64>> {
    create_subscription_and_bind_leg_with_search(pool, journey_id, leg_id, user_id, trains_id, None)
        .await
}

/// A train leg's window search, as "Edit search" on a journey leg saves it
/// along with the train picked from it: the leg's own origin and
/// destination, service date and time window. Validated by
/// [`validate_window_leg`] before it gets here.
#[derive(Debug, Clone)]
pub struct LegSearch {
    pub origin_crs: String,
    pub destination_crs: String,
    pub service_date: NaiveDate,
    pub depart_window: TimeWindow,
    pub arrive_window: TimeWindow,
}

/// [`create_subscription_and_bind_leg`], and when `search` is given, the
/// leg's search replaced by it in the same transaction (an edited search
/// and the train picked from it land together or not at all, so the leg
/// never names one route and tracks a train for another). The leg becomes
/// window-searched, so "Change train" re-runs the new search. An untracked
/// leg is never touched (`Ok(None)`).
pub async fn create_subscription_and_bind_leg_with_search(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
    trains_id: i64,
    search: Option<&LegSearch>,
) -> anyhow::Result<Option<i64>> {
    let mut tx = pool.begin().await?;
    let tracking_id =
        crate::data::train_tracking::create_subscription_for_train(&mut *tx, trains_id, user_id)
            .await?;
    let updated =
        set_leg_train_subscription_locked(&mut tx, journey_id, leg_id, user_id, tracking_id)
            .await?;
    if !updated {
        tx.rollback().await?;
        return Ok(None);
    }
    if let Some(search) = search {
        // The row is already locked and owner-checked above.
        let result = sqlx::query(
            "UPDATE journey_legs SET origin_crs = $1, destination_crs = $2, service_date = $3, \
                 depart_after = $4, depart_before = $5, arrive_after = $6, arrive_before = $7, \
                 window_searched = TRUE \
             WHERE id = $8 AND journey_id = $9 AND kind <> $10",
        )
        .bind(search.origin_crs.trim().to_ascii_uppercase())
        .bind(search.destination_crs.trim().to_ascii_uppercase())
        .bind(search.service_date)
        .bind(search.depart_window.after)
        .bind(search.depart_window.before)
        .bind(search.arrive_window.after)
        .bind(search.arrive_window.before)
        .bind(leg_id)
        .bind(journey_id)
        .bind(LEG_KIND_UNTRACKED)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(None);
        }
    }
    crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    tx.commit().await?;
    Ok(Some(tracking_id))
}

/// Removes one leg from a journey the caller owns -- the 2026-09-22 UX
/// review's I14/2.4 recovery path for a `pin`/`knownTrain`-mode leg with a
/// wrong pick: that leg has no persisted window to re-search
/// (`journey_legs.depart_after` etc. all `NULL`), so `JourneyLegCard.tsx`'s
/// `hasWindow` gate never offers "Change train" for it, and until this
/// function existed there was no way to remove it either -- see this
/// module's own doc comment on Phase 1 never having a delete route at all.
///
/// Deliberately still not a general "delete a journey" route (that Non-goal
/// stands): this only ever removes ONE leg, but when it's the journey's
/// LAST remaining one, the whole now-empty `journeys` row is deleted too
/// (`journey_legs.journey_id ... ON DELETE CASCADE`,
/// `20260922090000_journeys.sql`, takes the leg with it in the same
/// statement) rather than leaving a zero-leg journey nothing else in this
/// codebase expects to see (`get_journey_summary`, `list_journeys_for_user`,
/// every frontend page). The leg's `train_subscriptions` row, if it was
/// matched, used to be left behind, where it showed in `GET /Train/mine`
/// as a train tracked on its own (DS-93). It is now released like a
/// "Change train" re-pick's old one ([`release_leg_subscription`], no
/// `move_to`): deleted, with its tickets and group shares, unless another
/// leg still uses it or the user tracks it on its own.
///
/// Ownership-checked read first, then a `COUNT` and one of two deletes, all
/// inside one transaction, with the journey row locked (`FOR UPDATE`) up
/// front -- closes a real race the transaction alone did NOT: two
/// concurrent `delete_leg` calls for DIFFERENT legs of the SAME two-leg
/// journey could otherwise both read `remaining = 2` (READ COMMITTED
/// doesn't block a plain `SELECT COUNT(*)` on another transaction's
/// in-flight `DELETE`), both conclude "delete just this leg, the journey
/// still has one left", and both commit -- leaving a zero-leg journey
/// behind, invisible to `list_journeys_for_user`'s inner join but still
/// reachable by id, exactly the state this function's own doc comment
/// above says nothing else in this codebase expects to see. Locking
/// `journeys` first (the same `FOR UPDATE`/`FOR KEY SHARE`-inside-one-
/// transaction pattern `groups::grant_custom_line` already uses for its own
/// race) forces the second concurrent call to wait for the first to commit
/// before it can even run its own `COUNT`, so it then correctly reads
/// `remaining = 1` and takes the "delete the whole journey" branch instead.
/// `FOR UPDATE`, not the lighter `FOR KEY SHARE`: this function's own
/// writes below delete rows that reference `journey_id`, not merely read
/// it.
///
/// Returns `Ok(None)` for "no such leg, or not this caller's" (route maps
/// to `404`, matching `get_owned_leg`'s own convention). Returns
/// `Ok(Some(journey_also_deleted))` on success.
pub async fn delete_leg(
    pool: &PgPool,
    journey_id: i64,
    leg_id: i64,
    user_id: &str,
) -> anyhow::Result<Option<bool>> {
    let mut tx = pool.begin().await?;

    // Locks the journey row (if it exists) for the rest of this
    // transaction -- see this function's own doc comment above. A
    // nonexistent `journey_id` simply locks nothing here and falls through
    // to the ownership check below, which reports "not found" exactly as
    // before.
    sqlx::query_as::<_, (i64,)>("SELECT id FROM journeys WHERE id = $1 FOR UPDATE")
        .bind(journey_id)
        .fetch_optional(&mut *tx)
        .await?;

    // `FOR UPDATE OF jl`: a train the notifier's auto-match is binding to
    // this leg right now is waited for and read as committed, so it is
    // released below rather than left behind (DS-93; see
    // [`delete_journey_row_releasing_trains`]).
    let owned: Option<(Option<i64>,)> = sqlx::query_as(
        "SELECT jl.train_subscription_id FROM journey_legs jl \
         JOIN journeys j ON j.id = jl.journey_id \
         WHERE jl.id = $1 AND jl.journey_id = $2 AND j.user_id = $3 \
         FOR UPDATE OF jl",
    )
    .bind(leg_id)
    .bind(journey_id)
    .bind(user_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((train_subscription_id,)) = owned else {
        return Ok(None);
    };

    let (remaining,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM journey_legs WHERE journey_id = $1")
            .bind(journey_id)
            .fetch_one(&mut *tx)
            .await?;
    let journey_also_deleted = remaining <= 1;

    if journey_also_deleted {
        // Removing the last leg removes the whole occurrence, so this is a
        // discard of a template occurrence exactly as much as
        // `delete_journey` is -- tombstone it before the delete, or the
        // recurrence sweep re-mints it within the hour. Deliberately NOT
        // done when other legs survive: the occurrence itself still exists
        // then, and the sweep's own "already has a leg on this date" guard
        // still sees it.
        record_template_occurrence_skips(&mut tx, journey_id).await?;
        // Cascades into `journey_legs` for us (`ON DELETE CASCADE`) --
        // deleting the leg row explicitly first would be redundant, not
        // wrong, but this is the one statement, not two.
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("DELETE FROM journey_legs WHERE id = $1")
            .bind(leg_id)
            .execute(&mut *tx)
            .await?;
    }
    if let Some(train_subscription_id) = train_subscription_id {
        release_leg_subscription(&mut tx, user_id, train_subscription_id, None).await?;
    }
    if !journey_also_deleted {
        crate::data::journey_revisions::record_plan_change(&mut tx, journey_id, Utc::now()).await?;
    }

    tx.commit().await?;
    Ok(Some(journey_also_deleted))
}

/// Records "the occurrence this template produced for this date was
/// explicitly discarded by its owner" for every `(source_template_id,
/// service_date)` pair `journey_id` covers -- a tombstone in
/// `journey_template_skipped_dates` (migration `20260925214500`).
///
/// Real bug this closes: `crates/notifier`'s recurrence sweep decides
/// whether today's occurrence still needs minting purely by asking "does a
/// journey from this template already have a leg on this date"
/// (`notifier::queries::materialize_due_template_occurrence`). Deleting
/// today's auto-minted occurrence -- the ordinary "I'm not travelling
/// today" action -- made that question false again, so the next sweep tick,
/// AT MOST AN HOUR LATER, re-minted the journey, re-auto-committed its legs
/// and resumed pushing notifications for something the user had explicitly
/// thrown away. Nothing anywhere recorded that the deletion had happened:
/// the `journeys` row was the only trace of an occurrence, and deleting it
/// removes that trace by definition. This is that missing record.
///
/// Called from inside both delete paths' own transactions, BEFORE the
/// delete (it reads the legs it is about to lose). Idempotent (`ON
/// CONFLICT DO NOTHING`) and a no-op for a journey with no
/// `source_template_id` -- an ordinary hand-built journey has no template
/// occurrence to suppress.
///
/// Scoped per `(template_id, service_date)`, not per template: skipping
/// today must never suppress tomorrow's occurrence of the same commute.
pub(crate) async fn record_template_occurrence_skips(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    journey_id: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO journey_template_skipped_dates (template_id, service_date) \
         SELECT j.source_template_id, jl.service_date \
         FROM journeys j JOIN journey_legs jl ON jl.journey_id = j.id \
         WHERE j.id = $1 AND j.source_template_id IS NOT NULL \
         GROUP BY j.source_template_id, jl.service_date \
         ON CONFLICT (template_id, service_date) DO NOTHING",
    )
    .bind(journey_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Deletes an ENTIRE journey the caller owns, in one step -- the direct
/// "delete this journey" action [`delete_leg`]'s own doc comment names as
/// a deliberate non-goal ("Deliberately still not a general 'delete a
/// journey' route ... this only ever removes ONE leg"). Until this
/// function existed, the only way to remove a whole journey was to call
/// [`delete_leg`] once per leg until none remained, which only
/// incidentally deleted the `journeys` row on the LAST call -- a
/// traveller who wanted to abandon a whole multi-leg journey in one step
/// had no such action.
///
/// A single `DELETE FROM journeys WHERE id = $1 AND user_id = $2` is
/// sufficient on its own -- no explicit `journey_legs`/other cleanup
/// needed. Verified against every migration that adds a `REFERENCES
/// journeys(id)` foreign key (there are exactly two):
/// `journey_legs.journey_id ... ON DELETE CASCADE`
/// (`20260922090000_journeys.sql`) removes every leg of this journey, and
/// `journey_leg_notification_state.journey_leg_id ... ON DELETE CASCADE`
/// (`20260922130000_journey_leg_notification_state.sql`) transitively
/// removes any per-leg notification-dedup rows through THAT cascade in
/// turn -- Postgres walks a multi-hop `ON DELETE CASCADE` chain in one
/// statement, no second cascade needed on this table's own FK. `group_journeys.journey_id
/// ... ON DELETE CASCADE` (`20260922110000_group_journeys.sql`) removes
/// any group-sharing grants for this journey too, so a deleted journey
/// never lingers as a dangling entry in a group's shared list. The one FK
/// pointing the OTHER way, `journeys.source_template_id ... ON DELETE SET
/// NULL` (`20260922140000_journey_templates.sql`), is irrelevant to this
/// function -- that column describes what happens to a journey when its
/// SOURCE TEMPLATE is deleted, not the reverse; deleting a journey never
/// touches `journey_templates` at all (a template is a saved, independent
/// SHAPE, not owned by any journey it once produced).
///
/// Each leg's `train_subscriptions` row, if matched, is released as
/// [`delete_leg`] releases one ([`delete_journey_row_releasing_trains`],
/// DS-93): deleted, with its tickets and group shares, unless the user
/// tracks it on its own or another journey's leg uses it. Before DS-93 they
/// were all left behind and showed in `GET /Train/mine` as trains tracked
/// on their own. `train_subscription_id ... ON DELETE SET NULL` runs in
/// the other direction only (deleting the SUBSCRIPTION orphans the leg,
/// not the reverse), so this delete never cascades into that table at
/// all.
///
/// Ownership-scoped via the same folded-in `WHERE ... AND user_id = $N`
/// convention as every other write in this file (never a separate
/// read-then-check race). Returns `true` if a row was deleted, `false`
/// for "no such journey, or not this caller's" (the route maps this to
/// `404`, never `403`, matching [`delete_leg`]'s own convention).
pub async fn delete_journey(pool: &PgPool, journey_id: i64, user_id: &str) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;

    // Ownership-scoped read FIRST, so the tombstone write below can never
    // record a skip for a journey this caller doesn't own (the tombstone
    // statement itself is keyed only by journey id -- ownership is proven
    // here instead, inside the same transaction as the delete, so a
    // not-yours journey leaves no trace at all and still reports `false`).
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM journeys WHERE id = $1 AND user_id = $2")
            .bind(journey_id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    if owned.is_none() {
        return Ok(false);
    }

    // Deleting a template-minted occurrence is the user saying "not this
    // one" -- without this the recurrence sweep re-mints it within the
    // hour. See `record_template_occurrence_skips`.
    record_template_occurrence_skips(&mut tx, journey_id).await?;

    let deleted = delete_journey_row_releasing_trains(&mut tx, journey_id, user_id).await?;
    tx.commit().await?;
    Ok(deleted)
}

/// Deletes `user_id`'s journey `journey_id` (its legs cascade) and then
/// releases each leg's train subscription with
/// [`release_leg_subscription`] (no `move_to`), all on the caller's
/// transaction: [`delete_journey`] and the delegated delete
/// (`delegated::delete_journey_delegated`) both end this way (DS-93).
/// Returns whether the journey row was deleted.
pub(crate) async fn delete_journey_row_releasing_trains(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<bool> {
    // The journey row first, as every add-leg path and [`delete_leg`] lock
    // it: a leg added concurrently either committed already (and is read
    // below) or waits and then finds no journey. Then every leg row, with
    // no filter on its train: a leg the notifier's auto-match
    // (`notifier::queries::auto_commit_leg_to_train`) or a "Change train"
    // is binding right now is waited for and read as committed, so its new
    // train is released too. Reading the legs without the lock (or only
    // those already bound) let a bind that committed between that read and
    // the delete leave its train behind as one tracked on its own.
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM journeys WHERE id = $1 AND user_id = $2 FOR UPDATE")
            .bind(journey_id)
            .bind(user_id)
            .fetch_optional(&mut *conn)
            .await?;
    if owned.is_none() {
        return Ok(false);
    }
    let legs: Vec<Option<i64>> = sqlx::query_scalar(
        "SELECT train_subscription_id FROM journey_legs WHERE journey_id = $1 \
         ORDER BY id FOR UPDATE",
    )
    .bind(journey_id)
    .fetch_all(&mut *conn)
    .await?;
    // Every leg's train once, whichever legs share it, in id order.
    let subscriptions: std::collections::BTreeSet<i64> = legs.into_iter().flatten().collect();
    let result = sqlx::query("DELETE FROM journeys WHERE id = $1 AND user_id = $2")
        .bind(journey_id)
        .bind(user_id)
        .execute(&mut *conn)
        .await?;
    for subscription in subscriptions {
        release_leg_subscription(&mut *conn, user_id, subscription, None).await?;
    }
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
#[expect(
    clippy::struct_excessive_bools,
    reason = "a wire row: each flag is an independent API field, not a state machine"
)]
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyListItem {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
    /// `'web'`, or the delegated client that saved the journey (spec
    /// 2026-10-14 §5.8). A delegated client may delete only its own
    /// (§5.5a), so its list needs this.
    pub created_via: String,
    pub leg_id: i64,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    /// The current leg's own `journey_legs.service_date` -- the date the
    /// traveller is travelling, NOT `created_at` (the date they set the
    /// journey up, which can be weeks earlier). A list row cannot name
    /// which journey it is without it; `/track/mine`'s tracked-train rows
    /// beside it have printed a real service date since they shipped.
    pub service_date: NaiveDate,
    /// Resolved display names for `origin_crs`/`destination_crs`, via the
    /// same `LEFT JOIN stations ... ON s.crs = UPPER(...)` mechanism
    /// `train_tracking::list_tracked_trains_for_user` already uses (see
    /// its comment for why `UPPER` is mandatory). `None` when the code is
    /// itself `None`, or when no reference row exists for it --
    /// `lib/stationLabel.ts`'s `routeLabel` degrades both ends to bare
    /// codes together in that case, rather than mixing one resolved name
    /// with one bare code.
    pub origin_name: Option<String>,
    pub destination_name: Option<String>,
    /// The whole journey's ends, for naming it: the FIRST leg's origin and
    /// the LAST leg's destination (by `leg_order`), whatever the current
    /// leg is. `origin_crs`/`destination_crs` above are the current leg's
    /// own ends, which for a journey not yet under way are the first
    /// leg's -- naming a planned three-leg journey from them showed it
    /// ending at its first change. Names resolve as above. Additive
    /// (2026-10-20).
    pub first_origin_crs: Option<String>,
    pub first_origin_name: Option<String>,
    pub last_destination_crs: Option<String>,
    pub last_destination_name: Option<String>,
    /// The first leg's `service_date`: the day the journey starts, which
    /// is what names it. `service_date` above is the current leg's.
    pub first_service_date: NaiveDate,
    /// How many legs the journey has.
    pub leg_count: i64,
    pub match_mode: String,
    pub train_subscription_id: Option<i64>,
    pub resolution_status: Option<String>,
    pub status: Option<String>,
    /// The delay at the passenger's own stop, against the PUBLIC timetable
    /// (`data::stop_delay`, design doc §9 decision 2): the current leg's destination. `None` until
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
    /// The leg's shared `trains` row and its UID, internal: what the delay
    /// is read for.
    #[serde(skip_serializing)]
    pub trains_id: Option<i64>,
    #[serde(skip_serializing)]
    pub train_uid: Option<String>,
    /// The current leg's kind: `train` or `untracked` (spec 2026-10-20
    /// §2.4). For an untracked leg `originName`/`destinationName` carry its
    /// place names, and the five fields below are set.
    pub kind: String,
    pub transit_mode: Option<String>,
    pub line_name: Option<String>,
    pub planned_departure: Option<DateTime<Utc>>,
    pub planned_arrival: Option<DateTime<Utc>>,
    /// The untracked leg's clock-only state; `None` for a train leg.
    #[sqlx(skip)]
    pub untracked_status: Option<UntrackedStatus>,
    /// Whether the leg's `trains` row carries timetable data, internal: an
    /// input to `tracking_phase`.
    #[serde(skip_serializing)]
    pub timetable_on_row: bool,
    /// The current train leg's `trackingPhase` (see
    /// `train_tracking::TrackedTrainState::tracking_phase`); `None` for an
    /// open or untracked leg.
    #[sqlx(skip)]
    pub tracking_phase: Option<crate::data::tracking_phase::TrackingPhase>,
    #[sqlx(skip)]
    pub first_report_expected_at: Option<DateTime<Utc>>,
    /// The whole journey, not just the current leg (additive, 2026-10-21):
    /// how many legs are train legs, have a cancelled train, are train legs
    /// with no train picked, are over and have started. A train leg is over
    /// once its train is `completed` or has reached the leg's destination
    /// per `travel_order::train_arrived_sql`; an untracked leg once its
    /// planned arrival has passed.
    pub train_leg_count: i64,
    /// Whether the journey is over, by the same rule that splits the
    /// list's travel order (`travel_order`): the last leg has actually
    /// arrived (a cancelled last train once its departure has passed), or
    /// every leg is finished. Rows come still-to-come first, so the first
    /// `true` is where past journeys begin.
    pub travelled: bool,
    pub cancelled_leg_count: i64,
    pub unmatched_leg_count: i64,
    pub finished_leg_count: i64,
    pub started_leg_count: i64,
    /// The delay at the journey's final arrival: the last train leg's
    /// public delay at its own destination. A journey is on time or late
    /// by this, not by an earlier leg. `None` when not known, when the
    /// journey has no train leg, or when that train is cancelled.
    #[sqlx(skip)]
    pub final_arrival_delay_minutes: Option<i32>,
    #[sqlx(skip)]
    pub final_arrival_delay_provisional: bool,
    /// The last train leg's train, date, destination, running delay and
    /// status, internal: what `final_arrival_delay_minutes` is read for.
    #[serde(skip_serializing)]
    pub final_trains_id: Option<i64>,
    #[serde(skip_serializing)]
    pub final_train_uid: Option<String>,
    #[serde(skip_serializing)]
    pub final_service_date: Option<NaiveDate>,
    #[serde(skip_serializing)]
    pub final_destination_crs: Option<String>,
    #[serde(skip_serializing)]
    pub final_working_delay_minutes: Option<i32>,
    #[serde(skip_serializing)]
    pub final_status: Option<String>,
}

/// One journey's final arrival, for [`apply_public_delays`]: the last
/// train leg's own destination.
///
/// [`apply_public_delays`]: crate::data::stop_delay::apply_public_delays
struct FinalArrivalDelay {
    target: Option<crate::data::stop_delay::StopDelayTarget>,
    delay: Option<crate::data::stop_delay::StopDelay>,
}

impl crate::data::stop_delay::PublicDelayFields for FinalArrivalDelay {
    fn delay_target(&self) -> Option<crate::data::stop_delay::StopDelayTarget> {
        self.target.clone()
    }

    fn set_public_delay(&mut self, delay: Option<crate::data::stop_delay::StopDelay>) {
        self.delay = delay;
    }
}

/// Fills each row's `final_arrival_delay_*` in one batched read.
async fn apply_final_arrival_delays(
    pool: &PgPool,
    rows: &mut [JourneyListItem],
) -> anyhow::Result<()> {
    let mut finals: Vec<FinalArrivalDelay> = rows
        .iter()
        .map(|row| FinalArrivalDelay {
            target: match (row.final_service_date, row.final_status.as_deref()) {
                (Some(date), status) if status != Some("cancelled") => {
                    crate::data::stop_delay::target(
                        row.final_trains_id,
                        row.final_train_uid.as_deref(),
                        date,
                        row.final_destination_crs.as_deref(),
                        row.final_working_delay_minutes,
                    )
                }
                _ => None,
            },
            delay: None,
        })
        .collect();
    crate::data::stop_delay::apply_public_delays(pool, &mut finals).await?;
    for (row, fin) in rows.iter_mut().zip(finals) {
        (
            row.final_arrival_delay_minutes,
            _,
            row.final_arrival_delay_provisional,
        ) = crate::data::stop_delay::split(fin.delay);
    }
    Ok(())
}

impl crate::data::tracking_phase::PhaseRow for JourneyListItem {
    fn phase_key(&self) -> Option<(&str, NaiveDate)> {
        self.train_uid
            .as_deref()
            .map(|uid| (uid, self.service_date))
    }

    fn phase_signals(&self) -> crate::data::tracking_phase::PhaseSignals<'_> {
        crate::data::tracking_phase::PhaseSignals {
            applies: self.kind == LEG_KIND_TRAIN && self.train_subscription_id.is_some(),
            activated: self.resolution_status.as_deref() == Some("resolved"),
            status: self.status.as_deref(),
            timetable_on_row: self.timetable_on_row,
        }
    }

    fn set_phase(
        &mut self,
        phase: crate::data::tracking_phase::TrackingPhase,
        expected: Option<DateTime<Utc>>,
    ) {
        self.tracking_phase = Some(phase);
        self.first_report_expected_at = expected;
    }
}

impl crate::data::stop_delay::PublicDelayFields for JourneyListItem {
    fn delay_target(&self) -> Option<crate::data::stop_delay::StopDelayTarget> {
        crate::data::stop_delay::target(
            self.trains_id,
            self.train_uid.as_deref(),
            self.service_date,
            self.destination_crs.as_deref(),
            self.working_delay_minutes,
        )
    }

    fn set_public_delay(&mut self, delay: Option<crate::data::stop_delay::StopDelay>) {
        (self.delay_minutes, self.delay_basis, self.delay_provisional) =
            crate::data::stop_delay::split(delay);
    }
}

/// `list_journeys_for_user`'s query, built once: its sort key is assembled
/// from `travel_order`'s shared fragments.
static LIST_JOURNEYS_SQL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    use crate::data::travel_order::{
        day_start_sql, order_by_sql, scheduled_departure_sql, train_arrived_sql,
    };
    format!(
        "WITH keyed AS ( \
             SELECT j.id, j.custom_name, j.created_at, j.created_via, \
                    s.first_kind, s.first_service_date, s.first_origin_crs, \
                    s.first_planned_departure, s.first_trains_id, s.first_pin, \
                    CASE WHEN s.first_kind = 'untracked' \
                         THEN (s.first_planned_departure AT TIME ZONE 'Europe/London')::date \
                         ELSE s.first_service_date \
                    END AS first_date, \
                    (s.all_finished \
                     OR CASE WHEN s.last_kind = 'untracked' \
                             THEN s.last_planned_arrival <= now() \
                             WHEN s.last_status = 'cancelled' \
                             THEN COALESCE({last_departure}, \
                                           {last_day_end}) <= now() \
                             ELSE {last_arrived} \
                        END) AS travelled \
             FROM journeys j \
             CROSS JOIN LATERAL ( \
                 SELECT (array_agg(l.kind ORDER BY l.leg_order))[1] AS first_kind, \
                        (array_agg(l.service_date ORDER BY l.leg_order))[1] AS first_service_date, \
                        (array_agg(l.origin_crs ORDER BY l.leg_order))[1] AS first_origin_crs, \
                        (array_agg(l.planned_departure ORDER BY l.leg_order))[1] \
                            AS first_planned_departure, \
                        (array_agg(ts.trains_id ORDER BY l.leg_order))[1] AS first_trains_id, \
                        (array_agg(ts.pin_scheduled_departure ORDER BY l.leg_order))[1] \
                            AS first_pin, \
                        (array_agg(l.kind ORDER BY l.leg_order DESC))[1] AS last_kind, \
                        (array_agg(l.service_date ORDER BY l.leg_order DESC))[1] \
                            AS last_service_date, \
                        (array_agg(l.destination_crs ORDER BY l.leg_order DESC))[1] \
                            AS last_destination_crs, \
                        (array_agg(l.planned_arrival ORDER BY l.leg_order DESC))[1] \
                            AS last_planned_arrival, \
                        (array_agg(ts.trains_id ORDER BY l.leg_order DESC))[1] AS last_trains_id, \
                        (array_agg(cs.delay_minutes ORDER BY l.leg_order DESC))[1] AS last_delay, \
                        (array_agg(cs.status ORDER BY l.leg_order DESC))[1] AS last_status, \
                        (array_agg(l.origin_crs ORDER BY l.leg_order DESC))[1] AS last_origin_crs, \
                        bool_and(COALESCE(cs.status = 'completed', FALSE) \
                                 OR (l.kind = 'untracked' AND l.planned_arrival <= now())) \
                            AS all_finished \
                 FROM journey_legs l \
                 LEFT JOIN train_subscriptions ts ON ts.id = l.train_subscription_id \
                 LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
                 WHERE l.journey_id = j.id \
                 HAVING COUNT(*) > 0 \
             ) s \
             WHERE j.user_id = $1 \
         ), \
         candidates AS ( \
             SELECT k.*, RANK() OVER (ORDER BY {coarse_order}) AS coarse_rank \
             FROM keyed k \
         ), \
         timed AS MATERIALIZED ( \
             SELECT c.id, c.custom_name, c.created_at, c.created_via, c.travelled, \
                    COALESCE(CASE WHEN c.first_kind = 'untracked' THEN c.first_planned_departure \
                                  ELSE COALESCE({first_departure}, c.first_pin, \
                                                (SELECT ft.scheduled_departure FROM trains ft \
                                                 WHERE ft.id = c.first_trains_id)) \
                             END, \
                             {first_day_start}) AS departs_at \
             FROM candidates c \
             WHERE c.coarse_rank <= $2 \
         ), \
         recent AS ( \
             SELECT t.id, t.custom_name, t.created_at, t.created_via, t.travelled, \
                    ROW_NUMBER() OVER (ORDER BY {order}, t.id DESC) AS position \
             FROM timed t \
             ORDER BY position \
             LIMIT $2 \
         ), \
         ranked_legs AS ( \
             SELECT jl.id, jl.journey_id, jl.leg_order, jl.origin_crs, jl.destination_crs, \
                    jl.service_date, jl.match_mode, jl.train_subscription_id, \
                    ts.resolution_status, cs.status, cs.delay_minutes, \
                    ts.trains_id, tr.train_uid, \
                    jl.kind, jl.transit_mode, jl.line_name, jl.origin_place, \
                    jl.destination_place, jl.planned_departure, jl.planned_arrival, \
                    (tr.calling_points IS NOT NULL OR tr.origin_crs IS NOT NULL) \
                        AS timetable_on_row, \
                    COUNT(*) OVER (PARTITION BY jl.journey_id) AS leg_count, \
                    FIRST_VALUE(jl.origin_crs) OVER first_leg AS first_origin_crs, \
                    FIRST_VALUE(jl.origin_place) OVER first_leg AS first_origin_place, \
                    FIRST_VALUE(jl.service_date) OVER first_leg AS first_service_date, \
                    FIRST_VALUE(jl.destination_crs) OVER last_leg AS last_destination_crs, \
                    FIRST_VALUE(jl.destination_place) OVER last_leg AS last_destination_place, \
                    COUNT(*) FILTER (WHERE jl.kind = 'train') OVER journey AS train_leg_count, \
                    COUNT(*) FILTER (WHERE jl.kind = 'train' AND cs.status = 'cancelled') \
                        OVER journey AS cancelled_leg_count, \
                    COUNT(*) FILTER (WHERE jl.kind = 'train' AND jl.train_subscription_id IS NULL) \
                        OVER journey AS unmatched_leg_count, \
                    COUNT(*) FILTER (WHERE CASE WHEN jl.kind = 'untracked' \
                                                THEN jl.planned_arrival <= now() \
                                                WHEN ts.trains_id IS NULL THEN FALSE \
                                                ELSE cs.status = 'completed' \
                                                     OR (cs.status IS DISTINCT FROM 'cancelled' \
                                                         AND {leg_arrived}) \
                                           END) OVER journey AS finished_leg_count, \
                    COUNT(*) FILTER (WHERE CASE WHEN jl.kind = 'untracked' \
                                                THEN jl.planned_departure <= now() \
                                                ELSE cs.status IN ('en_route', 'completed') \
                                           END) OVER journey AS started_leg_count, \
                    FIRST_VALUE(CASE WHEN jl.kind = 'train' THEN ts.trains_id END) \
                        OVER last_train AS final_trains_id, \
                    FIRST_VALUE(CASE WHEN jl.kind = 'train' THEN tr.train_uid END) \
                        OVER last_train AS final_train_uid, \
                    FIRST_VALUE(CASE WHEN jl.kind = 'train' THEN jl.service_date END) \
                        OVER last_train AS final_service_date, \
                    FIRST_VALUE(CASE WHEN jl.kind = 'train' THEN jl.destination_crs END) \
                        OVER last_train AS final_destination_crs, \
                    FIRST_VALUE(CASE WHEN jl.kind = 'train' THEN cs.delay_minutes END) \
                        OVER last_train AS final_working_delay_minutes, \
                    FIRST_VALUE(CASE WHEN jl.kind = 'train' THEN cs.status END) \
                        OVER last_train AS final_status, \
                    ROW_NUMBER() OVER ( \
                        PARTITION BY jl.journey_id \
                        ORDER BY (cs.status IS DISTINCT FROM 'completed' \
                                  AND NOT (jl.kind = 'untracked' \
                                           AND jl.planned_arrival <= now())) DESC, \
                                 CASE WHEN cs.status = 'completed' \
                                        OR (jl.kind = 'untracked' \
                                            AND jl.planned_arrival <= now()) \
                                      THEN -jl.leg_order ELSE jl.leg_order END ASC \
                    ) AS rn \
             FROM journey_legs jl \
             LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
             LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
             LEFT JOIN trains tr ON tr.id = ts.trains_id \
             WHERE jl.journey_id IN (SELECT id FROM recent) \
             WINDOW first_leg AS (PARTITION BY jl.journey_id ORDER BY jl.leg_order ASC), \
                    last_leg AS (PARTITION BY jl.journey_id ORDER BY jl.leg_order DESC), \
                    last_train AS (PARTITION BY jl.journey_id \
                                   ORDER BY (jl.kind = 'train') DESC, jl.leg_order DESC), \
                    journey AS (PARTITION BY jl.journey_id) \
         ) \
         SELECT r.id, r.custom_name, r.created_at, r.created_via, r.travelled, \
                rl.id AS leg_id, rl.origin_crs, rl.destination_crs, rl.service_date, \
                COALESCE(so.name, rl.origin_place) AS origin_name, \
                COALESCE(sd.name, rl.destination_place) AS destination_name, \
                rl.first_origin_crs, COALESCE(sfo.name, rl.first_origin_place) AS first_origin_name, \
                rl.last_destination_crs, \
                COALESCE(sld.name, rl.last_destination_place) AS last_destination_name, \
                rl.first_service_date, rl.leg_count, rl.match_mode, \
                rl.train_subscription_id, rl.resolution_status, rl.status, rl.delay_minutes, \
                rl.delay_minutes AS working_delay_minutes, rl.trains_id, rl.train_uid, \
                rl.kind, rl.transit_mode, rl.line_name, rl.planned_departure, \
                rl.planned_arrival, rl.timetable_on_row, \
                rl.train_leg_count, rl.cancelled_leg_count, rl.unmatched_leg_count, \
                rl.finished_leg_count, rl.started_leg_count, \
                rl.final_trains_id, rl.final_train_uid, rl.final_service_date, \
                rl.final_destination_crs, rl.final_working_delay_minutes, rl.final_status \
         FROM recent r \
         JOIN ranked_legs rl ON rl.journey_id = r.id AND rl.rn = 1 \
         LEFT JOIN stations so ON so.crs = UPPER(rl.origin_crs)::bpchar \
         LEFT JOIN stations sd ON sd.crs = UPPER(rl.destination_crs)::bpchar \
         LEFT JOIN stations sfo ON sfo.crs = UPPER(rl.first_origin_crs)::bpchar \
         LEFT JOIN stations sld ON sld.crs = UPPER(rl.last_destination_crs)::bpchar \
         ORDER BY r.position",
        last_arrived = train_arrived_sql(
            "s.last_trains_id",
            "(SELECT lt.train_uid FROM trains lt WHERE lt.id = s.last_trains_id)",
            "s.last_service_date",
            "s.last_destination_crs",
            "s.last_delay",
        ),
        // A cancelled last train never arrives: the journey is over once
        // that train's timetabled departure has passed, as on the home page.
        last_departure = scheduled_departure_sql(
            "(SELECT lt.train_uid FROM trains lt WHERE lt.id = s.last_trains_id)",
            "s.last_service_date",
            "s.last_origin_crs",
        ),
        last_day_end = day_start_sql("s.last_service_date + 1"),
        coarse_order = order_by_sql("k.travelled", "k.first_date"),
        first_departure = scheduled_departure_sql(
            "(SELECT ft.train_uid FROM trains ft WHERE ft.id = c.first_trains_id)",
            "c.first_service_date",
            "c.first_origin_crs",
        ),
        first_day_start = day_start_sql("c.first_date"),
        order = order_by_sql("t.travelled", "t.departs_at"),
        leg_arrived = train_arrived_sql(
            "ts.trains_id",
            "tr.train_uid",
            "jl.service_date",
            "jl.destination_crs",
            "cs.delay_minutes",
        ),
    )
});

/// Journeys in travel order (`travel_order`, DS-112): still to come or
/// under way, soonest first departure first, then travelled, most recent
/// first; capped at the same `train_tracking::MINE_LIST_LIMIT` `GET
/// /Train/mine` uses -- `pub(crate)` on that constant already permits this
/// cross-module read (see its own doc comment, which anticipates exactly
/// this: "any list this list's own cap should agree with"). Each journey
/// is shown with its "current leg" -- the earliest leg that isn't already
/// `completed`, or the journey's LAST leg (highest `leg_order`) if every
/// leg is `completed`.
///
/// The first leg's departure is an untracked leg's `planned_departure`, a
/// train leg's timetabled departure from the leg's own origin, else its
/// subscription's pinned departure, else the train's own scheduled
/// departure, else the start of its service date. The journey is
/// travelled once its last leg has actually arrived (an untracked leg's
/// planned arrival has passed; a train leg's per
/// `travel_order::train_arrived_sql`: TRUST's reported arrival, else the
/// timetable plus the live delay, else plus
/// `TRAVEL_ARRIVAL_GRACE_MINUTES`), or every leg is finished (the rule
/// `ranked_legs` uses for the current leg). Ordering by `created_at` instead kept the
/// newest-saved journeys, so a trip saved weeks ahead could drop off
/// behind a hundred since-travelled ones.
///
/// **Order and cap first, then legs (DB2-18).** `keyed` reads each of the
/// user's journeys' legs once (one index scan per journey; `HAVING` drops
/// a leg-less journey) for the first and last legs' fields. Only the
/// day of the first departure is needed to rank most journeys, so
/// `candidates` ranks on that (`RANK`, so every journey tied with the
/// `MINE_LIST_LIMIT`-th stays in), and only those get the timetable probe
/// for the exact time (`timed`). `recent` keeps the first `MINE_LIST_LIMIT`
/// of those, and only their legs are ranked. (A train leg leaving after
/// midnight counts as its service date's for that first cut, so at the
/// cap's very edge it is kept or dropped by that date.) With 1,500
/// journeys (seeded, EXPLAIN ANALYZE) the query takes about 30 ms against
/// 5 ms for the old `created_at` order.
pub async fn list_journeys_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<JourneyListItem>> {
    let rows = sqlx::query_as::<_, JourneyListItem>(LIST_JOURNEYS_SQL.as_str())
        .bind(user_id)
        .bind(crate::data::train_tracking::MINE_LIST_LIMIT)
        .fetch_all(pool)
        .await?;
    let mut rows = rows;
    crate::data::stop_delay::apply_public_delays(pool, &mut rows).await?;
    apply_final_arrival_delays(pool, &mut rows).await?;
    let now = Utc::now();
    for row in &mut rows {
        if let (Some(departure), Some(arrival)) = (row.planned_departure, row.planned_arrival) {
            row.untracked_status = Some(untracked_status(departure, arrival, now));
        }
    }
    crate::data::tracking_phase::attach(pool, &mut rows, now).await;
    Ok(rows)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct JourneySummaryRow {
    pub id: i64,
    pub custom_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub user_id: String,
    /// `'web'`, or the delegated client that saved it
    /// (`crate::auth::delegated`, migration `20261015010000`).
    pub created_via: String,
}

/// Unscoped journey summary fetch -- `journey_id` alone, no `user_id`
/// filter. Paired with [`journey_readable_by`] (call that FIRST to
/// authorize the read; this function only fetches once authorization has
/// already passed) rather than folding both into one query, mirroring
/// `train_tracking::tracked_train_owner` + `get_by_tracking_id`'s existing
/// "separate gate, unscoped fetch" split in this codebase. Defensive
/// `Option` return (mapped to the same 404 by the caller) rather than an
/// `.expect()`/`.unwrap()` past the DB round-trip -- should always be
/// `Some` given `journey_readable_by` already confirmed the journey exists,
/// but a second query is still a second query.
pub async fn get_journey_summary(
    pool: &PgPool,
    journey_id: i64,
) -> anyhow::Result<Option<JourneySummaryRow>> {
    let row = sqlx::query_as::<_, JourneySummaryRow>(
        "SELECT id, custom_name, created_at, user_id, created_via FROM journeys WHERE id = $1",
    )
    .bind(journey_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Whether `user_id` may read journey `journey_id`'s full detail: either
/// they own it outright, or it's been shared (`group_journeys`, see
/// `crates/api/src/data/groups.rs`) into at least one group they're
/// currently a member of. Mirrors `custom_lines::readable_custom_line_ids`'s
/// "owned OR granted-into-a-group-I'm-in" shape (see
/// docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md
/// §3.2) -- the closest existing precedent for widening a private
/// resource's read gate to group sharing -- but returns a single `bool`
/// rather than a batched id set: this function has exactly one call site
/// (`GET /Journeys/{journeyId}`, a single-id read), and no bulk
/// journey-status list exists in this codebase to justify the extra
/// complexity a `HashSet<i64>`-returning, `ANY($1)`-parameterized version
/// would add for zero current callers.
///
/// READ-ONLY AUTHORIZATION ONLY. This function must NEVER be used to gate
/// a write route (rename/add-leg/commit-leg/delete a journey, or anything
/// under `/Journeys/*` that mutates state) -- every write stays scoped to
/// `journeys.user_id = caller.id` alone, via each write function's own
/// folded-in `WHERE ... AND user_id = $N` ownership clause (e.g.
/// `get_owned_leg`/`set_leg_train_subscription` in this file), unchanged by
/// this feature. Sharing a
/// journey into a group conveys READ access ONLY, per
/// docs/superpowers/specs/2026-09-22-journey-tracking-design.md §6 -- the
/// same hard boundary `custom_line_group_grants`/`group_trains` already
/// enforce for their own resources ("no group role can edit or delete
/// someone else's shared resource, only unshare it").
pub async fn journey_readable_by(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
) -> anyhow::Result<bool> {
    let (readable,): (bool,) = sqlx::query_as(
        "SELECT EXISTS (SELECT 1 FROM journeys j WHERE j.id = $1 AND j.user_id = $2) \
         OR EXISTS ( \
             SELECT 1 FROM group_journeys gj \
             JOIN group_members gm ON gm.group_id = gj.group_id AND gm.user_id = $2 \
             WHERE gj.journey_id = $1 \
         )",
    )
    .bind(journey_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(readable)
}

/// Every leg of a journey, `leg_order` ascending -- Phase 1 callers only
/// ever see one row (this module's own doc comment), but this is already
/// shaped for a later phase's longer result. Deliberately NOT
/// ownership-scoped on its own (unlike [`get_owned_leg`]) -- the one real
/// caller (`routes::journeys::get_journey`) already confirmed the journey
/// is READABLE by the caller via [`journey_readable_by`] one call earlier
/// in the same request (owner OR group-shared-with, since Task 4 -- see
/// that function's own doc comment), so re-checking here would be a
/// redundant query, not a real safety gain. This is also exactly what
/// makes Task 4's invariant 2 true by construction: nothing downstream of
/// `journey_readable_by` re-filters a leg's `train_subscriptions` row by
/// `user_id`, because this function never took a `user_id` to filter by in
/// the first place.
pub async fn list_legs_for_journey(
    pool: &PgPool,
    journey_id: i64,
) -> anyhow::Result<Vec<JourneyLegWithNamesRow>> {
    let rows = sqlx::query_as::<_, JourneyLegWithNamesRow>(
        "SELECT jl.id, jl.journey_id, jl.origin_crs, so.name AS origin_name, \
                jl.destination_crs, sd.name AS destination_name, jl.service_date, \
                jl.depart_after, jl.depart_before, jl.arrive_after, jl.arrive_before, \
                jl.train_subscription_id, jl.match_mode, jl.window_searched, jl.kind, \
                jl.transit_mode, jl.origin_place, jl.destination_place, jl.line_name, \
                jl.planned_departure, jl.planned_arrival, jl.origin_stop_id, \
                jl.destination_stop_id \
         FROM journey_legs jl \
         LEFT JOIN stations so ON so.crs = UPPER(jl.origin_crs)::bpchar \
         LEFT JOIN stations sd ON sd.crs = UPPER(jl.destination_crs)::bpchar \
         WHERE jl.journey_id = $1 ORDER BY jl.leg_order",
    )
    .bind(journey_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// One leg of `POST /Journeys/itinerary`, validated and resolved to its
/// shared `trains` row by the route (a train leg), or an untracked leg
/// (spec 2026-10-20 §2.4).
#[derive(Debug, Clone)]
pub enum ItineraryLeg {
    Train {
        trains_id: i64,
        service_date: NaiveDate,
        /// The leg's own boarding and alighting points, as for
        /// [`create_journey_with_known_train_leg`]'s overrides.
        origin_crs: Option<String>,
        destination_crs: Option<String>,
    },
    Untracked(UntrackedLeg),
}

/// Creates a journey and every one of its legs on `tx`, the caller's
/// transaction, so `POST /Journeys/itinerary` is all or nothing (spec
/// 2026-10-14 §5.8, replacing `PlanTripFlow`'s one-request-per-leg loop).
/// Each train leg is built exactly as [`create_journey_with_known_train_leg`]
/// builds its one leg; an untracked leg as [`add_untracked_leg_to_journey`]
/// builds one. `created_via` is `'web'` or the delegated client's name.
/// Returns the journey id and `(leg_id, tracking_id)` per leg, in order,
/// `tracking_id` `None` for an untracked leg.
pub async fn create_journey_from_itinerary(
    tx: &mut sqlx::PgConnection,
    user_id: &str,
    custom_name: Option<&str>,
    created_via: &str,
    legs: &[ItineraryLeg],
) -> anyhow::Result<(i64, Vec<(i64, Option<i64>)>)> {
    let (journey_id,): (i64,) = sqlx::query_as(
        "INSERT INTO journeys (user_id, custom_name, created_via) VALUES ($1, $2, $3) RETURNING id",
    )
    .bind(user_id)
    .bind(custom_name)
    .bind(created_via)
    .fetch_one(&mut *tx)
    .await?;
    let mut created = Vec::with_capacity(legs.len());
    for (index, leg) in legs.iter().enumerate() {
        let leg_order = i32::try_from(index + 1)?;
        let (trains_id, service_date, origin_override, destination_override) = match leg {
            ItineraryLeg::Untracked(untracked) => {
                let leg_id =
                    insert_untracked_leg(&mut *tx, journey_id, leg_order, untracked).await?;
                created.push((leg_id, None));
                continue;
            }
            ItineraryLeg::Train {
                trains_id,
                service_date,
                origin_crs,
                destination_crs,
            } => (*trains_id, *service_date, origin_crs, destination_crs),
        };
        let tracking_id = crate::data::train_tracking::create_subscription_for_train(
            &mut *tx, trains_id, user_id,
        )
        .await?;
        let pins: Option<(Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT pin_origin_crs, pin_destination_crs FROM train_subscriptions WHERE id = $1",
        )
        .bind(tracking_id)
        .fetch_optional(&mut *tx)
        .await?;
        let (pin_origin_crs, pin_destination_crs) = pins.unwrap_or((None, None));
        let origin_crs = origin_override.clone().or(pin_origin_crs);
        let destination_crs = destination_override.clone().or(pin_destination_crs);
        let leg_id = insert_leg(
            &mut *tx,
            journey_id,
            leg_order,
            origin_crs.as_deref(),
            destination_crs.as_deref(),
            service_date,
            Some(tracking_id),
            "manual",
            None,
            None,
            None,
            None,
            false,
        )
        .await?;
        created.push((leg_id, Some(tracking_id)));
    }
    crate::data::journey_revisions::record_plan_change(&mut *tx, journey_id, Utc::now()).await?;
    Ok((journey_id, created))
}

/// Journeys `user_id` saved through `created_via` in the last 24 hours, for
/// a delegated client's daily cap (spec 2026-10-14 §5.10, D-2), and the
/// oldest one's creation time (when the window next frees a slot).
pub async fn count_journeys_created_via_last_day(
    pool: &PgPool,
    user_id: &str,
    created_via: &str,
) -> anyhow::Result<(i64, Option<DateTime<Utc>>)> {
    Ok(sqlx::query_as(
        "SELECT COUNT(*), MIN(created_at) FROM journeys \
         WHERE user_id = $1 AND created_via = $2 AND created_at > now() - INTERVAL '24 hours'",
    )
    .bind(user_id)
    .bind(created_via)
    .fetch_one(pool)
    .await?)
}

#[cfg(test)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::items_after_statements,
    clippy::too_many_lines,
    reason = "test code: casts of small known test values; fixtures sit next to their use; scenario tests read top to bottom"
)]
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
        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $2) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
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

    /// Bare journey with no legs -- enough for [`journey_readable_by`]'s own
    /// tests below, which only ever query the `journeys` table itself.
    /// Reuses the module's own private `insert_journey` (accessible here via
    /// `use super::*` -- a child module may reach a private item of its
    /// parent) rather than duplicating that one-line `INSERT`.
    async fn seed_journey(pool: &PgPool, user_id: &str) -> i64 {
        insert_journey(pool, user_id, None)
            .await
            .expect("seed fixture journey")
    }

    /// Cleans up exactly one journey (`journey_id`, not "every journey this
    /// user owns" -- unlike [`cleanup_user`]) plus every listed user's own
    /// fixture rows. Deletes `journey_legs` for this journey, then the
    /// journey itself, then each user's `train_subscriptions` rows, then
    /// each user -- same FK-respecting order as `cleanup_user`.
    async fn cleanup_journey(pool: &PgPool, journey_id: i64, user_ids: &[&str]) {
        sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1")
            .bind(journey_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journey_legs");
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(pool)
            .await
            .expect("cleanup fixture journey");
        for user_id in user_ids {
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
    }

    // --- delete_leg (concurrency) -----------------------------------

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                concurrent_deletes_of_different_legs_never_leave_a_zero_leg_journey \
                -- --ignored --test-threads=1`"]
    async fn concurrent_deletes_of_different_legs_never_leave_a_zero_leg_journey() {
        // Regression test for the 19-pass security/bug review's journeys-
        // area Medium finding 4: two concurrent `delete_leg` calls for
        // DIFFERENT legs of the SAME two-leg journey used to both read
        // `remaining = 2` before either committed, so both took the
        // "delete just this leg" branch and left a zero-leg journey behind
        // -- see `delete_leg`'s own updated doc comment for the `SELECT ...
        // FOR UPDATE` fix.
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-DELETE-LEG-RACE";
        seed_user(&pool, user_id).await;
        let journey_id = insert_journey(&pool, user_id, None)
            .await
            .expect("insert journey");
        let leg_a = insert_leg(
            &pool,
            journey_id,
            1,
            Some("WAT"),
            Some("RDG"),
            "2026-09-22".parse().unwrap(),
            None,
            "unmatched",
            None,
            None,
            None,
            None,
            false,
        )
        .await
        .expect("insert leg a");
        let leg_b = insert_leg(
            &pool,
            journey_id,
            2,
            Some("RDG"),
            Some("BRI"),
            "2026-09-22".parse().unwrap(),
            None,
            "unmatched",
            None,
            None,
            None,
            None,
            false,
        )
        .await
        .expect("insert leg b");

        let pool_a = pool.clone();
        let pool_b = pool.clone();
        let (result_a, result_b) = tokio::join!(
            delete_leg(&pool_a, journey_id, leg_a, user_id),
            delete_leg(&pool_b, journey_id, leg_b, user_id),
        );
        let result_a = result_a.expect("delete leg a");
        let result_b = result_b.expect("delete leg b");
        assert!(result_a.is_some(), "leg a delete must find its leg");
        assert!(result_b.is_some(), "leg b delete must find its leg");

        // The bug: both concurrent deletes concluding "the journey
        // survives with one leg left" (`journey_also_deleted = false` for
        // both), because both read `remaining = 2` before either wrote.
        let both_left_the_journey_alive = result_a == Some(false) && result_b == Some(false);
        assert!(
            !both_left_the_journey_alive,
            "both concurrent deletes reported the journey survived -- the \
             zero-leg-journey race this test guards against"
        );

        let (legs_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM journey_legs WHERE journey_id = $1")
                .bind(journey_id)
                .fetch_one(&pool)
                .await
                .expect("count remaining legs");
        let (journeys_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM journeys WHERE id = $1")
                .bind(journey_id)
                .fetch_one(&pool)
                .await
                .expect("count remaining journeys");

        // The only two valid end states: the journey is gone entirely (one
        // of the two deletes correctly saw `remaining = 1` and took the
        // "delete the whole journey" branch), or -- if some future change
        // ever made this race survivable another way -- it has exactly its
        // normal complement of legs, never zero.
        if journeys_left == 0 {
            assert_eq!(
                legs_left, 0,
                "if the journey is gone, no orphaned legs may remain"
            );
        } else {
            assert_eq!(
                legs_left, 1,
                "a surviving journey must never have zero legs"
            );
        }

        cleanup_journey(&pool, journey_id, &[user_id]).await;
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
            // Same "direct struct literal still needs the field" reason as
            // `skipped_stations` above. `None` is exactly "no platform
            // known", which is what this fixture's CRS+time pin models.
            platform: None,
            planned_platform: None,
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_pin_leg -- --ignored --test-threads=1`"]
    async fn create_journey_with_pin_leg_wraps_a_pin_in_a_one_row_journey() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
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
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-WINDOW").await;

        let depart_window = TimeWindow {
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
            TimeWindow::default(),
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
        let err = validate_window_leg("WAT", "RDG", &TimeWindow::default(), &TimeWindow::default())
            .unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn validate_window_leg_accepts_one_bound_set() {
        let depart_window = TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: None,
        };
        assert!(validate_window_leg("WAT", "RDG", &depart_window, &TimeWindow::default()).is_ok());
    }

    #[test]
    // 2026-09 Signal Box Audit Low finding regression: CRS validation used
    // to be `.trim().len() != 3`, a UTF-8 BYTE count. "é" alone is 2 bytes
    // and a bare 3-byte multi-byte character (many exist, e.g. "€", U+20AC)
    // would pass as if it were three ASCII letters. Pin down that a
    // single 3-byte character is rejected, and that it's rejected for the
    // right reason (not merely coincidentally too short/long).
    fn validate_window_leg_rejects_a_three_byte_non_ascii_character_as_origin() {
        let depart_window = TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: None,
        };
        // "€" (U+20AC) encodes to exactly 3 UTF-8 bytes but is one
        // character -- the exact shape of value a byte-length check would
        // have wrongly accepted.
        let err =
            validate_window_leg("€", "RDG", &depart_window, &TimeWindow::default()).unwrap_err();
        assert!(err.contains("origin"));
    }

    #[test]
    fn validate_window_leg_accepts_lowercase_crs() {
        // Case-insensitive, matching `routes::trains::normalize_crs`'s own
        // posture (normalization to uppercase happens downstream).
        let depart_window = TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: None,
        };
        assert!(validate_window_leg("wat", "rdg", &depart_window, &TimeWindow::default()).is_ok());
    }

    #[test]
    // 2026-09 Signal Box Audit Low finding regression: a backwards window
    // (`after` later than `before`) used to be accepted outright and
    // would silently never match any real calling point once persisted.
    fn validate_window_leg_rejects_a_backwards_depart_window() {
        let depart_window = TimeWindow {
            after: Some("10:00:00".parse().unwrap()),
            before: Some("08:00:00".parse().unwrap()),
        };
        let err =
            validate_window_leg("WAT", "RDG", &depart_window, &TimeWindow::default()).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn validate_window_leg_rejects_a_backwards_arrive_window() {
        let arrive_window = TimeWindow {
            after: Some("10:00:00".parse().unwrap()),
            before: Some("08:00:00".parse().unwrap()),
        };
        let err =
            validate_window_leg("WAT", "RDG", &TimeWindow::default(), &arrive_window).unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn validate_window_leg_accepts_after_equal_to_before() {
        // A zero-width window (an exact single minute) is unusual but not
        // backwards -- only `after > before` is rejected, not `after ==
        // before`.
        let depart_window = TimeWindow {
            after: Some("08:00:00".parse().unwrap()),
            before: Some("08:00:00".parse().unwrap()),
        };
        assert!(validate_window_leg("WAT", "RDG", &depart_window, &TimeWindow::default()).is_ok());
    }

    #[test]
    fn validate_window_leg_messages_carry_no_internal_field_names() {
        let messages = [
            validate_window_leg("W", "RDG", &TimeWindow::default(), &TimeWindow::default())
                .unwrap_err(),
            validate_window_leg("WAT", "RDG", &TimeWindow::default(), &TimeWindow::default())
                .unwrap_err(),
        ];
        for message in messages {
            assert!(!message.is_empty());
            assert!(
                !message.contains('_'),
                "user-facing copy leaked an identifier: {message}"
            );
        }
    }

    #[test]
    fn validate_known_train_overrides_messages_carry_no_internal_field_names() {
        let messages = [
            validate_known_train_overrides(Some("X"), None).unwrap_err(),
            validate_known_train_overrides(None, Some("Y")).unwrap_err(),
        ];
        for message in messages {
            assert!(!message.is_empty());
            assert!(
                !message.contains('_'),
                "user-facing copy leaked an identifier: {message}"
            );
        }
    }

    #[test]
    // 2026-09 Signal Box Audit Low finding regression: same byte-length
    // bug as `validate_window_leg`'s own regression test above.
    fn validate_known_train_overrides_rejects_a_three_byte_non_ascii_character() {
        let err = validate_known_train_overrides(Some("€"), None).unwrap_err();
        assert!(err.contains("origin"));
    }

    #[test]
    fn validate_known_train_overrides_accepts_both_omitted() {
        assert!(validate_known_train_overrides(None, None).is_ok());
    }

    #[test]
    fn validate_known_train_overrides_accepts_one_valid_override_with_the_other_omitted() {
        assert!(validate_known_train_overrides(Some("CRE"), None).is_ok());
    }

    #[test]
    fn validate_known_train_overrides_rejects_a_malformed_destination_and_names_it() {
        let err = validate_known_train_overrides(None, Some("PR")).unwrap_err();
        assert!(
            err.contains("destination"),
            "message should name the destination, not the origin: {err}"
        );
    }

    #[test]
    fn validate_known_train_overrides_checks_origin_before_destination() {
        // Both ends are malformed here -- the origin's message must win,
        // matching the sequential-check order in the function body (origin
        // checked first, an early `return` before destination is ever
        // examined). This pins that ordering down as intentional, not
        // incidental.
        let err = validate_known_train_overrides(Some("X"), Some("Y")).unwrap_err();
        assert!(
            err.contains("origin"),
            "origin's message should take priority when both ends are malformed: {err}"
        );
    }

    #[test]
    fn validate_train_uid_accepts_a_well_formed_six_character_uid() {
        assert!(validate_train_uid("C21373").is_ok());
    }

    #[test]
    fn validate_train_uid_accepts_surrounding_whitespace() {
        assert!(validate_train_uid("  C21373  ").is_ok());
    }

    #[test]
    fn validate_train_uid_rejects_something_shorter_than_six_characters() {
        assert!(validate_train_uid("C2137").is_err());
    }

    #[test]
    fn validate_train_uid_rejects_something_longer_than_six_characters() {
        assert!(validate_train_uid("C213733").is_err());
    }

    #[test]
    fn validate_train_uid_rejects_non_alphanumeric_characters() {
        assert!(validate_train_uid("C2137!").is_err());
        assert!(validate_train_uid("C213 3").is_err());
        assert!(validate_train_uid("'; DR").is_err());
    }

    #[test]
    fn validate_train_uid_rejects_an_empty_string() {
        assert!(validate_train_uid("").is_err());
        assert!(validate_train_uid("      ").is_err());
    }

    #[test]
    fn validate_train_uid_message_carries_no_internal_field_names() {
        let err = validate_train_uid("nope").unwrap_err();
        assert!(!err.is_empty());
        assert!(
            !err.contains('_'),
            "user-facing copy leaked an identifier: {err}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    async fn set_leg_train_subscription_binds_an_unmatched_leg_and_is_reusable_for_change_train() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");

        // First pick.
        let first_tracking_id = crate::data::train_tracking::create_pin(
            &pool,
            &fixture_pin("WAT"),
            "TEST-JOURNEY-COMMIT",
        )
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
        let second_tracking_id = crate::data::train_tracking::create_pin(
            &pool,
            &fixture_pin("WAT"),
            "TEST-JOURNEY-COMMIT",
        )
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

        // 2026-09 review finding (Medium), DS-93: the OLD subscription, now
        // unreferenced by any leg and never tracked on its own, is gone --
        // neither left live to keep pushing for a train this leg no longer
        // tracks, nor left behind as a standalone tracked train.
        assert!(
            !subscription_exists(&pool, first_tracking_id).await,
            "the leg-only old subscription must be deleted"
        );
        assert!(
            notifications_enabled_of(&pool, second_tracking_id).await,
            "the new, currently-referenced subscription must stay active"
        );

        cleanup_user(&pool, "TEST-JOURNEY-COMMIT").await;
    }

    async fn subscription_exists(pool: &PgPool, train_subscription_id: i64) -> bool {
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM train_subscriptions WHERE id = $1)")
            .bind(train_subscription_id)
            .fetch_one(pool)
            .await
            .expect("read train_subscriptions")
    }

    async fn notifications_enabled_of(pool: &PgPool, train_subscription_id: i64) -> bool {
        sqlx::query_scalar("SELECT notifications_enabled FROM train_subscriptions WHERE id = $1")
            .bind(train_subscription_id)
            .fetch_one(pool)
            .await
            .expect("read notifications_enabled")
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    #[expect(
        clippy::used_underscore_binding,
        reason = "test binding kept for readability of the fixture"
    )]
    async fn set_leg_train_subscription_does_not_deactivate_a_still_shared_old_subscription() {
        // The old subscription being replaced can legitimately still be
        // referenced by a DIFFERENT leg -- `create_subscription_for_train`
        // is idempotent per `(user_id, trains_id)`, so two legs can share
        // one `train_subscriptions` row. Re-picking one leg's train must
        // not deactivate notifications for the OTHER leg still relying on
        // the shared subscription.
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT-SHARED").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT-SHARED",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create first window leg");
        let (_journey_id_two, other_leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT-SHARED",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("09:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create second window leg (separate journey)");

        let shared_tracking_id = crate::data::train_tracking::create_pin(
            &pool,
            &fixture_pin("WAT"),
            "TEST-JOURNEY-COMMIT-SHARED",
        )
        .await
        .expect("seed the shared candidate subscription");

        assert!(
            set_leg_train_subscription(
                &pool,
                journey_id,
                leg_id,
                "TEST-JOURNEY-COMMIT-SHARED",
                shared_tracking_id,
            )
            .await
            .expect("bind first leg to the shared subscription")
        );
        assert!(
            set_leg_train_subscription(
                &pool,
                _journey_id_two,
                other_leg_id,
                "TEST-JOURNEY-COMMIT-SHARED",
                shared_tracking_id,
            )
            .await
            .expect("bind second leg to the SAME shared subscription")
        );

        // "Change train" on the FIRST leg only -- the second leg still
        // points at `shared_tracking_id`.
        let new_tracking_id = crate::data::train_tracking::create_pin(
            &pool,
            &fixture_pin("WAT"),
            "TEST-JOURNEY-COMMIT-SHARED",
        )
        .await
        .expect("seed a replacement subscription");
        assert!(
            set_leg_train_subscription(
                &pool,
                journey_id,
                leg_id,
                "TEST-JOURNEY-COMMIT-SHARED",
                new_tracking_id,
            )
            .await
            .expect("re-pick the first leg's train")
        );

        assert!(
            notifications_enabled_of(&pool, shared_tracking_id).await,
            "the shared subscription is still referenced by the OTHER leg -- must stay active"
        );

        cleanup_user(&pool, "TEST-JOURNEY-COMMIT-SHARED").await;
    }

    /// 2026-09-26 review, Medium finding 10: `set_leg_train_subscription`
    /// ("Change train") deactivating an orphaned subscription is meant to
    /// be scoped to the journey-leg context it was replacing, not a
    /// permanent verdict on the underlying `(user_id, trains_id)` pin --
    /// `train_tracking::create_subscription_for_train` is idempotent on
    /// that exact key, so before this fix a disabled row, once created,
    /// could never come back: ANY later, unrelated call for the same user
    /// and the same physical train (a brand-new journey leg auto-matching
    /// to it, a standalone "Track this train", a second journey) just
    /// silently inherited the disabled row forever.
    ///
    /// Since DS-93 "Change train" deletes a leg-only old subscription
    /// rather than disabling it, so nothing writes the flag any more; a row
    /// disabled before that still exists, so this disables one directly
    /// (as the old "Change train" left it), then calls
    /// `create_subscription_for_train` directly for the SAME `(user_id,
    /// trains_id)` -- exactly what a brand-new, unrelated journey/tracking
    /// use case does -- and asserts it comes back re-enabled.
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                a_disabled_subscription_is_reactivated_by_a_later_unrelated_use -- --ignored \
                --test-threads=1`"]
    async fn a_disabled_subscription_is_reactivated_by_a_later_unrelated_use() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-M10-REACTIVATE";
        seed_user(&pool, user_id).await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");

        let service_date: NaiveDate = "2026-09-22".parse().unwrap();
        let first_trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-M10-UID-FIRST", service_date)
                .await
                .expect("seed the first physical train");
        let first_tracking_id = crate::data::train_tracking::create_subscription_for_train(
            &pool,
            first_trains_id,
            user_id,
        )
        .await
        .expect("create the first candidate subscription");
        assert!(
            set_leg_train_subscription(&pool, journey_id, leg_id, user_id, first_tracking_id)
                .await
                .expect("commit first pick")
        );

        // A row the old "Change train" disabled: the leg moved to a
        // different train, and this one was left unreferenced and off.
        let second_trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-M10-UID-SECOND", service_date)
                .await
                .expect("seed the second physical train");
        let second_tracking_id = crate::data::train_tracking::create_subscription_for_train(
            &pool,
            second_trains_id,
            user_id,
        )
        .await
        .expect("create the second candidate subscription");
        sqlx::query("UPDATE journey_legs SET train_subscription_id = $1 WHERE id = $2")
            .bind(second_tracking_id)
            .bind(leg_id)
            .execute(&pool)
            .await
            .expect("move the leg the way the old \"Change train\" did");
        sqlx::query("UPDATE train_subscriptions SET notifications_enabled = FALSE WHERE id = $1")
            .bind(first_tracking_id)
            .execute(&pool)
            .await
            .expect("disable the old subscription the way the old \"Change train\" did");

        // The new use case: a LATER, unrelated call for the SAME
        // `(user_id, trains_id)` as the disabled subscription -- e.g. a
        // fresh "Track this train" or a different journey's leg
        // auto-matching onto the same physical train. This has nothing to
        // do with the journey/leg above; it must not silently inherit a
        // notification stream the "Change train" action killed for a
        // different reason entirely.
        let reused_tracking_id = crate::data::train_tracking::create_subscription_for_train(
            &pool,
            first_trains_id,
            user_id,
        )
        .await
        .expect("re-establish a subscription for the same physical train");
        assert_eq!(
            reused_tracking_id, first_tracking_id,
            "create_subscription_for_train's own (user_id, trains_id) idempotency must still \
             return the SAME row, not a duplicate"
        );
        assert!(
            notifications_enabled_of(&pool, first_tracking_id).await,
            "a new, unrelated use of the same physical train must re-enable notifications, not \
             inherit the stale disabled flag forever"
        );

        cleanup_user(&pool, user_id).await;
        sqlx::query("DELETE FROM trains WHERE id IN ($1, $2)")
            .bind(first_trains_id)
            .bind(second_trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    /// Which of `user_id`'s subscriptions the notifier pushes `trains_id`'s
    /// changes to: the same subscriber pick as `notifier::queries::
    /// candidates_for_trains_id` (enabled rows on the train, one per user,
    /// a journey leg's first, then the oldest). `None` means no push.
    async fn pushed_subscription(pool: &PgPool, trains_id: i64, user_id: &str) -> Option<i64> {
        sqlx::query_scalar(
            "SELECT DISTINCT ON (ts.user_id) ts.id FROM train_subscriptions ts \
             WHERE ts.trains_id = $1 AND ts.notifications_enabled AND ts.user_id = $2 \
             ORDER BY ts.user_id, \
                      EXISTS (SELECT 1 FROM journey_legs jl \
                              WHERE jl.train_subscription_id = ts.id) DESC, \
                      ts.id",
        )
        .bind(trains_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .expect("read the notifier's subscriber pick")
    }

    /// `GET /Train/mine` rows no journey folds in. Since DS-124 every
    /// tracked train has a journey, so this is always empty.
    async fn standalone_listed(pool: &PgPool, user_id: &str) -> Vec<i64> {
        crate::data::train_tracking::list_tracked_trains_for_user(pool, user_id)
            .await
            .expect("list tracked trains")
            .into_iter()
            .filter(|row| row.journey_ids.is_empty())
            .map(|row| row.id)
            .collect()
    }

    /// `singleLegJourneyIds` of `tracking_id`'s `GET /Train/mine` row: the
    /// train's own journeys, which "Stop tracking" deletes. `None` when
    /// the train is not listed.
    async fn own_journeys_listed(
        pool: &PgPool,
        user_id: &str,
        tracking_id: i64,
    ) -> Option<Vec<i64>> {
        crate::data::train_tracking::list_tracked_trains_for_user(pool, user_id)
            .await
            .expect("list tracked trains")
            .into_iter()
            .find(|row| row.id == tracking_id)
            .map(|row| row.single_leg_journey_ids)
    }

    /// "Track this train" on the web (`POST /Journeys`, a `knownTrain`
    /// leg) on the fixture date: a one-leg journey of the train's own,
    /// sharing the user's row for the train. Returns `(journey_id,
    /// tracking_id)`.
    async fn track_on_its_own(pool: &PgPool, trains_id: i64, user_id: &str) -> (i64, i64) {
        let (journey_id, _, tracking_id) = create_journey_with_known_train_leg(
            pool,
            user_id,
            None,
            trains_id,
            "2026-09-22".parse().unwrap(),
            None,
            None,
        )
        .await
        .expect("track the train on its own");
        (journey_id, tracking_id)
    }

    /// What [`change_train_fixture`] made.
    #[expect(
        clippy::struct_field_names,
        reason = "every field is a row id; the suffix says which kind"
    )]
    struct ChangedTrain {
        journey_id: i64,
        leg_id: i64,
        old_trains_id: i64,
        old_tracking_id: i64,
        new_trains_id: i64,
        new_tracking_id: i64,
    }

    /// A window leg bound to `old_uid`'s train, then "Change train" onto
    /// `new_uid`'s, both through the route's own data function
    /// ([`create_subscription_and_bind_leg`]). `before_change` gets the old
    /// pick's `(trains_id, tracking_id)` and runs between the two picks.
    async fn change_train_fixture<F, Fut>(
        pool: &PgPool,
        user_id: &str,
        old_uid: &str,
        new_uid: &str,
        before_change: F,
    ) -> ChangedTrain
    where
        F: FnOnce(i64, i64) -> Fut,
        Fut: Future<Output = ()>,
    {
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();
        let (journey_id, leg_id) = create_journey_with_window_leg(
            pool,
            user_id,
            None,
            "WAT",
            "RDG",
            service_date,
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");
        let old_trains_id = crate::data::trains::find_or_create_train(pool, old_uid, service_date)
            .await
            .expect("seed the first train");
        let new_trains_id = crate::data::trains::find_or_create_train(pool, new_uid, service_date)
            .await
            .expect("seed the second train");
        let old_tracking_id =
            create_subscription_and_bind_leg(pool, journey_id, leg_id, user_id, old_trains_id)
                .await
                .expect("first pick")
                .expect("leg is the caller's");
        before_change(old_trains_id, old_tracking_id).await;
        let new_tracking_id =
            create_subscription_and_bind_leg(pool, journey_id, leg_id, user_id, new_trains_id)
                .await
                .expect("change train")
                .expect("leg is the caller's");
        ChangedTrain {
            journey_id,
            leg_id,
            old_trains_id,
            old_tracking_id,
            new_trains_id,
            new_tracking_id,
        }
    }

    /// DS-93: "Change train" used to leave the leg's old subscription
    /// behind, disabled but listed in `GET /Train/mine` as a train tracked
    /// on its own. A subscription only the leg used is now deleted, its
    /// ticket and group share move to the new one, and only the new train
    /// pushes.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                change_train_ -- --ignored --test-threads=1`"]
    async fn change_train_leaves_no_orphan_and_moves_what_was_attached() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-CHANGE-TRAIN-ORPHAN";
        let group_id = "TEST-DS93-CHANGE-TRAIN-GROUP";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(&pool)
            .await
            .expect("clear the fixture group");
        sqlx::query("INSERT INTO groups (id, name, created_by) VALUES ($1, 'DS-93', $2)")
            .bind(group_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("seed group");

        let pool_ref = &pool;
        let changed = change_train_fixture(
            &pool,
            user_id,
            "TEST-TRAIN-DS93-ORPHAN-OLD",
            "TEST-TRAIN-DS93-ORPHAN-NEW",
            |_, old_tracking_id| async move {
                sqlx::query(
                    "INSERT INTO tracked_train_tickets (tracked_train_id, user_id, operator) \
                     VALUES ($1, $2, 'DS93')",
                )
                .bind(old_tracking_id)
                .bind(user_id)
                .execute(pool_ref)
                .await
                .expect("attach a ticket to the old pick");
                sqlx::query(
                    "INSERT INTO group_trains (group_id, train_subscription_id, added_by) \
                     VALUES ($1, $2, $3)",
                )
                .bind(group_id)
                .bind(old_tracking_id)
                .bind(user_id)
                .execute(pool_ref)
                .await
                .expect("share the old pick into a group");
            },
        )
        .await;

        let leg = get_owned_leg(&pool, changed.journey_id, changed.leg_id, user_id)
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, Some(changed.new_tracking_id));
        assert!(
            !subscription_exists(&pool, changed.old_tracking_id).await,
            "the leg-only old subscription must be deleted"
        );
        assert!(
            standalone_listed(&pool, user_id).await.is_empty(),
            "GET /Train/mine must not show the old train as tracked on its own"
        );
        let tickets_on: Vec<Option<i64>> = sqlx::query_scalar(
            "SELECT tracked_train_id FROM tracked_train_tickets WHERE user_id = $1",
        )
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .expect("read tickets");
        assert_eq!(
            tickets_on,
            vec![Some(changed.new_tracking_id)],
            "the ticket follows the leg"
        );
        let shared: Vec<i64> = sqlx::query_scalar(
            "SELECT train_subscription_id FROM group_trains WHERE group_id = $1",
        )
        .bind(group_id)
        .fetch_all(&pool)
        .await
        .expect("read group shares");
        assert_eq!(
            shared,
            vec![changed.new_tracking_id],
            "the group share follows the leg"
        );
        assert_eq!(
            pushed_subscription(&pool, changed.old_trains_id, user_id).await,
            None,
            "the old train no longer pushes"
        );
        assert_eq!(
            pushed_subscription(&pool, changed.new_trains_id, user_id).await,
            Some(changed.new_tracking_id),
            "the new train pushes, on the leg's subscription"
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(&pool)
            .await
            .expect("clear the fixture group");
        cleanup_user(&pool, user_id).await;
    }

    /// DS-93, DS-124: a train the user also tracks on its own ("Track this
    /// train", a one-leg journey of its own sharing the leg's row) survives
    /// the leg's "Change train", still listed under its own journey and
    /// still pushing; the new train pushes on the leg's subscription.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                change_train_ -- --ignored --test-threads=1`"]
    async fn change_train_keeps_a_train_also_tracked_on_its_own() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-CHANGE-TRAIN-KEEP";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;

        let pool_ref = &pool;
        let changed = change_train_fixture(
            &pool,
            user_id,
            "TEST-TRAIN-DS93-KEEP-OLD",
            "TEST-TRAIN-DS93-KEEP-NEW",
            |old_trains_id, old_tracking_id| async move {
                let (_, standalone) = track_on_its_own(pool_ref, old_trains_id, user_id).await;
                assert_eq!(standalone, old_tracking_id, "one row serves both");
            },
        )
        .await;

        assert!(subscription_exists(&pool, changed.old_tracking_id).await);
        assert!(notifications_enabled_of(&pool, changed.old_tracking_id).await);
        assert!(standalone_listed(&pool, user_id).await.is_empty());
        assert_eq!(
            own_journeys_listed(&pool, user_id, changed.old_tracking_id)
                .await
                .map(|journeys| journeys.len()),
            Some(1),
            "the train tracked on its own stays in GET /Train/mine, under its own journey"
        );
        assert_eq!(
            pushed_subscription(&pool, changed.old_trains_id, user_id).await,
            Some(changed.old_tracking_id),
            "the train tracked on its own still pushes"
        );
        assert_eq!(
            pushed_subscription(&pool, changed.new_trains_id, user_id).await,
            Some(changed.new_tracking_id),
        );

        cleanup_user(&pool, user_id).await;
    }

    /// DS-93: a pin tracked on its own (a one-leg journey of its own since
    /// DS-124) that resolved to the leg's train is a second row (the dedup
    /// case of `TrackedTrainListItem::journey_ids` and the notifier's one
    /// push per user per train). "Change train" deletes the leg's own row
    /// and leaves the pin, which keeps its journey and takes over that
    /// train's push.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                change_train_ -- --ignored --test-threads=1`"]
    async fn change_train_keeps_a_separate_pin_on_the_old_train() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-CHANGE-TRAIN-PIN";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;

        let (pin_journey, _, pin) =
            create_journey_with_pin_leg(&pool, user_id, None, &fixture_pin("WAT"))
                .await
                .expect("track a pin on its own");
        let pool_ref = &pool;
        let changed = change_train_fixture(
            &pool,
            user_id,
            "TEST-TRAIN-DS93-PIN-OLD",
            "TEST-TRAIN-DS93-PIN-NEW",
            |old_trains_id, _| async move {
                sqlx::query(
                    "UPDATE train_subscriptions SET trains_id = $1, \
                     resolution_status = 'resolved' WHERE id = $2",
                )
                .bind(old_trains_id)
                .bind(pin)
                .execute(pool_ref)
                .await
                .expect("resolve the pin to the leg's train");
            },
        )
        .await;

        assert!(!subscription_exists(&pool, changed.old_tracking_id).await);
        assert!(standalone_listed(&pool, user_id).await.is_empty());
        assert_eq!(
            own_journeys_listed(&pool, user_id, pin).await,
            Some(vec![pin_journey])
        );
        assert_eq!(
            pushed_subscription(&pool, changed.old_trains_id, user_id).await,
            Some(pin)
        );
        assert_eq!(
            pushed_subscription(&pool, changed.new_trains_id, user_id).await,
            Some(changed.new_tracking_id)
        );

        cleanup_user(&pool, user_id).await;
    }

    /// A window leg in a new journey of its own, bound to `uid`'s train on
    /// the fixture date through the route's own data function. Returns
    /// `(journey_id, leg_id, tracking_id)`.
    async fn bound_leg(pool: &PgPool, user_id: &str, uid: &str) -> (i64, i64, i64) {
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();
        let (journey_id, leg_id) = create_journey_with_window_leg(
            pool,
            user_id,
            None,
            "WAT",
            "RDG",
            service_date,
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");
        let trains_id = crate::data::trains::find_or_create_train(pool, uid, service_date)
            .await
            .expect("seed train");
        let tracking_id =
            create_subscription_and_bind_leg(pool, journey_id, leg_id, user_id, trains_id)
                .await
                .expect("pick the train")
                .expect("leg is the caller's");
        (journey_id, leg_id, tracking_id)
    }

    /// DS-93: removing a leg drops a subscription only that leg used, with
    /// its ticket (the same cascade as `DELETE /Train/{trackingId}`), so it
    /// does not reappear as a train tracked on its own. A row another leg
    /// still uses stays until that leg goes too.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_leg_ -- --ignored --test-threads=1`"]
    async fn delete_leg_drops_a_leg_only_subscription() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELETE-LEG-ORPHAN";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;

        let (journey_a, leg_a, tracking_id) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DELETE-LEG").await;
        let (journey_b, leg_b, shared) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DELETE-LEG").await;
        assert_eq!(shared, tracking_id, "both legs share the user's row");
        sqlx::query(
            "INSERT INTO tracked_train_tickets (tracked_train_id, user_id) VALUES ($1, $2)",
        )
        .bind(tracking_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("attach a ticket");

        assert_eq!(
            delete_leg(&pool, journey_a, leg_a, user_id)
                .await
                .expect("remove the first leg"),
            Some(true),
            "its journey's only leg, so the journey goes too"
        );
        assert!(
            subscription_exists(&pool, tracking_id).await,
            "the other leg still uses it"
        );

        assert_eq!(
            delete_leg(&pool, journey_b, leg_b, user_id)
                .await
                .expect("remove the second leg"),
            Some(true)
        );
        assert!(
            !subscription_exists(&pool, tracking_id).await,
            "no leg uses it and it was never tracked on its own"
        );
        assert!(standalone_listed(&pool, user_id).await.is_empty());
        let tickets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tracked_train_tickets WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .expect("count tickets");
        assert_eq!(tickets, 0, "the ticket goes with the row");

        cleanup_user(&pool, user_id).await;
    }

    /// DS-93: removing a leg keeps a train the user also tracks on its own,
    /// still pushing, under its own journey (DS-124).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_leg_ -- --ignored --test-threads=1`"]
    async fn delete_leg_keeps_a_train_also_tracked_on_its_own() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELETE-LEG-KEEP";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;

        let (journey_id, leg_id, tracking_id) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DELETE-LEG-KEEP").await;
        let trains_id: i64 =
            sqlx::query_scalar("SELECT trains_id FROM train_subscriptions WHERE id = $1")
                .bind(tracking_id)
                .fetch_one(&pool)
                .await
                .expect("read trains_id");
        let (own_journey, standalone) = track_on_its_own(&pool, trains_id, user_id).await;
        assert_eq!(standalone, tracking_id);

        delete_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("remove the leg")
            .expect("leg is the caller's");

        assert!(notifications_enabled_of(&pool, tracking_id).await);
        assert!(standalone_listed(&pool, user_id).await.is_empty());
        assert_eq!(
            own_journeys_listed(&pool, user_id, tracking_id).await,
            Some(vec![own_journey])
        );
        assert_eq!(
            pushed_subscription(&pool, trains_id, user_id).await,
            Some(tracking_id)
        );

        cleanup_user(&pool, user_id).await;
    }

    /// DS-124: "Stop tracking" (`DELETE /Train/{trackingId}`) deletes the
    /// train's own journey, but a multi-leg journey's leg keeps the train.
    /// Removing that leg afterwards then drops it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_leg_ -- --ignored --test-threads=1`"]
    async fn delete_leg_after_stop_tracking_drops_the_shared_train() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELETE-LEG-UNTRACK";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;

        let (journey_id, leg_id, tracking_id) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DELETE-LEG-UNTRACK").await;
        add_window_leg(&pool, journey_id, user_id).await;
        let trains_id: i64 =
            sqlx::query_scalar("SELECT trains_id FROM train_subscriptions WHERE id = $1")
                .bind(tracking_id)
                .fetch_one(&pool)
                .await
                .expect("read trains_id");
        let (own_journey, _) = track_on_its_own(&pool, trains_id, user_id).await;

        assert!(
            stop_tracking_train(&pool, tracking_id, user_id)
                .await
                .expect("stop tracking it")
        );
        assert!(
            get_journey_summary(&pool, own_journey)
                .await
                .expect("read journey")
                .is_none(),
            "the train's own journey goes"
        );
        let leg = get_owned_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(
            leg.train_subscription_id,
            Some(tracking_id),
            "the two-leg journey's leg keeps it"
        );
        assert_eq!(leg.match_mode, "manual");
        assert_eq!(
            own_journeys_listed(&pool, user_id, tracking_id).await,
            Some(vec![]),
            "listed under the two-leg journey only"
        );
        assert_eq!(
            pushed_subscription(&pool, trains_id, user_id).await,
            Some(tracking_id),
            "the leg's train still pushes"
        );

        delete_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("remove the leg")
            .expect("leg is the caller's");
        assert!(!subscription_exists(&pool, tracking_id).await);
        assert_eq!(pushed_subscription(&pool, trains_id, user_id).await, None);

        cleanup_user(&pool, user_id).await;
    }

    /// DS-124: "Stop tracking" a train tracked on its own deletes its
    /// journey and the train, with the journey's group share, the train's
    /// ticket and a remaining `group_trains` share. Someone else's train
    /// is a `false` (the route's 404). A row no leg uses (an older
    /// binary's, written during a rollout) is deleted outright.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                stop_tracking_ -- --ignored --test-threads=1`"]
    async fn stop_tracking_deletes_the_trains_own_journey() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS124-STOP-TRACKING";
        let other_id = "TEST-DS124-STOP-TRACKING-OTHER";
        let group_id = "TEST-DS124-STOP-TRACKING-GROUP";
        for user in [user_id, other_id] {
            cleanup_user(&pool, user).await;
            seed_user(&pool, user).await;
        }
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(&pool)
            .await
            .expect("clear the fixture group");
        sqlx::query("INSERT INTO groups (id, name, created_by) VALUES ($1, 'DS-124', $2)")
            .bind(group_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("seed group");

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-TRAIN-DS124-STOP",
            "2026-09-22".parse().unwrap(),
        )
        .await
        .expect("seed train");
        let (journey_id, tracking_id) = track_on_its_own(&pool, trains_id, user_id).await;
        sqlx::query(
            "INSERT INTO group_journeys (group_id, journey_id, added_by) VALUES ($1, $2, $3)",
        )
        .bind(group_id)
        .bind(journey_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("share the journey");
        sqlx::query(
            "INSERT INTO group_trains (group_id, train_subscription_id, added_by) \
             VALUES ($1, $2, $3)",
        )
        .bind(group_id)
        .bind(tracking_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("share the train the old way");
        sqlx::query(
            "INSERT INTO tracked_train_tickets (tracked_train_id, user_id) VALUES ($1, $2)",
        )
        .bind(tracking_id)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("attach a ticket");

        assert!(
            !stop_tracking_train(&pool, tracking_id, other_id)
                .await
                .expect("someone else's stop")
        );
        assert!(subscription_exists(&pool, tracking_id).await);

        assert!(
            stop_tracking_train(&pool, tracking_id, user_id)
                .await
                .expect("stop tracking")
        );
        assert!(!subscription_exists(&pool, tracking_id).await);
        assert!(
            get_journey_summary(&pool, journey_id)
                .await
                .expect("read journey")
                .is_none()
        );
        for (table, column) in [
            ("group_journeys", "group_id"),
            ("group_trains", "group_id"),
            ("tracked_train_tickets", "user_id"),
        ] {
            let left: i64 =
                sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE {column} = $1"))
                    .bind(if column == "group_id" {
                        group_id
                    } else {
                        user_id
                    })
                    .fetch_one(&pool)
                    .await
                    .expect("count leftovers");
            assert_eq!(left, 0, "{table} rows go with the train");
        }

        let legless =
            crate::data::train_tracking::create_subscription_for_train(&pool, trains_id, user_id)
                .await
                .expect("a row no leg uses");
        assert!(
            stop_tracking_train(&pool, legless, user_id)
                .await
                .expect("stop tracking the legless row")
        );
        assert!(!subscription_exists(&pool, legless).await);

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(&pool)
            .await
            .expect("clear the fixture group");
        for user in [user_id, other_id] {
            cleanup_user(&pool, user).await;
        }
    }

    /// DS-124: the old `POST /Train/by-uid/.../track` makes a one-leg
    /// journey, and on a train one of the user's journeys already has,
    /// hands that subscription and journey back without a second journey.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                track_known_train_on_its_own -- --ignored --test-threads=1`"]
    async fn track_known_train_on_its_own_makes_one_journey() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS124-TRACK-ON-ITS-OWN";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();
        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-TRAIN-DS124-OWN", service_date)
                .await
                .expect("seed train");

        let first = track_known_train_on_its_own(&pool, user_id, trains_id, service_date)
            .await
            .expect("track");
        assert!(first.created);
        let legs = list_legs_for_journey(&pool, first.journey_id)
            .await
            .expect("list legs");
        assert_eq!(legs.len(), 1);
        assert_eq!(legs[0].train_subscription_id, Some(first.tracking_id));
        assert_eq!(legs[0].match_mode, "manual");

        let again = track_known_train_on_its_own(&pool, user_id, trains_id, service_date)
            .await
            .expect("track again");
        assert_eq!(
            again,
            TrainTrackedOnItsOwn {
                created: false,
                ..first
            }
        );
        let journeys: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM journeys WHERE user_id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .expect("count journeys");
        assert_eq!(journeys, 1, "no second journey for the same train");

        // Renaming the train renames its own journey.
        assert!(
            crate::data::train_tracking::rename_tracked_train(
                &pool,
                first.tracking_id,
                user_id,
                Some("Commute")
            )
            .await
            .expect("rename the train")
        );
        let summary = get_journey_summary(&pool, first.journey_id)
            .await
            .expect("read journey")
            .expect("journey exists");
        assert_eq!(summary.custom_name.as_deref(), Some("Commute"));

        cleanup_user(&pool, user_id).await;
    }

    /// DS-124, migration `20261022090000`: a subscription no leg uses
    /// becomes a one-leg journey of its own (name, ends, date, bound,
    /// `manual`), its `group_trains` share moves onto that journey, a share
    /// of a train only a multi-leg journey uses stays, and a rerun changes
    /// nothing. Then `scripts/rollback-tracked-trains-as-journeys.sql`
    /// puts it all back. Runs as the schema owner (the migrator's role),
    /// in a transaction it rolls back.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                tracked_trains_as_journeys_migration -- --ignored --test-threads=1`"]
    async fn tracked_trains_as_journeys_migration_converts_and_rolls_back() {
        const MIGRATION: &str = include_str!(
            "../../../ds-store/migrations/20261022090000_tracked_trains_as_journeys.sql"
        );
        const ROLLBACK: &str =
            include_str!("../../../../scripts/rollback-tracked-trains-as-journeys.sql");
        let pool = connect().await;
        let owner = ds_store::test_support::schedule_fixture_pool(&pool).await;
        let mut tx = owner.begin().await.expect("begin");
        let user_id = "TEST-DS124-MIGRATION";
        let group_id = "TEST-DS124-MIGRATION-GROUP";
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        sqlx::query("INSERT INTO users (id, name) VALUES ($1, $1)")
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .expect("seed user");
        sqlx::query("INSERT INTO groups (id, name, created_by) VALUES ($1, 'DS-124', $2)")
            .bind(group_id)
            .bind(user_id)
            .execute(&mut *tx)
            .await
            .expect("seed group");
        let mut subscriptions = Vec::new();
        for (uid, name) in [
            ("TEST-TRAIN-DS124-MIG-ALONE", Some("Commute")),
            ("TEST-TRAIN-DS124-MIG-SHARED", None),
            ("TEST-TRAIN-DS124-MIG-LEG", None),
        ] {
            let id: i64 = sqlx::query_scalar(
                "WITH tr AS ( \
                     INSERT INTO trains (train_uid, service_date, origin_crs, destination_crs) \
                     VALUES ($1, $2, 'WAT', 'RDG') RETURNING id) \
                 INSERT INTO train_subscriptions \
                     (user_id, trains_id, service_date, pin_destination_crs, custom_name, \
                      tracked_standalone, resolution_status) \
                 SELECT $3, tr.id, $2, 'GLD', $4, TRUE, 'resolved' FROM tr RETURNING id",
            )
            .bind(uid)
            .bind(service_date)
            .bind(user_id)
            .bind(name)
            .fetch_one(&mut *tx)
            .await
            .expect("seed subscription");
            subscriptions.push(id);
        }
        let [alone, shared, on_leg] = subscriptions[..] else {
            unreachable!("three subscriptions seeded");
        };
        let two_leg_journey: i64 =
            sqlx::query_scalar("INSERT INTO journeys (user_id) VALUES ($1) RETURNING id")
                .bind(user_id)
                .fetch_one(&mut *tx)
                .await
                .expect("seed journey");
        sqlx::query(
            "INSERT INTO journey_legs \
                 (journey_id, leg_order, service_date, train_subscription_id, match_mode) \
             VALUES ($1, 1, $2, $3, 'manual'), ($1, 2, $2, NULL, 'unmatched')",
        )
        .bind(two_leg_journey)
        .bind(service_date)
        .bind(on_leg)
        .execute(&mut *tx)
        .await
        .expect("seed legs");
        sqlx::query(
            "INSERT INTO group_trains (group_id, train_subscription_id, added_by, added_at) \
             VALUES ($1, $2, $3, '2026-09-20T10:00:00Z'), ($1, $4, $3, '2026-09-20T11:00:00Z')",
        )
        .bind(group_id)
        .bind(shared)
        .bind(user_id)
        .bind(on_leg)
        .execute(&mut *tx)
        .await
        .expect("seed shares");

        sqlx::raw_sql(MIGRATION)
            .execute(&mut *tx)
            .await
            .expect("run the migration");

        let own_journey = |tracking_id: i64| {
            sqlx::query_as::<
                _,
                (
                    i64,
                    Option<String>,
                    Option<String>,
                    Option<String>,
                    NaiveDate,
                    String,
                ),
            >(
                "SELECT j.id, j.custom_name, jl.origin_crs, jl.destination_crs, jl.service_date, \
                        jl.match_mode \
                 FROM journeys j JOIN journey_legs jl ON jl.journey_id = j.id \
                 WHERE jl.train_subscription_id = $1 \
                   AND (SELECT COUNT(*) FROM journey_legs o WHERE o.journey_id = j.id) = 1",
            )
            .bind(tracking_id)
        };
        let (alone_journey, name, origin, destination, date, mode) = own_journey(alone)
            .fetch_one(&mut *tx)
            .await
            .expect("the lone train has a journey of its own");
        assert_eq!(name.as_deref(), Some("Commute"));
        assert_eq!(origin.as_deref(), Some("WAT"), "the train's origin");
        assert_eq!(
            destination.as_deref(),
            Some("GLD"),
            "the pinned destination"
        );
        assert_eq!((date, mode.as_str()), (service_date, "manual"));
        let (shared_journey, ..) = own_journey(shared)
            .fetch_one(&mut *tx)
            .await
            .expect("the shared train has a journey of its own");
        assert!(
            own_journey(on_leg)
                .fetch_optional(&mut *tx)
                .await
                .expect("read")
                .is_none(),
            "a train a leg uses gets no second journey"
        );
        let journey_shares: Vec<(i64, DateTime<Utc>)> =
            sqlx::query_as("SELECT journey_id, added_at FROM group_journeys WHERE group_id = $1")
                .bind(group_id)
                .fetch_all(&mut *tx)
                .await
                .expect("read journey shares");
        assert_eq!(
            journey_shares,
            vec![(shared_journey, "2026-09-20T10:00:00Z".parse().unwrap())],
            "the share moved onto the train's journey, keeping when it was made"
        );
        let train_shares: Vec<i64> = sqlx::query_scalar(
            "SELECT train_subscription_id FROM group_trains WHERE group_id = $1",
        )
        .bind(group_id)
        .fetch_all(&mut *tx)
        .await
        .expect("read train shares");
        assert_eq!(
            train_shares,
            vec![on_leg],
            "a multi-leg journey's train stays shared"
        );

        async fn journeys_after(tx: &mut sqlx::PgConnection) -> i64 {
            sqlx::query_scalar("SELECT COUNT(*) FROM journeys WHERE user_id = $1")
                .bind("TEST-DS124-MIGRATION")
                .fetch_one(tx)
                .await
                .expect("count journeys")
        }
        assert_eq!(journeys_after(&mut tx).await, 3);
        sqlx::raw_sql(MIGRATION)
            .execute(&mut *tx)
            .await
            .expect("rerun the migration");
        assert_eq!(
            journeys_after(&mut tx).await,
            3,
            "a rerun converts nothing twice"
        );

        sqlx::raw_sql(ROLLBACK)
            .execute(&mut *tx)
            .await
            .expect("run the rollback");
        assert_eq!(
            journeys_after(&mut tx).await,
            1,
            "only the two-leg journey is left"
        );
        assert!(
            sqlx::query_scalar::<_, i64>("SELECT id FROM journeys WHERE id = ANY($1)")
                .bind(vec![alone_journey, shared_journey])
                .fetch_optional(&mut *tx)
                .await
                .expect("read")
                .is_none()
        );
        let mut train_shares: Vec<i64> = sqlx::query_scalar(
            "SELECT train_subscription_id FROM group_trains WHERE group_id = $1",
        )
        .bind(group_id)
        .fetch_all(&mut *tx)
        .await
        .expect("read train shares");
        train_shares.sort_unstable();
        assert_eq!(
            train_shares,
            vec![shared, on_leg],
            "the share is back on the train"
        );
        let journey_shares: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM group_journeys WHERE group_id = $1")
                .bind(group_id)
                .fetch_one(&mut *tx)
                .await
                .expect("count journey shares");
        assert_eq!(journey_shares, 0);
        let standalone: Vec<bool> = sqlx::query_scalar(
            "SELECT tracked_standalone FROM train_subscriptions WHERE id = ANY($1) ORDER BY id",
        )
        .bind(vec![alone, shared])
        .fetch_all(&mut *tx)
        .await
        .expect("read flags");
        assert_eq!(
            standalone,
            vec![true, true],
            "the subscriptions stay as they were"
        );

        tx.rollback().await.expect("roll back the fixture");
    }

    /// DS-93: deleting a whole journey releases every leg's train as
    /// "Remove leg" does. Here: a two-leg journey whose first train only it
    /// uses (with a ticket and a group share), whose second train another
    /// journey's leg also uses, plus a third journey on a train the user
    /// also tracks on its own.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_journey_releases -- --ignored --test-threads=1`"]
    async fn delete_journey_releases_its_legs_trains() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELETE-JOURNEY";
        let group_id = "TEST-DS93-DELETE-JOURNEY-GROUP";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(&pool)
            .await
            .expect("clear the fixture group");
        sqlx::query("INSERT INTO groups (id, name, created_by) VALUES ($1, 'DS-93', $2)")
            .bind(group_id)
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("seed group");

        // Journey A: leg 1 on a train only it uses, leg 2 (moved in from a
        // journey of its own) on a train journey B's leg also uses.
        let (journey_a, _, only_a) = bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DJ-ONLY").await;
        let (scratch, scratch_leg, shared) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DJ-SHARED").await;
        sqlx::query("UPDATE journey_legs SET journey_id = $1, leg_order = 2 WHERE id = $2")
            .bind(journey_a)
            .bind(scratch_leg)
            .execute(&pool)
            .await
            .expect("move the second leg into journey A");
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(scratch)
            .execute(&pool)
            .await
            .expect("drop the emptied journey");
        let (journey_b, leg_b, shared_b) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DJ-SHARED").await;
        assert_eq!(shared_b, shared, "journey B's leg shares the row");
        sqlx::query(
            "INSERT INTO tracked_train_tickets (tracked_train_id, user_id) VALUES ($1, $2)",
        )
        .bind(only_a)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("attach a ticket");
        sqlx::query(
            "INSERT INTO group_trains (group_id, train_subscription_id, added_by) \
             VALUES ($1, $2, $3)",
        )
        .bind(group_id)
        .bind(only_a)
        .bind(user_id)
        .execute(&pool)
        .await
        .expect("share it into a group");

        // Journey C: a train the user also tracks on its own.
        let (journey_c, _, standalone) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DJ-STANDALONE").await;
        let standalone_trains_id: i64 =
            sqlx::query_scalar("SELECT trains_id FROM train_subscriptions WHERE id = $1")
                .bind(standalone)
                .fetch_one(&pool)
                .await
                .expect("read trains_id");
        let (own_journey, _) = track_on_its_own(&pool, standalone_trains_id, user_id).await;

        assert!(
            delete_journey(&pool, journey_a, user_id)
                .await
                .expect("delete journey A")
        );
        assert!(
            !subscription_exists(&pool, only_a).await,
            "a train only journey A used goes"
        );
        let tickets: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM tracked_train_tickets WHERE user_id = $1")
                .bind(user_id)
                .fetch_one(&pool)
                .await
                .expect("count tickets");
        assert_eq!(tickets, 0, "its ticket goes with it");
        let group_shares: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM group_trains WHERE group_id = $1")
                .bind(group_id)
                .fetch_one(&pool)
                .await
                .expect("count shares");
        assert_eq!(group_shares, 0, "its group share goes with it");
        assert!(
            subscription_exists(&pool, shared).await,
            "journey B's leg still uses the shared train"
        );
        let leg = get_owned_leg(&pool, journey_b, leg_b, user_id)
            .await
            .expect("read leg")
            .expect("journey B's leg exists");
        assert_eq!(leg.train_subscription_id, Some(shared));

        assert!(
            delete_journey(&pool, journey_c, user_id)
                .await
                .expect("delete journey C")
        );
        assert!(notifications_enabled_of(&pool, standalone).await);
        assert!(standalone_listed(&pool, user_id).await.is_empty());
        assert_eq!(
            own_journeys_listed(&pool, user_id, standalone).await,
            Some(vec![own_journey])
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(group_id)
            .execute(&pool)
            .await
            .expect("clear the fixture group");
        cleanup_user(&pool, user_id).await;
    }

    /// DS-93: a delegated client's journey delete releases the legs' trains
    /// too.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_journey_releases -- --ignored --test-threads=1`"]
    async fn delegated_delete_journey_releases_its_legs_trains() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELEGATED-DELETE";
        let client = "ds93-test";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let (journey_id, _, tracking_id) =
            bound_leg(&pool, user_id, "TEST-TRAIN-DS93-DELEGATED").await;
        sqlx::query("UPDATE journeys SET created_via = $1 WHERE id = $2")
            .bind(client)
            .bind(journey_id)
            .execute(&pool)
            .await
            .expect("saved by the client");
        let grant_id: i64 = sqlx::query_scalar(
            "INSERT INTO delegated_grants (user_id, client, client_display_name, scopes) \
             VALUES ($1, $2, 'DS-93 test', ARRAY['journeys:delete']) RETURNING id",
        )
        .bind(user_id)
        .bind(client)
        .fetch_one(&pool)
        .await
        .expect("seed grant");

        let outcome = crate::data::delegated::delete_journey_delegated(
            &pool, journey_id, user_id, client, grant_id, 10,
        )
        .await
        .expect("delegated delete");
        assert!(matches!(
            outcome,
            crate::data::delegated::DelegatedDeleteOutcome::Deleted
        ));
        assert!(!subscription_exists(&pool, tracking_id).await);
        assert!(standalone_listed(&pool, user_id).await.is_empty());

        cleanup_user(&pool, user_id).await;
    }

    /// A window leg added to `journey_id` (WAT to RDG on the fixture date,
    /// after 08:00), unbound.
    async fn add_window_leg(pool: &PgPool, journey_id: i64, user_id: &str) -> i64 {
        match add_window_leg_to_journey(
            pool,
            journey_id,
            user_id,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("add window leg")
        {
            AddLegOutcome::Added(leg_id) => leg_id,
            _ => panic!("the journey is the caller's and under the leg cap"),
        }
    }

    /// The notifier's auto-match (`notifier::queries::auto_commit_leg_to_train`)
    /// as SQL on `conn`: the user's row for `trains_id`, bound to the leg
    /// with `match_mode = 'auto'`. Returns the subscription id.
    async fn auto_bind_leg(
        conn: &mut sqlx::PgConnection,
        leg_id: i64,
        trains_id: i64,
        user_id: &str,
    ) -> i64 {
        let tracking_id = crate::data::train_tracking::create_subscription_for_train(
            &mut *conn, trains_id, user_id,
        )
        .await
        .expect("subscribe as the auto-match does");
        sqlx::query(
            "UPDATE journey_legs SET train_subscription_id = $1, match_mode = 'auto' \
             WHERE id = $2 AND match_mode = 'unmatched'",
        )
        .bind(tracking_id)
        .bind(leg_id)
        .execute(&mut *conn)
        .await
        .expect("bind the leg as the auto-match does");
        tracking_id
    }

    /// DS-93, the production report: deleting a journey of many legs
    /// untracks every leg's train, not only the first or current leg's. One
    /// journey with every kind of leg: planner legs (two sharing one
    /// train's row), an untracked leg, a leg on a CRS+time pin with no train
    /// yet, a pin resolved to the same train as a planner leg (a second row
    /// for that train), an auto-matched leg, an unmatched window leg, and a
    /// leg on a train the user also tracks on its own. Only that last train
    /// is left, listed on its own and still pushing.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_journey_releases -- --ignored --test-threads=1`"]
    async fn delete_journey_releases_every_legs_train_of_a_many_leg_journey() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELETE-MANY-LEGS";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();
        let mut trains = Vec::new();
        for uid in [
            "TEST-TRAIN-DS93-MANY-FIRST",
            "TEST-TRAIN-DS93-MANY-SECOND",
            "TEST-TRAIN-DS93-MANY-OWN",
            "TEST-TRAIN-DS93-MANY-AUTO",
        ] {
            trains.push(
                crate::data::trains::find_or_create_train(&pool, uid, service_date)
                    .await
                    .expect("seed train"),
            );
        }
        let [first, second, own, auto] = trains[..] else {
            unreachable!("four trains seeded");
        };
        let train_leg = |trains_id| ItineraryLeg::Train {
            trains_id,
            service_date,
            origin_crs: None,
            destination_crs: None,
        };
        let untracked = ItineraryLeg::Untracked(UntrackedLeg {
            transit_mode: "bus".to_string(),
            origin_place: Some("Stop A".to_string()),
            destination_place: Some("Stop B".to_string()),
            line_name: None,
            planned_departure: "2026-09-22T10:00:00Z".parse().unwrap(),
            planned_arrival: "2026-09-22T10:30:00Z".parse().unwrap(),
            origin_stop_id: None,
            destination_stop_id: None,
        });

        let mut tx = pool.begin().await.expect("begin");
        let (journey_id, created) = create_journey_from_itinerary(
            &mut tx,
            user_id,
            None,
            "web",
            &[
                train_leg(first),
                untracked,
                train_leg(second),
                train_leg(first),
                train_leg(own),
            ],
        )
        .await
        .expect("save the itinerary");
        tx.commit().await.expect("commit");
        let tracking: Vec<Option<i64>> = created.iter().map(|(_, t)| *t).collect();
        let [
            Some(first_sub),
            None,
            Some(second_sub),
            Some(first_again),
            Some(own_sub),
        ] = tracking[..]
        else {
            panic!("five legs, the second untracked: {tracking:?}");
        };
        assert_eq!(
            first_again, first_sub,
            "two legs share the first train's row"
        );
        let (own_journey, own_again) = track_on_its_own(&pool, own, user_id).await;
        assert_eq!(own_again, own_sub, "the train's own journey shares the row");

        let pending_pin_leg = add_window_leg(&pool, journey_id, user_id).await;
        let pending_pin =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("WAT"), user_id)
                .await
                .expect("pin with no train yet");
        assert!(
            set_leg_train_subscription(&pool, journey_id, pending_pin_leg, user_id, pending_pin)
                .await
                .expect("bind the pin")
        );
        let resolved_pin_leg = add_window_leg(&pool, journey_id, user_id).await;
        let resolved_pin =
            crate::data::train_tracking::create_pin(&pool, &fixture_pin("RDG"), user_id)
                .await
                .expect("second pin");
        sqlx::query(
            "UPDATE train_subscriptions SET trains_id = $1, resolution_status = 'resolved' \
             WHERE id = $2",
        )
        .bind(second)
        .bind(resolved_pin)
        .execute(&pool)
        .await
        .expect("resolve the pin to the second planner leg's train");
        assert!(
            set_leg_train_subscription(&pool, journey_id, resolved_pin_leg, user_id, resolved_pin)
                .await
                .expect("bind the resolved pin")
        );
        let auto_leg = add_window_leg(&pool, journey_id, user_id).await;
        let mut conn = pool.acquire().await.expect("acquire");
        let auto_sub = auto_bind_leg(&mut conn, auto_leg, auto, user_id).await;
        drop(conn);
        add_window_leg(&pool, journey_id, user_id).await;

        let legs = list_legs_for_journey(&pool, journey_id)
            .await
            .expect("list legs");
        assert_eq!(legs.len(), 9);
        assert!(
            standalone_listed(&pool, user_id).await.is_empty(),
            "every train is folded into a journey before the delete"
        );

        assert!(
            delete_journey(&pool, journey_id, user_id)
                .await
                .expect("delete the journey")
        );
        for tracking_id in [first_sub, second_sub, pending_pin, resolved_pin, auto_sub] {
            assert!(
                !subscription_exists(&pool, tracking_id).await,
                "subscription {tracking_id}, used only by this journey, is released"
            );
        }
        assert!(standalone_listed(&pool, user_id).await.is_empty());
        assert_eq!(
            own_journeys_listed(&pool, user_id, own_sub).await,
            Some(vec![own_journey])
        );
        assert!(notifications_enabled_of(&pool, own_sub).await);
        assert_eq!(
            pushed_subscription(&pool, own, user_id).await,
            Some(own_sub)
        );
        for trains_id in [first, second, auto] {
            assert_eq!(pushed_subscription(&pool, trains_id, user_id).await, None);
        }

        cleanup_user(&pool, user_id).await;
    }

    /// Waits until another backend is blocked on a lock in a statement
    /// naming a journey table: the delete under test has reached the leg
    /// the racing transaction holds.
    async fn wait_for_a_lock_waiter(pool: &PgPool) {
        for _ in 0..200 {
            let waiting: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pg_stat_activity \
                 WHERE wait_event_type = 'Lock' AND query ILIKE '%journey%' \
                   AND pid <> pg_backend_pid()",
            )
            .fetch_one(pool)
            .await
            .expect("read pg_stat_activity");
            if waiting > 0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        panic!("the delete never waited on the leg the auto-match holds");
    }

    /// Seeds a journey with one bound leg and one unbound window leg, and
    /// starts the notifier's auto-match on the unbound one in a transaction
    /// left open. Returns `(journey_id, bound_tracking_id, racing_leg_id,
    /// auto_tracking_id, open_transaction)`.
    async fn racing_auto_match(
        pool: &PgPool,
        user_id: &str,
        uid_prefix: &str,
    ) -> (
        i64,
        i64,
        i64,
        i64,
        sqlx::Transaction<'static, sqlx::Postgres>,
    ) {
        cleanup_user(pool, user_id).await;
        seed_user(pool, user_id).await;
        let (journey_id, _, bound) = bound_leg(pool, user_id, &format!("{uid_prefix}-BOUND")).await;
        let racing_leg = add_window_leg(pool, journey_id, user_id).await;
        let trains_id = crate::data::trains::find_or_create_train(
            pool,
            &format!("{uid_prefix}-AUTO"),
            "2026-09-22".parse().unwrap(),
        )
        .await
        .expect("seed train");
        let mut matching = pool.begin().await.expect("begin the auto-match");
        let auto_sub = auto_bind_leg(&mut matching, racing_leg, trains_id, user_id).await;
        (journey_id, bound, racing_leg, auto_sub, matching)
    }

    /// DS-93: the notifier's auto-match binding a leg while its journey is
    /// being deleted. The delete waits for it and releases the train it
    /// bound; reading the legs before locking them missed that train and
    /// left it behind as one tracked on its own.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_journey_releases -- --ignored --test-threads=1`"]
    async fn delete_journey_releases_a_train_auto_matched_mid_delete() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELETE-RACING-MATCH";
        let (journey_id, bound, _, auto_sub, matching) =
            racing_auto_match(&pool, user_id, "TEST-TRAIN-DS93-RACE").await;
        let delete = tokio::spawn({
            let pool = pool.clone();
            async move { delete_journey(&pool, journey_id, user_id).await }
        });
        wait_for_a_lock_waiter(&pool).await;
        matching.commit().await.expect("commit the auto-match");
        assert!(delete.await.expect("join").expect("delete the journey"));

        assert!(!subscription_exists(&pool, bound).await);
        assert!(
            !subscription_exists(&pool, auto_sub).await,
            "the train matched mid-delete is released too"
        );
        assert!(standalone_listed(&pool, user_id).await.is_empty());

        cleanup_user(&pool, user_id).await;
    }

    /// DS-93: the same race for "Remove leg".
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                delete_leg_ -- --ignored --test-threads=1`"]
    async fn delete_leg_releases_a_train_auto_matched_mid_delete() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DS93-DELETE-LEG-RACING-MATCH";
        let (journey_id, bound, racing_leg, auto_sub, matching) =
            racing_auto_match(&pool, user_id, "TEST-TRAIN-DS93-LEG-RACE").await;
        let delete = tokio::spawn({
            let pool = pool.clone();
            async move { delete_leg(&pool, journey_id, racing_leg, user_id).await }
        });
        wait_for_a_lock_waiter(&pool).await;
        matching.commit().await.expect("commit the auto-match");
        assert_eq!(
            delete.await.expect("join").expect("remove the leg"),
            Some(false),
            "the journey keeps its other leg"
        );

        assert!(
            subscription_exists(&pool, bound).await,
            "the other leg's train stays"
        );
        assert!(
            !subscription_exists(&pool, auto_sub).await,
            "the train matched mid-delete is released"
        );
        assert!(standalone_listed(&pool, user_id).await.is_empty());

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    async fn set_leg_train_subscription_a_non_owner_cannot_bind_someone_elses_leg() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT-OWNER").await;
        seed_user(&pool, "TEST-JOURNEY-COMMIT-OTHER").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-COMMIT-OWNER",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");
        let tracking_id = crate::data::train_tracking::create_pin(
            &pool,
            &fixture_pin("WAT"),
            "TEST-JOURNEY-COMMIT-OTHER",
        )
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
                set_leg_train_subscription -- --ignored --test-threads=1`"]
    // 2026-09 Signal Box Audit Low finding regression test: distinct from
    // `set_leg_train_subscription_a_non_owner_cannot_bind_someone_elses_leg`
    // above -- that test has a non-owner trying to bind THEIR OWN
    // subscription to someone ELSE's leg. This test is the other half: the
    // OWNER of the leg trying to bind SOMEONE ELSE's subscription to their
    // own, legitimately-owned leg. Before this fix, this succeeded (the
    // query only checked leg/journey ownership, never subscription
    // ownership) -- a real IDOR, even though today's only caller
    // (`post_leg_train`) never actually exercises it because it always
    // mints a fresh subscription for the caller first.
    async fn set_leg_train_subscription_cannot_bind_someone_elses_subscription() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-IDOR-OWNER").await;
        seed_user(&pool, "TEST-JOURNEY-IDOR-OTHER").await;
        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            "TEST-JOURNEY-IDOR-OWNER",
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");
        // The OTHER user's own subscription -- the owner of `leg_id` has
        // no claim to this.
        let others_tracking_id = crate::data::train_tracking::create_pin(
            &pool,
            &fixture_pin("WAT"),
            "TEST-JOURNEY-IDOR-OTHER",
        )
        .await
        .expect("seed the other user's candidate subscription");

        let updated = set_leg_train_subscription(
            &pool,
            journey_id,
            leg_id,
            "TEST-JOURNEY-IDOR-OWNER",
            others_tracking_id,
        )
        .await
        .expect("attempt bind of someone else's subscription");
        assert!(
            !updated,
            "binding a leg to another user's train_subscriptions row must fail"
        );

        let leg = get_owned_leg(&pool, journey_id, leg_id, "TEST-JOURNEY-IDOR-OWNER")
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(
            leg.train_subscription_id, None,
            "the leg must remain unbound, not silently linked to the other user's subscription"
        );

        cleanup_user(&pool, "TEST-JOURNEY-IDOR-OWNER").await;
        cleanup_user(&pool, "TEST-JOURNEY-IDOR-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_subscription_and_bind_leg_creates_and_binds_in_one_call \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_and_bind_leg_creates_and_binds_in_one_call() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-CREATE-BIND";
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            service_date,
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-TRAIN-CREATE-BIND",
            service_date,
        )
        .await
        .expect("seed fixture train");

        let tracking_id =
            create_subscription_and_bind_leg(&pool, journey_id, leg_id, user_id, trains_id)
                .await
                .expect("create and bind must not error")
                .expect("leg is owned");

        let leg = get_owned_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.train_subscription_id, Some(tracking_id));
        assert_eq!(leg.match_mode, "manual");

        let subscription_owner: (String,) =
            sqlx::query_as("SELECT user_id FROM train_subscriptions WHERE id = $1")
                .bind(tracking_id)
                .fetch_one(&pool)
                .await
                .expect("the subscription must exist");
        assert_eq!(subscription_owner.0, user_id);

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_subscription_and_bind_leg_never_orphans_a_subscription_when_the_leg_is_gone \
                -- --ignored --test-threads=1`"]
    // M12 regression (2026-09 security/bug review, Repeater Signal): before
    // this fix, `post_leg_train` created the `train_subscriptions` row via
    // its own bare, immediately-committed statement, THEN separately called
    // `set_leg_train_subscription` to link it -- a concurrent deletion of
    // the leg between those two steps (`set_leg_train_subscription`'s own
    // documented "lost a race" case) left the freshly minted subscription
    // permanently orphaned, with no `journey_legs` row ever pointing at it.
    // `create_subscription_and_bind_leg` wraps both steps in one
    // transaction, so this same "leg vanished first" case must now roll
    // the subscription insert back too, leaving no row behind at all.
    async fn create_subscription_and_bind_leg_never_orphans_a_subscription_when_the_leg_is_gone() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-CREATE-BIND-GONE";
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        let (journey_id, leg_id) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            service_date,
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create window leg");

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-TRAIN-CREATE-BIND-GONE",
            service_date,
        )
        .await
        .expect("seed fixture train");

        // Simulates a concurrent deletion landing between the (would-be)
        // subscription creation and the leg link -- deleting the leg's
        // whole journey (its only leg), same as `delete_leg`'s own
        // last-leg-deletes-the-journey behavior.
        sqlx::query("DELETE FROM journeys WHERE id = $1")
            .bind(journey_id)
            .execute(&pool)
            .await
            .expect("simulate the concurrent deletion");

        let result =
            create_subscription_and_bind_leg(&pool, journey_id, leg_id, user_id, trains_id)
                .await
                .expect("must not error even though the leg is gone");
        assert_eq!(result, None, "no leg to bind to must report Ok(None)");

        let orphaned: Option<(i64,)> = sqlx::query_as(
            "SELECT id FROM train_subscriptions WHERE user_id = $1 AND trains_id = $2",
        )
        .bind(user_id)
        .bind(trains_id)
        .fetch_optional(&pool)
        .await
        .expect("check for an orphaned subscription");
        assert_eq!(
            orphaned, None,
            "the subscription this call would have minted must be rolled back, not left \
             orphaned with no leg ever pointing at it"
        );

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_journeys_for_user -- --ignored --test-threads=1`"]
    async fn list_journeys_for_user_picks_the_earliest_non_completed_leg() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
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
        .bind(format!("TEST-TRAIN-{user_id}-1"))
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .execute(&pool)
        .await
        .expect("insert test train 1");

        let trains_id_1: i64 =
            sqlx::query_scalar("SELECT id FROM trains WHERE train_uid = $1 AND service_date = $2")
                .bind(format!("TEST-TRAIN-{user_id}-1"))
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
        // 2026-09-22 UX review, C1: `/track/mine` now renders these rows,
        // and it cannot name a journey without the leg's own service date
        // (NOT `created_at`, which is when the journey was set up).
        assert_eq!(
            our_journey.service_date,
            "2026-09-22".parse::<NaiveDate>().unwrap(),
            "Should show leg 2's own service_date, not the journey's created_at"
        );
        // Both ends run through the same `LEFT JOIN stations ... ON
        // s.crs = UPPER(...)` the tracked-train list already uses. The
        // join is independent per end, so one end CAN resolve while the
        // other doesn't (this very fixture: `EDB` has a reference row,
        // `GLG` may not) -- that asymmetry is handled on the display side
        // by `lib/stationLabel.ts`'s `routeLabel`, which drops BOTH ends
        // to bare codes rather than printing one resolved name beside one
        // bare code. What this asserts is only that the join is wired at
        // all: a real reference row must come back as a name.
        let edb_has_reference_row: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM stations WHERE crs = 'EDB')")
                .fetch_one(&pool)
                .await
                .expect("check EDB reference row");
        if edb_has_reference_row {
            assert!(
                our_journey.origin_name.is_some(),
                "origin_name must resolve when the CRS has a stations row"
            );
        }

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
        .bind(format!("TEST-TRAIN-{user_id}-2"))
        .bind("2026-09-22".parse::<NaiveDate>().unwrap())
        .execute(&pool)
        .await
        .expect("insert test train 2");

        let trains_id_2: i64 =
            sqlx::query_scalar("SELECT id FROM trains WHERE train_uid = $1 AND service_date = $2")
                .bind(format!("TEST-TRAIN-{user_id}-2"))
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
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-ADD-WINDOW";
        seed_user(&pool, user_id).await;

        let (journey_id, first_leg_id) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create initial window leg");
        assert_eq!(leg_order_of(&pool, first_leg_id).await, 1);

        let depart_window = TimeWindow {
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
            TimeWindow::default(),
        )
        .await
        .expect("add second window leg")
        .into_added()
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
            TimeWindow {
                after: Some("16:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("add third window leg")
        .into_added()
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
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
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
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create initial window leg");
        assert_eq!(leg_order_of(&pool, first_leg_id).await, 1);

        let trains_id =
            crate::data::trains::find_or_create_train(&pool, "TEST-TRAIN-ADD-KNOWN", service_date)
                .await
                .expect("seed fixture train");

        let (leg_id, tracking_id) = add_known_train_leg_to_journey(
            &pool,
            journey_id,
            user_id,
            trains_id,
            service_date,
            None,
            None,
        )
        .await
        .expect("add known-train leg")
        .into_added()
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

    /// DB2-39: `journey_owner` returns the owning user, and `None` for an
    /// id with no journey.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                journey_owner_returns_the_owner_or_none -- --ignored --test-threads=1`"]
    async fn journey_owner_returns_the_owner_or_none() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DB2-39-JOURNEY-OWNER";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let (journey_id, _) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            "2026-09-22".parse().unwrap(),
            TimeWindow::default(),
            TimeWindow::default(),
        )
        .await
        .expect("create journey");

        assert_eq!(
            journey_owner(&pool, journey_id).await.unwrap(),
            Some(user_id.to_string())
        );
        let (unused,): (i64,) = sqlx::query_as("SELECT COALESCE(MAX(id), 0) + 1000 FROM journeys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(journey_owner(&pool, unused).await.unwrap(), None);

        cleanup_user(&pool, user_id).await;
    }

    /// One untracked leg, planned `departure` to `arrival`.
    async fn seed_untracked_leg(
        pool: &PgPool,
        journey_id: i64,
        leg_order: i32,
        departure: &str,
        arrival: &str,
    ) {
        sqlx::query(
            "INSERT INTO journey_legs (journey_id, leg_order, service_date, match_mode, kind, \
                                       transit_mode, planned_departure, planned_arrival) \
             VALUES ($1, $2, ($3::timestamptz)::date, 'manual', 'untracked', 'bus', \
                     $3::timestamptz, $4::timestamptz)",
        )
        .bind(journey_id)
        .bind(leg_order)
        .bind(departure)
        .bind(arrival)
        .execute(pool)
        .await
        .expect("seed untracked leg");
    }

    /// One WAT to RDG train leg on `service_date`, bound to
    /// `train_subscription_id` when given, else unmatched.
    async fn seed_train_leg(
        pool: &PgPool,
        journey_id: i64,
        leg_order: i32,
        service_date: &str,
        train_subscription_id: Option<i64>,
    ) {
        sqlx::query(
            "INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, \
                                       service_date, train_subscription_id, match_mode) \
             VALUES ($1, $2, 'WAT', 'RDG', $3::date, $4, \
                     CASE WHEN $4::bigint IS NULL THEN 'unmatched' ELSE 'manual' END)",
        )
        .bind(journey_id)
        .bind(leg_order)
        .bind(service_date)
        .bind(train_subscription_id)
        .execute(pool)
        .await
        .expect("seed train leg");
    }

    /// A `trains` row for `uid` on `service_date` and `user_id`'s
    /// subscription to it, pinned at WAT at `pinned` when given.
    async fn seed_subscribed_train(
        pool: &PgPool,
        user_id: &str,
        uid: &str,
        service_date: &str,
        pinned: Option<&str>,
    ) -> (i64, i64) {
        let trains_id: i64 = sqlx::query_scalar(
            "INSERT INTO trains (train_uid, service_date) VALUES ($1, $2::date) RETURNING id",
        )
        .bind(uid)
        .bind(service_date)
        .fetch_one(pool)
        .await
        .expect("seed train");
        let subscription_id: i64 = sqlx::query_scalar(
            "INSERT INTO train_subscriptions \
                 (user_id, trains_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3::date, CASE WHEN $4::timestamptz IS NULL THEN NULL ELSE 'WAT' END, \
                     $4::timestamptz) \
             RETURNING id",
        )
        .bind(user_id)
        .bind(trains_id)
        .bind(service_date)
        .bind(pinned)
        .fetch_one(pool)
        .await
        .expect("seed subscription");
        (trains_id, subscription_id)
    }

    /// DS-112: journeys come in travel order. Still to come or under way
    /// first, soonest first departure first (the journey in progress, then
    /// by first leg: an untracked leg's planned time, a train leg's PUBLIC
    /// timetabled departure from its origin, else its pinned departure, else
    /// its service date), newest id on a tie; then travelled journeys, most
    /// recent first. A journey whose every leg is finished is travelled even
    /// though its date is ahead; one whose train runs past midnight is not
    /// travelled until its timetabled arrival. A leg-less journey is left
    /// out.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_journeys_for_user_orders_by_travel_time -- --ignored --test-threads=1`"]
    async fn list_journeys_for_user_orders_by_travel_time() {
        let pool = connect().await;
        let _fixtures = crate::test_support::FixtureCleanup::new(
            &pool,
            [
                "DELETE FROM schedule_destination_departures WHERE service_date = '2099-01-20' \
                 AND train_uid IN ('TSORT1', 'TSORT2', 'TSORT3')",
                "DELETE FROM trains WHERE service_date IN ('2099-01-20', '2099-01-21') \
                 AND train_uid IN ('TSORT1', 'TSORT2', 'TSORT3')",
                "DELETE FROM schedule_destination_departures WHERE train_uid = 'TSORT7'",
                "DELETE FROM trains WHERE train_uid = 'TSORT7'",
            ],
        )
        .await;
        let user_id = "TEST-DS112-JOURNEY-ORDER";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let journey = || seed_journey(&pool, user_id);

        // Still running: yesterday's (London) 22:00 overnight train, due at
        // its terminus at 23:59:59 tonight. Its arrival comes from the
        // timetable; the end of its service date (midnight just gone) would
        // have called it travelled.
        let yesterday: String =
            sqlx::query_scalar("SELECT ((now() AT TIME ZONE 'Europe/London')::date - 1)::text")
                .fetch_one(&pool)
                .await
                .expect("yesterday");
        let overnight = journey().await;
        let (_, overnight_sub) =
            seed_subscribed_train(&pool, user_id, "TSORT7", &yesterday, None).await;
        seed_train_leg(&pool, overnight, 1, &yesterday, Some(overnight_sub)).await;
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                 (service_date, destination_crs, scheduled, train_uid, origin_crs, \
                  public_departure, destination_arrival, destination_arrival_day_offset) \
             VALUES ($1::date, 'RDG', '22:00', 'TSORT7', 'WAT', '22:00', '23:59:59', 1)",
        )
        .bind(&yesterday)
        .execute(&pool)
        .await
        .expect("seed overnight timetable");

        // Travelled: a train leg whose service date is long over, and an
        // untracked leg that arrived four hours ago.
        let past_old = journey().await;
        seed_train_leg(&pool, past_old, 1, "2020-01-01", None).await;
        let past_recent = journey().await;
        let now = Utc::now();
        let hours = |h: i64| (now + chrono::Duration::hours(h)).to_rfc3339();
        seed_untracked_leg(&pool, past_recent, 1, &hours(-5), &hours(-4)).await;
        // Under way: left an hour ago, arrives in an hour.
        let in_progress = journey().await;
        seed_untracked_leg(&pool, in_progress, 1, &hours(-1), &hours(1)).await;
        // 2099-01-20 (GMT, so London time is UTC): a leg with no time at
        // all sorts at the start of its day; a pinned train at 08:00; an
        // untracked leg at 08:59:30; a train whose working departure is
        // 08:59 but whose public one is 09:00 sorts by the public one.
        let day_only = journey().await;
        seed_train_leg(&pool, day_only, 1, "2099-01-20", None).await;
        let scheduled = journey().await;
        let (_, scheduled_sub) =
            seed_subscribed_train(&pool, user_id, "TSORT1", "2099-01-20", None).await;
        seed_train_leg(&pool, scheduled, 1, "2099-01-20", Some(scheduled_sub)).await;
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                 (service_date, destination_crs, scheduled, train_uid, origin_crs, public_departure) \
             VALUES ('2099-01-20', 'RDG', '08:59', 'TSORT1', 'WAT', '09:00')",
        )
        .execute(&pool)
        .await
        .expect("seed timetable");
        let pinned = journey().await;
        let (_, pinned_sub) = seed_subscribed_train(
            &pool,
            user_id,
            "TSORT2",
            "2099-01-20",
            Some("2099-01-20T08:00:00Z"),
        )
        .await;
        seed_train_leg(&pool, pinned, 1, "2099-01-20", Some(pinned_sub)).await;
        let untracked = journey().await;
        seed_untracked_leg(
            &pool,
            untracked,
            1,
            "2099-01-20T08:59:30Z",
            "2099-01-20T10:00:00Z",
        )
        .await;
        // Two legs: sorted by the first (the day before), not the second.
        let two_legs = journey().await;
        seed_untracked_leg(
            &pool,
            two_legs,
            1,
            "2099-01-19T12:00:00Z",
            "2099-01-19T13:00:00Z",
        )
        .await;
        seed_train_leg(&pool, two_legs, 2, "2099-01-21", None).await;
        // Same departure: the newer journey first.
        let tie_older = journey().await;
        seed_untracked_leg(
            &pool,
            tie_older,
            1,
            "2099-01-22T10:00:00Z",
            "2099-01-22T11:00:00Z",
        )
        .await;
        let tie_newer = journey().await;
        seed_untracked_leg(
            &pool,
            tie_newer,
            1,
            "2099-01-22T10:00:00Z",
            "2099-01-22T11:00:00Z",
        )
        .await;
        // Dated ahead, but its only train has completed: travelled.
        let completed = journey().await;
        let (completed_train, completed_sub) =
            seed_subscribed_train(&pool, user_id, "TSORT3", "2099-01-21", None).await;
        seed_train_leg(&pool, completed, 1, "2099-01-21", Some(completed_sub)).await;
        sqlx::query("INSERT INTO train_current_state (trains_id, status) VALUES ($1, 'completed')")
            .bind(completed_train)
            .execute(&pool)
            .await
            .expect("seed completed state");
        // No leg: never listed.
        let _legless = journey().await;

        let listed: Vec<i64> = list_journeys_for_user(&pool, user_id)
            .await
            .expect("list")
            .iter()
            .map(|j| j.id)
            .collect();
        assert_eq!(
            listed,
            vec![
                overnight,
                in_progress,
                two_legs,
                day_only,
                pinned,
                untracked,
                scheduled,
                tie_newer,
                tie_older,
                completed,
                past_recent,
                past_old,
            ],
        );

        cleanup_user(&pool, user_id).await;
    }

    /// DS-112: a journey is over only once its last train has actually
    /// arrived. Both trains here were due at RDG ten minutes ago with no
    /// live delay, so inside `TRAVEL_ARRIVAL_GRACE_MINUTES`; the one TRUST
    /// reported arriving is travelled, the other is still under way.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_journeys_for_user_waits_for_the_actual_arrival -- --ignored --test-threads=1`"]
    async fn list_journeys_for_user_waits_for_the_actual_arrival() {
        let pool = connect().await;
        let _fixtures = crate::test_support::FixtureCleanup::new(
            &pool,
            [
                "DELETE FROM schedule_destination_departures \
                 WHERE train_uid IN ('TSORT31', 'TSORT32')",
                "DELETE FROM trains WHERE train_uid IN ('TSORT31', 'TSORT32')",
            ],
        )
        .await;
        let user_id = "TEST-DS112-JOURNEY-ARRIVED";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let (date, due): (String, NaiveTime) = sqlx::query_as(
            "SELECT (t AT TIME ZONE 'Europe/London')::date::text, \
                    (t AT TIME ZONE 'Europe/London')::time \
             FROM (SELECT now() - INTERVAL '10 minutes' AS t) x",
        )
        .fetch_one(&pool)
        .await
        .expect("due time");

        let mut journeys = Vec::new();
        for uid in ["TSORT31", "TSORT32"] {
            let journey = seed_journey(&pool, user_id).await;
            let (trains_id, subscription) =
                seed_subscribed_train(&pool, user_id, uid, &date, None).await;
            seed_train_leg(&pool, journey, 1, &date, Some(subscription)).await;
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                     (service_date, destination_crs, scheduled, train_uid, origin_crs, \
                      destination_arrival) \
                 VALUES ($1::date, 'RDG', '00:00', $2, 'WAT', $3)",
            )
            .bind(&date)
            .bind(uid)
            .bind(due)
            .execute(&pool)
            .await
            .expect("seed timetable");
            journeys.push((journey, trains_id));
        }
        let (still_running, _) = journeys[0];
        let (arrived, arrived_train) = journeys[1];
        sqlx::query(
            "INSERT INTO train_movement_events \
                 (trains_id, dedup_key, msg_type, event_type, loc_crs, actual_timestamp, raw_body) \
             VALUES ($1, 'ds112-journey-arrival', '0003', 'ARRIVAL', 'RDG', \
                     now() - INTERVAL '2 minutes', '{}'::jsonb)",
        )
        .bind(arrived_train)
        .execute(&pool)
        .await
        .expect("seed arrival report");

        let listed: Vec<i64> = list_journeys_for_user(&pool, user_id)
            .await
            .expect("list")
            .iter()
            .map(|j| j.id)
            .collect();
        // Under way first even though it was created first and departs no
        // earlier: the arrived one is in the travelled half.
        assert_eq!(listed, vec![still_running, arrived]);

        cleanup_user(&pool, user_id).await;
    }

    /// DS-114: the list row summarises the whole journey, not the current
    /// leg: leg counts across every leg, and the LAST train leg's train for
    /// the final arrival (the user's rule: a late first train that still
    /// makes its connection leaves the journey on time).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_journeys_for_user_summarises_every_leg -- --ignored --test-threads=1`"]
    async fn list_journeys_for_user_summarises_every_leg() {
        let pool = connect().await;
        let _fixtures = crate::test_support::FixtureCleanup::new(
            &pool,
            ["DELETE FROM trains WHERE train_uid IN ('TSUM41', 'TSUM42', 'TSUM43')"],
        )
        .await;
        let user_id = "TEST-DS114-SUMMARY";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let date = "2099-03-04";

        // Leg 1 late but completed, leg 2 on time and running, leg 3 with no
        // train picked yet; a second journey whose last train is cancelled.
        let journey = seed_journey(&pool, user_id).await;
        let (first_train, first_sub) =
            seed_subscribed_train(&pool, user_id, "TSUM41", date, None).await;
        let (last_train, last_sub) =
            seed_subscribed_train(&pool, user_id, "TSUM42", date, None).await;
        seed_train_leg(&pool, journey, 1, date, Some(first_sub)).await;
        seed_train_leg(&pool, journey, 2, date, Some(last_sub)).await;
        seed_train_leg(&pool, journey, 3, date, None).await;
        let cancelled_journey = seed_journey(&pool, user_id).await;
        let (cancelled_train, cancelled_sub) =
            seed_subscribed_train(&pool, user_id, "TSUM43", date, None).await;
        seed_train_leg(&pool, cancelled_journey, 1, date, Some(cancelled_sub)).await;
        for (trains_id, status, delay) in [
            (first_train, "completed", 12),
            (last_train, "en_route", 0),
            (cancelled_train, "cancelled", 0),
        ] {
            sqlx::query(
                "INSERT INTO train_current_state (trains_id, status, delay_minutes) \
                 VALUES ($1, $2, $3)",
            )
            .bind(trains_id)
            .bind(status)
            .bind(delay)
            .execute(&pool)
            .await
            .expect("seed state");
        }

        let rows = list_journeys_for_user(&pool, user_id).await.expect("list");
        let row = rows.iter().find(|r| r.id == journey).expect("journey row");
        assert!(!row.travelled, "leg 2 is still running");
        assert_eq!(row.leg_count, 3);
        assert_eq!(row.train_leg_count, 3);
        assert_eq!(row.cancelled_leg_count, 0);
        assert_eq!(row.unmatched_leg_count, 1);
        assert_eq!(row.finished_leg_count, 1);
        assert_eq!(row.started_leg_count, 2);
        // The final arrival is the last train leg with a train: leg 3 has
        // none, so it is read from leg 3's own (empty) train, not leg 1's
        // late one.
        assert_eq!(row.final_trains_id, None);
        assert_eq!(row.final_arrival_delay_minutes, None);

        // Once leg 3 is dropped, the final arrival is leg 2's on-time train,
        // whatever leg 1's delay.
        sqlx::query("DELETE FROM journey_legs WHERE journey_id = $1 AND leg_order = 3")
            .bind(journey)
            .execute(&pool)
            .await
            .expect("drop leg 3");
        let rows = list_journeys_for_user(&pool, user_id).await.expect("list");
        let row = rows.iter().find(|r| r.id == journey).expect("journey row");
        assert_eq!(row.final_trains_id, Some(last_train));
        assert_eq!(row.final_working_delay_minutes, Some(0));
        assert_eq!(row.final_status.as_deref(), Some("en_route"));

        let cancelled = rows
            .iter()
            .find(|r| r.id == cancelled_journey)
            .expect("cancelled row");
        assert_eq!(cancelled.cancelled_leg_count, 1);
        assert_eq!(cancelled.finished_leg_count, 0);
        // A cancelled train never arrives: no final delay.
        assert_eq!(cancelled.final_arrival_delay_minutes, None);

        cleanup_user(&pool, user_id).await;
    }

    /// DS-112 (and DB2-18): the cap keeps the journeys still to come, then
    /// the most recently travelled, however long ago the upcoming ones were
    /// saved. Ordering by `created_at` kept the newest-saved instead, here
    /// all travelled. A newer leg-less journey neither appears nor takes a
    /// slot.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_journeys_for_user_keeps_the_upcoming_journeys_under_the_cap -- --ignored --test-threads=1`"]
    async fn list_journeys_for_user_keeps_the_upcoming_journeys_under_the_cap() {
        let pool = connect().await;
        let user_id = "TEST-DB2-18-LIST";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let limit = crate::data::train_tracking::MINE_LIST_LIMIT;

        // Five upcoming journeys, saved long ago, one day apart in 2099.
        let upcoming: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO journeys (user_id, created_at) \
             SELECT $1, TIMESTAMPTZ '2020-01-01 00:00Z' + g * INTERVAL '1 minute' \
             FROM generate_series(1, 5) g ORDER BY g RETURNING id",
        )
        .bind(user_id)
        .fetch_all(&pool)
        .await
        .expect("seed upcoming journeys");
        // Saved in the order 5th, 4th, ... so the soonest is the newest:
        // the list must not just be id order.
        sqlx::query(
            "INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, match_mode) \
             SELECT id, 1, 'WAT', 'RDG', DATE '2099-02-06' - n::int, 'unmatched' \
             FROM unnest($1::bigint[]) WITH ORDINALITY AS u(id, n)",
        )
        .bind(&upcoming)
        .execute(&pool)
        .await
        .expect("seed upcoming legs");
        // `limit` travelled journeys, all saved after the upcoming ones,
        // one day apart in 2020.
        let travelled: Vec<i64> = sqlx::query_scalar(
            "INSERT INTO journeys (user_id, created_at) \
             SELECT $1, TIMESTAMPTZ '2026-09-01 00:00Z' + g * INTERVAL '1 minute' \
             FROM generate_series(1, $2) g ORDER BY g RETURNING id",
        )
        .bind(user_id)
        .bind(limit as i32)
        .fetch_all(&pool)
        .await
        .expect("seed travelled journeys");
        sqlx::query(
            "INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, match_mode) \
             SELECT id, 1, 'WAT', 'RDG', DATE '2020-01-01' + n::int, 'unmatched' \
             FROM unnest($1::bigint[]) WITH ORDINALITY AS u(id, n)",
        )
        .bind(&travelled)
        .execute(&pool)
        .await
        .expect("seed travelled legs");
        // Newest of all, but with no leg.
        sqlx::query("INSERT INTO journeys (user_id, created_at) VALUES ($1, TIMESTAMPTZ '2026-12-01 00:00Z')")
            .bind(user_id)
            .execute(&pool)
            .await
            .expect("seed leg-less journey");

        let listed = list_journeys_for_user(&pool, user_id).await.expect("list");
        assert_eq!(listed.len() as i64, limit, "capped");
        let got: Vec<i64> = listed.iter().map(|j| j.id).collect();
        // Upcoming soonest first (the last seeded is the soonest), then
        // travelled most recent first (the last seeded is the most recent).
        let expected: Vec<i64> = upcoming
            .iter()
            .rev()
            .chain(travelled.iter().rev())
            .take(limit as usize)
            .copied()
            .collect();
        assert_eq!(got, expected);

        cleanup_user(&pool, user_id).await;
    }

    /// A planned three-leg journey with no tracking data: the row's current
    /// leg is the first, but the whole-journey fields name it from the first
    /// leg's origin to the LAST leg's destination (the `/track/mine` bug
    /// that showed it ending at its first change), with the first leg's
    /// date and the leg count. `leg_order` gaps (a deleted leg) don't matter.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_journeys_for_user_names_the_whole_journey -- --ignored --test-threads=1`"]
    async fn list_journeys_for_user_names_the_whole_journey() {
        let pool = connect().await;
        let user_id = "TEST-LIST-JOURNEYS-WHOLE-NAME";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        sqlx::query(
            "INSERT INTO stations (crs, name) VALUES ('ZZA', 'Test Origin'), ('ZZD', 'Test End') \
             ON CONFLICT (crs) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stations");

        let journey_id = insert_journey(&pool, user_id, None)
            .await
            .expect("insert journey");
        sqlx::query(
            "INSERT INTO journey_legs (journey_id, leg_order, origin_crs, destination_crs, service_date, match_mode) \
             VALUES ($1, 1, 'zza', 'ZZB', DATE '2026-09-22', 'unmatched'), \
                    ($1, 2, 'ZZB', 'ZZC', DATE '2026-09-22', 'unmatched'), \
                    ($1, 5, 'ZZC', 'ZZD', DATE '2026-09-23', 'unmatched')",
        )
        .bind(journey_id)
        .execute(&pool)
        .await
        .expect("seed legs");

        let listed = list_journeys_for_user(&pool, user_id).await.expect("list");
        let row = listed.iter().find(|j| j.id == journey_id).expect("listed");
        // The current leg is still the first...
        assert_eq!(row.destination_crs.as_deref(), Some("ZZB"));
        // ...but the journey is named end to end.
        assert_eq!(row.first_origin_crs.as_deref(), Some("zza"));
        assert_eq!(row.first_origin_name.as_deref(), Some("Test Origin"));
        assert_eq!(row.last_destination_crs.as_deref(), Some("ZZD"));
        assert_eq!(row.last_destination_name.as_deref(), Some("Test End"));
        assert_eq!(
            row.first_service_date,
            "2026-09-22".parse::<NaiveDate>().unwrap()
        );
        assert_eq!(row.leg_count, 3);

        let json = serde_json::to_value(row).expect("serialize");
        assert_eq!(json["lastDestinationCrs"], "ZZD");
        assert_eq!(json["firstServiceDate"], "2026-09-22");
        assert_eq!(json["legCount"], 3);

        cleanup_user(&pool, user_id).await;
        sqlx::query("DELETE FROM stations WHERE crs IN ('ZZA', 'ZZD')")
            .execute(&pool)
            .await
            .expect("cleanup stations");
    }

    /// DB2-17: concurrent adds to one journey serialise on the journey row
    /// lock. Every call succeeds with its own `leg_order` (no 500 from the
    /// unique constraint), and the leg cap is exact under a burst.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                concurrent_add_leg_calls_serialise_on_the_journey_row -- --ignored --test-threads=1`"]
    async fn concurrent_add_leg_calls_serialise_on_the_journey_row() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-DB2-17-CONCURRENT-ADD";
        cleanup_user(&pool, user_id).await;
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();
        let (journey_id, _) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            service_date,
            TimeWindow::default(),
            TimeWindow::default(),
        )
        .await
        .expect("create journey");

        // 1 existing leg + 25 concurrent adds against a cap of 20.
        let mut calls = tokio::task::JoinSet::new();
        for _ in 0..25 {
            let pool = pool.clone();
            calls.spawn(async move {
                add_window_leg_to_journey(
                    &pool,
                    journey_id,
                    user_id,
                    "RDG",
                    "OXF",
                    service_date,
                    TimeWindow::default(),
                    TimeWindow::default(),
                )
                .await
            });
        }
        let mut added = 0;
        let mut at_cap = 0;
        while let Some(result) = calls.join_next().await {
            match result
                .unwrap()
                .expect("no add may fail with a constraint violation")
            {
                AddLegOutcome::Added(_) => added += 1,
                AddLegOutcome::AtCap => at_cap += 1,
                AddLegOutcome::NotFound | AddLegOutcome::NoSuchLeg => {
                    panic!("the journey is the caller's")
                }
            }
        }
        assert_eq!(added, MAX_LEGS_PER_JOURNEY - 1);
        assert_eq!(at_cap, 25 - (MAX_LEGS_PER_JOURNEY - 1));

        let orders: Vec<i32> = sqlx::query_scalar(
            "SELECT leg_order FROM journey_legs WHERE journey_id = $1 ORDER BY leg_order",
        )
        .bind(journey_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert_eq!(
            orders,
            (1..=MAX_LEGS_PER_JOURNEY as i32).collect::<Vec<_>>()
        );

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_known_train_leg_to_journey_racing_leg_order_never_orphans_a_subscription -- \
                --ignored --test-threads=1`"]
    // 2026-09 Signal Box Audit Low finding regression test: before this
    // fix, `create_subscription_for_train` committed as its own bare
    // statement, separately from (and before) the `insert_leg` call that
    // attaches it to a leg. `owned_next_leg_order`'s own doc comment
    // already names the READ COMMITTED race two concurrent calls for the
    // SAME journey can hit -- both computing the same `next_leg_order` and
    // one losing to the `UNIQUE (journey_id, leg_order)` constraint. The
    // bug: the LOSING call's subscription was left committed anyway, with
    // no `journey_legs` row ever pointing at it -- a real orphan, exactly
    // as inert-but-not-actually-inert as `set_leg_train_subscription`'s
    // own doc comment describes. After this fix, the loser's subscription
    // must not exist at all once its transaction rolls back.
    async fn add_known_train_leg_to_journey_racing_leg_order_never_orphans_a_subscription() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-ADD-KNOWN-RACE";
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        let (journey_id, _first_leg_id) = create_journey_with_window_leg(
            &pool,
            user_id,
            None,
            "WAT",
            "RDG",
            service_date,
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("create initial window leg");

        // Two DIFFERENT trains, so whichever call loses the leg_order race
        // is identifiable afterwards by its own distinct `trains_id`.
        let trains_id_a = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-TRAIN-ADD-KNOWN-RACE-A",
            service_date,
        )
        .await
        .expect("seed fixture train a");
        let trains_id_b = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-TRAIN-ADD-KNOWN-RACE-B",
            service_date,
        )
        .await
        .expect("seed fixture train b");

        let pool_a = pool.clone();
        let pool_b = pool.clone();
        let (result_a, result_b) = tokio::join!(
            add_known_train_leg_to_journey(
                &pool_a,
                journey_id,
                user_id,
                trains_id_a,
                service_date,
                None,
                None,
            ),
            add_known_train_leg_to_journey(
                &pool_b,
                journey_id,
                user_id,
                trains_id_b,
                service_date,
                None,
                None,
            ),
        );

        // Both calls read `next_leg_order` from the SAME starting point --
        // at most one of them can have actually won the `leg_order` slot;
        // the other either lost outright (`Err`, unique-constraint
        // violation) or -- if this race didn't land this particular run --
        // both succeeded with different leg_orders. Only the guaranteed
        // invariant matters here: neither trains_id ends up with an
        // orphaned subscription.
        for (trains_id, result) in [(trains_id_a, result_a), (trains_id_b, result_b)] {
            let subscription: Option<(i64,)> = sqlx::query_as(
                "SELECT id FROM train_subscriptions WHERE user_id = $1 AND trains_id = $2",
            )
            .bind(user_id)
            .bind(trains_id)
            .fetch_optional(&pool)
            .await
            .expect("check for a subscription row");

            match result {
                Ok(AddLegOutcome::Added(_)) => {
                    // Succeeded: a subscription row for this trains_id is
                    // expected and correct.
                    assert!(subscription.is_some());
                }
                Err(_) => {
                    // Lost the race: the fix means its subscription must
                    // have been rolled back, not left orphaned.
                    assert!(
                        subscription.is_none(),
                        "a losing call must not leave an orphaned train_subscriptions row \
                         for trains_id {trains_id}"
                    );
                }
                Ok(other) => panic!(
                    "journey_id is owned by user_id and far under the leg cap -- got {other:?}"
                ),
            }
        }

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_known_train_leg_overrides_both_ends_when_given -- \
                --ignored --test-threads=1`"]
    async fn create_journey_with_known_train_leg_overrides_both_ends_when_given() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-KNOWN-OVERRIDE-BOTH";
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        // A bare `find_or_create_train` upsert carries no schedule data at
        // all, so `pin_origin_crs`/`pin_destination_crs` are both `None`
        // off the resulting subscription -- whatever lands on the leg here
        // can only be the override, never a coincidental pin value.
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-TRAIN-KNOWN-OVERRIDE-BOTH",
            service_date,
        )
        .await
        .expect("seed fixture train");

        let (journey_id, leg_id, _tracking_id) = create_journey_with_known_train_leg(
            &pool,
            user_id,
            None,
            trains_id,
            service_date,
            Some("CRE"),
            Some("PRE"),
        )
        .await
        .expect("create journey with known-train leg and both overrides");

        let leg = get_owned_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.origin_crs.as_deref(), Some("CRE"));
        assert_eq!(leg.destination_crs.as_deref(), Some("PRE"));

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_known_train_leg_overrides_one_end_and_falls_back_to_the_pin_for_the_other \
                -- --ignored --test-threads=1`"]
    async fn create_journey_with_known_train_leg_overrides_one_end_and_falls_back_to_the_pin_for_the_other()
     {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-KNOWN-OVERRIDE-PARTIAL";
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        // Seed a `trains` row WITH real origin/destination set directly --
        // same raw-SQL fixture pattern
        // `list_journeys_for_user_picks_the_earliest_non_completed_leg`
        // already uses -- so `create_subscription_for_train`'s pin
        // read-back is non-`None` for both ends, and the destination end
        // (left un-overridden below) has a real pin value to fall back to.
        let trains_id: i64 = sqlx::query_scalar(
            "INSERT INTO trains (train_uid, service_date, origin_crs, destination_crs) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind("TEST-TRAIN-KNOWN-OVERRIDE-PARTIAL")
        .bind(service_date)
        .bind("BHM")
        .bind("GLC")
        .fetch_one(&pool)
        .await
        .expect("seed fixture train with real origin/destination");

        let (journey_id, leg_id, _tracking_id) = create_journey_with_known_train_leg(
            &pool,
            user_id,
            None,
            trains_id,
            service_date,
            Some("CRE"),
            None,
        )
        .await
        .expect("create journey with known-train leg and a partial override");

        let leg = get_owned_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(
            leg.origin_crs.as_deref(),
            Some("CRE"),
            "origin should be the supplied override"
        );
        assert_eq!(
            leg.destination_crs.as_deref(),
            Some("GLC"),
            "destination should fall back to the pin-derived value"
        );

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .expect("cleanup fixture train");
        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                create_journey_with_known_train_leg_omitting_both_overrides_reproduces_the_pin_derived_behavior \
                -- --ignored --test-threads=1`"]
    async fn create_journey_with_known_train_leg_omitting_both_overrides_reproduces_the_pin_derived_behavior()
     {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-KNOWN-OVERRIDE-NONE";
        seed_user(&pool, user_id).await;
        let service_date: NaiveDate = "2026-09-22".parse().unwrap();

        // Same seeded-`trains`-row fixture as the partial-override test
        // above -- this is the explicit regression test for Judgment Call
        // 1's backward-compatibility claim: `None, None` must reproduce
        // today's exact pin-derived behavior.
        let trains_id: i64 = sqlx::query_scalar(
            "INSERT INTO trains (train_uid, service_date, origin_crs, destination_crs) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind("TEST-TRAIN-KNOWN-OVERRIDE-NONE")
        .bind(service_date)
        .bind("BHM")
        .bind("GLC")
        .fetch_one(&pool)
        .await
        .expect("seed fixture train with real origin/destination");

        let (journey_id, leg_id, _tracking_id) = create_journey_with_known_train_leg(
            &pool,
            user_id,
            None,
            trains_id,
            service_date,
            None,
            None,
        )
        .await
        .expect("create journey with known-train leg and no overrides");

        let leg = get_owned_leg(&pool, journey_id, leg_id, user_id)
            .await
            .expect("read leg")
            .expect("leg exists");
        assert_eq!(leg.origin_crs.as_deref(), Some("BHM"));
        assert_eq!(leg.destination_crs.as_deref(), Some("GLC"));

        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .expect("cleanup fixture train");
        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                add_leg_to_journey_a_non_owner_cannot_add_a_leg_to_someone_elses_journey -- --ignored --test-threads=1`"]
    async fn add_leg_to_journey_a_non_owner_cannot_add_a_leg_to_someone_elses_journey() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
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
            TimeWindow {
                after: Some("08:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
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
            TimeWindow {
                after: Some("12:00:00".parse().unwrap()),
                before: None,
            },
            TimeWindow::default(),
        )
        .await
        .expect("attempt add window leg as non-owner");
        assert_eq!(window_result, AddLegOutcome::NotFound);

        // `trains_id` is never dereferenced: ownership is checked first,
        // before this function ever touches `train_subscriptions`.
        let known_train_result = add_known_train_leg_to_journey(
            &pool,
            journey_id,
            other_id,
            i64::MAX,
            "2026-09-22".parse().unwrap(),
            None,
            None,
        )
        .await
        .expect("attempt add known-train leg as non-owner");
        assert_eq!(known_train_result, AddLegOutcome::NotFound);

        cleanup_user(&pool, owner_id).await;
        cleanup_user(&pool, other_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_the_owner_can_always_read_their_own_journey_with_no_grant() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-1").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-1").await;

        assert!(
            journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-OWNER-1")
                .await
                .expect("check readability")
        );

        cleanup_journey(&pool, journey_id, &["TEST-JOURNEY-READABLE-OWNER-1"]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_a_fellow_group_member_can_read_a_shared_journey() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-2").await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-MEMBER-2").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-2").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Journey Readable Test",
            "TEST-JOURNEY-READABLE-OWNER-2",
        )
        .await
        .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-JOURNEY-READABLE-MEMBER-2")
        .execute(&pool)
        .await
        .expect("seed member");
        crate::data::groups::add_journey_to_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-JOURNEY-READABLE-OWNER-2",
        )
        .await
        .expect("share journey");

        assert!(
            journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-MEMBER-2")
                .await
                .expect("check readability")
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_journey(
            &pool,
            journey_id,
            &[
                "TEST-JOURNEY-READABLE-OWNER-2",
                "TEST-JOURNEY-READABLE-MEMBER-2",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_excludes_a_stranger_in_no_shared_group() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-3").await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-STRANGER-3").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-3").await;

        // The stranger is a member of SOME group, just not one this
        // journey was shared into -- the core negative case, protecting
        // against a query that accidentally checks "is a member of any
        // group" instead of "is a member of a group THIS journey was
        // shared into".
        let unrelated_group_id = crate::data::groups::create_group(
            &pool,
            "Unrelated Group",
            "TEST-JOURNEY-READABLE-STRANGER-3",
        )
        .await
        .expect("create unrelated group");

        assert!(
            !journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-STRANGER-3")
                .await
                .expect("check readability")
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&unrelated_group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_journey(
            &pool,
            journey_id,
            &[
                "TEST-JOURNEY-READABLE-OWNER-3",
                "TEST-JOURNEY-READABLE-STRANGER-3",
            ],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_a_former_member_loses_access_once_removed() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-OWNER-4").await;
        seed_user(&pool, "TEST-JOURNEY-READABLE-MEMBER-4").await;
        let journey_id = seed_journey(&pool, "TEST-JOURNEY-READABLE-OWNER-4").await;
        let group_id = crate::data::groups::create_group(
            &pool,
            "Journey Readable Departure Test",
            "TEST-JOURNEY-READABLE-OWNER-4",
        )
        .await
        .expect("create group");
        sqlx::query(
            "INSERT INTO group_members (group_id, user_id, role) VALUES ($1, $2, 'member')",
        )
        .bind(&group_id)
        .bind("TEST-JOURNEY-READABLE-MEMBER-4")
        .execute(&pool)
        .await
        .expect("seed member");
        crate::data::groups::add_journey_to_group(
            &pool,
            &group_id,
            journey_id,
            "TEST-JOURNEY-READABLE-OWNER-4",
        )
        .await
        .expect("share journey");
        assert!(
            journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-MEMBER-4")
                .await
                .expect("readable while a member")
        );

        crate::data::groups::remove_member(
            &pool,
            &group_id,
            "TEST-JOURNEY-READABLE-MEMBER-4",
            true,
        )
        .await
        .expect("remove member");

        // Task 2's remove_member cascade should have deleted the
        // group_journeys row too, so this is doubly protected -- even a
        // query that only checked group_members (and not group_journeys)
        // would already deny this, but the point of this test is
        // end-to-end: departure really does revoke read access.
        assert!(
            !journey_readable_by(&pool, journey_id, "TEST-JOURNEY-READABLE-MEMBER-4")
                .await
                .expect("no longer readable after leaving")
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_journey(
            &pool,
            journey_id,
            &[
                "TEST-JOURNEY-READABLE-OWNER-4",
                "TEST-JOURNEY-READABLE-MEMBER-4",
            ],
        )
        .await;
    }

    // --- delete_journey (direct whole-journey delete) -------------------

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_journey -- --ignored --test-threads=1`"]
    async fn delete_journey_removes_a_single_leg_journey_and_every_referencing_row() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-DELETE-SINGLE";
        seed_user(&pool, user_id).await;
        let journey_id = insert_journey(&pool, user_id, Some("Cascade test"))
            .await
            .expect("insert journey");
        let leg_id = insert_leg(
            &pool,
            journey_id,
            1,
            Some("WAT"),
            Some("RDG"),
            "2026-09-22".parse().unwrap(),
            None,
            "unmatched",
            None,
            None,
            None,
            None,
            false,
        )
        .await
        .expect("insert leg");

        // Seeds a `journey_leg_notification_state` row -- this table has no
        // direct FK to `journeys`, only a two-hop one via `journey_legs`
        // (`journey_leg_id ... ON DELETE CASCADE`), so this row is the one
        // that proves the CASCADE keeps walking past the immediate
        // `journey_legs` row, not just deleting that one table.
        sqlx::query(
            "INSERT INTO journey_leg_notification_state \
                (user_id, journey_leg_id, last_notified_skipped, last_notified_at) \
             VALUES ($1, $2, false, NOW())",
        )
        .bind(user_id)
        .bind(leg_id)
        .execute(&pool)
        .await
        .expect("seed fixture notification state");

        // Shares the journey into a group -- proves `group_journeys.journey_id
        // ... ON DELETE CASCADE` fires too, not just the leg-side cascades.
        let group_id =
            crate::data::groups::create_group(&pool, "Delete Journey Cascade Test", user_id)
                .await
                .expect("create group");
        let shared =
            crate::data::groups::add_journey_to_group(&pool, &group_id, journey_id, user_id)
                .await
                .expect("share journey into group");
        assert!(shared);

        let deleted = delete_journey(&pool, journey_id, user_id)
            .await
            .expect("delete journey");
        assert!(deleted);

        let (journeys_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM journeys WHERE id = $1")
                .bind(journey_id)
                .fetch_one(&pool)
                .await
                .expect("count journeys");
        assert_eq!(journeys_left, 0, "the journeys row itself must be gone");

        let (legs_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM journey_legs WHERE journey_id = $1")
                .bind(journey_id)
                .fetch_one(&pool)
                .await
                .expect("count journey_legs");
        assert_eq!(legs_left, 0, "every leg must be gone via ON DELETE CASCADE");

        let (notification_state_left,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM journey_leg_notification_state WHERE journey_leg_id = $1",
        )
        .bind(leg_id)
        .fetch_one(&pool)
        .await
        .expect("count journey_leg_notification_state");
        assert_eq!(
            notification_state_left, 0,
            "notification-dedup rows must be gone via the transitive cascade through journey_legs"
        );

        let (group_journeys_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM group_journeys WHERE journey_id = $1")
                .bind(journey_id)
                .fetch_one(&pool)
                .await
                .expect("count group_journeys");
        assert_eq!(
            group_journeys_left, 0,
            "the group-sharing grant must be gone via ON DELETE CASCADE"
        );

        sqlx::query("DELETE FROM groups WHERE id = $1")
            .bind(&group_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_journey -- --ignored --test-threads=1`"]
    async fn delete_journey_removes_a_multi_leg_journey_in_one_call() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-DELETE-MULTI";
        seed_user(&pool, user_id).await;
        let journey_id = insert_journey(&pool, user_id, None)
            .await
            .expect("insert journey");
        insert_leg(
            &pool,
            journey_id,
            1,
            Some("WAT"),
            Some("RDG"),
            "2026-09-22".parse().unwrap(),
            None,
            "unmatched",
            None,
            None,
            None,
            None,
            false,
        )
        .await
        .expect("insert leg 1");
        insert_leg(
            &pool,
            journey_id,
            2,
            Some("RDG"),
            Some("BRI"),
            "2026-09-22".parse().unwrap(),
            None,
            "unmatched",
            None,
            None,
            None,
            None,
            false,
        )
        .await
        .expect("insert leg 2");

        let deleted = delete_journey(&pool, journey_id, user_id)
            .await
            .expect("delete journey");
        assert!(deleted);

        let (legs_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM journey_legs WHERE journey_id = $1")
                .bind(journey_id)
                .fetch_one(&pool)
                .await
                .expect("count journey_legs");
        assert_eq!(legs_left, 0, "both legs must be gone, not just one");

        cleanup_user(&pool, user_id).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_journey -- --ignored --test-threads=1`"]
    async fn delete_journey_a_non_owner_cannot_delete_and_it_survives() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let owner_id = "TEST-JOURNEY-DELETE-OWNER";
        let bystander_id = "TEST-JOURNEY-DELETE-BYSTANDER";
        seed_user(&pool, owner_id).await;
        seed_user(&pool, bystander_id).await;
        let journey_id = seed_journey(&pool, owner_id).await;

        let deleted = delete_journey(&pool, journey_id, bystander_id)
            .await
            .expect("attempt delete as non-owner");
        assert!(!deleted);

        let (journeys_left,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM journeys WHERE id = $1")
                .bind(journey_id)
                .fetch_one(&pool)
                .await
                .expect("count journeys");
        assert_eq!(
            journeys_left, 1,
            "the owner's journey must genuinely survive a non-owner's delete attempt"
        );

        cleanup_journey(&pool, journey_id, &[owner_id, bystander_id]).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_journey -- --ignored --test-threads=1`"]
    async fn delete_journey_a_nonexistent_journey_returns_false() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let deleted = delete_journey(&pool, 99_999_999, "TEST-JOURNEY-DELETE-NOBODY")
            .await
            .expect("attempt delete of a nonexistent journey");
        assert!(!deleted);
    }
    /// Finding 3's api-side regression test: deleting a template-minted
    /// occurrence must leave a `(template_id, service_date)` tombstone behind,
    /// or `crates/notifier`'s recurrence sweep re-mints that occurrence on its
    /// next tick -- within the hour -- and starts pushing again for a journey
    /// the user explicitly discarded. Covers both delete paths (whole journey,
    /// and last-leg-takes-the-journey-with-it), the "not this caller's
    /// journey" case (no tombstone at all), and an ordinary non-template
    /// journey (nothing to tombstone).
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                deleting_a_template_occurrence -- --ignored --test-threads=1`"]
    async fn deleting_a_template_occurrence_records_a_skip_day_so_the_sweep_cannot_remint_it() {
        let pool = connect().await;
        let _trains = crate::test_support::fixture_trains_cleanup(&pool).await;
        let user_id = "TEST-JOURNEY-SKIPDAY-OWNER";
        let bystander_id = "TEST-JOURNEY-SKIPDAY-BYSTANDER";
        seed_user(&pool, user_id).await;
        seed_user(&pool, bystander_id).await;
        let service_date: NaiveDate = "2026-09-25".parse().unwrap();

        let template_id: i64 = sqlx::query_scalar(
            "INSERT INTO journey_templates (user_id, custom_name, default_match_mode) \
             VALUES ($1, 'Skip Day Fixture', 'auto') RETURNING id",
        )
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("seed template");

        async fn seed_occurrence(
            pool: &PgPool,
            user_id: &str,
            template_id: i64,
            service_date: NaiveDate,
            legs: i32,
        ) -> i64 {
            let journey_id: i64 = sqlx::query_scalar(
                "INSERT INTO journeys (user_id, custom_name, source_template_id) \
                 VALUES ($1, NULL, $2) RETURNING id",
            )
            .bind(user_id)
            .bind(template_id)
            .fetch_one(pool)
            .await
            .expect("seed occurrence");
            for leg_order in 1..=legs {
                insert_leg(
                    pool,
                    journey_id,
                    leg_order,
                    Some("RDG"),
                    Some("WOK"),
                    service_date,
                    None,
                    "unmatched",
                    None,
                    None,
                    None,
                    None,
                    // A template-materialized occurrence -- always a search,
                    // even a fully-open one (see the window_searched column's
                    // own migration doc comment).
                    true,
                )
                .await
                .expect("seed occurrence leg");
            }
            journey_id
        }

        async fn tombstones(pool: &PgPool, template_id: i64) -> Vec<NaiveDate> {
            sqlx::query_scalar(
                "SELECT service_date FROM journey_template_skipped_dates \
                 WHERE template_id = $1 ORDER BY service_date",
            )
            .bind(template_id)
            .fetch_all(pool)
            .await
            .expect("read tombstones")
        }

        // 1. A non-owner's failed delete must leave no trace at all --
        //    otherwise anyone could suppress someone else's commute.
        let journey_id = seed_occurrence(&pool, user_id, template_id, service_date, 1).await;
        assert!(
            !delete_journey(&pool, journey_id, bystander_id)
                .await
                .expect("non-owner delete attempt")
        );
        assert!(
            tombstones(&pool, template_id).await.is_empty(),
            "a delete that deleted nothing must not record a skip day"
        );

        // 2. The owner deleting the whole occurrence records the skip day.
        assert!(
            delete_journey(&pool, journey_id, user_id)
                .await
                .expect("owner delete")
        );
        assert_eq!(
            tombstones(&pool, template_id).await,
            vec![service_date],
            "deleting today's occurrence must record THIS date -- and only this date, so the same \
             commute still materializes tomorrow"
        );

        // 3. Removing the LAST leg also removes the occurrence, so it must
        //    tombstone too -- for a different date, proving the scoping.
        let other_date: NaiveDate = "2026-09-26".parse().unwrap();
        let single_leg_journey = seed_occurrence(&pool, user_id, template_id, other_date, 1).await;
        let leg_id: i64 = sqlx::query_scalar("SELECT id FROM journey_legs WHERE journey_id = $1")
            .bind(single_leg_journey)
            .fetch_one(&pool)
            .await
            .expect("read the single leg");
        let journey_also_deleted = delete_leg(&pool, single_leg_journey, leg_id, user_id)
            .await
            .expect("delete the last leg")
            .expect("the leg must have existed");
        assert!(journey_also_deleted);
        assert_eq!(
            tombstones(&pool, template_id).await,
            vec![service_date, other_date]
        );

        // 4. Removing ONE leg of a two-leg occurrence leaves the occurrence
        //    standing, so there is nothing to suppress -- the sweep's own
        //    "already has a leg on this date" guard still sees it.
        let third_date: NaiveDate = "2026-09-27".parse().unwrap();
        let two_leg_journey = seed_occurrence(&pool, user_id, template_id, third_date, 2).await;
        let first_leg_id: i64 = sqlx::query_scalar(
            "SELECT id FROM journey_legs WHERE journey_id = $1 ORDER BY leg_order LIMIT 1",
        )
        .bind(two_leg_journey)
        .fetch_one(&pool)
        .await
        .expect("read the first leg");
        let also_deleted = delete_leg(&pool, two_leg_journey, first_leg_id, user_id)
            .await
            .expect("delete one of two legs")
            .expect("the leg must have existed");
        assert!(!also_deleted);
        assert_eq!(
            tombstones(&pool, template_id).await,
            vec![service_date, other_date],
            "removing one leg of a surviving occurrence must NOT record a skip day"
        );

        // 5. An ordinary, non-template journey has no occurrence to suppress.
        let plain_journey = seed_journey(&pool, user_id).await;
        assert!(
            delete_journey(&pool, plain_journey, user_id)
                .await
                .expect("delete a plain journey")
        );
        assert_eq!(
            tombstones(&pool, template_id).await,
            vec![service_date, other_date]
        );

        sqlx::query("DELETE FROM journeys WHERE source_template_id = $1")
            .bind(template_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM journey_templates WHERE id = $1")
            .bind(template_id)
            .execute(&pool)
            .await
            .ok(); // cascades journey_template_legs + journey_template_skipped_dates
        cleanup_user(&pool, user_id).await;
        cleanup_user(&pool, bystander_id).await;
    }
}
