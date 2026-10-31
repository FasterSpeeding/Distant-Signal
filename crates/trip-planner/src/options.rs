//! Diverse journey options for `/Trips/plan?results=options` (DS-196).
//!
//! RAPTOR's Pareto set over (arrival, changes) from one departure time is
//! usually a single journey: when the fastest train is also direct, nothing
//! with more changes can beat it, so "Compare options" had nothing to
//! compare. This module adds up to [`MAX_ALTERNATIVES`] genuinely different
//! routes to that set by iterative exclusion:
//!
//! 1. Every journey already kept contributes its corridor: the stations its
//!    trains call at between boarding and alighting (banned as calls) and
//!    the stations it changes or walks between legs at (banned as changes).
//!    The ends, waypoints and vias are never banned.
//! 2. The caller's search runs again under those bans on top of the
//!    request's own restrictions (a single Connection Scan per attempt, so
//!    an attempt costs about what `results=fastest` does).
//! 3. A result is kept only when it is sane: within the change cap and at
//!    most two changes more than the fewest-change option; arriving (or,
//!    arrive-by, leaving) at most [`allowance_minutes`] later (earlier) than
//!    the best option and taking at most that much longer; never calling at
//!    the same station twice (no back-tracking); and not a near copy of a
//!    kept journey (same route signature, or mostly the same stations).
//!    Its corridor is then banned too, and the next attempt looks for a
//!    third route.
//! 4. When an attempt finds nothing usable, the bans are relaxed once to
//!    the best journey's trunk alone (the longest train ride's calls), since
//!    banning every change station as well can leave nothing within reach.
//!
//! At most [`MAX_SEARCHES`] extra searches run, and nothing runs when the
//! best journey has no corridor to ban (a non-stop train).
//!
//! Cost (`tests/bench_arrive_by.rs`'s `bench_options_cpu`: 20 random pairs
//! on a synthetic ~400k-connection day, release, CPU time, 2026-10-30):
//! RAPTOR's Pareto set alone took 16.3 s before DS-196 and 6.0 s after it
//! (each round's sweep now stops at the best arrival so far, see
//! `raptor::raptor_search_restricted`); the alternatives add 0.4 s, and
//! turn 39 options into 61.
//!
//! The same train or route at another time is deliberately not an option:
//! the planner's "Earlier routes"/"Later routes" paging already covers
//! departure times, and a list of the same train every half hour is what
//! "Compare options" should not be.

use std::cmp::Reverse;
use std::collections::HashSet;

use schedule_query::{
    ChangeTime, Connection, InterchangeData, minimum_change_time, normalize_tiploc,
};

use crate::csa::{Journey, JourneyLeg};
use crate::raptor::RaptorJourney;
use crate::restrictions::Restrictions;
use crate::staged::StagedJourney;

/// Alternatives added to the Pareto set at most.
pub const MAX_ALTERNATIVES: usize = 2;
/// Options returned at most: the Pareto set (rarely more than three) plus
/// alternatives, fewer alternatives when the set is already large.
pub const MAX_OPTIONS: usize = 5;
/// Extra searches one request may run looking for alternatives.
pub const MAX_SEARCHES: usize = 3;
/// The detour allowance's floor and ceiling, minutes.
pub const MIN_ALLOWANCE: u32 = 20;
pub const MAX_ALLOWANCE: u32 = 90;

/// How much later than the best option an alternative may arrive (or,
/// arrive-by, how much earlier it may leave), and how much longer it may
/// take: half the best option's duration, at least [`MIN_ALLOWANCE`] and at
/// most [`MAX_ALLOWANCE`] minutes.
#[must_use]
pub fn allowance_minutes(best_duration: u32) -> u32 {
    (best_duration / 2).clamp(MIN_ALLOWANCE, MAX_ALLOWANCE)
}

/// What [`diversify`] needs of a journey.
pub trait OptionJourney {
    fn departure_min(&self) -> u32;
    fn arrival_min(&self) -> u32;
    fn changes(&self) -> u32;
    /// Every leg in order (a staged journey's parts flattened).
    fn all_legs(&self) -> Vec<&JourneyLeg>;
}

impl OptionJourney for RaptorJourney {
    fn departure_min(&self) -> u32 {
        self.departure_min
    }
    fn arrival_min(&self) -> u32 {
        self.arrival_min
    }
    fn changes(&self) -> u32 {
        self.changes
    }
    fn all_legs(&self) -> Vec<&JourneyLeg> {
        self.legs.iter().collect()
    }
}

impl OptionJourney for StagedJourney {
    fn departure_min(&self) -> u32 {
        self.departure_min
    }
    fn arrival_min(&self) -> u32 {
        self.arrival_min
    }
    fn changes(&self) -> u32 {
        self.changes
    }
    fn all_legs(&self) -> Vec<&JourneyLeg> {
        self.parts.iter().flat_map(|part| &part.legs).collect()
    }
}

