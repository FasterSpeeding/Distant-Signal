//! Journeys through ordered waypoints, searched as ONE journey rather than
//! as independently-chained segments.
//!
//! `/Trips/plan` used to solve `origin -> w1 -> ... -> destination` one
//! segment at a time, each later segment starting at the previous arrival
//! plus the waypoint's minimum change time. That had two faults (the
//! Distant-Signal-MCP team's review of the waypoint chaining, 2026-09-29):
//!
//! - a traveller staying aboard a train that calls at the waypoint was
//!   charged a change there that never happens, and could be forced off a
//!   through train that dwells less than the change time;
//! - `maxChanges` capped each segment separately, so the whole journey could
//!   exceed it.
//!
//! Here the search state carries a STAGE: stage `s` means "the first `s`
//! waypoints have been called at". Every label (earliest arrival, "aboard
//! this train") is kept per stage. Arriving at waypoint `s`'s TIPLOCs at
//! stage `s` -- by train or on foot -- is also arriving there at stage
//! `s + 1`, and a traveller aboard a train that CALLS at waypoint `s` is
//! also aboard it at stage `s + 1`, with no change charged. The destination
//! is reached at the last stage. A waypoint is therefore satisfied by a
//! call (or a walk into it), never by a train running through it without
//! stopping -- the `viaStop` meaning in Skye's `train-mcp`
//! (`src/timetable/plan/constraints.ts`), whose `searchLeg` and
//! `mergeAdjacentLegs` handle the same through-train case after the fact.
//!
//! Pass-through vias ([`crate::via`], 2026-10-06) add a second, independent
//! coordinate: the VIA PROGRESS `v`, "the first `v` vias have been passed".
//! A state is the pair (stage, progress); riding a connection advances the
//! progress over everything the connection calls at or runs through
//! ([`Vias::advance`]), and arriving somewhere advances it over the
//! location. That needs no new part boundary: a ride whose progress
//! advances mid-train stays one leg. The destination is reached at the last
//! stage with every via passed. Waypoints keep their order, vias theirs;
//! the two lists interleave freely.
//!
//! A state (s, v) DOMINATES (s', v') when `s >= s'` and `v >= v'`: whatever
//! a journey can still do from the lesser state, it can do from the greater
//! one. Labels and rides at a dominated state are pruned (see `relax` and
//! `sweep`). Without vias the states are just the stages, and this is the
//! 2026-09-29 search unchanged.
//!
//! The Connection Scan ([`scan_staged`]) and RAPTOR ([`raptor_staged`])
//! share one sweep: RAPTOR reads the previous round's labels for a fresh
//! boarding, CSA its own. Rounds count trains across the WHOLE journey, a
//! ride continuing through a waypoint counting once, which is what makes
//! `maxChanges` an end-to-end cap. Every other rule is `csa.rs`'s: change
//! times, same-CRS siblings, fixed links, the live overlay and
//! [`crate::restrictions`].
//!
//! With no waypoints and no vias this is the plain search; `csa.rs`/
//! `raptor.rs` remain the implementation for that case, and a differential
//! test checks the two agree.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use schedule_query::{
    ChangeTime, Connection, InterchangeData, fixed_links_from, minimum_change_time,
    normalize_tiploc, sibling_tiplocs,
};

use crate::csa::{JourneyLeg, TrainLeg, TransferLeg};
use crate::overlay::ConnectionOverlay;
use crate::restrictions::{self, Restrictions};
use crate::via::{ViaHow, ViaLeg, Vias};

pub struct StagedOptions<'a> {
    /// Sorted as `build_connections` returns it.
    pub connections: &'a [Connection],
    pub interchange: &'a InterchangeData,
    pub from_tiplocs: &'a [String],
    /// The waypoints in order, each as every TIPLOC it covers.
    pub waypoints: &'a [Vec<String>],
    pub to_tiplocs: &'a [String],
    /// Pass-through vias, in order (`None`: none).
    pub vias: Option<&'a Vias>,
    pub date: NaiveDate,
}

/// One segment's share of a [`StagedJourney`]: the legs between one stop
/// of `origin, waypoints..., destination` and the next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JourneyPart {
    pub legs: Vec<JourneyLeg>,
    /// The first leg is the same train the previous part's last leg rode
    /// into the waypoint: the traveller stays aboard, no change is made.
    pub continues_previous_train: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedJourney {
    /// `waypoints.len() + 1` parts, in order.
    pub parts: Vec<JourneyPart>,
    pub departure_min: u32,
    pub arrival_min: u32,
    /// Changes over the whole journey: trains ridden minus one, a train
    /// continuing through a waypoint counted once.
    pub changes: u32,
    /// For each via in order, the leg that first satisfied it.
    pub via_legs: Vec<ViaLeg>,
}

/// How a label was reached.
#[derive(Debug, Clone)]
enum Reached {
    Train {
        connection: Connection,
        round: usize,
        boarding: usize,
    },
    Link {
        from_tiploc: String,
        mode: String,
        minutes: i32,
    },
    /// Reached at the lesser state `from`, at this same stop and time (the
    /// stop is that state's next waypoint, or its next via, or both).
    Advance { from: usize },
}

#[derive(Debug, Clone)]
enum Source {
    /// A fresh boarding, made ready by an arrival at `from` (same state;
    /// the previous round's labels in RAPTOR).
    Ready { from: String },
    /// Still aboard from the previous stage, whose ride (`previous`, in the
    /// same round's arena) arrived at the waypoint by `arriving`. Starts a
    /// new part.
    Continued {
        arriving: Connection,
        previous: usize,
    },
    /// Still aboard, at the same stage, from the ride `previous` whose via
    /// progress `arriving` advanced. The same leg.
    Carried {
        arriving: Connection,
        previous: usize,
    },
}

#[derive(Debug, Clone)]
struct Boarding {
    /// The first connection ridden from this boarding (for a continued or
    /// carried ride, set by the first connection after the change of state).
    first: Option<Connection>,
    source: Source,
    /// The state this ride is at.
    state: usize,
}

#[derive(Debug, Clone)]
struct Label {
    state: usize,
    time: u32,
    reached: Reached,
}

#[derive(Clone)]
struct Labels {
    /// Per stop, its labels: at most one per state.
    stops: HashMap<String, Vec<Label>>,
    best: Option<(u32, String)>,
    touched: bool,
}

impl Labels {
    fn new() -> Self {
        Self {
            stops: HashMap::new(),
            best: None,
            touched: false,
        }
    }

    fn label(&self, tiploc: &str, state: usize) -> Option<&Label> {
        self.stops
            .get(tiploc)
            .and_then(|labels| labels.iter().find(|label| label.state == state))
    }
}

/// Change time at a waypoint whose own is the `NoInterchange` sentinel: the
/// traveller asked to stop there, so the coach-stand "never change here"
/// marker says nothing about how long they need. The same 5 minutes
/// `schedule_query::minimum_change_time` gives a TIPLOC with no record, and
/// the same rule the chained planner used
/// (`trip_planning_itinerary::WAYPOINT_FALLBACK_CHANGE_MINUTES`).
pub const WAYPOINT_FALLBACK_CHANGE_MINUTES: u32 = 5;

/// Minutes a fresh boarding at `tiploc` costs at `stage`, or `None` when it
/// is not allowed (an avoided station, or a `NoInterchange` sentinel that is
/// not the waypoint just reached).
pub(crate) fn change_minutes(
    interchange: &InterchangeData,
    restrictions: Option<&Restrictions>,
    targets: &[HashSet<String>],
    stage: usize,
    tiploc: &str,
) -> Option<u32> {
    if !restrictions::allows_interchange(restrictions, tiploc) {
        return None;
    }
    match minimum_change_time(interchange, tiploc) {
        ChangeTime::Finite(minutes) => Some(minutes),
        ChangeTime::NoInterchange
            if stage > 0 && targets[stage - 1].contains(normalize_tiploc(tiploc)) =>
        {
            Some(WAYPOINT_FALLBACK_CHANGE_MINUTES)
        }
        ChangeTime::NoInterchange => None,
    }
}

/// The (stage, via progress) grid both directions search over: state
/// `stage * width + progress`, `width` being the number of vias plus one.
/// Index order is lexicographic, so a dominating state never has a smaller
/// index than a state it dominates.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Grid {
    stages: usize,
    width: usize,
}

impl Grid {
    pub(crate) fn new(waypoints: usize, vias: Option<&Vias>) -> Self {
        Self {
            stages: waypoints + 1,
            width: vias.map_or(0, Vias::len) + 1,
        }
    }

    pub(crate) fn states(self) -> usize {
        self.stages * self.width
    }

    pub(crate) fn last(self) -> usize {
        self.states() - 1
    }

    pub(crate) fn state(self, stage: usize, progress: usize) -> usize {
        stage * self.width + progress
    }

    pub(crate) fn stage(self, state: usize) -> usize {
        state / self.width
    }

    pub(crate) fn progress(self, state: usize) -> usize {
        state % self.width
    }

    /// `a` dominates `b`, or is `b`: at least as far along both lists.
    pub(crate) fn covers(self, a: usize, b: usize) -> bool {
        if self.width == 1 {
            return a >= b;
        }
        self.stage(a) >= self.stage(b) && self.progress(a) >= self.progress(b)
    }
}

/// `Vias::advance`, treating "no vias" as no progress to make.
pub(crate) fn advance(vias: Option<&Vias>, progress: usize, connection: &Connection) -> usize {
    vias.map_or(progress, |vias| vias.advance(progress, connection))
}

/// `Vias::advance_at`, treating "no vias" as no progress to make.
pub(crate) fn advance_at(vias: Option<&Vias>, progress: usize, tiploc: &str) -> usize {
    vias.map_or(progress, |vias| vias.advance_at(progress, tiploc))
}

/// A connection one ride could take this sweep, at some state: already
/// aboard (the boarding), or boarding fresh, made ready by an arrival at the
/// connection's departure stop (`Fresh(0)`) or at its `n`th same-CRS
/// sibling (`Fresh(n)`).
enum Candidate {
    Aboard(usize),
    Fresh(usize),
}

struct Forward<'a> {
    interchange: &'a InterchangeData,
    restrictions: Option<&'a Restrictions>,
    vias: Option<&'a Vias>,
    grid: Grid,
    date: NaiveDate,
    origin: HashSet<String>,
    destinations: HashSet<String>,
    /// `targets[s]`: waypoint `s`'s TIPLOCs.
    targets: Vec<HashSet<String>>,
    departure_min: u32,
}

impl<'a> Forward<'a> {
    fn new(
        options: &StagedOptions<'a>,
        departure_min: u32,
        restrictions: Option<&'a Restrictions>,
    ) -> Self {
        let set = |tiplocs: &[String]| -> HashSet<String> {
            tiplocs
                .iter()
                .map(|t| normalize_tiploc(t).to_string())
                .collect()
        };
        Self {
            interchange: options.interchange,
            restrictions,
            vias: options.vias,
            grid: Grid::new(options.waypoints.len(), options.vias),
            date: options.date,
            origin: set(options.from_tiplocs),
            destinations: set(options.to_tiplocs),
            targets: options.waypoints.iter().map(|w| set(w)).collect(),
            departure_min,
        }
    }

