//! Where `schedule-ingest`'s two writes go (ingest architecture plan 2d.1,
//! spec §9.1): the per-delivery feed marker (`schedule_feed_ingests`) and
//! the monthly CORPUS load (`corpus_locations`).
//!
//! `INGEST_SINK` picks one implementation of [`IngestSink`] at startup:
//!
//! - [`HttpSink`] (`http`, the default): today's `POST`s to api's
//!   `/private/schedule-feed-ingests` and `/private/corpus-locations`,
//!   unchanged;
//! - [`DbSink`] (`db`): the same `ds_store` calls those routes make
//!   (`insert_schedule_feed_ingest`, `replace_corpus_locations_with_provenance`),
//!   as the `schedule_ingest` role, after the same validation the routes
//!   apply (`schedule_feed_ingest_problem`, `corpus_load_problem` and the
//!   CORPUS provenance check, all in `ds_store`).
//!
//! Both report failures in one vocabulary, [`SinkError`], so the callers'
//! retry and quarantine logic is the same whichever sink is active.
//!
//! What the api route does after a CORPUS load and [`DbSink`] does not: the
//! `api_corpus_last_delivered_at_seconds` gauge and the CORPUS-vs-timetable
//! comparison. Under `db` both come from the ingest-writer's CORPUS
//! crosswalk loop (`ds_store::loops::corpus_crosswalk_comparing`), which
//! reads the durable `corpus_deliveries` marker, so this process needs no
//! read grants beyond the load's own.

use anyhow::{Context, anyhow};
use chrono::{DateTime, Utc};
use common::oauth_client::OAuthTokenCache;
use common::secret::Secret;
use ds_store::corpus::{
    CorpusLocation, DeliveredFileProvenance, corpus_delivery_problem, corpus_provenance_problem,
    replace_corpus_locations_with_provenance,
};
use ds_store::schedule::{
    ScheduleFeedSource, insert_schedule_feed_ingest, schedule_feed_ingest_problem,
};
use reqwest::Client;
use serde::Serialize;
use sqlx::PgPool;

use crate::ScheduleFeedIngestRequest;

/// `pg_stat_activity.application_name` of the [`DbSink`] pool.
const APPLICATION_NAME: &str = "distant-signal-schedule-ingest";
/// Spec §6.6: pool 1, role limit 2. Scans are sequential: the feed marker
/// and the CORPUS load never run at once.
const DEFAULT_MAX_CONNECTIONS: u32 = 1;

/// `INGEST_SINK`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SinkKind {
    /// api's `/private/*` routes (today's behaviour).
    Http,
    /// Postgres directly (`DATABASE_URL`).
    Db,
}

/// Why a write failed, as far as retrying it is concerned.
#[derive(Debug)]
pub(crate) enum SinkError {
    /// The data itself was refused, so the same request fails the same way
    /// every time: api's 400/413/422 under `http`; under `db` the routes'
    /// own checks or a data error (SQLSTATE class 22 or 23).
    Rejected(anyhow::Error),
    /// Anything else: api or Postgres unreachable, a timeout, a 5xx. The
    /// same data can succeed later.
    Transient(anyhow::Error),
}

impl SinkError {
    /// Classifies an error from `common::ingest`'s POST helpers.
    fn from_http(err: anyhow::Error) -> Self {
        match common::ingest::classify_failure(&err) {
            common::ingest::FailureClass::Rejected => Self::Rejected(err),
            common::ingest::FailureClass::Transient => Self::Transient(err),
        }
    }

    /// Classifies an error from a `ds_store` write: a data error (class 22
    /// or 23) is a rejection, everything else transient.
    fn from_db(err: anyhow::Error) -> Self {
        let data_error = err
            .chain()
            .filter_map(|cause| cause.downcast_ref::<sqlx::Error>())
            .filter_map(sqlx::Error::as_database_error)
            .filter_map(|db| db.code())
            .any(|code| code.starts_with("22") || code.starts_with("23"));
        if data_error {
            Self::Rejected(err)
        } else {
            Self::Transient(err)
        }
    }
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(err) | Self::Transient(err) => std::fmt::Display::fmt(err, f),
        }
    }
}

/// One CORPUS delivery: the body of `POST /private/corpus-locations`
/// (api's `ds_store::corpus::CorpusLoadRequest`, field for field).
#[derive(Debug, Serialize)]
pub(crate) struct CorpusLoadRequest<'a> {
    pub delivered_at: DateTime<Utc>,
    pub source_file: &'a str,
    pub locations: &'a [CorpusLocation],
    /// The delivered file's size and SHA-256 (`audit.rs`).
    pub source_bytes: u64,
    pub sha256: &'a str,
}

