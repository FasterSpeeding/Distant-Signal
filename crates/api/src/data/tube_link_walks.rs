//! The walks between a National Rail station and its Underground station
//! on a Tube fixed link (London Euston to Euston (Tube), Vauxhall to
//! Vauxhall (Tube)): what a `TUBE` hop is charged and how it is shown.
//!
//! **What a link's minutes hold.** RSPS5046 (§5.10.1.3, §5.11.1.3) gives a
//! fixed link's time as a transit time, "summated with the minimum
//! interchange times at the stations at either end of the link". The
//! planner charges the minimum change time at the far end only, before the
//! next train (`trip_planner::csa`, `raptor`), as it does after any link;
//! the curated links (`reference-data/supplementary-fixed-links.csv`) and
//! the `TfL` access rides (`tfl_access`) count the walks at both ends in
//! the link's own minutes. Most of the timetable's London Tube links do
//! too: Vauxhall to Euston is 22 minutes at the peak, the Victoria line
//! about 10 of them. The short ones don't: Euston to Victoria is 9 minutes
//! with 7 on the train, and Euston to King's Cross 5 with 1.4 on the
//! train, which leaves no time to walk to the Tube, wait, and walk out.
//!
//! **The floor** ([`raise_tube_links`]). A London Underground link (a
//! `TUBE` link, or a `TRANSFER` over a Tube pair: `planner_operators::
//! attribute_fixed_links`) between two stations whose Tube stations are
//! known is charged at least the walk in, [`tfl_access::RIDE_WAIT_MINUTES`]
//! for the train, the timetabled running time (`tfl_line_routes.
//! step_seconds`, the quickest one route with both stops in that order)
//! and the walk out. A link already that long is left alone, so nothing
//! is counted twice. So is one whose running time alone is longer than
//! the link: the timetable knows a quicker way than one route (Paddington
//! to Liverpool Street by the Hammersmith & City, not round the Circle),
//! so the floor would be wrong. A pair with no route, or no known running
//! time, keeps its link.
//!
//! **The walks** ([`walk_minutes`]) are the timetabled transit path's own
//! (`transit_planning::rail_link_walk_minutes`, a `transitWalk`'s
//! `walkMinutes`): the straight line from the station to its Tube station,
//! at the `TfL` access model's walking speed and detour, 2 to 15 minutes.
//! Each station's Tube station is the one a transfer leg names
//! (`tfl_stops::stops_for_crs`, the best `tube` stop).
//!
//! **The legs** ([`attach_station_walks`]). A Tube transfer leg naming
//! both its Tube stations carries `stationWalks` (`originMinutes`,
//! `destinationMinutes`), the walks its `minutes` include: the client shows
//! "Walk London Euston (National Rail) to Euston (Tube), 2 min" before the
//! ride and the other walk after it, and the ride as the rest.

use std::collections::{HashMap, HashSet};

use anyhow::Result;
use schedule_query::{InterchangeData, OperatorId};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::tfl_access::{self, RouteRow};
use crate::data::tfl_stops;
use crate::data::transit_planning::{metres_between, rail_link_walk_minutes};
use crate::data::trip_planning_itinerary::{PlannedLeg, SegmentResult};

/// A National Rail station's Tube station and the walk between them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StationTube {
    /// The Tube station's stop-area id (`940GZZLUEUS`).
    pub stop_id: String,
    pub walk_minutes: i32,
}

/// The walks a Tube transfer leg's `minutes` include, at each end
/// (additive, 2026-11-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StationWalks {
    /// From the National Rail station to the Tube station.
    pub origin_minutes: i32,
    /// From the Tube station to the National Rail station.
    pub destination_minutes: i32,
}

/// The walk between a station at `station` and its Tube station at `stop`
/// (each `(latitude, longitude)` when known): the timetabled transit
/// path's rail-link walk. `None` when they are more than
/// [`tfl_access::WALK_MAX_METRES`] apart in a straight line: no station's
/// own Tube station is that far, so one of the positions is wrong (prod's
/// Canary Wharf Elizabeth line station sits 2.4 km east of the Jubilee's),
/// and the link is better left as the timetable has it.
#[must_use]
pub fn walk_minutes(station: Option<(f64, f64)>, stop: Option<(f64, f64)>) -> Option<i32> {
    let distance = station.zip(stop).map(|(a, b)| metres_between(a, b));
    if distance.is_some_and(|metres| metres > tfl_access::WALK_MAX_METRES) {
        return None;
    }
    Some(rail_link_walk_minutes(distance, "interchange"))
}

