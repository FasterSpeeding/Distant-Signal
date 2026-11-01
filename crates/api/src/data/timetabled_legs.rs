//! Timetabled untracked legs (DS-237): a Tube, DLR, London tram or
//! Metrolink ride saved from a plan, found again in its network's active
//! timetable each time the journey is read.
//!
//! **Scheduled, never live.** Such a leg is still `kind = 'untracked'`: no
//! subscription, no live times, no breaks. What the timetable adds is its
//! calling points, how often the line runs, whether the timetable still
//! has the ride (a notice when it doesn't, never a break or a push), and
//! the line's status.
//!
//! **The key is the content, not a trip id.** A `transit_*` feed version
//! lasts about two days (the horizon slides daily, and only the `active`
//! and `previous` versions are kept), and pattern and service ids are
//! renumbered at every import. So a leg is found by its network, line,
//! boarding and alighting stations and planned times, all of which a feed
//! re-import keeps. The trip id matched when it was saved
//! (`timetable_hint`) only breaks ties.
//!
//! **Matching** ([`choose`]): a ride on the leg's line between its two
//! stations whose departure and arrival, to the minute (the planner's
//! rounding, [`transit_planning::round_to_minute`]), are the leg's own is
//! `matched`; else the ride departing nearest the planned time within
//! [`RETIME_WINDOW_MINUTES`] is `retimed` (the timetable moved it); else
//! `notInTimetable` when the line still runs between the stations that day,
//! `noService` when it doesn't (a planned closure), and `noTimetable` when
//! the date is outside the feed's horizon or the network has no feed.

use std::collections::HashMap;

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::data::transit_planning::{self, PublicStopId, TransitLegNotice};

/// The untracked-leg modes a timetabled leg can have: the planner's
/// `serviceMode`s of a transit ride (a replacement bus has no line, so it
/// stays a plain untracked `bus` leg).
pub const MODES: [&str; 3] = ["tube", "dlr", "tram"];

/// The longest headsign a leg keeps (the migration's CHECK).
pub const HEADSIGN_MAX_CHARS: usize = 100;

/// How far the nearest timetabled ride may depart from the planned time
/// and still be taken as the leg's, moved.
pub const RETIME_WINDOW_MINUTES: i64 = 10;

/// How far either side of the planned departure rides are read: enough
/// for the headway, and for the connection checks to find the next ride
/// after a late train (phase B).
const READ_WINDOW_SECONDS: i64 = 4 * 3600;

/// The `journey_legs.timetable_hint` JSON: the ride matched when the leg
/// was saved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimetableHint {
    pub trip_id: String,
    pub feed_version: String,
}

/// What a saved leg says about its ride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegKey {
    pub network: String,
    pub line_id: String,
    /// In the network's public spelling, as saved (`940GZZLUOXC`,
    /// `9400ZZMAPIC`).
    pub origin_stop_id: String,
    pub destination_stop_id: String,
    pub planned_departure: DateTime<Utc>,
    pub planned_arrival: DateTime<Utc>,
    pub hint_trip_id: Option<String>,
}

/// One timetabled ride on the leg's line from its boarding station to its
/// alighting station.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ride {
    pub trip_id: String,
    pub pattern_id: String,
    pub board_seq: i32,
    pub alight_seq: i32,
    pub service_date: NaiveDate,
    pub start_s: i32,
    /// Both to the minute, as the planner shows them.
    pub departure: DateTime<Utc>,
    pub arrival: DateTime<Utc>,
    pub headsign: Option<String>,
}

/// How the leg's ride stands in today's timetable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchState {
    /// The timetable has the ride at the planned times.
    Matched,
    /// The nearest ride departs at another time (within
    /// [`RETIME_WINDOW_MINUTES`]); `scheduledDeparture` and
    /// `scheduledArrival` are its.
    Retimed,
    /// No ride near the planned time, but the line still runs between the
    /// two stations that day.
    NotInTimetable,
    /// Nothing on the line between the two stations that day.
    NoService,
    /// No timetable for that date (outside the feed's horizon, or no feed).
    NoTimetable,
}

