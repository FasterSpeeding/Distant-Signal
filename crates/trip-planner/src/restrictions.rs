//! Per-request station restrictions (`/Trips/plan`'s `avoid`, `avoidStop`
//! and `avoidChange`), honoured INSIDE every search rather than by filtering
//! finished journeys.
//!
//! Filtering afterwards would find the fastest journey, discover it breaks a
//! restriction, and report nothing -- when a compliant, merely slower one
//! exists. The idea, and the three-way split of what "avoid" can mean, come
//! from Skye's `train-mcp` (`src/timetable/plan/constraints.ts`,
//! `pruneConnections`); see
//! docs/superpowers/specs/2026-09-29-trips-plan-arrive-by-avoid-design.md.
//!
//! Three restrictions, all keyed on normalized TIPLOCs (the caller expands a
//! CRS code to every TIPLOC it covers):
//!
//! - [`Restrictions::no_interchange`]: the traveller never starts or ends a
//!   ride there -- no fresh boarding, no alighting, no walking in or out.
//!   Staying aboard a train that calls there is fine. (`avoidChange`, and
//!   implied by the other two.)
//! - [`Restrictions::no_call`]: no train ridden may call there -- every
//!   connection starting or ending there is unusable. (`avoidStop`, and
//!   implied by `avoid`.)
//! - [`Restrictions::pass_legs`]: no train ridden may even run through
//!   there without stopping. Connections don't carry the untimed pass rows
//!   between their two calls, so the caller supplies, for each train that
//!   passes an avoided station, its base connections in order with the
//!   blocked ones marked. (`avoid`.)
//!
//! A blocked connection must also END the ride on that train: both searches
//! decide "already aboard" by UID alone, so merely skipping the connection
//! would let a passenger who boarded before the avoided station carry on
//! after it as if nothing happened. `train-mcp` hits the same problem and
//! relabels each surviving run of a schedule with a synthetic id; here the
//! searches instead drop the UID from their "aboard" set when they meet a
//! blocked connection, which reports the real UID on every leg unchanged.

use std::collections::{HashMap, HashSet};

use schedule_query::{Connection, normalize_tiploc};

/// One base connection of a train that passes an avoided station, in the
/// train's own order. `blocked` when its span includes an avoided TIPLOC
/// the train runs through without calling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassLeg {
    pub from_tiploc: String,
    pub to_tiploc: String,
    pub blocked: bool,
}

/// See the module doc.
#[derive(Debug, Clone, Default)]
pub struct Restrictions {
    no_interchange: HashSet<String>,
    no_call: HashSet<String>,
    pass_legs: HashMap<String, Vec<PassLeg>>,
}

