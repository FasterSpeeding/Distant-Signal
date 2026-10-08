//! Network Rail CORPUS loading: the second, independent pipeline sharing
//! `watch_dir` with the CIF zip. Off unless `CORPUS_INGEST_ENABLED=true`.
//! See docs/superpowers/specs/2026-09-28-corpus-sftp-ingest-design.md.
//!
//! RDM pushes `CORPUSExtract.json.gz` (gzipped
//! `{"TIPLOCDATA":[{NLC, STANOX, TIPLOC, 3ALPHA, UIC, NLCDESC,
//! NLCDESC16}, ...]}`, about monthly) over the same SFTP account as the
//! CIF feed. Each cycle, [`run_corpus_cycle`]:
//!
//! 1. picks the newest stable file matching `CORPUS_FILE_PATTERN`;
//! 2. gunzips (capped) and parses it, normalising every value
//!    ([`parse_extract`]) -- a file that is not gzip, not that JSON, has a
//!    row with no NLC or fewer than `CORPUS_MIN_ROWS` rows is REJECTED:
//!    counted, logged and moved to `storage_dir/corpus/rejected/`, never
//!    loaded;
//! 3. hands the whole set to the sink (`sink.rs`): POSTed to api, or under
//!    `INGEST_SINK=db` written directly; either way `corpus_locations` is
//!    replaced in one transaction;
//! 4. on success moves the file to `storage_dir/corpus/`, keeping the
//!    newest `CORPUS_RETENTION_KEEP`, so files never pile up in
//!    `watch_dir`.
//!
//! Both moves re-check the file's `(mtime, size)` first, so a re-upload
//! that starts mid-cycle is never moved away half-written. A failed POST
//! leaves the file where it is and the next cycle tries again; loading the
//! same file twice is idempotent on the api side, so a restart at any point
//! is safe without persisted state.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::config::CorpusArgs;
use crate::delivery::delivery_dir_name;
use crate::pattern::Routing;
use crate::scan::{DirSnapshot, StabilityTracker, scan_incoming};
use crate::sink::{CorpusLoadRequest, IngestSink};

/// Counts rejected CORPUS files; `DistantSignalCorpusRejected` reads it.
pub(crate) const REJECTED_METRIC: &str = "schedule_feed_corpus_rejected_total";
const LAST_LOAD_METRIC: &str = "schedule_feed_corpus_last_load_delivered_at_seconds";
const ROWS_METRIC: &str = "schedule_feed_corpus_rows";

/// Subdirectory of `storage_dir` that processed files are moved into.
const ARCHIVE_DIR: &str = "corpus";
/// Subdirectory of [`ARCHIVE_DIR`] for rejected files.
const REJECTED_DIR: &str = "rejected";

/// One normalised CORPUS location: `ds_store`'s own type (plan 2d.1), the
/// one the api route deserialises and the direct sink writes.
pub(crate) use ds_store::corpus::CorpusLocation;

/// Why a file can never be loaded. Its bytes cannot change without its
/// `(mtime, size)` changing, so it is moved aside rather than retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rejected(pub String);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Process-lifetime state of the CORPUS pipeline. Nothing here needs to
/// survive a restart (see the module doc).
#[derive(Debug, Default)]
pub(crate) struct CorpusState {
    tracker: StabilityTracker,
    known_stable: HashSet<String>,
    /// The last file this process loaded or rejected, as `(name, mtime,
    /// size)`. Only matters when the move out of `watch_dir` failed: it
    /// stops the same file being re-POSTed (or re-counted as rejected)
    /// every cycle while the move is retried.
    handled: Option<(String, SystemTime, u64)>,
}

