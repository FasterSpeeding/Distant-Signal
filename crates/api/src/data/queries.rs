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
use common::{
    IncidentMessage, LineStatusReport, StationFullCoverageSample, StationReference, StationSample,
    TocReference,
};
use serde::Deserialize;
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

/// Upserts a batch of per-(crs, operator) full-coverage rows. No
/// history -- wholesale-replaced per producer resolution cycle, same
/// rationale as `upsert_station_samples`. Written by
/// `post_station_full_coverage_samples` (Task 5), a future
/// full-coverage-consumer's real caller once it exists (not built by this
/// plan).
pub async fn upsert_station_full_coverage_samples(
    pool: &PgPool,
    samples: &[StationFullCoverageSample],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for sample in samples {
        let stats_json = serde_json::to_value(&sample.stats)?;

        sqlx::query(
            r#"
            INSERT INTO station_full_coverage_samples (crs, operator, resolved_at, stats)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (crs, operator) DO UPDATE SET
                resolved_at = EXCLUDED.resolved_at,
                stats       = EXCLUDED.stats
            "#,
        )
        .bind(&sample.crs)
        .bind(&sample.operator)
        .bind(sample.resolved_at)
        .bind(&stats_json)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Pure diff check, factored out of `upsert_tfl_line_status` so it's
/// testable without a database: a TfL line's statuses are "changed" if the
/// line is new to us, or if the incoming `statuses` JSON differs from what
/// is stored, ignoring `sample_stats`/`sample_availability` — mirroring the
/// aggregator's own `normalize_for_diff` (`crates/aggregator/src/queries.rs`),
/// which strips the same fields for the same reason: a live delay/
/// cancellation count (and its accompanying availability state) rolls over
/// almost every poll cycle even when nothing about the underlying
/// disruption has changed, and must not participate in change detection or
/// `line_status_history` grows a row every poll cycle. This guard exists
/// ahead of any TfL-sourced line actually populating `sample_stats` (see
/// `crates/poller-tfl/src/dlr`), so it's already in place once one does.
fn tfl_statuses_changed(
    existing: Option<&serde_json::Value>,
    incoming: &serde_json::Value,
) -> bool {
    match existing {
        None => true,
        Some(stored) => normalize_for_diff(stored) != normalize_for_diff(incoming),
    }
}

/// Strips `sample_stats`/`sample_availability` (and their Decision-1
/// full-coverage siblings, `full_coverage_stats`/`full_coverage_availability`)
/// from every status entry before comparison. See `tfl_statuses_changed`.
/// The full-coverage pair is stripped symmetrically even though no TfL line
/// populates it today (Decision 5: full coverage is scoped to national-rail
/// lines only, out of scope for TfL) -- matching this function's own stated
/// rationale for `sample_stats`: strip on principle so a future producer
/// doesn't silently reintroduce spurious `line_status_history` churn.
fn normalize_for_diff(statuses: &serde_json::Value) -> serde_json::Value {
    let mut statuses = statuses.clone();
    if let Some(entries) = statuses.as_array_mut() {
        for entry in entries {
            if let Some(obj) = entry.as_object_mut() {
                obj.remove("sample_stats");
                obj.remove("sample_availability");
                obj.remove("full_coverage_stats");
                obj.remove("full_coverage_availability");
            }
        }
    }
    statuses
}

/// Upserts a batch of TfL line-status reports into `line_status` (marked
/// `source = 'tfl'`), appending a `line_status_history` snapshot for each
/// line whose statuses actually changed, and deleting any TfL row missing
/// from this batch.
///
/// The whole batch is one transaction — unlike `upsert_incidents`, which
/// chunks to bound its lock-hold window, this is ~20 rows once every 300s.
///
/// An empty batch is a no-op rather than a mass delete: "TfL returned
/// nothing" is a fault, not an instruction to forget every line. The poller
/// refuses to post one either (belt and braces, since this is the side that
/// would do the damage).
pub async fn upsert_tfl_line_status(pool: &PgPool, reports: &[LineStatusReport]) -> Result<u64> {
    if reports.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for report in reports {
        let statuses_json = serde_json::to_value(&report.statuses)?;

        let existing: Option<serde_json::Value> = sqlx::query_scalar(
            "SELECT statuses FROM line_status WHERE line_id = $1 AND source = 'tfl'",
        )
        .bind(&report.id)
        .fetch_optional(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO line_status (line_id, name, mode_name, operators, statuses, computed_at, source)
            VALUES ($1, $2, $3, $4, $5, NOW(), 'tfl')
            ON CONFLICT (line_id) DO UPDATE SET
                name        = EXCLUDED.name,
                mode_name   = EXCLUDED.mode_name,
                operators   = EXCLUDED.operators,
                statuses    = EXCLUDED.statuses,
                computed_at = NOW(),
                source      = 'tfl'
            "#,
        )
        .bind(&report.id)
        .bind(&report.name)
        .bind(&report.mode_name)
        .bind(&report.operators)
        .bind(&statuses_json)
        .execute(&mut *tx)
        .await?;

        if tfl_statuses_changed(existing.as_ref(), &statuses_json) {
            sqlx::query(
                "INSERT INTO line_status_history (line_id, statuses, computed_at) VALUES ($1, $2, NOW())",
            )
            .bind(&report.id)
            .bind(&statuses_json)
            .execute(&mut *tx)
            .await?;
        }

        count += 1;
    }

    // A TfL line that leaves the feed (a renamed id, a withdrawn service)
    // has no other way of disappearing — `/public/lines` derives its TfL
    // entries from exactly these rows. The aggregator's
    // `prune_removed_lines` is the same idea from the other side of the
    // fence; each writer prunes only what it owns.
    let ids: Vec<&str> = reports.iter().map(|r| r.id.as_str()).collect();
    let pruned =
        sqlx::query("DELETE FROM line_status WHERE source = 'tfl' AND NOT (line_id = ANY($1))")
            .bind(&ids)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if pruned > 0 {
        tracing::info!(pruned, "removed TfL lines no longer present in the feed");
    }

    tx.commit().await?;
    Ok(count)
}

/// The identity of one TfL line, for the `/public/lines` catalogue.
pub struct TflLineSummaryRow {
    pub id: String,
    pub name: String,
    pub mode_name: String,
}

/// TfL lines, derived from the rows `crates/poller-tfl` wrote rather than
/// from a hand-curated `lines/*.toml` entry.
///
/// A TOML entry would be wrong three ways: the aggregator loads that
/// directory and would overwrite each ingested TfL status with a
/// Good-Service fallback on its next cycle; a `LineDefinition` is mostly
/// route topology (ordered CRS stations, segments, sample stations,
/// keywords, thresholds) that a finished-status feed has no use for; and it
/// would drift out of date — TfL split "London Overground" into six named
/// lines in 2024. These rows are the feed's own answer, and
/// `upsert_tfl_line_status` prunes the ones that leave it.
pub async fn tfl_line_summaries(pool: &PgPool) -> Result<Vec<TflLineSummaryRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name FROM line_status WHERE source = 'tfl' ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(TflLineSummaryRow {
                id: row.try_get("line_id")?,
                name: row.try_get("name")?,
                mode_name: row.try_get("mode_name")?,
            })
        })
        .collect()
}

/// Timestamp of the most recent TfL line-status ingest, or `None` if none
/// has ever landed. Backs both `GET /private/tfl-line-status` (the poller's
/// startup freshness check) and the public `/public/freshness` endpoint.
pub async fn last_tfl_line_status_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (computed_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(computed_at) FROM line_status WHERE source = 'tfl'")
            .fetch_one(pool)
            .await?;
    Ok(computed_at)
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

pub async fn last_station_samples_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (polled_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(polled_at) FROM station_samples")
            .fetch_one(pool)
            .await?;
    Ok(polled_at)
}

pub async fn last_station_full_coverage_samples_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(resolved_at) FROM station_full_coverage_samples")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

/// Timestamp of the most recently *delivered* schedule feed (i.e.
/// `MAX(delivered_at)`, the delivery zip's own mtime -- not
/// `MAX(ingested_at)`, when this table happened to be written to), or
/// `None` if `schedule_feed_ingests` has never been populated. Backs both
/// `GET /private/schedule-feed-ingests` (the `schedule-ingest` crate's
/// startup check) and the public `/public/freshness` endpoint's
/// `schedule_feed` field -- using `delivered_at` here is what makes that
/// freshness signal mean "when did a real feed delivery last land", not
/// "when did `schedule-ingest` last happen to run a cycle that processed
/// one" (see
/// docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md).
pub async fn last_schedule_feed_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (delivered_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(delivered_at) FROM schedule_feed_ingests")
            .fetch_one(pool)
            .await?;
    Ok(delivered_at)
}

/// Records one verified schedule-feed delivery, keyed on `delivered_at` --
/// the delivery zip's own mtime, the one stable identifier a plain-overwrite
/// delivery has (there is no sequence number -- see this table's own
/// migration). `ON CONFLICT (delivered_at) DO NOTHING`, not an upsert -- a
/// re-POST of an already-recorded delivery (e.g. after `schedule-ingest`
/// restarts and re-observes a delivery it already recorded, since it keeps
/// no persistent state of its own) is a harmless no-op, not an error,
/// matching this route's own idempotency needs -- `schedule-ingest` itself
/// doesn't track "have I already POSTed this" locally (state lives here).
pub async fn insert_schedule_feed_ingest(
    pool: &PgPool,
    delivered_at: chrono::DateTime<chrono::Utc>,
    ingested_at: chrono::DateTime<chrono::Utc>,
    files: &serde_json::Value,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO schedule_feed_ingests (delivered_at, ingested_at, files) VALUES ($1, $2, $3) \
         ON CONFLICT (delivered_at) DO NOTHING",
    )
    .bind(delivered_at)
    .bind(ingested_at)
    .bind(files)
    .execute(pool)
    .await?;
    Ok(())
}

/// Upserts a batch of resolved STANOX/CRS rows. Every daily delivery is a
/// full refresh (see this table's migration comment), so this is always a
/// complete-table upsert-by-`stanox`, never a delta -- no separate
/// "delete rows missing from today's delivery" step is needed, since every
/// successful run re-asserts every row it still resolves.
pub async fn upsert_stanox_crs(pool: &PgPool, records: &[common::StanoxCrsRecord]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for record in records {
        sqlx::query(
            r#"
            INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, updated_at)
            VALUES ($1, $2, $3, $4, $5, NOW())
            ON CONFLICT (stanox) DO UPDATE SET
                crs             = EXCLUDED.crs,
                tiploc          = EXCLUDED.tiploc,
                station_name    = EXCLUDED.station_name,
                source_sequence = EXCLUDED.source_sequence,
                updated_at      = NOW()
            "#,
        )
        .bind(&record.stanox)
        .bind(&record.crs)
        .bind(&record.tiploc)
        .bind(&record.station_name)
        .bind(record.source_sequence)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Row shape for `list_stanox_crs`'s `SELECT` -- a dedicated `FromRow`
/// struct, matching this file's own established convention for any
/// multi-column query result (see `IncidentRow`; `train_tracking.rs`'s
/// `TrackedTrainRow`/`TrackedTrainListItem`), rather than a bare tuple --
/// this repo reserves raw tuple `query_as` for single-column results only
/// (e.g. `last_stations_fetch`'s `(Option<DateTime<Utc>>,)`).
#[derive(Debug, Clone, sqlx::FromRow)]
struct StanoxCrsRow {
    stanox: String,
    crs: String,
    tiploc: String,
    station_name: String,
    source_sequence: i32,
}

impl From<StanoxCrsRow> for common::StanoxCrsRecord {
    fn from(row: StanoxCrsRow) -> Self {
        common::StanoxCrsRecord {
            stanox: row.stanox,
            crs: row.crs,
            tiploc: row.tiploc,
            station_name: row.station_name,
            source_sequence: row.source_sequence,
        }
    }
}

/// The full current STANOX/CRS table, ordered by `stanox` for a stable,
/// reviewable response shape -- backs `GET /private/stanox-crs`, which
/// `trust-consumer`'s periodic reload consumes directly (Task 5), unlike
/// every `last_*_fetch` query in this file, which only returns a
/// timestamp.
pub async fn list_stanox_crs(pool: &PgPool) -> Result<Vec<common::StanoxCrsRecord>> {
    let rows = sqlx::query_as::<_, StanoxCrsRow>(
        "SELECT stanox, crs, tiploc, station_name, source_sequence FROM stanox_crs ORDER BY stanox",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(common::StanoxCrsRecord::from)
        .collect())
}

/// Every `stanox_crs` row for one CRS -- the "which TIPLOCs does this
/// station's code cover" lookup Decision 3 step 3 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md
/// calls for (`list_stanox_crs`'s existing `WHERE`-less shape returns
/// everything; this is its `WHERE crs = $1` sibling). `UPPER(...)` on both
/// sides, matching `TRACKED_TRAIN_STATE_SELECT`'s own established
/// convention -- `tracked_trains.pin_origin_crs` is never
/// case-normalized at write time (`validate_pin` doesn't uppercase it),
/// so a case-insensitive compare here is load-bearing, not defensive
/// tidiness.
pub async fn list_stanox_crs_for_crs(
    pool: &PgPool,
    crs: &str,
) -> Result<Vec<common::StanoxCrsRecord>> {
    let rows = sqlx::query_as::<_, StanoxCrsRow>(
        "SELECT stanox, crs, tiploc, station_name, source_sequence FROM stanox_crs \
         WHERE UPPER(crs) = UPPER($1)",
    )
    .bind(crs)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(common::StanoxCrsRecord::from)
        .collect())
}

