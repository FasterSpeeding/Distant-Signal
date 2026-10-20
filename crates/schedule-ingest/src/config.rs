use std::path::PathBuf;

use clap::Parser;
use common::secret::Secret;

/// Default for [`Config::cif_file_pattern`]: exactly the name DTD delivers
/// the full CIF timetable under. Locked to that name by the repo owner
/// (2026-09-28), since the same SFTP account now also receives other feeds;
/// any other file is a logged stray, never published as the timetable.
pub(crate) const DEFAULT_CIF_FILE_PATTERN: &str = "timetable_full.zip";

/// Default for [`Config::cif_exclude_pattern`]: everything named like a
/// Network Rail CORPUS extract, whatever its extension. The same SFTP
/// account now also receives `CORPUSExtract.json.gz` (CORPUS) and
/// `CORPUSExtract.csv.gz` (SMART berth data), and a zip of either must
/// never become the timetable.
pub(crate) const DEFAULT_CIF_EXCLUDE_PATTERN: &str = "CORPUSExtract*";

/// Default for [`CorpusArgs::corpus_file_pattern`]: the RDM delivery name of
/// Network Rail's CORPUS extract. Only this gzipped-JSON shape is loaded;
/// the provider's `CORPUSExtract.csv.gz` is SMART berth data, deliberately
/// ignored (a stray).
pub(crate) const DEFAULT_CORPUS_FILE_PATTERN: &str = "CORPUSExtract.json.gz";

/// Sanity checks on each extracted CIF delivery before it is accepted
/// (`cif_check.rs`); a delivery that fails one is quarantined.
#[derive(Debug, Clone, clap::Args)]
#[expect(
    clippy::struct_field_names,
    reason = "clap derives each env var from the field name, so the prefix is part of the interface"
)]
pub(crate) struct CifCheckArgs {
    /// Quarantine a delivery whose MSN banner `Generated` date is more than
    /// this many days before the delivery (a replayed old extract). Real
    /// deliveries are generated the same day. 0 disables.
    #[arg(long, env, default_value_t = 3)]
    pub cif_max_generated_age_days: u32,

    /// Quarantine an MCA with fewer schedule (`BS`) records. The real full
    /// extract has ~505,000. 0 disables.
    #[arg(long, env, default_value_t = 100_000)]
    pub cif_min_schedules: u64,

    /// Quarantine a delivery whose schedule (`BS`) or TIPLOC (`TI`) count
    /// fell by more than this percentage since the last accepted delivery.
    /// Real day-to-day changes are under 0.3%. 0 disables.
    #[arg(long, env, default_value_t = 20, value_parser = clap::value_parser!(u32).range(0..=100))]
    pub cif_max_record_drop_percent: u32,
}

impl CifCheckArgs {
    pub(crate) fn checks(&self) -> crate::cif_check::CifChecks {
        crate::cif_check::CifChecks {
            max_generated_age_days: self.cif_max_generated_age_days,
            min_schedules: self.cif_min_schedules,
            max_drop_percent: self.cif_max_record_drop_percent,
        }
    }
}

/// Network Rail CORPUS loading (`corpus.rs`,
/// docs/superpowers/specs/2026-09-28-corpus-sftp-ingest-design.md).
#[derive(Debug, Clone, clap::Args)]
#[expect(
    clippy::struct_field_names,
    clippy::struct_excessive_bools,
    reason = "clap derives each env var from the field name, so the prefix is part of the interface; each bool is an independent on/off env switch"
)]
pub(crate) struct CorpusArgs {
    /// Load CORPUS deliveries from `watch_dir` into
    /// `corpus_locations`. Off by default: until it is on, a CORPUS file is
    /// left in `watch_dir` and reported once as a stray, as before.
    #[arg(long, env, default_value_t = false)]
    pub corpus_ingest_enabled: bool,

    /// Case-insensitive `*` globs (see `pattern.rs`) naming the CORPUS
    /// extract in `watch_dir`. Never a CIF candidate.
    #[arg(long, env, default_value = DEFAULT_CORPUS_FILE_PATTERN)]
    pub corpus_file_pattern: String,

    /// The most bytes one extract may decompress to (gzip-bomb guard). The
    /// real extract is ~7 MB of JSON.
    #[arg(long, env, default_value_t = 256 * 1024 * 1024)]
    pub corpus_max_decompressed_bytes: u64,