impl CorpusState {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// Registers the metrics at 0 so the alert's `increase()` sees the first
/// rejection.
pub(crate) fn register_metrics() {
    metrics::counter!(common::metrics::metric_name(REJECTED_METRIC)).increment(0);
}

/// Gunzips `compressed`, refusing more than `max_bytes` of output.
pub(crate) fn decompress(compressed: &[u8], max_bytes: u64) -> Result<Vec<u8>, Rejected> {
    if !compressed.starts_with(&[0x1f, 0x8b]) {
        return Err(Rejected("not a gzip file".to_string()));
    }
    let mut out = Vec::new();
    flate2::read::MultiGzDecoder::new(compressed)
        .take(max_bytes.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|err| Rejected(format!("gzip stream is corrupt: {err}")))?;
    if out.len() as u64 > max_bytes {
        return Err(Rejected(format!(
            "decompresses to more than CORPUS_MAX_DECOMPRESSED_BYTES ({max_bytes})"
        )));
    }
    Ok(out)
}

#[derive(Deserialize)]
struct Extract {
    #[serde(rename = "TIPLOCDATA")]
    tiploc_data: Vec<RawRow>,
}

/// One raw `TIPLOCDATA` row. Every field is read as a raw JSON value:
/// CORPUS pads absent values with a single space, and extracts have carried
/// `NLC`/`STANOX` both as numbers and as strings.
#[derive(Deserialize)]
struct RawRow {
    #[serde(rename = "NLC", default)]
    nlc: serde_json::Value,
    #[serde(rename = "STANOX", default)]
    stanox: serde_json::Value,
    #[serde(rename = "TIPLOC", default)]
    tiploc: serde_json::Value,
    #[serde(rename = "3ALPHA", default)]
    three_alpha: serde_json::Value,
    #[serde(rename = "UIC", default)]
    uic: serde_json::Value,
    #[serde(rename = "NLCDESC", default)]
    nlc_desc: serde_json::Value,
    #[serde(rename = "NLCDESC16", default)]
    nlc_desc16: serde_json::Value,
}

/// A string or number as trimmed text; `None` for null or blank. Anything
/// else (an array, an object, a boolean) is not CORPUS.
#[allow(
    clippy::similar_names,
    reason = "only rustc 1.88's clippy flags these names, so #[expect] can't be used"
)]
fn text(value: &serde_json::Value, field: &str, row: usize) -> Result<Option<String>, Rejected> {
    let raw = match value {
        serde_json::Value::Null => return Ok(None),
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        other => {
            return Err(Rejected(format!(
                "row {row}: {field} is {other}, not a string or number"
            )));
        }
    };
    Ok((!raw.is_empty()).then_some(raw))
}

/// Left-pads an all-digit code to `width` (a JSON-number NLC or STANOX has
/// lost its leading zeros); anything else is kept as it is.
fn pad_digits(code: String, width: usize) -> String {
    if code.len() < width && code.bytes().all(|b| b.is_ascii_digit()) {
        format!("{code:0>width$}")
    } else {
        code
    }
}

/// Parses and normalises one CORPUS extract's JSON.
pub(crate) fn parse_extract(json: &[u8], min_rows: usize) -> Result<Vec<CorpusLocation>, Rejected> {
    let extract: Extract = serde_json::from_slice(json).map_err(|err| {
        Rejected(format!(
            "not a CORPUS extract (expected JSON with a TIPLOCDATA array): {err}"
        ))
    })?;
    if extract.tiploc_data.len() < min_rows {
        return Err(Rejected(format!(
            "only {} rows, fewer than CORPUS_MIN_ROWS ({min_rows})",
            extract.tiploc_data.len()
        )));
    }
    extract
        .tiploc_data
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let nlc =
                text(&row.nlc, "NLC", i)?.ok_or_else(|| Rejected(format!("row {i} has no NLC")))?;
            Ok(CorpusLocation {
                nlc: pad_digits(nlc, 6),
                stanox: text(&row.stanox, "STANOX", i)?.map(|s| pad_digits(s, 5)),
                tiploc: text(&row.tiploc, "TIPLOC", i)?,
                crs: text(&row.three_alpha, "3ALPHA", i)?,
                uic: text(&row.uic, "UIC", i)?,
                nlc_desc: text(&row.nlc_desc, "NLCDESC", i)?,
                nlc_desc16: text(&row.nlc_desc16, "NLCDESC16", i)?,
            })
        })
        .collect()
}

/// Every stable-shaped CORPUS candidate in `snapshot`, oldest first.
fn candidates(snapshot: &DirSnapshot, routing: &Routing) -> Vec<(String, SystemTime, u64)> {
    let mut found: Vec<(String, SystemTime, u64)> = snapshot
        .0
        .iter()
        .filter(|(name, _)| routing.is_corpus(name))
        .map(|(name, &(mtime, len))| (name.clone(), mtime, len))
        .collect();
    found.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    found
}

