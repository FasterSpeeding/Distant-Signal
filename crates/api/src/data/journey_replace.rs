//! Replacing part of a journey's plan (docs/superpowers/specs/
//! 2026-10-22-mid-journey-changes-design.md §4.4): one transaction under
//! the journey row lock that
//!
//! 1. makes sure the plan as it stands is recorded (revision 1 written or
//!    refreshed if it is still the only one);
//! 2. keeps every leg before the replaced run (same ids, so notification
//!    state, links and the current-leg logic stay put);
//! 3. deletes the replaced legs and inserts the new ones in their place,
//!    moving the later legs along (the two-step renumber
//!    `owned_leg_order_at` uses);
//! 4. releases each replaced train's subscription (DS-93), moving its
//!    tickets to the new train of the same operator (else the first new
//!    train), or keeping them as standalone tickets when there is none:
//!    a replacement never cascades a ticket away;
//! 5. writes the next revision (or, while the journey is still being
//!    planned, refreshes revision 1: §4.3) and resolves the break.
//!
//! The new legs are built by `journeys::insert_itinerary_leg`, as
//! `POST /Journeys/itinerary` builds them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::data::journey_revisions::{self, RevisionMeta};
use crate::data::journeys::{self, ItineraryLeg, MAX_LEGS_PER_JOURNEY};

/// One replacement.
#[derive(Debug, Clone)]
pub struct Replacement<'a> {
    pub journey_id: i64,
    pub user_id: &'a str,
    /// The first leg replaced, and the last (inclusive); `None`: just the
    /// first.
    pub from_leg_id: i64,
    pub through_leg_id: Option<i64>,
    pub legs: &'a [ItineraryLeg],
    /// What the new revision records; its `break_id` is resolved
    /// `accepted`.
    pub meta: RevisionMeta,
    /// What happened to the first replaced leg (its fate in the
    /// superseded plan).
    pub trigger_fate: &'a str,
    pub now: DateTime<Utc>,
    /// An `Idempotency-Key` and the request's hash: a repeat answers with
    /// the first response (`journey_idempotency_keys`).
    pub idempotency: Option<(&'a str, &'a str)>,
}

/// What a replacement answers (`POST .../replan/accept`, `.../replace`,
/// `.../revert`), and what an `Idempotency-Key` replays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceResponse {
    pub journey_id: i64,
    /// The revision now current (1 when revision 1 was refreshed).
    pub revision_no: i32,
    pub new_leg_ids: Vec<i64>,
    /// The revision an Undo goes back to; `None` when the change refreshed
    /// the booked plan (nothing to undo to).
    pub undo_revision_no: Option<i32>,
}

/// What [`replace`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replaced {
    /// No such journey or leg, or not the caller's.
    NotFound,
    /// `through_leg_id` comes before `from_leg_id`.
    BadRange,
    /// A replaced leg is already travelled (frozen, §11).
    Travelled,
    /// The journey would have more than [`MAX_LEGS_PER_JOURNEY`] legs.
    AtCap,
    /// The same `Idempotency-Key` with a different request.
    IdempotencyConflict,
    /// The same `Idempotency-Key` and request: its first response.
    Replay(serde_json::Value),
    Done {
        /// The revision written; `None` when revision 1 was refreshed.
        revision_id: Option<i64>,
        response: ReplaceResponse,
    },
}

#[derive(Debug, sqlx::FromRow)]
struct LegRow {
    id: i64,
    leg_order: i32,
    train_subscription_id: Option<i64>,
}

