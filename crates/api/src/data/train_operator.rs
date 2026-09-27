//! A train's operating company, for the train-detail read models
//! (`trains::PublicTrainState` and `train_tracking::TrackedTrainState`),
//! served as `operatorCode`/`operatorName`.
//!
//! Source: the CIF schedule's `BX` ATOC code
//! (`schedule_destination_departures.operator_atoc`), named from the RDM
//! TOC reference table (`tocs`). Both are read at request time and never
//! copied onto `trains`, so no migration is needed. This lets the MCP's
//! `service-detail` tool drop LDBWS `GetServiceDetails` without losing that
//! call's `operator`/`operatorCode`.
//!
//! Every field is `None` rather than guessed when:
//! - the train has no `schedule_destination_departures` row. These are
//!   non-passenger workings such as ECS and freight. Rows are also pruned
//!   after `schedule_destination_departures_retention_days`, so an older
//!   service has none.
//! - the schedule's `BX` line carries no ATOC code.
//! - the rows for this `(train_uid, service_date)` name more than one ATOC
//!   code. This never happened in production on 2026-09-27; the rule is the
//!   same one the `headcode` subselect in `trains::get_public_train_state`
//!   uses.
//!
//! `operatorName` is additionally `None` when the code is not in `tocs`.
//! On 2026-09-27 that was `TW` (Tyne and Wear Metro), `QC` and `LF`. The
//! code is still returned in that case.
//!
//! TRUST's own `toc_id` is deliberately NOT a fallback. It is the numeric
//! TRUST business code (`"79"`, `"88"`, ...), not an ATOC code. Its mapping
//! to ATOC is many-to-one: `88` is the whole of GTR (TL/GN/SN/GX). DS does
//! not store it per train either, so using it would mean guessing.

use std::collections::HashMap;

use chrono::NaiveDate;
use sqlx::PgPool;

/// One train's operator: the ATOC code, plus the `tocs` display name when
/// the reference table knows that code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainOperator {
    pub code: String,
    pub name: Option<String>,
}

/// The operator of every `train_uid` in `train_uids` on `service_date`.
/// Keyed by `train_uid`. A uid with no single known operator is absent from
/// the map (see the module doc). One query for the whole batch, so it is
/// safe to call on a line's whole population.
pub async fn operators_for_trains(
    pool: &PgPool,
    train_uids: &[String],
    service_date: NaiveDate,
) -> anyhow::Result<HashMap<String, TrainOperator>> {
    if train_uids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT o.train_uid, o.operator_atoc, t.name \
         FROM (SELECT train_uid, MIN(operator_atoc) AS operator_atoc \
                 FROM schedule_destination_departures \
                WHERE service_date = $2 AND train_uid = ANY($1) \
                  AND operator_atoc IS NOT NULL \
                GROUP BY train_uid \
               HAVING COUNT(DISTINCT operator_atoc) = 1) o \
         LEFT JOIN tocs t ON t.atoc_code::text = o.operator_atoc",
    )
    .bind(train_uids)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(uid, code, name)| (uid, TrainOperator { code, name }))
        .collect())
}

/// Single-train form of [`operators_for_trains`].
pub async fn operator_for_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: NaiveDate,
) -> anyhow::Result<Option<TrainOperator>> {
    let uid = train_uid.to_string();
    Ok(
        operators_for_trains(pool, std::slice::from_ref(&uid), service_date)
            .await?
            .remove(&uid),
    )
}

/// Fills `operator_code`/`operator_name` on a `PublicTrainState`. This is
/// best-effort: a DB error leaves both `None` and is logged. A missing
/// operator must never fail a train-detail read, the same posture as
/// `routes::train::attach_journey_stops_public`.
pub async fn attach_to_public_state(
    pool: &PgPool,
    mut state: crate::data::trains::PublicTrainState,
) -> crate::data::trains::PublicTrainState {
    match operator_for_train(pool, &state.train_uid, state.service_date).await {
        Ok(operator) => {
            (state.operator_code, state.operator_name) = split(operator);
        }
        Err(err) => {
            tracing::warn!(error = ?err, trains_id = state.trains_id, "could not read train operator");
        }
    }
    state
}

/// Batched form of [`attach_to_public_state`], for
/// `GET /public/lines/{id}/trains`. It fills every state in place with one
/// query.
pub async fn attach_to_public_states(
    pool: &PgPool,
    states: &mut [crate::data::trains::PublicTrainState],
    service_date: NaiveDate,
) {
    let uids: Vec<String> = states.iter().map(|s| s.train_uid.clone()).collect();
    match operators_for_trains(pool, &uids, service_date).await {
        Ok(mut by_uid) => {
            for state in states.iter_mut() {
                (state.operator_code, state.operator_name) = split(by_uid.remove(&state.train_uid));
            }
        }
        Err(err) => {
            tracing::warn!(error = ?err, "could not read train operators for line");
        }
    }
}

