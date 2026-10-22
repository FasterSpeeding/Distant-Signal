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
        let runs_through = line.is_multiple_of(7);
        if !runs_through && !touching.contains(c.uid.as_str()) {
            continue;
        }
        let passed = if runs_through {
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

/// OR-group waypoints (2026-10-07): waypoint `i` of `count` as `members`
/// stations, a hub (`S0004 + 5i`) plus ordinary stations spread over the
/// network, apart from [`bench_group_vias`]' members. With `members == 1`
/// it is the hub alone. Adjacent waypoints share no station.
fn bench_group_waypoints(count: usize, members: u64) -> Vec<Vec<String>> {
    (0..count as u64)
        .map(|waypoint| {
            (0..members)
                .map(|m| {
                    if m == 0 {
                        tiploc(4 + (5 * waypoint) % (HUBS - 4))
                    } else {
                        tiploc(HUBS + (900 + waypoint * 89 + m * 11) % (STATIONS - HUBS))
                    }
                })
                .collect()
        })
        .collect()
}

/// The cost of 18-member OR-group waypoints against single-station ones,
/// at the limits `/Trips/plan` allows: 54 stations across the waypoints
/// (three 18-station groups; or two and 18 single stations, 20
/// waypoints), 8 RAPTOR rounds (`maxChanges=6`), and with three
/// 18-member group vias at the options guard's limit (3 waypoints, 3
/// vias, 8 rounds).
#[test]
#[ignore = "benchmark; see the module doc"]
fn bench_group_waypoints_against_single() {
    use trip_planner::{
        StagedOptions, latest_departures_by_trips, raptor_staged, scan_staged, staged_arrive_by,
    };
    let (connections, interchange) = network();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    println!(
        "{:>24} | {:>22} | {:>22} | {:>22} | {:>22}",
        "", "csa", "csa arriveBy", "raptor 8 rounds", "arriveBy 8 rounds"
    );
    // (waypoints, how many of them are groups, vias)
    for (waypoint_count, grouped, via_count) in [(3usize, 3usize, 0usize), (20, 2, 0), (3, 3, 3)] {
        for members in [1u64, 18] {
            let mut waypoints = bench_group_waypoints(waypoint_count, 1);
            for (index, group) in bench_group_waypoints(grouped, members)
                .into_iter()
                .enumerate()
            {
                waypoints[index * waypoint_count / grouped] = group;
            }
            let vias = bench_group_vias(&connections, via_count, members);
            let vias = (via_count > 0).then_some(&vias);
            let mut rng = Rng(41 + waypoint_count as u64);
            let mut rows = Vec::new();
            for _ in 0..3 {
                let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
                let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
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
                    once(&|| raptor_staged(&staged, 360, 8, None, None).len()),
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
                "{waypoint_count:>2} wp ({grouped} grp) {via_count} via x{members:>2} | {} | {} | {} | {}",
                column(0),
                column(1),
                column(2),
                column(3)
            );
        }
    }
}

/// Fixed-link rows `(from_crs, link)` for [`bench_fixed_links_both_ways`].
///
/// With `DS_BENCH_ALF` set to a real `RJTTF*ALF.txt`, its rows, each CRS
/// mapped onto a synthetic station: the 40 most-linked CRSs (London's
/// termini and the like) onto the hubs, the rest onto other stations. The
/// ALF is Rail Data Marketplace data, so it is not checked in. Without it,
/// synthetic rows of the same size and shape (4,651 rows over 1,308 links in
/// RJTTF995ALF, 2026-10-20; three to five time windows per link; half the
/// links at a hub, each to a station near it in the numbering).
#[expect(
    clippy::unwrap_used,
    reason = "bench code: an unreadable or malformed ALF file should stop the bench"
)]
fn bench_link_rows(rng: &mut Rng) -> Vec<(String, schedule_query::FixedLink)> {
    use schedule_query::FixedLink;

    let link = |from: u64, to: u64, minutes: i32, window: (&str, &str, &str)| {
        (
            format!("C{from:04}"),
            FixedLink {
                mode: "TUBE".to_string(),
                to_crs: format!("C{to:04}"),
                minutes,
                valid_from: window.0.to_string(),
                valid_to: window.1.to_string(),
                days_mask: window.2.to_string(),
            },
        )
    };
    if let Ok(path) = std::env::var("DS_BENCH_ALF") {
        let text = std::fs::read_to_string(&path).unwrap();
        let parsed: Vec<HashMap<&str, &str>> = text
            .lines()
            .filter(|line| line.starts_with("M="))
            .map(|line| {
                line.trim()
                    .split(',')
                    .filter_map(|kv| kv.split_once('='))
                    .collect()
            })
            .collect();
        let mut degree: HashMap<&str, usize> = HashMap::new();
        for row in &parsed {
            *degree.entry(row["O"]).or_default() += 1;
            *degree.entry(row["D"]).or_default() += 1;
        }
        let mut by_degree: Vec<(&str, usize)> = degree.into_iter().collect();
        by_degree.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let station: HashMap<&str, u64> = by_degree
            .iter()
            .enumerate()
            .map(|(i, (crs, _))| {
                let i = i as u64;
                let index = if i < HUBS {
                    i
                } else {
                    HUBS + (i * 7) % (STATIONS - HUBS)
                };
                (*crs, index)
            })
            .collect();
        println!("{} ALF rows from {path}", parsed.len());
        return parsed
            .iter()
            .map(|row| {
                link(
                    station[row["O"]],
                    station[row["D"]],
                    row["T"].parse().unwrap(),
                    (row["S"], row["E"], row["R"]),
                )
            })
            .collect();
    }
    let windows = [
        (("0700", "1900", "1111110"), 0),
        (("1901", "2359", "1111110"), 5),
        (("0529", "0659", "1111100"), 5),
        (("0700", "2359", "0000001"), 5),
        (("0001", "0529", "1111100"), 10),
    ];
    let mut rows = Vec::new();
    for _ in 0..1308 {
        let from = if rng.next(2) == 0 {
            rng.next(HUBS)
        } else {
            rng.next(STATIONS)
        };
        let to = (from + 1 + rng.next(60)) % STATIONS;
        let minutes = 5 + rng.next(40) as i32;
        for (window, extra) in &windows[..3 + rng.next(3) as usize] {
            rows.push(link(from, to, minutes + extra, *window));
        }
    }
    rows
}

