//! Journey revisions: append-only snapshots of a journey's whole plan
//! (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md §4).
//!
//! `journey_legs` is the current plan; a revision is the whole plan at one
//! moment, with its legs denormalised (train UID, date, stations, booked
//! public times, ticket ids) so it outlives the subscriptions and `trains`
//! rows a later change or retention deletes.
//!
//! - **Revision 1 is the booked plan.** Every journey-creating path writes
//!   it in its own transaction ([`record_plan_change`]), and every planning
//!   edit (pick or change a train, add, insert, remove or edit a leg)
//!   refreshes it while the journey has not started and has never broken
//!   (§4.3). A refresh keeps the booked times of a leg whose train and
//!   stations are unchanged, so a timetable change after booking does not
//!   rewrite what was booked.
//! - **After the journey starts, or once it has broken,** every change
//!   appends the next revision ([`append_revision`]) and never edits an
//!   older one. The superseded revision's legs get their `fate`, once
//!   ([`supersede`]).
//! - **A journey with no revision** (made by an older release during a
//!   rollout) is read as "revision 1 is the current plan": a change writes
//!   revision 1 first ([`ensure_booked`]).
//!
//! The compensation baseline (§9.2) starts on revision 1 and moves to a
//! passenger-fault or chosen revision.

use std::collections::HashSet;

use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// The legs of journey `$1` as a snapshot for revision `$2`: one
/// `journey_revision_legs` row per leg, in `leg_order`. The same SELECT
/// as the backfill migration (`20261023090500_journey_revisions_backfill.sql`),
/// so a snapshot never depends on which wrote it. Booked times are the
/// train's public (else working) times at the leg's stations, else the
/// pin's departure; a leg with no train yet gets none.
const SNAPSHOT_LEGS: &str = "\
INSERT INTO journey_revision_legs \
    (revision_id, position, source_leg_id, kind, train_uid, service_date, trains_id, \
     atoc_code, origin_crs, destination_crs, booked_departure, booked_arrival, \
     transit_mode, origin_place, destination_place, line_name, \
     planned_departure, planned_arrival, ticket_ids, \
     origin_stop_id, destination_stop_id, transit_network, transit_line_id, headsign) \
SELECT $2, \
       ROW_NUMBER() OVER (ORDER BY jl.leg_order)::INT, \
       jl.id, jl.kind, tr.train_uid, jl.service_date, ts.trains_id, \
       (SELECT MIN(sdd.operator_atoc) FROM schedule_destination_departures sdd \
         WHERE sdd.service_date = tr.service_date AND sdd.train_uid = tr.train_uid), \
       ends.origin_crs, ends.destination_crs, \
       COALESCE( \
           (SELECT ((cp.service_date + cp.day_offset::INT) \
                    + COALESCE(cp.public_departure, cp.booked_departure)) \
                   AT TIME ZONE 'Europe/London' \
              FROM schedule_calling_points_full cp \
             WHERE cp.service_date = tr.service_date AND cp.uid = tr.train_uid \
               AND COALESCE(cp.public_departure, cp.booked_departure) IS NOT NULL \
               AND UPPER(TRIM(cp.tiploc)) IN ( \
                   SELECT UPPER(TRIM(tc.tiploc)) FROM tiploc_crs tc \
                    WHERE UPPER(tc.crs) = ends.origin_crs \
                   UNION \
                   SELECT UPPER(TRIM(sc.tiploc)) FROM stanox_crs sc \
                    WHERE UPPER(sc.crs) = ends.origin_crs) \
             ORDER BY cp.seq LIMIT 1), \
           ts.pin_scheduled_departure), \
       (SELECT ((cp.service_date + cp.day_offset::INT) \
                + COALESCE(cp.public_arrival, cp.booked_arrival)) \
               AT TIME ZONE 'Europe/London' \
          FROM schedule_calling_points_full cp \
         WHERE cp.service_date = tr.service_date AND cp.uid = tr.train_uid \
           AND COALESCE(cp.public_arrival, cp.booked_arrival) IS NOT NULL \
           AND UPPER(TRIM(cp.tiploc)) IN ( \
               SELECT UPPER(TRIM(tc.tiploc)) FROM tiploc_crs tc \
                WHERE UPPER(tc.crs) = ends.destination_crs \
               UNION \
               SELECT UPPER(TRIM(sc.tiploc)) FROM stanox_crs sc \
                WHERE UPPER(sc.crs) = ends.destination_crs) \
         ORDER BY cp.seq DESC LIMIT 1), \
       jl.transit_mode, jl.origin_place, jl.destination_place, jl.line_name, \
       jl.planned_departure, jl.planned_arrival, \
       COALESCE( \
           (SELECT array_agg(t.id ORDER BY t.id) FROM tracked_train_tickets t \
             WHERE t.tracked_train_id = jl.train_subscription_id), \
           '{}'), \
       jl.origin_stop_id, jl.destination_stop_id, jl.transit_network, jl.transit_line_id, \
       jl.headsign \
FROM journey_legs jl \
LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
LEFT JOIN trains tr ON tr.id = ts.trains_id \
CROSS JOIN LATERAL ( \
    SELECT UPPER(COALESCE(jl.origin_crs, ts.pin_origin_crs, tr.origin_crs)) AS origin_crs, \
           UPPER(COALESCE(jl.destination_crs, ts.pin_destination_crs, tr.destination_crs)) \
               AS destination_crs \
) ends \
WHERE jl.journey_id = $1";

