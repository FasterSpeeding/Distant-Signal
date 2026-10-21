//! Pub/Sub notifications for the bucket source (`BUCKET_PUBSUB_SUBSCRIPTION`;
//! the GCS spec §6 option C, phase 6): the bucket's `OBJECT_FINALIZE`
//! notifications reach a pull subscription (Ranma's `ds-ingest-bucket`
//! release, `notifications.pubsub`), and this task pulls it over outbound
//! HTTPS and wakes the bucket source ([`crate::bucket::Wake`]) for an early
//! poll. Polling stays: every `BUCKET_PUBSUB_RECONCILE_INTERVAL_SECS` while
//! the subscription is healthy, every `BUCKET_POLL_INTERVAL_SECS` while it is
//! not.
//!
//! A message is a hint, never an instruction: nothing in it is downloaded
//! or trusted, the next listing decides. A message about this bucket's
//! `OBJECT_FINALIZE` wakes a poll and is acknowledged once a poll that
//! started after it has finished (its ack deadline extended meanwhile, up
//! to [`ACK_WAIT`]); any other message (another event type or bucket, or a
//! malformed one) is acknowledged at once. Either way the delivery is
//! at-least-once and every duplicate is harmless (a confirmed generation is
//! never fetched again).
//!
//! REST over the workspace's `reqwest` (`:pull`, `:acknowledge`,
//! `:modifyAckDeadline`), with the reader's credential and the same 401/403
//! handling as GCS (`gcs::send_authorized`). The reader needs
//! `roles/pubsub.subscriber` on the subscription. A refusal or failure marks
//! the subscription unhealthy (so the source falls back to polling), is
//! logged once per change and retried with backoff; nothing here can stop
//! the bucket source, SFTP or the process.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::bucket::Wake;
use crate::gcs::{Credentials, GcsError, send_authorized, without_url};

/// The public Pub/Sub endpoint (`BUCKET_PUBSUB_BASE_URL` empty).
pub(crate) const DEFAULT_BASE_URL: &str = "https://pubsub.googleapis.com";
/// Least time between two woken polls.
pub(crate) const MIN_WAKE_GAP: Duration = Duration::from_secs(60);
/// Messages per pull.
const MAX_MESSAGES: u32 = 20;
/// A pull may wait this long for messages.
const PULL_TIMEOUT: Duration = Duration::from_secs(90);
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Least time between two pulls that found nothing.
const EMPTY_PULL_GAP: Duration = Duration::from_secs(10);
/// How long a woken message waits for its poll before it is acknowledged
/// anyway (the reconcile poll finds the object regardless).
pub(crate) const ACK_WAIT: Duration = Duration::from_secs(30 * 60);
/// While waiting, the ack deadline is pushed out by [`ACK_EXTENSION_SECS`]
/// this often.
const ACK_EXTEND_EVERY: Duration = Duration::from_secs(240);
const ACK_EXTENSION_SECS: u32 = 600;
/// First backoff after a failed pull, doubled up to the ceiling.
const FIRST_BACKOFF: Duration = Duration::from_secs(30);
/// Longest object name shown in a log line (the publisher controls it).
const MAX_LOGGED_NAME: usize = 256;

const SOURCE: &str = "bucket";
pub(crate) const MESSAGES_METRIC: &str = "schedule_feed_source_notifications_total";
pub(crate) const ERRORS_METRIC: &str = "schedule_feed_source_notification_errors_total";
pub(crate) const HEALTHY_METRIC: &str = "schedule_feed_source_notifications_healthy";
/// `outcome` of [`MESSAGES_METRIC`].
pub(crate) const OUTCOMES: [&str; 2] = ["wake", "ignored"];
/// `kind` of [`ERRORS_METRIC`].
pub(crate) const ERROR_KINDS: [&str; 3] = ["auth", "pull", "ack"];