/// The two writes, whichever way they go.
pub(crate) trait IngestSink {
    /// Records one verified schedule-feed delivery. Idempotent on
    /// `delivered_at`.
    async fn record_feed_ingest(
        &self,
        request: &ScheduleFeedIngestRequest,
    ) -> Result<(), SinkError>;

    /// Replaces `corpus_locations` with one whole CORPUS delivery.
    /// Idempotent: the same delivery twice leaves the same rows.
    async fn load_corpus(&self, request: &CorpusLoadRequest<'_>) -> Result<(), SinkError>;
}

/// The configured sink.
pub(crate) enum Sink {
    Http(HttpSink),
    Db(DbSink),
}

impl IngestSink for Sink {
    async fn record_feed_ingest(
        &self,
        request: &ScheduleFeedIngestRequest,
    ) -> Result<(), SinkError> {
        match self {
            Self::Http(sink) => sink.record_feed_ingest(request).await,
            Self::Db(sink) => sink.record_feed_ingest(request).await,
        }
    }

    async fn load_corpus(&self, request: &CorpusLoadRequest<'_>) -> Result<(), SinkError> {
        match self {
            Self::Http(sink) => sink.load_corpus(request).await,
            Self::Db(sink) => sink.load_corpus(request).await,
        }
    }
}

/// api's `/private/*` routes, with an internal OAuth token.
pub(crate) struct HttpSink {
    client: Client,
    feed_url: String,
    corpus_url: String,
    tokens: OAuthTokenCache,
}

impl HttpSink {
    pub(crate) fn new(
        client: Client,
        feed_url: String,
        corpus_url: String,
        tokens: OAuthTokenCache,
    ) -> Self {
        Self {
            client,
            feed_url,
            corpus_url,
            tokens,
        }
    }
}

impl IngestSink for HttpSink {
    /// Deliberately **not** `common::ingest::post_batch`: that helper
    /// always serializes a JSON *array*, but the route takes one JSON
    /// *object*, one record per verified delivery.
    async fn record_feed_ingest(
        &self,
        request: &ScheduleFeedIngestRequest,
    ) -> Result<(), SinkError> {
        common::ingest::post_json(&self.client, &self.feed_url, &self.tokens, request)
            .await
            // `context`, not a fresh `anyhow!`: the `HttpStatusError`
            // underneath must survive for `classify_failure` (N-2).
            .context("schedule feed ingest POST failed")
            .map_err(SinkError::from_http)?;
        tracing::info!(
            delivered_at = %request.delivered_at,
            files = request.files.len(),
            "posted schedule feed ingest to api"
        );
        Ok(())
    }

    async fn load_corpus(&self, request: &CorpusLoadRequest<'_>) -> Result<(), SinkError> {
        common::ingest::post_json(&self.client, &self.corpus_url, &self.tokens, request)
            .await
            .map_err(SinkError::from_http)
    }
}

/// Postgres directly, through `ds_store`.
pub(crate) struct DbSink {
    pool: PgPool,
}

impl DbSink {
    /// A sink on an existing pool (tests).
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Waits for Postgres (INF-5: not a crash loop), builds the pool
    /// (`DATABASE_*` settings, default size 1), then waits for the schema
    /// gate (spec §12.2: the migrations this build needs and the
    /// `schedule_ingest` role's grants) before the first write.
    pub(crate) async fn connect(
        database_url: &Secret,
        progress: &health_http::Progress,
    ) -> anyhow::Result<Self> {
        common::startup::retry_until_ready(
            "Postgres",
            common::startup::CONNECT_BACKOFF,
            Some(progress),
            || async {
                use sqlx::Connection;
                sqlx::PgConnection::connect(database_url.expose())
                    .await?
                    .close()
                    .await
            },
        )
        .await;
        let pool =
            ds_store::pool::PoolSettings::from_env(APPLICATION_NAME, DEFAULT_MAX_CONNECTIONS)?
                .connect(database_url.expose())
                .await?;
        ds_store::schema::wait_for_schema(
            &pool,
            ds_store::schema::DbRole::ScheduleIngest,
            Some(progress),
        )
        .await?;
        Ok(Self { pool })
    }
}

impl IngestSink for DbSink {
    /// `POST /private/schedule-feed-ingests`'s body, without the HTTP:
    /// the route's 422 check, then the same insert.
    async fn record_feed_ingest(
        &self,
        request: &ScheduleFeedIngestRequest,
    ) -> Result<(), SinkError> {
        let record = feed_ingest_record(request);
        if let Some(problem) = schedule_feed_ingest_problem(&record) {
            return Err(SinkError::Rejected(
                anyhow!(problem).context("schedule feed ingest record refused"),
            ));
        }
        let files = serde_json::to_value(&record.files)
            .context("serialising the schedule feed files")
            .map_err(SinkError::Transient)?;
        let source = ScheduleFeedSource {
            file: record.source_file.as_deref(),
            bytes: record.source_bytes.and_then(|b| i64::try_from(b).ok()),
            sha256: record.source_sha256.as_deref(),
        };
        insert_schedule_feed_ingest(
            &self.pool,
            record.delivered_at,
            record.ingested_at,
            &files,
            &source,
        )
        .await
        .context("schedule feed ingest insert failed")
        .map_err(SinkError::from_db)?;
        tracing::info!(
            delivered_at = %request.delivered_at,
            files = request.files.len(),
            "recorded schedule feed ingest in Postgres"
        );
        Ok(())
    }