/// A Connection Scan journey as a [`RaptorJourney`], for a caller whose
/// alternative search is [`crate::scan_connections_restricted`] or
/// [`crate::scan_connections_arrive_by`].
impl From<Journey> for RaptorJourney {
    fn from(journey: Journey) -> Self {
        let trains = journey
            .legs
            .iter()
            .filter(|leg| matches!(leg, JourneyLeg::Train(_)))
            .count();
        Self {
            changes: u32::try_from(trains.saturating_sub(1)).unwrap_or(u32::MAX),
            departure_min: journey.departure_min,
            arrival_min: journey.arrival_min,
            legs: journey.legs,
        }
    }
}

/// Which way the request is anchored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Best = earliest arrival.
    DepartAfter,
    /// Best = latest departure.
    ArriveBy,
}

/// The rank of a journey (smaller is better): depart-after, the earliest
/// arrival, then fewest changes, then the latest departure; arrive-by, the
/// latest departure, then the earliest arrival, then fewest changes.
#[must_use]
pub fn rank(direction: Direction, departure: u32, arrival: u32, changes: u32) -> (u32, u32, u32) {
    match direction {
        Direction::DepartAfter => (arrival, changes, u32::MAX - departure),
        Direction::ArriveBy => (u32::MAX - departure, arrival, changes),
    }
}

/// Whether a journey `(departure, arrival)` is within the detour bound of
/// `best` `(departure, arrival)`: see [`allowance_minutes`].
#[must_use]
pub fn within_bound(direction: Direction, best: (u32, u32), candidate: (u32, u32)) -> bool {
    let best_duration = best.1.saturating_sub(best.0);
    let allowance = allowance_minutes(best_duration);
    let duration = candidate.1.saturating_sub(candidate.0);
    let timely = match direction {
        Direction::DepartAfter => candidate.1 <= best.1 + allowance,
        Direction::ArriveBy => candidate.0 + allowance >= best.0,
    };
    timely && duration <= best_duration + allowance
}

/// Inputs to [`diversify`] besides the journeys and the search.
pub struct DiversifyOptions<'a> {
    /// The connections the journeys were found on (base, sorted by
    /// departure): where a train's calls between two stops are read from.
    pub connections: &'a [Connection],
    pub interchange: &'a InterchangeData,
    /// The request's own restrictions; the bans are added to a copy.
    pub restrictions: Option<&'a Restrictions>,
    /// TIPLOCs never banned: the ends (an access stop's too), waypoints and
    /// vias. Their stations' other TIPLOCs are protected with them.
    pub protected: Vec<String>,
    pub direction: Direction,
    pub max_changes: u32,
}

/// Why a journey is in the options when it is not one of the Pareto set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alternative {
    /// The station that tells this route apart from the best option (its
    /// first change or boarding station the best one doesn't use, else its
    /// most important call the best one doesn't make). `None` when nothing
    /// stands out.
    pub via_tiploc: Option<String>,
}

/// One option: a journey and, for an alternative, why.
#[derive(Debug, Clone)]
pub struct Choice<J> {
    pub journey: J,
    pub alternative: Option<Alternative>,
}

/// `base` (a Pareto set, in its own order) followed by up to
/// [`MAX_ALTERNATIVES`] alternatives found by `search`, best first. See the
/// module doc. `search(restrictions)` runs the caller's search under
/// `restrictions` (the request's own plus the bans) and returns its best
/// journey, or `None`.
pub fn diversify<J: OptionJourney>(
    base: Vec<J>,
    options: &DiversifyOptions<'_>,
    mut search: impl FnMut(&Restrictions) -> Option<J>,
) -> Vec<Choice<J>> {
    let wanted = MAX_ALTERNATIVES.min(MAX_OPTIONS.saturating_sub(base.len()));
    let Some(best_index) = (0..base.len()).min_by_key(|&i| {
        let j = &base[i];
        rank(
            options.direction,
            j.departure_min(),
            j.arrival_min(),
            j.changes(),
        )
    }) else {
        return Vec::new();
    };
    let protected = Protected::new(options.interchange, &options.protected);
    let profiles: Vec<Profile> = base
        .iter()
        .map(|j| Profile::of(j, options.connections, options.interchange))
        .collect();
    let best = (
        base[best_index].departure_min(),
        base[best_index].arrival_min(),
    );
    let fewest_changes = base.iter().map(OptionJourney::changes).min().unwrap_or(0);

    let mut kept: Vec<Profile> = profiles.clone();
    let mut alternatives: Vec<(J, Alternative)> = Vec::new();
    let mut bans = Bans::default();
    for profile in &profiles {
        bans.add_corridor(profile);
    }
    let mut relaxed = false;
    let mut searches = 0;
    while alternatives.len() < wanted && searches < MAX_SEARCHES {
        let (no_interchange, no_call) = bans.expanded(options.interchange, &protected);
        let found = if no_call.is_empty() && no_interchange.is_empty() {
            None
        } else {
            searches += 1;
            let restrictions = options
                .restrictions
                .cloned()
                .unwrap_or_default()
                .with_extra(no_interchange, no_call);
            search(&restrictions)
        };
        let accepted = found.and_then(|journey| {
            let profile = Profile::of(&journey, options.connections, options.interchange);
            let sane = journey.changes() <= options.max_changes
                && journey.changes() <= fewest_changes + 2
                && within_bound(
                    options.direction,
                    best,
                    (journey.departure_min(), journey.arrival_min()),
                )
                && !profile.backtracks()
                && !kept.iter().any(|other| profile.similar(other, &protected));
            sane.then_some((journey, profile))
        });
        match accepted {
            Some((journey, profile)) => {
                let via_tiploc =
                    profile.distinguishing(&profiles[best_index], options.interchange, &protected);
                bans.add_corridor(&profile);
                kept.push(profile);
                alternatives.push((journey, Alternative { via_tiploc }));
            }
            None if !relaxed && searches > 0 => {
                // Too strict: keep only the best journey's trunk (and what
                // the alternatives found so far rode) banned.
                relaxed = true;
                bans = Bans::default();
                bans.add_trunk(&profiles[best_index]);
                for profile in &kept[profiles.len()..] {
                    bans.add_trunk(profile);
                }
                let again = bans.expanded(options.interchange, &protected);
                if again.0.is_empty() && again.1.is_empty() {
                    break;
                }
            }
            None => break,
        }
    }

    assemble(base, alternatives, options.direction)
}

