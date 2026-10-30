//! A `TfL` Tube, DLR or tram stop as a `/Trips/plan` origin or destination
//! (DS-135; docs/superpowers/specs/2026-10-23-trips-plan-tfl-stop-ends-design.md).
//!
//! A stop with no National Rail station of its own (Uxbridge,
//! `940GZZLUUXB`) is planned to or from nearby National Rail stations, with
//! a Tube (or DLR, tram or walk) leg for the last part. This module finds
//! those stations, the access candidates, and estimates each one's minutes;
//! `trip_planner::access` turns them into one-way fixed links from (or to)
//! a virtual stop on a per-request copy of the interchange data, so the
//! search itself is the ordinary multi-source, multi-target one; and
//! [`AccessEnds::finish_legs`] dresses the access legs it returns.
//!
//! **Candidates**, each station once, quickest first:
//!
//! 1. **Hub**: the stop's own National Rail station: its `tfl_stop_crs`
//!    crosswalk, or a [`tfl_stops::CRS_OVERRIDES`] entry naming it. A walk
//!    of [`HUB_WALK_MINUTES`]; a hub station is never anything else.
//! 2. **Line** and **walk**, for a hub stop too (DS-167: Old Street's own
//!    station has few trains, its Northern line ride to Euston many): the
//!    hub stations (rule 1 there) of the nearest
//!    [`LINE_CANDIDATES_PER_LINE`] such stops along each of the stop's own
//!    lines (and its siblings', see below), by riding it, within
//!    [`RIDE_MAX_MINUTES`]; plus every station
//!    within [`WALK_MAX_METRES`] in a straight line, on foot. A station
//!    reached two ways keeps the quicker. At most [`MAX_CANDIDATES`] in
//!    all, the hub's included.
//! 3. **Nearest**, only when rules 1 and 2 find none: the
//!    [`NEAREST_COUNT`] nearest stations within [`NEAREST_MAX_METRES`], on
//!    foot.
//! 4. **Station ride**: when the plan's other end is a station with a stop
//!    of its own (rule 1 there) on one of the stop's lines, that station by
//!    the ride between the two stops ([`station_ride`]), whatever the cuts
//!    of rule 2: Euston to Old Street is the Northern line ride.
//!
//! **Estimates.** DS has no `TfL` timetables, so each time is a conservative
//! model, documented here and echoed in the response:
//!
//! - A ride is [`RIDE_ENTRY_MINUTES`] (walking in and the average wait)
//!   plus the distance along the line at the line's speed ([`line_mode`]:
//!   Tube 30 km/h, DLR 25, tram 18; `TfL` quotes about 33 km/h for the
//!   Underground, stops included) plus [`RIDE_EXIT_MINUTES`] (from the
//!   platform to the National Rail station), as a curated link's minutes
//!   include the walks at both ends (`reference-data/
//!   supplementary-fixed-links.csv`). The planner still adds the station's
//!   minimum change time before the train, as it does after any link.
//! - The distance along a line follows the line's routes (DS-151,
//!   `tfl_line_routes`: each route end to end, its stops in running order,
//!   [`LineRoutes`]): the sum of the straight lines between consecutive
//!   stops, and the stops ridden past. One route with both stops, in that
//!   order, is a ride with no change. When a route from the start and a
//!   route to the end share a stop, the ride may change trains there (the
//!   junction, the shared stop that makes the ride shortest:
//!   Harrow-on-the-Hill from Uxbridge to Rickmansworth), and costs
//!   [`BRANCH_CHANGE_MINUTES`] plus [`BRANCH_CHANGE_WAIT_MINUTES`] more: it
//!   is taken where no one train goes, or where its minutes, the change's
//!   included, are fewer than the one train's (DS-191: the Circle's
//!   trains from Paddington run round by Victoria to Liverpool Street,
//!   twice as far as by Edgware Road, where they change); a tie keeps the
//!   one train. A ride needing two changes on one line is not offered. A
//!   line with no routes, or a stop no route of its line has, falls back
//!   to a minimum spanning tree of the line's stops by straight-line
//!   distance ([`line_tree`]), as before DS-151, with the loops the tree
//!   broke closed again ([`line_graph`]: the Northern's Angel to Old
//!   Street, so Euston to Old Street is not measured round by Charing Cross
//!   and Bank), measured by the shortest way through it: close to the
//!   track, but blind to changes between branches, and short where the
//!   tree itself links two branches no train joins (Nine Elms to
//!   Stockwell). Such a ride is counted in [`ROUTES_MISSING_METRIC`], with
//!   a warning (throttled) when a line has no routes at all:
//!   `tfl_line_routes` is empty until poller-tfl's routes job runs. Rides
//!   stay on one line; no change between lines is modelled.
//! - A stop's station may have other stop areas (DS-191,
//!   [`with_siblings`]: `TfL` hub stops within [`SIBLING_MAX_METRES`]):
//!   Paddington's Bakerloo, Circle and District stop and its Hammersmith &
//!   City stop. A ride from (or to) a stop may start (or end) at one of
//!   them instead, its minutes including the walk between the two
//!   ([`walk_minutes`]); the candidate or leg names it (`boardStop`,
//!   `alightStop`).
//! - A walk is the straight-line distance times [`WALK_DETOUR`], at
//!   4.5 km/h ([`WALK_METRES_PER_MINUTE`]).
//! - Rides run in [`SERVICE_WINDOWS`] only (about 05:30 to 00:30; the Night
//!   Tube is not modelled); walks at any time.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::Result;
use common::tfl_stops::TflStopLine;
use schedule_query::{ChangeTime, InterchangeData, minimum_change_time};
use serde::Serialize;
use sqlx::PgPool;
use trip_planner::{AccessEnd, AccessLink};

use crate::data::tfl_stops::{self, TflStopSummary};
use crate::data::trip_planning_itinerary::{PlannedItinerary, PlannedLeg, SegmentResult};

/// 4.5 km/h.
pub const WALK_METRES_PER_MINUTE: f64 = 75.0;
/// Streets are not straight lines.
pub const WALK_DETOUR: f64 = 1.25;
/// How far a walk candidate may be, in a straight line (rule 2).
pub const WALK_MAX_METRES: f64 = 1_000.0;
/// How far the fallback looks (rule 3).
pub const NEAREST_MAX_METRES: f64 = 3_000.0;
pub const NEAREST_COUNT: usize = 3;
/// From a Tube stop to the National Rail station at the same hub.
pub const HUB_WALK_MINUTES: i32 = 5;
/// Walking in to the platform, and the average wait for a train.
pub const RIDE_ENTRY_MINUTES: i32 = 5;
/// From the platform at the far end to the National Rail station.
pub const RIDE_EXIT_MINUTES: i32 = 3;
/// A longer ride is not offered.
pub const RIDE_MAX_MINUTES: i32 = 90;
/// A ride straight between two stops may be longer: there is no other way
/// to offer it.
pub const DIRECT_RIDE_MAX_MINUTES: i32 = 150;
/// How many hub stops along each line (the nearest) give candidates.
pub const LINE_CANDIDATES_PER_LINE: usize = 5;
/// At most this many candidate stations in all, quickest first.
pub const MAX_CANDIDATES: usize = 16;
/// A ride that changes between two branches of its line (DS-151): the
/// minimum change time at the junction (a walk along or across the
/// platform, as `TfL`'s own planner allows for a cross-platform change).
pub const BRANCH_CHANGE_MINUTES: i32 = 5;
/// ... and the extra wait there: a branch runs less often than the trunk
/// (the Metropolitan's Amersham branch every 15 minutes or so, its trunk
/// far more often), so the second train is waited for again. Half a
/// branch's typical 10-minute headway.
pub const BRANCH_CHANGE_WAIT_MINUTES: i32 = 5;
/// The no-routes fallback ([`line_graph`]) links two ends of its spanning
/// tree when they are at most this far apart in a straight line (Angel
/// to Old Street is 1.45 km, King's Cross to Farringdon 1.73) ...
pub const LOOP_LINK_MAX_METRES: f64 = 1_800.0;
/// ... and the way between them through the tree is more than this many
/// times as far.
pub const LOOP_LINK_DETOUR: f64 = 2.0;
/// Two stop areas of one `TfL` hub at most this far apart in a straight
/// line are one station ([`with_siblings`]). The farthest such pair `TfL`
/// has is Paddington's two (about 255 m: the Bakerloo, Circle and District
/// stop and the Hammersmith & City one); Canary Wharf's DLR and Jubilee
/// stops are about 240 m apart, Hammersmith's two about 160. 400 m leaves
/// room for those and keeps out a hub that groups stops a real walk apart:
/// 500 m on foot, 7 minutes, is already a long change inside a station.
pub const SIBLING_MAX_METRES: f64 = 400.0;
/// When a ride can be started (`HHMM`, inclusive), every day.
pub const SERVICE_WINDOWS: [(&str, &str); 2] = [("0530", "2359"), ("0000", "0030")];

const EARTH_RADIUS_METRES: f64 = 6_371_000.0;

/// Whether `code` is a `TfL` stop id the planner takes as an end: a `NaPTAN`
/// metro or tram stop area (`940G...`), upper case, as `tfl_stops` keys
/// them. A CRS (three letters) or a `tiploc:` code never is.
pub fn is_tfl_stop_id(code: &str) -> bool {
    let code = code.trim();
    (8..=20).contains(&code.len())
        && code.starts_with("940G")
        && code
            .bytes()
            .all(|b| b.is_ascii_digit() || b.is_ascii_uppercase())
}

/// The fixed-link mode and speed (km/h) of a ride on `line`.
pub fn line_mode(line: &TflStopLine) -> (&'static str, f64) {
    let id = line.id.to_ascii_lowercase();
    if id == "dlr" {
        ("METRO", 25.0)
    } else if id.contains("tram") {
        ("METRO", 18.0)
    } else {
        ("TUBE", 30.0)
    }
}

/// One `tfl_stops` row with its hub stations.
#[derive(Debug, Clone)]
pub struct StopRecord {
    pub summary: TflStopSummary,
    /// `(latitude, longitude)`.
    pub position: Option<(f64, f64)>,
    /// Its hub's National Rail stations (rule 1).
    pub rail_crs: Vec<String>,
    /// `TfL`'s interchange hub (`HUBPAD`), which its stop areas share: see
    /// [`with_siblings`].
    pub hub: Option<String>,
}

/// How a candidate was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessBasis {
    /// The stop's own hub station.
    Hub,
    /// A ride on one of the stop's lines.
    Line,
    /// A short walk.
    Walk,
    /// The fallback: one of the nearest stations, on foot.
    Nearest,
}

/// One National Rail station a stop can be left for (or reached from).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessCandidate {
    pub crs: String,
    /// `TUBE`, `METRO` or `WALK`.
    pub mode: String,
    /// The estimate (see the module doc).
    pub minutes: i32,
    pub basis: AccessBasis,
    /// The line ridden (`Metropolitan`), for a ride.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// The stop at the station's end of a ride (`Harrow-on-the-Hill`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station_stop: Option<TflStopSummary>,
    /// Along the line for a ride, in a straight line for a walk.
    pub distance_metres: u32,
    /// Stops ridden to the far end, for a ride measured along the line's
    /// routes (absent for a spanning-tree estimate or a walk).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_count: Option<u32>,
    /// Where a ride changes trains between two branches of its line
    /// (`Harrow-on-the-Hill`); its `minutes` include the change.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_at: Option<TflStopSummary>,
    /// Where the ride is boarded, when that is not the plan's own stop but
    /// another stop area of its station ([`with_siblings`]: Paddington
    /// (H&C Line) for Paddington); `minutes` include the walk to it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_stop: Option<TflStopSummary>,
    /// Where the ride is left, when that is not the plan's own stop but
    /// another stop area of its station; `minutes` include the walk from it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alight_stop: Option<TflStopSummary>,
}

impl AccessCandidate {
    fn link(&self) -> AccessLink {
        AccessLink {
            crs: self.crs.clone(),
            mode: self.mode.clone(),
            minutes: self.minutes,
            windows: if self.mode == "WALK" {
                Vec::new()
            } else {
                SERVICE_WINDOWS
                    .iter()
                    .map(|(from, to)| ((*from).to_owned(), (*to).to_owned()))
                    .collect()
            },
        }
    }
}

/// One end of a plan that is a `TfL` stop, and its candidates.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopAccess {
    pub stop: TflStopSummary,
    pub candidates: Vec<AccessCandidate>,
}

/// Which end of the journey an access leg is at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccessLegEnd {
    Origin,
    Destination,
    /// A ride straight from the origin stop to the destination stop.
    Both,
}

/// On a `/Trips/plan` transfer leg that is an access or egress leg: which
/// end, how it was found, and how far it goes. Its `minutes` are an
/// estimate.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessLeg {
    pub end: AccessLegEnd,
    pub basis: AccessBasis,
    pub distance_metres: u32,
    /// See [`AccessCandidate::change_at`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_at: Option<TflStopSummary>,
    /// See [`AccessCandidate::board_stop`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board_stop: Option<TflStopSummary>,
    /// See [`AccessCandidate::alight_stop`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alight_stop: Option<TflStopSummary>,
}

fn metres_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1, lat2, lon2) = (
        a.0.to_radians(),
        a.1.to_radians(),
        b.0.to_radians(),
        b.1.to_radians(),
    );
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    EARTH_RADIUS_METRES * 2.0 * h.sqrt().asin()
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "minutes here are small and positive"
)]
fn whole(value: f64) -> i32 {
    value.ceil().max(1.0) as i32
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "distances here are at most a few hundred kilometres"
)]
fn metres(value: f64) -> u32 {
    value.round() as u32
}

fn walk_minutes(straight_metres: f64) -> i32 {
    whole(straight_metres * WALK_DETOUR / WALK_METRES_PER_MINUTE)
}

/// The edges of a minimum spanning tree over `members` (indices into
/// `stops`, each with a position) by straight-line distance: Prim's, which
/// is quadratic, over a line's few dozen stops. Each edge as
/// `(a, b, metres)`.
fn line_tree(stops: &[StopRecord], members: &[usize]) -> Vec<(usize, usize, f64)> {
    let mut edges = Vec::new();
    let Some((&first, rest)) = members.split_first() else {
        return edges;
    };
    let position = |i: usize| stops[i].position.unwrap_or_default();
    let mut outside: Vec<usize> = rest.to_vec();
    // For each stop outside, its nearest stop inside so far.
    let mut nearest: Vec<(usize, f64)> = outside
        .iter()
        .map(|&o| (first, metres_between(position(first), position(o))))
        .collect();
    while !outside.is_empty() {
        let pick = nearest
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map_or(0, |(index, _)| index);
        let added = outside.swap_remove(pick);
        let (from, distance) = nearest.swap_remove(pick);
        edges.push((from, added, distance));
        for (index, &o) in outside.iter().enumerate() {
            let d = metres_between(position(added), position(o));
            if d < nearest[index].1 {
                nearest[index] = (added, d);
            }
        }
    }
    edges
}

/// A line's stops as a graph: each stop's neighbours, with the straight
/// line to each.
type LineGraph = BTreeMap<usize, Vec<(usize, f64)>>;

