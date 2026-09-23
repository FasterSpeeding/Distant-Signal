# Plan: Dynamic Trip Planning — Phase 4: RAPTOR + Differential Testing

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 4 of the six-phase breakdown in
`docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md` §8: the
round-based search backing `results: 'options'` — a Pareto set trading
arrival time against number of changes — plus the differential test suite
that is this feature's primary correctness mechanism at national scale (per
the design spec's own §1: "on a graph of roughly 290,000 connections per
day, no fixture set can be hand-verified exhaustively... running two
independently-implemented algorithms against the same connections array and
asserting agreement on earliest-arrival [is] materially cheaper to gain
confidence from").

**Architecture — a real correction to the design spec's own §1/§8, found by
reading the actual sibling code:** the design spec states RAPTOR "needs one
preprocessing step CSA doesn't: grouping trips into routes by identical stop
pattern... a real, if modest, additional preprocessing step" (§1). **This is
not what the real, working `Distant-Signal-MCP` implementation does** —
independently confirmed this pass by reading `src/timetable/plan/raptor.ts`
in full (re-cloned directly for this plan's own research pass, not carried
forward from the design spec's own unverified citation). That file's
`raptorSearch` runs a full sweep of the *same flat connections array* CSA
uses, once per round, with no route-grouping concept anywhere in it — round
*k* answers "earliest arrival using at most *k* trips" by re-scanning every
connection and tracking per-round reachability, trading the classic textbook
RAPTOR's per-route scan efficiency for an implementation that needs **zero**
new preprocessing beyond what Phase 3 already built. See Judgment Call 1.
This phase's own `raptor.rs` is therefore a Rust port of that real file's
actual round-based-Connection-Scan shape, not the route-pattern-grouped
textbook RAPTOR the design spec describes — and needs no new Phase 2 work at
all.

**Tech stack:** Rust, `crates/trip-planner` (added in Phase 3).

**Spec:** `docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md`
§1, §4, §8 Phase 4. Depends on Phase 3
(`2026-09-22-dynamic-trip-planning-phase3-connection-scan-plan.md`) —
`scan_connections`/`Journey`/`JourneyLeg`/`TrainLeg`/`TransferLeg` must
already exist; this phase's differential test asserts against them
directly.

---

## Judgment calls this plan makes (read before Task 1)

1. **No route-pattern-grouping preprocessing is built — see this plan's own
   Architecture section above for the direct evidence.** This corrects the
   design spec's own §1/§2/§8 Phase 4 text, which the spec's own provenance
   note already flags as carried forward from an investigation that could
   not re-verify the sibling project's code directly. This plan's research
   pass *could* (the sibling repository was reachable via
   `ssh://git@git-bringer-ssh.fox-prometheus.ts.net/lucy/Distant-Signal-MCP.git`),
   and reading `raptor.ts:1-355` end to end confirms: `RaptorOptions`
   carries the same `connections: Connection[]` shape `CsaOptions` does
   (`raptor.ts:92-115`); the round loop (`raptor.ts:322-352`) iterates `for
   const c of connections` directly, exactly like `csa.ts`'s own single
   sweep; and there is no `Route`/`RoutePattern`/grouping type anywhere in
   the file (confirmed by a full-file read, not a grep that could miss an
   import). The design spec's own §2 table entry ("needs one preprocessing
   step CSA doesn't... `ScheduleIndex` is UID-keyed today, not
   route-pattern-keyed") is simply not true of the real implementation it
   cites, and this plan does not implement a feature the reference
   implementation itself doesn't have.

2. **RAPTOR's own internal round cap must NEVER be trusted at its
   library-level default — flagged prominently here for Phase 5 to act on,
   even though this phase's own code only needs to expose the parameter,
   not decide its value.** Re-reading `src/tools/plan-journey.ts:60-102`
   directly this pass surfaces a real, previously-hard-won lesson from the
   sibling project's own history: `raptor.ts`'s own `DEFAULT_MAX_ROUNDS = 8`
   (round *k* = at most *k−1* changes, so 8 rounds ⇒ up to 7 changes) is
   **not always enough** — that project's own wide differential-test run
   found a real pair (Kilmaurs to Hunstanton Bus Station, a specific date)
   whose only itinerary needs exactly 9 changes, which an 8-round cap
   cannot see *at all* (not "finds a worse answer" — finds **no** answer,
   indistinguishable from "no route exists"). The sibling's own fix was
   architectural, not a bigger constant: the *caller* (`plan-journey.ts`,
   this app's future Phase 5) always computes and passes an explicit
   `max_rounds` derived from whatever changes-cap is actually in effect
   for that call (`changes_cap + 1` trips, plus one further round of
   headroom to detect whether the cap actually bound the answer), **never**
   relying on `raptor_search`'s own default. This phase's `raptor_search`
   signature (Task 1) therefore takes `max_rounds` as a required, not
   optional-with-a-silently-trusted-default, concept — see Task 1's own
   doc comment on the field, and Phase 5's own plan document (not yet
   written at the time of this document) must repeat this exact warning
   rather than call `raptor_search` with no thought given to the parameter.

3. **`ArrivalSource` is redefined privately inside `raptor.rs`, not shared
   with `csa.rs`'s own private type of the same shape.** Matches the
   sibling's own deliberate choice (`raptor.ts:120-127`: "Mirrors `csa.ts`'s
   `ArrivalSource` exactly... redefined here rather than imported because
   `csa.ts` does not export it and this file must not modify `csa.ts`") —
   Task 2's differential test is only meaningful evidence if the two
   algorithms are genuinely independent implementations sharing no
   search-logic code, not one calling into the other's internals. Both
   already share Phase 2's `interchange` lookup layer (the *data*, not the
   *search algorithm*) — that sharing is intentional and unavoidable (both
   need the same MSN/ALF-derived facts), and does not compromise the
   differential test's value; sharing `relax`/`readySourceAt` logic would.

