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

    // southeastern-main-line was the first Southeastern file in this
    // catalogue (Batch 5, Task 5.1). At the time it shared no segment with
    // any already-curated line — thameslink-core.toml overlaps at LBG by
    // station only, not by segment. That's still true of `seml-weald`
    // specifically (Task 5.3's southeastern-highspeed.toml overlaps this
    // line only at AFK, by station, on its own `hs1-ashford` segment - see
    // afk_station_overlap_matches_both_seml_and_hs1_as_independent_exclusive_segments
    // below), so an incident on a `seml-weald` station untouched by any
    // other file should still stay exclusive to this line alone.
    #[test]
    fn seml_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-1",
            "Signal failure at Tonbridge",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["TON"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southeastern-main-line".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // southeastern-chatham (Batch 5, Task 5.2) shares no segment with any
    // other already-curated line. Its DVP overlap with
    // southeastern-main-line.toml is station-only (the two lines approach
    // Dover Priory from physically different directions), documented in
    // southeastern-chatham.toml's own header comment. Since Task 5.3
    // (southeastern-highspeed.toml) reused `chatham-medway`/
    // `chatham-coastal` verbatim as a genuine shared trunk (see
    // hs1_chatham_medway_shared_segment_incident_propagates_to_both_lines
    // below), this test now exercises a `chatham-medway` station the new
    // file doesn't touch (Meopham, between Longfield and Sole Street -
    // west of Strood, where the Javelin's North Kent pattern joins), to
    // confirm the untouched part of chatham-medway still stays exclusive.
    #[test]
    fn chatham_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-2",
            "Signal failure at Meopham",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["MEP"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southeastern-chatham".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // The two branches within this one file (chatham-coastal via Ramsgate,
    // chatham-dover via Canterbury East) are both exclusive to
    // southeastern-chatham - an incident on the Dover branch shouldn't
    // pull in southeastern-main-line even though both lines terminate at
    // DVP (station overlap only, not a shared segment; see the file's own
    // header comment).
    #[test]
    fn chatham_dover_branch_incident_does_not_propagate_to_seml() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-3",
            "Points failure at Adisham",
            "Points failure causing delays to Southeastern services.",
            &["SE"],
            &["ADM"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southeastern-chatham".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // southeastern-highspeed (Batch 5, Task 5.3) is the domestic "Javelin"
    // HS1 service. Its St Pancras - Stratford International - Ebbsfleet
    // International trunk (`hs1-domestic`) is purpose-built high-speed
    // infrastructure no other curated line touches, so an incident there
    // should stay exclusive to this file - mirrors
    // seml_exclusive_segment_incident_does_not_propagate above.
    #[test]
    fn hs1_domestic_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-4",
            "Overhead line problem at Stratford International",
            "An overhead line problem is causing delays to Southeastern high speed services.",
            &["SE"],
            &["SFA"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southeastern-highspeed".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Ashford International (AFK) is on both southeastern-main-line.toml
    // (`seml-weald`) and southeastern-highspeed.toml (`hs1-ashford`) -
    // deliberately different segment names, because HS1 reaches Ashford via
    // its own purpose-built alignment through the North Downs Tunnel, not
    // via SEML's classic Sevenoaks/Tonbridge route. Per this task's brief
    // and both files' header comments, that's station overlap, not a
    // shared trunk: an AFK incident should match both lines independently,
    // each still scoped ExclusiveSegment, never SharedSegment.
    #[test]
    fn afk_station_overlap_matches_both_seml_and_hs1_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-5",
            "Signal failure at Ashford International",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["AFK"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["southeastern-main-line".to_string(), "southeastern-highspeed".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Past Ashford, the Javelin runs over the same physical track
    // southeastern-main-line.toml documents as `seml-coast` (there is only
    // one route from Ashford to Dover via Folkestone). This file
    // deliberately does NOT reuse that segment name, though: `is_shared`
    // treats an entire segment name as shared the moment two files use it,
    // and this file doesn't call at every `seml-coast` station (e.g. it
    // skips Westenhanger/Sandling) - reusing the name verbatim would
    // therefore also mark those untouched stations SharedSegment. So this
    // is kept as station overlap on this file's own `hs1-ashford` segment
    // (see the file's header comment) - both lines still match a
    // Folkestone Central incident, but each independently as
    // ExclusiveSegment.
    #[test]
    fn hs1_ashford_station_overlap_matches_both_seml_and_hs1_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-6",
            "Points failure at Folkestone Central",
            "Points failure causing delays to Southeastern services.",
            &["SE"],
            &["FKC"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["southeastern-main-line".to_string(), "southeastern-highspeed".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Same reasoning again for the North Kent pattern: from Strood onward
    // this file's `hs1-northkent` segment runs over the same physical
    // track as southeastern-chatham.toml's `chatham-medway`/
    // `chatham-coastal`, but the name isn't reused (this file doesn't
    // touch e.g. Longfield/Meopham/Sole Street), so it's station overlap,
    // not a shared trunk - both lines match a Ramsgate incident
    // independently, each ExclusiveSegment.
    #[test]
    fn hs1_northkent_station_overlap_matches_both_chatham_and_hs1_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-7",
            "Signal failure at Ramsgate",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["RAM"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["southeastern-chatham".to_string(), "southeastern-highspeed".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // southeastern-metro-north-kent (Batch 5, Task 5.4) covers the
    // Bexleyheath line and Dartford Loop line, both diverging from a
    // shared London Bridge-Lewisham trunk (`southeastern-lewisham-
    // corridor`). Per this file's own header comment (FINDING 2), research
    // for this task could NOT confirm the gap analysis's premise that
    // Thameslink genuinely shares that trunk under normal service - so no
    // sibling file uses `southeastern-lewisham-corridor` yet, and an
    // incident on this line's own exclusive Bexleyheath branch (past the
    // Lewisham junction) should stay exclusive to this line alone. Mirrors
    // swr_exclusive_segment_incident_does_not_propagate and
    // elizabeth_branch_incident_stays_on_its_branch above.
    #[test]
    fn senk_bexleyheath_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-8",
            "Points failure at Bexleyheath",
            "Points failure causing delays to Southeastern services.",
            &["SE"],
            &["BXH"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southeastern-metro-north-kent".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Same for the Dartford Loop branch (the other branch in this one
    // file, diverging from the shared trunk at Hither Green rather than at
    // Lewisham itself) - an incident on it should also stay exclusive to
    // this line, and shouldn't spuriously pull in the Bexleyheath branch's
    // own segment name either (the two branches use different segment
    // names, `senk-bexleyheath` vs `senk-dartford-loop`, despite being the
    // same file/line).
    #[test]
    fn senk_dartford_loop_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-9",
            "Signal failure at Sidcup",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["SID"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southeastern-metro-north-kent".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // LBG is also thameslink-core.toml's own terminus (its own segment
    // ends there too) and southeastern-main-line.toml's own `seml-london`
    // station, but per this file's header comment that's station overlap
    // only, not a shared trunk - same judgment southeastern-main-line.toml
    // already made for LBG/Thameslink. Confirms an LBG incident matches
    // all three lines independently, each still scoped ExclusiveSegment,
    // never SharedSegment - mirrors
    // afk_station_overlap_matches_both_seml_and_hs1_as_independent_exclusive_segments
    // above.
    //
    // Task 5.5 (southeastern-hayes-line.toml) added a fourth LBG overlap:
    // its own `hayes-london` segment also calls at LBG (see that file's own
    // header comment for why it does NOT reuse senk's
    // `southeastern-lewisham-corridor` name despite passing through the
    // same station - the Hayes line's own calling pattern skips New
    // Cross/St Johns, so the two runs aren't confirmed to share physical
    // track for that stretch).
    //
    // NOTE for Task 5.14 (lines/thameslink-southern.toml, not yet
    // written): this task could not add the shared-segment propagation
    // test the batch's testing convention otherwise requires (mirrors
    // swr_shared_trunk_incident_propagates /
    // xc_hub_incident_propagates_to_every_cross_country_arm) because that
    // sibling file doesn't exist yet. If Task 5.14's own research
    // independently confirms genuine Thameslink running over the London
    // Bridge-Lewisham stretch and it reuses `southeastern-lewisham-
    // corridor` verbatim, its implementer should add a test here (or in
    // that task's own matcher tests) asserting an incident on that shared
    // segment matches BOTH `southeastern-metro-north-kent` and
    // `thameslink-southern` with `MatchScope::SharedSegment`.
    #[test]
    fn lbg_station_overlap_matches_senk_thameslink_core_seml_and_hayes_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-10",
            "Signal failure at London Bridge",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["LBG"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "southeastern-metro-north-kent".to_string(),
                "thameslink-core".to_string(),
                "southeastern-main-line".to_string(),
                "southeastern-hayes-line".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Task 5.5 (southeastern-hayes-line.toml). An incident on the Hayes
    // line's own exclusive branch (past Lewisham, e.g. West Wickham) should
    // stay exclusive to this line - mirrors
    // swr_exclusive_segment_incident_does_not_propagate and
    // elizabeth_branch_incident_stays_on_its_branch above.
    #[test]
    fn hayes_branch_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-11",
            "Signal failure at West Wickham",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["WWI"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southeastern-hayes-line".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.5 (southeastern-hayes-line.toml). Lewisham (LEW) is a station
    // overlap between this file's own `hayes-london` segment and
    // southeastern-metro-north-kent.toml's `southeastern-lewisham-corridor`
    // - two different segment names for the same station, per this file's
    // own header comment (not a shared trunk, since the Hayes line's own
    // calling pattern diverges from senk's before Lewisham). Confirms an
    // incident there matches both lines independently, each still scoped
    // ExclusiveSegment, never SharedSegment.
    #[test]
    fn lew_station_overlap_matches_hayes_line_and_senk_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SE-12",
            "Signal failure at Lewisham",
            "Signal failure causing delays to Southeastern services.",
            &["SE"],
            &["LEW"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["southeastern-hayes-line".to_string(), "southeastern-metro-north-kent".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }
}
