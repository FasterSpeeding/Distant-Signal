//! Arrive-by: the LATEST departure from any of a set of origin TIPLOCs that
//! still reaches any of a set of destination TIPLOCs by a deadline, and the
//! journeys that achieve it.
//!
//! A backward Connection Scan: the day's connections (departing no later
//! than the deadline) are walked in reverse order, keeping for every stop
//! the latest time a traveller can be standing there, just arrived, and
//! still make the deadline. Every interchange rule is the exact mirror of
//! `csa.rs`'s forward one, so the two agree:
//!
//! - a fresh boarding at a non-origin stop costs that stop's
//!   `minimum_change_time` (a `NoInterchange` sentinel forbids it), and an
//!   arrival at any same-CRS sibling can use it, charged at the boarding
//!   stop's own figure;
//! - the origin needs no change time;
//! - staying aboard the same UID is free;
//! - fixed links (walk, tube, ...) are relaxed backwards, using the same
//!   `fixed_links_from` choice (shortest valid link per destination at the
//!   walk's own start time) the forward search makes;
//! - [`crate::restrictions`] apply exactly as they do going forward.
//!
//! The backward scan yields only the departure time. The journey itself is
//! then produced by the ordinary FORWARD search from that time (over the
//! connections departing by the deadline), so an arrive-by itinerary is
//! built by the same code, with the same legs, as a depart-after one. If
//! the forward search ever disagreed (it should not; the unit tests below
//! check it against a brute-force search), [`scan_connections_arrive_by`]
//! falls back to Skye's `train-mcp` approach (`src/tools/plan-journey.ts`,
//! `latestDepartureFor`): a binary search over departure times using the
//! forward search as the probe, which is correct because earliest arrival
//! never decreases as the departure time increases.
//!
//! [`latest_departures_by_trips`] is the round-based (RAPTOR-shaped)
//! version: round *k* answers "latest departure using at most *k* trains",
//! which is what `results=options` needs to offer one itinerary per change
//! count.
//!
//! See docs/superpowers/specs/2026-09-29-trips-plan-arrive-by-avoid-design.md.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use schedule_query::{
    Connection, InterchangeData, fixed_links_from, normalize_tiploc, sibling_tiplocs,
};

use crate::csa::{Journey, ScanOptions, scan_connections_restricted};
use crate::overlay::ConnectionOverlay;
use crate::raptor::{RaptorJourney, RaptorOptions, raptor_search_restricted};
use crate::restrictions::{self, Restrictions};
use crate::staged::{StagedJourney, StagedOptions, raptor_staged, scan_staged};

pub struct ArriveByOptions<'a> {
    /// Sorted as `build_connections` returns it (same contract as
    /// `ScanOptions::connections`).
    pub connections: &'a [Connection],
    pub interchange: &'a InterchangeData,
    pub from_tiplocs: &'a [String],
    /// Waypoints to call at in order, each as every TIPLOC it covers (see
    /// [`crate::staged`]); empty for a direct search. The Journey-returning
    /// functions ([`scan_connections_arrive_by`], [`raptor_arrive_by`])
    /// need it empty; the `staged_*` ones take any.
    pub waypoints: &'a [Vec<String>],
    pub to_tiplocs: &'a [String],
    /// Latest acceptable arrival, minutes from service-day midnight.
    pub arrive_by_min: u32,
    pub date: NaiveDate,
}

/// The per-stop labels of one backward pass (or one round), per stage
/// (see [`crate::staged`]: stage `s` = the first `s` waypoints called at).
#[derive(Clone)]
struct Labels {
    /// Latest time a traveller may have ARRIVED at a stop (by train or
    /// walk), at that stage, and still reach the destination by the
    /// deadline.
    latest_arrival: Vec<HashMap<String, u32>>,
    /// Latest departure from the origin found so far.
    origin_departure: Option<u32>,
    touched: bool,
}

impl Labels {
    fn new(stages: usize) -> Self {
        Self {
            latest_arrival: vec![HashMap::new(); stages],
            origin_departure: None,
            touched: false,
        }
    }

    fn offer_origin(&mut self, departure: u32) {
        if self.origin_departure.is_none_or(|best| departure > best) {
            self.origin_departure = Some(departure);
            self.touched = true;
        }
    }
}