    /// `POST /private/corpus-locations`'s body, without the HTTP: the
    /// route's 400 and 422 checks in its order, then the same load (one
    /// transaction, crosswalk included).
    async fn load_corpus(&self, request: &CorpusLoadRequest<'_>) -> Result<(), SinkError> {
        if let Some(problem) = corpus_load_request_problem(request) {
            return Err(SinkError::Rejected(
                anyhow!(problem).context("CORPUS load refused"),
            ));
        }
        let upserted = replace_corpus_locations_with_provenance(
            &self.pool,
            request.delivered_at,
            request.source_file,
            &DeliveredFileProvenance {
                bytes: i64::try_from(request.source_bytes).ok(),
                sha256: Some(request.sha256),
            },
            request.locations,
        )
        .await
        .context("CORPUS load failed")
        .map_err(SinkError::from_db)?;
        tracing::info!(
            delivered_at = %request.delivered_at,
            source_file = %request.source_file,
            sha256 = request.sha256,
            rows = upserted,
            "replaced corpus_locations"
        );
        Ok(())
    }
}

/// The record as api's route deserialises it.
fn feed_ingest_record(
    request: &ScheduleFeedIngestRequest,
) -> ds_store::schedule::ScheduleFeedIngestRequest {
    ds_store::schedule::ScheduleFeedIngestRequest {
        delivered_at: request.delivered_at,
        ingested_at: request.ingested_at,
        files: request
            .files
            .iter()
            .map(|f| ds_store::schedule::ScheduleFeedFile {
                name: f.name.clone(),
                bytes: f.bytes,
                sha256: f.sha256.clone(),
            })
            .collect(),
        source_file: Some(request.source_file.clone()),
        source_bytes: Some(request.source_bytes),
        source_sha256: Some(request.source_sha256.clone()),
    }
}

/// The CORPUS route's checks, in its order: `corpus_load_problem` (its
/// 400), then the provenance (its 422).
fn corpus_load_request_problem(request: &CorpusLoadRequest<'_>) -> Option<String> {
    corpus_delivery_problem(request.source_file, request.locations)
        .or_else(|| corpus_provenance_problem(Some(request.sha256), Some(request.source_bytes)))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::ScheduleFeedFile;

    pub(crate) fn location(nlc: &str) -> CorpusLocation {
        CorpusLocation {
            nlc: nlc.to_string(),
            stanox: None,
            tiploc: None,
            crs: None,
            uic: None,
            nlc_desc: None,
            nlc_desc16: None,
        }
    }

    fn feed_request() -> ScheduleFeedIngestRequest {
        ScheduleFeedIngestRequest {
            delivered_at: "2001-02-03T04:05:06Z".parse().unwrap(),
            ingested_at: "2001-02-03T05:00:00Z".parse().unwrap(),
            files: vec![ScheduleFeedFile {
                name: "RJTTF975MCA.txt".to_string(),
                bytes: 3,
                sha256: Some("cd".repeat(32)),
            }],
            source_file: "timetable_full.zip".to_string(),
            source_bytes: 77_222_226,
            source_sha256: "ab".repeat(32),
        }
    }

    /// The api's own deserialisation of what [`HttpSink`] sends.
    fn as_api_parses_it(
        request: &ScheduleFeedIngestRequest,
    ) -> ds_store::schedule::ScheduleFeedIngestRequest {
        serde_json::from_value(serde_json::to_value(request).unwrap()).unwrap()
    }

    /// A pool that never connects: every test below must be refused before
    /// any query.
    fn offline_sink() -> DbSink {
        DbSink::new(
            sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://nobody@127.0.0.1:1/none")
                .unwrap(),
        )
    }

    fn rejected_message(result: Result<(), SinkError>) -> String {
        match result {
            Err(SinkError::Rejected(err)) => err.root_cause().to_string(),
            other => panic!("expected a rejection, got {other:?}"),
        }
    }

    /// Plan 2d.1, "validation errors identical": for every malformed feed
    /// record, the direct sink refuses it with exactly the text api's
    /// route answers 422 with (`schedule_feed_ingest_problem` on what api
    /// deserialises from the HTTP body), and never reaches the database.
    #[tokio::test]
    async fn the_direct_sink_refuses_a_bad_feed_record_with_the_route_s_message() {
        let sink = offline_sink();
        let cases: Vec<Box<dyn Fn(&mut ScheduleFeedIngestRequest)>> = vec![
            Box::new(|r| r.source_sha256 = "NOT-A-SHA".to_string()),
            Box::new(|r| r.source_bytes = u64::MAX),
            Box::new(|r| r.source_file = "  ".to_string()),
            Box::new(|r| r.files[0].sha256 = Some("AB".repeat(32))),
        ];
        for (i, mutate) in cases.iter().enumerate() {
            let mut request = feed_request();
            mutate(&mut request);
            let route = schedule_feed_ingest_problem(&as_api_parses_it(&request))
                .unwrap_or_else(|| panic!("case {i}: the route accepts it"));
            let direct = rejected_message(sink.record_feed_ingest(&request).await);
            assert_eq!(direct, route, "case {i}");
        }
        assert_eq!(
            schedule_feed_ingest_problem(&as_api_parses_it(&feed_request())),
            None
        );
        assert_eq!(
            feed_ingest_record(&feed_request()).files[0].sha256,
            as_api_parses_it(&feed_request()).files[0].sha256
        );
    }

    /// The same for CORPUS: the route's 400 (`corpus_load_problem` on
    /// api's deserialisation) and then its 422 (the provenance), in that
    /// order.
    #[tokio::test]
    async fn the_direct_sink_refuses_a_bad_corpus_load_with_the_route_s_message() {
        let sink = offline_sink();
        let good = [location("559500")];
        let blank = [location("559500"), location(" ")];
        let sha = "0f".repeat(32);
        let request = |source_file, locations, source_bytes, sha256| CorpusLoadRequest {
            delivered_at: "2001-02-03T04:05:06Z".parse().unwrap(),
            source_file,
            locations,
            source_bytes,
            sha256,
        };
        let cases = [
            request("CORPUSExtract.json.gz", &[][..], 1, sha.as_str()),
            request("", &good[..], 1, sha.as_str()),
            request("CORPUSExtract.json.gz", &blank[..], 1, sha.as_str()),
            request("CORPUSExtract.json.gz", &good[..], 1, "NOT-A-SHA"),
            request("CORPUSExtract.json.gz", &good[..], u64::MAX, sha.as_str()),
            // Both wrong: the 400 wins, as in the route.
            request("CORPUSExtract.json.gz", &[][..], 1, "NOT-A-SHA"),
        ];
        for (i, case) in cases.iter().enumerate() {
            let parsed: ds_store::corpus::CorpusLoadRequest =
                serde_json::from_value(serde_json::to_value(case).unwrap()).unwrap();
            let route = ds_store::corpus::corpus_load_problem(&parsed)
                .or_else(|| {
                    corpus_provenance_problem(parsed.sha256.as_deref(), parsed.source_bytes)
                })
                .unwrap_or_else(|| panic!("case {i}: the route accepts it"));
            let direct = rejected_message(sink.load_corpus(case).await);
            assert_eq!(direct, route, "case {i}");
        }
        assert_eq!(
            corpus_load_request_problem(&request(
                "CORPUSExtract.json.gz",
                &good[..],
                1,
                sha.as_str()
            )),
            None
        );
    }

    /// Under `http` a 400/413/422 is a rejection and everything else
    /// (here a 503) transient, exactly as `common::ingest::classify_failure`
    /// said before the sink existed.
    #[tokio::test]
    async fn the_http_sink_classifies_the_route_s_answers() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"access_token": "t", "expires_in": 3600})),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/feed"))
            .respond_with(
                ResponseTemplate::new(422).set_body_string("source_file must not be blank"),
            )
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/corpus"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let sink = HttpSink::new(
            Client::new(),
            format!("{}/feed", server.uri()),
            format!("{}/corpus", server.uri()),
            OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
                token_url: format!("{}/token", server.uri()),
                client_id: "test-client".to_string(),
                scope: "groups".to_string(),
                username: "test-user".to_string(),
                password: "test-password".to_string(),
            }),
        );
        match sink.record_feed_ingest(&feed_request()).await {
            Err(SinkError::Rejected(err)) => {
                assert!(format!("{err:#}").contains("source_file must not be blank"));
            }
            other => panic!("expected a rejection, got {other:?}"),
        }
        let good = [location("559500")];
        let sha = "0f".repeat(32);
        let corpus = CorpusLoadRequest {
            delivered_at: Utc::now(),
            source_file: "CORPUSExtract.json.gz",
            locations: &good,
            source_bytes: 1,
            sha256: &sha,
        };
        assert!(matches!(
            sink.load_corpus(&corpus).await,
            Err(SinkError::Transient(_))
        ));
    }

    #[test]
    fn data_errors_are_rejections_and_the_rest_transient() {
        assert!(matches!(
            SinkError::from_db(anyhow!("connection refused")),
            SinkError::Transient(_)
        ));
        assert!(matches!(
            SinkError::from_db(anyhow::Error::new(sqlx::Error::PoolTimedOut).context("load")),
            SinkError::Transient(_)
        ));
    }
}

