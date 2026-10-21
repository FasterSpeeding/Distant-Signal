//! The GCS bucket delivery source (`BUCKET_SOURCE_ENABLED`; the GCS spec
//! §5 "An object's life" and §9 "Loop and revocation guards").
//!
//! A task of its own, independent of the scan loop: every
//! `BUCKET_POLL_INTERVAL_SECS` it lists the bucket root (`gcs.rs`) and, for
//! each object:
//!
//! - **expected** (its name matches `BUCKET_EXPECTED_KEYS` *and* routes to
//!   CIF or CORPUS, its listed size is within `BUCKET_MAX_OBJECT_BYTES`)
//!   and not yet confirmed (its `(name, generation)` is not in the seen
//!   ledger, `sources/bucket/.seen`): downloaded, exactly that generation,
//!   into `sources/bucket/.partial-<name>`, hashing SHA-256, CRC32C and MD5
//!   as it streams; size, CRC32C and (when GCS has one) MD5 checked; fsynced;
//!   renamed into `sources/bucket/archive/` with the object's creation time
//!   as its mtime; recorded in the seen ledger; then **handed off** by
//!   hard-linking it into `sources/bucket/incoming/<name>` (an atomic
//!   rename), the bucket lane's landing folder. The scan and CORPUS cycles
//!   treat that folder exactly as they treat `WATCH_DIR` (extraction caps,
//!   CIF checks, quarantine, CORPUS formats), with the creation time as
//!   `delivered_at`. A failed check keeps the object and retries later.
//! - **unexpected** (anything else): never downloaded. One log line (the
//!   name escaped and truncated: the publisher controls it) and
//!   `schedule_feed_source_unexpected_objects_total{reason}` once per
//!   generation.
//!
//! Once a confirmed or unexpected object is `BUCKET_DELETE_MIN_AGE_SECS`
//! old it is deleted with `ifGenerationMatch`: a newer upload in between is
//! never lost (412), and one already gone (404) is fine. The ingest outcome
//! never gates the delete.
//!
//! Guards: a confirmed generation is never downloaded again, even across
//! restarts; at most `BUCKET_MAX_DOWNLOADS_PER_POLL` downloads a poll, and
//! `BUCKET_MAX_DOWNLOAD_BYTES_PER_HOUR` / `_PER_DAY` over rolling windows
//! (`schedule_feed_source_download_capped`); errors back off exponentially
//! up to `BUCKET_MAX_BACKOFF_SECS`; revoked access (401/403, no usable
//! credential) sets `schedule_feed_source_access_revoked`, logs once per
//! change and retries at the backoff ceiling. Nothing here can stop the SFTP
//! lane or the process.
//!
//! With Pub/Sub notifications on (`pubsub.rs`, [`Wake`]), an
//! `OBJECT_FINALIZE` message wakes the source for a poll early (at most one
//! every [`Wake`]'s minimum gap, and never during a backoff), and while the
//! subscription is healthy the regular poll stretches to
//! `BUCKET_PUBSUB_RECONCILE_INTERVAL_SECS`. A message is only a hint: the
//! listing decides what is fetched, so a lost, duplicated or forged message
//! changes nothing but the timing, and with the subscription failing the
//! source polls every `BUCKET_POLL_INTERVAL_SECS` as before.

use std::collections::{HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::Digest as _;

use crate::delivery::delivery_dir_name;
use crate::gcs::{Deleted, GcsClient, GcsError, ObjectMeta};
use crate::pattern::{FilePattern, Routing};

/// The `source` label of every bucket metric.
const SOURCE: &str = "bucket";
pub(crate) const ACCESS_REVOKED_METRIC: &str = "schedule_feed_source_access_revoked";
pub(crate) const LAST_NEW_OBJECT_METRIC: &str = "schedule_feed_source_last_new_object_seconds";
pub(crate) const ERRORS_METRIC: &str = "schedule_feed_source_errors_total";
pub(crate) const UNEXPECTED_METRIC: &str = "schedule_feed_source_unexpected_objects_total";
pub(crate) const DOWNLOAD_CAPPED_METRIC: &str = "schedule_feed_source_download_capped";
pub(crate) const DOWNLOADED_BYTES_METRIC: &str = "schedule_feed_source_downloaded_bytes_total";
pub(crate) const POLL_DURATION_METRIC: &str = "schedule_feed_source_poll_duration_seconds";
/// `kind` of [`ERRORS_METRIC`]. `DistantSignalScheduleBucketReadErrors`
/// reads every kind but `auth`.
pub(crate) const ERROR_KINDS: [&str; 6] = ["auth", "list", "get", "verify", "delete", "size"];
/// `reason` of [`UNEXPECTED_METRIC`].
pub(crate) const UNEXPECTED_REASONS: [&str; 3] = ["name", "size", "unroutable"];

/// Confirmed generations kept in the seen ledger.
const SEEN_KEEP: usize = 200;
/// Longest object name shown in a log line.
const MAX_LOGGED_NAME: usize = 256;
const HOUR: Duration = Duration::from_secs(3600);
const DAY: Duration = Duration::from_secs(24 * 3600);

/// The bucket source's settings (from `config::BucketArgs`).
#[derive(Debug, Clone)]
pub(crate) struct BucketSettings {
    pub expected: FilePattern,
    pub routing: Routing,
    pub poll_interval: Duration,
    pub delete_min_age: Duration,
    pub archive_keep: u32,
    pub max_object_bytes: u64,
    pub max_downloads_per_poll: u32,
    pub max_bytes_per_hour: u64,
    pub max_bytes_per_day: u64,
    pub max_backoff: Duration,
}

/// The bucket source's directories under `storage_dir`.
#[derive(Debug, Clone)]
pub(crate) struct BucketDirs {
    root: PathBuf,
    archive: PathBuf,
    /// The bucket lane's landing folder: the scan and CORPUS cycles' watch
    /// directory for this source.
    pub incoming: PathBuf,
    seen: PathBuf,
}

impl BucketDirs {
    pub(crate) fn new(storage_dir: &Path) -> Self {
        let root = storage_dir.join("sources").join("bucket");
        Self {
            archive: root.join("archive"),
            incoming: root.join("incoming"),
            seen: root.join(".seen"),
            root,
        }
    }
}

/// One confirmed generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    name: String,
    generation: i64,
    bytes: u64,
    crc32c: Option<u32>,
    sha256: String,
    time_created: DateTime<Utc>,
}

