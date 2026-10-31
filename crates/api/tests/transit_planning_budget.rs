//! The memory and latency budget of trams in `/Trips/plan`'s day graph
//! (DS-164, docs/superpowers/specs/2026-10-24-transit-trip-planning-design.md
//! §10): at most 25 MB more for the cached dates, at most 10% more search
//! time. Measured on hand-made, production-shaped data, never production
//! rows:
//!
//! - a rail day of about 457,000 connections (26,000 trains over 2,500
//!   stations, the spec's §3.2 scale) with an interchange table of 3,000
//!   stations, 12,000 TIPLOCs and 15,000 fixed links;
//! - a Metrolink-shaped day: 7 lines, 99 stations, 200 platforms, 137
//!   patterns and 1,529 trips, about 27,600 connections (the spec's §3.1
//!   weekday), built by `api::data::transit_planning` exactly as the api
//!   does.
//!
//! Ignored by default, and a no-op unless `DS_RUN_BUDGET_BENCH` is set, so
//! CI's DB job (which runs every `--ignored` test, in debug) skips it. Run
//! it in release mode:
//!
//! ```text
//! DS_RUN_BUDGET_BENCH=1 cargo test --release -p api --test transit_planning_budget -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Memory is this process's resident set (`common::heap`), read after
//! freed pages are returned to the kernel, with each graph held: the
//! figure the api's pod pays. The phase M4 measurement on a copy of
//! production (`scripts/measure-transit-trip-planning.py`) checks it there.

#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::unwrap_used,
    clippy::print_stdout,
    reason = "test code: small known values; a panic is the right failure; benchmark output"
)]

use std::collections::HashMap;
use std::time::{Duration, Instant};

use api::data::transit_planning::{
    self, LineRow, NetworkDay, PatternRow, PatternStopRow, RailLinkRow, StopRow, TripRow,
};
use chrono::NaiveDate;
use schedule_query::{CallingPointForConnections, Connection, FixedLink, InterchangeData};
use trip_planner::{RaptorOptions, ScanOptions, raptor_search, scan_connections};

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

const STATIONS: u64 = 2500;
const HUBS: u64 = 40;
/// The rail stations the tram stations link to.
const TRAM_LINKED: [u64; 8] = [0, 1, 2, 3, 4, 5, 6, 7];

fn tiploc(i: u64) -> String {
    format!("T{i:05}")
}

fn crs(i: u64) -> String {
    format!("C{i:04}")
}

fn time(minutes: u32) -> chrono::NaiveTime {
    chrono::NaiveTime::from_num_seconds_from_midnight_opt((minutes % 1440) * 60, 0).unwrap()
}

/// A rail day as the api reads it: calling points by UID.
fn rail_calling_points() -> HashMap<String, Vec<CallingPointForConnections>> {
    let mut rng = Rng(0x00ba_d5ee);
    let mut by_uid = HashMap::new();
    let mut trains = 0;
    while by_uid.values().map(Vec::len).sum::<usize>() < 483_000 {
        let line = trains;
        let stops = 6 + rng.next(30) as usize;
        let mut route: Vec<u64> = (0..stops).map(|_| rng.next(STATIONS)).collect();
        route[0] = rng.next(HUBS);
        route[stops / 2] = rng.next(HUBS);
        let headway = [30, 60, 60, 120][rng.next(4) as usize];
        let mut start = 300 + rng.next(u64::from(headway)) as u32;
        while start < 1400 && trains < line + 30 {
            trains += 1;
            let mut minute = start;
            let points = route
                .iter()
                .enumerate()
                .map(|(k, station)| {
                    let point = CallingPointForConnections {
                        tiploc: tiploc(*station),
                        booked_arrival: (k > 0).then(|| time(minute)),
                        booked_departure: (k + 1 < stops).then(|| time(minute + 1)),
                        day_offset: (minute / 1440) as u8,
                        can_board: true,
                        can_alight: true,
                        public_arrival: None,
                        public_departure: None,
                    };
                    minute += 4;
                    point
                })
                .collect();
            by_uid.insert(format!("C{trains:05}"), points);
            start += headway;
        }
    }
    by_uid
}

