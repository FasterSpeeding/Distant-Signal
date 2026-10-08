//! Where a publish goes: the api's `/private` ingest routes ([`HttpSink`],
//! today's path) or Postgres directly (`DbSink`, ingest architecture plan
//! 2a.2). `INGEST_SINK` picks one at startup (spec §9.1, §13.1).
//!
//! [`PublishSink`] has one method per product (and one per chunk of the two
//! chunked products), so the protocol logic around it -- chunking, the
//! publish id, the empty-date guard, the in-cycle retry and the
//! defer-to-next-cycle rule -- lives once in `main.rs` and runs unchanged
//! whichever sink is active.
//!
//! Both sinks speak one error vocabulary, [`SinkError`], so that logic sees
//! the same outcomes from either.

pub(crate) mod db;

use std::time::Duration;

pub(crate) use db::DbSink;
use reqwest::Client;

use crate::config::Config;

/// Why a publish (or one chunk of it) failed, the same for both sinks
/// (spec §9.1). Every variant keeps its cause, so logs still show the HTTP
/// status or the SQLSTATE underneath.
///
/// | Variant | `HttpSink` | `DbSink` |
/// |---|---|---|
/// | `Busy` | 409 | `SchedulePublishBusy` |
/// | `Timeout` | 503, or this client's own request timeout | SQLSTATE 57014 |
/// | `Rejected` | 400, 413, 422 | SQLSTATE class 22 or 23, or a refused batch |
/// | `Transient` | anything else | anything else |
///
/// `Busy` and `Timeout` on a final chunk mean "the delete did not run; try
/// the whole publish next cycle" (`main::DeferToNextCycle`). Anywhere else
/// they are retried like `Transient`. `Rejected` is never retried for the
/// same delivery.
#[derive(Debug)]
pub(crate) enum SinkError {
    Busy(anyhow::Error),
    Timeout(anyhow::Error),
    Rejected(anyhow::Error),
    Transient(anyhow::Error),
}

impl SinkError {
    fn cause(&self) -> &anyhow::Error {
        match self {
            Self::Busy(err) | Self::Timeout(err) | Self::Rejected(err) | Self::Transient(err) => {
                err
            }
        }
    }

    /// Whether a FINAL chunk that failed this way may still be (or just
    /// was) deleting server-side, so an in-cycle retry would only add load
    /// (the 2026-09-27 incident).
    pub(crate) fn defers_final_chunk(&self) -> bool {
        matches!(self, Self::Busy(_) | Self::Timeout(_))
    }
}

/// Transparent: shows its cause, and the cause's own sources follow it.
impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.cause(), f)
    }
}

impl std::error::Error for SinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause().source()
    }
}

/// Classifies a publish failure for `CycleOutcome` and the in-cycle retry:
/// a [`SinkError::Rejected`] anywhere in the chain is a rejection of the
/// data, any other [`SinkError`] is transient, and an error from outside a
/// sink falls back to [`common::ingest::classify_failure`].
pub(crate) fn classify(err: &anyhow::Error) -> common::ingest::FailureClass {
    match err
        .chain()
        .find_map(|cause| cause.downcast_ref::<SinkError>())
    {
        Some(SinkError::Rejected(_)) => common::ingest::FailureClass::Rejected,
        Some(_) => common::ingest::FailureClass::Transient,
        None => common::ingest::classify_failure(err),
    }
}

/// The two products published per date in chunks under the diff protocol
/// (`ds_store::schedule::SchedulePublishPart`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DatedProduct {
    DestinationDepartures,
    CallingPointsFull,
}

impl DatedProduct {
    /// The rows' noun in log and error messages.
    pub(crate) const fn noun(self) -> &'static str {
        match self {
            Self::DestinationDepartures => "schedule-derived destination departures rows",
            Self::CallingPointsFull => "schedule-derived full calling-point rows",
        }
    }
}

/// One chunk's place in its publish (the api's `ScheduleChunkParams`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct ChunkPart<'a> {
    pub publish_id: &'a str,
    pub first_chunk: bool,
    /// `Some(total rows across every chunk)` on the final chunk only.
    pub final_total_rows: Option<usize>,
    /// The date an empty publish (no rows, `total_rows=0`) clears (PL-14).
    /// `None` for every chunk that carries rows.
    pub empty_service_date: Option<chrono::NaiveDate>,
}

/// One line's population for one date: the body of
/// `POST /private/schedule-line-population`.
#[derive(Debug, serde::Serialize)]
pub(crate) struct LinePopulation {
    pub line_id: String,
    pub service_date: chrono::NaiveDate,
    pub population: serde_json::Value,
}

