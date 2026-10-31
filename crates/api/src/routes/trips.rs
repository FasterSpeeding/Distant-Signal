//! `GET /Trips/plan` -- the read-only journey-planning endpoint. See
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §5.2
//! and
//! docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase5-planning-api-plan.md's
//! own Judgment Call 1 for why this lives under a new `/Trips` prefix, not
//! `/Journeys/*`. Unauthenticated, read-only -- computing a hypothetical
//! itinerary commits nothing and belongs to no user, matching
//! `reference::nearest_stations`'s own public/read-only posture, not
//! `routes::journeys`'s authenticated-write one.

use std::sync::{Arc, LazyLock};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use chrono::{NaiveDate, NaiveTime};
use serde::Deserialize;

use crate::app::App;
use crate::data::station_groups::{self, StationChoice};
use crate::data::{
    operator_groups, planner_operators, tfl_access, transit_planning, trip_plan_live,
    trip_planning, trip_planning_itinerary,
};

/// Hard cap on `?waypoints=`, enforced before any database read (2026-09-25
/// review, High 4b): before it, `?waypoints=YRK,YRK,YRK,...` repeated a few
/// hundred times was a legal query string that turned one unauthenticated
/// GET into hundreds of graph searches.
///
/// Waypoints are now searched as one journey (`trip_planner::staged`,
/// 2026-09-29), whose cost grows with the number of waypoints: every stage
/// keeps its own labels. Measured (release, synthetic day of ~400k
/// connections, `crates/trip-planner/tests/bench_arrive_by.rs`
/// `bench_waypoints`), fastest/CSA took about 10, 50, 86, 170, 197 and
/// 332 ms at 0, 2, 4, 8, 12 and 20 waypoints; options/RAPTOR (6 rounds)
/// 0.85, 1.2, 1.7, 1.6, 1.5 and 1.9 s. The per-segment planner it replaced
/// cost roughly `waypoints + 1` RAPTOR runs for options (about 5-7 s at 8),
/// so 20 -- what the Distant-Signal-MCP `plan_journey` tool allows -- costs
/// less than 8 used to. This is the ceiling and the default; the
/// `TRIP_PLAN_MAX_WAYPOINTS` environment variable (chart
/// `api.tripPlanMaxWaypoints`) can lower it, see [`WAYPOINT_LIMIT`].
const MAX_WAYPOINTS: usize = 20;
const MAX_WAYPOINTS_ENV: &str = "TRIP_PLAN_MAX_WAYPOINTS";

/// The waypoint cap in effect: `TRIP_PLAN_MAX_WAYPOINTS`, clamped to
/// `1..=`[`MAX_WAYPOINTS`], default [`MAX_WAYPOINTS`].
static WAYPOINT_LIMIT: LazyLock<usize> =
    LazyLock::new(|| env_or_default(MAX_WAYPOINTS_ENV, MAX_WAYPOINTS).clamp(1, MAX_WAYPOINTS));

/// Cap on each of `?avoid=`, `?avoidStop=` and `?avoidChange=`, checked
/// before any database read like [`MAX_WAYPOINTS`]. An avoided station adds
/// no search of its own (the restrictions are applied inside the one search
/// per segment), but `avoid` reads every train that runs through it, and an
/// empty segment is explained by re-running it without each list.
const MAX_AVOIDED: usize = 8;

/// Cap on `?via=` (pass-through vias), checked before any database read.
/// Each via multiplies the states the joint search carries (one per
/// waypoint stage and via progress: `(waypoints + 1) * (vias + 1)`), so this
/// bounds the search's size together with [`MAX_WAYPOINTS`]; the measured
/// cost is in
/// docs/superpowers/specs/2026-10-06-trips-plan-via-and-max-changes-design.md.
/// Three is what a ticket routing ("via X, Y and Z") needs in practice.
/// An OR choice (`KGX|EUS`, `group:LON`) counts as ONE via.
pub(crate) const MAX_VIAS: usize = 3;

/// Cap on the stations one OR via stands for (2026-10-07), its groups
/// expanded: room for the eighteen London Terminals (`group:LON`) and a few
/// more. A group is still one via to the search; what grows with its size
/// is the trains that touch a member (they carry pass spans), and three
/// 18-station groups measured at about the cost of three single vias
/// (`bench_group_vias_against_single`; see
/// docs/superpowers/specs/2026-10-07-trips-plan-or-group-vias-design.md).
const MAX_VIA_ALTERNATIVES: usize = 24;

/// Cap on the stations all the vias together stand for: the measured worst
/// case, three 18-station groups.
const MAX_VIA_STATIONS: usize = 54;

/// Guard on an `options` search's size, about states * rounds:
/// `(waypoints + 1) * (2 * vias + 1) * (maxChanges + 2)`. A via counts
/// double: its progress is reached all over the network (any train passing
/// it), where a waypoint stage only fills in once the waypoint is reached;
/// measured, 4 waypoints and 3 vias cost 2-3.5x what 20 waypoints alone do.
///
/// The default bound is 252, twice what 20 waypoints at `maxChanges=4` (the
/// worst case before the ceiling was raised to 6) cost: the user's decision
/// of 2026-10-06, kept when the ceiling went to 12 (2026-10-24, DS-85).
/// Without vias it binds only above `maxChanges=10` (18 waypoints at 11,
/// 17 at 12); with 1 via, `maxChanges` 2/4/6/8/10/12 allow 20/13/9/7/6/5
/// waypoints; with 3 vias, 8/5/3/2/2/1. A request that leaves `maxChanges`
/// out gets the default of 4 only where it fits, else the most that does
/// down to the old default of 2 ([`default_max_changes`]), so no request
/// valid before the default rose is refused. `fastest` (one CSA sweep, no
/// rounds) is not limited beyond [`MAX_WAYPOINTS`] and [`MAX_VIAS`].
///
/// `TRIP_PLAN_MAX_OPTIONS_SEARCH_SIZE` (chart `api.tripPlanMaxOptionsSearchSize`)
/// sets it, clamped to [`MIN_OPTIONS_SEARCH_SIZE`]..=[`MAX_OPTIONS_SEARCH_SIZE`];
/// see [`OPTIONS_SEARCH_SIZE_LIMIT`]. Measured cost:
/// docs/superpowers/specs/2026-10-06-trips-plan-via-and-max-changes-design.md.
const DEFAULT_OPTIONS_SEARCH_SIZE: usize = 252;
/// The lowest bound accepted: every request with no waypoints and no vias
/// still fits at any allowed `maxChanges` (1 * 1 * 14 at 12).
const MIN_OPTIONS_SEARCH_SIZE: usize =
    trip_planning_itinerary::max_rounds(trip_planning_itinerary::MAX_CHANGES_LIMIT) as usize;
/// The highest bound accepted: four times the pre-2026-10-06 worst case,
/// about 4x the latency of 20 waypoints at `maxChanges=4`.
const MAX_OPTIONS_SEARCH_SIZE: usize = 504;
const OPTIONS_SEARCH_SIZE_ENV: &str = "TRIP_PLAN_MAX_OPTIONS_SEARCH_SIZE";

/// The options search-size bound in effect: `TRIP_PLAN_MAX_OPTIONS_SEARCH_SIZE`,
/// clamped to `MIN_OPTIONS_SEARCH_SIZE..=MAX_OPTIONS_SEARCH_SIZE`, default
/// [`DEFAULT_OPTIONS_SEARCH_SIZE`].
static OPTIONS_SEARCH_SIZE_LIMIT: LazyLock<usize> = LazyLock::new(|| {
    env_or_default(OPTIONS_SEARCH_SIZE_ENV, DEFAULT_OPTIONS_SEARCH_SIZE)
        .clamp(MIN_OPTIONS_SEARCH_SIZE, MAX_OPTIONS_SEARCH_SIZE)
});

/// Global cap on trip plans being computed at once (2026-09-25 review, High
/// 4c). This is a per-process concurrency gate, NOT a per-IP rate limit --
/// stated plainly because the two are often conflated: it bounds how much of
/// this box's CPU one endpoint can hold at any instant, and says nothing
/// about how often any single caller may ask.
///
/// A plain `tokio::sync::Semaphore` with `try_acquire`, rather than a tower
/// layer: `tower::limit::ConcurrencyLimitLayer` QUEUES excess requests
/// (unbounded, since axum awaits readiness through `oneshot`) instead of
/// shedding them, which converts a CPU flood into a memory flood plus
/// ever-growing latency. The per-client limit is separate: `crate::rate_limit`
/// keys this route on the frontend-set `X-Real-IP`. Shedding with a 503
/// is the honest behaviour for work this expensive: the caller learns
/// immediately, and every other route -- healthcheck included -- keeps its
/// worker threads.
///
/// 4 permits: enough that ordinary interactive use never sees a 503 (a plan
/// takes well under a second), small enough that the blocking pool retains
/// ample capacity for the PDF parser and sqlx's own work.
static PLAN_SLOTS: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(4));

/// How many service dates' graphs [`GRAPH_CACHE`] keeps (each ~100 MB);
/// 0 disables it. `TRIP_PLAN_GRAPH_CACHE_DATES`, default 2.
const GRAPH_CACHE_DATES_ENV: &str = "TRIP_PLAN_GRAPH_CACHE_DATES";
const DEFAULT_GRAPH_CACHE_DATES: usize = 2;
/// Oldest a cached graph may be before it is rebuilt even with no new
/// publish marker. `TRIP_PLAN_GRAPH_CACHE_MAX_AGE_SECS`, default 600.
const GRAPH_CACHE_MAX_AGE_ENV: &str = "TRIP_PLAN_GRAPH_CACHE_MAX_AGE_SECS";
const DEFAULT_GRAPH_CACHE_MAX_AGE_SECS: u64 = 600;

/// TRIPS-1: built graphs, shared across requests for the same date. See
/// [`trip_planning::GraphCache`].
static GRAPH_CACHE: LazyLock<Arc<trip_planning::GraphCache<trip_planning::PlanningGraph>>> =
    LazyLock::new(|| {
        // Off in unit tests: the DB tests reseed the same date with different
        // rows and must each see their own.
        let default_dates = if cfg!(test) {
            0
        } else {
            DEFAULT_GRAPH_CACHE_DATES
        };
        let dates = env_or_default(GRAPH_CACHE_DATES_ENV, default_dates);
        let max_age = env_or_default(GRAPH_CACHE_MAX_AGE_ENV, DEFAULT_GRAPH_CACHE_MAX_AGE_SECS);
        Arc::new(trip_planning::GraphCache::new(
            dates,
            std::time::Duration::from_secs(max_age),
        ))
    });

/// A numeric env var, or `default` (with a warning) when unset or invalid.
fn env_or_default<T: std::str::FromStr + std::fmt::Display + Copy>(name: &str, default: T) -> T {
    match std::env::var(name) {
        Err(_) => default,
        Ok(raw) => raw.trim().parse().unwrap_or_else(|_| {
            tracing::warn!(name, raw, %default, "invalid value; using the default");
            default
        }),
    }
}

pub fn router() -> crate::app::Router {
    crate::app::Router::new()
        .route("/Trips/plan", axum::routing::get(get_trip_plan))
        .route(
            "/Trips/station-groups",
            axum::routing::get(get_station_groups),
        )
        .route(
            "/Trips/operator-groups",
            axum::routing::get(get_operator_groups),
        )
}

/// `GET /Trips/operator-groups` (2026-10-31, planner operator filtering
/// spec §6.4): the named operator groups `/Trips/plan`'s `operator` and
/// `avoidOperator` accept as `group:NAME`, the `ds:` operators (the DLR,
/// London Trams, Metrolink, NET), and display names for the timetable codes
/// `tocs` doesn't name. Unauthenticated, no database read: compiled in.
async fn get_operator_groups() -> (
    [(axum::http::header::HeaderName, &'static str); 1],
    Json<serde_json::Value>,
) {
    (
        [(
            axum::http::header::CACHE_CONTROL,
            STATION_GROUPS_CACHE_CONTROL,
        )],
        Json(operator_groups::catalogue()),
    )
}

/// `Cache-Control` for [`get_station_groups`]: the groups are compiled in
/// and change only with a deploy; an hour, as for the other reference
/// catalogues (`routes::island_of_ireland::CATALOGUE_CACHE_CONTROL`).
const STATION_GROUPS_CACHE_CONTROL: &str = "public, max-age=3600";

/// `GET /Trips/station-groups` (2026-10-07): every named station group
/// `/Trips/plan` accepts as `group:NAME` in `via`, `waypoints` and the
/// avoid lists, with its members, for a picker. Unauthenticated, no
/// database read: `reference-data/station-groups.csv`, compiled in.
async fn get_station_groups() -> (
    [(axum::http::header::HeaderName, &'static str); 1],
    Json<serde_json::Value>,
) {
    (
        [(
            axum::http::header::CACHE_CONTROL,
            STATION_GROUPS_CACHE_CONTROL,
        )],
        Json(serde_json::json!({ "groups": station_groups::catalogue() })),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TripPlanParams {
    origin: String,
    destination: String,
    /// Comma-separated ordered CRS codes, e.g. `?waypoints=YRK,NCL`. Absent
    /// or empty means no waypoints -- a direct origin->destination plan.
    /// An entry may be an OR choice (2026-10-07: `KGX|EUS`, `group:LON`),
    /// a stop at ANY of its stations; see [`parse_waypoints`].
    #[serde(default)]
    waypoints: Option<String>,
    date: NaiveDate,
    #[serde(default)]
    depart_after: Option<NaiveTime>,
    /// `?arriveBy=HH:MM`: the latest acceptable arrival at the destination.
    /// Mutually exclusive with `departAfter` (a 400 when both are given).
    #[serde(default)]
    arrive_by: Option<NaiveTime>,
    /// `?dayOffset=0|1` (2026-10-19): `1` puts `departAfter`/`arriveBy` on
    /// the day after `date` -- `date=FRI&arriveBy=00:30&dayOffset=1` is "by
    /// 00:30 on Saturday, on Friday's trains". Kept as a raw string for
    /// [`parse_day_offset`]'s own 400.
    #[serde(default)]
    day_offset: Option<String>,
    /// `?acrossMidnight=true` (2026-10-20), with `dayOffset=1` only: also
    /// search `date + 1`'s trains from the same clock time and merge, so a
    /// time just after midnight sees the trains of both service dates
    /// running then. See [`parse_across_midnight`].
    #[serde(default)]
    across_midnight: Option<String>,
    /// `?avoid=CRS[,CRS]`: never call at, run through, change at or walk
    /// via these stations. See [`trip_planning_itinerary::build_restrictions`].
    #[serde(default)]
    avoid: Option<String>,
    /// `?avoidStop=CRS[,CRS]`: never ride a train that CALLS at these
    /// stations (running through without stopping is fine).
    #[serde(default)]
    avoid_stop: Option<String>,
    /// `?avoidChange=CRS[,CRS]`: never board, alight or change at these
    /// stations (staying aboard a train calling there is fine).
    #[serde(default)]
    avoid_change: Option<String>,
    /// `?via=CRS[,CRS]` (2026-10-06): stations to pass through in order,
    /// calling there or not; each may be an OR choice `A|B|group:LON`
    /// (2026-10-07). See [`parse_vias`] and
    /// [`trip_planning_itinerary::build_vias`].
    #[serde(default)]
    via: Option<String>,
    #[serde(default = "default_results")]
    results: String,
    /// `?alternatives=true|false` (default `true`; DS-196): with
    /// `results=options`, add alternative routes to the Pareto set. Ignored
    /// with `results=fastest`. Validated by [`parse_alternatives`].
    #[serde(default)]
    alternatives: Option<String>,
    /// Optional interchange cap, `?maxChanges=0`..`=12` (inclusive). Absent
    /// or empty means [`trip_planning_itinerary::DEFAULT_MAX_CHANGES`] (4;
    /// 2 before 2026-10-24), see [`default_max_changes`]. Kept as
    /// a raw string and validated by [`parse_max_changes`] rather than
    /// deserialized straight into an integer, so a bad value gets this
    /// route's own clear 400 message rather than axum's generic
    /// query-rejection text.
    #[serde(default)]
    max_changes: Option<String>,
    /// `?live=true|false` (default `true`): apply the live overlay
    /// (`data::trip_plan_live`). `false` returns exactly the timetable-only
    /// response. Validated by [`parse_live`].
    #[serde(default)]
    live: Option<String>,
    /// `?transit=true|false` (default `false`; DS-164): plan with the
    /// transit networks in `API_TRIP_PLAN_TRANSIT_NETWORKS` (trams as
    /// `kind: "transit"` legs). Ignored while that list is empty. A tram
    /// stop as `origin` or `destination` opts in too. See [`parse_transit`].
    #[serde(default)]
    transit: Option<String>,
    /// `?operator=VT,group:GTR,ds:DLR` (2026-10-31): ride only these
    /// operators -- their trains, buses, ferries, links and connecting
    /// rides; walks always. See [`parse_operator_filter`].
    #[serde(default)]
    operator: Option<String>,
    /// `?avoidOperator=LT` (2026-10-31): never these operators, in any
    /// way. Not together with `operator`.
    #[serde(default)]
    avoid_operator: Option<String>,
}

/// A request's operator filter (2026-10-31): `?operator=` (only) or
/// `?avoidOperator=`.
#[derive(Debug, Clone)]
struct OperatorFilter {
    only: bool,
    list: operator_groups::OperatorList,
}

impl OperatorFilter {
    /// The search rule: one operator, every way of riding it (spec §5.1).
    fn rule(&self) -> trip_planner::OperatorRule {
        if self.only {
            trip_planner::OperatorRule::only(self.list.mask())
        } else {
            trip_planner::OperatorRule::avoid(self.list.mask())
        }
    }

    /// The listed operators with nothing in any of `graphs`.
    fn not_running(&self, graphs: &[&trip_planning::PlanningGraph]) -> Vec<String> {
        self.list
            .operators
            .iter()
            .filter(|id| !graphs.iter().any(|graph| graph.running.contains(**id)))
            .filter_map(|id| id.code())
            .collect()
    }
}

/// Parses and checks `?operator=` and `?avoidOperator=` (at most one), a
/// `400` for: both; a malformed entry, an unknown `group:` or `ds:` name,
/// more than 8 entries ([`operator_groups::parse_operator_list`]); an ATOC
/// code neither `tocs` nor the timetable of `service_dates` knows (the
/// Trains search's rule, `reference::known_operator_codes`). A known code
/// that runs nothing those days is fine (`operatorsNotRunning`).
async fn parse_operator_filter(
    app: &App,
    only: Option<&str>,
    avoid: Option<&str>,
    service_dates: &[NaiveDate],
) -> Result<Option<OperatorFilter>, (StatusCode, String)> {
    let bad = |message: String| (StatusCode::BAD_REQUEST, message);
    let only = operator_groups::parse_operator_list("operator", only).map_err(bad)?;
    let avoid = operator_groups::parse_operator_list("avoidOperator", avoid).map_err(bad)?;
    let filter = match (only, avoid) {
        (Some(_), Some(_)) => {
            return Err(bad(
                "operator and avoidOperator can't be used together: choose one".to_string(),
            ));
        }
        (Some(list), None) => OperatorFilter { only: true, list },
        (None, Some(list)) => OperatorFilter { only: false, list },
        (None, None) => return Ok(None),
    };
    let codes = filter.list.named_atoc_codes();
    if !codes.is_empty() {
        let known =
            crate::data::reference::known_operator_codes(&app.database, &codes, service_dates)
                .await
                .map_err(internal_error("check operator codes"))?;
        let unknown: Vec<&str> = codes
            .iter()
            .filter(|code| !known.contains(*code))
            .map(String::as_str)
            .collect();
        match unknown.as_slice() {
            [] => {}
            [code] => {
                return Err(bad(format!(
                    "operator {code} is not a known train operator code"
                )));
            }
            codes => {
                return Err(bad(format!(
                    "operators {} are not known train operator codes",
                    codes.join(", ")
                )));
            }
        }
    }
    metrics::counter!(
        common::metrics::metric_name("api_trip_plan_operator_filter_total"),
        "mode" => if filter.only { "only" } else { "avoid" }
    )
    .increment(1);
    Ok(Some(filter))
}

/// The live overlay's configuration, read once from the environment.
static LIVE_CONFIG: LazyLock<trip_plan_live::LiveConfig> =
    LazyLock::new(trip_plan_live::LiveConfig::from_env);

/// [`LIVE_CONFIG`], with a test's [`set_in_search_for_tests`] applied.
fn live_config() -> std::borrow::Cow<'static, trip_plan_live::LiveConfig> {
    #[cfg(test)]
    if let Some(in_search) = IN_SEARCH_OVERRIDE.with(std::cell::Cell::get) {
        return std::borrow::Cow::Owned(trip_plan_live::LiveConfig {
            in_search,
            ..LIVE_CONFIG.clone()
        });
    }
    std::borrow::Cow::Borrowed(&*LIVE_CONFIG)
}

#[cfg(test)]
thread_local! {
    static IN_SEARCH_OVERRIDE: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Sets [`trip_plan_live::LiveConfig::in_search`] on the current thread
/// until the returned guard is dropped (as `pin_london_now_for_tests`).
#[cfg(test)]
#[must_use = "the override is lifted as soon as the guard is dropped"]
fn set_in_search_for_tests(in_search: bool) -> InSearchOverride {
    IN_SEARCH_OVERRIDE.with(|cell| cell.set(Some(in_search)));
    InSearchOverride(())
}

#[cfg(test)]
struct InSearchOverride(());

#[cfg(test)]
impl Drop for InSearchOverride {
    fn drop(&mut self) {
        IN_SEARCH_OVERRIDE.with(|cell| cell.set(None));
    }
}

/// One date's in-search snapshot (`trip_plan_live::LiveSnapshot`), reused
/// by every request on the same graph for `snapshot_ttl_seconds`.
struct CachedSnapshot {
    date: NaiveDate,
    graph: std::sync::Weak<trip_planning::PlanningGraph>,
    built: std::time::Instant,
    snapshot: Arc<trip_plan_live::LiveSnapshot>,
}

/// The cached snapshots. Behind an async mutex held across the read and
/// build, so concurrent requests for a stale date build it once.
static LIVE_SNAPSHOTS: LazyLock<tokio::sync::Mutex<Vec<CachedSnapshot>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(Vec::new()));

/// `date`'s in-search snapshot on `graph`: the cached one while it is
/// younger than `snapshot_ttl_seconds` and built on this very graph, else
/// read (two queries) and built (one pass over the day array, on the
/// blocking pool) now.
async fn live_snapshot(
    app: &App,
    graph: &Arc<trip_planning::PlanningGraph>,
    date: NaiveDate,
    config: &trip_plan_live::LiveConfig,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<Arc<trip_plan_live::LiveSnapshot>> {
    let ttl = std::time::Duration::from_secs(config.snapshot_ttl_seconds);
    let mut cache = LIVE_SNAPSHOTS.lock().await;
    cache.retain(|cached| cached.built.elapsed() < ttl && cached.graph.strong_count() > 0);
    if let Some(cached) = cache.iter().find(|cached| {
        cached.date == date
            && cached
                .graph
                .upgrade()
                .is_some_and(|cached| Arc::ptr_eq(&cached, graph))
    }) {
        return Ok(cached.snapshot.clone());
    }
    let started = std::time::Instant::now();
    let states = trip_plan_live::fetch_snapshot_states(&app.database, date, config, now).await?;
    let build_graph = graph.clone();
    let build_config = config.clone();
    let snapshot = Arc::new(
        tokio::task::spawn_blocking(move || {
            trip_plan_live::LiveSnapshot::build(
                &build_graph.connections,
                date,
                &states,
                &build_config,
                now,
            )
        })
        .await?,
    );
    metrics::histogram!(common::metrics::metric_name(
        "api_trip_plan_live_snapshot_build_seconds"
    ))
    .record(started.elapsed().as_secs_f64());
    metrics::gauge!(common::metrics::metric_name(
        "api_trip_plan_live_snapshot_trains"
    ))
    .set(f64::from(
        u32::try_from(snapshot.trains()).unwrap_or(u32::MAX),
    ));
    if !ttl.is_zero() {
        cache.push(CachedSnapshot {
            date,
            graph: Arc::downgrade(graph),
            built: std::time::Instant::now(),
            snapshot: snapshot.clone(),
        });
    }
    Ok(snapshot)
}

fn default_results() -> String {
    "fastest".to_string()
}

/// `GET /Trips/plan?origin=&destination=&date=[&waypoints=][&via=]
/// [&departAfter=|&arriveBy=][&dayOffset=0|1][&acrossMidnight=true][&avoid=][&avoidStop=][&avoidChange=]
/// [&results=fastest|options][&alternatives=true|false][&maxChanges=0..12][&live=true|false]`.
///
/// - `dayOffset=1` (2026-10-19): the `departAfter`/`arriveBy` time is on
///   the morning after `date`, still on `date`'s trains -- "arrive by 00:30
///   on Saturday" is `date=<Friday>&arriveBy=00:30&dayOffset=1`. The
///   top-level `arriveBy` and (new) `departAfter` echoes carry it as
///   `{time, dayOffset}`. See
///   docs/superpowers/specs/2026-10-19-trips-plan-arrive-by-design.md.
/// - `acrossMidnight=true` (2026-10-20, with `dayOffset=1` only): also
///   plan `date + 1`'s trains from the same clock time, concurrently, and
///   merge (`data::trip_plan_overnight`): one journey set, deduplicated,
///   with the usual ordering rules, every time on `date`'s clock. The
///   top-level `serviceDates` lists the dates searched.
///
/// - `arriveBy=HH:MM` (2026-09-29): instead of the earliest arrival after
///   `departAfter` (exclusive with it: both given is a 400), the
///   LATEST-DEPARTING itineraries arriving at the destination by this time
///   (a backward Connection Scan, `trip_planner::reverse`). `fastest`: the
///   single latest departure that makes it; `options`: for each number of
///   changes up to `maxChanges`, the latest departure that makes it (one
///   itinerary per change count, fewest changes first). With waypoints, the
///   LAST segment is searched to arrive by `arriveBy` and each earlier one
///   to arrive by the next one's latest departure less the waypoint's
///   minimum change time (the mirror of the depart-after chain); each
///   segment echoes the deadline it was searched for as `arriveBy`, and
///   `departAfter` is then `null`. With `live`, an itinerary whose live
///   arrival is after its segment's `arriveBy` is `liveFeasible: false`, and
///   that triggers a re-plan within the usual budget.
/// - `avoid`, `avoidStop`, `avoidChange` (2026-09-29): comma-separated CRS
///   lists (at most [`MAX_AVOIDED`] each), applied to every segment:
///   `avoid` = never call at, pass through, change at or walk via;
///   `avoidStop` = never ride a train that calls there; `avoidChange` =
///   never board, alight or change there. An unknown code, or an avoided
///   origin, destination or waypoint, is a 400. The semantics of `avoid` and
///   `avoidStop` are Skye's `train-mcp`'s; see
///   docs/superpowers/specs/2026-09-29-trips-plan-arrive-by-avoid-design.md.
///   An entry may also be a named group (`group:LON`, 2026-10-07), avoiding
///   every member and counting as one entry; `|` is a 400 there.
/// - `waypoints` (2026-09-29): the trip is planned as ONE journey calling at
///   every waypoint in order (`trip_planner::staged`), split into one
///   itinerary per segment. Staying aboard a train through a waypoint is no
///   change (`continuesPreviousTrain: true` on the onward part), and
///   `maxChanges` counts the whole journey's changes. The segments'
///   `itineraries` are aligned -- `segments[s].itineraries[j]` is part `s`
///   of journey `j` -- and the top-level `journeys` summarises each. A
///   waypoint repeating the origin, destination or previous waypoint is a
///   400.
/// - `via` (2026-10-06): comma-separated CRS codes (at most [`MAX_VIAS`]),
///   or a bus stop's or ferry terminal's `tiploc:` code as for the other
///   lists (2026-10-07; satisfied only by a call or a walk there: no train
///   runs through a bus stop),
///   stations every journey must pass through in this order -- staying
///   aboard a train that calls there or runs through without stopping,
///   or changing or walking there. The `via` of Skye's `train-mcp` and the
///   Distant-Signal-MCP's `plan_journey`. Vias and waypoints are separate
///   ordered lists that may interleave. Running through is seen only where
///   CIF records a passing point (timing points); elsewhere only a call
///   counts. Each journey's `viaSatisfiedBy` names the leg that passed each
///   via. An unknown code, a via equal to the origin or the destination,
///   the same via twice in a row, or a via that is also in `avoid`, is a
///   400. OR choices (2026-10-07): one via may list alternatives separated
///   by `|` and named groups (`via=KGX|EUS`, `via=group:LON`), and is
///   passed by ANY of them; `viaSatisfiedBy[k].matchedCrs` names which. See
///   [`parse_vias`], [`check_via_conflicts`] and
///   docs/superpowers/specs/2026-10-07-trips-plan-or-group-vias-design.md.
/// - `origin`/`destination` may be a `TfL` Tube, DLR or tram stop id
///   (`940GZZLUUXB`, 2026-10-23, DS-135): the plan runs from or to the best
///   of the stop's candidate National Rail stations, with an access leg (a
///   `TUBE`, `METRO` or `WALK` transfer carrying `access`, its minutes an
///   estimate) between the stop and the station, and the response gains a
///   top-level `access` listing the candidates. The stop becomes a virtual
///   end with one-way links on a per-request copy of the interchange data
///   (`trip_planner::access`, `data::tfl_access`); the cached graph is not
///   changed, and a station-only request does no extra work. See
///   docs/superpowers/specs/2026-10-23-trips-plan-tfl-stop-ends-design.md.
/// - A segment with no itineraries carries `noResultReason`
///   (`{constraint, values, message}`, see
///   [`trip_planning_itinerary::NoResultReason`]) naming the constraint that
///   made it infeasible -- still a 200, as before.
///
/// - `live` (default `true`): for today's or yesterday's service date, TRUST
///   and Darwin facts are applied and the plan re-run while they change it
///   (at most `TRIP_PLAN_LIVE_MAX_REPLANS` extra times): cancelled trains and
///   calls are withdrawn, known delays shift times, and legs carry `live`.
///   See docs/superpowers/specs/2026-09-28-trips-plan-live-overlay-design.md.
///   `live=false` is exactly the timetable-only response.
///
/// - `results=fastest` (default): one earliest-arrival itinerary per segment
///   (CSA). In this mode `maxChanges` is NOT a hard limit: the fastest
///   itinerary is returned even if it needs more than `maxChanges` changes,
///   and is then flagged `exceedsRecommendedChanges: true` (`false` when
///   within the cap). `cappedByMaxChanges` is always `false` in this mode.
///   Callers that need a strict limit must use `results=options`.
/// - `results=options`: a Pareto set (arrival time vs. changes, RAPTOR) of
///   itineraries with at most `maxChanges` changes each (a hard limit;
///   over-cap itineraries are never returned), plus
///   `cappedByMaxChanges: true` when a strictly faster itinerary needing more
///   changes exists. Then (2026-10-30, DS-196; `alternatives=false` turns
///   it off) up to two alternative routes: other corridors or stations
///   within a detour bound of the best, found by banning the kept routes'
///   calls and changes (`trip_planner::options`), after the Pareto set.
///   Every journey carries `labels` (`fastest` or, arrive-by,
///   `leavesLatest`; `fewestChanges`;
///   `alternative` with `viaCrs`), on `journeys[j]` and each segment's
///   `itineraries[j]`.
/// - `maxChanges`: integer `0..=`[`trip_planning_itinerary::MAX_CHANGES_LIMIT`]
///   (12 since 2026-10-24, 6 from 2026-10-06, 4 before); absent/empty =>
///   [`trip_planning_itinerary::DEFAULT_MAX_CHANGES`] (4 since 2026-10-24,
///   2 before), or less where `options`' search-size guard needs it (see
///   [`default_max_changes`]). Anything else (out
///   of range, negative, non-integer) is a 400 naming the allowed range,
///   rejected before any database read. The effective value is echoed back
///   as the response's top-level `maxChanges`. With `results=options`, the
///   combination of waypoints, vias and `maxChanges` must also fit
///   [`OPTIONS_SEARCH_SIZE_LIMIT`] (a 400 otherwise).
/// - `operator` / `avoidOperator` (2026-10-31): ride only, or never, these
///   operators, every way of riding them (trains, buses, ferries, links,
///   access rides; walks always pass). See [`parse_operator_filter`] for
///   the 400s and `data::planner_operators` for who runs what. The filter
///   is one more rule in the searches' `Restrictions`; the options
///   search-size guard is unchanged.
///
/// **Read this before adding work to this handler.** One request here reads
/// every `schedule_calling_points_full` row for the requested date (hundreds
/// of thousands), builds and sorts the whole day's connections graph in
/// memory, and then runs one graph search per leg. That is by far the most
/// expensive thing this unauthenticated API can be asked to do, so four
/// separate bounds apply, all of them load-bearing (2026-09-25 review, High
/// 4) and none of them a substitute for another:
///
/// 1. [`WAYPOINT_LIMIT`] (at most [`MAX_WAYPOINTS`]), checked BEFORE any
///    database read -- bounds how many stages the one joint search carries
///    (see [`MAX_WAYPOINTS`] for the measured cost). Rejected requests cost
///    a string split, not a query.
/// 2. [`trip_planning_itinerary::MAX_CHANGES_LIMIT`] and
///    [`OPTIONS_SEARCH_SIZE_LIMIT`], also checked before any database read --
///    bound how deep and how wide each `options`-mode search can go. RAPTOR
///    does one full sweep of the day's connections per round, and runs
///    `maxChanges + 2` rounds at most (fewer if a round improves nothing): 14
///    sweeps at `maxChanges=12`, 6 at the default of 4. On prod's timetable
///    every search measured had stopped improving by its 8th round (DS-85,
///    2026-10-24), so a cap above 6 costs only searches that don't converge.
///    Each sweep carries one label set per (waypoint stage, via progress)
///    state, `(waypoints + 1) * (vias + 1)` of them, so the work is about
///    states * rounds (a via state weighing about two waypoint states); the
///    guard keeps that at or below twice what 20 waypoints at
///    `maxChanges=4` (the most allowed before 2026-10-06) already cost.
///    (The whole-day read and graph build, which dominate memory, are
///    unchanged, and `fastest` mode's single CSA scan doesn't depend on
///    `maxChanges`; with the live overlay, `options` re-plans at most once.)
///    Per-search memory is one label set per round and state.
/// 3. [`PLAN_SLOTS`], held across the whole read-plus-compute body -- bounds
///    how many of these can be in flight at once, so the peak is a few
///    graphs' worth of memory and a few threads, not one per connection. The
///    permit is MOVED INTO the blocking task rather than held by this async
///    fn: `spawn_blocking` work can't be cancelled, so if a client
///    disconnected and axum dropped this handler's future mid-search, a
///    permit held here would be released while the search kept burning a
///    blocking thread -- letting connect-then-disconnect callers stack up
///    unbounded concurrent searches past the cap.
/// 4. `spawn_blocking` around the graph build and the searches -- keeps
///    minutes of synchronous CPU off the tokio worker threads. Without it, a
///    handful of concurrent requests starved every async task in the process,
///    including `/public/health`, so the API looked dead rather than slow and
///    orchestration would restart a pod that was merely busy. Same treatment,
///    for the same reason, as `routes::train`'s PDF ticket parser.
/// 5. (TRIPS-1) [`GRAPH_CACHE`]: the whole-day read and graph build happen
///    once per date per schedule publish (or per cache max-age), not once per
///    request, and only one build runs at a time. A request for a cached date
///    costs one marker read, the searches and the leg-details read.
/// 6. (Live overlay, 2026-09-28) at most `TRIP_PLAN_LIVE_MAX_REPLANS` (3)
///    extra planning passes, each only when newly read live data changed the
///    overlay, and at most `TRIP_PLAN_LIVE_MAX_TRAINS` (60) trains read, in
///    batches of a few queries per pass. All passes run under the same
///    permit. The searches now start at their own departure time rather than
///    at 00:00, which more than pays for a re-plan on a daytime query.
async fn get_trip_plan(
    State(app): State<App>,
    Query(params): Query<TripPlanParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let result = plan_trip_request(app, params).await;
    // Usage analytics, phase 1 (DS-190): the outcome only, never the
    // stations asked about.
    crate::usage_events::FeatureEvent::record_result(
        &result,
        |Json(body)| {
            if body["journeys"].as_array().is_some_and(|j| !j.is_empty()) {
                crate::usage_events::FeatureEvent::TripPlanResults
            } else {
                crate::usage_events::FeatureEvent::TripPlanNoRoute
            }
        },
        crate::usage_events::FeatureEvent::TripPlanRejected,
        crate::usage_events::FeatureEvent::TripPlanError,
    );
    result
}

/// [`get_trip_plan`]'s work.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear; splitting it would scatter its shared state across helpers"
)]
async fn plan_trip_request(
    app: App,
    params: TripPlanParams,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    if params.results != "fastest" && params.results != "options" {
        return Err((
            StatusCode::BAD_REQUEST,
            "results must be 'fastest' or 'options'".to_string(),
        ));
    }

    let waypoints = parse_waypoints(params.waypoints.as_deref())?;
    let requested_max_changes = parse_max_changes(params.max_changes.as_deref())?;
    let live_requested = parse_live(params.live.as_deref())?;
    let alternatives = parse_alternatives(params.alternatives.as_deref())?;
    let day_offset = parse_day_offset(
        params.day_offset.as_deref(),
        params.depart_after.is_some() || params.arrive_by.is_some(),
    )?;
    let time = parse_time_bound(params.depart_after, params.arrive_by, day_offset)?;
    let across_midnight = parse_across_midnight(params.across_midnight.as_deref(), day_offset)?;
    // A CRS, or a bus stop's or ferry terminal's `tiploc:` code.
    let mut origin = common::location_naming::normalize_location_code(&params.origin);
    let mut destination = common::location_naming::normalize_location_code(&params.destination);
    // Transit networks (DS-164): only while the switch lists any. A tram
    // stop end (either NaPTAN spelling, or a platform) becomes its station
    // node and opts in, as `transit=true` does; checked before any graph is
    // built.
    let transit_networks = transit_planning::enabled_networks();
    let transit_requested = parse_transit(params.transit.as_deref(), !transit_networks.is_empty())?;
    let transit_ends = resolve_transit_ends(
        &app,
        &transit_networks,
        transit_requested,
        &mut origin,
        &mut destination,
    )
    .await?;
    let transit = !transit_networks.is_empty()
        && (transit_requested || transit_ends.iter().any(Option::is_some));
    let avoid_choices = AvoidChoices {
        avoid: parse_station_list("avoid", params.avoid.as_deref())?,
        avoid_stop: parse_station_list("avoidStop", params.avoid_stop.as_deref())?,
        avoid_change: parse_station_list("avoidChange", params.avoid_change.as_deref())?,
    };
    // Single-station waypoints keep their avoid-list 400; an OR choice's
    // avoided members are dropped instead (`apply_waypoint_choices`).
    let single_waypoints: Vec<String> = waypoints
        .iter()
        .filter(|waypoint| waypoint.is_single_station())
        .map(|waypoint| waypoint.label.clone())
        .collect();
    check_avoid_conflicts(&avoid_choices, &origin, &single_waypoints, &destination)?;
    let avoid = avoid_choices.applied();
    let waypoint_codes = apply_waypoint_choices(&waypoints, &avoid)?;
    let via_choices = parse_vias(params.via.as_deref())?;
    check_via_conflicts(&via_choices, &avoid, &origin, &destination)?;
    if transit {
        check_no_transit_stops(
            waypoints
                .iter()
                .chain(&via_choices)
                .chain(avoid_choices.all()),
        )?;
    }
    // A group via is ONE via to the search (one progress step), measured to
    // cost about what a single-station via does (`bench_group_vias_against_single`).
    let max_changes = requested_max_changes.unwrap_or_else(|| {
        default_max_changes(
            &params.results,
            waypoints.len(),
            via_choices.len(),
            *OPTIONS_SEARCH_SIZE_LIMIT,
        )
    });
    check_options_search_size(
        &params.results,
        waypoints.len(),
        via_choices.len(),
        max_changes,
        *OPTIONS_SEARCH_SIZE_LIMIT,
    )?;
    let station_groups = used_station_groups(
        via_choices
            .iter()
            .chain(&waypoints)
            .chain(avoid_choices.all()),
    );
    let has_waypoint_choice = waypoints.iter().any(|w| !w.is_single_station());
    let waypoints: Vec<String> = waypoints.into_iter().map(|w| w.label).collect();
    let vias: Vec<String> = via_choices.iter().map(|via| via.label.clone()).collect();
    let via_codes: Vec<Vec<String>> = via_choices.into_iter().map(|via| via.codes).collect();
    // Operator filters (2026-10-31): parsed and checked before the permit,
    // like the station lists.
    let service_dates_searched: Vec<NaiveDate> = std::iter::once(params.date)
        .chain(params.date.succ_opt().filter(|_| across_midnight))
        .collect();
    let operator_filter = parse_operator_filter(
        &app,
        params.operator.as_deref(),
        params.avoid_operator.as_deref(),
        &service_dates_searched,
    )
    .await?;
    // A `TfL` stop end (DS-135): its access candidates, read before the
    // permit like the other checks (a few hundred `tfl_stops` rows). A tram
    // stop end of a planner network is a graph node instead (spec §12.2).
    let access = load_access_ends(
        &app,
        if transit_ends[0].is_some() {
            ""
        } else {
            &origin
        },
        if transit_ends[1].is_some() {
            ""
        } else {
            &destination
        },
    )
    .await?;

    // Acquired BEFORE the reads below, not just around the search: the
    // whole-day row read and the graph built from it are the memory half of
    // this endpoint's cost, and admitting unbounded concurrent requests to
    // that and only serialising the CPU afterwards would still let a few
    // callers exhaust this process's memory. `try_acquire`, so an overloaded
    // process sheds load immediately instead of queueing unboundedly.
    let Ok(permit) = PLAN_SLOTS.try_acquire() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "too many trip plans are being computed right now; please retry in a moment"
                .to_string(),
        ));
    };

    let permit = Arc::new(permit);
    let marker = graph_marker(&app, &transit_networks).await?;
    let date = params.date;
    let Some(graph) = load_graph(&app, date, marker).await? else {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "no CIF-derived schedule data has been published for {} yet",
                params.date
            ),
        ));
    };
    if transit {
        check_transit_ends_planned(&graph, &transit_ends)?;
    }
    let access = match access {
        Some(mut access) => {
            access
                .retain_known(planning_interchange(&graph, transit))
                .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))?;
            Some(Arc::new(access))
        }
        None => None,
    };
    let inputs = DateInputs {
        origin,
        destination,
        waypoints,
        waypoint_codes: has_waypoint_choice.then_some(waypoint_codes),
        avoid,
        vias,
        via_codes,
        results: params.results.clone(),
        alternatives,
        max_changes,
        live_requested,
        operator_rule: operator_filter.as_ref().map(OperatorFilter::rule),
        access: access.clone(),
        transit,
    };
    // `acrossMidnight` (2026-10-20): the next service date's trains too,
    // from the same clock time, merged in (`data::trip_plan_overnight`).
    // The two dates are planned concurrently, under this request's one
    // permit, so the wait is the slower date's rather than the sum. A next
    // date with no published timetable just adds nothing.
    let next_date = date.succ_opt().filter(|_| across_midnight);
    let second_plan = async {
        let Some(next) = next_date else {
            return Ok(None);
        };
        let Some(next_graph) = load_graph(&app, next, marker).await? else {
            return Ok(None);
        };
        plan_on_date(
            &app,
            &next_graph,
            &inputs,
            next,
            time.a_day_earlier(),
            &permit,
        )
        .await
        .map(Some)
    };
    let (first, second) = tokio::try_join!(
        plan_on_date(&app, &graph, &inputs, date, time, &permit),
        second_plan
    )?;
    drop(permit);
    let request = first.request.clone();
    let transit_skipped: Vec<(String, transit_planning::SkipReason)> = first
        .transit_skipped
        .iter()
        .chain(second.iter().flat_map(|second| &second.transit_skipped))
        .cloned()
        .collect();
    let (mut segments, live_outcome, service_dates) = match (second, next_date) {
        (Some(second), Some(next)) => (
            crate::data::trip_plan_overnight::merge_across_midnight(
                first.segments,
                second.segments,
                time,
                &params.results,
            ),
            combine_live(first.live, second.live),
            vec![date, next],
        ),
        _ => (first.segments, first.live, vec![date]),
    };
    if params.results == "options" {
        trip_planning_itinerary::label_options(&mut segments, request.interchange(&graph), time);
    }
    let live_summary = live_outcome.map(|outcome| {
        let label = match &outcome {
            Ok(_) => "applied",
            Err("disabled") => "disabled",
            Err("outsideLiveWindow") => "outside_window",
            Err(_) => "unavailable",
        };
        metrics::counter!(
            common::metrics::metric_name("api_trip_plan_live_requests_total"),
            "outcome" => label
        )
        .increment(1);
        match outcome {
            Ok(summary) => serde_json::json!({
                "applied": true,
                "reason": null,
                "replans": summary.replans,
                "trainsRead": summary.trains_read,
                "adjustedTrains": summary.adjusted_trains,
                "inSearch": summary.in_search,
                "snapshotTrains": summary.snapshot_trains,
            }),
            Err(reason) => serde_json::json!({ "applied": false, "reason": reason }),
        }
    });
    if live_summary.is_none() {
        metrics::counter!(
            common::metrics::metric_name("api_trip_plan_live_requests_total"),
            "outcome" => "not_requested"
        )
        .increment(1);
    }
    let waypoint_stops = if request.waypoint_codes.is_some() {
        trip_planning_itinerary::waypoint_satisfied_by(
            &segments,
            request.interchange(&graph),
            &request.origin,
            &request.waypoints,
            request.waypoint_codes.as_deref(),
            &request.destination,
        )
    } else {
        Vec::new()
    };
    let mut body = serde_json::json!({
        "results": params.results,
        "maxChanges": max_changes,
        // Additive (2026-09-29): the arrive-by deadline (`null` for a
        // depart-after request) and the avoid lists as applied.
        "arriveBy": match request.time {
            trip_planning_itinerary::TimeBound::ArriveBy(minutes) => Some(segment_clock(minutes)),
            trip_planning_itinerary::TimeBound::DepartAfter(_) => None,
        },
        // Additive (2026-10-19): the effective depart-after start (00:00
        // when none was given), `null` for an arrive-by request. Both
        // echoes carry `dayOffset` (see `?dayOffset=`).
        "departAfter": match request.time {
            trip_planning_itinerary::TimeBound::DepartAfter(minutes) => Some(segment_clock(minutes)),
            trip_planning_itinerary::TimeBound::ArriveBy(_) => None,
        },
        // Additive (2026-10-20): the service dates whose trains were
        // searched -- `[date]`, or `[date, date + 1]` with
        // `acrossMidnight`. Every time in this body is on `date`'s clock;
        // each train leg also names its own `serviceDate`.
        "serviceDates": service_dates,
        "avoid": request.avoid.avoid,
        "avoidStop": request.avoid.avoid_stop,
        "avoidChange": request.avoid.avoid_change,
        // Additive (2026-10-06): the pass-through vias as applied.
        "via": request.vias,
        // Additive (2026-10-07): each station group the request named
        // (`group:NAME` in `via`, `waypoints` or an avoid list) -> its
        // member CRS codes,
        // as expanded. `{}` when none.
        "stationGroups": station_groups,
        // Additive (2026-10-31): the operator filter as applied, each list
        // expanded to its operators (`[]` when unused), the groups it named
        // (`{}` when none), and the chosen operators that run nothing on
        // `date`.
        "operator": operator_filter.as_ref().filter(|f| f.only).map(|f| f.list.codes()).unwrap_or_default(),
        "avoidOperator": operator_filter.as_ref().filter(|f| !f.only).map(|f| f.list.codes()).unwrap_or_default(),
        "operatorGroups": operator_filter.as_ref().map(|f| f.list.groups.clone()).unwrap_or_default(),
        "operatorsNotRunning": operator_filter
            .as_ref()
            .map(|f| f.not_running(&[&graph]))
            .unwrap_or_default(),
        "segments": segments.iter().map(|segment| serde_json::json!({
            "originCrs": segment.origin_crs,
            "destinationCrs": segment.destination_crs,
            "itineraries": segment.itineraries,
            "cappedByMaxChanges": segment.capped_by_max_changes,
            // Additive (2026-09-28): when this segment was searched from --
            // `departAfter` for the first, the previous segment's arrival
            // plus the waypoint's change time for later ones; `null` when
            // the previous segment found nothing to chain from, and for an
            // arrive-by request.
            "departAfter": segment.depart_after_min.map(segment_clock),
            // Additive (2026-09-29), arrive-by only: the latest arrival this
            // segment was searched for; `null` otherwise.
            "arriveBy": segment.arrive_by_min.map(segment_clock),
            // Additive (2026-09-29): why `itineraries` is empty; `null` when
            // it is not.
            "noResultReason": segment.no_result_reason,
        })).collect::<Vec<_>>(),
        // Additive (2026-09-29): the whole journeys, aligned with every
        // segment's `itineraries` -- journey `j` is `itineraries[j]` of each
        // segment -- with their end-to-end change count.
        "journeys": trip_planning_itinerary::journey_summaries(&segments, request.interchange(&graph))
            .iter()
            .enumerate()
            .map(|(j, journey)| {
                let mut value = serde_json::json!({
                    "changeCount": journey.change_count,
                    // Additive (2026-10-30): changes within `TfL` access
                    // legs, not in `changeCount`.
                    "accessChangeCount": journey.access_change_count,
                    "departure": segment_clock(journey.departure_min),
                    "arrival": segment_clock(journey.arrival_min),
                    "totalDurationMinutes": journey.arrival_min - journey.departure_min,
                });
                if let Some(exceeds) = journey.exceeds_recommended_changes {
                    value["exceedsRecommendedChanges"] = exceeds.into();
                }
                if let Some(feasible) = journey.live_feasible {
                    value["liveFeasible"] = feasible.into();
                }
                // Additive (2026-10-30, DS-196), `results=options` only:
                // what this option is (`fastest` or `leavesLatest`, `fewestChanges`,
                // `alternative` with its `viaCrs`).
                if params.results == "options" {
                    value["labels"] = serde_json::json!(journey.labels);
                }
                // Additive (2026-10-06), with `via` only: for each via in
                // order, the leg that passed it.
                if !request.vias.is_empty() {
                    value["viaSatisfiedBy"] = serde_json::json!(journey.via_satisfied_by);
                }
                // Additive (2026-10-07), only when a waypoint is an OR
                // choice: for each waypoint in order, where the journey
                // stopped for it.
                if let Some(stops) = waypoint_stops.get(j) {
                    value["waypointSatisfiedBy"] = serde_json::json!(stops);
                }
                value
            })
            .collect::<Vec<_>>(),
    });
    if let Some(summary) = live_summary {
        body["live"] = summary;
    }
    // Additive (2026-10-23, DS-135), only when an end is a `TfL` stop: each
    // stop end with its access candidates (`null` for a station end).
    if let Some(access) = &access {
        body["access"] = access.to_json();
    }
    // Additive (2026-10-24, DS-164), only when planned with transit
    // networks: each network whose timetable does not cover the date
    // (`noTimetable`), so the plan was made without it. Never "no trams".
    if transit {
        body["transitNotices"] = transit_planning::notices_json(&transit_skipped);
    }
    // Additive (2026-10-08): whether `date`'s timetable is still
    // provisional. Once per response, not per journey or leg. With
    // `acrossMidnight`, the later of the two dates: it is the one that can
    // be provisional when the earlier is not.
    if let Some(object) = body.as_object_mut() {
        crate::routes::provisional::TimetableCertainty::for_date(
            service_dates.last().copied().unwrap_or(date),
            crate::routes::london_today(),
        )
        .insert_into(object);
    }
    Ok(Json(body))
}

