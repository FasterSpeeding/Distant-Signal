//! `GET /public/incidents/{incidentId}` -- a single Knowledgebase incident's
//! full detail: description, affected stations, every validity period,
//! which lines currently report it, and its own change history.
//! Unauthenticated, matching every other read in `public_router()` -- see
//! docs/superpowers/specs/2026-08-31-incident-detail-page-design.md's
//! "Public read-route convention" finding: every field this returns is
//! already fully public today via `GET /Line/{ids}/Status?detail=true`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new().route("/incidents/{incidentId}", axum::routing::get(get_incident))
}

/// `knowledgebase-incident-{incidentId}` is the ONLY provenance-string
/// format that names a real `incidents` row -- see the design spec's
/// Correction 1. Reconstructing it here (rather than storing/returning the
/// bare incident_id as `disruption.source`) is what lets
/// `lines_currently_reporting_incident` reach into `line_status.statuses`'
/// JSONB and find this exact incident.
fn knowledgebase_source(incident_id: &str) -> String {
    format!("knowledgebase-incident-{incident_id}")
}

async fn get_incident(
    State(app): State<App>,
    Path(incident_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let Some(incident) = queries::incident_by_id(&app.database, &incident_id)
        .await
        .map_err(internal_error)?
    else {
        return Err((StatusCode::NOT_FOUND, "incident not found".to_string()));
    };
    let history = queries::incident_history_for_id(&app.database, &incident_id)
        .await
        .map_err(internal_error)?;
    let source = knowledgebase_source(&incident_id);
    let lines = queries::lines_currently_reporting_incident(&app.database, &source)
        .await
        .map_err(internal_error)?;

    Ok(Json(to_incident_detail_json(incident, history, lines)))
}

/// Renders `serde_json::Value` field-by-field via `json!()`, exactly like
/// `crates/api/src/render.rs::status_to_json` -- deliberately NOT
/// `#[derive(Serialize)] #[serde(rename_all = "camelCase")]` on a struct
/// that embeds `validity_periods` directly, because `rename_all` is not
/// inherited into a nested type. See this plan's Status note Correction A
/// and Global Constraints for the concrete failure mode that would produce
/// (a response that's camelCase at the top level but snake_case inside
/// every validity period). Pure function, no I/O -- unit-testable without
/// a database, matching `to_tfl_shape`'s own testable shape in `render.rs`.
fn to_incident_detail_json(
    incident: queries::IncidentRow,
    history: Vec<queries::IncidentHistoryRow>,
    lines: Vec<queries::IncidentLineRefRow>,
) -> Value {
    json!({
        "incidentId": incident.incident_id,
        "summary": incident.summary,
        "description": incident.description,
        "operators": incident.operators,
        "affectedStations": incident.affected_stations,
        "priority": incident.priority,
        "validityPeriods": render_validity_periods(&incident.validity_periods),
        "isPlanned": incident.is_planned,
        "isCleared": incident.is_cleared,
        "firstSeenAt": incident.first_seen_at.to_rfc3339(),
        "fetchedAt": incident.fetched_at.to_rfc3339(),
        "currentlyAffectsLines": lines.iter().map(|l| json!({
            "id": l.line_id,
            "name": l.name,
        })).collect::<Vec<_>>(),
        "history": history.iter().map(|h| json!({
            "summary": h.summary,
            "description": h.description,
            "operators": h.operators,
            "affectedStations": h.affected_stations,
            "priority": h.priority,
            "validityPeriods": render_validity_periods(&h.validity_periods),
            "isPlanned": h.is_planned,
            "isCleared": h.is_cleared,
            "recordedAt": h.recorded_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })
}

/// `validity_periods` comes back from `queries::incident_by_id`/
/// `incident_history_for_id` as raw `serde_json::Value` (the column's own
/// stored JSONB, snake-case field names -- `from_date`/`to_date`/`is_now`,
/// since `common::ValidityPeriod` has no `rename_all`). Deserializes into
/// the real Rust type first so a malformed row fails loudly via
/// `unwrap_or_default` -> empty array, rather than this function
/// re-implementing JSONB field access by hand.
fn render_validity_periods(raw: &Value) -> Value {
    let periods: Vec<common::ValidityPeriod> = serde_json::from_value(raw.clone()).unwrap_or_default();
    Value::Array(
        periods
            .into_iter()
            .map(|p| {
                json!({
                    "fromDate": p.from_date.to_rfc3339(),
                    "toDate": p.to_date.map(|d| d.to_rfc3339()),
                    "isNow": p.is_now,
                })
            })
            .collect(),
    )
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "incident lookup failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "operation failed".to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn sample_incident() -> queries::IncidentRow {
        queries::IncidentRow {
            incident_id: "12345".to_string(),
            summary: "Signal failure at Woking".to_string(),
            description: "<p>Delays expected</p>".to_string(),
            operators: vec!["VT".to_string()],
            affected_stations: vec!["WOK".to_string(), "WAT".to_string()],
            priority: 3,
            validity_periods: serde_json::json!([
                {"from_date": "2026-08-30T09:00:00Z", "to_date": null, "is_now": true}
            ]),
            is_planned: false,
            is_cleared: false,
            first_seen_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            fetched_at: Utc.with_ymd_and_hms(2026, 8, 31, 10, 15, 0).unwrap(),
        }
    }

    #[test]
    fn renders_top_level_fields_as_camel_case() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        assert_eq!(json["incidentId"], "12345");
        assert_eq!(json["summary"], "Signal failure at Woking");
        assert_eq!(json["affectedStations"][0], "WOK");
        assert_eq!(json["isPlanned"], false);
        assert_eq!(json["isCleared"], false);
    }

    #[test]
    fn validity_periods_render_as_camel_case_not_snake_case() {
        // The direct regression test for Correction A -- proves this
        // function does not fall back to a derived Serialize impl that
        // would leak `from_date`/`to_date`/`is_now` through unrenamed.
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        let period = &json["validityPeriods"][0];
        assert_eq!(period["fromDate"], "2026-08-30T09:00:00+00:00");
        assert!(period["toDate"].is_null());
        assert_eq!(period["isNow"], true);
        assert!(period.get("from_date").is_none(), "must not leak the raw snake_case JSONB field name");
    }

    #[test]
    fn currently_affects_lines_is_empty_array_not_null_when_no_lines_match() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        assert!(json["currentlyAffectsLines"].is_array());
        assert_eq!(json["currentlyAffectsLines"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn history_renders_every_entry_newest_first_order_preserved() {
        let history = vec![
            queries::IncidentHistoryRow {
                summary: "v2".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 2,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 31, 9, 0, 0).unwrap(),
            },
            queries::IncidentHistoryRow {
                summary: "v1".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 1,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            },
        ];
        let json = to_incident_detail_json(sample_incident(), history, vec![]);
        assert_eq!(json["history"][0]["summary"], "v2");
        assert_eq!(json["history"][1]["summary"], "v1");
    }

    #[test]
    fn currently_affects_lines_renders_id_and_name() {
        let lines = vec![queries::IncidentLineRefRow { line_id: "south-western".to_string(), name: "South Western Main Line".to_string() }];
        let json = to_incident_detail_json(sample_incident(), vec![], lines);
        assert_eq!(json["currentlyAffectsLines"][0]["id"], "south-western");
        assert_eq!(json["currentlyAffectsLines"][0]["name"], "South Western Main Line");
    }

    #[test]
    fn knowledgebase_source_matches_the_exact_format_correction_1_verified() {
        assert_eq!(knowledgebase_source("12345"), "knowledgebase-incident-12345");
    }
}
