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
//! 3. [`extract_zip`] -- unzipping a stable candidate's contents directly
//!    into a timestamp-named storage directory (see [`delivery_dir_name`]),
//!    so `schedule-reference` keeps reading real flat `RJTTFnnn*.txt` files
//!    off disk, unchanged.
//!
//! Stability detection itself is unchanged -- `scan::StabilityTracker` is
//! reused as-is, just pointed at the zip candidate instead of a manifest
//! candidate.

use std::path::Path;
use std::time::SystemTime;

use chrono::{DateTime, Utc};

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
pub fn find_zip_candidates(snapshot: &DirSnapshot) -> Vec<(String, SystemTime)> {
    let mut candidates: Vec<(String, SystemTime)> = snapshot
        .0
        .iter()
        .filter(|(name, _)| is_zip_filename(name))
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
pub fn extract_zip(zip_path: &Path, dest_dir: &Path) -> anyhow::Result<Vec<(String, u64)>> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|err| anyhow::anyhow!("failed to open {zip_path:?} as a zip archive: {err}"))?;

    std::fs::create_dir_all(dest_dir)?;

    let mut extracted = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
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

        let out_path = dest_dir.join(&enclosed);
        let mut out_file = std::fs::File::create(&out_path)?;
        let bytes = std::io::copy(&mut entry, &mut out_file)?;
        extracted.push((enclosed.to_string_lossy().into_owned(), bytes));
    }

    Ok(extracted)
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
        let candidates = find_zip_candidates(&snap);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].0, "timetable_full.zip");
    }

    #[test]
    fn find_zip_candidates_sorts_the_most_recently_modified_last() {
        let snap = snapshot(&[("old.zip", 100, 1), ("new.zip", 200, 1)]);
        let candidates = find_zip_candidates(&snap);
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

        let mut extracted = extract_zip(&zip_path, &dest_dir).unwrap();
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

        extract_zip(&zip_path, &dest_dir).unwrap();
        let extracted_again = extract_zip(&zip_path, &dest_dir).unwrap();
        assert_eq!(extracted_again.len(), 1);
    }
}