    fn last_stage(&self) -> usize {
        self.targets.len()
    }

    fn initial_labels(&self) -> Labels {
        let mut labels = Labels::new();
        let origin: Vec<String> = self.origin.iter().cloned().collect();
        for tiploc in origin {
            self.relax_links(&mut labels, 0, &tiploc, self.departure_min);
        }
        labels
    }

    /// `csa::Scan::relax`, per state, plus the advances at a waypoint or via.
    fn relax(&self, labels: &mut Labels, state: usize, tiploc: &str, time: u32, reached: Reached) {
        let tiploc = normalize_tiploc(tiploc);
        if !restrictions::allows_interchange(self.restrictions, tiploc) {
            return;
        }
        let grid = self.grid;
        // A walk on from a bus or ferry is a change off it: the alighting
        // buffer comes first (`csa::alighting_buffer`). As in
        // `ready_states`, an advance (a waypoint or via reached here) owes
        // none of its own, matching `reverse::alighting_extra`.
        let walk_from = time
            + match &reached {
                Reached::Train { connection, .. } => {
                    self.interchange.modal_change.extra_for(&connection.uid)
                }
                Reached::Link { .. } | Reached::Advance { .. } => 0,
            };
        let label = Label {
            state,
            time,
            reached,
        };
        if let Some(existing) = labels.stops.get_mut(tiploc) {
            // Already here as early at this state, or at one further along
            // (see the module doc on dominance).
            if existing
                .iter()
                .any(|known| known.time <= time && grid.covers(known.state, state))
            {
                return;
            }
            match existing.iter_mut().find(|known| known.state == state) {
                Some(known) => *known = label,
                None => existing.push(label),
            }
        } else {
            labels.stops.insert(tiploc.to_string(), vec![label]);
        }
        labels.touched = true;
        if state == grid.last()
            && self.destinations.contains(tiploc)
            && labels.best.as_ref().is_none_or(|(best, _)| time < *best)
        {
            labels.best = Some((time, tiploc.to_string()));
        }
        let (at_stage, progress) = (grid.stage(state), grid.progress(state));
        if at_stage < self.last_stage() && self.targets[at_stage].contains(tiploc) {
            let next = grid.state(at_stage + 1, progress);
            self.relax(labels, next, tiploc, time, Reached::Advance { from: state });
        }
        let passed = advance_at(self.vias, progress, tiploc);
        if passed != progress {
            let next = grid.state(at_stage, passed);
            self.relax(labels, next, tiploc, time, Reached::Advance { from: state });
        }
        self.relax_links(labels, state, tiploc, walk_from);
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "the value is clamped to >= 0 first, and minute values fit easily"
    )]
    fn relax_links(&self, labels: &mut Labels, state: usize, from_tiploc: &str, at: u32) {
        let from_tiploc = normalize_tiploc(from_tiploc);
        let Some(crs) = self.interchange.tiploc_to_crs.get(from_tiploc) else {
            return;
        };
        for link in fixed_links_from(self.interchange, crs, self.date, at) {
            let Some(destinations) = self.interchange.crs_to_tiplocs.get(&link.to_crs) else {
                continue;
            };
            for to_tiploc in destinations {
                self.relax(
                    labels,
                    state,
                    to_tiploc,
                    at + link.minutes.max(0) as u32,
                    Reached::Link {
                        from_tiploc: from_tiploc.to_string(),
                        mode: link.mode.clone(),
                        minutes: link.minutes,
                    },
                );
            }
        }
    }

    /// `csa::Scan::ready_source_at` for every state at once: the states at
    /// which a fresh boarding at `tiploc` is ready by `departure`, the time,
    /// and the stop whose arrival made it ready (0: `tiploc` itself, `n`:
    /// `siblings[n - 1]`, which this fills). The origin only at the initial
    /// state. States covered by one in `aboard` are skipped: the ride already
    /// aboard there serves them. Includes the bus and ferry buffer on both
    /// sides of the change (`connection`'s side, and the arriving train's).
    fn ready_states(
        &self,
        labels: &Labels,
        connection: &Connection,
        aboard: &[usize],
        siblings: &mut Vec<&'a str>,
        out: &mut Vec<(usize, u32, usize)>,
    ) {
        out.clear();
        siblings.clear();
        let grid = self.grid;
        let departure = connection.departure_min;
        let boarding_extra = self.interchange.modal_change.extra_for(&connection.uid);
        let tiploc = normalize_tiploc(&connection.from_tiploc);
        let at_origin = self.origin.contains(tiploc);
        if at_origin
            && self.departure_min <= departure
            && !aboard.iter().any(|&known| grid.covers(known, 0))
        {
            out.push((0, self.departure_min, 0));
        }
        siblings.extend(sibling_tiplocs(self.interchange, tiploc));
        // The change time per stage, computed once.
        let mut changes: Vec<(usize, Option<u32>)> = Vec::new();
        for source in 0..=siblings.len() {
            let stop = if source == 0 {
                tiploc
            } else {
                siblings[source - 1]
            };
            let Some(found) = labels.stops.get(stop) else {
                continue;
            };
            for label in found {
                if label.state == 0 && at_origin {
                    continue;
                }
                if label.time > departure
                    || aboard.iter().any(|&known| grid.covers(known, label.state))
                {
                    continue;
                }
                let stage = grid.stage(label.state);
                let change =
                    if let Some(&(_, change)) = changes.iter().find(|(known, _)| *known == stage) {
                        change
                    } else {
                        let change = change_minutes(
                            self.interchange,
                            self.restrictions,
                            &self.targets,
                            stage,
                            tiploc,
                        );
                        changes.push((stage, change));
                        change
                    };
                let Some(change) = change else {
                    continue;
                };
                let alighting_extra = match &label.reached {
                    Reached::Train {
                        connection: arrived,
                        ..
                    } => self.interchange.modal_change.extra_for(&arrived.uid),
                    _ => 0,
                };
                let ready = label.time + change + boarding_extra + alighting_extra;
                if ready > departure {
                    continue;
                }
                match out.iter_mut().find(|(state, ..)| *state == label.state) {
                    Some(entry) if ready < entry.1 => *entry = (label.state, ready, source),
                    Some(_) => {}
                    None => out.push((label.state, ready, source)),
                }
            }
        }
    }

    /// One sweep. `previous`: the labels a fresh boarding reads (RAPTOR's
    /// previous round), `None` for CSA (its own). `stop_at_best`: CSA's
    /// "nothing departing after the best arrival can beat it".
    #[expect(
        clippy::too_many_lines,
        reason = "long but linear; splitting it would scatter its shared state across helpers"
    )]
    fn sweep<'c>(
        &self,
        connections: impl Iterator<Item = &'c Connection>,
        round: usize,
        previous: Option<&Labels>,
        current: &mut Labels,
        arena: &mut Vec<Boarding>,
        stop_at_best: bool,
    ) {
        let grid = self.grid;
        // Per train, the states it is ridden at and their boardings.
        let mut aboard: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
        let mut candidates: Vec<(usize, Candidate)> = Vec::new();
        let mut ready: Vec<(usize, u32, usize)> = Vec::new();
        let mut aboard_states: Vec<usize> = Vec::new();
        let mut siblings: Vec<&'a str> = Vec::new();
        let mut riding: Vec<usize> = Vec::new();
        let mut continuations: Vec<(usize, usize, bool)> = Vec::new();
        for connection in connections {
            if stop_at_best
                && current
                    .best
                    .as_ref()
                    .is_some_and(|(best, _)| connection.departure_min >= *best)
            {
                break;
            }
            if restrictions::blocks(self.restrictions, connection) {
                aboard.remove(&connection.uid);
                continue;
            }
            candidates.clear();
            aboard_states.clear();
            if let Some(rides) = aboard.get(&connection.uid) {
                candidates.extend(
                    rides
                        .iter()
                        .map(|&(state, boarding)| (state, Candidate::Aboard(boarding))),
                );
                aboard_states.extend(rides.iter().map(|&(state, _)| state));
            }
            // No fresh boarding at a set-down-only stop.
            if connection.can_board {
                let labels = previous.unwrap_or(&*current);
                self.ready_states(
                    labels,
                    connection,
                    &aboard_states,
                    &mut siblings,
                    &mut ready,
                );
                candidates.extend(
                    ready
                        .drain(..)
                        .map(|(state, _, source)| (state, Candidate::Fresh(source))),
                );
            }
            if candidates.is_empty() {
                continue;
            }
            // Furthest along first: once a state rides this connection, the
            // same ride at a state it dominates is pruned (see `relax`).
            candidates.sort_unstable_by_key(|(state, _)| std::cmp::Reverse(*state));
            riding.clear();
            continuations.clear();
            for (state, candidate) in candidates.drain(..) {
                if riding.iter().any(|&known| grid.covers(known, state)) {
                    continue;
                }
                riding.push(state);
                let boarding = match candidate {
                    Candidate::Aboard(boarding) => boarding,
                    Candidate::Fresh(source) => {
                        let from = if source == 0 {
                            normalize_tiploc(&connection.from_tiploc)
                        } else {
                            siblings[source - 1]
                        };
                        arena.push(Boarding {
                            first: Some(connection.clone()),
                            source: Source::Ready {
                                from: from.to_string(),
                            },
                            state,
                        });
                        aboard
                            .entry(connection.uid.clone())
                            .or_default()
                            .push((state, arena.len() - 1));
                        arena.len() - 1
                    }
                };
                if arena[boarding].first.is_none() {
                    arena[boarding].first = Some(connection.clone());
                }
                let stage = grid.stage(state);
                let progress = advance(self.vias, grid.progress(state), connection);
                // No arrival at a pick-up-only stop. Riding on through it --
                // including through a waypoint there -- is still allowed.
                if connection.can_alight {
                    self.relax(
                        current,
                        grid.state(stage, progress),
                        &connection.to_tiploc,
                        connection.arrival_min,
                        Reached::Train {
                            connection: connection.clone(),
                            round,
                            boarding,
                        },
                    );
                }
                let next_stage = if stage < self.last_stage()
                    && self.targets[stage].contains(normalize_tiploc(&connection.to_tiploc))
                {
                    stage + 1
                } else {
                    stage
                };
                let onward = grid.state(next_stage, progress);
                if onward != state {
                    continuations.push((onward, boarding, next_stage != stage));
                }
            }
            // Staying aboard through the waypoint (a new part) or past a via
            // (the same leg), from the NEXT connection on.
            for &(state, previous, new_part) in &continuations {
                let rides = aboard.entry(connection.uid.clone()).or_default();
                if rides.iter().any(|&(known, _)| grid.covers(known, state)) {
                    continue;
                }
                let source = if new_part {
                    Source::Continued {
                        arriving: connection.clone(),
                        previous,
                    }
                } else {
                    Source::Carried {
                        arriving: connection.clone(),
                        previous,
                    }
                };
                arena.push(Boarding {
                    first: None,
                    source,
                    state,
                });
                rides.retain(|&(known, _)| !grid.covers(state, known));
                rides.push((state, arena.len() - 1));
            }
        }
    }

    /// Walks back from the destination. `rounds[r]` is a round's labels and
    /// boarding arena; `single_pass` for CSA (one "round", no stepping back).
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::expect_used,
        clippy::too_many_lines,
        reason = "the value is clamped to >= 0 first, and minute values fit easily; a journey has a handful of legs; the invariant is established just above; the expect message names it; long but linear; splitting it would scatter its shared state across helpers"
    )]
    fn reconstruct(
        &self,
        rounds: &[(Labels, Vec<Boarding>)],
        start_round: usize,
        destination: &str,
        single_pass: bool,
    ) -> StagedJourney {
        let grid = self.grid;
        let stages = self.last_stage() + 1;
        let mut parts: Vec<Vec<JourneyLeg>> = vec![Vec::new(); stages];
        let mut continues = vec![false; stages];
        // Per via: (part, position from the END of that part's legs, how,
        // the TIPLOC that satisfied it).
        let via_count = grid.width - 1;
        let mut via_hits: Vec<Option<(usize, usize, ViaHow, String)>> = vec![None; via_count];
        // Vias passed by arriving at the stop being walked back from (and
        // that stop); the leg that arrived there satisfied them.
        let mut pending: Vec<(usize, String)> = Vec::new();
        let (mut round, mut state, mut stop) = (start_round, grid.last(), destination.to_string());
        let arrival_min = rounds[start_round]
            .0
            .label(&stop, state)
            .expect("internal error: the destination has a label")
            .time;

        while !(state == 0 && self.origin.contains(&stop)) {
            let label = rounds[round]
                .0
                .label(&stop, state)
                .unwrap_or_else(|| panic!("internal error: {stop} (state {state}) has no source"));
            match &label.reached {
                Reached::Advance { from } => {
                    pending.extend(
                        (grid.progress(*from)..grid.progress(state)).map(|via| (via, stop.clone())),
                    );
                    state = *from;
                }
                Reached::Link {
                    from_tiploc,
                    mode,
                    minutes,
                } => {
                    let part = grid.stage(state);
                    for (via, tiploc) in pending.drain(..) {
                        via_hits[via] = Some((part, parts[part].len(), ViaHow::Walk, tiploc));
                    }
                    parts[part].push(JourneyLeg::Transfer(TransferLeg {
                        mode: mode.clone(),
                        from_tiploc: from_tiploc.clone(),
                        to_tiploc: stop.clone(),
                        departure_min: label.time - (*minutes).max(0) as u32,
                        arrival_min: label.time,
                        minutes: *minutes,
                    }));
                    stop = from_tiploc.clone();
                }
                Reached::Train {
                    connection,
                    round: boarded_round,
                    boarding,
                } => {
                    let arena = &rounds[*boarded_round].1;
                    let (mut part, mut last, mut index) =
                        (grid.stage(state), connection.clone(), *boarding);
                    let progress_of = |index: usize| grid.progress(arena[index].state);
                    // This leg's via hits: (via, how, TIPLOC).
                    let mut hits: Vec<(usize, ViaHow, String)> = pending
                        .drain(..)
                        .map(|(via, tiploc)| (via, ViaHow::Call, tiploc))
                        .collect();
                    if let Some(vias) = self.vias {
                        hits.extend(vias.hits(progress_of(index), &last));
                    }
                    loop {
                        let ride = &arena[index];
                        if let Source::Carried { arriving, previous } = &ride.source {
                            if let Some(vias) = self.vias {
                                hits.extend(vias.hits(progress_of(*previous), arriving));
                            }
                            index = *previous;
                            continue;
                        }
                        let first = ride
                            .first
                            .as_ref()
                            .expect("internal error: a ridden boarding has a first connection");
                        for (via, how, tiploc) in hits.drain(..) {
                            via_hits[via] = Some((part, parts[part].len(), how, tiploc));
                        }
                        parts[part].push(JourneyLeg::Train(TrainLeg {
                            uid: first.uid.clone(),
                            from_tiploc: first.from_tiploc.clone(),
                            to_tiploc: last.to_tiploc.clone(),
                            departure_min: first.departure_min,
                            arrival_min: last.arrival_min,
                            working_departure_min: first.working_departure_min,
                            working_arrival_min: last.working_arrival_min,
                        }));
                        match &ride.source {
                            Source::Ready { from } => {
                                stop = normalize_tiploc(from).to_string();
                                state = ride.state;
                                if !single_pass {
                                    round = boarded_round - 1;
                                }
                                break;
                            }
                            Source::Continued { arriving, previous } => {
                                continues[part] = true;
                                part -= 1;
                                last = arriving.clone();
                                if let Some(vias) = self.vias {
                                    hits.extend(vias.hits(progress_of(*previous), arriving));
                                }
                                index = *previous;
                            }
                            Source::Carried { .. } => unreachable!("handled above"),
                        }
                    }
                }
            }
        }

        // Vias passed at the origin itself (an OR group with the origin as
        // one of its members, 2026-10-07): the first leg, which starts there.
        for (via, tiploc) in pending.drain(..) {
            let from_end = parts[0].len().saturating_sub(1);
            via_hits[via] = Some((0, from_end, ViaHow::Call, tiploc));
        }
        let via_legs: Vec<ViaLeg> = via_hits
            .into_iter()
            .map(|hit| match hit {
                Some((part, from_end, how, tiploc)) => ViaLeg {
                    part,
                    leg: parts[part].len().saturating_sub(1 + from_end),
                    how,
                    tiploc,
                },
                // Unreachable: the walk back from the last state passes every
                // progress step. Attributed to the first leg rather than
                // panicking in a request.
                None => ViaLeg {
                    part: 0,
                    leg: 0,
                    how: ViaHow::Call,
                    tiploc: String::new(),
                },
            })
            .collect();
        let parts: Vec<JourneyPart> = parts
            .into_iter()
            .zip(continues)
            .map(|(mut legs, continues_previous_train)| {
                legs.reverse();
                JourneyPart {
                    legs,
                    continues_previous_train,
                }
            })
            .collect();
        let rides: u32 = parts
            .iter()
            .map(|part| {
                part.legs
                    .iter()
                    .filter(|leg| matches!(leg, JourneyLeg::Train(_)))
                    .count() as u32
                    - u32::from(part.continues_previous_train)
            })
            .sum();
        let departure_min = parts
            .iter()
            .filter_map(|part| part.legs.first())
            .map(|leg| match leg {
                JourneyLeg::Train(t) => t.departure_min,
                JourneyLeg::Transfer(t) => t.departure_min,
            })
            .next()
            .unwrap_or(arrival_min);
        StagedJourney {
            parts,
            departure_min,
            arrival_min,
            changes: rides.saturating_sub(1),
            via_legs,
        }
    }
}

