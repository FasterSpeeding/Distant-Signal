//! Pass-through vias (`/Trips/plan`'s `?via=`): stations the journey must
//! physically pass through, in order. Staying aboard a train that runs
//! through without stopping counts, and so does a train calling there, or
//! the traveller changing or walking there. This is the `via` of Skye's
//! `train-mcp` and the Distant-Signal-MCP (`src/timetable/plan/constraints.ts`):
//! "Stations the route must pass through, in order -- stopping there or not."
//!
//! Unlike there (a chain of sub-searches, each to "wherever a train that
//! touched the via next stops", re-run past decoys by `legsTouchTarget`),
//! the vias are part of the search state. [`crate::staged`] keeps a VIA
//! PROGRESS next to the waypoint stage: progress `v` means "the first `v`
//! vias have been passed". A ride advances it over each connection, using
//! [`Vias::advance`], so the progress is exact for the train actually
//! ridden and there are no decoys to retry. Vias and waypoints are two
//! independent ordered lists: each list's own order is kept, but a via may
//! fall before, between or after any of the waypoints.
//!
//! **What a connection passes.** A connection names only its two calls.
//! The untimed rows between them are `schedule_query::PassIndex`'s; the
//! caller turns them into [`PassSpan`]s: for every train that calls at or
//! passes a via, its base connections in order, each with the via TIPLOCs
//! it runs through. A live-overlay replacement connection (two calls joined
//! because the one between was cancelled) passes everything its base spans
//! pass, plus the cancelled calls themselves: the train still runs through
//! them. One that cannot be placed on the train's base order passes nothing
//! but its own two ends, conservatively.
//!
//! **Timing points only.** CIF records a pass only at a timing point (a
//! junction, or a station the timetable times trains through). A train
//! running through a station that is not a timing point leaves no row at
//! all, so passing it cannot be seen; it satisfies a via there only by
//! calling. Within one span between two calls, the passing rows are taken
//! in their recorded order.

use std::collections::{HashMap, HashSet};

use schedule_query::{Connection, normalize_tiploc};

/// One base connection of a train relevant to a via, in the train's order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassSpan {
    pub from_tiploc: String,
    pub to_tiploc: String,
    /// The base connection's departure: tells apart two visits of a train
    /// to the same pair of calls.
    pub departure_min: u32,
    /// The TIPLOCs the train runs through between the two calls, in order.
    /// Only via TIPLOCs matter; the caller may leave the others out.
    pub passed: Vec<String>,
}

/// How a via was satisfied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViaHow {
    /// The train called there: the traveller stayed aboard, boarded or
    /// alighted.
    Call,
    /// The train ran through without calling (a timing-point pass, or a
    /// call cancelled by the live overlay).
    Pass,
    /// The traveller walked (or took a fixed link) into it.
    Walk,
}

/// Which leg of a [`crate::StagedJourney`] satisfied a via.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViaLeg {
    /// Index into `StagedJourney::parts`.
    pub part: usize,
    /// Index into that part's `legs`.
    pub leg: usize,
    pub how: ViaHow,
    /// The via TIPLOC (normalized) the leg called at, ran through or walked
    /// into: which of the via's alternatives satisfied it (2026-10-07, OR
    /// groups). Empty only in the unreachable fallback.
    pub tiploc: String,
}

/// The ordered vias of one request. See the module doc.
#[derive(Debug, Clone, Default)]
pub struct Vias {
    /// `targets[v]`: via `v`'s TIPLOCs, normalized.
    targets: Vec<HashSet<String>>,
    /// Per train UID, its base connections in order (see [`PassSpan`]).
    spans: HashMap<String, Vec<PassSpan>>,
}

impl Vias {
    /// `targets[v]` is every TIPLOC via `v` covers; `spans` is keyed on
    /// train UID. All TIPLOCs are normalized here.
    pub fn new(targets: &[Vec<String>], spans: HashMap<String, Vec<PassSpan>>) -> Self {
        let norm = |t: &String| normalize_tiploc(t).to_string();
        Self {
            targets: targets
                .iter()
                .map(|tiplocs| tiplocs.iter().map(norm).collect())
                .collect(),
            spans: spans
                .into_iter()
                .map(|(uid, spans)| {
                    let spans = spans
                        .iter()
                        .map(|span| PassSpan {
                            from_tiploc: norm(&span.from_tiploc),
                            to_tiploc: norm(&span.to_tiploc),
                            departure_min: span.departure_min,
                            passed: span.passed.iter().map(norm).collect(),
                        })
                        .collect();
                    (uid, spans)
                })
                .collect(),
        }
    }

