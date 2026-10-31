//! DS-135: a journey from or to a Tube-only stop (`trip_planner::access`).
//! The stop is a virtual end joined to its candidate National Rail
//! stations by access links, so a plan from it is a multi-source search
//! and a plan to it a multi-target one. The network is Uxbridge-like: the
//! stop `940GUXB` reaches Harrow-on-the-Hill (`HOH`) by the Metropolitan
//! line in 32 minutes, and West Ruislip (`WRU`) by a 50-minute walk; both
//! have trains to Marylebone (`MYB`).

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

use std::collections::HashMap;

use chrono::NaiveDate;
use schedule_query::{Connection, InterchangeData, ModalChangeBuffer};
use trip_planner::{
    AccessEnd, AccessLink, ArriveByOptions, JourneyLeg, RaptorOptions, ScanOptions, TransferLeg,
    raptor_arrive_by, raptor_search, scan_connections, scan_connections_arrive_by, with_access,
};

const STOP: &str = "940GUXB";

fn conn(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
    Connection {
        operator: schedule_query::OperatorId::UNKNOWN,
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

fn hhmm(h: u32, m: u32) -> u32 {
    h * 60 + m
}

/// Trains both ways between Marylebone and each candidate.
fn network() -> Vec<Connection> {
    let mut connections = vec![
        // Up to Marylebone.
        conn("HOH0840", "HROW", "MRYLBNE", hhmm(8, 40), hhmm(8, 55)),
        conn("HOH0910", "HROW", "MRYLBNE", hhmm(9, 10), hhmm(9, 25)),
        conn("WRU0820", "WRUISLP", "MRYLBNE", hhmm(8, 20), hhmm(8, 45)),
        conn("WRU0845", "WRUISLP", "MRYLBNE", hhmm(8, 45), hhmm(9, 10)),
        conn("WRU0905", "WRUISLP", "MRYLBNE", hhmm(9, 5), hhmm(9, 30)),
        // Down from Marylebone.
        conn("MYB1000", "MRYLBNE", "HROW", hhmm(10, 0), hhmm(10, 15)),
        conn("MYB1020", "MRYLBNE", "HROW", hhmm(10, 20), hhmm(10, 35)),
        conn("MYB0940", "MRYLBNE", "WRUISLP", hhmm(9, 40), hhmm(10, 5)),
    ];
    connections.sort_by_key(|c| c.departure_min);
    connections
}

fn base() -> InterchangeData {
    let tiplocs = [("HROW", "HOH"), ("WRUISLP", "WRU"), ("MRYLBNE", "MYB")];
    InterchangeData {
        modal_change: ModalChangeBuffer::default(),
        // Every station's minimum change time is the default 5.
        change_time_by_tiploc: HashMap::new(),
        tiploc_to_crs: tiplocs
            .iter()
            .map(|(t, c)| ((*t).to_string(), (*c).to_string()))
            .collect(),
        crs_to_tiplocs: tiplocs
            .iter()
            .map(|(t, c)| ((*c).to_string(), vec![(*t).to_string()]))
            .collect(),
        fixed_links_from_crs: HashMap::new(),
    }
}

fn tube_hours() -> Vec<(String, String)> {
    vec![
        ("0530".to_string(), "2359".to_string()),
        ("0000".to_string(), "0030".to_string()),
    ]
}

fn links() -> Vec<AccessLink> {
    vec![
        AccessLink {
            operator: schedule_query::OperatorId::UNKNOWN,
            crs: "HOH".to_string(),
            mode: "TUBE".to_string(),
            minutes: 32,
            windows: tube_hours(),
        },
        AccessLink {
            operator: schedule_query::OperatorId::UNKNOWN,
            crs: "WRU".to_string(),
            mode: "WALK".to_string(),
            minutes: 50,
            windows: Vec::new(),
        },
    ]
}

/// Wednesday 21 October 2026.
fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 21).unwrap()
}