/// Moves `watch_dir/name` to `dest_dir/<YYYYMMDDTHHMMSSZ>-name` -- unless
/// its `(mtime, size)` no longer matches what was processed, i.e. a new
/// upload has started, in which case it is left alone (`Ok(false)`).
fn move_if_unchanged(
    watch_dir: &Path,
    name: &str,
    expected: (SystemTime, u64),
    dest_dir: &Path,
) -> std::io::Result<bool> {
    let src = watch_dir.join(name);
    let metadata = std::fs::metadata(&src)?;
    if (metadata.modified()?, metadata.len()) != expected {
        return Ok(false);
    }
    std::fs::create_dir_all(dest_dir)?;
    let dest = dest_dir.join(format!("{}-{name}", delivery_dir_name(expected.0)));
    std::fs::rename(&src, &dest)?;
    Ok(true)
}

/// Whether `name` is `<YYYYMMDDTHHMMSSZ>-<anything>`, the shape
/// [`move_if_unchanged`] writes. Pruning touches nothing else.
fn is_archived_name(name: &str) -> bool {
    name.len() > 17
        && name.as_bytes()[16] == b'-'
        && crate::delivery::is_delivery_dir_name(&name[..16])
}

/// Keeps the `keep` newest archived files in `dir`, removing older ones.
fn prune_archive(dir: &Path, keep: u32) -> std::io::Result<()> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        if let Some(name) = entry.file_name().to_str()
            && is_archived_name(name)
        {
            files.push((name.to_string(), entry.path()));
        }
    }
    files.sort();
    let excess = files.len().saturating_sub(keep as usize);
    for (name, path) in &files[..excess] {
        tracing::info!(file = %name, "pruning an old archived CORPUS file");
        std::fs::remove_file(path)?;
    }
    Ok(())
}

/// Moves a processed file out of `watch_dir` and prunes its destination,
/// logging (never failing the cycle) on error.
fn archive(watch_dir: &Path, name: &str, stat: (SystemTime, u64), dest_dir: &Path, keep: u32) {
    match move_if_unchanged(watch_dir, name, stat, dest_dir) {
        Ok(true) => {
            tracing::info!(file = %name, dest = %dest_dir.display(), "moved CORPUS file out of watch_dir");
        }
        Ok(false) => {
            tracing::info!(file = %name, "CORPUS file changed since it was read (a new upload?); left in watch_dir");
        }
        Err(err) => {
            tracing::error!(error = %err, file = %name, "failed to move CORPUS file out of watch_dir; retrying next cycle");
        }
    }
    if let Err(err) = prune_archive(dest_dir, keep) {
        tracing::error!(error = %err, dir = %dest_dir.display(), "CORPUS archive pruning failed");
    }
}