/// The quickest timetabled run from `from` to `to` along one route of
/// `routes` with both in that order, in minutes; `None` when no route has
/// them so, or no such stretch has every step's time.
#[must_use]
pub fn running_minutes(routes: &[RouteRow], from: &str, to: &str) -> Option<f64> {
    let mut best: Option<i64> = None;
    for route in routes {
        let Some(steps) = route
            .step_seconds
            .as_ref()
            .filter(|steps| steps.len() + 1 == route.stop_ids.len())
        else {
            continue;
        };
        for (i, stop) in route.stop_ids.iter().enumerate() {
            if stop != from {
                continue;
            }
            let Some(offset) = route.stop_ids[i + 1..].iter().position(|s| s == to) else {
                continue;
            };
            let seconds: Option<i64> = steps[i..=i + offset]
                .iter()
                .map(|step| step.filter(|s| *s >= 0).map(i64::from))
                .sum();
            if let Some(seconds) = seconds
                && best.is_none_or(|known| seconds < known)
            {
                best = Some(seconds);
            }
        }
    }
    #[expect(clippy::cast_precision_loss, reason = "a few thousand seconds")]
    best.map(|seconds| seconds as f64 / 60.0)
}

/// The least a link from `from` to `to` (each a station's Tube station)
/// may take, and the running time it was worked out from: see the module
/// doc's "The floor".
fn floor_minutes(from: &StationTube, to: &StationTube, routes: &[RouteRow]) -> Option<(i32, f64)> {
    let running = running_minutes(routes, &from.stop_id, &to.stop_id)?;
    #[expect(clippy::cast_possible_truncation, reason = "minutes of a Tube ride")]
    let ride = running.ceil() as i32;
    Some((
        from.walk_minutes + tfl_access::RIDE_WAIT_MINUTES + ride + to.walk_minutes,
        running,
    ))
}

/// Whether `link` is a London Underground hop the floor applies to.
fn is_tube_hop(link: &schedule_query::FixedLink) -> bool {
    link.operator == OperatorId::LONDON_UNDERGROUND && !link.is_stand_in()
}

/// The stations a London Underground link leaves from or goes to.
#[must_use]
pub fn tube_link_stations(data: &InterchangeData) -> Vec<String> {
    let mut codes: HashSet<&str> = HashSet::new();
    for (from, links) in &data.fixed_links_from_crs {
        for link in links.iter().filter(|link| is_tube_hop(link)) {
            codes.insert(from);
            codes.insert(&link.to_crs);
        }
    }
    let mut codes: Vec<String> = codes.into_iter().map(str::to_owned).collect();
    codes.sort_unstable();
    codes
}

/// Raises every London Underground link of `data` to its floor (the
/// module doc's "The floor"), after `planner_operators::
/// attribute_fixed_links`; returns how many it raised. `tubes` is each
/// station's Tube station.
#[expect(
    clippy::implicit_hasher,
    reason = "the caller passes station_tubes' own map"
)]
pub fn raise_tube_links(
    data: &mut InterchangeData,
    tubes: &HashMap<String, StationTube>,
    routes: &[RouteRow],
) -> usize {
    let mut floors: HashMap<(String, String), Option<(i32, f64)>> = HashMap::new();
    let mut raised = 0;
    for (from, links) in &mut data.fixed_links_from_crs {
        for link in links.iter_mut().filter(|link| is_tube_hop(link)) {
            let floor = *floors
                .entry((from.clone(), link.to_crs.clone()))
                .or_insert_with(|| {
                    let (a, b) = (tubes.get(from)?, tubes.get(&link.to_crs)?);
                    floor_minutes(a, b, routes)
                });
            if let Some((floor, running)) = floor
                && running <= f64::from(link.minutes)
                && floor > link.minutes
            {
                link.minutes = floor;
                raised += 1;
            }
        }
    }
    if raised > 0 {
        // The shortest valid link first (`fixed_links_from_where`), as
        // the build left them.
        data.sort_fixed_links();
    }
    raised
}

/// Each of `crs_codes`' Tube station (the one a transfer leg names) and the
/// walk to it. A station with no Tube station is absent.
pub async fn station_tubes(
    pool: &PgPool,
    crs_codes: &[String],
) -> Result<HashMap<String, StationTube>> {
    let best: Vec<(String, String)> =
        tfl_stops::stops_for_crs(pool, crs_codes, &["tube".to_owned()])
            .await?
            .into_iter()
            .filter_map(|(crs, stops)| Some((crs, stops.into_iter().next()?.id)))
            .collect();
    let pairs: Vec<(&str, &str)> = best
        .iter()
        .map(|(crs, stop)| (crs.as_str(), stop.as_str()))
        .collect();
    let walks = pair_walks(pool, &pairs).await?;
    Ok(best
        .into_iter()
        .filter_map(|(crs, stop_id)| {
            let walk_minutes = *walks.get(&(crs.clone(), stop_id.clone()))?;
            Some((
                crs,
                StationTube {
                    stop_id,
                    walk_minutes,
                },
            ))
        })
        .collect())
}

