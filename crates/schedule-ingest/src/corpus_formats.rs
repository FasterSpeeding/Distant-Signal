//! The migration signal between the two CORPUS formats: the JSON extract
//! (`CORPUSExtract.json.gz`) and the NLC XML snapshot (`NLC*.xml.gz`).
//!
//! The XML format carries only the TIPLOC-bearing locations, so its row
//! count is about a fifth of the JSON extract's and is not itself a signal.
//! What matters is whether switching would change what the app derives:
//!
//! - the TIPLOCs each side has, and whether a TIPLOC both have agrees on
//!   its NLC, STANOX, CRS and UIC (descriptions are counted separately: the
//!   two formats word many of them differently);
//! - the runtime crosswalk each side would build
//!   (`ds_store::corpus::crosswalk::derive`, the same conservative inference
//!   the ingest-writer's CORPUS loop stores), before its `stations` filter,
//!   which needs the database: TIPLOC->CRS and STANOX->CRS pairs only one
//!   side gives, and keys both give with different CRSs.
//!
//! [`compare`] is pure. The caller logs [`FormatComparison`] and sets the
//! `schedule_feed_corpus_format_diff{measure}` gauges from it.

use std::collections::{BTreeMap, BTreeSet};

use ds_store::corpus::crosswalk::{corpus_rows, derive};

use crate::corpus::CorpusLocation;

/// The gauge [`FormatComparison::measures`] sets, one series per measure.
pub(crate) const DIFF_METRIC: &str = "schedule_feed_corpus_format_diff";
/// How many example keys a log line names per bucket.
const SAMPLES: usize = 10;

/// One side's keys, for counting and sampling differences.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct KeyDiff {
    pub count: usize,
    pub sample: Vec<String>,
}

impl KeyDiff {
    fn of<'a>(keys: impl Iterator<Item = &'a String>) -> Self {
        let mut diff = Self::default();
        for key in keys {
            diff.count += 1;
            if diff.sample.len() < SAMPLES {
                diff.sample.push(key.clone());
            }
        }
        diff
    }
}

/// How the JSON extract and the XML snapshot differ.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct FormatComparison {
    pub json_rows: usize,
    pub xml_rows: usize,
    /// JSON rows with a TIPLOC (the part the XML format carries).
    pub json_tiploc_rows: usize,
    pub tiplocs_only_json: KeyDiff,
    pub tiplocs_only_xml: KeyDiff,
    pub tiplocs_both: usize,
    /// TIPLOCs both carry whose NLC, STANOX, CRS or UIC differ.
    pub code_mismatches: KeyDiff,
    /// TIPLOCs both carry whose description differs.
    pub description_mismatches: usize,
    pub crosswalk_tiplocs_only_json: KeyDiff,
    pub crosswalk_tiplocs_only_xml: KeyDiff,
    pub crosswalk_tiplocs_conflicting: KeyDiff,
    pub crosswalk_stanoxes_only_json: KeyDiff,
    pub crosswalk_stanoxes_only_xml: KeyDiff,
    pub crosswalk_stanoxes_conflicting: KeyDiff,
}

impl FormatComparison {
    /// Every count, as `(measure, value)` for the gauge's `measure` label.
    pub(crate) fn measures(&self) -> [(&'static str, usize); 14] {
        [
            ("json_rows", self.json_rows),
            ("xml_rows", self.xml_rows),
            ("json_tiploc_rows", self.json_tiploc_rows),
            ("tiplocs_only_json", self.tiplocs_only_json.count),
            ("tiplocs_only_xml", self.tiplocs_only_xml.count),
            ("tiplocs_both", self.tiplocs_both),
            ("code_mismatches", self.code_mismatches.count),
            ("description_mismatches", self.description_mismatches),
            (
                "crosswalk_tiplocs_only_json",
                self.crosswalk_tiplocs_only_json.count,
            ),
            (
                "crosswalk_tiplocs_only_xml",
                self.crosswalk_tiplocs_only_xml.count,
            ),
            (
                "crosswalk_tiplocs_conflicting",
                self.crosswalk_tiplocs_conflicting.count,
            ),
            (
                "crosswalk_stanoxes_only_json",
                self.crosswalk_stanoxes_only_json.count,
            ),
            (
                "crosswalk_stanoxes_only_xml",
                self.crosswalk_stanoxes_only_xml.count,
            ),
            (
                "crosswalk_stanoxes_conflicting",
                self.crosswalk_stanoxes_conflicting.count,
            ),
        ]
    }
}

/// The codes compared per TIPLOC.
fn codes(l: &CorpusLocation) -> (&str, Option<&str>, Option<&str>, Option<&str>) {
    (
        l.nlc.as_str(),
        l.stanox.as_deref(),
        l.crs.as_deref(),
        l.uic.as_deref(),
    )
}

/// Keys in `a` and not `b`, keys in `b` and not `a`, and keys in both
/// with different values.
fn diff_maps(
    a: &BTreeMap<String, String>,
    b: &BTreeMap<String, String>,
) -> (KeyDiff, KeyDiff, KeyDiff) {
    (
        KeyDiff::of(a.keys().filter(|k| !b.contains_key(*k))),
        KeyDiff::of(b.keys().filter(|k| !a.contains_key(*k))),
        KeyDiff::of(
            a.iter()
                .filter(|(k, v)| b.get(*k).is_some_and(|w| w != *v))
                .map(|(k, _)| k),
        ),
    )
}

/// The crosswalk `locations` would give, as TIPLOC->CRS and STANOX->CRS.
fn crosswalk_maps(
    locations: &[CorpusLocation],
) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
    let (_, crosswalk) = derive(&corpus_rows(locations));
    (
        crosswalk
            .tiplocs
            .into_iter()
            .map(|t| (t.tiploc, t.crs))
            .collect(),
        crosswalk
            .stanoxes
            .into_iter()
            .map(|s| (s.stanox, s.crs))
            .collect(),
    )
}

