//! Journey breaks: each time a journey stops working
//! (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md §4.1,
//! §5.3, §6). At most one is open per journey (`journey_breaks_one_open`).
//!
//! The api writes them: on read of a journey page and in the break sweep
//! (a cancelled or cut-short train, a missed connection, a skipped stop),
//! and on "I missed this train". The notifier only reads open ones and
//! stamps its push bookkeeping. A break ends `accepted` (the traveller took
//! an alternative), `dismissed`, `recovered` (the plan works again) or
//! `expired` (the journey is over).

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// `journey_breaks.cause`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    Cancelled,
    PartCancelled,
    ConnectionMissed,
    UserMissed,
    SkippedStop,
}

impl Cause {
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Cancelled => "cancelled",
            Cause::PartCancelled => "part_cancelled",
            Cause::ConnectionMissed => "connection_missed",
            Cause::UserMissed => "user_missed",
            Cause::SkippedStop => "skipped_stop",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "cancelled" => Cause::Cancelled,
            "part_cancelled" => Cause::PartCancelled,
            "connection_missed" => Cause::ConnectionMissed,
            "user_missed" => Cause::UserMissed,
            "skipped_stop" => Cause::SkippedStop,
            _ => return None,
        })
    }

    /// The fate of the trigger leg in the plan this break supersedes
    /// (`journey_revision_legs.fate`).
    pub fn trigger_fate(self) -> &'static str {
        match self {
            Cause::Cancelled | Cause::SkippedStop => "cancelled",
            Cause::PartCancelled => "part_cancelled",
            Cause::ConnectionMissed | Cause::UserMissed => "missed",
        }
    }

    /// The severity before suggestions are known (§6.2): a cancelled leg 3,
    /// a part-cancelled or skipped one 2, a missed connection 1.
    pub fn base_severity(self) -> i16 {
        match self {
            Cause::Cancelled => 3,
            Cause::PartCancelled | Cause::SkippedStop => 2,
            Cause::ConnectionMissed | Cause::UserMissed => 1,
        }
    }
}

/// A break's severity (§6.2) from its cause and the best suggestion: 4 with
/// no way to the destination; a missed connection whose best alternative
/// arrives more than 30 minutes after the plan is 2.
pub fn severity(cause: Cause, best_delay_minutes: Option<Option<i64>>) -> i16 {
    match best_delay_minutes {
        // Planned, and nothing found.
        Some(None) => 4,
        Some(Some(delay)) if delay > 30 => cause.base_severity().max(2),
        _ => cause.base_severity(),
    }
}

/// One `journey_breaks` row.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
pub struct BreakRow {
    pub id: i64,
    pub journey_id: i64,
    pub user_id: String,
    pub detected_at: DateTime<Utc>,
    pub cause: String,
    pub fault: String,
    pub severity: i16,
    pub trigger_leg_id: Option<i64>,
    pub trigger_trains_id: Option<i64>,
    pub break_position: i32,
    pub fingerprint: String,
    pub status: String,
    pub suggestions: Option<serde_json::Value>,
    pub suggestions_at: Option<DateTime<Utc>>,
    pub confirmations: i16,
    pub notified_at: Option<DateTime<Utc>>,
    pub notified_severity: Option<i16>,
    pub opened_at: Option<DateTime<Utc>>,
    pub opened_via: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub accepted_revision_id: Option<i64>,
}

const BREAK_COLUMNS: &str = "id, journey_id, user_id, detected_at, cause, fault, severity, \
     trigger_leg_id, trigger_trains_id, break_position, fingerprint, status, suggestions, \
     suggestions_at, confirmations, notified_at, notified_severity, opened_at, opened_via, \
     resolved_at, accepted_revision_id";

/// A detected break, before it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    pub cause: Cause,
    /// `operator`, `user` or `unknown`.
    pub fault: &'static str,
    pub severity: i16,
    pub trigger_leg_id: Option<i64>,
    pub trigger_trains_id: Option<i64>,
    pub break_position: i32,
    pub fingerprint: String,
}

