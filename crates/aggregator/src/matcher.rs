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
    fn heathrow_express_incident_stays_exclusive_of_elizabeth_heathrow() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Task 6.8: Heathrow Terminal 5 (HWV) is a real station on BOTH
        // `heathrow-express` (its own exclusive `hx-paddington-heathrow`
        // segment) and `elizabeth-heathrow` (its own, differently-named
        // `elizabeth-heathrow` segment) -- the two lines genuinely serve the
        // same terminal buildings. Without `elizabeth-heathrow.toml`'s
        // pre-existing `excluded_keywords = ["Heathrow Express", ...]` veto,
        // an incident like this one (which names Heathrow Express in its
        // text and hits a station both lines share) would station-hit-match
        // `elizabeth-heathrow` too. This is the required re-check from the
        // task brief, made concrete: with `heathrow-express.toml` now a
        // real, matchable line, the pre-existing exclusion on
        // `elizabeth-heathrow.toml` is confirmed still necessary and is left
        // unchanged (see `heathrow-express.toml`'s own file-level comment
        // for the full reasoning) -- this test is the evidence for that
        // conclusion, not just an assertion of it.
        let inc = incident(
            "HX-1",
            "Points failure at Heathrow Terminal 5",
            "A points failure is causing delays to Heathrow Express services near Heathrow Terminal 5.",
            &["HX"],
            &["HWV"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["heathrow-express".to_string()]),
            "elizabeth-heathrow should be vetoed by its own excluded_keywords, not station-match HWV"
        );
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

    #[test]
    fn lner_ecml_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Aberdeen sits on `ecml-aberdeen`, north of the Doncaster/Newark
        // junctions that Tasks 6.2-6.4's not-yet-written Leeds/Hull/Lincoln
        // branches will share `ecml-doncaster`/`ecml-fenland` with — no
        // other line touches `ecml-aberdeen` today, so this should stay
        // exclusive to `lner-ecml` and not propagate anywhere else.
        let inc = incident("LNER-1", "Points failure at Aberdeen", "Points failure causing delays at Aberdeen.", &["GR"], &["ABD"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["lner-ecml".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn lner_leeds_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Harrogate sits on `lner-leeds-harrogate`, exclusive to this file
        // (LNER's Skipton working diverges at Leeds onto a different physical
        // line and isn't modeled as stations — see the file's comments).
        let inc = incident(
            "LNER-2",
            "Signal failure at Harrogate",
            "Signal failure causing delays to services at Harrogate.",
            &["GR"],
            &["HGT"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["lner-leeds".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn lner_leeds_doncaster_shared_trunk_propagates_to_ecml() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Doncaster is `ecml-doncaster`, shared between `lner-ecml` and
        // `lner-leeds` (both run over the same ECML trunk to Doncaster
        // before the Leeds branch peels off onto the Wakefield Line).
        // `cross-country.toml` also has a station at Doncaster, but on its
        // own exclusive `xc-yorkshire` segment, so it's not asserted here.
        let inc = incident(
            "LNER-3",
            "Points failure at Doncaster",
            "Points failure causing disruption to services through Doncaster.",
            &["GR"],
            &["DON"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("lner-ecml"));
        assert!(matched_ids.contains("lner-leeds"));
        for m in &matches {
            if m.line.id.starts_with("lner-") {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    #[test]
    fn lner_hull_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Selby sits on `lner-hull`, exclusive to this file — the real
        // physical divergence from the ECML is Temple Hirst Junction
        // (north of Doncaster, no CRS code), but the shared trunk still
        // ends at Doncaster per `lner-ecml.toml`'s own instruction (see
        // that file's DON entry and this file's comments), so Selby is
        // this branch's first exclusive station.
        //
        // Since Task 6.6 added `hull-trains.toml` (a different operator,
        // `HT`, that also genuinely calls at Selby en route to Hull
        // Paragon — see that file's own research comments), Selby now also
        // matches `hull-trains` by station. This mirrors
        // `lner_leeds_station_overlap_at_leeds_does_not_share_northern_segment`'s
        // precedent: both lines match, but each stays `ExclusiveSegment`
        // because neither's own segment name (`lner-hull` vs.
        // `ht-kings-cross-hull`) is literally shared with the other — this
        // is real station-level overlap between two different operators'
        // files, not a shared-trunk relationship the matcher recognizes by
        // segment name.
        let inc = incident(
            "LNER-4",
            "Signal failure at Selby",
            "Signal failure causing delays to services at Selby.",
            &["GR"],
            &["SBY"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["lner-hull".to_string(), "hull-trains".to_string()]));
        for m in &matches {
            assert_eq!(m.scope, MatchScope::ExclusiveSegment, "{} should be ExclusiveSegment", m.line.id);
        }
    }

    #[test]
    fn lner_hull_doncaster_shared_trunk_propagates_to_ecml() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Doncaster is `ecml-doncaster`, shared between `lner-ecml` and
        // `lner-hull` (both run over the same ECML trunk to Doncaster
        // before the Hull branch peels off toward Selby/Brough). Mirrors
        // `lner_leeds_doncaster_shared_trunk_propagates_to_ecml` above;
        // `lner-leeds` also shares this segment, so it's expected to show
        // up here too, but only `lner-ecml`/`lner-hull` are asserted since
        // that's what this test is about.
        let inc = incident(
            "LNER-5",
            "Points failure at Doncaster",
            "Points failure causing disruption to services through Doncaster.",
            &["GR"],
            &["DON"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("lner-ecml"));
        assert!(matched_ids.contains("lner-hull"));
        for m in &matches {
            if m.line.id.starts_with("lner-") {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    #[test]
    fn lner_lincoln_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Lincoln sits on `lner-lincoln`, exclusive to this file — the real
        // physical divergence from the ECML is the Newark flat crossing,
        // just north of Newark Northgate (no CRS code), but the shared
        // trunk still ends at Newark Northgate per `lner-ecml.toml`'s own
        // instruction (see that file's NNG entry and this file's
        // comments), so Lincoln is this branch's first exclusive station.
        let inc = incident(
            "LNER-6",
            "Signal failure at Lincoln",
            "Signal failure causing delays to services at Lincoln.",
            &["GR"],
            &["LCN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["lner-lincoln".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn lner_lincoln_newark_northgate_shared_trunk_propagates_to_ecml() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Newark Northgate is `ecml-fenland`, shared between `lner-ecml`
        // and `lner-lincoln` (both run over the same ECML trunk to Newark
        // Northgate before the Lincoln branch peels off onto the
        // Nottingham-Lincoln line at the Newark flat crossing).
        let inc = incident(
            "LNER-7",
            "Points failure at Newark Northgate",
            "Points failure causing disruption to services through Newark Northgate.",
            &["GR"],
            &["NNG"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("lner-ecml"));
        assert!(matched_ids.contains("lner-lincoln"));
        for m in &matches {
            if m.line.id.starts_with("lner-") {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    #[test]
    fn lner_leeds_station_overlap_at_leeds_does_not_share_northern_segment() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Leeds (LDS) is a station on all three of `lner-leeds`, `northern`
        // and `northern-yorkshire-coast` — but research found no LNER
        // service running the physical Leeds<->York trunk that Northern's
        // `northern-yorkshire` segment represents (see the file comment on
        // `lner-leeds.toml`'s LDS entry), so `lner-leeds` deliberately does
        // NOT reuse that segment name. This mirrors `xc-south-coast.toml`'s
        // "station overlap is fine, segment-sharing is a deliberate choice"
        // precedent: all three lines match this incident by station, but
        // `lner-leeds` stays ExclusiveSegment (its own `lner-leeds` segment
        // isn't shared with anyone) while Northern's two lines are
        // SharedSegment between themselves via `northern-yorkshire`.
        let inc = incident(
            "LNER-4",
            "Overhead line damage at Leeds",
            "Overhead line damage causing disruption to services at Leeds.",
            &["GR"],
            &["LDS"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["lner-leeds".to_string(), "northern".to_string(), "northern-yorkshire-coast".to_string()])
        );
        for m in &matches {
            let expected = if m.line.id == "lner-leeds" { MatchScope::ExclusiveSegment } else { MatchScope::SharedSegment };
            assert_eq!(m.scope, expected, "{} scope mismatch", m.line.id);
        }
    }

    #[test]
    fn grand_central_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Sunderland sits on `gc-sunderland`, exclusive to `grand-central` —
        // no other line in this catalogue reaches Sunderland, so this should
        // stay exclusive and not propagate anywhere else. Per the task
        // brief, no shared-trunk test against `lner-ecml.toml` (or any other
        // LNER file) is required for Grand Central: the plan is explicit
        // that Grand Central's relationship to LNER is station-overlap-only
        // (shared at King's Cross/Peterborough/Doncaster/York, none of which
        // this test touches), not a forced shared segment.
        let inc = incident(
            "GC-1",
            "Signal failure at Sunderland",
            "Signal failure causing delays to services at Sunderland.",
            &["GC"],
            &["SUN"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["grand-central".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn grand_central_kings_cross_trunk_shared_trunk_propagates_to_bradford() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Final review, Fix #1: `grand-central.toml` used to model BOTH the
        // King's Cross-Sunderland and King's Cross-Bradford Interchange
        // routes as one non-linear file, with a `gc-trunk-kings-cross`
        // segment that could never be a real shared trunk (only one *line*
        // used the name). Split into `grand-central.toml` (Sunderland) and
        // `grand-central-bradford.toml` (Bradford Interchange), which now
        // genuinely share `gc-trunk-kings-cross` across King's Cross,
        // Peterborough and Doncaster — mirroring
        // `swr_shared_trunk_incident_propagates` above. An incident at
        // Doncaster should now match both files, each SharedSegment.
        let inc = incident(
            "GC-2",
            "Points failure at Doncaster",
            "Points failure causing disruption to services through Doncaster.",
            &["GC"],
            &["DON"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("grand-central"));
        assert!(matched_ids.contains("grand-central-bradford"));
        for m in &matches {
            if m.line.id.starts_with("grand-central") {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    #[test]
    fn grand_central_bradford_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Halifax sits on `gc-bradford`, exclusive to `grand-central-bradford`
        // — no other line in this catalogue reaches Halifax on Grand
        // Central's own segment naming, so this should stay exclusive and
        // not propagate anywhere else. Mirrors
        // `grand_central_exclusive_segment_incident_does_not_propagate`
        // above (the Sunderland-side equivalent), added for the new
        // Bradford file per the final review, Fix #1.
        let inc = incident(
            "GC-3",
            "Signal failure at Halifax",
            "Signal failure causing delays to services at Halifax.",
            &["GC"],
            &["HFX"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["grand-central-bradford".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn grand_central_birmingham_shopping_centre_mention_vetoes_match() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Final review, Fix #3: `excluded_keywords` was narrowed from a bare
        // "Birmingham" to "Grand Central, Birmingham" (the shopping
        // centre's own name+city, per Wikipedia's "Grand Central,
        // Birmingham" article), so it should still veto an incident that
        // genuinely mentions the shopping centre. Mirrors
        // `excluded_keyword_vetoes_match`'s style above.
        let inc = incident(
            "GC-4",
            "Fire alarm at Grand Central, Birmingham",
            "A fire alarm was activated at the Grand Central, Birmingham shopping centre, next to New Street station.",
            &[],
            &[],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(!matched_ids.contains("grand-central"), "shopping-centre mention should still veto grand-central");
        assert!(!matched_ids.contains("grand-central-bradford"), "shopping-centre mention should still veto grand-central-bradford");
    }

    #[test]
    fn grand_central_unrelated_birmingham_mention_does_not_veto_match() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Final review, Fix #3: the old bare "Birmingham" exclusion would
        // have wrongly vetoed a genuine Grand Central incident that happens
        // to mention Birmingham for an unrelated reason (e.g. a diversion
        // routed via Birmingham). The narrowed "Grand Central, Birmingham"
        // phrase should NOT fire here, so the incident matches via the
        // `match_keywords` "Grand Central" phrase instead.
        let inc = incident(
            "GC-5",
            "Grand Central service diverted",
            "A Grand Central service was diverted via Birmingham due to engineering works.",
            &[],
            &[],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("grand-central"), "unrelated Birmingham mention should not veto grand-central");
        assert!(matched_ids.contains("grand-central-bradford"), "unrelated Birmingham mention should not veto grand-central-bradford");
        for m in &matches {
            if m.line.id.starts_with("grand-central") {
                assert_eq!(m.scope, MatchScope::KeywordOnly, "{} should match via keyword only", m.line.id);
            }
        }
    }

    #[test]
    fn hull_trains_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Howden sits on `ht-kings-cross-hull`, exclusive to `hull-trains` —
        // no other line in this catalogue has a station at Howden, so this
        // should stay exclusive and not propagate anywhere else. Per the
        // task brief (the same standalone-operator exception
        // `grand-central.toml` already established for its own relationship
        // to LNER), no shared-trunk test against any `lner-*.toml` file is
        // required for Hull Trains: `hull-trains.toml`'s station-level
        // overlap with `lner-hull.toml` (Stevenage, Grantham, Retford,
        // Doncaster, Selby, Brough, Hull Paragon) is deliberate and
        // documented, not a forced shared segment.
        let inc = incident(
            "HT-1",
            "Signal failure at Howden",
            "Signal failure causing delays to services at Howden.",
            &["HT"],
            &["HOW"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["hull-trains".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    #[test]
    fn lumo_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        // Falkirk High sits on `lumo-glasgow`, exclusive to `lumo` — no
        // other line in this catalogue has a station at Falkirk High, so
        // this should stay exclusive and not propagate anywhere else. Per
        // the task brief (the same standalone-operator exception
        // `grand-central.toml` and `hull-trains.toml` already established
        // for their own relationship to LNER), no shared-trunk test against
        // any `lner-*.toml` file is required for Lumo: `lumo.toml`'s
        // station-level overlap with `lner-ecml.toml` (King's Cross,
        // Stevenage, Newcastle, Morpeth, Edinburgh Waverley, Haymarket) is
        // deliberate and documented, not a forced shared segment. None of
        // those shared stations are used here, so no pre-existing test
        // needed updating for this task (unlike Task 6.6's Selby situation
        // with `lner-hull.toml`) — checked: no other test in this file
        // references King's Cross, Stevenage, Newcastle, Morpeth, Edinburgh
        // Waverley or Haymarket.
        let inc = incident(
            "LD-1",
            "Signal failure at Falkirk High",
            "Signal failure causing delays to services at Falkirk High.",
            &["LD"],
            &["FKK"],
        );
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["lumo".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }
}