fn rail_interchange() -> InterchangeData {
    let mut rng = Rng(7);
    let mut data = InterchangeData::default();
    for i in 0..STATIONS {
        data.tiploc_to_crs.insert(tiploc(i), crs(i));
        data.change_time_by_tiploc
            .insert(tiploc(i), if i < HUBS { 8 } else { 4 });
        data.crs_to_tiplocs.insert(crs(i), vec![tiploc(i)]);
    }
    // Platform TIPLOCs and junction TIPLOCs, to about 12,000.
    for j in 0..9_500 {
        let station = rng.next(STATIONS);
        let code = format!("X{j:05}");
        data.tiploc_to_crs.insert(code.clone(), crs(station));
        data.change_time_by_tiploc.insert(code.clone(), 5);
        data.crs_to_tiplocs
            .get_mut(&crs(station))
            .unwrap()
            .push(code);
    }
    // Local clusters of eight stations, as the ALF's links are (London's
    // terminals, a city's stations), not chains across the country.
    let rows = (0..7_500).map(|_| {
        let from = rng.next(STATIONS);
        let to = (from / 8) * 8 + rng.next(8);
        (
            crs(from),
            FixedLink {
                only_without: schedule_query::OperatorId::UNKNOWN,
                operator: schedule_query::OperatorId::UNKNOWN,
                mode: ["WALK", "TUBE", "TRANSFER", "BUS"][rng.next(4) as usize].to_owned(),
                to_crs: crs(to),
                minutes: 3 + rng.next(30) as i32,
                valid_from: "0000".to_owned(),
                valid_to: "2359".to_owned(),
                days_mask: "1111111".to_owned(),
            },
        )
    });
    data.fixed_links_from_crs = schedule_query::index_fixed_links_both_ways(rows);
    data
}

/// A Metrolink-shaped weekday.
#[expect(clippy::too_many_lines, reason = "one hand-made network, part by part")]
fn metrolink_day() -> NetworkDay {
    let mut rng = Rng(42);
    let station = |i: usize| format!("9400ZZMA{i:03}");
    let mut stops = Vec::new();
    for i in 0..99 {
        let position = (53.4 + i as f64 * 0.002, -2.3 + i as f64 * 0.001);
        stops.push(StopRow {
            stop_id: station(i),
            kind: "station".to_owned(),
            parent_stop_id: None,
            name: format!("Stop {i}"),
            indicator: None,
            lat: Some(position.0),
            lon: Some(position.1),
        });
        for platform in 1..=2 + usize::from(i % 50 == 0) {
            stops.push(StopRow {
                stop_id: format!("{}{platform}", station(i)),
                kind: "platform".to_owned(),
                parent_stop_id: Some(station(i)),
                name: format!("Stop {i}"),
                indicator: Some(format!("Platform {platform}")),
                lat: Some(position.0),
                lon: Some(position.1),
            });
        }
    }
    let mut patterns = HashMap::new();
    let mut lines = HashMap::new();
    let mut trips = Vec::new();
    for line in 0..7 {
        lines.insert(
            format!("l{line}"),
            LineRow {
                name: format!("Line {line}"),
                colour: Some("#1f6fb5".to_owned()),
                mode: None,
            },
        );
        let first = line * 12;
        let length = 18 + line;
        for variant in 0..10 {
            for direction in 0..2_i16 {
                let mut calls: Vec<usize> = (first..first + length - variant % 3)
                    .map(|s| s % 99)
                    .collect();
                if direction == 1 {
                    calls.reverse();
                }
                let id = format!("l{line}v{variant}d{direction}");
                let last = calls.len() - 1;
                patterns.insert(
                    id.clone(),
                    PatternRow {
                        line_id: Some(format!("l{line}")),
                        direction,
                        headsign: None,
                        stops: calls
                            .iter()
                            .enumerate()
                            .map(|(k, s)| PatternStopRow {
                                stop_id: format!("{}{}", station(*s), direction + 1),
                                arr_offset_s: i32::try_from(k).unwrap() * 95,
                                dep_offset_s: i32::try_from(k).unwrap() * 95,
                                pickup: i16::from(k == last),
                                drop_off: i16::from(k == 0),
                            })
                            .collect(),
                    },
                );
            }
        }
        for direction in 0..2 {
            let mut start = 5 * 3600 + rng.next(600) as i32;
            while start < 24 * 3600 {
                trips.push(TripRow {
                    trip_id: format!("{:042}", trips.len()),
                    line_id: Some(format!("l{line}")),
                    pattern_id: format!("l{line}v{}d{direction}", rng.next(10)),
                    start_s: start,
                    headsign: None,
                    replacement_bus: false,
                });
                start += if (7 * 3600..19 * 3600).contains(&start) {
                    450
                } else {
                    900
                };
            }
        }
    }
    trips.truncate(1_529);
    trips.sort_by(|a, b| (a.start_s, &a.trip_id).cmp(&(b.start_s, &b.trip_id)));
    let rail_links = TRAM_LINKED
        .iter()
        .enumerate()
        .map(|(n, rail)| RailLinkRow {
            stop_id: station(n * 12),
            crs: crs(*rail),
            kind: if n < 6 { "interchange" } else { "nearby" }.to_owned(),
            station_position: Some((53.4 + n as f64 * 0.024, -2.3 + n as f64 * 0.012)),
        })
        .collect();
    NetworkDay {
        network: "metrolink".to_owned(),
        settings: transit_planning::PLANNER_NETWORKS[0],
        stops,
        patterns,
        trips,
        lines,
        rail_links,
        hub_walks: Vec::new(),
        stop_modes: HashMap::new(),
    }
}