/// Reverse of the above: one CRS for a TIPLOC, or `None` if unmapped.
/// Used to resolve a matched schedule's own terminus CRS
/// (`schedule_destination_crs`) from its last calling point's TIPLOC.
/// `LIMIT 1`: a TIPLOC maps to at most one real station in practice, but
/// this doesn't assume uniqueness at the SQL level (no `UNIQUE`
/// constraint on `stanox_crs.tiploc` -- multiple STANOX rows can share a
/// TIPLOC, e.g. different platforms/areas of one physical location), so
/// this is "a plausible one," not "the guaranteed only one."
pub async fn crs_for_tiploc(pool: &PgPool, tiploc: &str) -> Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT crs FROM stanox_crs WHERE UPPER(tiploc) = UPPER($1) LIMIT 1")
            .bind(tiploc)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(crs,)| crs))
}

/// Batched sibling of `crs_for_tiploc` -- one `WHERE UPPER(tiploc) =
/// ANY($1)` query resolving every distinct TIPLOC in a calling-point list,
/// instead of one query per TIPLOC. Mirrors the existing single/batch
/// pairing convention `trains::find_or_create_train`/
/// `find_or_create_trains_batch` already establishes. Keys are
/// `UPPER(tiploc)`; a TIPLOC with no `stanox_crs` row is simply absent from
/// the map (degrade, don't fabricate -- same posture `crs_for_tiploc`
/// already has for a single lookup).
pub async fn crs_for_tiplocs_batch(
    pool: &PgPool,
    tiplocs: &[String],
) -> Result<HashMap<String, String>> {
    if tiplocs.is_empty() {
        return Ok(HashMap::new());
    }
    let upper: Vec<String> = tiplocs.iter().map(|t| t.to_uppercase()).collect();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT UPPER(tiploc), UPPER(crs) FROM stanox_crs WHERE UPPER(tiploc) = ANY($1)",
    )
    .bind(&upper)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Upserts one line's population for one service date -- wholesale
/// replaces any existing row for that `(line_id, service_date)` (a fresh
/// CIF read supersedes the prior one entirely, never merged). `population`
/// is stored opaquely; `api` never deserializes it into
/// `schedule_query::LinePopulationEntry` -- only `schedule-reference`
/// (writer) and `full-coverage-consumer` (reader) need that shape.
pub async fn upsert_schedule_line_population(
    pool: &PgPool,
    line_id: &str,
    service_date: chrono::NaiveDate,
    population: &serde_json::Value,
) -> Result<()> {
    sqlx::query(
        r#"
        INSERT INTO schedule_line_population (line_id, service_date, population, updated_at)
        VALUES ($1, $2, $3, now())
        ON CONFLICT (line_id, service_date) DO UPDATE SET
            population = EXCLUDED.population,
            updated_at = EXCLUDED.updated_at
        "#,
    )
    .bind(line_id)
    .bind(service_date)
    .bind(population)
    .execute(pool)
    .await?;
    Ok(())
}

/// Reads one line's population for one service date, if published.
/// `None` when `full-coverage-consumer` reloads before `schedule-reference`
/// has ever published that day's population yet (a real, expected startup
/// race, not an error -- the caller treats it the same as "empty
/// population," per Decision 2e's own Pending semantics).
pub async fn get_schedule_line_population(
    pool: &PgPool,
    line_id: &str,
    service_date: chrono::NaiveDate,
) -> Result<Option<serde_json::Value>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT population FROM schedule_line_population WHERE line_id = $1 AND service_date = $2",
    )
    .bind(line_id)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.try_get("population"))
        .transpose()
        .map_err(Into::into)
}

/// One `POST /private/schedule-network-departures` batch element --
/// query-scoped, deserialized straight off the request body by
/// `routes::ingest::post_schedule_network_departures`. Defined here
/// (the data layer), not in `routes/ingest.rs`, so the data layer never
/// depends on a route-layer type -- same direction as every other
/// dependency between these two files. `departures` stays an opaque
/// `serde_json::Value` -- see this table's own migration comment for why.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleNetworkDeparturesRow {
    pub crs: String,
    pub service_date: chrono::NaiveDate,
    pub departures: serde_json::Value,
}

/// Upserts one cycle's batch of per-station CIF-derived departures --
/// wholesale replaces any existing row for each `(crs, service_date)` (a
/// fresh cycle's grouping pass supersedes the prior one entirely, never
/// merged), same shape as `upsert_full_coverage_line_stats`/
/// `upsert_stanox_crs`: one transaction, one `INSERT ... ON CONFLICT` per
/// row.
pub async fn upsert_schedule_network_departures(
    pool: &PgPool,
    rows: &[ScheduleNetworkDeparturesRow],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO schedule_network_departures (crs, service_date, departures, updated_at)
            VALUES ($1, $2, $3, now())
            ON CONFLICT (crs, service_date) DO UPDATE SET
                departures = EXCLUDED.departures,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&row.crs)
        .bind(row.service_date)
        .bind(&row.departures)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Reads one station's CIF-derived departures for one service date, if
/// published. `None` when no `schedule-reference` cycle has published for
/// this `(crs, service_date)` yet -- either the station never appears in
/// `stanox_crs` at all, or (far more likely in practice) the current
/// service date's cycle just hasn't run yet. The caller
/// (`routes::departures::get_station_schedule_departures`) maps this to a
/// `404`, the same honesty split `get_station_departures` already uses for
/// `station_samples`.
pub async fn latest_schedule_network_departures(
    pool: &PgPool,
    crs: &str,
    service_date: chrono::NaiveDate,
) -> Result<Option<serde_json::Value>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT departures FROM schedule_network_departures WHERE crs = $1 AND service_date = $2",
    )
    .bind(crs)
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    row.map(|r| r.try_get("departures"))
        .transpose()
        .map_err(Into::into)
}

/// One `POST /private/schedule-destination-departures` batch element -- one
/// DEPARTURE, not one destination bucket. Query-scoped, deserialized
/// straight off the request body by
/// `routes::ingest::post_schedule_destination_departures`. Defined here
/// (the data layer), not in `routes/ingest.rs`, so the data layer never
/// depends on a route-layer type -- same direction as every other
/// dependency between these two files.
///
/// Deliberately NOT shaped like `ScheduleNetworkDeparturesRow` above, which
/// carries an opaque `serde_json::Value` bucket. Every field here is a flat
/// scalar mapping one-to-one onto a column of
/// `schedule_destination_departures`, because the destination product needs
/// to be FILTERED and PAGINATED in SQL rather than stored and relayed
/// whole. See
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
/// §3 for why the bucket shape could not work here.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleDestinationDeparturesRow {
    pub service_date: chrono::NaiveDate,
    pub destination_crs: String,
    pub scheduled: chrono::NaiveTime,
    pub train_uid: String,
    pub origin_crs: String,
    pub true_origin_crs: Option<String>,
    /// The schedule's terminating calling point's own `booked_arrival`,
    /// mirroring `true_origin_crs`'s plumbing exactly -- see
    /// `schedule_query::DestinationDeparture::destination_arrival`'s own
    /// doc comment and
    /// docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.
    /// Missing from the wire JSON deserializes as `None` (Option<T> fields
    /// are optional-by-default for self-describing formats like JSON),
    /// same as `true_origin_crs`.
    pub destination_arrival: Option<chrono::NaiveTime>,
}

/// An opaque-to-the-caller position in one station's ordered results: the
/// last row of the page just returned. The next page is everything
/// strictly after it under `ORDER BY scheduled, train_uid`.
///
/// Two components, not three like the destination-keyed predecessor this
/// replaces: `origin_crs` (the calling point / station being searched) is
/// now the FIXED equality filter for the whole query, constant across
/// every row of one response, so it carries no ordering information and
/// would be a redundant cursor component. `train_uid` alone is a
/// sufficient tiebreaker on `scheduled` because a schedule's `train_uid`
/// is unique per `(service_date, origin_crs)` under normal CIF data (see
/// the calling-point-search design doc's Open Question 2 for the one
/// theoretical exception this doesn't try to rule out).
///
/// `routes::trains` encodes this onto the wire and parses it back; nothing
/// outside that module should construct one from user input without going
/// through that parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallingPointDepartureCursor {
    pub scheduled: chrono::NaiveTime,
    pub train_uid: String,
}

/// One page of calling-point-search results.
///
/// `departures` elements are `serde_json::Value` in the
/// `{"uid", "destination_crs", "true_origin_crs", "scheduled": "HH:MM:SS"}`
/// shape -- the exact element shape `crate::render::calling_point_departure_json`
/// reads. `next_cursor` is `Some` only when there is genuinely at least one
/// more row (the query fetches `limit + 1` to know that).
#[derive(Debug, Clone)]
pub struct CallingPointDeparturePage {
    pub departures: Vec<serde_json::Value>,
    pub next_cursor: Option<CallingPointDepartureCursor>,
}

/// Replaces one CIF delivery's worth of per-destination departures.
///
/// **Deliberately NOT shaped like `upsert_schedule_network_departures`
/// above.** That one loops a single-row `INSERT ... ON CONFLICT` per row
/// inside a transaction, which is correct for its ~2,500 rows and would be
/// ~377,000 round trips here. This is instead, in ONE transaction:
///
/// 1. `DELETE FROM schedule_destination_departures WHERE service_date =
///    ANY(...)` over the batch's distinct service dates, then
/// 2. one multi-row `INSERT ... SELECT * FROM UNNEST(...)`.
///
/// The pair preserves the same "wholesale replace, never merged" posture
/// both existing CIF-derived products document, just at day granularity
/// instead of per-key. `UNNEST` follows this crate's own established batch
/// pattern -- see `crate::data::trains::find_or_create_trains_batch` and
/// `mark_trains_resolved_batch` for the identical
/// build-parallel-Vecs-then-bind style. Five bind parameters regardless of
/// row count, so the 65,535-parameter protocol ceiling is not in play.
///
/// **An empty `rows` is a no-op, and that is load-bearing.** A publish that
/// produced nothing (an upstream parse failure, a delivery with no
/// schedules) must not be allowed to delete a service date's real
/// timetable. The per-row `ON CONFLICT` loop it replaces could not have
/// this bug; a DELETE-then-INSERT can, so it is guarded and tested
/// (`upsert_with_an_empty_batch_does_not_wipe_the_day`).
///
/// `ON CONFLICT DO NOTHING` on the insert: the primary key covers all five
/// columns, so a conflict can only mean the publisher emitted a
/// byte-identical duplicate. Dropping it silently is strictly better than
/// failing a ~377,000-row batch over one pathological schedule. The return
/// value is therefore rows actually INSERTED, which may be under
/// `rows.len()` in that case.
pub async fn upsert_schedule_destination_departures(
    pool: &PgPool,
    rows: &[ScheduleDestinationDeparturesRow],
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }

    let service_dates: Vec<chrono::NaiveDate> = rows.iter().map(|r| r.service_date).collect();
    let destination_crs: Vec<&str> = rows.iter().map(|r| r.destination_crs.as_str()).collect();
    let scheduled: Vec<chrono::NaiveTime> = rows.iter().map(|r| r.scheduled).collect();
    let train_uids: Vec<&str> = rows.iter().map(|r| r.train_uid.as_str()).collect();
    let origin_crs: Vec<&str> = rows.iter().map(|r| r.origin_crs.as_str()).collect();
    let true_origin_crs: Vec<Option<&str>> =
        rows.iter().map(|r| r.true_origin_crs.as_deref()).collect();
    let destination_arrival: Vec<Option<chrono::NaiveTime>> =
        rows.iter().map(|r| r.destination_arrival).collect();

    // Normally exactly one date. Handled as a set anyway so a batch that
    // straddles a rail-day boundary replaces both days rather than half of
    // one -- and so the DELETE can never be wider than what is being
    // written.
    let mut distinct_dates = service_dates.clone();
    distinct_dates.sort_unstable();
    distinct_dates.dedup();

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = ANY($1::date[])")
        .bind(&distinct_dates)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query(
        "INSERT INTO schedule_destination_departures \
            (service_date, destination_crs, scheduled, train_uid, origin_crs, true_origin_crs, destination_arrival) \
         SELECT * FROM UNNEST($1::date[], $2::text[], $3::time[], $4::text[], $5::text[], $6::text[], $7::time[]) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&service_dates)
    .bind(&destination_crs)
    .bind(&scheduled)
    .bind(&train_uids)
    .bind(&origin_crs)
    .bind(&true_origin_crs)
    .bind(&destination_arrival)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
}

