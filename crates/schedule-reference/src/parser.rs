//! Pure MSN/MCA `TI`/`A` record parsing and the STANOX disambiguation
//! policy. No I/O here -- `main.rs`'s `read_prefixed_lines` (Task 4) is the
//! only thing that touches the filesystem; everything in this module takes
//! and returns plain in-memory data, matching this repo's "keep parsing
//! logic pure and testable separately from I/O" convention (see
//! `crates/schedule-ingest/src/manifest.rs::parse`'s own shape of taking
//! `&str` rather than a path).
//!
//! See
//! docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md
//! Decision 2 for the schema and disambiguation policy this module
//! implements, and `reference-data/stanox-crs.md` for the original,
//! hand-curated version of the same policy this reimplements as real code.

use std::collections::HashMap;

/// Guards every fixed-byte-offset `&str` slice in this module against the
/// only two ways such a slice can panic: a line shorter than the offset
/// (`byte index N is out of bounds`) and a line where the offset falls
/// INSIDE a multi-byte UTF-8 character (`byte index N is not a char
/// boundary`). A `line.len() >= min_len` check alone -- which is all this
/// module had before this fix -- catches the first and not the second.
///
/// This is not hypothetical. `parse_ti_lines`/`parse_msn_a_lines`/
/// `parse_msn_change_time_by_tiploc` are each driven by `main.rs`'s
/// `read_prefixed_lines` in a loop over every matching line of a real
/// 700MB+ `RJTTF<n>MCA.txt`/`RJTTF<n>MSN.txt`, inside a long-running
/// container. One non-ASCII byte straddling one of these offsets -- in a
/// station name, a TI description, or any corrupted stretch of the file --
/// panics the whole process, and because the delivery on the read-only PVC
/// does not change, the restarted container reads the same bad line and
/// panics again: a crash loop, with every one of this service's seven
/// published products frozen for as long as it lasts.
///
/// **This is a verbatim port of `schedule_query::parse`'s own
/// `is_fixed_width_decodable`** (`crates/schedule-query/src/parse.rs`),
/// which was added to that sibling crate after coverage-guided fuzzing
/// found eight separate panic sites of exactly this shape in it. That
/// crate's copy carries the long-form reasoning; the parts that matter
/// here are repeated rather than only cross-referenced:
///
/// * The ASCII check is WHOLE-LINE, not per-field, so a line with a
///   multi-byte character anywhere -- even outside the fields this module
///   actually decodes -- rejects the whole record. That is a real widening
///   of "malformed", accepted on purpose: CIF/MSN are ASCII by
///   specification, so a line carrying non-ASCII bytes anywhere has
///   already failed the format, and a per-field check would be both slower
///   and easier to leave a hole in.
/// * It is not a content check in the other direction: `is_ascii()` is true
///   of the C0 controls, so a `NUL`-filled line still decodes into a
///   plausible-looking record. Tightening what counts as a valid field
///   VALUE is a separate question; the job here is the char-boundary panic
///   class.
/// * A rejected line is SKIPPED silently, matching this module's existing
///   posture for every other malformed line (see `parse_ti_lines`'s own
///   "a single malformed line must not abort the whole extraction").
///
/// Kept as a local copy rather than imported from `schedule-query`:
/// `schedule-reference` does depend on that crate, but its `parse` module's
/// helper is private and deliberately so (that module decodes the schedule
/// BODY record family, this one the `TI`/`A` reference-data family -- they
/// share a hazard, not a record format), and exporting a two-line
/// predicate across a crate boundary to avoid duplicating it would couple
/// the two parsers' evolution for no benefit.
fn is_fixed_width_decodable(line: &str, min_len: usize) -> bool {
    line.len() >= min_len && line.is_ascii()
}

/// Shortest `TI` line [`parse_ti_lines`] can decode: its own widest slice
/// ends at byte 56 (`53..56`, the CRS field). Named rather than inlined so
/// the guard call site and this module's tests refer to the same number,
/// matching `schedule_query::parse`'s own `MIN_*_LEN` convention.
const MIN_TI_LEN: usize = 56;

/// Shortest MSN `A` line [`parse_msn_a_lines`] can decode: its widest
/// slice ends at byte 52 (`49..52`, the CRS field).
const MIN_MSN_A_LEN: usize = 52;

/// Shortest MSN `A` line [`parse_msn_change_time_by_tiploc`] can decode:
/// its widest slice ends at byte 65 (`63..65`, the change-time field) --
/// deliberately a different, longer minimum than [`MIN_MSN_A_LEN`] for the
/// same record type, because this function reads a field further into the
/// line than `parse_msn_a_lines` does, so a line long enough for one is not
/// necessarily long enough for the other.
const MIN_MSN_CHANGE_TIME_LEN: usize = 65;

/// One parsed `TI` (TIPLOC Insert) record from a CIF `MCA` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiRecord {
    pub tiploc: String,
    pub station_name: String,
    pub stanox: Option<String>,
    pub crs: Option<String>,
}

