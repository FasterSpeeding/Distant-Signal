use std::path::PathBuf;

use clap::Parser;
use common::config::{LineCatalogue, parse_lines};

/// CLI/env configuration for the `schedule-reference` service.
///
/// Mounts the same PVC `schedule-ingest` writes to, READ-ONLY -- see
/// docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md's
/// Decision 1(c). Never writes to `storage_dir`.
#[derive(Debug, Parser)]
pub struct Config {
    /// Root of the shared PVC -- same path `schedule-ingest`'s own
    /// `--storage-dir` writes into (`crates/schedule-ingest/src/config.rs`),
    /// mounted read-only in this container.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// How often to check `storage_dir` for a new complete delivery.
    /// Independent of the underlying daily delivery cadence -- see
    /// Decision 4: most checks find nothing new, since a fresh delivery
    /// only lands roughly once a day, but reading an already-local
    /// directory listing is cheap.
    #[arg(long, env, default_value_t = 1800)]
    pub poll_interval_secs: u64,

    /// The `api` crate's ingestion endpoint for resolved STANOX/CRS rows.
    #[arg(long, env, default_value = "http://api:8080/private/stanox-crs")]
    pub api_ingest_url: String,

    /// This service's OWN per-delivery completion marker route (`GET` +
    /// `POST /private/schedule-reference-publishes`) -- read once at startup
    /// to seed `last_processed_delivery`, and written once per delivery,
    /// only after EVERY product derived from that delivery has published
    /// successfully. See `main::seed_last_processed_delivery` and
    /// `main::poll_once`.
    ///
    /// **This replaced `schedule_feed_ingests_url` as the seeding source,
    /// and the difference is the whole reason this field exists.** That
    /// route is `schedule-ingest`'s record of having EXTRACTED a delivery
    /// zip, written the moment extraction verifies -- before this service
    /// has read a byte of it. Seeding from it meant a restart of the
    /// `reference` container between "ingest recorded the delivery" and
    /// "reference finished publishing it" (an OOM kill during the in-memory
    /// CIF parse, a rolling deploy, any crash) made this process believe it
    /// had already handled a delivery it had never published, so `poll_once`
    /// short-circuited and every product for that delivery silently never
    /// landed until the next delivery arrived ~24 hours later. See
    /// `crates/api/migrations/20260925130000_schedule_reference_publishes.sql`.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-reference-publishes"
    )]
    pub schedule_reference_publishes_url: String,

    /// The `api` crate's ingestion endpoint for this service's second
    /// responsibility (Task 7): per-line CIF SCHEDULE population publish.
    /// See docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md
    /// Decision 2a/2b.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-line-population"
    )]
    pub schedule_line_population_url: String,

    /// The `api` crate's ingestion endpoint for this service's third
    /// responsibility: the whole-network trip-search fallback's per-CRS,
    /// CIF-derived "next 10 scheduled departures" publish. See
    /// docs/superpowers/specs/2026-09-04-whole-network-trip-search-design.md
    /// Decision 1. POST-only, no GET pair -- see that decision's own note on
    /// why this differs from `schedule_line_population_url`'s route shape.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-network-departures"
    )]
    pub schedule_network_departures_url: String,

    /// The `api` crate's ingestion endpoint for this service's fourth
    /// responsibility: the destination-keyed, CIF-derived whole-network
    /// train-search publish. See
    /// docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
    /// Approach B. POST-only, no GET pair -- same shape as
    /// `schedule_network_departures_url` directly above, and reusing the
    /// same `internal_oauth_group_schedule_reference` writer credential.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-destination-departures"
    )]
    pub schedule_destination_departures_url: String,

    /// The `api` crate's ingestion endpoint for this service's fifth
    /// responsibility (Phase 1 dynamic trip planning): the CIF ALF-derived
    /// fixed-link (interchange time) publish. See
    /// docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase1-cif-interchange-ingestion-plan.md.
    /// POST-only, no GET pair -- same shape as `schedule_network_departures_url`
    /// and `schedule_destination_departures_url` above, and reusing the same
    /// `internal_oauth_group_schedule_reference` writer credential.
    #[arg(
        env = "FIXED_LINKS_URL",
        long,
        default_value = "http://api:8080/private/fixed-links"
    )]
    pub fixed_links_url: String,

    /// The `api` crate's ingestion endpoint for this service's sixth
    /// responsibility (Phase 2 dynamic trip planning): the whole-network,
    /// STP-resolved, un-bucketed calling-point publish -- see
    /// docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase2-connections-array-plan.md
    /// Task 1. POST-only, no GET pair -- same shape as `fixed_links_url`
    /// above, and reusing the same `internal_oauth_group_schedule_reference`
    /// writer credential.
    #[arg(
        env = "SCHEDULE_CALLING_POINTS_FULL_URL",
        long,
        default_value = "http://api:8080/private/schedule-calling-points-full"
    )]
    pub schedule_calling_points_full_url: String,

    /// The `api` crate's ingestion endpoint for this service's seventh
    /// responsibility (Task 4 of the TIPLOC-primary CRS crosswalk plan): the
    /// richer, TIPLOC-primary CRS crosswalk publish, run alongside (not
    /// instead of) the existing `api_ingest_url`/`stanox_crs` publish above
    /// -- see
    /// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md's
    /// "Design decision" section. POST-only, no GET pair -- same shape as
    /// `schedule_network_departures_url` and `schedule_destination_departures_url`
    /// above, and reusing the same `internal_oauth_group_schedule_reference`
    /// writer credential.
    #[arg(
        env = "TIPLOC_CRS_URL",
        long,
        default_value = "http://api:8080/private/tiploc-crs"
    )]
    pub tiploc_crs_url: String,

    /// The static line catalogue -- same `--lines-dir`/`LINES_DIR`
    /// value_parser pattern as `crates/aggregator/src/config.rs`'s own
    /// field of the same name. Used to build the per-line TIPLOC set this
    /// service's own `schedules_touching` query needs (Task 7) -- a
    /// responsibility this crate did not have before Task 7.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines)]
    pub lines: LineCatalogue,

    /// Shared, non-secret OAuth2 client-credentials config (same value
    /// across all 9 real callers).
    #[command(flatten)]
    pub internal_oauth: common::oauth_client::InternalOAuthArgs,

    /// Port for this service's Prometheus `/metrics` endpoint. MUST differ
    /// from the `ingest` sibling container's own metrics port -- both
    /// containers share one Pod network namespace (see this plan's Global
    /// Constraints).
    #[arg(long, env, default_value_t = 9092)]
    pub metrics_port: u16,

    #[command(flatten)]
    pub metrics: common::service_args::MetricsArgs,
}