/// [`MatchState`] and the ride it found, from `rides` (in departure order,
/// those near the planned time). With none, `noService`: the caller
/// tells it from `notInTimetable` by whether any ride runs that day.
/// Pure.
pub fn choose<'a>(rides: &'a [Ride], key: &LegKey) -> (MatchState, Option<&'a Ride>) {
    if rides.is_empty() {
        return (MatchState::NoService, None);
    }
    let exact: Vec<&Ride> = rides
        .iter()
        .filter(|ride| {
            ride.departure == key.planned_departure && ride.arrival == key.planned_arrival
        })
        .collect();
    if let Some(ride) = exact
        .iter()
        .find(|ride| Some(&ride.trip_id) == key.hint_trip_id.as_ref())
        .or_else(|| exact.first())
    {
        return (MatchState::Matched, Some(ride));
    }
    let window = Duration::minutes(RETIME_WINDOW_MINUTES);
    let nearest = rides
        .iter()
        .filter(|ride| (ride.departure - key.planned_departure).abs() <= window)
        .min_by_key(|ride| {
            (
                (ride.departure - key.planned_departure).abs(),
                ride.departure,
            )
        });
    match nearest {
        Some(ride) => (MatchState::Retimed, Some(ride)),
        None => (MatchState::NotInTimetable, None),
    }
}

/// The median gap between the rides (departures from the boarding station
/// that reach the alighting one) in the hour around `departure`; `None`
/// with fewer than three. Pure.
pub fn headway_minutes(rides: &[Ride], departure: DateTime<Utc>) -> Option<u32> {
    let half = Duration::minutes(30);
    let mut minutes: Vec<i64> = rides
        .iter()
        .filter(|ride| (ride.departure - departure).abs() <= half)
        .map(|ride| ride.departure.timestamp() / 60)
        .collect();
    minutes.sort_unstable();
    minutes.dedup();
    if minutes.len() < 3 {
        return None;
    }
    let mut gaps: Vec<i64> = minutes.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort_unstable();
    u32::try_from(gaps[(gaps.len() - 1) / 2]).ok()
}

/// A ride's line, as a plan's transit leg shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineView {
    /// The public id, `tfl:victoria`, `metrolink:blue`.
    pub id: String,
    pub name: String,
    pub colour: Option<String>,
    /// `tube`, `dlr`, `tram` (a line's own, else the network's).
    pub mode: Option<String>,
}

/// One timetabled call of the ride, from boarding to alighting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimetabledCall {
    /// The station, in the network's public spelling.
    pub stop_id: String,
    pub name: String,
    /// The platform's indicator (`To Bury`), when known.
    pub platform: Option<String>,
    /// `null` at the boarding call.
    pub scheduled_arrival: Option<DateTime<Utc>>,
    /// `null` at the alighting call.
    pub scheduled_departure: Option<DateTime<Utc>>,
}

/// A leg's `timetable` on `GET /Journeys/{id}`: what the network's active
/// timetable says about it now. Scheduled times only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegTimetable {
    pub network: String,
    /// `null` when the active feed no longer has the line.
    pub line: Option<LineView>,
    pub state: MatchState,
    /// The ride found (`matched` or `retimed`); else `null`.
    pub scheduled_departure: Option<DateTime<Utc>>,
    pub scheduled_arrival: Option<DateTime<Utc>>,
    pub headsign: Option<String>,
    /// Boarding to alighting; empty without a ride.
    pub calls: Vec<TimetabledCall>,
    /// How often the line runs between the two stations around the
    /// planned time; absent with too few rides to say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headway_minutes: Option<u32>,
    /// Current disruption on the line (the plan's transit leg notices).
    pub notices: Vec<TransitLegNotice>,
    /// Every ride between the two stations in the read window, for the
    /// connection checks.
    #[serde(skip)]
    pub rides: Vec<Ride>,
}