/// The access candidates of a `TfL` stop origin or destination (DS-135),
/// `None` when both ends are stations. An unknown stop, a stop with no
/// National Rail station near it, and the same stop at both ends are 400s.
async fn load_access_ends(
    app: &App,
    origin: &str,
    destination: &str,
) -> Result<Option<tfl_access::AccessEnds>, (StatusCode, String)> {
    if !tfl_access::is_tfl_stop_id(origin) && !tfl_access::is_tfl_stop_id(destination) {
        return Ok(None);
    }
    if origin == destination {
        return Err((
            StatusCode::BAD_REQUEST,
            "origin and destination must differ".to_string(),
        ));
    }
    tfl_access::load_access(&app.database, origin, destination)
        .await
        .map_err(internal_error("read the TfL stops"))?
        .map(Some)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}

/// One request's planning inputs, shared with every blocking search.
/// Everything about a request that doesn't depend on the service date
/// planned (the graph-dependent search state is built per date).
struct DateInputs {
    origin: String,
    destination: String,
    waypoints: Vec<String>,
    waypoint_codes: Option<Vec<Vec<String>>>,
    avoid: trip_planning_itinerary::AvoidLists,
    vias: Vec<String>,
    via_codes: Vec<Vec<String>>,
    results: String,
    /// `options` only: add alternative routes (DS-196).
    alternatives: bool,
    max_changes: u32,
    live_requested: bool,
    /// Ride only what this allows (a journey alternative's "same
    /// operator" search, `plan_for_journey`); `None` for `/Trips/plan`.
    operator_rule: Option<trip_planner::OperatorRule>,
    /// The `TfL` stop ends and their access candidates (DS-135); `None`
    /// when both ends are stations.
    access: Option<Arc<tfl_access::AccessEnds>>,
    /// Plan with the transit networks (DS-164): `transit=true` or a tram
    /// stop end, with the switch on. Searches the graph's transit
    /// interchange data, and dresses the transit legs.
    transit: bool,
}

/// One service date's finished plan: searched, live-checked (when asked)
/// and with leg details attached.
struct DatePlan {
    request: Arc<PlanRequest>,
    segments: Vec<trip_planning_itinerary::SegmentResult>,
    /// `None` when live data was not asked for; `Err(reason)` when it was
    /// but did not apply.
    live: Option<Result<LiveSummary, &'static str>>,
    /// The transit networks left out of this date's graph (empty unless
    /// planned with transit).
    transit_skipped: Vec<(String, transit_planning::SkipReason)>,
}

/// TRIPS-1: `date`'s graph from the per-date cache, keyed on the latest
/// schedule publish; only a miss reads and builds it. `None` when nothing
/// is published for `date`.
async fn load_graph(
    app: &App,
    date: NaiveDate,
    marker: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<Option<Arc<trip_planning::PlanningGraph>>, (StatusCode, String)> {
    let pool = app.database.clone();
    // Read here, not in the build: the build may run on another task.
    let transit_networks = transit_planning::enabled_networks();
    let cached = GRAPH_CACHE
        .get_or_build(date, marker, move || async move {
            let Some(mut calling_points) =
                trip_planning::fetch_calling_points_for_date(&pool, date).await?
            else {
                return Ok(None);
            };
            let mut interchange = trip_planning::fetch_interchange_data(&pool).await?;
            // Who runs each link (operator filtering spec §5.1), before the
            // transit graph copies the rail interchange data.
            let unattributed_links = planner_operators::attribute_fixed_links(&mut interchange);
            // The estimated walks a filter that rules the Tube out uses
            // instead (spec §5.1); no other request ever sees them.
            let positions =
                planner_operators::fetch_tube_station_positions(&pool, &interchange).await?;
            planner_operators::add_tube_stand_in_walks(&mut interchange, &positions);
            let operators = planner_operators::OperatorTable::fetch(&pool, date).await?;
            let road_or_water = trip_planning::fetch_road_or_water_tiplocs(&pool).await?;
            // Per-UID published modes; an error degrades to the bus-stop
            // heuristic for every UID rather than failing the plan.
            let modes = crate::data::schedule_services::all_modes_for_date(&pool, date)
                .await
                .unwrap_or_else(|err| {
                    tracing::warn!(error = ?err, %date, "could not read service modes");
                    std::collections::HashMap::new()
                });
            interchange.modal_change = trip_planning::modal_change_buffer(
                &calling_points,
                &modes,
                &road_or_water,
                trip_planning::road_water_change_minutes(),
            );
            // The transit networks' trips join the trains before the one
            // sort (DS-164); with the switch empty, nothing is read.
            let transit_days = if transit_networks.is_empty() {
                None
            } else {
                let (days, skipped) =
                    transit_planning::read_days(&pool, &transit_networks, date).await;
                calling_points.extend(transit_planning::calling_points(&days));
                Some((days, skipped))
            };
            // The sort over every calling point of the day, on the blocking
            // pool (bound 4 in `get_trip_plan`'s doc comment), and the
            // transit interchange data (a copy of the rail data).
            let (connections, passes, interchange, transit, operators, unknown_operators, running) =
                tokio::task::spawn_blocking(move || {
                    let (mut connections, passes) =
                        trip_planning::build_connections_with_passes(calling_points);
                    let unknown_operators = operators.attribute_connections(&mut connections);
                    let running = planner_operators::running_operators(&connections, &interchange);
                    let transit = transit_days.map(|(days, skipped)| {
                        transit_planning::build_transit_graph(
                            &interchange,
                            &days,
                            &transit_planning::count_connections(&connections),
                            skipped,
                            transit_planning::change_minutes(),
                            trip_planning::road_water_change_minutes(),
                        )
                    });
                    (
                        connections,
                        passes,
                        interchange,
                        transit,
                        operators,
                        unknown_operators,
                        running,
                    )
                })
                .await?;
            if let Some(transit) = &transit {
                transit_planning::record_connections(transit);
            }
            planner_operators::record_attribution(date, unknown_operators, &unattributed_links);
            Ok(Some(trip_planning::PlanningGraph {
                connections,
                interchange,
                passes,
                transit,
                operators,
                running,
            }))
        })
        .await
        .map_err(internal_error("build the connections graph"))?;
    let Some((graph, outcome)) = cached else {
        return Ok(None);
    };
    metrics::counter!(
        common::metrics::metric_name("api_trip_plan_graph_cache_total"),
        "result" => match outcome {
            trip_planning::CacheOutcome::Hit => "hit",
            trip_planning::CacheOutcome::Built => "miss",
        }
    )
    .increment(1);
    Ok(Some(graph))
}

/// Plans `inputs` on one service date's `graph`, from `time` on that
/// date's clock: the searches, the live overlay and the leg details.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear: restrictions, the live snapshot, the plan, the overlay, leg details"
)]
async fn plan_on_date(
    app: &App,
    graph: &Arc<trip_planning::PlanningGraph>,
    inputs: &DateInputs,
    date: NaiveDate,
    time: trip_planning_itinerary::TimeBound,
    permit: &PlanPermit,
) -> Result<DatePlan, (StatusCode, String)> {
    // The avoid lists' search restrictions and the vias' search state, once
    // per date (each walks the day's connections for the trains that call
    // at or run through its stations).
    let (restrictions, via_search) = if inputs.avoid.is_empty() && inputs.vias.is_empty() {
        (None, None)
    } else {
        let graph = graph.clone();
        let lists = inputs.avoid.clone();
        let codes = inputs.via_codes.clone();
        let permit = permit.clone();
        let (restrictions, via_search) = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let restrictions = trip_planning_itinerary::build_restrictions(
                &graph.connections,
                &graph.interchange,
                Some(&graph.passes),
                &lists,
            )?;
            let via_search = trip_planning_itinerary::build_vias(
                &graph.connections,
                &graph.interchange,
                Some(&graph.passes),
                &codes,
            )?;
            Ok::<_, String>((restrictions, via_search))
        })
        .await
        .map_err(|join_err| {
            tracing::error!(error = ?join_err, "trip planning task panicked or was cancelled");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to plan trip".to_string(),
            )
        })?
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
        (restrictions.map(Arc::new), via_search.map(Arc::new))
    };
    let restrictions = match &inputs.operator_rule {
        None => restrictions,
        Some(rule) => Some(Arc::new(
            restrictions
                .map(|r| (*r).clone())
                .unwrap_or_default()
                .with_operator_rule(rule.clone()),
        )),
    };
    // A `TfL` stop end (DS-135): this request's own copy of the interchange
    // data with the stop's access links; the cached graph is not changed.
    // With transit (DS-164), the graph's transit interchange data, shared.
    let interchange = match (&inputs.access, graph.transit.as_ref()) {
        (Some(access), _) => Some(Arc::new(
            access.interchange(planning_interchange(graph, inputs.transit)),
        )),
        (None, Some(transit_graph)) if inputs.transit => Some(transit_graph.interchange.clone()),
        (None, _) => None,
    };
    let request = Arc::new(PlanRequest {
        interchange,
        date,
        origin: inputs.origin.clone(),
        destination: inputs.destination.clone(),
        waypoints: inputs.waypoints.clone(),
        waypoint_codes: inputs.waypoint_codes.clone(),
        time,
        avoid: inputs.avoid.clone(),
        restrictions,
        vias: inputs.vias.clone(),
        via_search,
        results: inputs.results.clone(),
        alternatives: inputs.alternatives,
        max_changes: inputs.max_changes,
    });
    // The live overlay: requested, switched on, and a service date with
    // trains running now. `Ok(now)` when it applies.
    let config = live_config();
    let live_wanted: Option<Result<chrono::DateTime<chrono::Utc>, &'static str>> =
        inputs.live_requested.then(|| {
            let today = crate::routes::london_today();
            if !config.enabled {
                Err("disabled")
            } else if date != today && Some(date) != today.pred_opt() {
                Err("outsideLiveWindow")
            } else {
                Ok(crate::routes::london_now().with_timezone(&chrono::Utc))
            }
        });
    // In-search (2026-10-22): the first search already runs on every
    // delay and cancellation TRUST knows of today. A failed read only
    // loses that: the re-plan loop below still runs.
    let snapshot = match live_wanted {
        Some(Ok(now)) if config.in_search => {
            match live_snapshot(app, graph, date, &config, now).await {
                Ok(snapshot) => Some(snapshot),
                Err(err) => {
                    tracing::warn!(error = ?err, "trip plan live snapshot failed; planning on the timetable first");
                    None
                }
            }
        }
        _ => None,
    };
    let first_overlay = snapshot
        .as_ref()
        .map(|snapshot| snapshot.overlay.clone())
        .filter(|overlay| !overlay.is_empty());
    let mut segments = run_plan(
        graph.clone(),
        request.clone(),
        first_overlay.clone(),
        permit.clone(),
    )
    .await?
    .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let live = match live_wanted {
        None => None,
        Some(Err(reason)) => Some(Err(reason)),
        Some(Ok(now)) => Some(
            match plan_live(
                app,
                graph,
                &request,
                permit,
                &segments,
                snapshot.as_deref(),
                &config,
                now,
            )
            .await
            {
                Ok((live_segments, summary)) => {
                    segments = live_segments;
                    Ok(summary)
                }
                Err(err) => {
                    tracing::warn!(error = ?err, "trip plan live overlay failed; serving the timetable plan");
                    if first_overlay.is_some() {
                        // The first plan ran on live minutes and was never
                        // annotated: plan on the timetable instead.
                        segments = run_plan(graph.clone(), request.clone(), None, permit.clone())
                            .await?
                            .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;
                    }
                    Err("unavailable")
                }
            },
        ),
    };

    crate::data::trip_leg_details::attach_leg_details(&app.database, date, &mut segments)
        .await
        .map_err(internal_error("attach trip leg details"))?;
    crate::data::tfl_stops::attach_transfer_stops(&app.database, &mut segments).await;
    crate::data::tfl_stops::attach_transfer_lines(&mut segments);
    if let Some(access) = &inputs.access {
        access.finish_legs(
            &mut segments,
            request.interchange(graph),
            matches!(time, trip_planning_itinerary::TimeBound::DepartAfter(_)),
        );
    }
    crate::data::tfl_stops::merge_transfer_rides(&app.database, &mut segments).await;
    // Transit legs (DS-164): `kind: "transit"`, the rail-link walks'
    // `transitWalk`, `scheduledOnly` and the lines' disruption notices.
    let transit_skipped = match graph.transit.as_ref().filter(|_| inputs.transit) {
        Some(transit_graph) => {
            transit_planning::dress_segments(
                transit_graph,
                &graph.connections,
                date,
                &mut segments,
            );
            transit_planning::attach_line_notices(&app.database, &mut segments).await;
            transit_graph.skipped.clone()
        }
        None => Vec::new(),
    };
    trip_planning_itinerary::count_access_changes(&mut segments);
    Ok(DatePlan {
        request,
        segments,
        live,
        transit_skipped,
    })
}

