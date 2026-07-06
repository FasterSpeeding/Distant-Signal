//! Live query functions backing the ingestion endpoints.
//!
//! Each `upsert_*` function does a batch `INSERT ... ON CONFLICT DO UPDATE`
//! against reference/incident data pushed by a poller. Deliberately uses
//! runtime-checked `sqlx::query`/`sqlx::query_as` rather than the `query!`
//! macro family: the macros need either a live database or a checked-in
//! `.sqlx` query cache available at *compile* time, and pinning this crate
//! to that is more machinery than a handful of straightforward upserts
//! warrant.

use anyhow::Result;
use common::{IncidentMessage, StationReference, TocReference};
use sqlx::PgPool;

/// The subset of an existing `incidents` row needed to decide whether an
/// incoming `IncidentMessage` represents a real change worth recording in
/// `incident_history`.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
struct ExistingIncident {
    summary: String,
    description: String,
    validity_periods: serde_json::Value,
}

/// Pure diff check, factored out of `upsert_incidents` so it's testable
/// without a database: an incident is "changed" if it's new, or if its
/// summary, description, or validity periods differ from what's stored.
fn incident_changed(
    existing: Option<&ExistingIncident>,
    summary: &str,
    description: &str,
    validity_periods: &serde_json::Value,
) -> bool {
    match existing {
        None => true,
        Some(row) => {
            row.summary != summary
                || row.description != description
                || row.validity_periods != *validity_periods
        }
    }
}

