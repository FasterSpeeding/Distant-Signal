//! RAPTOR: a round-based search over the SAME `Connection` array Connection
//! Scan (`csa.rs`) walks, returning a Pareto set over (arrival time, number
//! of changes) instead of CSA's single earliest-arrival answer. A faithful
//! Rust port of the real, working `Distant-Signal-MCP` sibling project's
//! `src/timetable/plan/raptor.ts` (re-cloned and re-read directly for this
//! plan's own research pass) -- **not** the classic, route-pattern-grouped
//! textbook RAPTOR the design spec's own §1/§2 describes; see this plan's
//! own Architecture section for why that description doesn't match the
//! real reference implementation, confirmed by reading this exact file.
//!
//! Round *k* answers "earliest arrival at each stop using at most *k*
//! trips" -- round *k* corresponds to *at most k-1 changes*. Every
//! interchange rule (same-train continuation is free, a fresh boarding
//! charges `minimum_change_time`, a same-CRS sibling change is offered at
//! the boarding TIPLOC's own minimum, fixed links are relaxed exactly as
//! `csa.rs`'s FIX-3 relaxation does) is duplicated from `csa.rs`, not
//! shared with it -- see this plan's Judgment Call 3 for why that
//! duplication is deliberate, not an oversight.
//!
//! **State (DS-225).** Each round used to start as a clone of the previous
//! round's `HashMap<String, _>`s, and every connection hashed its UID and
//! TIPLOCs. Now stops get a per-search index, a connection's stops and
//! train are looked up by their interned codes, and each stop keeps a short
//! history of its improvements tagged with their round (`Search::arrival`),
//! so a round costs what it improves. From round 2 a round's sweep starts
//! at the earliest arrival the round before improved: a boarding before it
//! could only repeat that round's arrivals. Results are unchanged; the
//! sweep is still over connections in departure order, not grouped into
//! route patterns, so restrictions that end a ride mid-train, the live
//! overlay's replacement trains and every tie-break behave exactly as
//! before.

use std::collections::HashMap;
use std::rc::Rc;

use chrono::NaiveDate;
use schedule_query::{
    ChangeTime, Connection, InterchangeData, LinkWindow, StopCode, TripCode, minimum_change_time,
    normalize_tiploc, sibling_tiplocs,
};

use crate::csa::{JourneyLeg, TrainLeg, TransferLeg};
use crate::dense::{DenseMap, ModalBuffer, StopLink};
use crate::restrictions::{self, Restrictions};

#[expect(
    clippy::cast_possible_truncation,
    reason = "a journey has a handful of legs and rounds"
)]
fn train_leg_count(legs: &[JourneyLeg]) -> u32 {
    legs.iter()
        .filter(|leg| matches!(leg, JourneyLeg::Train(_)))
        .count() as u32
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaptorJourney {
    /// Consecutive legs are normally contiguous -- one leg's `to_tiploc`
    /// equals the next leg's `from_tiploc` -- EXCEPT across a same-CRS
    /// sibling change (see `csa::Scan::ready_source_at`'s own doc comment,
    /// whose reasoning `ready_source_at` in this file duplicates): there,
    /// the walk between two platform groups of the same physical station
    /// (e.g. `WDON` -> `WIMBLDN`) is charged in time against the next
    /// leg's minimum change time, but is never materialized as its own
    /// [`TransferLeg`]. A consumer rendering these legs (e.g. by
    /// converting each one independently) must not assume strict
    /// `to_tiploc == from_tiploc` contiguity between consecutive legs --
    /// same discontinuity as `csa::Journey::legs`'s own doc comment.
    pub legs: Vec<JourneyLeg>,
    pub departure_min: u32,
    /// May exceed 1440 -- see `schedule_query::Connection::arrival_min`.
    pub arrival_min: u32,
    /// `max(train_leg_count - 1, 0)` -- a `TransferLeg` is never itself a
    /// change (it's what MAKES the boarding either side of it a change).
    /// A journey that is a fixed link with no train ride at all has
    /// `changes: 0`, matching a direct no-change train journey's own
    /// `changes: 0` -- both are journeys with nothing to change BETWEEN.
    /// See this plan's own Review Focus for the test this distinction
    /// needs.
    pub changes: u32,
}

pub struct RaptorOptions<'a> {
    /// Sorted by `departure_min` ascending -- same contract as
    /// `csa::ScanOptions::connections`.
    pub connections: &'a [Connection],
    pub interchange: &'a InterchangeData,
    pub from_tiplocs: &'a [String],
    pub to_tiplocs: &'a [String],
    pub departure_min: u32,
    pub date: NaiveDate,
    /// REQUIRED reasoning, not an optional tuning knob: see this plan's
    /// Judgment Call 2. The sibling project's own library-level default of
    /// 8 rounds (up to 7 changes) is independently confirmed, by that
    /// project's own wide differential-test run, to be too low for at
    /// least one real query (a genuine 9-change-only route). Do not treat
    /// whatever numeric default this field's own type carries as safe to
    /// rely on implicitly -- the caller (Phase 5's route handler) MUST
    /// derive this from its own actual changes-cap-in-effect
    /// (`changes_cap + 1`, plus one further round of headroom), never omit
    /// it or copy a hardcoded "8" without that reasoning.
    pub max_rounds: u32,
}

