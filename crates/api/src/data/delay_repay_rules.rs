//! A small, explicitly-maintained-in-this-repo Delay Repay ruleset. There
//! is no official API to sync this against (see
//! docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
//! Research summary §4) -- every value here is compiled from each
//! operator's own live compensation page, cited below, not guessed at.
//!
//! STRUCTURAL SAFETY NOTE, not just a comment: every function in this file
//! is pure (no `PgPool`, no I/O of any kind) and is called from exactly one
//! place in the whole codebase -- `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
//! (crates/api/src/routes/train.rs, Task 5), a read-only route. This file
//! must never gain a function that writes anywhere, and no future change
//! anywhere in this codebase may wire either function below into a write
//! path without a fresh design-doc pass -- see this plan's Global
//! Constraints. This app estimates eligibility and links out; it never
//! submits a claim or asserts proof of travel, full stop.

use serde::Serialize;

/// A rough eligibility estimate for a Delay Repay claim -- never a
/// guarantee, never proof of travel, never a claim itself. `disclaimer` is
/// intentionally NOT optional: every estimate this function returns
/// carries its own caveat text baked in, so a caller serializing this type
/// cannot accidentally display a bare percentage with no caveat attached.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayRepayEstimate {
    pub scheme: &'static str, // "DR15" | "DR30"
    pub band_minutes: i32,    // the threshold band this delay fell into
    pub percentage: u8,       // rough percentage-of-fare estimate
    pub disclaimer: &'static str,
}

const DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a guarantee of \
    compensation and not proof you travelled. Always verify eligibility and submit any claim \
    directly with the operator -- this app never submits a claim on your behalf.";

/// The route-level disclaimer rendered by every HTTP response that carries
/// a Delay Repay estimate -- textually DIFFERENT from `DISCLAIMER` above
/// (that one lives inside a non-null `DelayRepayEstimate.disclaimer`; this
/// one is the always-populated, top-level field on the response, present
/// even when `estimate` is `None`). Hoisted here (rather than staying
/// private to `routes/train.rs`) so both call sites that need it --
/// `routes/train.rs`'s `build_delay_repay_response` and
/// `train_tracking.rs`'s `build_ticket_list_item` -- read the exact same
/// string, closing a drift risk for a safety-critical, verbatim-required
/// piece of text (see `components/DelayRepayEstimate.tsx`'s own doc
/// comment: this string must render "in full, every time," never
/// paraphrased or shortened). Byte-identical to the const this replaced --
/// a mechanical move, not a wording change.
pub const ROUTE_DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a \
    guarantee of compensation and not proof you travelled. This app never submits a claim on your \
    behalf -- verify eligibility and claim directly from the operator using the link above.";

/// Operators verified (in this plan's own research pass, cross-checking
/// docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
/// citation) to still run the older Delay Repay 30 scheme, which has no
/// 15-29 minute band at all -- confirmed against each operator's own live
/// page as of 2026-08-29:
///   - LNER: 30+ minutes (delayrepay.lner.co.uk)
///   - CrossCountry: 30+ minutes (delayrepay.crosscountrytrains.co.uk)
///   - ScotRail: 30+ minutes (scotrail.co.uk/plan-your-journey/our-delay-repay-guarantee)
///     Matched case-insensitively as a substring of the ticket's free-text
///     `operator` field (not a hard ATOC-code catalogue -- see this plan's
///     Global Constraints and the design doc's Open Question 6, which this
///     plan does not resolve). An operator NOT in this list is assumed DR15,
///     per the design doc's own cited "most operators use DR15" finding --
///     see `estimate_delay_repay` below.
const DR30_OPERATORS: &[&str] = &["lner", "crosscountry", "scotrail"];

/// Verified, operator-specific claim pages for the same three DR30
/// operators above (found alongside their scheme during this plan's
/// research pass). Every other operator falls back to `GENERIC_CLAIM_URL`
/// -- deliberately not filled in with unverified guesses. See this plan's
/// Global Constraints.
const CLAIM_URLS: &[(&str, &str)] = &[
    ("lner", "https://delayrepay.lner.co.uk/delayrepayV2/"),
    (
        "crosscountry",
        "https://delayrepay.crosscountrytrains.co.uk/",
    ),
    (
        "scotrail",
        "https://www.scotrail.co.uk/plan-your-journey/our-delay-repay-guarantee",
    ),
];

/// National Rail's own compensation page -- confirmed real and accurate by
/// the design doc's own research (Research summary §4): it "directs
/// passengers to claim directly from your train company." The universal
/// fallback for any operator not in `CLAIM_URLS`, so this route never
/// returns a claim link that goes nowhere real.
pub const GENERIC_CLAIM_URL: &str =
    "https://www.nationalrail.co.uk/help-and-assistance/compensation-and-refunds/";

