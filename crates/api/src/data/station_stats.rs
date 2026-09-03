//! Per-(station, operator) `SampleStats`, computed on demand from
//! `station_samples` at read time -- Option C from
//! docs/superpowers/specs/2026-09-03-per-station-stats-research.md.
//! No new table, no new aggregator write path: reuses
//! `queries::latest_station_sample` (already shipping, backs
//! `eta_blend.rs`) and `common::compute_sample_stats` (promoted for this
//! purpose -- design doc Decision 5).

use std::collections::BTreeSet;

use common::{Defaults, SampleAvailability, StationDeparture, StationSample};

/// One operator's sample-derived stats at one station. `NoCoverage` never
/// appears here by construction -- the caller (the route handler) only
/// invokes this once it already knows `station_samples` has a row for
/// this CRS at all (design doc Decision 7's 404 gate covers the
/// no-row-at-all case one level up).
pub struct OperatorSampleStats {
    pub operator: String,
    pub availability: SampleAvailability,
}

/// One entry per distinct `operator` value observed in `sample`'s current
/// departures -- not every ATOC code this app knows about, only the ones
/// with at least one departure on today's board right now. An operator
/// with zero current departures has nothing to report and is not listed,
/// the same way a line with no `sample_stations` row for a CRS wouldn't
/// invent a `BelowThreshold { observed: 0, .. }` entry for a station it
/// doesn't cover. Sorted alphabetically by ATOC code (via `BTreeSet`) for
/// deterministic wire output, mirroring `dedup_sample_stations`'s
/// (`crates/api/src/data/samples.rs:11-23`) own rationale.
pub fn compute_station_operator_stats(
    sample: &StationSample,
    defaults: &Defaults,
) -> Vec<OperatorSampleStats> {
    let operators: BTreeSet<&str> = sample
        .departures
        .iter()
        .map(|d| d.operator.as_str())
        .collect();

    operators
        .into_iter()
        .map(|operator| {
            let relevant: Vec<&StationDeparture> = sample
                .departures
                .iter()
                .filter(|d| d.operator == operator)
                .collect();
            let availability = if (relevant.len() as i64) < defaults.min_sample_size {
                SampleAvailability::BelowThreshold {
                    observed: relevant.len(),
                    required: defaults.min_sample_size,
                }
            } else {
                let stats = common::compute_sample_stats(
                    &relevant,
                    defaults.delay_threshold_minutes,
                    |d| d.skipped_stations.iter().any(|crs| crs == &sample.crs),
                );
                SampleAvailability::Available(stats)
            };
            OperatorSampleStats {
                operator: operator.to_string(),
                availability,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn departure(
        operator: &str,
        delay_minutes: i32,
        is_cancelled: bool,
        skipped_stations: Vec<&str>,
    ) -> StationDeparture {
        StationDeparture {
            service_id: "svc".to_string(),
            operator: operator.to_string(),
            destination_crs: "WAT".to_string(),
            scheduled: "10:00".to_string(),
            estimated: "10:00".to_string(),
            is_cancelled,
            delay_minutes,
            cancel_reason: if is_cancelled {
                Some("fault".to_string())
            } else {
                None
            },
            delay_reason: None,
            headcode: None,
            skipped_stations: skipped_stations.into_iter().map(str::to_string).collect(),
        }
    }

    fn sample(crs: &str, departures: Vec<StationDeparture>) -> StationSample {
        StationSample {
            crs: crs.to_string(),
            polled_at: chrono::Utc::now(),
            departures,
        }
    }

    #[test]
    fn two_operators_one_above_and_one_below_threshold_are_both_reported_alphabetically() {
        let defaults = Defaults {
            min_sample_size: 2,
            ..Defaults::default()
        };
        let sample = sample(
            "EDB",
            vec![
                departure("SR", 0, false, vec![]),
                departure("GR", 0, false, vec![]),
                departure("GR", 0, false, vec![]),
            ],
        );

        let stats = compute_station_operator_stats(&sample, &defaults);

        assert_eq!(stats.len(), 2);
        // Alphabetical: GR before SR.
        assert_eq!(stats[0].operator, "GR");
        assert!(matches!(
            stats[0].availability,
            SampleAvailability::Available(_)
        ));
        assert_eq!(stats[1].operator, "SR");
        assert!(matches!(
            stats[1].availability,
            SampleAvailability::BelowThreshold {
                observed: 1,
                required: 2
            }
        ));
    }

    #[test]
    fn empty_departures_yields_empty_vec_not_a_panic_or_synthetic_entry() {
        let defaults = Defaults::default();
        let sample = sample("EDB", vec![]);

        let stats = compute_station_operator_stats(&sample, &defaults);

        assert!(stats.is_empty());
    }

    #[test]
    fn skipped_stations_matching_this_crs_counts_toward_skipped() {
        let defaults = Defaults {
            min_sample_size: 1,
            ..Defaults::default()
        };
        let sample = sample("EDB", vec![departure("GR", 0, false, vec!["EDB"])]);

        let stats = compute_station_operator_stats(&sample, &defaults);

        assert_eq!(stats.len(), 1);
        match &stats[0].availability {
            SampleAvailability::Available(s) => assert_eq!(s.skipped, 1),
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn skipped_stations_matching_a_different_crs_does_not_count() {
        let defaults = Defaults {
            min_sample_size: 1,
            ..Defaults::default()
        };
        // This departure skips some other station on the route, not the
        // one being asked about -- Decision 4's per-station (not
        // per-route) definition means this must not count as skipped.
        let sample = sample("EDB", vec![departure("GR", 0, false, vec!["HYM"])]);

        let stats = compute_station_operator_stats(&sample, &defaults);

        assert_eq!(stats.len(), 1);
        match &stats[0].availability {
            SampleAvailability::Available(s) => assert_eq!(s.skipped, 0),
            other => panic!("expected Available, got {other:?}"),
        }
    }

    #[test]
    fn a_cancelled_departure_with_a_matching_skipped_station_is_not_double_counted() {
        let defaults = Defaults {
            min_sample_size: 1,
            ..Defaults::default()
        };
        let sample = sample("EDB", vec![departure("GR", 0, true, vec!["EDB"])]);

        let stats = compute_station_operator_stats(&sample, &defaults);

        assert_eq!(stats.len(), 1);
        match &stats[0].availability {
            SampleAvailability::Available(s) => {
                assert_eq!(s.cancelled, 1);
                assert_eq!(s.skipped, 0);
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }
}