/// Why a revision was written (`journey_revisions.reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Booked,
    Replace,
    AutoShift,
    Revert,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Booked => "booked",
            Reason::Replace => "replace",
            Reason::AutoShift => "auto_shift",
            Reason::Revert => "revert",
        }
    }
}

/// What a new (non-booked) revision records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RevisionMeta {
    pub reason: Reason,
    /// `journey_breaks.cause` vocabulary, or `user_choice`.
    pub cause: String,
    /// `operator`, `user`, `none` or `unknown`.
    pub fault: String,
    pub break_id: Option<i64>,
    /// `owner` or `system`.
    pub actor: &'static str,
}

impl RevisionMeta {
    /// A change the owner made with no break open (§4.2's `user_choice`).
    pub fn user_choice() -> Self {
        Self {
            reason: Reason::Replace,
            cause: "user_choice".to_string(),
            fault: "none".to_string(),
            break_id: None,
            actor: "owner",
        }
    }

    /// Whether this revision becomes the compensation baseline (§9.2): a
    /// passenger-fault or chosen change moves it; a railway-fault one
    /// leaves it on the booked plan.
    fn moves_baseline(&self) -> bool {
        matches!(self.fault.as_str(), "user" | "none") && self.reason != Reason::Revert
    }
}

/// The latest revision of a journey: `(id, revision_no)`.
async fn latest(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
) -> anyhow::Result<Option<(i64, i32)>> {
    Ok(sqlx::query_as(
        "SELECT id, revision_no FROM journey_revisions WHERE journey_id = $1 \
         ORDER BY revision_no DESC LIMIT 1",
    )
    .bind(journey_id)
    .fetch_optional(&mut *conn)
    .await?)
}

