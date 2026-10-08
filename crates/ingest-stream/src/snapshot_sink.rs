//! The stream half of a snapshot poller's sink (spec §9.1, §11.3; plan
//! 3a.7 and 3c.2): `INGEST_SINK` ([`SinkMode`]), the producer's Redis
//! connection ([`RedisArgs`]) and [`SnapshotStream`], which splits a parsed
//! snapshot into envelope parts and hands them to a latest-snapshot
//! [`Producer`].
//!
//! | `INGEST_SINK` | The api's `POST` | The stream | Startup cursor |
//! |---|---|---|---|
//! | `http` (default) | authoritative | – | the api's `GET` |
//! | `http+shadow` | authoritative | a copy of every snapshot, best effort (the writer's `shadow` mode checks it) | the api's `GET` |
//! | `stream` | – | authoritative | [`SnapshotStream::last_produced_at`] |
//!
//! The island-of-Ireland pollers (decision D8) have only `stream`.
//!
//! [`SnapshotStream::publish`] never waits for Redis
//! ([`ProducePolicy::LatestSnapshot`]): while Redis is unavailable the
//! newest unsent snapshot is kept and retried with backoff, and a newer one
//! replaces it (`ingest_stream_produce_dropped_total{reason="superseded"}`).
//! A failed or slow XADD therefore never fails a poll cycle; it is
//! reported by the `ingest_stream_produce_*` metrics and their alerts.

use std::fmt;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, Utc};
use common::secret::Secret;
use serde::Serialize;
use tokio::task::JoinHandle;

use crate::envelope::{
    EnvelopeError, SchemaId, field, format_produced_at, parse_produced_at, split_snapshot,
};
use crate::producer::{NotWritten, ProducePolicy, Producer, ProducerConfig};
use crate::{budget, metrics};

/// `INGEST_SINK`: where a poller's snapshots go. See the module docs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SinkMode {
    #[default]
    Http,
    HttpShadow,
    Stream,
}

impl SinkMode {
    /// Whether the api's `POST` is the write path.
    pub fn posts_http(self) -> bool {
        matches!(self, Self::Http | Self::HttpShadow)
    }

    /// Whether snapshots go to the stream (and so Redis is needed).
    pub fn produces(self) -> bool {
        matches!(self, Self::HttpShadow | Self::Stream)
    }
}

impl FromStr for SinkMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "http" => Ok(Self::Http),
            "http+shadow" => Ok(Self::HttpShadow),
            "stream" => Ok(Self::Stream),
            other => Err(format!(
                "unknown INGEST_SINK {other:?} (http, http+shadow or stream)"
            )),
        }
    }
}

impl fmt::Display for SinkMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Http => "http",
            Self::HttpShadow => "http+shadow",
            Self::Stream => "stream",
        })
    }
}

/// A stream producer's Redis connection settings (`REDIS_URL`,
/// `REDIS_USERNAME`, `REDIS_PASSWORD`; chart `redis.acl.clients.<poller>`
/// for the producer's own ACL user, decision D6).
#[derive(Clone, clap::Args)]
pub struct RedisArgs {
    /// Redis, for the ingest stream. Needed by `INGEST_SINK=http+shadow`
    /// and `stream`. May carry a password (`redis://:pw@host`).
    #[arg(long, env, hide_env_values = true)]
    pub redis_url: Option<Secret>,

    /// Redis AUTH password (chart `redis.auth`, or the producer's ACL
    /// user's own). Applied by `common::redis_auth::redis_url_with_credentials`,
    /// never logged.
    #[arg(long, env, hide_env_values = true)]
    pub redis_password: Option<Secret>,

    /// Redis ACL user (`poller-tfl`, `poller-tocs`, …). Unset or empty: the
    /// `default` user.
    #[arg(long, env)]
    pub redis_username: Option<String>,
}

impl fmt::Debug for RedisArgs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisArgs")
            .field("redis_url", &self.redis_url)
            .field("redis_password", &self.redis_password)
            .field("redis_username", &self.redis_username)
            .finish()
    }
}

