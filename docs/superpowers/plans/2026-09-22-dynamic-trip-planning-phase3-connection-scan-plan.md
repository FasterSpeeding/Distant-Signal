# Plan: Dynamic Trip Planning — Phase 3: Connection Scan Algorithm

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 3 of the six-phase breakdown in
`docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md` §8: the
Connection Scan Algorithm (CSA) that backs `results: 'fastest'` — a single
linear sweep over Phase 2's sorted `Connection` array producing the
earliest-arrival itinerary from any of a set of origin TIPLOCs to any of a
set of destination TIPLOCs, including both same-station changes (Phase 1's
MSN minimum-change-time data) and cross-station fixed-link walks (Phase 1's
ALF data), via Phase 2's shared `interchange` lookup layer. **This is the
first genuinely new pathfinding logic in this codebase.**

**Architecture:** a new crate, `crates/trip-planner`, houses both search
algorithms (CSA here, RAPTOR in Phase 4) and their eventual differential
test (also Phase 4) — kept separate from `crates/schedule-query` (Phase 2's
home for the pure data/lookup layer) because this is meaningfully new,
independently-versionable algorithmic surface, matching this workspace's
existing one-crate-per-concern convention (`schedule-reference` parses and
publishes; `schedule-query` resolves; a new `trip-planner` searches). This
phase's own algorithm, `scan_connections`, is a faithful Rust port of the
real, working, tested Connection Scan implementation in the sibling
`Distant-Signal-MCP` project (`src/timetable/plan/csa.ts`, re-cloned and
re-read directly for this plan's own research pass) — same relax/
readySourceAt/fixed-link-relaxation structure, adapted from TypeScript
closures-over-mutable-state to a Rust struct with `&mut self` methods (the
idiomatic equivalent — recursive mutable closures don't borrow-check the
way nested function declarations do in TS; see Judgment Call 2). The
sibling's own per-lookup memoization caches (`changeTimeCache`,
`siblingsCache`, etc.) are deliberately **not** ported — see Judgment Call 3
for why they would be redundant complexity here.

**Tech stack:** Rust, new crate `crates/trip-planner`, depending on
`crates/schedule-query` (Phase 2's `Connection`/`InterchangeData`/lookup
functions).

**Spec:** `docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md`
§1, §2, §4 (≤2-interchange cap — note per Judgment Call 4, CSA itself is
NOT capped; the cap is a `results: 'fastest'` presentation-layer concern for
Phase 5, matching the sibling's own "CSA has no changes cap at all" design),
§8 Phase 3. Depends on this plan's own Phase 1 and Phase 2 documents (data
and lookup layer must already exist and be tested).

---

## Judgment calls this plan makes (read before Task 1)

