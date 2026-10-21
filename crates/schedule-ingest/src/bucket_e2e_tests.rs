//! The bucket source end to end against a real GCS JSON API emulator,
//! [fake-gcs-server](https://github.com/fsouza/fake-gcs-server), instead of
//! the hand-written fakes of `bucket.rs` and `sources_pipeline_tests.rs`:
//! the listing (`delimiter=/`, paging), generation numbers, CRC32C and MD5
//! and deletes all come from an independent implementation.
//!
//! Every test is `#[ignore]`d and needs `FAKE_GCS_URL` (an http
//! fake-gcs-server, which ignores the bearer token):
//!
//! ```sh
//! docker run -d --rm --name fake-gcs -p 127.0.0.1:4443:4443 \
//!   fsouza/fake-gcs-server:1.55 -scheme http -port 4443 -backend memory
//! FAKE_GCS_URL=http://127.0.0.1:4443 \
//!   cargo test -p schedule-ingest bucket_e2e_tests -- --ignored
//! ```
//!
//! Each test makes its own bucket, so they run in parallel. The reader signs
//! in with a throwaway service-account key (key mode, a self-signed JWT):
//! no Google credentials anywhere.
//!
//! The reader talks to the emulator through [`Proxy`], an in-process HTTP
//! proxy that forwards everything and can, per test:
//!
//! - page listings (`maxResults`), so `nextPageToken` is the emulator's;
//! - tamper with what the reader sees (a listed size, CRC32C or MD5, or a
//!   flipped byte in a download), for the checks the emulator never fails
//!   (it computes both hashes itself and refuses a mismatching upload);
//! - act as the publisher mid-poll (a new upload just before or after a
//!   download);
//! - refuse every call (403, the kill switch);
//! - enforce `ifGenerationMatch` on a delete, which fake-gcs-server 1.55
//!   ignores (it deletes whatever generation is live). GCS answers 412; the
//!   proxy does the same from the live object's metadata.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use crate::bucket::{BucketDirs, BucketSettings, BucketSource};
use crate::delivery::delivery_dir_name;
use crate::gcs::{Credentials, GcsClient};
use crate::pattern::{FilePattern, Routing};
use crate::sink::tests::{Answer, FakeSink};
use crate::sources::Precedence;
use crate::sources_pipeline_tests::{Setup, ago, delivery_dirs, put, sha};
use crate::tests::real_zip;

/// The emulator's base URL, or a panic naming the variable.
fn fake_gcs_url() -> String {
    std::env::var("FAKE_GCS_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .expect("FAKE_GCS_URL must point at an http fake-gcs-server; see the module doc")
        .trim_end_matches('/')
        .to_string()
}

/// What the proxy changes, per test.
#[derive(Default)]
struct Tamper {
    /// `maxResults` on every listing.
    page_size: Option<u32>,
    /// Per object name, fields of its listed resource replaced (`null`
    /// removes the field).
    listing: HashMap<String, serde_json::Map<String, serde_json::Value>>,
    /// Objects whose downloads have their first byte flipped.
    flip_body: HashSet<String>,
    /// The publisher uploads this under the name just before the reader's
    /// download is served (so the listed generation is gone).
    upload_before_get: HashMap<String, Vec<u8>>,
    /// The publisher uploads this under the name just after the reader's
    /// download (so the delete that follows is of an older generation).
    upload_after_get: HashMap<String, Vec<u8>>,
    /// Answer every call with this status.
    refuse: Option<StatusCode>,
}

/// One call the reader made: method, object name (empty for a listing) and
/// whether it was a media download.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Call {
    method: Method,
    object: String,
    media: bool,
}

/// The in-process proxy in front of the emulator (see the module doc).
struct Proxy {
    upstream: String,
    bucket: String,
    http: reqwest::Client,
    tamper: Mutex<Tamper>,
    calls: Mutex<Vec<Call>>,
}