/// `base` in its own order, then `alternatives` best first.
fn assemble<J: OptionJourney>(
    base: Vec<J>,
    mut alternatives: Vec<(J, Alternative)>,
    direction: Direction,
) -> Vec<Choice<J>> {
    alternatives
        .sort_by_key(|(j, _)| rank(direction, j.departure_min(), j.arrival_min(), j.changes()));
    base.into_iter()
        .map(|journey| Choice {
            journey,
            alternative: None,
        })
        .chain(
            alternatives
                .into_iter()
                .map(|(journey, alternative)| Choice {
                    journey,
                    alternative: Some(alternative),
                }),
        )
        .collect()
}

/// A station's identity for comparing routes: its CRS when known, else its
/// normalized TIPLOC (a tram stop, an access stop).
fn station_key(interchange: &InterchangeData, tiploc: &str) -> String {
    let tiploc = normalize_tiploc(tiploc);
    interchange
        .tiploc_to_crs
        .get(tiploc)
        .cloned()
        .unwrap_or_else(|| tiploc.to_string())
}

/// Every TIPLOC of `tiploc`'s station (itself when it has no CRS).
fn station_tiplocs(interchange: &InterchangeData, tiploc: &str) -> Vec<String> {
    let tiploc = normalize_tiploc(tiploc);
    let mut out = vec![tiploc.to_string()];
    if let Some(siblings) = interchange
        .tiploc_to_crs
        .get(tiploc)
        .and_then(|crs| interchange.crs_to_tiplocs.get(crs))
    {
        out.extend(siblings.iter().map(|t| normalize_tiploc(t).to_string()));
    }
    out
}

/// The protected stations, by key.
struct Protected(HashSet<String>);

impl Protected {
    fn new(interchange: &InterchangeData, tiplocs: &[String]) -> Self {
        Self(
            tiplocs
                .iter()
                .map(|t| station_key(interchange, t))
                .collect(),
        )
    }

    fn contains(&self, interchange: &InterchangeData, tiploc: &str) -> bool {
        self.0.contains(&station_key(interchange, tiploc))
    }
}

/// The calls a train makes strictly between boarding at `from` (at `dep`)
/// and alighting at `to` (at `arr`): read off `connections` (sorted by
/// departure), whose consecutive hops of one train join its calls.
fn calls_between(
    connections: &[Connection],
    uid: &str,
    to: &str,
    dep: u32,
    arr: u32,
) -> Vec<String> {
    let start = connections.partition_point(|c| c.departure_min < dep);
    let mut calls: Vec<String> = connections[start..]
        .iter()
        .take_while(|c| c.departure_min < arr)
        .filter(|c| c.uid == uid && c.arrival_min <= arr)
        .map(|c| normalize_tiploc(&c.to_tiploc).to_string())
        .collect();
    if calls
        .last()
        .is_some_and(|last| last == normalize_tiploc(to))
    {
        calls.pop();
    }
    calls
}

/// What [`diversify`] compares and bans, worked out once per journey.
#[derive(Clone)]
struct Profile {
    /// Per leg: (train?, from key, to key).
    signature: Vec<(bool, String, String)>,
    /// Every station passed through, in order, as keys; consecutive repeats
    /// (a change, a walk's end) collapsed.
    visited: Vec<String>,
    /// TIPLOCs called at between boarding and alighting, every train leg.
    calls: Vec<String>,
    /// The longest train ride's intermediate calls.
    trunk_calls: Vec<String>,
    /// TIPLOCs between legs (changes, walk ends), the journey's ends excluded.
    boundaries: Vec<String>,
    /// Where the first train is boarded and the last one left.
    first_board: Option<String>,
    last_alight: Option<String>,
}

