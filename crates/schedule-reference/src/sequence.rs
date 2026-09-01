use std::path::Path;

/// The highest-numbered immediate subdirectory of `storage_dir` that
/// contains both an `RJTTF<n>MCA.txt` and an `RJTTF<n>MSN.txt` file.
/// Mirrors `schedule-ingest`'s own `prune_old_sequences`
/// (crates/schedule-ingest/src/main.rs:445-484) numeric-subdirectory-scan
/// technique -- independently reimplemented here, not shared code across
/// the crate boundary (see this design's Decision 1: "a new, small,
/// independently-written function, not shared code"). `None` if
/// `storage_dir` doesn't exist yet, or no subdirectory has both files --
/// not an error, matching `schedule-ingest::scan::scan_incoming`'s own
/// "not-yet-existing is empty, not an error" posture.
pub fn highest_complete_sequence(storage_dir: &Path) -> anyhow::Result<Option<u32>> {
    let read_dir = match std::fs::read_dir(storage_dir) {
        Ok(read_dir) => read_dir,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };

    let mut sequences: Vec<u32> = Vec::new();
    for entry in read_dir {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(sequence) = name.parse::<u32>() else {
            continue;
        };

        let dir = entry.path();
        let has_mca = dir.join(format!("RJTTF{sequence}MCA.txt")).is_file();
        let has_msn = dir.join(format!("RJTTF{sequence}MSN.txt")).is_file();
        if has_mca && has_msn {
            sequences.push(sequence);
        }
    }

    Ok(sequences.into_iter().max())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(dir: &Path, sequence: &str, name: &str) {
        std::fs::write(dir.join(sequence).join(name), b"x").unwrap();
    }

    #[test]
    fn picks_the_highest_sequence_with_both_files_present() {
        let dir = tempfile::tempdir().unwrap();
        for seq in ["940", "941", "942"] {
            std::fs::create_dir_all(dir.path().join(seq)).unwrap();
        }
        // 942 is missing MSN -- must not be picked.
        touch(dir.path(), "940", "RJTTF940MCA.txt");
        touch(dir.path(), "940", "RJTTF940MSN.txt");
        touch(dir.path(), "941", "RJTTF941MCA.txt");
        touch(dir.path(), "941", "RJTTF941MSN.txt");
        touch(dir.path(), "942", "RJTTF942MCA.txt");

        assert_eq!(highest_complete_sequence(dir.path()).unwrap(), Some(941));
    }

    #[test]
    fn nonexistent_storage_dir_is_none_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist-yet");
        assert_eq!(highest_complete_sequence(&missing).unwrap(), None);
    }

    #[test]
    fn a_sequence_with_only_one_of_the_two_files_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("942")).unwrap();
        touch(dir.path(), "942", "RJTTF942MCA.txt"); // MSN missing
        assert_eq!(highest_complete_sequence(dir.path()).unwrap(), None);
    }
}