impl Proxy {
    fn tamper(&self) -> std::sync::MutexGuard<'_, Tamper> {
        self.tamper.lock().unwrap()
    }

    /// Media downloads of `name` so far.
    fn gets(&self, name: &str) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.method == Method::GET && c.media && c.object == name)
            .count()
    }

    fn all_gets(&self) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.media)
            .count()
    }

    fn calls(&self) -> usize {
        self.calls.lock().unwrap().len()
    }

    /// Uploads `contents` as `name` (the publisher); returns its generation.
    async fn upload(&self, name: &str, contents: &[u8]) -> i64 {
        upload(&self.http, &self.upstream, &self.bucket, name, contents).await
    }
}

async fn upload(
    http: &reqwest::Client,
    upstream: &str,
    bucket: &str,
    name: &str,
    contents: &[u8],
) -> i64 {
    let response = http
        .post(format!("{upstream}/upload/storage/v1/b/{bucket}/o"))
        .query(&[("uploadType", "media"), ("name", name)])
        .body(contents.to_vec())
        .send()
        .await
        .unwrap();
    assert!(
        response.status().is_success(),
        "upload {name}: {response:?}"
    );
    let object: serde_json::Value = response.json().await.unwrap();
    object["generation"].as_str().unwrap().parse().unwrap()
}

fn query<'a>(uri: &'a Uri, key: &str) -> Option<&'a str> {
    uri.query()?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

fn status(code: StatusCode) -> Response {
    code.into_response()
}

async fn handle(
    State(proxy): State<Arc<Proxy>>,
    method: Method,
    uri: Uri,
    body: Bytes,
) -> Response {
    let list_path = format!("/storage/v1/b/{}/o", proxy.bucket);
    let object = uri
        .path()
        .strip_prefix(&format!("{list_path}/"))
        .unwrap_or_default()
        .to_string();
    let media = query(&uri, "alt") == Some("media");
    proxy.calls.lock().unwrap().push(Call {
        method: method.clone(),
        object: object.clone(),
        media,
    });
    if let Some(code) = proxy.tamper().refuse {
        return status(code);
    }

    if method == Method::DELETE
        && let Some(wanted) = query(&uri, "ifGenerationMatch")
    {
        // GCS's precondition, which the emulator ignores.
        let live = proxy
            .http
            .get(format!("{}{}", proxy.upstream, uri.path()))
            .send()
            .await
            .unwrap();
        if live.status() == StatusCode::NOT_FOUND {
            return status(StatusCode::NOT_FOUND);
        }
        let live: serde_json::Value = live.json().await.unwrap();
        if live["generation"].as_str() != Some(wanted) {
            return status(StatusCode::PRECONDITION_FAILED);
        }
    }
    let before = if media {
        proxy.tamper().upload_before_get.remove(&object)
    } else {
        None
    };
    if let Some(contents) = before {
        proxy.upload(&object, &contents).await;
    }

    let mut url = format!("{}{}", proxy.upstream, uri.path());
    let mut query_string = uri.query().unwrap_or_default().to_string();
    let page_size = proxy.tamper().page_size;
    if uri.path() == list_path
        && let Some(size) = page_size
    {
        query_string = format!("{query_string}&maxResults={size}");
    }
    if !query_string.is_empty() {
        url = format!("{url}?{query_string}");
    }
    let upstream = proxy
        .http
        .request(method.clone(), url)
        .body(body)
        .send()
        .await
        .unwrap();
    let code = upstream.status();
    let mut bytes = upstream.bytes().await.unwrap().to_vec();

    if uri.path() == list_path && code.is_success() {
        let mut page: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let tamper = proxy.tamper();
        for item in page["items"].as_array_mut().into_iter().flatten() {
            let name = item["name"].as_str().unwrap_or_default().to_string();
            for (field, value) in tamper.listing.get(&name).into_iter().flatten() {
                let item = item.as_object_mut().unwrap();
                if value.is_null() {
                    item.remove(field);
                } else {
                    item.insert(field.clone(), value.clone());
                }
            }
        }
        bytes = serde_json::to_vec(&page).unwrap();
    }
    if media && code.is_success() {
        if proxy.tamper().flip_body.contains(&object)
            && let Some(first) = bytes.first_mut()
        {
            *first ^= 0xff;
        }
        let after = proxy.tamper().upload_after_get.remove(&object);
        if let Some(contents) = after {
            proxy.upload(&object, &contents).await;
        }
    }
    (code, bytes).into_response()
}