/// Where schedule-reference's products go. One method per product; see
/// the module doc. The rows of the CIF-derived products are the JSON
/// values the api's routes take, so both sinks publish the same thing.
pub(crate) trait PublishSink {
    /// The STANOX/CRS crosswalk: upsert, then prune what this delivery
    /// does not list.
    async fn stanox_crs(&self, records: &[common::StanoxCrsRecord]) -> Result<(), SinkError>;

    /// The TIPLOC/CRS crosswalk: upsert, then prune.
    async fn tiploc_crs(&self, records: &[common::TiplocCrsRecord]) -> Result<(), SinkError>;

    /// The whole `tiploc_locations` table. An empty batch is `Rejected`.
    async fn tiploc_locations(
        &self,
        records: &[common::TiplocLocationRecord],
    ) -> Result<(), SinkError>;

    /// The whole `fixed_links` table, as a multiset diff.
    async fn fixed_links(&self, records: &[common::FixedLinkRecord]) -> Result<(), SinkError>;

    /// One line's population for one date (and its train summaries).
    async fn line_population(&self, body: &LinePopulation) -> Result<(), SinkError>;

    /// The per-station network departures boards.
    async fn network_departures(&self, rows: &[serde_json::Value]) -> Result<(), SinkError>;

    /// One chunk of a per-date diff publish of `product`.
    async fn publish_part(
        &self,
        product: DatedProduct,
        rows: &[serde_json::Value],
        part: ChunkPart<'_>,
    ) -> Result<(), SinkError>;

    /// `service_date`'s whole `schedule_services` set (empty clears it).
    async fn services(
        &self,
        service_date: chrono::NaiveDate,
        rows: &[serde_json::Value],
    ) -> Result<(), SinkError>;

    /// Writes this service's durable "delivery fully published" marker.
    async fn record_completed_publish(&self, delivery: &str) -> Result<(), SinkError>;

    /// Reads that marker back: the newest delivery whose publish completed.
    async fn last_completed_publish(&self) -> Result<Option<String>, SinkError>;
}

/// The api's `/private` ingest routes: today's code, unchanged (spec
/// §9.1). Every URL comes from [`Config`], as before.
pub(crate) struct HttpSink<'a> {
    client: &'a Client,
    tokens: &'a common::oauth_client::OAuthTokenCache,
    urls: HttpUrls,
    /// The timeout of a final chunk (and of the single-request
    /// `schedule_services` publish), overriding the client's own: see
    /// `main::FINAL_CHUNK_REQUEST_TIMEOUT`.
    final_chunk_timeout: Duration,
}

#[derive(Debug, Clone)]
struct HttpUrls {
    stanox_crs: String,
    tiploc_crs: String,
    tiploc_locations: String,
    fixed_links: String,
    line_population: String,
    network_departures: String,
    destination_departures: String,
    calling_points_full: String,
    services: String,
    publishes: String,
}

impl<'a> HttpSink<'a> {
    pub(crate) fn new(
        client: &'a Client,
        config: &Config,
        tokens: &'a common::oauth_client::OAuthTokenCache,
        final_chunk_timeout: Duration,
    ) -> Self {
        Self {
            client,
            tokens,
            urls: HttpUrls {
                stanox_crs: config.api_ingest_url.clone(),
                tiploc_crs: config.tiploc_crs_url.clone(),
                tiploc_locations: config.tiploc_locations_url.clone(),
                fixed_links: config.fixed_links_url.clone(),
                line_population: config.schedule_line_population_url.clone(),
                network_departures: config.schedule_network_departures_url.clone(),
                destination_departures: config.schedule_destination_departures_url.clone(),
                calling_points_full: config.schedule_calling_points_full_url.clone(),
                services: config.schedule_services_url.clone(),
                publishes: config.schedule_reference_publishes_url.clone(),
            },
            final_chunk_timeout,
        }
    }

    /// Points both chunked products at `url` (the chunk-protocol tests
    /// mount one route for them).
    #[cfg(test)]
    pub(crate) fn with_dated_url(mut self, url: &str) -> Self {
        url.clone_into(&mut self.urls.destination_departures);
        url.clone_into(&mut self.urls.calling_points_full);
        self
    }

    fn dated_url(&self, product: DatedProduct) -> &str {
        match product {
            DatedProduct::DestinationDepartures => &self.urls.destination_departures,
            DatedProduct::CallingPointsFull => &self.urls.calling_points_full,
        }
    }

    async fn post_batch<T: serde::Serialize>(
        &self,
        url: &str,
        items: &[T],
        noun: &str,
        timeout: Option<Duration>,
    ) -> Result<(), SinkError> {
        common::ingest::post_batch_with_timeout(self.client, url, self.tokens, items, noun, timeout)
            .await
            .map_err(http_error)
    }
}

