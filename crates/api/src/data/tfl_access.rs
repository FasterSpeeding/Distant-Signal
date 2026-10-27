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
//!    lines, by riding it, within [`RIDE_MAX_MINUTES`]; plus every station
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
//!   order, is a ride with no change. Otherwise, when a route from the
//!   start and a route to the end share a stop, the ride changes trains
//!   there (the junction, the shared stop that makes the ride shortest:
//!   Harrow-on-the-Hill from Uxbridge to Rickmansworth), and costs
//!   [`BRANCH_CHANGE_MINUTES`] plus [`BRANCH_CHANGE_WAIT_MINUTES`] more. A
//!   ride needing two changes on one line is not offered. A line with no
//!   routes, or a stop no route of its line has, falls back to a minimum
//!   spanning tree of the line's stops by straight-line distance
//!   ([`line_tree`]), as before DS-151: close to the track, but blind to
//!   changes between branches. Rides stay on one line; no change between
//!   lines is modelled.
//! - A walk is the straight-line distance times [`WALK_DETOUR`], at
//!   4.5 km/h ([`WALK_METRES_PER_MINUTE`]).
//! - Rides run in [`SERVICE_WINDOWS`] only (about 05:30 to 00:30; the Night
//!   Tube is not modelled); walks at any time.

use std::collections::{BTreeMap, HashMap};

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

