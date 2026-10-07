//! api's own view of whether it can reach its database:
//! `distant_signal_api_db_up` (1 or 0), set every [`PROBE_INTERVAL`] by a
//! cheap `SELECT 1` through the request pool, and
//! `distant_signal_api_db_probe_failures_total`, one per failed probe.
//!
//! **Why** (2026-10-01): Postgres was down for six hours and nothing alerted
//! on api's side. api kept answering `/livez` and its `/private` ingest
//! routes answered 5xx, which the consumers retried and only logged. The
//! chart's `DistantSignalApiDatabaseDown` alert reads this gauge.
//!
//! Both series exist from the moment metrics are on (the gauge at 0, "not
//! yet confirmed"), and the probe loop starts before the migrations. api
//! only gets that far once it has connected at least once (`App::new` waits
//! for the database), so an api that has never connected exports neither;
//! the chart's `DistantSignalPostgresDown` covers that case. Going through the pool (not a dedicated connection) is
//! deliberate: a pool that cannot hand out a connection within
//! [`PROBE_TIMEOUT`] is as unusable to api as a database that is down.
//!
//! The probe itself is `ds_store::pool` now (plan task 1A.11), shared by
//! every DB service; this module names api's series and wires them up.
//! [`register_metrics`] also registers the pool's `db_pool_*` series, which
//! `ds_store::pool::PoolSettings` samples for the pool `AppState::init`
//! builds.

use sqlx::PgPool;

pub use ds_store::pool::{PROBE_INTERVAL, PROBE_TIMEOUT, probe};

pub const DB_UP_METRIC: &str = "api_db_up";
pub const DB_PROBE_FAILURES_METRIC: &str = "api_db_probe_failures_total";

/// api's two series. Built on each call, after the recorder is installed.
fn health() -> ds_store::pool::DbHealth {
    ds_store::pool::DbHealth::new(
        "api",
        metrics::gauge!(common::metrics::metric_name(DB_UP_METRIC)),
        metrics::counter!(common::metrics::metric_name(DB_PROBE_FAILURES_METRIC)),
    )
}

/// Registers both series (the gauge at 0 and the counter at 0), and the
/// `db_pool_*` series at 0.
pub fn register_metrics() {
    health().register();
    ds_store::pool::register_metrics();
}

/// Exports one probe's outcome; see `ds_store::pool::DbHealth::record`.
pub fn record(outcome: &Result<(), String>, was_up: Option<bool>) -> bool {
    health().record(outcome, was_up)
}

/// The probe loop; never returns. Spawn it once, when metrics are on.
pub async fn probe_loop(pool: PgPool) {
    health().probe_loop(pool).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(f: impl FnOnce()) -> String {
        let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();
        metrics::with_local_recorder(&recorder, f);
        handle.render()
    }

    #[test]
    fn both_series_are_registered_before_any_probe() {
        let rendered = render(register_metrics);
        assert!(
            rendered.contains("distant_signal_api_db_up 0"),
            "{rendered}"
        );
        assert!(
            rendered.contains("distant_signal_api_db_probe_failures_total 0"),
            "{rendered}"
        );
        assert!(
            rendered.contains("distant_signal_db_pool_connections{state=\"in_use\"} 0"),
            "{rendered}"
        );
    }

    #[test]
    fn a_failed_probe_sets_the_gauge_to_0_and_counts_it() {
        let rendered = render(|| {
            register_metrics();
            assert!(record(&Ok(()), None));
            assert!(!record(&Err("connection refused".to_string()), Some(true)));
            assert!(!record(&Err("connection refused".to_string()), Some(false)));
        });
        assert!(
            rendered.contains("distant_signal_api_db_up 0"),
            "{rendered}"
        );
        assert!(
            rendered.contains("distant_signal_api_db_probe_failures_total 2"),
            "{rendered}"
        );
        let recovered = render(|| {
            record(&Ok(()), Some(false));
        });
        assert!(
            recovered.contains("distant_signal_api_db_up 1"),
            "{recovered}"
        );
    }
}
