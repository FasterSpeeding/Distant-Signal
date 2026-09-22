//! Journey tracking (Phase 1): a `journeys` row groups one-or-more
//! `journey_legs`, each either bound to a real `train_subscriptions` row
//! (a "matched" leg) or an open time-window search waiting for a manual
//! pick (an "unmatched" leg). See
//! docs/superpowers/specs/2026-09-22-journey-tracking-design.md and
//! docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md.
//!
//! **Phase 1 never creates more than one leg per journey.** Every
//! leg-creation function in this file hardcodes `leg_order = 1` and says so
//! in its own doc comment -- multi-leg chaining (design doc §3, "add a leg
//! to an existing journey") is a later phase's job. The schema itself
//! (`leg_order`, `UNIQUE (journey_id, leg_order)`) is already
//! multi-leg-shaped regardless, per the design doc's own reasoning for not
//! folding leg fields onto `train_subscriptions` (§1.1) -- so that later
//! phase needs no schema change, only a new writer.

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

/// `leg_order` is always `1` -- see this module's own doc comment on why
/// Phase 1 never writes anything else.
#[allow(clippy::too_many_arguments)]
async fn insert_leg(
    pool: &PgPool,
    journey_id: i64,
    origin_crs: Option<&str>,
    destination_crs: Option<&str>,
    service_date: NaiveDate,
    train_subscription_id: Option<i64>,
    match_mode: &str,
) -> anyhow::Result<i64> {
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_legs \
            (journey_id, leg_order, origin_crs, destination_crs, service_date, \
             train_subscription_id, match_mode) \
         VALUES ($1, 1, $2, $3, $4, $5, $6) \
         RETURNING id",
    )
    .bind(journey_id)
    .bind(origin_crs)
    .bind(destination_crs)
    .bind(service_date)
    .bind(train_subscription_id)
    .bind(match_mode)
    .fetch_one(pool)
    .await?;
    Ok(id)
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
        Some(pin.origin_crs.as_str()),
        pin.destination_crs.as_deref(),
        pin.service_date,
        Some(tracking_id),
        "manual",
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
        origin_crs.as_deref(),
        destination_crs.as_deref(),
        service_date,
        Some(tracking_id),
        "manual",
    )
    .await?;
    Ok((journey_id, leg_id, tracking_id))
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
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_legs \
            (journey_id, leg_order, origin_crs, destination_crs, service_date, \
             depart_after, depart_before, arrive_after, arrive_before, match_mode) \
         VALUES ($1, 1, $2, $3, $4, $5, $6, $7, $8, 'unmatched') \
         RETURNING id",
    )
    .bind(journey_id)
    .bind(origin_crs)
    .bind(destination_crs)
    .bind(service_date)
    .bind(depart_window.after)
    .bind(depart_window.before)
    .bind(arrive_window.after)
    .bind(arrive_window.before)
    .fetch_one(pool)
    .await?;
    Ok((journey_id, id))
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
/// split. Phase 1 never creates more than one leg per journey (this
/// module's own doc comment), so this surfaces `leg_order = 1`'s own
/// fields directly rather than a nested array -- a genuine multi-leg
/// rollup (design doc §3's "worst status across legs" idiom) is a later
/// phase's job, once a journey can actually have more than one leg.
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
/// this list's own cap should agree with").
pub async fn list_journeys_for_user(
    pool: &PgPool,
    user_id: &str,
) -> anyhow::Result<Vec<JourneyListItem>> {
    let rows = sqlx::query_as::<_, JourneyListItem>(
        "SELECT j.id, j.custom_name, j.created_at, \
                jl.id AS leg_id, jl.origin_crs, jl.destination_crs, jl.match_mode, \
                jl.train_subscription_id, \
                ts.resolution_status, cs.status, cs.delay_minutes \
         FROM journeys j \
         JOIN journey_legs jl ON jl.journey_id = j.id AND jl.leg_order = 1 \
         LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = ts.trains_id \
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
        "SELECT id, custom_name, created_at FROM journeys WHERE id = $1",
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
/// `journeys.user_id = caller.id` alone, via `journey_owner` (this file's
/// existing ownership-only check, unchanged by this feature). Sharing a
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
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_the_owner_can_always_read_their_own_journey_with_no_grant() {
        let pool = connect().await;
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
            &["TEST-JOURNEY-READABLE-OWNER-2", "TEST-JOURNEY-READABLE-MEMBER-2"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_excludes_a_stranger_in_no_shared_group() {
        let pool = connect().await;
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
            &["TEST-JOURNEY-READABLE-OWNER-3", "TEST-JOURNEY-READABLE-STRANGER-3"],
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                journey_readable_by -- --ignored --test-threads=1`"]
    async fn journey_readable_by_a_former_member_loses_access_once_removed() {
        let pool = connect().await;
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

        crate::data::groups::remove_member(&pool, &group_id, "TEST-JOURNEY-READABLE-MEMBER-4")
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
            &["TEST-JOURNEY-READABLE-OWNER-4", "TEST-JOURNEY-READABLE-MEMBER-4"],
        )
        .await;
    }
}