/// One `schedule_destination_departures` row for one train_uid/service_date,
/// used to reconstruct a scheduled stop list when `trains.calling_points`
/// hasn't been populated by schedule-matching (`crates/api/src/data/journey.rs`'s
/// fallback source -- see
/// docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md §0.2).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct CallingPointDepartureRow {
    pub origin_crs: String,
    pub scheduled: chrono::NaiveTime,
    pub true_origin_crs: Option<String>,
    pub destination_crs: Option<String>,
}

/// Every departure-bearing calling point of `train_uid`'s schedule on
/// `service_date`, chronological. See `CallingPointDepartureRow`'s doc
/// comment for why this exists; see the design doc §0.2 for why the
/// schedule's own terminus is NOT among these rows (no `booked_departure`
/// for a `Terminate` calling point) -- the caller appends it separately.
pub async fn list_calling_point_departures_for_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: chrono::NaiveDate,
) -> Result<Vec<CallingPointDepartureRow>> {
    let rows = sqlx::query_as::<_, CallingPointDepartureRow>(
        "SELECT origin_crs, scheduled, true_origin_crs, destination_crs \
         FROM schedule_destination_departures \
         WHERE train_uid = $1 AND service_date = $2 \
         ORDER BY scheduled",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Cheap, day-scoped existence probe backing the 404-versus-`200 []` split.
///
/// **Scoped to the DAY, not to the destination**, and that is a deliberate
/// semantic change from the bucket shape this replaces. Under a flat table
/// an empty result set is empty whether the destination is unknown or the
/// timetable is missing, so the only honest thing left to probe is whether
/// today's CIF publish landed at all. Consequently `404` now means "we do
/// not have today's timetable" and an unknown or train-less destination CRS
/// returns an empty `200`. See
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
/// §3 and §7 item 3 -- this diverges from
/// `routes::departures::get_station_schedule_departures`' own split, which
/// is unchanged.
///
/// One indexed lookup: `service_date` is the primary key's leading column.
async fn schedule_destination_departures_published_for(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
) -> Result<bool> {
    let probe: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM schedule_destination_departures WHERE service_date = $1 LIMIT 1",
    )
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(probe.is_some())
}

/// The calling-point-first train search's one read: a bounded index range
/// scan over `schedule_destination_departures_calling_point_idx`, with a
/// keyset cursor. Replaces `search_schedule_destination_departures`
/// (destination-first) in place -- see
/// docs/superpowers/specs/2026-09-08-calling-point-train-search-design.md.
///
/// `station_crs` is REQUIRED and matches the `origin_crs` column -- already
/// "the calling point of this row" (`schedule_query::DestinationDeparture`'s
/// own doc comment), so no data-model change was needed for the primary
/// key, only a new leading index. `true_origin_crs` and `destination_crs`
/// are BOTH optional filters layered on top, independent of each other and
/// of `station_crs`.
///
/// `scheduled_from` is an INCLUSIVE lower bound, the caller's already-
/// combined `max(now, from)`. `to_time` is an INCLUSIVE upper bound. Both
/// carry the exact same reasoning as the predecessor query.
///
/// `destination_arrival_from`/`destination_arrival_to` are a SEPARATE
/// inclusive bound pair on `destination_arrival`, independent of
/// `scheduled_from`/`to_time` above -- the former is "when does the train
/// reach `destination_crs`", the latter is "when is the train at
/// `station_crs`". Both pairs may be supplied at once; neither widens or
/// implies the other. The route layer (not this function) rejects either
/// being set without `destination_crs` -- this function applies whatever
/// it is given, filter-shaped, with no cross-field validation of its own,
/// matching how it already treats every other Option argument here. See
/// docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.
///
/// `Ok(None)` means no CIF publish has landed for `service_date` at all
/// (maps to a 404). `Ok(Some(page))` with an empty `page.departures` means
/// the day IS published and the filters matched nothing (a 200 with an
/// empty `results` array). Reuses
/// `schedule_destination_departures_published_for` unchanged -- that probe
/// was already day-scoped, not destination-scoped, so it needs no change
/// for the new leading column.
#[allow(clippy::too_many_arguments)]
pub async fn search_schedule_calling_point_departures(
    pool: &PgPool,
    station_crs: &str,
    service_date: chrono::NaiveDate,
    scheduled_from: chrono::NaiveTime,
    true_origin_crs: Option<&str>,
    destination_crs: Option<&str>,
    to_time: Option<chrono::NaiveTime>,
    destination_arrival_from: Option<chrono::NaiveTime>,
    destination_arrival_to: Option<chrono::NaiveTime>,
    after: Option<&CallingPointDepartureCursor>,
    limit: i64,
) -> Result<Option<CallingPointDeparturePage>> {
    let fetch = limit.saturating_add(1);

    let rows: Vec<(
        String,
        String,
        Option<String>,
        chrono::NaiveTime,
        Option<chrono::NaiveTime>,
    )> = sqlx::query_as(
        r#"
            SELECT train_uid, destination_crs, true_origin_crs, scheduled, destination_arrival
            FROM schedule_destination_departures
            WHERE service_date = $1
              AND origin_crs = $2
              AND scheduled >= $3
              AND ($4::text IS NULL OR true_origin_crs = $4)
              AND ($5::text IS NULL OR destination_crs = $5)
              AND ($6::time IS NULL OR scheduled <= $6)
              AND ($7::time IS NULL OR destination_arrival >= $7)
              AND ($8::time IS NULL OR destination_arrival <= $8)
              AND ($9::time IS NULL
                   OR (scheduled, train_uid) > ($9, $10))
            ORDER BY scheduled, train_uid
            LIMIT $11
            "#,
    )
    .bind(service_date)
    .bind(station_crs)
    .bind(scheduled_from)
    .bind(true_origin_crs)
    .bind(destination_crs)
    .bind(to_time)
    .bind(destination_arrival_from)
    .bind(destination_arrival_to)
    .bind(after.map(|c| c.scheduled))
    .bind(after.map(|c| c.train_uid.as_str()))
    .bind(fetch)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        if !schedule_destination_departures_published_for(pool, service_date).await? {
            return Ok(None);
        }
        return Ok(Some(CallingPointDeparturePage {
            departures: Vec::new(),
            next_cursor: None,
        }));
    }

    let has_more = rows.len() as i64 > limit;
    let page_rows = if has_more {
        &rows[..limit as usize]
    } else {
        &rows[..]
    };

    let next_cursor = if has_more {
        page_rows.last().map(
            |(train_uid, _, _, scheduled, _)| CallingPointDepartureCursor {
                scheduled: *scheduled,
                train_uid: train_uid.clone(),
            },
        )
    } else {
        None
    };

    let departures = page_rows
        .iter()
        .map(
            |(train_uid, destination_crs, true_origin_crs, scheduled, destination_arrival)| {
                serde_json::json!({
                    "uid": train_uid,
                    "destination_crs": destination_crs,
                    "true_origin_crs": true_origin_crs,
                    "scheduled": scheduled.format("%H:%M:%S").to_string(),
                    "destination_arrival": destination_arrival.map(|t| t.format("%H:%M:%S").to_string()),
                })
            },
        )
        .collect();

    Ok(Some(CallingPointDeparturePage {
        departures,
        next_cursor,
    }))
}

/// Upserts one line's full-coverage stats row -- wholesale replaces any
/// existing row for that `line_id` (a live snapshot, never merged/append).
pub async fn upsert_full_coverage_line_stats(
    pool: &PgPool,
    rows: &[common::FullCoverageLineStatsRow],
) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;
    for row in rows {
        sqlx::query(
            r#"
            INSERT INTO full_coverage_line_stats
                (line_id, service_date, availability, total, delayed, cancelled, skipped, avg_delay_minutes, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, now())
            ON CONFLICT (line_id) DO UPDATE SET
                service_date      = EXCLUDED.service_date,
                availability      = EXCLUDED.availability,
                total             = EXCLUDED.total,
                delayed           = EXCLUDED.delayed,
                cancelled         = EXCLUDED.cancelled,
                skipped           = EXCLUDED.skipped,
                avg_delay_minutes = EXCLUDED.avg_delay_minutes,
                updated_at        = EXCLUDED.updated_at
            "#,
        )
        .bind(&row.line_id)
        .bind(row.service_date)
        .bind(&row.availability)
        .bind(row.stats.total as i32)
        .bind(row.stats.delayed as i32)
        .bind(row.stats.cancelled as i32)
        .bind(row.stats.skipped as i32)
        .bind(row.stats.avg_delay_minutes)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }
    tx.commit().await?;
    Ok(count)
}

/// The most recent `updated_at` across every `full_coverage_line_stats`
/// row -- the freshness-only GET shape (Correction 2), mirroring
/// `last_station_samples_fetch`'s own shape. The real reader of the rows
/// themselves is `aggregator`'s own direct SQL
/// (`load_full_coverage_line_stats`, Task 14), not this route.
pub async fn last_full_coverage_line_stats_fetch(
    pool: &PgPool,
) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (fetched_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(updated_at) FROM full_coverage_line_stats")
            .fetch_one(pool)
            .await?;
    Ok(fetched_at)
}

/// The latest `StationSample` polled for a single station, or `None` if
/// `station_samples` has no row for that CRS yet. `station_samples` is
/// wholesale-replaced per poll (one row per station, no history -- see
/// `upsert_station_samples`), so "latest" here just means "the current
/// row", not a query over a time range. Backs `crates/api/src/data/eta_blend.rs`'s
/// read-time Darwin/TRUST correlation (`routes/train.rs`'s
/// `blend_darwin_eta`), which needs one station's current departure board
/// to look up against a tracked train's pin/next-calling-point.
pub async fn latest_station_sample(pool: &PgPool, crs: &str) -> Result<Option<StationSample>> {
    use sqlx::Row;
    let row = sqlx::query("SELECT crs, polled_at, departures FROM station_samples WHERE crs = $1")
        .bind(crs)
        .fetch_optional(pool)
        .await?;

    row.map(|row| {
        let departures_json: serde_json::Value = row.try_get("departures")?;
        Ok(StationSample {
            crs: row.try_get("crs")?,
            polled_at: row.try_get("polled_at")?,
            departures: serde_json::from_value(departures_json)?,
        })
    })
    .transpose()
}

/// Every `station_full_coverage_samples` row for one CRS, one per
/// operator that has resolved this cycle. Full-coverage analog of
/// `latest_station_sample`, one level finer -- design doc Decision 2.
/// Empty `Vec` for every station today: no producer writes this table yet.
pub async fn latest_station_full_coverage_samples(
    pool: &PgPool,
    crs: &str,
) -> Result<Vec<StationFullCoverageSample>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT crs, operator, resolved_at, stats FROM station_full_coverage_samples WHERE crs = $1",
    )
    .bind(crs)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            let stats_json: serde_json::Value = row.try_get("stats")?;
            Ok(StationFullCoverageSample {
                crs: row.try_get("crs")?,
                operator: row.try_get("operator")?,
                resolved_at: row.try_get("resolved_at")?,
                stats: serde_json::from_value(stats_json)?,
            })
        })
        .collect()
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

