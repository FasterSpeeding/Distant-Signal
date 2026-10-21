//! Ships the delivery bucket's Cloud Audit Logs to stdout for Loki
//! (`BUCKET_AUDIT_LOGS_SHIP`; the GCS spec §8 "Shipping to Loki: the
//! simplest pull path", phase 5).
//!
//! Ranma's Logging sink writes the bucket's audit log entries (Admin
//! Activity, and Data Access: every object read, write and delete) into the
//! audit-log bucket (`BUCKET_AUDIT_LOGS_BUCKET`) as hourly files of
//! newline-delimited JSON `LogEntry`s, named
//! `cloudaudit.googleapis.com/<log>/YYYY/MM/DD/HH:MM:SS_HH:MM:SS_S<n>.json`.
//! A file is written once and never changed; an hour's late entries arrive
//! as another shard (`S1`, ...) of that hour, possibly after later hours'.
//!
//! A task of its own: every `BUCKET_AUDIT_LOGS_POLL_INTERVAL_SECS` it lists
//! each log's folders from [`LOOKBACK_DAYS`] days ago onwards (one listing
//! with `delimiter=/` for the logs, then one per log with `startOffset`),
//! downloads every file it has not shipped yet (that generation, its CRC32C
//! checked, at most [`MAX_FILE_BYTES`]) and writes one JSON line per entry
//! with target `schedule_ingest::bucket_access`: `principal`, `caller_ip`,
//! `method`, `object`, `status`, `time`, plus `resource`, `log` and
//! `insert_id`. The names it shipped within the lookback are kept in
//! `sources/bucket/.audit-shipped`, so a restart ships nothing twice
//! (at-least-once: a crash between a file's lines and that record ships the
//! file again; `insert_id` tells the copies apart). A first start ships the
//! last [`LOOKBACK_DAYS`] days.
//!
//! The same reader credential as the bucket source (`roles/storage.objectViewer`
//! on the audit-log bucket). Nothing here touches the delivery bucket, its
//! metrics or the SFTP lane; a refused or failing call is logged once per
//! state change and backs off up to `BUCKET_MAX_BACKOFF_SECS`.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::bucket::Clock;
use crate::gcs::{GcsClient, GcsError, ObjectMeta};

/// The target of every shipped line: Alloy and the docs' `LogQL` select on it.
pub(crate) const TARGET: &str = "schedule_ingest::bucket_access";
/// Where a Logging sink puts audit logs in a bucket.
const LOG_ROOT: &str = "cloudaudit.googleapis.com/";
/// Days of folders listed back from today (UTC), for late shards.
pub(crate) const LOOKBACK_DAYS: i64 = 2;
/// A larger audit file is never downloaded (a few dozen entries a day are
/// a few kilobytes).
pub(crate) const MAX_FILE_BYTES: u64 = 64 << 20;
/// Files shipped per poll; the rest on the next.
const MAX_FILES_PER_POLL: usize = 200;
/// Longest object or resource name shown in a line (the publisher controls
/// object names).
const MAX_SHOWN: usize = 1024;

pub(crate) const LINES_METRIC: &str = "schedule_feed_bucket_audit_lines_total";
pub(crate) const ERRORS_METRIC: &str = "schedule_feed_bucket_audit_errors_total";
/// `kind` of [`ERRORS_METRIC`].
pub(crate) const ERROR_KINDS: [&str; 5] = ["auth", "list", "get", "verify", "parse"];

/// The shipper's settings (from `config::BucketArgs`).
#[derive(Debug, Clone)]
pub(crate) struct AuditSettings {
    pub poll_interval: Duration,
    pub max_backoff: Duration,
}

/// The audit-log shipper. Run it with [`AuditShipper::run`].
pub(crate) struct AuditShipper {
    client: GcsClient,
    settings: AuditSettings,
    state: PathBuf,
    /// Files shipped within the lookback.
    shipped: BTreeSet<String>,
    failures: u32,
    /// The last poll failed (logged once per change).
    failing: bool,
    clock: Clock,
}

