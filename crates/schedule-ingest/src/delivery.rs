//! Zip-delivery detection, mtime-based dedup, and extraction.
//!
//! Replaces this crate's original manifest/sequence-number design (see
//! `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`):
//! the real "raildata push API" delivers a single `.zip` archive, overwritten
//! in place on every new delivery, with no manifest and no sequence number.
//! This module owns the three pieces that replace `manifest.rs`'s old job:
//!
//! 1. [`find_zip_candidates`] -- generically matching any `.zip` file present
//!    in `watch_dir`, rather than a fixed filename.
//! 2. [`classify_delivery`] -- deciding "is this the same delivery I already
//!    ingested, or a new one" by comparing the candidate's own mtime against
//!    the last one this process successfully ingested, since there is no
//!    sequence number to compare instead.
//! 3. [`ensure_extracted`] -- unzipping a stable candidate's contents into
//!    a timestamp-named storage directory (see [`delivery_dir_name`]), so
//!    `schedule-reference` keeps reading real flat `RJTTFnnn*.txt` files off
//!    disk. Since PL-6 this is atomic: the zip is extracted into
//!    `storage_dir/.tmp-<dir_name>`, every file is fsynced, the
//!    `common::schedule_delivery::COMPLETE_MARKER` file is written last, and
//!    only then is the directory renamed into place. `schedule-reference`
//!    requires the marker, so it can never read a half-written MCA. And a
//!    directory that already has the marker is never extracted again
//!    (PL-13), however many times the api POST fails.
//!
//! Stability detection itself is unchanged -- `scan::StabilityTracker` is
//! reused as-is, just pointed at the zip candidate instead of a manifest
//! candidate.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Utc};
use common::schedule_delivery::{COMPLETE_MARKER, TEMP_DIR_PREFIX};

use crate::pattern::Routing;
use crate::scan::DirSnapshot;

/// Whether `name` has a `.zip` extension, case-insensitively. Deliberately
/// generic -- per the repo owner's explicit guidance, the delivery always
/// happens to be named `timetable_full.zip` today, but that is not
/// guaranteed to stay true, so this matches on shape (any `.zip` file), not
/// the exact filename.
pub fn is_zip_filename(name: &str) -> bool {
    name.len() > 4 && name.to_ascii_lowercase().ends_with(".zip")
}

/// Every `.zip`-shaped filename in `snapshot`, paired with its observed
/// mtime, sorted ascending by `(mtime, name)` -- so the caller can `pop()`
/// to get the most-recently-modified candidate, mirroring the old
/// `find_manifest_candidates`' pop-the-last convention (there `name` order
/// happened to equal recency; here mtime is compared directly since name
/// order carries no such guarantee for a fixed/overwritten filename).
///
/// Normally there is at most one `.zip` present at a time (the delivery is
/// a single file, overwritten in place) -- a second candidate is a
/// pathological case the caller is expected to log a warning about, same
/// defensive posture the old manifest-candidate handling had.
///
/// Only names `routing` classifies as CIF are candidates: a CORPUS extract
/// (any format, a zip included) never is -- see `pattern.rs`.
pub fn find_zip_candidates(snapshot: &DirSnapshot, routing: &Routing) -> Vec<(String, SystemTime)> {
    let mut candidates: Vec<(String, SystemTime)> = snapshot
        .0
        .iter()
        .filter(|(name, _)| is_zip_filename(name) && routing.is_cif(name))
        .map(|(name, &(mtime, _))| (name.clone(), mtime))
        .collect();
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
    candidates
}

/// How a newly observed, stable zip candidate's mtime relates to the last
/// one this service successfully ingested.
///
/// There is no equivalent of the old `SequenceRelation::Gap` -- without a
/// sequence number there is nothing to be non-contiguous, so this only has
/// two variants; don't invent a fake third one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryRelation {
    /// `current` matches the last ingested mtime exactly -- this delivery
    /// has already been processed.
    AlreadyIngested,
    /// Anything else -- in practice always newer, since the file is
    /// overwritten forward in time, but this is not assumed; any different
    /// mtime is treated as a new delivery to process.
    New,
}

/// Classifies `current` (a stable zip candidate's observed mtime) against
/// `last` (the last successfully ingested delivery's mtime, or `None` if
/// this service has never ingested one).
pub fn classify_delivery(last: Option<SystemTime>, current: SystemTime) -> DeliveryRelation {
    match last {
        Some(last) if last == current => DeliveryRelation::AlreadyIngested,
        _ => DeliveryRelation::New,
    }
}