/// The local time `seconds` after `date`'s midnight in London, to the
/// minute ([`transit_planning::round_to_minute`]). A missing local time
/// (the spring change) takes the later reading.
pub fn london_instant(date: NaiveDate, seconds: i64) -> Option<DateTime<Utc>> {
    let minutes = transit_planning::round_to_minute(seconds);
    let local = date.and_hms_opt(0, 0, 0)? + Duration::minutes(i64::from(minutes));
    let tz = chrono_tz::Europe::London;
    tz.from_local_datetime(&local)
        .earliest()
        .or_else(|| {
            tz.from_local_datetime(&(local + Duration::hours(1)))
                .earliest()
        })
        .map(|at| at.with_timezone(&Utc))
}

fn london_date(at: DateTime<Utc>) -> NaiveDate {
    at.with_timezone(&chrono_tz::Europe::London).date_naive()
}

/// Seconds from `date`'s London midnight to `at`.
fn seconds_since(date: NaiveDate, at: DateTime<Utc>) -> Option<i64> {
    let midnight = chrono_tz::Europe::London
        .from_local_datetime(&date.and_hms_opt(0, 0, 0)?)
        .earliest()?;
    Some((at - midnight.with_timezone(&Utc)).num_seconds())
}

/// The network's spelling of a station node.
fn spelling(network: &str) -> PublicStopId {
    transit_planning::planner_network(network)
        .map_or(PublicStopId::AccessNode, |network| network.public_stop_id)
}

/// `network`'s active feed version and horizon.
async fn active_feed(
    pool: &PgPool,
    network: &str,
) -> Result<Option<(String, Option<NaiveDate>, Option<NaiveDate>)>> {
    let row: Option<(Option<String>, Option<NaiveDate>, Option<NaiveDate>)> = sqlx::query_as(
        "SELECT n.active_feed_version, v.horizon_from, v.horizon_to FROM transit_networks n \
         LEFT JOIN transit_feed_versions v \
           ON v.network = n.network AND v.feed_version = n.active_feed_version \
         WHERE n.network = $1",
    )
    .bind(network)
    .fetch_optional(pool)
    .await
    .context("read the active transit feed")?;
    Ok(row.and_then(|(version, from, to)| version.map(|version| (version, from, to))))
}

/// The line's row in the active feed, as a [`LineView`].
async fn line_view(
    pool: &PgPool,
    network: &str,
    feed_version: &str,
    line_id: &str,
) -> Result<Option<LineView>> {
    let row: Option<(String, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT l.name, l.colour, l.mode, n.mode FROM transit_lines l \
         JOIN transit_networks n ON n.network = l.network \
         WHERE l.network = $1 AND l.feed_version = $2 AND l.line_id = $3",
    )
    .bind(network)
    .bind(feed_version)
    .bind(line_id)
    .fetch_optional(pool)
    .await
    .context("read the transit line")?;
    Ok(row.map(|(name, colour, mode, network_mode)| LineView {
        id: format!("{network}:{line_id}"),
        name,
        colour,
        mode: Some(mode.unwrap_or(network_mode)),
    }))
}

