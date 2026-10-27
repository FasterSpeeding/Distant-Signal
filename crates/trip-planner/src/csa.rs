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
//!
//! Fewest changes on a tie (2026-10-26). Of the journeys arriving
//! earliest, the one with the fewest trains is returned, then the one
//! leaving latest, then the one walking least. A single earliest arrival
//! per stop cannot find it: the fewest-changes journey to the destination
//! may reach some stop on the way later than another route does, and
//! still make the same onward train. So each stop keeps a small Pareto
//! set of labels over (arrival, arrival plus the bus or ferry buffer,
//! trains ridden), as multi-criteria Connection Scan does, and a train
//! already boarded is boarded again further along when that takes fewer
//! trains. Departure and walking only break ties between otherwise equal
//! labels, greedily, so they are a preference, not a guarantee. The
//! earliest arrival itself is never later than the single-label scan's:
//! the label that one kept is never dominated. Labels arriving after the
//! best destination arrival found so far are dropped, as the plain scan
//! stops at it, which keeps the sets small.
//!
//! The arrive-by search's forward half ([`Goal::FewestChangesBy`]) ranks
//! the other way round: of the journeys arriving by a deadline, fewest
//! changes first, then the earliest arrival (see [`crate::reverse`]).

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};

use chrono::NaiveDate;
use schedule_query::{
    ChangeTime, Connection, InterchangeData, fixed_links_from, minimum_change_time,
    normalize_tiploc, sibling_tiplocs,
};

use crate::restrictions::{self, Restrictions};

/// One merged leg of a [`Journey`]: every consecutive [`Connection`] sharing
/// a `uid` collapsed into a single ride, exactly as a passenger who never
/// got off would describe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainLeg {
    pub uid: String,
    pub from_tiploc: String,
    pub to_tiploc: String,
    /// Public times (see `schedule_query::Connection::departure_min`).
    pub departure_min: u32,
    pub arrival_min: u32,
    /// The same two times on the working timetable
    /// (`schedule_query::Connection::working_departure_min`).
    pub working_departure_min: u32,
    pub working_arrival_min: u32,
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

/// Which of the journeys reaching the destination a scan returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Goal {
    /// Depart-after: the earliest arrival; among those, the fewest changes,
    /// then the latest departure, then the least walking.
    EarliestArrival,
    /// The forward half of arrive-by, run from the latest departure: of the
    /// journeys arriving by this deadline, the fewest changes, then the
    /// earliest arrival, then the latest departure, then the least walking.
    FewestChangesBy(u32),
}

/// How a label was produced.
#[derive(Debug, Clone, Copy)]
enum ArrivalSource<'a> {
    /// The connection that arrived, and the ride it was part of (an index
    /// into `Scan::rides`). The ride is recorded here, not looked up by UID
    /// afterwards: with restrictions a train can be left (a blocked
    /// connection) and boarded again further on, a train can be boarded
    /// again for fewer changes, and an arrival made on the first ride must
    /// still reconstruct from the first boarding.
    Train(&'a Connection, usize),
    /// A fixed link from `from_tiploc`, started from the label `from`
    /// (`None`: from the origin itself, at the requested departure time).
    Link {
        from: Option<usize>,
        from_tiploc: &'a str,
        mode: &'a str,
        minutes: i32,
    },
}

/// One way of standing at a stop: an entry in that stop's Pareto set.
/// Labels live in `Scan::labels` and are never moved or changed, apart from
/// `live`, so rides and later labels can point back at them by index.
#[derive(Debug, Clone)]
struct Label<'a> {
    /// Normalized.
    tiploc: &'a str,
    /// Arrival.
    time: u32,
    /// When a fresh boarding or a walk on may start, before the next
    /// stop's own change time: `time` plus the bus or ferry buffer owed
    /// for getting off (see [`alighting_buffer`]).
    ready: u32,
    /// Trains ridden so far.
    trains: u32,
    /// The journey's departure (its first leg's).
    departure: u32,
    /// Fixed-link minutes so far.
    walk: u32,
    via: ArrivalSource<'a>,
    /// Still in its stop's Pareto set. A dominated label is dropped from
    /// the set but kept here, for whatever already points at it.
    live: bool,
}