/// The interchange data a request plans on before any `TfL` access links:
/// the graph's transit copy when it plans with transit, else its own.
fn planning_interchange(
    graph: &trip_planning::PlanningGraph,
    transit: bool,
) -> &schedule_query::InterchangeData {
    match graph.transit.as_ref().filter(|_| transit) {
        Some(transit_graph) => &transit_graph.interchange,
        None => &graph.interchange,
    }
}

/// The graph cache key (TRIPS-1): the latest schedule publish, or (DS-164)
/// the latest activation of a switched-on transit network's feed when that
/// is later. With the switch empty, the publish marker alone, as before.
async fn graph_marker(
    app: &App,
    transit_networks: &[String],
) -> Result<Option<chrono::DateTime<chrono::Utc>>, (StatusCode, String)> {
    let schedule = trip_planning::latest_schedule_publish(&app.database)
        .await
        .map_err(internal_error("read the latest schedule publish"))?;
    // A failed read only loses the early invalidation (the cache's
    // max-age still applies): a rail plan never fails because of trams.
    let activation =
        match transit_planning::latest_activation(&app.database, transit_networks).await {
            Ok(activation) => activation,
            Err(err) => {
                tracing::warn!(error = ?err, "could not read the transit activation marker");
                None
            }
        };
    Ok(transit_planning::cache_marker(schedule, activation))
}

/// `?transit=`: `true` or `false` (default). Read only while the switch
/// lists a network; with it empty the parameter is ignored, so every
/// response stays as before.
fn parse_transit(raw: Option<&str>, switch_on: bool) -> Result<bool, (StatusCode, String)> {
    if !switch_on {
        return Ok(false);
    }
    match raw.map(str::trim) {
        None | Some("" | "false") => Ok(false),
        Some("true") => Ok(true),
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            "transit must be 'true' or 'false'".to_string(),
        )),
    }
}

/// Resolves a tram stop `origin` or `destination` (spec §5.2) to its
/// station node, in place, and returns each end's network (`None` for any
/// other end). Only with the switch on and a `NaPTAN`-shaped code: either
/// spelling (`9400ZZMAPIC`, `940GZZMAPIC`) or a platform, any case. A
/// `9400` code no switched-on network knows is a 400 when the request
/// opted in (otherwise it is left to the station lookup, as before); a
/// `940G` code no network knows stays a `TfL` stop. The same station at
/// both ends is a 400.
async fn resolve_transit_ends(
    app: &App,
    networks: &[String],
    transit_requested: bool,
    origin: &mut String,
    destination: &mut String,
) -> Result<[Option<String>; 2], (StatusCode, String)> {
    let mut ends = [None, None];
    if networks.is_empty() {
        return Ok(ends);
    }
    for (slot, end) in ends.iter_mut().zip([&mut *origin, &mut *destination]) {
        let code = end.trim().to_ascii_uppercase();
        if !transit_planning::looks_like_naptan(&code) {
            continue;
        }
        match transit_planning::find_station(&app.database, networks, &code)
            .await
            .map_err(internal_error("read the transit stops"))?
        {
            Some((network, station)) => {
                *end = station;
                *slot = Some(network);
            }
            None if transit_requested && code.starts_with("9400") => {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("'{code}' is not a stop the planner knows"),
                ));
            }
            None => {}
        }
    }
    if ends.iter().any(Option::is_some) && origin == destination {
        return Err((
            StatusCode::BAD_REQUEST,
            "origin and destination must differ".to_string(),
        ));
    }
    Ok(ends)
}

/// A tram stop end whose network has no timetable for the date (spec §7.3)
/// cannot be planned from: a 400 saying so, rather than "unknown station".
fn check_transit_ends_planned(
    graph: &trip_planning::PlanningGraph,
    ends: &[Option<String>; 2],
) -> Result<(), (StatusCode, String)> {
    let planned = |network: &str| {
        graph.transit.as_ref().is_some_and(|transit| {
            transit
                .networks
                .iter()
                .any(|known| known.network == network)
        })
    };
    if ends.iter().flatten().all(|network| planned(network)) {
        Ok(())
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            "Tram times aren't available for this date, so the plan can't start or end at a tram stop."
                .to_string(),
        ))
    }
}

/// Tram stops are not waypoints, vias or avoided stations yet (Q9): a 400
/// for a `NaPTAN`-shaped entry, when planning with transit.
fn check_no_transit_stops<'c>(
    choices: impl Iterator<Item = &'c StationChoice>,
) -> Result<(), (StatusCode, String)> {
    for choice in choices {
        if let Some(code) = choice
            .codes
            .iter()
            .find(|code| transit_planning::looks_like_naptan(code))
        {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "'{code}' is a tram stop: waypoints, vias and avoided stations must be stations"
                ),
            ));
        }
    }
    Ok(())
}

/// Both dates' live outcomes as one: applied when either applied (its
/// counts summed), else the request date's reason.
fn combine_live(
    first: Option<Result<LiveSummary, &'static str>>,
    second: Option<Result<LiveSummary, &'static str>>,
) -> Option<Result<LiveSummary, &'static str>> {
    match (first, second) {
        (Some(Ok(a)), Some(Ok(b))) => Some(Ok(LiveSummary {
            replans: a.replans + b.replans,
            trains_read: a.trains_read + b.trains_read,
            adjusted_trains: a.adjusted_trains + b.adjusted_trains,
            in_search: a.in_search || b.in_search,
            snapshot_trains: a.snapshot_trains + b.snapshot_trains,
        })),
        (Some(Err(_)), Some(Ok(b))) => Some(Ok(b)),
        (first, _) => first,
    }
}

struct PlanRequest {
    /// The interchange data with a `TfL` stop end's access links
    /// (DS-135); `None` to plan on the graph's own.
    interchange: Option<Arc<schedule_query::InterchangeData>>,
    date: NaiveDate,
    origin: String,
    destination: String,
    /// Each waypoint's label (its CRS, or an OR choice's: `KGX|EUS`,
    /// `group:LON`).
    waypoints: Vec<String>,
    /// Each waypoint's codes as applied (avoided members dropped), when
    /// any waypoint is an OR choice; `None` otherwise.
    waypoint_codes: Option<Vec<Vec<String>>>,
    time: trip_planning_itinerary::TimeBound,
    avoid: trip_planning_itinerary::AvoidLists,
    /// `build_restrictions(avoid)`, `None` when no list is given.
    restrictions: Option<Arc<trip_planner::Restrictions>>,
    /// `?via=` (each via's label: `KGX`, `KGX|EUS`, `group:LON`), and
    /// `build_vias` of its codes (`None` when empty).
    vias: Vec<String>,
    via_search: Option<Arc<trip_planner::Vias>>,
    results: String,
    alternatives: bool,
    max_changes: u32,
}

impl PlanRequest {
    /// The interchange data this request plans on: `graph`'s own, or the
    /// copy with its access links.
    fn interchange<'a>(
        &'a self,
        graph: &'a trip_planning::PlanningGraph,
    ) -> &'a schedule_query::InterchangeData {
        self.interchange.as_deref().unwrap_or(&graph.interchange)
    }
}

type PlanPermit = Arc<tokio::sync::SemaphorePermit<'static>>;

/// One planning pass on the blocking pool, holding a clone of the permit.
/// The outer `Err` is a 500 (the task panicked); the inner one a planning
/// validation message.
async fn run_plan(
    graph: Arc<trip_planning::PlanningGraph>,
    request: Arc<PlanRequest>,
    overlay: Option<Arc<trip_planner::ConnectionOverlay>>,
    permit: PlanPermit,
) -> Result<Result<Vec<trip_planning_itinerary::SegmentResult>, String>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        // Held until the search itself finishes, not until this handler's
        // future does -- see bound 3 in `get_trip_plan`'s doc comment.
        let _permit = permit;
        trip_planning_itinerary::plan_trip(&trip_planning_itinerary::TripPlanInput {
            search: trip_planning_itinerary::SegmentSearch {
                connections: &graph.connections,
                interchange: request.interchange(&graph),
                date: request.date,
                results: &request.results,
                max_changes: request.max_changes,
                overlay: overlay.as_deref(),
                restrictions: request.restrictions.as_deref(),
                alternatives: request.alternatives,
            },
            passes: Some(&graph.passes),
            avoid: &request.avoid,
            origin_crs: &request.origin,
            waypoints: &request.waypoints,
            waypoint_codes: request.waypoint_codes.as_deref(),
            destination_crs: &request.destination,
            vias: &request.vias,
            via_search: request.via_search.as_deref(),
            time: request.time,
        })
    })
    .await
    .map_err(|join_err| {
        tracing::error!(error = ?join_err, "trip planning task panicked or was cancelled");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to plan trip".to_string(),
        )
    })
}

/// What the live overlay did, for the response's top-level `live`.
#[derive(Debug, Default)]
struct LiveSummary {
    replans: u32,
    trains_read: usize,
    adjusted_trains: usize,
    /// Whether the searches ran on the in-search snapshot.
    in_search: bool,
    /// How many trains the snapshot changed (0 without one).
    snapshot_trains: usize,
}

/// The live overlay's bounded re-planning loop (design doc §4.4): read the
/// live profile of every train in the current plan (inside the live window)
/// not yet read; if the plan no longer works on live times (a cancelled leg,
/// an impossible change, a broken waypoint chain) or a late train from the
/// origin has become catchable, plan again with every known change applied
/// -- at most `config.max_replans_for(results)` times (3 for fastest, 1 for
/// options by default). A plan that is merely late is
/// annotated, not re-planned. Trains read after the last allowed re-plan
/// are still annotated. Any read error aborts the overlay; the caller then
/// serves the timetable plan.
///
/// `first` is the plan already made: on the timetable, or, with an
/// in-search `snapshot` (2026-10-22), on the snapshot's overlay. Each later
/// overlay is the snapshot with every train read one by one replacing the
/// snapshot's view of it (`trip_plan_live::combined_overlay`), so the
/// re-plans now only correct what the per-train reads (boards, actual
/// times, skips) know better than TRUST's running state.
#[expect(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "long but linear; splitting it would scatter its shared state across helpers; each argument is an independent input"
)]
async fn plan_live(
    app: &App,
    graph: &Arc<trip_planning::PlanningGraph>,
    request: &Arc<PlanRequest>,
    permit: &PlanPermit,
    first: &[trip_planning_itinerary::SegmentResult],
    snapshot: Option<&trip_plan_live::LiveSnapshot>,
    config: &trip_plan_live::LiveConfig,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<(Vec<trip_planning_itinerary::SegmentResult>, LiveSummary)> {
    use std::collections::{HashMap, HashSet};

    let date = request.date;
    let mut segments = first.to_vec();
    let mut lives: HashMap<String, trip_plan_live::TrainLive> = HashMap::new();
    let mut read: HashSet<String> = HashSet::new();
    let mut chains: HashMap<String, Vec<schedule_query::Connection>> = HashMap::new();
    // The overlay `segments` was planned on, and which of its trains it
    // took from the per-train reads (none yet: the rest are the snapshot's).
    let mut overlay = snapshot.map_or_else(
        || Arc::new(trip_planner::ConnectionOverlay::default()),
        |snapshot| snapshot.overlay.clone(),
    );
    let mut overlay_read: HashSet<String> = HashSet::new();
    let mut withdrawn = snapshot.map_or(0, |snapshot| {
        snapshot.chains.values().map(Vec::len).sum::<usize>()
    });
    let mut replans = 0u32;

    // Trains booked to leave the origin in the hour before `departAfter`:
    // one running late may now be catchable. Not for arrive-by: a late train
    // only arrives later, and the backward search already takes the latest
    // departure that works.
    let depart_min = match request.time {
        trip_planning_itinerary::TimeBound::DepartAfter(minutes) => Some(minutes),
        trip_planning_itinerary::TimeBound::ArriveBy(_) => None,
    };
    let origin_tiplocs = graph
        .interchange
        .crs_to_tiplocs
        .get(&request.origin)
        .cloned()
        .unwrap_or_default();
    let seeds = match depart_min {
        Some(depart_min) if trip_plan_live::within_horizon(date, depart_min, now, config) => {
            trip_plan_live::origin_lookback_uids(&graph.connections, &origin_tiplocs, depart_min)
        }
        _ => Vec::new(),
    };
    let mut pending_seeds = seeds.clone();

    loop {
        let mut wanted = trip_plan_live::uids_in_window(&segments, date, now, config);
        wanted.append(&mut pending_seeds);
        wanted.sort();
        wanted.dedup();
        wanted.retain(|uid| !read.contains(uid));
        wanted.truncate(config.max_trains.saturating_sub(read.len()));
        if wanted.is_empty() {
            break;
        }
        let fetched =
            trip_plan_live::fetch_train_lives(&app.database, date, &wanted, config, now).await?;
        read.extend(wanted);
        let new_uids: HashSet<String> = fetched.keys().cloned().collect();
        let graph_for_chains = graph.clone();
        let new_chains = tokio::task::spawn_blocking(move || {
            trip_plan_live::chains_for(&graph_for_chains.connections, &new_uids)
        })
        .await?;
        chains.extend(new_chains);
        lives.extend(fetched);
        if replans >= config.max_replans_for(&request.results) {
            break;
        }
        let (next, next_withdrawn) = trip_plan_live::combined_overlay(snapshot, &chains, &lives);
        if next.replaced_uids() == overlay.replaced_uids()
            && next.replacements() == overlay.replacements()
        {
            break;
        }
        // Re-plan only when the current plan no longer works on what is now
        // known (or a late origin train became catchable).
        let mut probe = segments.clone();
        trip_plan_live::annotate(
            &mut probe,
            &trip_plan_live::LiveContext {
                date,
                now,
                config,
                interchange: request.interchange(graph),
                chains: &chains,
                lives: &lives,
                snapshot,
                overlaid: overlay.replaced_uids(),
                overlaid_read: &overlay_read,
            },
        );
        let invalidated = trip_plan_live::plan_invalidated(&probe, request.interchange(graph))
            || depart_min.is_some_and(|depart_min| {
                trip_plan_live::seed_became_catchable(
                    &seeds,
                    &chains,
                    &lives,
                    &origin_tiplocs,
                    depart_min,
                )
            });
        if !invalidated {
            break;
        }
        // Thousands of trains with a snapshot: indexed once, so the searches
        // test a bit per connection instead of hashing its UID.
        overlay = if snapshot.is_some() {
            let graph = graph.clone();
            Arc::new(tokio::task::spawn_blocking(move || next.indexed(&graph.connections)).await?)
        } else {
            Arc::new(next)
        };
        overlay_read = lives.keys().cloned().collect();
        withdrawn = next_withdrawn;
        segments = run_plan(
            graph.clone(),
            request.clone(),
            Some(overlay.clone()),
            permit.clone(),
        )
        .await
        .map_err(|(_, msg)| anyhow::anyhow!(msg))?
        .map_err(|msg| anyhow::anyhow!(msg))?;
        replans += 1;
    }

    trip_plan_live::annotate(
        &mut segments,
        &trip_plan_live::LiveContext {
            date,
            now,
            config,
            interchange: request.interchange(graph),
            chains: &chains,
            lives: &lives,
            snapshot,
            overlaid: overlay.replaced_uids(),
            overlaid_read: &overlay_read,
        },
    );
    // A later waypoint segment can be left from the live arrival plus the
    // change time, which is what its `departAfter` now says.
    if depart_min.is_some()
        && segments
            .windows(2)
            .all(|pair| pair[0].itineraries.len() == pair[1].itineraries.len())
    {
        trip_planning_itinerary::set_waypoint_ready_mins(&mut segments, request.interchange(graph));
    }
    metrics::counter!(common::metrics::metric_name(
        "api_trip_plan_live_replans_total"
    ))
    .increment(u64::from(replans));
    metrics::counter!(common::metrics::metric_name(
        "api_trip_plan_live_trains_read_total"
    ))
    .increment(read.len() as u64);
    metrics::counter!(common::metrics::metric_name(
        "api_trip_plan_live_adjusted_connections_total"
    ))
    .increment(withdrawn as u64);
    Ok((
        segments,
        LiveSummary {
            replans,
            trains_read: read.len(),
            adjusted_trains: overlay.replaced_uids().len(),
            in_search: snapshot.is_some(),
            snapshot_trains: snapshot.map_or(0, trip_plan_live::LiveSnapshot::trains),
        },
    ))
}

/// The break sweep's own planning slot (mid-journey changes spec
/// 2026-10-22 §5.3): one search at a time, waited for rather than shed, so
/// the sweep never takes a [`PLAN_SLOTS`] permit from a user's request.
static SWEEP_PLAN_SLOTS: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(1));

/// Whose slot a journey re-plan holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplanSlot {
    /// A user's request: one of [`PLAN_SLOTS`], a 503 when none is free.
    Request,
    /// The break sweep: [`SWEEP_PLAN_SLOTS`], waited for.
    Sweep,
}

/// One search for a journey's alternatives (`plan_for_journey`).
#[derive(Debug, Clone)]
pub(crate) struct ReplanQuery {
    pub origin: String,
    pub destination: String,
    pub date: NaiveDate,
    /// Minutes from `date`'s midnight (may exceed 1440).
    pub depart_after_min: u32,
    /// `fastest` (CSA) or `options` (RAPTOR's Pareto set).
    pub results: &'static str,
    pub max_changes: u32,
    /// Ride only what this allows, when set.
    pub operator_rule: Option<trip_planner::OperatorRule>,
}

/// Plans one of a journey's alternative searches exactly as
/// `GET /Trips/plan` would (`live=true`, same graph cache, overlay and leg
/// details), holding `slot`. `Ok(None)` when no timetable is published for
/// the date. The itineraries carry their live times (`departure_min`,
/// `arrival_min`) and each train leg its `live`.
pub(crate) async fn plan_for_journey(
    app: &App,
    query: &ReplanQuery,
    slot: ReplanSlot,
) -> Result<Option<Vec<trip_planning_itinerary::PlannedItinerary>>, (StatusCode, String)> {
    let permit = match slot {
        ReplanSlot::Request => PLAN_SLOTS.try_acquire().map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "too many trip plans are being computed right now; please retry in a moment"
                    .to_string(),
            )
        })?,
        ReplanSlot::Sweep => SWEEP_PLAN_SLOTS.acquire().await.map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "the sweep's planning slot is closed".to_string(),
            )
        })?,
    };
    let permit = Arc::new(permit);
    let marker = graph_marker(app, &transit_planning::enabled_networks()).await?;
    let Some(graph) = load_graph(app, query.date, marker).await? else {
        return Ok(None);
    };
    let inputs = DateInputs {
        origin: query.origin.clone(),
        destination: query.destination.clone(),
        waypoints: Vec::new(),
        waypoint_codes: None,
        avoid: trip_planning_itinerary::AvoidLists::default(),
        vias: Vec::new(),
        via_codes: Vec::new(),
        results: query.results.to_string(),
        // A journey's alternatives keep the bare Pareto set (DS-196).
        alternatives: false,
        max_changes: query
            .max_changes
            .min(trip_planning_itinerary::MAX_CHANGES_LIMIT),
        live_requested: true,
        operator_rule: query.operator_rule.clone(),
        access: None,
        // Journey alternatives and re-plans ride trains only (spec §9).
        transit: false,
    };
    let plan = plan_on_date(
        app,
        &graph,
        &inputs,
        query.date,
        trip_planning_itinerary::TimeBound::DepartAfter(query.depart_after_min),
        &permit,
    )
    .await?;
    Ok(Some(
        plan.segments
            .into_iter()
            .next()
            .map(|segment| segment.itineraries)
            .unwrap_or_default(),
    ))
}

/// Splits, trims, uppercases and CAPS the `?waypoints=` list. Factored out of
/// the handler purely so the cap is testable without a database or an HTTP
/// stack -- the parsing itself is unchanged.
///
/// The cap is a 400 with a message naming the limit, not a silent truncation:
/// silently dropping waypoints would return an itinerary that answers a
/// different question than the one asked, which for a journey planner is
/// worse than an error.
fn parse_waypoints(raw: Option<&str>) -> Result<Vec<StationChoice>, (StatusCode, String)> {
    let bad = |message: String| (StatusCode::BAD_REQUEST, message);
    let entries: Vec<&str> = raw
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    if entries.len() > *WAYPOINT_LIMIT {
        return Err(bad(format!(
            "too many waypoints: {} given, at most {} allowed",
            entries.len(),
            *WAYPOINT_LIMIT
        )));
    }
    // OR choices (2026-10-07): `KGX|EUS`, `group:LON`, as for `via`. A
    // choice is ONE waypoint, of at most MAX_VIA_ALTERNATIVES stations, all
    // the waypoints together at most MAX_VIA_STATIONS.
    let waypoints = entries
        .into_iter()
        .map(|entry| station_groups::parse_choice("waypoints", entry, true).map_err(bad))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(waypoint) = waypoints
        .iter()
        .find(|waypoint| waypoint.codes.len() > MAX_VIA_ALTERNATIVES)
    {
        return Err(bad(format!(
            "waypoints: '{}' stands for {} stations, at most {MAX_VIA_ALTERNATIVES} allowed in one \
             waypoint",
            waypoint.label,
            waypoint.codes.len()
        )));
    }
    let total: usize = waypoints.iter().map(|waypoint| waypoint.codes.len()).sum();
    if total > MAX_VIA_STATIONS {
        return Err(bad(format!(
            "waypoints: the waypoints stand for {total} stations together, at most \
             {MAX_VIA_STATIONS} allowed"
        )));
    }
    Ok(waypoints)
}

/// The OR-choice waypoints (2026-10-07) against the avoid lists: a member
/// in any avoid list is dropped (a stop there is not allowed, and an
/// avoided member is just never used, as for a via); a choice left with
/// none is a 400. Then two adjacent waypoints, one a choice, that share a
/// station are a 400 ([`trip_planning_itinerary::check_adjacent_waypoints`]).
/// Returns each waypoint's codes as applied. Single-station waypoints are
/// untouched: [`check_avoid_conflicts`] and `plan_trip` keep their 400s.
fn apply_waypoint_choices(
    waypoints: &[StationChoice],
    avoid: &trip_planning_itinerary::AvoidLists,
) -> Result<Vec<Vec<String>>, (StatusCode, String)> {
    let avoided = |code: &String| avoid.lists().iter().any(|(_, list)| list.contains(code));
    let mut applied: Vec<Vec<String>> = Vec::with_capacity(waypoints.len());
    for waypoint in waypoints {
        if waypoint.is_single_station() {
            applied.push(waypoint.codes.clone());
            continue;
        }
        let codes: Vec<String> = waypoint
            .codes
            .iter()
            .filter(|code| !avoided(code))
            .cloned()
            .collect();
        if codes.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "waypoints: every station of '{}' is in an avoid list; a trip cannot stop at \
                     one and avoid them all",
                    waypoint.label
                ),
            ));
        }
        applied.push(codes);
    }
    let labels: Vec<String> = waypoints.iter().map(|w| w.label.clone()).collect();
    trip_planning_itinerary::check_adjacent_waypoints(&labels, &applied)
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    Ok(applied)
}

/// Splits, trims and normalises one avoid list (`name` is its wire name,
/// for the message), capped at [`MAX_AVOIDED`] entries -- a 400 before any
/// database read, like [`parse_waypoints`]. An entry is a station or a
/// named group (`group:LON`, 2026-10-07; see [`station_groups`]), which
/// counts as one entry: an avoided station adds no search state, and
/// `avoid`'s read of the trains running through it is linear in the day's
/// connections however many stations it names. `|` is a 400 (an avoid list
/// already means "none of these").
pub(crate) fn parse_station_list(
    name: &str,
    raw: Option<&str>,
) -> Result<Vec<StationChoice>, (StatusCode, String)> {
    let mut choices: Vec<StationChoice> = Vec::new();
    for entry in raw
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let choice = station_groups::parse_choice(name, entry, false)
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
        if !choices.iter().any(|known| known.label == choice.label) {
            choices.push(choice);
        }
    }
    if choices.len() > MAX_AVOIDED {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "too many {name} stations: {} given, at most {MAX_AVOIDED} allowed",
                choices.len()
            ),
        ));
    }
    Ok(choices)
}

/// Every station code `choices` stand for, in order, each once.
fn expand_choices(choices: &[StationChoice]) -> Vec<String> {
    let mut codes: Vec<String> = Vec::new();
    for code in choices.iter().flat_map(|choice| &choice.codes) {
        if !codes.contains(code) {
            codes.push(code.clone());
        }
    }
    codes
}

/// The three avoid lists as parsed, before their groups are expanded into
/// [`trip_planning_itinerary::AvoidLists`] (kept so a 400 can name the
/// group a conflicting station came from).
struct AvoidChoices {
    avoid: Vec<StationChoice>,
    avoid_stop: Vec<StationChoice>,
    avoid_change: Vec<StationChoice>,
}

impl AvoidChoices {
    /// Each list with its wire name.
    fn lists(&self) -> [(&'static str, &[StationChoice]); 3] {
        [
            ("avoid", &self.avoid),
            ("avoidStop", &self.avoid_stop),
            ("avoidChange", &self.avoid_change),
        ]
    }

    fn all(&self) -> impl Iterator<Item = &StationChoice> {
        self.avoid
            .iter()
            .chain(&self.avoid_stop)
            .chain(&self.avoid_change)
    }

    /// The lists as applied: every group expanded into its members.
    fn applied(&self) -> trip_planning_itinerary::AvoidLists {
        trip_planning_itinerary::AvoidLists {
            avoid: expand_choices(&self.avoid),
            avoid_stop: expand_choices(&self.avoid_stop),
            avoid_change: expand_choices(&self.avoid_change),
        }
    }
}

/// Every group `choices` name -> its members, for the response's
/// `stationGroups`.
fn used_station_groups<'c>(
    choices: impl Iterator<Item = &'c StationChoice>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    choices
        .flat_map(|choice| &choice.groups)
        .filter_map(|group| {
            station_groups::groups()
                .get(group)
                .map(|members| (group.clone(), members.clone()))
        })
        .collect()
}

/// An avoided station that is also the origin, the destination or a
/// waypoint makes the request contradictory: a 400 naming both (and the
/// group it came from, if any), before any database read.
fn check_avoid_conflicts(
    avoid: &AvoidChoices,
    origin: &str,
    waypoints: &[String],
    destination: &str,
) -> Result<(), (StatusCode, String)> {
    for (name, choices) in avoid.lists() {
        for choice in choices {
            for code in &choice.codes {
                let role = if code == origin {
                    "the origin"
                } else if code == destination {
                    "the destination"
                } else if waypoints.contains(code) {
                    "a waypoint"
                } else {
                    continue;
                };
                let message = if choice.is_single_station() {
                    format!("{name}: '{code}' is {role}; a trip cannot avoid it")
                } else {
                    format!(
                        "{name}: '{}' includes '{code}', which is {role}; a trip cannot avoid it",
                        choice.label
                    )
                };
                return Err((StatusCode::BAD_REQUEST, message));
            }
        }
    }
    Ok(())
}

/// Splits, trims and normalises `?via=` (a CRS upper-cased, a bus stop's or
/// ferry terminal's `tiploc:` code to `tiploc:UPPER`, as for the other
/// lists), keeping the order (a via may repeat, but not twice in a row),
/// capped at [`MAX_VIAS`] -- a 400 before any database read, like
/// [`parse_waypoints`].
///
/// Each via may be an OR choice (2026-10-07): alternatives separated by `|`
/// and named groups (`group:LON`), passed by ANY of its stations; see
/// [`station_groups`]. One choice counts as one via, and holds at most
/// [`MAX_VIA_ALTERNATIVES`] stations, all the vias together at most
/// [`MAX_VIA_STATIONS`].
fn parse_vias(raw: Option<&str>) -> Result<Vec<StationChoice>, (StatusCode, String)> {
    parse_ordered_choices("via", raw, MAX_VIAS)
}

/// [`parse_vias`] for any ordered list of OR choices named `name` on the
/// wire, capped at `max` entries: also the `/trains` boards' `via` and
/// `calls_at` (2026-10-30). Each entry holds at most
/// [`MAX_VIA_ALTERNATIVES`] stations, all of them together at most
/// [`MAX_VIA_STATIONS`], and no entry repeats the one before it.
pub(crate) fn parse_ordered_choices(
    name: &str,
    raw: Option<&str>,
    max: usize,
) -> Result<Vec<StationChoice>, (StatusCode, String)> {
    let bad = |message: String| (StatusCode::BAD_REQUEST, message);
    let entries: Vec<&str> = raw
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if entries.len() > max {
        return Err(bad(format!(
            "too many {name} stations: {} given, at most {max} allowed",
            entries.len()
        )));
    }
    let choices = entries
        .into_iter()
        .map(|entry| station_groups::parse_choice(name, entry, true).map_err(bad))
        .collect::<Result<Vec<_>, _>>()?;
    if let Some(choice) = choices
        .iter()
        .find(|choice| choice.codes.len() > MAX_VIA_ALTERNATIVES)
    {
        return Err(bad(format!(
            "{name}: '{}' stands for {} stations, at most {MAX_VIA_ALTERNATIVES} allowed in one {name}",
            choice.label,
            choice.codes.len()
        )));
    }
    let total: usize = choices.iter().map(|choice| choice.codes.len()).sum();
    if total > MAX_VIA_STATIONS {
        return Err(bad(format!(
            "{name}: the {} stand for {total} stations together, at most {MAX_VIA_STATIONS} allowed",
            if name == "via" { "vias" } else { "entries" }
        )));
    }
    // The same stations twice in a row, however written (`KGX|EUS,EUS|KGX`).
    let set = |choice: &StationChoice| -> std::collections::BTreeSet<String> {
        choice.codes.iter().cloned().collect()
    };
    let same = |a: &StationChoice, b: &StationChoice| set(a) == set(b);
    if let Some([repeated, _]) = choices.windows(2).find(|pair| same(&pair[0], &pair[1])) {
        return Err(bad(format!(
            "{name}: '{}' is given twice in a row; name it once",
            repeated.label
        )));
    }
    Ok(choices)
}

/// A via that is the origin or the destination says nothing (every journey
/// passes them), and a via that is also avoided (`avoid`: not even passing
/// through) cannot be satisfied: a 400 naming both, before any database
/// read. A via in `avoidStop` (pass through without calling) or
/// `avoidChange` is a meaningful combination and allowed.
///
/// An OR choice (2026-10-07) is a 400 only when EVERY station it names is
/// in `avoid`; a member that is the origin or the destination satisfies it
/// there (the journey passes it), and an avoided member is just never used.
fn check_via_conflicts(
    vias: &[StationChoice],
    avoid: &trip_planning_itinerary::AvoidLists,
    origin: &str,
    destination: &str,
) -> Result<(), (StatusCode, String)> {
    for via in vias {
        let label = &via.label;
        let message = if via.is_single_station() && label == origin {
            format!("via: '{label}' is the origin; every journey passes it already")
        } else if via.is_single_station() && label == destination {
            format!("via: '{label}' is the destination; every journey passes it already")
        } else if via.codes.iter().all(|code| avoid.avoid.contains(code)) {
            if via.is_single_station() {
                format!(
                    "via: '{label}' is also in avoid; a trip cannot both pass through and avoid it"
                )
            } else {
                format!(
                    "via: every station of '{label}' is also in avoid; a trip cannot both pass \
                     through one and avoid them all"
                )
            }
        } else {
            continue;
        };
        return Err((StatusCode::BAD_REQUEST, message));
    }
    Ok(())
}

