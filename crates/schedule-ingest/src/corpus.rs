//! Network Rail CORPUS loading: the second, independent pipeline sharing
//! `watch_dir` with the CIF zip. Off unless `CORPUS_INGEST_ENABLED=true`.
//! See docs/superpowers/specs/2026-09-28-corpus-sftp-ingest-design.md and
//! docs/superpowers/specs/2026-10-14-corpus-nlc-xml-format-design.md.
//!
//! CORPUS arrives in two formats, two versions of NWR's CORPUS API that can
//! both be subscribed to at once, each switched on its own:
//!
//! - **JSON** (`CORPUS_JSON_ENABLED`): `CORPUSExtract.json.gz` (gzipped
//!   `{"TIPLOCDATA":[{NLC, STANOX, TIPLOC, 3ALPHA, UIC, NLCDESC,
//!   NLCDESC16}, ...]}`, every location, ~56,000 rows), matched by
//!   `CORPUS_FILE_PATTERN` and parsed here ([`parse_extract`]);
//! - **XML** (`CORPUS_XML_ENABLED`): dated full snapshots such as
//!   `NLC20261009.xml.gz` (only TIPLOC-bearing locations, ~12,100 rows),
//!   matched by the `NLC` prefix and `.xml.gz` extension
//!   (`pattern::is_nlc_xml_name`) and parsed in `corpus_xml.rs`.
//!
//! Both become the same [`CorpusLocation`] rows and the same table. Each
//! cycle, [`run_corpus_cycle`]:
//!
//! 1. reports any `NLC*` file that is not `.xml.gz` as format drift (warn,
//!    `schedule_feed_corpus_format_drift_*`, `DistantSignalCorpusFormatDrift`)
//!    and never touches it;
//! 2. waits until every CORPUS candidate is stable;
//! 3. chooses the file to load (see "Choosing the file to load");
//! 4. gunzips (capped) and parses it, normalising every value -- a file
//!    that is not gzip, not its format, has a row with no NLC or fewer than
//!    its format's minimum rows is REJECTED: counted, logged and moved to
//!    `storage_dir/corpus/rejected/`, never loaded;
//! 5. hands the whole set to the sink (`sink.rs`), which replaces
//!    `corpus_locations` in Postgres in one transaction;
//! 6. on success moves the file to `storage_dir/corpus/`, and every other
//!    candidate to `storage_dir/corpus/superseded/` unloaded, keeping the
//!    newest `CORPUS_RETENTION_KEEP` of each, so files never pile up in
//!    `watch_dir`;
//! 7. optionally compares the file with the newest one of the other format
//!    (`corpus_formats.rs`), the migration signal.
//!
//! ## Choosing the file to load
//!
//! Files are ordered by their mtime as this process's scan records it
//! (with the name as a tie-break), never by the date in an XML snapshot's
//! name: that date is logged and exported
//! (`schedule_feed_corpus_xml_file_date_seconds`), with a warning when it
//! goes backwards, but drives nothing. `CORPUS_FORMAT_PRECEDENCE` then
//! picks:
//!
//! - `newest`: the newest file of either format;
//! - `json` (the default) or `xml`: the newest file of the preferred
//!   format. A file of the other format is loaded only while no file of the
//!   preferred one has been loaded within `CORPUS_PREFERRED_GRACE_DAYS`
//!   (judged from the newest file in `storage_dir/corpus/`, so it survives
//!   restarts); otherwise it is still parsed (a broken one is rejected and
//!   alerts), compared, and moved to `superseded/` unloaded.
//!
//! Both moves re-check the file's `(mtime, size)` first, so a re-upload
//! that starts mid-cycle is never moved away half-written. A failed load
//! leaves the file where it is and the next cycle tries again; loading the
//! same file twice is idempotent, so a restart at any point
//! is safe without persisted state.
//!
//! A cycle that cannot scan `watch_dir`, or read or load a stable file,
//! counts [`FAILURES_METRIC`] by `stage`. That backs the prompt
//! `DistantSignalCorpusLoadFailing` alert, kept apart from "no new file
//! yet": CORPUS arrives every couple of weeks to monthly, so a quiet
//! `watch_dir` is normal and only `DistantSignalCorpusStale` (weeks)
//! covers it. Format drift (an `NLC*` file that is not `.xml.gz`) counts
//! as `stage="drift"` on every cycle the file is present, which backs
//! `DistantSignalCorpusFormatDrift`.

use std::collections::{BTreeSet, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::config::{CorpusArgs, FormatPrecedence};
use crate::delivery::delivery_dir_name;
use crate::pattern::{NLC_PREFIX, Routing, is_nlc_drift_name, is_nlc_xml_name};
use crate::scan::{DirSnapshot, StabilityTracker, scan_incoming};
use crate::sink::{CorpusLoadRequest, IngestSink};

/// Counts rejected CORPUS files, by `format`; `DistantSignalCorpusRejected`
/// reads it.
pub(crate) const REJECTED_METRIC: &str = "schedule_feed_corpus_rejected_total";
const LAST_LOAD_METRIC: &str = "schedule_feed_corpus_last_load_delivered_at_seconds";
const ROWS_METRIC: &str = "schedule_feed_corpus_rows";
/// The last load's mtime per `format`.
const FORMAT_LAST_LOAD_METRIC: &str = "schedule_feed_corpus_format_last_load_delivered_at_seconds";
/// How many `NLC*` files that are not `.xml.gz` sit in `watch_dir` now
/// (for dashboards; the alert reads [`FAILURES_METRIC`]'s `drift`).
pub(crate) const DRIFT_FILES_METRIC: &str = "schedule_feed_corpus_format_drift_files";
/// The date in the last processed XML snapshot's name (metadata only).
const XML_FILE_DATE_METRIC: &str = "schedule_feed_corpus_xml_file_date_seconds";
/// Counts CORPUS cycles that failed to scan `watch_dir` (`stage="scan"`),
/// read a stable file (`"read"`) or load it into Postgres (`"load"`). Each
/// is retried next cycle. `DistantSignalCorpusLoadFailing` reads those.
/// `"drift"` counts each cycle that sees an `NLC*` file that is not
/// `.xml.gz` (never loaded); `DistantSignalCorpusFormatDrift` reads it.
const FAILURES_METRIC: &str = "schedule_feed_corpus_failures_total";
/// Every `stage` label value of [`FAILURES_METRIC`].
const FAILURE_STAGES: [&str; 4] = ["scan", "read", "load", "drift"];

/// Subdirectory of `storage_dir` that loaded files are moved into.
const ARCHIVE_DIR: &str = "corpus";
/// Subdirectory of [`ARCHIVE_DIR`] for rejected files.
const REJECTED_DIR: &str = "rejected";
/// Subdirectory of [`ARCHIVE_DIR`] for files that were not loaded because
/// another file won (a newer one, or the preferred format).
const SUPERSEDED_DIR: &str = "superseded";

/// One normalised CORPUS location: `ds_store`'s own type (plan 2d.1), the
/// one the direct sink writes.
pub(crate) use ds_store::corpus::CorpusLocation;

/// A CORPUS delivery format (one CORPUS API version).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum CorpusFormat {
    /// `CORPUSExtract.json.gz`, the older API version.
    Json,
    /// `NLC*.xml.gz`, the newer API version.
    Xml,
}