/// Database-gated: run in CI against a freshly migrated database, both as
/// the superuser and (plan 2d.2) as the narrow `schedule_ingest` role
/// (`scripts/test-postgres-roles.py --mode per-service`), so the role's
/// grants are proven sufficient for both writes.
///
/// The CORPUS test replaces the WHOLE `corpus_locations` table, so it
/// refuses to run on a database holding any other CORPUS delivery (see
/// `ds_store::test_support`'s module doc): it is for CI's fresh database or
/// a scratch one. Fixture deliveries are dated 2001, so they never look
/// fresh. Cleanup the narrow role cannot do (it has no DELETE on
/// `schedule_feed_ingests`) runs as `MIGRATION_DATABASE_URL` (the owner,
/// which the role-split harness sets) when set, else `DATABASE_URL`.
#[cfg(test)]
mod db_tests {
    use chrono::TimeZone;

    use super::tests::location;
    use super::*;
    use crate::ScheduleFeedFile;

    fn url(var: &str) -> Option<String> {
        std::env::var(var).ok().filter(|v| !v.is_empty())
    }

    async fn sink() -> DbSink {
        let url = url("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        DbSink::new(PgPool::connect(&url).await.expect("connect"))
    }

    /// A pool that may delete the fixtures.
    async fn cleanup_pool() -> PgPool {
        let url = url("MIGRATION_DATABASE_URL")
            .or_else(|| url("DATABASE_URL"))
            .expect("DATABASE_URL must be set to run this test");
        PgPool::connect(&url).await.expect("connect for cleanup")
    }

    /// The schema gate passes for the role's grants (as `DATABASE_URL`).
    #[tokio::test]
    #[ignore = "requires a live database; run with DATABASE_URL set and --ignored"]
    async fn the_schema_gate_passes_for_the_schedule_ingest_role() {
        let gate = ds_store::schema::SchemaGate {
            deadline: std::time::Duration::ZERO,
            ..ds_store::schema::SchemaGate::for_role(ds_store::schema::DbRole::ScheduleIngest)
        };
        ds_store::schema::wait_for_schema_with(&sink().await.pool, &gate, None)
            .await
            .unwrap();
    }

    /// The direct sink stores exactly what api's route stores for the same
    /// HTTP body, and a second record of the same delivery is a no-op.
    #[tokio::test]
    #[ignore = "requires a live database; run with DATABASE_URL set and --ignored"]
    async fn the_direct_sink_records_a_feed_delivery_as_the_route_does() {
        let delivered_at = Utc.with_ymd_and_hms(2001, 2, 3, 4, 5, 6).unwrap();
        let cleanup = cleanup_pool().await;
        let delete = || async {
            sqlx::query("DELETE FROM schedule_feed_ingests WHERE delivered_at = $1")
                .bind(delivered_at)
                .execute(&cleanup)
                .await
                .expect("cleanup");
        };
        delete().await;

        let request = ScheduleFeedIngestRequest {
            delivered_at,
            ingested_at: delivered_at + chrono::Duration::hours(1),
            files: vec![
                ScheduleFeedFile {
                    name: "RJTTF975MCA.txt".to_string(),
                    bytes: 3,
                    sha256: Some("cd".repeat(32)),
                },
                ScheduleFeedFile {
                    name: "RJTTF975.MSN".to_string(),
                    bytes: 4,
                    sha256: None,
                },
            ],
            source_file: "timetable_full.zip".to_string(),
            source_bytes: 77_222_226,
            source_sha256: "ab".repeat(32),
        };
        let sink = sink().await;
        sink.record_feed_ingest(&request).await.unwrap();
        let mut again = request.clone();
        again.source_file = "other.zip".to_string();
        sink.record_feed_ingest(&again).await.unwrap();

        type Row = (
            DateTime<Utc>,
            serde_json::Value,
            Option<String>,
            Option<i64>,
            Option<String>,
        );
        let row: Row = sqlx::query_as(
            "SELECT ingested_at, files, source_file, source_bytes, source_sha256 \
             FROM schedule_feed_ingests WHERE delivered_at = $1",
        )
        .bind(delivered_at)
        .fetch_one(&cleanup)
        .await
        .unwrap();
        // What the route stores: its own deserialisation of the HTTP body.
        let route: ds_store::schedule::ScheduleFeedIngestRequest =
            serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
        assert_eq!(row.0, route.ingested_at);
        assert_eq!(row.1, serde_json::to_value(&route.files).unwrap());
        assert_eq!(
            row.2.as_deref(),
            Some("timetable_full.zip"),
            "first one wins"
        );
        assert_eq!(row.3, Some(77_222_226));
        assert_eq!(row.4, route.source_sha256);
        delete().await;
    }

    /// A whole CORPUS load through the direct sink: the rows, the
    /// delivery's provenance, and the same load twice is idempotent.
    #[tokio::test]
    #[ignore = "requires a live database with no real CORPUS data (CI's fresh one); replaces corpus_locations"]
    async fn the_direct_sink_loads_a_corpus_delivery() {
        let delivered_at = Utc.with_ymd_and_hms(2001, 2, 3, 4, 5, 6).unwrap();
        let cleanup = cleanup_pool().await;
        let foreign: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM corpus_deliveries WHERE delivered_at <> $1) \
                  + (SELECT COUNT(*) FROM corpus_crosswalk_build WHERE delivered_at <> $1)",
        )
        .bind(delivered_at)
        .fetch_one(&cleanup)
        .await
        .unwrap();
        assert_eq!(
            foreign, 0,
            "refusing to replace corpus_locations: this database holds another CORPUS delivery"
        );
        let delete = || async {
            for sql in [
                "DELETE FROM corpus_locations WHERE delivered_at = $1",
                "DELETE FROM corpus_deliveries WHERE delivered_at = $1",
                "DELETE FROM corpus_crosswalk_build WHERE delivered_at = $1",
            ] {
                sqlx::query(sql)
                    .bind(delivered_at)
                    .execute(&cleanup)
                    .await
                    .expect("cleanup");
            }
        };
        delete().await;

