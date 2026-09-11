//! Parsing and plausibility-guarding for TRUST/RDM's millisecond-epoch
//! timestamp fields (`planned_timestamp`/`actual_timestamp`/
//! `canx_timestamp`).
//!
//! ## Background
//!
//! docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md's
//! "2026-09-11: Task 4 goes live" section documents a sustained,
//! multi-hour live-feed anomaly: the raw TRUST/RDM Kafka feed's
//! `planned_timestamp`/`actual_timestamp` fields arrive already inflated by
//! a near-constant ~59-60 minutes. This was confirmed via two
//! independently-written, byte-identical `parse_epoch_millis`
//! implementations (`crates/trust-consumer/src/process.rs` and
//! `crates/trust-backlog-consumer/src/process.rs`, both just
//! `chrono::DateTime::from_timestamp_millis(raw.parse().ok()?)`,
//! timezone-safe by construction) against a sustained live sample, and is
//! NOT explained by this codebase's own DB session timezone or column
//! types (`TIMESTAMPTZ` throughout, confirmed against
//! `crates/api/migrations/20260828120000_train_tracking.sql`).
//!
//! The best-evidenced hypothesis: whatever upstream system stamps these
//! fields is emitting Europe/London LOCAL wall-clock time (BST, currently
//! UTC+1) but labelling it as though it were already UTC -- a missed
//! timezone conversion one hop upstream of this codebase, which this
//! codebase cannot fix at the source and has NOT had vendor-confirmed.
//! `parse_trust_epoch_millis` below applies that correction, but only as a
//! *guarded* one -- see its own doc comment for why and how it falls back
//! when the correction doesn't actually help.
//!
//! ## Why this lives in `common`
//!
//! `trust-consumer` and `trust-backlog-consumer` each used to define their
//! own byte-identical, uncorrected `parse_epoch_millis` -- the exact
//! duplication that let the corruption above go unnoticed for as long as
//! it did (it took two independent confirmations to be believed). This
//! module exists so there is now exactly one implementation for both
//! crates to share, and it can't drift again.

use chrono::{DateTime, TimeZone, Utc};

/// How far ahead of the reporting message's receipt time an event
/// timestamp (`actual_timestamp`, or `parse_trust_epoch_millis`'s own
/// corrected reinterpretation of any TRUST epoch-millis field) may
/// plausibly be, before it's treated as implausible rather than trusted
/// into a matching decision.
///
/// A few seconds to a couple of minutes of clock skew between systems (the
/// feed's own clock, this process's, the database's) is normal and must
/// not be rejected. The corruption this guards against inflates
/// timestamps by ~59-60 minutes (see this module's own doc comment), so 10
/// minutes sits comfortably above ordinary clock skew and comfortably
/// below the smallest corruption actually observed -- there is no
/// evidence of any real-world value landing in between that this
/// threshold would misclassify either way.
pub const MAX_TIMESTAMP_SKEW_AHEAD_OF_RECEIPT: chrono::Duration = chrono::Duration::minutes(10);

/// `true` if `candidate` is not implausibly ahead of `received_at` -- i.e.
/// not "reported before it happened" by more than ordinary clock skew.
/// `received_at` may legitimately be far AFTER `candidate` (a delayed or
/// backlogged message is perfectly normal); only `candidate` being far
/// AFTER `received_at` is implausible, since nothing can be received
/// before it happens.
///
/// Used both as `parse_trust_epoch_millis`'s own guard on its corrected
/// value (below), and independently, as defense-in-depth, at the two pin
/// -matching boundaries that trust a TRUST `actual_timestamp` into a
/// matching decision: `trust-consumer::matching::resolve_origin_departure`
/// and `api::data::trust_event_backlog_match::find_backlog_match`. Kept as
/// a single shared predicate rather than three separate inline checks so
/// the threshold and its reasoning live in exactly one place.
pub fn is_plausible_actual_timestamp(
    candidate: DateTime<Utc>,
    received_at: DateTime<Utc>,
) -> bool {
    candidate <= received_at + MAX_TIMESTAMP_SKEW_AHEAD_OF_RECEIPT
}

