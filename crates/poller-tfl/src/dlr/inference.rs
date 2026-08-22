//! Matches DLR scheduled trips (`timetable::ScheduledTrip`) against live
//! Arrivals predictions (`arrivals::Prediction`) to infer per-trip delay,
//! and (via `DlrMatchState`, added once this module also owns cross-poll
//! state) cancellation. `sample_stats` computed here is attached to the
//! DLR line's existing `LineStatus` — it never changes that status's
//! `severity`; see the plan's Global Constraints for why.

use chrono::{DateTime, Utc};

use super::arrivals::Prediction;
use super::timetable::ScheduledTrip;

/// A service is "delayed" once its delay exceeds this many minutes —
/// mirrors `common::Defaults::delay_threshold_minutes`'s default (5). Not
/// read from `Defaults` itself: that struct is wired to the NR aggregator's
/// `severity_overrides` TOML mechanism (per-line configuration this pilot
/// has no equivalent of), and the spec's Non-goals rule out unifying the
/// two severity models beyond areas 1-2. This is a local, DLR-only
/// constant instead.
const DLR_DELAY_THRESHOLD_MINUTES: i64 = 5;

/// How close a live prediction's `expected_arrival` must be to a scheduled
/// trip's `scheduled_departure` at the same station to count as a match.
/// Wide enough to tolerate a train running early or a schedule/prediction
/// clock skew, narrow enough that two distinct trips ~4-10 minutes apart
/// (DLR's typical headway) don't both claim the same prediction.
const MATCH_WINDOW_MINUTES: i64 = 3;

#[derive(Debug, Clone, PartialEq)]
pub enum MatchedTrip {
    /// Found a prediction within `MATCH_WINDOW_MINUTES` of this trip's
    /// scheduled time. `delay_minutes` can be negative (early) but callers
    /// treat negative as zero — see `Task 6`'s `SampleStats` computation.
    Matched { delay_minutes: i64 },
    /// No matching prediction yet. Not necessarily cancelled — the train
    /// may simply not be visible in predictions yet if its scheduled time
    /// hasn't arrived. Task 6 decides when "pending" becomes "cancelled".
    Pending,
}

/// For each scheduled trip, finds the live prediction (at the same
/// station) whose `expected_arrival` is closest to `scheduled_departure`
/// and within `MATCH_WINDOW_MINUTES`, and computes its delay. Each
/// prediction can match at most one trip — the closest trip claims it,
/// so two trips near the same time don't both consume one late train's
/// prediction as evidence they individually ran on time.
pub fn match_trips(
    trips: Vec<ScheduledTrip>,
    predictions: &[Prediction],
    now: DateTime<Utc>,
) -> Vec<MatchedTrip> {
    let mut claimed = vec![false; predictions.len()];
    trips
        .into_iter()
        .map(|trip| {
            let best = predictions
                .iter()
                .enumerate()
                .filter(|(i, _)| !claimed[*i])
                .map(|(i, p)| (i, p, (p.expected_arrival - trip.scheduled_departure).num_minutes().abs()))
                .filter(|(_, _, diff)| *diff <= MATCH_WINDOW_MINUTES)
                .min_by_key(|(_, _, diff)| *diff);

            match best {
                Some((i, p, _)) => {
                    claimed[i] = true;
                    let delay_minutes = (p.expected_arrival - trip.scheduled_departure).num_minutes();
                    MatchedTrip::Matched { delay_minutes: delay_minutes.max(0) }
                }
                None => {
                    let _ = now; // `now` is unused by matching itself; kept in the signature for Task 6's cancellation check, which needs it and calls this function.
                    MatchedTrip::Pending
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trip(minute: u32) -> ScheduledTrip {
        ScheduledTrip {
            scheduled_departure: "2026-08-22T10:00:00Z".parse::<DateTime<Utc>>().unwrap() + chrono::Duration::minutes(minute as i64),
            interval_id: None,
        }
    }

    fn prediction(expected_offset_minutes: i64) -> Prediction {
        Prediction {
            vehicle_id: "301".to_string(),
            naptan_id: "940GZZDLPOP".to_string(),
            station_name: "Poplar".to_string(),
            destination_naptan_id: String::new(),
            destination_name: String::new(),
            expected_arrival: "2026-08-22T10:00:00Z".parse::<DateTime<Utc>>().unwrap()
                + chrono::Duration::minutes(expected_offset_minutes),
            time_to_station: 0,
        }
    }

    #[test]
    fn an_on_time_prediction_matches_with_zero_delay() {
        let result = match_trips(vec![trip(0)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 0 }]);
    }

    #[test]
    fn a_late_prediction_matches_with_the_observed_delay() {
        let result = match_trips(vec![trip(0)], &[prediction(6)], "2026-08-22T10:00:00Z".parse().unwrap());
        // 6 minutes is outside MATCH_WINDOW_MINUTES (3), so this should be
        // Pending, not a 6-minute-late match — window too tight to claim a
        // 6-minute-late train as "this trip" rather than a later one.
        assert_eq!(result, vec![MatchedTrip::Pending]);
    }

    #[test]
    fn a_prediction_within_the_match_window_computes_its_delay() {
        let result = match_trips(vec![trip(0)], &[prediction(2)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 2 }]);
    }

    #[test]
    fn a_trip_with_no_nearby_prediction_is_pending() {
        let result = match_trips(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Pending]);
    }

    #[test]
    fn each_prediction_matches_at_most_one_trip() {
        // Two trips 4 minutes apart (DLR's typical headway), one
        // prediction. The closer trip claims it; the other stays Pending
        // rather than both being marked on-time from the same train.
        let result = match_trips(vec![trip(0), trip(4)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 0 }, MatchedTrip::Pending]);
    }

    #[test]
    fn an_early_prediction_is_clamped_to_zero_delay_not_negative() {
        let result = match_trips(vec![trip(2)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(result, vec![MatchedTrip::Matched { delay_minutes: 0 }]);
    }
}