/// One audit-log entry's fields we read (`LogEntry` with an `AuditLog`
/// payload). Everything is optional: a malformed or foreign entry still
/// ships what it has.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(default)]
    insert_id: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    proto_payload: Payload,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    #[serde(default)]
    method_name: Option<String>,
    #[serde(default)]
    resource_name: Option<String>,
    #[serde(default)]
    authentication_info: AuthenticationInfo,
    #[serde(default)]
    request_metadata: RequestMetadata,
    #[serde(default)]
    status: Option<Status>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticationInfo {
    #[serde(default)]
    principal_email: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RequestMetadata {
    #[serde(default)]
    caller_ip: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct Status {
    #[serde(default)]
    code: Option<i64>,
}

/// A `google.rpc.Code` by name; `OK` for an empty status (success).
fn status_name(status: Option<&Status>) -> String {
    const NAMES: [&str; 17] = [
        "OK",
        "CANCELLED",
        "UNKNOWN",
        "INVALID_ARGUMENT",
        "DEADLINE_EXCEEDED",
        "NOT_FOUND",
        "ALREADY_EXISTS",
        "PERMISSION_DENIED",
        "RESOURCE_EXHAUSTED",
        "FAILED_PRECONDITION",
        "ABORTED",
        "OUT_OF_RANGE",
        "UNIMPLEMENTED",
        "INTERNAL",
        "UNAVAILABLE",
        "DATA_LOSS",
        "UNAUTHENTICATED",
    ];
    let code = status.and_then(|s| s.code).unwrap_or(0);
    usize::try_from(code)
        .ok()
        .and_then(|i| NAMES.get(i))
        .map_or_else(|| code.to_string(), |name| (*name).to_string())
}

/// `text` truncated for a line (JSON logging escapes the rest).
fn shown(text: &str) -> String {
    if text.len() <= MAX_SHOWN {
        return text.to_string();
    }
    let mut end = MAX_SHOWN;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &text[..end])
}

/// The object an audit `resourceName` names
/// (`projects/_/buckets/<bucket>/objects/<name>`), if it is one.
fn object_of(resource: &str) -> Option<&str> {
    let rest = resource.strip_prefix("projects/_/buckets/")?;
    let (_, object) = rest.split_once("/objects/")?;
    Some(object)
}

/// The `YYYY/MM/DD/HH:MM:SS...` part of an audit file's name, for ordering
/// files of different logs by time.
fn when(name: &str) -> &str {
    name.strip_prefix(LOG_ROOT)
        .and_then(|rest| rest.split_once('/'))
        .map_or(name, |(_, when)| when)
}

/// Writes the line for one entry of `log`.
fn ship_entry(entry: &Entry, log: &str) {
    let payload = &entry.proto_payload;
    let resource = payload.resource_name.as_deref();
    let object = resource.and_then(object_of).map(shown);
    tracing::info!(
        target: TARGET,
        principal = payload.authentication_info.principal_email.as_deref(),
        caller_ip = payload.request_metadata.caller_ip.as_deref(),
        method = payload.method_name.as_deref(),
        object = object.as_deref(),
        resource = resource.map(shown).as_deref(),
        status = %status_name(payload.status.as_ref()),
        time = entry.timestamp.as_deref(),
        log,
        insert_id = entry.insert_id.as_deref(),
        "bucket access"
    );
}

enum PollError {
    Revoked(String),
    Failed(&'static str, String),
}

impl From<(&'static str, GcsError)> for PollError {
    fn from((kind, err): (&'static str, GcsError)) -> Self {
        match err {
            GcsError::Revoked(why) => Self::Revoked(why),
            GcsError::Auth(why) => Self::Failed("auth", why),
            GcsError::Failed(why) => Self::Failed(kind, why),
        }
    }
}

impl AuditShipper {
    pub(crate) fn new(
        client: GcsClient,
        settings: AuditSettings,
        storage_dir: &Path,
        clock: Clock,
    ) -> Self {
        register_metrics();
        let state = storage_dir
            .join("sources")
            .join("bucket")
            .join(".audit-shipped");
        let shipped = read_shipped(&state);
        Self {
            client,
            settings,
            state,
            shipped,
            failures: 0,
            failing: false,
            clock,
        }
    }

    /// Ships forever. Never returns, never panics on a bucket error.
    pub(crate) async fn run(mut self) {
        tracing::info!(
            audit_bucket = %self.client.bucket(),
            poll_interval_secs = self.settings.poll_interval.as_secs(),
            "bucket audit-log shipper started"
        );
        loop {
            let delay = self.poll_once().await;
            tokio::time::sleep(delay).await;
        }
    }

    /// One pass; returns how long to wait before the next.
    pub(crate) async fn poll_once(&mut self) -> Duration {
        match self.poll_inner().await {
            Ok(()) => {
                self.failures = 0;
                if std::mem::take(&mut self.failing) {
                    tracing::info!(audit_bucket = %self.client.bucket(), "bucket audit-log shipping recovered");
                }
                self.settings.poll_interval
            }
            Err(PollError::Revoked(why)) => {
                count_error("auth");
                if !std::mem::replace(&mut self.failing, true) {
                    tracing::error!(audit_bucket = %self.client.bucket(), reason = %why, retry_secs = self.settings.max_backoff.as_secs(), "the audit-log bucket refused the reader; no bucket audit lines until it is allowed again");
                }
                self.settings.max_backoff
            }
            Err(PollError::Failed(kind, why)) => {
                count_error(kind);
                self.failures = self.failures.saturating_add(1);
                if !std::mem::replace(&mut self.failing, true) {
                    tracing::warn!(audit_bucket = %self.client.bucket(), call = kind, error = %why, "bucket audit-log shipping failed; backing off");
                }
                let factor = 1u32 << self.failures.min(20);
                self.settings
                    .poll_interval
                    .checked_mul(factor)
                    .unwrap_or(self.settings.max_backoff)
                    .min(self.settings.max_backoff)
            }
        }
    }

    async fn poll_inner(&mut self) -> Result<(), PollError> {
        let now: DateTime<Utc> = (self.clock)().into();
        let since = (now - chrono::Duration::days(LOOKBACK_DAYS))
            .format("%Y/%m/%d/")
            .to_string();
        let logs = self
            .client
            .list(LOG_ROOT, true, None)
            .await
            .map_err(|err| PollError::from(("list", err)))?
            .prefixes;
        let mut starts = Vec::new();
        let mut pending: Vec<(String, ObjectMeta)> = Vec::new();
        for log_prefix in &logs {
            let start = format!("{log_prefix}{since}");
            let listing = self
                .client
                .list(log_prefix, false, Some(&start))
                .await
                .map_err(|err| PollError::from(("list", err)))?;
            let log = log_prefix
                .trim_start_matches(LOG_ROOT)
                .trim_end_matches('/')
                .to_string();
            pending.extend(
                listing
                    .objects
                    .into_iter()
                    .filter(|o| {
                        o.name >= start
                            && !o.name.contains(char::is_control)
                            && !self.shipped.contains(&o.name)
                    })
                    .map(|o| (log.clone(), o)),
            );
            starts.push((log_prefix.clone(), start));
        }
        // Oldest first (by the hour in the name, then the shard), whatever
        // the log.
        pending.sort_by(|a, b| (when(&a.1.name), &a.1.name).cmp(&(when(&b.1.name), &b.1.name)));
        let mut result = Ok(());
        for (log, object) in pending.into_iter().take(MAX_FILES_PER_POLL) {
            if let Err(err) = self.ship_file(&log, &object).await {
                result = Err(err);
                break;
            }
        }
        // Forget names older than every log's lookback.
        let before = self.shipped.len();
        self.shipped.retain(|name| {
            starts
                .iter()
                .any(|(prefix, start)| name.starts_with(prefix.as_str()) && name >= start)
        });
        if self.shipped.len() != before {
            self.persist();
        }
        result
    }

    /// Downloads one audit file and ships its entries.
    async fn ship_file(&mut self, log: &str, object: &ObjectMeta) -> Result<(), PollError> {
        if object.size > MAX_FILE_BYTES {
            count_error("verify");
            tracing::warn!(file = %shown(&object.name), bytes = object.size, max_bytes = MAX_FILE_BYTES, "skipping an audit-log file larger than the cap");
            self.mark_shipped(&object.name);
            return Ok(());
        }
        let body = self.read(object).await?;
        let mut shipped = 0u64;
        let mut malformed = 0u64;
        for line in body.split(|&b| b == b'\n') {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            match serde_json::from_slice::<Entry>(line) {
                Ok(entry) => {
                    ship_entry(&entry, log);
                    shipped += 1;
                }
                Err(_) => malformed += 1,
            }
        }
        metrics::counter!(common::metrics::metric_name(LINES_METRIC)).increment(shipped);
        if malformed > 0 {
            metrics::counter!(common::metrics::metric_name(ERRORS_METRIC), "kind" => "parse")
                .increment(malformed);
            tracing::warn!(file = %shown(&object.name), malformed, "audit-log file has lines that are not JSON log entries; skipped them");
        }
        tracing::debug!(file = %shown(&object.name), entries = shipped, "shipped an audit-log file");
        self.mark_shipped(&object.name);
        Ok(())
    }

    /// `object`'s bytes, that generation, size and CRC32C checked.
    async fn read(&self, object: &ObjectMeta) -> Result<Vec<u8>, PollError> {
        let mut response = self
            .client
            .download(object)
            .await
            .map_err(|err| PollError::from(("get", err)))?;
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    body.extend_from_slice(&chunk);
                    if body.len() as u64 > object.size {
                        return Err(PollError::Failed(
                            "verify",
                            "more bytes than listed".to_string(),
                        ));
                    }
                }
                Ok(None) => break,
                Err(err) => {
                    return Err(PollError::Failed(
                        "get",
                        format!("download interrupted: {err}"),
                    ));
                }
            }
        }
        if body.len() as u64 != object.size {
            return Err(PollError::Failed(
                "verify",
                "fewer bytes than listed".to_string(),
            ));
        }
        let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI).checksum(&body);
        if object.crc32c != Some(crc) {
            return Err(PollError::Failed(
                "verify",
                "CRC32C does not match the object's".to_string(),
            ));
        }
        Ok(body)
    }

    fn mark_shipped(&mut self, name: &str) {
        self.shipped.insert(name.to_string());
        self.persist();
    }

    fn persist(&self) {
        if let Err(err) = write_shipped(&self.state, &self.shipped) {
            tracing::error!(error = %err, "failed to persist the shipped audit-log files; a restart may ship some again");
        }
    }
}

