//! Whether each change in a saved journey still works, on live times and
//! with the station's minimum change time (spec
//! docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md §5.1,
//! Phase 0). Served on `GET /Journeys/{id}` as each leg's `connection`.
//!
//! Built on the `/Trips/plan` live overlay's own layers
//! ([`trip_plan_live`]), so a planned itinerary and a saved journey are
//! judged by one rule:
//!
//! 1. each train leg's calls become a connection chain, and
//!    [`trip_plan_live::evaluate_chain`] applies the train's
//!    [`trip_plan_live::TrainLive`] (read with
//!    [`trip_plan_live::fetch_train_lives`] inside the live window, else the
//!    timetable alone): which calls still happen, and when;
//! 2. [`evaluate_connections`] (pure) walks the legs in order and judges
//!    each change with [`trip_plan_live::interchange_feasible`], charging
//!    the minimum change time (`schedule_query::minimum_change_time`, MSN)
//!    at the boarding station, or a fixed link (read both ways) plus that
//!    MSN when the change is between two stations.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use chrono::{DateTime, NaiveDate, Timelike, Utc};
use schedule_query::{ChangeTime, Connection, InterchangeData, normalize_tiploc};
use serde::Serialize;
use sqlx::PgPool;

use crate::data::journey::JourneyStop;
use crate::data::trip_plan_live::{self, LiveConfig, TrainLive};
use crate::data::trip_planning::{self, GraphCache};

/// A change with less spare time than this, after the minimum change time,
/// is at risk once delays have eaten into it (spec §5.1).
pub const AT_RISK_SLACK_MINUTES: i64 = 3;

/// How one change between two consecutive legs stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ConnectionState {
    /// Enough time to change, with room to spare (or none was needed).
    Ok,
    /// Delays leave less than [`AT_RISK_SLACK_MINUTES`] to spare, or the
    /// change was booked below the minimum change time and is no worse.
    AtRisk,
    /// The next leg leaves before the traveller can get to it.
    Missed,
    /// Not judged: a time is unknown, a train is cancelled at either end of
    /// the change, the station allows no interchange (MSN 98 or 99), or no
    /// fixed link joins two different stations.
    Unknown,
}

/// The change onto a leg from the one before it (`connection` on every
/// leg of `GET /Journeys/{id}` after the first).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegConnection {
    /// Minutes from the previous leg's expected arrival (reported, else
    /// estimated, else timetabled) to this leg's expected departure.
    /// Negative when this leg leaves first. `null` when either is unknown.
    pub gap_minutes: Option<i64>,
    /// The time the change needs: the boarding station's minimum change
    /// time, a fixed link's minutes plus that, or 0 staying on one train.
    /// `null` when not known (see [`ConnectionState::Unknown`]) or when the
    /// next leg is untracked (a walk or a Tube ride starts when you do).
    pub minimum_change_minutes: Option<u32>,
    pub state: ConnectionState,
    /// Set when the change works only because the next train is running
    /// late (additive, DS-141; absent otherwise): it would not, on the
    /// next train's timetabled departure. The state is judged as before
    /// (usually `ok`); this says the train could leave earlier than
    /// expected. See [`trip_plan_live::LateDepartureReliance`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relies_on_late_departure: Option<trip_plan_live::LateDepartureReliance<DateTime<Utc>>>,
    /// Onto a timetabled leg (DS-237, additive; absent otherwise): the
    /// ride its timetable has first from when the traveller gets there,
    /// when that is not the planned one (the traveller arrives late, or
    /// the timetable has moved it). The changes after it are judged from
    /// its arrival. Scheduled times, never live.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_timetabled: Option<TimetabledRide>,
}

/// A ride of a timetabled leg, by its scheduled times.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimetabledRide {
    pub departure: DateTime<Utc>,
    pub arrival: DateTime<Utc>,
}

/// One train leg's boarding and alighting calls, timetabled and live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainLegTiming {
    pub uid: String,
    pub date: NaiveDate,
    pub board_crs: String,
    pub board_tiploc: String,
    pub alight_crs: String,
    pub sched_dep: DateTime<Utc>,
    pub live_dep: DateTime<Utc>,
    pub sched_arr: DateTime<Utc>,
    pub live_arr: DateTime<Utc>,
    /// The train still calls at both ends of the leg.
    pub served: bool,
}

/// One leg as [`evaluate_connections`] sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegTiming {
    Train(TrainLegTiming),
    /// A Tube ride, bus or walk: its planned times. A timetabled leg
    /// (DS-237) also has its line's rides between its two stops around
    /// then, in departure order (else empty): the traveller takes the
    /// first one leaving once they get there.
    Untracked {
        departure: Option<DateTime<Utc>>,
        arrival: Option<DateTime<Utc>>,
        rides: Vec<TimetabledRide>,
    },
    /// A train leg with no train picked yet, or whose calls aren't known.
    Unknown,
}

