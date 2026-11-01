//! The trip planner's timetable snapshot (DS-235): every published service
//! date's planning inputs in one structure, built once per schedule publish
//! (or transit feed), stored in Postgres and shared by every date's graph.
//!
//! **Why.** A date's graph was built from a per-date read: ~490k
//! `schedule_calling_points_full` rows, the date's `schedule_services` and
//! each transit network's trips, about 2.7 s on production. Most of those
//! rows repeat from date to date: on 2026-10-31 the 36 published dates'
//! 16.3M calling points were 2.1M distinct timed trips' worth (113k trips,
//! 11.5k stop patterns). The snapshot keeps each distinct calling-point
//! sequence (a block) once, each train UID's block with the days it runs
//! on, and each transit network's feed once with its trips' service days,
//! so a date's inputs are a filter over it, in memory.
//!
//! **What it holds**, for `[start, start + days)` (at most [`DaySet::DAYS`]):
//!
//! - `calls`/`blocks`: the distinct blocks, each a run of [`Call`]s;
//! - `trips`: (UID, block, days run);
//! - `services`: each `schedule_services` (UID, operator, mode), with the
//!   days it is published for;
//! - `feeds`: each switched-on transit network's active feed
//!   ([`NetworkFeed`]), its trips with their service days.
//!
//! [`TimetableSnapshot::day`] gives exactly what the per-date reads give
//! (`trip_planning::fetch_calling_points_for_date`,
//! `OperatorTable::fetch`, `schedule_services::all_modes_for_date` and
//! `transit_planning::read_days`), so a graph built from it is the graph
//! built from Postgres: the build that follows is the same code. The
//! date-independent reads (interchange data, fixed links, positions) stay
//! live: they are small.
//!
//! **Lifecycle.** The api builds it in the background (the graph prewarm
//! loop, `routes::trips::prewarm_graphs`) when the graph cache's key
//! ([`GraphMarker`]) moves, or the stored one is older than the cache's max
//! age, under a session advisory lock so only one process builds; stores
//! it gzip-compressed in `planner_snapshots`, keyed by [`version_for`]; and
//! every api process loads the stored row instead of building. Until one is
//! loaded (or with `TRIP_PLAN_SNAPSHOT_ENABLED` off), graphs are built from
//! Postgres as before.

use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, LazyLock, RwLock};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, NaiveDate, NaiveTime, Timelike, Utc};
use schedule_query::CallingPointForConnections;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::data::planner_operators::OperatorTable;
use crate::data::schedule_services::ServiceMode;
use crate::data::transit_planning::{self, NetworkDay, NetworkFeed, SkipReason};
use crate::data::trip_planning::{self, GraphMarker};

/// The stored format. A change to [`TimetableSnapshot`]'s shape bumps it, so
/// an older process never decodes a newer row (the version differs).
pub const FORMAT: u32 = 1;

/// `TRIP_PLAN_SNAPSHOT_ENABLED`: build, store and use the snapshot. Off by
/// default (the chart's `api.tripPlanGraphCache.snapshot`).
pub const ENABLED_ENV: &str = "TRIP_PLAN_SNAPSHOT_ENABLED";

/// Whether [`ENABLED_ENV`] is on.
pub fn enabled() -> bool {
    std::env::var(ENABLED_ENV).is_ok_and(|value| matches!(value.trim(), "true" | "1"))
}

/// A set of days, as offsets from a snapshot's start: one bit each.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DaySet([u64; 2]);

impl DaySet {
    /// How many days a set (and so a snapshot) spans: past the publish
    /// horizon's 7 days back and 60 ahead, with room to spare.
    pub const DAYS: u16 = 128;

    pub fn insert(&mut self, offset: u16) {
        if offset < Self::DAYS {
            self.0[usize::from(offset / 64)] |= 1 << (offset % 64);
        }
    }

    pub fn contains(self, offset: u16) -> bool {
        offset < Self::DAYS && self.0[usize::from(offset / 64)] & (1 << (offset % 64)) != 0
    }
}

/// One calling point: its stop (an index into `stops`), the four times as
/// seconds from midnight ([`Call::NONE`] for none), the day offset, and
/// whether one may board (bit 0) and alight (bit 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Call(u32, u32, u32, u32, u32, u8, u8);