/// Extracts every `TI` line from `text` (already filtered to `TI`-prefixed
/// lines by the caller's I/O layer -- see Task 4's `read_prefixed_lines`)
/// into a [`TiRecord`]. A line shorter than the fixed 80-byte real record
/// shape is skipped, not a hard error -- a single malformed line must not
/// abort the whole extraction (see the spec's Error handling section).
///
/// Byte layout (independently re-verified against real
/// `timetable_full.zip` bytes, see this module's tests):
/// `0..2` record type `"TI"`, `2..9` TIPLOC, `18..44` station name,
/// `44..49` STANOX (blank/`00000` = none), `53..56` CRS (blank = none).
pub fn parse_ti_lines(text: &str) -> Vec<TiRecord> {
    text.lines()
        .filter_map(|line| {
            // Guards all four fixed-offset slices below (`2..9`, `18..44`,
            // `44..49`, `53..56`) against BOTH panic conditions at once --
            // see [`is_fixed_width_decodable`].
            if !is_fixed_width_decodable(line, MIN_TI_LEN) {
                return None;
            }
            let tiploc = line[2..9].trim().to_string();
            let station_name = line[18..44].trim().to_string();
            let stanox_raw = line[44..49].trim();
            let stanox = if stanox_raw.is_empty() || stanox_raw == "00000" {
                None
            } else {
                Some(stanox_raw.to_string())
            };
            let crs_raw = line[53..56].trim();
            let crs = if crs_raw.is_empty() {
                None
            } else {
                Some(crs_raw.to_string())
            };
            Some(TiRecord {
                tiploc,
                station_name,
                stanox,
                crs,
            })
        })
        .collect()
}

/// TIPLOC -> CRS, from every real `A` record in `text` (already filtered
/// to `A`-prefixed lines by the caller). The one `FILE-SPEC=...` header
/// pseudo-record present in a real MSN file decodes to a non-alphanumeric
/// "TIPLOC" (`"PEC=05"`) at these byte offsets and is excluded by the same
/// alphanumeric check that guards against any other malformed line -- no
/// special-cased header skip needed.
///
/// Byte layout: `0..1` record type `"A"`, `5..35` station name, `35..36`
/// CATE digit, `36..43` TIPLOC, `49..52` CRS (always populated in a real
/// record).
pub fn parse_msn_a_lines(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        // Guards the `36..43` and `49..52` slices below against BOTH panic
        // conditions at once -- see [`is_fixed_width_decodable`].
        if !is_fixed_width_decodable(line, MIN_MSN_A_LEN) {
            continue;
        }
        let tiploc = line[36..43].trim();
        if tiploc.is_empty() || !tiploc.chars().all(|c| c.is_ascii_alphanumeric()) {
            continue; // catches the FILE-SPEC=05 header pseudo-record too
        }
        let crs = line[49..52].trim();
        if crs.is_empty() {
            continue;
        }
        map.insert(tiploc.to_string(), crs.to_string());
    }
    map
}

/// TIPLOC -> raw minimum-change-time minutes, from every real `A` record in
/// `text` (same already-filtered `A`-prefixed text `parse_msn_a_lines`
/// reads, `main.rs`'s `read_prefixed_lines(&delivery.msn_path, "A")`).
///
/// Byte layout: `63..65` (0-indexed, half-open) -- this is the sibling
/// `Distant-Signal-MCP` project's own documented `64-65` (1-indexed)
/// hypothesis, **NOT YET CONFIRMED** against a real delivery's MSN file in
/// this app's own byte layout: no live `timetable_full.zip` delivery was
/// available to this task's own implementation pass to run the verification
/// this plan's Task 2 Step 2 calls for. Treat this constant as an
/// honestly-flagged placeholder, not a verified fact -- the two codebases'
/// CRS-field byte math already disagrees by 6 bytes on the exact same real
/// fixture (this plan's Judgment Call 2), so a byte range copied from
/// another codebase's documentation is not sufficient evidence on its own,
/// and applying `63..65` to this crate's own already-tested `A_WATRLMN`
/// fixture (see this module's `change_time_tests`) produces `15`, outside
/// the sibling's own documented typical single-digit/modal-5 shape -- a
/// concrete reason to suspect this exact range, not just a formality.
///
/// When a real delivery becomes available, confirm or correct this range
/// with:
///
/// ```text
/// cargo run -p schedule-reference --example msn_change_time_probe -- \
///     /path/to/RJTTFnnnMSN.txt 63 65
/// ```
///
/// and compare the printed distribution against the sibling project's own
/// documented real shape: mostly single-digit values, a clear mode around
/// 5, and a small number (order of ten, not hundreds) of `98`/`99`
/// sentinels. If `[63,65)` doesn't produce that shape, try adjacent ranges
/// (`[62,64)`, `[64,66)`, etc.) until one does, then update both this
/// constant and this doc comment.
///
/// **Supporting evidence for `[63,65)`, beyond the sibling project's own
/// documentation (still not a substitute for live-delivery confirmation --
/// see above):** decomposing this crate's own real, already byte-verified
/// `A_WATRLMN` fixture (`change_time_tests`/`msn_tests`) field-by-field
/// shows `[63,65)` falls out deterministically once anchored on this
/// crate's own already-production-verified TIPLOC (`36..43`) and CRS
/// (`49..52`) offsets -- it is not an independent guess:
///
/// ```text
/// [35]      "3"        CATE interchange status
/// [36..43]  "WATRLMN"  TIPLOC          (this app's verified offset)
/// [43..46]  "WAT"      subsidiary 3-alpha
/// [46..49]  "   "      filler
/// [49..52]  "WAT"      CRS             (this app's verified, production offset)
/// [52..57]  "15312"    easting   (5)
/// [57]      " "        estimated-coords flag (1)
/// [58..63]  "61798"    northing  (5)
/// [63..65]  "15"       change time (2)   <- falls out deterministically
/// ```
///
/// This also likely explains Judgment Call 2's worry about this app's and
/// the sibling project's CRS byte-offsets disagreeing by 6 bytes: the
/// sibling's documented `44-46` (1-indexed) corresponds to a
/// 25-character-station-name MSN variant, while this app's real data (the
/// 30 characters of padded station name visible in `[5..35]` above) uses a
/// 30-character-station-name variant -- the two are not contradicting each
/// other, they are describing two different real layout variants of the
/// same record type. `[63,65)` is the offset that falls out of THIS app's
/// own 30-character variant, consistently with its own already-verified
/// TIPLOC/CRS offsets.
///
/// Returns the RAW parsed integer, with no default/sentinel interpretation
/// applied (deliberately -- see this plan's Judgment Call 3): `NULL`
/// downstream (this function simply omits the entry) means "no MSN record
/// matched this TIPLOC at all," a genuinely different fact from "this
/// TIPLOC's own recorded value happens to be a 98/99 sentinel" or "happens
/// to be the modal 5" -- both of which DO appear as real, present map
/// entries. A line whose change-time field is present but not a valid
/// non-negative integer is skipped for that one TIPLOC (same "skip
/// malformed, never abort the whole extraction" posture as
/// `parse_msn_a_lines`, not the sibling's own throw -- Judgment Call 5),
/// not a hard error.
pub fn parse_msn_change_time_by_tiploc(text: &str) -> HashMap<String, i32> {
    let mut map = HashMap::new();
    for line in text.lines() {
        // Guards the `36..43` and `63..65` slices below against BOTH panic
        // conditions at once -- see [`is_fixed_width_decodable`].
        if !is_fixed_width_decodable(line, MIN_MSN_CHANGE_TIME_LEN) {
            continue;
        }
        let tiploc = line[36..43].trim();
        if tiploc.is_empty() || !tiploc.chars().all(|c| c.is_ascii_alphanumeric()) {
            continue; // catches the FILE-SPEC=05 header pseudo-record, same as parse_msn_a_lines
        }
        let raw = line[63..65].trim();
        let Ok(minutes) = raw.parse::<i32>() else {
            continue;
        };
        if minutes < 0 {
            continue;
        }
        map.insert(tiploc.to_string(), minutes);
    }
    map
}

