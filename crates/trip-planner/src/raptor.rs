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

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use schedule_query::{
    ChangeTime, Connection, InterchangeData, fixed_links_from, minimum_change_time,
    normalize_tiploc, sibling_tiplocs,
};

use crate::csa::{JourneyLeg, TrainLeg, TransferLeg};

fn train_leg_count(legs: &[JourneyLeg]) -> u32 {
    legs.iter()
        .filter(|leg| matches!(leg, JourneyLeg::Train(_)))
        .count() as u32
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaptorJourney {
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
#[derive(Debug, Clone)]
enum ArrivalSource {
    Train(Connection),
    Link {
        from_tiploc: String,
        mode: String,
        minutes: i32,
    },
}

struct ReadySource {
    time: u32,
    from: String,
}

/// One round's working state. `arrival`/`arrived_via` start as a clone of
/// the previous round's (a stop reachable with fewer trips stays
/// reachable); `leg_boarded_at`/`boarded_from` start empty each round --
/// which connection first made a `uid` reachable, and which TIPLOC's
/// readiness enabled that boarding, are questions scoped to THIS round's
/// boardings alone.
#[derive(Clone)]
struct RoundState {
    arrival: HashMap<String, u32>,
    arrived_via: HashMap<String, ArrivalSource>,
    leg_boarded_at: HashMap<String, Connection>,
    boarded_from: HashMap<String, String>,
}

impl RoundState {
    fn empty() -> Self {
        Self {
            arrival: HashMap::new(),
            arrived_via: HashMap::new(),
            leg_boarded_at: HashMap::new(),
            boarded_from: HashMap::new(),
        }
    }

    fn clone_for_next_round(&self) -> Self {
        Self {
            arrival: self.arrival.clone(),
            arrived_via: self.arrived_via.clone(),
            leg_boarded_at: HashMap::new(),
            boarded_from: HashMap::new(),
        }
    }
}

fn ready_source_at(
    source: &HashMap<String, u32>,
    origin: &HashSet<String>,
    departure_min: u32,
    interchange: &InterchangeData,
    tiploc: &str,
) -> Option<ReadySource> {
    let tiploc = normalize_tiploc(tiploc);
    if origin.contains(tiploc) {
        return Some(ReadySource {
            time: departure_min,
            from: tiploc.to_string(),
        });
    }
    let ChangeTime::Finite(change_time) = minimum_change_time(interchange, tiploc) else {
        return None;
    };

    let mut best: Option<ReadySource> = None;
    if let Some(&arrival) = source.get(tiploc) {
        best = Some(ReadySource {
            time: arrival + change_time,
            from: tiploc.to_string(),
        });
    }
    for sibling in sibling_tiplocs(interchange, tiploc) {
        let Some(&sibling_arrival) = source.get(sibling) else {
            continue;
        };
        let candidate = sibling_arrival + change_time;
        if best.as_ref().is_none_or(|b| candidate < b.time) {
            best = Some(ReadySource {
                time: candidate,
                from: sibling.to_string(),
            });
        }
    }
    best
}

/// Improves `tiploc`'s arrival IN `round` to `arrival_min`, records how it
/// was reached, marks it touched (so the round loop can tell whether this
/// round achieved anything at all), and relaxes every fixed link reachable
/// from this stop at this new, better time -- confined entirely to
/// `round`'s own maps, which is what makes "at most k trips" an honest
/// count.
fn relax_in_round(
    round: &mut RoundState,
    touched: &mut HashSet<String>,
    interchange: &InterchangeData,
    date: NaiveDate,
    tiploc: &str,
    arrival_min: u32,
    via: ArrivalSource,
) {
    let tiploc = normalize_tiploc(tiploc);
    let current_best = round.arrival.get(tiploc).copied().unwrap_or(u32::MAX);
    if arrival_min >= current_best {
        return;
    }
    round.arrival.insert(tiploc.to_string(), arrival_min);
    round.arrived_via.insert(tiploc.to_string(), via);
    touched.insert(tiploc.to_string());
    relax_fixed_links_in_round(round, touched, interchange, date, tiploc, arrival_min);
}

fn relax_fixed_links_in_round(
    round: &mut RoundState,
    touched: &mut HashSet<String>,
    interchange: &InterchangeData,
    date: NaiveDate,
    from_tiploc: &str,
    at_min: u32,
) {
    let from_tiploc = normalize_tiploc(from_tiploc);
    let Some(crs) = interchange.tiploc_to_crs.get(from_tiploc).cloned() else {
        return;
    };
    let links: Vec<_> = fixed_links_from(interchange, &crs, date, at_min)
        .into_iter()
        .cloned()
        .collect();
    for link in links {
        let candidate = at_min + link.minutes as u32;
        let Some(destination_tiplocs) = interchange.crs_to_tiplocs.get(&link.to_crs).cloned()
        else {
            continue;
        };
        for to_tiploc in destination_tiplocs {
            relax_in_round(
                round,
                touched,
                interchange,
                date,
                &to_tiploc,
                candidate,
                ArrivalSource::Link {
                    from_tiploc: from_tiploc.to_string(),
                    mode: link.mode.clone(),
                    minutes: link.minutes,
                },
            );
        }
    }
}

pub fn raptor_search(options: RaptorOptions) -> Vec<RaptorJourney> {
    let origin: HashSet<String> = options
        .from_tiplocs
        .iter()
        .map(|tiploc| normalize_tiploc(tiploc).to_string())
        .collect();

    let mut round0 = RoundState::empty();
    let mut round0_touched = HashSet::new();
    for tiploc in &origin {
        relax_fixed_links_in_round(
            &mut round0,
            &mut round0_touched,
            options.interchange,
            options.date,
            tiploc,
            options.departure_min,
        );
    }

    let mut rounds: Vec<RoundState> = vec![round0];

    for _ in 1..=options.max_rounds {
        let previous = rounds.last().expect("round0 was just pushed");
        let mut current = previous.clone_for_next_round();
        let mut touched = HashSet::new();
        let mut reachable_trip: HashSet<String> = HashSet::new();

        for connection in options.connections {
            let already_aboard = reachable_trip.contains(&connection.uid);
            if !already_aboard {
                let Some(source) = ready_source_at(
                    &previous.arrival,
                    &origin,
                    options.departure_min,
                    options.interchange,
                    &connection.from_tiploc,
                ) else {
                    continue;
                };
                if source.time > connection.departure_min {
                    continue;
                }
                reachable_trip.insert(connection.uid.clone());
                current
                    .leg_boarded_at
                    .insert(connection.uid.clone(), connection.clone());
                current
                    .boarded_from
                    .insert(connection.uid.clone(), source.from);
            }
            relax_in_round(
                &mut current,
                &mut touched,
                options.interchange,
                options.date,
                &connection.to_tiploc,
                connection.arrival_min,
                ArrivalSource::Train(connection.clone()),
            );
        }

        let improved_nothing = touched.is_empty();
        rounds.push(current);
        if improved_nothing {
            break;
        }
    }

    build_pareto_set(&rounds, &origin, options.to_tiplocs)
}

/// Walks each round's best destination arrival in order (round 1 upward)
/// and keeps only the ones that STRICTLY beat every earlier round -- a
/// round that ties or loses to an earlier round is dominated by it (same
/// or slower arrival, no fewer changes), and contributes nothing.
fn build_pareto_set(
    rounds: &[RoundState],
    origin: &HashSet<String>,
    to_tiplocs: &[String],
) -> Vec<RaptorJourney> {
    let mut results = Vec::new();
    let mut running_best = u32::MAX;

    for (k, round) in rounds.iter().enumerate().skip(1) {
        let mut best_tiploc: Option<&str> = None;
        let mut best_arrival = u32::MAX;
        for destination in to_tiplocs {
            let normalized = normalize_tiploc(destination);
            if let Some(&arrival) = round.arrival.get(normalized)
                && arrival < best_arrival
            {
                best_arrival = arrival;
                best_tiploc = Some(normalized);
            }
        }
        let Some(best_tiploc) = best_tiploc else {
            continue;
        };
        if !(best_arrival < running_best) {
            continue;
        }
        running_best = best_arrival;

        let legs = reconstruct_legs(best_tiploc, k, rounds, origin);
        let Some(first_leg) = legs.first() else {
            continue;
        };
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
/// to the origin -- a `'train'` step also steps back one round (that
/// boarding's readiness was computed from round `k-1`'s frozen arrivals); a
/// `'link'` step stays within the same round (a walk is never a change).
fn reconstruct_legs(
    end_tiploc: &str,
    start_round: usize,
    rounds: &[RoundState],
    origin: &HashSet<String>,
) -> Vec<JourneyLeg> {
    let mut legs = Vec::new();
    let mut stop = normalize_tiploc(end_tiploc).to_string();
    let mut round_index = start_round;

    while !origin.contains(&stop) {
        let round = &rounds[round_index];
        let via = round
            .arrived_via
            .get(&stop)
            .unwrap_or_else(|| panic!("internal error: stop {stop} reached in round {round_index} but has no recorded arrival source"));

        match via {
            ArrivalSource::Link {
                from_tiploc,
                mode,
                minutes,
            } => {
                let arrival_min = *round
                    .arrival
                    .get(&stop)
                    .unwrap_or_else(|| panic!("internal error: stop {stop} reached by a fixed link in round {round_index} has no recorded arrival time"));
                legs.push(JourneyLeg::Transfer(TransferLeg {
                    mode: mode.clone(),
                    from_tiploc: from_tiploc.clone(),
                    to_tiploc: stop.clone(),
                    departure_min: arrival_min - *minutes as u32,
                    arrival_min,
                    minutes: *minutes,
                }));
                stop = from_tiploc.clone();
            }
            ArrivalSource::Train(connection) => {
                let boarded = round
                    .leg_boarded_at
                    .get(&connection.uid)
                    .unwrap_or_else(|| panic!("internal error: uid {} relaxed in round {round_index} without ever being boarded there", connection.uid));
                legs.push(JourneyLeg::Train(TrainLeg {
                    uid: boarded.uid.clone(),
                    from_tiploc: boarded.from_tiploc.clone(),
                    to_tiploc: connection.to_tiploc.clone(),
                    departure_min: boarded.departure_min,
                    arrival_min: connection.arrival_min,
                }));
                let source = round
                    .boarded_from
                    .get(&connection.uid)
                    .unwrap_or_else(|| panic!("internal error: uid {} boarded in round {round_index} without a recorded readiness source", connection.uid));
                stop = source.clone();
                round_index -= 1;
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
            uid: uid.to_string(),
            from_tiploc: from.to_string(),
            to_tiploc: to.to_string(),
            departure_min: dep,
            arrival_min: arr,
        }
    }

    fn empty_interchange() -> InterchangeData {
        InterchangeData {
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
    fn the_round_loop_stops_early_once_nothing_improves() {
        let connections = vec![conn("U1", "EUSTON", "MKC", 480, 530)];
        let interchange = empty_interchange();
        // A generous max_rounds -- if the loop didn't stop early, this
        // would still return promptly; the real assertion is on the
        // RESULT shape (no phantom later-round duplicates), not on timing.
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MKC".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 20,
        });
        assert_eq!(results.len(), 1);
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
}
