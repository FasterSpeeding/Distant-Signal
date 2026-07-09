//! Render `common::LineStatusReport`/`LineStatus` as TfL-shaped JSON.
//! Ported from `src/render.py`. Deliberately independent of any
//! `#[serde(rename)]` on the stored types — the internal storage
//! representation (however `LineStatus` happens to serialize by default)
//! and the public TfL response shape are different concerns; this module
//! is the only place that knows the public shape, exactly like the
//! Python original builds its response dict by hand rather than relying
//! on dataclass field names.

use common::{LineStatus, LineStatusReport, Severity};
use serde_json::{Value, json};

pub fn to_tfl_shape(report: &LineStatusReport, detail: bool) -> Value {
    json!({
        "$type": "NRStatus.LineStatusReport",
        "id": report.id,
        "name": report.name,
        "modeName": report.mode_name,
        "operators": report.operators,
        "lineStatuses": report.statuses.iter().map(|s| status_to_json(s, detail)).collect::<Vec<_>>(),
    })
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
    use chrono::Utc;
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

    #[test]
    fn renders_top_level_fields() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, false);
        assert_eq!(json["$type"], "NRStatus.LineStatusReport");
        assert_eq!(json["id"], "wcml");
        assert_eq!(json["name"], "West Coast Main Line");
        assert_eq!(json["modeName"], "national-rail");
        assert_eq!(json["operators"][0], "AW");
    }

    #[test]
    fn renders_status_fields() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, false);
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
        let json = to_tfl_shape(&report, false);
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
        let json = to_tfl_shape(&report, true);
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
        let json = to_tfl_shape(&report, true);
        assert!(json["lineStatuses"][0].get("disruption").is_none());
    }

    #[test]
    fn sample_stats_included_when_present() {
        let mut report = sample_report(None);
        report.statuses[0].sample_stats = Some(SampleStats {
            total: 10,
            delayed: 4,
            cancelled: 1,
            avg_delay_minutes: 6.5,
        });
        let json = to_tfl_shape(&report, false);
        let stats = &json["lineStatuses"][0]["sampleStats"];
        assert_eq!(stats["total"], 10);
        assert_eq!(stats["delayed"], 4);
        assert_eq!(stats["cancelled"], 1);
        assert_eq!(stats["avgDelayMinutes"], 6.5);
    }

    #[test]
    fn sample_stats_omitted_when_absent() {
        let report = sample_report(None);
        let json = to_tfl_shape(&report, false);
        assert!(json["lineStatuses"][0].get("sampleStats").is_none());
    }
}