impl Label<'_> {
    /// Ties between labels equal on every Pareto criterion: the later
    /// departure, then the shorter walk.
    fn tie_key(&self) -> (Reverse<u32>, u32) {
        (Reverse(self.departure), self.walk)
    }

    /// At least as good as `other` on (arrival, ready, trains), and on the
    /// tie key too if equal on all three: `other` adds nothing.
    fn covers(&self, other: &Label<'_>) -> bool {
        if self.time > other.time || self.ready > other.ready || self.trains > other.trains {
            return false;
        }
        (self.time, self.ready, self.trains) != (other.time, other.ready, other.trains)
            || self.tie_key() <= other.tie_key()
    }

    fn changes(&self) -> u32 {
        self.trains.saturating_sub(1)
    }
}

/// One ride on a train: where it was boarded, and the journey so far.
struct Ride<'a> {
    boarded: &'a Connection,
    /// The label whose arrival made the boarding possible (`None`: the
    /// origin).
    from: Option<usize>,
    /// Trains ridden, this one included.
    trains: u32,
    departure: u32,
    walk: u32,
}

/// A possible fresh boarding, as [`Ride`]'s fields.
struct ReadySource {
    from: Option<usize>,
    trains: u32,
    departure: u32,
    walk: u32,
}

/// Lower is better: fewer trains, then a later departure, then less walking.
type RideKey = (u32, Reverse<u32>, u32);

fn ride_key(trains: u32, departure: u32, walk: u32) -> RideKey {
    (trains, Reverse(departure), walk)
}

/// `FxHash`, as rustc uses: far cheaper than the default `SipHash` for the
/// short TIPLOC and UID keys of a scan's own per-query maps, which are
/// looked up for nearly every connection. Not resistant to collision
/// attacks, which is fine: the keys are timetable identifiers, not request
/// input.
#[derive(Default, Clone, Copy)]
struct FxHasher(u64);

impl FxHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl std::hash::Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.add(u64::from_le_bytes(*word));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, byte: u8) {
        self.add(u64::from(byte));
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type FxMap<K, V> = HashMap<K, V, std::hash::BuildHasherDefault<FxHasher>>;

/// What a scan knows about one stop: its labels, and everything a boarding
/// there needs, looked up from the interchange data once per scan rather
/// than once per connection.
struct Stop<'a> {
    /// Its live labels: a Pareto set, usually of one or two.
    bag: Vec<usize>,
    origin: bool,
    destination: bool,
    /// Not avoided (`avoidChange` and stricter): may be alighted at or
    /// walked into, and boarded at fresh.
    allows_interchange: bool,
    /// What a fresh boarding here costs before any bus or ferry buffer:
    /// `minimum_change_time`, `None` for a `NoInterchange` sentinel (the
    /// nine real coach-stand stations, see `schedule_query::ChangeTime`'s
    /// own doc) or an avoided station.
    change_time: Option<u32>,
    /// Every other TIPLOC sharing its CRS (`schedule_query::sibling_tiplocs`).
    siblings: Vec<&'a str>,
}

/// Where fixed links are expanded from: a label, or the origin.
struct Walker<'a> {
    from: Option<usize>,
    /// Normalized.
    tiploc: &'a str,
    at_min: u32,
    trains: u32,
    departure: u32,
    walk: u32,
}

type LinkQueue = BinaryHeap<Reverse<(u32, usize)>>;

