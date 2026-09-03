//! Render `common::LineStatusReport`/`LineStatus` as TfL-shaped JSON.
//! Ported from `src/render.py`. Deliberately independent of any
//! `#[serde(rename)]` on the stored types — the internal storage
//! representation (however `LineStatus` happens to serialize by default)
//! and the public TfL response shape are different concerns; this module
//! is the only place that knows the public shape, exactly like the
//! Python original builds its response dict by hand rather than relying
//! on dataclass field names.

use chrono::{DateTime, Utc};
use common::{LineStatus, LineStatusReport, Severity};
use serde_json::{Value, json};

pub fn to_tfl_shape(report: &LineStatusReport, computed_at: DateTime<Utc>, detail: bool) -> Value {
    json!({
        "$type": "DistantSignal.LineStatusReport",
        "id": report.id,
        "name": report.name,
        "modeName": report.mode_name,
        "operators": report.operators,
        "computedAt": computed_at.to_rfc3339(),
        "lineStatuses": report.statuses.iter().map(|s| status_to_json(s, detail)).collect::<Vec<_>>(),
    })
}

/// Like `to_tfl_shape`, but attaches a second line's current statuses under
/// a `tflStatus` field when `tfl_overlay` is `Some`. Used only by the
/// single-line detail endpoint (`routes/line_status.rs::get_line_status`)
/// for lines with a TfL counterpart merged away from `/public/lines` --
/// see `docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md`
/// Area 1. `tfl_overlay`'s statuses are rendered through the same
/// `status_to_json` as the primary line, unchanged, so this never
/// constructs new `reason` text -- see that spec's hard constraint.
pub fn to_tfl_shape_with_overlay(
    report: &LineStatusReport,
    computed_at: DateTime<Utc>,
    detail: bool,
    tfl_overlay: Option<&[LineStatus]>,
) -> Value {
    let mut out = to_tfl_shape(report, computed_at, detail);
    if let Some(statuses) = tfl_overlay {
        out["tflStatus"] =
            Value::Array(statuses.iter().map(|s| status_to_json(s, detail)).collect());
    }
    out
}

fn status_to_json(status: &LineStatus, detail: bool) -> Value {
    let mut out = json!({
        "statusSeverity": status.severity as i32,
        "statusSeverityDescription": severity_description(status.severity),
        "reason": status.reason,
        "dataQuality": status.data_quality,
        "validityPeriods": [
            {
                "fromDate": status.validity.from_date.to_rfc3339(),
                "toDate": status.validity.to_date.map(|d| d.to_rfc3339()),
                "isNow": status.validity.is_now,
            }
        ],
    });

    if let Some(stats) = &status.sample_stats {
        out["sampleStats"] = sample_stats_json(stats);
    }

    out["sampleAvailability"] = sample_availability_json(&status.sample_availability);

    if let Some(stats) = &status.full_coverage_stats {
        out["fullCoverageStats"] = sample_stats_json(stats);
    }

    out["fullCoverageAvailability"] =
        full_coverage_availability_json(&status.full_coverage_availability);

    if detail && let Some(disruption) = &status.disruption {
        out["disruption"] = json!({
            "category": disruption.category,
            "description": disruption.description,
            "affectedStops": disruption.affected_stops,
            "affectedRoutes": disruption.affected_routes.iter().map(|r| json!({
                "from": r.from_crs,
                "to": r.to_crs,
            })).collect::<Vec<_>>(),
            "source": disruption.source,
            "impactType": disruption.impact_type,
        });
    }

    out
}

fn severity_description(severity: Severity) -> &'static str {
    severity.description()
}

pub(crate) fn sample_stats_json(stats: &common::SampleStats) -> Value {
    json!({
        "total": stats.total,
        "delayed": stats.delayed,
        "cancelled": stats.cancelled,
        "skipped": stats.skipped,
        "avgDelayMinutes": stats.avg_delay_minutes,
    })
}