/// An entry of [`shortest`]'s queue: nearest first, then the lower index.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Queued {
    metres: f64,
    stop: usize,
}

impl Eq for Queued {}

impl Ord for Queued {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reversed: `BinaryHeap` pops the greatest.
        other
            .metres
            .total_cmp(&self.metres)
            .then(other.stop.cmp(&self.stop))
    }
}

impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Dijkstra from `from` over `graph`: each stop reached, by index, with
/// its distance. Stops early once `until` is settled.
fn shortest(graph: &LineGraph, from: usize, until: Option<usize>) -> BTreeMap<usize, f64> {
    let mut settled: BTreeMap<usize, f64> = BTreeMap::new();
    let mut queue = std::collections::BinaryHeap::from([Queued {
        metres: 0.0,
        stop: from,
    }]);
    while let Some(Queued { metres, stop }) = queue.pop() {
        if settled.contains_key(&stop) {
            continue;
        }
        settled.insert(stop, metres);
        if until == Some(stop) {
            break;
        }
        for &(next, d) in graph.get(&stop).map(Vec::as_slice).unwrap_or_default() {
            if !settled.contains_key(&next) {
                queue.push(Queued {
                    metres: metres + d,
                    stop: next,
                });
            }
        }
    }
    settled
}

/// The fallback's graph of a line with no routes (see the module doc):
/// [`line_tree`], plus the links that close the loops the tree broke. A
/// tree has no loops, so it cuts each of the line's loops at a link, and
/// leaves the stops either side of the cut as two ends (stops with one
/// neighbour): the Northern's Angel and Old Street, with Euston to Old
/// Street the long way round by Charing Cross and Bank. So, shortest first,
/// two ends at most [`LOOP_LINK_MAX_METRES`] apart in a straight line,
/// whose way through the graph so far is over [`LOOP_LINK_DETOUR`] times
/// that, are linked straight. Only ends: two stops along the tree are
/// close in a straight line where a line bends back on itself or two
/// branches fork (Farringdon and Blackfriars on the Circle, Chalk Farm and
/// Kentish Town either side of Camden Town), and a link there would be a
/// ride no train makes. A linked end is no longer one, so each loop closes
/// once. A line's own ends (Morden, Edgware) are far apart, or near by
/// way of the tree.
fn line_graph(stops: &[StopRecord], members: &[usize]) -> LineGraph {
    let mut graph: LineGraph = members.iter().map(|&m| (m, Vec::new())).collect();
    let link = |graph: &mut LineGraph, a: usize, b: usize, d: f64| {
        graph.entry(a).or_default().push((b, d));
        graph.entry(b).or_default().push((a, d));
    };
    for (a, b, d) in line_tree(stops, members) {
        link(&mut graph, a, b, d);
    }
    let end = |graph: &LineGraph, stop: usize| graph.get(&stop).is_some_and(|n| n.len() == 1);
    let ends: Vec<usize> = members
        .iter()
        .copied()
        .filter(|&m| end(&graph, m))
        .collect();
    let position = |i: usize| stops[i].position.unwrap_or_default();
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (k, &a) in ends.iter().enumerate() {
        for &b in &ends[k + 1..] {
            let d = metres_between(position(a), position(b));
            if d <= LOOP_LINK_MAX_METRES {
                pairs.push((d, a.min(b), a.max(b)));
            }
        }
    }
    pairs.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
    for (d, a, b) in pairs {
        if !(end(&graph, a) && end(&graph, b)) {
            continue;
        }
        let broken = shortest(&graph, a, Some(b))
            .get(&b)
            .is_none_or(|&along| along > LOOP_LINK_DETOUR * d);
        if broken {
            link(&mut graph, a, b, d);
        }
    }
    graph
}

/// Every stop reachable from `stops[from]` along `line`'s fallback graph
/// ([`line_graph`]), with its distance along it, by index.
fn along_line(stops: &[StopRecord], from: usize, line: &TflStopLine) -> Vec<(usize, f64)> {
    let members: Vec<usize> = stops
        .iter()
        .enumerate()
        .filter(|(_, stop)| {
            stop.position.is_some() && stop.summary.lines.iter().any(|l| l.id == line.id)
        })
        .map(|(index, _)| index)
        .collect();
    shortest(&line_graph(stops, &members), from, None)
        .into_iter()
        .collect()
}

/// One of a line's routes as indices into the stops, in running order,
/// with each stop's distance along it from the first.
#[derive(Debug, Clone, PartialEq)]
struct Route {
    stops: Vec<usize>,
    along: Vec<f64>,
}

impl Route {
    /// The route through `members` (each with a position), at least two.
    fn new(stops: &[StopRecord], members: Vec<usize>) -> Option<Self> {
        if members.len() < 2 {
            return None;
        }
        let mut along = Vec::with_capacity(members.len());
        let mut total = 0.0;
        let mut previous: Option<(f64, f64)> = None;
        for &index in &members {
            let at = stops[index].position?;
            if let Some(before) = previous {
                total += metres_between(before, at);
            }
            along.push(total);
            previous = Some(at);
        }
        Some(Self {
            stops: members,
            along,
        })
    }

    /// The same track run the other way: for rides into a stop.
    fn reversed(&self) -> Self {
        let total = self.along.last().copied().unwrap_or_default();
        Self {
            stops: self.stops.iter().rev().copied().collect(),
            along: self.along.iter().rev().map(|d| total - d).collect(),
        }
    }

    /// Each place `stop` is on the route (the Circle passes some twice).
    fn positions(&self, stop: usize) -> impl Iterator<Item = usize> + '_ {
        self.stops
            .iter()
            .enumerate()
            .filter(move |(_, s)| **s == stop)
            .map(|(i, _)| i)
    }
}

/// Every line's routes (`tfl_line_routes`, DS-151), by line id, over the
/// stops of one request.
#[derive(Debug, Clone, Default)]
pub struct LineRoutes {
    by_line: HashMap<String, Vec<Route>>,
}

impl LineRoutes {
    /// `rows` are `(line id, stop ids in running order)`. A stop id not in
    /// `stops`, or without a position, is passed over: the route runs
    /// straight from the stop before it to the one after.
    pub fn new(stops: &[StopRecord], rows: &[(String, Vec<String>)]) -> Self {
        let index: HashMap<&str, usize> = stops
            .iter()
            .enumerate()
            .filter(|(_, stop)| stop.position.is_some())
            .map(|(i, stop)| (stop.summary.id.as_str(), i))
            .collect();
        let mut by_line: HashMap<String, Vec<Route>> = HashMap::new();
        for (line, ids) in rows {
            let members: Vec<usize> = ids
                .iter()
                .filter_map(|id| index.get(id.as_str()).copied())
                .collect();
            if let Some(route) = Route::new(stops, members) {
                by_line.entry(line.clone()).or_default().push(route);
            }
        }
        Self { by_line }
    }
}

/// Which way the rides of [`rides`] run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Toward {
    /// Out of the stop (a plan's origin).
    Out,
    /// Into the stop (a plan's destination).
    In,
}

/// One ride along a line.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Ride {
    metres: f64,
    /// Stops ridden, along routes; `None` on the spanning tree.
    stops: Option<usize>,
    /// The junction a branch change is made at.
    change_at: Option<usize>,
}

impl Ride {
    /// The ride's own minutes at `speed_kmh`, with a branch change's.
    fn minutes(&self, speed_kmh: f64) -> i32 {
        let change = if self.change_at.is_some() {
            BRANCH_CHANGE_MINUTES + BRANCH_CHANGE_WAIT_MINUTES
        } else {
            0
        };
        whole(self.metres / 1000.0 / speed_kmh * 60.0) + change
    }
}

/// Keeps `ride` for `stop` in `best` if it is shorter than what is there.
fn keep_shorter(best: &mut BTreeMap<usize, Ride>, stop: usize, ride: Ride) {
    if best.get(&stop).is_none_or(|kept| ride.metres < kept.metres) {
        best.insert(stop, ride);
    }
}

/// Every stop `stops[from]` can ride to on `line` (or from, for
/// [`Toward::In`]), with the ride: along the line's routes where it has
/// any that pass `from` (one train when one route has both stops in order,
/// one change at the best shared stop when no route has or the change is
/// quicker, else none), on the spanning tree for the rest (see the module
/// doc).
fn rides(
    stops: &[StopRecord],
    routes: &LineRoutes,
    line: &TflStopLine,
    from: usize,
    toward: Toward,
) -> Vec<(usize, Ride)> {
    let tree = |keep: &dyn Fn(usize) -> bool| -> Vec<(usize, Ride)> {
        along_line(stops, from, line)
            .into_iter()
            .filter(|(index, _)| keep(*index))
            .map(|(index, metres)| {
                (
                    index,
                    Ride {
                        metres,
                        stops: None,
                        change_at: None,
                    },
                )
            })
            .collect()
    };
    let line_routes: Vec<Route> = routes
        .by_line
        .get(&line.id)
        .map(|routes| {
            routes
                .iter()
                .map(|route| match toward {
                    Toward::Out => route.clone(),
                    Toward::In => route.reversed(),
                })
                .collect()
        })
        .unwrap_or_default();
    if !line_routes.iter().any(|route| route.stops.contains(&from)) {
        return tree(&|_| true);
    }

    // No change: one route has `from`, then the other stop.
    let mut best: BTreeMap<usize, Ride> = BTreeMap::new();
    for route in &line_routes {
        for i in route.positions(from) {
            for j in i + 1..route.stops.len() {
                let ride = Ride {
                    metres: route.along[j] - route.along[i],
                    stops: Some(j - i),
                    change_at: None,
                };
                keep_shorter(&mut best, route.stops[j], ride);
            }
        }
    }
    best.remove(&from);

    // One change, at a stop reached without one (so never two), onto a
    // route on from it. A stop one train reaches may still be quicker
    // with a change (DS-191): the Circle's trains run from Paddington round
    // by Victoria to Liverpool Street, but to Edgware Road and change is
    // half as far. The guard:
    // - the change is never at either end: `junction` is in `best`, which
    //   has no `from`, and a stop the second train comes back to (the
    //   Circle's Edgware Road) is skipped;
    // - the second train never passes back through `from`: its stops from
    //   there on are not taken;
    // - a ride with a change replaces one without only when its minutes
    //   (the change's included) are fewer; a tie keeps the one train.
    let speed_kmh = line_mode(line).1;
    let mut changed: BTreeMap<usize, Ride> = BTreeMap::new();
    for (&junction, first) in &best {
        for route in &line_routes {
            for k in route.positions(junction) {
                for m in k + 1..route.stops.len() {
                    let to = route.stops[m];
                    if to == from {
                        break;
                    }
                    if to == junction {
                        continue;
                    }
                    let ride = Ride {
                        metres: first.metres + route.along[m] - route.along[k],
                        stops: first.stops.map(|n| n + m - k),
                        change_at: Some(junction),
                    };
                    keep_shorter(&mut changed, to, ride);
                }
            }
        }
    }
    for (to, ride) in changed {
        let quicker = best
            .get(&to)
            .is_none_or(|kept| ride.minutes(speed_kmh) < kept.minutes(speed_kmh));
        if quicker {
            best.insert(to, ride);
        }
    }

    // Stops of the line that no route has: the tree's estimate.
    let on_routes: std::collections::HashSet<usize> = line_routes
        .iter()
        .flat_map(|route| route.stops.iter().copied())
        .collect();
    let mut out: Vec<(usize, Ride)> = best.into_iter().collect();
    out.extend(tree(&|index| index != from && !on_routes.contains(&index)));
    out
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "a route has at most a few hundred stops"
)]
fn stop_count(ride: &Ride) -> Option<u32> {
    ride.stops.map(|n| n as u32)
}

fn hub_candidates(stop: &StopRecord) -> Vec<AccessCandidate> {
    stop.rail_crs
        .iter()
        .map(|crs| AccessCandidate {
            crs: crs.clone(),
            mode: "WALK".to_owned(),
            minutes: HUB_WALK_MINUTES,
            basis: AccessBasis::Hub,
            line: None,
            station_stop: None,
            distance_metres: 0,
            stop_count: None,
            change_at: None,
            board_stop: None,
            alight_stop: None,
        })
        .collect()
}

/// `stops[at]` and the other stop areas of its station, each with the walk
/// between them in minutes ([`walk_minutes`]; 0 for `stops[at]` itself),
/// itself first (DS-191): the stops of its `TfL` hub ([`StopRecord::hub`])
/// within [`SIBLING_MAX_METRES`] of it. Paddington's Bakerloo, Circle and
/// District stop and its Hammersmith & City stop, 255 m apart, are one
/// station: a ride from either is a ride from Paddington. A hub's stops
/// share its National Rail station too (rule 1, `HUBPAD` is `PAD`), but
/// not every hub has one (Hammersmith's two stops, Bank's DLR and Tube).
fn with_siblings(stops: &[StopRecord], at: usize) -> Vec<(usize, i32)> {
    let mut out = vec![(at, 0)];
    let (Some(hub), Some(position)) = (stops[at].hub.as_deref(), stops[at].position) else {
        return out;
    };
    for (index, stop) in stops.iter().enumerate() {
        if index == at || stop.hub.as_deref() != Some(hub) {
            continue;
        }
        let Some(other) = stop.position else {
            continue;
        };
        let d = metres_between(position, other);
        if d <= SIBLING_MAX_METRES {
            out.push((index, walk_minutes(d)));
        }
    }
    out
}

/// One ride from a plan's stop end, or into it for [`Toward::In`]: see
/// [`end_rides`].
#[derive(Debug, Clone, Copy)]
struct EndRide<'a> {
    /// Where the ride starts (ends, into the stop): the stop end itself or
    /// one of its siblings ([`with_siblings`]).
    end: usize,
    /// The walk between the stop end and `end`, in minutes.
    walk: i32,
    line: &'a TflStopLine,
    /// The stop at the ride's other end.
    other: usize,
    ride: Ride,
}

impl EndRide<'_> {
    /// The walk to (or from) `end`, [`RIDE_ENTRY_MINUTES`] and the ride.
    fn minutes(&self) -> i32 {
        self.walk + RIDE_ENTRY_MINUTES + self.ride.minutes(line_mode(self.line).1)
    }

    /// As a candidate for `crs`, at `minutes`, `stops[at]` being the stop
    /// end: a sibling `end` is where the ride is boarded (or, into the
    /// stop, left).
    fn candidate(
        &self,
        stops: &[StopRecord],
        at: usize,
        toward: Toward,
        crs: String,
        minutes: i32,
        station_stop: Option<usize>,
    ) -> AccessCandidate {
        let sibling = (self.end != at).then(|| stops[self.end].summary.clone());
        let (board_stop, alight_stop) = match toward {
            Toward::Out => (sibling, None),
            Toward::In => (None, sibling),
        };
        AccessCandidate {
            crs,
            mode: line_mode(self.line).0.to_owned(),
            minutes,
            basis: AccessBasis::Line,
            line: Some(self.line.name.clone()),
            station_stop: station_stop.map(|index| stops[index].summary.clone()),
            distance_metres: metres(self.ride.metres),
            stop_count: stop_count(&self.ride),
            change_at: self
                .ride
                .change_at
                .map(|index| stops[index].summary.clone()),
            board_stop,
            alight_stop,
        }
    }
}