impl Profile {
    fn of<J: OptionJourney>(
        journey: &J,
        connections: &[Connection],
        interchange: &InterchangeData,
    ) -> Self {
        let legs = journey.all_legs();
        let mut signature = Vec::with_capacity(legs.len());
        let mut visited: Vec<String> = Vec::new();
        let mut calls = Vec::new();
        let mut trunk: (u32, Vec<String>) = (0, Vec::new());
        let mut boundaries = Vec::new();
        let (mut first_board, mut last_alight) = (None, None);
        let mut visit = |key: String| {
            if visited.last() != Some(&key) {
                visited.push(key);
            }
        };
        for (index, leg) in legs.iter().enumerate() {
            let (from, to, train) = match leg {
                JourneyLeg::Train(t) => (&t.from_tiploc, &t.to_tiploc, true),
                JourneyLeg::Transfer(t) => (&t.from_tiploc, &t.to_tiploc, false),
            };
            signature.push((
                train,
                station_key(interchange, from),
                station_key(interchange, to),
            ));
            visit(station_key(interchange, from));
            if let JourneyLeg::Train(t) = leg {
                let between = calls_between(
                    connections,
                    &t.uid,
                    &t.to_tiploc,
                    t.departure_min,
                    t.arrival_min,
                );
                for call in &between {
                    visit(station_key(interchange, call));
                }
                let ride = t.arrival_min.saturating_sub(t.departure_min);
                if ride > trunk.0 || trunk.1.is_empty() && ride == trunk.0 {
                    trunk = (ride, between.clone());
                }
                calls.extend(between);
                if first_board.is_none() {
                    first_board = Some(normalize_tiploc(&t.from_tiploc).to_string());
                }
                last_alight = Some(normalize_tiploc(&t.to_tiploc).to_string());
            }
            visit(station_key(interchange, to));
            if index > 0 {
                boundaries.push(normalize_tiploc(from).to_string());
            }
            if index + 1 < legs.len() {
                boundaries.push(normalize_tiploc(to).to_string());
            }
        }
        Self {
            signature,
            visited,
            calls,
            trunk_calls: trunk.1,
            boundaries,
            first_board,
            last_alight,
        }
    }

    /// Calls at (or passes through) some station twice: a loop or a
    /// back-track.
    fn backtracks(&self) -> bool {
        let mut seen = HashSet::new();
        !self.visited.iter().all(|key| seen.insert(key))
    }

    /// A near copy of `other`: mostly the same stations (three quarters of
    /// them), or the same legs between the same stations and half of them.
    /// The protected stations (ends, waypoints, vias), which every option
    /// shares, are not counted.
    fn similar(&self, other: &Self, protected: &Protected) -> bool {
        let stations = |profile: &Self| -> HashSet<String> {
            profile
                .visited
                .iter()
                .filter(|key| !protected.0.contains(*key))
                .cloned()
                .collect()
        };
        let (a, b) = (stations(self), stations(other));
        let union = a.union(&b).count();
        if union == 0 {
            return self.signature == other.signature;
        }
        #[expect(clippy::cast_precision_loss, reason = "station counts are tiny")]
        let overlap = a.intersection(&b).count() as f64 / union as f64;
        overlap >= 0.75 || (self.signature == other.signature && overlap >= 0.5)
    }

    /// The station that tells this route apart from `best`: see
    /// [`Alternative::via_tiploc`].
    fn distinguishing(
        &self,
        best: &Self,
        interchange: &InterchangeData,
        protected: &Protected,
    ) -> Option<String> {
        let best_keys: HashSet<&String> = best.visited.iter().collect();
        let usable = |tiploc: &String| {
            !protected.contains(interchange, tiploc)
                && !best_keys.contains(&station_key(interchange, tiploc))
        };
        if let Some(first) = self.first_board.as_ref().filter(|t| usable(t)) {
            return Some(first.clone());
        }
        if let Some(change) = self.boundaries.iter().find(|t| usable(t)) {
            return Some(change.clone());
        }
        // The call with the longest minimum change time: a proxy for the
        // most important station on the way (a junction, not a halt).
        let weight = |tiploc: &String| match minimum_change_time(interchange, tiploc) {
            ChangeTime::Finite(minutes) => minutes,
            ChangeTime::NoInterchange => 0,
        };
        let call = self
            .calls
            .iter()
            .enumerate()
            .filter(|(_, t)| usable(t))
            .min_by_key(|(index, t)| (Reverse(weight(t)), *index))
            .map(|(_, t)| t.clone());
        call.or_else(|| self.last_alight.clone().filter(usable))
    }
}

