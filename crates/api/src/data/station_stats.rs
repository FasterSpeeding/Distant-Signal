//! Per-(station, operator) `SampleStats`, computed on demand from
//! `station_samples` at read time -- Option C from
//! docs/superpowers/specs/2026-09-03-per-station-stats-research.md.
//! No new table, no new aggregator write path: reuses
//! `queries::latest_station_sample` (already shipping, backs
//! `eta_blend.rs`) and `common::compute_sample_stats` (promoted for this
//! purpose -- design doc Decision 5).

use std::collections::BTreeSet;

use common::{
    Defaults, FullCoverageAvailability, LineDefinition, SampleAvailability, SampleStats,
    StationDeparture, StationFullCoverageSample, StationSample,
};

/// One operator's sample-derived stats at one station. `NoCoverage` never
/// appears here by construction -- the caller (the route handler) only
/// invokes this once it already knows `station_samples` has a row for
/// this CRS at all (design doc Decision 7's 404 gate covers the
/// no-row-at-all case one level up).
pub struct OperatorSampleStats {
    pub operator: String,
    pub availability: SampleAvailability,
    pub full_coverage_stats: Option<SampleStats>,
    pub full_coverage_availability: FullCoverageAvailability,
}

/// Whether a future full-coverage consumer would ever be expected to
/// resolve a signal for this (station, operator) pair -- derived
/// dynamically from `LineDefinition.full_coverage_enabled`
/// (`crates/common/src/lib.rs:498`), the SAME per-line rollout flag the
/// line-level scaffolding already established
/// (docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
/// Decision 3). No new per-station config surface: a station's gate is
/// the union, over every line that (a) runs this operator and (b) calls
/// at this CRS, of that line's own flag.
///
/// Route membership (`line.stations`), not `line.sample_stations`, is the
/// right membership check here -- deliberately wider than the LDBWS-only
/// sample-stats path uses. A full-coverage consumer's whole premise (per
/// the deferred doc's Decision 3 "granularity" argument) is "every
/// scheduled service on the line," not the curated 2-5-station LDBWS
/// subset, so gating on the curated list would under-scope exactly the
/// stations full coverage is meant to newly reach.
fn full_coverage_enabled_for(crs: &str, operator: &str, lines: &[LineDefinition]) -> bool {
    lines.iter().any(|line| {
        line.full_coverage_enabled
            && line.operators.iter().any(|op| op == operator)
            && line.stations.iter().any(|s| s.crs == crs)
    })
}

