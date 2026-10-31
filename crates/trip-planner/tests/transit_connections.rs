//! Transit networks in the day graph (DS-164, docs/superpowers/specs/
//! 2026-10-24-transit-trip-planning-design.md): tram trips are ordinary
//! connections, a tram station is a node whose platforms are its members,
//! and a rail station and a tram station are joined by a `WALK` link whose
//! minutes are the walk plus the tram-side change buffer. None of the
//! searches changes; these tests pin that every one of them handles that
//! shape: CSA, RAPTOR, arrive-by, and the CSA/RAPTOR differential on a
//! tram-dense network.
//!
//! The fixture is hand-made, shaped on central Manchester: rail stations
//! MAN (`MNCRPIC`) and MCV (`MNCRVIC`), tram stations Piccadilly
//! (`9400ZZMAPIC`, platforms 1 and 2) and Victoria (`9400ZZMAVIC`, platforms
//! 1 and 2), the ALF's 22-minute MAN-MCV transfer, a 3-minute platform
//! change and a 2-minute tram buffer.

#![expect(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    reason = "test code: a panic is the right failure in a test; small known values"
)]

use std::collections::HashMap;

use chrono::NaiveDate;
use schedule_query::{Connection, FixedLink, InterchangeData};
use trip_planner::{
    ArriveByOptions, Journey, JourneyLeg, RaptorOptions, ScanOptions, raptor_arrive_by,
    raptor_search, scan_connections, scan_connections_arrive_by,
};

const PIC: &str = "9400ZZMAPIC";
const VIC: &str = "9400ZZMAVIC";
const PLATFORM_CHANGE: i32 = 3;

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 11, 4).unwrap()
}

fn hm(h: u32, m: u32) -> u32 {
    h * 60 + m
}

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

fn link(mode: &str, to: &str, minutes: i32) -> FixedLink {
    FixedLink {
        only_without: schedule_query::OperatorId::UNKNOWN,
        operator: schedule_query::OperatorId::UNKNOWN,
        mode: mode.to_string(),
        to_crs: to.to_string(),
        minutes,
        valid_from: "0000".to_string(),
        valid_to: "2359".to_string(),
        days_mask: "1111111".to_string(),
    }
}

fn add_node(data: &mut InterchangeData, node: &str, members: &[&str], change: Option<i32>) {
    for member in members {
        data.tiploc_to_crs
            .insert((*member).to_string(), node.to_string());
        if let Some(change) = change {
            data.change_time_by_tiploc
                .insert((*member).to_string(), change);
        }
    }
    data.crs_to_tiplocs.insert(
        node.to_string(),
        members.iter().map(|m| (*m).to_string()).collect(),
    );
}

fn add_link_both_ways(data: &mut InterchangeData, a: &str, b: &str, mode: &str, minutes: i32) {
    data.fixed_links_from_crs
        .entry(a.to_string())
        .or_default()
        .push(link(mode, b, minutes));
    data.fixed_links_from_crs
        .entry(b.to_string())
        .or_default()
        .push(link(mode, a, minutes));
}

/// The rail stations, the tram nodes and the links. `buffer` is the tram
/// side of a rail-tram change, carried on the rail-link walks (2 and 4
/// minutes of walking).
fn interchange(buffer: i32) -> InterchangeData {
    let mut data = InterchangeData::default();
    add_node(&mut data, "SPT", &["STKP"], Some(4));
    add_node(&mut data, "MAN", &["MNCRPIC"], Some(5));
    add_node(&mut data, "MCV", &["MNCRVIC"], Some(5));
    add_node(&mut data, "LDS", &["LEEDS"], Some(5));
    add_node(&mut data, "ALT", &["ALTRNHM"], Some(4));
    add_node(
        &mut data,
        PIC,
        &["9400ZZMAPIC1", "9400ZZMAPIC2"],
        Some(PLATFORM_CHANGE),
    );
    add_node(
        &mut data,
        VIC,
        &["9400ZZMAVIC1", "9400ZZMAVIC2"],
        Some(PLATFORM_CHANGE),
    );
    add_node(
        &mut data,
        "9400ZZMAALT",
        &["9400ZZMAALT1"],
        Some(PLATFORM_CHANGE),
    );
    add_link_both_ways(&mut data, "MAN", "MCV", "TRANSFER", 22);
    add_link_both_ways(&mut data, "MAN", PIC, "WALK", 2 + buffer);
    add_link_both_ways(&mut data, "MCV", VIC, "WALK", 4 + buffer);
    add_link_both_ways(&mut data, "ALT", "9400ZZMAALT", "WALK", 2 + buffer);
    data.sort_fixed_links();
    data
}

