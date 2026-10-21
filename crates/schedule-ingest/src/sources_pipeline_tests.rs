//! The bucket source handing off into the existing pipeline, and the two
//! sources together (dedup, disagreement, precedence), end to end through
//! `run_lanes_once` with the real-shaped fixtures and a fake sink.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use common::secret::Secret;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::bucket::{BucketDirs, BucketSettings, BucketSource};
use crate::config::Config;
use crate::gcs::{Credentials, GcsClient};
use crate::pattern::{FilePattern, Routing};
use crate::sink::tests::{Answer, FakeSink};
use crate::sources::{MultiSource, Precedence, SourceId};
use crate::tests::{real_zip, test_config};
use crate::{LaneState, audit, cif_check, corpus, delivery, run_lanes_once};

const BUCKET: &str = "example-ds-ingest";

/// The real-shaped CIF delivery with one extra entry: different content
/// that still passes every check.
fn other_zip() -> Vec<u8> {
    let generated = Utc::now().format("%d/%m/%Y").to_string();
    let files = cif_check::tests::delivery_files(Some(&generated));
    let mut entries: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(name, text)| (*name, text.as_bytes()))
        .collect();
    entries.push(("README.txt", b"a different delivery"));
    delivery::build_test_zip(&entries)
}

pub(crate) fn sha(bytes: &[u8]) -> String {
    audit::DeliveredFile::from_bytes("", bytes).sha256
}

pub(crate) fn put(dir: &Path, name: &str, bytes: &[u8], mtime: SystemTime) {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
}

pub(crate) fn ago(minutes: u64) -> SystemTime {
    // Whole seconds, as a delivery directory name keeps.
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    SystemTime::UNIX_EPOCH + Duration::from_secs(now - minutes * 60)
}

pub(crate) fn delivery_dirs(storage: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(storage)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| delivery::is_delivery_dir_name(name))
        .collect();
    names.sort();
    names
}

pub(crate) struct Setup {
    pub(crate) watch: tempfile::TempDir,
    pub(crate) storage: tempfile::TempDir,
    pub(crate) config: Config,
}

impl Setup {
    pub(crate) fn new() -> Self {
        let watch = tempfile::tempdir().unwrap();
        let storage = tempfile::tempdir().unwrap();
        let config = test_config(watch.path(), storage.path());
        Self {
            watch,
            storage,
            config,
        }
    }

    pub(crate) fn incoming(&self) -> PathBuf {
        BucketDirs::new(self.storage.path()).incoming
    }

    pub(crate) fn multi(&self, precedence: Precedence) -> MultiSource {
        MultiSource::new(self.storage.path(), precedence, 120)
    }

    /// Both lanes, in `precedence` order (SFTP with the config's stability
    /// gate, the bucket with none), as `run` builds them.
    pub(crate) fn lanes(&self, precedence: Precedence) -> Vec<LaneState> {
        let mut lanes = vec![
            LaneState::new(
                SourceId::Sftp,
                self.watch.path().to_path_buf(),
                self.config.stability_cycles,
            ),
            LaneState::new(SourceId::Bucket, self.incoming(), 1),
        ];
        lanes.sort_by_key(|lane| precedence.rank(lane.source));
        lanes
    }

    pub(crate) async fn cycles(
        &self,
        sink: &FakeSink,
        routing: &Routing,
        multi: &MultiSource,
        lanes: &mut [LaneState],
        n: usize,
    ) -> Vec<serde_json::Value> {
        let (guard, logs) = audit::tests::capture_default();
        for _ in 0..n {
            run_lanes_once(sink, &self.config, routing, Some(multi), lanes, false).await;
        }
        drop(guard);
        logs.audit_lines()
    }
}

/// The real hand-off: a fake GCS bucket holds the fixture delivery; one
/// poll downloads and lands it; the bucket lane extracts, checks and
/// records it, with the object's creation time as `delivered_at`.
#[tokio::test]
async fn a_bucket_object_goes_through_the_existing_cif_pipeline() {
    let setup = Setup::new();
    let bytes = real_zip();
    let created: DateTime<Utc> = DateTime::<Utc>::from(ago(10));
    let mut object = crate::gcs::tests::object_json("timetable_full.zip", &bytes, 42);
    object["timeCreated"] = created.to_rfc3339().into();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(format!("/storage/v1/b/{BUCKET}/o")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "items": [object] })),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/storage/v1/b/{BUCKET}/o/timetable_full.zip")))
        .and(query_param("generation", "42"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()))
        .expect(1)
        .mount(&server)
        .await;
    let client = GcsClient::new(
        Some(&server.uri()),
        BUCKET,
        Credentials::Static(Secret::new("t")),
    )
    .unwrap();
    let settings = BucketSettings {
        expected: FilePattern::parse("timetable_full.zip,CORPUSExtract.json.gz").unwrap(),
        routing: Routing::defaults(),
        poll_interval: Duration::from_secs(300),
        delete_min_age: Duration::from_secs(3600),
        archive_keep: 5,
        max_object_bytes: 1 << 20,
        max_downloads_per_poll: 2,
        max_bytes_per_hour: 1 << 21,
        max_bytes_per_day: 1 << 22,
        max_backoff: Duration::from_secs(3600),
    };
    let mut source = BucketSource::new(client, settings, BucketDirs::new(setup.storage.path()));
    source.poll_once().await;
    source.poll_once().await;

    let multi = setup.multi(Precedence::default());
    let mut lanes = vec![LaneState::new(SourceId::Bucket, setup.incoming(), 1)];
    let sink = FakeSink::new(Answer::Accept);
    let audit = setup
        .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 3)
        .await;

    assert_eq!(audit.len(), 1, "{audit:?}");
    assert_eq!(audit[0]["outcome"], "accepted");
    assert_eq!(audit[0]["source"], "bucket");
    assert_eq!(audit[0]["sha256"], sha(&bytes).as_str());
    assert_eq!(
        delivery_dirs(setup.storage.path()),
        [delivery::delivery_dir_name(SystemTime::from(created))]
    );
    let records = sink.feed_records();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["source_sha256"], sha(&bytes).as_str());
    assert_eq!(records[0]["delivery_source"], "bucket");
}

