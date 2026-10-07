//! Build of a service date's [`schedule_query::Connection`] array and
//! [`schedule_query::InterchangeData`] from Postgres. Originally built per
//! query and discarded; since TRIPS-1 the route keeps the last few dates'
//! graphs in a [`GraphCache`], invalidated by a new schedule publish.
//! See
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase2-connections-array-plan.md's
//! Judgment Call 1 for why this shape (an already-ingested, indexed
//! Postgres read, not a raw CIF re-parse) was chosen over both of the
//! design spec's own named hosting options.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use schedule_query::{CallingPointForConnections, Connection, FixedLink, InterchangeData};
use sqlx::PgPool;

#[derive(Debug, sqlx::FromRow)]
struct CallingPointRow {
    uid: String,
    // Selected only so the query text documents what `ORDER BY uid, seq`
    // orders by; the ordering itself is done in SQL, not by reading this
    // field back in Rust.
    #[expect(
        dead_code,
        reason = "selected only so the query documents its ORDER BY; see above"
    )]
    seq: i16,
    tiploc: String,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
    day_offset: i16,
    /// NULL on a row published before migration `20261001160000`: read as
    /// `true`, the behaviour before direction was published (a
    /// set-down-only stop was then simply absent).
    can_board: Option<bool>,
    can_alight: Option<bool>,
    /// The public times the planner searches on (design doc §10, P6);
    /// NULL on a row published before migration `20261001160000`, which
    /// plans on the working time instead.
    public_arrival: Option<chrono::NaiveTime>,
    public_departure: Option<chrono::NaiveTime>,
}

/// Reads every `schedule_calling_points_full` row for `date`, grouped by
/// `uid` and already ordered by `seq` (the `ORDER BY` below, not an
/// in-memory re-sort) -- exactly the shape [`schedule_query::build_connections`]
/// needs. `None` if no rows exist for `date` at all (no CIF delivery has
/// published this far ahead yet) -- the caller (Phase 5's route handler)
/// maps this to a 404, same "no CIF-derived schedule data has been
/// published for this leg's service date" convention
/// `search_journey_leg_candidates` already establishes.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to >= 0 first, and minute values fit easily"
)]
pub async fn fetch_calling_points_for_date(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<Option<HashMap<String, Vec<CallingPointForConnections>>>> {
    let rows: Vec<CallingPointRow> = sqlx::query_as(
        "SELECT uid, seq, tiploc, booked_arrival, booked_departure, day_offset, can_board, can_alight, \
                public_arrival, public_departure \
         FROM schedule_calling_points_full WHERE service_date = $1 ORDER BY uid, seq",
    )
    .bind(date)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }

    let mut by_uid: HashMap<String, Vec<CallingPointForConnections>> = HashMap::new();
    for row in rows {
        by_uid
            .entry(row.uid)
            .or_default()
            .push(CallingPointForConnections {
                tiploc: row.tiploc,
                booked_arrival: row.booked_arrival,
                booked_departure: row.booked_departure,
                day_offset: row.day_offset.max(0) as u8,
                can_board: row.can_board.unwrap_or(true),
                can_alight: row.can_alight.unwrap_or(true),
                public_arrival: row.public_arrival,
                public_departure: row.public_departure,
            });
    }
    Ok(Some(by_uid))
}

/// The purely CPU-bound half of building a date's connections array: the
/// borrow-shuffling plus [`schedule_query::build_connections`] itself (which
/// sorts every connection of the whole day).
///
/// Split out of [`build_connections_for_date`] so a caller that must not block
/// the async runtime can run THIS part on the blocking pool while keeping the
/// database read (`fetch_calling_points_for_date`) async -- which is exactly
/// what `routes::trips::get_trip_plan` does, per the 2026-09-25 review's High
/// 4a finding. Takes its input by value for the same reason: `spawn_blocking`
/// requires `'static`, so the map cannot be borrowed across the hop.
#[expect(
    clippy::implicit_hasher,
    clippy::needless_pass_by_value,
    reason = "callers always use the default hasher; public planner API takes its options and overlay by value"
)]
pub fn build_connections(
    by_uid: HashMap<String, Vec<CallingPointForConnections>>,
) -> Vec<Connection> {
    let schedules: Vec<(&str, &[CallingPointForConnections])> = by_uid
        .iter()
        .map(|(uid, points)| (uid.as_str(), points.as_slice()))
        .collect();
    schedule_query::build_connections(schedules)
}

/// [`build_connections`] plus the day's [`schedule_query::PassIndex`] (which
/// connections run through which TIPLOCs without calling), for
/// `/Trips/plan`'s pass-through `avoid`. The index holds one `u32` per
/// untimed calling-point row (about 190k on a 2026-09 weekday, under 1 MB).
#[expect(
    clippy::implicit_hasher,
    clippy::needless_pass_by_value,
    reason = "callers always use the default hasher; public planner API takes its options and overlay by value"
)]
pub fn build_connections_with_passes(
    by_uid: HashMap<String, Vec<CallingPointForConnections>>,
) -> (Vec<Connection>, schedule_query::PassIndex) {
    let schedules: Vec<(&str, &[CallingPointForConnections])> = by_uid
        .iter()
        .map(|(uid, points)| (uid.as_str(), points.as_slice()))
        .collect();
    schedule_query::build_connections_with_passes(schedules)
}

