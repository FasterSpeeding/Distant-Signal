//! Whether a train got a ticket holder to their destination, for Delay
//! Repay (`delay_repay_rules::Outcome`): one batched read of the facts
//! `delay_repay_rules::classify_outcome` decides on. Read-only.
//!
//! The facts, all for one train (`trains_id`) and the ticket's destination
//! CRS:
//! - TRUST's ARRIVAL, DEPARTURE and PASS movements there
//!   (`train_movement_events`, with an actual time);
//! - an ARRIVAL or DEPARTURE at a call AFTER the destination in the CIF
//!   schedule (`schedule_calling_points_full`, its TIPLOCs mapped to CRS the
//!   way `common::public_delay::db` maps them);
//! - Darwin's cancelled calls captured for the train (`trains.skipped_stations`);
//! - `train_current_state.status = 'cancelled'`, with the TRUST `0002`
//!   cancellation's type and location (`train_reasons`) placed before, at
//!   or after the destination on the schedule.
//!
//! See docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md,
//! "Decisions (2026-10-07)".

use chrono::NaiveDate;
use sqlx::PgPool;

use crate::data::delay_repay_rules::{CancelPosition, Outcome, OutcomeFacts, classify_outcome};

/// One train and the ticket destination on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutcomeTarget {
    pub trains_id: i64,
    pub train_uid: String,
    pub service_date: NaiveDate,
    /// Upper-case CRS.
    pub destination_crs: String,
}

#[derive(sqlx::FromRow)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one row of independent yes/no facts, as the query selects them"
)]
struct FactsRow {
    ord: i64,
    arrived: bool,
    departed: bool,
    passed: bool,
    reported_beyond: bool,
    darwin_skipped: bool,
    status: Option<String>,
    canx_type: Option<String>,
    canx_seq: Option<i32>,
    first_seq: Option<i32>,
}

impl FactsRow {
    fn facts(&self) -> OutcomeFacts {
        let cancelled = (self.status.as_deref() == Some("cancelled"))
            .then(|| cancel_position(self.canx_type.as_deref(), self.canx_seq, self.first_seq));
        OutcomeFacts {
            arrived: self.arrived,
            departed: self.departed,
            passed: self.passed,
            darwin_skipped: self.darwin_skipped,
            reported_beyond: self.reported_beyond,
            cancelled,
        }
    }
}

/// Where a `0002` applies: an `EN ROUTE` cancellation from the call at
/// `canx_seq`, against the destination's first call at `first_seq`; any
/// other type is the whole train.
fn cancel_position(
    canx_type: Option<&str>,
    canx_seq: Option<i32>,
    first_seq: Option<i32>,
) -> CancelPosition {
    match canx_type.map(str::trim) {
        Some("EN ROUTE") => match (canx_seq, first_seq) {
            (Some(canx), Some(first)) if canx >= first => CancelPosition::AtOrAfterDestination,
            (Some(_), Some(_)) => CancelPosition::BeforeDestination,
            _ => CancelPosition::Unknown,
        },
        Some("AT ORIGIN" | "ON CALL" | "OUT OF PLAN") => CancelPosition::WholeTrain,
        _ => CancelPosition::Unknown,
    }
}