fn describe(legs: &[JourneyLeg]) -> Vec<String> {
    legs.iter()
        .map(|leg| match leg {
            JourneyLeg::Train(train) => train.uid.clone(),
            JourneyLeg::Transfer(TransferLeg {
                mode,
                minutes,
                from_tiploc,
                to_tiploc,
                ..
            }) => format!("{mode} {minutes} {from_tiploc}-{to_tiploc}"),
        })
        .collect()
}

fn from_stop(ic: &InterchangeData, depart: u32) -> Option<(Vec<String>, u32)> {
    let connections = network();
    let from = vec![STOP.to_string()];
    let to = vec!["MRYLBNE".to_string()];
    scan_connections(ScanOptions {
        connections: &connections,
        interchange: ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: depart,
        date: date(),
    })
    .map(|journey| (describe(&journey.legs), journey.arrival_min))
}

fn origin_ic() -> InterchangeData {
    let links = links();
    with_access(
        &base(),
        Some(AccessEnd {
            code: STOP,
            links: &links,
        }),
        None,
        None,
    )
}

fn destination_ic() -> InterchangeData {
    let links = links();
    with_access(
        &base(),
        None,
        Some(AccessEnd {
            code: STOP,
            links: &links,
        }),
        None,
    )
}

#[test]
fn the_search_starts_from_every_candidate_and_takes_the_best() {
    let ic = origin_ic();
    // 08:00: Tube to Harrow (08:32), change (5), the 08:40 to 08:55; the
    // walk to West Ruislip (08:50) would make only the 09:05.
    assert_eq!(
        from_stop(&ic, hhmm(8, 0)).unwrap(),
        (
            vec!["TUBE 32 940GUXB-HROW".to_string(), "HOH0840".to_string()],
            hhmm(8, 55)
        )
    );
    // 08:10: Harrow at 08:42 misses the 08:40 and makes the 09:10 (09:25);
    // the walk reaches West Ruislip at 09:00 for the 09:05 (09:30).
    assert_eq!(
        from_stop(&ic, hhmm(8, 10)).unwrap(),
        (
            vec!["TUBE 32 940GUXB-HROW".to_string(), "HOH0910".to_string()],
            hhmm(9, 25)
        )
    );
    // 07:25: the walk reaches West Ruislip at 08:15 for the 08:20 (08:45),
    // ahead of the Tube to Harrow (07:57) and the 08:40 (08:55).
    assert_eq!(
        from_stop(&ic, hhmm(7, 25)).unwrap(),
        (
            vec!["WALK 50 940GUXB-WRUISLP".to_string(), "WRU0820".to_string()],
            hhmm(8, 45)
        )
    );
}

#[test]
fn a_link_outside_its_hours_is_not_offered() {
    let ic = origin_ic();
    // 00:45 is outside the Tube's hours: only the walk is open, and there is
    // no train before 08:20 anyway, so the walk is started at 00:45.
    let (legs, arrival) = from_stop(&ic, hhmm(0, 45)).unwrap();
    assert_eq!(legs, ["WALK 50 940GUXB-WRUISLP", "WRU0820"]);
    assert_eq!(arrival, hhmm(8, 45));
}

#[test]
fn raptor_agrees_with_the_scan() {
    let ic = origin_ic();
    let connections = network();
    let from = vec![STOP.to_string()];
    let to = vec!["MRYLBNE".to_string()];
    let journeys = raptor_search(RaptorOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: hhmm(8, 0),
        date: date(),
        max_rounds: 4,
    });
    let quickest = journeys.iter().min_by_key(|j| j.arrival_min).unwrap();
    assert_eq!(
        describe(&quickest.legs),
        ["TUBE 32 940GUXB-HROW", "HOH0840"]
    );
    assert_eq!(quickest.changes, 0);
}