/// [`fetch_calling_points_for_date`] plus [`build_connections`] in one call.
/// Kept for callers that are not on a latency/blocking-sensitive path (this
/// module's own tests, and any future non-HTTP consumer); the trip-planning
/// route deliberately calls the two halves separately so the CPU-bound one
/// can go through `spawn_blocking` -- see [`build_connections`].
pub async fn build_connections_for_date(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<Option<Vec<Connection>>> {
    let Some(by_uid) = fetch_calling_points_for_date(pool, date).await? else {
        return Ok(None);
    };
    Ok(Some(build_connections(by_uid)))
}

#[derive(Debug, sqlx::FromRow)]
struct FixedLinkRow {
    from_crs: String,
    to_crs: String,
    mode: String,
    minutes: i32,
    valid_from: String,
    valid_to: String,
    days_mask: String,
}

/// Builds [`InterchangeData`] from the whole current `stanox_crs`,
/// `tiploc_crs`, and `fixed_links` tables (Phase 1's `stanox_crs`/
/// `fixed_links` reads, plus `tiploc_crs` added by Task 3 of
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md) -- all
/// small tables, so a full-table read on every trip-planning query is the
/// same cost class as the existing per-request reference-data reads this
/// app already does elsewhere (e.g. `queries::list_stanox_crs`), not a new
/// performance concern.
///
/// `tiploc_crs` is read in a SECOND pass, after the `stanox_crs` pass
/// below, so a `tiploc_crs` row naturally overrides/adds to whatever the
/// `stanox_crs` pass already populated for that TIPLOC on `insert`
/// (`change_time_by_tiploc`, `tiploc_to_crs`) -- same union-read posture as
/// `queries::crs_for_tiploc`/`crs_for_tiplocs_batch`/
/// `list_stanox_crs_for_crs`, which prefer a `tiploc_crs` row when a
/// TIPLOC exists in both tables. `crs_to_tiplocs`'s existing
/// dedup-by-`contains` guard already prevents duplicate entries regardless
/// of which pass runs first. A TIPLOC that exists ONLY in `tiploc_crs` --
/// e.g. Vauxhall's/Clapham Junction's previously-dropped sibling TIPLOC --
/// now also populates every one of these maps, which it could not before
/// this plan (see `journey.rs`'s `tiploc_key` doc comment).
pub async fn fetch_interchange_data(pool: &PgPool) -> Result<InterchangeData> {
    let stanox_rows = crate::data::queries::list_stanox_crs(pool).await?;
    let mut change_time_by_tiploc = HashMap::new();
    let mut tiploc_to_crs = HashMap::new();
    let mut crs_to_tiplocs: HashMap<String, Vec<String>> = HashMap::new();
    for row in &stanox_rows {
        if let Some(minutes) = row.change_time_minutes {
            change_time_by_tiploc.insert(row.tiploc.clone(), minutes);
        }
        tiploc_to_crs.insert(row.tiploc.clone(), row.crs.clone());
        // `stanox_crs.stanox` is the primary key, not `tiploc` -- multiple
        // STANOX rows (different platforms/areas of one physical station,
        // see `queries::crs_for_tiploc`'s own doc comment) can share one
        // TIPLOC, so guard against pushing the same TIPLOC into the same
        // CRS's list twice (harmless but wasteful: `sibling_tiplocs` would
        // otherwise return the same sibling more than once). This table is
        // small (~3,100 rows total), so an O(n) `contains` check per push
        // is fine.
        let siblings = crs_to_tiplocs.entry(row.crs.clone()).or_default();
        if !siblings.contains(&row.tiploc) {
            siblings.push(row.tiploc.clone());
        }
    }

    let tiploc_crs_rows = crate::data::queries::list_tiploc_crs(pool).await?;
    for row in &tiploc_crs_rows {
        if let Some(minutes) = row.change_time_minutes {
            change_time_by_tiploc.insert(row.tiploc.clone(), minutes);
        }
        tiploc_to_crs.insert(row.tiploc.clone(), row.crs.clone());
        let siblings = crs_to_tiplocs.entry(row.crs.clone()).or_default();
        if !siblings.contains(&row.tiploc) {
            siblings.push(row.tiploc.clone());
        }
    }

    // Bus stops and ferry terminals with no CRS of their own become end
    // points under their `tiploc:` code (`add_road_or_water_endpoints`).
    let locations = crate::data::tiploc_locations::road_or_water_locations(pool).await?;
    add_road_or_water_endpoints(&mut tiploc_to_crs, &mut crs_to_tiplocs, &locations);

    let fixed_link_rows: Vec<FixedLinkRow> = sqlx::query_as(
        "SELECT from_crs, to_crs, mode, minutes, valid_from, valid_to, days_mask FROM fixed_links",
    )
    .fetch_all(pool)
    .await?;
    let mut fixed_links_from_crs: HashMap<String, Vec<FixedLink>> = HashMap::new();
    for row in fixed_link_rows {
        fixed_links_from_crs
            .entry(row.from_crs)
            .or_default()
            .push(FixedLink {
                mode: row.mode,
                to_crs: row.to_crs,
                minutes: row.minutes,
                valid_from: row.valid_from,
                valid_to: row.valid_to,
                days_mask: row.days_mask,
            });
    }

    let mut data = InterchangeData {
        modal_change: schedule_query::ModalChangeBuffer::default(),
        change_time_by_tiploc,
        tiploc_to_crs,
        crs_to_tiplocs,
        fixed_links_from_crs,
    };

    // Walks between those stops and their parent stations.
    let parent_links = crate::data::tiploc_locations::parent_links(pool).await?;
    add_parent_walk_links(
        &mut data,
        &parent_links,
        &common::tiploc_parents::curated_parents(),
    );
    Ok(data)
}

/// The `mode` of a stop <-> parent station walking link: the same `WALK`
/// the ALF's own walking links carry, so the leg reads like any other
/// fixed-link leg (`kind: "transfer"`, `mode: "WALK"`).
pub const PARENT_WALK_MODE: &str = "WALK";

/// Links every bus stop and ferry terminal that is its own planner end
/// point (`tiploc:CODE`, see [`add_road_or_water_endpoints`]) to its parent
/// station with a walk each way, so a journey can change there: bus ->
/// walk -> train and train -> walk -> bus. Returns how many stops were
/// linked.
///
/// The walk is [`common::tiploc_parents::walk_minutes`]: the curated CSV's
/// `walk_minutes` when it gives one for this parent, else from the MSN grid
/// distance (`ceil(m / 80) + 2`, 3 to 15 minutes), else 15.
///
/// The bus or ferry change buffer (`schedule_query::ModalChangeBuffer`)
/// applies on the bus side as at any change, and is not part of the walk:
/// the forward searches charge it after alighting from a bus or ferry and
/// before walking on (`trip_planner::csa`'s `alighting_buffer`), and before
/// boarding one after the walk; the arrive-by search mirrors both. Starting
/// or ending a journey at the stop owes none.
///
/// A linked stop's own change time is left alone (stops have no MSN change
/// time in `tiploc_crs`/`stanox_crs`, so it is the default 5), except that a
/// "no interchange" sentinel there is dropped: walking in from the station
/// and boarding must be possible. An unlinked stop keeps whatever it had.
///
/// Skipped: a stop whose TIPLOC is part of a station (it is already that
/// station's sibling), and a parent the planner has no TIPLOC for.
pub fn add_parent_walk_links(
    data: &mut InterchangeData,
    links: &[crate::data::tiploc_locations::ParentLink],
    curated: &std::collections::BTreeMap<String, common::tiploc_parents::CuratedParent>,
) -> usize {
    let mut linked = 0;
    for link in links {
        let code = common::location_naming::tiploc_code(&link.tiploc);
        if data.tiploc_to_crs.get(&link.tiploc) != Some(&code)
            || !data.crs_to_tiplocs.contains_key(&link.parent_crs)
        {
            continue;
        }
        let curated_minutes = curated
            .get(&link.tiploc)
            .filter(|row| row.parent_crs == link.parent_crs)
            .and_then(|row| row.walk_minutes);
        let walk = common::tiploc_parents::walk_minutes(link.distance_m, curated_minutes);
        let walk_link = |to_crs: &str, minutes: i32| FixedLink {
            mode: PARENT_WALK_MODE.to_string(),
            to_crs: to_crs.to_string(),
            minutes,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
        };
        data.fixed_links_from_crs
            .entry(code.clone())
            .or_default()
            .push(walk_link(&link.parent_crs, walk));
        data.fixed_links_from_crs
            .entry(link.parent_crs.clone())
            .or_default()
            .push(walk_link(&code, walk));
        if schedule_query::minimum_change_time(data, &link.tiploc)
            == schedule_query::ChangeTime::NoInterchange
        {
            data.change_time_by_tiploc.remove(&link.tiploc);
        }
        linked += 1;
    }
    linked
}

