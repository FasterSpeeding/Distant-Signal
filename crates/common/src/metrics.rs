//! Shared Prometheus metrics installer for the six binaries that have no
//! HTTP server of their own today (`aggregator`, `enricher`, and the five
//! pollers) -- mirrors `crates/common::ingest`'s precedent of being "the
//! one place that changes" for boilerplate every one of those binaries
//! would otherwise repeat (`crates/common/src/ingest.rs`'s own module doc).
//! `api` does NOT call `install` (see
//! docs/superpowers/plans/2026-08-29-metrics.md's Task 2): it already has
//! an axum listener to attach `axum-prometheus`'s middleware to instead,
//! and composes the same underlying `metrics` facade through that crate.
//!
//! See docs/superpowers/specs/2026-08-29-metrics-design.md's Architecture
//! section for the full reasoning behind this split.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{Context, Result};
use metrics_exporter_prometheus::PrometheusBuilder;

/// Every metric this app emits by hand is prefixed `nr_status_`, so it can
/// never collide with `metrics-exporter-prometheus`'s own process-level
/// defaults (e.g. `process_cpu_seconds_total`) or a future metric from an
/// unrelated process sharing the same Prometheus instance. Callers build a
/// metric's full name through this function rather than hand-writing the
/// prefix at each of the (many) call sites across six crates, so the one
/// place that changes if the prefix itself ever does is this function, not
/// every call site.
pub fn metric_name(suffix: &str) -> String {
    format!("nr_status_{suffix}")
}

/// Installs the process-global Prometheus recorder and starts its embedded
/// HTTP listener on `0.0.0.0:<port>`, serving `/metrics` in Prometheus text
/// exposition format. Must be called exactly once, near the top of `main`,
/// before any `counter!`/`histogram!`/`gauge!` call and before the caller's
/// real work begins -- `metrics`'s macros are silent no-ops against no
/// installed recorder, dropping every observation rather than erroring, if
/// this hasn't run yet.
///
/// No `axum` dependency: `metrics-exporter-prometheus`'s
/// `with_http_listener` spins up its own minimal `hyper`-based listener, so
/// this doesn't pull a web framework into six crates that have never
/// needed one -- confirmed against the crate's docs.rs page as part of
/// this feature's design pass
/// (docs/superpowers/specs/2026-08-29-metrics-design.md).
///
/// Note: the exact builder method names below
/// (`PrometheusBuilder::with_http_listener`, `.install()`) match
/// `metrics-exporter-prometheus` 0.18's documented usage as of the design
/// doc's research pass, but this plan was written without the ability to
/// compile-check against the crate directly -- confirm against `cargo doc
/// -p metrics-exporter-prometheus --open` while implementing this step.
pub fn install(port: u16) -> Result<()> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    PrometheusBuilder::new()
        .with_http_listener(addr)
        .install()
        .context("failed to install the Prometheus metrics exporter")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_name_adds_the_shared_prefix() {
        assert_eq!(metric_name("poller_cycle_total"), "nr_status_poller_cycle_total");
    }

    #[test]
    fn metric_name_does_not_detect_or_strip_an_already_prefixed_suffix() {
        // Documents current behavior rather than testing a real
        // requirement: metric_name always prepends, it never inspects its
        // input. Callers are responsible for passing a bare suffix (e.g.
        // "poller_cycle_total", not "nr_status_poller_cycle_total").
        assert_eq!(
            metric_name("nr_status_poller_cycle_total"),
            "nr_status_nr_status_poller_cycle_total"
        );
    }

    // `install` is not unit-tested here -- see this plan's Global
    // Constraints ("Testing convention for metrics") for why: it sets a
    // process-global recorder exactly once per process, which doesn't
    // compose with Rust's default concurrent, same-binary test execution.
    // Verified instead by Task 1 Step 7's manual curl check below, and
    // implicitly by every downstream task's own manual verification step,
    // each of which depends on `install` having actually started a
    // listener.
}
