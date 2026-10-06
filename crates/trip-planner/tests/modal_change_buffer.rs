//! The bus and ferry change buffer (`schedule_query::ModalChangeBuffer`):
//! every search -- CSA, RAPTOR, the staged search and arrive-by -- charges
//! it on the bus or ferry side of a change, and never at the origin or
//! destination. Modelled on St Andrews bus station -> Leuchars -> Edinburgh
//! (TIPLOCs SANWBUS, LEUCHRS, EDINBUR), with the 5-minute default.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

use std::collections::HashSet;

use chrono::NaiveDate;
use schedule_query::{Connection, InterchangeData, ModalChangeBuffer};
use trip_planner::{
    ArriveByOptions, JourneyLeg, RaptorOptions, ScanOptions, StagedOptions, raptor_search,
    scan_connections, scan_connections_arrive_by, scan_staged,
};

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
}

fn hm(h: u32, m: u32) -> u32 {
    h * 60 + m
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

/// The bus reaches Leuchars at 07:11; the 07:17 train is 6 minutes later
/// (enough for Leuchars's 5-minute default change time alone, not with a
/// 5-minute bus buffer on top), the 07:30 is the next.
fn network() -> Vec<Connection> {
    let mut connections = vec![
        conn("BUS1", "SANWBUS", "LEUCHRS", hm(7, 0), hm(7, 11)),
        conn("TRAIN17", "LEUCHRS", "EDINBUR", hm(7, 17), hm(8, 10)),
        conn("TRAIN30", "LEUCHRS", "EDINBUR", hm(7, 30), hm(8, 25)),
    ];
    connections.sort_by_key(|c| c.departure_min);
    connections
}

fn interchange(buffer_minutes: u32) -> InterchangeData {
    InterchangeData {
        modal_change: ModalChangeBuffer {
            road_or_water_uids: HashSet::from(["BUS1".to_string(), "BUS2".to_string()]),
            minutes: buffer_minutes,
        },
        ..InterchangeData::default()
    }
}

fn uids(legs: &[JourneyLeg]) -> Vec<&str> {
    legs.iter()
        .filter_map(|leg| match leg {
            JourneyLeg::Train(train) => Some(train.uid.as_str()),
            JourneyLeg::Transfer(_) => None,
        })
        .collect()
}

fn tiplocs(names: &[&str]) -> Vec<String> {
    names.iter().map(|t| (*t).to_string()).collect()
}

#[test]
fn csa_charges_the_buffer_when_changing_off_a_bus() {
    let connections = network();
    let from = tiplocs(&["SANWBUS"]);
    let to = tiplocs(&["EDINBUR"]);
    let plan = |buffer| {
        let ic = interchange(buffer);
        scan_connections(ScanOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            to_tiplocs: &to,
            departure_min: hm(6, 0),
            date: date(),
        })
        .unwrap()
    };
    let without = plan(0);
    assert_eq!(uids(&without.legs), ["BUS1", "TRAIN17"]);
    assert_eq!(without.arrival_min, hm(8, 10));
    let with = plan(5);
    assert_eq!(uids(&with.legs), ["BUS1", "TRAIN30"]);
    assert_eq!(with.arrival_min, hm(8, 25));
}

#[test]
fn raptor_and_the_staged_search_agree() {
    let connections = network();
    let from = tiplocs(&["SANWBUS"]);
    let to = tiplocs(&["EDINBUR"]);
    let ic = interchange(5);
    let raptor = raptor_search(RaptorOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: hm(6, 0),
        date: date(),
        max_rounds: 4,
    });
    assert_eq!(raptor.len(), 1);
    assert_eq!(raptor[0].arrival_min, hm(8, 25));
    assert_eq!(uids(&raptor[0].legs), ["BUS1", "TRAIN30"]);

    let staged = scan_staged(
        &StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            date: date(),
        },
        hm(6, 0),
        None,
        None,
    )
    .unwrap();
    assert_eq!(staged.arrival_min, hm(8, 25));
}

#[test]
fn arrive_by_mirrors_the_buffer() {
    let connections = network();
    let from = tiplocs(&["SANWBUS"]);
    let to = tiplocs(&["EDINBUR"]);
    let arrive_by = |buffer, deadline| {
        let ic = interchange(buffer);
        scan_connections_arrive_by(
            ArriveByOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &[],
                to_tiplocs: &to,
                arrive_by_min: deadline,
                date: date(),
            },
            None,
            None,
        )
    };
    // Without the buffer the 07:00 bus makes the 08:10 arrival.
    assert_eq!(arrive_by(0, hm(8, 15)).unwrap().arrival_min, hm(8, 10));
    // With it, nothing arrives by 08:15 ...
    assert!(arrive_by(5, hm(8, 15)).is_none());
    // ... and by 08:30 the bus connects into the 07:30 train.
    let journey = arrive_by(5, hm(8, 30)).unwrap();
    assert_eq!(uids(&journey.legs), ["BUS1", "TRAIN30"]);
}

/// Boarding a bus at a change owes the buffer too; arriving by bus at the
/// destination, or boarding one at the origin, does not.
#[test]
fn changing_onto_a_bus_owes_it_but_the_ends_of_the_journey_do_not() {
    let mut connections = vec![
        conn("TRAIN", "EDINBUR", "LEUCHRS", hm(6, 0), hm(6, 50)),
        conn("BUS2", "LEUCHRS", "SANWBUS", hm(6, 56), hm(7, 7)),
        conn("BUS3", "LEUCHRS", "SANWBUS", hm(7, 5), hm(7, 16)),
    ];
    connections.sort_by_key(|c| c.departure_min);
    let mut ic = interchange(5);
    ic.modal_change
        .road_or_water_uids
        .insert("BUS3".to_string());
    let journey = scan_connections(ScanOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &tiplocs(&["EDINBUR"]),
        to_tiplocs: &tiplocs(&["SANWBUS"]),
        departure_min: hm(5, 0),
        date: date(),
    })
    .unwrap();
    // 06:50 + 5 (change) + 5 (onto a bus) = 07:00: the 06:56 is missed.
    assert_eq!(uids(&journey.legs), ["TRAIN", "BUS3"]);
    // The bus's own arrival is the journey's: no buffer at the destination.
    assert_eq!(journey.arrival_min, hm(7, 16));

    // A bus straight from the origin: no buffer either.
    let direct = scan_connections(ScanOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &tiplocs(&["LEUCHRS"]),
        to_tiplocs: &tiplocs(&["SANWBUS"]),
        departure_min: hm(6, 56),
        date: date(),
    })
    .unwrap();
    assert_eq!(uids(&direct.legs), ["BUS2"]);
    assert_eq!(direct.arrival_min, hm(7, 7));
}