/// How a stop's arrival in a given round was produced.
#[derive(Debug, Clone, Copy)]
enum ArrivalSource<'a> {
    /// The connection that arrived and the ride it was part of (an index
    /// into `Search::rides`, which records the round it was ridden in).
    /// Recorded here rather than looked up by UID afterwards: an arrival
    /// can be carried into later rounds, and with restrictions a train can
    /// be left and boarded again within a round -- see
    /// `csa::ArrivalSource::Train`.
    Train(&'a Connection, usize),
    Link {
        /// The stop walked from.
        from: usize,
        mode: &'a str,
        minutes: i32,
    },
}

/// A stop's arrival as of some round: one entry of its history (at most
/// one per round), newest first through `previous`.
#[derive(Debug, Clone, Copy)]
struct Arrival<'a> {
    round: usize,
    time: u32,
    via: ArrivalSource<'a>,
    /// The stop's arrival before this one (an index into
    /// `Search::arrivals`), from an earlier round or earlier in this one.
    previous: Option<usize>,
}

/// One ride: the connection boarded, the stop whose (previous-round)
/// readiness allowed it, and the round it was ridden in.
struct Ride<'a> {
    boarded: &'a Connection,
    from: usize,
    round: usize,
}

/// What a search knows about one stop. Stops are indexed per search, by
/// normalized TIPLOC, as in `csa::Scan`.
struct Stop<'a> {
    /// Normalized.
    tiploc: &'a str,
    origin: bool,
    /// Not avoided: may be alighted at, walked into and boarded at fresh.
    allows_interchange: bool,
    /// `minimum_change_time`, `None` for a `NoInterchange` sentinel.
    change_time: Option<u32>,
    /// Same-CRS siblings, in `sibling_tiplocs`' order (as the interchange
    /// data spells them, as `csa::Stop::siblings`). Looked up among the
    /// search's stops when boarding: one with no stop yet has no arrival.
    siblings: Vec<&'a str>,
    /// The newest entry of its arrival history (an index into
    /// `Search::arrivals`).
    latest: Option<usize>,
    /// Its CRS's fixed links in lookup order, each with the stops it leads
    /// to: read from the interchange data once per search (see
    /// [`Search::links_of`]).
    links: Option<Rc<[StopLink<'a>]>>,
}

/// A possible fresh boarding: when, and from which stop.
struct ReadySource {
    time: u32,
    from: usize,
}

/// One RAPTOR search's state (DS-225). Round *k*'s arrivals are not a
/// copy of round *k - 1*'s: each stop keeps a short history of its
/// improvements, each tagged with its round, and "the arrival as of round
/// *k*" is the newest entry no later than *k*. So a round costs only what
/// it improves, and nothing is hashed per connection: a connection's stops
/// and train are looked up by code.
struct Search<'a> {
    interchange: &'a InterchangeData,
    restrictions: Option<&'a Restrictions>,
    date: NaiveDate,
    departure_min: u32,
    modal: ModalBuffer,
    stop_ids: HashMap<&'a str, usize>,
    stop_by_code: DenseMap,
    stops: Vec<Stop<'a>>,
    arrivals: Vec<Arrival<'a>>,
    rides: Vec<Ride<'a>>,
    /// Per `TripCode`, the ride the traveller is on in the current round;
    /// `aboard` lists the trains set, to clear them at the round's end.
    reachable_trip: DenseMap,
    aboard: Vec<TripCode>,
}

impl<'a> Search<'a> {
    fn new(
        interchange: &'a InterchangeData,
        restrictions: Option<&'a Restrictions>,
        date: NaiveDate,
        departure_min: u32,
    ) -> Self {
        Self {
            interchange,
            restrictions,
            date,
            departure_min,
            modal: ModalBuffer::new(&interchange.modal_change),
            stop_ids: HashMap::new(),
            stop_by_code: DenseMap::with_len(StopCode::count()),
            stops: Vec::new(),
            arrivals: Vec::new(),
            rides: Vec::new(),
            reachable_trip: DenseMap::with_len(TripCode::count()),
            aboard: Vec::new(),
        }
    }

    /// `tiploc`'s (normalized) index, added on first sight.
    fn stop_id(&mut self, tiploc: &'a str) -> usize {
        if let Some(&id) = self.stop_ids.get(tiploc) {
            return id;
        }
        let allows_interchange = restrictions::allows_interchange(self.restrictions, tiploc);
        let change_time = match minimum_change_time(self.interchange, tiploc) {
            ChangeTime::Finite(minutes) => Some(minutes),
            ChangeTime::NoInterchange => None,
        };
        let id = self.stops.len();
        let interchange: &'a InterchangeData = self.interchange;
        self.stops.push(Stop {
            tiploc,
            origin: false,
            allows_interchange,
            change_time,
            siblings: sibling_tiplocs(interchange, tiploc),
            latest: None,
            links: None,
        });
        self.stop_ids.insert(tiploc, id);
        id
    }