/// The same delivery through both sources, in either precedence order:
/// one extraction, one record, one `accepted` and one `duplicate` line;
/// nothing more on later cycles or after a restart.
#[tokio::test]
async fn the_same_delivery_through_both_sources_is_ingested_once() {
    for precedence in [Precedence::default(), "sftp,bucket".parse().unwrap()] {
        let setup = Setup::new();
        let bytes = real_zip();
        put(setup.watch.path(), "timetable_full.zip", &bytes, ago(3));
        put(&setup.incoming(), "timetable_full.zip", &bytes, ago(1));
        let multi = setup.multi(precedence);
        let sink = FakeSink::new(Answer::Accept);
        let mut lanes = setup.lanes(precedence);
        let audit = setup
            .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 4)
            .await;

        assert_eq!(sink.feed_records().len(), 1, "{precedence:?}");
        assert_eq!(delivery_dirs(setup.storage.path()).len(), 1);
        let outcomes: Vec<&str> = audit
            .iter()
            .map(|l| l["outcome"].as_str().unwrap())
            .collect();
        assert_eq!(outcomes, ["accepted", "duplicate"], "{precedence:?}");
        assert_eq!(audit[1]["duplicate_of"], sha(&bytes).as_str());
        assert_eq!(audit[1]["first_source"], audit[0]["source"]);
        assert_ne!(audit[0]["source"], audit[1]["source"]);
        // The record names the lane of the accepted copy: SFTP's when it
        // ranks first, the bucket's otherwise.
        assert_eq!(
            sink.feed_records()[0]["delivery_source"],
            audit[0]["source"]
        );

        // A restart: fresh lanes, the same disk.
        let mut lanes = setup.lanes(precedence);
        let audit = setup
            .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 3)
            .await;
        assert!(audit.is_empty(), "{audit:?}");
        assert_eq!(sink.feed_records().len(), 1);
    }
}

/// SFTP's copy was accepted first; the bucket's different copy arrives
/// within the window, created earlier. The bucket wins (D5): it is
/// ingested and SFTP's newer directory is retired, so schedule-reference
/// publishes the bucket's.
#[tokio::test]
async fn the_bucket_wins_a_disagreement_even_when_it_arrives_second() {
    let setup = Setup::new();
    let precedence = Precedence::default();
    let multi = setup.multi(precedence);
    let sink = FakeSink::new(Answer::Accept);
    let mut lanes = setup.lanes(precedence);
    let (sftp_at, bucket_at) = (ago(5), ago(20));
    put(
        setup.watch.path(),
        "timetable_full.zip",
        &real_zip(),
        sftp_at,
    );
    let first = setup
        .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 3)
        .await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["source"], "sftp");
    assert_eq!(
        delivery_dirs(setup.storage.path()),
        [delivery::delivery_dir_name(sftp_at)]
    );

    put(
        &setup.incoming(),
        "timetable_full.zip",
        &other_zip(),
        bucket_at,
    );
    let second = setup
        .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 3)
        .await;
    assert_eq!(second.len(), 1, "{second:?}");
    assert_eq!(second[0]["outcome"], "accepted");
    assert_eq!(second[0]["source"], "bucket");
    assert_eq!(
        delivery_dirs(setup.storage.path()),
        [delivery::delivery_dir_name(bucket_at)],
        "the losing SFTP directory is retired"
    );
    assert_eq!(sink.feed_records().len(), 2);
}

