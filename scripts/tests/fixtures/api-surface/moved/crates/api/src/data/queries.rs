//! Fixture for scripts/tests/test_diff_api_surface.py: the api after a move.

pub use ds_store::freshness::{FETCH_METRIC, last_fetch, record_ingest};

pub fn public_reader() -> &'static str {
    metrics::gauge!("api_reader_up").set(1.0);
    "SELECT id FROM stations WHERE crs = $1"
}