/// What one listed object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    /// Expected and not confirmed: download it.
    New,
    /// Already confirmed: only the delete is left.
    Confirmed,
    /// Never downloaded; deleted once old enough.
    Unexpected(&'static str),
}

/// A wall clock (tests substitute their own).
pub(crate) type Clock = Arc<dyn Fn() -> SystemTime + Send + Sync>;

/// Wakes the bucket source early: shared between it and the Pub/Sub
/// subscriber (`pubsub.rs`).
pub(crate) struct Wake {
    notify: tokio::sync::Notify,
    /// `(polls started, polls finished)`.
    progress: tokio::sync::watch::Sender<(u64, u64)>,
    /// The subscription is delivering (its last pull succeeded).
    healthy: AtomicBool,
    /// Least time from one poll's start to a woken one's.
    min_gap: Duration,
}

impl Wake {
    pub(crate) fn new(min_gap: Duration) -> Arc<Self> {
        Arc::new(Self {
            notify: tokio::sync::Notify::new(),
            progress: tokio::sync::watch::channel((0, 0)).0,
            healthy: AtomicBool::new(false),
            min_gap,
        })
    }

    /// Asks for a poll soon. Returns a ticket for [`Wake::wait`].
    pub(crate) fn request(&self) -> u64 {
        let started = self.progress.borrow().0;
        self.notify.notify_one();
        started + 1
    }

    /// Returns once a poll that started after `ticket`'s request has
    /// finished.
    pub(crate) async fn wait(&self, ticket: u64) {
        let mut progress = self.progress.subscribe();
        // The sender lives in `self`, so this never errors.
        let _ = progress.wait_for(|&(_, done)| done >= ticket).await;
    }

    pub(crate) fn set_healthy(&self, healthy: bool) {
        self.healthy.store(healthy, Ordering::Relaxed);
    }

    pub(crate) fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }

    fn started(&self) {
        self.progress.send_modify(|(started, _)| *started += 1);
    }

    fn finished(&self) {
        self.progress
            .send_modify(|(started, done)| *done = *started);
    }
}

/// The bucket source. Run it with [`BucketSource::run`].
pub(crate) struct BucketSource {
    client: GcsClient,
    settings: BucketSettings,
    dirs: BucketDirs,
    seen: Vec<Seen>,
    /// `(when, bytes)` of every download within the last day.
    budget: VecDeque<(SystemTime, u64)>,
    revoked: bool,
    capped: bool,
    /// Consecutive failed polls (the backoff exponent).
    failures: u32,
    /// `(name, generation)` already reported as unexpected.
    reported: HashSet<(String, i64)>,
    clock: Clock,
    /// Pub/Sub on: the wake-up, and the poll interval while it is healthy.
    wake: Option<(Arc<Wake>, Duration)>,
}

impl BucketSource {
    pub(crate) fn new(client: GcsClient, settings: BucketSettings, dirs: BucketDirs) -> Self {
        Self::with_clock(client, settings, dirs, Arc::new(SystemTime::now))
    }

    pub(crate) fn with_clock(
        client: GcsClient,
        settings: BucketSettings,
        dirs: BucketDirs,
        clock: Clock,
    ) -> Self {
        register_metrics();
        let seen = read_seen(&dirs.seen);
        if let Some(newest) = seen.iter().map(|s| s.time_created).max() {
            set_last_new_object(newest);
        }
        remove_partials(&dirs.root);
        Self {
            client,
            settings,
            dirs,
            seen,
            budget: VecDeque::new(),
            revoked: false,
            capped: false,
            failures: 0,
            reported: HashSet::new(),
            clock,
            wake: None,
        }
    }

    /// Lets `wake` start a poll early, and polls only every `reconcile`
    /// while its subscription is healthy.
    pub(crate) fn with_wake(mut self, wake: Arc<Wake>, reconcile: Duration) -> Self {
        self.wake = Some((wake, reconcile));
        self
    }

    /// Polls forever. Never returns, never panics on a bucket error.
    pub(crate) async fn run(mut self) {
        tracing::info!(
            bucket = %self.client.bucket(),
            poll_interval_secs = self.settings.poll_interval.as_secs(),
            "bucket source started"
        );
        loop {
            let started = Instant::now();
            if let Some((wake, _)) = &self.wake {
                wake.started();
            }
            let delay = self.poll_once().await;
            if let Some((wake, _)) = &self.wake {
                wake.finished();
            }
            self.wait_next(started, delay).await;
        }
    }

