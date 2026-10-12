//! A recording, scriptable [`PublishSink`] for the publish-protocol tests.
//!
//! It stands in for the database the way a mock api stood in for the
//! retired `HttpSink` (release B, ingest phase 5,
//! docs/ingest-phase5-runbook.md): every write is recorded as an endpoint
//! name, its JSON body and the chunk protocol's parameters in the old
//! route's query-string form, and any endpoint can be scripted to fail.

use std::future::{Future, ready};
use std::sync::Mutex;

use super::{ChunkPart, DatedProduct, LinePopulation, PublishSink, SinkError};

pub(crate) const STANOX_CRS: &str = "stanox-crs";
pub(crate) const TIPLOC_CRS: &str = "tiploc-crs";
pub(crate) const TIPLOC_LOCATIONS: &str = "tiploc-locations";
pub(crate) const FIXED_LINKS: &str = "fixed-links";
pub(crate) const LINE_POPULATION: &str = "schedule-line-population";
pub(crate) const NETWORK_DEPARTURES: &str = "schedule-network-departures";
pub(crate) const DESTINATION_DEPARTURES: &str = "schedule-destination-departures";
pub(crate) const CALLING_POINTS_FULL: &str = "schedule-calling-points-full";
pub(crate) const SERVICES: &str = "schedule-services";
/// The durable completion marker's write.
pub(crate) const PUBLISHES: &str = "schedule-reference-publishes";
/// The completion marker's read (the startup seed).
pub(crate) const LAST_PUBLISH: &str = "schedule-reference-publishes (read)";

/// The endpoint a chunked product's parts are recorded under.
pub(crate) const fn dated_endpoint(product: DatedProduct) -> &'static str {
    match product {
        DatedProduct::DestinationDepartures => DESTINATION_DEPARTURES,
        DatedProduct::CallingPointsFull => CALLING_POINTS_FULL,
    }
}

/// How a scripted write fails ([`SinkError`]'s variants).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Fail {
    /// Postgres unreachable, or any other retryable error.
    Transient,
    /// Another final chunk of the product is still deleting.
    Busy,
    /// The final chunk's delete hit its statement timeout.
    Timeout,
    /// The data itself was refused.
    Rejected,
}

impl Fail {
    fn error(self, endpoint: &str) -> SinkError {
        let cause = anyhow::anyhow!("{endpoint}: scripted {self:?} failure");
        match self {
            Self::Transient => SinkError::Transient(cause),
            Self::Busy => SinkError::Busy(cause),
            Self::Timeout => SinkError::Timeout(cause),
            Self::Rejected => SinkError::Rejected(cause),
        }
    }
}

/// One recorded write (or read).
#[derive(Debug, Clone)]
pub(crate) struct Call {
    pub endpoint: &'static str,
    /// The rows (a JSON array) or the single object written; `null` for a
    /// read.
    pub body: serde_json::Value,
    /// The chunk protocol's parameters as the retired route's query string
    /// (`first_chunk=true&publish_id=...`), `service_date=...` for
    /// `schedule_services`, else empty.
    pub query: String,
}

#[derive(Debug)]
struct Script {
    endpoint: &'static str,
    fail: Fail,
    remaining: u64,
    final_chunk_only: bool,
}

/// See the module doc. Every write succeeds unless scripted otherwise; the
/// marker read answers [`FakePublishSink::with_last_completed`]'s value.
#[derive(Debug, Default)]
pub(crate) struct FakePublishSink {
    calls: Mutex<Vec<Call>>,
    scripts: Mutex<Vec<Script>>,
    last_completed: Mutex<Option<String>>,
}

impl FakePublishSink {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// The marker the startup seed reads.
    pub(crate) fn with_last_completed(self, delivery: Option<&str>) -> Self {
        *self.last_completed.lock().unwrap() = delivery.map(str::to_owned);
        self
    }

    /// The next `times` calls to `endpoint` fail with `fail` (`u64::MAX`:
    /// every call). Scripts are consumed in the order they were added.
    pub(crate) fn fail(&self, endpoint: &'static str, times: u64, fail: Fail) {
        self.script(endpoint, times, fail, false);
    }

    /// As [`Self::fail`], for final chunks (`last_chunk=true`) only.
    pub(crate) fn fail_final_chunks(&self, endpoint: &'static str, times: u64, fail: Fail) {
        self.script(endpoint, times, fail, true);
    }

    fn script(&self, endpoint: &'static str, times: u64, fail: Fail, final_chunk_only: bool) {
        self.scripts.lock().unwrap().push(Script {
            endpoint,
            fail,
            remaining: times,
            final_chunk_only,
        });
    }

    /// Forgets every recorded call and script: the database recovered.
    pub(crate) fn reset(&self) {
        self.calls.lock().unwrap().clear();
        self.scripts.lock().unwrap().clear();
    }