/// One test's bucket, proxy, reader state and clock.
struct Fixture {
    proxy: Arc<Proxy>,
    proxy_url: String,
    key: tempfile::TempDir,
    storage: PathBuf,
    now: Arc<Mutex<SystemTime>>,
}

impl Fixture {
    /// A fresh bucket (named after `tag`) and a proxy in front of it; the
    /// reader's state under `storage`.
    async fn new(tag: &str, storage: &Path) -> Self {
        let upstream = fake_gcs_url();
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let bucket = format!("ds-e2e-{tag}-{nanos}");
        let http = reqwest::Client::new();
        let created = http
            .post(format!("{upstream}/storage/v1/b"))
            .json(&serde_json::json!({ "name": bucket }))
            .send()
            .await
            .expect("FAKE_GCS_URL is not answering");
        assert!(created.status().is_success(), "create bucket: {created:?}");

        let proxy = Arc::new(Proxy {
            upstream,
            bucket,
            http,
            tamper: Mutex::default(),
            calls: Mutex::default(),
        });
        let app = axum::Router::new()
            .fallback(handle)
            .with_state(Arc::clone(&proxy));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let key = tempfile::tempdir().unwrap();
        std::fs::write(
            key.path().join("service-account.json"),
            crate::gcs::tests::throwaway_key_json().0,
        )
        .unwrap();
        Self {
            proxy,
            proxy_url,
            key,
            storage: storage.to_path_buf(),
            now: Arc::new(Mutex::new(SystemTime::now())),
        }
    }

    fn settings() -> BucketSettings {
        BucketSettings {
            expected: FilePattern::parse("timetable_full.zip,CORPUSExtract.json.gz,NLC*.xml.gz")
                .unwrap(),
            routing: Routing::with_corpus(),
            poll_interval: Duration::from_secs(300),
            delete_min_age: Duration::from_secs(3600),
            archive_keep: 5,
            max_object_bytes: 1 << 20,
            max_downloads_per_poll: 4,
            max_bytes_per_hour: 1 << 22,
            max_bytes_per_day: 1 << 23,
            max_backoff: Duration::from_secs(3600),
        }
    }

    /// A reader as `run` builds one (key mode), with this fixture's clock.
    /// Calling it again is a restart: the same disk.
    fn source(&self, settings: BucketSettings) -> BucketSource {
        let client = GcsClient::new(
            Some(&self.proxy_url),
            &self.proxy.bucket,
            Credentials::key(self.key.path().join("service-account.json")),
        )
        .unwrap();
        let now = Arc::clone(&self.now);
        BucketSource::with_clock(
            client,
            settings,
            BucketDirs::new(&self.storage),
            Arc::new(move || *now.lock().unwrap()),
        )
    }

    fn advance(&self, by: Duration) {
        *self.now.lock().unwrap() += by;
    }

    fn bucket_dir(&self) -> PathBuf {
        self.storage.join("sources").join("bucket")
    }

    fn incoming(&self, name: &str) -> PathBuf {
        BucketDirs::new(&self.storage).incoming.join(name)
    }

