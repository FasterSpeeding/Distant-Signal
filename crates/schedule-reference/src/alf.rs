//! Pure CIF `ALF` (Additional Fixed Links) record parsing -- one
//! comma-separated `key=value` line per fixed link (a walk, tube, bus, tram
//! or ferry connection between two CRS codes), e.g.:
//!
//!     M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2359,P=4,R=0000001
//!
//! This is a genuinely different record shape from every other CIF member
//! this crate reads: `MCA`/`MSN` are fixed-width, byte-offset records
//! (`parser.rs`); `ALF` is comma-delimited key=value pairs, so this module
//! does not import or reuse `parser.rs`'s byte-slice helpers. Real line
//! shape and field meanings confirmed against the sibling `Distant-Signal-MCP`
//! project's own `src/timetable/cif/alf.ts` (re-cloned and independently
//! re-read for this plan's own research pass, not carried forward
//! unverified) -- see this plan's own header for the exact commit
//! provenance. `P` (a real field on every quoted row) is deliberately
//! never parsed -- see this plan's Judgment Call 6.
//!
//! No I/O here -- same "parsing logic pure and testable separately from
//! I/O" convention `parser.rs`'s own module doc establishes.

/// One parsed `ALF` fixed-link record. `from_crs`/`to_crs` are CRS codes
/// (NOT TIPLOCs -- a different identifier space from every other record
/// this crate parses, and a real source of join bugs if the two are
/// confused when Phase 2 consumes this data against a TIPLOC-keyed
/// connections array).
// Staged for Phase 2 (Task 6) wiring — not yet called by this Task 3
// (the bin target has no direct consumer). Allowed rather than deleted:
// the interchange-logic tests in Phase 2 will validate it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct ParsedFixedLink {
    pub mode: String,
    pub from_crs: String,
    pub to_crs: String,
    pub minutes: i32,
    /// Raw `HHMM`, 4 ASCII digits, e.g. `"0001"`. Not parsed into a
    /// `NaiveTime` here -- Phase 2's read-side interchange logic is where
    /// this gets compared against a query's own clock time, matching this
    /// codebase's "store the raw CIF value, interpret at read time"
    /// convention (this plan's Judgment Call 3).
    pub valid_from: String,
    pub valid_to: String,
    /// Raw 7-character `'0'`/`'1'` bitmask, Monday-first -- the same
    /// day-of-week convention `schedule_query::records::BasicSchedule::days_of_week`
    /// already uses for the CIF `SCHEDULE` member's own days-run bitmask,
    /// stored here as text rather than `[bool; 7]` since nothing in this
    /// crate needs to inspect individual days at ingest time.
    pub days_mask: String,
}

/// Parses one `ALF` line. Returns `None` for a blank line or a `/!!`-prefixed
/// comment line (both real, confirmed present in a real extract) -- neither
/// is a parse failure worth logging on every cycle for a normal,
/// well-formed file. Returns `None` (logged by the caller, not this pure
/// function -- see `main.rs`'s wiring) for a line that has link-like shape
/// but is missing a required key, rather than panicking the whole
/// extraction -- this crate's own established "skip malformed, never abort"
/// convention (this plan's Judgment Call 5), diverging deliberately from
/// the sibling project's own `throw`-on-missing-field posture.
// Staged for Phase 2 (Task 6) wiring — not yet called by this crate.
// Allowed rather than deleted: the line-parsing tests in this module
// validate it.
#[allow(dead_code)]
pub fn parse_alf_line(line: &str) -> Option<ParsedFixedLink> {
    let text = line.trim();
    if text.is_empty() || text.starts_with("/!!") {
        return None;
    }

    let mut values: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for pair in text.split(',') {
        if let Some((key, value)) = pair.split_once('=') {
            values.insert(key.trim(), value.trim());
        }
    }

    let mode = values.get("M")?.to_string();
    let from_crs = values.get("O")?.to_string();
    let to_crs = values.get("D")?.to_string();
    let minutes: i32 = values.get("T")?.parse().ok()?;
    if minutes < 0 {
        return None;
    }
    let valid_from = values.get("S")?.to_string();
    let valid_to = values.get("E")?.to_string();
    let days_mask = values.get("R")?.to_string();

    Some(ParsedFixedLink {
        mode,
        from_crs,
        to_crs,
        minutes,
        valid_from,
        valid_to,
        days_mask,
    })
}

/// Every successfully-parsed link in `text`, one call per already-read-into-memory
/// ALF file (mirrors `parser::parse_ti_lines`'s own "whole file as one
/// `&str` in, `Vec` out" shape). A malformed line simply contributes
/// nothing to the result -- see [`parse_alf_line`]'s own doc comment.
// Staged for Phase 2 (Task 6) wiring — not yet called by this crate.
// Allowed rather than deleted: the integration test in this module
// validates it.
#[allow(dead_code)]
pub fn parse_alf_lines(text: &str) -> Vec<ParsedFixedLink> {
    text.lines().filter_map(parse_alf_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real quoted line, re-verified against the sibling project's own
    // `src/timetable/cif/alf.ts` module doc during this plan's own
    // research pass (that project's own doc states this line's provenance
    // as a real extract; not independently re-extracted from a live
    // delivery by this pass, since none was available -- see this plan's
    // own header note).
    const REAL_LINE: &str = "M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2359,P=4,R=0000001";

    #[test]
    fn parses_a_real_fixed_link_line() {
        let link = parse_alf_line(REAL_LINE).expect("real line parses");
        assert_eq!(link.mode, "WALK");
        assert_eq!(link.from_crs, "AFK");
        assert_eq!(link.to_crs, "ASI");
        assert_eq!(link.minutes, 5);
        assert_eq!(link.valid_from, "0001");
        assert_eq!(link.valid_to, "2359");
        assert_eq!(link.days_mask, "0000001");
    }

    #[test]
    fn a_blank_line_is_skipped() {
        assert_eq!(parse_alf_line(""), None);
        assert_eq!(parse_alf_line("   "), None);
    }

    #[test]
    fn a_comment_line_is_skipped() {
        assert_eq!(parse_alf_line("/!! Sequence: 904"), None);
    }

    #[test]
    fn a_line_missing_a_required_field_is_skipped_not_a_panic() {
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,S=0001,E=2359,R=0000001"),
            None
        );
    }

    #[test]
    fn a_negative_transfer_time_is_rejected() {
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=-5,S=0001,E=2359,R=0000001"),
            None
        );
    }

    #[test]
    fn parse_alf_lines_skips_blanks_and_comments_and_keeps_real_links() {
        let text = format!("/!! Sequence: 904\n\n{REAL_LINE}\n");
        let links = parse_alf_lines(&text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].mode, "WALK");
    }
}