    /// The fewest rows an extract must carry to replace the table (the
    /// real one has ~56,000), so a truncated or placeholder file cannot
    /// wipe it.
    #[arg(long, env, default_value_t = 10_000)]
    pub corpus_min_rows: usize,

    /// How many processed extracts to keep in `storage_dir/corpus/` (and,
    /// separately, rejected ones in `storage_dir/corpus/rejected/` and
    /// superseded ones in `storage_dir/corpus/superseded/`). Every
    /// processed file is moved out of `watch_dir`.
    #[arg(long, env, default_value_t = 3)]
    pub corpus_retention_keep: u32,

    /// Load the JSON extract (`CORPUS_FILE_PATTERN`, the older CORPUS API
    /// version). `false` leaves those files in `watch_dir` as strays.
    #[arg(long, env, default_value_t = true, action = clap::ArgAction::Set)]
    pub corpus_json_enabled: bool,

    /// Load the NLC XML snapshot (`NLC*.xml.gz`, the newer CORPUS API
    /// version; `corpus_xml.rs`). `false` leaves those files in `watch_dir`
    /// as strays. A file named `NLC*` that is not `.xml.gz` is never loaded
    /// either way: it is reported as format drift.
    #[arg(long, env, default_value_t = true, action = clap::ArgAction::Set)]
    pub corpus_xml_enabled: bool,

    /// The fewest rows an XML snapshot must carry (the 2026-10-09 one has
    /// 12,102: the format holds only TIPLOC-bearing locations).
    #[arg(long, env, default_value_t = 10_000)]
    pub corpus_xml_min_rows: usize,

    /// Which file is loaded when both formats are delivered
    /// (`corpus.rs`, "Choosing the file to load").
    #[arg(long, env, value_enum, default_value_t = FormatPrecedence::Json)]
    pub corpus_format_precedence: FormatPrecedence,

    /// With a preferred format (`json` or `xml`): a file of the other
    /// format is loaded only when no file of the preferred one has been
    /// loaded within this many days (by the archived file's mtime), so the
    /// other takes over if the preferred feed stops. One monthly cycle plus
    /// a few days.
    #[arg(long, env, default_value_t = 35)]
    pub corpus_preferred_grace_days: u32,

    /// After each decision, compare the file with the newest one of the
    /// other format (in `watch_dir` or the archive) and log and export the
    /// differences (`corpus_formats.rs`). Read-only.
    #[arg(long, env, default_value_t = true, action = clap::ArgAction::Set)]
    pub corpus_compare_formats: bool,
}

/// `CORPUS_FORMAT_PRECEDENCE`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum FormatPrecedence {
    /// The newest stable file of either format, by mtime. Each delivery
    /// replaces the table, so with both feeds subscribed the table
    /// alternates between them.
    Newest,
    /// The JSON extract; the XML snapshot only when no JSON extract has
    /// been loaded within `CORPUS_PREFERRED_GRACE_DAYS`. The default: the
    /// JSON extract is the superset (every location, not only TIPLOCs) and
    /// what production loads today.
    #[default]
    Json,
    /// The XML snapshot; the JSON extract only when no XML snapshot has
    /// been loaded within `CORPUS_PREFERRED_GRACE_DAYS`.
    Xml,
}

/// CLI/env configuration for the `schedule-ingest` service.
///
/// Unlike the now-superseded pull design's equivalent `Config`, this crate
/// makes no outbound SFTP connection at all — it only scans a local mounted
/// directory that the sibling `schedule-sftp` (`SFTPGo`) container writes
/// into. See
/// docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md.
#[derive(Debug, Parser)]
pub(crate) struct Config {
    /// Where the SFTP daemon writes incoming files. Scanned each check time
    /// via `std::fs::read_dir` — see `src/scan.rs`.
    #[arg(long, env, default_value = "/data/schedule-feed/incoming")]
    pub watch_dir: PathBuf,

    /// Comma-separated, case-insensitive `*` globs naming CIF SCHEDULE
    /// deliveries in `watch_dir` (see `pattern.rs`).
    #[arg(long, env, default_value = DEFAULT_CIF_FILE_PATTERN)]
    pub cif_file_pattern: String,

