//! Provenance and the per-file audit line (Ranma's
//! sftp-audit-observability spec; docs/schedule-feed-sftp.md).
//!
//! Every file DTD's SFTP push delivers is hashed (SHA-256) as it is read,
//! and each decision about it -- loaded, quarantined, refused by api,
//! rejected as CORPUS -- is logged as exactly one INFO line with target
//! `schedule_ingest::audit` and message `delivery decision`. Alloy routes
//! that target into the long-retention audit stream, so the field names
//! below are a contract:
//! `file`, `bytes`, `sha256`, `delivered_at`, `outcome`, `reason` (absent
//! when the file was accepted). The line joins `SFTPGo`'s `Upload` line on file
//! name, size and time: `delivered_at` is the file's mtime, which `SFTPGo`
//! sets when the upload closes.

use std::io::Read;
use std::path::Path;

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};

/// One delivered file as this process read it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DeliveredFile {
    pub name: String,
    pub bytes: u64,
    /// Lowercase hex SHA-256 of the file's contents.
    pub sha256: String,
}

impl DeliveredFile {
    /// Hashes `bytes`, already read in full (the CORPUS path).
    pub(crate) fn from_bytes(name: &str, bytes: &[u8]) -> Self {
        Self {
            name: name.to_string(),
            bytes: bytes.len() as u64,
            sha256: hex(&Sha256::digest(bytes)),
        }
    }

    /// Streams the file at `path` through SHA-256 (the CIF zip, ~77 MB, is
    /// never held in memory).
    pub(crate) fn hash_file(name: &str, path: &Path) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut total: u64 = 0;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            total += n as u64;
        }
        Ok(Self {
            name: name.to_string(),
            bytes: total,
            sha256: hex(&hasher.finalize()),
        })
    }
}

/// A [`std::io::Write`] that hashes everything written through it, for
/// hashing each zip entry while it is extracted.
pub(crate) struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
}

impl<W> HashingWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
        }
    }

    /// The writer back, and the lowercase hex SHA-256 of what went through.
    pub(crate) fn finish(self) -> (W, String) {
        (self.inner, hex(&self.hasher.finalize()))
    }
}

impl<W: std::io::Write> std::io::Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hasher.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn hex(digest: &[u8]) -> String {
    use std::fmt::Write;
    digest.iter().fold(String::with_capacity(64), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// What happened to a delivered file. The strings are the `outcome` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Checked, extracted (CIF) or loaded (CORPUS), and recorded by api.
    Accepted,
    /// A CIF zip that failed a check and will not be retried until a new
    /// upload replaces it.
    Quarantined,
    /// api refused the delivery record (400/413/422).
    RejectedByApi,
    /// A CORPUS file that failed its checks, moved to the rejected archive.
    CorpusRejected,
}

impl Outcome {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Quarantined => "quarantined",
            Self::RejectedByApi => "rejected_by_api",
            Self::CorpusRejected => "corpus_rejected",
        }
    }
}

/// Logs the one audit line for `file`. `reason` is required for every
/// outcome but [`Outcome::Accepted`], and omitted for it.
pub(crate) fn decision(
    file: &DeliveredFile,
    delivered_at: DateTime<Utc>,
    outcome: Outcome,
    reason: Option<&str>,
) {
    let delivered_at = delivered_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    if let Some(reason) = reason {
        tracing::info!(
            target: "schedule_ingest::audit",
            file = %file.name,
            bytes = file.bytes,
            sha256 = %file.sha256,
            delivered_at = %delivered_at,
            outcome = outcome.as_str(),
            reason = %reason,
            "delivery decision"
        );
    } else {
        tracing::info!(
            target: "schedule_ingest::audit",
            file = %file.name,
            bytes = file.bytes,
            sha256 = %file.sha256,
            delivered_at = %delivered_at,
            outcome = outcome.as_str(),
            "delivery decision"
        );
    }
}

