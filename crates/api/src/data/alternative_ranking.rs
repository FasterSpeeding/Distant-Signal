//! Ranking the alternatives offered when a journey breaks
//! (docs/superpowers/specs/2026-10-22-mid-journey-changes-design.md §17,
//! phases R0 and R1). Pure: no I/O, tested with tables.
//!
//! Three steps:
//!
//! 1. [`ticket_constraint`]: what the tickets on the replaced legs allow,
//!    guessed from their free-text `ticket_type` and operator (R0 has no
//!    structured fare data): train-specific (Advance), one operator only,
//!    any permitted route, or unknown.
//! 2. [`entitlement`]: what the break entitles the traveller to: a missed
//!    connection (the railway's fault), a cancelled booked train, the
//!    passenger's own miss, or a free choice.
//! 3. [`ticket_fit`] per alternative, then [`rank`]: a generalised cost in
//!    minutes picks the Recommended option, and up to two more cards carry
//!    the picks it does not win ("Fastest", "Fewest changes", "Same
//!    operator", "No Tube", "Keeps your later trains"), each card with its
//!    fit badge. Options are flagged, never hidden, for fit (§17.10, 3).
//!
//! DS never says a ticket *is* valid (§11): the strongest wording is
//! "usually covered by your ticket", and every fit carries a plain-words
//! reason.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

/// A ticket's fare class, guessed from its free-text type (§17.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FareClass {
    Advance,
    OffPeak,
    SuperOffPeak,
    Anytime,
    Season,
    Other,
    Unknown,
}

/// [`FareClass`] from a ticket's `ticket_type` text ("Advance Single",
/// "Off-Peak Day Return", "Anytime Day Single", "7 Day Season").
pub fn guess_fare_class(ticket_type: Option<&str>) -> FareClass {
    let Some(text) = ticket_type.map(str::trim).filter(|t| !t.is_empty()) else {
        return FareClass::Unknown;
    };
    let lower = text.to_lowercase().replace(['-', '_'], " ");
    if lower.contains("advance") {
        FareClass::Advance
    } else if lower.contains("super off peak") || lower.contains("super offpeak") {
        FareClass::SuperOffPeak
    } else if lower.contains("off peak") || lower.contains("offpeak") || lower.contains("saver") {
        FareClass::OffPeak
    } else if lower.contains("anytime") || lower.contains("open") {
        FareClass::Anytime
    } else if lower.contains("season") || lower.contains("flexi") {
        FareClass::Season
    } else {
        FareClass::Other
    }
}

/// One ticket on a replaced leg, as ranking reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketInfo {
    pub ticket_type: Option<String>,
    /// The ticket's own operator as an ATOC code, when its free text names
    /// a known one.
    pub operator_atoc: Option<String>,
}

/// What a journey's tickets allow (§17.3), most restrictive first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketConstraint {
    /// Advance: the booked train(s) only, operator by operator.
    TrainSpecific { operators: Vec<String> },
    /// "X only" on the ticket: that operator's trains.
    OperatorOnly { operators: Vec<String> },
    /// Off-Peak, Anytime, a season: any permitted route.
    AnyPermitted { time_restricted: bool },
    /// No ticket, or nothing DS can read from it.
    Unknown,
}

/// The constraint of `tickets` (the replaced legs'), most restrictive
/// winning; `booked_operators` are the replaced trains' own operators, the
/// fallback for an Advance whose ticket names none.
pub fn ticket_constraint(tickets: &[TicketInfo], booked_operators: &[String]) -> TicketConstraint {
    let mut best = TicketConstraint::Unknown;
    for ticket in tickets {
        let class = guess_fare_class(ticket.ticket_type.as_deref());
        let operators: Vec<String> = match &ticket.operator_atoc {
            Some(code) => vec![code.clone()],
            None => booked_operators.to_vec(),
        };
        let only = ticket
            .ticket_type
            .as_deref()
            .is_some_and(|t| t.to_lowercase().split_whitespace().any(|w| w == "only"));
        let constraint = match class {
            FareClass::Advance => TicketConstraint::TrainSpecific { operators },
            _ if only && !operators.is_empty() => TicketConstraint::OperatorOnly { operators },
            FareClass::OffPeak | FareClass::SuperOffPeak => TicketConstraint::AnyPermitted {
                time_restricted: true,
            },
            FareClass::Anytime | FareClass::Season => TicketConstraint::AnyPermitted {
                time_restricted: false,
            },
            FareClass::Other | FareClass::Unknown => TicketConstraint::Unknown,
        };
        if strictness(&constraint) > strictness(&best) {
            best = constraint;
        }
    }
    best
}