impl Restrictions {
    /// `no_interchange` and `no_call` are TIPLOCs (normalized here);
    /// `no_call` is added to `no_interchange` too. `pass_legs` is keyed on
    /// train UID, legs normalized here.
    pub fn new(
        no_interchange: impl IntoIterator<Item = String>,
        no_call: impl IntoIterator<Item = String>,
        pass_legs: HashMap<String, Vec<PassLeg>>,
    ) -> Self {
        let no_call: HashSet<String> = no_call
            .into_iter()
            .map(|t| normalize_tiploc(&t).to_string())
            .collect();
        let mut no_interchange: HashSet<String> = no_interchange
            .into_iter()
            .map(|t| normalize_tiploc(&t).to_string())
            .collect();
        no_interchange.extend(no_call.iter().cloned());
        let pass_legs = pass_legs
            .into_iter()
            .map(|(uid, legs)| {
                let legs = legs
                    .into_iter()
                    .map(|leg| PassLeg {
                        from_tiploc: normalize_tiploc(&leg.from_tiploc).to_string(),
                        to_tiploc: normalize_tiploc(&leg.to_tiploc).to_string(),
                        blocked: leg.blocked,
                    })
                    .collect();
                (uid, legs)
            })
            .collect();
        Self {
            no_interchange,
            no_call,
            pass_legs,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.no_interchange.is_empty() && self.no_call.is_empty() && self.pass_legs.is_empty()
    }

    /// Whether a ride may start or end at `tiploc` (or a walk touch it).
    pub fn allows_interchange(&self, tiploc: &str) -> bool {
        !self.no_interchange.contains(normalize_tiploc(tiploc))
    }

    /// Whether `connection` is unusable: it calls at a `no_call` TIPLOC, or
    /// its train runs through an avoided one between its two calls.
    ///
    /// For a train in `pass_legs`, a connection that is not one of its base
    /// legs (a live-overlay replacement joining two calls because the one
    /// between was cancelled) is blocked when any base leg it spans is, and
    /// also -- conservatively -- when it cannot be placed on the train's base
    /// order at all.
    pub fn blocks(&self, connection: &Connection) -> bool {
        let from = normalize_tiploc(&connection.from_tiploc);
        let to = normalize_tiploc(&connection.to_tiploc);
        if self.no_call.contains(from) || self.no_call.contains(to) {
            return true;
        }
        let Some(legs) = self.pass_legs.get(&connection.uid) else {
            return false;
        };
        let Some(start) = legs.iter().position(|leg| leg.from_tiploc == from) else {
            return true;
        };
        for leg in &legs[start..] {
            if leg.blocked {
                return true;
            }
            if leg.to_tiploc == to {
                return false;
            }
        }
        true
    }
}

/// `Restrictions::allows_interchange`, treating "no restrictions" as allowed.
pub(crate) fn allows_interchange(restrictions: Option<&Restrictions>, tiploc: &str) -> bool {
    restrictions.is_none_or(|r| r.allows_interchange(tiploc))
}

/// `Restrictions::blocks`, treating "no restrictions" as not blocked.
pub(crate) fn blocks(restrictions: Option<&Restrictions>, connection: &Connection) -> bool {
    restrictions.is_some_and(|r| r.blocks(connection))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(uid: &str, from: &str, to: &str) -> Connection {
        Connection {
            uid: uid.to_string(),
            from_tiploc: from.to_string(),
            to_tiploc: to.to_string(),
            departure_min: 0,
            arrival_min: 1,
            working_departure_min: 0,
            working_arrival_min: 1,
            can_board: true,
            can_alight: true,
        }
    }

    fn leg(from: &str, to: &str, blocked: bool) -> PassLeg {
        PassLeg {
            from_tiploc: from.to_string(),
            to_tiploc: to.to_string(),
            blocked,
        }
    }

    #[test]
    fn no_call_blocks_both_ends_and_implies_no_interchange() {
        let r = Restrictions::new([], ["BHAMNWS ".to_string()], HashMap::new());
        assert!(r.blocks(&conn("U", "BHAMNWS", "COV")));
        assert!(r.blocks(&conn("U", "WVH", "BHAMNWS ")));
        assert!(!r.blocks(&conn("U", "WVH", "COV")));
        assert!(!r.allows_interchange("BHAMNWS"));
        assert!(r.allows_interchange("COV"));
    }

    #[test]
    fn no_interchange_alone_blocks_no_connection() {
        let r = Restrictions::new(["CLPHMJC".to_string()], [], HashMap::new());
        assert!(!r.blocks(&conn("U", "CLPHMJC", "WATRLMN")));
        assert!(!r.allows_interchange("CLPHMJC"));
    }

    #[test]
    fn pass_legs_block_the_spanning_connection_and_any_replacement_over_it() {
        // U runs A -> B -> C -> D, passing the avoided station between B and C.
        let r = Restrictions::new(
            [],
            [],
            HashMap::from([(
                "U".to_string(),
                vec![
                    leg("A", "B", false),
                    leg("B", "C", true),
                    leg("C", "D", false),
                ],
            )]),
        );
        assert!(!r.blocks(&conn("U", "A", "B")));
        assert!(r.blocks(&conn("U", "B", "C")));
        assert!(!r.blocks(&conn("U", "C", "D")));
        // A replacement A -> C (B cancelled) spans the blocked leg.
        assert!(r.blocks(&conn("U", "A", "C")));
        // A replacement B -> D (C cancelled) spans it too.
        assert!(r.blocks(&conn("U", "B", "D")));
        // Cannot be placed at all: conservatively blocked.
        assert!(r.blocks(&conn("U", "X", "D")));
        // Other trains are untouched.
        assert!(!r.blocks(&conn("V", "B", "C")));
    }

    fn timed(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
        Connection {
            departure_min: dep,
            arrival_min: arr,
            working_departure_min: dep,
            working_arrival_min: arr,
            can_board: true,
            can_alight: true,
            ..conn(uid, from, to)
        }
    }

    /// Both forward searches, for one restriction set: the train UIDs of the
    /// CSA journey and of RAPTOR's earliest-arriving one.
    fn both(connections: &[Connection], restrictions: &Restrictions) -> (Vec<String>, Vec<String>) {
        use crate::{JourneyLeg, RaptorOptions, ScanOptions};
        let interchange = schedule_query::InterchangeData {
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        let (from, to) = (vec!["A".to_string()], vec!["D".to_string()]);
        let train_uids = |legs: &[JourneyLeg]| -> Vec<String> {
            legs.iter()
                .filter_map(|leg| match leg {
                    JourneyLeg::Train(t) => Some(t.uid.clone()),
                    JourneyLeg::Transfer(_) => None,
                })
                .collect()
        };
        let csa = crate::scan_connections_restricted(
            ScanOptions {
                connections,
                interchange: &interchange,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 0,
                date,
            },
            None,
            Some(restrictions),
        )
        .map(|j| train_uids(&j.legs))
        .unwrap_or_default();
        let raptor = crate::raptor_search_restricted(
            RaptorOptions {
                connections,
                interchange: &interchange,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 0,
                date,
                max_rounds: 4,
            },
            None,
            Some(restrictions),
        )
        .last()
        .map(|j| train_uids(&j.legs))
        .unwrap_or_default();
        (csa, raptor)
    }

    /// `T` runs A -> Y -> B -> Z -> D; `S` is a slower A -> C -> D
    /// alternative. Blocking any middle connection of `T` leaves later ones
    /// that a search still "aboard T" would wrongly ride.
    fn network() -> Vec<Connection> {
        let mut connections = vec![
            timed("T", "A", "Y", 100, 105),
            timed("T", "Y", "B", 106, 110),
            timed("T", "B", "Z", 111, 115),
            timed("T", "Z", "D", 116, 130),
            timed("S", "A", "C", 100, 120),
            timed("S", "C", "D", 121, 160),
        ];
        connections.sort_by_key(|c| (c.departure_min, c.uid.clone(), c.from_tiploc.clone()));
        connections
    }

    #[test]
    fn the_forward_searches_ride_through_a_no_interchange_station() {
        let r = Restrictions::new(["B".to_string()], [], HashMap::new());
        let (csa, raptor) = both(&network(), &r);
        assert_eq!(csa, vec!["T"]);
        assert_eq!(raptor, vec!["T"]);
    }

    #[test]
    fn the_forward_searches_never_carry_a_ride_across_a_blocked_connection() {
        // No call at B: T's A -> B and B -> D are both unusable, and being
        // "aboard T" must not survive them.
        let r = Restrictions::new([], ["B".to_string()], HashMap::new());
        let (csa, raptor) = both(&network(), &r);
        assert_eq!(csa, vec!["S"]);
        assert_eq!(raptor, vec!["S"]);

        // T passes X between Y and B: that connection is blocked, so a
        // passenger who boarded T at A may not stay aboard past Y.
        let r = Restrictions::new(
            ["X".to_string()],
            [],
            HashMap::from([(
                "T".to_string(),
                vec![
                    leg("A", "Y", false),
                    leg("Y", "B", true),
                    leg("B", "Z", false),
                    leg("Z", "D", false),
                ],
            )]),
        );
        let (csa, raptor) = both(&network(), &r);
        assert_eq!(csa, vec!["S"]);
        assert_eq!(raptor, vec!["S"]);
    }

    /// Regression: a train left at a blocked connection and boarded again
    /// further on must not rewrite the first ride. Here T is ridden A -> X,
    /// blocked X -> Y, and boarded again at Y (reached on S); the journey to
    /// E uses the FIRST ride (A -> X) then R. Looking the ride up by UID
    /// afterwards used to find the second boarding and report "Y -> X".
    #[test]
    fn a_train_boarded_again_after_a_block_keeps_its_first_ride() {
        use crate::{JourneyLeg, RaptorOptions, ScanOptions};
        let mut connections = vec![
            timed("T", "A", "X", 100, 110),
            timed("T", "X", "Y", 111, 120),
            timed("T", "Y", "Z", 121, 130),
            timed("S", "A", "Y", 100, 110),
            timed("R", "X", "E", 115, 200),
        ];
        connections.sort_by_key(|c| (c.departure_min, c.uid.clone(), c.from_tiploc.clone()));
        let r = Restrictions::new(
            ["P".to_string()],
            [],
            HashMap::from([(
                "T".to_string(),
                vec![
                    leg("A", "X", false),
                    leg("X", "Y", true),
                    leg("Y", "Z", false),
                ],
            )]),
        );
        let interchange = schedule_query::InterchangeData {
            change_time_by_tiploc: HashMap::new(),
            tiploc_to_crs: HashMap::new(),
            crs_to_tiplocs: HashMap::new(),
            fixed_links_from_crs: HashMap::new(),
        };
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 29).unwrap();
        let (from, to) = (vec!["A".to_string()], vec!["E".to_string()]);
        let shape = |legs: &[JourneyLeg]| -> Vec<(String, String, String)> {
            legs.iter()
                .filter_map(|leg| match leg {
                    JourneyLeg::Train(t) => {
                        Some((t.uid.clone(), t.from_tiploc.clone(), t.to_tiploc.clone()))
                    }
                    JourneyLeg::Transfer(_) => None,
                })
                .collect()
        };
        let expected = vec![
            ("T".to_string(), "A".to_string(), "X".to_string()),
            ("R".to_string(), "X".to_string(), "E".to_string()),
        ];
        let csa = crate::scan_connections_restricted(
            ScanOptions {
                connections: &connections,
                interchange: &interchange,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 0,
                date,
            },
            None,
            Some(&r),
        )
        .expect("T then R");
        assert_eq!(shape(&csa.legs), expected);
        let raptor = crate::raptor_search_restricted(
            RaptorOptions {
                connections: &connections,
                interchange: &interchange,
                from_tiplocs: &from,
                to_tiplocs: &to,
                departure_min: 0,
                date,
                max_rounds: 4,
            },
            None,
            Some(&r),
        );
        assert_eq!(shape(&raptor.last().expect("T then R").legs), expected);
    }
}