    /// Waits out `delay` after the poll that began at `started`; with
    /// Pub/Sub, the healthy interval instead, cut short by a wake-up (but
    /// never a backoff).
    async fn wait_next(&self, started: Instant, delay: Duration) {
        let Some((wake, _)) = &self.wake else {
            return tokio::time::sleep(delay).await;
        };
        if self.failures > 0 || self.revoked {
            return tokio::time::sleep(delay).await;
        }
        tokio::select! {
            () = tokio::time::sleep(self.idle_delay(delay)) => {}
            () = wake.notify.notified() => {
                tokio::time::sleep_until((started + wake.min_gap).into()).await;
            }
        }
    }

    /// The wait after a successful poll: the reconcile interval while the
    /// subscription is healthy, else `delay` (the poll interval).
    fn idle_delay(&self, delay: Duration) -> Duration {
        match &self.wake {
            Some((wake, reconcile)) if wake.is_healthy() => delay.max(*reconcile),
            _ => delay,
        }
    }

    /// One listing and everything it leads to. Returns how long to wait
    /// before the next.
    pub(crate) async fn poll_once(&mut self) -> Duration {
        let started = Instant::now();
        let result = self.poll_inner().await;
        metrics::histogram!(common::metrics::metric_name(POLL_DURATION_METRIC), "source" => SOURCE)
            .record(started.elapsed().as_secs_f64());
        match result {
            Ok(()) => {
                self.failures = 0;
                self.settings.poll_interval
            }
            Err(PollError::Revoked) => self.settings.max_backoff,
            Err(PollError::Failed) => {
                self.failures = self.failures.saturating_add(1);
                backoff(
                    self.settings.poll_interval,
                    self.settings.max_backoff,
                    self.failures,
                )
            }
        }
    }

    async fn poll_inner(&mut self) -> Result<(), PollError> {
        let objects = match self.client.list_root().await {
            Ok(objects) => objects,
            Err(err) => return Err(self.fail("list", &err)),
        };
        self.set_revoked(false, None);
        let mut objects: Vec<(ObjectMeta, Class)> = objects
            .into_iter()
            .map(|object| {
                let class = self.classify(&object);
                (object, class)
            })
            .collect();
        objects.sort_by(|a, b| {
            a.0.time_created
                .cmp(&b.0.time_created)
                .then_with(|| a.0.name.cmp(&b.0.name))
        });

        let mut failed = false;
        let mut downloads = 0u32;
        let mut refused_by_budget = false;
        for (object, class) in &mut objects {
            match *class {
                Class::Unexpected(reason) => self.report_unexpected(object, reason),
                Class::Confirmed => {}
                Class::New => {
                    if downloads >= self.settings.max_downloads_per_poll || refused_by_budget {
                        continue;
                    }
                    if !self.budget_allows(object.size) {
                        refused_by_budget = true;
                        continue;
                    }
                    downloads += 1;
                    match self.fetch(object).await {
                        Ok(()) => *class = Class::Confirmed,
                        Err(Some(err)) => return Err(err),
                        Err(None) => failed = true,
                    }
                }
            }
        }
        self.set_capped(refused_by_budget);

        let now = (self.clock)();
        for (object, class) in &objects {
            if matches!(class, Class::New) {
                continue;
            }
            let age = now
                .duration_since(SystemTime::from(object.time_created))
                .unwrap_or_default();
            if age < self.settings.delete_min_age {
                continue;
            }
            match self.client.delete(&object.name, object.generation).await {
                Ok(Deleted::Yes) => {
                    tracing::info!(object = %shown(&object.name), generation = object.generation, "deleted a handled object from the bucket");
                }
                Ok(Deleted::Gone) => {
                    tracing::info!(object = %shown(&object.name), generation = object.generation, "a handled object was already gone from the bucket");
                }
                Ok(Deleted::Newer) => {
                    tracing::info!(object = %shown(&object.name), generation = object.generation, "a newer upload replaced a handled object before its delete; keeping it for the next poll");
                }
                Err(err) => {
                    if let PollError::Revoked = self.fail("delete", &err) {
                        return Err(PollError::Revoked);
                    }
                    failed = true;
                }
            }
        }
        if failed {
            Err(PollError::Failed)
        } else {
            Ok(())
        }
    }

    fn classify(&self, object: &ObjectMeta) -> Class {
        if !is_safe_name(&object.name) || !self.settings.expected.matches(&object.name) {
            return Class::Unexpected("name");
        }
        if !(self.settings.routing.is_cif(&object.name)
            || self.settings.routing.is_corpus(&object.name))
        {
            return Class::Unexpected("unroutable");
        }
        if object.size > self.settings.max_object_bytes {
            return Class::Unexpected("size");
        }
        if self
            .seen
            .iter()
            .any(|s| s.name == object.name && s.generation == object.generation)
        {
            Class::Confirmed
        } else {
            Class::New
        }
    }

    fn report_unexpected(&mut self, object: &ObjectMeta, reason: &'static str) {
        if !self
            .reported
            .insert((object.name.clone(), object.generation))
        {
            return;
        }
        metrics::counter!(
            common::metrics::metric_name(UNEXPECTED_METRIC),
            "source" => SOURCE,
            "reason" => reason
        )
        .increment(1);
        if reason == "size" {
            count_error("size");
        }
        tracing::warn!(
            object = %shown(&object.name),
            bytes = object.size,
            generation = object.generation,
            reason,
            "unexpected object in the schedule-feed bucket: never downloaded, deleted unread once BUCKET_DELETE_MIN_AGE_SECS old"
        );
    }