/// Makes every bus stop and ferry terminal in `locations` that has no CRS
/// in the crosswalks a planner end point of its own: `tiploc:SANWBUS` ->
/// `[SANWBUS]` in `crs_to_tiplocs`, and the reverse in `tiploc_to_crs`, so
/// `?origin=tiploc:SANWBUS` resolves and a leg ending there reports that
/// code. One TIPLOC per code, so it has no same-station siblings; changes
/// there use its own MSN change time (often the 98/99 "no interchange"
/// sentinel, which still allows starting or ending a journey there). A stop
/// whose TIPLOC already maps to a station CRS stays part of that station.
///
/// Why a TIPLOC rather than the stop's MSN code (`SAO`): MSN codes share
/// the CRS namespace (a stop's code can be another station's CRS) and are
/// not unique per stop, while the TIPLOC is what the timetable itself
/// names. See docs/superpowers/specs/2026-10-06-tiploc-locations-design.md.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always use the default hasher"
)]
pub fn add_road_or_water_endpoints(
    tiploc_to_crs: &mut HashMap<String, String>,
    crs_to_tiplocs: &mut HashMap<String, Vec<String>>,
    locations: &[crate::data::tiploc_locations::LocationInfo],
) {
    for location in locations {
        if !location.location_type.is_road_or_water()
            || tiploc_to_crs.contains_key(&location.tiploc)
        {
            continue;
        }
        let code = location.code();
        tiploc_to_crs.insert(location.tiploc.clone(), code.clone());
        crs_to_tiplocs.insert(code, vec![location.tiploc.clone()]);
    }
}

/// `TRIP_PLAN_ROAD_WATER_CHANGE_MINUTES` (chart
/// `api.tripPlanRoadWaterChangeMinutes`): the extra change time on each bus
/// or ferry side of a change, clamped to `0..=30`, default 5. See
/// `schedule_query::ModalChangeBuffer`.
pub const ROAD_WATER_CHANGE_MINUTES_ENV: &str = "TRIP_PLAN_ROAD_WATER_CHANGE_MINUTES";
pub const DEFAULT_ROAD_WATER_CHANGE_MINUTES: u32 = 5;
const MAX_ROAD_WATER_CHANGE_MINUTES: u32 = 30;

/// The configured buffer, read once.
pub fn road_water_change_minutes() -> u32 {
    static MINUTES: std::sync::LazyLock<u32> = std::sync::LazyLock::new(|| {
        parse_road_water_change_minutes(
            std::env::var(ROAD_WATER_CHANGE_MINUTES_ENV).ok().as_deref(),
        )
    });
    *MINUTES
}

fn parse_road_water_change_minutes(raw: Option<&str>) -> u32 {
    match raw.map(str::trim) {
        None | Some("") => DEFAULT_ROAD_WATER_CHANGE_MINUTES,
        Some(raw) => raw.parse::<u32>().map_or_else(
            |_| {
                tracing::warn!(
                    raw,
                    "invalid {ROAD_WATER_CHANGE_MINUTES_ENV}; using the default"
                );
                DEFAULT_ROAD_WATER_CHANGE_MINUTES
            },
            |minutes| minutes.min(MAX_ROAD_WATER_CHANGE_MINUTES),
        ),
    }
}

/// The date's bus and ferry services, for the change buffer.
///
/// Each UID's published mode (`schedule_services`, `modes`) decides when it
/// has a row, so a rail-replacement bus calling only at station TIPLOCs gets
/// the buffer too, and a train that happens to call at a TIPLOC also listed
/// as a bus stop does not. A UID with no row (the date not yet published,
/// or a deploy before the first publish) falls back to the old heuristic:
/// it is a bus or ferry when it calls at a bus stop or ferry terminal
/// (`road_or_water_tiplocs`).
#[expect(
    clippy::implicit_hasher,
    reason = "callers always use the default hasher"
)]
pub fn modal_change_buffer(
    by_uid: &HashMap<String, Vec<CallingPointForConnections>>,
    modes: &HashMap<String, crate::data::schedule_services::ServiceMode>,
    road_or_water_tiplocs: &std::collections::HashSet<String>,
    minutes: u32,
) -> schedule_query::ModalChangeBuffer {
    let road_or_water_uids = if minutes == 0 {
        std::collections::HashSet::new()
    } else {
        by_uid
            .iter()
            .filter(|(uid, points)| {
                modes.get(uid.as_str()).map_or_else(
                    || {
                        points.iter().any(|point| {
                            road_or_water_tiplocs
                                .contains(schedule_query::normalize_tiploc(&point.tiploc))
                        })
                    },
                    |mode| mode.is_timetable_only(),
                )
            })
            .map(|(uid, _)| uid.clone())
            .collect()
    };
    schedule_query::ModalChangeBuffer {
        road_or_water_uids,
        minutes,
    }
}

/// Every bus stop's and ferry terminal's TIPLOC.
pub async fn fetch_road_or_water_tiplocs(
    pool: &PgPool,
) -> Result<std::collections::HashSet<String>> {
    Ok(crate::data::tiploc_locations::road_or_water_locations(pool)
        .await?
        .into_iter()
        .map(|location| location.tiploc)
        .collect())
}

/// A service date's connections graph and the interchange data it is
/// searched with: everything `routes::trips::get_trip_plan` reads from the
/// database before searching, built once and shared by every request for
/// that date through [`GraphCache`].
pub struct PlanningGraph {
    pub connections: Vec<Connection>,
    pub interchange: InterchangeData,
    /// Indices into `connections`, per TIPLOC run through without calling.
    pub passes: schedule_query::PassIndex,
}

/// When `schedule-reference` last finished publishing a delivery
/// (`schedule_reference_publishes.completed_at`), or `None` before the first.
/// [`GraphCache`] keys on it, so a new publish invalidates every cached
/// graph. One index-only top-1 read.
pub async fn latest_schedule_publish(pool: &PgPool) -> Result<Option<DateTime<Utc>>> {
    Ok(
        sqlx::query_scalar("SELECT max(completed_at) FROM schedule_reference_publishes")
            .fetch_one(pool)
            .await?,
    )
}