/// Every ride out of `stops[at]` ([`rides`]; into it, for [`Toward::In`])
/// and out of each of its siblings ([`with_siblings`]), on each of its
/// lines, to a stop that is none of them: the stop end's own lines first.
fn end_rides<'a>(
    stops: &'a [StopRecord],
    routes: &LineRoutes,
    at: usize,
    toward: Toward,
) -> Vec<EndRide<'a>> {
    let ends = with_siblings(stops, at);
    let mut out = Vec::new();
    for &(end, walk) in &ends {
        if stops[end].position.is_none() {
            continue;
        }
        for line in &stops[end].summary.lines {
            for (other, ride) in rides(stops, routes, line, end, toward) {
                if ends.iter().all(|(e, _)| *e != other) {
                    out.push(EndRide {
                        end,
                        walk,
                        line,
                        other,
                        ride,
                    });
                }
            }
        }
    }
    out
}

/// Rides from `stops[from]` on each of its lines (into it, for
/// [`Toward::In`]), and on each of its siblings' lines from them
/// ([`with_siblings`], with the walk), to the quickest
/// [`LINE_CANDIDATES_PER_LINE`] stops with a hub station: rule 2's line
/// half. A station may come twice (two lines); the caller keeps the
/// quicker.
fn line_candidates(
    stops: &[StopRecord],
    routes: &LineRoutes,
    from: usize,
    toward: Toward,
) -> Vec<AccessCandidate> {
    let mut out: Vec<AccessCandidate> = Vec::new();
    if stops[from].position.is_none() {
        return out;
    }
    let all = end_rides(stops, routes, from, toward);
    let mut lines: Vec<&str> = Vec::new();
    for ride in &all {
        if !lines.contains(&ride.line.id.as_str()) {
            lines.push(&ride.line.id);
        }
    }
    for line in lines {
        // The quickest stops along the line with a hub station, each by its
        // quickest ride (the stop end's own on a tie), each with every
        // station of its hub (King's Cross St. Pancras: KGX, STP...).
        let mut quickest: BTreeMap<usize, (&EndRide<'_>, i32)> = BTreeMap::new();
        for ride in all
            .iter()
            .filter(|r| r.line.id == line && !stops[r.other].rail_crs.is_empty())
        {
            let minutes = ride.minutes() + RIDE_EXIT_MINUTES;
            if quickest
                .get(&ride.other)
                .is_none_or(|(_, kept)| minutes < *kept)
            {
                quickest.insert(ride.other, (ride, minutes));
            }
        }
        let mut reached: Vec<(&EndRide<'_>, i32)> = quickest.into_values().collect();
        reached.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then(a.0.ride.metres.total_cmp(&b.0.ride.metres))
                .then(a.0.other.cmp(&b.0.other))
        });
        for (ride, minutes) in reached.into_iter().take(LINE_CANDIDATES_PER_LINE) {
            if minutes > RIDE_MAX_MINUTES {
                break;
            }
            for crs in &stops[ride.other].rail_crs {
                out.push(ride.candidate(
                    stops,
                    from,
                    toward,
                    crs.clone(),
                    minutes,
                    Some(ride.other),
                ));
            }
        }
    }
    out
}

/// Stations within `max_metres` of `position`, nearest first, as walks.
fn walk_candidates(
    stations: &[(String, (f64, f64))],
    position: (f64, f64),
    max_metres: f64,
    basis: AccessBasis,
) -> Vec<AccessCandidate> {
    let mut near: Vec<(f64, &String)> = stations
        .iter()
        .map(|(crs, at)| (metres_between(position, *at), crs))
        .filter(|(d, _)| *d <= max_metres)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    near.into_iter()
        .map(|(d, crs)| AccessCandidate {
            crs: crs.clone(),
            mode: "WALK".to_owned(),
            minutes: walk_minutes(d),
            basis,
            line: None,
            station_stop: None,
            distance_metres: metres(d),
            stop_count: None,
            change_at: None,
            board_stop: None,
            alight_stop: None,
        })
        .collect()
}

/// The access candidates of `stops[from]` (see the module doc), quickest
/// first, one per station: rides out of the stop for [`Toward::Out`] (an
/// origin), into it for [`Toward::In`]. `stations` are National Rail
/// stations with their positions (only those within
/// [`NEAREST_MAX_METRES`] matter).
pub fn access_candidates(
    stops: &[StopRecord],
    routes: &LineRoutes,
    from: usize,
    toward: Toward,
    stations: &[(String, (f64, f64))],
) -> Vec<AccessCandidate> {
    // The hub walk stays, but no longer stands alone (DS-167): a hub stop
    // rides its lines and walks too, as Old Street (`OLD`, a few trains a
    // day) needs its Northern line ride to Euston. Its own stations keep
    // the hub walk: a walk or a ride there is not the hub's estimate.
    let hub = hub_candidates(&stops[from]);
    let mut out = line_candidates(stops, routes, from, toward);
    if let Some(position) = stops[from].position {
        out.extend(walk_candidates(
            stations,
            position,
            WALK_MAX_METRES,
            AccessBasis::Walk,
        ));
    }
    out.sort_by(|a, b| (a.minutes, &a.crs).cmp(&(b.minutes, &b.crs)));
    let mut seen: Vec<String> = hub.iter().map(|c| c.crs.clone()).collect();
    out.retain(|candidate| {
        let new = !seen.contains(&candidate.crs);
        seen.push(candidate.crs.clone());
        new
    });
    out.truncate(MAX_CANDIDATES.saturating_sub(hub.len()));
    out.extend(hub);
    out.sort_by(|a, b| (a.minutes, &a.crs).cmp(&(b.minutes, &b.crs)));
    if out.is_empty()
        && let Some(position) = stops[from].position
    {
        out = walk_candidates(stations, position, NEAREST_MAX_METRES, AccessBasis::Nearest);
        out.truncate(NEAREST_COUNT);
    }
    out
}

/// A ride straight from `stops[from]` to `stops[to]` on a line they share,
/// the quickest, when there is one within [`DIRECT_RIDE_MAX_MINUTES`]: two
/// stops on one line need no train between them. Either end may be ridden
/// from a sibling instead ([`with_siblings`], with the walk): Paddington to
/// Liverpool Street from Paddington's Hammersmith & City stop. Its `crs` is
/// the destination stop's id.
pub fn direct_ride(
    stops: &[StopRecord],
    routes: &LineRoutes,
    from: usize,
    to: usize,
) -> Option<AccessCandidate> {
    stops[from].position?;
    let to_ends = with_siblings(stops, to);
    end_rides(stops, routes, from, Toward::Out)
        .iter()
        .filter_map(|ride| {
            let &(alight, walk) = to_ends.iter().find(|(end, _)| *end == ride.other)?;
            // No platform-to-station walk at the far end: the traveller
            // stays at the Tube stop.
            let minutes = ride.minutes() + walk;
            (minutes <= DIRECT_RIDE_MAX_MINUTES).then(|| AccessCandidate {
                alight_stop: (alight != to).then(|| stops[alight].summary.clone()),
                ..ride.candidate(
                    stops,
                    from,
                    Toward::Out,
                    stops[to].summary.id.clone(),
                    minutes,
                    None,
                )
            })
        })
        .min_by_key(|candidate| candidate.minutes)
}

/// When the plan's other end is the station `crs` and one of its own stops
/// (a stop whose hub stations, rule 1, include it: Euston's is
/// `940GZZLUEUS`) shares a line with `stops[from]` or a sibling of it
/// ([`with_siblings`], with the walk): that station as a candidate, by the
/// quickest such ride at [`direct_ride`]'s minutes (no platform-to-station
/// walk: the traveller's journey ends, or starts, at the station). Rides
/// out of `stops[from]` for [`Toward::Out`], into it for [`Toward::In`].
/// The caller adds it whatever the cuts of rule 2, so Euston to Old Street
/// is always offered as the Northern line ride.
pub fn station_ride(
    stops: &[StopRecord],
    routes: &LineRoutes,
    from: usize,
    toward: Toward,
    crs: &str,
) -> Option<AccessCandidate> {
    stops[from].position?;
    if stops[from].rail_crs.iter().any(|c| c == crs) {
        // `stops[from]` is one of its stops: the hub walk.
        return None;
    }
    let ends = with_siblings(stops, from);
    let own: Vec<usize> = stops
        .iter()
        .enumerate()
        .filter(|(index, stop)| {
            ends.iter().all(|(end, _)| end != index) && stop.rail_crs.iter().any(|c| c == crs)
        })
        .map(|(index, _)| index)
        .collect();
    if own.is_empty() {
        return None;
    }
    end_rides(stops, routes, from, toward)
        .iter()
        .filter(|ride| own.contains(&ride.other))
        .filter_map(|ride| {
            let minutes = ride.minutes();
            (minutes <= DIRECT_RIDE_MAX_MINUTES).then(|| {
                ride.candidate(
                    stops,
                    from,
                    toward,
                    crs.to_owned(),
                    minutes,
                    Some(ride.other),
                )
            })
        })
        .min_by(|a, b| {
            (a.minutes, a.distance_metres, &a.line).cmp(&(b.minutes, b.distance_metres, &b.line))
        })
}

/// Puts `ride` ([`station_ride`]) into `candidates` in place of a slower
/// candidate for its station, quickest first; a quicker one stays.
fn merge_station_ride(candidates: &mut Vec<AccessCandidate>, ride: AccessCandidate) {
    if let Some(at) = candidates.iter().position(|c| c.crs == ride.crs) {
        if candidates[at].minutes <= ride.minutes {
            return;
        }
        candidates.remove(at);
    }
    let at = candidates
        .iter()
        .position(|c| (c.minutes, &c.crs) > (ride.minutes, &ride.crs))
        .unwrap_or(candidates.len());
    candidates.insert(at, ride);
}

/// A plan's `TfL` stop ends: what the search and the response need.
#[derive(Debug, Clone, Default)]
pub struct AccessEnds {
    pub origin: Option<StopAccess>,
    pub destination: Option<StopAccess>,
    /// See [`direct_ride`].
    pub direct: Option<AccessCandidate>,
}

/// Why a stop cannot be an end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessError {
    UnknownStop(String),
    NoStation { id: String, name: String },
}

impl std::fmt::Display for AccessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccessError::UnknownStop(id) => write!(f, "'{id}' is not a known TfL stop"),
            AccessError::NoStation { id, name } => write!(
                f,
                "no National Rail station could be found near {name} ({id})"
            ),
        }
    }
}

#[derive(sqlx::FromRow)]
struct StopRow {
    stop_id: String,
    name: String,
    modes: Vec<String>,
    lines: sqlx::types::Json<Vec<TflStopLine>>,
    hub_code: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

/// Every `tfl_stops` row (a few hundred) with its hub stations.
async fn load_stops(pool: &PgPool) -> Result<Vec<StopRecord>> {
    let rows: Vec<StopRow> = sqlx::query_as(
        "SELECT stop_id, name, modes, lines, hub_code, latitude, longitude FROM tfl_stops \
         ORDER BY stop_id",
    )
    .fetch_all(pool)
    .await?;
    let links: Vec<(String, String)> = sqlx::query_as(
        "SELECT stop_id, crs::text FROM tfl_stop_crs ORDER BY stop_id, source <> 'hub', crs",
    )
    .fetch_all(pool)
    .await?;
    let mut rail: HashMap<String, Vec<String>> = HashMap::new();
    for (crs, stop) in tfl_stops::CRS_OVERRIDES {
        rail.entry(stop.to_owned())
            .or_default()
            .push(crs.to_owned());
    }
    for (stop, crs) in links {
        let codes = rail.entry(stop).or_default();
        let crs = crs.trim().to_ascii_uppercase();
        if !codes.contains(&crs) {
            codes.push(crs);
        }
    }
    Ok(rows
        .into_iter()
        .map(|row| StopRecord {
            rail_crs: rail.remove(&row.stop_id).unwrap_or_default(),
            position: row.latitude.zip(row.longitude),
            hub: row.hub_code,
            summary: TflStopSummary {
                id: row.stop_id,
                name: row.name,
                modes: row.modes,
                lines: row.lines.0,
            },
        })
        .collect())
}

/// Every `tfl_line_routes` row (about 80) as `(line id, stop ids)`.
async fn load_routes(pool: &PgPool) -> Result<Vec<(String, Vec<String>)>> {
    Ok(sqlx::query_as(
        "SELECT line_id, stop_ids FROM tfl_line_routes ORDER BY line_id, direction, route_index",
    )
    .fetch_all(pool)
    .await?)
}

/// `api_tfl_access_routes_missing_total{line, reason}`: each line of a
/// planned `TfL` stop end whose rides fall back to the spanning tree
/// (see the module doc), `reason` `line` (no routes loaded for the line:
/// is poller-tfl's routes job on?) or `stop` (the line's routes do not
/// pass the stop).
pub const ROUTES_MISSING_METRIC: &str = "api_tfl_access_routes_missing_total";
/// At most one warning per this long, per api process, for stop ends
/// planned with no routes loaded for a line of theirs.
const ROUTES_MISSING_WARN_EVERY: Duration = Duration::from_secs(60 * 60);
static ROUTES_MISSING_WARNED: Mutex<Option<Instant>> = Mutex::new(None);

/// Why rides from a stop on a line fall back to the spanning tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RoutesMissing {
    /// No routes are loaded for the line.
    Line,
    /// The line's routes do not pass the stop.
    Stop,
}

impl RoutesMissing {
    fn metric_label(self) -> &'static str {
        match self {
            RoutesMissing::Line => "line",
            RoutesMissing::Stop => "stop",
        }
    }
}

impl LineRoutes {
    /// Why `stop`'s rides on `line` use the spanning tree, if they do.
    fn missing(&self, line: &str, stop: usize) -> Option<RoutesMissing> {
        match self.by_line.get(line) {
            None => Some(RoutesMissing::Line),
            Some(routes) if !routes.iter().any(|r| r.stops.contains(&stop)) => {
                Some(RoutesMissing::Stop)
            }
            Some(_) => None,
        }
    }
}

/// Whether a throttled warning is due at `now`: none in the last `every`
/// (and records it).
fn warn_due(last: &Mutex<Option<Instant>>, now: Instant, every: Duration) -> bool {
    let mut last = last.lock().unwrap_or_else(PoisonError::into_inner);
    if last.is_some_and(|at| now.saturating_duration_since(at) < every) {
        return false;
    }
    *last = Some(now);
    true
}