/// All of `scanConnections`'s TypeScript closures' captured mutable state,
/// as struct fields -- see this plan's Judgment Call 2.
struct Scan<'a> {
    interchange: &'a InterchangeData,
    restrictions: Option<&'a Restrictions>,
    date: NaiveDate,
    goal: Goal,
    origin: HashSet<&'a str>,
    destinations: HashSet<&'a str>,
    departure_min: u32,

    /// Every label ever recorded (see [`Label`]).
    labels: Vec<Label<'a>>,
    /// Normalized TIPLOC -> index into `stops`.
    stop_ids: FxMap<&'a str, usize>,
    stops: Vec<Stop<'a>>,
    /// UID -> the ride (index into `rides`) the traveller is on.
    reachable_trip: FxMap<&'a str, usize>,
    rides: Vec<Ride<'a>>,
    /// The best label at a destination so far, by `goal`.
    best: Option<usize>,
}

impl<'a> Scan<'a> {
    /// No label arriving later than this can lead to a better answer: the
    /// best destination arrival so far (the plain scan's stopping rule), or
    /// the arrive-by deadline.
    fn bound(&self) -> u32 {
        match self.goal {
            Goal::EarliestArrival => self.best.map_or(u32::MAX, |best| self.labels[best].time),
            Goal::FewestChangesBy(deadline) => deadline,
        }
    }

    /// `tiploc`'s (normalized) index into `stops`, added on first sight.
    fn stop_id(&mut self, tiploc: &'a str) -> usize {
        if let Some(&id) = self.stop_ids.get(tiploc) {
            return id;
        }
        let allows_interchange = restrictions::allows_interchange(self.restrictions, tiploc);
        let change_time = match minimum_change_time(self.interchange, tiploc) {
            ChangeTime::Finite(minutes) if allows_interchange => Some(minutes),
            _ => None,
        };
        self.stops.push(Stop {
            bag: Vec::new(),
            origin: self.origin.contains(tiploc),
            destination: self.destinations.contains(tiploc),
            allows_interchange,
            change_time,
            siblings: sibling_tiplocs(self.interchange, tiploc),
        });
        let id = self.stops.len() - 1;
        self.stop_ids.insert(tiploc, id);
        id
    }

    /// `a` is a better answer than `b`, by `goal`.
    fn better_answer(&self, a: &Label<'_>, b: &Label<'_>) -> bool {
        let rest = |l: &Label<'_>| (Reverse(l.departure), l.walk);
        match self.goal {
            Goal::EarliestArrival => {
                (a.time, a.changes(), rest(a)) < (b.time, b.changes(), rest(b))
            }
            Goal::FewestChangesBy(_) => {
                (a.changes(), a.time, rest(a)) < (b.changes(), b.time, rest(b))
            }
        }
    }

    /// The best fresh boarding of `connection` at its departure stop, if
    /// any: from the origin (no interchange time), or from a label there
    /// or at a CRS sibling (a different platform group of the same
    /// physical station) ready in time. A non-origin stop folds in
    /// `minimum_change_time` -- `ChangeTime::NoInterchange` at the nine
    /// real coach-stand sentinel stations, forbidding a fresh board there
    /// without a special case (see `schedule_query::ChangeTime::allows`'s
    /// own doc) -- charged at the boarding stop's own figure for a sibling
    /// too, plus the bus or ferry buffer on both sides of the change
    /// (`interchange.modal_change`, see `schedule_query::ModalChangeBuffer`):
    /// the boarding service's here, the arriving one's in the label's
    /// `ready`. Of the labels ready in time, the one with the fewest
    /// trains, then the latest departure, then the least walking.
    fn ready_source_at(&mut self, connection: &'a Connection) -> Option<ReadySource> {
        // Normalize defensively at this module's own boundary, matching
        // `schedule_query::interchange`'s own "callers should still
        // normalize, but correctness must not depend on their remembering
        // to" defense -- a raw `Connection::from_tiploc` may still be
        // padded, while `origin` and `bags` are keyed on the normalized form
        // (see Finding 2 of the whole-branch review).
        let id = self.stop_id(normalize_tiploc(&connection.from_tiploc));
        let stop = &self.stops[id];
        if stop.origin {
            return Some(ReadySource {
                from: None,
                trains: 1,
                departure: connection.departure_min,
                walk: 0,
            });
        }
        // An avoided station or a sentinel: no fresh boarding.
        let change_time =
            stop.change_time? + self.interchange.modal_change.extra_for(&connection.uid);
        let latest_ready = connection.departure_min.checked_sub(change_time)?;

        let mut best: Option<(usize, RideKey)> = None;
        let siblings = stop
            .siblings
            .iter()
            .filter_map(|sibling| self.stop_ids.get(sibling))
            .map(|&sibling| &self.stops[sibling]);
        for at in std::iter::once(stop).chain(siblings) {
            for &index in &at.bag {
                let label = &self.labels[index];
                if label.ready > latest_ready {
                    continue;
                }
                let key = ride_key(label.trains + 1, label.departure, label.walk);
                if best.is_none_or(|(_, known)| key < known) {
                    best = Some((index, key));
                }
            }
        }
        best.map(|(index, (trains, Reverse(departure), walk))| ReadySource {
            from: Some(index),
            trains,
            departure,
            walk,
        })
    }

