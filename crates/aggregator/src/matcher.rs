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

    // `gwr-main-line`'s Bristol-bound exclusive segment starts at Chippenham
    // (CPM), the first station beyond Swindon reached only by GWML-via-Bath
    // services (South Wales Main Line diverges at Wootton Bassett Junction,
    // just west of Swindon), mirroring
    // `swr_exclusive_segment_incident_does_not_propagate`. See
    // `gwr_trunk_paddington_incident_propagates_to_cotswold` below for the
    // shared-trunk case, now that `gwr-cotswold` (Task 4.2) also shares
    // `gwr-trunk-paddington`.
    #[test]
    fn gwr_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-1", "Points failure at Chippenham", "Points failure causing delays at Chippenham.", &["GW"], &["CPM"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-main-line".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // `gwr-cotswold`'s (Task 4.2) own exclusive segment starts at Oxford
    // (OXF), the first station beyond Didcot reached only by Cotswold Line
    // services (South Wales/Bristol-bound gwr-main-line services continue
    // west towards Swindon at Didcot instead). Mirrors
    // `swr_exclusive_segment_incident_does_not_propagate` /
    // `gwr_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn gwr_cotswold_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-2", "Signal failure at Moreton-in-Marsh", "Signal failure causing delays at Moreton-in-Marsh.", &["GW"], &["MIM"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-cotswold".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // `gwr-main-line` and `gwr-cotswold` both share the `gwr-trunk-paddington`
    // segment (PAD/RDG/DID) established by Task 4.1 and reused verbatim by
    // Task 4.2. An incident at Didcot (a station on that shared segment)
    // should propagate to both lines as a shared-trunk event. Mirrors
    // `swr_shared_trunk_incident_propagates`'s shape.
    //
    // Task 4.5 (`gwr-thames-valley`) also has a DID station, but on its own
    // `gwr-thames-valley` segment (deliberately not sharing
    // `gwr-trunk-paddington` — see that file's own segment-naming comment),
    // so it's excluded from the SharedSegment check below even though its ID
    // also starts with "gwr-".
    #[test]
    fn gwr_trunk_paddington_incident_propagates_to_cotswold() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-3", "Signal failure at Didcot Parkway", "Signal failure causing delays to GWR services.", &["GW"], &["DID"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert!(matched_ids.contains("gwr-main-line"));
        assert!(matched_ids.contains("gwr-cotswold"));
        for m in &matches {
            if m.line.id.starts_with("gwr-") && m.line.id != "gwr-thames-valley" {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    // `gwr-south-wales`'s (Task 4.3) own exclusive segment covers Bristol
    // Parkway through Swansea. Bridgend (BGN) is not shared with any other
    // line's own station list in this catalogue, so this is a clean
    // ExclusiveSegment case, mirroring
    // `swr_exclusive_segment_incident_does_not_propagate` /
    // `gwr_cotswold_exclusive_segment_incident_does_not_propagate`. See
    // `gwr_south_wales_station_overlap_with_xc_cardiff_stays_exclusive_each_line`
    // below for the deliberately-not-shared overlap case at Newport/Cardiff
    // (task-4.3-brief.md's plan-mandated "don't force a shared segment"
    // decision), and `gwr_trunk_paddington_incident_propagates_to_south_wales`
    // for the shared-trunk case.
    #[test]
    fn gwr_south_wales_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-4", "Overhead line damage at Bridgend", "Overhead line damage causing delays at Bridgend.", &["GW"], &["BGN"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-south-wales".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // `gwr-main-line`, `gwr-cotswold` and `gwr-south-wales` all share the
    // `gwr-trunk-paddington` segment, but only at the stations each file
    // actually lists on it: gwr-cotswold.toml stops at DID (Cotswold
    // services diverge onto the Cherwell Valley line there, before Swindon),
    // while gwr-main-line.toml and gwr-south-wales.toml both continue
    // through SWI. Didcot Parkway (DID) is therefore the one station all
    // three files genuinely share, so an incident there should propagate to
    // all three as a shared-trunk event. Mirrors
    // `swr_shared_trunk_incident_propagates`'s full-set-assertion shape, now
    // extended to a third sibling per task-4.3-brief.md's test requirement
    // #2 (that requirement names Swindon as the example station, but Swindon
    // is not actually on gwr-cotswold.toml's own station list — see that
    // file's own segment-naming comment — so Didcot is used here instead to
    // get a real three-way match rather than a two-way one).
    //
    // Task 4.5 (`gwr-thames-valley`) also stops at DID, but on its own
    // exclusive `gwr-thames-valley` segment (deliberately not sharing
    // `gwr-trunk-paddington` — see that file's own segment-naming comment),
    // so it's included in the matched set (a real station overlap) but stays
    // ExclusiveSegment rather than SharedSegment.
    #[test]
    fn gwr_trunk_paddington_incident_propagates_to_south_wales() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-5", "Signal failure at Didcot Parkway", "Signal failure causing delays to GWR services.", &["GW"], &["DID"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "gwr-main-line".to_string(),
                "gwr-cotswold".to_string(),
                "gwr-south-wales".to_string(),
                "gwr-thames-valley".to_string(),
            ])
        );
        for m in &matches {
            if m.line.id == "gwr-thames-valley" {
                assert_eq!(m.scope, MatchScope::ExclusiveSegment, "gwr-thames-valley should stay ExclusiveSegment");
            } else {
                assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
            }
        }
    }

    // task-4.3-brief.md's plan-mandated regression guard: Cardiff Central is
    // a real station overlap between `gwr-south-wales` (this task) and
    // `xc-cardiff.toml` (already committed) — both lines call there, but via
    // physically different corridors for most of their length (South Wales
    // Main Line via Bristol Parkway/Severn Tunnel vs. xc-cardiff's Gloucester
    // to Newport Line via Chepstow), and per this task's file-scope
    // restriction (lines/gwr-south-wales.toml + matcher.rs only, not
    // xc-cardiff.toml) neither file reuses the other's segment name for
    // NWP/CDF. So an incident at Cardiff Central should match BOTH lines
    // (real station overlap — each notified about an incident at "their"
    // station) but EACH must stay `MatchScope::ExclusiveSegment` for its own
    // segment, never `SharedSegment` — confirming the two lines' overlap
    // here stays a station-level thing, not a segment-level one. See
    // gwr-south-wales.toml's own segment-naming comment for the research
    // this decision is based on (a genuine, not just assumed, finding that
    // physical track sharing exists further up the corridor at Severn Tunnel
    // Junction, deliberately not modelled as a shared segment given this
    // task's file-scope limits).
    #[test]
    fn gwr_south_wales_station_overlap_with_xc_cardiff_stays_exclusive_each_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-6", "Points failure at Cardiff Central", "Points failure causing delays at Cardiff Central.", &["GW"], &["CDF"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-south-wales".to_string(), "xc-cardiff".to_string()]));
        for m in &matches {
            assert_eq!(
                m.scope,
                MatchScope::ExclusiveSegment,
                "{} should stay ExclusiveSegment (station overlap, not a shared segment)",
                m.line.id
            );
        }
    }

    // Task 4.4 split `gwr-west-of-england` (Reading-Taunton line) into its own
    // file. Originally its exclusive segment (`gwr-west-of-england`) covered
    // Newbury through Castle Cary with no *cross-file segment-name* sharing
    // at all. Task 4.6 (gwr-bristol-suburban.toml) found genuine physical
    // track sharing at Westbury/Castle Cary, but an early draft of that fix
    // reused the whole `gwr-west-of-england` segment name (including Newbury,
    // which gwr-bristol-suburban's own service never reaches, and Frome/
    // Bruton, which are gwr-bristol-suburban's own exclusive territory) —
    // wrong, since segment sharing is tracked per segment *name*, not per
    // individual station, so that draft mislabelled all three as "shared"
    // catalogue-wide. The final-review fix wave introduced a new, narrower
    // segment name, `gwr-westbury-castle-cary`, covering ONLY Westbury (WSB)
    // and Castle Cary (CLC) — the two stations both files' own cited sources
    // actually name as shared. Newbury (NBY) reverts to being a genuinely
    // exclusive station on this line's own `gwr-west-of-england` segment
    // (gwr-bristol-suburban.toml never reaches it), and Frome/Bruton move
    // onto gwr-bristol-suburban.toml's own `gwr-bristol-weymouth` segment.
    // See `gwr_westbury_castle_cary_trunk_incident_propagates_to_bristol_
    // suburban` below for the corrected shared-segment case, and
    // `gwr_thames_valley_station_overlap_with_gwr_west_of_england_stays_
    // exclusive_each_line` below for the Newbury case, now back to
    // ExclusiveSegment on both sides (a real station overlap, not a segment
    // share).

    // Task 4.4's second file, `gwr-cornish-main-line`, picks up its own
    // exclusive segment (also named `gwr-cornish-main-line`) at Liskeard,
    // the first station west of Plymouth not already claimed by
    // cross-country.toml's `xc-south-west` segment. Truro is deep inside
    // that exclusive stretch, so this should stay a clean ExclusiveSegment
    // case, mirroring `swr_exclusive_segment_incident_does_not_propagate` /
    // `gwr_cotswold_exclusive_segment_incident_does_not_propagate` above.
    #[test]
    fn gwr_cornish_main_line_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-8", "Signal failure at Truro", "Signal failure causing delays at Truro.", &["GW"], &["TRU"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-cornish-main-line".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 4.4's research found a genuine multi-station shared trunk with
    // cross-country.toml's own `xc-south-west` segment (Taunton-Exeter St
    // Davids-Newton Abbot-Plymouth, on the Bristol to Exeter line / South
    // Devon Main Line) — unlike gwr-south-wales.toml's single-waypoint
    // overlaps with cross-country.toml/xc-cardiff.toml, which deliberately
    // stayed station-only (see
    // `gwr_south_wales_station_overlap_with_xc_cardiff_stays_exclusive_each_line`
    // above). Exeter St Davids (EXD) is the station both of Task 4.4's own
    // files share with each other (the west-of-england/Cornish Main Line
    // split boundary) *and* with cross-country.toml, so an incident there
    // should propagate to all three as a shared-trunk event, mirroring
    // `swr_shared_trunk_incident_propagates`'s / `xc_hub_incident_propagates_
    // to_every_cross_country_arm`'s full-set-assertion shape.
    #[test]
    fn gwr_trunk_xc_south_west_incident_propagates_across_west_of_england_and_cornish_main_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-9", "Flooding at Exeter St Davids", "Flooding causing delays to GWR and CrossCountry services.", &["GW"], &["EXD"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from([
                "gwr-west-of-england".to_string(),
                "gwr-cornish-main-line".to_string(),
                "cross-country".to_string(),
            ])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    // Task 4.5's research found this line's real local/stopping service runs
    // on the physically separate Relief lines (Reading railway station's own
    // Wikipedia article, independently corroborated at Southall), not the
    // express Main lines gwr-trunk-paddington represents — so, unlike its
    // GWR siblings, `gwr-thames-valley` does NOT reuse `gwr-trunk-paddington`
    // anywhere, including at PAD/RDG. The whole line (both the Didcot-Oxford
    // and Reading-Newbury branches) uses one exclusive segment,
    // `gwr-thames-valley`, not shared with any other catalogued line. Culham
    // (CUM), on the Oxford branch, is a clean ExclusiveSegment case, mirroring
    // `swr_exclusive_segment_incident_does_not_propagate` /
    // `gwr_cotswold_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn gwr_thames_valley_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-10", "Signal failure at Culham", "Signal failure causing delays at Culham.", &["GW"], &["CUM"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-thames-valley".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // task-4.5-brief.md's plan-mandated regression guard: Maidenhead (MAI) is
    // a real station overlap between `gwr-thames-valley` (this task) and
    // elizabeth-line.toml's own `elizabeth-west` segment — both lines call
    // there (different service classes on the same physical station: a 2tph
    // GWR semi-fast continuing past Reading to Didcot/Oxford/Newbury, versus
    // the Elizabeth line's high-frequency metro stopper terminating at
    // Reading) but this task's research deliberately did not force
    // segment-sharing (see gwr-thames-valley.toml's own segment-naming
    // comment). So an incident at Maidenhead should match BOTH lines (real
    // station overlap) but EACH must stay `MatchScope::ExclusiveSegment` for
    // its own segment, never `SharedSegment` — mirrors
    // `gwr_south_wales_station_overlap_with_xc_cardiff_stays_exclusive_each_line`.
    #[test]
    fn gwr_thames_valley_station_overlap_with_elizabeth_west_stays_exclusive_each_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-11", "Points failure at Maidenhead", "Points failure causing delays at Maidenhead.", &["GW"], &["MAI"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-thames-valley".to_string(), "elizabeth-line".to_string()]));
        for m in &matches {
            assert_eq!(
                m.scope,
                MatchScope::ExclusiveSegment,
                "{} should stay ExclusiveSegment (station overlap, not a shared segment)",
                m.line.id
            );
        }
    }

    // A second, genuine (not just assumed) overlap this task's own research
    // found beyond what task-4.5-brief.md names: Oxford (OXF) is also
    // gwr-cotswold.toml's own exclusive segment's starting station, and the
    // Didcot-Oxford stretch both lines use is real single-track shared
    // infrastructure (see gwr-thames-valley.toml's own segment-naming
    // comment). Kept as station overlap only for this task's file-scope
    // reasons, so both lines should stay ExclusiveSegment, never
    // SharedSegment, mirroring the Maidenhead/elizabeth-line case above.
    // xc-south-coast.toml also calls at OXF (already documented, by
    // gwr-cotswold.toml's own comment, as a pre-existing station overlap
    // with that line) — included here too, also staying ExclusiveSegment.
    #[test]
    fn gwr_thames_valley_station_overlap_with_gwr_cotswold_stays_exclusive_each_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-12", "Overhead line damage at Oxford", "Overhead line damage causing delays at Oxford.", &["GW"], &["OXF"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["gwr-thames-valley".to_string(), "gwr-cotswold".to_string(), "xc-south-coast".to_string()])
        );
        for m in &matches {
            assert_eq!(
                m.scope,
                MatchScope::ExclusiveSegment,
                "{} should stay ExclusiveSegment (station overlap, not a shared segment)",
                m.line.id
            );
        }
    }

    // A third genuine overlap this task's own research found: Newbury (NBY)
    // is also gwr-west-of-england.toml's own exclusive segment's starting
    // station, and the Reading-Newbury stretch both lines use (via Southcote
    // Junction) is real shared Berks and Hants line track (see
    // gwr-thames-valley.toml's own segment-naming comment). Kept as station
    // overlap only for this task's file-scope reasons, mirroring the Oxford/
    // gwr-cotswold.toml case above. An earlier draft of gwr-bristol-
    // suburban.toml's own Westbury/Castle Cary fix mistakenly reused the
    // whole `gwr-west-of-england` segment name (not just WSB/CLC), which
    // pulled NBY into SharedSegment status too even though
    // gwr-bristol-suburban's own service never reaches it. The final-review
    // fix wave narrowed that shared segment to a new name,
    // `gwr-westbury-castle-cary` (WSB/CLC only — see
    // `gwr_westbury_castle_cary_trunk_incident_propagates_to_bristol_
    // suburban` below), so NBY is once again a genuinely exclusive station on
    // gwr-west-of-england's own `gwr-west-of-england` segment: both lines
    // should now stay `MatchScope::ExclusiveSegment` for their own segment,
    // confirming this is a real station overlap, not a segment-level share.
    #[test]
    fn gwr_thames_valley_station_overlap_with_gwr_west_of_england_stays_exclusive_each_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-13", "Points failure at Newbury", "Points failure causing delays at Newbury.", &["GW"], &["NBY"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-thames-valley".to_string(), "gwr-west-of-england".to_string()]));
        for m in &matches {
            assert_eq!(
                m.scope,
                MatchScope::ExclusiveSegment,
                "{} should stay ExclusiveSegment (station overlap, not a shared segment)",
                m.line.id
            );
        }
    }

    // Task 4.6's own exclusive segment (`gwr-severn-beach`) covers the whole
    // Severn Beach branch — Severn Beach itself (SVB) is not shared with any
    // other catalogued line, so this should stay a clean ExclusiveSegment
    // case, mirroring `swr_exclusive_segment_incident_does_not_propagate` /
    // `gwr_cotswold_exclusive_segment_incident_does_not_propagate`.
    #[test]
    fn gwr_bristol_suburban_severn_beach_exclusive_segment_incident_does_not_propagate() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-14", "Trespass incident at Severn Beach", "Trespass incident causing delays at Severn Beach.", &["GW"], &["SVB"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-bristol-suburban".to_string()]));
        assert_eq!(matches[0].scope, MatchScope::ExclusiveSegment);
    }

    // Task 4.6's research found a genuine, multi-station shared trunk with
    // gwr-west-of-england.toml between Westbury and Castle Cary — both
    // stations' own Wikipedia pages independently confirm this ("the Reading
    // to Taunton Line and the Heart of Wessex Line... share tracks between
    // Westbury and Castle Cary stations"), the same shape as `xc-south-west`/
    // `gwr-trunk-paddington` earlier in this batch. An earlier draft reused
    // gwr-west-of-england.toml's own `gwr-west-of-england` segment name
    // verbatim for this — wrong, because that also pulled Newbury (not
    // reached by gwr-bristol-suburban.toml's service) and Frome/Bruton
    // (gwr-bristol-suburban.toml's own exclusive territory, not actually
    // shared) into "shared" status. The final-review fix wave introduced a
    // new, narrower segment name, `gwr-westbury-castle-cary`, covering ONLY
    // Westbury (WSB) and Castle Cary (CLC) — the two stations both files'
    // own cited sources actually name as shared. An incident at either
    // should still propagate to both lines as a shared-trunk event, mirroring
    // `swr_shared_trunk_incident_propagates`'s / `gwr_trunk_xc_south_west_
    // incident_propagates_across_west_of_england_and_cornish_main_line`'s
    // shape.
    #[test]
    fn gwr_westbury_castle_cary_trunk_incident_propagates_to_bristol_suburban() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-15", "Points failure at Westbury", "Points failure causing delays at Westbury.", &["GW"], &["WSB"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(
            matched_ids,
            HashSet::from(["gwr-west-of-england".to_string(), "gwr-bristol-suburban".to_string()])
        );
        for m in &matches {
            assert_eq!(m.scope, MatchScope::SharedSegment, "{} should be SharedSegment", m.line.id);
        }
    }

    // A second, genuine (not just assumed) overlap this task's own research
    // found beyond the Westbury/Castle Cary one the brief itself names: Bath
    // Spa (BTH) and Bristol Temple Meads (BRI) are also gwr-main-line.toml's
    // own exclusive-segment stations (its `gwr-main-line` segment). This
    // task's research confirms genuine physical track sharing (there is only
    // one railway between Bristol Temple Meads and Bath Spa), but
    // `gwr-main-line` also covers Chippenham (CPM), which this line's own
    // Bristol-Weymouth service does not reach — the same file-scope reason
    // gwr-thames-valley.toml's Oxford/Newbury cases stayed station-overlap
    // only (see `gwr_thames_valley_station_overlap_with_gwr_cotswold_stays_
    // exclusive_each_line` above). So an incident at Bath Spa should match
    // both lines (real station overlap) but each must stay
    // `MatchScope::ExclusiveSegment`, never `SharedSegment`.
    #[test]
    fn gwr_bristol_suburban_station_overlap_with_gwr_main_line_stays_exclusive_each_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-16", "Overhead line damage at Bath Spa", "Overhead line damage causing delays at Bath Spa.", &["GW"], &["BTH"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-bristol-suburban".to_string(), "gwr-main-line".to_string()]));
        for m in &matches {
            assert_eq!(
                m.scope,
                MatchScope::ExclusiveSegment,
                "{} should stay ExclusiveSegment (station overlap, not a shared segment)",
                m.line.id
            );
        }
    }

    // A third genuine overlap this task's own research found, caught during
    // review after an earlier draft of gwr-bristol-suburban.toml's own WEY
    // comment wrongly claimed SWR's route "is not otherwise catalogued yet":
    // Weymouth (WEY) is also swr-south-west-main.toml's own terminus (its
    // own exclusive `swr-swml-south` segment). gwr-bristol-suburban's own
    // Bristol-Weymouth service never runs over any of swr-south-west-
    // main.toml's own claimed stations except WEY itself, so this stays
    // station overlap only, not a shared segment — different segment names
    // (`gwr-bristol-weymouth` vs `swr-swml-south`) mean no incorrect
    // `SharedSegment` cross-propagation. Mirrors
    // `gwr_bristol_suburban_station_overlap_with_gwr_main_line_stays_exclusive_each_line`
    // above.
    #[test]
    fn gwr_bristol_suburban_station_overlap_with_swr_south_west_main_stays_exclusive_each_line() {
        let lines = load_all_lines();
        let registry = SegmentRegistry::new(&lines);
        let inc = incident("GW-17", "Flooding at Weymouth", "Flooding causing delays at Weymouth.", &["GW"], &["WEY"]);
        let matches = lines_affected_by(&inc, &lines, &registry);
        let matched_ids: HashSet<String> = matches.iter().map(|m| m.line.id.clone()).collect();
        assert_eq!(matched_ids, HashSet::from(["gwr-bristol-suburban".to_string(), "swr-south-west-main".to_string()]));
        for m in &matches {
            assert_eq!(
                m.scope,
                MatchScope::ExclusiveSegment,
                "{} should stay ExclusiveSegment (station overlap, not a shared segment)",
                m.line.id
            );
        }
    }
}