/// One resolved STANOX->CRS row, ready to be sent as a
/// `common::StanoxCrsRecord` (Task 3 supplies `source_sequence`, which this
/// pure module has no reason to know about).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRow {
    pub stanox: String,
    pub crs: String,
    pub tiploc: String,
    pub station_name: String,
    /// Raw minimum-change-time minutes from the MSN `A` record matching
    /// this row's own TIPLOC, or `None` if no MSN record matched it at
    /// all (a real, honest gap -- e.g. a junction-only TIPLOC with no
    /// passenger station record). See [`parse_msn_change_time_by_tiploc`]'s
    /// own doc comment for why this is never defaulted or sentinel-resolved
    /// here.
    pub change_time_minutes: Option<i32>,
}

/// Resolves the final STANOX->CRS table: completes a blank `TI` CRS from
/// `msn_crs_by_tiploc` (the WATRLMN case), groups by STANOX, and for any
/// STANOX with more than one distinct CRS applies the exact policy
/// `reference-data/stanox-crs.md:104-113` documents by hand for the
/// checked-in CSV -- prefer the sole non-`X`-prefixed candidate; otherwise
/// (2+ non-X, or 2+ X-prefixed, with no principled tiebreaker) exclude the
/// STANOX entirely. See this design's Decision 2.
pub fn resolve(
    ti: &[TiRecord],
    msn_crs_by_tiploc: &HashMap<String, String>,
    msn_change_time_by_tiploc: &HashMap<String, i32>,
) -> Vec<ParsedRow> {
    let mut by_stanox: HashMap<String, Vec<(&TiRecord, String)>> = HashMap::new();

    for record in ti {
        let Some(stanox) = &record.stanox else {
            continue;
        };
        let crs = record
            .crs
            .clone()
            .or_else(|| msn_crs_by_tiploc.get(&record.tiploc).cloned());
        let Some(crs) = crs else { continue };
        by_stanox
            .entry(stanox.clone())
            .or_default()
            .push((record, crs));
    }

    let mut rows = Vec::new();
    for (stanox, candidates) in by_stanox {
        let mut distinct: Vec<&str> = candidates.iter().map(|(_, crs)| crs.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();

        let winner = if distinct.len() == 1 {
            Some(distinct[0])
        } else {
            let non_x: Vec<&str> = distinct
                .iter()
                .copied()
                .filter(|c| !c.starts_with('X'))
                .collect();
            if non_x.len() == 1 {
                Some(non_x[0])
            } else {
                None
            }
        };

        if let Some(winner) = winner {
            let (record, crs) = candidates
                .iter()
                .find(|(_, crs)| crs == winner)
                .expect("winner came from distinct");
            rows.push(ParsedRow {
                stanox,
                crs: crs.clone(),
                tiploc: record.tiploc.clone(),
                station_name: record.station_name.clone(),
                change_time_minutes: msn_change_time_by_tiploc.get(&record.tiploc).copied(),
            });
        }
        // Otherwise: 2+ non-X candidates, or 2+ X-prefixed with none
        // non-X -- irresolvable, excluded entirely (see this design's
        // Error handling: "treat as irresolvable... never guess").
    }

    rows.sort_by(|a, b| a.stanox.cmp(&b.stanox));
    rows
}

/// One directly-resolved TIPLOC->CRS row -- the TIPLOC-primary output
/// `resolve_tiploc_crs` produces. Same fields `ParsedRow` carries, minus
/// `stanox`'s role as a grouping key (it is still carried through, just
/// never grouped/deduplicated on).
///
/// Wired into `main.rs`'s `poll_once` by this plan's Task 4 (the `POST
/// /private/tiploc-crs` publish and the `tiploc_to_crs` map-construction
/// sites) -- see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiplocCrsRow {
    pub tiploc: String,
    pub crs: String,
    pub station_name: String,
    pub stanox: String,
    pub change_time_minutes: Option<i32>,
}

