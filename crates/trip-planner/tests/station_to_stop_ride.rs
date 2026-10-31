//! DS-167: Euston to Old Street Underground (`940GZZLUODS`). Old Street's
//! only candidate used to be its hub walk to the National Rail station
//! (`OLD`), so from Euston the planner took the Tube transfer link to
//! Moorgate, a train one stop to Old Street, and the walk: 38 minutes.
//! `data::tfl_access` now also offers Euston itself as an egress candidate,
//! a Northern line ride from Euston's own stop (12 minutes), and that one
//! ride wins, depart-after and arrive-by.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

use std::collections::HashMap;

use chrono::NaiveDate;
use schedule_query::{Connection, FixedLink, InterchangeData, ModalChangeBuffer};
use trip_planner::{
    AccessEnd, AccessLink, ArriveByOptions, JourneyLeg, RaptorOptions, ScanOptions, TransferLeg,
    raptor_search, scan_connections, scan_connections_arrive_by, with_access,
};

const STOP: &str = "940GZZLUODS";

fn hhmm(h: u32, m: u32) -> u32 {
    h * 60 + m
}

/// The one train: Moorgate 06:31 to Old Street 06:33.
fn connections() -> Vec<Connection> {
    vec![Connection {
        operator: schedule_query::OperatorId::UNKNOWN,
        uid: "C10117".to_string().into(),
        from_tiploc: "MRGT".to_string().into(),
        to_tiploc: "OLDST".to_string().into(),
        departure_min: hhmm(6, 31),
        arrival_min: hhmm(6, 33),
        working_departure_min: hhmm(6, 31),
        working_arrival_min: hhmm(6, 33),
        can_board: true,
        can_alight: true,
    }]
}

fn tube(from: &str, to: &str) -> (String, FixedLink) {
    (
        from.to_string(),
        FixedLink {
            only_without: schedule_query::OperatorId::UNKNOWN,
            operator: schedule_query::OperatorId::UNKNOWN,
            mode: "TUBE".to_string(),
            to_crs: to.to_string(),
            minutes: 16,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
        },
    )
}

/// Three stations and the timetable's 16-minute Tube link between Euston
/// and Moorgate, both ways.
fn base() -> InterchangeData {
    let tiplocs = [("EUSTON", "EUS"), ("MRGT", "MOG"), ("OLDST", "OLD")];
    let mut fixed_links_from_crs: HashMap<String, Vec<FixedLink>> = HashMap::new();
    for (from, link) in [tube("EUS", "MOG"), tube("MOG", "EUS")] {
        fixed_links_from_crs.entry(from).or_default().push(link);
    }
    let mut data = InterchangeData {
        modal_change: ModalChangeBuffer::default(),
        change_time_by_tiploc: HashMap::new(),
        tiploc_to_crs: tiplocs
            .iter()
            .map(|(t, c)| ((*t).to_string(), (*c).to_string()))
            .collect(),
        crs_to_tiplocs: tiplocs
            .iter()
            .map(|(t, c)| ((*c).to_string(), vec![(*t).to_string()]))
            .collect(),
        fixed_links_from_crs,
    };
    data.sort_fixed_links();
    data
}

fn link(crs: &str, mode: &str, minutes: i32) -> AccessLink {
    AccessLink {
        operator: schedule_query::OperatorId::UNKNOWN,
        crs: crs.to_string(),
        mode: mode.to_string(),
        minutes,
        windows: if mode == "WALK" {
            Vec::new()
        } else {
            vec![
                ("0530".to_string(), "2359".to_string()),
                ("0000".to_string(), "0030".to_string()),
            ]
        },
    }
}

fn with_egress(links: &[AccessLink]) -> InterchangeData {
    with_access(&base(), None, Some(AccessEnd { code: STOP, links }), None)
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

fn plan(ic: &InterchangeData) -> (Vec<String>, u32) {
    let connections = connections();
    let from = vec!["EUSTON".to_string()];
    let to = vec![STOP.to_string()];
    let journey = scan_connections(ScanOptions {
        connections: &connections,
        interchange: ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: hhmm(6, 0),
        date: date(),
    })
    .unwrap();
    (describe(&journey.legs), journey.arrival_min)
}

#[test]
fn with_only_the_hub_walk_the_planner_takes_the_train() {
    let ic = with_egress(&[link("OLD", "WALK", 5)]);
    assert_eq!(
        plan(&ic),
        (
            vec![
                "TUBE 16 EUSTON-MRGT".to_string(),
                "C10117".to_string(),
                format!("WALK 5 OLDST-{STOP}"),
            ],
            hhmm(6, 38)
        )
    );
}

#[test]
fn the_ride_from_euston_itself_wins() {
    let links = [
        link("OLD", "WALK", 5),
        link("MOG", "TUBE", 10),
        link("EUS", "TUBE", 12),
    ];
    let ic = with_egress(&links);
    assert_eq!(
        plan(&ic),
        (vec![format!("TUBE 12 EUSTON-{STOP}")], hhmm(6, 12))
    );

    // RAPTOR agrees: no train at all.
    let connections = connections();
    let from = vec!["EUSTON".to_string()];
    let to = vec![STOP.to_string()];
    let journeys = raptor_search(RaptorOptions {
        connections: &connections,
        interchange: &ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: hhmm(6, 0),
        date: date(),
        max_rounds: 4,
    });
    let quickest = journeys.iter().min_by_key(|j| j.arrival_min).unwrap();
    assert_eq!(describe(&quickest.legs), [format!("TUBE 12 EUSTON-{STOP}")]);

    // Arrive by 07:00: the same ride, left as late as 06:48.
    let journey = scan_connections_arrive_by(
        ArriveByOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: None,
            arrive_by_min: hhmm(7, 0),
            date: date(),
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(describe(&journey.legs), [format!("TUBE 12 EUSTON-{STOP}")]);
    assert_eq!(journey.arrival_min, hhmm(7, 0));
}
