use std::path::PathBuf;

use clap::Parser;
use common::config::{LineCatalogue, parse_lines};
use common::secret::Secret;

/// Where the products go (`INGEST_SINK`, chart value
/// `scheduleFeed.reference.ingest.sink`; ingest architecture spec §9.1,
/// §13.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum IngestSink {
    /// The api's `/private` ingest routes, with the internal `OAuth2`
    /// client: today's path, and the default.
    Http,
    /// Postgres directly, through `ds_store`, as the
    /// `distant_signal_schedule_reference` role (`DATABASE_URL`).
    Db,
}

/// The `db` sink's pool size by default (spec §6.6: publishes are
/// sequential and the final chunk holds one connection; the role's limit is
/// 4). `DATABASE_MAX_CONNECTIONS` overrides it (`common::pg`).
pub(crate) const DB_POOL_SIZE: u32 = 3;

/// [`Config::forward_publish_days`] by default: four weeks ahead (raised
/// from 7 on 2026-10-08 so the train search and DS-MCP's `find_services`
/// can see that far). At production sizes each extra day costs ~190 MB of
/// table and index (~266k `schedule_destination_departures` rows and
/// ~490k `schedule_calling_points_full` rows) and ~30 s of publish cycle.
pub(crate) const DEFAULT_FORWARD_PUBLISH_DAYS: i64 = 28;

/// The smallest [`Config::forward_publish_days`] accepted: the pin horizon
/// itself, `ds_store::tracking::PIN_MAX_DAYS_AHEAD` (28 since 2026-10-08),
/// so raising that constant raises this bound with it. A train may be
/// tracked (pinned, or by uid) that far ahead, and its page reads its stops
/// from `schedule_calling_points_full` and its true origin from
/// `schedule_destination_departures`; `api`'s train search accepts dates
/// up to `SEARCH_WINDOW_FORWARD_DAYS` (`crates/api/src/routes/trains.rs`,
/// 7, inside this bound) ahead. A shorter window would leave both
/// answering dates nothing is published for.
pub(crate) const MIN_FORWARD_PUBLISH_DAYS: i64 = ds_store::tracking::PIN_MAX_DAYS_AHEAD;

/// The largest [`Config::forward_publish_days`] accepted. The cost is
/// linear (see [`DEFAULT_FORWARD_PUBLISH_DAYS`]): 60 days is ~6 GB more
/// table than the default window and ~15 more minutes per cycle. A whole
/// cycle must finish inside the container's /livez stall window
/// (`scheduleFeed.reference.progressStallSecs`, 7200 s), and the further
/// out a date is, the more of its late (STP) changes the CIF does not
/// hold yet.
pub(crate) const MAX_FORWARD_PUBLISH_DAYS: i64 = 60;

const _: () = assert!(
    MIN_FORWARD_PUBLISH_DAYS <= DEFAULT_FORWARD_PUBLISH_DAYS
        && DEFAULT_FORWARD_PUBLISH_DAYS <= MAX_FORWARD_PUBLISH_DAYS
);

/// CLI/env configuration for the `schedule-reference` service.
///
/// Mounts the same PVC `schedule-ingest` writes to, READ-ONLY -- see
/// docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md's
/// Decision 1(c). Never writes to `storage_dir`.
///
/// `Debug` is safe to log: `DATABASE_URL` is a [`Secret`], and the `OAuth2`
/// password is redacted by `InternalOAuthArgs`' own `Debug`.
#[derive(Debug, Parser)]
pub(crate) struct Config {
    /// Where the products go: `http` (the default, today's path) or `db`
    /// (Postgres directly). See [`IngestSink`]. Everything else this
    /// service does -- the delivery scan, the parse, the retries, the
    /// dedup marker -- is the same either way.
    #[arg(long, env = "INGEST_SINK", value_enum, default_value_t = IngestSink::Http)]
    pub ingest_sink: IngestSink,