/// Resolves a TIPLOC-PRIMARY CRS crosswalk: every `TI` record whose own CRS
/// is populated, or whose blank CRS is completed from `msn_crs_by_tiploc`
/// for that SAME TIPLOC (identical per-TIPLOC completion `resolve` already
/// does), becomes its own row -- with NO STANOX-based grouping,
/// tiebreaking, or exclusion step of any kind. This is deliberate, not a
/// simplification that drops a needed safeguard: see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md's
/// "Investigation finding" section for why a STANOX-inheritance policy is
/// NOT needed here, and why this cannot wrongly promote a same-STANOX
/// junction TIPLOC (case 2 of `crates/api/src/data/journey.rs`'s
/// `tiploc_key` doc comment) to a real station's identity: a junction
/// TIPLOC with neither its own `TI` CRS nor its own `MSN` record simply
/// produces no row here, exactly as it produces none in `resolve`.
///
/// Real example this exists to fix: Vauxhall's `VAUXHLM`/`VAUXHLW` (STANOX
/// `87214`, both CRS `VXH`) and Clapham Junction's `CLPHMJM`/`CLPHMJW`
/// (STANOX `87219`, both CRS `CLJ`) each get their own row here, unlike
/// `resolve`, which keeps only one TIPLOC per STANOX.
///
/// Unlike `resolve`, this function does NOT filter out X-prefixed
/// pseudo-CRS candidates in favor of a sole non-X-prefixed one, so a TIPLOC
/// whose only resolvable CRS is X-prefixed (e.g. real STANOX 87201's
/// `VICTRCR`/`XVR`, documented in reference-data/stanox-crs.md:100-113) now
/// gets its own `tiploc_crs` row where it previously had none via
/// `stanox_crs` -- not a new user-facing bug, since `journey.rs`'s
/// `an_x_prefixed_pseudo_crs_is_blanked_rather_than_displayed_as_a_real_station`
/// test/mechanism already exists specifically to blank such values back out
/// at render time, but worth documenting explicitly here.
pub fn resolve_tiploc_crs(
    ti: &[TiRecord],
    msn_crs_by_tiploc: &HashMap<String, String>,
    msn_change_time_by_tiploc: &HashMap<String, i32>,
) -> Vec<TiplocCrsRow> {
    // `by_tiploc: HashMap` rather than pushing straight into a `Vec` guards
    // against two `TI` lines for the same TIPLOC in a malformed delivery --
    // last-one-wins, matching this module's existing "skip/degrade malformed
    // input, never hard-error" posture elsewhere in this same file.
    let mut by_tiploc: HashMap<String, TiplocCrsRow> = HashMap::new();

    for record in ti {
        let Some(stanox) = &record.stanox else {
            continue;
        };
        let crs = record
            .crs
            .clone()
            .or_else(|| msn_crs_by_tiploc.get(&record.tiploc).cloned());
        let Some(crs) = crs else { continue };

        by_tiploc.insert(
            record.tiploc.clone(),
            TiplocCrsRow {
                tiploc: record.tiploc.clone(),
                crs,
                station_name: record.station_name.clone(),
                stanox: stanox.clone(),
                change_time_minutes: msn_change_time_by_tiploc.get(&record.tiploc).copied(),
            },
        );
    }

    let mut rows: Vec<TiplocCrsRow> = by_tiploc.into_values().collect();
    rows.sort_by(|a, b| a.tiploc.cmp(&b.tiploc));
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real `TI` lines, byte-verbatim, independently re-extracted this
    // session from timetable_full.zip's RJTTF942MCA.txt via
    // `unzip -p timetable_full.zip RJTTF942MCA.txt | grep ...` (no invented
    // test data -- see this plan's Global Constraints). EUSTON/VICTRIA/
    // VICTRCR match crates/trust-consumer/src/stanox_crs.rs's existing
    // REAL_EUSTON/REAL_VICTORIA/REAL_VICTORIA_CARRIAGE_ROAD fixtures
    // exactly.
    const TI_EUSTON: &str =
        "TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON           ";
    const TI_WATRLMN: &str =
        "TIWATRLMN16559801RLONDON WATERLOO           87212   0                           ";

    #[test]
    fn extracts_stanox_tiploc_crs_and_name_from_a_real_ti_line() {
        let records = parse_ti_lines(TI_EUSTON);
        assert_eq!(
            records,
            vec![TiRecord {
                tiploc: "EUSTON".to_string(),
                station_name: "LONDON EUSTON".to_string(),
                stanox: Some("72410".to_string()),
                crs: Some("EUS".to_string()),
            }]
        );
    }

    #[test]
    fn a_blank_crs_field_parses_as_none_not_an_empty_string() {
        let records = parse_ti_lines(TI_WATRLMN);
        assert_eq!(records[0].tiploc, "WATRLMN");
        assert_eq!(records[0].stanox, Some("87212".to_string()));
        assert_eq!(records[0].crs, None);
    }

    #[test]
    fn a_short_malformed_line_is_skipped_not_an_error() {
        assert_eq!(parse_ti_lines("TIshort"), Vec::new());
    }
}

#[cfg(test)]
mod msn_tests {
    use super::*;

    // Real `A` lines, byte-verbatim, independently re-extracted this
    // session from timetable_full.zip's RJTTF942MSN.txt. `pub(super)` so
    // the sibling `change_time_tests` module below can reuse the exact
    // same byte-verified fixture lines rather than re-declaring them.
    pub(super) const A_WATRLMN: &str =
        "A    LONDON WATERLOO               3WATRLMNWAT   WAT15312 6179815";
    pub(super) const A_HEADER: &str =
        "A                             FILE-SPEC=05 1.00 28/08/26 18.08.01   944           ";

    #[test]
    fn extracts_tiploc_to_crs_from_a_real_a_record() {
        let map = parse_msn_a_lines(A_WATRLMN);
        assert_eq!(map.get("WATRLMN"), Some(&"WAT".to_string()));
    }