    /// Globs (same syntax) that are never CIF candidates even when they
    /// match `cif_file_pattern`. Guards the CIF pipeline against the other
    /// files the same SFTP account receives.
    #[arg(long, env, default_value = DEFAULT_CIF_EXCLUDE_PATTERN)]
    pub cif_exclude_pattern: String,

    /// Network Rail CORPUS loading (`corpus.rs`), off by default.
    #[command(flatten)]
    pub corpus: CorpusArgs,

    /// Root of the shared PVC. Each verified-stable delivery is extracted
    /// into `storage_dir/<timestamp>/` (a compact sortable UTC rendering of
    /// the delivery zip's own mtime -- see `delivery::delivery_dir_name`);
    /// retention pruning operates on this directory's immediate
    /// timestamp-shaped subdirectories.
    #[arg(long, env, default_value = "/data/schedule-feed")]
    pub storage_dir: PathBuf,

    /// Comma-separated HH:MM times, Europe/London — reused directly from
    /// the (now-superseded) pull design's Scheduling section: the window
    /// describes when DTD *produces* the feed, not which party connects.
    ///
    /// No longer controls *when* `watch_dir` gets scanned (see
    /// `poll_interval_secs` for that) — a real delivery once landed outside
    /// every configured slot (mid-afternoon, at DTD SFTP account
    /// provisioning time) and sat unprocessed for hours because the old
    /// design only scanned at these ~9 daily slots. What's left of this
    /// field's job: its *last configured entry* still marks "today's final
    /// realistic chance the production window described by RSPS5046 has to
    /// deliver", used only to decide whether a still-incomplete delivery
    /// logs at `error` vs `info` severity — see `main`'s
    /// `is_final_check_of_day`.
    #[arg(
        long,
        env,
        default_value = "22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00"
    )]
    pub check_times: String,

    /// How often to scan `watch_dir`, in seconds. `scan_incoming` (see
    /// `scan.rs`) is a cheap local `std::fs::read_dir` + per-file `stat` —
    /// no network call, no external rate limit to respect — so there is no
    /// real cost concern scanning far more often than the old design's
    /// sparse `check_times` slots did. This is what actually fixes the
    /// stuck-delivery bug described on `check_times`: every delivery,
    /// whenever it lands, is picked up within roughly one interval instead
    /// of potentially waiting until the next of ~9 daily slots (which could
    /// be many hours away).
    #[arg(long, env, default_value_t = 120)]
    pub poll_interval_secs: u64,

    /// How many complete deliveries to retain on disk (current + fallback).
    /// Renamed from the old `retention_keep_sequences` -- there is no
    /// sequence number any more, just delivery timestamps (see
    /// `main::prune_old_deliveries`). No history/retention requirement
    /// beyond this exists today -- a future "also copy elsewhere for
    /// long-term retention" need should be a separate, purposefully-called
    /// copy step, not a change to this simple keep-N-most-recent behavior.
    /// 3 per the repo owner (2026-09-28): three of each resource, counted
    /// separately from CORPUS's own `corpus_retention_keep`.
    #[arg(long, env, default_value_t = 3)]
    pub retention_keep_deliveries: u32,

    /// PL-5: the most uncompressed bytes one delivery zip may declare
    /// across its entries. A real full CIF is ~1-2 GB; a zip over this is
    /// quarantined (never extracted) rather than allowed to fill the volume.
    #[arg(long, env, default_value_t = 4 * 1024 * 1024 * 1024)]
    pub max_extracted_bytes: u64,

    /// PL-5: the most entries one delivery zip may contain (a real one has
    /// about a dozen).
    #[arg(long, env, default_value_t = 64)]
    pub max_zip_entries: usize,

    /// Content checks on each extracted delivery (`CIF_*`).
    #[command(flatten)]
    pub cif_checks: CifCheckArgs,

    /// How many consecutive polling cycles the delivery zip's mtime and
    /// size must be unchanged before it's treated as stable/complete —
    /// see `scan.rs`. There is no manifest-declared size any more (there is
    /// no manifest at all), so this stability check remains the only
    /// completeness signal available, not a fallback.
    ///
    /// Raised from this crate's original default of `2` alongside
    /// shrinking the scan cadence to `poll_interval_secs` (120s default).
    /// At the old sparse cadence (30 minutes apart during the busiest part
    /// of the overnight window, and up to ~14.5 hours apart between the
    /// 01:30 and 16:00 slots), "2 consecutive stable polls" was already a
    /// strong — if wildly inconsistent — time-based signal. At a 2-minute
    /// cadence, 2 consecutive stable polls is only 4 minutes, which a
    /// brief mid-transfer pause could satisfy by accident. `5` at the new
    /// default interval gives a 10-minute unchanged-on-disk window, which
    /// comfortably covers a transient pause without reintroducing anywhere
    /// near the old design's multi-hour worst-case detection latency.
    #[arg(long, env, default_value_t = 5)]
    pub stability_cycles: u32,

    /// Where each delivery's record and each CORPUS load go: only `db`,
    /// Postgres directly at `DATABASE_URL` (`sink.rs`). `http` (api's
    /// `/private/*` routes) was removed in release B (ingest phase 5,
    /// docs/ingest-phase5-runbook.md), so an old `INGEST_SINK=http` fails
    /// at startup; the next release drops the variable.
    #[arg(long, env = "INGEST_SINK", default_value = "db", value_parser = ["db"])]
    pub ingest_sink: String,

    /// Postgres, as the `schedule_ingest` role. The pool reads the shared
    /// `DATABASE_*` settings (`common::pg`); its default size is 1.
    #[arg(long, env, hide_env_values = true)]
    pub database_url: Secret,

    /// Port for this service's Prometheus `/metrics` endpoint. Stays a
    /// plain field, not part of `MetricsArgs` -- its default differs per
    /// crate and `docker-compose.yml` relies on the code default.
    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,

    #[command(flatten)]
    pub metrics: common::service_args::MetricsArgs,

    /// `/livez` listener and stall window (SVC-08/INF-9).
    #[command(flatten)]
    pub health: common::service_args::HealthArgs,

    /// Which delivery sources run: the SFTP watch directory and the GCS
    /// bucket (`sources.rs`, `bucket.rs`).
    #[command(flatten)]
    pub sources: SourceArgs,
}

