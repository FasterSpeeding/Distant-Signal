//! The incident snapshot: `upsert_incident_snapshot` and its helpers,
//! `incident_removal.rs` (whole, as [`removal`]) and `parse_snapshot`
//! (today's `incident_snapshot_from_body`). Redis stays out: the upsert
//! takes a publish callback, and the api passes `publish_text_changed`.
//!
//! Moved here by plan task 1A.8.

pub mod removal;