/// A `TfL`-shaped weekday (the `TfL` transit design §2.3, §11.2): 13 lines
/// of 18 to 55 stations (about 420 in all, the busiest shared), each with
/// a trunk and two branches each way, trains every 2.5 minutes in the
/// peaks and 4 otherwise from 05:30 to 00:30: about 11,000 trips and
/// 230,000 connections. Its stations link 60 rail stations, and ten pairs
/// are one hub.
#[expect(clippy::too_many_lines, reason = "one hand-made network, part by part")]
fn tfl_day() -> NetworkDay {
    let station = |i: usize| format!("9400ZZLU{i:03}");
    let mut stops = Vec::new();
    for i in 0..420 {
        let position = (
            51.4 + (i % 21) as f64 * 0.01,
            -0.3 + (i / 21) as f64 * 0.015,
        );
        stops.push(StopRow {
            stop_id: station(i),
            kind: "station".to_owned(),
            parent_stop_id: None,
            name: format!("Station {i}"),
            indicator: None,
            lat: Some(position.0),
            lon: Some(position.1),
        });
        for platform in 1..=2 {
            stops.push(StopRow {
                stop_id: format!("{}{platform}", station(i)),
                kind: "platform".to_owned(),
                parent_stop_id: Some(station(i)),
                name: format!("Station {i}"),
                indicator: None,
                lat: Some(position.0),
                lon: Some(position.1),
            });
        }
    }
    let mut patterns = HashMap::new();
    let mut lines = HashMap::new();
    let mut trips = Vec::new();
    for line in 0..13_usize {
        lines.insert(
            format!("l{line}"),
            LineRow {
                name: format!("Line {line}"),
                colour: Some("#1c3a94".to_owned()),
                mode: Some(if line < 11 { "tube" } else { "dlr" }.to_owned()),
            },
        );
        let length = 18 + (line * 7) % 38;
        // Each line's stations: a run through the shared middle (the
        // first 60 stations are on several lines), then its own.
        let route: Vec<usize> = (0..length)
            .map(|k| {
                if k % 3 == 0 {
                    (line * 5 + k) % 60
                } else {
                    60 + (line * 29 + k) % 360
                }
            })
            .collect();
        // A trunk and two branches: the whole run, its first two thirds,
        // and its last two thirds.
        let variants = [
            route.clone(),
            route[..length * 2 / 3].to_vec(),
            route[length / 3..].to_vec(),
        ];
        for (v, calls) in variants.iter().enumerate() {
            for direction in 0..2_i16 {
                let mut calls = calls.clone();
                if direction == 1 {
                    calls.reverse();
                }
                let last = calls.len() - 1;
                patterns.insert(
                    format!("l{line}v{v}d{direction}"),
                    PatternRow {
                        line_id: Some(format!("l{line}")),
                        direction,
                        headsign: None,
                        stops: calls
                            .iter()
                            .enumerate()
                            .map(|(k, s)| PatternStopRow {
                                stop_id: format!("{}{}", station(*s), direction + 1),
                                arr_offset_s: i32::try_from(k).unwrap() * 120,
                                dep_offset_s: i32::try_from(k).unwrap() * 120 + 20,
                                pickup: i16::from(k == last),
                                drop_off: i16::from(k == 0),
                            })
                            .collect(),
                    },
                );
            }
        }
        for direction in 0..2 {
            let mut start = 5 * 3600 + 1800;
            let mut n = 0;
            while start < 24 * 3600 + 1800 {
                trips.push(TripRow {
                    trip_id: format!("t{line}-{direction}-{n:04}"),
                    line_id: Some(format!("l{line}")),
                    pattern_id: format!("l{line}v{}d{direction}", n % 3),
                    start_s: start,
                    headsign: None,
                    replacement_bus: false,
                });
                n += 1;
                let peak = (7 * 3600..10 * 3600).contains(&start)
                    || (16 * 3600..19 * 3600).contains(&start);
                start += if peak { 150 } else { 240 };
            }
        }
    }
    trips.sort_by(|a, b| (a.start_s, &a.trip_id).cmp(&(b.start_s, &b.trip_id)));
    let rail_links = (0..60_u64)
        .map(|n| RailLinkRow {
            stop_id: station(n as usize),
            crs: crs(n),
            kind: "interchange".to_owned(),
            station_position: None,
        })
        .collect();
    let hub_walks = (0..10)
        .map(|n| transit_planning::HubWalkRow {
            from: station(100 + 2 * n),
            to: station(101 + 2 * n),
            minutes: 4,
        })
        .collect();
    NetworkDay {
        network: "tfl".to_owned(),
        settings: transit_planning::PLANNER_NETWORKS[1],
        stops,
        patterns,
        trips,
        lines,
        rail_links,
        hub_walks,
        stop_modes: HashMap::new(),
    }
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}