    /// Starts a ride on `connection` from `source`.
    fn board(&mut self, connection: &'a Connection, source: &ReadySource) -> usize {
        self.rides.push(Ride {
            boarded: connection,
            from: source.from,
            trains: source.trains,
            departure: source.departure,
            walk: source.walk,
        });
        let ride = self.rides.len() - 1;
        self.reachable_trip.insert(connection.uid.as_str(), ride);
        ride
    }

    /// Records `label` if it is not dominated at its stop, then relaxes
    /// every fixed link from it (see [`Self::relax_fixed_links`]).
    fn relax(&mut self, label: Label<'a>) {
        if let Some(index) = self.record_arrival(label) {
            self.relax_fixed_links(index);
        }
    }

    /// The bookkeeping half of [`Self::relax`]: adds `label` to its stop's
    /// Pareto set unless a label there already covers it (dropping any it
    /// covers), and notes a better answer at a destination. Returns the new
    /// label's index, or `None` when it was not recorded.
    fn record_arrival(&mut self, label: Label<'a>) -> Option<usize> {
        if label.time > self.bound() {
            return None;
        }
        // Arrive-by: no better than an answer already found, and nothing
        // after this label can arrive earlier or take a change back.
        if matches!(self.goal, Goal::FewestChangesBy(_))
            && self.best.is_some_and(|best| {
                let best = &self.labels[best];
                label.changes() > best.changes()
                    || (label.changes() == best.changes() && label.time >= best.time)
            })
        {
            return None;
        }
        let id = self.stop_id(label.tiploc);
        // Never alight at (or walk into) an avoided station; a train that
        // calls there simply carries on.
        if !self.stops[id].allows_interchange {
            return None;
        }
        let labels = &mut self.labels;
        let bag = &mut self.stops[id].bag;
        if bag.iter().any(|&known| labels[known].covers(&label)) {
            return None;
        }
        bag.retain(|&known| {
            let dominated = label.covers(&labels[known]);
            if dominated {
                labels[known].live = false;
            }
            !dominated
        });
        let index = labels.len();
        bag.push(index);
        labels.push(label);
        let label = &self.labels[index];
        if self.stops[id].destination
            && self
                .best
                .is_none_or(|best| self.better_answer(label, &self.labels[best]))
        {
            self.best = Some(index);
        }
        Some(index)
    }

    /// Connection Scan WITH footpaths: after a label is recorded, also
    /// record every stop reachable from it by a fixed link, so the search
    /// recognises that two platforms a short walk apart are effectively the
    /// same station. Started from the label `start`, and on through further
    /// links from every label that records.
    ///
    /// Earliest-first (a min-heap on the time the next links start), so a
    /// label is expanded once. The depth-first recursion of
    /// `csa.ts:343-359` re-expanded a stop each time another route improved
    /// it, and with every fixed link usable both ways
    /// (`schedule_query::index_fixed_links_both_ways`) London's links are
    /// one dense component, where that multiplied the work.
    fn relax_fixed_links(&mut self, start: usize) {
        let mut queue = LinkQueue::new();
        self.expand_label(start, &mut queue);
        self.drain_links(&mut queue);
    }

