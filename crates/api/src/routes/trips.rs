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
use crate::data::{trip_plan_live, trip_planning, trip_planning_itinerary};

/// Hard cap on `?waypoints=`, enforced before any database read (2026-09-25
/// review, High 4b). `plan_via_waypoints` solves one INDEPENDENT pathfinding
/// search per consecutive pair of stops, so `n` waypoints means `n + 1` full
/// searches over the whole day's connections graph -- and nothing capped `n`.
/// `?waypoints=YRK,YRK,YRK,...` repeated a few hundred times is a legal query
/// string that turns one unauthenticated GET into hundreds of graph searches.
///
/// 8 is generous for a real itinerary (a ten-leg cross-country trip with
/// eight intermediate stops the traveller specifically asked to route via)
/// while keeping the worst case a small multiple of the single-leg cost, not
/// an unbounded one.
const MAX_WAYPOINTS: usize = 8;

/// Cap on each of `?avoid=`, `?avoidStop=` and `?avoidChange=`, checked
/// before any database read like [`MAX_WAYPOINTS`]. An avoided station adds
/// no search of its own (the restrictions are applied inside the one search
/// per segment), but `avoid` reads every train that runs through it, and an
/// empty segment is explained by re-running it without each list.
const MAX_AVOIDED: usize = 8;

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
static GRAPH_CACHE: LazyLock<
    std::sync::Arc<trip_planning::GraphCache<trip_planning::PlanningGraph>>,