/// Replaces `from_leg_id ..= through_leg_id` of the journey with `legs`.
/// See the module doc.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear: one transaction, step by step as the module doc lists"
)]
pub async fn replace(pool: &PgPool, replacement: &Replacement<'_>) -> anyhow::Result<Replaced> {
    let Replacement {
        journey_id,
        user_id,
        from_leg_id,
        through_leg_id,
        legs,
        ref meta,
        trigger_fate,
        now,
        idempotency,
    } = *replacement;
    let mut tx = pool.begin().await?;
    if let Some((key, hash)) = idempotency {
        match crate::data::delegated::claim_idempotency_key(&mut tx, user_id, key, hash).await? {
            crate::data::delegated::IdempotencyClaim::Fresh => {}
            crate::data::delegated::IdempotencyClaim::Replay(response) => {
                return Ok(Replaced::Replay(response));
            }
            crate::data::delegated::IdempotencyClaim::Conflict => {
                return Ok(Replaced::IdempotencyConflict);
            }
        }
    }
    let owned: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM journeys WHERE id = $1 AND user_id = $2 FOR UPDATE")
            .bind(journey_id)
            .bind(user_id)
            .fetch_optional(&mut *tx)
            .await?;
    if owned.is_none() {
        return Ok(Replaced::NotFound);
    }
    let current: Vec<LegRow> = sqlx::query_as(
        "SELECT id, leg_order, train_subscription_id FROM journey_legs \
         WHERE journey_id = $1 ORDER BY leg_order FOR UPDATE",
    )
    .bind(journey_id)
    .fetch_all(&mut *tx)
    .await?;
    let Some(from) = current.iter().position(|leg| leg.id == from_leg_id) else {
        return Ok(Replaced::NotFound);
    };
    let through = match through_leg_id {
        None => from,
        Some(id) => match current.iter().position(|leg| leg.id == id) {
            None => return Ok(Replaced::NotFound),
            Some(index) if index < from => return Ok(Replaced::BadRange),
            Some(index) => index,
        },
    };
    let replaced = &current[from..=through];
    let replaced_ids: Vec<i64> = replaced.iter().map(|leg| leg.id).collect();
    if !crate::data::journey_breaks::travelled_legs(&mut tx, &replaced_ids)
        .await?
        .is_empty()
    {
        return Ok(Replaced::Travelled);
    }
    let remaining = current.len() - replaced.len() + legs.len();
    if i64::try_from(remaining).unwrap_or(i64::MAX) > MAX_LEGS_PER_JOURNEY || remaining == 0 {
        return Ok(Replaced::AtCap);
    }

    // Still being planned (no break, not started): this is a planning edit
    // and refreshes revision 1. Otherwise record the plan as it stands
    // first, so the revision this writes supersedes it.
    let planning = meta.break_id.is_none()
        && journey_revisions::still_planning(&mut tx, journey_id, now).await?;
    let trigger_position = if planning {
        None
    } else {
        let (previous_id, _) = journey_revisions::prepare_replacement(&mut tx, journey_id).await?;
        let position: Option<(i32,)> = sqlx::query_as(
            "SELECT position FROM journey_revision_legs \
             WHERE revision_id = $1 AND source_leg_id = $2",
        )
        .bind(previous_id)
        .bind(from_leg_id)
        .fetch_optional(&mut *tx)
        .await?;
        Some(position.map_or(i32::try_from(from + 1)?, |(p,)| p))
    };

    let from_order = replaced[0].leg_order;
    let through_order = replaced[replaced.len() - 1].leg_order;
    sqlx::query("DELETE FROM journey_legs WHERE id = ANY($1)")
        .bind(&replaced_ids)
        .execute(&mut *tx)
        .await?;
    // Later legs move along by the difference, through negative values
    // (`UNIQUE (journey_id, leg_order)` is checked row by row). The
    // replaced run spans at least its own length (orders are distinct), so
    // they land at or after `from_order + legs.len()`, clear of the new
    // legs.
    let shift = i32::try_from(legs.len())? - i32::try_from(replaced.len())?;
    if shift != 0 {
        sqlx::query(
            "UPDATE journey_legs SET leg_order = -(leg_order + $3) \
             WHERE journey_id = $1 AND leg_order > $2",
        )
        .bind(journey_id)
        .bind(through_order)
        .bind(shift)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE journey_legs SET leg_order = -leg_order WHERE journey_id = $1 AND leg_order < 0",
        )
        .bind(journey_id)
        .execute(&mut *tx)
        .await?;
    }
    let mut new_leg_ids = Vec::with_capacity(legs.len());
    let mut new_subscriptions: Vec<i64> = Vec::new();
    for (offset, leg) in legs.iter().enumerate() {
        let (leg_id, tracking_id) = journeys::insert_itinerary_leg(
            &mut tx,
            journey_id,
            user_id,
            from_order + i32::try_from(offset)?,
            leg,
        )
        .await?;
        new_leg_ids.push(leg_id);
        new_subscriptions.extend(tracking_id);
    }

    // Each replaced train's subscription is released; its tickets go to
    // the new train run by the same operator, else the first new train.
    let mut released: Vec<i64> = Vec::new();
    for old in replaced.iter().filter_map(|leg| leg.train_subscription_id) {
        if released.contains(&old) || new_subscriptions.contains(&old) {
            continue;
        }
        released.push(old);
        let move_to = same_operator_subscription(&mut tx, old, &new_subscriptions)
            .await?
            .or_else(|| new_subscriptions.first().copied());
        journeys::release_leg_subscription(&mut tx, user_id, old, move_to, true).await?;
    }

    let previous_no: Option<(i32,)> =
        sqlx::query_as("SELECT MAX(revision_no) FROM journey_revisions WHERE journey_id = $1")
            .bind(journey_id)
            .fetch_optional(&mut *tx)
            .await?;
    let previous_no = previous_no.map(|(no,)| no);
    let (revision_id, revision_no) = if let Some(position) = trigger_position {
        let id = journey_revisions::append_revision(
            &mut tx,
            journey_id,
            meta,
            Some((position, trigger_fate)),
            now,
        )
        .await?;
        let (no,): (i32,) =
            sqlx::query_as("SELECT revision_no FROM journey_revisions WHERE id = $1")
                .bind(id)
                .fetch_one(&mut *tx)
                .await?;
        (Some(id), no)
    } else {
        journey_revisions::record_plan_change(&mut tx, journey_id, now).await?;
        (None, 1)
    };
    if let Some(break_id) = meta.break_id {
        crate::data::journey_breaks::resolve(&mut tx, break_id, "accepted", revision_id, None)
            .await?;
    }
    sqlx::query("UPDATE journeys SET updated_at = now() WHERE id = $1")
        .bind(journey_id)
        .execute(&mut *tx)
        .await?;
    let response = ReplaceResponse {
        journey_id,
        revision_no,
        new_leg_ids,
        undo_revision_no: revision_id.and(previous_no),
    };
    if let Some((key, _)) = idempotency {
        crate::data::delegated::record_idempotent_response(
            &mut tx,
            user_id,
            key,
            &serde_json::to_value(&response)?,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(Replaced::Done {
        revision_id,
        response,
    })
}

/// The stored response of an earlier request with the same
/// `Idempotency-Key` and body, if any (an accept retried after it went
/// through).
pub async fn replay(
    pool: &PgPool,
    user_id: &str,
    key: &str,
    request_hash: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let row: Option<(Option<serde_json::Value>,)> = sqlx::query_as(
        "SELECT response FROM journey_idempotency_keys \
         WHERE user_id = $1 AND idempotency_key = $2 AND request_hash = $3",
    )
    .bind(user_id)
    .bind(key)
    .bind(request_hash)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(response,)| response))
}