    /// Every call so far, in order.
    pub(crate) fn calls(&self) -> Vec<Call> {
        self.calls.lock().unwrap().clone()
    }

    /// The calls to `endpoint`, in order.
    pub(crate) fn calls_to(&self, endpoint: &str) -> Vec<Call> {
        self.calls()
            .into_iter()
            .filter(|call| call.endpoint == endpoint)
            .collect()
    }

    /// How many calls `endpoint` got (failed ones included).
    pub(crate) fn count(&self, endpoint: &str) -> usize {
        self.calls_to(endpoint).len()
    }

    /// Records the call, then answers it from the scripts.
    fn answer(
        &self,
        endpoint: &'static str,
        body: serde_json::Value,
        query: String,
    ) -> Result<(), SinkError> {
        let final_chunk = query.contains("last_chunk=true");
        self.calls.lock().unwrap().push(Call {
            endpoint,
            body,
            query,
        });
        let mut scripts = self.scripts.lock().unwrap();
        let script = scripts.iter_mut().find(|script| {
            script.endpoint == endpoint
                && script.remaining > 0
                && (!script.final_chunk_only || final_chunk)
        });
        match script {
            Some(script) => {
                script.remaining -= 1;
                Err(script.fail.error(endpoint))
            }
            None => Ok(()),
        }
    }

    fn write<T: serde::Serialize + ?Sized>(
        &self,
        endpoint: &'static str,
        body: &T,
    ) -> Result<(), SinkError> {
        let body = serde_json::to_value(body).expect("serialisable");
        self.answer(endpoint, body, String::new())
    }
}

/// What every write answers.
type Answer = Result<(), SinkError>;

/// The chunk protocol's parameters, as the retired route's query string.
fn chunk_query(rows_empty: bool, part: ChunkPart<'_>) -> String {
    let publish_id = part.publish_id;
    if let (true, Some(service_date)) = (rows_empty, part.empty_service_date) {
        format!(
            "first_chunk=true&publish_id={publish_id}&last_chunk=true&total_rows=0\
             &service_date={service_date}"
        )
    } else {
        let mut query = format!("first_chunk={}&publish_id={publish_id}", part.first_chunk);
        if let Some(total_rows) = part.final_total_rows {
            use std::fmt::Write as _;
            write!(query, "&last_chunk=true&total_rows={total_rows}").expect("a String");
        }
        query
    }
}

impl PublishSink for FakePublishSink {
    fn stanox_crs(&self, records: &[common::StanoxCrsRecord]) -> impl Future<Output = Answer> {
        ready(self.write(STANOX_CRS, records))
    }

    fn tiploc_crs(&self, records: &[common::TiplocCrsRecord]) -> impl Future<Output = Answer> {
        ready(self.write(TIPLOC_CRS, records))
    }

    fn tiploc_locations(
        &self,
        records: &[common::TiplocLocationRecord],
    ) -> impl Future<Output = Answer> {
        ready(self.write(TIPLOC_LOCATIONS, records))
    }

    fn fixed_links(&self, records: &[common::FixedLinkRecord]) -> impl Future<Output = Answer> {
        ready(self.write(FIXED_LINKS, records))
    }

    fn line_population(&self, body: &LinePopulation) -> impl Future<Output = Answer> {
        ready(self.write(LINE_POPULATION, body))
    }

    fn network_departures(&self, rows: &[serde_json::Value]) -> impl Future<Output = Answer> {
        ready(self.write(NETWORK_DEPARTURES, rows))
    }

    fn publish_part(
        &self,
        product: DatedProduct,
        rows: &[serde_json::Value],
        part: ChunkPart<'_>,
    ) -> impl Future<Output = Answer> {
        let body = serde_json::Value::Array(rows.to_vec());
        ready(self.answer(
            dated_endpoint(product),
            body,
            chunk_query(rows.is_empty(), part),
        ))
    }

    fn services(
        &self,
        service_date: chrono::NaiveDate,
        rows: &[serde_json::Value],
    ) -> impl Future<Output = Answer> {
        let body = serde_json::Value::Array(rows.to_vec());
        ready(self.answer(SERVICES, body, format!("service_date={service_date}")))
    }

    fn record_completed_publish(&self, delivery: &str) -> impl Future<Output = Answer> {
        ready(self.write(PUBLISHES, &serde_json::json!({ "delivery": delivery })))
    }

    fn last_completed_publish(&self) -> impl Future<Output = Result<Option<String>, SinkError>> {
        let answer = self
            .answer(LAST_PUBLISH, serde_json::Value::Null, String::new())
            .map(|()| self.last_completed.lock().unwrap().clone());
        ready(answer)
    }
}