/// TRIPS-1: a small per-date cache of built [`PlanningGraph`]s.
///
/// Building a date's graph reads ~490k `schedule_calling_points_full` rows
/// and sorts the day's connections, so before this every `/Trips/plan`
/// request, anonymous and unauthenticated, did that from scratch, and one
/// client looping on it kept every planning slot busy.
///
/// An entry is reused while both hold:
///
/// - the latest publish marker ([`latest_schedule_publish`]) is the one it
///   was built under, so a completed delivery invalidates it at once;
/// - it is younger than `max_age`. The marker is written only when a whole
///   delivery's products have published, so a calling-points publish that
///   lands without one (another product failed, or a request raced a
///   publish in progress and cached a half-written day) is picked up within
///   `max_age` instead of never.
///
/// At most `capacity` dates are kept (least recently used evicted); each is
/// on the order of 100 MB, so the default is 2 (today and tomorrow).
/// `capacity` 0 disables caching. Builds run one at a time, in a spawned
/// task: concurrent misses for one date build it once, and a client that
/// disconnects mid-build doesn't abandon a build that another request will
/// then start again.
pub struct GraphCache<T> {
    entries: std::sync::Mutex<Vec<CacheEntry<T>>>,
    build_lock: tokio::sync::Mutex<()>,
    capacity: usize,
    max_age: Duration,
}

struct CacheEntry<T> {
    date: NaiveDate,
    marker: Option<DateTime<Utc>>,
    built_at: Instant,
    last_used: Instant,
    value: Arc<T>,
}

/// Whether a lookup was served from the cache; for the metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheOutcome {
    Hit,
    Built,
}

impl<T: Send + Sync + 'static> GraphCache<T> {
    pub fn new(capacity: usize, max_age: Duration) -> Self {
        Self {
            entries: std::sync::Mutex::new(Vec::new()),
            build_lock: tokio::sync::Mutex::new(()),
            capacity,
            max_age,
        }
    }

    #[expect(
        clippy::expect_used,
        reason = "a poisoned lock means another thread already panicked"
    )]
    fn lookup(&self, date: NaiveDate, marker: Option<DateTime<Utc>>) -> Option<Arc<T>> {
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("graph cache lock poisoned");
        let entry = entries.iter_mut().find(|entry| {
            entry.date == date
                && entry.marker == marker
                && now.duration_since(entry.built_at) < self.max_age
        })?;
        entry.last_used = now;
        Some(entry.value.clone())
    }

    #[expect(
        clippy::expect_used,
        reason = "a poisoned lock means another thread already panicked"
    )]
    fn insert(&self, date: NaiveDate, marker: Option<DateTime<Utc>>, value: Arc<T>) {
        let now = Instant::now();
        let mut entries = self.entries.lock().expect("graph cache lock poisoned");
        entries.retain(|entry| entry.date != date);
        while entries.len() >= self.capacity {
            let Some(oldest) = entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(index, _)| index)
            else {
                break;
            };
            entries.swap_remove(oldest);
        }
        entries.push(CacheEntry {
            date,
            marker,
            built_at: now,
            last_used: now,
            value,
        });
    }

    /// The cached value for `date` under `marker`, else `build()`'s, cached.
    /// `build` returning `Ok(None)` (nothing published for the date) is not
    /// cached: that answer costs one index probe, and a publish can change it
    /// at any moment.
    pub async fn get_or_build<F, Fut>(
        self: &Arc<Self>,
        date: NaiveDate,
        marker: Option<DateTime<Utc>>,
        build: F,
    ) -> Result<Option<(Arc<T>, CacheOutcome)>>
    where
        F: FnOnce() -> Fut + Send + 'static,
        Fut: Future<Output = Result<Option<T>>> + Send + 'static,
    {
        if self.capacity == 0 {
            return Ok(build()
                .await?
                .map(|value| (Arc::new(value), CacheOutcome::Built)));
        }
        if let Some(hit) = self.lookup(date, marker) {
            return Ok(Some((hit, CacheOutcome::Hit)));
        }
        let this = self.clone();
        tokio::spawn(async move {
            let _building = this.build_lock.lock().await;
            // Another request may have built it while this one waited.
            if let Some(hit) = this.lookup(date, marker) {
                return Ok(Some((hit, CacheOutcome::Hit)));
            }
            let Some(value) = build().await? else {
                return Ok(None);
            };
            let value = Arc::new(value);
            this.insert(date, marker, value.clone());
            Ok(Some((value, CacheOutcome::Built)))
        })
        .await
        .map_err(|err| anyhow::anyhow!("trip-planning graph build task failed: {err}"))?
    }

    #[cfg(test)]
    fn cached_dates(&self) -> Vec<NaiveDate> {
        let entries = self.entries.lock().unwrap();
        let mut dates: Vec<NaiveDate> = entries.iter().map(|entry| entry.date).collect();
        dates.sort();
        dates
    }
}

#[cfg(test)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::unnecessary_wraps,
    reason = "test code: casts of small known test values; fakes mirror the signatures they stand in for"
)]
mod graph_cache_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, day).unwrap()
    }

    fn marker(hour: u32) -> Option<DateTime<Utc>> {
        Some(
            NaiveDate::from_ymd_opt(2026, 9, 27)
                .unwrap()
                .and_hms_opt(hour, 0, 0)
                .unwrap()
                .and_utc(),
        )
    }

    /// Builds `day * 100 + n` where n counts builds, so a rebuild is visible.
    async fn get(
        cache: &Arc<GraphCache<u32>>,
        builds: &Arc<AtomicUsize>,
        day: u32,
        marker: Option<DateTime<Utc>>,
    ) -> (u32, CacheOutcome) {
        let builds = builds.clone();
        let (value, outcome) = cache
            .get_or_build(date(day), marker, move || async move {
                let n = builds.fetch_add(1, Ordering::SeqCst) as u32;
                Ok(Some(day * 100 + n))
            })
            .await
            .unwrap()
            .unwrap();
        (*value, outcome)
    }

    #[tokio::test]
    async fn a_second_request_for_the_same_date_is_a_hit() {
        let cache = Arc::new(GraphCache::new(2, Duration::from_secs(600)));
        let builds = Arc::new(AtomicUsize::new(0));
        assert_eq!(
            get(&cache, &builds, 1, marker(1)).await,
            (100, CacheOutcome::Built)
        );
        assert_eq!(
            get(&cache, &builds, 1, marker(1)).await,
            (100, CacheOutcome::Hit)
        );
        assert_eq!(builds.load(Ordering::SeqCst), 1);
    }

    /// A new publish marker invalidates the cached graph.
    #[tokio::test]
    async fn a_new_publish_rebuilds() {
        let cache = Arc::new(GraphCache::new(2, Duration::from_secs(600)));
        let builds = Arc::new(AtomicUsize::new(0));
        get(&cache, &builds, 1, marker(1)).await;
        assert_eq!(
            get(&cache, &builds, 1, marker(2)).await,
            (101, CacheOutcome::Built)
        );
        assert_eq!(
            cache.cached_dates(),
            [date(1)],
            "the stale entry is replaced"
        );
    }

    #[tokio::test]
    async fn an_entry_older_than_max_age_is_rebuilt() {
        let cache = Arc::new(GraphCache::new(2, Duration::from_millis(50)));
        let builds = Arc::new(AtomicUsize::new(0));
        get(&cache, &builds, 1, marker(1)).await;
        tokio::time::sleep(Duration::from_millis(80)).await;
        assert_eq!(
            get(&cache, &builds, 1, marker(1)).await,
            (101, CacheOutcome::Built)
        );
    }

    #[tokio::test]
    async fn the_least_recently_used_date_is_evicted_at_capacity() {
        let cache = Arc::new(GraphCache::new(2, Duration::from_secs(600)));
        let builds = Arc::new(AtomicUsize::new(0));
        get(&cache, &builds, 1, marker(1)).await;
        get(&cache, &builds, 2, marker(1)).await;
        // Touch day 1, so day 2 is the least recently used.
        get(&cache, &builds, 1, marker(1)).await;
        get(&cache, &builds, 3, marker(1)).await;
        assert_eq!(cache.cached_dates(), [date(1), date(3)]);
    }

    #[tokio::test]
    async fn concurrent_misses_for_one_date_build_it_once() {
        let cache = Arc::new(GraphCache::new(2, Duration::from_secs(600)));
        let builds = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let cache = cache.clone();
            let builds = builds.clone();
            tasks.push(tokio::spawn(async move {
                cache
                    .get_or_build(date(1), marker(1), move || async move {
                        builds.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(20)).await;
                        Ok(Some(7u32))
                    })
                    .await
                    .unwrap()
                    .unwrap()
                    .0
            }));
        }
        for task in tasks {
            assert_eq!(*task.await.unwrap(), 7);
        }
        assert_eq!(builds.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn nothing_published_is_not_cached() {
        let cache: Arc<GraphCache<u32>> = Arc::new(GraphCache::new(2, Duration::from_secs(600)));
        let result = cache
            .get_or_build(date(1), marker(1), || async { Ok(None) })
            .await
            .unwrap();
        assert!(result.is_none());
        assert!(cache.cached_dates().is_empty());
    }

    #[tokio::test]
    async fn capacity_zero_disables_the_cache() {
        let cache = Arc::new(GraphCache::new(0, Duration::from_secs(600)));
        let builds = Arc::new(AtomicUsize::new(0));
        get(&cache, &builds, 1, marker(1)).await;
        assert_eq!(
            get(&cache, &builds, 1, marker(1)).await,
            (101, CacheOutcome::Built)
        );
        assert!(cache.cached_dates().is_empty());
    }
}