> = LazyLock::new(|| {
    // Off in unit tests: the DB tests reseed the same date with different
    // rows and must each see their own.
    let default_dates = if cfg!(test) {
        0
    } else {
        DEFAULT_GRAPH_CACHE_DATES
    };
    let dates = env_or_default(GRAPH_CACHE_DATES_ENV, default_dates);
    let max_age = env_or_default(GRAPH_CACHE_MAX_AGE_ENV, DEFAULT_GRAPH_CACHE_MAX_AGE_SECS);
    std::sync::Arc::new(trip_planning::GraphCache::new(
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
    crate::app::Router::new().route("/Trips/plan", axum::routing::get(get_trip_plan))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TripPlanParams {
    origin: String,
    destination: String,
    /// Comma-separated ordered CRS codes, e.g. `?waypoints=YRK,NCL`. Absent
    /// or empty means no waypoints -- a direct origin->destination plan.
    #[serde(default)]
    waypoints: Option<String>,
    date: NaiveDate,
    #[serde(default)]
    depart_after: Option<NaiveTime>,
    /// `?arriveBy=HH:MM`: the latest acceptable arrival at the destination.
    /// Mutually exclusive with `departAfter` (a 400 when both are given).
    #[serde(default)]
    arrive_by: Option<NaiveTime>,
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
    #[serde(default = "default_results")]
    results: String,
    /// Optional interchange cap, `?maxChanges=0`..`=4` (inclusive). Absent
    /// or empty means [`trip_planning_itinerary::DEFAULT_MAX_CHANGES`] (2),
    /// exactly this route's behaviour before the parameter existed. Kept as
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
}

/// The live overlay's configuration, read once from the environment.
static LIVE_CONFIG: LazyLock<trip_plan_live::LiveConfig> =
    LazyLock::new(trip_plan_live::LiveConfig::from_env);

fn default_results() -> String {
    "fastest".to_string()
}

/// `GET /Trips/plan?origin=&destination=&date=[&waypoints=]
/// [&departAfter=|&arriveBy=][&avoid=][&avoidStop=][&avoidChange=]
/// [&results=fastest|options][&maxChanges=0..4][&live=true|false]`.
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
///   changes exists.
/// - `maxChanges`: integer `0..=`[`trip_planning_itinerary::MAX_CHANGES_LIMIT`]
///   (4); absent/empty => [`trip_planning_itinerary::DEFAULT_MAX_CHANGES`]
///   (2). Anything else (out of range, negative, non-integer) is a 400 naming
///   the allowed range, rejected before any database read. The effective
///   value is echoed back as the response's top-level `maxChanges`.
///
/// **Read this before adding work to this handler.** One request here reads
/// every `schedule_calling_points_full` row for the requested date (hundreds
/// of thousands), builds and sorts the whole day's connections graph in
/// memory, and then runs one graph search per leg. That is by far the most
/// expensive thing this unauthenticated API can be asked to do, so four
/// separate bounds apply, all of them load-bearing (2026-09-25 review, High
/// 4) and none of them a substitute for another:
///
/// 1. [`MAX_WAYPOINTS`], checked BEFORE any database read -- bounds how many
///    graph searches one request can ask for. Rejected requests cost a string
///    split, not a query.
/// 2. [`trip_planning_itinerary::MAX_CHANGES_LIMIT`], also checked before any
///    database read -- bounds how deep each `options`-mode search can go.
///    RAPTOR does one full sweep of the day's connections per round, and runs
///    `maxChanges + 2` rounds at most (fewer if a round improves nothing), so
///    the worst case per request is `(MAX_WAYPOINTS + 1) * (4 + 2)` = 54
///    sweeps at `maxChanges=4`, versus 36 at the default of 2 -- a bounded
///    1.5x on the search phase alone (the whole-day read and graph build,
///    which dominate memory, are unchanged, and `fastest` mode's single CSA
///    scan per segment doesn't depend on `maxChanges` at all). Per-search
///    memory grows the same bounded 1.5x (one arrival map per round), and
///    segments are solved one after another, never at once.
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
    if params.results != "fastest" && params.results != "options" {
        return Err((
            StatusCode::BAD_REQUEST,
            "results must be 'fastest' or 'options'".to_string(),
        ));
    }

    let waypoints = parse_waypoints(params.waypoints.as_deref())?;
    let max_changes = parse_max_changes(params.max_changes.as_deref())?;
    let live_requested = parse_live(params.live.as_deref())?;
    let time = parse_time_bound(params.depart_after, params.arrive_by)?;
    let origin = params.origin.trim().to_ascii_uppercase();
    let destination = params.destination.trim().to_ascii_uppercase();
    let avoid = trip_planning_itinerary::AvoidLists {
        avoid: parse_station_list("avoid", params.avoid.as_deref())?,
        avoid_stop: parse_station_list("avoidStop", params.avoid_stop.as_deref())?,
        avoid_change: parse_station_list("avoidChange", params.avoid_change.as_deref())?,
    };
    check_avoid_conflicts(&avoid, &origin, &waypoints, &destination)?;

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

    // TRIPS-1: the day's graph comes from the per-date cache, keyed on the
    // latest schedule publish; only a miss reads and builds it.
    let marker = trip_planning::latest_schedule_publish(&app.database)
        .await
        .map_err(internal_error("read the latest schedule publish"))?;
    let date = params.date;
    let pool = app.database.clone();
    let cached = GRAPH_CACHE
        .get_or_build(date, marker, move || async move {
            let Some(calling_points) =
                trip_planning::fetch_calling_points_for_date(&pool, date).await?
            else {
                return Ok(None);
            };
            let interchange = trip_planning::fetch_interchange_data(&pool).await?;
            // The sort over every calling point of the day, on the blocking
            // pool (bound 4 in this fn's doc comment).
            let (connections, passes) = tokio::task::spawn_blocking(move || {
                trip_planning::build_connections_with_passes(calling_points)
            })
            .await?;
            Ok(Some(trip_planning::PlanningGraph {
                connections,
                interchange,
                passes,
            }))
        })
        .await
        .map_err(internal_error("build the connections graph"))?;
    let Some((graph, outcome)) = cached else {
        return Err((
            StatusCode::NOT_FOUND,
            format!(
                "no CIF-derived schedule data has been published for {} yet",
                params.date
            ),
        ));
    };
    metrics::counter!(
        common::metrics::metric_name("api_trip_plan_graph_cache_total"),
        "result" => match outcome {
            trip_planning::CacheOutcome::Hit => "hit",
            trip_planning::CacheOutcome::Built => "miss",
        }
    )
    .increment(1);

    // The per-leg searches, on the blocking pool. The graph is shared, not
    // copied: an `Arc` into the cache. The permit is shared with every
    // blocking search (the live overlay may run several) and released only
    // when the last of them finishes -- see bound 3 in this fn's doc comment.
    let permit = Arc::new(permit);
    // The avoid lists' search restrictions, once per request (`avoid` walks
    // the day's connections for the trains running through a station).
    let restrictions = if avoid.is_empty() {
        None
    } else {
        let graph = graph.clone();
        let lists = avoid.clone();
        let permit = permit.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            trip_planning_itinerary::build_restrictions(
                &graph.connections,
                &graph.interchange,
                Some(&graph.passes),
                &lists,
            )
        })
        .await
        .map_err(|join_err| {
            tracing::error!(error = ?join_err, "trip planning task panicked or was cancelled");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "failed to plan trip".to_string(),
            )
        })?
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?
        .map(Arc::new)
    };
    let request = Arc::new(PlanRequest {
        date,
        origin,
        destination,
        waypoints,
        time,
        avoid,
        restrictions,
        results: params.results.clone(),
        max_changes,
    });
    let mut segments = run_plan(graph.clone(), request.clone(), None, permit.clone())
        .await?
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    let live_summary = if live_requested {
        let config = &*LIVE_CONFIG;
        let today = crate::routes::london_today();
        let outcome = if !config.enabled {
            Err("disabled")
        } else if date != today && Some(date) != today.pred_opt() {
            Err("outsideLiveWindow")
        } else {
            let now = crate::routes::london_now().with_timezone(&chrono::Utc);
            match plan_live(&app, &graph, &request, &permit, &segments, config, now).await {
                Ok((live_segments, summary)) => {
                    segments = live_segments;
                    Ok(summary)
                }
                Err(err) => {
                    tracing::warn!(error = ?err, "trip plan live overlay failed; serving the timetable plan");
                    Err("unavailable")
                }
            }
        };
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
        Some(match outcome {
            Ok(summary) => serde_json::json!({
                "applied": true,
                "reason": null,
                "replans": summary.replans,
                "trainsRead": summary.trains_read,
                "adjustedTrains": summary.adjusted_trains,
            }),
            Err(reason) => serde_json::json!({ "applied": false, "reason": reason }),
        })
    } else {
        metrics::counter!(
            common::metrics::metric_name("api_trip_plan_live_requests_total"),
            "outcome" => "not_requested"
        )
        .increment(1);
        None
    };
    drop(permit);

    crate::data::trip_leg_details::attach_leg_details(&app.database, date, &mut segments)
        .await
        .map_err(internal_error("attach trip leg details"))?;

    let mut body = serde_json::json!({
        "results": params.results,
        "maxChanges": max_changes,
        // Additive (2026-09-29): the arrive-by deadline (`null` for a
        // depart-after request) and the avoid lists as applied.
        "arriveBy": match request.time {
            trip_planning_itinerary::TimeBound::ArriveBy(minutes) => Some(segment_clock(minutes)),
            trip_planning_itinerary::TimeBound::DepartAfter(_) => None,
        },
        "avoid": request.avoid.avoid,
        "avoidStop": request.avoid.avoid_stop,
        "avoidChange": request.avoid.avoid_change,
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
    });
    if let Some(summary) = live_summary {
        body["live"] = summary;
    }
    Ok(Json(body))
}