fn strictness(constraint: &TicketConstraint) -> u8 {
    match constraint {
        TicketConstraint::Unknown => 0,
        TicketConstraint::AnyPermitted { .. } => 1,
        TicketConstraint::OperatorOnly { .. } => 2,
        TicketConstraint::TrainSpecific { .. } => 3,
    }
}

/// What the break entitles the traveller to (§17.3, step 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entitlement {
    /// A connection missed because of the railway (`NRCoT` 9.4): a missed
    /// connection, a part-cancelled or skipped train, or "I missed it" when
    /// the inbound was late.
    MissedConnection,
    /// The booked train was cancelled (28.3 and ticket acceptance).
    Cancelled { booked_departure: DateTime<Utc> },
    /// The passenger missed the train themselves.
    PassengerFault,
    /// A change with no break.
    Choice,
}

/// [`Entitlement`] from a break's `cause` and `fault` (§4.2).
pub fn entitlement(
    cause: &str,
    fault: &str,
    booked_departure: Option<DateTime<Utc>>,
) -> Entitlement {
    match (cause, fault, booked_departure) {
        ("cancelled", _, Some(booked_departure)) => Entitlement::Cancelled { booked_departure },
        ("user_choice", ..) => Entitlement::Choice,
        ("user_missed", "user", _) => Entitlement::PassengerFault,
        _ => Entitlement::MissedConnection,
    }
}

/// How well one alternative fits the ticket (§17.3, step 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Fit {
    Covered,
    CoveredIfCancelled,
    Check,
    LikelyNewTicket,
}

impl Fit {
    /// The badge text.
    pub fn label(self) -> &'static str {
        match self {
            Fit::Covered => "Usually covered by your ticket",
            Fit::CoveredIfCancelled => "Accepted when your train is cancelled",
            Fit::Check => "Check your ticket",
            Fit::LikelyNewTicket => "May need a new ticket",
        }
    }

    fn penalty(self) -> i64 {
        match self {
            Fit::Covered | Fit::CoveredIfCancelled => 0,
            Fit::Check => 15,
            Fit::LikelyNewTicket => 90,
        }
    }
}

/// One train leg of an alternative, as fit reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateTrain {
    pub operator: Option<String>,
    pub departure: DateTime<Utc>,
}

/// One alternative, as ranking reads it (§17.4's signals).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub departure: DateTime<Utc>,
    /// Live (else timetabled) arrival at the final destination.
    pub arrival: DateTime<Utc>,
    pub changes: u32,
    /// `TUBE`, `BUS` and `METRO` transfers.
    pub tube_or_bus_transfers: u32,
    pub walk_minutes: u32,
    /// The tightest change's spare minutes after the minimum change time.
    pub min_slack_minutes: Option<i64>,
    /// Keeps a later booked leg (§5.5).
    pub rejoins: bool,
    pub trains: Vec<CandidateTrain>,
}

/// One of the badged picks (§17.6), in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Pick {
    Recommended,
    Fastest,
    FewestChanges,
    SameOperator,
    NoTube,
    Rejoin,
}

impl Pick {
    pub fn label(self) -> &'static str {
        match self {
            Pick::Recommended => "Recommended",
            Pick::Fastest => "Fastest",
            Pick::FewestChanges => "Fewest changes",
            Pick::SameOperator => "Same operator",
            Pick::NoTube => "No Tube",
            Pick::Rejoin => "Keeps your later trains",
        }
    }

    /// The `pick` label of `api_journey_replan_accepted_total` (§17.9).
    pub fn metric_label(self) -> &'static str {
        match self {
            Pick::Recommended => "recommended",
            Pick::Fastest => "fastest",
            Pick::FewestChanges => "fewest_changes",
            Pick::SameOperator => "same_operator",
            Pick::NoTube => "no_tube",
            Pick::Rejoin => "rejoin",
        }
    }
}