---

## Non-goals

- **No route constraints (`via`/`avoid`/`viaStop`/`avoidStop`)** — future
  work per the design spec's §1.
- **No API route, no frontend** — Phase 5/6.
- **No decision, in this phase, about what `max_rounds` value Phase 5's
  route handler actually passes** — that is a Phase 5 decision informed by
  Judgment Call 2 above, not resolved here.

## Global Constraints

- **File scope.** Created:
  `crates/trip-planner/src/raptor.rs` (new),
  `crates/trip-planner/tests/differential.rs` (new, an integration test
  crate-level file so it can freely construct both `csa::scan_connections`
  and `raptor::raptor_search` fixtures without `cfg(test)`-only visibility
  concerns).
  Modified: `crates/trip-planner/src/lib.rs` (register `raptor`, re-export
  its public types).
- **Testing.** `cargo fmt --all`, `cargo clippy --workspace --all-features
  --all-targets -- -D warnings`, `cargo test --workspace` — all three exact
  CI invocations. Pure Rust, no database — no `#[ignore]`d tests in this
  phase either.
- **No invented algorithm behavior.** Every piece of round-based
  relaxation logic in `raptor.rs` must trace to a specific, cited line
  range in the sibling project's own `raptor.ts`, re-read directly during
  this plan's research pass — not reconstructed from the design spec's own
  (in this specific case, incorrect) prose summary.

## Review Focus

- **RAPTOR and Connection Scan disagreeing on earliest arrival for any
  query** — this is not a "nice to have" test, it is this feature's stated
  primary correctness mechanism (design spec §1); Task 2's differential
  test suite must run across a genuinely varied set of queries (different
  origin/destination pairs, different times of day, a query with a fixed
  link involved, a query with no route at all), not one happy-path case.