fn sorted(mut connections: Vec<Connection>) -> Vec<Connection> {
    connections.sort_by(|a, b| {
        (a.departure_min, &a.uid, &a.from_tiploc).cmp(&(b.departure_min, &b.uid, &b.from_tiploc))
    });
    connections
}

/// Stockport to MAN arriving 08:00, Blue Line trams from Piccadilly's
/// platform 1 to Victoria's every 6 minutes from 07:56, and the 08:25 and
/// 08:40 Victoria to Leeds trains.
fn cross_city() -> Vec<Connection> {
    let mut connections = vec![
        conn("C10001", "STKP", "MNCRPIC", hm(7, 50), hm(8, 0)),
        conn("C10002", "MNCRVIC", "LEEDS", hm(8, 25), hm(9, 15)),
        conn("C10003", "MNCRVIC", "LEEDS", hm(8, 40), hm(9, 30)),
    ];
    for (n, dep) in (hm(7, 56)..hm(8, 30)).step_by(6).enumerate() {
        connections.push(conn(
            &format!("metrolink:{n}"),
            "9400ZZMAPIC1",
            "9400ZZMAVIC1",
            dep,
            dep + 6,
        ));
    }
    sorted(connections)
}

fn tiplocs(names: &[&str]) -> Vec<String> {
    names.iter().map(|t| (*t).to_string()).collect()
}

fn rides(legs: &[JourneyLeg]) -> Vec<String> {
    legs.iter()
        .map(|leg| match leg {
            JourneyLeg::Train(train) => train.uid.clone(),
            JourneyLeg::Transfer(transfer) => {
                format!("{}:{}", transfer.mode, transfer.minutes)
            }
        })
        .collect()
}

fn csa(
    connections: &[Connection],
    data: &InterchangeData,
    from: &[&str],
    to: &[&str],
    at: u32,
) -> Option<Journey> {
    scan_connections(ScanOptions {
        connections,
        interchange: data,
        from_tiplocs: &tiplocs(from),
        to_tiplocs: &tiplocs(to),
        departure_min: at,
        date: date(),
    })
}

/// Rail into MAN, the tram from Piccadilly to Victoria, rail out of MCV:
/// beats the ALF transfer, and CSA and RAPTOR agree.
#[test]
fn rail_walk_tram_walk_rail_mid_journey() {
    let connections = cross_city();
    let data = interchange(2);
    let journey = csa(&connections, &data, &["STKP"], &["LEEDS"], hm(7, 45)).unwrap();
    // 08:00 at MAN + 4 (walk and buffer) + 3 (platform) = 08:07: the 08:08
    // tram, at Victoria 08:14 + 6 + 5 (MCV) = 08:25, the 08:25 train. The
    // ALF transfer would reach MCV at 08:22 + 5: only the 08:40.
    assert_eq!(
        rides(&journey.legs),
        ["C10001", "WALK:4", "metrolink:2", "WALK:6", "C10002"]
    );
    assert_eq!(journey.arrival_min, hm(9, 15));
    let raptor = raptor_search(RaptorOptions {
        connections: &connections,
        interchange: &data,
        from_tiplocs: &tiplocs(&["STKP"]),
        to_tiplocs: &tiplocs(&["LEEDS"]),
        departure_min: hm(7, 45),
        date: date(),
        max_rounds: 5,
    });
    let best = raptor.iter().min_by_key(|j| j.arrival_min).unwrap();
    assert_eq!(best.arrival_min, hm(9, 15));
    // Three rides: two changes (a tram leg counts, Q5).
    assert_eq!(best.changes, 2);
    // Without the tram (the rail-only interchange data of a request that
    // did not opt in), the trams are unreachable: the ALF transfer.
    let mut rail_only = data.clone();
    for links in rail_only.fixed_links_from_crs.values_mut() {
        links.retain(|link| link.mode != "WALK");
    }
    rail_only.fixed_links_from_crs.remove(PIC);
    rail_only.fixed_links_from_crs.remove(VIC);
    let fallback = csa(&connections, &rail_only, &["STKP"], &["LEEDS"], hm(7, 45)).unwrap();
    assert_eq!(rides(&fallback.legs), ["C10001", "TRANSFER:22", "C10003"]);
}

