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
//! **Candidates**, in order (the first rule that finds any wins):
//!
//! 1. **Hub**: the stop's own National Rail station: its `tfl_stop_crs`
//!    crosswalk, or a [`tfl_stops::CRS_OVERRIDES`] entry naming it. A walk
//!    of [`HUB_WALK_MINUTES`].
//! 2. **Line** and **walk**: the hub stations (rule 1 there) of the nearest
//!    [`LINE_CANDIDATES_PER_LINE`] such stops along each of the stop's own
//!    lines, by riding it, within [`RIDE_MAX_MINUTES`]; plus every station
//!    within [`WALK_MAX_METRES`] in a straight line, on foot. At most
//!    [`MAX_CANDIDATES`] in all, quickest first.
//! 3. **Nearest**: the [`NEAREST_COUNT`] nearest stations within
//!    [`NEAREST_MAX_METRES`], on foot.
//!
//! **Estimates.** DS has no `TfL` timetables or line sequences, so each time
//! is a conservative model, documented here and echoed in the response:
//!
//! - A ride is [`RIDE_ENTRY_MINUTES`] (walking in and the average wait)
//!   plus the distance along the line at the line's speed ([`line_mode`]:
//!   Tube 30 km/h, DLR 25, tram 18; `TfL` quotes about 33 km/h for the
//!   Underground, stops included) plus [`RIDE_EXIT_MINUTES`] (from the
//!   platform to the National Rail station), as a curated link's minutes
//!   include the walks at both ends (`reference-data/
//!   supplementary-fixed-links.csv`). The planner still adds the station's
//!   minimum change time before the train, as it does after any link.
//! - The distance along a line follows a minimum spanning tree of the
//!   line's stops by straight-line distance ([`line_tree`]). For a line
//!   with branches that is close to the track: Uxbridge to Heathrow on the
//!   Piccadilly line runs in via Acton Town, not across country. Rides
//!   stay on one line; no change between lines is modelled.
//! - A walk is the straight-line distance times [`WALK_DETOUR`], at
//!   4.5 km/h ([`WALK_METRES_PER_MINUTE`]).
//! - Rides run in [`SERVICE_WINDOWS`] only (about 05:30 to 00:30; the Night
//!   Tube is not modelled); walks at any time.

use std::collections::HashMap;

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
    clippy::cast_sign_loss,
    reason = "distances and minutes here are small and positive"
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
    let mut inside = vec![first];
    let mut outside: Vec<usize> = rest.to_vec();
    // For each stop outside, its nearest stop inside so far.
    let mut nearest: Vec<(usize, f64)> = outside
        .iter()
        .map(|&o| (first, metres_between(position(first), position(o))))
        .collect();
    while !outside.is_empty() {
        let (pick, _) = nearest
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map(|(index, best)| (index, *best))
            .unwrap_or((0, (first, 0.0)));
        let added = outside.swap_remove(pick);
        let (from, distance) = nearest.swap_remove(pick);
        edges.push((from, added, distance));
        inside.push(added);
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
        })
        .collect()
}

/// Rides from `stops[from]` on each of its lines to the nearest
/// [`LINE_CANDIDATES_PER_LINE`] stops with a hub station: rule 2's line
/// half. A station may come twice (two lines); the caller keeps the
/// quicker.
fn line_candidates(stops: &[StopRecord], from: usize) -> Vec<AccessCandidate> {
    let mut out: Vec<AccessCandidate> = Vec::new();
    if stops[from].position.is_none() {
        return out;
    }
    for line in &stops[from].summary.lines {
        let (mode, speed_kmh) = line_mode(line);
        // The nearest stops along the line with a hub station, each with
        // every station of its hub (King's Cross St. Pancras: KGX, STP...).
        let mut reached: Vec<(usize, f64)> = along_line(stops, from, line)
            .into_iter()
            .filter(|(index, _)| *index != from && !stops[*index].rail_crs.is_empty())
            .collect();
        reached.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for (index, distance) in reached.into_iter().take(LINE_CANDIDATES_PER_LINE) {
            let minutes = RIDE_ENTRY_MINUTES
                + whole(distance / 1000.0 / speed_kmh * 60.0)
                + RIDE_EXIT_MINUTES;
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
                    distance_metres: metres(distance),
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
        })
        .collect()
}

