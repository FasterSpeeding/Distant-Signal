//! Differential test: for every query below, `scan_connections`'s earliest
//! arrival must equal `raptor_search`'s own best (round-1-onward, lowest
//! arrival) Pareto entry -- the primary correctness mechanism named in
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §1.
//! The two algorithms share NO search-logic code (only the pure
//! `schedule_query::interchange` data layer, per this plan's Judgment
//! Call 3) -- agreement here is real, independent evidence, not a
//! tautology.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

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
        working_departure_min: dep,
        working_arrival_min: arr,
        can_board: true,
        can_alight: true,
    }
}

/// A small but structurally varied network: two direct EUSTON->MKC
/// services (one earlier, one later), a EUSTON->MKC->MAN two-leg route
/// requiring a real 5-minute minimum change at MKC, an alternative
/// MKC->MAN service too tight to make the change, and an
/// EUSTON<->KINGX fixed link with no train involved.
fn network() -> (Vec<Connection>, InterchangeData) {
    // Sorted by departure_min ascending, per ScanOptions::connections's and
    // RaptorOptions::connections's own doc comments -- both algorithms
    // rely on this order without re-sorting. DIRECT-EARLY and LEG1 tie at
    // 480; their relative order doesn't matter.
    let connections = vec![
        conn("DIRECT-EARLY", "EUSTON", "MKC", 480, 530),
        conn("LEG1", "EUSTON", "MKC", 480, 530),
        conn("TOO-TIGHT", "MKC", "MAN", 531, 590),
        conn("LEG2", "MKC", "MAN", 536, 600),
        conn("DIRECT-LATE", "EUSTON", "MKC", 600, 650),
    ];
    let mut interchange = InterchangeData {
        modal_change: schedule_query::ModalChangeBuffer::default(),
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
        // Test-only literal, appropriate for this fixture's shallow depth
        // (at most one change) -- NOT a value to copy elsewhere; see
        // RaptorOptions::max_rounds's own doc comment against relying on
        // an unreasoned default.
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
            // RAPTOR's entry at that arrival is the first round to reach
            // it, so it has the fewest changes; CSA must match it.
            let raptor_changes = raptor_results
                .iter()
                .find(|j| j.arrival_min == raptor_arrival)
                .map(|j| j.changes);
            assert_eq!(
                Some(train_changes(csa)),
                raptor_changes,
                "CSA and RAPTOR disagree on the fewest changes for {from} -> {to} departing {departure_min}"
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
    assert_eq!(
        raptor.len(),
        1,
        "exactly one Pareto entry is expected for this fixture -- a future fixture change \
         adding a second path to KINGX must fail loudly here, not silently index the wrong entry"
    );
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

/// One train, A -> U -> D -> Z, where U is pick-up-only and D is
/// set-down-only. Both algorithms must agree that nobody alights at U and
/// nobody boards at D, while riding through both is fine.
fn directional_network() -> (Vec<Connection>, InterchangeData) {
    let mut a_u = conn("T1", "A", "U", 480, 490);
    a_u.can_alight = false;
    let u_d = conn("T1", "U", "D", 491, 500);
    let mut d_z = conn("T1", "D", "Z", 501, 510);
    d_z.can_board = false;
    let interchange = InterchangeData {
        modal_change: schedule_query::ModalChangeBuffer::default(),
        change_time_by_tiploc: HashMap::new(),
        tiploc_to_crs: HashMap::new(),
        crs_to_tiplocs: HashMap::new(),
        fixed_links_from_crs: HashMap::new(),
    };
    (vec![a_u, u_d, d_z], interchange)
}

fn arrival(
    connections: &[Connection],
    interchange: &InterchangeData,
    from: &str,
    to: &str,
) -> Option<u32> {
    let (from, to) = (vec![from.to_string()], vec![to.to_string()]);
    let csa = scan_connections(ScanOptions {
        connections,
        interchange,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: 0,
        date: date(),
    })
    .map(|journey| journey.arrival_min);
    let raptor = raptor_search(RaptorOptions {
        connections,
        interchange,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: 0,
        date: date(),
        max_rounds: 4,
    })
    .iter()
    .map(|journey| journey.arrival_min)
    .min();
    assert_eq!(csa, raptor, "{from:?} -> {to:?}");
    csa
}

#[test]
fn set_down_only_and_pick_up_only_stops_agree() {
    let (connections, interchange) = directional_network();
    // Cannot alight at the pick-up-only stop.
    assert_eq!(arrival(&connections, &interchange, "A", "U"), None);
    // Can board there.
    assert_eq!(arrival(&connections, &interchange, "U", "Z"), Some(510));
    // Can alight at the set-down-only stop.
    assert_eq!(arrival(&connections, &interchange, "A", "D"), Some(500));
    // Cannot board there.
    assert_eq!(arrival(&connections, &interchange, "D", "Z"), None);
    // Riding through both is fine.
    assert_eq!(arrival(&connections, &interchange, "A", "Z"), Some(510));
}

/// A journey's changes, as `RaptorJourney::changes` counts them.
fn train_changes(journey: &trip_planner::Journey) -> u32 {
    let trains = journey
        .legs
        .iter()
        .filter(|leg| matches!(leg, trip_planner::JourneyLeg::Train(_)))
        .count();
    u32::try_from(trains).unwrap().saturating_sub(1)
}

/// Pseudo-random small networks, dense enough that several routes often
/// tie on arrival with different numbers of changes: CSA's answer must have
/// RAPTOR's earliest arrival AND its fewest changes at that arrival. With
/// change times, a same-CRS sibling pair (B, B2) and a fixed link (C to D).
#[test]
fn random_networks_agree_on_arrival_and_fewest_changes() {
    let stops = ["A", "B", "B2", "C", "D", "E", "F", "G"];
    let mut seed: u64 = 0x00c5_a0ff;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (seed >> 33) % n
    };
    let mut interchange = InterchangeData {
        modal_change: schedule_query::ModalChangeBuffer::default(),
        change_time_by_tiploc: [("C", 3), ("E", 0), ("F", 8)]
            .iter()
            .map(|(t, m)| ((*t).to_string(), *m))
            .collect(),
        tiploc_to_crs: HashMap::new(),
        crs_to_tiplocs: HashMap::new(),
        fixed_links_from_crs: HashMap::new(),
    };
    for (tiploc, crs) in [("B", "BBB"), ("B2", "BBB"), ("C", "CCC"), ("D", "DDD")] {
        interchange
            .tiploc_to_crs
            .insert(tiploc.to_string(), crs.to_string());
        interchange
            .crs_to_tiplocs
            .entry(crs.to_string())
            .or_default()
            .push(tiploc.to_string());
    }
    interchange.fixed_links_from_crs.insert(
        "CCC".to_string(),
        vec![FixedLink {
            mode: "WALK".to_string(),
            to_crs: "DDD".to_string(),
            minutes: 7,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
        }],
    );
    let (mut checked, mut changed) = (0, 0);
    for _ in 0..80 {
        let mut connections = Vec::new();
        for train in 0..40 {
            let mut at = stops[usize::try_from(next(8)).unwrap()];
            let mut time = 300 + u32::try_from(next(240)).unwrap();
            for _ in 0..=next(1) {
                let mut to = stops[usize::try_from(next(8)).unwrap()];
                if to == at {
                    to = stops[(stops.iter().position(|s| *s == at).unwrap() + 1) % 8];
                }
                // Coarse times, so arrivals tie often.
                let run = 10 * (1 + u32::try_from(next(4)).unwrap());
                connections.push(conn(&format!("T{train}"), at, to, time, time + run));
                time += run + u32::try_from(next(2)).unwrap();
                at = to;
            }
        }
        connections.sort_by(|a, b| {
            (a.departure_min, &a.uid, &a.from_tiploc).cmp(&(
                b.departure_min,
                &b.uid,
                &b.from_tiploc,
            ))
        });
        for (from, to) in [("A", "G"), ("B", "F"), ("E", "D"), ("G", "A")] {
            for departure in [300, 360, 420] {
                let (csa, _) = assert_agreement(&connections, &interchange, from, to, departure);
                checked += 1;
                changed += usize::from(csa.is_some_and(|j| train_changes(&j) >= 1));
            }
        }
    }
    assert!(
        changed * 4 > checked,
        "only {changed} of {checked} cases needed a change"
    );
}