impl Call {
    const NONE: u32 = u32::MAX;

    fn time(time: Option<NaiveTime>) -> Result<u32> {
        match time {
            None => Ok(Self::NONE),
            // A fraction would be lost: refuse rather than plan on a
            // different timetable (CIF times are whole seconds).
            Some(time) if time.nanosecond() != 0 => bail!("a time with a fraction: {time}"),
            Some(time) => Ok(time.num_seconds_from_midnight()),
        }
    }

    pub fn of(stop: u32, point: &CallingPointForConnections) -> Result<Self> {
        Ok(Self(
            stop,
            Self::time(point.booked_arrival)?,
            Self::time(point.booked_departure)?,
            Self::time(point.public_arrival)?,
            Self::time(point.public_departure)?,
            point.day_offset,
            u8::from(point.can_board) | (u8::from(point.can_alight) << 1),
        ))
    }

    fn naive(seconds: u32) -> Option<NaiveTime> {
        (seconds != Self::NONE)
            .then(|| NaiveTime::from_num_seconds_from_midnight_opt(seconds, 0))
            .flatten()
    }

    fn point(self, stops: &[String]) -> CallingPointForConnections {
        CallingPointForConnections {
            tiploc: stops[self.0 as usize].clone(),
            booked_arrival: Self::naive(self.1),
            booked_departure: Self::naive(self.2),
            day_offset: self.5,
            can_board: self.6 & 1 != 0,
            can_alight: self.6 & 2 != 0,
            public_arrival: Self::naive(self.3),
            public_departure: Self::naive(self.4),
        }
    }
}

/// A train UID's block on the days it runs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RailTrip {
    pub uid: String,
    pub block: u32,
    pub days: DaySet,
}

/// A `schedule_services` row (UID, operator, mode) on the days it is
/// published for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceRow {
    pub uid: String,
    pub operator_atoc: Option<String>,
    pub mode: String,
    pub days: DaySet,
}

/// See the module doc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimetableSnapshot {
    pub format: u32,
    /// The transit networks it was built for (the planner switch then).
    pub networks: Vec<String>,
    pub start: NaiveDate,
    pub days: u16,
    /// The days with calling points published.
    pub rail_dates: DaySet,
    pub stops: Vec<String>,
    pub calls: Vec<Call>,
    /// Where each block starts in `calls`; it ends where the next starts.
    pub blocks: Vec<u32>,
    pub trips: Vec<RailTrip>,
    pub services: Vec<ServiceRow>,
    /// Per network, in `networks`' order: its feed, `None` with no active
    /// feed version.
    pub feeds: Vec<(String, Option<NetworkFeed>)>,
}

/// A date's transit networks: the days read, and those left out and why.
pub type TransitDays = (Vec<NetworkDay>, Vec<(String, SkipReason)>);

/// One date's planning inputs from a snapshot: what the per-date reads
/// return.
pub struct DayInputs {
    pub calling_points: HashMap<String, Vec<CallingPointForConnections>>,
    pub operators: OperatorTable,
    pub modes: HashMap<String, ServiceMode>,
    /// `None` with the switch empty, as `read_days` is then not called.
    pub transit: Option<TransitDays>,
}

impl TimetableSnapshot {
    /// `date`'s offset from `start`, when the snapshot spans it.
    pub fn offset(&self, date: NaiveDate) -> Option<u16> {
        u16::try_from((date - self.start).num_days())
            .ok()
            .filter(|&offset| offset < self.days)
    }

    fn block(&self, block: u32) -> &[Call] {
        let start = self.blocks[block as usize] as usize;
        let end = self
            .blocks
            .get(block as usize + 1)
            .map_or(self.calls.len(), |&end| end as usize);
        &self.calls[start..end]
    }