    fn archived(&self) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(self.bucket_dir().join("archive"))
            .map(|dir| {
                dir.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    /// The seen ledger's `(name, generation)` pairs.
    fn seen(&self) -> Vec<(String, i64)> {
        std::fs::read_to_string(self.bucket_dir().join(".seen"))
            .unwrap_or_default()
            .lines()
            .map(|line| {
                let entry: serde_json::Value = serde_json::from_str(line).unwrap();
                (
                    entry["name"].as_str().unwrap().to_string(),
                    entry["generation"].as_i64().unwrap(),
                )
            })
            .collect()
    }

    /// The bucket's objects, every level, straight from the emulator.
    async fn objects(&self) -> Vec<(String, i64)> {
        let page: serde_json::Value = self
            .proxy
            .http
            .get(format!(
                "{}/storage/v1/b/{}/o",
                self.proxy.upstream, self.proxy.bucket
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let mut objects: Vec<(String, i64)> = page["items"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|o| {
                (
                    o["name"].as_str().unwrap().to_string(),
                    o["generation"].as_str().unwrap().parse().unwrap(),
                )
            })
            .collect();
        objects.sort();
        objects
    }

    async fn object_created(&self, name: &str) -> SystemTime {
        let object: serde_json::Value = self
            .proxy
            .http
            .get(format!(
                "{}/storage/v1/b/{}/o/{name}",
                self.proxy.upstream, self.proxy.bucket
            ))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let created =
            chrono::DateTime::parse_from_rfc3339(object["timeCreated"].as_str().unwrap()).unwrap();
        SystemTime::from(created)
    }
}

/// One way to tamper, for a table of cases.
type TamperWith = Box<dyn Fn(&mut Tamper)>;

fn names(objects: &[(String, i64)]) -> Vec<&str> {
    objects.iter().map(|(name, _)| name.as_str()).collect()
}

/// Listing (paged, root only), download, hand-off, archive, the seen
/// ledger, unexpected objects, and the delete once `deleteMinAge` old.
#[tokio::test]
#[ignore = "requires fake-gcs-server; run with FAKE_GCS_URL=... cargo test -p schedule-ingest bucket_e2e_tests -- --ignored"]
async fn objects_are_listed_fetched_archived_handed_off_and_deleted_once_old() {
    let storage = tempfile::tempdir().unwrap();
    let f = Fixture::new("life", storage.path()).await;
    f.proxy.tamper().page_size = Some(2);
    let cif = real_zip();
    let corpus = b"{\"TIPLOCDATA\":[]}".to_vec();
    let cif_generation = f.proxy.upload("timetable_full.zip", &cif).await;
    let corpus_generation = f.proxy.upload("CORPUSExtract.json.gz", &corpus).await;
    f.proxy.upload("probe.txt", b"hello").await;
    f.proxy.upload("CORPUSExtract.csv.gz", b"unroutable").await;
    f.proxy.upload("nested/timetable_full.zip", b"nested").await;

    let mut source = f.source(Fixture::settings());
    assert_eq!(source.poll_once().await, Duration::from_secs(300));

    // The two expected objects, byte for byte, with the creation time as
    // the mtime; nothing else is downloaded or landed.
    assert_eq!(f.proxy.all_gets(), 2);
    assert_eq!(
        std::fs::read(f.incoming("timetable_full.zip")).unwrap(),
        cif
    );
    assert_eq!(
        std::fs::read(f.incoming("CORPUSExtract.json.gz")).unwrap(),
        corpus
    );
    let created = f.object_created("timetable_full.zip").await;
    let landed = std::fs::metadata(f.incoming("timetable_full.zip"))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        delivery_dir_name(landed),
        delivery_dir_name(created),
        "mtime is the object's creation time"
    );
    assert!(!f.incoming("probe.txt").exists());
    let mut seen = f.seen();
    seen.sort();
    assert_eq!(
        seen,
        [
            ("CORPUSExtract.json.gz".to_string(), corpus_generation),
            ("timetable_full.zip".to_string(), cif_generation),
        ]
    );
    let archived = f.archived();
    assert_eq!(archived.len(), 2, "{archived:?}");
    assert!(
        archived.contains(&format!(
            "{}-{cif_generation}-timetable_full.zip",
            delivery_dir_name(created)
        )),
        "{archived:?}"
    );
    // Paged: more than one listing call for five objects (two a page).
    let listings = f
        .proxy
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.method == Method::GET && c.object.is_empty())
        .count();
    assert!(listings >= 2, "{listings} listing calls");

    // Too young: nothing is deleted, and nothing is downloaded again, not
    // on a re-poll and not after a restart.
    source.poll_once().await;
    drop(source);
    let mut source = f.source(Fixture::settings());
    source.poll_once().await;
    assert_eq!(f.proxy.all_gets(), 2);
    assert_eq!(f.objects().await.len(), 5);

    // Once old enough: the handled and the unexpected objects at the root
    // are deleted (the unexpected ones unread); the nested one is never
    // seen, so never touched.
    f.advance(Duration::from_secs(3601));
    assert_eq!(source.poll_once().await, Duration::from_secs(300));
    assert_eq!(names(&f.objects().await), ["nested/timetable_full.zip"]);
    assert_eq!(f.proxy.all_gets(), 2);
    assert_eq!(f.proxy.gets("probe.txt"), 0);
}

/// Each download is of exactly the listed generation: a re-upload between
/// the listing and the download is a 404, nothing lands, and the next poll
/// takes the new generation as a new delivery.
#[tokio::test]
#[ignore = "requires fake-gcs-server; run with FAKE_GCS_URL=... cargo test -p schedule-ingest bucket_e2e_tests -- --ignored"]
async fn downloads_are_pinned_to_the_listed_generation() {
    let storage = tempfile::tempdir().unwrap();
    let f = Fixture::new("pin", storage.path()).await;
    let first = f.proxy.upload("timetable_full.zip", b"day-1").await;
    f.proxy
        .tamper()
        .upload_before_get
        .insert("timetable_full.zip".into(), b"day-2".to_vec());

    let mut source = f.source(Fixture::settings());
    assert_eq!(
        source.poll_once().await,
        Duration::from_secs(600),
        "a failed get backs off"
    );
    assert!(!f.incoming("timetable_full.zip").exists());
    assert!(f.seen().is_empty());
    let objects = f.objects().await;
    assert_eq!(names(&objects), ["timetable_full.zip"]);
    let second = objects[0].1;
    assert_ne!(first, second);

    assert_eq!(source.poll_once().await, Duration::from_secs(300));
    assert_eq!(
        std::fs::read(f.incoming("timetable_full.zip")).unwrap(),
        b"day-2"
    );
    assert_eq!(f.seen(), [("timetable_full.zip".to_string(), second)]);
    assert_eq!(f.proxy.gets("timetable_full.zip"), 2);

    // A later upload is a new generation, so a new delivery.
    let third = f.proxy.upload("timetable_full.zip", b"day-3").await;
    source.poll_once().await;
    assert_eq!(
        std::fs::read(f.incoming("timetable_full.zip")).unwrap(),
        b"day-3"
    );
    assert_eq!(f.seen().last().unwrap().1, third);
}

/// Every check against the object's metadata: a mismatch hands nothing off,
/// keeps the object in the bucket (even once old) and records nothing; the
/// next good read takes it.
#[tokio::test]
#[ignore = "requires fake-gcs-server; run with FAKE_GCS_URL=... cargo test -p schedule-ingest bucket_e2e_tests -- --ignored"]
async fn size_crc32c_and_md5_mismatches_are_refused_and_retried() {
    let flipped = |t: &mut Tamper| {
        t.flip_body.insert("timetable_full.zip".into());
    };
    let field = |field: &'static str, value: serde_json::Value| {
        move |t: &mut Tamper| {
            t.listing
                .entry("timetable_full.zip".into())
                .or_default()
                .insert(field.into(), value.clone());
        }
    };
    let cases: Vec<(&str, TamperWith)> = vec![
        ("a corrupted body (CRC32C and MD5)", Box::new(flipped)),
        (
            "a wrong listed CRC32C",
            Box::new(field("crc32c", "AAAAAA==".into())),
        ),
        (
            "no listed CRC32C",
            Box::new(field("crc32c", serde_json::Value::Null)),
        ),
        (
            "a wrong listed MD5 (CRC32C right)",
            Box::new(field("md5Hash", "AAAAAAAAAAAAAAAAAAAAAA==".into())),
        ),
        (
            "fewer bytes than listed",
            Box::new(field("size", "6".into())),
        ),
        (
            "more bytes than listed",
            Box::new(field("size", "4".into())),
        ),
    ];
    for (case, tamper) in cases {
        let storage = tempfile::tempdir().unwrap();
        let f = Fixture::new("verify", storage.path()).await;
        let generation = f.proxy.upload("timetable_full.zip", b"hello").await;
        tamper(&mut f.proxy.tamper());
        let mut source = f.source(Fixture::settings());
        assert_eq!(
            source.poll_once().await,
            Duration::from_secs(600),
            "{case}: backs off"
        );
        f.advance(Duration::from_secs(7200));
        source.poll_once().await;
        assert!(!f.incoming("timetable_full.zip").exists(), "{case}");
        assert!(f.seen().is_empty(), "{case}");
        assert!(f.archived().is_empty(), "{case}");
        assert_eq!(
            f.objects().await,
            [("timetable_full.zip".to_string(), generation)],
            "{case}: kept in the bucket"
        );
        assert!(
            !std::fs::read_dir(f.bucket_dir()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".partial-")),
            "{case}: no partial download left"
        );

        *f.proxy.tamper() = Tamper::default();
        source.poll_once().await;
        assert_eq!(
            std::fs::read(f.incoming("timetable_full.zip")).unwrap(),
            b"hello",
            "{case}"
        );
    }
}