async fn insert_revision(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
    revision_no: i32,
    meta: Option<&RevisionMeta>,
) -> anyhow::Result<i64> {
    let (reason, cause, fault, break_id, actor) = match meta {
        None => (Reason::Booked.as_str(), None, None, None, "owner"),
        Some(meta) => (
            meta.reason.as_str(),
            Some(meta.cause.as_str()),
            Some(meta.fault.as_str()),
            meta.break_id,
            meta.actor,
        ),
    };
    let (id,): (i64,) = sqlx::query_as(
        "INSERT INTO journey_revisions \
             (journey_id, revision_no, reason, cause, fault, break_id, actor, is_compensation_baseline) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(journey_id)
    .bind(revision_no)
    .bind(reason)
    .bind(cause)
    .bind(fault)
    .bind(break_id)
    .bind(actor)
    .bind(meta.is_none())
    .fetch_one(&mut *conn)
    .await?;
    sqlx::query(SNAPSHOT_LEGS)
        .bind(journey_id)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(id)
}

/// Revision 1 for a journey with none, from its current legs; the latest
/// revision's id either way. The caller holds the journey row lock.
pub async fn ensure_booked(conn: &mut sqlx::PgConnection, journey_id: i64) -> anyhow::Result<i64> {
    if let Some((id, _)) = latest(conn, journey_id).await? {
        return Ok(id);
    }
    insert_revision(conn, journey_id, 1, None).await
}

/// Rewrites revision `revision_id`'s legs from the journey's current legs,
/// keeping the booked times of every leg whose train, date and stations
/// are unchanged (or, for an untracked leg, whose planned times are).
async fn refresh(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
    revision_id: i64,
) -> anyhow::Result<()> {
    type Kept = (
        Option<String>,
        NaiveDate,
        Option<String>,
        Option<String>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );
    let kept: Vec<Kept> = sqlx::query_as(
        "SELECT train_uid, service_date, origin_crs, destination_crs, booked_departure, booked_arrival \
         FROM journey_revision_legs WHERE revision_id = $1 AND train_uid IS NOT NULL",
    )
    .bind(revision_id)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query("DELETE FROM journey_revision_legs WHERE revision_id = $1")
        .bind(revision_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query(SNAPSHOT_LEGS)
        .bind(journey_id)
        .bind(revision_id)
        .execute(&mut *conn)
        .await?;
    for (uid, date, origin, destination, departure, arrival) in kept {
        if departure.is_none() && arrival.is_none() {
            continue;
        }
        sqlx::query(
            "UPDATE journey_revision_legs \
             SET booked_departure = COALESCE($6, booked_departure), \
                 booked_arrival = COALESCE($7, booked_arrival) \
             WHERE revision_id = $1 AND train_uid = $2 AND service_date = $3 \
               AND origin_crs IS NOT DISTINCT FROM $4 AND destination_crs IS NOT DISTINCT FROM $5",
        )
        .bind(revision_id)
        .bind(uid)
        .bind(date)
        .bind(origin)
        .bind(destination)
        .bind(departure)
        .bind(arrival)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Whether a journey's plan is still being planned (§4.3): it has only
/// revision 1 (or none), has never broken, and its first leg has not left.
/// Started means a booked or planned departure at or before `now`, or a
/// leg dated before today in London.
pub async fn still_planning(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
    now: DateTime<Utc>,
) -> anyhow::Result<bool> {
    let (planning,): (bool,) = sqlx::query_as(
        "SELECT NOT EXISTS (SELECT 1 FROM journey_revisions \
                             WHERE journey_id = $1 AND revision_no > 1) \
            AND NOT EXISTS (SELECT 1 FROM journey_breaks WHERE journey_id = $1) \
            AND NOT EXISTS (SELECT 1 FROM journey_revisions r \
                              JOIN journey_revision_legs l ON l.revision_id = r.id \
                             WHERE r.journey_id = $1 AND r.revision_no = 1 \
                               AND COALESCE(l.booked_departure, l.planned_departure) <= $2) \
            AND NOT EXISTS (SELECT 1 FROM journey_legs \
                             WHERE journey_id = $1 \
                               AND (service_date < ($2 AT TIME ZONE 'Europe/London')::date \
                                    OR planned_departure <= $2))",
    )
    .bind(journey_id)
    .bind(now)
    .fetch_one(&mut *conn)
    .await?;
    Ok(planning)
}

/// One plan's legs as compared for "did anything change".
async fn plan_signature(
    conn: &mut sqlx::PgConnection,
    revision_id: i64,
) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT concat_ws('|', source_leg_id, kind, train_uid, service_date, origin_crs, \
                          destination_crs, transit_mode, origin_place, destination_place, \
                          planned_departure, planned_arrival) \
         FROM journey_revision_legs WHERE revision_id = $1 ORDER BY position",
    )
    .bind(revision_id)
    .fetch_all(&mut *conn)
    .await?)
}

async fn current_signature(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
) -> anyhow::Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "SELECT concat_ws('|', jl.id, jl.kind, tr.train_uid, jl.service_date, \
                          UPPER(COALESCE(jl.origin_crs, ts.pin_origin_crs, tr.origin_crs)), \
                          UPPER(COALESCE(jl.destination_crs, ts.pin_destination_crs, tr.destination_crs)), \
                          jl.transit_mode, jl.origin_place, jl.destination_place, \
                          jl.planned_departure, jl.planned_arrival) \
         FROM journey_legs jl \
         LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         WHERE jl.journey_id = $1 ORDER BY jl.leg_order",
    )
    .bind(journey_id)
    .fetch_all(&mut *conn)
    .await?)
}

