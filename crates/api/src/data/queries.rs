//! Live query functions backing the ingestion endpoints.
//!
//! Each `upsert_*` function does a batch `INSERT ... ON CONFLICT DO UPDATE`
//! against reference/incident data pushed by a poller. Deliberately uses
//! runtime-checked `sqlx::query`/`sqlx::query_as` rather than the `query!`
//! macro family: the macros need either a live database or a checked-in
//! `.sqlx` query cache available at *compile* time, and pinning this crate
//! to that is more machinery than a handful of straightforward upserts
//! warrant.

use std::collections::HashMap;

use anyhow::Result;
use common::{IncidentMessage, StationReference, StationSample, TocReference};
use sqlx::PgPool;

/// Incidents are upserted in chunks of this size, each as its own
/// transaction, rather than one transaction for the whole poll batch --
/// see the `upsert_incidents` doc comment for why.
const UPSERT_CHUNK_SIZE: usize = 50;

/// The subset of an existing `incidents` row needed to decide whether an
/// incoming `IncidentMessage` represents a real change worth recording in
/// `incident_history`.
#[derive(Debug, Clone, PartialEq, sqlx::FromRow)]
struct ExistingIncident {
    incident_id: String,
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

/// Narrower than `incident_changed`: true only if summary or description
/// differ from what's stored. Validity-only changes don't need
/// re-extraction -- the prose an LLM would read hasn't moved. Drives
/// whether `upsert_incidents` publishes a `text-changed` event.
fn text_changed(existing: Option<&ExistingIncident>, summary: &str, description: &str) -> bool {
    match existing {
        None => true,
        Some(row) => row.summary != summary || row.description != description,
    }
}

/// Upserts a batch of Knowledgebase incidents. Each incident is inserted or
/// updated in `incidents`; if the stored summary/description/validity_periods
/// differ from what's incoming (or the incident is new), a snapshot is also
/// appended to `incident_history`.
///
/// Runs as a series of `UPSERT_CHUNK_SIZE`-sized transactions rather than one
/// transaction for the whole batch -- a full poll cycle can carry hundreds of
/// incidents, and holding row locks on all of them for the duration of one
/// giant transaction blocks unrelated single-row writers (e.g. the enricher
/// persisting extraction results) for as long as the whole batch takes.
/// Chunking bounds that lock-hold window to one chunk's worth of work. Each
/// chunk is still atomic with respect to its own `incidents`/`incident_history`
/// writes, but a failure partway through the batch no longer rolls back
/// chunks that already committed -- acceptable here because the poller
/// resends the full current feed state every cycle (see `poller-incidents`),
/// so anything not persisted this round is retried wholesale next round.
pub async fn upsert_incidents(
    pool: &PgPool,
    redis: &redis::Client,
    incidents: &[IncidentMessage],
) -> Result<u64> {
    let mut count = 0u64;
    let mut text_changed_ids = Vec::new();

    for chunk in incidents.chunks(UPSERT_CHUNK_SIZE) {
        let mut tx = pool.begin().await?;

        let chunk_ids: Vec<&str> = chunk.iter().map(|i| i.incident_id.as_str()).collect();
        let existing_rows: Vec<ExistingIncident> = sqlx::query_as(
            "SELECT incident_id, summary, description, validity_periods FROM incidents WHERE incident_id = ANY($1)",
        )
        .bind(&chunk_ids)
        .fetch_all(&mut *tx)
        .await?;
        let existing_by_id: HashMap<&str, &ExistingIncident> = existing_rows
            .iter()
            .map(|row| (row.incident_id.as_str(), row))
            .collect();

        for incident in chunk {
            let validity_json = serde_json::to_value(&incident.validity)?;
            let existing = existing_by_id.get(incident.incident_id.as_str()).copied();

            let changed = incident_changed(
                existing,
                &incident.summary,
                &incident.description,
                &validity_json,
            );
            if text_changed(existing, &incident.summary, &incident.description) {
                text_changed_ids.push(incident.incident_id.clone());
            }

            sqlx::query(
                r#"
                INSERT INTO incidents (
                    incident_id, summary, description, operators, affected_stations,
                    priority, validity_periods, is_planned, is_cleared, fetched_at,
                    first_seen_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
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
    }

    // Publish only after commit: a publish before commit could announce an
    // incident that a later failure in this same batch rolls back. Publish
    // failure is logged, not propagated -- the hourly sweep (Task 5) is the
    // backstop for a missed publish, so ingestion must not fail because
    // Redis is briefly unavailable.
    if text_changed_ids.is_empty() {
        return Ok(count);
    }

    // Connecting happens HERE, not at api startup: `AppState.redis` is a
    // lazy `redis::Client` that has never opened a socket. A Redis that is
    // down therefore surfaces as a failed publish -- which this function
    // already logs and continues past -- instead of failing `AppState::init`
    // and crash-looping the public status API.
    let mut redis = match redis.get_connection_manager().await {
        Ok(conn) => conn,
        Err(err) => {
            tracing::warn!(
                error = ?err,
                pending = text_changed_ids.len(),
                "could not connect to redis to publish text-changed events; hourly sweep will catch them"
            );
            return Ok(count);
        }
    };
    for incident_id in text_changed_ids {
        let result: redis::RedisResult<String> = redis::cmd("XADD")
            .arg("incident-text-changed")
            .arg("*")
            .arg("incident_id")
            .arg(&incident_id)
            .query_async(&mut redis)
            .await;
        if let Err(err) = result {
            tracing::warn!(error = ?err, incident_id, "failed to publish text-changed event; hourly sweep will catch it");
        }
    }

    Ok(count)
}

/// Upserts a batch of station reference records. No history — this is
/// reference data, not an event stream (see the reference-data migration's
/// comment).
pub async fn upsert_stations(pool: &PgPool, stations: &[StationReference]) -> Result<u64> {
    let mut tx = pool.begin().await?;
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
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Upserts a batch of station samples (LDBWS departure-board snapshots).
/// No history — this is a point-in-time sample, wholesale-replaced per
/// poll, same rationale as `upsert_stations`/`upsert_tocs`.
pub async fn upsert_station_samples(pool: &PgPool, samples: &[StationSample]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for sample in samples {
        let departures_json = serde_json::to_value(&sample.departures)?;

        sqlx::query(
            r#"
            INSERT INTO station_samples (crs, polled_at, departures)
            VALUES ($1, $2, $3)
            ON CONFLICT (crs) DO UPDATE SET
                polled_at  = EXCLUDED.polled_at,
                departures = EXCLUDED.departures
            "#,
        )
        .bind(&sample.crs)
        .bind(sample.polled_at)
        .bind(&departures_json)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Upserts a batch of TOC reference records. No history, same rationale as
/// `upsert_stations`.
pub async fn upsert_tocs(pool: &PgPool, tocs: &[TocReference]) -> Result<u64> {
    let mut tx = pool.begin().await?;
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
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Timestamp of the most recent successful ingest for each poller-fed
/// table, or `None` if the table has never been populated. Backs the
/// `GET /private/*` freshness-check endpoints
/// (`crates/api/src/routes/ingest.rs`) each poller calls once at startup
/// to decide whether to skip an immediately-redundant first fetch (see
/// `common::ingest::time_until_next_poll`). `MAX(...)` over zero rows
/// returns one row with a `NULL` column, not zero rows — `fetch_one`
/// (not `fetch_optional`) is deliberate here, matching that: it's the
/// *column* that's optional, not the row.
pub async fn last_stations_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(fetched_at) FROM stations")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

pub async fn last_tocs_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(fetched_at) FROM tocs")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

pub async fn last_incidents_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(fetched_at) FROM incidents")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

pub async fn last_station_samples_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (polled_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(polled_at) FROM station_samples")
            .fetch_one(pool)
            .await?;
    Ok(polled_at)
}

/// One row from `line_status`, deserialized into the shape `render.rs`
/// consumes.
pub struct LineStatusRow {
    pub id: String,
    pub name: String,
    pub mode_name: String,
    pub operators: Vec<String>,
    pub statuses: Vec<common::LineStatus>,
    pub computed_at: chrono::DateTime<chrono::Utc>,
}

fn row_to_report(row: sqlx::postgres::PgRow) -> Result<LineStatusRow> {
    use sqlx::Row;
    let statuses_json: serde_json::Value = row.try_get("statuses")?;
    Ok(LineStatusRow {
        id: row.try_get("line_id")?,
        name: row.try_get("name")?,
        mode_name: row.try_get("mode_name")?,
        operators: row.try_get("operators")?,
        statuses: serde_json::from_value(statuses_json)?,
        computed_at: row.try_get("computed_at")?,
    })
}

pub async fn line_status_for_mode(pool: &PgPool, mode: &str) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name, operators, statuses, computed_at FROM line_status WHERE mode_name = $1",
    )
    .bind(mode)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_report).collect()
}

pub async fn line_status_for_ids(pool: &PgPool, ids: &[String]) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name, operators, statuses, computed_at FROM line_status WHERE line_id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_report).collect()
}

pub async fn line_status_history_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<(chrono::DateTime<chrono::Utc>, Vec<common::LineStatus>)>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT statuses, computed_at FROM line_status_history \
         WHERE line_id = $1 AND computed_at BETWEEN $2 AND $3 ORDER BY computed_at",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let statuses_json: serde_json::Value = row.try_get("statuses")?;
            let computed_at: chrono::DateTime<chrono::Utc> = row.try_get("computed_at")?;
            Ok((computed_at, serde_json::from_value(statuses_json)?))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn existing(summary: &str, description: &str, validity: serde_json::Value) -> ExistingIncident {
        ExistingIncident {
            incident_id: "TEST123".to_string(),
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

    #[test]
    fn text_changed_true_for_a_new_incident() {
        assert!(text_changed(None, "Signal failure", "Delays expected"));
    }

    #[test]
    fn text_changed_true_when_summary_differs() {
        let row = existing("Signal failure", "Delays expected", serde_json::json!([]));
        assert!(text_changed(Some(&row), "Points failure", "Delays expected"));
    }

    #[test]
    fn text_changed_true_when_description_differs() {
        let row = existing("Signal failure", "Delays expected", serde_json::json!([]));
        assert!(text_changed(Some(&row), "Signal failure", "Disruption has now ended"));
    }

    #[test]
    fn text_changed_false_when_only_validity_periods_would_differ() {
        // text_changed only compares summary/description -- validity is
        // deliberately excluded, since it doesn't require re-extraction of
        // prose that hasn't moved. This test simulates that by reusing the
        // same summary/description text_changed actually looks at; there's
        // no validity parameter to vary because text_changed never takes one.
        let row = existing("Signal failure", "Delays expected", serde_json::json!([]));
        assert!(!text_changed(Some(&row), "Signal failure", "Delays expected"));
    }
}
