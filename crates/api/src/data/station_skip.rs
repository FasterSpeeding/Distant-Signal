//! Station-skip detection for a journey leg's own origin/destination --
//! §5.2 of docs/superpowers/specs/2026-09-22-journey-tracking-design.md.
//! Structurally mirrors eta_blend.rs's find_darwin_eta/blend_darwin_eta
//! split (a pure matching function, plus an async "fetch samples, apply
//! it" wrapper), reusing common::match_darwin_departure/
//! departure_skips_station so this exact matching/skip semantics is shared
//! with crates/notifier's own, independently-written implementation of the
//! same check (crates/notifier/src/skip_check.rs) -- that crate cannot
//! depend on this one, see this module's own Cargo dependency note in the
//! plan that added it.
//!
//! "The leg's own origin/destination", not the matched train's full-route
//! origin/destination -- see the design spec's §1.1 for why journey_legs
//! keeps its own origin_crs/destination_crs even once matched to a real
//! train. A skip 50 miles from either end of this traveller's leg is not
//! this traveller's problem (§5.2's own framing).

use common::{StationDeparture, departure_skips_station, match_darwin_departure};
use sqlx::PgPool;

use crate::data::queries;

/// Whether either end of one journey leg's own travel intent is among
/// today's Darwin-reported skipped calling points. `false`/`false` (never
/// an error) whenever there's simply no live sample to check against --
/// same best-effort posture as `blend_darwin_eta`'s own overlay; this is a
/// nice-to-have enhancement layered on a read, never something a read
/// route should fail over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LegSkipStatus {
    pub origin_skipped: bool,
    pub destination_skipped: bool,
}

impl LegSkipStatus {
    pub fn any(self) -> bool {
        self.origin_skipped || self.destination_skipped
    }
}

/// Pure core, no I/O: given the departure-board sample already fetched at
/// the leg's own `origin_crs` (`origin_board`), and -- only when the bound
/// train's own true point of origin differs from the leg's own origin (a
/// traveller boarding partway through a through-service) -- a second
/// sample fetched at that true origin (`train_origin_board`), decide
/// whether either end of this leg is among today's skipped calling points.
/// `match_target` is the same "which entry on this departure board is
/// actually the service we're tracking" key `find_darwin_eta` already
/// uses: the bound train's own pinned destination, or failing that its
/// current next calling point -- NOT `leg_destination_crs`, which may be
/// only an intermediate stop on the train's full route and so would never
/// match any departure board entry's own reported destination.
pub fn find_leg_skip(
    origin_board: &[StationDeparture],
    train_origin_board: Option<&[StationDeparture]>,
    match_target: Option<&str>,
    leg_origin_crs: &str,
    leg_destination_crs: &str,
) -> LegSkipStatus {
    let destination_skipped = match_darwin_departure(origin_board, match_target)
        .is_some_and(|matched| departure_skips_station(matched, leg_destination_crs));

    let origin_skipped = train_origin_board
        .and_then(|board| match_darwin_departure(board, match_target))
        .is_some_and(|matched| departure_skips_station(matched, leg_origin_crs));

    LegSkipStatus {
        origin_skipped,
        destination_skipped,
    }
}

/// Async wrapper: fetches the live sample(s) `find_leg_skip` needs and
/// applies it. `trains_id` is the leg's bound `train_subscriptions.trains_id`
/// -- used only to look up that train's own true `origin_crs`
/// (`trains.origin_crs`, nullable,
/// `crates/api/migrations/20260906100000_trains.sql:19`) for the symmetric
/// origin-skip check; never re-derives the leg's own origin/destination,
/// which the caller already has from its own `journey_legs` row. Any
/// failure to fetch a sample degrades to `LegSkipStatus::default()`
/// (both `false`) -- same best-effort posture as `blend_darwin_eta`.
pub async fn leg_skip_status(
    pool: &PgPool,
    trains_id: i64,
    leg_origin_crs: &str,
    leg_destination_crs: &str,
    match_target: Option<&str>,
) -> LegSkipStatus {
    let Ok(Some(origin_sample)) = queries::latest_station_sample(pool, leg_origin_crs).await else {
        return LegSkipStatus::default();
    };

    let train_true_origin_crs: Option<String> =
        sqlx::query_scalar("SELECT origin_crs FROM trains WHERE id = $1")
            .bind(trains_id)
            .fetch_optional(pool)
            .await
            .ok()
            .flatten()
            .flatten();

    let train_origin_sample = match train_true_origin_crs.as_deref() {
        Some(crs) if !crs.eq_ignore_ascii_case(leg_origin_crs) => {
            queries::latest_station_sample(pool, crs).await.ok().flatten()
        }
        _ => None,
    };

    find_leg_skip(
        &origin_sample.departures,
        train_origin_sample.as_ref().map(|s| s.departures.as_slice()),
        match_target,
        leg_origin_crs,
        leg_destination_crs,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn departure(destination_crs: &str, skipped: Vec<&str>) -> StationDeparture {
        StationDeparture {
            service_id: "test".to_string(),
            operator: "SW".to_string(),
            destination_crs: destination_crs.to_string(),
            scheduled: "18:32".to_string(),
            estimated: "18:41".to_string(),
            is_cancelled: false,
            delay_minutes: 0,
            cancel_reason: None,
            delay_reason: None,
            headcode: None,
            skipped_stations: skipped.into_iter().map(str::to_string).collect(),
            // Skip detection never reads the platform fields -- `None`
            // ("platform not known") rather than a fabricated value.
            platform: None,
            planned_platform: None,
        }
    }

    #[test]
    fn no_skip_when_the_matched_entry_reports_nothing_skipped() {
        let origin_board = vec![departure("WAT", vec![])];
        let status = find_leg_skip(&origin_board, None, Some("WAT"), "RDG", "WOK");
        assert_eq!(status, LegSkipStatus::default());
    }

    #[test]
    fn destination_skip_is_detected_from_the_origin_board() {
        let origin_board = vec![departure("WAT", vec!["WOK"])];
        let status = find_leg_skip(&origin_board, None, Some("WAT"), "RDG", "WOK");
        assert!(status.destination_skipped);
        assert!(!status.origin_skipped);
        assert!(status.any());
    }

    #[test]
    fn origin_skip_is_only_checked_when_a_train_origin_board_is_supplied() {
        let origin_board = vec![departure("WAT", vec![])];
        let train_origin_board = vec![departure("WAT", vec!["RDG"])];
        let status = find_leg_skip(
            &origin_board,
            Some(&train_origin_board),
            Some("WAT"),
            "RDG",
            "WOK",
        );
        assert!(status.origin_skipped);
        assert!(!status.destination_skipped);
    }

    #[test]
    fn no_match_target_means_nothing_can_ever_be_flagged() {
        let origin_board = vec![departure("WAT", vec!["WOK"])];
        let status = find_leg_skip(&origin_board, None, None, "RDG", "WOK");
        assert_eq!(status, LegSkipStatus::default());
    }

    #[test]
    fn a_cancelled_service_never_flags_a_skip_here_either() {
        // Whole-service cancellation is a separate, already-handled signal
        // (train_current_state.status) -- see match_darwin_departure's own
        // "never matches a cancelled departure" test in crates/common.
        let mut cancelled = departure("WAT", vec!["WOK"]);
        cancelled.is_cancelled = true;
        let origin_board = vec![cancelled];
        let status = find_leg_skip(&origin_board, None, Some("WAT"), "RDG", "WOK");
        assert_eq!(status, LegSkipStatus::default());
    }
}