/// Of `candidates` (new subscriptions), the first whose train has the same
/// operator as subscription `old`'s train, by the timetable's operator.
async fn same_operator_subscription(
    conn: &mut sqlx::PgConnection,
    old: i64,
    candidates: &[i64],
) -> anyhow::Result<Option<i64>> {
    if candidates.is_empty() {
        return Ok(None);
    }
    let rows: Vec<(i64, Option<String>)> = sqlx::query_as(
        "SELECT ts.id, (SELECT MIN(sdd.operator_atoc) FROM schedule_destination_departures sdd \
                         WHERE sdd.service_date = tr.service_date AND sdd.train_uid = tr.train_uid) \
         FROM train_subscriptions ts JOIN trains tr ON tr.id = ts.trains_id \
         WHERE ts.id = ANY($1)",
    )
    .bind(
        std::iter::once(old)
            .chain(candidates.iter().copied())
            .collect::<Vec<_>>(),
    )
    .fetch_all(&mut *conn)
    .await?;
    let operator_of =
        |id: i64| -> Option<&str> { rows.iter().find(|(r, _)| *r == id)?.1.as_deref() };
    let Some(old_operator) = operator_of(old) else {
        return Ok(None);
    };
    Ok(candidates
        .iter()
        .copied()
        .find(|id| operator_of(*id) == Some(old_operator)))
}