/// One pulled message: its ack id and the attributes Cloud Storage sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Received {
    pub ack_id: String,
    pub event_type: Option<String>,
    pub bucket: Option<String>,
    pub object: Option<String>,
    pub generation: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PullResponse {
    #[serde(default)]
    received_messages: Vec<RawReceived>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawReceived {
    ack_id: String,
    #[serde(default)]
    message: RawMessage,
}

#[derive(Default, Deserialize)]
struct RawMessage {
    #[serde(default)]
    attributes: std::collections::HashMap<String, String>,
}

/// The subscription, through the Pub/Sub REST API.
#[derive(Debug)]
pub(crate) struct PubSubClient {
    http: reqwest::Client,
    /// `<base>/v1/projects/<p>/subscriptions/<s>`.
    url: String,
    credentials: Credentials,
}

impl PubSubClient {
    pub(crate) fn new(
        base_url: Option<&str>,
        subscription: &str,
        credentials: Credentials,
    ) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!(
                "distant-signal-schedule-ingest/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()?;
        let base = base_url
            .filter(|url| !url.is_empty())
            .unwrap_or(DEFAULT_BASE_URL)
            .trim_end_matches('/');
        Ok(Self {
            http,
            url: format!("{base}/v1/{subscription}"),
            credentials,
        })
    }

    async fn post(
        &self,
        what: &str,
        verb: &str,
        body: &serde_json::Value,
        timeout: Duration,
    ) -> Result<reqwest::Response, GcsError> {
        let url = format!("{}:{verb}", self.url);
        let response = send_authorized(&self.http, &self.credentials, what, |http| {
            http.post(&url).timeout(timeout).json(body)
        })
        .await?;
        let status = response.status();
        if status.is_success() {
            Ok(response)
        } else {
            Err(GcsError::Failed(format!("{what}: HTTP {status}")))
        }
    }

    /// Up to [`MAX_MESSAGES`] messages; empty when none came within the
    /// pull's wait.
    pub(crate) async fn pull(&self) -> Result<Vec<Received>, GcsError> {
        let response = match self
            .post(
                "pubsub pull",
                "pull",
                &serde_json::json!({ "maxMessages": MAX_MESSAGES }),
                PULL_TIMEOUT,
            )
            .await
        {
            Ok(response) => response,
            // Nothing arrived while the server held the pull.
            Err(GcsError::Failed(why)) if why.contains("(timeout)") => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let page: PullResponse = response
            .json()
            .await
            .map_err(|err| GcsError::Failed(format!("pubsub pull: {}", without_url(&err))))?;
        Ok(page
            .received_messages
            .into_iter()
            .map(|raw| {
                let mut attributes = raw.message.attributes;
                Received {
                    ack_id: raw.ack_id,
                    event_type: attributes.remove("eventType"),
                    bucket: attributes.remove("bucketId"),
                    object: attributes.remove("objectId"),
                    generation: attributes.remove("objectGeneration"),
                }
            })
            .collect())
    }

    pub(crate) async fn acknowledge(&self, ack_ids: &[String]) -> Result<(), GcsError> {
        if ack_ids.is_empty() {
            return Ok(());
        }
        self.post(
            "pubsub acknowledge",
            "acknowledge",
            &serde_json::json!({ "ackIds": ack_ids }),
            CALL_TIMEOUT,
        )
        .await
        .map(drop)
    }

    async fn extend(&self, ack_ids: &[String]) -> Result<(), GcsError> {
        self.post(
            "pubsub modifyAckDeadline",
            "modifyAckDeadline",
            &serde_json::json!({ "ackIds": ack_ids, "ackDeadlineSeconds": ACK_EXTENSION_SECS }),
            CALL_TIMEOUT,
        )
        .await
        .map(drop)
    }
}

/// Pulls the subscription forever and wakes the bucket source.
pub(crate) struct Subscriber {
    client: PubSubClient,
    /// The delivery bucket: only its notifications wake a poll.
    bucket: String,
    wake: Arc<Wake>,
    max_backoff: Duration,
    empty_pull_gap: Duration,
    failures: u32,
    healthy: bool,
}

impl Subscriber {
    pub(crate) fn new(
        client: PubSubClient,
        bucket: &str,
        wake: Arc<Wake>,
        max_backoff: Duration,
    ) -> Self {
        register_metrics();
        Self {
            client,
            bucket: bucket.to_string(),
            wake,
            max_backoff,
            empty_pull_gap: EMPTY_PULL_GAP,
            failures: 0,
            healthy: false,
        }
    }

    /// Never returns, never panics on a Pub/Sub error.
    pub(crate) async fn run(mut self) {
        tracing::info!(
            subscription = %self.client.url.rsplit("/v1/").next().unwrap_or_default(),
            "bucket Pub/Sub subscriber started"
        );
        loop {
            let pause = self.step().await;
            tokio::time::sleep(pause).await;
        }
    }

    /// One pull and what it leads to; returns how long to wait before the
    /// next.
    async fn step(&mut self) -> Duration {
        let started = Instant::now();
        match self.client.pull().await {
            Ok(messages) => {
                self.set_healthy(true, None);
                self.failures = 0;
                if messages.is_empty() {
                    return self.empty_pull_gap.saturating_sub(started.elapsed());
                }
                self.handle(messages).await;
                Duration::ZERO
            }
            Err(err) => {
                count_error(if matches!(err, GcsError::Failed(_)) {
                    "pull"
                } else {
                    "auth"
                });
                self.set_healthy(false, Some(&err));
                if matches!(err, GcsError::Revoked(_)) {
                    return self.max_backoff;
                }
                self.failures = self.failures.saturating_add(1);
                FIRST_BACKOFF
                    .checked_mul(1 << self.failures.min(16))
                    .unwrap_or(self.max_backoff)
                    .min(self.max_backoff)
            }
        }
    }

    /// Wakes a poll for this bucket's new objects and acknowledges every
    /// message (the relevant ones once that poll is done).
    async fn handle(&self, messages: Vec<Received>) {
        let (wakes, ignored): (Vec<Received>, Vec<Received>) =
            messages.into_iter().partition(|m| {
                m.event_type.as_deref() == Some("OBJECT_FINALIZE")
                    && m.bucket.as_deref() == Some(self.bucket.as_str())
            });
        count_messages("ignored", ignored.len());
        count_messages("wake", wakes.len());
        for message in &ignored {
            tracing::debug!(event_type = ?message.event_type, bucket = ?message.bucket.as_deref().map(shown), "ignoring a Pub/Sub message that is not this bucket's OBJECT_FINALIZE");
        }
        self.ack(&ignored).await;
        if wakes.is_empty() {
            return;
        }
        for message in &wakes {
            tracing::info!(
                object = ?message.object.as_deref().map(shown),
                generation = ?message.generation,
                "a new bucket object was notified; polling the bucket early"
            );
        }
        let ids: Vec<String> = wakes.iter().map(|m| m.ack_id.clone()).collect();
        let ticket = self.wake.request();
        let deadline = Instant::now() + ACK_WAIT;
        let mut waited = std::pin::pin!(self.wake.wait(ticket));
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                tracing::warn!(
                    wait_secs = ACK_WAIT.as_secs(),
                    "no bucket poll finished after a notification; acknowledging it (the reconcile poll catches the object)"
                );
                break;
            }
            match tokio::time::timeout(ACK_EXTEND_EVERY.min(left), &mut waited).await {
                Ok(()) => break,
                Err(_) => {
                    if let Err(err) = self.client.extend(&ids).await {
                        count_error("ack");
                        tracing::warn!(error = %err, "could not extend the Pub/Sub ack deadline; the message may be redelivered (harmless)");
                    }
                }
            }
        }
        self.ack(&wakes).await;
    }

    async fn ack(&self, messages: &[Received]) {
        let ids: Vec<String> = messages.iter().map(|m| m.ack_id.clone()).collect();
        if let Err(err) = self.client.acknowledge(&ids).await {
            count_error("ack");
            tracing::warn!(error = %err, messages = ids.len(), "could not acknowledge Pub/Sub messages; they will be redelivered (harmless)");
        }
    }

    fn set_healthy(&mut self, healthy: bool, err: Option<&GcsError>) {
        self.wake.set_healthy(healthy);
        if healthy == self.healthy {
            return;
        }
        self.healthy = healthy;
        metrics::gauge!(common::metrics::metric_name(HEALTHY_METRIC), "source" => SOURCE)
            .set(if healthy { 1.0 } else { 0.0 });
        match err {
            None => tracing::info!(
                "the bucket's Pub/Sub subscription is delivering; polling only every BUCKET_PUBSUB_RECONCILE_INTERVAL_SECS"
            ),
            Some(err) => {
                tracing::error!(error = %err, "the bucket's Pub/Sub subscription failed; polling the bucket every BUCKET_POLL_INTERVAL_SECS until it recovers");
            }
        }
    }
}

