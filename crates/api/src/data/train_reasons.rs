//! TRUST reason codes for a train, and the train-level `cancelled` flag,
//! for the train-detail read models (`trains::PublicTrainState`,
//! `train_tracking::TrackedTrainState`) and the line trains list.
//!
//! **Write side.** `trust-backlog-consumer` posts a
//! `common::TrainReasonMessage` to `POST /private/train-reasons` for every
//! `0002` Cancellation and `0006` Change of Origin that carries a code
//! ([`upsert_reasons`]). Each is filed against its shared `trains` row in
//! `train_reasons`, one row per (train, message type), the latest winning.
//!
//! **Read side.** Every field is nullable and read at request time:
//! - `cancelled`: `status == "cancelled"`, the same TRUST-derived status
//!   the response already carries. A reinstated train is no longer
//!   cancelled.
//! - `cancelReasonCode`/`cancelReason`: the stored `0002` code and its
//!   text. Served only while `cancelled` is true, so a reinstated train
//!   does not show a stale reason.
//! - `changeOfOriginReasonCode`/`changeOfOriginReason`: the stored `0006`
//!   code and its text, whenever there is one.
//!
//! **Text.** A code is looked up in the Network Rail Historic Delay
//! Attribution Glossary (reference-data/delay-attribution-reasons.tsv,
//! August 2021, bundled at compile time). The text is the glossary's own
//! description, verbatim: industry attribution wording, not Darwin's
//! passenger prose. The text is `None`, and the code is still served, when:
//! - the glossary lacks the code (added or renamed since 2021);
//! - the code is in [`SUPPRESSED_TEXT_CODES`]: system codes whose text
//!   says nothing about why the train is not running.
//!
//! **No delay reason.** TRUST's open movement feed carries no reason on a
//! `0003` Movement, so DS has no equivalent of Darwin's `delayReason`, and
//! no such field is served.

use std::collections::HashMap;

use sqlx::PgPool;

// Moved to ds_store::backlog::reasons (ingest architecture plan 1A.10)
pub use ds_store::backlog::reasons::{
    ReasonFields, SUPPRESSED_TEXT_CODES, StoredReasons, reason_fields, reason_text,
    reasons_for_trains, upsert_reasons,
};

/// Fills the reason fields and each journey stop's `status` on a
/// `PublicTrainState`. Call it after `journey_stops` and the final `status`
/// are known. Best-effort: a DB error leaves the reason codes `None` (but
/// still sets `cancelled` and the stop statuses) and is logged.
pub async fn attach_to_public_state(
    pool: &PgPool,
    mut state: crate::data::trains::PublicTrainState,
) -> crate::data::trains::PublicTrainState {
    let stored = fetch_one(pool, Some(state.trains_id)).await;
    let fields = reason_fields(state.status.as_deref(), stored);
    if let Some(stops) = state.journey_stops.as_mut() {
        let live =
            crate::data::stop_live_status::train_has_live_data(state.status.as_deref(), stops);
        crate::data::stop_live_status::apply(stops, fields.cancelled, live);
    }
    state.cancelled = fields.cancelled;
    state.cancel_reason_code = fields.cancel_reason_code;
    state.cancel_reason = fields.cancel_reason;
    state.change_of_origin_reason_code = fields.change_of_origin_reason_code;
    state.change_of_origin_reason = fields.change_of_origin_reason;
    state
}

/// [`attach_to_public_state`] for `TrackedTrainState`. A pending
/// subscription has no `trains_id` and gets `cancelled` from its status
/// alone.
pub async fn attach_to_tracked_state(
    pool: &PgPool,
    mut state: crate::data::train_tracking::TrackedTrainState,
) -> crate::data::train_tracking::TrackedTrainState {
    let stored = fetch_one(pool, state.trains_id).await;
    apply_to_tracked_state(&mut state, stored);
    state
}

/// Batched form of [`attach_to_tracked_state`], for journey detail: one
/// [`reasons_for_trains`] query for every leg. A DB error leaves the reason
/// codes `None` on every state (still setting `cancelled` and the stop
/// statuses) and is logged, as the single form does per train.
pub async fn attach_to_tracked_states(
    pool: &PgPool,
    states: &mut [crate::data::train_tracking::TrackedTrainState],
) {
    let mut ids: Vec<i64> = states.iter().filter_map(|s| s.trains_id).collect();
    ids.sort_unstable();
    ids.dedup();
    let stored = match reasons_for_trains(pool, &ids).await {
        Ok(map) => map,
        Err(err) => {
            tracing::warn!(error = ?err, "could not read train reasons for journey legs");
            HashMap::new()
        }
    };
    for state in states.iter_mut() {
        let own = state.trains_id.and_then(|id| stored.get(&id).cloned());
        apply_to_tracked_state(state, own);
    }
}