    #[test]
    fn the_file_spec_header_pseudo_record_is_excluded() {
        let map = parse_msn_a_lines(A_HEADER);
        assert!(
            map.is_empty(),
            "the header record must not be mistaken for a real TIPLOC"
        );
    }
}

#[cfg(test)]
mod change_time_tests {
    use super::msn_tests::{A_HEADER, A_WATRLMN};
    use super::*;

    // Clearly-labeled SYNTHETIC-but-byte-layout-correct line: a real 98/99
    // sentinel station's exact byte-for-byte `A` line was not available to
    // this task's implementation pass (no live delivery -- see
    // parse_msn_change_time_by_tiploc's own doc comment), so this is built
    // at the same real-byte-verified TIPLOC (`36..43`) and change-time
    // (`63..65`) offsets, per this crate's own "quote real bytes when
    // available, clearly mark anything else synthetic" convention
    // (`crates/schedule-query/src/records.rs:130-136`'s sibling
    // precedent). Every other byte is blank filler -- only the two fields
    // this parser reads are meaningful.
    const A_SENTINEL_SYNTHETIC: &str =
        "A                                   SENTNL                     98";

    // Clearly-labeled SYNTHETIC-but-byte-layout-correct line, same
    // convention as `A_SENTINEL_SYNTHETIC` directly above: a real,
    // otherwise-valid-shaped `A` record (>=65 bytes, valid alphanumeric
    // TIPLOC at `36..43`) whose change-time field (`63..65`) is PRESENT but
    // BLANK (two spaces) -- distinct from `a_tiploc_with_no_msn_record_at_all_is_absent_not_zero`
    // below, which tests no `A` record matching the TIPLOC at all. This
    // tests the field being present-but-empty within a record that DOES
    // match, the specific risk this plan's own Review Focus section named.
    const A_BLANK_CHANGE_TIME_SYNTHETIC: &str =
        "A                                   BLANKTP                      ";

    #[test]
    fn extracts_the_change_time_for_a_real_a_record() {
        let map = parse_msn_change_time_by_tiploc(A_WATRLMN);
        // `[63,65)` is NOT YET CONFIRMED against a real delivery (see
        // parse_msn_change_time_by_tiploc's own doc comment) -- this
        // asserts on whatever that unverified range actually produces for
        // this exact byte-verbatim real fixture line, computed directly
        // (bytes 63..65 of A_WATRLMN are "15"), not on a value independently
        // confirmed as correct. Update this assertion if a future
        // verification pass (this plan's Task 2 Step 2) confirms a
        // different byte range.
        assert_eq!(map.get("WATRLMN"), Some(&15));
    }

    #[test]
    fn the_file_spec_header_pseudo_record_contributes_no_change_time() {
        let map = parse_msn_change_time_by_tiploc(A_HEADER);
        assert!(map.is_empty());
    }

    #[test]
    fn a_tiploc_with_no_msn_record_at_all_is_absent_not_zero() {
        let map = parse_msn_change_time_by_tiploc("");
        assert_eq!(map.get("ANYTPL"), None);
    }

    #[test]
    fn a_present_but_blank_change_time_field_is_absent_not_zero_or_a_panic() {
        // A record that DOES match the TIPLOC, but whose change-time bytes
        // are blank (not a valid integer), must behave the same as "no MSN
        // record matched this TIPLOC at all" (see
        // a_tiploc_with_no_msn_record_at_all_is_absent_not_zero above) --
        // the TIPLOC is simply absent from the returned map, proven here via
        // a different, more specific input shape: a present-but-blank
        // field, not an absent-entirely record.
        let map = parse_msn_change_time_by_tiploc(A_BLANK_CHANGE_TIME_SYNTHETIC);
        assert_eq!(map.get("BLANKTP"), None);
    }

    #[test]
    fn a_98_99_sentinel_is_stored_as_a_real_present_value_not_confused_with_absence() {
        // A sentinel (98/99) is a genuinely present, real recorded value --
        // distinct from `None` ("no MSN record matched this TIPLOC at
        // all", see the a_tiploc_with_no_msn_record_at_all_is_absent_not_zero
        // case above). This function must not special-case or filter it
        // out (Judgment Call 3: no default/sentinel interpretation here).
        let map = parse_msn_change_time_by_tiploc(A_SENTINEL_SYNTHETIC);
        assert_eq!(map.get("SENTNL"), Some(&98));
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::*;

    fn ti(tiploc: &str, name: &str, stanox: &str, crs: &str) -> TiRecord {
        TiRecord {
            tiploc: tiploc.to_string(),
            station_name: name.to_string(),
            stanox: if stanox.is_empty() {
                None
            } else {
                Some(stanox.to_string())
            },
            crs: if crs.is_empty() {
                None
            } else {
                Some(crs.to_string())
            },
        }
    }

    #[test]
    fn an_unambiguous_stanox_resolves_directly() {
        let rows = resolve(
            &[ti("EUSTON", "LONDON EUSTON", "72410", "EUS")],
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(
            rows,
            vec![ParsedRow {
                stanox: "72410".to_string(),
                crs: "EUS".to_string(),
                tiploc: "EUSTON".to_string(),
                station_name: "LONDON EUSTON".to_string(),
                change_time_minutes: None,
            }]
        );
    }

    #[test]
    fn a_matched_change_time_wires_through_as_some_not_just_the_empty_map_path() {
        // Every other resolve_tests case passes &HashMap::new() for the
        // change-time map, which only exercises the "absent" branch of
        // msn_change_time_by_tiploc.get(&record.tiploc).copied() in
        // resolve(). This case passes a real entry to confirm the
        // Some(...) branch actually wires the value onto the resolved row.
        let ti_records = vec![ti("EUSTON", "LONDON EUSTON", "72410", "EUS")];
        let change_time = HashMap::from([("EUSTON".to_string(), 5)]);
        let rows = resolve(&ti_records, &HashMap::new(), &change_time);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].change_time_minutes, Some(5));
    }