/// The weights of the generalised cost (§17.4), in minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Weights {
    pub change: i64,
    pub transfer: i64,
    pub tight_slack: i64,
    pub rejoin_bonus: i64,
    /// With no ticket on a railway break, a same-operator train wins ties.
    pub unknown_same_operator_bonus: i64,
    /// "Fastest" is shown only when it arrives this much earlier.
    pub fastest_margin_minutes: i64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            change: 10,
            transfer: 10,
            tight_slack: 10,
            rejoin_bonus: 15,
            unknown_same_operator_bonus: 5,
            fastest_margin_minutes: 15,
        }
    }
}

/// A change with less spare time than this is tight (§5.1).
pub const AT_RISK_SLACK_MINUTES: i64 = 3;

/// How far either side of a cancelled train other operators accept the
/// ticket (the ticket acceptance policy), and the same operator's window
/// beyond its two trains either side (`NRCoT` 28.3).
const CANCELLED_ACCEPTANCE: Duration = Duration::hours(2);

/// Everything ranking needs besides the candidates.
#[derive(Debug, Clone)]
pub struct Context {
    pub constraint: TicketConstraint,
    pub entitlement: Entitlement,
    /// The replaced train legs' operators ("same operator").
    pub replaced_operators: Vec<String>,
    pub weights: Weights,
}

/// One alternative's ranking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ranked {
    /// Index into the candidates.
    #[serde(skip)]
    pub index: usize,
    pub cost: i64,
    /// `None` when there is no ticket to judge (no badge).
    pub fit: Option<Fit>,
    pub fit_label: Option<&'static str>,
    pub fit_reason: Option<String>,
    pub picks: Vec<Pick>,
    /// One of the (at most three) cards; the rest are "More options".
    pub card: bool,
}

/// The ranked alternatives: the cards first (Recommended, then the picks
/// in §17.6's order), then the rest by cost; plus the sheet's one-line
/// ticket hint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ranking {
    pub order: Vec<Ranked>,
    pub hint: Option<&'static str>,
}

/// At most this many cards (§17.10, 1).
pub const MAX_CARDS: usize = 3;

fn same_operator(candidate: &Candidate, operators: &[String]) -> bool {
    !operators.is_empty()
        && !candidate.trains.is_empty()
        && candidate.trains.iter().all(|train| {
            train
                .operator
                .as_deref()
                .is_some_and(|op| operators.iter().any(|o| o.eq_ignore_ascii_case(op)))
        })
}

fn operator_names(operators: &[String]) -> String {
    operators.join(" or ")
}