/// The synthetic network with [`bench_link_rows`] indexed one way (as
/// published) and both ways (`schedule_query::index_fixed_links_both_ways`).
fn link_indexes() -> (Vec<Connection>, InterchangeData, InterchangeData) {
    use schedule_query::index_fixed_links_both_ways;

    let (connections, base) = network();
    let rows = bench_link_rows(&mut Rng(0x0a1f));
    let mut one_way = base.clone();
    for (from, link) in rows.clone() {
        one_way
            .fixed_links_from_crs
            .entry(from)
            .or_default()
            .push(link);
    }
    let mut both_ways = base;
    both_ways.fixed_links_from_crs = index_fixed_links_both_ways(rows.clone());
    println!(
        "{} fixed-link rows; indexed both ways: {}",
        rows.len(),
        both_ways
            .fixed_links_from_crs
            .values()
            .map(Vec::len)
            .sum::<usize>()
    );
    (connections, one_way, both_ways)
}

/// The ALF fixed links one way (as published) against both ways
/// (`schedule_query::index_fixed_links_both_ways`), on the synthetic
/// network plus [`bench_link_rows`]. Each row: CSA depart-after, CSA
/// arrive-by, RAPTOR depart-after (4 rounds) and RAPTOR arrive-by, one way
/// then both ways. For the real file's links:
///
/// ```text
/// DS_BENCH_ALF=/path/to/RJTTF995ALF.txt cargo test --release -p trip-planner \
///     --test bench_arrive_by bench_fixed_links_both_ways -- --ignored --nocapture
/// ```
#[test]
#[ignore = "benchmark; see the module doc"]
fn bench_fixed_links_both_ways() {
    let (connections, one_way, both_ways) = link_indexes();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    let mut rng = Rng(0x0a1f);

    let mut table: Vec<[Duration; 8]> = Vec::new();
    for _ in 0..10 {
        let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
        let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
        let times = |interchange: &InterchangeData| {
            let arrive = || ArriveByOptions {
                connections: &connections,
                interchange,
                from_tiplocs: &from,
                waypoints: &[],
                to_tiplocs: &to,
                vias: None,
                arrive_by_min: 720,
                date,
            };
            [
                median(|| {
                    usize::from(
                        scan_connections_restricted(
                            ScanOptions {
                                connections: &connections,
                                interchange,
                                from_tiplocs: &from,
                                to_tiplocs: &to,
                                departure_min: 480,
                                date,
                            },
                            None,
                            None,
                        )
                        .is_some(),
                    )
                }),
                median(|| usize::from(scan_connections_arrive_by(arrive(), None, None).is_some())),
                median(|| {
                    raptor_search_restricted(
                        RaptorOptions {
                            connections: &connections,
                            interchange,
                            from_tiplocs: &from,
                            to_tiplocs: &to,
                            departure_min: 480,
                            date,
                            max_rounds: 4,
                        },
                        None,
                        None,
                    )
                    .len()
                }),
                median(|| raptor_arrive_by(arrive(), None, None, 4).len()),
            ]
            .map(|(t, _)| t)
        };
        let before = times(&one_way);
        let after = times(&both_ways);
        let mut row = [Duration::ZERO; 8];
        row[..4].copy_from_slice(&before);
        row[4..].copy_from_slice(&after);
        table.push(row);
    }
    let labels = [
        "csa depart",
        "csa arriveBy",
        "raptor depart",
        "raptor arriveBy",
    ];
    for (column, label) in labels.iter().enumerate() {
        let pick = |offset: usize| {
            let mut values: Vec<Duration> = table.iter().map(|row| row[column + offset]).collect();
            values.sort();
            (values[values.len() / 2], values[values.len() - 1])
        };
        let (before_median, before_max) = pick(0);
        let (after_median, after_max) = pick(4);
        println!(
            "{label:16} one way median {before_median:>8.1?} max {before_max:>8.1?}  \
             both ways median {after_median:>8.1?} max {after_max:>8.1?}"
        );
    }
}

