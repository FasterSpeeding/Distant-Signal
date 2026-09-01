//! Directory scanning and mtime/size stability tracking for `watch_dir`.
//!
//! `watch_dir` is written into by a sibling SFTPGo container as DTD pushes
//! a delivery — unlike a *pull* design's remote directory listing (which
//! only ever sees a file DTD has already finished writing), a push
//! receiver can observe a file mid-write. [`StabilityTracker`] mitigates
//! this by only surfacing a filename as a completeness candidate once its
//! `(mtime, len)` pair has been unchanged for several consecutive polls.
//!
//! **Applies specifically to the manifest file itself first**: the Task 5
//! orchestration loop is expected to check the `RJTTFnnnDAT.txt` file's own
//! stability (via this same [`StabilityTracker`]) *before* even attempting
//! to parse it with `manifest::parse`, then check stability of every file
//! the parsed manifest names before considering the whole delivery
//! complete. This module only provides the primitives — it does not
//! implement that two-phase orchestration itself (that's Task 5's job).

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

/// One directory listing of `watch_dir`, keyed by filename, each with its
/// current `(mtime, len)`. Cheap to build every polling cycle.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DirSnapshot(pub HashMap<String, (SystemTime, u64)>);

/// Lists `watch_dir` and stats every regular file in it.
///
/// Subdirectories are skipped. An empty (or not-yet-existing-but-readable)
/// directory yields an empty [`DirSnapshot`], not an error.
pub fn scan_incoming(watch_dir: &Path) -> anyhow::Result<DirSnapshot> {
    let mut entries = HashMap::new();

    for entry in std::fs::read_dir(watch_dir)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if !metadata.is_file() {
            continue;
        }

        let name = entry.file_name().to_string_lossy().into_owned();
        let mtime = metadata.modified()?;
        let len = metadata.len();
        entries.insert(name, (mtime, len));
    }

    Ok(DirSnapshot(entries))
}

/// Tracks how many consecutive [`observe`](StabilityTracker::observe)
/// calls each filename's `(mtime, len)` pair has been unchanged.
///
/// A file only becomes a completeness candidate once it has been stable
/// for `required_cycles` consecutive polls.
#[derive(Debug, Default)]
pub struct StabilityTracker {
    stable_since: HashMap<String, ((SystemTime, u64), u32)>,
}

impl StabilityTracker {
    pub fn new() -> Self {
        Self {
            stable_since: HashMap::new(),
        }
    }

    /// Feeds one polling cycle's [`DirSnapshot`] in and returns the
    /// filenames that have *just* reached `required_cycles` consecutive
    /// identical `(mtime, len)` observations — i.e. only on the cycle they
    /// reach the threshold, not on every subsequent cycle.
    ///
    /// Filenames absent from `snapshot` (deleted, or never seen before)
    /// are dropped from internal tracking entirely, not just skipped — if
    /// such a filename reappears in a later snapshot it starts counting
    /// from zero again.
    pub fn observe(&mut self, snapshot: &DirSnapshot, required_cycles: u32) -> Vec<String> {
        // Drop tracking for anything that vanished since the last
        // snapshot, so a reappearing file starts fresh rather than
        // resuming a stale count.
        self.stable_since
            .retain(|name, _| snapshot.0.contains_key(name));

        let mut just_reached = Vec::new();

        for (name, &current) in &snapshot.0 {
            let entry = self.stable_since.entry(name.clone());
            match entry {
                std::collections::hash_map::Entry::Occupied(mut occupied) => {
                    let (seen, count) = occupied.get_mut();
                    if *seen == current {
                        *count += 1;
                    } else {
                        *seen = current;
                        *count = 1;
                    }

                    if *count == required_cycles {
                        just_reached.push(name.clone());
                    }
                }
                std::collections::hash_map::Entry::Vacant(vacant) => {
                    vacant.insert((current, 1));
                    if required_cycles <= 1 {
                        just_reached.push(name.clone());
                    }
                }
            }
        }

        just_reached
    }
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
    fn returned_exactly_once_on_the_cycle_it_reaches_the_threshold() {
        let mut tracker = StabilityTracker::new();
        let snap = snapshot(&[("RJTTF942ZTR.txt", 100, 1234)]);

        assert_eq!(tracker.observe(&snap, 3), Vec::<String>::new());
        assert_eq!(tracker.observe(&snap, 3), Vec::<String>::new());
        assert_eq!(tracker.observe(&snap, 3), vec!["RJTTF942ZTR.txt".to_string()]);
        // Not returned again on subsequent identical observations.
        assert_eq!(tracker.observe(&snap, 3), Vec::<String>::new());
        assert_eq!(tracker.observe(&snap, 3), Vec::<String>::new());
    }