/// The delivery sources (docs/schedule-feed-bucket.md; the env contract is
/// the bucket chart plan's "The interface contract").
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct SourceArgs {
    /// Scan `WATCH_DIR` (the SFTP landing folder). `false`: never read it.
    #[arg(long, env, default_value_t = true, action = clap::ArgAction::Set)]
    pub sftp_source_enabled: bool,

    /// Run the GCS bucket source (`bucket.rs`). Off by default.
    #[arg(long, env, default_value_t = false, action = clap::ArgAction::Set)]
    pub bucket_source_enabled: bool,

    /// Which source wins when the two deliver different content within
    /// `DISAGREEMENT_WINDOW_MINUTES` (decision D5: the bucket): a
    /// permutation of `bucket,sftp`. Only used with the bucket on.
    #[arg(long, env, default_value = "bucket,sftp")]
    pub source_precedence: crate::sources::Precedence,

    /// Different content of the same kind from the two sources within this
    /// many minutes is a disagreement
    /// (`DistantSignalScheduleFeedSourcesDisagree`).
    #[arg(long, env, default_value_t = 120)]
    pub disagreement_window_minutes: u32,

    #[command(flatten)]
    pub bucket: BucketArgs,
}

/// The GCS bucket source (`BUCKET_*`, `GOOGLE_*`). Only read when
/// `BUCKET_SOURCE_ENABLED`.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct BucketArgs {
    /// The bucket, listed at its root. Required with the bucket on.
    #[arg(long, env)]
    pub bucket_name: Option<String>,

    /// Base URL of the GCS JSON API. Empty: `https://storage.googleapis.com`.
    /// Set only for a fake-GCS test server.
    #[arg(long, env)]
    pub bucket_base_url: Option<String>,

    /// Key mode: the reader service account's JSON key (mounted from a
    /// sealed Secret). Missing or unreadable is revoked access, retried;
    /// never logged.
    #[arg(long, env)]
    pub google_service_account_path: Option<PathBuf>,

    /// Keyless mode: an `external_account` credential configuration
    /// (`common::gcp_external_account`). Exactly one of this and
    /// `GOOGLE_SERVICE_ACCOUNT_PATH`.
    #[arg(long, env)]
    pub google_application_credentials: Option<PathBuf>,

    /// Comma-separated, case-insensitive `*` globs of the object names to
    /// download (the same matcher as `CIF_FILE_PATTERN`). A match is
    /// downloaded only if it also routes to CIF or (with CORPUS on) CORPUS.
    #[arg(long, env)]
    pub bucket_expected_keys: Option<String>,

    /// Seconds between bucket listings.
    #[arg(long, env, default_value_t = 300)]
    pub bucket_poll_interval_secs: u64,

    /// Delete a confirmed or unexpected object only once it is this old
    /// (by its creation time), so the publisher's read-back and scan
    /// finish first.
    #[arg(long, env, default_value_t = 3600)]
    pub bucket_delete_min_age_secs: u64,

    /// Raw objects kept under `STORAGE_DIR/sources/bucket/archive`.
    #[arg(long, env, default_value_t = 5)]
    pub bucket_archive_keep: u32,

    /// A listed object larger than this is never downloaded (unexpected,
    /// reason `size`).
    #[arg(long, env, default_value_t = 268_435_456)]
    pub bucket_max_object_bytes: u64,

    /// Downloads per poll; the rest wait for the next poll.
    #[arg(long, env, default_value_t = 2)]
    pub bucket_max_downloads_per_poll: u32,

    /// Bytes downloaded per rolling hour (`download_capped` past it).
    #[arg(long, env, default_value_t = 268_435_456)]
    pub bucket_max_download_bytes_per_hour: u64,

    /// Bytes downloaded per rolling 24 hours (`download_capped` past it).
    #[arg(long, env, default_value_t = 1_073_741_824)]
    pub bucket_max_download_bytes_per_day: u64,

    /// Ceiling of the exponential backoff after errors, and the retry
    /// interval while access is revoked.
    #[arg(long, env, default_value_t = 3600)]
    pub bucket_max_backoff_secs: u64,

    /// Ship the bucket's Cloud Audit Logs to stdout. Not implemented yet
    /// (the GCS spec's phase 5): a warning at start says so.
    #[arg(long, env, default_value_t = false, action = clap::ArgAction::Set)]
    pub bucket_audit_logs_ship: bool,

    /// The audit-log bucket (phase 5).
    #[arg(long, env)]
    pub bucket_audit_logs_bucket: Option<String>,

    /// Seconds between audit-log listings (phase 5).
    #[arg(long, env, default_value_t = 600)]
    pub bucket_audit_logs_poll_interval_secs: u64,
}