/// The open break of a journey, if any.
pub async fn open_break(pool: &PgPool, journey_id: i64) -> anyhow::Result<Option<BreakRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {BREAK_COLUMNS} FROM journey_breaks WHERE journey_id = $1 AND status = 'open'"
    ))
    .bind(journey_id)
    .fetch_optional(pool)
    .await?)
}

/// One break by id, scoped to its journey.
pub async fn get_break(
    pool: &PgPool,
    journey_id: i64,
    break_id: i64,
) -> anyhow::Result<Option<BreakRow>> {
    Ok(sqlx::query_as(&format!(
        "SELECT {BREAK_COLUMNS} FROM journey_breaks WHERE id = $1 AND journey_id = $2"
    ))
    .bind(break_id)
    .bind(journey_id)
    .fetch_optional(pool)
    .await?)
}

/// What [`upsert_open`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upserted {
    /// A new open break.
    New,
    /// The open break, with new inputs (its suggestions are now stale).
    Changed,
    /// The open break, seen again unchanged (`confirmations` counted).
    Same,
}

/// Opens a break for `journey_id`, or updates the open one: cause, fault,
/// trigger and fingerprint follow the latest detection; the severity only
/// ever rises; with `confirm` (the sweep, not a page read),
/// `confirmations` counts consecutive detections with the same fingerprint
/// (§11: two sweeps before a push, unless cancelled).
pub async fn upsert_open(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    detected: &Detected,
    confirm: bool,
) -> anyhow::Result<(BreakRow, Upserted)> {
    let mut tx = pool.begin().await?;
    let existing: Option<BreakRow> = sqlx::query_as(&format!(
        "SELECT {BREAK_COLUMNS} FROM journey_breaks WHERE journey_id = $1 AND status = 'open' \
         FOR UPDATE"
    ))
    .bind(journey_id)
    .fetch_optional(&mut *tx)
    .await?;
    let outcome = match &existing {
        None => Upserted::New,
        Some(row) if row.fingerprint == detected.fingerprint => Upserted::Same,
        Some(_) => Upserted::Changed,
    };
    let row: BreakRow = match existing {
        None => {
            sqlx::query_as(&format!(
                "INSERT INTO journey_breaks \
                     (journey_id, user_id, cause, fault, severity, trigger_leg_id, \
                      trigger_trains_id, break_position, fingerprint) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                 ON CONFLICT (journey_id) WHERE status = 'open' DO UPDATE SET \
                     confirmations = journey_breaks.confirmations \
                 RETURNING {BREAK_COLUMNS}"
            ))
            .bind(journey_id)
            .bind(user_id)
            .bind(detected.cause.as_str())
            .bind(detected.fault)
            .bind(detected.severity)
            .bind(detected.trigger_leg_id)
            .bind(detected.trigger_trains_id)
            .bind(detected.break_position)
            .bind(&detected.fingerprint)
            .fetch_one(&mut *tx)
            .await?
        }
        Some(row) => {
            sqlx::query_as(&format!(
                "UPDATE journey_breaks SET cause = $2, fault = $3, \
                     severity = GREATEST(severity, $4), trigger_leg_id = $5, \
                     trigger_trains_id = $6, break_position = $7, fingerprint = $8, \
                     confirmations = CASE WHEN fingerprint <> $8 THEN 1 \
                                          WHEN $9 THEN LEAST(confirmations + 1, 100) \
                                          ELSE confirmations END \
                 WHERE id = $1 RETURNING {BREAK_COLUMNS}"
            ))
            .bind(row.id)
            .bind(detected.cause.as_str())
            .bind(detected.fault)
            .bind(detected.severity)
            .bind(detected.trigger_leg_id)
            .bind(detected.trigger_trains_id)
            .bind(detected.break_position)
            .bind(&detected.fingerprint)
            .bind(confirm)
            .fetch_one(&mut *tx)
            .await?
        }
    };
    tx.commit().await?;
    if outcome == Upserted::New {
        metrics::counter!(
            common::metrics::metric_name("api_journey_breaks_detected_total"),
            "cause" => detected.cause.as_str(),
            "fault" => detected.fault
        )
        .increment(1);
    }
    Ok((row, outcome))
}

