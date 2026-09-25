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
#[derive(Debug, Clone, PartialEq, Eq)]
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

    // A key present with an empty value (e.g. `M=,O=AFK,...`) is treated the
    // same as the key being missing entirely -- both are "not a usable
    // value" for a required string field. This matches this module's own
    // "skip malformed, never abort" posture (Judgment Call 5): rather than
    // let an empty string flow through into a `NOT NULL` column as silent
    // junk, a blank required field simply fails this line's parse, exactly
    // like a genuinely absent key already does.
    fn non_empty(values: &std::collections::HashMap<&str, &str>, key: &str) -> Option<String> {
        let value = *values.get(key)?;
        if value.is_empty() {
            return None;
        }
        Some(value.to_string())
    }

    let mode = non_empty(&values, "M")?;
    let from_crs = non_empty(&values, "O")?;
    let to_crs = non_empty(&values, "D")?;
    let minutes: i32 = values.get("T")?.parse().ok()?;
    if minutes < 0 {
        return None;
    }
    let valid_from = non_empty(&values, "S")?;
    let valid_to = non_empty(&values, "E")?;
    let days_mask = non_empty(&values, "R")?;

    // **2026-09-25 fix.** Every field above this point was validated to at
    // least the extent of "present and non-empty"; `valid_from`/`valid_to`/
    // `days_mask` were not -- ANY non-empty string, however malformed, used
    // to sail straight through into the `fixed_links` table unchecked. That
    // is not a cosmetic gap: `schedule_query::interchange::fixed_links_from`
    // (the only place this app ever actually consults a link's validity
    // window) compares `valid_from`/`valid_to` as plain `&str` -- not parsed
    // `NaiveTime`s -- specifically because zero-padded 4-digit `HHMM` text
    // sorts identically to the times it represents. A value that ISN'T
    // exactly 4 zero-padded digits breaks that assumption silently: it
    // doesn't reject the link, it makes the `<=`/`<=` window comparison
    // compare wrong -- which can just as easily make a link that should have
    // stopped applying keep comparing as always-in-window (i.e. "expired,
    // but treated as permanent") as the reverse. Validating the real
    // `HH`/`MM` shape here, at the one place this crate actually parses ALF
    // text, closes that gap for good rather than leaving every read-side
    // caller to defend against a write-side format it has no way to verify
    // itself. `days_mask` gets the matching check -- `fixed_links_from`
    // reads it as a `'1'`-or-not byte per weekday index (`interchange.rs`'s
    // own `link.days_mask.as_bytes().get(day)`), which fails safe on a
    // wrong-LENGTH mask (missing days silently never match) but not on a
    // wrong-CHARACTER one, e.g. a mask using `'Y'`/`'N'` instead of `'1'`/
    // `'0'` would silently decode as "never runs any day" instead of being
    // caught as the malformed row it is.
    //
    // Matches this module's own established "skip malformed, never abort"
    // posture (Judgment Call 5): an ALF row whose validity fields don't
    // parse is simply not one of the links this cycle publishes, exactly
    // like a row missing a required key outright.
    if !is_valid_hhmm(&valid_from) || !is_valid_hhmm(&valid_to) {
        return None;
    }
    if !is_valid_days_mask(&days_mask) {
        return None;
    }

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

/// Is `value` a well-formed CIF fixed-link time-of-day: exactly 4 ASCII
/// digits, `HH` in `00..=23` and `MM` in `00..=59`? The shape this module's
/// own real quoted line's `S=0001`/`E=2359` values have, and the shape
/// `schedule_query::interchange::fixed_links_from`'s read-side `&str`
/// comparison needs in order to sort correctly against a same-format clock
/// string -- see [`parse_alf_line`]'s own doc comment (2026-09-25 fix) for
/// the failure this closes.
fn is_valid_hhmm(value: &str) -> bool {
    if value.len() != 4 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let hour: u32 = value[0..2].parse().unwrap_or(u32::MAX);
    let minute: u32 = value[2..4].parse().unwrap_or(u32::MAX);
    hour <= 23 && minute <= 59
}

