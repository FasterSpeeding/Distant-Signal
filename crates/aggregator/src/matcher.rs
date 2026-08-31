//! Decide which lines a Knowledgebase incident affects, and classify the
//! scope of each match. Ported from `src/matcher.py`.

use std::collections::{HashMap, HashSet};

use common::{IncidentMessage, LineDefinition};

use crate::segments::SegmentRegistry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchScope {
    ExclusiveSegment,
    SharedSegment,
    StationHit,
    KeywordOnly,
    OperatorOnly,
}

// `segments`/`operators`/`keywords` are faithful ports of the Python
// prototype's `evidence` dict (`src/matcher.py`) and are intentionally kept
// as API surface for future consumers (e.g. richer disruption messages,
// debugging) even though only `.stations` is read today.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Evidence {
    pub stations: Vec<String>,
    pub segments: Vec<String>,
    pub operators: Vec<String>,
    pub keywords: Vec<String>,
}

pub struct Match<'a> {
    pub line: &'a LineDefinition,
    pub scope: MatchScope,
    pub evidence: Evidence,
}

/// Return all lines the incident could plausibly affect, classified.
pub fn lines_affected_by<'a>(
    incident: &IncidentMessage,
    lines: &'a HashMap<String, LineDefinition>,
    registry: &SegmentRegistry,
) -> Vec<Match<'a>> {
    let haystack = format!("{} {}", incident.summary, incident.description).to_lowercase();
    let mut out: Vec<Match<'a>> = Vec::new();

    for line in lines.values() {
        if is_excluded(line, &haystack) {
            continue;
        }
        if let Some(m) = match_one(line, incident, registry, &haystack) {
            out.push(m);
        }
    }

    // If any precise match exists, drop operator-only matches — they're
    // almost certainly false positives where another line on the same
    // operator is the actual target.
    let has_precise = out.iter().any(|m| m.scope != MatchScope::OperatorOnly);
    if has_precise {
        out.retain(|m| m.scope != MatchScope::OperatorOnly);
    }

    out
}

fn match_one<'a>(
    line: &'a LineDefinition,
    incident: &IncidentMessage,
    registry: &SegmentRegistry,
    haystack: &str,
) -> Option<Match<'a>> {
    let operator_overlap: Vec<String> = line
        .operators
        .iter()
        .filter(|op| incident.operators.contains(op))
        .cloned()
        .collect();
    let station_hits: Vec<String> = incident
        .affected_stations
        .iter()
        .filter(|crs| line.has_station(crs))
        .cloned()
        .collect();
    let keyword_hits: Vec<String> = line
        .match_keywords
        .iter()
        .filter(|kw| haystack.contains(&kw.to_lowercase()))
        .cloned()
        .collect();

    // Tier 1: station hits — try to classify by segment.
    if !station_hits.is_empty() {
        let segments: HashSet<String> = registry.segments_touched_by(line, &station_hits);
        let evidence = Evidence {
            stations: station_hits,
            segments: segments.iter().cloned().collect(),
            operators: operator_overlap,
            keywords: keyword_hits,
        };

        if !segments.is_empty() && segments.iter().all(|s| registry.is_exclusive_to(s, &line.id)) {
            return Some(Match { line, scope: MatchScope::ExclusiveSegment, evidence });
        }
        if !segments.is_empty() && segments.iter().any(|s| registry.is_shared(s)) {
            return Some(Match { line, scope: MatchScope::SharedSegment, evidence });
        }
        return Some(Match { line, scope: MatchScope::StationHit, evidence });
    }

    // Tier 2: keyword match.
    if !keyword_hits.is_empty() {
        return Some(Match {
            line,
            scope: MatchScope::KeywordOnly,
            evidence: Evidence { stations: vec![], segments: vec![], operators: operator_overlap, keywords: keyword_hits },
        });
    }

    // Tier 3: operator only.
    if !operator_overlap.is_empty() {
        return Some(Match {
            line,
            scope: MatchScope::OperatorOnly,
            evidence: Evidence { stations: vec![], segments: vec![], operators: operator_overlap, keywords: vec![] },
        });
    }

    None
}