    /// `date`'s inputs for `networks`: `None` when this snapshot cannot
    /// answer (outside its span, or built for other networks), so the
    /// caller reads Postgres; `Some(None)` when nothing is published for
    /// the date (the per-date read's `None`).
    pub fn day(&self, date: NaiveDate, networks: &[String]) -> Option<Option<DayInputs>> {
        if self.networks != networks {
            return None;
        }
        let offset = self.offset(date)?;
        if !self.rail_dates.contains(offset) {
            return Some(None);
        }
        let calling_points = self
            .trips
            .iter()
            .filter(|trip| trip.days.contains(offset))
            .map(|trip| {
                let points = self
                    .block(trip.block)
                    .iter()
                    .map(|call| call.point(&self.stops))
                    .collect();
                (trip.uid.clone(), points)
            })
            .collect();
        let services = self.services.iter().filter(|row| row.days.contains(offset));
        let operators = OperatorTable::new(
            services
                .clone()
                .filter_map(|row| {
                    let code = row.operator_atoc.as_deref()?;
                    Some((
                        row.uid.clone(),
                        schedule_query::OperatorId::from_atoc(code)?,
                    ))
                })
                .collect(),
        );
        let modes = services
            .map(|row| (row.uid.clone(), ServiceMode::from_db_str(&row.mode)))
            .collect();
        let transit = (!networks.is_empty()).then(|| {
            let mut days = Vec::new();
            let mut skipped = Vec::new();
            for (network, feed) in &self.feeds {
                let day = match (feed, transit_planning::planner_network(network)) {
                    (Some(feed), Some(settings)) => feed.day(network, *settings, date, offset),
                    _ => Err(SkipReason::NoFeed),
                };
                match day {
                    Ok(day) => days.push(day),
                    Err(reason) => {
                        transit_planning::count_skip(network, reason);
                        skipped.push((network.clone(), reason));
                    }
                }
            }
            (days, skipped)
        });
        Some(Some(DayInputs {
            calling_points,
            operators,
            modes,
            transit,
        }))
    }
}

/// The rail half of a snapshot as it is built: stops, blocks and trips
/// interned as each date's calling points are added.
#[derive(Default)]
struct RailBuilder {
    stop_ids: HashMap<String, u32>,
    stops: Vec<String>,
    /// Block ids by their calls' hash (the calls themselves are only in
    /// `calls`, so the build holds each block once).
    block_ids: HashMap<u64, Vec<u32>>,
    calls: Vec<Call>,
    blocks: Vec<u32>,
    trips: HashMap<(String, u32), DaySet>,
    rail_dates: DaySet,
}

impl RailBuilder {
    fn stop(&mut self, tiploc: &str) -> Result<u32> {
        if let Some(&id) = self.stop_ids.get(tiploc) {
            return Ok(id);
        }
        let id = u32::try_from(self.stops.len()).context("too many stops")?;
        self.stops.push(tiploc.to_owned());
        self.stop_ids.insert(tiploc.to_owned(), id);
        Ok(id)
    }

    fn stored(&self, id: u32) -> &[Call] {
        let start = self.blocks[id as usize] as usize;
        let end = self
            .blocks
            .get(id as usize + 1)
            .map_or(self.calls.len(), |&end| end as usize);
        &self.calls[start..end]
    }

    fn block(&mut self, block: &[Call]) -> Result<u32> {
        let hash = {
            use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
            BuildHasherDefault::<DefaultHasher>::default().hash_one(block)
        };
        if let Some(ids) = self.block_ids.get(&hash)
            && let Some(&id) = ids.iter().find(|&&id| self.stored(id) == block)
        {
            return Ok(id);
        }
        let id = u32::try_from(self.blocks.len()).context("too many blocks")?;
        self.blocks
            .push(u32::try_from(self.calls.len()).context("too many calls")?);
        self.calls.extend_from_slice(block);
        self.block_ids.entry(hash).or_default().push(id);
        Ok(id)
    }

    /// Adds day `offset`'s calling points.
    fn add(
        &mut self,
        offset: u16,
        by_uid: HashMap<String, Vec<CallingPointForConnections>>,
    ) -> Result<()> {
        self.rail_dates.insert(offset);
        for (uid, points) in by_uid {
            let mut block = Vec::with_capacity(points.len());
            for point in &points {
                let stop = self.stop(&point.tiploc)?;
                block.push(Call::of(stop, point)?);
            }
            let id = self.block(&block)?;
            self.trips.entry((uid, id)).or_default().insert(offset);
        }
        Ok(())
    }
}