/// Renders `mtime` as a compact, sortable-as-a-plain-string UTC timestamp
/// (`20260903T172830Z`) -- used as the storage subdirectory name for one
/// delivery. Lexicographic string ordering of this format matches
/// chronological ordering exactly (fixed-width fields, most-significant
/// first), which both `schedule-reference`'s "find the latest" discovery
/// logic and this crate's own retention pruning rely on.
pub fn delivery_dir_name(mtime: SystemTime) -> String {
    let dt: DateTime<Utc> = DateTime::<Utc>::from(mtime);
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Whether `name` has [`delivery_dir_name`]'s exact shape
/// (`YYYYMMDDTHHMMSSZ`, 16 ASCII bytes) -- used defensively by pruning to
/// ignore any directory that isn't one this crate itself created, same
/// "never guess about unrelated names" posture the old numeric-only
/// `prune_old_sequences` had.
pub fn is_delivery_dir_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    bytes.len() == 16
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'Z'
}

/// Caps on what one delivery zip may expand to (finding PL-5 of the
/// 2026-09-27 pipelines review). The zip arrives through the
/// internet-facing SFTP container, so a small archive that inflates to fill
/// `storage_dir`'s volume would stop the whole CIF pipeline. A real full
/// CIF delivery is ~1-2 GB uncompressed across about a dozen files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractLimits {
    /// Total uncompressed bytes across every extracted entry.
    pub max_total_bytes: u64,
    /// Number of entries in the archive (directories included).
    pub max_entries: usize,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            max_total_bytes: 4 * 1024 * 1024 * 1024,
            max_entries: 64,
        }
    }
}

/// A zip that can never be extracted as it stands: over a cap in
/// [`ExtractLimits`], or an entry whose inflated size differs from the size
/// its central directory declares. Permanent for these bytes, so the caller
/// quarantines the delivery instead of retrying it every cycle.
#[derive(Debug)]
pub struct RejectedZip(pub String);

impl std::fmt::Display for RejectedZip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "zip delivery rejected: {}", self.0)
    }
}

impl std::error::Error for RejectedZip {}

/// Whether `err` is a [`RejectedZip`] (permanent) rather than an IO or
/// transient failure worth retrying.
pub fn is_rejected(err: &anyhow::Error) -> bool {
    err.downcast_ref::<RejectedZip>().is_some()
}

/// Extracts every regular-file entry of the zip at `zip_path` directly into
/// `dest_dir` (created if needed), streaming each entry straight to disk
/// (the real `RJTTFnnnMCA.txt` entry is ~700MB uncompressed -- this must
/// never hold a whole entry in memory). Returns each extracted file's name
/// and byte count, mirroring the shape `ScheduleFeedFile` records already
/// used (see `main.rs`).
///
/// Entry paths are sanitized via `enclosed_name()` (guards against a
/// zip-slip-style `../` escape) -- a real delivery's entries are all flat
/// top-level files, but this is defensive, not assumed.
///
/// Every file is fsynced, and its extracted size checked against the size
/// the zip's own central directory records for it, so a short write can
/// never pass for a complete file. Callers outside tests go through
/// [`ensure_extracted`], never this directly: `dest_dir` is written in place.
///
/// PL-5: before anything is written, the entry count and the sum of the
/// declared sizes are checked against `limits`; and each entry is read
/// through a `take()` of its declared size plus one byte, so an entry that
/// lies about its size stops there instead of filling the disk. Any of
/// these is a [`RejectedZip`].
pub fn extract_zip(
    zip_path: &Path,
    dest_dir: &Path,
    limits: ExtractLimits,
) -> anyhow::Result<Vec<(String, u64)>> {
    use std::io::Read;

    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| anyhow::anyhow!("failed to open {zip_path:?} as a zip archive: {err}"))?;

    if archive.len() > limits.max_entries {
        return Err(RejectedZip(format!(
            "{} entries, over the cap of {}",
            archive.len(),
            limits.max_entries
        ))
        .into());
    }
    let mut declared_total: u64 = 0;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        if !entry.is_dir() {
            declared_total = declared_total.saturating_add(entry.size());
        }
    }
    if declared_total > limits.max_total_bytes {
        return Err(RejectedZip(format!(
            "declares {declared_total} uncompressed bytes, over the cap of {}",
            limits.max_total_bytes
        ))
        .into());
    }

    std::fs::create_dir_all(dest_dir)?;

    let mut extracted = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            tracing::warn!(
                entry = entry.name(),
                "skipping zip entry with an unsafe/unresolvable path"
            );
            continue;
        };
        // Real deliveries only ever contain flat top-level files -- reject
        // (rather than silently nest) anything with path components, since
        // this crate's whole downstream contract assumes flat filenames
        // directly under the delivery directory.
        if enclosed.components().count() != 1 {
            tracing::warn!(entry = %enclosed.display(), "skipping zip entry with unexpected nested path");
            continue;
        }

        let declared = entry.size();
        let out_path = dest_dir.join(&enclosed);
        let mut out_file = std::fs::File::create(&out_path)?;
        let bytes = std::io::copy(&mut entry.take(declared.saturating_add(1)), &mut out_file)?;
        out_file.sync_all()?;
        if bytes != declared {
            return Err(RejectedZip(format!(
                "entry {} inflated to {}{} bytes but the zip records {}",
                enclosed.display(),
                if bytes > declared { "more than " } else { "" },
                bytes.min(declared),
                declared
            ))
            .into());
        }
        extracted.push((enclosed.to_string_lossy().into_owned(), bytes));
    }

    Ok(extracted)
}