/// [`bench_fixed_links_both_ways`] for the staged (waypoint and via)
/// searches: CSA depart-after, RAPTOR depart-after (6 rounds) and CSA
/// arrive-by, with 0 and 2 hub waypoints and with one via, one way then
/// both ways. `DS_BENCH_ALF` as there.
#[test]
#[ignore = "benchmark; see the module doc"]
fn bench_staged_fixed_links_both_ways() {
    use trip_planner::{StagedOptions, raptor_staged, scan_staged, staged_arrive_by};

    let (connections, one_way, both_ways) = link_indexes();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    let vias = bench_vias(&connections, 1);
    for (shape, waypoint_count, with_via) in [
        ("no waypoints", 0usize, false),
        ("2 waypoints", 2, false),
        ("1 via", 0, true),
    ] {
        let mut rng = Rng(0x57a9);
        let mut table: Vec<[(Duration, usize); 6]> = Vec::new();
        for _ in 0..10 {
            let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
            let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
            let mut waypoints: Vec<Vec<String>> = Vec::new();
            while waypoints.len() < waypoint_count {
                let hub = vec![tiploc(rng.next(HUBS))];
                if waypoints.last() != Some(&hub) {
                    waypoints.push(hub);
                }
            }
            let vias = with_via.then_some(&vias);
            let times = |interchange: &InterchangeData| {
                let staged = StagedOptions {
                    connections: &connections,
                    interchange,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias,
                    date,
                };
                let arrive = ArriveByOptions {
                    connections: &connections,
                    interchange,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias,
                    arrive_by_min: 1380,
                    date,
                };
                [
                    median(|| usize::from(scan_staged(&staged, 480, None, None).is_some())),
                    median(|| raptor_staged(&staged, 480, 6, None, None).len()),
                    median(|| usize::from(staged_arrive_by(&arrive, None, None).is_some())),
                ]
            };
            let before = times(&one_way);
            let after = times(&both_ways);
            let mut row = [(Duration::ZERO, 0); 6];
            row[..3].copy_from_slice(&before);
            row[3..].copy_from_slice(&after);
            table.push(row);
        }
        for (column, label) in ["csa depart", "raptor depart", "csa arriveBy"]
            .iter()
            .enumerate()
        {
            let pick = |offset: usize| {
                let mut values: Vec<Duration> =
                    table.iter().map(|row| row[column + offset].0).collect();
                values.sort();
                let found: usize = table
                    .iter()
                    .map(|row| usize::from(row[column + offset].1 > 0))
                    .sum();
                (values[values.len() / 2], values[values.len() - 1], found)
            };
            let (before_median, before_max, before_found) = pick(0);
            let (after_median, after_max, after_found) = pick(3);
            println!(
                "{shape:12} {label:14} one way median {before_median:>8.1?} max {before_max:>8.1?} \
                 ({before_found}/10)  both ways median {after_median:>8.1?} max {after_max:>8.1?} \
                 ({after_found}/10)"
            );
        }
    }
}