struct Reverse<'a> {
    interchange: &'a InterchangeData,
    restrictions: Option<&'a Restrictions>,
    date: NaiveDate,
    origin: HashSet<String>,
    /// `targets[s]`: waypoint `s`'s TIPLOCs.
    targets: Vec<HashSet<String>>,
    /// to-CRS -> every CRS with at least one fixed link into it.
    links_into: HashMap<&'a str, Vec<&'a str>>,
}

impl<'a> Reverse<'a> {
    fn new(options: &ArriveByOptions<'a>, restrictions: Option<&'a Restrictions>) -> Self {
        let mut links_into: HashMap<&'a str, Vec<&'a str>> = HashMap::new();
        for (from_crs, links) in &options.interchange.fixed_links_from_crs {
            for link in links {
                let sources = links_into.entry(link.to_crs.as_str()).or_default();
                if !sources.contains(&from_crs.as_str()) {
                    sources.push(from_crs.as_str());
                }
            }
        }
        let set = |tiplocs: &[String]| -> HashSet<String> {
            tiplocs
                .iter()
                .map(|t| normalize_tiploc(t).to_string())
                .collect()
        };
        Self {
            interchange: options.interchange,
            restrictions,
            date: options.date,
            origin: set(options.from_tiplocs),
            targets: options.waypoints.iter().map(|w| set(w)).collect(),
            links_into,
        }
    }

    fn stages(&self) -> usize {
        self.targets.len() + 1
    }

    /// Round 0: standing at the destination (last stage) by the deadline,
    /// and every stop a walk from which reaches it in time.
    fn initial_labels(&self, options: &ArriveByOptions<'_>) -> Labels {
        let mut labels = Labels::new(self.stages());
        for tiploc in options.to_tiplocs {
            self.set_arrival(
                &mut labels,
                self.stages() - 1,
                tiploc,
                options.arrive_by_min,
            );
        }
        labels
    }