fn count_error(kind: &'static str) {
    metrics::counter!(common::metrics::metric_name(ERRORS_METRIC), "kind" => kind).increment(1);
}

/// Every shipper metric at 0.
pub(crate) fn register_metrics() {
    metrics::counter!(common::metrics::metric_name(LINES_METRIC)).increment(0);
    for kind in ERROR_KINDS {
        metrics::counter!(common::metrics::metric_name(ERRORS_METRIC), "kind" => kind).increment(0);
    }
}

fn read_shipped(path: &Path) -> BTreeSet<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => text
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
        Err(err) => {
            tracing::error!(error = %err, "failed to read the shipped audit-log files; recent ones may be shipped again");
            BTreeSet::new()
        }
    }
}

fn write_shipped(path: &Path, shipped: &BTreeSet<String>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    {
        let mut out = std::fs::File::create(&temp)?;
        for name in shipped {
            // Never a newline: names with control characters are not
            // shipped (`poll_inner`).
            writeln!(out, "{name}")?;
        }
        out.sync_all()?;
    }
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::SystemTime;

    use common::secret::Secret;
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    use super::*;
    use crate::gcs::Credentials;
    use crate::gcs::tests::object_json;

    const AUDIT_BUCKET: &str = "example-ds-ingest-audit";
    const READER: &str = "reader@example-project.iam.gserviceaccount.com";
    const OBJECT: &str = "projects/_/buckets/example-ds-ingest/objects/timetable_full.zip";

    /// Names and bytes.
    type Objects = Arc<Mutex<Vec<(String, Vec<u8>)>>>;

    /// A fake audit-log bucket: names and bytes, listed as the JSON API
    /// does (prefix, delimiter, startOffset).
    #[derive(Clone, Default)]
    struct FakeAuditBucket {
        objects: Objects,
        /// Answer every call with 403.
        refuse: Arc<Mutex<bool>>,
        /// Flip the first byte of this object's downloads (a CRC mismatch).
        corrupt: Arc<Mutex<Option<String>>>,
    }

    impl FakeAuditBucket {
        fn put(&self, name: &str, contents: &str) {
            self.objects
                .lock()
                .unwrap()
                .push((name.to_string(), contents.as_bytes().to_vec()));
        }
    }

    impl Respond for FakeAuditBucket {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            if *self.refuse.lock().unwrap() {
                return ResponseTemplate::new(403);
            }
            let query = |key: &str| {
                request
                    .url
                    .query_pairs()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.into_owned())
            };
            let objects = self.objects.lock().unwrap();
            let list = format!("/storage/v1/b/{AUDIT_BUCKET}/o");
            let path = request.url.path().to_string();
            if path == list {
                let prefix = query("prefix").unwrap_or_default();
                let start = query("startOffset").unwrap_or_default();
                let delimiter = query("delimiter").is_some();
                let mut items = Vec::new();
                let mut prefixes = BTreeSet::new();
                for (i, (name, bytes)) in objects.iter().enumerate() {
                    let Some(rest) = name.strip_prefix(&prefix) else {
                        continue;
                    };
                    if name.as_str() < start.as_str() {
                        continue;
                    }
                    match rest.find('/') {
                        Some(at) if delimiter => {
                            prefixes.insert(format!("{prefix}{}", &rest[..=at]));
                        }
                        _ => items.push(object_json(name, bytes, i64::try_from(i).unwrap() + 1)),
                    }
                }
                return ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "items": items, "prefixes": prefixes }));
            }
            let encoded = path.trim_start_matches(&format!("{list}/"));
            let name = encoded.replace("%2F", "/").replace("%3A", ":");
            match objects.iter().find(|(n, _)| *n == name) {
                Some((_, bytes)) => {
                    let mut bytes = bytes.clone();
                    if self.corrupt.lock().unwrap().as_deref() == Some(name.as_str()) {
                        bytes[0] ^= 0xff;
                    }
                    ResponseTemplate::new(200).set_body_bytes(bytes)
                }
                None => ResponseTemplate::new(404),
            }
        }
    }

    /// One Cloud Audit Logs entry for a Cloud Storage call, as a Logging
    /// sink writes it.
    fn entry(
        method: &str,
        resource: &str,
        principal: &str,
        status: &serde_json::Value,
        at: &str,
    ) -> String {
        serde_json::json!({
            "insertId": format!("{method}-{at}"),
            "logName": "projects/example-project/logs/cloudaudit.googleapis.com%2Fdata_access",
            "protoPayload": {
                "@type": "type.googleapis.com/google.cloud.audit.AuditLog",
                "status": status,
                "authenticationInfo": { "principalEmail": principal },
                "requestMetadata": {
                    "callerIp": "203.0.113.7",
                    "callerSuppliedUserAgent": "distant-signal-schedule-ingest/0.1.0"
                },
                "serviceName": "storage.googleapis.com",
                "methodName": method,
                "resourceName": resource,
            },
            "resource": { "type": "gcs_bucket", "labels": { "bucket_name": "example-ds-ingest" } },
            "timestamp": at,
            "severity": "INFO",
        })
        .to_string()
    }

    fn get(at: &str) -> String {
        entry(
            "storage.objects.get",
            OBJECT,
            READER,
            &serde_json::json!({}),
            at,
        )
    }

    fn data_access(day: &str, hour: &str, shard: u8) -> String {
        format!(
            "cloudaudit.googleapis.com/data_access/{day}/{hour}:00:00_{hour}:59:59_S{shard}.json"
        )
    }

    struct Fixture {
        server: MockServer,
        bucket: FakeAuditBucket,
        storage: tempfile::TempDir,
        now: Arc<Mutex<SystemTime>>,
    }

    impl Fixture {
        async fn new() -> Self {
            let server = MockServer::start().await;
            let bucket = FakeAuditBucket::default();
            Mock::given(wiremock::matchers::any())
                .respond_with(bucket.clone())
                .mount(&server)
                .await;
            let now: DateTime<Utc> = "2026-10-21T12:00:00Z".parse().unwrap();
            Self {
                server,
                bucket,
                storage: tempfile::tempdir().unwrap(),
                now: Arc::new(Mutex::new(now.into())),
            }
        }

        fn shipper(&self) -> AuditShipper {
            let client = GcsClient::new(
                Some(&self.server.uri()),
                AUDIT_BUCKET,
                Credentials::Static(Secret::new("t")),
            )
            .unwrap();
            let now = Arc::clone(&self.now);
            AuditShipper::new(
                client,
                AuditSettings {
                    poll_interval: Duration::from_secs(600),
                    max_backoff: Duration::from_secs(3600),
                },
                self.storage.path(),
                Arc::new(move || *now.lock().unwrap()),
            )
        }

        /// One poll; its delay and its `bucket_access` lines.
        async fn poll(&self, shipper: &mut AuditShipper) -> (Duration, Vec<serde_json::Value>) {
            let (guard, logs) = crate::audit::tests::capture_default();
            let delay = shipper.poll_once().await;
            drop(guard);
            let lines = logs
                .lines()
                .into_iter()
                .filter(|l| l["target"] == TARGET)
                .collect();
            (delay, lines)
        }

        async fn media_gets(&self) -> usize {
            self.server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.url.query().unwrap_or("").contains("alt=media"))
                .count()
        }
    }

    #[tokio::test]
    async fn every_entry_becomes_one_bucket_access_line_with_the_contract_fields() {
        let f = Fixture::new().await;
        let lines = [
            get("2026-10-21T10:15:00.123Z"),
            "not json".to_string(),
            String::new(),
            entry(
                "storage.objects.delete",
                OBJECT,
                "publisher@example-dtd.iam.gserviceaccount.com",
                &serde_json::json!({ "code": 7, "message": "PERMISSION_DENIED" }),
                "2026-10-21T10:16:00Z",
            ),
        ];
        f.bucket
            .put(&data_access("2026/10/21", "10", 0), &lines.join("\n"));
        f.bucket.put(
            "cloudaudit.googleapis.com/activity/2026/10/21/09:00:00_09:59:59_S0.json",
            &entry(
                "storage.setIamPermissions",
                "projects/_/buckets/example-ds-ingest",
                "crossplane@example-project.iam.gserviceaccount.com",
                &serde_json::json!({}),
                "2026-10-21T09:30:00Z",
            ),
        );
        let mut shipper = f.shipper();
        let (delay, lines) = f.poll(&mut shipper).await;
        assert_eq!(delay, Duration::from_secs(600));
        assert_eq!(lines.len(), 3, "{lines:?}");
        // Oldest hour first, whichever log.
        assert_eq!(lines[0]["method"], "storage.setIamPermissions");
        assert_eq!(lines[0]["log"], "activity");
        assert!(lines[0].get("object").is_none(), "a bucket, not an object");
        assert_eq!(lines[0]["resource"], "projects/_/buckets/example-ds-ingest");
        let read = &lines[1];
        assert_eq!(read["message"], "bucket access");
        assert_eq!(read["level"], "INFO");
        assert_eq!(read["principal"], READER);
        assert_eq!(read["caller_ip"], "203.0.113.7");
        assert_eq!(read["method"], "storage.objects.get");
        assert_eq!(read["object"], "timetable_full.zip");
        assert_eq!(read["status"], "OK");
        assert_eq!(read["time"], "2026-10-21T10:15:00.123Z");
        assert_eq!(read["log"], "data_access");
        assert_eq!(
            read["insert_id"],
            "storage.objects.get-2026-10-21T10:15:00.123Z"
        );
        assert_eq!(lines[2]["status"], "PERMISSION_DENIED");
    }

    /// A restart ships nothing again; a late shard of an earlier hour is
    /// still shipped; folders older than the lookback are never listed, and
    /// the record forgets them.
    #[tokio::test]
    async fn each_file_is_shipped_once_late_shards_included() {
        let f = Fixture::new().await;
        f.bucket.put(
            &data_access("2026/10/18", "23", 0),
            &get("2026-10-18T23:10:00Z"),
        );
        f.bucket.put(
            &data_access("2026/10/21", "10", 0),
            &get("2026-10-21T10:10:00Z"),
        );
        f.bucket.put(
            &data_access("2026/10/21", "11", 0),
            &get("2026-10-21T11:10:00Z"),
        );
        let mut shipper = f.shipper();
        let (_, lines) = f.poll(&mut shipper).await;
        assert_eq!(lines.len(), 2, "the 18th is beyond the lookback");
        drop(shipper);

        let mut shipper = f.shipper();
        let (_, lines) = f.poll(&mut shipper).await;
        assert!(lines.is_empty(), "{lines:?}");
        f.bucket.put(
            &data_access("2026/10/21", "10", 1),
            &get("2026-10-21T10:59:59Z"),
        );
        let (_, lines) = f.poll(&mut shipper).await;
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["time"], "2026-10-21T10:59:59Z");
        assert_eq!(f.media_gets().await, 3);

        // Three days on, the record holds nothing from the 21st.
        *f.now.lock().unwrap() += Duration::from_secs(3 * 86_400);
        f.poll(&mut shipper).await;
        let record =
            std::fs::read_to_string(f.storage.path().join("sources/bucket/.audit-shipped"))
                .unwrap();
        assert!(record.is_empty(), "{record}");
    }

    #[tokio::test]
    async fn a_refusal_backs_off_at_the_ceiling_and_a_bad_file_is_retried() {
        let f = Fixture::new().await;
        let name = data_access("2026/10/21", "10", 0);
        f.bucket.put(&name, &get("2026-10-21T10:10:00Z"));
        let mut shipper = f.shipper();
        *f.bucket.refuse.lock().unwrap() = true;
        assert_eq!(f.poll(&mut shipper).await.0, Duration::from_secs(3600));
        *f.bucket.refuse.lock().unwrap() = false;

        *f.bucket.corrupt.lock().unwrap() = Some(name);
        let (delay, lines) = f.poll(&mut shipper).await;
        assert_eq!(delay, Duration::from_secs(1200), "backs off");
        assert!(lines.is_empty());
        *f.bucket.corrupt.lock().unwrap() = None;
        let (delay, lines) = f.poll(&mut shipper).await;
        assert_eq!(delay, Duration::from_secs(600));
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn statuses_are_named_and_objects_are_parsed_from_the_resource() {
        assert_eq!(status_name(None), "OK");
        assert_eq!(
            status_name(Some(&Status { code: Some(16) })),
            "UNAUTHENTICATED"
        );
        assert_eq!(status_name(Some(&Status { code: Some(99) })), "99");
        assert_eq!(object_of(OBJECT), Some("timetable_full.zip"));
        assert_eq!(
            object_of("projects/_/buckets/b/objects/a/objects/b"),
            Some("a/objects/b")
        );
        assert_eq!(object_of("projects/_/buckets/b"), None);
        assert!(shown(&"é".repeat(1000)).len() <= MAX_SHOWN + 3);
        assert_eq!(
            when("cloudaudit.googleapis.com/activity/2026/10/21/09:00:00_09:59:59_S0.json"),
            "2026/10/21/09:00:00_09:59:59_S0.json"
        );
    }
}
