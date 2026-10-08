//! `schedule-ingest`: watches a locally mounted directory for a pushed CIF
//! SCHEDULE feed delivery from Network Rail/RDG (a single `.zip` archive,
//! overwritten in place on every new delivery), extracts it once stable,
//! and forwards each new delivery to the `api` crate's ingestion endpoint.
//!
//! See `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md` for
//! the original design and
//! `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`
//! for the correction that reshaped this crate around the real delivery's
//! actual outer shape: one zip, no manifest, no sequence number -- see
//! `delivery.rs` for the replacement detection/dedup/extraction logic. This
//! service never dials out itself -- a sibling `SFTPGo` container receives the
//! push and writes into `watch_dir`; this crate only reads what lands there
//! (see `config.rs`).
//!
//! ## The last-ingested-mtime gap (same shape as the old sequence gap)
//!
//! `GET /private/schedule-feed-ingests` (see `crates/api/src/routes/ingest.rs`)
//! only returns `fetched_at` -- the last known-delivered timestamp -- not a
//! value this process could seed `last_ingested_mtime` from cheaply without
//! re-deriving state. Since `schedule-ingest` keeps no persistent state of
//! its own (state lives in `api`, per the design), a process restart loses
//! `last_ingested_mtime` and `known_stable`/`known_stray_files` -- the next
//! cycle will find the same zip (still present in `watch_dir`, since this
//! crate reads it in place and never deletes/moves it -- see `delivery.rs`)
//! and must not treat it as new. Since PL-13 it never extracts it again:
//! the delivery directory already carries the completion marker (see
//! `delivery::ensure_extracted`). And since 2026-10-01 a delivery api
//! accepted also carries `delivery::INGESTED_RECORD` (the zip's name, size,
//! exact mtime and SHA-256), so the restarted process recognises the zip on
//! its first cycle (`delivery::recognise_completed`): no stability wait, no
//! "stalled upload" ERROR past the final check time, no second POST. A
//! delivery extracted but not known to be posted (the POST had not yet
//! succeeded, or the directory predates the record) skips the wait and is
//! posted once; the `api` insert is `ON CONFLICT (delivered_at) DO
//! NOTHING`, so that is at worst a harmless redundant POST.

mod audit;
mod cif_check;
mod config;
mod corpus;
mod delivery;
mod pattern;
mod scan;
mod sink;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use anyhow::Context;
use chrono::{DateTime, NaiveTime, Utc};
use chrono_tz::Europe::London;
use clap::Parser;
use config::Config;
use delivery::DeliveryRelation;
use pattern::{FilePattern, Routing};
use reqwest::Client;
use scan::{StabilityTracker, scan_incoming};
use serde::Serialize;
use sink::{IngestSink, SinkError};

/// Per-request timeout — matches the other pollers' identical rationale
/// (comfortably short relative to `poll_interval_secs`'s default of two
/// minutes between scans).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Builds the scan-cycle `tokio::time::Interval`, ticking every
/// `poll_interval_secs` -- with `MissedTickBehavior::Delay` rather than the
/// default `Burst`.
///
/// `Burst` fires every missed tick back-to-back with zero gap once a cycle
/// overruns its interval (a slow extraction, a slow POST to `api`) --
/// exactly when the dependency it's calling is already struggling, it
/// would pile up a burst of immediate follow-up scans instead of settling
/// back into its normal cadence. `Delay` instead waits a fresh
/// `poll_interval_secs` from whenever the overrun tick actually completes,
/// so a slow cycle degrades to a slower cadence, never a thundering-herd
/// burst. Same fix, same rationale, as `common::poller_loop`'s own
/// `poll_interval_with_delay_on_overrun`/`aggregator`'s own
/// `cycle_interval`/`enricher`'s own `ticking_interval`. This does not
/// change the "first tick fires immediately" property relied on above --
/// that's governed by the interval's start instant, not its missed-tick
/// behavior, which only applies once the loop is already running. Split
/// into its own function so the configuration is directly assertable in a
/// unit test via `Interval::missed_tick_behavior()`, since the missed-tick
/// BEHAVIOR itself (skipping ticks under a real overrun) isn't practically
/// observable without a slow, flaky, real-time test.
fn scan_interval(poll_interval_secs: u64) -> tokio::time::Interval {
    let mut interval = tokio::time::interval(Duration::from_secs(poll_interval_secs));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    interval
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    common::logging::exit_code(run().await)
}

#[expect(
    clippy::expect_used,
    reason = "parse_check_times guarantees a non-empty list"
)]
#[expect(
    clippy::too_many_lines,
    reason = "the process's startup sequence, read top to bottom; plan 2d's sink choice tipped it over"
)]
async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    common::logging::init("schedule-ingest");

    let config = Config::parse();
    common::metrics::ingest_sink_info(&common::metrics::value_enum_name(&config.ingest_sink));
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let progress = health_http::spawn_liveness(&config.health);

    let check_times = parse_check_times(&config.check_times)?;
    // The *last* entry in the configured (not sorted) list is treated as
    // the day's final fallback check — per the plan, derived generically
    // from whatever the operator configured rather than hardcoding
    // RSPS5046's documented `16:00` fallback. The default `check_times`
    // value is deliberately not chronologically sorted (it walks
    // `22:00..01:30` overnight, then `16:00` the following afternoon as a
    // catch-all), so "last in the list" and "chronologically latest" are
    // NOT the same thing — this must stay a positional lookup.
    let final_check_time = *check_times
        .last()
        .expect("parse_check_times guarantees a non-empty list");
    // The *first* configured entry marks when today's overnight production
    // window generically reopens (`22:00` by default) -- used alongside
    // `final_check_time` to bound "final check of day" to the gap between
    // the fallback deadline and the next window's start, rather than
    // leaving it open-ended for the rest of the day. See
    // `is_final_check_of_day`'s own doc comment for why an open-ended
    // comparison is wrong here.
    let window_start_time = *check_times
        .first()
        .expect("parse_check_times guarantees a non-empty list");

    let routing = Routing {
        cif: FilePattern::parse(&config.cif_file_pattern)
            .map_err(|err| anyhow::anyhow!("CIF_FILE_PATTERN: {err}"))?,
        cif_exclude: FilePattern::parse(&config.cif_exclude_pattern)
            .map_err(|err| anyhow::anyhow!("CIF_EXCLUDE_PATTERN: {err}"))?,
        corpus: if config.corpus.corpus_ingest_enabled {
            Some(
                FilePattern::parse(&config.corpus.corpus_file_pattern)
                    .map_err(|err| anyhow::anyhow!("CORPUS_FILE_PATTERN: {err}"))?,
            )
        } else {
            None
        },
    };
    if config.corpus.corpus_ingest_enabled {
        corpus::register_metrics();
        tracing::info!(pattern = %config.corpus.corpus_file_pattern, "CORPUS ingest enabled");
    }

    // Plan 2d.1: api's routes (`http`, the default) or Postgres (`db`).
    let sink = match config.ingest_sink {
        sink::SinkKind::Http => sink::Sink::Http(sink::HttpSink::new(
            Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            config.api_ingest_url.clone(),
            config.corpus.corpus_api_url.clone(),
            config.internal_oauth.token_cache(),
        )),
        sink::SinkKind::Db => {
            let database_url = config
                .database_url
                .as_ref()
                .context("INGEST_SINK=db needs DATABASE_URL")?;
            tracing::info!("INGEST_SINK=db: feed markers and CORPUS loads go to Postgres directly");
            sink::Sink::Db(sink::DbSink::connect(database_url, &progress).await?)
        }
    };
    // Registered at 0 so the alert's increase() sees the first rejection.
    metrics::counter!(common::metrics::metric_name(ZIP_REJECTED_METRIC)).increment(0);
    metrics::counter!(common::metrics::metric_name(INGEST_REJECTED_METRIC)).increment(0);

    // PL-6: directories extracted before the completion marker existed are
    // adopted (or left for re-extraction), and scratch directories from an
    // interrupted extraction removed, before anything else touches the
    // volume.
    match delivery::adopt_legacy_deliveries(&config.storage_dir, &config.watch_dir, &routing) {
        Ok(adopted) if !adopted.is_empty() => {
            tracing::info!(adopted = ?adopted, "marked pre-existing complete delivery directories as complete");
        }
        Ok(_) => {}
        Err(err) => {
            tracing::error!(error = ?err, "failed to adopt pre-existing delivery directories; they stay invisible to schedule-reference until re-extracted");
        }
    }

    let mut tracker = StabilityTracker::new();
    let mut known_stable: HashSet<String> = HashSet::new();
    let mut known_stray_files: HashSet<String> = HashSet::new();
    let mut last_ingested_mtime: Option<SystemTime> = None;
    let mut pending_post: Option<ScheduleFeedIngestRequest> = None;
    let mut rejected_mtime: Option<SystemTime> = None;
    let mut corpus_state = corpus::CorpusState::new();

    // `tokio::time::interval`'s first `tick()` fires immediately regardless
    // of missed-tick behavior, so every run -- including the very first --
    // scans right away with no special-cased bypass needed; subsequent
    // ticks are `poll_interval_secs` apart. See `scan_interval`'s own doc
    // comment for why it also sets `MissedTickBehavior::Delay`.
    let mut interval = scan_interval(config.poll_interval_secs);

    loop {
        progress.idle(interval.tick()).await;

        // `check_times`/`final_check_time` no longer drive *when* a scan
        // happens (that's `poll_interval_secs` now) -- their one remaining
        // job is this severity gate: has today's last configured check
        // time (the day's final realistic chance per RSPS5046) already
        // passed? Compared directly against the current wall-clock time
        // rather than derived from "did the scheduler just wake for that
        // exact slot", since scanning is no longer tied to waking at
        // specific slots.
        let now_london = Utc::now().with_timezone(&London);
        let is_final_check_of_day =
            is_final_check_of_day(now_london.time(), final_check_time, window_start_time);
        let cycle_start = Instant::now();

        if let Err(err) = run_scan_cycle(
            &sink,
            &config,
            &routing,
            &mut tracker,
            &mut known_stable,
            &mut known_stray_files,
            &mut last_ingested_mtime,
            &mut pending_post,
            &mut rejected_mtime,
            is_final_check_of_day,
        )
        .await
        {
            tracing::error!(error = ?err, "scan cycle failed unexpectedly; will retry next poll interval");
        }
        if config.corpus.corpus_ingest_enabled
            && let Err(err) = corpus::run_corpus_cycle(
                &sink,
                &config.watch_dir,
                &config.storage_dir,
                &config.corpus,
                &routing,
                config.stability_cycles,
                &mut corpus_state,
            )
            .await
        {
            tracing::error!(error = ?err, "CORPUS cycle failed unexpectedly; will retry next poll interval");
        }
        progress.beat();

        metrics::histogram!(common::metrics::metric_name(
            "schedule_feed_scan_duration_seconds"
        ))
        .record(cycle_start.elapsed().as_secs_f64());
    }
}

