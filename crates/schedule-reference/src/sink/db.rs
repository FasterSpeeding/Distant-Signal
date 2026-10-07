//! [`DbSink`]: schedule-reference's products written straight to Postgres
//! through `ds_store`, as the `distant_signal_schedule_reference` role
//! (ingest architecture plan 2a.2, spec §9.2).
//!
//! Each method does what the api's matching `/private` route does, with
//! the same `ds_store` functions:
//!
//! | Method | The api's route | `ds_store` |
//! |---|---|---|
//! | `stanox_crs`, `tiploc_crs` | `post_stanox_crs`, `post_tiploc_crs` | `reference::upsert_*`, then `reference::prune_*_not_in` |
//! | `tiploc_locations` | `post_tiploc_locations` (400 on an empty batch) | `reference::replace_tiploc_locations` |
//! | `fixed_links` | `post_fixed_links` | `reference::upsert_fixed_links` |
//! | `line_population` | `post_schedule_line_population` | `schedule::summaries::upsert_population_with_summaries` |
//! | `network_departures` | `post_schedule_network_departures` | `schedule::upsert_schedule_network_departures` |
//! | `publish_part` | the two chunked routes and `ScheduleChunkParams` | `schedule::SchedulePublishPart`, `schedule::upsert_*_publish_part`, `schedule::finish_*_without_rows` |
//! | `services` | `post_schedule_services` | `schedule::services::replace_for_date` |
//! | `record_completed_publish`, `last_completed_publish` | `/schedule-reference-publishes` | `schedule::{insert,last_completed}_schedule_reference_publish` |
//!
//! The CIF-derived rows arrive as the JSON values the api's routes take and
//! are read into `ds_store`'s row types with the same `Deserialize`, so
//! both sinks store exactly the same thing. A row that does not read (the
//! api's 422) or a refused publish (its 400) is [`SinkError::Rejected`];
//! database errors go through [`ds_store::writes::classify`].
//!
//! Every write is counted in `db_writes_total{operation, outcome}` and
//! timed in `db_write_seconds{operation}` ([`ds_store::writes`]).

use std::time::Instant;

use ds_store::writes::WriteFailure;
use sqlx::PgPool;

use super::{ChunkPart, DatedProduct, LinePopulation, PublishSink, SinkError};

/// The `operation` labels this sink records.
pub(crate) const OPERATIONS: [&str; 9] = [
    "stanox_crs",
    "tiploc_crs",
    "tiploc_locations",
    "fixed_links",
    "line_population",
    "network_departures",
    "publish_part",
    "services",
    "publish_marker",
];

/// Postgres, through `ds_store`. `lines` is the line catalogue the
/// population's train summaries are derived against (the api uses its own
/// copy of the same catalogue).
pub(crate) struct DbSink<'a> {
    pool: PgPool,
    lines: &'a [common::LineDefinition],
}

impl<'a> DbSink<'a> {
    pub(crate) fn new(pool: PgPool, lines: &'a [common::LineDefinition]) -> Self {
        Self { pool, lines }
    }
}

/// Startup telemetry for the db sink: the publish metrics under their
/// `store_` names (spec §14.1) and every `db_writes_total` series at 0.
pub(crate) fn register_metrics() {
    ds_store::schedule::use_store_metric_names();
    ds_store::schedule::register_schedule_publish_metrics();
    ds_store::writes::register(&OPERATIONS);
}

/// A database error as a [`SinkError`] (see [`ds_store::writes::classify`]).
fn db_error(err: anyhow::Error) -> SinkError {
    match ds_store::writes::classify(&err) {
        WriteFailure::Busy => SinkError::Busy(err),
        WriteFailure::Timeout => SinkError::Timeout(err),
        WriteFailure::Rejected => SinkError::Rejected(err),
        WriteFailure::Transient => SinkError::Transient(err),
    }
}

/// A publish refused before it reached the database (the api's 400/422).
fn refused(problem: impl std::fmt::Display) -> SinkError {
    SinkError::Rejected(anyhow::anyhow!("{problem}"))
}

