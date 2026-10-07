//! A small, explicitly-maintained-in-this-repo Delay Repay ruleset. There
//! is no official API to sync this against (see
//! docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
//! Research summary §4) -- every value here is compiled from a cited
//! source, not guessed at. Last checked: [`RULES_CHECKED_ON`].
//!
//! # Sources (checked 2026-10-07)
//!
//! * Which scheme each operator runs: ORR, "Rail delay compensation
//!   claims, rail periods 11 to 13 (4 January to 31 March 2026)", 25 June
//!   2026, the table "Delay compensation scheme by train operator":
//!   <https://dataportal.orr.gov.uk/media/ittbwvrh/delay-compensation-claims-factsheet-2025-26-rail-periods-11-13.pdf>.
//!   Its Annex 1 defines DR30 (50% of the single at 30-59 minutes, 100% at
//!   60+, the return fare at 120+) and DR15 (DR30 plus 25% at 15-29).
//!   Operators it lists as DR30: Caledonian Sleeper, `CrossCountry`,
//!   Heathrow Express (see below), Hull Trains, LNER, Lumo, `ScotRail`.
//!   "Traditional" (own-scheme): Elizabeth line, Grand Central, London
//!   Overground, Merseyrail. Every other operator is DR15.
//! * Heathrow Express: the ORR lists DR30 "from 1 September 2024", but
//!   its own scheme for tickets bought from that date pays 25% of the
//!   ticket for a delay of more than 30 minutes and 50% for more than 60
//!   (Conditions of Carriage, July 2026,
//!   <https://www.heathrowexpress.com/docs/heathrow-express-conditions-of-carriage-july-2026.pdf>),
//!   modelled here as its own scheme ([`Scheme::HeathrowExpress`]).
//! * Caledonian Sleeper room supplements: 50% at 30-59 minutes, 100% at
//!   60+ (Guest Experience Charter,
//!   <https://www.sleeper.scot/media/fgpdnlsm/caledonian-sleeper-guest-experience-charter-201920-firearms-update-1.pdf>).
//! * Claim pages: each operator's own, linked in [`OPERATORS`]. The LNER
//!   and `ScotRail` pages returned HTTP 403 to an automated check on
//!   2026-10-07, so their entries rest on the ORR table and the earlier
//!   2026-08-29 check, not a fresh read of the operator's own page.
//!
//! The ATOC codes are those in reference-data/toc-codes.csv.
//!
//! STRUCTURAL SAFETY NOTE, not just a comment: every function in this file
//! is pure (no `PgPool`, no I/O of any kind) and is called only from
//! read-only paths -- `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
//! (crates/api/src/routes/train.rs) and `GET /Train/tickets/mine`
//! (`train_tracking::list_tickets_for_user`). This file must never gain a
//! function that writes anywhere, and no future change anywhere in this
//! codebase may wire any function below into a write path without a fresh
//! design-doc pass. This app estimates eligibility and links out; it never
//! submits a claim or asserts proof of travel, full stop.

use serde::Serialize;

/// When the scheme table below was last checked against its sources.
/// `frontend/components/DelayRepayEstimate.tsx` shows it too (served as
/// `rulesCheckedOn`).
pub const RULES_CHECKED_ON: &str = "2026-10-07";

/// How many minutes above a band's threshold a PROVISIONAL projection is
/// "borderline": in production (2026-09-29..10-05) a band projected 30
/// minutes before arrival was right only ~61% of the time for a projection
/// of 15-17 minutes and ~60% for 30-32, against ~90% outside this zone.
/// See docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md,
/// "Decisions (2026-10-07)".
pub const BORDERLINE_MINUTES: i32 = 3;

/// A compensation scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Delay Repay 15: 25% / 50% / 100% of the single fare at 15 / 30 / 60
    /// minutes, the return fare at 120.
    Dr15,
    /// Delay Repay 30: DR15 without the 15-minute band.
    Dr30,
    /// Heathrow Express's own scheme (tickets from 1 September 2024): 25%
    /// of the ticket for more than 30 minutes, 50% for more than 60.
    HeathrowExpress,
    /// An operator running its own ("traditional") compensation scheme:
    /// no percentage is estimated, only the claim link.
    OwnScheme,
}

impl Scheme {
    /// The wire spelling (`DR15`, `DR30`, `HX`, `own`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dr15 => "DR15",
            Self::Dr30 => "DR30",
            Self::HeathrowExpress => "HX",
            Self::OwnScheme => "own",
        }
    }

    /// Each band, highest first: (nominal band minutes, the first delay in
    /// whole minutes that qualifies, percentage). Heathrow Express pays on
    /// "more than" 30 and 60 minutes, so its first qualifying minutes are 31
    /// and 61.
    fn bands(self) -> &'static [(i32, i32, u8)] {
        match self {
            Self::Dr15 => &[(120, 120, 100), (60, 60, 100), (30, 30, 50), (15, 15, 25)],
            Self::Dr30 => &[(120, 120, 100), (60, 60, 100), (30, 30, 50)],
            Self::HeathrowExpress => &[(60, 61, 50), (30, 31, 25)],
            Self::OwnScheme => &[],
        }
    }
}

/// One operator whose scheme or claim page differs from the default (DR15
/// with the National Rail claim page).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorRule {
    /// Display name.
    pub name: &'static str,
    /// The ATOC code (reference-data/toc-codes.csv).
    pub atoc: &'static str,
    /// Lower-case spellings matched as substrings of a ticket's free-text
    /// `operator`.
    pub spellings: &'static [&'static str],
    pub scheme: Scheme,
    pub claim_url: &'static str,
    /// Caledonian Sleeper also compensates the room supplement.
    pub room_supplement: bool,
}

