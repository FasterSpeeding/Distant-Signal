//! Live arrivals at a `TfL` Tube, DLR or tram stop (DS-136), for
//! `GET /public/tfl/stops/{id}/arrivals` and the `/tfl/stops/{id}` page.
//!
//! **Where the fetching happens.** On demand, in the api, from `TfL`'s
//! Unified API (`GET {base}/StopPoint/{id}/Arrivals`), behind a short
//! per-stop cache ([`TflArrivals`]): traffic is low and bursty (a page open
//! on one stop refreshes every 30 s), so a poller fetching every stop all
//! the time would spend far more of `TfL`'s rate limit than the readers do.
//! The cache is per pod and in memory, like the rate limiter: a few hundred
//! stops at most (a stop must be in `tfl_stops` before anything is
//! fetched), and a second pod fetching the same stop once more is harmless.
//!
//! **Single flight.** Each stop has one slot holding a
//! [`tokio::sync::OnceCell`]: concurrent requests for a stop whose entry is
//! missing or stale share one upstream call, and the rest wait for its
//! answer rather than stampeding `TfL`. A failure is cached too, for a
//! shorter time ([`TflArrivalsSettings::error_cache_secs`]), so an outage
//! costs one upstream call per stop per few seconds rather than one per
//! request.
//!
//! **Key.** `TfL` accepts anonymous calls at a lower rate limit; with
//! `TFL_APP_KEY` set (the chart passes poller-tfl's key) the call carries
//! it in the `Ocp-Apim-Subscription-Key` header, never in the URL, so it
//! cannot reach a log line.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;

/// `TfL`'s subscription-key header (as poller-tfl sends it).
const TFL_AUTH_HEADER_NAME: &str = "Ocp-Apim-Subscription-Key";

/// The largest arrivals body read. A busy interchange is tens of KB.
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;

/// Slots kept before expired ones are swept out. Only stops in
/// `tfl_stops` get a slot (the route checks first), so this is headroom.
const MAX_SLOTS: usize = 2_048;

/// Settings from the environment (`crate::app::AppState::init`). The chart
/// sets them from `api.tflArrivals`.
#[derive(Clone, clap::Parser)]
#[command(name = "api-tfl-arrivals", no_binary_name = true)]
pub struct TflArrivalsSettings {
    /// Off answers every arrivals request with a 503.
    #[arg(long, env = "TFL_ARRIVALS_ENABLED", default_value_t = true, action = clap::ArgAction::Set)]
    pub enabled: bool,
    /// `TfL` Unified API root, without a trailing path.
    #[arg(long, env = "TFL_BASE_URL", default_value = "https://api.tfl.gov.uk")]
    pub base_url: String,
    /// `TfL` subscription key. Optional: unset or blank calls anonymously.
    #[arg(long, env = "TFL_APP_KEY", hide_env_values = true)]
    pub app_key: Option<String>,
    /// How long one stop's arrivals are served from the cache.
    #[arg(long, env = "TFL_ARRIVALS_CACHE_SECS", default_value_t = 30)]
    pub cache_secs: u64,
    /// How long a failed fetch is remembered before the next try.
    #[arg(long, env = "TFL_ARRIVALS_ERROR_CACHE_SECS", default_value_t = 10)]
    pub error_cache_secs: u64,
    /// Per-request timeout. The page renders on the server and the
    /// frontend gives up on the api after 8 s, so this stays well inside.
    #[arg(long, env = "TFL_ARRIVALS_TIMEOUT_SECS", default_value_t = 5)]
    pub timeout_secs: u64,
}

impl TflArrivalsSettings {
    pub fn from_env() -> Result<Self> {
        use clap::Parser;
        Self::try_parse_from(std::iter::empty::<String>())
            .context("invalid TfL arrivals settings in the environment")
    }
}

/// Hand-written so the key never reaches a log line.
impl std::fmt::Debug for TflArrivalsSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TflArrivalsSettings")
            .field("enabled", &self.enabled)
            .field("base_url", &self.base_url)
            .field("app_key", &self.app_key.as_ref().map(|_| "[REDACTED]"))
            .field("cache_secs", &self.cache_secs)
            .field("error_cache_secs", &self.error_cache_secs)
            .field("timeout_secs", &self.timeout_secs)
            .finish()
    }
}