/// One date's graph, as `routes::trips::load_graph` builds it.
struct Graph {
    connections: Vec<Connection>,
    interchange: InterchangeData,
    /// Held, as the cache holds it, for the memory figure.
    _passes: schedule_query::PassIndex,
    transit: Option<transit_planning::TransitGraph>,
}

fn build(with_trams: bool) -> Graph {
    build_with(&if with_trams {
        vec![metrolink_day()]
    } else {
        Vec::new()
    })
}

/// One date's graph with `days`' networks (none: the rail graph).
fn build_with(days: &[NetworkDay]) -> Graph {
    let mut calling_points = rail_calling_points();
    let interchange = rail_interchange();
    calling_points.extend(transit_planning::calling_points(days));
    let schedules: Vec<(&str, &[CallingPointForConnections])> = calling_points
        .iter()
        .map(|(uid, points)| (uid.as_str(), points.as_slice()))
        .collect();
    let (connections, passes) = schedule_query::build_connections_with_passes(schedules);
    let transit = (!days.is_empty()).then(|| {
        transit_planning::build_transit_graph(
            &interchange,
            days,
            &transit_planning::count_connections(&connections),
            Vec::new(),
            2,
            5,
        )
    });
    Graph {
        connections,
        interchange,
        _passes: passes,
        transit,
    }
}

/// The resident set once freed memory is back with the kernel.
fn settled_rss() -> usize {
    common::heap::release_freed_memory();
    usize::try_from(common::heap::resident_set_bytes().unwrap()).unwrap()
}