/// Every line whose `mode_name` is in `modes`. Plural because TfL's
/// `/Line/Mode/{modes}/Status` takes a comma-separated list and this API
/// mimics its URL scheme — and because the frontend's list pages want
/// National Rail and the five TfL modes in one round trip.
pub async fn line_status_for_modes(pool: &PgPool, modes: &[String]) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name, operators, statuses, computed_at FROM line_status WHERE mode_name = ANY($1)",
    )
    .bind(modes)
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

pub struct DailyStatsRow {
    pub day: chrono::NaiveDate,
    pub sample_cycles: i64,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
    pub skipped: i64,
    pub running_count: i64,
    pub delay_minutes_sum: f64,
}

/// Reads the `line_status_daily_stats` rollup for one line over
/// `[from, to]` (inclusive both ends, matching the DATE column's own
/// semantics -- unlike the sibling `line_status_history_for_range`'s
/// timestamp `BETWEEN`, there is no time-of-day component to reason
/// about). Returns an empty vec for an unknown `line_id` -- no error, no
/// special-casing -- matching `line_status_history_for_range`'s existing
/// behavior for the same case (see
/// docs/superpowers/specs/2026-08-31-line-history-graphics-design.md,
/// Error handling).
pub async fn daily_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<DailyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum
         FROM line_status_daily_stats
         WHERE line_id = $1 AND day BETWEEN $2 AND $3
         ORDER BY day",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(DailyStatsRow {
                day: row.try_get("day")?,
                sample_cycles: row.try_get("sample_cycles")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}

pub struct HalfHourlyStatsRow {
    pub half_hour_start: chrono::DateTime<chrono::Utc>,
    pub sample_cycles: i64,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
    pub skipped: i64,
    pub running_count: i64,
    pub delay_minutes_sum: f64,
}

/// Half-hourly-granularity sibling of `daily_stats_for_range` -- same
/// shape, same "empty vec for an unknown line_id, no error" behavior, same
/// read-time rate derivation posture (never stored pre-divided). `from`/
/// `to` are real instants (`DateTime<Utc>`), not calendar dates -- a
/// 30-minute bucket has no calendar-day analog to round-trip through,
/// unlike the daily route (Decision 6 of
/// docs/superpowers/specs/2026-09-02-trend-chart-granularity-design.md,
/// written when this was still an hourly bucket -- the reasoning is
/// unchanged at 30 minutes). Originally `hourly_stats_for_range` reading
/// `line_status_hourly_stats`; renamed alongside that table when the
/// bucket size was halved -- see git history for the hourly-era version.
pub async fn half_hourly_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<HalfHourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum
         FROM line_status_half_hourly_stats
         WHERE line_id = $1 AND half_hour_start BETWEEN $2 AND $3
         ORDER BY half_hour_start",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HalfHourlyStatsRow {
                half_hour_start: row.try_get("half_hour_start")?,
                sample_cycles: row.try_get("sample_cycles")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}

/// Sub-daily sibling of `half_hourly_stats_for_range`, for the two
/// intermediate granularities (Decision 1 of
/// docs/superpowers/specs/2026-09-05-configurable-trend-granularity-design.md):
/// 1-hour and 6-hour buckets, derived at READ time by grouping
/// `line_status_half_hourly_stats` rows via `date_bin` -- no new table, no
/// new aggregator write path (Decision 2; every column here is a SUM, and
/// summing sums is lossless per that decision's Correction 4).
///
/// `bucket_minutes` is NEVER taken from raw request input: it is a plain
/// bound parameter, always one of exactly two caller-supplied literals (60
/// or 360) from this crate's two thin route handlers
/// (`routes::line_status::get_line_hourly_stats`/`get_line_six_hourly_stats`)
/// -- never string-interpolated into the query text, so there is no
/// SQL-injection surface despite selecting the bucket width dynamically.
///
/// The `date_bin` origin (`2000-01-01T00:00:00Z`, a UTC midnight) is
/// arbitrary but load-bearing: `utc_half_hour_start`
/// (`crates/aggregator/src/queries.rs`) only ever produces `:00`/`:30` UTC
/// timestamps, and any UTC midnight divides evenly into 30-minute, 1-hour,
/// AND 6-hour buckets alike, so this origin aligns every bucket boundary
/// to whole hours regardless of which `bucket_minutes` value is requested
/// -- no origin-dependent edge case to get wrong. `date_bin` requires
/// PostgreSQL 14+; this deployment runs Postgres 16
/// (`docker-compose.yml`'s `postgres:16` image), confirmed, not assumed.
///
/// Returns the same `HalfHourlyStatsRow` shape `half_hourly_stats_for_range`
/// does (Decision 2's own sketch: "reuses the existing row shape
/// verbatim") -- its `half_hour_start` field here is the START OF
/// WHATEVER BUCKET WIDTH WAS REQUESTED, not literally a half hour.
/// Callers must not re-expose this field name to JSON unchanged for this
/// function's results -- see `routes::line_status::sub_daily_stats_to_json`,
/// which renames it to `bucketStart` for exactly this reason.
pub async fn sub_daily_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_minutes: i64,
) -> Result<Vec<HalfHourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT
            date_bin($4 * INTERVAL '1 minute', half_hour_start, TIMESTAMPTZ '2000-01-01T00:00:00Z') AS half_hour_start,
            SUM(sample_cycles)::bigint AS sample_cycles,
            SUM(total)::bigint AS total,
            SUM(delayed)::bigint AS delayed,
            SUM(cancelled)::bigint AS cancelled,
            SUM(skipped)::bigint AS skipped,
            SUM(running_count)::bigint AS running_count,
            SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_half_hourly_stats
         WHERE line_id = $1 AND half_hour_start BETWEEN $2 AND $3
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .bind(bucket_minutes)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HalfHourlyStatsRow {
                half_hour_start: row.try_get("half_hour_start")?,
                sample_cycles: row.try_get("sample_cycles")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}

// --- Decision 4 scaffolding: line_status_{daily,half_hourly}_coverage_stats reads ---

pub struct DailyCoverageStatsRow {
    pub day: chrono::NaiveDate,
    pub resolved_windows: i64,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
    pub skipped: i64,
    pub running_count: i64,
    pub delay_minutes_sum: f64,
}

/// Full-coverage sibling of `daily_stats_for_range` -- identical shape and
/// "empty vec for an unknown line_id, no error" contract, reading
/// `line_status_daily_coverage_stats` instead (`resolved_windows` in place
/// of `sample_cycles`). See
/// docs/superpowers/specs/2026-09-03-full-coverage-metrics-transition-design.md
/// Decision 4.
pub async fn daily_coverage_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<DailyCoverageStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT day, resolved_windows, total, delayed, cancelled, skipped, running_count, delay_minutes_sum
         FROM line_status_daily_coverage_stats
         WHERE line_id = $1 AND day BETWEEN $2 AND $3
         ORDER BY day",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(DailyCoverageStatsRow {
                day: row.try_get("day")?,
                resolved_windows: row.try_get("resolved_windows")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}

pub struct HalfHourlyCoverageStatsRow {
    pub half_hour_start: chrono::DateTime<chrono::Utc>,
    pub resolved_windows: i64,
    pub total: i64,
    pub delayed: i64,
    pub cancelled: i64,
    pub skipped: i64,
    pub running_count: i64,
    pub delay_minutes_sum: f64,
}

/// Half-hourly-granularity sibling of `daily_coverage_stats_for_range` --
/// same relationship `half_hourly_stats_for_range` already has to
/// `daily_stats_for_range`.
pub async fn half_hourly_coverage_stats_for_range(
    pool: &PgPool,
    line_id: &str,
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<HalfHourlyCoverageStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT half_hour_start, resolved_windows, total, delayed, cancelled, skipped, running_count, delay_minutes_sum
         FROM line_status_half_hourly_coverage_stats
         WHERE line_id = $1 AND half_hour_start BETWEEN $2 AND $3
         ORDER BY half_hour_start",
    )
    .bind(line_id)
    .bind(from)
    .bind(to)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(HalfHourlyCoverageStatsRow {
                half_hour_start: row.try_get("half_hour_start")?,
                resolved_windows: row.try_get("resolved_windows")?,
                total: row.try_get("total")?,
                delayed: row.try_get("delayed")?,
                cancelled: row.try_get("cancelled")?,
                skipped: row.try_get("skipped")?,
                running_count: row.try_get("running_count")?,
                delay_minutes_sum: row.try_get("delay_minutes_sum")?,
            })
        })
        .collect()
}

/// One row from `incidents`, by primary key. `validity_periods` is kept as
/// raw `serde_json::Value` here (not deserialized into
/// `Vec<common::ValidityPeriod>`) because the route layer needs to
/// re-render each period as camelCase JSON by hand anyway (see
/// `routes/incidents.rs`'s `to_incident_detail_json` and this plan's
/// Global Constraints) -- deserializing into the Rust struct and then
/// re-serializing through `serde_json::json!()` field-by-field would just
/// add a round trip with no benefit.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentRow {
    pub incident_id: String,
    pub summary: String,
    pub description: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    pub priority: i32,
    pub validity_periods: serde_json::Value,
    pub is_planned: bool,
    pub is_cleared: bool,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
}

