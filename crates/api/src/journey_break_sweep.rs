//! The break sweep (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md
//! §5.3, D-4): every `JOURNEY_BREAKS_INTERVAL_SECS`, under its own
//! advisory lock (`common::advisory_locks::JOURNEY_BREAKS_SWEEP`, so with
//! several api replicas one runs it), the api
//!
//! 1. expires open breaks of journeys that are over;
//! 2. picks today's and yesterday's journeys with a train leg leaving
//!    inside the live window (or under way), at most
//!    `JOURNEY_BREAKS_MAX_JOURNEYS_PER_TICK`;
//! 3. judges each on live times (`journey_feasibility`, the same rule as
//!    the journey page) and opens, updates or ends its break;
//! 4. plans suggestions for a new or changed break, or one whose stored
//!    suggestions went stale, through its own planning slot
//!    (`ReplanSlot::Sweep`), so it never takes a user's.
//!
//! The notifier then reads the breaks and pushes (`break_notices`); the
//! api never pushes. Off unless `JOURNEY_BREAKS_ENABLED` (the chart's
//! `api.journeyBreaks.enabled`, default false): the kill switch.

use std::time::{Duration as StdDuration, Instant};

use chrono::{DateTime, Utc};

use crate::app::App;
use crate::data::journey_alternatives::SuggestionSet;
use crate::data::journey_breaks::{self, Detected, Upserted};
use crate::routes::journey_replan;
use crate::routes::trips::ReplanSlot;

/// The sweep's tunables, from the environment (chart `api.journeyBreaks`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepConfig {
    /// `JOURNEY_BREAKS_ENABLED` (default false).
    pub enabled: bool,
    /// `JOURNEY_BREAKS_INTERVAL_SECS` (60).
    pub interval_secs: u64,
    /// `JOURNEY_BREAKS_MAX_JOURNEYS_PER_TICK` (200).
    pub max_journeys_per_tick: i64,
}

impl Default for SweepConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 60,
            max_journeys_per_tick: 200,
        }
    }
}

impl SweepConfig {
    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            enabled: crate::data::trip_plan_live::env_bool(
                "JOURNEY_BREAKS_ENABLED",
                default.enabled,
            ),
            interval_secs: crate::data::trip_plan_live::env_num(
                "JOURNEY_BREAKS_INTERVAL_SECS",
                default.interval_secs,
            )
            .max(1),
            max_journeys_per_tick: crate::data::trip_plan_live::env_num(
                "JOURNEY_BREAKS_MAX_JOURNEYS_PER_TICK",
                default.max_journeys_per_tick,
            ),
        }
    }
}

/// The sweep as a `ds_store::loops` loop, under its advisory lock.
pub fn loop_spec(app: App, config: &SweepConfig) -> ds_store::loops::LoopSpec {
    ds_store::loops::LoopSpec::new(
        common::advisory_locks::JOURNEY_BREAKS_SWEEP,
        StdDuration::from_secs(config.interval_secs),
        move |_pool| {
            let app = app.clone();
            async move {
                let started = Instant::now();
                let result = run_sweep(&app, Utc::now()).await;
                metrics::histogram!(common::metrics::metric_name(
                    "api_journey_breaks_sweep_seconds"
                ))
                .record(started.elapsed().as_secs_f64());
                match result {
                    Ok(swept) => {
                        if swept.breaks > 0 || swept.recovered > 0 || swept.expired > 0 {
                            tracing::info!(
                                journeys = swept.journeys,
                                breaks = swept.breaks,
                                planned = swept.planned,
                                recovered = swept.recovered,
                                expired = swept.expired,
                                "journey break sweep"
                            );
                        }
                        Ok(())
                    }
                    Err(err) => {
                        tracing::error!(error = ?err, "journey break sweep failed; will retry next interval");
                        Err(err)
                    }
                }
            }
        },
    )
}

/// What one tick did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Swept {
    pub journeys: usize,
    pub breaks: usize,
    pub planned: usize,
    pub recovered: usize,
    pub expired: u64,
}

