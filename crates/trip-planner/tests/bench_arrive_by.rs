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
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
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
    let mut rng = Rng(0xdecade);
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
            let mut start = 300 + rng.next(headway as u64) as u32;
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
            to_tiplocs: &to,
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
                .map(|(t, n)| format!("{:>7.1?}/{n}", t))
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