/// [`Fit`] of `candidate` with its reason in plain words, `None` with no
/// ticket to judge. `same_operator_rank` is the candidate's place among the
/// same operator's alternatives leaving at or after the cancelled train
/// (0 = the first), for `NRCoT` 28.3's "two trains after".
pub fn ticket_fit(
    constraint: &TicketConstraint,
    entitlement: Entitlement,
    candidate: &Candidate,
    same_operator_rank: Option<usize>,
) -> Option<(Fit, String)> {
    let within_acceptance = |booked: DateTime<Utc>| {
        candidate
            .trains
            .first()
            .is_some_and(|t| (t.departure - booked).abs() <= CANCELLED_ACCEPTANCE)
    };
    match constraint {
        TicketConstraint::Unknown => None,
        TicketConstraint::AnyPermitted { time_restricted } => Some(match entitlement {
            Entitlement::PassengerFault | Entitlement::Choice if *time_restricted => (
                Fit::Check,
                "Your ticket may have time restrictions. Check before you board.".to_string(),
            ),
            _ => (
                Fit::Covered,
                "Your ticket is usually valid on any permitted route.".to_string(),
            ),
        }),
        TicketConstraint::TrainSpecific { operators }
        | TicketConstraint::OperatorOnly { operators } => {
            let advance = matches!(constraint, TicketConstraint::TrainSpecific { .. });
            let same = same_operator(candidate, operators);
            let names = operator_names(operators);
            Some(match entitlement {
                Entitlement::MissedConnection if same => (
                    Fit::Covered,
                    format!(
                        "Your ticket lets you take the next {names} train when a connection is missed."
                    ),
                ),
                Entitlement::Cancelled { booked_departure } if same => {
                    if !advance
                        || within_acceptance(booked_departure)
                        || same_operator_rank.is_some_and(|rank| rank < 2)
                    {
                        (
                            Fit::Covered,
                            format!(
                                "Your ticket is usually valid on a {names} train near the one that was cancelled."
                            ),
                        )
                    } else {
                        (
                            Fit::LikelyNewTicket,
                            "This train is outside the trains your ticket usually allows after a cancellation."
                                .to_string(),
                        )
                    }
                }
                Entitlement::Cancelled { booked_departure } if within_acceptance(booked_departure) => (
                    Fit::CoveredIfCancelled,
                    "Other operators usually accept your ticket within 2 hours of a cancelled train."
                        .to_string(),
                ),
                Entitlement::PassengerFault if advance => (
                    Fit::LikelyNewTicket,
                    "Your Advance ticket is only valid on the train you missed.".to_string(),
                ),
                Entitlement::PassengerFault | Entitlement::Choice if same => (
                    Fit::Check,
                    "Your ticket may have time restrictions. Check before you board.".to_string(),
                ),
                Entitlement::Choice if advance => (
                    Fit::LikelyNewTicket,
                    "Your Advance ticket is only valid on the train you booked.".to_string(),
                ),
                _ => (
                    Fit::LikelyNewTicket,
                    format!("Your ticket is for {names} trains only."),
                ),
            })
        }
    }
}

fn arrival_minutes(candidate: &Candidate, epoch: DateTime<Utc>) -> i64 {
    (candidate.arrival - epoch).num_minutes()
}

/// The sheet's one-line hint (§17.5, §8.3), if any.
fn hint(context: &Context) -> Option<&'static str> {
    match (&context.constraint, context.entitlement) {
        (TicketConstraint::TrainSpecific { .. }, Entitlement::PassengerFault) => Some(
            "Your Advance ticket is only valid on the train you missed. You'll likely need a new ticket.",
        ),
        (TicketConstraint::Unknown, Entitlement::PassengerFault) => {
            Some("If you have an Advance ticket, you'll likely need a new one.")
        }
        (_, Entitlement::PassengerFault) => {
            Some("Your ticket may not be valid on a different train. Check before you board.")
        }
        (TicketConstraint::Unknown, _) => Some("Add your ticket to see which trains it covers."),
        _ => None,
    }
}