impl CorpusFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Xml => "xml",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Json => Self::Xml,
            Self::Xml => Self::Json,
        }
    }

    /// The format of a file in the archive, from its original name (only
    /// CORPUS files are ever archived, and only XML ones are `NLC*.xml.gz`).
    fn of_archived(original_name: &str) -> Self {
        if is_nlc_xml_name(original_name) {
            Self::Xml
        } else {
            Self::Json
        }
    }
}

/// Why a file can never be loaded. Its bytes cannot change without its
/// `(mtime, size)` changing, so it is moved aside rather than retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Rejected(pub String);

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a handled file goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Dest {
    Loaded,
    Rejected,
}

/// The archive directories under `storage_dir`.
struct Dirs {
    loaded: PathBuf,
    rejected: PathBuf,
    superseded: PathBuf,
}

impl Dirs {
    fn new(storage_dir: &Path) -> Self {
        let loaded = storage_dir.join(ARCHIVE_DIR);
        Self {
            rejected: loaded.join(REJECTED_DIR),
            superseded: loaded.join(SUPERSEDED_DIR),
            loaded,
        }
    }

    fn of(&self, dest: Dest) -> &Path {
        match dest {
            Dest::Loaded => &self.loaded,
            Dest::Rejected => &self.rejected,
        }
    }
}

/// Process-lifetime state of the CORPUS pipeline. Nothing here needs to
/// survive a restart (see the module doc).
#[derive(Debug, Default)]
pub(crate) struct CorpusState {
    tracker: StabilityTracker,
    known_stable: HashSet<String>,
    /// The last file this process loaded or rejected, as `(name, mtime,
    /// size)` and where it goes. Only matters when the move out of
    /// `watch_dir` failed: it stops the same file being loaded again (or
    /// re-counted as rejected) every cycle while the move is retried.
    handled: Option<(String, SystemTime, u64, Dest)>,
    /// `NLC*` files that are not `.xml.gz`, already reported.
    drift_seen: HashSet<String>,
}

impl CorpusState {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

/// Registers the metrics at 0 so the alerts' `increase()` sees the first
/// rejection, failure or drift.
pub(crate) fn register_metrics() {
    for format in [CorpusFormat::Json, CorpusFormat::Xml] {
        metrics::counter!(common::metrics::metric_name(REJECTED_METRIC), "format" => format.as_str())
            .increment(0);
    }
    for stage in FAILURE_STAGES {
        metrics::counter!(common::metrics::metric_name(FAILURES_METRIC), "stage" => stage)
            .increment(0);
    }
    metrics::gauge!(common::metrics::metric_name(DRIFT_FILES_METRIC)).set(0.0);
}

/// Counts one failed CORPUS cycle at `stage` (one of [`FAILURE_STAGES`]).
fn count_failure(stage: &'static str) {
    debug_assert!(FAILURE_STAGES.contains(&stage), "unknown stage {stage}");
    metrics::counter!(common::metrics::metric_name(FAILURES_METRIC), "stage" => stage).increment(1);
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
pub(crate) fn pad_digits(code: String, width: usize) -> String {
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

/// One parsed file of either format.
struct Parsed {
    locations: Vec<CorpusLocation>,
    /// XML item fields the parser does not know (ignored).
    unknown_fields: BTreeSet<String>,
}

/// Gunzips (capped) and parses `bytes` as `format`.
fn parse_file(format: CorpusFormat, bytes: &[u8], args: &CorpusArgs) -> Result<Parsed, Rejected> {
    let max = args.corpus_max_decompressed_bytes;
    match format {
        CorpusFormat::Json => decompress(bytes, max)
            .and_then(|json| parse_extract(&json, args.corpus_min_rows))
            .map(|locations| Parsed {
                locations,
                unknown_fields: BTreeSet::new(),
            }),
        CorpusFormat::Xml => crate::corpus_xml::parse_gzipped(bytes, max, args.corpus_xml_min_rows)
            .map(|xml| Parsed {
                locations: xml.locations,
                unknown_fields: xml.unknown_fields,
            }),
    }
}

/// The date in an XML snapshot's name (`NLC20261009.xml.gz`), if it has
/// one. Metadata only: it never orders or selects files.
pub(crate) fn nlc_file_date(name: &str) -> Option<NaiveDate> {
    let digits = name.get(NLC_PREFIX.len()..NLC_PREFIX.len() + 8)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    NaiveDate::parse_from_str(digits, "%Y%m%d").ok()
}

/// One CORPUS file in `watch_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Candidate {
    name: String,
    mtime: SystemTime,
    len: u64,
    format: CorpusFormat,
}

/// Every CORPUS candidate in `snapshot`, oldest first (by mtime, then name).
fn candidates(snapshot: &DirSnapshot, routing: &Routing) -> Vec<Candidate> {
    let mut found: Vec<Candidate> = snapshot
        .0
        .iter()
        .filter_map(|(name, &(mtime, len))| {
            routing.corpus_format(name).map(|format| Candidate {
                name: name.clone(),
                mtime,
                len,
                format,
            })
        })
        .collect();
    found.sort_by(|a, b| a.mtime.cmp(&b.mtime).then_with(|| a.name.cmp(&b.name)));
    found
}

/// Reports every `NLC*` file in `snapshot` that is not `.xml.gz`: the
/// gauge and one `drift` failure every cycle one is present, and a warning
/// the first time each name is seen. Such a file is never loaded, moved or
/// deleted.
#[expect(
    clippy::cast_precision_loss,
    reason = "a gauge takes f64, and a file count stays far below 2^52"
)]
fn report_drift(snapshot: &DirSnapshot, state: &mut CorpusState) {
    let mut drift: Vec<&String> = snapshot
        .0
        .keys()
        .filter(|name| is_nlc_drift_name(name))
        .collect();
    drift.sort();
    metrics::gauge!(common::metrics::metric_name(DRIFT_FILES_METRIC)).set(drift.len() as f64);
    if !drift.is_empty() {
        count_failure("drift");
    }
    for name in drift {
        if state.drift_seen.insert(name.clone()) {
            tracing::warn!(
                file = %name,
                "a file named like the NLC CORPUS feed is not .xml.gz: the delivery format may have drifted; it is ignored, not loaded"
            );
        }
    }
    state
        .drift_seen
        .retain(|name| snapshot.0.contains_key(name));
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
        && name.is_char_boundary(16)
        && name.as_bytes()[16] == b'-'
        && crate::delivery::is_delivery_dir_name(&name[..16])
}