/// `incident_id` is this table's primary key (see `upsert_incidents`'s own
/// `INSERT ... ON CONFLICT (incident_id)`), so this is a direct index
/// lookup -- no new index needed. Deliberately does not filter on
/// `is_cleared`: a cleared incident is still a real, fully valid detail
/// page (Decision 2 of the design spec).
pub async fn incident_by_id(pool: &PgPool, incident_id: &str) -> Result<Option<IncidentRow>> {
    let row = sqlx::query_as::<_, IncidentRow>(
        "SELECT incident_id, summary, description, operators, affected_stations, priority, \
                validity_periods, is_planned, is_cleared, first_seen_at, fetched_at \
         FROM incidents WHERE incident_id = $1",
    )
    .bind(incident_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// One append-only snapshot from `incident_history`, per the same
/// raw-JSONB rationale as `IncidentRow` above.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentHistoryRow {
    pub summary: String,
    pub description: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    pub priority: i32,
    pub validity_periods: serde_json::Value,
    pub is_planned: bool,
    pub is_cleared: bool,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

/// Newest-first, matching the `incident_history_id_time` index
/// (`(incident_id, recorded_at DESC)`, created in the initial migration)
/// exactly -- no new index needed.
pub async fn incident_history_for_id(
    pool: &PgPool,
    incident_id: &str,
) -> Result<Vec<IncidentHistoryRow>> {
    let rows = sqlx::query_as::<_, IncidentHistoryRow>(
        "SELECT summary, description, operators, affected_stations, priority, validity_periods, \
                is_planned, is_cleared, recorded_at \
         FROM incident_history WHERE incident_id = $1 ORDER BY recorded_at DESC",
    )
    .bind(incident_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentLineRefRow {
    pub line_id: String,
    pub name: String,
}

/// Which lines currently carry a status whose `disruption.source` equals
/// `source` exactly (the full `knowledgebase-incident-{id}` string, not
/// the bare id -- that's the literal value stored in the JSONB, see
/// Decision 3 of the design spec). `jsonb_array_elements` unnests
/// `line_status.statuses` (one row per line, one array element per
/// simultaneous status) so each element's `disruption.source` can be
/// compared with a plain path expression. Deliberately NOT JSONB
/// containment (`s @> '{"disruption": {"source": "..."}}'`): Postgres
/// array/object containment requires a full structural match of every
/// key in the compared object, and a real stored status object also
/// carries `severity`/`reason`/`validity`/`dataQuality`, so `@>` would
/// silently match nothing -- see the design spec's Correction 2. Also NOT
/// `line_status.source` (a same-named, unrelated top-level column:
/// `'aggregator' | 'tfl'`, which *service* wrote the row -- added by
/// `20260822120000_line_status_source.sql`). No new index: this table is
/// tens of rows total, matching this repo's own stated rationale for
/// leaving `line_status.source` itself unindexed.
pub async fn lines_currently_reporting_incident(
    pool: &PgPool,
    source: &str,
) -> Result<Vec<IncidentLineRefRow>> {
    let rows = sqlx::query_as::<_, IncidentLineRefRow>(
        "SELECT DISTINCT line_status.line_id, line_status.name \
         FROM line_status, jsonb_array_elements(statuses) AS s \
         WHERE s -> 'disruption' ->> 'source' = $1 \
         ORDER BY line_status.name",
    )
    .bind(source)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

// --- Movement Events Queries ---

/// One `train_movement_events` row, already collapsed to the latest
/// (`received_at`-DESC) event per distinct `loc_crs` for one `trains_id` --
/// the per-stop live overlay source
/// (docs/superpowers/specs/2026-09-08-journey-timetable-overlay-design.md
/// §0.4/§3.3). `loc_crs` is never `NULL` here (`WHERE loc_crs IS NOT NULL`
/// below) -- a message whose STANOX never translated to a CRS has nothing
/// to key an overlay row on and is dropped, same "degrade, don't attach to
/// the wrong stop" posture as everywhere else in this data model.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct MovementEventRow {
    pub loc_crs: String,
    pub event_type: Option<String>,
    pub planned_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub actual_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub variation_status: Option<String>,
}

/// `DISTINCT ON (UPPER(loc_crs))` keeps only the most-recently-`received_at`
/// event for each location -- so a location visited with an ARRIVAL then
/// later a DEPARTURE collapses to the DEPARTURE (the more complete, more
/// recent report), matching this app's existing "last reported" framing
/// (`train_current_state.last_reported_location`/`last_event_type`)
/// extended to a per-location granularity.
pub async fn latest_movement_event_per_location(
    pool: &PgPool,
    trains_id: i64,
) -> Result<Vec<MovementEventRow>> {
    let rows = sqlx::query_as::<_, MovementEventRow>(
        "SELECT DISTINCT ON (UPPER(loc_crs)) UPPER(loc_crs) AS loc_crs, event_type, \
                planned_timestamp, actual_timestamp, variation_status \
         FROM train_movement_events \
         WHERE trains_id = $1 AND loc_crs IS NOT NULL \
         ORDER BY UPPER(loc_crs), received_at DESC",
    )
    .bind(trains_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// `crs -> name` for every code in `crs_codes` that has a `stations` row --
/// batched sibling of the `LEFT JOIN stations` pattern used everywhere else
/// in this data model (`pin_origin_name`, etc.), for a stop list built from
/// several separate CRS codes rather than one join target. A code with no
/// reference row is simply absent from the map.
pub async fn station_names_for_crs_batch(
    pool: &PgPool,
    crs_codes: &[String],
) -> Result<HashMap<String, String>> {
    if crs_codes.is_empty() {
        return Ok(HashMap::new());
    }
    let upper: Vec<String> = crs_codes.iter().map(|c| c.to_uppercase()).collect();
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT UPPER(crs), name FROM stations WHERE UPPER(crs) = ANY($1)")
            .bind(&upper)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().collect())
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
        assert!(text_changed(
            Some(&row),
            "Points failure",
            "Delays expected"
        ));
    }

    #[test]
    fn text_changed_true_when_description_differs() {
        let row = existing("Signal failure", "Delays expected", serde_json::json!([]));
        assert!(text_changed(
            Some(&row),
            "Signal failure",
            "Disruption has now ended"
        ));
    }

    #[test]
    fn text_changed_false_when_only_validity_periods_would_differ() {
        // text_changed only compares summary/description -- validity is
        // deliberately excluded, since it doesn't require re-extraction of
        // prose that hasn't moved. This test simulates that by reusing the
        // same summary/description text_changed actually looks at; there's
        // no validity parameter to vary because text_changed never takes one.
        let row = existing("Signal failure", "Delays expected", serde_json::json!([]));
        assert!(!text_changed(
            Some(&row),
            "Signal failure",
            "Delays expected"
        ));
    }

    #[test]
    fn a_line_with_no_stored_row_is_always_changed() {
        assert!(tfl_statuses_changed(None, &serde_json::json!([])));
    }

    #[test]
    fn identical_statuses_are_not_changed() {
        let stored = serde_json::json!([{ "severity": 10, "reason": "Good Service" }]);
        let incoming = serde_json::json!([{ "severity": 10, "reason": "Good Service" }]);
        assert!(!tfl_statuses_changed(Some(&stored), &incoming));
    }

    #[test]
    fn a_new_severity_is_changed() {
        let stored = serde_json::json!([{ "severity": 10, "reason": "Good Service" }]);
        let incoming =
            serde_json::json!([{ "severity": 6, "reason": "Signal failure at Oxford Circus" }]);
        assert!(tfl_statuses_changed(Some(&stored), &incoming));
    }

    #[test]
    fn a_second_simultaneous_status_is_changed() {
        // TfL routinely reports several statuses on one line at once — a
        // planned closure alongside a live disruption. Gaining or losing
        // one is a change even if the first entry is untouched.
        let stored = serde_json::json!([{ "severity": 4, "reason": "Planned engineering work" }]);
        let incoming = serde_json::json!([
            { "severity": 4, "reason": "Planned engineering work" },
            { "severity": 6, "reason": "Signal failure at Oxford Circus" },
        ]);
        assert!(tfl_statuses_changed(Some(&stored), &incoming));
    }

    #[test]
    fn tfl_statuses_changed_ignores_sample_stats_only_differences() {
        let existing = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_stats": { "total": 40, "delayed": 3, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 1.2 }
        }]);
        let incoming = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_stats": { "total": 41, "delayed": 5, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 2.4 }
        }]);
        assert!(!tfl_statuses_changed(Some(&existing), &incoming));
    }

    #[test]
    fn tfl_statuses_changed_ignores_sample_availability_only_differences() {
        let existing = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_availability": { "state": "no-coverage" }
        }]);
        let incoming = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_availability": { "state": "below-threshold", "observed": 0, "required": 1 }
        }]);
        assert!(!tfl_statuses_changed(Some(&existing), &incoming));
    }

    #[test]
    fn tfl_statuses_changed_ignores_full_coverage_field_only_differences() {
        let existing = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "full_coverage_stats": { "total": 40, "delayed": 3, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 1.2 },
            "full_coverage_availability": { "state": "available" }
        }]);
        let incoming = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "full_coverage_stats": { "total": 41, "delayed": 5, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 2.4 },
            "full_coverage_availability": { "state": "pending" }
        }]);
        assert!(!tfl_statuses_changed(Some(&existing), &incoming));
    }

    #[test]
    fn tfl_statuses_changed_still_true_when_severity_changes_alongside_sample_stats() {
        let existing = serde_json::json!([{
            "severity": "GoodService",
            "reason": "Good Service",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_stats": { "total": 40, "delayed": 3, "cancelled": 0, "skipped": 0, "avg_delay_minutes": 1.2 }
        }]);
        let incoming = serde_json::json!([{
            "severity": "MinorDelays",
            "reason": "Minor Delays",
            "validity": { "from_date": "2026-08-22T02:00:00Z", "to_date": null, "is_now": true },
            "data_quality": "tfl",
            "sample_stats": { "total": 41, "delayed": 5, "cancelled": 1, "skipped": 0, "avg_delay_minutes": 2.4 }
        }]);
        assert!(tfl_statuses_changed(Some(&existing), &incoming));
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                tfl_line_summaries_lists_only_tfl_owned_rows -- --ignored`"]
    async fn tfl_line_summaries_lists_only_tfl_owned_rows() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES \
                ('TEST-AGG', 'test aggregator line', 'national-rail', '{NT}', '[]', 'aggregator'), \
                ('TEST-TFL', 'test tfl line', 'tube', '{TfL}', '[]', 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let summaries = tfl_line_summaries(&pool).await.expect("tfl_line_summaries");

        sqlx::query("DELETE FROM line_status WHERE line_id IN ('TEST-AGG', 'TEST-TFL')")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        let ids: Vec<&str> = summaries.iter().map(|row| row.id.as_str()).collect();
        assert!(
            ids.contains(&"TEST-TFL"),
            "a TfL-owned row should be listed"
        );
        assert!(
            !ids.contains(&"TEST-AGG"),
            "the catalogue already lists aggregator lines"
        );

        let tfl = summaries.iter().find(|row| row.id == "TEST-TFL").unwrap();
        assert_eq!(tfl.mode_name, "tube");
        assert_eq!(tfl.name, "test tfl line");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                a_re_post_with_a_changed_crs_overwrites_the_existing_row -- --ignored`"]
    async fn a_re_post_with_a_changed_crs_overwrites_the_existing_row() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let first = common::StanoxCrsRecord {
            stanox: "99999".to_string(),
            crs: "TST".to_string(),
            tiploc: "TESTLOC".to_string(),
            station_name: "TEST STATION".to_string(),
            source_sequence: 942,
        };
        upsert_stanox_crs(&pool, &[first])
            .await
            .expect("first upsert");

        let second = common::StanoxCrsRecord {
            stanox: "99999".to_string(),
            crs: "TS2".to_string(),
            tiploc: "TESTLOC".to_string(),
            station_name: "TEST STATION".to_string(),
            source_sequence: 943,
        };
        upsert_stanox_crs(&pool, &[second])
            .await
            .expect("re-upsert with changed crs");

        let rows = list_stanox_crs(&pool).await.expect("list_stanox_crs");
        let row = rows
            .iter()
            .find(|r| r.stanox == "99999")
            .expect("row present");
        assert_eq!(row.crs, "TS2", "the re-POST must overwrite, not duplicate");
        assert_eq!(row.source_sequence, 943);

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = '99999'")
            .execute(&pool)
            .await
            .expect("cleanup fixture row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                daily_stats_for_range -- --ignored`"]
    async fn daily_stats_for_range_filters_orders_and_handles_unknown_lines() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO line_status_daily_stats \
                (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-STATS', '2026-08-01', 10, 100, 5, 1, 2, 97, 120.0), \
                ('TEST-STATS', '2026-08-03', 12, 110, 6, 0, 1, 109, 90.0), \
                ('TEST-STATS', '2026-08-02', 8, 90, 4, 2, 0, 88, 60.0), \
                ('TEST-STATS', '2026-07-31', 5, 50, 1, 0, 0, 50, 10.0) \
             ON CONFLICT (line_id, day) DO UPDATE SET total = EXCLUDED.total",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let from = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap();
        let rows = daily_stats_for_range(&pool, "TEST-STATS", from, to)
            .await
            .expect("daily_stats_for_range");

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id = 'TEST-STATS'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        // 2026-07-31 falls outside [from, to] and must be excluded.
        assert_eq!(rows.len(), 3, "row outside the range should be excluded");
        let days: Vec<chrono::NaiveDate> = rows.iter().map(|r| r.day).collect();
        assert_eq!(
            days,
            vec![
                chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 8, 2).unwrap(),
                chrono::NaiveDate::from_ymd_opt(2026, 8, 3).unwrap(),
            ],
            "rows should be ordered ascending by day"
        );

        let unknown = daily_stats_for_range(&pool, "TEST-STATS-UNKNOWN", from, to)
            .await
            .expect("daily_stats_for_range for an unknown line_id");
        assert!(
            unknown.is_empty(),
            "unknown line_id should return an empty vec, not an error"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                half_hourly_stats_for_range_filters_orders_and_handles_unknown_lines -- --ignored` \
                against docker compose's postgres"]
    async fn half_hourly_stats_for_range_filters_orders_and_handles_unknown_lines() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");
        const LINE_ID: &str = "TEST-HALF-HOURLY-RANGE";

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
            .bind(LINE_ID)
            .execute(&pool)
            .await
            .unwrap();

        let h1: chrono::DateTime<chrono::Utc> = "2026-08-31T12:00:00Z".parse().unwrap();
        let h2: chrono::DateTime<chrono::Utc> = "2026-08-31T14:30:00Z".parse().unwrap();
        let out_of_range: chrono::DateTime<chrono::Utc> = "2026-08-28T00:00:00Z".parse().unwrap();

        sqlx::query(
            "INSERT INTO line_status_half_hourly_stats (line_id, half_hour_start, sample_cycles, total) VALUES \
                ($1, $2, 1, 5), ($1, $3, 1, 3), ($1, $4, 1, 99)",
        )
        .bind(LINE_ID).bind(h2).bind(h1).bind(out_of_range) // inserted out of order on purpose
        .execute(&pool).await.expect("seed rows");

        let rows = half_hourly_stats_for_range(
            &pool,
            LINE_ID,
            "2026-08-31T00:00:00Z".parse().unwrap(),
            "2026-09-01T00:00:00Z".parse().unwrap(),
        )
        .await
        .expect("half_hourly_stats_for_range");

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
            .bind(LINE_ID)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(rows.len(), 2, "the out-of-range row must be excluded");
        assert_eq!(
            rows[0].half_hour_start, h1,
            "results must be ordered ascending by half_hour_start"
        );
        assert_eq!(rows[1].half_hour_start, h2);

        let unknown = half_hourly_stats_for_range(
            &pool,
            "TEST-HALF-HOURLY-RANGE-UNKNOWN",
            "2026-08-31T00:00:00Z".parse().unwrap(),
            "2026-09-01T00:00:00Z".parse().unwrap(),
        )
        .await
        .expect("half_hourly_stats_for_range for an unknown line_id");
        assert!(unknown.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                sub_daily_stats_for_range_groups_half_hourly_rows_into_hourly_buckets -- --ignored` \
                against docker compose's postgres"]
    async fn sub_daily_stats_for_range_groups_half_hourly_rows_into_hourly_buckets() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");
        const LINE_ID: &str = "TEST-SUB-DAILY-HOURLY";

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
            .bind(LINE_ID)
            .execute(&pool)
            .await
            .unwrap();

        // Two half-hourly rows in the same 1-hour bucket (12:00, 12:30), one in
        // the next hour (13:00) -- summed columns must add losslessly
        // (Correction 4 of the design spec).
        sqlx::query(
            "INSERT INTO line_status_half_hourly_stats
                (line_id, half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum)
             VALUES
                ($1, '2026-08-31T12:00:00Z', 10, 100, 10, 2, 1, 98, 50.0),
                ($1, '2026-08-31T12:30:00Z', 12, 120, 12, 0, 2, 118, 60.0),
                ($1, '2026-08-31T13:00:00Z', 5, 50, 5, 1, 0, 49, 20.0)",
        )
        .bind(LINE_ID)
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let rows = sub_daily_stats_for_range(
            &pool,
            LINE_ID,
            "2026-08-31T00:00:00Z".parse().unwrap(),
            "2026-09-01T00:00:00Z".parse().unwrap(),
            60,
        )
        .await
        .expect("sub_daily_stats_for_range");

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
            .bind(LINE_ID)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            rows.len(),
            2,
            "two hourly buckets: 12:00-13:00 and 13:00-14:00"
        );
        assert_eq!(
            rows[0].half_hour_start,
            "2026-08-31T12:00:00Z"
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
        );
        assert_eq!(
            rows[0].sample_cycles, 22,
            "10 + 12, the two half-hour rows binned into the same hour"
        );
        assert_eq!(rows[0].total, 220);
        assert_eq!(rows[0].delayed, 22);
        assert_eq!(rows[0].delay_minutes_sum, 110.0);
        assert_eq!(
            rows[1].half_hour_start,
            "2026-08-31T13:00:00Z"
                .parse::<chrono::DateTime<chrono::Utc>>()
                .unwrap()
        );
        assert_eq!(rows[1].sample_cycles, 5);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                sub_daily_stats_for_range_with_360_minute_buckets_groups_six_hours_together -- --ignored` \
                against docker compose's postgres"]
    async fn sub_daily_stats_for_range_with_360_minute_buckets_groups_six_hours_together() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");
        const LINE_ID: &str = "TEST-SUB-DAILY-SIX-HOURLY";

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
            .bind(LINE_ID)
            .execute(&pool)
            .await
            .unwrap();

        sqlx::query(
            "INSERT INTO line_status_half_hourly_stats (line_id, half_hour_start, sample_cycles, total) VALUES
                ($1, '2026-08-31T00:00:00Z', 1, 10),
                ($1, '2026-08-31T05:30:00Z', 1, 10),
                ($1, '2026-08-31T06:00:00Z', 1, 10)",
        )
        .bind(LINE_ID)
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let rows = sub_daily_stats_for_range(
            &pool,
            LINE_ID,
            "2026-08-31T00:00:00Z".parse().unwrap(),
            "2026-09-01T00:00:00Z".parse().unwrap(),
            360,
        )
        .await
        .expect("sub_daily_stats_for_range");

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1")
            .bind(LINE_ID)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            rows.len(),
            2,
            "00:00-06:00 and 06:00-12:00 six-hour buckets"
        );
        assert_eq!(
            rows[0].sample_cycles, 2,
            "the 00:00 and 05:30 rows both fall in the first six-hour bucket"
        );
        assert_eq!(rows[1].sample_cycles, 1);
    }
}

#[cfg(test)]
mod incident_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api incident_by_id -- --ignored`"]
    async fn incident_by_id_finds_a_seeded_row_and_none_for_an_unknown_id() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO incidents (incident_id, summary, description, operators, affected_stations, priority) \
             VALUES ('TEST-INC-1', 'Signal failure', 'Delays expected', '{VT}', '{WOK}', 3) \
             ON CONFLICT (incident_id) DO UPDATE SET summary = EXCLUDED.summary",
        )
        .execute(&pool)
        .await
        .expect("seed fixture row");

        let found = incident_by_id(&pool, "TEST-INC-1")
            .await
            .expect("query")
            .expect("row should exist");
        assert_eq!(found.summary, "Signal failure");
        assert_eq!(found.affected_stations, vec!["WOK".to_string()]);

        let missing = incident_by_id(&pool, "TEST-INC-DOES-NOT-EXIST")
            .await
            .expect("query");
        assert!(missing.is_none());

        sqlx::query("DELETE FROM incidents WHERE incident_id = 'TEST-INC-1'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api incident_history_for_id -- --ignored`"]
    async fn incident_history_for_id_is_ordered_newest_first_and_empty_for_an_unknown_id() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO incident_history (incident_id, summary, description, operators, affected_stations, \
                                             priority, is_planned, recorded_at) \
             VALUES \
                ('TEST-INC-2', 'v1', 'd', '{}', '{}', 1, false, NOW() - INTERVAL '1 hour'), \
                ('TEST-INC-2', 'v2', 'd', '{}', '{}', 2, false, NOW())",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let history = incident_history_for_id(&pool, "TEST-INC-2")
            .await
            .expect("query");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].summary, "v2", "newest snapshot should be first");
        assert_eq!(history[1].summary, "v1");

        let empty = incident_history_for_id(&pool, "TEST-INC-DOES-NOT-EXIST")
            .await
            .expect("query");
        assert!(empty.is_empty());

        sqlx::query("DELETE FROM incident_history WHERE incident_id = 'TEST-INC-2'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api lines_currently_reporting_incident -- --ignored`"]
    async fn lines_currently_reporting_incident_matches_only_the_exact_jsonb_source_string() {
        // The concrete regression test for Correction 2: this must match a
        // real `knowledgebase-incident-*` source and must NOT false-positive
        // against an `ldbws-sampling`/`tfl-line-status-*` row, nor against
        // the unrelated `line_status.source` COLUMN (set to 'tfl' on the
        // second fixture row here, deliberately, to prove the query reaches
        // into the JSONB and not that column).
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) VALUES \
                ('TEST-LINE-A', 'Test Line A', 'national-rail', '{VT}', \
                 '[{\"severity\":9,\"reason\":\"x\",\"validity\":{\"from_date\":\"2026-01-01T00:00:00Z\",\"to_date\":null,\"is_now\":true}, \
                    \"data_quality\":\"knowledgebase\",\"disruption\":{\"category\":\"RealTime\",\"description\":\"x\", \
                    \"affected_stops\":[],\"affected_routes\":[],\"source\":\"knowledgebase-incident-TEST-INC-3\"}}]', \
                 'aggregator'), \
                ('TEST-LINE-B', 'Test Line B', 'tube', '{TfL}', \
                 '[{\"severity\":9,\"reason\":\"x\",\"validity\":{\"from_date\":\"2026-01-01T00:00:00Z\",\"to_date\":null,\"is_now\":true}, \
                    \"data_quality\":\"tfl\",\"disruption\":{\"category\":\"RealTime\",\"description\":\"x\", \
                    \"affected_stops\":[],\"affected_routes\":[],\"source\":\"tfl-line-status-TEST-LINE-B\"}}]', \
                 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET statuses = EXCLUDED.statuses, source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let matches =
            lines_currently_reporting_incident(&pool, "knowledgebase-incident-TEST-INC-3")
                .await
                .expect("query");
        let ids: Vec<&str> = matches.iter().map(|r| r.line_id.as_str()).collect();
        assert!(ids.contains(&"TEST-LINE-A"));
        assert!(
            !ids.contains(&"TEST-LINE-B"),
            "must not match the unrelated tfl-line-status-* source"
        );

        let no_match = lines_currently_reporting_incident(&pool, "ldbws-sampling")
            .await
            .expect("query");
        assert!(
            no_match
                .iter()
                .all(|r| r.line_id != "TEST-LINE-A" && r.line_id != "TEST-LINE-B"),
            "the shared 'ldbws-sampling' literal must never match a real incident lookup"
        );

        sqlx::query("DELETE FROM line_status WHERE line_id IN ('TEST-LINE-A', 'TEST-LINE-B')")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}