/// The walk ([`walk_minutes`]) for each `(crs, Tube stop id)` of `pairs`
/// that has one.
async fn pair_walks(
    pool: &PgPool,
    pairs: &[(&str, &str)],
) -> Result<HashMap<(String, String), i32>> {
    let (codes, stops): (Vec<String>, Vec<String>) = pairs
        .iter()
        .map(|(crs, stop)| ((*crs).to_owned(), (*stop).to_owned()))
        .unzip();
    let stations: HashMap<String, (f64, f64)> = sqlx::query_as::<_, (String, f64, f64)>(
        "SELECT crs::text, latitude, longitude FROM stations \
         WHERE crs::text = ANY($1::text[]) AND latitude IS NOT NULL AND longitude IS NOT NULL",
    )
    .bind(&codes)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(crs, lat, lon)| (crs.trim().to_ascii_uppercase(), (lat, lon)))
    .collect();
    let positions: HashMap<String, (f64, f64)> = sqlx::query_as::<_, (String, f64, f64)>(
        "SELECT stop_id, latitude, longitude FROM tfl_stops \
         WHERE stop_id = ANY($1::text[]) AND latitude IS NOT NULL AND longitude IS NOT NULL",
    )
    .bind(&stops)
    .fetch_all(pool)
    .await?
    .into_iter()
    .map(|(id, lat, lon)| (id, (lat, lon)))
    .collect();
    Ok(pairs
        .iter()
        .filter_map(|(crs, stop)| {
            let crs = crs.trim().to_ascii_uppercase();
            let walk = walk_minutes(stations.get(&crs).copied(), positions.get(*stop).copied())?;
            Some(((crs, (*stop).to_owned()), walk))
        })
        .collect())
}

/// Raises `data`'s London Underground links to their floors (module doc),
/// reading each station's Tube station and the Tube's routes. Returns how
/// many it raised.
pub async fn apply(pool: &PgPool, data: &mut InterchangeData) -> Result<usize> {
    let codes = tube_link_stations(data);
    if codes.is_empty() {
        return Ok(0);
    }
    let tubes = station_tubes(pool, &codes).await?;
    let routes = tfl_access::load_routes(pool).await?;
    Ok(raise_tube_links(data, &tubes, &routes))
}

/// [`apply`], best effort: on a database error the links keep the
/// timetable's minutes, with a warning, and the plan is served as before.
pub async fn raise_or_warn(pool: &PgPool, data: &mut InterchangeData) {
    match apply(pool, data).await {
        Ok(raised) => tracing::debug!(raised, "raised short Tube links to their walks and ride"),
        Err(err) => {
            tracing::warn!(error = ?err, "could not read Tube stations for Tube links; keeping their minutes");
        }
    }
}

/// Whether a planned transfer is a Tube hop between two National Rail
/// stations, with both its Tube stations named: a `TUBE` link, or a
/// `TRANSFER` `tfl_stops::attach_transfer_stops` found Tube stations for
/// at both ends. Never an access ride or a walk to a transit stop.
fn walked_hop(leg: &PlannedLeg) -> Option<(&str, &str, &str, &str, i32)> {
    let PlannedLeg::Transfer {
        mode,
        origin_crs: Some(origin_crs),
        destination_crs: Some(destination_crs),
        origin_stop: Some(origin_stop),
        destination_stop: Some(destination_stop),
        access: None,
        transit_walk: None,
        estimated_walk: false,
        minutes,
        ..
    } = leg
    else {
        return None;
    };
    let tube = |stop: &tfl_stops::TflStopSummary| stop.modes.iter().any(|m| m == "tube");
    let hop = (mode.eq_ignore_ascii_case("TUBE") || mode.eq_ignore_ascii_case("TRANSFER"))
        && origin_crs != destination_crs
        && tube(origin_stop)
        && tube(destination_stop);
    hop.then_some((
        origin_crs.as_str(),
        origin_stop.id.as_str(),
        destination_crs.as_str(),
        destination_stop.id.as_str(),
        *minutes,
    ))
}

/// The walks for a leg of `minutes` whose two ends walk `origin` and
/// `destination` minutes: `None` when they would leave no time to ride.
#[must_use]
pub fn station_walks(minutes: i32, origin: i32, destination: i32) -> Option<StationWalks> {
    (origin + destination < minutes).then_some(StationWalks {
        origin_minutes: origin,
        destination_minutes: destination,
    })
}

