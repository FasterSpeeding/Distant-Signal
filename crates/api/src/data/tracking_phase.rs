//! Where a tracked train is in its life, from the traveller's point of view
//! (`trackingPhase` on the wire): planned, waiting for its first Network
//! Rail report, live, not in the timetable at all, or timetable-only. See
//! docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
//! §3 and §4.
//!
//! Derived on every read, never stored: from the subscription's own state
//! (`resolution_status`, `train_current_state.status`), whether we hold a
//! timetable for the train, the timetabled departure from the train's own
//! origin, and the clock. `resolutionStatus` keeps its meaning; this is the
//! field the frontend and ds-mcp branch on for "planned" and "not in the
//! timetable".
//!
//! **The threshold** (spec §4.1, §4.2). TRUST activates a train about an
//! hour before it leaves its origin: on 2026-10-19/20 in production, 22,785
//! Activations had a median lead of 59.8 minutes and a 1st percentile of
//! 44.6, none after the departure. So a train with a timetable and no
//! Activation is planned until [`ACTIVATION_DUE_BEFORE_ORIGIN`] before its
//! origin departure; once activated, it is planned until the origin
//! departure itself, when its first movement is due.
//!
//! Applied after the existing reads (`attach`), by one batched lookup; a
//! failed lookup leaves every row's phase `None` (the wire's `null`), which
//! the frontend reads as today's behaviour.

use std::collections::HashMap;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;
use sqlx::PgPool;

/// How long before a train's timetabled departure from its own origin we
/// expect its TRUST Activation: 99% of them have arrived by then (module
/// doc).
pub const ACTIVATION_DUE_BEFORE_ORIGIN: Duration = Duration::minutes(45);

/// The traveller-facing phase of a tracked train (spec §4.3), evaluated in
/// declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TrackingPhase {
    /// A bus or ferry: TRUST never reports it.
    TimetableOnly,
    /// Network Rail has reported a movement, a cancellation or an arrival.
    Live,
    /// No timetable for it and no Network Rail report: we can't track it.
    NoTimetable,
    /// Timetabled, and its first report is not due yet.
    Planned,
    /// Its first report is due (or we can't tell when it is): the existing
    /// waiting states.
    AwaitingReport,
}

/// What [`tracking_phase`] reads off one row.
#[derive(Debug, Clone, Copy)]
pub struct PhaseInput<'a> {
    /// `false` for a bus or ferry.
    pub live_tracking: bool,
    /// TRUST has activated the train (`resolution_status = 'resolved'`, or
    /// a TRUST train id on the shared row).
    pub activated: bool,
    /// `train_current_state.status`.
    pub status: Option<&'a str>,
    /// We hold a timetable for the train: a timetable row for its UID and
    /// date, or calling points on its `trains` row.
    pub has_timetable: bool,
    /// The timetabled (working) departure from the train's own origin.
    pub origin_departure: Option<DateTime<Utc>>,
}

/// The phase, and when its first report is (or was) expected: `Some` only
/// when an origin departure is known and the train is not yet live.
pub fn tracking_phase(
    input: PhaseInput<'_>,
    now: DateTime<Utc>,
) -> (TrackingPhase, Option<DateTime<Utc>>) {
    if !input.live_tracking {
        return (TrackingPhase::TimetableOnly, None);
    }
    if matches!(input.status, Some("en_route" | "cancelled" | "completed")) {
        return (TrackingPhase::Live, None);
    }
    if !input.has_timetable && !input.activated {
        return (TrackingPhase::NoTimetable, None);
    }
    let Some(origin_departure) = input.origin_departure else {
        return (TrackingPhase::AwaitingReport, None);
    };
    let expected = if input.activated {
        origin_departure
    } else {
        origin_departure - ACTIVATION_DUE_BEFORE_ORIGIN
    };
    let phase = if now < expected {
        TrackingPhase::Planned
    } else {
        TrackingPhase::AwaitingReport
    };
    (phase, Some(expected))
}