/// Maps an `HttpSink` failure to [`SinkError`] (spec §9.1): this client's
/// own timeout and 503 are `Timeout`, 409 is `Busy`, the statuses
/// [`common::ingest::classify_failure`] calls a rejection of the data
/// (400, 413, 422) are `Rejected`, and everything else (other statuses,
/// connection errors, the token fetch) is `Transient`, exactly as today's
/// retry logic treated them.
pub(crate) fn http_error(err: anyhow::Error) -> SinkError {
    let timed_out = err.chain().any(|cause| {
        cause
            .downcast_ref::<reqwest::Error>()
            .is_some_and(reqwest::Error::is_timeout)
    });
    let status = err.chain().find_map(|cause| {
        cause
            .downcast_ref::<common::ingest::HttpStatusError>()
            .map(|e| e.status)
    });
    if timed_out || status == Some(reqwest::StatusCode::SERVICE_UNAVAILABLE) {
        return SinkError::Timeout(err);
    }
    if status == Some(reqwest::StatusCode::CONFLICT) {
        return SinkError::Busy(err);
    }
    match common::ingest::classify_failure(&err) {
        common::ingest::FailureClass::Rejected => SinkError::Rejected(err),
        common::ingest::FailureClass::Transient => SinkError::Transient(err),
    }
}

/// `url` plus one more query parameter, whether or not it already has a
/// query string (these URLs come from configuration).
fn with_query(url: &str, param: &str) -> String {
    let separator = if url.contains('?') { '&' } else { '?' };
    format!("{url}{separator}{param}")
}

/// Appends the `first_chunk` query parameter of the chunk protocol.
pub(crate) fn first_chunk_url(url: &str, first_chunk: bool) -> String {
    with_query(url, &format!("first_chunk={first_chunk}"))
}

/// The full per-chunk URL: [`first_chunk_url`] plus the diff protocol's
/// `publish_id`, and on the final chunk (`final_total_rows: Some(total)`)
/// `last_chunk=true` and `total_rows`.
#[expect(
    clippy::format_push_string,
    reason = "short strings off the hot path; format! reads clearer"
)]
pub(crate) fn diff_chunk_url(
    url: &str,
    publish_id: &str,
    first_chunk: bool,
    final_total_rows: Option<usize>,
) -> String {
    let mut chunk_url = format!(
        "{}&publish_id={publish_id}",
        first_chunk_url(url, first_chunk)
    );
    if let Some(total_rows) = final_total_rows {
        chunk_url.push_str(&format!("&last_chunk=true&total_rows={total_rows}"));
    }
    chunk_url
}

/// The URL of an empty publish (PL-14): the first and the final chunk at
/// once, with `total_rows=0` and the date it clears.
fn empty_publish_url(url: &str, publish_id: &str, service_date: chrono::NaiveDate) -> String {
    with_query(
        url,
        &format!(
            "first_chunk=true&publish_id={publish_id}&last_chunk=true&total_rows=0\
             &service_date={service_date}"
        ),
    )
}