/// How [`ensure_extracted`] arrived at a complete delivery directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extraction {
    /// The directory already carried the completion marker: nothing was
    /// extracted (PL-13 -- this is the steady state while an api POST keeps
    /// failing, and after a restart).
    AlreadyComplete,
    /// A directory from before the marker existed whose files match the
    /// zip's recorded sizes exactly: the marker was added, nothing extracted.
    Adopted,
    /// The zip was extracted (atomically) into place.
    Extracted,
}

/// Makes `storage_dir/<dir_name>` a complete, marked extraction of
/// `zip_path`, doing as little as possible:
///
/// 1. Already marked complete: returns the file list recorded in the
///    marker. No extraction, no IO beyond reading the marker.
/// 2. Present but unmarked (written by a version before PL-6) and every
///    zip entry is on disk at exactly its recorded size: marks it complete.
/// 3. Otherwise extracts into `storage_dir/.tmp-<dir_name>` (a stale one
///    from a crash is removed first), fsyncs every file, writes the marker,
///    fsyncs the directory, and renames it into place. A pre-existing
///    unmarked (so incomplete) final directory is first renamed aside and
///    removed after the swap. `storage_dir` is on one volume, so each rename
///    is atomic: a reader sees either no directory or a complete one.
pub fn ensure_extracted(
    zip_path: &Path,
    storage_dir: &Path,
    dir_name: &str,
    limits: ExtractLimits,
) -> anyhow::Result<(Vec<(String, u64)>, Extraction)> {
    let final_dir = storage_dir.join(dir_name);
    if let Some(files) = read_marker(&final_dir)? {
        return Ok((files, Extraction::AlreadyComplete));
    }
    if final_dir.is_dir()
        && let Some(files) = files_matching_zip(zip_path, &final_dir)?
    {
        write_marker(&final_dir, &files)?;
        return Ok((files, Extraction::Adopted));
    }

    std::fs::create_dir_all(storage_dir)?;
    let temp_dir = storage_dir.join(format!("{TEMP_DIR_PREFIX}{dir_name}"));
    if temp_dir.exists() {
        std::fs::remove_dir_all(&temp_dir)?;
    }
    let files = match extract_zip(zip_path, &temp_dir, limits) {
        Ok(files) => files,
        Err(err) => {
            // Free the space now: a partial (or rejected, oversized)
            // extraction must not sit on the shared volume until the next
            // attempt.
            if let Err(cleanup) = std::fs::remove_dir_all(&temp_dir) {
                tracing::warn!(error = ?cleanup, path = ?temp_dir, "failed to remove a failed extraction's scratch directory");
            }
            return Err(err);
        }
    };
    write_marker(&temp_dir, &files)?;
    fsync_dir(&temp_dir)?;

    if final_dir.exists() {
        let aside = storage_dir.join(format!("{STALE_DIR_PREFIX}{dir_name}"));
        if aside.exists() {
            std::fs::remove_dir_all(&aside)?;
        }
        std::fs::rename(&final_dir, &aside)?;
        std::fs::rename(&temp_dir, &final_dir)?;
        fsync_dir(storage_dir)?;
        if let Err(err) = std::fs::remove_dir_all(&aside) {
            tracing::warn!(error = ?err, path = ?aside, "failed to remove a replaced incomplete delivery directory");
        }
    } else {
        std::fs::rename(&temp_dir, &final_dir)?;
        fsync_dir(storage_dir)?;
    }
    Ok((files, Extraction::Extracted))
}