    /// Mirror of `csa::Scan::relax`: `tiploc` may be arrived at, at
    /// `stage`, as late as `time`. Propagates backwards over every fixed
    /// link into it, and -- at a waypoint -- to the stage before it (being
    /// there at stage `s + 1` is being there at stage `s`, then calling).
    #[expect(
        clippy::cast_sign_loss,
        reason = "fixed-link minutes are small and strictly positive (see relax_in_round)"
    )]
    fn set_arrival(&self, labels: &mut Labels, stage: usize, tiploc: &str, time: u32) {
        let tiploc = normalize_tiploc(tiploc);
        if !restrictions::allows_interchange(self.restrictions, tiploc) {
            return;
        }
        if labels.latest_arrival[stage]
            .get(tiploc)
            .is_some_and(|&known| known >= time)
        {
            return;
        }
        // Dominated: a plan from here that still has to call at MORE of the
        // waypoints works at least this late, and serves this stage too.
        if labels.latest_arrival[..stage]
            .iter()
            .any(|earlier| earlier.get(tiploc).is_some_and(|&known| known >= time))
        {
            return;
        }
        labels.latest_arrival[stage].insert(tiploc.to_string(), time);
        labels.touched = true;
        if stage > 0 && self.targets[stage - 1].contains(tiploc) {
            self.set_arrival(labels, stage - 1, tiploc, time);
        }

        let Some(crs) = self.interchange.tiploc_to_crs.get(tiploc) else {
            return;
        };
        let Some(sources) = self.links_into.get(crs.as_str()) else {
            return;
        };
        for &from_crs in sources {
            let mut minutes: Vec<u32> = self.interchange.fixed_links_from_crs[from_crs]
                .iter()
                .filter(|link| link.to_crs == *crs && link.minutes > 0)
                .map(|link| link.minutes as u32)
                .collect();
            minutes.sort_unstable();
            minutes.dedup();
            for walk in minutes {
                let Some(start) = time.checked_sub(walk) else {
                    continue;
                };
                // The forward search takes the shortest link valid at the
                // walk's start time; accept `start` only if that link
                // arrives in time.
                let fits = fixed_links_from(self.interchange, from_crs, self.date, start)
                    .into_iter()
                    .any(|link| link.to_crs == *crs && start + link.minutes.max(0) as u32 <= time);
                if !fits {
                    continue;
                }
                let Some(from_tiplocs) = self.interchange.crs_to_tiplocs.get(from_crs) else {
                    continue;
                };
                for from_tiploc in from_tiplocs {
                    if stage == 0 && self.origin.contains(normalize_tiploc(from_tiploc)) {
                        labels.offer_origin(start);
                    } else {
                        self.set_arrival(labels, stage, from_tiploc, start);
                    }
                }
            }
        }
    }

    /// One backward sweep over `connections` (descending order). `previous`
    /// is the round whose labels decide where alighting works (`None` for
    /// the single-pass CSA mirror, which reads and writes `current`).
    fn sweep(&self, connections: &[&Connection], previous: Option<&Labels>, current: &mut Labels) {
        let stages = self.stages();
        // Per stage, the UIDs a traveller aboard at that stage can stay on
        // and still make it (from their later connections).
        let mut aboard_ok: Vec<HashSet<&str>> = vec![HashSet::new(); stages];
        let mut ok_stages: Vec<usize> = Vec::with_capacity(stages);
        for connection in connections.iter().rev() {
            // Nothing departing at or before the best origin departure found
            // can improve on it (every departure a path yields is no later
            // than its own first connection's).
            if current
                .origin_departure
                .is_some_and(|best| connection.departure_min <= best)
            {
                break;
            }
            let uid = connection.uid.as_str();
            if restrictions::blocks(self.restrictions, connection) {
                for ok in &mut aboard_ok {
                    ok.remove(uid);
                }
                continue;
            }
            let to = normalize_tiploc(&connection.to_tiploc);
            // Every stage at which riding this connection works, decided
            // before any of them is recorded: alighting at `to`, staying
            // aboard, or -- `to` being this stage's waypoint -- staying
            // aboard through it into the next stage.
            ok_stages.clear();
            {
                let labels = previous.unwrap_or(&*current);
                for stage in 0..stages {
                    let alight = connection.can_alight
                        && labels.latest_arrival[stage]
                            .get(to)
                            .is_some_and(|&latest| connection.arrival_min <= latest);
                    let stay = aboard_ok[stage].contains(uid);
                    let through = stage + 1 < stages
                        && self.targets[stage].contains(to)
                        && aboard_ok[stage + 1].contains(uid);
                    if alight || stay || through {
                        // The lowest such stage dominates the higher ones
                        // (see `set_arrival`).
                        ok_stages.push(stage);
                        break;
                    }
                }
            }

            let from = normalize_tiploc(&connection.from_tiploc);
            for &stage in &ok_stages {
                aboard_ok[stage].insert(uid);
                // Someone already aboard may ride on, but nobody boards at a
                // set-down-only stop.
                if !connection.can_board {
                    continue;
                }
                if stage == 0 && self.origin.contains(from) {
                    current.offer_origin(connection.departure_min);
                    continue;
                }
                let Some(change) = crate::staged::change_minutes(
                    self.interchange,
                    self.restrictions,
                    &self.targets,
                    stage,
                    from,
                ) else {
                    continue;
                };
                let Some(ready_by) = connection.departure_min.checked_sub(change) else {
                    continue;
                };
                self.set_arrival(current, stage, from, ready_by);
                for sibling in sibling_tiplocs(self.interchange, from) {
                    self.set_arrival(current, stage, sibling, ready_by);
                }
            }
        }
    }
}

/// Every connection (base plus overlay) departing no later than `deadline`,
/// in the forward search's order.
fn connections_by<'a>(
    base: &'a [Connection],
    overlay: Option<&'a ConnectionOverlay>,
    deadline: u32,
) -> Vec<&'a Connection> {
    let end = base.partition_point(|c| c.departure_min <= deadline);
    crate::overlay::connections_from(&base[..end], overlay, 0)
        .take_while(|c| c.departure_min <= deadline)
        .collect()
}

/// The base connections departing no later than the deadline -- what the
/// forward searches building an arrive-by journey walk.
fn base_by(base: &[Connection], deadline: u32) -> &[Connection] {
    &base[..base.partition_point(|c| c.departure_min <= deadline)]
}