/// Compares one JSON extract with one XML snapshot.
pub(crate) fn compare(json: &[CorpusLocation], xml: &[CorpusLocation]) -> FormatComparison {
    // A TIPLOC CORPUS repeats keeps its first row on each side: the
    // comparison is about coverage, not about the few duplicate rows.
    let by_tiploc = |rows: &[CorpusLocation]| {
        let mut map: BTreeMap<String, CorpusLocation> = BTreeMap::new();
        for row in rows {
            if let Some(tiploc) = &row.tiploc {
                map.entry(tiploc.clone()).or_insert_with(|| row.clone());
            }
        }
        map
    };
    let json_by = by_tiploc(json);
    let xml_by = by_tiploc(xml);
    let both: BTreeSet<&String> = json_by.keys().filter(|k| xml_by.contains_key(*k)).collect();
    let code_mismatches = KeyDiff::of(
        both.iter()
            .copied()
            .filter(|k| codes(&json_by[*k]) != codes(&xml_by[*k])),
    );
    let description_mismatches = both
        .iter()
        .filter(|k| json_by[**k].nlc_desc != xml_by[**k].nlc_desc)
        .count();

    let (json_tiplocs, json_stanoxes) = crosswalk_maps(json);
    let (xml_tiplocs, xml_stanoxes) = crosswalk_maps(xml);
    let (t_json, t_xml, t_conflict) = diff_maps(&json_tiplocs, &xml_tiplocs);
    let (s_json, s_xml, s_conflict) = diff_maps(&json_stanoxes, &xml_stanoxes);

    FormatComparison {
        json_rows: json.len(),
        xml_rows: xml.len(),
        json_tiploc_rows: json.iter().filter(|l| l.tiploc.is_some()).count(),
        tiplocs_only_json: KeyDiff::of(json_by.keys().filter(|k| !xml_by.contains_key(*k))),
        tiplocs_only_xml: KeyDiff::of(xml_by.keys().filter(|k| !json_by.contains_key(*k))),
        tiplocs_both: both.len(),
        code_mismatches,
        description_mismatches,
        crosswalk_tiplocs_only_json: t_json,
        crosswalk_tiplocs_only_xml: t_xml,
        crosswalk_tiplocs_conflicting: t_conflict,
        crosswalk_stanoxes_only_json: s_json,
        crosswalk_stanoxes_only_xml: s_xml,
        crosswalk_stanoxes_conflicting: s_conflict,
    }
}