/// What holding one more built graph adds to the resident set.
fn held(with_trams: bool) -> (usize, Graph) {
    let before = settled_rss();
    let graph = build(with_trams);
    (settled_rss().saturating_sub(before), graph)
}

/// The benchmarks only run when asked for (see the module doc): in a debug
/// build on a shared CI runner they take minutes and their timing ratio is
/// noise.
fn bench_requested() -> bool {
    std::env::var_os("DS_RUN_BUDGET_BENCH").is_some()
}

#[test]
#[ignore = "benchmark; see the module doc"]
fn memory_budget() {
    if !bench_requested() {
        return;
    }
    common::heap::pin_mmap_threshold();
    // Both held at once, as the cache holds its dates.
    let (rail, rail_graph) = held(false);
    let rail_connections = rail_graph.connections.len();
    let (both, graph) = held(true);
    let transit = graph.transit.as_ref().unwrap();
    let trams = transit.networks[0].connections;
    let delta = both.saturating_sub(rail);
    // Of which: the transit interchange table, a copy of the rail one.
    let before_copy = settled_rss();
    let copy = rail_graph.interchange.clone();
    let interchange_copy = settled_rss().saturating_sub(before_copy);
    drop(copy);
    drop(rail_graph);
    // `GraphCache` keeps 2 dates by default (`TRIP_PLAN_GRAPH_CACHE_DATES`).
    let cached = 2 * delta;
    println!(
        "rail day: {rail_connections} connections, {:.1} MB; with Metrolink: +{trams} connections, \
         {:.1} MB; per date +{:.1} MB (of which the interchange copy {:.1} MB), for 2 cached \
         dates +{:.1} MB (budget 25 MB){}",
        mb(rail),
        mb(both),
        mb(delta),
        mb(interchange_copy),
        mb(cached),
        if cached <= 25 * 1_048_576 {
            ""
        } else {
            ": OVER BUDGET"
        }
    );
    assert!(
        (440_000..480_000).contains(&rail_connections),
        "{rail_connections}"
    );
    assert!((24_000..34_000).contains(&trams), "{trams}");
    // No budget assertion: this prints the figure (and "OVER BUDGET") for
    // the PR and the card. The budget is checked on a copy of production
    // (phase M4), where the interchange table's real size applies.
}

/// The `TfL` network's memory (the `TfL` transit design §11.2): the rail
/// graph, then the same with Metrolink and `TfL`, both held. Budget: at
/// most 60 MB more for the two cached dates than Metrolink alone.
#[test]
#[ignore = "benchmark; see the module doc"]
fn memory_budget_with_tfl() {
    if !bench_requested() {
        return;
    }
    common::heap::pin_mmap_threshold();
    let (metrolink, metrolink_graph) = held(true);
    let (both, graph) = held_with(&[metrolink_day(), tfl_day()]);
    let transit = graph.transit.as_ref().unwrap();
    let tfl = transit
        .networks
        .iter()
        .find(|n| n.network == "tfl")
        .unwrap()
        .connections;
    let delta = both.saturating_sub(metrolink);
    let cached = 2 * delta;
    println!(
        "with Metrolink: {} connections, {:.1} MB; with TfL too: +{tfl} connections, {:.1} MB; \
         per date +{:.1} MB, for 2 cached dates +{:.1} MB (budget 60 MB){}",
        metrolink_graph.connections.len(),
        mb(metrolink),
        mb(both),
        mb(delta),
        mb(cached),
        if cached <= 60 * 1_048_576 {
            ""
        } else {
            ": OVER BUDGET"
        }
    );
    assert!((200_000..260_000).contains(&tfl), "{tfl}");
}

/// What holding one more graph with `days` adds to the resident set.
fn held_with(days: &[NetworkDay]) -> (usize, Graph) {
    let before = settled_rss();
    let graph = build_with(days);
    (settled_rss().saturating_sub(before), graph)
}