/// The conditional delete: a new upload after the reader's download (the
/// publisher re-delivering) makes the delete of the handled generation a
/// 412. The new one stays and is the next poll's delivery.
#[tokio::test]
#[ignore = "requires fake-gcs-server; run with FAKE_GCS_URL=... cargo test -p schedule-ingest bucket_e2e_tests -- --ignored"]
async fn a_newer_upload_survives_the_delete_of_the_handled_one() {
    let storage = tempfile::tempdir().unwrap();
    let f = Fixture::new("cond", storage.path()).await;
    let mut settings = Fixture::settings();
    settings.delete_min_age = Duration::ZERO;
    let first = f.proxy.upload("timetable_full.zip", b"v1").await;
    f.proxy
        .tamper()
        .upload_after_get
        .insert("timetable_full.zip".into(), b"v2".to_vec());
    let mut source = f.source(settings);
    assert_eq!(source.poll_once().await, Duration::from_secs(300));
    assert_eq!(
        std::fs::read(f.incoming("timetable_full.zip")).unwrap(),
        b"v1"
    );
    let objects = f.objects().await;
    assert_eq!(names(&objects), ["timetable_full.zip"]);
    assert_ne!(objects[0].1, first, "the newer upload is kept");

    source.poll_once().await;
    assert_eq!(
        std::fs::read(f.incoming("timetable_full.zip")).unwrap(),
        b"v2"
    );
    assert!(f.objects().await.is_empty(), "and deleted once handled");
}