const LNER_CLAIM_URL: &str = "https://delayrepay.lner.co.uk/delayrepayV2/";
const TFL_CLAIM_URL: &str = "https://tfl.gov.uk/fares/refunds/apply-for-a-service-delay-refund";

/// Every operator not on the default (DR15, generic claim page). See the
/// module doc for the sources.
///
/// The extra spellings (`"cross country"`, `"london north eastern
/// railway"`) are Low finding #5 of the 2026-09-25 review: real ticket text
/// does not always spell an operator the way its brand does.
pub const OPERATORS: &[OperatorRule] = &[
    OperatorRule {
        name: "LNER",
        atoc: "GR",
        spellings: &["lner", "london north eastern railway"],
        scheme: Scheme::Dr30,
        claim_url: LNER_CLAIM_URL,
        room_supplement: false,
    },
    OperatorRule {
        name: "CrossCountry",
        atoc: "XC",
        spellings: &["crosscountry", "cross country"],
        scheme: Scheme::Dr30,
        claim_url: "https://delayrepay.crosscountrytrains.co.uk/",
        room_supplement: false,
    },
    OperatorRule {
        name: "ScotRail",
        atoc: "SR",
        spellings: &["scotrail"],
        scheme: Scheme::Dr30,
        claim_url: "https://www.scotrail.co.uk/plan-your-journey/our-delay-repay-guarantee",
        room_supplement: false,
    },
    // Not "caledonian" alone: Caledonian MacBrayne is a ferry operator.
    OperatorRule {
        name: "Caledonian Sleeper",
        atoc: "CS",
        spellings: &["caledonian sleeper"],
        scheme: Scheme::Dr30,
        claim_url: "https://www.sleeper.scot/help-support/after-your-trip/",
        room_supplement: true,
    },
    OperatorRule {
        name: "Hull Trains",
        atoc: "HT",
        spellings: &["hull trains"],
        scheme: Scheme::Dr30,
        claim_url: "https://www.hulltrains.co.uk/support-and-contact/refunds-and-compensation",
        room_supplement: false,
    },
    OperatorRule {
        name: "Lumo",
        atoc: "LD",
        spellings: &["lumo"],
        scheme: Scheme::Dr30,
        claim_url: "https://www.lumo.co.uk/help/delay-repay",
        room_supplement: false,
    },
    OperatorRule {
        name: "Heathrow Express",
        atoc: "HX",
        spellings: &["heathrow express"],
        scheme: Scheme::HeathrowExpress,
        claim_url: "https://www.heathrowexpress.com/contact-us",
        room_supplement: false,
    },
    OperatorRule {
        name: "Elizabeth line",
        atoc: "XR",
        spellings: &["elizabeth line"],
        scheme: Scheme::OwnScheme,
        claim_url: TFL_CLAIM_URL,
        room_supplement: false,
    },
    OperatorRule {
        name: "London Overground",
        atoc: "LO",
        spellings: &["overground"],
        scheme: Scheme::OwnScheme,
        claim_url: TFL_CLAIM_URL,
        room_supplement: false,
    },
    OperatorRule {
        name: "Merseyrail",
        atoc: "ME",
        spellings: &["merseyrail"],
        scheme: Scheme::OwnScheme,
        claim_url: "https://www.merseyrail.org/help-support/refunds-and-compensation/refunds-day-tickets/",
        room_supplement: false,
    },
    OperatorRule {
        name: "Grand Central",
        atoc: "GC",
        spellings: &["grand central"],
        scheme: Scheme::OwnScheme,
        claim_url: "https://www.grandcentralrail.com/help/refunds-and-compensation",
        room_supplement: false,
    },
];

/// National Rail's own compensation page -- confirmed real and accurate by
/// the design doc's own research (Research summary §4): it "directs
/// passengers to claim directly from your train company." The universal
/// fallback for any operator not in [`OPERATORS`], so no response ever
/// carries a claim link that goes nowhere real.
pub const GENERIC_CLAIM_URL: &str =
    "https://www.nationalrail.co.uk/help-and-assistance/compensation-and-refunds/";

/// The scheme that applies, and where to claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorScheme {
    pub scheme: Scheme,
    pub claim_url: &'static str,
    pub room_supplement: bool,
}

impl OperatorScheme {
    fn from_rule(rule: &OperatorRule) -> Self {
        Self {
            scheme: rule.scheme,
            claim_url: rule.claim_url,
            room_supplement: rule.room_supplement,
        }
    }

    /// The default for a named operator not in [`OPERATORS`]: DR15, which
    /// "most operators" run (ORR).
    const DEFAULT: Self = Self {
        scheme: Scheme::Dr15,
        claim_url: GENERIC_CLAIM_URL,
        room_supplement: false,
    };
}

/// The rule for a ticket's free-text `operator`: the whole text equal to
/// an ATOC code (any case), else a known spelling within it (any case).
fn rule_for_text(operator: &str) -> Option<&'static OperatorRule> {
    let text = operator.trim();
    if let Some(rule) = OPERATORS
        .iter()
        .find(|rule| rule.atoc.eq_ignore_ascii_case(text))
    {
        return Some(rule);
    }
    let lower = text.to_lowercase();
    OPERATORS
        .iter()
        .find(|rule| rule.spellings.iter().any(|s| lower.contains(s)))
}