/// Counts [`ROUTES_MISSING_METRIC`] for the stop end `stops[end]`, and
/// warns (at most every [`ROUTES_MISSING_WARN_EVERY`]) when a line of it
/// has no routes loaded at all.
fn record_missing_routes(stops: &[StopRecord], routes: &LineRoutes, end: usize) {
    let mut without: Vec<&str> = Vec::new();
    for line in &stops[end].summary.lines {
        let Some(reason) = routes.missing(&line.id, end) else {
            continue;
        };
        metrics::counter!(
            common::metrics::metric_name(ROUTES_MISSING_METRIC),
            "line" => line.id.clone(),
            "reason" => reason.metric_label()
        )
        .increment(1);
        if reason == RoutesMissing::Line {
            without.push(&line.id);
        }
    }
    if !without.is_empty()
        && warn_due(
            &ROUTES_MISSING_WARNED,
            Instant::now(),
            ROUTES_MISSING_WARN_EVERY,
        )
    {
        tracing::warn!(
            stop = %stops[end].summary.id,
            lines = ?without,
            "a TfL stop end was planned with no routes loaded for its lines; rides on them are \
             straight-line estimates (is poller-tfl's routes job on?)"
        );
    }
}

/// National Rail stations within `max_metres` of `position` (a bounding
/// box read; the caller measures).
async fn stations_near(
    pool: &PgPool,
    position: (f64, f64),
    max_metres: f64,
) -> Result<Vec<(String, (f64, f64))>> {
    let lat_span = max_metres / 111_000.0;
    let lon_span = max_metres / (111_000.0 * position.0.to_radians().cos().max(0.1));
    let rows: Vec<(String, f64, f64)> = sqlx::query_as(
        r"
        SELECT crs::text, latitude, longitude FROM stations
        WHERE latitude BETWEEN $1 AND $2 AND longitude BETWEEN $3 AND $4
        ORDER BY crs
        ",
    )
    .bind(position.0 - lat_span)
    .bind(position.0 + lat_span)
    .bind(position.1 - lon_span)
    .bind(position.1 + lon_span)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(crs, lat, lon)| (crs.trim().to_ascii_uppercase(), (lat, lon)))
        .collect())
}

/// The access candidates of `origin` and `destination`, the request's own
/// codes: a stop id ([`is_tfl_stop_id`]) is a stop end, anything else a
/// station end (a CRS there may add a [`station_ride`] to the other end's
/// candidates). `Ok(Err(..))` for an unknown stop or one with no candidate
/// at all.
pub async fn load_access(
    pool: &PgPool,
    origin: &str,
    destination: &str,
) -> Result<Result<AccessEnds, AccessError>> {
    let codes = [origin, destination];
    let [origin, destination] = codes.map(|code| is_tfl_stop_id(code).then(|| code.trim()));
    if origin.is_none() && destination.is_none() {
        return Ok(Ok(AccessEnds::default()));
    }
    // A station end's CRS, for the other end's station ride.
    let station = |code: &str| {
        let code = code.trim().to_ascii_uppercase();
        (!is_tfl_stop_id(&code) && code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase()))
            .then_some(code)
    };
    let others = [station(codes[1]), station(codes[0])];
    let stops = load_stops(pool).await?;
    let routes = LineRoutes::new(&stops, &load_routes(pool).await?);
    let mut indices = [None, None];
    let mut ends = [None, None];
    for (slot, id) in [origin, destination].into_iter().enumerate() {
        let Some(id) = id else { continue };
        let Some(index) = stops.iter().position(|stop| stop.summary.id == id) else {
            return Ok(Err(AccessError::UnknownStop(id.to_owned())));
        };
        let stations = match stops[index].position {
            Some(position) => stations_near(pool, position, NEAREST_MAX_METRES).await?,
            None => Vec::new(),
        };
        let toward = if slot == 0 { Toward::Out } else { Toward::In };
        let mut candidates = access_candidates(&stops, &routes, index, toward, &stations);
        if let Some(ride) = others[slot]
            .as_deref()
            .and_then(|crs| station_ride(&stops, &routes, index, toward, crs))
        {
            merge_station_ride(&mut candidates, ride);
        }
        if candidates.is_empty() {
            return Ok(Err(AccessError::NoStation {
                id: id.to_owned(),
                name: stops[index].summary.name.clone(),
            }));
        }
        record_missing_routes(&stops, &routes, index);
        indices[slot] = Some(index);
        ends[slot] = Some(StopAccess {
            stop: stops[index].summary.clone(),
            candidates,
        });
    }
    let [origin, destination] = ends;
    let direct = match indices {
        [Some(from), Some(to)] => direct_ride(&stops, &routes, from, to),
        _ => None,
    };
    Ok(Ok(AccessEnds {
        origin,
        destination,
        direct,
    }))
}

impl AccessEnds {
    /// Drops the candidates whose station the timetable does not know (no
    /// TIPLOC in `interchange`). `Err` when an end is left with none.
    pub fn retain_known(&mut self, interchange: &InterchangeData) -> Result<(), AccessError> {
        for end in [&mut self.origin, &mut self.destination]
            .into_iter()
            .flatten()
        {
            end.candidates
                .retain(|candidate| interchange.crs_to_tiplocs.contains_key(&candidate.crs));
            if end.candidates.is_empty() {
                return Err(AccessError::NoStation {
                    id: end.stop.id.clone(),
                    name: end.stop.name.clone(),
                });
            }
        }
        Ok(())
    }

    /// `base` with this request's virtual stops and their links
    /// (`trip_planner::with_access`); `base` itself is not changed.
    pub fn interchange(&self, base: &InterchangeData) -> InterchangeData {
        fn links(end: Option<&StopAccess>) -> Option<(&str, Vec<AccessLink>)> {
            end.map(|end| {
                (
                    end.stop.id.as_str(),
                    end.candidates.iter().map(AccessCandidate::link).collect(),
                )
            })
        }
        let (origin, destination) = (
            links(self.origin.as_ref()),
            links(self.destination.as_ref()),
        );
        let direct = self.direct.as_ref().map(AccessCandidate::link);
        trip_planner::with_access(
            base,
            origin
                .as_ref()
                .map(|(code, links)| AccessEnd { code, links }),
            destination
                .as_ref()
                .map(|(code, links)| AccessEnd { code, links }),
            direct.as_ref(),
        )
    }

    /// The response's top-level `access`: each stop end with its
    /// candidates, `null` for a station end.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "origin": self.origin,
            "destination": self.destination,
        })
    }

    /// Dresses the access legs of `segments` (run after
    /// `tfl_stops::attach_transfer_lines`, whose stops and lines it
    /// replaces on these legs): the stop end's code (a stop id, not a CRS)
    /// becomes `originCrs`/`destinationCrs` `null` with the stop as
    /// `originStop`/`destinationStop`; a ride names its line and the stop at
    /// the station's end; each carries `access`. With `depart_after`, an
    /// itinerary that starts with an access leg leaves as late as still
    /// makes its first train (the search starts the leg at the requested
    /// time), its first train's departure less the legs before it and the
    /// minimum change time there.
    pub fn finish_legs(
        &self,
        segments: &mut [SegmentResult],
        interchange: &InterchangeData,
        depart_after: bool,
    ) {
        let last = segments.len().saturating_sub(1);
        for (index, segment) in segments.iter_mut().enumerate() {
            let origin = self.origin.as_ref().filter(|_| index == 0);
            let destination = self.destination.as_ref().filter(|_| index == last);
            if origin.is_none() && destination.is_none() {
                continue;
            }
            for itinerary in &mut segment.itineraries {
                for leg in &mut itinerary.legs {
                    self.finish_leg(leg, origin, destination);
                }
                if depart_after && origin.is_some() {
                    leave_just_in_time(itinerary, interchange);
                }
            }
        }
    }

    fn finish_leg(
        &self,
        leg: &mut PlannedLeg,
        origin: Option<&StopAccess>,
        destination: Option<&StopAccess>,
    ) {
        let PlannedLeg::Transfer {
            mode,
            origin_crs,
            destination_crs,
            line,
            origin_stop,
            destination_stop,
            access,
            ..
        } = leg
        else {
            return;
        };
        let from = origin.filter(|end| origin_crs.as_deref() == Some(end.stop.id.as_str()));
        let to = destination.filter(|end| destination_crs.as_deref() == Some(end.stop.id.as_str()));
        let (candidate, end, station_end) = match (from, to) {
            (Some(_), Some(_)) => (self.direct.as_ref(), AccessLegEnd::Both, None),
            (Some(stop), None) => (
                stop.candidates
                    .iter()
                    .find(|c| Some(&c.crs) == destination_crs.as_ref() && c.mode == *mode),
                AccessLegEnd::Origin,
                Some(false),
            ),
            (None, Some(stop)) => (
                stop.candidates
                    .iter()
                    .find(|c| Some(&c.crs) == origin_crs.as_ref() && c.mode == *mode),
                AccessLegEnd::Destination,
                Some(true),
            ),
            (None, None) => return,
        };
        let Some(candidate) = candidate else {
            return;
        };
        if let Some(stop) = from {
            *origin_crs = None;
            *origin_stop = Some(Box::new(stop.stop.clone()));
        }
        if let Some(stop) = to {
            *destination_crs = None;
            *destination_stop = Some(Box::new(stop.stop.clone()));
        }
        // The station end of a ride is the stop the ride reaches there; a
        // walk's is the station itself.
        match station_end {
            Some(false) => *destination_stop = candidate.station_stop.clone().map(Box::new),
            Some(true) => *origin_stop = candidate.station_stop.clone().map(Box::new),
            None => {}
        }
        line.clone_from(&candidate.line);
        *access = Some(Box::new(AccessLeg {
            end,
            basis: candidate.basis,
            distance_metres: candidate.distance_metres,
            change_at: candidate.change_at.clone(),
            board_stop: candidate.board_stop.clone(),
            alight_stop: candidate.alight_stop.clone(),
        }));
    }
}