/// One poll interval's worth of work: scan `watch_dir`, feed the snapshot
/// into the (process-lifetime) `StabilityTracker`, and if the newest `.zip`
/// candidate is stable and represents a new delivery (per its mtime --
/// see `delivery::classify_delivery`), extract it into `storage_dir` and
/// record it with `api`.
///
/// Returns `Err` only for genuinely unexpected failures (e.g. `watch_dir`
/// itself unreadable); every "not ready yet" / "already ingested" outcome
/// is handled internally via logging and an early `Ok(())`, so a single bad
/// cycle never crashes the process.
#[expect(
    clippy::cast_precision_loss,
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "each argument is an independent input from the single caller; a struct would only wrap them; metric gauges take f64, and these counts and timestamps stay far below 2^52; long but linear; splitting it would scatter its shared state across helpers"
)]
async fn run_scan_cycle(
    sink: &impl IngestSink,
    config: &Config,
    routing: &Routing,
    tracker: &mut StabilityTracker,
    known_stable: &mut HashSet<String>,
    known_stray_files: &mut HashSet<String>,
    last_ingested_mtime: &mut Option<SystemTime>,
    pending_post: &mut Option<ScheduleFeedIngestRequest>,
    rejected_mtime: &mut Option<SystemTime>,
    is_final_check_of_day: bool,
) -> anyhow::Result<()> {
    // Retry a previously-extracted-but-not-yet-successfully-posted delivery
    // first. Its files already live under `storage_dir/<timestamp>/` (see
    // this function's tail below) -- this only retries the HTTP call, using
    // the exact sizes observed at extraction time, not a re-stat. If this
    // process restarts before a pending POST ever succeeds, that in-memory
    // pending record is lost too (same class of limitation as the mtime gap
    // documented in this module's doc comment) -- a real but narrow gap,
    // not silently pretended away.
    if let Some(pending) = pending_post.take() {
        match sink.record_feed_ingest(&pending).await {
            Ok(()) => {
                tracing::info!(
                    delivered_at = %pending.delivered_at,
                    "retried and succeeded posting a previously-failed ingest record"
                );
                audit::decision(
                    &pending.delivered_file(),
                    pending.delivered_at,
                    audit::Outcome::Accepted,
                    None,
                );
                record_ingested(&config.storage_dir, &pending);
                *last_ingested_mtime = Some(SystemTime::from(pending.delivered_at));
                metrics::gauge!(common::metrics::metric_name(
                    "schedule_feed_last_ingest_delivered_at_seconds"
                ))
                .set(pending.delivered_at.timestamp() as f64);
            }
            Err(err) => {
                queue_or_quarantine_failed_post(err, pending, pending_post, rejected_mtime);
            }
        }
    }

    let snapshot = scan_incoming(&config.watch_dir)?;

    let just_stabilized = tracker.observe(&snapshot, config.stability_cycles);
    known_stable.extend(just_stabilized);
    // Anything that vanished from the directory since the last snapshot is
    // no longer "known stable" -- mirrors `StabilityTracker::observe`'s own
    // drop-and-restart-from-zero behavior for the same filenames.
    known_stable.retain(|name| snapshot.0.contains_key(name));

    let mut candidates = delivery::find_zip_candidates(&snapshot, routing);
    if candidates.len() > 1 {
        tracing::warn!(
            candidates = ?candidates.iter().map(|(name, _)| name.clone()).collect::<Vec<_>>(),
            "multiple .zip files present at once in watch_dir; using the most recently modified"
        );
    }
    let winner = candidates.pop();

    // Anything in `watch_dir` that isn't this cycle's winning zip candidate
    // is unrecognized -- a stray file that would otherwise sit there
    // completely silently (the exact gap that made the original
    // zip-vs-manifest mismatch this crate was built to fix so hard to
    // diagnose in production). Logged at `warn`, but only when the set of
    // stray names actually changes since the last cycle -- otherwise a
    // single leftover file would re-log every `poll_interval_secs` forever,
    // drowning out the signal.
    //
    // A CORPUS candidate is not stray while CORPUS ingest is on: it is
    // `corpus.rs`'s to handle (`routing.is_corpus` is false while it is off).
    let stray: HashSet<String> = snapshot
        .0
        .keys()
        .filter(|name| Some(name.as_str()) != winner.as_ref().map(|(name, _)| name.as_str()))
        .filter(|name| !routing.is_corpus(name))
        .cloned()
        .collect();
    if &stray != known_stray_files {
        if !stray.is_empty() {
            let mut names: Vec<&String> = stray.iter().collect();
            names.sort();
            tracing::warn!(files = ?names, "watch_dir contains file(s) not recognized as the current delivery candidate");
        }
        *known_stray_files = stray;
    }

    let Some((zip_filename, zip_mtime)) = winner else {
        if is_final_check_of_day {
            tracing::error!(
                "no .zip delivery observed in watch_dir by the day's final configured check time; likely a real delivery problem"
            );
        } else {
            tracing::debug!("no .zip file present in watch_dir yet");
        }
        return Ok(());
    };

    // After a restart nothing in memory says this zip was already handled;
    // `storage_dir` does (see `delivery::recognise_completed`). Recognising
    // it here, before the stability gate, means a restart neither waits
    // `stability_cycles` on a zip that finished uploading long ago (logging
    // it as a stalled upload past the final check time) nor posts it again.
    let zip_path = config.watch_dir.join(&zip_filename);
    let mut extracted_before_restart = false;
    if *last_ingested_mtime != Some(zip_mtime)
        && *rejected_mtime != Some(zip_mtime)
        && pending_post.is_none()
    {
        let zip_bytes = snapshot.0.get(&zip_filename).map_or(0, |&(_, len)| len);
        match delivery::recognise_completed(&config.storage_dir, &zip_path, zip_mtime, zip_bytes) {
            Ok(Some(delivery::Recognised::Ingested(record))) => {
                tracing::info!(
                    zip = %zip_filename,
                    dir = %delivery::delivery_dir_name(zip_mtime),
                    sha256 = %record.zip_sha256,
                    "zip delivery was already extracted and posted before this process started; not waiting or posting again"
                );
                *last_ingested_mtime = Some(zip_mtime);
                metrics::gauge!(common::metrics::metric_name(
                    "schedule_feed_last_ingest_delivered_at_seconds"
                ))
                .set(DateTime::<Utc>::from(zip_mtime).timestamp() as f64);
                return Ok(());
            }
            Ok(Some(delivery::Recognised::Extracted)) => {
                tracing::info!(
                    zip = %zip_filename,
                    dir = %delivery::delivery_dir_name(zip_mtime),
                    "zip delivery was already extracted before this process started but is not known to be posted; posting it without a stability wait"
                );
                extracted_before_restart = true;
            }
            Ok(None) => {}
            Err(err) => {
                tracing::warn!(error = ?err, zip = %zip_filename, "could not check whether the zip delivery was already completed; waiting for it to be stable");
            }
        }
    }

    if !extracted_before_restart && !known_stable.contains(&zip_filename) {
        if is_final_check_of_day {
            tracing::error!(
                zip = %zip_filename,
                "zip file present but still not stable by the day's final configured check time; may indicate a stalled or partial upload"
            );
        } else {
            tracing::info!(zip = %zip_filename, "zip file present but not yet stable");
        }
        return Ok(());
    }

    match delivery::classify_delivery(*last_ingested_mtime, zip_mtime) {
        DeliveryRelation::AlreadyIngested => {
            // Steady state for most of the day once today's delivery has
            // been ingested -- never escalated by `is_final_check_of_day`,
            // since "already done today" is the healthy outcome, not a
            // problem.
            tracing::info!(zip = %zip_filename, "zip delivery already ingested; heartbeat only");
            return Ok(());
        }
        DeliveryRelation::New => {}
    }

    // PL-5: this exact delivery was rejected as over the extraction caps
    // (or internally inconsistent). Its bytes cannot change without its
    // mtime changing, so it stays quarantined until a new upload replaces
    // it, instead of being re-read every cycle.
    if *rejected_mtime == Some(zip_mtime) {
        tracing::debug!(zip = %zip_filename, "zip delivery is quarantined; waiting for a new upload");
        return Ok(());
    }

    let delivered_at: DateTime<Utc> = DateTime::<Utc>::from(zip_mtime);
    // PL-13: this delivery's POST already failed and is queued for retry at
    // the top of every cycle -- a second, identical request is pointless.
    if pending_post
        .as_ref()
        .is_some_and(|pending| pending.delivered_at == delivered_at)
    {
        tracing::info!(zip = %zip_filename, "zip delivery already extracted; its api POST is pending retry");
        return Ok(());
    }

    let dir_name = delivery::delivery_dir_name(zip_mtime);

    // Provenance: the zip's SHA-256 as read now. A size different from the
    // stable snapshot's means a new upload has started; wait for it.
    let delivered = match audit::DeliveredFile::hash_file(&zip_filename, &zip_path) {
        Ok(delivered) => delivered,
        Err(err) => {
            tracing::error!(error = %err, zip = %zip_filename, "failed to read the zip delivery to hash it; retrying next cycle");
            return Ok(());
        }
    };
    if snapshot.0.get(&zip_filename).map(|&(_, len)| len) != Some(delivered.bytes) {
        tracing::info!(zip = %zip_filename, "zip delivery changed while being hashed (a new upload?); retrying next cycle");
        return Ok(());
    }

    // PL-6/PL-13: atomic (temp dir, fsync, marker, rename), and a no-op for
    // a delivery that is already complete on disk.
    let limits = delivery::ExtractLimits {
        max_total_bytes: config.max_extracted_bytes,
        max_entries: config.max_zip_entries,
    };
    // The CIF content checks (`cif_check.rs`): run on the extracted files
    // before the delivery is marked complete, against the last accepted
    // delivery; a failure quarantines it like an over-cap zip.
    let checks = config.cif_checks.checks();
    let check = |extracted: &std::path::Path| -> anyhow::Result<()> {
        let stats = cif_check::inspect(extracted)?;
        let previous = cif_check::previous_stats(&config.storage_dir, &dir_name);
        cif_check::check(&stats, previous.as_ref(), delivered_at, &checks)?;
        tracing::info!(
            zip = %zip_filename,
            generated = %stats.generated,
            sequence = ?stats.sequence,
            schedules = stats.schedules,
            tiplocs = stats.tiplocs,
            previous_schedules = ?previous.as_ref().map(|p| p.schedules),
            "CIF delivery passed its checks"
        );
        cif_check::write_stats(extracted, &stats)
    };
    let extracted = match delivery::ensure_extracted(
        &zip_path,
        &config.storage_dir,
        &dir_name,
        limits,
        check,
    ) {
        Ok((extracted, how)) => {
            tracing::info!(zip = %zip_filename, dir = %dir_name, outcome = ?how, sha256 = %delivered.sha256, "delivery directory complete");
            extracted
        }
        Err(err) if delivery::is_rejected(&err) => {
            tracing::error!(error = %err, zip = %zip_filename, "quarantining a zip delivery that can never be extracted; waiting for a new upload");
            audit::decision(
                &delivered,
                delivered_at,
                audit::Outcome::Quarantined,
                Some(&err.to_string()),
            );
            metrics::counter!(common::metrics::metric_name(
                "schedule_feed_zip_rejected_total"
            ))
            .increment(1);
            *rejected_mtime = Some(zip_mtime);
            return Ok(());
        }
        Err(err) => {
            tracing::error!(error = ?err, zip = %zip_filename, "failed to extract a stable zip delivery; retrying next cycle");
            return Ok(());
        }
    };

    let files = extracted
        .into_iter()
        .map(|file| ScheduleFeedFile {
            name: file.name,
            bytes: file.bytes,
            sha256: file.sha256,
        })
        .collect();

    let request = ScheduleFeedIngestRequest {
        delivered_at,
        ingested_at: Utc::now(),
        files,
        source_file: delivered.name.clone(),
        source_bytes: delivered.bytes,
        source_sha256: delivered.sha256.clone(),
    };

    match sink.record_feed_ingest(&request).await {
        Ok(()) => {
            tracing::info!(
                delivered_at = %delivered_at,
                dir = %dir_name,
                "schedule feed delivery extracted to storage and posted to api"
            );
            audit::decision(&delivered, delivered_at, audit::Outcome::Accepted, None);
            record_ingested(&config.storage_dir, &request);
            *last_ingested_mtime = Some(zip_mtime);
            metrics::gauge!(common::metrics::metric_name(
                "schedule_feed_last_ingest_delivered_at_seconds"
            ))
            .set(delivered_at.timestamp() as f64);
        }
        Err(err) => {
            // The files stay in `storage_dir/<timestamp>/` -- this is a
            // locally-verified-complete delivery; a failed POST is a
            // record-keeping problem to retry, not a reason to remove the
            // extracted files. See `pending_post` handling at the top of
            // this function.
            queue_or_quarantine_failed_post(err, request, pending_post, rejected_mtime);
        }
    }

    if let Err(err) = prune_old_deliveries(&config.storage_dir, config.retention_keep_deliveries) {
        tracing::error!(error = ?err, "retention pruning failed");
    }

    Ok(())
}