/// Stores freshly planned suggestions on an open break, and raises its
/// severity to `severity` if higher.
pub async fn store_suggestions(
    pool: &PgPool,
    break_id: i64,
    suggestions: &serde_json::Value,
    severity: i16,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE journey_breaks SET suggestions = $2, suggestions_at = now(), \
             severity = GREATEST(severity, $3) \
         WHERE id = $1 AND status = 'open'",
    )
    .bind(break_id)
    .bind(suggestions)
    .bind(severity)
    .execute(pool)
    .await?;
    Ok(())
}

/// Stamps `opened_at` (and how) the first time the owner opens a break.
/// `true` when this call stamped it.
pub async fn stamp_opened(pool: &PgPool, break_id: i64, via: &str) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE journey_breaks SET opened_at = now(), opened_via = $2 \
         WHERE id = $1 AND opened_at IS NULL",
    )
    .bind(break_id)
    .bind(via)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Ends an open break on the caller's transaction. `true` when it was
/// open.
pub async fn resolve(
    conn: &mut sqlx::PgConnection,
    break_id: i64,
    status: &str,
    accepted_revision_id: Option<i64>,
    dismiss_reason: Option<&str>,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE journey_breaks SET status = $2, resolved_at = now(), \
             accepted_revision_id = COALESCE($3, accepted_revision_id), \
             dismiss_reason = COALESCE($4, dismiss_reason) \
         WHERE id = $1 AND status = 'open'",
    )
    .bind(break_id)
    .bind(status)
    .bind(accepted_revision_id)
    .bind(dismiss_reason)
    .execute(&mut *conn)
    .await?;
    let resolved = result.rows_affected() == 1;
    if resolved {
        metrics::counter!(
            common::metrics::metric_name("api_journey_breaks_resolved_total"),
            "status" => status.to_string()
        )
        .increment(1);
    }
    Ok(resolved)
}

/// Reopens break `break_id` after an Undo (§8.3): the plan it was accepted
/// for is gone again. Only when no other break is open for the journey.
pub async fn reopen(conn: &mut sqlx::PgConnection, break_id: i64) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE journey_breaks b SET status = 'open', resolved_at = NULL, \
             accepted_revision_id = NULL \
         WHERE b.id = $1 AND b.status = 'accepted' \
           AND NOT EXISTS (SELECT 1 FROM journey_breaks o \
                            WHERE o.journey_id = b.journey_id AND o.status = 'open')",
    )
    .bind(break_id)
    .execute(&mut *conn)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Whether the owner turned suggestion pushes off (`notification_preferences`;
/// no row means on).
pub async fn journey_suggestions_enabled(pool: &PgPool, user_id: &str) -> anyhow::Result<bool> {
    let row: Option<(bool,)> = sqlx::query_as(
        "SELECT journey_suggestions FROM notification_preferences WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.is_none_or(|(on,)| on))
}

/// Sets the owner's suggestion push switch.
pub async fn set_journey_suggestions(
    pool: &PgPool,
    user_id: &str,
    enabled: bool,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO notification_preferences (user_id, journey_suggestions) VALUES ($1, $2) \
         ON CONFLICT (user_id) DO UPDATE SET journey_suggestions = $2, updated_at = now()",
    )
    .bind(user_id)
    .bind(enabled)
    .execute(pool)
    .await?;
    Ok(())
}

/// Mutes or unmutes suggestion pushes for one of `user_id`'s journeys.
/// `false` when it is not theirs.
pub async fn set_suggestions_muted(
    pool: &PgPool,
    journey_id: i64,
    user_id: &str,
    muted: bool,
) -> anyhow::Result<bool> {
    let result =
        sqlx::query("UPDATE journeys SET suggestions_muted = $3 WHERE id = $1 AND user_id = $2")
            .bind(journey_id)
            .bind(user_id)
            .bind(muted)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() == 1)
}