/// Logs `comparison` (with example keys) and sets its gauges.
#[expect(
    clippy::cast_precision_loss,
    reason = "metric gauges take f64, and these counts stay far below 2^52"
)]
pub(crate) fn report(comparison: &FormatComparison, json_file: &str, xml_file: &str) {
    for (measure, value) in comparison.measures() {
        metrics::gauge!(common::metrics::metric_name(DIFF_METRIC), "measure" => measure)
            .set(value as f64);
    }
    let c = comparison;
    tracing::info!(
        json_file = %json_file,
        xml_file = %xml_file,
        json_rows = c.json_rows,
        json_tiploc_rows = c.json_tiploc_rows,
        xml_rows = c.xml_rows,
        tiplocs_both = c.tiplocs_both,
        tiplocs_only_json = c.tiplocs_only_json.count,
        tiplocs_only_json_sample = ?c.tiplocs_only_json.sample,
        tiplocs_only_xml = c.tiplocs_only_xml.count,
        tiplocs_only_xml_sample = ?c.tiplocs_only_xml.sample,
        code_mismatches = c.code_mismatches.count,
        code_mismatches_sample = ?c.code_mismatches.sample,
        description_mismatches = c.description_mismatches,
        crosswalk_tiplocs_only_json = c.crosswalk_tiplocs_only_json.count,
        crosswalk_tiplocs_only_json_sample = ?c.crosswalk_tiplocs_only_json.sample,
        crosswalk_tiplocs_only_xml = c.crosswalk_tiplocs_only_xml.count,
        crosswalk_tiplocs_only_xml_sample = ?c.crosswalk_tiplocs_only_xml.sample,
        crosswalk_tiplocs_conflicting = c.crosswalk_tiplocs_conflicting.count,
        crosswalk_tiplocs_conflicting_sample = ?c.crosswalk_tiplocs_conflicting.sample,
        crosswalk_stanoxes_only_json = c.crosswalk_stanoxes_only_json.count,
        crosswalk_stanoxes_only_xml = c.crosswalk_stanoxes_only_xml.count,
        crosswalk_stanoxes_conflicting = c.crosswalk_stanoxes_conflicting.count,
        "compared the CORPUS JSON extract with the NLC XML snapshot"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc(
        nlc: &str,
        tiploc: Option<&str>,
        stanox: Option<&str>,
        crs: Option<&str>,
    ) -> CorpusLocation {
        CorpusLocation {
            nlc: nlc.to_string(),
            stanox: stanox.map(str::to_string),
            tiploc: tiploc.map(str::to_string),
            crs: crs.map(str::to_string),
            uic: None,
            nlc_desc: Some(format!("PLACE {nlc}")),
            nlc_desc16: None,
        }
    }

    #[test]
    fn identical_tiploc_sets_differ_only_by_the_json_only_rows() {
        let xml = vec![
            loc("370100", Some("QZAPLTN"), Some("90101"), Some("QZA")),
            loc("370200", Some("QZBRMTH"), Some("90102"), Some("QZB")),
        ];
        let mut json = xml.clone();
        json.push(loc("370900", None, None, None));
        let c = compare(&json, &xml);
        assert_eq!((c.json_rows, c.json_tiploc_rows, c.xml_rows), (3, 2, 2));
        assert_eq!(c.tiplocs_both, 2);
        assert_eq!(c.tiplocs_only_json.count + c.tiplocs_only_xml.count, 0);
        assert_eq!(c.code_mismatches.count, 0);
        assert_eq!(c.crosswalk_tiplocs_only_json.count, 0);
        assert_eq!(c.crosswalk_tiplocs_conflicting.count, 0);
    }

    #[test]
    fn coverage_code_and_crosswalk_differences_are_counted() {
        let json = vec![
            loc("370100", Some("QZAPLTN"), Some("90101"), Some("QZA")),
            loc("370200", Some("QZBRMTH"), Some("90102"), Some("QZB")),
            loc("370300", Some("QZONLYJ"), Some("90103"), Some("QZC")),
        ];
        let xml = vec![
            loc("370100", Some("QZAPLTN"), Some("90101"), Some("QZA")),
            // Same TIPLOC, another CRS: a code mismatch and a crosswalk
            // conflict.
            loc("370200", Some("QZBRMTH"), Some("90102"), Some("QZE")),
            loc("370400", Some("QZONLYX"), None, None),
        ];
        let c = compare(&json, &xml);
        assert_eq!(c.tiplocs_only_json.sample, ["QZONLYJ"]);
        assert_eq!(c.tiplocs_only_xml.sample, ["QZONLYX"]);
        assert_eq!(c.code_mismatches.sample, ["QZBRMTH"]);
        assert_eq!(c.crosswalk_tiplocs_only_json.sample, ["QZONLYJ"]);
        assert_eq!(c.crosswalk_tiplocs_conflicting.sample, ["QZBRMTH"]);
        assert_eq!(c.crosswalk_stanoxes_conflicting.sample, ["90102"]);
        let measures: BTreeMap<_, _> = c.measures().into_iter().collect();
        assert_eq!(measures["code_mismatches"], 1);
        assert_eq!(measures["xml_rows"], 3);
    }

    /// Compares a real extract with a real snapshot (neither committed):
    /// `CORPUS_EXTRACT_SAMPLE=/path/CORPUSExtract.json.gz
    /// CORPUS_XML_SAMPLE=/path/NLC20261009.xml.gz cargo test -p
    /// schedule-ingest -- --ignored --nocapture real_formats_compare`.
    #[test]
    #[ignore = "needs CORPUS_EXTRACT_SAMPLE and CORPUS_XML_SAMPLE pointing at real files"]
    fn real_formats_compare() {
        let (Ok(json_path), Ok(xml_path)) = (
            std::env::var("CORPUS_EXTRACT_SAMPLE"),
            std::env::var("CORPUS_XML_SAMPLE"),
        ) else {
            return;
        };
        let cap = 256 * 1024 * 1024;
        let json = crate::corpus::decompress(&std::fs::read(json_path).unwrap(), cap)
            .and_then(|bytes| crate::corpus::parse_extract(&bytes, 10_000))
            .unwrap();
        let xml = crate::corpus_xml::parse_gzipped(&std::fs::read(xml_path).unwrap(), cap, 10_000)
            .unwrap()
            .locations;
        let c = compare(&json, &xml);
        println!("{c:#?}");
        assert!(c.tiplocs_both > 10_000);
    }
}