/// One tick (see the module doc). A journey that fails is logged and
/// skipped; the tick goes on.
pub async fn run_sweep(app: &App, now: DateTime<Utc>) -> anyhow::Result<Swept> {
    let config = SweepConfig::from_env();
    let mut swept = Swept {
        expired: journey_breaks::expire_finished(&app.database).await?,
        ..Swept::default()
    };
    let departs = crate::data::travel_order::scheduled_departure_sql(
        "tr.train_uid",
        "tr.service_date",
        "COALESCE(jl.origin_crs, ts.pin_origin_crs, tr.origin_crs)",
    );
    let horizon = crate::data::trip_plan_live::LiveConfig::from_env();
    let sql = format!(
        "SELECT DISTINCT j.id, j.user_id FROM journeys j \
         JOIN journey_legs jl ON jl.journey_id = j.id \
         JOIN train_subscriptions ts ON ts.id = jl.train_subscription_id \
         JOIN trains tr ON tr.id = ts.trains_id \
         WHERE jl.kind = 'train' \
           AND jl.service_date BETWEEN ($1 AT TIME ZONE 'Europe/London')::date - 1 \
                                   AND ($1 AT TIME ZONE 'Europe/London')::date \
           AND COALESCE({departs}, ts.pin_scheduled_departure) \
               BETWEEN $1 - make_interval(mins => $2) AND $1 + make_interval(mins => $3) \
         ORDER BY j.id LIMIT $4"
    );
    let candidates: Vec<(i64, String)> = sqlx::query_as(&sql)
        .bind(now)
        .bind(i32::try_from(horizon.lookback_minutes).unwrap_or(120))
        .bind(i32::try_from(horizon.horizon_minutes).unwrap_or(180))
        .bind(config.max_journeys_per_tick)
        .fetch_all(&app.database)
        .await?;
    for (journey_id, user_id) in candidates {
        swept.journeys += 1;
        if let Err(err) = sweep_journey(app, journey_id, &user_id, now, &mut swept).await {
            tracing::warn!(error = ?err, journey_id, "journey break sweep: one journey failed");
        }
    }
    Ok(swept)
}

async fn sweep_journey(
    app: &App,
    journey_id: i64,
    user_id: &str,
    now: DateTime<Utc>,
    swept: &mut Swept,
) -> anyhow::Result<()> {
    let analysis = journey_replan::analyse(app, journey_id, now).await?;
    if analysis.facts.iter().all(|leg| leg.travelled) {
        return Ok(());
    }
    let open = journey_breaks::open_break(&app.database, journey_id).await?;
    // A `user_missed` break (the traveller's own word, from before
    // "I missed this train" stopped opening breaks, or reopened by an
    // Undo) stands until they act on it or it expires.
    if open.as_ref().is_some_and(|b| b.cause == "user_missed") {
        return Ok(());
    }
    let Some(timings) = analysis.timings.as_ref() else {
        return Ok(());
    };
    let Some(found) =
        journey_breaks::detect(&analysis.facts, &timings.timings, &timings.connections)
    else {
        if let Some(row) = open {
            let mut conn = app.database.acquire().await?;
            journey_breaks::resolve(&mut conn, row.id, "recovered", None, None).await?;
            swept.recovered += 1;
        }
        return Ok(());
    };
    swept.breaks += 1;
    let detected = Detected {
        cause: found.cause,
        fault: "operator",
        severity: found.cause.base_severity(),
        trigger_leg_id: Some(found.trigger_leg_id),
        trigger_trains_id: found.trigger_trains_id,
        break_position: i32::try_from(found.index + 1)?,
        fingerprint: journey_breaks::fingerprint(found.cause, found.index, &timings.timings),
    };
    let (row, upserted) =
        journey_breaks::upsert_open(&app.database, journey_id, user_id, &detected, true).await?;
    let stale = row
        .suggestions
        .clone()
        .and_then(|value| serde_json::from_value::<SuggestionSet>(value).ok())
        .is_none_or(|set| set.is_stale(&row.fingerprint, now));
    if upserted != Upserted::Same || stale {
        match journey_replan::replan_break(app, &row, &analysis, ReplanSlot::Sweep).await {
            Ok(_) => swept.planned += 1,
            Err(err) => {
                metrics::counter!(
                    common::metrics::metric_name("api_journey_break_suggestions_total"),
                    "result" => "error"
                )
                .increment(1);
                tracing::warn!(
                    ?err,
                    journey_id,
                    "journey break sweep: could not plan suggestions"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sweep_is_off_unless_turned_on() {
        let config = SweepConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval_secs, 60);
        assert_eq!(config.max_journeys_per_tick, 200);
    }
}