/// What one row knows about its train before the batched lookup.
#[derive(Debug, Clone, Copy)]
pub struct PhaseSignals<'a> {
    /// The row is about a train at all (a journey list row whose current
    /// leg is open or untracked is not).
    pub applies: bool,
    pub activated: bool,
    pub status: Option<&'a str>,
    /// Timetable evidence already on the row (calling points, a schedule
    /// origin), independent of the lookup.
    pub timetable_on_row: bool,
}

/// A read model that carries `trackingPhase`/`firstReportExpectedAt`.
pub trait PhaseRow {
    /// The train's UID and service date, when known.
    fn phase_key(&self) -> Option<(&str, NaiveDate)>;
    fn phase_signals(&self) -> PhaseSignals<'_>;
    fn set_phase(&mut self, phase: TrackingPhase, expected: Option<DateTime<Utc>>);
}

/// One train's timetable facts: its origin departure, if any timetable row
/// exists, and its `schedule_services` mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimetableFacts {
    pub origin_departure: Option<DateTime<Utc>>,
    pub live_tracking: bool,
}

/// The timetable facts for many `(uid, service_date)` pairs in one query.
/// A pair with no timetable row and no `schedule_services` row is absent.
///
/// The origin departure is the departure row whose `origin_crs` is the
/// schedule's own `true_origin_crs` (the train's first timetabled
/// departure), else the earliest departure row (a train starting at a
/// depot with no station code), as London local time on the service date:
/// `day_offset` 0 by construction for the origin. Served by
/// `schedule_destination_departures_train_uid_service_date_scheduled_idx`.
pub async fn timetable_facts(
    pool: &PgPool,
    keys: &[(String, NaiveDate)],
) -> anyhow::Result<HashMap<(String, NaiveDate), TimetableFacts>> {
    let mut uids: Vec<String> = Vec::with_capacity(keys.len());
    let mut dates: Vec<NaiveDate> = Vec::with_capacity(keys.len());
    for (uid, date) in keys {
        if uids.iter().zip(&dates).any(|(u, d)| u == uid && d == date) {
            continue;
        }
        uids.push(uid.clone());
        dates.push(*date);
    }
    if uids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, NaiveDate, Option<DateTime<Utc>>, Option<String>)> = sqlx::query_as(
        "SELECT k.uid, k.service_date, \
                (SELECT (d.service_date + d.scheduled) AT TIME ZONE 'Europe/London' \
                 FROM schedule_destination_departures d \
                 WHERE d.train_uid = k.uid AND d.service_date = k.service_date \
                 ORDER BY (d.origin_crs = d.true_origin_crs) DESC NULLS LAST, \
                          d.day_offset, d.scheduled \
                 LIMIT 1) AS origin_departure, \
                (SELECT ss.mode FROM schedule_services ss \
                 WHERE ss.service_date = k.service_date AND ss.uid = k.uid \
                 LIMIT 1) AS mode \
         FROM UNNEST($1::text[], $2::date[]) AS k(uid, service_date)",
    )
    .bind(&uids)
    .bind(&dates)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .filter(|(_, _, departure, mode)| departure.is_some() || mode.is_some())
        .map(|(uid, date, origin_departure, mode)| {
            let live_tracking = mode.as_deref().is_none_or(|mode| {
                crate::data::schedule_services::ServiceMode::from_db_str(mode).live_tracking()
            });
            (
                (uid, date),
                TimetableFacts {
                    origin_departure,
                    live_tracking,
                },
            )
        })
        .collect())
}