/// The loop guards: the per-poll cap, the rolling byte budgets against an
/// object that is always a new generation, and revoked access backing off
/// at the ceiling without a tight loop.
#[tokio::test]
#[ignore = "requires fake-gcs-server; run with FAKE_GCS_URL=... cargo test -p schedule-ingest bucket_e2e_tests -- --ignored"]
async fn the_loop_guards_hold_against_a_real_bucket() {
    // At most maxDownloadsPerPoll a poll; the rest on the next.
    let storage = tempfile::tempdir().unwrap();
    let f = Fixture::new("perpoll", storage.path()).await;
    let mut settings = Fixture::settings();
    settings.max_downloads_per_poll = 1;
    f.proxy.upload("timetable_full.zip", b"a").await;
    f.proxy.upload("CORPUSExtract.json.gz", b"b").await;
    let mut source = f.source(settings);
    source.poll_once().await;
    assert_eq!(f.proxy.all_gets(), 1);
    source.poll_once().await;
    assert_eq!(f.proxy.all_gets(), 2);
    source.poll_once().await;
    assert_eq!(f.proxy.all_gets(), 2);

    // A loop: a new generation every poll. 900 bytes each against 2000 an
    // hour and 3000 a day: two in the first hour, a third in the second,
    // then nothing for the rest of the day.
    let storage = tempfile::tempdir().unwrap();
    let f = Fixture::new("budget", storage.path()).await;
    let mut settings = Fixture::settings();
    settings.max_bytes_per_hour = 2000;
    settings.max_bytes_per_day = 3000;
    let mut source = f.source(settings);
    for poll in 0..24 {
        f.proxy.upload("timetable_full.zip", &[b'x'; 900]).await;
        source.poll_once().await;
        f.advance(Duration::from_secs(600));
        let expected = match poll {
            0 => 1,
            1..=5 => 2,
            _ => 3,
        };
        assert_eq!(f.proxy.all_gets(), expected, "after poll {poll}");
    }

    // The kill switch: every call refused. One call a poll, at the backoff
    // ceiling; access comes back on the next good listing.
    let storage = tempfile::tempdir().unwrap();
    let f = Fixture::new("revoked", storage.path()).await;
    f.proxy.upload("timetable_full.zip", b"a").await;
    f.proxy.tamper().refuse = Some(StatusCode::FORBIDDEN);
    let mut source = f.source(Fixture::settings());
    for _ in 0..3 {
        assert_eq!(source.poll_once().await, Duration::from_secs(3600));
    }
    assert_eq!(f.proxy.calls(), 3, "one refused listing a poll");
    f.proxy.tamper().refuse = None;
    assert_eq!(source.poll_once().await, Duration::from_secs(300));
    assert_eq!(f.proxy.all_gets(), 1);
}