/// One poll interval of the CORPUS pipeline (see the module doc). Returns
/// `Err` only when `watch_dir` itself cannot be read; every per-file
/// problem is logged and handled.
#[expect(
    clippy::cast_precision_loss,
    reason = "metric gauges take f64, and these counts and timestamps stay far below 2^52"
)]
pub(crate) async fn run_corpus_cycle(
    sink: &impl IngestSink,
    watch_dir: &Path,
    storage_dir: &Path,
    args: &CorpusArgs,
    routing: &Routing,
    stability_cycles: u32,
    state: &mut CorpusState,
) -> anyhow::Result<()> {
    let snapshot = scan_incoming(watch_dir)?;
    let just_stabilized = state.tracker.observe(&snapshot, stability_cycles);
    state.known_stable.extend(just_stabilized);
    state
        .known_stable
        .retain(|name| snapshot.0.contains_key(name));

    let mut found = candidates(&snapshot, routing);
    let Some((name, mtime, len)) = found.pop() else {
        return Ok(());
    };
    if !state.known_stable.contains(&name) {
        tracing::info!(file = %name, "CORPUS file present but not yet stable");
        return Ok(());
    }
    let archive_dir = storage_dir.join(ARCHIVE_DIR);
    let rejected_dir = archive_dir.join(REJECTED_DIR);
    let keep = args.corpus_retention_keep;

    if state.handled.as_ref() == Some(&(name.clone(), mtime, len)) {
        // Loaded or rejected already; only the move out of watch_dir
        // failed. Retry just that.
        archive(watch_dir, &name, (mtime, len), &archive_dir, keep);
        return Ok(());
    }

    let delivered_at = DateTime::<Utc>::from(mtime);
    let (delivered, locations) = match std::fs::read(watch_dir.join(&name)) {
        Ok(bytes) if bytes.len() as u64 == len => (
            crate::audit::DeliveredFile::from_bytes(&name, &bytes),
            decompress(&bytes, args.corpus_max_decompressed_bytes)
                .and_then(|json| parse_extract(&json, args.corpus_min_rows)),
        ),
        Ok(_) => {
            tracing::info!(file = %name, "CORPUS file changed while being read; retrying next cycle");
            return Ok(());
        }
        Err(err) => {
            tracing::error!(error = %err, file = %name, "failed to read CORPUS file; retrying next cycle");
            return Ok(());
        }
    };
    let locations = match locations {
        Ok(locations) => locations,
        Err(rejected) => {
            tracing::error!(file = %name, reason = %rejected, "rejecting CORPUS file; it is not loaded and is moved to the rejected archive");
            crate::audit::decision(
                &delivered,
                delivered_at,
                crate::audit::Outcome::CorpusRejected,
                Some(&rejected.0),
            );
            metrics::counter!(common::metrics::metric_name(REJECTED_METRIC)).increment(1);
            state.handled = Some((name.clone(), mtime, len));
            archive(watch_dir, &name, (mtime, len), &rejected_dir, keep);
            return Ok(());
        }
    };

    let request = CorpusLoadRequest {
        delivered_at,
        source_file: &name,
        locations: &locations,
        source_bytes: delivered.bytes,
        sha256: &delivered.sha256,
    };
    if let Err(err) = sink.load_corpus(&request).await {
        // Every failure is retried, a rejection too (as before the sink):
        // the file stays in watch_dir.
        tracing::error!(error = %err, file = %name, "CORPUS load POST to api failed; retrying next cycle");
        return Ok(());
    }
    tracing::info!(file = %name, delivered_at = %delivered_at, rows = locations.len(), "loaded CORPUS extract");
    crate::audit::decision(
        &delivered,
        delivered_at,
        crate::audit::Outcome::Accepted,
        None,
    );
    metrics::gauge!(common::metrics::metric_name(LAST_LOAD_METRIC))
        .set(delivered_at.timestamp() as f64);
    metrics::gauge!(common::metrics::metric_name(ROWS_METRIC)).set(locations.len() as f64);
    state.handled = Some((name.clone(), mtime, len));
    archive(watch_dir, &name, (mtime, len), &archive_dir, keep);

    // Older matching files (only possible with a wildcard pattern) are
    // superseded by the one just loaded: archive them unloaded, so a later
    // cycle can never load older data over newer.
    for (old, old_mtime, old_len) in found {
        if state.known_stable.contains(&old) {
            tracing::info!(file = %old, "archiving a CORPUS file superseded by a newer one, without loading it");
            match crate::audit::DeliveredFile::hash_file(&old, &watch_dir.join(&old)) {
                Ok(superseded) => crate::audit::decision(
                    &superseded,
                    DateTime::<Utc>::from(old_mtime),
                    crate::audit::Outcome::CorpusRejected,
                    Some(&format!("superseded by the newer {name}; not loaded")),
                ),
                Err(err) => {
                    tracing::warn!(error = %err, file = %old, "failed to hash a superseded CORPUS file for its audit line");
                }
            }
            archive(watch_dir, &old, (old_mtime, old_len), &archive_dir, keep);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    /// A hand-trimmed excerpt of a real `CORPUSExtract.json` (2026-09
    /// delivery): blanks are single spaces, `NLC` a JSON number, plus
    /// padded descriptions and a TIPLOC with an inner space.
    const FIXTURE: &str = include_str!("../tests/fixtures/corpus_extract_excerpt.json");

    pub(crate) fn gzip(bytes: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(bytes).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn the_real_shape_parses_and_normalises() {
        let rows = parse_extract(FIXTURE.as_bytes(), 1).unwrap();
        assert_eq!(rows.len(), 8);
        assert_eq!(
            rows[0],
            CorpusLocation {
                nlc: "000700".to_string(),
                stanox: Some("43423".to_string()),
                tiploc: Some("FENTON".to_string()),
                crs: Some("FMA".to_string()),
                uic: Some("00070".to_string()),
                nlc_desc: Some("FENTON MANOR".to_string()),
                nlc_desc16: None,
            }
        );
        // All-blank codes become None; NLCDESC16 keeps its text, trimmed.
        assert_eq!(
            rows[1],
            CorpusLocation {
                nlc: "000800".to_string(),
                stanox: None,
                tiploc: None,
                crs: None,
                uic: None,
                nlc_desc: Some("MERSEYRAIL ELECTRICS-HQ INPUT".to_string()),
                nlc_desc16: Some("MPTE HQ INPUT".to_string()),
            }
        );
        let clapham = rows
            .iter()
            .find(|r| r.crs.as_deref() == Some("CLJ"))
            .unwrap();
        assert_eq!(clapham.tiploc.as_deref(), Some("CLPHMJN"));
        assert_eq!(clapham.nlc, "559500");
        let padded = rows.iter().find(|r| r.nlc == "011411").unwrap();
        assert_eq!(padded.nlc_desc.as_deref(), Some("CE NORTH"));
        assert_eq!(padded.nlc_desc16.as_deref(), Some("NORTH EAST CWP"));
        assert!(rows.iter().any(|r| r.tiploc.as_deref() == Some("ST BZPM")));
    }

    /// Checks a full, real extract (never committed) when pointed at one:
    /// `CORPUS_EXTRACT_SAMPLE=/path/CORPUSExtract.json.gz cargo test -p
    /// schedule-ingest -- --ignored full_real_extract`.
    #[test]
    #[ignore = "needs CORPUS_EXTRACT_SAMPLE pointing at a real CORPUSExtract.json.gz"]
    fn full_real_extract_decompresses_and_parses_under_the_defaults() {
        let Ok(path) = std::env::var("CORPUS_EXTRACT_SAMPLE") else {
            return;
        };
        let bytes = std::fs::read(path).unwrap();
        let json = decompress(&bytes, 256 * 1024 * 1024).unwrap();
        let rows = parse_extract(&json, 10_000).unwrap();
        assert!(rows.len() > 50_000, "{}", rows.len());
        assert!(rows.iter().all(|r| !r.nlc.is_empty()));
        assert!(
            rows.iter()
                .filter_map(|r| r.crs.as_deref())
                .all(|crs| crs.len() == 3)
        );
    }

    #[test]
    fn string_codes_are_accepted_and_padded_like_numbers() {
        let json = br#"{"TIPLOCDATA":[
            {"NLC":"700","STANOX":4311,"TIPLOC":"X","3ALPHA":" ","UIC":" ","NLCDESC":"A","NLCDESC16":" "},
            {"NLC":"000700","STANOX":"04311","TIPLOC":"Y","3ALPHA":" ","UIC":" ","NLCDESC":"B","NLCDESC16":" "}
        ]}"#;
        let rows = parse_extract(json, 1).unwrap();
        assert_eq!(rows[0].nlc, "000700");
        assert_eq!(rows[0].stanox.as_deref(), Some("04311"));
        assert_eq!(rows[1].nlc, "000700");
        assert_eq!(rows[1].stanox.as_deref(), Some("04311"));
    }

    #[test]
    fn garbage_is_rejected_not_loaded() {
        assert!(parse_extract(b"{}", 1).is_err());
        assert!(parse_extract(b"TD,STEPTYPE\nA2,B", 1).is_err());
        assert!(parse_extract(br#"{"TIPLOCDATA":[{"NLC":" "}]}"#, 1).is_err());
        assert!(parse_extract(br#"{"TIPLOCDATA":[{"NLC":1,"TIPLOC":[1]}]}"#, 1).is_err());
        let too_few = parse_extract(FIXTURE.as_bytes(), 9).unwrap_err();
        assert!(too_few.0.contains("CORPUS_MIN_ROWS"), "{too_few}");
    }

    #[test]
    fn decompress_requires_gzip_and_caps_the_output() {
        assert_eq!(decompress(&gzip(b"hello"), 5).unwrap(), b"hello");
        assert!(decompress(b"plain json", 1024).is_err());
        let err = decompress(&gzip(&[b'x'; 64]), 63).unwrap_err();
        assert!(err.0.contains("CORPUS_MAX_DECOMPRESSED_BYTES"), "{err}");
    }

    #[test]
    fn archived_names_are_recognised_and_pruned_oldest_first() {
        assert!(is_archived_name("20260901T030000Z-CORPUSExtract.json.gz"));
        assert!(!is_archived_name("CORPUSExtract.json.gz"));
        assert!(!is_archived_name("20260901T030000Z"));

        let dir = tempfile::tempdir().unwrap();
        for name in [
            "20260701T030000Z-CORPUSExtract.json.gz",
            "20260801T030000Z-CORPUSExtract.json.gz",
            "20260901T030000Z-CORPUSExtract.json.gz",
            "unrelated.txt",
        ] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join(REJECTED_DIR)).unwrap();
        prune_archive(dir.path(), 2).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "20260801T030000Z-CORPUSExtract.json.gz",
                "20260901T030000Z-CORPUSExtract.json.gz",
                REJECTED_DIR,
                "unrelated.txt"
            ]
        );
    }

    #[test]
    fn a_changed_file_is_not_moved() {
        let watch = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let path = watch.path().join("CORPUSExtract.json.gz");
        std::fs::write(&path, b"abc").unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        let stat = (metadata.modified().unwrap(), metadata.len());

        assert!(
            !move_if_unchanged(
                watch.path(),
                "CORPUSExtract.json.gz",
                (stat.0, 4),
                dest.path()
            )
            .unwrap()
        );
        assert!(path.exists());
        assert!(
            move_if_unchanged(watch.path(), "CORPUSExtract.json.gz", stat, dest.path()).unwrap()
        );
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(dest.path()).unwrap().count(), 1);
    }

    fn args(api_url: String) -> CorpusArgs {
        CorpusArgs {
            corpus_ingest_enabled: true,
            corpus_file_pattern: crate::config::DEFAULT_CORPUS_FILE_PATTERN.to_string(),
            corpus_api_url: api_url,
            corpus_max_decompressed_bytes: 1024 * 1024,
            corpus_min_rows: 1,
            corpus_retention_keep: 3,
        }
    }

    /// The HTTP sink at `args.corpus_api_url`, as `main` builds it.
    fn http_sink(
        args: &CorpusArgs,
        tokens: common::oauth_client::OAuthTokenCache,
    ) -> crate::sink::HttpSink {
        crate::sink::HttpSink::new(
            reqwest::Client::new(),
            "http://127.0.0.1:1/unused".to_string(),
            args.corpus_api_url.clone(),
            tokens,
        )
    }

    fn oauth(token_url: String) -> common::oauth_client::OAuthTokenCache {
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url,
            client_id: "test-client".to_string(),
            scope: "groups".to_string(),
            username: "test-user".to_string(),
            password: "test-password".to_string(),
        })
    }

    /// Runs `cycles` CORPUS cycles (`stability_cycles` = 2).
    async fn run(
        watch: &Path,
        storage: &Path,
        args: &CorpusArgs,
        sink: &impl IngestSink,
        state: &mut CorpusState,
        cycles: usize,
    ) {
        for _ in 0..cycles {
            run_corpus_cycle(
                sink,
                watch,
                storage,
                args,
                &Routing::with_corpus(),
                2,
                state,
            )
            .await
            .unwrap();
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = match std::fs::read_dir(dir) {
            Ok(read_dir) => read_dir
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        names
    }

    /// The whole path against a mock api: the stable extract is `POSTed`
    /// once with normalised rows, then moved out of `watch_dir` into the
    /// archive; the SMART `.csv.gz` sharing its name is never touched.
    #[tokio::test]
    async fn a_stable_extract_is_loaded_once_and_archived() {
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
            .and(path("/private/corpus-locations"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"upserted": 8})),
            )
            .expect(1)
            .mount(&server)
            .await;

        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let delivered = gzip(FIXTURE.as_bytes());
        std::fs::write(watch.path().join("CORPUSExtract.json.gz"), &delivered).unwrap();
        std::fs::write(
            watch.path().join("CORPUSExtract.csv.gz"),
            gzip(b"A2,B,0632"),
        )
        .unwrap();

        let args = args(format!("{}/private/corpus-locations", server.uri()));
        let sink = http_sink(&args, oauth(format!("{}/token", server.uri())));
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args, &sink, &mut state, 4).await;
        drop(guard);

        // Provenance: the POST and one audit line carry the file's hash.
        let expected = crate::audit::DeliveredFile::from_bytes("CORPUSExtract.json.gz", &delivered);
        let audit = logs.audit_lines();
        assert_eq!(audit.len(), 1, "{audit:?}");
        assert_eq!(audit[0]["file"], "CORPUSExtract.json.gz");
        assert_eq!(audit[0]["outcome"], "accepted");
        assert_eq!(audit[0]["sha256"], expected.sha256.as_str());
        assert_eq!(audit[0]["bytes"], expected.bytes);

        assert_eq!(names(watch.path()), ["CORPUSExtract.csv.gz"]);
        let archived = names(&storage.path().join(ARCHIVE_DIR));
        assert_eq!(archived.len(), 1);
        assert!(
            archived[0].ends_with("-CORPUSExtract.json.gz"),
            "{archived:?}"
        );

        let requests = server.received_requests().await.unwrap();
        let load = requests
            .iter()
            .find(|r| r.url.path() == "/private/corpus-locations")
            .unwrap();
        let body: serde_json::Value = serde_json::from_slice(&load.body).unwrap();
        assert_eq!(body["source_file"], "CORPUSExtract.json.gz");
        assert_eq!(body["sha256"], expected.sha256.as_str());
        assert_eq!(body["source_bytes"], expected.bytes);
        assert_eq!(body["locations"].as_array().unwrap().len(), 8);
        assert_eq!(body["locations"][0]["nlc"], "000700");
        assert_eq!(body["locations"][1]["tiploc"], serde_json::Value::Null);
    }

    /// A failing api leaves the file in place for the next cycle.
    #[tokio::test]
    async fn a_failed_load_leaves_the_file_for_the_next_cycle() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        std::fs::write(
            watch.path().join("CORPUSExtract.json.gz"),
            gzip(FIXTURE.as_bytes()),
        )
        .unwrap();
        let args = args("http://127.0.0.1:1/private/corpus-locations".to_string());
        let sink = http_sink(&args, oauth("http://127.0.0.1:1/token".to_string()));
        let mut state = CorpusState::new();
        run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;

        assert_eq!(names(watch.path()), ["CORPUSExtract.json.gz"]);
        assert!(names(storage.path()).is_empty());
    }

    /// An extract that is not CORPUS is never `POSTed`: it is moved to the
    /// rejected archive (no api is listening here, so a POST attempt would
    /// have left the file in place instead).
    #[tokio::test]
    async fn a_malformed_extract_is_rejected_and_moved_aside() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        std::fs::write(
            watch.path().join("CORPUSExtract.json.gz"),
            gzip(b"TD,STEPTYPE,FROMBERTH\nA2,B,0632\n"),
        )
        .unwrap();
        let args = args("http://127.0.0.1:1/private/corpus-locations".to_string());
        let sink = http_sink(&args, oauth("http://127.0.0.1:1/token".to_string()));
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;
        drop(guard);

        let audit = logs.audit_lines();
        assert_eq!(audit.len(), 1, "{audit:?}");
        assert_eq!(audit[0]["outcome"], "corpus_rejected");
        assert!(
            audit[0]["reason"]
                .as_str()
                .is_some_and(|r| r.contains("not a CORPUS extract")),
            "{audit:?}"
        );
        assert!(names(watch.path()).is_empty());
        let rejected = names(&storage.path().join(ARCHIVE_DIR).join(REJECTED_DIR));
        assert_eq!(rejected.len(), 1);
        assert!(
            rejected[0].ends_with("-CORPUSExtract.json.gz"),
            "{rejected:?}"
        );
    }
}