/// Where and when the traveller is ready to start the next leg.
struct Ready {
    at: DateTime<Utc>,
    /// The same on the timetable (for the slack the plan was booked with).
    sched_at: DateTime<Utc>,
    /// The station, or `None` after an untracked leg (a place).
    crs: Option<String>,
    /// The train ridden there, to tell staying aboard from a change.
    train: Option<(String, NaiveDate)>,
    served: bool,
}

fn whole_minutes(from: DateTime<Utc>, to: DateTime<Utc>) -> i64 {
    (to - from).num_minutes()
}

/// The state of a change with `gap` minutes available, `planned_gap` on
/// the timetable, needing `required`.
fn classify(gap: i64, planned_gap: i64, required: u32) -> ConnectionState {
    if gap < 0 {
        return ConnectionState::Missed;
    }
    let slack = gap - i64::from(required);
    let planned_slack = planned_gap - i64::from(required);
    if !trip_plan_live::interchange_feasible(gap, 0, required) {
        // Booked below the minimum and no worse than booked: tight, as it
        // always was. Otherwise delays have broken it.
        return if planned_slack < 0 && slack >= planned_slack {
            ConnectionState::AtRisk
        } else {
            ConnectionState::Missed
        };
    }
    if slack < AT_RISK_SLACK_MINUTES && slack < planned_slack {
        ConnectionState::AtRisk
    } else {
        ConnectionState::Ok
    }
}

/// The MSN at `tiploc`, `None` for no interchange (98/99).
fn msn(interchange: &InterchangeData, tiploc: &str) -> Option<u32> {
    match schedule_query::minimum_change_time(interchange, tiploc) {
        ChangeTime::Finite(minutes) => Some(minutes),
        ChangeTime::NoInterchange => None,
    }
}

/// The minutes a change from `ready` onto `next` needs, `None` when it
/// can't be judged.
fn required_change(
    ready: &Ready,
    next: &TrainLegTiming,
    interchange: &InterchangeData,
) -> Option<u32> {
    let same_station = ready
        .crs
        .as_deref()
        .is_some_and(|crs| crs.eq_ignore_ascii_case(&next.board_crs));
    if same_station && ready.train.as_ref() == Some(&(next.uid.clone(), next.date)) {
        // Staying on the same train.
        return Some(0);
    }
    match ready.crs.as_deref() {
        // Walked or rode in from a place: the station's MSN to reach the
        // platform, as the planners charge it at every fresh boarding.
        None => msn(interchange, &next.board_tiploc),
        Some(_) if same_station => msn(interchange, &next.board_tiploc),
        Some(from_crs) => {
            let local = ready.at.with_timezone(&chrono_tz::Europe::London);
            // Both codes are upper case (`train_leg_timing`).
            let link = schedule_query::fixed_link_between(
                interchange,
                from_crs,
                &next.board_crs,
                local.date_naive(),
                local.hour() * 60 + local.minute(),
            )?;
            let link_minutes = u32::try_from(link.minutes.max(0)).unwrap_or(0);
            Some(link_minutes + msn(interchange, &next.board_tiploc).unwrap_or(0))
        }
    }
}

/// The change from `ready` onto `next`, needing `required` minutes, works
/// only because `next` is running late ([`trip_plan_live::late_departure_slack`],
/// the planner's rule). Never for staying aboard one train.
fn late_departure_reliance(
    ready: &Ready,
    next: &TrainLegTiming,
    required: u32,
) -> Option<trip_plan_live::LateDepartureReliance<DateTime<Utc>>> {
    let same_train = ready.train.as_ref() == Some(&(next.uid.clone(), next.date));
    if same_train {
        return None;
    }
    let slack = trip_plan_live::late_departure_slack(
        whole_minutes(ready.at, next.sched_dep),
        whole_minutes(ready.at, next.live_dep),
        0,
        required,
    )?;
    Some(trip_plan_live::LateDepartureReliance {
        scheduled_departure: next.sched_dep,
        expected_departure: next.live_dep,
        delay_minutes: whole_minutes(next.sched_dep, next.live_dep),
        slack_if_on_time_minutes: slack,
    })
}

/// The ride of a timetabled leg the traveller takes when ready at
/// `ready` (`None`: at the start of the journey): the first leaving at or
/// after the later of then and the planned departure. `None` with no
/// rides, or none that late (the leg then shifts as any untracked leg).
fn ride_taken(
    rides: &[TimetabledRide],
    departure: DateTime<Utc>,
    ready: Option<DateTime<Utc>>,
) -> Option<TimetabledRide> {
    let from = ready.map_or(departure, |at| at.max(departure));
    rides.iter().find(|ride| ride.departure >= from).copied()
}

/// Every leg's [`LegConnection`] (`None` for the first), in order. Pure.
///
/// An untracked leg is never missed: it starts when the traveller gets
/// there, keeping its planned duration, and the change after it is judged
/// from that shifted arrival (spec §5.1). A timetabled one (DS-237) is
/// re-timed instead: the traveller takes its line's first ride leaving
/// once they are there, and the change after it is judged from that
/// ride's arrival. A change involving a leg with unknown times, or a train
/// cancelled at either end, is [`ConnectionState::Unknown`]; the
/// cancellation shows on the leg itself.
pub fn evaluate_connections(
    legs: &[LegTiming],
    interchange: &InterchangeData,
) -> Vec<Option<LegConnection>> {
    evaluate(legs, interchange).0
}

