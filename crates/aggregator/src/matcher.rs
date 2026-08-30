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
    // Updated by Task 5.6 (southern-brighton-main-line.toml): that file also
    // has a station at LBG (its own `southern-bml-north` segment, named as a
    // courtesy hand-off for Task 5.14's thameslink-southern.toml, not yet a
    // real cross-file shared trunk - see that file's own header comment), so
    // it now joins this set as a fifth independent exclusive-segment match.
    //
    // Updated by Task 5.9 (southern-oxted-uckfield.toml): that file also has
    // a station at LBG (its own `oxted-london-bridge-approach` segment - the
    // usual London terminus for its Uckfield branch service, see that
    // file's own header comment for why this is station overlap, not a
    // shared trunk, with every other line here), so it now joins this set
    // as a sixth independent exclusive-segment match.
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
                "southern-brighton-main-line".to_string(),
                "southern-oxted-uckfield".to_string(),
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

    // Task 5.6 (southern-brighton-main-line.toml). An incident on this
    // line's own exclusive `southern-bml-south` segment (past the ECR
    // junction) should stay exclusive to this line alone - mirrors
    // swr_exclusive_segment_incident_does_not_propagate and
    // elizabeth_branch_incident_stays_on_its_branch above. Uses Hassocks
    // (HSK) rather than Brighton (BTN) itself: Task 5.7
    // (southern-coastway-east.toml) added its own station at BTN too (a
    // real station overlap, since Coastway East also originates there -
    // see that file's own header comment), so BTN alone no longer proves
    // "no other already-curated line touches this station" the way it did
    // when this test was first written - that overlap is now covered
    // separately by
    // btn_station_overlap_matches_coastway_east_and_brighton_main_line_as_independent_exclusive_segments
    // below. HSK remains exclusive to this line.
    #[test]
    fn southern_bml_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-1",
            "Signal failure at Hassocks",
            "Signal failure causing delays to Southern services.",
            &["SN"],
            &["HSK"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southern-brighton-main-line".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.6. NOTE ON WHAT THIS ACTUALLY PROVES (corrected after review -
    // the original comment here overclaimed): an incident tagged with the
    // `GX` operator at Preston Park - one of Gatwick Express's own
    // (Wikipedia-only-sourced, see southern-brighton-main-line.toml's own
    // header comment) peak-only calling points - still resolves to
    // `southern-brighton-main-line`, ExclusiveSegment. But this is a
    // station-hit match: `match_one`'s Tier 1 path matches on
    // `line.has_station(crs)` alone (see `common::LineDefinition::
    // has_station`) and never consults `operators`, so this test would pass
    // identically even if `GX` were never added to this line's `operators`
    // list - PRP is already this line's own station regardless of who's
    // asking. It does NOT exercise the `operators` field or prove the
    // fold-in decision "works" in the sense the file's own comment claims.
    // What it does confirm: a `GX`-tagged incident at a real Brighton Main
    // Line station isn't accidentally excluded or misrouted by this line's
    // matching logic - a narrower but still real assurance.
    //
    // The `operators` field's actual role - LDBWS sample classification via
    // `belongs_to_line` - is exercised by a separate test,
    // `belongs_to_line_gatwick_express_operator_folds_in_via_operators_list`
    // in `aggregation.rs`, which WOULD fail without `GX` in `operators`.
    #[test]
    fn southern_bml_station_hit_matches_regardless_of_gx_or_sn_operator_tag() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "GX-1",
            "Delays at Preston Park",
            "Gatwick Express services are delayed at Preston Park.",
            &["GX"],
            &["PRP"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southern-brighton-main-line".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.7 (southern-coastway-east.toml). An incident on this line's
    // own exclusive `coastway-east-hastings` segment (past the Lewes
    // junction) should stay exclusive to this line alone - mirrors
    // swr_exclusive_segment_incident_does_not_propagate and
    // elizabeth_branch_incident_stays_on_its_branch above. This line is a
    // genuinely standalone route (no sibling file shares any of its
    // segment names - see that file's own header comment), so this also
    // confirms no accidental cross-file match: no shared-segment
    // propagation test is added for this line, per COMMON.md's own
    // "skip only for a genuinely standalone line" exception.
    #[test]
    fn coastway_east_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-2",
            "Signal failure at Eastbourne",
            "Signal failure causing delays to Southern services.",
            &["SN"],
            &["EBN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southern-coastway-east".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.7, updated by Task 5.8. Brighton (BTN) is also
    // southern-brighton-main-line.toml's own terminus, but per this file's
    // own header comment that's station overlap only (the two lines
    // diverge immediately east of Brighton onto physically different
    // routes, and use different segment names - `coastway-east-brighton`
    // here vs `southern-bml-victoria`/`southern-bml-south` there) - same
    // judgment call as the LBG overlaps exercised above. Task 5.8
    // (southern-coastway-west.toml) added a third line at this same
    // station (its own `coastway-west-brighton` segment, diverging west
    // out of Brighton) - see that file's own header comment. Confirms an
    // incident at Brighton matches all three lines independently, each
    // still scoped ExclusiveSegment, never SharedSegment.
    #[test]
    fn btn_station_overlap_matches_coastway_east_and_brighton_main_line_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-3",
            "Signal failure at Brighton",
            "Signal failure causing delays to Southern services.",
            &["SN"],
            &["BTN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "southern-coastway-east".to_string(),
                "southern-brighton-main-line".to_string(),
                "southern-coastway-west".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Task 5.8 (southern-coastway-west.toml). An incident on this line's
    // own exclusive `coastway-west-brighton` segment (a station no other
    // curated file touches) should stay exclusive to this line alone -
    // mirrors swr_exclusive_segment_incident_does_not_propagate and
    // coastway_east_exclusive_segment_incident_does_not_propagate above.
    // This line shares no segment name with any sibling file (its two
    // real overlaps - Brighton with southern-brighton-main-line.toml/
    // southern-coastway-east.toml, and Havant/Portsmouth with
    // swr-portsmouth-direct.toml - are both deliberately station overlap
    // only, per this file's own header comment), so no SharedSegment
    // propagation test is added for this line, per COMMON.md's own "skip
    // only for a genuinely standalone line" exception - the two station-
    // overlap tests below exercise both real overlaps instead.
    #[test]
    fn coastway_west_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-4",
            "Signal failure at Chichester",
            "Signal failure causing delays to Southern services.",
            &["SN"],
            &["CCH"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southern-coastway-west".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.8. Havant (HAV) is also swr-portsmouth-direct.toml's own
    // station (on that file's `swr-portsmouth-direct` segment). Per this
    // file's own STATION-OVERLAP AT HAVANT/PORTSMOUTH header comment: this
    // is genuine physical track-sharing (Southern's West Coastway stopping
    // service and SWR's Portsmouth Direct service both run Havant-
    // Bedhampton-Hilsea-Fratton-Portsmouth into Portsmouth), but per the
    // cross-operator precedent xc-south-coast.toml/xc-manchester.toml
    // already set, segment names are only reused between sibling lines of
    // the SAME operator - SN and SW are different operators, so this is
    // treated as station overlap, not a shared trunk, same judgment call
    // as the AFK/Ramsgate overlaps between Southeastern and HS1 above.
    // Confirms an incident at Havant matches both lines independently,
    // each still scoped ExclusiveSegment, never SharedSegment.
    #[test]
    fn hav_station_overlap_matches_coastway_west_and_swr_portsmouth_direct_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-5",
            "Signal failure at Havant",
            "Signal failure causing delays to services.",
            &["SN"],
            &["HAV"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["southern-coastway-west".to_string(), "swr-portsmouth-direct".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Task 5.9 (southern-oxted-uckfield.toml). An incident on this line's
    // own exclusive `oxted-uckfield-branch` segment (past the Hurst Green
    // junction) should stay exclusive to this line alone - mirrors
    // swr_exclusive_segment_incident_does_not_propagate and
    // elizabeth_branch_incident_stays_on_its_branch above. This line shares
    // no segment name with any sibling file (its three real overlaps - VIC,
    // LBG and ECR, all station overlap only per this file's own header
    // comment - are exercised by the two tests below and by the updated LBG
    // test above), so no SharedSegment propagation test is added for this
    // line, per COMMON.md's own "skip only for a genuinely standalone line"
    // exception.
    #[test]
    fn oxted_uckfield_branch_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-6",
            "Signal failure at Buxted",
            "Signal failure causing delays to Southern services.",
            &["SN"],
            &["BXD"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southern-oxted-uckfield".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.9. Same for the East Grinstead branch (the other branch past
    // Hurst Green Junction) - an incident there should also stay exclusive
    // to this line. Per this file's own header comment, East Grinstead is
    // confirmed as Southern's own Oxted line terminus (not Thameslink
    // territory, despite the gap analysis grouping it with Thameslink's
    // southern branches) - no Thameslink sibling file exists yet, so this
    // stays a plain exclusive-segment case for now; see that file's own
    // HAND-OFF NOTE for what a future Thameslink southern-branches file
    // should re-check.
    #[test]
    fn oxted_east_grinstead_branch_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-7",
            "Points failure at Lingfield",
            "Points failure causing delays to Southern services.",
            &["SN"],
            &["LFD"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["southern-oxted-uckfield".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.9. Victoria (VIC) is also southern-brighton-main-line.toml's
    // own terminus and southeastern-chatham.toml's own station. Per this
    // file's own STATION OVERLAP AT VIC / LBG / ECR header comment: whether
    // Oxted line services physically share fast/slow tracks with those
    // other services out of Victoria was not confirmed to COMMON.md's bar,
    // so this is treated as station overlap only (this file's own
    // `oxted-victoria-approach` segment, not reusing either sibling's
    // segment name) - same judgment call as the AFK/Ramsgate/LBG overlaps
    // exercised elsewhere in this module. Confirms an incident at Victoria
    // matches all three lines independently, each still scoped
    // ExclusiveSegment, never SharedSegment.
    #[test]
    fn vic_station_overlap_matches_brighton_main_line_chatham_and_oxted_uckfield_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "SN-8",
            "Signal failure at Victoria",
            "Signal failure causing delays to Southern services.",
            &["SN"],
            &["VIC"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "southern-brighton-main-line".to_string(),
                "southeastern-chatham".to_string(),
                "southern-oxted-uckfield".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Task 5.10 (great-northern-kings-lynn.toml). No shared-segment test is
    // added for the LNER pairing documented in that file's own LNER HAND-OFF
    // comment (`gn-ecml-slow-lines`) - `lines/lner-ecml.toml` (Batch 6) is
    // being written in a separate, parallel git worktree and does not exist
    // here, so a test asserting it would fail. Add that test once both
    // files exist and Batch 6 confirms whether it reuses the segment name.
    //
    // Originally this test also covered the direct Peterborough branch
    // (using Huntingdon) to show it stayed exclusive to this line. Task 5.13
    // (thameslink-cambridge.toml) confirmed genuine track-sharing on that
    // exact branch instead (see gn_peterborough_branch_shared_trunk_incident_
    // propagates_to_thameslink_cambridge below) - every station on that
    // branch is now genuinely shared, so there is no longer an exclusive
    // proof point left on it. That half of this test is retired in favour of
    // the new shared-trunk test, which asserts the real current behaviour.
    //
    // Same reasoning for the Cambridge/King's Lynn branch (the other branch
    // past Hitchin): Baldock (BDK) is no longer exclusive to this line
    // either, since Task 5.13's own semi-fast Cambridge service also stops
    // there (station overlap only, not a shared trunk - see
    // bdk_station_overlap_matches_great_northern_kings_lynn_and_thameslink_cambridge_as_independent_exclusive_segments
    // below). Meldreth (MEL), which that file's own MEL/STH/FXN EXCLUSION
    // header comment confirms Thameslink's semi-fast service skips
    // entirely, remains genuinely untouched by any other curated line and
    // takes over as this test's proof point.
    #[test]
    fn gn_kings_lynn_cambridge_branch_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "GN-2",
            "Points failure at Meldreth",
            "Points failure causing delays to GN train services.",
            &["GN"],
            &["MEL"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["great-northern-kings-lynn".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.10. Cambridge (CBG) is also xc-stansted.toml's own terminus,
    // reached via a physically distinct route (Ely/March, not Hitchin/
    // Royston) - per that file's own STATION OVERLAP comment, this is
    // station overlap only, not a shared trunk. Confirms an incident there
    // matches both lines independently, each still scoped ExclusiveSegment,
    // never SharedSegment - same pattern as
    // vic_station_overlap_matches_brighton_main_line_chatham_and_oxted_uckfield_as_independent_exclusive_segments
    // above.
    //
    // Updated by Task 5.13 (thameslink-cambridge.toml): that file's own
    // Cambridge Line branch also terminates at CBG (its own
    // `thameslink-cambridge-branch` segment - see that file's own OVERLAP
    // (d) header comment), so it now joins this set as a third independent
    // exclusive-segment match.
    #[test]
    fn cbg_station_overlap_matches_great_northern_kings_lynn_and_xc_stansted_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "GN-3",
            "Signal failure at Cambridge",
            "Signal failure causing delays to train services.",
            &["GN"],
            &["CBG"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "great-northern-kings-lynn".to_string(),
                "xc-stansted".to_string(),
                "thameslink-cambridge".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Task 5.11 (great-northern-suburban.toml). This file's research
    // (documented in its own SHARED-TRUNK RESEARCH FINDING header comment)
    // confirmed the Moorgate suburban service physically joins the East
    // Coast Main Line at Finsbury Park and runs over the same
    // `gn-ecml-slow-lines` corridor `great-northern-kings-lynn.toml` already
    // documents as far as Welwyn Garden City - but with a different calling
    // pattern (Moorgate stops at extra local stations that file's semi-fast
    // service skips), so - mirroring `southeastern-highspeed.toml`'s own
    // decision for the same "same track, different calling pattern"
    // situation - the segment name is deliberately NOT reused. This file is
    // therefore genuinely standalone with respect to cross-file segment
    // sharing (no `SharedSegment` propagation test is added, per COMMON.md's
    // own exception for a standalone line). This test confirms an incident
    // on this line's own exclusive `gn-moorgate-hertford-branch` segment
    // (Winchmore Hill - not a station either sibling GN file touches) stays
    // exclusive to this line alone - mirrors
    // swr_exclusive_segment_incident_does_not_propagate and
    // elizabeth_branch_incident_stays_on_its_branch above.
    #[test]
    fn gn_suburban_hertford_branch_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "GN-4",
            "Signal failure at Winchmore Hill",
            "Signal failure causing delays to GN train services.",
            &["GN"],
            &["WIH"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["great-northern-suburban".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.11. Same for the Welwyn Garden City branch's own local-only
    // stops (the other branch past Alexandra Palace) - an incident at
    // Oakleigh Park, a station `great-northern-kings-lynn.toml`'s semi-fast
    // service never calls at, should also stay exclusive to this line, and
    // shouldn't spuriously pull in the Hertford Loop branch's own segment
    // name either (the two branches use different segment names despite
    // being the same file/line).
    #[test]
    fn gn_suburban_wgc_branch_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "GN-5",
            "Points failure at Oakleigh Park",
            "Points failure causing delays to GN train services.",
            &["GN"],
            &["OKL"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["great-northern-suburban".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.11. Finsbury Park (FPK) is also `great-northern-kings-lynn.toml`'s
    // own `gn-ecml-slow-lines` station - per this file's own SHARED-TRUNK
    // RESEARCH FINDING header comment, this is a genuine physical overlap
    // (both services' trains run over the same East Coast Main Line slow
    // lines here) but with different calling patterns, so it's deliberately
    // kept as station overlap only, not a shared segment name - same
    // judgment call as the AFK/Ramsgate/LBG overlaps between Southeastern
    // and HS1 exercised above. Confirms an incident at Finsbury Park matches
    // both lines independently, each still scoped ExclusiveSegment, never
    // SharedSegment.
    //
    // Updated by Task 5.13 (thameslink-cambridge.toml): that file also has a
    // station at FPK (its own `thameslink-cambridge-peterborough-trunk`
    // segment - the Canal Tunnels' connection to the ECML, physically the
    // FAST lines here rather than `gn-ecml-slow-lines`, per that file's own
    // OVERLAP (a) header comment), so it now joins this set as a third
    // independent exclusive-segment match.
    #[test]
    fn fpk_station_overlap_matches_great_northern_suburban_and_kings_lynn_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "GN-6",
            "Signal failure at Finsbury Park",
            "Signal failure causing delays to GN train services.",
            &["GN"],
            &["FPK"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "great-northern-suburban".to_string(),
                "great-northern-kings-lynn".to_string(),
                "thameslink-cambridge".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Task 5.12 (thameslink-bedford.toml). This file's `mml-bedford-
    // st-pancras` segment (Kentish Town through Bedford) is named and
    // documented, per this file's own header comment, as the segment
    // Batch 7's EMR Midland Main Line file is required to cite and reuse
    // verbatim for its own Bedford-St Pancras section - but that sibling
    // file doesn't exist in this worktree yet, so no SharedSegment test for
    // that pairing is added here (would fail to compile/pass without the
    // sibling). Until that file exists and reuses the name, this segment is
    // exclusive to this line alone: an incident at Harpenden, a station on
    // that segment untouched by any other curated line, should stay
    // exclusive to this line - mirrors
    // swr_exclusive_segment_incident_does_not_propagate and
    // elizabeth_branch_incident_stays_on_its_branch above.
    #[test]
    fn thameslink_bedford_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "TL-1",
            "Signal failure at Harpenden",
            "Signal failure causing delays to train services.",
            &["TL"],
            &["HPD"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["thameslink-bedford".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 5.12. St Pancras International (STP) is where this file's
    // northern branch meets thameslink-core.toml's own terminus, and is
    // also southeastern-highspeed.toml's own terminus (`hs1-domestic`).
    // Per this file's own SEGMENT NAMING header comment, this is
    // deliberately kept as station overlap only in every direction: this
    // file's own `mml-bedford-st-pancras` segment name is NOT reused by
    // thameslink-core (whose `thameslink-core` segment also covers
    // Farringdon/City Thameslink/Blackfriars/London Bridge, none of which
    // this branch file touches - reusing the name verbatim would
    // incorrectly mark those untouched stations SharedSegment too, exactly
    // the trap southeastern-highspeed.toml's own header comment already
    // flags for `seml-coast`/`chatham-medway`). Confirms an incident at STP
    // matches all three lines independently, each still scoped
    // ExclusiveSegment, never SharedSegment - mirrors
    // afk_station_overlap_matches_both_seml_and_hs1_as_independent_exclusive_segments
    // above.
    //
    // Updated by Task 5.13 (thameslink-cambridge.toml): that file's own
    // Cambridge/Peterborough branch also meets the core at STP (its own
    // `thameslink-cambridge-peterborough-trunk` segment, diverging into the
    // Canal Tunnels towards the ECML rather than north up the Midland Main
    // Line like thameslink-bedford - see that file's own STP header
    // comment), so it now joins this set as a fourth independent
    // exclusive-segment match.
    #[test]
    fn stp_station_overlap_matches_thameslink_core_bedford_and_highspeed_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "TL-2",
            "Signal failure at St Pancras International",
            "Signal failure causing delays to train services.",
            &["TL"],
            &["STP"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "thameslink-core".to_string(),
                "thameslink-bedford".to_string(),
                "southeastern-highspeed".to_string(),
                "thameslink-cambridge".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }

    // Task 5.13 (thameslink-cambridge.toml). Per that file's own OVERLAP (c)
    // header comment: the direct Peterborough branch (Arlesey, Biggleswade,
    // Sandy, St Neots, Huntingdon, Peterborough) is a genuine shared trunk
    // with great-northern-kings-lynn.toml's own `gn-peterborough-branch`
    // segment - Stevenage station's own Wikipedia services section confirms
    // this line runs "2 tph to Peterborough (all stations)", an identical
    // calling pattern to that file's own complete station list for the
    // branch (unlike the Cambridge Line branch, where this line's semi-fast
    // pattern skips Meldreth/Shepreth/Foxton - see
    // bdk_station_overlap_matches_great_northern_kings_lynn_and_thameslink_cambridge_as_independent_exclusive_segments
    // below). This replaces the previous exclusive-segment expectation for
    // Huntingdon (now genuinely shared) - mirrors
    // swr_shared_trunk_incident_propagates above.
    #[test]
    fn gn_peterborough_branch_shared_trunk_incident_propagates_to_thameslink_cambridge() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "GN-1",
            "Signal failure at Huntingdon",
            "Signal failure causing delays to GN train services.",
            &["GN"],
            &["HUN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["great-northern-kings-lynn".to_string(), "thameslink-cambridge".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    // Task 5.13. Baldock (BDK) is on this line's own Cambridge Line branch
    // (`thameslink-cambridge-branch`) and also
    // great-northern-kings-lynn.toml's own `gn-cambridge-kings-lynn-branch`
    // station - per that file's own OVERLAP (d) header comment, this is
    // station overlap only, not a shared trunk, because this line's
    // semi-fast Cambridge service skips Meldreth/Shepreth/Foxton that GN's
    // own segment treats as consecutive stops (the same "same track,
    // different calling pattern" situation as the FPK/Stevenage overlaps
    // above). Confirms an incident at Baldock matches both lines
    // independently, each still scoped ExclusiveSegment, never
    // SharedSegment - mirrors
    // cbg_station_overlap_matches_great_northern_kings_lynn_and_xc_stansted_as_independent_exclusive_segments
    // above.
    #[test]
    fn bdk_station_overlap_matches_great_northern_kings_lynn_and_thameslink_cambridge_as_independent_exclusive_segments() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "TL-3",
            "Points failure at Baldock",
            "Points failure causing delays to train services.",
            &["TL"],
            &["BDK"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["great-northern-kings-lynn".to_string(), "thameslink-cambridge".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment, not shared", m.line.id);
        }
    }
}