/// The searches with `TfL` (the `TfL` transit design §11.2): the same rail
/// queries on the Metrolink graph and on the graph with `TfL` too, both
/// with the transit interchange (a request that opted in). Budget: at most
/// 60% slower.
#[test]
#[ignore = "benchmark; see the module doc"]
fn latency_budget_with_tfl() {
    if !bench_requested() {
        return;
    }
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(measure_latency_with_tfl)
        .unwrap()
        .join()
        .unwrap();
}

fn measure_latency_with_tfl() {
    let metrolink = build(true);
    let tfl = build_with(&[metrolink_day(), tfl_day()]);
    let date = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    let mut rng = Rng(99);
    let queries: Vec<(Vec<String>, Vec<String>, u32)> = (0..12)
        .map(|_| {
            (
                vec![tiploc(HUBS + rng.next(STATIONS - HUBS))],
                vec![tiploc(HUBS + rng.next(STATIONS - HUBS))],
                420 + rng.next(300) as u32,
            )
        })
        .collect();
    let cases: [(&str, &Graph); 2] = [("Metrolink", &metrolink), ("Metrolink and TfL", &tfl)];
    let mut fastest: Vec<Vec<Duration>> = vec![Vec::new(); 2];
    let mut options: Vec<Vec<Duration>> = vec![Vec::new(); 2];
    for round in 0..7 {
        for (index, (_, graph)) in cases.iter().enumerate() {
            let interchange = &graph.transit.as_ref().unwrap().interchange;
            let started = Instant::now();
            for (from, to, at) in &queries {
                scan_connections(ScanOptions {
                    connections: &graph.connections,
                    interchange,
                    from_tiplocs: from,
                    to_tiplocs: to,
                    departure_min: *at,
                    date,
                });
            }
            let csa = started.elapsed();
            let started = Instant::now();
            for (from, to, at) in queries.iter().take(4) {
                raptor_search(RaptorOptions {
                    connections: &graph.connections,
                    interchange,
                    from_tiplocs: from,
                    to_tiplocs: to,
                    departure_min: *at,
                    date,
                    max_rounds: 4,
                });
            }
            let raptor = started.elapsed();
            if round > 0 {
                fastest[index].push(csa / queries.len() as u32);
                options[index].push(raptor / 4);
            }
        }
    }
    let base_fastest = median(&mut fastest[0]);
    let base_options = median(&mut options[0]);
    for (index, (name, _)) in cases.iter().enumerate() {
        let f = median(&mut fastest[index]);
        let o = median(&mut options[index]);
        println!(
            "{name}: fastest (CSA) {f:?} ({:+.1}%), options (RAPTOR, 4 rounds) {o:?} ({:+.1}%)",
            (f.as_secs_f64() / base_fastest.as_secs_f64() - 1.0) * 100.0,
            (o.as_secs_f64() / base_options.as_secs_f64() - 1.0) * 100.0,
        );
    }
    let with = median(&mut fastest[1]).as_secs_f64() / base_fastest.as_secs_f64();
    let with_options = median(&mut options[1]).as_secs_f64() / base_options.as_secs_f64();
    assert!(with <= 1.60, "fastest {with:.3}x");
    assert!(with_options <= 1.60, "options {with_options:.3}x");
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

/// The searches before and after: the same rail queries on the rail-only
/// graph (today), on the tram graph without the opt-in (the rail
/// interchange: a client that does not send `transit=true`), and with it
/// (the transit interchange). Interleaved, many rounds, medians.
#[test]
#[ignore = "benchmark; see the module doc"]
fn latency_budget() {
    if !bench_requested() {
        return;
    }
    // RAPTOR relaxes fixed links recursively, and a synthetic link network
    // can chain further than the ALF does: a large stack keeps this a
    // benchmark of time, not of stack depth.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(measure_latency)
        .unwrap()
        .join()
        .unwrap();
}

fn measure_latency() {
    let rail = build(false);
    let trams = build(true);
    let transit_interchange = &trams.transit.as_ref().unwrap().interchange;
    let date = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
    let mut rng = Rng(99);
    let queries: Vec<(Vec<String>, Vec<String>, u32)> = (0..12)
        .map(|_| {
            (
                vec![tiploc(HUBS + rng.next(STATIONS - HUBS))],
                vec![tiploc(HUBS + rng.next(STATIONS - HUBS))],
                420 + rng.next(300) as u32,
            )
        })
        .collect();
    let cases: [(&str, &[Connection], &InterchangeData); 3] = [
        ("today", &rail.connections, &rail.interchange),
        ("trams, no opt-in", &trams.connections, &trams.interchange),
        (
            "trams, transit=true",
            &trams.connections,
            transit_interchange,
        ),
    ];
    let mut fastest: Vec<Vec<Duration>> = vec![Vec::new(); 3];
    let mut options: Vec<Vec<Duration>> = vec![Vec::new(); 3];
    for round in 0..7 {
        for (index, (_, connections, interchange)) in cases.iter().enumerate() {
            let started = Instant::now();
            for (from, to, at) in &queries {
                scan_connections(ScanOptions {
                    connections,
                    interchange,
                    from_tiplocs: from,
                    to_tiplocs: to,
                    departure_min: *at,
                    date,
                });
            }
            let csa = started.elapsed();
            let started = Instant::now();
            for (from, to, at) in queries.iter().take(4) {
                raptor_search(RaptorOptions {
                    connections,
                    interchange,
                    from_tiplocs: from,
                    to_tiplocs: to,
                    departure_min: *at,
                    date,
                    max_rounds: 4,
                });
            }
            let raptor = started.elapsed();
            if round > 0 {
                fastest[index].push(csa / queries.len() as u32);
                options[index].push(raptor / 4);
            }
        }
    }
    let base_fastest = median(&mut fastest[0]);
    let base_options = median(&mut options[0]);
    for (index, (name, ..)) in cases.iter().enumerate() {
        let f = median(&mut fastest[index]);
        let o = median(&mut options[index]);
        println!(
            "{name}: fastest (CSA) {f:?} ({:+.1}%), options (RAPTOR, 4 rounds) {o:?} ({:+.1}%)",
            (f.as_secs_f64() / base_fastest.as_secs_f64() - 1.0) * 100.0,
            (o.as_secs_f64() / base_options.as_secs_f64() - 1.0) * 100.0,
        );
    }
    let with = median(&mut fastest[2]).as_secs_f64() / base_fastest.as_secs_f64();
    let with_options = median(&mut options[2]).as_secs_f64() / base_options.as_secs_f64();
    assert!(with <= 1.10, "fastest {with:.3}x");
    assert!(with_options <= 1.10, "options {with_options:.3}x");
}

/// The operator table each cached graph holds (planner operator filtering
/// spec 2026-10-31, §6.1): one `schedule_services` operator per schedule,
/// about 26,000 a weekday. Budget: 4 MB per date, so 8 MB for the two
/// cached dates. The per-connection and per-link operator ids fit in
/// existing padding (the `size_of` checks), so the table is the whole cost.
#[test]
#[ignore = "benchmark; see the module doc"]
fn operator_table_memory() {
    if !bench_requested() {
        return;
    }
    assert_eq!(
        size_of::<Connection>(),
        96,
        "the operator id must fit Connection's padding"
    );
    assert_eq!(
        size_of::<FixedLink>(),
        128,
        "the operator id must fit FixedLink's padding"
    );
    common::heap::pin_mmap_threshold();
    let before = settled_rss();
    let codes = ["VT", "GR", "GW", "SW", "SN", "SE", "NT", "SR", "XC", "LM"];
    let table = api::data::planner_operators::OperatorTable::new(
        (0..26_000_usize)
            .map(|n| {
                (
                    format!("C{n:05}"),
                    schedule_query::OperatorId::from_atoc(codes[n % codes.len()]).unwrap(),
                )
            })
            .collect(),
    );
    let used = settled_rss().saturating_sub(before);
    println!(
        "operator table: {} schedules, {:.1} MB per date, {:.1} MB for 2 cached dates \
         (budget 8 MB)",
        table.len(),
        mb(used),
        mb(2 * used)
    );
    assert!(2 * used <= 8 * 1_048_576, "{:.1} MB", mb(2 * used));
    drop(table);
}
