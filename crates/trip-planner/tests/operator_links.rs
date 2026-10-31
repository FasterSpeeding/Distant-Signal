//! Operator filters on links (planner operator filtering spec 2026-10-31,
//! §5.1): one operator, every way of riding it. Euston to Victoria is the
//! Tube (10 minutes) or its Tube-speed `TRANSFER` twin (12), both London
//! Underground; a stand-in walk (25 minutes) is there for a filter that
//! rules the Tube out, and for no other search. Avanti runs Manchester to
//! Euston, Southern Victoria to Brighton.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

use chrono::NaiveDate;
use schedule_query::{
    Connection, FixedLink, InterchangeData, ModalChangeBuffer, OperatorId, OperatorMask,
    index_fixed_links_both_ways,
};
use trip_planner::{
    ArriveByOptions, JourneyLeg, OperatorRule, RaptorOptions, Restrictions, ScanOptions,
    raptor_search_restricted, scan_connections_arrive_by, scan_connections_restricted,
};

fn atoc(code: &str) -> OperatorId {
    OperatorId::from_atoc(code).unwrap()
}

fn conn(uid: &str, operator: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
    Connection {
        uid: uid.into(),
        from_tiploc: from.into(),
        to_tiploc: to.into(),
        departure_min: dep,
        arrival_min: arr,
        working_departure_min: dep,
        working_arrival_min: arr,
        can_board: true,
        can_alight: true,
        operator: atoc(operator),
    }
}

fn link(
    mode: &str,
    to: &str,
    minutes: i32,
    operator: OperatorId,
    only_without: OperatorId,
) -> FixedLink {
    FixedLink {
        mode: mode.to_string(),
        to_crs: to.to_string(),
        minutes,
        valid_from: "0000".to_string(),
        valid_to: "2359".to_string(),
        days_mask: "1111111".to_string(),
        operator,
        only_without,
    }
}

/// 08:00 Avanti MAN-EUS (10:00), Southern VIC-BTN at 10:20 (11:20), 10:40
/// (11:40) and 11:00 (12:00).
fn network() -> Vec<Connection> {
    let mut connections = vec![
        conn("VT1", "VT", "MNCRPIC", "EUSTON", 480, 600),
        conn("SN1", "SN", "VICTRIC", "BRGHTN", 620, 680),
        conn("SN2", "SN", "VICTRIC", "BRGHTN", 640, 700),
        conn("SN3", "SN", "VICTRIC", "BRGHTN", 660, 720),
    ];
    connections.sort_by_key(|c| c.departure_min);
    connections
}

fn interchange(with_tube: bool, with_stand_in: bool) -> InterchangeData {
    let lu = OperatorId::LONDON_UNDERGROUND;
    let none = OperatorId::UNKNOWN;
    let mut rows = Vec::new();
    if with_tube {
        rows.push(("EUS".to_string(), link("TUBE", "VIC", 10, lu, none)));
        rows.push(("EUS".to_string(), link("TRANSFER", "VIC", 12, lu, none)));
    }
    if with_stand_in {
        rows.push(("EUS".to_string(), link("WALK", "VIC", 25, none, lu)));
    }
    let tiplocs = [
        ("MNCRPIC", "MAN"),
        ("EUSTON", "EUS"),
        ("VICTRIC", "VIC"),
        ("BRGHTN", "BTN"),
    ];
    InterchangeData {
        modal_change: ModalChangeBuffer::default(),
        change_time_by_tiploc: tiplocs.iter().map(|(t, _)| ((*t).to_string(), 5)).collect(),
        tiploc_to_crs: tiplocs
            .iter()
            .map(|(t, c)| ((*t).to_string(), (*c).to_string()))
            .collect(),
        crs_to_tiplocs: tiplocs
            .iter()
            .map(|(t, c)| ((*c).to_string(), vec![(*t).to_string()]))
            .collect(),
        fixed_links_from_crs: index_fixed_links_both_ways(rows),
    }
}

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()
}

fn describe(legs: &[JourneyLeg]) -> Vec<String> {
    legs.iter()
        .map(|leg| match leg {
            JourneyLeg::Train(train) => train.uid.clone(),
            JourneyLeg::Transfer(transfer) => format!("{} {}", transfer.mode, transfer.minutes),
        })
        .collect()
}