#[cfg(test)]
#[expect(
    clippy::cast_sign_loss,
    reason = "test code: casts of small known test values"
)]
pub(crate) mod tests {
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// Captures the JSON lines `common::logging` writes, for asserting the
    /// audit line's exact shape.
    #[derive(Clone, Default)]
    pub(crate) struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
        type Writer = Capture;
        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    impl Capture {
        /// Every captured line with the audit target, parsed.
        pub(crate) fn audit_lines(&self) -> Vec<serde_json::Value> {
            let text = String::from_utf8(self.0.lock().unwrap().clone()).unwrap();
            text.lines()
                .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
                .filter(|line| line["target"] == "schedule_ingest::audit")
                .collect()
        }
    }

    /// Runs `f` with the production JSON subscriber writing into a capture.
    pub(crate) fn capture<T>(f: impl FnOnce() -> T) -> (T, Capture) {
        let (guard, out) = capture_default();
        let result = f();
        drop(guard);
        (result, out)
    }

    /// Installs the production JSON subscriber, writing into a capture, as
    /// this thread's default until the guard drops (for `#[tokio::test]`'s
    /// single-threaded runtime).
    pub(crate) fn capture_default() -> (tracing::subscriber::DefaultGuard, Capture) {
        let out = Capture::default();
        let subscriber = common::logging::json_subscriber(
            "schedule-ingest",
            tracing_subscriber::EnvFilter::new("info"),
            out.clone(),
        );
        (tracing::subscriber::set_default(subscriber), out)
    }

    #[test]
    fn hashes_match_the_known_sha256_vectors() {
        let empty = DeliveredFile::from_bytes("e", b"");
        assert_eq!(
            empty.sha256,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let abc = DeliveredFile::from_bytes("abc", b"abc");
        assert_eq!(
            abc.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(abc.bytes, 3);
    }

    #[test]
    fn hashing_a_file_streams_it_and_agrees_with_hashing_its_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("timetable_full.zip");
        // Over one read buffer, so the loop runs more than once.
        let content: Vec<u8> = (0..(3 << 20)).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &content).unwrap();
        let streamed = DeliveredFile::hash_file("timetable_full.zip", &path).unwrap();
        assert_eq!(
            streamed,
            DeliveredFile::from_bytes("timetable_full.zip", &content)
        );
    }

    #[test]
    fn the_hashing_writer_hashes_what_it_writes() {
        let mut writer = HashingWriter::new(Vec::new());
        writer.write_all(b"ab").unwrap();
        writer.write_all(b"c").unwrap();
        let (inner, sha) = writer.finish();
        assert_eq!(inner, b"abc");
        assert_eq!(sha, DeliveredFile::from_bytes("x", b"abc").sha256);
    }

    /// The exact line Alloy and the runbook depend on.
    #[test]
    fn a_decision_is_one_json_line_with_the_contract_fields() {
        let file = DeliveredFile::from_bytes("timetable_full.zip", b"abc");
        let at = DateTime::parse_from_rfc3339("2026-09-30T19:59:59Z")
            .unwrap()
            .with_timezone(&Utc);
        let ((), out) = capture(|| {
            decision(&file, at, Outcome::Accepted, None);
            decision(
                &file,
                at,
                Outcome::Quarantined,
                Some("MCA has 10 schedules"),
            );
        });
        let lines = out.audit_lines();
        assert_eq!(lines.len(), 2);
        let accepted = &lines[0];
        assert_eq!(accepted["level"], "INFO");
        assert_eq!(accepted["target"], "schedule_ingest::audit");
        assert_eq!(accepted["message"], "delivery decision");
        assert_eq!(accepted["file"], "timetable_full.zip");
        assert_eq!(accepted["bytes"], 3);
        assert_eq!(accepted["sha256"], file.sha256.as_str());
        assert_eq!(accepted["delivered_at"], "2026-09-30T19:59:59Z");
        assert_eq!(accepted["outcome"], "accepted");
        assert!(accepted.get("reason").is_none(), "{accepted}");
        assert_eq!(lines[1]["outcome"], "quarantined");
        assert_eq!(lines[1]["reason"], "MCA has 10 schedules");
    }

    #[test]
    fn outcome_strings_are_the_contract_values() {
        assert_eq!(
            [
                Outcome::Accepted,
                Outcome::Quarantined,
                Outcome::RejectedByApi,
                Outcome::CorpusRejected
            ]
            .map(Outcome::as_str),
            [
                "accepted",
                "quarantined",
                "rejected_by_api",
                "corpus_rejected"
            ]
        );
    }
}