        let mut clapham = location("559500");
        clapham.tiploc = Some("CLPHMJN".to_string());
        let locations = [clapham, location("000800")];
        let sha = "0f".repeat(32);
        let request = CorpusLoadRequest {
            delivered_at,
            source_file: "CORPUSExtract.json.gz",
            locations: &locations,
            source_bytes: 295_957,
            sha256: &sha,
        };
        let sink = sink().await;
        for _ in 0..2 {
            sink.load_corpus(&request).await.unwrap();
        }

        let rows: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT nlc, tiploc FROM corpus_locations ORDER BY nlc")
                .fetch_all(&cleanup)
                .await
                .unwrap();
        assert_eq!(
            rows,
            [
                ("000800".to_string(), None),
                ("559500".to_string(), Some("CLPHMJN".to_string()))
            ]
        );
        let delivery: (String, i32, Option<i64>, Option<String>) = sqlx::query_as(
            "SELECT source_file, row_count, source_bytes, sha256 FROM corpus_deliveries",
        )
        .fetch_one(&cleanup)
        .await
        .unwrap();
        assert_eq!(
            delivery,
            (
                "CORPUSExtract.json.gz".to_string(),
                2,
                Some(295_957),
                Some(sha.clone())
            )
        );
        delete().await;
    }
}