/// The bucket's copy was accepted first; SFTP's different copy within the
/// window is superseded: never extracted or recorded.
#[tokio::test]
async fn a_lower_ranked_different_copy_is_superseded() {
    let setup = Setup::new();
    let precedence = Precedence::default();
    let multi = setup.multi(precedence);
    let sink = FakeSink::new(Answer::Accept);
    let mut lanes = setup.lanes(precedence);
    let bucket = real_zip();
    let bucket_at = ago(30);
    put(&setup.incoming(), "timetable_full.zip", &bucket, bucket_at);
    put(
        setup.watch.path(),
        "timetable_full.zip",
        &other_zip(),
        ago(25),
    );
    let audit = setup
        .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 4)
        .await;
    let outcomes: Vec<(&str, &str)> = audit
        .iter()
        .map(|l| {
            (
                l["source"].as_str().unwrap(),
                l["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(outcomes, [("bucket", "accepted"), ("sftp", "superseded")]);
    assert_eq!(audit[1]["first_source"], "bucket");
    assert_eq!(audit[1]["duplicate_of"], sha(&bucket).as_str());
    assert_eq!(sink.feed_records().len(), 1);
    assert_eq!(
        delivery_dirs(setup.storage.path()),
        [delivery::delivery_dir_name(bucket_at)]
    );
}

/// Outside the window, a later different delivery is simply newer.
#[tokio::test]
async fn outside_the_window_a_different_copy_is_just_a_new_delivery() {
    let setup = Setup::new();
    let precedence = Precedence::default();
    let multi = setup.multi(precedence);
    let sink = FakeSink::new(Answer::Accept);
    let mut lanes = setup.lanes(precedence);
    put(
        &setup.incoming(),
        "timetable_full.zip",
        &real_zip(),
        ago(600),
    );
    put(
        setup.watch.path(),
        "timetable_full.zip",
        &other_zip(),
        ago(5),
    );
    let audit = setup
        .cycles(&sink, &Routing::defaults(), &multi, &mut lanes, 4)
        .await;
    let outcomes: Vec<&str> = audit
        .iter()
        .map(|l| l["outcome"].as_str().unwrap())
        .collect();
    assert_eq!(outcomes, ["accepted", "accepted"]);
    assert_eq!(delivery_dirs(setup.storage.path()).len(), 2);
}

/// CORPUS through both sources: loaded once; the other copy is a
/// duplicate, archived unloaded.
#[tokio::test]
async fn the_same_corpus_extract_through_both_sources_is_loaded_once() {
    let mut setup = Setup::new();
    setup.config.corpus.corpus_ingest_enabled = true;
    let extract = corpus::tests::gzip(
        include_str!("../tests/fixtures/corpus_extract_excerpt.json").as_bytes(),
    );
    put(
        setup.watch.path(),
        "CORPUSExtract.json.gz",
        &extract,
        ago(4),
    );
    put(&setup.incoming(), "CORPUSExtract.json.gz", &extract, ago(2));
    let precedence = Precedence::default();
    let multi = setup.multi(precedence);
    let sink = FakeSink::new(Answer::Accept);
    let mut lanes = setup.lanes(precedence);
    let audit = setup
        .cycles(&sink, &Routing::with_corpus(), &multi, &mut lanes, 4)
        .await;
    assert_eq!(sink.corpus_loads().len(), 1);
    assert_eq!(sink.corpus_loads()[0]["delivery_source"], "bucket");
    let outcomes: Vec<(&str, &str)> = audit
        .iter()
        .map(|l| {
            (
                l["source"].as_str().unwrap(),
                l["outcome"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(outcomes, [("bucket", "accepted"), ("sftp", "duplicate")]);
    assert!(!setup.watch.path().join("CORPUSExtract.json.gz").exists());
    let superseded = setup.storage.path().join("corpus/superseded");
    assert_eq!(std::fs::read_dir(superseded).unwrap().count(), 1);
}

/// Every bucket and multi-source metric exists at 0 once registered, so
/// the alerts' `increase()` sees the first event.
#[test]
fn every_source_metric_is_registered_at_zero() {
    let recorder = metrics_exporter_prometheus::PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    metrics::with_local_recorder(&recorder, || {
        crate::bucket::register_metrics();
        MultiSource::register_metrics();
    });
    let rendered = handle.render();
    let mut expected: Vec<String> = vec![
        "distant_signal_schedule_feed_source_access_revoked{source=\"bucket\"} 0".into(),
        "distant_signal_schedule_feed_source_download_capped{source=\"bucket\"} 0".into(),
        "distant_signal_schedule_feed_source_last_new_object_seconds{source=\"bucket\"} 0".into(),
        "distant_signal_schedule_feed_source_downloaded_bytes_total{source=\"bucket\"} 0".into(),
    ];
    for kind in crate::bucket::ERROR_KINDS {
        expected.push(format!(
            "distant_signal_schedule_feed_source_errors_total{{source=\"bucket\",kind=\"{kind}\"}} 0"
        ));
    }
    for reason in crate::bucket::UNEXPECTED_REASONS {
        expected.push(format!(
            "distant_signal_schedule_feed_source_unexpected_objects_total{{source=\"bucket\",reason=\"{reason}\"}} 0"
        ));
    }
    for kind in crate::sources::DISAGREEMENT_KINDS {
        expected.push(format!(
            "distant_signal_schedule_feed_source_disagreement_total{{kind=\"{kind}\"}} 0"
        ));
    }
    for line in expected {
        assert!(rendered.contains(&line), "missing {line:?} in\n{rendered}");
    }
}