/// Appends the next revision, a snapshot of the journey's current legs,
/// and sets the superseded revision's fates ([`supersede`]). The caller
/// holds the journey row lock and has already changed `journey_legs`.
/// `break_position` is the 1-based position, in the superseded revision,
/// of the first leg that no longer works (the trigger), and `trigger_fate`
/// what happened to it; `None` for a change with no break.
pub async fn append_revision(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
    meta: &RevisionMeta,
    trigger: Option<(i32, &str)>,
    now: DateTime<Utc>,
) -> anyhow::Result<i64> {
    let previous = latest(conn, journey_id).await?;
    let next_no = previous.map_or(1, |(_, no)| no + 1);
    let id = insert_revision(conn, journey_id, next_no, Some(meta)).await?;
    if let Some((previous_id, _)) = previous {
        supersede(conn, previous_id, id, trigger, now).await?;
    }
    if meta.moves_baseline() {
        sqlx::query(
            "UPDATE journey_revisions SET is_compensation_baseline = (id = $2) \
             WHERE journey_id = $1 AND is_compensation_baseline <> (id = $2)",
        )
        .bind(journey_id)
        .bind(id)
        .execute(&mut *conn)
        .await?;
    }
    Ok(id)
}

/// Sets the fates of `previous`'s legs, once, from `planned`: a leg the
/// new plan `next` keeps (same `source_leg_id`) before the trigger is
/// `travelled` once its booked arrival has passed; the trigger leg gets
/// `trigger_fate`; a kept leg after it stays `planned` (it lives on in the
/// next revision); every other leg is `not_used`.
async fn supersede(
    conn: &mut sqlx::PgConnection,
    previous: i64,
    next: i64,
    trigger: Option<(i32, &str)>,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let (trigger_position, trigger_fate) = trigger.unwrap_or((i32::MAX, "not_used"));
    sqlx::query(
        "UPDATE journey_revision_legs p SET fate = CASE \
             WHEN p.position = $3 THEN $4 \
             WHEN p.source_leg_id IN (SELECT source_leg_id FROM journey_revision_legs n \
                                       WHERE n.revision_id = $2 AND n.source_leg_id IS NOT NULL) \
                  THEN CASE WHEN p.position < $3 \
                             AND COALESCE(p.booked_arrival, p.planned_arrival) <= $5 \
                            THEN 'travelled' ELSE 'planned' END \
             ELSE 'not_used' END \
         WHERE p.revision_id = $1 AND p.fate = 'planned'",
    )
    .bind(previous)
    .bind(next)
    .bind(trigger_position)
    .bind(trigger_fate)
    .bind(now)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// The hook every journey-creating and planning-edit path calls on its own
/// transaction after changing `journey_legs` (§4.3), with the journey row
/// locked (it takes the lock itself if not):
///
/// - no revision yet: writes revision 1, the booked plan;
/// - still planning ([`still_planning`]): refreshes revision 1;
/// - otherwise: appends a `user_choice` revision, unless the plan is
///   unchanged since the latest one.
///
/// A deleted journey has nothing to record.
pub async fn record_plan_change(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let exists: Option<(i64,)> = sqlx::query_as("SELECT id FROM journeys WHERE id = $1 FOR UPDATE")
        .bind(journey_id)
        .fetch_optional(&mut *conn)
        .await?;
    if exists.is_none() {
        return Ok(());
    }
    let Some((latest_id, latest_no)) = latest(conn, journey_id).await? else {
        insert_revision(conn, journey_id, 1, None).await?;
        return Ok(());
    };
    if latest_no == 1 && still_planning(conn, journey_id, now).await? {
        return refresh(conn, journey_id, latest_id).await;
    }
    if plan_signature(conn, latest_id).await? == current_signature(conn, journey_id).await? {
        return Ok(());
    }
    append_revision(conn, journey_id, &RevisionMeta::user_choice(), None, now).await?;
    Ok(())
}

/// Before a replacement (§4.4): revision 1 written if missing, and while
/// it is the only revision, refreshed from the legs as they are now (a
/// train the notifier bound to a template leg, say), keeping booked times.
/// Returns the latest revision's id and number.
pub async fn prepare_replacement(
    conn: &mut sqlx::PgConnection,
    journey_id: i64,
) -> anyhow::Result<(i64, i32)> {
    match latest(conn, journey_id).await? {
        None => {
            let id = insert_revision(conn, journey_id, 1, None).await?;
            Ok((id, 1))
        }
        Some((id, 1)) => {
            refresh(conn, journey_id, id).await?;
            Ok((id, 1))
        }
        Some(found) => Ok(found),
    }
}

/// One revision leg as served by `GET /Journeys/{id}/revisions`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct RevisionLeg {
    pub position: i32,
    pub source_leg_id: Option<i64>,
    pub kind: String,
    pub train_uid: Option<String>,
    pub service_date: NaiveDate,
    pub atoc_code: Option<String>,
    pub origin_crs: Option<String>,
    pub origin_name: Option<String>,
    pub destination_crs: Option<String>,
    pub destination_name: Option<String>,
    pub booked_departure: Option<DateTime<Utc>>,
    pub booked_arrival: Option<DateTime<Utc>>,
    pub transit_mode: Option<String>,
    pub origin_place: Option<String>,
    pub destination_place: Option<String>,
    pub line_name: Option<String>,
    pub planned_departure: Option<DateTime<Utc>>,
    pub planned_arrival: Option<DateTime<Utc>>,
    pub ticket_ids: Vec<i64>,
    pub fate: String,
    /// An untracked leg's stops and network ride (DS-237, additive), so
    /// going back to this plan keeps them; `null` otherwise and on a
    /// revision written before them.
    pub origin_stop_id: Option<String>,
    pub destination_stop_id: Option<String>,
    pub transit_network: Option<String>,
    pub transit_line_id: Option<String>,
    pub headsign: Option<String>,
    #[serde(skip)]
    pub revision_id: i64,
}