    #[test]
    fn a_blank_ti_crs_is_completed_from_the_msn_a_record_before_grouping() {
        let ti_records = vec![ti("WATRLMN", "LONDON WATERLOO", "87212", "")];
        let msn = HashMap::from([("WATRLMN".to_string(), "WAT".to_string())]);
        let rows = resolve(&ti_records, &msn, &HashMap::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].crs, "WAT");
    }

    #[test]
    fn ambiguous_stanox_with_one_non_x_candidate_resolves_to_it() {
        // The real 87201 case: VICTRIA/VIC (real passenger CRS) vs
        // VICTRCR/XVR (X-prefixed pseudo-code).
        let ti_records = vec![
            ti("VICTRIA", "LONDON VICTORIA", "87201", "VIC"),
            ti("VICTRCR", "VICTORIA CARRIAGE ROAD", "87201", "XVR"),
        ];
        let rows = resolve(&ti_records, &HashMap::new(), &HashMap::new());
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].stanox, "87201");
        assert_eq!(rows[0].crs, "VIC", "the non-X-prefixed candidate wins");
    }

    #[test]
    fn ambiguous_stanox_with_two_non_x_candidates_is_excluded_entirely() {
        // The real, genuinely irresolvable 89428 case: ASI and AFK are both
        // real, non-X-prefixed CRS codes -- no principled tiebreaker.
        let ti_records = vec![
            ti("ASHFKI", "ASHFORD INT (PLATS 3-4)", "89428", "ASI"),
            ti("ASHFKY", "ASHFORD INTERNATIONAL", "89428", "AFK"),
        ];
        let rows = resolve(&ti_records, &HashMap::new(), &HashMap::new());
        assert!(rows.is_empty(), "89428 must be excluded, not guessed at");
    }

    #[test]
    fn all_14_real_ambiguous_stanox_values_resolve_exactly_as_the_checked_in_csv_does() {
        // The full real 2026-08-28 ambiguity set (Current relevant state,
        // this plan's spec) -- 9 resolved via the non-X-preference rule, 5
        // excluded. Regression guard: if a future CIF extract's ambiguity
        // set differs, this test's own failure is the signal to update it
        // (Open question 4 in the spec).
        let ti_records = vec![
            ti("A1", "n", "30120", "PRE"),
            ti("A2", "n", "30120", "XPU"),
            ti("B1", "n", "31510", "MCV"),
            ti("B2", "n", "31510", "XVS"),
            ti("C1", "n", "40320", "CTR"),
            ti("C2", "n", "40320", "XCZ"),
            ti("D1", "n", "52215", "SDI"),
            ti("D2", "n", "52215", "SFA"),
            ti("E1", "n", "86441", "BOG"),
            ti("E2", "n", "86441", "XBN"),
            ti("F1", "n", "86935", "PFT"),
            ti("F2", "n", "86935", "POO"),
            ti("G1", "n", "86981", "WEY"),
            ti("G2", "n", "86981", "XWJ"),
            ti("H1", "n", "87201", "VIC"),
            ti("H2", "n", "87201", "XVR"),
            ti("I1", "n", "87219", "CLJ"),
            ti("I2", "n", "87219", "XCP"),
            ti("J1", "n", "87261", "WIM"),
            ti("J2", "n", "87261", "XWD"),
            ti("K1", "n", "87981", "XBP"),
            ti("K2", "n", "87981", "XMP"),
            ti("L1", "n", "88486", "SAY"),
            ti("L2", "n", "88486", "XSQ"),
            ti("M1", "n", "89428", "AFK"),
            ti("M2", "n", "89428", "ASI"),
            ti("N1", "n", "89530", "EBD"),
            ti("N2", "n", "89530", "EBF"),
        ];
        let rows = resolve(&ti_records, &HashMap::new(), &HashMap::new());
        let resolved: HashMap<&str, &str> = rows
            .iter()
            .map(|r| (r.stanox.as_str(), r.crs.as_str()))
            .collect();

        for (stanox, expected_crs) in [
            ("30120", "PRE"),
            ("31510", "MCV"),
            ("40320", "CTR"),
            ("86441", "BOG"),
            ("86981", "WEY"),
            ("87201", "VIC"),
            ("87219", "CLJ"),
            ("87261", "WIM"),
            ("88486", "SAY"),
        ] {
            assert_eq!(
                resolved.get(stanox),
                Some(&expected_crs),
                "stanox {stanox} should resolve to {expected_crs}"
            );
        }
        for stanox in ["52215", "86935", "87981", "89428", "89530"] {
            assert!(
                !resolved.contains_key(stanox),
                "stanox {stanox} should be excluded, not resolved"
            );
        }
        assert_eq!(rows.len(), 9);
    }
}

#[cfg(test)]
mod resolve_tiploc_crs_tests {
    use super::*;

    // Duplicated from resolve_tests::ti (rather than making that helper
    // pub(super) and importing it) to keep this task's diff additive-only:
    // resolve_tests and every other existing test in this file stays
    // byte-for-byte unmodified. Same 6-line shape, same convention (blank
    // stanox/crs strings map to None).
    fn ti(tiploc: &str, name: &str, stanox: &str, crs: &str) -> TiRecord {
        TiRecord {
            tiploc: tiploc.to_string(),
            station_name: name.to_string(),
            stanox: if stanox.is_empty() {
                None
            } else {
                Some(stanox.to_string())
            },
            crs: if crs.is_empty() {
                None
            } else {
                Some(crs.to_string())
            },
        }
    }