/// [`evaluate_connections`], and when the traveller is ready after each
/// leg (`None` where unknown): a train's live arrival, an untracked leg's
/// shifted or re-timed arrival.
fn evaluate(
    legs: &[LegTiming],
    interchange: &InterchangeData,
) -> (Vec<Option<LegConnection>>, Vec<Option<DateTime<Utc>>>) {
    let unknown = |gap_minutes: Option<i64>| LegConnection {
        gap_minutes,
        minimum_change_minutes: None,
        state: ConnectionState::Unknown,
        relies_on_late_departure: None,
        next_timetabled: None,
    };
    let mut out = Vec::with_capacity(legs.len());
    let mut ready_after = Vec::with_capacity(legs.len());
    let mut ready: Option<Ready> = None;
    for (index, leg) in legs.iter().enumerate() {
        let connection = match (index, &ready, leg) {
            (0, _, _) => None,
            (_, _, LegTiming::Unknown) | (_, None, _) => Some(unknown(None)),
            (
                _,
                Some(r),
                LegTiming::Untracked {
                    departure,
                    arrival,
                    rides,
                },
            ) => {
                let taken = departure.and_then(|d| ride_taken(rides, d, Some(r.at)));
                let boards = taken.map(|ride| ride.departure).or(*departure);
                let gap = boards.map(|d| whole_minutes(r.at, d));
                Some(match gap {
                    Some(gap) => LegConnection {
                        gap_minutes: Some(gap),
                        minimum_change_minutes: None,
                        state: ConnectionState::Ok,
                        relies_on_late_departure: None,
                        next_timetabled: taken.filter(|ride| {
                            Some(ride.departure) != *departure || Some(ride.arrival) != *arrival
                        }),
                    },
                    None => unknown(None),
                })
            }
            (_, Some(r), LegTiming::Train(next)) => {
                let gap = whole_minutes(r.at, next.live_dep);
                let planned_gap = whole_minutes(r.sched_at, next.sched_dep);
                Some(if !r.served || !next.served {
                    unknown(Some(gap))
                } else {
                    match required_change(r, next, interchange) {
                        Some(required) => LegConnection {
                            gap_minutes: Some(gap),
                            minimum_change_minutes: Some(required),
                            state: classify(gap, planned_gap, required),
                            relies_on_late_departure: late_departure_reliance(r, next, required),
                            next_timetabled: None,
                        },
                        None => unknown(Some(gap)),
                    }
                })
            }
        };
        out.push(connection);

        ready = match leg {
            LegTiming::Train(t) => Some(Ready {
                at: t.live_arr,
                sched_at: t.sched_arr,
                crs: Some(t.alight_crs.clone()),
                train: Some((t.uid.clone(), t.date)),
                served: t.served,
            }),
            LegTiming::Untracked {
                departure: Some(departure),
                arrival: Some(arrival),
                rides,
            } => {
                let at = ride_taken(rides, *departure, ready.as_ref().map(|r| r.at)).map_or_else(
                    || {
                        let start = ready.as_ref().map_or(*departure, |r| r.at.max(*departure));
                        start + (*arrival - *departure)
                    },
                    |ride| ride.arrival,
                );
                Some(Ready {
                    at,
                    sched_at: *arrival,
                    crs: None,
                    train: None,
                    served: true,
                })
            }
            // No times: nothing to start the next leg from.
            LegTiming::Untracked { .. } | LegTiming::Unknown => None,
        };
        ready_after.push(ready.as_ref().map(|r| r.at));
    }
    (out, ready_after)
}

/// One call of a train, as the leg needs it: its TIPLOC and the minutes
/// [`trip_plan_live::evaluate_chain`] works in.
struct Call {
    tiploc: String,
    sched_arr: Option<u32>,
    sched_dep: Option<u32>,
    work_arr: Option<u32>,
    work_dep: Option<u32>,
    can_board: bool,
    can_alight: bool,
}

/// `stops`' calls (passing points left out) as connections, the shape the
/// planning graph gives `evaluate_chain`: public minutes (else working)
/// and working minutes from the train's own service date.
fn chain_from_stops(uid: &str, date: NaiveDate, stops: &[JourneyStop]) -> Vec<Connection> {
    let minute =
        |at: Option<DateTime<Utc>>| at.and_then(|at| trip_plan_live::service_minute(date, at));
    let calls: Vec<Call> = stops
        .iter()
        .filter(|stop| !stop.timetable.passing_point)
        .filter_map(|stop| {
            Some(Call {
                tiploc: stop.tiploc.clone()?,
                sched_arr: minute(stop.timetable.scheduled_arrival()),
                sched_dep: minute(stop.timetable.scheduled_departure()),
                work_arr: minute(stop.timetable.working_minute_arrival),
                work_dep: minute(stop.timetable.working_minute_departure),
                can_board: stop.timetable.can_board,
                can_alight: stop.timetable.can_alight,
            })
        })
        .collect();
    calls
        .windows(2)
        .filter_map(|pair| {
            let (from, to) = (&pair[0], &pair[1]);
            Some(Connection {
                operator: schedule_query::OperatorId::UNKNOWN,
                uid: uid.to_string().into(),
                from_tiploc: from.tiploc.clone().into(),
                to_tiploc: to.tiploc.clone().into(),
                departure_min: from.sched_dep?,
                arrival_min: to.sched_arr?,
                working_departure_min: from.work_dep?,
                working_arrival_min: to.work_arr?,
                can_board: from.can_board,
                can_alight: to.can_alight,
            })
        })
        .collect()
}