    #[test]
    fn size_change_resets_the_counter_to_zero_not_one() {
        let mut tracker = StabilityTracker::new();
        let growing = snapshot(&[("RJTTF942ZTR.txt", 100, 1234)]);
        let grown_again = snapshot(&[("RJTTF942ZTR.txt", 101, 2345)]);
        let stable = snapshot(&[("RJTTF942ZTR.txt", 101, 2345)]);

        assert_eq!(tracker.observe(&growing, 2), Vec::<String>::new());
        // Size changed -- counter resets to 0 (this observation counts as
        // the first of a new run, i.e. count becomes 1 after this call).
        assert_eq!(tracker.observe(&grown_again, 2), Vec::<String>::new());
        // If the reset had instead left the counter at 1 (treating this
        // as the second consecutive observation), this call would already
        // report the file stable one cycle too early. It must take a
        // further two matching observations from the reset point.
        assert_eq!(tracker.observe(&stable, 2), vec!["RJTTF942ZTR.txt".to_string()]);
    }

    #[test]
    fn disappearing_file_is_dropped_and_restarts_from_zero_on_reappearance() {
        let mut tracker = StabilityTracker::new();
        let present = snapshot(&[("RJTTF942ZTR.txt", 100, 1234)]);
        let empty = snapshot(&[]);

        assert_eq!(tracker.observe(&present, 3), Vec::<String>::new());
        assert_eq!(tracker.observe(&present, 3), Vec::<String>::new());
        // Vanishes before reaching the threshold.
        assert_eq!(tracker.observe(&empty, 3), Vec::<String>::new());
        // Reappears with the same (mtime, len) it had before vanishing --
        // if tracking had not been dropped, this would already be at
        // count 3 (stable). It must instead take a fresh 3 observations.
        assert_eq!(tracker.observe(&present, 3), Vec::<String>::new());
        assert_eq!(tracker.observe(&present, 3), Vec::<String>::new());
        assert_eq!(tracker.observe(&present, 3), vec!["RJTTF942ZTR.txt".to_string()]);
    }

    #[test]
    fn multiple_files_tracked_independently() {
        let mut tracker = StabilityTracker::new();
        let snap = snapshot(&[
            ("RJTTF942ZTR.txt", 100, 1234),
            ("RJTTF942REJ.txt", 200, 5678),
        ]);

        assert_eq!(tracker.observe(&snap, 2), Vec::<String>::new());
        let mut second = tracker.observe(&snap, 2);
        second.sort();
        assert_eq!(
            second,
            vec!["RJTTF942REJ.txt".to_string(), "RJTTF942ZTR.txt".to_string()]
        );
    }

    #[test]
    fn scan_incoming_returns_matching_triples_for_seeded_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("RJTTF942ZTR.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("RJTTF942REJ.txt"), b"a bit longer content").unwrap();

        let snapshot = scan_incoming(dir.path()).unwrap();

        assert_eq!(snapshot.0.len(), 2);
        let (_, len_a) = snapshot.0.get("RJTTF942ZTR.txt").expect("ZTR present");
        assert_eq!(*len_a, "hello".len() as u64);
        let (_, len_b) = snapshot.0.get("RJTTF942REJ.txt").expect("REJ present");
        assert_eq!(*len_b, "a bit longer content".len() as u64);
    }

    #[test]
    fn scan_incoming_on_empty_directory_returns_empty_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = scan_incoming(dir.path()).unwrap();
        assert!(snapshot.0.is_empty());
    }
}