/// [`OPTIONS_SEARCH_SIZE_LIMIT`] (`limit`) for `results=options`: a 400
/// naming the numbers and the ways out, before any database read.
fn check_options_search_size(
    results: &str,
    waypoints: usize,
    vias: usize,
    max_changes: u32,
    limit: usize,
) -> Result<(), (StatusCode, String)> {
    if results != "options" {
        return Ok(());
    }
    let rounds = trip_planning_itinerary::max_rounds(max_changes) as usize;
    let size = (waypoints + 1) * (2 * vias + 1) * rounds;
    if size <= limit {
        return Ok(());
    }
    Err((
        StatusCode::BAD_REQUEST,
        format!(
            "results=options with {waypoints} waypoints, {vias} vias and maxChanges={max_changes} \
             is too large a search ((waypoints + 1) * (2 * vias + 1) * (maxChanges + 2) = {size}, \
             at most {limit}); use fewer waypoints or vias, a lower \
             maxChanges, or results=fastest"
        ),
    ))
}

/// `?dayOffset=`: absent or empty is 0; otherwise `0` or `1`, and only
/// with `departAfter` or `arriveBy` (`has_time`) -- an offset for no time
/// would be silently meaningless.
fn parse_day_offset(raw: Option<&str>, has_time: bool) -> Result<u32, (StatusCode, String)> {
    let raw = raw.map(str::trim).unwrap_or_default();
    if raw.is_empty() {
        return Ok(0);
    }
    let offset = match raw {
        "0" => 0,
        "1" => 1,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("dayOffset must be 0 or 1, not '{raw}'"),
            ));
        }
    };
    if !has_time {
        return Err((
            StatusCode::BAD_REQUEST,
            "dayOffset needs departAfter or arriveBy".to_string(),
        ));
    }
    Ok(offset)
}

/// `?acrossMidnight=`: absent, empty or `false` is off; `true` needs
/// `dayOffset=1` (a time on the morning after `date`, the only place two
/// service dates' trains overlap). Anything else is a 400.
fn parse_across_midnight(raw: Option<&str>, day_offset: u32) -> Result<bool, (StatusCode, String)> {
    match raw
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "" | "false" => Ok(false),
        "true" if day_offset == 1 => Ok(true),
        "true" => Err((
            StatusCode::BAD_REQUEST,
            "acrossMidnight needs dayOffset=1 (a time on the morning after date)".to_string(),
        )),
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("acrossMidnight must be true or false, not '{other}'"),
        )),
    }
}

/// `?departAfter=` / `?arriveBy=`: at most one. Neither means departing
/// after 00:00, as before. `day_offset` (0 or 1, [`parse_day_offset`])
/// adds that many days: the engine's minutes run on past 1440 for a
/// service date's post-midnight calls.
fn parse_time_bound(
    depart_after: Option<NaiveTime>,
    arrive_by: Option<NaiveTime>,
    day_offset: u32,
) -> Result<trip_planning_itinerary::TimeBound, (StatusCode, String)> {
    use chrono::Timelike;
    let minutes = |time: NaiveTime| time.num_seconds_from_midnight() / 60 + day_offset * 1440;
    match (depart_after, arrive_by) {
        (Some(_), Some(_)) => Err((
            StatusCode::BAD_REQUEST,
            "departAfter and arriveBy are mutually exclusive; give at most one".to_string(),
        )),
        (_, Some(arrive_by)) => Ok(trip_planning_itinerary::TimeBound::ArriveBy(minutes(
            arrive_by,
        ))),
        (depart_after, None) => Ok(trip_planning_itinerary::TimeBound::DepartAfter(minutes(
            depart_after.unwrap_or(NaiveTime::MIN),
        ))),
    }
}

/// The cap for a request that left `?maxChanges=` out:
/// [`trip_planning_itinerary::DEFAULT_MAX_CHANGES`] (4), or, for an
/// `options` search too large for the guard at 4 (`limit`, see
/// [`check_options_search_size`]), the most that fits down to
/// [`LEGACY_DEFAULT_MAX_CHANGES`]. Every request that omitted `maxChanges`
/// and fitted before the default rose from 2 (2026-10-24) still fits, and
/// still gets at least what it got then; one too large even at 2 gets 2 and
/// its 400, as before. Echoed as the response's `maxChanges`.
fn default_max_changes(results: &str, waypoints: usize, vias: usize, limit: usize) -> u32 {
    (LEGACY_DEFAULT_MAX_CHANGES..=trip_planning_itinerary::DEFAULT_MAX_CHANGES)
        .rev()
        .find(|&max_changes| {
            check_options_search_size(results, waypoints, vias, max_changes, limit).is_ok()
        })
        .unwrap_or(LEGACY_DEFAULT_MAX_CHANGES)
}

/// The default `maxChanges` before 2026-10-24: the floor
/// [`default_max_changes`] steps down to.
const LEGACY_DEFAULT_MAX_CHANGES: u32 = 2;

/// Validates `?maxChanges=`: absent, empty or whitespace-only is `None`, the
/// default ([`default_max_changes`], matching how an empty `?waypoints=`
/// means "none"); otherwise it must be an integer in
/// `0..=`[`trip_planning_itinerary::MAX_CHANGES_LIMIT`]. Like
/// [`parse_waypoints`], this is a pure function checked before the permit and
/// any database read, so a rejected request costs a string parse, not a
/// query -- and it's a 400 naming the allowed range, never a silent clamp: a
/// plan computed under a different cap than the one asked for answers a
/// different question.
fn parse_max_changes(raw: Option<&str>) -> Result<Option<u32>, (StatusCode, String)> {
    use trip_planning_itinerary::{DEFAULT_MAX_CHANGES, MAX_CHANGES_LIMIT};

    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return Ok(None);
    }
    match raw.parse::<u32>() {
        Ok(value) if value <= MAX_CHANGES_LIMIT => Ok(Some(value)),
        _ => Err((
            StatusCode::BAD_REQUEST,
            format!(
                "maxChanges must be a whole number from 0 to {MAX_CHANGES_LIMIT} \
                 (default {DEFAULT_MAX_CHANGES}), not '{raw}'"
            ),
        )),
    }
}

/// `{"time": "HH:MM:SS", "dayOffset": n}` for a minutes-from-service-day-
/// midnight value (which may pass 1440).
#[expect(
    clippy::expect_used,
    reason = "a constant or range-checked time is always valid"
)]
fn segment_clock(minutes: u32) -> serde_json::Value {
    let time = NaiveTime::from_num_seconds_from_midnight_opt((minutes % 1440) * 60, 0)
        .expect("minutes modulo 1440 is a valid clock time");
    serde_json::json!({ "time": time, "dayOffset": minutes / 1440 })
}

/// Validates `?live=`: absent or empty means `true`; otherwise `true` or
/// `false` (any case). Anything else is a 400, before any database read.
fn parse_live(raw: Option<&str>) -> Result<bool, (StatusCode, String)> {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "" | "true" => Ok(true),
        "false" => Ok(false),
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("live must be 'true' or 'false', not '{other}'"),
        )),
    }
}

/// `?alternatives=` (DS-196): absent, empty or `true` is on.
fn parse_alternatives(raw: Option<&str>) -> Result<bool, (StatusCode, String)> {
    match raw.unwrap_or("").trim().to_ascii_lowercase().as_str() {
        "" | "true" => Ok(true),
        "false" => Ok(false),
        other => Err((
            StatusCode::BAD_REQUEST,
            format!("alternatives must be 'true' or 'false', not '{other}'"),
        )),
    }
}

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
        if let Some(unavailable) = crate::unavailable::response_for(&err) {
            return unavailable;
        }
        tracing::error!(error = ?err, operation, "trip plan request failed");
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("failed to {operation}"),
        )
    }
}

/// Pure, DB-free tests for this route's own query-param parsing -- no
/// `db_tests`-style `#[ignore]`/live-database dependency needed, since
/// `Query<TripPlanParams>::try_from_uri` is exactly the same deserialization
/// path axum's `FromRequestParts` impl for `Query` uses at request time (see
/// `axum::extract::query::Query::try_from_uri`, which
/// `from_request_parts` calls directly).
#[cfg(test)]
mod transit_db_tests;

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for a final-whole-branch-review finding: without
    /// `#[serde(rename_all = "camelCase")]` on `TripPlanParams`, this
    /// route's own documented wire contract (`?departAfter=17:00`, matching
    /// every other route in this crate) silently failed to populate
    /// `depart_after` at all -- `serde_urlencoded`'s `Query` extractor
    /// ignores unknown query keys rather than erroring, so the caller got
    /// no error and a plan silently searched from `00:00` instead of the
    /// requested time.
    #[test]
    fn depart_after_is_read_from_its_camel_case_wire_name() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=EUS&destination=MKC&\
                                     date=2026-09-23&departAfter=17:00"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(
            params.depart_after,
            Some(NaiveTime::from_hms_opt(17, 0, 0).unwrap()),
            "departAfter=17:00 must populate depart_after, not silently leave it None \
             (which the handler then defaults to NaiveTime::MIN, searching from 00:00)"
        );
    }

    /// The old, non-wire key must NOT work -- otherwise this test would
    /// pass for the wrong reason (e.g. some other default) rather than
    /// proving the camelCase rename is what did it.
    #[test]
    fn the_old_snake_case_key_no_longer_matches() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=EUS&destination=MKC&\
                                     date=2026-09-23&depart_after=17:00"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(
            params.depart_after, None,
            "depart_after (snake_case) is not this route's documented wire key; it must be \
             silently ignored just like any other unknown query key, not accidentally accepted"
        );
    }

    /// **The 2026-09-25 High 4b regression test**: `?waypoints=` had no count
    /// cap at all, and each waypoint costs a FULL extra pathfinding search
    /// over the whole day's connections graph (`plan_via_waypoints` solves one
    /// per consecutive pair of stops). A single unauthenticated GET carrying a
    /// few hundred repeated waypoints therefore bought a few hundred graph
    /// searches on one request -- the cheapest possible way to pin this
    /// process's CPU.
    ///
    /// Asserts the cap rejects with a 400 naming the limit, and -- crucially
    /// -- that it is checked at PARSE time, before any database read: this
    /// test never touches a pool.
    #[test]
    fn too_many_waypoints_are_rejected_before_any_database_read() {
        let raw = ["YRK"; MAX_WAYPOINTS + 1].join(",");
        let (status, message) =
            parse_waypoints(Some(&raw)).expect_err("more than the cap must be rejected");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            message.contains(&MAX_WAYPOINTS.to_string()),
            "the error must name the limit so a caller can fix the request: {message}"
        );
    }

    /// The other side of the cap: a generous-but-real itinerary is untouched,
    /// and the parsing (trim, uppercase, drop empties) still behaves exactly
    /// as it did before the cap existed.
    #[test]
    fn a_waypoint_list_at_the_cap_is_accepted_and_still_normalised() {
        let raw = ["yrk"; MAX_WAYPOINTS].join(" , ");
        let waypoints = parse_waypoints(Some(&raw)).expect("exactly the cap is allowed");
        assert_eq!(waypoints.len(), MAX_WAYPOINTS);
        assert!(
            waypoints
                .iter()
                .all(|w| w.label == "YRK" && w.is_single_station())
        );

        assert!(parse_waypoints(None).expect("absent is fine").is_empty());
        assert!(
            parse_waypoints(Some(" ,, "))
                .expect("empty entries are dropped")
                .is_empty()
        );
    }

    /// Omitted (or empty) `?maxChanges=` is the default, resolved once the
    /// waypoints and vias are known ([`default_max_changes`]).
    #[test]
    fn max_changes_is_none_when_omitted_or_empty() {
        assert_eq!(parse_max_changes(None), Ok(None));
        assert_eq!(parse_max_changes(Some("")), Ok(None));
        assert_eq!(parse_max_changes(Some("  ")), Ok(None));
    }

    /// The default is 4 (DS-85, 2026-10-24), stepping down towards the old
    /// default of 2 only where `options`' search-size guard needs it, so
    /// every request that left `maxChanges` out and fitted before still fits.
    #[test]
    fn the_default_max_changes_is_four_where_the_search_size_guard_allows() {
        assert_eq!(trip_planning_itinerary::DEFAULT_MAX_CHANGES, 4);
        let limit = DEFAULT_OPTIONS_SEARCH_SIZE;
        for results in ["fastest", "options"] {
            assert_eq!(default_max_changes(results, 0, 0, limit), 4, "{results}");
            assert_eq!(default_max_changes(results, MAX_WAYPOINTS, 0, limit), 4);
            assert_eq!(default_max_changes(results, 13, 1, limit), 4);
        }
        // fastest has no rounds, so its default never steps down.
        assert_eq!(default_max_changes("fastest", 20, 3, limit), 4);
        // options: 4 where it fits, else the most that does, never below 2.
        for (waypoints, vias, expected) in [
            (14, 1, 3), // 15 * 3 * 6 = 270 > 252; 15 * 3 * 5 = 225
            (20, 1, 2), // 21 * 3 * 5 = 315; 21 * 3 * 4 = 252
            (5, 3, 4),  // 6 * 7 * 6 = 252
            (6, 3, 3),  // 7 * 7 * 6 = 294; 7 * 7 * 5 = 245
            (8, 3, 2),  // 9 * 7 * 5 = 315; 9 * 7 * 4 = 252
            (9, 3, 2),  // over even at 2: 2, and its 400 as before
        ] {
            assert_eq!(
                default_max_changes("options", waypoints, vias, limit),
                expected,
                "{waypoints} waypoints, {vias} vias"
            );
        }
        // Every combination that fitted at the old default of 2 still fits.
        for waypoints in 0..=MAX_WAYPOINTS {
            for vias in 0..=MAX_VIAS {
                let old_fits = check_options_search_size("options", waypoints, vias, 2, limit);
                let new = default_max_changes("options", waypoints, vias, limit);
                assert!(new >= 2);
                if old_fits.is_ok() {
                    assert!(
                        check_options_search_size("options", waypoints, vias, new, limit).is_ok(),
                        "{waypoints} waypoints, {vias} vias"
                    );
                }
            }
        }
    }

    #[test]
    fn max_changes_accepts_every_value_from_zero_to_the_limit() {
        assert_eq!(trip_planning_itinerary::MAX_CHANGES_LIMIT, 12);
        for value in 0..=trip_planning_itinerary::MAX_CHANGES_LIMIT {
            assert_eq!(parse_max_changes(Some(&value.to_string())), Ok(Some(value)));
        }
        assert_eq!(parse_max_changes(Some(" 3 ")), Ok(Some(3)));
    }

    #[test]
    fn max_changes_out_of_range_or_non_integer_is_a_400_naming_the_range() {
        for bad in [
            "13",
            "16",
            "99",
            "-1",
            "abc",
            "2.5",
            "1e1",
            "3x",
            "99999999999999999999",
        ] {
            let (status, message) =
                parse_max_changes(Some(bad)).expect_err(&format!("'{bad}' must be rejected"));
            assert_eq!(status, StatusCode::BAD_REQUEST, "{bad}");
            assert!(
                message.contains("maxChanges")
                    && message.contains("0 to 12")
                    && message.contains("default 4"),
                "the error must name the parameter and its range: {message}"
            );
        }
    }

    /// `maxChanges` must be read from its camelCase wire name (the same
    /// silent-ignore trap `depart_after_is_read_from_its_camel_case_wire_name`
    /// guards against).
    #[test]
    fn max_changes_is_read_from_its_camel_case_wire_name() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=EUS&destination=MKC&\
                                     date=2026-09-23&maxChanges=4"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(params.max_changes.as_deref(), Some("4"));
    }

    #[test]
    fn depart_after_and_arrive_by_are_mutually_exclusive() {
        let at = |h| NaiveTime::from_hms_opt(h, 30, 0);
        let (status, message) = parse_time_bound(at(9), at(10), 0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("mutually exclusive"), "{message}");
        assert_eq!(
            parse_time_bound(None, at(10), 0),
            Ok(trip_planning_itinerary::TimeBound::ArriveBy(630))
        );
        assert_eq!(
            parse_time_bound(at(9), None, 0),
            Ok(trip_planning_itinerary::TimeBound::DepartAfter(570))
        );
        assert_eq!(
            parse_time_bound(None, None, 0),
            Ok(trip_planning_itinerary::TimeBound::DepartAfter(0))
        );
    }

    /// `dayOffset=1` moves either bound onto the next morning: past 1440,
    /// where a service date's post-midnight calls already sit.
    #[test]
    fn a_day_offset_moves_the_time_bound_past_midnight() {
        let at = |h| NaiveTime::from_hms_opt(h, 30, 0);
        assert_eq!(
            parse_time_bound(None, at(0), 1),
            Ok(trip_planning_itinerary::TimeBound::ArriveBy(1440 + 30))
        );
        assert_eq!(
            parse_time_bound(at(1), None, 1),
            Ok(trip_planning_itinerary::TimeBound::DepartAfter(1440 + 90))
        );
    }

    #[test]
    fn day_offset_is_0_or_1_and_needs_a_time() {
        assert_eq!(parse_day_offset(None, false), Ok(0));
        assert_eq!(parse_day_offset(Some(""), false), Ok(0));
        assert_eq!(parse_day_offset(Some(" 1 "), true), Ok(1));
        assert_eq!(parse_day_offset(Some("0"), true), Ok(0));
        for bad in ["2", "-1", "x", "1.0"] {
            let (status, message) = parse_day_offset(Some(bad), true).unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(message.contains("0 or 1"), "{bad}: {message}");
        }
        let (status, message) = parse_day_offset(Some("1"), false).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("departAfter or arriveBy"), "{message}");
    }

    #[test]
    fn across_midnight_is_true_or_false_and_needs_day_offset_1() {
        assert_eq!(parse_across_midnight(None, 0), Ok(false));
        assert_eq!(parse_across_midnight(Some(""), 1), Ok(false));
        assert_eq!(parse_across_midnight(Some("false"), 0), Ok(false));
        assert_eq!(parse_across_midnight(Some("TRUE"), 1), Ok(true));
        let (status, message) = parse_across_midnight(Some("true"), 0).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("needs dayOffset=1"), "{message}");
        let (status, message) = parse_across_midnight(Some("1"), 1).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("true or false"), "{message}");
    }

    #[test]
    fn day_offset_is_read_from_its_camel_case_wire_name() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=EUS&destination=MKC&\
                                     date=2026-09-23&arriveBy=00:30&dayOffset=1"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(params.day_offset.as_deref(), Some("1"));
    }

    #[test]
    fn the_new_parameters_are_read_from_their_camel_case_wire_names() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=EUS&destination=MKC&\
                                     date=2026-09-23&arriveBy=17:00&avoid=CRE&\
                                     avoidStop=BHM,wvh&avoidChange=CLJ"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(params.arrive_by, NaiveTime::from_hms_opt(17, 0, 0));
        assert_eq!(params.avoid.as_deref(), Some("CRE"));
        assert_eq!(params.avoid_stop.as_deref(), Some("BHM,wvh"));
        assert_eq!(params.avoid_change.as_deref(), Some("CLJ"));
    }

    fn labels(choices: &[StationChoice]) -> Vec<String> {
        choices.iter().map(|choice| choice.label.clone()).collect()
    }

    fn avoid_choices(avoid: &str, avoid_stop: &str, avoid_change: &str) -> AvoidChoices {
        AvoidChoices {
            avoid: parse_station_list("avoid", Some(avoid)).expect("valid avoid"),
            avoid_stop: parse_station_list("avoidStop", Some(avoid_stop)).expect("valid"),
            avoid_change: parse_station_list("avoidChange", Some(avoid_change)).expect("valid"),
        }
    }

    #[test]
    fn avoid_lists_are_normalised_capped_and_checked_against_the_stops() {
        assert_eq!(
            parse_station_list("avoid", Some(" cre, ,CRE,bhm ")).map(|c| labels(&c)),
            Ok(vec!["CRE".to_string(), "BHM".to_string()])
        );
        let raw = (0..=MAX_AVOIDED)
            .map(|i| format!("Z{i:02}"))
            .collect::<Vec<_>>()
            .join(",");
        let (status, message) = parse_station_list("avoidStop", Some(&raw)).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("avoidStop") && message.contains(&MAX_AVOIDED.to_string()));

        let lists = avoid_choices("", "", "YRK");
        let (status, message) =
            check_avoid_conflicts(&lists, "KGX", &["YRK".to_string()], "EDB").unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            message,
            "avoidChange: 'YRK' is a waypoint; a trip cannot avoid it"
        );
        assert!(check_avoid_conflicts(&lists, "KGX", &[], "EDB").is_ok());
    }

    /// 2026-10-07: an avoid list takes named groups, expanded into their
    /// members; `|` is a 400; the expanded size is capped; a member that is
    /// an end or a waypoint is a 400 naming the group.
    #[test]
    fn avoid_lists_take_station_groups() {
        let lists = avoid_choices("group:lon, CRE", "", "");
        assert_eq!(labels(&lists.avoid), vec!["group:LON", "CRE"]);
        let applied = lists.applied();
        assert_eq!(applied.avoid.len(), 19);
        assert!(applied.avoid.contains(&"KGX".to_string()));
        assert!(applied.avoid_stop.is_empty());
        assert_eq!(
            used_station_groups(lists.all()).keys().collect::<Vec<_>>(),
            vec!["LON"]
        );
        assert!(check_avoid_conflicts(&lists, "BHM", &[], "MAN").is_ok());
        let (status, message) = check_avoid_conflicts(&lists, "BHM", &[], "PAD").unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            message,
            "avoid: 'group:LON' includes 'PAD', which is the destination; a trip cannot avoid it"
        );

        let (status, message) = parse_station_list("avoidStop", Some("KGX|EUS")).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("only via"), "{message}");
        let (_, message) = parse_station_list("avoid", Some("group:NOPE")).unwrap_err();
        assert!(message.contains("not a known station group"), "{message}");
        // A group is one entry against MAX_AVOIDED (the same group twice is
        // one entry): 18 + 7 stations in 8 entries.
        let seven = (0..7)
            .map(|i| format!("Z{i:02}"))
            .collect::<Vec<_>>()
            .join(",");
        let full = parse_station_list("avoid", Some(&format!("group:LON,GROUP:lon,{seven}")))
            .expect("8 entries");
        assert_eq!(expand_choices(&full).len(), 25);
        assert!(parse_station_list("avoid", Some(&format!("group:LON,{seven},Z99"))).is_err());
    }

    #[test]
    fn via_is_read_from_its_wire_name_normalised_ordered_and_capped() {
        let uri: axum::http::Uri =
            "http://example.com/Trips/plan?origin=EUS&destination=MAN&date=2026-10-06&via=wfj,CRE"
                .parse()
                .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        assert_eq!(params.via.as_deref(), Some("wfj,CRE"));
        let parsed = |raw: Option<&str>| parse_vias(raw).map(|vias| labels(&vias));
        assert_eq!(
            parsed(params.via.as_deref()),
            Ok(vec!["WFJ".to_string(), "CRE".to_string()])
        );
        // Order is kept; a repeat that is not in a row is allowed.
        assert_eq!(
            parsed(Some(" cre , wfj,, CRE")),
            Ok(vec![
                "CRE".to_string(),
                "WFJ".to_string(),
                "CRE".to_string()
            ])
        );
        assert_eq!(parsed(None), Ok(Vec::new()));
        assert_eq!(parsed(Some(" , ")), Ok(Vec::new()));

        let raw = (0..=MAX_VIAS)
            .map(|i| format!("Z{i:02}"))
            .collect::<Vec<_>>()
            .join(",");
        let (status, message) = parse_vias(Some(&raw)).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("via") && message.contains(&MAX_VIAS.to_string()));
        let (status, message) = parse_vias(Some("CRE,cre")).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("twice in a row"), "{message}");
        // A bus stop's or ferry terminal's `tiploc:` code, as for waypoints.
        assert_eq!(
            parsed(Some("Tiploc:sanwbus,lev")),
            Ok(vec!["tiploc:SANWBUS".to_string(), "LEV".to_string()])
        );
        let (_, message) = parse_vias(Some("tiploc:sanwbus,TIPLOC:SANWBUS")).unwrap_err();
        assert!(message.contains("twice in a row"), "{message}");
    }

    /// 2026-10-07: OR choices. `|` alternatives and `group:` names make one
    /// via each; the caps; the same set twice in a row however written.
    #[test]
    fn a_via_may_be_an_or_choice_of_stations_and_groups() {
        let uri: axum::http::Uri = "http://example.com/Trips/plan?origin=CBG&destination=BTN\
                                    &date=2026-10-07&via=kgx%7Ceus,group:LON%7CCBG,BHM"
            .parse()
            .expect("parse uri");
        let Query(params) =
            Query::<TripPlanParams>::try_from_uri(&uri).expect("valid query string");
        let vias = parse_vias(params.via.as_deref()).expect("valid vias");
        assert_eq!(labels(&vias), vec!["KGX|EUS", "group:LON|CBG", "BHM"]);
        assert_eq!(vias[0].codes, vec!["KGX", "EUS"]);
        assert_eq!(vias[1].codes.len(), 19);
        assert!(vias[2].is_single_station());
        assert_eq!(
            used_station_groups(vias.iter()).get("LON").map(Vec::len),
            Some(18)
        );

        // Three groups of 18 fit (54 stations); one more alternative is over.
        assert!(parse_vias(Some("group:LON,KGX,group:LON")).is_ok());
        assert!(parse_vias(Some("group:LON,KGX|EUS,group:LON")).is_ok());
        let err = |raw: &str| parse_vias(Some(raw)).unwrap_err();
        let twenty_five = (0..25)
            .map(|i| format!("Z{i:02}"))
            .collect::<Vec<_>>()
            .join("|");
        let (status, message) = err(&twenty_five);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            message.contains("25 stations") && message.contains("at most 24"),
            "{message}"
        );
        let (_, message) =
            err("group:LON|Z01|Z02|Z03|Z04|Z05,group:LON|Z06|Z07|Z08|Z09|Z10,group:LON|Z11");
        assert!(message.contains("at most 54"), "{message}");
        let (_, message) = err("KGX|EUS,eus|kgx");
        assert!(message.contains("twice in a row"), "{message}");
        let (_, message) = err("group:LON,group:lon");
        assert!(message.contains("twice in a row"), "{message}");
        let (_, message) = err("group:NOWHERE");
        assert!(message.contains("not a known station group"), "{message}");
        let (_, message) = err("KGX|");
        assert!(message.contains("empty alternative"), "{message}");
        // Still at most three vias, a group counting as one.
        let (_, message) = err("group:LON,KGX,EUS,PAD");
        assert!(message.contains("at most 3"), "{message}");
    }

    /// Waypoints take OR choices too (2026-10-07): a choice is one
    /// waypoint against the cap, of at most 24 stations, and all the
    /// waypoints together stand for at most 54.
    #[test]
    fn waypoint_choices_parse_and_are_capped() {
        let waypoints = parse_waypoints(Some("YRK, group:lon ,kgx|eus")).expect("valid");
        let labels: Vec<&str> = waypoints.iter().map(|w| w.label.as_str()).collect();
        assert_eq!(labels, vec!["YRK", "group:LON", "KGX|EUS"]);
        assert_eq!(waypoints[1].codes.len(), 18);
        assert!(waypoints[0].is_single_station() && !waypoints[2].is_single_station());

        let err = |raw: &str| parse_waypoints(Some(raw)).unwrap_err();
        let (status, message) = err("group:LON|Z01|Z02|Z03|Z04|Z05|Z06|Z07");
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            message,
            "waypoints: 'group:LON|Z01|Z02|Z03|Z04|Z05|Z06|Z07' stands for 25 stations, at most \
             24 allowed in one waypoint"
        );
        let (_, message) = err("group:LON,YRK,group:LON,NCL,group:LON");
        assert_eq!(
            message,
            "waypoints: the waypoints stand for 56 stations together, at most 54 allowed"
        );
        let (_, message) = err("group:NOWHERE");
        assert!(message.starts_with("waypoints: 'group:NOWHERE' is not a known station group"));
        let (_, message) = err("KGX|");
        assert_eq!(message, "waypoints: 'KGX|' has an empty alternative");
        // A group is one waypoint against the count cap.
        let many = vec!["group:LON"; MAX_WAYPOINTS + 1].join(",");
        let (_, message) = err(&many);
        assert!(
            message.starts_with("too many waypoints: 21 given"),
            "{message}"
        );
        assert!(parse_waypoints(Some("group:LON,YRK,group:LON,NCL")).is_ok());
    }

    /// A choice's avoided members are dropped (a 400 only when none is
    /// left); adjacent waypoints sharing a station, one of them a choice,
    /// are a 400. Two equal single stations are still `plan_trip`'s 400.
    #[test]
    fn waypoint_choices_drop_avoided_members_and_must_not_overlap() {
        let lists = |avoid: &[&str], avoid_stop: &[&str]| trip_planning_itinerary::AvoidLists {
            avoid: avoid.iter().map(ToString::to_string).collect(),
            avoid_stop: avoid_stop.iter().map(ToString::to_string).collect(),
            avoid_change: Vec::new(),
        };
        let apply = |raw: &str, avoid: &trip_planning_itinerary::AvoidLists| {
            apply_waypoint_choices(&parse_waypoints(Some(raw)).unwrap(), avoid)
        };
        let applied = apply("YRK,KGX|EUS|STP", &lists(&["EUS"], &["STP"])).unwrap();
        assert_eq!(
            applied,
            vec![vec!["YRK".to_string()], vec!["KGX".to_string()]]
        );
        let applied = apply("group:LON", &lists(&["KGX"], &[])).unwrap();
        assert_eq!(applied[0].len(), 17);

        let (status, message) = apply("KGX|EUS", &lists(&["KGX"], &["EUS"])).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            message,
            "waypoints: every station of 'KGX|EUS' is in an avoid list; a trip cannot stop at \
             one and avoid them all"
        );

        let none = lists(&[], &[]);
        let (_, message) = apply("group:LON,KGX", &none).unwrap_err();
        assert_eq!(
            message,
            "waypoints: 'group:LON' and 'KGX' are next to each other and share 'KGX'; one stop \
             there would satisfy both, so name stations they do not share or put another \
             waypoint between them"
        );
        let (_, message) = apply("KGX|EUS,group:LON", &none).unwrap_err();
        assert!(
            message.contains("'KGX|EUS' and 'group:LON' are next to each other"),
            "{message}"
        );
        assert!(apply("group:LON,CBG,group:LON", &none).is_ok());
        // Dropping the shared member resolves it.
        assert!(apply("KGX|EUS,STP|KGX", &lists(&["KGX"], &[])).is_ok());
        // Two single stations: left to plan_trip ("is the waypoint before it").
        assert!(apply("YRK,YRK", &none).is_ok());
    }

    /// An OR choice may include the origin or the destination (satisfied
    /// there) and avoided stations (never used); only a choice whose every
    /// station is in `avoid` is a 400.
    #[test]
    fn an_or_via_conflicts_only_when_every_station_is_avoided() {
        let none = trip_planning_itinerary::AvoidLists::default();
        let vias = parse_vias(Some("group:LON")).expect("valid");
        assert!(check_via_conflicts(&vias, &none, "KGX", "EDB").is_ok());
        assert!(check_via_conflicts(&vias, &none, "BTN", "VIC").is_ok());
        let vias = parse_vias(Some("KGX|EUS")).expect("valid");
        let some = trip_planning_itinerary::AvoidLists {
            avoid: vec!["KGX".to_string()],
            ..Default::default()
        };
        assert!(check_via_conflicts(&vias, &some, "CBG", "MAN").is_ok());
        let all = avoid_choices("group:LON", "", "").applied();
        let (status, message) = check_via_conflicts(&vias, &all, "CBG", "MAN").unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            message,
            "via: every station of 'KGX|EUS' is also in avoid; a trip cannot both pass \
             through one and avoid them all"
        );
        // A single station keeps its old messages.
        let vias = parse_vias(Some("KGX")).expect("valid");
        let (_, message) = check_via_conflicts(&vias, &all, "CBG", "MAN").unwrap_err();
        assert!(message.contains("'KGX' is also in avoid"), "{message}");
        let (_, message) = check_via_conflicts(&vias, &none, "KGX", "MAN").unwrap_err();
        assert!(message.contains("is the origin"), "{message}");
    }

    #[test]
    fn a_tiploc_via_equal_to_a_tiploc_origin_is_a_400() {
        let vias = parse_vias(Some("tiploc:sanwbus")).expect("valid vias");
        let none = trip_planning_itinerary::AvoidLists::default();
        let (status, message) =
            check_via_conflicts(&vias, &none, "tiploc:SANWBUS", "EDB").unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("the origin"), "{message}");
    }

    #[test]
    fn the_options_search_size_guard_bounds_waypoints_vias_and_rounds() {
        let limit = DEFAULT_OPTIONS_SEARCH_SIZE;
        assert_eq!(limit, 252);
        let fits = |waypoints, vias, max_changes| {
            check_options_search_size("options", waypoints, vias, max_changes, limit).is_ok()
        };
        // No vias: every waypoint count allowed up to maxChanges=10.
        for max_changes in 0..=10 {
            assert!(fits(MAX_WAYPOINTS, 0, max_changes), "{max_changes}");
        }
        // At the bound exactly, and one waypoint over it.
        for (waypoints, vias, max_changes) in [
            (18, 0, 11), // 19 * 1 * 13 = 247
            (17, 0, 12), // 18 * 1 * 14 = 252
            (20, 1, 2),  // 21 * 3 * 4 = 252
            (13, 1, 4),  // 14 * 3 * 6 = 252
            (11, 1, 5),  // 12 * 3 * 7 = 252
            (9, 1, 6),   // 10 * 3 * 8 = 240
            (7, 1, 8),   // 8 * 3 * 10 = 240
            (6, 1, 10),  // 7 * 3 * 12 = 252
            (5, 1, 12),  // 6 * 3 * 14 = 252
            (8, 3, 2),   // 9 * 7 * 4 = 252
            (5, 3, 4),   // 6 * 7 * 6 = 252
            (4, 3, 5),   // 5 * 7 * 7 = 245
            (3, 3, 6),   // 4 * 7 * 8 = 224
            (2, 3, 10),  // 3 * 7 * 12 = 252
            (1, 3, 12),  // 2 * 7 * 14 = 196
        ] {
            assert!(
                fits(waypoints, vias, max_changes),
                "{waypoints} {vias} {max_changes}"
            );
            let (status, message) =
                check_options_search_size("options", waypoints + 1, vias, max_changes, limit)
                    .unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(
                message.contains("too large a search")
                    && message.contains("at most 252")
                    && message.contains("results=fastest"),
                "{message}"
            );
        }
        // fastest is one sweep whatever maxChanges says.
        assert!(check_options_search_size("fastest", 20, 3, 6, limit).is_ok());
        // The configurable bound is honoured, and its floor still admits a
        // direct plan at the highest maxChanges.
        assert_eq!(MIN_OPTIONS_SEARCH_SIZE, 14);
        let highest = trip_planning_itinerary::MAX_CHANGES_LIMIT;
        assert!(
            check_options_search_size("options", 0, 0, highest, MIN_OPTIONS_SEARCH_SIZE).is_ok()
        );
        assert!(
            check_options_search_size("options", 1, 0, highest, MIN_OPTIONS_SEARCH_SIZE).is_err()
        );
        assert!(check_options_search_size("options", 20, 3, 6, MAX_OPTIONS_SEARCH_SIZE).is_err());
        assert!(check_options_search_size("options", 8, 3, 6, MAX_OPTIONS_SEARCH_SIZE).is_ok());
    }

    #[test]
    fn a_via_at_either_end_or_also_avoided_is_a_400() {
        let none = trip_planning_itinerary::AvoidLists::default();
        let vias = |codes: &[&str]| parse_vias(Some(&codes.join(","))).expect("valid vias");
        for (codes, needle) in [
            (vias(&["EUS"]), "the origin"),
            (vias(&["CRE", "MAN"]), "the destination"),
        ] {
            let (status, message) = check_via_conflicts(&codes, &none, "EUS", "MAN").unwrap_err();
            assert_eq!(status, StatusCode::BAD_REQUEST);
            assert!(message.contains(needle), "{message}");
        }
        let avoided = trip_planning_itinerary::AvoidLists {
            avoid: vec!["CRE".to_string()],
            ..Default::default()
        };
        let (_, message) =
            check_via_conflicts(&vias(&["CRE"]), &avoided, "EUS", "MAN").unwrap_err();
        assert!(message.contains("also in avoid"), "{message}");
        // Passing through without calling (avoidStop) or without changing
        // (avoidChange) is a meaningful combination.
        let loose = trip_planning_itinerary::AvoidLists {
            avoid_stop: vec!["CRE".to_string()],
            avoid_change: vec!["CRE".to_string()],
            ..Default::default()
        };
        assert!(check_via_conflicts(&vias(&["CRE"]), &loose, "EUS", "MAN").is_ok());
    }
}