    /// The `schedule_reference` role's connection, for `INGEST_SINK=db`
    /// (required then, unused otherwise). Pool size and timeouts come from
    /// the shared `DATABASE_*` variables (`common::pg`); the default pool
    /// is [`DB_POOL_SIZE`].
    #[arg(long, env = "DATABASE_URL", hide_env_values = true)]
    pub database_url: Option<Secret>,

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
    /// `crates/ds-store/migrations/20260925130000_schedule_reference_publishes.sql`.
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

    /// The `api` crate's ingestion endpoint for the per-date service-mode
    /// publish (`schedule_services`: train, replacement bus, bus or ferry per
    /// schedule). POST-only, one request per service date with
    /// `?service_date=`, same writer credential as every publish above.
    #[arg(
        env = "SCHEDULE_SERVICES_URL",
        long,
        default_value = "http://api:8080/private/schedule-services"
    )]
    pub schedule_services_url: String,

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

    /// The `api` crate's ingestion endpoint for `tiploc_locations`: every
    /// TIPLOC's name, location type and parent station (see
    /// `crate::locations`). POST-only, same writer credential as
    /// `tiploc_crs_url`.
    #[arg(
        env = "TIPLOC_LOCATIONS_URL",
        long,
        default_value = "http://api:8080/private/tiploc-locations"
    )]
    pub tiploc_locations_url: String,

    /// How many days beyond today the per-date products are published on
    /// every cycle (`SCHEDULE_FORWARD_PUBLISH_DAYS`, chart value
    /// `scheduleFeed.reference.forwardPublishDays`): today through
    /// today+N, inclusive, for `schedule_destination_departures`,
    /// `schedule_calling_points_full` and `schedule_services`. See
    /// `main::publish_cif_derived_products`.
    ///
    /// One window for all three, on purpose: a search result
    /// (`schedule_destination_departures`) links to `/Train/by-uid`, whose
    /// stops come from `schedule_calling_points_full`, and both label buses
    /// and ferries from `schedule_services`. `/Trips/plan` reads
    /// `schedule_calling_points_full` for whatever date it is asked, so it
    /// reaches as far as this window too.
    ///
    /// Bounded below by [`MIN_FORWARD_PUBLISH_DAYS`] and above by
    /// [`MAX_FORWARD_PUBLISH_DAYS`]; see those for why.
    #[arg(
        long,
        env = "SCHEDULE_FORWARD_PUBLISH_DAYS",
        default_value_t = DEFAULT_FORWARD_PUBLISH_DAYS,
        value_parser = clap::value_parser!(i64).range(MIN_FORWARD_PUBLISH_DAYS..=MAX_FORWARD_PUBLISH_DAYS),
    )]
    pub forward_publish_days: i64,

    /// The static line catalogue -- same `--lines-dir`/`LINES_DIR`
    /// `value_parser` pattern as `crates/aggregator/src/config.rs`'s own
    /// field of the same name. Used to build the per-line TIPLOC set this
    /// service's own `schedules_touching` query needs (Task 7) -- a
    /// responsibility this crate did not have before Task 7.
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines)]
    pub lines: LineCatalogue,

    /// Shared, non-secret `OAuth2` client-credentials config (same value
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

    /// `/livez` listener and stall window (SVC-08/INF-9).
    #[command(flatten)]
    pub health: common::service_args::HealthArgs,

    /// Backoff for the startup read of this service's own completion marker
    /// (`main::seed_last_processed_delivery`), retried until it succeeds.
    /// Not a CLI/env flag: fixed in production, overridden only by tests.
    #[arg(skip = STARTUP_SEED_BACKOFF)]
    pub startup_backoff: common::backoff::Backoff,

    /// How each product publish is retried within one cycle before it is
    /// recorded as failed (`main::publish_with_retry`). Not a CLI/env flag:
    /// fixed in production, overridden only by tests.
    #[arg(skip = PUBLISH_RETRY)]
    pub publish_retry: PublishRetry,
}

