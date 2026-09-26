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
///
/// `line_matcher` is run over each incoming incident to fill
/// `incidents.affected_lines` -- see that column's migration
/// (`20260917090000_incidents_affected_lines.sql`) and `common::matcher`'s
/// module doc. It is a pure function of the incident's own text + operator
/// list against the line catalogue, so recomputing it on every poll cycle
/// is both cheap and the mechanism by which a catalogue edit (a new
/// `match_keywords` entry, say) reaches still-live incidents: they are
/// re-sent every cycle. Incidents that have dropped out of the feed keep
/// whatever was computed when they were last seen, which is why the
/// backfill binary exists.
pub async fn upsert_incidents(
    pool: &PgPool,
    redis: &redis::Client,
    line_matcher: &common::matcher::LineMatcher,
    incidents: &[IncidentMessage],
) -> Result<u64> {
    let mut count = 0u64;
    let mut text_changed_ids = Vec::new();

    // Matched up front, outside every transaction. This function's whole
    // chunking scheme exists to bound how long a transaction holds row
    // locks (see the doc comment above), so pure CPU work that needs no
    // database at all has no business running inside one -- even work this
    // cheap (a substring scan per catalogue line).
    let affected_lines: Vec<Vec<String>> = incidents
        .iter()
        .map(|incident| line_matcher.affected_line_ids(incident))
        .collect();

    for (chunk_index, chunk) in incidents.chunks(UPSERT_CHUNK_SIZE).enumerate() {
        let chunk_offset = chunk_index * UPSERT_CHUNK_SIZE;
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

        for (offset_in_chunk, incident) in chunk.iter().enumerate() {
            let affected_lines = &affected_lines[chunk_offset + offset_in_chunk];
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
                    first_seen_at, affected_lines
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW(), $10)
                ON CONFLICT (incident_id) DO UPDATE SET
                    summary           = EXCLUDED.summary,
                    description       = EXCLUDED.description,
                    operators         = EXCLUDED.operators,
                    affected_stations = EXCLUDED.affected_stations,
                    priority          = EXCLUDED.priority,
                    validity_periods  = EXCLUDED.validity_periods,
                    is_planned        = EXCLUDED.is_planned,
                    is_cleared        = EXCLUDED.is_cleared,
                    fetched_at        = NOW(),
                    affected_lines    = EXCLUDED.affected_lines
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
            .bind(affected_lines)
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
///
/// **Ownership guard.** `line_status.line_id` is only `TEXT PRIMARY KEY` --
/// nothing in the schema stops a TfL line id (`crates/poller-tfl`'s own
/// naming, e.g. `"victoria"`) from colliding with an `aggregator`-owned
/// line id (derived from `lines/*.toml` file stems). The two writers'
/// naming schemes staying disjoint is a CONVENTION, not an enforced
/// constraint (see `20260822120000_line_status_source.sql`'s own migration
/// comment, which introduced `source` for exactly this reason but as a
/// plain unindexed column, not part of the key). Before this fix, a
/// collision was invisible: `ON CONFLICT (line_id) DO UPDATE SET ...
/// source = 'tfl'` would silently steal an `aggregator`-owned row -- and
/// the "is this line changed" read just above only checked
/// `source = 'tfl'` rows, so a colliding `aggregator` row read back as "no
/// existing TfL row" (`existing = None`) rather than "an existing row I
/// must not touch", making the theft look like an ordinary first-write.
/// Now: (1) the pre-write read checks the row's actual owner regardless of
/// source, and bails loudly (`anyhow::bail!`, aborting the whole batch's
/// transaction) the moment it finds a same-`line_id` row owned by anyone
/// other than `'tfl'`; (2) the `ON CONFLICT DO UPDATE` itself carries a
/// `WHERE line_status.source = 'tfl'` guard so a same-`line_id` row
/// created by a concurrent `aggregator` write, in the window between that
/// read and this statement, is refused rather than overwritten; and (3) a
/// resulting zero-rows-affected write (only reachable via that race, since
/// the pre-write read already ruled out the non-racy case) is itself
/// treated as the same loud failure, not silently ignored.
pub async fn upsert_tfl_line_status(pool: &PgPool, reports: &[LineStatusReport]) -> Result<u64> {
    if reports.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for report in reports {
        let statuses_json = serde_json::to_value(&report.statuses)?;

        let existing_owner: Option<(String, Option<serde_json::Value>)> = sqlx::query_as(
            "SELECT source, CASE WHEN source = 'tfl' THEN statuses ELSE NULL END \
             FROM line_status WHERE line_id = $1",
        )
        .bind(&report.id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some((owner, _)) = &existing_owner
            && owner != "tfl"
        {
            anyhow::bail!(
                "refusing to upsert TfL line status for line_id {:?}: that line_id is \
                 already owned by source {:?}, not 'tfl' -- this is a naming collision \
                 between two independent line-id schemes (see upsert_tfl_line_status's \
                 doc comment), not a legitimate TfL update",
                report.id,
                owner
            );
        }
        let existing = existing_owner.and_then(|(_, statuses)| statuses);

        let write_result = sqlx::query(
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
            WHERE line_status.source = 'tfl'
            "#,
        )
        .bind(&report.id)
        .bind(&report.name)
        .bind(&report.mode_name)
        .bind(&report.operators)
        .bind(&statuses_json)
        .execute(&mut *tx)
        .await?;

        if write_result.rows_affected() == 0 {
            anyhow::bail!(
                "refusing to upsert TfL line status for line_id {:?}: the write affected no \
                 rows, which only happens when a same-line_id row owned by a different source \
                 was created concurrently after this function's own ownership check -- \
                 aborting rather than silently no-op'ing what should have been an insert or \
                 update",
                report.id
            );
        }

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

/// The delivery directory name (`YYYYMMDDTHHMMSSZ`) whose `schedule-reference`
/// publish cycle most recently COMPLETED -- every product for that delivery
/// published successfully -- or `None` if that producer has never completed
/// a full cycle (a fresh deployment).
///
/// Deliberately NOT [`last_schedule_feed_fetch`] above, and the difference is
/// the whole point of this table existing: that one reports when a delivery
/// was EXTRACTED by `schedule-ingest`, which says nothing about whether
/// `schedule-reference` ever processed it. Backs `GET
/// /private/schedule-reference-publishes`, which
/// `schedule-reference::main::seed_last_processed_delivery` reads once at
/// startup. See `20260925130000_schedule_reference_publishes.sql` for the
/// production failure mode this replaced.
///
/// `ORDER BY completed_at DESC`, not `MAX(delivery)`: the delivery name
/// happens to sort chronologically today, but ordering on the column that
/// actually means "when did this finish" cannot be broken by a future change
/// to the directory-name format.
pub async fn last_completed_schedule_reference_publish(pool: &PgPool) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT delivery FROM schedule_reference_publishes ORDER BY completed_at DESC LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(delivery,)| delivery))
}

/// Records that `schedule-reference` has completed a FULL successful publish
/// cycle for `delivery` -- every product derived from that delivery landed.
///
/// `ON CONFLICT (delivery) DO UPDATE SET completed_at = now()`, not `DO
/// NOTHING`: a re-POST for the same delivery means that delivery's whole
/// cycle really did run to completion again (the marker was lost, or a
/// previous cycle left it unset because a product had failed and the retry
/// has now succeeded), and `completed_at` should reflect the latest such
/// completion so [`last_completed_schedule_reference_publish`]'s ordering
/// stays honest.
pub async fn insert_schedule_reference_publish(pool: &PgPool, delivery: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO schedule_reference_publishes (delivery, completed_at) VALUES ($1, NOW()) \
         ON CONFLICT (delivery) DO UPDATE SET completed_at = NOW()",
    )
    .bind(delivery)
    .execute(pool)
    .await?;
    Ok(())
}

/// Upserts a batch of resolved STANOX/CRS rows. Every daily delivery is a
/// full refresh (see this table's migration comment), so this is always a
/// complete-table upsert-by-`stanox`.
///
/// **This alone does NOT remove a stale row (2026-09-25 correction).** An
/// earlier version of this doc comment claimed "no separate delete step is
/// needed, since every successful run re-asserts every row it still
/// resolves" -- true only for a STANOX still present in the source data. A
/// STANOX a later delivery simply stops mentioning at all (decommissioned,
/// renamed, a source correction) was never removed by this function alone,
/// so a stale STANOX->CRS mapping accumulated in this table forever,
/// silently diverging from the real network. See [`prune_stanox_crs_not_in`]
/// for the cleanup half of a real publish cycle, and its own doc comment
/// for why that is a SEPARATE function rather than folded into this one.
pub async fn upsert_stanox_crs(pool: &PgPool, records: &[common::StanoxCrsRecord]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for record in records {
        sqlx::query(
            r#"
            INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (stanox) DO UPDATE SET
                crs                 = EXCLUDED.crs,
                tiploc              = EXCLUDED.tiploc,
                station_name        = EXCLUDED.station_name,
                source_sequence     = EXCLUDED.source_sequence,
                change_time_minutes = EXCLUDED.change_time_minutes,
                updated_at          = NOW()
            "#,
        )
        .bind(&record.stanox)
        .bind(&record.crs)
        .bind(&record.tiploc)
        .bind(&record.station_name)
        .bind(record.source_sequence)
        .bind(record.change_time_minutes)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Deletes every `stanox_crs` row whose `stanox` is absent from
/// `keep_stanoxes` -- the stale-row cleanup half of a real publish cycle
/// that [`upsert_stanox_crs`] alone never covered (Signal Box Audit Low
/// finding, 2026-09-25; see that function's own corrected doc comment).
/// `routes::ingest::post_stanox_crs` calls this immediately after
/// `upsert_stanox_crs`, in the same HTTP request, passing that SAME
/// delivery's full STANOX set -- one publish cycle, two SQL statements.
///
/// **Deliberately its own function, not folded into `upsert_stanox_crs`
/// itself.** `upsert_stanox_crs` is reused throughout this crate's own test
/// suite (`journey.rs`, `train.rs`, this file's own other test modules) purely
/// to seed a couple of synthetic STANOX/CRS rows for an unrelated feature
/// test -- baking an unconditional "delete every OTHER row in the table"
/// into it would turn every one of those call sites into a table-wide wipe
/// of whatever fixture rows a DIFFERENT test already seeded, the exact
/// "an empty/small batch wipes real data" hazard `upsert_fixed_links`'s own
/// empty-batch guard exists to prevent, just triggered by a SMALL batch
/// instead of an EMPTY one. Keeping the prune a separate, explicitly-called
/// step confines it to the one real caller that actually represents a whole
/// delivery: the ingest route.
///
/// `keep_stanoxes.is_empty()` is a no-op, not "delete everything" -- matches
/// this crate's established "an empty batch must never be read as delete
/// everything" posture ([`upsert_fixed_links`],
/// [`upsert_schedule_destination_departures_chunk`]).
pub async fn prune_stanox_crs_not_in(pool: &PgPool, keep_stanoxes: &[String]) -> Result<u64> {
    if keep_stanoxes.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query("DELETE FROM stanox_crs WHERE NOT (stanox = ANY($1))")
        .bind(keep_stanoxes)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
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
    change_time_minutes: Option<i32>,
}

impl From<StanoxCrsRow> for common::StanoxCrsRecord {
    fn from(row: StanoxCrsRow) -> Self {
        common::StanoxCrsRecord {
            stanox: row.stanox,
            crs: row.crs,
            tiploc: row.tiploc,
            station_name: row.station_name,
            source_sequence: row.source_sequence,
            change_time_minutes: row.change_time_minutes,
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
        "SELECT stanox, crs, tiploc, station_name, source_sequence, change_time_minutes FROM stanox_crs ORDER BY stanox",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(common::StanoxCrsRecord::from)
        .collect())
}

/// Every TIPLOC->CRS row for one CRS -- the "which TIPLOCs does this
/// station's code cover" lookup Decision 3 step 3 of
/// docs/superpowers/specs/2026-09-05-schedule-first-train-tracking-design.md
/// calls for (`list_stanox_crs`'s existing `WHERE`-less shape returns
/// everything; this is its `WHERE crs = $1` sibling). `UPPER(TRIM(...))`
/// on both sides, matching `TRACKED_TRAIN_STATE_SELECT`'s own established
/// convention -- `tracked_trains.pin_origin_crs` is never
/// case-normalized at write time (`validate_pin` doesn't uppercase it),
/// so a case-insensitive compare here is load-bearing, not defensive
/// tidiness.
///
/// This file's convention for every TIPLOC/CRS equality lookup is
/// `UPPER(TRIM(column)) = UPPER(TRIM(parameter))`: a Signal Box Audit Low
/// finding found the lookups in this file disagreeing -- some raw, some
/// `UPPER`-only, one pair (`crs_for_tiploc`/`crs_for_tiplocs_batch`)
/// already `UPPER`+`TRIM` -- so two functions that both claimed to
/// resolve "the same" code could silently return different answers for a
/// lowercase or whitespace-padded input depending on which one a caller
/// happened to call. `UPPER(TRIM(...))` is now applied uniformly across
/// every such lookup in this file (see `latest_station_sample`,
/// `latest_station_full_coverage_samples`,
/// `latest_schedule_network_departures`, `list_fixed_links_from_crs`,
/// `station_names_for_crs_batch` for the others), and
/// `queries_crs_tiploc_normalization_tests` below has a regression test
/// proving two of them now agree on the same lowercase/padded input.
///
/// As of docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md
/// (Task 3), this reads the UNION of `tiploc_crs` and `stanox_crs`,
/// preferring a `tiploc_crs` row when the SAME TIPLOC appears in both
/// (`DISTINCT ON (tiploc) ... ORDER BY tiploc, priority`, `tiploc_crs` at
/// priority 1). Every TIPLOC this function used to return from
/// `stanox_crs` alone still comes back unchanged (no existing caller or
/// fixture loses anything); a TIPLOC that exists ONLY in `tiploc_crs` --
/// e.g. Vauxhall's `VAUXHLM`/`VAUXHLW` or Clapham Junction's
/// `CLPHMJM`/`CLPHMJW`, both previously collapsed to one row per STANOX
/// by `stanox_crs`'s `PRIMARY KEY (stanox)` -- now ALSO comes back, which
/// it could not before this plan (see `journey.rs`'s `tiploc_key` doc
/// comment). The return type stays `Vec<common::StanoxCrsRecord>`: this is
/// a generic "which rows cover this CRS" lookup, and `StanoxCrsRecord`'s
/// shape already has every field a `tiploc_crs` row also has.
pub async fn list_stanox_crs_for_crs(
    pool: &PgPool,
    crs: &str,
) -> Result<Vec<common::StanoxCrsRecord>> {
    let rows = sqlx::query_as::<_, StanoxCrsRow>(
        "SELECT DISTINCT ON (tiploc) tiploc, crs, station_name, stanox, source_sequence, change_time_minutes \
         FROM ( \
             SELECT tiploc, crs, station_name, stanox, source_sequence, change_time_minutes, 1 AS priority \
             FROM tiploc_crs WHERE UPPER(TRIM(crs)) = UPPER(TRIM($1)) \
             UNION ALL \
             SELECT tiploc, crs, station_name, stanox, source_sequence, change_time_minutes, 2 AS priority \
             FROM stanox_crs WHERE UPPER(TRIM(crs)) = UPPER(TRIM($1)) \
         ) merged \
         ORDER BY tiploc, priority",
    )
    .bind(crs)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(common::StanoxCrsRecord::from)
        .collect())
}

/// Upserts a batch of directly-resolved TIPLOC->CRS rows into the
/// TIPLOC-primary `tiploc_crs` table -- the sibling of `upsert_stanox_crs`
/// above, modeled on it directly (same transaction-per-batch shape, same
/// `ON CONFLICT (tiploc) DO UPDATE SET ...` pattern for every column
/// except `tiploc`/`updated_at`). Every daily delivery is a full refresh,
/// same as `stanox_crs` (see this table's migration comment), so this is
/// always a complete-table upsert-by-`tiploc`. See `common::TiplocCrsRecord`'s
/// own doc comment for why this table keeps EVERY TIPLOC as its own row
/// rather than `stanox_crs`'s one-row-per-STANOX disambiguation.
///
/// **This alone does NOT remove a stale row**, the identical gap
/// `upsert_stanox_crs`'s own doc comment corrects (2026-09-25) -- a TIPLOC a
/// later delivery stops mentioning was never removed. See
/// [`prune_tiploc_crs_not_in`], the direct sibling of
/// [`prune_stanox_crs_not_in`], for the cleanup half.
pub async fn upsert_tiploc_crs(pool: &PgPool, records: &[common::TiplocCrsRecord]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for record in records {
        sqlx::query(
            r#"
            INSERT INTO tiploc_crs (tiploc, crs, station_name, stanox, source_sequence, change_time_minutes, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (tiploc) DO UPDATE SET
                crs                  = EXCLUDED.crs,
                station_name         = EXCLUDED.station_name,
                stanox               = EXCLUDED.stanox,
                source_sequence      = EXCLUDED.source_sequence,
                change_time_minutes  = EXCLUDED.change_time_minutes,
                updated_at           = NOW()
            "#,
        )
        .bind(&record.tiploc)
        .bind(&record.crs)
        .bind(&record.station_name)
        .bind(&record.stanox)
        .bind(record.source_sequence)
        .bind(record.change_time_minutes)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Deletes every `tiploc_crs` row whose `tiploc` is absent from
/// `keep_tiplocs` -- the direct sibling of [`prune_stanox_crs_not_in`],
/// same reasoning, same "own function, not folded into the upsert" rationale
/// (see that function's own doc comment), same "empty batch is a no-op"
/// guard. `routes::ingest::post_tiploc_crs` calls this immediately after
/// `upsert_tiploc_crs`, in the same HTTP request.
pub async fn prune_tiploc_crs_not_in(pool: &PgPool, keep_tiplocs: &[String]) -> Result<u64> {
    if keep_tiplocs.is_empty() {
        return Ok(0);
    }
    let result = sqlx::query("DELETE FROM tiploc_crs WHERE NOT (tiploc = ANY($1))")
        .bind(keep_tiplocs)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}

/// Row shape for `list_tiploc_crs`'s `SELECT` -- a dedicated `FromRow`
/// struct, mirroring `StanoxCrsRow`'s own convention directly above.
#[derive(Debug, Clone, sqlx::FromRow)]
struct TiplocCrsRow {
    tiploc: String,
    crs: String,
    station_name: String,
    stanox: String,
    source_sequence: i32,
    change_time_minutes: Option<i32>,
}

impl From<TiplocCrsRow> for common::TiplocCrsRecord {
    fn from(row: TiplocCrsRow) -> Self {
        common::TiplocCrsRecord {
            tiploc: row.tiploc,
            crs: row.crs,
            station_name: row.station_name,
            stanox: row.stanox,
            source_sequence: row.source_sequence,
            change_time_minutes: row.change_time_minutes,
        }
    }
}

/// The full current `tiploc_crs` table, ordered by `tiploc` for a stable,
/// reviewable response shape -- mirrors `list_stanox_crs`'s own shape.
/// Task 3's `trip_planning.rs` reads this to build its TIPLOC->CRS
/// resolution alongside `stanox_crs`.
pub async fn list_tiploc_crs(pool: &PgPool) -> Result<Vec<common::TiplocCrsRecord>> {
    let rows = sqlx::query_as::<_, TiplocCrsRow>(
        "SELECT tiploc, crs, station_name, stanox, source_sequence, change_time_minutes FROM tiploc_crs ORDER BY tiploc",
    )
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(common::TiplocCrsRecord::from)
        .collect())
}

/// Wholesale-replaces `fixed_links` with `records` in one transaction --
/// see this plan's Judgment Call 4 for why this is a full replace, not a
/// per-row `ON CONFLICT` upsert like `upsert_stanox_crs`: a real ALF row
/// has no natural stable per-row key. At ~4,222 real rows (confirmed
/// against the sibling `Distant-Signal-MCP` project's own measurement,
/// see this plan's header), this is cheap on every ~30-minute publish
/// cycle. Called only when `schedule-reference` actually found an ALF
/// file this cycle (`routes::ingest::post_fixed_links`'s own caller,
/// `main.rs`'s `publish_fixed_links`) -- an absent ALF file means this
/// function is simply never called that cycle, leaving the previous
/// cycle's rows in place rather than deleting them with nothing to
/// replace them (this plan's Judgment Call 1's "degrade this one product,
/// never wipe it for no reason" posture).
///
/// **An empty `records` is a no-op, and that is load-bearing (2026-09-25
/// fix).** Before this guard, this was the one wholesale-replace publish
/// function in this file WITHOUT it -- both `upsert_schedule_destination_departures_chunk`
/// and `upsert_schedule_calling_points_full_chunk` already reject an empty
/// batch before their own `DELETE`, for exactly this reason: a delete-then-insert
/// upsert can wipe a whole product on a client mistake or an upstream
/// parse failure that produces zero rows, in a way a per-row `ON CONFLICT`
/// loop never could. `schedule-reference` itself only calls this when it
/// found real ALF rows to publish, so an empty batch reaching here means
/// something upstream sent (or the caller constructed) a batch it should
/// not have -- exactly the same client-mistake shape the two sibling
/// functions' own doc comments describe, not a legitimate "this cycle's
/// ALF file was empty" signal (that case is `main.rs`'s `publish_fixed_links`
/// never calling this function at all, per the paragraph above).
pub async fn upsert_fixed_links(pool: &PgPool, records: &[common::FixedLinkRecord]) -> Result<u64> {
    if records.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM fixed_links")
        .execute(&mut *tx)
        .await?;

    let mut count = 0u64;
    for record in records {
        sqlx::query(
            r#"
            INSERT INTO fixed_links (mode, from_crs, to_crs, minutes, valid_from, valid_to, days_mask, source_sequence, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            "#,
        )
        .bind(&record.mode)
        .bind(&record.from_crs)
        .bind(&record.to_crs)
        .bind(record.minutes)
        .bind(&record.valid_from)
        .bind(&record.valid_to)
        .bind(&record.days_mask)
        .bind(record.source_sequence)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Every `fixed_links` row whose `from_crs` matches `crs` -- Phase 2's own
/// read-side lookup shape (mirrors `list_stanox_crs_for_crs`'s own
/// `WHERE crs = $1` pattern). Case-insensitive and trim-insensitive,
/// matching this file's single `UPPER(TRIM(...))` normalization
/// convention for every TIPLOC/CRS lookup (see `list_stanox_crs_for_crs`'s
/// doc comment for the regression this convention closes).
pub async fn list_fixed_links_from_crs(
    pool: &PgPool,
    crs: &str,
) -> Result<Vec<common::FixedLinkRecord>> {
    let rows = sqlx::query_as::<_, FixedLinkRow>(
        "SELECT mode, from_crs, to_crs, minutes, valid_from, valid_to, days_mask, source_sequence \
         FROM fixed_links WHERE UPPER(TRIM(from_crs)) = UPPER(TRIM($1))",
    )
    .bind(crs)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(common::FixedLinkRecord::from)
        .collect())
}

#[derive(Debug, sqlx::FromRow)]
struct FixedLinkRow {
    mode: String,
    from_crs: String,
    to_crs: String,
    minutes: i32,
    valid_from: String,
    valid_to: String,
    days_mask: String,
    source_sequence: i32,
}

impl From<FixedLinkRow> for common::FixedLinkRecord {
    fn from(row: FixedLinkRow) -> Self {
        Self {
            mode: row.mode,
            from_crs: row.from_crs,
            to_crs: row.to_crs,
            minutes: row.minutes,
            valid_from: row.valid_from,
            valid_to: row.valid_to,
            days_mask: row.days_mask,
            source_sequence: row.source_sequence,
        }
    }
}

/// Reverse of the above: one CRS for a TIPLOC, or `None` if unmapped.
/// Used to resolve a matched schedule's own terminus CRS
/// (`schedule_destination_crs`) from its last calling point's TIPLOC.
/// `LIMIT 1`: a TIPLOC maps to at most one real station in practice, but
/// this doesn't assume uniqueness at the SQL level (no `UNIQUE`
/// constraint on `stanox_crs.tiploc` -- multiple STANOX rows can share a
/// TIPLOC, e.g. different platforms/areas of one physical location), so
/// this is "a plausible one," not "the guaranteed only one."
///
/// Both sides of the comparison are `TRIM`med as well as case-folded. A CIF
/// schedule-body TIPLOC is a fixed 7-character, space-padded field (see
/// `schedule_query::normalize_tiploc`) while `stanox_crs.tiploc` holds the
/// trimmed form, so a caller that forgets to normalize otherwise gets a
/// silent miss for every TIPLOC shorter than 7 characters -- roughly a
/// third of all real station TIPLOCs, and the cause of the 2026-09-16
/// "Unknown location" journey-page bug (see `journey::tiploc_key`).
/// Callers should still normalize, and all of them do, but correctness
/// must not depend on their remembering to.
///
/// The input is trimmed in Rust rather than in SQL, matching
/// `crs_for_tiplocs_batch`'s own `t.trim()` -- deliberately, so the two
/// siblings cannot diverge *on trimming*: Rust's `str::trim` strips all
/// Unicode whitespace while Postgres `TRIM()` strips spaces only, which is
/// indistinguishable for real space-padded ASCII CIF data but would make
/// the pair disagree on anything exotic. (Case folding is still done
/// SQL-side here and Rust-side in the batch; both are ASCII-identical for
/// a TIPLOC, so that asymmetry is cosmetic rather than a second trap.)
///
/// As of docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md
/// (Task 3), this reads the UNION of `tiploc_crs` and `stanox_crs`,
/// deterministically preferring a `tiploc_crs` row when the SAME TIPLOC
/// exists in both tables with DIFFERENT CRS values (a real possible
/// transient state across delivery cycles) -- same `priority` pattern
/// `list_stanox_crs_for_crs` established (`tiploc_crs` at priority 1,
/// `stanox_crs` at priority 2, `ORDER BY priority` before `LIMIT 1`). A
/// TIPLOC present in `stanox_crs` but not yet in `tiploc_crs` still
/// resolves exactly as before; a TIPLOC present ONLY in `tiploc_crs` --
/// e.g. Vauxhall's/Clapham Junction's previously-dropped sibling TIPLOC --
/// now ALSO resolves, which it could not before this plan.
///
/// **The returned `crs` is `UPPER()`-cased, matching [`crs_for_tiplocs_batch`]'s
/// own `UPPER(crs)` projection -- this function used to select the bare,
/// as-stored `crs` column instead.** Neither `upsert_stanox_crs` nor
/// `upsert_tiploc_crs` case-normalizes `crs` on write (it is stored exactly
/// as `schedule_reference::parser` decoded it from the raw CIF byte range,
/// and no `CHECK` constraint enforces uppercase in either migration), so
/// the two sibling functions could disagree about a single TIPLOC's CRS
/// casing depending on which one a caller happened to call -- the same
/// "two functions that both claim to resolve the same code silently
/// disagree" bug class the Signal Box Audit's `UPPER(TRIM(...))`-everywhere
/// pass already closed for every WHERE-clause comparison in this file (see
/// this module's own doc note on `list_stanox_crs_for_crs`), just on the
/// *output* side instead of the input side, and so missed by that pass.
/// This matters beyond cosmetics: `schedule_matching::find_schedule_match`
/// feeds this function's result straight into
/// `.filter(|crs| queries::is_bookable_crs(crs))`, and `is_bookable_crs`
/// checks `crs.starts_with('X')` -- a case-sensitive, uppercase-only
/// check. An un-normalized lowercase pseudo-CRS (e.g. `"xvr"` instead of
/// `"XVR"`) would silently pass that filter and render as if it were a
/// real, bookable station -- exactly the failure mode `is_bookable_crs`'s
/// own doc comment cites real production evidence for (`XVR`/`XHN`/`XOZ`/
/// `XOD`/`XOE`/`XWI`).
pub async fn crs_for_tiploc(pool: &PgPool, tiploc: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as(
        "SELECT UPPER(crs) FROM ( \
             SELECT tiploc, crs, 1 AS priority FROM tiploc_crs \
             UNION ALL \
             SELECT tiploc, crs, 2 AS priority FROM stanox_crs \
         ) merged \
         WHERE UPPER(TRIM(tiploc)) = UPPER($1) \
         ORDER BY priority \
         LIMIT 1",
    )
    .bind(tiploc.trim())
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(crs,)| crs))
}

/// Batched sibling of `crs_for_tiploc` -- one `WHERE UPPER(TRIM(tiploc)) =
/// ANY($1)` query resolving every distinct TIPLOC in a calling-point list,
/// instead of one query per TIPLOC. Mirrors the existing single/batch
/// pairing convention `trains::find_or_create_train`/
/// `find_or_create_trains_batch` already establishes. Keys are
/// `UPPER(TRIM(tiploc))` -- see `crs_for_tiploc`'s own doc comment for why
/// the `TRIM` is load-bearing rather than cosmetic, and
/// `journey::tiploc_key` for the matching Rust-side key a caller's `get`
/// has to build. A TIPLOC with no row in either table is simply absent
/// from the map (degrade, don't fabricate -- same posture `crs_for_tiploc`
/// already has for a single lookup).
///
/// As of docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md
/// (Task 3), this reads the UNION of `tiploc_crs` and `stanox_crs`,
/// deterministically preferring a `tiploc_crs` row when the SAME TIPLOC
/// exists in both tables with DIFFERENT CRS values -- see `crs_for_tiploc`
/// above for why that matters and the `priority` pattern both share.
/// `DISTINCT ON (UPPER(TRIM(tiploc)))` with `ORDER BY UPPER(TRIM(tiploc)),
/// priority` picks the `tiploc_crs` row (priority 1) first per TIPLOC
/// before `.collect()` builds the map, so the result no longer depends on
/// unspecified `UNION` row order.
pub async fn crs_for_tiplocs_batch(
    pool: &PgPool,
    tiplocs: &[String],
) -> Result<HashMap<String, String>> {
    if tiplocs.is_empty() {
        return Ok(HashMap::new());
    }
    let upper: Vec<String> = tiplocs.iter().map(|t| t.trim().to_uppercase()).collect();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT DISTINCT ON (UPPER(TRIM(tiploc))) UPPER(TRIM(tiploc)), UPPER(crs) FROM ( \
             SELECT tiploc, crs, 1 AS priority FROM tiploc_crs \
             UNION ALL \
             SELECT tiploc, crs, 2 AS priority FROM stanox_crs \
         ) merged \
         WHERE UPPER(TRIM(tiploc)) = ANY($1) \
         ORDER BY UPPER(TRIM(tiploc)), priority",
    )
    .bind(&upper)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Whether `crs` is a genuine, bookable National Rail station code rather
/// than one of Network Rail's own `X`-prefixed pseudo-codes for a
/// non-passenger location (a junction, siding, or depot that still needs a
/// STANOX->CRS entry for TRUST tracking purposes but was never sold a
/// ticket to) -- see `reference-data/stanox-crs.md`'s own "Extraction and
/// exclusion policy" section and `schedule_reference::parser::resolve`'s
/// doc comment, which both independently document this exact convention.
///
/// Lives here, in `queries` -- rather than staying private to
/// `data::journey` (where it was first added, for the single-train journey
/// timeline) or moving out to `crates/common` -- because every one of its
/// real call sites is a display-bound CRS resolution inside THIS crate, and
/// `queries` is already the one module all four of them import for their
/// own TIPLOC->CRS lookups (`crs_for_tiploc`/`crs_for_tiplocs_batch`,
/// directly above): [`crate::data::journey::stops_from_calling_points`]
/// (the journey timeline's per-calling-point CRS, the original call site),
/// [`crate::data::schedule_matching::find_schedule_match`]'s
/// `destination_crs` (flows into `trains.destination_crs`, rendered as
/// `train.destinationName ?? train.destinationCrs` on the single-train
/// page), `crate::routes::lines::get_line_trains`'s schedule-side
/// origin/destination resolution (`ScheduleRouteEndpoints`), and
/// `crate::render::schedule_departure_json`'s `destinationCrs` field. A
/// cross-crate `common` helper would be the wrong call: no crate outside
/// `api` resolves a CRS for DISPLAY this way today, and `schedule_query`'s
/// own STANOX-disambiguation policy (`resolve.rs`) deliberately still
/// ACCEPTS a sole X-prefixed candidate for a STANOX -- the opposite
/// question this function answers -- so sharing one helper across that
/// boundary would invite exactly the confusion this doc comment is
/// disambiguating.
///
/// **Real evidence this matters, not a hypothetical.** `schedule_query::
/// resolve`'s STANOX-disambiguation policy accepts an X-prefixed CRS as a
/// STANOX's row whenever it is the SOLE candidate for that STANOX (only
/// excluding a STANOX outright when 2+ non-X or 2+ X candidates tie) -- so
/// a plain junction with no real passenger identity can still come back
/// from [`crs_for_tiplocs_batch`]/[`crs_for_tiploc`] with a resolved,
/// non-`None` `crs`. Confirmed against live production data for train
/// `Y80908` on 2026-09-23 (Birmingham New Street to London Euston):
/// `HANSLPJ` (Hanslope Junction) resolved to CRS `XHN`, `PROOFHJ` (Proof
/// House Junction) to `XOZ`, `LEDBRNJ` (Ledburn Junction) to `XOD`,
/// `BONENDJ` (Bourne End Junction) to `XOE`, and `WLSDWLJ` (Willesden
/// Junction) to `XWI` -- none of them a real station, none of them present
/// in `stations`, yet before this filter each one still carried a
/// non-`None` `crs` that was enough to render as if it were a real, terse
/// station identity wherever a caller displayed it unfiltered.
///
/// **A second, independent case the 2026-09-24 `tiploc_crs` crosswalk
/// widened.** That change (Task 3/4 of
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md) made
/// [`crs_for_tiploc`]/[`crs_for_tiplocs_batch`] resolve roughly a dozen
/// TIPLOCs that previously returned `None` (no STANOX-level
/// disambiguation possible) via the new TIPLOC-keyed `tiploc_crs` table
/// instead -- e.g. `VICTRCR` (a common empty-coaching-stock terminus) now
/// resolves to the X-prefixed pseudo-CRS `XVR` rather than `None`. Any
/// call site that resolves a display-bound CRS without this filter would,
/// as of that change, newly start showing a tracked ECS working's
/// destination as "XVR" instead of correctly showing nothing -- the exact
/// same bug class the `HANSLPJ`/`XHN` case above already documents, just
/// reachable through a second, newly-widened path.
///
/// A stop/row that only resolves to an X-prefixed pseudo-CRS should be
/// treated exactly like one that didn't resolve at all: blank the `crs`
/// (or `destination_crs`) out to `None`/absent rather than passing the
/// pseudo-code through, same degrade every call site above already applies
/// consistently.
pub fn is_bookable_crs(crs: &str) -> bool {
    !crs.starts_with('X')
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

/// Every published line whose `service_date` population contains a schedule
/// with this `train_uid` -- the identity-first inverse of
/// [`get_schedule_line_population`], which can only answer "what is on THIS
/// line."
///
/// Exists for `schedule_matching::find_schedule_match`'s
/// known-identity fallback: a train whose ORIGIN station appears on no
/// `lines/*.toml` at all (a real shape -- an uncatalogued branch terminus)
/// has no candidate line to look up by CRS, even though
/// `schedule_query::schedules_touching` has already put its whole schedule,
/// origin calling point included, into the population of every line that
/// lists ANY station it calls at further down the route. Without this, such
/// a train's schedule was unreachable from a known `train_uid`.
///
/// One JSONB containment query, not 100-plus per-line reads: `population @>
/// '[{"uid": ...}]'` is true exactly when some array element of
/// `population` is an object carrying that `uid` (jsonb containment matches
/// objects partially), so Postgres does the "which line carries this uid"
/// scan itself. `ORDER BY line_id` so a caller iterating the result is
/// deterministic across calls, matching `crs_to_line_ids`' own
/// alphabetical-by-file candidate ordering.
///
/// Deliberately NOT indexed: this runs only on the fallback path above (a
/// known uid whose origin CRS is uncatalogued), the table holds one row per
/// line per date (a few hundred rows), and a GIN index on a
/// whole-day-of-schedules JSONB column would cost every
/// `schedule-reference` publish far more than it saves here.
pub async fn list_line_ids_with_uid_in_population(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
    train_uid: &str,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT line_id FROM schedule_line_population \
         WHERE service_date = $1 \
           AND population @> jsonb_build_array(jsonb_build_object('uid', $2::text)) \
         ORDER BY line_id",
    )
    .bind(service_date)
    .bind(train_uid)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(line_id,)| line_id).collect())
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
///
/// `UPPER(TRIM(crs)) = UPPER(TRIM($1))`, not a raw `=`: `crs` here comes straight off
/// the URL path (`Path<String>` in `routes::departures`, no normalization
/// applied), and this file's other CRS lookups --
/// `list_stanox_crs_for_crs`, `list_fixed_links_from_crs`,
/// `station_names_for_crs_batch` -- all case-fold before comparing. A raw
/// `=` here made this function the odd one out: a caller hitting
/// `/stations/kgx/schedule-departures` (lowercase) would silently 404
/// even though `/stations/KGX/schedule-departures` resolves, purely
/// because this one comparison never normalized case.
pub async fn latest_schedule_network_departures(
    pool: &PgPool,
    crs: &str,
    service_date: chrono::NaiveDate,
) -> Result<Option<serde_json::Value>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT departures FROM schedule_network_departures \
         WHERE UPPER(TRIM(crs)) = UPPER(TRIM($1)) AND service_date = $2",
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
    /// How many calendar days past `service_date` `scheduled` actually
    /// falls on -- mirrors `schedule_query::DestinationDeparture::day_offset`
    /// verbatim (this row is built directly from one, see
    /// `schedule-reference::schedule_destination_departures_rows`). See
    /// `schedule_query::CallingPoint::day_offset`'s own doc comment for the
    /// live-confirmed real overnight-service example this exists for.
    /// `#[serde(default)]` for the same rolling-deploy-safety reason
    /// `true_origin_crs`/`destination_arrival` tolerate a missing key.
    #[serde(default)]
    pub day_offset: i16,
    pub train_uid: String,
    pub origin_crs: String,
    pub true_origin_crs: Option<String>,
    /// THIS row's own calling point's `booked_arrival` -- NOT the
    /// schedule's true destination's arrival (`destination_arrival`
    /// below). Mirrors `schedule_query::DestinationDeparture::calling_point_arrival`'s
    /// own doc comment exactly: `None` for the schedule's true origin (an
    /// `Origin` calling point never has a `booked_arrival`), `Some` for a
    /// genuine `Intermediate` calling point. Backs `GET
    /// /public/trains/search?arrival_from=&arrival_to=`, which only
    /// applies when `stops_at` is set -- see
    /// docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md.
    pub calling_point_arrival: Option<chrono::NaiveTime>,
    /// The schedule's terminating calling point's own `booked_arrival`,
    /// mirroring `true_origin_crs`'s plumbing exactly -- see
    /// `schedule_query::DestinationDeparture::destination_arrival`'s own
    /// doc comment and
    /// docs/superpowers/specs/2026-09-08-destination-arrival-time-filter-design.md.
    /// Missing from the wire JSON deserializes as `None` (Option<T> fields
    /// are optional-by-default for self-describing formats like JSON),
    /// same as `true_origin_crs`.
    pub destination_arrival: Option<chrono::NaiveTime>,
    /// How many calendar days past `service_date` `destination_arrival`
    /// actually falls on -- mirrors
    /// `schedule_query::DestinationDeparture::destination_arrival_day_offset`
    /// verbatim (this row is built directly from one, see
    /// `schedule-reference::schedule_destination_departures_rows`). NOT the
    /// same value as this row's own `day_offset` above in general: on a
    /// genuine overnight schedule, the DEPARTING calling point this row
    /// represents and the schedule's TERMINATING calling point can fall on
    /// two different calendar days (see that struct's own doc comment for
    /// the live-confirmed c2c UID `F49687` example). `#[serde(default)]`
    /// for the same rolling-deploy-safety reason `day_offset` above tolerates
    /// a missing key -- a row published before this field existed still
    /// deserializes, as `0` ("assume same day as the departure").
    #[serde(default)]
    pub destination_arrival_day_offset: i16,
    /// The schedule's `BX` ATOC Code, mirroring
    /// `schedule_query::records::DestinationDeparture::operator_atoc`'s own
    /// doc comment exactly: computed once per schedule and copied
    /// unchanged onto every departure row it contributes, `None` when no
    /// `BX` line follows the schedule's `BS` line (or its ATOC Code field
    /// was blank/undecodable). `#[serde(default)]` for the same
    /// rolling-deploy-safety reason `day_offset`/`destination_arrival_day_offset`
    /// above tolerate a missing key -- a row published before this field
    /// existed still deserializes, as `None`.
    #[serde(default)]
    pub operator_atoc: Option<String>,
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
/// build-parallel-Vecs-then-bind style. Ten bind parameters regardless of
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
///
/// Always clears the touched dates first -- this is the "one call is the
/// whole day" entry point every existing caller (this file's own tests,
/// `crates/api/src/routes/journeys.rs`'s fixtures) uses. The ingest route
/// (`crates/api/src/routes/ingest.rs::post_schedule_destination_departures`)
/// instead calls [`upsert_schedule_destination_departures_chunk`], which lets
/// the caller say whether THIS call is the first one touching these dates in
/// the current publish cycle -- see that function's doc comment for why a
/// second variant exists rather than a parameter added here (it would force
/// every one of those unrelated call sites to pass a redundant `true`).
pub async fn upsert_schedule_destination_departures(
    pool: &PgPool,
    rows: &[ScheduleDestinationDeparturesRow],
) -> Result<u64> {
    upsert_schedule_destination_departures_chunk(pool, rows, true).await
}

/// Chunk-aware core of [`upsert_schedule_destination_departures`]. `rows` is
/// one call's worth of a publish cycle that may, in the future, split one
/// day's ~377,000 rows across multiple calls (`rows.chunks(50_000)`, see
/// `crates/schedule-reference/src/main.rs::publish_schedule_destination_departures`'s
/// own doc comment) -- `first_chunk` says whether THIS call is the first one
/// touching `rows`' dates in the current cycle.
///
/// `first_chunk = true` clears the touched dates first, identical to
/// `upsert_schedule_destination_departures` above (in fact that function is
/// just this one called with `true`). `first_chunk = false` skips the
/// DELETE and only inserts -- so a later chunk's call does not wipe out an
/// earlier chunk's just-inserted rows for the same date, which a per-call
/// unconditional DELETE would do the moment more than one chunk is ever sent
/// for the same date in one cycle. `crates/api/src/routes/ingest.rs`'s
/// `post_schedule_destination_departures` is the only caller that passes
/// `false` (via its `?first_chunk=` query parameter) -- today it always
/// passes `true`, since `schedule-reference` sends exactly one call per
/// date; the flag exists so that story stays correct the day chunking
/// actually gets turned on, without a further schema/contract change.
pub async fn upsert_schedule_destination_departures_chunk(
    pool: &PgPool,
    rows: &[ScheduleDestinationDeparturesRow],
    first_chunk: bool,
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }

    let service_dates: Vec<chrono::NaiveDate> = rows.iter().map(|r| r.service_date).collect();
    let destination_crs: Vec<&str> = rows.iter().map(|r| r.destination_crs.as_str()).collect();
    let scheduled: Vec<chrono::NaiveTime> = rows.iter().map(|r| r.scheduled).collect();
    let day_offsets: Vec<i16> = rows.iter().map(|r| r.day_offset).collect();
    let train_uids: Vec<&str> = rows.iter().map(|r| r.train_uid.as_str()).collect();
    let origin_crs: Vec<&str> = rows.iter().map(|r| r.origin_crs.as_str()).collect();
    let true_origin_crs: Vec<Option<&str>> =
        rows.iter().map(|r| r.true_origin_crs.as_deref()).collect();
    let calling_point_arrival: Vec<Option<chrono::NaiveTime>> =
        rows.iter().map(|r| r.calling_point_arrival).collect();
    let destination_arrival: Vec<Option<chrono::NaiveTime>> =
        rows.iter().map(|r| r.destination_arrival).collect();
    let destination_arrival_day_offsets: Vec<i16> = rows
        .iter()
        .map(|r| r.destination_arrival_day_offset)
        .collect();
    let operator_atoc: Vec<Option<&str>> =
        rows.iter().map(|r| r.operator_atoc.as_deref()).collect();

    // Normally exactly one date. Handled as a set anyway so a batch that
    // straddles a rail-day boundary replaces both days rather than half of
    // one -- and so the DELETE (when it runs at all -- see `first_chunk`
    // above) can never be wider than what is being written.
    let mut distinct_dates = service_dates.clone();
    distinct_dates.sort_unstable();
    distinct_dates.dedup();

    let mut tx = pool.begin().await?;

    if first_chunk {
        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE service_date = ANY($1::date[])",
        )
        .bind(&distinct_dates)
        .execute(&mut *tx)
        .await?;
    }

    let result = sqlx::query(
        "INSERT INTO schedule_destination_departures \
            (service_date, destination_crs, scheduled, day_offset, train_uid, origin_crs, true_origin_crs, calling_point_arrival, destination_arrival, destination_arrival_day_offset, operator_atoc) \
         SELECT * FROM UNNEST($1::date[], $2::text[], $3::time[], $4::smallint[], $5::text[], $6::text[], $7::text[], $8::time[], $9::time[], $10::smallint[], $11::text[]) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&service_dates)
    .bind(&destination_crs)
    .bind(&scheduled)
    .bind(&day_offsets)
    .bind(&train_uids)
    .bind(&origin_crs)
    .bind(&true_origin_crs)
    .bind(&calling_point_arrival)
    .bind(&destination_arrival)
    .bind(&destination_arrival_day_offsets)
    .bind(&operator_atoc)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
}

/// One `schedule_calling_points_full` row -- the literal, un-bucketed
/// "ordered stop_times per trip" shape, one row per calling point of one
/// resolved (non-cancelled) schedule on one service date. Mirrors
/// `schedule_query::CallingPoint` plus the schedule-level `uid` and the
/// publish-time-assigned `seq` ordering key, exactly as
/// `schedule-reference::publish_schedule_calling_points_full` emits them.
/// See docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase2-connections-array-plan.md
/// Task 1 and the migration's own doc comment
/// (`20260923100000_schedule_calling_points_full.sql`).
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleCallingPointsFullRow {
    pub service_date: chrono::NaiveDate,
    pub uid: String,
    /// 0-based position within this schedule's own calling-point sequence
    /// (from the publisher's own `.enumerate()`, `schedule-reference`'s
    /// `publish_schedule_calling_points_full`) -- the ORDER BY key that
    /// reconstructs stopping order; NOT a real CIF field, assigned at
    /// publish time.
    pub seq: i16,
    pub tiploc: String,
    /// One of `"origin"`, `"intermediate"`, `"terminate"` -- mirrors
    /// `schedule_query::CallingPointKind`'s three variants verbatim, kept
    /// as a plain string here (not a Rust enum) since this row's only job
    /// is to pass straight through to the `CHECK (kind IN (...))` column
    /// the migration defines.
    pub kind: String,
    pub booked_arrival: Option<chrono::NaiveTime>,
    pub booked_departure: Option<chrono::NaiveTime>,
    pub day_offset: i16,
}

/// Replaces one service date's worth of whole-network resolved calling
/// points. Same shape as `upsert_schedule_destination_departures` directly
/// above -- one transaction, `DELETE ... WHERE service_date = ANY(...)`
/// over the batch's distinct service dates followed by one multi-row
/// `INSERT ... SELECT * FROM UNNEST(...)` -- for the identical reason: a
/// per-row `ON CONFLICT` loop would be one round trip per calling point,
/// and this product is every calling point of every non-cancelled schedule
/// for the day.
///
/// **An empty `rows` is a no-op, and that is load-bearing**, same posture
/// and same reason as `upsert_schedule_destination_departures`: a publish
/// that produced nothing must not be allowed to delete a service date's
/// real data.
///
/// `ON CONFLICT DO NOTHING` on the insert: the primary key
/// (`service_date, uid, seq`) covers every row this publisher can produce
/// once per cycle, so a conflict can only mean a byte-identical duplicate
/// within the same batch -- dropping it silently is strictly better than
/// failing the whole batch over one pathological schedule.
///
/// Always clears the touched dates first -- the "one call is the whole day"
/// entry point every existing caller (this file's own tests,
/// `crates/api/src/routes/train.rs` and `crates/api/src/data/journey.rs`'s
/// fixtures) uses. The ingest route
/// (`crates/api/src/routes/ingest.rs::post_schedule_calling_points_full`)
/// instead calls [`upsert_schedule_calling_points_full_chunk`] -- see that
/// function's doc comment, and
/// `upsert_schedule_destination_departures`/`upsert_schedule_destination_departures_chunk`
/// directly above for the identical split and why it exists as two
/// functions rather than one with an added parameter.
pub async fn upsert_schedule_calling_points_full(
    pool: &PgPool,
    rows: &[ScheduleCallingPointsFullRow],
) -> Result<u64> {
    upsert_schedule_calling_points_full_chunk(pool, rows, true).await
}

/// Chunk-aware core of [`upsert_schedule_calling_points_full`] -- same
/// `first_chunk` contract as
/// [`upsert_schedule_destination_departures_chunk`]'s own doc comment:
/// `true` (every caller today) clears the touched dates first; `false`
/// skips the DELETE and only inserts, so a later chunk in the same publish
/// cycle cannot wipe out an earlier chunk's just-inserted rows for the same
/// date. `crates/api/src/routes/ingest.rs`'s `post_schedule_calling_points_full`
/// is the only caller that can pass `false`, via its `?first_chunk=` query
/// parameter.
pub async fn upsert_schedule_calling_points_full_chunk(
    pool: &PgPool,
    rows: &[ScheduleCallingPointsFullRow],
    first_chunk: bool,
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }

    let service_dates: Vec<chrono::NaiveDate> = rows.iter().map(|r| r.service_date).collect();
    let uids: Vec<&str> = rows.iter().map(|r| r.uid.as_str()).collect();
    let seqs: Vec<i16> = rows.iter().map(|r| r.seq).collect();
    let tiplocs: Vec<&str> = rows.iter().map(|r| r.tiploc.as_str()).collect();
    let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
    let booked_arrivals: Vec<Option<chrono::NaiveTime>> =
        rows.iter().map(|r| r.booked_arrival).collect();
    let booked_departures: Vec<Option<chrono::NaiveTime>> =
        rows.iter().map(|r| r.booked_departure).collect();
    let day_offsets: Vec<i16> = rows.iter().map(|r| r.day_offset).collect();

    // Normally exactly one date. Handled as a set anyway so a batch that
    // straddles a rail-day boundary replaces both days rather than half of
    // one -- and so the DELETE (when it runs at all -- see `first_chunk`
    // above) can never be wider than what is being written. Same
    // convention as `upsert_schedule_destination_departures_chunk`.
    let mut distinct_dates = service_dates.clone();
    distinct_dates.sort_unstable();
    distinct_dates.dedup();

    let mut tx = pool.begin().await?;

    if first_chunk {
        sqlx::query(
            "DELETE FROM schedule_calling_points_full WHERE service_date = ANY($1::date[])",
        )
        .bind(&distinct_dates)
        .execute(&mut *tx)
        .await?;
    }

    let result = sqlx::query(
        "INSERT INTO schedule_calling_points_full \
            (service_date, uid, seq, tiploc, kind, booked_arrival, booked_departure, day_offset) \
         SELECT * FROM UNNEST($1::date[], $2::text[], $3::smallint[], $4::text[], $5::text[], $6::time[], $7::time[], $8::smallint[]) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&service_dates)
    .bind(&uids)
    .bind(&seqs)
    .bind(&tiplocs)
    .bind(&kinds)
    .bind(&booked_arrivals)
    .bind(&booked_departures)
    .bind(&day_offsets)
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
    /// How many calendar days past `service_date` `scheduled` actually
    /// falls on -- mirrors `schedule_query::CallingPoint::day_offset` /
    /// `ScheduleDestinationDeparturesRow::day_offset` (see that struct's own
    /// doc comment for the live-confirmed overnight-service example this
    /// exists for). `journey::build_journey_stops`'s fallback branch adds
    /// this many days to `service_date` before pairing it with `scheduled`.
    pub day_offset: i16,
    pub true_origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    /// The schedule's own booked arrival at ITS terminus (not this row's
    /// calling point) -- same column `queries::CallingPointDepartureRow`'s
    /// sibling rows in the departure-board/search paths already select
    /// (e.g. `schedule_departures_between`, `search_journey_leg_candidates`).
    /// Every row for the same `train_uid`/`service_date` carries the same
    /// value (`schedule_destination_departures` denormalizes the terminus
    /// arrival onto every one of the schedule's own departure rows), so any
    /// row -- in practice `journey::build_journey_stops`'s fallback branch
    /// reads it off the LAST one -- gives the same answer.
    /// `journey::build_journey_stops`'s fallback branch (2026-09-22 UX
    /// review finding I16/2.7) uses this to populate the synthetic
    /// `Terminate` stop's `scheduled_arrival`, which was previously always
    /// hardcoded to `None` -- the one time this table's own booked-terminus
    /// arrival went uncollected. Genuinely `NULL` in real published CIF data
    /// for the rare schedule whose public timetable has no booked arrival at
    /// its own terminus (see this struct's sibling doc comments elsewhere in
    /// this file) -- not a bug when it happens, just nothing to show.
    pub destination_arrival: Option<chrono::NaiveTime>,
    /// `destination_arrival`'s own day offset past `service_date` -- mirrors
    /// `day_offset` above for the SAME overnight-service reason, but
    /// computed independently: the terminus arrival can fall on a different
    /// calendar day than this particular calling point's own departure.
    pub destination_arrival_day_offset: i16,
}

/// Every departure-bearing calling point of `train_uid`'s schedule on
/// `service_date`, in true schedule order. See `CallingPointDepartureRow`'s
/// doc comment for why this exists; see the design doc §0.2 for why the
/// schedule's own terminus is NOT among these rows (no `booked_departure`
/// for a `Terminate` calling point) -- the caller appends it separately.
///
/// `ORDER BY day_offset, scheduled`, NOT bare `ORDER BY scheduled` -- a bare
/// `NaiveTime` sort would put a real overnight service's post-midnight
/// calling points (small `scheduled` values, e.g. `00:07`) BEFORE its
/// pre-midnight ones (large values, e.g. `23:48`), inverting the journey
/// timeline for exactly the schedules this whole fix targets. `day_offset`
/// as the leading sort key restores true chronological order; this table's
/// per-train row count is small enough (one schedule's worth of calling
/// points) that no index change is needed for it.
///
/// **No longer `journey::build_journey_stops`'s own fallback source** (see
/// [`list_schedule_calling_points_full_for_train`], below, which replaced it
/// there 2026-09-23) -- `schedule_destination_departures` is built by
/// `schedule_query::resolve::departures_by_destination_crs`, which silently
/// `continue`s (drops the row entirely) whenever a calling point's own
/// TIPLOC doesn't resolve to a CRS via that cycle's `tiploc_to_crs` map.
/// That is the right call for THIS table's actual job (the calling-point
/// search index needs a real CRS to search by, and a station nobody can
/// name can't be searched for), but it made a real, booked calling point
/// whose TIPLOC happened not to resolve vanish ENTIRELY from a single
/// train's pre-tracking calling-point list, while the exact same
/// resolution gap on the post-tracking path (`journey::stops_from_calling_points`,
/// fed from `trains.calling_points`) instead kept the row with a blank
/// identity -- one symptom read as "a real stop is missing", the other as
/// "the same stop shows up unresolved", for what was really one shared
/// root cause. Confirmed against live production data for train `Y80908`
/// on 2026-09-23: its calling point at Northampton (`NMPTN`, booked
/// 17:08/17:18) has both a real booked arrival AND departure -- unlike this
/// function's own `WHERE`-clause siblings, a genuine passenger stop, not a
/// junction -- yet came back with `crs: None`/`name: None` on the
/// post-tracking path. This function is kept (and still exercised by its
/// own tests below) purely because `schedule_destination_departures` still
/// backs `GET /public/trains/search`'s calling-point index, which is
/// unrelated to and unaffected by this change.
pub async fn list_calling_point_departures_for_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: chrono::NaiveDate,
) -> Result<Vec<CallingPointDepartureRow>> {
    let rows = sqlx::query_as::<_, CallingPointDepartureRow>(
        "SELECT origin_crs, scheduled, day_offset, true_origin_crs, destination_crs, \
                destination_arrival, destination_arrival_day_offset \
         FROM schedule_destination_departures \
         WHERE train_uid = $1 AND service_date = $2 \
         ORDER BY day_offset, scheduled",
    )
    .bind(train_uid)
    .bind(service_date)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// One `schedule_calling_points_full` row as read back for a single train's
/// journey view -- deliberately narrower than `ScheduleCallingPointsFullRow`
/// above (no `service_date`/`uid`/`seq`: the caller already knows the first
/// two, having supplied them as the query's own `WHERE` filter, and `seq` is
/// consumed entirely by the `ORDER BY` below, never read back into Rust --
/// same "ordering is SQL's job, not re-sorted in Rust" posture
/// `trip_planning::CallingPointRow` already has for the identical column).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ScheduleCallingPointFullRowForTrain {
    pub tiploc: String,
    /// `"origin"`/`"intermediate"`/`"terminate"` -- see
    /// `ScheduleCallingPointsFullRow::kind`'s own doc comment; the caller
    /// (`journey::build_journey_stops`) parses this back into
    /// `schedule_query::CallingPointKind`.
    pub kind: String,
    pub booked_arrival: Option<chrono::NaiveTime>,
    pub booked_departure: Option<chrono::NaiveTime>,
    pub day_offset: i16,
}

/// Every calling point of `train_uid`'s resolved (non-cancelled) schedule on
/// `service_date`, in true schedule order, straight off
/// `schedule_calling_points_full` -- `journey::build_journey_stops`'s
/// fallback source when `trains.calling_points` hasn't been populated by
/// schedule-matching yet (see `list_calling_point_departures_for_train`'s
/// own doc comment, above, for why this replaced
/// `schedule_destination_departures` there). Unlike that predecessor:
///
/// * every calling point comes back, INCLUDING one whose TIPLOC never
///   resolves to a CRS -- resolution happens later, in
///   `journey::stops_from_calling_points`, via the same
///   `crs_for_tiplocs_batch` batch join the primary (`trains.calling_points`)
///   path already uses, so a resolution gap now degrades identically on
///   both paths instead of "missing" on one and "unresolved" on the other;
/// * the schedule's own terminating calling point is already one of these
///   rows (`kind = 'terminate'`), so the caller no longer needs to
///   separately reconstruct a synthetic `Terminate` stop from a
///   denormalized `destination_crs`/`destination_arrival` pair.
///
/// `ORDER BY seq` alone (not `day_offset, scheduled` like its predecessor):
/// `seq` is assigned at publish time by walking the schedule's own resolved
/// `calling_points` in order (`schedule-reference`'s
/// `publish_schedule_calling_points_full`), so it is already true
/// chronological order -- including across a midnight crossing -- with no
/// separate day-offset leading key needed to recover it.
pub async fn list_schedule_calling_points_full_for_train(
    pool: &PgPool,
    train_uid: &str,
    service_date: chrono::NaiveDate,
) -> Result<Vec<ScheduleCallingPointFullRowForTrain>> {
    let rows = sqlx::query_as::<_, ScheduleCallingPointFullRowForTrain>(
        "SELECT tiploc, kind, booked_arrival, booked_departure, day_offset \
         FROM schedule_calling_points_full \
         WHERE service_date = $1 AND uid = $2 \
         ORDER BY seq",
    )
    .bind(service_date)
    .bind(train_uid)
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
/// key, only a new leading index. `true_origin_crs` is an optional filter
/// layered on top, independent of `station_crs`.
///
/// `scheduled_from` is an INCLUSIVE lower bound: the caller's explicit
/// `from` when one was given, or its own `now`-forward default otherwise
/// (see `crates/api/src/routes/trains.rs`'s own doc comment for that
/// default's rules) -- either way, a single already-resolved value with no
/// further flooring done by this function. `to_time` is an INCLUSIVE upper
/// bound. Both carry the exact same reasoning as the predecessor query.
///
/// **`stops_at`: "calls at this station LATER in the journey than the
/// searched calling point" (a plain membership test with an ordering
/// constraint, not relational division).** `None` means "no filter" (every
/// row matches). `Some(crs)` means the schedule must call at that CRS on
/// the SAME `service_date`, strictly after the calling point this result
/// row is itself anchored at -- either
///
/// * some other departure-bearing row of the same `train_uid` carries it
///   (a correlated `EXISTS` on `origin_crs`, the same column `station_crs`
///   matches against) -- departure-BEARING, so an arrival-only call that
///   is not the terminus would fall through both branches; CIF's `LI`
///   records carry both times, making that case theoretical rather than
///   real; or
/// * it is the schedule's TRUE terminating calling point
///   (`main.destination_crs`), which has no `booked_departure` and so
///   never gets a row of its own in this table at all (see the table's own
///   migration comment).
///
/// Both halves changed in the "loop service" fix (2026-09-17), and both for
/// the same reason -- a CRS does not identify one PLACE IN A JOURNEY,
/// exactly the mistake `journey::assign_events_to_stops` was fixed for. The
/// ordering rule below was then widened (2026-09-22, superseding part of
/// the 2026-09-17 fix -- see this function's own history and
/// docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md's
/// "Addendum (2026-09-22)") from applying only when `stops_at` repeats
/// `station_crs` to applying unconditionally, on the product decision that
/// "stops at X" should always mean "you can actually get there from where
/// you searched" -- no special-cased exception for a `stops_at` value that
/// differs from `station_crs`.
///
/// 1. EVERY call this branch matches -- same station as the search anchor
///    or a different one -- counts only if it comes LATER in the journey,
///    by `(day_offset, scheduled)`. This is what makes `stops_at ==
///    station_crs` a real question instead of a tautology: a row is a
///    member of its own calling-point list by construction, so an
///    unrestricted `EXISTS` made "departing from WAT AND stopping at WAT"
///    match every single train out of Waterloo -- byte-for-byte the same
///    result set as supplying no `stops_at` at all (live-confirmed against
///    a real Postgres before the 2026-09-17 fix). What a caller typing the
///    same station twice means is "come BACK here": a loop/circular
///    working such as South Western Railway's Kingston Loop (train
///    L82877, 2026-09-14 -- Waterloo 07:27 round via Clapham Junction,
///    Kingston and Richmond, terminating back at Waterloo 08:46). The same
///    reasoning now extends to any other named station too: a rider typing
///    "stops at Reading" wants trains that genuinely go on to reach
///    Reading from where they searched, not ones that already passed
///    through Reading before reaching the search origin.
///
///    LATER, not merely OTHER, and the difference is not academic. A
///    working that passes back through its own origin and carries on
///    (`WAT -> ... -> WAT -> ... -> SOU`) offers two WAT departures, and
///    only the FIRST of them comes back; a bare "some other row at this
///    CRS" test would return both, half of them being trains the caller
///    would board expecting a return that never happens. The same logic
///    applies across two different stations: a working that calls at X
///    AFTER the searched station is exactly what a caller wants, but one
///    that called at X BEFORE ever reaching the searched station is not
///    reachable from there at all.
///
///    The comparison is `(day_offset, scheduled)`, not `scheduled` alone:
///    a real overnight schedule crosses midnight and its later calls carry
///    a smaller clock time (see `day_offset`'s own migration). Two
///    same-station calls can never TIE under it, and the guarantee is
///    structural rather than a fact about timetabling: this table's
///    primary key covers `(service_date, destination_crs, scheduled,
///    train_uid, origin_crs)`, `destination_crs` is constant per
///    `(service_date, train_uid)` (`schedule_query::resolve::
///    departures_by_destination_crs` computes it once from the schedule's
///    last calling point), and the ingest is `ON CONFLICT DO NOTHING`, so
///    two same-station calls sharing a `scheduled` cannot BOTH be rows
///    here -- they collapse to one at insert. (Note the boundary: the key
///    does not carry `day_offset`, so a revisit at the same clock minute
///    exactly a day later loses its row at ingest and is then correctly,
///    but vacuously, excluded here. Pre-existing, and vanishingly rare.)
///    Two DIFFERENT stations, by contrast, genuinely can share a booked
///    minute -- the ordering comparison still resolves it (neither ties
///    the other under `>`), it just is not backed by the same uniqueness
///    guarantee.
///
///    This is also the only part of this query that reads `day_offset` at
///    all: `scheduled_from`/`to_time`, the `ORDER BY` and the keyset
///    cursor are all still plain wall-clock comparisons on `scheduled`,
///    exactly as they were. Deliberate -- widening them is a separate
///    question about how a midnight-crossing rail day should paginate --
///    but do not read this rule as evidence that they follow suit.
///
/// 2. ADMITTING the true terminus closes the gap this filter shipped with
///    and its design note flagged ("a `stops_at` value naming a schedule's
///    true TERMINATING calling point never matches"). That gap contradicts
///    the filter's own stated purpose -- "does this train call at Reading,
///    regardless of whether Reading is where the schedule actually ends"
///    -- and, more sharply, it makes the loop case above unanswerable: the
///    Kingston Loop's second Waterloo call IS its terminus, so without
///    this branch item 1's `EXISTS` finds nothing. The terminus needs no
///    ordering test of its own: it is downstream of every departure-
///    bearing row by construction. This branch is a widening for ordinary
///    searches too (`stops_at` naming any schedule's true destination now
///    matches it), which is deliberate.
///
/// Deliberately single-valued: an earlier version of this filter accepted
/// zero or more stations and matched ALL-of-N (relational division via
/// `COUNT(DISTINCT origin_crs)`); that was scoped back down to exactly one
/// station. See
/// docs/superpowers/specs/2026-09-09-stops-at-search-filter-design.md.
///
/// **`stop_arrival_from`/`stop_arrival_to`: a SEPARATE inclusive bound pair
/// on the arrival at the calling point `stops_at` named** -- NOT on
/// `main`'s own `scheduled`/`origin_crs` (`station_crs` and the
/// arrival-bound calling point are frequently two different rows of the
/// same schedule). It mirrors `stops_at`'s own two branches above, same
/// unconditional ordering rule and all, so that these bounds are asked
/// about the very calling point `stops_at` matched on:
///
/// * against `calling_point_arrival` on the correlated `EXISTS` branch,
///   for a genuine intermediate call. Not `destination_arrival`: that
///   column is the schedule's TRUE destination's arrival, a different,
///   schedule-level value that need not have anything to do with which
///   calling point `stops_at` named (see `calling_point_arrival`'s own
///   column comment); and
/// * against `destination_arrival` when -- and only when -- `stops_at` is
///   matching via the terminus branch, where it is not a schedule-level
///   stand-in but literally the arrival at the named calling point.
///
/// A NULL arrival never satisfies a bound, on either branch. Both columns
/// are genuinely nullable in real published data (`destination_arrival`'s
/// own migration says as much), so setting an arrival bound drops
/// schedules whose named calling point has no booked arrival at all --
/// pre-existing behavior on the `calling_point_arrival` branch, and the
/// terminus branch matches it deliberately rather than inventing a
/// "NULL passes" rule for one branch only.
///
/// Day offsets are deliberately not consulted by either ARRIVAL
/// comparison (unlike item 1's ordering test, which needs them): these are
/// wall-clock bounds on a `TIME` column, matching how the original
/// `calling_point_arrival` bound has always behaved.
///
/// The route layer (not this function) rejects either bound being set
/// without `stops_at` also being set -- this function applies whatever it
/// is given, filter-shaped, with no cross-field validation of its own,
/// matching how it already treats every other Option argument here.
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
    stops_at: Option<&str>,
    to_time: Option<chrono::NaiveTime>,
    stop_arrival_from: Option<chrono::NaiveTime>,
    stop_arrival_to: Option<chrono::NaiveTime>,
    after: Option<&CallingPointDepartureCursor>,
    limit: i64,
) -> Result<Option<CallingPointDeparturePage>> {
    let fetch = limit.saturating_add(1);

    // train_uid, destination_crs, true_origin_crs, scheduled,
    // destination_arrival, destination_arrival_day_offset, operator_atoc.
    type CallingPointDepartureRow = (
        String,
        String,
        Option<String>,
        chrono::NaiveTime,
        Option<chrono::NaiveTime>,
        i16,
        Option<String>,
    );

    let rows: Vec<CallingPointDepartureRow> = sqlx::query_as(
        r#"
            SELECT main.train_uid, main.destination_crs, main.true_origin_crs, main.scheduled, main.destination_arrival, main.destination_arrival_day_offset, main.operator_atoc
            FROM schedule_destination_departures main
            WHERE main.service_date = $1
              AND main.origin_crs = $2
              AND main.scheduled >= $3
              AND ($4::text IS NULL OR main.true_origin_crs = $4)
              AND ($6::time IS NULL OR main.scheduled <= $6)
              AND (
                    $5::text IS NULL
                    -- The true terminus: arrival-only, so it has no row of
                    -- its own here and is reachable ONLY as this column.
                    OR main.destination_crs = $5
                    OR EXISTS (
                        SELECT 1
                        FROM schedule_destination_departures stop
                        WHERE stop.service_date = $1
                          AND stop.train_uid = main.train_uid
                          AND stop.origin_crs = $5
                          -- A call at the calling point `stops_at` named --
                          -- same station this result row is anchored at, or
                          -- a different one -- counts only if it comes
                          -- LATER: otherwise it is not reachable from the
                          -- searched calling point at all (2026-09-22:
                          -- this used to be gated to the same-station case
                          -- only -- see the design doc's superseded
                          -- 2026-09-17 addendum -- but "stops at X" now
                          -- means "later than the search origin" for every
                          -- X, not just a same-station loop match).
                          AND (stop.day_offset, stop.scheduled)
                              > (main.day_offset, main.scheduled)
                    )
              )
              AND (
                    ($7::time IS NULL AND $8::time IS NULL)
                    OR (
                        main.destination_crs = $5
                        AND ($7::time IS NULL OR main.destination_arrival >= $7)
                        AND ($8::time IS NULL OR main.destination_arrival <= $8)
                    )
                    OR EXISTS (
                        SELECT 1
                        FROM schedule_destination_departures stop
                        WHERE stop.service_date = $1
                          AND stop.train_uid = main.train_uid
                          AND stop.origin_crs = $5
                          AND (stop.day_offset, stop.scheduled)
                              > (main.day_offset, main.scheduled)
                          AND ($7::time IS NULL OR stop.calling_point_arrival >= $7)
                          AND ($8::time IS NULL OR stop.calling_point_arrival <= $8)
                    )
              )
              AND ($9::time IS NULL
                   OR (main.scheduled, main.train_uid) > ($9, $10))
            ORDER BY main.scheduled, main.train_uid
            LIMIT $11
            "#,
    )
    .bind(service_date)
    .bind(station_crs)
    .bind(scheduled_from)
    .bind(true_origin_crs)
    .bind(stops_at)
    .bind(to_time)
    .bind(stop_arrival_from)
    .bind(stop_arrival_to)
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
            |(train_uid, _, _, scheduled, _, _, _)| CallingPointDepartureCursor {
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
            |(
                train_uid,
                destination_crs,
                true_origin_crs,
                scheduled,
                destination_arrival,
                destination_arrival_day_offset,
                operator_atoc,
            )| {
                serde_json::json!({
                    "uid": train_uid,
                    "destination_crs": destination_crs,
                    "true_origin_crs": true_origin_crs,
                    "scheduled": scheduled.format("%H:%M:%S").to_string(),
                    "destination_arrival": destination_arrival.map(|t| t.format("%H:%M:%S").to_string()),
                    "destination_arrival_day_offset": destination_arrival_day_offset,
                    "operator_atoc": operator_atoc,
                })
            },
        )
        .collect();

    Ok(Some(CallingPointDeparturePage {
        departures,
        next_cursor,
    }))
}

/// The time-window candidate search behind journey-leg matching (`GET
/// /Journeys/{journeyId}/legs/{legId}/candidates`,
/// `crates/api/src/routes/journeys.rs`) --
/// docs/superpowers/specs/2026-09-22-journey-tracking-design.md §2.1.
///
/// A deliberate SIBLING of `search_schedule_calling_point_departures`
/// above, not an extension of it, even though the design doc names
/// extending that function as one option. Two reasons:
///
/// 1. **Required-vs-optional shape, not a behavioral difference in
///    ordering.** A journey leg's "depart X, arrive Y" is meaningless
///    unless Y is reached strictly AFTER departing X (§0.5's own named
///    gap) -- so this function's `EXISTS` branches enforce that ordering
///    UNCONDITIONALLY, for every origin/destination pair, with BOTH
///    `origin_crs` and `destination_crs` required parameters.
///    **Correction (Signal Box Audit, Low finding):** this doc comment
///    used to claim `search_schedule_calling_point_departures` above
///    still only enforced that ordering for the SAME-station case and
///    deliberately not for two different stations -- true when this
///    function was first written, but stale even at the time this
///    comment shipped: that function's own doc comment (point 1,
///    directly above) documents the 2026-09-22 widening that made it
///    enforce the identical unconditional "later in the journey, by
///    `(day_offset, scheduled)`" ordering for its own OPTIONAL `stops_at`
///    parameter, same-station or not. The genuine, still-true reason
///    this stays a sibling rather than folding into that function is the
///    required-vs-optional shape: a journey leg always names both ends
///    (`origin_crs`/`destination_crs` both mandatory here), while
///    `search_schedule_calling_point_departures` backs the general-purpose
///    `/trains` search page, where `stops_at` is one optional filter among
///    several on a single-station-anchored search, not a second mandatory
///    endpoint. Forcing that shape to also carry a mandatory second
///    endpoint would be a real, separate API-shape change to an
///    already-shipped public endpoint, not something to fold in silently
///    here.
/// 2. **Merge safety.** At the time this function was introduced, other
///    in-flight, unmerged branches independently modified
///    `search_schedule_calling_point_departures`'s own `stops_at`/
///    ordering logic (see this plan's own staleness note) -- since landed
///    as the 2026-09-22 widening point 1 references above. A sibling
///    function with zero line overlap could not collide with that
///    in-flight work.
///
/// Consequently this duplicates ~25 lines of row-to-JSON mapping logic
/// from `search_schedule_calling_point_departures` rather than factoring
/// out a shared helper -- deliberately, for the same merge-safety reason.
/// Worth doing once the in-flight `stops_at` work above has landed and
/// this function's own shape has proven stable; not attempted here.
///
/// No `true_origin_crs`/`stops_at` params, unlike the function above: a
/// journey leg always names both ends explicitly (`origin_crs`,
/// `destination_crs`, both required), so there is no "optional filter"
/// shape to carry over. `depart_after`/`depart_before` bound
/// `main.scheduled` (the departure at `origin_crs`, both now genuinely
/// optional, unlike `search_schedule_calling_point_departures`'s
/// mandatory `scheduled_from` -- that function's caller always supplies a
/// concrete floor, either an explicit `from` or a `now`-forward default;
/// a journey-leg window search has no such default to fall back on).
/// `arrive_after`/`arrive_before` bound the arrival at `destination_crs`,
/// mirroring `stop_arrival_from`/`stop_arrival_to`'s own two-branch shape
/// above (the `EXISTS` branch for an intermediate call, `main.destination_crs
/// = $5` for the true-terminus case) -- same NULL-never-satisfies-a-bound
/// contract.
///
/// `Ok(None)` means no CIF publish has landed for `service_date` at all
/// (maps to a 404, mirroring the function above). `Ok(Some(page))` with an
/// empty `page.departures` means the day IS published and the window
/// matched nothing.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub async fn search_journey_leg_candidates(
    pool: &PgPool,
    origin_crs: &str,
    destination_crs: &str,
    service_date: chrono::NaiveDate,
    depart_after: Option<chrono::NaiveTime>,
    depart_before: Option<chrono::NaiveTime>,
    arrive_after: Option<chrono::NaiveTime>,
    arrive_before: Option<chrono::NaiveTime>,
    // Optional ATOC-code allowlist, same "comma-split, empty means no
    // filter" shape `search_incidents`'s own `operators` param establishes
    // (`routes::incidents::search_incidents`) -- an "any of" filter. `None`
    // correctly means "no filter": the bound SQL parameter is NULL and the
    // `$8::text[] IS NULL OR ...` predicate below short-circuits to true
    // for every row. `Some(vec![])` does NOT also mean "no filter", though
    // -- an empty-but-present list still binds an empty array, and
    // `operator_atoc = ANY('{}')` is false for every row, so it would
    // filter out everything. The current caller
    // (`routes::journeys::get_leg_candidates`) can never actually produce
    // `Some(vec![])` (its own comma-split collapses an empty result back
    // to `None`), so this distinction has no live effect today -- but it
    // is load-bearing for any future caller that skips that step.
    operators: Option<Vec<String>>,
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
        i16,
        Option<String>,
        Option<chrono::NaiveTime>,
        Option<i16>,
    )> = sqlx::query_as(
        r#"
            SELECT main.train_uid, main.destination_crs, main.true_origin_crs, main.scheduled, main.destination_arrival, main.destination_arrival_day_offset, main.operator_atoc,
                   -- The arrival at the LEG's own destination, which is
                   -- what the traveller is actually choosing between --
                   -- NOT `destination_arrival`, the arrival at the
                   -- schedule's TERMINUS, which for a York->Newcastle leg
                   -- on a London->Edinburgh service names a station the
                   -- traveller never reaches. Two branches, mirroring the
                   -- arrive_after/arrive_before filter below exactly: the
                   -- leg's destination IS this schedule's terminus (a
                   -- terminus has no departure row of its own, so the
                   -- value lives on `main`), or it is an intermediate
                   -- call, found by the same keys the EXISTS below uses.
                   -- COALESCE onto `stop.scheduled` because
                   -- `calling_point_arrival` is NULL by design for a
                   -- schedule's own true origin and for rows published
                   -- before that column existed: the booked DEPARTURE at
                   -- that calling point is an honest, real scheduled time
                   -- for that station rather than a fabricated one, and
                   -- it is the same departure-or-arrival precedence
                   -- `JourneyTimeline` already displays per stop. NULL
                   -- stays NULL when neither is known -- the caller
                   -- renders nothing rather than guessing.
                   CASE WHEN main.destination_crs = $5 THEN main.destination_arrival
                        ELSE (SELECT COALESCE(stop.calling_point_arrival, stop.scheduled)
                              FROM schedule_destination_departures stop
                              WHERE stop.service_date = $1
                                AND stop.train_uid = main.train_uid
                                AND stop.origin_crs = $5
                                AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled)
                              ORDER BY stop.day_offset, stop.scheduled
                              LIMIT 1)
                   END AS leg_destination_arrival,
                   CASE WHEN main.destination_crs = $5 THEN main.destination_arrival_day_offset
                        ELSE (SELECT stop.day_offset
                              FROM schedule_destination_departures stop
                              WHERE stop.service_date = $1
                                AND stop.train_uid = main.train_uid
                                AND stop.origin_crs = $5
                                AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled)
                              ORDER BY stop.day_offset, stop.scheduled
                              LIMIT 1)
                   END AS leg_destination_arrival_day_offset
            FROM schedule_destination_departures main
            WHERE main.service_date = $1
              AND main.origin_crs = $2
              AND ($3::time IS NULL OR main.scheduled >= $3)
              AND ($4::time IS NULL OR main.scheduled <= $4)
              AND (
                    main.destination_crs = $5
                    OR EXISTS (
                        SELECT 1
                        FROM schedule_destination_departures stop
                        WHERE stop.service_date = $1
                          AND stop.train_uid = main.train_uid
                          AND stop.origin_crs = $5
                          -- Unconditional, unlike
                          -- search_schedule_calling_point_departures'
                          -- same-station-only ordering check -- a journey
                          -- leg's destination must be reached AFTER its
                          -- origin regardless of which two stations they
                          -- are. See this function's own doc comment.
                          AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled)
                    )
              )
              AND (
                    ($6::time IS NULL AND $7::time IS NULL)
                    OR (
                        main.destination_crs = $5
                        AND ($6::time IS NULL OR main.destination_arrival >= $6)
                        AND ($7::time IS NULL OR main.destination_arrival <= $7)
                    )
                    OR EXISTS (
                        SELECT 1
                        FROM schedule_destination_departures stop
                        WHERE stop.service_date = $1
                          AND stop.train_uid = main.train_uid
                          AND stop.origin_crs = $5
                          AND (stop.day_offset, stop.scheduled) > (main.day_offset, main.scheduled)
                          AND ($6::time IS NULL OR stop.calling_point_arrival >= $6)
                          AND ($7::time IS NULL OR stop.calling_point_arrival <= $7)
                    )
              )
              AND ($8::text[] IS NULL OR main.operator_atoc = ANY($8))
              AND ($9::time IS NULL
                   OR (main.scheduled, main.train_uid) > ($9, $10))
            ORDER BY main.scheduled, main.train_uid
            LIMIT $11
            "#,
    )
    .bind(service_date)
    .bind(origin_crs)
    .bind(depart_after)
    .bind(depart_before)
    .bind(destination_crs)
    .bind(arrive_after)
    .bind(arrive_before)
    .bind(operators)
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
        page_rows
            .last()
            .map(
                |(train_uid, _, _, scheduled, _, _, _, _, _)| CallingPointDepartureCursor {
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
            |(
                train_uid,
                destination_crs,
                true_origin_crs,
                scheduled,
                destination_arrival,
                destination_arrival_day_offset,
                operator_atoc,
                leg_destination_arrival,
                leg_destination_arrival_day_offset,
            )| {
                serde_json::json!({
                    "uid": train_uid,
                    "destination_crs": destination_crs,
                    "true_origin_crs": true_origin_crs,
                    "scheduled": scheduled.format("%H:%M:%S").to_string(),
                    "destination_arrival": destination_arrival.map(|t| t.format("%H:%M:%S").to_string()),
                    "destination_arrival_day_offset": destination_arrival_day_offset,
                    "operator_atoc": operator_atoc,
                    // Two EXTRA keys this sibling emits and
                    // `search_schedule_calling_point_departures` does not:
                    // the arrival at the LEG's own destination. Carried on
                    // the same opaque row `Value` the shared
                    // `render::calling_point_departure_json` already
                    // consumes, so `/public/trains/search` (which has no
                    // leg, and no destination to scope to) is untouched --
                    // the journeys route picks these two off the row
                    // itself. `null` is a real, expected value: a
                    // candidate whose schedule has no arrival or departure
                    // recorded at the leg's destination renders no arrival
                    // rather than a guessed one.
                    "leg_destination_arrival": leg_destination_arrival.map(|t| t.format("%H:%M:%S").to_string()),
                    "leg_destination_arrival_day_offset": leg_destination_arrival_day_offset,
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

/// One line's current `full_coverage_line_stats` row, if
/// `full-coverage-consumer` has ever published one -- unlike
/// `last_full_coverage_line_stats_fetch` (a bare freshness timestamp for
/// `poller`-style startup-skip logic), this returns the actual row.
/// Added for `data::full_coverage_comparison`'s own "live snapshot" section
/// (this crate had no full-row reader for this table at all before that --
/// every other caller either upserts it or only needs the freshness
/// timestamp).
pub async fn get_full_coverage_line_stats(
    pool: &PgPool,
    line_id: &str,
) -> Result<Option<common::FullCoverageLineStatsRow>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT service_date, availability, total, delayed, cancelled, skipped, avg_delay_minutes
         FROM full_coverage_line_stats WHERE line_id = $1",
    )
    .bind(line_id)
    .fetch_optional(pool)
    .await?;
    row.map(|row| {
        Ok(common::FullCoverageLineStatsRow {
            line_id: line_id.to_string(),
            service_date: row.try_get("service_date")?,
            availability: row.try_get("availability")?,
            stats: common::SampleStats {
                total: row.try_get::<i32, _>("total")? as usize,
                delayed: row.try_get::<i32, _>("delayed")? as usize,
                cancelled: row.try_get::<i32, _>("cancelled")? as usize,
                skipped: row.try_get::<i32, _>("skipped")? as usize,
                avg_delay_minutes: row.try_get("avg_delay_minutes")?,
            },
        })
    })
    .transpose()
}

/// The latest `StationSample` polled for a single station, or `None` if
/// `station_samples` has no row for that CRS yet. `station_samples` is
/// wholesale-replaced per poll (one row per station, no history -- see
/// `upsert_station_samples`), so "latest" here just means "the current
/// row", not a query over a time range. Backs `crates/api/src/data/eta_blend.rs`'s
/// read-time Darwin/TRUST correlation (`routes/train.rs`'s
/// `blend_darwin_eta`), which needs one station's current departure board
/// to look up against a tracked train's pin/next-calling-point.
///
/// `UPPER(TRIM(crs)) = UPPER(TRIM($1))`: this is the exact odd-one-out this doc
/// comment used to warn about -- every sibling CRS lookup in this file
/// (`list_stanox_crs_for_crs`, `list_fixed_links_from_crs`,
/// `station_names_for_crs_batch`) case-folds before comparing, but this
/// one used to compare with a raw `=`. That silently broke two real
/// callers: `routes::departures::get_station_departures`, which binds
/// `crs` straight off the URL path with no normalization, would 404 for
/// a lowercase path segment even though the uppercase form resolved; and
/// `station_skip::leg_skip_status` (the "origin-skip detection" this
/// backs -- §5.2 of the journey-tracking design), which would silently
/// treat a leg as "no sample, not skipped" instead of actually checking,
/// for any `journey_legs.origin_crs`/`destination_crs` that wasn't stored
/// upper-case.
pub async fn latest_station_sample(pool: &PgPool, crs: &str) -> Result<Option<StationSample>> {
    use sqlx::Row;
    let row = sqlx::query(
        "SELECT crs, polled_at, departures FROM station_samples WHERE UPPER(TRIM(crs)) = UPPER(TRIM($1))",
    )
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
///
/// `UPPER(TRIM(crs)) = UPPER(TRIM($1))`, matching `latest_station_sample`'s own fix
/// directly above (same table family, same `routes::station_stats`
/// caller passing an un-normalized path segment) -- kept consistent
/// rather than letting this sibling drift back into the same raw-`=` bug.
pub async fn latest_station_full_coverage_samples(
    pool: &PgPool,
    crs: &str,
) -> Result<Vec<StationFullCoverageSample>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT crs, operator, resolved_at, stats FROM station_full_coverage_samples \
         WHERE UPPER(TRIM(crs)) = UPPER(TRIM($1))",
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

/// Cross-line sibling of `daily_stats_for_range` -- sums the same
/// `line_status_daily_stats` rows across every id in `line_ids` instead of
/// reading one line. Used for an operator's or the whole network's Trends
/// rollup (docs/superpowers/plans/2026-09-22-operator-overview-phase4-historical-views-plan.md).
///
/// Every column here is ALREADY a running sum-per-line-per-day (see that
/// table's own migration comment) -- summing a sum across several lines
/// for the same day is exactly as lossless as summing a sum across
/// several poll cycles for one line, which `sub_daily_stats_for_range`
/// already relies on (that function's own doc comment: "summing sums is
/// lossless per [Decision 2's] Correction 4"). Rates are still derived at
/// READ time from the summed numerator/denominator columns, never
/// pre-averaged across lines.
///
/// An empty `line_ids` slice is valid and returns an empty vec (Postgres'
/// `= ANY('{}')` is always false, never an error) -- the caller (an
/// unknown operator code, or a network with zero catalogue lines) needs
/// no special-case branch for this.
///
/// **`sample_cycles` is the one exception to "every column here is a plain
/// sum"**: it is normalized to the AVERAGE per contributing line
/// (`SUM(sample_cycles) / COUNT(*)`, `COUNT(*)` here counting the number of
/// per-line rows -- i.e. lines with a row -- that fed this day) rather than
/// summed outright. `SPARSE_FLOOR` in the frontend's `toChartPoints` is
/// calibrated against one line's poll-cycle count; a raw cross-line sum
/// would scale with however many lines are in `line_ids` (~125 at network
/// scope) and make the sparse-data gap check effectively never fire. See
/// Finding 1 of the Phase 4 final-review pass for the full reasoning.
pub async fn daily_stats_for_range_multi(
    pool: &PgPool,
    line_ids: &[String],
    from: chrono::NaiveDate,
    to: chrono::NaiveDate,
) -> Result<Vec<DailyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT day,
                (SUM(sample_cycles) / GREATEST(COUNT(*), 1))::bigint AS sample_cycles,
                SUM(total)::bigint AS total,
                SUM(delayed)::bigint AS delayed,
                SUM(cancelled)::bigint AS cancelled,
                SUM(skipped)::bigint AS skipped,
                SUM(running_count)::bigint AS running_count,
                SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_daily_stats
         WHERE line_id = ANY($1) AND day BETWEEN $2 AND $3
         GROUP BY day
         ORDER BY day",
    )
    .bind(line_ids)
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

/// Cross-line sibling of `half_hourly_stats_for_range` -- same relationship
/// `daily_stats_for_range_multi` has to `daily_stats_for_range`, including
/// the same `sample_cycles` normalization: `SUM(sample_cycles) / COUNT(*)`
/// (average per contributing line for this bucket, `COUNT(*)` counting the
/// per-line rows that fed it), not a raw sum -- see
/// `daily_stats_for_range_multi`'s doc comment for why.
pub async fn half_hourly_stats_for_range_multi(
    pool: &PgPool,
    line_ids: &[String],
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
) -> Result<Vec<HalfHourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT half_hour_start,
                (SUM(sample_cycles) / GREATEST(COUNT(*), 1))::bigint AS sample_cycles,
                SUM(total)::bigint AS total,
                SUM(delayed)::bigint AS delayed,
                SUM(cancelled)::bigint AS cancelled,
                SUM(skipped)::bigint AS skipped,
                SUM(running_count)::bigint AS running_count,
                SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_half_hourly_stats
         WHERE line_id = ANY($1) AND half_hour_start BETWEEN $2 AND $3
         GROUP BY half_hour_start
         ORDER BY half_hour_start",
    )
    .bind(line_ids)
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

/// Cross-line sibling of `sub_daily_stats_for_range` -- same `date_bin`
/// re-bucketing (1-hour or 6-hour, selected by `bucket_minutes`, always a
/// literal `60`/`360` from this crate's own route handlers, never raw
/// request input -- see that function's own doc comment for the full
/// injection-safety/origin-alignment reasoning, unchanged here), but
/// summed across every id in `line_ids` in the SAME `GROUP BY` pass rather
/// than as a separate step -- there is no correctness difference between
/// "sum across lines, then re-bucket" and "re-bucket, then sum across
/// lines" for a plain SUM aggregate, so the single combined query is
/// preferred for one round trip instead of two.
///
/// **`sample_cycles` is normalized here too, but by a different
/// denominator than its two siblings above**: `SUM(sample_cycles) /
/// COUNT(DISTINCT line_id)`, not `/ COUNT(*)`. This query's `GROUP BY` can
/// combine BOTH several half-hourly sub-buckets from one line AND several
/// lines into a single output row, so `COUNT(*)` here would count
/// half-hour rows, not lines, and would under-count the true per-line
/// average whenever a bucket legitimately aggregates several half-hours of
/// real coverage from one line. Dividing by the number of distinct lines
/// keeps the same "average coverage per contributing line" meaning
/// `daily_stats_for_range_multi`'s doc comment describes, without
/// double-penalizing wider sub-daily buckets.
pub async fn sub_daily_stats_for_range_multi(
    pool: &PgPool,
    line_ids: &[String],
    from: chrono::DateTime<chrono::Utc>,
    to: chrono::DateTime<chrono::Utc>,
    bucket_minutes: i64,
) -> Result<Vec<HalfHourlyStatsRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT
            date_bin($4 * INTERVAL '1 minute', half_hour_start, TIMESTAMPTZ '2000-01-01T00:00:00Z') AS half_hour_start,
            (SUM(sample_cycles) / GREATEST(COUNT(DISTINCT line_id), 1))::bigint AS sample_cycles,
            SUM(total)::bigint AS total,
            SUM(delayed)::bigint AS delayed,
            SUM(cancelled)::bigint AS cancelled,
            SUM(skipped)::bigint AS skipped,
            SUM(running_count)::bigint AS running_count,
            SUM(delay_minutes_sum)::double precision AS delay_minutes_sum
         FROM line_status_half_hourly_stats
         WHERE line_id = ANY($1) AND half_hour_start BETWEEN $2 AND $3
         GROUP BY 1
         ORDER BY 1",
    )
    .bind(line_ids)
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
///
/// PRIVACY: this returns EVERY matching `line_status` row, private
/// custom-line rows included -- it is deliberately an ungated read, like
/// every other query in this module. Any caller that renders these rows to
/// an HTTP client MUST first put them through
/// [`crate::data::custom_lines::retain_readable_custom_rows`]; shipping
/// this result straight to a response is exactly the disclosure described
/// in
/// docs/superpowers/specs/2026-09-16-custom-lines-in-incident-archive-filter-research.md
/// §5c.
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

/// Deliberately lighter than `IncidentRow` -- no `description`, no
/// `validity_periods`. See
/// docs/superpowers/specs/2026-09-12-incident-archive-design.md Decision 7:
/// a list row that may render dozens per page has no use for either field,
/// and `description`'s raw HTML would otherwise force every list-rendering
/// call site to sanitize it for nothing.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentSummaryRow {
    pub incident_id: String,
    pub summary: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    /// Catalogue line ids this incident matched, per `common::matcher`.
    /// Returned alongside the row (not just filtered on) so the archive's
    /// list can show *why* a row came back for a given Line filter.
    ///
    /// The column is nullable ("never computed" -- see the migration) but
    /// this field is not: the query coalesces, since a *reader* has nothing
    /// useful to do with the distinction and every consumer would otherwise
    /// have to unwrap it. Operational checks query the column directly.
    pub affected_lines: Vec<String>,
    pub priority: i32,
    pub is_planned: bool,
    pub is_cleared: bool,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
}