    #[test]
    fn vauxhall_both_real_tiplocs_resolve_to_the_same_real_crs_when_both_ti_records_carry_it_directly()
     {
        // Core regression test for this plan's whole point. Vauxhall's two
        // real TIPLOCs (STANOX 87214, both CRS VXH -- see
        // reference-data/stanox-crs.csv line "87214,VXH") each carry their
        // own CRS directly on their own TI record.
        let ti_records = vec![
            ti("VAUXHLM", "VAUXHALL", "87214", "VXH"),
            ti("VAUXHLW", "VAUXHALL", "87214", "VXH"),
        ];

        let rows = resolve_tiploc_crs(&ti_records, &HashMap::new(), &HashMap::new());
        assert_eq!(rows.len(), 2, "one row per TIPLOC, not per STANOX");
        assert_eq!(rows[0].crs, "VXH");
        assert_eq!(rows[1].crs, "VXH");

        // Explicit contrast: `resolve` (existing, unchanged) on this SAME
        // input keeps only ONE TIPLOC per STANOX -- this is the exact
        // information loss this plan's tiploc_crs crosswalk exists to fix.
        // This assertion fails loudly if a future change to `resolve`
        // itself ever accidentally "fixes" this the wrong way (i.e. inside
        // `resolve` rather than via this new, separate crosswalk).
        assert_eq!(
            resolve(&ti_records, &HashMap::new(), &HashMap::new()).len(),
            1
        );
    }

    #[test]
    fn vauxhall_windsor_lines_still_resolves_when_its_own_ti_crs_is_blank_and_only_msn_completes_it()
     {
        // Same two real Vauxhall TIPLOCs, but VAUXHLW's own TI CRS is blank
        // here, completed only via its own MSN A record instead -- proves
        // the fix works whichever of the two real completion paths turns
        // out to be true for this TIPLOC in a live delivery (this plan's
        // Context section explains why that was not independently
        // re-verified byte-for-byte in this pass).
        let ti_records = vec![
            ti("VAUXHLM", "VAUXHALL", "87214", "VXH"),
            ti("VAUXHLW", "VAUXHALL", "87214", ""),
        ];
        let msn_crs_by_tiploc = HashMap::from([("VAUXHLW".to_string(), "VXH".to_string())]);

        let rows = resolve_tiploc_crs(&ti_records, &msn_crs_by_tiploc, &HashMap::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].crs, "VXH");
        assert_eq!(rows[1].crs, "VXH");
    }

    #[test]
    fn clapham_junction_both_real_tiplocs_resolve_to_the_same_real_crs() {
        // Same shape as the Vauxhall case above, for Clapham Junction's two
        // real TIPLOCs (STANOX 87219, both CRS CLJ -- see
        // reference-data/stanox-crs.csv line "87219,CLJ").
        let ti_records = vec![
            ti("CLPHMJM", "CLAPHAM JUNCTION", "87219", "CLJ"),
            ti("CLPHMJW", "CLAPHAM JUNCTION", "87219", "CLJ"),
        ];

        let rows = resolve_tiploc_crs(&ti_records, &HashMap::new(), &HashMap::new());
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].crs, "CLJ");
        assert_eq!(rows[1].crs, "CLJ");
    }

    #[test]
    fn a_junction_tiploc_sharing_a_stations_stanox_with_no_own_crs_anywhere_still_does_not_resolve()
    {
        // Proves no accidental STANOX-inheritance -- the exact worry
        // `crates/api/src/data/journey.rs`'s `tiploc_key` doc comment names
        // by example (a Waterloo-area junction TIPLOC wrongly inheriting
        // Waterloo's own CRS). WATRLWC shares STANOX 87212 with real
        // station WATRLMN, but has neither its own TI CRS nor a matching
        // MSN record, so it must simply not resolve -- not default to
        // WATRLMN's WAT.
        let ti_records = vec![
            ti("WATRLMN", "LONDON WATERLOO", "87212", "WAT"),
            ti("WATRLWC", "WATERLOO WINDSOR JN", "87212", ""),
        ];

        let rows = resolve_tiploc_crs(&ti_records, &HashMap::new(), &HashMap::new());
        assert_eq!(
            rows.len(),
            1,
            "WATRLWC must be absent, not defaulted to WAT"
        );
        assert_eq!(rows[0].tiploc, "WATRLMN");
        assert_eq!(rows[0].crs, "WAT");
    }

    #[test]
    fn ambiguous_stanox_with_two_genuine_non_x_candidates_now_resolves_both_instead_of_neither() {
        // The real, currently-excluded `resolve` case (STANOX 89428: ASHFKI
        // /ASI vs ASHFKY/AFK, both real, distinct, bookable stations --
        // copied from
        // resolve_tests::ambiguous_stanox_with_two_non_x_candidates_is_excluded_entirely).
        // Explicit, positive contrast: `resolve` on this SAME input still
        // returns 0 rows (per that existing, unchanged test) because
        // neither candidate has a principled STANOX-level tiebreaker: this
        // is a real side benefit of the TIPLOC-primary design, not a
        // required behavior change to `resolve` itself, since each TIPLOC's
        // own CRS is directly known and neither needs to borrow the
        // other's identity.
        let ti_records = vec![
            ti("ASHFKI", "ASHFORD INT (PLATS 3-4)", "89428", "ASI"),
            ti("ASHFKY", "ASHFORD INTERNATIONAL", "89428", "AFK"),
        ];

        let rows = resolve_tiploc_crs(&ti_records, &HashMap::new(), &HashMap::new());
        assert_eq!(rows.len(), 2);
        // Rows are sorted by tiploc: "ASHFKI" < "ASHFKY".
        assert_eq!(rows[0].tiploc, "ASHFKI");
        assert_eq!(rows[0].crs, "ASI");
        assert_eq!(rows[1].tiploc, "ASHFKY");
        assert_eq!(rows[1].crs, "AFK");

        assert_eq!(
            resolve(&ti_records, &HashMap::new(), &HashMap::new()).len(),
            0,
            "resolve itself is unchanged: 89428 stays excluded there"
        );
    }

    #[test]
    fn a_tiploc_with_no_stanox_at_all_still_does_not_resolve() {
        // Matches `resolve`'s existing guard, carried over unchanged: a
        // blank STANOX maps to None per `ti()`'s own mapping, so the record
        // is skipped before CRS resolution is even attempted.
        let ti_records = vec![ti("FOO", "n", "", "BAR")];
        let rows = resolve_tiploc_crs(&ti_records, &HashMap::new(), &HashMap::new());
        assert!(rows.is_empty());
    }
}