/// One prediction as `TfL` serves it (`Tfl.Api.Presentation.Entities.Prediction`),
/// the fields this uses. Every text field may be missing or empty.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflPrediction {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub line_id: String,
    #[serde(default)]
    pub line_name: String,
    #[serde(default)]
    pub platform_name: String,
    /// `inbound` / `outbound`, when `TfL` knows.
    #[serde(default)]
    pub direction: Option<String>,
    /// `Cockfosters Underground Station`.
    #[serde(default)]
    pub destination_name: Option<String>,
    /// `Cockfosters`, or `Check Front of Train`.
    #[serde(default)]
    pub towards: Option<String>,
    #[serde(default)]
    pub current_location: Option<String>,
    pub expected_arrival: DateTime<Utc>,
}

/// Why `TfL`'s arrivals could not be had. Every variant is a 503 to the
/// caller; the detail is for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// `TFL_ARRIVALS_ENABLED=false`.
    Disabled,
    /// `TfL` answered with this non-success status (a 429 included).
    Status(u16),
    /// No answer: connect failure, timeout, a dropped connection.
    Transport(String),
    /// An answer that is not an arrivals list.
    Decode(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FetchError::Disabled => write!(f, "TfL arrivals are turned off"),
            FetchError::Status(status) => write!(f, "TfL answered {status}"),
            FetchError::Transport(err) => write!(f, "TfL request failed: {err}"),
            FetchError::Decode(err) => write!(f, "TfL answer not understood: {err}"),
        }
    }
}

impl std::error::Error for FetchError {}

impl FetchError {
    /// The metric label.
    pub fn label(&self) -> &'static str {
        match self {
            FetchError::Disabled => "disabled",
            FetchError::Status(429) => "rate_limited",
            FetchError::Status(_) => "status",
            FetchError::Transport(_) => "transport",
            FetchError::Decode(_) => "decode",
        }
    }
}

type FetchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<TflPrediction>, FetchError>> + Send + 'a>>;

/// Where predictions come from: [`HttpArrivalsSource`] in production, a
/// fake in tests.
pub trait ArrivalsSource: Send + Sync + 'static {
    fn fetch<'a>(&'a self, stop_id: &'a str) -> FetchFuture<'a>;
}

/// `GET {base}/StopPoint/{id}/Arrivals`.
pub struct HttpArrivalsSource {
    client: reqwest::Client,
    base_url: String,
    app_key: Option<String>,
}

impl HttpArrivalsSource {
    pub fn new(base_url: &str, app_key: Option<&str>, timeout: Duration) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("build the TfL arrivals HTTP client")?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            app_key: app_key
                .map(str::trim)
                .filter(|key| !key.is_empty())
                .map(str::to_owned),
        })
    }

    async fn fetch_inner(&self, stop_id: &str) -> Result<Vec<TflPrediction>, FetchError> {
        // `stop_id` is a checked NaPTAN id (letters and digits), so it
        // needs no escaping.
        let url = format!("{}/StopPoint/{stop_id}/Arrivals", self.base_url);
        let mut request = self.client.get(&url);
        if let Some(key) = &self.app_key {
            request = request.header(TFL_AUTH_HEADER_NAME, key);
        }
        let response = request
            .send()
            .await
            .map_err(|err| FetchError::Transport(err.without_url().to_string()))?;
        let status = response.status();
        if !status.is_success() {
            return Err(FetchError::Status(status.as_u16()));
        }
        let body = response
            .bytes()
            .await
            .map_err(|err| FetchError::Transport(err.without_url().to_string()))?;
        if body.len() > MAX_BODY_BYTES {
            return Err(FetchError::Decode(format!(
                "body of {} bytes is over the {MAX_BODY_BYTES}-byte limit",
                body.len()
            )));
        }
        parse_predictions(&body)
    }
}

