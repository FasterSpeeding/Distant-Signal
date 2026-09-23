//! Differential test: for every query below, `scan_connections`'s earliest
//! arrival must equal `raptor_search`'s own best (round-1-onward, lowest
//! arrival) Pareto entry -- the primary correctness mechanism named in
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §1.
//! The two algorithms share NO search-logic code (only the pure
//! `schedule_query::interchange` data layer, per this plan's Judgment
//! Call 3) -- agreement here is real, independent evidence, not a
//! tautology.

use chrono::NaiveDate;
use schedule_query::{Connection, FixedLink, InterchangeData};
use std::collections::HashMap;
use trip_planner::{RaptorOptions, ScanOptions, raptor_search, scan_connections};

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 23).unwrap()
}

fn conn(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
    Connection {
        uid: uid.to_string(),
        from_tiploc: from.to_string(),
        to_tiploc: to.to_string(),
        departure_min: dep,
        arrival_min: arr,
    }
}

/// A small but structurally varied network: two direct EUSTON->MKC
/// services (one earlier, one later), a EUSTON->MKC->MAN two-leg route
/// requiring a real 5-minute minimum change at MKC, an alternative
/// MKC->MAN service too tight to make the change, and an
/// EUSTON<->KINGX fixed link with no train involved.
fn network() -> (Vec<Connection>, InterchangeData) {
    let connections = vec![
        conn("DIRECT-EARLY", "EUSTON", "MKC", 480, 530),
        conn("DIRECT-LATE", "EUSTON", "MKC", 600, 650),
        conn("LEG1", "EUSTON", "MKC", 480, 530),
        conn("TOO-TIGHT", "MKC", "MAN", 531, 590),
        conn("LEG2", "MKC", "MAN", 536, 600),
    ];
    let mut interchange = InterchangeData {
        change_time_by_tiploc: HashMap::new(),
        tiploc_to_crs: HashMap::new(),
        crs_to_tiplocs: HashMap::new(),
        fixed_links_from_crs: HashMap::new(),
    };
    interchange
        .change_time_by_tiploc
        .insert("MKC".to_string(), 5);
    interchange
        .tiploc_to_crs
        .insert("EUSTON".to_string(), "EUS".to_string());
    interchange
        .crs_to_tiplocs
        .insert("KGX".to_string(), vec!["KINGX".to_string()]);
    interchange.fixed_links_from_crs.insert(
        "EUS".to_string(),
        vec![FixedLink {
            mode: "TUBE".to_string(),
            to_crs: "KGX".to_string(),
            minutes: 5,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
        }],
    );
    (connections, interchange)
}

/// Runs both algorithms for one query and asserts their earliest arrival
/// agrees, returning both results for any further assertion the caller
/// wants to make.
fn assert_agreement(
    connections: &[Connection],
    interchange: &InterchangeData,
    from: &str,
    to: &str,
    departure_min: u32,
) -> (
    Option<trip_planner::Journey>,
    Vec<trip_planner::RaptorJourney>,
) {
    let from_tiplocs = vec![from.to_string()];
    let to_tiplocs = vec![to.to_string()];

    let csa_result = scan_connections(ScanOptions {
        connections,
        interchange,
        from_tiplocs: &from_tiplocs,
        to_tiplocs: &to_tiplocs,
        departure_min,
        date: date(),
    });
    let raptor_results = raptor_search(RaptorOptions {
        connections,
        interchange,
        from_tiplocs: &from_tiplocs,
        to_tiplocs: &to_tiplocs,
        departure_min,
        date: date(),
        max_rounds: 8,
    });
    let raptor_best = raptor_results.iter().map(|j| j.arrival_min).min();

    match (&csa_result, raptor_best) {
        (Some(csa), Some(raptor_arrival)) => {
            assert_eq!(
                csa.arrival_min, raptor_arrival,
                "CSA and RAPTOR disagree on earliest arrival for {from} -> {to} departing {departure_min}: \
                 CSA says {}, RAPTOR says {raptor_arrival}",
                csa.arrival_min
            );
        }
        (None, None) => {}
        (csa, raptor) => panic!(
            "CSA and RAPTOR disagree on REACHABILITY for {from} -> {to} departing {departure_min}: \
             CSA={csa:?}, RAPTOR best={raptor:?}"
        ),
    }

    (csa_result, raptor_results)
}

#[test]
fn direct_journey_agrees() {
    let (connections, interchange) = network();
    let (csa, _) = assert_agreement(&connections, &interchange, "EUSTON", "MKC", 480);
    assert_eq!(csa.unwrap().arrival_min, 530);
}

#[test]
fn a_later_departure_time_still_agrees() {
    let (connections, interchange) = network();
    let (csa, _) = assert_agreement(&connections, &interchange, "EUSTON", "MKC", 590);
    assert_eq!(
        csa.unwrap().arrival_min,
        650,
        "only the DIRECT-LATE service is boardable this late"
    );
}

#[test]
fn a_journey_requiring_the_valid_change_agrees() {
    let (connections, interchange) = network();
    let (csa, _) = assert_agreement(&connections, &interchange, "EUSTON", "MAN", 480);
    assert_eq!(
        csa.unwrap().arrival_min,
        600,
        "the too-tight 531 change must be rejected by both algorithms identically"
    );
}

#[test]
fn a_fixed_link_only_query_agrees() {
    let (connections, interchange) = network();
    let (csa, raptor) = assert_agreement(&connections, &interchange, "EUSTON", "KINGX", 480);
    let csa = csa.expect("a fixed-link journey exists");
    assert_eq!(csa.arrival_min, 485);
    assert_eq!(raptor[0].changes, 0);
}

#[test]
fn an_unreachable_destination_agrees_on_no_journey_at_all() {
    let (connections, interchange) = network();
    assert_agreement(&connections, &interchange, "EUSTON", "NOWHERE", 480);
}

#[test]
fn a_departure_time_after_every_service_has_left_agrees_on_no_journey() {
    let (connections, interchange) = network();
    assert_agreement(&connections, &interchange, "EUSTON", "MKC", 700);
}