/// Regression tests for the 2026-09-25 non-ASCII panic-crash-loop fix --
/// see [`is_fixed_width_decodable`]'s own doc comment for the production
/// failure mode (one non-ASCII byte in a 700MB+ delivery file panics this
/// service, which then crash-loops on the same line forever, freezing all
/// seven of its published products).
///
/// Every case below takes a REAL, byte-verbatim fixture line from the
/// modules above and moves a single multi-byte character into it so that one
/// of this module's own hard-coded slice offsets lands in the MIDDLE of that
/// character, with the line's byte LENGTH deliberately unchanged -- so the
/// pre-existing `line.len() >= N` check still passes and the char-boundary
/// panic is the only thing left to catch. Before the fix, each of these
/// panicked with `byte index N is not a char boundary; it is inside 'é'
/// (bytes M..N+1) of ...`; after it, the malformed line is silently
/// skipped, exactly like every other malformed line in this module.
#[cfg(test)]
mod non_ascii_boundary_tests {
    use super::msn_tests::A_WATRLMN;
    use super::*;

    /// Rewrites `line` so that byte index `boundary` falls INSIDE a
    /// multi-byte character, without changing the line's total byte length.
    ///
    /// `é` is two bytes (`0xC3 0xA9`), so overwriting the two ASCII bytes at
    /// `boundary - 1` and `boundary` with it puts its first byte at
    /// `boundary - 1` and its continuation byte at `boundary` -- making
    /// `boundary` itself not a char boundary while every other offset in the
    /// line, and the line's length, stay exactly as they were.
    fn straddling_char_at(line: &str, boundary: usize) -> String {
        let mut out = String::with_capacity(line.len());
        out.push_str(&line[..boundary - 1]);
        out.push('é');
        out.push_str(&line[boundary + 1..]);
        assert_eq!(
            out.len(),
            line.len(),
            "the fixture must keep its original byte length so the length check still passes"
        );
        assert!(
            !out.is_char_boundary(boundary),
            "the fixture must actually put a char boundary violation at the offset under test"
        );
        out
    }

    const TI_EUSTON: &str =
        "TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON           ";

    #[test]
    fn parse_ti_lines_skips_a_line_whose_tiploc_offset_is_mid_character() {
        // `line[2..9]`, the TIPLOC field's own end offset.
        let line = straddling_char_at(TI_EUSTON, 9);
        assert_eq!(parse_ti_lines(&line), Vec::new());
    }

    #[test]
    fn parse_ti_lines_skips_a_line_whose_station_name_offset_is_mid_character() {
        // `line[18..44]`/`line[44..49]`'s shared boundary -- the realistic
        // production shape: a multi-byte character inside a station NAME,
        // straddling the start of the STANOX field.
        let line = straddling_char_at(TI_EUSTON, 44);
        assert_eq!(parse_ti_lines(&line), Vec::new());
    }

    #[test]
    fn parse_ti_lines_skips_a_line_whose_crs_offset_is_mid_character() {
        // `line[53..56]`, the last and furthest-in slice this function reads.
        let line = straddling_char_at(TI_EUSTON, 56);
        assert_eq!(parse_ti_lines(&line), Vec::new());
    }

    #[test]
    fn parse_ti_lines_keeps_decoding_the_rest_of_the_file_after_a_bad_line() {
        // The whole point of skipping rather than panicking: a real
        // delivery's other ~12,084 `TI` records must still publish.
        let bad = straddling_char_at(TI_EUSTON, 44);
        let text = format!("{bad}\n{TI_EUSTON}\n");
        let records = parse_ti_lines(&text);
        assert_eq!(records.len(), 1, "the one good line must still decode");
        assert_eq!(records[0].tiploc, "EUSTON");
    }

    #[test]
    fn parse_msn_a_lines_skips_a_line_whose_tiploc_offset_is_mid_character() {
        // `line[36..43]`, the TIPLOC field's own end offset.
        let line = straddling_char_at(A_WATRLMN, 43);
        assert!(parse_msn_a_lines(&line).is_empty());
    }

    #[test]
    fn parse_msn_a_lines_skips_a_line_whose_crs_offset_is_mid_character() {
        // `line[49..52]`, the CRS field's own end offset.
        let line = straddling_char_at(A_WATRLMN, 52);
        assert!(parse_msn_a_lines(&line).is_empty());
    }

    #[test]
    fn parse_msn_change_time_skips_a_line_whose_change_time_offset_is_mid_character() {
        // `line[63..65]`'s own start offset -- the field this function reads
        // furthest into the line, and the one `MIN_MSN_A_LEN`'s shorter
        // minimum would not have covered.
        let line = straddling_char_at(A_WATRLMN, 63);
        assert!(parse_msn_change_time_by_tiploc(&line).is_empty());
    }

    #[test]
    fn parse_msn_change_time_skips_a_line_whose_tiploc_offset_is_mid_character() {
        let line = straddling_char_at(A_WATRLMN, 43);
        assert!(parse_msn_change_time_by_tiploc(&line).is_empty());
    }
}