/// Every ride on `line_id` of `feed_version` from station `origin` to
/// station `destination` (both stored nodes) departing within `window`
/// seconds of `around`, in departure order. Reads the service day of
/// `around` and the day before (a ride after midnight belongs to the day
/// before).
#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "one read's filters, each from the leg; one query and its unpacking"
)]
async fn read_rides(
    pool: &PgPool,
    network: &str,
    feed_version: &str,
    line_id: &str,
    origin: &str,
    destination: &str,
    around: DateTime<Utc>,
    window: i64,
) -> Result<Vec<Ride>> {
    let date = london_date(around);
    let Some(day_before) = date.pred_opt() else {
        return Ok(Vec::new());
    };
    let Some(at) = seconds_since(date, around) else {
        return Ok(Vec::new());
    };
    #[expect(
        clippy::type_complexity,
        reason = "one row of a single read, unpacked straight away"
    )]
    let rows: Vec<(
        String,
        String,
        i32,
        i32,
        NaiveDate,
        i32,
        i32,
        i32,
        Option<String>,
    )> = sqlx::query_as(
        "WITH platforms AS ( \
             SELECT s.stop_id, COALESCE(s.parent_stop_id, s.stop_id) AS station \
             FROM transit_stops s \
             WHERE s.network = $1 AND s.feed_version = $2 \
               AND (s.stop_id = ANY($4) OR s.parent_stop_id = ANY($4))) \
         SELECT t.trip_id, t.pattern_id, o.seq, a.seq, d.service_date, t.start_s, \
                o.dep_offset_s, a.arr_offset_s, t.headsign \
         FROM transit_pattern_stops o \
         JOIN platforms po ON po.stop_id = o.stop_id AND po.station = $5 \
         JOIN transit_pattern_stops a \
           ON a.network = o.network AND a.feed_version = o.feed_version \
          AND a.pattern_id = o.pattern_id AND a.seq > o.seq AND a.drop_off <> 1 \
         JOIN platforms pa ON pa.stop_id = a.stop_id AND pa.station = $6 \
         JOIN transit_trips t \
           ON t.network = o.network AND t.feed_version = o.feed_version \
          AND t.pattern_id = o.pattern_id AND t.line_id = $3 \
         JOIN transit_service_days d \
           ON d.network = t.network AND d.feed_version = t.feed_version \
          AND d.service_id = t.service_id \
         WHERE o.network = $1 AND o.feed_version = $2 AND o.pickup <> 1 \
           AND ((d.service_date = $7 AND t.start_s + o.dep_offset_s BETWEEN $9 AND $10) \
             OR (d.service_date = $8 AND t.start_s + o.dep_offset_s BETWEEN $9 + 86400 AND $10 + 86400))",
    )
    .bind(network)
    .bind(feed_version)
    .bind(line_id)
    .bind([origin, destination])
    .bind(origin)
    .bind(destination)
    .bind(date)
    .bind(day_before)
    .bind(at - window)
    .bind(at + window)
    .fetch_all(pool)
    .await
    .context("read timetabled rides")?;
    let mut rides: Vec<Ride> = rows
        .into_iter()
        .filter_map(
            |(
                trip_id,
                pattern_id,
                board_seq,
                alight_seq,
                service_date,
                start_s,
                dep,
                arr,
                headsign,
            )| {
                let departure = london_instant(service_date, i64::from(start_s) + i64::from(dep))?;
                let arrival = london_instant(service_date, i64::from(start_s) + i64::from(arr))?;
                Some(Ride {
                    trip_id,
                    pattern_id,
                    board_seq,
                    alight_seq,
                    service_date,
                    start_s,
                    departure,
                    arrival,
                    headsign,
                })
            },
        )
        .collect();
    // A loop calling twice at a station gives a trip twice: keep its
    // shortest ride from each boarding.
    rides.sort_by(|a, b| {
        (a.departure, &a.trip_id, a.board_seq, a.alight_seq).cmp(&(
            b.departure,
            &b.trip_id,
            b.board_seq,
            b.alight_seq,
        ))
    });
    rides.dedup_by(|later, earlier| {
        later.trip_id == earlier.trip_id && later.board_seq == earlier.board_seq
    });
    Ok(rides)
}

/// One call as [`read_calls`] reads it: its sequence, station, name,
/// platform indicator and arrival and departure offsets.
type CallRow = (i32, String, Option<String>, Option<String>, i32, i32);