#[cfg(test)]
mod road_or_water_tests {
    use std::collections::HashSet;

    use super::*;
    use crate::data::tiploc_locations::LocationInfo;
    use common::location_naming::LocationType;

    fn location(tiploc: &str, kind: LocationType) -> LocationInfo {
        LocationInfo {
            tiploc: tiploc.to_string(),
            name: tiploc.to_string(),
            display_name: tiploc.to_string(),
            location_type: kind,
            parent_crs: None,
            parent_name: None,
        }
    }

    #[test]
    fn bus_stops_without_a_crs_become_tiploc_coded_end_points() {
        let mut tiploc_to_crs = HashMap::from([("BANSBUS".to_string(), "BAD".to_string())]);
        let mut crs_to_tiplocs = HashMap::from([("BAD".to_string(), vec!["BANSBUS".to_string()])]);
        add_road_or_water_endpoints(
            &mut tiploc_to_crs,
            &mut crs_to_tiplocs,
            &[
                location("SANWBUS", LocationType::BusStop),
                location("BDICK", LocationType::FerryTerminal),
                location("BANSBUS", LocationType::BusStop),
                location("MARY10", LocationType::PassingPoint),
            ],
        );
        assert_eq!(crs_to_tiplocs["tiploc:SANWBUS"], ["SANWBUS"]);
        assert_eq!(tiploc_to_crs["SANWBUS"], "tiploc:SANWBUS");
        assert_eq!(crs_to_tiplocs["tiploc:BDICK"], ["BDICK"]);
        // Already a station's TIPLOC: stays with the station.
        assert_eq!(tiploc_to_crs["BANSBUS"], "BAD");
        assert!(!crs_to_tiplocs.contains_key("tiploc:BANSBUS"));
        assert!(!crs_to_tiplocs.contains_key("tiploc:MARY10"));
    }

    fn call(tiploc: &str) -> CallingPointForConnections {
        CallingPointForConnections {
            tiploc: tiploc.to_string(),
            booked_arrival: None,
            booked_departure: None,
            day_offset: 0,
            can_board: true,
            can_alight: true,
            public_arrival: None,
            public_departure: None,
        }
    }

    #[test]
    fn services_calling_at_a_bus_stop_get_the_buffer() {
        let by_uid = HashMap::from([
            ("BUS1".to_string(), vec![call("SANWBUS"), call("LEUCHRS")]),
            ("TRAIN".to_string(), vec![call("LEUCHRS"), call("EDINBUR")]),
        ]);
        let stops = HashSet::from(["SANWBUS".to_string()]);
        let buffer = modal_change_buffer(&by_uid, &HashMap::new(), &stops, 5);
        assert_eq!(
            buffer.road_or_water_uids,
            HashSet::from(["BUS1".to_string()])
        );
        assert_eq!(buffer.extra_for("BUS1"), 5);
        assert_eq!(buffer.extra_for("TRAIN"), 0);
        assert!(
            modal_change_buffer(&by_uid, &HashMap::new(), &stops, 0)
                .road_or_water_uids
                .is_empty()
        );
    }

    /// A published mode wins over the bus-stop heuristic both ways; a UID
    /// with no `schedule_services` row still falls back to it.
    #[test]
    fn the_published_service_mode_decides_the_buffer_when_present() {
        use crate::data::schedule_services::ServiceMode;
        let by_uid = HashMap::from([
            // A replacement bus calling only at station TIPLOCs.
            ("RBUS".to_string(), vec![call("LEUCHRS"), call("CUPR")]),
            // A train calling at a TIPLOC also listed as a bus stop.
            ("TRAIN".to_string(), vec![call("SANWBUS"), call("EDINBUR")]),
            // No row: the heuristic.
            ("UNPUB".to_string(), vec![call("SANWBUS"), call("LEUCHRS")]),
            ("FERRY".to_string(), vec![call("ARDROSS"), call("BRODICK")]),
        ]);
        let modes = HashMap::from([
            ("RBUS".to_string(), ServiceMode::ReplacementBus),
            ("TRAIN".to_string(), ServiceMode::Train),
            ("FERRY".to_string(), ServiceMode::Ferry),
        ]);
        let stops = HashSet::from(["SANWBUS".to_string()]);
        let buffer = modal_change_buffer(&by_uid, &modes, &stops, 5);
        assert_eq!(
            buffer.road_or_water_uids,
            HashSet::from(["RBUS".to_string(), "UNPUB".to_string(), "FERRY".to_string()])
        );
        // No bus stops known at all: published modes still apply.
        let buffer = modal_change_buffer(&by_uid, &modes, &HashSet::new(), 5);
        assert_eq!(
            buffer.road_or_water_uids,
            HashSet::from(["RBUS".to_string(), "FERRY".to_string()])
        );
    }

