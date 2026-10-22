//! Vauxhall to Euston: the ALF lists its Victoria line link once, as
//! `M=TUBE,O=EUS,D=VXH,T=22,S=0700,E=1900,P=4,R=1111110`, and the planner
//! read that one way only, so `VXH -> EUS` went by train to Waterloo, a walk
//! to Waterloo East, a train to Charing Cross and the Northern line on
//! (`M=TUBE,O=CHX,D=EUS,T=18,...`). Indexed both ways
//! (`schedule_query::index_fixed_links_both_ways`), the 22-minute Tube ride
//! wins. The links below are the real RJTTF995ALF rows (2026-10-20) for
//! these pairs; the trains are representative South Western and
//! Southeastern timings on a Wednesday.

#![expect(
    clippy::unwrap_used,
    reason = "test code: a panic is the right failure in a test"
)]

use std::collections::HashMap;

use chrono::NaiveDate;
use schedule_query::{
    Connection, FixedLink, InterchangeData, ModalChangeBuffer, index_fixed_links_both_ways,
};
use trip_planner::{
    ArriveByOptions, JourneyLeg, RaptorOptions, ScanOptions, TransferLeg, raptor_search,
    scan_connections, scan_connections_arrive_by,
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

/// One ALF row: `M=mode,O=from,D=to,T=minutes,S=valid_from,E=valid_to,R=days`.
fn row(
    mode: &str,
    from: &str,
    to: &str,
    minutes: i32,
    window: (&str, &str),
    days: &str,
) -> (String, FixedLink) {
    (
        from.to_string(),
        FixedLink {
            mode: mode.to_string(),
            to_crs: to.to_string(),
            minutes,
            valid_from: window.0.to_string(),
            valid_to: window.1.to_string(),
            days_mask: days.to_string(),
        },
    )
}

/// The ALF rows for the pairs this network uses, as published (one
/// direction each).
fn alf_rows() -> Vec<(String, FixedLink)> {
    vec![
        row("TUBE", "EUS", "VXH", 22, ("0700", "1900"), "1111110"),
        row("TUBE", "EUS", "VXH", 27, ("0700", "2359"), "0000001"),
        row("TUBE", "EUS", "VXH", 27, ("1901", "2359"), "1111110"),
        row("TUBE", "EUS", "VXH", 27, ("0529", "0659"), "1111110"),
        row("TUBE", "CHX", "EUS", 18, ("0700", "1900"), "1111110"),
        row("TUBE", "CHX", "EUS", 23, ("1901", "2359"), "1111110"),
        row("TRANSFER", "CHX", "EUS", 28, ("0001", "0529"), "1111100"),
        row("WALK", "WAT", "WAE", 1, ("0001", "2359"), "1111110"),
        row("TRANSFER", "WAE", "CHX", 10, ("0001", "2359"), "0000001"),
    ]
}

/// The pre-fix index: each row only from its own `O`.
fn one_way(rows: Vec<(String, FixedLink)>) -> HashMap<String, Vec<FixedLink>> {
    let mut by_from: HashMap<String, Vec<FixedLink>> = HashMap::new();
    for (from, link) in rows {
        by_from.entry(from).or_default().push(link);
    }
    by_from
}

/// Trains every few minutes from 10:00: Vauxhall to Waterloo (SWR) and
/// Waterloo East to Charing Cross (Southeastern).
fn network() -> Vec<Connection> {
    let mut connections = Vec::new();
    for (i, start) in (600..660).step_by(5).enumerate() {
        connections.push(conn(
            &format!("SW{i}"),
            "VAUXHLM",
            "WATRLMN",
            start + 2,
            start + 7,
        ));
        connections.push(conn(
            &format!("SE{i}"),
            "WATRLOE",
            "CHRX",
            start + 1,
            start + 3,
        ));
    }
    connections.sort_by_key(|c| c.departure_min);
    connections
}

fn interchange(fixed_links_from_crs: HashMap<String, Vec<FixedLink>>) -> InterchangeData {
    let tiplocs = [
        ("VAUXHLM", "VXH"),
        ("WATRLMN", "WAT"),
        ("WATRLOE", "WAE"),
        ("CHRX", "CHX"),
        ("EUSTON", "EUS"),
    ];
    InterchangeData {
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
    }
}

/// Wednesday 21 October 2026.
fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, 21).unwrap()
}

/// Train UIDs, and `MODE n FROM-TO` for a fixed link.
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

/// The CSA plan, RAPTOR's quickest plan and its arrival, leaving `from` at
/// 10:00 for `to`.
fn plan(ic: &InterchangeData, from: &str, to: &str) -> (Vec<String>, Vec<String>, u32) {
    let connections = network();
    let from = vec![from.to_string()];
    let to = vec![to.to_string()];
    let csa = scan_connections(ScanOptions {
        connections: &connections,
        interchange: ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: 600,
        date: date(),
    })
    .unwrap();
    let raptor = raptor_search(RaptorOptions {
        connections: &connections,
        interchange: ic,
        from_tiplocs: &from,
        to_tiplocs: &to,
        departure_min: 600,
        date: date(),
        max_rounds: 6,
    });
    let quickest = raptor.iter().min_by_key(|j| j.arrival_min).unwrap();
    (
        describe(&csa.legs),
        describe(&quickest.legs),
        csa.arrival_min,
    )
}