/// Notes in the delivery's directory that api accepted it
/// (`delivery::INGESTED_RECORD`), so a restarted process recognises the zip
/// at once. Best effort: without it a restart only costs one redundant,
/// deduplicated POST.
fn record_ingested(storage_dir: &std::path::Path, request: &ScheduleFeedIngestRequest) {
    let zip_mtime = SystemTime::from(request.delivered_at);
    let dir_name = delivery::delivery_dir_name(zip_mtime);
    let record = delivery::IngestedRecord {
        zip_name: request.source_file.clone(),
        zip_bytes: request.source_bytes,
        zip_mtime,
        zip_sha256: request.source_sha256.clone(),
    };
    if let Err(err) = delivery::write_ingested_record(storage_dir, &dir_name, &record) {
        tracing::warn!(error = ?err, dir = %dir_name, "failed to record the accepted delivery; a restart will post it once more");
    }
}

/// Counts zip deliveries quarantined by the PL-5 extraction caps; the
/// chart's `DistantSignalScheduleFeedZipRejected` alert reads it.
const ZIP_REJECTED_METRIC: &str = "schedule_feed_zip_rejected_total";

/// Counts delivery records api refused (400/413/422); the chart's
/// `DistantSignalScheduleFeedIngestRejected` alert reads it.
const INGEST_REJECTED_METRIC: &str = "schedule_feed_ingest_rejected_total";

