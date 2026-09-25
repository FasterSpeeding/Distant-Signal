//! Best-effort Darwin/TRUST correlation, applied at read time only (see
//! this plan's Global Constraints for why it doesn't live in
//! `trust-consumer`). Keyed on `(origin CRS, destination CRS, scheduled
//! departure time)`: a currently-sampled `StationDeparture` must both head
//! for the tracked train's pinned destination/next calling point AND be
//! scheduled within a couple of minutes of the pin's own departure -- see
//! docs/superpowers/specs/2026-08-28-train-tracking-design.md's Open
//! Questions #5. Still deliberately NOT a guaranteed join, but no longer a
//! destination-only one: the scheduled-time half was added for the
//! 2026-09-25 review's High 3 finding, where a departed train's board row
//! being gone meant the NEXT service to the same destination silently became
//! the match (see [`find_darwin_eta`]'s own doc comment).

use chrono::{DateTime, Duration, NaiveTime, TimeZone, Utc};
use common::StationDeparture;

/// How far a departure-board row's own scheduled time may sit from the pin's
/// scheduled departure and still be accepted as the SAME service.
///
/// Deliberately tight. Both values normally describe the same Darwin board
/// row to the exact minute (the pin was created from one), so zero would
/// almost always do; the slack exists only for a pin that came from the CIF
/// picker or manual entry instead, where a half-minute CIF time
/// (`is_half_minute_departure`) can round a minute away from Darwin's own
/// rendering of it. Two minutes is comfortably inside the headway between two
/// services to the SAME destination even on the busiest suburban corridor,
/// which is the discrimination this constant exists to make -- widening it
/// towards `common::MATCH_TOLERANCE` (20 minutes) would re-open exactly the
/// wrong-train bug it was added for.
const SCHEDULED_MATCH_TOLERANCE: Duration = Duration::minutes(2);

/// Looks for a live `StationDeparture` sampled at `pin_origin_crs` that is
/// really THIS tracked train -- its `destination_crs` matches either the
/// pinned destination or the currently-known next calling point, AND its own
/// scheduled time is within [`SCHEDULED_MATCH_TOLERANCE`] of the pin's own
/// `pin_scheduled_departure` -- and returns Darwin's own estimated time for
/// it if that departure isn't cancelled. `estimated` is either `"On time"`,
/// `"Cancelled"`, or an `"HH:MM"` string (see `common::StationDeparture`'s
/// doc comment) -- only the `"HH:MM"` case yields a concrete ETA; `"On time"`
/// has no better estimate to offer than what trust-consumer's own
/// propagation already computed, so this function returns `None` for it
/// rather than fabricating a value from the scheduled time.
///
/// **The time constraint is the 2026-09-25 review's High 3 fix.** This used
/// to match on destination alone (`common::match_darwin_departure`, first
/// non-cancelled row to that destination) -- so the moment the tracked train
/// departed and left its origin's board, the NEXT service to the same
/// destination became the match and ITS estimate was published as this
/// train's ETA, indistinguishably from a real one. See
/// `common::match_darwin_departure_near_time`'s own doc comment for the full
/// account; nothing within tolerance now yields `None`, leaving TRUST's own
/// propagated ETA in place, rather than another train's number.
///
/// **Dating is anchored on the pin's own instant, not on `service_date`.**
/// The previous code built the returned ETA as
/// `london_to_utc(service_date.and_time(estimated))`, which dates a
/// post-midnight estimate a full day EARLY: a 23:58 departure running seven
/// minutes late has `estimated` `"00:05"`, which belongs to `service_date +
/// 1` -- the same class of bug `schedule_query::CallingPoint::day_offset`
/// exists to prevent on the schedule side. [`resolve_london_time_near`] picks
/// the calendar day that puts the wall-clock time CLOSEST to the pin instead,
/// which is correct on both sides of midnight and needs no day-offset field
/// of its own.
///
/// **Known, deliberately-unclosed residual (same review finding):** the value
/// this returns is Darwin's estimated DEPARTURE from the pin's ORIGIN, while
/// the caller (`routes::train::blend_darwin_eta`) writes it into `eta_next`,
/// whose documented meaning is the ETA at the NEXT calling point. The two
/// coincide while the train has not yet left its origin -- which, now that
/// the match is time-scoped to the pin's own departure, is very nearly the
/// only window in which any row still matches at all. Publishing it honestly
/// instead would need a distinct `eta_source` value, and that value is
/// constrained in three places at once (`train_current_state`'s own `CHECK
/// (eta_source IN ('trust-propagated', 'darwin-estimated'))`, the
/// `EtaSource` union in `frontend/lib/types.ts`, and `EtaBadge`'s labelling)
/// -- a wire-contract change well outside a bug fix. Named here rather than
/// silently left.
pub fn find_darwin_eta(
    samples: &[StationDeparture],
    pin_destination_crs: Option<&str>,
    next_calling_point: Option<&str>,
    pin_scheduled_departure: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let target_destination = pin_destination_crs.or(next_calling_point)?;
    let matched = common::match_darwin_departure_near_time(
        samples,
        Some(target_destination),
        pin_scheduled_departure,
        SCHEDULED_MATCH_TOLERANCE,
        |time| resolve_london_time_near(pin_scheduled_departure, time),
    )?;
    let estimated = NaiveTime::parse_from_str(&matched.estimated, "%H:%M").ok()?;
    resolve_london_time_near(pin_scheduled_departure, estimated)
}

