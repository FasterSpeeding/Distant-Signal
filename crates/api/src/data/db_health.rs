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

use std::time::Duration;

use sqlx::PgPool;

pub const DB_UP_METRIC: &str = "api_db_up";
pub const DB_PROBE_FAILURES_METRIC: &str = "api_db_probe_failures_total";
/// How often the probe runs.
pub const PROBE_INTERVAL: Duration = Duration::from_secs(15);
/// How long one probe (acquire + `SELECT 1`) may take before it counts as a
/// failure. Above the pool's default 5s acquire timeout, so a busy pool
/// gets its full wait first.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Registers both series: the gauge at 0 and the counter at 0.
pub fn register_metrics() {
    metrics::gauge!(common::metrics::metric_name(DB_UP_METRIC)).set(0.0);
    metrics::counter!(common::metrics::metric_name(DB_PROBE_FAILURES_METRIC)).increment(0);
}

/// One probe: `SELECT 1` through `pool`, within `timeout`.
pub async fn probe(pool: &PgPool, timeout: Duration) -> Result<(), String> {
    match tokio::time::timeout(timeout, sqlx::query("SELECT 1").execute(pool)).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(err)) => Err(err.to_string()),
        Err(_) => Err(format!("no answer within {}s", timeout.as_secs())),
    }
}

/// Exports one probe's outcome. Logs only the transitions, so a long
/// outage logs once rather than every [`PROBE_INTERVAL`].
pub fn record(outcome: &Result<(), String>, was_up: Option<bool>) -> bool {
    let up = outcome.is_ok();
    metrics::gauge!(common::metrics::metric_name(DB_UP_METRIC)).set(if up { 1.0 } else { 0.0 });
    if let Err(err) = outcome {
        metrics::counter!(common::metrics::metric_name(DB_PROBE_FAILURES_METRIC)).increment(1);
        if was_up != Some(false) {
            tracing::error!(error = %err, "api cannot query its database");
        }
    } else if was_up == Some(false) {
        tracing::info!("api can query its database again");
    }
    up
}

/// The probe loop; never returns. Spawn it once, when metrics are on.
pub async fn probe_loop(pool: PgPool) {
    let mut interval = tokio::time::interval(PROBE_INTERVAL);
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut was_up = None;
    loop {
        interval.tick().await;
        let outcome = probe(&pool, PROBE_TIMEOUT).await;
        was_up = Some(record(&outcome, was_up));
    }
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

    /// Nothing listening: the probe fails (fast, on a refused connection)
    /// instead of hanging.
    #[tokio::test]
    async fn the_probe_fails_against_an_unreachable_database() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let pool = sqlx::postgres::PgPoolOptions::new()
            .acquire_timeout(Duration::from_secs(1))
            .connect_lazy(&format!("postgres://u:p@127.0.0.1:{port}/db"))
            .unwrap();
        assert!(probe(&pool, Duration::from_secs(5)).await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with DATABASE_URL set and --ignored"]
    async fn the_probe_succeeds_against_a_live_database() {
        let pool = PgPool::connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL"))
            .await
            .unwrap();
        assert_eq!(probe(&pool, PROBE_TIMEOUT).await, Ok(()));
    }
}