/// The calls of `ride`, boarding to alighting, with station names and
/// platform indicators.
async fn read_calls(
    pool: &PgPool,
    network: &str,
    feed_version: &str,
    ride: &Ride,
) -> Result<Vec<TimetabledCall>> {
    let rows: Vec<CallRow> = sqlx::query_as(
        "SELECT ps.seq, COALESCE(p.parent_stop_id, ps.stop_id), \
                COALESCE(st.name, p.name), p.indicator, ps.arr_offset_s, ps.dep_offset_s \
         FROM transit_pattern_stops ps \
         LEFT JOIN transit_stops p ON p.network = ps.network \
              AND p.feed_version = ps.feed_version AND p.stop_id = ps.stop_id \
         LEFT JOIN transit_stops st ON st.network = ps.network \
              AND st.feed_version = ps.feed_version AND st.stop_id = p.parent_stop_id \
         WHERE ps.network = $1 AND ps.feed_version = $2 AND ps.pattern_id = $3 \
           AND ps.seq BETWEEN $4 AND $5 \
         ORDER BY ps.seq",
    )
    .bind(network)
    .bind(feed_version)
    .bind(&ride.pattern_id)
    .bind(ride.board_seq)
    .bind(ride.alight_seq)
    .fetch_all(pool)
    .await
    .context("read timetabled calls")?;
    let spelling = spelling(network);
    let at = |offset: i32| {
        london_instant(
            ride.service_date,
            i64::from(ride.start_s) + i64::from(offset),
        )
    };
    Ok(rows
        .into_iter()
        .map(|(seq, station, name, platform, arr, dep)| TimetabledCall {
            stop_id: transit_planning::public_id(&station, spelling),
            name: name.unwrap_or_else(|| station.clone()),
            platform,
            scheduled_arrival: if seq == ride.board_seq { None } else { at(arr) },
            scheduled_departure: if seq == ride.alight_seq {
                None
            } else {
                at(dep)
            },
        })
        .collect())
}

/// The rides between `key`'s stations around its planned departure and the
/// active feed version they come from; `None` when there is no timetable
/// for the date (or no feed). With none near the planned time, whether
/// any runs that day is the third value (`notInTimetable` against
/// `noService`).
pub async fn find_rides(pool: &PgPool, key: &LegKey) -> Result<Option<(String, Vec<Ride>, bool)>> {
    let Some((feed_version, from, to)) = active_feed(pool, &key.network).await? else {
        return Ok(None);
    };
    let date = london_date(key.planned_departure);
    let covered = from.is_some_and(|from| from <= date) && to.is_some_and(|to| date <= to);
    if !covered {
        return Ok(None);
    }
    let origin = transit_planning::access_node_spelling(&key.origin_stop_id);
    let destination = transit_planning::access_node_spelling(&key.destination_stop_id);
    let read = |window: i64| {
        read_rides(
            pool,
            &key.network,
            &feed_version,
            &key.line_id,
            &origin,
            &destination,
            key.planned_departure,
            window,
        )
    };
    let rides = read(READ_WINDOW_SECONDS).await?;
    // The whole service day either side: is there any ride at all?
    let runs = !rides.is_empty() || !read(30 * 3600).await?.is_empty();
    Ok(Some((feed_version, rides, runs)))
}

/// The hint to save with a new leg: the ride its planned times match in the
/// active feed now, if any. Best effort: a failed read saves none.
pub async fn hint_for(pool: &PgPool, key: &LegKey) -> Option<TimetableHint> {
    match find_rides(pool, key).await {
        Ok(Some((feed_version, rides, _))) => match choose(&rides, key) {
            (MatchState::Matched, Some(ride)) => Some(TimetableHint {
                trip_id: ride.trip_id.clone(),
                feed_version,
            }),
            _ => None,
        },
        Ok(None) => None,
        Err(err) => {
            tracing::warn!(error = ?err, network = key.network, "could not match a new leg in the timetable");
            None
        }
    }
}

/// Whether `line_id` is a line of `network`'s active feed (`None`: the
/// network has no active feed).
pub async fn line_exists(pool: &PgPool, network: &str, line_id: &str) -> Result<Option<bool>> {
    let Some((feed_version, _, _)) = active_feed(pool, network).await? else {
        return Ok(None);
    };
    Ok(Some(
        line_view(pool, network, &feed_version, line_id)
            .await?
            .is_some(),
    ))
}

