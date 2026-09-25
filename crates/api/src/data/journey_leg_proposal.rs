//! Turns an already-saved ticket's own extracted fields into a PROPOSED
//! window-mode journey leg -- origin/destination CRS plus a search-window
//! HINT derived from the ticket's `current_departure_date`, when it has one
//! (see `ticket_extraction::PartialTicket::current_departure_date`'s own
//! doc comment for what that field is and, just as importantly, is NOT: a
//! hard pin to a specific train). `GET /Train/tickets/{ticketId}/journey-leg-proposal`
//! (`routes::train::get_ticket_journey_leg_proposal`) is this module's one
//! caller -- a read-only route that NEVER writes to the database, matching
//! `ticket_extraction`'s own "review-before-save, never auto-create" posture
//! one layer up: this proposal is meant to pre-fill the EXISTING
//! `POST /Journeys` window-mode leg form (`TrackTrainForm.tsx`, reached via
//! `/track?...` query params, same as `TrackJourneyAgainButton.tsx`'s own
//! "track again" deep link), which the caller must still explicitly review
//! and submit -- nothing in this module ever creates a journey leg itself.

use chrono::{DateTime, NaiveDate, NaiveTime, Timelike, Utc};
use serde::Serialize;

/// A proposed window-mode journey leg -- the read side's flattened
/// `depart_after`/`depart_before` shape, matching
/// `routes::journeys::JourneyLegDetailResponse`'s own established
/// read-side convention (see `common::TimeWindow`'s doc comment for why
/// that convention exists) rather than introducing a nested-object shape
/// only this one response would use. No `arrive_after`/`arrive_before`:
/// a ticket's `current_departure_date` only ever tells us when the
/// traveller meant to LEAVE, never when they'd arrive, so there is nothing
/// non-guessed to propose for an arrival window (see this module's own
/// "never guessed at" posture, shared with `ticket_extraction`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JourneyLegProposal {
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    /// `None` whenever the ticket has no `current_departure_date` at all --
    /// there is no honest calendar day to propose without it. The caller's
    /// own `/track` form already defaults an empty `serviceDate` to today,
    /// same as every other visit to that page.
    pub service_date: Option<NaiveDate>,
    pub depart_after: Option<NaiveTime>,
    pub depart_before: Option<NaiveTime>,
}

/// Half the width of the proposed departure window on EACH side of the
/// ticket's own claimed departure time -- 30 minutes either way, an hour
/// wide in total. A ticket's booked time is a hint, not a hard pin (this
/// module's own doc comment): a real booked service can run early, and a
/// traveller can catch an adjacent off-peak service on the same ticket
/// type, so a bare exact-minute pin would be over-confident. An hour is
/// wide enough to comfortably cover an off-by-a-few-minutes booking or an
/// adjacent stopping/fast service pair without being so wide it stops
/// meaningfully narrowing the search at a busy station's departure board.
/// A reasonable-sounding, not researched or load-tested figure -- same
/// posture `common::CUSTOM_NAME_MAX_LENGTH`'s own doc comment flags for its
/// bound, revisit once real usage exists.
const PROPOSAL_WINDOW_HALF_WIDTH_SECONDS: i64 = 30 * 60;

/// Builds a [`JourneyLegProposal`] from a saved ticket's own fields --
/// pure, no I/O, and safe to unit-test directly (this module's own test
/// module below does exactly that) without a `PgPool`/`App` at all, same
/// posture `routes::train::build_delay_repay_response` already established
/// for the sibling Delay Repay estimate route.
pub fn propose_window_leg(
    origin_crs: Option<&str>,
    destination_crs: Option<&str>,
    current_departure_date: Option<DateTime<Utc>>,
) -> JourneyLegProposal {
    let (service_date, depart_after, depart_before) = match current_departure_date {
        Some(instant) => {
            let local = instant.with_timezone(&chrono_tz::Europe::London);
            let (after, before) = window_around(local.time());
            (Some(local.date_naive()), Some(after), Some(before))
        }
        None => (None, None, None),
    };

    JourneyLegProposal {
        origin_crs: origin_crs.map(str::to_string),
        destination_crs: destination_crs.map(str::to_string),
        service_date,
        depart_after,
        depart_before,
    }
}