    /// `code`'s stop, by its code (see `csa::Scan::stop_for`).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a search touches far fewer than u32::MAX stops"
    )]
    fn stop_for(&mut self, code: StopCode) -> usize {
        if let Some(id) = self.stop_by_code.get(code.index()) {
            return id as usize;
        }
        let id = self.stop_id(code.normalized().as_str());
        self.stop_by_code.set(code.index(), id as u32);
        id
    }

    /// `stop`'s arrival as of `round`: its newest entry no later.
    fn arrival(&self, stop: usize, round: usize) -> Option<&Arrival<'a>> {
        let mut at = self.stops[stop].latest;
        while let Some(index) = at {
            let arrival = &self.arrivals[index];
            if arrival.round <= round {
                return Some(arrival);
            }
            at = arrival.previous;
        }
        None
    }

    /// The bus or ferry buffer owed for getting off whatever `arrival` came
    /// by (see `csa::Label::ready`).
    fn alighting_extra(&self, arrival: &Arrival<'_>) -> u32 {
        match arrival.via {
            ArrivalSource::Train(connection, _) => self.modal.extra_for(connection.uid),
            ArrivalSource::Link { .. } => 0,
        }
    }

    /// See `csa::Scan::ready_source_at`, including its bus and ferry
    /// buffer: `connection`'s side, and the arriving service's side read
    /// off the previous round's arrival. Of the stop and its siblings, the
    /// earliest ready, the stop itself on a tie.
    fn ready_source_at(
        &self,
        from: usize,
        connection: &Connection,
        previous: usize,
    ) -> Option<ReadySource> {
        let stop = &self.stops[from];
        if stop.origin {
            return Some(ReadySource {
                time: self.departure_min,
                from,
            });
        }
        if !stop.allows_interchange {
            return None;
        }
        let change_time = stop.change_time? + self.modal.extra_for(connection.uid);
        let mut best: Option<ReadySource> = None;
        let siblings = stop
            .siblings
            .iter()
            .filter_map(|sibling| self.stop_ids.get(sibling).copied());
        for at in std::iter::once(from).chain(siblings) {
            let Some(arrival) = self.arrival(at, previous) else {
                continue;
            };
            let candidate = arrival.time + change_time + self.alighting_extra(arrival);
            if best.as_ref().is_none_or(|b| candidate < b.time) {
                best = Some(ReadySource {
                    time: candidate,
                    from: at,
                });
            }
        }
        best
    }

    /// Improves `stop`'s arrival in `round` to `time`, recording how it
    /// was reached, and relaxes every fixed link from it. A no-op when
    /// `time` does not improve on its arrival as of `round`, which keeps the
    /// recursion with [`Self::relax_fixed_links`] finite (every link takes
    /// strictly positive minutes). Returns whether it improved.
    fn relax(&mut self, round: usize, stop: usize, time: u32, via: ArrivalSource<'a>) -> bool {
        if !self.stops[stop].allows_interchange {
            return false;
        }
        if self
            .arrival(stop, round)
            .is_some_and(|known| time >= known.time)
        {
            return false;
        }
        // One entry per stop and round: a later improvement in the same
        // round replaces it (as the round's own map did), so a stop's
        // history stays at most a round long however often the walks
        // between dense links improve it.
        let latest = self.stops[stop].latest;
        let entry = match latest {
            Some(index) if self.arrivals[index].round == round => {
                let entry = &mut self.arrivals[index];
                entry.time = time;
                entry.via = via;
                *entry
            }
            _ => {
                let entry = Arrival {
                    round,
                    time,
                    via,
                    previous: latest,
                };
                self.stops[stop].latest = Some(self.arrivals.len());
                self.arrivals.push(entry);
                entry
            }
        };
        // A walk on from a bus or ferry is a change off it: the alighting
        // buffer comes first.
        let walk_from = time + self.alighting_extra(&entry);
        self.relax_fixed_links(round, stop, walk_from);
        true
    }

    /// `from`'s fixed links (see [`Stop::links`]), read on first use.
    fn links_of(&mut self, from: usize) -> Rc<[StopLink<'a>]> {
        if let Some(links) = &self.stops[from].links {
            return Rc::clone(links);
        }
        let interchange: &'a InterchangeData = self.interchange;
        let links: Rc<[StopLink<'a>]> = interchange
            .tiploc_to_crs
            .get(self.stops[from].tiploc)
            .map_or_else(Vec::new, |crs| StopLink::of(interchange, crs))
            .into();
        self.stops[from].links = Some(Rc::clone(&links));
        links
    }

    /// Relaxes every stop one fixed link from `from`, walked at `at_min`:
    /// `schedule_query::fixed_links_from_where`'s choice (the first link
    /// valid then to each CRS, in lookup order), over the stop's links read
    /// once per search.
    #[expect(
        clippy::cast_sign_loss,
        reason = "fixed-link minutes are small and strictly positive (see relax)"
    )]
    fn relax_fixed_links(&mut self, round: usize, from: usize, at_min: u32) {
        let links = self.links_of(from);
        let window = LinkWindow::new(self.date, at_min);
        let mut last_to: Option<&str> = None;
        for StopLink { link, to } in links.iter() {
            if !restrictions::allows_link(self.restrictions, link)
                || !window.applies(link)
                || last_to == Some(link.to_crs.as_str())
            {
                continue;
            }
            last_to = Some(link.to_crs.as_str());
            let candidate = at_min + link.minutes as u32;
            for tiploc in *to {
                let stop = self.stop_id(normalize_tiploc(tiploc));
                self.relax(
                    round,
                    stop,
                    candidate,
                    ArrivalSource::Link {
                        from,
                        mode: &link.mode,
                        minutes: link.minutes,
                    },
                );
            }
        }
    }

    /// Runs round `round`: boards every connection reachable from the
    /// previous round's arrivals and relaxes each boarding's arrivals (and
    /// their fixed links). Returns whether anything improved, which is what
    /// lets the round loop stop early.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a search boards far fewer than u32::MAX rides"
    )]
    fn run_round(
        &mut self,
        round: usize,
        connections: impl IntoIterator<Item = &'a Connection>,
    ) -> bool {
        let previous = round - 1;
        let mut improved = false;
        for connection in connections {
            // Unusable, and ends any ride on its train -- see
            // `crate::restrictions`'s module doc.
            if restrictions::blocks(self.restrictions, connection) {
                self.reachable_trip.clear(connection.uid.index());
                continue;
            }
            let ride = if let Some(ride) = self.reachable_trip.get(connection.uid.index()) {
                ride as usize
            } else {
                // No fresh boarding at a set-down-only stop.
                if !connection.can_board {
                    continue;
                }
                let from = self.stop_for(connection.from_tiploc);
                let Some(source) = self.ready_source_at(from, connection, previous) else {
                    continue;
                };
                if source.time > connection.departure_min {
                    continue;
                }
                self.rides.push(Ride {
                    boarded: connection,
                    from: source.from,
                    round,
                });
                let ride = self.rides.len() - 1;
                self.reachable_trip.set(connection.uid.index(), ride as u32);
                self.aboard.push(connection.uid);
                ride
            };
            // No arrival at a pick-up-only stop; the ride carries on.
            if connection.can_alight {
                let to = self.stop_for(connection.to_tiploc);
                improved |= self.relax(
                    round,
                    to,
                    connection.arrival_min,
                    ArrivalSource::Train(connection, ride),
                );
            }
        }
        for uid in self.aboard.drain(..) {
            self.reachable_trip.clear(uid.index());
        }
        improved
    }

    /// The earliest of `stops`' arrivals as of `round`, the first listed on
    /// a tie.
    fn best_of(&self, stops: &[usize], round: usize) -> Option<(usize, u32)> {
        let mut best: Option<(usize, u32)> = None;
        for &stop in stops {
            if let Some(arrival) = self.arrival(stop, round)
                && best.is_none_or(|(_, time)| arrival.time < time)
            {
                best = Some((stop, arrival.time));
            }
        }
        best
    }

    /// The earliest arrival any stop improved to in `round`: no fresh
    /// boarding in the next round can leave before it (see
    /// [`raptor_search_restricted`]'s round loop).
    fn earliest_improvement(&self, round: usize) -> Option<u32> {
        self.arrivals
            .iter()
            .rev()
            .take_while(|arrival| arrival.round == round)
            .map(|arrival| arrival.time)
            .min()
    }
}

