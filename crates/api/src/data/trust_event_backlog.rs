// crates/api/src/data/trust_event_backlog.rs
//! Storage for `trust_event_backlog`
//! (docs/superpowers/plans/2026-09-05-trust-event-backlog-plan.md). Write
//! side only -- Task 5 (`schedule_matching.rs` or a new sibling module)
//! owns the read/consumption side.

// Moved to ds_store::backlog (ingest architecture plan 1A.10)
pub(crate) use ds_store::backlog::classify_anyhow_data_error;
pub use ds_store::backlog::{
    BacklogBatchOutcome, UID_INFERRED_METRIC, UidlessReplayReport, ingest_shared_movement,
    ingest_shared_movements_batch, register_uid_inference_metrics, replay_uidless_backlog,
    upsert_trust_event_backlog_batch,
};