/// The bucket's credential (exactly one of the two paths).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BucketCredential {
    /// `GOOGLE_SERVICE_ACCOUNT_PATH`.
    Key(PathBuf),
    /// `GOOGLE_APPLICATION_CREDENTIALS`.
    ExternalAccount(PathBuf),
}

impl SourceArgs {
    /// Refuses a configuration the process cannot run: no source at all,
    /// or the bucket on without what it needs. (The chart refuses the same
    /// values at render time.)
    pub(crate) fn validate(&self) -> anyhow::Result<()> {
        if !self.sftp_source_enabled && !self.bucket_source_enabled {
            anyhow::bail!(
                "no delivery source: set SFTP_SOURCE_ENABLED=true and/or BUCKET_SOURCE_ENABLED=true"
            );
        }
        if self.bucket_source_enabled {
            self.bucket.validate()?;
        }
        Ok(())
    }

    /// SFTP alone, every bucket setting at its default (tests).
    #[cfg(test)]
    pub(crate) fn sftp_only() -> Self {
        use clap::Parser as _;
        #[derive(Parser)]
        struct Only {
            #[command(flatten)]
            sources: SourceArgs,
        }
        Only::parse_from(["schedule-ingest"]).sources
    }
}

impl BucketArgs {
    fn validate(&self) -> anyhow::Result<()> {
        let name = self.bucket_name.as_deref().unwrap_or_default();
        let valid_name = (3..=63).contains(&name.len())
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
            && name.as_bytes()[0].is_ascii_alphanumeric()
            && name.as_bytes()[name.len() - 1].is_ascii_alphanumeric();
        if !valid_name {
            anyhow::bail!(
                "BUCKET_NAME must be a GCS bucket name (3-63 of a-z, 0-9, - and _, no dots), got {name:?}"
            );
        }
        if self
            .bucket_expected_keys
            .as_deref()
            .is_none_or(|keys| keys.split(',').all(|k| k.trim().is_empty()))
        {
            anyhow::bail!("BUCKET_EXPECTED_KEYS must list at least one object name (glob)");
        }
        if let Some(url) = self.bucket_base_url.as_deref().filter(|u| !u.is_empty())
            && !(url.starts_with("https://") || url.starts_with("http://"))
        {
            anyhow::bail!("BUCKET_BASE_URL must be an http(s):// URL, got {url:?}");
        }
        self.credential()?;
        if self.bucket_poll_interval_secs == 0 {
            anyhow::bail!("BUCKET_POLL_INTERVAL_SECS must be at least 1");
        }
        if self.bucket_max_backoff_secs < self.bucket_poll_interval_secs {
            anyhow::bail!("BUCKET_MAX_BACKOFF_SECS must be at least BUCKET_POLL_INTERVAL_SECS");
        }
        if self.bucket_archive_keep == 0 || self.bucket_max_downloads_per_poll == 0 {
            anyhow::bail!(
                "BUCKET_ARCHIVE_KEEP and BUCKET_MAX_DOWNLOADS_PER_POLL must be at least 1"
            );
        }
        if !(1 <= self.bucket_max_object_bytes
            && self.bucket_max_object_bytes <= self.bucket_max_download_bytes_per_hour
            && self.bucket_max_download_bytes_per_hour <= self.bucket_max_download_bytes_per_day)
        {
            anyhow::bail!(
                "the bucket caps need 1 <= BUCKET_MAX_OBJECT_BYTES <= BUCKET_MAX_DOWNLOAD_BYTES_PER_HOUR <= BUCKET_MAX_DOWNLOAD_BYTES_PER_DAY"
            );
        }
        Ok(())
    }