/// CSA depart-after 08:00, CSA arrive-by 12:00 and RAPTOR's quickest, each
/// as its legs; `None` for nothing found.
fn plans(ic: &InterchangeData, r: Option<&Restrictions>) -> [Option<Vec<String>>; 3] {
    let connections = network();
    let from = vec!["MNCRPIC".to_string()];
    let to = vec!["BRGHTN".to_string()];
    let csa = scan_connections_restricted(
        ScanOptions {
            connections: &connections,
            interchange: ic,
            from_tiplocs: &from,
            to_tiplocs: &to,
            departure_min: 480,
            date: date(),
        },
        None,
        r,
    )
    .map(|j| describe(&j.legs));
    let arrive = scan_connections_arrive_by(
        ArriveByOptions {
            connections: &connections,
            interchange: ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: None,
            arrive_by_min: 720,
            date: date(),
        },
        None,
        r,
    )
    .map(|j| describe(&j.legs));
    let raptor = raptor_search_restricted(
        RaptorOptions {
            connections: &connections,
            interchange: ic,
            from_tiplocs: &from,
            to_tiplocs: &to,
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        },
        None,
        r,
    )
    .into_iter()
    .min_by_key(|j| j.arrival_min)
    .map(|j| describe(&j.legs));
    [csa, arrive, raptor]
}

fn rule(r: OperatorRule) -> Restrictions {
    Restrictions::default().with_operator_rule(r)
}

fn mask(codes: &[&str]) -> OperatorMask {
    OperatorMask::of(codes.iter().map(|c| OperatorId::from_code(c).unwrap()))
}

fn all(plan: &[&str]) -> [Option<Vec<String>>; 3] {
    let plan: Vec<String> = plan.iter().map(|s| (*s).to_string()).collect();
    [Some(plan.clone()), Some(plan.clone()), Some(plan)]
}

#[test]
fn unfiltered_plans_take_the_tube_and_never_the_stand_in() {
    let ic = interchange(true, true);
    assert_eq!(plans(&ic, None), all(&["VT1", "TUBE 10", "SN1"]));
    // Without the Tube, the stand-in walk alone is never used.
    let ic = interchange(false, true);
    assert_eq!(plans(&ic, None), [None, None, None]);
    assert_eq!(
        plans(&ic, Some(&Restrictions::default())),
        [None, None, None]
    );
}

#[test]
fn only_without_london_underground_walks_instead() {
    let ic = interchange(true, true);
    let r = rule(OperatorRule::only(mask(&["VT", "SN"])));
    // 10:00 at Euston, 25 minutes on foot, 5 to change: the 10:40.
    assert_eq!(plans(&ic, Some(&r)), all(&["VT1", "WALK 25", "SN2"]));
}

#[test]
fn only_without_london_underground_and_no_walk_finds_nothing() {
    let ic = interchange(true, false);
    let r = rule(OperatorRule::only(mask(&["VT", "SN"])));
    assert_eq!(plans(&ic, Some(&r)), [None, None, None]);
}

#[test]
fn only_with_london_underground_takes_the_tube() {
    let ic = interchange(true, true);
    let r = rule(OperatorRule::only(mask(&["VT", "SN", "LT"])));
    assert_eq!(plans(&ic, Some(&r)), all(&["VT1", "TUBE 10", "SN1"]));
}

#[test]
fn avoiding_london_underground_drops_the_tube_and_its_transfer_twin() {
    let ic = interchange(true, true);
    let r = rule(OperatorRule::avoid(mask(&["LT"])));
    assert_eq!(plans(&ic, Some(&r)), all(&["VT1", "WALK 25", "SN2"]));
}

#[test]
fn a_cif_trains_only_rule_keeps_the_tube_and_skips_the_stand_in() {
    let ic = interchange(true, true);
    let r = rule(OperatorRule::only(mask(&["VT", "SN"])).cif_trains_only());
    assert_eq!(plans(&ic, Some(&r)), all(&["VT1", "TUBE 10", "SN1"]));
}

#[test]
fn trains_of_other_operators_are_still_blocked_with_links_allowed() {
    let ic = interchange(true, true);
    let r = rule(OperatorRule::only(mask(&["VT", "LT"])));
    assert_eq!(plans(&ic, Some(&r)), [None, None, None]);
}