impl ArrivalsSource for HttpArrivalsSource {
    fn fetch<'a>(&'a self, stop_id: &'a str) -> FetchFuture<'a> {
        Box::pin(self.fetch_inner(stop_id))
    }
}

/// `TfL`'s arrivals JSON (an array of predictions). A prediction this
/// cannot read (no `expectedArrival`) is skipped rather than failing the
/// whole board.
pub fn parse_predictions(body: &[u8]) -> Result<Vec<TflPrediction>, FetchError> {
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(body).map_err(|err| FetchError::Decode(err.to_string()))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| serde_json::from_value(row).ok())
        .collect())
}

/// One stop's predictions and when they were fetched.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub fetched_at: DateTime<Utc>,
    pub predictions: Arc<[TflPrediction]>,
}

#[derive(Debug, Clone)]
struct Outcome {
    at: Instant,
    result: Result<Snapshot, FetchError>,
}

type Slot = Arc<OnceCell<Outcome>>;

/// The per-stop cache in front of an [`ArrivalsSource`]. `Default` is
/// disabled (every call a [`FetchError::Disabled`]), for test apps.
pub struct TflArrivals {
    source: Option<Arc<dyn ArrivalsSource>>,
    ttl: Duration,
    error_ttl: Duration,
    slots: Mutex<HashMap<String, Slot>>,
}

impl Default for TflArrivals {
    fn default() -> Self {
        Self::disabled()
    }
}

impl std::fmt::Debug for TflArrivals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TflArrivals")
            .field("enabled", &self.source.is_some())
            .field("ttl", &self.ttl)
            .field("error_ttl", &self.error_ttl)
            .finish_non_exhaustive()
    }
}

impl TflArrivals {
    pub fn disabled() -> Self {
        Self {
            source: None,
            ttl: Duration::ZERO,
            error_ttl: Duration::ZERO,
            slots: Mutex::new(HashMap::new()),
        }
    }

    pub fn new(source: Arc<dyn ArrivalsSource>, ttl: Duration, error_ttl: Duration) -> Self {
        Self {
            source: Some(source),
            ttl,
            error_ttl,
            slots: Mutex::new(HashMap::new()),
        }
    }

    pub fn from_settings(settings: &TflArrivalsSettings) -> Result<Self> {
        if !settings.enabled {
            return Ok(Self::disabled());
        }
        let source = HttpArrivalsSource::new(
            &settings.base_url,
            settings.app_key.as_deref(),
            Duration::from_secs(settings.timeout_secs.max(1)),
        )?;
        Ok(Self::new(
            Arc::new(source),
            Duration::from_secs(settings.cache_secs),
            Duration::from_secs(settings.error_cache_secs),
        ))
    }

    fn expired(&self, outcome: &Outcome, now: Instant) -> bool {
        let ttl = if outcome.result.is_ok() {
            self.ttl
        } else {
            self.error_ttl
        };
        now.duration_since(outcome.at) >= ttl
    }

    /// The slot to read `stop_id` from: the current one while it is in
    /// flight or fresh, else a new, empty one.
    fn slot(&self, stop_id: &str) -> Slot {
        let now = Instant::now();
        let mut slots = self.slots.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(slot) = slots.get(stop_id)
            && !slot.get().is_some_and(|outcome| self.expired(outcome, now))
        {
            return Arc::clone(slot);
        }
        if slots.len() >= MAX_SLOTS {
            slots.retain(|_, slot| !slot.get().is_some_and(|outcome| self.expired(outcome, now)));
        }
        let slot: Slot = Arc::new(OnceCell::new());
        slots.insert(stop_id.to_owned(), Arc::clone(&slot));
        slot
    }

