//! The timetable snapshot's cost (DS-235): its size stored and held, how
//! long it takes to encode and decode, and how long a date's inputs and
//! graph take to come out of it, on a hand-made timetable of production's
//! shape (2026-10-31: 26,000 schedules a weekday, about 19 calls each,
//! weekday, Saturday and Sunday blocks, and a little short-term variation),
//! never production rows. Ignored by default, and a no-op unless
//! `DS_RUN_BUDGET_BENCH` is set. Run it in release mode:
//!
//! ```text
//! DS_RUN_BUDGET_BENCH=1 cargo test --release -p api --test planner_snapshot_budget -- --ignored --nocapture
//! ```
//!
//! `DS_SNAPSHOT_DAYS` sets the span (default 68: 7 days back, today and
//! 60 ahead).

#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::unwrap_used,
    reason = "test code: small known values; a panic is the right failure; benchmark output"
)]

use std::time::Instant;

use api::data::planner_snapshot::{
    Call, DaySet, FORMAT, RailTrip, ServiceRow, TimetableSnapshot, decode, encode,
};
use chrono::{Datelike, NaiveDate, NaiveTime};
use schedule_query::CallingPointForConnections;

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

fn bench_requested() -> bool {
    std::env::var_os("DS_RUN_BUDGET_BENCH").is_some()
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / 1_048_576.0
}

fn settled_rss() -> usize {
    common::heap::release_freed_memory();
    usize::try_from(common::heap::resident_set_bytes().unwrap()).unwrap()
}

const STOPS: u64 = 3_800;

/// A block: `calls` calls from a start time, about 40% of them untimed
/// passes, as a weekday's calling points are.
fn block(rng: &mut Rng, calls: usize) -> Vec<Call> {
    let mut seconds = 5 * 3600 + 30 * rng.next(36 * 60) as u32;
    (0..calls)
        .map(|i| {
            seconds += 60 + 30 * rng.next(16) as u32;
            let at = NaiveTime::from_num_seconds_from_midnight_opt(seconds % 86_400, 0);
            let timed = i == 0 || i + 1 == calls || rng.next(10) >= 4;
            let point = CallingPointForConnections {
                tiploc: String::new(),
                booked_arrival: (timed && i > 0).then_some(at).flatten(),
                booked_departure: (timed && i + 1 < calls).then_some(at).flatten(),
                day_offset: (seconds / 86_400) as u8,
                can_board: true,
                can_alight: true,
                public_arrival: (timed && i > 0).then_some(at).flatten(),
                public_departure: (timed && i + 1 < calls).then_some(at).flatten(),
            };
            Call::of(rng.next(STOPS) as u32, &point).unwrap()
        })
        .collect()
}

/// Production's shape over `days` days from a Monday: 26,000 schedules
/// that run on weekdays (19,000), Saturdays (4,500) or Sundays (2,500),
/// each with its own block, and on each day 11% of them on a short-term
/// variation (a block of its own for that day): over 36 days that is about
/// 110,000 distinct trips, as production's 2026-10-31 horizon held 113,000.
fn timetable(days: u16) -> TimetableSnapshot {
    let mut rng = Rng(0x5a17);
    let start = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
    let mut calls: Vec<Call> = Vec::new();
    let mut blocks: Vec<u32> = Vec::new();
    let mut trips: Vec<RailTrip> = Vec::new();
    let mut services: Vec<ServiceRow> = Vec::new();
    let mut add_block = |calls: &mut Vec<Call>, block: Vec<Call>| {
        blocks.push(calls.len() as u32);
        calls.extend(block);
        blocks.len() as u32 - 1
    };
    for n in 0..26_000_u32 {
        let kind = match n {
            0..19_000 => 0,
            19_000..23_500 => 1,
            _ => 2,
        };
        let size = 8 + rng.next(23) as usize;
        let base = add_block(&mut calls, block(&mut rng, size));
        let mut base_days = DaySet::default();
        for offset in 0..days {
            let date = start + chrono::Days::new(u64::from(offset));
            let weekday = date.weekday().num_days_from_monday();
            let runs = match kind {
                0 => weekday < 5,
                1 => weekday == 5,
                _ => weekday == 6,
            };
            if !runs {
                continue;
            }
            if rng.next(100) < 11 {
                let variant = add_block(&mut calls, block(&mut rng, size));
                let mut one = DaySet::default();
                one.insert(offset);
                trips.push(RailTrip {
                    uid: format!("C{n:05}"),
                    block: variant,
                    days: one,
                });
            } else {
                base_days.insert(offset);
            }
        }
        trips.push(RailTrip {
            uid: format!("C{n:05}"),
            block: base,
            days: base_days,
        });
        let mut all = DaySet::default();
        for offset in 0..days {
            all.insert(offset);
        }
        services.push(ServiceRow {
            uid: format!("C{n:05}"),
            operator_atoc: Some("VT".to_string()),
            mode: "train".to_string(),
            days: all,
        });
    }
    let mut rail_dates = DaySet::default();
    for offset in 0..days {
        rail_dates.insert(offset);
    }
    TimetableSnapshot {
        format: FORMAT,
        networks: Vec::new(),
        start,
        days,
        rail_dates,
        stops: (0..STOPS).map(|i| format!("S{i:04}")).collect(),
        calls,
        blocks,
        trips,
        services,
        feeds: Vec::new(),
    }
}

#[test]
#[ignore = "benchmark; see the module doc"]
fn snapshot_budget() {
    if !bench_requested() {
        return;
    }
    let days: u16 = std::env::var("DS_SNAPSHOT_DAYS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(68)
        .min(DaySet::DAYS);
    common::heap::pin_mmap_threshold();
    let before = settled_rss();
    let snapshot = timetable(days);
    let held = settled_rss().saturating_sub(before);

    let started = Instant::now();
    let bytes = encode(&snapshot).unwrap();
    let encoded = started.elapsed();
    let started = Instant::now();
    let decoded = decode(&bytes).unwrap();
    let decoded_in = started.elapsed();
    assert_eq!(decoded, snapshot);
    drop(decoded);

    // A weekday's inputs, then its connections as the graph build makes
    // them (the same sort as from Postgres).
    let date = snapshot.start + chrono::Days::new(8);
    let started = Instant::now();
    let inputs = snapshot.day(date, &[]).unwrap().unwrap();
    let filtered = started.elapsed();
    let rows: usize = inputs.calling_points.values().map(Vec::len).sum();
    let schedules = inputs.calling_points.len();
    let started = Instant::now();
    let (connections, _) =
        api::data::trip_planning::build_connections_with_passes(inputs.calling_points);
    let sorted = started.elapsed();
    println!(
        "{days} days: {} trips, {} blocks, {} distinct calls; held {:.1} MB; stored {:.1} MB \
         (encode {encoded:.2?}, decode {decoded_in:.2?}); a weekday: {schedules} schedules, {rows} calls in \
         {filtered:.2?}, {} connections in {sorted:.2?}",
        snapshot.trips.len(),
        snapshot.blocks.len(),
        snapshot.calls.len(),
        mb(held),
        mb(bytes.len()),
        connections.len(),
    );
    assert!((18_000..20_000).contains(&schedules), "{schedules}");
}