pub(crate) fn sample_availability_json(availability: &common::SampleAvailability) -> Value {
    match availability {
        common::SampleAvailability::NoCoverage => json!({ "state": "no-coverage" }),
        common::SampleAvailability::BelowThreshold { observed, required } => {
            json!({ "state": "below-threshold", "observed": observed, "required": required })
        }
        common::SampleAvailability::Available(_) => json!({ "state": "available" }),
    }
}

/// Full-coverage analog of `sample_availability_json` -- same "never
/// duplicate the SampleStats payload a second time on the wire" posture
/// (`full_coverage_stats` above already carries it when present).
pub(crate) fn full_coverage_availability_json(
    availability: &common::FullCoverageAvailability,
) -> Value {
    match availability {
        common::FullCoverageAvailability::NotEnabled => json!({ "state": "not-enabled" }),
        common::FullCoverageAvailability::Pending => json!({ "state": "pending" }),
        common::FullCoverageAvailability::Available(_) => json!({ "state": "available" }),
    }
}

/// Hand-built camelCase JSON for one `common::StationDeparture` row, backing
/// `GET /public/stations/{crs}/departures`
/// (`docs/superpowers/specs/2026-09-03-trip-search-design.md` Decision 2).
/// Same rationale as `sample_stats_json`/`sample_availability_json` above:
/// a `#[serde(rename_all = "camelCase")]` wrapper around `StationDeparture`
/// directly would still emit its own un-renamed field names one level down
/// (`incidents.rs:53-59`'s documented pitfall). `headcode` is deliberately
/// omitted -- always `None` at the source
/// (`poller-ldbws/src/schema.rs:104-105`), and `TrackPinRequest` has no
/// field for it anyway.
pub(crate) fn station_departure_json(d: &common::StationDeparture) -> Value {
    json!({
        "serviceId": d.service_id,
        "operator": d.operator,
        "destinationCrs": d.destination_crs,
        "scheduled": d.scheduled,
        "estimated": d.estimated,
        "isCancelled": d.is_cancelled,
        "delayMinutes": d.delay_minutes,
        "cancelReason": d.cancel_reason,
        "delayReason": d.delay_reason,
        "skippedStations": d.skipped_stations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use common::{DataQuality, Disruption, SampleAvailability, SampleStats, ValidityPeriod};

    fn sample_report(disruption: Option<Disruption>) -> LineStatusReport {
        LineStatusReport {
            id: "wcml".to_string(),
            name: "West Coast Main Line".to_string(),
            mode_name: "national-rail".to_string(),
            operators: vec!["VT".to_string()],
            statuses: vec![LineStatus {
                severity: Severity::MinorDelays,
                reason: "Signal failure".to_string(),
                validity: ValidityPeriod {
                    from_date: Utc::now(),
                    to_date: None,
                    is_now: true,
                },
                disruption,
                data_quality: DataQuality::Knowledgebase,
                sample_stats: None,
                sample_availability: SampleAvailability::NoCoverage,
                full_coverage_stats: None,
                full_coverage_availability: common::FullCoverageAvailability::NotEnabled,
            }],
        }
    }

    fn sample_computed_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap()
    }

    #[test]
    fn renders_computed_at() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(json["computedAt"], "2026-07-15T09:00:00+00:00");
    }

    #[test]
    fn renders_top_level_fields() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(json["$type"], "DistantSignal.LineStatusReport");
        assert_eq!(json["id"], "wcml");
        assert_eq!(json["name"], "West Coast Main Line");
        assert_eq!(json["modeName"], "national-rail");
        assert_eq!(json["operators"][0], "VT");
    }

    #[test]
    fn renders_status_fields() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        let status = &json["lineStatuses"][0];
        assert_eq!(status["statusSeverity"], 9);
        assert_eq!(status["statusSeverityDescription"], "Minor Delays");
        assert_eq!(status["reason"], "Signal failure");
        assert_eq!(status["dataQuality"], "knowledgebase");
        assert_eq!(status["validityPeriods"][0]["isNow"], true);
        assert!(status["validityPeriods"][0]["toDate"].is_null());
    }

    #[test]
    fn disruption_omitted_without_detail_flag() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure at Woking".to_string(),
            affected_stops: vec!["WOK".to_string()],
            affected_routes: vec![],
            source: Some("knowledgebase-incident-1".to_string()),
            impact_type: None,
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert!(json["lineStatuses"][0].get("disruption").is_none());
    }

    #[test]
    fn disruption_included_with_detail_flag() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure at Woking".to_string(),
            affected_stops: vec!["WOK".to_string()],
            affected_routes: vec![common::AffectedRoute {
                from_crs: "WAT".to_string(),
                to_crs: "WOK".to_string(),
            }],
            source: Some("knowledgebase-incident-1".to_string()),
            impact_type: Some("rail_replacement_bus".to_string()),
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, sample_computed_at(), true);
        let d = &json["lineStatuses"][0]["disruption"];
        assert_eq!(d["category"], "RealTime");
        assert_eq!(d["description"], "Signal failure at Woking");
        assert_eq!(d["affectedStops"][0], "WOK");
        assert_eq!(d["affectedRoutes"][0]["from"], "WAT");
        assert_eq!(d["affectedRoutes"][0]["to"], "WOK");
        assert_eq!(d["source"], "knowledgebase-incident-1");
        assert_eq!(d["impactType"], "rail_replacement_bus");
    }

    #[test]
    fn impact_type_renders_as_json_null_when_absent() {
        let disruption = Disruption {
            category: "RealTime".to_string(),
            description: "Signal failure".to_string(),
            affected_stops: vec![],
            affected_routes: vec![],
            source: None,
            impact_type: None,
        };
        let report = sample_report(Some(disruption));
        let json = to_tfl_shape(&report, sample_computed_at(), true);
        assert!(json["lineStatuses"][0]["disruption"]["impactType"].is_null());
    }

    #[test]
    fn no_disruption_present_even_with_detail_flag() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, sample_computed_at(), true);
        assert!(json["lineStatuses"][0].get("disruption").is_none());
    }

    #[test]
    fn sample_stats_included_when_present() {
        let mut report = sample_report(None);
        report.statuses[0].sample_stats = Some(SampleStats {
            total: 10,
            delayed: 4,
            cancelled: 1,
            skipped: 2,
            avg_delay_minutes: 6.5,
        });
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        let stats = &json["lineStatuses"][0]["sampleStats"];
        assert_eq!(stats["total"], 10);
        assert_eq!(stats["delayed"], 4);
        assert_eq!(stats["cancelled"], 1);
        assert_eq!(stats["skipped"], 2);
        assert_eq!(stats["avgDelayMinutes"], 6.5);
    }

    #[test]
    fn sample_stats_omitted_when_absent() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert!(json["lineStatuses"][0].get("sampleStats").is_none());
    }

    #[test]
    fn sample_availability_is_always_present_unlike_sample_stats() {
        let report = sample_report(None); // sample_stats is None
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(
            json["lineStatuses"][0]["sampleAvailability"],
            serde_json::json!({"state": "no-coverage"})
        );
    }

    #[test]
    fn sample_availability_below_threshold_shape() {
        let mut report = sample_report(None);
        report.statuses[0].sample_availability = SampleAvailability::BelowThreshold {
            observed: 2,
            required: 3,
        };
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(
            json["lineStatuses"][0]["sampleAvailability"],
            serde_json::json!({"state": "below-threshold", "observed": 2, "required": 3})
        );
    }

    #[test]
    fn sample_availability_available_case_does_not_duplicate_sample_stats_fields() {
        let mut report = sample_report(None);
        let stats = SampleStats {
            total: 10,
            delayed: 4,
            cancelled: 1,
            skipped: 2,
            avg_delay_minutes: 6.5,
        };
        report.statuses[0].sample_stats = Some(stats.clone());
        report.statuses[0].sample_availability = SampleAvailability::Available(stats);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(
            json["lineStatuses"][0]["sampleAvailability"],
            serde_json::json!({"state": "available"})
        );
        assert!(
            json["lineStatuses"][0]["sampleAvailability"]
                .get("total")
                .is_none(),
            "Available must not re-embed SampleStats fields"
        );
    }

    #[test]
    fn full_coverage_stats_included_when_present() {
        let mut report = sample_report(None);
        report.statuses[0].full_coverage_stats = Some(SampleStats {
            total: 20,
            delayed: 3,
            cancelled: 0,
            skipped: 1,
            avg_delay_minutes: 2.5,
        });
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        let stats = &json["lineStatuses"][0]["fullCoverageStats"];
        assert_eq!(stats["total"], 20);
        assert_eq!(stats["delayed"], 3);
        assert_eq!(stats["cancelled"], 0);
        assert_eq!(stats["skipped"], 1);
        assert_eq!(stats["avgDelayMinutes"], 2.5);
    }

    #[test]
    fn full_coverage_stats_omitted_when_absent() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert!(json["lineStatuses"][0].get("fullCoverageStats").is_none());
    }

    #[test]
    fn full_coverage_availability_is_always_present_unlike_full_coverage_stats() {
        let report = sample_report(None); // full_coverage_stats is None
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(
            json["lineStatuses"][0]["fullCoverageAvailability"],
            serde_json::json!({"state": "not-enabled"})
        );
    }

    #[test]
    fn full_coverage_availability_pending_shape() {
        let mut report = sample_report(None);
        report.statuses[0].full_coverage_availability = common::FullCoverageAvailability::Pending;
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(
            json["lineStatuses"][0]["fullCoverageAvailability"],
            serde_json::json!({"state": "pending"})
        );
    }

    #[test]
    fn full_coverage_availability_available_case_does_not_duplicate_stats_fields() {
        let mut report = sample_report(None);
        let stats = SampleStats {
            total: 20,
            delayed: 3,
            cancelled: 0,
            skipped: 1,
            avg_delay_minutes: 2.5,
        };
        report.statuses[0].full_coverage_stats = Some(stats.clone());
        report.statuses[0].full_coverage_availability =
            common::FullCoverageAvailability::Available(stats);
        let json = to_tfl_shape(&report, sample_computed_at(), false);
        assert_eq!(
            json["lineStatuses"][0]["fullCoverageAvailability"],
            serde_json::json!({"state": "available"})
        );
        assert!(
            json["lineStatuses"][0]["fullCoverageAvailability"]
                .get("total")
                .is_none(),
            "Available must not re-embed SampleStats fields"
        );
    }

    fn overlay_status(reason: &str) -> LineStatus {
        LineStatus {
            severity: Severity::MinorDelays,
            reason: reason.to_string(),
            validity: ValidityPeriod {
                from_date: Utc::now(),
                to_date: None,
                is_now: true,
            },
            disruption: None,
            data_quality: DataQuality::Tfl,
            sample_stats: None,
            sample_availability: SampleAvailability::NoCoverage,
            full_coverage_stats: None,
            full_coverage_availability: common::FullCoverageAvailability::NotEnabled,
        }
    }

    #[test]
    fn tfl_status_included_when_overlay_present() {
        let report = sample_report(None);
        let overlay = vec![overlay_status("Minor delays due to signalling")];
        let json = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
        assert_eq!(
            json["tflStatus"][0]["reason"],
            "Minor delays due to signalling"
        );
        assert_eq!(json["tflStatus"][0]["dataQuality"], "tfl");
    }

    #[test]
    fn tfl_status_omitted_when_overlay_absent() {
        let report = sample_report(None);
        let json = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, None);
        assert!(json.get("tflStatus").is_none());
    }

    #[test]
    fn overlay_does_not_alter_the_primary_line_statuses_field() {
        // The NR row's own statuses must render identically with or without an
        // overlay present -- the overlay is additive, never a merge into the
        // primary field.
        let report = sample_report(None);
        let without = to_tfl_shape(&report, sample_computed_at(), false);
        let overlay = vec![overlay_status("Some TfL text")];
        let with = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
        assert_eq!(without["lineStatuses"], with["lineStatuses"]);
    }

    #[test]
    fn overlay_reason_text_is_stable_across_identical_calls() {
        // Regression guard for the hard constraint in
        // docs/superpowers/specs/2026-08-22-tfl-service-metrics-v2-design.md
        // Area 1: this function must never synthesize or annotate `reason`
        // text -- it passes the source LineStatus through verbatim. Two calls
        // with byte-identical input must produce byte-identical output,
        // unlike the aggregator's volatile sample-stats annotation pattern
        // that caused a separate line-history duplication bug.
        let report = sample_report(None);
        let overlay = vec![overlay_status(
            "Severe delays between Paddington and Heathrow",
        )];
        let first = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
        let second =
            to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
        assert_eq!(first["tflStatus"], second["tflStatus"]);
        assert_eq!(
            first["tflStatus"][0]["reason"],
            "Severe delays between Paddington and Heathrow"
        );
    }

    #[test]
    fn station_departure_json_maps_every_field_to_camel_case() {
        let departure = common::StationDeparture {
            service_id: "svc-1".to_string(),
            operator: "SW".to_string(),
            destination_crs: "BSK".to_string(),
            scheduled: "14:40".to_string(),
            estimated: "14:47".to_string(),
            is_cancelled: false,
            delay_minutes: 7,
            cancel_reason: None,
            delay_reason: Some("signalling problem".to_string()),
            headcode: None,
            skipped_stations: vec!["ZQT".to_string()],
        };
        let json = station_departure_json(&departure);
        assert_eq!(
            json,
            serde_json::json!({
                "serviceId": "svc-1",
                "operator": "SW",
                "destinationCrs": "BSK",
                "scheduled": "14:40",
                "estimated": "14:47",
                "isCancelled": false,
                "delayMinutes": 7,
                "cancelReason": null,
                "delayReason": "signalling problem",
                "skippedStations": ["ZQT"],
            })
        );
        // No stray snake_case field survives alongside the camelCase one.
        assert!(json.get("destination_crs").is_none());
        assert!(json.get("delay_minutes").is_none());
        assert!(json.get("is_cancelled").is_none());
        assert!(json.get("cancel_reason").is_none());
        assert!(json.get("delay_reason").is_none());
        assert!(json.get("skipped_stations").is_none());
        assert!(
            json.get("headcode").is_none(),
            "headcode is never carried through"
        );
    }

    #[test]
    fn station_departure_json_none_fields_serialize_to_null_not_omitted() {
        let departure = common::StationDeparture {
            service_id: "svc-2".to_string(),
            operator: "ZA".to_string(),
            destination_crs: "WAT".to_string(),
            scheduled: "10:00".to_string(),
            estimated: "On time".to_string(),
            is_cancelled: true,
            delay_minutes: 0,
            cancel_reason: Some("fleet issue".to_string()),
            delay_reason: None,
            headcode: None,
            skipped_stations: vec![],
        };
        let json = station_departure_json(&departure);
        assert_eq!(json["cancelReason"], "fleet issue");
        // `delay_reason: None` must serialize as JSON `null` -- a present
        // key with a null value, not an omitted key -- because this is a
        // plain hand-built `json!`, unlike `TrackPinRequest`'s
        // `skip_serializing_if` on the request side.
        assert!(json.get("delayReason").is_some(), "key must be present");
        assert!(json["delayReason"].is_null());
        assert_eq!(json["skippedStations"], serde_json::json!([]));
    }
}