/// Scaffolding copied verbatim from `routes::journeys::db_tests`'s own
/// `test_app`/`test_router`/`connect` (that module's own comment notes it
/// was itself copied from `routes::train::db_tests` -- the same
/// cross-file-duplication convention this crate uses throughout
/// `crates/api/src/routes/*.rs`'s own `db_tests` modules rather than a
/// shared crate-visible fixture). `test_app`/`test_router`/`connect` below
/// are that same fixture, trimmed to only what this file's tests actually
/// use: no `seed_session`/`post_json` (this endpoint is unauthenticated
/// and GET-only), no `cleanup_user` (this endpoint has no per-user state to
/// clean up -- only the ad-hoc `schedule_calling_points_full`/`stanox_crs`
/// rows the seeded-connection test inserts and deletes itself).
#[cfg(test)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    reason = "test code: casts of small known test values"
)]
mod db_tests {
    mod live_in_search;
    mod operator_filter;
    mod operator_rule;
    mod tfl_access;
    mod tfl_access_times;

    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use sqlx::PgPool;
    use tower::ServiceExt;

    use super::*;
    use crate::app::AppState;
    use crate::auth::oidc::{OidcClient, OidcConfig};
    use crate::data::config::{LineCatalogue, ServiceArguments};

    /// Every `ServiceArguments` field filled with an inert placeholder --
    /// this route doesn't read `config.lines` (or any other config field)
    /// at all. Copied from `routes::journeys::db_tests::test_app`'s own
    /// empty-index default.
    fn test_app(pool: PgPool) -> App {
        let config = ServiceArguments {
            bind_url: "0.0.0.0:0".to_string(),
            database_url: String::new(),
            migration_database_url: None,
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_mcp: "srv-ds-mcp".to_string(),
            transit_networks: Vec::new(),
            admin_group: String::new(),
            sso_issuer_url: "https://example.invalid".to_string(),
            sso_client_id: "test-client".to_string(),
            sso_client_secret: "test-secret".to_string(),
            sso_redirect_url: "https://example.invalid/callback".to_string(),
            sso_post_login_redirect_url: "https://example.invalid/".to_string(),
            session_ttl_days: 14,
            history_retention_days: 7,
            daily_stats_retention_days: 300,
            half_hourly_stats_retention_hours: 840,
            metrics_enabled: false,
            metrics_port: 9091,
            defaults_file: None,
            lines: LineCatalogue(vec![]),
            vapid_public_key: "test-vapid-public-key".to_string(),
            full_coverage_enabled_default: false,
            schedule_match_interval_secs: 300,
            reconciliation_sweep_interval_secs: 300,
            schedule_enrichment_grace_minutes: 30,
            backlog_match_sweep_interval_secs: 300,
            session_cleanup_interval_secs: 3600,
            background_loops: true,
            past_travel_retention_days: 548,
            stale_push_subscription_days: 365,
            inactive_account_retention_days: 0,
        };

        Arc::new(AppState {
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
            oidc: OidcClient::new(OidcConfig {
                issuer_url: "https://example.invalid".to_string(),
                client_id: "test-client".to_string(),
                client_secret: "test-secret".to_string(),
                redirect_url: "https://example.invalid/callback".to_string(),
            })
            .expect("construct placeholder oidc client"),
            internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                "https://example.invalid".to_string(),
                "test-internal-oauth-client".to_string(),
            )
            .expect("construct placeholder internal-oauth verifier"),
            delegated: Arc::default(),
            tfl_arrivals: Arc::default(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    /// The real `trips::router()`, mounted unprefixed exactly as `main.rs`
    /// does, turned into a `tower::Service` a test can drive with
    /// `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new().merge(router()).with_state(app)
    }

    async fn connect() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// Issues a GET against `router` and returns `(status, parsed JSON
    /// body)`. This route always returns either a JSON object body or a
    /// plain-text `(StatusCode, String)` error body, so wrapping the latter
    /// as a JSON string lets every case share one return shape -- same
    /// convention as `routes::journeys::db_tests::request`.
    async fn get(router: axum::Router, uri: String) -> (StatusCode, Value) {
        let req = Request::builder()
            .uri(uri)
            .body(Body::empty())
            .expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        let value = serde_json::from_slice(&bytes).unwrap_or_else(|_| {
            Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
        });
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn an_unresolvable_origin_crs_is_a_clear_error() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        // Seed at least one calling-point row for the date, so this fails
        // on CRS resolution specifically, not on the "no schedule data
        // published for this date" 404 path.
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANZZZ', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANZZZ', 1, 'MILTNKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=ZZZ&destination=MKC&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(
            body.as_str().unwrap().contains("ZZZ"),
            "error must name the unresolvable CRS: {body:?}"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLANZZZ'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_date_with_no_published_schedule_data_is_a_clear_404() {
        let pool = connect().await;
        let router = test_router(test_app(pool));
        let (status, body) = get(
            router,
            "/Trips/plan?origin=EUS&destination=MKC&date=2099-01-01".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(body.as_str().unwrap().contains("2099-01-01"));
    }

    /// The end-to-end half of the 2026-09-25 High 4b fix: the cap is enforced
    /// by the real route, and it fires BEFORE the "no schedule data published
    /// for this date" read -- proven by using a date nothing is published for
    /// (which would otherwise 404) and asserting a 400 instead. A rejected
    /// request must not have cost a whole-day row read.
    /// `GET /Trips/station-groups` (2026-10-07): the compiled-in groups,
    /// cacheable, with no database read (the pool here connects nowhere).
    #[tokio::test]
    async fn station_groups_are_listed_without_a_database() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://nobody@127.0.0.1:1/none")
            .expect("lazy pool");
        let request = Request::builder()
            .uri("/Trips/station-groups")
            .body(Body::empty())
            .expect("build request");
        let response = test_router(test_app(pool))
            .oneshot(request)
            .await
            .expect("oneshot");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(axum::http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=3600")
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: Value = serde_json::from_slice(&bytes).expect("json");
        let london = &body["groups"][0];
        assert_eq!(london["group"], "LON");
        assert_eq!(london["code"], "group:LON");
        assert_eq!(london["name"], "London Terminals");
        assert_eq!(london["members"].as_array().map(Vec::len), Some(18));
        assert_eq!(
            london["members"][6],
            serde_json::json!({"crs": "KGX", "name": "London Kings Cross"})
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_waypoint_flood_is_rejected_before_the_date_is_even_looked_up() {
        let pool = connect().await;
        let router = test_router(test_app(pool));
        let flood = ["YRK"; MAX_WAYPOINTS + 40].join(",");
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=EUS&destination=MKC&date=2099-01-01&waypoints={flood}"),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "a waypoint flood must be rejected outright, not planned: {body:?}"
        );
        let message = body.as_str().expect("plain-text error body");
        assert!(
            message.contains("waypoints"),
            "the error must say what was wrong: {message}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn an_invalid_results_value_is_a_clear_400() {
        let pool = connect().await;
        let router = test_router(test_app(pool));
        let (status, body) = get(
            router,
            "/Trips/plan?origin=EUS&destination=MKC&date=2026-09-23&results=quickest".to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let message = body.as_str().unwrap();
        assert!(message.contains("fastest"));
        assert!(message.contains("options"));
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn plan_via_waypoints_names_the_failing_segment() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANWP', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANWP', 1, 'MILTNKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANWP-EUS', 'EUS', 'EUSTON', 'LONDON EUSTON', 1), \
                    ('TESTPLANWP-MKC', 'MKC', 'MILTNKC', 'MILTON KEYNES CENTRAL', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=EUS&destination=MKC&waypoints=ZZZ&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(
            body.as_str().unwrap().contains("EUS -> ZZZ"),
            "error must name the failing segment: {body:?}"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLANWP'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANWP-%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// The sibling of `plan_via_waypoints_names_the_failing_segment` above:
    /// that test's failing segment happens to be the FIRST one
    /// (`EUS -> ZZZ`), so it doesn't exercise "an earlier segment resolved
    /// fine, and the error names the LATER one that didn't" -- a distinct
    /// case (`plan_via_waypoints`'s per-segment loop must keep going past
    /// a successful segment and still attribute the eventual failure
    /// correctly, not just report failure-at-index-0 correctly). Here
    /// `origin=EUS, waypoints=MKC, destination=ZZZ` makes the FIRST
    /// segment `EUS -> MKC` (both resolve -- no error), and the SECOND
    /// `MKC -> ZZZ` the one that fails.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn plan_via_waypoints_names_the_failing_segment_when_an_earlier_one_succeeded() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANWP2', 0, 'EUSTON', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANWP2', 1, 'MILTNKC', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANWP2-EUS', 'EUS', 'EUSTON', 'LONDON EUSTON', 1), \
                    ('TESTPLANWP2-MKC', 'MKC', 'MILTNKC', 'MILTON KEYNES CENTRAL', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=EUS&waypoints=MKC&destination=ZZZ&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        let message = body.as_str().unwrap();
        assert!(
            message.contains("MKC -> ZZZ"),
            "error must name the SECOND, failing segment, not the first (which resolved fine): {message:?}"
        );
        assert!(
            !message.contains("EUS -> MKC"),
            "the first segment resolved fine and must not be reported as the failure: {message:?}"
        );

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLANWP2'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANWP2-%'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_real_seeded_connection_is_found_end_to_end() {
        // Regression test for a final-whole-branch-review finding: the
        // origin/destination CRS codes here (and their underlying TIPLOCs)
        // must be entirely synthetic, not real ones like EUS/MKC. On any
        // database that already has real EUS/MKC reference data (i.e. any
        // real deployment), a search keyed on the real EUSTON/MILTNKC
        // TIPLOCs would match every real service touching them on this
        // date too, not just the one seeded below -- and a real, faster
        // service could easily beat the seeded one, breaking the
        // `trainUid == "TESTPLAN1"` assertion. Using synthetic TIPLOCs that
        // no real schedule data could ever reference keeps the search
        // space provably isolated.
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLAN1', 0, 'TESTPL1O', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLAN1', 1, 'TESTPL1D', 'terminate', '08:50:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLAN-ZZA', 'ZZA', 'TESTPL1O', 'TEST PLAN ORIGIN', 1), \
                    ('TESTPLAN-ZZB', 'ZZB', 'TESTPL1D', 'TEST PLAN DESTINATION', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=ZZA&destination=ZZB&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segments = body["segments"].as_array().expect("segments array");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0]["originCrs"], "ZZA");
        assert_eq!(segments[0]["destinationCrs"], "ZZB");
        let itineraries = segments[0]["itineraries"]
            .as_array()
            .expect("itineraries array");
        assert_eq!(itineraries.len(), 1);
        assert_eq!(itineraries[0]["legs"][0]["trainUid"], "TESTPLAN1");

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLAN1'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLAN-ZZ%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// A bus stop as origin and as destination (`tiploc:` codes, any case),
    /// modelled on St Andrews bus station -> Leuchars -> Edinburgh, with the
    /// default 5-minute bus change buffer: the bus reaches Leuchars at 07:11,
    /// so the 07:17 train (enough for the 5-minute change alone) is missed
    /// and the 07:30 taken.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        clippy::type_complexity,
        reason = "test code: one seeded network and three requests against it"
    )]
    async fn a_bus_stop_is_a_planner_origin_and_destination_with_the_change_buffer() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 10, 8).unwrap();
        let rows: &[(&str, i16, &str, &str, Option<&str>, Option<&str>)] = &[
            ("TESTBUS1", 0, "ZSANBUS", "origin", None, Some("07:00:00")),
            (
                "TESTBUS1",
                1,
                "ZLEUCHR",
                "terminate",
                Some("07:11:00"),
                None,
            ),
            ("TESTRL17", 0, "ZLEUCHR", "origin", None, Some("07:17:00")),
            (
                "TESTRL17",
                1,
                "ZEDINBR",
                "terminate",
                Some("08:10:00"),
                None,
            ),
            ("TESTRL30", 0, "ZLEUCHR", "origin", None, Some("07:30:00")),
            (
                "TESTRL30",
                1,
                "ZEDINBR",
                "terminate",
                Some("08:25:00"),
                None,
            ),
            ("TESTRL08", 0, "ZEDINBR", "origin", None, Some("08:00:00")),
            (
                "TESTRL08",
                1,
                "ZLEUCHR",
                "terminate",
                Some("08:50:00"),
                None,
            ),
            ("TESTBUS2", 0, "ZLEUCHR", "origin", None, Some("08:56:00")),
            (
                "TESTBUS2",
                1,
                "ZSANBUS",
                "terminate",
                Some("09:07:00"),
                None,
            ),
            ("TESTBUS3", 0, "ZLEUCHR", "origin", None, Some("09:00:00")),
            (
                "TESTBUS3",
                1,
                "ZSANBUS",
                "terminate",
                Some("09:11:00"),
                None,
            ),
        ];
        for (uid, seq, tiploc, kind, arrival, departure) in rows {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, $2, $3, $4, $5, $6::time, $7::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(date)
            .bind(uid)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(&pool)
            .await
            .expect("seed calling point");
        }
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTBUS-ZLE', 'ZLE', 'ZLEUCHR', 'ZED LEUCHARS', 1), \
                    ('TESTBUS-ZED', 'ZED', 'ZEDINBR', 'ZED EDINBURGH', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO tiploc_locations (tiploc, location_type, name, display_name, \
                 msn_code, bus_calls, source_sequence) \
             VALUES ('ZSANBUS', 'bus_stop', 'Zed Andrews', 'Zed Andrews (bus station)', \
                     'ZSA', 2, 980) \
             ON CONFLICT (tiploc) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed tiploc_locations");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router.clone(),
            format!(
                "/Trips/plan?origin=tiploc:ZSANBUS&destination=ZED&date={date}\
                 &departAfter=06:30&live=false"
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segment = &body["segments"][0];
        assert_eq!(segment["originCrs"], "tiploc:ZSANBUS");
        let legs = segment["itineraries"][0]["legs"].as_array().expect("legs");
        let uids: Vec<&str> = legs
            .iter()
            .filter_map(|leg| leg["trainUid"].as_str())
            .collect();
        assert_eq!(uids, ["TESTBUS1", "TESTRL30"], "{body:?}");
        assert_eq!(legs[0]["originCrs"], "tiploc:ZSANBUS");
        assert_eq!(legs[0]["destinationCrs"], "ZLE");

        // Back again, to the bus stop: the 08:50 arrival + 5 (change) + 5
        // (onto a bus) misses the 08:56 bus and takes the 09:00.
        let (status, body) = get(
            router.clone(),
            format!(
                "/Trips/plan?origin=ZED&destination=TIPLOC:zsanbus&date={date}\
                 &departAfter=07:45&live=false"
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segment = &body["segments"][0];
        assert_eq!(segment["destinationCrs"], "tiploc:ZSANBUS");
        let uids: Vec<&str> = segment["itineraries"][0]["legs"]
            .as_array()
            .expect("legs")
            .iter()
            .filter_map(|leg| leg["trainUid"].as_str())
            .collect();
        assert_eq!(uids, ["TESTRL08", "TESTBUS3"], "{body:?}");

        // An unknown stop is the usual clear 400.
        let (status, _) = get(
            router,
            format!("/Trips/plan?origin=tiploc:ZNOSUCH&destination=ZED&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid LIKE 'TESTBUS%' OR uid LIKE 'TESTRL%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTBUS-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tiploc_locations WHERE tiploc = 'ZSANBUS'")
            .execute(&pool)
            .await
            .ok();
    }

    /// A change between a bus stop and its parent station, modelled on the
    /// Terminal 3 bus stop 412 m from Heathrow Terminals 2 & 3: the bus
    /// reaches the stop at 10:00, the 5-minute bus buffer and an 8-minute
    /// walk reach HXX at 10:13, HXX's change time is 2, so the 10:14 train
    /// is missed and the 10:15 taken. The walk is a `kind: "transfer"`,
    /// `mode: "WALK"`, 8-minute leg from the stop's `tiploc:` code;
    /// arrive-by (the backward search) finds the same plan.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        clippy::type_complexity,
        reason = "test code: one seeded network and two requests against it"
    )]
    async fn a_bus_stop_links_to_its_parent_station_with_a_walk() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 10, 9).unwrap();
        let rows: &[(&str, i16, &str, &str, Option<&str>, Option<&str>)] = &[
            ("TESTWBUS", 0, "ZWOKING", "origin", None, Some("09:00:00")),
            (
                "TESTWBUS",
                1,
                "ZHTRBS3",
                "terminate",
                Some("10:00:00"),
                None,
            ),
            ("TESTWT14", 0, "ZHTRWAP", "origin", None, Some("10:14:00")),
            (
                "TESTWT14",
                1,
                "ZPADTON",
                "terminate",
                Some("10:29:00"),
                None,
            ),
            ("TESTWT15", 0, "ZHTRWAP", "origin", None, Some("10:15:00")),
            (
                "TESTWT15",
                1,
                "ZPADTON",
                "terminate",
                Some("10:30:00"),
                None,
            ),
        ];
        for (uid, seq, tiploc, kind, arrival, departure) in rows {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, $2, $3, $4, $5, $6::time, $7::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(date)
            .bind(uid)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(&pool)
            .await
            .expect("seed calling point");
        }
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, change_time_minutes, \
                 source_sequence) \
             VALUES ('TESTWALK-ZWK', 'ZWK', 'ZWOKING', 'ZED WOKING', NULL, 1), \
                    ('TESTWALK-ZHX', 'ZHX', 'ZHTRWAP', 'ZED HEATHROW', 2, 1), \
                    ('TESTWALK-ZPD', 'ZPD', 'ZPADTON', 'ZED PADDINGTON', NULL, 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        sqlx::query(
            "INSERT INTO stations (crs, name) VALUES ('ZHX', 'Zed Heathrow Terminals 2 & 3') \
             ON CONFLICT (crs) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stations");
        sqlx::query(
            "INSERT INTO tiploc_locations (tiploc, location_type, name, display_name, \
                 parent_crs, parent_source, parent_distance_m, bus_calls, source_sequence) \
             VALUES ('ZHTRBS3', 'bus_stop', 'Zed Terminal 3', 'Zed Terminal 3 (bus stop)', \
                     'ZHX', 'curated', 412, 1, 981) \
             ON CONFLICT (tiploc) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed tiploc_locations");

        let router = test_router(test_app(pool.clone()));
        for window in ["departAfter=08:30", "arriveBy=10:30"] {
            let (status, body) = get(
                router.clone(),
                format!("/Trips/plan?origin=ZWK&destination=ZPD&date={date}&{window}&live=false"),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{window}: {body:?}");
            let itinerary = &body["segments"][0]["itineraries"][0];
            let legs = itinerary["legs"]
                .as_array()
                .unwrap_or_else(|| panic!("{window}: no itinerary: {body:?}"));
            assert_eq!(legs.len(), 3, "{window}: {body:?}");
            assert_eq!(legs[0]["trainUid"], "TESTWBUS");
            assert_eq!(legs[0]["destinationCrs"], "tiploc:ZHTRBS3");
            assert_eq!(legs[1]["kind"], "transfer");
            assert_eq!(legs[1]["mode"], "WALK");
            assert_eq!(legs[1]["originCrs"], "tiploc:ZHTRBS3");
            assert_eq!(legs[1]["destinationCrs"], "ZHX");
            assert_eq!(legs[1]["minutes"], 8);
            assert_eq!(legs[2]["trainUid"], "TESTWT15", "{window}: {body:?}");
            assert_eq!(itinerary["changeCount"], 1);
        }

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid LIKE 'TESTW%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTWALK-%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM tiploc_locations WHERE tiploc = 'ZHTRBS3'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stations WHERE crs = 'ZHX'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn options_mode_excludes_results_over_the_cap_but_flags_when_capped() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        // Within-cap route: 2 changes (3 legs), ORIGIN -> A -> B -> DEST.
        // Over-cap-but-faster route: 3 changes (4 legs),
        // ORIGIN -> P -> Q -> R -> DEST, arriving strictly before the
        // within-cap route -- so `options` mode must exclude it from
        // `itineraries` but flag `cappedByMaxChanges: true`.
        //
        // Regression test for a final-whole-branch-review finding: ORIGIN
        // and DEST here are entirely synthetic CRS codes/TIPLOCs, not real
        // ones like EUS/MKC -- same reasoning as
        // `a_real_seeded_connection_is_found_end_to_end` above: real
        // background data for a real CRS/TIPLOC could add extra
        // within-cap routes and break the `itineraries.len() == 1`
        // assertion below.
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANCAPA', 0, 'TESTPCEO', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANCAPA', 1, 'TESTPLA', 'terminate', '08:15:00', NULL, 0), \
                    ($1, 'TESTPLANCAPB', 0, 'TESTPLA', 'origin', NULL, '08:20:00', 0), \
                    ($1, 'TESTPLANCAPB', 1, 'TESTPLB', 'terminate', '08:35:00', NULL, 0), \
                    ($1, 'TESTPLANCAPC', 0, 'TESTPLB', 'origin', NULL, '08:40:00', 0), \
                    ($1, 'TESTPLANCAPC', 1, 'TESTPCMD', 'terminate', '09:00:00', NULL, 0), \
                    ($1, 'TESTPLANFASTA', 0, 'TESTPCEO', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANFASTA', 1, 'TESTPLP', 'terminate', '08:10:00', NULL, 0), \
                    ($1, 'TESTPLANFASTB', 0, 'TESTPLP', 'origin', NULL, '08:15:00', 0), \
                    ($1, 'TESTPLANFASTB', 1, 'TESTPLQ', 'terminate', '08:20:00', NULL, 0), \
                    ($1, 'TESTPLANFASTC', 0, 'TESTPLQ', 'origin', NULL, '08:25:00', 0), \
                    ($1, 'TESTPLANFASTC', 1, 'TESTPLR', 'terminate', '08:30:00', NULL, 0), \
                    ($1, 'TESTPLANFASTD', 0, 'TESTPLR', 'origin', NULL, '08:35:00', 0), \
                    ($1, 'TESTPLANFASTD', 1, 'TESTPCMD', 'terminate', '08:45:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANCAP-ZZC', 'ZZC', 'TESTPCEO', 'TEST CAP ORIGIN', 1), \
                    ('TESTPLANCAP-ZZD', 'ZZD', 'TESTPCMD', 'TEST CAP DESTINATION', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        // No `stanox_crs` row (and so no `change_time_by_tiploc` entry) for
        // any of the intermediate TIPLOCs (TESTPLA/B, TESTPLP/Q/R) --
        // `minimum_change_time` falls back to its own
        // `DEFAULT_CHANGE_TIME` (5 minutes,
        // `schedule_query::interchange::DEFAULT_CHANGE_TIME`) for a TIPLOC
        // with no MSN record at all, which every gap below is seeded to
        // exactly meet.

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!(
                "/Trips/plan?origin=ZZC&destination=ZZD&date={date}&results=options&maxChanges=2"
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segments = body["segments"].as_array().expect("segments array");
        assert_eq!(segments.len(), 1);
        let itineraries = segments[0]["itineraries"]
            .as_array()
            .expect("itineraries array");
        for itinerary in itineraries {
            assert!(
                itinerary["changeCount"].as_u64().unwrap() <= 2,
                "{itinerary:?}"
            );
        }
        // The genuinely-within-cap 2-change route must actually be found
        // and returned -- `capped` is computed independently of
        // `within_cap` in `plan_segment`, so a regression that wrongly
        // drops the within-cap route would still leave `cappedByMaxChanges`
        // true and pass the loop above with an empty `itineraries` unless
        // this is asserted explicitly.
        assert_eq!(
            itineraries.len(),
            1,
            "the within-cap 2-change route must be found: {body:?}"
        );
        assert_eq!(itineraries[0]["changeCount"], 2, "{body:?}");
        assert_eq!(
            segments[0]["cappedByMaxChanges"], true,
            "a strictly faster, over-cap itinerary exists and must be flagged: {body:?}"
        );

        for uid in [
            "TESTPLANCAPA",
            "TESTPLANCAPB",
            "TESTPLANCAPC",
            "TESTPLANFASTA",
            "TESTPLANFASTB",
            "TESTPLANFASTC",
            "TESTPLANFASTD",
        ] {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = $1")
                .bind(uid)
                .execute(&pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANCAP-%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// DS-196: `results=options` adds another corridor to the Pareto set
    /// and labels both; a later train over the fast route is no option;
    /// `alternatives=false` is the bare set; arrive-by mirrors it; and
    /// `fastest` mode is unchanged, with no labels.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    #[expect(clippy::too_many_lines, reason = "one scenario, read top to bottom")]
    async fn options_mode_adds_a_labelled_alternative_corridor() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        let cleanup = || async {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid LIKE 'TESTALT%'")
                .execute(&pool)
                .await
                .ok();
            sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTALT-%'")
                .execute(&pool)
                .await
                .ok();
        };
        cleanup().await;
        // ZAL to ZAM: the fast route via ZAS at 09:00 and 09:30, and a
        // slower one via ZAC (40 minutes longer, within the bound).
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES \
              ($1, 'TESTALTF1', 0, 'TESTALTL', 'origin', NULL, '09:00:00', 0), \
              ($1, 'TESTALTF1', 1, 'TESTALTS', 'intermediate', '10:00:00', '10:01:00', 0), \
              ($1, 'TESTALTF1', 2, 'TESTALTM', 'terminate', '11:00:00', NULL, 0), \
              ($1, 'TESTALTF2', 0, 'TESTALTL', 'origin', NULL, '09:30:00', 0), \
              ($1, 'TESTALTF2', 1, 'TESTALTS', 'intermediate', '10:30:00', '10:31:00', 0), \
              ($1, 'TESTALTF2', 2, 'TESTALTM', 'terminate', '11:30:00', NULL, 0), \
              ($1, 'TESTALTC1', 0, 'TESTALTL', 'origin', NULL, '09:05:00', 0), \
              ($1, 'TESTALTC1', 1, 'TESTALTC', 'intermediate', '10:20:00', '10:22:00', 0), \
              ($1, 'TESTALTC1', 2, 'TESTALTM', 'terminate', '11:40:00', NULL, 0)",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTALT-ZAL', 'ZAL', 'TESTALTL', 'TEST ALT LONDON', 1), \
                    ('TESTALT-ZAS', 'ZAS', 'TESTALTS', 'TEST ALT STOKE', 1), \
                    ('TESTALT-ZAC', 'ZAC', 'TESTALTC', 'TEST ALT CREWE', 1), \
                    ('TESTALT-ZAM', 'ZAM', 'TESTALTM', 'TEST ALT MANCHESTER', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        let plan = |query: &str| {
            let pool = pool.clone();
            let uri =
                format!("/Trips/plan?origin=ZAL&destination=ZAM&date={date}&live=false&{query}");
            async move { get(test_router(test_app(pool)), uri).await }
        };
        let uids = |body: &Value| -> Vec<String> {
            body["segments"][0]["itineraries"]
                .as_array()
                .map(|itineraries| {
                    itineraries
                        .iter()
                        .map(|itinerary| {
                            itinerary["legs"][0]["trainUid"]
                                .as_str()
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect()
                })
                .unwrap_or_default()
        };

        let (status, body) = plan("departAfter=08:30&results=options").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(uids(&body), ["TESTALTF1", "TESTALTC1"], "{body:?}");
        let itineraries = &body["segments"][0]["itineraries"];
        assert_eq!(
            itineraries[0]["labels"],
            serde_json::json!([{"kind": "fastest"}]),
            "{body:?}"
        );
        assert_eq!(
            itineraries[1]["labels"],
            serde_json::json!([{"kind": "alternative", "viaCrs": "ZAC"}]),
            "{body:?}"
        );
        assert_eq!(
            body["journeys"][1]["labels"], itineraries[1]["labels"],
            "{body:?}"
        );

        let (status, body) = plan("departAfter=08:30&results=options&alternatives=false").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(uids(&body), ["TESTALTF1"], "{body:?}");
        assert_eq!(
            body["segments"][0]["itineraries"][0]["labels"],
            serde_json::json!([{"kind": "fastest"}])
        );

        // Arrive by 11:45: the 09:30 fast train leaves last; the ZAC
        // route is the alternative.
        let (status, body) = plan("arriveBy=11:45&results=options").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(uids(&body), ["TESTALTF2", "TESTALTC1"], "{body:?}");
        assert_eq!(
            body["journeys"][0]["labels"],
            serde_json::json!([{"kind": "leavesLatest"}]),
            "{body:?}"
        );

        let (status, body) = plan("departAfter=08:30").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(uids(&body), ["TESTALTF1"], "{body:?}");
        assert!(
            body["segments"][0]["itineraries"][0]
                .get("labels")
                .is_none(),
            "{body:?}"
        );
        assert!(body["journeys"][0].get("labels").is_none(), "{body:?}");

        let (status, body) = plan("results=options&alternatives=maybe").await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");

        cleanup().await;
    }

    /// `?maxChanges=` validation is enforced by the real route BEFORE the
    /// "no schedule data published for this date" read -- proven, like
    /// `a_waypoint_flood_is_rejected_before_the_date_is_even_looked_up`, by
    /// using a date nothing is published for (which would otherwise 404).
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn an_invalid_max_changes_is_a_clear_400_before_the_date_is_looked_up() {
        let pool = connect().await;
        for bad in ["13", "-1", "abc", "2.5", "4294967296"] {
            let router = test_router(test_app(pool.clone()));
            let (status, body) = get(
                router,
                format!("/Trips/plan?origin=EUS&destination=MKC&date=2099-01-01&maxChanges={bad}"),
            )
            .await;
            assert_eq!(
                status,
                StatusCode::BAD_REQUEST,
                "maxChanges={bad}: {body:?}"
            );
            let message = body.as_str().expect("plain-text error body");
            assert!(
                message.contains("maxChanges")
                    && message.contains("0 to 12")
                    && message.contains("default 4"),
                "the error must name the parameter and its range: {message}"
            );
        }
    }

    /// `max_changes_lifts_the_cap_only_when_asked`'s one route: 3 changes,
    /// its four seeded trains, not capped.
    fn assert_three_change_route(body: &Value) {
        let segment = &body["segments"][0];
        let itineraries = segment["itineraries"]
            .as_array()
            .expect("itineraries array");
        assert_eq!(itineraries.len(), 1, "{body:?}");
        assert_eq!(itineraries[0]["changeCount"], 3, "{body:?}");
        let uids: Vec<&str> = itineraries[0]["legs"]
            .as_array()
            .expect("legs array")
            .iter()
            .filter_map(|leg| leg["trainUid"].as_str())
            .collect();
        assert_eq!(
            uids,
            ["TESTPLANMXA", "TESTPLANMXB", "TESTPLANMXC", "TESTPLANMXD"],
            "{body:?}"
        );
        assert_eq!(segment["cappedByMaxChanges"], false, "{body:?}");
    }

    /// The end-to-end proof for `?maxChanges=`: a synthetic network whose
    /// ONLY route needs exactly 3 changes (4 legs,
    /// ORIGIN -> P -> Q -> R -> DEST). At the default cap of 4 (2 before
    /// 2026-10-24, DS-85) `options` mode returns it, and an explicit
    /// `maxChanges=4` is byte-for-byte the same response as omitting it; at
    /// `maxChanges=2` it returns nothing but flags `cappedByMaxChanges`; the
    /// new ceiling of 12 is accepted and changes nothing here.
    /// `fastest` mode ignores the cap as a limit (the over-cap route is still
    /// returned) and only uses it as the `exceedsRecommendedChanges` threshold.
    /// Synthetic CRS codes/TIPLOCs for the same isolation reason as
    /// `a_real_seeded_connection_is_found_end_to_end`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn max_changes_lifts_the_cap_only_when_asked() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
        // Every gap is exactly `DEFAULT_CHANGE_TIME` (5 minutes) -- see
        // `options_mode_excludes_results_over_the_cap_but_flags_when_capped`.
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANMXA', 0, 'TESTMXO', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANMXA', 1, 'TESTMXP', 'terminate', '08:10:00', NULL, 0), \
                    ($1, 'TESTPLANMXB', 0, 'TESTMXP', 'origin', NULL, '08:15:00', 0), \
                    ($1, 'TESTPLANMXB', 1, 'TESTMXQ', 'terminate', '08:20:00', NULL, 0), \
                    ($1, 'TESTPLANMXC', 0, 'TESTMXQ', 'origin', NULL, '08:25:00', 0), \
                    ($1, 'TESTPLANMXC', 1, 'TESTMXR', 'terminate', '08:30:00', NULL, 0), \
                    ($1, 'TESTPLANMXD', 0, 'TESTMXR', 'origin', NULL, '08:35:00', 0), \
                    ($1, 'TESTPLANMXD', 1, 'TESTMXD', 'terminate', '08:45:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANMX-ZZE', 'ZZE', 'TESTMXO', 'TEST MAXCHANGES ORIGIN', 1), \
                    ('TESTPLANMX-ZZF', 'ZZF', 'TESTMXD', 'TEST MAXCHANGES DESTINATION', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let plan = |query: &'static str| {
            let router = test_router(test_app(pool.clone()));
            async move {
                get(
                    router,
                    format!("/Trips/plan?origin=ZZE&destination=ZZF&date={date}{query}"),
                )
                .await
            }
        };

        // options, omitted => the default of 4: the real 3-change itinerary.
        let (status, omitted) = plan("&results=options").await;
        assert_eq!(status, StatusCode::OK, "{omitted:?}");
        assert_eq!(omitted["maxChanges"], 4, "{omitted:?}");
        assert_three_change_route(&omitted);

        // options, explicit 4 => identical to omitted.
        let (status, explicit_four) = plan("&results=options&maxChanges=4").await;
        assert_eq!(status, StatusCode::OK, "{explicit_four:?}");
        assert_eq!(explicit_four, omitted);

        // options, the new ceiling of 12 => the same route, echoed as 12.
        let (status, twelve) = plan("&results=options&maxChanges=12").await;
        assert_eq!(status, StatusCode::OK, "{twelve:?}");
        assert_eq!(twelve["maxChanges"], 12, "{twelve:?}");
        assert_three_change_route(&twelve);

        // options, 2 (the old default) => nothing, flagged as capped.
        let (status, two) = plan("&results=options&maxChanges=2").await;
        assert_eq!(status, StatusCode::OK, "{two:?}");
        assert_eq!(two["maxChanges"], 2, "{two:?}");
        let segment = &two["segments"][0];
        assert_eq!(
            segment["itineraries"]
                .as_array()
                .expect("itineraries array")
                .len(),
            0,
            "the only route needs 3 changes, over a cap of 2: {two:?}"
        );
        assert_eq!(segment["cappedByMaxChanges"], true, "{two:?}");

        // fastest: the same route either way, flagged against the cap in effect.
        let (status, fastest_default) = plan("").await;
        assert_eq!(status, StatusCode::OK, "{fastest_default:?}");
        assert_eq!(fastest_default["maxChanges"], 4, "{fastest_default:?}");
        let itinerary = &fastest_default["segments"][0]["itineraries"][0];
        assert_eq!(itinerary["changeCount"], 3, "{fastest_default:?}");
        assert_eq!(itinerary["exceedsRecommendedChanges"], false);
        assert_eq!(fastest_default["segments"][0]["cappedByMaxChanges"], false);

        let (status, fastest_two) = plan("&maxChanges=2").await;
        assert_eq!(status, StatusCode::OK, "{fastest_two:?}");
        let itinerary = &fastest_two["segments"][0]["itineraries"][0];
        assert_eq!(itinerary["changeCount"], 3, "{fastest_two:?}");
        assert_eq!(itinerary["exceedsRecommendedChanges"], true);

        for uid in ["TESTPLANMXA", "TESTPLANMXB", "TESTPLANMXC", "TESTPLANMXD"] {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = $1")
                .bind(uid)
                .execute(&pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANMX-%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// End-to-end regression test for the waypoint-chaining bug: the
    /// second segment must search from the first segment's 08:50 arrival
    /// plus the waypoint's (default, 5-minute) change time, so the 05:00
    /// `TESTPLANCHE` -- which used to be offered, since later segments
    /// searched from 00:00 -- is out and the 09:00 `TESTPLANCHO` is in.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_later_waypoint_segment_searches_from_the_previous_arrival() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 26).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             VALUES ($1, 'TESTPLANCH1', 0, 'TESTCHA', 'origin', NULL, '08:00:00', 0), \
                    ($1, 'TESTPLANCH1', 1, 'TESTCHB', 'terminate', '08:50:00', NULL, 0), \
                    ($1, 'TESTPLANCHE', 0, 'TESTCHB', 'origin', NULL, '05:00:00', 0), \
                    ($1, 'TESTPLANCHE', 1, 'TESTCHC', 'terminate', '06:00:00', NULL, 0), \
                    ($1, 'TESTPLANCHO', 0, 'TESTCHB', 'origin', NULL, '09:00:00', 0), \
                    ($1, 'TESTPLANCHO', 1, 'TESTCHC', 'terminate', '10:00:00', NULL, 0) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLANCH-ZXA', 'ZXA', 'TESTCHA', 'TEST CHAIN A', 1), \
                    ('TESTPLANCH-ZXB', 'ZXB', 'TESTCHB', 'TEST CHAIN B', 1), \
                    ('TESTPLANCH-ZXC', 'ZXC', 'TESTCHC', 'TEST CHAIN C', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let (status, body) = get(
            test_router(test_app(pool.clone())),
            format!("/Trips/plan?origin=ZXA&waypoints=ZXB&destination=ZXC&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segments = body["segments"].as_array().expect("segments array");
        assert_eq!(segments.len(), 2, "{body:?}");
        assert_eq!(
            segments[0]["itineraries"][0]["legs"][0]["trainUid"],
            "TESTPLANCH1"
        );
        assert_eq!(
            segments[1]["itineraries"][0]["legs"][0]["trainUid"], "TESTPLANCHO",
            "the onward train must leave after the first leg arrives: {body:?}"
        );
        assert_eq!(segments[1]["departAfter"]["time"], "08:55:00", "{body:?}");
        assert_eq!(segments[1]["departAfter"]["dayOffset"], 0, "{body:?}");

        for uid in ["TESTPLANCH1", "TESTPLANCHE", "TESTPLANCHO"] {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = $1")
                .bind(uid)
                .execute(&pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLANCH-%'")
            .execute(&pool)
            .await
            .ok();
    }

    // ---------------------------------------------------------------------
    // Live overlay (2026-09-28). Every test pins "now" to 09:30 BST on
    // 2026-10-05 and seeds synthetic stations/trains for that date.
    // ---------------------------------------------------------------------

    fn live_date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
    }

    fn live_now() -> chrono::DateTime<chrono::Utc> {
        "2026-10-05T08:30:00Z".parse().unwrap()
    }

    /// `(seq, tiploc, kind, arrival, departure)`.
    type SeedCall<'a> = (i32, &'a str, &'a str, Option<&'a str>, Option<&'a str>);

    /// `(uid, [(seq, tiploc, kind, arrival, departure)])` into
    /// `schedule_calling_points_full` for [`live_date`].
    async fn seed_schedule(pool: &PgPool, uid: &str, calls: &[SeedCall<'_>]) {
        for (seq, tiploc, kind, arrival, departure) in calls {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, $2, $3, $4, $5, $6::time, $7::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(live_date())
            .bind(uid)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(pool)
            .await
            .expect("seed calling point");
        }
    }

    async fn seed_stations(pool: &PgPool, prefix: &str, stations: &[(&str, &str)]) {
        for (crs, tiploc) in stations {
            sqlx::query(
                "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
                 VALUES ($1, $2, $3, $2, 1) ON CONFLICT (stanox) DO NOTHING",
            )
            .bind(format!("{prefix}-{crs}"))
            .bind(crs)
            .bind(tiploc)
            .execute(pool)
            .await
            .expect("seed stanox_crs");
        }
    }

    /// A `trains` row plus its TRUST state; returns `trains.id`.
    async fn seed_trust_state(
        pool: &PgPool,
        uid: &str,
        status: &str,
        delay_minutes: Option<i32>,
        updated_at: chrono::DateTime<chrono::Utc>,
    ) -> i64 {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO trains (train_uid, service_date) VALUES ($1, $2) \
             ON CONFLICT (train_uid, service_date) DO UPDATE SET train_uid = EXCLUDED.train_uid \
             RETURNING id",
        )
        .bind(uid)
        .bind(live_date())
        .fetch_one(pool)
        .await
        .expect("seed trains");
        sqlx::query(
            "INSERT INTO train_current_state (trains_id, status, delay_minutes, updated_at) \
             VALUES ($1, $2, $3, $4)",
        )
        .bind(id)
        .bind(status)
        .bind(delay_minutes)
        .bind(updated_at)
        .execute(pool)
        .await
        .expect("seed train_current_state");
        id
    }

    async fn cleanup_live(pool: &PgPool, uids: &[&str], stanox_prefix: &str) {
        for uid in uids {
            sqlx::query("DELETE FROM trains WHERE train_uid = $1")
                .bind(uid)
                .execute(pool)
                .await
                .ok();
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = $1")
                .bind(uid)
                .execute(pool)
                .await
                .ok();
        }
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE $1")
            .bind(format!("{stanox_prefix}-%"))
            .execute(pool)
            .await
            .ok();
    }

    /// `provisional`/`provisionalFrom` (2026-10-08): the same plan is firm
    /// when "now" is the service date itself and provisional when it is
    /// fifteen days before it; `provisionalFrom` follows the clock.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn plan_for_a_far_date_is_marked_provisional() {
        let pool = connect().await;
        cleanup_live(&pool, &["TWPROV1"], "TWPROV").await;
        seed_stations(&pool, "TWPROV", &[("ZPA", "ZPROVA"), ("ZPB", "ZPROVB")]).await;
        seed_schedule(
            &pool,
            "TWPROV1",
            &[
                (0, "ZPROVA", "origin", None, Some("10:00:00")),
                (1, "ZPROVB", "terminate", Some("10:30:00"), None),
            ],
        )
        .await;
        let uri = format!(
            "/Trips/plan?origin=ZPA&destination=ZPB&date={}&departAfter=09:00&live=false",
            live_date()
        );

        for (now, expected, from) in [
            (live_now(), false, "2026-10-13"),
            ("2026-09-20T08:30:00Z".parse().unwrap(), true, "2026-09-28"),
        ] {
            let _now = crate::routes::pin_london_now_for_tests(now);
            let (status, body) = get(test_router(test_app(pool.clone())), uri.clone()).await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(
                body["segments"][0]["itineraries"][0]["legs"][0]["trainUid"], "TWPROV1",
                "{body:?}"
            );
            assert_eq!(body["provisional"], expected, "{now}: {body:?}");
            assert_eq!(body["provisionalFrom"], from, "{body:?}");
        }

        cleanup_live(&pool, &["TWPROV1"], "TWPROV").await;
    }

    fn train_uids(body: &Value, segment: usize) -> Vec<String> {
        body["segments"][segment]["itineraries"][0]["legs"]
            .as_array()
            .expect("legs")
            .iter()
            .filter_map(|leg| leg["trainUid"].as_str().map(str::to_string))
            .collect()
    }

    /// A train TRUST says is cancelled is withdrawn and the plan re-run onto
    /// the next one; `live=false` still offers the cancelled one, with no
    /// live keys at all (the pre-overlay shape).
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn live_a_cancelled_train_is_replaced_and_live_false_is_timetable_only() {
        let pool = connect().await;
        let uids = ["TLVCANF", "TLVCANS"];
        cleanup_live(&pool, &uids, "TLVCAN").await;
        seed_stations(&pool, "TLVCAN", &[("ZQA", "TLVCA"), ("ZQB", "TLVCB")]).await;
        seed_schedule(
            &pool,
            "TLVCANF",
            &[
                (0, "TLVCA", "origin", None, Some("10:00:00")),
                (1, "TLVCB", "terminate", Some("10:30:00"), None),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TLVCANS",
            &[
                (0, "TLVCA", "origin", None, Some("10:05:00")),
                (1, "TLVCB", "terminate", Some("10:45:00"), None),
            ],
        )
        .await;
        let trains_id = seed_trust_state(&pool, "TLVCANF", "cancelled", None, live_now()).await;
        sqlx::query(
            "INSERT INTO train_reasons (trains_id, msg_type, reason_code, canx_type) \
             VALUES ($1, '0002', 'TG', 'AT ORIGIN')",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_reasons");

        let _now = crate::routes::pin_london_now_for_tests(live_now());
        let uri = |live: &str| {
            format!(
                "/Trips/plan?origin=ZQA&destination=ZQB&date={}&departAfter=09:45{live}",
                live_date()
            )
        };
        let (status, timetable) =
            get(test_router(test_app(pool.clone())), uri("&live=false")).await;
        assert_eq!(status, StatusCode::OK, "{timetable:?}");
        assert_eq!(train_uids(&timetable, 0), ["TLVCANF"]);
        assert!(timetable.get("live").is_none(), "{timetable:?}");
        let leg = &timetable["segments"][0]["itineraries"][0]["legs"][0];
        assert!(leg.get("live").is_none(), "{leg:?}");
        assert!(
            timetable["segments"][0]["itineraries"][0]
                .get("liveFeasible")
                .is_none()
        );

        // In-search, the first search already knows; otherwise one re-plan.
        for in_search in [true, false] {
            let _mode = set_in_search_for_tests(in_search);
            let (status, live) = get(test_router(test_app(pool.clone())), uri("")).await;
            assert_eq!(status, StatusCode::OK, "{live:?}");
            assert_eq!(live["live"]["applied"], true, "{live:?}");
            assert_eq!(live["live"]["inSearch"], in_search, "{live:?}");
            assert_eq!(live["live"]["replans"], u32::from(!in_search), "{live:?}");
            // In-search, every train of the date TRUST changes counts (other
            // tests' leftovers included); otherwise the one re-planned around.
            if !in_search {
                assert_eq!(live["live"]["adjustedTrains"], 1, "{live:?}");
            }
            assert_eq!(train_uids(&live, 0), ["TLVCANS"], "{live:?}");
            let itinerary = &live["segments"][0]["itineraries"][0];
            assert_eq!(itinerary["liveFeasible"], true);
            // The replacement has no live record: an explicit null.
            let leg = itinerary["legs"][0].as_object().unwrap();
            assert!(leg.contains_key("live") && leg["live"].is_null(), "{leg:?}");
            assert_eq!(leg["scheduledDeparture"], "10:05:00");
        }

        cleanup_live(&pool, &uids, "TLVCAN").await;
    }

    /// A feeder running 15 late misses its timetabled connection: the plan is
    /// re-run onto the next onward train, the late leg reports its delay and
    /// keeps its timetable times, and a TRUST state older than the staleness
    /// bound is ignored (the timetable connection stands).
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn live_a_delay_that_breaks_an_interchange_replans_unless_it_is_stale() {
        let pool = connect().await;
        let uids = ["TLVDLY1", "TLVDLY2", "TLVDLY3"];
        cleanup_live(&pool, &uids, "TLVDLY").await;
        seed_stations(
            &pool,
            "TLVDLY",
            &[("ZQC", "TLVDA"), ("ZQD", "TLVDB"), ("ZQE", "TLVDC")],
        )
        .await;
        seed_schedule(
            &pool,
            "TLVDLY1",
            &[
                (0, "TLVDA", "origin", None, Some("10:00:00")),
                (1, "TLVDB", "terminate", Some("10:30:00"), None),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TLVDLY2",
            &[
                (0, "TLVDB", "origin", None, Some("10:40:00")),
                (1, "TLVDC", "terminate", Some("11:00:00"), None),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TLVDLY3",
            &[
                (0, "TLVDB", "origin", None, Some("11:00:00")),
                (1, "TLVDC", "terminate", Some("11:20:00"), None),
            ],
        )
        .await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());
        let uri = format!(
            "/Trips/plan?origin=ZQC&destination=ZQE&date={}&departAfter=09:45",
            live_date()
        );

        // Stale: last updated two hours ago -> ignored.
        seed_trust_state(
            &pool,
            "TLVDLY1",
            "en_route",
            Some(15),
            live_now() - chrono::Duration::hours(2),
        )
        .await;
        let (status, stale) = get(test_router(test_app(pool.clone())), uri.clone()).await;
        assert_eq!(status, StatusCode::OK, "{stale:?}");
        assert_eq!(train_uids(&stale, 0), ["TLVDLY1", "TLVDLY2"], "{stale:?}");
        assert_eq!(stale["live"]["replans"], 0, "{stale:?}");

        // Fresh but only 3 late: 10:33 + 5 still makes the 10:40, so the
        // plan is annotated, not re-planned.
        let set_delay = |delay: i32| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "UPDATE train_current_state SET updated_at = $1, delay_minutes = $2 \
                     WHERE trains_id = (SELECT id FROM trains WHERE train_uid = 'TLVDLY1')",
                )
                .bind(live_now())
                .bind(delay)
                .execute(&pool)
                .await
                .expect("freshen");
            }
        };
        set_delay(3).await;
        let (status, late) = get(test_router(test_app(pool.clone())), uri.clone()).await;
        assert_eq!(status, StatusCode::OK, "{late:?}");
        assert_eq!(late["live"]["replans"], 0, "{late:?}");
        assert_eq!(train_uids(&late, 0), ["TLVDLY1", "TLVDLY2"], "{late:?}");
        let itinerary = &late["segments"][0]["itineraries"][0];
        assert_eq!(itinerary["legs"][0]["live"]["delayMinutes"], 3, "{late:?}");
        assert_eq!(itinerary["legs"][1]["live"], Value::Null, "{late:?}");
        assert_eq!(itinerary["liveFeasible"], true);
        assert_eq!(itinerary["totalDurationMinutes"], 57, "10:03 to 11:00");

        // Fresh: the 15-minute delay lands at 10:45, inside the 5-minute
        // change onto the 10:40 -> the 11:00: found by the first search
        // in-search, by a re-plan otherwise.
        set_delay(15).await;
        for in_search in [true, false] {
            let _mode = set_in_search_for_tests(in_search);
            let (status, fresh) = get(test_router(test_app(pool.clone())), uri.clone()).await;
            assert_eq!(status, StatusCode::OK, "{fresh:?}");
            assert_eq!(fresh["live"]["replans"], u32::from(!in_search), "{fresh:?}");
            assert_eq!(train_uids(&fresh, 0), ["TLVDLY1", "TLVDLY3"], "{fresh:?}");
            let itinerary = &fresh["segments"][0]["itineraries"][0];
            let late = &itinerary["legs"][0];
            assert_eq!(late["scheduledDeparture"], "10:00:00", "{late:?}");
            assert_eq!(late["scheduledArrival"], "10:30:00", "{late:?}");
            assert_eq!(late["live"]["status"], "Late", "{late:?}");
            assert_eq!(late["live"]["delayMinutes"], 15, "{late:?}");
            assert_eq!(late["live"]["arrivalDelayMinutes"], 15, "{late:?}");
            assert_eq!(late["live"]["cancelled"], false);
            assert!(late["live"]["interchangeFeasible"].is_null());
            assert!(itinerary["legs"][1]["live"].is_null());
            assert_eq!(itinerary["liveFeasible"], true);
            // 10:15 live departure to 11:20 arrival.
            assert_eq!(itinerary["totalDurationMinutes"], 65, "{itinerary:?}");
        }

        cleanup_live(&pool, &uids, "TLVDLY").await;
    }

    /// An EN ROUTE cancellation at an intermediate call (located through
    /// `train_reasons.loc_stanox` -> `stanox_crs`) cuts the train there:
    /// a trip to the call after it moves to another train, while a trip to
    /// the cut call itself still uses it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn live_an_en_route_cancellation_terminates_the_train_short() {
        let pool = connect().await;
        let uids = ["TLVENR1", "TLVENR2"];
        cleanup_live(&pool, &uids, "TLVENR").await;
        seed_stations(
            &pool,
            "TLVENR",
            &[("ZQF", "TLVEA"), ("ZQG", "TLVEB"), ("ZQH", "TLVEC")],
        )
        .await;
        seed_schedule(
            &pool,
            "TLVENR1",
            &[
                (0, "TLVEA", "origin", None, Some("10:00:00")),
                (
                    1,
                    "TLVEB",
                    "intermediate",
                    Some("10:20:00"),
                    Some("10:21:00"),
                ),
                (2, "TLVEC", "terminate", Some("10:40:00"), None),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TLVENR2",
            &[
                (0, "TLVEA", "origin", None, Some("10:30:00")),
                (1, "TLVEC", "terminate", Some("11:10:00"), None),
            ],
        )
        .await;
        let trains_id = seed_trust_state(&pool, "TLVENR1", "en_route", None, live_now()).await;
        sqlx::query(
            "INSERT INTO train_reasons (trains_id, msg_type, reason_code, canx_type, loc_stanox) \
             VALUES ($1, '0002', 'TG', 'EN ROUTE', 'TLVENR-ZQG')",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_reasons");

        let _now = crate::routes::pin_london_now_for_tests(live_now());
        let plan = |destination: &str| {
            format!(
                "/Trips/plan?origin=ZQF&destination={destination}&date={}&departAfter=09:45",
                live_date()
            )
        };
        let (status, beyond) = get(test_router(test_app(pool.clone())), plan("ZQH")).await;
        assert_eq!(status, StatusCode::OK, "{beyond:?}");
        assert_eq!(train_uids(&beyond, 0), ["TLVENR2"], "{beyond:?}");

        let (status, to_cut) = get(test_router(test_app(pool.clone())), plan("ZQG")).await;
        assert_eq!(status, StatusCode::OK, "{to_cut:?}");
        assert_eq!(train_uids(&to_cut, 0), ["TLVENR1"], "{to_cut:?}");
        let leg = &to_cut["segments"][0]["itineraries"][0]["legs"][0];
        assert_eq!(leg["live"]["cancelled"], false, "{leg:?}");

        cleanup_live(&pool, &uids, "TLVENR").await;
    }

    /// Latency of `/Trips/plan` with and without the live overlay on a real
    /// day's data. Does nothing unless `TRIP_PLAN_BENCH_NOW` (RFC 3339, the
    /// instant the data was snapshotted) is set; run it against a database
    /// holding a production snapshot, in release mode, with the graph cache
    /// on (`TRIP_PLAN_GRAPH_CACHE_DATES=2`):
    ///
    /// ```text
    /// TRIP_PLAN_BENCH_NOW=2026-09-28T06:46:30Z TRIP_PLAN_GRAPH_CACHE_DATES=2 \
    ///   cargo test --release -p api --lib bench_trip_plan_live -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "benchmark; see its doc comment"]
    async fn bench_trip_plan_live() {
        let Ok(raw_now) = std::env::var("TRIP_PLAN_BENCH_NOW") else {
            return;
        };
        let now: chrono::DateTime<chrono::Utc> = raw_now.parse().expect("RFC 3339");
        let pool = connect().await;
        let _pin = crate::routes::pin_london_now_for_tests(now);
        let date = crate::routes::london_today();
        let depart = now
            .with_timezone(&chrono_tz::Europe::London)
            .format("%H:%M")
            .to_string();
        let router = test_router(test_app(pool.clone()));
        let warm = std::time::Instant::now();
        let (status, _) = get(
            router.clone(),
            format!("/Trips/plan?origin=WAT&destination=WOK&date={date}&live=false"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        println!("graph build (first request): {:?}", warm.elapsed());
        let queries = [
            ("WAT", "WOK", ""),
            ("WAT", "SOU", ""),
            ("EUS", "MAN", ""),
            ("KGX", "EDB", ""),
            ("PAD", "BRI", ""),
            ("VIC", "BTN", ""),
            ("MAN", "LDS", ""),
            ("BHM", "NCL", ""),
            ("WAT", "BTN", "&waypoints=CLJ"),
            ("EUS", "GLC", "&waypoints=PRE"),
        ];
        for results in ["fastest", "options"] {
            for (origin, destination, extra) in queries {
                let mut timings = Vec::new();
                let mut summary = Value::Null;
                for live in ["false", "true"] {
                    let mut samples = Vec::new();
                    for _ in 0..5 {
                        let started = std::time::Instant::now();
                        let (status, body) = get(
                            router.clone(),
                            format!(
                                "/Trips/plan?origin={origin}&destination={destination}&date={date}\
                                 &departAfter={depart}&results={results}&live={live}{extra}"
                            ),
                        )
                        .await;
                        samples.push(started.elapsed());
                        assert_eq!(status, StatusCode::OK, "{body:?}");
                        if live == "true" {
                            summary = body["live"].clone();
                        }
                    }
                    samples.sort();
                    timings.push(samples[samples.len() / 2]);
                }
                println!(
                    "{results:8} {origin}->{destination}{extra:16} timetable {:>8.1?} live {:>8.1?} {summary}",
                    timings[0], timings[1]
                );
            }
        }
    }

    /// `bench_across_midnight`'s network: 676 `BQ`/`Q` stations and 20,000
    /// ten-stop `BX` trains on each of `date` and `next`.
    async fn seed_across_midnight_bench(pool: &PgPool, date: NaiveDate, next: NaiveDate) {
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             SELECT 'BQ-' || c, 'Q' || c, 'BQ' || c, 'Q' || c, 1 FROM ( \
               SELECT chr(65 + s / 26) || chr(65 + s % 26) AS c \
               FROM generate_series(0, 675) s) codes",
        )
        .execute(pool)
        .await
        .expect("seed stations");
        sqlx::query("SELECT setseed(0.42)")
            .execute(pool)
            .await
            .expect("seed the random generator");
        sqlx::query(
            "WITH trains AS ( \
               SELECT d AS service_date, 'BX' || lpad(t::text, 5, '0') AS uid, \
                      (300 + floor(random() * 1230))::int AS start, \
                      floor(random() * 676)::int AS s0, (1 + floor(random() * 40))::int AS step \
               FROM unnest(ARRAY[$1::date, $2::date]) d, generate_series(1, 20000) t), \
             stops AS ( \
               SELECT tr.*, k AS seq, tr.start + k * 8 AS minute, (tr.s0 + k * tr.step) % 676 AS st \
               FROM trains tr, generate_series(0, 9) k) \
             INSERT INTO schedule_calling_points_full \
               (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
             SELECT service_date, uid, seq, 'BQ' || chr(65 + st / 26) || chr(65 + st % 26), \
                    CASE WHEN seq = 0 THEN 'origin' WHEN seq = 9 THEN 'terminate' \
                         ELSE 'intermediate' END, \
                    CASE WHEN seq = 0 THEN NULL \
                         ELSE make_time((minute % 1440) / 60, minute % 60, 0) END, \
                    CASE WHEN seq = 9 THEN NULL \
                         ELSE make_time((minute % 1440) / 60, minute % 60, 0) END, \
                    (minute / 1440)::smallint \
             FROM stops",
        )
        .bind(date)
        .bind(next)
        .execute(pool)
        .await
        .expect("seed schedules");
    }

    /// Latency of `acrossMidnight` (2026-10-20) against one service date,
    /// end to end through the route, on a synthetic network seeded here:
    /// 676 stations and 20,000 ten-stop trains on each of two dates (about
    /// 180k connections a date; production is about 290k), some running
    /// past midnight. Does nothing unless `TRIP_PLAN_ACROSS_BENCH` is set.
    /// Seeds and removes `BX%` schedules and `BQ-%` stations; run it on a
    /// scratch database, in release mode:
    ///
    /// ```text
    /// TRIP_PLAN_ACROSS_BENCH=1 cargo test --release -p api --lib \
    ///   bench_across_midnight -- --ignored --nocapture
    /// ```
    #[tokio::test]
    #[ignore = "benchmark; see its doc comment"]
    async fn bench_across_midnight() {
        if std::env::var("TRIP_PLAN_ACROSS_BENCH").is_err() {
            return;
        }
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 11, 6).unwrap();
        let next = date.succ_opt().unwrap();
        let cleanup = || async {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid LIKE 'BX%'")
                .execute(&pool)
                .await
                .expect("clean schedules");
            sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'BQ-%'")
                .execute(&pool)
                .await
                .expect("clean stations");
        };
        cleanup().await;
        seed_across_midnight_bench(&pool, date, next).await;

        let router = test_router(test_app(pool.clone()));
        let pairs = [
            ("QAA", "QMZ"),
            ("QBC", "QXY"),
            ("QDE", "QPQ"),
            ("QFG", "QRS"),
            ("QHI", "QTU"),
            ("QJK", "QVW"),
            ("QLM", "QAB"),
            ("QNO", "QCD"),
        ];
        let cases = [
            ("arriveBy 12:00 (one date)", "&arriveBy=12:00"),
            ("arriveBy 00:30+1 (one date)", "&arriveBy=00:30&dayOffset=1"),
            (
                "arriveBy 00:30+1 acrossMidnight",
                "&arriveBy=00:30&dayOffset=1&acrossMidnight=true",
            ),
            ("departAfter 08:00 (one date)", "&departAfter=08:00"),
            (
                "departAfter 00:30+1 (one date)",
                "&departAfter=00:30&dayOffset=1",
            ),
            (
                "departAfter 00:30+1 acrossMidnight",
                "&departAfter=00:30&dayOffset=1&acrossMidnight=true",
            ),
        ];
        // Warm both dates' graphs.
        for day in [date, next] {
            let warm = std::time::Instant::now();
            let (status, body) = get(
                router.clone(),
                format!("/Trips/plan?origin=QAA&destination=QMZ&date={day}&live=false"),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            println!("graph build {day}: {:?}", warm.elapsed());
        }
        for results in ["fastest", "options"] {
            for (label, query) in cases {
                let mut samples = Vec::new();
                let mut found = 0;
                for (origin, destination) in pairs {
                    let mut runs = Vec::new();
                    for _ in 0..5 {
                        let started = std::time::Instant::now();
                        let (status, body) = get(
                            router.clone(),
                            format!(
                                "/Trips/plan?origin={origin}&destination={destination}\
                                 &date={date}&results={results}&live=false{query}"
                            ),
                        )
                        .await;
                        runs.push(started.elapsed());
                        assert_eq!(status, StatusCode::OK, "{body:?}");
                        if runs.len() == 1 && !body["journeys"].as_array().is_none_or(Vec::is_empty)
                        {
                            found += 1;
                        }
                    }
                    runs.sort();
                    samples.push(runs[2]);
                }
                samples.sort();
                println!(
                    "{results:8} {label:36} median {:>9.1?} max {:>9.1?} found {found}/{}",
                    samples[samples.len() / 2],
                    samples[samples.len() - 1],
                    pairs.len()
                );
            }
        }
        cleanup().await;
    }

    /// Live data is only applied for today's (or yesterday's) service date.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn live_is_not_applied_outside_the_live_window() {
        let pool = connect().await;
        let uids = ["TLVWIN1"];
        cleanup_live(&pool, &uids, "TLVWIN").await;
        seed_stations(&pool, "TLVWIN", &[("ZQJ", "TLVWA"), ("ZQK", "TLVWB")]).await;
        seed_schedule(
            &pool,
            "TLVWIN1",
            &[
                (0, "TLVWA", "origin", None, Some("10:00:00")),
                (1, "TLVWB", "terminate", Some("10:30:00"), None),
            ],
        )
        .await;
        let _now = crate::routes::pin_london_now_for_tests(live_now() + chrono::Duration::days(3));
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            format!(
                "/Trips/plan?origin=ZQJ&destination=ZQK&date={}",
                live_date()
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(body["live"]["applied"], false);
        assert_eq!(body["live"]["reason"], "outsideLiveWindow");
        assert!(
            body["segments"][0]["itineraries"][0]["legs"][0]
                .get("live")
                .is_none()
        );
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            format!(
                "/Trips/plan?origin=ZQJ&destination=ZQK&date={}&live=maybe",
                live_date()
            ),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");

        cleanup_live(&pool, &uids, "TLVWIN").await;
    }

    /// `/Trips/plan` train legs carry the CIF booked platform at the
    /// boarding and alighting calling points and the schedule's ATOC
    /// operator and headcode (`trip_leg_details::attach_leg_details`) -- present when the
    /// CIF-derived tables have them, explicit `null` when not.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn train_legs_carry_booked_platforms_operator_and_headcode_or_null() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset, platform) \
             VALUES ($1, 'TESTPLAT1', 0, 'TESTPT1O', 'origin', NULL, '08:00:00', 0, '3'), \
                    ($1, 'TESTPLAT1', 1, 'TESTPT1D', 'terminate', '08:50:00', NULL, 0, NULL) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
             (service_date, destination_crs, scheduled, train_uid, origin_crs, operator_atoc, headcode) \
             VALUES ($1, 'ZYB', '08:00:00', 'TESTPLAT1', 'ZYA', NULL, '1S00') ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed schedule_destination_departures");
        // The operator comes from `schedule_services` (2026-10-31), which
        // has every schedule, buses included: this one is a bus, with no
        // operator on its `schedule_destination_departures` row.
        sqlx::query(
            "INSERT INTO schedule_services (service_date, uid, mode, stp, operator_atoc) \
             VALUES ($1, 'TESTPLAT1', 'bus', 'P', 'SW') ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed schedule_services");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPLAT-ZYA', 'ZYA', 'TESTPT1O', 'TEST PLAT ORIGIN', 1), \
                    ('TESTPLAT-ZYB', 'ZYB', 'TESTPT1D', 'TEST PLAT DESTINATION', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        let router = test_router(test_app(pool.clone()));
        let (status, body) = get(
            router,
            format!("/Trips/plan?origin=ZYA&destination=ZYB&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let leg = &body["segments"][0]["itineraries"][0]["legs"][0];
        assert_eq!(leg["trainUid"], "TESTPLAT1", "{body:?}");
        assert_eq!(leg["bookedDeparturePlatform"], "3");
        let leg = leg.as_object().unwrap();
        assert!(
            leg.contains_key("bookedArrivalPlatform") && leg["bookedArrivalPlatform"].is_null()
        );
        assert_eq!(leg["operator"], "SW");
        assert_eq!(leg["headcode"], "1S00");

        // A blank CIF Train Identity (stored NULL) renders as an explicit
        // `"headcode": null`, not an omitted key.
        sqlx::query(
            "UPDATE schedule_destination_departures SET headcode = NULL \
             WHERE train_uid = 'TESTPLAT1' AND service_date = $1",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("blank the headcode");
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            format!("/Trips/plan?origin=ZYA&destination=ZYB&date={date}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let leg = body["segments"][0]["itineraries"][0]["legs"][0]
            .as_object()
            .unwrap();
        assert!(leg.contains_key("headcode") && leg["headcode"].is_null());
        assert_eq!(leg["operator"], "SW");

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TESTPLAT1'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TESTPLAT1'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM schedule_services WHERE uid = 'TESTPLAT1'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLAT-ZY%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// A schedule whose public (GBTT) times differ from its working (WTT)
    /// ones, including half-minute working times: the planner searches,
    /// times and changes on the public minutes (design doc
    /// docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md,
    /// §11 P6), while `scheduled*` keep the truncated working minute for one
    /// release and `public*` carry the public one.
    ///
    /// ```text
    ///              ZWA dep          ZWB arr / dep         ZWC arr   ZWD arr
    /// TESTPUBW1    10:00½ (09:58)   10:20½ (10:21) /      10:40
    ///                               10:21½ (10:21)        (10:44)
    /// TESTPUBW2                     dep 10:25 (10:25)               10:35
    /// TESTPUBW3                     dep 10:26 (10:26)               10:40
    /// ```
    /// (working time, public time in brackets; the default 5-minute change
    /// at ZWB). On working minutes TESTPUBW2 would make the change
    /// (10:20 + 5 = 10:25); on public ones it does not (10:21 + 5 = 10:26).
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        reason = "test code: one seeded network and five requests against it"
    )]
    async fn the_planner_uses_public_times_where_they_differ_from_working_times() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset, \
              working_arrival, working_departure, public_arrival, public_departure, \
              can_board, can_alight) \
             VALUES \
              ($1, 'TESTPUBW1', 0, 'TESTPWA', 'origin', NULL, '10:00:00', 0, \
               NULL, '10:00:30', NULL, '09:58:00', true, false), \
              ($1, 'TESTPUBW1', 1, 'TESTPWB', 'intermediate', '10:20:00', '10:21:00', 0, \
               '10:20:30', '10:21:30', '10:21:00', '10:21:00', true, true), \
              ($1, 'TESTPUBW1', 2, 'TESTPWC', 'terminate', '10:40:00', NULL, 0, \
               '10:40:00', NULL, '10:44:00', NULL, false, true), \
              ($1, 'TESTPUBW2', 0, 'TESTPWB', 'origin', NULL, '10:25:00', 0, \
               NULL, '10:25:00', NULL, '10:25:00', true, false), \
              ($1, 'TESTPUBW2', 1, 'TESTPWD', 'terminate', '10:35:00', NULL, 0, \
               '10:35:00', NULL, '10:35:00', NULL, false, true), \
              ($1, 'TESTPUBW3', 0, 'TESTPWB', 'origin', NULL, '10:26:00', 0, \
               NULL, '10:26:00', NULL, '10:26:00', true, false), \
              ($1, 'TESTPUBW3', 1, 'TESTPWD', 'terminate', '10:40:00', NULL, 0, \
               '10:40:00', NULL, '10:40:00', NULL, false, true) \
             ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTPUBW-ZWA', 'ZWA', 'TESTPWA', 'TEST PUBLIC A', 1), \
                    ('TESTPUBW-ZWB', 'ZWB', 'TESTPWB', 'TEST PUBLIC B', 1), \
                    ('TESTPUBW-ZWC', 'ZWC', 'TESTPWC', 'TEST PUBLIC C', 1), \
                    ('TESTPUBW-ZWD', 'ZWD', 'TESTPWD', 'TEST PUBLIC D', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        let plan = |query: String| {
            let pool = pool.clone();
            async move {
                get(
                    test_router(test_app(pool)),
                    format!("/Trips/plan?date={date}&live=false&{query}"),
                )
                .await
            }
        };

        // The leg: `scheduled*` is the public time (design doc §9 decision
        // 1, switched 2026-10-20), `working*` the working minute
        // (half-minute truncated), and the itinerary's duration runs public
        // departure to public arrival (09:58 -> 10:44).
        let (status, body) = plan("origin=ZWA&destination=ZWC&departAfter=09:55".into()).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let itinerary = &body["segments"][0]["itineraries"][0];
        let leg = &itinerary["legs"][0];
        assert_eq!(leg["trainUid"], "TESTPUBW1", "{body:?}");
        assert_eq!(leg["scheduledDeparture"], "09:58:00", "{leg:?}");
        assert_eq!(leg["publicDeparture"], "09:58:00", "{leg:?}");
        assert_eq!(leg["workingDeparture"], "10:00:00", "{leg:?}");
        assert_eq!(leg["scheduledArrival"], "10:44:00", "{leg:?}");
        assert_eq!(leg["publicArrival"], "10:44:00", "{leg:?}");
        assert_eq!(leg["workingArrival"], "10:40:00", "{leg:?}");
        assert_eq!(leg["publicDepartureDayOffset"], 0, "{leg:?}");
        assert_eq!(leg["publicArrivalDayOffset"], 0, "{leg:?}");
        assert_eq!(itinerary["totalDurationMinutes"], 46, "{itinerary:?}");

        // Departing after 09:59: the train left at 09:58 by the public
        // timetable, though its working departure (10:00½) is later.
        let (status, body) = plan("origin=ZWA&destination=ZWC&departAfter=09:59".into()).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert!(
            body["segments"][0]["itineraries"]
                .as_array()
                .unwrap()
                .is_empty(),
            "the 09:58 public departure has gone: {body:?}"
        );

        // Arriving by 10:42: the public arrival (10:44, the recovery margin)
        // is too late, though the working one (10:40) is in time. By 10:44
        // it is in time.
        let (status, body) = plan("origin=ZWA&destination=ZWC&arriveBy=10:42".into()).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert!(
            body["segments"][0]["itineraries"]
                .as_array()
                .unwrap()
                .is_empty(),
            "the public arrival is 10:44: {body:?}"
        );
        let (status, body) = plan("origin=ZWA&destination=ZWC&arriveBy=10:44".into()).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(
            body["segments"][0]["itineraries"][0]["legs"][0]["trainUid"], "TESTPUBW1",
            "{body:?}"
        );

        // The change at ZWB is timed on public minutes: 10:21 + 5 misses
        // TESTPUBW2's 10:25 and takes TESTPUBW3's 10:26.
        let (status, body) = plan("origin=ZWA&destination=ZWD&departAfter=09:55".into()).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let legs: Vec<&Value> = body["segments"][0]["itineraries"][0]["legs"]
            .as_array()
            .expect("legs")
            .iter()
            .filter(|leg| leg["kind"] == "train")
            .collect();
        let uids: Vec<&str> = legs
            .iter()
            .map(|leg| leg["trainUid"].as_str().unwrap())
            .collect();
        assert_eq!(uids, ["TESTPUBW1", "TESTPUBW3"], "{body:?}");
        assert_eq!(legs[0]["publicArrival"], "10:21:00", "{body:?}");
        assert_eq!(legs[0]["scheduledArrival"], "10:21:00", "{body:?}");
        assert_eq!(legs[0]["workingArrival"], "10:20:00", "{body:?}");

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid LIKE 'TESTPUBW%'")
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPUBW-%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// Set-down-only and pick-up-only calls, as published (`can_board`,
    /// `can_alight`), shaped like the Caledonian Sleeper Highlander C04568
    /// (pick-up only at Crewe and Preston, set-down only from Stirling to
    /// Newtonmore) folded into one day:
    ///
    /// ```text
    /// TESTSLP1  ZUE 10:00 -> ZUC 11:00/11:02 (U) -> ZUP 12:00/12:02 (D)
    ///           -> ZUA 13:00/13:04 -> ZUI 14:00
    /// TESTSLP2  ZUP 12:30 -> ZUA 13:40
    /// TESTSLP3  ZUE 10:30 -> ZUC 11:40
    /// TESTSLP4  ZUE 09:00 -> ZUC 10:30
    /// ```
    ///
    /// `/Trips/plan` never boards TESTSLP1 at ZUP nor alights it at ZUC,
    /// depart-after or arrive-by, and offers the legal alternative instead.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        reason = "test code: one seeded network and six requests against it"
    )]
    async fn the_planner_honours_set_down_only_and_pick_up_only_calls() {
        let pool = connect().await;
        let date = NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        let cleanup = || async {
            sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid LIKE 'TESTSLP%'")
                .execute(&pool)
                .await
                .ok();
            sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTSLP-%'")
                .execute(&pool)
                .await
                .ok();
        };
        cleanup().await;
        sqlx::query(
            "INSERT INTO schedule_calling_points_full \
             (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset, \
              public_arrival, public_departure, can_board, can_alight) \
             VALUES \
              ($1, 'TESTSLP1', 0, 'TESTSDE', 'origin', NULL, '10:00:00', 0, \
               NULL, '10:00:00', true, false), \
              ($1, 'TESTSLP1', 1, 'TESTSDC', 'intermediate', '11:00:00', '11:02:00', 0, \
               NULL, '11:02:00', true, false), \
              ($1, 'TESTSLP1', 2, 'TESTSDP', 'intermediate', '12:00:00', '12:02:00', 0, \
               '12:00:00', NULL, false, true), \
              ($1, 'TESTSLP1', 3, 'TESTSDA', 'intermediate', '13:00:00', '13:04:00', 0, \
               '13:00:00', '13:04:00', true, true), \
              ($1, 'TESTSLP1', 4, 'TESTSDI', 'terminate', '14:00:00', NULL, 0, \
               '14:00:00', NULL, false, true), \
              ($1, 'TESTSLP2', 0, 'TESTSDP', 'origin', NULL, '12:30:00', 0, \
               NULL, '12:30:00', true, false), \
              ($1, 'TESTSLP2', 1, 'TESTSDA', 'terminate', '13:40:00', NULL, 0, \
               '13:40:00', NULL, false, true), \
              ($1, 'TESTSLP3', 0, 'TESTSDE', 'origin', NULL, '10:30:00', 0, \
               NULL, '10:30:00', true, false), \
              ($1, 'TESTSLP3', 1, 'TESTSDC', 'terminate', '11:40:00', NULL, 0, \
               '11:40:00', NULL, false, true), \
              ($1, 'TESTSLP4', 0, 'TESTSDE', 'origin', NULL, '09:00:00', 0, \
               NULL, '09:00:00', true, false), \
              ($1, 'TESTSLP4', 1, 'TESTSDC', 'terminate', '10:30:00', NULL, 0, \
               '10:30:00', NULL, false, true)",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed calling points");
        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence) \
             VALUES ('TESTSLP-ZUE', 'ZUE', 'TESTSDE', 'TEST SLEEPER EUSTON', 1), \
                    ('TESTSLP-ZUC', 'ZUC', 'TESTSDC', 'TEST SLEEPER CREWE', 1), \
                    ('TESTSLP-ZUP', 'ZUP', 'TESTSDP', 'TEST SLEEPER PERTH', 1), \
                    ('TESTSLP-ZUA', 'ZUA', 'TESTSDA', 'TEST SLEEPER AVIEMORE', 1), \
                    ('TESTSLP-ZUI', 'ZUI', 'TESTSDI', 'TEST SLEEPER INVERNESS', 1) \
             ON CONFLICT (stanox) DO NOTHING",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");
        let plan = |query: &str| {
            let pool = pool.clone();
            let uri = format!("/Trips/plan?date={date}&live=false&{query}");
            async move { get(test_router(test_app(pool)), uri).await }
        };
        let first_uids = |body: &Value| -> Vec<String> {
            body["segments"][0]["itineraries"][0]["legs"]
                .as_array()
                .map(|legs| {
                    legs.iter()
                        .filter(|leg| leg["kind"] == "train")
                        .map(|leg| leg["trainUid"].as_str().unwrap().to_string())
                        .collect()
                })
                .unwrap_or_default()
        };

        // Set-down only at ZUP: never boarded there.
        let (status, body) = plan("origin=ZUP&destination=ZUA&departAfter=11:50").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(first_uids(&body), ["TESTSLP2"], "{body:?}");
        let (status, body) = plan("origin=ZUP&destination=ZUA&arriveBy=13:30").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert!(
            body["segments"][0]["itineraries"]
                .as_array()
                .unwrap()
                .is_empty(),
            "only boarding the set-down-only call would make 13:30: {body:?}"
        );

        // Pick-up only at ZUC: never alighted there.
        let (status, body) = plan("origin=ZUE&destination=ZUC&departAfter=09:55").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(first_uids(&body), ["TESTSLP3"], "{body:?}");
        let (status, body) = plan("origin=ZUE&destination=ZUC&arriveBy=11:30").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(first_uids(&body), ["TESTSLP4"], "{body:?}");

        // Riding through ZUC (U) and alighting at ZUP (D) is the legal use.
        let (status, body) = plan("origin=ZUE&destination=ZUP&departAfter=09:55").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(first_uids(&body), ["TESTSLP1"], "{body:?}");
        assert_eq!(
            body["segments"][0]["itineraries"][0]["legs"][0]["publicArrival"], "12:00:00",
            "{body:?}"
        );
        // And via ZUC: staying aboard through a pick-up-only call satisfies it.
        let (status, body) = plan("origin=ZUE&destination=ZUA&via=ZUC&departAfter=09:55").await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(first_uids(&body), ["TESTSLP1"], "{body:?}");

        cleanup().await;
    }

    // ---------------------------------------------------------------------
    // Arrive-by, avoid lists and noResultReason (2026-09-29), live on and
    // off. Same live date and pinned "now" (09:30 BST) as above.
    //
    // ZVA -> ZVC, with ZVB a calling point and ZVP a station run through:
    //   TAVF1  ZVA 10:00 -> (passes ZVP) -> ZVC 10:40   fast, direct
    //   TAVS1  ZVA 09:50 -> ZVB 10:10/10:12 -> ZVC 10:50
    //   TAVE1  ZVA 09:40 -> ZVC 10:30                     earlier, direct
    // ---------------------------------------------------------------------

    async fn seed_arrive_by_network(pool: &PgPool) -> [&'static str; 3] {
        let uids = ["TAVF1", "TAVS1", "TAVE1"];
        cleanup_live(pool, &uids, "TAVNET").await;
        seed_stations(
            pool,
            "TAVNET",
            &[
                ("ZVA", "TAVA"),
                ("ZVB", "TAVB"),
                ("ZVC", "TAVC"),
                ("ZVP", "TAVP"),
            ],
        )
        .await;
        seed_schedule(
            pool,
            "TAVF1",
            &[
                (0, "TAVA", "origin", None, Some("10:00:00")),
                (1, "TAVP", "intermediate", None, None),
                (2, "TAVC", "terminate", Some("10:40:00"), None),
            ],
        )
        .await;
        seed_schedule(
            pool,
            "TAVS1",
            &[
                (0, "TAVA", "origin", None, Some("09:50:00")),
                (
                    1,
                    "TAVB",
                    "intermediate",
                    Some("10:10:00"),
                    Some("10:12:00"),
                ),
                (2, "TAVC", "terminate", Some("10:50:00"), None),
            ],
        )
        .await;
        seed_schedule(
            pool,
            "TAVE1",
            &[
                (0, "TAVA", "origin", None, Some("09:40:00")),
                (1, "TAVC", "terminate", Some("10:30:00"), None),
            ],
        )
        .await;
        uids
    }

    fn arrive_by_uri(query: &str) -> String {
        format!(
            "/Trips/plan?origin=ZVA&destination=ZVC&date={}{query}",
            live_date()
        )
    }

    // ---------------------------------------------------------------------
    // Overnight (2026-10-19): `dayOffset=1` puts the time bound on the next
    // morning, on `date`'s trains.
    //
    //   TOVN2  ZOA 23:00 -> ZOB 23:50
    //   TOVN1  ZOA 23:30 -> ZOB 00:20 (+1)
    //   TOVN3  ZOA 00:15 (+1) -> ZOB 00:45 (+1)
    // and on the NEXT service date (2026-10-20, `acrossMidnight`):
    //   TOVN4  ZOA 00:05 -> ZOB 00:25
    // ---------------------------------------------------------------------

    /// `(uid, seq, tiploc, kind, arrival, departure, day_offset)`.
    type OvernightCall<'a> = (
        &'a str,
        i32,
        &'a str,
        &'a str,
        Option<&'a str>,
        Option<&'a str>,
        i16,
    );

    async fn seed_overnight_network(pool: &PgPool) -> [&'static str; 4] {
        let uids = ["TOVN1", "TOVN2", "TOVN3", "TOVN4"];
        cleanup_live(pool, &uids, "TOVNET").await;
        seed_stations(pool, "TOVNET", &[("ZOA", "TOVA"), ("ZOB", "TOVB")]).await;
        let rows: [OvernightCall<'_>; 6] = [
            ("TOVN2", 0, "TOVA", "origin", None, Some("23:00:00"), 0),
            ("TOVN2", 1, "TOVB", "terminate", Some("23:50:00"), None, 0),
            ("TOVN1", 0, "TOVA", "origin", None, Some("23:30:00"), 0),
            ("TOVN1", 1, "TOVB", "terminate", Some("00:20:00"), None, 1),
            ("TOVN3", 0, "TOVA", "origin", None, Some("00:15:00"), 1),
            ("TOVN3", 1, "TOVB", "terminate", Some("00:45:00"), None, 1),
        ];
        for (uid, seq, tiploc, kind, arrival, departure, day_offset) in rows {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, $2, $3, $4, $5, $6::time, $7::time, $8) ON CONFLICT DO NOTHING",
            )
            .bind(live_date())
            .bind(uid)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .bind(day_offset)
            .execute(pool)
            .await
            .expect("seed calling point");
        }
        let next = live_date().succ_opt().expect("a next day");
        for (seq, tiploc, kind, arrival, departure) in [
            (0, "TOVA", "origin", None, Some("00:05:00")),
            (1, "TOVB", "terminate", Some("00:25:00"), None),
        ] {
            sqlx::query(
                "INSERT INTO schedule_calling_points_full \
                 (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
                 VALUES ($1, 'TOVN4', $2, $3, $4, $5::time, $6::time, 0) ON CONFLICT DO NOTHING",
            )
            .bind(next)
            .bind(seq)
            .bind(tiploc)
            .bind(kind)
            .bind(arrival)
            .bind(departure)
            .execute(pool)
            .await
            .expect("seed next-day calling point");
        }
        uids
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn day_offset_plans_across_midnight_live_on_and_off() {
        let pool = connect().await;
        let uids = seed_overnight_network(&pool).await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());

        for live in ["&live=false", ""] {
            let plan = |query: &str| {
                let pool = pool.clone();
                let uri = format!(
                    "/Trips/plan?origin=ZOA&destination=ZOB&date={}{query}{live}",
                    live_date()
                );
                async move { get(test_router(test_app(pool)), uri).await }
            };

            // By 00:30 the next morning: the 23:30, arriving 00:20.
            let (status, body) = plan("&arriveBy=00:30&dayOffset=1").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(train_uids(&body, 0), ["TOVN1"], "{live} {body:?}");
            assert_eq!(
                body["arriveBy"],
                serde_json::json!({"time": "00:30:00", "dayOffset": 1}),
                "{body:?}"
            );
            assert!(body["departAfter"].is_null(), "{body:?}");
            assert_eq!(
                body["journeys"][0]["arrival"],
                serde_json::json!({"time": "00:20:00", "dayOffset": 1}),
                "{body:?}"
            );

            // By 00:30 on the service date itself: nothing (the trains are
            // all late evening), with the earliest arrival named.
            let (status, body) = plan("&arriveBy=00:30").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(
                body["segments"][0]["noResultReason"]["constraint"], "arriveBy",
                "{body:?}"
            );

            // By 23:59: the 23:00, not the 00:20 arrival.
            let (_, body) = plan("&arriveBy=23:59").await;
            assert_eq!(train_uids(&body, 0), ["TOVN2"], "{live} {body:?}");

            // Depart after 00:10 the next morning: the 00:15, and the echo.
            let (status, body) = plan("&departAfter=00:10&dayOffset=1").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(train_uids(&body, 0), ["TOVN3"], "{live} {body:?}");
            assert_eq!(
                body["departAfter"],
                serde_json::json!({"time": "00:10:00", "dayOffset": 1}),
                "{body:?}"
            );
            assert!(body["arriveBy"].is_null(), "{body:?}");

            // No time given: the echo is the effective 00:00 start.
            let (_, body) = plan("").await;
            assert_eq!(
                body["departAfter"],
                serde_json::json!({"time": "00:00:00", "dayOffset": 0}),
                "{body:?}"
            );
        }

        // The 400s, checked before any search.
        for (query, needle) in [
            ("&arriveBy=00:30&dayOffset=2", "0 or 1"),
            ("&dayOffset=1", "departAfter or arriveBy"),
            ("&departAfter=09:00&arriveBy=10:00", "mutually exclusive"),
        ] {
            let uri = format!(
                "/Trips/plan?origin=ZOA&destination=ZOB&date={}{query}",
                live_date()
            );
            let (status, body) = get(test_router(test_app(pool.clone())), uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body:?}");
            assert!(body.to_string().contains(needle), "{query}: {body:?}");
        }

        // The same station at both ends: arrive-by answers as depart-after.
        let same = |query: &'static str| {
            let pool = pool.clone();
            let uri = format!(
                "/Trips/plan?origin=ZOA&destination=ZOA&date={}{query}&live=false",
                live_date()
            );
            async move { get(test_router(test_app(pool)), uri).await }
        };
        let (depart_status, depart) = same("&departAfter=22:00").await;
        let (arrive_status, arrive) = same("&arriveBy=23:59").await;
        assert_eq!(depart_status, arrive_status, "{depart:?} {arrive:?}");
        assert_eq!(
            depart["segments"][0]["itineraries"]
                .as_array()
                .map(Vec::len),
            arrive["segments"][0]["itineraries"]
                .as_array()
                .map(Vec::len),
            "{depart:?} {arrive:?}"
        );
        for body in [&depart, &arrive] {
            assert_eq!(
                body["segments"][0]["noResultReason"]["constraint"], "noRoute",
                "{body:?}"
            );
        }

        cleanup_live(&pool, &uids, "TOVNET").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn across_midnight_merges_the_next_service_dates_trains() {
        let pool = connect().await;
        let uids = seed_overnight_network(&pool).await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());
        let next = live_date().succ_opt().unwrap().to_string();

        for live in ["&live=false", ""] {
            let plan = |query: &str| {
                let pool = pool.clone();
                let uri = format!(
                    "/Trips/plan?origin=ZOA&destination=ZOB&date={}{query}{live}",
                    live_date()
                );
                async move { get(test_router(test_app(pool)), uri).await }
            };

            // Without it: only the 5th's trains, so the 23:30.
            let (_, body) = plan("&arriveBy=00:30&dayOffset=1").await;
            assert_eq!(train_uids(&body, 0), ["TOVN1"], "{live} {body:?}");
            assert_eq!(
                body["serviceDates"],
                serde_json::json!([live_date().to_string()]),
                "{body:?}"
            );

            // With it: the 6th's 00:05 leaves later and still arrives by
            // 00:30. Its time is on the 5th's clock (+1); the leg names its
            // own service date.
            let (status, body) = plan("&arriveBy=00:30&dayOffset=1&acrossMidnight=true").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(train_uids(&body, 0), ["TOVN4"], "{live} {body:?}");
            assert_eq!(
                body["serviceDates"],
                serde_json::json!([live_date().to_string(), next]),
                "{body:?}"
            );
            assert_eq!(
                body["journeys"][0]["departure"],
                serde_json::json!({"time": "00:05:00", "dayOffset": 1}),
                "{body:?}"
            );
            assert_eq!(
                body["segments"][0]["itineraries"][0]["legs"][0]["serviceDate"], next,
                "{body:?}"
            );

            // Depart after 00:00: the earliest arrival of either date (the
            // 6th's 00:25, not the 5th's 00:45).
            let (_, body) = plan("&departAfter=00:00&dayOffset=1&acrossMidnight=true").await;
            assert_eq!(train_uids(&body, 0), ["TOVN4"], "{live} {body:?}");

            // options: one journey per change count, deduplicated.
            let (_, body) =
                plan("&arriveBy=00:50&dayOffset=1&acrossMidnight=true&results=options").await;
            let itineraries = body["segments"][0]["itineraries"].as_array().unwrap();
            assert_eq!(itineraries.len(), 1, "all direct: {body:?}");
            assert_eq!(itineraries[0]["legs"][0]["trainUid"], "TOVN3", "{body:?}");
        }

        for (query, needle) in [
            ("&arriveBy=09:00&acrossMidnight=true", "needs dayOffset=1"),
            (
                "&arriveBy=00:30&dayOffset=1&acrossMidnight=yes",
                "true or false",
            ),
        ] {
            let uri = format!(
                "/Trips/plan?origin=ZOA&destination=ZOB&date={}{query}",
                live_date()
            );
            let (status, body) = get(test_router(test_app(pool.clone())), uri).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body:?}");
            assert!(body.to_string().contains(needle), "{query}: {body:?}");
        }

        cleanup_live(&pool, &uids, "TOVNET").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn arrive_by_and_avoid_end_to_end_live_on_and_off() {
        let pool = connect().await;
        let uids = seed_arrive_by_network(&pool).await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());

        for live in ["&live=false", ""] {
            let plan = |query: &str| {
                let pool = pool.clone();
                let uri = arrive_by_uri(&format!("{query}{live}"));
                async move { get(test_router(test_app(pool)), uri).await }
            };

            // The latest departure arriving by 10:45 is the 10:00.
            let (status, body) = plan("&arriveBy=10:45").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(train_uids(&body, 0), ["TAVF1"], "{live} {body:?}");
            assert_eq!(body["arriveBy"]["time"], "10:45:00", "{body:?}");
            let segment = &body["segments"][0];
            assert_eq!(segment["arriveBy"]["time"], "10:45:00", "{body:?}");
            assert!(segment["departAfter"].is_null(), "{body:?}");
            assert!(segment["noResultReason"].is_null(), "{body:?}");
            if live.is_empty() {
                assert_eq!(body["live"]["applied"], true, "{body:?}");
                assert_eq!(body["live"]["replans"], 0, "{body:?}");
                assert_eq!(segment["itineraries"][0]["liveFeasible"], true);
            } else {
                assert!(body.get("live").is_none(), "{body:?}");
            }

            // avoid ZVP: the 10:00 runs through it; the 09:50 is too late,
            // so the 09:40.
            let (status, body) = plan("&arriveBy=10:45&avoid=ZVP").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(train_uids(&body, 0), ["TAVE1"], "{live} {body:?}");
            assert_eq!(body["avoid"], serde_json::json!(["ZVP"]), "{body:?}");

            // avoidStop ZVP: the 10:00 never CALLS there, so it stands.
            let (status, body) = plan("&arriveBy=10:45&avoidStop=zvp").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            assert_eq!(train_uids(&body, 0), ["TAVF1"], "{live} {body:?}");
            assert_eq!(body["avoidStop"], serde_json::json!(["ZVP"]), "{body:?}");

            // Arrive by 10:55 avoiding ZVP: the 09:50 via ZVB -- unless it
            // may not call at ZVB either.
            let (_, body) = plan("&arriveBy=10:55&avoid=ZVP").await;
            assert_eq!(train_uids(&body, 0), ["TAVS1"], "{live} {body:?}");
            let (_, body) = plan("&arriveBy=10:55&avoid=ZVP&avoidStop=ZVB").await;
            assert_eq!(train_uids(&body, 0), ["TAVE1"], "{live} {body:?}");
            // ...but staying aboard through ZVB is fine under avoidChange.
            let (_, body) = plan("&arriveBy=10:55&avoid=ZVP&avoidChange=ZVB").await;
            assert_eq!(train_uids(&body, 0), ["TAVS1"], "{live} {body:?}");

            // options: one itinerary per change count, latest first-found.
            let (_, body) = plan("&arriveBy=10:55&results=options").await;
            let itineraries = body["segments"][0]["itineraries"]
                .as_array()
                .expect("itineraries");
            assert_eq!(itineraries.len(), 1, "all direct: {body:?}");
            assert_eq!(itineraries[0]["legs"][0]["trainUid"], "TAVF1");

            // Too early: nothing arrives by 10:00; the earliest arrival is
            // 10:30 -- named, not just an empty list.
            let (status, body) = plan("&arriveBy=10:00").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            let reason = &body["segments"][0]["noResultReason"];
            assert_eq!(reason["constraint"], "arriveBy", "{body:?}");
            assert_eq!(reason["values"], serde_json::json!(["10:00"]));
            assert!(
                reason["message"].as_str().unwrap().contains("10:30"),
                "{reason:?}"
            );

            // Depart after 09:45 while avoiding ZVP and never calling at ZVB:
            // only the 09:40 is left, and it has gone. Dropping `avoid`
            // alone brings back the 10:00, so `avoid` is named.
            let (status, body) = plan("&departAfter=09:45&avoid=ZVP&avoidStop=ZVB").await;
            assert_eq!(status, StatusCode::OK, "{body:?}");
            let reason = &body["segments"][0]["noResultReason"];
            assert_eq!(reason["constraint"], "avoid", "{body:?}");
            assert_eq!(reason["values"], serde_json::json!(["ZVP"]));
            assert!(
                reason["message"]
                    .as_str()
                    .unwrap()
                    .contains("one exists without that restriction"),
                "{reason:?}"
            );
        }

        // Validation, before and after the graph is read.
        for (query, needle) in [
            ("&departAfter=09:00&arriveBy=10:00", "mutually exclusive"),
            ("&avoid=ZVA", "the origin"),
            ("&avoidChange=ZVC", "the destination"),
            ("&avoid=ZVZ", "'ZVZ' is not a recognised station"),
        ] {
            let (status, body) =
                get(test_router(test_app(pool.clone())), arrive_by_uri(query)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body:?}");
            assert!(body.as_str().unwrap().contains(needle), "{query}: {body:?}");
        }

        cleanup_live(&pool, &uids, "TAVNET").await;
    }

    /// Live on: the 10:00 running 10 late would arrive at 10:50, after an
    /// `arriveBy` of 10:45. The plan is marked, re-planned within the live
    /// budget, and lands on the 09:40; `live=false` still offers the 10:00.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn live_a_delay_past_arrive_by_replans_onto_an_earlier_train() {
        let pool = connect().await;
        let uids = seed_arrive_by_network(&pool).await;
        seed_trust_state(&pool, "TAVF1", "en_route", Some(10), live_now()).await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());
        // The re-plan loop on its own; in-search, the first search already
        // lands there (`live_in_search::arrive_by_searches_on_live_times`).
        let _mode = set_in_search_for_tests(false);

        let (status, timetable) = get(
            test_router(test_app(pool.clone())),
            arrive_by_uri("&arriveBy=10:45&live=false"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{timetable:?}");
        assert_eq!(train_uids(&timetable, 0), ["TAVF1"], "{timetable:?}");

        let (status, live) = get(
            test_router(test_app(pool.clone())),
            arrive_by_uri("&arriveBy=10:45"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{live:?}");
        assert_eq!(live["live"]["applied"], true, "{live:?}");
        assert_eq!(live["live"]["replans"], 1, "{live:?}");
        assert_eq!(train_uids(&live, 0), ["TAVE1"], "{live:?}");
        assert_eq!(
            live["segments"][0]["itineraries"][0]["liveFeasible"], true,
            "{live:?}"
        );

        // options mode, whose budget is one re-plan by default, lands on the
        // same answer.
        let (status, options) = get(
            test_router(test_app(pool.clone())),
            arrive_by_uri("&arriveBy=10:45&results=options"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{options:?}");
        assert_eq!(options["live"]["replans"], 1, "{options:?}");
        assert_eq!(train_uids(&options, 0), ["TAVE1"], "{options:?}");

        cleanup_live(&pool, &uids, "TAVNET").await;
    }

    // ---------------------------------------------------------------------
    // Waypoints as one journey (2026-09-29): no phantom change on a through
    // train, maxChanges over the whole journey, and the waypoint cap.
    // ---------------------------------------------------------------------

    /// ZJA -> ZJB (via) -> ZJC. TWJT1 calls at ZJB for two minutes (a change
    /// needs the default five); TWJT2 is the next train on. The traveller
    /// stays aboard TWJT1: no change, one journey, live on and off, depart-
    /// after and arrive-by.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn a_through_train_at_a_waypoint_is_not_a_change_end_to_end() {
        let pool = connect().await;
        let uids = ["TWJT1", "TWJT2"];
        cleanup_live(&pool, &uids, "TWJNET").await;
        seed_stations(
            &pool,
            "TWJNET",
            &[("ZJA", "TWJA"), ("ZJB", "TWJB"), ("ZJC", "TWJC")],
        )
        .await;
        seed_schedule(
            &pool,
            "TWJT1",
            &[
                (0, "TWJA", "origin", None, Some("10:00:00")),
                (
                    1,
                    "TWJB",
                    "intermediate",
                    Some("10:30:00"),
                    Some("10:32:00"),
                ),
                (2, "TWJC", "terminate", Some("11:00:00"), None),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TWJT2",
            &[
                (0, "TWJB", "origin", None, Some("10:40:00")),
                (1, "TWJC", "terminate", Some("11:30:00"), None),
            ],
        )
        .await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());

        for query in [
            "&departAfter=09:45",
            "&departAfter=09:45&live=false",
            "&arriveBy=11:15",
            "&arriveBy=11:15&results=options&maxChanges=0",
        ] {
            let (status, body) = get(
                test_router(test_app(pool.clone())),
                format!(
                    "/Trips/plan?origin=ZJA&waypoints=ZJB&destination=ZJC&date={}{query}",
                    live_date()
                ),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{query}: {body:?}");
            assert_eq!(train_uids(&body, 0), ["TWJT1"], "{query}: {body:?}");
            assert_eq!(train_uids(&body, 1), ["TWJT1"], "{query}: {body:?}");
            let onward = &body["segments"][1]["itineraries"][0];
            assert_eq!(onward["continuesPreviousTrain"], true, "{query}: {body:?}");
            assert_eq!(onward["legs"][0]["scheduledDeparture"], "10:32:00");
            assert_eq!(
                body["segments"][0]["itineraries"][0]["continuesPreviousTrain"],
                false
            );
            assert_eq!(body["journeys"][0]["changeCount"], 0, "{query}: {body:?}");
            assert_eq!(body["journeys"][0]["departure"]["time"], "10:00:00");
            assert_eq!(body["journeys"][0]["arrival"]["time"], "11:00:00");
            if !query.contains("live=false") {
                assert_eq!(body["journeys"][0]["liveFeasible"], true, "{body:?}");
            }
        }

        cleanup_live(&pool, &uids, "TWJNET").await;
    }

    /// ZMD -> ZME -> ZMF -> ZMG with a direct train per segment: each
    /// segment alone has no change, the journey has two. `maxChanges=1`
    /// finds nothing and says so; `maxChanges=2` finds it.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn max_changes_caps_the_whole_journey_across_two_waypoints() {
        let pool = connect().await;
        let uids = ["TWMC1", "TWMC2", "TWMC3"];
        cleanup_live(&pool, &uids, "TWMNET").await;
        seed_stations(
            &pool,
            "TWMNET",
            &[
                ("ZMD", "TWMD"),
                ("ZME", "TWME"),
                ("ZMF", "TWMF"),
                ("ZMG", "TWMG"),
            ],
        )
        .await;
        for (uid, from, to, dep, arr) in [
            ("TWMC1", "TWMD", "TWME", "10:00:00", "10:20:00"),
            ("TWMC2", "TWME", "TWMF", "10:30:00", "10:50:00"),
            ("TWMC3", "TWMF", "TWMG", "11:00:00", "11:20:00"),
        ] {
            seed_schedule(
                &pool,
                uid,
                &[
                    (0, from, "origin", None, Some(dep)),
                    (1, to, "terminate", Some(arr), None),
                ],
            )
            .await;
        }
        let plan = |query: &str| {
            let pool = pool.clone();
            let uri = format!(
                "/Trips/plan?origin=ZMD&waypoints=ZME,ZMF&destination=ZMG&date={}\
                 &departAfter=09:30&live=false{query}",
                live_date()
            );
            async move { get(test_router(test_app(pool)), uri).await }
        };

        let (status, capped) = plan("&results=options&maxChanges=1").await;
        assert_eq!(status, StatusCode::OK, "{capped:?}");
        for segment in capped["segments"].as_array().unwrap() {
            assert!(segment["itineraries"].as_array().unwrap().is_empty());
            assert_eq!(segment["cappedByMaxChanges"], true, "{capped:?}");
            assert_eq!(segment["noResultReason"]["constraint"], "maxChanges");
        }
        assert_eq!(capped["journeys"], serde_json::json!([]));

        let (_, two) = plan("&results=options&maxChanges=2").await;
        assert_eq!(two["journeys"][0]["changeCount"], 2, "{two:?}");
        assert_eq!(train_uids(&two, 2), ["TWMC3"], "{two:?}");

        // fastest: found regardless, flagged against the whole journey.
        let (_, fastest) = plan("&maxChanges=1").await;
        assert_eq!(fastest["journeys"][0]["changeCount"], 2, "{fastest:?}");
        assert_eq!(fastest["journeys"][0]["exceedsRecommendedChanges"], true);
        for segment in fastest["segments"].as_array().unwrap() {
            assert_eq!(segment["itineraries"][0]["exceedsRecommendedChanges"], true);
        }

        cleanup_live(&pool, &uids, "TWMNET").await;
    }

    /// The waypoint cap: exactly [`MAX_WAYPOINTS`] waypoints, all called at
    /// by one train, plan as one journey with no change; one more is a 400.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn the_maximum_number_of_waypoints_plans_as_one_journey() {
        let pool = connect().await;
        let letters: Vec<char> = ('A'..='Z').take(MAX_WAYPOINTS + 2).collect();
        let stations: Vec<(String, String)> = letters
            .iter()
            .map(|l| (format!("ZU{l}"), format!("TWXC{l}")))
            .collect();
        cleanup_live(&pool, &["TWXALL"], "TWXNET").await;
        let station_refs: Vec<(&str, &str)> = stations
            .iter()
            .map(|(crs, tiploc)| (crs.as_str(), tiploc.as_str()))
            .collect();
        seed_stations(&pool, "TWXNET", &station_refs).await;
        let times: Vec<String> = (0..stations.len())
            .map(|i| format!("{:02}:{:02}:00", 10 + i * 3 / 60, i * 3 % 60))
            .collect();
        let calls: Vec<SeedCall<'_>> = stations
            .iter()
            .enumerate()
            .map(|(i, (_, tiploc))| {
                let last = i == stations.len() - 1;
                (
                    i as i32,
                    tiploc.as_str(),
                    if i == 0 {
                        "origin"
                    } else if last {
                        "terminate"
                    } else {
                        "intermediate"
                    },
                    (i > 0).then(|| times[i].as_str()),
                    (!last).then(|| times[i].as_str()),
                )
            })
            .collect();
        seed_schedule(&pool, "TWXALL", &calls).await;

        let crs: Vec<&str> = stations.iter().map(|(crs, _)| crs.as_str()).collect();
        let uri = |waypoints: &[&str]| {
            format!(
                "/Trips/plan?origin={}&waypoints={}&destination={}&date={}&live=false",
                crs[0],
                waypoints.join(","),
                crs[crs.len() - 1],
                live_date()
            )
        };
        let waypoints = &crs[1..crs.len() - 1];
        assert_eq!(waypoints.len(), MAX_WAYPOINTS);
        let (status, body) = get(test_router(test_app(pool.clone())), uri(waypoints)).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segments = body["segments"].as_array().unwrap();
        assert_eq!(segments.len(), MAX_WAYPOINTS + 1);
        assert!(
            segments[1..]
                .iter()
                .all(|s| s["itineraries"][0]["continuesPreviousTrain"] == true),
            "{body:?}"
        );
        assert_eq!(body["journeys"][0]["changeCount"], 0, "{body:?}");

        let mut too_many: Vec<&str> = waypoints.to_vec();
        too_many.push("ZZZ");
        let (status, body) = get(test_router(test_app(pool.clone())), uri(&too_many)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(body.as_str().unwrap().contains("too many waypoints"));

        cleanup_live(&pool, &["TWXALL"], "TWXNET").await;
    }

    /// `?via=` end to end: TWVF1 runs ZVA -> ZVC through ZVP without calling
    /// (an untimed passing row); TWVN1 is faster and goes nowhere near it.
    /// ZVX is a known station no train touches.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        reason = "test code: scenario tests read top to bottom"
    )]
    async fn via_passes_through_without_calling_end_to_end() {
        let pool = connect().await;
        let uids = ["TWVF1", "TWVN1"];
        cleanup_live(&pool, &uids, "TWVNET").await;
        seed_stations(
            &pool,
            "TWVNET",
            &[
                ("ZVA", "TWVA"),
                ("ZVP", "TWVP"),
                ("ZVC", "TWVC"),
                ("ZVX", "TWVX"),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TWVF1",
            &[
                (0, "TWVA", "origin", None, Some("10:00:00")),
                (1, "TWVP", "intermediate", None, None),
                (2, "TWVC", "terminate", Some("11:00:00"), None),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TWVN1",
            &[
                (0, "TWVA", "origin", None, Some("10:05:00")),
                (1, "TWVC", "terminate", Some("10:50:00"), None),
            ],
        )
        .await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());
        let uri = |query: &str| {
            format!(
                "/Trips/plan?origin=ZVA&destination=ZVC&date={}{query}",
                live_date()
            )
        };

        for query in [
            "&via=zvp&departAfter=09:45",
            "&via=ZVP&departAfter=09:45&live=false",
            "&via=ZVP&arriveBy=11:30",
            "&via=ZVP&departAfter=09:45&results=options&maxChanges=0",
            "&via=ZVP&avoidStop=ZVP&departAfter=09:45",
        ] {
            let (status, body) = get(test_router(test_app(pool.clone())), uri(query)).await;
            assert_eq!(status, StatusCode::OK, "{query}: {body:?}");
            assert_eq!(body["via"], serde_json::json!(["ZVP"]), "{query}");
            assert_eq!(train_uids(&body, 0), ["TWVF1"], "{query}: {body:?}");
            assert_eq!(
                body["journeys"][0]["viaSatisfiedBy"],
                serde_json::json!([{
                    "crs": "ZVP", "matchedCrs": "ZVP", "segment": 0, "leg": 0, "how": "pass"
                }]),
                "{query}: {body:?}"
            );
            assert_eq!(body["stationGroups"], serde_json::json!({}), "{query}");
        }

        // An OR via (2026-10-07): ZVX (untouched) or ZVP; `|` encoded or not.
        for query in [
            "&via=zvx%7Czvp&departAfter=09:45",
            "&via=ZVX|ZVP&arriveBy=11:30",
        ] {
            let (status, body) = get(test_router(test_app(pool.clone())), uri(query)).await;
            assert_eq!(status, StatusCode::OK, "{query}: {body:?}");
            assert_eq!(body["via"], serde_json::json!(["ZVX|ZVP"]), "{query}");
            assert_eq!(train_uids(&body, 0), ["TWVF1"], "{query}: {body:?}");
            assert_eq!(
                body["journeys"][0]["viaSatisfiedBy"],
                serde_json::json!([{
                    "crs": "ZVX|ZVP", "matchedCrs": "ZVP", "segment": 0, "leg": 0, "how": "pass"
                }]),
                "{query}: {body:?}"
            );
        }

        // No via: the faster train, `via` empty, no `viaSatisfiedBy`.
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            uri("&departAfter=09:45"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(train_uids(&body, 0), ["TWVN1"]);
        assert_eq!(body["via"], serde_json::json!([]));
        assert!(body["journeys"][0].get("viaSatisfiedBy").is_none());

        // Unsatisfiable: still a 200, with the via named.
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            uri("&via=ZVX&departAfter=09:45"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        let segment = &body["segments"][0];
        assert_eq!(segment["itineraries"], serde_json::json!([]));
        assert_eq!(segment["noResultReason"]["constraint"], "via", "{body:?}");
        assert_eq!(
            segment["noResultReason"]["values"],
            serde_json::json!(["ZVX"])
        );
        assert_eq!(body["journeys"], serde_json::json!([]));

        // 400s, consistent with waypoints and the avoid lists.
        for (query, needle) in [
            ("&via=ZZQ", "not a recognised station code"),
            ("&via=ZVA", "the origin"),
            ("&via=ZVC", "the destination"),
            ("&via=ZVP&avoid=ZVP", "also in avoid"),
            ("&via=ZVP,ZVP", "twice in a row"),
            ("&via=ZVP,ZVX,ZVP,ZVX", "too many via"),
            // OR choices (2026-10-07).
            ("&via=ZZQ%7CZVP", "'ZZQ' is not a recognised station code"),
            ("&via=group:NOPE", "not a known station group"),
            ("&via=ZVX%7CZVP&avoid=ZVX,ZVP", "every station of 'ZVX|ZVP'"),
            ("&via=ZVP%7CZVX,zvx%7Czvp", "twice in a row"),
            ("&avoid=ZVX%7CZVP", "only via"),
        ] {
            let (status, body) = get(test_router(test_app(pool.clone())), uri(query)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body:?}");
            let message = body.as_str().unwrap_or_default();
            assert!(message.contains(needle), "{query}: {message}");
        }

        cleanup_live(&pool, &uids, "TWVNET").await;
    }

    /// OR-choice waypoints (2026-10-07) end to end: TWGW1 (10:00) calls at
    /// ZG1, TWGW2 (10:30) at ZG2, both on to ZGC. `waypoints=ZG1|ZG2` stops
    /// at whichever member the journey reaches, reported per journey; an
    /// end in a first or last choice satisfies it there; the 400s.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    #[expect(
        clippy::too_many_lines,
        reason = "one end-to-end scenario, read top to bottom"
    )]
    async fn group_waypoints_end_to_end() {
        let pool = connect().await;
        let uids = ["TWGW1", "TWGW2"];
        cleanup_live(&pool, &uids, "TWGNET").await;
        seed_stations(
            &pool,
            "TWGNET",
            &[
                ("ZGA", "TWGA"),
                ("ZG1", "TWG1"),
                ("ZG2", "TWG2"),
                ("ZGC", "TWGC"),
                ("ZGX", "TWGX"),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TWGW1",
            &[
                (0, "TWGA", "origin", None, Some("10:00:00")),
                (
                    1,
                    "TWG1",
                    "intermediate",
                    Some("10:20:00"),
                    Some("10:21:00"),
                ),
                (2, "TWGC", "terminate", Some("11:00:00"), None),
            ],
        )
        .await;
        seed_schedule(
            &pool,
            "TWGW2",
            &[
                (0, "TWGA", "origin", None, Some("10:30:00")),
                (
                    1,
                    "TWG2",
                    "intermediate",
                    Some("10:50:00"),
                    Some("10:51:00"),
                ),
                (2, "TWGC", "terminate", Some("11:30:00"), None),
            ],
        )
        .await;
        let _now = crate::routes::pin_london_now_for_tests(live_now());
        let uri = |query: &str| {
            format!(
                "/Trips/plan?origin=ZGA&destination=ZGC&date={}&live=false{query}",
                live_date()
            )
        };
        let stop = |crs: &str, matched: &str, segment: usize, how: &str| {
            serde_json::json!({
                "crs": crs, "matchedCrs": matched, "segment": segment, "how": how
            })
        };

        for (query, uid, member) in [
            ("&waypoints=zg1%7Czg2&departAfter=09:45", "TWGW1", "ZG1"),
            ("&waypoints=ZG1|ZG2&departAfter=10:10", "TWGW2", "ZG2"),
            ("&waypoints=ZG1|ZG2&arriveBy=11:45", "TWGW2", "ZG2"),
            ("&waypoints=ZG1|ZG2&arriveBy=11:10", "TWGW1", "ZG1"),
            (
                "&waypoints=ZG1|ZG2&departAfter=09:45&results=options",
                "TWGW1",
                "ZG1",
            ),
            // An avoided member is dropped, not a 400.
            (
                "&waypoints=ZG1|ZG2&avoid=ZG1&departAfter=09:45",
                "TWGW2",
                "ZG2",
            ),
            // With a via: only TWGW2 passes ZG2.
            (
                "&waypoints=ZG1|ZG2&via=ZG2&departAfter=09:45",
                "TWGW2",
                "ZG2",
            ),
        ] {
            let (status, body) = get(test_router(test_app(pool.clone())), uri(query)).await;
            assert_eq!(status, StatusCode::OK, "{query}: {body:?}");
            let segments = body["segments"].as_array().expect("segments");
            assert_eq!(segments.len(), 2, "{query}: {body:?}");
            assert_eq!(segments[0]["destinationCrs"], "ZG1|ZG2", "{query}");
            assert_eq!(segments[1]["originCrs"], "ZG1|ZG2", "{query}");
            assert_eq!(train_uids(&body, 0), [uid], "{query}: {body:?}");
            assert_eq!(train_uids(&body, 1), [uid], "{query}: {body:?}");
            assert_eq!(
                segments[1]["itineraries"][0]["continuesPreviousTrain"],
                true
            );
            assert_eq!(
                body["journeys"][0]["waypointSatisfiedBy"],
                serde_json::json!([stop("ZG1|ZG2", member, 0, "call")]),
                "{query}: {body:?}"
            );
            assert_eq!(body["journeys"][0]["changeCount"], 0, "{query}");
            assert_eq!(body["stationGroups"], serde_json::json!({}), "{query}");
        }

        // An end in a first (last) choice satisfies it there: no segment
        // for it, and the journey is the plain fastest one.
        for (query, satisfied) in [
            (
                "&waypoints=ZGX|ZGA&departAfter=09:45",
                stop("ZGX|ZGA", "ZGA", 0, "origin"),
            ),
            (
                "&waypoints=ZGC|ZGX&departAfter=09:45",
                stop("ZGC|ZGX", "ZGC", 0, "destination"),
            ),
        ] {
            let (status, body) = get(test_router(test_app(pool.clone())), uri(query)).await;
            assert_eq!(status, StatusCode::OK, "{query}: {body:?}");
            assert_eq!(
                body["segments"].as_array().map(Vec::len),
                Some(1),
                "{query}"
            );
            assert_eq!(train_uids(&body, 0), ["TWGW1"], "{query}: {body:?}");
            assert_eq!(
                body["journeys"][0]["waypointSatisfiedBy"],
                serde_json::json!([satisfied]),
                "{query}: {body:?}"
            );
        }
        // Then a real stop after the origin's: segments 0 and 1.
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            uri("&waypoints=ZGX|ZGA,ZG2&departAfter=09:45"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(train_uids(&body, 0), ["TWGW2"], "{body:?}");
        assert_eq!(
            body["journeys"][0]["waypointSatisfiedBy"],
            serde_json::json!([
                stop("ZGX|ZGA", "ZGA", 0, "origin"),
                stop("ZG2", "ZG2", 0, "call")
            ]),
            "{body:?}"
        );

        // Single-station waypoints: no `waypointSatisfiedBy`, as before.
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            uri("&waypoints=ZG2&departAfter=09:45"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert!(body["journeys"][0].get("waypointSatisfiedBy").is_none());

        // No member is called at: a 200 with every segment explained.
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            uri("&waypoints=ZGX|ZG1&avoid=ZG1&departAfter=09:45"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(body["journeys"], serde_json::json!([]));
        for segment in body["segments"].as_array().expect("segments") {
            assert!(
                segment["noResultReason"]["constraint"].is_string(),
                "{body:?}"
            );
        }

        for (query, needle) in [
            (
                "&waypoints=ZZQ%7CZG1",
                "'ZZQ' is not a recognised station code",
            ),
            ("&waypoints=group:NOPE", "not a known station group"),
            (
                "&waypoints=ZG1%7CZG2&avoid=ZG1&avoidStop=ZG2",
                "every station of 'ZG1|ZG2' is in an avoid list",
            ),
            (
                "&waypoints=ZG1%7CZG2,ZG2",
                "'ZG1|ZG2' and 'ZG2' are next to each other and share 'ZG2'",
            ),
            // Single stations: the messages from before groups.
            ("&waypoints=ZGA", "waypoint 'ZGA' is the origin"),
            ("&waypoints=ZG1,ZG1", "is the waypoint before it"),
            ("&waypoints=ZG1&avoid=ZG1", "avoid: 'ZG1' is a waypoint"),
        ] {
            let (status, body) = get(test_router(test_app(pool.clone())), uri(query)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body:?}");
            let message = body.as_str().unwrap_or_default();
            assert!(message.contains(needle), "{query}: {message}");
        }

        cleanup_live(&pool, &uids, "TWGNET").await;
    }

    /// The raised ceiling end to end: the only ZKA -> ZKZ route takes seven
    /// trains (six changes, every gap the 5-minute default change time).
    /// `maxChanges=5` finds nothing and blames the cap; `maxChanges=6` finds
    /// it. The options search-size guard is a 400 before any read.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn max_changes_five_and_six_end_to_end() {
        let pool = connect().await;
        let uids = ["TWK1", "TWK2", "TWK3", "TWK4", "TWK5", "TWK6", "TWK7"];
        cleanup_live(&pool, &uids, "TWKNET").await;
        seed_stations(&pool, "TWKNET", &[("ZKA", "TWKS0"), ("ZKZ", "TWKS7")]).await;
        for (hop, uid) in uids.iter().enumerate() {
            let departure = format!("{:02}:{:02}:00", 8 + (hop * 15) / 60, (hop * 15) % 60);
            let arrival = format!(
                "{:02}:{:02}:00",
                8 + (hop * 15 + 10) / 60,
                (hop * 15 + 10) % 60
            );
            let (from, to) = (format!("TWKS{hop}"), format!("TWKS{}", hop + 1));
            seed_schedule(
                &pool,
                uid,
                &[
                    (0, from.as_str(), "origin", None, Some(departure.as_str())),
                    (1, to.as_str(), "terminate", Some(arrival.as_str()), None),
                ],
            )
            .await;
        }
        let _now = crate::routes::pin_london_now_for_tests(live_now());
        let uri = |query: &str| {
            format!(
                "/Trips/plan?origin=ZKA&destination=ZKZ&date={}&results=options&live=false{query}",
                live_date()
            )
        };

        let (status, body) = get(test_router(test_app(pool.clone())), uri("&maxChanges=5")).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(body["maxChanges"], 5);
        let segment = &body["segments"][0];
        assert_eq!(segment["itineraries"], serde_json::json!([]), "{body:?}");
        assert_eq!(segment["cappedByMaxChanges"], true);
        assert_eq!(segment["noResultReason"]["constraint"], "maxChanges");

        let (status, body) = get(test_router(test_app(pool.clone())), uri("&maxChanges=6")).await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(body["maxChanges"], 6);
        let itineraries = body["segments"][0]["itineraries"]
            .as_array()
            .expect("itineraries");
        assert_eq!(itineraries.len(), 1, "{body:?}");
        assert_eq!(itineraries[0]["changeCount"], 6);
        assert_eq!(body["segments"][0]["cappedByMaxChanges"], false);
        assert_eq!(body["journeys"][0]["changeCount"], 6);

        // Arrive-by agrees.
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            uri("&maxChanges=6&arriveBy=12:00"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body:?}");
        assert_eq!(body["journeys"][0]["changeCount"], 6, "{body:?}");

        // The search-size guard: 4 waypoints and 3 vias at maxChanges=6 is
        // too large (5 * 7 * 8 = 280 > 252); fastest is not limited by it.
        let (status, body) = get(
            test_router(test_app(pool.clone())),
            uri("&maxChanges=6&waypoints=ZQA,ZQB,ZQC,ZQD&via=ZQE,ZQF,ZQG"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body:?}");
        assert!(
            body.as_str()
                .unwrap_or_default()
                .contains("too large a search"),
            "{body:?}"
        );

        cleanup_live(&pool, &uids, "TWKNET").await;
    }
}