    /// Exactly one of the two credential paths.
    pub(crate) fn credential(&self) -> anyhow::Result<BucketCredential> {
        let nonempty = |p: &Option<PathBuf>| p.clone().filter(|p| !p.as_os_str().is_empty());
        match (
            nonempty(&self.google_service_account_path),
            nonempty(&self.google_application_credentials),
        ) {
            (Some(key), None) => Ok(BucketCredential::Key(key)),
            (None, Some(config)) => Ok(BucketCredential::ExternalAccount(config)),
            (Some(_), Some(_)) => anyhow::bail!(
                "set exactly one of GOOGLE_SERVICE_ACCOUNT_PATH (key) and GOOGLE_APPLICATION_CREDENTIALS (keyless), not both"
            ),
            (None, None) => anyhow::bail!(
                "the bucket source needs GOOGLE_SERVICE_ACCOUNT_PATH (key) or GOOGLE_APPLICATION_CREDENTIALS (keyless)"
            ),
        }
    }
}

/// Every routing/CORPUS/URL env var this `Config` declares must be set on
/// the `ingest` container in
/// `charts/distant-signal/templates/schedulefeed-deployment.yaml` -- the
/// same declared-but-unwired guard as
/// `crates/schedule-reference/src/config.rs`'s own `chart_env_wiring_tests`.
/// An unwired `CORPUS_*`/`CIF_*` means the chart value an operator sets
/// does nothing.
#[cfg(test)]
mod chart_env_wiring_tests {
    use clap::CommandFactory;

    use super::Config;

    /// The `ingest` container's slice of the schedulefeed Deployment: from
    /// its `- name: ingest` line to the next container (`reference`), so a
    /// var set only on a sibling container cannot satisfy the check, plus
    /// the `distant-signal.scheduleFeedIngestEnv` define its env is
    /// rendered from (through `distant-signal.mergeEnv`, so
    /// `scheduleFeed.ingest.extraEnv` can override it).
    fn ingest_container_block() -> String {
        const DEFINE: &str = "{{- define \"distant-signal.scheduleFeedIngestEnv\" }}";
        let chart = common::manifest_dir!()
            .join("../../charts/distant-signal/templates/schedulefeed-deployment.yaml");
        let rendered = std::fs::read_to_string(&chart)
            .unwrap_or_else(|err| panic!("read {}: {err}", chart.display()));
        let start = rendered
            .find("- name: ingest\n")
            .expect("the schedulefeed Deployment must still declare a container named `ingest`");
        let end = rendered[start..]
            .find("- name: reference\n")
            .map_or(rendered.len(), |offset| start + offset);
        let mut block = rendered[start..end].to_string();
        let at = rendered
            .find(DEFINE)
            .expect("the ingest env define must exist");
        let rest = &rendered[at + DEFINE.len()..];
        let end_def = rest
            .find("{{- define ")
            .map_or(rendered.len(), |offset| at + DEFINE.len() + offset);
        block.push_str(&rendered[at..end_def]);
        block
    }