impl RedisArgs {
    /// The client for `REDIS_URL` with the ACL user's credentials applied.
    /// `Err` (a message) when `REDIS_URL` is unset or the credentials do
    /// not combine; `why` names what needs it (`"INGEST_SINK=stream"`).
    pub fn client(&self, why: &str) -> Result<redis::Client, String> {
        let Some(url) = &self.redis_url else {
            return Err(format!("{why} needs REDIS_URL"));
        };
        let url = common::redis_auth::redis_url_with_credentials(
            url.expose(),
            self.redis_username.as_deref(),
            self.redis_password.as_ref(),
        )
        .map_err(|err| format!("{err:#}"))?;
        redis::Client::open(url.expose()).map_err(|err| format!("REDIS_URL: {err}"))
    }
}

/// Why a snapshot was not queued.
#[derive(Debug, thiserror::Error)]
pub enum PublishError {
    /// It could not be encoded (a single row over the part limit, counted
    /// as `reason="oversize"`, or a row that does not serialize).
    #[error("encoding the snapshot: {0}")]
    Encode(#[from] EnvelopeError),
    /// The producer was shut down.
    #[error(transparent)]
    Closed(#[from] NotWritten),
}

/// The declaration of `stream`, matched by suffix so a test's key prefix
/// (`<prefix>ds:ingest:tfl`) finds its stream's.
fn stream_decl(stream: &str) -> Option<&'static budget::StreamDecl> {
    budget::INGEST_STREAMS
        .iter()
        .find(|decl| stream.ends_with(decl.stream))
}

/// One poller's latest-snapshot producer on one stream and schema.
pub struct SnapshotStream {
    producer: Producer,
    task: JoinHandle<()>,
    client: redis::Client,
    schema: SchemaId,
    producer_id: String,
    max_rows_per_part: usize,
}

impl SnapshotStream {
    /// Starts the producer task for `stream` (its `MAXLEN` from
    /// [`budget::INGEST_STREAMS`]), writing `schema` entries as `component/<pod>`
    /// (`HOSTNAME`), at most `max_rows_per_part` rows per part. A
    /// `TfL` or tocs snapshot fits one part; a part is also halved until it
    /// fits 512 KiB ([`split_snapshot`]).
    pub fn spawn(
        client: redis::Client,
        stream: &str,
        schema: SchemaId,
        component: &str,
        max_rows_per_part: usize,
    ) -> Self {
        // Every ingest stream is declared (`budget` tests); a stream that
        // is not gets the smallest declared cap.
        let maxlen = stream_decl(stream).map_or(30, budget::StreamDecl::maxlen);
        let (producer, task) = Producer::spawn(
            client.clone(),
            ProducerConfig::new(stream, maxlen, ProducePolicy::LatestSnapshot),
        );
        let pod = std::env::var("HOSTNAME").unwrap_or_else(|_| "local".to_owned());
        Self {
            producer,
            task,
            client,
            schema,
            producer_id: format!("{component}/{pod}"),
            max_rows_per_part: max_rows_per_part.max(1),
        }
    }

    pub fn stream(&self) -> &str {
        self.producer.stream()
    }

    /// Queues `rows`, fetched at `produced_at`, as one snapshot: its parts
    /// share the batch `produced_at` and keep their keys across retries.
    /// Never waits for Redis. The write's outcome is logged when known (a
    /// newer snapshot superseding this one is normal while Redis is down).
    ///
    /// # Errors
    ///
    /// [`PublishError`]; nothing is queued then.
    pub async fn publish<T: Serialize>(
        &self,
        rows: &[T],
        produced_at: DateTime<Utc>,
    ) -> Result<(), PublishError> {
        let batch = format_produced_at(produced_at);
        let parts = match split_snapshot(
            &self.schema,
            &self.producer_id,
            produced_at,
            &batch,
            rows,
            self.max_rows_per_part,
            serde_json::value::to_raw_value,
        ) {
            Ok(parts) => parts,
            Err(err) => {
                if matches!(err, EnvelopeError::TooLarge { .. }) {
                    metrics::record_oversize(self.stream());
                }
                return Err(err.into());
            }
        };
        let count = parts.len();
        let receipt = self.producer.submit(parts).await?;
        let stream = self.stream().to_owned();
        let schema = self.schema.to_string();
        tokio::spawn(async move {
            match receipt.written().await {
                Ok(ids) => {
                    tracing::debug!(%stream, %schema, %batch, parts = count, ?ids, "snapshot written to the ingest stream");
                }
                Err(NotWritten::Superseded) => {
                    tracing::info!(%stream, %schema, %batch, "snapshot superseded by a newer one before Redis took it");
                }
                Err(NotWritten::Closed) => {
                    tracing::warn!(%stream, %schema, %batch, "producer closed before the snapshot was written");
                }
            }
        });
        Ok(())
    }