/// The same delivery through SFTP and the bucket, the bucket's copy read
/// from the emulator: ingested once, one `accepted` and one `duplicate`,
/// whichever source ranks first; nothing more on later cycles.
#[tokio::test]
#[ignore = "requires fake-gcs-server; run with FAKE_GCS_URL=... cargo test -p schedule-ingest bucket_e2e_tests -- --ignored"]
async fn a_delivery_through_both_sources_is_ingested_once() {
    for precedence in [Precedence::default(), "sftp,bucket".parse().unwrap()] {
        let setup = Setup::new();
        let f = Fixture::new("dedup", setup.storage.path()).await;
        let bytes = real_zip();
        put(setup.watch.path(), "timetable_full.zip", &bytes, ago(3));
        f.proxy.upload("timetable_full.zip", &bytes).await;
        let mut source = f.source(Fixture::settings());
        source.poll_once().await;
        assert_eq!(
            std::fs::read(setup.incoming().join("timetable_full.zip")).unwrap(),
            bytes
        );

        let multi = setup.multi(precedence);
        let sink = FakeSink::new(Answer::Accept);
        let mut lanes = setup.lanes(precedence);
        let audit = setup
            .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 4)
            .await;
        assert_eq!(sink.feed_records().len(), 1, "{precedence:?}: {audit:?}");
        assert_eq!(delivery_dirs(setup.storage.path()).len(), 1);
        let outcomes: Vec<(&str, &str)> = audit
            .iter()
            .map(|l| {
                (
                    l["outcome"].as_str().unwrap(),
                    l["source"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(outcomes.len(), 2, "{precedence:?}: {audit:?}");
        assert_eq!(outcomes[0].0, "accepted");
        assert_eq!(outcomes[1].0, "duplicate");
        assert_ne!(outcomes[0].1, outcomes[1].1);
        assert_eq!(audit[1]["duplicate_of"], sha(&bytes).as_str());

        // The bucket copy's delete is not gated on the ingest, and a later
        // poll and later cycles add nothing.
        f.advance(Duration::from_secs(3601));
        source.poll_once().await;
        assert!(f.objects().await.is_empty());
        let audit = setup
            .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 2)
            .await;
        assert!(audit.is_empty(), "{audit:?}");
        assert_eq!(sink.feed_records().len(), 1);
        assert_eq!(f.proxy.all_gets(), 1);
    }
}