/// A train leg's timing from its train's stops and live profile (`None`:
/// the timetable alone). `None` when the train doesn't call at both
/// stations, in order.
pub fn train_leg_timing(
    uid: &str,
    date: NaiveDate,
    stops: &[JourneyStop],
    board_crs: &str,
    alight_crs: &str,
    live: Option<&TrainLive>,
) -> Option<TrainLegTiming> {
    let crs_by_tiploc: HashMap<String, String> = stops
        .iter()
        .filter_map(|stop| {
            Some((
                normalize_tiploc(stop.tiploc.as_deref()?).to_ascii_uppercase(),
                stop.crs.clone()?,
            ))
        })
        .collect();
    let chain = chain_from_stops(uid, date, stops);
    let refs: Vec<&Connection> = chain.iter().collect();
    let timetable_only = TrainLive::default();
    let points = trip_plan_live::evaluate_chain(&refs, live.unwrap_or(&timetable_only));
    let at = |tiploc: &str, crs: &str| {
        crs_by_tiploc
            .get(&normalize_tiploc(tiploc).to_ascii_uppercase())
            .is_some_and(|c| c.eq_ignore_ascii_case(crs))
    };
    let board_index = points
        .iter()
        .position(|p| p.sched_dep.is_some() && at(&p.tiploc, board_crs))?;
    let board = &points[board_index];
    let alight = points[board_index + 1..]
        .iter()
        .find(|p| p.sched_arr.is_some() && at(&p.tiploc, alight_crs))?;
    let instant =
        |minute: Option<u32>| minute.and_then(|m| trip_plan_live::service_instant(date, m));
    Some(TrainLegTiming {
        uid: uid.to_string(),
        date,
        board_crs: board_crs.to_ascii_uppercase(),
        board_tiploc: board.tiploc.clone(),
        alight_crs: alight_crs.to_ascii_uppercase(),
        sched_dep: instant(board.sched_dep)?,
        live_dep: instant(board.live_dep.or(board.sched_dep))?,
        sched_arr: instant(alight.sched_arr)?,
        live_arr: instant(alight.live_arr.or(alight.sched_arr))?,
        served: board.served && alight.served,
    })
}

/// One leg of a journey as read for `GET /Journeys/{id}`.
pub enum LegInput<'a> {
    Train {
        /// The leg's own stations, else its train's pin, else the train's
        /// own route.
        board_crs: Option<&'a str>,
        alight_crs: Option<&'a str>,
        /// `None` until a train is picked and identified.
        train: Option<TrainInput<'a>>,
    },
    Untracked {
        departure: Option<DateTime<Utc>>,
        arrival: Option<DateTime<Utc>>,
        /// A timetabled leg's rides (DS-237), in departure order; empty
        /// otherwise.
        rides: Vec<TimetabledRide>,
    },
}

/// A picked train's identity and stops (`TrackedTrainState`).
pub struct TrainInput<'a> {
    pub uid: &'a str,
    pub date: NaiveDate,
    pub stops: &'a [JourneyStop],
}

static LIVE_CONFIG: LazyLock<LiveConfig> = LazyLock::new(LiveConfig::from_env);

/// Interchange data (MSN, fixed links) changes only with a reference-data
/// load, so one copy is kept for ten minutes. Off in unit tests, which
/// reseed it.
static INTERCHANGE_CACHE: LazyLock<Arc<GraphCache<InterchangeData>>> = LazyLock::new(|| {
    Arc::new(GraphCache::new(
        usize::from(!cfg!(test)),
        std::time::Duration::from_secs(600),
    ))
});

async fn interchange_data(pool: &PgPool) -> anyhow::Result<Arc<InterchangeData>> {
    let pool = pool.clone();
    INTERCHANGE_CACHE
        .get_or_build(
            NaiveDate::MIN,
            trip_planning::GraphMarker::default(),
            move || async move { trip_planning::fetch_interchange_data(&pool).await.map(Some) },
        )
        .await?
        .map(|(data, _)| data)
        .ok_or_else(|| anyhow::anyhow!("interchange data build returned nothing"))
}