/// The one invariant this crate cannot check at compile time and that has now
/// been broken twice in production: **every `*_URL` env var this `Config`
/// declares must also be set on the `reference` container in
/// `charts/distant-signal/templates/schedulefeed-deployment.yaml`.**
///
/// Every default above points at `http://api:8080/...`, which resolves only
/// under `docker-compose.yml` (where the api service really is named `api`
/// -- see that file's own `schedule-reference` entry, which deliberately
/// relies on these defaults and sets no URL beyond `API_INGEST_URL`). Under
/// Helm the api Service is `{{ include "distant-signal.apiFullname" . }}` --
/// `<release>-api`, never bare `api` (`_helpers.tpl`'s
/// `distant-signal.apiBaseUrl`) -- so a URL the chart forgets to set does
/// NOT fall back to something workable: it points at a hostname that does
/// not exist in the cluster, and because each `publish_*` function in
/// `main.rs` is deliberately best-effort log-and-continue, the product it
/// publishes silently never lands in Postgres at all, forever, with nothing
/// but a recurring `error!` line to show for it.
///
/// That is exactly what happened to `SCHEDULE_CALLING_POINTS_FULL_URL`
/// (added to this file by the 2026-09-23 dynamic-trip-planning Phase 2
/// commit, which touched no chart file): `schedule_calling_points_full`
/// stayed permanently empty in production, so `GET /Trips/plan` answered
/// "no CIF-derived schedule data has been published for <date> yet" for
/// *every* date, not just a date near the edge of the forward window. The
/// same omission had already happened to `SCHEDULE_FEED_INGESTS_URL`
/// (silently disabling `main::seed_last_processed_delivery`'s
/// restart-dedup), and was caught by hand for `TIPLOC_CRS_URL` only during
/// a late whole-branch review. A test is what makes the next one impossible
/// to ship.
#[cfg(test)]
mod chart_env_wiring_tests {
    use clap::CommandFactory;

    use super::Config;

    /// The `reference` container's own slice of the schedulefeed Deployment
    /// template -- scoped rather than matching the whole file, so a var set
    /// only on the sibling `ingest` container (which has its own
    /// `API_INGEST_URL`, pointing at a different route) cannot satisfy this
    /// check by accident. `reference` is the last container in the template,
    /// so "from its `- name:` line to EOF" is the whole block.
    fn reference_container_block() -> String {
        let chart = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../charts/distant-signal/templates/schedulefeed-deployment.yaml");
        let rendered = std::fs::read_to_string(&chart)
            .unwrap_or_else(|err| panic!("read {}: {err}", chart.display()));
        let marker = "- name: reference";
        let start = rendered.find(marker).expect(
            "the schedulefeed Deployment must still declare a container named `reference`; \
             if it was renamed, update this test's marker",
        );
        rendered[start..].to_string()
    }

    #[test]
    fn every_api_url_this_config_declares_is_set_on_the_charts_reference_container() {
        let block = reference_container_block();
        let command = Config::command();

        let declared: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .filter(|env| env.ends_with("_URL"))
            .map(str::to_string)
            .collect();
        assert!(
            declared.len() >= 6,
            "sanity check: this Config declares several *_URL env vars (stanox-crs, \
             schedule-feed-ingests, line-population, network-departures, \
             destination-departures, fixed-links, calling-points-full, tiploc-crs, plus \
             internal-oauth's token URL); got {declared:?}"
        );

        let missing: Vec<&String> = declared
            .iter()
            .filter(|env| !block.contains(&format!("- name: {env}")))
            .collect();

        assert!(
            missing.is_empty(),
            "these *_URL env vars are declared by crates/schedule-reference/src/config.rs but \
             never set on the `reference` container in \
             charts/distant-signal/templates/schedulefeed-deployment.yaml, so under Helm they \
             silently fall back to their `http://api:8080/...` defaults -- a hostname that does \
             not exist in the cluster, since the chart's api Service is `<release>-api`: \
             {missing:?}"
        );
    }
}
