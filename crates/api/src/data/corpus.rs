//! Network Rail CORPUS location reference data (`corpus_locations`,
//! `corpus_deliveries`), loaded by `schedule-ingest` through
//! `POST /private/corpus-locations`. See
//! `crates/ds-store/migrations/20260928100000_corpus_locations.sql` and
//! docs/superpowers/specs/2026-09-28-corpus-sftp-ingest-design.md.
//!
//! Every load also rebuilds the CORPUS-derived crosswalk in the same
//! transaction ([`crate::data::corpus_crosswalk`]), which only the
//! off-by-default lookup fallback reads, and the route logs a comparison
//! against the timetable crosswalk ([`crate::data::corpus_comparison`]).

// Moved to `ds_store::corpus` (ingest architecture plan 1A.6).
pub use ds_store::corpus::{
    CorpusLocation, DeliveredFileProvenance, LAST_DELIVERY_METRIC, last_corpus_delivery,
    refresh_last_delivery_metric, replace_corpus_locations,
    replace_corpus_locations_with_provenance,
};