/// Each target's outcome, in order (`None`: not known yet). One query for
/// the whole slice; none for an empty one.
pub async fn outcomes(
    pool: &PgPool,
    targets: &[OutcomeTarget],
) -> anyhow::Result<Vec<Option<Outcome>>> {
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i64> = targets.iter().map(|t| t.trains_id).collect();
    let uids: Vec<String> = targets.iter().map(|t| t.train_uid.clone()).collect();
    let dates: Vec<NaiveDate> = targets.iter().map(|t| t.service_date).collect();
    let crs: Vec<String> = targets
        .iter()
        .map(|t| t.destination_crs.trim().to_uppercase())
        .collect();
    let rows: Vec<FactsRow> = sqlx::query_as(
        "WITH k AS ( \
             SELECT * FROM UNNEST($1::bigint[], $2::text[], $3::date[], $4::text[]) \
                 WITH ORDINALITY AS k(trains_id, uid, service_date, crs, ord) \
         ), calls AS ( \
             SELECT k.ord, c.seq::int AS seq, x.crs \
             FROM k \
             JOIN schedule_calling_points_full c \
               ON c.service_date = k.service_date AND c.uid = k.uid \
             JOIN LATERAL ( \
                 SELECT UPPER(m.crs) AS crs FROM ( \
                     SELECT crs, 1 AS priority FROM tiploc_crs \
                      WHERE tiploc = UPPER(TRIM(c.tiploc)) \
                     UNION ALL \
                     SELECT crs, 2 AS priority FROM stanox_crs \
                      WHERE tiploc = UPPER(TRIM(c.tiploc)) \
                     UNION ALL \
                     SELECT crs, 3 AS priority FROM corpus_tiploc_crs \
                      WHERE $5 AND tiploc = UPPER(TRIM(c.tiploc)) \
                 ) m ORDER BY m.priority LIMIT 1 \
             ) x ON TRUE \
         ), dest AS ( \
             SELECT calls.ord, MIN(calls.seq) AS first_seq, MAX(calls.seq) AS last_seq \
             FROM calls JOIN k ON k.ord = calls.ord \
             WHERE calls.crs = k.crs \
             GROUP BY calls.ord \
         ) \
         SELECT k.ord, \
             EXISTS (SELECT 1 FROM train_movement_events e \
                      WHERE e.trains_id = k.trains_id AND UPPER(e.loc_crs) = k.crs \
                        AND e.event_type = 'ARRIVAL' AND e.actual_timestamp IS NOT NULL) AS arrived, \
             EXISTS (SELECT 1 FROM train_movement_events e \
                      WHERE e.trains_id = k.trains_id AND UPPER(e.loc_crs) = k.crs \
                        AND e.event_type = 'DEPARTURE' AND e.actual_timestamp IS NOT NULL) AS departed, \
             EXISTS (SELECT 1 FROM train_movement_events e \
                      WHERE e.trains_id = k.trains_id AND UPPER(e.loc_crs) = k.crs \
                        AND e.event_type = 'PASS' AND e.actual_timestamp IS NOT NULL) AS passed, \
             EXISTS (SELECT 1 FROM train_movement_events e \
                      JOIN calls c ON c.ord = k.ord AND c.crs = UPPER(e.loc_crs) \
                      WHERE e.trains_id = k.trains_id \
                        AND e.event_type IN ('ARRIVAL', 'DEPARTURE') \
                        AND e.actual_timestamp IS NOT NULL \
                        AND c.seq > d.last_seq) AS reported_beyond, \
             COALESCE((SELECT k.crs = ANY (SELECT UPPER(TRIM(s)) FROM UNNEST(t.skipped_stations) s) \
                         FROM trains t WHERE t.id = k.trains_id), FALSE) AS darwin_skipped, \
             (SELECT cs.status FROM train_current_state cs \
               WHERE cs.trains_id = k.trains_id LIMIT 1) AS status, \
             r.canx_type, \
             (SELECT MIN(c.seq) FROM calls c \
               WHERE c.ord = k.ord \
                 AND c.crs = (SELECT UPPER(sc.crs) FROM stanox_crs sc \
                               WHERE sc.stanox = r.loc_stanox LIMIT 1)) AS canx_seq, \
             d.first_seq \
         FROM k \
         LEFT JOIN dest d ON d.ord = k.ord \
         LEFT JOIN train_reasons r ON r.trains_id = k.trains_id AND r.msg_type = '0002' \
         ORDER BY k.ord",
    )
    .bind(&ids)
    .bind(&uids)
    .bind(&dates)
    .bind(&crs)
    .bind(crate::data::corpus_crosswalk::fallback_enabled())
    .fetch_all(pool)
    .await?;
    let mut out = vec![None; targets.len()];
    for row in rows {
        let index = usize::try_from(row.ord - 1)?;
        if let Some(slot) = out.get_mut(index) {
            *slot = classify_outcome(row.facts());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_types_place_the_cancellation() {
        use CancelPosition::{AtOrAfterDestination, BeforeDestination, Unknown, WholeTrain};
        assert_eq!(cancel_position(Some("ON CALL"), None, Some(5)), WholeTrain);
        assert_eq!(
            cancel_position(Some("AT ORIGIN"), Some(0), Some(5)),
            WholeTrain
        );
        assert_eq!(cancel_position(Some("OUT OF PLAN"), None, None), WholeTrain);
        assert_eq!(
            cancel_position(Some("EN ROUTE"), Some(3), Some(5)),
            BeforeDestination
        );
        assert_eq!(
            cancel_position(Some("EN ROUTE"), Some(5), Some(5)),
            AtOrAfterDestination
        );
        assert_eq!(
            cancel_position(Some("EN ROUTE"), Some(7), Some(5)),
            AtOrAfterDestination
        );
        assert_eq!(cancel_position(Some("EN ROUTE"), None, Some(5)), Unknown);
        assert_eq!(cancel_position(None, None, None), Unknown);
    }

    // name, (event type, CRS index) movements, status, cancellation
    // (type, stanox index), Darwin-skipped B, expected.
    type Case<'a> = (
        &'a str,
        &'a [(&'a str, usize)],
        &'a str,
        Option<(&'a str, Option<usize>)>,
        bool,
        Option<Outcome>,
    );

    async fn connect() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&url)
            .await
            .expect("connect to postgres")
    }

    /// End to end on a three-call schedule A -> B -> C, the ticket to B:
    /// arrived, departure-only, cancelled before B, cancelled after B,
    /// passed B, Darwin-skipped B then reported at C, and en route.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                outcomes_detect_arrival_departure_and_not_reached -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        reason = "one end-to-end fixture: its seed rows are most of the length"
    )]
    async fn outcomes_detect_arrival_departure_and_not_reached() {
        use crate::data::queries;
        let pool = connect().await;
        let date: NaiveDate = "2026-10-07".parse().unwrap();
        let tag = "TDRO";
        let stations = ["A", "B", "C"].map(|s| format!("{tag}{s}"));
        let crs = ["ZZA", "ZZB", "ZZC"];
        let records: Vec<common::StanoxCrsRecord> = crs
            .iter()
            .enumerate()
            .map(|(i, c)| common::StanoxCrsRecord {
                stanox: format!("{tag}-{i}"),
                crs: (*c).to_string(),
                tiploc: stations[i].clone(),
                station_name: (*c).to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            })
            .collect();
        queries::upsert_stanox_crs(&pool, &records)
            .await
            .expect("seed stanox_crs");

        let cases: &[Case<'_>] = &[
            (
                "ARR",
                &[("DEPARTURE", 0), ("ARRIVAL", 1)],
                "en_route",
                None,
                false,
                Some(Outcome::Arrived),
            ),
            (
                "DEP",
                &[("DEPARTURE", 0), ("DEPARTURE", 1)],
                "en_route",
                None,
                false,
                Some(Outcome::DepartedOnly),
            ),
            (
                "CXB",
                &[("DEPARTURE", 0)],
                "cancelled",
                Some(("EN ROUTE", Some(0))),
                false,
                Some(Outcome::NotReached),
            ),
            (
                "CXA",
                &[("DEPARTURE", 0)],
                "cancelled",
                Some(("EN ROUTE", Some(2))),
                false,
                None,
            ),
            (
                "CXW",
                &[],
                "cancelled",
                Some(("ON CALL", None)),
                false,
                Some(Outcome::NotReached),
            ),
            (
                "PAS",
                &[("DEPARTURE", 0), ("PASS", 1)],
                "en_route",
                None,
                false,
                Some(Outcome::NotReached),
            ),
            (
                "SKP",
                &[("DEPARTURE", 0), ("ARRIVAL", 2)],
                "completed",
                None,
                true,
                Some(Outcome::NotReached),
            ),
            ("SKN", &[("DEPARTURE", 0)], "en_route", None, true, None),
            ("RUN", &[("DEPARTURE", 0)], "en_route", None, false, None),
        ];

        let mut targets = Vec::new();
        for (name, movements, status, cancel, skipped, _) in cases {
            let uid = format!("{tag}{name}");
            let trains_id = crate::data::trains::find_or_create_train(&pool, &uid, date)
                .await
                .expect("find_or_create_train");
            for sql in [
                "DELETE FROM train_movement_events WHERE trains_id = $1",
                "DELETE FROM train_reasons WHERE trains_id = $1",
                "DELETE FROM train_current_state WHERE trains_id = $1",
            ] {
                sqlx::query(sql)
                    .bind(trains_id)
                    .execute(&pool)
                    .await
                    .expect("clear");
            }
            let row = |seq: i16, kind: &str| queries::ScheduleCallingPointsFullRow {
                service_date: date,
                uid: uid.clone(),
                seq,
                tiploc: stations[usize::try_from(seq).unwrap()].clone(),
                kind: kind.to_string(),
                day_offset: 0,
                ..Default::default()
            };
            queries::upsert_schedule_calling_points_full(
                &pool,
                &[
                    row(0, "origin"),
                    row(1, "intermediate"),
                    row(2, "terminate"),
                ],
            )
            .await
            .expect("seed schedule");
            for (n, (event_type, at)) in movements.iter().enumerate() {
                sqlx::query(
                    "INSERT INTO train_movement_events \
                         (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                          actual_timestamp, raw_body) \
                     VALUES ($1, $2, '0003', $3, $4, NOW(), NOW(), '{}'::jsonb)",
                )
                .bind(trains_id)
                .bind(format!("{uid}-{n}"))
                .bind(event_type)
                .bind(crs[*at])
                .execute(&pool)
                .await
                .expect("seed movement");
            }
            sqlx::query("INSERT INTO train_current_state (trains_id, status) VALUES ($1, $2)")
                .bind(trains_id)
                .bind(status)
                .execute(&pool)
                .await
                .expect("seed current state");
            if let Some((canx_type, at)) = cancel {
                sqlx::query(
                    "INSERT INTO train_reasons (trains_id, msg_type, reason_code, canx_type, loc_stanox) \
                     VALUES ($1, '0002', 'TG', $2, $3)",
                )
                .bind(trains_id)
                .bind(canx_type)
                .bind(at.map(|i| format!("{tag}-{i}")))
                .execute(&pool)
                .await
                .expect("seed cancellation");
            }
            let skipped: Vec<String> = if *skipped {
                vec!["ZZB".to_string()]
            } else {
                Vec::new()
            };
            sqlx::query("UPDATE trains SET skipped_stations = $2 WHERE id = $1")
                .bind(trains_id)
                .bind(&skipped)
                .execute(&pool)
                .await
                .expect("seed skipped stations");
            targets.push(OutcomeTarget {
                trains_id,
                train_uid: uid,
                service_date: date,
                destination_crs: "zzb".to_string(),
            });
        }

        let got = outcomes(&pool, &targets).await.expect("outcomes");
        for ((name, .., expected), got) in cases.iter().zip(&got) {
            assert_eq!(got, expected, "{name}");
        }
        assert!(outcomes(&pool, &[]).await.unwrap().is_empty());

        for target in &targets {
            sqlx::query("DELETE FROM trains WHERE id = $1")
                .bind(target.trains_id)
                .execute(&pool)
                .await
                .ok();
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = $1")
                .bind(&target.train_uid)
                .execute(&pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TDRO-%'")
            .execute(&pool)
            .await
            .ok();
    }
}