/// Builds a snapshot from Postgres: every date with calling points (the
/// latest [`DaySet::DAYS`] of them at most), read one date at a time, and
/// each of `networks`' active feeds over the same span. About one
/// calling-point read per date (a minute or so for the whole horizon).
pub async fn build(pool: &PgPool, networks: &[String]) -> Result<TimetableSnapshot> {
    let (first, last): (Option<NaiveDate>, Option<NaiveDate>) = sqlx::query_as(
        "SELECT min(service_date), max(service_date) FROM schedule_calling_points_full",
    )
    .fetch_one(pool)
    .await
    .context("calling-point date range")?;
    let today = Utc::now().date_naive();
    let last = last.unwrap_or(today);
    let start = first
        .unwrap_or(today)
        .max(last - chrono::Days::new(u64::from(DaySet::DAYS - 1)));
    let days = u16::try_from((last - start).num_days() + 1)
        .unwrap_or(DaySet::DAYS)
        .clamp(1, DaySet::DAYS);

    let mut rail = RailBuilder::default();
    let mut services: HashMap<(String, Option<String>, String), DaySet> = HashMap::new();
    for offset in 0..days {
        let date = start + chrono::Days::new(u64::from(offset));
        if let Some(by_uid) = trip_planning::fetch_calling_points_for_date(pool, date).await? {
            rail.add(offset, by_uid)?;
        }
        let rows: Vec<(String, Option<String>, String)> = sqlx::query_as(
            "SELECT uid, operator_atoc, mode FROM schedule_services WHERE service_date = $1",
        )
        .bind(date)
        .fetch_all(pool)
        .await
        .context("schedule services")?;
        for (uid, operator_atoc, mode) in rows {
            services
                .entry((uid, operator_atoc, mode))
                .or_default()
                .insert(offset);
        }
    }
    let mut trips: Vec<RailTrip> = rail
        .trips
        .into_iter()
        .map(|((uid, block), days)| RailTrip { uid, block, days })
        .collect();
    trips.sort_by(|a, b| (&a.uid, a.block).cmp(&(&b.uid, b.block)));
    let mut services: Vec<ServiceRow> = services
        .into_iter()
        .map(|((uid, operator_atoc, mode), days)| ServiceRow {
            uid,
            operator_atoc,
            mode,
            days,
        })
        .collect();
    services.sort_by(|a, b| {
        (&a.uid, &a.operator_atoc, &a.mode).cmp(&(&b.uid, &b.operator_atoc, &b.mode))
    });
    let mut feeds = Vec::with_capacity(networks.len());
    for network in networks {
        let feed = transit_planning::read_network_feed(pool, network, start, days)
            .await
            .with_context(|| format!("transit feed {network}"))?;
        feeds.push((network.clone(), feed));
    }
    Ok(TimetableSnapshot {
        format: FORMAT,
        networks: networks.to_vec(),
        start,
        days,
        rail_dates: rail.rail_dates,
        stops: rail.stops,
        calls: rail.calls,
        blocks: rail.blocks,
        trips,
        services,
        feeds,
    })
}

/// The stored version of the snapshot for `marker` and `networks`: what a
/// process looks the row up by.
pub fn version_for(marker: &GraphMarker, networks: &[String]) -> String {
    let key = format!("{FORMAT}|{networks:?}|{marker:?}");
    hex(&Sha256::digest(key.as_bytes()))
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// JSON, gzip-compressed at the fastest level (the JSON is mostly digits:
/// even that is about a sixth of its size).
pub fn encode(snapshot: &TimetableSnapshot) -> Result<Vec<u8>> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    serde_json::to_writer(&mut encoder, snapshot).context("encode the snapshot")?;
    Ok(encoder.finish()?)
}

pub fn decode(bytes: &[u8]) -> Result<TimetableSnapshot> {
    let mut json = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut json)
        .context("decompress the snapshot")?;
    let snapshot: TimetableSnapshot =
        serde_json::from_slice(&json).context("decode the snapshot")?;
    if snapshot.format != FORMAT {
        bail!("snapshot format {} is not {FORMAT}", snapshot.format);
    }
    Ok(snapshot)
}