/// `(after, before)`, `time` +/- [`PROPOSAL_WINDOW_HALF_WIDTH_SECONDS`],
/// clamped to `time`'s OWN calendar day (`00:00:00`..=`23:59:59`) rather
/// than wrapping past midnight -- `journeys::validate_window_leg` rejects
/// any `after > before` pair outright, and letting a late-night or
/// early-morning departure wrap around to the OTHER end of the clock would
/// produce exactly that (e.g. a naive wrap for 23:50 would place `before`
/// at 00:20, before `after`). Deliberately a same-calendar-day clamp, not a
/// genuine cross-midnight window: this is already a best-effort HINT (this
/// module's own doc comment), and a search window that's a little
/// asymmetric right at the boundary of a calendar day costs far less than
/// silently producing a backwards, rejected window at all. Pure integer
/// arithmetic in seconds-since-midnight, not `NaiveTime` addition/
/// subtraction directly, specifically to sidestep `chrono`'s own wrapping
/// semantics there rather than fighting them after the fact.
fn window_around(time: NaiveTime) -> (NaiveTime, NaiveTime) {
    const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
    let secs = i64::from(time.num_seconds_from_midnight());
    let after_secs = (secs - PROPOSAL_WINDOW_HALF_WIDTH_SECONDS).max(0);
    let before_secs = (secs + PROPOSAL_WINDOW_HALF_WIDTH_SECONDS).min(SECONDS_PER_DAY - 1);
    (
        NaiveTime::from_num_seconds_from_midnight_opt(after_secs as u32, 0)
            .expect("after_secs is clamped to [0, SECONDS_PER_DAY)"),
        NaiveTime::from_num_seconds_from_midnight_opt(before_secs as u32, 0)
            .expect("before_secs is clamped to [0, SECONDS_PER_DAY)"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        s.parse().unwrap()
    }

    #[test]
    fn no_departure_date_proposes_only_the_stations_no_window_at_all() {
        let proposal = propose_window_leg(Some("KGX"), Some("EDB"), None);
        assert_eq!(proposal.origin_crs, Some("KGX".to_string()));
        assert_eq!(proposal.destination_crs, Some("EDB".to_string()));
        assert_eq!(proposal.service_date, None);
        assert_eq!(proposal.depart_after, None);
        assert_eq!(proposal.depart_before, None);
    }

    #[test]
    fn neither_station_known_still_proposes_whatever_window_the_date_gives() {
        // "Never guessed at" cuts both ways: a ticket with a departure date
        // but no recovered CRS still proposes the window it does know,
        // rather than withholding everything because part of it is missing.
        let proposal = propose_window_leg(None, None, Some(utc("2026-09-22T18:32:00Z")));
        assert_eq!(proposal.origin_crs, None);
        assert_eq!(proposal.destination_crs, None);
        assert!(proposal.service_date.is_some());
    }

    #[test]
    fn a_departure_date_proposes_a_symmetric_hour_wide_window_around_it_in_london_local_time() {
        // 18:32 UTC in September is 19:32 British Summer Time.
        let proposal =
            propose_window_leg(Some("wat"), Some("rdg"), Some(utc("2026-09-22T18:32:00Z")));
        assert_eq!(
            proposal.service_date,
            Some(NaiveDate::from_ymd_opt(2026, 9, 22).unwrap())
        );
        assert_eq!(
            proposal.depart_after,
            Some(NaiveTime::from_hms_opt(19, 2, 0).unwrap())
        );
        assert_eq!(
            proposal.depart_before,
            Some(NaiveTime::from_hms_opt(20, 2, 0).unwrap())
        );
    }

    #[test]
    fn origin_and_destination_crs_are_passed_through_unnormalized() {
        // Normalization (trim + uppercase) is `routes::journeys`' own job at
        // the point a caller actually submits a window-mode leg
        // (`post_journey`/`post_journey_leg`) -- this proposal is a preview,
        // not a save, so it echoes back exactly what the ticket itself
        // stored, same "don't guess, don't silently rewrite" posture this
        // whole feature already takes.
        let proposal = propose_window_leg(Some("wat"), Some("Rdg"), None);
        assert_eq!(proposal.origin_crs, Some("wat".to_string()));
        assert_eq!(proposal.destination_crs, Some("Rdg".to_string()));
    }

    #[test]
    fn a_departure_late_in_the_local_day_clamps_the_window_to_the_same_calendar_day_rather_than_wrapping()
     {
        // 23:50 UTC in January (GMT, no DST offset) -- a naive +/-30-minute
        // wrap would place `before` at 00:20, before `after`, which
        // `journeys::validate_window_leg` rejects outright.
        let proposal =
            propose_window_leg(Some("KGX"), Some("EDB"), Some(utc("2026-01-15T23:50:00Z")));
        assert_eq!(
            proposal.depart_after,
            Some(NaiveTime::from_hms_opt(23, 20, 0).unwrap())
        );
        assert_eq!(
            proposal.depart_before,
            Some(NaiveTime::from_hms_opt(23, 59, 59).unwrap())
        );
        assert!(proposal.depart_after <= proposal.depart_before);
    }

    #[test]
    fn a_departure_early_in_the_local_day_clamps_the_window_to_the_same_calendar_day_rather_than_wrapping()
     {
        let proposal =
            propose_window_leg(Some("KGX"), Some("EDB"), Some(utc("2026-01-15T00:10:00Z")));
        assert_eq!(
            proposal.depart_after,
            Some(NaiveTime::from_hms_opt(0, 0, 0).unwrap())
        );
        assert_eq!(
            proposal.depart_before,
            Some(NaiveTime::from_hms_opt(0, 40, 0).unwrap())
        );
        assert!(proposal.depart_after <= proposal.depart_before);
    }
}