/// One request's planning inputs, shared with every blocking search.
struct PlanRequest {
    date: NaiveDate,
    origin: String,
    destination: String,
    waypoints: Vec<String>,
    time: trip_planning_itinerary::TimeBound,
    avoid: trip_planning_itinerary::AvoidLists,
    /// `build_restrictions(avoid)`, `None` when no list is given.
    restrictions: Option<Arc<trip_planner::Restrictions>>,
    results: String,
    max_changes: u32,
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
                interchange: &graph.interchange,
                date: request.date,
                results: &request.results,
                max_changes: request.max_changes,
                overlay: overlay.as_deref(),
                restrictions: request.restrictions.as_deref(),
            },
            passes: Some(&graph.passes),
            avoid: &request.avoid,
            origin_crs: &request.origin,
            waypoints: &request.waypoints,
            destination_crs: &request.destination,
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
async fn plan_live(
    app: &App,
    graph: &Arc<trip_planning::PlanningGraph>,
    request: &Arc<PlanRequest>,
    permit: &PlanPermit,
    timetable: &[trip_planning_itinerary::SegmentResult],
    config: &trip_plan_live::LiveConfig,
    now: chrono::DateTime<chrono::Utc>,
) -> anyhow::Result<(Vec<trip_planning_itinerary::SegmentResult>, LiveSummary)> {
    use std::collections::{HashMap, HashSet};

    let date = request.date;
    let mut segments = timetable.to_vec();
    let mut lives: HashMap<String, trip_plan_live::TrainLive> = HashMap::new();
    let mut read: HashSet<String> = HashSet::new();
    let mut chains: HashMap<String, Vec<schedule_query::Connection>> = HashMap::new();
    let mut overlay = Arc::new(trip_planner::ConnectionOverlay::default());
    let mut withdrawn = 0usize;
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
        let (next, next_withdrawn) = trip_plan_live::build_overlay(&chains, &lives);
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
                interchange: &graph.interchange,
                chains: &chains,
                lives: &lives,
                overlaid: overlay.replaced_uids(),
            },
        );
        let invalidated = trip_plan_live::plan_invalidated(&probe, &graph.interchange)
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
        overlay = Arc::new(next);
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
            interchange: &graph.interchange,
            chains: &chains,
            lives: &lives,
            overlaid: overlay.replaced_uids(),
        },
    );
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
        },
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
fn parse_waypoints(raw: Option<&str>) -> Result<Vec<String>, (StatusCode, String)> {
    let waypoints: Vec<String> = raw
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_uppercase())
        .collect();

    if waypoints.len() > MAX_WAYPOINTS {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "too many waypoints: {} given, at most {MAX_WAYPOINTS} allowed",
                waypoints.len()
            ),
        ));
    }
    Ok(waypoints)
}

