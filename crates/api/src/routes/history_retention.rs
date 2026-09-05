//! `/public/history-retention`: how many days of `line_status_history` the
//! backend actually retains, so the frontend's history range picker
//! (`/lines/[id]/history`, `frontend/lib/history.ts`) can tell a
//! genuinely-pruned empty result apart from a genuinely-quiet line rather
//! than silently showing a truncated result with no explanation -- see
//! `docs/superpowers/specs/2026-08-31-line-history-graphics-design.md`'s
//! Correction 4. Unauthenticated, read-only -- same `public_router()`
//! pattern as `freshness.rs`.
//!
//! This is a static config echo, not a DB query: `crates/api` doesn't
//! enforce this retention itself (the aggregator's `queries::prune_history`
//! does) -- it only reports the value it was told via
//! `ServiceArguments::history_retention_days`, which operators must keep in
//! sync with the aggregator's own `HISTORY_RETENTION_DAYS` (see that
//! field's own doc comment).
//!
//! `dailyStatsRetentionDays`/`halfHourlyStatsRetentionHours` (Decision 8 of
//! docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md)
//! extend this same echo to the two other retention ceilings the History
//! page's Trends tab needs, so `frontend/lib/history.ts`'s
//! `availableGranularities`/`resolveGranularity` can decide, honestly,
//! which granularity tiers a given date range can actually support instead
//! of guessing or hardcoding a number that could drift from what's
//! actually configured.

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::app::{App, Router};

pub fn router() -> Router {
    Router::new().route(
        "/history-retention",
        axum::routing::get(get_history_retention),
    )
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRetention {
    pub history_retention_days: i64,
    pub daily_stats_retention_days: i64,
    pub half_hourly_stats_retention_hours: i64,
}

async fn get_history_retention(State(app): State<App>) -> Json<HistoryRetention> {
    Json(HistoryRetention {
        history_retention_days: app.config.history_retention_days,
        daily_stats_retention_days: app.config.daily_stats_retention_days,
        half_hourly_stats_retention_hours: app.config.half_hourly_stats_retention_hours,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_as_camel_case() {
        let body = HistoryRetention {
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["historyRetentionDays"], 7);
        assert_eq!(json["dailyStatsRetentionDays"], 300);
        assert_eq!(json["halfHourlyStatsRetentionHours"], 840);
        assert!(json.get("history_retention_days").is_none());
        assert!(json.get("daily_stats_retention_days").is_none());
        assert!(json.get("half_hourly_stats_retention_hours").is_none());
    }
}
