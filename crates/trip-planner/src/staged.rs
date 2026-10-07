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
//! The Connection Scan ([`scan_staged`]) and RAPTOR ([`raptor_staged`])
//! share one sweep: RAPTOR reads the previous round's labels for a fresh
//! boarding, CSA its own. Rounds count trains across the WHOLE journey, a
//! ride continuing through a waypoint counting once, which is what makes
//! `maxChanges` an end-to-end cap. Every other rule is `csa.rs`'s: change
//! times, same-CRS siblings, fixed links, the live overlay and
//! [`crate::restrictions`].
//!
//! With no waypoints this is the plain search; `csa.rs`/`raptor.rs` remain
//! the implementation for that case, and a differential test checks the two
//! agree.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use schedule_query::{
    ChangeTime, Connection, InterchangeData, fixed_links_from, minimum_change_time,
    normalize_tiploc, sibling_tiplocs,
};

use crate::csa::{JourneyLeg, TrainLeg, TransferLeg};
use crate::overlay::ConnectionOverlay;
use crate::restrictions::{self, Restrictions};

pub struct StagedOptions<'a> {
    /// Sorted as `build_connections` returns it.
    pub connections: &'a [Connection],
    pub interchange: &'a InterchangeData,
    pub from_tiplocs: &'a [String],
    /// The waypoints in order, each as every TIPLOC it covers.
    pub waypoints: &'a [Vec<String>],
    pub to_tiplocs: &'a [String],
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
}

#[derive(Debug, Clone)]
enum Via {
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
    /// Reached at the previous stage, at this same stop and time (the stop
    /// is that stage's waypoint).
    Advance,
}

#[derive(Debug, Clone)]
enum Source {
    /// A fresh boarding, made ready by an arrival at `from` (same stage;
    /// the previous round's labels in RAPTOR).
    Ready { from: String },
    /// Still aboard from the previous stage, whose ride (`previous`, in the
    /// same round's arena) arrived at the waypoint by `arriving`.
    Continued {
        arriving: Connection,
        previous: usize,
    },
}

#[derive(Debug, Clone)]
struct Boarding {
    /// The first connection ridden at this stage (for a continued ride, set
    /// by the first connection after the waypoint).
    first: Option<Connection>,
    source: Source,
}

#[derive(Clone)]
struct Labels {
    arrival: Vec<HashMap<String, u32>>,
    via: Vec<HashMap<String, Via>>,
    best: Option<(u32, String)>,
    touched: bool,
}