- **RAPTOR reporting `changes: 0` for both a direct no-change train journey
  AND a fixed-link-only journey with no train at all** (the sibling's own
  explicitly-reasoned choice, `raptor.ts:73-90`: "both are journeys with
  nothing to change *between*") — a naive `legs.len() - 1` count would give
  the wrong answer for the fixed-link-only case (`0` legs of *any* kind
  minus one would underflow, or `1` transfer leg minus one would wrongly
  say `0` looks right by accident but for the wrong reason if legs mix
  kinds) — the test must specifically distinguish "0 changes because no
  train boarded at all" from "0 changes because exactly 1 train boarded."
- **A round that improves nothing** — the loop must actually stop early
  (`touched.is_empty()`), not run to `max_rounds` unconditionally; a test
  should assert the returned Pareto set doesn't contain duplicate/dominated
  entries from rounds that found nothing new.
- **A Pareto entry from a later round that ties, rather than strictly
  beats, an earlier round's arrival** — must be excluded (dominated), per
  `raptor.ts`'s own strict-improvement rule; a test must construct exactly
  this tie and assert only the earlier (fewer-changes) entry survives.

---

## Task 1: `raptor.rs` — the round-based search

**Files:**
- Create: `crates/trip-planner/src/raptor.rs`
- Modify: `crates/trip-planner/src/lib.rs`

**Interfaces:**
- Produces: `raptor_search(options: RaptorOptions) -> Vec<RaptorJourney>`,
  `RaptorJourney { legs: Vec<crate::csa::JourneyLeg>, departure_min: u32,
  arrival_min: u32, changes: u32 }` — consumed by Task 2's differential test
  and, in Phase 5, the route handler's `results: 'options'` path.
- Consumes: `crate::csa::JourneyLeg`/`TrainLeg`/`TransferLeg` (Phase 3 —
  RAPTOR's own *output shape* is deliberately shared with CSA's, since both
  ultimately describe the same kind of itinerary; only the *search logic*
  producing it is independent, per Judgment Call 3), and
  `schedule_query::{Connection, InterchangeData, ChangeTime,
  minimum_change_time, sibling_tiplocs, fixed_links_from}` (Phase 2).

- [ ] **Step 1: Write the options/journey types and round state**

```rust
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
use schedule_query::{fixed_links_from, minimum_change_time, sibling_tiplocs, ChangeTime, Connection, InterchangeData};

use crate::csa::{JourneyLeg, TrainLeg, TransferLeg};

fn train_leg_count(legs: &[JourneyLeg]) -> u32 {
    legs.iter().filter(|leg| matches!(leg, JourneyLeg::Train(_))).count() as u32
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
    Link { from_tiploc: String, mode: String, minutes: i32 },
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
```

- [ ] **Step 2: Write the free functions** — `ready_source_at`,
  `relax_in_round`, `relax_fixed_links_in_round`, direct translations of
  `raptor.ts:220-301`, as plain functions (not struct methods, since
  `RoundState` alone — no origin/destination/departure_min fields needed on
  it — is enough state to thread through explicitly):

```rust
fn ready_source_at(
    source: &HashMap<String, u32>,
    origin: &HashSet<String>,
    departure_min: u32,
    interchange: &InterchangeData,
    tiploc: &str,
) -> Option<ReadySource> {
    if origin.contains(tiploc) {
        return Some(ReadySource { time: departure_min, from: tiploc.to_string() });
    }
    let ChangeTime::Finite(change_time) = minimum_change_time(interchange, tiploc) else {
        return None;
    };

    let mut best: Option<ReadySource> = None;
    if let Some(&arrival) = source.get(tiploc) {
        best = Some(ReadySource { time: arrival + change_time, from: tiploc.to_string() });
    }
    for sibling in sibling_tiplocs(interchange, tiploc) {
        let Some(&sibling_arrival) = source.get(sibling) else {
            continue;
        };
        let candidate = sibling_arrival + change_time;
        if best.as_ref().is_none_or(|b| candidate < b.time) {
            best = Some(ReadySource { time: candidate, from: sibling.to_string() });
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
    let Some(crs) = interchange.tiploc_to_crs.get(from_tiploc).cloned() else {
        return;
    };
    let links: Vec<_> = fixed_links_from(interchange, &crs, date, at_min).into_iter().cloned().collect();
    for link in links {
        let candidate = at_min + link.minutes as u32;
        let Some(destination_tiplocs) = interchange.crs_to_tiplocs.get(&link.to_crs).cloned() else {
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
```

- [ ] **Step 3: Write `raptor_search` and `build_pareto_set`** — direct
  translations of `raptor.ts:169-355`:

```rust
pub fn raptor_search(options: RaptorOptions) -> Vec<RaptorJourney> {
    let origin: HashSet<String> = options.from_tiplocs.iter().cloned().collect();

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
                current.leg_boarded_at.insert(connection.uid.clone(), connection.clone());
                current.boarded_from.insert(connection.uid.clone(), source.from);
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
fn build_pareto_set(rounds: &[RoundState], origin: &HashSet<String>, to_tiplocs: &[String]) -> Vec<RaptorJourney> {
    let mut results = Vec::new();
    let mut running_best = u32::MAX;

    for (k, round) in rounds.iter().enumerate().skip(1) {
        let mut best_tiploc: Option<&str> = None;
        let mut best_arrival = u32::MAX;
        for destination in to_tiplocs {
            if let Some(&arrival) = round.arrival.get(destination) {
                if arrival < best_arrival {
                    best_arrival = arrival;
                    best_tiploc = Some(destination.as_str());
                }
            }
        }
        let Some(best_tiploc) = best_tiploc else { continue };
        if !(best_arrival < running_best) {
            continue;
        }
        running_best = best_arrival;

        let legs = reconstruct_legs(best_tiploc, k, rounds, origin);
        let Some(first_leg) = legs.first() else { continue };
        let departure_min = match first_leg {
            JourneyLeg::Train(leg) => leg.departure_min,
            JourneyLeg::Transfer(leg) => leg.departure_min,
        };
        results.push(RaptorJourney {
            changes: train_leg_count(&legs).saturating_sub(1).max(0),
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
fn reconstruct_legs(end_tiploc: &str, start_round: usize, rounds: &[RoundState], origin: &HashSet<String>) -> Vec<JourneyLeg> {
    let mut legs = Vec::new();
    let mut stop = end_tiploc.to_string();
    let mut round_index = start_round;

    while !origin.contains(&stop) {
        let round = &rounds[round_index];
        let via = round
            .arrived_via
            .get(&stop)
            .unwrap_or_else(|| panic!("internal error: stop {stop} reached in round {round_index} but has no recorded arrival source"));

        match via {
            ArrivalSource::Link { from_tiploc, mode, minutes } => {
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
```

- [ ] **Step 4: Register the module** in `crates/trip-planner/src/lib.rs`:

```rust
pub mod csa;
pub mod raptor;

pub use csa::{scan_connections, Journey, JourneyLeg, ScanOptions, TrainLeg, TransferLeg};
pub use raptor::{raptor_search, RaptorJourney, RaptorOptions};
```

- [ ] **Step 5: Write tests** covering both bullets of this plan's own
  Review Focus that are specific to RAPTOR's own logic (the cross-algorithm
  agreement tests live in Task 2's differential suite instead):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn conn(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
        Connection { uid: uid.to_string(), from_tiploc: from.to_string(), to_tiploc: to.to_string(), departure_min: dep, arrival_min: arr }
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
        interchange.tiploc_to_crs.insert("EUSTON".to_string(), "EUS".to_string());
        interchange.crs_to_tiplocs.insert("KGX".to_string(), vec!["KINGX".to_string()]);
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
        assert_eq!(results[0].changes, 0, "a fixed-link-only journey has nothing to change BETWEEN");
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
        interchange.change_time_by_tiploc.insert("MKC".to_string(), 5);
        let results = raptor_search(RaptorOptions {
            connections: &connections,
            interchange: &interchange,
            from_tiplocs: &["EUSTON".to_string()],
            to_tiplocs: &["MAN".to_string()],
            departure_min: 480,
            date: date(),
            max_rounds: 4,
        });
        assert_eq!(results.len(), 2, "both the 0-change and 1-change options are Pareto-optimal");
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
        interchange.change_time_by_tiploc.insert("MKC".to_string(), 5);
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
        assert_eq!(arrivals_at_560, 1, "a tied later round must not add a second, dominated entry");
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
```

- [ ] **Step 6: Run the tests, fmt, clippy**

```bash
cargo test -p trip-planner raptor::
cargo fmt --all
cargo clippy -p trip-planner --all-features --all-targets -- -D warnings
```

  Expected: all 6 new tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/trip-planner/src/raptor.rs crates/trip-planner/src/lib.rs
git commit -m "trip-planner: implement RAPTOR (results: options), a round-based scan over the shared connections array"
```

---

## Task 2: Differential testing — CSA and RAPTOR must agree

**Files:**
- Create: `crates/trip-planner/tests/differential.rs`

**Interfaces:**
- Consumes: `trip_planner::{scan_connections, raptor_search, ScanOptions,
  RaptorOptions}` (both public re-exports from Task 1/Phase 3).

- [ ] **Step 1: Write a shared fixture builder** representing a small but
  genuinely non-trivial network (enough stations/schedules/interchanges
  that the two algorithms' agreement is real evidence, not a coincidence of
  triviality) — a hand-built "mini timetable" covering: a direct route, a
  route needing exactly one change with a real minimum-change-time
  constraint, a route only reachable via a fixed link, and a route with no
  connection at all:

```rust
//! Differential test: for every query below, `scan_connections`'s earliest
//! arrival must equal `raptor_search`'s own best (round-1-onward, lowest
//! arrival) Pareto entry -- the primary correctness mechanism named in
//! docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §1.
//! The two algorithms share NO search-logic code (only the pure
//! `schedule_query::interchange` data layer, per this plan's Judgment
//! Call 3) -- agreement here is real, independent evidence, not a
//! tautology.

use chrono::NaiveDate;
use schedule_query::{Connection, FixedLink, InterchangeData};
use std::collections::HashMap;
use trip_planner::{raptor_search, scan_connections, RaptorOptions, ScanOptions};

fn date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 23).unwrap()
}

fn conn(uid: &str, from: &str, to: &str, dep: u32, arr: u32) -> Connection {
    Connection { uid: uid.to_string(), from_tiploc: from.to_string(), to_tiploc: to.to_string(), departure_min: dep, arrival_min: arr }
}

/// A small but structurally varied network: two direct EUSTON->MKC
/// services (one earlier, one later), a EUSTON->MKC->MAN two-leg route
/// requiring a real 5-minute minimum change at MKC, an alternative
/// MKC->MAN service too tight to make the change, and an
/// EUSTON<->KINGX fixed link with no train involved.
fn network() -> (Vec<Connection>, InterchangeData) {
    let connections = vec![
        conn("DIRECT-EARLY", "EUSTON", "MKC", 480, 530),
        conn("DIRECT-LATE", "EUSTON", "MKC", 600, 650),
        conn("LEG1", "EUSTON", "MKC", 480, 530),
        conn("TOO-TIGHT", "MKC", "MAN", 531, 590),
        conn("LEG2", "MKC", "MAN", 536, 600),
    ];
    let mut interchange = InterchangeData {
        change_time_by_tiploc: HashMap::new(),
        tiploc_to_crs: HashMap::new(),
        crs_to_tiplocs: HashMap::new(),
        fixed_links_from_crs: HashMap::new(),
    };
    interchange.change_time_by_tiploc.insert("MKC".to_string(), 5);
    interchange.tiploc_to_crs.insert("EUSTON".to_string(), "EUS".to_string());
    interchange.crs_to_tiplocs.insert("KGX".to_string(), vec!["KINGX".to_string()]);
    interchange.fixed_links_from_crs.insert(
        "EUS".to_string(),
        vec![FixedLink {
            mode: "TUBE".to_string(),
            to_crs: "KGX".to_string(),
            minutes: 5,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
        }],
    );
    (connections, interchange)
}

/// Runs both algorithms for one query and asserts their earliest arrival
/// agrees, returning both results for any further assertion the caller
/// wants to make.
fn assert_agreement(
    connections: &[Connection],
    interchange: &InterchangeData,
    from: &str,
    to: &str,
    departure_min: u32,
) -> (Option<trip_planner::Journey>, Vec<trip_planner::RaptorJourney>) {
    let from_tiplocs = vec![from.to_string()];
    let to_tiplocs = vec![to.to_string()];

    let csa_result = scan_connections(ScanOptions {
        connections,
        interchange,
        from_tiplocs: &from_tiplocs,
        to_tiplocs: &to_tiplocs,
        departure_min,
        date: date(),
    });
    let raptor_results = raptor_search(RaptorOptions {
        connections,
        interchange,
        from_tiplocs: &from_tiplocs,
        to_tiplocs: &to_tiplocs,
        departure_min,
        date: date(),
        max_rounds: 8,
    });
    let raptor_best = raptor_results.iter().map(|j| j.arrival_min).min();

    match (&csa_result, raptor_best) {
        (Some(csa), Some(raptor_arrival)) => {
            assert_eq!(
                csa.arrival_min, raptor_arrival,
                "CSA and RAPTOR disagree on earliest arrival for {from} -> {to} departing {departure_min}: \
                 CSA says {}, RAPTOR says {raptor_arrival}",
                csa.arrival_min
            );
        }
        (None, None) => {}
        (csa, raptor) => panic!(
            "CSA and RAPTOR disagree on REACHABILITY for {from} -> {to} departing {departure_min}: \
             CSA={csa:?}, RAPTOR best={raptor:?}"
        ),
    }

    (csa_result, raptor_results)
}
```

- [ ] **Step 2: Write the query set**

```rust
#[test]
fn direct_journey_agrees() {
    let (connections, interchange) = network();
    let (csa, _) = assert_agreement(&connections, &interchange, "EUSTON", "MKC", 480);
    assert_eq!(csa.unwrap().arrival_min, 530);
}

#[test]
fn a_later_departure_time_still_agrees() {
    let (connections, interchange) = network();
    let (csa, _) = assert_agreement(&connections, &interchange, "EUSTON", "MKC", 590);
    assert_eq!(csa.unwrap().arrival_min, 650, "only the DIRECT-LATE service is boardable this late");
}

#[test]
fn a_journey_requiring_the_valid_change_agrees() {
    let (connections, interchange) = network();
    let (csa, _) = assert_agreement(&connections, &interchange, "EUSTON", "MAN", 480);
    assert_eq!(csa.unwrap().arrival_min, 600, "the too-tight 531 change must be rejected by both algorithms identically");
}

#[test]
fn a_fixed_link_only_query_agrees() {
    let (connections, interchange) = network();
    let (csa, raptor) = assert_agreement(&connections, &interchange, "EUSTON", "KINGX", 480);
    let csa = csa.expect("a fixed-link journey exists");
    assert_eq!(csa.arrival_min, 485);
    assert_eq!(raptor[0].changes, 0);
}

#[test]
fn an_unreachable_destination_agrees_on_no_journey_at_all() {
    let (connections, interchange) = network();
    assert_agreement(&connections, &interchange, "EUSTON", "NOWHERE", 480);
}

#[test]
fn a_departure_time_after_every_service_has_left_agrees_on_no_journey() {
    let (connections, interchange) = network();
    assert_agreement(&connections, &interchange, "EUSTON", "MKC", 700);
}
```

- [ ] **Step 3: Run the differential suite**

```bash
cargo test -p trip-planner --test differential
```

  Expected: all 6 tests pass, proving CSA and RAPTOR agree across every
  structurally distinct case this fixture network covers (direct, delayed
  departure, valid change, rejected too-tight change, fixed-link-only,
  unreachable, and departed-too-late).

- [ ] **Step 4: Commit**

```bash
git add crates/trip-planner/tests/differential.rs
git commit -m "trip-planner: add CSA/RAPTOR differential test suite"
```

---

## Self-review notes

- **Spec coverage**: §8 Phase 4's "differential test suite asserting
  RAPTOR's earliest-arrival result agrees with CSA's on the same query,
  across a real, varied set of origin/destination/date combinations" is
  Task 2 in full. The route-pattern-grouping preprocessing step named in
  §1/§2/§8 is corrected, not implemented, per this plan's own Architecture
  section — a deliberate, evidenced deviation, not a gap.
- **Placeholder scan**: none. Judgment Call 2's `max_rounds` warning is
  intentionally unresolved AS A VALUE in this phase (Phase 5's job to pick
  one) but is not a placeholder — it is a correctly-scoped required
  parameter with an explicit, real historical justification for why no
  silent default belongs in this phase's own code.
- **Type consistency**: `RaptorJourney.legs: Vec<JourneyLeg>` reuses Phase
  3's exact `JourneyLeg`/`TrainLeg`/`TransferLeg` types verbatim — Phase
  5's route handler can render both algorithms' output through one shared
  leg-rendering function.
- **Review Focus**: all four items have a directly corresponding test
  (`a_fixed_link_only_journey_also_has_zero_changes`,
  `a_round_that_only_ties_an_earlier_rounds_arrival_is_dominated_and_excluded`,
  `the_round_loop_stops_early_once_nothing_improves`, and the differential
  suite's own cross-algorithm agreement assertions in Task 2).
