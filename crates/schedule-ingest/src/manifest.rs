//! Parsing for the `RJTTFnnnDAT.txt` manifest that accompanies every CIF
//! SCHEDULE feed delivery, plus sequence-number classification against the
//! last successfully ingested sequence.
//!
//! The real manifest (confirmed directly against a sample delivery during
//! this crate's own design/planning phase — never committed to this repo,
//! see the implementation plan's Global Constraints) has this exact shape:
//!
//! ```text
//! /!! Start of file                                                               \r\n
//! /!! Content type:  DAT                                                          \r\n
//! /!! Sequence:      942                                                          \r\n
//! /!! Generated:     28/08/2026                                                   \r\n
//! /!! Exporter:      RjEhrTTT                                                     \r\n
//! RJTTF942ZTR.txt
//! RJTTF942REJ.txt
//! RJTTF942SET.txt
//! RJTTF942FLF.txt
//! RJTTF942MCA.txt
//! RJTTF942MSN.txt
//! RJTTF942ALF.txt
//! RJTTF942TSI.txt
//! /!! End of file (8 records) (28/08/2026)
//! ```
//!
//! There is no byte-size or checksum field anywhere in this format — do not
//! implement or assume one. The `/!!` header/footer lines pad with trailing
//! spaces to a fixed column width in the real file; that padding is brittle
//! to match exactly and not meaningful to this parser's job, so only the
//! `/!!` prefix is used to recognize those lines. Everything else (bare
//! lines with no `/!!` prefix) is treated as a listed filename.
//!
//! Per RSPS5046 §5.2.2, the manifest correctly excludes its own filename
//! (`RJTTFnnnDAT.txt`) from its own listing — `Manifest::files` therefore
//! deliberately does not include it either.

/// A parsed `RJTTFnnnDAT.txt` manifest: the delivery's sequence number and
/// the ordered list of every *other* file the delivery contains (the
/// manifest's own filename is not included — see module docs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub sequence: u32,
    pub files: Vec<String>,
}

/// Parses a manifest's raw text content.
///
/// Recognizes `/!!`-prefixed lines as header/footer metadata (extracting
/// the sequence number from the `Sequence:` line) and every other
/// non-blank line as a listed filename. Errors if no `Sequence:` line is
/// found, if the sequence number fails to parse, or if the manifest lists
/// zero files.
pub fn parse(content: &str) -> anyhow::Result<Manifest> {
    let mut sequence: Option<u32> = None;
    let mut files = Vec::new();

    for line in content.lines() {
        let line = line.trim_end();
        if line.is_empty() {
            continue;
        }

        if let Some(rest) = line.strip_prefix("/!!") {
            if let Some(value) = rest.trim().strip_prefix("Sequence:") {
                let value = value.trim();
                let value = value.split_whitespace().next().unwrap_or(value);
                sequence = Some(
                    value
                        .parse()
                        .map_err(|error| anyhow::anyhow!("invalid Sequence value {value:?}: {error}"))?,
                );
            }
            // Every other `/!!` line (Start of file, Content type,
            // Generated, Exporter, End of file) carries no information
            // this parser needs.
            continue;
        }

        files.push(line.trim().to_string());
    }

    let sequence = sequence.ok_or_else(|| anyhow::anyhow!("manifest has no `Sequence:` line"))?;

    if files.is_empty() {
        anyhow::bail!("manifest lists zero files");
    }

    Ok(Manifest { sequence, files })
}

/// How a newly observed manifest sequence number relates to the last one
/// this service successfully ingested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceRelation {
    /// `current` matches the last ingested sequence exactly — this
    /// delivery has already been processed.
    AlreadyIngested,
    /// `current == last + 1`, or this is the very first ingest (`last` is
    /// `None`).
    Expected,
    /// Anything else — a non-contiguous jump, in either direction.
    ///
    /// The caller (the Task 5 orchestration loop, not this module) is
    /// expected to log this at `ERROR` with both sequence numbers and
    /// increment a `distant_signal_schedule_feed_sequence_gap_total`
    /// counter, but **still proceed to ingest**. Per RSPS5046 §7.4, a
    /// non-contiguous sequence number is documented, expected DTD
    /// behaviour after an "Empty" feed — not proof of a missed delivery.
    /// This function only classifies; it has no logging or metrics side
    /// effects of its own.
    Gap,
}

