//! The `api` crate's shared library root.
//!
//! These modules used to be declared directly on `src/main.rs` (the binary
//! crate root). They were hoisted into a real library target so that the
//! OTHER binaries in this crate can reuse exactly the same, unit-tested code
//! path the server itself does, rather than each carrying its own
//! independently-drifting copy of the same SQL. They are read-only reports
//! and the api-maintenance pass:
//!
//! - `src/bin/compare_full_coverage.rs` -- read-only report comparing
//!   `full-coverage-consumer`'s TRUST-vs-schedule output with the
//!   LDBWS-sample-derived output (and, with `--windows`, the windowed
//!   full-coverage stats) for one or all lines.
//! - `src/bin/corpus_compare.rs` -- read-only CORPUS report.
//! - `src/bin/maintenance.rs` -- the hourly `api-maintenance` pass.
//!
//! The binaries that WROTE outside the api's grants (`backfill_trains`,
//! `backfill_incident_lines`, `backfill_line_train_summaries`,
//! `replay_uidless_movements`) left this crate in ingest phase 5, step
//! 5.4b: they are `ds-migrate backfill-trains` /
//! `backfill-line-train-summaries` (as the schema owner) and
//! `writer-maintenance replay-uidless-movements` /
//! `backfill-incident-lines` (as the writer and incidents roles).
//!
//! Nothing else moved: every module below is byte-for-byte the module it
//! was, `main.rs` still owns the server's own wiring (router, CORS,
//! metrics, the startup migration), and every `crate::...` path inside
//! these modules resolves exactly as it did before.
//!
//! `migrate` is `ds_store::migrate` re-exported: the migrator and the
//! migrations directory moved to `ds-store` in plan task 1B.1.

pub mod app;
pub mod auth;
pub mod data;
pub mod edge;
pub mod journey_break_sweep;
pub use ds_store::migrate;
pub mod rate_limit;
pub mod readiness;
pub mod render;
pub mod route_metrics;
pub mod routes;
pub mod shutdown;
#[cfg(test)]
pub(crate) mod test_support;
pub mod unavailable;