/// Where [`ensure_extracted`] moves an incomplete final directory it is
/// about to replace. Hidden, so neither discovery nor pruning ever sees it.
const STALE_DIR_PREFIX: &str = ".stale-";

/// The file list the completion marker in `dir` records, or `None` when
/// `dir` has no marker (or an unreadable one, which is treated the same:
/// the delivery is simply extracted again).
pub fn read_marker(dir: &Path) -> anyhow::Result<Option<Vec<(String, u64)>>> {
    let contents = match std::fs::read_to_string(dir.join(COMPLETE_MARKER)) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut files = Vec::new();
    for line in contents.lines().filter(|line| !line.is_empty()) {
        let Some((name, bytes)) = line.split_once('\t') else {
            return Ok(None);
        };
        let Ok(bytes) = bytes.parse() else {
            return Ok(None);
        };
        files.push((name.to_string(), bytes));
    }
    Ok(Some(files))
}

/// Writes the completion marker into `dir` atomically (a temp file, fsynced,
/// then renamed), so the marker itself can never be seen half-written.
fn write_marker(dir: &Path, files: &[(String, u64)]) -> anyhow::Result<()> {
    let temp = dir.join(format!("{COMPLETE_MARKER}.tmp"));
    {
        let mut out = std::fs::File::create(&temp)?;
        for (name, bytes) in files {
            writeln!(out, "{name}\t{bytes}")?;
        }
        out.sync_all()?;
    }
    std::fs::rename(&temp, dir.join(COMPLETE_MARKER))?;
    fsync_dir(dir)?;
    Ok(())
}

/// fsyncs a directory, making the renames and file creations inside it
/// durable.
fn fsync_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::File::open(dir)?.sync_all()
}

/// The zip's flat regular-file entries with their recorded sizes, when every
/// one of them exists in `dir` at exactly that size; `None` otherwise (or
/// for an empty zip).
fn files_matching_zip(zip_path: &Path, dir: &Path) -> anyhow::Result<Option<Vec<(String, u64)>>> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| anyhow::anyhow!("failed to open {zip_path:?} as a zip archive: {err}"))?;
    let mut files = Vec::new();
    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        if enclosed.components().count() != 1 {
            continue;
        }
        let on_disk = match std::fs::metadata(dir.join(&enclosed)) {
            Ok(metadata) if metadata.is_file() => metadata.len(),
            Ok(_) => return Ok(None),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        if on_disk != entry.size() {
            return Ok(None);
        }
        files.push((enclosed.to_string_lossy().into_owned(), on_disk));
    }
    Ok((!files.is_empty()).then_some(files))
}

/// One-off startup pass for directories written before PL-6 introduced the
/// completion marker, so `schedule-reference` (which now requires it) keeps
/// seeing the deliveries already on the volume. Also removes scratch
/// directories (`.tmp-*`, `.stale-*`) a crash left behind -- nothing else
/// is running in `storage_dir` at startup, so none of them is in progress.
///
/// An unmarked delivery directory is adopted (marked complete) when:
/// * it belongs to a zip still in `watch_dir` (same timestamp name) and
///   every entry of that zip is on disk at exactly its recorded size -- a
///   truncated one is left unmarked, and the next scan cycle re-extracts it
///   atomically; or
/// * it belongs to an older delivery and holds both an `RJTTF*MCA.txt` and
///   an `RJTTF*MSN.txt` -- exactly the old completeness rule, the best
///   available for a delivery whose zip is gone.
///
/// Returns the names adopted.
pub fn adopt_legacy_deliveries(
    storage_dir: &Path,
    watch_dir: &Path,
    routing: &Routing,
) -> anyhow::Result<Vec<String>> {
    let read_dir = match std::fs::read_dir(storage_dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut delivery_dirs: Vec<(String, PathBuf)> = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with(TEMP_DIR_PREFIX) || name.starts_with(STALE_DIR_PREFIX) {
            tracing::warn!(dir = %name, "removing a scratch delivery directory left behind by an interrupted extraction");
            std::fs::remove_dir_all(entry.path())?;
        } else if is_delivery_dir_name(&name) {
            delivery_dirs.push((name, entry.path()));
        }
    }

    let current_zips: std::collections::HashMap<String, PathBuf> =
        find_zip_candidates(&crate::scan::scan_incoming(watch_dir)?, routing)
            .into_iter()
            .map(|(name, mtime)| (delivery_dir_name(mtime), watch_dir.join(name)))
            .collect();

    let mut adopted = Vec::new();
    for (name, path) in delivery_dirs {
        if path.join(COMPLETE_MARKER).exists() {
            continue;
        }
        let files = match current_zips.get(&name) {
            Some(zip_path) => files_matching_zip(zip_path, &path)?,
            None => legacy_files_if_complete(&path)?,
        };
        match files {
            Some(files) => {
                write_marker(&path, &files)?;
                tracing::info!(dir = %name, "adopted a delivery directory extracted before the completion marker existed");
                adopted.push(name);
            }
            None => {
                tracing::warn!(dir = %name, "delivery directory without a completion marker is incomplete; leaving it for re-extraction or pruning");
            }
        }
    }
    Ok(adopted)
}