/// Keyset cursor for `search_incidents`, matching
/// `CallingPointDepartureCursor`'s own shape and rationale exactly (see
/// that struct's doc comment) but over `(first_seen_at DESC, incident_id
/// DESC)` instead of `(scheduled, train_uid)`. `routes::incidents` encodes
/// this onto the wire and parses it back; nothing outside that module
/// should construct one from user input directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncidentSearchCursor {
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub incident_id: String,
}

/// One page of incident-archive search results. `next_cursor` is `Some`
/// only when there is genuinely at least one more row (the query fetches
/// `limit + 1` to know that) -- same convention as
/// `CallingPointDeparturePage`.
#[derive(Debug, Clone)]
pub struct IncidentSearchPage {
    pub results: Vec<IncidentSummaryRow>,
    pub next_cursor: Option<IncidentSearchCursor>,
}

/// The incident archive's one read: a keyset-paginated, dynamically
/// filtered scan of `incidents`, ordered newest-`first_seen_at`-first with
/// ties broken by `incident_id` descending -- see
/// docs/superpowers/specs/2026-09-12-incident-archive-design.md Decision 4.
/// Modelled directly on `search_schedule_calling_point_departures`'s
/// "`fetch = limit + 1`, one extra row to detect `has_more`,
/// `($n::type IS NULL OR condition)` per optional filter, keyset tuple
/// comparison in `WHERE`, matching `ORDER BY`" shape.
///
/// **Unlike that function, this one never returns `Ok(None)`.** There is
/// no "has this day been published yet" concept for `incidents` -- an
/// unfiltered request that matches nothing (or a filter combination that
/// matches nothing) is `Ok` with an empty `results` Vec, always a `200`
/// with an empty array at the route layer, never a `404`.
///
/// `line` is a catalogue line id, already validated against the catalogue
/// by `routes::incidents` (this function has no knowledge of line
/// catalogues), or `None` for "no line filter". It is matched against
/// `incidents.affected_lines`, which `upsert_incidents` fills from
/// `common::matcher` -- the same matcher that decides which lines report
/// the incident on the live status pages.
///
/// It used to be a *station* list instead (the line's own CRS codes,
/// overlap-matched against `incidents.affected_stations`). That filter
/// returned zero rows for every line in production, because nothing ever
/// writes `affected_stations`: RDM's Incidents XML carries no CRS field,
/// only free-text `RoutesAffected`. See
/// docs/superpowers/specs/2026-09-16-tfl-incident-archive-design.md 1c.
/// Line ids are also a strictly better filter than station overlap would
/// have been even if the column were populated, since the matcher's
/// `KeywordOnly`/`OperatorOnly` tiers -- which station overlap could never
/// see -- are how most real incidents are attributed to a line.
#[allow(clippy::too_many_arguments)]
pub async fn search_incidents(
    pool: &PgPool,
    operators: Option<Vec<String>>,
    line: Option<String>,
    is_planned: Option<bool>,
    is_cleared: Option<bool>,
    priority_min: Option<i32>,
    priority_max: Option<i32>,
    first_seen_from: Option<chrono::DateTime<chrono::Utc>>,
    first_seen_to: Option<chrono::DateTime<chrono::Utc>>,
    after: Option<&IncidentSearchCursor>,
    limit: i64,
) -> Result<IncidentSearchPage> {
    let fetch = limit.saturating_add(1);

    let rows: Vec<IncidentSummaryRow> = sqlx::query_as(
        r#"
            SELECT incident_id, summary, operators, affected_stations,
                   COALESCE(affected_lines, '{}') AS affected_lines,
                   priority, is_planned, is_cleared, first_seen_at, fetched_at
            FROM incidents
            WHERE ($1::text[]      IS NULL OR operators && $1)
              AND ($2::text        IS NULL OR affected_lines @> ARRAY[$2::text])
              AND ($3::boolean     IS NULL OR is_planned = $3)
              AND ($4::boolean     IS NULL OR is_cleared = $4)
              AND ($5::integer     IS NULL OR priority >= $5)
              AND ($6::integer     IS NULL OR priority <= $6)
              AND ($7::timestamptz IS NULL OR first_seen_at >= $7)
              AND ($8::timestamptz IS NULL OR first_seen_at <= $8)
              AND ($9::timestamptz IS NULL
                   OR (first_seen_at, incident_id) < ($9, $10))
            ORDER BY first_seen_at DESC, incident_id DESC
            LIMIT $11
            "#,
    )
    .bind(operators)
    .bind(line)
    .bind(is_planned)
    .bind(is_cleared)
    .bind(priority_min)
    .bind(priority_max)
    .bind(first_seen_from)
    .bind(first_seen_to)
    .bind(after.map(|c| c.first_seen_at))
    .bind(after.map(|c| c.incident_id.as_str()))
    .bind(fetch)
    .fetch_all(pool)
    .await?;

    let has_more = rows.len() as i64 > limit;
    let mut page_rows = rows;
    if has_more {
        page_rows.truncate(limit as usize);
    }

    let next_cursor = if has_more {
        page_rows.last().map(|r| IncidentSearchCursor {
            first_seen_at: r.first_seen_at,
            incident_id: r.incident_id.clone(),
        })
    } else {
        None
    };

    Ok(IncidentSearchPage {
        results: page_rows,
        next_cursor,
    })
}