// Tested at the query level rather than through a full route/router
// harness: `routes/ingest.rs` has no existing route-level `db_tests`
// precedent to mirror (unlike, say, a hypothetical prior ingest route with
// its own axum test setup), and exercising `insert_schedule_feed_ingest`/
// `last_schedule_feed_fetch` directly against a live database already
// covers the real SQL and `ON CONFLICT DO NOTHING` idempotency behavior
// that matters here -- the route handlers themselves are thin
// serialize/deserialize wrappers around these two functions.
#[cfg(test)]
mod schedule_feed_ingest_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_feed_insert_then_last_fetch_returns_the_delivered_at \
                -- --ignored`"]
    async fn schedule_feed_insert_then_last_fetch_returns_the_delivered_at() {
        use chrono::SubsecRound;

        let pool = test_pool().await;
        // `schedule_feed_ingests.delivered_at`/`ingested_at` are both
        // `TIMESTAMPTZ`, which Postgres only ever stores at microsecond
        // precision (it silently truncates, not rounds, anything finer) --
        // whereas `chrono::Utc::now()` captures nanosecond precision from
        // the system clock. Truncate the in-memory expectations to the
        // same microsecond precision the round trip through Postgres
        // actually guarantees, rather than asserting bit-for-bit equality
        // against a precision level the database can't preserve.
        let delivered_at = chrono::Utc::now().trunc_subsecs(6);
        let ingested_at = (delivered_at + chrono::Duration::minutes(5)).trunc_subsecs(6);
        let files = serde_json::json!([{"name": "TEST.DAT", "bytes": 123}]);

        insert_schedule_feed_ingest(&pool, delivered_at, ingested_at, &files)
            .await
            .expect("insert schedule feed ingest");

        let last = last_schedule_feed_fetch(&pool)
            .await
            .expect("last_schedule_feed_fetch");
        assert_eq!(
            last,
            Some(delivered_at),
            "freshness must reflect delivered_at, not ingested_at"
        );

        sqlx::query("DELETE FROM schedule_feed_ingests WHERE delivered_at = $1")
            .bind(delivered_at)
            .execute(&pool)
            .await
            .expect("cleanup fixture row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_feed_reinserting_the_same_delivered_at_does_not_change_the_row \
                -- --ignored`"]
    async fn schedule_feed_reinserting_the_same_delivered_at_does_not_change_the_row() {
        use chrono::SubsecRound;

        let pool = test_pool().await;
        // See the trunc_subsecs(6) comment in
        // `schedule_feed_insert_then_last_fetch_returns_the_delivered_at`
        // above.
        let delivered_at = chrono::Utc::now().trunc_subsecs(6);
        let first_ingested_at = delivered_at.trunc_subsecs(6);
        let first_files = serde_json::json!([{"name": "TEST-A.DAT", "bytes": 111}]);

        insert_schedule_feed_ingest(&pool, delivered_at, first_ingested_at, &first_files)
            .await
            .expect("insert schedule feed ingest");

        // Same delivered_at (this is the whole point -- a re-POST of an
        // already-recorded delivery, e.g. after schedule-ingest restarts),
        // but a different ingested_at and files -- ON CONFLICT DO NOTHING
        // means this second insert must be a harmless no-op, not an
        // upsert.
        let second_ingested_at = (first_ingested_at + chrono::Duration::hours(1)).trunc_subsecs(6);
        let second_files = serde_json::json!([{"name": "TEST-B.DAT", "bytes": 222}]);
        insert_schedule_feed_ingest(&pool, delivered_at, second_ingested_at, &second_files)
            .await
            .expect("re-insert schedule feed ingest with the same delivered_at");

        let last = last_schedule_feed_fetch(&pool)
            .await
            .expect("last_schedule_feed_fetch");
        assert_eq!(
            last,
            Some(delivered_at),
            "the original row must survive unchanged"
        );

        let (stored_files,): (serde_json::Value,) =
            sqlx::query_as("SELECT files FROM schedule_feed_ingests WHERE delivered_at = $1")
                .bind(delivered_at)
                .fetch_one(&pool)
                .await
                .expect("fetch stored row");
        assert_eq!(
            stored_files, first_files,
            "the original files payload must survive unchanged"
        );

        sqlx::query("DELETE FROM schedule_feed_ingests WHERE delivered_at = $1")
            .bind(delivered_at)
            .execute(&pool)
            .await
            .expect("cleanup fixture row");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                schedule_feed_last_fetch_against_an_empty_table_returns_none \
                -- --ignored`"]
    async fn schedule_feed_last_fetch_against_an_empty_table_returns_none() {
        let pool = test_pool().await;

        // No fixture row inserted/deleted here for this timestamp -- this
        // asserts the zero-rows-for-this-value case, matching
        // `last_stations_fetch`'s own doc comment about `MAX(...)` over zero
        // rows returning one row with a NULL column.
        let sentinel_delivered_at = chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        sqlx::query("DELETE FROM schedule_feed_ingests WHERE delivered_at = $1")
            .bind(sentinel_delivered_at)
            .execute(&pool)
            .await
            .expect("ensure fixture delivered_at is absent");

        let last = last_schedule_feed_fetch(&pool)
            .await
            .expect("last_schedule_feed_fetch");
        // Note: this only proves `None` when the whole table is empty (the
        // realistic case for a fresh environment); if other rows already
        // exist this assertion is skipped rather than false-failing.
        let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM schedule_feed_ingests")
            .fetch_one(&pool)
            .await
            .expect("count rows");
        if count == 0 {
            assert_eq!(last, None);
        }
    }
}

/// DB-gated tests for `list_stanox_crs_for_crs`/`crs_for_tiploc` (Task 5 of
/// docs/superpowers/plans/2026-09-05-schedule-first-train-tracking-plan.md).
/// Named and shaped like `schedule_feed_ingest_query_tests` above --
/// this file's own local convention for grouping one query area's
/// DB-gated tests, rather than `train_tracking.rs`'s `db_tests` shape --
/// so this file's own test-module naming stays internally consistent.
#[cfg(test)]
mod stanox_crs_lookup_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                list_stanox_crs_for_crs -- --ignored`"]
    async fn list_stanox_crs_for_crs_returns_only_matching_rows_case_insensitively() {
        let pool = test_pool().await;
        upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-EUS".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "EUSTON".to_string(),
                    station_name: "LONDON EUSTON".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-WAT".to_string(),
                    crs: "WAT".to_string(),
                    tiploc: "WATRLMN".to_string(),
                    station_name: "LONDON WATERLOO".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let rows = list_stanox_crs_for_crs(&pool, "eus").await.expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].tiploc, "EUSTON");

        sqlx::query("DELETE FROM stanox_crs WHERE stanox IN ('TEST-EUS', 'TEST-WAT')")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                crs_for_tiploc -- --ignored`"]
    async fn crs_for_tiploc_resolves_a_known_tiploc_and_none_for_an_unknown_one() {
        let pool = test_pool().await;
        upsert_stanox_crs(
            &pool,
            &[common::StanoxCrsRecord {
                stanox: "TEST-CRE".to_string(),
                crs: "CRE".to_string(),
                tiploc: "CREWE".to_string(),
                station_name: "CREWE".to_string(),
                source_sequence: 1,
            }],
        )
        .await
        .expect("seed stanox_crs");

        assert_eq!(
            crs_for_tiploc(&pool, "crewe").await.unwrap(),
            Some("CRE".to_string())
        );
        assert_eq!(crs_for_tiploc(&pool, "NOWHERE").await.unwrap(), None);

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-CRE'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                crs_for_tiplocs_batch_resolves_every_known_tiploc_and_omits_unknown_ones \
                -- --ignored --test-threads=1`"]
    async fn crs_for_tiplocs_batch_resolves_every_known_tiploc_and_omits_unknown_ones() {
        let pool = test_pool().await;
        upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-JS-CRE".to_string(),
                    crs: "CRE".to_string(),
                    tiploc: "TEST-JS-CREWE".to_string(),
                    station_name: "CREWE".to_string(),
                    source_sequence: 1,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JS-EUS".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "TEST-JS-EUSTON".to_string(),
                    station_name: "EUSTON".to_string(),
                    source_sequence: 1,
                },
            ],
        )
        .await
        .expect("seed stanox_crs");

        let result = crs_for_tiplocs_batch(
            &pool,
            &[
                "test-js-crewe".to_string(),
                "TEST-JS-EUSTON".to_string(),
                "TEST-JS-UNKNOWN".to_string(),
            ],
        )
        .await
        .expect("crs_for_tiplocs_batch");

        assert_eq!(result.get("TEST-JS-CREWE"), Some(&"CRE".to_string()));
        assert_eq!(result.get("TEST-JS-EUSTON"), Some(&"EUS".to_string()));
        assert_eq!(result.get("TEST-JS-UNKNOWN"), None);
        assert_eq!(result.len(), 2);

        sqlx::query("DELETE FROM stanox_crs WHERE tiploc LIKE 'TEST-JS-%'")
            .execute(&pool)
            .await
            .ok();
    }
}