pub fn raptor_search(options: RaptorOptions<'_>) -> Vec<RaptorJourney> {
    raptor_search_with_overlay(options, None)
}

/// [`raptor_search`] over the base array with `overlay`'s trains replaced
/// -- see [`crate::overlay`].
pub fn raptor_search_with_overlay(
    options: RaptorOptions<'_>,
    overlay: Option<&crate::overlay::ConnectionOverlay>,
) -> Vec<RaptorJourney> {
    raptor_search_restricted(options, overlay, None)
}

/// [`raptor_search_with_overlay`] honouring `restrictions` -- see
/// [`crate::restrictions`].
#[expect(
    clippy::needless_pass_by_value,
    reason = "public planner API takes its options and overlay by value"
)]
pub fn raptor_search_restricted(
    options: RaptorOptions<'_>,
    overlay: Option<&crate::overlay::ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
) -> Vec<RaptorJourney> {
    let mut search = Search::new(
        options.interchange,
        restrictions,
        options.date,
        options.departure_min,
    );
    let mut origin: Vec<usize> = Vec::new();
    for tiploc in options.from_tiplocs {
        let stop = search.stop_id(normalize_tiploc(tiploc));
        search.stops[stop].origin = true;
        if !origin.contains(&stop) {
            origin.push(stop);
        }
    }
    let to: Vec<usize> = options
        .to_tiplocs
        .iter()
        .map(|tiploc| search.stop_id(normalize_tiploc(tiploc)))
        .collect();

    // Round 0: whatever a walk from the origin reaches by itself.
    for &stop in &origin {
        search.relax_fixed_links(0, stop, options.departure_min);
    }

    // Bounded by max_rounds, but also stops the moment a round improves
    // nothing at all -- without this, a large caller-supplied max_rounds
    // (see RaptorOptions::max_rounds's own doc comment) would force a full
    // sweep of `connections` for every remaining round even after the
    // network has been fully explored.
    //
    // Target pruning (DS-196): a later round only contributes to the Pareto
    // set by reaching the destination strictly before every earlier round
    // did, and nothing departing at or after that arrival can, so each
    // round's sweep stops there. On a day's timetable most of the sweep is
    // after the first arrival, so this is most of the rounds' cost.
    //
    // And a late start (DS-225): from round 2 on, a round's sweep starts at
    // the earliest arrival the round before improved. A boarding before it
    // can only be made from an arrival older than the previous round, which
    // the previous round already boarded from, so it could only repeat that
    // round's arrivals, never improve on them.
    let mut rounds = 0;
    for round in 1..=options.max_rounds as usize {
        let cutoff = search
            .best_of(&to, round - 1)
            .map_or(u32::MAX, |(_, time)| time);
        let start = if round == 1 {
            options.departure_min
        } else {
            match search.earliest_improvement(round - 1) {
                Some(time) => time.max(options.departure_min),
                None => break,
            }
        };
        let improved = search.run_round(
            round,
            crate::overlay::connections_from(options.connections, overlay, start)
                .take_while(|connection| connection.departure_min < cutoff),
        );
        rounds = round;
        if !improved {
            break;
        }
    }

    build_pareto_set(&search, rounds, &origin, &to)
}