    #[test]
    fn every_routing_corpus_and_url_env_var_is_set_on_the_charts_ingest_container() {
        let block = ingest_container_block();
        let command = Config::command();
        let declared: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            // DATABASE_URL: from the shared databaseEnvFor helper
            // (checked below).
            .filter(|env| *env != "DATABASE_URL")
            .filter(|env| {
                ["CORPUS_", "CIF_", "SFTP_SOURCE_", "BUCKET_", "SOURCE_", "DISAGREEMENT_", "GOOGLE_"]
                    .iter()
                    .any(|prefix| env.starts_with(prefix))
                    || env.ends_with("_URL")
            })
            .map(str::to_string)
            .collect();
        assert!(
            declared.len() >= 35,
            "sanity check: expected the CIF_*, CORPUS_*, source, BUCKET_*, GOOGLE_* and *_URL env vars; got {declared:?}"
        );
        for env in [
            "SFTP_SOURCE_ENABLED",
            "BUCKET_SOURCE_ENABLED",
            "SOURCE_PRECEDENCE",
            "DISAGREEMENT_WINDOW_MINUTES",
            "BUCKET_NAME",
            "GOOGLE_SERVICE_ACCOUNT_PATH",
            "GOOGLE_APPLICATION_CREDENTIALS",
            "BUCKET_EXPECTED_KEYS",
            "BUCKET_MAX_BACKOFF_SECS",
        ] {
            assert!(declared.iter().any(|d| d == env), "{env} is declared");
        }
        let missing: Vec<&String> = declared
            .iter()
            .filter(|env| !block.contains(&format!("- name: {env}\n")))
            .collect();
        assert!(
            missing.is_empty(),
            "declared by crates/schedule-ingest/src/config.rs but not set on the `ingest` \
             container in charts/distant-signal/templates/schedulefeed-deployment.yaml: \
             {missing:?}"
        );
    }

    /// Release B: the chart sets the `schedule_ingest` role's
    /// `DATABASE_URL` on this container, and no longer `INGEST_SINK`, the
    /// api URLs or the internal OAuth settings; `INGEST_SINK` accepts only
    /// its default, `db`.
    #[test]
    fn the_db_sink_env_is_set_on_the_charts_ingest_container() {
        let block = ingest_container_block();
        for gone in [
            "INGEST_SINK",
            "API_INGEST_URL",
            "CORPUS_API_URL",
            "INTERNAL_OAUTH_TOKEN_URL",
            "INTERNAL_OAUTH_USERNAME",
            "INTERNAL_OAUTH_PASSWORD",
        ] {
            assert!(
                !block.contains(&format!("- name: {gone}\n")),
                "{gone} is no longer rendered"
            );
        }
        assert!(
            block.contains(
                "include \"distant-signal.databaseEnvFor\" (dict \"root\" . \"service\" \"schedule_ingest\")"
            ),
            "DATABASE_URL from databaseEnvFor, as the schedule_ingest role"
        );
        let command = Config::command();
        let arg = |id: &str| {
            command
                .get_arguments()
                .find(|arg| arg.get_id() == id)
                .unwrap_or_else(|| panic!("no argument {id}"))
        };
        let sink = arg("ingest_sink");
        assert_eq!(
            sink.get_env().and_then(|env| env.to_str()),
            Some("INGEST_SINK")
        );
        let defaults: Vec<String> = sink
            .get_default_values()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect();
        assert_eq!(defaults, ["db"]);
        let possible: Vec<String> = sink
            .get_possible_values()
            .iter()
            .map(|v| v.get_name().to_owned())
            .collect();
        assert_eq!(possible, ["db"], "http and http+shadow are refused");
        assert_eq!(
            arg("database_url").get_env().and_then(|env| env.to_str()),
            Some("DATABASE_URL")
        );
        assert!(arg("database_url").is_required_set());
    }
}

#[cfg(test)]
mod source_args_tests {
    use super::*;
    use crate::sources::{Precedence, SourceId};

    #[derive(clap::Parser)]
    struct Only {
        #[command(flatten)]
        sources: SourceArgs,
    }

    fn parse(args: &[&str]) -> SourceArgs {
        Only::try_parse_from(std::iter::once("schedule-ingest").chain(args.iter().copied()))
            .unwrap()
            .sources
    }