    /// Whether `size` more bytes fit the rolling hour and day.
    fn budget_allows(&mut self, size: u64) -> bool {
        let now = (self.clock)();
        let within =
            |at: SystemTime, window: Duration| now.duration_since(at).unwrap_or_default() < window;
        while self.budget.front().is_some_and(|&(at, _)| !within(at, DAY)) {
            self.budget.pop_front();
        }
        let hour: u64 = self
            .budget
            .iter()
            .filter(|&&(at, _)| within(at, HOUR))
            .map(|&(_, bytes)| bytes)
            .sum();
        let day: u64 = self.budget.iter().map(|&(_, bytes)| bytes).sum();
        hour.saturating_add(size) <= self.settings.max_bytes_per_hour
            && day.saturating_add(size) <= self.settings.max_bytes_per_day
    }

    /// Downloads, verifies, archives, records and hands off one object.
    /// `Err(Some(Revoked))` stops the poll; `Err(None)` is a counted
    /// failure that leaves the object for a later poll.
    async fn fetch(&mut self, object: &ObjectMeta) -> Result<(), Option<PollError>> {
        let partial = self.dirs.root.join(format!(".partial-{}", object.name));
        let result = self.download_to(object, &partial).await;
        // Every byte received counts against the caps, a failed
        // verification's too.
        let received = match &result {
            Ok((_, received)) | Err((_, _, received)) => *received,
        };
        self.budget.push_back(((self.clock)(), received));
        metrics::counter!(common::metrics::metric_name(DOWNLOADED_BYTES_METRIC), "source" => SOURCE)
            .increment(received);
        let sha256 = match result {
            Ok((sha256, _)) => sha256,
            Err((kind, err, _)) => {
                let _ = std::fs::remove_file(&partial);
                return Err(match self.fail(kind, &err) {
                    PollError::Revoked => Some(PollError::Revoked),
                    PollError::Failed => None,
                });
            }
        };
        match self.land(object, &partial) {
            Ok(()) => {}
            Err(err) => {
                let _ = std::fs::remove_file(&partial);
                tracing::error!(error = %err, object = %shown(&object.name), "failed to archive a verified bucket object locally; retrying next poll");
                count_error("get");
                return Err(None);
            }
        }
        self.seen.push(Seen {
            name: object.name.clone(),
            generation: object.generation,
            bytes: object.size,
            crc32c: object.crc32c,
            sha256: sha256.clone(),
            time_created: object.time_created,
        });
        let excess = self.seen.len().saturating_sub(SEEN_KEEP);
        self.seen.drain(..excess);
        if let Err(err) = write_seen(&self.dirs.seen, &self.seen) {
            tracing::error!(error = %err, "failed to persist the bucket's seen ledger; a restart may download this object once more");
        }
        set_last_new_object(object.time_created);
        tracing::info!(
            object = %shown(&object.name),
            generation = object.generation,
            bytes = object.size,
            sha256 = %sha256,
            time_created = %object.time_created,
            "downloaded and verified a bucket object; handed to the pipeline"
        );
        Ok(())
    }

