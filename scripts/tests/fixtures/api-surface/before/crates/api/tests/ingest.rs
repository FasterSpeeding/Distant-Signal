//! Fixture: an integration test; all of its literals are test code.

#[test]
fn reads_back() {
    let _ = "SELECT count(*) FROM ingest_log";
}
