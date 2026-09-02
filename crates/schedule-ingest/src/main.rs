//! `schedule-ingest`: watches a locally mounted directory for a pushed CIF
//! SCHEDULE feed delivery from Network Rail/RDG, verifies completeness
//! against the delivery's own manifest, and forwards completed sequences to
//! the `api` crate's ingestion endpoint.
//!
//! See `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md` for
//! the full design and `docs/superpowers/plans/2026-09-01-schedule-feed-push-ingestion.md`
//! for the implementation plan this crate is built against. This service
//! never dials out itself — a sibling SFTPGo container receives the push
//! and writes into `watch_dir`; this crate only reads what lands there (see
//! `config.rs`).
//!
//! ## The last-ingested-sequence gap (Task 5's own scoped limitation)
//!
//! `GET /private/schedule-feed-ingests` (see `crates/api/src/routes/ingest.rs`)
//! currently only returns `fetched_at` — the last ingest *timestamp* — not
//! the last ingested *sequence number*. That means this crate cannot learn
//! the last sequence from `api` on startup. Since `schedule-ingest` keeps no
//! persistent state of its own (state lives in `api`, per the design), this
//! is a real gap between what Task 4 built and what Task 5 needs.
//!
//! Resolved here by tracking the last-ingested sequence **in-memory only**
//! (see `last_ingested_sequence` in [`main`]'s loop state). This is
//! acceptable because [`manifest::SequenceRelation::Gap`] is a loud-log-
//! and-still-proceed signal (RSPS5046 §7.4), never a hard blocker — a
//! `None` (fresh process, no in-memory history yet) already correctly
//! classifies as `Expected` via `classify_sequence(None, current)`. A
//! process restart therefore just costs one cycle where a genuine gap
//! wouldn't be logged/counted, which is an honestly-scoped limitation, not
//! a silently swallowed one. Extending the route to also return the last
//! sequence is a natural follow-up, but that's an `api` change and out of
//! scope for this task, which owns `schedule-ingest` only.

mod config;
mod manifest;
mod scan;

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use chrono::{DateTime, NaiveTime, TimeZone, Utc};
use chrono_tz::Europe::London;
use chrono_tz::Tz;
use clap::Parser;
use config::Config;
use manifest::SequenceRelation;
use reqwest::Client;
use scan::{DirSnapshot, StabilityTracker, scan_incoming};
use serde::Serialize;

/// Per-request timeout — matches the other pollers' identical rationale
/// (comfortably short relative to how infrequently this crate actually
/// polls: every tens of minutes at worst, per `check_times`).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics_enabled {
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
    let final_check_time = *check_times.last().expect("parse_check_times guarantees a non-empty list");

    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
        token_url: config.internal_oauth_token_url.clone(),
        client_id: config.internal_oauth_client_id.clone(),
        scope: config.internal_oauth_scope.clone(),
        username: config.internal_oauth_username.clone(),
        password: config.internal_oauth_password.clone(),
    });

    let mut tracker = StabilityTracker::new();
    let mut known_stable: HashSet<String> = HashSet::new();
    let mut last_ingested_sequence: Option<u32> = None;
    let mut pending_post: Option<ScheduleFeedIngestRequest> = None;
    let mut first_run = true;

    loop {
        // First run bypasses the check-time gate and scans immediately —
        // matching RSPS5046 §7.6.1's "new recipients get a full refresh
        // regardless of when they start", and this repo's existing
        // `common::ingest::time_until_next_poll` "no prior fetch -> poll
        // now" precedent (cited for that rationale only; its exact shape
        // -- a single fixed interval, not a list of daily check-times --
        // doesn't fit this scheduler, so it isn't called directly here).
        let current_slot = if first_run {
            first_run = false;
            tracing::info!("first run: bypassing the check-time gate for an immediate scan");
            None
        } else {
            let now_london = Utc::now().with_timezone(&London);
            let (target, slot) = next_check_time(now_london, &check_times);
            let sleep_for = target.signed_duration_since(now_london).to_std().unwrap_or(Duration::ZERO);
            tracing::info!(sleep_secs = sleep_for.as_secs(), slot = %slot, "sleeping until next check time");
            tokio::time::sleep(sleep_for).await;
            Some(slot)
        };

        let is_final_check_of_day = current_slot == Some(final_check_time);
        let cycle_start = Instant::now();

        if let Err(err) = run_scan_cycle(
            &client,
            &config,
            &internal_oauth,
            &mut tracker,
            &mut known_stable,
            &mut last_ingested_sequence,
            &mut pending_post,
            is_final_check_of_day,
        )
        .await
        {
            tracing::error!(error = ?err, "scan cycle failed unexpectedly; will retry next check time");
        }

        metrics::histogram!(common::metrics::metric_name("schedule_feed_scan_duration_seconds"))
            .record(cycle_start.elapsed().as_secs_f64());
    }
}

