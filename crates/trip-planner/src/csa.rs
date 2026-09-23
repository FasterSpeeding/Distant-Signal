//! Connection Scan: the earliest-arrival journey from any of a set of
//! origin TIPLOCs to any of a set of destination TIPLOCs, departing no
//! earlier than a given time, over one already-built connections array —
//! a faithful Rust port of the real, working, tested implementation in
//! the sibling `Distant-Signal-MCP` project's `src/timetable/plan/csa.ts`
//! (re-cloned and re-read directly for this plan's own research pass).
//! See this plan's own header for why the TypeScript closures-over-state
//! shape becomes a struct with `&mut self` methods here (Judgment Call 2),
//! and why the sibling's own per-lookup memoization caches are not ported
//! (Judgment Call 3).

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use schedule_query::{
    ChangeTime, Connection, InterchangeData, fixed_links_from, minimum_change_time,
    normalize_tiploc, sibling_tiplocs,
};

/// One merged leg of a [`Journey`]: every consecutive [`Connection`] sharing
/// a `uid` collapsed into a single ride, exactly as a passenger who never
/// got off would describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainLeg {
    pub uid: String,
    pub from_tiploc: String,
    pub to_tiploc: String,
    pub departure_min: u32,
    pub arrival_min: u32,
}

/// One fixed-link hop of a [`Journey`] -- a walk, tube, bus or ferry ride
/// with no train involved. Distinct from [`TrainLeg`] so a consumer
/// rendering this can say "take the Underground, 5 minutes" rather than
/// fabricate a train identity for a leg with none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferLeg {
    pub mode: String,
    pub from_tiploc: String,
    pub to_tiploc: String,
    pub departure_min: u32,
    pub arrival_min: u32,
    pub minutes: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JourneyLeg {
    Train(TrainLeg),
    Transfer(TransferLeg),
}

