//! Storage for `trust_event_backlog`: moved to `ds_store::backlog`
//! (ingest architecture plan 1A.10). The api keeps only the shared-movement
//! insert its route tests seed fixtures with; the writes are
//! trust-backlog-consumer's (`db` sink) since release B removed
//! `POST /private/trust-event-backlog` (ingest phase 5).

pub use ds_store::backlog::ingest_shared_movements_batch;