    /// Expands the queued labels' fixed links, earliest first.
    fn drain_links(&mut self, queue: &mut LinkQueue) {
        while let Some(Reverse((_, index))) = queue.pop() {
            // Dominated by a label recorded since it was queued.
            if self.labels[index].live {
                self.expand_label(index, queue);
            }
        }
    }

    fn expand_label(&mut self, index: usize, queue: &mut LinkQueue) {
        let label = &self.labels[index];
        let from = Walker {
            from: Some(index),
            tiploc: label.tiploc,
            at_min: label.ready,
            trains: label.trains,
            departure: label.departure,
            walk: label.walk,
        };
        self.expand_fixed_links(&from, queue);
    }

    /// One step of [`Self::relax_fixed_links`]: records every stop one
    /// fixed link from `from`, and queues each label recorded to be
    /// expanded in turn.
    #[expect(
        clippy::cast_sign_loss,
        reason = "fixed-link minutes are small and strictly positive (see relax_in_round)"
    )]
    fn expand_fixed_links(&mut self, from: &Walker<'a>, queue: &mut LinkQueue) {
        // The interchange data outlives `self`'s borrow: read links and
        // TIPLOCs from it without copying while recording updates `self`.
        let interchange: &'a InterchangeData = self.interchange;
        let Some(crs) = interchange.tiploc_to_crs.get(from.tiploc) else {
            return;
        };
        for link in fixed_links_from(interchange, crs, self.date, from.at_min) {
            let arrival = from.at_min + link.minutes as u32;
            let Some(destination_tiplocs) = interchange.crs_to_tiplocs.get(&link.to_crs) else {
                continue;
            };
            for to_tiploc in destination_tiplocs {
                let label = Label {
                    tiploc: normalize_tiploc(to_tiploc),
                    time: arrival,
                    ready: arrival,
                    trains: from.trains,
                    departure: from.departure,
                    walk: from.walk + link.minutes as u32,
                    via: ArrivalSource::Link {
                        from: from.from,
                        from_tiploc: from.tiploc,
                        mode: &link.mode,
                        minutes: link.minutes,
                    },
                    live: true,
                };
                if let Some(index) = self.record_arrival(label) {
                    queue.push(Reverse((arrival, index)));
                }
            }
        }
    }
}

/// The bus or ferry buffer owed before walking on from a stop reached by
/// `connection` (`schedule_query::ModalChangeBuffer`): a walk to another
/// station or stop is a change off that bus or ferry, just as a fresh
/// boarding at the same stop is (`Scan::ready_source_at`), and the arrive-by
/// search charges it there too (`reverse`'s alighting side). Nothing after a
/// train or a walk. RAPTOR's `relax_in_round` does the same.
fn alighting_buffer(interchange: &InterchangeData, connection: &Connection) -> u32 {
    interchange.modal_change.extra_for(&connection.uid)
}

/// Connection Scan: the earliest-arrival journey from any of
/// `options.from_tiplocs` to any of `options.to_tiplocs`, departing no
/// earlier than `options.departure_min` -- of those, the one with the fewest
/// changes, then the latest departure, then the least walking (see the
/// module doc). `None` when no connection reaches the destination at all.
pub fn scan_connections(options: ScanOptions<'_>) -> Option<Journey> {
    scan_connections_with_overlay(options, None)
}

/// [`scan_connections`] over the base array with `overlay`'s trains
/// replaced -- see [`crate::overlay`].
pub fn scan_connections_with_overlay(
    options: ScanOptions<'_>,
    overlay: Option<&crate::overlay::ConnectionOverlay>,
) -> Option<Journey> {
    scan_connections_restricted(options, overlay, None)
}

