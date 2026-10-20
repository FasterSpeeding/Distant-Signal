//! Several delivery sources feeding one pipeline: the SFTP watch directory
//! and the GCS bucket (`bucket.rs`), each a *lane* that the scan and CORPUS
//! cycles run over in turn. See
//! docs/superpowers/specs/2026-10-02-schedule-feed-gcs-landing-design.md,
//! §9 "One pipeline, deduplicated by content" and "When the two sources
//! disagree".
//!
//! While only SFTP is on, nothing here runs: the cycles behave exactly as
//! before (no ledger, no `source` in audit lines). With the bucket on, every
//! decision about a delivered file is appended to the *content ledger*
//! (`storage_dir/sources/ledger.jsonl`), and each new arrival is judged
//! against it before it is extracted or loaded:
//!
//! - the same content (SHA-256) already handled through the **other**
//!   source is a `duplicate`: not extracted, checked or recorded again;
//! - different content of the same kind from the other source within
//!   `DISAGREEMENT_WINDOW_MINUTES` is a **disagreement**
//!   (`schedule_feed_source_disagreement_total{kind}`, the
//!   `DistantSignalScheduleFeedSourcesDisagree` alert). `SOURCE_PRECEDENCE`
//!   decides: an arrival from the lower-ranked source is `superseded`
//!   (never ingested); one from the higher-ranked source is ingested, and
//!   the other copy's delivery directory is retired if it would otherwise
//!   stay the newest. A quarantined winner never promotes the loser.
//! - anything else goes through the normal path, as before.
//!
//! The same content re-delivered through the *same* source is not a
//! duplicate: that path is unchanged from SFTP-only operation.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// `schedule_feed_source_disagreement_total{kind}` (no `source` label).
pub(crate) const DISAGREEMENT_METRIC: &str = "schedule_feed_source_disagreement_total";
/// The `kind` label values of [`DISAGREEMENT_METRIC`].
pub(crate) const DISAGREEMENT_KINDS: [&str; 2] = ["cif", "corpus"];

/// The ledger under `storage_dir`.
const LEDGER_FILE: &str = "sources/ledger.jsonl";
/// Entries kept when the ledger is compacted (it is compacted once it
/// reaches twice this). A day brings a few entries.
const LEDGER_KEEP: usize = 500;

/// Where a delivery came from. The strings are a contract: they appear in
/// audit lines, the ledger and metric labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SourceId {
    Sftp,
    Bucket,
}

impl SourceId {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Sftp => "sftp",
            Self::Bucket => "bucket",
        }
    }
}

impl std::str::FromStr for SourceId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "sftp" => Ok(Self::Sftp),
            "bucket" => Ok(Self::Bucket),
            other => Err(format!(
                "unknown source {other:?} (expected sftp or bucket)"
            )),
        }
    }
}

/// `SOURCE_PRECEDENCE`: both sources, highest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Precedence(pub [SourceId; 2]);

impl Precedence {
    /// 0 for the source that wins a disagreement.
    pub(crate) fn rank(self, source: SourceId) -> usize {
        usize::from(self.0[0] != source)
    }
}

impl Default for Precedence {
    /// Decision D5: the bucket wins.
    fn default() -> Self {
        Self([SourceId::Bucket, SourceId::Sftp])
    }
}

impl std::str::FromStr for Precedence {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let sources = s
            .split(',')
            .map(str::parse)
            .collect::<Result<Vec<SourceId>, _>>()?;
        match sources.as_slice() {
            [first, second] if first != second => Ok(Self([*first, *second])),
            _ => Err(format!(
                "SOURCE_PRECEDENCE must be a permutation of bucket,sftp, got {s:?}"
            )),
        }
    }
}

/// What a delivered file is, for matching it against the ledger. The two
/// CORPUS formats always differ in content, so each is its own stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Kind {
    Cif,
    CorpusJson,
    CorpusXml,
}