/// The earliest-arriving journey through every waypoint (and via) in
/// order, departing no earlier than `departure_min` (Connection Scan).
/// `None` when none reaches the destination.
pub fn scan_staged(
    options: &StagedOptions<'_>,
    departure_min: u32,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
) -> Option<StagedJourney> {
    let forward = Forward::new(options, departure_min, restrictions);
    let mut labels = forward.initial_labels();
    let mut arena = Vec::new();
    forward.sweep(
        crate::overlay::connections_from(options.connections, overlay, departure_min),
        0,
        None,
        &mut labels,
        &mut arena,
        true,
    );
    let (_, destination) = labels.best.clone()?;
    let rounds = [(labels, arena)];
    Some(forward.reconstruct(&rounds, 0, &destination, true))
}

/// The Pareto set over (arrival, changes for the whole journey) of journeys
/// through every waypoint (and via), using at most `max_rounds` trains
/// (RAPTOR). Fewest changes first, as `raptor::raptor_search`.
pub fn raptor_staged(
    options: &StagedOptions<'_>,
    departure_min: u32,
    max_rounds: u32,
    overlay: Option<&ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
) -> Vec<StagedJourney> {
    let forward = Forward::new(options, departure_min, restrictions);
    let mut rounds: Vec<(Labels, Vec<Boarding>)> = vec![(forward.initial_labels(), Vec::new())];
    for round in 1..=max_rounds as usize {
        let previous = &rounds[round - 1].0;
        let mut current = previous.clone();
        current.touched = false;
        let mut arena = Vec::new();
        forward.sweep(
            crate::overlay::connections_from(options.connections, overlay, departure_min),
            round,
            Some(previous),
            &mut current,
            &mut arena,
            false,
        );
        let touched = current.touched;
        rounds.push((current, arena));
        if !touched {
            break;
        }
    }

    let mut results: Vec<StagedJourney> = Vec::new();
    let mut running_best = u32::MAX;
    for round in 1..rounds.len() {
        let Some((arrival, destination)) = rounds[round].0.best.clone() else {
            continue;
        };
        if arrival >= running_best {
            continue;
        }
        running_best = arrival;
        results.push(forward.reconstruct(&rounds, round, &destination, false));
    }
    results
}

#[cfg(test)]
#[expect(
    clippy::cast_possible_truncation,
    clippy::too_many_lines,
    reason = "test code: casts of small known test values; scenario tests read top to bottom"
)]
mod tests {
    use super::*;
    use crate::{
        RaptorOptions, ScanOptions, raptor_search_restricted, scan_connections_restricted,
    };

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
            modal_change: schedule_query::ModalChangeBuffer::default(),
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

    fn part_uids(journey: &StagedJourney) -> Vec<Vec<String>> {
        journey
            .parts
            .iter()
            .map(|part| {
                part.legs
                    .iter()
                    .filter_map(|leg| match leg {
                        JourneyLeg::Train(t) => Some(t.uid.clone()),
                        JourneyLeg::Transfer(_) => None,
                    })
                    .collect()
            })
            .collect()
    }

    /// T1 runs A -> W -> C, dwelling 2 minutes at W, where a change needs
    /// 10. Via W, the traveller stays aboard: no phantom change, and T1 is
    /// not missed for want of 10 minutes at W.
    #[test]
    fn a_through_train_at_a_waypoint_needs_no_change() {
        let connections = sorted(vec![
            conn("T1", "A", "W", 480, 500),
            conn("T1", "W", "C", 502, 540),
            conn("T2", "W", "C", 515, 560),
        ]);
        let ic = interchange(&[("W", 10)]);
        let (from, waypoints, to) = (s("A"), vec![s("W")], s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &waypoints,
            to_tiplocs: &to,
            vias: None,
            date: date(),
        };
        let journey = scan_staged(&options, 0, None, None).expect("T1 through W");
        assert_eq!(part_uids(&journey), vec![vec!["T1"], vec!["T1"]]);
        assert!(journey.parts[1].continues_previous_train);
        assert_eq!(journey.changes, 0);
        assert_eq!(journey.arrival_min, 540);
        let JourneyLeg::Train(first) = &journey.parts[1].legs[0] else {
            panic!("a train leg");
        };
        assert_eq!(
            (first.from_tiploc.as_str(), first.departure_min),
            ("W", 502)
        );

        let raptor = raptor_staged(&options, 0, 1, None, None);
        assert_eq!(raptor.len(), 1, "one train, zero changes: {raptor:?}");
        assert_eq!(raptor[0].changes, 0);
        assert!(raptor[0].parts[1].continues_previous_train);
    }

