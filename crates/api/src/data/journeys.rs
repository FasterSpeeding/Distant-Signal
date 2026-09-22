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