1. **A new crate, `crates/trip-planner`, not a new module inside
   `crates/schedule-query`.** The design spec's own §8 doesn't name a
   specific crate for Phase 3/4's algorithms. Given this workspace's own
   established convention — one crate per architectural concern, confirmed
   by reading the root `Cargo.toml`'s 24-member list, e.g. `schedule-ingest`/
   `schedule-reference`/`schedule-query` are already three separate crates
   for "extract," "parse and publish," and "resolve" respectively — CSA and
   RAPTOR (a genuinely new ~500-line-each algorithmic surface per the
   sibling's own real file sizes: `csa.ts` 536 lines, `raptor.ts` 494 lines)
   deserve their own crate rather than growing `schedule-query` (whose own
   module doc, `lib.rs:17-26`, currently states "nothing in `crates/api`...
   depends on this crate" as a load-bearing fact about its own scope) into
   a mixed resolve-and-search library.

2. **The sibling's nested closures over mutable state become a struct with
   `&mut self` methods.** `csa.ts`'s `relax`/`relaxFixedLinks`/
   `readySourceAt` are closures inside `scanConnections`, capturing
   `earliestArrival`/`arrivedVia`/etc. by mutable reference and calling each
   other recursively (`relax` calls `relaxFixedLinks` which calls `relax`
   again). Rust's borrow checker does not accept this shape directly for
   closures capturing `&mut` state recursively; a `struct Scan<'a> { ...
   state fields ... }` with ordinary recursive methods (`fn relax(&mut self,
   ...)`, `fn relax_fixed_links(&mut self, ...)`) is the direct, idiomatic
   equivalent — the exact same state, the exact same call graph, just
   expressed as methods instead of closures. This is a mechanical
   translation, not an algorithmic change.

3. **The sibling's four per-lookup memoization caches
   (`changeTimeCache`/`siblingsCache`/`crsCache`/`tiplocsForCrsCache`,
   `csa.ts:191-291`) are deliberately NOT ported.** Those caches exist
   because that project's `minimumChangeTime`/`siblingTiplocs`/
   `crsForTiploc`/`tiplocsForCrs` are real SQLite round trips
   (`TimetableStore` methods) — expensive enough, repeated across a real
   ~290k-connection sweep, to be worth memoizing. This app's Phase 2
   equivalents (`schedule_query::minimum_change_time`/`sibling_tiplocs`,
   consulting `InterchangeData`'s own already-fully-materialized
   `HashMap`s) are already O(1) in-memory lookups with no round-trip cost —
   adding a cache in front of an already-O(1) `HashMap::get` would be
   redundant complexity with no measurable benefit, not a missing
   optimization. If a future profiling pass finds otherwise, add the cache
   then, backed by a real measurement, not preemptively here.

4. **CSA itself carries no `results: 'fastest'`-specific concerns, including
   no interchange-count cap.** The design spec's own §1 confirms this
   directly: *"CSA has no changes cap at all — it is Connection Scan's own
   unconstrained earliest-arrival answer, regardless of how many changes
   that requires"* (design spec, quoting the sibling's own `plan-journey.ts`
   reasoning almost verbatim, itself re-confirmed this pass by re-reading
   `src/tools/plan-journey.ts:60-102` directly). The design spec's §4
   "hard cap of at most 2 interchanges" is enforced at the Phase 5 API/
   presentation layer (a itinerary CSA finds with more than 2 changes is
   rejected/re-queried there, not something `scan_connections` itself knows
   about) — exactly mirroring how the sibling's own `plan_journey` tool,
   not `csa.ts` itself, owns the changes-cap reconciliation logic (that
   project's own `plan-journey.ts:60-102` comment block, re-read directly
   this pass, is explicit that `raptor.ts`'s cap is a caller-supplied
   parameter, never a value `csa.ts`/`raptor.ts` invent for themselves).
   `scan_connections` here matches that split precisely: no cap parameter
   at all.

---

## Non-goals

- **No RAPTOR, no Pareto set, no route-pattern grouping** — Phase 4.
- **No route constraints (`via`/`avoid`/`viaStop`/`avoidStop`)** — explicitly
  future work per the design spec's §1.
- **No API route, no frontend** — Phase 5/6.
- **No interchange-count cap inside this algorithm** — see Judgment Call 4.
- **No live-disruption awareness** — `scan_connections` reasons only over
  the static, scheduled `Connection` array Phase 2 built; matches the
  design spec's own §4 scope.

## Global Constraints

- **File scope.** Created:
  `crates/trip-planner/Cargo.toml` (new),
  `crates/trip-planner/src/lib.rs` (new),
  `crates/trip-planner/src/csa.rs` (new).
  Modified: `Cargo.toml` (workspace root, add `"crates/trip-planner"` to
  `members`).
- **Testing.** `cargo fmt --all`, `cargo clippy --workspace --all-features
  --all-targets -- -D warnings`, `cargo test --workspace` — all three exact
  CI invocations. This entire phase is pure, I/O-free Rust with no database
  dependency at all; there are no `#[ignore]`d tests in this phase.
- **No invented algorithm behavior.** Every piece of interchange/relaxation
  logic in `csa.rs` must trace to a specific, cited line range in the
  sibling project's own `csa.ts`, re-read directly during this plan's
  research pass (re-cloned from
  `ssh://git@git-bringer-ssh.fox-prometheus.ts.net/lucy/Distant-Signal-MCP.git`) —
  not reconstructed from the design spec's own higher-level prose summary
  alone.

## Review Focus

- **A query where origin and destination are the same station (or share a
  CRS via siblings)** — `scan_connections` must return a trivial/None
  result sensibly, not loop or panic (the origin is explicitly excluded
  from needing a change, per `ready_source_at`'s own origin branch).
- **A journey reachable ONLY by a fixed-link walk, with no train ride at
  all** (the sibling's own explicitly-named case, `csa.ts:395-417`: "Euston
  -> King's Cross by tube, with no train ride at all... is a perfectly
  ordinary, valid journey and must not throw") — `reconstruct_legs` must
  produce exactly one `TransferLeg` and no `TrainLeg`s, not treat this as
  "unreachable."
  it.
- **A same-train continuation through a coach-stand sentinel station**
  (`ChangeTime::NoInterchange` must only ever block a *fresh* boarding,
  never a through-train already aboard, per Phase 2's own
  `minimum_change_time` doc) — a schedule that calls at one of these
  stations mid-route, without anyone changing there, must still produce a
  connection through it.
- **A same-CRS sibling change enabling a boarding that would otherwise be
  impossible** (the real Wimbledon `WDON`/`WIMBLDN` case) — a destination
  reachable only because a different platform-group TIPLOC's earlier
  arrival satisfies the boarding TIPLOC's own minimum change time.
- **No connection at all reaches any destination TIPLOC** — must return
  `None`, not panic on an empty reconstruction walk.

---

## Task 1: `crates/trip-planner` — crate scaffold

**Files:**
- Create: `crates/trip-planner/Cargo.toml`
- Create: `crates/trip-planner/src/lib.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Register the crate** in the workspace root `Cargo.toml`,
  after `"crates/schedule-query"`:

```toml
    "crates/schedule-query",
    "crates/trip-planner",
```

- [ ] **Step 2: Write the crate manifest**

```toml
[package]
name = "trip-planner"
version = "0.1.0"
edition = "2021"

[dependencies]
schedule-query = { path = "../schedule-query" }
chrono = { workspace = true }
```

  (Match the exact `chrono`/edition version convention every sibling crate's
  own `Cargo.toml` already uses — `grep -A3 "\[dependencies\]" crates/schedule-query/Cargo.toml`
  to confirm the exact `chrono` version/feature spec before writing this
  file, since this plan cannot see whether this workspace pins `chrono` via
  a `[workspace.dependencies]` table or per-crate.)

- [ ] **Step 3: Write `lib.rs`**

```rust
//! Journey-search algorithms over a `schedule_query::Connection` array and
//! `schedule_query::InterchangeData` -- Connection Scan (this phase) and,
//! from Phase 4 onward, RAPTOR, plus the differential test asserting they
//! agree. See
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §1.

pub mod csa;

pub use csa::{scan_connections, Journey, JourneyLeg, ScanOptions, TrainLeg, TransferLeg};
```

- [ ] **Step 4: Verify**

```bash
cargo build -p trip-planner
```

  Expected: builds (with an empty `csa` module stub — Task 2 fills it in;
  add a placeholder `pub mod csa {}` if doing this step before Task 2's
  file exists, then remove it once Task 2 lands).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/trip-planner/Cargo.toml crates/trip-planner/src/lib.rs
git commit -m "trip-planner: scaffold the new crate for journey-search algorithms"
```

---

## Task 2: `csa.rs` — the Connection Scan algorithm

**Files:**
- Create: `crates/trip-planner/src/csa.rs`

**Interfaces:**
- Produces: `scan_connections(options: ScanOptions) -> Option<Journey>`,
  `Journey { legs: Vec<JourneyLeg>, departure_min: u32, arrival_min: u32 }`,
  `JourneyLeg::{Train(TrainLeg), Transfer(TransferLeg)}` — consumed by
  Phase 4's differential test and Phase 5's route handler.
- Consumes: `schedule_query::{Connection, InterchangeData, ChangeTime,
  minimum_change_time, sibling_tiplocs, fixed_links_from}` (Phase 2).

- [ ] **Step 1: Write the leg/journey types**, at the top of `csa.rs`:

```rust
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
use schedule_query::{fixed_links_from, minimum_change_time, sibling_tiplocs, ChangeTime, Connection, InterchangeData};

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
```

- [ ] **Step 2: Write the `ArrivalSource` type and the `Scan` struct**:

```rust
/// How a stop's earliest-arrival entry was produced.
#[derive(Debug, Clone)]
enum ArrivalSource {
    Train(Connection),
    Link { from_tiploc: String, mode: String, minutes: i32 },
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
```

- [ ] **Step 3: Implement `Scan`'s methods** — `ready_source_at`, `relax`,
  `relax_fixed_links`, direct translations of `csa.ts:244-359`:

```rust
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
        if self.origin.contains(tiploc) {
            return Some(ReadySource { time: self.departure_min, from: tiploc.to_string() });
        }
        let ChangeTime::Finite(change_time) = minimum_change_time(self.interchange, tiploc) else {
            return None;
        };

        let mut best: Option<ReadySource> = None;
        if let Some(&arrival) = self.earliest_arrival.get(tiploc) {
            best = Some(ReadySource { time: arrival + change_time, from: tiploc.to_string() });
        }
        for sibling in sibling_tiplocs(self.interchange, tiploc) {
            let Some(&sibling_arrival) = self.earliest_arrival.get(sibling) else {
                continue;
            };
            let candidate = sibling_arrival + change_time;
            if best.as_ref().is_none_or(|b| candidate < b.time) {
                best = Some(ReadySource { time: candidate, from: sibling.to_string() });
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
        let current_best = self.earliest_arrival.get(tiploc).copied().unwrap_or(u32::MAX);
        if arrival_min >= current_best {
            return;
        }
        self.earliest_arrival.insert(tiploc.to_string(), arrival_min);
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
        let Some(crs) = self.interchange.tiploc_to_crs.get(from_tiploc).cloned() else {
            return;
        };
        let links: Vec<_> = fixed_links_from(self.interchange, &crs, self.date, at_min)
            .into_iter()
            .cloned()
            .collect();
        for link in links {
            let candidate = at_min + link.minutes as u32;
            let Some(destination_tiplocs) = self.interchange.crs_to_tiplocs.get(&link.to_crs).cloned() else {
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
```

  (`links`/`destination_tiplocs` are cloned into owned `Vec`s before the
  loop body calls `self.relax` — `fixed_links_from`/`crs_to_tiplocs`
  otherwise borrow `self.interchange` immutably for the loop's whole
  duration, which conflicts with `self.relax`'s `&mut self`. This is the
  one place the Rust port needs an extra clone the TypeScript original
  didn't — a direct, unavoidable consequence of Rust's borrow checker, not
  a behavior change; both `Connection`/`FixedLink`/`String` clones here are
  small and this is off the hot per-connection path (it only runs once per
  *improved* stop, not once per connection scanned).)

- [ ] **Step 4: Implement `scan_connections` itself** — direct translation
  of `csa.ts:148-428`'s main sweep:

```rust
/// Connection Scan: the earliest-arrival journey from any of
/// `options.from_tiplocs` to any of `options.to_tiplocs`, departing no
/// earlier than `options.departure_min`. `None` when no connection reaches
/// the destination at all.
pub fn scan_connections(options: ScanOptions) -> Option<Journey> {
    let origin: HashSet<String> = options.from_tiplocs.iter().cloned().collect();
    let destinations: HashSet<String> = options.to_tiplocs.iter().cloned().collect();

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
            scan.leg_boarded_at.insert(connection.uid.clone(), connection.clone());
            scan.boarded_from.insert(connection.uid.clone(), source.from);
        }

        scan.relax(
            &connection.to_tiploc,
            connection.arrival_min,
            ArrivalSource::Train(connection.clone()),
        );
    }

    let best_dest_tiploc = scan.best_dest_tiploc?;
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
```

- [ ] **Step 5: Implement `reconstruct_legs`** — direct translation of
  `csa.ts:476-536`:

```rust
/// Walks backward from the stop that reached the destination to the
/// origin, one leg at a time -- a train leg read off `leg_boarded_at` in
/// one step (the merge already decided during the sweep), a fixed-link
/// step producing its own `TransferLeg`.
fn reconstruct_legs(end_tiploc: &str, scan: &Scan) -> Vec<JourneyLeg> {
    let mut legs = Vec::new();
    let mut stop = end_tiploc.to_string();

    while !scan.origin.contains(&stop) {
        let via = scan
            .arrived_via
            .get(&stop)
            .unwrap_or_else(|| panic!("internal error: stop {stop} was reached but has no recorded arrival source"));

        match via {
            ArrivalSource::Link { from_tiploc, mode, minutes } => {
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
                let boarded = scan
                    .leg_boarded_at
                    .get(&connection.uid)
                    .unwrap_or_else(|| panic!("internal error: uid {} was relaxed without ever being boarded", connection.uid));
                legs.push(JourneyLeg::Train(TrainLeg {
                    uid: boarded.uid.clone(),
                    from_tiploc: boarded.from_tiploc.clone(),
                    to_tiploc: connection.to_tiploc.clone(),
                    departure_min: boarded.departure_min,
                    arrival_min: connection.arrival_min,
                }));
                let source = scan
                    .boarded_from
                    .get(&connection.uid)
                    .unwrap_or_else(|| panic!("internal error: uid {} was boarded without a recorded readiness source", connection.uid));
                stop = source.clone();
            }
        }
    }

    legs.reverse();
    legs
}
```

- [ ] **Step 6: Write the tests**, using small, hand-built `InterchangeData`/
  `Connection` fixtures (no CIF parsing involved — this algorithm's own
  correctness is independent of how the array was built):

```rust
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
        interchange.change_time_by_tiploc.insert("MKC".to_string(), 5);
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
        interchange.change_time_by_tiploc.insert("MKC".to_string(), 5);
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a journey exists via the later, valid connection");
        assert_eq!(journey.arrival_min, 610, "the too-tight 532 change must be rejected");
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
        interchange.change_time_by_tiploc.insert("MKC".to_string(), 99);
        let journey = scan_connections(ScanOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
        })
        .expect("a through-train journey exists despite MKC's sentinel");
        assert_eq!(journey.legs.len(), 1, "one merged leg -- the same uid throughout");
        assert_eq!(journey.arrival_min, 560);
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
        interchange.tiploc_to_crs.insert("WDON".to_string(), "WIM".to_string());
        interchange.tiploc_to_crs.insert("WIMBLDN".to_string(), "WIM".to_string());
        interchange
            .crs_to_tiplocs
            .insert("WIM".to_string(), vec!["WDON".to_string(), "WIMBLDN".to_string()]);
        interchange.change_time_by_tiploc.insert("WIMBLDN".to_string(), 5);

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
    }

    #[test]
    fn a_journey_reached_purely_by_a_fixed_link_with_no_train_at_all_is_valid() {
        // Euston -> King's Cross by tube, no train ride at all.
        let connections: Vec<Connection> = Vec::new();
        let mut interchange = empty_interchange();
        interchange.tiploc_to_crs.insert("EUSTON".to_string(), "EUS".to_string());
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
```

- [ ] **Step 7: Run the tests**

```bash
cargo test -p trip-planner
```

  Expected: all 9 tests pass.

- [ ] **Step 8: Run clippy and fmt**

```bash
cargo fmt --all
cargo clippy -p trip-planner --all-features --all-targets -- -D warnings
```

- [ ] **Step 9: Commit**

```bash
git add crates/trip-planner/src/csa.rs
git commit -m "trip-planner: implement Connection Scan (results: fastest)"
```

---

## Self-review notes

- **Spec coverage**: §8 Phase 3's "back-pointer/predecessor trace needed to
  recover an actual leg-by-leg itinerary from CSA's raw arrival-time
  frontier" (§1) is `reconstruct_legs`; the ≤2-interchange cap and
  hosting/profiling against real `timetable_full.zip` data named in §8
  Phase 3's own text are explicitly deferred to Phase 5 (Judgment Call 4) —
  this algorithm itself has no interchange-count concept to cap, matching
  the sibling's own confirmed design.
- **Placeholder scan**: none — `scan_connections`/`reconstruct_legs` are
  complete, real implementations with real tests, not stubs.
- **Type consistency**: `Journey`/`JourneyLeg`/`TrainLeg`/`TransferLeg` here
  are exactly what Phase 4's differential test and Phase 5's route handler
  will consume — Phase 4's own plan document should re-read this file's
  exact type names rather than re-deriving them.
- **Review Focus**: all five items have a directly corresponding test above
  (`a_journey_reached_purely_by_a_fixed_link_with_no_train_at_all_is_valid`,
  `a_same_train_continuation_through_a_coach_stand_sentinel_is_unaffected`,
  `a_same_crs_sibling_change_enables_an_otherwise_impossible_boarding`,
  `no_connection_reaches_the_destination_returns_none`).