/// The scheme for a ticket: its own free-text `operator` when that names a
/// known operator, else the train's ATOC code from the CIF schedule
/// (`atoc_code`, `TrackedTrainState::operator_code`), else DR15 when the
/// ticket names an operator at all. `None` when neither is known: there is
/// nothing to base an estimate on.
pub fn scheme_for(operator: Option<&str>, atoc_code: Option<&str>) -> Option<OperatorScheme> {
    let operator = operator.map(str::trim).filter(|s| !s.is_empty());
    if let Some(rule) = operator.and_then(rule_for_text) {
        return Some(OperatorScheme::from_rule(rule));
    }
    let atoc = atoc_code.map(str::trim).filter(|s| !s.is_empty());
    if let Some(atoc) = atoc {
        return Some(
            OPERATORS
                .iter()
                .find(|rule| rule.atoc.eq_ignore_ascii_case(atoc))
                .map_or(OperatorScheme::DEFAULT, OperatorScheme::from_rule),
        );
    }
    operator.map(|_| OperatorScheme::DEFAULT)
}

/// Whether a ticket is a single or a return, as far as its free-text
/// `ticket_type` clearly says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TicketKind {
    Single,
    Return,
    /// Neither, both, or no ticket type: the 120-minute band shows both
    /// readings.
    Unknown,
}

/// [`TicketKind`] from `ticket_type`, by whole words: `return`/`rtn` or
/// `single`/`sgl`/`sngl`. Anything else (a season, a rover, a fare code, or
/// text naming both) is `Unknown`, never a guess.
pub fn ticket_kind(ticket_type: Option<&str>) -> TicketKind {
    let Some(text) = ticket_type else {
        return TicketKind::Unknown;
    };
    let lower = text.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let has = |options: &[&str]| words.iter().any(|w| options.contains(w));
    match (has(&["return", "rtn"]), has(&["single", "sgl", "sngl"])) {
        (true, false) => TicketKind::Return,
        (false, true) => TicketKind::Single,
        _ => TicketKind::Unknown,
    }
}

/// A rough eligibility estimate for a Delay Repay claim -- never a
/// guarantee, never proof of travel, never a claim itself. `disclaimer` is
/// intentionally NOT optional: every estimate carries its own caveat text
/// baked in, so a caller serializing this type cannot accidentally display
/// a bare percentage with no caveat attached.
///
/// The delay it is computed from is the train's delay against the PUBLIC
/// arrival at the ticket's destination (design doc §9 decision 3), which
/// is what operators pay Delay Repay on. Before the train has arrived there
/// that delay is a projection, and the estimate says so: `provisional` is
/// `true` and `disclaimer` is [`PROVISIONAL_DISCLAIMER`]. It becomes final
/// (`provisional: false`, [`DISCLAIMER`]) once the train has arrived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayRepayEstimate {
    /// `DR15`, `DR30` or `HX` (Heathrow Express's own scheme).
    pub scheme: &'static str,
    /// The band the delay fell into: 15, 30, 60 or 120 (`HX`: 30 or 60,
    /// meaning "more than").
    pub band_minutes: i32,
    /// Rough percentage of `fare_basis`. `None` exactly when `borderline`:
    /// the band could go either way, so no figure is shown.
    pub percentage: Option<u8>,
    /// Which fare `percentage` is of: `single` (the single fare, or half a
    /// return) below 120 minutes and for a single at 120+; `return` (the
    /// whole return fare) at 120+ for a return or an unknown ticket (see
    /// `ticket_kind`); `ticket` (the ticket price) for Heathrow Express.
    pub fare_basis: &'static str,
    /// What the ticket's type says it is; the 120-minute band's wording
    /// depends on it.
    pub ticket_kind: TicketKind,
    /// Caledonian Sleeper only: the percentage of the room supplement
    /// (50 at 30-59 minutes, 100 at 60+). `None` otherwise, and when
    /// `borderline`.
    pub room_supplement_percentage: Option<u8>,
    /// `true` while provisional and the projection is less than
    /// [`BORDERLINE_MINUTES`] above `threshold_minutes`.
    pub borderline: bool,
    /// The band threshold a `borderline` projection is just above; `None`
    /// when not borderline.
    pub threshold_minutes: Option<i32>,
    /// `true` while the train has not reached the ticket's destination yet.
    pub provisional: bool,
    pub disclaimer: &'static str,
}

const DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a guarantee of \
    compensation and not proof you travelled. Always verify eligibility and submit any claim \
    directly with the operator -- this app never submits a claim on your behalf.";

/// [`DISCLAIMER`] for a provisional estimate: the same caveat, led by the
/// fact that the train has not arrived yet and the figure will change.
pub const PROVISIONAL_DISCLAIMER: &str = "Provisional: the train has not reached your \
    destination yet, so this uses its current delay projected to the public timetable arrival \
    there, and it will change. This is a rough, community-sourced estimate, not a guarantee of \
    compensation and not proof you travelled. Always verify eligibility and submit any claim \
    directly with the operator -- this app never submits a claim on your behalf.";