impl Config {
    /// Rejects a configuration that cannot run: `INGEST_SINK=db` without a
    /// `DATABASE_URL`.
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if self.ingest_sink == IngestSink::Db
            && self.database_url.as_ref().is_none_or(Secret::is_empty)
        {
            anyhow::bail!("INGEST_SINK=db needs DATABASE_URL");
        }
        Ok(())
    }

    /// `DATABASE_URL`, for the `db` sink (after [`Self::validate`]).
    pub(crate) fn database_url(&self) -> anyhow::Result<&str> {
        self.database_url
            .as_ref()
            .map(Secret::expose)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| anyhow::anyhow!("INGEST_SINK=db needs DATABASE_URL"))
    }
}

/// Production value of [`Config::startup_backoff`]: 1s doubling to a 60s cap,
/// so a dependency that comes back after the ~1 minute the 2026-09-26 node
/// reboot's SSO outage lasted is noticed within seconds, while a long outage
/// costs one GET a minute.
pub(crate) const STARTUP_SEED_BACKOFF: common::backoff::Backoff = common::backoff::Backoff::new(
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(60),
);

/// Bounded, in-cycle retry of one product publish -- see
/// `main::publish_with_retry`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PublishRetry {
    /// Total attempts, including the first (so `1` means "no retry").
    pub attempts: u32,
    pub backoff: common::backoff::Backoff,
}

/// Production value of [`Config::publish_retry`]: three attempts, waiting
/// ~5-10s then ~10-20s. Long enough to ride out an `api` pod restart or an
/// `IdP` blip, short enough that a genuinely broken product does not hold the
/// cycle for minutes -- and a product that still fails is picked up again
/// by the next cycle (`main::PublishState`), on its own.
pub(crate) const PUBLISH_RETRY: PublishRetry = PublishRetry {
    attempts: 3,
    backoff: common::backoff::Backoff::new(
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(60),
    ),
};

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
mod tests {
    use clap::Parser;

    use super::{Config, IngestSink};

    /// The repo's own line catalogue and the internal `OAuth2` client's
    /// required arguments, plus `args`.
    fn argv(args: &[&str]) -> Vec<String> {
        let lines = common::manifest_dir!().join("../../lines");
        let mut full: Vec<String> = [
            "schedule-reference",
            "--lines-dir",
            lines.to_str().unwrap(),
            "--internal-oauth-token-url",
            "http://idp/token/",
            "--internal-oauth-client-id",
            "client",
            "--internal-oauth-username",
            "user",
            "--internal-oauth-password",
            "password",
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        full.extend(args.iter().map(ToString::to_string));
        full
    }

    fn parse(args: &[&str]) -> Config {
        Config::try_parse_from(argv(args)).expect("parses")
    }

    #[test]
    fn the_sink_defaults_to_http_and_needs_no_database() {
        // `try_parse_from` also reads the environment, and the DB-gated runs
        // set DATABASE_URL, so the URL is cleared by hand.
        let mut config = parse(&[]);
        assert_eq!(config.ingest_sink, IngestSink::Http);
        config.database_url = None;
        config.validate().unwrap();
    }

    #[test]
    fn the_db_sink_needs_a_database_url() {
        let mut config = parse(&["--ingest-sink", "db"]);
        assert_eq!(config.ingest_sink, IngestSink::Db);
        config.database_url = None;
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("DATABASE_URL"), "{err}");

        let config = parse(&[
            "--ingest-sink",
            "db",
            "--database-url",
            "postgres://u:p@db/ds",
        ]);
        config.validate().unwrap();
        assert_eq!(config.database_url().unwrap(), "postgres://u:p@db/ds");
    }

    #[test]
    fn the_forward_publish_window_defaults_to_28_days_and_is_bounded() {
        assert_eq!(parse(&[]).forward_publish_days, 28);
        assert_eq!(
            parse(&["--forward-publish-days", "45"]).forward_publish_days,
            45
        );
        assert_eq!(
            super::MIN_FORWARD_PUBLISH_DAYS,
            ds_store::tracking::PIN_MAX_DAYS_AHEAD,
            "the minimum is the pin horizon"
        );
        assert_eq!(
            parse(&["--forward-publish-days", "28"]).forward_publish_days,
            28
        );
        // 27 is one day short of the 28-day pin horizon.
        for out_of_range in ["27", "7", "61", "-1"] {
            assert!(
                Config::try_parse_from(argv(&["--forward-publish-days", out_of_range])).is_err(),
                "{out_of_range} must be refused"
            );
        }
    }