fn apply_to_tracked_state(
    state: &mut crate::data::train_tracking::TrackedTrainState,
    stored: Option<StoredReasons>,
) {
    let fields = reason_fields(state.status.as_deref(), stored);
    if let Some(stops) = state.journey_stops.as_mut() {
        let live =
            crate::data::stop_live_status::train_has_live_data(state.status.as_deref(), stops);
        crate::data::stop_live_status::apply(stops, fields.cancelled, live);
    }
    state.cancelled = fields.cancelled;
    state.cancel_reason_code = fields.cancel_reason_code;
    state.cancel_reason = fields.cancel_reason;
    state.change_of_origin_reason_code = fields.change_of_origin_reason_code;
    state.change_of_origin_reason = fields.change_of_origin_reason;
}

/// Batched form, for `GET /public/lines/{id}/trains` (no journey stops
/// there).
pub async fn attach_to_public_states(
    pool: &PgPool,
    states: &mut [crate::data::trains::PublicTrainState],
) {
    let ids: Vec<i64> = states.iter().map(|s| s.trains_id).collect();
    let mut stored = match reasons_for_trains(pool, &ids).await {
        Ok(map) => map,
        Err(err) => {
            tracing::warn!(error = ?err, "could not read train reasons for line");
            HashMap::new()
        }
    };
    for state in states.iter_mut() {
        let fields = reason_fields(state.status.as_deref(), stored.remove(&state.trains_id));
        state.cancelled = fields.cancelled;
        state.cancel_reason_code = fields.cancel_reason_code;
        state.cancel_reason = fields.cancel_reason;
        state.change_of_origin_reason_code = fields.change_of_origin_reason_code;
        state.change_of_origin_reason = fields.change_of_origin_reason;
    }
}

