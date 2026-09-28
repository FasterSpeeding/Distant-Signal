//! The `api` crate's shared library root.
//!
//! These modules used to be declared directly on `src/main.rs` (the binary
//! crate root). They were hoisted into a real library target so that the
//! OTHER binaries in this crate can reuse exactly the same, unit-tested code
//! path the server itself does, rather than each carrying its own
//! independently-drifting copy of the same SQL. Three of them exist today,
//! all operational one-offs:
//!
//! - `src/bin/backfill_trains.rs` -- required before the
//!   shared-train-identity contract migration
//!   (`migrations/20260906140000_drop_legacy_columns.sql`) can be applied to
//!   a database with pre-existing data.
//! - `src/bin/backfill_incident_lines.rs` -- fills `incidents.affected_lines`
//!   for rows ingested before that column existed, without which the
//!   incident archive's Line filter cannot find them (see
//!   `docs/incident-affected-lines-backfill.md`).
//! - `src/bin/compare_full_coverage.rs` -- read-only report comparing
//!   `full-coverage-consumer`'s TRUST-vs-schedule output with the
//!   LDBWS-sample-derived output (and, with `--windows`, the windowed
//!   full-coverage stats) for one or all lines.
//!
//! Nothing else moved: every module below is byte-for-byte the module it
//! was, `main.rs` still owns the server's own wiring (router, CORS,
//! metrics, `sqlx::migrate!()`), and every `crate::...` path inside these
//! modules resolves exactly as it did before.

pub mod app;
pub mod auth;
pub mod data;
pub mod edge;
pub mod migrate;
pub mod rate_limit;
pub mod render;
pub mod routes;
#[cfg(test)]
pub(crate) mod test_support;