/// The TIPLOCs [`diversify`] bans next.
#[derive(Default)]
struct Bans {
    no_call: Vec<String>,
    no_interchange: Vec<String>,
}

impl Bans {
    fn add_corridor(&mut self, profile: &Profile) {
        self.no_call.extend(profile.calls.iter().cloned());
        self.no_interchange
            .extend(profile.boundaries.iter().cloned());
    }

    fn add_trunk(&mut self, profile: &Profile) {
        self.no_call.extend(profile.trunk_calls.iter().cloned());
    }

    /// Both lists with each station's other TIPLOCs added and the
    /// protected stations removed: `(no_interchange, no_call)`.
    fn expanded(
        &self,
        interchange: &InterchangeData,
        protected: &Protected,
    ) -> (Vec<String>, Vec<String>) {
        let expand = |list: &[String]| -> Vec<String> {
            let mut seen = HashSet::new();
            list.iter()
                .filter(|t| !protected.contains(interchange, t))
                .flat_map(|t| station_tiplocs(interchange, t))
                .filter(|t| seen.insert(t.clone()))
                .collect()
        };
        (expand(&self.no_interchange), expand(&self.no_call))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::NaiveDate;

    use super::*;
    use crate::{
        ArriveByOptions, RaptorOptions, ScanOptions, StagedOptions, raptor_search_restricted,
        scan_connections_arrive_by, scan_connections_restricted, scan_staged,
    };

    fn conn(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
        Connection {
            operator: schedule_query::OperatorId::UNKNOWN,
            uid: uid.to_string().into(),
            from_tiploc: from.to_string().into(),
            to_tiploc: to.to_string().into(),
            departure_min: dep,
            arrival_min: arr,
            working_departure_min: dep,
            working_arrival_min: arr,
            can_board: true,
            can_alight: true,
        }
    }

    /// A train calling at `stops` (TIPLOC, minute) in order.
    fn train(uid: &str, stops: &[(&str, u32)]) -> Vec<Connection> {
        stops
            .windows(2)
            .map(|pair| conn(uid, pair[0].0, pair[1].0, pair[0].1, pair[1].1))
            .collect()
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

    /// Each TIPLOC is its own station, named by itself as CRS; change
    /// times as given (5 elsewhere).
    fn interchange(tiplocs: &[&str], change_times: &[(&str, i32)]) -> InterchangeData {
        InterchangeData {
            modal_change: schedule_query::ModalChangeBuffer::default(),
            change_time_by_tiploc: change_times
                .iter()
                .map(|(t, m)| ((*t).to_string(), *m))
                .collect(),
            tiploc_to_crs: tiplocs
                .iter()
                .map(|t| ((*t).to_string(), (*t).to_string()))
                .collect(),
            crs_to_tiplocs: tiplocs
                .iter()
                .map(|t| ((*t).to_string(), vec![(*t).to_string()]))
                .collect(),
            fixed_links_from_crs: HashMap::new(),
        }
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 10, 30).unwrap()
    }

    fn s(v: &str) -> Vec<String> {
        vec![v.to_string()]
    }

    fn uids(legs: &[&JourneyLeg]) -> Vec<String> {
        legs.iter()
            .filter_map(|leg| match leg {
                JourneyLeg::Train(t) => Some(t.uid.clone()),
                JourneyLeg::Transfer(_) => None,
            })
            .collect()
    }

    /// London (L) to Manchester (M): the fast direct train via S every
    /// half hour, a slower direct one via C, a much slower one via X, and
    /// a looping one via S that comes back through L2... see each test.
    fn corridor_network() -> (Vec<Connection>, InterchangeData) {
        let mut connections = Vec::new();
        // The fast route, every 30 minutes: L 09:00 S 10:00 M 11:00.
        for (n, start) in [540u32, 570, 600, 630].into_iter().enumerate() {
            connections.extend(train(
                &format!("FAST{n}"),
                &[("L", start), ("S", start + 60), ("M", start + 120)],
            ));
        }
        // Another corridor, 40 minutes slower: L 09:05 C 10:20 M 11:45.
        connections.extend(train("SLOW", &[("L", 545), ("C", 620), ("M", 705)]));
        // A third, far too slow: L 09:10 X 11:30 M 13:30 (+150 min).
        connections.extend(train("CRAWL", &[("L", 550), ("X", 690), ("M", 810)]));
        (
            sorted(connections),
            interchange(&["L", "S", "C", "X", "M"], &[("C", 10), ("S", 5)]),
        )
    }

    fn depart_after(
        connections: &[Connection],
        ic: &InterchangeData,
        protected: Vec<String>,
        max_changes: u32,
    ) -> Vec<Choice<RaptorJourney>> {
        let (from, to) = (s("L"), s("M"));
        let options = |departure_min| RaptorOptions {
            connections,
            interchange: ic,
            from_tiplocs: &from,
            to_tiplocs: &to,
            departure_min,
            date: date(),
            max_rounds: max_changes + 2,
        };
        let base: Vec<RaptorJourney> = raptor_search_restricted(options(530), None, None)
            .into_iter()
            .filter(|j| j.changes <= max_changes)
            .collect();
        diversify(
            base,
            &DiversifyOptions {
                connections,
                interchange: ic,
                restrictions: None,
                protected: [from.clone(), to.clone(), protected].concat(),
                direction: Direction::DepartAfter,
                max_changes,
            },
            |restrictions| {
                scan_connections_restricted(
                    ScanOptions {
                        connections,
                        interchange: ic,
                        from_tiplocs: &from,
                        to_tiplocs: &to,
                        departure_min: 530,
                        date: date(),
                    },
                    None,
                    Some(restrictions),
                )
                .map(RaptorJourney::from)
            },
        )
    }

    #[test]
    fn the_allowance_is_half_the_duration_within_its_floor_and_ceiling() {
        assert_eq!(allowance_minutes(10), MIN_ALLOWANCE);
        assert_eq!(allowance_minutes(120), 60);
        assert_eq!(allowance_minutes(600), MAX_ALLOWANCE);
        assert!(within_bound(Direction::DepartAfter, (540, 660), (545, 705)));
        assert!(!within_bound(
            Direction::DepartAfter,
            (540, 660),
            (550, 810)
        ));
        assert!(within_bound(Direction::ArriveBy, (540, 660), (500, 640)));
        assert!(!within_bound(Direction::ArriveBy, (540, 660), (470, 590)));
        // Arriving in time but taking far longer is a detour too.
        assert!(!within_bound(
            Direction::DepartAfter,
            (540, 660),
            (400, 700)
        ));
    }

    /// The Pareto set alone is the 09:00 fast train. The same route at
    /// 09:30, 10:00 and 10:30 is not an option; the slower corridor via C
    /// is, labelled by C; the one via X is past the detour bound.
    #[test]
    fn an_alternative_corridor_is_added_and_later_departures_of_the_same_route_are_not() {
        let (connections, ic) = corridor_network();
        let choices = depart_after(&connections, &ic, Vec::new(), 4);
        let shape: Vec<(Vec<String>, Option<Option<String>>)> = choices
            .iter()
            .map(|c| {
                (
                    uids(&c.journey.all_legs()),
                    c.alternative.as_ref().map(|a| a.via_tiploc.clone()),
                )
            })
            .collect();
        assert_eq!(
            shape,
            vec![
                (vec!["FAST0".to_string()], None),
                (vec!["SLOW".to_string()], Some(Some("C".to_string()))),
            ]
        );
    }

    /// A protected station (a via or waypoint) is never banned: with S
    /// protected the fast route's corridor is empty, so no search runs and
    /// the Pareto set is returned as it was.
    #[test]
    fn protected_stations_are_never_banned() {
        let (connections, ic) = corridor_network();
        let choices = depart_after(&connections, &ic, s("S"), 4);
        assert_eq!(choices.len(), 1);
        assert!(choices[0].alternative.is_none());
    }

    /// A search that only finds a near copy of a kept journey (another
    /// train over the same stations) adds nothing.
    #[test]
    fn a_near_copy_of_a_kept_journey_is_dropped_by_its_signature() {
        let (connections, ic) = corridor_network();
        let (from, to) = (s("L"), s("M"));
        let base = raptor_search_restricted(
            RaptorOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 530,
                date: date(),
                max_rounds: 6,
            },
            None,
            None,
        );
        let copy = base[0].clone();
        let mut calls = 0;
        let choices = diversify(
            base,
            &DiversifyOptions {
                connections: &connections,
                interchange: &ic,
                restrictions: None,
                protected: [from.clone(), to.clone()].concat(),
                direction: Direction::DepartAfter,
                max_changes: 4,
            },
            |_| {
                calls += 1;
                // The 09:30 over the same stations.
                let mut later = copy.clone();
                if let JourneyLeg::Train(t) = &mut later.legs[0] {
                    t.uid = "FAST1".to_string();
                }
                later.departure_min += 30;
                later.arrival_min += 30;
                Some(later)
            },
        );
        assert_eq!(choices.len(), 1);
        // Tried strict, then relaxed, then gave up.
        assert_eq!(calls, 2);
    }

