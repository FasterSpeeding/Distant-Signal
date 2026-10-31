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
//! A fourth, [`Restrictions::with_operator_rule`], keeps the searches to
//! some operators, or away from some ([`OperatorRule`]; the planner
//! operator filtering spec, docs/superpowers/specs/2026-10-31-planner-
//! operator-filtering-design.md, §6.3). It reads each connection's
//! `operator` id, one array read, and blocks the connections the rule
//! refuses, so their trains are never boarded. Its first user is the
//! journey alternatives' "same operator" search (mid-journey changes spec
//! 2026-10-22 §17.7), which used to list the allowed trains by UID.
//!
//! A blocked connection must also END the ride on that train: both searches
//! decide "already aboard" by UID alone, so merely skipping the connection
//! would let a passenger who boarded before the avoided station carry on
//! after it as if nothing happened. `train-mcp` hits the same problem and
//! relabels each surviving run of a schedule with a synthetic id; here the
//! searches instead drop the UID from their "aboard" set when they meet a
//! blocked connection, which reports the real UID on every leg unchanged.

use std::collections::{HashMap, HashSet};

use schedule_query::{Connection, FixedLink, OperatorId, OperatorMask, normalize_tiploc};

/// Which operators a search may ride (spec §5.1-5.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperatorMode {
    /// Only these operators.
    Only,
    /// Any operator but these.
    Avoid,
}

/// What an [`OperatorRule`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorScope {
    /// Everything with an operator: CIF connections, transit trips and
    /// attributed fixed links (a `/Trips/plan` operator filter: one
    /// operator, every way of riding it, spec §5.1).
    Everything,
    /// CIF connections only (ATOC codes): transit trips and links always
    /// pass. A ticket's "same operator" search: a ticket valid on one
    /// operator's trains still allows the Tube across London.
    CifTrains,
}

/// An operator filter (spec §6.3). An unknown operator always passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorRule {
    pub mode: OperatorMode,
    pub operators: OperatorMask,
    pub scope: OperatorScope,
}

impl OperatorRule {
    /// Only `operators`, everywhere.
    #[must_use]
    pub fn only(operators: OperatorMask) -> Self {
        Self {
            mode: OperatorMode::Only,
            operators,
            scope: OperatorScope::Everything,
        }
    }

    /// Never `operators`, everywhere.
    #[must_use]
    pub fn avoid(operators: OperatorMask) -> Self {
        Self {
            mode: OperatorMode::Avoid,
            operators,
            scope: OperatorScope::Everything,
        }
    }

    /// This rule, applied to CIF trains only.
    #[must_use]
    pub fn cif_trains_only(mut self) -> Self {
        self.scope = OperatorScope::CifTrains;
        self
    }

    /// Whether something run by `operator` may be ridden.
    #[must_use]
    #[inline]
    pub fn allows(&self, operator: OperatorId) -> bool {
        if operator.is_unknown()
            || (self.scope == OperatorScope::CifTrains && operator.ds_operator().is_some())
        {
            return true;
        }
        match self.mode {
            OperatorMode::Only => self.operators.contains(operator),
            OperatorMode::Avoid => !self.operators.contains(operator),
        }
    }