/// Upserts a batch of Knowledgebase incidents. Each incident is inserted or
/// updated in `incidents`; if the stored summary/description/validity_periods
/// differ from what's incoming (or the incident is new), a snapshot is also
/// appended to `incident_history`. Runs as a single transaction so a
/// mid-batch failure doesn't leave `incidents` and `incident_history`
/// inconsistent with each other.
pub async fn upsert_incidents(pool: &PgPool, incidents: &[IncidentMessage]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for incident in incidents {
        let validity_json = serde_json::to_value(&incident.validity)?;

        let existing: Option<ExistingIncident> = sqlx::query_as(
            "SELECT summary, description, validity_periods FROM incidents WHERE incident_id = $1",
        )
        .bind(&incident.incident_id)
        .fetch_optional(&mut *tx)
        .await?;

        let changed = incident_changed(
            existing.as_ref(),
            &incident.summary,
            &incident.description,
            &validity_json,
        );

        sqlx::query(
            r#"
            INSERT INTO incidents (
                incident_id, summary, description, operators, affected_stations,
                priority, validity_periods, is_planned, is_cleared, fetched_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
            ON CONFLICT (incident_id) DO UPDATE SET
                summary           = EXCLUDED.summary,
                description       = EXCLUDED.description,
                operators         = EXCLUDED.operators,
                affected_stations = EXCLUDED.affected_stations,
                priority          = EXCLUDED.priority,
                validity_periods  = EXCLUDED.validity_periods,
                is_planned        = EXCLUDED.is_planned,
                is_cleared        = EXCLUDED.is_cleared,
                fetched_at        = NOW()
            "#,
        )
        .bind(&incident.incident_id)
        .bind(&incident.summary)
        .bind(&incident.description)
        .bind(&incident.operators)
        .bind(&incident.affected_stations)
        .bind(incident.priority)
        .bind(&validity_json)
        .bind(incident.is_planned)
        .bind(incident.is_cleared)
        .execute(&mut *tx)
        .await?;

        if changed {
            sqlx::query(
                r#"
                INSERT INTO incident_history (
                    incident_id, summary, description, operators, affected_stations,
                    priority, validity_periods, is_planned, is_cleared
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
                "#,
            )
            .bind(&incident.incident_id)
            .bind(&incident.summary)
            .bind(&incident.description)
            .bind(&incident.operators)
            .bind(&incident.affected_stations)
            .bind(incident.priority)
            .bind(&validity_json)
            .bind(incident.is_planned)
            .bind(incident.is_cleared)
            .execute(&mut *tx)
            .await?;
        }

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Upserts a batch of station reference records. No history — this is
/// reference data, not an event stream (see the reference-data migration's
/// comment).
pub async fn upsert_stations(pool: &PgPool, stations: &[StationReference]) -> Result<u64> {
    let mut count = 0u64;

    for station in stations {
        sqlx::query(
            r#"
            INSERT INTO stations (crs, name, latitude, longitude, station_operator, accessibility, fetched_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (crs) DO UPDATE SET
                name             = EXCLUDED.name,
                latitude         = EXCLUDED.latitude,
                longitude        = EXCLUDED.longitude,
                station_operator = EXCLUDED.station_operator,
                accessibility    = EXCLUDED.accessibility,
                fetched_at       = NOW()
            "#,
        )
        .bind(&station.crs)
        .bind(&station.name)
        .bind(station.latitude)
        .bind(station.longitude)
        .bind(&station.station_operator)
        .bind(&station.accessibility)
        .execute(pool)
        .await?;

        count += 1;
    }

    Ok(count)
}

/// Upserts a batch of TOC reference records. No history, same rationale as
/// `upsert_stations`.
pub async fn upsert_tocs(pool: &PgPool, tocs: &[TocReference]) -> Result<u64> {
    let mut count = 0u64;

    for toc in tocs {
        sqlx::query(
            r#"
            INSERT INTO tocs (atoc_code, name, legal_name, atoc_member, station_operator, fetched_at)
            VALUES ($1, $2, $3, $4, $5, NOW())
            ON CONFLICT (atoc_code) DO UPDATE SET
                name             = EXCLUDED.name,
                legal_name       = EXCLUDED.legal_name,
                atoc_member      = EXCLUDED.atoc_member,
                station_operator = EXCLUDED.station_operator,
                fetched_at       = NOW()
            "#,
        )
        .bind(&toc.atoc_code)
        .bind(&toc.name)
        .bind(&toc.legal_name)
        .bind(toc.atoc_member)
        .bind(toc.station_operator)
        .execute(pool)
        .await?;

        count += 1;
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing(summary: &str, description: &str, validity: serde_json::Value) -> ExistingIncident {
        ExistingIncident {
            summary: summary.to_string(),
            description: description.to_string(),
            validity_periods: validity,
        }
    }

    #[test]
    fn new_incident_is_always_changed() {
        assert!(incident_changed(
            None,
            "summary",
            "description",
            &serde_json::json!([])
        ));
    }

    #[test]
    fn identical_incident_is_not_changed() {
        let row = existing("summary", "description", serde_json::json!([]));
        assert!(!incident_changed(
            Some(&row),
            "summary",
            "description",
            &serde_json::json!([])
        ));
    }

    #[test]
    fn changed_summary_is_detected() {
        let row = existing("old summary", "description", serde_json::json!([]));
        assert!(incident_changed(
            Some(&row),
            "new summary",
            "description",
            &serde_json::json!([])
        ));
    }

    #[test]
    fn changed_description_is_detected() {
        let row = existing("summary", "old description", serde_json::json!([]));
        assert!(incident_changed(
            Some(&row),
            "summary",
            "new description",
            &serde_json::json!([])
        ));
    }

    #[test]
    fn changed_validity_periods_is_detected() {
        let row = existing("summary", "description", serde_json::json!([]));
        let new_validity = serde_json::json!([{"from_date": "2026-01-01T00:00:00Z", "to_date": null, "is_now": true}]);
        assert!(incident_changed(
            Some(&row),
            "summary",
            "description",
            &new_validity
        ));
    }

    #[test]
    fn unrelated_operators_or_stations_changes_are_not_this_functions_concern() {
        // operators/affected_stations/priority/is_planned/is_cleared changes
        // still get written to `incidents` (the upsert always overwrites),
        // they just don't independently trigger a history row per the
        // brief's spec (only summary/description/validity_periods do).
        let row = existing("summary", "description", serde_json::json!([]));
        assert!(!incident_changed(
            Some(&row),
            "summary",
            "description",
            &serde_json::json!([])
        ));
    }
}