/// Fills `stationWalks` on every Tube hop of `segments` naming both its
/// Tube stations (module doc, "The legs"). Run after
/// `tfl_stops::merge_transfer_rides`, so a ride through a station walks
/// only at its two ends. Best effort: on a database error the legs keep
/// no walks.
pub async fn attach_station_walks(pool: &PgPool, segments: &mut [SegmentResult]) {
    let mut pairs: Vec<(&str, &str)> = Vec::new();
    for leg in segments
        .iter()
        .flat_map(|segment| &segment.itineraries)
        .flat_map(|itinerary| &itinerary.legs)
    {
        if let Some((origin_crs, origin_stop, destination_crs, destination_stop, _)) =
            walked_hop(leg)
        {
            pairs.push((origin_crs, origin_stop));
            pairs.push((destination_crs, destination_stop));
        }
    }
    if pairs.is_empty() {
        return;
    }
    pairs.sort_unstable();
    pairs.dedup();
    let walks = match pair_walks(pool, &pairs).await {
        Ok(walks) => walks,
        Err(err) => {
            tracing::warn!(error = ?err, "station walks for Tube transfers failed; serving them without");
            return;
        }
    };
    for leg in segments
        .iter_mut()
        .flat_map(|segment| &mut segment.itineraries)
        .flat_map(|itinerary| &mut itinerary.legs)
    {
        let Some((origin_crs, origin_stop, destination_crs, destination_stop, minutes)) =
            walked_hop(leg)
        else {
            continue;
        };
        let walk = |crs: &str, stop: &str| {
            walks
                .get(&(crs.trim().to_ascii_uppercase(), stop.to_owned()))
                .copied()
        };
        let found = walk(origin_crs, origin_stop)
            .zip(walk(destination_crs, destination_stop))
            .and_then(|(origin, destination)| station_walks(minutes, origin, destination));
        if let PlannedLeg::Transfer {
            station_walks: slot,
            ..
        } = leg
        {
            *slot = found;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use schedule_query::FixedLink;

    fn route(stop_ids: &[&str], step_seconds: Option<Vec<Option<i32>>>) -> RouteRow {
        RouteRow {
            line_id: "victoria".to_owned(),
            stop_ids: stop_ids.iter().map(|s| (*s).to_owned()).collect(),
            step_seconds,
        }
    }

    /// Brixton to Walthamstow, cut down: Vauxhall, Pimlico, Victoria,
    /// Green Park, Oxford Circus, Warren Street, Euston (seconds between).
    fn victoria_line() -> Vec<RouteRow> {
        let north = ["VXL", "PCO", "VIC", "GPK", "OXC", "WRR", "EUS"];
        let steps = [90, 90, 120, 90, 90, 120];
        vec![
            route(&north, Some(steps.iter().map(|s| Some(*s)).collect())),
            route(
                &north.iter().rev().copied().collect::<Vec<_>>(),
                Some(steps.iter().rev().map(|s| Some(*s)).collect()),
            ),
        ]
    }

    fn tube(stop: &str, walk: i32) -> StationTube {
        StationTube {
            stop_id: stop.to_owned(),
            walk_minutes: walk,
        }
    }

    fn tubes() -> HashMap<String, StationTube> {
        HashMap::from([
            ("VXH".to_owned(), tube("VXL", 2)),
            ("VIC".to_owned(), tube("VIC", 3)),
            ("EUS".to_owned(), tube("EUS", 2)),
        ])
    }

    fn link(mode: &str, to: &str, minutes: i32, operator: OperatorId) -> FixedLink {
        FixedLink {
            mode: mode.to_owned(),
            to_crs: to.to_owned(),
            minutes,
            valid_from: "0000".to_owned(),
            valid_to: "2359".to_owned(),
            days_mask: "1111111".to_owned(),
            operator,
            only_without: OperatorId::UNKNOWN,
        }
    }

    fn minutes(data: &InterchangeData, from: &str, to: &str) -> Vec<i32> {
        data.fixed_links_from_crs[from]
            .iter()
            .filter(|l| l.to_crs == to)
            .map(|l| l.minutes)
            .collect()
    }

    #[test]
    fn running_minutes_sums_one_route_in_order() {
        let routes = victoria_line();
        assert_eq!(running_minutes(&routes, "EUS", "VIC"), Some(7.0));
        assert_eq!(running_minutes(&routes, "VXL", "EUS"), Some(10.0));
        assert_eq!(running_minutes(&routes, "VIC", "VIC"), None);
        assert_eq!(running_minutes(&routes, "EUS", "KSX"), None);
        // A step with no time: no running time over it.
        let gap = vec![route(&["A", "B", "C"], Some(vec![Some(60), None]))];
        assert_eq!(running_minutes(&gap, "A", "B"), Some(1.0));
        assert_eq!(running_minutes(&gap, "A", "C"), None);
        // Times that don't fit the stops are not used.
        let bad = vec![route(&["A", "B", "C"], Some(vec![Some(60)]))];
        assert_eq!(running_minutes(&bad, "A", "B"), None);
        assert_eq!(running_minutes(&[route(&["A", "B"], None)], "A", "B"), None);
    }

    #[test]
    fn a_short_tube_link_is_raised_to_its_walks_wait_and_ride() {
        let lu = OperatorId::LONDON_UNDERGROUND;
        let mut data = InterchangeData::default();
        data.fixed_links_from_crs.insert(
            "EUS".to_owned(),
            vec![
                // 2 walk + 3 wait + 7 ride + 3 walk = 15.
                link("TUBE", "VIC", 9, lu),
                // Already longer than 2 + 3 + 10 + 2 = 17: kept.
                link("TUBE", "VXH", 22, lu),
            ],
        );
        data.fixed_links_from_crs.insert(
            "VIC".to_owned(),
            vec![
                // The night allowance over a Tube pair: raised too.
                link("TRANSFER", "EUS", 12, lu),
                // A walk is never a Tube hop.
                link("WALK", "EUS", 9, OperatorId::UNKNOWN),
            ],
        );
        let raised = raise_tube_links(&mut data, &tubes(), &victoria_line());
        assert_eq!(raised, 2);
        assert_eq!(minutes(&data, "EUS", "VIC"), [15]);
        assert_eq!(minutes(&data, "EUS", "VXH"), [22]);
        assert_eq!(minutes(&data, "VIC", "EUS"), [9, 15]);
    }

    #[test]
    fn a_link_shorter_than_its_ride_or_without_stations_is_kept() {
        let lu = OperatorId::LONDON_UNDERGROUND;
        let mut data = InterchangeData::default();
        data.fixed_links_from_crs.insert(
            "EUS".to_owned(),
            vec![
                // 7 minutes on the train alone: the timetable knows a
                // quicker way, so the floor is not used.
                link("TUBE", "VIC", 6, lu),
                // No Tube station known for KGX.
                link("TUBE", "KGX", 5, lu),
            ],
        );
        let mut stand_in = link("WALK", "VIC", 3, OperatorId::UNKNOWN);
        stand_in.only_without = lu;
        data.fixed_links_from_crs
            .insert("VXH".to_owned(), vec![stand_in]);
        assert_eq!(raise_tube_links(&mut data, &tubes(), &victoria_line()), 0);
        assert_eq!(minutes(&data, "EUS", "VIC"), [6]);
        assert_eq!(minutes(&data, "EUS", "KGX"), [5]);
        assert_eq!(minutes(&data, "VXH", "VIC"), [3]);
    }

    #[test]
    fn tube_link_stations_lists_both_ends_of_tube_hops_only() {
        let mut data = InterchangeData::default();
        data.fixed_links_from_crs.insert(
            "EUS".to_owned(),
            vec![
                link("TUBE", "VIC", 9, OperatorId::LONDON_UNDERGROUND),
                link("WALK", "STP", 5, OperatorId::UNKNOWN),
            ],
        );
        assert_eq!(tube_link_stations(&data), ["EUS", "VIC"]);
    }

    #[test]
    fn walks_need_time_left_to_ride() {
        assert_eq!(
            station_walks(22, 2, 2),
            Some(StationWalks {
                origin_minutes: 2,
                destination_minutes: 2
            })
        );
        assert_eq!(station_walks(4, 2, 2), None);
    }

    #[test]
    fn a_walk_is_the_transit_paths_rail_link_walk() {
        // London Euston to Euston (Tube): about 60 m, the 2-minute least.
        let walk = walk_minutes(
            Some((51.528_376, -0.134_558)),
            Some((51.527_999, -0.133_785)),
        );
        assert_eq!(walk, Some(2));
        // No position: the transit path's fallback.
        assert_eq!(walk_minutes(None, Some((51.5, -0.1))), Some(5));
        // Over a kilometre apart: a wrong position, no walk. Prod's
        // Canary Wharf (Elizabeth line) station and the Jubilee's.
        assert_eq!(
            walk_minutes(Some((51.5061, 0.01578)), Some((51.503_488, -0.018_246))),
            None
        );
    }
}
