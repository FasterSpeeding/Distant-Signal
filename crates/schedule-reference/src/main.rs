//! `schedule-reference`: a sibling container in the `schedulefeed` Pod.
//! Once `schedule-ingest` has extracted a verified-stable delivery into
//! `storage_dir/<timestamp>/` (see
//! `docs/superpowers/specs/2026-09-03-schedule-feed-zip-delivery-correction.md`),
//! reads that delivery's `RJTTF*MCA.txt` (`TI` records) and `RJTTF*MSN.txt`
//! (`A` records) directly off the already-local, read-only-mounted PVC,
//! resolves a STANOX->CRS table, and POSTs it to `api`'s
//! `/private/stanox-crs`. See
//! docs/superpowers/specs/2026-09-01-schedule-ingest-stanox-crs-table-design.md.

mod config;
mod discovery;
mod parser;

use std::time::Duration;

use clap::Parser;
use config::Config;
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });
    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));
    let mut last_processed_delivery: Option<String> = None;

    loop {
        interval.tick().await;
        let cycle_start = std::time::Instant::now();
        let result = poll_once(
            &client,
            &config,
            &mut last_processed_delivery,
            &internal_oauth,
        )
        .await;
        metrics::histogram!(common::metrics::metric_name(
            "schedule_reference_cycle_duration_seconds"
        ))
        .record(cycle_start.elapsed().as_secs_f64());
        if let Err(err) = result {
            tracing::error!(error = ?err, "schedule-reference cycle failed; will retry next interval");
        }
    }
}

/// Streams `path` line-by-line, keeping only lines starting with `prefix`
/// -- so the real 707MB `RJTTF<n>MCA.txt` is never held in memory whole,
/// only its ~12,085 `TI` lines (the `RJTTF<n>MSN.txt` file, at ~340KB
/// total, is small enough that this matters far less for it, but the same
/// function is reused for both for one consistent code path).
fn read_prefixed_lines(path: &std::path::Path, prefix: &str) -> anyhow::Result<String> {
    use std::io::BufRead;
    let file = std::fs::File::open(path)?;
    let reader = std::io::BufReader::new(file);
    let mut out = String::new();
    for line in reader.lines() {
        let line = line?;
        if line.starts_with(prefix) {
            out.push_str(&line);
            out.push('\n');
        }
    }
    Ok(out)
}

/// Scans for the most recent complete delivery, skips if unchanged since
/// `last_processed_delivery`, else reads+parses+POSTs it and only advances
/// `last_processed_delivery` on a successful POST.
async fn poll_once(
    client: &Client,
    config: &Config,
    last_processed_delivery: &mut Option<String>,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<()> {
    let Some(delivery) = discovery::latest_complete_delivery(&config.storage_dir)? else {
        tracing::debug!("no complete MCA+MSN delivery directory found yet");
        return Ok(());
    };
    if Some(&delivery.dir_name) == last_processed_delivery.as_ref() {
        tracing::debug!(
            delivery = %delivery.dir_name,
            "no new delivery since last successful parse; nothing to do"
        );
        return Ok(());
    }

    let ti_text = read_prefixed_lines(&delivery.mca_path, "TI")?;
    let a_text = read_prefixed_lines(&delivery.msn_path, "A")?;

    let ti_records = parser::parse_ti_lines(&ti_text);
    let msn_crs = parser::parse_msn_a_lines(&a_text);
    let rows = parser::resolve(&ti_records, &msn_crs);

    tracing::info!(
        delivery = %delivery.dir_name,
        ti_records = ti_records.len(),
        resolved = rows.len(),
        "parsed stanox/crs table from delivery"
    );

    // `common::StanoxCrsRecord::source_sequence` predates this crate's own
    // zip/mtime-delivery rework and is shared with `crates/trust-consumer`
    // -- out of this fix's scope to retype. Best-effort only: the embedded
    // number in the MCA filename (e.g. the `942` in `RJTTF942MCA.txt`) is
    // NOT relied on to decide which delivery is newest (see `discovery.rs`
    // and this repo's 2026-09-03 correction note) -- it's used here purely
    // as informational provenance for this one downstream table, falling
    // back to `0` if the filename doesn't carry a parseable number.
    let source_sequence = embedded_sequence_number(&delivery.mca_path).unwrap_or(0);

    let records: Vec<common::StanoxCrsRecord> = rows
        .into_iter()
        .map(|row| common::StanoxCrsRecord {
            stanox: row.stanox,
            crs: row.crs,
            tiploc: row.tiploc,
            station_name: row.station_name,
            source_sequence,
        })
        .collect();

    common::ingest::post_batch(
        client,
        &config.api_ingest_url,
        internal_oauth,
        &records,
        "stanox/crs rows",
    )
    .await?;

    // Only advance on a successful POST -- a failed POST just means the
    // already-computed table is discarded and rebuilt from the same
    // still-local, unchanged files next cycle (cheap), matching the
    // spec's Error handling: "a failed POST just means the already-
    // computed in-memory table is discarded and rebuilt... next cycle".
    *last_processed_delivery = Some(delivery.dir_name);
    Ok(())
}

/// Best-effort extraction of the digits embedded in a real delivery's own
/// MCA filename (e.g. `942` from `RJTTF942MCA.txt`) -- see this function's
/// one call site for why this is informational only, never used to decide
/// delivery identity/recency.
fn embedded_sequence_number(mca_path: &std::path::Path) -> Option<i32> {
    let name = mca_path.file_name()?.to_str()?;
    let digits = name.strip_prefix("RJTTF")?.strip_suffix("MCA.txt")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

#[cfg(test)]
mod poll_once_tests {
    use super::*;

    #[test]
    fn read_prefixed_lines_extracts_only_matching_lines_from_a_mixed_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.txt");
        std::fs::write(
            &path,
            "HDsomething\nTIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON           \nBSsomeschedule\n",
        )
        .unwrap();

        let ti_text = read_prefixed_lines(&path, "TI").unwrap();
        assert_eq!(ti_text.lines().count(), 1);
        assert!(ti_text.starts_with("TIEUSTON"));

        let records = parser::parse_ti_lines(&ti_text);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].tiploc, "EUSTON");
    }

    #[test]
    fn embedded_sequence_number_parses_the_real_filename_shape() {
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("RJTTF942MCA.txt")),
            Some(942)
        );
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("/some/dir/RJTTF1MCA.txt")),
            Some(1)
        );
    }

    #[test]
    fn embedded_sequence_number_is_none_for_a_non_matching_shape() {
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("not-a-real-name.txt")),
            None
        );
        assert_eq!(
            embedded_sequence_number(std::path::Path::new("RJTTFabcMCA.txt")),
            None
        );
    }
}