/// One entry per distinct `operator` value observed in `sample`'s current
/// departures, UNIONED with every operator that has a `full_coverage_rows`
/// entry for this station (design doc Decision 4) -- not every ATOC code
/// this app knows about, only the ones with at least one departure on
/// today's board right now OR a resolved full-coverage row. A station can
/// have a real full-coverage row for an operator with zero current LDBWS
/// departures, since full coverage's route-membership gate
/// (`full_coverage_enabled_for`) is structurally wider than LDBWS's
/// `sample_stations`-only reach -- the union, not intersection, is
/// deliberate. Sorted alphabetically by ATOC code (via `BTreeSet`) for
/// deterministic wire output, mirroring `dedup_sample_stations`'s
/// (`crates/api/src/data/samples.rs:11-23`) own rationale.
pub fn compute_station_operator_stats(
    sample: &StationSample,
    defaults: &Defaults,
    full_coverage_rows: &[StationFullCoverageSample],
    lines: &[LineDefinition],
) -> Vec<OperatorSampleStats> {
    let operators: BTreeSet<&str> = sample
        .departures
        .iter()
        .map(|d| d.operator.as_str())
        .chain(full_coverage_rows.iter().map(|r| r.operator.as_str()))
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

            let full_coverage_availability =
                if !full_coverage_enabled_for(&sample.crs, operator, lines) {
                    FullCoverageAvailability::NotEnabled
                } else {
                    match full_coverage_rows.iter().find(|r| r.operator == operator) {
                        Some(row) => FullCoverageAvailability::Available(row.stats.clone()),
                        None => FullCoverageAvailability::Pending,
                    }
                };
            // Reuses the existing accessor (lib.rs:858-863) rather than
            // re-deriving the same match a second time.
            let full_coverage_stats = full_coverage_availability.full_coverage_stats();

            OperatorSampleStats {
                operator: operator.to_string(),
                availability,
                full_coverage_stats,
                full_coverage_availability,
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

        let stats = compute_station_operator_stats(&sample, &defaults, &[], &[]);

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

        let stats = compute_station_operator_stats(&sample, &defaults, &[], &[]);

        assert!(stats.is_empty());
    }

    #[test]
    fn skipped_stations_matching_this_crs_counts_toward_skipped() {
        let defaults = Defaults {
            min_sample_size: 1,
            ..Defaults::default()
        };
        let sample = sample("EDB", vec![departure("GR", 0, false, vec!["EDB"])]);

        let stats = compute_station_operator_stats(&sample, &defaults, &[], &[]);

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

        let stats = compute_station_operator_stats(&sample, &defaults, &[], &[]);

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

        let stats = compute_station_operator_stats(&sample, &defaults, &[], &[]);

        assert_eq!(stats.len(), 1);
        match &stats[0].availability {
            SampleAvailability::Available(s) => {
                assert_eq!(s.cancelled, 1);
                assert_eq!(s.skipped, 0);
            }
            other => panic!("expected Available, got {other:?}"),
        }
    }

    fn line(
        id: &str,
        operators: &[&str],
        stations: &[&str],
        full_coverage_enabled: bool,
    ) -> LineDefinition {
        LineDefinition {
            id: id.to_string(),
            name: id.to_string(),
            mode: "national-rail".to_string(),
            category: "main-line".to_string(),
            operators: operators.iter().map(|s| s.to_string()).collect(),
            stations: stations
                .iter()
                .map(|crs| common::Station {
                    crs: crs.to_string(),
                    tiploc: None,
                    role: "minor".to_string(),
                    segment: None,
                })
                .collect(),
            sample_stations: vec![],
            match_keywords: vec![],
            excluded_keywords: vec![],
            severity_overrides: Default::default(),
            exclusive_segments: vec![],
            destination_crs_filter: vec![],
            headcode_prefixes: vec![],
            full_coverage_enabled,
        }
    }

    fn full_coverage_row(
        crs: &str,
        operator: &str,
        stats: common::SampleStats,
    ) -> StationFullCoverageSample {
        StationFullCoverageSample {
            crs: crs.to_string(),
            operator: operator.to_string(),
            resolved_at: chrono::Utc::now(),
            stats,
        }
    }

    fn sample_stats(total: usize) -> common::SampleStats {
        common::SampleStats {
            total,
            delayed: 0,
            cancelled: 0,
            skipped: 0,
            avg_delay_minutes: 0.0,
        }
    }

    #[test]
    fn full_coverage_enabled_for_true_only_when_a_line_covers_both_the_crs_and_the_operator() {
        // Covers this CRS but not this operator -> false.
        let lines = vec![line("L1", &["SR"], &["EDB"], true)];
        assert!(!full_coverage_enabled_for("EDB", "GR", &lines));

        // Covers this operator but at a different station -> false.
        let lines = vec![line("L1", &["GR"], &["WAT"], true)];
        assert!(!full_coverage_enabled_for("EDB", "GR", &lines));

        // Two lines cover this (crs, operator); only one is enabled -> true
        // (union over every covering line).
        let lines = vec![
            line("L1", &["GR"], &["EDB"], false),
            line("L2", &["GR"], &["EDB"], true),
        ];
        assert!(full_coverage_enabled_for("EDB", "GR", &lines));
    }

    #[test]
    fn enabled_line_with_no_matching_row_yields_pending() {
        let defaults = Defaults {
            min_sample_size: 1,
            ..Defaults::default()
        };
        let sample = sample("EDB", vec![departure("GR", 0, false, vec![])]);
        let lines = vec![line("L1", &["GR"], &["EDB"], true)];

        let stats = compute_station_operator_stats(&sample, &defaults, &[], &lines);

        assert_eq!(stats.len(), 1);
        assert_eq!(
            stats[0].full_coverage_availability,
            FullCoverageAvailability::Pending
        );
        assert!(stats[0].full_coverage_stats.is_none());
    }

    #[test]
    fn enabled_line_with_a_matching_row_yields_available_and_the_accessor_round_trips() {
        let defaults = Defaults {
            min_sample_size: 1,
            ..Defaults::default()
        };
        let sample = sample("EDB", vec![departure("GR", 0, false, vec![])]);
        let lines = vec![line("L1", &["GR"], &["EDB"], true)];
        let row_stats = sample_stats(52);
        let rows = vec![full_coverage_row("EDB", "GR", row_stats.clone())];

        let stats = compute_station_operator_stats(&sample, &defaults, &rows, &lines);

        assert_eq!(stats.len(), 1);
        assert_eq!(
            stats[0].full_coverage_availability,
            FullCoverageAvailability::Available(row_stats.clone())
        );
        assert_eq!(stats[0].full_coverage_stats, Some(row_stats));
    }

    #[test]
    fn no_line_has_full_coverage_enabled_yields_not_enabled_even_with_a_stray_row_present() {
        let defaults = Defaults {
            min_sample_size: 1,
            ..Defaults::default()
        };
        let sample = sample("EDB", vec![departure("GR", 0, false, vec![])]);
        // A line covers this (crs, operator) but full_coverage_enabled is
        // false -- the real, current state of every catalogued line today.
        let lines = vec![line("L1", &["GR"], &["EDB"], false)];
        // A stray full-coverage row exists anyway -- must not flip the gate.
        let rows = vec![full_coverage_row("EDB", "GR", sample_stats(10))];

        let stats = compute_station_operator_stats(&sample, &defaults, &rows, &lines);

        assert_eq!(stats.len(), 1);
        assert_eq!(
            stats[0].full_coverage_availability,
            FullCoverageAvailability::NotEnabled
        );
        assert!(stats[0].full_coverage_stats.is_none());
    }

    #[test]
    fn an_operator_with_a_full_coverage_row_but_zero_ldbws_departures_still_appears_union_not_intersection()
     {
        let defaults = Defaults::default();
        // No departures at all in the sample.
        let sample = sample("EDB", vec![]);
        let lines = vec![line("L1", &["GR"], &["EDB"], true)];
        let rows = vec![full_coverage_row("EDB", "GR", sample_stats(30))];

        let stats = compute_station_operator_stats(&sample, &defaults, &rows, &lines);

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].operator, "GR");
        assert!(matches!(
            stats[0].availability,
            SampleAvailability::BelowThreshold { observed: 0, .. }
        ));
        assert_eq!(
            stats[0].full_coverage_availability,
            FullCoverageAvailability::Available(sample_stats(30))
        );
    }
}