/// One revision with its legs.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    #[serde(skip)]
    pub id: i64,
    pub revision_no: i32,
    pub created_at: DateTime<Utc>,
    pub reason: String,
    pub cause: Option<String>,
    pub fault: Option<String>,
    pub break_id: Option<i64>,
    pub actor: String,
    pub is_compensation_baseline: bool,
    #[sqlx(skip)]
    pub legs: Vec<RevisionLeg>,
}

/// Every revision of a journey with its legs, newest first.
pub async fn list_revisions(pool: &PgPool, journey_id: i64) -> anyhow::Result<Vec<Revision>> {
    let mut revisions: Vec<Revision> = sqlx::query_as(
        "SELECT id, revision_no, created_at, reason, cause, fault, break_id, actor, \
                is_compensation_baseline \
         FROM journey_revisions WHERE journey_id = $1 ORDER BY revision_no DESC",
    )
    .bind(journey_id)
    .fetch_all(pool)
    .await?;
    let ids: Vec<i64> = revisions.iter().map(|r| r.id).collect();
    let legs: Vec<RevisionLeg> = sqlx::query_as(
        "SELECT l.revision_id, l.position, l.source_leg_id, l.kind, l.train_uid, l.service_date, \
                l.atoc_code, l.origin_crs, so.name AS origin_name, l.destination_crs, \
                sd.name AS destination_name, l.booked_departure, l.booked_arrival, \
                l.transit_mode, l.origin_place, l.destination_place, l.line_name, \
                l.planned_departure, l.planned_arrival, l.ticket_ids, l.fate, \
                l.origin_stop_id, l.destination_stop_id, l.transit_network, \
                l.transit_line_id, l.headsign \
         FROM journey_revision_legs l \
         LEFT JOIN stations so ON so.crs = l.origin_crs \
         LEFT JOIN stations sd ON sd.crs = l.destination_crs \
         WHERE l.revision_id = ANY($1) ORDER BY l.revision_id, l.position",
    )
    .bind(&ids)
    .fetch_all(pool)
    .await?;
    for leg in legs {
        if let Some(revision) = revisions.iter_mut().find(|r| r.id == leg.revision_id) {
            revision.legs.push(leg);
        }
    }
    Ok(revisions)
}

