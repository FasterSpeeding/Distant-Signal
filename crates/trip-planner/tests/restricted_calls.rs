//! Set-down-only (`D`) and pick-up-only (`U`) calls, end to end through
//! every search: depart-after (CSA and RAPTOR), arrive-by (the backward
//! scan and its round-based form), waypoints and pass-through vias.
//!
//! The network is built the production way, from per-call rows through
//! `schedule_query::build_connections`, around the two Caledonian Sleeper
//! Highlander workings as published on 2026-10-20:
//!
//! - C04568, Euston 21:15 -> Inverness: pick-up only at Crewe and Preston,
//!   set-down only at Stirling, Dunblane, Gleneagles, Perth, Dunkeld,
//!   Pitlochry, Blair Atholl, Dalwhinnie and Newtonmore;
//! - C04565, Inverness 20:45 -> Euston: pick-up only from Aviemore to
//!   Falkirk Grahamston, set-down only at Preston, Crewe and Birmingham
//!   International.
//!
//! Only some of those calls are kept here, with their real public times.
//! The other trains are invented: each query has one illegal journey (only
//! possible by boarding at a `D` call or alighting at a `U` one) that would
//! beat a legal alternative, and must get the legal one, or nothing.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveTime};
use schedule_query::{CallingPointForConnections, Connection, InterchangeData};
use trip_planner::{
    ArriveByOptions, JourneyLeg, RaptorOptions, ScanOptions, StagedJourney, StagedOptions, Vias,
    latest_departure, latest_departures_by_trips, raptor_arrive_by, raptor_search, raptor_staged,
    scan_connections, scan_connections_arrive_by, scan_staged, staged_arrive_by,
};

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
}

/// Which way a passenger may use a call.
#[derive(Clone, Copy)]
enum Use {
    Both,
    /// `D`: alight only.
    SetDown,
    /// `U`: board only.
    PickUp,
}

fn time(hhmm: &str) -> NaiveTime {
    NaiveTime::parse_from_str(hhmm, "%H:%M").unwrap()
}

/// One call. `arr`/`dep` are `HH:MM` (empty for none); `day` is the
/// call's day offset from the service date. An origin has no arrival and is
/// never alighted at; a terminus has no departure and is never boarded at.
fn call(tiploc: &str, arr: &str, dep: &str, day: u8, usage: Use) -> CallingPointForConnections {
    let arrival = (!arr.is_empty()).then(|| time(arr));
    let departure = (!dep.is_empty()).then(|| time(dep));
    let (board, alight) = match usage {
        Use::Both => (true, true),
        Use::SetDown => (false, true),
        Use::PickUp => (true, false),
    };
    CallingPointForConnections {
        tiploc: tiploc.to_string(),
        booked_arrival: arrival,
        booked_departure: departure,
        day_offset: day,
        can_board: board && departure.is_some(),
        can_alight: alight && arrival.is_some(),
        public_arrival: arrival,
        public_departure: departure,
    }
}

