//! Latency of the arrive-by and restricted searches against the existing
//! depart-after ones, on a synthetic whole-day network somewhat above
//! production scale (~28k trains, ~400k connections, 2,000 stations;
//! production is ~25k schedules and ~290k connections). Ignored by default;
//! run in release mode:
//!
//! ```text
//! cargo test --release -p trip-planner --test bench_arrive_by -- --ignored --nocapture
//! ```
//!
//! Synthetic because the searches' cost depends on the connection count
//! and network shape, not on which stations are real; the route-level
//! overhead (graph cache, leg details, live reads) is unchanged by these
//! features and is measured in the live-overlay design doc.

#![expect(
    clippy::cast_possible_truncation,
    clippy::print_stdout,
    reason = "test code: casts of small known test values; test diagnostics"
)]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use schedule_query::{Connection, InterchangeData};
use trip_planner::{
    ArriveByOptions, RaptorOptions, Restrictions, ScanOptions, raptor_arrive_by,
    raptor_search_restricted, scan_connections_arrive_by, scan_connections_restricted,
};

struct Rng(u64);

impl Rng {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

const STATIONS: u64 = 2000;
/// Hubs every line visits one of, so the network is connected.
const HUBS: u64 = 40;

fn tiploc(i: u64) -> String {
    format!("S{i:04}")
}

fn network() -> (Vec<Connection>, InterchangeData) {
    let mut rng = Rng(0x00de_cade);
    let mut connections = Vec::new();
    let mut trains = 0;
    for line in 0..700 {
        let stops = 6 + rng.next(20) as usize;
        let mut route: Vec<u64> = (0..stops).map(|_| rng.next(STATIONS)).collect();
        route[stops / 2] = rng.next(HUBS);
        route[0] = rng.next(HUBS);
        let runs: Vec<u32> = (0..stops - 1).map(|_| 3 + rng.next(15) as u32).collect();
        let headway = [30, 60, 60, 90, 120][rng.next(5) as usize];
        for direction in 0..2 {
            let ordered: Vec<u64> = if direction == 0 {
                route.clone()
            } else {
                route.iter().rev().copied().collect()
            };
            let mut start = 300 + rng.next(u64::from(headway)) as u32;
            while start < 1440 {
                trains += 1;
                let uid = format!("L{line}D{direction}T{trains}");
                let mut time = start;
                for (hop, pair) in ordered.windows(2).enumerate() {
                    let run = runs[if direction == 0 { hop } else { stops - 2 - hop }];
                    connections.push(Connection {
                        uid: uid.clone(),
                        from_tiploc: tiploc(pair[0]),
                        to_tiploc: tiploc(pair[1]),
                        departure_min: time,
                        arrival_min: time + run,
                        working_departure_min: time,
                        working_arrival_min: time + run,
                        can_board: true,
                        can_alight: true,
                    });
                    time += run + 1;
                }
                start += headway;
            }
        }
    }
    connections.sort_by(|a, b| {
        (a.departure_min, &a.uid, &a.from_tiploc).cmp(&(b.departure_min, &b.uid, &b.from_tiploc))
    });
    let mut interchange = InterchangeData {
        modal_change: schedule_query::ModalChangeBuffer::default(),
        change_time_by_tiploc: HashMap::new(),
        tiploc_to_crs: HashMap::new(),
        crs_to_tiplocs: HashMap::new(),
        fixed_links_from_crs: HashMap::new(),
    };
    for i in 0..STATIONS {
        let crs = format!("C{i:04}");
        interchange.tiploc_to_crs.insert(tiploc(i), crs.clone());
        interchange.crs_to_tiplocs.insert(crs, vec![tiploc(i)]);
        interchange
            .change_time_by_tiploc
            .insert(tiploc(i), if i < HUBS { 8 } else { 4 });
    }
    println!("{trains} trains, {} connections", connections.len());
    (connections, interchange)
}

fn median(mut f: impl FnMut() -> usize) -> (Duration, usize) {
    let mut samples = Vec::new();
    let mut found = 0;
    for _ in 0..5 {
        let started = Instant::now();
        found = f();
        samples.push(started.elapsed());
    }
    samples.sort();
    (samples[2], found)
}

#[test]
#[ignore = "benchmark; see the module doc"]
fn bench_arrive_by_and_restrictions() {
    let (connections, interchange) = network();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    let mut rng = Rng(7);
    // Avoid two hubs, both ways it can mean something.
    let avoided = [tiploc(1), tiploc(2)];
    let no_change = Restrictions::new(avoided.clone(), [], HashMap::new());
    let no_call = Restrictions::new([], avoided.clone(), HashMap::new());
    let mut rows = Vec::new();
    for _ in 0..10 {
        let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
        let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
        let scan = |departure_min, restrictions: Option<&Restrictions>| {
            scan_connections_restricted(
                ScanOptions {
                    connections: &connections,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    to_tiplocs: &to,
                    departure_min,
                    date,
                },
                None,
                restrictions,
            )
        };
        let arrive = |deadline| ArriveByOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: None,
            arrive_by_min: deadline,
            date,
        };
        let raptor = |restrictions: Option<&Restrictions>| {
            raptor_search_restricted(
                RaptorOptions {
                    connections: &connections,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    to_tiplocs: &to,
                    departure_min: 480,
                    date,
                    max_rounds: 4,
                },
                None,
                restrictions,
            )
            .len()
        };
        let times = [
            median(|| usize::from(scan(480, None).is_some())),
            median(|| usize::from(scan_connections_arrive_by(arrive(720), None, None).is_some())),
            median(|| usize::from(scan(480, Some(&no_change)).is_some())),
            median(|| usize::from(scan(480, Some(&no_call)).is_some())),
            median(|| {
                usize::from(scan_connections_arrive_by(arrive(720), None, Some(&no_call)).is_some())
            }),
            median(|| raptor(None)),
            median(|| raptor_arrive_by(arrive(720), None, None, 4).len()),
            median(|| raptor(Some(&no_call))),
            median(|| raptor_arrive_by(arrive(720), None, Some(&no_call), 4).len()),
        ];
        println!(
            "{} -> {}: {}",
            from[0],
            to[0],
            times
                .iter()
                .map(|(t, n)| format!("{t:>7.1?}/{n}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        rows.push(times.map(|(t, _)| t));
    }
    let labels = [
        "csa depart 08:00",
        "csa arriveBy 12:00",
        "csa avoidChange",
        "csa avoidStop",
        "csa arriveBy+avoidStop",
        "raptor depart 08:00",
        "raptor arriveBy 12:00",
        "raptor avoidStop",
        "raptor arriveBy+avoidStop",
    ];
    for (column, label) in labels.iter().enumerate() {
        let mut values: Vec<Duration> = rows.iter().map(|row| row[column]).collect();
        values.sort();
        println!(
            "{label:28} median {:>8.1?}  max {:>8.1?}",
            values[values.len() / 2],
            values[values.len() - 1]
        );
    }
}

/// Cost of the joint (staged) waypoint search by number of waypoints, the
/// input to `/Trips/plan`'s `MAX_WAYPOINTS`. Waypoints are hubs, so most
/// pairs are a change or two apart. Each row: CSA depart-after, CSA
/// arrive-by (deadline 23:00), RAPTOR over 6 rounds (maxChanges 4), and the
/// old chained CSA (one search per segment) for comparison.
#[test]
#[ignore = "benchmark; see the module doc"]
#[expect(
    clippy::too_many_lines,
    reason = "one benchmark table, read top to bottom"
)]
fn bench_waypoints() {
    use trip_planner::{
        StagedOptions, latest_departures_by_trips, raptor_staged, scan_staged, staged_arrive_by,
    };
    let (connections, interchange) = network();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    let mut rng = Rng(11);
    for count in [0usize, 2, 4, 8, 12, 20] {
        let mut rows = Vec::new();
        for _ in 0..3 {
            let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
            let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
            let mut waypoints: Vec<Vec<String>> = Vec::new();
            while waypoints.len() < count {
                let hub = vec![tiploc(rng.next(HUBS))];
                if waypoints.last() != Some(&hub) {
                    waypoints.push(hub);
                }
            }
            let staged = StagedOptions {
                connections: &connections,
                interchange: &interchange,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: None,
                date,
            };
            let arrive = ArriveByOptions {
                connections: &connections,
                interchange: &interchange,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: None,
                arrive_by_min: 1380,
                date,
            };
            let chained = || {
                let mut stops = vec![from.clone()];
                stops.extend(waypoints.iter().cloned());
                stops.push(to.clone());
                let mut ready = Some(360u32);
                for pair in stops.windows(2) {
                    ready = ready.and_then(|start| {
                        scan_connections_restricted(
                            ScanOptions {
                                connections: &connections,
                                interchange: &interchange,
                                from_tiplocs: &pair[0],
                                to_tiplocs: &pair[1],
                                departure_min: start,
                                date,
                            },
                            None,
                            None,
                        )
                        .map(|j| j.arrival_min + 5)
                    });
                }
                usize::from(ready.is_some())
            };
            let once = |f: &dyn Fn() -> usize| {
                let started = Instant::now();
                let found = f();
                (started.elapsed(), found)
            };
            rows.push([
                once(&|| usize::from(scan_staged(&staged, 360, None, None).is_some())),
                once(&|| usize::from(staged_arrive_by(&arrive, None, None).is_some())),
                once(&|| raptor_staged(&staged, 360, 6, None, None).len()),
                once(&|| {
                    latest_departures_by_trips(&arrive, None, None, 6)
                        .iter()
                        .flatten()
                        .count()
                }),
                once(&chained),
            ]);
            println!(
                "  {count} waypoints, pair done: {:?}",
                rows.last().unwrap().map(|(t, n)| (t, n))
            );
        }
        let column = |i: usize| {
            let mut values: Vec<Duration> = rows.iter().map(|row| row[i].0).collect();
            values.sort();
            let found: usize = rows.iter().map(|row| usize::from(row[i].1 > 0)).sum();
            format!(
                "{:>8.1?} (max {:>8.1?}, {found}/3 found)",
                values[1], values[2]
            )
        };
        println!(
            "{count:2} waypoints: csa {} | arriveBy {} | raptor {} | arriveBy rounds {} | chained csa {}",
            column(0),
            column(1),
            column(2),
            column(3),
            column(4)
        );
    }
}

/// Vias on hubs `S0001..S0003`, and, for every 7th line, a synthetic pass
/// of one of them on every connection of its trains (so passing is
/// exercised, not only calls).
fn bench_vias(connections: &[Connection], count: usize) -> trip_planner::Vias {
    use trip_planner::{PassSpan, Vias};
    let targets: Vec<Vec<String>> = (1..=count as u64).map(|i| vec![tiploc(i)]).collect();
    let mut spans: HashMap<String, Vec<PassSpan>> = HashMap::new();
    for c in connections {
        let Some(line) = c.uid[1..]
            .split('D')
            .next()
            .and_then(|n| n.parse::<u64>().ok())
        else {
            continue;
        };
        if count == 0 || !line.is_multiple_of(7) {
            continue;
        }
        spans.entry(c.uid.clone()).or_default().push(PassSpan {
            from_tiploc: c.from_tiploc.clone(),
            to_tiploc: c.to_tiploc.clone(),
            departure_min: c.departure_min,
            passed: vec![tiploc(1 + line % count as u64)],
        });
    }
    Vias::new(&targets, spans)
}

/// The cost of raising `maxChanges` from 4 to 6 (RAPTOR rounds
/// `max_changes + 2`: 6 -> 8) with and without waypoints and pass-through
/// vias, depart-after and arrive-by; CSA (`fastest`, independent of
/// `maxChanges`) for reference. Three OD pairs per row: median and max.
#[test]
#[ignore = "benchmark; see the module doc"]
fn bench_max_changes_and_vias() {
    use trip_planner::{
        StagedOptions, latest_departures_by_trips, raptor_staged, scan_staged, staged_arrive_by,
    };
    let (connections, interchange) = network();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    println!(
        "{:>3} wp {:>2} via | {:>22} | {:>22} | {:>22} | {:>22} | {:>22} | {:>22}",
        "",
        "",
        "csa",
        "csa arriveBy",
        "raptor 6 rounds (mc4)",
        "raptor 8 rounds (mc6)",
        "arriveBy 6 rounds",
        "arriveBy 8 rounds"
    );
    for waypoint_count in [0usize, 4, 20] {
        for via_count in [0usize, 3] {
            let vias = bench_vias(&connections, via_count);
            let vias = (via_count > 0).then_some(&vias);
            let mut rng = Rng(17 + waypoint_count as u64);
            let mut rows = Vec::new();
            for _ in 0..3 {
                let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
                let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
                let mut waypoints: Vec<Vec<String>> = Vec::new();
                while waypoints.len() < waypoint_count {
                    let hub = vec![tiploc(4 + rng.next(HUBS - 4))];
                    if waypoints.last() != Some(&hub) {
                        waypoints.push(hub);
                    }
                }
                let staged = StagedOptions {
                    connections: &connections,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias,
                    date,
                };
                let arrive = ArriveByOptions {
                    connections: &connections,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias,
                    arrive_by_min: 1380,
                    date,
                };
                let once = |f: &dyn Fn() -> usize| {
                    let started = Instant::now();
                    let found = f();
                    (started.elapsed(), found)
                };
                rows.push([
                    once(&|| usize::from(scan_staged(&staged, 360, None, None).is_some())),
                    once(&|| usize::from(staged_arrive_by(&arrive, None, None).is_some())),
                    once(&|| raptor_staged(&staged, 360, 6, None, None).len()),
                    once(&|| raptor_staged(&staged, 360, 8, None, None).len()),
                    once(&|| {
                        latest_departures_by_trips(&arrive, None, None, 6)
                            .iter()
                            .flatten()
                            .count()
                    }),
                    once(&|| {
                        latest_departures_by_trips(&arrive, None, None, 8)
                            .iter()
                            .flatten()
                            .count()
                    }),
                ]);
            }
            let column = |i: usize| {
                let mut values: Vec<Duration> = rows.iter().map(|row| row[i].0).collect();
                values.sort();
                let found: usize = rows.iter().map(|row| usize::from(row[i].1 > 0)).sum();
                format!("{:>7.1?} max {:>7.1?} {found}/3", values[1], values[2])
            };
            println!(
                "{waypoint_count:>3} wp {via_count:>2} via | {} | {} | {} | {} | {} | {}",
                column(0),
                column(1),
                column(2),
                column(3),
                column(4),
                column(5)
            );
        }
    }
}

/// OR-group vias (2026-10-07): `count` vias, each a group of `members`
/// stations (via `i`'s are hubs and ordinary stations spread over the
/// network), built as `build_vias` in the api does: every train that calls
/// at any member gets spans, and every 7th line also runs through one
/// member of each group between its calls.
fn bench_group_vias(connections: &[Connection], count: usize, members: u64) -> trip_planner::Vias {
    use std::collections::HashSet;
    use trip_planner::{PassSpan, Vias};
    let targets: Vec<Vec<String>> = (0..count as u64)
        .map(|via| {
            (0..members)
                .map(|m| {
                    if m == 0 {
                        tiploc(1 + via)
                    } else {
                        tiploc(HUBS + via * 97 + m * 13)
                    }
                })
                .collect()
        })
        .collect();
    let all: HashSet<&str> = targets.iter().flatten().map(String::as_str).collect();
    let touching: HashSet<&str> = connections
        .iter()
        .filter(|c| all.contains(c.from_tiploc.as_str()) || all.contains(c.to_tiploc.as_str()))
        .map(|c| c.uid.as_str())
        .collect();
    let mut spans: HashMap<String, Vec<PassSpan>> = HashMap::new();
    for c in connections {
        let line = c.uid[1..]
            .split('D')
            .next()
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(1);
        let passes = line.is_multiple_of(7);
        if !passes && !touching.contains(c.uid.as_str()) {
            continue;
        }
        let passed = if passes {
            targets
                .iter()
                .map(|group| group[(line % members) as usize].clone())
                .collect()
        } else {
            Vec::new()
        };
        spans.entry(c.uid.clone()).or_default().push(PassSpan {
            from_tiploc: c.from_tiploc.clone(),
            to_tiploc: c.to_tiploc.clone(),
            departure_min: c.departure_min,
            passed,
        });
    }
    println!(
        "{count} vias x {members} members: {} trains carry spans",
        spans.len()
    );
    Vias::new(&targets, spans)
}

/// The cost of an 18-member OR group per via against a single-station via,
/// at the options guard's limits with 3 vias (`(waypoints + 1) * 7 *
/// rounds <= 252`: 5 waypoints at 6 rounds, 3 at 8). A group is ONE via
/// to the search (one progress step), so it should cost about what a
/// single via does; the extra is the per-connection span lookup of the
/// many more trains that touch a member.
#[test]
#[ignore = "benchmark; see the module doc"]
fn bench_group_vias_against_single() {
    use trip_planner::{
        StagedOptions, latest_departures_by_trips, raptor_staged, scan_staged, staged_arrive_by,
    };
    let (connections, interchange) = network();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    println!(
        "{:>14} | {:>22} | {:>22} | {:>22} | {:>22}",
        "", "csa", "csa arriveBy", "raptor (guard max)", "arriveBy rounds"
    );
    for (waypoint_count, rounds) in [(0usize, 8u32), (5, 6), (3, 8)] {
        for members in [1u64, 18] {
            let vias = bench_group_vias(&connections, 3, members);
            let mut rng = Rng(31 + waypoint_count as u64);
            let mut rows = Vec::new();
            for _ in 0..3 {
                let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
                let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
                let mut waypoints: Vec<Vec<String>> = Vec::new();
                while waypoints.len() < waypoint_count {
                    let hub = vec![tiploc(4 + rng.next(HUBS - 4))];
                    if waypoints.last() != Some(&hub) {
                        waypoints.push(hub);
                    }
                }
                let staged = StagedOptions {
                    connections: &connections,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias: Some(&vias),
                    date,
                };
                let arrive = ArriveByOptions {
                    connections: &connections,
                    interchange: &interchange,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias: Some(&vias),
                    arrive_by_min: 1380,
                    date,
                };
                let once = |f: &dyn Fn() -> usize| {
                    let started = Instant::now();
                    let found = f();
                    (started.elapsed(), found)
                };
                rows.push([
                    once(&|| usize::from(scan_staged(&staged, 360, None, None).is_some())),
                    once(&|| usize::from(staged_arrive_by(&arrive, None, None).is_some())),
                    once(&|| raptor_staged(&staged, 360, rounds, None, None).len()),
                    once(&|| {
                        latest_departures_by_trips(&arrive, None, None, rounds)
                            .iter()
                            .flatten()
                            .count()
                    }),
                ]);
            }
            let column = |i: usize| {
                let mut values: Vec<Duration> = rows.iter().map(|row| row[i].0).collect();
                values.sort();
                let found: usize = rows.iter().map(|row| usize::from(row[i].1 > 0)).sum();
                format!("{:>7.1?} max {:>7.1?} {found}/3", values[1], values[2])
            };
            println!(
                "{waypoint_count} wp r{rounds} x{members:>2} | {} | {} | {} | {}",
                column(0),
                column(1),
                column(2),
                column(3)
            );
        }
    }
}
