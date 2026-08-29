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
    Utc.from_local_datetime(&service_date.and_time(time)).single()
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
        assert_eq!(find_darwin_eta(&[departure("WOK", "18:40", false)], None, None, date), None);
    }

    #[test]
    fn matches_by_pinned_destination_and_parses_hhmm() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("WOK", "18:41", false)];
        let eta = find_darwin_eta(&samples, Some("WOK"), None, date);
        assert_eq!(eta, Some("2026-08-28T18:41:00Z".parse().unwrap()));
    }

    #[test]
    fn falls_back_to_next_calling_point_when_no_pinned_destination() {
        let date: NaiveDate = "2026-08-28".parse().unwrap();
        let samples = vec![departure("SUR", "18:45", false)];
        let eta = find_darwin_eta(&samples, None, Some("SUR"), date);
        assert_eq!(eta, Some("2026-08-28T18:45:00Z".parse().unwrap()));
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