/// The route-level disclaimer rendered by every HTTP response that carries
/// a Delay Repay estimate -- textually DIFFERENT from `DISCLAIMER` above
/// (that one lives inside a non-null `DelayRepayEstimate.disclaimer`; this
/// one is the always-populated, top-level field on the response, present
/// even when `estimate` is `None`). Both responses read it through
/// [`assess`], so the safety-critical, verbatim-required text cannot drift
/// between them (see `components/DelayRepayEstimate.tsx`'s doc comment).
pub const ROUTE_DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a \
    guarantee of compensation and not proof you travelled. This app never submits a claim on your \
    behalf -- verify eligibility and claim directly from the operator using the link above.";

/// The estimate for `delay_minutes` under `scheme`; `None` below the
/// scheme's lowest band, or for an own-scheme operator (no percentage is
/// estimated for those).
pub fn estimate_delay_repay(
    scheme: OperatorScheme,
    delay_minutes: i32,
    provisional: bool,
    ticket: TicketKind,
) -> Option<DelayRepayEstimate> {
    let &(band, first_minute, percentage) = scheme
        .scheme
        .bands()
        .iter()
        .find(|(_, first_minute, _)| delay_minutes >= *first_minute)?;
    let borderline = provisional && delay_minutes - first_minute < BORDERLINE_MINUTES;
    let fare_basis = match (scheme.scheme, band, ticket) {
        (Scheme::HeathrowExpress, _, _) => "ticket",
        (_, b, TicketKind::Return | TicketKind::Unknown) if b >= 120 => "return",
        _ => "single",
    };
    let room = scheme
        .room_supplement
        .then_some(if band >= 60 { 100 } else { 50 });
    Some(DelayRepayEstimate {
        scheme: scheme.scheme.as_str(),
        band_minutes: band,
        percentage: (!borderline).then_some(percentage),
        fare_basis,
        ticket_kind: ticket,
        room_supplement_percentage: room.filter(|_| !borderline),
        borderline,
        threshold_minutes: borderline.then_some(band),
        provisional,
        disclaimer: if provisional {
            PROVISIONAL_DISCLAIMER
        } else {
            DISCLAIMER
        },
    })
}

/// Whether the train got the passenger to the ticket's destination, from
/// TRUST and Darwin (`data::delay_repay_outcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    /// TRUST reported its arrival there: the delay is final.
    Arrived,
    /// TRUST reported only a departure there (a station that reports
    /// departures only): the delay is final, measured on that departure.
    DepartedOnly,
    /// It was cancelled before the destination, terminated short of it,
    /// or ran through it without calling. No percentage: eligibility
    /// depends on the replacement journey.
    NotReached,
}

/// Where a TRUST cancellation (`0002`) applies, relative to the
/// destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelPosition {
    /// `AT ORIGIN`, `ON CALL` or `OUT OF PLAN`: the whole train.
    WholeTrain,
    /// `EN ROUTE` from a call before the destination.
    BeforeDestination,
    /// `EN ROUTE` from the destination or a call after it: the train still
    /// got there.
    AtOrAfterDestination,
    /// No location, or one not on the schedule.
    Unknown,
}

/// The facts [`classify_outcome`] reads, all at the ticket's destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct OutcomeFacts {
    /// TRUST reported an ARRIVAL there.
    pub arrived: bool,
    /// TRUST reported a DEPARTURE there.
    pub departed: bool,
    /// TRUST reported a PASS there (it ran through).
    pub passed: bool,
    /// Darwin listed the destination as a cancelled call
    /// (`trains.skipped_stations`, captured at pin time).
    pub darwin_skipped: bool,
    /// TRUST reported an arrival or departure at a call after the
    /// destination.
    pub reported_beyond: bool,
    /// `Some` while the train is cancelled (`status = 'cancelled'`).
    pub cancelled: Option<CancelPosition>,
}

/// The outcome at the destination; `None` while it is not known yet (the
/// train is on its way, or the destination reports nothing).
///
/// A reported call there always wins: an arrival is [`Outcome::Arrived`],
/// a departure alone [`Outcome::DepartedOnly`]. Without one, the train did
/// not get there when it ran through (a TRUST pass, or Darwin's cancelled
/// call once TRUST has reported it beyond the destination: Darwin's list is
/// a pin-time snapshot, so it alone is not enough), or when it is
/// cancelled anywhere but at or after the destination.
pub fn classify_outcome(facts: OutcomeFacts) -> Option<Outcome> {
    if facts.arrived {
        return Some(Outcome::Arrived);
    }
    if facts.departed {
        return Some(Outcome::DepartedOnly);
    }
    if facts.passed || (facts.reported_beyond && facts.darwin_skipped) {
        return Some(Outcome::NotReached);
    }
    match facts.cancelled {
        Some(CancelPosition::AtOrAfterDestination) | None => None,
        Some(_) => Some(Outcome::NotReached),
    }
}

/// Everything a ticket needs to assess, from its own fields and its train.
#[derive(Debug, Clone, Copy, Default)]
pub struct AssessInputs<'a> {
    /// The ticket's free-text operator.
    pub operator: Option<&'a str>,
    /// The ticket's free-text type.
    pub ticket_type: Option<&'a str>,
    /// The train's ATOC code, from its CIF schedule.
    pub atoc_code: Option<&'a str>,
    /// The delay against the public arrival at `measured_at_crs`.
    pub delay: Option<crate::data::stop_delay::StopDelay>,
    pub outcome: Option<Outcome>,
    /// The destination the delay is measured at.
    pub measured_at_crs: Option<&'a str>,
    pub measured_at_name: Option<&'a str>,
}