impl Labels {
    fn new(stages: usize) -> Self {
        Self {
            arrival: vec![HashMap::new(); stages],
            via: vec![HashMap::new(); stages],
            best: None,
            touched: false,
        }
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

struct Forward<'a> {
    interchange: &'a InterchangeData,
    restrictions: Option<&'a Restrictions>,
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
            date: options.date,
            origin: set(options.from_tiplocs),
            destinations: set(options.to_tiplocs),
            targets: options.waypoints.iter().map(|w| set(w)).collect(),
            departure_min,
        }
    }

    fn last(&self) -> usize {
        self.targets.len()
    }

    fn stages(&self) -> usize {
        self.targets.len() + 1
    }

    fn initial_labels(&self) -> Labels {
        let mut labels = Labels::new(self.stages());
        let origin: Vec<String> = self.origin.iter().cloned().collect();
        for tiploc in origin {
            self.relax_links(&mut labels, 0, &tiploc, self.departure_min);
        }
        labels
    }

    /// `csa::Scan::relax`, per stage, plus the stage advance at a waypoint.
    fn relax(&self, labels: &mut Labels, stage: usize, tiploc: &str, time: u32, via: Via) {
        let tiploc = normalize_tiploc(tiploc);
        if !restrictions::allows_interchange(self.restrictions, tiploc) {
            return;
        }
        if labels.arrival[stage]
            .get(tiploc)
            .is_some_and(|&known| known <= time)
        {
            return;
        }
        // Dominated: already here as early, further along the waypoints.
        // Whatever this label could lead to (calling at waypoint `stage`,
        // then the rest in order) is also open to the one further along.
        if labels.arrival[stage + 1..]
            .iter()
            .any(|later| later.get(tiploc).is_some_and(|&known| known <= time))
        {
            return;
        }
        // A walk on from a bus or ferry is a change off it: the alighting
        // buffer comes first (`csa::alighting_buffer`).
        let walk_from = time
            + match &via {
                Via::Train { connection, .. } => {
                    self.interchange.modal_change.extra_for(&connection.uid)
                }
                Via::Link { .. } | Via::Advance => 0,
            };
        labels.arrival[stage].insert(tiploc.to_string(), time);
        labels.via[stage].insert(tiploc.to_string(), via);
        labels.touched = true;
        if stage == self.last()
            && self.destinations.contains(tiploc)
            && labels.best.as_ref().is_none_or(|(best, _)| time < *best)
        {
            labels.best = Some((time, tiploc.to_string()));
        }
        if stage < self.last() && self.targets[stage].contains(tiploc) {
            self.relax(labels, stage + 1, tiploc, time, Via::Advance);
        }
        self.relax_links(labels, stage, tiploc, walk_from);
    }

    #[expect(
        clippy::cast_sign_loss,
        reason = "the value is clamped to >= 0 first, and minute values fit easily"
    )]
    fn relax_links(&self, labels: &mut Labels, stage: usize, from_tiploc: &str, at: u32) {
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
                    stage,
                    to_tiploc,
                    at + link.minutes.max(0) as u32,
                    Via::Link {
                        from_tiploc: from_tiploc.to_string(),
                        mode: link.mode.clone(),
                        minutes: link.minutes,
                    },
                );
            }
        }
    }

    /// `csa::Scan::ready_source_at`, per stage: the origin only at stage 0.
    /// Includes the bus and ferry buffer on both sides of the change.
    fn ready(
        &self,
        labels: &Labels,
        stage: usize,
        tiploc: &str,
        boarding_uid: &str,
    ) -> Option<(u32, String)> {
        let tiploc = normalize_tiploc(tiploc);
        if stage == 0 && self.origin.contains(tiploc) {
            return Some((self.departure_min, tiploc.to_string()));
        }
        let change = change_minutes(
            self.interchange,
            self.restrictions,
            &self.targets,
            stage,
            tiploc,
        )? + self.interchange.modal_change.extra_for(boarding_uid);
        let arrivals = &labels.arrival[stage];
        let alighting_extra = |at: &str| match labels.via[stage].get(at) {
            Some(Via::Train { connection, .. }) => {
                self.interchange.modal_change.extra_for(&connection.uid)
            }
            _ => 0,
        };
        let mut best: Option<(u32, String)> = arrivals.get(tiploc).map(|&arrival| {
            (
                arrival + change + alighting_extra(tiploc),
                tiploc.to_string(),
            )
        });
        for sibling in sibling_tiplocs(self.interchange, tiploc) {
            if let Some(&arrival) = arrivals.get(sibling) {
                let candidate = arrival + change + alighting_extra(sibling);
                if best.as_ref().is_none_or(|(time, _)| candidate < *time) {
                    best = Some((candidate, sibling.to_string()));
                }
            }
        }
        best
    }

    /// One sweep. `previous`: the labels a fresh boarding reads (RAPTOR's
    /// previous round), `None` for CSA (its own). `stop_at_best`: CSA's
    /// "nothing departing after the best arrival can beat it".
    fn sweep<'c>(
        &self,
        connections: impl Iterator<Item = &'c Connection>,
        round: usize,
        previous: Option<&Labels>,
        current: &mut Labels,
        arena: &mut Vec<Boarding>,
        stop_at_best: bool,
    ) {
        let stages = self.stages();
        let mut aboard: Vec<HashMap<String, usize>> = vec![HashMap::new(); stages];
        let mut continuations: Vec<(usize, usize)> = Vec::new();
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
                for riding in &mut aboard {
                    riding.remove(&connection.uid);
                }
                continue;
            }
            continuations.clear();
            // Highest stage first: once a stage rides this connection, the
            // same ride at a lower stage is dominated (see `relax`).
            for stage in (0..stages).rev() {
                let boarding = if let Some(&boarding) = aboard[stage].get(&connection.uid) {
                    boarding
                } else {
                    // No fresh boarding at a set-down-only stop.
                    if !connection.can_board {
                        continue;
                    }
                    let labels = previous.unwrap_or(&*current);
                    if stage > 0 && labels.arrival[stage].is_empty() {
                        continue;
                    }
                    let Some((ready, from)) =
                        self.ready(labels, stage, &connection.from_tiploc, &connection.uid)
                    else {
                        continue;
                    };
                    if ready > connection.departure_min {
                        continue;
                    }
                    arena.push(Boarding {
                        first: Some(connection.clone()),
                        source: Source::Ready { from },
                    });
                    aboard[stage].insert(connection.uid.clone(), arena.len() - 1);
                    arena.len() - 1
                };
                if arena[boarding].first.is_none() {
                    arena[boarding].first = Some(connection.clone());
                }
                // No arrival at a pick-up-only stop. Riding on through it --
                // including through a waypoint there -- is still allowed.
                if connection.can_alight {
                    self.relax(
                        current,
                        stage,
                        &connection.to_tiploc,
                        connection.arrival_min,
                        Via::Train {
                            connection: connection.clone(),
                            round,
                            boarding,
                        },
                    );
                }
                if stage < self.last()
                    && self.targets[stage].contains(normalize_tiploc(&connection.to_tiploc))
                {
                    continuations.push((stage + 1, boarding));
                }
                break;
            }
            // Staying aboard through the waypoint, from the NEXT connection
            // on (this one ends there).
            for &(stage, previous) in &continuations {
                if !aboard[stage].contains_key(&connection.uid) {
                    arena.push(Boarding {
                        first: None,
                        source: Source::Continued {
                            arriving: connection.clone(),
                            previous,
                        },
                    });
                    aboard[stage].insert(connection.uid.clone(), arena.len() - 1);
                }
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
        let stages = self.stages();
        let mut parts: Vec<Vec<JourneyLeg>> = vec![Vec::new(); stages];
        let mut continues = vec![false; stages];
        let (mut round, mut stage, mut stop) = (start_round, self.last(), destination.to_string());
        let arrival_min = rounds[start_round].0.arrival[stage][&stop];

        while !(stage == 0 && self.origin.contains(&stop)) {
            let via = rounds[round].0.via[stage]
                .get(&stop)
                .unwrap_or_else(|| panic!("internal error: {stop} (stage {stage}) has no source"));
            match via {
                Via::Advance => stage -= 1,
                Via::Link {
                    from_tiploc,
                    mode,
                    minutes,
                } => {
                    let arrival = rounds[round].0.arrival[stage][&stop];
                    parts[stage].push(JourneyLeg::Transfer(TransferLeg {
                        mode: mode.clone(),
                        from_tiploc: from_tiploc.clone(),
                        to_tiploc: stop.clone(),
                        departure_min: arrival - (*minutes).max(0) as u32,
                        arrival_min: arrival,
                        minutes: *minutes,
                    }));
                    stop = from_tiploc.clone();
                }
                Via::Train {
                    connection,
                    round: boarded_round,
                    boarding,
                } => {
                    let arena = &rounds[*boarded_round].1;
                    let (mut part, mut last, mut index) = (stage, connection.clone(), *boarding);
                    loop {
                        let ride = &arena[index];
                        let first = ride
                            .first
                            .as_ref()
                            .expect("internal error: a ridden boarding has a first connection");
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
                                stage = part;
                                if !single_pass {
                                    round = boarded_round - 1;
                                }
                                break;
                            }
                            Source::Continued { arriving, previous } => {
                                continues[part] = true;
                                part -= 1;
                                last = arriving.clone();
                                index = *previous;
                            }
                        }
                    }
                }
            }
        }

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
        }
    }
}