/// `name` escaped and truncated for a log line.
fn shown(name: &str) -> String {
    let escaped: String = name.escape_debug().collect();
    if escaped.len() <= MAX_LOGGED_NAME {
        return escaped;
    }
    let mut end = MAX_LOGGED_NAME;
    while !escaped.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &escaped[..end])
}

fn count_messages(outcome: &'static str, n: usize) {
    metrics::counter!(
        common::metrics::metric_name(MESSAGES_METRIC),
        "source" => SOURCE,
        "outcome" => outcome
    )
    .increment(n as u64);
}

fn count_error(kind: &'static str) {
    metrics::counter!(
        common::metrics::metric_name(ERRORS_METRIC),
        "source" => SOURCE,
        "kind" => kind
    )
    .increment(1);
}

/// Every Pub/Sub metric at 0.
pub(crate) fn register_metrics() {
    metrics::gauge!(common::metrics::metric_name(HEALTHY_METRIC), "source" => SOURCE).set(0.0);
    for outcome in OUTCOMES {
        count_messages(outcome, 0);
    }
    for kind in ERROR_KINDS {
        metrics::counter!(common::metrics::metric_name(ERRORS_METRIC), "source" => SOURCE, "kind" => kind)
            .increment(0);
    }
}

/// Whether `name` is a subscription's full name,
/// `projects/<project>/subscriptions/<subscription>`.
pub(crate) fn is_subscription_name(name: &str) -> bool {
    let mut parts = name.split('/');
    let (Some("projects"), Some(project), Some("subscriptions"), Some(subscription), None) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) else {
        return false;
    };
    let project_ok = (6..=30).contains(&project.len())
        && project
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    let subscription_ok = (3..=255).contains(&subscription.len())
        && subscription.as_bytes()[0].is_ascii_alphabetic()
        && !subscription.starts_with("goog")
        && subscription
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.~+%".contains(&b));
    project_ok && subscription_ok
}