/// Fills `operator_code`/`operator_name` on a `TrackedTrainState` (the
/// private `/Train/{trackingId}` and journey-leg read model). A pending or
/// unresolved subscription has no `train_uid`, so it is left unchanged.
pub async fn attach_to_tracked_state(
    pool: &PgPool,
    mut state: crate::data::train_tracking::TrackedTrainState,
) -> crate::data::train_tracking::TrackedTrainState {
    let Some(train_uid) = state.train_uid.clone() else {
        return state;
    };
    match operator_for_train(pool, &train_uid, state.service_date).await {
        Ok(operator) => {
            (state.operator_code, state.operator_name) = split(operator);
        }
        Err(err) => {
            tracing::warn!(error = ?err, tracking_id = state.id, "could not read train operator");
        }
    }
    state
}

fn split(operator: Option<TrainOperator>) -> (Option<String>, Option<String>) {
    match operator {
        Some(TrainOperator { code, name }) => (Some(code), name),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_a_code_without_a_name() {
        assert_eq!(
            split(Some(TrainOperator {
                code: "TW".to_string(),
                name: None
            })),
            (Some("TW".to_string()), None)
        );
        assert_eq!(split(None), (None, None));
    }

    async fn pool() -> PgPool {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for DB-gated tests");
        PgPool::connect(&url).await.expect("connect")
    }

    async fn seed_departure(
        pool: &PgPool,
        uid: &str,
        date: NaiveDate,
        origin: &str,
        operator: Option<&str>,
    ) {
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                 (service_date, origin_crs, destination_crs, scheduled, train_uid, operator_atoc) \
             VALUES ($1, $2, 'ZZB', TIME '08:00', $3, $4)",
        )
        .bind(date)
        .bind(origin)
        .bind(uid)
        .bind(operator)
        .execute(pool)
        .await
        .expect("seed schedule_destination_departures");
    }

    async fn cleanup(pool: &PgPool, uids: &[&str]) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = ANY($1)")
            .bind(uids)
            .execute(pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires DATABASE_URL"]
    async fn operator_is_the_schedules_atoc_code_named_from_tocs() {
        let pool = pool().await;
        let date: NaiveDate = "2031-03-04".parse().unwrap();
        let uids = ["TOP001", "TOP002", "TOP003", "TOP004"];
        cleanup(&pool, &uids).await;
        sqlx::query(
            "INSERT INTO tocs (atoc_code, name, legal_name) VALUES ('Q9', 'Test Operator', 'Test Operator Ltd') \
             ON CONFLICT (atoc_code) DO NOTHING",
        )
        .execute(&pool)
        .await
        .unwrap();
        // Known code, two calling-point rows agreeing.
        seed_departure(&pool, "TOP001", date, "ZZA", Some("Q9")).await;
        seed_departure(&pool, "TOP001", date, "ZZC", Some("Q9")).await;
        // Code with no `tocs` row.
        seed_departure(&pool, "TOP002", date, "ZZA", Some("Q8")).await;
        // No BX ATOC code at all.
        seed_departure(&pool, "TOP003", date, "ZZA", None).await;
        // Conflicting codes: not guessed.
        seed_departure(&pool, "TOP004", date, "ZZA", Some("Q9")).await;
        seed_departure(&pool, "TOP004", date, "ZZC", Some("Q8")).await;

        let owned: Vec<String> = uids
            .iter()
            .map(|s| s.to_string())
            .chain(["TOP999".to_string()])
            .collect();
        let got = operators_for_trains(&pool, &owned, date).await.unwrap();
        assert_eq!(
            got.get("TOP001"),
            Some(&TrainOperator {
                code: "Q9".to_string(),
                name: Some("Test Operator".to_string())
            })
        );
        assert_eq!(
            got.get("TOP002"),
            Some(&TrainOperator {
                code: "Q8".to_string(),
                name: None
            })
        );
        assert!(!got.contains_key("TOP003"), "no ATOC code -> absent");
        assert!(!got.contains_key("TOP004"), "disagreeing codes -> absent");
        assert!(!got.contains_key("TOP999"), "no schedule row -> absent");
        // Another day is another train.
        let other_day = operator_for_train(&pool, "TOP001", date.succ_opt().unwrap())
            .await
            .unwrap();
        assert_eq!(other_day, None);

        cleanup(&pool, &uids).await;
        sqlx::query("DELETE FROM tocs WHERE atoc_code = 'Q9'")
            .execute(&pool)
            .await
            .unwrap();
    }
}