fn network() -> Vec<Connection> {
    let schedules: Vec<(&str, Vec<CallingPointForConnections>)> = vec![
        (
            "C04568",
            vec![
                call("EUSTON", "", "21:15", 0, Use::PickUp),
                call("CREWE", "23:43", "23:45", 0, Use::PickUp),
                call("PRST", "00:28", "00:30", 1, Use::PickUp),
                call("STIRLNG", "04:59", "05:00", 1, Use::SetDown),
                call("PERTH", "05:42", "05:44", 1, Use::SetDown),
                call("AVIEMRE", "07:39", "07:43", 1, Use::Both),
                call("IVRNESS", "08:45", "", 1, Use::SetDown),
            ],
        ),
        (
            "C04565",
            vec![
                call("IVRNESS", "", "20:45", 0, Use::PickUp),
                call("AVIEMRE", "21:30", "21:32", 0, Use::PickUp),
                call("PERTH", "23:24", "23:26", 0, Use::PickUp),
                call("STIRLNG", "00:10", "00:11", 1, Use::PickUp),
                call("PRST", "04:15", "04:17", 1, Use::SetDown),
                call("EUSTON", "08:00", "", 1, Use::SetDown),
            ],
        ),
        // Perth -> Aviemore the next morning: one before the sleeper calls
        // at Perth, one after.
        (
            "EARLYPA",
            vec![
                call("PERTH", "", "04:00", 1, Use::Both),
                call("AVIEMRE", "05:30", "", 1, Use::Both),
            ],
        ),
        (
            "DAYPA",
            vec![
                call("PERTH", "", "06:50", 1, Use::Both),
                call("AVIEMRE", "08:20", "", 1, Use::Both),
            ],
        ),
        // Euston -> Crewe: one before the sleeper, one after.
        (
            "AFTCR",
            vec![
                call("EUSTON", "", "20:00", 0, Use::Both),
                call("CREWE", "22:30", "", 0, Use::Both),
            ],
        ),
        (
            "EVECR",
            vec![
                call("EUSTON", "", "21:40", 0, Use::Both),
                call("CREWE", "00:00", "", 1, Use::Both),
            ],
        ),
        // Preston -> Lancaster, 15 minutes after the sleeper reaches
        // Preston; and a slower through train from Euston.
        (
            "LOCALPL",
            vec![
                call("PRST", "", "00:45", 1, Use::Both),
                call("LANCSTR", "01:05", "", 1, Use::Both),
            ],
        ),
        (
            "DAYLAN",
            vec![
                call("EUSTON", "", "22:00", 0, Use::Both),
                call("LANCSTR", "01:40", "", 1, Use::Both),
            ],
        ),
        // Aviemore -> Perth, arriving two minutes before PDUN leaves: too
        // tight for the default five-minute change.
        (
            "DAYAP",
            vec![
                call("AVIEMRE", "", "21:20", 0, Use::Both),
                call("PERTH", "23:33", "", 0, Use::Both),
            ],
        ),
        (
            "PDUN",
            vec![
                call("PERTH", "", "23:35", 0, Use::Both),
                call("DUNDETB", "00:00", "", 1, Use::Both),
            ],
        ),
        (
            "PDUN2",
            vec![
                call("PERTH", "", "00:00", 1, Use::Both),
                call("DUNDETB", "00:25", "", 1, Use::Both),
            ],
        ),
    ];
    schedule_query::build_connections(
        schedules
            .iter()
            .map(|(uid, calls)| (*uid, calls.as_slice())),
    )
}

fn interchange() -> InterchangeData {
    InterchangeData {
        modal_change: schedule_query::ModalChangeBuffer::default(),
        change_time_by_tiploc: HashMap::new(),
        tiploc_to_crs: HashMap::new(),
        crs_to_tiplocs: HashMap::new(),
        fixed_links_from_crs: HashMap::new(),
    }
}

/// `HH:MM` on day `day` as minutes from the service date's midnight.
fn min(day: u32, hhmm: &str) -> u32 {
    use chrono::Timelike;
    day * 1440 + time(hhmm).num_seconds_from_midnight() / 60
}

fn tiplocs(names: &[&str]) -> Vec<String> {
    names.iter().map(ToString::to_string).collect()
}

fn train_uids(legs: &[JourneyLeg]) -> Vec<String> {
    legs.iter()
        .filter_map(|leg| match leg {
            JourneyLeg::Train(train) => Some(train.uid.clone()),
            JourneyLeg::Transfer(_) => None,
        })
        .collect()
}

fn staged_uids(journey: &StagedJourney) -> Vec<String> {
    journey
        .parts
        .iter()
        .flat_map(|part| train_uids(&part.legs))
        .collect()
}