/// Sets every applicable row's phase from one [`timetable_facts`] lookup.
/// Best effort: a failed lookup is logged and leaves the rows' phase unset
/// (`null` on the wire, today's display).
pub async fn attach<T: PhaseRow>(pool: &PgPool, rows: &mut [T], now: DateTime<Utc>) {
    let keys: Vec<(String, NaiveDate)> = rows
        .iter()
        .filter(|row| row.phase_signals().applies)
        .filter_map(|row| row.phase_key().map(|(uid, date)| (uid.to_string(), date)))
        .collect();
    let facts = match timetable_facts(pool, &keys).await {
        Ok(facts) => facts,
        Err(err) => {
            tracing::warn!(error = ?err, "could not read timetable facts for tracking phases");
            return;
        }
    };
    apply(rows, &facts, now);
}

/// The pure half of [`attach`]: one phase per applicable row from already
/// read facts.
pub fn apply<T: PhaseRow>(
    rows: &mut [T],
    facts: &HashMap<(String, NaiveDate), TimetableFacts>,
    now: DateTime<Utc>,
) {
    for row in rows.iter_mut() {
        let signals = row.phase_signals();
        if !signals.applies {
            continue;
        }
        let found = row
            .phase_key()
            .and_then(|(uid, date)| facts.get(&(uid.to_string(), date)))
            .copied();
        let (phase, expected) = tracking_phase(
            PhaseInput {
                live_tracking: found.is_none_or(|f| f.live_tracking),
                activated: signals.activated,
                status: signals.status,
                has_timetable: signals.timetable_on_row
                    || found.is_some_and(|f| f.origin_departure.is_some()),
                origin_departure: found.and_then(|f| f.origin_departure),
            },
            now,
        );
        row.set_phase(phase, expected);
    }
}