const fn failure_of(err: &SinkError) -> WriteFailure {
    match err {
        SinkError::Busy(_) => WriteFailure::Busy,
        SinkError::Timeout(_) => WriteFailure::Timeout,
        SinkError::Rejected(_) => WriteFailure::Rejected,
        SinkError::Transient(_) => WriteFailure::Transient,
    }
}

/// Runs one write and records it under `operation`.
async fn timed<T>(
    operation: &'static str,
    write: impl Future<Output = Result<T, SinkError>>,
) -> Result<T, SinkError> {
    let started = Instant::now();
    let result = write.await;
    ds_store::writes::record(
        operation,
        result.as_ref().err().map(failure_of),
        started.elapsed(),
    );
    result
}

/// Reads JSON rows into `T` with its own `Deserialize`, as the api's `Json`
/// extractor does; a row that does not read is the api's 422.
fn typed<T: serde::de::DeserializeOwned>(rows: &[serde_json::Value]) -> Result<Vec<T>, SinkError> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            T::deserialize(row).map_err(|err| refused(format!("row {index} does not read: {err}")))
        })
        .collect()
}

impl PublishSink for DbSink<'_> {
    async fn stanox_crs(&self, records: &[common::StanoxCrsRecord]) -> Result<(), SinkError> {
        timed("stanox_crs", async {
            ds_store::reference::upsert_stanox_crs(&self.pool, records)
                .await
                .map_err(db_error)?;
            let keep: Vec<String> = records.iter().map(|r| r.stanox.clone()).collect();
            ds_store::reference::prune_stanox_crs_not_in(&self.pool, &keep)
                .await
                .map_err(db_error)?;
            Ok(())
        })
        .await
    }

    async fn tiploc_crs(&self, records: &[common::TiplocCrsRecord]) -> Result<(), SinkError> {
        timed("tiploc_crs", async {
            ds_store::reference::upsert_tiploc_crs(&self.pool, records)
                .await
                .map_err(db_error)?;
            let keep: Vec<String> = records.iter().map(|r| r.tiploc.clone()).collect();
            ds_store::reference::prune_tiploc_crs_not_in(&self.pool, &keep)
                .await
                .map_err(db_error)?;
            Ok(())
        })
        .await
    }

    async fn tiploc_locations(
        &self,
        records: &[common::TiplocLocationRecord],
    ) -> Result<(), SinkError> {
        timed("tiploc_locations", async {
            if records.is_empty() {
                return Err(refused(
                    "refusing an empty tiploc_locations batch: it would clear the table",
                ));
            }
            ds_store::reference::replace_tiploc_locations(&self.pool, records)
                .await
                .map(drop)
                .map_err(db_error)
        })
        .await
    }

    async fn fixed_links(&self, records: &[common::FixedLinkRecord]) -> Result<(), SinkError> {
        timed("fixed_links", async {
            ds_store::reference::upsert_fixed_links(&self.pool, records)
                .await
                .map(drop)
                .map_err(db_error)
        })
        .await
    }

    async fn line_population(&self, body: &LinePopulation) -> Result<(), SinkError> {
        timed("line_population", async {
            let line = self.lines.iter().find(|l| l.id == body.line_id);
            let text = serde_json::to_string(&body.population)
                .map_err(|err| refused(format!("population does not serialize: {err}")))?;
            let started = Instant::now();
            let outcome = ds_store::schedule::summaries::upsert_population_with_summaries(
                &self.pool,
                line,
                &body.line_id,
                body.service_date,
                text.into_boxed_str(),
            )
            .await
            .map_err(db_error)?;
            if let Some(rows) = outcome.summaries_written {
                tracing::info!(
                    line_id = %body.line_id,
                    service_date = %body.service_date,
                    population_changed = outcome.population_changed,
                    rows,
                    elapsed_ms = started.elapsed().as_millis(),
                    "line_train_summaries rewritten"
                );
            }
            Ok(())
        })
        .await
    }

    async fn network_departures(&self, rows: &[serde_json::Value]) -> Result<(), SinkError> {
        timed("network_departures", async {
            let rows: Vec<ds_store::schedule::ScheduleNetworkDeparturesRow> = typed(rows)?;
            ds_store::schedule::upsert_schedule_network_departures(&self.pool, &rows)
                .await
                .map(drop)
                .map_err(db_error)
        })
        .await
    }

    async fn publish_part(
        &self,
        product: DatedProduct,
        rows: &[serde_json::Value],
        part: ChunkPart<'_>,
    ) -> Result<(), SinkError> {
        use ds_store::schedule as store;
        timed("publish_part", async {
            let total_rows = part.final_total_rows.map(|n| n as u64);
            let last_chunk = total_rows.is_some();
            let publish_part = store::SchedulePublishPart::new(
                Some(part.publish_id),
                part.first_chunk,
                last_chunk,
                total_rows,
            )
            .map_err(refused)?;
            let written = if rows.is_empty() {
                let service_date = store::SchedulePublishPart::empty_publish_date(
                    last_chunk,
                    total_rows,
                    part.empty_service_date,
                )
                .map_err(refused)?;
                match product {
                    DatedProduct::DestinationDepartures => {
                        store::finish_schedule_destination_departures_publish_without_rows(
                            &self.pool,
                            publish_part,
                            service_date,
                        )
                        .await
                    }
                    DatedProduct::CallingPointsFull => {
                        store::finish_schedule_calling_points_full_publish_without_rows(
                            &self.pool,
                            publish_part,
                            service_date,
                        )
                        .await
                    }
                }
            } else {
                match product {
                    DatedProduct::DestinationDepartures => {
                        let rows: Vec<store::ScheduleDestinationDeparturesRow> = typed(rows)?;
                        store::upsert_schedule_destination_departures_publish_part(
                            &self.pool,
                            &rows,
                            publish_part,
                        )
                        .await
                    }
                    DatedProduct::CallingPointsFull => {
                        let rows: Vec<store::ScheduleCallingPointsFullRow> = typed(rows)?;
                        store::upsert_schedule_calling_points_full_publish_part(
                            &self.pool,
                            &rows,
                            publish_part,
                        )
                        .await
                    }
                }
            };
            written.map(drop).map_err(db_error)
        })
        .await
    }

    async fn services(
        &self,
        service_date: chrono::NaiveDate,
        rows: &[serde_json::Value],
    ) -> Result<(), SinkError> {
        timed("services", async {
            let rows: Vec<ds_store::schedule::services::ScheduleServiceRow> = typed(rows)?;
            ds_store::schedule::services::replace_for_date(&self.pool, service_date, &rows)
                .await
                .map(drop)
                .map_err(|err| {
                    if err.is::<ds_store::schedule::services::InvalidPublish>() {
                        SinkError::Rejected(err)
                    } else {
                        db_error(err)
                    }
                })
        })
        .await
    }

    async fn record_completed_publish(&self, delivery: &str) -> Result<(), SinkError> {
        timed("publish_marker", async {
            ds_store::schedule::insert_schedule_reference_publish(&self.pool, delivery)
                .await
                .map_err(db_error)
        })
        .await
    }

    async fn last_completed_publish(&self) -> Result<Option<String>, SinkError> {
        ds_store::schedule::last_completed_schedule_reference_publish(&self.pool)
            .await
            .map_err(db_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refused_and_unreadable_rows_are_rejections() {
        let rows = [serde_json::json!({ "crs": "EUS" })];
        let err = typed::<ds_store::schedule::ScheduleNetworkDeparturesRow>(&rows)
            .expect_err("no service_date or departures");
        assert!(matches!(err, SinkError::Rejected(_)), "{err:?}");
        assert!(err.to_string().starts_with("row 0 does not read"), "{err}");
    }

    #[test]
    fn database_failures_keep_their_class() {
        let busy = anyhow::Error::new(ds_store::schedule::SchedulePublishBusy {
            product: "schedule_calling_points_full",
        });
        assert!(matches!(db_error(busy), SinkError::Busy(_)));
        assert!(matches!(
            db_error(anyhow::Error::new(sqlx::Error::PoolTimedOut)),
            SinkError::Transient(_)
        ));
    }
}