/// The earliest arrival from `from` to `to` leaving at or after
/// `departure`, by CSA and by RAPTOR (which must agree), with its trains.
fn depart_after(from: &str, to: &str, departure: u32) -> Option<(u32, Vec<String>)> {
    let connections = network();
    let interchange = interchange();
    let (from, to) = (tiplocs(&[from]), tiplocs(&[to]));
    let csa = scan_connections(ScanOptions {
        connections: &connections,
        interchange: &interchange,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: departure,
        date: date(),
    })
    .map(|journey| (journey.arrival_min, train_uids(&journey.legs)));
    let raptor = raptor_search(RaptorOptions {
        connections: &connections,
        interchange: &interchange,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: departure,
        date: date(),
        max_rounds: 4,
    })
    .into_iter()
    .min_by_key(|journey| journey.arrival_min)
    .map(|journey| journey.arrival_min);
    assert_eq!(
        csa.as_ref().map(|(arrival, _)| *arrival),
        raptor,
        "CSA and RAPTOR disagree: {from:?} -> {to:?}"
    );
    csa
}

/// The latest departure from `from` reaching `to` by `deadline`, by the
/// backward scan and by every round of its round-based form (whose best
/// must agree), with the trains of the journey the backward scan returns.
fn arrive_by(from: &str, to: &str, deadline: u32) -> Option<(u32, Vec<String>)> {
    let connections = network();
    let interchange = interchange();
    let (from, to) = (tiplocs(&[from]), tiplocs(&[to]));
    let options = || ArriveByOptions {
        connections: &connections,
        interchange: &interchange,
        from_tiplocs: &from,
        waypoints: &[],
        to_tiplocs: &to,
        vias: None,
        arrive_by_min: deadline,
        date: date(),
    };
    let latest = latest_departure(&options(), None, None);
    let by_trips = latest_departures_by_trips(&options(), None, None, 4)
        .into_iter()
        .flatten()
        .max();
    assert_eq!(latest, by_trips, "backward scan and rounds disagree");
    let raptor = raptor_arrive_by(options(), None, None, 4)
        .into_iter()
        .map(|journey| journey.departure_min)
        .max();
    assert_eq!(
        latest, raptor,
        "backward scan and RAPTOR arrive-by disagree"
    );
    let journey = scan_connections_arrive_by(options(), None, None);
    assert_eq!(latest, journey.as_ref().map(|j| j.departure_min));
    if let Some(journey) = &journey {
        assert!(journey.arrival_min <= deadline);
    }
    journey.map(|journey| (journey.departure_min, train_uids(&journey.legs)))
}

#[test]
fn the_sleeper_is_published_with_its_restricted_calls_on_its_connections() {
    let connections = network();
    let leg = |from: &str| {
        connections
            .iter()
            .find(|c| c.uid == "C04568" && c.from_tiploc == from)
            .unwrap()
    };
    // Euston -> Crewe: board at Euston, never alight at Crewe (U).
    assert!(leg("EUSTON").can_board && !leg("EUSTON").can_alight);
    // Preston -> Stirling: board at Preston (U), alight at Stirling (D).
    assert!(leg("PRST").can_board && leg("PRST").can_alight);
    // Stirling -> Perth: never board at Stirling (D).
    assert!(!leg("STIRLNG").can_board && leg("STIRLNG").can_alight);
}

/// Perth is set-down only northbound: the sleeper (05:44, Aviemore 07:39)
/// is never offered from Perth; the 06:50 is.
#[test]
fn depart_after_never_boards_at_a_set_down_only_call() {
    assert_eq!(
        depart_after("PERTH", "AVIEMRE", min(1, "05:00")),
        Some((min(1, "08:20"), vec!["DAYPA".to_string()]))
    );
    // Nor at Stirling, with nothing else from there.
    assert_eq!(depart_after("STIRLNG", "AVIEMRE", min(1, "00:00")), None);
}

/// Crewe is pick-up only northbound: Euston -> Crewe after 20:30 is the
/// 21:40 arriving 00:00, not the sleeper's 23:43 call.
#[test]
fn depart_after_never_alights_at_a_pick_up_only_call() {
    assert_eq!(
        depart_after("EUSTON", "CREWE", min(0, "20:30")),
        Some((min(1, "00:00"), vec!["EVECR".to_string()]))
    );
}