/// Walks each round's best destination arrival in order (round 1 upward)
/// and keeps only the ones that STRICTLY beat every earlier round -- a
/// round that ties or loses to an earlier round is dominated by it (same
/// or slower arrival, no fewer changes), and contributes nothing.
fn build_pareto_set(
    search: &Search<'_>,
    rounds: usize,
    origin: &[usize],
    to: &[usize],
) -> Vec<RaptorJourney> {
    let mut results = Vec::new();
    let mut running_best = u32::MAX;

    for k in 1..=rounds {
        let Some((best_stop, best_arrival)) = search.best_of(to, k) else {
            continue;
        };
        if best_arrival >= running_best {
            continue;
        }

        let legs = reconstruct_legs(search, best_stop, k, origin);
        let Some(first_leg) = legs.first() else {
            continue;
        };
        // Only committed once the candidate is confirmed to produce real
        // legs -- updating this any earlier would let a candidate that
        // gets skipped just below (empty legs) still consume this round's
        // "strictly better than running_best" slot, for no result.
        running_best = best_arrival;
        let departure_min = match first_leg {
            JourneyLeg::Train(leg) => leg.departure_min,
            JourneyLeg::Transfer(leg) => leg.departure_min,
        };
        results.push(RaptorJourney {
            changes: train_leg_count(&legs).saturating_sub(1),
            departure_min,
            arrival_min: best_arrival,
            legs,
        });
    }

    results
}