    #[test]
    fn the_buffer_setting_defaults_and_clamps() {
        assert_eq!(parse_road_water_change_minutes(None), 5);
        assert_eq!(parse_road_water_change_minutes(Some(" ")), 5);
        assert_eq!(parse_road_water_change_minutes(Some("0")), 0);
        assert_eq!(parse_road_water_change_minutes(Some("8")), 8);
        assert_eq!(parse_road_water_change_minutes(Some("99")), 30);
        assert_eq!(parse_road_water_change_minutes(Some("-1")), 5);
    }
}

/// Bus stop <-> parent station walking links (`add_parent_walk_links`),
/// on a Heathrow-like network: a bus from Woking to the Terminal 3 bus
/// stop (`HTRBUS3`, 412 m from Heathrow Terminals 2 & 3, `HXX`), trains
/// between `HXX` and Paddington, and an unlinked stop (`KESWICK`).
#[cfg(test)]
mod parent_walk_tests {
    use std::collections::{BTreeMap, HashSet};

    use chrono::NaiveDate;
    use common::location_naming::LocationType;
    use common::tiploc_parents::CuratedParent;
    use schedule_query::{ChangeTime, minimum_change_time};
    use trip_planner::{
        JourneyLeg, RaptorOptions, ScanOptions, TransferLeg, raptor_search, scan_connections,
    };

    use super::*;
    use crate::data::tiploc_locations::{LocationInfo, ParentLink};

    const BUFFER: u32 = 5;

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

    fn stop(tiploc: &str) -> LocationInfo {
        LocationInfo {
            tiploc: tiploc.to_string(),
            name: tiploc.to_string(),
            display_name: tiploc.to_string(),
            location_type: LocationType::BusStop,
            parent_crs: None,
            parent_name: None,
        }
    }

    fn link(tiploc: &str, parent: &str, distance_m: Option<i32>) -> ParentLink {
        ParentLink {
            tiploc: tiploc.to_string(),
            parent_crs: parent.to_string(),
            distance_m,
        }
    }

    fn curated(tiploc: &str, parent: &str, minutes: i32) -> BTreeMap<String, CuratedParent> {
        BTreeMap::from([(
            tiploc.to_string(),
            CuratedParent {
                parent_crs: parent.to_string(),
                walk_minutes: Some(minutes),
            },
        )])
    }

    /// Stations WOK (`WOKING`), HXX (`HTRWAPT`, 2-minute change), PAD
    /// (`PADTON`) and BAD (`BANSTED`, with its bus stop `BANSBUS` filed
    /// under it); bus stops `HTRBUS3` and `KESWICK` as `tiploc:` end
    /// points; buses `BUS*` get the 5-minute buffer. No links yet.
    fn network() -> InterchangeData {
        let mut tiploc_to_crs = HashMap::new();
        let mut crs_to_tiplocs: HashMap<String, Vec<String>> = HashMap::new();
        for (tiploc, crs) in [
            ("WOKING", "WOK"),
            ("HTRWAPT", "HXX"),
            ("PADTON", "PAD"),
            ("PENRITH", "PNR"),
            ("BANSTED", "BAD"),
            ("BANSBUS", "BAD"),
        ] {
            tiploc_to_crs.insert(tiploc.to_string(), crs.to_string());
            crs_to_tiplocs
                .entry(crs.to_string())
                .or_default()
                .push(tiploc.to_string());
        }
        add_road_or_water_endpoints(
            &mut tiploc_to_crs,
            &mut crs_to_tiplocs,
            &[stop("HTRBUS3"), stop("KESWICK"), stop("BANSBUS")],
        );
        InterchangeData {
            modal_change: schedule_query::ModalChangeBuffer {
                road_or_water_uids: ["BUS1", "BUS2", "BUS3", "BUSK"]
                    .into_iter()
                    .map(str::to_string)
                    .collect::<HashSet<_>>(),
                minutes: BUFFER,
            },
            change_time_by_tiploc: HashMap::from([("HTRWAPT".to_string(), 2)]),
            tiploc_to_crs,
            crs_to_tiplocs,
            fixed_links_from_crs: HashMap::new(),
        }
    }