/// A change between two platforms of one tram station costs the network's
/// change time, and a change between two trams owes no tram buffer.
#[test]
fn a_platform_change_is_charged_at_the_network_change_time() {
    let data = interchange(2);
    let connections = sorted(vec![
        conn(
            "metrolink:0",
            "9400ZZMAALT1",
            "9400ZZMAPIC1",
            hm(8, 0),
            hm(8, 10),
        ),
        // Platform 2: 2 minutes later (too soon), then 3.
        conn(
            "metrolink:1",
            "9400ZZMAPIC2",
            "9400ZZMAVIC2",
            hm(8, 12),
            hm(8, 18),
        ),
        conn(
            "metrolink:2",
            "9400ZZMAPIC2",
            "9400ZZMAVIC2",
            hm(8, 13),
            hm(8, 19),
        ),
    ]);
    let journey = csa(
        &connections,
        &data,
        &["9400ZZMAALT1"],
        &["9400ZZMAVIC1", "9400ZZMAVIC2"],
        hm(7, 55),
    )
    .unwrap();
    assert_eq!(rides(&journey.legs), ["metrolink:0", "metrolink:2"]);
    assert_eq!(journey.arrival_min, hm(8, 19));
    // The same platform: a fresh boarding costs the change time too.
    let same = sorted(vec![
        conn(
            "metrolink:0",
            "9400ZZMAALT1",
            "9400ZZMAPIC1",
            hm(8, 0),
            hm(8, 10),
        ),
        conn(
            "metrolink:1",
            "9400ZZMAPIC1",
            "9400ZZMAVIC1",
            hm(8, 12),
            hm(8, 18),
        ),
        conn(
            "metrolink:2",
            "9400ZZMAPIC1",
            "9400ZZMAVIC1",
            hm(8, 13),
            hm(8, 19),
        ),
    ]);
    let journey = csa(
        &same,
        &data,
        &["9400ZZMAALT1"],
        &["9400ZZMAVIC1"],
        hm(7, 55),
    )
    .unwrap();
    assert_eq!(rides(&journey.legs), ["metrolink:0", "metrolink:2"]);
}

/// A trip's first stop is set-down-only in the GTFS sense (no drop-off)
/// and its last pick-up-only: a passenger cannot alight at the first or
/// board at the last; and an intermediate drop-off-forbidden stop is ridden
/// through.
#[test]
fn set_down_only_and_pick_up_only_stops_are_honoured() {
    // No walks: otherwise Victoria, MCV, the ALF transfer and MAN lead back
    // to Piccadilly on foot.
    let mut data = interchange(2);
    data.fixed_links_from_crs.clear();
    let mut through = conn(
        "metrolink:0",
        "9400ZZMAALT1",
        "9400ZZMAPIC1",
        hm(8, 0),
        hm(8, 10),
    );
    through.can_alight = false;
    let onward = conn(
        "metrolink:0",
        "9400ZZMAPIC1",
        "9400ZZMAVIC1",
        hm(8, 10),
        hm(8, 16),
    );
    let connections = sorted(vec![through, onward]);
    // Piccadilly cannot be alighted at ...
    assert!(
        csa(
            &connections,
            &data,
            &["9400ZZMAALT1"],
            &["9400ZZMAPIC1"],
            hm(7, 55)
        )
        .is_none()
    );
    // ... but the tram is ridden through it to Victoria.
    let journey = csa(
        &connections,
        &data,
        &["9400ZZMAALT1"],
        &["9400ZZMAVIC1"],
        hm(7, 55),
    )
    .unwrap();
    assert_eq!(rides(&journey.legs), ["metrolink:0"]);
    // A stop with no pick-up cannot be boarded at.
    let mut last = conn(
        "metrolink:1",
        "9400ZZMAVIC1",
        "9400ZZMAPIC1",
        hm(8, 20),
        hm(8, 26),
    );
    last.can_board = false;
    assert!(
        csa(
            &sorted(vec![last]),
            &data,
            &["9400ZZMAVIC1"],
            &["9400ZZMAPIC1"],
            hm(8, 0)
        )
        .is_none()
    );
}