/// What to do with a delivery record whose POST failed (N-2, the R-083
/// classification schedule-reference already uses): a transient failure
/// (api down, a timeout, 5xx, 401/403/404/408/429) is queued and retried
/// every cycle; a data rejection (400/413/422) would be refused the same
/// way every time, so the delivery is quarantined like a rejected zip
/// until a new upload replaces it, and counted for the alert. Under
/// `INGEST_SINK=db` the same split comes from the route's checks and the
/// SQLSTATE ([`SinkError`]).
fn queue_or_quarantine_failed_post(
    err: SinkError,
    request: ScheduleFeedIngestRequest,
    pending_post: &mut Option<ScheduleFeedIngestRequest>,
    rejected_mtime: &mut Option<SystemTime>,
) {
    match err {
        SinkError::Rejected(err) => {
            tracing::error!(
                error = ?err,
                delivered_at = %request.delivered_at,
                "api rejected this delivery's ingest record (400/413/422); NOT retrying it, \
                 waiting for a new upload"
            );
            audit::decision(
                &request.delivered_file(),
                request.delivered_at,
                audit::Outcome::RejectedByApi,
                Some(&format!("{err:#}")),
            );
            metrics::counter!(common::metrics::metric_name(INGEST_REJECTED_METRIC)).increment(1);
            *rejected_mtime = Some(SystemTime::from(request.delivered_at));
            *pending_post = None;
        }
        SinkError::Transient(err) => {
            tracing::error!(
                error = ?err,
                delivered_at = %request.delivered_at,
                "files extracted to storage but POST to api failed; will retry next cycle"
            );
            *pending_post = Some(request);
        }
    }
}

/// Mirrors `crates/api/src/routes/ingest.rs`'s private
/// `ScheduleFeedIngestRequest`/`ScheduleFeedFile` structs field-for-field
/// (names, types, and JSON casing -- neither struct carries a
/// `#[serde(rename_all = ...)]`, so plain `snake_case` field names already
/// match on the wire). Those types are private to the `api` crate, so this
/// crate can't import them -- it only needs to produce matching JSON, not
/// share a Rust type. If either crate's shape drifts, this comment is the
/// first thing to check.
///
/// `delivered_at` is the delivery's *own* mtime (converted to UTC) -- the
/// real identity of "which delivery is this", used as the primary key on
/// the `api` side. `ingested_at` is when this process actually processed
/// it, kept only as separate observability data (see the migration/query
/// changes for why `delivered_at`, not `ingested_at`, now backs freshness).
///
/// `source_*` are the delivered zip's own name, size and SHA-256 (see
/// `audit.rs`), stored with the record (migration
/// `20261001150000_schedule_feed_delivery_sha256.sql`).
#[derive(Debug, Clone, Serialize)]
struct ScheduleFeedIngestRequest {
    delivered_at: DateTime<Utc>,
    ingested_at: DateTime<Utc>,
    files: Vec<ScheduleFeedFile>,
    source_file: String,
    source_bytes: u64,
    source_sha256: String,
}

impl ScheduleFeedIngestRequest {
    /// The delivered zip, for its audit line.
    fn delivered_file(&self) -> audit::DeliveredFile {
        audit::DeliveredFile {
            name: self.source_file.clone(),
            bytes: self.source_bytes,
            sha256: self.source_sha256.clone(),
        }
    }
}

/// One extracted file; `sha256` is absent for a delivery re-posted from a
/// completion marker written before the hashes existed.
#[derive(Debug, Clone, Serialize)]
struct ScheduleFeedFile {
    name: String,
    bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

/// Parses `check_times` (comma-separated `HH:MM`) into an ordered (as
/// configured, NOT sorted) list of [`NaiveTime`]s. Errors on an empty list
/// or a malformed entry.
fn parse_check_times(raw: &str) -> anyhow::Result<Vec<NaiveTime>> {
    let times: Vec<NaiveTime> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            NaiveTime::parse_from_str(s, "%H:%M")
                .map_err(|error| anyhow::anyhow!("invalid check time {s:?}: {error}"))
        })
        .collect::<Result<_, _>>()?;

    if times.is_empty() {
        anyhow::bail!("check_times must list at least one HH:MM time");
    }

    Ok(times)
}

/// Whether `now` (a Europe/London wall-clock time-of-day) falls in the gap
/// between today's final configured `check_times` fallback
/// (`final_check_time`, `16:00` by default) and the next overnight
/// production window reopening (`window_start_time`, the *first* configured
/// entry, `22:00` by default) -- i.e. whether RSPS5046's production window
/// has fully closed for today with no new window yet open.
///
/// This is the one remaining behavioral role `check_times` plays now that
/// scanning itself runs on a fixed `poll_interval_secs` cadence rather than
/// sleeping until specific slots (see `main`'s loop): gating whether "we're
/// past today's realistic delivery window and still haven't seen a new
/// stable zip" logs at `error` (loud -- the window has closed, this is
/// likely a real problem) or `info`/`debug` (quiet -- still within an
/// expected window, try again next poll) severity. Deliberately **not**
/// applied to the "already ingested today" steady state (see
/// `run_scan_cycle`'s `DeliveryRelation::AlreadyIngested` arm) -- that's the
/// healthy outcome for most of the day after a successful ingest, not a
/// problem `is_final_check_of_day` should ever escalate.
///
/// **Deliberately bounded, not `now >= final_check_time` unbounded to
/// midnight** -- an earlier version of this function compared only against
/// `final_check_time`, which stayed `true` from `16:00` all the way through
/// `23:59:59`, including the `22:00`-`23:59` stretch when a brand new
/// delivery is normally still uploading/stabilizing. That version would have
/// logged `error` every poll cycle during completely normal, expected
/// in-progress delivery -- a real regression from the old design's single
/// once-a-day check right at the `16:00` slot, not just a style change.
/// Bounding the window to `[final_check_time, window_start_time)` restores
/// that "only loud once the fallback has passed AND no new window has
/// opened" intent. Handles the case where the gap wraps past midnight (not
/// true for the current default, where `16:00 < 22:00` same-day, but kept
/// correct for any operator-configured `check_times` shape).
fn is_final_check_of_day(
    now: NaiveTime,
    final_check_time: NaiveTime,
    window_start_time: NaiveTime,
) -> bool {
    if final_check_time <= window_start_time {
        now >= final_check_time && now < window_start_time
    } else {
        now >= final_check_time || now < window_start_time
    }
}

