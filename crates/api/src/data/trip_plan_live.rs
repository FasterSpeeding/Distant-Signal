//! The `/Trips/plan` live overlay: TRUST and Darwin (LDBWS board) facts
//! applied to a timetable plan, with bounded re-planning. See
//! docs/superpowers/specs/2026-09-28-trips-plan-live-overlay-design.md.
//!
//! Layers, from the bottom:
//!
//! 1. [`TrainLive`]: one train's live profile, built by [`train_live`] from
//!    the same `JourneyStop`s the train page shows
//!    (`journey::build_journey_stops_batch`) plus its TRUST status and
//!    `train_reasons` rows. Pure.
//! 2. [`evaluate_chain`]: that profile applied to the train's timetabled
//!    connections -- which calls still happen, and when. Pure.
//! 3. [`build_overlay`]: every changed train's connections replaced, as a
//!    `trip_planner::ConnectionOverlay`. Pure.
//! 4. [`annotate`]: each planned train leg gets its [`LegLive`], its
//!    timetable times restored, and each itinerary its `liveFeasible`. Pure.
//! 5. [`fetch_train_lives`]: the batched reads behind (1). The re-planning
//!    loop itself lives in `routes::trips`, which owns the blocking-pool
//!    and permit handling.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use schedule_query::{Connection, InterchangeData, normalize_tiploc};
use serde::Serialize;
use sqlx::PgPool;
use trip_planner::ConnectionOverlay;