/// [`scan_connections_with_overlay`] honouring `restrictions` -- see
/// [`crate::restrictions`].
#[expect(
    clippy::needless_pass_by_value,
    reason = "public planner API takes its options and overlay by value"
)]
pub fn scan_connections_restricted(
    options: ScanOptions<'_>,
    overlay: Option<&crate::overlay::ConnectionOverlay>,
    restrictions: Option<&Restrictions>,
) -> Option<Journey> {
    scan(&options, overlay, restrictions, Goal::EarliestArrival)
}

/// The scan behind [`scan_connections_restricted`], answering `goal`.
pub(crate) fn scan<'a>(
    options: &ScanOptions<'a>,
    overlay: Option<&'a crate::overlay::ConnectionOverlay>,
    restrictions: Option<&'a Restrictions>,
    goal: Goal,
) -> Option<Journey> {
    // Normalized at this module's own boundary, same defense-in-depth
    // `schedule_query::interchange` already applies at its own boundary --
    // every other TIPLOC-keyed lookup and containment check in this file
    // (`ready_source_at`, `relax`, `relax_fixed_links`) normalizes before
    // comparing against these sets, so they must be normalized too, or a
    // padded caller-supplied TIPLOC would never match (Finding 2 of the
    // whole-branch review).
    let origin: HashSet<&'a str> = options
        .from_tiplocs
        .iter()
        .map(|tiploc| normalize_tiploc(tiploc))
        .collect();
    let destinations: HashSet<&'a str> = options
        .to_tiplocs
        .iter()
        .map(|tiploc| normalize_tiploc(tiploc))
        .collect();

    let mut scan = Scan {
        interchange: options.interchange,
        restrictions,
        date: options.date,
        goal,
        origin: origin.clone(),
        destinations,
        departure_min: options.departure_min,
        labels: Vec::new(),
        stop_ids: FxMap::default(),
        stops: Vec::new(),
        reachable_trip: FxMap::default(),
        rides: Vec::new(),
        best: None,
    };

    // Seed fixed-link relaxation from every origin TIPLOC at departure_min
    // -- a passenger may need to walk before ever boarding a first train.
    // Deliberately records no label at the origin TIPLOCs themselves --
    // ready_source_at's own origin branch must stay authoritative for
    // them (csa.ts:361-370).
    for tiploc in origin {
        let mut queue = LinkQueue::new();
        let from = Walker {
            from: None,
            tiploc,
            at_min: scan.departure_min,
            trains: 0,
            departure: scan.departure_min,
            walk: 0,
        };
        scan.expand_fixed_links(&from, &mut queue);
        scan.drain_links(&mut queue);
    }

    for connection in
        crate::overlay::connections_from(options.connections, overlay, options.departure_min)
    {
        // No later-departing connection can reach the destination by the
        // best arrival already found (csa.ts:372-379), or by the deadline.
        // Strictly later: one departing at that minute may still tie it.
        if connection.departure_min > scan.bound() {
            break;
        }
        // A blocked connection is unusable, and ends any ride on its train
        // (see `crate::restrictions`'s module doc).
        if restrictions::blocks(restrictions, connection) {
            scan.reachable_trip.remove(connection.uid.as_str());
            continue;
        }

        let ride = match scan.reachable_trip.get(connection.uid.as_str()) {
            // Aboard already. Boarding again here pays only if it takes
            // fewer trains (or as few, leaving later): never for a ride
            // from the origin, nor where nobody may board.
            Some(&ride) if scan.rides[ride].trains <= 1 || !connection.can_board => ride,
            Some(&ride) => {
                let current = &scan.rides[ride];
                let current = ride_key(current.trains, current.departure, current.walk);
                match scan.ready_source_at(connection) {
                    Some(source)
                        if ride_key(source.trains, source.departure, source.walk) < current =>
                    {
                        scan.board(connection, &source)
                    }
                    _ => ride,
                }
            }
            None => {
                // A set-down-only stop: the train calls, but nobody may get
                // on here.
                if !connection.can_board {
                    continue;
                }
                let Some(source) = scan.ready_source_at(connection) else {
                    continue;
                };
                scan.board(connection, &source)
            }
        };

        // A pick-up-only stop: stay aboard, but never arrive here.
        if connection.can_alight {
            let current = &scan.rides[ride];
            let time = connection.arrival_min;
            let label = Label {
                // Normalize before this becomes a `bags` key or a
                // `destinations` containment check -- it may still be
                // padded (Finding 2 of the whole-branch review).
                tiploc: normalize_tiploc(&connection.to_tiploc),
                time,
                ready: time + alighting_buffer(scan.interchange, connection),
                trains: current.trains,
                departure: current.departure,
                walk: current.walk,
                via: ArrivalSource::Train(connection, ride),
                live: true,
            };
            scan.relax(label);
        }
    }

    let best = scan.best?;
    let legs = reconstruct_legs(best, &scan);
    let first_leg = legs.first()?;

    Some(Journey {
        departure_min: first_leg.departure_min(),
        // Not the last leg's own arrival_min: a trailing fixed-link walk
        // can reach the destination AFTER the last train leg.
        arrival_min: scan.labels[best].time,
        legs,
    })
}