    /// Streams `object` into `partial`, checking it against its metadata.
    /// Returns its SHA-256 and the bytes received; an error carries the
    /// `errors_total` kind and the bytes received so far.
    async fn download_to(
        &self,
        object: &ObjectMeta,
        partial: &Path,
    ) -> Result<(String, u64), (&'static str, GcsError, u64)> {
        let mut response = self
            .client
            .download(object)
            .await
            .map_err(|err| ("get", err, 0))?;
        let io = |err: std::io::Error, received| {
            (
                "get",
                GcsError::Failed(format!("writing the download: {err}")),
                received,
            )
        };
        std::fs::create_dir_all(&self.dirs.root).map_err(|err| io(err, 0))?;
        let mut file = std::fs::File::create(partial).map_err(|err| io(err, 0))?;
        let crc = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
        let mut crc_digest = crc.digest();
        let mut sha = sha2::Sha256::new();
        let mut md5 = <md5::Md5 as md5::Digest>::new();
        let mut received: u64 = 0;
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(err) => {
                    return Err((
                        "get",
                        GcsError::Failed(format!("download interrupted: {err}")),
                        received,
                    ));
                }
            };
            received += chunk.len() as u64;
            if received > object.size {
                return Err((
                    "verify",
                    GcsError::Failed(format!(
                        "more bytes than the listed {} (a new upload?)",
                        object.size
                    )),
                    received,
                ));
            }
            crc_digest.update(&chunk);
            sha.update(&chunk);
            md5::Digest::update(&mut md5, &chunk);
            file.write_all(&chunk).map_err(|err| io(err, received))?;
        }
        let verify = |why: String| ("verify", GcsError::Failed(why), received);
        if received != object.size {
            return Err(verify(format!(
                "received {received} bytes, the listing says {}",
                object.size
            )));
        }
        let crc32c = crc_digest.finalize();
        match object.crc32c {
            Some(expected) if expected != crc32c => {
                return Err(verify(format!(
                    "CRC32C {crc32c:08x} does not match the object's {expected:08x}"
                )));
            }
            None => return Err(verify("the object has no CRC32C".to_string())),
            Some(_) => {}
        }
        if let Some(expected) = object.md5
            && <[u8; 16]>::from(md5::Digest::finalize(md5)) != expected
        {
            return Err(verify(
                "MD5 does not match the object's md5Hash".to_string(),
            ));
        }
        file.sync_all().map_err(|err| io(err, received))?;
        let sha256 = sha
            .finalize()
            .iter()
            .fold(String::with_capacity(64), |mut out, b| {
                use std::fmt::Write as _;
                let _ = write!(out, "{b:02x}");
                out
            });
        Ok((sha256, received))
    }

    /// Archives the verified `partial` (mtime = the object's creation
    /// time), prunes the archive, and hands it to the pipeline: a hard link
    /// renamed into the landing folder, replacing an older copy.
    fn land(&self, object: &ObjectMeta, partial: &Path) -> std::io::Result<()> {
        let created = SystemTime::from(object.time_created);
        std::fs::File::options()
            .write(true)
            .open(partial)?
            .set_modified(created)?;
        std::fs::create_dir_all(&self.dirs.archive)?;
        let archived = self.dirs.archive.join(format!(
            "{}-{}-{}",
            delivery_dir_name(created),
            object.generation,
            object.name
        ));
        std::fs::rename(partial, &archived)?;
        std::fs::create_dir_all(&self.dirs.incoming)?;
        let landing = self.dirs.incoming.join(format!(".landing-{}", object.name));
        let _ = std::fs::remove_file(&landing);
        std::fs::hard_link(&archived, &landing)?;
        std::fs::rename(&landing, self.dirs.incoming.join(&object.name))?;
        std::fs::File::open(&self.dirs.incoming)?.sync_all()?;
        prune_archive(&self.dirs.archive, self.settings.archive_keep);
        Ok(())
    }

    /// Counts and logs a failed call; revoked access is a state change.
    fn fail(&mut self, kind: &'static str, err: &GcsError) -> PollError {
        match err {
            GcsError::Revoked(why) => {
                count_error("auth");
                self.set_revoked(true, Some(why));
                PollError::Revoked
            }
            GcsError::Auth(why) => {
                count_error("auth");
                tracing::warn!(error = %why, "could not get a bucket access token; backing off");
                PollError::Failed
            }
            GcsError::Failed(why) => {
                count_error(kind);
                tracing::warn!(error = %why, call = kind, "bucket call failed; backing off");
                PollError::Failed
            }
        }
    }

    fn set_revoked(&mut self, revoked: bool, why: Option<&str>) {
        if revoked == self.revoked {
            return;
        }
        self.revoked = revoked;
        metrics::gauge!(common::metrics::metric_name(ACCESS_REVOKED_METRIC), "source" => SOURCE)
            .set(if revoked { 1.0 } else { 0.0 });
        if revoked {
            tracing::error!(
                reason = why.unwrap_or_default(),
                retry_secs = self.settings.max_backoff.as_secs(),
                "bucket access revoked (kill switch, or a missing or rotated credential); the SFTP source is unaffected"
            );
        } else {
            tracing::info!("bucket access restored");
        }
    }

    fn set_capped(&mut self, capped: bool) {
        if capped == self.capped {
            return;
        }
        self.capped = capped;
        metrics::gauge!(common::metrics::metric_name(DOWNLOAD_CAPPED_METRIC), "source" => SOURCE)
            .set(if capped { 1.0 } else { 0.0 });
        if capped {
            tracing::error!(
                max_bytes_per_hour = self.settings.max_bytes_per_hour,
                max_bytes_per_day = self.settings.max_bytes_per_day,
                "bucket download budget reached; no downloads until the window allows one (a loop or an attack?)"
            );
        } else {
            tracing::info!("bucket download budget available again");
        }
    }
}

enum PollError {
    Revoked,
    Failed,
}

/// `poll * 2^failures`, at most `max`.
fn backoff(poll: Duration, max: Duration, failures: u32) -> Duration {
    poll.checked_mul(1u32 << failures.min(20))
        .unwrap_or(max)
        .min(max)
}

fn count_error(kind: &'static str) {
    metrics::counter!(
        common::metrics::metric_name(ERRORS_METRIC),
        "source" => SOURCE,
        "kind" => kind
    )
    .increment(1);
}

#[expect(
    clippy::cast_precision_loss,
    reason = "a Unix timestamp stays far below 2^52"
)]
fn set_last_new_object(at: DateTime<Utc>) {
    metrics::gauge!(common::metrics::metric_name(LAST_NEW_OBJECT_METRIC), "source" => SOURCE)
        .set(at.timestamp() as f64);
}

/// Every bucket metric at 0, so the alerts' `increase()` sees the first
/// event.
pub(crate) fn register_metrics() {
    let gauge = |name| metrics::gauge!(common::metrics::metric_name(name), "source" => SOURCE);
    gauge(ACCESS_REVOKED_METRIC).set(0.0);
    gauge(DOWNLOAD_CAPPED_METRIC).set(0.0);
    gauge(LAST_NEW_OBJECT_METRIC).set(0.0);
    for kind in ERROR_KINDS {
        metrics::counter!(common::metrics::metric_name(ERRORS_METRIC), "source" => SOURCE, "kind" => kind)
            .increment(0);
    }
    for reason in UNEXPECTED_REASONS {
        metrics::counter!(common::metrics::metric_name(UNEXPECTED_METRIC), "source" => SOURCE, "reason" => reason)
            .increment(0);
    }
    metrics::counter!(common::metrics::metric_name(DOWNLOADED_BYTES_METRIC), "source" => SOURCE)
        .increment(0);
}

/// A name the reader may write to disk: one path component, not hidden,
/// no control characters, at most 255 bytes.
fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && !name.contains(['/', '\\'])
        && !name.chars().any(char::is_control)
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

fn read_seen(path: &Path) -> Vec<Seen> {
    match std::fs::read_to_string(path) {
        Ok(contents) => contents
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect(),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(err) => {
            tracing::error!(error = %err, "failed to read the bucket's seen ledger; confirmed objects may be downloaded once more");
            Vec::new()
        }
    }
}