async fn fetch_one(pool: &PgPool, trains_id: Option<i64>) -> Option<StoredReasons> {
    let trains_id = trains_id?;
    match reasons_for_trains(pool, &[trains_id]).await {
        Ok(mut map) => map.remove(&trains_id),
        Err(err) => {
            tracing::warn!(error = ?err, trains_id, "could not read train reasons");
            None
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::too_many_lines,
    reason = "test code: scenario tests read top to bottom"
)]
mod tests {
    use super::*;
    use crate::data::journey::StopTimetable;
    use ds_store::test_support::train_reason_message as message;

    async fn pool() -> PgPool {
        let url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for DB-gated tests");
        PgPool::connect(&url).await.expect("connect")
    }

    fn stop(hhmm: &str) -> crate::data::journey::JourneyStop {
        let at = Some(format!("2031-05-06T{hhmm}:00Z").parse().unwrap());
        crate::data::journey::JourneyStop {
            crs: None,
            name: None,
            location_type: None,
            parent_crs: None,
            tiploc: None,
            kind: None,
            scheduled_arrival: at,
            scheduled_departure: at,
            actual_arrival: None,
            actual_departure: None,
            estimated_arrival: None,
            estimated_departure: None,
            last_event_type: None,
            variation_status: None,
            delay_minutes: None,
            delay_basis: None,
            stop_status: crate::data::journey::StopStatus::Scheduled,
            skip_source: None,
            platform: None,
            planned_platform: None,
            platform_changed: false,
            platform_status: None,
            booked_platform: None,
            live_status: None,
            late_minutes: None,
            board: None,
            timetable: StopTimetable::default(),
        }
    }

    /// The full public overlay against a real row: reason fields from
    /// `train_reasons`, `cancelled` from the status, and every stop's
    /// status from both.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                attach_to_public_state_fills_reasons_and_stop_statuses -- --ignored --test-threads=1`"]
    async fn attach_to_public_state_fills_reasons_and_stop_statuses() {
        use crate::data::stop_live_status::LiveStopStatus;
        let pool = pool().await;
        let date: chrono::NaiveDate = "2031-05-06".parse().unwrap();
        sqlx::query("DELETE FROM trains WHERE train_uid = 'TRSN03'")
            .execute(&pool)
            .await
            .unwrap();
        upsert_reasons(
            &pool,
            &[
                message(Some("TRSN03"), "0002", "TG", "2031-05-06T08:05:00Z"),
                message(Some("TRSN03"), "0006", "PD", "2031-05-06T07:00:00Z"),
            ],
        )
        .await
        .unwrap();
        let trains_id = crate::data::trains::find_or_create_train(&pool, "TRSN03", date)
            .await
            .unwrap();

        let mut stops = vec![stop("08:00"), stop("08:10"), stop("08:20")];
        stops[0].actual_departure = stops[0].scheduled_departure;
        let state = |status: &str, stops: Vec<crate::data::journey::JourneyStop>| {
            crate::data::trains::PublicTrainState {
                service: crate::data::schedule_services::ServiceModeFields::default(),
                trains_id,
                train_uid: "TRSN03".to_string(),
                service_date: date,
                origin_crs: None,
                origin_name: None,
                destination_crs: None,
                destination_name: None,
                scheduled_departure: None,
                calling_points: None,
                train_id: None,
                headcode: None,
                status: Some(status.to_string()),
                last_reported_location: None,
                last_event_type: None,
                delay_minutes: None,
                delay_basis: None,
                delay_provisional: false,
                working_delay_minutes: None,
                next_calling_point: None,
                eta_next: None,
                eta_source: None,
                skipped_stations: vec![],
                platform: None,
                planned_platform: None,
                journey_stops: Some(stops),
                may_have_arrived: false,
                operator_code: None,
                operator_name: None,
                cancelled: false,
                cancel_reason_code: None,
                cancel_reason: None,
                change_of_origin_reason_code: None,
                change_of_origin_reason: None,
            }
        };

        let cancelled = attach_to_public_state(&pool, state("cancelled", stops.clone())).await;
        assert!(cancelled.cancelled);
        assert_eq!(cancelled.cancel_reason_code.as_deref(), Some("TG"));
        assert_eq!(cancelled.cancel_reason.as_deref(), Some("Driver"));
        assert_eq!(
            cancelled.change_of_origin_reason_code.as_deref(),
            Some("PD")
        );
        assert_eq!(
            cancelled.change_of_origin_reason, None,
            "PD text is suppressed"
        );
        let statuses: Vec<_> = cancelled
            .journey_stops
            .unwrap()
            .iter()
            .map(|s| s.live_status)
            .collect();
        assert_eq!(
            statuses,
            vec![
                Some(LiveStopStatus::Departed),
                Some(LiveStopStatus::Cancelled),
                Some(LiveStopStatus::Cancelled)
            ]
        );

        let mut running_stops = stops;
        running_stops[1].estimated_departure = Some("2031-05-06T08:14:00Z".parse().unwrap());
        running_stops[2].estimated_departure = running_stops[2].scheduled_departure;
        let running = attach_to_public_state(&pool, state("en_route", running_stops)).await;
        assert!(!running.cancelled);
        assert_eq!(running.cancel_reason_code, None);
        let stops = running.journey_stops.unwrap();
        assert_eq!(stops[1].live_status, Some(LiveStopStatus::Late));
        assert_eq!(stops[1].late_minutes, Some(4));
        assert_eq!(stops[2].live_status, Some(LiveStopStatus::OnTime));

        sqlx::query("DELETE FROM trains WHERE train_uid = 'TRSN03'")
            .execute(&pool)
            .await
            .unwrap();
    }

    fn tracked(
        id: i64,
        train_uid: Option<&str>,
        service_date: chrono::NaiveDate,
        trains_id: Option<i64>,
        status: &str,
        stops: Option<Vec<crate::data::journey::JourneyStop>>,
    ) -> crate::data::train_tracking::TrackedTrainState {
        crate::data::train_tracking::TrackedTrainState {
            service: crate::data::schedule_services::ServiceModeFields::default(),
            id,
            service_date,
            pin_origin_crs: None,
            pin_destination_crs: None,
            pin_scheduled_departure: None,
            pin_origin_name: None,
            pin_destination_name: None,
            resolution_status: "resolved".to_string(),
            train_uid: train_uid.map(str::to_string),
            train_id: None,
            schedule_destination_crs: None,
            schedule_destination_name: None,
            schedule_calling_points: None,
            schedule_skipped_stations: vec![],
            schedule_platform: None,
            schedule_planned_platform: None,
            status: Some(status.to_string()),
            last_reported_location: None,
            last_event_type: None,
            delay_minutes: None,
            delay_basis: None,
            delay_provisional: false,
            working_delay_minutes: None,
            next_calling_point: None,
            eta_next: None,
            eta_source: None,
            custom_name: None,
            shared_group_count: 0,
            trains_id,
            journey_stops: stops,
            may_have_arrived: false,
            operator_code: None,
            operator_name: None,
            cancelled: false,
            cancel_reason_code: None,
            cancel_reason: None,
            change_of_origin_reason_code: None,
            change_of_origin_reason: None,
        }
    }

    /// Journey detail's batched overlays (`train_operator::
    /// attach_to_tracked_states` then [`attach_to_tracked_states`]) give
    /// exactly what the per-leg `attach_to_tracked_state` pair gave, across
    /// two service dates, a pending leg and one subscription on two legs.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                batched_leg_overlays_match_the_per_leg_ones -- --ignored --test-threads=1`"]
    async fn batched_leg_overlays_match_the_per_leg_ones() {
        use crate::data::train_operator;
        let pool = pool().await;
        let date: chrono::NaiveDate = "2031-05-06".parse().unwrap();
        let next = date.succ_opt().unwrap();
        let uids = ["TRSN11", "TRSN12"];
        let cleanup = || async {
            sqlx::query("DELETE FROM trains WHERE train_uid = ANY($1)")
                .bind(uids)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = ANY($1)")
                .bind(uids)
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("DELETE FROM tocs WHERE atoc_code = 'Q7'")
                .execute(&pool)
                .await
                .unwrap();
        };
        cleanup().await;
        sqlx::query(
            "INSERT INTO tocs (atoc_code, name, legal_name) \
             VALUES ('Q7', 'Batch Operator', 'Batch Operator Ltd')",
        )
        .execute(&pool)
        .await
        .unwrap();
        for (uid, day, operator) in [
            ("TRSN11", date, "Q7"),
            ("TRSN12", date, "Q6"),
            ("TRSN11", next, "Q5"),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                     (service_date, origin_crs, destination_crs, scheduled, train_uid, operator_atoc) \
                 VALUES ($1, 'ZZA', 'ZZB', TIME '08:00', $2, $3)",
            )
            .bind(day)
            .bind(uid)
            .bind(operator)
            .execute(&pool)
            .await
            .unwrap();
        }
        upsert_reasons(
            &pool,
            &[
                message(Some("TRSN11"), "0002", "TG", "2031-05-06T08:05:00Z"),
                message(Some("TRSN12"), "0006", "YI", "2031-05-06T07:00:00Z"),
            ],
        )
        .await
        .unwrap();
        let first = crate::data::trains::find_or_create_train(&pool, "TRSN11", date)
            .await
            .unwrap();
        let second = crate::data::trains::find_or_create_train(&pool, "TRSN12", date)
            .await
            .unwrap();
        let third = crate::data::trains::find_or_create_train(&pool, "TRSN11", next)
            .await
            .unwrap();

        let mut stops = vec![stop("08:00"), stop("08:10")];
        stops[0].actual_departure = stops[0].scheduled_departure;
        let legs = vec![
            tracked(
                1,
                Some("TRSN11"),
                date,
                Some(first),
                "cancelled",
                Some(stops.clone()),
            ),
            tracked(
                2,
                Some("TRSN12"),
                date,
                Some(second),
                "en_route",
                Some(stops),
            ),
            tracked(3, Some("TRSN11"), next, Some(third), "scheduled", None),
            tracked(4, None, date, None, "pending", None),
            tracked(1, Some("TRSN11"), date, Some(first), "cancelled", None),
        ];

        let mut per_leg = Vec::new();
        for state in legs.clone() {
            let state = train_operator::attach_to_tracked_state(&pool, state).await;
            per_leg.push(attach_to_tracked_state(&pool, state).await);
        }
        let mut batched = legs;
        train_operator::attach_to_tracked_states(&pool, &mut batched).await;
        attach_to_tracked_states(&pool, &mut batched).await;

        let json = |states: &[crate::data::train_tracking::TrackedTrainState]| {
            serde_json::to_value(states).unwrap()
        };
        assert_eq!(json(&batched), json(&per_leg));
        // Not trivially equal: the overlays really landed.
        assert_eq!(batched[0].operator_code.as_deref(), Some("Q7"));
        assert_eq!(batched[0].operator_name.as_deref(), Some("Batch Operator"));
        assert_eq!(batched[0].cancel_reason_code.as_deref(), Some("TG"));
        assert_eq!(batched[1].operator_code.as_deref(), Some("Q6"));
        assert_eq!(
            batched[1].change_of_origin_reason_code.as_deref(),
            Some("YI")
        );
        assert_eq!(batched[2].operator_code.as_deref(), Some("Q5"));
        assert_eq!(batched[3].operator_code, None);
        assert_eq!(batched[4].cancel_reason_code.as_deref(), Some("TG"));
        assert!(
            batched[0].journey_stops.as_ref().unwrap()[1]
                .live_status
                .is_some()
        );

        cleanup().await;
    }
}