/// The Delay Repay fields both `GET .../delay-repay` and
/// `GET /Train/tickets/mine` serve (flattened into each), so the two can
/// never disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayRepayFields {
    /// The delay against the PUBLIC arrival at `measured_at_crs` (design
    /// doc §9 decision 3): measured once the train has arrived (or, at a
    /// departure-only station, departed), projected before then
    /// (`provisional`). `None` when `outcome` is `notReached`.
    pub delay_minutes: Option<i32>,
    /// `true` while `delay_minutes` (and `estimate`) are a projection.
    pub provisional: bool,
    /// See `common::public_delay::DelayBasis`; `None` exactly when
    /// `delay_minutes` is.
    pub delay_basis: Option<crate::data::stop_delay::DelayBasis>,
    /// Where the delay was measured: the ticket's destination, else the
    /// pin destination, else the terminus. Set when `delay_minutes` is, and
    /// when `outcome` is `notReached` (the destination not reached).
    pub measured_at_crs: Option<String>,
    /// `measured_at_crs`'s station name, when known.
    pub measured_at_name: Option<String>,
    /// See [`Outcome`]; `None` while not known yet.
    pub outcome: Option<Outcome>,
    /// `true` for an operator running its own compensation scheme: no
    /// percentage, just the claim link.
    pub own_scheme: bool,
    /// The scheme's operator, when it is one in [`OPERATORS`] (else `None`:
    /// the ticket's own `operator` text is the best name).
    pub scheme_operator: Option<&'static str>,
    pub estimate: Option<DelayRepayEstimate>,
    /// Always populated, independent of `estimate`: no caller is ever left
    /// with nowhere real to go.
    pub claim_url: &'static str,
    /// Always populated, independent of `estimate`.
    pub disclaimer: &'static str,
    /// [`RULES_CHECKED_ON`].
    pub rules_checked_on: &'static str,
}

/// [`DelayRepayFields`] for one ticket. Pure.
pub fn assess(inputs: AssessInputs<'_>) -> DelayRepayFields {
    let scheme = scheme_for(inputs.operator, inputs.atoc_code);
    let scheme_operator = inputs
        .operator
        .and_then(rule_for_text)
        .or_else(|| {
            inputs.atoc_code.and_then(|code| {
                OPERATORS
                    .iter()
                    .find(|r| r.atoc.eq_ignore_ascii_case(code.trim()))
            })
        })
        .map(|rule| rule.name);
    let claim_url = scheme.map_or(GENERIC_CLAIM_URL, |s| s.claim_url);
    let own_scheme = scheme.is_some_and(|s| s.scheme == Scheme::OwnScheme);
    let not_reached = inputs.outcome == Some(Outcome::NotReached);
    let delay = inputs.delay.filter(|_| !not_reached);
    let estimate = match (scheme, delay) {
        (Some(scheme), Some(delay)) => estimate_delay_repay(
            scheme,
            delay.minutes,
            delay.provisional,
            ticket_kind(inputs.ticket_type),
        ),
        _ => None,
    };
    let (delay_minutes, delay_basis, provisional) = crate::data::stop_delay::split(delay);
    let measured_here = delay.is_some() || not_reached;
    DelayRepayFields {
        delay_minutes,
        provisional,
        delay_basis,
        measured_at_crs: inputs
            .measured_at_crs
            .filter(|_| measured_here)
            .map(str::to_string),
        measured_at_name: inputs
            .measured_at_name
            .filter(|_| measured_here)
            .map(str::to_string),
        outcome: inputs.outcome,
        own_scheme,
        scheme_operator,
        estimate,
        claim_url,
        disclaimer: ROUTE_DISCLAIMER,
        rules_checked_on: RULES_CHECKED_ON,
    }
}