/// Rewrites the seen ledger atomically.
fn write_seen(path: &Path, seen: &[Seen]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    {
        let mut out = std::fs::File::create(&temp)?;
        for entry in seen {
            writeln!(out, "{}", serde_json::to_string(entry)?)?;
        }
        out.sync_all()?;
    }
    std::fs::rename(&temp, path)?;
    Ok(())
}

/// Keeps the `keep` newest archived objects (names start with the
/// creation time).
fn prune_archive(dir: &Path, keep: u32) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = read_dir
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    let excess = names.len().saturating_sub(keep as usize);
    for name in &names[..excess] {
        if let Err(err) = std::fs::remove_file(dir.join(name)) {
            tracing::warn!(error = %err, file = %shown(name), "failed to prune an archived bucket object");
        }
    }
}

/// Removes downloads a crash left half-written.
fn remove_partials(root: &Path) {
    let Ok(read_dir) = std::fs::read_dir(root) else {
        return;
    };
    for entry in read_dir.filter_map(Result::ok) {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".partial-"))
        {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use common::secret::Secret;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

    use super::*;
    use crate::gcs::Credentials;
    use crate::gcs::tests::object_json;

    const BUCKET: &str = "example-ds-ingest";

    /// A fake bucket: a mutable set of objects behind the JSON API.
    #[derive(Clone, Default)]
    pub(crate) struct FakeBucket(Arc<Mutex<Vec<FakeObject>>>);

    /// An object's JSON resource and its bytes.
    type FakeObject = (serde_json::Value, Vec<u8>);

    impl FakeBucket {
        pub(crate) fn put(
            &self,
            name: &str,
            contents: &[u8],
            generation: i64,
            created: DateTime<Utc>,
        ) {
            let mut objects = self.0.lock().unwrap();
            objects.retain(|(o, _)| o["name"] != name);
            let mut json = object_json(name, contents, generation);
            json["timeCreated"] = created.to_rfc3339().into();
            objects.push((json, contents.to_vec()));
        }

        fn names(&self) -> Vec<String> {
            self.0
                .lock()
                .unwrap()
                .iter()
                .map(|(o, _)| o["name"].as_str().unwrap().to_string())
                .collect()
        }
    }

    impl Respond for FakeBucket {
        fn respond(&self, request: &Request) -> ResponseTemplate {
            let mut objects = self.0.lock().unwrap();
            let prefix = format!("/storage/v1/b/{BUCKET}/o");
            let path = request.url.path().to_string();
            let query = |key: &str| {
                request
                    .url
                    .query_pairs()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.into_owned())
            };
            if path == prefix && request.method == "GET" {
                let items: Vec<_> = objects.iter().map(|(o, _)| o.clone()).collect();
                return ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "items": items }));
            }
            let name = path.trim_start_matches(&format!("{prefix}/")).to_string();
            let name = name.replace("%2F", "/");
            let Some(at) = objects.iter().position(|(o, _)| o["name"] == name.as_str()) else {
                return ResponseTemplate::new(404);
            };
            let generation = objects[at].0["generation"].as_str().unwrap().to_string();
            match request.method.as_str() {
                "GET" => {
                    if query("generation").as_deref() != Some(generation.as_str()) {
                        return ResponseTemplate::new(404);
                    }
                    ResponseTemplate::new(200).set_body_bytes(objects[at].1.clone())
                }
                "DELETE" => {
                    if query("ifGenerationMatch").as_deref() != Some(generation.as_str()) {
                        return ResponseTemplate::new(412);
                    }
                    objects.remove(at);
                    ResponseTemplate::new(204)
                }
                _ => ResponseTemplate::new(405),
            }
        }
    }

    pub(crate) struct Fixture {
        pub(crate) server: MockServer,
        pub(crate) bucket: FakeBucket,
        pub(crate) storage: tempfile::TempDir,
        now: Arc<Mutex<SystemTime>>,
    }

    impl Fixture {
        pub(crate) async fn new() -> Self {
            let server = MockServer::start().await;
            let bucket = FakeBucket::default();
            Mock::given(wiremock::matchers::path_regex("^/storage/"))
                .respond_with(bucket.clone())
                .mount(&server)
                .await;
            Self {
                server,
                bucket,
                storage: tempfile::tempdir().unwrap(),
                now: Arc::new(Mutex::new(SystemTime::now())),
            }
        }

        pub(crate) fn settings() -> BucketSettings {
            BucketSettings {
                expected: FilePattern::parse(
                    "timetable_full.zip,CORPUSExtract.json.gz,NLC*.xml.gz",
                )
                .unwrap(),
                routing: Routing::with_corpus(),
                poll_interval: Duration::from_secs(300),
                delete_min_age: Duration::from_secs(3600),
                archive_keep: 2,
                max_object_bytes: 1000,
                max_downloads_per_poll: 2,
                max_bytes_per_hour: 2000,
                max_bytes_per_day: 3000,
                max_backoff: Duration::from_secs(3600),
            }
        }

        pub(crate) fn source(&self, settings: BucketSettings) -> BucketSource {
            let client = GcsClient::new(
                Some(&self.server.uri()),
                BUCKET,
                Credentials::Static(Secret::new("t")),
            )
            .unwrap();
            let now = Arc::clone(&self.now);
            BucketSource::with_clock(
                client,
                settings,
                BucketDirs::new(self.storage.path()),
                Arc::new(move || *now.lock().unwrap()),
            )
        }

        fn advance(&self, by: Duration) {
            *self.now.lock().unwrap() += by;
        }

        pub(crate) fn created(&self, ago: Duration) -> DateTime<Utc> {
            DateTime::<Utc>::from(*self.now.lock().unwrap() - ago)
        }

        pub(crate) fn incoming(&self) -> PathBuf {
            BucketDirs::new(self.storage.path()).incoming
        }

        async fn gets(&self) -> usize {
            self.server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .filter(|r| r.method == "GET" && r.url.query().unwrap_or("").contains("alt=media"))
                .count()
        }
    }

    #[tokio::test]
    async fn a_new_object_is_downloaded_verified_archived_and_handed_off_once() {
        let f = Fixture::new().await;
        let created = f.created(Duration::from_secs(60));
        f.bucket
            .put("timetable_full.zip", b"zip-bytes", 11, created);
        let mut source = f.source(Fixture::settings());
        assert_eq!(source.poll_once().await, Duration::from_secs(300));
        let landed = f.incoming().join("timetable_full.zip");
        assert_eq!(std::fs::read(&landed).unwrap(), b"zip-bytes");
        assert_eq!(
            std::fs::metadata(&landed).unwrap().modified().unwrap(),
            SystemTime::from(created)
        );
        // Re-polls never download it again: not now, not after a restart.
        source.poll_once().await;
        drop(source);
        let mut restarted = f.source(Fixture::settings());
        restarted.poll_once().await;
        assert_eq!(f.gets().await, 1);
        // Too young to delete; once old enough it is deleted.
        assert_eq!(f.bucket.names(), ["timetable_full.zip"]);
        f.advance(Duration::from_secs(3600));
        restarted.poll_once().await;
        assert!(f.bucket.names().is_empty());
        assert_eq!(f.gets().await, 1);
        let archive = BucketDirs::new(f.storage.path()).archive;
        assert_eq!(std::fs::read_dir(archive).unwrap().count(), 1);
    }

    #[tokio::test]
    async fn a_new_generation_is_a_new_delivery() {
        let f = Fixture::new().await;
        f.bucket.put(
            "timetable_full.zip",
            b"day-1",
            1,
            f.created(Duration::from_secs(60)),
        );
        let mut source = f.source(Fixture::settings());
        source.poll_once().await;
        f.advance(Duration::from_secs(600));
        f.bucket.put(
            "timetable_full.zip",
            b"day-2",
            2,
            f.created(Duration::from_secs(1)),
        );
        source.poll_once().await;
        assert_eq!(
            std::fs::read(f.incoming().join("timetable_full.zip")).unwrap(),
            b"day-2"
        );
        assert_eq!(f.gets().await, 2);
    }

    #[tokio::test]
    async fn unexpected_objects_are_never_fetched_and_deleted_once_old() {
        let f = Fixture::new().await;
        let old = f.created(Duration::from_secs(7200));
        f.bucket.put("probe.txt", b"hello", 3, old);
        f.bucket.put("CORPUSExtract.csv.gz", b"smart", 4, old);
        f.bucket.put("timetable_full.zip", &[0u8; 1001], 5, old);
        f.bucket.put(
            "NLC20261009.xml.gz",
            b"x",
            6,
            f.created(Duration::from_secs(1)),
        );
        let mut source = f.source(Fixture::settings());
        source.poll_once().await;
        // The young NLC is expected (and fetched); the three old
        // unexpected ones are deleted unread.
        assert_eq!(f.gets().await, 1);
        assert_eq!(f.bucket.names(), ["NLC20261009.xml.gz"]);
        assert_eq!(source.reported.len(), 3);
        assert!(!f.incoming().join("probe.txt").exists());
    }

    #[tokio::test]
    async fn a_crc_mismatch_keeps_the_object_and_hands_nothing_off() {
        let f = Fixture::new().await;
        f.bucket.put(
            "timetable_full.zip",
            b"good",
            1,
            f.created(Duration::from_secs(7200)),
        );
        f.bucket.0.lock().unwrap()[0].1 = b"evil".to_vec();
        let mut source = f.source(Fixture::settings());
        let delay = source.poll_once().await;
        assert_eq!(delay, Duration::from_secs(600), "backs off");
        assert!(!f.incoming().join("timetable_full.zip").exists());
        assert_eq!(f.bucket.names(), ["timetable_full.zip"], "not deleted");
        assert!(source.seen.is_empty());
    }

    /// A loop (an object that is always a new generation) downloads at
    /// most the caps, and the capped gauge is set.
    #[tokio::test]
    async fn the_download_caps_stop_a_loop() {
        let f = Fixture::new().await;
        let mut source = f.source(Fixture::settings());
        for generation in 1..=20 {
            f.bucket.put(
                "timetable_full.zip",
                &[b'x'; 900],
                generation,
                f.created(Duration::from_secs(1)),
            );
            source.poll_once().await;
            f.advance(Duration::from_secs(300));
        }
        // 900 bytes each: the hourly cap (2000) allows 2 an hour, the
        // daily (3000) 3 in all, over 100 minutes.
        assert_eq!(f.gets().await, 3);
        assert!(source.capped);
    }

    #[tokio::test]
    async fn at_most_the_per_poll_downloads_and_the_rest_next_poll() {
        let f = Fixture::new().await;
        let mut settings = Fixture::settings();
        settings.max_downloads_per_poll = 1;
        let created = f.created(Duration::from_secs(1));
        f.bucket.put("timetable_full.zip", b"a", 1, created);
        f.bucket.put("CORPUSExtract.json.gz", b"b", 2, created);
        let mut source = f.source(settings);
        source.poll_once().await;
        assert_eq!(f.gets().await, 1);
        assert!(!source.capped, "the per-poll cap is not the budget");
        source.poll_once().await;
        assert_eq!(f.gets().await, 2);
    }

    #[tokio::test]
    async fn a_412_keeps_the_newer_upload_for_the_next_poll() {
        let f = Fixture::new().await;
        let mut settings = Fixture::settings();
        settings.delete_min_age = Duration::ZERO;
        f.bucket.put(
            "timetable_full.zip",
            b"v1",
            1,
            f.created(Duration::from_secs(10)),
        );
        let mut source = f.source(settings);
        source.poll_once().await;
        // Generation 1 is confirmed (and deleted, at age 0). A publisher
        // upload of generation 2 between a listing and its delete makes
        // the conditional delete of generation 1 a 412, never a loss.
        f.bucket
            .put("timetable_full.zip", b"v2", 2, f.created(Duration::ZERO));
        assert_eq!(
            source.client.delete("timetable_full.zip", 1).await.unwrap(),
            Deleted::Newer
        );
        assert_eq!(f.bucket.names(), ["timetable_full.zip"]);
        source.poll_once().await;
        assert_eq!(
            std::fs::read(f.incoming().join("timetable_full.zip")).unwrap(),
            b"v2"
        );
        assert_eq!(f.gets().await, 2);
    }

    /// A 403 storm: one revoked state, the backoff ceiling, no tight loop;
    /// recovery clears it.
    #[tokio::test]
    async fn revoked_access_backs_off_at_the_ceiling_and_recovers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(format!("/storage/v1/b/{BUCKET}/o")))
            .respond_with(ResponseTemplate::new(403))
            .up_to_n_times(3)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/storage/v1/b/{BUCKET}/o")))
            .and(query_param("delimiter", "/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&server)
            .await;
        let storage = tempfile::tempdir().unwrap();
        let client = GcsClient::new(
            Some(&server.uri()),
            BUCKET,
            Credentials::Static(Secret::new("t")),
        )
        .unwrap();
        let mut source =
            BucketSource::new(client, Fixture::settings(), BucketDirs::new(storage.path()));
        for _ in 0..3 {
            assert_eq!(source.poll_once().await, Duration::from_secs(3600));
            assert!(source.revoked);
        }
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
        assert_eq!(source.poll_once().await, Duration::from_secs(300));
        assert!(!source.revoked);
    }

    /// With Pub/Sub, the idle wait stretches to the reconcile interval only
    /// while the subscription is healthy; a request during a poll is
    /// answered by the next poll, not that one.
    #[tokio::test]
    async fn a_wake_stretches_the_idle_poll_only_while_healthy() {
        let f = Fixture::new().await;
        let wake = Wake::new(Duration::ZERO);
        let source = f
            .source(Fixture::settings())
            .with_wake(Arc::clone(&wake), Duration::from_secs(3600));
        let poll = Duration::from_secs(300);
        assert_eq!(
            source.idle_delay(poll),
            poll,
            "unhealthy: the poll interval"
        );
        wake.set_healthy(true);
        assert_eq!(source.idle_delay(poll), Duration::from_secs(3600));
        wake.set_healthy(false);
        assert_eq!(source.idle_delay(poll), poll, "falls back");

        wake.started();
        let ticket = wake.request();
        wake.finished();
        let waited = tokio::time::timeout(Duration::from_millis(50), wake.wait(ticket)).await;
        assert!(waited.is_err(), "the poll in progress does not answer it");
        wake.started();
        wake.finished();
        tokio::time::timeout(Duration::from_secs(1), wake.wait(ticket))
            .await
            .unwrap();
    }

    #[test]
    fn backoff_doubles_to_the_ceiling() {
        let poll = Duration::from_secs(300);
        let max = Duration::from_secs(3600);
        assert_eq!(backoff(poll, max, 1), Duration::from_secs(600));
        assert_eq!(backoff(poll, max, 3), Duration::from_secs(2400));
        assert_eq!(backoff(poll, max, 4), max);
        assert_eq!(backoff(poll, max, 400), max);
    }

    #[test]
    fn unsafe_names_are_never_written_and_logged_names_are_escaped() {
        for bad in ["", ".seen", "a/b", "a\\b", "a\nb", &"x".repeat(256)] {
            assert!(!is_safe_name(bad), "{bad:?}");
        }
        assert!(is_safe_name("NLC20261009.xml.gz"));
        assert_eq!(shown("a\nb"), "a\\nb");
        assert!(shown(&"é".repeat(300)).len() <= MAX_LOGGED_NAME + 3);
    }

    #[test]
    fn the_metric_names_are_the_alerts_contract() {
        assert_eq!(
            [
                ACCESS_REVOKED_METRIC,
                LAST_NEW_OBJECT_METRIC,
                ERRORS_METRIC,
                UNEXPECTED_METRIC,
                DOWNLOAD_CAPPED_METRIC,
                DOWNLOADED_BYTES_METRIC,
                POLL_DURATION_METRIC,
                crate::sources::DISAGREEMENT_METRIC,
            ],
            [
                "schedule_feed_source_access_revoked",
                "schedule_feed_source_last_new_object_seconds",
                "schedule_feed_source_errors_total",
                "schedule_feed_source_unexpected_objects_total",
                "schedule_feed_source_download_capped",
                "schedule_feed_source_downloaded_bytes_total",
                "schedule_feed_source_poll_duration_seconds",
                "schedule_feed_source_disagreement_total",
            ]
        );
    }
}