#[cfg(test)]
#[expect(clippy::unwrap_used, reason = "test code")]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn input(departure: Option<&str>) -> PhaseInput<'static> {
        PhaseInput {
            live_tracking: true,
            activated: false,
            status: None,
            has_timetable: true,
            origin_departure: departure.map(at),
        }
    }

    #[test]
    fn a_timetabled_train_is_planned_until_its_activation_is_due() {
        let base = input(Some("2026-10-21T08:00:00Z"));
        assert_eq!(
            tracking_phase(base, at("2026-10-20T12:00:00Z")),
            (TrackingPhase::Planned, Some(at("2026-10-21T07:15:00Z")))
        );
        assert_eq!(
            tracking_phase(base, at("2026-10-21T07:14:59Z")).0,
            TrackingPhase::Planned
        );
        assert_eq!(
            tracking_phase(base, at("2026-10-21T07:15:00Z")),
            (
                TrackingPhase::AwaitingReport,
                Some(at("2026-10-21T07:15:00Z"))
            ),
            "at the threshold the existing waiting copy takes over"
        );
    }

    #[test]
    fn an_activated_train_is_planned_until_its_origin_departure() {
        let activated = PhaseInput {
            activated: true,
            status: Some("awaiting_activation"),
            ..input(Some("2026-10-21T08:00:00Z"))
        };
        assert_eq!(
            tracking_phase(activated, at("2026-10-21T07:30:00Z")),
            (TrackingPhase::Planned, Some(at("2026-10-21T08:00:00Z")))
        );
        assert_eq!(
            tracking_phase(activated, at("2026-10-21T08:00:00Z")).0,
            TrackingPhase::AwaitingReport
        );
    }

    #[test]
    fn any_live_report_wins_over_the_clock() {
        for status in ["en_route", "cancelled", "completed"] {
            let live = PhaseInput {
                status: Some(status),
                activated: true,
                ..input(Some("2026-10-21T08:00:00Z"))
            };
            assert_eq!(
                tracking_phase(live, at("2026-10-20T08:00:00Z")),
                (TrackingPhase::Live, None),
                "{status}"
            );
        }
        let cancelled_before_activation = PhaseInput {
            status: Some("cancelled"),
            has_timetable: false,
            ..input(None)
        };
        assert_eq!(
            tracking_phase(cancelled_before_activation, at("2026-10-20T08:00:00Z")).0,
            TrackingPhase::Live
        );
    }

    #[test]
    fn no_timetable_and_no_report_is_no_timetable() {
        let unknown = PhaseInput {
            has_timetable: false,
            ..input(None)
        };
        assert_eq!(
            tracking_phase(unknown, at("2026-10-20T08:00:00Z")),
            (TrackingPhase::NoTimetable, None)
        );
        let activated_without_timetable = PhaseInput {
            activated: true,
            ..unknown
        };
        assert_eq!(
            tracking_phase(activated_without_timetable, at("2026-10-20T08:00:00Z")).0,
            TrackingPhase::AwaitingReport,
            "Network Rail has reported it, so it is not 'can't track'"
        );
    }

    #[test]
    fn a_timetable_with_no_known_origin_time_keeps_todays_waiting_state() {
        assert_eq!(
            tracking_phase(input(None), at("2026-10-20T08:00:00Z")),
            (TrackingPhase::AwaitingReport, None)
        );
    }

    #[test]
    fn a_bus_or_ferry_is_timetable_only_whatever_else_holds() {
        let bus = PhaseInput {
            live_tracking: false,
            has_timetable: false,
            ..input(None)
        };
        assert_eq!(
            tracking_phase(bus, at("2026-10-20T08:00:00Z")),
            (TrackingPhase::TimetableOnly, None)
        );
    }

    #[test]
    fn a_passenger_after_midnight_uses_the_trains_own_origin() {
        // Leaves its origin at 23:50 on the 20th (London, BST = UTC+1); the
        // passenger boards at 00:20 on the 21st. The Activation is due at
        // 23:05 on the 20th, so at 23:30 the train is no longer planned.
        let late = input(Some("2026-10-20T22:50:00Z"));
        assert_eq!(
            tracking_phase(late, at("2026-10-20T22:04:00Z")).0,
            TrackingPhase::Planned
        );
        assert_eq!(
            tracking_phase(late, at("2026-10-20T22:30:00Z")).0,
            TrackingPhase::AwaitingReport
        );
    }

    struct Row {
        uid: Option<&'static str>,
        applies: bool,
        activated: bool,
        timetable_on_row: bool,
        phase: Option<TrackingPhase>,
        expected: Option<DateTime<Utc>>,
    }

    impl PhaseRow for Row {
        fn phase_key(&self) -> Option<(&str, NaiveDate)> {
            self.uid
                .map(|uid| (uid, NaiveDate::from_ymd_opt(2026, 10, 21).unwrap()))
        }
        fn phase_signals(&self) -> PhaseSignals<'_> {
            PhaseSignals {
                applies: self.applies,
                activated: self.activated,
                status: None,
                timetable_on_row: self.timetable_on_row,
            }
        }
        fn set_phase(&mut self, phase: TrackingPhase, expected: Option<DateTime<Utc>>) {
            self.phase = Some(phase);
            self.expected = expected;
        }
    }

    fn row(uid: Option<&'static str>) -> Row {
        Row {
            uid,
            applies: true,
            activated: false,
            timetable_on_row: false,
            phase: None,
            expected: None,
        }
    }

    #[test]
    fn apply_reads_each_rows_facts() {
        let date = NaiveDate::from_ymd_opt(2026, 10, 21).unwrap();
        let facts = HashMap::from([
            (
                ("P00001".to_string(), date),
                TimetableFacts {
                    origin_departure: Some(at("2026-10-21T08:00:00Z")),
                    live_tracking: true,
                },
            ),
            (
                ("B00001".to_string(), date),
                TimetableFacts {
                    origin_departure: Some(at("2026-10-21T08:00:00Z")),
                    live_tracking: false,
                },
            ),
        ]);
        let mut rows = vec![
            row(Some("P00001")),
            row(Some("B00001")),
            row(Some("X99999")),
            row(None),
            Row {
                timetable_on_row: true,
                ..row(None)
            },
            Row {
                applies: false,
                ..row(Some("P00001"))
            },
        ];
        apply(&mut rows, &facts, at("2026-10-20T12:00:00Z"));
        let phases: Vec<_> = rows.iter().map(|r| r.phase).collect();
        assert_eq!(
            phases,
            vec![
                Some(TrackingPhase::Planned),
                Some(TrackingPhase::TimetableOnly),
                Some(TrackingPhase::NoTimetable),
                Some(TrackingPhase::NoTimetable),
                Some(TrackingPhase::AwaitingReport),
                None,
            ]
        );
        assert_eq!(rows[0].expected, Some(at("2026-10-21T07:15:00Z")));
    }

    #[tokio::test]
    #[ignore = "requires a live database: DATABASE_URL=... cargo test -p api \
                timetable_facts_reads_the_true_origin_and_the_mode -- --ignored"]
    async fn timetable_facts_reads_the_true_origin_and_the_mode() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL"))
            .await
            .expect("connect to postgres");
        // A far-future date no other test publishes for.
        let date = NaiveDate::from_ymd_opt(2031, 7, 14).unwrap();
        let cleanup = || async {
            sqlx::query(
                "DELETE FROM schedule_destination_departures \
                 WHERE service_date = $1 AND train_uid LIKE 'TPH%'",
            )
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();
            sqlx::query(
                "DELETE FROM schedule_services WHERE service_date = $1 AND uid LIKE 'TPH%'",
            )
            .bind(date)
            .execute(&pool)
            .await
            .unwrap();
        };
        cleanup().await;
        // TPH001: departs its true origin (WOK) at 07:10 and a later stop
        // (CLJ) at 07:40, BST, so 06:10Z.
        // TPH002: starts at a depot with no station code (true origin NULL);
        // its earliest departure row is the fallback. A bus.
        for (uid, origin, true_origin, scheduled) in [
            ("TPH001", "CLJ", Some("WOK"), "07:40"),
            ("TPH001", "WOK", Some("WOK"), "07:10"),
            ("TPH002", "RDG", None, "09:00"),
            ("TPH002", "TWY", None, "09:10"),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs) \
                 VALUES ($1, 'WAT', $2::time, $3, $4, $5)",
            )
            .bind(date)
            .bind(scheduled)
            .bind(uid)
            .bind(origin)
            .bind(true_origin)
            .execute(&pool)
            .await
            .unwrap();
        }
        sqlx::query(
            "INSERT INTO schedule_services (service_date, uid, mode, stp) VALUES ($1, 'TPH002', 'bus', 'P')",
        )
        .bind(date)
        .execute(&pool)
        .await
        .unwrap();

        let facts = timetable_facts(
            &pool,
            &[
                ("TPH001".to_string(), date),
                ("TPH001".to_string(), date),
                ("TPH002".to_string(), date),
                ("TPH404".to_string(), date),
            ],
        )
        .await
        .unwrap();
        cleanup().await;

        assert_eq!(facts.len(), 2, "the unknown UID is absent: {facts:?}");
        assert_eq!(
            facts[&("TPH001".to_string(), date)],
            TimetableFacts {
                origin_departure: Some(at("2031-07-14T06:10:00Z")),
                live_tracking: true,
            }
        );
        assert_eq!(
            facts[&("TPH002".to_string(), date)],
            TimetableFacts {
                origin_departure: Some(at("2031-07-14T08:00:00Z")),
                live_tracking: false,
            }
        );
    }

    #[test]
    fn the_wire_names_are_camel_case() {
        assert_eq!(
            serde_json::to_value(TrackingPhase::AwaitingReport).unwrap(),
            "awaitingReport"
        );
        assert_eq!(
            serde_json::to_value(TrackingPhase::NoTimetable).unwrap(),
            "noTimetable"
        );
        assert_eq!(
            serde_json::to_value(TrackingPhase::TimetableOnly).unwrap(),
            "timetableOnly"
        );
    }
}
