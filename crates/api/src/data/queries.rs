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
use common::{IncidentMessage, LineStatusReport, StationReference, StationSample, TocReference};
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
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");
        const LINE_ID: &str = "TEST-HALF-HOURLY-RANGE";

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1").bind(LINE_ID).execute(&pool).await.unwrap();

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
            &pool, LINE_ID,
            "2026-08-31T00:00:00Z".parse().unwrap(),
            "2026-09-01T00:00:00Z".parse().unwrap(),
        ).await.expect("half_hourly_stats_for_range");

        sqlx::query("DELETE FROM line_status_half_hourly_stats WHERE line_id = $1").bind(LINE_ID).execute(&pool).await.unwrap();

        assert_eq!(rows.len(), 2, "the out-of-range row must be excluded");
        assert_eq!(rows[0].half_hour_start, h1, "results must be ordered ascending by half_hour_start");
        assert_eq!(rows[1].half_hour_start, h2);

        let unknown = half_hourly_stats_for_range(
            &pool, "TEST-HALF-HOURLY-RANGE-UNKNOWN",
            "2026-08-31T00:00:00Z".parse().unwrap(),
            "2026-09-01T00:00:00Z".parse().unwrap(),
        ).await.expect("half_hourly_stats_for_range for an unknown line_id");
        assert!(unknown.is_empty());
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
