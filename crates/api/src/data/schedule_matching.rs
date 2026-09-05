//! Schedule-first resolution of a tracked-train pin's `train_uid`, per
//! docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md.
//! Attempted once at pin-creation time (`routes::train::post_track`) and
//! again periodically for every still-`pending` row
//! (`run_schedule_match_sweep`, `main.rs`'s new background loop) -- both
//! paths funnel through `attempt_schedule_match`, the only place this
//! crate ever calls `schedule_query::match_pin`.

use std::collections::HashMap;

use common::LineDefinition;

/// `CRS -> Vec<line_id>` (Decision 2 of the design spec), built from the
/// static `lines/*.toml` catalogue -- mirrors
/// `crates/schedule-reference/src/main.rs`'s own `lines_to_publish`
/// predicate exactly (a line qualifies if it has at least one
/// `tiploc`-bearing station), then further filters to only the
/// TIPLOC-bearing stations themselves, since a station with no TIPLOC has
/// no way to ever appear in a CIF calling-point list anyway. Built once
/// at `AppState::init` from `app.config.lines` (already loaded there for
/// `full_coverage_enabled_for`'s own use -- this is a pure re-keying of
/// data already in memory, no new I/O).
pub fn crs_to_line_ids(lines: &[LineDefinition]) -> HashMap<String, Vec<String>> {
    let mut index: HashMap<String, Vec<String>> = HashMap::new();
    for line in lines {
        if !line.stations.iter().any(|s| s.tiploc.is_some()) {
            continue;
        }
        for station in &line.stations {
            if station.tiploc.is_none() {
                continue;
            }
            let crs = station.crs.to_uppercase();
            let ids = index.entry(crs).or_default();
            if !ids.contains(&line.id) {
                ids.push(line.id.clone());
            }
        }
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str, stations: Vec<(&str, Option<&str>)>) -> LineDefinition {
        LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "rail".to_string(),
            category: "national-rail".to_string(),
            operators: vec![],
            stations: stations
                .into_iter()
                .map(|(crs, tiploc)| common::Station {
                    crs: crs.to_string(),
                    tiploc: tiploc.map(str::to_string),
                    role: "minor".to_string(),
                    segment: None,
                })
                .collect(),
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: std::collections::HashMap::new(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled: false,
        }
    }

    #[test]
    fn a_tiploc_bearing_station_maps_its_crs_to_its_line() {
        let lines = vec![line("wcml", vec![("EUS", Some("EUSTON"))])];
        let index = crs_to_line_ids(&lines);
        assert_eq!(index.get("EUS"), Some(&vec!["wcml".to_string()]));
    }

    #[test]
    fn a_crs_with_no_tiploc_on_its_station_entry_is_not_indexed() {
        let lines = vec![line("wcml", vec![("EUS", Some("EUSTON")), ("ZZZ", None)])];
        let index = crs_to_line_ids(&lines);
        assert_eq!(index.get("ZZZ"), None);
    }

    #[test]
    fn a_crs_shared_by_two_lines_maps_to_both() {
        let lines = vec![
            line("line-a", vec![("EUS", Some("EUSTON"))]),
            line("line-b", vec![("EUS", Some("EUSTON"))]),
        ];
        let index = crs_to_line_ids(&lines);
        let mut ids = index.get("EUS").cloned().unwrap_or_default();
        ids.sort();
        assert_eq!(ids, vec!["line-a".to_string(), "line-b".to_string()]);
    }

    #[test]
    fn a_line_with_no_tiploc_bearing_station_at_all_is_excluded_entirely() {
        let lines = vec![line("no-tiploc-line", vec![("ZZA", None), ("ZZB", None)])];
        let index = crs_to_line_ids(&lines);
        assert!(index.is_empty());
    }

    #[test]
    fn a_lowercase_crs_on_a_station_entry_is_indexed_uppercased() {
        let lines = vec![line("wcml", vec![("eus", Some("EUSTON"))])];
        let index = crs_to_line_ids(&lines);
        assert_eq!(index.get("EUS"), Some(&vec!["wcml".to_string()]));
    }
}
