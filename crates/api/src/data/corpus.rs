//! Network Rail CORPUS location reference data (`corpus_locations`,
//! `corpus_deliveries`), loaded by `schedule-ingest` straight into Postgres
//! (`ds_store::corpus`; the api's `POST /private/corpus-locations` went in
//! release B, ingest phase 5). See
//! `crates/ds-store/migrations/20260928100000_corpus_locations.sql` and
//! docs/superpowers/specs/2026-09-28-corpus-sftp-ingest-design.md.

// Moved to `ds_store::corpus` (ingest architecture plan 1A.6).
pub use ds_store::corpus::{
    CorpusLocation, last_corpus_delivery, refresh_last_delivery_metric, replace_corpus_locations,
};