/// Whether a journey's suggestion pushes are muted.
pub async fn suggestions_muted(pool: &PgPool, journey_id: i64) -> anyhow::Result<bool> {
    let row: Option<(bool,)> =
        sqlx::query_as("SELECT suggestions_muted FROM journeys WHERE id = $1")
            .bind(journey_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some_and(|(muted,)| muted))
}

/// The legs of `leg_ids` that are already travelled (frozen, §11): an
/// untracked leg whose planned arrival has passed; a train leg whose train
/// reached its alighting station, by `travel_order`'s rule.
pub async fn travelled_legs(
    conn: &mut sqlx::PgConnection,
    leg_ids: &[i64],
) -> anyhow::Result<Vec<i64>> {
    if leg_ids.is_empty() {
        return Ok(Vec::new());
    }
    let arrived = crate::data::travel_order::train_arrived_sql(
        "tr.id",
        "tr.train_uid",
        "tr.service_date",
        "COALESCE(jl.destination_crs, ts.pin_destination_crs, tr.destination_crs)",
        "cs.delay_minutes",
    );
    let sql = format!(
        "SELECT jl.id FROM journey_legs jl \
         LEFT JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         LEFT JOIN trains tr ON tr.id = ts.trains_id \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         WHERE jl.id = ANY($1) AND CASE \
             WHEN jl.kind = 'untracked' THEN COALESCE(jl.planned_arrival <= now(), FALSE) \
             WHEN tr.id IS NULL THEN FALSE \
             ELSE {arrived} END"
    );
    Ok(sqlx::query_scalar(&sql)
        .bind(leg_ids)
        .fetch_all(&mut *conn)
        .await?)
}

/// Ends open breaks whose journey is over (§11): every leg dated before
/// yesterday, or the last leg's service date ended more than four hours
/// ago in London (04:00 the next morning). Returns how many.
pub async fn expire_finished(pool: &PgPool) -> anyhow::Result<u64> {
    let result = sqlx::query(
        "UPDATE journey_breaks b SET status = 'expired', resolved_at = now() \
         WHERE b.status = 'open' \
           AND NOT EXISTS ( \
               SELECT 1 FROM journey_legs jl WHERE jl.journey_id = b.journey_id \
                 AND (jl.service_date + 1)::timestamp + INTERVAL '4 hours' \
                     > (now() AT TIME ZONE 'Europe/London'))",
    )
    .execute(pool)
    .await?;
    if result.rows_affected() > 0 {
        metrics::counter!(
            common::metrics::metric_name("api_journey_breaks_resolved_total"),
            "status" => "expired"
        )
        .increment(result.rows_affected());
    }
    Ok(result.rows_affected())
}

/// What detection needs to know about one leg besides its timing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts about one leg, read field by field"
)]
pub struct LegFacts {
    pub leg_id: i64,
    pub is_train: bool,
    pub trains_id: Option<i64>,
    /// The whole train is cancelled (TRUST `cancelled`).
    pub whole_cancelled: bool,
    /// The train no longer calls at the leg's boarding or alighting stop
    /// (the departure board says so).
    pub skipped: bool,
    /// Already travelled: frozen, never a break (§11).
    pub travelled: bool,
}

/// A break found by [`detect`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
    pub cause: Cause,
    /// Index of the first leg that no longer works.
    pub index: usize,
    pub trigger_leg_id: i64,
    pub trigger_trains_id: Option<i64>,
}

