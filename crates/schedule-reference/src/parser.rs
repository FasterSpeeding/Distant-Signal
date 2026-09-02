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
            if line.len() < 56 {
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
        if line.len() < 52 {
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

/// One resolved STANOX->CRS row, ready to be sent as a
/// `common::StanoxCrsRecord` (Task 3 supplies `source_sequence`, which this
/// pure module has no reason to know about).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRow {
    pub stanox: String,
    pub crs: String,
    pub tiploc: String,
    pub station_name: String,
}

/// Resolves the final STANOX->CRS table: completes a blank `TI` CRS from
/// `msn_crs_by_tiploc` (the WATRLMN case), groups by STANOX, and for any
/// STANOX with more than one distinct CRS applies the exact policy
/// `reference-data/stanox-crs.md:104-113` documents by hand for the
/// checked-in CSV -- prefer the sole non-`X`-prefixed candidate; otherwise
/// (2+ non-X, or 2+ X-prefixed, with no principled tiebreaker) exclude the
/// STANOX entirely. See this design's Decision 2.
pub fn resolve(ti: &[TiRecord], msn_crs_by_tiploc: &HashMap<String, String>) -> Vec<ParsedRow> {
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
            });
        }
        // Otherwise: 2+ non-X candidates, or 2+ X-prefixed with none
        // non-X -- irresolvable, excluded entirely (see this design's
        // Error handling: "treat as irresolvable... never guess").
    }

    rows.sort_by(|a, b| a.stanox.cmp(&b.stanox));
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
    // session from timetable_full.zip's RJTTF942MSN.txt.
    const A_WATRLMN: &str = "A    LONDON WATERLOO               3WATRLMNWAT   WAT15312 6179815";
    const A_HEADER: &str =
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
        );
        assert_eq!(
            rows,
            vec![ParsedRow {
                stanox: "72410".to_string(),
                crs: "EUS".to_string(),
                tiploc: "EUSTON".to_string(),
                station_name: "LONDON EUSTON".to_string(),
            }]
        );
    }

    #[test]
    fn a_blank_ti_crs_is_completed_from_the_msn_a_record_before_grouping() {
        let ti_records = vec![ti("WATRLMN", "LONDON WATERLOO", "87212", "")];
        let msn = HashMap::from([("WATRLMN".to_string(), "WAT".to_string())]);
        let rows = resolve(&ti_records, &msn);
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
        let rows = resolve(&ti_records, &HashMap::new());
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
        let rows = resolve(&ti_records, &HashMap::new());
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
        let rows = resolve(&ti_records, &HashMap::new());
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