/// See [`AccessEnds::finish_legs`].
fn leave_just_in_time(itinerary: &mut PlannedItinerary, interchange: &InterchangeData) {
    let starts_with_access = matches!(
        itinerary.legs.first(),
        Some(PlannedLeg::Transfer { access: Some(access), .. })
            if access.end == AccessLegEnd::Origin
    );
    if !starts_with_access {
        return;
    }
    let Some(train) = itinerary
        .legs
        .iter()
        .position(|leg| matches!(leg, PlannedLeg::Train { .. }))
    else {
        return;
    };
    let before: u32 = itinerary.legs[..train]
        .iter()
        .map(|leg| match leg {
            PlannedLeg::Transfer { minutes, .. } => (*minutes).max(0).unsigned_abs(),
            PlannedLeg::Train { .. } => 0,
        })
        .sum();
    let PlannedLeg::Train {
        departure_min,
        from_tiploc,
        train_uid,
        ..
    } = &itinerary.legs[train]
    else {
        return;
    };
    let change = match minimum_change_time(interchange, from_tiploc) {
        ChangeTime::Finite(minutes) => minutes,
        ChangeTime::NoInterchange => 0,
    } + interchange.modal_change.extra_for(train_uid);
    let latest = departure_min.saturating_sub(before + change);
    if latest > itinerary.departure_min {
        itinerary.total_duration_minutes = itinerary
            .total_duration_minutes
            .saturating_sub(latest - itinerary.departure_min);
        itinerary.departure_min = latest;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: &str) -> TflStopLine {
        TflStopLine {
            id: id.to_owned(),
            name: format!("{}{}", id[..1].to_ascii_uppercase(), &id[1..]),
        }
    }

    fn stop(id: &str, lines: &[&str], at: (f64, f64), rail: &[&str]) -> StopRecord {
        StopRecord {
            summary: TflStopSummary {
                id: id.to_owned(),
                name: id.to_owned(),
                modes: vec!["tube".to_owned()],
                lines: lines.iter().map(|l| line(l)).collect(),
            },
            position: Some(at),
            rail_crs: rail.iter().map(|c| (*c).to_owned()).collect(),
            hub: None,
        }
    }

    /// An Uxbridge-like corner of the network, with real positions: the
    /// Uxbridge branch of the Metropolitan and Piccadilly lines to Rayners
    /// Lane, the Metropolitan on to Harrow-on-the-Hill (a hub, `HOH`) and
    /// the Piccadilly south through Acton Town to Heathrow (`HXX`): close
    /// to Uxbridge as the crow flies, a long way round by the line.
    fn network() -> Vec<StopRecord> {
        vec![
            stop(
                "940GUXB",
                &["metropolitan", "piccadilly"],
                (51.5466, -0.4779),
                &[],
            ),
            stop(
                "940GICK",
                &["metropolitan", "piccadilly"],
                (51.5619, -0.4421),
                &[],
            ),
            stop(
                "940GRUI",
                &["metropolitan", "piccadilly"],
                (51.5714, -0.4213),
                &[],
            ),
            stop(
                "940GRLN",
                &["metropolitan", "piccadilly"],
                (51.5753, -0.3714),
                &[],
            ),
            stop("940GWHR", &["metropolitan"], (51.5795, -0.3533), &[]),
            stop("940GHOH", &["metropolitan"], (51.5792, -0.3372), &["HOH"]),
            stop("940GSHA", &["piccadilly"], (51.5646, -0.3521), &[]),
            stop("940GALP", &["piccadilly"], (51.5408, -0.2996), &[]),
            stop("940GACT", &["piccadilly"], (51.5028, -0.2801), &[]),
            stop("940GHNE", &["piccadilly"], (51.4767, -0.3666), &[]),
            stop("940GHTX", &["piccadilly"], (51.4669, -0.4232), &[]),
            stop("940GHRC", &["piccadilly"], (51.4712, -0.4523), &["HXX"]),
        ]
    }

    #[test]
    fn stop_ids_are_naptan_stop_areas_only() {
        assert!(is_tfl_stop_id("940GZZLUUXB"));
        assert!(is_tfl_stop_id(" 940GZZDLBEC "));
        for code in [
            "UXB",
            "tiploc:UXBRIDG",
            "940gzzluuxb",
            "940G",
            "490G00123456",
        ] {
            assert!(!is_tfl_stop_id(code), "{code}");
        }
    }

    #[test]
    fn a_tube_only_stop_rides_its_lines_to_hub_stations() {
        let stops = network();
        let candidates = access_candidates(&stops, &LineRoutes::default(), 0, Toward::Out, &[]);
        let summary: Vec<(&str, &str, i32, Option<&str>)> = candidates
            .iter()
            .map(|c| {
                (
                    c.crs.as_str(),
                    c.mode.as_str(),
                    c.minutes,
                    c.line.as_deref(),
                )
            })
            .collect();
        // Uxbridge to Harrow-on-the-Hill is about 11 km along the
        // Metropolitan (5 + 22 + 3 minutes); Heathrow is 8.5 km away but
        // about 32 km by the Piccadilly, round by Acton Town (5 + 64 + 3).
        assert_eq!(
            summary,
            [
                ("HOH", "TUBE", 30, Some("Metropolitan")),
                ("HXX", "TUBE", 72, Some("Piccadilly")),
            ]
        );
        assert_eq!(candidates[0].basis, AccessBasis::Line);
        assert_eq!(
            candidates[0].station_stop.as_ref().map(|s| s.id.as_str()),
            Some("940GHOH")
        );
    }

    #[test]
    fn a_hub_stop_walks_to_its_own_station_and_rides_its_lines() {
        let (stops, routes) = branched();
        let hoh = index(&stops, "940GHOH");
        // Its own station, and a walk to it (0 m, 1 minute), stays the hub's.
        let stations = vec![("HOH".to_owned(), (51.5792, -0.3372))];
        let candidates = access_candidates(&stops, &routes, hoh, Toward::Out, &stations);
        let summary: Vec<(&str, &str, AccessBasis)> = candidates
            .iter()
            .map(|c| (c.crs.as_str(), c.mode.as_str(), c.basis))
            .collect();
        assert_eq!(
            summary,
            [
                ("HOH", "WALK", AccessBasis::Hub),
                ("RIC", "TUBE", AccessBasis::Line),
            ]
        );
        assert_eq!(candidates[0].minutes, HUB_WALK_MINUTES);
        assert_eq!(candidates[0].distance_metres, 0);
    }

    #[test]
    fn a_station_within_a_kilometre_is_a_walk_beside_the_rides() {
        let stops = network();
        // West Ruislip, about 600 m from the Ruislip stop: 750 m on foot at
        // 75 m a minute, 10.02 minutes, so 11.
        let stations = vec![("WRU".to_owned(), (51.5698, -0.4296))];
        let candidates =
            access_candidates(&stops, &LineRoutes::default(), 2, Toward::Out, &stations);
        let walk = candidates.iter().find(|c| c.crs == "WRU").unwrap();
        assert_eq!(walk.mode, "WALK");
        assert_eq!(walk.basis, AccessBasis::Walk);
        assert_eq!(walk.distance_metres, 601);
        assert_eq!(walk.minutes, 11);
        assert_eq!(candidates[0].crs, "WRU", "quickest first");
        assert!(candidates.iter().any(|c| c.crs == "HOH"));
    }

    #[test]
    fn with_no_line_or_walk_the_nearest_stations_are_walks() {
        let mut stops = network();
        // A stop on a line with no hub, and stations 1.5, 2 and 2.5 km away
        // and one 4 km away.
        stops.push(stop("940GLONE", &["lonely"], (51.60, -0.60), &[]));
        let stations: Vec<(String, (f64, f64))> = [
            ("AAA", 0.0135),
            ("BBB", 0.018),
            ("CCC", 0.0225),
            ("DDD", 0.036),
        ]
        .iter()
        .map(|(crs, dlat)| ((*crs).to_owned(), (51.60 + dlat, -0.60)))
        .collect();
        let candidates = access_candidates(
            &stops,
            &LineRoutes::default(),
            stops.len() - 1,
            Toward::Out,
            &stations,
        );
        let codes: Vec<&str> = candidates.iter().map(|c| c.crs.as_str()).collect();
        assert_eq!(codes, ["AAA", "BBB", "CCC"]);
        assert!(candidates.iter().all(|c| c.basis == AccessBasis::Nearest));
        assert_eq!(candidates[0].minutes, walk_minutes(1501.0));
    }

    #[test]
    fn two_stops_on_one_line_ride_straight_there() {
        let stops = network();
        let ride = direct_ride(&stops, &LineRoutes::default(), 0, 3).unwrap();
        assert_eq!(ride.crs, "940GRLN");
        assert_eq!(ride.mode, "TUBE");
        assert_eq!(ride.basis, AccessBasis::Line);
        // Only the Metropolitan reaches Harrow-on-the-Hill.
        let ride = direct_ride(&stops, &LineRoutes::default(), 0, 5).unwrap();
        assert_eq!(ride.line.as_deref(), Some("Metropolitan"));
        // West Harrow and South Harrow share no line.
        assert!(direct_ride(&stops, &LineRoutes::default(), 4, 6).is_none());
    }

    #[test]
    fn dlr_and_tram_rides_are_metro_at_their_own_speeds() {
        assert_eq!(line_mode(&line("dlr")), ("METRO", 25.0));
        assert_eq!(line_mode(&line("tram")), ("METRO", 18.0));
        assert_eq!(line_mode(&line("victoria")), ("TUBE", 30.0));
    }

    /// [`network`] with the Metropolitan's Amersham branch from
    /// Harrow-on-the-Hill (Moor Park, then Rickmansworth, a hub `RIC`),
    /// real positions, and the Metropolitan's routes as `TfL` runs them:
    /// Uxbridge to the trunk, and the trunk to Rickmansworth, each both
    /// ways. Rickmansworth is 10 km from Uxbridge as the crow flies, so
    /// the spanning tree hangs it off the Uxbridge branch.
    fn branched() -> (Vec<StopRecord>, LineRoutes) {
        let mut stops = network();
        stops.push(stop("940GMPK", &["metropolitan"], (51.6302, -0.4325), &[]));
        stops.push(stop(
            "940GRIC",
            &["metropolitan"],
            (51.6402, -0.4733),
            &["RIC"],
        ));
        let uxbridge: Vec<String> = ["940GUXB", "940GICK", "940GRUI", "940GRLN", "940GWHR"]
            .into_iter()
            .chain(["940GHOH"])
            .map(str::to_owned)
            .collect();
        let amersham: Vec<String> = ["940GHOH", "940GMPK", "940GRIC"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let rows: Vec<(String, Vec<String>)> = [uxbridge, amersham]
            .into_iter()
            .flat_map(|ids| {
                let back: Vec<String> = ids.iter().rev().cloned().collect();
                [
                    ("metropolitan".to_owned(), ids),
                    ("metropolitan".to_owned(), back),
                ]
            })
            .collect();
        let routes = LineRoutes::new(&stops, &rows);
        (stops, routes)
    }

    fn index(stops: &[StopRecord], id: &str) -> usize {
        stops.iter().position(|s| s.summary.id == id).unwrap()
    }

    fn metropolitan() -> TflStopLine {
        line("metropolitan")
    }

    #[test]
    fn a_ride_along_a_route_sums_its_stops_and_counts_them() {
        let (stops, routes) = branched();
        let rides = rides(&stops, &routes, &metropolitan(), 0, Toward::Out);
        let (_, harrow) = rides
            .iter()
            .find(|(i, _)| *i == index(&stops, "940GHOH"))
            .unwrap();
        let expected: f64 = [
            "940GUXB", "940GICK", "940GRUI", "940GRLN", "940GWHR", "940GHOH",
        ]
        .windows(2)
        .map(|pair| {
            metres_between(
                stops[index(&stops, pair[0])].position.unwrap(),
                stops[index(&stops, pair[1])].position.unwrap(),
            )
        })
        .sum();
        assert!((harrow.metres - expected).abs() < 1e-6);
        assert_eq!(harrow.stops, Some(5));
        assert_eq!(harrow.change_at, None);
    }

    #[test]
    fn another_branch_is_a_change_at_the_junction() {
        let (stops, routes) = branched();
        let ric = index(&stops, "940GRIC");
        let hoh = index(&stops, "940GHOH");
        let to_ric = |routes: &LineRoutes, toward: Toward, from: usize| {
            rides(&stops, routes, &metropolitan(), from, toward)
                .into_iter()
                .find(|(i, _)| *i == ric)
                .map(|(_, ride)| ride)
        };
        let ride = to_ric(&routes, Toward::Out, 0).unwrap();
        assert_eq!(ride.change_at, Some(hoh));
        assert_eq!(ride.stops, Some(7), "5 to Harrow, 2 on");
        // Uxbridge to Harrow, then Harrow to Rickmansworth.
        let leg = |a: &str, b: &str| {
            rides(
                &stops,
                &routes,
                &metropolitan(),
                index(&stops, a),
                Toward::Out,
            )
            .into_iter()
            .find(|(i, _)| *i == index(&stops, b))
            .unwrap()
            .1
            .metres
        };
        let expected = leg("940GUXB", "940GHOH") + leg("940GHOH", "940GRIC");
        assert!((ride.metres - expected).abs() < 1e-6);
        // The change costs its minimum time and the extra wait.
        assert_eq!(
            ride.minutes(30.0),
            whole(expected / 1000.0 / 30.0 * 60.0)
                + BRANCH_CHANGE_MINUTES
                + BRANCH_CHANGE_WAIT_MINUTES
        );
        // Into Uxbridge from Rickmansworth (an egress ride) changes there too.
        let back = rides(&stops, &routes, &metropolitan(), 0, Toward::In)
            .into_iter()
            .find(|(i, _)| *i == ric)
            .unwrap()
            .1;
        assert_eq!(back.change_at, Some(hoh));
        assert!((back.metres - expected).abs() < 1.0);
        // Before DS-151 (no routes) the tree took it as one ride, no change,
        // and far shorter than the track.
        let tree = to_ric(&LineRoutes::default(), Toward::Out, 0).unwrap();
        assert_eq!(tree.change_at, None);
        assert_eq!(tree.stops, None);
        assert!(tree.metres < expected);
    }

    #[test]
    fn the_branch_change_is_in_the_candidate_and_its_minutes() {
        let (stops, routes) = branched();
        let candidates = access_candidates(&stops, &routes, 0, Toward::Out, &[]);
        let ric = candidates.iter().find(|c| c.crs == "RIC").unwrap();
        assert_eq!(
            ric.change_at.as_ref().map(|s| s.id.as_str()),
            Some("940GHOH")
        );
        assert_eq!(ric.stop_count, Some(7));
        let hoh = candidates.iter().find(|c| c.crs == "HOH").unwrap();
        assert_eq!(hoh.change_at, None);
        assert_eq!(hoh.stop_count, Some(5));
        let along = |c: &AccessCandidate| f64::from(c.distance_metres);
        assert_eq!(
            ric.minutes,
            RIDE_ENTRY_MINUTES
                + whole(along(ric) / 1000.0 / 30.0 * 60.0)
                + BRANCH_CHANGE_MINUTES
                + BRANCH_CHANGE_WAIT_MINUTES
                + RIDE_EXIT_MINUTES
        );
        // The Piccadilly has no routes here: its tree estimate stands.
        let hxx = candidates.iter().find(|c| c.crs == "HXX").unwrap();
        assert_eq!(hxx.minutes, 72);
        assert_eq!(hxx.stop_count, None);

        // Two stops: the direct ride changes at Harrow too.
        let ride = direct_ride(&stops, &routes, 0, index(&stops, "940GRIC")).unwrap();
        assert_eq!(
            ride.change_at.as_ref().map(|s| s.id.as_str()),
            Some("940GHOH")
        );
        assert_eq!(
            ride.minutes,
            RIDE_ENTRY_MINUTES
                + whole(f64::from(ride.distance_metres) / 1000.0 / 30.0 * 60.0)
                + BRANCH_CHANGE_MINUTES
                + BRANCH_CHANGE_WAIT_MINUTES
        );
    }

    #[test]
    fn a_ride_needing_two_changes_is_not_offered() {
        let (stops, _) = branched();
        // Three routes in a chain: Uxbridge to Ickenham, Ruislip to Rayners
        // Lane, and Rayners Lane to Harrow; nothing joins the first two.
        let rows: Vec<(String, Vec<String>)> = [
            ["940GUXB", "940GICK"].as_slice(),
            &["940GRUI", "940GRLN"],
            &["940GRLN", "940GHOH"],
        ]
        .iter()
        .map(|ids| {
            (
                "metropolitan".to_owned(),
                ids.iter().map(|id| (*id).to_owned()).collect(),
            )
        })
        .collect();
        let routes = LineRoutes::new(&stops, &rows);
        let reached: Vec<usize> = rides(&stops, &routes, &metropolitan(), 0, Toward::Out)
            .into_iter()
            .map(|(i, _)| i)
            .collect();
        assert!(reached.contains(&index(&stops, "940GICK")));
        assert!(!reached.contains(&index(&stops, "940GRLN")));
        assert!(!reached.contains(&index(&stops, "940GHOH")));
        // A stop no route has (West Harrow) keeps the tree's estimate.
        assert!(reached.contains(&index(&stops, "940GWHR")));
    }

    #[test]
    fn route_stops_that_are_unknown_are_passed_over() {
        let (stops, _) = branched();
        let rows = vec![(
            "metropolitan".to_owned(),
            vec!["940GUXB".into(), "940GNOPE".into(), "940GICK".into()],
        )];
        let routes = LineRoutes::new(&stops, &rows);
        let ride = rides(&stops, &routes, &metropolitan(), 0, Toward::Out)
            .into_iter()
            .find(|(i, _)| *i == index(&stops, "940GICK"))
            .unwrap()
            .1;
        assert_eq!(ride.stops, Some(1));
    }

    fn ids(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| format!("940GZZLU{id}")).collect()
    }

    /// Both ways along each of `routes`, as `tfl_line_routes` holds them.
    fn both_ways(line: &str, routes: &[Vec<String>]) -> Vec<(String, Vec<String>)> {
        routes
            .iter()
            .flat_map(|ids| {
                let back: Vec<String> = ids.iter().rev().cloned().collect();
                [(line.to_owned(), ids.clone()), (line.to_owned(), back)]
            })
            .collect()
    }

    /// A hand-written subset of the Northern line in central London
    /// (positions to about 100 m, from a map): Camden Town to Kennington by
    /// Bank, and by Charing Cross, a loop through Euston and Kennington.
    /// Hubs: Euston (`EUS`), King's Cross St. Pancras (`KGX`, `STP`), Old
    /// Street (`OLD`), Moorgate (`MOG`), London Bridge (`LBG`), Elephant &
    /// Castle (`EPH`), Waterloo (`WAT`) and Charing Cross (`CHX`).
    fn northern() -> (Vec<StopRecord>, Vec<(String, Vec<String>)>) {
        let n = &["northern"];
        let stops = vec![
            stop("940GZZLUCTN", n, (51.5392, -0.1426), &[]),
            stop("940GZZLUMTC", n, (51.5342, -0.1387), &[]),
            stop("940GZZLUEUS", n, (51.5282, -0.1337), &["EUS"]),
            stop("940GZZLUKSX", n, (51.5308, -0.1238), &["KGX", "STP"]),
            stop("940GZZLUAGL", n, (51.5322, -0.1058), &[]),
            stop("940GZZLUODS", n, (51.5263, -0.0873), &["OLD"]),
            stop("940GZZLUMGT", n, (51.5186, -0.0886), &["MOG"]),
            stop("940GZZLUBNK", n, (51.5133, -0.0886), &[]),
            stop("940GZZLULNB", n, (51.5052, -0.0864), &["LBG"]),
            stop("940GZZLUBOR", n, (51.5011, -0.0943), &[]),
            stop("940GZZLUEAC", n, (51.4943, -0.1001), &["EPH"]),
            stop("940GZZLUKNG", n, (51.4884, -0.1053), &[]),
            stop("940GZZLUWLO", n, (51.5036, -0.1143), &["WAT"]),
            stop("940GZZLUEMB", n, (51.5074, -0.1223), &[]),
            stop("940GZZLUCHX", n, (51.5080, -0.1247), &["CHX"]),
            stop("940GZZLULSQ", n, (51.5113, -0.1281), &[]),
            stop("940GZZLUTCR", n, (51.5165, -0.1310), &[]),
            stop("940GZZLUGDG", n, (51.5205, -0.1346), &[]),
            stop("940GZZLUWRR", n, (51.5247, -0.1384), &[]),
        ];
        let bank = ids(&[
            "CTN", "EUS", "KSX", "AGL", "ODS", "MGT", "BNK", "LNB", "BOR", "EAC", "KNG",
        ]);
        let charing_cross = ids(&[
            "CTN", "MTC", "EUS", "WRR", "GDG", "TCR", "LSQ", "CHX", "EMB", "WLO", "KNG",
        ]);
        (stops, both_ways("northern", &[bank, charing_cross]))
    }

    /// [`northern`] with its branches' first stops (positions to about
    /// 100 m, from a map): Belsize Park and Chalk Farm (Edgware), Tufnell
    /// Park and Kentish Town (High Barnet), Oval, Stockwell and Clapham
    /// North (Morden), Nine Elms and Battersea Power Station. Close
    /// branches: Chalk Farm and Kentish Town either side of Camden Town,
    /// Nine Elms and Stockwell either side of Kennington. Routes from both
    /// northern branches by Bank to Morden and by Charing Cross to
    /// Battersea, and by Charing Cross to Morden from Edgware.
    fn northern_wide() -> (Vec<StopRecord>, Vec<(String, Vec<String>)>) {
        let (mut stops, _) = northern();
        let n = &["northern"];
        for (id, at) in [
            ("BZP", (51.5504, -0.1642)),
            ("CFM", (51.5441, -0.1538)),
            ("TFP", (51.5567, -0.1376)),
            ("KTN", (51.5503, -0.1405)),
            ("OVL", (51.4819, -0.1127)),
            ("SKW", (51.4723, -0.1229)),
            ("CPN", (51.4649, -0.1299)),
            ("NEM", (51.4799, -0.1286)),
            ("BPS", (51.4794, -0.1422)),
        ] {
            stops.push(stop(&format!("940GZZLU{id}"), n, at, &[]));
        }
        let bank = [
            "EUS", "KSX", "AGL", "ODS", "MGT", "BNK", "LNB", "BOR", "EAC", "KNG",
        ];
        let charing_cross = [
            "MTC", "EUS", "WRR", "GDG", "TCR", "LSQ", "CHX", "EMB", "WLO", "KNG",
        ];
        let route = |north: &[&str], middle: &[&str], south: &[&str]| {
            ids(&[north, &["CTN"], middle, south].concat())
        };
        let (barnet, edgware) = (["TFP", "KTN"], ["BZP", "CFM"]);
        let (morden, battersea) = (["OVL", "SKW", "CPN"], ["NEM", "BPS"]);
        let routes = [
            route(&barnet, &bank, &morden),
            route(&edgware, &bank, &morden),
            route(&barnet, &charing_cross, &battersea),
            route(&edgware, &charing_cross, &battersea),
            route(&edgware, &charing_cross, &morden),
        ];
        (stops, both_ways("northern", &routes))
    }

    fn ride_metres(
        stops: &[StopRecord],
        routes: &LineRoutes,
        line_id: &str,
        from: &str,
        to: &str,
    ) -> f64 {
        rides(
            stops,
            routes,
            &line(line_id),
            index(stops, from),
            Toward::Out,
        )
        .into_iter()
        .find(|(i, _)| *i == index(stops, to))
        .unwrap()
        .1
        .metres
    }

    #[test]
    fn the_northern_loop_is_measured_along_its_routes() {
        let (stops, rows) = northern();
        let routes = LineRoutes::new(&stops, &rows);
        let (ods, eus) = ("940GZZLUODS", "940GZZLUEUS");
        // Old Street, Angel, King's Cross, Euston: about 3.4 km.
        let along = ride_metres(&stops, &routes, "northern", ods, eus);
        assert!((3_000.0..4_000.0).contains(&along), "{along}");
        let back = ride_metres(&stops, &routes, "northern", eus, ods);
        assert!((along - back).abs() < 1.0);
        // The spanning tree breaks the loop between Angel and Old Street,
        // and goes round by Bank and Charing Cross: over twice as far.
        let members: Vec<usize> = (0..stops.len()).collect();
        let tree = tree_metres(&stops, &members, index(&stops, ods))[&index(&stops, eus)];
        assert!(tree > 2.0 * along, "{tree}");
        // The fallback closes it again: the same way as the route.
        let fallback = ride_metres(&stops, &LineRoutes::default(), "northern", ods, eus);
        assert!((fallback - along).abs() < 1.0, "{fallback}");
    }

    /// Each stop of `members` reached along [`line_tree`] alone (before the
    /// loops were closed), with its distance from `stops[from]`.
    fn tree_metres(stops: &[StopRecord], members: &[usize], from: usize) -> BTreeMap<usize, f64> {
        let mut graph = LineGraph::new();
        for (a, b, d) in line_tree(stops, members) {
            graph.entry(a).or_default().push((b, d));
            graph.entry(b).or_default().push((a, d));
        }
        shortest(&graph, from, None)
    }

    /// The links [`line_graph`] adds to the tree, as sorted id pairs
    /// without the `940GZZLU` prefix.
    fn loop_links(stops: &[StopRecord]) -> Vec<(String, String)> {
        let members: Vec<usize> = (0..stops.len()).collect();
        let tree: Vec<(usize, usize)> = line_tree(stops, &members)
            .into_iter()
            .map(|(a, b, _)| (a.min(b), a.max(b)))
            .collect();
        let name = |i: usize| {
            stops[i]
                .summary
                .id
                .trim_start_matches("940GZZLU")
                .to_owned()
        };
        let mut links: Vec<(String, String)> = line_graph(stops, &members)
            .into_iter()
            .flat_map(|(a, next)| next.into_iter().map(move |(b, _)| (a, b)))
            .filter(|&(a, b)| a < b && !tree.contains(&(a, b)))
            .map(|(a, b)| {
                let (a, b) = (name(a), name(b));
                if a < b { (a, b) } else { (b, a) }
            })
            .collect();
        links.sort();
        links
    }

    fn pair(a: &str, b: &str) -> (String, String) {
        (a.to_owned(), b.to_owned())
    }

    #[test]
    fn the_fallback_closes_the_loops_the_tree_broke_and_no_more() {
        // The Northern: Angel to Old Street only. Not Chalk Farm to Kentish
        // Town (either side of Camden Town), nor Nine Elms to Oval.
        let (stops, _) = northern_wide();
        assert_eq!(loop_links(&stops), [pair("AGL", "ODS")]);
        // The Circle: King's Cross to Farringdon, where the tree cut the
        // loop; not Farringdon to Blackfriars, either side of the City's
        // bend.
        let (stops, _, _) = circle();
        assert_eq!(loop_links(&stops), [pair("FCC", "KSX")]);
        // Uxbridge's corner has no loop: Heathrow stays the long way round.
        let stops = network();
        let piccadilly: Vec<StopRecord> = stops
            .iter()
            .filter(|s| s.summary.lines.iter().any(|l| l.id == "piccadilly"))
            .cloned()
            .collect();
        assert_eq!(loop_links(&piccadilly), []);
        let heathrow = ride_metres(
            &stops,
            &LineRoutes::default(),
            "piccadilly",
            "940GUXB",
            "940GHRC",
        );
        assert!(heathrow > 25_000.0, "{heathrow}");
    }

    #[test]
    fn the_fallback_is_the_same_whatever_the_order_of_the_stops() {
        let (stops, _) = northern_wide();
        let by_id = |stops: &[StopRecord]| -> BTreeMap<(String, String), u32> {
            let l = line("northern");
            (0..stops.len())
                .flat_map(|a| {
                    rides(stops, &LineRoutes::default(), &l, a, Toward::Out)
                        .into_iter()
                        .map(move |(b, ride)| ((a, b), ride))
                })
                .map(|((a, b), ride)| {
                    (
                        (stops[a].summary.id.clone(), stops[b].summary.id.clone()),
                        metres(ride.metres),
                    )
                })
                .collect()
        };
        let mut reversed = stops.clone();
        reversed.reverse();
        let mut rotated = stops.clone();
        rotated.rotate_left(7);
        let expected = by_id(&stops);
        assert_eq!(by_id(&reversed), expected);
        assert_eq!(by_id(&rotated), expected);
    }

    /// Without routes, against the routes: no ride one train makes is over
    /// 1.7 times as far (the tree's own Waterloo link, Borough here, still
    /// sends Kennington to Waterloo round by Elephant & Castle), and none
    /// is shorter than the tree alone made it (no new shortcut).
    #[test]
    fn the_fallback_stays_near_the_routes_on_the_northern() {
        let (stops, rows) = northern_wide();
        let routes = LineRoutes::new(&stops, &rows);
        let l = line("northern");
        let members: Vec<usize> = (0..stops.len()).collect();
        let (mut over, mut checked) = (0, 0);
        for a in 0..stops.len() {
            let tree = tree_metres(&stops, &members, a);
            let fallback: BTreeMap<usize, Ride> =
                rides(&stops, &LineRoutes::default(), &l, a, Toward::Out)
                    .into_iter()
                    .collect();
            for (b, truth) in rides(&stops, &routes, &l, a, Toward::Out) {
                let ratio = fallback[&b].metres / truth.metres;
                assert!(
                    fallback[&b].metres >= truth.metres.min(tree[&b]) - 1.0,
                    "{a} to {b}: {ratio}"
                );
                if truth.change_at.is_none() {
                    checked += 1;
                    assert!(ratio < 1.7, "{a} to {b}: {ratio}");
                    if ratio > 1.5 {
                        over += 1;
                    }
                }
            }
        }
        // Kennington to Waterloo, Embankment and Charing Cross, both ways.
        assert!(over <= 8, "{over} of {checked}");
    }

    #[test]
    fn old_street_rides_to_euston_and_kings_cross_beside_its_hub_walk() {
        let (stops, rows) = northern();
        let routes = LineRoutes::new(&stops, &rows);
        let ods = index(&stops, "940GZZLUODS");
        for toward in [Toward::Out, Toward::In] {
            let candidates = access_candidates(&stops, &routes, ods, toward, &[]);
            let find = |crs: &str| candidates.iter().find(|c| c.crs == crs).unwrap();
            assert_eq!(candidates[0].crs, "OLD");
            assert_eq!(candidates[0].basis, AccessBasis::Hub);
            let euston = find("EUS");
            assert_eq!(euston.basis, AccessBasis::Line);
            assert_eq!(euston.line.as_deref(), Some("Northern"));
            assert_eq!(euston.stop_count, Some(3));
            assert_eq!(euston.change_at, None);
            // 5 in, 7 riding 3.4 km, 3 out.
            assert_eq!(euston.minutes, 15);
            assert_eq!(find("KGX").stop_count, Some(2));
            assert_eq!(find("STP").minutes, find("KGX").minutes);
            assert_eq!(find("MOG").stop_count, Some(1));
            // Each station once, quickest first.
            assert!(candidates.windows(2).all(|w| w[0].minutes <= w[1].minutes));
            let mut codes: Vec<&str> = candidates.iter().map(|c| c.crs.as_str()).collect();
            codes.sort_unstable();
            codes.dedup();
            assert_eq!(codes.len(), candidates.len());
        }
    }

    #[test]
    fn a_hub_stop_keeps_its_hub_walk_within_the_cap() {
        // A hub stop on a line with more hub stations, and more stations
        // within a walk, than the cap allows.
        let mut stops = vec![stop("940GHUB", &["l"], (51.5, 0.0), &["HUB"])];
        for i in 0..12_u8 {
            let at = (51.5 + 0.005 * f64::from(i + 1), 0.0);
            stops.push(stop(
                &format!("940G{i:03}"),
                &["l"],
                at,
                &[&format!("L{i:02}")],
            ));
        }
        let stations: Vec<(String, (f64, f64))> = (0..20_u8)
            .map(|i| (format!("W{i:02}"), (51.5, 0.0005 * f64::from(i))))
            .collect();
        let candidates =
            access_candidates(&stops, &LineRoutes::default(), 0, Toward::Out, &stations);
        assert_eq!(candidates.len(), MAX_CANDIDATES);
        let hub = candidates.iter().find(|c| c.crs == "HUB").unwrap();
        assert_eq!(hub.basis, AccessBasis::Hub);
        assert_eq!(hub.minutes, HUB_WALK_MINUTES);
    }

    #[test]
    fn a_station_end_with_a_stop_on_the_line_is_always_a_candidate() {
        let (stops, rows) = northern();
        let routes = LineRoutes::new(&stops, &rows);
        let ods = index(&stops, "940GZZLUODS");
        let eus = index(&stops, "940GZZLUEUS");
        // Euston to Old Street: the ride into Old Street from Euston's
        // own stop, with no walk out at the Euston end.
        let ride = station_ride(&stops, &routes, ods, Toward::In, "EUS").unwrap();
        assert_eq!(ride.crs, "EUS");
        assert_eq!(ride.mode, "TUBE");
        assert_eq!(ride.basis, AccessBasis::Line);
        assert_eq!(ride.line.as_deref(), Some("Northern"));
        assert_eq!(
            ride.station_stop.as_ref().map(|s| s.id.as_str()),
            Some("940GZZLUEUS")
        );
        assert_eq!(ride.minutes, 12, "5 in, 7 riding");
        assert_eq!(
            ride.minutes,
            direct_ride(&stops, &routes, eus, ods).unwrap().minutes
        );
        // It takes the place of the slower line candidate.
        let mut candidates = access_candidates(&stops, &routes, ods, Toward::In, &[]);
        merge_station_ride(&mut candidates, ride.clone());
        let euston: Vec<&AccessCandidate> = candidates.iter().filter(|c| c.crs == "EUS").collect();
        assert_eq!(euston, [&ride]);
        assert!(candidates.windows(2).all(|w| w[0].minutes <= w[1].minutes));
        // Even where the nearest-N cut has dropped the station.
        let mut cut: Vec<AccessCandidate> = candidates
            .iter()
            .filter(|c| c.crs != "EUS")
            .cloned()
            .collect();
        merge_station_ride(&mut cut, ride.clone());
        assert!(cut.contains(&ride));
        // Old Street to Euston, the other way.
        let out = station_ride(&stops, &routes, ods, Toward::Out, "EUS").unwrap();
        assert_eq!(out.minutes, 12);
        // Euston's own stop walks to Euston: no ride.
        assert!(station_ride(&stops, &routes, eus, Toward::In, "EUS").is_none());
        // A station with no stop of its own, or none on a shared line.
        assert!(station_ride(&stops, &routes, ods, Toward::In, "ZZZ").is_none());
        let (uxbridge, _) = branched();
        assert!(station_ride(&uxbridge, &LineRoutes::default(), 0, Toward::In, "EUS").is_none());
    }

    /// Euston to Old Street (into Old Street) and back (out of it), as
    /// [`station_ride`] gives them.
    fn euston_old_street(routes: &LineRoutes) -> [AccessCandidate; 2] {
        let (stops, _) = northern();
        let ods = index(&stops, "940GZZLUODS");
        [Toward::In, Toward::Out]
            .map(|toward| station_ride(&stops, routes, ods, toward, "EUS").unwrap())
    }

    #[test]
    fn without_routes_euston_old_street_is_still_the_short_way() {
        // Not round the loop by Charing Cross and Bank (8 km): the
        // fallback closes it between Angel and Old Street.
        for ride in euston_old_street(&LineRoutes::default()) {
            assert!(ride.distance_metres < 4_000, "{}", ride.distance_metres);
            assert!(ride.minutes <= 13, "{}", ride.minutes);
            assert_eq!(ride.change_at, None);
            assert_eq!(ride.stop_count, None);
        }
    }

    #[test]
    fn with_routes_euston_old_street_is_one_ride_of_three_stops() {
        let (stops, rows) = northern();
        for ride in euston_old_street(&LineRoutes::new(&stops, &rows)) {
            assert_eq!(ride.minutes, 12, "5 in, 7 riding");
            assert_eq!(ride.change_at, None);
            assert_eq!(ride.stop_count, Some(3));
        }
    }

    /// Every ride that changes, on every fixture with routes, either way:
    /// the change stop is neither end; a route reaches it from the start
    /// and the end from it, the second without passing back through the
    /// start; the ride is the two added up; and no route takes the start
    /// to the end without a change.
    #[test]
    fn a_change_is_between_the_ends_and_only_when_needed() {
        let (northern_stops, northern_rows) = northern_wide();
        let (branched_stops, branched_routes) = branched();
        let (circle_stops, _, circle_routes) = circle();
        let fixtures = [
            (
                northern_stops.clone(),
                LineRoutes::new(&northern_stops, &northern_rows),
                "northern",
            ),
            (branched_stops, branched_routes, "metropolitan"),
            (circle_stops, circle_routes, "circle"),
        ];
        let mut changes = 0;
        for (stops, routes, line_id) in &fixtures {
            let l = line(line_id);
            let speed_kmh = line_mode(&l).1;
            for toward in [Toward::Out, Toward::In] {
                let line_routes: Vec<Route> = routes.by_line[*line_id]
                    .iter()
                    .map(|r| match toward {
                        Toward::Out => r.clone(),
                        Toward::In => r.reversed(),
                    })
                    .collect();
                // `a` then `b` on one route, with nothing of `avoid` between.
                let one_ride = |a: usize, b: usize, avoid: Option<usize>| -> Option<f64> {
                    line_routes
                        .iter()
                        .flat_map(|r| {
                            r.positions(a).flat_map(move |i| {
                                r.positions(b).filter(move |j| *j > i).filter_map(move |j| {
                                    let passes = avoid.is_some_and(|x| r.stops[i..=j].contains(&x));
                                    (!passes).then(|| r.along[j] - r.along[i])
                                })
                            })
                        })
                        .reduce(f64::min)
                };
                for a in 0..stops.len() {
                    if routes.missing(line_id, a).is_some() {
                        // The tree's (which has the stop itself, at 0).
                        continue;
                    }
                    for (b, ride) in rides(stops, routes, &l, a, toward) {
                        assert_ne!(a, b);
                        let Some(j) = ride.change_at else {
                            continue;
                        };
                        changes += 1;
                        assert!(j != a && j != b, "{line_id}: {a} to {b} at {j}");
                        // DS-191: one train may go there too, but slower.
                        if let Some(one) = one_ride(a, b, None) {
                            let one = Ride {
                                metres: one,
                                stops: None,
                                change_at: None,
                            };
                            assert!(
                                ride.minutes(speed_kmh) < one.minutes(speed_kmh),
                                "{line_id}: {a} to {b}"
                            );
                        }
                        let first = one_ride(a, j, None).unwrap();
                        let second = one_ride(j, b, Some(a)).unwrap();
                        assert!((first + second - ride.metres).abs() < 1.0);
                    }
                }
            }
        }
        assert!(changes > 50, "{changes}");
    }

    #[test]
    fn missing_routes_are_told_apart_and_warned_of_rarely() {
        let (stops, rows) = northern();
        let ods = index(&stops, "940GZZLUODS");
        assert_eq!(
            LineRoutes::default().missing("northern", ods),
            Some(RoutesMissing::Line)
        );
        let routes = LineRoutes::new(&stops, &rows);
        assert_eq!(routes.missing("northern", ods), None);
        // Mornington Crescent is on the Charing Cross route only; a route
        // set without it leaves it to the tree.
        let bank_only = LineRoutes::new(&stops, &rows[..2]);
        let mtc = index(&stops, "940GZZLUMTC");
        assert_eq!(
            bank_only.missing("northern", mtc),
            Some(RoutesMissing::Stop)
        );
        assert_eq!(bank_only.missing("northern", ods), None);

        let last = Mutex::new(None);
        let start = Instant::now();
        let every = Duration::from_secs(60);
        assert!(warn_due(&last, start, every));
        assert!(!warn_due(&last, start + Duration::from_secs(59), every));
        assert!(warn_due(&last, start + every, every));
    }

    /// A hand-written Circle line (positions to about 100 m, from a map),
    /// as `TfL` runs it, both ways: from Paddington's Hammersmith & City
    /// stop (`PAH`) to Edgware Road, then once round the loop to Edgware
    /// Road again, where the trains terminate. Both Paddington stops are
    /// Paddington's (`PAD`).
    fn circle() -> (Vec<StopRecord>, Vec<String>, LineRoutes) {
        let stations = [
            ("PAH", (51.5190, -0.1790), "PAD"),
            ("ERC", (51.5199, -0.1679), ""),
            ("BST", (51.5226, -0.1571), ""),
            ("GPS", (51.5238, -0.1439), ""),
            ("ESQ", (51.5257, -0.1359), ""),
            ("KSX", (51.5308, -0.1238), "KGX"),
            ("FCC", (51.5203, -0.1053), "ZFD"),
            ("BBN", (51.5204, -0.0979), ""),
            ("MGT", (51.5186, -0.0886), "MOG"),
            ("LVT", (51.5178, -0.0823), "LST"),
            ("ALD", (51.5143, -0.0755), ""),
            ("TWH", (51.5098, -0.0766), ""),
            ("MMT", (51.5108, -0.0863), ""),
            ("CST", (51.5113, -0.0904), "CST"),
            ("MSH", (51.5122, -0.0940), ""),
            ("BKF", (51.5120, -0.1031), "BFR"),
            ("TMP", (51.5111, -0.1141), ""),
            ("EMB", (51.5074, -0.1223), ""),
            ("WSM", (51.5010, -0.1254), ""),
            ("SJP", (51.4994, -0.1335), ""),
            ("VIC", (51.4965, -0.1447), "VIC"),
            ("SSQ", (51.4924, -0.1565), ""),
            ("SKS", (51.4941, -0.1738), ""),
            ("GTR", (51.4945, -0.1829), ""),
            ("HSK", (51.5009, -0.1925), ""),
            ("NHG", (51.5094, -0.1967), ""),
            ("BWT", (51.5121, -0.1879), ""),
            ("PAC", (51.5154, -0.1755), "PAD"),
        ];
        let stops: Vec<StopRecord> = stations
            .iter()
            .map(|(id, at, crs)| {
                let rail: &[&str] = if crs.is_empty() { &[] } else { &[crs] };
                stop(&format!("940GZZLU{id}"), &["circle"], *at, rail)
            })
            .collect();
        let mut order: Vec<&str> = stations.iter().map(|(id, _, _)| *id).collect();
        order.push("ERC");
        let route = ids(&order);
        let routes = LineRoutes::new(&stops, &both_ways("circle", std::slice::from_ref(&route)));
        (stops, route, routes)
    }

    #[test]
    fn the_circle_rides_its_route_either_way() {
        let (stops, route, routes) = circle();
        let position = |id: &str| stops[index(&stops, id)].position.unwrap();
        let legs: Vec<f64> = route
            .windows(2)
            .map(|pair| metres_between(position(&pair[0]), position(&pair[1])))
            .collect();
        let at =
            |id: &String| -> Vec<usize> { (0..route.len()).filter(|i| route[*i] == *id).collect() };
        // Every pair: the shorter way along the route, either direction
        // (Edgware Road, at both ends of the loop, either side of it); a
        // way through Edgware Road changes trains, so is taken only when
        // quicker with the change (DS-191).
        let erc = index(&stops, "940GZZLUERC");
        let mut changes = 0;
        for a in &route {
            for b in route.iter().filter(|b| *b != a) {
                let expected = at(a)
                    .iter()
                    .flat_map(|i| at(b).into_iter().map(move |j| (*i, j)))
                    .map(|(i, j)| legs[i.min(j)..i.max(j)].iter().sum::<f64>())
                    .fold(f64::INFINITY, f64::min);
                let ride = rides(
                    &stops,
                    &routes,
                    &line("circle"),
                    index(&stops, a),
                    Toward::Out,
                )
                .into_iter()
                .find(|(i, _)| *i == index(&stops, b))
                .unwrap()
                .1;
                if ride.change_at.is_none() {
                    assert!(
                        (ride.metres - expected).abs() < 1.0,
                        "{a} to {b}: {} against {expected}",
                        ride.metres
                    );
                } else {
                    changes += 1;
                    assert_eq!(ride.change_at, Some(erc), "{a} to {b}");
                    let one = Ride {
                        metres: expected,
                        stops: None,
                        change_at: None,
                    };
                    assert!(ride.minutes(30.0) < one.minutes(30.0), "{a} to {b}");
                }
            }
        }
        assert!(changes > 0);
        // Edgware Road to Paddington is the short way, either Paddington.
        let short = ride_metres(&stops, &routes, "circle", "940GZZLUERC", "940GZZLUPAC");
        assert!(short < 1_000.0, "{short}");
        // Liverpool Street to Paddington (`PAD`) by King's Cross, to the
        // Hammersmith & City stop, not round by Victoria: about 7.5 km.
        let lvt = index(&stops, "940GZZLULVT");
        let ride = station_ride(&stops, &routes, lvt, Toward::Out, "PAD").unwrap();
        assert!(ride.distance_metres < 8_000, "{}", ride.distance_metres);
        assert_eq!(
            ride.station_stop.as_ref().map(|s| s.id.as_str()),
            Some("940GZZLUPAH")
        );
        assert_eq!(ride.change_at, None);
        // A stop with no station of its own rides a stop either way first.
        let barbican = index(&stops, "940GZZLUBBN");
        let candidates = access_candidates(&stops, &routes, barbican, Toward::Out, &[]);
        let mut first: Vec<&str> = candidates[..2].iter().map(|c| c.crs.as_str()).collect();
        first.sort_unstable();
        assert_eq!(first, ["MOG", "ZFD"]);
        assert!(candidates[..2].iter().all(|c| c.stop_count == Some(1)));
    }

    /// `(id suffix, lines, (latitude, longitude), station CRS or "", hub)`.
    type FixtureStop<'a> = (&'a str, &'a [&'a str], (f64, f64), &'a str, Option<&'a str>);

    /// DS-191: the Circle and the Hammersmith & City as `TfL` runs them,
    /// hand-written (positions to about 100 m, from a map), one route each
    /// way for each line. The Circle from Edgware Road (`ERC`) by
    /// Paddington's Circle stop (`PAC`) round by Victoria, Liverpool Street
    /// and King's Cross to Edgware Road again, then by Paddington's
    /// Hammersmith & City stop (`PAH`) to Hammersmith (`HSC`); the
    /// Hammersmith & City from Hammersmith by `PAH` and King's Cross to
    /// Liverpool Street. Each also the other way. `PAC` and `PAH`, about
    /// 250 m apart, are both Paddington's hub (`HUBPAD`, station `PAD`);
    /// Hammersmith's two stops, `HSC` and `HSD` (District and Piccadilly),
    /// share `HUBHMS`, which has no station.
    fn circle_both_ways() -> (Vec<StopRecord>, LineRoutes) {
        let (c, both) = (&["circle"][..], &["circle", "hammersmith-city"][..]);
        let stations: [FixtureStop<'_>; 38] = [
            ("ERC", both, (51.5199, -0.1679), "", None),
            (
                "PAC",
                &["circle", "district", "bakerloo"],
                (51.5166, -0.1757),
                "PAD",
                Some("HUBPAD"),
            ),
            ("BWT", c, (51.5121, -0.1879), "", None),
            ("NHG", c, (51.5094, -0.1967), "", None),
            ("HSK", c, (51.5009, -0.1925), "", None),
            ("GTR", c, (51.4945, -0.1829), "", None),
            ("SKS", c, (51.4941, -0.1738), "", None),
            ("SSQ", c, (51.4924, -0.1565), "", None),
            ("VIC", c, (51.4965, -0.1447), "VIC", None),
            ("SJP", c, (51.4994, -0.1335), "", None),
            ("WSM", c, (51.5010, -0.1254), "", None),
            ("EMB", c, (51.5074, -0.1223), "", None),
            ("TMP", c, (51.5111, -0.1141), "", None),
            ("BKF", c, (51.5120, -0.1031), "BFR", None),
            ("MSH", c, (51.5122, -0.0940), "", None),
            ("CST", c, (51.5113, -0.0904), "CST", None),
            ("MMT", c, (51.5108, -0.0863), "", None),
            ("TWH", c, (51.5098, -0.0766), "", None),
            ("ALD", c, (51.5143, -0.0755), "", None),
            ("LVT", both, (51.5178, -0.0823), "LST", None),
            ("MGT", both, (51.5186, -0.0886), "MOG", None),
            ("BBN", both, (51.5204, -0.0979), "", None),
            ("FCC", both, (51.5203, -0.1053), "ZFD", None),
            ("KSX", both, (51.5308, -0.1238), "KGX", None),
            ("ESQ", both, (51.5257, -0.1359), "", None),
            ("GPS", both, (51.5238, -0.1439), "", None),
            ("BST", both, (51.5226, -0.1571), "", None),
            ("PAH", both, (51.5182, -0.1783), "PAD", Some("HUBPAD")),
            ("RYO", both, (51.5191, -0.1880), "", None),
            ("WSP", both, (51.5210, -0.2011), "", None),
            ("LAD", both, (51.5172, -0.2107), "", None),
            ("LRD", both, (51.5139, -0.2172), "", None),
            ("WLA", both, (51.5098, -0.2244), "", None),
            ("SBM", both, (51.5058, -0.2265), "", None),
            ("GHK", both, (51.5018, -0.2267), "", None),
            ("HSC", both, (51.4934, -0.2250), "", Some("HUBHMS")),
            (
                "HSD",
                &["district", "piccadilly"],
                (51.4923, -0.2236),
                "",
                Some("HUBHMS"),
            ),
            // Not a sibling of anything: no hub.
            ("ERB", &["bakerloo"], (51.5203, -0.1702), "", None),
        ];
        let stops: Vec<StopRecord> = stations
            .iter()
            .map(|(id, lines, at, crs, hub)| {
                let rail: &[&str] = if crs.is_empty() { &[] } else { &[crs] };
                let mut stop = stop(&format!("940GZZLU{id}"), lines, *at, rail);
                stop.hub = hub.map(str::to_owned);
                stop
            })
            .collect();
        let west = [
            "PAH", "RYO", "WSP", "LAD", "LRD", "WLA", "SBM", "GHK", "HSC",
        ];
        let north = [
            "LVT", "MGT", "BBN", "FCC", "KSX", "ESQ", "GPS", "BST", "ERC",
        ];
        let circle_order: Vec<&str> = stations[..19]
            .iter()
            .map(|(id, ..)| *id)
            .chain(north)
            .chain(west)
            .collect();
        let hc_order: Vec<&str> = north.iter().chain(&west).copied().collect();
        let mut rows = both_ways("circle", &[ids(&circle_order)]);
        rows.extend(both_ways("hammersmith-city", &[ids(&hc_order)]));
        let routes = LineRoutes::new(&stops, &rows);
        (stops, routes)
    }

    /// The one ride from `a` to `b` on `line_id` that [`rides`] gives.
    fn ride_on(stops: &[StopRecord], routes: &LineRoutes, line_id: &str, a: &str, b: &str) -> Ride {
        rides(stops, routes, &line(line_id), index(stops, a), Toward::Out)
            .into_iter()
            .find(|(i, _)| *i == index(stops, b))
            .unwrap()
            .1
    }

    /// [`circle_both_ways`] with no stop a sibling of another.
    fn without_hubs(mut stops: Vec<StopRecord>) -> Vec<StopRecord> {
        for stop in &mut stops {
            stop.hub = None;
        }
        stops
    }

    fn id_of(stop: Option<&TflStopSummary>) -> Option<&str> {
        stop.map(|s| s.id.as_str())
    }

    #[test]
    fn change_beats_the_long_way_round() {
        let (stops, routes) = circle_both_ways();
        let (pac, erc) = ("940GZZLUPAC", index(&stops, "940GZZLUERC"));
        // One Circle train from Paddington runs round by Victoria to
        // Liverpool Street, about 13 km: to Edgware Road and on is 7 km.
        let lvt = ride_on(&stops, &routes, "circle", pac, "940GZZLULVT");
        assert_eq!(lvt.change_at, Some(erc), "{lvt:?}");
        assert!((6_500.0..7_500.0).contains(&lvt.metres), "{lvt:?}");
        let ksx = ride_on(&stops, &routes, "circle", pac, "940GZZLUKSX");
        assert_eq!(ksx.change_at, Some(erc), "{ksx:?}");
        // Without the other Paddington stop: 5 in, 15 riding, 10 for the
        // change, against 31 by the one train.
        let alone = without_hubs(stops.clone());
        let from = index(&alone, pac);
        let ride = direct_ride(&alone, &routes, from, index(&alone, "940GZZLULVT")).unwrap();
        assert_eq!(id_of(ride.change_at.as_ref()), Some("940GZZLUERC"));
        assert_eq!(ride.board_stop, None);
        assert_eq!(ride.minutes, 30, "{ride:?}");
        let ride = direct_ride(&alone, &routes, from, index(&alone, "940GZZLUKSX")).unwrap();
        assert_eq!(id_of(ride.change_at.as_ref()), Some("940GZZLUERC"));
        assert!(ride.minutes <= 25, "{ride:?}");
    }

    #[test]
    fn no_change_kept_when_quicker() {
        let (stops, routes) = circle_both_ways();
        // Paddington to Victoria: the one train round the south side.
        let vic = ride_on(&stops, &routes, "circle", "940GZZLUPAC", "940GZZLUVIC");
        assert_eq!(vic.change_at, None);
        assert_eq!(vic.stops, Some(7));
        // Hammersmith to Liverpool Street: one train all the way, about
        // 31 minutes, not changed anywhere.
        let hsc = index(&stops, "940GZZLUHSC");
        let ride = direct_ride(&stops, &routes, hsc, index(&stops, "940GZZLULVT")).unwrap();
        assert_eq!(ride.change_at, None);
        assert_eq!(ride.board_stop, None);
        assert!((29..=32).contains(&ride.minutes), "{ride:?}");
        // Edgware Road to Moorgate and back: one train, 18 minutes.
        let (erc, mgt) = (index(&stops, "940GZZLUERC"), index(&stops, "940GZZLUMGT"));
        for (a, b) in [(erc, mgt), (mgt, erc)] {
            let ride = direct_ride(&stops, &routes, a, b).unwrap();
            assert_eq!(ride.change_at, None, "{ride:?}");
            assert_eq!(ride.minutes, 18, "{ride:?}");
        }
    }

    /// Every ride on the fixture's lines, either way: one with a change
    /// is taken exactly where it is quicker than every one-train ride; a
    /// tie (there are some) keeps the one train.
    #[test]
    fn tie_prefers_no_change() {
        let (stops, routes) = circle_both_ways();
        let mut ties = 0;
        for line_id in ["circle", "hammersmith-city"] {
            let l = line(line_id);
            for toward in [Toward::Out, Toward::In] {
                let line_routes: Vec<Route> = routes.by_line[line_id]
                    .iter()
                    .map(|r| match toward {
                        Toward::Out => r.clone(),
                        Toward::In => r.reversed(),
                    })
                    .collect();
                let one_train = |a: usize, b: usize| -> Option<f64> {
                    line_routes
                        .iter()
                        .flat_map(|r| {
                            r.positions(a).flat_map(move |i| {
                                r.positions(b)
                                    .filter(move |j| *j > i)
                                    .map(move |j| r.along[j] - r.along[i])
                            })
                        })
                        .reduce(f64::min)
                };
                for a in 0..stops.len() {
                    if routes.missing(line_id, a).is_some() {
                        continue;
                    }
                    let reached = rides(&stops, &routes, &l, a, toward);
                    // Every changed ride, before the comparison: each
                    // junction reached by one train, then one train on.
                    for (b, ride) in reached {
                        let Some(one) = one_train(a, b) else {
                            assert!(ride.change_at.is_some());
                            continue;
                        };
                        let one = Ride {
                            metres: one,
                            stops: None,
                            change_at: None,
                        }
                        .minutes(30.0);
                        let best_change = (0..stops.len())
                            .filter(|&j| j != a && j != b)
                            .filter_map(|j| Some(one_train(a, j)? + one_train(j, b)?))
                            .map(|metres| {
                                Ride {
                                    metres,
                                    stops: None,
                                    change_at: Some(0),
                                }
                                .minutes(30.0)
                            })
                            .min();
                        if best_change == Some(one) {
                            ties += 1;
                        }
                        match ride.change_at {
                            Some(_) => assert!(ride.minutes(30.0) < one, "{a} to {b}"),
                            None => assert!(best_change.is_none_or(|c| c >= one), "{a} to {b}"),
                        }
                    }
                }
            }
        }
        assert!(ties > 0, "no tie to test");
    }

    #[test]
    fn second_train_never_back_through_from() {
        let (stops, routes) = circle_both_ways();
        let erc = index(&stops, "940GZZLUERC");
        for line_id in ["circle", "hammersmith-city"] {
            let l = line(line_id);
            let line_routes = &routes.by_line[line_id];
            for a in 0..stops.len() {
                if routes.missing(line_id, a).is_some() {
                    continue;
                }
                for (b, ride) in rides(&stops, &routes, &l, a, Toward::Out) {
                    let Some(j) = ride.change_at else {
                        continue;
                    };
                    assert!(j != a && j != b);
                    // Some route takes `j` to `b` without passing `a`, at
                    // the ride's length less the first train's.
                    let clear = line_routes.iter().any(|r| {
                        r.positions(j).any(|i| {
                            r.positions(b)
                                .filter(|k| *k > i)
                                .any(|k| !r.stops[i..=k].contains(&a))
                        })
                    });
                    assert!(clear, "{a} to {b} at {j}");
                }
            }
        }
        // From Paddington the inbound Circle at Edgware Road runs straight
        // back through Paddington: Bayswater stays one train from there.
        let bwt = ride_on(&stops, &routes, "circle", "940GZZLUPAC", "940GZZLUBWT");
        assert_eq!(bwt.change_at, None);
        assert_eq!(bwt.stops, Some(1));
        let ride = ride_on(&stops, &routes, "circle", "940GZZLUPAC", "940GZZLULVT");
        assert_eq!(ride.change_at, Some(erc));
    }

    #[test]
    fn sibling_stop_area_rides_its_lines() {
        let (stops, routes) = circle_both_ways();
        let (pac, paddington_hc) = (index(&stops, "940GZZLUPAC"), index(&stops, "940GZZLUPAH"));
        assert_eq!(with_siblings(&stops, pac), [(pac, 0), (paddington_hc, 5)]);
        // Paddington to Liverpool Street: walk 5 to the Hammersmith & City
        // stop, 5 in, 15 riding; no change.
        let lvt = index(&stops, "940GZZLULVT");
        let ride = direct_ride(&stops, &routes, pac, lvt).unwrap();
        assert_eq!(id_of(ride.board_stop.as_ref()), Some("940GZZLUPAH"));
        assert_eq!(ride.alight_stop, None);
        assert_eq!(ride.change_at, None);
        assert_eq!(ride.crs, "940GZZLULVT");
        assert!(ride.minutes <= 25, "{ride:?}");
        // King's Cross: about 19.
        let ksx = direct_ride(&stops, &routes, pac, index(&stops, "940GZZLUKSX")).unwrap();
        assert_eq!(id_of(ksx.board_stop.as_ref()), Some("940GZZLUPAH"));
        assert!((18..=20).contains(&ksx.minutes), "{ksx:?}");
        // And back, into Paddington: left at the other stop.
        let back = direct_ride(&stops, &routes, lvt, pac).unwrap();
        assert_eq!(id_of(back.alight_stop.as_ref()), Some("940GZZLUPAH"));
        assert_eq!(back.board_stop, None);
        assert!(back.minutes <= 25, "{back:?}");
        // The Hammersmith pair: the District stop rides the Circle.
        let hsd = index(&stops, "940GZZLUHSD");
        let ride = direct_ride(&stops, &routes, hsd, lvt).unwrap();
        assert_eq!(id_of(ride.board_stop.as_ref()), Some("940GZZLUHSC"));
        // Paddington's candidates: Liverpool Street from the other stop,
        // out of it and into it.
        for toward in [Toward::Out, Toward::In] {
            let candidates = access_candidates(&stops, &routes, pac, toward, &[]);
            let to_liverpool = candidates.iter().find(|c| c.crs == "LST").unwrap();
            let sibling = match toward {
                Toward::Out => &to_liverpool.board_stop,
                Toward::In => &to_liverpool.alight_stop,
            };
            assert_eq!(
                id_of(sibling.as_ref()),
                Some("940GZZLUPAH"),
                "{to_liverpool:?}"
            );
            assert_eq!(
                id_of(to_liverpool.station_stop.as_ref()),
                Some("940GZZLULVT")
            );
            assert_eq!(to_liverpool.change_at, None);
            assert!(to_liverpool.minutes <= 28, "{to_liverpool:?}");
        }
        // A station ride to Liverpool Street's own stop, from either.
        let ride = station_ride(&stops, &routes, pac, Toward::Out, "LST").unwrap();
        assert_eq!(id_of(ride.board_stop.as_ref()), Some("940GZZLUPAH"));
        assert!(ride.minutes <= 25, "{ride:?}");
        // The two Paddington stops are one station: no ride between them.
        assert!(direct_ride(&stops, &routes, pac, paddington_hc).is_none());
    }

    #[test]
    fn distant_same_hub_stops_not_siblings() {
        let (mut stops, routes) = circle_both_ways();
        let (pac, paddington_hc) = (index(&stops, "940GZZLUPAC"), index(&stops, "940GZZLUPAH"));
        // Edgware Road's Bakerloo stop, 180 m from its Circle stop, has no
        // hub: not a sibling.
        let erc = index(&stops, "940GZZLUERC");
        assert_eq!(with_siblings(&stops, erc), [(erc, 0)]);
        // Paddington's other stop moved 600 m off: one hub, not one station.
        stops[paddington_hc].position = Some((51.5220, -0.1830));
        assert!(
            metres_between(
                stops[pac].position.unwrap(),
                stops[paddington_hc].position.unwrap()
            ) > 400.0
        );
        assert_eq!(with_siblings(&stops, pac), [(pac, 0)]);
        let ride = direct_ride(&stops, &routes, pac, index(&stops, "940GZZLULVT")).unwrap();
        assert_eq!(ride.board_stop, None);
        assert_eq!(id_of(ride.change_at.as_ref()), Some("940GZZLUERC"));
        assert_eq!(ride.minutes, 30, "{ride:?}");
        // And a stop of another hub close by is not one either.
        stops[paddington_hc].position = Some((51.5182, -0.1783));
        stops[paddington_hc].hub = Some("HUBOTHER".to_owned());
        assert_eq!(with_siblings(&stops, pac), [(pac, 0)]);
    }

    /// The Piccadilly as `TfL` runs it (DS-151), hand-written on
    /// [`network`]'s positions: from a central stop (`HSD`, Hammersmith)
    /// through Acton Town to Uxbridge, to Heathrow Terminal 4, and to
    /// Heathrow Terminals 2 and 3 and on to Terminal 5, each both ways. No
    /// route has both Uxbridge and Heathrow.
    fn piccadilly() -> (Vec<StopRecord>, LineRoutes) {
        let mut stops = network();
        let p = &["piccadilly"];
        stops.push(stop("940GHSD", p, (51.4923, -0.2236), &[]));
        stops.push(stop("940GHR4", p, (51.4585, -0.4458), &["HAF"]));
        stops.push(stop("940GHR5", p, (51.4723, -0.4901), &["HWV"]));
        let route =
            |ids: &[&str]| -> Vec<String> { ids.iter().map(|id| format!("940G{id}")).collect() };
        let rows = both_ways(
            "piccadilly",
            &[
                route(&["HSD", "ACT", "ALP", "SHA", "RLN", "RUI", "ICK", "UXB"]),
                route(&["HSD", "ACT", "HNE", "HTX", "HR4"]),
                route(&["HSD", "ACT", "HNE", "HTX", "HRC", "HR5"]),
            ],
        );
        let routes = LineRoutes::new(&stops, &rows);
        (stops, routes)
    }

    #[test]
    fn uxbridge_to_heathrow_changes_at_acton_town() {
        let (stops, routes) = piccadilly();
        let uxb = index(&stops, "940GUXB");
        for heathrow in ["940GHRC", "940GHR4", "940GHR5"] {
            let to = index(&stops, heathrow);
            for (a, b) in [(uxb, to), (to, uxb)] {
                let ride = direct_ride(&stops, &routes, a, b).unwrap();
                assert_eq!(ride.line.as_deref(), Some("Piccadilly"), "{heathrow}");
                assert_eq!(
                    id_of(ride.change_at.as_ref()),
                    Some("940GACT"),
                    "{heathrow}: {ride:?}"
                );
                assert!(ride.distance_metres > 25_000, "{ride:?}");
            }
        }
        // As a candidate (Heathrow's station from Uxbridge), the same.
        let ride = station_ride(&stops, &routes, uxb, Toward::Out, "HXX").unwrap();
        assert_eq!(id_of(ride.change_at.as_ref()), Some("940GACT"));
    }
}