/// Every archived file in `dir`, as `(archived name, path)`, sorted (so
/// oldest first).
fn archived_files(dir: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    let read_dir = match std::fs::read_dir(dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
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
    Ok(files)
}

/// The newest archived file of `format` in any of `dirs`, as `(archived
/// name, path)`. Archived names start with the file's mtime, so the
/// greatest name is the newest.
fn newest_archived(dirs: &[&Path], format: CorpusFormat) -> Option<(String, PathBuf)> {
    let mut newest: Option<(String, PathBuf)> = None;
    for dir in dirs {
        let files = match archived_files(dir) {
            Ok(files) => files,
            Err(err) => {
                tracing::warn!(error = %err, dir = %dir.display(), "failed to list the CORPUS archive");
                continue;
            }
        };
        if let Some(found) = files
            .into_iter()
            .rev()
            .find(|(name, _)| CorpusFormat::of_archived(&name[17..]) == format)
            && newest.as_ref().is_none_or(|(n, _)| found.0 > *n)
        {
            newest = Some(found);
        }
    }
    newest
}

/// Keeps the `keep` newest archived files in `dir`, removing older ones.
fn prune_archive(dir: &Path, keep: u32) -> std::io::Result<()> {
    let files = archived_files(dir)?;
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

/// What to do this cycle.
#[derive(Debug, PartialEq, Eq)]
enum Choice {
    /// Load this file; every other candidate is superseded.
    Load(Candidate),
    /// Every candidate is of the non-preferred format and the preferred one
    /// was loaded recently: check and compare the newest, load none.
    NotPreferred(CorpusFormat),
}

/// Applies `CORPUS_FORMAT_PRECEDENCE` to `stable` (oldest first, not empty).
fn choose(stable: &[Candidate], args: &CorpusArgs, dirs: &Dirs, now: SystemTime) -> Option<Choice> {
    let newest_of = |format| stable.iter().rev().find(|c| c.format == format).cloned();
    let preferred = match args.corpus_format_precedence {
        FormatPrecedence::Newest => return stable.last().cloned().map(Choice::Load),
        FormatPrecedence::Json => CorpusFormat::Json,
        FormatPrecedence::Xml => CorpusFormat::Xml,
    };
    if let Some(file) = newest_of(preferred) {
        return Some(Choice::Load(file));
    }
    let other = newest_of(preferred.other())?;
    let grace = Duration::from_secs(u64::from(args.corpus_preferred_grace_days) * 86_400);
    let recently_loaded = newest_archived(&[&dirs.loaded], preferred).is_some_and(|(_, path)| {
        std::fs::metadata(&path)
            .and_then(|m| m.modified())
            // A future mtime counts as recent.
            .is_ok_and(|mtime| now.duration_since(mtime).map_or(true, |age| age < grace))
    });
    Some(if recently_loaded {
        Choice::NotPreferred(preferred)
    } else {
        Choice::Load(other)
    })
}

/// A candidate's bytes and their parse.
enum ReadOutcome {
    /// Read whole and parsed (or rejected).
    Parsed(crate::audit::DeliveredFile, Result<Parsed, Rejected>),
    /// Changed while being read, or unreadable: retried next cycle.
    Retry,
}

fn read_and_parse(watch_dir: &Path, file: &Candidate, args: &CorpusArgs) -> ReadOutcome {
    match std::fs::read(watch_dir.join(&file.name)) {
        Ok(bytes) if bytes.len() as u64 == file.len => ReadOutcome::Parsed(
            crate::audit::DeliveredFile::from_bytes(&file.name, &bytes),
            parse_file(file.format, &bytes, args),
        ),
        Ok(_) => {
            tracing::info!(file = %file.name, "CORPUS file changed while being read; retrying next cycle");
            ReadOutcome::Retry
        }
        Err(err) => {
            tracing::error!(error = %err, file = %file.name, "failed to read CORPUS file; retrying next cycle");
            count_failure("read");
            ReadOutcome::Retry
        }
    }
}

/// Counts, logs and archives a file that can never be loaded.
fn reject(
    watch_dir: &Path,
    dirs: &Dirs,
    keep: u32,
    file: &Candidate,
    delivered: &crate::audit::DeliveredFile,
    rejected: &Rejected,
    state: &mut CorpusState,
) {
    tracing::error!(file = %file.name, format = file.format.as_str(), reason = %rejected, "rejecting CORPUS file; it is not loaded and is moved to the rejected archive");
    crate::audit::decision(
        delivered,
        DateTime::<Utc>::from(file.mtime),
        crate::audit::Outcome::CorpusRejected,
        Some(&rejected.0),
    );
    metrics::counter!(common::metrics::metric_name(REJECTED_METRIC), "format" => file.format.as_str())
        .increment(1);
    state.handled = Some((file.name.clone(), file.mtime, file.len, Dest::Rejected));
    archive(
        watch_dir,
        &file.name,
        (file.mtime, file.len),
        dirs.of(Dest::Rejected),
        keep,
    );
}

/// Logs what a parse noticed about an XML snapshot: unknown fields, and
/// the date in its name (exported, and warned about when it is older than
/// the newest archived snapshot's).
#[expect(
    clippy::cast_precision_loss,
    reason = "a gauge takes f64, and a timestamp stays far below 2^52"
)]
fn note_xml(file: &Candidate, parsed: &Parsed, dirs: &Dirs) {
    if file.format != CorpusFormat::Xml {
        return;
    }
    if !parsed.unknown_fields.is_empty() {
        tracing::warn!(file = %file.name, fields = ?parsed.unknown_fields, "the NLC CORPUS XML has fields this parser does not know; they are ignored");
    }
    let Some(date) = nlc_file_date(&file.name) else {
        tracing::warn!(file = %file.name, "the NLC CORPUS XML's name carries no YYYYMMDD date");
        return;
    };
    if let Some(midnight) = date.and_hms_opt(0, 0, 0) {
        metrics::gauge!(common::metrics::metric_name(XML_FILE_DATE_METRIC))
            .set(midnight.and_utc().timestamp() as f64);
    }
    if let Some((archived, _)) =
        newest_archived(&[&dirs.loaded, &dirs.superseded], CorpusFormat::Xml)
        && let Some(previous) = nlc_file_date(&archived[17..])
        && date < previous
    {
        tracing::warn!(
            file = %file.name,
            file_date = %date,
            previous_file = %&archived[17..],
            previous_file_date = %previous,
            "the NLC CORPUS XML received last is dated earlier than the previous one; it is still processed in receipt order"
        );
    }
}