fn is_excluded(line: &LineDefinition, haystack: &str) -> bool {
    line.excluded_keywords.iter().any(|kw| haystack.contains(&kw.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn load_line(id: &str) -> HashMap<String, LineDefinition> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        let all = LineDefinition::from_dir(&dir).expect("lines/ directory should parse");
        all.into_iter()
            .filter(|l| l.id == id)
            .map(|l| (l.id.clone(), l))
            .collect()
    }

    fn load_all_lines() -> HashMap<String, LineDefinition> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        LineDefinition::from_dir(&dir)
            .expect("lines/ directory should parse")
            .into_iter()
            .map(|l| (l.id.clone(), l))
            .collect()
    }

    fn incident(id: &str, summary: &str, description: &str, operators: &[&str], affected_stations: &[&str]) -> IncidentMessage {
        IncidentMessage {
            incident_id: id.to_string(),
            summary: summary.to_string(),
            description: description.to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            affected_stations: affected_stations.iter().map(|s| s.to_string()).collect(),
            priority: 0,
            validity: vec![],
            is_planned: false,
            is_cleared: false,
        }
    }

    #[test]
    fn excluded_keyword_vetoes_match() {
        let lines = load_line("wcml");
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("T1", "Cross Country delays", "Cross Country services are delayed at Rugby.", &[], &["RUG"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        assert!(matches.is_empty(), "excluded keyword should veto match");
    }

    #[test]
    fn keyword_only_match() {
        let lines = load_line("wcml");
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("T2", "WCML engineering", "Overnight engineering work on the West Coast Main Line.", &[], &[]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].scope, MatchScope::KeywordOnly);
    }

    #[test]
    fn swr_shared_trunk_incident_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("SWR-1", "Signal failure at Woking", "Signal failure causing delays to SWR services.", &["SW"], &["WOK"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("swr-south-west-main"));
        assert!(matched_ids.contains("swr-portsmouth-direct"));
        assert!(matched_ids.contains("swr-alton"));
        for m in &matches {
            if m.line.id.starts_with("swr-") {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    #[test]
    fn xc_hub_incident_propagates_to_every_cross_country_arm() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("XC-1", "Signal failure at Birmingham New Street", "Services are delayed.", &["XC"], &["BHM"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "cross-country".to_string(),
                "xc-manchester".to_string(),
                "xc-cardiff".to_string(),
                "xc-south-coast".to_string(),
                "xc-stansted".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    #[test]
    fn elizabeth_branch_incident_stays_on_its_branch() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("XR-1", "Trespass at Shenfield", "Trespass incident causing delays.", &["XR"], &["SNF"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["elizabeth-shenfield".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn swr_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("SWR-2", "Power supply issue at Alton", "Power supply problem causing delays at Alton.", &["SW"], &["AON"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["swr-alton".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.1 (ScotRail Central Belt): no sibling ScotRail line exists yet
    // in `lines/` to share `scotrail-central-belt`/
    // `scotrail-central-belt-edinburgh-throat` with, so there is no
    // shared-segment propagation to assert today -- see
    // `lines/scotrail-central-belt.toml`'s own comments for the segment-
    // naming groundwork left for Task 10.2. Only the exclusive-segment
    // non-propagation assertion applies for now, mirroring
    // `swr_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn scotrail_central_belt_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-1",
            "Signal failure at Falkirk High",
            "Signal failure causing delays to ScotRail services at Falkirk High.",
            &["SR"],
            &["FKK"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-central-belt".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.2 (ScotRail Glasgow Suburban): no other `lines/*.toml` file
    // touches North Clyde/Argyle Line territory yet, and this file
    // deliberately stops short of sharing a segment with
    // `scotrail-central-belt.toml` (see this file's own comments on the
    // Airdrie/Bathgate boundary decision) -- so there is no shared-segment
    // propagation to assert today, mirroring
    // `scotrail_central_belt_exclusive_segment_incident_does_not_propagate`.
    // Only the exclusive-segment non-propagation assertion applies for now.
    #[test]
    fn scotrail_glasgow_suburban_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-2",
            "Signal failure at Milngavie",
            "Signal failure causing delays to ScotRail services at Milngavie.",
            &["SR"],
            &["MLN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-glasgow-suburban".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.3 (ScotRail Ayrshire Coast): no other `lines/*.toml` file
    // touches this line's Glasgow-Ayr trunk or its Girvan/Stranraer branch
    // yet (see `lines/scotrail-ayrshire.toml`'s own comments), so there is
    // no shared-segment propagation to assert today -- only exclusive-
    // segment non-propagation for each of this file's two segments,
    // mirroring `scotrail_central_belt_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn scotrail_ayrshire_glasgow_ayr_trunk_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-3",
            "Signal failure at Kilwinning",
            "Signal failure causing delays to ScotRail services at Kilwinning.",
            &["SR"],
            &["KWN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-ayrshire".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn scotrail_ayrshire_stranraer_branch_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-4",
            "Level crossing fault at Girvan",
            "Level crossing fault causing delays to ScotRail services at Girvan.",
            &["SR"],
            &["GIR"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-ayrshire".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.4 (ScotRail Fife Circle + Borders Railway): this file
    // bundles two genuinely separate routes with distinct segment-name
    // prefixes (`scotrail-fife-circle*` / `scotrail-borders`), neither of
    // which is shared with any other `lines/*.toml` file today (see
    // `lines/scotrail-fife-borders.toml`'s own comments on why the
    // Edinburgh Waverley/Haymarket overlap with `scotrail-central-belt`
    // isn't modelled as a shared segment) -- so, mirroring Task 10.3's
    // two-exclusive-segments treatment, one exclusive-segment
    // non-propagation assertion per route, not a shared-segment one.
    #[test]
    fn scotrail_fife_circle_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-5",
            "Signal failure at Kirkcaldy",
            "Signal failure causing delays to ScotRail services at Kirkcaldy.",
            &["SR"],
            &["KDY"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-fife-borders".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn scotrail_borders_railway_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-6",
            "Points failure at Galashiels",
            "Points failure causing delays to ScotRail services at Galashiels.",
            &["SR"],
            &["GAL"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-fife-borders".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.5 (ScotRail Highland Main Line): no sibling `lines/*.toml`
    // file touches this line's `scotrail-highland-main-line` segment yet
    // (see `lines/scotrail-highland-main-line.toml`'s own comments on the
    // Inverness-area segment-naming handoff left open for Tasks
    // 10.6/10.7/10.10), so there is no shared-segment propagation to
    // assert today -- only exclusive-segment non-propagation, mirroring
    // `scotrail_central_belt_exclusive_segment_incident_does_not_propagate`.
    // The incident is placed at Kingussie (on the line's exclusive
    // Perth-Carrbridge segment), not Inverness, since Inverness's own
    // segment fate is deliberately left open for a later task to decide.
    #[test]
    fn scotrail_highland_main_line_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-7",
            "Signal failure at Kingussie",
            "Signal failure causing delays to ScotRail services at Kingussie.",
            &["SR"],
            &["KIN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-highland-main-line".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.6 (ScotRail Far North Line): the Wick and Thurso branches
    // each get their own exclusive segment
    // (`scotrail-far-north-wick`/`scotrail-far-north-thurso`), used only by
    // this one file, so an incident on either branch should stay local to
    // `scotrail-far-north` with `ExclusiveSegment` scope -- mirroring
    // `swr_exclusive_segment_incident_does_not_propagate`.
    //
    // This file also tags Inverness-Dingwall as
    // `scotrail-inverness-dingwall-trunk`, a segment name Task 10.7's
    // Kyle of Lochalsh Line (`lines/scotrail-kyle.toml`) now independently
    // confirmed and reused for its own Inverness-Dingwall stations -- see
    // `scotrail_shared_inverness_dingwall_trunk_incident_propagates` below
    // for the resulting cross-file `SharedSegment` propagation assertion.
    // The same-file trunk-vs-branch structure this file also has
    // (Inverness-Georgemas Junction shared by both the Wick and Thurso
    // branches) is separately asserted directly via `SegmentRegistry` in
    // `crates/aggregator/src/segments.rs`, since that particular sharing
    // is internal to one line/file and `MatchScope::SharedSegment` only
    // applies across distinct line files.
    #[test]
    fn scotrail_far_north_wick_branch_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-8",
            "Points failure at Wick",
            "Points failure causing delays to ScotRail services at Wick.",
            &["SR"],
            &["WCK"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-far-north".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn scotrail_far_north_thurso_branch_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-9",
            "Signal failure at Thurso",
            "Signal failure causing delays to ScotRail services at Thurso.",
            &["SR"],
            &["THS"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-far-north".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.7 (ScotRail Kyle of Lochalsh Line): this line's own exclusive
    // track west of Dingwall Junction (Garve through Kyle of Lochalsh) is
    // tagged `scotrail-kyle-exclusive`, used only by this one file, so an
    // incident there should stay local to `scotrail-kyle` with
    // `ExclusiveSegment` scope -- mirroring
    // `swr_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn scotrail_kyle_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-10",
            "Landslip near Strathcarron",
            "Landslip causing delays to ScotRail services at Strathcarron.",
            &["SR"],
            &["STC"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-kyle".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.7 (ScotRail Kyle of Lochalsh Line): the load-bearing
    // shared-trunk test this Batch 10 pairing exists for. Task 10.6
    // (`scotrail-far-north.toml`) reserved the segment name
    // `scotrail-inverness-dingwall-trunk` for the Inverness-Dingwall
    // stretch both lines' services physically run over; this file
    // independently re-confirmed that shared-track claim (see
    // `lines/scotrail-kyle.toml`'s own Sources comments) and reused the
    // exact same segment name. Now that both files exist,
    // `SegmentRegistry::is_shared` correctly reports the segment shared
    // between two distinct line IDs, so an incident at Dingwall (a
    // station on that shared segment) should match BOTH
    // `scotrail-far-north` and `scotrail-kyle`, each with
    // `MatchScope::SharedSegment` -- mirroring
    // `swr_shared_trunk_incident_propagates`.
    #[test]
    fn scotrail_shared_inverness_dingwall_trunk_incident_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-11",
            "Signal failure at Dingwall",
            "Signal failure causing delays to ScotRail services at Dingwall.",
            &["SR"],
            &["DIN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["scotrail-far-north".to_string(), "scotrail-kyle".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    // Task 10.8/10.9 (ScotRail West Highland Line pairing): the
    // load-bearing shared-trunk test this Batch 10 pairing exists for,
    // mirroring `scotrail_shared_inverness_dingwall_trunk_incident_propagates`.
    // Task 10.8 (`scotrail-west-highland-fort-william.toml`) reserved the
    // segment name `scotrail-west-highland-glasgow-crianlarich` for the
    // Glasgow-Crianlarich stretch both West Highland Line branches
    // physically run over (combined trains splitting/joining at
    // Crianlarich); Task 10.9 (`scotrail-west-highland-oban.toml`)
    // independently re-confirmed that shared-track claim against a fresh
    // fetch of Wikipedia's "West Highland Line" and "Crianlarich railway
    // station" articles (see that file's own verification-note comments)
    // and reused the exact same segment name. `SegmentRegistry::is_shared`
    // now correctly reports the segment shared between two distinct line
    // IDs, so an incident at Ardlui (a station on that shared segment)
    // matches BOTH `scotrail-west-highland-fort-william` and
    // `scotrail-west-highland-oban`, each with `MatchScope::SharedSegment`.
    #[test]
    fn scotrail_west_highland_shares_glasgow_crianlarich_trunk_incident_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-12",
            "Landslip near Ardlui",
            "Landslip causing delays to ScotRail services at Ardlui.",
            &["SR"],
            &["AUI"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "scotrail-west-highland-fort-william".to_string(),
                "scotrail-west-highland-oban".to_string()
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    // Task 10.8: this line's own exclusive track between Crianlarich and
    // Fort William (tagged `scotrail-west-highland-fort-william-exclusive`,
    // used only by this one file) should stay local to
    // `scotrail-west-highland-fort-william` with `ExclusiveSegment` scope --
    // mirroring `scotrail_ayrshire_glasgow_ayr_trunk_incident_does_not_propagate`.
    #[test]
    fn scotrail_west_highland_fort_william_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-13",
            "Signal failure at Tulloch",
            "Signal failure causing delays to ScotRail services at Tulloch.",
            &["SR"],
            &["TUL"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-west-highland-fort-william".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.8: the Mallaig extension beyond Fort William (tagged
    // `scotrail-west-highland-mallaig`, used only by this one file) is a
    // second, distinct exclusive segment within the same file -- mirroring
    // `scotrail_ayrshire_stranraer_branch_incident_does_not_propagate`'s
    // identical trunk+branch treatment.
    #[test]
    fn scotrail_west_highland_mallaig_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-14",
            "Landslip near Glenfinnan",
            "Landslip causing delays to ScotRail services at Glenfinnan.",
            &["SR"],
            &["GLF"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-west-highland-fort-william".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.8/10.9: unlike the Crianlarich reservation above, this
    // line's Glasgow-area sharing with `scotrail-glasgow-suburban.toml`'s
    // own `scotrail-glasgow-suburban-west-trunk` segment (Dalmuir -
    // Dumbarton Central) is a REAL, already-merged sibling -- see this
    // line's own Sources comments for the independent verification. Task
    // 10.9 (`scotrail-west-highland-oban.toml`) independently confirmed
    // that Oban services also call at Dumbarton Central before diverging
    // near Craigendoran Junction (Dumbarton Central's own Wikipedia
    // article explicitly names "trains ... between Glasgow and Oban and
    // Mallaig") and reused this exact segment name for its own DMR/DBC
    // entries too, making this a genuine three-way shared segment. An
    // incident at Dumbarton Central should therefore match ALL THREE of
    // `scotrail-glasgow-suburban`, `scotrail-west-highland-fort-william`
    // and `scotrail-west-highland-oban` with `MatchScope::SharedSegment`,
    // mirroring `scotrail_shared_inverness_dingwall_trunk_incident_propagates`.
    #[test]
    fn scotrail_west_highland_shares_glasgow_suburban_west_trunk_incident_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-15",
            "Signal failure at Dumbarton Central",
            "Signal failure causing delays to ScotRail services at Dumbarton Central.",
            &["SR"],
            &["DBC"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "scotrail-glasgow-suburban".to_string(),
                "scotrail-west-highland-fort-william".to_string(),
                "scotrail-west-highland-oban".to_string()
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    // Task 10.9 (ScotRail West Highland Line, Oban arm): this line's own
    // exclusive track beyond Crianlarich (tagged
    // `scotrail-west-highland-oban-exclusive`, used only by this one file)
    // should stay local to `scotrail-west-highland-oban` with
    // `ExclusiveSegment` scope -- mirroring
    // `scotrail_west_highland_fort_william_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn scotrail_west_highland_oban_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-16",
            "Signal failure at Taynuilt",
            "Signal failure causing delays to ScotRail services at Taynuilt.",
            &["SR"],
            &["TAY"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-west-highland-oban".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.9 (fix round 1, post-review): GLQ (Glasgow Queen Street) is
    // reused verbatim as `scotrail-west-highland-glasgow-terminus` by both
    // `scotrail-west-highland-fort-william` and
    // `scotrail-west-highland-oban` -- see both files' own GLQ comments
    // for why (the "combined trains ... splitting at Crianlarich" sourcing
    // means the shared corridor genuinely starts at GLQ itself, not just
    // from Dalmuir onward). GLQ is ALSO a real `[[stations]]` entry in
    // `scotrail-central-belt.toml` (its own exclusive
    // `scotrail-central-belt` segment) and `scotrail-glasgow-suburban.toml`
    // (its own exclusive `scotrail-glasgow-suburban-core` segment) -- both
    // genuinely different physical platform groups/services at the same
    // named station, unaffected by this fix. So an incident at GLQ
    // correctly matches all four lines, but with different scopes: the two
    // West Highland lines get `SharedSegment` (this test's own point),
    // while Central Belt and Glasgow Suburban each stay `ExclusiveSegment`
    // on their own unrelated segments.
    #[test]
    fn scotrail_west_highland_shares_glasgow_terminus_incident_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-17",
            "Points failure at Glasgow Queen Street",
            "Points failure causing delays to ScotRail services at Glasgow Queen Street.",
            &["SR"],
            &["GLQ"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "scotrail-central-belt".to_string(),
                "scotrail-glasgow-suburban".to_string(),
                "scotrail-west-highland-fort-william".to_string(),
                "scotrail-west-highland-oban".to_string(),
            ])
        );
        for m in &matches {
            let expected = match m.line.id.as_str() {
                "scotrail-west-highland-fort-william" | "scotrail-west-highland-oban" => {
                    MatchScope::SharedSegment
                }
                _ => MatchScope::ExclusiveSegment,
            };
            assert_eq!(m.scope, expected, "{} scope mismatch", m.line.id);
        }
    }

    // Task 10.10 (ScotRail Aberdeen - Inverness Line): this line's own
    // exclusive track (Aberdeen through Inverness Airport, tagged
    // `scotrail-aberdeen-inverness`) is used only by this one file, so an
    // incident there should stay local with `ExclusiveSegment` scope --
    // mirroring `swr_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn scotrail_aberdeen_inverness_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-18",
            "Signal failure at Elgin",
            "Signal failure causing delays to ScotRail services at Elgin.",
            &["SR"],
            &["ELG"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["scotrail-aberdeen-inverness".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 10.10 (ScotRail Aberdeen - Inverness Line): the load-bearing
    // shared-trunk test this pairing exists for. Task 10.5
    // (`scotrail-highland-main-line.toml`) reserved the segment name
    // `scotrail-highland-inverness-approach` for its own Inverness station
    // entry, deliberately not assuming a shared segment on
    // platform-sharing evidence alone; this file independently confirmed
    // genuine track-sharing (Wikipedia's Millburn Junction detail,
    // cross-checked against railwaycodes.org.uk's own ELR database -- see
    // `lines/scotrail-aberdeen-inverness.toml`'s own Sources comments) and
    // reused the exact same segment name.
    //
    // Inverness is also a station on `scotrail-far-north` and
    // `scotrail-kyle` (tagged `scotrail-inverness-dingwall-trunk`, that
    // pair's own independently-confirmed shared trunk from Task 10.6/10.7)
    // -- `LineDefinition::has_station` matches on station presence
    // regardless of segment name, so an incident at Inverness hits all
    // four lines, mirroring
    // `scotrail_west_highland_shares_glasgow_terminus_incident_propagates`'s
    // precedent for a station where two independent shared-trunk pairs
    // happen to converge. Since Inverness sits on a genuine cross-file
    // shared segment for *both* pairs (not merely a same-line exclusive
    // segment for either), all four lines get `MatchScope::SharedSegment`
    // here -- unlike the Glasgow Queen Street case, where only two of the
    // four lines had a cross-file shared segment at that station.
    #[test]
    fn scotrail_shared_highland_inverness_approach_incident_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SR-19",
            "Points failure at Inverness",
            "Points failure causing delays to ScotRail services at Inverness.",
            &["SR"],
            &["INV"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "scotrail-highland-main-line".to_string(),
                "scotrail-aberdeen-inverness".to_string(),
                "scotrail-far-north".to_string(),
                "scotrail-kyle".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }
}
