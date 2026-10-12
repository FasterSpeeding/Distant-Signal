//! Where a publish goes: Postgres directly ([`DbSink`], ingest
//! architecture plan 2a.2). The api's `/private` ingest routes (the
//! `HttpSink`) went in release B (ingest phase 5,
//! docs/ingest-phase5-runbook.md).
//!
//! [`PublishSink`] has one method per product (and one per chunk of the two
//! chunked products), so the protocol logic around it -- chunking, the
//! publish id, the empty-date guard, the in-cycle retry and the
//! defer-to-next-cycle rule -- lives once in `main.rs`; the tests drive it
//! through a recording fake (`fake`).
//!
//! Failures come back in one vocabulary, [`SinkError`].

pub(crate) mod db;
#[cfg(test)]
pub(crate) mod fake;

pub(crate) use db::DbSink;

/// Why a publish (or one chunk of it) failed (spec §9.1). Every variant
/// keeps its cause, so logs still show the SQLSTATE underneath.
///
/// | Variant | `DbSink` |
/// |---|---|
/// | `Busy` | `SchedulePublishBusy` (the api's 409 before release B) |
/// | `Timeout` | SQLSTATE 57014 (its 503) |
/// | `Rejected` | SQLSTATE class 22 or 23, or a refused batch (its 400, 413, 422) |
/// | `Transient` | anything else |
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

/// One chunk's place in its publish (`ds_store::schedule::SchedulePublishPart`).
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

/// One line's population for one date (the body the retired
/// `POST /private/schedule-line-population` took).
#[derive(Debug, serde::Serialize)]
pub(crate) struct LinePopulation {
    pub line_id: String,
    pub service_date: chrono::NaiveDate,
    pub population: serde_json::Value,
}

/// Where schedule-reference's products go. One method per product; see
/// the module doc. The rows of the CIF-derived products are the JSON
/// values the api's retired routes took.
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

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn sink_errors_display_their_cause() {
        let err = SinkError::Rejected(anyhow::anyhow!("bad data").context("the batch"));
        assert_eq!(err.to_string(), "the batch");
        let chain: Vec<String> = anyhow::Error::new(err)
            .chain()
            .map(ToString::to_string)
            .collect();
        assert_eq!(chain.len(), 2, "{chain:?}");
    }
}
