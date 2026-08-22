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
        "$type": "NRStatus.LineStatusReport",
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
        out["tflStatus"] = Value::Array(statuses.iter().map(|s| status_to_json(s, detail)).collect());
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
        out["sampleStats"] = json!({
            "total": stats.total,
            "delayed": stats.delayed,
            "cancelled": stats.cancelled,
            "skipped": stats.skipped,
            "avgDelayMinutes": stats.avg_delay_minutes,
        });
    }

    if detail
        && let Some(disruption) = &status.disruption
    {
        out["disruption"] = json!({
            "category": disruption.category,
            "description": disruption.description,
            "affectedStops": disruption.affected_stops,
            "affectedRoutes": disruption.affected_routes.iter().map(|r| json!({
                "from": r.from_crs,
                "to": r.to_crs,
            })).collect::<Vec<_>>(),
            "source": disruption.source,
        });
    }

    out
}

fn severity_description(severity: Severity) -> &'static str {
    severity.description()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, TimeZone, Utc};
    use common::{DataQuality, Disruption, SampleStats, ValidityPeriod};

    fn sample_report(disruption: Option<Disruption>) -> LineStatusReport {
        LineStatusReport {
            id: "wcml".to_string(),
            name: "West Coast Main Line".to_string(),
            mode_name: "national-rail".to_string(),
            operators: vec!["AW".to_string()],
            statuses: vec![LineStatus {
                severity: Severity::MinorDelays,
                reason: "Signal failure".to_string(),
                validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
                disruption,
                data_quality: DataQuality::Knowledgebase,
                sample_stats: None,
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
        assert_eq!(json["$type"], "NRStatus.LineStatusReport");
        assert_eq!(json["id"], "wcml");
        assert_eq!(json["name"], "West Coast Main Line");
        assert_eq!(json["modeName"], "national-rail");
        assert_eq!(json["operators"][0], "AW");
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
            affected_routes: vec![common::AffectedRoute { from_crs: "WAT".to_string(), to_crs: "WOK".to_string() }],
            source: Some("knowledgebase-incident-1".to_string()),
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

    fn overlay_status(reason: &str) -> LineStatus {
        LineStatus {
            severity: Severity::MinorDelays,
            reason: reason.to_string(),
            validity: ValidityPeriod { from_date: Utc::now(), to_date: None, is_now: true },
            disruption: None,
            data_quality: DataQuality::Tfl,
            sample_stats: None,
        }
    }

    #[test]
    fn tfl_status_included_when_overlay_present() {
        let report = sample_report(None);
        let overlay = vec![overlay_status("Minor delays due to signalling")];
        let json = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
        assert_eq!(json["tflStatus"][0]["reason"], "Minor delays due to signalling");
        assert_eq!(json["tflStatus"][0]["dataQuality"], "tfl");
    }

    #[test]
    fn tfl_status_omitted_when_overlay_absent() {
        let report = sample_report(None);
        let json = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, None);
        assert!(json.get("tflStatus").is_none());
    }

    #[test]
    fn overlay_does_not_alter_the_primary_lineStatuses_field() {
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
        let overlay = vec![overlay_status("Severe delays between Paddington and Heathrow")];
        let first = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
        let second = to_tfl_shape_with_overlay(&report, sample_computed_at(), false, Some(&overlay));
        assert_eq!(first["tflStatus"], second["tflStatus"]);
        assert_eq!(first["tflStatus"][0]["reason"], "Severe delays between Paddington and Heathrow");
    }
}