/// Keeps only the `keep` most-recent (by directory-name string, which sorts
/// lexicographically == chronologically -- see `delivery::delivery_dir_name`)
/// immediate subdirectories of `storage_dir` whose name matches
/// [`delivery::is_delivery_dir_name`]; removes the rest via
/// `std::fs::remove_dir_all`. Non-matching subdirectory names (and any
/// plain files directly in `storage_dir`) are left untouched -- they aren't
/// this function's concern, and it never guesses about them.
///
/// Renamed from the old `prune_old_sequences` -- there is no sequence
/// number any more, just delivery timestamps.
fn prune_old_deliveries(storage_dir: &std::path::Path, keep: u32) -> anyhow::Result<()> {
    let mut dirs: Vec<(String, PathBuf)> = Vec::new();

    // Same "not-yet-existing is empty, not an error" reasoning as
    // `scan::scan_incoming` -- storage_dir defaults to the raw volume mount
    // point, which normally exists once mounted, but nothing guarantees
    // that in every deployment shape, and there is genuinely nothing to
    // prune if it doesn't.
    let read_dir = match std::fs::read_dir(storage_dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err.into()),
    };

    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if delivery::is_delivery_dir_name(&name) {
            dirs.push((name, entry.path()));
        }
    }

    dirs.sort_by(|a, b| a.0.cmp(&b.0));

    let keep = keep as usize;
    if dirs.len() > keep {
        let remove_count = dirs.len() - keep;
        for (name, path) in &dirs[..remove_count] {
            tracing::info!(dir = name, path = ?path, "pruning old schedule feed delivery directory");
            std::fs::remove_dir_all(path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
#[expect(
    clippy::format_collect,
    reason = "test code: test string building is not hot"
)]
mod tests {
    use super::*;

    /// Regression for the "L1 -- `MissedTickBehavior::Burst` still default"
    /// finding: this scan loop's own interval must opt into `Delay`, not
    /// leave `Burst` as the default, so an overrun cycle doesn't fire a
    /// burst of back-to-back catch-up scans.
    #[tokio::test]
    async fn scan_interval_defaults_to_delay_not_burst_on_a_missed_tick() {
        let interval = scan_interval(60);
        assert_eq!(
            interval.missed_tick_behavior(),
            tokio::time::MissedTickBehavior::Delay
        );
    }

    #[test]
    fn parse_check_times_parses_and_preserves_configured_order() {
        let times = parse_check_times("22:00, 23:30 ,16:00").unwrap();
        assert_eq!(
            times,
            vec![
                NaiveTime::from_hms_opt(22, 0, 0).unwrap(),
                NaiveTime::from_hms_opt(23, 30, 0).unwrap(),
                NaiveTime::from_hms_opt(16, 0, 0).unwrap(),
            ]
        );
    }

    #[test]
    fn parse_check_times_rejects_empty_and_malformed_entries() {
        assert!(parse_check_times("").is_err());
        assert!(parse_check_times("not-a-time").is_err());
    }

    #[test]
    fn is_final_check_of_day_false_before_the_final_time() {
        let final_check_time = NaiveTime::from_hms_opt(16, 0, 0).unwrap();
        let window_start_time = NaiveTime::from_hms_opt(22, 0, 0).unwrap();
        assert!(!is_final_check_of_day(
            NaiveTime::from_hms_opt(15, 59, 59).unwrap(),
            final_check_time,
            window_start_time
        ));
        assert!(!is_final_check_of_day(
            NaiveTime::from_hms_opt(0, 30, 0).unwrap(),
            final_check_time,
            window_start_time
        ));
    }

    #[test]
    fn is_final_check_of_day_true_only_in_the_gap_after_the_final_time_and_before_the_next_window()
    {
        let final_check_time = NaiveTime::from_hms_opt(16, 0, 0).unwrap();
        let window_start_time = NaiveTime::from_hms_opt(22, 0, 0).unwrap();
        assert!(is_final_check_of_day(
            final_check_time,
            final_check_time,
            window_start_time
        ));
        assert!(is_final_check_of_day(
            NaiveTime::from_hms_opt(21, 59, 59).unwrap(),
            final_check_time,
            window_start_time
        ));
    }

    #[test]
    fn is_final_check_of_day_false_once_the_next_window_has_reopened() {
        let final_check_time = NaiveTime::from_hms_opt(16, 0, 0).unwrap();
        let window_start_time = NaiveTime::from_hms_opt(22, 0, 0).unwrap();
        assert!(!is_final_check_of_day(
            window_start_time,
            final_check_time,
            window_start_time
        ));
        assert!(!is_final_check_of_day(
            NaiveTime::from_hms_opt(23, 59, 59).unwrap(),
            final_check_time,
            window_start_time
        ));
    }

    #[test]
    fn is_final_check_of_day_handles_a_gap_that_wraps_past_midnight() {
        let final_check_time = NaiveTime::from_hms_opt(23, 0, 0).unwrap();
        let window_start_time = NaiveTime::from_hms_opt(6, 0, 0).unwrap();
        assert!(is_final_check_of_day(
            NaiveTime::from_hms_opt(23, 30, 0).unwrap(),
            final_check_time,
            window_start_time
        ));
        assert!(is_final_check_of_day(
            NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
            final_check_time,
            window_start_time
        ));
        assert!(!is_final_check_of_day(
            NaiveTime::from_hms_opt(12, 0, 0).unwrap(),
            final_check_time,
            window_start_time
        ));
    }

    #[test]
    fn prune_keeps_only_the_n_most_recent_delivery_dirs() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "20260901T090000Z",
            "20260902T090000Z",
            "20260903T090000Z",
            "not-a-delivery-dir",
            "942",
        ] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        std::fs::write(dir.path().join("stray.txt"), b"x").unwrap();

        prune_old_deliveries(dir.path(), 2).unwrap();

        let mut remaining: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();

        assert_eq!(
            remaining,
            vec![
                "20260902T090000Z".to_string(),
                "20260903T090000Z".to_string(),
                "942".to_string(),
                "not-a-delivery-dir".to_string(),
                "stray.txt".to_string(),
            ]
        );
    }

    #[test]
    fn prune_on_nonexistent_storage_dir_is_a_noop_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist-yet");
        prune_old_deliveries(&missing, 2).unwrap();
    }

    #[test]
    fn prune_is_a_noop_when_at_or_under_the_keep_count() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["20260901T090000Z", "20260902T090000Z"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }

        prune_old_deliveries(dir.path(), 2).unwrap();

        let mut remaining: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();

        assert_eq!(
            remaining,
            vec![
                "20260901T090000Z".to_string(),
                "20260902T090000Z".to_string()
            ]
        );
    }

    /// End-to-end-ish integration test through `run_scan_cycle` itself,
    /// using a real zip fixture built via `delivery::build_test_zip` --
    /// covers zip detection, stability, mtime-dedup, and extraction all
    /// wired together the way `main`'s loop actually calls them (an HTTP
    /// POST is not exercised here -- `config.api_ingest_url` points at an
    /// address nothing listens on, so the POST fails and the delivery is
    /// left in `pending_post`, which is exactly what this test asserts).
    #[tokio::test]
    async fn a_stable_new_zip_is_extracted_into_a_timestamp_named_directory() {
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();

        let bytes = real_zip();
        std::fs::write(watch_dir.path().join("timetable_full.zip"), &bytes).unwrap();

        let config = test_config(watch_dir.path(), storage_dir.path());
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let internal_oauth = test_oauth();

        let sink = test_sink(&config, client, internal_oauth);

        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();
        let mut known_stray_files = HashSet::new();
        let mut last_ingested_mtime = None;
        let mut pending_post = None;
        let mut rejected_mtime = None;

        // stability_cycles = 2 by default in test_config -- two identical
        // cycles reach stability.
        for _ in 0..2 {
            run_scan_cycle(
                &sink,
                &config,
                &Routing::defaults(),
                &mut tracker,
                &mut known_stable,
                &mut known_stray_files,
                &mut last_ingested_mtime,
                &mut pending_post,
                &mut rejected_mtime,
                false,
            )
            .await
            .unwrap();
        }

        // The POST attempt fails (nothing is listening), so the delivery
        // is tracked as pending rather than advancing last_ingested_mtime
        // -- but the files must already be extracted to storage_dir at
        // this point (extraction happens before the POST attempt).
        assert!(pending_post.is_some());

        let entries: Vec<String> = std::fs::read_dir(storage_dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 1);
        assert!(delivery::is_delivery_dir_name(&entries[0]));

        let delivery_dir = storage_dir.path().join(&entries[0]);
        assert_eq!(
            std::fs::read_to_string(delivery_dir.join("RJTTF975MCA.txt")).unwrap(),
            cif_check::tests::MCA
        );
        assert_eq!(
            std::fs::read_to_string(delivery_dir.join("RJTTF975MSN.txt")).unwrap(),
            cif_check::tests::delivery_files(Some(&today()))[1].1
        );
    }

    /// The CIF guard end to end: CORPUS-named files that land after the CIF
    /// zip (a zip of the extract, the JSON extract, the SMART `.csv.gz`)
    /// are never extracted as the timetable; the CIF zip still is.
    #[tokio::test]
    async fn newer_corpus_named_files_never_displace_the_cif_zip() {
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let cif = real_zip();
        std::fs::write(watch_dir.path().join("timetable_full.zip"), &cif).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        let corpus_zip = delivery::build_test_zip(&[("CORPUSExtract.json", b"{}")]);
        std::fs::write(watch_dir.path().join("CORPUSExtract.zip"), &corpus_zip).unwrap();
        std::fs::write(watch_dir.path().join("CORPUSExtract.json.gz"), b"gz").unwrap();
        std::fs::write(watch_dir.path().join("CORPUSExtract.csv.gz"), b"gz").unwrap();

        let config = test_config(watch_dir.path(), storage_dir.path());
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let internal_oauth = test_oauth();
        let sink = test_sink(&config, client, internal_oauth);
        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();
        let mut known_stray_files = HashSet::new();
        let mut last_ingested_mtime = None;
        let mut pending_post = None;
        let mut rejected_mtime = None;
        for _ in 0..2 {
            run_scan_cycle(
                &sink,
                &config,
                &Routing::defaults(),
                &mut tracker,
                &mut known_stable,
                &mut known_stray_files,
                &mut last_ingested_mtime,
                &mut pending_post,
                &mut rejected_mtime,
                false,
            )
            .await
            .unwrap();
        }

        let dirs: Vec<_> = std::fs::read_dir(storage_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(dirs.len(), 1);
        assert_eq!(
            std::fs::read_to_string(dirs[0].join("RJTTF975MCA.txt")).unwrap(),
            cif_check::tests::MCA
        );
        assert!(!dirs[0].join("CORPUSExtract.json").exists());
        let mut stray: Vec<&str> = known_stray_files.iter().map(String::as_str).collect();
        stray.sort_unstable();
        assert_eq!(
            stray,
            [
                "CORPUSExtract.csv.gz",
                "CORPUSExtract.json.gz",
                "CORPUSExtract.zip"
            ]
        );
    }

    /// PL-5: a zip over the extraction caps is quarantined: nothing is
    /// written to `storage_dir`, no POST is queued, and later cycles do not
    /// try it again until a new upload (a new mtime) replaces it.
    #[tokio::test]
    async fn an_oversized_zip_is_quarantined_not_retried_every_cycle() {
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let bytes = real_zip();
        let zip_path = watch_dir.path().join("timetable_full.zip");
        std::fs::write(&zip_path, &bytes).unwrap();

        let mut config = test_config(watch_dir.path(), storage_dir.path());
        config.max_extracted_bytes = 12;
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let internal_oauth = test_oauth();
        let sink = test_sink(&config, client, internal_oauth);
        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();
        let mut known_stray_files = HashSet::new();
        let mut last_ingested_mtime = None;
        let mut pending_post = None;
        let mut rejected_mtime = None;

        for _ in 0..4 {
            run_scan_cycle(
                &sink,
                &config,
                &Routing::defaults(),
                &mut tracker,
                &mut known_stable,
                &mut known_stray_files,
                &mut last_ingested_mtime,
                &mut pending_post,
                &mut rejected_mtime,
                false,
            )
            .await
            .unwrap();
        }
        let zip_mtime = std::fs::metadata(&zip_path).unwrap().modified().unwrap();
        assert_eq!(rejected_mtime, Some(zip_mtime));
        assert!(pending_post.is_none());
        assert_eq!(
            std::fs::read_dir(storage_dir.path()).unwrap().count(),
            0,
            "nothing extracted, and no scratch directory left behind"
        );
    }

    /// PL-13: while the api POST keeps failing, later cycles neither
    /// extract the zip again nor send a second copy of the request -- the
    /// queued retry is the only POST.
    #[tokio::test]
    async fn a_failing_post_does_not_re_extract_the_delivery_every_cycle() {
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let bytes = real_zip();
        std::fs::write(watch_dir.path().join("timetable_full.zip"), &bytes).unwrap();

        let config = test_config(watch_dir.path(), storage_dir.path());
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let internal_oauth = test_oauth();
        let sink = test_sink(&config, client, internal_oauth);
        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();
        let mut known_stray_files = HashSet::new();
        let mut last_ingested_mtime = None;
        let mut pending_post = None;
        let mut rejected_mtime = None;

        let mut marker_stamp = None;
        for cycle in 0..6 {
            run_scan_cycle(
                &sink,
                &config,
                &Routing::defaults(),
                &mut tracker,
                &mut known_stable,
                &mut known_stray_files,
                &mut last_ingested_mtime,
                &mut pending_post,
                &mut rejected_mtime,
                false,
            )
            .await
            .unwrap();
            let dirs: Vec<_> = std::fs::read_dir(storage_dir.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            if cycle == 1 {
                // First extraction (stability_cycles = 2): mark the file so a
                // re-extraction would be visible.
                assert_eq!(dirs.len(), 1);
                std::fs::write(dirs[0].join("RJTTF975MCA.txt"), b"sentinel").unwrap();
                marker_stamp = Some(
                    std::fs::metadata(dirs[0].join(common::schedule_delivery::COMPLETE_MARKER))
                        .unwrap()
                        .modified()
                        .unwrap(),
                );
            }
        }

        assert!(pending_post.is_some(), "the POST is still queued for retry");
        assert_eq!(last_ingested_mtime, None);
        let dirs: Vec<_> = std::fs::read_dir(storage_dir.path())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(dirs.len(), 1, "no scratch directories left behind");
        assert_eq!(
            std::fs::read_to_string(dirs[0].join("RJTTF975MCA.txt")).unwrap(),
            "sentinel",
            "the delivery was not extracted again"
        );
        assert_eq!(
            std::fs::metadata(dirs[0].join(common::schedule_delivery::COMPLETE_MARKER))
                .unwrap()
                .modified()
                .unwrap(),
            marker_stamp.unwrap()
        );
    }

    fn test_config(watch_dir: &std::path::Path, storage_dir: &std::path::Path) -> Config {
        Config {
            watch_dir: watch_dir.to_path_buf(),
            storage_dir: storage_dir.to_path_buf(),
            cif_file_pattern: config::DEFAULT_CIF_FILE_PATTERN.to_string(),
            cif_exclude_pattern: config::DEFAULT_CIF_EXCLUDE_PATTERN.to_string(),
            corpus: config::CorpusArgs {
                corpus_ingest_enabled: false,
                corpus_file_pattern: config::DEFAULT_CORPUS_FILE_PATTERN.to_string(),
                corpus_api_url: "http://127.0.0.1:1/corpus-locations".to_string(),
                corpus_max_decompressed_bytes: 256 * 1024 * 1024,
                corpus_min_rows: 1,
                corpus_retention_keep: 3,
            },
            check_times: "22:00,16:00".to_string(),
            poll_interval_secs: 120,
            retention_keep_deliveries: 2,
            max_extracted_bytes: 4 * 1024 * 1024 * 1024,
            max_zip_entries: 64,
            // The real-shaped fixture has 2 schedules: everything but the
            // minimum is at its default.
            cif_checks: config::CifCheckArgs {
                cif_max_generated_age_days: 3,
                cif_min_schedules: 1,
                cif_max_record_drop_percent: 20,
            },
            stability_cycles: 2,
            // Deliberately an address nothing listens on -- these tests
            // only exercise up to the POST attempt, not a real server.
            api_ingest_url: "http://127.0.0.1:1/schedule-feed-ingests".to_string(),
            ingest_sink: sink::SinkKind::Http,
            database_url: None,
            internal_oauth: common::oauth_client::InternalOAuthArgs {
                internal_oauth_token_url: "http://127.0.0.1:1/token".to_string(),
                internal_oauth_client_id: "test-client".to_string(),
                internal_oauth_scope: "groups".to_string(),
                internal_oauth_username: "test-user".to_string(),
                internal_oauth_password: "test-password".to_string(),
            },
            metrics_port: 0,
            metrics: common::service_args::MetricsArgs {
                metrics_enabled: false,
            },
            health: common::service_args::HealthArgs {
                health_bind_url: "127.0.0.1:0".to_string(),
                progress_stall_secs: 1800,
            },
        }
    }

    /// N-2: a 400 from api is a data rejection: the delivery is
    /// quarantined (not queued), and later cycles neither POST it again
    /// nor re-extract it. A 503 stays queued for retry.
    #[tokio::test]
    async fn a_rejected_ingest_post_is_not_retried_every_cycle() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        for (status, rejected) in [(400u16, true), (503, false)] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::json!({"access_token": "t", "expires_in": 3600}),
                    ),
                )
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/schedule-feed-ingests"))
                .respond_with(ResponseTemplate::new(status).set_body_string("bad"))
                .mount(&server)
                .await;

            let watch_dir = tempfile::tempdir().unwrap();
            let storage_dir = tempfile::tempdir().unwrap();
            let bytes = real_zip();
            std::fs::write(watch_dir.path().join("timetable_full.zip"), &bytes).unwrap();

            let mut config = test_config(watch_dir.path(), storage_dir.path());
            config.api_ingest_url = format!("{}/schedule-feed-ingests", server.uri());
            let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
            let internal_oauth = common::oauth_client::OAuthTokenCache::new(
                common::oauth_client::OAuthCredentials {
                    token_url: format!("{}/token", server.uri()),
                    client_id: "test-client".to_string(),
                    scope: "groups".to_string(),
                    username: "test-user".to_string(),
                    password: "test-password".to_string(),
                },
            );
            let sink = test_sink(&config, client, internal_oauth);
            let mut tracker = StabilityTracker::new();
            let mut known_stable = HashSet::new();
            let mut known_stray_files = HashSet::new();
            let mut last_ingested_mtime = None;
            let mut pending_post = None;
            let mut rejected_mtime = None;

            for _ in 0..5 {
                run_scan_cycle(
                    &sink,
                    &config,
                    &Routing::defaults(),
                    &mut tracker,
                    &mut known_stable,
                    &mut known_stray_files,
                    &mut last_ingested_mtime,
                    &mut pending_post,
                    &mut rejected_mtime,
                    false,
                )
                .await
                .unwrap();
            }

            let posts = server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.url.path() == "/schedule-feed-ingests")
                .count();
            assert_eq!(last_ingested_mtime, None, "status {status}");
            if rejected {
                assert_eq!(posts, 1, "a 400 is sent once, never retried");
                assert!(pending_post.is_none());
                assert!(rejected_mtime.is_some(), "the delivery is quarantined");
            } else {
                // Stable on cycle 2, then one queued retry per cycle.
                assert_eq!(posts, 4, "a 503 is retried every cycle");
                assert!(pending_post.is_some());
                assert!(rejected_mtime.is_none());
            }
        }
    }

    /// Provenance end to end: the record posted to api carries the zip's
    /// name, size and SHA-256 and each extracted file's SHA-256, and the
    /// delivery gets exactly one `accepted` audit line with the same hash.
    /// A 400 instead gives one `rejected_by_api` line.
    #[tokio::test]
    async fn an_accepted_delivery_records_and_logs_its_sha256() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        for (status, outcome) in [(200u16, "accepted"), (400, "rejected_by_api")] {
            let server = MockServer::start().await;
            Mock::given(method("POST"))
                .and(path("/token"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(
                        serde_json::json!({"access_token": "t", "expires_in": 3600}),
                    ),
                )
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .and(path("/schedule-feed-ingests"))
                .respond_with(
                    ResponseTemplate::new(status).set_body_json(serde_json::json!({"upserted": 1})),
                )
                .mount(&server)
                .await;

            let watch_dir = tempfile::tempdir().unwrap();
            let storage_dir = tempfile::tempdir().unwrap();
            let bytes = real_zip();
            std::fs::write(watch_dir.path().join("timetable_full.zip"), &bytes).unwrap();
            let zip_sha = audit::DeliveredFile::from_bytes("", &bytes).sha256;

            let mut config = test_config(watch_dir.path(), storage_dir.path());
            config.api_ingest_url = format!("{}/schedule-feed-ingests", server.uri());
            let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
            let internal_oauth = common::oauth_client::OAuthTokenCache::new(
                common::oauth_client::OAuthCredentials {
                    token_url: format!("{}/token", server.uri()),
                    client_id: "test-client".to_string(),
                    scope: "groups".to_string(),
                    username: "test-user".to_string(),
                    password: "test-password".to_string(),
                },
            );
            let sink = test_sink(&config, client, internal_oauth);
            let mut tracker = StabilityTracker::new();
            let mut known_stable = HashSet::new();
            let mut known_stray_files = HashSet::new();
            let mut last_ingested_mtime = None;
            let mut pending_post = None;
            let mut rejected_mtime = None;

            let (guard, logs) = audit::tests::capture_default();
            for _ in 0..4 {
                run_scan_cycle(
                    &sink,
                    &config,
                    &Routing::defaults(),
                    &mut tracker,
                    &mut known_stable,
                    &mut known_stray_files,
                    &mut last_ingested_mtime,
                    &mut pending_post,
                    &mut rejected_mtime,
                    false,
                )
                .await
                .unwrap();
            }
            drop(guard);

            let audit = logs.audit_lines();
            assert_eq!(audit.len(), 1, "one decision per delivery: {audit:?}");
            assert_eq!(audit[0]["outcome"], outcome);
            assert_eq!(audit[0]["file"], "timetable_full.zip");
            assert_eq!(audit[0]["bytes"], bytes.len() as u64);
            assert_eq!(audit[0]["sha256"], zip_sha.as_str());
            assert_eq!(audit[0].get("reason").is_some(), status != 200);

            let posts: Vec<serde_json::Value> = server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.url.path() == "/schedule-feed-ingests")
                .map(|r| serde_json::from_slice(&r.body).unwrap())
                .collect();
            assert_eq!(posts.len(), 1);
            assert_eq!(posts[0]["source_file"], "timetable_full.zip");
            assert_eq!(posts[0]["source_bytes"], bytes.len() as u64);
            assert_eq!(posts[0]["source_sha256"], zip_sha.as_str());
            assert_eq!(
                posts[0]["files"][0]["sha256"],
                audit::DeliveredFile::from_bytes("", cif_check::tests::MCA.as_bytes())
                    .sha256
                    .as_str()
            );
        }
    }

    /// A mock api accepting every token and ingest request.
    async fn accepting_api() -> wiremock::MockServer {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "t", "expires_in": 3600})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/schedule-feed-ingests"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"upserted": 1})),
            )
            .mount(&server)
            .await;
        server
    }

    async fn ingest_posts(server: &wiremock::MockServer) -> usize {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path() == "/schedule-feed-ingests")
            .count()
    }

    /// One process's in-memory scan state, pointed at `server`. A new
    /// `Process` over the same directories is a pod restart.
    struct Process {
        config: Config,
        sink: sink::HttpSink,
        tracker: StabilityTracker,
        known_stable: HashSet<String>,
        known_stray_files: HashSet<String>,
        last_ingested_mtime: Option<SystemTime>,
        pending_post: Option<ScheduleFeedIngestRequest>,
        rejected_mtime: Option<SystemTime>,
    }

    impl Process {
        fn start(
            watch_dir: &std::path::Path,
            storage_dir: &std::path::Path,
            server: &wiremock::MockServer,
        ) -> Self {
            let mut config = test_config(watch_dir, storage_dir);
            config.api_ingest_url = format!("{}/schedule-feed-ingests", server.uri());
            let sink = test_sink(
                &config,
                Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap(),
                common::oauth_client::OAuthTokenCache::new(
                    common::oauth_client::OAuthCredentials {
                        token_url: format!("{}/token", server.uri()),
                        client_id: "test-client".to_string(),
                        scope: "groups".to_string(),
                        username: "test-user".to_string(),
                        password: "test-password".to_string(),
                    },
                ),
            );
            Self {
                config,
                sink,
                tracker: StabilityTracker::new(),
                known_stable: HashSet::new(),
                known_stray_files: HashSet::new(),
                last_ingested_mtime: None,
                pending_post: None,
                rejected_mtime: None,
            }
        }

        /// One cycle past the day's final check time (where a zip that is
        /// not yet stable is logged at ERROR); returns every log line.
        async fn final_check_cycle(&mut self) -> Vec<serde_json::Value> {
            let (guard, logs) = audit::tests::capture_default();
            run_scan_cycle(
                &self.sink,
                &self.config,
                &Routing::defaults(),
                &mut self.tracker,
                &mut self.known_stable,
                &mut self.known_stray_files,
                &mut self.last_ingested_mtime,
                &mut self.pending_post,
                &mut self.rejected_mtime,
                true,
            )
            .await
            .unwrap();
            drop(guard);
            logs.lines()
        }
    }

    fn errors(lines: &[serde_json::Value]) -> Vec<&serde_json::Value> {
        lines
            .iter()
            .filter(|line| line["level"] == "ERROR")
            .collect()
    }

    /// The 2026-10-01 incident: after a pod restart (past the final check
    /// time), the zip already extracted and accepted the day before is
    /// recognised on the first cycle -- no stability wait, no "stalled
    /// upload" ERROR, and no second POST -- and only once.
    #[tokio::test]
    async fn an_already_ingested_zip_is_recognised_at_once_after_a_restart() {
        let server = accepting_api().await;
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let zip_path = watch_dir.path().join("timetable_full.zip");
        std::fs::write(&zip_path, real_zip()).unwrap();
        let zip_mtime = std::fs::metadata(&zip_path).unwrap().modified().unwrap();

        let mut before = Process::start(watch_dir.path(), storage_dir.path(), &server);
        for _ in 0..2 {
            before.final_check_cycle().await;
        }
        assert_eq!(before.last_ingested_mtime, Some(zip_mtime));
        assert_eq!(ingest_posts(&server).await, 1);
        let dir = storage_dir
            .path()
            .join(delivery::delivery_dir_name(zip_mtime));
        assert!(dir.join(delivery::INGESTED_RECORD).is_file());

        let mut after = Process::start(watch_dir.path(), storage_dir.path(), &server);
        for _ in 0..3 {
            let lines = after.final_check_cycle().await;
            assert_eq!(errors(&lines), Vec::<&serde_json::Value>::new());
            assert!(
                lines
                    .iter()
                    .all(|line| line["message"] != "zip file present but not yet stable"),
                "{lines:?}"
            );
            assert_eq!(after.last_ingested_mtime, Some(zip_mtime));
        }
        assert_eq!(ingest_posts(&server).await, 1, "not posted again");
    }

    /// A delivery extracted before a restart whose POST had not yet
    /// succeeded (or whose directory predates the ingested record, as in
    /// production on 2026-10-01): the zip needs no stability wait, but is
    /// posted -- once -- and then recorded.
    #[tokio::test]
    async fn an_extracted_but_unposted_zip_is_posted_without_a_wait_after_a_restart() {
        let server = accepting_api().await;
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let zip_path = watch_dir.path().join("timetable_full.zip");
        std::fs::write(&zip_path, real_zip()).unwrap();
        let zip_mtime = std::fs::metadata(&zip_path).unwrap().modified().unwrap();

        let mut before = Process::start(watch_dir.path(), storage_dir.path(), &server);
        for _ in 0..2 {
            before.final_check_cycle().await;
        }
        let dir = storage_dir
            .path()
            .join(delivery::delivery_dir_name(zip_mtime));
        std::fs::remove_file(dir.join(delivery::INGESTED_RECORD)).unwrap();

        let mut after = Process::start(watch_dir.path(), storage_dir.path(), &server);
        let lines = after.final_check_cycle().await;
        assert_eq!(errors(&lines), Vec::<&serde_json::Value>::new());
        assert_eq!(after.last_ingested_mtime, Some(zip_mtime));
        assert_eq!(ingest_posts(&server).await, 2);
        assert!(dir.join(delivery::INGESTED_RECORD).is_file());
        after.final_check_cycle().await;
        assert_eq!(ingest_posts(&server).await, 2);
    }

    /// The stalled-upload detection still holds after a restart for a
    /// genuinely new zip (a different mtime from the completed delivery,
    /// and different bytes): it waits `stability_cycles`, logging the
    /// ERROR past the final check time, before it is extracted and posted.
    #[tokio::test]
    async fn a_new_zip_after_a_restart_still_waits_to_be_stable() {
        let server = accepting_api().await;
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let zip_path = watch_dir.path().join("timetable_full.zip");
        std::fs::write(&zip_path, real_zip()).unwrap();
        let old = SystemTime::now() - Duration::from_secs(86_400);
        std::fs::File::options()
            .write(true)
            .open(&zip_path)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let mut before = Process::start(watch_dir.path(), storage_dir.path(), &server);
        for _ in 0..2 {
            before.final_check_cycle().await;
        }
        assert_eq!(ingest_posts(&server).await, 1);

        // Today's upload replaces it in place, then the pod restarts.
        std::fs::write(&zip_path, real_zip()).unwrap();
        let new_mtime = std::fs::metadata(&zip_path).unwrap().modified().unwrap();
        assert_ne!(
            delivery::delivery_dir_name(new_mtime),
            delivery::delivery_dir_name(old)
        );

        let mut after = Process::start(watch_dir.path(), storage_dir.path(), &server);
        let lines = after.final_check_cycle().await;
        let errors = errors(&lines);
        assert_eq!(errors.len(), 1, "{lines:?}");
        assert!(
            errors[0]["message"]
                .as_str()
                .unwrap()
                .contains("still not stable"),
            "{errors:?}"
        );
        assert_eq!(after.last_ingested_mtime, None);
        assert_eq!(ingest_posts(&server).await, 1);

        after.final_check_cycle().await;
        assert_eq!(after.last_ingested_mtime, Some(new_mtime));
        assert_eq!(ingest_posts(&server).await, 2);
    }

    /// Today's date as the RJTTF banner writes it, so a fixture delivery
    /// written now passes the Generated-date check.
    fn today() -> String {
        Utc::now().format("%d/%m/%Y").to_string()
    }

    /// The real-shaped CIF delivery (`tests/fixtures/cif_delivery_excerpt`),
    /// generated today.
    fn real_zip() -> Vec<u8> {
        cif_check::tests::delivery_zip(&today())
    }

    /// Runs `cycles` scan cycles against `watch_dir`/`storage_dir` with no
    /// api listening; returns the quarantined mtime and the audit lines.
    async fn run_cycles(
        watch_dir: &std::path::Path,
        storage_dir: &std::path::Path,
        cycles: usize,
    ) -> (Option<SystemTime>, Vec<serde_json::Value>) {
        let config = test_config(watch_dir, storage_dir);
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let internal_oauth = test_oauth();
        let sink = test_sink(&config, client, internal_oauth);
        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();
        let mut known_stray_files = HashSet::new();
        let mut last_ingested_mtime = None;
        let mut pending_post = None;
        let mut rejected_mtime = None;
        let (guard, logs) = audit::tests::capture_default();
        for _ in 0..cycles {
            run_scan_cycle(
                &sink,
                &config,
                &Routing::defaults(),
                &mut tracker,
                &mut known_stable,
                &mut known_stray_files,
                &mut last_ingested_mtime,
                &mut pending_post,
                &mut rejected_mtime,
                false,
            )
            .await
            .unwrap();
        }
        drop(guard);
        assert!(
            rejected_mtime.is_none() || pending_post.is_none(),
            "a quarantined delivery is never also queued"
        );
        (rejected_mtime, logs.audit_lines())
    }

    /// A zip that is not a complete full CIF extract (here: the MCA cut
    /// short, no ZZ trailer) is quarantined before it is marked complete:
    /// nothing reaches `storage_dir`, one `quarantined` audit line gives the
    /// reason, and later cycles do not retry it.
    #[tokio::test]
    async fn a_cif_that_fails_its_checks_is_quarantined_with_an_audit_line() {
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let truncated: String = cif_check::tests::MCA
            .lines()
            .take(9)
            .map(|line| format!("{line}\r\n"))
            .collect();
        let mut files = cif_check::tests::delivery_files(Some(&today()));
        files[0].1 = truncated;
        let entries: Vec<(&str, &[u8])> = files
            .iter()
            .map(|(name, text)| (*name, text.as_bytes()))
            .collect();
        let bytes = delivery::build_test_zip(&entries);
        let zip_path = watch_dir.path().join("timetable_full.zip");
        std::fs::write(&zip_path, &bytes).unwrap();

        let (rejected_mtime, audit) = run_cycles(watch_dir.path(), storage_dir.path(), 5).await;

        assert_eq!(
            rejected_mtime,
            Some(std::fs::metadata(&zip_path).unwrap().modified().unwrap())
        );
        assert_eq!(std::fs::read_dir(storage_dir.path()).unwrap().count(), 0);
        assert_eq!(audit.len(), 1, "{audit:?}");
        assert_eq!(audit[0]["outcome"], "quarantined");
        assert_eq!(
            audit[0]["sha256"],
            audit::DeliveredFile::from_bytes("", &bytes).sha256.as_str()
        );
        let reason = audit[0]["reason"].as_str().unwrap();
        assert!(reason.contains("ZZ"), "{reason}");
    }

    /// Compared with the last accepted delivery: a delivery whose schedule
    /// count collapsed is quarantined, and the previous one stays the
    /// newest complete delivery for schedule-reference.
    #[tokio::test]
    async fn a_delivery_much_smaller_than_the_last_accepted_one_is_quarantined() {
        let watch_dir = tempfile::tempdir().unwrap();
        let storage_dir = tempfile::tempdir().unwrap();
        let previous = storage_dir.path().join("20200101T000000Z");
        std::fs::create_dir(&previous).unwrap();
        std::fs::write(
            previous.join(common::schedule_delivery::COMPLETE_MARKER),
            "",
        )
        .unwrap();
        cif_check::write_stats(
            &previous,
            &cif_check::CifStats {
                generated: chrono::NaiveDate::from_ymd_opt(2020, 1, 1).unwrap(),
                sequence: Some(974),
                schedules: 505_342,
                tiplocs: 12_096,
            },
        )
        .unwrap();
        std::fs::write(watch_dir.path().join("timetable_full.zip"), real_zip()).unwrap();

        let (rejected_mtime, audit) = run_cycles(watch_dir.path(), storage_dir.path(), 3).await;

        assert!(rejected_mtime.is_some());
        assert_eq!(audit.len(), 1, "{audit:?}");
        assert_eq!(audit[0]["outcome"], "quarantined");
        let reason = audit[0]["reason"].as_str().unwrap();
        assert!(reason.contains("fell from 505342 to 2"), "{reason}");
        let mut dirs: Vec<String> = std::fs::read_dir(storage_dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        dirs.sort();
        assert_eq!(dirs, ["20200101T000000Z"]);
    }

    /// The HTTP sink `main` builds from `config` (`INGEST_SINK=http`).
    fn test_sink(
        config: &Config,
        client: Client,
        tokens: common::oauth_client::OAuthTokenCache,
    ) -> sink::HttpSink {
        sink::HttpSink::new(
            client,
            config.api_ingest_url.clone(),
            config.corpus.corpus_api_url.clone(),
            tokens,
        )
    }

    fn test_oauth() -> common::oauth_client::OAuthTokenCache {
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: "http://127.0.0.1:1/token".to_string(),
            client_id: "test-client".to_string(),
            scope: "groups".to_string(),
            username: "test-user".to_string(),
            password: "test-password".to_string(),
        })
    }
}