/// Compares `rows` (parsed from `file`) with the newest file of the other
/// format: a candidate in `watch_dir`, else the newest archived one. Logs
/// and exports the result (`corpus_formats.rs`); never fails the cycle.
fn compare_with_other(
    file: &Candidate,
    rows: &[CorpusLocation],
    all: &[Candidate],
    watch_dir: &Path,
    dirs: &Dirs,
    args: &CorpusArgs,
) {
    let other = file.format.other();
    let source = all
        .iter()
        .rev()
        .find(|c| c.format == other)
        .map(|c| (c.name.clone(), watch_dir.join(&c.name)))
        .or_else(|| {
            newest_archived(&[&dirs.loaded, &dirs.superseded], other)
                .map(|(name, path)| (name[17..].to_string(), path))
        });
    let Some((other_name, path)) = source else {
        tracing::info!(file = %file.name, "no {} CORPUS file to compare with", other.as_str());
        return;
    };
    let other_rows = match std::fs::read(&path)
        .map_err(|err| Rejected(err.to_string()))
        .and_then(|bytes| parse_file(other, &bytes, args))
    {
        Ok(parsed) => parsed.locations,
        Err(err) => {
            tracing::warn!(file = %other_name, reason = %err, "could not read the other CORPUS format's file for the comparison");
            return;
        }
    };
    let (json, xml, json_name, xml_name) = match file.format {
        CorpusFormat::Json => (
            rows,
            other_rows.as_slice(),
            file.name.as_str(),
            other_name.as_str(),
        ),
        CorpusFormat::Xml => (
            other_rows.as_slice(),
            rows,
            other_name.as_str(),
            file.name.as_str(),
        ),
    };
    let comparison = crate::corpus_formats::compare(json, xml);
    crate::corpus_formats::report(&comparison, json_name, xml_name);
}

/// Archives every candidate but `except` unloaded, as superseded.
fn supersede(
    all: &[Candidate],
    except: Option<&str>,
    reason: &str,
    watch_dir: &Path,
    dirs: &Dirs,
    keep: u32,
) {
    for old in all.iter().filter(|c| Some(c.name.as_str()) != except) {
        tracing::info!(file = %old.name, reason = %reason, "archiving a CORPUS file without loading it");
        match crate::audit::DeliveredFile::hash_file(&old.name, &watch_dir.join(&old.name)) {
            Ok(superseded) => crate::audit::decision(
                &superseded,
                DateTime::<Utc>::from(old.mtime),
                crate::audit::Outcome::CorpusRejected,
                Some(reason),
            ),
            Err(err) => {
                tracing::warn!(error = %err, file = %old.name, "failed to hash a superseded CORPUS file for its audit line");
            }
        }
        archive(
            watch_dir,
            &old.name,
            (old.mtime, old.len),
            &dirs.superseded,
            keep,
        );
    }
}