/// The latest departure from the origin that reaches the destination by
/// `options.arrive_by_min`, any number of changes. `None` when none does.
pub fn latest_departure(
    options: &ArriveByOptions<'_>,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
) -> Option<u32> {
    let reverse = Reverse::new(options, restrictions);
    let connections = connections_by(options.connections, overlay, options.arrive_by_min);
    let mut labels = reverse.initial_labels(options);
    reverse.sweep(&connections, None, &mut labels);
    labels.origin_departure
}

/// Element `k - 1`: the latest departure reaching the destination by the
/// deadline using at most `k` trains (`k - 1` changes), for `k` in
/// `1..=max_rounds`. Non-decreasing; `None` until some round reaches it.
pub fn latest_departures_by_trips(
    options: &ArriveByOptions<'_>,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
    max_rounds: u32,
) -> Vec<Option<u32>> {
    let reverse = Reverse::new(options, restrictions);
    let connections = connections_by(options.connections, overlay, options.arrive_by_min);
    let mut previous = reverse.initial_labels(options);
    let mut out = Vec::with_capacity(max_rounds as usize);
    for _ in 0..max_rounds {
        let mut current = previous.clone();
        current.touched = false;
        reverse.sweep(&connections, Some(&previous), &mut current);
        out.push(current.origin_departure);
        if !current.touched {
            // Nothing changed: every later round would be identical.
            while out.len() < max_rounds as usize {
                out.push(current.origin_departure);
            }
            break;
        }
        previous = current;
    }
    out
}

/// `train-mcp`'s `latestDepartureFor` (see the module doc): the latest
/// departure in `[0, deadline]` whose `probe` journey arrives by `deadline`.
/// Only the fallback when the forward search disagrees with the backward one.
fn latest_by_bisection<T>(deadline: u32, probe: impl Fn(u32) -> Option<T>) -> Option<T> {
    let mut best = probe(0)?;
    let (mut lo, mut hi) = (0u32, deadline);
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        match probe(mid) {
            Some(journey) => {
                lo = mid;
                best = journey;
            }
            None => hi = mid - 1,
        }
    }
    Some(best)
}

/// The arrive-by counterpart of [`scan_connections_restricted`]: the
/// latest-departing journey arriving by `options.arrive_by_min` (and, among
/// those departing then, the earliest-arriving one). `None` when no journey
/// arrives in time.
#[expect(
    clippy::needless_pass_by_value,
    reason = "public planner API takes its options and overlay by value"
)]
pub fn scan_connections_arrive_by(
    options: ArriveByOptions<'_>,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
) -> Option<Journey> {
    let deadline = options.arrive_by_min;
    let departure = latest_departure(&options, overlay, restrictions)?;
    let connections = base_by(options.connections, deadline);
    let forward = |departure_min: u32| {
        scan_connections_restricted(
            ScanOptions {
                connections,
                interchange: options.interchange,
                from_tiplocs: options.from_tiplocs,
                to_tiplocs: options.to_tiplocs,
                departure_min,
                date: options.date,
            },
            overlay,
            restrictions,
        )
        .filter(|journey| journey.arrival_min <= deadline)
    };
    forward(departure).or_else(|| latest_by_bisection(departure, forward))
}

/// The arrive-by counterpart of [`raptor_search_restricted`]: for each
/// number of trains `k` in `1..=latest.len()` where the latest departure
/// strictly improves on fewer trains', the journey departing then (built by
/// a forward RAPTOR search capped at `k` trains). Fewest changes first.
/// `latest` is [`latest_departures_by_trips`]'s output, or a prefix of it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a journey has a handful of legs and rounds"
)]
pub fn raptor_arrive_by_from_latest(
    options: &ArriveByOptions<'_>,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
    latest: &[Option<u32>],
) -> Vec<RaptorJourney> {
    let deadline = options.arrive_by_min;
    let connections = base_by(options.connections, deadline);
    let mut out: Vec<RaptorJourney> = Vec::new();
    for (index, departure) in latest.iter().enumerate() {
        let Some(departure) = *departure else {
            continue;
        };
        if out.last().is_some_and(|j| departure <= j.departure_min) {
            continue;
        }
        let trains = index as u32 + 1;
        let best = raptor_search_restricted(
            RaptorOptions {
                connections,
                interchange: options.interchange,
                from_tiplocs: options.from_tiplocs,
                to_tiplocs: options.to_tiplocs,
                departure_min: departure,
                date: options.date,
                max_rounds: trains,
            },
            overlay,
            restrictions,
        )
        .into_iter()
        .filter(|j| j.arrival_min <= deadline)
        .min_by_key(|j| (j.arrival_min, j.changes));
        if let Some(journey) = best
            && out
                .last()
                .is_none_or(|j| journey.departure_min > j.departure_min)
        {
            out.push(journey);
        }
    }
    out
}

