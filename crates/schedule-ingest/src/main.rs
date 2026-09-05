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
//! service never dials out itself -- a sibling SFTPGo container receives the
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
//! and re-extract + re-POST it. Extraction into the same timestamp-derived
//! directory is idempotent, and the `api` insert is `ON CONFLICT (delivered_at)
//! DO NOTHING`, so a restart costs one harmless redundant cycle, not a
//! silently swallowed gap.

mod config;
mod delivery;
mod scan;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, NaiveTime, Utc};
use chrono_tz::Europe::London;
use clap::Parser;
use config::Config;
use delivery::DeliveryRelation;
use reqwest::Client;
use scan::{StabilityTracker, scan_incoming};
use serde::Serialize;

/// Per-request timeout — matches the other pollers' identical rationale
/// (comfortably short relative to `poll_interval_secs`'s default of two
/// minutes between scans).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }

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

    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = config.internal_oauth.token_cache();

    let mut tracker = StabilityTracker::new();
    let mut known_stable: HashSet<String> = HashSet::new();
    let mut known_stray_files: HashSet<String> = HashSet::new();
    let mut last_ingested_mtime: Option<SystemTime> = None;
    let mut pending_post: Option<ScheduleFeedIngestRequest> = None;

    // `tokio::time::interval`'s first `tick()` fires immediately (its
    // default `MissedTickBehavior::Burst`), so every run -- including the
    // very first -- scans right away with no special-cased bypass needed;
    // subsequent ticks are `poll_interval_secs` apart.
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    loop {
        interval.tick().await;

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
            &client,
            &config,
            &internal_oauth,
            &mut tracker,
            &mut known_stable,
            &mut known_stray_files,
            &mut last_ingested_mtime,
            &mut pending_post,
            is_final_check_of_day,
        )
        .await
        {
            tracing::error!(error = ?err, "scan cycle failed unexpectedly; will retry next poll interval");
        }

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
#[allow(clippy::too_many_arguments)]
async fn run_scan_cycle(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    tracker: &mut StabilityTracker,
    known_stable: &mut HashSet<String>,
    known_stray_files: &mut HashSet<String>,
    last_ingested_mtime: &mut Option<SystemTime>,
    pending_post: &mut Option<ScheduleFeedIngestRequest>,
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
        match post_ingest(client, config, internal_oauth, &pending).await {
            Ok(()) => {
                tracing::info!(
                    delivered_at = %pending.delivered_at,
                    "retried and succeeded posting a previously-failed ingest record"
                );
                *last_ingested_mtime = Some(SystemTime::from(pending.delivered_at));
                metrics::gauge!(common::metrics::metric_name(
                    "schedule_feed_last_ingest_delivered_at_seconds"
                ))
                .set(pending.delivered_at.timestamp() as f64);
            }
            Err(err) => {
                tracing::error!(error = ?err, delivered_at = %pending.delivered_at, "retry POST still failing; will retry again next cycle");
                *pending_post = Some(pending);
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

    let mut candidates = delivery::find_zip_candidates(&snapshot);
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
    let stray: HashSet<String> = snapshot
        .0
        .keys()
        .filter(|name| Some(name.as_str()) != winner.as_ref().map(|(name, _)| name.as_str()))
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

    if !known_stable.contains(&zip_filename) {
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

    let dir_name = delivery::delivery_dir_name(zip_mtime);
    let delivery_dir = config.storage_dir.join(&dir_name);
    let zip_path = config.watch_dir.join(&zip_filename);

    let extracted = match delivery::extract_zip(&zip_path, &delivery_dir) {
        Ok(extracted) => extracted,
        Err(err) => {
            tracing::error!(error = ?err, zip = %zip_filename, "failed to extract a stable zip delivery; retrying next cycle");
            return Ok(());
        }
    };

    let files = extracted
        .into_iter()
        .map(|(name, bytes)| ScheduleFeedFile { name, bytes })
        .collect();

    let delivered_at: DateTime<Utc> = DateTime::<Utc>::from(zip_mtime);
    let request = ScheduleFeedIngestRequest {
        delivered_at,
        ingested_at: Utc::now(),
        files,
    };

    match post_ingest(client, config, internal_oauth, &request).await {
        Ok(()) => {
            tracing::info!(
                delivered_at = %delivered_at,
                dir = %dir_name,
                "schedule feed delivery extracted to storage and posted to api"
            );
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
            tracing::error!(error = ?err, delivered_at = %delivered_at, "files extracted to storage but POST to api failed; will retry next cycle");
            *pending_post = Some(request);
        }
    }

    if let Err(err) = prune_old_deliveries(&config.storage_dir, config.retention_keep_deliveries) {
        tracing::error!(error = ?err, "retention pruning failed");
    }

    Ok(())
}

/// POSTs one completed delivery record to `config.api_ingest_url`.
///
/// Deliberately **not** `common::ingest::post_batch`: that helper always
/// serializes `items: &[T]` as a JSON *array*, but the `api` crate's
/// `ScheduleFeedIngestRequest` (see `crates/api/src/routes/ingest.rs`)
/// expects a single JSON *object* — one record per verified delivery, not a
/// per-cycle batch of reference rows like every other poller sends. Wrapping
/// a single record in a one-element slice would change the wire shape
/// rather than match it, so this is a small bespoke POST instead (mirroring
/// `post_batch`'s own request-building/error-handling shape, just without
/// the `&[T]` framing).
async fn post_ingest(
    client: &Client,
    config: &Config,
    tokens: &common::oauth_client::OAuthTokenCache,
    request: &ScheduleFeedIngestRequest,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(&config.api_ingest_url)
        .bearer_auth(&token)
        .json(request)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::info!(
            delivered_at = %request.delivered_at,
            files = request.files.len(),
            "posted schedule feed ingest to api"
        );
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("schedule feed ingest POST failed: {status} {text}");
    }
}

/// Mirrors `crates/api/src/routes/ingest.rs`'s private
/// `ScheduleFeedIngestRequest`/`ScheduleFeedFile` structs field-for-field
/// (names, types, and JSON casing -- neither struct carries a
/// `#[serde(rename_all = ...)]`, so plain snake_case field names already
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
#[derive(Debug, Clone, Serialize)]
struct ScheduleFeedIngestRequest {
    delivered_at: DateTime<Utc>,
    ingested_at: DateTime<Utc>,
    files: Vec<ScheduleFeedFile>,
}

#[derive(Debug, Clone, Serialize)]
struct ScheduleFeedFile {
    name: String,
    bytes: u64,
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
mod tests {
    use super::*;

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

        let bytes = delivery::build_test_zip(&[
            ("RJTTF942MCA.txt", b"mca content"),
            ("RJTTF942MSN.txt", b"msn content"),
        ]);
        std::fs::write(watch_dir.path().join("timetable_full.zip"), &bytes).unwrap();

        let config = test_config(watch_dir.path(), storage_dir.path());
        let client = Client::builder().timeout(REQUEST_TIMEOUT).build().unwrap();
        let internal_oauth = test_oauth();

        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();
        let mut known_stray_files = HashSet::new();
        let mut last_ingested_mtime = None;
        let mut pending_post = None;

        // stability_cycles = 2 by default in test_config -- two identical
        // cycles reach stability.
        for _ in 0..2 {
            run_scan_cycle(
                &client,
                &config,
                &internal_oauth,
                &mut tracker,
                &mut known_stable,
                &mut known_stray_files,
                &mut last_ingested_mtime,
                &mut pending_post,
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
            std::fs::read_to_string(delivery_dir.join("RJTTF942MCA.txt")).unwrap(),
            "mca content"
        );
        assert_eq!(
            std::fs::read_to_string(delivery_dir.join("RJTTF942MSN.txt")).unwrap(),
            "msn content"
        );
    }

    fn test_config(watch_dir: &std::path::Path, storage_dir: &std::path::Path) -> Config {
        Config {
            watch_dir: watch_dir.to_path_buf(),
            storage_dir: storage_dir.to_path_buf(),
            check_times: "22:00,16:00".to_string(),
            poll_interval_secs: 120,
            retention_keep_deliveries: 2,
            stability_cycles: 2,
            // Deliberately an address nothing listens on -- these tests
            // only exercise up to the POST attempt, not a real server.
            api_ingest_url: "http://127.0.0.1:1/schedule-feed-ingests".to_string(),
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
        }
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