/// Classifies `current` against `last` (the last successfully ingested
/// sequence number, or `None` if this service has never ingested one).
///
/// See [`SequenceRelation`] for what each variant means and what the
/// caller is expected to do about a [`SequenceRelation::Gap`].
pub fn classify_sequence(last: Option<u32>, current: u32) -> SequenceRelation {
    match last {
        None => SequenceRelation::Expected,
        Some(last) if current == last => SequenceRelation::AlreadyIngested,
        Some(last) if current == last + 1 => SequenceRelation::Expected,
        Some(_) => SequenceRelation::Gap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reproduces the real manifest's confirmed shape (see module docs) —
    /// hand-written, not sourced from any real delivery.
    const FIXTURE: &str = "/!! Start of file                                                               \r\n/!! Content type:  DAT                                                          \r\n/!! Sequence:      942                                                          \r\n/!! Generated:     28/08/2026                                                   \r\n/!! Exporter:      RjEhrTTT                                                     \r\nRJTTF942ZTR.txt\r\nRJTTF942REJ.txt\r\nRJTTF942SET.txt\r\nRJTTF942FLF.txt\r\nRJTTF942MCA.txt\r\nRJTTF942MSN.txt\r\nRJTTF942ALF.txt\r\nRJTTF942TSI.txt\r\n/!! End of file (8 records) (28/08/2026)                                        \r\n";

    #[test]
    fn parses_sequence_and_files_from_the_real_shape() {
        let manifest = parse(FIXTURE).unwrap();
        assert_eq!(manifest.sequence, 942);
        assert_eq!(
            manifest.files,
            vec![
                "RJTTF942ZTR.txt",
                "RJTTF942REJ.txt",
                "RJTTF942SET.txt",
                "RJTTF942FLF.txt",
                "RJTTF942MCA.txt",
                "RJTTF942MSN.txt",
                "RJTTF942ALF.txt",
                "RJTTF942TSI.txt",
            ]
        );
        assert_eq!(manifest.files.len(), 8);
        // The manifest's own filename is never part of its listing.
        assert!(!manifest.files.iter().any(|f| f.contains("DAT")));
    }

    #[test]
    fn errors_without_a_sequence_line() {
        let fixture = "/!! Start of file\r\nRJTTF942ZTR.txt\r\n/!! End of file (1 records)\r\n";
        let error = parse(fixture).unwrap_err();
        assert!(error.to_string().contains("Sequence"));
    }

    #[test]
    fn errors_with_zero_listed_files() {
        let fixture = "/!! Start of file\r\n/!! Sequence:      942\r\n/!! End of file (0 records)\r\n";
        let error = parse(fixture).unwrap_err();
        assert!(error.to_string().contains("zero files"));
    }

    #[test]
    fn classify_first_ever_ingest_is_expected() {
        assert_eq!(classify_sequence(None, 942), SequenceRelation::Expected);
    }

    #[test]
    fn classify_same_sequence_is_already_ingested() {
        assert_eq!(
            classify_sequence(Some(942), 942),
            SequenceRelation::AlreadyIngested
        );
    }

    #[test]
    fn classify_next_sequence_is_expected() {
        assert_eq!(classify_sequence(Some(942), 943), SequenceRelation::Expected);
    }

    #[test]
    fn classify_forward_jump_is_a_gap() {
        assert_eq!(classify_sequence(Some(942), 944), SequenceRelation::Gap);
    }

    #[test]
    fn classify_backward_jump_is_a_gap() {
        assert_eq!(classify_sequence(Some(942), 941), SequenceRelation::Gap);
    }
}