#[test]
fn the_search_ends_at_whichever_candidate_reaches_the_stop_first() {
    let ic = destination_ic();
    let connections = network();
    let from = vec!["MRYLBNE".to_string()];
    let to = vec![STOP.to_string()];
    // 09:30 from Marylebone: the 09:40 to West Ruislip (10:05) and the walk
    // (10:55), or the 10:00 to Harrow (10:15) and the Tube (10:47).
    let journey = scan_connections(ScanOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: hhmm(9, 30),
        date: date(),
    })
    .unwrap();
    assert_eq!(describe(&journey.legs), ["MYB1000", "TUBE 32 HROW-940GUXB"]);
    assert_eq!(journey.arrival_min, hhmm(10, 47));
}

#[test]
fn arrive_by_counts_the_egress_minutes() {
    let ic = destination_ic();
    let connections = network();
    let from = vec!["MRYLBNE".to_string()];
    let to = vec![STOP.to_string()];
    let options = |arrive_by_min| ArriveByOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        waypoints: &[],
        to_tiplocs: &to,
        vias: None,
        arrive_by_min,
        date: date(),
    };
    // By 11:00: the 10:20 reaches Harrow at 10:35, but the Tube takes it to
    // 11:07; the 10:00 (10:47) is the latest that makes it.
    let journey = scan_connections_arrive_by(options(hhmm(11, 0)), None, None).unwrap();
    assert_eq!(describe(&journey.legs), ["MYB1000", "TUBE 32 HROW-940GUXB"]);
    // By 11:10 the 10:20 does.
    let journey = scan_connections_arrive_by(options(hhmm(11, 10)), None, None).unwrap();
    assert_eq!(journey.legs.len(), 2);
    assert_eq!(describe(&journey.legs)[0], "MYB1020");
    // By 10:50: only the 09:40 and the walk (10:55) is too late, the 10:00
    // and the Tube (10:47) makes it.
    let journey = scan_connections_arrive_by(options(hhmm(10, 50)), None, None).unwrap();
    assert_eq!(describe(&journey.legs)[0], "MYB1000");
    // By 10:45 nothing arrives in time.
    assert!(scan_connections_arrive_by(options(hhmm(10, 45)), None, None).is_none());
    // RAPTOR's arrive-by agrees.
    let journeys = raptor_arrive_by(options(hhmm(11, 0)), None, None, 4);
    assert_eq!(
        describe(&journeys[0].legs),
        ["MYB1000", "TUBE 32 HROW-940GUXB"]
    );
}

#[test]
fn a_journey_never_passes_through_a_virtual_stop() {
    // After the last train, Harrow to West Ruislip could only be the Tube
    // into the stop and the walk out of it. Links leave a virtual origin and
    // enter a virtual destination, never both, so neither copy offers it.
    let connections = network();
    let from = vec!["HROW".to_string()];
    let to = vec!["WRUISLP".to_string()];
    for ic in [origin_ic(), destination_ic()] {
        assert!(
            scan_connections(ScanOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: hhmm(11, 0),
                date: date(),
            })
            .is_none()
        );
    }
}

#[test]
fn two_stops_on_one_line_get_the_direct_ride() {
    let links = links();
    let direct = AccessLink {
        operator: schedule_query::OperatorId::UNKNOWN,
        crs: String::new(),
        mode: "TUBE".to_string(),
        minutes: 20,
        windows: tube_hours(),
    };
    let ic = with_access(
        &base(),
        Some(AccessEnd {
            code: STOP,
            links: &links,
        }),
        Some(AccessEnd {
            code: "940GRLN",
            links: &links,
        }),
        Some(&direct),
    );
    let connections = network();
    let from = vec![STOP.to_string()];
    let to = vec!["940GRLN".to_string()];
    let journey = scan_connections(ScanOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: hhmm(8, 0),
        date: date(),
    })
    .unwrap();
    assert_eq!(describe(&journey.legs), ["TUBE 20 940GUXB-940GRLN"]);
}