/// One check-time's worth of work: scan `watch_dir`, feed the snapshot into
/// the (process-lifetime) `StabilityTracker`, and if a manifest has landed
/// and is fully stable — itself and every file it lists — move the
/// delivery into `storage_dir` and record it with `api`.
///
/// Returns `Err` only for genuinely unexpected failures (e.g. `watch_dir`
/// itself unreadable); every "not ready yet" / "parse failed, try again
/// next cycle" outcome is handled internally via logging and an early
/// `Ok(())`, so a single bad cycle never crashes the process — see this
/// module's doc comment and the plan's Task 5 Step 2.
#[allow(clippy::too_many_arguments)]
async fn run_scan_cycle(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    tracker: &mut StabilityTracker,
    known_stable: &mut HashSet<String>,
    last_ingested_sequence: &mut Option<u32>,
    pending_post: &mut Option<ScheduleFeedIngestRequest>,
    is_final_check_of_day: bool,
) -> anyhow::Result<()> {
    // Retry a previously-moved-but-not-yet-successfully-posted delivery
    // first. The files already live under `storage_dir/<nnn>/` (moved
    // there once, never moved back on a failed POST — see this function's
    // tail below) — this only retries the HTTP call, using the exact sizes
    // observed at move time, not a re-stat. If this process restarts
    // before a pending POST ever succeeds, that in-memory pending record
    // is lost too (same class of limitation as the sequence-gap gap
    // documented in this module's doc comment) — a real but narrow gap,
    // not silently pretended away.
    if let Some(pending) = pending_post.take() {
        match post_ingest(client, config, internal_oauth, &pending).await {
            Ok(()) => {
                tracing::info!(sequence = pending.sequence, "retried and succeeded posting a previously-failed ingest record");
                *last_ingested_sequence = Some(pending.sequence as u32);
                metrics::gauge!(common::metrics::metric_name("schedule_feed_last_ingest_sequence")).set(pending.sequence as f64);
            }
            Err(err) => {
                tracing::error!(error = ?err, sequence = pending.sequence, "retry POST still failing; will retry again next cycle");
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

    let mut candidates = find_manifest_candidates(&snapshot);
    if candidates.len() > 1 {
        tracing::warn!(candidates = ?candidates, "multiple manifest-shaped files present at once; using the lexicographically greatest");
    }
    let Some(manifest_filename) = candidates.pop() else {
        tracing::debug!("no manifest file present in watch_dir yet");
        return Ok(());
    };

    if !known_stable.contains(&manifest_filename) {
        tracing::info!(manifest = %manifest_filename, "manifest file present but not yet stable");
        return Ok(());
    }

    let manifest_path = config.watch_dir.join(&manifest_filename);
    let content = match std::fs::read_to_string(&manifest_path) {
        Ok(content) => content,
        Err(err) => {
            tracing::error!(error = ?err, manifest = %manifest_filename, "failed to read a stable manifest file; retrying next cycle");
            return Ok(());
        }
    };

    // A manifest could theoretically still be mid-write despite mtime/size
    // stability in a pathological case -- don't crash the process over one
    // bad parse, just log and retry next cycle.
    let parsed = match manifest::parse(&content) {
        Ok(parsed) => parsed,
        Err(err) => {
            tracing::error!(error = ?err, manifest = %manifest_filename, "failed to parse a stable manifest's content; retrying next cycle");
            return Ok(());
        }
    };

    match manifest::classify_sequence(*last_ingested_sequence, parsed.sequence) {
        SequenceRelation::AlreadyIngested => {
            tracing::info!(sequence = parsed.sequence, "manifest sequence already ingested; heartbeat only");
            return Ok(());
        }
        SequenceRelation::Gap => {
            tracing::error!(
                last_sequence = ?*last_ingested_sequence,
                current_sequence = parsed.sequence,
                "non-contiguous schedule feed sequence number; proceeding to ingest anyway per RSPS5046 \u{a7}7.4"
            );
            metrics::counter!(common::metrics::metric_name("schedule_feed_sequence_gap_total")).increment(1);
        }
        SequenceRelation::Expected => {}
    }

    let missing = missing_listed_files(&parsed.files, &snapshot, known_stable);
    if !missing.is_empty() {
        if is_final_check_of_day {
            tracing::error!(
                sequence = parsed.sequence,
                missing = ?missing,
                "delivery still incomplete after the day's final configured check time; likely a real delivery problem"
            );
        } else {
            tracing::info!(sequence = parsed.sequence, missing = ?missing, "delivery not yet complete; retrying next check time");
        }
        return Ok(());
    }

    // Every listed file, plus the manifest itself, is present and stable --
    // the delivery is complete. Move it into storage before recording it,
    // so a POST failure never leaves a verified-complete delivery sitting
    // in `watch_dir` where a future stability reset could re-churn it.
    let sequence_dir = config.storage_dir.join(parsed.sequence.to_string());
    std::fs::create_dir_all(&sequence_dir)?;

    let mut files = Vec::with_capacity(parsed.files.len() + 1);
    for name in std::iter::once(&manifest_filename).chain(parsed.files.iter()) {
        // Use the size already observed by this cycle's snapshot/stability
        // check -- do NOT re-stat after the move, per the plan.
        let bytes = snapshot
            .0
            .get(name)
            .map(|&(_, len)| len)
            .ok_or_else(|| anyhow::anyhow!("file {name:?} vanished from the snapshot just before moving"))?;
        std::fs::rename(config.watch_dir.join(name), sequence_dir.join(name))?;
        files.push(ScheduleFeedFile { name: name.clone(), bytes });
    }

    let request = ScheduleFeedIngestRequest {
        sequence: parsed.sequence as i32,
        ingested_at: Utc::now(),
        files,
    };

    match post_ingest(client, config, internal_oauth, &request).await {
        Ok(()) => {
            tracing::info!(sequence = parsed.sequence, "schedule feed delivery moved to storage and posted to api");
            *last_ingested_sequence = Some(parsed.sequence);
            metrics::gauge!(common::metrics::metric_name("schedule_feed_last_ingest_sequence")).set(parsed.sequence as f64);
        }
        Err(err) => {
            // The files stay in `storage_dir/<nnn>/` -- this is a locally-
            // verified-complete delivery; a failed POST is a record-keeping
            // problem to retry, not a reason to move files back. See
            // `pending_post` handling at the top of this function.
            tracing::error!(error = ?err, sequence = parsed.sequence, "files moved to storage but POST to api failed; will retry next cycle");
            *pending_post = Some(request);
        }
    }

    if let Err(err) = prune_old_sequences(&config.storage_dir, config.retention_keep_sequences) {
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
        tracing::info!(sequence = request.sequence, files = request.files.len(), "posted schedule feed ingest to api");
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
#[derive(Debug, Clone, Serialize)]
struct ScheduleFeedIngestRequest {
    sequence: i32,
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
        .map(|s| NaiveTime::parse_from_str(s, "%H:%M").map_err(|error| anyhow::anyhow!("invalid check time {s:?}: {error}")))
        .collect::<Result<_, _>>()?;

    if times.is_empty() {
        anyhow::bail!("check_times must list at least one HH:MM time");
    }

    Ok(times)
}

/// Resolves a naive Europe/London datetime to a concrete offset, handling
/// the two DST edge cases without panicking: an ambiguous (fall-back fold)
/// local time resolves to its earlier occurrence, and a nonexistent
/// (spring-forward gap) local time is nudged forward by an hour. Both are
/// rare (one hour, once a year each) and only affect which exact instant a
/// sleep targets by at most an hour -- never which slot's logic runs.
fn resolve_london(naive: chrono::NaiveDateTime) -> DateTime<Tz> {
    match London.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(earliest, _latest) => earliest,
        chrono::LocalResult::None => London
            .from_local_datetime(&(naive + chrono::Duration::hours(1)))
            .single()
            .unwrap_or_else(|| Utc::now().with_timezone(&London)),
    }
}

/// Computes the next occurrence (today or tomorrow, whichever is sooner)
/// of each time in `check_times` after `now`, and returns the earliest one
/// together with which configured time it corresponds to. Order within
/// `check_times` doesn't matter for this computation (unlike
/// `final_check_time` in [`main`], which cares about list position, not
/// chronological order).
fn next_check_time(now: DateTime<Tz>, check_times: &[NaiveTime]) -> (DateTime<Tz>, NaiveTime) {
    check_times
        .iter()
        .map(|&time| {
            let today = resolve_london(now.date_naive().and_time(time));
            let target = if today > now {
                today
            } else {
                resolve_london((now.date_naive() + chrono::Duration::days(1)).and_time(time))
            };
            (target, time)
        })
        .min_by_key(|&(dt, _)| dt)
        .expect("check_times is validated non-empty by parse_check_times")
}

/// Whether `name` matches the `RJTTF<digits>DAT.txt` manifest filename
/// shape (e.g. `RJTTF942DAT.txt`).
fn is_manifest_filename(name: &str) -> bool {
    const PREFIX: &str = "RJTTF";
    const SUFFIX: &str = "DAT.txt";

    if !name.starts_with(PREFIX) || !name.ends_with(SUFFIX) || name.len() < PREFIX.len() + SUFFIX.len() {
        return false;
    }

    let middle = &name[PREFIX.len()..name.len() - SUFFIX.len()];
    !middle.is_empty() && middle.bytes().all(|b| b.is_ascii_digit())
}

/// Every filename in `snapshot` matching [`is_manifest_filename`], sorted
/// ascending (so the caller can `pop()` the lexicographically greatest --
/// normally there's at most one candidate at a time; a second is a
/// pathological case the caller logs about).
fn find_manifest_candidates(snapshot: &DirSnapshot) -> Vec<String> {
    let mut names: Vec<String> = snapshot.0.keys().filter(|name| is_manifest_filename(name)).cloned().collect();
    names.sort();
    names
}

/// Filenames from `files` that are either absent from `snapshot` or not
/// yet a member of `known_stable` -- i.e. not yet safe to treat as part of
/// a complete delivery.
fn missing_listed_files(files: &[String], snapshot: &DirSnapshot, known_stable: &HashSet<String>) -> Vec<String> {
    files
        .iter()
        .filter(|name| !(snapshot.0.contains_key(name.as_str()) && known_stable.contains(name.as_str())))
        .cloned()
        .collect()
}

/// Keeps only the `keep` highest-numbered immediate subdirectories of
/// `storage_dir` whose name parses as a `u32`; removes the rest via
/// `std::fs::remove_dir_all`. Non-numeric or malformed subdirectory names
/// (and any plain files directly in `storage_dir`) are left untouched --
/// they aren't this function's concern, and it never guesses about them.
fn prune_old_sequences(storage_dir: &std::path::Path, keep: u32) -> anyhow::Result<()> {
    let mut sequences: Vec<(u32, PathBuf)> = Vec::new();

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
        if let Ok(sequence) = name.parse::<u32>() {
            sequences.push((sequence, entry.path()));
        }
    }

    sequences.sort_by_key(|&(sequence, _)| sequence);

    let keep = keep as usize;
    if sequences.len() > keep {
        let remove_count = sequences.len() - keep;
        for (sequence, path) in &sequences[..remove_count] {
            tracing::info!(sequence = sequence, path = ?path, "pruning old schedule feed sequence directory");
            std::fs::remove_dir_all(path)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest_files() -> Vec<String> {
        vec![
            "RJTTF942ZTR.txt".to_string(),
            "RJTTF942REJ.txt".to_string(),
            "RJTTF942SET.txt".to_string(),
            "RJTTF942FLF.txt".to_string(),
            "RJTTF942MCA.txt".to_string(),
            "RJTTF942MSN.txt".to_string(),
            "RJTTF942ALF.txt".to_string(),
            "RJTTF942TSI.txt".to_string(),
        ]
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
    fn next_check_time_picks_the_soonest_upcoming_slot_today() {
        let now = London.with_ymd_and_hms(2026, 9, 1, 10, 0, 0).unwrap();
        let check_times = vec![
            NaiveTime::from_hms_opt(9, 0, 0).unwrap(),  // already passed today
            NaiveTime::from_hms_opt(16, 0, 0).unwrap(), // later today -- soonest
            NaiveTime::from_hms_opt(22, 0, 0).unwrap(), // later today
        ];

        let (target, slot) = next_check_time(now, &check_times);
        assert_eq!(slot, NaiveTime::from_hms_opt(16, 0, 0).unwrap());
        assert_eq!(target.date_naive(), now.date_naive());
    }

    #[test]
    fn next_check_time_wraps_to_tomorrow_once_every_slot_today_has_passed() {
        let now = London.with_ymd_and_hms(2026, 9, 1, 23, 0, 0).unwrap();
        let check_times = vec![NaiveTime::from_hms_opt(16, 0, 0).unwrap()];

        let (target, slot) = next_check_time(now, &check_times);
        assert_eq!(slot, NaiveTime::from_hms_opt(16, 0, 0).unwrap());
        assert_eq!(target.date_naive(), now.date_naive() + chrono::Duration::days(1));
    }

    #[test]
    fn manifest_filename_matching() {
        assert!(is_manifest_filename("RJTTF942DAT.txt"));
        assert!(!is_manifest_filename("RJTTF942ZTR.txt"));
        assert!(!is_manifest_filename("RJTTFDAT.txt"));
        assert!(!is_manifest_filename("DAT.txt"));
        assert!(!is_manifest_filename("RJTTF9a2DAT.txt"));
    }

    #[test]
    fn complete_stable_nine_file_delivery_is_ready_to_move() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("RJTTF942DAT.txt"), b"manifest").unwrap();
        for name in sample_manifest_files() {
            std::fs::write(dir.path().join(&name), b"data").unwrap();
        }

        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();

        // Two polling cycles with nothing changing on disk reaches
        // stability at stability_cycles = 2 (this crate's own default).
        for _ in 0..2 {
            let snapshot = scan_incoming(dir.path()).unwrap();
            let just_stable = tracker.observe(&snapshot, 2);
            known_stable.extend(just_stable);
        }

        let snapshot = scan_incoming(dir.path()).unwrap();
        assert!(known_stable.contains("RJTTF942DAT.txt"));

        let missing = missing_listed_files(&sample_manifest_files(), &snapshot, &known_stable);
        assert!(missing.is_empty(), "expected a complete delivery to have no missing files, got {missing:?}");
    }

    #[test]
    fn delivery_missing_one_listed_file_is_not_ready() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("RJTTF942DAT.txt"), b"manifest").unwrap();
        let files = sample_manifest_files();
        for name in &files[..files.len() - 1] {
            std::fs::write(dir.path().join(name), b"data").unwrap();
        }

        let mut tracker = StabilityTracker::new();
        let mut known_stable = HashSet::new();
        for _ in 0..2 {
            let snapshot = scan_incoming(dir.path()).unwrap();
            let just_stable = tracker.observe(&snapshot, 2);
            known_stable.extend(just_stable);
        }

        let snapshot = scan_incoming(dir.path()).unwrap();
        let missing = missing_listed_files(&files, &snapshot, &known_stable);
        assert_eq!(missing, vec![files.last().unwrap().clone()]);
    }

    #[test]
    fn prune_keeps_only_the_n_highest_numeric_subdirectories() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["1", "2", "3", "4", "not-a-number", "04a"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        // A stray file directly in storage_dir should be ignored entirely,
        // not treated as (or crash on) a subdirectory.
        std::fs::write(dir.path().join("stray.txt"), b"x").unwrap();

        prune_old_sequences(dir.path(), 2).unwrap();

        let mut remaining: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();

        assert_eq!(remaining, vec!["04a".to_string(), "3".to_string(), "4".to_string(), "not-a-number".to_string(), "stray.txt".to_string()]);
    }

    /// Regression test, same shape as `scan::scan_incoming_on_nonexistent_
    /// directory_returns_empty_snapshot_not_an_error`: a not-yet-existing
    /// storage_dir must not error, since there is genuinely nothing to
    /// prune.
    #[test]
    fn prune_on_nonexistent_storage_dir_is_a_noop_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist-yet");
        prune_old_sequences(&missing, 2).unwrap();
    }

    #[test]
    fn prune_is_a_noop_when_at_or_under_the_keep_count() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["1", "2"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }

        prune_old_sequences(dir.path(), 2).unwrap();

        let mut remaining: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        remaining.sort();

        assert_eq!(remaining, vec!["1".to_string(), "2".to_string()]);
    }
}