/// Every stop reachable from `stops[from]` along `line`'s tree, with its
/// distance along it.
fn along_line(stops: &[StopRecord], from: usize, line: &TflStopLine) -> Vec<(usize, f64)> {
    let members: Vec<usize> = stops
        .iter()
        .enumerate()
        .filter(|(_, stop)| {
            stop.position.is_some() && stop.summary.lines.iter().any(|l| l.id == line.id)
        })
        .map(|(index, _)| index)
        .collect();
    let mut neighbours: HashMap<usize, Vec<(usize, f64)>> = HashMap::new();
    for (a, b, d) in line_tree(stops, &members) {
        neighbours.entry(a).or_default().push((b, d));
        neighbours.entry(b).or_default().push((a, d));
    }
    // A tree has one path to each stop: a walk outwards finds it.
    let mut out = Vec::new();
    let mut seen = vec![from];
    let mut queue = vec![(from, 0.0)];
    while let Some((at, distance)) = queue.pop() {
        out.push((at, distance));
        for &(next, d) in neighbours.get(&at).map(Vec::as_slice).unwrap_or_default() {
            if !seen.contains(&next) {
                seen.push(next);
                queue.push((next, distance + d));
            }
        }
    }
    out
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
/// any that pass `from` (no change when one route has both stops in order,
/// else one change at the best shared stop, else none), on the spanning
/// tree for the rest (see the module doc).
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

    // One change, at a stop reached without one, onto a route on from it.
    let mut changed: BTreeMap<usize, Ride> = BTreeMap::new();
    for (&junction, first) in &best {
        for route in &line_routes {
            for k in route.positions(junction) {
                for m in k + 1..route.stops.len() {
                    let to = route.stops[m];
                    if to == from || best.contains_key(&to) {
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
    best.extend(changed);

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
        })
        .collect()
}

/// Rides from `stops[from]` on each of its lines (into it, for
/// [`Toward::In`]) to the quickest [`LINE_CANDIDATES_PER_LINE`] stops with
/// a hub station: rule 2's line half. A station may come twice (two
/// lines); the caller keeps the quicker.
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
    for line in &stops[from].summary.lines {
        let (mode, speed_kmh) = line_mode(line);
        // The quickest stops along the line with a hub station, each with
        // every station of its hub (King's Cross St. Pancras: KGX, STP...).
        let mut reached: Vec<(usize, Ride, i32)> = rides(stops, routes, line, from, toward)
            .into_iter()
            .filter(|(index, _)| *index != from && !stops[*index].rail_crs.is_empty())
            .map(|(index, ride)| {
                let minutes = RIDE_ENTRY_MINUTES + ride.minutes(speed_kmh) + RIDE_EXIT_MINUTES;
                (index, ride, minutes)
            })
            .collect();
        reached.sort_by(|a, b| {
            a.2.cmp(&b.2)
                .then(a.1.metres.total_cmp(&b.1.metres))
                .then(a.0.cmp(&b.0))
        });
        for (index, ride, minutes) in reached.into_iter().take(LINE_CANDIDATES_PER_LINE) {
            if minutes > RIDE_MAX_MINUTES {
                break;
            }
            for crs in &stops[index].rail_crs {
                out.push(AccessCandidate {
                    crs: crs.clone(),
                    mode: mode.to_owned(),
                    minutes,
                    basis: AccessBasis::Line,
                    line: Some(line.name.clone()),
                    station_stop: Some(stops[index].summary.clone()),
                    distance_metres: metres(ride.metres),
                    stop_count: stop_count(&ride),
                    change_at: ride.change_at.map(|at| stops[at].summary.clone()),
                });
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
/// the quickest, when there is one within [`DIRECT_RIDE_MAX_MINUTES`]: two stops
/// on one line need no train between them. Its `crs` is the destination
/// stop's id.
pub fn direct_ride(
    stops: &[StopRecord],
    routes: &LineRoutes,
    from: usize,
    to: usize,
) -> Option<AccessCandidate> {
    stops[from].position?;
    stops[from]
        .summary
        .lines
        .iter()
        .filter_map(|line| {
            let (_, ride) = rides(stops, routes, line, from, Toward::Out)
                .into_iter()
                .find(|(index, _)| *index == to)?;
            let (mode, speed_kmh) = line_mode(line);
            // No platform-to-station walk at the far end: the traveller
            // stays at the Tube stop.
            let minutes = RIDE_ENTRY_MINUTES + ride.minutes(speed_kmh);
            (minutes <= DIRECT_RIDE_MAX_MINUTES).then(|| AccessCandidate {
                crs: stops[to].summary.id.clone(),
                mode: mode.to_owned(),
                minutes,
                basis: AccessBasis::Line,
                line: Some(line.name.clone()),
                station_stop: None,
                distance_metres: metres(ride.metres),
                stop_count: stop_count(&ride),
                change_at: ride.change_at.map(|at| stops[at].summary.clone()),
            })
        })
        .min_by_key(|candidate| candidate.minutes)
}

/// When the plan's other end is the station `crs` and one of its own stops
/// (a stop whose hub stations, rule 1, include it: Euston's is
/// `940GZZLUEUS`) shares a line with `stops[from]`: that station as a
/// candidate, by the quickest such ride at [`direct_ride`]'s minutes (no
/// platform-to-station walk: the traveller's journey ends, or starts, at
/// the station). Rides out of `stops[from]` for [`Toward::Out`], into it
/// for [`Toward::In`]. The caller adds it whatever the cuts of rule 2, so
/// Euston to Old Street is always offered as the Northern line ride.
pub fn station_ride(
    stops: &[StopRecord],
    routes: &LineRoutes,
    from: usize,
    toward: Toward,
    crs: &str,
) -> Option<AccessCandidate> {
    stops[from].position?;
    let own: Vec<usize> = stops
        .iter()
        .enumerate()
        .filter(|(index, stop)| *index != from && stop.rail_crs.iter().any(|c| c == crs))
        .map(|(index, _)| index)
        .collect();
    if own.is_empty() || stops[from].rail_crs.iter().any(|c| c == crs) {
        // No stop of its own, or `stops[from]` is one: the hub walk.
        return None;
    }
    stops[from]
        .summary
        .lines
        .iter()
        .filter(|line| {
            own.iter()
                .any(|&s| stops[s].summary.lines.iter().any(|l| l.id == line.id))
        })
        .flat_map(|line| {
            let (mode, speed_kmh) = line_mode(line);
            rides(stops, routes, line, from, toward)
                .into_iter()
                .filter(|(index, _)| own.contains(index))
                .map(move |(index, ride)| (index, ride, line, mode, speed_kmh))
        })
        .filter_map(|(index, ride, line, mode, speed_kmh)| {
            let minutes = RIDE_ENTRY_MINUTES + ride.minutes(speed_kmh);
            (minutes <= DIRECT_RIDE_MAX_MINUTES).then(|| AccessCandidate {
                crs: crs.to_owned(),
                mode: mode.to_owned(),
                minutes,
                basis: AccessBasis::Line,
                line: Some(line.name.clone()),
                station_stop: Some(stops[index].summary.clone()),
                distance_metres: metres(ride.metres),
                stop_count: stop_count(&ride),
                change_at: ride.change_at.map(|at| stops[at].summary.clone()),
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
    latitude: Option<f64>,
    longitude: Option<f64>,
}

/// Every `tfl_stops` row (a few hundred) with its hub stations.
async fn load_stops(pool: &PgPool) -> Result<Vec<StopRecord>> {
    let rows: Vec<StopRow> = sqlx::query_as(
        "SELECT stop_id, name, modes, lines, latitude, longitude FROM tfl_stops ORDER BY stop_id",
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
        let tree = ride_metres(&stops, &LineRoutes::default(), "northern", ods, eus);
        assert!(tree > 2.0 * along, "{tree}");
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

    #[test]
    fn without_routes_the_station_ride_is_the_trees_but_still_offered() {
        let (stops, _) = northern();
        let ods = index(&stops, "940GZZLUODS");
        let ride = station_ride(&stops, &LineRoutes::default(), ods, Toward::In, "EUS").unwrap();
        // Round the loop's long way: slow, but one ride, and quicker than a
        // train to Moorgate and a ride back.
        assert!(ride.distance_metres > 7_000, "{}", ride.distance_metres);
        assert_eq!(ride.stop_count, None);
        assert!(ride.minutes < 30, "{}", ride.minutes);
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
        // way through Edgware Road would change trains, so is not taken
        // while one ride reaches the stop.
        for a in &route {
            for b in route.iter().filter(|b| *b != a) {
                let expected = at(a)
                    .iter()
                    .flat_map(|i| at(b).into_iter().map(move |j| (*i, j)))
                    .map(|(i, j)| legs[i.min(j)..i.max(j)].iter().sum::<f64>())
                    .fold(f64::INFINITY, f64::min);
                let along = ride_metres(&stops, &routes, "circle", a, b);
                assert!(
                    (along - expected).abs() < 1.0,
                    "{a} to {b}: {along} against {expected}"
                );
            }
        }
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
}