    /// A route that calls at a station twice (out to B and back through
    /// A) is never an option, however quick.
    #[test]
    fn a_back_tracking_route_is_rejected() {
        let mut connections = train("FAST", &[("L", 540), ("S", 600), ("M", 660)]);
        // L -> P -> Q -> P -> M: through P twice.
        connections.extend(train(
            "LOOP",
            &[("L", 541), ("P", 560), ("Q", 580), ("P", 600), ("M", 670)],
        ));
        let connections = sorted(connections);
        let ic = interchange(&["L", "S", "P", "Q", "M"], &[]);
        let choices = depart_after(&connections, &ic, Vec::new(), 4);
        assert_eq!(choices.len(), 1, "{choices:?}");
    }

    /// The change cap holds for alternatives too: the other corridor
    /// needs a change, so it is only offered when one is allowed.
    #[test]
    fn alternatives_respect_max_changes() {
        let mut connections = train("FAST", &[("L", 540), ("S", 600), ("M", 660)]);
        connections.extend(train("A1", &[("L", 545), ("C", 600)]));
        connections.extend(train("A2", &[("C", 615), ("M", 680)]));
        let connections = sorted(connections);
        let ic = interchange(&["L", "S", "C", "M"], &[]);
        assert_eq!(depart_after(&connections, &ic, Vec::new(), 0).len(), 1);
        let choices = depart_after(&connections, &ic, Vec::new(), 1);
        assert_eq!(choices.len(), 2);
        assert_eq!(uids(&choices[1].journey.all_legs()), ["A1", "A2"]);
        assert_eq!(
            choices[1]
                .alternative
                .as_ref()
                .and_then(|a| a.via_tiploc.as_deref()),
            Some("C")
        );
    }