/// Riding through restricted calls, and the legal uses of them, still work.
#[test]
fn depart_after_rides_through_and_uses_restricted_calls_the_legal_way() {
    // Board at Euston, ride through Crewe and Preston (U), alight at Perth
    // (D).
    assert_eq!(
        depart_after("EUSTON", "PERTH", min(0, "21:00")),
        Some((min(1, "05:42"), vec!["C04568".to_string()]))
    );
    // Board at Preston (U) northbound.
    assert_eq!(
        depart_after("PRST", "STIRLNG", min(0, "23:00")),
        Some((min(1, "04:59"), vec!["C04568".to_string()]))
    );
    // Board at Perth (U) southbound, alight at Preston (D).
    assert_eq!(
        depart_after("PERTH", "PRST", min(0, "22:00")),
        Some((min(1, "04:15"), vec!["C04565".to_string()]))
    );
}

/// A change at a pick-up-only call is no change at all: Euston ->
/// Lancaster may not use the sleeper to Preston (U) and the 00:45 on; the
/// slower through train it is.
#[test]
fn depart_after_never_changes_off_a_train_at_a_pick_up_only_call() {
    assert_eq!(
        depart_after("EUSTON", "LANCSTR", min(0, "21:00")),
        Some((min(1, "01:40"), vec!["DAYLAN".to_string()]))
    );
}

/// The same southbound: the sleeper reaches Perth (U) in time for the
/// 23:35 to Dundee, but nobody may get off it there.
#[test]
fn depart_after_never_changes_at_a_pick_up_only_call_southbound() {
    assert_eq!(
        depart_after("AVIEMRE", "DUNDETB", min(0, "21:00")),
        Some((
            min(1, "00:25"),
            vec!["DAYAP".to_string(), "PDUN2".to_string()]
        ))
    );
}

/// Arrive-by mirrors each case: the latest LEGAL departure, never the
/// later illegal one.
#[test]
fn arrive_by_never_boards_at_a_set_down_only_call() {
    // By 07:50 at Aviemore: the 04:00, not the sleeper's 05:44 (D).
    assert_eq!(
        arrive_by("PERTH", "AVIEMRE", min(1, "07:50")),
        Some((min(1, "04:00"), vec!["EARLYPA".to_string()]))
    );
    assert_eq!(arrive_by("STIRLNG", "AVIEMRE", min(1, "09:00")), None);
}

#[test]
fn arrive_by_never_alights_at_a_pick_up_only_call() {
    // By 23:50 at Crewe: the 20:00, not the sleeper's 21:15 (U at Crewe).
    assert_eq!(
        arrive_by("EUSTON", "CREWE", min(0, "23:50")),
        Some((min(0, "20:00"), vec!["AFTCR".to_string()]))
    );
}

#[test]
fn arrive_by_never_changes_at_a_pick_up_only_call() {
    // By 01:20 at Lancaster only the sleeper-and-change would do: nothing.
    assert_eq!(arrive_by("EUSTON", "LANCSTR", min(1, "01:20")), None);
    // By 01:45 the through train does.
    assert_eq!(
        arrive_by("EUSTON", "LANCSTR", min(1, "01:45")),
        Some((min(0, "22:00"), vec!["DAYLAN".to_string()]))
    );
    // By 00:30 at Dundee: the 21:20 and a change, not the 21:32 sleeper.
    assert_eq!(
        arrive_by("AVIEMRE", "DUNDETB", min(1, "00:30")),
        Some((
            min(0, "21:20"),
            vec!["DAYAP".to_string(), "PDUN2".to_string()]
        ))
    );
}