use crate::data::journey::{JourneyStop, JourneyStopsRequest, StopStatus};
use crate::data::trip_planning_itinerary::{PlannedLeg, SegmentResult};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Tunables, from the environment (chart `api.tripPlanLive.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveConfig {
    /// Kill-switch, `TRIP_PLAN_LIVE_ENABLED` (default `true`).
    pub enabled: bool,
    /// Re-plans after the first plan for `results=fastest`,
    /// `TRIP_PLAN_LIVE_MAX_REPLANS` (3). CSA is cheap (80-150 ms with live).
    pub max_replans: u32,
    /// Re-plans for `results=options`, `TRIP_PLAN_LIVE_MAX_REPLANS_OPTIONS`
    /// (1). RAPTOR is the expensive pass (0.5-1.3 s with live); one round
    /// caps the worst case near 1 s (repo-owner decision, 2026-09-29).
    pub max_replans_options: u32,
    /// A TRUST delay whose `train_current_state` row was last updated longer
    /// ago than this is ignored, `TRIP_PLAN_LIVE_TRUST_MAX_AGE_MINUTES` (30).
    /// Board freshness is `stop_board::BOARD_FRESHNESS` (10 minutes).
    pub trust_max_age_minutes: i64,
    /// Legs booked to depart more than this far ahead get no live data,
    /// `TRIP_PLAN_LIVE_HORIZON_MINUTES` (180).
    pub horizon_minutes: i64,
    /// ...or more than this long ago, `TRIP_PLAN_LIVE_LOOKBACK_MINUTES` (120).
    pub lookback_minutes: i64,
    /// Most trains read per request, `TRIP_PLAN_LIVE_MAX_TRAINS` (60).
    pub max_trains: usize,
}

impl Default for LiveConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_replans: 3,
            max_replans_options: 1,
            trust_max_age_minutes: 30,
            horizon_minutes: 180,
            lookback_minutes: 120,
            max_trains: 60,
        }
    }
}

/// Trains booked to leave the origin in the hour before `departAfter` whose
/// live data is read up front: a late one may now be catchable.
pub const ORIGIN_LOOKBACK_MINUTES: u32 = 60;
/// At most this many of them.
pub const ORIGIN_LOOKBACK_TRAINS: usize = 20;

impl LiveConfig {
    /// The re-plan budget for a `results` mode (`"fastest"` or `"options"`).
    pub fn max_replans_for(&self, results: &str) -> u32 {
        if results == "options" {
            self.max_replans_options
        } else {
            self.max_replans
        }
    }

    pub fn from_env() -> Self {
        let default = Self::default();
        Self {
            enabled: env_bool("TRIP_PLAN_LIVE_ENABLED", default.enabled),
            max_replans: env_num("TRIP_PLAN_LIVE_MAX_REPLANS", default.max_replans),
            max_replans_options: env_num(
                "TRIP_PLAN_LIVE_MAX_REPLANS_OPTIONS",
                default.max_replans_options,
            ),
            trust_max_age_minutes: env_num(
                "TRIP_PLAN_LIVE_TRUST_MAX_AGE_MINUTES",
                default.trust_max_age_minutes,
            ),
            horizon_minutes: env_num("TRIP_PLAN_LIVE_HORIZON_MINUTES", default.horizon_minutes),
            lookback_minutes: env_num("TRIP_PLAN_LIVE_LOOKBACK_MINUTES", default.lookback_minutes),
            max_trains: env_num("TRIP_PLAN_LIVE_MAX_TRAINS", default.max_trains),
        }
    }
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name) {
        Err(_) => default,
        Ok(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => true,
            "false" | "0" | "no" => false,
            _ => {
                tracing::warn!(name, raw, default, "invalid boolean; using the default");
                default
            }
        },
    }
}

fn env_num<T: std::str::FromStr + std::fmt::Display + Copy>(name: &str, default: T) -> T {
    match std::env::var(name) {
        Err(_) => default,
        Ok(raw) => raw.trim().parse().unwrap_or_else(|_| {
            tracing::warn!(name, raw, %default, "invalid value; using the default");
            default
        }),
    }
}

// ---------------------------------------------------------------------------
// Time helpers
// ---------------------------------------------------------------------------

/// Minutes from London-local midnight of `date` to `at` (may exceed 1440,
/// like `schedule_query::Connection::departure_min`). `None` before it.
pub fn service_minute(date: NaiveDate, at: DateTime<Utc>) -> Option<u32> {
    let local = at.with_timezone(&chrono_tz::Europe::London).naive_local();
    let minutes = (local - date.and_time(NaiveTime::MIN)).num_minutes();
    u32::try_from(minutes).ok()
}

/// The instant `minute` minutes past London-local midnight of `date`.
pub fn service_instant(date: NaiveDate, minute: u32) -> Option<DateTime<Utc>> {
    let naive = date.and_time(NaiveTime::MIN) + Duration::minutes(i64::from(minute));
    crate::data::eta_blend::london_to_utc(naive)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::expect_used,
    reason = "minute and day-offset values are bounded by a service day or two; a constant or range-checked time is always valid"
)]
fn minutes_to_clock(total_minutes: u32) -> (NaiveTime, u8) {
    (
        NaiveTime::from_num_seconds_from_midnight_opt((total_minutes % 1440) * 60, 0)
            .expect("minutes modulo 1440 is a valid clock time"),
        (total_minutes / 1440) as u8,
    )
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "clamped to >= 0; delays in minutes are far below i32::MAX"
)]
fn delay_between(scheduled: DateTime<Utc>, observed: DateTime<Utc>) -> i32 {
    (observed - scheduled).num_minutes().max(0) as i32
}

// ---------------------------------------------------------------------------
// 1. One train's live profile
// ---------------------------------------------------------------------------

/// What is known about one call of a train.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StopFact {
    /// `false` when Darwin or TRUST says the train no longer calls here
    /// (`StopStatus::Skipped`) or the fresh board row says it is cancelled
    /// at this station.
    pub served: bool,
    /// Minutes late departing (>= 0): TRUST actual, else Darwin `etd`, else
    /// the TRUST-propagated estimate.
    pub dep_delay: Option<i32>,
    /// Minutes late arriving (>= 0): TRUST actual, else the TRUST estimate.
    pub arr_delay: Option<i32>,
    pub departed: bool,
    /// Darwin reports the service as `Delayed` here, with no time.
    pub delayed_unknown: bool,
    /// Live platform, only from a fresh, uniquely matched board row.
    pub platform: Option<String>,
    pub delay_reason: Option<String>,
    pub cancel_reason: Option<String>,
    /// The matched board's poll time.
    pub observed_at: Option<DateTime<Utc>>,
}

/// One train's live profile. See the module doc and [`train_live`].
#[derive(Debug, Clone, Default)]
pub struct TrainLive {
    facts: Vec<StopFact>,
    /// `(normalized TIPLOC, booked minute)` -> index into `facts`, for both
    /// the booked arrival and departure minute of each call.
    index: HashMap<(String, u32), usize>,
    /// Cancelled outright (every call not yet reached).
    whole_cancelled: bool,
    /// Cut short: no call after this booked minute (EN ROUTE / OUT OF PLAN).
    last_served_min: Option<u32>,
    /// Starts late: no call before this booked minute (change of origin).
    first_served_min: Option<u32>,
    /// Latest booked minute at which TRUST reported the train.
    last_reported_min: Option<u32>,
    /// TRUST cancellation reason text (glossary), when cancelled.
    trust_cancel_reason: Option<String>,
    /// When TRUST last updated this train's state.
    trust_observed_at: Option<DateTime<Utc>>,
}

/// A `train_reasons` row, with its location resolved to a TIPLOC.
#[derive(Debug, Clone, PartialEq, Eq, sqlx::FromRow)]
pub struct ReasonRow {
    pub msg_type: String,
    pub reason_code: String,
    pub canx_type: Option<String>,
    pub tiploc: Option<String>,
}

/// Inputs to [`train_live`].
#[derive(Debug, Clone, Copy)]
pub struct TrainLiveInput<'a> {
    pub date: NaiveDate,
    /// After `build_journey_stops_batch` (movements, skips, boards, TRUST
    /// estimates -- the latter only when the TRUST state was fresh).
    pub stops: &'a [JourneyStop],
    /// `train_current_state.status`; `None` when there is no state row.
    pub status: Option<&'a str>,
    pub state_updated_at: Option<DateTime<Utc>>,
    pub reasons: &'a [ReasonRow],
}

/// Builds a [`TrainLive`]. Rules (design doc §4.2):
/// - a whole-train cancellation is `status = cancelled`, or, with no state
///   row, a `0002` reason row;
/// - an `EN ROUTE`/`OUT OF PLAN` cancellation whose location is one of the
///   train's calls cuts the train after that call instead, unless TRUST
///   reported it at a later call (it ran on);
/// - a `0006` change of origin at one of the calls removes the calls before
///   it, unless TRUST reported the train before it.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear; splitting it would scatter its shared state across helpers"
)]
pub fn train_live(input: TrainLiveInput<'_>) -> TrainLive {
    let date = input.date;
    let mut live = TrainLive {
        trust_observed_at: input.state_updated_at,
        ..TrainLive::default()
    };
    // Booked minute of each call (arrival where it has one, else
    // departure), for the ordering rules.
    let mut call_minutes: Vec<(String, Option<u32>, Option<u32>)> = Vec::new();

    for stop in input.stops {
        let sched_arr = stop
            .scheduled_arrival
            .and_then(|at| service_minute(date, at));
        let sched_dep = stop
            .scheduled_departure
            .and_then(|at| service_minute(date, at));
        let tiploc = stop
            .tiploc
            .as_deref()
            .map(|t| normalize_tiploc(t).to_ascii_uppercase())
            .unwrap_or_default();
        let board = stop.board.as_ref();
        let board_cancelled =
            board.is_some_and(|b| b.is_cancelled) && stop.actual_departure.is_none();
        // Every delay here is against the PUBLIC time, the time the
        // connections are planned on (`schedule_query::Connection`), and each
        // pairs like with like:
        // - a reported call: the stop's own `delay_minutes`, which the
        //   journey overlay measured from that TRUST report's own fields
        //   (actual against its gbtt/planned, see `journey`), never TRUST's
        //   actual against a CIF time;
        // - Darwin's board: `etd - std`, both public;
        // - an estimate: the working time plus TRUST's running delay, against
        //   the public time, both CIF-derived.
        let public_departure = stop.timetable.public_departure.or(stop.scheduled_departure);
        let public_arrival = stop.timetable.public_arrival.or(stop.scheduled_arrival);
        let reported = |side: &str| {
            (stop.last_event_type.as_deref() == Some(side)
                || stop.last_event_type.as_deref() == Some("PASS"))
            .then_some(stop.delay_minutes)
            .flatten()
            .map(|d| d.max(0))
        };
        let dep_delay = if stop.actual_departure.is_some() {
            reported("DEPARTURE")
        } else {
            board
                .filter(|b| !b.is_cancelled)
                .and_then(|b| b.delay_minutes)
                .map(|d| d.max(0))
                .or_else(|| {
                    public_departure
                        .zip(stop.estimated_departure)
                        .map(|(s, e)| delay_between(s, e))
                })
        };
        let arr_delay = if stop.actual_arrival.is_some() {
            reported("ARRIVAL")
        } else {
            public_arrival
                .zip(stop.estimated_arrival)
                .map(|(s, e)| delay_between(s, e))
        };
        let fact = StopFact {
            served: stop.stop_status != StopStatus::Skipped && !board_cancelled,
            dep_delay,
            arr_delay,
            departed: stop.actual_departure.is_some(),
            delayed_unknown: board
                .is_some_and(|b| !b.is_cancelled && b.estimated.eq_ignore_ascii_case("Delayed")),
            platform: board.and(stop.platform.clone()),
            delay_reason: board.and_then(|b| b.delay_reason.clone()),
            cancel_reason: board.and_then(|b| b.cancel_reason.clone()),
            observed_at: board.map(|b| b.observed_at),
        };
        let index = live.facts.len();
        live.facts.push(fact);
        for minute in [sched_arr, sched_dep].into_iter().flatten() {
            live.index.entry((tiploc.clone(), minute)).or_insert(index);
        }
        if stop.actual_arrival.is_some() || stop.actual_departure.is_some() {
            let reported = sched_dep.or(sched_arr);
            live.last_reported_min = live.last_reported_min.max(reported);
        }
        call_minutes.push((tiploc, sched_arr, sched_dep));
    }

    let call_at = |tiploc: &str, prefer_departure: bool| -> Option<u32> {
        let wanted = normalize_tiploc(tiploc).to_ascii_uppercase();
        call_minutes
            .iter()
            .find(|(t, _, _)| *t == wanted)
            .and_then(|(_, arr, dep)| {
                if prefer_departure {
                    dep.or(*arr)
                } else {
                    arr.or(*dep)
                }
            })
    };

    let cancellation = input.reasons.iter().find(|r| r.msg_type == "0002");
    let status_cancelled = input.status == Some("cancelled");
    match cancellation {
        Some(reason) => {
            let partial = matches!(
                reason.canx_type.as_deref().map(str::trim),
                Some("EN ROUTE" | "OUT OF PLAN")
            );
            let cut = partial
                .then(|| reason.tiploc.as_deref().and_then(|t| call_at(t, false)))
                .flatten();
            match cut {
                Some(cut) => {
                    let ran_on = live.last_reported_min.is_some_and(|r| r > cut);
                    if !ran_on {
                        live.last_served_min = Some(cut);
                    }
                }
                None => {
                    live.whole_cancelled = if input.status.is_some() {
                        status_cancelled
                    } else {
                        true
                    };
                }
            }
            if live.whole_cancelled || live.last_served_min.is_some() {
                live.trust_cancel_reason =
                    crate::data::train_reasons::reason_text(reason.reason_code.trim())
                        .map(str::to_string);
            }
        }
        None => live.whole_cancelled = status_cancelled,
    }
    if let Some(new_origin) = input
        .reasons
        .iter()
        .find(|r| r.msg_type == "0006")
        .and_then(|r| r.tiploc.as_deref())
        .and_then(|t| call_at(t, true))
    {
        let reported_before = live.last_reported_min.is_some_and(|r| r < new_origin);
        if !reported_before {
            live.first_served_min = Some(new_origin);
        }
    }
    live
}

impl TrainLive {
    fn fact(&self, tiploc: &str, minutes: [Option<u32>; 2]) -> Option<&StopFact> {
        let key = normalize_tiploc(tiploc).to_ascii_uppercase();
        minutes
            .into_iter()
            .flatten()
            .find_map(|m| self.index.get(&(key.clone(), m)))
            .map(|&i| &self.facts[i])
    }

    /// Whether the call booked at `arrival`/`departure` still happens. The
    /// cut and new-origin minutes are the call's own booked minutes, so a
    /// call is compared by its earlier minute against a cut and by its later
    /// one against a new origin.
    fn served(
        &self,
        arrival: Option<u32>,
        departure: Option<u32>,
        fact: Option<&StopFact>,
    ) -> bool {
        if fact.is_some_and(|f| !f.served) {
            return false;
        }
        let (Some(earliest), Some(latest)) = (arrival.or(departure), departure.or(arrival)) else {
            return true;
        };
        if self.last_reported_min.is_some_and(|r| earliest <= r) {
            // Already reached: history, not a plan.
            return true;
        }
        !(self.whole_cancelled
            || self.last_served_min.is_some_and(|cut| earliest > cut)
            || self.first_served_min.is_some_and(|origin| latest < origin))
    }

    /// Whether anything in this profile is known at all (a delay, a
    /// cancellation or a skip) -- a train with nothing known has no `live`.
    pub fn has_data(&self) -> bool {
        self.whole_cancelled
            || self.last_served_min.is_some()
            || self.first_served_min.is_some()
            || self.last_reported_min.is_some()
            || self.facts.iter().any(|f| {
                !f.served || f.dep_delay.is_some() || f.arr_delay.is_some() || f.delayed_unknown
            })
    }
}

// ---------------------------------------------------------------------------
// 2. A train's timetabled connections under its profile
// ---------------------------------------------------------------------------

/// One call of a train's connection chain, timetabled and live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainPoint {
    /// As in the connections array (possibly padded).
    pub tiploc: String,
    /// Timetabled minutes as planned on: public, else working (see
    /// `schedule_query::Connection::departure_min`).
    pub sched_arr: Option<u32>,
    pub sched_dep: Option<u32>,
    /// The same calls' working-timetable minutes: what TRUST and Darwin
    /// facts are looked up by, and what a leg's `scheduled*` serve.
    pub work_arr: Option<u32>,
    pub work_dep: Option<u32>,
    pub live_arr: Option<u32>,
    pub live_dep: Option<u32>,
    pub dep_delay: Option<i32>,
    pub arr_delay: Option<i32>,
    pub served: bool,
    /// The call's direction flags, carried from the connections (see
    /// `schedule_query::Connection::can_board`/`can_alight`), so a
    /// replacement connection keeps them.
    pub can_board: bool,
    pub can_alight: bool,
}

/// One call while [`evaluate_chain`] reassembles a chain: its TIPLOC, its
/// timetabled arrival and departure minutes, and whether a passenger may
/// board and alight there.
struct ChainCall {
    tiploc: String,
    sched_arr: Option<u32>,
    sched_dep: Option<u32>,
    work_arr: Option<u32>,
    work_dep: Option<u32>,
    can_board: bool,
    can_alight: bool,
}

/// The calls of one train's connections (`chain` sorted by departure, as the
/// day array is), with [`TrainLive`] applied: which are still served, and
/// their live times. Delays propagate forward from the last call with a
/// known delay, never backward; a call before any known delay keeps its
/// timetable time. Live times are made monotone along the train.
#[expect(
    clippy::cast_sign_loss,
    reason = "the value is clamped to >= 0 first, and minute values fit easily"
)]
pub fn evaluate_chain(chain: &[&Connection], live: &TrainLive) -> Vec<ChainPoint> {
    let mut calls: Vec<ChainCall> = Vec::new();
    for connection in chain {
        match calls.last_mut() {
            Some(last)
                if normalize_tiploc(&last.tiploc) == normalize_tiploc(&connection.from_tiploc)
                    && last.sched_dep.is_none() =>
            {
                last.sched_dep = Some(connection.departure_min);
                last.work_dep = Some(connection.working_departure_min);
                last.can_board = connection.can_board;
            }
            _ => calls.push(ChainCall {
                tiploc: connection.from_tiploc.clone(),
                sched_arr: None,
                sched_dep: Some(connection.departure_min),
                work_arr: None,
                work_dep: Some(connection.working_departure_min),
                can_board: connection.can_board,
                can_alight: true,
            }),
        }
        calls.push(ChainCall {
            tiploc: connection.to_tiploc.clone(),
            sched_arr: Some(connection.arrival_min),
            sched_dep: None,
            work_arr: Some(connection.working_arrival_min),
            work_dep: None,
            can_board: true,
            can_alight: connection.can_alight,
        });
    }

    let mut carry: Option<i32> = None;
    let mut floor = 0u32;
    calls
        .into_iter()
        .map(|call| {
            let ChainCall {
                tiploc,
                sched_arr,
                sched_dep,
                work_arr,
                work_dep,
                can_board,
                can_alight,
            } = call;
            let fact = live.fact(&tiploc, [work_arr, work_dep]);
            let known_arr = fact.and_then(|f| f.arr_delay);
            let arr_delay = known_arr.or(carry);
            if known_arr.is_some() {
                carry = known_arr;
            }
            let known_dep = fact.and_then(|f| f.dep_delay);
            let dep_delay = known_dep.or(carry);
            if known_dep.is_some() {
                carry = known_dep;
            }
            let live_arr = sched_arr.map(|a| {
                let t = (a + arr_delay.unwrap_or(0).max(0) as u32).max(floor);
                floor = t;
                t
            });
            let live_dep = sched_dep.map(|d| {
                let t = (d + dep_delay.unwrap_or(0).max(0) as u32).max(floor);
                floor = t;
                t
            });
            ChainPoint {
                served: live.served(work_arr, work_dep, fact),
                tiploc,
                sched_arr,
                sched_dep,
                work_arr,
                work_dep,
                live_arr,
                live_dep,
                dep_delay: sched_dep.and(dep_delay),
                arr_delay: sched_arr.and(arr_delay),
                can_board,
                can_alight,
            }
        })
        .collect()
}

/// The connections `points` leave in the graph: one between each pair of
/// consecutive served calls, at live times. `None` when that is exactly the
/// timetable (nothing to replace).
pub fn adjusted_connections(uid: &str, points: &[ChainPoint]) -> Option<Vec<Connection>> {
    let unchanged = points
        .iter()
        .all(|p| p.served && p.live_arr == p.sched_arr && p.live_dep == p.sched_dep);
    if unchanged {
        return None;
    }
    let served: Vec<&ChainPoint> = points.iter().filter(|p| p.served).collect();
    Some(
        served
            .windows(2)
            .filter_map(|pair| {
                let (from, to) = (pair[0], pair[1]);
                Some(Connection {
                    uid: uid.to_string(),
                    from_tiploc: from.tiploc.clone(),
                    to_tiploc: to.tiploc.clone(),
                    departure_min: from.live_dep?,
                    arrival_min: to.live_arr?,
                    working_departure_min: from.work_dep?,
                    working_arrival_min: to.work_arr?,
                    can_board: from.can_board,
                    can_alight: to.can_alight,
                })
            })
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// 3. The overlay
// ---------------------------------------------------------------------------

/// Every train's timetabled connections, for the UIDs in `uids`, in day
/// order -- one pass over the day array.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always use the default hasher"
)]
pub fn chains_for(
    connections: &[Connection],
    uids: &HashSet<String>,
) -> HashMap<String, Vec<Connection>> {
    let mut chains: HashMap<String, Vec<Connection>> = HashMap::new();
    if uids.is_empty() {
        return chains;
    }
    for connection in connections {
        if uids.contains(&connection.uid) {
            chains
                .entry(connection.uid.clone())
                .or_default()
                .push(connection.clone());
        }
    }
    chains
}

/// The overlay for every train in `lives` whose profile changes its
/// connections, and how many timetabled connections it withdraws.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always use the default hasher"
)]
pub fn build_overlay(
    chains: &HashMap<String, Vec<Connection>>,
    lives: &HashMap<String, TrainLive>,
) -> (ConnectionOverlay, usize) {
    let mut replaced: Vec<String> = Vec::new();
    let mut replacements: Vec<Connection> = Vec::new();
    let mut withdrawn = 0;
    for (uid, live) in lives {
        let Some(chain) = chains.get(uid) else {
            continue;
        };
        let refs: Vec<&Connection> = chain.iter().collect();
        if let Some(adjusted) = adjusted_connections(uid, &evaluate_chain(&refs, live)) {
            replaced.push(uid.clone());
            withdrawn += chain.len();
            replacements.extend(adjusted);
        }
    }
    (ConnectionOverlay::new(replaced, replacements), withdrawn)
}

// ---------------------------------------------------------------------------
// 4. Annotation
// ---------------------------------------------------------------------------

/// A train leg's live status, served as the leg's `live`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LegLive {
    /// `Cancelled`, `Departed`, `Late`, `Delayed`, `OnTime` or `Scheduled`
    /// -- the `stop_live_status` vocabulary, for the boarding call.
    pub status: &'static str,
    /// The train does not call at the boarding or alighting stop today.
    pub cancelled: bool,
    /// Minutes late departing the boarding stop, when known or propagated.
    pub delay_minutes: Option<i32>,
    /// Minutes late arriving at the alighting stop, when known or propagated.
    pub arrival_delay_minutes: Option<i32>,
    /// Darwin's cancel/delay text, else TRUST's cancellation reason.
    pub reason: Option<String>,
    /// `darwin` or `trust`, when `reason` is set.
    pub reason_source: Option<&'static str>,
    /// Live platform at the boarding stop (fresh board only).
    pub platform: Option<String>,
    /// When the newest fact behind this was observed.
    pub observed_at: Option<DateTime<Utc>>,
    /// Whether the change onto this leg from the previous train leg still
    /// works on live times; `None` for the first train leg.
    pub interchange_feasible: Option<bool>,
}

/// Everything [`annotate`] needs.
pub struct LiveContext<'a> {
    pub date: NaiveDate,
    pub now: DateTime<Utc>,
    pub config: &'a LiveConfig,
    pub interchange: &'a InterchangeData,
    pub chains: &'a HashMap<String, Vec<Connection>>,
    pub lives: &'a HashMap<String, TrainLive>,
    /// UIDs the searches saw replaced (their leg minutes are live times).
    pub overlaid: &'a HashSet<String>,
}

/// Whether a leg booked to depart at `minute` is inside the live window.
pub fn within_horizon(
    date: NaiveDate,
    minute: u32,
    now: DateTime<Utc>,
    config: &LiveConfig,
) -> bool {
    service_instant(date, minute).is_some_and(|at| {
        at >= now - Duration::minutes(config.lookback_minutes)
            && at <= now + Duration::minutes(config.horizon_minutes)
    })
}

/// Train UIDs of `segments`' legs inside the live window.
pub fn uids_in_window(
    segments: &[SegmentResult],
    date: NaiveDate,
    now: DateTime<Utc>,
    config: &LiveConfig,
) -> Vec<String> {
    let mut uids: Vec<String> = segments
        .iter()
        .flat_map(|s| &s.itineraries)
        .flat_map(|i| &i.legs)
        .filter_map(|leg| match leg {
            PlannedLeg::Train {
                train_uid,
                departure_min,
                ..
            } if within_horizon(date, *departure_min, now, config) => Some(train_uid.clone()),
            _ => None,
        })
        .collect();
    uids.sort();
    uids.dedup();
    uids
}

/// UIDs booked to leave any of `origin_tiplocs` in the hour before
/// `depart_after_min` (at most [`ORIGIN_LOOKBACK_TRAINS`], latest first).
pub fn origin_lookback_uids(
    connections: &[Connection],
    origin_tiplocs: &[String],
    depart_after_min: u32,
) -> Vec<String> {
    let from = depart_after_min.saturating_sub(ORIGIN_LOOKBACK_MINUTES);
    let start = connections.partition_point(|c| c.departure_min < from);
    let end = connections.partition_point(|c| c.departure_min < depart_after_min);
    let origins: HashSet<&str> = origin_tiplocs.iter().map(|t| normalize_tiploc(t)).collect();
    let mut uids: Vec<String> = Vec::new();
    for connection in connections[start..end].iter().rev() {
        if origins.contains(normalize_tiploc(&connection.from_tiploc))
            && !uids.contains(&connection.uid)
        {
            uids.push(connection.uid.clone());
            if uids.len() == ORIGIN_LOOKBACK_TRAINS {
                break;
            }
        }
    }
    uids
}

/// Whether an [`annotate`]d plan no longer works on live times: an
/// itinerary with a cancelled leg or an impossible change, or a journey
/// whose change at a waypoint no longer works on live times (for a chained
/// plan: an onward segment searched from before the previous segment's live
/// arrival plus change time). Staying aboard through a waypoint is always
/// fine. This is what triggers a
/// re-plan; a plan that is merely late is annotated, not re-planned.
pub fn plan_invalidated(segments: &[SegmentResult], interchange: &InterchangeData) -> bool {
    if segments
        .iter()
        .flat_map(|s| &s.itineraries)
        .any(|i| i.live_feasible == Some(false))
    {
        return true;
    }
    segments.windows(2).any(|pair| {
        let (previous, next) = (&pair[0].itineraries, &pair[1].itineraries);
        if previous.len() == next.len() {
            // A joint plan's aligned journeys (`plan_trip`): each journey's
            // own change at the waypoint.
            return previous.iter().zip(next).any(|(previous, next)| {
                next.departure_min
                    < crate::data::trip_planning_itinerary::waypoint_ready_min(
                        previous,
                        next,
                        interchange,
                    )
            });
        }
        let ready = crate::data::trip_planning_itinerary::chain_ready_min(previous, interchange);
        let earliest_onward = next.iter().map(|i| i.departure_min).min();
        matches!((ready, earliest_onward), (Some(ready), Some(onward)) if onward < ready)
    })
}

/// Whether any of `seeds` (trains booked to leave the origin before
/// `depart_after_min`) now leaves one of `origin_tiplocs` at or after it on
/// live times -- a late train that has become catchable, worth a re-plan.
#[expect(
    clippy::implicit_hasher,
    reason = "callers always use the default hasher"
)]
pub fn seed_became_catchable(
    seeds: &[String],
    chains: &HashMap<String, Vec<Connection>>,
    lives: &HashMap<String, TrainLive>,
    origin_tiplocs: &[String],
    depart_after_min: u32,
) -> bool {
    let origins: HashSet<&str> = origin_tiplocs.iter().map(|t| normalize_tiploc(t)).collect();
    seeds.iter().any(|uid| {
        let (Some(chain), Some(live)) = (chains.get(uid), lives.get(uid)) else {
            return false;
        };
        let refs: Vec<&Connection> = chain.iter().collect();
        evaluate_chain(&refs, live).iter().any(|p| {
            p.served
                && origins.contains(normalize_tiploc(&p.tiploc))
                && p.sched_dep.is_some_and(|d| d < depart_after_min)
                && p.live_dep.is_some_and(|d| d >= depart_after_min)
        })
    })
}

fn change_minutes(interchange: &InterchangeData, tiploc: &str) -> u32 {
    match schedule_query::minimum_change_time(interchange, tiploc) {
        schedule_query::ChangeTime::Finite(minutes) => minutes,
        schedule_query::ChangeTime::NoInterchange => 0,
    }
}

/// Fills every train leg's `live` (inside the window: `Some(None)` when
/// nothing is known, `Some(Some(..))` otherwise), restores its timetable
/// times where the search saw live ones, sets `interchangeFeasible` and each
/// itinerary's `liveFeasible` -- `false` for a cancelled leg, an impossible
/// change, or (arrive-by) a live arrival after the segment's `arriveBy`.
#[expect(
    clippy::cast_sign_loss,
    clippy::too_many_lines,
    reason = "the value is clamped to >= 0 first, and minute values fit easily; long but linear; splitting it would scatter its shared state across helpers"
)]
pub fn annotate(segments: &mut [SegmentResult], ctx: &LiveContext<'_>) {
    for segment in segments.iter_mut() {
        for itinerary in &mut segment.itineraries {
            let mut feasible = true;
            // Live arrival of the previous train leg, plus transfer minutes
            // walked since.
            let mut previous_arrival: Option<u32> = None;
            for leg in &mut itinerary.legs {
                match leg {
                    PlannedLeg::Transfer { minutes, .. } => {
                        if let Some(arrival) = previous_arrival.as_mut() {
                            *arrival += (*minutes).max(0) as u32;
                        }
                    }
                    PlannedLeg::Train {
                        train_uid,
                        from_tiploc,
                        to_tiploc,
                        departure_min,
                        arrival_min,
                        scheduled_departure,
                        scheduled_arrival,
                        departure_day_offset,
                        arrival_day_offset,
                        live,
                        ..
                    } => {
                        let in_overlay = ctx.overlaid.contains(train_uid.as_str());
                        let points = ctx.lives.get(train_uid.as_str()).and_then(|train_live| {
                            let chain = ctx.chains.get(train_uid.as_str())?;
                            let refs: Vec<&Connection> = chain.iter().collect();
                            Some((train_live, evaluate_chain(&refs, train_live)))
                        });
                        let matched = points.as_ref().and_then(|(train_live, points)| {
                            let pick = |tiploc: &str, minute: u32, departing: bool| {
                                points.iter().find(|p| {
                                    normalize_tiploc(&p.tiploc) == normalize_tiploc(tiploc)
                                        && if departing {
                                            (if in_overlay { p.live_dep } else { p.sched_dep })
                                                == Some(minute)
                                        } else {
                                            (if in_overlay { p.live_arr } else { p.sched_arr })
                                                == Some(minute)
                                        }
                                })
                            };
                            let board = pick(from_tiploc, *departure_min, true)?;
                            let alight = pick(to_tiploc, *arrival_min, false)?;
                            Some((*train_live, board.clone(), alight.clone()))
                        });

                        // The leg's own timetable and live minutes.
                        let (sched_dep, live_dep, live_arr) = match &matched {
                            Some((_, board, alight)) => (
                                board.sched_dep.unwrap_or(*departure_min),
                                board.live_dep.unwrap_or(*departure_min),
                                alight.live_arr.unwrap_or(*arrival_min),
                            ),
                            None => (*departure_min, *departure_min, *arrival_min),
                        };
                        // `scheduled*` stay working-timetable times for one
                        // release (design doc §9 decision 1).
                        if in_overlay
                            && let Some((_, board, alight)) = &matched
                            && let (Some(work_dep), Some(work_arr)) =
                                (board.work_dep, alight.work_arr)
                        {
                            (*scheduled_departure, *departure_day_offset) =
                                minutes_to_clock(work_dep);
                            (*scheduled_arrival, *arrival_day_offset) = minutes_to_clock(work_arr);
                        }
                        *departure_min = live_dep;
                        *arrival_min = live_arr;

                        let interchange_feasible = previous_arrival.map(|arrival| {
                            live_dep >= arrival + change_minutes(ctx.interchange, from_tiploc)
                        });
                        previous_arrival = Some(live_arr);

                        if !within_horizon(ctx.date, sched_dep, ctx.now, ctx.config) {
                            *live = Some(None);
                            continue;
                        }
                        let leg_live = matched.and_then(|(train_live, board, alight)| {
                            if !train_live.has_data() {
                                return None;
                            }
                            let fact = train_live.fact(&board.tiploc, [board.work_dep, None]);
                            let cancelled = !board.served || !alight.served;
                            let (reason, reason_source) = if cancelled {
                                match fact.and_then(|f| f.cancel_reason.clone()) {
                                    Some(text) => (Some(text), Some("darwin")),
                                    None => match train_live.trust_cancel_reason.clone() {
                                        Some(text) => (Some(text), Some("trust")),
                                        None => (None, None),
                                    },
                                }
                            } else {
                                match fact.and_then(|f| f.delay_reason.clone()) {
                                    Some(text) => (Some(text), Some("darwin")),
                                    None => (None, None),
                                }
                            };
                            let status = if cancelled {
                                "Cancelled"
                            } else if fact.is_some_and(|f| f.departed) {
                                "Departed"
                            } else if fact.is_some_and(|f| f.delayed_unknown) {
                                "Delayed"
                            } else {
                                match board.dep_delay {
                                    Some(d) if d >= 1 => "Late",
                                    Some(_) => "OnTime",
                                    None => "Scheduled",
                                }
                            };
                            Some(LegLive {
                                status,
                                cancelled,
                                delay_minutes: board.dep_delay,
                                arrival_delay_minutes: alight.arr_delay,
                                reason,
                                reason_source,
                                platform: fact.and_then(|f| f.platform.clone()),
                                observed_at: fact
                                    .and_then(|f| f.observed_at)
                                    .max(train_live.trust_observed_at),
                                interchange_feasible,
                            })
                        });
                        if leg_live.as_ref().is_some_and(|l| l.cancelled)
                            || interchange_feasible == Some(false)
                        {
                            feasible = false;
                        }
                        *live = Some(leg_live.map(Box::new));
                    }
                }
            }
            // Live start and end: a leading train's live departure (a
            // leading walk starts when it started), and the last train's
            // live arrival plus any walk after it. Unchanged when the
            // itinerary has no train leg.
            let mut last_arrival = None;
            for leg in &itinerary.legs {
                match leg {
                    PlannedLeg::Transfer { minutes, .. } => {
                        if let Some(arrival) = last_arrival.as_mut() {
                            *arrival += (*minutes).max(0) as u32;
                        }
                    }
                    PlannedLeg::Train { arrival_min, .. } => last_arrival = Some(*arrival_min),
                }
            }
            if let Some(PlannedLeg::Train { departure_min, .. }) = itinerary.legs.first() {
                itinerary.departure_min = *departure_min;
            }
            if let Some(arrival) = last_arrival {
                itinerary.arrival_min = arrival.max(itinerary.departure_min);
                itinerary.total_duration_minutes = itinerary.arrival_min - itinerary.departure_min;
            }
            // Arrive-by: a live arrival after the segment's deadline misses
            // it (for an earlier waypoint segment, the deadline is the latest
            // arrival that still makes an onward itinerary).
            if segment
                .arrive_by_min
                .is_some_and(|deadline| itinerary.arrival_min > deadline)
            {
                feasible = false;
            }
            itinerary.live_feasible = Some(feasible);
        }
    }
}

// ---------------------------------------------------------------------------
// 5. Reads
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct TrainRow {
    trains_id: i64,
    train_uid: String,
    skipped_stations: Vec<String>,
    platform: Option<String>,
    planned_platform: Option<String>,
    status: Option<String>,
    delay_minutes: Option<i32>,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, sqlx::FromRow)]
struct CallingPointRow {
    uid: String,
    tiploc: String,
    kind: String,
    booked_arrival: Option<NaiveTime>,
    booked_departure: Option<NaiveTime>,
    day_offset: i16,
    platform: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct ReasonDbRow {
    trains_id: i64,
    #[sqlx(flatten)]
    reason: ReasonRow,
}

/// The `trains.calling_points` JSON shape (`schedule_matching`'s
/// `ScheduleCallingPointDto`) for rows of `schedule_calling_points_full`,
/// so `build_journey_stops_batch` gets every train's calling points from
/// this one batched read instead of one fallback query per train (feed
/// created `trains` rows carry no `calling_points`).
fn calling_points_json(rows: &[&CallingPointRow]) -> serde_json::Value {
    serde_json::Value::Array(
        rows.iter()
            .filter_map(|row| {
                let kind = match row.kind.as_str() {
                    "origin" => schedule_query::CallingPointKind::Origin,
                    "intermediate" => schedule_query::CallingPointKind::Intermediate,
                    "terminate" => schedule_query::CallingPointKind::Terminate,
                    _ => return None,
                };
                Some(serde_json::json!({
                    "tiploc": row.tiploc,
                    "kind": kind,
                    "bookedArrival": row.booked_arrival,
                    "bookedDeparture": row.booked_departure,
                    "dayOffset": u8::try_from(row.day_offset).unwrap_or(0),
                    "platform": row.platform,
                }))
            })
            .collect(),
    )
}

/// Reads the live profile of every train in `uids` on `date`: one query for
/// the `trains`/`train_current_state` rows, one for their calling points,
/// one for `train_reasons`, then `build_journey_stops_batch`. A train with
/// no `trains` row is absent. Never writes.
pub async fn fetch_train_lives(
    pool: &PgPool,
    date: NaiveDate,
    uids: &[String],
    config: &LiveConfig,
    now: DateTime<Utc>,
) -> anyhow::Result<HashMap<String, TrainLive>> {
    if uids.is_empty() {
        return Ok(HashMap::new());
    }
    let trains: Vec<TrainRow> = sqlx::query_as(
        "SELECT tr.id AS trains_id, tr.train_uid, tr.skipped_stations, tr.platform, \
                tr.planned_platform, cs.status, cs.delay_minutes, cs.updated_at \
         FROM trains tr \
         LEFT JOIN train_current_state cs ON cs.trains_id = tr.id \
         WHERE tr.service_date = $1 AND tr.train_uid = ANY($2)",
    )
    .bind(date)
    .bind(uids)
    .fetch_all(pool)
    .await?;
    if trains.is_empty() {
        return Ok(HashMap::new());
    }
    let found: Vec<&str> = trains.iter().map(|t| t.train_uid.as_str()).collect();
    let calling_points: Vec<CallingPointRow> = sqlx::query_as(
        "SELECT uid, tiploc, kind, booked_arrival, booked_departure, day_offset, platform \
         FROM schedule_calling_points_full \
         WHERE service_date = $1 AND uid = ANY($2) ORDER BY uid, seq",
    )
    .bind(date)
    .bind(&found)
    .fetch_all(pool)
    .await?;
    let trains_ids: Vec<i64> = trains.iter().map(|t| t.trains_id).collect();
    let reason_rows: Vec<ReasonDbRow> = sqlx::query_as(
        "SELECT r.trains_id, r.msg_type, r.reason_code, r.canx_type, s.tiploc \
         FROM train_reasons r \
         LEFT JOIN stanox_crs s ON s.stanox = r.loc_stanox \
         WHERE r.trains_id = ANY($1)",
    )
    .bind(&trains_ids)
    .fetch_all(pool)
    .await?;

    let mut points_by_uid: HashMap<&str, Vec<&CallingPointRow>> = HashMap::new();
    for row in &calling_points {
        points_by_uid.entry(row.uid.as_str()).or_default().push(row);
    }
    let json_by_uid: HashMap<&str, serde_json::Value> = points_by_uid
        .iter()
        .map(|(uid, rows)| (*uid, calling_points_json(rows)))
        .collect();
    let mut reasons: HashMap<i64, Vec<ReasonRow>> = HashMap::new();
    for row in reason_rows {
        reasons.entry(row.trains_id).or_default().push(row.reason);
    }

    let max_age = Duration::minutes(config.trust_max_age_minutes);
    let requests: Vec<JourneyStopsRequest<'_>> = trains
        .iter()
        .map(|train| JourneyStopsRequest {
            trains_id: train.trains_id,
            train_uid: &train.train_uid,
            service_date: date,
            calling_points_json: json_by_uid.get(train.train_uid.as_str()),
            // Stale TRUST state: no propagated estimate (actual times
            // still apply -- they are facts).
            current_delay_minutes: train
                .delay_minutes
                .filter(|_| train.updated_at.is_some_and(|at| now - at <= max_age)),
            skipped_stations: &train.skipped_stations,
            platform: train.platform.as_deref(),
            planned_platform: train.planned_platform.as_deref(),
        })
        .collect();
    let built = crate::data::journey::build_journey_stops_batch(pool, &requests).await?;

    let mut out = HashMap::new();
    for (train, stops) in trains.iter().zip(built) {
        let stops = match stops {
            Ok(Some(stops)) => stops,
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!(error = ?err, uid = %train.train_uid, "trip plan live: stops failed for one train");
                continue;
            }
        };
        let profile = train_live(TrainLiveInput {
            date,
            stops: &stops,
            status: train.status.as_deref(),
            state_updated_at: train.updated_at,
            reasons: reasons
                .get(&train.trains_id)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        });
        out.insert(train.train_uid.clone(), profile);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::stop_board::StopBoard;
    use schedule_query::CallingPointKind;

    #[test]
    fn options_gets_its_own_smaller_replan_budget() {
        let config = LiveConfig::default();
        assert_eq!(config.max_replans_for("fastest"), 3);
        assert_eq!(config.max_replans_for("options"), 1);
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 28).unwrap()
    }

    /// 2026-09-28 is BST: local = UTC + 1.
    fn utc(minute: u32) -> DateTime<Utc> {
        service_instant(date(), minute).unwrap()
    }

    fn stop(tiploc: &str, arr: Option<u32>, dep: Option<u32>) -> JourneyStop {
        let mut s = crate::data::journey::test_support::stop(
            "ZZZ",
            tiploc,
            CallingPointKind::Intermediate,
            None,
        );
        s.scheduled_arrival = arr.map(utc);
        s.scheduled_departure = dep.map(utc);
        s.stop_status = StopStatus::Scheduled;
        s
    }

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

    /// A -> B -> C -> D at 08:00/08:10-08:11/08:20-08:21/08:30.
    fn chain() -> Vec<Connection> {
        vec![
            conn("U1", "A", "B", 480, 490),
            conn("U1", "B", "C", 491, 500),
            conn("U1", "C", "D", 501, 510),
        ]
    }

    fn stops() -> Vec<JourneyStop> {
        vec![
            stop("A", None, Some(480)),
            stop("B", Some(490), Some(491)),
            stop("C", Some(500), Some(501)),
            stop("D", Some(510), None),
        ]
    }

    fn profile(stops: &[JourneyStop], status: Option<&str>, reasons: &[ReasonRow]) -> TrainLive {
        train_live(TrainLiveInput {
            date: date(),
            stops,
            status,
            state_updated_at: None,
            reasons,
        })
    }

    fn evaluate(live: &TrainLive) -> Vec<ChainPoint> {
        let chain = chain();
        let refs: Vec<&Connection> = chain.iter().collect();
        evaluate_chain(&refs, live)
    }

    fn board(etd: &str, delay: Option<i32>, cancelled: bool) -> StopBoard {
        StopBoard {
            delay_reason: Some("a signalling fault".to_string()),
            cancel_reason: cancelled.then(|| "a points failure".to_string()),
            is_cancelled: cancelled,
            delay_minutes: delay,
            estimated: etd.to_string(),
            observed_at: "2026-09-28T07:00:00Z".parse().unwrap(),
        }
    }

    #[test]
    fn nothing_known_changes_nothing() {
        let live = profile(&stops(), None, &[]);
        assert!(!live.has_data());
        assert_eq!(adjusted_connections("U1", &evaluate(&live)), None);
    }

    #[test]
    fn a_whole_cancellation_withdraws_every_connection() {
        let live = profile(&stops(), Some("cancelled"), &[]);
        assert!(evaluate(&live).iter().all(|p| !p.served));
        assert_eq!(adjusted_connections("U1", &evaluate(&live)), Some(vec![]));
    }

    #[test]
    fn a_cancellation_reason_without_a_state_row_cancels_but_a_running_state_does_not() {
        let reasons = [ReasonRow {
            msg_type: "0002".to_string(),
            reason_code: "TG".to_string(),
            canx_type: Some("AT ORIGIN".to_string()),
            tiploc: None,
        }];
        let live = profile(&stops(), None, &reasons);
        assert_eq!(adjusted_connections("U1", &evaluate(&live)), Some(vec![]));
        assert!(live.trust_cancel_reason.is_some());
        // Reinstated: the state row says it is running again.
        let live = profile(&stops(), Some("en_route"), &reasons);
        assert_eq!(adjusted_connections("U1", &evaluate(&live)), None);
    }

    #[test]
    fn an_en_route_cancellation_terminates_the_train_short() {
        let reasons = [ReasonRow {
            msg_type: "0002".to_string(),
            reason_code: "TG".to_string(),
            canx_type: Some("EN ROUTE".to_string()),
            tiploc: Some("C".to_string()),
        }];
        let live = profile(&stops(), Some("en_route"), &reasons);
        let adjusted = adjusted_connections("U1", &evaluate(&live)).unwrap();
        assert_eq!(
            adjusted,
            vec![
                conn("U1", "A", "B", 480, 490),
                conn("U1", "B", "C", 491, 500)
            ],
            "calls up to and including C remain; C -> D is withdrawn"
        );
    }

    #[test]
    fn a_change_of_origin_removes_the_calls_before_the_new_origin() {
        let reasons = [ReasonRow {
            msg_type: "0006".to_string(),
            reason_code: "TG".to_string(),
            canx_type: None,
            tiploc: Some("B".to_string()),
        }];
        let live = profile(&stops(), Some("en_route"), &reasons);
        let adjusted = adjusted_connections("U1", &evaluate(&live)).unwrap();
        assert_eq!(
            adjusted,
            vec![
                conn("U1", "B", "C", 491, 500),
                conn("U1", "C", "D", 501, 510)
            ]
        );
    }

    #[test]
    fn a_darwin_skipped_call_is_bridged_not_boardable() {
        let mut stops = stops();
        stops[2].stop_status = StopStatus::Skipped;
        let live = profile(&stops, None, &[]);
        let adjusted = adjusted_connections("U1", &evaluate(&live)).unwrap();
        assert_eq!(
            adjusted,
            vec![
                conn("U1", "A", "B", 480, 490),
                conn("U1", "B", "D", 491, 510)
            ]
        );
    }

    #[test]
    fn a_board_etd_delay_shifts_the_departure_and_propagates_forward_only() {
        let mut stops = stops();
        stops[1].board = Some(board("08:21", Some(10), false));
        stops[1].platform = Some("4".to_string());
        let live = profile(&stops, None, &[]);
        let points = evaluate(&live);
        // A: untouched (before the first known delay).
        assert_eq!(points[0].live_dep, Some(480));
        // B departs 10 late; C and D inherit it.
        assert_eq!(points[1].live_dep, Some(501));
        assert_eq!(points[2].live_arr, Some(510));
        assert_eq!(points[3].live_arr, Some(520));
        let adjusted = adjusted_connections("U1", &points).unwrap();
        // Live minutes to plan on; the working minutes stay the timetable's.
        assert_eq!(
            adjusted[1],
            Connection {
                working_departure_min: 491,
                working_arrival_min: 500,
                ..conn("U1", "B", "C", 501, 510)
            }
        );
    }

    #[test]
    fn a_trust_actual_departure_wins_and_early_running_clamps_to_zero() {
        let mut stops = stops();
        // As the journey overlay leaves them: each actual with the delay
        // measured from its own TRUST report.
        stops[0].actual_departure = Some(utc(478));
        stops[0].last_event_type = Some("DEPARTURE".to_string());
        stops[0].delay_minutes = Some(-2);
        stops[1].actual_arrival = Some(utc(497));
        stops[1].last_event_type = Some("ARRIVAL".to_string());
        stops[1].delay_minutes = Some(7);
        let live = profile(&stops, Some("en_route"), &[]);
        let points = evaluate(&live);
        assert_eq!(points[0].dep_delay, Some(0));
        assert_eq!(points[1].arr_delay, Some(7));
        assert_eq!(
            points[1].live_dep,
            Some(498),
            "departs no earlier than it arrived"
        );
    }

    /// A reported call's delay is the one measured from that TRUST report's
    /// own fields, never its actual time against the CIF (public or working)
    /// time: under TRUST's hour-skewed timestamps the latter would read an
    /// on-time train as an hour late.
    #[test]
    fn a_reported_delay_never_diffs_a_trust_actual_against_a_cif_time() {
        let mut stops = stops();
        stops[0].actual_departure = Some(utc(480 + 60));
        stops[0].last_event_type = Some("DEPARTURE".to_string());
        stops[0].delay_minutes = Some(0);
        stops[1].actual_arrival = Some(utc(490 + 60));
        stops[1].last_event_type = Some("ARRIVAL".to_string());
        stops[1].delay_minutes = None;
        let live = profile(&stops, Some("en_route"), &[]);
        let points = evaluate(&live);
        assert_eq!(points[0].dep_delay, Some(0));
        assert_eq!(
            points[1].arr_delay,
            Some(0),
            "no TRUST delay at B: the delay carried from A, not 60"
        );
    }

    /// Estimates are compared with the public time: running 3 late on the
    /// working timetable into a call whose public arrival is 2 minutes
    /// later is 1 late on the timetable passengers are sold.
    #[test]
    fn an_estimate_is_measured_against_the_public_time() {
        let mut stops = stops();
        stops[3].timetable.public_arrival = Some(utc(512));
        stops[3].estimated_arrival = Some(utc(513));
        let live = profile(&stops, Some("en_route"), &[]);
        let points = evaluate(&live);
        assert_eq!(points[3].arr_delay, Some(1));
    }

    #[test]
    fn a_board_cancelled_call_is_not_served() {
        let mut stops = stops();
        stops[1].board = Some(board("Cancelled", None, true));
        let live = profile(&stops, None, &[]);
        let points = evaluate(&live);
        assert!(!points[1].served);
        assert!(points[0].served && points[2].served);
    }

    /// A -> B on U1 (08:00-08:10), change at B (default 5 minutes), B -> C on
    /// U2 (08:20-08:40), planned from 07:50 with "now" 07:50.
    fn two_leg_plan_with_u1_late_by(delay: i32) -> (Vec<SegmentResult>, bool) {
        let connections = vec![
            conn("U1", "A", "B", 480, 490),
            conn("U2", "B", "C", 500, 520),
        ];
        let mut interchange = InterchangeData {
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        for (crs, tiploc) in [("AAA", "A"), ("BBB", "B"), ("CCC", "C")] {
            interchange
                .tiploc_to_crs
                .insert(tiploc.to_string(), crs.to_string());
            interchange
                .crs_to_tiplocs
                .insert(crs.to_string(), vec![tiploc.to_string()]);
        }
        let mut segments = crate::data::trip_planning_itinerary::plan_via_waypoints(
            &connections,
            &interchange,
            date(),
            "AAA",
            &[],
            "CCC",
            NaiveTime::from_hms_opt(7, 50, 0).unwrap(),
            "fastest",
            2,
        )
        .unwrap();
        let mut stops = vec![stop("A", None, Some(480)), stop("B", Some(490), None)];
        stops[0].board = Some(board("x", Some(delay), false));
        let lives = HashMap::from([("U1".to_string(), profile(&stops, None, &[]))]);
        let chains = chains_for(&connections, &HashSet::from(["U1".to_string()]));
        let config = LiveConfig::default();
        annotate(
            &mut segments,
            &LiveContext {
                date: date(),
                now: utc(470),
                config: &config,
                interchange: &interchange,
                chains: &chains,
                lives: &lives,
                overlaid: &HashSet::new(),
            },
        );
        let invalidated = plan_invalidated(&segments, &interchange);
        (segments, invalidated)
    }

    #[test]
    fn a_late_plan_that_still_connects_is_annotated_not_invalidated() {
        let (segments, invalidated) = two_leg_plan_with_u1_late_by(3);
        assert!(!invalidated);
        let itinerary = &segments[0].itineraries[0];
        assert_eq!(itinerary.live_feasible, Some(true));
        let PlannedLeg::Train {
            live: Some(Some(live)),
            scheduled_departure,
            ..
        } = &itinerary.legs[0]
        else {
            panic!("expected an annotated train leg: {:?}", itinerary.legs[0]);
        };
        assert_eq!(live.status, "Late");
        assert_eq!(live.delay_minutes, Some(3));
        assert_eq!(live.reason.as_deref(), Some("a signalling fault"));
        assert_eq!(live.reason_source, Some("darwin"));
        assert_eq!(
            *scheduled_departure,
            NaiveTime::from_hms_opt(8, 0, 0).unwrap()
        );
        let PlannedLeg::Train {
            live: Some(onward), ..
        } = &itinerary.legs[1]
        else {
            panic!("expected a train leg");
        };
        assert!(onward.is_none(), "no live record for U2");
    }

    #[test]
    fn a_delay_that_breaks_the_change_invalidates_the_plan() {
        // 8 late: arrives B 08:18, ready 08:23, after U2's 08:20.
        let (segments, invalidated) = two_leg_plan_with_u1_late_by(8);
        assert!(invalidated);
        assert_eq!(segments[0].itineraries[0].live_feasible, Some(false));
    }

    /// Arrive-by: U1 (A 08:00 -> B 08:10) is planned to arrive by 08:12. On
    /// time it makes it; 5 minutes late it does not, which marks the
    /// itinerary `liveFeasible: false` -- and that is what re-plans.
    #[test]
    fn a_delay_past_an_arrive_by_deadline_makes_the_plan_infeasible() {
        use crate::data::trip_planning_itinerary::{
            AvoidLists, SegmentSearch, TimeBound, TripPlanInput, plan_trip,
        };
        let connections = vec![conn("U1", "A", "B", 480, 490)];
        let mut interchange = InterchangeData {
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        for (crs, tiploc) in [("AAA", "A"), ("BBB", "B")] {
            interchange
                .tiploc_to_crs
                .insert(tiploc.to_string(), crs.to_string());
            interchange
                .crs_to_tiplocs
                .insert(crs.to_string(), vec![tiploc.to_string()]);
        }
        let annotated = |delay: i32| {
            let mut segments = plan_trip(&TripPlanInput {
                search: SegmentSearch {
                    connections: &connections,
                    interchange: &interchange,
                    date: date(),
                    results: "fastest",
                    max_changes: 2,
                    overlay: None,
                    restrictions: None,
                },
                passes: None,
                avoid: &AvoidLists::default(),
                origin_crs: "AAA",
                waypoints: &[],
                destination_crs: "BBB",
                vias: &[],
                via_search: None,
                time: TimeBound::ArriveBy(492),
            })
            .unwrap();
            let mut stops = vec![stop("A", None, Some(480)), stop("B", Some(490), None)];
            stops[0].board = Some(board("x", Some(delay), false));
            let lives = HashMap::from([("U1".to_string(), profile(&stops, None, &[]))]);
            let chains = chains_for(&connections, &HashSet::from(["U1".to_string()]));
            let config = LiveConfig::default();
            annotate(
                &mut segments,
                &LiveContext {
                    date: date(),
                    now: utc(470),
                    config: &config,
                    interchange: &interchange,
                    chains: &chains,
                    lives: &lives,
                    overlaid: &HashSet::new(),
                },
            );
            let invalidated = plan_invalidated(&segments, &interchange);
            (segments, invalidated)
        };
        let (segments, invalidated) = annotated(0);
        assert_eq!(segments[0].itineraries[0].live_feasible, Some(true));
        assert!(!invalidated);
        let (segments, invalidated) = annotated(5);
        assert_eq!(segments[0].itineraries[0].arrival_min, 495);
        assert_eq!(segments[0].itineraries[0].live_feasible, Some(false));
        assert!(invalidated);
    }

    #[test]
    fn a_late_origin_train_that_became_catchable_is_noticed() {
        let connections = vec![conn("EARLY", "A", "B", 470, 490)];
        let chains = chains_for(&connections, &HashSet::from(["EARLY".to_string()]));
        let mut stops = vec![stop("A", None, Some(470)), stop("B", Some(490), None)];
        stops[0].board = Some(board("08:05", Some(15), false));
        let lives = HashMap::from([("EARLY".to_string(), profile(&stops, None, &[]))]);
        let seeds = ["EARLY".to_string()];
        let origin = ["A".to_string()];
        assert!(seed_became_catchable(&seeds, &chains, &lives, &origin, 480));
        assert!(!seed_became_catchable(
            &seeds, &chains, &lives, &origin, 490
        ));
    }

    #[test]
    fn origin_lookback_finds_the_latest_trains_before_depart_after() {
        let connections = vec![
            conn("OLD", "A", "B", 400, 410),
            conn("R1", "A", "B", 440, 450),
            conn("X", "Q", "B", 445, 455),
            conn("R2", "A", "B", 470, 480),
            conn("LATER", "A", "B", 480, 490),
        ];
        assert_eq!(
            origin_lookback_uids(&connections, &["A".to_string()], 480),
            vec!["R2".to_string(), "R1".to_string()]
        );
    }

    #[test]
    fn the_horizon_bounds_which_legs_get_live_data() {
        let config = LiveConfig::default();
        let now = utc(600);
        assert!(within_horizon(date(), 600, now, &config));
        assert!(within_horizon(date(), 600 + 180, now, &config));
        assert!(!within_horizon(date(), 600 + 181, now, &config));
        assert!(!within_horizon(date(), 600 - 121, now, &config));
    }

    #[test]
    fn service_minutes_round_trip_across_midnight() {
        assert_eq!(service_minute(date(), utc(1455)), Some(1455));
        assert_eq!(service_minute(date(), utc(0)), Some(0));
    }
}