/// Every leg's [`LegConnection`] for a journey (`None` for the first leg),
/// or `None` when it couldn't be worked out (the interchange data failed
/// to load), so the field is simply absent. Live data is read only for
/// trains whose leg departs inside the live window
/// ([`LiveConfig`]); a failure there falls back to the timetable.
pub async fn journey_connections(
    pool: &PgPool,
    legs: &[LegInput<'_>],
    now: DateTime<Utc>,
) -> Option<Vec<Option<LegConnection>>> {
    if legs.len() < 2 {
        return Some(legs.iter().map(|_| None).collect());
    }
    journey_timings(pool, legs, now)
        .await
        .map(|timings| timings.connections)
}

/// A journey's legs judged on live times: each leg's [`LegTiming`], each
/// change's [`LegConnection`] (`None` for the first leg), and the
/// interchange data they were judged with. What `GET /Journeys/{id}`'s
/// `connection` and the mid-journey break logic (§5.3) both read.
#[derive(Debug, Clone)]
pub struct JourneyTimings {
    pub timings: Vec<LegTiming>,
    pub connections: Vec<Option<LegConnection>>,
    /// When the traveller is ready after each leg (`None` where unknown):
    /// a train's live arrival, an untracked leg's shifted arrival, a
    /// timetabled leg's re-timed one.
    pub ready_after: Vec<Option<DateTime<Utc>>>,
    pub interchange: Arc<InterchangeData>,
}

impl JourneyTimings {
    /// The minutes a change onto train leg `next` needs when arriving from
    /// `from_crs` (`None`: from a place, after a walk or a Tube ride) on
    /// another train: the station's minimum change time, or a fixed link
    /// plus that between two stations. `None` when it can't be judged.
    pub fn change_minutes(
        &self,
        from_crs: Option<&str>,
        at: DateTime<Utc>,
        next: &TrainLegTiming,
    ) -> Option<u32> {
        let ready = Ready {
            at,
            sched_at: at,
            crs: from_crs.map(str::to_ascii_uppercase),
            train: None,
            served: true,
        };
        required_change(&ready, next, &self.interchange)
    }
}

/// [`JourneyTimings`] for a journey, `None` when the interchange data
/// failed to load. See [`journey_connections`] for the live window.
pub async fn journey_timings(
    pool: &PgPool,
    legs: &[LegInput<'_>],
    now: DateTime<Utc>,
) -> Option<JourneyTimings> {
    let interchange = match interchange_data(pool).await {
        Ok(data) => data,
        Err(err) => {
            tracing::warn!(error = ?err, "could not read interchange data; no connection states");
            return None;
        }
    };
    let config = &*LIVE_CONFIG;

    // Timetable-only first, to find which trains are inside the live window.
    let resolved: Vec<Option<(&TrainInput<'_>, String, String)>> = legs
        .iter()
        .map(|leg| match leg {
            LegInput::Train {
                board_crs,
                alight_crs,
                train: Some(train),
            } => Some((train, (*board_crs)?.to_string(), (*alight_crs)?.to_string())),
            _ => None,
        })
        .collect();
    let mut wanted: HashMap<NaiveDate, HashSet<String>> = HashMap::new();
    if config.enabled {
        for (train, board, alight) in resolved.iter().flatten() {
            let Some(timing) =
                train_leg_timing(train.uid, train.date, train.stops, board, alight, None)
            else {
                continue;
            };
            let window_start = now - chrono::Duration::minutes(config.lookback_minutes);
            let window_end = now + chrono::Duration::minutes(config.horizon_minutes);
            if timing.sched_arr >= window_start && timing.sched_dep <= window_end {
                wanted
                    .entry(train.date)
                    .or_default()
                    .insert(train.uid.to_string());
            }
        }
    }
    let mut lives: HashMap<(NaiveDate, String), TrainLive> = HashMap::new();
    for (date, uids) in wanted {
        let uids: Vec<String> = uids.into_iter().take(config.max_trains).collect();
        match trip_plan_live::fetch_train_lives(pool, date, &uids, config, now).await {
            Ok(found) => {
                lives.extend(found.into_iter().map(|(uid, live)| ((date, uid), live)));
            }
            Err(err) => {
                tracing::warn!(error = ?err, %date, "could not read live train data; judging connections on the timetable");
            }
        }
    }

    let timings: Vec<LegTiming> = legs
        .iter()
        .zip(&resolved)
        .map(|(leg, resolved)| match (leg, resolved) {
            (
                LegInput::Untracked {
                    departure,
                    arrival,
                    rides,
                },
                _,
            ) => LegTiming::Untracked {
                departure: *departure,
                arrival: *arrival,
                rides: rides.clone(),
            },
            (LegInput::Train { .. }, Some((train, board, alight))) => train_leg_timing(
                train.uid,
                train.date,
                train.stops,
                board,
                alight,
                lives.get(&(train.date, train.uid.to_string())),
            )
            .map_or(LegTiming::Unknown, LegTiming::Train),
            (LegInput::Train { .. }, None) => LegTiming::Unknown,
        })
        .collect();
    let (connections, ready_after) = evaluate(&timings, &interchange);
    Some(JourneyTimings {
        timings,
        connections,
        ready_after,
        interchange,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::journey::StopStatus;
    use schedule_query::{CallingPointKind, FixedLink};

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 22).unwrap()
    }

    fn at(minute: u32) -> DateTime<Utc> {
        trip_plan_live::service_instant(date(), minute).unwrap()
    }

    fn interchange() -> InterchangeData {
        let mut data = InterchangeData::default();
        // York: MSN 5 (the default). Doncaster: 8. A bus stand: 99.
        data.change_time_by_tiploc.insert("DONC".to_string(), 8);
        data.change_time_by_tiploc.insert("BUSSTND".to_string(), 99);
        data.fixed_links_from_crs = schedule_query::index_fixed_links_both_ways([(
            "KGX".to_string(),
            FixedLink {
                only_without: schedule_query::OperatorId::UNKNOWN,
                operator: schedule_query::OperatorId::UNKNOWN,
                mode: "WALK".to_string(),
                to_crs: "STP".to_string(),
                minutes: 6,
                valid_from: "0000".to_string(),
                valid_to: "2359".to_string(),
                days_mask: "1111111".to_string(),
            },
        )]);
        data
    }

    #[expect(clippy::too_many_arguments, reason = "a test fixture")]
    fn train(
        uid: &str,
        board: (&str, &str),
        alight: &str,
        sched_dep: u32,
        live_dep: u32,
        sched_arr: u32,
        live_arr: u32,
        served: bool,
    ) -> LegTiming {
        LegTiming::Train(TrainLegTiming {
            uid: uid.to_string(),
            date: date(),
            board_crs: board.0.to_string(),
            board_tiploc: board.1.to_string(),
            alight_crs: alight.to_string(),
            sched_dep: at(sched_dep),
            live_dep: at(live_dep),
            sched_arr: at(sched_arr),
            live_arr: at(live_arr),
            served,
        })
    }

    type Judged = Option<(Option<i64>, Option<u32>, ConnectionState)>;

    fn states(legs: &[LegTiming]) -> Vec<Judged> {
        evaluate_connections(legs, &interchange())
            .into_iter()
            .map(|c| c.map(|c| (c.gap_minutes, c.minimum_change_minutes, c.state)))
            .collect()
    }

    #[test]
    fn a_change_with_room_to_spare_is_ok_and_names_the_msn() {
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 600, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(
            states(&legs),
            vec![None, Some((Some(15), Some(5), ConnectionState::Ok))]
        );
    }

    #[test]
    fn a_late_arrival_inside_the_msn_is_missed_even_with_minutes_to_spare() {
        // 12 late into York: 3 minutes left, MSN 5.
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 612, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(
            states(&legs)[1],
            Some((Some(3), Some(5), ConnectionState::Missed))
        );
    }

    #[test]
    fn a_late_next_train_is_credited() {
        // 12 late in, but the next train is 10 late too.
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 612, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 625, 760, 770, true),
        ];
        assert_eq!(
            states(&legs)[1],
            Some((Some(13), Some(5), ConnectionState::Ok))
        );
    }

    #[test]
    fn delays_eating_into_the_slack_put_it_at_risk() {
        // 15 booked, 7 late: 8 left, 3 over the MSN.
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 607, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(states(&legs)[1].unwrap().2, ConnectionState::Ok);
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 608, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(states(&legs)[1].unwrap().2, ConnectionState::AtRisk);
    }

    #[test]
    fn a_change_booked_tight_is_at_risk_not_missed_until_it_gets_worse() {
        // Booked 6 minutes at Doncaster (MSN 8).
        let legs = [
            train("A1", ("KGX", "KNGX"), "DON", 480, 480, 580, 580, true),
            train("B1", ("DON", "DONC"), "SHF", 586, 586, 620, 620, true),
        ];
        assert_eq!(
            states(&legs)[1],
            Some((Some(6), Some(8), ConnectionState::AtRisk))
        );
        let legs = [
            train("A1", ("KGX", "KNGX"), "DON", 480, 480, 580, 582, true),
            train("B1", ("DON", "DONC"), "SHF", 586, 586, 620, 620, true),
        ];
        assert_eq!(states(&legs)[1].unwrap().2, ConnectionState::Missed);
    }

    #[test]
    fn a_change_made_only_by_a_late_next_train_is_ok_and_flagged() {
        // On time into York at 10:00 (MSN 5); the 10:03 is 12 late.
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 600, true),
            train("B1", ("YRK", "YORK"), "EDB", 603, 615, 760, 772, true),
        ];
        let connection = evaluate_connections(&legs, &interchange())[1]
            .clone()
            .unwrap();
        assert_eq!(connection.state, ConnectionState::Ok);
        assert_eq!(
            connection.relies_on_late_departure,
            Some(trip_plan_live::LateDepartureReliance {
                scheduled_departure: at(603),
                expected_departure: at(615),
                delay_minutes: 12,
                slack_if_on_time_minutes: -2,
            })
        );
        let json = serde_json::to_value(&connection).unwrap();
        assert_eq!(json["reliesOnLateDeparture"]["delayMinutes"], 12);
        assert_eq!(json["reliesOnLateDeparture"]["slackIfOnTimeMinutes"], -2);
    }

    #[test]
    fn a_late_next_train_is_flagged_only_when_its_booked_time_would_be_missed() {
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 612, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 625, 760, 770, true),
        ];
        let connections = evaluate_connections(&legs, &interchange());
        let connection = connections[1].as_ref().unwrap();
        // 12 late in: on time, the 10:15 would be missed, so it IS flagged.
        assert!(connection.relies_on_late_departure.is_some());
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 600, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 625, 760, 770, true),
        ];
        let connections = evaluate_connections(&legs, &interchange());
        let connection = connections[1].as_ref().unwrap();
        assert_eq!(connection.state, ConnectionState::Ok);
        assert_eq!(connection.relies_on_late_departure, None);
        let json = serde_json::to_value(connection).unwrap();
        assert!(json.get("reliesOnLateDeparture").is_none(), "{json}");
    }

    #[test]
    fn staying_aboard_a_late_train_is_never_flagged() {
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 610, true),
            train("A1", ("YRK", "YORK"), "EDB", 602, 612, 760, 770, true),
        ];
        let connections = evaluate_connections(&legs, &interchange());
        assert_eq!(
            connections[1].as_ref().unwrap().relies_on_late_departure,
            None
        );
    }

    #[test]
    fn staying_on_the_same_train_needs_no_change_time() {
        let legs = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 610, true),
            train("A1", ("YRK", "YORK"), "EDB", 602, 612, 760, 770, true),
        ];
        assert_eq!(
            states(&legs)[1],
            Some((Some(2), Some(0), ConnectionState::Ok))
        );
    }

    #[test]
    fn a_fixed_link_is_read_both_ways_and_adds_the_msn() {
        // Arrive St Pancras, walk to King's Cross (stored KGX -> STP): 6 + 5.
        let legs = [
            train("A1", ("SHF", "SHEFFLD"), "STP", 480, 480, 600, 600, true),
            train("B1", ("KGX", "KNGX"), "YRK", 615, 615, 730, 730, true),
        ];
        assert_eq!(
            states(&legs)[1],
            Some((Some(15), Some(11), ConnectionState::Ok))
        );
        // Five late into St Pancras: 10 minutes, the link needs 11.
        let legs = [
            train("A1", ("SHF", "SHEFFLD"), "STP", 480, 480, 600, 605, true),
            train("B1", ("KGX", "KNGX"), "YRK", 615, 615, 730, 730, true),
        ];
        assert_eq!(states(&legs)[1].unwrap().2, ConnectionState::Missed);
    }

    #[test]
    fn unjudgeable_changes_are_unknown() {
        // Two stations with no link, a no-interchange stand, a cancelled
        // train, a leg with no train.
        let no_link = [
            train("A1", ("SHF", "SHEFFLD"), "LDS", 480, 480, 600, 600, true),
            train("B1", ("YRK", "YORK"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(
            states(&no_link)[1],
            Some((Some(15), None, ConnectionState::Unknown))
        );
        let stand = [
            train("A1", ("SHF", "SHEFFLD"), "XBS", 480, 480, 600, 600, true),
            train("B1", ("XBS", "BUSSTND"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(states(&stand)[1].unwrap().2, ConnectionState::Unknown);
        let cancelled = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 600, false),
            train("B1", ("YRK", "YORK"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(states(&cancelled)[1].unwrap().2, ConnectionState::Unknown);
        let unpicked = [
            train("A1", ("KGX", "KNGX"), "YRK", 480, 480, 600, 600, true),
            LegTiming::Unknown,
            train("B1", ("YRK", "YORK"), "EDB", 615, 615, 760, 760, true),
        ];
        assert_eq!(
            states(&unpicked),
            vec![
                None,
                Some((None, None, ConnectionState::Unknown)),
                Some((None, None, ConnectionState::Unknown)),
            ]
        );
    }

    #[test]
    fn an_untracked_leg_shifts_and_the_next_train_is_judged_from_it() {
        // Arrive KGX 10 late at 10:10; a 15-minute Tube ride planned
        // 10:05-10:20 now ends 10:25; the 10:28 from Euston (MSN 5) is
        // missed, which the ride's own planned times would have hidden.
        let legs = [
            train("A1", ("YRK", "YORK"), "KGX", 480, 480, 600, 610, true),
            LegTiming::Untracked {
                departure: Some(at(605)),
                arrival: Some(at(620)),
                rides: Vec::new(),
            },
            train("B1", ("EUS", "EUSTON"), "MAN", 628, 628, 760, 760, true),
        ];
        assert_eq!(
            states(&legs),
            vec![
                None,
                Some((Some(-5), None, ConnectionState::Ok)),
                Some((Some(3), Some(5), ConnectionState::Missed)),
            ]
        );
    }

    /// A timetabled Tube leg planned 10:05-10:20, its line every five
    /// minutes from 10:05 (DS-237).
    fn timetabled_tube() -> LegTiming {
        LegTiming::Untracked {
            departure: Some(at(605)),
            arrival: Some(at(620)),
            rides: (0..6)
                .map(|n| TimetabledRide {
                    departure: at(605 + 5 * n),
                    arrival: at(620 + 5 * n),
                })
                .collect(),
        }
    }

    #[test]
    fn a_late_train_re_times_a_timetabled_leg_to_its_next_ride() {
        // Into KGX at 10:12, twelve late: the 10:05 Tube is gone, the next
        // is 10:15, in at 10:30, so the 10:33 from Euston (MSN 5) is
        // missed. Shifting the planned ride (10:12 to 10:27) would have
        // called it at risk.
        let legs = [
            train("A1", ("YRK", "YORK"), "KGX", 480, 480, 600, 612, true),
            timetabled_tube(),
            train("B1", ("EUS", "EUSTON"), "MAN", 633, 633, 760, 760, true),
        ];
        assert_eq!(
            states(&legs),
            vec![
                None,
                Some((Some(3), None, ConnectionState::Ok)),
                Some((Some(3), Some(5), ConnectionState::Missed)),
            ]
        );
        let connections = evaluate_connections(&legs, &interchange());
        assert_eq!(
            connections[1].as_ref().unwrap().next_timetabled,
            Some(TimetabledRide {
                departure: at(615),
                arrival: at(630),
            }),
            "the ride taken, not the planned one"
        );
        let (_, ready) = evaluate(&legs, &interchange());
        assert_eq!(ready[1], Some(at(630)));
    }

    #[test]
    fn on_time_a_timetabled_leg_is_its_planned_ride() {
        let legs = [
            train("A1", ("YRK", "YORK"), "KGX", 480, 480, 600, 600, true),
            timetabled_tube(),
            train("B1", ("EUS", "EUSTON"), "MAN", 633, 633, 760, 760, true),
        ];
        let connections = evaluate_connections(&legs, &interchange());
        let onto_tube = connections[1].as_ref().unwrap();
        assert_eq!(onto_tube.gap_minutes, Some(5));
        assert_eq!(
            onto_tube.next_timetabled, None,
            "the planned ride: nothing to say"
        );
        assert_eq!(
            connections[2].as_ref().unwrap().state,
            ConnectionState::Ok,
            "13 minutes from 10:20"
        );
    }

    #[test]
    fn past_its_last_ride_a_timetabled_leg_shifts_like_any_untracked_leg() {
        // In at 11:00: no ride read leaves that late, so the 15-minute
        // ride shifts to end 11:15.
        let legs = [
            train("A1", ("YRK", "YORK"), "KGX", 480, 480, 600, 660, true),
            timetabled_tube(),
            train("B1", ("EUS", "EUSTON"), "MAN", 700, 700, 760, 760, true),
        ];
        let (connections, ready) = evaluate(&legs, &interchange());
        assert_eq!(ready[1], Some(at(675)));
        assert_eq!(connections[1].as_ref().unwrap().next_timetabled, None);
    }

    fn stop(crs: &str, tiploc: &str, arr: Option<u32>, dep: Option<u32>) -> JourneyStop {
        let mut s = crate::data::journey::test_support::stop(
            crs,
            tiploc,
            CallingPointKind::Intermediate,
            None,
        );
        s.timetable.working_minute_arrival = arr.map(at);
        s.timetable.working_minute_departure = dep.map(at);
        s.stop_status = StopStatus::Scheduled;
        s
    }

    #[test]
    fn a_train_legs_timing_comes_from_its_own_calls_with_live_data_applied() {
        let stops = vec![
            stop("KGX", "KNGX", None, Some(480)),
            stop("PBO", "PBRO", Some(530), Some(531)),
            stop("YRK", "YORK", Some(600), Some(602)),
            stop("EDB", "EDINBUR", Some(760), None),
        ];
        let timing = train_leg_timing("A1", date(), &stops, "PBO", "YRK", None).unwrap();
        assert_eq!(timing.board_tiploc, "PBRO");
        assert_eq!((timing.sched_dep, timing.live_dep), (at(531), at(531)));
        assert_eq!((timing.sched_arr, timing.live_arr), (at(600), at(600)));
        assert!(timing.served);

        // 9 late leaving Peterborough: carried forward to York.
        let mut late = stops.clone();
        late[1].actual_departure = Some(at(540));
        late[1].last_event_type = Some("DEPARTURE".to_string());
        late[1].delay_minutes = Some(9);
        let live = trip_plan_live::train_live(trip_plan_live::TrainLiveInput {
            date: date(),
            stops: &late,
            status: Some("en_route"),
            state_updated_at: None,
            reasons: &[],
        });
        let timing = train_leg_timing("A1", date(), &stops, "PBO", "YRK", Some(&live)).unwrap();
        assert_eq!(timing.live_dep, at(540));
        assert_eq!(timing.live_arr, at(609));

        // Not calling at the destination after the origin: no timing.
        assert!(train_leg_timing("A1", date(), &stops, "YRK", "PBO", None).is_none());
    }
}
