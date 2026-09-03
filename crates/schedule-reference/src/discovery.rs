use std::path::{Path, PathBuf};

/// One `schedule-ingest`-produced delivery directory that has both a
/// `RJTTF*MCA.txt`-shaped and a `RJTTF*MSN.txt`-shaped file directly inside
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteDelivery {
    /// The delivery directory's own name -- a timestamp-derived string
    /// (see `schedule-ingest`'s `delivery::delivery_dir_name`), used as
    /// this delivery's identity for dedup purposes (see `main.rs`'s
    /// `last_processed_delivery`). Deliberately NOT constructed from any
    /// number embedded in the filenames inside it -- see this module's
    /// `mca_path`/`msn_path` doc comment for why that number isn't a
    /// reliable identifier either.
    pub dir_name: String,
    pub mca_path: PathBuf,
    pub msn_path: PathBuf,
}

/// The most-recent (by directory name, which sorts lexicographically ==
/// chronologically -- see `schedule-ingest`'s `delivery::delivery_dir_name`)
/// immediate subdirectory of `storage_dir` that contains both a
/// `RJTTF*MCA.txt`-shaped and a `RJTTF*MSN.txt`-shaped file. Falls back to
/// the next-most-recent complete directory if the newest one isn't complete
/// yet (mirrors the old `highest_complete_sequence`'s own behavior of only
/// considering directories where both files are present, never guessing at
/// an in-progress one).
///
/// Filenames are matched by prefix/suffix shape (`RJTTF` / `MCA.txt` or
/// `MSN.txt`), not reconstructed from the directory name -- the number
/// embedded inside a real delivery's own filenames (e.g. the `942` in
/// `RJTTF942MCA.txt`) is not the same identifier as the timestamp-named
/// directory it lives in, and per the repo owner's guidance that embedded
/// number shouldn't be relied on as a stable identifier either (see
/// `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`).
///
/// `None` if `storage_dir` doesn't exist yet, or no subdirectory has both
/// files -- not an error, matching `schedule-ingest::scan::scan_incoming`'s
/// own "not-yet-existing is empty, not an error" posture.
pub fn latest_complete_delivery(storage_dir: &Path) -> anyhow::Result<Option<CompleteDelivery>> {
    let read_dir = match std::fs::read_dir(storage_dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let mut dir_names: Vec<String> = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        dir_names.push(name);
    }
    dir_names.sort();

    for name in dir_names.into_iter().rev() {
        let dir = storage_dir.join(&name);
        let mca_path = find_file_matching(&dir, "RJTTF", "MCA.txt")?;
        let msn_path = find_file_matching(&dir, "RJTTF", "MSN.txt")?;
        if let (Some(mca_path), Some(msn_path)) = (mca_path, msn_path) {
            return Ok(Some(CompleteDelivery {
                dir_name: name,
                mca_path,
                msn_path,
            }));
        }
    }

    Ok(None)
}

/// The first regular file directly inside `dir` whose name starts with
/// `prefix` and ends with `suffix`.
fn find_file_matching(dir: &Path, prefix: &str, suffix: &str) -> anyhow::Result<Option<PathBuf>> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(prefix) && name.ends_with(suffix) {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, delivery: &str, name: &str) {
        std::fs::write(dir.join(delivery).join(name), b"x").unwrap();
    }

    #[test]
    fn picks_the_most_recent_delivery_dir_with_both_files_present() {
        let dir = tempfile::tempdir().unwrap();
        for delivery in ["20260901T090000Z", "20260902T090000Z", "20260903T090000Z"] {
            std::fs::create_dir_all(dir.path().join(delivery)).unwrap();
        }
        // 20260903T090000Z is missing MSN -- must not be picked.
        touch(dir.path(), "20260901T090000Z", "RJTTF940MCA.txt");
        touch(dir.path(), "20260901T090000Z", "RJTTF940MSN.txt");
        touch(dir.path(), "20260902T090000Z", "RJTTF941MCA.txt");
        touch(dir.path(), "20260902T090000Z", "RJTTF941MSN.txt");
        touch(dir.path(), "20260903T090000Z", "RJTTF942MCA.txt");

        let delivery = latest_complete_delivery(dir.path()).unwrap().unwrap();
        assert_eq!(delivery.dir_name, "20260902T090000Z");
        assert_eq!(
            delivery.mca_path,
            dir.path().join("20260902T090000Z/RJTTF941MCA.txt")
        );
        assert_eq!(
            delivery.msn_path,
            dir.path().join("20260902T090000Z/RJTTF941MSN.txt")
        );
    }

    #[test]
    fn nonexistent_storage_dir_is_none_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist-yet");
        assert_eq!(latest_complete_delivery(&missing).unwrap(), None);
    }

    #[test]
    fn a_delivery_dir_with_only_one_of_the_two_files_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("20260903T090000Z")).unwrap();
        touch(dir.path(), "20260903T090000Z", "RJTTF942MCA.txt"); // MSN missing
        assert_eq!(latest_complete_delivery(dir.path()).unwrap(), None);
    }

    /// The embedded number inside a delivery's own filenames must play no
    /// role in picking the winner -- only directory-name recency matters.
    #[test]
    fn the_embedded_filename_number_is_irrelevant_to_which_delivery_wins() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("20260901T090000Z")).unwrap();
        std::fs::create_dir_all(dir.path().join("20260902T090000Z")).unwrap();
        // The chronologically earlier directory has the numerically higher
        // embedded sequence number -- it must still lose to the later dir.
        touch(dir.path(), "20260901T090000Z", "RJTTF999MCA.txt");
        touch(dir.path(), "20260901T090000Z", "RJTTF999MSN.txt");
        touch(dir.path(), "20260902T090000Z", "RJTTF001MCA.txt");
        touch(dir.path(), "20260902T090000Z", "RJTTF001MSN.txt");

        let delivery = latest_complete_delivery(dir.path()).unwrap().unwrap();
        assert_eq!(delivery.dir_name, "20260902T090000Z");
    }
}