    /// Whether `link` may be used: walks always, links only under
    /// [`OperatorScope::Everything`].
    #[must_use]
    pub fn allows_link(&self, link: &FixedLink) -> bool {
        link.is_walk() || self.scope == OperatorScope::CifTrains || self.allows(link.operator)
    }
}

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
    /// When set, the operators that may (or may not) be ridden.
    operator_rule: Option<OperatorRule>,
    /// `no_call` or `pass_legs` is non-empty: [`Self::blocks`] has station
    /// rules to check. Without them it skips normalizing both TIPLOCs on
    /// every connection, which cost about a quarter of a restricted
    /// Connection Scan (operator filtering spec §7.2).
    has_station_rules: bool,
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
        let pass_legs: HashMap<String, Vec<PassLeg>> = pass_legs
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
        let has_station_rules = !no_call.is_empty() || !pass_legs.is_empty();
        Self {
            no_interchange,
            no_call,
            pass_legs,
            operator_rule: None,
            has_station_rules,
        }
    }

    /// These restrictions, riding only what `rule` allows.
    #[must_use]
    pub fn with_operator_rule(mut self, rule: OperatorRule) -> Self {
        self.operator_rule = Some(rule);
        self
    }

    /// The operator rule, if any.
    #[must_use]
    pub fn operator_rule(&self) -> Option<&OperatorRule> {
        self.operator_rule.as_ref()
    }

    /// These restrictions without their operator rule (what an empty
    /// result is explained against).
    #[must_use]
    pub fn without_operator_rule(&self) -> Self {
        Self {
            operator_rule: None,
            ..self.clone()
        }
    }

    /// Whether `link` may be walked or ridden under the operator rule.
    #[must_use]
    pub fn allows_link(&self, link: &FixedLink) -> bool {
        self.operator_rule
            .as_ref()
            .is_none_or(|rule| rule.allows_link(link))
    }

    /// These restrictions plus more `no_interchange` and `no_call` TIPLOCs
    /// (normalized here; `no_call` implies `no_interchange` as in
    /// [`Self::new`]). What [`crate::options`] bans to find another route.
    #[must_use]
    pub fn with_extra(
        &self,
        no_interchange: impl IntoIterator<Item = String>,
        no_call: impl IntoIterator<Item = String>,
    ) -> Self {
        let mut copy = self.clone();
        for tiploc in no_call {
            let tiploc = normalize_tiploc(&tiploc).to_string();
            copy.no_interchange.insert(tiploc.clone());
            copy.no_call.insert(tiploc);
        }
        copy.has_station_rules = !copy.no_call.is_empty() || !copy.pass_legs.is_empty();
        copy.no_interchange.extend(
            no_interchange
                .into_iter()
                .map(|t| normalize_tiploc(&t).to_string()),
        );
        copy
    }

    pub fn is_empty(&self) -> bool {
        self.no_interchange.is_empty()
            && self.no_call.is_empty()
            && self.pass_legs.is_empty()
            && self.operator_rule.is_none()
    }

    /// Whether a ride may start or end at `tiploc` (or a walk touch it).
    pub fn allows_interchange(&self, tiploc: &str) -> bool {
        self.no_interchange.is_empty() || !self.no_interchange.contains(normalize_tiploc(tiploc))
    }

    /// Whether `connection` is unusable: it calls at a `no_call` TIPLOC, or
    /// its train runs through an avoided one between its two calls.
    ///
    /// For a train in `pass_legs`, a connection that is not one of its base
    /// legs (a live-overlay replacement joining two calls because the one
    /// between was cancelled) is blocked when any base leg it spans is, and
    /// also -- conservatively -- when it cannot be placed on the train's base
    /// order at all.
    #[inline]
    pub fn blocks(&self, connection: &Connection) -> bool {
        if self
            .operator_rule
            .as_ref()
            .is_some_and(|rule| !rule.allows(connection.operator))
        {
            return true;
        }
        if !self.has_station_rules {
            return false;
        }
        let from = normalize_tiploc(&connection.from_tiploc);
        let to = normalize_tiploc(&connection.to_tiploc);
        if self.no_call.contains(from) || self.no_call.contains(to) {
            return true;
        }
        let Some(legs) = self.pass_legs.get(connection.uid.as_str()) else {
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
            operator: OperatorId::UNKNOWN,
            uid: uid.to_string().into(),
            from_tiploc: from.to_string().into(),
            to_tiploc: to.to_string().into(),
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

    fn atoc(code: &str) -> OperatorId {
        OperatorId::from_atoc(code).unwrap()
    }

    fn run_by(uid: &str, operator: OperatorId) -> Connection {
        Connection {
            operator,
            ..conn(uid, "A", "B")
        }
    }

    fn mask(codes: &[&str]) -> OperatorMask {
        OperatorMask::of(codes.iter().map(|c| OperatorId::from_code(c).unwrap()))
    }

    #[test]
    fn only_blocks_every_other_operator_and_unknown_passes() {
        let r = Restrictions::default().with_operator_rule(OperatorRule::only(mask(&["VT"])));
        assert!(!r.is_empty());
        assert!(!r.blocks(&run_by("U", atoc("VT"))));
        assert!(r.blocks(&run_by("V", atoc("GR"))));
        assert!(!r.blocks(&run_by("W", OperatorId::UNKNOWN)));
        // A transit trip is an operator too, under the default scope.
        let metrolink = OperatorId::ds(schedule_query::DsOperator::Metrolink);
        assert!(r.blocks(&run_by("metrolink:1", metrolink)));
        assert!(r.allows_interchange("A"));
    }

    #[test]
    fn avoid_blocks_only_its_operators() {
        let r =
            Restrictions::default().with_operator_rule(OperatorRule::avoid(mask(&["VT", "LT"])));
        assert!(r.blocks(&run_by("U", atoc("VT"))));
        assert!(r.blocks(&run_by("U", atoc("LT"))));
        assert!(!r.blocks(&run_by("V", atoc("GR"))));
        assert!(!r.blocks(&run_by("W", OperatorId::UNKNOWN)));
    }

    #[test]
    fn cif_trains_scope_lets_transit_and_links_through() {
        let r = Restrictions::default()
            .with_operator_rule(OperatorRule::only(mask(&["GR"])).cif_trains_only());
        let metrolink = OperatorId::ds(schedule_query::DsOperator::Metrolink);
        assert!(!r.blocks(&run_by("metrolink:1", metrolink)));
        assert!(r.blocks(&run_by("U", atoc("VT"))));
        assert!(r.allows_link(&fixed("TUBE", OperatorId::LONDON_UNDERGROUND)));
    }

    fn fixed(mode: &str, operator: OperatorId) -> FixedLink {
        FixedLink {
            mode: mode.to_string(),
            to_crs: "VIC".to_string(),
            minutes: 10,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
            operator,
        }
    }

    #[test]
    fn links_follow_the_rule_but_walks_always_pass() {
        let lu = OperatorId::LONDON_UNDERGROUND;
        let only_vt = Restrictions::default().with_operator_rule(OperatorRule::only(mask(&["VT"])));
        assert!(!only_vt.allows_link(&fixed("TUBE", lu)));
        assert!(!only_vt.allows_link(&fixed("TRANSFER", lu)));
        assert!(only_vt.allows_link(&fixed("WALK", OperatorId::UNKNOWN)));
        assert!(only_vt.allows_link(&fixed("BUS", OperatorId::UNKNOWN)));
        let with_lu =
            Restrictions::default().with_operator_rule(OperatorRule::only(mask(&["VT", "LT"])));
        assert!(with_lu.allows_link(&fixed("TUBE", lu)));
        let avoid_lu =
            Restrictions::default().with_operator_rule(OperatorRule::avoid(mask(&["LT"])));
        assert!(!avoid_lu.allows_link(&fixed("TUBE", lu)));
        assert!(avoid_lu.allows_link(&fixed(
            "METRO",
            OperatorId::ds(schedule_query::DsOperator::Dlr)
        )));
        // No rule: every link.
        assert!(Restrictions::default().allows_link(&fixed("TUBE", lu)));
    }

    #[test]
    fn the_operator_rule_survives_with_extra_and_can_be_dropped() {
        let r = Restrictions::default().with_operator_rule(OperatorRule::only(mask(&["VT"])));
        let extra = r.with_extra([], ["COV".to_string()]);
        assert!(extra.blocks(&run_by("V", atoc("GR"))));
        assert!(extra.blocks(&Connection {
            operator: atoc("VT"),
            ..conn("U", "COV", "B")
        }));
        let dropped = extra.without_operator_rule();
        assert!(!dropped.blocks(&run_by("V", atoc("GR"))));
        assert!(
            dropped.blocks(&conn("U", "COV", "B")),
            "the station rule stays"
        );
        assert!(r.without_operator_rule().is_empty());
    }

    #[test]
    fn an_operator_rule_keeps_both_searches_to_its_trains() {
        // V is faster, U slower; only U's operator allowed.
        let connections = vec![
            Connection {
                operator: atoc("GR"),
                ..timed("V", "A", "D", 10, 20)
            },
            Connection {
                operator: atoc("VT"),
                ..timed("U", "A", "D", 15, 40)
            },
        ];
        let r = Restrictions::default().with_operator_rule(OperatorRule::only(mask(&["VT"])));
        assert_eq!(
            both(&connections, &r),
            (vec!["U".to_string()], vec!["U".to_string()])
        );
        let r = Restrictions::default().with_operator_rule(OperatorRule::avoid(mask(&["VT"])));
        assert_eq!(
            both(&connections, &r),
            (vec!["V".to_string()], vec!["V".to_string()])
        );
        // An empty restriction changes nothing.
        assert_eq!(
            both(&connections, &Restrictions::default()),
            (vec!["V".to_string()], vec!["V".to_string()])
        );
    }

    #[test]
    fn station_rules_still_apply_without_an_operator_rule() {
        // The fast path must not skip a no-call station.
        let r = Restrictions::new([], ["B".to_string()], HashMap::new());
        assert!(r.blocks(&conn("U", "A", "B")));
        let extra = Restrictions::default().with_extra([], ["B".to_string()]);
        assert!(extra.blocks(&conn("U", "A", "B")));
        let change_only = Restrictions::new(["B".to_string()], [], HashMap::new());
        assert!(!change_only.blocks(&conn("U", "A", "B")));
        assert!(!change_only.allows_interchange("B"));
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
            modal_change: schedule_query::ModalChangeBuffer::default(),
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
            modal_change: schedule_query::ModalChangeBuffer::default(),
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
