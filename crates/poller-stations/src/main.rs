//! `poller-stations`: polls the RDM Stations JSON feed on an interval and
//! forwards parsed `StationReference`s to the `api` crate's
//! `/private/stations` ingestion endpoint.
//!
//! See `.superpowers/sdd/task-4-brief.md` for the RDM facts this is built
//! against (RSPS5050 P-03-00 Rev A, §6) — this is the best-documented of
//! the three RDM products: the `/stations` endpoint path and the 24-hour
//! poll frequency are both confirmed. The one open gap is the exact JSON
//! field casing (see `schema.rs` module docs for how that's handled).

mod config;
mod schema;

use std::time::Duration;

use clap::Parser;
use common::StationReference;
use config::Config;
use reqwest::Client;

/// Header RDM uses for API-key auth. Not stated specifically for the
/// Stations product in RSPS5050 P-03-00 Rev A §6 ("An API Key will be
/// required to access the JSON feed via RDM" — no header name given); this
/// is the same working assumption used for `poller-incidents` (Task 3),
/// isolated behind this one constant so it's a one-line fix if it turns out
/// wrong for this product.
const RDM_AUTH_HEADER_NAME: &str = "x-apikey";

/// Must match `crates/api/src/auth.rs`'s `INTERNAL_TOKEN_HEADER`.
const INTERNAL_TOKEN_HEADER: &str = "x-internal-token";

/// Per-request timeout for both the RDM fetch and the ingestion POST. A
/// peer that accepts the TCP connection but never responds would otherwise
/// hang `poll_once` forever, silently ending the "log and keep the loop
/// alive" resilience the poll loop relies on. 30s is comfortably short
/// relative to the 24-hour recommended poll interval for this feed.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;

    let mut interval = tokio::time::interval(Duration::from_secs(config.poll_interval_secs));

    loop {
        interval.tick().await;

        if let Err(err) = poll_once(&client, &config).await {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}

async fn poll_once(client: &Client, config: &Config) -> anyhow::Result<()> {
    let body = fetch_stations_json(client, config).await?;
    let stations = schema::parse_stations(&body)?;

    tracing::info!(count = stations.len(), "parsed stations from RDM feed");

    post_stations(client, config, &stations).await?;

    Ok(())
}

async fn fetch_stations_json(client: &Client, config: &Config) -> anyhow::Result<String> {
    let response = client
        .get(format!("{}/stations", config.rdm_stations_base_url))
        .header(RDM_AUTH_HEADER_NAME, &config.rdm_api_key)
        .send()
        .await?
        .error_for_status()?;

    let body = response.text().await?;

    // GAP: the JSON field casing for this feed is unconfirmed (see
    // `schema.rs` module docs). Logging the raw body here, before parsing,
    // is the mechanism for resolving that gap on a real run: enable
    // `RUST_LOG=poller_stations=debug`, inspect the logged body against a
    // known station (e.g. `EUS`), and adjust `schema::RdmStation`'s
    // `rename_all`/per-field `rename` attributes if reality differs.
    tracing::debug!(body = %body, "raw stations response body");

    Ok(body)
}

async fn post_stations(
    client: &Client,
    config: &Config,
    stations: &[StationReference],
) -> anyhow::Result<()> {
    let response = client
        .post(&config.api_ingest_url)
        .header(INTERNAL_TOKEN_HEADER, &config.internal_token)
        .json(stations)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::info!(count = stations.len(), "posted stations to ingestion API");
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("ingestion POST failed: {status} {text}");
    }
}
