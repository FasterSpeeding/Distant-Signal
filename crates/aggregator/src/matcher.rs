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

    // `tfw-cambrian`'s own trunk segment (`tfw-cambrian-trunk`) is still
    // unique to Cambrian -- no other file shares that exact segment name,
    // so an incident away from Shrewsbury only ever matches Cambrian as
    // `MatchScope::ExclusiveSegment`, which is what this test covers.
    // Shrewsbury itself, however, is now a genuine three-way overlap point:
    // `tfw-marches.toml` (Task 11.5) and `tfw-heart-of-wales.toml` both tag
    // SHR with the shared `tfw-heart-of-wales-shrewsbury` segment, while
    // Cambrian keeps SHR on its own exclusive `tfw-cambrian-trunk` segment
    // (the Cambrian Line diverges west immediately, still station-overlap
    // only there -- see `lines/tfw-cambrian.toml`'s own comment). So an
    // incident specifically at Shrewsbury resolves three ways at once: see
    // `shrewsbury_three_way_overlap_resolves_per_line` below for that case.
    #[test]
    fn cambrian_coast_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-1",
            "Signal failure at Barmouth",
            "Signal failure causing delays to Transport for Wales services.",
            &["AW"],
            &["BRM"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["tfw-cambrian".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn cambrian_aberystwyth_branch_incident_does_not_propagate_to_coast_branch() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-2",
            "Trespass incident at Aberystwyth",
            "Trespass incident causing delays to Transport for Wales services.",
            &["AW"],
            &["AYW"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["tfw-cambrian".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // `tfw-heart-of-wales` is standalone on its own southern stretch
    // (Craven Arms southwards) -- `tfw-marches.toml` (Task 11.5) now exists
    // but only shares the Shrewsbury-Craven Arms stretch, not the branch
    // south of Craven Arms towards Llandrindod/Swansea, so an incident well
    // south of Craven Arms should still match only Heart of Wales, as
    // `MatchScope::ExclusiveSegment`. See
    // `heart_of_wales_shrewsbury_shared_trunk_propagates` below for the
    // shared-trunk case Task 11.5 decided on.
    #[test]
    fn heart_of_wales_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-3",
            "Signal failure at Llandrindod",
            "Signal failure causing delays to Transport for Wales services.",
            &["AW"],
            &["LLO"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["tfw-heart-of-wales".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // `tfw-conwy-valley` is, at this point in the line catalogue, a
    // genuinely standalone line: it uses a single whole-line segment name
    // and no other file in the catalogue shares it yet.
    // `tfw-north-wales-coast.toml` (Task 11.4) now exists and also reaches
    // Llandudno Junction, but Task 11.4 deliberately ruled that overlap
    // "station-overlap-only" rather than a genuine shared trunk (the Conwy
    // Valley Line diverges south immediately at the junction, with no track
    // actually shared by both lines' services beyond that one calling
    // point) -- so the two files use distinct segment names there and this
    // remains an exclusive-segment case for Conwy Valley. See
    // `lines/tfw-north-wales-coast.toml`'s comments for the full reasoning,
    // and `llj_station_overlap_matches_both_lines_as_exclusive` below for
    // the assertion that exercises the overlap itself.
    #[test]
    fn conwy_valley_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-4",
            "Signal failure at Betws-y-Coed",
            "Signal failure causing delays to Transport for Wales services.",
            &["AW"],
            &["BYC"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["tfw-conwy-valley".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // `tfw-north-wales-coast` (Task 11.4). An incident on a station well
    // away from the Llandudno Junction overlap should match only this line,
    // as `MatchScope::ExclusiveSegment` -- e.g. Rhyl.
    #[test]
    fn north_wales_coast_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "VT-1",
            "Signal failure at Rhyl",
            "Signal failure causing delays to services on the North Wales Coast Line.",
            &["AW"],
            &["RHL"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["tfw-north-wales-coast".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Llandudno Junction is on both `tfw-conwy-valley` and
    // `tfw-north-wales-coast`, but Task 11.4 ruled that overlap
    // station-overlap-only rather than a shared trunk (see the comment
    // above `conwy_valley_exclusive_segment_incident_does_not_propagate`
    // and the comments in `lines/tfw-north-wales-coast.toml`). So an
    // incident there should match both lines independently, each still
    // classified as `MatchScope::ExclusiveSegment` (not `SharedSegment` --
    // that scope only applies when a segment name is genuinely shared
    // across line files, which is deliberately not the case here).
    #[test]
    fn llj_station_overlap_matches_both_lines_as_exclusive() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-5",
            "Points failure at Llandudno Junction",
            "Points failure causing delays to Transport for Wales services.",
            &["AW"],
            &["LLJ"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["tfw-conwy-valley".to_string(), "tfw-north-wales-coast".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment", m.line.id);
        }
    }

    // `tfw-marches` (Task 11.5). An incident on a station well away from
    // both coordination points (the Shrewsbury-Craven Arms shared trunk
    // with Heart of Wales, and the Chester station-overlap with North
    // Wales Coast) should match only this line, as
    // `MatchScope::ExclusiveSegment` -- e.g. Hereford, which sits on
    // `tfw-marches-south`, a segment no other line in the catalogue uses.
    #[test]
    fn marches_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-6",
            "Signal failure at Hereford",
            "Signal failure causing delays to Transport for Wales services.",
            &["AW"],
            &["HFD"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["tfw-marches".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Craven Arms is on both `tfw-marches` and `tfw-heart-of-wales`, and
    // Task 11.5 ruled -- after independently verifying the physical
    // track-sharing claim against three separate Wikipedia articles (see
    // the comments above Craven Arms in `lines/tfw-marches.toml`) -- that
    // this is a genuine shared trunk, not mere station overlap: Heart of
    // Wales services physically run over Marches Line metals between
    // Craven Arms and Shrewsbury, calling at the intermediate station
    // Church Stretton along the way. Both files tag Craven Arms (and
    // Church Stretton, and Shrewsbury) with the same segment name,
    // `tfw-heart-of-wales-shrewsbury` (reusing the forward-bet name
    // `tfw-heart-of-wales.toml` left for this file to pick up), so an
    // incident there should propagate to both lines, each classified
    // `MatchScope::SharedSegment`.
    #[test]
    fn heart_of_wales_shrewsbury_shared_trunk_propagates() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-7",
            "Points failure at Craven Arms",
            "Points failure causing delays to Transport for Wales services.",
            &["AW"],
            &["CRV"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["tfw-marches".to_string(), "tfw-heart-of-wales".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    // Chester is on both `tfw-marches` and `tfw-north-wales-coast`, but
    // Task 11.5 ruled that overlap station-overlap-only rather than a
    // shared trunk, mirroring `tfw-north-wales-coast.toml`'s own Llandudno
    // Junction decision against `tfw-conwy-valley.toml`: despite genuine
    // physical track sharing existing at Saltney Junction on the final
    // approach into Chester (see the comment above Chester in
    // `lines/tfw-marches.toml`), `tfw-north-wales-coast.toml` uses one
    // single whole-line segment name for its entire route, so reusing it
    // here would incorrectly mark that line's whole route (Rhyl, Bangor,
    // Holyhead, etc.) as shared with Marches. So the two files use distinct
    // segment names at Chester, and an incident there should match both
    // lines independently, each still classified as
    // `MatchScope::ExclusiveSegment` (not `SharedSegment`).
    #[test]
    fn chester_station_overlap_matches_both_lines_as_exclusive() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-8",
            "Points failure at Chester",
            "Points failure causing delays to Transport for Wales services.",
            &["AW"],
            &["CTR"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["tfw-marches".to_string(), "tfw-north-wales-coast".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment", m.line.id);
        }
    }

    // Shrewsbury is a genuine three-way overlap point, introduced by Task
    // 11.5: `tfw-cambrian.toml` tags SHR with its own exclusive
    // `tfw-cambrian-trunk` segment (the Cambrian Line diverges west
    // immediately -- station-overlap only there, per that file's own
    // comment), while `tfw-marches.toml` and `tfw-heart-of-wales.toml` both
    // tag SHR with the shared `tfw-heart-of-wales-shrewsbury` segment (see
    // the comment above Craven Arms in `lines/tfw-marches.toml` for the
    // shared-trunk sourcing). So a single incident at SHR should resolve
    // differently per line, all at once: Cambrian stays
    // `MatchScope::ExclusiveSegment`, while Marches and Heart of Wales are
    // both `MatchScope::SharedSegment`.
    #[test]
    fn shrewsbury_three_way_overlap_resolves_per_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident(
            "AW-9",
            "Signal failure at Shrewsbury",
            "Signal failure causing delays to Transport for Wales services.",
            &["AW"],
            &["SHR"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let scopes: HashMap<String, MatchScope> =
            matches.iter().map(|m| (m.line.id.clone(), m.scope)).collect();
        assert_eq!(
            scopes.keys().cloned().collect::<HashSet<String>>(),
            HashSet::from(["tfw-cambrian".to_string(), "tfw-marches".to_string(), "tfw-heart-of-wales".to_string()])
        );
        assert_eq!(scopes["tfw-cambrian"], MatchScope::ExclusiveSegment);
        assert_eq!(scopes["tfw-marches"], MatchScope::SharedSegment);
        assert_eq!(scopes["tfw-heart-of-wales"], MatchScope::SharedSegment);
    }
}