impl Kind {
    /// The `kind` label of [`DISAGREEMENT_METRIC`].
    pub(crate) fn metric_label(self) -> &'static str {
        match self {
            Self::Cif => "cif",
            Self::CorpusJson | Self::CorpusXml => "corpus",
        }
    }

    pub(crate) fn of_corpus(format: crate::corpus::CorpusFormat) -> Self {
        match format {
            crate::corpus::CorpusFormat::Json => Self::CorpusJson,
            crate::corpus::CorpusFormat::Xml => Self::CorpusXml,
        }
    }
}

/// What was decided about an arrival, as recorded in the ledger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Settled {
    Accepted,
    /// Failed its checks (a CIF quarantine, a rejected CORPUS file, a
    /// record the database refused). Still a "winner" for precedence.
    Quarantined,
    Duplicate,
    Superseded,
}

/// One delivered file from one source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Arrival {
    pub kind: Kind,
    pub source: SourceId,
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
    pub delivered_at: DateTime<Utc>,
}

/// One ledger line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Entry {
    #[serde(flatten)]
    pub arrival: Arrival,
    pub outcome: Settled,
}

/// What to do with an arrival.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// This very file (source, name, size, delivery time) was decided
    /// before, e.g. by this process before a restart. Nothing to do.
    Handled(Entry),
    /// The same content already came through the other source.
    Duplicate { of: Entry },
    /// Different content from the higher-ranked source within the window.
    Superseded { by: Entry },
    /// Ingest it. `disagrees_with` is the lower-ranked source's different
    /// copy within the window, if any (counted as a disagreement).
    Proceed { disagrees_with: Option<Entry> },
}

/// Judges `arrival` against `ledger` (oldest first). Pure.
pub(crate) fn judge(
    ledger: &[Entry],
    arrival: &Arrival,
    precedence: Precedence,
    window: chrono::Duration,
) -> Verdict {
    if let Some(entry) = ledger.iter().rev().find(|e| {
        e.arrival.source == arrival.source
            && e.arrival.kind == arrival.kind
            && e.arrival.name == arrival.name
            && e.arrival.bytes == arrival.bytes
            && e.arrival.delivered_at == arrival.delivered_at
            && e.arrival.sha256 == arrival.sha256
    }) {
        return Verdict::Handled(entry.clone());
    }
    let others = || {
        ledger
            .iter()
            .rev()
            .filter(|e| e.arrival.source != arrival.source && e.arrival.kind == arrival.kind)
    };
    if let Some(entry) = others().find(|e| {
        e.arrival.sha256 == arrival.sha256
            && matches!(e.outcome, Settled::Accepted | Settled::Quarantined)
    }) {
        return Verdict::Duplicate { of: entry.clone() };
    }
    let rival = others().find(|e| {
        matches!(e.outcome, Settled::Accepted | Settled::Quarantined)
            && (e.arrival.delivered_at - arrival.delivered_at).abs() <= window
    });
    match rival {
        Some(rival) if precedence.rank(rival.arrival.source) < precedence.rank(arrival.source) => {
            Verdict::Superseded { by: rival.clone() }
        }
        rival => Verdict::Proceed {
            disagrees_with: rival.cloned(),
        },
    }
}

/// The multi-source state shared by every lane: the ledger, precedence and
/// the window. Exists only while the bucket source is on.
#[derive(Debug)]
pub(crate) struct MultiSource {
    ledger: PathBuf,
    pub precedence: Precedence,
    pub window: chrono::Duration,
    /// `(source, sha256)` of arrivals already counted as a disagreement by
    /// this process, so a retried one is counted once.
    counted: Mutex<HashSet<(SourceId, String)>>,
}

impl MultiSource {
    pub(crate) fn new(storage_dir: &Path, precedence: Precedence, window_minutes: u32) -> Self {
        Self {
            ledger: storage_dir.join(LEDGER_FILE),
            precedence,
            window: chrono::Duration::minutes(i64::from(window_minutes)),
            counted: Mutex::new(HashSet::new()),
        }
    }

    /// Registers the disagreement counter at 0.
    pub(crate) fn register_metrics() {
        for kind in DISAGREEMENT_KINDS {
            metrics::counter!(common::metrics::metric_name(DISAGREEMENT_METRIC), "kind" => kind)
                .increment(0);
        }
    }

