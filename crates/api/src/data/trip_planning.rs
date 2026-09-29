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
    #[allow(dead_code)]
    seq: i16,
    tiploc: String,
    booked_arrival: Option<chrono::NaiveTime>,
    booked_departure: Option<chrono::NaiveTime>,
    day_offset: i16,
}

/// Reads every `schedule_calling_points_full` row for `date`, grouped by
/// `uid` and already ordered by `seq` (the `ORDER BY` below, not an
/// in-memory re-sort) -- exactly the shape [`schedule_query::build_connections`]
/// needs. `None` if no rows exist for `date` at all (no CIF delivery has
/// published this far ahead yet) -- the caller (Phase 5's route handler)
/// maps this to a 404, same "no CIF-derived schedule data has been
/// published for this leg's service date" convention
/// `search_journey_leg_candidates` already establishes.
pub async fn fetch_calling_points_for_date(
    pool: &PgPool,
    date: NaiveDate,
) -> Result<Option<HashMap<String, Vec<CallingPointForConnections>>>> {
    let rows: Vec<CallingPointRow> = sqlx::query_as(
        "SELECT uid, seq, tiploc, booked_arrival, booked_departure, day_offset \
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

    Ok(InterchangeData {
        change_time_by_tiploc,
        tiploc_to_crs,
        crs_to_tiplocs,
        fixed_links_from_crs,
    })
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
        Fut: std::future::Future<Output = Result<Option<T>>> + Send + 'static,
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
        let far_future = chrono::NaiveDate::from_ymd_opt(2099, 1, 1).unwrap();
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 23).unwrap();
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();

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