/// Returns `None` if `delay_minutes` doesn't clear the relevant scheme's
/// lowest band (e.g. a 20-minute delay on a DR30 operator) -- there is
/// nothing positive to estimate, and the route (Task 5) still surfaces the
/// disclaimer and a claim link regardless of whether this returns `Some`.
pub fn estimate_delay_repay(operator: &str, delay_minutes: i32) -> Option<DelayRepayEstimate> {
    let operator_lower = operator.to_lowercase();
    let scheme_is_dr30 = DR30_OPERATORS.iter().any(|op| operator_lower.contains(op));

    let (scheme, band) = if scheme_is_dr30 {
        ("DR30", dr30_band(delay_minutes)?)
    } else {
        ("DR15", dr15_band(delay_minutes)?)
    };

    Some(DelayRepayEstimate {
        scheme,
        band_minutes: band.0,
        percentage: band.1,
        disclaimer: DISCLAIMER,
    })
}

fn dr15_band(delay_minutes: i32) -> Option<(i32, u8)> {
    match delay_minutes {
        d if d >= 60 => Some((60, 100)),
        d if d >= 30 => Some((30, 50)),
        d if d >= 15 => Some((15, 25)),
        _ => None,
    }
}

fn dr30_band(delay_minutes: i32) -> Option<(i32, u8)> {
    match delay_minutes {
        d if d >= 60 => Some((60, 100)),
        d if d >= 30 => Some((30, 50)),
        _ => None,
    }
}

/// Never returns `None` -- every caller gets somewhere real to go, even for
/// an operator this table has no specific page for. See `GENERIC_CLAIM_URL`.
pub fn claim_url_for(operator: &str) -> &'static str {
    let operator_lower = operator.to_lowercase();
    CLAIM_URLS
        .iter()
        .find(|(op, _)| operator_lower.contains(op))
        .map(|(_, url)| *url)
        .unwrap_or(GENERIC_CLAIM_URL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dr15_band_edges() {
        assert_eq!(estimate_delay_repay("Southeastern", 14), None);
        assert_eq!(
            estimate_delay_repay("Southeastern", 15).unwrap().percentage,
            25
        );
        assert_eq!(
            estimate_delay_repay("Southeastern", 29).unwrap().percentage,
            25
        );
        assert_eq!(
            estimate_delay_repay("Southeastern", 30).unwrap().percentage,
            50
        );
        assert_eq!(
            estimate_delay_repay("Southeastern", 59).unwrap().percentage,
            50
        );
        assert_eq!(
            estimate_delay_repay("Southeastern", 60).unwrap().percentage,
            100
        );
        assert_eq!(
            estimate_delay_repay("Southeastern", 30).unwrap().scheme,
            "DR15"
        );
    }

    #[test]
    fn dr30_band_edges_have_no_fifteen_minute_band() {
        assert_eq!(estimate_delay_repay("LNER", 15), None);
        assert_eq!(estimate_delay_repay("LNER", 29), None);
        assert_eq!(estimate_delay_repay("LNER", 30).unwrap().percentage, 50);
        assert_eq!(estimate_delay_repay("LNER", 59).unwrap().percentage, 50);
        assert_eq!(estimate_delay_repay("LNER", 60).unwrap().percentage, 100);
        assert_eq!(estimate_delay_repay("LNER", 30).unwrap().scheme, "DR30");
    }

    #[test]
    fn dr30_operator_matching_is_case_insensitive_and_substring_based() {
        assert_eq!(estimate_delay_repay("ScotRail", 30).unwrap().scheme, "DR30");
        assert_eq!(estimate_delay_repay("scotrail", 30).unwrap().scheme, "DR30");
        assert_eq!(
            estimate_delay_repay("Abellio ScotRail", 30).unwrap().scheme,
            "DR30"
        );
    }

    #[test]
    fn every_estimate_carries_the_disclaimer() {
        let estimate = estimate_delay_repay("LNER", 60).unwrap();
        assert_eq!(estimate.disclaimer, DISCLAIMER);
    }

    #[test]
    fn known_operators_get_their_own_claim_page() {
        assert_eq!(
            claim_url_for("LNER"),
            "https://delayrepay.lner.co.uk/delayrepayV2/"
        );
        assert_eq!(
            claim_url_for("CrossCountry"),
            "https://delayrepay.crosscountrytrains.co.uk/"
        );
    }

    #[test]
    fn an_unlisted_operator_still_gets_a_real_link_never_none() {
        assert_eq!(
            claim_url_for("Some Operator Not In Our Table"),
            GENERIC_CLAIM_URL
        );
    }

    #[test]
    fn route_disclaimer_is_distinct_from_the_per_estimate_disclaimer_and_non_empty() {
        // Two different strings by design -- see ROUTE_DISCLAIMER's own doc
        // comment and components/DelayRepayEstimate.tsx's doc comment for
        // why rendering both at once would read as inconsistent, not
        // doubly cautious.
        assert_ne!(ROUTE_DISCLAIMER, DISCLAIMER);
        assert!(!ROUTE_DISCLAIMER.is_empty());
    }
}