/// What `GET /Journeys/{id}` adds from the revisions: how many there are,
/// and for each current leg that the latest revision brought in, the legs
/// (of the plan before it) it replaced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RevisionSummary {
    pub revision_count: i64,
    pub replaced_leg_ids: Vec<i64>,
    pub new_leg_ids: HashSet<i64>,
}

/// [`RevisionSummary`] for one journey.
pub async fn summary(pool: &PgPool, journey_id: i64) -> anyhow::Result<RevisionSummary> {
    let (revision_count,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM journey_revisions WHERE journey_id = $1")
            .bind(journey_id)
            .fetch_one(pool)
            .await?;
    if revision_count < 2 {
        return Ok(RevisionSummary {
            revision_count,
            ..RevisionSummary::default()
        });
    }
    let rows: Vec<(i32, Option<i64>)> = sqlx::query_as(
        "SELECT r.revision_no, l.source_leg_id FROM journey_revisions r \
         JOIN journey_revision_legs l ON l.revision_id = r.id \
         WHERE r.journey_id = $1 \
           AND r.revision_no >= (SELECT MAX(revision_no) - 1 FROM journey_revisions \
                                  WHERE journey_id = $1)",
    )
    .bind(journey_id)
    .fetch_all(pool)
    .await?;
    let newest = rows.iter().map(|(no, _)| *no).max().unwrap_or(0);
    let ids_of = |wanted: i32| -> HashSet<i64> {
        rows.iter()
            .filter(|(no, _)| *no == wanted)
            .filter_map(|(_, id)| *id)
            .collect()
    };
    let (latest_ids, previous_ids) = (ids_of(newest), ids_of(newest - 1));
    let mut replaced_leg_ids: Vec<i64> = previous_ids.difference(&latest_ids).copied().collect();
    replaced_leg_ids.sort_unstable();
    Ok(RevisionSummary {
        revision_count,
        replaced_leg_ids,
        new_leg_ids: latest_ids.difference(&previous_ids).copied().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_passenger_fault_or_chosen_change_moves_the_baseline_and_a_railway_fault_does_not() {
        let mut meta = RevisionMeta::user_choice();
        assert!(meta.moves_baseline());
        meta.fault = "user".to_string();
        assert!(meta.moves_baseline());
        meta.fault = "operator".to_string();
        assert!(!meta.moves_baseline());
        meta.fault = "none".to_string();
        meta.reason = Reason::Revert;
        assert!(!meta.moves_baseline());
    }
}

#[cfg(test)]
mod db_tests;