    /// Every readable ledger entry, oldest first. A corrupt line is
    /// skipped; a missing ledger is empty.
    pub(crate) fn entries(&self) -> Vec<Entry> {
        let contents = match std::fs::read_to_string(&self.ledger) {
            Ok(contents) => contents,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(err) => {
                tracing::warn!(error = %err, "failed to read the delivery source ledger; judging without it");
                return Vec::new();
            }
        };
        contents
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    /// Judges `arrival`, logging and counting a disagreement (once per
    /// arrival per process).
    pub(crate) fn judge(&self, arrival: &Arrival) -> Verdict {
        let verdict = judge(&self.entries(), arrival, self.precedence, self.window);
        let rival = match &verdict {
            Verdict::Superseded { by } => Some(by),
            Verdict::Proceed {
                disagrees_with: Some(rival),
            } => Some(rival),
            _ => None,
        };
        if let Some(rival) = rival {
            let first = self
                .counted
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert((arrival.source, arrival.sha256.clone()));
            if first {
                metrics::counter!(
                    common::metrics::metric_name(DISAGREEMENT_METRIC),
                    "kind" => arrival.kind.metric_label()
                )
                .increment(1);
                let winner = if matches!(verdict, Verdict::Superseded { .. }) {
                    rival.arrival.source
                } else {
                    arrival.source
                };
                tracing::warn!(
                    kind = arrival.kind.metric_label(),
                    source = arrival.source.as_str(),
                    file = %arrival.name,
                    bytes = arrival.bytes,
                    sha256 = %arrival.sha256,
                    delivered_at = %arrival.delivered_at,
                    other_source = rival.arrival.source.as_str(),
                    other_file = %rival.arrival.name,
                    other_bytes = rival.arrival.bytes,
                    other_sha256 = %rival.arrival.sha256,
                    other_delivered_at = %rival.arrival.delivered_at,
                    winner = winner.as_str(),
                    "the two delivery sources delivered different content within the disagreement window; SOURCE_PRECEDENCE decides"
                );
            }
        }
        verdict
    }

    /// Appends a decision. Best effort: a failure only loses dedup for
    /// this file, and is logged.
    pub(crate) fn record(&self, arrival: &Arrival, outcome: Settled) {
        if let Err(err) = self.append(&Entry {
            arrival: arrival.clone(),
            outcome,
        }) {
            tracing::warn!(error = %err, file = %arrival.name, "failed to record a delivery decision in the source ledger");
        }
    }

    fn append(&self, entry: &Entry) -> anyhow::Result<()> {
        if let Some(parent) = self.ledger.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut entries = self.entries();
        entries.push(entry.clone());
        if entries.len() >= 2 * LEDGER_KEEP {
            let excess = entries.len() - LEDGER_KEEP;
            entries.drain(..excess);
            let temp = self.ledger.with_extension("jsonl.tmp");
            {
                let mut out = std::fs::File::create(&temp)?;
                for entry in &entries {
                    writeln!(out, "{}", serde_json::to_string(entry)?)?;
                }
                out.sync_all()?;
            }
            std::fs::rename(&temp, &self.ledger)?;
        } else {
            let mut out = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.ledger)?;
            writeln!(out, "{}", serde_json::to_string(entry)?)?;
            out.sync_all()?;
        }
        Ok(())
    }
}

/// One delivery source as the scan and CORPUS cycles see it: its landing
/// folder (`WATCH_DIR` for SFTP, `sources/bucket/incoming` for the bucket),
/// its stability gate, and its context.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Lane<'a> {
    pub ctx: LaneCtx<'a>,
    pub watch_dir: &'a Path,
    pub stability_cycles: u32,
}

#[cfg(test)]
impl Lane<'_> {
    /// SFTP alone, as before the bucket source existed (tests).
    pub(crate) fn sftp_only(config: &crate::config::Config) -> Lane<'_> {
        Lane {
            ctx: LaneCtx::SFTP_ONLY,
            watch_dir: &config.watch_dir,
            stability_cycles: config.stability_cycles,
        }
    }
}