/// The first leg, after the travelled ones, that no longer works (§4.2):
/// a cancelled train, a train cut short or starting late so it misses the
/// leg's stops, a skipped stop, or a connection onto it missed on live
/// times and minimum change times. Pure. "At risk" is not a break.
pub fn detect(
    legs: &[LegFacts],
    timings: &[crate::data::journey_feasibility::LegTiming],
    connections: &[Option<crate::data::journey_feasibility::LegConnection>],
) -> Option<Detection> {
    use crate::data::journey_feasibility::{ConnectionState, LegTiming};
    for (index, leg) in legs.iter().enumerate() {
        if leg.travelled || !leg.is_train {
            continue;
        }
        let served = match timings.get(index) {
            Some(LegTiming::Train(timing)) => timing.served,
            _ => true,
        };
        let cause = if leg.whole_cancelled {
            Some(Cause::Cancelled)
        } else if !served {
            Some(Cause::PartCancelled)
        } else if leg.skipped {
            Some(Cause::SkippedStop)
        } else if connections
            .get(index)
            .and_then(Option::as_ref)
            .is_some_and(|c| c.state == ConnectionState::Missed)
        {
            Some(Cause::ConnectionMissed)
        } else {
            None
        };
        if let Some(cause) = cause {
            return Some(Detection {
                cause,
                index,
                trigger_leg_id: leg.leg_id,
                trigger_trains_id: leg.trains_id,
            });
        }
    }
    None
}

/// Who missed a train the traveller marked missed (§4.2, §14 Q5: the data
/// decides): the railway when the change onto it no longer worked on live
/// times, else the traveller.
pub fn user_missed_fault(
    connection_onto: Option<&crate::data::journey_feasibility::LegConnection>,
) -> &'static str {
    use crate::data::journey_feasibility::ConnectionState;
    if connection_onto.is_some_and(|c| c.state == ConnectionState::Missed) {
        "operator"
    } else {
        "user"
    }
}