    /// The change cap is end to end: A -> W needs one change (U1, U2) and
    /// W -> C another (U3, U4) -- plus the change at W, three in all.
    #[test]
    fn changes_are_counted_over_the_whole_journey() {
        let connections = sorted(vec![
            conn("U1", "A", "X", 480, 490),
            conn("U2", "X", "W", 495, 505),
            conn("U3", "W", "Y", 515, 525),
            conn("U4", "Y", "C", 530, 540),
        ]);
        let ic = interchange(&[("X", 5), ("W", 5), ("Y", 5)]);
        let (from, waypoints, to) = (s("A"), vec![s("W")], s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &waypoints,
            to_tiplocs: &to,
            vias: None,
            date: date(),
        };
        assert_eq!(scan_staged(&options, 0, None, None).unwrap().changes, 3);
        assert!(raptor_staged(&options, 0, 3, None, None).is_empty());
        let four = raptor_staged(&options, 0, 4, None, None);
        assert_eq!(four.len(), 1);
        assert_eq!(four[0].changes, 3);
    }

    /// Two waypoints on the same through train: still one ride.
    #[test]
    fn a_train_through_two_waypoints_is_one_ride_in_three_parts() {
        let connections = sorted(vec![
            conn("T1", "A", "W1", 480, 490),
            conn("T1", "W1", "W2", 491, 500),
            conn("T1", "W2", "C", 501, 520),
        ]);
        let ic = interchange(&[]);
        let (from, waypoints, to) = (s("A"), vec![s("W1"), s("W2")], s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &waypoints,
            to_tiplocs: &to,
            vias: None,
            date: date(),
        };
        let journey = scan_staged(&options, 0, None, None).unwrap();
        assert_eq!(
            part_uids(&journey),
            vec![vec!["T1"], vec!["T1"], vec!["T1"]]
        );
        assert_eq!(journey.changes, 0);
        assert!(
            journey.parts[1].continues_previous_train && journey.parts[2].continues_previous_train
        );
        let raptor = raptor_staged(&options, 0, 1, None, None);
        assert_eq!(raptor.len(), 1);
    }

    /// A train that passes a waypoint without calling does not satisfy it.
    #[test]
    fn a_waypoint_must_be_called_at() {
        let connections = sorted(vec![conn("T1", "A", "C", 480, 520)]);
        let ic = interchange(&[]);
        let (from, waypoints, to) = (s("A"), vec![s("W")], s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &waypoints,
            to_tiplocs: &to,
            vias: None,
            date: date(),
        };
        assert!(scan_staged(&options, 0, None, None).is_none());
    }

    /// `(can board at the next call, can alight at it)` for a random network:
    /// an intermediate call is set-down-only one time in seven and
    /// pick-up-only one time in seven; the last call is an ordinary one.
    fn random_direction(intermediate: bool, roll: u64) -> (bool, bool) {
        match (intermediate, roll) {
            (true, 0) => (false, true),
            (true, 1) => (true, false),
            _ => (true, true),
        }
    }

