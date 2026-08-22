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

/// A scheduled trip still waiting for a matching prediction, remembered
/// across poll cycles so `resolve` can tell "genuinely never showed up"
/// from "hasn't happened yet".
#[derive(Debug, Clone)]
struct PendingTrip {
    scheduled_departure: DateTime<Utc>,
}

/// A trip that has been resolved one way or the other, kept for
/// `RESOLVED_RETENTION_MINUTES` so `SampleStats` reflects a rolling recent
/// window rather than every trip since the poller started.
#[derive(Debug, Clone)]
struct ResolvedTrip {
    resolved_at: DateTime<Utc>,
    delay_minutes: Option<i64>, // None means cancelled
}

/// A trip pending longer than this past its scheduled time, with no
/// matching prediction ever found, is treated as cancelled. DLR's typical
/// headway is 3-10 minutes; this is roughly two headways' grace so a
/// train that's simply running very late doesn't get misread as
/// cancelled — a pilot-tuned value, not derived from any published TfL
/// number, and worth revisiting once real data is observed.
const CANCELLATION_GRACE_MINUTES: i64 = 15;

/// How long a resolved trip counts toward the reported `SampleStats`
/// before aging out. An hour gives a stable-enough sample size at DLR's
/// headway (roughly 6-20 trips) without the reported numbers describing
/// disruption from hours ago as if it were still happening.
const RESOLVED_RETENTION_MINUTES: i64 = 60;

pub struct DlrMatchState {
    pending: Vec<PendingTrip>,
    resolved: Vec<ResolvedTrip>,
}

impl DlrMatchState {
    pub fn new() -> Self {
        DlrMatchState { pending: Vec::new(), resolved: Vec::new() }
    }

    /// Runs one poll cycle: adds newly-seen scheduled trips to the pending
    /// set (skipping ones already tracked, by `scheduled_departure`),
    /// matches everything pending against this cycle's predictions,
    /// promotes newly-matched or grace-window-expired trips into
    /// `resolved`, evicts resolved trips older than
    /// `RESOLVED_RETENTION_MINUTES`, and returns the resulting
    /// `SampleStats` — `None` if nothing has resolved yet (e.g. right
    /// after startup).
    pub fn resolve(
        &mut self,
        trips: Vec<ScheduledTrip>,
        predictions: &[Prediction],
        now: DateTime<Utc>,
    ) -> Option<common::SampleStats> {
        let known: std::collections::HashSet<DateTime<Utc>> =
            self.pending.iter().map(|t| t.scheduled_departure).collect();
        for trip in trips.iter().filter(|t| !known.contains(&t.scheduled_departure)) {
            self.pending.push(PendingTrip { scheduled_departure: trip.scheduled_departure });
        }

        let pending_as_trips: Vec<ScheduledTrip> = self
            .pending
            .iter()
            .map(|p| ScheduledTrip { scheduled_departure: p.scheduled_departure, interval_id: None })
            .collect();
        let matches = match_trips(pending_as_trips, predictions, now);

        let mut still_pending = Vec::new();
        for (pending_trip, matched) in self.pending.drain(..).zip(matches) {
            match matched {
                MatchedTrip::Matched { delay_minutes } => {
                    self.resolved.push(ResolvedTrip { resolved_at: now, delay_minutes: Some(delay_minutes) });
                }
                MatchedTrip::Pending => {
                    let overdue = (now - pending_trip.scheduled_departure).num_minutes();
                    if overdue >= CANCELLATION_GRACE_MINUTES {
                        self.resolved.push(ResolvedTrip { resolved_at: now, delay_minutes: None });
                    } else {
                        still_pending.push(pending_trip);
                    }
                }
            }
        }
        self.pending = still_pending;

        self.resolved.retain(|r| (now - r.resolved_at).num_minutes() < RESOLVED_RETENTION_MINUTES);

        if self.resolved.is_empty() {
            return None;
        }

        let total = self.resolved.len();
        let cancelled = self.resolved.iter().filter(|r| r.delay_minutes.is_none()).count();
        let running: Vec<i64> = self.resolved.iter().filter_map(|r| r.delay_minutes).collect();
        let delayed = running.iter().filter(|&&d| d >= DLR_DELAY_THRESHOLD_MINUTES).count();
        let avg_delay_minutes = if running.is_empty() {
            0.0
        } else {
            running.iter().sum::<i64>() as f64 / running.len() as f64
        };

        Some(common::SampleStats { total, delayed, cancelled, skipped: 0, avg_delay_minutes })
    }
}

impl Default for DlrMatchState {
    fn default() -> Self {
        Self::new()
    }
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

    #[test]
    fn a_matched_trip_resolves_immediately() {
        let mut state = DlrMatchState::new();
        let stats = state
            .resolve(vec![trip(0)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap())
            .expect("one resolved trip");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.cancelled, 0);
        assert_eq!(stats.delayed, 0);
    }

    #[test]
    fn a_trip_still_pending_within_the_grace_window_produces_no_stats_yet() {
        let mut state = DlrMatchState::new();
        let stats = state.resolve(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
        assert_eq!(stats, None);
    }

    #[test]
    fn a_trip_still_unmatched_past_the_grace_window_is_cancelled() {
        let mut state = DlrMatchState::new();
        // First cycle: trip scheduled for 10:00, no prediction yet.
        state.resolve(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
        // Second cycle, 16 minutes later: still nothing, past the 15-minute
        // grace window.
        let stats = state
            .resolve(vec![], &[], "2026-08-22T10:16:00Z".parse().unwrap())
            .expect("the overdue trip should have resolved as cancelled");
        assert_eq!(stats.total, 1);
        assert_eq!(stats.cancelled, 1);
    }

    #[test]
    fn resolved_trips_age_out_after_the_retention_window() {
        let mut state = DlrMatchState::new();
        state.resolve(vec![trip(0)], &[prediction(0)], "2026-08-22T10:00:00Z".parse().unwrap());
        // 61 minutes later, with no new trips at all: the earlier resolved
        // trip should have aged out, leaving nothing to report.
        let stats = state.resolve(vec![], &[], "2026-08-22T11:01:00Z".parse().unwrap());
        assert_eq!(stats, None);
    }

    #[test]
    fn a_trip_already_pending_is_not_added_twice_on_the_next_cycle() {
        let mut state = DlrMatchState::new();
        state.resolve(vec![trip(0)], &[], "2026-08-22T10:00:00Z".parse().unwrap());
        // Same trip handed in again next cycle (the timetable poll always
        // returns the same day's full schedule) — must not be double-counted.
        let stats = state
            .resolve(vec![trip(0)], &[prediction(0)], "2026-08-22T10:01:00Z".parse().unwrap())
            .expect("the trip should resolve exactly once");
        assert_eq!(stats.total, 1);
    }
}