/// The tram buffer on each side of a rail-tram change (it rides on the
/// rail-link walk, so a tram-to-tram change never pays it), and arrive-by
/// mirrors it.
#[test]
fn the_tram_buffer_applies_on_each_side_and_arrive_by_mirrors_it() {
    // Rail to tram: 08:00 at MAN + 2 (walk) + 3 (platform) = 08:05 makes
    // the 08:05 tram; with the 2-minute buffer only the 08:08.
    let onto = sorted(vec![
        conn("C10001", "STKP", "MNCRPIC", hm(7, 50), hm(8, 0)),
        conn(
            "metrolink:0",
            "9400ZZMAPIC1",
            "9400ZZMAVIC1",
            hm(8, 5),
            hm(8, 11),
        ),
        conn(
            "metrolink:1",
            "9400ZZMAPIC1",
            "9400ZZMAVIC1",
            hm(8, 8),
            hm(8, 14),
        ),
    ]);
    let journey = csa(
        &onto,
        &interchange(0),
        &["STKP"],
        &["9400ZZMAVIC1"],
        hm(7, 45),
    )
    .unwrap();
    assert_eq!(rides(&journey.legs), ["C10001", "WALK:2", "metrolink:0"]);
    let journey = csa(
        &onto,
        &interchange(2),
        &["STKP"],
        &["9400ZZMAVIC1"],
        hm(7, 45),
    )
    .unwrap();
    assert_eq!(rides(&journey.legs), ["C10001", "WALK:4", "metrolink:1"]);

    // Tram to rail: the 08:02 tram reaches Victoria at 08:08, + 4 (walk) +
    // 5 (MCV) = 08:17: the 08:17 train without the buffer, the 08:25 with.
    let mut tight = cross_city();
    tight.push(conn("C10004", "MNCRVIC", "LEEDS", hm(8, 17), hm(9, 5)));
    let tight = sorted(tight);
    let from_tram = |buffer: i32| {
        csa(
            &tight,
            &interchange(buffer),
            &["9400ZZMAPIC1"],
            &["LEEDS"],
            hm(8, 0),
        )
        .unwrap()
    };
    assert_eq!(from_tram(0).arrival_min, hm(9, 5));
    assert_eq!(from_tram(2).arrival_min, hm(9, 15));

    // Arrive-by mirrors it: the latest tram from Piccadilly that still
    // makes the 08:25 into Leeds (09:15) leaves by 08:10 less the buffer.
    let connections = cross_city();
    for (buffer, expected) in [(0, hm(8, 8)), (3, hm(8, 2)), (10, hm(7, 56))] {
        let data = interchange(buffer);
        let backward = scan_connections_arrive_by(
            ArriveByOptions {
                connections: &connections,
                interchange: &data,
                from_tiplocs: &tiplocs(&["9400ZZMAPIC1"]),
                waypoints: &[],
                vias: None,
                to_tiplocs: &tiplocs(&["LEEDS"]),
                arrive_by_min: hm(9, 15),
                date: date(),
            },
            None,
            None,
        )
        .unwrap();
        assert_eq!(backward.departure_min, expected, "buffer {buffer}");
        let forward = csa(
            &connections,
            &data,
            &["9400ZZMAPIC1"],
            &["LEEDS"],
            backward.departure_min,
        )
        .unwrap();
        assert_eq!(forward.arrival_min, hm(9, 15), "buffer {buffer}");
        // One minute later, the 08:25 is missed.
        let later = csa(
            &connections,
            &data,
            &["9400ZZMAPIC1"],
            &["LEEDS"],
            backward.departure_min + 1,
        )
        .unwrap();
        assert!(later.arrival_min > hm(9, 15), "buffer {buffer}");
    }
}