impl JourneyLeg {
    fn departure_min(&self) -> u32 {
        match self {
            JourneyLeg::Train(leg) => leg.departure_min,
            JourneyLeg::Transfer(leg) => leg.departure_min,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Journey {
    /// Consecutive legs are normally contiguous -- one leg's `to_tiploc`
    /// equals the next leg's `from_tiploc` -- EXCEPT across a same-CRS
    /// sibling change (see `Scan::ready_source_at`'s own doc comment):
    /// there, the walk between two platform groups of the same physical
    /// station (e.g. `WDON` -> `WIMBLDN`) is charged in time against the
    /// next leg's minimum change time, but is never materialized as its
    /// own [`TransferLeg`]. A consumer rendering these legs (e.g. by
    /// converting each one independently) must not assume strict
    /// `to_tiploc == from_tiploc` contiguity between consecutive legs.
    pub legs: Vec<JourneyLeg>,
    pub departure_min: u32,
    /// May exceed 1440 for an overnight itinerary -- see
    /// `schedule_query::Connection::arrival_min`'s own doc comment.
    pub arrival_min: u32,
}

pub struct ScanOptions<'a> {
    /// Sorted by `departure_min` ascending -- `build_connections`'s own
    /// contract. `scan_connections` relies on this order; it does not
    /// re-sort.
    pub connections: &'a [Connection],
    pub interchange: &'a InterchangeData,
    pub from_tiplocs: &'a [String],
    pub to_tiplocs: &'a [String],
    /// Minutes from midnight the passenger is ready to depart.
    pub departure_min: u32,
    /// The same date `connections` was built for -- what `fixed_links_from`
    /// resolves its day-of-week mask against.
    pub date: NaiveDate,
}

/// How a stop's earliest-arrival entry was produced.
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

/// All of `scanConnections`'s TypeScript closures' captured mutable state,
/// as struct fields -- see this plan's Judgment Call 2.
struct Scan<'a> {
    interchange: &'a InterchangeData,
    date: NaiveDate,
    origin: HashSet<String>,
    destinations: HashSet<String>,
    departure_min: u32,

    earliest_arrival: HashMap<String, u32>,
    arrived_via: HashMap<String, ArrivalSource>,
    reachable_trip: HashSet<String>,
    leg_boarded_at: HashMap<String, Connection>,
    boarded_from: HashMap<String, String>,

    best_dest_arrival: u32,
    best_dest_tiploc: Option<String>,
}

impl<'a> Scan<'a> {
    /// The time a NEW boarding becomes possible at `tiploc`. The origin
    /// needs no interchange time. Every other stop folds in
    /// `minimum_change_time`, which can be `ChangeTime::NoInterchange` at
    /// the nine real coach-stand sentinel stations -- correctly forbidding
    /// a fresh board there without a special case (see
    /// `schedule_query::ChangeTime::allows`'s own doc). Also checks every
    /// CRS-sibling TIPLOC's own earlier arrival (a different platform group
    /// of the same physical station), charged at `tiploc`'s OWN minimum
    /// change time -- the same figure a same-TIPLOC change already pays.
    /// `None` when no candidate (neither `tiploc` itself nor any sibling)
    /// has any recorded arrival, or `tiploc`'s own change time is
    /// `NoInterchange` regardless of arrival -- direct translation of
    /// `csa.ts`'s own `Infinity` return, just as `Option::None` instead of
    /// a float sentinel.
    fn ready_source_at(&self, tiploc: &str) -> Option<ReadySource> {
        // Normalize defensively at this module's own boundary, matching
        // `schedule_query::interchange`'s own "callers should still
        // normalize, but correctness must not depend on their remembering
        // to" defense -- `tiploc` here may be a raw, still-padded
        // `Connection::from_tiploc`, while `origin` and `earliest_arrival`
        // are keyed on the normalized form (see Finding 2 of the
        // whole-branch review).
        let tiploc = normalize_tiploc(tiploc);
        if self.origin.contains(tiploc) {
            return Some(ReadySource {
                time: self.departure_min,
                from: tiploc.to_string(),
            });
        }
        let ChangeTime::Finite(change_time) = minimum_change_time(self.interchange, tiploc) else {
            return None;
        };

        let mut best: Option<ReadySource> = None;
        if let Some(&arrival) = self.earliest_arrival.get(tiploc) {
            best = Some(ReadySource {
                time: arrival + change_time,
                from: tiploc.to_string(),
            });
        }
        for sibling in sibling_tiplocs(self.interchange, tiploc) {
            let Some(&sibling_arrival) = self.earliest_arrival.get(sibling) else {
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

    /// Improves `tiploc`'s earliest arrival to `arrival_min`, if it
    /// genuinely is one -- records how it was reached, checks whether a
    /// destination was just reached, and relaxes every fixed link
    /// reachable from this stop at this new, better time. A no-op when
    /// `arrival_min` does not improve on what's already known, which is
    /// what keeps the recursive call into `relax_fixed_links` safe (every
    /// link has strictly positive `minutes`, and there are finitely many
    /// CRS codes, so this always terminates -- direct translation of
    /// `csa.ts:306-318`).
    fn relax(&mut self, tiploc: &str, arrival_min: u32, via: ArrivalSource) {
        // Normalize before this becomes an `earliest_arrival`/`arrived_via`
        // key or a `destinations` containment check -- `tiploc` may come
        // straight off a `Connection::to_tiploc`, potentially still padded
        // (Finding 2 of the whole-branch review).
        let tiploc = normalize_tiploc(tiploc);
        let current_best = self
            .earliest_arrival
            .get(tiploc)
            .copied()
            .unwrap_or(u32::MAX);
        if arrival_min >= current_best {
            return;
        }
        self.earliest_arrival
            .insert(tiploc.to_string(), arrival_min);
        self.arrived_via.insert(tiploc.to_string(), via);
        if self.destinations.contains(tiploc) && arrival_min < self.best_dest_arrival {
            self.best_dest_arrival = arrival_min;
            self.best_dest_tiploc = Some(tiploc.to_string());
        }
        self.relax_fixed_links(tiploc, arrival_min);
    }

    /// Connection Scan WITH footpaths: after a stop's arrival improves,
    /// also relax every stop reachable from it by a fixed link, so the
    /// search recognises that two platforms a short walk apart are
    /// effectively the same station. Direct translation of `csa.ts:343-359`.
    fn relax_fixed_links(&mut self, from_tiploc: &str, at_min: u32) {
        // Normalize before the `tiploc_to_crs` lookup (keyed on the bare
        // form) and before this value is stored as an `ArrivalSource::Link`
        // endpoint that `reconstruct_legs` later walks back through and
        // compares against `origin` (Finding 2 of the whole-branch review).
        let from_tiploc = normalize_tiploc(from_tiploc);
        let Some(crs) = self.interchange.tiploc_to_crs.get(from_tiploc).cloned() else {
            return;
        };
        let links: Vec<_> = fixed_links_from(self.interchange, &crs, self.date, at_min)
            .into_iter()
            .cloned()
            .collect();
        for link in links {
            let candidate = at_min + link.minutes as u32;
            let Some(destination_tiplocs) =
                self.interchange.crs_to_tiplocs.get(&link.to_crs).cloned()
            else {
                continue;
            };
            for to_tiploc in destination_tiplocs {
                self.relax(
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
}

/// Connection Scan: the earliest-arrival journey from any of
/// `options.from_tiplocs` to any of `options.to_tiplocs`, departing no
/// earlier than `options.departure_min`. `None` when no connection reaches
/// the destination at all.
pub fn scan_connections(options: ScanOptions) -> Option<Journey> {
    // Normalized at this module's own boundary, same defense-in-depth
    // `schedule_query::interchange` already applies at its own boundary --
    // every other TIPLOC-keyed lookup and containment check in this file
    // (`ready_source_at`, `relax`, `relax_fixed_links`) normalizes before
    // comparing against these sets, so they must be normalized too, or a
    // padded caller-supplied TIPLOC would never match (Finding 2 of the
    // whole-branch review).
    let origin: HashSet<String> = options
        .from_tiplocs
        .iter()
        .map(|tiploc| normalize_tiploc(tiploc).to_string())
        .collect();
    let destinations: HashSet<String> = options
        .to_tiplocs
        .iter()
        .map(|tiploc| normalize_tiploc(tiploc).to_string())
        .collect();

    let mut scan = Scan {
        interchange: options.interchange,
        date: options.date,
        origin: origin.clone(),
        destinations,
        departure_min: options.departure_min,
        earliest_arrival: HashMap::new(),
        arrived_via: HashMap::new(),
        reachable_trip: HashSet::new(),
        leg_boarded_at: HashMap::new(),
        boarded_from: HashMap::new(),
        best_dest_arrival: u32::MAX,
        best_dest_tiploc: None,
    };

    // Seed fixed-link relaxation from every origin TIPLOC at departure_min
    // -- a passenger may need to walk before ever boarding a first train.
    // Deliberately does not call relax() for the origin TIPLOCs themselves
    // -- ready_source_at's own origin branch must stay authoritative for
    // them (csa.ts:361-370).
    for tiploc in &origin {
        scan.relax_fixed_links(tiploc, options.departure_min);
    }

    for connection in options.connections {
        // No later-departing connection can beat a destination arrival
        // already found (csa.ts:372-379).
        if connection.departure_min >= scan.best_dest_arrival {
            break;
        }

        let already_aboard = scan.reachable_trip.contains(&connection.uid);
        if !already_aboard {
            let Some(source) = scan.ready_source_at(&connection.from_tiploc) else {
                continue;
            };
            if source.time > connection.departure_min {
                continue;
            }
            scan.reachable_trip.insert(connection.uid.clone());
            scan.leg_boarded_at
                .insert(connection.uid.clone(), connection.clone());
            scan.boarded_from
                .insert(connection.uid.clone(), source.from);
        }

        scan.relax(
            &connection.to_tiploc,
            connection.arrival_min,
            ArrivalSource::Train(connection.clone()),
        );
    }

    let best_dest_tiploc = scan.best_dest_tiploc.clone()?;
    let legs = reconstruct_legs(&best_dest_tiploc, &scan);
    let first_leg = legs.first()?;

    Some(Journey {
        departure_min: first_leg.departure_min(),
        // Not the last leg's own arrival_min: a trailing fixed-link walk
        // can reach the destination AFTER the last train leg, which
        // best_dest_arrival already accounts for.
        arrival_min: scan.best_dest_arrival,
        legs,
    })
}

/// Walks backward from the stop that reached the destination to the
/// origin, one leg at a time -- a train leg read off `leg_boarded_at` in
/// one step (the merge already decided during the sweep), a fixed-link
/// step producing its own `TransferLeg`.
fn reconstruct_legs(end_tiploc: &str, scan: &Scan) -> Vec<JourneyLeg> {
    let mut legs = Vec::new();
    let mut stop = end_tiploc.to_string();

    while !scan.origin.contains(&stop) {
        let via = scan.arrived_via.get(&stop).unwrap_or_else(|| {
            panic!("internal error: stop {stop} was reached but has no recorded arrival source")
        });

        match via {
            ArrivalSource::Link {
                from_tiploc,
                mode,
                minutes,
            } => {
                let arrival_min = *scan
                    .earliest_arrival
                    .get(&stop)
                    .unwrap_or_else(|| panic!("internal error: stop {stop} reached by a fixed link has no recorded arrival time"));
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
                let boarded = scan.leg_boarded_at.get(&connection.uid).unwrap_or_else(|| {
                    panic!(
                        "internal error: uid {} was relaxed without ever being boarded",
                        connection.uid
                    )
                });
                legs.push(JourneyLeg::Train(TrainLeg {
                    uid: boarded.uid.clone(),
                    from_tiploc: boarded.from_tiploc.clone(),
                    to_tiploc: connection.to_tiploc.clone(),
                    departure_min: boarded.departure_min,
                    arrival_min: connection.arrival_min,
                }));
                let source = scan.boarded_from.get(&connection.uid).unwrap_or_else(|| {
                    panic!(
                        "internal error: uid {} was boarded without a recorded readiness source",
                        connection.uid
                    )
                });
                stop = source.clone();
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
    fn a_single_direct_connection_is_found() {
        let connections = vec![conn("U1", "EUSTON", "MKC", 480, 530)];
        let interchange = empty_interchange();
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MKC".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a direct journey exists");
        assert_eq!(journey.arrival_min, 530);
        assert_eq!(journey.legs.len(), 1);
    }

    #[test]
    fn no_connection_reaches_the_destination_returns_none() {
        let connections = vec![conn("U1", "EUSTON", "MKC", 480, 530)];
        let interchange = empty_interchange();
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["EDINBURGH".to_string()],
            departure_min: 480,
            date: date(),
        });
        assert!(journey.is_none());
    }

    #[test]
    fn a_change_within_minimum_change_time_is_offered_two_legs() {
        let connections = vec![
            conn("U1", "EUSTON", "MKC", 480, 530),
            conn("U2", "MKC", "MAN", 535, 600),
        ];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 5);
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a two-leg journey exists");
        assert_eq!(journey.legs.len(), 2);
        assert_eq!(journey.arrival_min, 600);
    }

    #[test]
    fn a_change_that_does_not_meet_minimum_change_time_is_rejected() {
        let connections = vec![
            conn("U1", "EUSTON", "MKC", 480, 530),
            // Only 2 minutes to change, but MKC needs 5.
            conn("U2", "MKC", "MAN", 532, 600),
            // A later, valid onward connection exists too, so a route
            // still exists overall -- just not via the too-tight change.
            conn("U3", "MKC", "MAN", 540, 610),
        ];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 5);
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a journey exists via the later, valid connection");
        assert_eq!(
            journey.arrival_min, 610,
            "the too-tight 532 change must be rejected"
        );
    }

    #[test]
    fn a_same_train_continuation_through_a_coach_stand_sentinel_is_unaffected() {
        // MKC is a NoInterchange sentinel station, but nobody changes there
        // -- the same working (uid U1) continues straight through it.
        let connections = vec![
            conn("U1", "EUSTON", "MKC", 480, 500),
            conn("U1", "MKC", "MAN", 500, 560),
        ];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 99);
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a through-train journey exists despite MKC's sentinel");
        assert_eq!(
            journey.legs.len(),
            1,
            "one merged leg -- the same uid throughout"
        );
        assert_eq!(journey.arrival_min, 560);
    }

    #[test]
    fn a_no_interchange_sentinel_blocks_a_fresh_boarding_by_a_different_uid() {
        // MKC is a NoInterchange sentinel station. Unlike
        // `a_same_train_continuation_through_a_coach_stand_sentinel_is_unaffected`,
        // this uses two DIFFERENT uids at MKC, so `already_aboard` is false
        // and `ready_source_at` (where the NoInterchange check lives) is
        // actually exercised for that boarding -- covering the "must only
        // ever block a FRESH boarding" half of the Review Focus claim that
        // the same-uid test cannot reach. A NoInterchange sentinel blocks
        // EVERY fresh boarding at that station (there is no gap large
        // enough to satisfy it, unlike a merely-too-tight Finite change
        // time), so -- unlike
        // `a_change_that_does_not_meet_minimum_change_time_is_rejected` --
        // no later departure from MKC itself can rescue the journey; only a
        // genuinely different route (via RUGBY, a normal interchange) can.
        let connections = vec![
            conn("U1", "EUSTON", "MKC", 480, 500),
            conn("U3", "EUSTON", "RUGBY", 481, 510),
            // A different working from MKC -- a fresh boarding, which the
            // NoInterchange sentinel must block regardless of how much time
            // is available.
            conn("U2", "MKC", "MAN", 505, 560),
            // RUGBY has no MSN record and so falls back to the default
            // 5-minute change time -- a normal, allowed interchange.
            conn("U4", "RUGBY", "MAN", 520, 600),
        ];
        let mut interchange = empty_interchange();
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 99);
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a journey exists via RUGBY");
        assert_eq!(
            journey.arrival_min, 600,
            "the NoInterchange-blocked fresh boarding at MKC (U1 -> U2, \
             arriving 560) must be rejected; only the RUGBY route (arriving \
             600) is reachable"
        );
    }

    #[test]
    fn a_same_crs_sibling_change_enables_an_otherwise_impossible_boarding() {
        // WDON and WIMBLDN share CRS WIM; a passenger arriving at WDON can
        // board a train departing WIMBLDN, charged WIMBLDN's own minimum
        // change time.
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

        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["SURBITON".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("the sibling-enabled journey exists");
        assert_eq!(journey.legs.len(), 2);
        // The discontinuity documented on `Journey::legs`: leg 0 arrives at
        // WDON, leg 1 departs from WIMBLDN -- the sibling walk is charged
        // in time (via ready_source_at) but never materialized as its own
        // leg. This is expected, faithful-to-the-port behaviour, asserted
        // here explicitly so a future reader does not mistake it for a bug.
        match &journey.legs[0] {
            JourneyLeg::Train(leg) => assert_eq!(leg.to_tiploc, "WDON"),
            JourneyLeg::Transfer(_) => panic!("expected leg 0 to be a train leg"),
        }
        match &journey.legs[1] {
            JourneyLeg::Train(leg) => assert_eq!(leg.from_tiploc, "WIMBLDN"),
            JourneyLeg::Transfer(_) => panic!("expected leg 1 to be a train leg"),
        }
        assert_eq!(
            journey.legs[0].departure_min(),
            480,
            "sanity check that leg 0 is indeed the first leg"
        );
        assert_eq!(journey.arrival_min, 520);
    }

    #[test]
    fn a_padded_tiploc_in_connections_and_interchange_still_matches_bare_scan_options() {
        // Regression test for Finding 2 of the whole-branch review: a
        // Connection array built from a ScheduleIndex-driven fixture path
        // can carry still-padded TIPLOCs (schedule_query::records's own
        // CallingPoint::tiploc doc comment: "exactly as decoded, still
        // padded"), while ScanOptions's own from_tiplocs/to_tiplocs may be
        // bare. Every TIPLOC-keyed lookup and containment check in this
        // module must normalize so this still resolves correctly.
        let connections = vec![conn("U1", "EUSTON ", "MKC", 480, 530)];
        let mut interchange = empty_interchange();
        // The interchange data is also seeded with the padded form, mirroring
        // a real still-padded MSN/schedule-body TIPLOC.
        interchange
            .change_time_by_tiploc
            .insert("MKC".to_string(), 5);
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            // Bare form here -- deliberately mismatched padding from the
            // Connection's own "EUSTON " above.
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MKC".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("the journey is still found despite the padding mismatch");
        assert_eq!(journey.arrival_min, 530);
        assert_eq!(journey.legs.len(), 1);
    }

    #[test]
    fn a_journey_reached_purely_by_a_fixed_link_with_no_train_at_all_is_valid() {
        // Euston -> King's Cross by tube, no train ride at all.
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

        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["KINGX".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a fixed-link-only journey is valid");
        assert_eq!(journey.legs.len(), 1);
        match &journey.legs[0] {
            JourneyLeg::Transfer(leg) => assert_eq!(leg.mode, "TUBE"),
            JourneyLeg::Train(_) => panic!("expected a transfer leg, got a train leg"),
        }
    }

    #[test]
    fn origin_and_destination_being_the_same_tiploc_returns_none_not_a_panic() {
        let connections = vec![conn("U1", "EUSTON", "MKC", 480, 530)];
        let interchange = empty_interchange();
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["EUSTON".to_string()],
            departure_min: 480,
            date: date(),
        });
        // The origin is never itself written into earliest_arrival/
        // arrived_via (ready_source_at's own origin branch stays
        // authoritative for it, matching csa.ts's own comment on this),
        // so relax() is never called for it and best_dest_tiploc stays
        // None -- no journey is reported "to itself".
        assert!(journey.is_none());
    }

    #[test]
    fn an_overnight_connection_with_arrival_past_1440_is_handled_correctly() {
        let connections = vec![conn("F1", "LIVST", "BARKING", 23 * 60 + 48, 1440 + 6)];
        let interchange = empty_interchange();
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["LIVST".to_string()],
            to_tiplocs: &["BARKING".to_string()],
            departure_min: 23 * 60 + 48,
            date: date(),
        })
        .expect("the overnight journey exists");
        assert_eq!(journey.arrival_min, 1440 + 6);
    }
}