/// Tested at the query level rather than through a route harness -- same
/// posture as this file's other `*_query_tests` modules. The real SQL here
/// (an indexed range scan over the destination table's own primary key,
/// three optional predicates, a keyset cursor, and the day-scoped existence
/// probe that keeps "no publish today" and "published but nothing matched"
/// distinguishable) is exactly what needs live-database coverage;
/// `routes/trains.rs`'s handler is a thin parse/render wrapper over it.
///
/// Every test here owns a distinct `service_date` in 2099, because the
/// existence probe is day-scoped: sharing a date between tests would let
/// one test's fixture answer another's "is anything published?" question,
/// and a real service date could let PRODUCTION data answer it.
#[cfg(test)]
mod schedule_destination_departures_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// A distinct, far-future fixture date per test. See this module's own
    /// doc comment for why both properties matter.
    fn fixture_date(day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2099, 1, day).expect("valid fixture date")
    }

    /// `fixture_date`'s sibling for once January's 31 days of distinct
    /// fixture dates run out.
    fn fixture_date_feb(day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2099, 2, day).expect("valid fixture date")
    }

    fn time(h: u32, m: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, 0).expect("valid fixture time")
    }

    /// Midnight -- the lower bound that admits everything, used wherever a
    /// test is not itself about the `now`-forward boundary.
    fn any_time() -> chrono::NaiveTime {
        chrono::NaiveTime::MIN
    }

    async fn delete_day(pool: &PgPool, service_date: chrono::NaiveDate) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(service_date)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_destination_departures rows");
    }

    fn row(
        service_date: chrono::NaiveDate,
        destination_crs: &str,
        scheduled: chrono::NaiveTime,
        train_uid: &str,
        origin_crs: &str,
        true_origin_crs: Option<&str>,
        destination_arrival: Option<chrono::NaiveTime>,
    ) -> ScheduleDestinationDeparturesRow {
        ScheduleDestinationDeparturesRow {
            service_date,
            destination_crs: destination_crs.to_string(),
            scheduled,
            train_uid: train_uid.to_string(),
            origin_crs: origin_crs.to_string(),
            destination_arrival,
            true_origin_crs: true_origin_crs.map(str::to_string),
        }
    }

    /// Three trains to ZRD from two origins at three times -- enough to
    /// discriminate the origin filter, the time bounds and the ordering
    /// independently. The flat-shape equivalent of the original plan's
    /// single three-element JSONB bucket.
    fn fixture_rows(service_date: chrono::NaiveDate) -> Vec<ScheduleDestinationDeparturesRow> {
        vec![
            row(
                service_date,
                "ZRD",
                time(8, 22),
                "C10001",
                "EUS",
                Some("PAD"),
                None,
            ),
            row(
                service_date,
                "ZRD",
                time(10, 5),
                "C10002",
                "CRE",
                Some("SWA"),
                None,
            ),
            row(
                service_date,
                "ZRD",
                time(18, 40),
                "C10003",
                "EUS",
                Some("PAD"),
                None,
            ),
        ]
    }

    async fn seed(pool: &PgPool, service_date: chrono::NaiveDate) {
        delete_day(pool, service_date).await;
        upsert_schedule_destination_departures(pool, &fixture_rows(service_date))
            .await
            .expect("seed fixture rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_wholesale_replaces_the_whole_service_date() {
        // The flat-shape successor to the bucket table's
        // "wholesale-replaces an existing row for the same key" test. The
        // unit of replacement is now the DAY, not one (destination_crs,
        // service_date) key -- a fresh delivery's grouping supersedes the
        // prior one entirely, including destinations that vanished from it.
        let pool = test_pool().await;
        let date = fixture_date(1);
        delete_day(&pool, date).await;

        let first = vec![
            row(date, "ZRB", time(8, 0), "OLD1", "EUS", None, None),
            row(date, "ZRC", time(9, 0), "OLD2", "CRE", None, None),
        ];
        let inserted = upsert_schedule_destination_departures(&pool, &first)
            .await
            .expect("first upsert");
        assert_eq!(inserted, 2);

        // The second publish drops ZRC entirely and changes ZRB's row.
        let second = vec![row(date, "ZRB", time(9, 30), "NEW1", "CRE", None, None)];
        upsert_schedule_destination_departures(&pool, &second)
            .await
            .expect("second upsert");

        let stored: Vec<(String, chrono::NaiveTime, String)> = sqlx::query_as(
            "SELECT destination_crs, scheduled, train_uid \
             FROM schedule_destination_departures WHERE service_date = $1 \
             ORDER BY destination_crs",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back");

        assert_eq!(
            stored.len(),
            1,
            "a fresh publish wholesale-replaces the whole service_date, never merges into it"
        );
        assert_eq!(stored[0].0, "ZRB");
        assert_eq!(stored[0].1, time(9, 30));
        assert_eq!(stored[0].2, "NEW1");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_with_an_empty_batch_does_not_wipe_the_day() {
        // Guards the one way a DELETE-then-INSERT upsert can destroy real
        // data that a per-row ON CONFLICT loop never could: a publish that
        // produced no rows (a parse failure upstream, an empty grouping)
        // must be a no-op, NOT "delete today's timetable".
        let pool = test_pool().await;
        let date = fixture_date(2);
        seed(&pool, date).await;

        let affected = upsert_schedule_destination_departures(&pool, &[])
            .await
            .expect("empty upsert");
        assert_eq!(affected, 0);

        let (remaining,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM schedule_destination_departures WHERE service_date = $1",
        )
        .bind(date)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(
            remaining, 3,
            "an empty batch must leave the day untouched, never clear it"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_round_trips_true_origin_crs_including_a_null_value() {
        let pool = test_pool().await;
        let date = fixture_date(20);
        delete_day(&pool, date).await;

        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "ZRD", time(8, 0), "C30001", "EUS", Some("PAD"), None),
                row(date, "ZRD", time(9, 0), "C30002", "CRE", None, None),
            ],
        )
        .await
        .expect("seed rows");

        let stored: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT train_uid, true_origin_crs FROM schedule_destination_departures \
             WHERE service_date = $1 ORDER BY train_uid",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back");

        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0], ("C30001".to_string(), Some("PAD".to_string())));
        assert_eq!(
            stored[1],
            ("C30002".to_string(), None),
            "an absent true_origin_crs must round-trip as SQL NULL, not an empty string"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_round_trips_destination_arrival_including_a_null_value() {
        let pool = test_pool().await;
        let date = fixture_date(31);
        delete_day(&pool, date).await;

        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "ZRD", time(8, 0), "C70001", "EUS", Some("EUS"), Some(time(11, 30))),
                row(date, "ZRD", time(9, 0), "C70002", "CRE", None, None),
            ],
        )
        .await
        .expect("seed rows");

        let stored: Vec<(String, Option<chrono::NaiveTime>)> = sqlx::query_as(
            "SELECT train_uid, destination_arrival FROM schedule_destination_departures \
             WHERE service_date = $1 ORDER BY train_uid",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back");

        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0], ("C70001".to_string(), Some(time(11, 30))));
        assert_eq!(
            stored[1],
            ("C70002".to_string(), None),
            "an absent destination_arrival must round-trip as SQL NULL, not a fabricated time"
        );

        delete_day(&pool, date).await;
    }

    /// Three trains all callable-at "RDG" (the new required search key),
    /// to two different destinations, from two different true origins --
    /// exactly what's needed to prove `origin`/`destination` are
    /// independent optional filters layered on a FIXED station, not the
    /// primary key.
    fn calling_point_fixture_rows(
        service_date: chrono::NaiveDate,
    ) -> Vec<ScheduleDestinationDeparturesRow> {
        vec![
            row(
                service_date,
                "WAT",
                time(8, 22),
                "C40001",
                "RDG",
                Some("PAD"),
                None,
            ),
            row(
                service_date,
                "WAT",
                time(10, 5),
                "C40002",
                "RDG",
                Some("SWA"),
                None,
            ),
            row(
                service_date,
                "BRI",
                time(18, 40),
                "C40003",
                "RDG",
                Some("PAD"),
                None,
            ),
        ]
    }

    async fn seed_calling_point(pool: &PgPool, service_date: chrono::NaiveDate) {
        delete_day(pool, service_date).await;
        upsert_schedule_destination_departures(pool, &calling_point_fixture_rows(service_date))
            .await
            .expect("seed calling-point fixture rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_with_nothing_published_for_the_day_is_none() {
        let pool = test_pool().await;
        let date = fixture_date(21);
        delete_day(&pool, date).await;

        let result = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_with_no_filters_returns_every_row_for_that_station() {
        let pool = test_pool().await;
        let date = fixture_date(22);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 3);
        assert_eq!(
            page.departures[0],
            serde_json::json!({
                "uid": "C40001",
                "destination_crs": "WAT",
                "true_origin_crs": "PAD",
                "scheduled": "08:22:00",
                "destination_arrival": null,
            }),
            "element shape is exactly what render::calling_point_departure_json reads"
        );
        assert_eq!(page.departures[1]["uid"], "C40002");
        assert_eq!(page.departures[2]["uid"], "C40003");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_only_returns_rows_for_the_requested_station() {
        let pool = test_pool().await;
        let date = fixture_date(23);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "WAT", time(8, 0), "C50001", "RDG", None, None),
                row(date, "WAT", time(8, 5), "C50002", "SLO", None, None),
            ],
        )
        .await
        .expect("seed two-station fixture");

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C50001");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_filters_by_true_origin_independent_of_destination() {
        let pool = test_pool().await;
        let date = fixture_date(24);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            Some("PAD"),
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        let uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            vec!["C40001", "C40003"],
            "PAD-origin filter matches trains to TWO different destinations"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_filters_by_destination_independent_of_true_origin() {
        let pool = test_pool().await;
        let date = fixture_date(25);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            Some("WAT"),
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        let uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            vec!["C40001", "C40002"],
            "WAT-destination filter matches trains from TWO different true origins"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_combines_both_optional_filters() {
        let pool = test_pool().await;
        let date = fixture_date(26);
        seed_calling_point(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            Some("PAD"),
            Some("WAT"),
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C40001");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_published_day_with_no_matching_filters_is_some_and_empty() {
        // Pins the deliberate 404-vs-empty-200 split this function's own doc
        // comment documents: the day IS published, but the filters simply
        // matched nothing, so the result must be `Some(page)` with an empty
        // `departures`, never `None`. Ported coverage for the branch the
        // deleted destination-first equivalent
        // (`search_with_the_day_published_but_no_matching_rows_is_some_and_empty`)
        // used to pin.
        let pool = test_pool().await;
        let date = fixture_date(30);
        seed_calling_point(&pool, date).await;

        // No fixture row has this destination.
        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            Some("ZZZ"),
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day IS published");

        assert!(
            page.departures.is_empty(),
            "a published-but-unmatched day is Some(empty), never None"
        );
        assert!(page.next_cursor.is_none());

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_is_scoped_to_the_requested_service_date_only() {
        // Ported from the deleted search_is_scoped_to_the_requested_service_date_only
        // (destination-first predecessor) -- this coverage must not be lost
        // just because the old test block was deleted wholesale. Proves the
        // "always today, server-side" scoping: a stale day's rows must
        // never leak through, and must not even make the existence probe
        // say "published".
        let pool = test_pool().await;
        let date = fixture_date(29);
        let yesterday = date - chrono::Duration::days(1);
        delete_day(&pool, date).await;
        delete_day(&pool, yesterday).await;

        upsert_schedule_destination_departures(
            &pool,
            &[row(
                yesterday,
                "WAT",
                time(8, 0),
                "STALE",
                "RDG",
                None,
                None,
            )],
        )
        .await
        .expect("seed a stale day");

        let result = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");
        assert!(
            result.is_none(),
            "yesterday's rows must not answer today's query, nor satisfy today's existence probe"
        );

        delete_day(&pool, yesterday).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_filters_by_an_inclusive_time_range_and_excludes_before_now() {
        let pool = test_pool().await;
        let date = fixture_date(27);
        seed_calling_point(&pool, date).await;

        // Lower bound 10:05 is inclusive and matches exactly; upper bound
        // 12:00 excludes the 18:40 row.
        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            time(10, 5),
            None,
            None,
            Some(time(12, 0)),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C40002");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_filters_by_destination_arrival_independent_of_the_station_time_range()
     {
        let pool = test_pool().await;
        let date = fixture_date_feb(1);
        delete_day(&pool, date).await;
        // Two trains both callable at RDG within the SAME scheduled
        // (station) time window, but arriving at WAT 40 minutes apart --
        // so only the destination_arrival bound, not scheduled/to_time,
        // can tell them apart.
        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "WAT", time(8, 0), "C80001", "RDG", None, Some(time(8, 40))),
                row(date, "WAT", time(8, 5), "C80002", "RDG", None, Some(time(9, 20))),
            ],
        )
        .await
        .expect("seed fixture");

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            Some("WAT"),
            None,
            Some(time(9, 0)),
            Some(time(9, 30)),
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C80002");
        assert_eq!(page.departures[0]["destination_arrival"], "09:20:00");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_with_no_destination_arrival_bounds_ignores_a_null_destination_arrival()
     {
        let pool = test_pool().await;
        let date = fixture_date_feb(2);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[row(date, "WAT", time(8, 0), "C80003", "RDG", None, None)],
        )
        .await
        .expect("seed fixture");

        let page = search_schedule_calling_point_departures(
            &pool, "RDG", date, any_time(), None, None, None, None, None, None, 100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(
            page.departures.len(),
            1,
            "no destination_from/to means the NULL row is still returned"
        );
        assert!(page.departures[0]["destination_arrival"].is_null());

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_keyset_cursor_pages_without_gaps_or_repeats_and_breaks_ties_on_train_uid()
     {
        // Two rows share one `scheduled` (09:00) at the SAME station, to
        // prove train_uid alone is a sufficient tiebreaker now that
        // origin_crs is fixed per query, not part of the ordering.
        let pool = test_pool().await;
        let date = fixture_date(28);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "WAT", time(9, 0), "C60002", "RDG", None, None),
                row(date, "WAT", time(9, 0), "C60001", "RDG", None, None),
                row(date, "BRI", time(11, 0), "C60003", "RDG", None, None),
            ],
        )
        .await
        .expect("seed tied-time fixture");

        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<CallingPointDepartureCursor> = None;
        for _ in 0..5 {
            let page = search_schedule_calling_point_departures(
                &pool,
                "RDG",
                date,
                any_time(),
                None,
                None,
                None,
                None,
                None,
                cursor.as_ref(),
                1,
            )
            .await
            .expect("search")
            .expect("the day is published");
            for departure in &page.departures {
                seen.push(departure["uid"].as_str().unwrap().to_string());
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        assert_eq!(
            seen,
            vec!["C60001", "C60002", "C60003"],
            "the 09:00 tie is broken by train_uid, and every row appears exactly once"
        );
        assert!(
            cursor.is_none(),
            "the last page must not hand back a cursor"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_calling_point_departures_for_train_returns_rows_ordered_by_scheduled_time \
                -- --ignored --test-threads=1`"]
    async fn list_calling_point_departures_for_train_returns_rows_ordered_by_scheduled_time() {
        let pool = test_pool().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JS-CPD'")
            .execute(&pool)
            .await
            .ok();

        upsert_schedule_destination_departures(
            &pool,
            &[
                ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "10:15:00".parse().unwrap(),
                    train_uid: "TEST-JS-CPD".to_string(),
                    origin_crs: "RDG".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                },
                ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "10:32:00".parse().unwrap(),
                    train_uid: "TEST-JS-CPD".to_string(),
                    origin_crs: "SLO".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                },
            ],
        )
        .await
        .expect("seed schedule_destination_departures");

        let rows = list_calling_point_departures_for_train(&pool, "TEST-JS-CPD", service_date)
            .await
            .expect("list_calling_point_departures_for_train");

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].origin_crs, "RDG");
        assert_eq!(rows[0].true_origin_crs.as_deref(), Some("RDG"));
        assert_eq!(rows[0].destination_crs.as_deref(), Some("WAT"));
        assert_eq!(rows[1].origin_crs, "SLO");

        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-JS-CPD'")
            .execute(&pool)
            .await
            .ok();
    }
}

#[cfg(test)]
mod journey_timetable_overlay_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                latest_movement_event_per_location_dedups_to_the_most_recently_received_event \
                -- --ignored --test-threads=1`"]
    async fn latest_movement_event_per_location_dedups_to_the_most_recently_received_event() {
        let pool = test_pool().await;
        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-JS-MOVE",
            "2026-09-08".parse().unwrap(),
        )
        .await
        .expect("find_or_create_train");

        sqlx::query(
            "INSERT INTO train_movement_events \
                (trains_id, dedup_key, msg_type, event_type, loc_crs, planned_timestamp, \
                 actual_timestamp, variation_status, raw_body, received_at) \
             VALUES \
                ($1, 'k1', '0003', 'ARRIVAL', 'rdg', '2026-09-08T09:15:00Z', '2026-09-08T09:17:00Z', \
                 'LATE', '{}'::jsonb, NOW() - interval '2 minutes'), \
                ($1, 'k2', '0003', 'DEPARTURE', 'RDG', '2026-09-08T09:20:00Z', '2026-09-08T09:23:00Z', \
                 'LATE', '{}'::jsonb, NOW())",
        )
        .bind(trains_id)
        .execute(&pool)
        .await
        .expect("seed train_movement_events");

        let rows = latest_movement_event_per_location(&pool, trains_id)
            .await
            .expect("latest_movement_event_per_location");

        assert_eq!(rows.len(), 1, "one location, dedup to its latest event");
        assert_eq!(rows[0].loc_crs, "RDG");
        assert_eq!(rows[0].event_type.as_deref(), Some("DEPARTURE"));

        sqlx::query("DELETE FROM train_movement_events WHERE trains_id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                station_names_for_crs_batch_resolves_known_codes_and_omits_unknown_ones \
                -- --ignored --test-threads=1`"]
    async fn station_names_for_crs_batch_resolves_known_codes_and_omits_unknown_ones() {
        let pool = test_pool().await;

        sqlx::query("INSERT INTO stations (crs, name) VALUES ('JTO', 'TEST STATION') ON CONFLICT (crs) DO NOTHING")
            .execute(&pool)
            .await
            .expect("seed test station");

        let names = station_names_for_crs_batch(&pool, &["jto".to_string(), "ZZZ".to_string()])
            .await
            .expect("station_names_for_crs_batch");

        assert!(names.contains_key("JTO"), "JTO test station should be found");
        assert!(!names.contains_key("ZZZ"));

        sqlx::query("DELETE FROM stations WHERE crs = 'JTO'")
            .execute(&pool)
            .await
            .ok();
    }
}