/// One poll interval of the CORPUS pipeline (see the module doc). Returns
/// `Err` only when `watch_dir` itself cannot be read; every per-file
/// problem is logged and handled.
#[expect(
    clippy::cast_precision_loss,
    clippy::too_many_lines,
    reason = "metric gauges take f64, and these counts and timestamps stay far below 2^52; one linear decision per cycle"
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
    let snapshot = scan_incoming(watch_dir).inspect_err(|_| count_failure("scan"))?;
    let just_stabilized = state.tracker.observe(&snapshot, stability_cycles);
    state.known_stable.extend(just_stabilized);
    state
        .known_stable
        .retain(|name| snapshot.0.contains_key(name));
    report_drift(&snapshot, state);

    let all = candidates(&snapshot, routing);
    if all.is_empty() {
        return Ok(());
    }
    // Decide only once every candidate is stable, so a file still being
    // uploaded can neither be chosen nor be passed over for an older one.
    if let Some(unstable) = all.iter().find(|c| !state.known_stable.contains(&c.name)) {
        tracing::info!(file = %unstable.name, "CORPUS file present but not yet stable");
        return Ok(());
    }
    let dirs = Dirs::new(storage_dir);
    let keep = args.corpus_retention_keep;

    if let Some((name, mtime, len, dest)) = &state.handled
        && all
            .iter()
            .any(|c| (&c.name, &c.mtime, &c.len) == (name, mtime, len))
    {
        // Loaded or rejected already; only the move out of watch_dir
        // failed. Retry just that.
        archive(watch_dir, name, (*mtime, *len), dirs.of(*dest), keep);
        return Ok(());
    }

    let Some(choice) = choose(&all, args, &dirs, SystemTime::now()) else {
        return Ok(());
    };
    let winner = match choice {
        Choice::Load(winner) => winner,
        Choice::NotPreferred(preferred) => {
            // Never loaded, but still checked (a broken file of either
            // format alerts) and compared: the migration signal.
            let Some(newest) = all.last() else {
                return Ok(());
            };
            match read_and_parse(watch_dir, newest, args) {
                ReadOutcome::Retry => return Ok(()),
                ReadOutcome::Parsed(delivered, Err(rejected)) => {
                    reject(watch_dir, &dirs, keep, newest, &delivered, &rejected, state);
                    return Ok(());
                }
                ReadOutcome::Parsed(_, Ok(parsed)) => {
                    note_xml(newest, &parsed, &dirs);
                    if args.corpus_compare_formats {
                        compare_with_other(newest, &parsed.locations, &all, watch_dir, &dirs, args);
                    }
                }
            }
            let reason = format!(
                "not loaded: CORPUS_FORMAT_PRECEDENCE prefers {} and one was loaded within CORPUS_PREFERRED_GRACE_DAYS ({})",
                preferred.as_str(),
                args.corpus_preferred_grace_days
            );
            supersede(&all, None, &reason, watch_dir, &dirs, keep);
            return Ok(());
        }
    };

    let delivered_at = DateTime::<Utc>::from(winner.mtime);
    let (delivered, parsed) = match read_and_parse(watch_dir, &winner, args) {
        ReadOutcome::Retry => return Ok(()),
        ReadOutcome::Parsed(delivered, Err(rejected)) => {
            reject(
                watch_dir, &dirs, keep, &winner, &delivered, &rejected, state,
            );
            return Ok(());
        }
        ReadOutcome::Parsed(delivered, Ok(parsed)) => (delivered, parsed),
    };
    note_xml(&winner, &parsed, &dirs);
    let locations = parsed.locations;

    let request = CorpusLoadRequest {
        delivered_at,
        source_file: &winner.name,
        locations: &locations,
        source_bytes: delivered.bytes,
        sha256: &delivered.sha256,
    };
    if let Err(err) = sink.load_corpus(&request).await {
        // Every failure is retried, a rejection too (as before the sink):
        // the file stays in watch_dir.
        tracing::error!(error = %err, file = %winner.name, "CORPUS load failed; retrying next cycle");
        count_failure("load");
        return Ok(());
    }
    tracing::info!(
        file = %winner.name,
        format = winner.format.as_str(),
        file_date = ?nlc_file_date(&winner.name).filter(|_| winner.format == CorpusFormat::Xml),
        delivered_at = %delivered_at,
        rows = locations.len(),
        "loaded CORPUS delivery"
    );
    crate::audit::decision(
        &delivered,
        delivered_at,
        crate::audit::Outcome::Accepted,
        None,
    );
    metrics::gauge!(common::metrics::metric_name(LAST_LOAD_METRIC))
        .set(delivered_at.timestamp() as f64);
    metrics::gauge!(common::metrics::metric_name(FORMAT_LAST_LOAD_METRIC), "format" => winner.format.as_str())
        .set(delivered_at.timestamp() as f64);
    metrics::gauge!(common::metrics::metric_name(ROWS_METRIC)).set(locations.len() as f64);
    state.handled = Some((winner.name.clone(), winner.mtime, winner.len, Dest::Loaded));

    if args.corpus_compare_formats {
        compare_with_other(&winner, &locations, &all, watch_dir, &dirs, args);
    }
    archive(
        watch_dir,
        &winner.name,
        (winner.mtime, winner.len),
        dirs.of(Dest::Loaded),
        keep,
    );

    // Every other candidate -- older, or of the other format -- is
    // superseded by the one just loaded: archive them unloaded, so a later
    // cycle can never load older (or non-preferred) data over it.
    let reason = format!("superseded by {} (loaded); not loaded", winner.name);
    supersede(&all, Some(&winner.name), &reason, watch_dir, &dirs, keep);
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use crate::sink::tests::{Answer, FakeSink};

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

    fn args() -> CorpusArgs {
        CorpusArgs {
            corpus_ingest_enabled: true,
            corpus_file_pattern: crate::config::DEFAULT_CORPUS_FILE_PATTERN.to_string(),
            corpus_max_decompressed_bytes: 1024 * 1024,
            corpus_min_rows: 1,
            corpus_retention_keep: 3,
            corpus_json_enabled: true,
            corpus_xml_enabled: true,
            corpus_xml_min_rows: 1,
            corpus_format_precedence: FormatPrecedence::Json,
            corpus_preferred_grace_days: 35,
            corpus_compare_formats: true,
        }
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

    /// The whole path: the stable extract is loaded once with normalised
    /// rows, then moved out of `watch_dir` into the archive; the SMART
    /// `.csv.gz` sharing its name is never touched.
    #[tokio::test]
    async fn a_stable_extract_is_loaded_once_and_archived() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let delivered = gzip(FIXTURE.as_bytes());
        std::fs::write(watch.path().join("CORPUSExtract.json.gz"), &delivered).unwrap();
        std::fs::write(
            watch.path().join("CORPUSExtract.csv.gz"),
            gzip(b"A2,B,0632"),
        )
        .unwrap();

        let args = args();
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args, &sink, &mut state, 4).await;
        drop(guard);

        // Provenance: the load and one audit line carry the file's hash.
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

        let loads = sink.corpus_loads();
        assert_eq!(loads.len(), 1, "loaded once");
        let body = &loads[0];
        assert_eq!(body["source_file"], "CORPUSExtract.json.gz");
        assert_eq!(body["sha256"], expected.sha256.as_str());
        assert_eq!(body["source_bytes"], expected.bytes);
        assert_eq!(body["locations"].as_array().unwrap().len(), 8);
        assert_eq!(body["locations"][0]["nlc"], "000700");
        assert_eq!(body["locations"][1]["tiploc"], serde_json::Value::Null);
    }

    /// A failing database leaves the file in place for the next cycle.
    #[tokio::test]
    async fn a_failed_load_leaves_the_file_for_the_next_cycle() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        std::fs::write(
            watch.path().join("CORPUSExtract.json.gz"),
            gzip(FIXTURE.as_bytes()),
        )
        .unwrap();
        let args = args();
        let sink = FakeSink::new(Answer::Transient);
        let mut state = CorpusState::new();
        run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;

        assert_eq!(names(watch.path()), ["CORPUSExtract.json.gz"]);
        assert!(names(storage.path()).is_empty());
    }

    /// An extract that is not CORPUS is never loaded: it is moved to the
    /// rejected archive (the sink fails every write here, so a load attempt
    /// would have left the file in place instead).
    #[tokio::test]
    async fn a_malformed_extract_is_rejected_and_moved_aside() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        std::fs::write(
            watch.path().join("CORPUSExtract.json.gz"),
            gzip(b"TD,STEPTYPE,FROMBERTH\nA2,B,0632\n"),
        )
        .unwrap();
        let args = args();
        let sink = FakeSink::new(Answer::Transient);
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
        assert!(sink.corpus_loads().is_empty());
        let rejected = names(&storage.path().join(ARCHIVE_DIR).join(REJECTED_DIR));
        assert_eq!(rejected.len(), 1);
        assert!(
            rejected[0].ends_with("-CORPUSExtract.json.gz"),
            "{rejected:?}"
        );
    }

    /// The synthetic NLC XML fixture (`tests/fixtures/SOURCE.md`).
    const XML_FIXTURE: &str = include_str!("../tests/fixtures/nlc_corpus_synthetic.xml");

    /// Writes `watch/name` with an mtime `age_secs` in the past.
    fn put(dir: &Path, name: &str, bytes: &[u8], age_secs: u64) {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(age_secs))
            .unwrap();
    }

    /// Every captured log line whose message contains `needle`.
    fn logged(logs: &crate::audit::tests::Capture, needle: &str) -> Vec<serde_json::Value> {
        logs.lines()
            .into_iter()
            .filter(|line| line["message"].as_str().is_some_and(|m| m.contains(needle)))
            .collect()
    }

    fn loaded_files(sink: &FakeSink) -> Vec<String> {
        sink.corpus_loads()
            .iter()
            .map(|load| load["source_file"].as_str().unwrap().to_string())
            .collect()
    }

    /// The whole cycle on real files (never committed), with the shipped
    /// defaults but for the precedence: `CORPUS_XML_SAMPLE=/path/NLC*.xml.gz
    /// [CORPUS_EXTRACT_SAMPLE=/path/CORPUSExtract.json.gz] cargo test -p
    /// schedule-ingest -- --ignored --nocapture real_files_through_the_cycle`.
    #[tokio::test]
    #[ignore = "needs CORPUS_XML_SAMPLE (and optionally CORPUS_EXTRACT_SAMPLE) pointing at real files"]
    async fn real_files_through_the_cycle() {
        let Ok(xml_path) = std::env::var("CORPUS_XML_SAMPLE") else {
            return;
        };
        let json_path = std::env::var("CORPUS_EXTRACT_SAMPLE").ok();
        for precedence in [
            FormatPrecedence::Json,
            FormatPrecedence::Xml,
            FormatPrecedence::Newest,
        ] {
            let watch = tempfile::tempdir().unwrap();
            let storage = tempfile::tempdir().unwrap();
            let xml_name = Path::new(&xml_path).file_name().unwrap().to_str().unwrap();
            put(
                watch.path(),
                xml_name,
                &std::fs::read(&xml_path).unwrap(),
                60,
            );
            if let Some(json_path) = &json_path {
                put(
                    watch.path(),
                    "CORPUSExtract.json.gz",
                    &std::fs::read(json_path).unwrap(),
                    120,
                );
            }
            let args = CorpusArgs {
                corpus_max_decompressed_bytes: 256 * 1024 * 1024,
                corpus_min_rows: 10_000,
                corpus_xml_min_rows: 10_000,
                corpus_format_precedence: precedence,
                ..args()
            };
            let sink = FakeSink::new(Answer::Accept);
            let mut state = CorpusState::new();
            let (guard, logs) = crate::audit::tests::capture_default();
            run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;
            drop(guard);
            let loads = sink.corpus_loads();
            assert_eq!(loads.len(), 1, "{precedence:?}");
            println!(
                "{precedence:?}: loaded {} with {} rows; superseded {:?}; compared {}",
                loads[0]["source_file"],
                loads[0]["locations"].as_array().unwrap().len(),
                names(&storage.path().join(ARCHIVE_DIR).join(SUPERSEDED_DIR)),
                logged(&logs, "compared the CORPUS JSON extract")
                    .first()
                    .map_or_else(String::new, ToString::to_string),
            );
        }
    }

    #[test]
    fn the_date_in_an_nlc_name_is_read_as_metadata() {
        assert_eq!(
            nlc_file_date("NLC20261009.xml.gz"),
            NaiveDate::from_ymd_opt(2026, 10, 9)
        );
        assert_eq!(
            nlc_file_date("nlc20261009.XML.GZ"),
            NaiveDate::from_ymd_opt(2026, 10, 9)
        );
        assert_eq!(nlc_file_date("NLC.xml.gz"), None);
        assert_eq!(nlc_file_date("NLC20261399.xml.gz"), None);
        assert_eq!(nlc_file_date("NLCabcdefgh.xml.gz"), None);
    }

    /// The new format end to end: loaded into the same model, archived.
    #[tokio::test]
    async fn an_nlc_xml_snapshot_is_loaded_and_archived() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let delivered = gzip(XML_FIXTURE.as_bytes());
        put(watch.path(), "NLC20261009.xml.gz", &delivered, 60);
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args(), &sink, &mut state, 4).await;
        drop(guard);

        let loads = sink.corpus_loads();
        assert_eq!(loads.len(), 1, "loaded once");
        assert_eq!(loads[0]["source_file"], "NLC20261009.xml.gz");
        let expected = crate::audit::DeliveredFile::from_bytes("NLC20261009.xml.gz", &delivered);
        assert_eq!(loads[0]["sha256"], expected.sha256.as_str());
        let locations = loads[0]["locations"].as_array().unwrap();
        assert_eq!(locations.len(), 6);
        assert_eq!(locations[0]["nlc"], "370100");
        assert_eq!(locations[0]["tiploc"], "QZAPLTN");
        assert_eq!(locations[0]["crs"], "QZA");
        assert_eq!(locations[0]["uic"], "37010");
        assert_eq!(locations[1]["nlc_desc"], "BRAMBLE & THORN");
        assert_eq!(locations[2]["crs"], serde_json::Value::Null);

        assert!(names(watch.path()).is_empty());
        let archived = names(&storage.path().join(ARCHIVE_DIR));
        assert_eq!(archived.len(), 1);
        assert!(archived[0].ends_with("-NLC20261009.xml.gz"), "{archived:?}");
        let audit = logs.audit_lines();
        assert_eq!(audit.len(), 1, "{audit:?}");
        assert_eq!(audit[0]["outcome"], "accepted");
        // Nothing of the other format to compare with: said, not failed.
        assert_eq!(
            logged(&logs, "no json CORPUS file to compare with").len(),
            1
        );
    }

    /// An `NLC*` file that is not `.xml.gz` is never loaded, moved or
    /// deleted, and is reported once per file, not every cycle.
    #[tokio::test]
    async fn a_drifted_nlc_file_is_reported_once_and_never_loaded() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        put(watch.path(), "NLC20261009.xml", XML_FIXTURE.as_bytes(), 60);
        put(watch.path(), "NLC20261009.zip", b"PK", 60);
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args(), &sink, &mut state, 4).await;
        drop(guard);

        assert!(sink.corpus_loads().is_empty());
        assert_eq!(names(watch.path()), ["NLC20261009.xml", "NLC20261009.zip"]);
        assert!(names(storage.path()).is_empty());
        let warnings = logged(&logs, "format may have drifted");
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings.iter().all(|w| w["level"] == "WARN"));
        assert_eq!(state.drift_seen.len(), 2);

        // Removed, then back: reported again.
        std::fs::remove_file(watch.path().join("NLC20261009.zip")).unwrap();
        run(watch.path(), storage.path(), &args(), &sink, &mut state, 1).await;
        assert_eq!(state.drift_seen.len(), 1);
    }

    /// The default precedence (`json`): with both formats delivered, the
    /// JSON extract is loaded even though the XML snapshot is newer; the
    /// snapshot is compared with it, then archived unloaded.
    #[tokio::test]
    async fn with_json_preferred_a_newer_xml_is_compared_but_not_loaded() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        put(
            watch.path(),
            "CORPUSExtract.json.gz",
            &gzip(FIXTURE.as_bytes()),
            120,
        );
        put(
            watch.path(),
            "NLC20261009.xml.gz",
            &gzip(XML_FIXTURE.as_bytes()),
            60,
        );
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args(), &sink, &mut state, 4).await;
        drop(guard);

        assert_eq!(loaded_files(&sink), ["CORPUSExtract.json.gz"]);
        assert!(names(watch.path()).is_empty());
        let superseded = names(&storage.path().join(ARCHIVE_DIR).join(SUPERSEDED_DIR));
        assert_eq!(superseded.len(), 1);
        assert!(
            superseded[0].ends_with("-NLC20261009.xml.gz"),
            "{superseded:?}"
        );
        let compared = logged(&logs, "compared the CORPUS JSON extract");
        assert_eq!(compared.len(), 1, "{compared:?}");
        assert_eq!(compared[0]["json_rows"], 8);
        assert_eq!(compared[0]["xml_rows"], 6);
        assert_eq!(compared[0]["xml_file"], "NLC20261009.xml.gz");
        let audit = logs.audit_lines();
        assert_eq!(audit.len(), 2, "{audit:?}");
        assert!(
            audit[1]["reason"]
                .as_str()
                .is_some_and(|r| r.contains("superseded by CORPUSExtract.json.gz")),
            "{audit:?}"
        );
    }

    /// With `json` preferred, an XML snapshot is loaded while no JSON
    /// extract has been loaded within the grace period, and only then.
    #[tokio::test]
    async fn the_other_format_takes_over_only_when_the_preferred_one_is_stale() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        let args = args();

        // Nothing loaded yet: the XML snapshot is loaded.
        put(
            watch.path(),
            "NLC20261002.xml.gz",
            &gzip(XML_FIXTURE.as_bytes()),
            300,
        );
        run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;
        assert_eq!(loaded_files(&sink), ["NLC20261002.xml.gz"]);

        // A JSON extract arrives: preferred, so loaded.
        put(
            watch.path(),
            "CORPUSExtract.json.gz",
            &gzip(FIXTURE.as_bytes()),
            200,
        );
        run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;
        assert_eq!(loaded_files(&sink)[1], "CORPUSExtract.json.gz");

        // The next snapshot is checked and archived, not loaded.
        let (guard, logs) = crate::audit::tests::capture_default();
        put(
            watch.path(),
            "NLC20261009.xml.gz",
            &gzip(XML_FIXTURE.as_bytes()),
            100,
        );
        run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;
        drop(guard);
        assert_eq!(sink.corpus_loads().len(), 2);
        assert!(names(watch.path()).is_empty());
        assert!(
            names(&storage.path().join(ARCHIVE_DIR).join(SUPERSEDED_DIR))
                .iter()
                .any(|n| n.ends_with("-NLC20261009.xml.gz"))
        );
        assert_eq!(logged(&logs, "compared the CORPUS JSON extract").len(), 1);
        let audit = logs.audit_lines();
        assert!(
            audit[0]["reason"]
                .as_str()
                .is_some_and(|r| r.contains("prefers json")),
            "{audit:?}"
        );

        // The JSON extract was loaded longer ago than the grace period: the
        // next snapshot takes over.
        let (archived_json, path) =
            newest_archived(&[&storage.path().join(ARCHIVE_DIR)], CorpusFormat::Json).unwrap();
        assert!(archived_json.ends_with("-CORPUSExtract.json.gz"));
        std::fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(36 * 86_400))
            .unwrap();
        put(
            watch.path(),
            "NLC20261016.xml.gz",
            &gzip(XML_FIXTURE.as_bytes()),
            50,
        );
        run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;
        assert_eq!(loaded_files(&sink)[2], "NLC20261016.xml.gz");
    }

    /// `newest` loads the newest file of either format; `xml` prefers the
    /// snapshot over a newer extract.
    #[tokio::test]
    async fn newest_and_xml_precedence() {
        for (precedence, expected) in [
            (FormatPrecedence::Newest, "CORPUSExtract.json.gz"),
            (FormatPrecedence::Xml, "NLC20261009.xml.gz"),
        ] {
            let watch = tempfile::tempdir().unwrap();
            let storage = tempfile::tempdir().unwrap();
            put(
                watch.path(),
                "NLC20261009.xml.gz",
                &gzip(XML_FIXTURE.as_bytes()),
                120,
            );
            put(
                watch.path(),
                "CORPUSExtract.json.gz",
                &gzip(FIXTURE.as_bytes()),
                60,
            );
            let sink = FakeSink::new(Answer::Accept);
            let mut state = CorpusState::new();
            let args = CorpusArgs {
                corpus_format_precedence: precedence,
                ..args()
            };
            run(watch.path(), storage.path(), &args, &sink, &mut state, 3).await;
            assert_eq!(loaded_files(&sink), [expected], "{precedence:?}");
            assert!(names(watch.path()).is_empty(), "{precedence:?}");
            assert_eq!(
                names(&storage.path().join(ARCHIVE_DIR).join(SUPERSEDED_DIR)).len(),
                1,
                "{precedence:?}"
            );
        }
    }

    /// A switched-off format is a stray: never loaded, never moved.
    #[tokio::test]
    async fn a_disabled_format_is_left_alone() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        put(
            watch.path(),
            "NLC20261009.xml.gz",
            &gzip(XML_FIXTURE.as_bytes()),
            60,
        );
        let routing = Routing {
            corpus: Some(crate::pattern::CorpusRouting {
                json: Routing::with_corpus().corpus.unwrap().json,
                xml: false,
            }),
            ..Routing::defaults()
        };
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        for _ in 0..3 {
            run_corpus_cycle(
                &sink,
                watch.path(),
                storage.path(),
                &args(),
                &routing,
                2,
                &mut state,
            )
            .await
            .unwrap();
        }
        assert!(sink.corpus_loads().is_empty());
        assert_eq!(names(watch.path()), ["NLC20261009.xml.gz"]);
    }

    /// Files are processed in receipt (mtime) order, never by the date in
    /// their names; a date that goes backwards only warns.
    #[tokio::test]
    async fn receipt_order_wins_over_the_date_in_the_name() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let archive_dir = storage.path().join(ARCHIVE_DIR);
        std::fs::create_dir_all(&archive_dir).unwrap();
        std::fs::write(
            archive_dir.join("20261016T030000Z-NLC20261016.xml.gz"),
            gzip(XML_FIXTURE.as_bytes()),
        )
        .unwrap();
        put(
            watch.path(),
            "NLC20261020.xml.gz",
            &gzip(XML_FIXTURE.as_bytes()),
            120,
        );
        put(
            watch.path(),
            "NLC20261009.xml.gz",
            &gzip(XML_FIXTURE.as_bytes()),
            60,
        );
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args(), &sink, &mut state, 3).await;
        drop(guard);

        assert_eq!(loaded_files(&sink), ["NLC20261009.xml.gz"]);
        let superseded = names(&archive_dir.join(SUPERSEDED_DIR));
        assert_eq!(superseded.len(), 1);
        assert!(
            superseded[0].ends_with("-NLC20261020.xml.gz"),
            "{superseded:?}"
        );
        let warnings = logged(&logs, "dated earlier than the previous one");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert_eq!(warnings[0]["previous_file"], "NLC20261016.xml.gz");
    }

    /// A broken XML snapshot is rejected like a broken extract.
    #[tokio::test]
    async fn a_malformed_xml_snapshot_is_rejected_and_moved_aside() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        put(watch.path(), "NLC20261009.xml.gz", &gzip(b"<Other/>"), 60);
        let sink = FakeSink::new(Answer::Accept);
        let mut state = CorpusState::new();
        let (guard, logs) = crate::audit::tests::capture_default();
        run(watch.path(), storage.path(), &args(), &sink, &mut state, 3).await;
        drop(guard);

        assert!(sink.corpus_loads().is_empty());
        assert!(names(watch.path()).is_empty());
        let rejected = names(&storage.path().join(ARCHIVE_DIR).join(REJECTED_DIR));
        assert_eq!(rejected.len(), 1);
        let audit = logs.audit_lines();
        assert_eq!(audit[0]["outcome"], "corpus_rejected");
        assert!(
            audit[0]["reason"]
                .as_str()
                .is_some_and(|r| r.contains("not an NLC CORPUS XML file")),
            "{audit:?}"
        );
    }
}