    /// The network with the Terminal 3 stop linked (412 m, no curated
    /// minutes: ceil(412 / 80) + 2 = 8).
    fn linked_network() -> InterchangeData {
        let mut data = network();
        let linked = add_parent_walk_links(
            &mut data,
            &[link("HTRBUS3", "HXX", Some(412))],
            &BTreeMap::new(),
        );
        assert_eq!(linked, 1);
        data
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 7).unwrap()
    }

    fn csa(
        connections: &[Connection],
        data: &InterchangeData,
        from: &str,
        to: &str,
        at: u32,
    ) -> Option<trip_planner::Journey> {
        scan_connections(ScanOptions {
            connections,
            interchange: data,
            from_tiplocs: &data.crs_to_tiplocs[from],
            to_tiplocs: &data.crs_to_tiplocs[to],
            departure_min: at,
            date: date(),
        })
    }

    fn raptor(
        connections: &[Connection],
        data: &InterchangeData,
        from: &str,
        to: &str,
        at: u32,
    ) -> Vec<trip_planner::RaptorJourney> {
        raptor_search(RaptorOptions {
            connections,
            interchange: data,
            from_tiplocs: &data.crs_to_tiplocs[from],
            to_tiplocs: &data.crs_to_tiplocs[to],
            departure_min: at,
            date: date(),
            max_rounds: 4,
        })
    }

    /// `uid` for a train leg, `WALK n` for a walk.
    fn describe(legs: &[JourneyLeg]) -> Vec<String> {
        legs.iter()
            .map(|leg| match leg {
                JourneyLeg::Train(train) => train.uid.clone(),
                JourneyLeg::Transfer(TransferLeg { mode, minutes, .. }) => {
                    format!("{mode} {minutes}")
                }
            })
            .collect()
    }

    fn sorted(mut connections: Vec<Connection>) -> Vec<Connection> {
        connections.sort_by_key(|c| c.departure_min);
        connections
    }

    #[test]
    fn a_linked_stop_gets_the_same_walk_each_way() {
        let data = linked_network();
        let from_stop = &data.fixed_links_from_crs["tiploc:HTRBUS3"];
        assert_eq!(from_stop.len(), 1);
        assert_eq!(from_stop[0].to_crs, "HXX");
        assert_eq!(from_stop[0].mode, "WALK");
        // The walk alone: the bus buffer is the planner's, not the link's.
        assert_eq!(from_stop[0].minutes, 8);
        assert_eq!(
            (
                from_stop[0].valid_from.as_str(),
                from_stop[0].valid_to.as_str(),
                from_stop[0].days_mask.as_str()
            ),
            ("0000", "2359", "1111111")
        );
        let from_station = &data.fixed_links_from_crs["HXX"];
        assert_eq!(from_station.len(), 1);
        assert_eq!(from_station[0].to_crs, "tiploc:HTRBUS3");
        assert_eq!(from_station[0].minutes, 8);
        // The unlinked stop has no walk either way.
        assert!(!data.fixed_links_from_crs.contains_key("tiploc:KESWICK"));
    }

    #[test]
    fn walk_times_use_the_curated_minutes_only_for_the_same_parent() {
        let mut data = network();
        add_parent_walk_links(
            &mut data,
            &[link("HTRBUS3", "HXX", Some(412))],
            &curated("HTRBUS3", "HXX", 11),
        );
        assert_eq!(data.fixed_links_from_crs["tiploc:HTRBUS3"][0].minutes, 11);
        assert_eq!(data.fixed_links_from_crs["HXX"][0].minutes, 11);

        let mut data = network();
        add_parent_walk_links(
            &mut data,
            &[link("HTRBUS3", "HXX", Some(141))],
            &curated("HTRBUS3", "PAD", 11),
        );
        // The CSV names another parent: the distance decides (141 m -> 4).
        assert_eq!(data.fixed_links_from_crs["HXX"][0].minutes, 4);

        let mut data = network();
        add_parent_walk_links(&mut data, &[link("HTRBUS3", "HXX", None)], &BTreeMap::new());
        // No distance and no curated time: the cautious 15.
        assert_eq!(data.fixed_links_from_crs["HXX"][0].minutes, 15);
    }

    #[test]
    fn stops_that_are_a_stations_tiploc_or_have_no_planner_parent_are_skipped() {
        let mut data = network();
        let linked = add_parent_walk_links(
            &mut data,
            &[
                // Part of Banstead already: a sibling, not a walk.
                link("BANSBUS", "BAD", Some(0)),
                // A parent the planner has no TIPLOC for.
                link("KESWICK", "ZZZ", Some(100)),
                // A stop that is not an end point at all.
                link("NOSUCH", "HXX", Some(100)),
            ],
            &BTreeMap::new(),
        );
        assert_eq!(linked, 0);
        assert!(data.fixed_links_from_crs.is_empty());
    }

    #[test]
    fn a_no_interchange_sentinel_is_lifted_at_linked_stops_only() {
        let mut data = network();
        data.change_time_by_tiploc.insert("HTRBUS3".to_string(), 99);
        data.change_time_by_tiploc.insert("KESWICK".to_string(), 98);
        add_parent_walk_links(
            &mut data,
            &[link("HTRBUS3", "HXX", Some(412))],
            &BTreeMap::new(),
        );
        assert_eq!(minimum_change_time(&data, "HTRBUS3"), ChangeTime::Finite(5));
        assert_eq!(
            minimum_change_time(&data, "KESWICK"),
            ChangeTime::NoInterchange
        );
    }

    /// Bus -> walk -> train. The bus reaches the stop at 10:00; + 5 for
    /// leaving the bus, then 8 minutes' walk = HXX at 10:13; + HXX's
    /// 2-minute change = 10:15. The 10:14 train is missed, the 10:15 taken.
    #[test]
    fn a_bus_then_a_walk_then_a_train() {
        let data = linked_network();
        let connections = sorted(vec![
            conn("BUS1", "WOKING", "HTRBUS3", 540, 600),
            conn("T14", "HTRWAPT", "PADTON", 614, 629),
            conn("T15", "HTRWAPT", "PADTON", 615, 630),
        ]);
        let journey = csa(&connections, &data, "WOK", "PAD", 530).expect("a journey");
        assert_eq!(describe(&journey.legs), ["BUS1", "WALK 8", "T15"]);
        assert_eq!(journey.arrival_min, 630);
        let JourneyLeg::Transfer(walk) = &journey.legs[1] else {
            panic!("the second leg is the walk");
        };
        assert_eq!(
            (walk.from_tiploc.as_str(), walk.to_tiploc.as_str()),
            ("HTRBUS3", "HTRWAPT")
        );
        assert_eq!((walk.departure_min, walk.arrival_min), (605, 613));

        let journeys = raptor(&connections, &data, "WOK", "PAD", 530);
        assert_eq!(describe(&journeys[0].legs), ["BUS1", "WALK 8", "T15"]);
        assert_eq!(journeys[0].changes, 1);

        // To the stop's own code as the destination: the walk is the way
        // in from the station (8 minutes, no bus boarded).
        let back = csa(
            &sorted(vec![conn("T1", "PADTON", "HTRWAPT", 500, 515)]),
            &data,
            "PAD",
            "tiploc:HTRBUS3",
            480,
        )
        .expect("a journey");
        assert_eq!(describe(&back.legs), ["T1", "WALK 8"]);
        assert_eq!(back.arrival_min, 523);
    }

    /// Starting at the stop owes no bus buffer: the walk leaves at once.
    #[test]
    fn a_journey_from_the_stop_walks_straight_to_the_station() {
        let data = linked_network();
        let connections = sorted(vec![
            conn("T09", "HTRWAPT", "PADTON", 609, 624),
            conn("T10", "HTRWAPT", "PADTON", 610, 625),
        ]);
        let journey = csa(&connections, &data, "tiploc:HTRBUS3", "PAD", 600).expect("a journey");
        assert_eq!(describe(&journey.legs), ["WALK 8", "T10"]);
        let journeys = raptor(&connections, &data, "tiploc:HTRBUS3", "PAD", 600);
        assert_eq!(describe(&journeys[0].legs), ["WALK 8", "T10"]);
    }

    /// Train -> walk -> bus. The train reaches HXX at 08:35; 8 minutes'
    /// walk = the stop at 08:43; + the stop's 5-minute change + 5 for
    /// boarding a bus = 08:53. The 08:52 bus is missed, the 08:53 taken.
    #[test]
    fn a_train_then_a_walk_then_a_bus() {
        let data = linked_network();
        let connections = sorted(vec![
            conn("T1", "PADTON", "HTRWAPT", 500, 515),
            conn("BUS2", "HTRBUS3", "WOKING", 532, 590),
            conn("BUS3", "HTRBUS3", "WOKING", 533, 591),
        ]);
        let journey = csa(&connections, &data, "PAD", "WOK", 480).expect("a journey");
        assert_eq!(describe(&journey.legs), ["T1", "WALK 8", "BUS3"]);
        assert_eq!(journey.arrival_min, 591);
        let journeys = raptor(&connections, &data, "PAD", "WOK", 480);
        assert_eq!(describe(&journeys[0].legs), ["T1", "WALK 8", "BUS3"]);
    }

    /// Without the link the same network has no journey: the bus stop and
    /// the station are unconnected places.
    #[test]
    fn an_unlinked_stop_stays_an_end_point_only() {
        let connections = sorted(vec![
            conn("BUS1", "WOKING", "HTRBUS3", 540, 600),
            conn("T15", "HTRWAPT", "PADTON", 615, 630),
        ]);
        assert!(csa(&connections, &network(), "WOK", "PAD", 530).is_none());
        assert!(raptor(&connections, &network(), "WOK", "PAD", 530).is_empty());

        // Keswick is never linked: a train to Penrith does not reach the
        // Keswick bus, even with the Heathrow stop linked.
        let data = linked_network();
        let connections = sorted(vec![
            conn("T2", "PADTON", "PENRITH", 500, 700),
            conn("BUSK", "KESWICK", "WOKING", 760, 900),
        ]);
        assert!(csa(&connections, &data, "PAD", "WOK", 480).is_none());
        // ...though Keswick is still an origin of its own.
        let journey = csa(&connections, &data, "tiploc:KESWICK", "WOK", 700).expect("a journey");
        assert_eq!(describe(&journey.legs), ["BUSK"]);
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;

    async fn connect() -> PgPool {
        let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for db_tests");
        PgPool::connect(&url)
            .await
            .expect("connect to test database")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                build_connections_for_date -- --ignored --test-threads=1`"]
    async fn build_connections_for_date_returns_none_when_nothing_is_published() {
        let pool = connect().await;
        let far_future = NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
        let result = build_connections_for_date(&pool, far_future)
            .await
            .expect("query succeeds");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                build_connections_for_date -- --ignored --test-threads=1`"]
    async fn build_connections_for_date_builds_a_real_connection_from_seeded_rows() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 23).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTUID1', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTUID1', 1, 'MKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");

        let connections = build_connections_for_date(&pool, date)
            .await
            .expect("query succeeds")
            .expect("rows exist for this date");
        assert!(
            connections
                .iter()
                .any(|c| c.uid == "TESTUID1" && c.from_tiploc == "EUSTON")
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTUID1'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                fetch_interchange_data -- --ignored --test-threads=1`"]
    async fn fetch_interchange_data_reads_real_stanox_crs_and_fixed_links_rows() {
        let pool = connect().await;
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes) \
             VALUES ('TEST-IC-STANOX', 'ZZZ', 'ZZZTPL', 'TEST STATION', 1, 7) \
             ON CONFLICT (stanox) DO UPDATE SET change_time_minutes = EXCLUDED.change_time_minutes",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO fixed_links (mode, from_crs, to_crs, minutes, valid_from, valid_to, days_mask, source_sequence) \
             VALUES ('WALK', 'ZZZ', 'YYY', 8, '0000', '2359', '1111111', 1)",
        )
        .execute(&pool)
        .await
        .expect("seed fixed_links");

        let data = fetch_interchange_data(&pool).await.expect("query succeeds");
        assert_eq!(data.change_time_by_tiploc.get("ZZZTPL"), Some(&7));
        assert!(
            data.fixed_links_from_crs
                .get("ZZZ")
                .is_some_and(|links| links.iter().any(|l| l.to_crs == "YYY"))
        );

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-IC-STANOX'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM fixed_links WHERE from_crs = 'ZZZ'")
            .execute(&pool)
            .await
            .ok();
    }

    /// Regression test for the final-review finding this Phase 2 fix round
    /// exists for: `schedule_calling_points_full.tiploc` is now written
    /// normalized (bare) at publish time
    /// (`schedule-reference::publish_schedule_calling_points_full` now
    /// calls `schedule_query::normalize_tiploc`, matching its two sibling
    /// publish functions in the same file), which matches
    /// `stanox_crs.tiploc`'s own bare storage form -- see
    /// `crate::data::queries::crs_for_tiploc`'s own doc comment for the
    /// real, already-hit "roughly a third of all real station TIPLOCs" /
    /// 2026-09-16 "Unknown location" incident this exact mismatch caused
    /// before.
    ///
    /// This test deliberately seeds a genuinely padded TIPLOC value
    /// directly into `schedule_calling_points_full` (bypassing the fixed
    /// publisher entirely, simulating any future regression that
    /// reintroduces an unnormalized write), then walks the full three-hop
    /// path -- `fetch_calling_points_for_date` ->
    /// `fetch_interchange_data` -> `schedule_query::minimum_change_time`
    /// -- to prove `minimum_change_time`'s own defense-in-depth
    /// normalization (not just the publisher fix) makes the padded value
    /// still resolve against the bare-keyed `stanox_crs` row, rather than
    /// silently missing and falling back to the 5-minute default. None of
    /// the three tasks' own individual tests spanned all three hops
    /// together, which is exactly how the original bug went unnoticed.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                a_padded_tiploc_from_calling_points_full_still_matches_bare_stanox_crs_change_time \
                -- --ignored --test-threads=1`"]
    async fn a_padded_tiploc_from_calling_points_full_still_matches_bare_stanox_crs_change_time() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();

        // A padded TIPLOC, exactly as a real CIF schedule-body TIPLOC field
        // carries it (see `schedule_query::tiploc`'s own doc comment) -- 7
        // characters, space-padded, shorter than 7 chars when trimmed.
        let padded_tiploc = "EUSTON ";
        assert_eq!(
            padded_tiploc.len(),
            7,
            "must be genuinely padded, matching real CIF shape"
        );

        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTUID-PAD', 0, $2, 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTUID-PAD', 1, 'MKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .bind(padded_tiploc)
        .execute(&pool)
        .await
        .expect("seed calling points");

        // The matching interchange row is keyed on the BARE form -- real
        // `stanox_crs` storage, per `crs_for_tiploc`'s own doc comment.
        // `change_time_minutes = 9`, distinct from the 5-minute default, so
        // the test can tell a real lookup apart from a silent miss.
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes) \
             VALUES ('TEST-PAD-STANOX', 'EUS', 'EUSTON', 'TEST EUSTON', 1, 9) \
             ON CONFLICT (stanox) DO UPDATE SET change_time_minutes = EXCLUDED.change_time_minutes",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let by_uid = fetch_calling_points_for_date(&pool, date)
            .await
            .expect("query succeeds")
            .expect("rows exist for this date");
        let calling_points = by_uid.get("TESTUID-PAD").expect("seeded schedule present");
        let the_tiploc_from_the_calling_point = calling_points
            .iter()
            .find(|cp| cp.tiploc.trim() == "EUSTON")
            .expect("the seeded, still-padded EUSTON calling point is present")
            .tiploc
            .clone();
        assert_eq!(
            the_tiploc_from_the_calling_point, padded_tiploc,
            "sanity check: the row read back must still be padded -- fetch_calling_points_for_date \
             does no normalization of its own, by design"
        );

        let interchange_data = fetch_interchange_data(&pool).await.expect("query succeeds");

        assert_eq!(
            schedule_query::minimum_change_time(
                &interchange_data,
                &the_tiploc_from_the_calling_point
            ),
            schedule_query::ChangeTime::Finite(9),
            "a padded TIPLOC read back from schedule_calling_points_full must still match its \
             bare-keyed stanox_crs change-time row via minimum_change_time's own normalization, \
             not silently miss and fall back to the 5-minute default"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTUID-PAD'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-PAD-STANOX'")
            .execute(&pool)
            .await
            .ok();
    }
}