/// Every fixture incident_id in this module is prefixed `archive-test-`
/// and cleaned up by prefix, rather than day-scoped like the calling-point
/// search tests (`incidents` has no natural per-test partition key the way
/// `schedule_destination_departures` has `service_date`).
#[cfg(test)]
mod incident_search_query_tests {
    use super::*;
    use chrono::TimeZone;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    async fn delete_fixtures(pool: &PgPool) {
        sqlx::query("DELETE FROM incident_history WHERE incident_id LIKE 'archive-test-%'")
            .execute(pool)
            .await
            .expect("cleanup fixture incident_history rows");
        sqlx::query("DELETE FROM incidents WHERE incident_id LIKE 'archive-test-%'")
            .execute(pool)
            .await
            .expect("cleanup fixture incidents rows");
    }

    /// Seeds a row with an explicit `affected_lines` array, bypassing the
    /// matcher -- for tests about the *filter*, as distinct from
    /// `line_filter_finds_an_incident_ingested_the_way_the_poller_ingests_one`
    /// below, which is about the filter and the write path agreeing.
    async fn seed_incident_with_lines(
        pool: &PgPool,
        incident_id: &str,
        operators: &[&str],
        affected_lines: &[&str],
        first_seen_at: chrono::DateTime<chrono::Utc>,
    ) {
        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, \
                 affected_lines, priority, is_planned, is_cleared, first_seen_at) \
             VALUES ($1, $2, '', $3, '{}', $4, 1, false, false, $5)",
        )
        .bind(incident_id)
        .bind(format!("Fixture incident {incident_id}"))
        .bind(operators)
        .bind(affected_lines)
        .bind(first_seen_at)
        .execute(pool)
        .await
        .expect("seed fixture incidents row");
    }

    #[allow(clippy::too_many_arguments)]
    async fn seed_incident(
        pool: &PgPool,
        incident_id: &str,
        operators: &[&str],
        affected_stations: &[&str],
        priority: i32,
        is_planned: bool,
        is_cleared: bool,
        first_seen_at: chrono::DateTime<chrono::Utc>,
    ) {
        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, priority, \
                 is_planned, is_cleared, first_seen_at) \
             VALUES ($1, $2, '', $3, $4, $5, $6, $7, $8)",
        )
        .bind(incident_id)
        .bind(format!("Fixture incident {incident_id}"))
        .bind(operators)
        .bind(affected_stations)
        .bind(priority)
        .bind(is_planned)
        .bind(is_cleared)
        .bind(first_seen_at)
        .execute(pool)
        .await
        .expect("seed fixture incidents row");
    }

    fn at(hour: u32) -> chrono::DateTime<chrono::Utc> {
        // Every fixture timestamp lands on a fixed far-future day so ties
        // and ordering are exact and reproducible, mirroring
        // `schedule_destination_departures_query_tests::fixture_date`'s own
        // "far future, deterministic" rationale.
        chrono::Utc
            .with_ymd_and_hms(2099, 1, 1, hour, 0, 0)
            .unwrap()
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_with_no_filters_returns_every_row_newest_first_ties_broken_by_incident_id_desc()
     {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(
            &pool,
            "archive-test-1",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;
        // Two incidents sharing the SAME first_seen_at -- the tiebreak this
        // test exists to prove.
        seed_incident(
            &pool,
            "archive-test-2",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(10),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-3",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(10),
        )
        .await;

        let page = search_incidents(
            &pool, None, None, None, None, None, None, None, None, None, 100,
        )
        .await
        .expect("search");

        let ids: Vec<&str> = page
            .results
            .iter()
            .map(|r| r.incident_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["archive-test-3", "archive-test-2", "archive-test-1"],
            "newest first_seen_at first; a tie at the same first_seen_at breaks on \
             incident_id descending: {ids:?}"
        );
        assert!(page.next_cursor.is_none());
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_operator_filter_matches_on_overlap_not_exact_match() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(
            &pool,
            "archive-test-a",
            &["VT", "SW"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-b",
            &["GW"],
            &["PAD"],
            1,
            false,
            false,
            at(9),
        )
        .await;

        let page = search_incidents(
            &pool,
            Some(vec!["SW".to_string()]),
            None,
            None,
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

        let ids: Vec<&str> = page
            .results
            .iter()
            .map(|r| r.incident_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["archive-test-a"],
            "an incident with operators {{VT, SW}} matches a request for SW alone: {ids:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_line_filter_matches_affected_lines_and_excludes_other_lines() {
        // The filter primitive, in both directions. `archive-test-d` is the
        // false-positive guard: same operator, different line, must not
        // come back -- the operator and line filters stay independent.
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident_with_lines(
            &pool,
            "archive-test-c",
            &["XR"],
            &["elizabeth-line", "elizabeth-shenfield"],
            at(9),
        )
        .await;
        seed_incident_with_lines(
            &pool,
            "archive-test-d",
            &["XR"],
            &["elizabeth-heathrow"],
            at(9),
        )
        .await;
        // The shape every production row had before this column existed.
        seed_incident_with_lines(&pool, "archive-test-e", &["XR"], &[], at(9)).await;

        let page = search_incidents(
            &pool,
            None,
            Some("elizabeth-line".to_string()),
            None,
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

        let ids: Vec<&str> = page
            .results
            .iter()
            .map(|r| r.incident_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["archive-test-c"],
            "only the incident whose affected_lines contains the filtered line comes back -- \
             not a sibling line on the same operator, and not an unattributed row: {ids:?}"
        );
        assert_eq!(
            page.results[0].affected_lines,
            vec![
                "elizabeth-line".to_string(),
                "elizabeth-shenfield".to_string()
            ],
            "the row carries its full line list back out, not just the filtered one"
        );
        delete_fixtures(&pool).await;
    }

    /// **The regression test for the reported defect.** Not a filter unit
    /// test: it drives the real write path (`upsert_incidents`, what
    /// `poller-incidents` POSTs into) with a real Knowledgebase incident --
    /// free-text route description, `operators = ["XR"]`, and no station
    /// codes, because RDM's Incidents XML has no field to carry any -- and
    /// then asks the archive's Line filter for it. Before the fix this
    /// returned zero rows for every line on the network. See
    /// docs/superpowers/specs/2026-09-16-tfl-incident-archive-design.md 1c.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn line_filter_finds_an_incident_ingested_the_way_the_poller_ingests_one() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;

        let lines_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        let lines =
            common::LineDefinition::from_dir(&lines_dir).expect("lines/ directory should parse");
        let matcher = common::matcher::LineMatcher::new(&lines);
        // Never opens a socket unless a publish is attempted, and
        // `upsert_incidents` logs-and-continues when it cannot connect --
        // same placeholder this crate's route tests use.
        let redis =
            redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url");

        let incident = IncidentMessage {
            incident_id: "archive-test-elizabeth".to_string(),
            summary: "Residual disruption to Elizabeth line services between Shenfield and \
                      Romford"
                .to_string(),
            description: "Following an earlier fault with the signalling system between \
                          Shenfield and Romford, all lines have now reopened."
                .to_string(),
            operators: vec!["XR".to_string()],
            affected_stations: vec![],
            priority: 2,
            validity: vec![],
            is_planned: false,
            is_cleared: true,
        };

        let upserted = upsert_incidents(&pool, &redis, &matcher, std::slice::from_ref(&incident))
            .await
            .expect("upsert the incident the way the poller does");
        assert_eq!(upserted, 1);

        let page = search_incidents(
            &pool,
            None,
            Some("elizabeth-line".to_string()),
            None,
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

        let ids: Vec<&str> = page
            .results
            .iter()
            .map(|r| r.incident_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["archive-test-elizabeth"],
            "an Elizabeth line incident ingested through the real write path must be findable \
             through the archive's Line filter: {ids:?}"
        );

        // And it must not leak into an unrelated line's archive.
        let other = search_incidents(
            &pool,
            None,
            Some("wcml".to_string()),
            None,
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
            other
                .results
                .iter()
                .all(|r| r.incident_id != "archive-test-elizabeth"),
            "an Elizabeth line incident must not appear under the West Coast Main Line"
        );

        // The ON CONFLICT DO UPDATE half of the write path, which the first
        // upsert above cannot reach. The poller re-sends the whole feed
        // every cycle and an incident's text is routinely edited in place,
        // so a stale `affected_lines` here would mean the archive keeps
        // filing an incident under a line it no longer describes -- the
        // same class of wrong answer this whole change is fixing.
        let mut edited = incident.clone();
        edited.summary =
            "Delays to Avanti West Coast services between Euston and Crewe".to_string();
        edited.description =
            "A fault with the signalling system on the West Coast Main Line.".to_string();
        edited.operators = vec!["VT".to_string()];
        upsert_incidents(&pool, &redis, &matcher, std::slice::from_ref(&edited))
            .await
            .expect("re-upsert the edited incident");

        let after_edit = search_incidents(
            &pool,
            None,
            Some("elizabeth-line".to_string()),
            None,
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
            after_edit
                .results
                .iter()
                .all(|r| r.incident_id != "archive-test-elizabeth"),
            "after the text was edited to describe a different railway, the row must no longer \
             be filed under the Elizabeth line"
        );

        let moved = search_incidents(
            &pool,
            None,
            Some("wcml".to_string()),
            None,
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
            moved
                .results
                .iter()
                .any(|r| r.incident_id == "archive-test-elizabeth"),
            "...and must now be filed under the line its new text describes"
        );

        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn backfill_fills_affected_lines_for_a_row_written_before_the_column_existed() {
        // The other half of the fix: rows already in the table. Seeded with
        // affected_lines left NULL -- exactly the state the migration leaves
        // all 1507 production rows in -- then recomputed.
        //
        // NOTE: `run_backfill` is deliberately whole-table, so this test
        // rewrites `affected_lines` on every row in the target database,
        // not just its own `archive-test-%` fixtures, and `delete_fixtures`
        // cannot undo that. Harmless against a throwaway CI database (the
        // values it writes are the correct ones); do not point DATABASE_URL
        // at a copy of production and expect it untouched.
        let pool = test_pool().await;
        delete_fixtures(&pool).await;

        sqlx::query(
            "INSERT INTO incidents \
                (incident_id, summary, description, operators, affected_stations, \
                 priority, validity_periods, is_planned, is_cleared, \
                 first_seen_at) \
             VALUES ($1, $2, $3, $4, '{}', 2, '[]'::jsonb, false, true, $5)",
        )
        .bind("archive-test-backfill")
        .bind("Residual disruption to Elizabeth line services between Shenfield and Romford")
        .bind("Following an earlier fault with the signalling system, all lines have reopened.")
        .bind(vec!["XR".to_string()])
        .bind(at(9))
        .execute(&pool)
        .await
        .expect("seed a pre-column row");

        let lines_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../lines");
        let lines =
            common::LineDefinition::from_dir(&lines_dir).expect("lines/ directory should parse");
        let matcher = common::matcher::LineMatcher::new(&lines);

        let report = crate::data::incident_line_backfill::run_backfill(&pool, &matcher)
            .await
            .expect("backfill");
        assert!(
            report.rows_updated >= 1,
            "the seeded row should have been updated: {report:?}"
        );
        assert!(
            report.rows_never_computed >= 1,
            "the seeded row had a NULL affected_lines and must be counted as never computed, \
             which is how an operator tells an outstanding backfill from a completed one: \
             {report:?}"
        );

        let page = search_incidents(
            &pool,
            None,
            Some("elizabeth-line".to_string()),
            None,
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
            page.results
                .iter()
                .any(|r| r.incident_id == "archive-test-backfill"),
            "the backfilled row must now be reachable through the Line filter"
        );

        // Idempotence: a second run finds nothing to do at all, and now
        // sees no never-computed rows, since the first run left every row
        // with a non-NULL array.
        let second = crate::data::incident_line_backfill::run_backfill(&pool, &matcher)
            .await
            .expect("second backfill");
        assert_eq!(
            second.rows_updated, 0,
            "re-running the backfill must be a no-op: {second:?}"
        );
        assert_eq!(
            second.rows_never_computed, 0,
            "after a completed run nothing is left uncomputed: {second:?}"
        );

        delete_fixtures(&pool).await;
    }

    /// The guard that stops a mis-set `LINES_DIR` turning the backfill into
    /// a mass-erase. It lives in `run_backfill` itself, not only in the
    /// binary, so this can assert it without going near the binary's
    /// argument handling.
    ///
    /// Deliberately NOT `#[ignore]`d, unlike every other test in this
    /// module: the guard returns before the pool is ever touched, so
    /// `connect_lazy` (which opens no socket -- the same trick `auth.rs`'s
    /// tests use) is enough, and a guard against erasing a column is worth
    /// having run on every `cargo test`, not only on the rare live-database
    /// pass. If this ever starts needing a real connection, that means the
    /// guard has moved after the first query and the test has caught a
    /// regression.
    #[tokio::test]
    async fn backfill_refuses_to_run_against_an_empty_line_catalogue() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://placeholder@127.0.0.1:0/placeholder")
            .expect("parse placeholder database url");
        let empty = common::matcher::LineMatcher::new(&[]);

        let err = crate::data::incident_line_backfill::run_backfill(&pool, &empty)
            .await
            .expect_err("an empty catalogue must be refused, not silently applied");
        assert!(
            err.to_string().contains("empty line catalogue"),
            "the error must say why: {err}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_from_to_bounds_are_inclusive() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(
            &pool,
            "archive-test-e",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(8),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-f",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-g",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(10),
        )
        .await;

        let page = search_incidents(
            &pool,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(at(8)),
            Some(at(9)),
            None,
            100,
        )
        .await
        .expect("search");

        let ids: Vec<&str> = page
            .results
            .iter()
            .map(|r| r.incident_id.as_str())
            .collect();
        assert_eq!(
            ids,
            vec!["archive-test-f", "archive-test-e"],
            "both bounds are inclusive; archive-test-g (hour 10) must be excluded: {ids:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_planned_and_cleared_filters() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(
            &pool,
            "archive-test-h",
            &["VT"],
            &["WAT"],
            1,
            true,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-i",
            &["VT"],
            &["WAT"],
            1,
            false,
            true,
            at(9),
        )
        .await;

        let planned_only = search_incidents(
            &pool,
            None,
            None,
            Some(true),
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
        assert_eq!(
            planned_only
                .results
                .iter()
                .map(|r| r.incident_id.as_str())
                .collect::<Vec<_>>(),
            vec!["archive-test-h"]
        );

        let cleared_only = search_incidents(
            &pool,
            None,
            None,
            None,
            Some(true),
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");
        assert_eq!(
            cleared_only
                .results
                .iter()
                .map(|r| r.incident_id.as_str())
                .collect::<Vec<_>>(),
            vec!["archive-test-i"]
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_priority_range_is_inclusive() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(
            &pool,
            "archive-test-j",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-k",
            &["VT"],
            &["WAT"],
            2,
            false,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-l",
            &["VT"],
            &["WAT"],
            3,
            false,
            false,
            at(9),
        )
        .await;

        let page = search_incidents(
            &pool,
            None,
            None,
            None,
            None,
            Some(2),
            Some(2),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");

        assert_eq!(
            page.results
                .iter()
                .map(|r| r.incident_id.as_str())
                .collect::<Vec<_>>(),
            vec!["archive-test-k"]
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_combines_filters_with_and_semantics() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        // Matches operator AND planned AND priority range:
        seed_incident(
            &pool,
            "archive-test-m",
            &["VT"],
            &["WAT"],
            5,
            true,
            false,
            at(9),
        )
        .await;
        // Fails on operator only:
        seed_incident(
            &pool,
            "archive-test-n",
            &["GW"],
            &["WAT"],
            5,
            true,
            false,
            at(9),
        )
        .await;
        // Fails on planned only:
        seed_incident(
            &pool,
            "archive-test-o",
            &["VT"],
            &["WAT"],
            5,
            false,
            false,
            at(9),
        )
        .await;
        // Fails on priority range only:
        seed_incident(
            &pool,
            "archive-test-p",
            &["VT"],
            &["WAT"],
            1,
            true,
            false,
            at(9),
        )
        .await;

        let page = search_incidents(
            &pool,
            Some(vec!["VT".to_string()]),
            None,
            Some(true),
            None,
            Some(4),
            Some(6),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");

        assert_eq!(
            page.results
                .iter()
                .map(|r| r.incident_id.as_str())
                .collect::<Vec<_>>(),
            vec!["archive-test-m"],
            "only the row matching every filter simultaneously must be returned"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_keyset_pagination_pages_without_gaps_or_repeats_and_breaks_ties_on_incident_id_desc()
     {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        // Five incidents, three of them sharing one first_seen_at, forcing
        // the tiebreak to matter mid-pagination.
        seed_incident(
            &pool,
            "archive-test-q1",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-q2",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-q3",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-r",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(8),
        )
        .await;
        seed_incident(
            &pool,
            "archive-test-s",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(10),
        )
        .await;

        let mut cursor: Option<IncidentSearchCursor> = None;
        let mut collected: Vec<String> = Vec::new();
        loop {
            let page = search_incidents(
                &pool,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                cursor.as_ref(),
                1,
            )
            .await
            .expect("search");
            assert_eq!(
                page.results.len(),
                1,
                "limit=1 must return exactly one row per page"
            );
            collected.push(page.results[0].incident_id.clone());
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        assert_eq!(
            collected,
            vec![
                "archive-test-s",
                "archive-test-q3",
                "archive-test-q2",
                "archive-test-q1",
                "archive-test-r",
            ],
            "no gaps, no repeats, tie broken by incident_id descending: {collected:?}"
        );
        delete_fixtures(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                incident_search_query_tests -- --ignored --test-threads=1`"]
    async fn search_incidents_with_no_matches_returns_ok_with_an_empty_vec_never_none() {
        let pool = test_pool().await;
        delete_fixtures(&pool).await;
        seed_incident(
            &pool,
            "archive-test-t",
            &["VT"],
            &["WAT"],
            1,
            false,
            false,
            at(9),
        )
        .await;

        let page = search_incidents(
            &pool,
            Some(vec!["ZZ".to_string()]),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search never fails on an unmatched filter -- there is no 404 concept here");

        assert!(page.results.is_empty());
        assert!(page.next_cursor.is_none());
        delete_fixtures(&pool).await;
    }
}

// --- Movement Events Queries ---

/// One `train_movement_events` row for one `trains_id` -- the per-stop live
/// overlay source
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

/// EVERY retained movement event for one train, oldest-`received_at` first
/// -- deliberately NOT collapsed to one row per location.
///
/// This used to be `latest_movement_event_per_location`, a `DISTINCT ON
/// (UPPER(loc_crs)) ... ORDER BY received_at DESC` that kept exactly one
/// event per CRS. That collapse silently corrupted every journey which
/// visits one station more than once -- the design doc's own §5 Decision 2
/// named it ("a location visited twice in one journey ... collapses to its
/// single latest-reported event. Not solved here") and wrote it off as "a
/// real but rare CIF anomaly". It is neither rare nor an anomaly: every
/// circular service is shaped that way. South Western Railway's Kingston
/// Loop (`lines/swr-kingston-loop.toml`) departs London Waterloo and
/// terminates back at London Waterloo, calling at Vauxhall and Clapham
/// Junction twice each on the way round -- and the collapse smeared the
/// TERMINUS's arrival back onto the ORIGIN row, so the train's first stop
/// claimed an actual arrival 80 minutes after it left.
///
/// Splitting the rows back out into per-visit groups needs every event, in
/// a stable order, so that is what this returns;
/// `journey::assign_events_to_stops` owns the (now sequence-aware)
/// assignment. `received_at ASC, id ASC` -- `id` breaks ties
/// deterministically, because two events of one batch can share a
/// `received_at` to the microsecond and the old query's "latest received
/// wins" rule (preserved per visit group, see that function) needs a total
/// order to be reproducible.
pub async fn movement_events_for_train(
    pool: &PgPool,
    trains_id: i64,
) -> Result<Vec<MovementEventRow>> {
    let rows = sqlx::query_as::<_, MovementEventRow>(
        "SELECT UPPER(loc_crs) AS loc_crs, event_type, \
                planned_timestamp, actual_timestamp, variation_status \
         FROM train_movement_events \
         WHERE trains_id = $1 AND loc_crs IS NOT NULL \
         ORDER BY received_at ASC, id ASC",
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
///
/// Keys are `UPPER(TRIM(...))` on both the input and the column, matching
/// this file's single TIPLOC/CRS normalization convention (see
/// `list_stanox_crs_for_crs`'s doc comment).
pub async fn station_names_for_crs_batch(
    pool: &PgPool,
    crs_codes: &[String],
) -> Result<HashMap<String, String>> {
    if crs_codes.is_empty() {
        return Ok(HashMap::new());
    }
    let upper: Vec<String> = crs_codes.iter().map(|c| c.trim().to_uppercase()).collect();
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT UPPER(TRIM(crs)), name FROM stations WHERE UPPER(TRIM(crs)) = ANY($1)",
    )
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

    // `is_bookable_crs`'s own tests -- real TIPLOC/CRS pairs reused from
    // that function's own doc comment (the `Y80908`/Hanslope Junction
    // incident this filter exists for).
    #[test]
    fn is_bookable_crs_rejects_only_the_x_prefixed_convention() {
        assert!(is_bookable_crs("WAT"));
        assert!(is_bookable_crs("EUS"));
        assert!(!is_bookable_crs("XHN"));
        assert!(!is_bookable_crs("XOZ"));
        assert!(!is_bookable_crs("XVR"));
        // A real CRS that merely happens to CONTAIN an 'X' is unaffected --
        // only a LEADING 'X' is Network Rail's pseudo-code convention.
        assert!(is_bookable_crs("BOX"));
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

    /// Regression test for the Signal Box Audit Low finding on
    /// `upsert_tfl_line_status`: `line_id` is only `TEXT PRIMARY KEY`, so
    /// nothing at the schema level stops a TfL line id from colliding with
    /// an `aggregator`-owned one. Before the ownership guard, a colliding
    /// TfL post would silently `ON CONFLICT (line_id) DO UPDATE SET ...
    /// source = 'tfl'`, stealing the aggregator's row -- this proves it
    /// now fails loudly (`Err`, whole batch rolled back by the caller
    /// never committing) and leaves the aggregator's row untouched.
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_tfl_line_status_refuses_to_steal_a_non_tfl_owned_row_with_the_same_line_id \
                -- --ignored`"]
    async fn upsert_tfl_line_status_refuses_to_steal_a_non_tfl_owned_row_with_the_same_line_id() {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES ('TEST-COLLIDE', 'aggregator owns this', 'national-rail', '{NT}', '[]', \
                     'aggregator') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source, \
                                                  name = EXCLUDED.name",
        )
        .execute(&pool)
        .await
        .expect("seed a non-TfL-owned row under the colliding line_id");

        let colliding_report = common::LineStatusReport {
            id: "TEST-COLLIDE".to_string(),
            name: "a TfL line that happens to share this id".to_string(),
            mode_name: "tube".to_string(),
            operators: vec!["TfL".to_string()],
            statuses: vec![],
        };

        let result = upsert_tfl_line_status(&pool, std::slice::from_ref(&colliding_report)).await;
        assert!(
            result.is_err(),
            "an id collision with a non-TfL-owned row must fail loudly, not silently overwrite"
        );

        let (source, name): (String, String) =
            sqlx::query_as("SELECT source, name FROM line_status WHERE line_id = 'TEST-COLLIDE'")
                .fetch_one(&pool)
                .await
                .expect("the aggregator's row must still exist, untouched");
        assert_eq!(
            source, "aggregator",
            "ownership must not have been stolen by the refused TfL write"
        );
        assert_eq!(
            name, "aggregator owns this",
            "the aggregator's own data must not have been overwritten by the refused TfL write"
        );

        sqlx::query("DELETE FROM line_status WHERE line_id = 'TEST-COLLIDE'")
            .execute(&pool)
            .await
            .expect("cleanup fixture row");
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
            change_time_minutes: None,
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
            change_time_minutes: None,
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                daily_stats_for_range_multi -- --ignored`"]
    async fn daily_stats_for_range_multi_sums_across_lines_and_excludes_others() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        sqlx::query(
            "INSERT INTO line_status_daily_stats \
                (line_id, day, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-MULTI-A', '2026-08-01', 10, 100, 5, 1, 2, 97, 120.0), \
                ('TEST-MULTI-B', '2026-08-01', 8, 80, 3, 0, 1, 79, 60.0), \
                ('TEST-MULTI-OTHER', '2026-08-01', 20, 200, 20, 20, 20, 160, 500.0) \
             ON CONFLICT (line_id, day) DO UPDATE SET total = EXCLUDED.total",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let from = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let line_ids = vec!["TEST-MULTI-A".to_string(), "TEST-MULTI-B".to_string()];
        let rows = daily_stats_for_range_multi(&pool, &line_ids, from, to)
            .await
            .expect("daily_stats_for_range_multi");

        sqlx::query("DELETE FROM line_status_daily_stats WHERE line_id LIKE 'TEST-MULTI-%'")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        assert_eq!(
            rows.len(),
            1,
            "one row per day, not one per contributing line"
        );
        let row = &rows[0];
        assert_eq!(row.total, 180, "100 + 80, TEST-MULTI-OTHER excluded");
        assert_eq!(row.delayed, 8);
        // sample_cycles is normalized to the average per contributing line,
        // not a raw sum (Finding 1 of the Phase 4 final-review pass): both
        // TEST-MULTI-A and TEST-MULTI-B have a row for this day, so
        // COUNT(*) = 2, and (10 + 8) / 2 = 9 -- not the raw sum, 18.
        assert_eq!(row.sample_cycles, 9);
        assert_eq!(row.delay_minutes_sum, 180.0);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                daily_stats_for_range_multi_an_empty_line_id_set -- --ignored`"]
    async fn daily_stats_for_range_multi_an_empty_line_id_set_returns_empty_not_an_error() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let from = chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap();
        let to = chrono::NaiveDate::from_ymd_opt(2026, 8, 31).unwrap();
        let rows = daily_stats_for_range_multi(&pool, &[], from, to)
            .await
            .expect("daily_stats_for_range_multi with no line ids");
        assert!(rows.is_empty());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                half_hourly_stats_for_range_multi -- --ignored`"]
    async fn half_hourly_stats_for_range_multi_sums_across_lines() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let bucket: chrono::DateTime<chrono::Utc> = "2026-08-01T12:00:00Z".parse().unwrap();
        sqlx::query(
            "INSERT INTO line_status_half_hourly_stats \
                (line_id, half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-HH-MULTI-A', $1, 5, 50, 2, 0, 1, 49, 30.0), \
                ('TEST-HH-MULTI-B', $1, 4, 40, 1, 0, 0, 40, 10.0) \
             ON CONFLICT (line_id, half_hour_start) DO UPDATE SET total = EXCLUDED.total",
        )
        .bind(bucket)
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let line_ids = vec!["TEST-HH-MULTI-A".to_string(), "TEST-HH-MULTI-B".to_string()];
        let rows = half_hourly_stats_for_range_multi(
            &pool,
            &line_ids,
            bucket - chrono::Duration::minutes(30),
            bucket + chrono::Duration::minutes(30),
        )
        .await
        .expect("half_hourly_stats_for_range_multi");

        sqlx::query(
            "DELETE FROM line_status_half_hourly_stats WHERE line_id LIKE 'TEST-HH-MULTI-%'",
        )
        .execute(&pool)
        .await
        .expect("cleanup fixture rows");

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total, 90);
        assert_eq!(rows[0].delayed, 3);
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                sub_daily_stats_for_range_multi -- --ignored`"]
    async fn sub_daily_stats_for_range_multi_groups_by_bucket_and_sums_across_lines() {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres");

        let first: chrono::DateTime<chrono::Utc> = "2026-08-01T12:00:00Z".parse().unwrap();
        let second: chrono::DateTime<chrono::Utc> = "2026-08-01T12:30:00Z".parse().unwrap();
        sqlx::query(
            "INSERT INTO line_status_half_hourly_stats \
                (line_id, half_hour_start, sample_cycles, total, delayed, cancelled, skipped, running_count, delay_minutes_sum) \
             VALUES \
                ('TEST-SUBDAY-MULTI-A', $1, 5, 50, 2, 0, 1, 49, 30.0), \
                ('TEST-SUBDAY-MULTI-B', $2, 4, 40, 1, 0, 0, 40, 10.0) \
             ON CONFLICT (line_id, half_hour_start) DO UPDATE SET total = EXCLUDED.total",
        )
        .bind(first)
        .bind(second)
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let line_ids = vec![
            "TEST-SUBDAY-MULTI-A".to_string(),
            "TEST-SUBDAY-MULTI-B".to_string(),
        ];
        let rows = sub_daily_stats_for_range_multi(
            &pool,
            &line_ids,
            first - chrono::Duration::minutes(30),
            second + chrono::Duration::minutes(30),
            60,
        )
        .await
        .expect("sub_daily_stats_for_range_multi");

        sqlx::query(
            "DELETE FROM line_status_half_hourly_stats WHERE line_id LIKE 'TEST-SUBDAY-MULTI-%'",
        )
        .execute(&pool)
        .await
        .expect("cleanup fixture rows");

        // Both half-hourly rows fall in the same 1-hour bucket (12:00-13:00) --
        // one combined row, both lines' contributions summed together.
        assert_eq!(rows.len(), 1, "both rows fall in the same 1-hour bucket");
        assert_eq!(rows[0].total, 90);
        assert_eq!(rows[0].delayed, 3);
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
                    change_time_minutes: None,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-WAT".to_string(),
                    crs: "WAT".to_string(),
                    tiploc: "WATRLMN".to_string(),
                    station_name: "LONDON WATERLOO".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
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
                change_time_minutes: None,
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
                crs_for_tiploc_uppercases_a_lower_case_stored_crs_matching_the_batch_sibling \
                -- --ignored --test-threads=1`"]
    async fn crs_for_tiploc_uppercases_a_lower_case_stored_crs_matching_the_batch_sibling() {
        // Neither `upsert_stanox_crs` nor `upsert_tiploc_crs` case-normalizes
        // `crs` on write (see `crs_for_tiploc`'s own doc comment) -- this
        // seeds a lower-case `crs` directly to prove `crs_for_tiploc` itself
        // uppercases on read, the same defense `crs_for_tiplocs_batch`
        // already applies via its own `UPPER(crs)` projection. Before this
        // fix, `crs_for_tiploc` returned the bare `"xvr"` here, which would
        // silently defeat `is_bookable_crs`'s case-sensitive
        // `starts_with('X')` check at this function's real
        // `find_schedule_match` call site.
        let pool = test_pool().await;
        upsert_stanox_crs(
            &pool,
            &[common::StanoxCrsRecord {
                stanox: "TEST-LOWER-XVR".to_string(),
                crs: "xvr".to_string(),
                tiploc: "TEST-LOWER-VICTRCR".to_string(),
                station_name: "VICTORIA CARRIAGE ROAD".to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            }],
        )
        .await
        .expect("seed stanox_crs");

        assert_eq!(
            crs_for_tiploc(&pool, "test-lower-victrcr").await.unwrap(),
            Some("XVR".to_string()),
            "crs_for_tiploc must uppercase a lower-case-stored crs, matching \
             crs_for_tiplocs_batch's own UPPER(crs) projection"
        );

        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-LOWER-XVR'")
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
                    change_time_minutes: None,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-JS-EUS".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "TEST-JS-EUSTON".to_string(),
                    station_name: "EUSTON".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_fixed_links -- --ignored --test-threads=1`"]
    async fn upsert_fixed_links_replaces_the_whole_table_each_call() {
        let pool = test_pool().await;
        let first = vec![common::FixedLinkRecord {
            mode: "TUBE".to_string(),
            from_crs: "EUS".to_string(),
            to_crs: "KGX".to_string(),
            minutes: 5,
            valid_from: "0500".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111100".to_string(),
            source_sequence: 1,
        }];
        upsert_fixed_links(&pool, &first)
            .await
            .expect("first publish");
        let after_first = list_fixed_links_from_crs(&pool, "EUS")
            .await
            .expect("read back");
        assert_eq!(after_first.len(), 1);

        let second = vec![common::FixedLinkRecord {
            mode: "TRANSFER".to_string(),
            from_crs: "EUS".to_string(),
            to_crs: "STP".to_string(),
            minutes: 15,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
            source_sequence: 2,
        }];
        upsert_fixed_links(&pool, &second)
            .await
            .expect("second publish replaces");
        let after_second = list_fixed_links_from_crs(&pool, "EUS")
            .await
            .expect("read back");
        assert_eq!(
            after_second.len(),
            1,
            "the first cycle's EUS->KGX row must be gone -- this is a full replace, not an upsert"
        );
        assert_eq!(after_second[0].to_crs, "STP");

        sqlx::query("DELETE FROM fixed_links")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_fixed_links -- --ignored --test-threads=1`"]
    async fn upsert_fixed_links_with_an_empty_batch_does_not_wipe_the_table() {
        // Guards the one way a DELETE-then-INSERT upsert can destroy real
        // data that a per-row ON CONFLICT loop never could: a call that
        // carries no rows at all must be a no-op, NOT "delete every fixed
        // link this app knows about" -- same posture and same reason as
        // `upsert_with_an_empty_batch_does_not_wipe_the_day` (schedule_destination_departures)
        // and `upsert_schedule_calling_points_full_chunk`'s own guard.
        let pool = test_pool().await;
        let seeded = vec![common::FixedLinkRecord {
            mode: "TUBE".to_string(),
            from_crs: "EUS".to_string(),
            to_crs: "KGX".to_string(),
            minutes: 5,
            valid_from: "0500".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111100".to_string(),
            source_sequence: 1,
        }];
        upsert_fixed_links(&pool, &seeded)
            .await
            .expect("seed a real row");

        let upserted = upsert_fixed_links(&pool, &[])
            .await
            .expect("an empty batch must not error");
        assert_eq!(upserted, 0);

        let after_empty = list_fixed_links_from_crs(&pool, "EUS")
            .await
            .expect("read back");
        assert_eq!(
            after_empty.len(),
            1,
            "an empty batch must leave the previously-published row in place, never clear it"
        );

        sqlx::query("DELETE FROM fixed_links")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                prune_stanox_crs_not_in -- --ignored --test-threads=1`"]
    async fn prune_stanox_crs_not_in_deletes_only_rows_absent_from_the_keep_set() {
        // The Signal Box Audit Low finding this closes: `upsert_stanox_crs`
        // alone never removed a STANOX a later delivery simply stopped
        // mentioning, so a stale mapping accumulated forever. This proves
        // the cleanup half on its own, independent of the ingest route.
        let pool = test_pool().await;
        upsert_stanox_crs(
            &pool,
            &[
                common::StanoxCrsRecord {
                    stanox: "TEST-PRUNE-KEEP".to_string(),
                    crs: "EUS".to_string(),
                    tiploc: "TEST-PRUNE-EUSTON".to_string(),
                    station_name: "EUSTON".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
                common::StanoxCrsRecord {
                    stanox: "TEST-PRUNE-STALE".to_string(),
                    crs: "CRE".to_string(),
                    tiploc: "TEST-PRUNE-CREWE".to_string(),
                    station_name: "CREWE".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
            ],
        )
        .await
        .expect("seed two rows");

        let deleted = prune_stanox_crs_not_in(&pool, &["TEST-PRUNE-KEEP".to_string()])
            .await
            .expect("prune");
        assert_eq!(deleted, 1);

        let remaining = list_stanox_crs_for_crs(&pool, "EUS")
            .await
            .expect("read back kept row");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].stanox, "TEST-PRUNE-KEEP");

        let gone = list_stanox_crs_for_crs(&pool, "CRE")
            .await
            .expect("read back stale row");
        assert!(
            gone.is_empty(),
            "the STANOX absent from the keep set must be gone"
        );

        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-PRUNE-%'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                prune_stanox_crs_not_in -- --ignored --test-threads=1`"]
    async fn prune_stanox_crs_not_in_with_an_empty_keep_set_is_a_no_op() {
        // Mirrors `upsert_fixed_links`'s own empty-batch guard: an empty
        // keep set must never be read as "delete everything" -- that would
        // just move the exact hazard this fix closes into the prune step
        // itself.
        let pool = test_pool().await;
        upsert_stanox_crs(
            &pool,
            &[common::StanoxCrsRecord {
                stanox: "TEST-PRUNE-EMPTY".to_string(),
                crs: "EUS".to_string(),
                tiploc: "TEST-PRUNE-EMPTY-TPL".to_string(),
                station_name: "EUSTON".to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            }],
        )
        .await
        .expect("seed a row");

        let deleted = prune_stanox_crs_not_in(&pool, &[]).await.expect("prune");
        assert_eq!(deleted, 0);

        let remaining = list_stanox_crs_for_crs(&pool, "EUS")
            .await
            .expect("read back");
        assert!(
            remaining.iter().any(|r| r.stanox == "TEST-PRUNE-EMPTY"),
            "an empty keep set must not delete the real row"
        );

        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-PRUNE-%'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                prune_tiploc_crs_not_in -- --ignored --test-threads=1`"]
    async fn prune_tiploc_crs_not_in_deletes_only_rows_absent_from_the_keep_set() {
        let pool = test_pool().await;
        upsert_tiploc_crs(
            &pool,
            &[
                common::TiplocCrsRecord {
                    tiploc: "TEST-PRUNE-TPL-KEEP".to_string(),
                    crs: "EUS".to_string(),
                    station_name: "EUSTON".to_string(),
                    stanox: "TEST-PRUNE-TPL-KEEP-STX".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
                common::TiplocCrsRecord {
                    tiploc: "TEST-PRUNE-TPL-STALE".to_string(),
                    crs: "CRE".to_string(),
                    station_name: "CREWE".to_string(),
                    stanox: "TEST-PRUNE-TPL-STALE-STX".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
            ],
        )
        .await
        .expect("seed two rows");

        let deleted = prune_tiploc_crs_not_in(&pool, &["TEST-PRUNE-TPL-KEEP".to_string()])
            .await
            .expect("prune");
        assert_eq!(deleted, 1);

        let remaining = list_tiploc_crs(&pool).await.expect("read back");
        assert!(
            remaining.iter().any(|r| r.tiploc == "TEST-PRUNE-TPL-KEEP"),
            "the kept TIPLOC must remain"
        );
        assert!(
            !remaining.iter().any(|r| r.tiploc == "TEST-PRUNE-TPL-STALE"),
            "the TIPLOC absent from the keep set must be gone"
        );

        sqlx::query("DELETE FROM tiploc_crs WHERE tiploc LIKE 'TEST-PRUNE-TPL-%'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_stanox_crs_change_time -- --ignored --test-threads=1`"]
    async fn upsert_stanox_crs_change_time_minutes_round_trips_including_none() {
        let pool = test_pool().await;
        let records = vec![
            common::StanoxCrsRecord {
                stanox: "TEST-STANOX-WITH-CHANGE-TIME".to_string(),
                crs: "ZZZ".to_string(),
                tiploc: "ZZZTPL".to_string(),
                station_name: "TEST STATION".to_string(),
                source_sequence: 1,
                change_time_minutes: Some(5),
            },
            common::StanoxCrsRecord {
                stanox: "TEST-STANOX-NO-CHANGE-TIME".to_string(),
                crs: "YYY".to_string(),
                tiploc: "YYYTPL".to_string(),
                station_name: "TEST STATION 2".to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            },
        ];
        upsert_stanox_crs(&pool, &records).await.expect("upsert");

        let all = list_stanox_crs(&pool).await.expect("read back");
        let with_time = all
            .iter()
            .find(|r| r.stanox == "TEST-STANOX-WITH-CHANGE-TIME")
            .expect("row present");
        assert_eq!(with_time.change_time_minutes, Some(5));
        let without_time = all
            .iter()
            .find(|r| r.stanox == "TEST-STANOX-NO-CHANGE-TIME")
            .expect("row present");
        assert_eq!(without_time.change_time_minutes, None);

        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-STANOX-%'")
            .execute(&pool)
            .await
            .ok();
    }

    /// The actual end-to-end proof of Task 3 of
    /// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md's
    /// union-read fix: seeds ONLY `tiploc_crs` (via `upsert_tiploc_crs`,
    /// never touching `stanox_crs` at all) with Vauxhall's two real
    /// TIPLOCs, both sharing one STANOX and CRS -- exactly the shape
    /// `stanox_crs`'s `PRIMARY KEY (stanox)` could never have represented
    /// simultaneously. Both `crs_for_tiplocs_batch` and
    /// `list_stanox_crs_for_crs` must resolve BOTH TIPLOCs even though
    /// neither ever queries `tiploc_crs` alone in this codebase -- proving
    /// the union SQL actually reads the new table, not just the old one.
    #[tokio::test]
    #[ignore = "requires a live database; see this plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                both_of_vauxhalls_tiplocs_resolve_through_the_union_when_seeded_only_in_tiploc_crs \
                -- --ignored --test-threads=1`"]
    async fn both_of_vauxhalls_tiplocs_resolve_through_the_union_when_seeded_only_in_tiploc_crs() {
        let pool = test_pool().await;
        upsert_tiploc_crs(
            &pool,
            &[
                common::TiplocCrsRecord {
                    tiploc: "TEST-VAUXHLM".to_string(),
                    crs: "VXH".to_string(),
                    station_name: "VAUXHALL".to_string(),
                    stanox: "TEST-VXH-STANOX".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
                common::TiplocCrsRecord {
                    tiploc: "TEST-VAUXHLW".to_string(),
                    crs: "VXH".to_string(),
                    station_name: "VAUXHALL".to_string(),
                    stanox: "TEST-VXH-STANOX".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
            ],
        )
        .await
        .expect("seed tiploc_crs (stanox_crs is deliberately left untouched)");

        let batch = crs_for_tiplocs_batch(
            &pool,
            &["TEST-VAUXHLM".to_string(), "TEST-VAUXHLW".to_string()],
        )
        .await
        .expect("crs_for_tiplocs_batch");
        assert_eq!(
            batch.get("TEST-VAUXHLM"),
            Some(&"VXH".to_string()),
            "TEST-VAUXHLM exists only in tiploc_crs -- must still resolve via the union"
        );
        assert_eq!(
            batch.get("TEST-VAUXHLW"),
            Some(&"VXH".to_string()),
            "TEST-VAUXHLW exists only in tiploc_crs -- must still resolve via the union"
        );

        let for_crs = list_stanox_crs_for_crs(&pool, "VXH")
            .await
            .expect("list_stanox_crs_for_crs");
        assert!(
            for_crs.iter().any(|r| r.tiploc == "TEST-VAUXHLM"),
            "TEST-VAUXHLM must be present in list_stanox_crs_for_crs('VXH')"
        );
        assert!(
            for_crs.iter().any(|r| r.tiploc == "TEST-VAUXHLW"),
            "TEST-VAUXHLW must be present in list_stanox_crs_for_crs('VXH')"
        );

        sqlx::query("DELETE FROM tiploc_crs WHERE tiploc IN ('TEST-VAUXHLM', 'TEST-VAUXHLW')")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}

/// Regression coverage for a Signal Box Audit Low finding: this file's
/// TIPLOC/CRS equality lookups used to disagree on case/whitespace
/// normalization -- some compared raw values, some `UPPER`-only, and
/// `crs_for_tiploc`/`crs_for_tiplocs_batch` already did `UPPER`+`TRIM` --
/// so two functions that both claimed to resolve "the same" CRS could
/// silently return different answers for the identical lowercase input
/// depending on which one a caller happened to call. Every lookup in this
/// file now normalizes with `UPPER(TRIM(...))` (see
/// `list_stanox_crs_for_crs`'s doc comment for the full list); these
/// tests seed one row and prove that two lookups which previously
/// disagreed -- `latest_station_sample` (used to compare with a raw `=`)
/// and `list_stanox_crs_for_crs` (already `UPPER`-only) -- now both
/// resolve the same lowercase/whitespace-padded input.
#[cfg(test)]
mod crs_tiploc_normalization_tests {
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
                latest_station_sample_and_list_stanox_crs_for_crs_agree_on_a_lowercase_query \
                -- --ignored --test-threads=1`"]
    async fn latest_station_sample_and_list_stanox_crs_for_crs_agree_on_a_lowercase_query() {
        let pool = test_pool().await;

        sqlx::query(
            "INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes, updated_at) \
             VALUES ('TEST-NORM-STANOX', 'ZNM', 'TESTNORM', 'TEST NORMALIZATION STATION', 1, NULL, NOW()) \
             ON CONFLICT (stanox) DO UPDATE SET crs = EXCLUDED.crs, tiploc = EXCLUDED.tiploc",
        )
        .execute(&pool)
        .await
        .expect("seed stanox_crs");

        sqlx::query(
            "INSERT INTO station_samples (crs, polled_at, departures) \
             VALUES ('ZNM', NOW(), '[]'::jsonb) \
             ON CONFLICT (crs) DO UPDATE SET departures = EXCLUDED.departures",
        )
        .execute(&pool)
        .await
        .expect("seed station_samples");

        // Before this fix, `latest_station_sample` compared with a raw
        // `=` while `list_stanox_crs_for_crs` already normalized with
        // `UPPER(...)` -- so a lowercase, whitespace-padded query like
        // this one would resolve through one lookup and silently miss
        // through the other. Both must now agree.
        let sample = latest_station_sample(&pool, "  znm  ")
            .await
            .expect("latest_station_sample");
        assert!(
            sample.is_some(),
            "latest_station_sample must resolve a lowercase, whitespace-padded CRS"
        );

        let stanox_rows = list_stanox_crs_for_crs(&pool, "  znm  ")
            .await
            .expect("list_stanox_crs_for_crs");
        assert!(
            stanox_rows.iter().any(|r| r.tiploc == "TESTNORM"),
            "list_stanox_crs_for_crs must resolve the same lowercase, whitespace-padded CRS"
        );

        sqlx::query("DELETE FROM station_samples WHERE crs = 'ZNM'")
            .execute(&pool)
            .await
            .expect("cleanup station_samples");
        sqlx::query("DELETE FROM stanox_crs WHERE stanox = 'TEST-NORM-STANOX'")
            .execute(&pool)
            .await
            .expect("cleanup stanox_crs");
    }
}

/// DB-gated tests for `upsert_tiploc_crs`/`list_tiploc_crs` (Task 2 of
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md). Same
/// shape/doc-comment convention as `stanox_crs_lookup_query_tests` above.
#[cfg(test)]
mod tiploc_crs_query_tests {
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
                two_tiplocs_sharing_a_stanox_both_persist_and_both_come_back -- --ignored`"]
    async fn two_tiplocs_sharing_a_stanox_both_persist_and_both_come_back() {
        // The direct DB-level proof this plan's whole point (two TIPLOCs,
        // one STANOX, both persisted) actually works against a real
        // schema -- independent of the `stanox_crs` table entirely, which
        // would only ever keep one of these two rows under its own
        // STANOX-keyed disambiguation. See `common::TiplocCrsRecord`'s own
        // doc comment.
        let pool = test_pool().await;

        upsert_tiploc_crs(
            &pool,
            &[
                common::TiplocCrsRecord {
                    tiploc: "TEST-VAUXHLM".to_string(),
                    crs: "VXH".to_string(),
                    station_name: "VAUXHALL".to_string(),
                    stanox: "TEST-87214".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
                common::TiplocCrsRecord {
                    tiploc: "TEST-VAUXHLW".to_string(),
                    crs: "VXH".to_string(),
                    station_name: "VAUXHALL".to_string(),
                    stanox: "TEST-87214".to_string(),
                    source_sequence: 1,
                    change_time_minutes: None,
                },
            ],
        )
        .await
        .expect("seed tiploc_crs");

        let rows = list_tiploc_crs(&pool).await.expect("list_tiploc_crs");
        let vauxhlm = rows.iter().find(|r| r.tiploc == "TEST-VAUXHLM");
        let vauxhlw = rows.iter().find(|r| r.tiploc == "TEST-VAUXHLW");
        assert!(
            vauxhlm.is_some(),
            "TEST-VAUXHLM should be present alongside TEST-VAUXHLW, both sharing STANOX TEST-87214"
        );
        assert!(
            vauxhlw.is_some(),
            "TEST-VAUXHLW should be present alongside TEST-VAUXHLM, both sharing STANOX TEST-87214"
        );
        assert_eq!(vauxhlm.unwrap().crs, "VXH");
        assert_eq!(vauxhlm.unwrap().stanox, "TEST-87214");
        assert_eq!(vauxhlw.unwrap().crs, "VXH");
        assert_eq!(vauxhlw.unwrap().stanox, "TEST-87214");

        sqlx::query("DELETE FROM tiploc_crs WHERE tiploc IN ('TEST-VAUXHLM', 'TEST-VAUXHLW')")
            .execute(&pool)
            .await
            .expect("cleanup");
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
        row_with_calling_point_arrival(
            service_date,
            destination_crs,
            scheduled,
            train_uid,
            origin_crs,
            true_origin_crs,
            destination_arrival,
            None,
        )
    }

    /// `row`'s sibling for the tests that actually need to control
    /// `calling_point_arrival` -- kept as a separate function rather than
    /// adding an 8th positional argument to `row` itself, so every existing
    /// `row(...)` call site (which is about something else entirely) does
    /// not need to grow a trailing `None`.
    #[allow(clippy::too_many_arguments)]
    fn row_with_calling_point_arrival(
        service_date: chrono::NaiveDate,
        destination_crs: &str,
        scheduled: chrono::NaiveTime,
        train_uid: &str,
        origin_crs: &str,
        true_origin_crs: Option<&str>,
        destination_arrival: Option<chrono::NaiveTime>,
        calling_point_arrival: Option<chrono::NaiveTime>,
    ) -> ScheduleDestinationDeparturesRow {
        ScheduleDestinationDeparturesRow {
            service_date,
            destination_crs: destination_crs.to_string(),
            scheduled,
            day_offset: 0,
            train_uid: train_uid.to_string(),
            origin_crs: origin_crs.to_string(),
            destination_arrival,
            destination_arrival_day_offset: 0,
            true_origin_crs: true_origin_crs.map(str::to_string),
            calling_point_arrival,
            operator_atoc: None,
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

    /// The real-failure-shape regression for the review finding: today
    /// `schedule-reference` sends exactly one call per service date, but
    /// `rows.chunks(50_000)` is a documented planned fallback for when a
    /// day's payload grows too large for one call. Before
    /// `upsert_schedule_destination_departures_chunk` existed, EVERY call
    /// unconditionally ran `DELETE ... WHERE service_date = ANY(...)`
    /// first, so a second chunk for the same date would have wiped out the
    /// first chunk's just-inserted rows, silently leaving only the last
    /// chunk's data for that day. This publishes two chunks for the same
    /// date -- the second with `first_chunk = false` -- and asserts BOTH
    /// chunks' rows survive.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn a_second_chunk_for_the_same_date_does_not_wipe_the_first_chunks_rows() {
        let pool = test_pool().await;
        let date = fixture_date(3);
        delete_day(&pool, date).await;

        let first_chunk_rows = vec![
            row(date, "ZRD", time(8, 0), "CHUNK-A1", "EUS", None, None),
            row(date, "ZRD", time(9, 0), "CHUNK-A2", "EUS", None, None),
        ];
        let inserted = upsert_schedule_destination_departures_chunk(&pool, &first_chunk_rows, true)
            .await
            .expect("first chunk upsert");
        assert_eq!(inserted, 2);

        // A different train_uid, same date -- the second chunk of the same
        // publish cycle. `first_chunk = false`: must NOT clear the date
        // before inserting.
        let second_chunk_rows = vec![row(date, "ZRD", time(10, 0), "CHUNK-B1", "CRE", None, None)];
        let inserted =
            upsert_schedule_destination_departures_chunk(&pool, &second_chunk_rows, false)
                .await
                .expect("second chunk upsert");
        assert_eq!(inserted, 1);

        let mut stored: Vec<String> = sqlx::query_scalar(
            "SELECT train_uid FROM schedule_destination_departures \
             WHERE service_date = $1 ORDER BY train_uid",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back both chunks");
        stored.sort();
        assert_eq!(
            stored,
            vec![
                "CHUNK-A1".to_string(),
                "CHUNK-A2".to_string(),
                "CHUNK-B1".to_string(),
            ],
            "both chunks' rows must survive -- the first chunk's rows must not be \
             wiped by the second chunk's insert"
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
                row(
                    date,
                    "ZRD",
                    time(8, 0),
                    "C70001",
                    "EUS",
                    Some("EUS"),
                    Some(time(11, 30)),
                ),
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_round_trips_operator_atoc_including_a_null_value() {
        let pool = test_pool().await;
        let date = fixture_date(25);
        delete_day(&pool, date).await;

        upsert_schedule_destination_departures(
            &pool,
            &[
                ScheduleDestinationDeparturesRow {
                    service_date: date,
                    destination_crs: "ZRD".to_string(),
                    scheduled: time(8, 0),
                    day_offset: 0,
                    train_uid: "C80001".to_string(),
                    origin_crs: "EUS".to_string(),
                    true_origin_crs: Some("EUS".to_string()),
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                    operator_atoc: Some("SR".to_string()),
                },
                ScheduleDestinationDeparturesRow {
                    service_date: date,
                    destination_crs: "ZRD".to_string(),
                    scheduled: time(9, 0),
                    day_offset: 0,
                    train_uid: "C80002".to_string(),
                    origin_crs: "CRE".to_string(),
                    true_origin_crs: None,
                    calling_point_arrival: None,
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                    operator_atoc: None,
                },
            ],
        )
        .await
        .expect("seed rows");

        let stored: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT train_uid, operator_atoc FROM schedule_destination_departures \
             WHERE service_date = $1 ORDER BY train_uid",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back");

        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0], ("C80001".to_string(), Some("SR".to_string())));
        assert_eq!(
            stored[1],
            ("C80002".to_string(), None),
            "an absent operator_atoc must round-trip as SQL NULL, not an empty string"
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
                "destination_arrival_day_offset": 0,
                "operator_atoc": null,
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
    async fn search_calling_point_filters_by_true_origin_independent_of_stops_at() {
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

    /// Four trains, each contributing MULTIPLE rows (one per
    /// departure-bearing calling point) sharing one `train_uid`, unlike
    /// `calling_point_fixture_rows` above (one row per schedule, only ever
    /// useful for `origin`/time-range coverage). C41001 calls RDG, OXF and
    /// DID; C41002 calls RDG only (discriminates a plain "calls at OXF"
    /// membership test from a bare "shares a train_uid" one); C41003 calls
    /// RDG and OXF but not DID; C41004 calls RDG and OXF too, but from a
    /// DIFFERENT true origin (PAD, not RDG), to prove `stops_at` and
    /// `origin` are independent filters.
    fn stops_at_fixture_rows(
        service_date: chrono::NaiveDate,
    ) -> Vec<ScheduleDestinationDeparturesRow> {
        vec![
            row(
                service_date,
                "BHM",
                time(8, 0),
                "C41001",
                "RDG",
                Some("RDG"),
                None,
            ),
            row(
                service_date,
                "BHM",
                time(8, 20),
                "C41001",
                "OXF",
                Some("RDG"),
                None,
            ),
            row(
                service_date,
                "BHM",
                time(8, 40),
                "C41001",
                "DID",
                Some("RDG"),
                None,
            ),
            row(
                service_date,
                "BHM",
                time(9, 0),
                "C41002",
                "RDG",
                Some("RDG"),
                None,
            ),
            row(
                service_date,
                "BHM",
                time(10, 0),
                "C41003",
                "RDG",
                Some("RDG"),
                None,
            ),
            row(
                service_date,
                "BHM",
                time(10, 20),
                "C41003",
                "OXF",
                Some("RDG"),
                None,
            ),
            row(
                service_date,
                "BHM",
                time(11, 0),
                "C41004",
                "RDG",
                Some("PAD"),
                None,
            ),
            row(
                service_date,
                "BHM",
                time(11, 20),
                "C41004",
                "OXF",
                Some("PAD"),
                None,
            ),
        ]
    }

    async fn seed_stops_at(pool: &PgPool, service_date: chrono::NaiveDate) {
        delete_day(pool, service_date).await;
        upsert_schedule_destination_departures(pool, &stops_at_fixture_rows(service_date))
            .await
            .expect("seed stops_at fixture rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stops_at_matches_any_train_calling_there() {
        let pool = test_pool().await;
        let date = fixture_date(26);
        seed_stops_at(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            None,
            Some("OXF"),
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        let mut uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        uids.sort();
        assert_eq!(
            uids,
            vec!["C41001", "C41003", "C41004"],
            "stops_at matches every train calling there, regardless of true origin"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_combines_true_origin_and_stops_at_filters() {
        let pool = test_pool().await;
        let date = fixture_date_feb(3);
        seed_stops_at(&pool, date).await;

        let page = search_schedule_calling_point_departures(
            &pool,
            "RDG",
            date,
            any_time(),
            Some("RDG"),
            Some("OXF"),
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        let mut uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        uids.sort();
        assert_eq!(
            uids,
            vec!["C41001", "C41003"],
            "true_origin=RDG excludes C41004 (true origin PAD) even though it also calls OXF"
        );

        delete_day(&pool, date).await;
    }

    /// Six WAT-departing schedules built to pull "calls at WAT again"
    /// apart from "departs WAT", which the unfixed `stops_at` could not
    /// tell apart at all:
    ///
    /// * `L82877` -- the loop. Waterloo 07:27 round via Clapham Junction,
    ///   Kingston and Richmond, TERMINATING back at Waterloo 08:46. Its
    ///   second Waterloo call is arrival-only and therefore has NO row of
    ///   its own here; `destination_crs` is the only place it exists. The
    ///   real shape of South Western Railway train L82877/2026-09-14.
    /// * `P00001` -- the ordinary point-to-point control. Waterloo 07:30
    ///   to Southampton via Clapham Junction and Woking; never returns.
    /// * `L99999` -- the THROUGH-loop, and the sharpest fixture here. It
    ///   departs Waterloo three times (09:00, 09:45, 10:30) and then
    ///   carries on to Southampton, so its first two Waterloo departures
    ///   come back and its LAST one does not. A rule that merely excluded
    ///   the searched row itself would wrongly return all three.
    /// * `O11111` -- an overnight loop: Waterloo 23:40, Surbiton 00:10 and
    ///   back at Waterloo 00:50, the last two after midnight
    ///   (`day_offset` 1). Its return call carries a SMALLER clock time
    ///   than its departure, so only an ordering that reads
    ///   `(day_offset, scheduled)` rather than `scheduled` alone finds it.
    /// * `X50000` -- a schedule with exactly ONE departure-bearing calling
    ///   point (Waterloo 06:00, terminating Southampton): the degenerate
    ///   case where the searched row is the schedule's whole presence in
    ///   this table, so the `EXISTS` has nothing left to consider at all.
    /// * `N00000` -- Waterloo 05:00 to Southampton via Fleet, with NO
    ///   booked arrival at its terminus (`destination_arrival` NULL): the
    ///   row a terminus-matched `stops_at` matches but an arrival bound
    ///   cannot keep.
    ///
    /// Clapham Junction appears in two schedules and is the true origin of
    /// neither -- it is what proves the ordering rule is keyed on the
    /// calling point the SEARCH is anchored at, not on the schedule's true
    /// origin. It also doubles (2026-09-22) as the fixture for the ordering
    /// rule's other direction: both schedules call WAT BEFORE CLJ, so
    /// `station=CLJ&stops_at=WAT` now has to exclude them, exactly as
    /// `station=WAT&stops_at=WAT` already excluded a WAT call that never
    /// comes back.
    fn loop_fixture_rows(service_date: chrono::NaiveDate) -> Vec<ScheduleDestinationDeparturesRow> {
        // Each call is (crs, booked departure, day_offset, this call's own
        // booked arrival).
        type Call = (&'static str, (u32, u32), i16, Option<(u32, u32)>);
        // Each schedule is (train_uid, true_origin_crs, destination_arrival,
        // calls).
        type Schedule = (
            &'static str,
            &'static str,
            Option<(u32, u32)>,
            &'static [Call],
        );
        let schedules: &[Schedule] = &[
            (
                "L82877",
                "WAT",
                Some((8, 46)),
                &[
                    ("WAT", (7, 27), 0, None),
                    ("CLJ", (7, 40), 0, Some((7, 38))),
                    ("KNG", (7, 58), 0, Some((7, 56))),
                    ("RMD", (8, 20), 0, Some((8, 18))),
                ],
            ),
            (
                "P00001",
                "SOU",
                Some((8, 50)),
                &[
                    ("WAT", (7, 30), 0, None),
                    ("CLJ", (7, 55), 0, Some((7, 53))),
                    ("WOK", (8, 10), 0, Some((8, 8))),
                ],
            ),
            (
                "L99999",
                "SOU",
                Some((11, 0)),
                &[
                    ("WAT", (9, 0), 0, None),
                    ("SUR", (9, 20), 0, Some((9, 18))),
                    ("WAT", (9, 45), 0, Some((9, 43))),
                    ("RMD", (10, 5), 0, Some((10, 3))),
                    ("WAT", (10, 30), 0, Some((10, 28))),
                ],
            ),
            (
                "O11111",
                "SOU",
                Some((1, 30)),
                &[
                    ("WAT", (23, 40), 0, None),
                    ("SUR", (0, 10), 1, Some((0, 8))),
                    ("WAT", (0, 50), 1, Some((0, 48))),
                ],
            ),
            ("X50000", "SOU", Some((6, 40)), &[("WAT", (6, 0), 0, None)]),
            (
                "N00000",
                "SOU",
                None,
                &[("WAT", (5, 0), 0, None), ("FLE", (5, 30), 0, Some((5, 28)))],
            ),
        ];

        schedules
            .iter()
            .flat_map(|(train_uid, destination_crs, destination_arrival, calls)| {
                calls.iter().map(
                    move |(origin_crs, (hour, minute), day_offset, calling_point_arrival)| {
                        ScheduleDestinationDeparturesRow {
                            service_date,
                            destination_crs: (*destination_crs).to_string(),
                            scheduled: time(*hour, *minute),
                            day_offset: *day_offset,
                            train_uid: (*train_uid).to_string(),
                            origin_crs: (*origin_crs).to_string(),
                            destination_arrival: destination_arrival.map(|(h, m)| time(h, m)),
                            // Every fixture schedule terminates on the rail
                            // day it started, except the overnight one.
                            destination_arrival_day_offset: i16::from(*train_uid == "O11111"),
                            true_origin_crs: Some("WAT".to_string()),
                            calling_point_arrival: calling_point_arrival.map(|(h, m)| time(h, m)),
                            operator_atoc: None,
                        }
                    },
                )
            })
            .collect()
    }

    async fn seed_loop(pool: &PgPool, service_date: chrono::NaiveDate) {
        delete_day(pool, service_date).await;
        upsert_schedule_destination_departures(pool, &loop_fixture_rows(service_date))
            .await
            .expect("seed loop fixture rows");
    }

    /// `(uid, HH:MM)` for every returned row, in the order the query
    /// returned them -- `uid` alone cannot express "this train matched
    /// three times, once per departure".
    fn uids_and_times(page: &CallingPointDeparturePage) -> Vec<(String, String)> {
        page.departures
            .iter()
            .map(|d| {
                (
                    d["uid"].as_str().unwrap().to_string(),
                    d["scheduled"].as_str().unwrap()[..5].to_string(),
                )
            })
            .collect()
    }

    #[allow(clippy::too_many_arguments)]
    async fn loop_search(
        pool: &PgPool,
        date: chrono::NaiveDate,
        station_crs: &str,
        stops_at: Option<&str>,
        stop_arrival_from: Option<chrono::NaiveTime>,
        stop_arrival_to: Option<chrono::NaiveTime>,
    ) -> CallingPointDeparturePage {
        search_schedule_calling_point_departures(
            pool,
            station_crs,
            date,
            any_time(),
            None,
            stops_at,
            None,
            stop_arrival_from,
            stop_arrival_to,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stops_at_the_searched_station_finds_only_loop_services() {
        // THE BUG. `station=WAT&stops_at=WAT` used to return every train
        // out of Waterloo -- the searched row is a member of its own
        // calling-point list, so the `EXISTS` was true by construction and
        // the result set was byte-identical to supplying no `stops_at` at
        // all. It must instead mean "comes back to Waterloo".
        //
        // What must match, and why each one is here:
        //   L82877 07:27 -- the loop, whose return call is its TERMINUS
        //                   and so reachable only as `destination_crs`.
        //   L99999 09:00 -- a later WAT call exists (09:45, and 10:30).
        //   L99999 09:45 -- a later WAT call exists (10:30).
        //   O11111 23:40 -- its later WAT call is at 00:50 the NEXT day,
        //                   an EARLIER clock time; only a
        //                   (day_offset, scheduled) ordering sees it.
        // What must not, and why:
        //   L99999 10:30 -- three WAT departures, but this last one never
        //                   comes back. "Some OTHER WAT row exists" would
        //                   wrongly keep it.
        //   O11111 00:50 -- likewise the overnight loop's final WAT call.
        //   P00001 07:30 -- the ordinary point-to-point control.
        //   X50000 06:00 -- one calling point in total.
        //   N00000 05:00 -- calls WAT once, terminates elsewhere.
        // Ordered by clock time, as this query has always ordered.
        let pool = test_pool().await;
        let date = fixture_date_feb(4);
        seed_loop(&pool, date).await;

        let page = loop_search(&pool, date, "WAT", Some("WAT"), None, None).await;

        assert_eq!(
            uids_and_times(&page),
            vec![
                ("L82877".to_string(), "07:27".to_string()),
                ("L99999".to_string(), "09:00".to_string()),
                ("L99999".to_string(), "09:45".to_string()),
                ("O11111".to_string(), "23:40".to_string()),
            ],
            "only departures the working actually comes BACK from may match"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stops_at_pages_a_loop_match_yielding_several_rows_per_train() {
        // The loop filter is the first thing that routinely returns several
        // rows for ONE train_uid, so the keyset cursor is worth re-pinning
        // under it: `(scheduled, train_uid)` stays a total order because
        // `origin_crs` is fixed per query, and L99999's two matching
        // departures must land on different pages without repeating or
        // dropping either.
        let pool = test_pool().await;
        let date = fixture_date_feb(10);
        seed_loop(&pool, date).await;

        let first = search_schedule_calling_point_departures(
            &pool,
            "WAT",
            date,
            any_time(),
            None,
            Some("WAT"),
            None,
            None,
            None,
            None,
            2,
        )
        .await
        .expect("search")
        .expect("the day is published");
        assert_eq!(
            uids_and_times(&first),
            vec![
                ("L82877".to_string(), "07:27".to_string()),
                ("L99999".to_string(), "09:00".to_string()),
            ]
        );
        let cursor = first.next_cursor.expect("a second page exists");

        let second = search_schedule_calling_point_departures(
            &pool,
            "WAT",
            date,
            any_time(),
            None,
            Some("WAT"),
            None,
            None,
            None,
            Some(&cursor),
            2,
        )
        .await
        .expect("search")
        .expect("the day is published");
        assert_eq!(
            uids_and_times(&second),
            vec![
                ("L99999".to_string(), "09:45".to_string()),
                ("O11111".to_string(), "23:40".to_string()),
            ],
            "the cursor resumes mid-train without repeating L99999's first matching departure"
        );
        assert!(
            second.next_cursor.is_none(),
            "four matches over two pages of two is exactly the last page"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stops_at_never_matches_on_the_searched_calling_point_itself() {
        // The same "must come later" rule, checked where the searched
        // station is NOT the schedule's true origin: CLJ is an intermediate
        // call of both L82877 and P00001 and the true origin of neither,
        // and neither calls there twice, so `station=CLJ&stops_at=CLJ` must
        // be empty. Pins that the rule is keyed on the SEARCHED calling
        // point, not on `true_origin_crs`.
        let pool = test_pool().await;
        let date = fixture_date_feb(5);
        seed_loop(&pool, date).await;

        let page = loop_search(&pool, date, "CLJ", Some("CLJ"), None, None).await;

        assert_eq!(
            uids_and_times(&page),
            Vec::<(String, String)>::new(),
            "a schedule calling at CLJ exactly once must not satisfy stops_at=CLJ from its own \
             CLJ row"
        );

        // ... and the day really is published, so the empty result above is
        // the filter's doing and not a missing-day artifact.
        let unfiltered = loop_search(&pool, date, "CLJ", None, None, None).await;
        assert_eq!(
            uids_and_times(&unfiltered),
            vec![
                ("L82877".to_string(), "07:40".to_string()),
                ("P00001".to_string(), "07:55".to_string()),
            ],
            "both CLJ departures exist; only the stops_at filter removed them"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stops_at_still_matches_a_genuine_intermediate_stop() {
        // The ordinary case: a different station from the one searched,
        // matched on its own departure-bearing row, which falls LATER in
        // the journey than the search station. KNG is called at by the
        // loop only, so this also proves the ordering rule (see the test
        // below for the case where it excludes instead) does not depend on
        // the two CRS codes being equal.
        let pool = test_pool().await;
        let date = fixture_date_feb(6);
        seed_loop(&pool, date).await;

        let page = loop_search(&pool, date, "WAT", Some("KNG"), None, None).await;

        assert_eq!(
            uids_and_times(&page),
            vec![("L82877".to_string(), "07:27".to_string())],
            "stops_at naming a genuine LATER intermediate call still matches"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stops_at_excludes_a_different_station_reached_before_the_searched_one()
     {
        // The reversed half of the ordering rule (2026-09-22): `stops_at`
        // now means "comes later in the journey than the searched
        // station" for EVERY named station, not only when `stops_at`
        // repeats `station_crs`. WAT is EARLIER in the journey than CLJ for
        // both L82877 (WAT 07:27, CLJ 07:40) and P00001 (WAT 07:30, CLJ
        // 07:55) in their ORIGIN role -- before this change, that role
        // alone was enough to match both (see the design doc's superseded
        // 2026-09-17 addendum).
        //
        // But L82877's true TERMINUS (`destination_crs`) is ALSO WAT: it
        // loops back there at 08:46 (see `loop_fixture_rows`), and the
        // terminus branch has no ordering test of its own -- a terminus is
        // definitionally later than every departure-bearing calling point.
        // So `station=CLJ&stops_at=WAT` still matches L82877, through that
        // branch rather than the ordering rule this test targets. Only
        // P00001, whose true destination is SOU and not WAT, is excluded
        // purely by the ordering rule: its own WAT call precedes CLJ and it
        // never returns to WAT at all.
        let pool = test_pool().await;
        let date = fixture_date_feb(13);
        seed_loop(&pool, date).await;

        let page = loop_search(&pool, date, "CLJ", Some("WAT"), None, None).await;

        assert_eq!(
            uids_and_times(&page),
            vec![("L82877".to_string(), "07:40".to_string())],
            "P00001's own WAT call BEFORE CLJ no longer satisfies stops_at, and it never returns \
             to WAT; L82877 still matches, but only through its true TERMINUS at WAT, not \
             through its earlier origin-role WAT call"
        );

        // ... and the day really is published, so the empty result above is
        // the filter's doing and not a missing-day artifact.
        let unfiltered = loop_search(&pool, date, "CLJ", None, None, None).await;
        assert_eq!(
            uids_and_times(&unfiltered),
            vec![
                ("L82877".to_string(), "07:40".to_string()),
                ("P00001".to_string(), "07:55".to_string()),
            ],
            "both CLJ departures exist; only the stops_at filter removed them"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stops_at_matches_the_true_terminating_calling_point() {
        // The terminus has no departure and so no row of its own in this
        // table; it is reachable only as `destination_crs`. Before the
        // loop fix `stops_at` could not match it at all, which is both the
        // gap the design note flagged and the reason L82877's return to
        // Waterloo was invisible.
        let pool = test_pool().await;
        let date = fixture_date_feb(7);
        seed_loop(&pool, date).await;

        let page = loop_search(&pool, date, "WOK", Some("SOU"), None, None).await;

        assert_eq!(
            uids_and_times(&page),
            vec![("P00001".to_string(), "08:10".to_string())],
            "SOU is P00001's TRUE destination, with no calling-point row of its own"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stop_arrival_bounds_a_terminus_matched_stops_at() {
        // A terminus-matched `stops_at` has no `calling_point_arrival` to
        // bound, so the arrival pair is asked of that terminus's own
        // `destination_arrival` (08:50 for P00001) rather than being
        // evaluated against a column the matched calling point does not
        // have and dropping the row whatever the bound said.
        let pool = test_pool().await;
        let date = fixture_date_feb(8);
        seed_loop(&pool, date).await;

        let inside = loop_search(
            &pool,
            date,
            "WOK",
            Some("SOU"),
            Some(time(8, 45)),
            Some(time(8, 55)),
        )
        .await;
        assert_eq!(
            uids_and_times(&inside),
            vec![("P00001".to_string(), "08:10".to_string())],
            "08:50 is inside 08:45..=08:55"
        );

        let outside = loop_search(
            &pool,
            date,
            "WOK",
            Some("SOU"),
            Some(time(9, 0)),
            Some(time(9, 30)),
        )
        .await;
        assert_eq!(
            uids_and_times(&outside),
            Vec::<(String, String)>::new(),
            "08:50 is outside 09:00..=09:30 -- the bound really is applied, not ignored"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stop_arrival_bounds_a_revisited_stops_at() {
        // The mirror of the test above on the `EXISTS` branch: WAT is
        // L99999's own searched station AND revisited twice, so the bound
        // must be read off the OTHER WAT rows' `calling_point_arrival`
        // (09:43 and 10:28), never off the anchor row (NULL at its true
        // origin) and never off `destination_arrival` (11:00).
        let pool = test_pool().await;
        let date = fixture_date_feb(9);
        seed_loop(&pool, date).await;

        let page = loop_search(
            &pool,
            date,
            "WAT",
            Some("WAT"),
            Some(time(10, 0)),
            Some(time(11, 0)),
        )
        .await;

        assert_eq!(
            uids_and_times(&page),
            vec![
                ("L99999".to_string(), "09:00".to_string()),
                ("L99999".to_string(), "09:45".to_string()),
            ],
            "only the 10:28 revisit is in range, so the two departures that precede it match; \
             the 10:30 departure has no later WAT call to arrive at"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stop_arrival_bounds_a_loop_whose_return_call_is_its_terminus() {
        // The headline case with the arrival pair on top of it: the loop
        // L82877 matched `station=WAT&stops_at=WAT` through its TERMINUS,
        // so "when does it get back to Waterloo" is `destination_arrival`
        // (08:46) and nothing else. Both branches are live in this query
        // at once -- L99999's revisits go through the `EXISTS` -- and the
        // bound must pick them apart rather than letting either rescue the
        // other.
        let pool = test_pool().await;
        let date = fixture_date_feb(11);
        seed_loop(&pool, date).await;

        let page = loop_search(
            &pool,
            date,
            "WAT",
            Some("WAT"),
            Some(time(8, 40)),
            Some(time(8, 50)),
        )
        .await;

        assert_eq!(
            uids_and_times(&page),
            vec![("L82877".to_string(), "07:27".to_string())],
            "08:46 back at Waterloo is inside 08:40..=08:50; L99999's revisits (09:43, 10:28) \
             and O11111's (00:48) are not"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_stop_arrival_drops_a_terminus_with_no_booked_arrival() {
        // `destination_arrival` is genuinely nullable in published data (see
        // its own migration), and a NULL never satisfies a bound. So a
        // schedule `stops_at` matched ONLY through its terminus disappears
        // the moment an arrival bound is set, however wide -- the same way
        // a NULL `calling_point_arrival` has always behaved on the other
        // branch. Pinned rather than left to be discovered: it is the one
        // place the arrival pair narrows what `stops_at` matched.
        let pool = test_pool().await;
        let date = fixture_date_feb(12);
        seed_loop(&pool, date).await;

        let unbounded = loop_search(&pool, date, "FLE", Some("SOU"), None, None).await;
        assert_eq!(
            uids_and_times(&unbounded),
            vec![("N00000".to_string(), "05:30".to_string())],
            "with no bound the terminus match stands, NULL arrival and all"
        );

        let bounded = loop_search(
            &pool,
            date,
            "FLE",
            Some("SOU"),
            Some(chrono::NaiveTime::MIN),
            Some(
                chrono::NaiveTime::from_hms_opt(23, 59, 59).expect("valid end-of-day fixture time"),
            ),
        )
        .await;
        assert_eq!(
            uids_and_times(&bounded),
            Vec::<(String, String)>::new(),
            "a whole-day bound still drops it: there is no arrival to compare against"
        );

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

        // No fixture row calls at this station.
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
    async fn search_calling_point_stop_arrival_filters_by_the_named_calling_points_own_arrival_not_the_true_destinations()
     {
        // The load-bearing distinction from the deleted destination_from/
        // destination_to: OXF is NOT either train's true destination (BHM
        // is), and its own `calling_point_arrival` is a DIFFERENT value
        // from `destination_arrival` for both trains -- only the former
        // must be what arrival_from/arrival_to bounds.
        let pool = test_pool().await;
        let date = fixture_date_feb(1);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[
                // C81001: calls RDG (required station, true origin, no
                // arrival) then OXF (intermediate, arrives 08:40) then
                // terminates at BHM (destination_arrival 10:00, deliberately
                // OUTSIDE the 09:00-09:30 window this test searches, to
                // prove that column is NOT what gets checked).
                row(
                    date,
                    "BHM",
                    time(8, 0),
                    "C81001",
                    "RDG",
                    Some("RDG"),
                    Some(time(10, 0)),
                ),
                row_with_calling_point_arrival(
                    date,
                    "BHM",
                    time(8, 20),
                    "C81001",
                    "OXF",
                    Some("RDG"),
                    Some(time(10, 0)),
                    Some(time(8, 40)),
                ),
                // C81002: same shape, but its OXF arrival (09:20) falls
                // inside the search window.
                row(
                    date,
                    "BHM",
                    time(8, 5),
                    "C81002",
                    "RDG",
                    Some("RDG"),
                    Some(time(10, 5)),
                ),
                row_with_calling_point_arrival(
                    date,
                    "BHM",
                    time(8, 25),
                    "C81002",
                    "OXF",
                    Some("RDG"),
                    Some(time(10, 5)),
                    Some(time(9, 20)),
                ),
                // C81003: also calls OXF, but that calling point's own
                // arrival was never recorded (None) -- must be excluded
                // once an arrival bound is applied, same NULL-is-not-a-match
                // posture the deleted destination_arrival filter had.
                row(date, "BHM", time(8, 10), "C81003", "RDG", Some("RDG"), None),
                row(date, "BHM", time(8, 30), "C81003", "OXF", Some("RDG"), None),
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
            Some("OXF"),
            None,
            Some(time(9, 0)),
            Some(time(9, 30)),
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
            vec!["C81002"],
            "only C81002's OWN OXF arrival (09:20) falls in the window -- C81001's OXF arrival \
             (08:40) doesn't, and C81003's OXF arrival is NULL, even though both trains' \
             destination_arrival columns would have matched or been irrelevant"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_calling_point -- --ignored --test-threads=1`"]
    async fn search_calling_point_with_no_stop_arrival_bounds_ignores_a_null_calling_point_arrival()
    {
        // `stops_at` deliberately names a DIFFERENT station from the one
        // searched: since the loop fix a `stops_at` equal to the searched
        // station asks "does this working come back here", which is a real
        // question and not the "any row at all, to switch the filter on"
        // this test wants. The NULL under test is OXF's own
        // `calling_point_arrival`.
        let pool = test_pool().await;
        let date = fixture_date_feb(2);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "WAT", time(8, 0), "C80003", "RDG", None, None),
                row(date, "WAT", time(8, 20), "C80003", "OXF", None, None),
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
            Some("OXF"),
            None,
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(
            page.departures.len(),
            1,
            "no arrival_from/to means the NULL calling_point_arrival row is still returned"
        );

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
                    day_offset: 0,
                    train_uid: "TEST-JS-CPD".to_string(),
                    origin_crs: "RDG".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                    calling_point_arrival: None,
                    operator_atoc: None,
                },
                ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "WAT".to_string(),
                    scheduled: "10:32:00".parse().unwrap(),
                    day_offset: 0,
                    train_uid: "TEST-JS-CPD".to_string(),
                    origin_crs: "SLO".to_string(),
                    true_origin_crs: Some("RDG".to_string()),
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                    calling_point_arrival: None,
                    operator_atoc: None,
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

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_schedule_calling_points_full_for_train_returns_every_row_in_seq_order \
                -- --ignored --test-threads=1`"]
    async fn list_schedule_calling_points_full_for_train_returns_every_row_in_seq_order() {
        // `journey::build_journey_stops`'s fallback source since the
        // 2026-09-23 fix (see `list_calling_point_departures_for_train`'s
        // own doc comment) -- unlike that predecessor, EVERY calling point
        // comes back regardless of whether its TIPLOC resolves to a CRS
        // (there is no CRS column on this table at all), and the
        // terminating calling point is one of these rows too, not appended
        // separately.
        let pool = test_pool().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TEST-JS-SCPF'")
            .execute(&pool)
            .await
            .ok();

        upsert_schedule_calling_points_full(
            &pool,
            &[
                ScheduleCallingPointsFullRow {
                    service_date,
                    uid: "TEST-JS-SCPF".to_string(),
                    seq: 0,
                    tiploc: "RDG    ".to_string(),
                    kind: "origin".to_string(),
                    booked_arrival: None,
                    booked_departure: "10:15:00".parse().ok(),
                    day_offset: 0,
                },
                ScheduleCallingPointsFullRow {
                    service_date,
                    uid: "TEST-JS-SCPF".to_string(),
                    seq: 1,
                    // A TIPLOC with no CRS at all -- unlike
                    // `schedule_destination_departures`, this table has no
                    // CRS column to fail to resolve, so a genuine junction
                    // still comes back as a real row.
                    tiploc: "TESTJCTJ".to_string(),
                    kind: "intermediate".to_string(),
                    booked_arrival: None,
                    booked_departure: None,
                    day_offset: 0,
                },
                ScheduleCallingPointsFullRow {
                    service_date,
                    uid: "TEST-JS-SCPF".to_string(),
                    seq: 2,
                    tiploc: "WAT    ".to_string(),
                    kind: "terminate".to_string(),
                    booked_arrival: "10:32:00".parse().ok(),
                    booked_departure: None,
                    day_offset: 0,
                },
            ],
        )
        .await
        .expect("seed schedule_calling_points_full");

        let rows = list_schedule_calling_points_full_for_train(&pool, "TEST-JS-SCPF", service_date)
            .await
            .expect("list_schedule_calling_points_full_for_train");

        assert_eq!(
            rows.len(),
            3,
            "every calling point, including the junction and the terminus"
        );
        assert_eq!(rows[0].tiploc, "RDG    ");
        assert_eq!(rows[0].kind, "origin");
        assert_eq!(rows[1].tiploc, "TESTJCTJ");
        assert_eq!(rows[1].kind, "intermediate");
        assert_eq!(rows[2].tiploc, "WAT    ");
        assert_eq!(rows[2].kind, "terminate");
        assert_eq!(rows[2].booked_arrival, "10:32:00".parse().ok());

        sqlx::query("DELETE FROM schedule_calling_points_full WHERE uid = 'TEST-JS-SCPF'")
            .execute(&pool)
            .await
            .ok();
    }

    /// The `schedule_calling_points_full` sibling of
    /// `a_second_chunk_for_the_same_date_does_not_wipe_the_first_chunks_rows`
    /// -- same review finding, same real-failure shape: before
    /// `upsert_schedule_calling_points_full_chunk` existed, every call
    /// unconditionally deleted the date first, so a second chunk publishing
    /// a different train's calling points for the same date would have
    /// wiped out the first chunk's just-inserted rows. Publishes two
    /// chunks (different `uid`s, same `service_date`) -- the second with
    /// `first_chunk = false` -- and asserts BOTH chunks' rows survive.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                a_second_chunk_for_the_same_date_does_not_wipe_the_first_chunks_calling_points \
                -- --ignored --test-threads=1`"]
    async fn a_second_chunk_for_the_same_date_does_not_wipe_the_first_chunks_calling_points() {
        let pool = test_pool().await;
        let service_date: chrono::NaiveDate = "2026-09-08".parse().unwrap();
        sqlx::query(
            "DELETE FROM schedule_calling_points_full WHERE uid IN ('TEST-CHUNK-A', 'TEST-CHUNK-B')",
        )
        .execute(&pool)
        .await
        .ok();

        let first_chunk_rows = vec![ScheduleCallingPointsFullRow {
            service_date,
            uid: "TEST-CHUNK-A".to_string(),
            seq: 0,
            tiploc: "RDG    ".to_string(),
            kind: "origin".to_string(),
            booked_arrival: None,
            booked_departure: "10:15:00".parse().ok(),
            day_offset: 0,
        }];
        let inserted = upsert_schedule_calling_points_full_chunk(&pool, &first_chunk_rows, true)
            .await
            .expect("first chunk upsert");
        assert_eq!(inserted, 1);

        // A different train (uid), same service_date -- the second chunk
        // of the same publish cycle. `first_chunk = false`: must NOT clear
        // the date before inserting.
        let second_chunk_rows = vec![ScheduleCallingPointsFullRow {
            service_date,
            uid: "TEST-CHUNK-B".to_string(),
            seq: 0,
            tiploc: "WAT    ".to_string(),
            kind: "origin".to_string(),
            booked_arrival: None,
            booked_departure: "11:00:00".parse().ok(),
            day_offset: 0,
        }];
        let inserted = upsert_schedule_calling_points_full_chunk(&pool, &second_chunk_rows, false)
            .await
            .expect("second chunk upsert");
        assert_eq!(inserted, 1);

        let mut stored: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT uid FROM schedule_calling_points_full \
             WHERE service_date = $1 AND uid IN ('TEST-CHUNK-A', 'TEST-CHUNK-B') ORDER BY uid",
        )
        .bind(service_date)
        .fetch_all(&pool)
        .await
        .expect("read back both chunks");
        stored.sort();
        assert_eq!(
            stored,
            vec!["TEST-CHUNK-A".to_string(), "TEST-CHUNK-B".to_string()],
            "both chunks' rows must survive -- the first chunk's rows must not be \
             wiped by the second chunk's insert"
        );

        sqlx::query(
            "DELETE FROM schedule_calling_points_full WHERE uid IN ('TEST-CHUNK-A', 'TEST-CHUNK-B')",
        )
        .execute(&pool)
        .await
        .ok();
    }

    /// The midnight-crossing regression this whole fix targets, at the
    /// query layer: a real overnight service's post-midnight calling point
    /// (small `scheduled`, e.g. `00:07`, but `day_offset = 1`) must still
    /// sort AFTER its pre-midnight calling points (large `scheduled`, e.g.
    /// `23:48`, `day_offset = 0`) -- a bare `ORDER BY scheduled` would put
    /// it first, inverting the journey timeline. Named after the real
    /// live-confirmed c2c UID F49687 case (2026-09-09 investigation).
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                list_calling_point_departures_for_train_orders_a_midnight_crossing_schedule_correctly \
                -- --ignored --test-threads=1`"]
    async fn list_calling_point_departures_for_train_orders_a_midnight_crossing_schedule_correctly()
    {
        let pool = test_pool().await;
        let service_date: chrono::NaiveDate = "2026-09-05".parse().unwrap();
        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-F49687'")
            .execute(&pool)
            .await
            .ok();

        upsert_schedule_destination_departures(
            &pool,
            &[
                ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "SHENFLD".to_string(),
                    scheduled: "23:48:00".parse().unwrap(),
                    day_offset: 0,
                    train_uid: "TEST-F49687".to_string(),
                    origin_crs: "LIVST".to_string(),
                    true_origin_crs: Some("LIVST".to_string()),
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                    calling_point_arrival: None,
                    operator_atoc: None,
                },
                ScheduleDestinationDeparturesRow {
                    service_date,
                    destination_crs: "SHENFLD".to_string(),
                    scheduled: "00:07:00".parse().unwrap(),
                    day_offset: 1,
                    train_uid: "TEST-F49687".to_string(),
                    origin_crs: "BARKING".to_string(),
                    true_origin_crs: Some("LIVST".to_string()),
                    destination_arrival: None,
                    destination_arrival_day_offset: 0,
                    calling_point_arrival: None,
                    operator_atoc: None,
                },
            ],
        )
        .await
        .expect("seed schedule_destination_departures");

        let rows = list_calling_point_departures_for_train(&pool, "TEST-F49687", service_date)
            .await
            .expect("list_calling_point_departures_for_train");

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0].origin_crs, "LIVST",
            "23:48 (day_offset 0) must sort FIRST, even though its own naive time is numerically \
             LARGER than Barking's 00:07 -- a bare ORDER BY scheduled would get this backwards"
        );
        assert_eq!(rows[0].day_offset, 0);
        assert_eq!(rows[1].origin_crs, "BARKING");
        assert_eq!(rows[1].day_offset, 1);

        sqlx::query("DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-F49687'")
            .execute(&pool)
            .await
            .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_journey_leg_candidates -- --ignored --test-threads=1`"]
    async fn search_journey_leg_candidates_enforces_ordering_for_different_stations() {
        let pool = test_pool().await;
        let date = fixture_date_feb(13);
        delete_day(&pool, date).await;

        upsert_schedule_destination_departures(
            &pool,
            &[
                // A train that calls at RDG BEFORE WAT on this diagram --
                // the "loops back the wrong way" case this function must
                // exclude for a WAT -> RDG leg search, unlike the
                // general-purpose search_schedule_calling_point_departures
                // (see this function's own doc comment).
                row(date, "SOU", time(8, 0), "T00001", "RDG", None, None),
                row(date, "SOU", time(8, 30), "T00001", "WAT", None, None),
                // A genuinely valid candidate: WAT then RDG, in order.
                row(date, "RDG", time(9, 0), "T00002", "WAT", None, None),
                row(date, "RDG", time(9, 30), "T00002", "RDG", None, None),
            ],
        )
        .await
        .expect("seed fixture rows");

        let page = search_journey_leg_candidates(
            &pool, "WAT", "RDG", date, None, None, None, None, None, None, 50,
        )
        .await
        .expect("search candidates")
        .expect("service date is published");

        let uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            vec!["T00002"],
            "T00001 calls at RDG before WAT and must be excluded"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_journey_leg_candidates -- --ignored --test-threads=1`"]
    async fn search_journey_leg_candidates_applies_depart_and_arrive_windows() {
        let pool = test_pool().await;
        let date = fixture_date_feb(14);
        delete_day(&pool, date).await;

        upsert_schedule_destination_departures(
            &pool,
            &[
                row(
                    date,
                    "RDG",
                    time(7, 0),
                    "T00003",
                    "WAT",
                    None,
                    Some(time(7, 30)),
                ),
                row(
                    date,
                    "RDG",
                    time(7, 30),
                    "T00003",
                    "RDG",
                    None,
                    Some(time(7, 30)),
                ),
                row(
                    date,
                    "RDG",
                    time(9, 0),
                    "T00004",
                    "WAT",
                    None,
                    Some(time(9, 30)),
                ),
                row(
                    date,
                    "RDG",
                    time(9, 30),
                    "T00004",
                    "RDG",
                    None,
                    Some(time(9, 30)),
                ),
            ],
        )
        .await
        .expect("seed fixture rows");

        let page = search_journey_leg_candidates(
            &pool,
            "WAT",
            "RDG",
            date,
            Some(time(8, 0)),
            None,
            None,
            Some(time(9, 35)),
            None,
            None,
            50,
        )
        .await
        .expect("search candidates")
        .expect("service date is published");

        let uids: Vec<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(uids, vec!["T00004"]);

        delete_day(&pool, date).await;
    }

    /// Real-world regression: EUS -> MKC window search silently missing one
    /// operator's real services (the reported bug this test exists for).
    ///
    /// Runs the FULL production ingestion pipeline this function's own doc
    /// comment says `schedule-reference` performs each CIF delivery --
    /// `schedule_query::ScheduleIndex::from_text` -> `departures_by_destination_crs`
    /// -- against two REAL CIF schedules, not hand-typed `row()` fixtures,
    /// to rule the parser/resolver layer in or out as the root cause, not
    /// just the SQL:
    ///
    /// * `C17798` (Avanti West Coast, WCML): real byte-for-byte block
    ///   already quoted verbatim in
    ///   `crates/schedule-query/tests/real_cif_fixtures.rs`'s
    ///   `WCML_MULTI_STATION_SCHEDULES` -- `EUS@0756 -> MKC@0837`
    ///   (terminus), reconstructed from
    ///   docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md
    ///   line 515's real quote.
    /// * `C18017` (a real `lnwr-birmingham-crewe` UID -- London Northwestern
    ///   Railway's own EUS-Crewe corridor line in this app's own monitoring,
    ///   same doc, "Pin 46 -- `C18017`, Euston->Crewe" section, lines
    ///   1890-1899): real UID/TIPLOC/time values quoted directly --
    ///   `EUSTON dep 14:46 -> MILTON KEYNES CENTRAL arr/dep 15:18/15:19 ->
    ///   STAFFORD arr 16:30`. The real schedule continues past Stafford to
    ///   Crewe per the quote's own "Euston->Crewe" label, but no further
    ///   real time is quoted anywhere in that doc, so this reconstruction
    ///   stops at Stafford (STA) -- same "leave it open, don't fabricate a
    ///   terminus" posture `real_cif_fixtures.rs`'s own `F26094_BANK_HOLIDAY_BODY`
    ///   documents for an identical gap. Critically, MKC here is a genuine
    ///   `LI` (Intermediate) calling point with its own booked arrival AND
    ///   departure -- MKC is NOT this schedule's terminus, so
    ///   `search_journey_leg_candidates`'s `EXISTS` branch (not its
    ///   `main.destination_crs = $5` branch) is what must find it, exactly
    ///   the branch the general-purpose sibling function historically
    ///   didn't check unconditionally (see this function's own doc comment,
    ///   point 1).
    ///
    /// Both real UIDs run a real WCML corridor through the real MKNSCEN
    /// TIPLOC (Milton Keynes Central's real TIPLOC, confirmed by
    /// `WCML_MULTI_STATION_SCHEDULES`'s own doc comment) -- two different,
    /// real operators' schedules calling at the exact same real station,
    /// the precondition the reported bug needed and every other fixture in
    /// this file (synthetic CRS codes like "ZRD"/"RDG") never exercised.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_journey_leg_candidates_eus_mkc -- --ignored --test-threads=1`"]
    async fn search_journey_leg_candidates_includes_every_real_operator_calling_at_a_shared_station()
     {
        // Real byte-verbatim BS/LO/LT block, quoted directly from
        // `crates/schedule-query/tests/real_cif_fixtures.rs`'s own
        // `WCML_MULTI_STATION_SCHEDULES` -- Avanti West Coast's C17798,
        // EUS@0756 terminating at MKC@0837. `date_from`/`date_to` are the
        // ONE deliberate deviation from that real quote (real range
        // 260523..261212): this module's own doc comment requires every DB
        // fixture to own a distant-future `service_date`, uncontaminated by
        // real production data -- but real CIF `YYMMDD` is a genuinely
        // 2-digit year, and `chrono`'s own `%y` pivot (verified directly:
        // `"68" -> 2068`, `"69" -> 1969`) caps how far "distant future" can
        // go through this crate's own real parser at 2068-12-31, short of
        // this file's usual 2099 sentinel. `670101..671231` (2067) is used
        // here instead -- still decades past any real delivery this app
        // will ever ingest, just inside the format's own real ceiling.
        const AVANTI_EUS_MKC: &str = "\
BSNC177986701016712311111111           P
LOEUSTON  0756         TB
LTMKNSCEN 0837         TF";

        // Reconstructed from the real, directly-quoted UID/TIPLOC/time
        // values in docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md's
        // "Pin 46 -- `C18017`, Euston->Crewe (`lnwr-birmingham-crewe`)"
        // section (lines 1890-1899) -- a real London Northwestern Railway
        // EUS-Crewe-corridor working. date_from/date_to/days_of_week are
        // not given by that quote (only that it ran on 2026-09-11), so
        // this reconstruction runs it daily across a wide real-shaped
        // range (2067, same "%y`-ceiling" reasoning as `AVANTI_EUS_MKC`
        // above).
        const LNR_EUS_MKC_INTERMEDIATE: &str = "\
BSNC180176701016712311111111           P
LOEUSTON  1446         TB
LIMKNSCEN 1518 1519         T
LTSTAFFRD 1630         TF";

        let index = schedule_query::ScheduleIndex::from_text(&format!(
            "{AVANTI_EUS_MKC}\n{LNR_EUS_MKC_INTERMEDIATE}"
        ));

        let tiploc_to_crs: std::collections::HashMap<String, String> =
            [("EUSTON", "EUS"), ("MKNSCEN", "MKC"), ("STAFFRD", "STA")]
                .into_iter()
                .map(|(tiploc, crs)| (tiploc.to_string(), crs.to_string()))
                .collect();

        // 2067, not this module's usual 2099 sentinel -- see
        // `AVANTI_EUS_MKC`'s own doc comment for why real CIF's 2-digit
        // year caps how far into the future a date parsed by the real
        // parser under test can go. Still decades clear of any real
        // service date this app will ever ingest.
        let date = chrono::NaiveDate::from_ymd_opt(2067, 2, 15).expect("valid fixture date");
        // Midnight -- the real `now` `publish_schedule_destination_departures`
        // uses (see that function's own doc comment, point 1): publishes
        // the whole rail day, uncapped, exactly like production.
        let by_destination = schedule_query::departures_by_destination_crs(
            &index,
            date,
            chrono::NaiveTime::MIN,
            &tiploc_to_crs,
        );

        // The exact same one-row-per-departure flatten
        // `crates/schedule-reference/src/main.rs::schedule_destination_departures_rows`
        // performs, rebuilt here as `ScheduleDestinationDeparturesRow`
        // instead of `serde_json::Value` purely so it can go straight into
        // `upsert_schedule_destination_departures` without a
        // serialize/deserialize round trip -- the ingest route
        // (`routes::ingest::post_schedule_destination_departures`)
        // deserializes the wire JSON into this exact same struct, so this
        // is a faithful stand-in for "the batch `schedule-reference` would
        // have POSTed this cycle."
        let rows: Vec<ScheduleDestinationDeparturesRow> = by_destination
            .into_iter()
            .flat_map(|(destination_crs, departures)| {
                departures
                    .into_iter()
                    .map(move |d| ScheduleDestinationDeparturesRow {
                        service_date: date,
                        destination_crs: destination_crs.clone(),
                        scheduled: d.scheduled,
                        day_offset: d.day_offset as i16,
                        train_uid: d.uid,
                        origin_crs: d.origin_crs,
                        true_origin_crs: d.true_origin_crs,
                        calling_point_arrival: d.calling_point_arrival,
                        destination_arrival: d.destination_arrival,
                        destination_arrival_day_offset: d.destination_arrival_day_offset as i16,
                        operator_atoc: d.operator_atoc,
                    })
            })
            .collect();

        assert!(
            !rows.is_empty(),
            "the real ingestion pipeline must have produced at least one row from two real, \
             non-cancelled, correctly-TIPLOC-resolved schedules"
        );

        let pool = test_pool().await;
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(&pool, &rows)
            .await
            .expect("seed real-pipeline-derived fixture rows");

        let page = search_journey_leg_candidates(
            &pool, "EUS", "MKC", date, None, None, None, None, None, None, 50,
        )
        .await
        .expect("search candidates")
        .expect("service date is published");

        let uids: std::collections::BTreeSet<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            std::collections::BTreeSet::from(["C17798", "C18017"]),
            "both real operators' EUS -> MKC services must appear in the candidate list -- \
             C17798 (Avanti, MKC as its terminus) AND C18017 (London Northwestern, MKC as a \
             genuine intermediate calling point on the way to Crewe). Got: {uids:?}"
        );

        delete_day(&pool, date).await;
    }

    /// Sibling of
    /// `search_journey_leg_candidates_includes_every_real_operator_calling_at_a_shared_station`
    /// above -- proves the NEW `operators` filter parameter this task
    /// adds, not the already-fixed ordering bug that test exists for. Not
    /// a mutation of that test: the two are proving two different things,
    /// and that test's own two fixture blocks deliberately carry no `BX`
    /// line at all (both currently resolve to `operator_atoc: None`),
    /// which would make them useless for this.
    ///
    /// Same real byte-verbatim `BS`/`LO`/`LI`/`LT` bodies as the sibling
    /// test above, with one addition: a `BX` line inserted between each
    /// `BS` line and its first `LO` line (the real CIF record order -- a
    /// `BX` line always immediately follows its own `BS`), following this
    /// crate's own established "synthetic value, real byte layout" fixture
    /// convention -- see
    /// `crates/schedule-query/tests/real_cif_fixtures.rs`'s own
    /// `SYNTHETIC_MINIMAL_BLOCK`'s `BX         SRYSR000000` line for the
    /// precedent of exactly this shape (the ATOC Code is decoded from the
    /// real, parser-verified `11..13` byte offset -- see
    /// `schedule_query::parse::parse_bx_operator`'s own doc comment).
    /// `"XX"`/`"ZZ"` are fully synthetic two-letter codes here -- just
    /// structurally valid and clearly distinct from each other, no
    /// real-world ATOC meaning claimed for either.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                search_journey_leg_candidates_operator_filter -- --ignored --test-threads=1`"]
    async fn search_journey_leg_candidates_operator_filter_matches_only_the_requested_atoc_code() {
        const AVANTI_EUS_MKC_XX: &str = "\
BSNC177986701016712311111111           P
BX         XXY000000
LOEUSTON  0756         TB
LTMKNSCEN 0837         TF";

        const LNR_EUS_MKC_INTERMEDIATE_ZZ: &str = "\
BSNC180176701016712311111111           P
BX         ZZY000000
LOEUSTON  1446         TB
LIMKNSCEN 1518 1519         T
LTSTAFFRD 1630         TF";

        let index = schedule_query::ScheduleIndex::from_text(&format!(
            "{AVANTI_EUS_MKC_XX}\n{LNR_EUS_MKC_INTERMEDIATE_ZZ}"
        ));

        let tiploc_to_crs: std::collections::HashMap<String, String> =
            [("EUSTON", "EUS"), ("MKNSCEN", "MKC"), ("STAFFRD", "STA")]
                .into_iter()
                .map(|(tiploc, crs)| (tiploc.to_string(), crs.to_string()))
                .collect();

        // A different day than the sibling test above -- not load-bearing
        // (each test's own `delete_day` bracket makes reuse safe even
        // under `--test-threads=1`), just avoids any doubt about
        // cross-test interference.
        let date = chrono::NaiveDate::from_ymd_opt(2067, 2, 16).expect("valid fixture date");
        let by_destination = schedule_query::departures_by_destination_crs(
            &index,
            date,
            chrono::NaiveTime::MIN,
            &tiploc_to_crs,
        );

        let rows: Vec<ScheduleDestinationDeparturesRow> = by_destination
            .into_iter()
            .flat_map(|(destination_crs, departures)| {
                departures
                    .into_iter()
                    .map(move |d| ScheduleDestinationDeparturesRow {
                        service_date: date,
                        destination_crs: destination_crs.clone(),
                        scheduled: d.scheduled,
                        day_offset: d.day_offset as i16,
                        train_uid: d.uid,
                        origin_crs: d.origin_crs,
                        true_origin_crs: d.true_origin_crs,
                        calling_point_arrival: d.calling_point_arrival,
                        destination_arrival: d.destination_arrival,
                        destination_arrival_day_offset: d.destination_arrival_day_offset as i16,
                        operator_atoc: d.operator_atoc,
                    })
            })
            .collect();

        assert!(
            rows.iter()
                .any(|r| r.operator_atoc.as_deref() == Some("XX")),
            "C17798's synthetic BX line must have decoded to operator_atoc \"XX\" -- got: {:?}",
            rows.iter()
                .map(|r| (&r.train_uid, &r.operator_atoc))
                .collect::<Vec<_>>()
        );
        assert!(
            rows.iter()
                .any(|r| r.operator_atoc.as_deref() == Some("ZZ")),
            "C18017's synthetic BX line must have decoded to operator_atoc \"ZZ\" -- got: {:?}",
            rows.iter()
                .map(|r| (&r.train_uid, &r.operator_atoc))
                .collect::<Vec<_>>()
        );

        let pool = test_pool().await;
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(&pool, &rows)
            .await
            .expect("seed real-pipeline-derived fixture rows");

        let page = search_journey_leg_candidates(
            &pool,
            "EUS",
            "MKC",
            date,
            None,
            None,
            None,
            None,
            Some(vec!["XX".to_string()]),
            None,
            50,
        )
        .await
        .expect("search candidates")
        .expect("service date is published");

        let uids: std::collections::BTreeSet<&str> = page
            .departures
            .iter()
            .map(|d| d["uid"].as_str().unwrap())
            .collect();
        assert_eq!(
            uids,
            std::collections::BTreeSet::from(["C17798"]),
            "operators: Some([\"XX\"]) must match only C17798 (operator_atoc \"XX\"), \
             excluding C18017 (operator_atoc \"ZZ\") even though both call at EUS/MKC in the \
             requested window. Got: {uids:?}"
        );

        delete_day(&pool, date).await;
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
                movement_events_for_train_returns_every_event_oldest_first \
                -- --ignored --test-threads=1`"]
    async fn movement_events_for_train_returns_every_event_oldest_first() {
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

        let rows = movement_events_for_train(&pool, trains_id)
            .await
            .expect("movement_events_for_train");

        // BOTH events come back now, oldest-received first, and both carry
        // an upper-cased `loc_crs` even though one was stored lower-cased.
        // Collapsing a location to its single latest event is no longer
        // this query's job -- `journey::assign_events_to_stops` does it per
        // VISIT instead, which is the only way a circular service's two
        // separate calls at one station can be told apart.
        assert_eq!(rows.len(), 2, "every retained event, not one per location");
        assert_eq!(rows[0].loc_crs, "RDG");
        assert_eq!(rows[0].event_type.as_deref(), Some("ARRIVAL"));
        assert_eq!(rows[1].loc_crs, "RDG");
        assert_eq!(rows[1].event_type.as_deref(), Some("DEPARTURE"));

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

        assert!(
            names.contains_key("JTO"),
            "JTO test station should be found"
        );
        assert!(!names.contains_key("ZZZ"));

        sqlx::query("DELETE FROM stations WHERE crs = 'JTO'")
            .execute(&pool)
            .await
            .ok();
    }
}

/// DB-gated coverage for the 2026-09-25 schedule-pipeline fixes:
///
/// * the chunk-aware `replace_dates` contract on
///   [`upsert_schedule_calling_points_full_chunk`] /
///   [`upsert_schedule_destination_departures_chunk`], which is what makes it
///   safe for `schedule-reference` to split an oversized per-date publish into
///   several POSTs, and
/// * [`insert_schedule_reference_publish`] /
///   [`last_completed_schedule_reference_publish`], the completion marker that
///   replaced "seed the restart dedup from `schedule-ingest`'s extraction
///   record".
///
/// Far-future fixture dates (2099) and a `TEST-`-prefixed delivery name, so
/// nothing here can be answered by, or damage, real data. Same posture as this
/// file's other `*_query_tests` modules.
#[cfg(test)]
mod schedule_pipeline_integrity_tests {
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

    fn fixture_date(day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2099, 6, day).expect("valid fixture date")
    }

    fn time(h: u32, m: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, 0).expect("valid fixture time")
    }

    fn calling_point(
        service_date: chrono::NaiveDate,
        uid: &str,
        seq: i16,
    ) -> ScheduleCallingPointsFullRow {
        ScheduleCallingPointsFullRow {
            service_date,
            uid: uid.to_string(),
            seq,
            tiploc: "EUSTON".to_string(),
            kind: "intermediate".to_string(),
            booked_arrival: Some(time(8, 0)),
            booked_departure: Some(time(8, 2)),
            day_offset: 0,
        }
    }

    async fn clear_calling_points(pool: &PgPool, service_date: chrono::NaiveDate) {
        sqlx::query("DELETE FROM schedule_calling_points_full WHERE service_date = $1")
            .bind(service_date)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_calling_points_full rows");
    }

    async fn stored_uids(pool: &PgPool, service_date: chrono::NaiveDate) -> Vec<String> {
        sqlx::query_as::<_, (String,)>(
            "SELECT DISTINCT uid FROM schedule_calling_points_full WHERE service_date = $1 \
             ORDER BY uid",
        )
        .bind(service_date)
        .fetch_all(pool)
        .await
        .expect("read back fixture rows")
        .into_iter()
        .map(|(uid,)| uid)
        .collect()
    }

    /// **The test that makes chunking safe.** Two chunks of the SAME service
    /// date: the first clears the date, the second must add to it. If the
    /// second chunk also ran the `DELETE`, the date would end up holding only
    /// the last chunk -- silently losing every earlier chunk of the publish,
    /// which is strictly worse than the oversized-body problem chunking exists
    /// to solve.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_pipeline_integrity -- --ignored --test-threads=1`"]
    async fn a_second_calling_points_chunk_adds_to_the_date_instead_of_wiping_the_first() {
        let pool = test_pool().await;
        let date = fixture_date(1);
        clear_calling_points(&pool, date).await;

        // Chunk 1 of 2: clears the date (there is a prior delivery's row to
        // clear, seeded here so the DELETE is genuinely exercised).
        upsert_schedule_calling_points_full(&pool, &[calling_point(date, "STALE1", 0)])
            .await
            .expect("seed a prior delivery's row");
        upsert_schedule_calling_points_full_chunk(&pool, &[calling_point(date, "FRESH1", 0)], true)
            .await
            .expect("first chunk");
        // Chunk 2 of 2: must NOT clear the date.
        upsert_schedule_calling_points_full_chunk(
            &pool,
            &[calling_point(date, "FRESH2", 0)],
            false,
        )
        .await
        .expect("second chunk");

        assert_eq!(
            stored_uids(&pool, date).await,
            vec!["FRESH1".to_string(), "FRESH2".to_string()],
            "both chunks must survive, and the previous delivery's row must not"
        );

        clear_calling_points(&pool, date).await;
    }

    /// The other half of the same contract: a `replace_dates: true` batch is
    /// still a wholesale replace, unchanged -- and so is the two-argument
    /// wrapper every existing in-process caller uses.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_pipeline_integrity -- --ignored --test-threads=1`"]
    async fn a_replacing_calling_points_chunk_still_clears_the_date_first() {
        let pool = test_pool().await;
        let date = fixture_date(2);
        clear_calling_points(&pool, date).await;

        upsert_schedule_calling_points_full(&pool, &[calling_point(date, "OLD", 0)])
            .await
            .expect("seed");
        upsert_schedule_calling_points_full_chunk(&pool, &[calling_point(date, "NEW", 0)], true)
            .await
            .expect("replacing batch");

        assert_eq!(
            stored_uids(&pool, date).await,
            vec!["NEW".to_string()],
            "a replacing batch must supersede the whole date, exactly as before chunking existed"
        );

        clear_calling_points(&pool, date).await;
    }

    /// The same contract on the sibling product, which shares the chunked
    /// publisher (`post_date_scoped_rows_in_chunks`) and therefore the same
    /// per-chunk delete hazard.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_pipeline_integrity -- --ignored --test-threads=1`"]
    async fn a_second_destination_departures_chunk_adds_to_the_date_instead_of_wiping_the_first() {
        let pool = test_pool().await;
        let date = fixture_date(3);
        let clear = |pool: PgPool| async move {
            sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
                .bind(date)
                .execute(&pool)
                .await
                .expect("cleanup fixture schedule_destination_departures rows");
        };
        clear(pool.clone()).await;

        let row = |uid: &str, scheduled: chrono::NaiveTime| ScheduleDestinationDeparturesRow {
            service_date: date,
            destination_crs: "ZRD".to_string(),
            scheduled,
            day_offset: 0,
            train_uid: uid.to_string(),
            origin_crs: "EUS".to_string(),
            true_origin_crs: None,
            calling_point_arrival: None,
            destination_arrival: None,
            destination_arrival_day_offset: 0,
            operator_atoc: None,
        };

        upsert_schedule_destination_departures_chunk(&pool, &[row("FRESH1", time(8, 0))], true)
            .await
            .expect("first chunk");
        upsert_schedule_destination_departures_chunk(&pool, &[row("FRESH2", time(9, 0))], false)
            .await
            .expect("second chunk");

        let uids: Vec<String> = sqlx::query_as::<_, (String,)>(
            "SELECT train_uid FROM schedule_destination_departures WHERE service_date = $1 \
             ORDER BY train_uid",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back")
        .into_iter()
        .map(|(uid,)| uid)
        .collect();
        assert_eq!(uids, vec!["FRESH1".to_string(), "FRESH2".to_string()]);

        clear(pool.clone()).await;
    }

    /// The completion marker `schedule-reference` seeds its restart dedup from
    /// -- round-tripped, and confirmed to report the most recently COMPLETED
    /// delivery rather than whatever happens to sort last.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_pipeline_integrity -- --ignored --test-threads=1`"]
    async fn the_completion_marker_round_trips_and_reports_the_most_recent_completion() {
        let pool = test_pool().await;
        sqlx::query("DELETE FROM schedule_reference_publishes")
            .execute(&pool)
            .await
            .expect("start from an empty marker table");

        assert_eq!(
            last_completed_schedule_reference_publish(&pool)
                .await
                .expect("read empty"),
            None,
            "an empty marker table must read as None -- the first-run case \
             schedule-reference falls back on"
        );

        insert_schedule_reference_publish(&pool, "TEST-20990601T180000Z")
            .await
            .expect("first marker");
        insert_schedule_reference_publish(&pool, "TEST-20990602T180000Z")
            .await
            .expect("second marker");

        assert_eq!(
            last_completed_schedule_reference_publish(&pool)
                .await
                .expect("read back"),
            Some("TEST-20990602T180000Z".to_string()),
            "must report the most recently completed delivery"
        );

        // Re-recording an earlier delivery (its publish cycle ran again, e.g.
        // after a retry) must move it to the front -- `completed_at` is what
        // is ordered on, and it is refreshed by the upsert.
        insert_schedule_reference_publish(&pool, "TEST-20990601T180000Z")
            .await
            .expect("re-record");
        assert_eq!(
            last_completed_schedule_reference_publish(&pool)
                .await
                .expect("read back"),
            Some("TEST-20990601T180000Z".to_string()),
            "ON CONFLICT DO UPDATE must refresh completed_at, not silently do nothing"
        );

        sqlx::query("DELETE FROM schedule_reference_publishes WHERE delivery LIKE 'TEST-%'")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