/// A deterministic tram-dense network: four lines of trams every 4 to 8
/// minutes over twelve two-platform stations through the day, some
/// set-down-only calls, and rail trains linking the rail ends.
fn dense() -> (Vec<Connection>, InterchangeData) {
    let mut data = InterchangeData::default();
    let stations: Vec<String> = (0..12).map(|i| format!("9400ZZT{i:02}")).collect();
    for station in &stations {
        add_node(
            &mut data,
            station,
            &[&format!("{station}1"), &format!("{station}2")],
            Some(PLATFORM_CHANGE),
        );
    }
    for crs in ["RA", "RB", "RC"] {
        add_node(
            &mut data,
            &format!("{crs}X"),
            &[&format!("R{crs}")],
            Some(5),
        );
    }
    add_link_both_ways(&mut data, "RAX", &stations[0], "WALK", 4);
    add_link_both_ways(&mut data, "RBX", &stations[6], "WALK", 5);
    add_link_both_ways(&mut data, "RCX", &stations[11], "WALK", 6);
    add_link_both_ways(&mut data, "RAX", "RCX", "TRANSFER", 40);
    data.sort_fixed_links();

    let lines: [&[usize]; 4] = [
        &[0, 1, 2, 3, 4, 5],
        &[5, 6, 7, 8],
        &[2, 9, 6, 10, 11],
        &[3, 7, 10],
    ];
    let mut connections = Vec::new();
    let mut seed: u32 = 12_345;
    let mut next = |modulo: u32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 16) % modulo
    };
    let mut trip = 0;
    for (line_index, line) in lines.iter().enumerate() {
        for direction in 0..2 {
            let stops: Vec<usize> = if direction == 0 {
                line.to_vec()
            } else {
                line.iter().rev().copied().collect()
            };
            let mut start = hm(6, 0) + next(5);
            while start < hm(10, 0) {
                let mut time = start;
                for pair in stops.windows(2) {
                    let run = 2 + next(3);
                    let mut c = conn(
                        &format!("metrolink:{trip}"),
                        &format!("{}{}", stations[pair[0]], direction + 1),
                        &format!("{}{}", stations[pair[1]], direction + 1),
                        time,
                        time + run,
                    );
                    // Now and then a stop where nobody may get off.
                    c.can_alight = next(11) != 0;
                    connections.push(c);
                    time += run;
                }
                trip += 1;
                start += 4 + next(5) + line_index as u32;
            }
        }
    }
    for (n, dep) in (hm(6, 10)..hm(10, 0)).step_by(17).enumerate() {
        connections.push(conn(&format!("C2{n:04}"), "RRA", "RRB", dep, dep + 12));
        connections.push(conn(&format!("C3{n:04}"), "RRB", "RRC", dep + 3, dep + 25));
    }
    (sorted(connections), data)
}