    /// `stop_id`'s predictions: from the cache while fresh, else from one
    /// shared upstream call. A request dropped mid-fetch hands the fetch to
    /// the next waiter (`OnceCell::get_or_init`).
    pub async fn get(&self, stop_id: &str) -> Result<Snapshot, FetchError> {
        let Some(source) = &self.source else {
            return Err(FetchError::Disabled);
        };
        let slot = self.slot(stop_id);
        let outcome = slot
            .get_or_init(|| async {
                let result = source.fetch(stop_id).await;
                let label = match &result {
                    Ok(_) => "success",
                    Err(err) => err.label(),
                };
                metrics::counter!(
                    common::metrics::metric_name("api_tfl_arrivals_fetch_total"),
                    "outcome" => label
                )
                .increment(1);
                if let Err(err) = &result {
                    tracing::warn!(stop_id, error = %err, "TfL arrivals fetch failed");
                }
                Outcome {
                    at: Instant::now(),
                    result: result.map(|predictions| Snapshot {
                        fetched_at: Utc::now(),
                        predictions: predictions.into(),
                    }),
                }
            })
            .await;
        outcome.result.clone()
    }
}

/// One arrival as the api serves it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Arrival {
    /// `TfL`'s prediction id: stable across refreshes, a list key.
    pub id: String,
    /// `Cockfosters`; `TfL`'s `towards` when it names no destination.
    pub destination: String,
    pub expected_arrival: DateTime<Utc>,
    /// Seconds from the response's `now` to `expectedArrival`, never
    /// negative (a train due now is 0).
    pub time_to_station_secs: i64,
    /// `Between Holborn and Russell Square`, when `TfL` says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_location: Option<String>,
}

/// The arrivals at one platform of one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArrivalGroup {
    pub line_id: String,
    pub line_name: String,
    /// `Westbound - Platform 1`; empty when `TfL` names none.
    pub platform_name: String,
    /// `inbound` / `outbound`, when `TfL` says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    /// Soonest first, at most [`MAX_PER_GROUP`].
    pub arrivals: Vec<Arrival>,
}

/// `GET /public/tfl/stops/{id}/arrivals`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StopArrivals {
    pub stop_id: String,
    /// When `TfL` was asked (the cache's age shows here).
    pub fetched_at: DateTime<Utc>,
    /// By line name, then platform name.
    pub groups: Vec<ArrivalGroup>,
}

/// Arrivals kept per platform.
pub const MAX_PER_GROUP: usize = 8;

/// How long past its expected time a prediction is still shown (as due).
const DEPARTED_GRACE_SECS: i64 = 30;