/// Splits, trims and uppercases one avoid list (`name` is its wire name,
/// for the message), capped at [`MAX_AVOIDED`] -- a 400 before any database
/// read, like [`parse_waypoints`].
fn parse_station_list(name: &str, raw: Option<&str>) -> Result<Vec<String>, (StatusCode, String)> {
    let mut codes: Vec<String> = Vec::new();
    for code in raw
        .unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let code = code.to_ascii_uppercase();
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    if codes.len() > MAX_AVOIDED {
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "too many {name} stations: {} given, at most {MAX_AVOIDED} allowed",
                codes.len()
            ),
        ));
    }
    Ok(codes)
}

/// An avoided station that is also the origin, the destination or a
/// waypoint makes the request contradictory: a 400 naming both, before any
/// database read.
fn check_avoid_conflicts(
    avoid: &trip_planning_itinerary::AvoidLists,
    origin: &str,
    waypoints: &[String],
    destination: &str,
) -> Result<(), (StatusCode, String)> {
    for (name, codes) in avoid.lists() {
        for code in codes {
            let role = if code == origin {
                "the origin"
            } else if code == destination {
                "the destination"
            } else if waypoints.contains(code) {
                "a waypoint"
            } else {
                continue;
            };
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{name}: '{code}' is {role}; a trip cannot avoid it"),
            ));
        }
    }
    Ok(())
}