    /// With no waypoints the staged searches are the plain ones: same
    /// arrivals, same change counts, over random networks.
    #[test]
    fn with_no_waypoints_it_agrees_with_csa_and_raptor() {
        let stops = ["A", "B", "C", "D", "E", "F"];
        let mut seed: u64 = 0xfeed;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
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
                    // Some intermediate calls are set-down-only or
                    // pick-up-only (see `random_direction`).
                    let (board_next, alight) = random_direction(hop + 1 < hops, next(7));
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
            let ic = interchange(&[("C", 3), ("D", 99), ("E", 0)]);
            let (from, to) = (s("A"), s("F"));
            let staged = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &[],
                to_tiplocs: &to,
                vias: None,
                date: date(),
            };
            let restrictions = Restrictions::new(["B".to_string()], [], HashMap::new());
            for restricted in [None, Some(&restrictions)] {
                for departure in [300u32, 500, 700] {
                    let plain = scan_connections_restricted(
                        ScanOptions {
                            connections: &connections,
                            interchange: &ic,
                            from_tiplocs: &from,
                            to_tiplocs: &to,
                            departure_min: departure,
                            date: date(),
                        },
                        None,
                        restricted,
                    );
                    let ours = scan_staged(&staged, departure, None, restricted);
                    assert_eq!(
                        ours.as_ref().map(|j| j.arrival_min),
                        plain.as_ref().map(|j| j.arrival_min),
                        "case {case} departure {departure}"
                    );
                    let plain: Vec<(u32, u32)> = raptor_search_restricted(
                        RaptorOptions {
                            connections: &connections,
                            interchange: &ic,
                            from_tiplocs: &from,
                            to_tiplocs: &to,
                            departure_min: departure,
                            date: date(),
                            max_rounds: 4,
                        },
                        None,
                        restricted,
                    )
                    .iter()
                    .map(|j| (j.changes, j.arrival_min))
                    .collect();
                    let ours: Vec<(u32, u32)> =
                        raptor_staged(&staged, departure, 4, None, restricted)
                            .iter()
                            .map(|j| (j.changes, j.arrival_min))
                            .collect();
                    assert_eq!(ours, plain, "case {case} departure {departure}");
                }
            }
        }
    }

    /// With waypoints, the staged CSA and RAPTOR agree on the earliest
    /// arrival, and it is never later than chaining the segments with a
    /// change at every waypoint (the old behaviour) -- often earlier.
    #[test]
    fn with_waypoints_csa_and_raptor_agree_and_beat_chaining() {
        let stops = ["A", "B", "C", "D", "E", "F"];
        let mut seed: u64 = 0xbeef;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let (mut found, mut total) = (0, 0);
        for case in 0..80 {
            let mut connections = Vec::new();
            for train in 0..30 {
                let mut at = stops[next(stops.len() as u64) as usize];
                let mut time = 300 + next(600) as u32;
                let mut can_board_at = true;
                let hops = 1 + next(5);
                for hop in 0..hops {
                    let mut to = stops[next(stops.len() as u64) as usize];
                    if to == at {
                        to =
                            stops[(stops.iter().position(|s| *s == at).unwrap() + 1) % stops.len()];
                    }
                    let run = 5 + next(60) as u32;
                    // Some intermediate calls are set-down-only or
                    // pick-up-only (see `random_direction`).
                    let (board_next, alight) = random_direction(hop + 1 < hops, next(7));
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
            let ic = interchange(&[("C", 3), ("D", 99), ("E", 0), ("B", 7)]);
            let (from, waypoints, to) = (s("A"), vec![s("B"), s("D")], s("F"));
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: None,
                date: date(),
            };
            let csa = scan_staged(&options, 300, None, None);
            let raptor = raptor_staged(&options, 300, 12, None, None);
            assert_eq!(
                csa.as_ref().map(|j| j.arrival_min),
                raptor.iter().map(|j| j.arrival_min).min(),
                "case {case}"
            );
            // Chained: each segment from the previous arrival plus change.
            let mut ready = Some(300u32);
            for (a, b) in [("A", "B"), ("B", "D"), ("D", "F")] {
                ready = ready.and_then(|start| {
                    scan_connections_restricted(
                        ScanOptions {
                            connections: &connections,
                            interchange: &ic,
                            from_tiplocs: &s(a),
                            to_tiplocs: &s(b),
                            departure_min: start,
                            date: date(),
                        },
                        None,
                        None,
                    )
                    .map(|j| {
                        let change = match minimum_change_time(&ic, b) {
                            ChangeTime::Finite(m) => m,
                            ChangeTime::NoInterchange => 5,
                        };
                        j.arrival_min + if b == "F" { 0 } else { change }
                    })
                });
            }
            if let Some(chained) = ready {
                let joint = csa
                    .as_ref()
                    .expect("chaining found one, so must the joint search");
                assert!(joint.arrival_min <= chained, "case {case}");
            }
            // Every reported part really links up.
            if let Some(journey) = &csa {
                assert_eq!(journey.parts.len(), 3);
                for (index, part) in journey.parts.iter().enumerate() {
                    let end = part.legs.last().map(|leg| match leg {
                        JourneyLeg::Train(t) => t.to_tiploc.as_str(),
                        JourneyLeg::Transfer(t) => t.to_tiploc.as_str(),
                    });
                    assert_eq!(
                        end,
                        Some(["B", "D", "F"][index]),
                        "case {case}: {journey:?}"
                    );
                }
            }
            total += 1;
            found += usize::from(csa.is_some());
        }
        assert!(found * 4 > total, "only {found} of {total} reachable");
    }

    /// The backward scan through waypoints agrees with a brute-force search
    /// over the staged forward ones: CSA, and RAPTOR per number of trains.
    #[test]
    fn with_waypoints_the_backward_scan_agrees_with_brute_force() {
        use crate::reverse::{
            ArriveByOptions, latest_departure, latest_departures_by_trips, staged_arrive_by,
        };
        let stops = ["A", "B", "C", "D", "E", "F"];
        let mut seed: u64 = 0xabcd;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let mut found = 0;
        for case in 0..60 {
            let mut connections = Vec::new();
            for train in 0..30 {
                let mut at = stops[next(stops.len() as u64) as usize];
                let mut time = 300 + next(600) as u32;
                let mut can_board_at = true;
                let hops = 1 + next(5);
                for hop in 0..hops {
                    let mut to = stops[next(stops.len() as u64) as usize];
                    if to == at {
                        to =
                            stops[(stops.iter().position(|s| *s == at).unwrap() + 1) % stops.len()];
                    }
                    let run = 5 + next(60) as u32;
                    // Some intermediate calls are set-down-only or
                    // pick-up-only (see `random_direction`).
                    let (board_next, alight) = random_direction(hop + 1 < hops, next(7));
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
            let mut candidates: Vec<u32> = connections
                .iter()
                .filter(|c| c.from_tiploc == "A")
                .map(|c| c.departure_min)
                .collect();
            candidates.sort_unstable();
            candidates.dedup();
            candidates.reverse();
            let ic = interchange(&[("C", 3), ("D", 99), ("E", 0), ("B", 7)]);
            let (from, waypoints, to) = (s("A"), vec![s("B"), s("D")], s("F"));
            let restrictions = Restrictions::new(["E".to_string()], [], HashMap::new());
            for restricted in [None, Some(&restrictions)] {
                for deadline in [900u32, 1100, 1400] {
                    let staged = StagedOptions {
                        connections: &connections,
                        interchange: &ic,
                        from_tiplocs: &from,
                        waypoints: &waypoints,
                        to_tiplocs: &to,
                        vias: None,
                        date: date(),
                    };
                    let brute = candidates
                        .iter()
                        .copied()
                        .filter(|&d| d <= deadline)
                        .find(|&d| {
                            scan_staged(&staged, d, None, restricted)
                                .is_some_and(|j| j.arrival_min <= deadline)
                        });
                    let options = ArriveByOptions {
                        connections: &connections,
                        interchange: &ic,
                        from_tiplocs: &from,
                        waypoints: &waypoints,
                        to_tiplocs: &to,
                        vias: None,
                        arrive_by_min: deadline,
                        date: date(),
                    };
                    assert_eq!(
                        latest_departure(&options, None, restricted),
                        brute,
                        "case {case} deadline {deadline}"
                    );
                    assert_eq!(
                        staged_arrive_by(&options, None, restricted).map(|j| j.departure_min),
                        brute,
                        "case {case} deadline {deadline}"
                    );
                    found += usize::from(brute.is_some());
                    for (index, backward) in
                        latest_departures_by_trips(&options, None, restricted, 5)
                            .iter()
                            .enumerate()
                    {
                        let trains = index as u32 + 1;
                        let brute =
                            candidates
                                .iter()
                                .copied()
                                .filter(|&d| d <= deadline)
                                .find(|&d| {
                                    raptor_staged(&staged, d, trains, None, restricted)
                                        .iter()
                                        .any(|j| j.arrival_min <= deadline)
                                });
                        assert_eq!(
                            *backward, brute,
                            "case {case} deadline {deadline} trains {trains}"
                        );
                    }
                }
            }
        }
        assert!(found > 40, "only {found} reachable cases");
    }

    /// An independent oracle for the forward staged search: repeat "board
    /// any train anywhere it is catchable, ride it, alight anywhere" until
    /// nothing improves, keeping the earliest arrival per (stop, stage). No
    /// scan order, no pruning -- just the rules. The staged CSA and RAPTOR
    /// must find the same earliest arrival.
    fn oracle(
        connections: &[Connection],
        ic: &InterchangeData,
        origin: &str,
        waypoints: &[&str],
        destination: &str,
        departure: u32,
    ) -> Option<u32> {
        let stages = waypoints.len() + 1;
        let mut trains: HashMap<&str, Vec<&Connection>> = HashMap::new();
        for c in connections {
            trains.entry(c.uid.as_str()).or_default().push(c);
        }
        let mut best: HashMap<(String, usize), u32> = HashMap::new();
        let advance = |mut stage: usize, stop: &str| {
            while stage < waypoints.len() && waypoints[stage] == stop {
                stage += 1;
            }
            stage
        };
        let change = |stop: &str, stage: usize| -> Option<u32> {
            match minimum_change_time(ic, stop) {
                ChangeTime::Finite(m) => Some(m),
                ChangeTime::NoInterchange if stage > 0 && waypoints[stage - 1] == stop => {
                    Some(WAYPOINT_FALLBACK_CHANGE_MINUTES)
                }
                ChangeTime::NoInterchange => None,
            }
        };
        loop {
            let mut improved = false;
            for ride in trains.values() {
                for (board, c) in ride.iter().enumerate() {
                    for stage in 0..stages {
                        let ready = if stage == 0 && c.from_tiploc == origin {
                            Some(departure)
                        } else {
                            best.get(&(c.from_tiploc.clone(), stage))
                                .and_then(|&arrival| {
                                    change(&c.from_tiploc, stage).map(|m| arrival + m)
                                })
                        };
                        if ready.is_none_or(|ready| ready > c.departure_min) {
                            continue;
                        }
                        let mut on = stage;
                        for later in &ride[board..] {
                            let reached = advance(on, &later.to_tiploc);
                            on = reached;
                            let key = (later.to_tiploc.clone(), reached);
                            if best
                                .get(&key)
                                .is_none_or(|&known| later.arrival_min < known)
                            {
                                best.insert(key, later.arrival_min);
                                improved = true;
                            }
                        }
                    }
                }
            }
            if !improved {
                break;
            }
        }
        best.get(&(destination.to_string(), stages - 1)).copied()
    }

    #[test]
    fn the_staged_searches_agree_with_an_independent_oracle() {
        let stops = ["A", "B", "C", "D", "E", "F"];
        let mut seed: u64 = 0x0ac1e;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let mut found = 0;
        for case in 0..150 {
            let mut connections = Vec::new();
            for train in 0..25 {
                let mut at = stops[next(stops.len() as u64) as usize];
                let mut time = 300 + next(500) as u32;
                for _ in 0..=next(5) {
                    let mut to = stops[next(stops.len() as u64) as usize];
                    if to == at {
                        to =
                            stops[(stops.iter().position(|s| *s == at).unwrap() + 1) % stops.len()];
                    }
                    let run = 5 + next(50) as u32;
                    connections.push(conn(&format!("T{train}"), at, to, time, time + run));
                    time += run + next(4) as u32;
                    at = to;
                }
            }
            let connections = sorted(connections);
            let ic = interchange(&[("C", 3), ("D", 99), ("E", 0), ("B", 7)]);
            let via: &[&str] = match case % 3 {
                0 => &["B"],
                1 => &["B", "D"],
                _ => &["D", "C", "B"],
            };
            let waypoints: Vec<Vec<String>> = via.iter().map(|w| s(w)).collect();
            let (from, to) = (s("A"), s("F"));
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: None,
                date: date(),
            };
            let expected = oracle(&connections, &ic, "A", via, "F", 300);
            let csa = scan_staged(&options, 300, None, None).map(|j| j.arrival_min);
            let raptor = raptor_staged(&options, 300, 20, None, None)
                .iter()
                .map(|j| j.arrival_min)
                .min();
            assert_eq!(csa, expected, "case {case}");
            assert_eq!(raptor, expected, "case {case}");
            found += usize::from(expected.is_some());
        }
        assert!(found > 40, "only {found} reachable cases");
    }

    // ---- Pass-through vias (2026-10-06, `crate::via`) ----

    use crate::via::{PassSpan, ViaHow, ViaLeg, Vias};

    /// `targets` as vias; `passes` lists `(uid, from, passed TIPLOCs)`: the
    /// connection of `uid` leaving `from` runs through them. Every listed
    /// train gets its whole base order as spans.
    fn vias_for(
        connections: &[Connection],
        targets: &[&str],
        passes: &[(&str, &str, &[&str])],
    ) -> Vias {
        let groups: Vec<&[&str]> = targets.iter().map(std::slice::from_ref).collect();
        group_vias_for(connections, &groups, passes)
    }

    /// [`vias_for`] with each via an OR group of TIPLOCs (2026-10-07).
    fn group_vias_for(
        connections: &[Connection],
        groups: &[&[&str]],
        passes: &[(&str, &str, &[&str])],
    ) -> Vias {
        let mut spans: HashMap<String, Vec<PassSpan>> = HashMap::new();
        for (uid, _, _) in passes {
            if spans.contains_key(*uid) {
                continue;
            }
            let legs = connections
                .iter()
                .filter(|c| c.uid == *uid)
                .map(|c| PassSpan {
                    from_tiploc: c.from_tiploc.clone(),
                    to_tiploc: c.to_tiploc.clone(),
                    departure_min: c.departure_min,
                    passed: passes
                        .iter()
                        .filter(|(u, from, _)| u == uid && *from == c.from_tiploc)
                        .flat_map(|(_, _, passed)| passed.iter().map(|p| (*p).to_string()))
                        .collect(),
                })
                .collect();
            spans.insert((*uid).to_string(), legs);
        }
        let targets: Vec<Vec<String>> = groups
            .iter()
            .map(|group| group.iter().map(|t| (*t).to_string()).collect())
            .collect();
        Vias::new(&targets, spans)
    }

    fn uids_of(journey: &StagedJourney) -> Vec<String> {
        part_uids(journey).concat()
    }

    /// T1 runs A -> B -> C, passing X between A and B without calling; T2 is
    /// a faster direct train that goes nowhere near X. Via X, the traveller
    /// stays aboard T1 through X: one leg, satisfied by a pass.
    #[test]
    fn staying_aboard_through_a_passed_via_satisfies_it() {
        let connections = sorted(vec![
            conn("T1", "A", "B", 480, 520),
            conn("T1", "B", "C", 522, 560),
            conn("T2", "A", "C", 470, 540),
        ]);
        let ic = interchange(&[]);
        let vias = vias_for(&connections, &["X"], &[("T1", "A", &["X"])]);
        let (from, to) = (s("A"), s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            date: date(),
        };
        let journey = scan_staged(&options, 0, None, None).expect("T1 passes X");
        assert_eq!(part_uids(&journey), vec![vec!["T1"]]);
        assert_eq!(journey.parts[0].legs.len(), 1, "one ride is one leg");
        assert_eq!((journey.arrival_min, journey.changes), (560, 0));
        assert_eq!(
            journey.via_legs,
            vec![ViaLeg {
                part: 0,
                leg: 0,
                how: ViaHow::Pass,
                tiploc: "X".to_string(),
            }]
        );
        let raptor = raptor_staged(&options, 0, 1, None, None);
        assert_eq!(raptor.len(), 1);
        assert_eq!(raptor[0], journey);
        // Without the via, the faster train.
        let plain = StagedOptions {
            vias: None,
            ..options
        };
        assert_eq!(
            uids_of(&scan_staged(&plain, 0, None, None).unwrap()),
            vec!["T2"]
        );
    }

    /// The decoy `train-mcp` retries past (`legsTouchTarget`): T1 passes X
    /// and next calls at D; T2 reaches D sooner by another branch. Arriving
    /// at D is not passing X -- only the train actually ridden counts.
    #[test]
    fn a_different_branch_to_the_same_station_does_not_satisfy_a_via() {
        let connections = sorted(vec![
            conn("T1", "A", "D", 480, 600),
            conn("T2", "A", "D", 490, 550),
            conn("T3", "D", "C", 610, 650),
            conn("T4", "D", "C", 560, 600),
            // A train that passes X, never reachable from A.
            conn("T5", "E", "F", 300, 320),
        ]);
        let ic = interchange(&[("D", 5)]);
        let vias = vias_for(
            &connections,
            &["X"],
            &[("T1", "A", &["Y", "X"]), ("T5", "E", &["X"])],
        );
        let (from, to) = (s("A"), s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            date: date(),
        };
        let journey = scan_staged(&options, 0, None, None).expect("T1 then T3");
        assert_eq!(uids_of(&journey), vec!["T1", "T3"]);
        assert_eq!(journey.arrival_min, 650);
        assert_eq!(journey.via_legs[0].leg, 0);
        let raptor = raptor_staged(&options, 0, 4, None, None);
        assert_eq!(raptor.len(), 1);
        assert_eq!(uids_of(&raptor[0]), vec!["T1", "T3"]);
        let plain = StagedOptions {
            vias: None,
            ..options
        };
        assert_eq!(
            uids_of(&scan_staged(&plain, 0, None, None).unwrap()),
            vec!["T2", "T4"]
        );
    }

    /// T1 calls at X; T2 passes X without calling (slower); T3 -> T4 is the
    /// fastest, changing at X.
    #[test]
    fn a_via_combines_with_avoid_stop_and_avoid_change() {
        let connections = sorted(vec![
            conn("T1", "A", "X", 480, 500),
            conn("T1", "X", "C", 502, 540),
            conn("T2", "A", "C", 490, 560),
            conn("T3", "A", "X", 470, 490),
            conn("T4", "X", "C", 495, 520),
        ]);
        let ic = interchange(&[("X", 2)]);
        let vias = vias_for(&connections, &["X"], &[("T2", "A", &["X"])]);
        let (from, to) = (s("A"), s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            date: date(),
        };
        let run = |restrictions: Option<&Restrictions>| {
            let csa = scan_staged(&options, 0, None, restrictions);
            let raptor = raptor_staged(&options, 0, 4, None, restrictions)
                .into_iter()
                .min_by_key(|j| j.arrival_min);
            assert_eq!(
                csa.as_ref().map(|j| j.arrival_min),
                raptor.as_ref().map(|j| j.arrival_min)
            );
            csa
        };
        // Changing at X passes it.
        let journey = run(None).unwrap();
        assert_eq!(uids_of(&journey), vec!["T3", "T4"]);
        assert_eq!(
            journey.via_legs,
            vec![ViaLeg {
                part: 0,
                leg: 0,
                how: ViaHow::Call,
                tiploc: "X".to_string(),
            }]
        );
        // No change at X: stay aboard T1, which calls there.
        let no_change = Restrictions::new(["X".to_string()], [], HashMap::new());
        let journey = run(Some(&no_change)).unwrap();
        assert_eq!(uids_of(&journey), vec!["T1"]);
        assert_eq!(journey.via_legs[0].how, ViaHow::Call);
        // No call at X: only running through it is left.
        let no_call = Restrictions::new([], ["X".to_string()], HashMap::new());
        let journey = run(Some(&no_call)).unwrap();
        assert_eq!(uids_of(&journey), vec!["T2"]);
        assert_eq!(journey.via_legs[0].how, ViaHow::Pass);
        // Avoiding X altogether contradicts the via.
        let avoid = Restrictions::new(
            [],
            ["X".to_string()],
            HashMap::from([(
                "T2".to_string(),
                vec![crate::PassLeg {
                    from_tiploc: "A".to_string(),
                    to_tiploc: "C".to_string(),
                    blocked: true,
                }],
            )]),
        );
        assert!(run(Some(&avoid)).is_none());
    }

    /// T1 runs A -> W -> C, passing P before W and Q after it. Vias keep
    /// their own order but may fall either side of a waypoint.
    #[test]
    fn vias_keep_their_order_and_interleave_with_waypoints() {
        let connections = sorted(vec![
            conn("T1", "A", "W", 480, 500),
            conn("T1", "W", "C", 502, 540),
        ]);
        let ic = interchange(&[]);
        let passes: &[(&str, &str, &[&str])] = &[("T1", "A", &["P"]), ("T1", "W", &["Q"])];
        let (from, to, waypoints) = (s("A"), s("C"), vec![s("W")]);
        let plan = |targets: &[&str], waypoints: &[Vec<String>]| {
            let vias = vias_for(&connections, targets, passes);
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints,
                to_tiplocs: &to,
                vias: Some(&vias),
                date: date(),
            };
            let csa = scan_staged(&options, 0, None, None);
            let raptor = raptor_staged(&options, 0, 3, None, None);
            assert_eq!(csa.is_some(), !raptor.is_empty());
            csa.map(|j| {
                (
                    j.via_legs
                        .iter()
                        .map(|v| (v.part, v.leg))
                        .collect::<Vec<_>>(),
                    j,
                )
            })
        };
        let (hits, journey) = plan(&["P", "Q"], &waypoints).expect("in order");
        assert_eq!(hits, vec![(0, 0), (1, 0)]);
        assert!(journey.parts[1].continues_previous_train);
        assert_eq!(journey.changes, 0);
        assert!(plan(&["Q", "P"], &waypoints).is_none(), "out of order");
        assert_eq!(plan(&["Q"], &waypoints).unwrap().0, vec![(1, 0)]);
        assert_eq!(plan(&["P"], &waypoints).unwrap().0, vec![(0, 0)]);
        assert_eq!(plan(&["P", "Q"], &[]).unwrap().0, vec![(0, 0), (0, 0)]);
        // A call at W (here not a waypoint) satisfies a via there too.
        assert_eq!(plan(&["P", "W", "Q"], &[]).unwrap().0, vec![(0, 0); 3]);
    }

    #[test]
    fn an_unsatisfiable_via_finds_nothing_either_way() {
        use crate::reverse::{ArriveByOptions, latest_departures_by_trips, staged_arrive_by};
        let connections = sorted(vec![
            conn("T1", "A", "B", 480, 520),
            conn("T1", "B", "C", 522, 560),
        ]);
        let ic = interchange(&[]);
        let vias = vias_for(&connections, &["Z"], &[("T1", "A", &["X"])]);
        let (from, to) = (s("A"), s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            date: date(),
        };
        assert!(scan_staged(&options, 0, None, None).is_none());
        assert!(raptor_staged(&options, 0, 4, None, None).is_empty());
        let arrive = ArriveByOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            arrive_by_min: 1000,
            date: date(),
        };
        assert!(staged_arrive_by(&arrive, None, None).is_none());
        assert_eq!(
            latest_departures_by_trips(&arrive, None, None, 3),
            vec![None; 3]
        );
    }

    /// Arrive-by: T2 leaves later and arrives in time, but only T1 passes X.
    #[test]
    fn arrive_by_with_a_via_takes_the_latest_train_that_passes_it() {
        use crate::reverse::{ArriveByOptions, latest_departure, staged_arrive_by};
        let connections = sorted(vec![
            conn("T1", "A", "C", 480, 560),
            conn("T2", "A", "C", 500, 550),
            conn("T0", "A", "C", 400, 470),
        ]);
        let ic = interchange(&[]);
        let vias = vias_for(
            &connections,
            &["X"],
            &[("T1", "A", &["X"]), ("T0", "A", &["X"])],
        );
        let (from, to) = (s("A"), s("C"));
        let with_via = ArriveByOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            arrive_by_min: 600,
            date: date(),
        };
        let without = ArriveByOptions {
            vias: None,
            ..with_via
        };
        assert_eq!(latest_departure(&without, None, None), Some(500));
        assert_eq!(latest_departure(&with_via, None, None), Some(480));
        let journey = staged_arrive_by(&with_via, None, None).unwrap();
        assert_eq!(uids_of(&journey), vec!["T1"]);
        assert_eq!(journey.via_legs[0].how, ViaHow::Pass);
    }

    // ---- OR-group vias (2026-10-07): one via, several acceptable stations ----

    /// T3 is the fastest train but touches no member of {X, Y}. T1 runs
    /// through X (a pass); T2, later, calls at Y. Departing early the group
    /// is satisfied by T1 passing X; departing after T1 has gone, by T2
    /// calling at Y. Either way it is ONE via, reported with its member.
    #[test]
    fn a_group_via_is_satisfied_by_whichever_member_the_journey_reaches() {
        use crate::reverse::{ArriveByOptions, staged_arrive_by};
        let connections = sorted(vec![
            conn("T3", "A", "C", 470, 540),
            conn("T1", "A", "C", 480, 560),
            conn("T2", "A", "Y", 600, 630),
            conn("T2", "Y", "C", 632, 680),
        ]);
        let ic = interchange(&[]);
        let vias = group_vias_for(&connections, &[&["X", "Y"]], &[("T1", "A", &["X"])]);
        assert_eq!(vias.len(), 1);
        let (from, to) = (s("A"), s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            date: date(),
        };
        let plain = StagedOptions {
            vias: None,
            ..options
        };
        assert_eq!(
            uids_of(&scan_staged(&plain, 0, None, None).unwrap()),
            vec!["T3"]
        );

        // Early: pass-through on member X.
        let early = scan_staged(&options, 0, None, None).expect("T1 passes X");
        assert_eq!(uids_of(&early), vec!["T1"]);
        assert_eq!(
            early.via_legs,
            vec![ViaLeg {
                part: 0,
                leg: 0,
                how: ViaHow::Pass,
                tiploc: "X".to_string(),
            }]
        );
        assert_eq!(raptor_staged(&options, 0, 2, None, None)[0], early);

        // Later: a call at member Y.
        let late = scan_staged(&options, 490, None, None).expect("T2 calls at Y");
        assert_eq!(uids_of(&late), vec!["T2"]);
        assert_eq!(late.parts[0].legs.len(), 1, "staying aboard through Y");
        assert_eq!(
            late.via_legs,
            vec![ViaLeg {
                part: 0,
                leg: 0,
                how: ViaHow::Call,
                tiploc: "Y".to_string(),
            }]
        );

        // Arrive-by 700: the latest train satisfying the group is T2.
        let arrive = ArriveByOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            arrive_by_min: 700,
            date: date(),
        };
        let journey = staged_arrive_by(&arrive, None, None).unwrap();
        assert_eq!(uids_of(&journey), vec!["T2"]);
        assert_eq!(journey.via_legs[0].tiploc, "Y");
    }

    /// A group none of whose members any train calls at or runs through
    /// finds no journey, in every search.
    #[test]
    fn a_group_via_with_every_member_unreachable_finds_nothing() {
        use crate::reverse::{ArriveByOptions, latest_departures_by_trips, staged_arrive_by};
        let connections = sorted(vec![
            conn("T1", "A", "B", 480, 520),
            conn("T1", "B", "C", 522, 560),
        ]);
        let ic = interchange(&[]);
        let vias = group_vias_for(&connections, &[&["P", "Q", "R"]], &[("T1", "A", &["X"])]);
        let (from, to) = (s("A"), s("C"));
        let options = StagedOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            date: date(),
        };
        assert!(scan_staged(&options, 0, None, None).is_none());
        assert!(raptor_staged(&options, 0, 4, None, None).is_empty());
        let arrive = ArriveByOptions {
            connections: &connections,
            interchange: &ic,
            from_tiplocs: &from,
            waypoints: &[],
            to_tiplocs: &to,
            vias: Some(&vias),
            arrive_by_min: 1000,
            date: date(),
        };
        assert!(staged_arrive_by(&arrive, None, None).is_none());
        assert_eq!(
            latest_departures_by_trips(&arrive, None, None, 3),
            vec![None; 3]
        );
    }

    /// A group with the origin (or the destination) among its members is
    /// satisfied there: every journey passes it. The via is attributed to
    /// the first (last) leg, with that member.
    #[test]
    fn a_group_via_containing_the_origin_or_destination_is_satisfied_there() {
        use crate::reverse::{ArriveByOptions, staged_arrive_by};
        let connections = sorted(vec![
            conn("T1", "A", "B", 480, 520),
            conn("T2", "B", "C", 530, 560),
        ]);
        let ic = interchange(&[]);
        let (from, to) = (s("A"), s("C"));
        for (group, member, leg) in [(["Z", "A"], "A", 0), (["C", "Z"], "C", 1)] {
            let vias = group_vias_for(&connections, &[&group], &[]);
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &[],
                to_tiplocs: &to,
                vias: Some(&vias),
                date: date(),
            };
            let journey = scan_staged(&options, 0, None, None).expect("satisfied at an end");
            assert_eq!(uids_of(&journey), vec!["T1", "T2"], "{member}");
            assert_eq!(
                journey.via_legs,
                vec![ViaLeg {
                    part: 0,
                    leg,
                    how: ViaHow::Call,
                    tiploc: member.to_string(),
                }],
                "{member}"
            );
            assert_eq!(raptor_staged(&options, 0, 3, None, None).len(), 1);
            let arrive = ArriveByOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &[],
                to_tiplocs: &to,
                vias: Some(&vias),
                arrive_by_min: 600,
                date: date(),
            };
            let journey = staged_arrive_by(&arrive, None, None).expect("satisfied at an end");
            assert_eq!(journey.via_legs[0].tiploc, member);
        }
    }

    /// OR semantics, checked against the single-station vias: on random
    /// networks a group via {P, Q, B} arrives exactly as early as the best
    /// of the three single vias, departs (arrive-by) exactly as late as the
    /// best of them, and reports a member that the reported leg touches.
    #[test]
    fn a_group_via_is_the_best_of_its_members() {
        use crate::reverse::{ArriveByOptions, latest_departure};
        let mut seed = 0x0bad_5eed_u64;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let members = ["P", "Q", "B"];
        let mut found = 0;
        for case in 0..80 {
            let (connections, passes) = random_with_passes(&mut next, 25);
            let ic = interchange(&[("C", 3), ("D", 99), ("E", 0), ("B", 7)]);
            let rows: Vec<(&str, &str, Vec<&str>)> = passes
                .iter()
                .map(|(uid, from, passed)| {
                    (
                        uid.as_str(),
                        from.as_str(),
                        passed.iter().map(String::as_str).collect(),
                    )
                })
                .collect();
            let rows: Vec<(&str, &str, &[&str])> = rows
                .iter()
                .map(|(uid, from, passed)| (*uid, *from, passed.as_slice()))
                .collect();
            let (from, to) = (s("A"), s("F"));
            let search = |vias: &Vias| {
                let options = StagedOptions {
                    connections: &connections,
                    interchange: &ic,
                    from_tiplocs: &from,
                    waypoints: &[],
                    to_tiplocs: &to,
                    vias: Some(vias),
                    date: date(),
                };
                let arrive = ArriveByOptions {
                    connections: &connections,
                    interchange: &ic,
                    from_tiplocs: &from,
                    waypoints: &[],
                    to_tiplocs: &to,
                    vias: Some(vias),
                    arrive_by_min: 1200,
                    date: date(),
                };
                (
                    scan_staged(&options, 300, None, None),
                    raptor_staged(&options, 300, 8, None, None)
                        .iter()
                        .map(|j| j.arrival_min)
                        .min(),
                    latest_departure(&arrive, None, None),
                )
            };
            let singles: Vec<_> = members
                .iter()
                .map(|m| search(&vias_for(&connections, &[m], &rows)))
                .collect();
            let group = group_vias_for(&connections, &[&members], &rows);
            let (csa, raptor, latest) = search(&group);
            let best_arrival = singles
                .iter()
                .filter_map(|(j, _, _)| j.as_ref().map(|j| j.arrival_min))
                .min();
            let best_departure = singles.iter().filter_map(|(_, _, d)| *d).max();
            assert_eq!(
                csa.as_ref().map(|j| j.arrival_min),
                best_arrival,
                "case {case}"
            );
            assert_eq!(raptor, best_arrival, "case {case}");
            assert_eq!(latest, best_departure, "case {case}");
            if let Some(journey) = csa {
                found += 1;
                let hit = &journey.via_legs[0];
                assert!(members.contains(&hit.tiploc.as_str()), "case {case}");
                let JourneyLeg::Train(leg) = &journey.parts[hit.part].legs[hit.leg] else {
                    panic!("case {case}: no walks in this network");
                };
                let touches = connections
                    .iter()
                    .filter(|c| {
                        c.uid == leg.uid
                            && c.departure_min >= leg.departure_min
                            && c.arrival_min <= leg.arrival_min
                    })
                    .any(|c| {
                        c.from_tiploc == hit.tiploc
                            || c.to_tiploc == hit.tiploc
                            || rows.iter().any(|(uid, from, passed)| {
                                *uid == c.uid
                                    && *from == c.from_tiploc
                                    && passed.contains(&hit.tiploc.as_str())
                            })
                    });
                assert!(touches, "case {case}: {} not on {}", hit.tiploc, leg.uid);
            }
        }
        assert!(found > 10, "the networks exercise the group ({found})");
    }

    /// `(uid, from, passed)` rows of [`random_with_passes`].
    type PassRows = Vec<(String, String, Vec<String>)>;

    /// A random network whose connections sometimes run through P or Q
    /// (never called at) or through one of the ordinary stops.
    fn random_with_passes(
        next: &mut impl FnMut(u64) -> u64,
        trains: usize,
    ) -> (Vec<Connection>, PassRows) {
        let stops = ["A", "B", "C", "D", "E", "F"];
        let passing = ["P", "Q", "B", "D"];
        let mut connections = Vec::new();
        let mut rows = Vec::new();
        for train in 0..trains {
            let uid = format!("T{train}");
            let mut at = stops[next(stops.len() as u64) as usize];
            let mut time = 300 + next(500) as u32;
            for _ in 0..=next(5) {
                let mut to = stops[next(stops.len() as u64) as usize];
                if to == at {
                    to = stops[(stops.iter().position(|s| *s == at).unwrap() + 1) % stops.len()];
                }
                let run = 5 + next(50) as u32;
                if next(3) == 0 {
                    let mut passed = vec![passing[next(passing.len() as u64) as usize].to_string()];
                    if next(3) == 0 {
                        passed.push(passing[next(passing.len() as u64) as usize].to_string());
                    }
                    rows.push((uid.clone(), at.to_string(), passed));
                }
                connections.push(conn(&uid, at, to, time, time + run));
                time += run + next(4) as u32;
                at = to;
            }
        }
        (sorted(connections), rows)
    }

    fn vias_from(connections: &[Connection], targets: &[&str], passes: &PassRows) -> Vias {
        let borrowed: Vec<(&str, &str, Vec<&str>)> = passes
            .iter()
            .map(|(uid, from, passed)| {
                (
                    uid.as_str(),
                    from.as_str(),
                    passed.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        let refs: Vec<(&str, &str, &[&str])> = borrowed
            .iter()
            .map(|(uid, from, passed)| (*uid, *from, passed.as_slice()))
            .collect();
        vias_for(connections, targets, &refs)
    }

    /// The independent oracle above, with the via progress added to the
    /// state: ride any catchable train, advancing the progress over every
    /// connection ridden.
    fn oracle_with_vias(
        connections: &[Connection],
        ic: &InterchangeData,
        waypoints: &[&str],
        vias: &Vias,
        departure: u32,
    ) -> Option<u32> {
        let stages = waypoints.len() + 1;
        let mut trains: HashMap<&str, Vec<&Connection>> = HashMap::new();
        for c in connections {
            trains.entry(c.uid.as_str()).or_default().push(c);
        }
        let mut best: HashMap<(String, usize, usize), u32> = HashMap::new();
        let next_stage = |mut stage: usize, stop: &str| {
            while stage < waypoints.len() && waypoints[stage] == stop {
                stage += 1;
            }
            stage
        };
        let change = |stop: &str, stage: usize| -> Option<u32> {
            match minimum_change_time(ic, stop) {
                ChangeTime::Finite(m) => Some(m),
                ChangeTime::NoInterchange if stage > 0 && waypoints[stage - 1] == stop => {
                    Some(WAYPOINT_FALLBACK_CHANGE_MINUTES)
                }
                ChangeTime::NoInterchange => None,
            }
        };
        loop {
            let mut improved = false;
            for ride in trains.values() {
                for (board, c) in ride.iter().enumerate() {
                    for stage in 0..stages {
                        for progress in 0..=vias.len() {
                            let ready = if stage == 0 && progress == 0 && c.from_tiploc == "A" {
                                Some(departure)
                            } else {
                                best.get(&(c.from_tiploc.clone(), stage, progress))
                                    .and_then(|&arrival| {
                                        change(&c.from_tiploc, stage).map(|m| arrival + m)
                                    })
                            };
                            if ready.is_none_or(|ready| ready > c.departure_min) {
                                continue;
                            }
                            let (mut on, mut passed) = (stage, progress);
                            for later in &ride[board..] {
                                passed = vias.advance(passed, later);
                                on = next_stage(on, &later.to_tiploc);
                                let key = (later.to_tiploc.clone(), on, passed);
                                if best
                                    .get(&key)
                                    .is_none_or(|&known| later.arrival_min < known)
                                {
                                    best.insert(key, later.arrival_min);
                                    improved = true;
                                }
                            }
                        }
                    }
                }
            }
            if !improved {
                break;
            }
        }
        best.get(&("F".to_string(), stages - 1, vias.len()))
            .copied()
    }

    /// Random networks with passing points: the staged CSA and RAPTOR agree
    /// with the via-aware oracle, every reported via leg really passes its
    /// via, and the backward scan agrees with brute force over the forward
    /// searches (CSA, and RAPTOR per number of trains).
    #[test]
    fn with_vias_csa_raptor_the_oracle_and_the_backward_scan_agree() {
        use crate::reverse::{
            ArriveByOptions, latest_departure, latest_departures_by_trips, staged_arrive_by,
        };
        let mut seed: u64 = 0x0071_a5e5;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let (mut found, mut differs_from_plain) = (0, 0);
        for case in 0..120 {
            let (connections, passes) = random_with_passes(&mut next, 25);
            let ic = interchange(&[("C", 3), ("D", 99), ("E", 0), ("B", 7)]);
            let (targets, waypoint_names): (&[&str], &[&str]) = match case % 4 {
                0 => (&["P"], &[]),
                1 => (&["P", "Q"], &[]),
                2 => (&["Q"], &["C"]),
                _ => (&["B", "Q", "P"], &["E"]),
            };
            let vias = vias_from(&connections, targets, &passes);
            let waypoints: Vec<Vec<String>> = waypoint_names.iter().map(|w| s(w)).collect();
            let (from, to) = (s("A"), s("F"));
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: Some(&vias),
                date: date(),
            };
            let expected = oracle_with_vias(&connections, &ic, waypoint_names, &vias, 300);
            let csa = scan_staged(&options, 300, None, None);
            let raptor = raptor_staged(&options, 300, 20, None, None);
            assert_eq!(csa.as_ref().map(|j| j.arrival_min), expected, "case {case}");
            assert_eq!(
                raptor.iter().map(|j| j.arrival_min).min(),
                expected,
                "case {case}"
            );
            let plain = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: None,
                date: date(),
            };
            let plain = scan_staged(&plain, 300, None, None);
            differs_from_plain +=
                usize::from(plain.map(|j| j.arrival_min) != csa.as_ref().map(|j| j.arrival_min));
            found += usize::from(expected.is_some());

            // Every reported via leg is a train leg that really passes or
            // calls there, in order.
            for journey in csa.iter().chain(&raptor) {
                assert_eq!(journey.via_legs.len(), targets.len(), "case {case}");
                let mut previous = (0, 0);
                for (via, hit) in journey.via_legs.iter().enumerate() {
                    assert!((hit.part, hit.leg) >= previous, "case {case}: order");
                    previous = (hit.part, hit.leg);
                    let JourneyLeg::Train(leg) = &journey.parts[hit.part].legs[hit.leg] else {
                        panic!("case {case}: no walks in this network");
                    };
                    let touches = connections
                        .iter()
                        .filter(|c| {
                            c.uid == leg.uid
                                && c.departure_min >= leg.departure_min
                                && c.arrival_min <= leg.arrival_min
                        })
                        .any(|c| {
                            c.from_tiploc == targets[via]
                                || c.to_tiploc == targets[via]
                                || passes.iter().any(|(uid, from, passed)| {
                                    *uid == c.uid
                                        && *from == c.from_tiploc
                                        && passed.iter().any(|p| p == targets[via])
                                })
                        });
                    assert!(touches, "case {case}: via {via} on {leg:?}");
                }
            }

            // Arrive-by, against brute force over the forward searches.
            let mut candidates: Vec<u32> = connections
                .iter()
                .filter(|c| c.from_tiploc == "A")
                .map(|c| c.departure_min)
                .collect();
            candidates.sort_unstable();
            candidates.dedup();
            candidates.reverse();
            for deadline in [700u32, 1000] {
                let brute = candidates
                    .iter()
                    .copied()
                    .filter(|&d| d <= deadline)
                    .find(|&d| {
                        scan_staged(&options, d, None, None)
                            .is_some_and(|j| j.arrival_min <= deadline)
                    });
                let arrive = ArriveByOptions {
                    connections: &connections,
                    interchange: &ic,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias: Some(&vias),
                    arrive_by_min: deadline,
                    date: date(),
                };
                assert_eq!(
                    latest_departure(&arrive, None, None),
                    brute,
                    "case {case} deadline {deadline}"
                );
                assert_eq!(
                    staged_arrive_by(&arrive, None, None).map(|j| j.departure_min),
                    brute,
                    "case {case} deadline {deadline}"
                );
                for (index, backward) in latest_departures_by_trips(&arrive, None, None, 4)
                    .iter()
                    .enumerate()
                {
                    let trains = index as u32 + 1;
                    let brute = candidates
                        .iter()
                        .copied()
                        .filter(|&d| d <= deadline)
                        .find(|&d| {
                            raptor_staged(&options, d, trains, None, None)
                                .iter()
                                .any(|j| j.arrival_min <= deadline)
                        });
                    assert_eq!(
                        *backward, brute,
                        "case {case} deadline {deadline} trains {trains}"
                    );
                }
            }
        }
        assert!(found > 30, "only {found} reachable cases");
        assert!(
            differs_from_plain > 10,
            "the vias rarely mattered: {differs_from_plain}"
        );
    }

    /// `oracle_with_vias` with the bus and ferry buffer: a label also
    /// records the buffer its arrival owes a later change (the arriving
    /// train's, unless the stop is the waypoint the stage advanced at), so
    /// an earlier arrival by bus and a later one by train are both kept.
    fn oracle_with_vias_and_buffer(
        connections: &[Connection],
        ic: &InterchangeData,
        waypoints: &[&str],
        vias: &Vias,
        departure: u32,
    ) -> Option<u32> {
        let stages = waypoints.len() + 1;
        let extra = |uid: &str| ic.modal_change.extra_for(uid);
        let mut trains: HashMap<&str, Vec<&Connection>> = HashMap::new();
        for c in connections {
            trains.entry(c.uid.as_str()).or_default().push(c);
        }
        // (stop, stage, progress, buffer owed) -> earliest arrival.
        let mut best: HashMap<(String, usize, usize, u32), u32> = HashMap::new();
        let next_stage = |mut stage: usize, stop: &str| {
            while stage < waypoints.len() && waypoints[stage] == stop {
                stage += 1;
            }
            stage
        };
        let change = |stop: &str, stage: usize| -> Option<u32> {
            match minimum_change_time(ic, stop) {
                ChangeTime::Finite(m) => Some(m),
                ChangeTime::NoInterchange if stage > 0 && waypoints[stage - 1] == stop => {
                    Some(WAYPOINT_FALLBACK_CHANGE_MINUTES)
                }
                ChangeTime::NoInterchange => None,
            }
        };
        loop {
            let mut improved = false;
            for ride in trains.values() {
                for (board, c) in ride.iter().enumerate() {
                    let mut readies: Vec<(usize, usize, u32)> = Vec::new();
                    if c.from_tiploc == "A" {
                        readies.push((0, 0, departure));
                    }
                    for (&(ref stop, stage, progress, owed), &arrival) in &best {
                        if *stop != c.from_tiploc {
                            continue;
                        }
                        if let Some(m) = change(stop, stage) {
                            readies.push((stage, progress, arrival + m + owed + extra(&c.uid)));
                        }
                    }
                    for (stage, progress, ready) in readies {
                        if ready > c.departure_min {
                            continue;
                        }
                        let (mut on, mut passed) = (stage, progress);
                        for later in &ride[board..] {
                            passed = vias.advance(passed, later);
                            let before = on;
                            on = next_stage(on, &later.to_tiploc);
                            let owed = if on == before { extra(&later.uid) } else { 0 };
                            let key = (later.to_tiploc.clone(), on, passed, owed);
                            if best
                                .get(&key)
                                .is_none_or(|&known| later.arrival_min < known)
                            {
                                best.insert(key, later.arrival_min);
                                improved = true;
                            }
                        }
                    }
                }
            }
            if !improved {
                break;
            }
        }
        best.iter()
            .filter(|((stop, stage, progress, _), _)| {
                stop == "F" && *stage == stages - 1 && *progress == vias.len()
            })
            .map(|(_, &arrival)| arrival)
            .min()
    }

    /// The bus and ferry buffer and the vias together (the buffer applies on
    /// a bus side of a change, whatever the via progress): on random
    /// networks where a third of the trains are buses, the staged CSA and
    /// RAPTOR agree with an oracle tracking the buffer owed per label, and
    /// the backward scan with brute force over the forward searches.
    #[test]
    fn with_vias_and_the_bus_buffer_csa_raptor_the_oracle_and_the_backward_scan_agree() {
        use crate::reverse::{ArriveByOptions, latest_departure, staged_arrive_by};
        let mut seed: u64 = 0x00b0_5b0f;
        let mut next = |n: u64| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) % n
        };
        let (mut found, mut buffer_mattered, mut mismatches) = (0, 0, Vec::new());
        for case in 0..160 {
            let (connections, passes) = random_with_passes(&mut next, 25);
            let mut ic = interchange(&[("C", 3), ("D", 99), ("E", 0), ("B", 7)]);
            let buses: HashSet<String> = (0..25)
                .filter(|train| train % 3 == case % 3)
                .map(|train| format!("T{train}"))
                .collect();
            ic.modal_change = schedule_query::ModalChangeBuffer {
                road_or_water_uids: buses,
                minutes: 12,
            };
            let (targets, waypoint_names): (&[&str], &[&str]) = match case % 4 {
                0 => (&["P"], &[]),
                1 => (&["B", "Q"], &[]),
                2 => (&["Q"], &["C"]),
                _ => (&["P"], &["E"]),
            };
            let vias = vias_from(&connections, targets, &passes);
            let waypoints: Vec<Vec<String>> = waypoint_names.iter().map(|w| s(w)).collect();
            let (from, to) = (s("A"), s("F"));
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias: Some(&vias),
                date: date(),
            };
            let expected =
                oracle_with_vias_and_buffer(&connections, &ic, waypoint_names, &vias, 300);
            let csa = scan_staged(&options, 300, None, None).map(|j| j.arrival_min);
            let raptor = raptor_staged(&options, 300, 20, None, None)
                .iter()
                .map(|j| j.arrival_min)
                .min();
            if csa != expected || raptor != expected {
                mismatches.push((case, expected, csa, raptor));
            }
            let mut unbuffered = ic.clone();
            unbuffered.modal_change.minutes = 0;
            let plain = StagedOptions {
                interchange: &unbuffered,
                ..options
            };
            buffer_mattered +=
                usize::from(scan_staged(&plain, 300, None, None).map(|j| j.arrival_min) != csa);
            found += usize::from(expected.is_some());

            let mut candidates: Vec<u32> = connections
                .iter()
                .filter(|c| c.from_tiploc == "A")
                .map(|c| c.departure_min)
                .collect();
            candidates.sort_unstable();
            candidates.dedup();
            candidates.reverse();
            for deadline in [700u32, 1000] {
                let brute = candidates
                    .iter()
                    .copied()
                    .filter(|&d| d <= deadline)
                    .find(|&d| {
                        scan_staged(&options, d, None, None)
                            .is_some_and(|j| j.arrival_min <= deadline)
                    });
                let arrive = ArriveByOptions {
                    connections: &connections,
                    interchange: &ic,
                    from_tiplocs: &from,
                    waypoints: &waypoints,
                    to_tiplocs: &to,
                    vias: Some(&vias),
                    arrive_by_min: deadline,
                    date: date(),
                };
                assert_eq!(
                    latest_departure(&arrive, None, None),
                    brute,
                    "case {case} deadline {deadline}"
                );
                assert_eq!(
                    staged_arrive_by(&arrive, None, None).map(|j| j.departure_min),
                    brute,
                    "case {case} deadline {deadline}"
                );
            }
        }
        assert!(mismatches.is_empty(), "{mismatches:?}");
        assert!(found > 30, "only {found} reachable cases");
        assert!(
            buffer_mattered > 5,
            "the buffer rarely mattered: {buffer_mattered}"
        );
    }

    /// A walk on from a bus owes the bus buffer first, as in CSA and
    /// RAPTOR: the bus reaches the stop at 10:00, + 5 off the bus + an
    /// 8-minute walk to H + H's 2-minute change = 10:15, so the 10:14 is
    /// missed. Calling at W (on the bus), or passing it as a via, changes
    /// nothing: the via search state and the walk buffer coexist.
    #[test]
    fn walking_on_from_a_bus_owes_the_buffer_first() {
        let connections = sorted(vec![
            conn("BUS1", "A", "W", 540, 550),
            conn("BUS1", "W", "STOP", 551, 600),
            conn("T14", "H", "C", 614, 629),
            conn("T15", "H", "C", 615, 630),
        ]);
        let mut ic = interchange(&[("H", 2)]);
        ic.modal_change = schedule_query::ModalChangeBuffer {
            road_or_water_uids: HashSet::from(["BUS1".to_string()]),
            minutes: 5,
        };
        for (tiploc, crs) in [("STOP", "tiploc:STOP"), ("H", "HXX")] {
            ic.tiploc_to_crs.insert(tiploc.to_string(), crs.to_string());
            ic.crs_to_tiplocs
                .insert(crs.to_string(), vec![tiploc.to_string()]);
        }
        let walk = schedule_query::FixedLink {
            mode: "WALK".to_string(),
            to_crs: "HXX".to_string(),
            minutes: 8,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
        };
        ic.fixed_links_from_crs
            .insert("tiploc:STOP".to_string(), vec![walk]);
        let (from, to) = (s("A"), s("C"));
        let via_w = vias_for(&connections, &["W"], &[]);
        let via_stop = vias_for(&connections, &["STOP"], &[]);
        for (waypoints, vias) in [
            (vec![], None),
            (vec![s("W")], None),
            (vec![], Some(&via_w)),
            (vec![], Some(&via_stop)),
        ] {
            let case = format!("{waypoints:?} via {:?}", vias.is_some());
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                vias,
                date: date(),
            };
            let journey = scan_staged(&options, 500, None, None).expect("a journey");
            assert_eq!(journey.arrival_min, 630, "{case}");
            let raptor = raptor_staged(&options, 500, 4, None, None);
            assert_eq!(
                raptor.iter().map(|j| j.arrival_min).min(),
                Some(630),
                "{case}"
            );
        }
    }
}