/// Resolves a bare Europe/London wall-clock `time` to the instant NEAREST
/// `anchor` -- trying the anchor's own local calendar day and the days either
/// side of it, and keeping whichever lands closest.
///
/// This is what makes a departure board's `"HH:MM"` values datable at all
/// without a day-offset field: a board sampled at 23:58 legitimately carries
/// `"00:05"` rows belonging to the NEXT calendar day, and one sampled at
/// 00:03 carries `"23:58"` rows belonging to the PREVIOUS one. Anchoring on
/// the pin's own instant (never on a bare `service_date`, which is midnight
/// and therefore equidistant from nothing useful) resolves both directions
/// correctly, and keeps the same time resolving to the same instant for both
/// the scheduled-time match and the estimate derived from it.
///
/// `None` only if the wall-clock time exists on none of the three candidate
/// days -- see [`london_to_utc`], whose `LocalResult::None` case this
/// inherits.
pub(crate) fn resolve_london_time_near(
    anchor: DateTime<Utc>,
    time: NaiveTime,
) -> Option<DateTime<Utc>> {
    let anchor_local_date = anchor
        .with_timezone(&chrono_tz::Europe::London)
        .date_naive();
    [-1i64, 0, 1]
        .into_iter()
        .filter_map(|day_offset| {
            london_to_utc((anchor_local_date + Duration::days(day_offset)).and_time(time))
        })
        .filter(|candidate| (*candidate - anchor).abs() <= MAX_ANCHOR_DISTANCE)
        .min_by_key(|candidate| (*candidate - anchor).abs())
}

