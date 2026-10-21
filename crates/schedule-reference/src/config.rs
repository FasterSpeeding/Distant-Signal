use std::path::PathBuf;

use clap::Parser;
use common::config::{LineCatalogue, parse_lines};
use common::secret::Secret;

/// The pool size by default (spec §6.6: publishes are
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
/// `Debug` is safe to log: `DATABASE_URL` is a [`Secret`].
#[derive(Debug, Parser)]
pub(crate) struct Config {
    /// Where the products go: Postgres directly through `ds_store`, as
    /// this role (release D, ingest phase 5, removed the `INGEST_SINK`
    /// switch, `db` only since release B).
    ///
    /// The `schedule_reference` role's connection. Pool size and timeouts
    /// come from the shared `DATABASE_*` variables (`common::pg`); the
    /// default pool is [`DB_POOL_SIZE`].
    #[arg(long, env = "DATABASE_URL", hide_env_values = true)]
    pub database_url: Secret,

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
    /// Rejects a configuration that cannot run: an empty `DATABASE_URL`.
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        self.database_url().map(|_| ())
    }

    /// `DATABASE_URL`, refused when empty.
    pub(crate) fn database_url(&self) -> anyhow::Result<&str> {
        Some(self.database_url.expose())
            .filter(|url| !url.is_empty())
            .ok_or_else(|| anyhow::anyhow!("schedule-reference needs DATABASE_URL"))
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
/// ~5-10s then ~10-20s. Long enough to ride out a Postgres restart or a
/// failover, short enough that a genuinely broken product does not hold the
/// cycle for minutes -- and a product that still fails is picked up again
/// by the next cycle (`main::PublishState`), on its own.
pub(crate) const PUBLISH_RETRY: PublishRetry = PublishRetry {
    attempts: 3,
    backoff: common::backoff::Backoff::new(
        std::time::Duration::from_secs(10),
        std::time::Duration::from_secs(60),
    ),
};

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::Config;

    /// The repo's own line catalogue and (unless `args` names one) a
    /// database URL, plus `args`.
    fn argv(args: &[&str]) -> Vec<String> {
        let lines = common::manifest_dir!().join("../../lines");
        let mut full: Vec<String> = ["schedule-reference", "--lines-dir", lines.to_str().unwrap()]
            .iter()
            .map(ToString::to_string)
            .collect();
        if !args.contains(&"--database-url") {
            full.extend([
                "--database-url".to_owned(),
                "postgres://u:p@db/ds".to_owned(),
            ]);
        }
        full.extend(args.iter().map(ToString::to_string));
        full
    }

    fn parse(args: &[&str]) -> Config {
        Config::try_parse_from(argv(args)).expect("parses")
    }

    #[test]
    fn the_database_url_must_not_be_empty() {
        let config = parse(&["--database-url", "postgres://u:p@db/ds"]);
        config.validate().unwrap();
        assert_eq!(config.database_url().unwrap(), "postgres://u:p@db/ds");
        let mut config = parse(&[]);
        config.database_url = common::secret::Secret::new("");
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("DATABASE_URL"), "{err}");
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
    fn debug_hides_the_database_url() {
        let config = parse(&["--database-url", "postgres://u:hunter2@db/ds"]);
        assert!(!format!("{config:?}").contains("hunter2"));
    }

    #[test]
    fn the_database_variables_keep_their_names() {
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
        assert_eq!(env("database_url").as_deref(), Some("DATABASE_URL"));
        assert!(
            command
                .get_arguments()
                .all(|arg| arg.get_env().is_none_or(|env| env != "INGEST_SINK")),
            "INGEST_SINK was removed in release D"
        );
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
    /// only on the sibling `ingest` container cannot satisfy this check by
    /// accident. `reference` is the last container in the template,
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

    /// The `reference` container gets the role's `DATABASE_URL` and its
    /// settings, and no longer `INGEST_SINK` (release D), the api URLs or
    /// the internal OAuth settings (release B).
    #[test]
    fn the_charts_reference_container_sets_this_configs_env() {
        let block = reference_container_block();
        assert!(
            block.contains("- name: SCHEDULE_FORWARD_PUBLISH_DAYS"),
            "the reference container must set SCHEDULE_FORWARD_PUBLISH_DAYS \
             (scheduleFeed.reference.forwardPublishDays)"
        );
        assert!(
            block.contains(
                r#"include "distant-signal.databaseEnvFor" (dict "root" . "service" "schedule_reference")"#
            ),
            "the reference container must get DATABASE_URL for the schedule_reference role"
        );
        let mut declared: Vec<String> = Config::command()
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .filter(|env| env.ends_with("_URL"))
            .map(str::to_string)
            .collect();
        declared.sort();
        assert_eq!(
            declared,
            ["DATABASE_URL", "HEALTH_BIND_URL"],
            "no api URL is left to wire"
        );
        for gone in [
            "INGEST_SINK",
            "API_INGEST_URL",
            "SCHEDULE_REFERENCE_PUBLISHES_URL",
            "SCHEDULE_CALLING_POINTS_FULL_URL",
            "INTERNAL_OAUTH_TOKEN_URL",
            "INTERNAL_OAUTH_PASSWORD",
        ] {
            assert!(
                !block.contains(&format!("- name: {gone}\n")),
                "{gone} is no longer rendered"
            );
        }
    }
}