/// A stored snapshot row.
pub struct Stored {
    pub built_at: DateTime<Utc>,
    pub bytes: Vec<u8>,
}

/// Stores `bytes` as `version` (replacing a row of that version), and
/// deletes every other version but the newest one before it: a process
/// still on the previous publish can load it until it moves on.
pub async fn store(pool: &PgPool, version: &str, bytes: &[u8]) -> Result<()> {
    let sha256 = hex(&Sha256::digest(bytes));
    sqlx::query(
        "INSERT INTO planner_snapshots (version, sha256, bytes, built_at) \
         VALUES ($1, $2, $3, now()) \
         ON CONFLICT (version) DO UPDATE SET \
           sha256 = EXCLUDED.sha256, bytes = EXCLUDED.bytes, built_at = EXCLUDED.built_at",
    )
    .bind(version)
    .bind(&sha256)
    .bind(bytes)
    .execute(pool)
    .await
    .context("store the snapshot")?;
    sqlx::query(
        "DELETE FROM planner_snapshots WHERE version NOT IN ( \
           SELECT version FROM planner_snapshots ORDER BY built_at DESC LIMIT 2)",
    )
    .execute(pool)
    .await
    .context("prune old snapshots")?;
    Ok(())
}

/// The stored row for `version`, its checksum verified; `None` if absent.
pub async fn load(pool: &PgPool, version: &str) -> Result<Option<Stored>> {
    let row: Option<(DateTime<Utc>, String, Vec<u8>)> =
        sqlx::query_as("SELECT built_at, sha256, bytes FROM planner_snapshots WHERE version = $1")
            .bind(version)
            .fetch_optional(pool)
            .await
            .context("read the snapshot")?;
    let Some((built_at, sha256, bytes)) = row else {
        return Ok(None);
    };
    if hex(&Sha256::digest(&bytes)) != sha256 {
        bail!("snapshot {version} fails its checksum");
    }
    Ok(Some(Stored { built_at, bytes }))
}

/// The snapshot in use: what it was built for, and when.
pub struct Installed {
    pub version: String,
    pub built_at: DateTime<Utc>,
    pub snapshot: TimetableSnapshot,
}

static INSTALLED: LazyLock<RwLock<Option<Arc<Installed>>>> = LazyLock::new(RwLock::default);

/// The installed snapshot when it is the one for `marker` and `networks`
/// (and so matches the graph being built).
pub fn current(marker: &GraphMarker, networks: &[String]) -> Option<Arc<Installed>> {
    let installed = INSTALLED
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()?;
    (installed.version == version_for(marker, networks)).then_some(installed)
}

fn install(installed: Installed) {
    *INSTALLED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::new(installed));
}

/// What [`ensure`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The installed one is current.
    Current,
    /// Loaded the stored row another process (or an earlier one) built.
    Loaded,
    Built,
    /// Another process holds the build lock; try again next tick.
    Busy,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Loaded => "loaded",
            Self::Built => "built",
            Self::Busy => "busy",
        }
    }
}

/// `pg_try_advisory_lock` key for the build: ASCII "plansnap".
const BUILD_LOCK_KEY: i64 = 0x706c_616e_736e_6170;

/// Makes the snapshot for `marker` and `networks` the installed one: keeps
/// the installed one if current and younger than `max_age`, else loads the
/// stored row if that is, else builds one (under the advisory lock), stores
/// and installs it.
pub async fn ensure(
    pool: &PgPool,
    marker: &GraphMarker,
    networks: &[String],
    max_age: Duration,
) -> Result<Outcome> {
    let version = version_for(marker, networks);
    let fresh = |built_at: DateTime<Utc>| {
        (Utc::now() - built_at)
            .to_std()
            .is_ok_and(|age| age < max_age)
    };
    if current(marker, networks).is_some_and(|installed| fresh(installed.built_at)) {
        return Ok(Outcome::Current);
    }
    if let Some(stored) = load(pool, &version).await?
        && fresh(stored.built_at)
    {
        let started = Instant::now();
        let snapshot = tokio::task::spawn_blocking(move || decode(&stored.bytes)).await??;
        record_seconds("load", started.elapsed());
        install(Installed {
            version,
            built_at: stored.built_at,
            snapshot,
        });
        return Ok(Outcome::Loaded);
    }
    let mut lock = pool.acquire().await.context("a connection for the lock")?;
    let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1)")
        .bind(BUILD_LOCK_KEY)
        .fetch_one(&mut *lock)
        .await
        .context("the snapshot build lock")?;
    if !locked {
        return Ok(Outcome::Busy);
    }
    let built = build_store_install(pool, version, networks).await;
    let unlocked = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(BUILD_LOCK_KEY)
        .execute(&mut *lock)
        .await;
    if unlocked.is_err() {
        // A lock left held would stop every later build: drop the
        // connection rather than return it to the pool.
        lock.detach();
    }
    built.map(|()| Outcome::Built)
}