/// Walks backward from the label that reached the destination to the
/// origin, one leg at a time -- a train leg read off its ride in one step
/// (the merge already decided during the sweep), a fixed-link step
/// producing its own `TransferLeg`.
#[expect(
    clippy::cast_sign_loss,
    reason = "fixed-link minutes are small and strictly positive (see relax_in_round)"
)]
fn reconstruct_legs(end: usize, scan: &Scan<'_>) -> Vec<JourneyLeg> {
    let mut legs = Vec::new();
    let mut at = Some(end);

    while let Some(index) = at {
        let label = &scan.labels[index];
        if scan.origin.contains(label.tiploc) {
            break;
        }
        match label.via {
            ArrivalSource::Link {
                from,
                from_tiploc,
                mode,
                minutes,
            } => {
                legs.push(JourneyLeg::Transfer(TransferLeg {
                    mode: mode.to_string(),
                    from_tiploc: from_tiploc.to_string(),
                    to_tiploc: label.tiploc.to_string(),
                    departure_min: label.time - minutes as u32,
                    arrival_min: label.time,
                    minutes,
                }));
                at = from;
            }
            ArrivalSource::Train(connection, ride) => {
                let ride = &scan.rides[ride];
                let boarded = ride.boarded;
                legs.push(JourneyLeg::Train(TrainLeg {
                    uid: boarded.uid.clone(),
                    from_tiploc: boarded.from_tiploc.clone(),
                    to_tiploc: connection.to_tiploc.clone(),
                    departure_min: boarded.departure_min,
                    arrival_min: connection.arrival_min,
                    working_departure_min: boarded.working_departure_min,
                    working_arrival_min: connection.working_arrival_min,
                }));
                at = ride.from;
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

    fn train_uids(journey: &Journey) -> Vec<&str> {
        journey
            .legs
            .iter()
            .filter_map(|leg| match leg {
                JourneyLeg::Train(t) => Some(t.uid.as_str()),
                JourneyLeg::Transfer(_) => None,
            })
            .collect()
    }

    fn o_to_z(connections: &[Connection], interchange: &InterchangeData) -> Journey {
        scan_connections(ScanOptions {
            connections,
            interchange,
            from_tiplocs: &["O".to_string()],
            to_tiplocs: &["Z".to_string()],
            departure_min: 470,
            date: date(),
        })
        .expect("a journey exists")
    }

    /// Two routes from O to Z arriving at 600, both on the final train F
    /// from M. The four-change one reaches M first (525, five trains); the
    /// two-change one later (532, two trains), still in time for F. A
    /// single earliest arrival per stop kept only the first, and rode F
    /// with four changes.
    fn two_and_four_change_network() -> Vec<Connection> {
        vec![
            conn("S", "O", "P", 480, 490),
            conn("T2", "P", "B", 495, 505),
            conn("U2", "P", "M", 500, 532),
            conn("T3", "B", "C", 510, 515),
            conn("T4", "C", "M", 520, 525),
            conn("F", "M", "Z", 540, 600),
        ]
    }

    #[test]
    fn of_two_journeys_arriving_together_the_one_with_fewer_changes_wins() {
        let journey = o_to_z(&two_and_four_change_network(), &empty_interchange());
        assert_eq!(journey.arrival_min, 600);
        assert_eq!(train_uids(&journey), ["S", "U2", "F"]);
    }

    #[test]
    fn a_train_is_boarded_again_further_on_when_that_takes_fewer_changes() {
        // The four-change route boards F at M1 at 540; the two-change one
        // reaches M2, F's next call, at 545, in time for F there at 550.
        let connections = vec![
            conn("S", "O", "P", 480, 490),
            conn("T2", "P", "B", 495, 505),
            conn("U2", "P", "M2", 500, 545),
            conn("T3", "B", "C", 510, 515),
            conn("T4", "C", "M1", 520, 525),
            conn("F", "M1", "M2", 540, 549),
            conn("F", "M2", "Z", 550, 600),
        ];
        let journey = o_to_z(&connections, &empty_interchange());
        assert_eq!(journey.arrival_min, 600);
        assert_eq!(train_uids(&journey), ["S", "U2", "F"]);
        match &journey.legs[2] {
            JourneyLeg::Train(leg) => assert_eq!(leg.from_tiploc, "M2"),
            JourneyLeg::Transfer(_) => panic!("expected F as a train leg"),
        }
    }

    #[test]
    fn fewer_changes_never_cost_an_earlier_arrival() {
        // A three-change route a minute faster than both: it wins.
        let mut connections = two_and_four_change_network();
        connections.push(conn("G", "C", "Z", 520, 599));
        connections.sort_by_key(|c| c.departure_min);
        let journey = o_to_z(&connections, &empty_interchange());
        assert_eq!(journey.arrival_min, 599);
        assert_eq!(train_uids(&journey), ["S", "T2", "T3", "G"]);
    }

    #[test]
    fn of_two_direct_trains_arriving_together_the_later_departure_wins() {
        let connections = vec![
            conn("EARLY", "O", "Z", 480, 600),
            conn("LATE", "O", "Z", 500, 600),
        ];
        let journey = o_to_z(&connections, &empty_interchange());
        assert_eq!(journey.departure_min, 500);
        assert_eq!(train_uids(&journey), ["LATE"]);
    }

    #[test]
    fn of_two_equal_routes_the_shorter_walk_wins() {
        // Both leave on T at 480 and reach Z at 520: by a 20-minute walk
        // from A, or a 3-minute one from B.
        let connections = vec![conn("T", "O", "A", 480, 500), conn("T", "A", "B", 501, 517)];
        let mut interchange = empty_interchange();
        for (tiploc, crs) in [("A", "AAA"), ("B", "BBB"), ("Z", "ZZZ")] {
            interchange
                .tiploc_to_crs
                .insert(tiploc.to_string(), crs.to_string());
            interchange
                .crs_to_tiplocs
                .insert(crs.to_string(), vec![tiploc.to_string()]);
        }
        let link = |minutes: i32| schedule_query::FixedLink {
            mode: "WALK".to_string(),
            to_crs: "ZZZ".to_string(),
            minutes,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
        };
        interchange
            .fixed_links_from_crs
            .insert("AAA".to_string(), vec![link(20)]);
        interchange
            .fixed_links_from_crs
            .insert("BBB".to_string(), vec![link(3)]);
        let journey = o_to_z(&connections, &interchange);
        assert_eq!(journey.arrival_min, 520);
        match &journey.legs[1] {
            JourneyLeg::Transfer(leg) => {
                assert_eq!((leg.from_tiploc.as_str(), leg.minutes), ("B", 3));
            }
            JourneyLeg::Train(_) => panic!("expected the walk"),
        }
    }
}