/// How far from the anchor [`resolve_london_time_near`] will place a
/// wall-clock time before declining entirely. Half a day, which is exactly
/// what makes "the nearest of three candidate days" a well-defined answer
/// rather than a coin flip -- and it is what preserves
/// `london_to_utc`'s own "a local time that does not exist yields `None`"
/// contract: without this bound, a 01:30 on a spring-forward Sunday (which
/// London skips entirely) would silently resolve to 01:30 on the day BEFORE
/// or AFTER, a ~24-hour error dressed up as an answer. No real departure
/// board row's wall-clock time is ever half a day from the pin it belongs to.
const MAX_ANCHOR_DISTANCE: Duration = Duration::hours(12);

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

    /// A board row scheduled at `18:32` local, the time every `pin()` below
    /// is also scheduled for -- so these fixtures describe ONE service seen
    /// from both sides (the pin and the board row), which is what the
    /// time-scoped match now requires.
    fn departure(destination_crs: &str, estimated: &str, is_cancelled: bool) -> StationDeparture {
        departure_at("18:32", destination_crs, estimated, is_cancelled)
    }

    fn departure_at(
        scheduled: &str,
        destination_crs: &str,
        estimated: &str,
        is_cancelled: bool,
    ) -> StationDeparture {
        StationDeparture {
            service_id: "test".to_string(),
            operator: "SW".to_string(),
            destination_crs: destination_crs.to_string(),
            scheduled: scheduled.to_string(),
            estimated: estimated.to_string(),
            is_cancelled,
            delay_minutes: 0,
            cancel_reason: None,
            delay_reason: None,
            headcode: None,
            skipped_stations: vec![],
            platform: None,
            planned_platform: None,
        }
    }

    /// The tracked pin's own scheduled departure, as an RFC-3339 string with
    /// an explicit offset (never a bare naive time) -- the anchor every
    /// assertion below is relative to.
    fn pin(rfc3339: &str) -> DateTime<Utc> {
        rfc3339.parse().unwrap()
    }

    #[test]
    fn no_target_destination_means_no_darwin_eta() {
        assert_eq!(
            find_darwin_eta(
                &[departure("WOK", "18:40", false)],
                None,
                None,
                pin("2026-08-28T18:32:00+01:00"),
            ),
            None
        );
    }

    /// August is inside British Summer Time, so Darwin's `18:41` is
    /// 17:41 UTC. The offset is not hardcoded anywhere: `chrono_tz` derives
    /// it from the date, which `a_winter_estimate_is_utc_because_gmt_is_utc`
    /// below pins down from the other side.
    #[test]
    fn matches_by_pinned_destination_and_parses_hhmm() {
        let samples = vec![departure("WOK", "18:41", false)];
        let eta = find_darwin_eta(
            &samples,
            Some("WOK"),
            None,
            pin("2026-08-28T18:32:00+01:00"),
        );
        assert_eq!(eta, Some("2026-08-28T17:41:00Z".parse().unwrap()));
    }

    #[test]
    fn falls_back_to_next_calling_point_when_no_pinned_destination() {
        let samples = vec![departure("SUR", "18:45", false)];
        let eta = find_darwin_eta(
            &samples,
            None,
            Some("SUR"),
            pin("2026-08-28T18:32:00+01:00"),
        );
        assert_eq!(eta, Some("2026-08-28T17:45:00Z".parse().unwrap()));
    }

    /// The same wall-clock time in January is UTC, because GMT is UTC. Held
    /// alongside the BST case so the pair proves the conversion is
    /// date-driven rather than a constant -- a blanket "subtract an hour"
    /// would fail here, and the original "it's already UTC" bug would fail
    /// the BST cases above.
    #[test]
    fn a_winter_estimate_is_utc_because_gmt_is_utc() {
        let samples = vec![departure("WOK", "18:41", false)];
        let eta = find_darwin_eta(&samples, Some("WOK"), None, pin("2026-01-15T18:32:00Z"));
        assert_eq!(eta, Some("2026-01-15T18:41:00Z".parse().unwrap()));
    }

    /// 01:30 on the spring-forward Sunday never happens in London. The
    /// overlay declines rather than inventing an instant -- the caller then
    /// keeps whatever trust-consumer already computed. Note this is now a
    /// real test of `MAX_ANCHOR_DISTANCE` too: without that bound,
    /// `resolve_london_time_near`'s day-either-side search would have
    /// "resolved" this to 01:30 on the 28th or the 30th instead of declining.
    #[test]
    fn a_nonexistent_local_time_yields_no_eta_rather_than_a_guess() {
        // BST begins at 01:00 on 2026-03-29, so 01:30 exists on neither the
        // anchor's own day nor (within half a day) either neighbour.
        let samples = vec![departure_at("01:25", "WOK", "01:30", false)];
        assert_eq!(
            find_darwin_eta(
                &samples,
                Some("WOK"),
                None,
                pin("2026-03-29T01:25:00Z"), // 01:25 GMT, five minutes before the jump
            ),
            None
        );
    }

    /// 01:30 on the autumn Sunday happens twice; the first (BST) occurrence
    /// wins, matching `poller-tfl`'s timetable resolution.
    #[test]
    fn an_ambiguous_local_time_takes_the_first_occurrence() {
        // BST ends at 02:00 on 2026-10-25. The pin is the 01:00 BST
        // departure (00:00Z), so the ambiguous 01:30 estimate is only half an
        // hour away and well inside MAX_ANCHOR_DISTANCE.
        let samples = vec![departure_at("01:00", "WOK", "01:30", false)];
        let eta = find_darwin_eta(
            &samples,
            Some("WOK"),
            None,
            pin("2026-10-25T01:00:00+01:00"),
        );
        assert_eq!(eta, Some("2026-10-25T00:30:00Z".parse().unwrap()));
    }

    #[test]
    fn a_cancelled_departure_never_matches() {
        let samples = vec![departure("WOK", "18:41", true)];
        assert_eq!(
            find_darwin_eta(
                &samples,
                Some("WOK"),
                None,
                pin("2026-08-28T18:32:00+01:00")
            ),
            None
        );
    }

    #[test]
    fn on_time_yields_no_concrete_eta_to_prefer_over_trust() {
        let samples = vec![departure("WOK", "On time", false)];
        assert_eq!(
            find_darwin_eta(
                &samples,
                Some("WOK"),
                None,
                pin("2026-08-28T18:32:00+01:00")
            ),
            None
        );
    }

    /// **The 2026-09-25 High 3 regression test.** The tracked 18:32 to Woking
    /// has departed and dropped off its origin's board; the only remaining
    /// Woking row is the NEXT service, the 19:02, running three minutes late.
    ///
    /// Before this fix `find_darwin_eta` matched on destination alone and
    /// took the first non-cancelled row, so it returned the 19:02's `19:05`
    /// estimate and `blend_darwin_eta` published it as the tracked train's
    /// own `eta_next`, labelled "Live departure board" -- a different train's
    /// number presented as this one's, with nothing anywhere to indicate it.
    /// Now the board row's own scheduled time must be within
    /// `SCHEDULED_MATCH_TOLERANCE` of the pin's, and 30 minutes is not, so
    /// the overlay declines and TRUST's own propagated ETA survives.
    #[test]
    fn the_next_service_to_the_same_destination_is_not_this_train() {
        let samples = vec![departure_at("19:02", "WOK", "19:05", false)];
        assert_eq!(
            find_darwin_eta(
                &samples,
                Some("WOK"),
                None,
                pin("2026-08-28T18:32:00+01:00"),
            ),
            None,
            "the 19:02 to WOK is a different service from the pinned 18:32 to WOK, and its \
             estimate must never be published as the pinned train's ETA"
        );
    }

    /// The other half of the same fix: with BOTH rows on the board -- the
    /// tracked train AND the following service to the same destination, in
    /// that order and then in the other -- the pinned service's own estimate
    /// is the one returned, whichever order the board happens to list them
    /// in. (Destination-only matching returned whichever came first.)
    #[test]
    fn the_pinned_service_is_picked_out_of_a_board_carrying_both() {
        let pinned = pin("2026-08-28T18:32:00+01:00");
        let expected: Option<DateTime<Utc>> = Some("2026-08-28T17:41:00Z".parse().unwrap());

        let following_first = vec![
            departure_at("19:02", "WOK", "19:05", false),
            departure_at("18:32", "WOK", "18:41", false),
        ];
        assert_eq!(
            find_darwin_eta(&following_first, Some("WOK"), None, pinned),
            expected
        );

        let pinned_first = vec![
            departure_at("18:32", "WOK", "18:41", false),
            departure_at("19:02", "WOK", "19:05", false),
        ];
        assert_eq!(
            find_darwin_eta(&pinned_first, Some("WOK"), None, pinned),
            expected
        );
    }

    /// The post-midnight dating bug, also from the 2026-09-25 review: a 23:58
    /// departure running seven minutes late has `estimated` `"00:05"`, which
    /// belongs to the day AFTER the pin's own. The old code built
    /// `service_date.and_time(estimated)` and dated it a full day early --
    /// an ETA roughly 24 hours in the past, which the frontend renders as a
    /// long-overdue train.
    #[test]
    fn a_post_midnight_estimate_is_dated_on_the_following_day() {
        let samples = vec![departure_at("23:58", "SOU", "00:05", false)];
        let eta = find_darwin_eta(
            &samples,
            Some("SOU"),
            None,
            pin("2026-08-28T23:58:00+01:00"),
        );
        assert_eq!(
            eta,
            Some("2026-08-28T23:05:00Z".parse().unwrap()),
            "00:05 BST on the 29th is 23:05Z on the 28th -- seven minutes after the pin, not \
             nearly a day before it"
        );
    }
}