async fn build_store_install(pool: &PgPool, version: String, networks: &[String]) -> Result<()> {
    let started = Instant::now();
    let snapshot = build(pool, networks).await?;
    record_seconds("build", started.elapsed());
    let started = Instant::now();
    let (snapshot, bytes) = tokio::task::spawn_blocking(move || {
        let bytes = encode(&snapshot)?;
        anyhow::Ok((snapshot, bytes))
    })
    .await??;
    record_seconds("encode", started.elapsed());
    store(pool, &version, &bytes).await?;
    record_size(&snapshot, bytes.len());
    tracing::info!(
        version,
        days = snapshot.days,
        trips = snapshot.trips.len(),
        blocks = snapshot.blocks.len(),
        calls = snapshot.calls.len(),
        bytes = bytes.len(),
        "built the trip-planning timetable snapshot"
    );
    install(Installed {
        version,
        built_at: Utc::now(),
        snapshot,
    });
    Ok(())
}

/// `api_trip_plan_snapshot_seconds{phase}`: `build` (the reads), `encode`
/// and `load` (decode a stored row).
pub const SECONDS_METRIC: &str = "api_trip_plan_snapshot_seconds";
/// `api_trip_plan_snapshot_bytes`: the stored (compressed) size.
pub const BYTES_METRIC: &str = "api_trip_plan_snapshot_bytes";
/// `api_trip_plan_snapshot_calls`: distinct calling points held.
pub const CALLS_METRIC: &str = "api_trip_plan_snapshot_calls";
/// `api_trip_plan_snapshot_total{outcome}`: [`Outcome`]s, and `failed`.
pub const OUTCOMES_METRIC: &str = "api_trip_plan_snapshot_total";

fn record_seconds(phase: &'static str, elapsed: Duration) {
    metrics::histogram!(common::metrics::metric_name(SECONDS_METRIC), "phase" => phase)
        .record(elapsed.as_secs_f64());
}

#[expect(clippy::cast_precision_loss, reason = "gauges of millions")]
fn record_size(snapshot: &TimetableSnapshot, bytes: usize) {
    metrics::gauge!(common::metrics::metric_name(BYTES_METRIC)).set(bytes as f64);
    metrics::gauge!(common::metrics::metric_name(CALLS_METRIC)).set(snapshot.calls.len() as f64);
}

/// Counts one [`ensure`] outcome (`failed` for an error).
pub fn count_outcome(outcome: Result<Outcome, ()>) {
    let label = outcome.map_or("failed", Outcome::label);
    metrics::counter!(common::metrics::metric_name(OUTCOMES_METRIC), "outcome" => label)
        .increment(1);
}

/// Registers the outcome series at 0.
pub fn register_metrics() {
    for outcome in ["current", "loaded", "built", "busy", "failed"] {
        metrics::counter!(common::metrics::metric_name(OUTCOMES_METRIC), "outcome" => outcome)
            .increment(0);
    }
}