/// [`scan_connections_arrive_by`] through `options.waypoints`: the
/// latest-departing journey calling at every waypoint in order and arriving
/// by the deadline (see [`crate::staged`]).
pub fn staged_arrive_by(
    options: &ArriveByOptions<'_>,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
) -> Option<StagedJourney> {
    let deadline = options.arrive_by_min;
    let departure = latest_departure(options, overlay, restrictions)?;
    let staged = StagedOptions {
        connections: base_by(options.connections, deadline),
        interchange: options.interchange,
        from_tiplocs: options.from_tiplocs,
        waypoints: options.waypoints,
        to_tiplocs: options.to_tiplocs,
        date: options.date,
    };
    let forward = |departure_min: u32| {
        scan_staged(&staged, departure_min, overlay, restrictions)
            .filter(|journey| journey.arrival_min <= deadline)
    };
    forward(departure).or_else(|| latest_by_bisection(departure, forward))
}

/// [`raptor_arrive_by_from_latest`] through `options.waypoints`, trains
/// (and so changes) counted over the whole journey.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a journey has a handful of legs and rounds"
)]
pub fn staged_raptor_arrive_by_from_latest(
    options: &ArriveByOptions<'_>,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
    latest: &[Option<u32>],
) -> Vec<StagedJourney> {
    let deadline = options.arrive_by_min;
    let staged = StagedOptions {
        connections: base_by(options.connections, deadline),
        interchange: options.interchange,
        from_tiplocs: options.from_tiplocs,
        waypoints: options.waypoints,
        to_tiplocs: options.to_tiplocs,
        date: options.date,
    };
    let mut out: Vec<StagedJourney> = Vec::new();
    for (index, departure) in latest.iter().enumerate() {
        let Some(departure) = *departure else {
            continue;
        };
        if out.last().is_some_and(|j| departure <= j.departure_min) {
            continue;
        }
        let best = raptor_staged(&staged, departure, index as u32 + 1, overlay, restrictions)
            .into_iter()
            .filter(|j| j.arrival_min <= deadline)
            .min_by_key(|j| (j.arrival_min, j.changes));
        if let Some(journey) = best
            && out
                .last()
                .is_none_or(|j| journey.departure_min > j.departure_min)
        {
            out.push(journey);
        }
    }
    out
}

/// [`latest_departures_by_trips`] then [`raptor_arrive_by_from_latest`].
#[expect(
    clippy::needless_pass_by_value,
    reason = "public planner API takes its options and overlay by value"
)]
pub fn raptor_arrive_by(
    options: ArriveByOptions<'_>,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
    max_rounds: u32,
) -> Vec<RaptorJourney> {
    let latest = latest_departures_by_trips(&options, overlay, restrictions, max_rounds);
    raptor_arrive_by_from_latest(&options, overlay, restrictions, &latest)
}

#[cfg(test)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    reason = "test code: casts of small known test values; scenario tests read top to bottom"
)]
mod tests {
    use super::*;
    use crate::JourneyLeg;
    use schedule_query::FixedLink;

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

    fn sorted(mut connections: Vec<Connection>) -> Vec<Connection> {
        connections.sort_by(|a, b| {
            (a.departure_min, &a.uid, &a.from_tiploc).cmp(&(
                b.departure_min,
                &b.uid,
                &b.from_tiploc,
            ))
        });
        connections
    }

    fn interchange(change_times: &[(&str, i32)]) -> InterchangeData {
        InterchangeData {
            change_time_by_tiploc: change_times
                .iter()
                .map(|(t, m)| ((*t).to_string(), *m))
                .collect(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        }
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 29).unwrap()
    }