#[test]
fn arrive_by_rides_through_and_uses_restricted_calls_the_legal_way() {
    assert_eq!(
        arrive_by("EUSTON", "PERTH", min(1, "06:00")),
        Some((min(0, "21:15"), vec!["C04568".to_string()]))
    );
    assert_eq!(
        arrive_by("PERTH", "PRST", min(1, "05:00")),
        Some((min(0, "23:26"), vec!["C04565".to_string()]))
    );
}

fn staged_options<'a>(
    connections: &'a [Connection],
    interchange: &'a InterchangeData,
    from: &'a [String],
    waypoints: &'a [Vec<String>],
    to: &'a [String],
    vias: Option<&'a Vias>,
) -> StagedOptions<'a> {
    StagedOptions {
        connections,
        interchange,
        from_tiplocs: from,
        waypoints,
        to_tiplocs: to,
        vias,
        date: date(),
    }
}

/// A waypoint or via at a restricted call: staying aboard through it is
/// fine (no one gets off), changing there is not.
#[test]
fn waypoints_and_vias_at_a_pick_up_only_call_never_alight_there() {
    let connections = network();
    let interchange = interchange();
    let (from, to) = (tiplocs(&["AVIEMRE"]), tiplocs(&["DUNDETB"]));
    let perth = vec![tiplocs(&["PERTH"])];
    let vias = Vias::new(&perth, HashMap::new());
    for (waypoints, vias) in [(perth.as_slice(), None), (&[][..], Some(&vias))] {
        let options = staged_options(&connections, &interchange, &from, waypoints, &to, vias);
        let forward = scan_staged(&options, min(0, "21:00"), None, None).unwrap();
        assert_eq!(forward.arrival_min, min(1, "00:25"));
        assert_eq!(staged_uids(&forward), ["DAYAP", "PDUN2"]);
        let raptor = raptor_staged(&options, min(0, "21:00"), 4, None, None);
        assert_eq!(
            raptor.iter().map(|j| j.arrival_min).min(),
            Some(min(1, "00:25"))
        );
        assert!(
            raptor
                .iter()
                .all(|j| !staged_uids(j).contains(&"C04565".to_string())),
            "the sleeper is never ridden to Perth to change there"
        );
        let backward = staged_arrive_by(
            &ArriveByOptions {
                connections: &connections,
                interchange: &interchange,
                from_tiplocs: &from,
                waypoints,
                to_tiplocs: &to,
                vias,
                arrive_by_min: min(1, "00:30"),
                date: date(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(backward.departure_min, min(0, "21:20"));
        assert_eq!(staged_uids(&backward), ["DAYAP", "PDUN2"]);
    }
}

#[test]
fn a_waypoint_at_a_restricted_call_is_satisfied_by_staying_aboard() {
    let connections = network();
    let interchange = interchange();
    // Northbound through Perth (D) to Aviemore; southbound through
    // Stirling (U) to Euston.
    for (from, waypoint, to, departure, arrival) in [
        (
            "EUSTON",
            "PERTH",
            "AVIEMRE",
            min(0, "21:00"),
            min(1, "07:39"),
        ),
        (
            "AVIEMRE",
            "STIRLNG",
            "EUSTON",
            min(0, "21:00"),
            min(1, "08:00"),
        ),
    ] {
        let (from, to) = (tiplocs(&[from]), tiplocs(&[to]));
        let waypoints = vec![tiplocs(&[waypoint])];
        let options = staged_options(&connections, &interchange, &from, &waypoints, &to, None);
        let journey = scan_staged(&options, departure, None, None).unwrap();
        assert_eq!(journey.arrival_min, arrival, "{waypoint}");
        assert_eq!(journey.changes, 0);
        assert!(journey.parts[1].continues_previous_train);
        let backward = staged_arrive_by(
            &ArriveByOptions {
                connections: &connections,
                interchange: &interchange,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: None,
                arrive_by_min: arrival,
                date: date(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(backward.arrival_min, arrival, "{waypoint} arrive-by");
        assert!(backward.parts[1].continues_previous_train);
    }
}