/// `?departAfter=` / `?arriveBy=`: at most one. Neither means departing
/// after 00:00, as before.
fn parse_time_bound(
    depart_after: Option<NaiveTime>,
    arrive_by: Option<NaiveTime>,
) -> Result<trip_planning_itinerary::TimeBound, (StatusCode, String)> {
    use chrono::Timelike;
    let minutes = |time: NaiveTime| time.num_seconds_from_midnight() / 60;
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

/// Validates `?maxChanges=`: absent, empty or whitespace-only means the
/// default ([`trip_planning_itinerary::DEFAULT_MAX_CHANGES`], matching how an
/// empty `?waypoints=` means "none"); otherwise it must be an integer in
/// `0..=`[`trip_planning_itinerary::MAX_CHANGES_LIMIT`]. Like
/// [`parse_waypoints`], this is a pure function checked before the permit and
/// any database read, so a rejected request costs a string parse, not a
/// query -- and it's a 400 naming the allowed range, never a silent clamp: a
/// plan computed under a different cap than the one asked for answers a
/// different question.
fn parse_max_changes(raw: Option<&str>) -> Result<u32, (StatusCode, String)> {
    use trip_planning_itinerary::{DEFAULT_MAX_CHANGES, MAX_CHANGES_LIMIT};

    let raw = raw.unwrap_or("").trim();
    if raw.is_empty() {
        return Ok(DEFAULT_MAX_CHANGES);
    }
    match raw.parse::<u32>() {
        Ok(value) if value <= MAX_CHANGES_LIMIT => Ok(value),
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

fn internal_error(operation: &'static str) -> impl Fn(anyhow::Error) -> (StatusCode, String) {
    move |err| {
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
        assert!(waypoints.iter().all(|crs| crs == "YRK"));

        assert_eq!(
            parse_waypoints(None).expect("absent is fine"),
            Vec::<String>::new()
        );
        assert_eq!(
            parse_waypoints(Some(" ,, ")).expect("empty entries are dropped"),
            Vec::<String>::new()
        );
    }

    /// Omitted (or empty) `?maxChanges=` must mean exactly the pre-parameter
    /// cap of 2, so existing callers see no behaviour change.
    #[test]
    fn max_changes_defaults_to_two_when_omitted_or_empty() {
        assert_eq!(trip_planning_itinerary::DEFAULT_MAX_CHANGES, 2);
        assert_eq!(parse_max_changes(None), Ok(2));
        assert_eq!(parse_max_changes(Some("")), Ok(2));
        assert_eq!(parse_max_changes(Some("  ")), Ok(2));
    }

    #[test]
    fn max_changes_accepts_every_value_from_zero_to_the_limit() {
        assert_eq!(trip_planning_itinerary::MAX_CHANGES_LIMIT, 4);
        for value in 0..=trip_planning_itinerary::MAX_CHANGES_LIMIT {
            assert_eq!(parse_max_changes(Some(&value.to_string())), Ok(value));
        }
        assert_eq!(parse_max_changes(Some(" 3 ")), Ok(3));
    }

    #[test]
    fn max_changes_out_of_range_or_non_integer_is_a_400_naming_the_range() {
        for bad in [
            "5",
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
                message.contains("maxChanges") && message.contains("0 to 4"),
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
        let (status, message) = parse_time_bound(at(9), at(10)).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("mutually exclusive"), "{message}");
        assert_eq!(
            parse_time_bound(None, at(10)),
            Ok(trip_planning_itinerary::TimeBound::ArriveBy(630))
        );
        assert_eq!(
            parse_time_bound(at(9), None),
            Ok(trip_planning_itinerary::TimeBound::DepartAfter(570))
        );
        assert_eq!(
            parse_time_bound(None, None),
            Ok(trip_planning_itinerary::TimeBound::DepartAfter(0))
        );
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

    #[test]
    fn avoid_lists_are_normalised_capped_and_checked_against_the_stops() {
        assert_eq!(
            parse_station_list("avoid", Some(" cre, ,CRE,bhm ")),
            Ok(vec!["CRE".to_string(), "BHM".to_string()])
        );
        let raw = (0..=MAX_AVOIDED)
            .map(|i| format!("Z{i:02}"))
            .collect::<Vec<_>>()
            .join(",");
        let (status, message) = parse_station_list("avoidStop", Some(&raw)).unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(message.contains("avoidStop") && message.contains(&MAX_AVOIDED.to_string()));

        let lists = trip_planning_itinerary::AvoidLists {
            avoid_change: vec!["YRK".to_string()],
            ..Default::default()
        };
        let (status, message) =
            check_avoid_conflicts(&lists, "KGX", &["YRK".to_string()], "EDB").unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            message.contains("avoidChange") && message.contains("a waypoint"),
            "{message}"
        );
        assert!(check_avoid_conflicts(&lists, "KGX", &[], "EDB").is_ok());
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
mod db_tests {
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
            redis_url: "redis://127.0.0.1:0".to_string(),
            redis_password: None,
            internal_oauth_issuer_url: "https://example.invalid".to_string(),
            internal_oauth_client_id: "test-internal-oauth-client".to_string(),
            internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
            internal_oauth_group_stations: "svc-poller-stations".to_string(),
            internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
            internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
            internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
            internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
            internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
            internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
            internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
            internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
            internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
            internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
            internal_oauth_group_corpus: "svc-corpus-ingest".to_string(),
            internal_oauth_group_mcp: "srv-ds-mcp".to_string(),
            chatbot_access_group: "distant-signal-chatbot-users".to_string(),
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
            past_travel_retention_days: 548,
            stale_push_subscription_days: 365,
            inactive_account_retention_days: 0,
        };

        std::sync::Arc::new(AppState {
            line_matcher: common::matcher::LineMatcher::new(&config.lines),
            config,
            database: pool,
            // `Client::open` only parses the URL, never opens a socket --
            // this route never touches Redis at all.
            redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
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
            internal_oauth_routes: Vec::new(),
            schedule_crs_line_index: std::collections::HashMap::new(),
        })
    }

    /// The real `trips::router()`, mounted unprefixed exactly as `main.rs`
    /// does, turned into a `tower::Service` a test can drive with
    /// `.oneshot(..)`.
    fn test_router(app: App) -> axum::Router {
        crate::app::Router::new()
            .merge(super::router())
            .with_state(app)
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn options_mode_excludes_results_over_the_cap_but_flags_when_capped() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
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
            format!("/Trips/plan?origin=ZZC&destination=ZZD&date={date}&results=options"),
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
                itinerary["changeCount"].as_u64().unwrap()
                    <= u64::from(trip_planning_itinerary::DEFAULT_MAX_CHANGES),
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

    /// `?maxChanges=` validation is enforced by the real route BEFORE the
    /// "no schedule data published for this date" read -- proven, like
    /// `a_waypoint_flood_is_rejected_before_the_date_is_even_looked_up`, by
    /// using a date nothing is published for (which would otherwise 404).
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn an_invalid_max_changes_is_a_clear_400_before_the_date_is_looked_up() {
        let pool = connect().await;
        for bad in ["5", "-1", "abc", "2.5", "4294967296"] {
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
                message.contains("maxChanges") && message.contains("0 to 4"),
                "the error must name the parameter and its range: {message}"
            );
        }
    }

    /// The end-to-end proof for `?maxChanges=`: a synthetic network whose
    /// ONLY route needs exactly 3 changes (4 legs,
    /// ORIGIN -> P -> Q -> R -> DEST). At the default cap `options` mode
    /// returns nothing but flags `cappedByMaxChanges`, and an explicit
    /// `maxChanges=2` is byte-for-byte the same response as omitting it; at
    /// `maxChanges=3` the real 3-change itinerary comes back, uncapped.
    /// `fastest` mode ignores the cap as a limit (the over-cap route is still
    /// returned) and only uses it as the `exceedsRecommendedChanges` threshold.
    /// Synthetic CRS codes/TIPLOCs for the same isolation reason as
    /// `a_real_seeded_connection_is_found_end_to_end`.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                routes::trips -- --ignored --test-threads=1`"]
    async fn max_changes_lifts_the_cap_only_when_asked() {
        let pool = connect().await;
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
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

        // options, omitted => the pre-parameter default of 2.
        let (status, omitted) = plan("&results=options").await;
        assert_eq!(status, StatusCode::OK, "{omitted:?}");
        assert_eq!(omitted["maxChanges"], 2, "{omitted:?}");
        let segment = &omitted["segments"][0];
        assert_eq!(
            segment["itineraries"]
                .as_array()
                .expect("itineraries array")
                .len(),
            0,
            "the only route needs 3 changes, over the default cap: {omitted:?}"
        );
        assert_eq!(segment["cappedByMaxChanges"], true, "{omitted:?}");

        // options, explicit 2 => identical to omitted.
        let (status, explicit_two) = plan("&results=options&maxChanges=2").await;
        assert_eq!(status, StatusCode::OK, "{explicit_two:?}");
        assert_eq!(explicit_two, omitted);

        // options, 3 => the real 3-change itinerary, not capped.
        let (status, three) = plan("&results=options&maxChanges=3").await;
        assert_eq!(status, StatusCode::OK, "{three:?}");
        assert_eq!(three["maxChanges"], 3, "{three:?}");
        let segment = &three["segments"][0];
        let itineraries = segment["itineraries"]
            .as_array()
            .expect("itineraries array");
        assert_eq!(itineraries.len(), 1, "{three:?}");
        assert_eq!(itineraries[0]["changeCount"], 3, "{three:?}");
        let uids: Vec<&str> = itineraries[0]["legs"]
            .as_array()
            .expect("legs array")
            .iter()
            .filter_map(|leg| leg["trainUid"].as_str())
            .collect();
        assert_eq!(
            uids,
            ["TESTPLANMXA", "TESTPLANMXB", "TESTPLANMXC", "TESTPLANMXD"],
            "{three:?}"
        );
        assert_eq!(segment["cappedByMaxChanges"], false, "{three:?}");

        // fastest: the same route either way, flagged against the cap in effect.
        let (status, fastest_default) = plan("").await;
        assert_eq!(status, StatusCode::OK, "{fastest_default:?}");
        let itinerary = &fastest_default["segments"][0]["itineraries"][0];
        assert_eq!(itinerary["changeCount"], 3, "{fastest_default:?}");
        assert_eq!(itinerary["exceedsRecommendedChanges"], true);
        assert_eq!(fastest_default["segments"][0]["cappedByMaxChanges"], false);

        let (status, fastest_three) = plan("&maxChanges=3").await;
        assert_eq!(status, StatusCode::OK, "{fastest_three:?}");
        let itinerary = &fastest_three["segments"][0]["itineraries"][0];
        assert_eq!(itinerary["changeCount"], 3, "{fastest_three:?}");
        assert_eq!(itinerary["exceedsRecommendedChanges"], false);

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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 26).unwrap();
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

    fn live_date() -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap()
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

        let (status, live) = get(test_router(test_app(pool.clone())), uri("")).await;
        assert_eq!(status, StatusCode::OK, "{live:?}");
        assert_eq!(live["live"]["applied"], true, "{live:?}");
        assert_eq!(live["live"]["replans"], 1, "{live:?}");
        assert_eq!(live["live"]["adjustedTrains"], 1, "{live:?}");
        assert_eq!(train_uids(&live, 0), ["TLVCANS"], "{live:?}");
        let itinerary = &live["segments"][0]["itineraries"][0];
        assert_eq!(itinerary["liveFeasible"], true);
        // The replacement has no live record: an explicit null.
        let leg = itinerary["legs"][0].as_object().unwrap();
        assert!(leg.contains_key("live") && leg["live"].is_null(), "{leg:?}");
        assert_eq!(leg["scheduledDeparture"], "10:05:00");

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
        // change onto the 10:40 -> re-planned onto the 11:00.
        set_delay(15).await;
        let (status, fresh) = get(test_router(test_app(pool.clone())), uri).await;
        assert_eq!(status, StatusCode::OK, "{fresh:?}");
        assert_eq!(fresh["live"]["replans"], 1, "{fresh:?}");
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
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
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
             VALUES ($1, 'ZYB', '08:00:00', 'TESTPLAT1', 'ZYA', 'SW', '1S00') ON CONFLICT DO NOTHING",
        )
        .bind(date)
        .execute(&pool)
        .await
        .expect("seed schedule_destination_departures");
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
        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TESTPLAT-ZY%'")
            .execute(&pool)
            .await
            .ok();
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
}