/// One leg's [`LegTimetable`], without its notices.
async fn resolve_one(pool: &PgPool, key: &LegKey) -> Result<LegTimetable> {
    let empty = |state: MatchState, line: Option<LineView>| LegTimetable {
        network: key.network.clone(),
        line,
        state,
        scheduled_departure: None,
        scheduled_arrival: None,
        headsign: None,
        calls: Vec::new(),
        headway_minutes: None,
        notices: Vec::new(),
        rides: Vec::new(),
    };
    let line = match active_feed(pool, &key.network).await? {
        Some((feed_version, _, _)) => {
            line_view(pool, &key.network, &feed_version, &key.line_id).await?
        }
        None => None,
    };
    let Some((feed_version, rides, runs)) = find_rides(pool, key).await? else {
        return Ok(empty(MatchState::NoTimetable, line));
    };
    let (state, ride) = match choose(&rides, key) {
        (MatchState::NoService, None) if runs => (MatchState::NotInTimetable, None),
        chosen => chosen,
    };
    let Some(ride) = ride.cloned() else {
        return Ok(LegTimetable {
            headway_minutes: headway_minutes(&rides, key.planned_departure),
            rides,
            ..empty(state, line)
        });
    };
    let calls = read_calls(pool, &key.network, &feed_version, &ride).await?;
    Ok(LegTimetable {
        network: key.network.clone(),
        line,
        state,
        scheduled_departure: Some(ride.departure),
        scheduled_arrival: Some(ride.arrival),
        headsign: ride.headsign.clone(),
        calls,
        headway_minutes: headway_minutes(&rides, ride.departure),
        notices: Vec::new(),
        rides,
    })
}