/// One lane's view: which source it is, and the multi-source state when
/// the bucket is on (`None`: SFTP only, every path as before).
#[derive(Debug, Clone, Copy)]
pub(crate) struct LaneCtx<'a> {
    pub source: SourceId,
    pub multi: Option<&'a MultiSource>,
}

impl LaneCtx<'_> {
    /// SFTP alone: no ledger, no `source` in audit lines (tests; `run`
    /// builds each lane's context itself).
    #[cfg(test)]
    pub(crate) const SFTP_ONLY: LaneCtx<'static> = LaneCtx {
        source: SourceId::Sftp,
        multi: None,
    };

    /// The audit line's provenance fields, only with several sources.
    pub(crate) fn provenance(&self) -> Option<crate::audit::Provenance<'static>> {
        self.multi.map(|_| crate::audit::Provenance {
            source: self.source.as_str(),
            duplicate_of: None,
            first_source: None,
        })
    }

    /// Logs the audit line for `file` with this lane's provenance.
    pub(crate) fn audit(
        &self,
        file: &crate::audit::DeliveredFile,
        delivered_at: DateTime<Utc>,
        outcome: crate::audit::Outcome,
        reason: Option<&str>,
    ) {
        crate::audit::decision_from(
            file,
            delivered_at,
            outcome,
            reason,
            self.provenance().as_ref(),
        );
    }

    /// Records a decision in the ledger (a no-op with SFTP alone).
    pub(crate) fn record(
        &self,
        kind: Kind,
        file: &crate::audit::DeliveredFile,
        delivered_at: DateTime<Utc>,
        outcome: Settled,
    ) {
        if let Some(multi) = self.multi {
            multi.record(&self.arrival(kind, file, delivered_at), outcome);
        }
    }

    pub(crate) fn arrival(
        &self,
        kind: Kind,
        file: &crate::audit::DeliveredFile,
        delivered_at: DateTime<Utc>,
    ) -> Arrival {
        Arrival {
            kind,
            source: self.source,
            name: file.name.clone(),
            bytes: file.bytes,
            sha256: file.sha256.clone(),
            delivered_at,
        }
    }

    /// Logs the audit line for an arrival the ledger settled without
    /// ingesting it (`duplicate` or `superseded`), and records it.
    pub(crate) fn settle_unloaded(
        &self,
        kind: Kind,
        file: &crate::audit::DeliveredFile,
        delivered_at: DateTime<Utc>,
        verdict: &Verdict,
    ) {
        let Some(multi) = self.multi else {
            return;
        };
        let (outcome, settled, other, reason) = match verdict {
            Verdict::Duplicate { of } => (
                crate::audit::Outcome::Duplicate,
                Settled::Duplicate,
                of,
                format!(
                    "the same content (sha256) was already handled through {}",
                    of.arrival.source.as_str()
                ),
            ),
            Verdict::Superseded { by } => (
                crate::audit::Outcome::Superseded,
                Settled::Superseded,
                by,
                format!(
                    "{} delivered different content within the disagreement window and wins (SOURCE_PRECEDENCE); not ingested",
                    by.arrival.source.as_str()
                ),
            ),
            Verdict::Handled(_) | Verdict::Proceed { .. } => return,
        };
        crate::audit::decision_from(
            file,
            delivered_at,
            outcome,
            Some(&reason),
            Some(&crate::audit::Provenance {
                source: self.source.as_str(),
                duplicate_of: Some(&other.arrival.sha256),
                first_source: Some(other.arrival.source.as_str()),
            }),
        );
        multi.record(&self.arrival(kind, file, delivered_at), settled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minute: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(1_790_000_000 + minute * 60, 0).unwrap()
    }

    fn arrival(source: SourceId, sha: &str, minute: i64) -> Arrival {
        Arrival {
            kind: Kind::Cif,
            source,
            name: "timetable_full.zip".to_string(),
            bytes: 10,
            sha256: sha.to_string(),
            delivered_at: at(minute),
        }
    }

    fn entry(source: SourceId, sha: &str, minute: i64, outcome: Settled) -> Entry {
        Entry {
            arrival: arrival(source, sha, minute),
            outcome,
        }
    }

    const WINDOW: chrono::Duration = chrono::Duration::minutes(120);

    #[test]
    fn source_strings_are_the_contract_values() {
        assert_eq!(SourceId::Sftp.as_str(), "sftp");
        assert_eq!(SourceId::Bucket.as_str(), "bucket");
        assert_eq!(
            serde_json::to_string(&SourceId::Bucket).unwrap(),
            "\"bucket\""
        );
        assert_eq!(Kind::Cif.metric_label(), "cif");
        assert_eq!(Kind::CorpusXml.metric_label(), "corpus");
    }

    #[test]
    fn precedence_parses_only_a_permutation() {
        assert_eq!(
            "bucket,sftp".parse::<Precedence>().unwrap(),
            Precedence::default()
        );
        let sftp_first: Precedence = "sftp, bucket".parse().unwrap();
        assert_eq!(sftp_first.rank(SourceId::Sftp), 0);
        assert_eq!(sftp_first.rank(SourceId::Bucket), 1);
        for bad in ["", "bucket", "bucket,bucket", "bucket,sftp,sftp", "s3,sftp"] {
            assert!(bad.parse::<Precedence>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_same_content_through_the_other_source_is_a_duplicate() {
        let ledger = [entry(SourceId::Sftp, "aa", 0, Settled::Accepted)];
        let verdict = judge(
            &ledger,
            &arrival(SourceId::Bucket, "aa", 3),
            Precedence::default(),
            WINDOW,
        );
        assert_eq!(
            verdict,
            Verdict::Duplicate {
                of: ledger[0].clone()
            }
        );
        // Days later too: content, not time, makes a duplicate.
        assert!(matches!(
            judge(
                &ledger,
                &arrival(SourceId::Bucket, "aa", 60 * 24 * 3),
                Precedence::default(),
                WINDOW
            ),
            Verdict::Duplicate { .. }
        ));
    }

    #[test]
    fn the_same_content_again_through_the_same_source_is_not_a_duplicate() {
        let ledger = [entry(SourceId::Sftp, "aa", 0, Settled::Accepted)];
        assert_eq!(
            judge(
                &ledger,
                &arrival(SourceId::Sftp, "aa", 60 * 24),
                Precedence::default(),
                WINDOW
            ),
            Verdict::Proceed {
                disagrees_with: None
            }
        );
    }

    #[test]
    fn an_already_decided_file_is_handled() {
        let ledger = [entry(SourceId::Bucket, "aa", 5, Settled::Duplicate)];
        assert!(matches!(
            judge(
                &ledger,
                &arrival(SourceId::Bucket, "aa", 5),
                Precedence::default(),
                WINDOW
            ),
            Verdict::Handled(_)
        ));
    }

    /// Arrival order x precedence x the earlier copy's outcome.
    #[test]
    fn disagreement_table() {
        let bucket_first = Precedence::default();
        let sftp_first: Precedence = "sftp,bucket".parse().unwrap();
        for (precedence, earlier, outcome, later, expect_superseded) in [
            // D5: the bucket wins whichever arrives first.
            (
                bucket_first,
                SourceId::Sftp,
                Settled::Accepted,
                SourceId::Bucket,
                false,
            ),
            (
                bucket_first,
                SourceId::Bucket,
                Settled::Accepted,
                SourceId::Sftp,
                true,
            ),
            // A quarantined winner never promotes the loser.
            (
                bucket_first,
                SourceId::Bucket,
                Settled::Quarantined,
                SourceId::Sftp,
                true,
            ),
            // The other way round.
            (
                sftp_first,
                SourceId::Sftp,
                Settled::Accepted,
                SourceId::Bucket,
                true,
            ),
            (
                sftp_first,
                SourceId::Bucket,
                Settled::Accepted,
                SourceId::Sftp,
                false,
            ),
        ] {
            let ledger = [entry(earlier, "aa", 0, outcome)];
            let verdict = judge(&ledger, &arrival(later, "bb", 30), precedence, WINDOW);
            if expect_superseded {
                assert_eq!(
                    verdict,
                    Verdict::Superseded {
                        by: ledger[0].clone()
                    },
                    "{precedence:?} {earlier:?} then {later:?}"
                );
            } else {
                assert_eq!(
                    verdict,
                    Verdict::Proceed {
                        disagrees_with: Some(ledger[0].clone())
                    },
                    "{precedence:?} {earlier:?} then {later:?}"
                );
            }
        }
    }

    #[test]
    fn outside_the_window_the_later_delivery_is_just_newer() {
        let ledger = [entry(SourceId::Bucket, "aa", 0, Settled::Accepted)];
        assert_eq!(
            judge(
                &ledger,
                &arrival(SourceId::Sftp, "bb", 121),
                Precedence::default(),
                WINDOW
            ),
            Verdict::Proceed {
                disagrees_with: None
            }
        );
        // An earlier arrival within the window counts too (|difference|).
        assert!(matches!(
            judge(
                &ledger,
                &arrival(SourceId::Sftp, "bb", -100),
                Precedence::default(),
                WINDOW
            ),
            Verdict::Superseded { .. }
        ));
    }

    #[test]
    fn a_superseded_or_duplicate_copy_is_never_a_rival() {
        let ledger = [
            entry(SourceId::Bucket, "aa", 0, Settled::Superseded),
            entry(SourceId::Bucket, "cc", 1, Settled::Duplicate),
        ];
        assert_eq!(
            judge(
                &ledger,
                &arrival(SourceId::Sftp, "bb", 2),
                Precedence::default(),
                WINDOW
            ),
            Verdict::Proceed {
                disagrees_with: None
            }
        );
    }

    #[test]
    fn corpus_formats_are_separate_streams() {
        let mut json = entry(SourceId::Bucket, "aa", 0, Settled::Accepted);
        json.arrival.kind = Kind::CorpusJson;
        let mut xml = arrival(SourceId::Sftp, "bb", 1);
        xml.kind = Kind::CorpusXml;
        assert_eq!(
            judge(&[json], &xml, Precedence::default(), WINDOW),
            Verdict::Proceed {
                disagrees_with: None
            }
        );
    }

    #[test]
    fn the_ledger_round_trips_skips_corrupt_lines_and_is_compacted() {
        let dir = tempfile::tempdir().unwrap();
        let multi = MultiSource::new(dir.path(), Precedence::default(), 120);
        assert!(multi.entries().is_empty());
        multi.record(&arrival(SourceId::Sftp, "aa", 0), Settled::Accepted);
        std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join(LEDGER_FILE))
            .unwrap()
            .write_all(b"{not json\n")
            .unwrap();
        multi.record(&arrival(SourceId::Bucket, "aa", 1), Settled::Duplicate);
        let entries = multi.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1].outcome, Settled::Duplicate);
        for i in 0..(2 * LEDGER_KEEP) {
            multi.record(
                &arrival(SourceId::Sftp, &format!("{i:x}"), 0),
                Settled::Accepted,
            );
        }
        let entries = multi.entries();
        assert!(entries.len() < 2 * LEDGER_KEEP);
        assert!(entries.len() >= LEDGER_KEEP);
        assert_eq!(
            entries.last().unwrap().arrival.sha256,
            format!("{:x}", 2 * LEDGER_KEEP - 1)
        );
    }

    #[test]
    fn a_disagreement_is_counted_once_per_arrival() {
        let dir = tempfile::tempdir().unwrap();
        let multi = MultiSource::new(dir.path(), Precedence::default(), 120);
        multi.record(&arrival(SourceId::Bucket, "aa", 0), Settled::Accepted);
        let late = arrival(SourceId::Sftp, "bb", 10);
        assert!(matches!(multi.judge(&late), Verdict::Superseded { .. }));
        assert!(matches!(multi.judge(&late), Verdict::Superseded { .. }));
        assert_eq!(multi.counted.lock().unwrap().len(), 1);
    }
}