/// Walks backward from the stop that reached the destination in round `k`
/// to the origin -- a train step also steps back one round (that
/// boarding's readiness was computed from the round before it was ridden
/// in); a link step stays within the same round (a walk is never a
/// change).
#[expect(
    clippy::cast_sign_loss,
    reason = "fixed-link minutes are small and strictly positive (see relax)"
)]
fn reconstruct_legs(
    search: &Search<'_>,
    end: usize,
    start_round: usize,
    origin: &[usize],
) -> Vec<JourneyLeg> {
    let mut legs = Vec::new();
    let mut stop = end;
    let mut round = start_round;

    while !origin.contains(&stop) {
        let arrival = search.arrival(stop, round).unwrap_or_else(|| {
            panic!(
                "internal error: stop {} reached in round {round} but has no recorded arrival",
                search.stops[stop].tiploc
            )
        });
        match arrival.via {
            ArrivalSource::Link {
                from,
                mode,
                minutes,
            } => {
                legs.push(JourneyLeg::Transfer(TransferLeg {
                    mode: mode.to_string(),
                    from_tiploc: search.stops[from].tiploc.to_string(),
                    to_tiploc: search.stops[stop].tiploc.to_string(),
                    departure_min: arrival.time - minutes as u32,
                    arrival_min: arrival.time,
                    minutes,
                }));
                stop = from;
            }
            ArrivalSource::Train(connection, ride) => {
                let ride = &search.rides[ride];
                let boarded = ride.boarded;
                legs.push(JourneyLeg::Train(TrainLeg {
                    uid: boarded.uid.to_string(),
                    from_tiploc: boarded.from_tiploc.to_string(),
                    to_tiploc: connection.to_tiploc.to_string(),
                    departure_min: boarded.departure_min,
                    arrival_min: connection.arrival_min,
                    working_departure_min: boarded.working_departure_min,
                    working_arrival_min: connection.working_arrival_min,
                }));
                stop = ride.from;
                // The boarding read the arrivals of the round before the
                // one it was made in.
                round = ride.round - 1;
            }
        }
    }

    legs.reverse();
    legs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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

    fn empty_interchange() -> InterchangeData {
        InterchangeData {
            modal_change: schedule_query::ModalChangeBuffer::default(),
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        }
    }

    fn date() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 9, 23).unwrap()
    }

    #[test]
    fn a_direct_train_journey_has_zero_changes() {
        let connections = vec![conn("U1", "EUSTON", "MKC", 480, 530)];
        let interchange = empty_interchange();
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MKC".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].changes, 0);
    }

    #[test]
    fn a_fixed_link_only_journey_also_has_zero_changes() {
        // Matches csa.rs's own "Euston -> King's Cross by tube" case --
        // zero TRAIN legs boarded, so zero changes, same as a direct
        // no-change train journey, per this plan's own Review Focus.
        let connections: Vec<Connection> = Vec::new();
        let mut interchange = empty_interchange();
        interchange
            .tiploc_to_crs
            .insert("EUSTON".to_string(), "EUS".to_string());
        interchange
            .crs_to_tiplocs
            .insert("KGX".to_string(), vec!["KINGX".to_string()]);
        interchange.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![schedule_query::FixedLink {
                only_without: schedule_query::OperatorId::UNKNOWN,
                operator: schedule_query::OperatorId::UNKNOWN,
                mode: "TUBE".to_string(),
                to_crs: "KGX".to_string(),
                minutes: 5,
                valid_from: "0000".to_string(),
                valid_to: "2359".to_string(),
                days_mask: "1111111".to_string(),
            }],
        );
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["KINGX".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].changes, 0,
            "a fixed-link-only journey has nothing to change BETWEEN"
        );
    }

    #[test]
    fn a_fixed_link_followed_by_one_train_leg_also_has_zero_changes() {
        // Discriminates the naive `legs.len().saturating_sub(1)` formula
        // from the correct `train_leg_count(&legs).saturating_sub(1)`:
        // this journey has TWO legs (one TransferLeg, one TrainLeg), so the
        // naive formula gives 1, while the correct formula -- exactly one
        // train boarded, so nothing to change BETWEEN -- gives 0.
        // `a_fixed_link_only_journey_also_has_zero_changes` alone cannot
        // catch this: with only one leg total, both formulas agree by
        // coincidence.
        let connections = vec![conn("U1", "KINGX", "YORK", 495, 540)];
        let mut interchange = empty_interchange();
        interchange
            .tiploc_to_crs
            .insert("EUSTON".to_string(), "EUS".to_string());
        interchange
            .crs_to_tiplocs
            .insert("KGX".to_string(), vec!["KINGX".to_string()]);
        interchange.fixed_links_from_crs.insert(
            "EUS".to_string(),
            vec![schedule_query::FixedLink {
                only_without: schedule_query::OperatorId::UNKNOWN,
                operator: schedule_query::OperatorId::UNKNOWN,
                mode: "TUBE".to_string(),
                to_crs: "KGX".to_string(),
                minutes: 5,
                valid_from: "0000".to_string(),
                valid_to: "2359".to_string(),
                days_mask: "1111111".to_string(),
            }],
        );
        // KINGX carries no change_time_by_tiploc entry, so it falls back to
        // the default 5-minute change time -- arriving 485 (480 + 5-minute
        // tube), ready to board a fresh train at 490. U1 departs at 495.
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["YORK".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(results.len(), 1);
        assert_eq!(
            results[0].legs.len(),
            2,
            "one TransferLeg (EUSTON -> KINGX) then one TrainLeg (KINGX -> YORK)"
        );
        assert_eq!(
            results[0].changes, 0,
            "exactly one train boarded -- nothing to change between, same as a direct \
             no-change train journey"
        );
    }

    #[test]
    fn a_slower_option_with_one_fewer_change_appears_in_the_pareto_set() {
        // Direct, slow: 480 -> 620 (0 changes). Faster with a change:
        // 480 -> 530 -> 560 (1 change, arrives earlier). Both are
        // Pareto-optimal: neither dominates the other.
        let connections = vec![
            conn("DIRECT", "EUSTON", "MAN", 480, 620),
            conn("LEG1", "EUSTON", "MKC", 480, 530),
            conn("LEG2", "MKC", "MAN", 535, 560),
        ];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 5);
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(
            results.len(),
            2,
            "both the 0-change and 1-change options are Pareto-optimal"
        );
        assert_eq!(results[0].changes, 0);
        assert_eq!(results[0].arrival_min, 620);
        assert_eq!(results[1].changes, 1);
        assert_eq!(results[1].arrival_min, 560);
    }

    #[test]
    fn a_round_that_only_ties_an_earlier_rounds_arrival_is_dominated_and_excluded() {
        // Two 1-change options both arrive at 560; only the FIRST (fewer
        // rounds needed) should appear -- a tie never displaces an earlier,
        // already-Pareto-optimal entry.
        let connections = vec![
            conn("LEG1", "EUSTON", "MKC", 480, 530),
            conn("LEG2", "MKC", "MAN", 535, 560),
            conn("LEG3", "MKC", "MAN", 550, 560),
        ];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 5);
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        let arrivals_at_560 = results.iter().filter(|r| r.arrival_min == 560).count();
        assert_eq!(
            arrivals_at_560, 1,
            "a tied later round must not add a second, dominated entry"
        );
    }

    #[test]
    fn a_round_past_reachability_depth_reports_no_improvement() {
        // Directly exercises `Search::run_round` -- the piece
        // `raptor_search`'s early-termination `break` actually relies on.
        // Asserting only on `raptor_search`'s overall RESULT shape can't
        // discriminate this: `build_pareto_set`'s own strict-improvement
        // filter already discards a fixpoint round's duplicate entries
        // whether or not the loop stopped early, so a naive "does the
        // Pareto set still look right" test passes even with the break
        // deleted entirely. This test instead pins the loop's actual
        // reason to exist: bounding the search's cost against a large
        // caller-supplied max_rounds (see `RaptorOptions::max_rounds`'s own
        // doc comment).
        let connections = vec![conn("U1", "EUSTON", "MKC", 480, 530)];
        let interchange = empty_interchange();
        let mut search = Search::new(&interchange, None, date(), 480);
        let origin = search.stop_id("EUSTON");
        search.stops[origin].origin = true;

        let improved1 = search.run_round(1, &connections);
        assert!(improved1, "round 1 should board U1 and reach MKC");
        let mkc = search.stop_id("MKC");
        assert_eq!(search.arrival(mkc, 1).map(|a| a.time), Some(530));

        // Round 2 has nothing left to board or improve: MKC was already
        // reached in round 1, so re-scanning the same connections array
        // against round 1's frozen arrivals cannot produce a strictly
        // better arrival anywhere.
        let improved2 = search.run_round(2, &connections);
        assert!(
            !improved2,
            "a round that cannot improve on the previous round's arrivals must report no \
             improvement -- this is exactly what lets raptor_search's round loop stop early \
             rather than run all the way to max_rounds"
        );
    }

    #[test]
    fn no_reachable_destination_returns_an_empty_pareto_set() {
        let connections = vec![conn("U1", "EUSTON", "MKC", 480, 530)];
        let interchange = empty_interchange();
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["EDINBURGH".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert!(results.is_empty());
    }

    #[test]
    fn a_journey_requiring_two_changes_is_found() {
        // A 3-train-leg chain: EUSTON -> A -> B -> C, each boarding a
        // DIFFERENT uid, each requiring the default 5-minute change time.
        // Nothing in this file's other tests reaches round 3 or a
        // `changes` value above 1 -- this is the deepest case RAPTOR's own
        // unit tests exercise.
        let connections = vec![
            conn("U1", "EUSTON", "A", 480, 500),
            // A ready at 500 + 5 = 505; U2 departs 510.
            conn("U2", "A", "B", 510, 520),
            // B ready at 520 + 5 = 525; U3 departs 530.
            conn("U3", "B", "C", 530, 540),
        ];
        let interchange = empty_interchange();
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["C".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].legs.len(), 3, "three train legs, one per uid");
        assert_eq!(
            results[0].changes, 2,
            "three trains boarded means two changes between them"
        );
        assert_eq!(results[0].arrival_min, 540);
    }

    #[test]
    fn a_same_crs_sibling_change_enables_an_otherwise_impossible_boarding() {
        // Mirrors csa.rs's own
        // `a_same_crs_sibling_change_enables_an_otherwise_impossible_boarding`
        // test, adapted to RaptorOptions/raptor_search: WDON and WIMBLDN
        // share CRS WIM, so a passenger arriving at WDON can board a train
        // departing WIMBLDN, charged WIMBLDN's own minimum change time.
        // `sibling_tiplocs` returns nothing in any of this file's other
        // tests, so this is the only coverage of that path in raptor.rs.
        let connections = vec![
            conn("U1", "EUSTON", "WDON", 480, 500),
            conn("U2", "WIMBLDN", "SURBITON", 506, 520),
        ];
        let mut interchange = empty_interchange();
        interchange
            .tiploc_to_crs
            .insert("WDON".to_string(), "WIM".to_string());
        interchange
            .tiploc_to_crs
            .insert("WIMBLDN".to_string(), "WIM".to_string());
        interchange.crs_to_tiplocs.insert(
            "WIM".to_string(),
            vec!["WDON".to_string(), "WIMBLDN".to_string()],
        );
        interchange
            .change_time_by_tiploc
            .insert("WIMBLDN".to_string(), 5);

        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["SURBITON".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(results.len(), 1, "the sibling-enabled journey exists");
        let journey = &results[0];
        assert_eq!(journey.legs.len(), 2);
        // Same discontinuity documented on `RaptorJourney::legs`: leg 0
        // arrives at WDON, leg 1 departs from WIMBLDN.
        match &journey.legs[0] {
            JourneyLeg::Train(leg) => assert_eq!(leg.to_tiploc, "WDON"),
            JourneyLeg::Transfer(_) => panic!("expected leg 0 to be a train leg"),
        }
        match &journey.legs[1] {
            JourneyLeg::Train(leg) => assert_eq!(leg.from_tiploc, "WIMBLDN"),
            JourneyLeg::Transfer(_) => panic!("expected leg 1 to be a train leg"),
        }
        assert_eq!(journey.arrival_min, 520);
    }

    #[test]
    fn a_no_interchange_sentinel_blocks_a_fresh_boarding_by_a_different_uid() {
        // Mirrors csa.rs's own
        // `a_no_interchange_sentinel_blocks_a_fresh_boarding_by_a_different_uid`
        // test, adapted to RaptorOptions/raptor_search. MKC is a
        // NoInterchange sentinel station (raw value 99); a fresh boarding
        // there by a different uid must be blocked regardless of how much
        // time is available, while a genuinely different route (via
        // RUGBY, a normal interchange) still succeeds.
        let connections = vec![
            conn("U1", "EUSTON", "MKC", 480, 500),
            conn("U3", "EUSTON", "RUGBY", 481, 510),
            // A different working from MKC -- a fresh boarding, which the
            // NoInterchange sentinel must block.
            conn("U2", "MKC", "MAN", 505, 560),
            // RUGBY has no change_time_by_tiploc entry and so falls back
            // to the default 5-minute change time -- a normal, allowed
            // interchange.
            conn("U4", "RUGBY", "MAN", 520, 600),
        ];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 99);
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        let best = results
            .iter()
            .map(|j| j.arrival_min)
            .min()
            .expect("a journey exists via RUGBY");
        assert_eq!(
            best, 600,
            "the NoInterchange-blocked fresh boarding at MKC (U1 -> U2, arriving 560) must be \
             rejected; only the RUGBY route (arriving 600) is reachable"
        );
    }

    #[test]
    fn max_rounds_below_the_journeys_required_depth_returns_no_route_at_all() {
        // Turns RaptorOptions::max_rounds's own doc-comment warning (an
        // insufficient round cap silently returns NO route at all, not a
        // worse one) into an executable regression guard. A 4-train-leg
        // chain, each requiring one more round than the last (see
        // `a_journey_requiring_two_changes_is_found` for the same
        // per-round-per-leg pattern with one fewer leg) -- the destination
        // is first reached in round 4.
        let connections = vec![
            conn("U1", "EUSTON", "A", 480, 500),
            conn("U2", "A", "B", 510, 520),
            conn("U3", "B", "C", 530, 540),
            // C ready at 540 + 5 = 545; U4 departs 550.
            conn("U4", "C", "D", 550, 560),
        ];
        let interchange = empty_interchange();
        let below = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["D".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 3,
        });
        assert!(
            below.is_empty(),
            "max_rounds: 3 cannot reach D, which needs round 4 -- and must report this as NO \
             route at all, not a worse one"
        );
    }

    #[test]
    fn max_rounds_at_the_journeys_required_depth_finds_the_route() {
        let connections = vec![
            conn("U1", "EUSTON", "A", 480, 500),
            conn("U2", "A", "B", 510, 520),
            conn("U3", "B", "C", 530, 540),
            conn("U4", "C", "D", 550, 560),
        ];
        let interchange = empty_interchange();
        let at_depth = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["D".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(
            at_depth.len(),
            1,
            "max_rounds: 4 is exactly enough to reach D in round 4"
        );
        assert_eq!(at_depth[0].arrival_min, 560);
    }

    #[test]
    fn a_padded_tiploc_in_connections_and_interchange_still_matches_bare_raptor_options() {
        // Mirrors csa.rs's own
        // `a_padded_tiploc_in_connections_and_interchange_still_matches_bare_scan_options`
        // regression test for Finding 2 of the whole-branch review, adapted
        // to raptor_search. Covers the `normalize_tiploc` calls this file
        // makes (in `ready_source_at`, `relax_in_round`,
        // `relax_fixed_links_in_round`, and `build_pareto_set`), which had
        // no dedicated padded-TIPLOC test until now.
        let connections = vec![conn("U1", "EUSTON ", "MKC", 480, 530)];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 5);
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            // Bare form here -- deliberately mismatched padding from the
            // Connection's own "EUSTON " above.
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MKC".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(
            results.len(),
            1,
            "the journey is still found despite the padding mismatch"
        );
        assert_eq!(results[0].arrival_min, 530);
        assert_eq!(results[0].legs.len(), 1);
    }
}