#[test]
fn before_the_fix_vauxhall_to_euston_went_round_by_charing_cross() {
    let ic = interchange(one_way(alf_rows()));
    let (csa, raptor, arrival) = plan(&ic, "VAUXHLM", "EUSTON");
    // 10:02 SWR, 10:07 Waterloo, walk to Waterloo East (10:08) and its
    // 5-minute change, 10:16 to Charing Cross (10:18), Northern line 18
    // minutes: 10:36.
    assert_eq!(
        csa,
        [
            "SW0",
            "WALK 1 WATRLMN-WATRLOE",
            "SE3",
            "TUBE 18 CHRX-EUSTON"
        ]
    );
    assert_eq!(arrival, 636);
    assert_eq!(raptor, csa);
}

#[test]
fn vauxhall_to_euston_takes_the_victoria_line() {
    let ic = interchange(index_fixed_links_both_ways(alf_rows()));
    let (csa, raptor, arrival) = plan(&ic, "VAUXHLM", "EUSTON");
    assert_eq!(csa, ["TUBE 22 VAUXHLM-EUSTON"]);
    assert_eq!(arrival, 622);
    assert_eq!(raptor, csa);
}

#[test]
fn arriving_by_ten_thirty_at_euston_takes_the_victoria_line_too() {
    let ic = interchange(index_fixed_links_both_ways(alf_rows()));
    let connections = network();
    let from = vec!["VAUXHLM".to_string()];
    let to = vec!["EUSTON".to_string()];
    let journey = scan_connections_arrive_by(
        ArriveByOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: None,
            arrive_by_min: 630,
            date: date(),
        },
        None,
        None,
    )
    .unwrap();
    assert_eq!(describe(&journey.legs), ["TUBE 22 VAUXHLM-EUSTON"]);
}

#[test]
fn a_pair_with_no_link_either_way_plans_as_before() {
    // No ALF row joins Vauxhall and Charing Cross, so the trains still win:
    // the Tube via Euston (22 + 18 minutes) is far slower.
    let before = plan(&interchange(one_way(alf_rows())), "VAUXHLM", "CHRX");
    let after = plan(
        &interchange(index_fixed_links_both_ways(alf_rows())),
        "VAUXHLM",
        "CHRX",
    );
    assert_eq!(before.0, ["SW0", "WALK 1 WATRLMN-WATRLOE", "SE3"]);
    assert_eq!(after, before);
}

/// DS-139: Euston to Vauxhall with a Victoria-Vauxhall link beside the
/// direct one. Chained links pay no change at the stop between them, so
/// the chain wins whenever its minutes add up to less: at the curated 11
/// minutes and a 10-minute Euston-Victoria link that is 21 against the
/// direct 22, and every search splits the one Victoria line ride at
/// Victoria. At the corrected 14 (25), all three take the direct link.
#[test]
fn a_chain_of_links_wins_only_when_its_minutes_add_up_to_less() {
    for (vic_vxh, expected) in [
        (
            11,
            vec!["TUBE 10 EUSTON-VICTRIC", "TUBE 11 VICTRIC-VAUXHLM"],
        ),
        (14, vec!["TUBE 22 EUSTON-VAUXHLM"]),
    ] {
        let mut rows = alf_rows();
        rows.push(row("TUBE", "EUS", "VIC", 10, ("0700", "1900"), "1111110"));
        rows.push(row(
            "TUBE",
            "VXH",
            "VIC",
            vic_vxh,
            ("0700", "1900"),
            "1111110",
        ));
        let mut ic = interchange(index_fixed_links_both_ways(rows));
        ic.tiploc_to_crs
            .insert("VICTRIC".to_string(), "VIC".to_string());
        ic.crs_to_tiplocs
            .insert("VIC".to_string(), vec!["VICTRIC".to_string()]);
        let (csa, raptor, arrival) = plan(&ic, "EUSTON", "VAUXHLM");
        assert_eq!(csa, expected, "Victoria-Vauxhall {vic_vxh}");
        assert_eq!(raptor, expected, "Victoria-Vauxhall {vic_vxh}");
        assert_eq!(arrival, if vic_vxh == 11 { 621 } else { 622 });
        let connections = network();
        let journey = scan_connections_arrive_by(
            ArriveByOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &["EUSTON".to_string()],
                waypoints: &[],
                to_tiplocs: &["VAUXHLM".to_string()],
                vias: None,
                arrive_by_min: 630,
                date: date(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(describe(&journey.legs), expected, "arrive-by, {vic_vxh}");
    }
}