/// The claim page for a ticket's operator text; never `None` (see
/// [`GENERIC_CLAIM_URL`]).
pub fn claim_url_for(operator: &str) -> &'static str {
    scheme_for(Some(operator), None).map_or(GENERIC_CLAIM_URL, |s| s.claim_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::stop_delay::{DelayBasis, StopDelay};

    fn scheme(operator: &str) -> OperatorScheme {
        scheme_for(Some(operator), None).unwrap()
    }

    /// (band, percentage, fare basis) for a final estimate of a ticket of
    /// unknown kind.
    fn band(operator: &str, minutes: i32) -> Option<(i32, Option<u8>, &'static str)> {
        estimate_delay_repay(scheme(operator), minutes, false, TicketKind::Unknown)
            .map(|e| (e.band_minutes, e.percentage, e.fare_basis))
    }

    #[test]
    fn dr15_band_edges() {
        let expected: &[(i32, Option<(i32, Option<u8>, &str)>)] = &[
            (14, None),
            (15, Some((15, Some(25), "single"))),
            (29, Some((15, Some(25), "single"))),
            (30, Some((30, Some(50), "single"))),
            (59, Some((30, Some(50), "single"))),
            (60, Some((60, Some(100), "single"))),
            (119, Some((60, Some(100), "single"))),
            (120, Some((120, Some(100), "return"))),
            (400, Some((120, Some(100), "return"))),
        ];
        for &(minutes, want) in expected {
            assert_eq!(band("Southeastern", minutes), want, "{minutes}");
        }
        assert_eq!(scheme("Southeastern").scheme, Scheme::Dr15);
    }

    /// Every DR30 operator, by name and by ATOC code: no 15-minute band.
    #[test]
    fn dr30_operators_have_no_fifteen_minute_band() {
        for (name, atoc) in [
            ("LNER", "GR"),
            ("London North Eastern Railway", "GR"),
            ("CrossCountry", "XC"),
            ("Cross Country", "XC"),
            ("ScotRail", "SR"),
            ("Abellio ScotRail", "SR"),
            ("Caledonian Sleeper", "CS"),
            ("Hull Trains", "HT"),
            ("Lumo", "LD"),
        ] {
            for s in [scheme(name), scheme_for(None, Some(atoc)).unwrap()] {
                assert_eq!(s.scheme, Scheme::Dr30, "{name}/{atoc}");
                for (minutes, want) in [
                    (15, None),
                    (29, None),
                    (30, Some((30, Some(50)))),
                    (59, Some((30, Some(50)))),
                    (60, Some((60, Some(100)))),
                    (119, Some((60, Some(100)))),
                    (120, Some((120, Some(100)))),
                ] {
                    assert_eq!(
                        estimate_delay_repay(s, minutes, false, TicketKind::Unknown)
                            .map(|e| (e.band_minutes, e.percentage)),
                        want,
                        "{name} {minutes}"
                    );
                }
            }
        }
    }

    #[test]
    fn heathrow_express_pays_on_more_than_30_and_60_minutes_of_the_ticket() {
        let hx = scheme("Heathrow Express");
        assert_eq!(hx.scheme, Scheme::HeathrowExpress);
        assert_eq!(scheme_for(None, Some("HX")), Some(hx));
        for (minutes, want) in [
            (15, None),
            (30, None),
            (31, Some((30, Some(25), "ticket"))),
            (60, Some((30, Some(25), "ticket"))),
            (61, Some((60, Some(50), "ticket"))),
            (200, Some((60, Some(50), "ticket"))),
        ] {
            assert_eq!(band("Heathrow Express", minutes), want, "{minutes}");
        }
        assert_eq!(
            estimate_delay_repay(hx, 61, false, TicketKind::Unknown)
                .unwrap()
                .scheme,
            "HX"
        );
    }

    #[test]
    fn caledonian_sleeper_also_estimates_the_room_supplement() {
        let cs = scheme("Caledonian Sleeper");
        let room = |minutes| {
            estimate_delay_repay(cs, minutes, false, TicketKind::Single)
                .and_then(|e| e.room_supplement_percentage)
        };
        assert_eq!(room(29), None);
        assert_eq!(room(30), Some(50));
        assert_eq!(room(59), Some(50));
        assert_eq!(room(60), Some(100));
        assert_eq!(room(130), Some(100));
        assert_eq!(
            estimate_delay_repay(scheme("LNER"), 60, false, TicketKind::Single)
                .unwrap()
                .room_supplement_percentage,
            None
        );
        // CalMac is a ferry operator, not the sleeper.
        assert_eq!(scheme("Caledonian MacBrayne").scheme, Scheme::Dr15);
    }

    #[test]
    fn own_scheme_operators_get_no_estimate_but_their_own_claim_page() {
        for (name, atoc) in [
            ("Elizabeth line", "XR"),
            ("London Overground", "LO"),
            ("Merseyrail", "ME"),
            ("Grand Central", "GC"),
        ] {
            let s = scheme(name);
            assert_eq!(s.scheme, Scheme::OwnScheme, "{name}");
            assert_eq!(scheme_for(None, Some(atoc)), Some(s), "{atoc}");
            assert_ne!(s.claim_url, GENERIC_CLAIM_URL, "{name}");
            assert_eq!(estimate_delay_repay(s, 90, false, TicketKind::Single), None);
        }
    }

    /// Every ATOC code in the table is a real one in
    /// reference-data/toc-codes.csv, named consistently.
    #[test]
    fn every_atoc_code_is_in_the_reference_table() {
        let csv = include_str!("../../../../reference-data/toc-codes.csv");
        for rule in OPERATORS {
            let line = csv
                .lines()
                .find(|l| l.split(',').next() == Some(rule.atoc))
                .unwrap_or_else(|| panic!("{} not in toc-codes.csv", rule.atoc));
            let name = line.split(',').nth(1).unwrap();
            assert_eq!(
                scheme_for(Some(name), None),
                Some(OperatorScheme::from_rule(rule)),
                "toc-codes.csv's name {name:?} must match {}",
                rule.name
            );
        }
    }

    #[test]
    fn the_ticket_text_wins_then_the_trains_atoc_code_then_dr15() {
        // An ATOC code typed as the operator.
        assert_eq!(scheme("gr").scheme, Scheme::Dr30);
        // Ticket text names a known operator: it wins over the train's code.
        assert_eq!(
            scheme_for(Some("LNER"), Some("GW")).unwrap().scheme,
            Scheme::Dr30
        );
        // Unrecognised text (a retailer): the train's code decides.
        assert_eq!(
            scheme_for(Some("Trainline"), Some("XC")).unwrap().scheme,
            Scheme::Dr30
        );
        assert_eq!(
            scheme_for(Some("Trainline"), Some("GW")),
            Some(OperatorScheme::DEFAULT)
        );
        assert_eq!(
            scheme_for(Some("Trainline"), None),
            Some(OperatorScheme::DEFAULT)
        );
        assert_eq!(scheme_for(None, None), None);
        assert_eq!(scheme_for(Some("  "), None), None);
        // A two-letter code is only matched as the whole text.
        assert_eq!(scheme("Great Western Railway").scheme, Scheme::Dr15);
    }

    #[test]
    fn ticket_kind_reads_only_what_the_text_clearly_says() {
        for (text, want) in [
            ("Off-Peak Day Return", TicketKind::Return),
            ("Anytime Return", TicketKind::Return),
            ("OFF-PEAK RTN", TicketKind::Return),
            ("Advance Single", TicketKind::Single),
            ("Anytime Day Single", TicketKind::Single),
            ("SGL", TicketKind::Single),
            ("single/return", TicketKind::Unknown),
            ("Weekly Season", TicketKind::Unknown),
            ("SOR", TicketKind::Unknown),
            ("Returnable deposit", TicketKind::Unknown),
            ("", TicketKind::Unknown),
        ] {
            assert_eq!(ticket_kind(Some(text)), want, "{text:?}");
        }
        assert_eq!(ticket_kind(None), TicketKind::Unknown);
    }

    #[test]
    fn the_120_minute_band_is_ticket_aware() {
        let dr15 = scheme("Southeastern");
        let basis = |kind| {
            estimate_delay_repay(dr15, 125, false, kind)
                .map(|e| (e.percentage, e.fare_basis, e.ticket_kind))
        };
        assert_eq!(
            basis(TicketKind::Return),
            Some((Some(100), "return", TicketKind::Return))
        );
        assert_eq!(
            basis(TicketKind::Single),
            Some((Some(100), "single", TicketKind::Single))
        );
        assert_eq!(
            basis(TicketKind::Unknown),
            Some((Some(100), "return", TicketKind::Unknown))
        );
        // Below 120 the fare basis is the single whatever the ticket.
        assert_eq!(
            estimate_delay_repay(dr15, 90, false, TicketKind::Return)
                .unwrap()
                .fare_basis,
            "single"
        );
    }

    /// The borderline zone: a provisional projection less than 3 minutes
    /// above a threshold shows no percentage; at or beyond 3 minutes above,
    /// and once final, the band and percentage show as before.
    #[test]
    fn borderline_boundaries() {
        let cases: &[(&str, &[(i32, Option<(bool, Option<i32>)>)])] = &[
            (
                "Southeastern",
                &[
                    (14, None),
                    (15, Some((true, Some(15)))),
                    (17, Some((true, Some(15)))),
                    (18, Some((false, None))),
                    (29, Some((false, None))),
                    (30, Some((true, Some(30)))),
                    (32, Some((true, Some(30)))),
                    (33, Some((false, None))),
                    (60, Some((true, Some(60)))),
                    (62, Some((true, Some(60)))),
                    (63, Some((false, None))),
                    (120, Some((true, Some(120)))),
                    (122, Some((true, Some(120)))),
                    (123, Some((false, None))),
                ],
            ),
            (
                "LNER",
                &[
                    (15, None),
                    (17, None),
                    (29, None),
                    (30, Some((true, Some(30)))),
                    (32, Some((true, Some(30)))),
                    (33, Some((false, None))),
                ],
            ),
            (
                "Heathrow Express",
                &[
                    (30, None),
                    (31, Some((true, Some(30)))),
                    (33, Some((true, Some(30)))),
                    (34, Some((false, None))),
                    (61, Some((true, Some(60)))),
                    (63, Some((true, Some(60)))),
                    (64, Some((false, None))),
                ],
            ),
        ];
        for (operator, table) in cases {
            let s = scheme(operator);
            for &(minutes, want) in *table {
                let estimate = estimate_delay_repay(s, minutes, true, TicketKind::Unknown);
                assert_eq!(
                    estimate
                        .as_ref()
                        .map(|e| (e.borderline, e.threshold_minutes)),
                    want,
                    "{operator} {minutes}"
                );
                if let Some(e) = &estimate {
                    assert_eq!(e.percentage.is_none(), e.borderline, "{operator} {minutes}");
                }
                // Final estimates are never borderline.
                let final_estimate = estimate_delay_repay(s, minutes, false, TicketKind::Unknown);
                if let Some(e) = final_estimate {
                    assert!(
                        !e.borderline && e.percentage.is_some(),
                        "{operator} {minutes}"
                    );
                }
            }
        }
        // The sleeper's room supplement is hidden in the zone too.
        let e = estimate_delay_repay(scheme("Caledonian Sleeper"), 31, true, TicketKind::Single)
            .unwrap();
        assert_eq!((e.percentage, e.room_supplement_percentage), (None, None));
    }

    #[test]
    fn every_estimate_carries_the_disclaimer_for_its_state() {
        let s = scheme("LNER");
        let e = estimate_delay_repay(s, 60, false, TicketKind::Unknown).unwrap();
        assert_eq!((e.disclaimer, e.provisional), (DISCLAIMER, false));
        let e = estimate_delay_repay(s, 60, true, TicketKind::Unknown).unwrap();
        assert_eq!(
            (e.disclaimer, e.provisional),
            (PROVISIONAL_DISCLAIMER, true)
        );
        assert!(PROVISIONAL_DISCLAIMER.starts_with("Provisional:"));
        assert!(PROVISIONAL_DISCLAIMER.ends_with(DISCLAIMER));
    }

    #[test]
    fn outcome_classification() {
        let none = OutcomeFacts::default();
        assert_eq!(classify_outcome(none), None);
        let arrived = OutcomeFacts {
            arrived: true,
            departed: true,
            cancelled: Some(CancelPosition::WholeTrain),
            ..none
        };
        assert_eq!(classify_outcome(arrived), Some(Outcome::Arrived));
        let departed = OutcomeFacts {
            departed: true,
            passed: true,
            ..none
        };
        assert_eq!(classify_outcome(departed), Some(Outcome::DepartedOnly));
        for facts in [
            OutcomeFacts {
                passed: true,
                ..none
            },
            OutcomeFacts {
                darwin_skipped: true,
                reported_beyond: true,
                ..none
            },
            OutcomeFacts {
                cancelled: Some(CancelPosition::WholeTrain),
                ..none
            },
            OutcomeFacts {
                cancelled: Some(CancelPosition::BeforeDestination),
                ..none
            },
            OutcomeFacts {
                cancelled: Some(CancelPosition::Unknown),
                ..none
            },
        ] {
            assert_eq!(
                classify_outcome(facts),
                Some(Outcome::NotReached),
                "{facts:?}"
            );
        }
        for facts in [
            // Darwin's pin-time snapshot alone is not enough.
            OutcomeFacts {
                darwin_skipped: true,
                ..none
            },
            // Reported beyond a destination that reports nothing.
            OutcomeFacts {
                reported_beyond: true,
                ..none
            },
            // Cancelled from the destination or later: it got there.
            OutcomeFacts {
                cancelled: Some(CancelPosition::AtOrAfterDestination),
                ..none
            },
        ] {
            assert_eq!(classify_outcome(facts), None, "{facts:?}");
        }
    }

    fn delay(minutes: i32, provisional: bool) -> Option<StopDelay> {
        Some(StopDelay {
            minutes,
            basis: DelayBasis::Public,
            provisional,
        })
    }

    #[test]
    fn assess_needs_an_operator_or_a_code_and_a_delay() {
        let base = AssessInputs {
            measured_at_crs: Some("EDB"),
            measured_at_name: Some("Edinburgh"),
            ..AssessInputs::default()
        };
        let f = assess(AssessInputs {
            delay: delay(45, false),
            ..base
        });
        assert_eq!(f.estimate, None);
        assert_eq!(f.claim_url, GENERIC_CLAIM_URL);
        assert_eq!(f.disclaimer, ROUTE_DISCLAIMER);
        assert_eq!(f.rules_checked_on, RULES_CHECKED_ON);
        assert_eq!(f.measured_at_crs.as_deref(), Some("EDB"));

        let f = assess(AssessInputs {
            operator: Some("LNER"),
            ..base
        });
        assert_eq!(
            (f.estimate, f.delay_minutes, f.measured_at_crs),
            (None, None, None)
        );
        assert_eq!(f.claim_url, LNER_CLAIM_URL);

        let f = assess(AssessInputs {
            atoc_code: Some("GR"),
            delay: delay(45, false),
            outcome: Some(Outcome::Arrived),
            ..base
        });
        let e = f.estimate.unwrap();
        assert_eq!((e.scheme, e.percentage), ("DR30", Some(50)));
        assert_eq!(f.scheme_operator, Some("LNER"));
        assert_eq!(f.outcome, Some(Outcome::Arrived));
    }

    #[test]
    fn assess_an_own_scheme_operator_shows_the_delay_but_no_estimate() {
        let f = assess(AssessInputs {
            operator: Some("Merseyrail"),
            delay: delay(45, false),
            measured_at_crs: Some("LVC"),
            ..AssessInputs::default()
        });
        assert!(f.own_scheme);
        assert_eq!(f.estimate, None);
        assert_eq!(f.delay_minutes, Some(45));
        assert_eq!(f.scheme_operator, Some("Merseyrail"));
    }

    #[test]
    fn assess_not_reached_drops_the_delay_and_estimate_but_names_the_destination() {
        let f = assess(AssessInputs {
            operator: Some("LNER"),
            delay: delay(70, true),
            outcome: Some(Outcome::NotReached),
            measured_at_crs: Some("EDB"),
            measured_at_name: Some("Edinburgh"),
            ..AssessInputs::default()
        });
        assert_eq!(f.outcome, Some(Outcome::NotReached));
        assert_eq!(
            (f.delay_minutes, f.delay_basis, f.provisional),
            (None, None, false)
        );
        assert_eq!(f.estimate, None);
        assert_eq!(f.measured_at_crs.as_deref(), Some("EDB"));
        assert_eq!(f.measured_at_name.as_deref(), Some("Edinburgh"));
        assert_eq!(f.claim_url, LNER_CLAIM_URL);
    }

    #[test]
    fn known_operators_get_their_own_claim_page_and_others_the_generic_one() {
        assert_eq!(claim_url_for("LNER"), LNER_CLAIM_URL);
        assert_eq!(
            claim_url_for("London North Eastern Railway"),
            LNER_CLAIM_URL
        );
        assert_eq!(
            claim_url_for("Cross Country"),
            "https://delayrepay.crosscountrytrains.co.uk/"
        );
        assert_eq!(
            claim_url_for("Some Operator Not In Our Table"),
            GENERIC_CLAIM_URL
        );
    }

    #[test]
    #[allow(
        clippy::const_is_empty,
        reason = "only rustc 1.88's clippy flags this; the test pins a constant's contract"
    )]
    fn route_disclaimer_is_distinct_from_the_per_estimate_disclaimer_and_non_empty() {
        // Two different strings by design -- see ROUTE_DISCLAIMER's own doc
        // comment and components/DelayRepayEstimate.tsx's doc comment.
        assert_ne!(ROUTE_DISCLAIMER, DISCLAIMER);
        assert!(!ROUTE_DISCLAIMER.is_empty());
    }
}