/// Each leg's [`LegTimetable`] (`None` for a leg with no key, or whose read
/// failed: a timetable never fails a journey read), with its line's current
/// disruption, one status read per network.
pub async fn resolve_all(pool: &PgPool, keys: &[Option<LegKey>]) -> Vec<Option<LegTimetable>> {
    use ds_store::transit::disruptions;
    let mut out: Vec<Option<LegTimetable>> = Vec::with_capacity(keys.len());
    for key in keys {
        let Some(key) = key else {
            out.push(None);
            continue;
        };
        out.push(match resolve_one(pool, key).await {
            Ok(timetable) => Some(timetable),
            Err(err) => {
                tracing::warn!(error = ?err, network = key.network, "could not read a leg's timetable");
                None
            }
        });
    }
    let mut statuses: HashMap<String, Option<_>> = HashMap::new();
    let now = Utc::now();
    for timetable in out.iter_mut().flatten() {
        let Some(line) = timetable.line.clone() else {
            continue;
        };
        if !statuses.contains_key(&timetable.network) {
            let network = timetable.network.clone();
            let read = async {
                let as_of = disruptions::situations_as_of(pool, &network).await?;
                let stored = disruptions::stored_line_statuses(pool, &network).await?;
                anyhow::Ok((disruptions::state_at(as_of, now), as_of, stored))
            };
            let read = read.await.map_or_else(
                |err| {
                    tracing::warn!(error = ?err, network, "could not read transit line statuses for journey legs");
                    None
                },
                Some,
            );
            statuses.insert(network, read);
        }
        let Some(Some((state, as_of, stored))) = statuses.get(&timetable.network) else {
            continue;
        };
        let prefix = format!("{}:", timetable.network);
        let local = line.id.strip_prefix(&prefix).unwrap_or(&line.id);
        let view = disruptions::line_view(*state, *as_of, stored.get(local));
        timetable.notices =
            transit_planning::line_notices(&timetable.network, local, &line.name, &view);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    fn ride(trip: &str, departure: &str, arrival: &str) -> Ride {
        Ride {
            trip_id: trip.to_owned(),
            pattern_id: "p1".to_owned(),
            board_seq: 0,
            alight_seq: 3,
            service_date: NaiveDate::from_ymd_opt(2026, 11, 4).unwrap(),
            start_s: 0,
            departure: at(departure),
            arrival: at(arrival),
            headsign: None,
        }
    }

    fn key(departure: &str, arrival: &str, hint: Option<&str>) -> LegKey {
        LegKey {
            network: "tfl".to_owned(),
            line_id: "victoria".to_owned(),
            origin_stop_id: "940GZZLUEUS".to_owned(),
            destination_stop_id: "940GZZLUVIC".to_owned(),
            planned_departure: at(departure),
            planned_arrival: at(arrival),
            hint_trip_id: hint.map(str::to_owned),
        }
    }

    #[test]
    fn the_planned_times_match_and_the_hint_breaks_a_tie() {
        let rides = [
            ride("a", "2026-11-04T08:10:00Z", "2026-11-04T08:20:00Z"),
            ride("b", "2026-11-04T08:14:00Z", "2026-11-04T08:24:00Z"),
            ride("c", "2026-11-04T08:14:00Z", "2026-11-04T08:24:00Z"),
        ];
        let (state, found) = choose(
            &rides,
            &key("2026-11-04T08:14:00Z", "2026-11-04T08:24:00Z", None),
        );
        assert_eq!(state, MatchState::Matched);
        assert_eq!(found.unwrap().trip_id, "b");
        let (_, found) = choose(
            &rides,
            &key("2026-11-04T08:14:00Z", "2026-11-04T08:24:00Z", Some("c")),
        );
        assert_eq!(found.unwrap().trip_id, "c", "the hint wins a tie");
    }

    #[test]
    fn a_moved_ride_is_retimed_within_the_window_only() {
        let rides = [
            ride("a", "2026-11-04T08:10:00Z", "2026-11-04T08:20:00Z"),
            ride("b", "2026-11-04T08:40:00Z", "2026-11-04T08:50:00Z"),
        ];
        let (state, found) = choose(
            &rides,
            &key("2026-11-04T08:14:00Z", "2026-11-04T08:24:00Z", None),
        );
        assert_eq!(state, MatchState::Retimed);
        assert_eq!(found.unwrap().trip_id, "a");
        let (state, found) = choose(
            &rides,
            &key("2026-11-04T08:25:00Z", "2026-11-04T08:35:00Z", None),
        );
        assert_eq!(state, MatchState::NotInTimetable, "15 minutes from either");
        assert!(found.is_none());
        assert_eq!(
            choose(
                &[],
                &key("2026-11-04T08:14:00Z", "2026-11-04T08:24:00Z", None)
            )
            .0,
            MatchState::NoService
        );
        // Same departure, a longer ride: retimed, not matched.
        let (state, _) = choose(
            &rides,
            &key("2026-11-04T08:10:00Z", "2026-11-04T08:19:00Z", None),
        );
        assert_eq!(state, MatchState::Retimed);
    }

    #[test]
    fn the_headway_is_the_median_gap_in_the_hour_around() {
        let rides = [
            ride("a", "2026-11-04T08:00:00Z", "2026-11-04T08:10:00Z"),
            ride("b", "2026-11-04T08:03:00Z", "2026-11-04T08:13:00Z"),
            ride("c", "2026-11-04T08:07:00Z", "2026-11-04T08:17:00Z"),
            ride("d", "2026-11-04T08:10:00Z", "2026-11-04T08:20:00Z"),
            ride("far", "2026-11-04T10:00:00Z", "2026-11-04T10:10:00Z"),
        ];
        assert_eq!(headway_minutes(&rides, at("2026-11-04T08:05:00Z")), Some(3));
        assert_eq!(headway_minutes(&rides, at("2026-11-04T10:00:00Z")), None);
    }

    #[test]
    fn local_times_are_london_times_to_the_minute() {
        let date = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        assert_eq!(
            london_instant(date, 8 * 3600 + 14 * 60 + 29),
            Some(at("2026-07-01T07:14:00Z")),
            "BST, rounded down"
        );
        assert_eq!(
            london_instant(date, 24 * 3600 + 30 * 60 + 30),
            Some(at("2026-07-01T23:31:00Z")),
            "past midnight is the next morning, rounded half up"
        );
        let winter = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        assert_eq!(
            london_instant(winter, 8 * 3600),
            Some(at("2026-11-04T08:00:00Z"))
        );
    }
}