    /// Whether the last XADD succeeded (readiness `stream_unavailable` when
    /// false).
    pub fn is_available(&self) -> bool {
        self.producer.is_available()
    }

    /// The `produced_at` of the newest entry of this producer's schema: the
    /// poller's "last fetched" cursor under `INGEST_SINK=stream` (spec
    /// §11.3). The stream is read backwards (`XREVRANGE`) a page at a time
    /// until an entry of this schema turns up, bounded by the stream's
    /// `MAXLEN`. On a one-schema stream (`tfl`, `reference`) that is the
    /// newest entry; the island-of-Ireland stream is shared by three
    /// pollers and five schemas, and a daily poller must not take the
    /// five-minute live poller's entries for its own.
    ///
    /// # Errors
    ///
    /// A Redis error (unreachable, NOPERM).
    pub async fn last_produced_at(&self) -> redis::RedisResult<Option<DateTime<Utc>>> {
        const PAGE: usize = 50;
        let mut conn = common::redis_conn::connect(&self.client).await?;
        let stream = self.stream();
        let wanted = self.schema.to_string();
        let limit = stream_decl(stream)
            .map_or(2000, budget::StreamDecl::maxlen)
            .saturating_add(100);
        let mut end = "+".to_owned();
        let mut seen: u64 = 0;
        loop {
            let reply: redis::streams::StreamRangeReply = redis::cmd("XREVRANGE")
                .arg(stream)
                .arg(&end)
                .arg("-")
                .arg("COUNT")
                .arg(PAGE)
                .query_async(&mut conn)
                .await?;
            for entry in &reply.ids {
                if entry.get::<String>(field::SCHEMA).as_deref() == Some(wanted.as_str()) {
                    return Ok(entry
                        .get::<String>(field::PRODUCED_AT)
                        .and_then(|text| parse_produced_at(&text)));
                }
            }
            seen = seen.saturating_add(u64::try_from(reply.ids.len()).unwrap_or(u64::MAX));
            match reply.ids.last() {
                // Exclusive start (Redis 6.2+): the page before this one.
                Some(last) if reply.ids.len() == PAGE && seen < limit => {
                    end = format!("({}", last.id);
                }
                _ => return Ok(None),
            }
        }
    }

    /// Closes the producer and waits up to `grace` for what is queued.
    pub async fn shutdown(self, grace: Duration) -> bool {
        self.producer.shutdown(self.task, grace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sink_modes_parse_and_print() {
        for (text, mode) in [
            ("http", SinkMode::Http),
            ("http+shadow", SinkMode::HttpShadow),
            ("stream", SinkMode::Stream),
        ] {
            assert_eq!(text.parse::<SinkMode>(), Ok(mode));
            assert_eq!(mode.to_string(), text);
        }
        assert!("db".parse::<SinkMode>().is_err());
        assert_eq!(SinkMode::default(), SinkMode::Http);
        assert!(SinkMode::Http.posts_http() && !SinkMode::Http.produces());
        assert!(SinkMode::HttpShadow.posts_http() && SinkMode::HttpShadow.produces());
        assert!(!SinkMode::Stream.posts_http() && SinkMode::Stream.produces());
    }

    #[test]
    fn redis_args_need_a_url_and_apply_the_acl_user() {
        let none = RedisArgs {
            redis_url: None,
            redis_password: None,
            redis_username: None,
        };
        let err = none.client("INGEST_SINK=stream").unwrap_err();
        assert!(err.contains("INGEST_SINK=stream needs REDIS_URL"), "{err}");
        let user = RedisArgs {
            redis_url: Some("redis://redis:6379".into()),
            redis_password: Some("pw".into()),
            redis_username: Some("poller-tfl".into()),
        };
        let client = user.client("x").unwrap();
        let info = client.get_connection_info();
        assert_eq!(info.redis.username.as_deref(), Some("poller-tfl"));
        assert!(
            !format!("{user:?}").contains("pw\""),
            "the password is redacted"
        );
    }
}