/// Parses a TRUST/RDM millisecond-epoch timestamp string, correcting for
/// the hypothesized Europe/London-local-mislabelled-as-UTC bug documented
/// in this module's own doc comment.
///
/// The raw value, read naively as UTC millis (`raw_utc` below), is
/// reinterpreted as a Europe/London LOCAL wall-clock instant and
/// relocalized to a true UTC instant -- exactly the `NaiveDateTime`
/// round-trip `rail_day.rs`'s own `next_rail_day_boundary` already performs
/// in the opposite direction (a true UTC instant -> its Europe/London
/// local wall-clock reading), via the same
/// `chrono_tz::Europe::London::from_local_datetime` this module now
/// reuses.
///
/// **This correction is guarded, not blindly trusted** -- the exact
/// upstream mechanism is a well-evidenced hypothesis, not a
/// vendor-confirmed root cause. `received_at` (the wall-clock time this
/// process is handling the message that carried `raw`) is used to
/// re-check the corrected value with [`is_plausible_actual_timestamp`]:
///
/// - If the correction is plausible, it's used -- this is the expected,
///   overwhelmingly common case for a real corrupted value.
/// - If the correction is NOT plausible (the hypothesis doesn't actually
///   explain this particular value), this function falls back to the raw,
///   uncorrected interpretation and logs loudly, rather than silently
///   trusting an unverified guess into the data.
///
/// ## DST edge cases
///
/// `chrono_tz::Europe::London::from_local_datetime` forces two to be
/// handled explicitly, since the reinterpreted naive wall-clock reading
/// might land on the one UK night a year where local time is ambiguous or
/// missing:
///
/// - **Autumn fallback (ambiguous)**: the naive reading occurs twice, once
///   in BST and once in GMT. The BST (earlier-UTC) occurrence is chosen:
///   the bug this hypothesis targets is specifically a BST-vs-UTC
///   mislabelling (a dropped `+1` offset), so the BST reading is the one a
///   genuinely-corrupted raw value would actually have been built from in
///   the ordinary case this function exists to fix. Picking the other
///   branch would cost at most one extra hour of error on this one
///   half-hour a year, and the plausibility guard above still catches an
///   implausible result from either choice.
/// - **Spring-forward (nonexistent)**: the naive reading falls in the
///   skipped 01:00-02:00 local hour and was never a real Europe/London
///   local time at all. There is no principled corrected value to compute,
///   so this function makes no correction here -- it falls straight back
///   to the raw interpretation, the same outcome as an implausible
///   correction.
///
/// Neither case panics.
pub fn parse_trust_epoch_millis(raw: &str, received_at: DateTime<Utc>) -> Option<DateTime<Utc>> {
    let millis: i64 = raw.parse().ok()?;
    let raw_utc = DateTime::from_timestamp_millis(millis)?;

    // `raw_utc`'s wall-clock reading, reinterpreted as a Europe/London
    // LOCAL instant rather than the true UTC one it was parsed as.
    let naive_local = raw_utc.naive_utc();
    let corrected = match chrono_tz::Europe::London.from_local_datetime(&naive_local) {
        chrono::LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(earliest, _latest) => Some(earliest.with_timezone(&Utc)),
        chrono::LocalResult::None => None,
    };

    match corrected {
        Some(corrected_utc) if is_plausible_actual_timestamp(corrected_utc, received_at) => {
            Some(corrected_utc)
        }
        Some(corrected_utc) => {
            tracing::warn!(
                raw,
                raw_utc = %raw_utc,
                corrected_utc = %corrected_utc,
                received_at = %received_at,
                "TRUST timestamp correction produced an implausible result (still ahead of \
                 receipt beyond common::MAX_TIMESTAMP_SKEW_AHEAD_OF_RECEIPT); falling back to \
                 the raw, uncorrected interpretation -- the Europe/London-mislabelling \
                 hypothesis may not hold for this value"
            );
            Some(raw_utc)
        }
        None => {
            tracing::warn!(
                raw,
                raw_utc = %raw_utc,
                "raw TRUST timestamp's wall-clock reading falls in the Europe/London \
                 spring-forward gap and has no corrected local interpretation; using the raw, \
                 uncorrected value"
            );
            Some(raw_utc)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bst_period_timestamp_is_corrected_one_hour_earlier_and_is_plausible() {
        // 2026-07-15 13:00 UTC read naively -- July is BST, so the intended
        // instant is 12:00 UTC (13:00 BST local). `received_at` is set just
        // after the CORRECTED instant, matching a real-time feed: the raw
        // (uncorrected) reading would be an hour in the future relative to
        // receipt, which is exactly the implausible shape this whole fix
        // exists to catch.
        let raw = "1784120400000"; // 2026-07-15T13:00:00Z as millis
        let received_at: DateTime<Utc> = "2026-07-15T12:01:00Z".parse().unwrap();

        let corrected = parse_trust_epoch_millis(raw, received_at).unwrap();

        assert_eq!(
            corrected,
            "2026-07-15T12:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            "a BST-period value must be corrected exactly one hour earlier"
        );
        assert!(
            is_plausible_actual_timestamp(corrected, received_at),
            "the corrected value must pass the plausibility check"
        );
    }

    #[test]
    fn a_gmt_period_timestamp_is_a_no_op() {
        // 2026-01-15 -- GMT period, Europe/London local == UTC, so the
        // correction must not shift the value at all.
        let raw = "1768480200000"; // 2026-01-15T12:30:00Z as millis
        let received_at: DateTime<Utc> = "2026-01-15T12:31:00Z".parse().unwrap();

        let corrected = parse_trust_epoch_millis(raw, received_at).unwrap();

        assert_eq!(
            corrected,
            "2026-01-15T12:30:00Z".parse::<DateTime<Utc>>().unwrap(),
            "a GMT-period value must not be shifted"
        );
    }

    #[test]
    fn the_spring_forward_gap_falls_back_to_the_raw_value_without_panicking() {
        // UK clocks spring forward at 01:00 UTC on 2026-03-29, jumping local
        // time from 01:00 GMT straight to 02:00 BST -- 01:30 local never
        // exists that day. Raw millis chosen so `raw_utc`'s naive wall-clock
        // reading is exactly 2026-03-29 01:30:00 -- the nonexistent local
        // time.
        let raw = "1774747800000"; // 2026-03-29T01:30:00Z as millis
        let received_at: DateTime<Utc> = "2026-03-29T02:00:00Z".parse().unwrap();

        let corrected = parse_trust_epoch_millis(raw, received_at).unwrap();

        assert_eq!(
            corrected,
            "2026-03-29T01:30:00Z".parse::<DateTime<Utc>>().unwrap(),
            "no principled correction exists for a nonexistent local time; the raw value is \
             used unchanged"
        );
    }

    #[test]
    fn the_autumn_fallback_overlap_resolves_to_the_bst_occurrence_without_panicking() {
        // UK clocks fall back at 02:00 BST -> 01:00 GMT on 2026-10-25 --
        // 01:30 local occurs twice: once at 00:30 UTC (BST) and once at
        // 01:30 UTC (GMT). Raw millis chosen so `raw_utc`'s naive wall-clock
        // reading is exactly 2026-10-25 01:30:00 -- the ambiguous local
        // time.
        let raw = "1792891800000"; // 2026-10-25T01:30:00Z as millis
        let received_at: DateTime<Utc> = "2026-10-25T00:31:00Z".parse().unwrap();

        let corrected = parse_trust_epoch_millis(raw, received_at).unwrap();

        assert_eq!(
            corrected,
            "2026-10-25T00:30:00Z".parse::<DateTime<Utc>>().unwrap(),
            "the ambiguous local time must resolve to its earlier (BST) UTC occurrence"
        );
    }

    #[test]
    fn a_correction_that_is_itself_implausible_falls_back_to_the_raw_value() {
        // The hypothesis-is-wrong case: the value corrects to 12:00:00Z (one
        // hour earlier than the raw 13:00:00Z, same as the ordinary BST
        // case above), but `received_at` is anchored an hour before even
        // THAT -- simulating a value the Europe/London-mislabelling
        // hypothesis does not actually explain, since neither the raw nor
        // the corrected reading is plausible relative to receipt. The
        // guarded correction must not trust the corrected value just
        // because it looks like the usual shape; it must fall back to the
        // raw interpretation and log loudly instead of guessing.
        let raw = "1784120400000"; // 2026-07-15T13:00:00Z as millis; corrects to 12:00:00Z
        let received_at: DateTime<Utc> = "2026-07-15T11:00:00Z".parse().unwrap(); // an hour before the correction

        let result = parse_trust_epoch_millis(raw, received_at).unwrap();

        assert_eq!(
            result,
            "2026-07-15T13:00:00Z".parse::<DateTime<Utc>>().unwrap(),
            "an implausible correction must fall back to the raw, uncorrected value"
        );
    }

    #[test]
    fn an_unparseable_value_returns_none() {
        let received_at: DateTime<Utc> = "2026-07-15T12:00:00Z".parse().unwrap();
        assert_eq!(parse_trust_epoch_millis("not-a-number", received_at), None);
    }

    #[test]
    fn is_plausible_actual_timestamp_allows_a_late_arrival_but_rejects_an_early_one() {
        let received_at: DateTime<Utc> = "2026-08-28T18:32:00Z".parse().unwrap();

        // Received well after the event -- always plausible, however late.
        assert!(is_plausible_actual_timestamp(
            "2026-08-28T10:00:00Z".parse().unwrap(),
            received_at
        ));
        // A couple of minutes ahead of receipt is ordinary clock skew.
        assert!(is_plausible_actual_timestamp(
            "2026-08-28T18:33:00Z".parse().unwrap(),
            received_at
        ));
        // ~60 minutes ahead of receipt -- the corruption this guards against.
        assert!(!is_plausible_actual_timestamp(
            "2026-08-28T19:32:00Z".parse().unwrap(),
            received_at
        ));
    }
}