#[cfg(test)]
mod tests {
    use common::secret::Secret;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};

    use super::*;
    use crate::bucket::tests::Fixture;

    const SUBSCRIPTION: &str = "projects/example-project/subscriptions/ds-ingest-events";

    fn message(ack_id: &str, event: &str, bucket: &str, object: &str) -> serde_json::Value {
        serde_json::json!({
            "ackId": ack_id,
            "message": {
                "data": "e30=",
                "attributes": {
                    "notificationConfig": "projects/_/buckets/example-ds-ingest/notificationConfigs/1",
                    "eventType": event,
                    "payloadFormat": "JSON_API_V1",
                    "bucketId": bucket,
                    "objectId": object,
                    "objectGeneration": "7",
                    "eventTime": "2026-10-21T10:00:00.000Z"
                },
                "messageId": ack_id,
                "publishTime": "2026-10-21T10:00:00.100Z"
            }
        })
    }

    fn client(f: &Fixture) -> PubSubClient {
        PubSubClient::new(
            Some(&f.server.uri()),
            SUBSCRIPTION,
            Credentials::Static(Secret::new("t")),
        )
        .unwrap()
    }

    async fn acked(f: &Fixture) -> Vec<Vec<String>> {
        f.server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|r| r.url.path().ends_with(":acknowledge"))
            .map(|r| {
                let body: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
                body["ackIds"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|id| id.as_str().unwrap().to_string())
                    .collect()
            })
            .collect()
    }

    /// End to end: the source has polled an empty bucket and sleeps its
    /// long reconcile interval; a finalize notification for this bucket
    /// wakes it, the new object lands, and only then is the message
    /// acknowledged. Messages about other events or buckets are
    /// acknowledged at once and wake nothing.
    #[tokio::test]
    async fn a_finalize_notification_wakes_a_poll_and_is_acked_after_it() {
        let f = Fixture::new().await;
        let pull = format!("/v1/{SUBSCRIPTION}:pull");
        Mock::given(method("POST"))
            .and(path(pull.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "receivedMessages": [
                    message("a-delete", "OBJECT_DELETE", "example-ds-ingest", "timetable_full.zip"),
                    message("a-other", "OBJECT_FINALIZE", "someone-else", "timetable_full.zip"),
                    message("a-ours", "OBJECT_FINALIZE", "example-ds-ingest", "timetable_full.zip"),
                ]
            })))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&f.server)
            .await;
        Mock::given(method("POST"))
            .and(path(pull.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&f.server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/v1/{SUBSCRIPTION}:acknowledge")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&f.server)
            .await;

        let wake = Wake::new(Duration::ZERO);
        let mut settings = Fixture::settings();
        settings.poll_interval = Duration::from_secs(3600);
        let source = f
            .source(settings)
            .with_wake(Arc::clone(&wake), Duration::from_secs(3600));
        tokio::spawn(source.run());
        // The first poll (an empty bucket) has finished.
        tokio::time::timeout(Duration::from_secs(5), wake.wait(1))
            .await
            .unwrap();
        f.bucket.put(
            "timetable_full.zip",
            b"zip-bytes",
            7,
            f.created(Duration::from_secs(1)),
        );

        let mut subscriber = Subscriber::new(
            client(&f),
            "example-ds-ingest",
            Arc::clone(&wake),
            Duration::from_secs(3600),
        );
        subscriber.empty_pull_gap = Duration::from_millis(10);
        assert_eq!(subscriber.step().await, Duration::ZERO);
        assert!(wake.is_healthy());
        assert_eq!(
            std::fs::read(f.incoming().join("timetable_full.zip")).unwrap(),
            b"zip-bytes",
            "landed before the ack"
        );
        assert_eq!(
            acked(&f).await,
            [
                vec!["a-delete".to_string(), "a-other".to_string()],
                vec!["a-ours".to_string()]
            ]
        );
        // An empty pull: nothing more.
        assert!(subscriber.step().await <= Duration::from_millis(10));
        assert_eq!(acked(&f).await.len(), 2);
    }

    /// A refused subscription is unhealthy (the source falls back to its
    /// poll interval) and is retried at the backoff ceiling; a failing one
    /// backs off from 30 s.
    #[tokio::test]
    async fn a_refused_or_failing_subscription_falls_back_to_polling() {
        let f = Fixture::new().await;
        let pull = format!("/v1/{SUBSCRIPTION}:pull");
        Mock::given(method("POST"))
            .and(path(pull.as_str()))
            .respond_with(ResponseTemplate::new(403))
            .up_to_n_times(1)
            .with_priority(1)
            .mount(&f.server)
            .await;
        Mock::given(method("POST"))
            .and(path(pull.as_str()))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .with_priority(2)
            .mount(&f.server)
            .await;
        Mock::given(method("POST"))
            .and(path(pull.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .with_priority(3)
            .mount(&f.server)
            .await;
        let wake = Wake::new(Duration::ZERO);
        wake.set_healthy(true);
        let mut subscriber = Subscriber::new(
            client(&f),
            "example-ds-ingest",
            Arc::clone(&wake),
            Duration::from_secs(3600),
        );
        assert_eq!(subscriber.step().await, Duration::from_secs(3600));
        assert!(!wake.is_healthy());
        assert_eq!(subscriber.step().await, Duration::from_secs(60));
        assert!(!wake.is_healthy());
        subscriber.step().await;
        assert!(wake.is_healthy(), "recovers");
    }

    #[test]
    fn subscription_names_are_full_paths() {
        assert!(is_subscription_name(SUBSCRIPTION));
        for bad in [
            "",
            "ds-ingest-events",
            "projects/example-project/topics/ds-ingest-events",
            "projects/Example/subscriptions/ds-ingest-events",
            "projects/example-project/subscriptions/goog-x",
            "projects/example-project/subscriptions/ds/extra",
            "projects/example-project/subscriptions/1abc",
        ] {
            assert!(!is_subscription_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn the_metric_names_are_stable() {
        assert_eq!(
            [MESSAGES_METRIC, ERRORS_METRIC, HEALTHY_METRIC],
            [
                "schedule_feed_source_notifications_total",
                "schedule_feed_source_notification_errors_total",
                "schedule_feed_source_notifications_healthy",
            ]
        );
    }
}