/// The access candidates of `stops[from]` (see the module doc), quickest
/// first, one per station. `stations` are National Rail stations with
/// their positions (only those within [`NEAREST_MAX_METRES`] matter).
pub fn access_candidates(
    stops: &[StopRecord],
    from: usize,
    stations: &[(String, (f64, f64))],
) -> Vec<AccessCandidate> {
    let hub = hub_candidates(&stops[from]);
    if !hub.is_empty() {
        return hub;
    }
    let mut out = line_candidates(stops, from);
    if let Some(position) = stops[from].position {
        out.extend(walk_candidates(
            stations,
            position,
            WALK_MAX_METRES,
            AccessBasis::Walk,
        ));
    }
    out.sort_by(|a, b| (a.minutes, &a.crs).cmp(&(b.minutes, &b.crs)));
    let mut seen: Vec<String> = Vec::new();
    out.retain(|candidate| {
        let new = !seen.contains(&candidate.crs);
        seen.push(candidate.crs.clone());
        new
    });
    out.truncate(MAX_CANDIDATES);
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
pub fn direct_ride(stops: &[StopRecord], from: usize, to: usize) -> Option<AccessCandidate> {
    stops[from].position?;
    stops[from]
        .summary
        .lines
        .iter()
        .filter_map(|line| {
            let (_, distance) = along_line(stops, from, line)
                .into_iter()
                .find(|(index, _)| *index == to)?;
            let (mode, speed_kmh) = line_mode(line);
            // No platform-to-station walk at the far end: the traveller
            // stays at the Tube stop.
            let minutes = RIDE_ENTRY_MINUTES + whole(distance / 1000.0 / speed_kmh * 60.0);
            (minutes <= DIRECT_RIDE_MAX_MINUTES).then(|| AccessCandidate {
                crs: stops[to].summary.id.clone(),
                mode: mode.to_owned(),
                minutes,
                basis: AccessBasis::Line,
                line: Some(line.name.clone()),
                station_stop: None,
                distance_metres: metres(distance),
            })
        })
        .min_by_key(|candidate| candidate.minutes)
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

/// The access candidates of `origin` and `destination`, each a stop id or
/// `None` for a station end. `Ok(Err(..))` for an unknown stop or one with
/// no candidate at all.
pub async fn load_access(
    pool: &PgPool,
    origin: Option<&str>,
    destination: Option<&str>,
) -> Result<Result<AccessEnds, AccessError>> {
    if origin.is_none() && destination.is_none() {
        return Ok(Ok(AccessEnds::default()));
    }
    let stops = load_stops(pool).await?;
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
        let candidates = access_candidates(&stops, index, &stations);
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
        [Some(from), Some(to)] => direct_ride(&stops, from, to),
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
        let candidates = access_candidates(&stops, 0, &[]);
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
    fn a_hub_stop_walks_to_its_own_station_only() {
        let stops = network();
        let candidates = access_candidates(&stops, 5, &[]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].crs, "HOH");
        assert_eq!(candidates[0].mode, "WALK");
        assert_eq!(candidates[0].minutes, HUB_WALK_MINUTES);
        assert_eq!(candidates[0].basis, AccessBasis::Hub);
    }

    #[test]
    fn a_station_within_a_kilometre_is_a_walk_beside_the_rides() {
        let stops = network();
        // West Ruislip, about 600 m from the Ruislip stop: 750 m on foot at
        // 75 m a minute, 10.02 minutes, so 11.
        let stations = vec![("WRU".to_owned(), (51.5698, -0.4296))];
        let candidates = access_candidates(&stops, 2, &stations);
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
        let candidates = access_candidates(&stops, stops.len() - 1, &stations);
        let codes: Vec<&str> = candidates.iter().map(|c| c.crs.as_str()).collect();
        assert_eq!(codes, ["AAA", "BBB", "CCC"]);
        assert!(candidates.iter().all(|c| c.basis == AccessBasis::Nearest));
        assert_eq!(candidates[0].minutes, walk_minutes(1501.0));
    }

    #[test]
    fn two_stops_on_one_line_ride_straight_there() {
        let stops = network();
        let ride = direct_ride(&stops, 0, 3).unwrap();
        assert_eq!(ride.crs, "940GRLN");
        assert_eq!(ride.mode, "TUBE");
        assert_eq!(ride.basis, AccessBasis::Line);
        // Only the Metropolitan reaches Harrow-on-the-Hill.
        let ride = direct_ride(&stops, 0, 5).unwrap();
        assert_eq!(ride.line.as_deref(), Some("Metropolitan"));
        // West Harrow and South Harrow share no line.
        assert!(direct_ride(&stops, 4, 6).is_none());
    }

    #[test]
    fn dlr_and_tram_rides_are_metro_at_their_own_speeds() {
        assert_eq!(line_mode(&line("dlr")), ("METRO", 25.0));
        assert_eq!(line_mode(&line("tram")), ("METRO", 18.0));
        assert_eq!(line_mode(&line("victoria")), ("TUBE", 30.0));
    }
}
