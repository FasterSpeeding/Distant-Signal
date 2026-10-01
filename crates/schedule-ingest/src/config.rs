use std::path::PathBuf;

use clap::Parser;

/// Default for [`Config::cif_file_pattern`]: exactly the name DTD delivers
/// the full CIF timetable under. Locked to that name by the repo owner
/// (2026-09-28), since the same SFTP account now also receives other feeds;
/// any other file is a logged stray, never published as the timetable.
pub const DEFAULT_CIF_FILE_PATTERN: &str = "timetable_full.zip";

/// Default for [`Config::cif_exclude_pattern`]: everything named like a
/// Network Rail CORPUS extract, whatever its extension. The same SFTP
/// account now also receives `CORPUSExtract.json.gz` (CORPUS) and
/// `CORPUSExtract.csv.gz` (SMART berth data), and a zip of either must
/// never become the timetable.
pub const DEFAULT_CIF_EXCLUDE_PATTERN: &str = "CORPUSExtract*";

/// Default for [`CorpusArgs::corpus_file_pattern`]: the RDM delivery name of
/// Network Rail's CORPUS extract. Only this gzipped-JSON shape is loaded;
/// the provider's `CORPUSExtract.csv.gz` is SMART berth data, deliberately
/// ignored (a stray).
pub const DEFAULT_CORPUS_FILE_PATTERN: &str = "CORPUSExtract.json.gz";

/// Sanity checks on each extracted CIF delivery before it is accepted
/// (`cif_check.rs`); a delivery that fails one is quarantined.
#[derive(Debug, Clone, clap::Args)]
pub struct CifCheckArgs {
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
    pub fn checks(&self) -> crate::cif_check::CifChecks {
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
pub struct CorpusArgs {
    /// Load CORPUS deliveries from `watch_dir` into api's
    /// `corpus_locations`. Off by default: until it is on, a CORPUS file is
    /// left in `watch_dir` and reported once as a stray, as before.
    #[arg(long, env, default_value_t = false)]
    pub corpus_ingest_enabled: bool,

    /// Case-insensitive `*` globs (see `pattern.rs`) naming the CORPUS
    /// extract in `watch_dir`. Never a CIF candidate.
    #[arg(long, env, default_value = DEFAULT_CORPUS_FILE_PATTERN)]
    pub corpus_file_pattern: String,

    /// api's CORPUS load endpoint.
    #[arg(long, env, default_value = "http://api:8080/private/corpus-locations")]
    pub corpus_api_url: String,

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
    /// separately, rejected ones in `storage_dir/corpus/rejected/`). Every
    /// processed file is moved out of `watch_dir`.
    #[arg(long, env, default_value_t = 3)]
    pub corpus_retention_keep: u32,
}

/// CLI/env configuration for the `schedule-ingest` service.
///
/// Unlike the now-superseded pull design's equivalent `Config`, this crate
/// makes no outbound SFTP connection at all — it only scans a local mounted
/// directory that the sibling `schedule-sftp` (SFTPGo) container writes
/// into. See
/// docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md.
#[derive(Debug, Parser)]
pub struct Config {
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

    /// The `api` crate's ingestion endpoint for completed schedule feed
    /// sequences.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-feed-ingests"
    )]
    pub api_ingest_url: String,

    /// Shared, non-secret OAuth2 client-credentials config (same value
    /// across all 9 real callers).
    #[command(flatten)]
    pub internal_oauth: common::oauth_client::InternalOAuthArgs,

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
}

/// Every routing/CORPUS/URL env var this `Config` declares must be set on
/// the `ingest` container in
/// `charts/distant-signal/templates/schedulefeed-deployment.yaml` -- the
/// same declared-but-unwired guard as
/// `crates/schedule-reference/src/config.rs`'s own `chart_env_wiring_tests`.
/// An unwired `*_URL` silently falls back to `http://api:8080/...`, a host
/// that does not exist under Helm, and an unwired `CORPUS_*`/`CIF_*` means
/// the chart value an operator sets does nothing.
#[cfg(test)]
mod chart_env_wiring_tests {
    use clap::CommandFactory;

    use super::Config;

    /// The `ingest` container's slice of the schedulefeed Deployment: from
    /// its `- name: ingest` line to the next container (`reference`), so a
    /// var set only on a sibling container cannot satisfy the check.
    fn ingest_container_block() -> String {
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
        rendered[start..end].to_string()
    }

    #[test]
    fn every_routing_corpus_and_url_env_var_is_set_on_the_charts_ingest_container() {
        let block = ingest_container_block();
        let command = Config::command();
        let declared: Vec<String> = command
            .get_arguments()
            .filter_map(|arg| arg.get_env().and_then(|env| env.to_str()))
            .filter(|env| {
                env.starts_with("CORPUS_") || env.starts_with("CIF_") || env.ends_with("_URL")
            })
            .map(str::to_string)
            .collect();
        assert!(
            declared.len() >= 9,
            "sanity check: expected the CIF_*, CORPUS_* and *_URL env vars; got {declared:?}"
        );
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
}