    fn s(v: &str) -> Vec<String> {
        vec![v.to_string()]
    }

    fn uids(journey: &Journey) -> Vec<String> {
        journey
            .legs
            .iter()
            .filter_map(|leg| match leg {
                JourneyLeg::Train(t) => Some(t.uid.clone()),
                JourneyLeg::Transfer(_) => None,
            })
            .collect()
    }

    fn arrive_by<'a>(
        connections: &'a [Connection],
        interchange: &'a InterchangeData,
        from: &'a [String],
        to: &'a [String],
        deadline: u32,
    ) -> ArriveByOptions<'a> {
        ArriveByOptions {
            connections,
            interchange,
            from_tiplocs: from,
            waypoints: &[],
            to_tiplocs: to,
            arrive_by_min: deadline,
            date: date(),
        }
    }

    #[test]
    fn the_latest_direct_train_arriving_in_time_is_chosen() {
        let connections = sorted(vec![
            conn("U1", "A", "B", 480, 540),
            conn("U2", "A", "B", 510, 570),
            conn("U3", "A", "B", 540, 601),
        ]);
        let ic = interchange(&[]);
        let (from, to) = (s("A"), s("B"));
        let journey =
            scan_connections_arrive_by(arrive_by(&connections, &ic, &from, &to, 600), None, None)
                .expect("U2 arrives by 10:00");
        assert_eq!(uids(&journey), vec!["U2"]);
        assert_eq!(journey.departure_min, 510);
        assert!(
            scan_connections_arrive_by(arrive_by(&connections, &ic, &from, &to, 539), None, None)
                .is_none()
        );
    }

    #[test]
    fn a_change_must_respect_the_minimum_change_time() {
        // Via B (change time 5): U1 then U3 works (arrive 530, depart 535);
        // U2 then U3 does not (arrive 532). The latest workable departure is
        // U1's 480, not U2's 490.
        let connections = sorted(vec![
            conn("U1", "A", "B", 480, 530),
            conn("U2", "A", "B", 490, 532),
            conn("U3", "B", "C", 535, 600),
        ]);
        let ic = interchange(&[("B", 5)]);
        let (from, to) = (s("A"), s("C"));
        let journey =
            scan_connections_arrive_by(arrive_by(&connections, &ic, &from, &to, 600), None, None)
                .expect("a journey exists");
        assert_eq!(uids(&journey), vec!["U1", "U3"]);
        assert_eq!(journey.departure_min, 480);
    }

    #[test]
    fn staying_aboard_is_free_and_a_sentinel_blocks_only_fresh_boardings() {
        // B is a NoInterchange sentinel: U1 through B is fine, a change there
        // (U2 -> U3) is not.
        let connections = sorted(vec![
            conn("U1", "A", "B", 480, 500),
            conn("U1", "B", "C", 501, 560),
            conn("U2", "A", "B", 520, 530),
            conn("U3", "B", "C", 540, 580),
        ]);
        let ic = interchange(&[("B", 99)]);
        let (from, to) = (s("A"), s("C"));
        let journey =
            scan_connections_arrive_by(arrive_by(&connections, &ic, &from, &to, 600), None, None)
                .expect("U1 works");
        assert_eq!(uids(&journey), vec!["U1"]);
    }

    #[test]
    fn a_walk_into_the_destination_counts_against_the_deadline() {
        // Train to EUSTON, then a 10-minute tube to KINGX.
        let connections = sorted(vec![
            conn("U1", "MKC", "EUSTON", 480, 530),
            conn("U2", "MKC", "EUSTON", 500, 552),
        ]);
        let mut ic = interchange(&[]);
        for (crs, tiploc) in [("EUS", "EUSTON"), ("KGX", "KINGX"), ("MKC", "MKC")] {
            ic.tiploc_to_crs.insert(tiploc.to_string(), crs.to_string());
            ic.crs_to_tiplocs
                .insert(crs.to_string(), vec![tiploc.to_string()]);
        }
        ic.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![FixedLink {
                mode: "TUBE".to_string(),
                to_crs: "KGX".to_string(),
                minutes: 10,
                valid_from: "0000".to_string(),
                valid_to: "2359".to_string(),
                days_mask: "1111111".to_string(),
            }],
        );
        let (from, to) = (s("MKC"), s("KINGX"));
        // U2 arrives 552 + 10 = 562 > 560; U1 (540) is the answer.
        let journey =
            scan_connections_arrive_by(arrive_by(&connections, &ic, &from, &to, 560), None, None)
                .expect("U1 plus the tube works");
        assert_eq!(uids(&journey), vec!["U1"]);
        assert_eq!(journey.arrival_min, 540);
        let journey =
            scan_connections_arrive_by(arrive_by(&connections, &ic, &from, &to, 562), None, None)
                .expect("U2 plus the tube works");
        assert_eq!(uids(&journey), vec!["U2"]);
    }

    #[test]
    fn the_rounds_offer_a_later_departure_for_one_more_change() {
        // Direct D1 leaves 420; with one change at B, U1 + U2 leaves 450.
        let connections = sorted(vec![
            conn("D1", "A", "C", 420, 590),
            conn("U1", "A", "B", 450, 500),
            conn("U2", "B", "C", 510, 580),
        ]);
        let ic = interchange(&[("B", 5)]);
        let (from, to) = (s("A"), s("C"));
        let options = arrive_by(&connections, &ic, &from, &to, 600);
        assert_eq!(
            latest_departures_by_trips(&options, None, None, 4),
            vec![Some(420), Some(450), Some(450), Some(450)]
        );
        let journeys = raptor_arrive_by(options, None, None, 4);
        let shape: Vec<(u32, u32)> = journeys
            .iter()
            .map(|j| (j.changes, j.departure_min))
            .collect();
        assert_eq!(shape, vec![(0, 420), (1, 450)]);
    }

    #[test]
    fn restrictions_apply_backwards_too() {
        // The only change point is B; avoiding changes there leaves the
        // slower direct train.
        let connections = sorted(vec![
            conn("D1", "A", "C", 420, 590),
            conn("U1", "A", "B", 450, 500),
            conn("U2", "B", "C", 510, 580),
        ]);
        let ic = interchange(&[("B", 5)]);
        let (from, to) = (s("A"), s("C"));
        let avoid = Restrictions::new(["B".to_string()], [], HashMap::new());
        let journey = scan_connections_arrive_by(
            arrive_by(&connections, &ic, &from, &to, 600),
            None,
            Some(&avoid),
        )
        .expect("D1 still works");
        assert_eq!(uids(&journey), vec!["D1"]);

        // A train calling at B cannot be ridden at all under no_call, even
        // straight through.
        let through = sorted(vec![
            conn("T1", "A", "B", 450, 500),
            conn("T1", "B", "C", 502, 560),
            conn("D1", "A", "C", 400, 590),
        ]);
        let no_call = Restrictions::new([], ["B".to_string()], HashMap::new());
        let journey = scan_connections_arrive_by(
            arrive_by(&through, &ic, &from, &to, 600),
            None,
            Some(&no_call),
        )
        .expect("D1 still works");
        assert_eq!(uids(&journey), vec!["D1"]);
        let journey = scan_connections_arrive_by(
            arrive_by(&through, &ic, &from, &to, 600),
            None,
            Some(&avoid),
        )
        .expect("T1 straight through B is fine when only changing there is avoided");
        assert_eq!(uids(&journey), vec!["T1"]);
    }

    #[test]
    fn a_live_overlay_delay_moves_the_answer_earlier() {
        let connections = sorted(vec![
            conn("U1", "A", "B", 480, 540),
            conn("U2", "A", "B", 510, 570),
        ]);
        let ic = interchange(&[]);
        let (from, to) = (s("A"), s("B"));
        let overlay =
            ConnectionOverlay::new(["U2".to_string()], vec![conn("U2", "A", "B", 525, 585)]);
        let journey = scan_connections_arrive_by(
            arrive_by(&connections, &ic, &from, &to, 580),
            Some(&overlay),
            None,
        )
        .expect("U1 works");
        assert_eq!(uids(&journey), vec!["U1"]);
    }

    /// Brute force: the latest departure minute whose forward earliest
    /// arrival is in time, over a small pseudo-random network, must equal
    /// the backward scan's answer -- with and without restrictions. Some
    /// intermediate calls are set-down-only or pick-up-only, so both
    /// directions agree on where boarding and alighting are allowed.
    #[test]
    fn the_backward_scan_agrees_with_a_brute_force_forward_search() {
        let stops = ["A", "B", "C", "D", "E", "F"];
        let mut seed: u64 = 0x5eed;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let (mut found, mut total) = (0, 0);
        for case in 0..60 {
            let mut connections = Vec::new();
            for train in 0..25 {
                let mut at = stops[next(stops.len() as u64) as usize];
                let mut time = 300 + next(600) as u32;
                let mut can_board_at = true;
                let hops = 1 + next(4);
                for hop in 0..hops {
                    let mut to = stops[next(stops.len() as u64) as usize];
                    if to == at {
                        to =
                            stops[(stops.iter().position(|s| *s == at).unwrap() + 1) % stops.len()];
                    }
                    let run = 5 + next(60) as u32;
                    // An intermediate call is set-down-only (D) or
                    // pick-up-only (U) one time in seven each.
                    let (board_next, alight) = match (hop + 1 < hops, next(7)) {
                        (true, 0) => (false, true),
                        (true, 1) => (true, false),
                        _ => (true, true),
                    };
                    let mut connection = conn(&format!("T{train}"), at, to, time, time + run);
                    connection.can_board = can_board_at;
                    connection.can_alight = alight;
                    connections.push(connection);
                    time += run + next(3) as u32;
                    at = to;
                    can_board_at = board_next;
                }
            }
            let connections = sorted(connections);
            // With no fixed links, a journey's departure is always some
            // train's departure from A: only those minutes need trying.
            let mut candidates: Vec<u32> = connections
                .iter()
                .filter(|c| c.from_tiploc == "A")
                .map(|c| c.departure_min)
                .collect();
            candidates.sort_unstable();
            candidates.dedup();
            candidates.reverse();
            let ic = interchange(&[("C", 3), ("D", 99), ("E", 0)]);
            let (from, to) = (s("A"), s("F"));
            let restrictions = Restrictions::new(["B".to_string()], [], HashMap::new());
            for restricted in [None, Some(&restrictions)] {
                for deadline in [600u32, 800, 1000] {
                    let brute = candidates
                        .iter()
                        .copied()
                        .filter(|&dep| dep <= deadline)
                        .find(|&dep| {
                            scan_connections_restricted(
                                ScanOptions {
                                    connections: &connections,
                                    interchange: &ic,
                                    from_tiplocs: &from,
                                    to_tiplocs: &to,
                                    departure_min: dep,
                                    date: date(),
                                },
                                None,
                                restricted,
                            )
                            .is_some_and(|j| j.arrival_min <= deadline)
                        });
                    let options = arrive_by(&connections, &ic, &from, &to, deadline);
                    let backward = latest_departure(&options, None, restricted);
                    assert_eq!(backward, brute, "case {case}, deadline {deadline}");
                    let journey = scan_connections_arrive_by(options, None, restricted);
                    assert_eq!(journey.map(|j| j.departure_min), brute);
                    total += 1;
                    found += usize::from(brute.is_some());

                    // Per number of trains, against forward RAPTOR.
                    let options = arrive_by(&connections, &ic, &from, &to, deadline);
                    let by_trips = latest_departures_by_trips(&options, None, restricted, 4);
                    for (index, backward) in by_trips.iter().enumerate() {
                        let trains = index as u32 + 1;
                        let brute = candidates
                            .iter()
                            .copied()
                            .filter(|&dep| dep <= deadline)
                            .find(|&dep| {
                                raptor_search_restricted(
                                    RaptorOptions {
                                        connections: &connections,
                                        interchange: &ic,
                                        from_tiplocs: &from,
                                        to_tiplocs: &to,
                                        departure_min: dep,
                                        date: date(),
                                        max_rounds: trains,
                                    },
                                    None,
                                    restricted,
                                )
                                .iter()
                                .any(|j| j.arrival_min <= deadline)
                            });
                        assert_eq!(
                            *backward, brute,
                            "case {case}, deadline {deadline}, {trains} trains"
                        );
                    }
                }
            }
        }
        // The generator must actually produce reachable cases.
        assert!(
            found * 3 > total,
            "only {found} of {total} cases were reachable"
        );
    }
}