    const BUCKET_ON: [&str; 6] = [
        "--bucket-source-enabled=true",
        "--bucket-name=example-ds-ingest",
        "--bucket-expected-keys=timetable_full.zip,CORPUSExtract.json.gz",
        "--google-service-account-path=/var/run/secrets/distant-signal/gcs/service-account.json",
        "--bucket-poll-interval-secs=300",
        "--bucket-max-backoff-secs=3600",
    ];

    /// The contract's code defaults: SFTP on, the bucket off, D5's
    /// precedence, a two-hour window, and the chart's numeric defaults.
    #[test]
    fn the_defaults_are_sftp_only_and_match_the_chart() {
        let args = SourceArgs::sftp_only();
        assert!(args.sftp_source_enabled);
        assert!(!args.bucket_source_enabled);
        assert_eq!(args.source_precedence, Precedence::default());
        assert_eq!(args.source_precedence.0[0], SourceId::Bucket);
        assert_eq!(args.disagreement_window_minutes, 120);
        let b = &args.bucket;
        assert_eq!(b.bucket_poll_interval_secs, 300);
        assert_eq!(b.bucket_delete_min_age_secs, 3600);
        assert_eq!(b.bucket_archive_keep, 5);
        assert_eq!(b.bucket_max_object_bytes, 268_435_456);
        assert_eq!(b.bucket_max_downloads_per_poll, 2);
        assert_eq!(b.bucket_max_download_bytes_per_hour, 268_435_456);
        assert_eq!(b.bucket_max_download_bytes_per_day, 1_073_741_824);
        assert_eq!(b.bucket_max_backoff_secs, 3600);
        assert!(!b.bucket_audit_logs_ship);
        // Off, nothing else is checked: a stray GOOGLE_* var is harmless.
        args.validate().unwrap();
    }

    #[test]
    fn a_bucket_configuration_is_validated_at_start() {
        parse(&BUCKET_ON).validate().unwrap();
        let keyless = parse(&[
            "--bucket-source-enabled=true",
            "--sftp-source-enabled=false",
            "--bucket-name=example-ds-ingest",
            "--bucket-expected-keys=timetable_full.zip",
            "--google-application-credentials=/var/run/secrets/distant-signal/gcs/credential-config.json",
            "--source-precedence=sftp,bucket",
        ]);
        keyless.validate().unwrap();
        assert!(matches!(
            keyless.bucket.credential().unwrap(),
            BucketCredential::ExternalAccount(_)
        ));
        assert_eq!(keyless.source_precedence.0[0], SourceId::Sftp);

        // BUCKET_ON with `extra` replacing (or adding) flags.
        let refused = |extra: &[&str]| {
            let flag = |arg: &str| arg.split('=').next().unwrap_or_default().to_string();
            let mut args: Vec<&str> = BUCKET_ON
                .iter()
                .copied()
                .filter(|arg| !extra.iter().any(|e| flag(e) == flag(arg)))
                .collect();
            args.extend_from_slice(extra);
            parse(&args).validate().unwrap_err().to_string()
        };
        assert!(refused(&["--bucket-name=Bad.Name"]).contains("BUCKET_NAME"));
        assert!(refused(&["--bucket-expected-keys= , "]).contains("BUCKET_EXPECTED_KEYS"));
        assert!(refused(&["--google-application-credentials=/x"]).contains("exactly one"));
        let keyless_without_config: Vec<&str> = BUCKET_ON
            .iter()
            .copied()
            .filter(|arg| !arg.starts_with("--google-"))
            .collect();
        assert!(
            parse(&keyless_without_config)
                .validate()
                .unwrap_err()
                .to_string()
                .contains("needs")
        );
        assert!(refused(&["--bucket-max-backoff-secs=60"]).contains("BACKOFF"));
        assert!(refused(&["--bucket-max-object-bytes=999999999999"]).contains("caps"));
        assert!(refused(&["--bucket-base-url=ftp://x"]).contains("BUCKET_BASE_URL"));
        assert!(
            parse(&["--sftp-source-enabled=false"])
                .validate()
                .unwrap_err()
                .to_string()
                .contains("no delivery source")
        );
        assert!(
            Only::try_parse_from(["x", "--source-precedence=bucket,bucket"]).is_err(),
            "not a permutation"
        );
    }
}