fn non_blank(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// What a prediction says it is going to: the cleaned destination name,
/// else `towards`, else `TfL`'s own advice to look at the train.
fn destination_of(prediction: &TflPrediction) -> String {
    non_blank(prediction.destination_name.as_deref())
        .map(common::tfl_stops::clean_stop_name)
        .or_else(|| non_blank(prediction.towards.as_deref()).map(str::to_owned))
        .unwrap_or_else(|| "Check front of train".to_owned())
}

/// `snapshot`'s predictions grouped by line and platform for the page,
/// timed against `now`: each group soonest first, groups by line name then
/// platform. A prediction more than 30 s past its time is dropped, as is a
/// repeat of a prediction id.
pub fn group_arrivals(stop_id: &str, snapshot: &Snapshot, now: DateTime<Utc>) -> StopArrivals {
    let mut seen = std::collections::HashSet::new();
    let mut groups: HashMap<(String, String), ArrivalGroup> = HashMap::new();
    for prediction in snapshot.predictions.iter() {
        if !prediction.id.is_empty() && !seen.insert(prediction.id.as_str()) {
            continue;
        }
        let secs = (prediction.expected_arrival - now).num_seconds();
        if secs < -DEPARTED_GRACE_SECS {
            continue;
        }
        let line_name = non_blank(Some(&prediction.line_name))
            .or(non_blank(Some(&prediction.line_id)))
            .unwrap_or("Unknown line")
            .to_owned();
        let platform_name = prediction.platform_name.trim().to_owned();
        let group = groups
            .entry((prediction.line_id.clone(), platform_name.clone()))
            .or_insert_with(|| ArrivalGroup {
                line_id: prediction.line_id.clone(),
                line_name,
                platform_name,
                direction: non_blank(prediction.direction.as_deref()).map(str::to_owned),
                arrivals: Vec::new(),
            });
        group.arrivals.push(Arrival {
            id: prediction.id.clone(),
            destination: destination_of(prediction),
            expected_arrival: prediction.expected_arrival,
            time_to_station_secs: secs.max(0),
            current_location: non_blank(prediction.current_location.as_deref()).map(str::to_owned),
        });
    }
    let mut groups: Vec<ArrivalGroup> = groups.into_values().collect();
    for group in &mut groups {
        group
            .arrivals
            .sort_by(|a, b| (a.expected_arrival, &a.id).cmp(&(b.expected_arrival, &b.id)));
        group.arrivals.truncate(MAX_PER_GROUP);
    }
    groups.sort_by(|a, b| {
        (&a.line_name, &a.line_id, platform_key(&a.platform_name)).cmp(&(
            &b.line_name,
            &b.line_id,
            platform_key(&b.platform_name),
        ))
    });
    StopArrivals {
        stop_id: stop_id.to_owned(),
        fetched_at: snapshot.fetched_at,
        groups,
    }
}

/// Sorts `Platform 2` before `Platform 10`: the text with its last number
/// (if any) as a number.
fn platform_key(name: &str) -> (String, u32, String) {
    let digits_end = name
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_ascii_digit())
        .map(|(i, c)| i + c.len_utf8());
    let Some(end) = digits_end else {
        return (name.to_owned(), 0, String::new());
    };
    let start = name[..end]
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_digit())
        .last()
        .map_or(end, |(i, _)| i);
    let number = name[start..end].parse().unwrap_or(u32::MAX);
    (name[..start].to_owned(), number, name[end..].to_owned())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A trimmed real `StopPoint/940GZZLURSQ/Arrivals` answer (Russell
    /// Square, Piccadilly line): every field `TfL` sends, two platforms,
    /// one repeated prediction and one without `expectedArrival`.
    pub(crate) const RUSSELL_SQUARE: &str = r#"[
      {
        "$type": "Tfl.Api.Presentation.Entities.Prediction, Tfl.Api.Presentation.Entities",
        "id": "-1416081476",
        "operationType": 1,
        "vehicleId": "315",
        "naptanId": "940GZZLURSQ",
        "stationName": "Russell Square Underground Station",
        "lineId": "piccadilly",
        "lineName": "Piccadilly",
        "platformName": "Westbound - Platform 2",
        "direction": "outbound",
        "bearing": "",
        "destinationNaptanId": "940GZZLUHR5",
        "destinationName": "Heathrow Terminal 5 Underground Station",
        "timestamp": "2026-10-23T10:00:00.1234567Z",
        "timeToStation": 120,
        "currentLocation": "Between King's Cross St. Pancras and Russell Square",
        "towards": "Heathrow T5",
        "expectedArrival": "2026-10-23T10:02:00Z",
        "timeToLive": "2026-10-23T10:02:00Z",
        "modeName": "tube",
        "timing": {
          "$type": "Tfl.Api.Presentation.Entities.PredictionTiming, Tfl.Api.Presentation.Entities",
          "countdownServerAdjustment": "00:00:00",
          "source": "0001-01-01T00:00:00",
          "insert": "0001-01-01T00:00:00",
          "read": "2026-10-23T09:59:58.123Z",
          "sent": "2026-10-23T10:00:00Z",
          "received": "0001-01-01T00:00:00Z"
        }
      },
      {
        "id": "-1416081477",
        "lineId": "piccadilly",
        "lineName": "Piccadilly",
        "platformName": "Eastbound - Platform 1",
        "direction": "inbound",
        "destinationName": "Cockfosters Underground Station",
        "currentLocation": "At Holborn",
        "towards": "Cockfosters",
        "expectedArrival": "2026-10-23T10:01:00Z",
        "modeName": "tube"
      },
      {
        "id": "-1416081478",
        "lineId": "piccadilly",
        "lineName": "Piccadilly",
        "platformName": "Westbound - Platform 2",
        "direction": "outbound",
        "destinationName": "",
        "currentLocation": "",
        "towards": "Check Front of Train",
        "expectedArrival": "2026-10-23T10:00:40Z",
        "modeName": "tube"
      },
      {
        "id": "-1416081476",
        "lineId": "piccadilly",
        "lineName": "Piccadilly",
        "platformName": "Westbound - Platform 2",
        "destinationName": "Heathrow Terminal 5 Underground Station",
        "expectedArrival": "2026-10-23T10:02:00Z"
      },
      {
        "id": "-1416081479",
        "lineId": "piccadilly",
        "lineName": "Piccadilly",
        "platformName": "Eastbound - Platform 1",
        "destinationName": "Arnos Grove Underground Station",
        "expectedArrival": "2026-10-23T09:58:00Z"
      },
      {
        "id": "-1416081480",
        "lineId": "piccadilly",
        "platformName": "Eastbound - Platform 1"
      }
    ]"#;

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().to_utc()
    }

    #[test]
    fn a_real_answer_parses_skipping_rows_without_a_time() {
        let predictions = parse_predictions(RUSSELL_SQUARE.as_bytes()).unwrap();
        assert_eq!(predictions.len(), 5);
        assert_eq!(predictions[0].line_id, "piccadilly");
        assert_eq!(predictions[0].expected_arrival, at("2026-10-23T10:02:00Z"));
        assert!(matches!(
            parse_predictions(b"{\"message\": \"nope\"}"),
            Err(FetchError::Decode(_))
        ));
        assert_eq!(parse_predictions(b"[]").unwrap(), []);
    }

    #[test]
    fn arrivals_group_by_platform_soonest_first() {
        let snapshot = Snapshot {
            fetched_at: at("2026-10-23T10:00:00Z"),
            predictions: parse_predictions(RUSSELL_SQUARE.as_bytes()).unwrap().into(),
        };
        let board = group_arrivals("940GZZLURSQ", &snapshot, at("2026-10-23T10:00:10Z"));
        assert_eq!(board.stop_id, "940GZZLURSQ");
        assert_eq!(board.fetched_at, at("2026-10-23T10:00:00Z"));
        let platforms: Vec<&str> = board
            .groups
            .iter()
            .map(|group| group.platform_name.as_str())
            .collect();
        assert_eq!(
            platforms,
            ["Eastbound - Platform 1", "Westbound - Platform 2"]
        );

        let east = &board.groups[0];
        assert_eq!(east.line_name, "Piccadilly");
        assert_eq!(east.direction.as_deref(), Some("inbound"));
        // Arnos Grove left two minutes ago: dropped.
        assert_eq!(east.arrivals.len(), 1);
        assert_eq!(east.arrivals[0].destination, "Cockfosters");
        assert_eq!(east.arrivals[0].time_to_station_secs, 50);
        assert_eq!(
            east.arrivals[0].current_location.as_deref(),
            Some("At Holborn")
        );

        let west = &board.groups[1];
        // The repeat is dropped; no destination falls back to `towards`.
        let west_rows: Vec<(&str, i64)> = west
            .arrivals
            .iter()
            .map(|a| (a.destination.as_str(), a.time_to_station_secs))
            .collect();
        assert_eq!(
            west_rows,
            [("Check Front of Train", 30), ("Heathrow Terminal 5", 110)]
        );
        assert_eq!(west.arrivals[0].current_location, None);
    }

    #[test]
    fn a_train_just_due_shows_as_zero_and_groups_are_capped() {
        let base = at("2026-10-23T10:00:00Z");
        let predictions: Vec<TflPrediction> = (0..12)
            .map(|i| TflPrediction {
                id: format!("p{i:02}"),
                line_id: "dlr".to_owned(),
                line_name: "DLR".to_owned(),
                platform_name: "2".to_owned(),
                direction: None,
                destination_name: None,
                towards: None,
                current_location: None,
                expected_arrival: base + chrono::Duration::seconds(i64::from(i) * 60 - 20),
            })
            .collect();
        let snapshot = Snapshot {
            fetched_at: base,
            predictions: predictions.into(),
        };
        let board = group_arrivals("940GZZDLCGT", &snapshot, base);
        assert_eq!(board.groups.len(), 1);
        let arrivals = &board.groups[0].arrivals;
        assert_eq!(arrivals.len(), MAX_PER_GROUP);
        assert_eq!(arrivals[0].time_to_station_secs, 0);
        assert_eq!(arrivals[0].destination, "Check front of train");
    }

    #[test]
    fn platforms_sort_by_their_number() {
        let mut names = vec!["Platform 10", "Platform 2", "Westbound", "Platform 1"];
        names.sort_by_key(|name| platform_key(name));
        assert_eq!(
            names,
            ["Platform 1", "Platform 2", "Platform 10", "Westbound"]
        );
        assert_eq!(platform_key("3"), (String::new(), 3, String::new()));
    }

    #[test]
    fn settings_default_on_and_redact_the_key() {
        use clap::Parser;
        let settings =
            TflArrivalsSettings::try_parse_from(["--app-key", "secret-key-123"]).unwrap();
        assert!(settings.enabled);
        assert_eq!(settings.base_url, "https://api.tfl.gov.uk");
        assert_eq!(settings.cache_secs, 30);
        let debug = format!("{settings:?}");
        assert!(!debug.contains("secret-key-123"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
        let off = TflArrivalsSettings::try_parse_from(["--enabled", "false"]).unwrap();
        assert!(!off.enabled);
    }

    /// Counts calls; answers each after `delay` with `answer`.
    pub(crate) struct FakeSource {
        pub(crate) calls: AtomicUsize,
        pub(crate) delay: Duration,
        pub(crate) answer: Mutex<Result<Vec<TflPrediction>, FetchError>>,
    }

    impl FakeSource {
        pub(crate) fn new(answer: Result<Vec<TflPrediction>, FetchError>) -> Arc<Self> {
            Arc::new(Self {
                calls: AtomicUsize::new(0),
                delay: Duration::from_millis(50),
                answer: Mutex::new(answer),
            })
        }

        pub(crate) fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    impl ArrivalsSource for FakeSource {
        fn fetch<'a>(&'a self, _stop_id: &'a str) -> FetchFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(self.delay).await;
                self.answer.lock().unwrap().clone()
            })
        }
    }

    fn one_prediction() -> Vec<TflPrediction> {
        parse_predictions(RUSSELL_SQUARE.as_bytes()).unwrap()[..1].to_vec()
    }

    #[tokio::test]
    async fn concurrent_requests_share_one_fetch() {
        let source = FakeSource::new(Ok(one_prediction()));
        let cache = Arc::new(TflArrivals::new(
            source.clone(),
            Duration::from_secs(30),
            Duration::from_secs(10),
        ));
        let mut tasks = Vec::new();
        for _ in 0..20 {
            let cache = Arc::clone(&cache);
            tasks.push(tokio::spawn(async move { cache.get("940GZZLURSQ").await }));
        }
        for task in tasks {
            assert_eq!(task.await.unwrap().unwrap().predictions.len(), 1);
        }
        assert_eq!(source.calls(), 1);
        // Fresh: still cached.
        cache.get("940GZZLURSQ").await.unwrap();
        assert_eq!(source.calls(), 1);
        // Another stop is its own entry.
        cache.get("940GZZLUHBN").await.unwrap();
        assert_eq!(source.calls(), 2);
    }

    #[tokio::test]
    async fn a_stale_entry_is_fetched_again() {
        let source = FakeSource::new(Ok(one_prediction()));
        let cache = TflArrivals::new(source.clone(), Duration::ZERO, Duration::ZERO);
        let first = cache.get("940GZZLURSQ").await.unwrap();
        let second = cache.get("940GZZLURSQ").await.unwrap();
        assert_eq!(source.calls(), 2);
        assert!(second.fetched_at >= first.fetched_at);
    }

    #[tokio::test]
    async fn failures_are_cached_briefly_then_retried() {
        let source = FakeSource::new(Err(FetchError::Status(503)));
        let cache = TflArrivals::new(
            source.clone(),
            Duration::from_secs(30),
            Duration::from_millis(200),
        );
        assert_eq!(
            cache.get("940GZZLURSQ").await.unwrap_err(),
            FetchError::Status(503)
        );
        assert_eq!(
            cache.get("940GZZLURSQ").await.unwrap_err(),
            FetchError::Status(503)
        );
        assert_eq!(source.calls(), 1, "a failure is remembered");

        *source.answer.lock().unwrap() = Ok(one_prediction());
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(cache.get("940GZZLURSQ").await.unwrap().predictions.len(), 1);
        assert_eq!(source.calls(), 2);
    }

    #[tokio::test]
    async fn a_dropped_request_hands_the_fetch_on() {
        let source = FakeSource::new(Ok(one_prediction()));
        let cache = Arc::new(TflArrivals::new(
            source.clone(),
            Duration::from_secs(30),
            Duration::from_secs(10),
        ));
        let first = {
            let cache = Arc::clone(&cache);
            tokio::spawn(async move { cache.get("940GZZLURSQ").await })
        };
        tokio::time::sleep(Duration::from_millis(10)).await;
        first.abort();
        assert!(cache.get("940GZZLURSQ").await.is_ok());
        assert_eq!(source.calls(), 2);
    }

    #[tokio::test]
    async fn the_default_is_disabled() {
        assert_eq!(
            TflArrivals::default().get("940GZZLURSQ").await.unwrap_err(),
            FetchError::Disabled
        );
    }

    mod http {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        use super::*;

        #[tokio::test]
        async fn the_key_goes_in_the_header() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/StopPoint/940GZZLURSQ/Arrivals"))
                .and(header(TFL_AUTH_HEADER_NAME, "test-key"))
                .respond_with(ResponseTemplate::new(200).set_body_string(RUSSELL_SQUARE))
                .expect(1)
                .mount(&server)
                .await;
            let source = HttpArrivalsSource::new(
                &format!("{}/", server.uri()),
                Some("test-key"),
                Duration::from_secs(2),
            )
            .unwrap();
            assert_eq!(source.fetch("940GZZLURSQ").await.unwrap().len(), 5);
            let request = &server.received_requests().await.unwrap()[0];
            assert!(request.url.query().is_none(), "no key in the URL");
        }

        #[tokio::test]
        async fn no_key_sends_no_header() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/StopPoint/940GZZLURSQ/Arrivals"))
                .respond_with(ResponseTemplate::new(200).set_body_string("[]"))
                .mount(&server)
                .await;
            let source =
                HttpArrivalsSource::new(&server.uri(), Some("  "), Duration::from_secs(2)).unwrap();
            assert!(source.fetch("940GZZLURSQ").await.unwrap().is_empty());
            let request = &server.received_requests().await.unwrap()[0];
            assert!(!request.headers.contains_key(TFL_AUTH_HEADER_NAME));
        }

        #[tokio::test]
        async fn errors_map_to_their_kind() {
            let server = MockServer::start().await;
            for (stop, template) in [
                ("940GZZLUAAA", ResponseTemplate::new(429)),
                ("940GZZLUBBB", ResponseTemplate::new(500)),
                (
                    "940GZZLUCCC",
                    ResponseTemplate::new(200).set_body_string("<html>"),
                ),
                (
                    "940GZZLUDDD",
                    ResponseTemplate::new(200)
                        .set_body_string("[]")
                        .set_delay(Duration::from_secs(3)),
                ),
            ] {
                Mock::given(method("GET"))
                    .and(path(format!("/StopPoint/{stop}/Arrivals")))
                    .respond_with(template)
                    .mount(&server)
                    .await;
            }
            let source =
                HttpArrivalsSource::new(&server.uri(), None, Duration::from_secs(1)).unwrap();
            let rate_limited = source.fetch("940GZZLUAAA").await.unwrap_err();
            assert_eq!(rate_limited, FetchError::Status(429));
            assert_eq!(rate_limited.label(), "rate_limited");
            assert_eq!(
                source.fetch("940GZZLUBBB").await.unwrap_err(),
                FetchError::Status(500)
            );
            assert!(matches!(
                source.fetch("940GZZLUCCC").await.unwrap_err(),
                FetchError::Decode(_)
            ));
            let timeout = source.fetch("940GZZLUDDD").await.unwrap_err();
            assert!(matches!(timeout, FetchError::Transport(_)), "{timeout:?}");
            assert!(!timeout.to_string().contains("127.0.0.1"), "{timeout}");
        }
    }
}