/// Ranks `candidates` (§17.4 to §17.6). An empty list ranks empty.
#[expect(
    clippy::too_many_lines,
    reason = "long but linear: cost, then each pick in §17.6's order, then the cards"
)]
pub fn rank(candidates: &[Candidate], context: &Context) -> Ranking {
    let weights = context.weights;
    let Some(epoch) = candidates.iter().map(|c| c.arrival).min() else {
        return Ranking {
            order: Vec::new(),
            hint: hint(context),
        };
    };
    let ticket_operators: &[String] = match &context.constraint {
        TicketConstraint::TrainSpecific { operators }
        | TicketConstraint::OperatorOnly { operators } => operators,
        _ => &[],
    };
    // Each candidate's place among the ticket operator's options leaving at
    // or after the cancelled train, for 28.3's "two trains after".
    let mut same_operator_ranks: Vec<Option<usize>> = vec![None; candidates.len()];
    if let Entitlement::Cancelled { booked_departure } = context.entitlement {
        let mut after: Vec<(DateTime<Utc>, usize)> = candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| same_operator(c, ticket_operators))
            .filter_map(|(i, c)| {
                let first = c.trains.first()?.departure;
                (first >= booked_departure).then_some((first, i))
            })
            .collect();
        after.sort();
        for (rank, (_, index)) in after.into_iter().enumerate() {
            same_operator_ranks[index] = Some(rank);
        }
    }
    let railway_break = matches!(
        context.entitlement,
        Entitlement::MissedConnection | Entitlement::Cancelled { .. }
    );

    let mut ranked: Vec<Ranked> = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            let fit = ticket_fit(
                &context.constraint,
                context.entitlement,
                candidate,
                same_operator_ranks[index],
            );
            let mut cost = arrival_minutes(candidate, epoch)
                + weights.change * i64::from(candidate.changes)
                + weights.transfer * i64::from(candidate.tube_or_bus_transfers)
                + i64::from(candidate.walk_minutes)
                + fit.as_ref().map_or(0, |(fit, _)| fit.penalty());
            if candidate
                .min_slack_minutes
                .is_some_and(|slack| slack < AT_RISK_SLACK_MINUTES)
            {
                cost += weights.tight_slack;
            }
            if candidate.rejoins {
                cost -= weights.rejoin_bonus;
            }
            if context.constraint == TicketConstraint::Unknown
                && railway_break
                && same_operator(candidate, &context.replaced_operators)
            {
                cost -= weights.unknown_same_operator_bonus;
            }
            let (fit, fit_reason) =
                fit.map_or((None, None), |(fit, reason)| (Some(fit), Some(reason)));
            Ranked {
                index,
                cost,
                fit,
                fit_label: fit.map(Fit::label),
                fit_reason,
                picks: Vec::new(),
                card: false,
            }
        })
        .collect();
    // By cost, then earlier arrival, then fewer changes, then input order.
    ranked.sort_by_key(|r| {
        let c = &candidates[r.index];
        (r.cost, c.arrival, c.changes, r.index)
    });

    let recommended = ranked[0].index;
    let rec = &candidates[recommended];
    let mut winners: Vec<(Pick, usize)> = vec![(Pick::Recommended, recommended)];
    let best_by = |pred: &dyn Fn(&Candidate) -> bool| -> Option<usize> {
        ranked
            .iter()
            .map(|r| r.index)
            .filter(|&i| pred(&candidates[i]))
            .min_by_key(|&i| (candidates[i].arrival, candidates[i].changes))
    };
    if let Some(fastest) = best_by(&|_| true)
        && fastest != recommended
        && (rec.arrival - candidates[fastest].arrival).num_minutes()
            >= weights.fastest_margin_minutes
    {
        winners.push((Pick::Fastest, fastest));
    }
    if let Some(fewest) = ranked
        .iter()
        .map(|r| r.index)
        .min_by_key(|&i| (candidates[i].changes, candidates[i].arrival))
        && candidates[fewest].changes < rec.changes
    {
        winners.push((Pick::FewestChanges, fewest));
    }
    let same_operator_shown = matches!(
        context.constraint,
        TicketConstraint::TrainSpecific { .. }
            | TicketConstraint::OperatorOnly { .. }
            | TicketConstraint::Unknown
    );
    let operators_for_same: &[String] = if ticket_operators.is_empty() {
        &context.replaced_operators
    } else {
        ticket_operators
    };
    if same_operator_shown
        && let Some(same) = best_by(&|c| same_operator(c, operators_for_same))
        && same != recommended
    {
        winners.push((Pick::SameOperator, same));
    }
    if rec.tube_or_bus_transfers > 0 {
        // The lowest-cost option with no Tube or bus.
        if let Some(no_tube) = ranked
            .iter()
            .map(|r| r.index)
            .find(|&i| candidates[i].tube_or_bus_transfers == 0)
        {
            winners.push((Pick::NoTube, no_tube));
        }
    }
    if let Some(rejoin) = ranked
        .iter()
        .map(|r| r.index)
        .find(|&i| candidates[i].rejoins)
        && rejoin != recommended
    {
        winners.push((Pick::Rejoin, rejoin));
    }

    // Cards: Recommended, then each pick's winner in order, merging picks
    // won by an itinerary that is already a card, up to MAX_CARDS.
    let mut cards: Vec<usize> = Vec::new();
    for (pick, index) in winners {
        let position = ranked
            .iter()
            .position(|r| r.index == index)
            .unwrap_or_default();
        if cards.contains(&index) {
            ranked[position].picks.push(pick);
        } else if cards.len() < MAX_CARDS {
            cards.push(index);
            ranked[position].picks.push(pick);
            ranked[position].card = true;
        }
    }
    let mut order: Vec<Ranked> = Vec::with_capacity(ranked.len());
    for index in &cards {
        if let Some(position) = ranked.iter().position(|r| r.index == *index) {
            order.push(ranked.remove(position));
        }
    }
    order.extend(ranked);
    Ranking {
        order,
        hint: hint(context),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(hour: u32, minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 22, hour, minute, 0).unwrap()
    }

    fn candidate(dep: (u32, u32), arr: (u32, u32), changes: u32, operators: &[&str]) -> Candidate {
        Candidate {
            departure: at(dep.0, dep.1),
            arrival: at(arr.0, arr.1),
            changes,
            tube_or_bus_transfers: 0,
            walk_minutes: 0,
            min_slack_minutes: None,
            rejoins: false,
            trains: operators
                .iter()
                .map(|op| CandidateTrain {
                    operator: Some((*op).to_string()),
                    departure: at(dep.0, dep.1),
                })
                .collect(),
        }
    }

    fn context(constraint: TicketConstraint, entitlement: Entitlement) -> Context {
        Context {
            constraint,
            entitlement,
            replaced_operators: vec!["GR".to_string()],
            weights: Weights::default(),
        }
    }

    fn advance() -> TicketConstraint {
        TicketConstraint::TrainSpecific {
            operators: vec!["GR".to_string()],
        }
    }

    #[test]
    fn fare_classes_are_guessed_from_ticket_text() {
        for (text, class) in [
            (Some("Advance Single"), FareClass::Advance),
            (Some("ADVANCE STD"), FareClass::Advance),
            (Some("Off-Peak Day Return"), FareClass::OffPeak),
            (Some("Super Off-Peak Single"), FareClass::SuperOffPeak),
            (Some("Anytime Day Single"), FareClass::Anytime),
            (Some("7 Day Season"), FareClass::Season),
            (Some("Flexi Season"), FareClass::Season),
            (Some("Group Save"), FareClass::Other),
            (Some("  "), FareClass::Unknown),
            (None, FareClass::Unknown),
        ] {
            assert_eq!(guess_fare_class(text), class, "{text:?}");
        }
    }

    #[test]
    fn the_most_restrictive_ticket_wins_and_an_advance_falls_back_to_the_booked_operator() {
        let tickets = [
            TicketInfo {
                ticket_type: Some("Anytime Return".to_string()),
                operator_atoc: None,
            },
            TicketInfo {
                ticket_type: Some("Advance Single".to_string()),
                operator_atoc: None,
            },
        ];
        assert_eq!(
            ticket_constraint(&tickets, &["GR".to_string()]),
            TicketConstraint::TrainSpecific {
                operators: vec!["GR".to_string()]
            }
        );
        let only = [TicketInfo {
            ticket_type: Some("Off-Peak Return LNER only".to_string()),
            operator_atoc: Some("GR".to_string()),
        }];
        assert_eq!(
            ticket_constraint(&only, &[]),
            TicketConstraint::OperatorOnly {
                operators: vec!["GR".to_string()]
            }
        );
        assert_eq!(ticket_constraint(&[], &[]), TicketConstraint::Unknown);
    }

    #[test]
    fn entitlement_follows_cause_and_fault() {
        assert_eq!(
            entitlement("cancelled", "operator", Some(at(9, 0))),
            Entitlement::Cancelled {
                booked_departure: at(9, 0)
            }
        );
        assert_eq!(
            entitlement("user_missed", "user", None),
            Entitlement::PassengerFault
        );
        // The data decides: a late inbound makes it the railway's.
        assert_eq!(
            entitlement("user_missed", "operator", None),
            Entitlement::MissedConnection
        );
        assert_eq!(
            entitlement("connection_missed", "operator", None),
            Entitlement::MissedConnection
        );
        assert_eq!(
            entitlement("user_choice", "none", None),
            Entitlement::Choice
        );
    }

    #[test]
    fn ticket_fit_table() {
        let same = candidate((10, 4), (12, 31), 0, &["GR"]);
        let other = candidate((9, 58), (12, 10), 0, &["XC"]);
        let late_other = candidate((13, 0), (15, 0), 0, &["XC"]);
        let fit = |constraint: &TicketConstraint, entitlement, c: &Candidate, rank| {
            ticket_fit(constraint, entitlement, c, rank).map(|(fit, _)| fit)
        };
        let missed = Entitlement::MissedConnection;
        let cancelled = Entitlement::Cancelled {
            booked_departure: at(10, 0),
        };
        let any = TicketConstraint::AnyPermitted {
            time_restricted: true,
        };
        let only = TicketConstraint::OperatorOnly {
            operators: vec!["GR".to_string()],
        };
        let rows = [
            // Advance, missed connection: the next train of its operator.
            (advance(), missed, &same, None, Some(Fit::Covered)),
            (advance(), missed, &other, None, Some(Fit::LikelyNewTicket)),
            // Advance, booked train cancelled.
            (advance(), cancelled, &same, Some(0), Some(Fit::Covered)),
            (
                advance(),
                cancelled,
                &other,
                None,
                Some(Fit::CoveredIfCancelled),
            ),
            (
                advance(),
                cancelled,
                &late_other,
                None,
                Some(Fit::LikelyNewTicket),
            ),
            // Advance, passenger fault: everything needs a new ticket.
            (
                advance(),
                Entitlement::PassengerFault,
                &same,
                None,
                Some(Fit::LikelyNewTicket),
            ),
            // Operator only.
            (only.clone(), missed, &same, None, Some(Fit::Covered)),
            (
                only.clone(),
                missed,
                &other,
                None,
                Some(Fit::LikelyNewTicket),
            ),
            (
                only,
                Entitlement::PassengerFault,
                &same,
                None,
                Some(Fit::Check),
            ),
            // Any permitted.
            (any.clone(), missed, &other, None, Some(Fit::Covered)),
            (
                any,
                Entitlement::PassengerFault,
                &other,
                None,
                Some(Fit::Check),
            ),
            // No ticket: no badge.
            (TicketConstraint::Unknown, missed, &same, None, None),
        ];
        for (constraint, entitlement, c, rank, expected) in rows {
            assert_eq!(
                fit(&constraint, entitlement, c, rank),
                expected,
                "{constraint:?} {entitlement:?} {:?}",
                c.trains
            );
        }
    }

    #[test]
    fn an_advance_cancellation_covers_two_same_operator_trains_after_beyond_two_hours() {
        let cancelled = Entitlement::Cancelled {
            booked_departure: at(8, 0),
        };
        let far = candidate((11, 0), (13, 0), 0, &["GR"]);
        assert_eq!(
            ticket_fit(&advance(), cancelled, &far, Some(1)).map(|f| f.0),
            Some(Fit::Covered)
        );
        assert_eq!(
            ticket_fit(&advance(), cancelled, &far, Some(2)).map(|f| f.0),
            Some(Fit::LikelyNewTicket)
        );
    }

    #[test]
    fn one_minute_faster_with_two_more_changes_loses() {
        let direct = candidate((10, 0), (12, 0), 0, &["XC"]);
        let fiddly = candidate((10, 0), (11, 59), 2, &["XC", "XC", "XC"]);
        let any = TicketConstraint::AnyPermitted {
            time_restricted: false,
        };
        let ranking = rank(
            &[fiddly, direct],
            &context(any, Entitlement::MissedConnection),
        );
        assert_eq!(ranking.order[0].index, 1);
        assert_eq!(ranking.order[0].picks, vec![Pick::Recommended]);
        // Not 15 minutes faster: no Fastest card.
        assert!(
            ranking
                .order
                .iter()
                .all(|r| !r.picks.contains(&Pick::Fastest))
        );
    }

    #[test]
    fn same_operator_two_hours_later_loses_to_a_covered_other_operator_in_the_cancellation_window()
    {
        let same_late = candidate((12, 0), (14, 0), 0, &["GR"]);
        let other_soon = candidate((10, 15), (12, 15), 0, &["XC"]);
        let ranking = rank(
            &[same_late, other_soon],
            &context(
                advance(),
                Entitlement::Cancelled {
                    booked_departure: at(10, 0),
                },
            ),
        );
        assert_eq!(ranking.order[0].index, 1, "{ranking:?}");
        assert_eq!(ranking.order[0].fit, Some(Fit::CoveredIfCancelled));
        // The same operator's train is still offered, badged.
        let same = ranking.order.iter().find(|r| r.index == 0).unwrap();
        assert!(same.picks.contains(&Pick::SameOperator));
        assert!(same.card);
    }

    #[test]
    fn an_advance_missed_connection_recommends_the_next_train_of_its_operator_and_flags_others() {
        let same = candidate((10, 4), (12, 31), 0, &["GR"]);
        let faster = candidate((9, 58), (12, 10), 0, &["XC"]);
        let ranking = rank(
            &[same, faster],
            &context(advance(), Entitlement::MissedConnection),
        );
        assert_eq!(ranking.order[0].index, 0);
        assert_eq!(ranking.order[0].fit, Some(Fit::Covered));
        let other = &ranking.order[1];
        assert_eq!(other.fit, Some(Fit::LikelyNewTicket));
        assert_eq!(other.picks, vec![Pick::Fastest]);
        assert_eq!(other.fit_label, Some("May need a new ticket"));
    }

    #[test]
    fn one_itinerary_winning_several_picks_is_one_card_and_there_are_at_most_three() {
        let recommended = candidate((10, 0), (12, 0), 1, &["XC", "XC"]);
        let fast_and_simple = candidate((10, 0), (11, 40), 0, &["TP"]);
        let mut rejoin = candidate((10, 5), (12, 30), 1, &["XC", "XC"]);
        rejoin.rejoins = false;
        let mut tube = candidate((10, 0), (12, 5), 0, &["XC"]);
        tube.tube_or_bus_transfers = 1;
        let any = TicketConstraint::AnyPermitted {
            time_restricted: false,
        };
        let mut ctx = context(any, Entitlement::MissedConnection);
        // Make "recommended" win on cost with a rejoin bonus.
        let mut rec = recommended;
        rec.rejoins = true;
        ctx.weights.rejoin_bonus = 60;
        let ranking = rank(&[rec, fast_and_simple, rejoin, tube], &ctx);
        assert_eq!(ranking.order[0].index, 0);
        let second = &ranking.order[1];
        assert_eq!(second.index, 1);
        assert_eq!(second.picks, vec![Pick::Fastest, Pick::FewestChanges]);
        assert!(ranking.order.iter().filter(|r| r.card).count() <= MAX_CARDS);
    }

    #[test]
    fn with_no_ticket_a_same_operator_train_wins_a_tie_and_the_sheet_asks_for_the_ticket() {
        let other = candidate((10, 0), (12, 0), 0, &["XC"]);
        let same = candidate((10, 0), (12, 0), 0, &["GR"]);
        let ranking = rank(
            &[other, same],
            &context(TicketConstraint::Unknown, Entitlement::MissedConnection),
        );
        assert_eq!(ranking.order[0].index, 1);
        assert_eq!(ranking.order[0].fit, None);
        assert_eq!(
            ranking.hint,
            Some("Add your ticket to see which trains it covers.")
        );
    }

    #[test]
    fn a_passenger_fault_on_an_advance_says_a_new_ticket_is_likely() {
        let ranking = rank(
            &[candidate((10, 0), (12, 0), 0, &["GR"])],
            &context(advance(), Entitlement::PassengerFault),
        );
        assert_eq!(ranking.order[0].fit, Some(Fit::LikelyNewTicket));
        assert!(ranking.hint.unwrap().contains("likely need a new ticket"));
        assert!(
            rank(&[], &context(advance(), Entitlement::Choice))
                .order
                .is_empty()
        );
    }
}
