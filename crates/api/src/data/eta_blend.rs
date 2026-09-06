//! Best-effort Darwin/TRUST correlation, applied at read time only (see
//! this plan's Global Constraints for why it doesn't live in
//! `trust-consumer`). Keyed on `(origin CRS, destination CRS)` matching a
//! currently-sampled `StationDeparture`'s `destination_crs` against the
//! tracked train's pin/next-calling-point -- deliberately NOT a guaranteed
//! join. See docs/superpowers/specs/2026-08-28-train-tracking-design.md's
//! Open Questions #5.

use chrono::{DateTime, NaiveDate, NaiveTime, TimeZone, Utc};
use common::StationDeparture;

/// Looks for a live `StationDeparture` sampled at `pin_origin_crs` whose
/// `destination_crs` matches either the tracked train's pinned destination
/// or its currently-known next calling point, and returns Darwin's own
/// estimated time for it if that departure isn't cancelled. `estimated` is
/// either `"On time"`, `"Cancelled"`, or an `"HH:MM"` string (see
/// `common::StationDeparture`'s doc comment) -- only the `"HH:MM"` case
/// yields a concrete ETA; `"On time"` has no better estimate to offer than
/// what trust-consumer's own propagation already computed, so this
/// function returns `None` for it rather than fabricating a value from the
/// scheduled time.
pub fn find_darwin_eta(
    samples: &[StationDeparture],
    pin_destination_crs: Option<&str>,
    next_calling_point: Option<&str>,
    service_date: NaiveDate,
) -> Option<DateTime<Utc>> {
    let target_destination = pin_destination_crs.or(next_calling_point)?;

    let matched = samples
        .iter()
        .find(|d| !d.is_cancelled && d.destination_crs.eq_ignore_ascii_case(target_destination))?;

    let time = NaiveTime::parse_from_str(&matched.estimated, "%H:%M").ok()?;
    london_to_utc(service_date.and_time(time))
}

/// Resolves a Darwin wall-clock time to the instant it names. Darwin
/// publishes Europe/London local times, not UTC -- building the
/// `DateTime<Utc>` directly from `HH:MM` made every `darwin-estimated` ETA
/// exactly an hour late for the ~7 months of British Summer Time, i.e. most
/// of the year, and the error was invisible in winter.
///
/// Same `LocalResult` handling as
/// `crates/poller-tfl/src/dlr/timetable.rs::london_to_utc`, and for the same
/// reason: a departure board really does carry 01:00-01:59 times, which are
/// the ones that occur twice on the autumn clock change and not at all on
/// the spring one. The ambiguous hour takes the first (BST) occurrence, and
/// a nonexistent local time yields `None` so the caller simply leaves TRUST's
/// own ETA in place -- this whole overlay is best-effort, so declining to
/// guess costs nothing. (The aggregator's variant panics on those cases
/// instead, but it only ever resolves local 02:00, which is never ambiguous.)
pub(crate) fn london_to_utc(naive: chrono::NaiveDateTime) -> Option<DateTime<Utc>> {
    match chrono_tz::Europe::London.from_local_datetime(&naive) {
        chrono::LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        chrono::LocalResult::Ambiguous(earliest, _) => Some(earliest.with_timezone(&Utc)),
        chrono::LocalResult::None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn departure(destination_crs: &str, estimated: &str, is_cancelled: bool) -> StationDeparture {
        StationDeparture {
            service_id: "test".to_string(),
            operator: "SW".to_string(),
            destination_crs: destination_crs.to_string(),
            scheduled: "18:32".to_string(),
            estimated: estimated.to_string(),
            is_cancelled,
            delay_minutes: 0,
            cancel_reason: None,
            delay_reason: None,
            headcode: None,
            skipped_stations: vec![],
        }
    }

    #[test]
    fn no_target_destination_means_no_darwin_eta() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        assert_eq!(
            find_darwin_eta(&[departure("WOK", "18:40", false)], None, None, date),
            None
        );
    }

    /// August is inside British Summer Time, so Darwin's `18:41` is
    /// 17:41 UTC. The offset is not hardcoded anywhere: `chrono_tz` derives
    /// it from the date, which `a_winter_estimate_is_utc_because_gmt_is_utc`
    /// below pins down from the other side.
    #[test]
    fn matches_by_pinned_destination_and_parses_hhmm() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("WOK", "18:41", false)];
        let eta = find_darwin_eta(&samples, Some("WOK"), None, date);
        assert_eq!(eta, Some("2026-08-28T17:41:00Z".parse().unwrap()));
    }

    #[test]
    fn falls_back_to_next_calling_point_when_no_pinned_destination() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("SUR", "18:45", false)];
        let eta = find_darwin_eta(&samples, None, Some("SUR"), date);
        assert_eq!(eta, Some("2026-08-28T17:45:00Z".parse().unwrap()));
    }

    /// The same wall-clock time in January is UTC, because GMT is UTC. Held
    /// alongside the BST case so the pair proves the conversion is
    /// date-driven rather than a constant -- a blanket "subtract an hour"
    /// would fail here, and the original "it's already UTC" bug would fail
    /// the BST cases above.
    #[test]
    fn a_winter_estimate_is_utc_because_gmt_is_utc() {
        let date: NaiveDate = "2026-01-15".parse().unwrap();
        let samples = vec![departure("WOK", "18:41", false)];
        let eta = find_darwin_eta(&samples, Some("WOK"), None, date);
        assert_eq!(eta, Some("2026-01-15T18:41:00Z".parse().unwrap()));
    }

    /// 01:30 on the spring-forward Sunday never happens in London. The
    /// overlay declines rather than inventing an instant -- the caller then
    /// keeps whatever trust-consumer already computed.
    #[test]
    fn a_nonexistent_local_time_yields_no_eta_rather_than_a_guess() {
        let date: NaiveDate = "2026-03-29".parse().unwrap(); // BST begins 01:00
        let samples = vec![departure("WOK", "01:30", false)];
        assert_eq!(find_darwin_eta(&samples, Some("WOK"), None, date), None);
    }

    /// 01:30 on the autumn Sunday happens twice; the first (BST) occurrence
    /// wins, matching `poller-tfl`'s timetable resolution.
    #[test]
    fn an_ambiguous_local_time_takes_the_first_occurrence() {
        let date: NaiveDate = "2026-10-25".parse().unwrap(); // BST ends 02:00
        let samples = vec![departure("WOK", "01:30", false)];
        let eta = find_darwin_eta(&samples, Some("WOK"), None, date);
        assert_eq!(eta, Some("2026-10-25T00:30:00Z".parse().unwrap()));
    }

    #[test]
    fn a_cancelled_departure_never_matches() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("WOK", "18:41", true)];
        assert_eq!(find_darwin_eta(&samples, Some("WOK"), None, date), None);
    }

    #[test]
    fn on_time_yields_no_concrete_eta_to_prefer_over_trust() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("WOK", "On time", false)];
        assert_eq!(find_darwin_eta(&samples, Some("WOK"), None, date), None);
    }
}