/// Is `value` a well-formed CIF days-of-week bitmask: exactly 7 bytes, each
/// literally `'0'` or `'1'`? Mirrors [`is_valid_hhmm`]'s own reasoning: the
/// read side (`fixed_links_from`) indexes this string by weekday and checks
/// the byte at that index against `b'1'` directly, so a mask using any other
/// character to mean "runs" would silently decode as "never runs" instead of
/// being caught as the malformed row it is.
fn is_valid_days_mask(value: &str) -> bool {
    value.len() == 7 && value.bytes().all(|b| b == b'0' || b == b'1')
}

/// Every successfully-parsed link in `text`, one call per already-read-into-memory
/// ALF file (mirrors `parser::parse_ti_lines`'s own "whole file as one
/// `&str` in, `Vec` out" shape). A malformed line simply contributes
/// nothing to the result -- see [`parse_alf_line`]'s own doc comment.
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
    fn a_line_with_a_present_but_blank_required_field_is_skipped_not_stored_as_empty_string() {
        // Distinct from the MISSING-key case above: here every required key
        // is present, but `M`'s value is the empty string -- e.g. a
        // truncated or malformed real line. Without the `non_empty` guard,
        // this would have parsed successfully into a `ParsedFixedLink` with
        // `mode: String::new()`, landing as `NOT NULL` empty-string junk in
        // the `fixed_links` table.
        assert_eq!(
            parse_alf_line("M=,O=AFK,D=ASI,T=5,S=0001,E=2359,R=0000001"),
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

    // --- Validity-window format validation (2026-09-25 fix) -------------
    //
    // Before this fix, `valid_from`/`valid_to`/`days_mask` were accepted as
    // long as they were non-empty -- no check that they were actually the
    // 4-digit `HHMM`/7-char `'0'`/`'1'` shape
    // `schedule_query::interchange::fixed_links_from`'s read-side `&str`
    // comparison depends on to sort/index correctly. A malformed value there
    // doesn't fail loudly; it makes that comparison silently wrong, which
    // can present as a link that should have expired continuing to compare
    // as always-valid.

    #[test]
    fn a_valid_from_that_is_not_four_digits_is_rejected() {
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=1,E=2359,R=0000001"),
            None,
            "a 1-digit start time must not silently become a 4-digit comparison string"
        );
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=001,E=2359,R=0000001"),
            None
        );
    }

    #[test]
    fn a_non_numeric_valid_from_is_rejected() {
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=AB01,E=2359,R=0000001"),
            None
        );
    }

    #[test]
    fn an_out_of_range_hour_or_minute_is_rejected() {
        // `24xx` and `xx60` are not real clock times, however digit-shaped.
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=2400,E=2359,R=0000001"),
            None,
            "hour 24 does not exist"
        );
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2360,R=0000001"),
            None,
            "minute 60 does not exist"
        );
    }

    #[test]
    fn a_valid_to_that_is_not_four_digits_is_rejected() {
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=0001,E=99,R=0000001"),
            None
        );
    }

    #[test]
    fn a_days_mask_that_is_not_seven_bits_is_rejected() {
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2359,R=000001"),
            None,
            "6 characters, one short of a real Monday-first week"
        );
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2359,R=00000010"),
            None,
            "8 characters, one over"
        );
    }

    #[test]
    fn a_days_mask_with_a_non_bit_character_is_rejected() {
        // A mask using e.g. 'Y'/'N' instead of '1'/'0' must not silently
        // decode as "never runs any day" -- see this fix's own doc comment.
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2359,R=0000Y00"),
            None
        );
    }

    #[test]
    fn boundary_valid_times_0000_and_2359_are_accepted() {
        // The exact values this module's own real quoted line uses --
        // pinned here so the format check above can never regress into
        // rejecting real, well-formed data.
        let link = parse_alf_line("M=WALK,O=AFK,D=ASI,T=5,S=0000,E=2359,R=1111111")
            .expect("0000/2359 are the real, valid boundary values");
        assert_eq!(link.valid_from, "0000");
        assert_eq!(link.valid_to, "2359");
    }

    #[test]
    fn parse_alf_lines_skips_blanks_and_comments_and_keeps_real_links() {
        let text = format!("/!! Sequence: 904\n\n{REAL_LINE}\n");
        let links = parse_alf_lines(&text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].mode, "WALK");
    }
}
