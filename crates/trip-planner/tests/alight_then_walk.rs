//! A walk on from a bus or ferry is a change off it: CSA and RAPTOR charge
//! the bus and ferry buffer (`schedule_query::ModalChangeBuffer`) after
//! alighting and before the walk, as a fresh boarding at the same stop
//! would and as the arrive-by search already did. Modelled on a bus to
//! Heathrow's Terminal 3 bus stop (`HTRBUS3`), an 8-minute walk to
//! Heathrow Terminals 2 & 3 (`HTRWAPT`, CRS `HXX`, 2-minute change) and a
//! train to Paddington.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use schedule_query::{Connection, FixedLink, InterchangeData, ModalChangeBuffer};
use trip_planner::{
    JourneyLeg, RaptorOptions, ScanOptions, TransferLeg, raptor_search, scan_connections,
};

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

fn walk(to_crs: &str, minutes: i32) -> FixedLink {
    FixedLink {
        mode: "WALK".to_string(),
        to_crs: to_crs.to_string(),
        minutes,
        valid_from: "0000".to_string(),
        valid_to: "2359".to_string(),
        days_mask: "1111111".to_string(),
    }
}

/// The bus (`BUS1`, or the train `RAIL1` on the same path) reaches the
/// stop at 10:00; trains leave HXX at 10:09, 10:10, 10:14 and 10:15.
fn network() -> Vec<Connection> {
    let mut connections = vec![
        conn("BUS1", "WOKING", "HTRBUS3", 540, 600),
        conn("RAIL1", "WOKING", "HTRBUS3", 541, 600),
        conn("T09", "HTRWAPT", "PADTON", 609, 624),
        conn("T10", "HTRWAPT", "PADTON", 610, 625),
        conn("T14", "HTRWAPT", "PADTON", 614, 629),
        conn("T15", "HTRWAPT", "PADTON", 615, 630),
    ];
    connections.sort_by_key(|c| c.departure_min);
    connections
}

fn interchange(buffer_minutes: u32) -> InterchangeData {
    let tiplocs = [
        ("WOKING", "WOK"),
        ("HTRBUS3", "tiploc:HTRBUS3"),
        ("HTRWAPT", "HXX"),
        ("PADTON", "PAD"),
    ];
    InterchangeData {
        modal_change: ModalChangeBuffer {
            road_or_water_uids: HashSet::from(["BUS1".to_string()]),
            minutes: buffer_minutes,
        },
        change_time_by_tiploc: HashMap::from([("HTRWAPT".to_string(), 2)]),
        tiploc_to_crs: tiplocs
            .iter()
            .map(|(t, c)| ((*t).to_string(), (*c).to_string()))
            .collect(),
        crs_to_tiplocs: tiplocs
            .iter()
            .map(|(t, c)| ((*c).to_string(), vec![(*t).to_string()]))
            .collect(),
        fixed_links_from_crs: HashMap::from([
            ("tiploc:HTRBUS3".to_string(), vec![walk("HXX", 8)]),
            ("HXX".to_string(), vec![walk("tiploc:HTRBUS3", 8)]),
        ]),
    }
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
}

/// Train UIDs, and `WALK n` for a walk.
fn describe(legs: &[JourneyLeg]) -> Vec<String> {
    legs.iter()
        .map(|leg| match leg {
            JourneyLeg::Train(train) => train.uid.clone(),
            JourneyLeg::Transfer(TransferLeg { mode, minutes, .. }) => format!("{mode} {minutes}"),
        })
        .collect()
}

/// The CSA and RAPTOR plans from `first` (a UID that leaves Woking) to
/// Paddington, the other Woking service removed.
fn plans(first: &str, buffer_minutes: u32) -> (Vec<String>, Vec<String>) {
    let connections: Vec<Connection> = network()
        .into_iter()
        .filter(|c| c.from_tiploc != "WOKING" || c.uid == first)
        .collect();
    let ic = interchange(buffer_minutes);
    let from = vec!["WOKING".to_string()];
    let to = vec!["PADTON".to_string()];
    let csa = scan_connections(ScanOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: 530,
        date: date(),
    })
    .unwrap();
    let raptor = raptor_search(RaptorOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: 530,
        date: date(),
        max_rounds: 4,
    });
    (describe(&csa.legs), describe(&raptor[0].legs))
}

#[test]
fn walking_on_from_a_bus_owes_the_buffer_first() {
    // 10:00 + 5 (off the bus) + 8 (walk) + 2 (HXX change) = 10:15.
    let (csa, raptor) = plans("BUS1", 5);
    assert_eq!(csa, ["BUS1", "WALK 8", "T15"]);
    assert_eq!(raptor, csa);
}

#[test]
fn without_the_buffer_or_off_a_train_the_walk_leaves_at_once() {
    // 10:00 + 8 + 2 = 10:10.
    let (csa, raptor) = plans("BUS1", 0);
    assert_eq!(csa, ["BUS1", "WALK 8", "T10"]);
    assert_eq!(raptor, csa);
    let (csa, raptor) = plans("RAIL1", 5);
    assert_eq!(csa, ["RAIL1", "WALK 8", "T10"]);
    assert_eq!(raptor, csa);
}

#[test]
fn the_walk_leg_starts_after_the_buffer() {
    let connections: Vec<Connection> = network().into_iter().filter(|c| c.uid != "RAIL1").collect();
    let ic = interchange(5);
    let journey = scan_connections(ScanOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &["WOKING".to_string()],
        to_tiplocs: &["HTRWAPT".to_string()],
        departure_min: 530,
        date: date(),
    })
    .unwrap();
    // Arriving at the walk's end is the destination: 10:00 + 5 + 8.
    assert_eq!(journey.arrival_min, 613);
    let JourneyLeg::Transfer(walk) = &journey.legs[1] else {
        panic!("the second leg is the walk: {:?}", journey.legs);
    };
    assert_eq!((walk.departure_min, walk.arrival_min), (605, 613));
}