/// DS-85: `/Trips/plan`'s options-mode `maxChanges` cap at 6, 8 and 10
/// (RAPTOR rounds `max_changes + 2`: 8, 10, 12), with the fixed links both
/// ways ([`link_indexes`]; `DS_BENCH_ALF` as there). Per cap: RAPTOR
/// depart-after and the arrive-by rounds (`latest_departures_by_trips`),
/// median and max over 10 pairs, and for how many pairs what a caller gets
/// within the cap (the options' changes and arrivals; the latest departure)
/// differs from the cap of 6.
#[test]
#[ignore = "benchmark; see the module doc"]
fn bench_max_changes_cap() {
    let (connections, _, interchange) = link_indexes();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    let mut rng = Rng(0x0d85);
    let pairs: Vec<(Vec<String>, Vec<String>)> = (0..10)
        .map(|_| {
            (
                vec![tiploc(HUBS + rng.next(STATIONS - HUBS))],
                vec![tiploc(HUBS + rng.next(STATIONS - HUBS))],
            )
        })
        .collect();
    let mut baseline: Vec<(usize, usize)> = Vec::new();
    for max_changes in [6u32, 8, 10] {
        let rounds = max_changes + 2;
        let mut depart = Vec::new();
        let mut arrive = Vec::new();
        let mut gained = 0;
        for (index, (from, to)) in pairs.iter().enumerate() {
            let (time, found) = median(|| {
                raptor_search_restricted(
                    RaptorOptions {
                        connections: &connections,
                        interchange: &interchange,
                        from_tiplocs: from,
                        to_tiplocs: to,
                        departure_min: 480,
                        date,
                        max_rounds: rounds,
                    },
                    None,
                    None,
                )
                .iter()
                .filter(|journey| journey.changes <= max_changes)
                // What a caller is offered: (changes, arrival) per option.
                .map(|journey| (journey.changes as usize) << 16 | journey.arrival_min as usize)
                .fold(0usize, |hash, option| hash.wrapping_mul(31).wrapping_add(option))
            });
            depart.push(time);
            let options = ArriveByOptions {
                connections: &connections,
                interchange: &interchange,
                from_tiplocs: from,
                waypoints: &[],
                to_tiplocs: to,
                vias: None,
                arrive_by_min: 1080,
                date,
            };
            let (time, latest) = median(|| {
                let latest = trip_planner::latest_departures_by_trips(&options, None, None, rounds);
                // The latest departure within the cap (non-decreasing).
                latest[max_changes as usize].map_or(0, |minute| minute as usize + 1)
            });
            arrive.push(time);
            if max_changes == 6 {
                baseline.push((found, latest));
            } else if (found, latest) != baseline[index] {
                gained += 1;
            }
        }
        let stats = |mut values: Vec<Duration>| {
            values.sort();
            (values[values.len() / 2], values[values.len() - 1])
        };
        let (depart_median, depart_max) = stats(depart);
        let (arrive_median, arrive_max) = stats(arrive);
        println!(
            "maxChanges {max_changes:2} ({rounds:2} rounds): raptor depart median {depart_median:>8.1?} \
             max {depart_max:>8.1?} | arriveBy rounds median {arrive_median:>8.1?} max {arrive_max:>8.1?} \
             | pairs changed vs 6: {gained}/10"
        );
    }
}