    /// Two alternatives at most, each banning the corridors before it:
    /// the second is found only because the first's corridor is banned.
    #[test]
    fn iterative_exclusion_finds_a_third_route() {
        let mut connections = train("FAST", &[("L", 540), ("S", 600), ("M", 660)]);
        connections.extend(train("VIA_C", &[("L", 542), ("C", 610), ("M", 680)]));
        connections.extend(train("VIA_D", &[("L", 544), ("D", 615), ("M", 690)]));
        connections.extend(train("VIA_E", &[("L", 546), ("E", 620), ("M", 695)]));
        let connections = sorted(connections);
        let ic = interchange(&["L", "S", "C", "D", "E", "M"], &[]);
        let choices = depart_after(&connections, &ic, Vec::new(), 4);
        let got: Vec<Vec<String>> = choices
            .iter()
            .map(|c| uids(&c.journey.all_legs()))
            .collect();
        assert_eq!(got, [["FAST"], ["VIA_C"], ["VIA_D"]]);
    }

    /// Arrive-by: the best is the latest departure (the fast 10:30); the
    /// alternative is the other corridor's latest train, leaving earlier
    /// but within the bound. The 09:05 via C leaves too early to be one.
    #[test]
    fn arrive_by_alternatives_mirror_depart_after() {
        let (mut connections, ic) = corridor_network();
        connections.extend(train("SLOW2", &[("L", 590), ("C", 665), ("M", 750)]));
        let connections = sorted(connections);
        let (from, to) = (s("L"), s("M"));
        let deadline = 755; // 12:35
        let arrive_by = ArriveByOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: None,
            arrive_by_min: deadline,
            date: date(),
        };
        let base: Vec<RaptorJourney> =
            crate::raptor_arrive_by(ArriveByOptions { ..arrive_by }, None, None, 6);
        assert_eq!(base.len(), 1);
        assert_eq!(base[0].departure_min, 630);
        let choices = diversify(
            base,
            &DiversifyOptions {
                connections: &connections,
                interchange: &ic,
                restrictions: None,
                protected: [from.clone(), to.clone()].concat(),
                direction: Direction::ArriveBy,
                max_changes: 4,
            },
            |restrictions| {
                scan_connections_arrive_by(
                    ArriveByOptions { ..arrive_by },
                    None,
                    Some(restrictions),
                )
                .map(RaptorJourney::from)
            },
        );
        let got: Vec<Vec<String>> = choices
            .iter()
            .map(|c| uids(&c.journey.all_legs()))
            .collect();
        assert_eq!(got, [["FAST3"], ["SLOW2"]]);
    }

    /// Waypoints: the staged search's alternatives keep calling at the
    /// waypoint (protected), and a route avoiding it is never offered.
    #[test]
    fn staged_alternatives_keep_the_waypoint() {
        let mut connections = train("FAST", &[("L", 540), ("W", 570), ("S", 600), ("M", 660)]);
        connections.extend(train(
            "OTHER",
            &[("L", 545), ("W", 575), ("C", 620), ("M", 690)],
        ));
        // Skips the waypoint: never an option.
        connections.extend(train("NOWAY", &[("L", 546), ("D", 600), ("M", 650)]));
        let connections = sorted(connections);
        let ic = interchange(&["L", "W", "S", "C", "D", "M"], &[]);
        let (from, waypoints, to) = (s("L"), vec![s("W")], s("M"));
        let staged = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &waypoints,
            to_tiplocs: &to,
            vias: None,
            date: date(),
        };
        let base = crate::raptor_staged(&staged, 530, 6, None, None);
        assert_eq!(base.len(), 1);
        let choices = diversify(
            base,
            &DiversifyOptions {
                connections: &connections,
                interchange: &ic,
                restrictions: None,
                protected: [from.clone(), s("W"), to.clone()].concat(),
                direction: Direction::DepartAfter,
                max_changes: 4,
            },
            |restrictions| scan_staged(&staged, 530, None, Some(restrictions)),
        );
        let got: Vec<Vec<String>> = choices
            .iter()
            .map(|c| uids(&c.journey.all_legs()))
            .collect();
        assert_eq!(got, [["FAST", "FAST"], ["OTHER", "OTHER"]]);
    }

    /// The request's own restrictions still apply to the alternative
    /// searches: with C avoided, the corridor via C is never offered.
    #[test]
    fn the_requests_own_restrictions_still_apply() {
        let (connections, ic) = corridor_network();
        let (from, to) = (s("L"), s("M"));
        let avoid = Restrictions::new(Vec::new(), s("C"), HashMap::new());
        let base = raptor_search_restricted(
            RaptorOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 530,
                date: date(),
                max_rounds: 6,
            },
            None,
            Some(&avoid),
        );
        let choices = diversify(
            base,
            &DiversifyOptions {
                connections: &connections,
                interchange: &ic,
                restrictions: Some(&avoid),
                protected: [from.clone(), to.clone()].concat(),
                direction: Direction::DepartAfter,
                max_changes: 4,
            },
            |restrictions| {
                scan_connections_restricted(
                    ScanOptions {
                        connections: &connections,
                        interchange: &ic,
                        from_tiplocs: &from,
                        to_tiplocs: &to,
                        departure_min: 530,
                        date: date(),
                    },
                    None,
                    Some(restrictions),
                )
                .map(RaptorJourney::from)
            },
        );
        assert_eq!(choices.len(), 1);
    }

    /// An access end (a Tube stop reaching two stations) whose fastest
    /// route boards at one is offered the other station's route, labelled
    /// by the station it boards at.
    #[test]
    fn a_different_boarding_station_labels_the_alternative() {
        let mut connections = train("FAST", &[("E", 560), ("R", 600), ("B", 640)]);
        connections.extend(train("CHIL", &[("Y", 565), ("H", 620), ("B", 665)]));
        let connections = sorted(connections);
        let mut ic = interchange(&["E", "Y", "R", "H", "B"], &[]);
        let stop = "940GZZTEST";
        let links = [
            crate::AccessLink {
                crs: "E".to_string(),
                mode: "TUBE".to_string(),
                minutes: 12,
                windows: Vec::new(),
            },
            crate::AccessLink {
                crs: "Y".to_string(),
                mode: "TUBE".to_string(),
                minutes: 10,
                windows: Vec::new(),
            },
        ];
        ic = crate::with_access(
            &ic,
            Some(crate::AccessEnd {
                code: stop,
                links: &links,
            }),
            None,
            None,
        );
        let (from, to) = (s(stop), s("B"));
        let scan = |restrictions: Option<&Restrictions>| {
            scan_connections_restricted(
                ScanOptions {
                    connections: &connections,
                    interchange: &ic,
                    from_tiplocs: &from,
                    to_tiplocs: &to,
                    departure_min: 530,
                    date: date(),
                },
                None,
                restrictions,
            )
        };
        let base: Vec<RaptorJourney> = scan(None).map(RaptorJourney::from).into_iter().collect();
        assert_eq!(uids(&base[0].all_legs()), ["FAST"]);
        let choices = diversify(
            base,
            &DiversifyOptions {
                connections: &connections,
                interchange: &ic,
                restrictions: None,
                protected: [from.clone(), to.clone()].concat(),
                direction: Direction::DepartAfter,
                max_changes: 4,
            },
            |restrictions| scan(Some(restrictions)).map(RaptorJourney::from),
        );
        assert_eq!(choices.len(), 2, "{choices:?}");
        assert_eq!(uids(&choices[1].journey.all_legs()), ["CHIL"]);
        assert_eq!(
            choices[1]
                .alternative
                .as_ref()
                .and_then(|a| a.via_tiploc.as_deref()),
            Some("Y")
        );
    }

    /// CSA and RAPTOR agree on the alternative: under the same bans,
    /// RAPTOR's earliest arrival is the scan's.
    #[test]
    fn csa_and_raptor_agree_under_the_bans() {
        let (connections, ic) = corridor_network();
        let (from, to) = (s("L"), s("M"));
        let bans = Restrictions::default().with_extra(Vec::new(), s("S"));
        let csa = scan_connections_restricted(
            ScanOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 530,
                date: date(),
            },
            None,
            Some(&bans),
        )
        .expect("via C");
        let raptor = raptor_search_restricted(
            RaptorOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 530,
                date: date(),
                max_rounds: 6,
            },
            None,
            Some(&bans),
        );
        let best = raptor.iter().map(|j| j.arrival_min).min();
        assert_eq!(best, Some(csa.arrival_min));
        assert_eq!(csa.arrival_min, 705);
    }
}
