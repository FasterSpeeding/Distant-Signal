//! The `api` crate's shared library root.
//!
//! These modules used to be declared directly on `src/main.rs` (the binary
//! crate root). They were hoisted into a real library target so that a
//! SECOND binary in this crate -- `src/bin/backfill_trains.rs`, the
//! operational backfill required before the shared-train-identity contract
//! migration (`migrations/20260906140000_drop_legacy_columns.sql`) can be
//! applied to a database with pre-existing data -- can reuse exactly the
//! same, unit-tested code path the server itself does, rather than carrying
//! a second, independently-drifting copy of the same SQL.
//!
//! Nothing else moved: every module below is byte-for-byte the module it
//! was, `main.rs` still owns the server's own wiring (router, CORS,
//! metrics, `sqlx::migrate!()`), and every `crate::...` path inside these
//! modules resolves exactly as it did before.

pub mod app;
pub mod auth;
pub mod data;
pub mod render;
pub mod routes;