/// Cost of a live overlay as large as `/Trips/plan`'s in-search snapshot
/// (2026-10-22): one train in ten replaced by itself running 1-30 minutes
/// late, one in thirty withdrawn (cancelled). Each search without the
/// overlay, with it, and with it indexed (`ConnectionOverlay::indexed`).
#[test]
#[ignore = "benchmark; see the module doc"]
#[expect(
    clippy::too_many_lines,
    reason = "one benchmark table, read top to bottom"
)]
fn bench_live_overlay() {
    use trip_planner::ConnectionOverlay;

    let (connections, interchange) = network();
    let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
    let mut uids: Vec<&str> = connections.iter().map(|c| c.uid.as_str()).collect();
    uids.sort_unstable();
    uids.dedup();
    let mut rng = Rng(11);
    let mut replaced = Vec::new();
    let mut delays: HashMap<&str, u32> = HashMap::new();
    for uid in &uids {
        match rng.next(30) {
            0..=2 => {
                delays.insert(uid, 1 + rng.next(30) as u32);
            }
            3 => replaced.push((*uid).to_string()),
            _ => {}
        }
    }
    let replacements: Vec<Connection> = connections
        .iter()
        .filter_map(|connection| {
            let delay = delays.get(connection.uid.as_str())?;
            let mut late = connection.clone();
            late.departure_min += delay;
            late.arrival_min += delay;
            Some(late)
        })
        .collect();
    replaced.extend(delays.keys().map(|uid| (*uid).to_string()));
    let started = Instant::now();
    let overlay = ConnectionOverlay::new(replaced.clone(), replacements.clone());
    println!(
        "overlay: {} trains replaced, {} connections, built in {:.1?}",
        replaced.len(),
        replacements.len(),
        started.elapsed()
    );
    let started = Instant::now();
    let indexed = overlay.clone().indexed(&connections);
    println!("indexed in {:.1?}", started.elapsed());

    let mut rng = Rng(7);
    let mut rows = Vec::new();
    for _ in 0..10 {
        let from = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
        let to = vec![tiploc(HUBS + rng.next(STATIONS - HUBS))];
        let scan = |overlay: Option<&ConnectionOverlay>| {
            usize::from(
                scan_connections_restricted(
                    ScanOptions {
                        connections: &connections,
                        interchange: &interchange,
                        from_tiplocs: &from,
                        to_tiplocs: &to,
                        departure_min: 480,
                        date,
                    },
                    overlay,
                    None,
                )
                .is_some(),
            )
        };
        let raptor = |overlay: Option<&ConnectionOverlay>| {
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
                overlay,
                None,
            )
            .len()
        };
        let arrive = || ArriveByOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: None,
            arrive_by_min: 720,
            date,
        };
        let times = [
            median(|| scan(None)),
            median(|| scan(Some(&overlay))),
            median(|| scan(Some(&indexed))),
            median(|| raptor(None)),
            median(|| raptor(Some(&overlay))),
            median(|| raptor(Some(&indexed))),
            median(|| raptor_arrive_by(arrive(), None, None, 4).len()),
            median(|| raptor_arrive_by(arrive(), Some(&overlay), None, 4).len()),
            median(|| raptor_arrive_by(arrive(), Some(&indexed), None, 4).len()),
        ];
        rows.push(times.map(|(t, _)| t));
    }
    let labels = [
        "csa depart 08:00",
        "csa depart 08:00 + overlay",
        "csa depart 08:00 + indexed",
        "raptor depart 08:00",
        "raptor depart 08:00 + overlay",
        "raptor depart 08:00 + indexed",
        "raptor arriveBy 12:00",
        "raptor arriveBy 12:00 + overlay",
        "raptor arriveBy 12:00 + indexed",
    ];
    for (column, label) in labels.iter().enumerate() {
        let mut values: Vec<Duration> = rows.iter().map(|row| row[column]).collect();
        values.sort();
        println!(
            "{label:32} median {:>8.1?}  max {:>8.1?}",
            values[values.len() / 2],
            values[values.len() - 1]
        );
    }
}