/// Every regular, non-hidden file in `dir` with its size, when `dir` has
/// both an `RJTTF*MCA.txt` and an `RJTTF*MSN.txt` (the pre-PL-6 rule).
fn legacy_files_if_complete(dir: &Path) -> anyhow::Result<Option<Vec<(String, u64)>>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        if !metadata.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        files.push((name, metadata.len()));
    }
    let has = |suffix: &str| {
        files
            .iter()
            .any(|(name, _)| name.starts_with("RJTTF") && name.ends_with(suffix))
    };
    if has("MCA.txt") && has("MSN.txt") {
        files.sort();
        Ok(Some(files))
    } else {
        Ok(None)
    }
}

/// A minimal in-memory `.zip` writer, used only by this module's own tests
/// (and reused by `main.rs`'s tests) to build a fixture archive without a
/// checked-in binary file.
#[cfg(test)]
pub fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        for (name, content) in entries {
            writer
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut writer, content).unwrap();
        }
        writer.finish().unwrap();
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn snapshot(entries: &[(&str, u64, u64)]) -> DirSnapshot {
        DirSnapshot(
            entries
                .iter()
                .map(|&(name, mtime_secs, len)| {
                    (
                        name.to_string(),
                        (UNIX_EPOCH + Duration::from_secs(mtime_secs), len),
                    )
                })
                .collect(),
        )
    }

    #[test]
    fn zip_filename_matching_is_case_insensitive_and_generic() {
        assert!(is_zip_filename("timetable_full.zip"));
        assert!(is_zip_filename("TIMETABLE_FULL.ZIP"));
        assert!(is_zip_filename("some-other-name.zip"));
        assert!(!is_zip_filename("RJTTF942DAT.txt"));
        assert!(!is_zip_filename("zip"));
        assert!(!is_zip_filename(".zip"));
    }

    #[test]
    fn find_zip_candidates_ignores_non_zip_files() {
        let snap = snapshot(&[
            ("timetable_full.zip", 100, 1234),
            ("RJTTF942DAT.txt", 100, 1),
            ("readme.txt", 100, 1),
        ]);
        let candidates = find_zip_candidates(&snap, &Routing::defaults());
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, "timetable_full.zip");
    }

    /// The CIF guard: a newer CORPUS extract pushed as a zip must not take
    /// the CIF delivery's place, and a CORPUS extract in any other format
    /// is not a candidate either.
    #[test]
    fn find_zip_candidates_never_picks_a_corpus_delivery() {
        let snap = snapshot(&[
            ("timetable_full.zip", 100, 1234),
            ("CORPUSExtract.zip", 200, 99),
            ("CORPUSExtract.json.zip", 201, 99),
            ("CORPUSExtract.csv.zip", 202, 99),
            ("CORPUSExtract.json.gz", 203, 99),
            ("CORPUSExtract.csv.gz", 204, 99),
            ("CORPUSExtract.csv", 205, 99),
            ("CORPUSExtract.json", 206, 99),
        ]);
        let candidates = find_zip_candidates(&snap, &Routing::defaults());
        assert_eq!(
            candidates,
            vec![(
                "timetable_full.zip".to_string(),
                UNIX_EPOCH + Duration::from_secs(100)
            )]
        );
    }

    /// A tightened CIF pattern excludes other zips entirely.
    #[test]
    fn find_zip_candidates_honours_a_narrower_cif_pattern() {
        let snap = snapshot(&[("timetable_full.zip", 100, 1), ("other.zip", 200, 1)]);
        let routing = Routing {
            cif: crate::pattern::FilePattern::parse("timetable*.zip").unwrap(),
            cif_exclude: crate::pattern::FilePattern::parse("CORPUSExtract*").unwrap(),
            corpus: None,
        };
        let candidates = find_zip_candidates(&snap, &routing);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, "timetable_full.zip");
    }

    #[test]
    fn find_zip_candidates_sorts_the_most_recently_modified_last() {
        let snap = snapshot(&[("old.zip", 100, 1), ("new.zip", 200, 1)]);
        let routing = Routing {
            cif: crate::pattern::FilePattern::parse("*.zip").unwrap(),
            ..Routing::defaults()
        };
        let candidates = find_zip_candidates(&snap, &routing);
        assert_eq!(
            candidates.last().map(|(name, _)| name.as_str()),
            Some("new.zip")
        );
    }

    #[test]
    fn classify_first_ever_ingest_is_new() {
        let mtime = UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(classify_delivery(None, mtime), DeliveryRelation::New);
    }

    #[test]
    fn classify_same_mtime_is_already_ingested() {
        let mtime = UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(
            classify_delivery(Some(mtime), mtime),
            DeliveryRelation::AlreadyIngested
        );
    }

    #[test]
    fn classify_a_different_mtime_is_new_even_if_earlier() {
        // Don't hard-assume monotonicity -- any *different* mtime is a new
        // delivery, not just a strictly later one.
        let last = UNIX_EPOCH + Duration::from_secs(200);
        let current = UNIX_EPOCH + Duration::from_secs(100);
        assert_eq!(
            classify_delivery(Some(last), current),
            DeliveryRelation::New
        );
    }

    #[test]
    fn delivery_dir_name_matches_the_expected_compact_sortable_format() {
        let mtime = DateTime::parse_from_rfc3339("2026-09-03T17:28:30Z")
            .unwrap()
            .with_timezone(&Utc);
        let name = delivery_dir_name(SystemTime::from(mtime));
        assert_eq!(name, "20260903T172830Z");
    }

    #[test]
    fn lexicographic_order_of_delivery_dir_names_matches_chronological_order() {
        let earlier = DateTime::parse_from_rfc3339("2026-09-03T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let later = DateTime::parse_from_rfc3339("2026-09-04T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let earlier_name = delivery_dir_name(SystemTime::from(earlier));
        let later_name = delivery_dir_name(SystemTime::from(later));
        assert!(earlier_name < later_name);
    }

    #[test]
    fn delivery_dir_name_shape_is_recognized_and_other_shapes_are_not() {
        assert!(is_delivery_dir_name("20260903T172830Z"));
        assert!(!is_delivery_dir_name("942"));
        assert!(!is_delivery_dir_name("not-a-timestamp"));
        assert!(!is_delivery_dir_name("20260903T172830"));
        assert!(!is_delivery_dir_name(""));
    }

    #[test]
    fn extract_zip_streams_every_flat_entry_to_dest_dir() {
        let bytes = build_test_zip(&[
            ("RJTTF942MCA.txt", b"mca content"),
            ("RJTTF942MSN.txt", b"msn content"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("timetable_full.zip");
        std::fs::write(&zip_path, &bytes).unwrap();
        let dest_dir = dir.path().join("20260903T172830Z");

        let mut extracted = extract_zip(&zip_path, &dest_dir, ExtractLimits::default()).unwrap();
        extracted.sort();

        assert_eq!(
            extracted,
            vec![
                ("RJTTF942MCA.txt".to_string(), "mca content".len() as u64),
                ("RJTTF942MSN.txt".to_string(), "msn content".len() as u64),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(dest_dir.join("RJTTF942MCA.txt")).unwrap(),
            "mca content"
        );
    }

    #[test]
    fn extract_zip_into_an_already_existing_dir_overwrites_cleanly() {
        // Exercises the restart-idempotency path: re-extracting the same
        // delivery (e.g. after an in-memory-state-losing restart) into the
        // same timestamp-named directory must not error.
        let bytes = build_test_zip(&[("RJTTF942MCA.txt", b"content")]);
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("timetable_full.zip");
        std::fs::write(&zip_path, &bytes).unwrap();
        let dest_dir = dir.path().join("20260903T172830Z");

        extract_zip(&zip_path, &dest_dir, ExtractLimits::default()).unwrap();
        let extracted_again = extract_zip(&zip_path, &dest_dir, ExtractLimits::default()).unwrap();
        assert_eq!(extracted_again.len(), 1);
    }

    /// PL-5: a zip declaring more uncompressed bytes than the cap is
    /// rejected before a single byte is written.
    #[test]
    fn extract_zip_rejects_a_zip_over_the_byte_cap_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(dir.path());
        let dest_dir = dir.path().join("out");
        let limits = ExtractLimits {
            max_total_bytes: 21,
            max_entries: 64,
        };
        let err = extract_zip(&zip_path, &dest_dir, limits).unwrap_err();
        assert!(is_rejected(&err), "{err:?}");
        assert!(
            !dest_dir.exists(),
            "nothing may be written for a rejected zip"
        );

        // Exactly at the cap (11 + 11 bytes) is fine.
        let limits = ExtractLimits {
            max_total_bytes: 22,
            max_entries: 64,
        };
        assert_eq!(extract_zip(&zip_path, &dest_dir, limits).unwrap().len(), 2);
    }

    #[test]
    fn extract_zip_rejects_a_zip_over_the_entry_cap() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(dir.path());
        let limits = ExtractLimits {
            max_total_bytes: u64::MAX,
            max_entries: 1,
        };
        let err = extract_zip(&zip_path, &dir.path().join("out"), limits).unwrap_err();
        assert!(is_rejected(&err), "{err:?}");
    }

    /// A rejected extraction leaves no scratch directory on the volume.
    #[test]
    fn ensure_extracted_cleans_up_after_a_rejected_zip() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(dir.path());
        let storage = tempfile::tempdir().unwrap();
        let limits = ExtractLimits {
            max_total_bytes: 1,
            max_entries: 64,
        };
        let err =
            ensure_extracted(&zip_path, storage.path(), "20260903T172830Z", limits).unwrap_err();
        assert!(is_rejected(&err));
        assert!(names_in(storage.path()).is_empty());
    }

    #[test]
    fn default_limits_fit_a_real_delivery() {
        let limits = ExtractLimits::default();
        assert!(limits.max_total_bytes >= 4 * 1024 * 1024 * 1024);
        assert!(limits.max_entries >= 16);
    }

    fn fixture_zip(dir: &Path) -> PathBuf {
        let bytes = build_test_zip(&[
            ("RJTTF942MCA.txt", b"mca content"),
            ("RJTTF942MSN.txt", b"msn content"),
        ]);
        let zip_path = dir.join("timetable_full.zip");
        std::fs::write(&zip_path, &bytes).unwrap();
        zip_path
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// PL-6: the delivery appears in `storage_dir` complete and marked, with
    /// no scratch directory left behind.
    #[test]
    fn ensure_extracted_renames_a_complete_marked_directory_into_place() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(watch.path());

        let (mut files, how) = ensure_extracted(
            &zip_path,
            storage.path(),
            "20260903T172830Z",
            ExtractLimits::default(),
        )
        .unwrap();
        files.sort();

        assert_eq!(how, Extraction::Extracted);
        assert_eq!(
            names_in(storage.path()),
            vec!["20260903T172830Z".to_string()]
        );
        let final_dir = storage.path().join("20260903T172830Z");
        assert_eq!(
            names_in(&final_dir),
            vec![
                COMPLETE_MARKER.to_string(),
                "RJTTF942MCA.txt".to_string(),
                "RJTTF942MSN.txt".to_string()
            ]
        );
        let mut recorded = read_marker(&final_dir).unwrap().unwrap();
        recorded.sort();
        assert_eq!(recorded, files);
        assert_eq!(
            std::fs::read_to_string(final_dir.join("RJTTF942MCA.txt")).unwrap(),
            "mca content"
        );
    }

    /// PL-13: a delivery already marked complete is never extracted again --
    /// the file list comes from the marker.
    #[test]
    fn ensure_extracted_does_not_re_extract_a_marked_directory() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(watch.path());
        ensure_extracted(
            &zip_path,
            storage.path(),
            "20260903T172830Z",
            ExtractLimits::default(),
        )
        .unwrap();
        let mca = storage.path().join("20260903T172830Z/RJTTF942MCA.txt");
        std::fs::write(&mca, b"untouched since").unwrap();

        for _ in 0..3 {
            let (files, how) = ensure_extracted(
                &zip_path,
                storage.path(),
                "20260903T172830Z",
                ExtractLimits::default(),
            )
            .unwrap();
            assert_eq!(how, Extraction::AlreadyComplete);
            assert_eq!(files.len(), 2);
        }
        assert_eq!(std::fs::read_to_string(&mca).unwrap(), "untouched since");
    }

    /// PL-6: an unmarked directory with a truncated file (an extraction by
    /// the old in-place code that was interrupted) is replaced whole.
    #[test]
    fn ensure_extracted_replaces_an_unmarked_truncated_directory() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(watch.path());
        let final_dir = storage.path().join("20260903T172830Z");
        std::fs::create_dir_all(&final_dir).unwrap();
        std::fs::write(final_dir.join("RJTTF942MCA.txt"), b"mca con").unwrap();
        std::fs::write(final_dir.join("RJTTF942MSN.txt"), b"msn content").unwrap();

        let (_, how) = ensure_extracted(
            &zip_path,
            storage.path(),
            "20260903T172830Z",
            ExtractLimits::default(),
        )
        .unwrap();

        assert_eq!(how, Extraction::Extracted);
        assert_eq!(
            std::fs::read_to_string(final_dir.join("RJTTF942MCA.txt")).unwrap(),
            "mca content"
        );
        assert!(final_dir.join(COMPLETE_MARKER).is_file());
        assert_eq!(
            names_in(storage.path()),
            vec!["20260903T172830Z".to_string()]
        );
    }

    /// Compatibility: an unmarked directory whose files match the zip
    /// exactly (a complete extraction by the old code) is adopted, not
    /// extracted again.
    #[test]
    fn ensure_extracted_adopts_an_unmarked_directory_matching_the_zip() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(watch.path());
        let final_dir = storage.path().join("20260903T172830Z");
        extract_zip(&zip_path, &final_dir, ExtractLimits::default()).unwrap();

        let (files, how) = ensure_extracted(
            &zip_path,
            storage.path(),
            "20260903T172830Z",
            ExtractLimits::default(),
        )
        .unwrap();

        assert_eq!(how, Extraction::Adopted);
        assert_eq!(files.len(), 2);
        assert!(final_dir.join(COMPLETE_MARKER).is_file());
    }

    /// A scratch directory from a crashed extraction is discarded, not
    /// extracted into on top of.
    #[test]
    fn ensure_extracted_discards_a_stale_scratch_directory() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(watch.path());
        let scratch = storage
            .path()
            .join(format!("{TEMP_DIR_PREFIX}20260903T172830Z"));
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::write(scratch.join("RJTTF000ZZZ.txt"), b"leftover").unwrap();

        ensure_extracted(
            &zip_path,
            storage.path(),
            "20260903T172830Z",
            ExtractLimits::default(),
        )
        .unwrap();

        assert_eq!(
            names_in(storage.path()),
            vec!["20260903T172830Z".to_string()]
        );
        assert!(
            !storage
                .path()
                .join("20260903T172830Z/RJTTF000ZZZ.txt")
                .exists()
        );
    }

    /// The startup pass: older complete legacy directories are adopted, an
    /// incomplete one is not, the current zip's truncated directory is left
    /// for re-extraction, and scratch directories are removed.
    #[test]
    fn adopt_legacy_deliveries_marks_only_complete_directories() {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let zip_path = fixture_zip(watch.path());
        let current = delivery_dir_name(std::fs::metadata(&zip_path).unwrap().modified().unwrap());

        let write = |dir: &str, files: &[(&str, &[u8])]| {
            let path = storage.path().join(dir);
            std::fs::create_dir_all(&path).unwrap();
            for (name, content) in files {
                std::fs::write(path.join(name), content).unwrap();
            }
        };
        write(
            "20200101T000000Z",
            &[("RJTTF1MCA.txt", b"a"), ("RJTTF1MSN.txt", b"b")],
        );
        write("20200102T000000Z", &[("RJTTF2MCA.txt", b"a")]);
        write(
            &current,
            &[
                ("RJTTF942MCA.txt", b"mca"),
                ("RJTTF942MSN.txt", b"msn content"),
            ],
        );
        write(".tmp-20200103T000000Z", &[("RJTTF3MCA.txt", b"a")]);

        let adopted =
            adopt_legacy_deliveries(storage.path(), watch.path(), &Routing::defaults()).unwrap();

        assert_eq!(adopted, vec!["20200101T000000Z".to_string()]);
        assert!(
            storage
                .path()
                .join("20200101T000000Z")
                .join(COMPLETE_MARKER)
                .is_file()
        );
        assert!(
            !storage
                .path()
                .join("20200102T000000Z")
                .join(COMPLETE_MARKER)
                .exists()
        );
        assert!(!storage.path().join(&current).join(COMPLETE_MARKER).exists());
        assert!(!storage.path().join(".tmp-20200103T000000Z").exists());

        // And once the current zip's directory is complete, it is adopted.
        std::fs::write(
            storage.path().join(&current).join("RJTTF942MCA.txt"),
            b"mca content",
        )
        .unwrap();
        let adopted =
            adopt_legacy_deliveries(storage.path(), watch.path(), &Routing::defaults()).unwrap();
        assert_eq!(adopted, vec![current]);
    }
}