#[cfg(test)]
pub(crate) fn uninstall_for_tests() {
    *INSTALLED
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_set_holds_its_days_and_nothing_past_its_span() {
        let mut days = DaySet::default();
        for offset in [0, 63, 64, 127, 128, 500] {
            days.insert(offset);
        }
        for offset in [0, 63, 64, 127] {
            assert!(days.contains(offset), "{offset}");
        }
        for offset in [1, 62, 65, 126, 128, 500] {
            assert!(!days.contains(offset), "{offset}");
        }
    }

    fn time(text: &str) -> NaiveTime {
        text.parse().unwrap()
    }

    #[test]
    fn a_call_round_trips_its_calling_point() {
        let point = CallingPointForConnections {
            tiploc: "EUSTON".to_string(),
            booked_arrival: None,
            booked_departure: Some(time("23:59:30")),
            day_offset: 1,
            can_board: true,
            can_alight: false,
            public_arrival: Some(time("00:00:00")),
            public_departure: None,
        };
        let stops = vec!["EUSTON".to_string()];
        let call = Call::of(0, &point).unwrap();
        let back = call.point(&stops);
        assert_eq!(back.tiploc, point.tiploc);
        assert_eq!(back.booked_arrival, point.booked_arrival);
        assert_eq!(back.booked_departure, point.booked_departure);
        assert_eq!(back.public_arrival, point.public_arrival);
        assert_eq!(back.public_departure, point.public_departure);
        assert_eq!(back.day_offset, 1);
        assert!(back.can_board && !back.can_alight);
        let fraction = CallingPointForConnections {
            booked_arrival: Some(NaiveTime::from_hms_milli_opt(1, 2, 3, 500).unwrap()),
            ..point
        };
        assert!(Call::of(0, &fraction).is_err(), "a fraction would be lost");
    }

    fn sample() -> TimetableSnapshot {
        let mut days = DaySet::default();
        days.insert(1);
        TimetableSnapshot {
            format: FORMAT,
            networks: Vec::new(),
            start: NaiveDate::from_ymd_opt(2026, 11, 1).unwrap(),
            days: 3,
            rail_dates: days,
            stops: vec!["A".to_string(), "B".to_string()],
            calls: vec![
                Call(0, Call::NONE, 600, Call::NONE, 600, 0, 3),
                Call(1, 1200, Call::NONE, 1200, Call::NONE, 0, 3),
            ],
            blocks: vec![0],
            trips: vec![RailTrip {
                uid: "C1".to_string(),
                block: 0,
                days,
            }],
            services: vec![ServiceRow {
                uid: "C1".to_string(),
                operator_atoc: Some("VT".to_string()),
                mode: "bus".to_string(),
                days,
            }],
            feeds: Vec::new(),
        }
    }

    #[test]
    fn a_snapshot_encodes_and_decodes_unchanged() {
        let snapshot = sample();
        let bytes = encode(&snapshot).unwrap();
        assert_eq!(decode(&bytes).unwrap(), snapshot);
        let mut other = snapshot.clone();
        other.format = FORMAT + 1;
        assert!(decode(&encode(&other).unwrap()).is_err());
    }

    #[test]
    fn a_day_is_a_filter_over_the_snapshot() {
        let snapshot = sample();
        let start = snapshot.start;
        let day = |offset: u64| snapshot.day(start + chrono::Days::new(offset), &[]);
        assert!(matches!(day(0), Some(None)), "nothing published");
        assert!(day(3).is_none(), "past the span");
        assert!(
            snapshot.day(start, &["metrolink".to_string()]).is_none(),
            "built for other networks"
        );
        let inputs = day(1).unwrap().unwrap();
        assert_eq!(inputs.calling_points["C1"].len(), 2);
        assert_eq!(inputs.calling_points["C1"][1].tiploc, "B");
        assert_eq!(
            inputs.operators.operator_of("C1", None),
            schedule_query::OperatorId::from_atoc("VT").unwrap()
        );
        assert_eq!(inputs.modes["C1"], ServiceMode::from_db_str("bus"));
        assert!(inputs.transit.is_none());
    }

    #[test]
    fn the_version_follows_the_marker_and_the_networks() {
        let marker = GraphMarker::default();
        let other = GraphMarker {
            schedule: Some(Utc::now()),
            transit: Some(Vec::new()),
        };
        assert_eq!(version_for(&marker, &[]), version_for(&marker, &[]));
        assert_ne!(version_for(&marker, &[]), version_for(&other, &[]));
        assert_ne!(
            version_for(&marker, &[]),
            version_for(&marker, &["tfl".to_string()])
        );
        assert_eq!(version_for(&marker, &[]).len(), 64);
    }
}