#[test]
fn csa_and_raptor_agree_on_a_tram_dense_network() {
    let (connections, data) = dense();
    let mut origins: Vec<String> = data.crs_to_tiplocs.keys().cloned().collect();
    origins.sort();
    let mut checked = 0;
    for from in &origins {
        for to in &origins {
            if from == to {
                continue;
            }
            for start in [hm(6, 0), hm(7, 13), hm(8, 47)] {
                let from_tiplocs = data.crs_to_tiplocs[from].clone();
                let to_tiplocs = data.crs_to_tiplocs[to].clone();
                let csa = scan_connections(ScanOptions {
                    connections: &connections,
                    interchange: &data,
                    from_tiplocs: &from_tiplocs,
                    to_tiplocs: &to_tiplocs,
                    departure_min: start,
                    date: date(),
                });
                let raptor = raptor_search(RaptorOptions {
                    connections: &connections,
                    interchange: &data,
                    from_tiplocs: &from_tiplocs,
                    to_tiplocs: &to_tiplocs,
                    departure_min: start,
                    date: date(),
                    max_rounds: 10,
                });
                let raptor_best = raptor.iter().map(|j| j.arrival_min).min();
                assert_eq!(
                    csa.as_ref().map(|j| j.arrival_min),
                    raptor_best,
                    "{from} to {to} from {start}"
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 400);
}

/// Arrive-by against the forward search on the same network: the latest
/// departure that makes a forward arrival never leaves earlier than the
/// forward journey did, and planning forward from it arrives in time.
#[test]
fn arrive_by_agrees_with_the_forward_search_on_trams() {
    let (connections, data) = dense();
    let mut nodes: Vec<String> = data.crs_to_tiplocs.keys().cloned().collect();
    nodes.sort();
    let mut checked = 0;
    for from in nodes.iter().step_by(2) {
        for to in nodes.iter().skip(1).step_by(3) {
            if from == to {
                continue;
            }
            let from_tiplocs = data.crs_to_tiplocs[from].clone();
            let to_tiplocs = data.crs_to_tiplocs[to].clone();
            let Some(forward) = scan_connections(ScanOptions {
                connections: &connections,
                interchange: &data,
                from_tiplocs: &from_tiplocs,
                to_tiplocs: &to_tiplocs,
                departure_min: hm(7, 30),
                date: date(),
            }) else {
                continue;
            };
            let options = ArriveByOptions {
                connections: &connections,
                interchange: &data,
                from_tiplocs: &from_tiplocs,
                waypoints: &[],
                vias: None,
                to_tiplocs: &to_tiplocs,
                arrive_by_min: forward.arrival_min,
                date: date(),
            };
            let backward = scan_connections_arrive_by(
                ArriveByOptions {
                    connections: &connections,
                    interchange: &data,
                    from_tiplocs: &from_tiplocs,
                    waypoints: &[],
                    vias: None,
                    to_tiplocs: &to_tiplocs,
                    arrive_by_min: forward.arrival_min,
                    date: date(),
                },
                None,
                None,
            )
            .unwrap_or_else(|| {
                panic!("{from} to {to}: nothing arrives by {}", forward.arrival_min)
            });
            assert!(
                backward.arrival_min <= forward.arrival_min,
                "{from} to {to}"
            );
            assert!(
                backward.departure_min >= forward.departure_min,
                "{from} to {to}"
            );
            let again = scan_connections(ScanOptions {
                connections: &connections,
                interchange: &data,
                from_tiplocs: &from_tiplocs,
                to_tiplocs: &to_tiplocs,
                departure_min: backward.departure_min,
                date: date(),
            })
            .unwrap();
            assert!(again.arrival_min <= forward.arrival_min, "{from} to {to}");
            // RAPTOR's arrive-by finds one at least as late.
            let raptor = raptor_arrive_by(options, None, None, 8);
            assert!(
                raptor
                    .iter()
                    .map(|j| j.departure_min)
                    .max()
                    .is_some_and(|latest| latest >= backward.departure_min),
                "{from} to {to}"
            );
            checked += 1;
        }
    }
    assert!(checked > 10, "{checked}");
}

#[test]
fn transit_uids_never_collide_with_rail_uids() {
    let (connections, _) = dense();
    let mut seen: HashMap<&str, u32> = HashMap::new();
    for connection in &connections {
        *seen.entry(connection.uid.as_str()).or_default() += 1;
    }
    assert!(seen.keys().any(|uid| uid.starts_with("metrolink:")));
    assert!(
        seen.keys()
            .filter(|uid| !uid.starts_with("metrolink:"))
            .all(|uid| !uid.contains(':'))
    );
}