    pub fn len(&self) -> usize {
        self.targets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// The same vias with via `index` left out (for explaining an empty
    /// plan: is that via the one that cannot be passed?).
    #[must_use]
    pub fn without(&self, index: usize) -> Self {
        let mut copy = self.clone();
        if index < copy.targets.len() {
            copy.targets.remove(index);
        }
        copy
    }

    /// The progress after being at `tiploc` (normalized) with progress `v`:
    /// every via in a row that covers it is passed.
    pub(crate) fn advance_at(&self, mut v: usize, tiploc: &str) -> usize {
        while v < self.targets.len() && self.targets[v].contains(tiploc) {
            v += 1;
        }
        v
    }

    /// The progress after riding `connection` with progress `v`: its
    /// departure call, then everything it runs through, then its arrival
    /// call, matched in order.
    pub(crate) fn advance(&self, v: usize, connection: &Connection) -> usize {
        self.walk(v, connection, |_, _, _| {})
    }

    /// The vias `connection` satisfies when ridden from progress `v`, how,
    /// and at which TIPLOC.
    pub(crate) fn hits(&self, v: usize, connection: &Connection) -> Vec<(usize, ViaHow, String)> {
        let mut hits = Vec::new();
        self.walk(v, connection, |via, how, tiploc| {
            hits.push((via, how, tiploc.to_string()));
        });
        hits
    }

    fn walk(
        &self,
        mut v: usize,
        connection: &Connection,
        mut hit: impl FnMut(usize, ViaHow, &str),
    ) -> usize {
        let n = self.targets.len();
        if v >= n {
            return v;
        }
        let mut at = |v: &mut usize, tiploc: &str, how: ViaHow| {
            while *v < n && self.targets[*v].contains(tiploc) {
                hit(*v, how, tiploc);
                *v += 1;
            }
        };
        let from = normalize_tiploc(&connection.from_tiploc);
        let to = normalize_tiploc(&connection.to_tiploc);
        at(&mut v, from, ViaHow::Call);
        if v < n
            && let Some(spans) = self.spans.get(&connection.uid)
            && let Some(covered) = covered_spans(spans, connection, from, to)
        {
            for (index, span) in covered.iter().enumerate() {
                for passed in &span.passed {
                    at(&mut v, passed, ViaHow::Pass);
                }
                // A call the connection skips (a live replacement over a
                // cancelled call) is run through.
                if index + 1 < covered.len() {
                    at(&mut v, &span.to_tiploc, ViaHow::Pass);
                }
            }
        }
        at(&mut v, to, ViaHow::Call);
        v
    }
}

/// The base spans `connection` covers: the base connection itself when it
/// is one (matched on its departure too, as a train may visit the same pair
/// of calls twice), otherwise the shortest run of spans from a call at
/// `from` to a call at `to` (a live replacement; its times have moved).
fn covered_spans<'s>(
    spans: &'s [PassSpan],
    connection: &Connection,
    from: &str,
    to: &str,
) -> Option<&'s [PassSpan]> {
    if let Some(index) = spans.iter().position(|span| {
        span.from_tiploc == from
            && span.to_tiploc == to
            && span.departure_min == connection.departure_min
    }) {
        return Some(&spans[index..=index]);
    }
    spans
        .iter()
        .enumerate()
        .filter(|(_, span)| span.from_tiploc == from)
        .filter_map(|(start, _)| {
            spans[start..]
                .iter()
                .position(|span| span.to_tiploc == to)
                .map(|length| &spans[start..=start + length])
        })
        .min_by_key(|run| run.len())
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

    fn span(from: &str, to: &str, passed: &[&str]) -> PassSpan {
        PassSpan {
            from_tiploc: from.to_string(),
            to_tiploc: to.to_string(),
            departure_min: 0,
            passed: passed.iter().map(|p| (*p).to_string()).collect(),
        }
    }

    fn vias(targets: &[&str]) -> Vias {
        // U runs A -> B (passing X) -> C (passing Y, then Z) -> D.
        Vias::new(
            &targets
                .iter()
                .map(|t| vec![(*t).to_string()])
                .collect::<Vec<_>>(),
            HashMap::from([(
                "U".to_string(),
                vec![
                    span("A", "B", &["X "]),
                    span("B", "C", &["Y", "Z"]),
                    span("C", "D", &[]),
                ],
            )]),
        )
    }

    #[test]
    fn calls_and_passes_advance_in_order() {
        let v = vias(&["X", "B", "Z"]);
        assert_eq!(v.advance(0, &conn("U", "A", "B")), 2);
        assert_eq!(v.advance(2, &conn("U", "B", "C")), 3);
        assert_eq!(
            v.hits(0, &conn("U", "A", "B")),
            vec![
                (0, ViaHow::Pass, "X".to_string()),
                (1, ViaHow::Call, "B".to_string())
            ]
        );
        // Another train over the same calls passes nothing.
        assert_eq!(v.advance(0, &conn("W", "A", "B")), 0);
        // Out of order: Z before X is not progress on [X, ...] past X.
        let v = vias(&["Z", "X"]);
        assert_eq!(v.advance(0, &conn("U", "A", "B")), 0);
    }

    #[test]
    fn a_replacement_runs_through_the_cancelled_call_and_its_passes() {
        let v = vias(&["X", "B", "Y"]);
        // A -> C replaces A -> B -> C (B cancelled): X, B, Y all run through.
        assert_eq!(
            v.hits(0, &conn("U", "A", "C")),
            vec![
                (0, ViaHow::Pass, "X".to_string()),
                (1, ViaHow::Pass, "B".to_string()),
                (2, ViaHow::Pass, "Y".to_string())
            ]
        );
        // Cannot be placed: only its own ends count.
        assert_eq!(v.advance(0, &conn("U", "Q", "C")), 0);
        assert_eq!(v.advance(0, &conn("U", "X", "C")), 1);
    }

    #[test]
    fn a_train_calling_twice_at_the_same_place_is_matched_by_departure() {
        // U runs A -> B (passing X) -> A -> B (passing nothing).
        let mut spans = vec![
            span("A", "B", &["X"]),
            span("B", "A", &[]),
            span("A", "B", &[]),
        ];
        spans[1].departure_min = 10;
        spans[2].departure_min = 20;
        let v = Vias::new(
            &[vec!["X".to_string()]],
            HashMap::from([("U".to_string(), spans)]),
        );
        let mut second = conn("U", "A", "B");
        second.departure_min = 20;
        assert_eq!(v.advance(0, &conn("U", "A", "B")), 1);
        assert_eq!(v.advance(0, &second), 0);
        // A replacement (B cancelled) takes the shortest run.
        let mut replacement = conn("U", "A", "A");
        replacement.departure_min = 3;
        assert_eq!(v.advance(0, &replacement), 1);
    }

    #[test]
    fn a_location_satisfies_every_via_in_a_row_it_covers() {
        let v = Vias::new(
            &[
                vec!["P".to_string()],
                vec!["P".to_string(), "Q".to_string()],
            ],
            HashMap::new(),
        );
        assert_eq!(v.advance_at(0, "P"), 2);
        assert_eq!(v.advance_at(1, "Q"), 2);
        assert_eq!(v.advance_at(0, "Q"), 0);
    }

    /// An OR group (2026-10-07): one via whose targets are several
    /// stations' TIPLOCs is passed by ANY of them, reports which, and is
    /// still one step of progress.
    #[test]
    fn a_group_via_is_passed_by_any_member() {
        let group = |members: &[&str]| -> Vec<String> {
            members.iter().map(|m| (*m).to_string()).collect()
        };
        let v = Vias::new(
            &[group(&["Q", "Y", "D"]), group(&["P", "Z"])],
            HashMap::from([(
                "U".to_string(),
                vec![
                    span("A", "B", &["X"]),
                    span("B", "C", &["Y", "Z"]),
                    span("C", "D", &[]),
                ],
            )]),
        );
        assert_eq!(v.len(), 2);
        // Y (a pass) satisfies the first group, then Z the second.
        assert_eq!(
            v.hits(0, &conn("U", "B", "C")),
            vec![
                (0, ViaHow::Pass, "Y".to_string()),
                (1, ViaHow::Pass, "Z".to_string())
            ]
        );
        // D (a call) satisfies the first group; the second is still due.
        assert_eq!(
            v.hits(0, &conn("U", "C", "D")),
            vec![(0, ViaHow::Call, "D".to_string())]
        );
        // Q, by arriving or changing there.
        assert_eq!(v.advance_at(0, "Q"), 1);
        // No member: no progress.
        assert_eq!(v.advance(0, &conn("U", "A", "B")), 0);
    }
}