/// The earliest-arriving journey through every waypoint in order,
/// departing no earlier than `departure_min` (Connection Scan). `None` when
/// none reaches the destination.
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
/// through every waypoint, using at most `max_rounds` trains (RAPTOR).
/// Fewest changes first, as `raptor::raptor_search`.
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

    /// A walk on from a bus owes the bus buffer first, as in CSA and
    /// RAPTOR: the bus reaches the stop at 10:00, + 5 off the bus + an
    /// 8-minute walk to H + H's 2-minute change = 10:15, so the 10:14 is
    /// missed. Via W (on the bus) changes nothing.
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
            road_or_water_uids: std::collections::HashSet::from(["BUS1".to_string()]),
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
        for waypoints in [vec![], vec![s("W")]] {
            let options = StagedOptions {
                connections: &connections,
                interchange: &ic,
                from_tiplocs: &from,
                waypoints: &waypoints,
                to_tiplocs: &to,
                date: date(),
            };
            let journey = scan_staged(&options, 500, None, None).expect("a journey");
            assert_eq!(journey.arrival_min, 630, "{waypoints:?}");
            let raptor = raptor_staged(&options, 500, 4, None, None);
            assert_eq!(
                raptor.iter().map(|j| j.arrival_min).min(),
                Some(630),
                "{waypoints:?}"
            );
        }
    }
}