    #[test]
    fn an_unknown_sink_is_a_startup_error() {
        assert!(Config::try_parse_from(argv(&["--ingest-sink", "redis"])).is_err());
    }

    #[test]
    fn debug_hides_the_database_url() {
        let config = parse(&["--database-url", "postgres://u:hunter2@db/ds"]);
        assert!(!format!("{config:?}").contains("hunter2"));
    }

    #[test]
    fn the_sink_and_database_variables_keep_their_names() {
        use clap::CommandFactory;
        let command = Config::command();
        let env = |id: &str| {
            command
                .get_arguments()
                .find(|arg| arg.get_id() == id)
                .and_then(|arg| arg.get_env())
                .and_then(|env| env.to_str())
                .map(str::to_string)
        };
        assert_eq!(env("ingest_sink").as_deref(), Some("INGEST_SINK"));
        assert_eq!(env("database_url").as_deref(), Some("DATABASE_URL"));
        assert_eq!(
            env("forward_publish_days").as_deref(),
            Some("SCHEDULE_FORWARD_PUBLISH_DAYS")
        );
    }
}

#[cfg(test)]
mod chart_env_wiring_tests {
    use clap::CommandFactory;

    use super::Config;

    /// The `reference` container's own slice of the schedulefeed Deployment
    /// template -- scoped rather than matching the whole file, so a var set
    /// only on the sibling `ingest` container (which has its own
    /// `API_INGEST_URL`, pointing at a different route) cannot satisfy this
    /// check by accident. `reference` is the last container in the template,
    /// so "from its `- name:` line to the first env `define` after it" (the
    /// sftp and ingest env defines sit at the end of the file) is the whole
    /// block, and a var set only in the ingest define can't satisfy it.
    fn reference_container_block() -> String {
        let chart = common::manifest_dir!()
            .join("../../charts/distant-signal/templates/schedulefeed-deployment.yaml");
        let rendered = std::fs::read_to_string(&chart)
            .unwrap_or_else(|err| panic!("read {}: {err}", chart.display()));
        let marker = "- name: reference";
        let start = rendered.find(marker).expect(
            "the schedulefeed Deployment must still declare a container named `reference`; \
             if it was renamed, update this test's marker",
        );
        let end = rendered[start..]
            .find("\n{{- define ")
            .map_or(rendered.len(), |offset| start + offset);
        rendered[start..end].to_string()
    }

    #[test]
    fn every_api_url_this_config_declares_is_set_on_the_charts_reference_container() {
        let block = reference_container_block();
        let command = Config::command();

        let declared: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .filter(|env| env.ends_with("_URL"))
            // Not an api URL: the db sink's connection, rendered by the
            // chart's `databaseEnvFor` helper (checked below).
            .filter(|env| *env != "DATABASE_URL")
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

        // The db sink (plan 2a.4): `INGEST_SINK` and the role's
        // `DATABASE_URL` (through `databaseEnvFor`, as every DB service).
        assert!(
            block.contains("- name: SCHEDULE_FORWARD_PUBLISH_DAYS"),
            "the reference container must set SCHEDULE_FORWARD_PUBLISH_DAYS \
             (scheduleFeed.reference.forwardPublishDays)"
        );
        assert!(
            block.contains("- name: INGEST_SINK"),
            "the reference container must set INGEST_SINK (scheduleFeed.reference.ingest.sink)"
        );
        assert!(
            block.contains(
                r#"include "distant-signal.databaseEnvFor" (dict "root" . "service" "schedule_reference")"#
            ),
            "the reference container must get DATABASE_URL for the schedule_reference role \
             under the db sink"
        );

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