impl PublishSink for HttpSink<'_> {
    async fn stanox_crs(&self, records: &[common::StanoxCrsRecord]) -> Result<(), SinkError> {
        self.post_batch(&self.urls.stanox_crs, records, "stanox/crs rows", None)
            .await
    }

    async fn tiploc_crs(&self, records: &[common::TiplocCrsRecord]) -> Result<(), SinkError> {
        self.post_batch(&self.urls.tiploc_crs, records, "tiploc/crs rows", None)
            .await
    }

    async fn tiploc_locations(
        &self,
        records: &[common::TiplocLocationRecord],
    ) -> Result<(), SinkError> {
        self.post_batch(
            &self.urls.tiploc_locations,
            records,
            "tiploc location rows",
            None,
        )
        .await
    }

    async fn fixed_links(&self, records: &[common::FixedLinkRecord]) -> Result<(), SinkError> {
        self.post_batch(&self.urls.fixed_links, records, "fixed-link rows", None)
            .await
    }

    /// A single-object POST (not a batch array): `post_batch` serializes a
    /// slice as a JSON array, which does not fit this route's body.
    async fn line_population(&self, body: &LinePopulation) -> Result<(), SinkError> {
        use anyhow::Context as _;
        common::ingest::post_json(self.client, &self.urls.line_population, self.tokens, body)
            .await
            .context("schedule-line-population POST failed")
            .map_err(http_error)
    }

    async fn network_departures(&self, rows: &[serde_json::Value]) -> Result<(), SinkError> {
        self.post_batch(
            &self.urls.network_departures,
            rows,
            "schedule-derived network departures rows",
            None,
        )
        .await
    }

    async fn publish_part(
        &self,
        product: DatedProduct,
        rows: &[serde_json::Value],
        part: ChunkPart<'_>,
    ) -> Result<(), SinkError> {
        let url = self.dated_url(product);
        let (url, timeout) = match (rows.is_empty(), part.empty_service_date) {
            (true, Some(service_date)) => (
                empty_publish_url(url, part.publish_id, service_date),
                Some(self.final_chunk_timeout),
            ),
            _ => (
                diff_chunk_url(
                    url,
                    part.publish_id,
                    part.first_chunk,
                    part.final_total_rows,
                ),
                part.final_total_rows.map(|_| self.final_chunk_timeout),
            ),
        };
        self.post_batch(&url, rows, product.noun(), timeout).await
    }

    async fn services(
        &self,
        service_date: chrono::NaiveDate,
        rows: &[serde_json::Value],
    ) -> Result<(), SinkError> {
        let url = with_query(&self.urls.services, &format!("service_date={service_date}"));
        self.post_batch(
            &url,
            rows,
            "schedule service-mode rows",
            Some(self.final_chunk_timeout),
        )
        .await
    }

    async fn record_completed_publish(&self, delivery: &str) -> Result<(), SinkError> {
        let body = common::ingest::ScheduleReferencePublishRequest {
            delivery: delivery.to_string(),
        };
        common::ingest::post_json(self.client, &self.urls.publishes, self.tokens, &body)
            .await
            .map_err(http_error)
    }

    async fn last_completed_publish(&self) -> Result<Option<String>, SinkError> {
        common::ingest::get_json::<common::ingest::LastCompletedPublishResponse>(
            self.client,
            &self.urls.publishes,
            self.tokens,
        )
        .await
        .map(|response| response.delivery)
        .map_err(http_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_error(status: u16) -> anyhow::Error {
        anyhow::Error::new(common::ingest::HttpStatusError {
            prefix: "POST failed",
            status: reqwest::StatusCode::from_u16(status).unwrap(),
            body: String::new(),
            retry_after: None,
        })
    }

    #[test]
    fn http_statuses_map_to_the_shared_vocabulary() {
        assert!(matches!(http_error(status_error(409)), SinkError::Busy(_)));
        assert!(matches!(
            http_error(status_error(503)),
            SinkError::Timeout(_)
        ));
        for rejected in [400, 413, 422] {
            assert!(
                matches!(http_error(status_error(rejected)), SinkError::Rejected(_)),
                "{rejected}"
            );
        }
        // Not a rejection of the data: an auth failure or a missing route is
        // retried, as it always was.
        for transient in [401, 403, 404, 500, 502] {
            assert!(
                matches!(http_error(status_error(transient)), SinkError::Transient(_)),
                "{transient}"
            );
        }
    }

    #[test]
    fn only_busy_and_timeout_defer_a_final_chunk() {
        assert!(SinkError::Busy(anyhow::anyhow!("x")).defers_final_chunk());
        assert!(SinkError::Timeout(anyhow::anyhow!("x")).defers_final_chunk());
        assert!(!SinkError::Rejected(anyhow::anyhow!("x")).defers_final_chunk());
        assert!(!SinkError::Transient(anyhow::anyhow!("x")).defers_final_chunk());
    }

    #[test]
    fn classify_reads_the_sink_error_through_context() {
        let rejected =
            anyhow::Error::new(SinkError::Rejected(anyhow::anyhow!("bad"))).context("chunk 1/1");
        assert_eq!(classify(&rejected), common::ingest::FailureClass::Rejected);
        let busy = anyhow::Error::new(SinkError::Busy(anyhow::anyhow!("busy"))).context("chunk");
        assert_eq!(classify(&busy), common::ingest::FailureClass::Transient);
        // Outside a sink: the HTTP status decides, as before.
        assert_eq!(
            classify(&status_error(422)),
            common::ingest::FailureClass::Rejected
        );
    }

    #[test]
    fn sink_errors_display_their_cause() {
        let err = SinkError::Rejected(status_error(400).context("the batch"));
        assert_eq!(err.to_string(), "the batch");
        let chain: Vec<String> = anyhow::Error::new(err)
            .chain()
            .map(ToString::to_string)
            .collect();
        assert_eq!(chain.len(), 2, "{chain:?}");
    }

    #[test]
    fn the_empty_publish_url_carries_the_date_it_clears() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 27).unwrap();
        assert_eq!(
            empty_publish_url("http://api/private/x", "sr-1", date),
            "http://api/private/x?first_chunk=true&publish_id=sr-1&last_chunk=true&total_rows=0\
             &service_date=2026-09-27"
        );
        assert_eq!(
            empty_publish_url("http://api/private/x?trace=1", "sr-1", date),
            "http://api/private/x?trace=1&first_chunk=true&publish_id=sr-1&last_chunk=true\
             &total_rows=0&service_date=2026-09-27"
        );
    }
}