/// A break's fingerprint (§5.4): its position and cause, and the live
/// departure and arrival minute of every train leg from it on. Planned
/// suggestions are stale once it changes.
pub fn fingerprint(
    cause: Cause,
    index: usize,
    timings: &[crate::data::journey_feasibility::LegTiming],
) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut text = format!("{index}|{}", cause.as_str());
    for timing in timings.iter().skip(index) {
        if let crate::data::journey_feasibility::LegTiming::Train(t) = timing {
            let _ = write!(
                text,
                "|{}:{}:{}:{}",
                t.uid,
                t.live_dep.timestamp() / 60,
                t.live_arr.timestamp() / 60,
                t.served
            );
        }
    }
    let digest = Sha256::digest(text.as_bytes());
    format!("{digest:x}")[..16].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_follows_the_cause_and_the_best_alternative() {
        assert_eq!(severity(Cause::ConnectionMissed, None), 1);
        assert_eq!(severity(Cause::ConnectionMissed, Some(Some(12))), 1);
        assert_eq!(severity(Cause::ConnectionMissed, Some(Some(45))), 2);
        assert_eq!(severity(Cause::PartCancelled, Some(Some(5))), 2);
        assert_eq!(severity(Cause::Cancelled, Some(Some(5))), 3);
        assert_eq!(severity(Cause::Cancelled, Some(None)), 4);
    }

    fn timing(
        uid: &str,
        dep: i64,
        arr: i64,
        served: bool,
    ) -> crate::data::journey_feasibility::LegTiming {
        use chrono::TimeZone;
        let at = |m: i64| {
            Utc.with_ymd_and_hms(2026, 10, 22, 8, 0, 0).unwrap() + chrono::Duration::minutes(m)
        };
        crate::data::journey_feasibility::LegTiming::Train(
            crate::data::journey_feasibility::TrainLegTiming {
                uid: uid.to_string(),
                date: chrono::NaiveDate::from_ymd_opt(2026, 10, 22).unwrap(),
                board_crs: "KGX".to_string(),
                board_tiploc: "KNGX".to_string(),
                alight_crs: "YRK".to_string(),
                sched_dep: at(dep),
                live_dep: at(dep),
                sched_arr: at(arr),
                live_arr: at(arr),
                served,
            },
        )
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "the shape `connections` takes, so call sites read as data"
    )]
    fn connection(
        state: crate::data::journey_feasibility::ConnectionState,
    ) -> Option<crate::data::journey_feasibility::LegConnection> {
        Some(crate::data::journey_feasibility::LegConnection {
            gap_minutes: Some(-3),
            minimum_change_minutes: Some(5),
            state,
        })
    }

    fn facts(leg_id: i64) -> LegFacts {
        LegFacts {
            leg_id,
            is_train: true,
            trains_id: Some(leg_id * 10),
            ..LegFacts::default()
        }
    }

    #[test]
    fn detection_finds_the_first_leg_that_no_longer_works() {
        use crate::data::journey_feasibility::ConnectionState;
        let timings = vec![timing("A", 0, 60, true), timing("B", 62, 120, true)];
        // A missed connection onto leg 2.
        let found = detect(
            &[facts(1), facts(2)],
            &timings,
            &[None, connection(ConnectionState::Missed)],
        )
        .unwrap();
        assert_eq!(
            (
                found.cause,
                found.index,
                found.trigger_leg_id,
                found.trigger_trains_id
            ),
            (Cause::ConnectionMissed, 1, 2, Some(20))
        );
        // At risk is not a break.
        assert_eq!(
            detect(
                &[facts(1), facts(2)],
                &timings,
                &[None, connection(ConnectionState::AtRisk)]
            ),
            None
        );
        // A cancelled first leg comes first; a travelled one never counts.
        let mut cancelled = facts(1);
        cancelled.whole_cancelled = true;
        assert_eq!(
            detect(&[cancelled.clone(), facts(2)], &timings, &[None, None]).map(|d| d.cause),
            Some(Cause::Cancelled)
        );
        cancelled.travelled = true;
        assert_eq!(
            detect(&[cancelled, facts(2)], &timings, &[None, None]),
            None
        );
        // Cut short so it misses the leg's stops; a skipped stop.
        let cut = vec![timing("A", 0, 60, false)];
        assert_eq!(
            detect(&[facts(1)], &cut, &[None]).map(|d| d.cause),
            Some(Cause::PartCancelled)
        );
        let mut skipped = facts(1);
        skipped.skipped = true;
        assert_eq!(
            detect(&[skipped], &timings, &[None]).map(|d| d.cause),
            Some(Cause::SkippedStop)
        );
    }

    #[test]
    fn the_data_decides_who_missed_the_train() {
        use crate::data::journey_feasibility::ConnectionState;
        assert_eq!(
            user_missed_fault(connection(ConnectionState::Missed).as_ref()),
            "operator"
        );
        assert_eq!(
            user_missed_fault(connection(ConnectionState::Ok).as_ref()),
            "user"
        );
        assert_eq!(user_missed_fault(None), "user");
    }

    #[test]
    fn the_fingerprint_follows_live_times_from_the_break_on() {
        let before = vec![timing("A", 0, 60, true), timing("B", 62, 120, true)];
        let mut later_first = before.clone();
        later_first[0] = timing("A", 5, 65, true);
        let mut later_second = before.clone();
        later_second[1] = timing("B", 70, 128, true);
        let base = fingerprint(Cause::ConnectionMissed, 1, &before);
        assert_eq!(base.len(), 16);
        // Leg 1 is before the break: no change.
        assert_eq!(fingerprint(Cause::ConnectionMissed, 1, &later_first), base);
        assert_ne!(fingerprint(Cause::ConnectionMissed, 1, &later_second), base);
        assert_ne!(fingerprint(Cause::Cancelled, 1, &before), base);
    }

    #[test]
    fn causes_round_trip_and_name_the_trigger_fate() {
        for cause in [
            Cause::Cancelled,
            Cause::PartCancelled,
            Cause::ConnectionMissed,
            Cause::UserMissed,
            Cause::SkippedStop,
        ] {
            assert_eq!(Cause::parse(cause.as_str()), Some(cause));
        }
        assert_eq!(Cause::UserMissed.trigger_fate(), "missed");
        assert_eq!(Cause::Cancelled.trigger_fate(), "cancelled");
        assert_eq!(Cause::parse("nope"), None);
    }
}
