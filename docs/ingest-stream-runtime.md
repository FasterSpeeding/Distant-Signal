# The ingest stream runtime (`crates/ingest-stream`)

The Redis Streams runtime of the ingest architecture
([spec §7](superpowers/specs/2026-10-06-ingest-architecture-design.md#7-stream-design),
plan phase 3a). It has no database dependency, so the stream producers
(pollers) stay light; the ingest-writer adds its own handlers on top.
Status (2026-10-08): built and tested; its first callers (plan 3a.6-3a.8:
the writer's snapshot handlers, poller-ldbws and full-coverage-consumer)
ship with every stream `off` and every sink `http`.

## Why a crate and not `common::ingest_stream`

The spec first put this in `common` behind a `stream` feature. It is a
separate crate because:

- `common` is a dependency of every binary, the api included; the runtime
  (redis, flate2, a background task, the consumer loop) is needed only by
  the five stream producers and the writer;
- its Redis-gated tests get their own test binary and CI step;
- it keeps phase 3 out of `common` while `ds-store` is extracted (phase 1A)
  and the writer skeleton lands in parallel.

It depends on `common` (feature `redis`) for `common::backoff` and the
bounded `common::redis_conn::RedisConn`. Every dependency was already in
`Cargo.lock`.

## Envelope (`ingest_stream::envelope`)

| Item | What |
|---|---|
| `Envelope { v, schema, producer, key, produced_at, batch, payload }` | The logical entry. `payload` is validated JSON (`Box<RawValue>`). `serde` gives the JSON form; `produced_at` is RFC 3339 at milliseconds |
| `Envelope::new(schema, producer, key, produced_at, &payload)` / `from_raw(…)` / `with_batch(BatchPart)` | Build one |
| `Envelope::encode() -> Result<EncodedEntry, EnvelopeError>` | The flat Redis fields (`v schema producer key produced_at enc [batch part parts] body`). Gzip (`enc = json+gzip`) when the payload is over **8 KiB**; `EnvelopeError::TooLarge` when the body is still over **512 KiB** |
| `Envelope::decode(&fields) -> Result<Envelope, DecodeError>` | Any field order; unknown fields ignored. Refuses a body over 1 MiB and a gunzip over 16 MiB. `DecodeError::is_poison()` is false only for `UnsupportedVersion` (a newer producer: left pending) |
| `split_snapshot(schema, producer, produced_at, batch, rows, max_rows_per_part, wrap)` | Splits a snapshot into parts `i/n` keyed `<schema name>:<batch>:<i>/<n>`, halving the rows per part until every part fits 512 KiB |
| `SchemaId` | `name/version`, e.g. `station-samples/1` |

Golden fixtures (`crates/ingest-stream/tests/fixtures/`) pin the JSON form,
the plain wire form byte for byte, and a gzipped v1 part. Never edit them
to make a test pass: a change is a new envelope or schema version.

## Producer (`ingest_stream::producer`)

```rust
let (producer, task) = Producer::spawn(
    client,                                   // redis::Client with the producer's own ACL user
    ProducerConfig::new(streams::STATION_SAMPLES, decl.maxlen(), ProducePolicy::LatestSnapshot),
);
let parts = split_snapshot(&schema, &producer_id, polled_at, &batch, &rows, 100, |c| to_raw_value(&Body { rows: c }))?;
let receipt = producer.submit(parts).await?;  // never waits under LatestSnapshot
// readiness: producer.is_available() == false → "stream_unavailable"
// shutdown: producer.shutdown(task, grace).await
```

- One task per stream owns the connection and XADDs items in order with
  `XADD <stream> MAXLEN ~ <cap> * …`. Producers never `XTRIM`, so their ACL
  user needs only `+xadd` (and `+xrevrange` for `last_produced_at`).
- On any failure (down, NOAUTH/NOPERM, OOM, MISCONF) it retries the same
  encoded part (same key: the writer's dedup absorbs a duplicate after an
  ambiguous failure) on `PRODUCER_BACKOFF` (1 s doubling to 60 s,
  jittered), and `is_available()` turns false.
- `ProducePolicy::LatestSnapshot`: only the newest unsent item is kept; the
  older one's receipt resolves to `NotWritten::Superseded`.
- `ProducePolicy::Event { max_buffered }`: a bounded FIFO; `submit` waits
  when it is full (backpressure). A caller that must not lose an event
  acks its own upstream only after `receipt.written().await`.
- `last_produced_at(conn, stream)`: the newest entry's `produced_at`
  (`XREVRANGE + - COUNT 1`), the producer's last-fetched cursor (§11.3).
- `xadd_entry(conn, stream, maxlen, &entry)`: one XADD, for one-shot use.

### Snapshot sinks (`ingest_stream::snapshot`, plans 3a.7, 3a.8 and 3c.2)

What a poller or consumer with `INGEST_SINK` uses:

- `SinkMode`: `http` (default), `http+shadow` (the POST stays
  authoritative; what the api accepted is also XADDed, best effort),
  `stream` (XADD only).
- `Snapshot::add(stream, schema, producer, produced_at, rows, 100)`: one
  schema's rows as parts of today's POST body, batch and `produced_at`
  the snapshot's fetch time. A snapshot can carry several schemas
  (full-coverage's three outputs), and goes to the producer as **one**
  item, so the latest-only policy replaces a whole unsent snapshot.
- `SnapshotProducer`: the stream's `LatestSnapshot` producer, spawned on
  first use; `submit(snapshot)` (counts `ingest_stream_sink_rows_total
  {sink="stream"}` once written) and `cursor()` (`XREVRANGE`, spec §11.3).
- `RedisArgs`: `REDIS_URL`, `REDIS_USERNAME`, `REDIS_PASSWORD`, flattened
  into a poller's clap config; `client(why)` applies the ACL user.
- `SnapshotStream::spawn(client, stream, schema, component, rows_per_part)`:
  the one-schema form over a `SnapshotProducer`, spawned at once;
  `publish(&rows, fetched_at)` never waits for Redis (an empty `rows` is
  one empty part); `last_produced_at()` is the newest `produced_at` of its
  own schema, read backwards a page at a time (the island-of-Ireland
  stream carries five schemas from three pollers); `shutdown(grace)`.
  Used by poller-tfl, poller-tocs (`http+shadow`/`stream`) and the three
  island-of-Ireland pollers (`stream` only, decision D8).

| Producer | Stream | Each cycle under `stream` | Startup cursor under `stream` |
|---|---|---|---|
| poller-ldbws (`pollers.ldbws.ingest.sink`) | `ds:ingest:station-samples` | one snapshot; waits for the XADD up to the POST retry budget, then fails the cycle (transient) with the snapshot held | the stream's newest `produced_at` |
| full-coverage-consumer (`fullCoverageConsumer.ingest.sink`) | `ds:ingest:full-coverage` | one snapshot of its three outputs; never waits | (none: it is not a poller) |
| poller-tfl, poller-tocs (`pollers.<name>.ingest.sink`) | `ds:ingest:tfl`, `ds:ingest:reference` | one snapshot; never waits | the newest `produced_at` of its schema |
| the island-of-Ireland pollers (stream only) | `ds:ingest:island-of-ireland` | one snapshot per schema; never waits | the newest `produced_at` of its schema |

### The rollout (plan 3a; spec §13.1)

Per producer, values changes in Ranma-Config:

1. writer `ingestWriter.streams.<stream>: shadow` (with
   `redis.acl.clients.ingestWriter`);
2. producer `ingest.sink: http+shadow` (with its own Redis user:
   `redis.acl.clients.pollerLdbws`; full-coverage-consumer already has
   one);
3. compare for 3 days: `distant_signal:ingest_stream_rows_vs_http:ratio`
   (the writer's rows over the api's, per schema; 1 when they agree) stays
   at 1 and `DistantSignalIngestShadowMismatch` silent, 0 dead letters,
   `ingest_stream_bytes` within budget;
4. flip both in one change: writer `apply`, producer `stream` (the chart
   refuses `stream` without the writer on `apply`);
5. soak 7 days: no backlog, stalled or dead-letter alert, the api route at
   0 requests, `DistantSignalLdbwsStationStale` and
   `DistantSignalFullCoverageWindowStatsStalled` silent.

Rollback: producer `http` (the writer can stay on `apply`).

The spec's first compare metric, `ingest_stream_consumed_total
{outcome="skipped"}` against the api's request count, counts entries, and
a snapshot is several entries (parts of 100 rows), so the compare is by
rows instead.

## Consumer (`ingest_stream::consumer`, for the ingest-writer)

```rust
struct StationSamples { pool: PgPool }
impl Handler for StationSamples {
    fn handle(&self, e: &StreamEntry) -> impl Future<Output = Result<Handled, HandlerError>> + Send {
        async move { /* decode e.envelope.payload_as(), dedup + upsert in one transaction */ Ok(Handled::Applied) }
    }
}
let decl = budget::decl(streams::STATION_SAMPLES).unwrap();
let mut consumer = StreamConsumer::new(conn, ConsumerConfig::new(decl.stream, pod_name, decl.dead_letter_maxlen()))
    .with_progress(progress);
consumer.run(&handler, shutdown_signal()).await;
```

- Group `ingest-writer`, created with `XGROUP CREATE … 0 MKSTREAM`
  (`BUSYGROUP` ignored), again after `NOGROUP`.
- Reads its own PEL (`0`) first: at startup, after `XAUTOCLAIM` moved
  entries to it, and after every retry; then new entries (`>`, `COUNT 16
  BLOCK 5000`). Entries are handled one at a time in id order.
- Every 60 s: `XAUTOCLAIM … 300000 0-0 COUNT 100 JUSTID` (entries a dead
  pod left pending), and `XGROUP DELCONSUMER` of other consumers with
  nothing pending and over 1 h idle.
- The handler's answer:

  | Answer | Effect | `outcome` |
  |---|---|---|
  | `Ok(Handled::Applied)` / `Duplicate` / `Skipped` (shadow) | ack | `applied` / `duplicate` / `skipped` |
  | `Ok(Handled::PartiallyRejected { reason, rejected })` | the rejected rows go to the dead-letter stream, then ack | `rejected` |
  | `Err(HandlerError::Poison(reason))`, or an envelope that does not decode or is over 1 MiB | dead-letter with the reason, then ack | `dead_lettered` |
  | `Err(HandlerError::Transient(_))` | not acked; back off (1 s → 60 s), re-read the PEL | `transient_error` |
  | `Err(HandlerError::UnsupportedSchema(_))`, or envelope `v` > 1 | as transient (alerts; roll the writer forward) | `unsupported_schema` |
  | a pending entry that `MAXLEN` trimmed away | ack | `trimmed` |

- Dead letters go to `ds:dlq:<domain>` (`dead_letter_stream(stream)`) with
  `MAXLEN ~ <the source's cap>`: the original fields plus `error`,
  `reason`, `failed_at`, `deliveries`, `source_stream`, `source_id` (and
  `rejected_rows = true` for rejected rows). The re-injection runbook is
  plan 3a.4.
- Shutdown is honoured while waiting (the blocking read, a backoff), never
  in the middle of a handler. `step()` runs one iteration, for tests or a
  writer that wants its own loop.

### The writer's half (`crates/ingest-writer`, plan 3a.3)

- `stream`: `INGEST_WRITER_STREAMS` modes (`off`/`shadow`/`apply`, all
  `off` by default), one `StreamConsumer` task per stream not `off` with
  `WriterHandler` as its `Handler`, and the hourly `XTRIM <dlq> MINID ~
  <now − 7 d>` of the dead-letter streams.
- `handlers`: the registry (`schema name/version → SchemaHandler`; unknown
  name → `Poison`, unknown version → `UnsupportedSchema`), `classify`
  (SQLSTATE class 22/23 → `Poison`, anything else → `Transient`) and
  `apply_rows` (a savepoint per row; refused rows become
  `PartiallyRejected`), `apply_batch` (the whole batch in one savepoint,
  falling back to `apply_rows` only on a data error).
- `handlers::snapshots` (plan 3a.6): `station-samples/1`,
  `full-coverage-stats/1`, `full-coverage-window-stats/1` (validated as the
  api route validates it: an invalid row is poison) and
  `station-full-coverage-samples/1`. Each is the api route's `ds-store`
  upsert (its `_on` form, on the writer's transaction) with the row's
  observed time clamped (`polled_at`, `computed_at`, `resolved_at`; line
  stats get `source_updated_at := produced_at`) and the ordering guard on
  that column, then `record_ingest(<schema name>, produced_at)` in
  `ingest_freshness` (`GREATEST`, never backwards). Line stats advance
  `source_updated_at` on every snapshot, even unchanged (`updated_at`
  still means "last changed"), so the guard compares against the newest
  snapshot applied.
- `dedup`: `ingest_dedup`, claimed in each `apply` entry's transaction
  (`Duplicate` when already there), pruned hourly after 48 h under the
  `ingest_dedup_prune` loop lock.
- `observed`: `Observed::observed_at(row_time)` (the row's own time, else
  `produced_at`, clamped to `now() + 2 min` and counted) and
  `guard(table, column)`, the upsert `WHERE`: `(t.c IS NULL OR
  EXCLUDED.c >= t.c OR t.c > now() + interval '2 min')`.

## Budget (`ingest_stream::budget`)

`INGEST_STREAMS` declares each stream's rate, worst-case entry size (after
gzip) and bound; `check_budget(&INGEST_STREAMS, BUDGET_BYTES)` computes the
`MAXLEN`s and the worst case of each stream **and** its dead-letter stream
(`(MAXLEN + 100) × (entry + 512 B)`, the 100 being `MAXLEN ~`'s node
slack), and fails if a stream covers less than the 2-hour outage target or
the total is over 512 MB. A unit test runs it on the spec table:

| Stream | `MAXLEN ~` | Covers | Worst case, stream + dead letters |
|---|---|---|---|
| `ds:ingest:station-samples` | 720 | 2 h | 2 × 67.6 MB |
| `ds:ingest:full-coverage` | 360 | 2 h | 2 × 37.9 MB |
| `ds:ingest:tfl` | 288 | 24 h | 2 × 1.0 MB |
| `ds:ingest:reference` (tocs) | 30 | 30 days | 2 × 0.5 MB |
| `ds:ingest:island-of-ireland` (disabled) | 2000 | about 6 days | 2 × 35.5 MB |
| **Total** | | | **about 285 MB** of 512 MiB (alert at 75%, 384 MiB) |

## Metrics

All prefixed `distant_signal_` (`common::metrics::metric_name`); names in
`ingest_stream::metrics`.

| Metric | Type | Labels | From |
|---|---|---|---|
| `ingest_stream_produce_total` | counter | `stream`, `outcome` (`ok`, `down`, `oom`, `noauth`, `noperm`, `misconf`, `error`) | producer, one per XADD attempt |
| `ingest_stream_produce_bytes_total` | counter | `stream` | producer, `body` bytes written |
| `ingest_stream_produce_buffered` | gauge | `stream` | producer, items not yet fully written |
| `ingest_stream_produce_dropped_total` | counter | `stream`, `reason` (`superseded`, `oversize`) | producer |
| `ingest_stream_consumed_total` | counter | `stream`, `schema`, `outcome` (table above) | consumer |
| `ingest_stream_handler_seconds` | histogram | `stream`, `schema` | consumer |
| `ingest_stream_dead_lettered_total` | counter | `stream`, `reason` (`poison`, `undecodable`, `oversize`, `rejected_rows`) | consumer |
| `ingest_stream_lag`, `ingest_stream_pending`, `ingest_stream_oldest_pending_age_seconds` | gauge | `stream` | consumer, every 30 s (`XINFO GROUPS`, `XPENDING`) |
| `ingest_stream_dlq_length`, `ingest_stream_dlq_oldest_age_seconds` | gauge | `stream` | consumer, every 30 s (`XLEN`, `XRANGE - + COUNT 1`) |
| `ingest_stream_bytes` | gauge | `stream` | consumer, every 30 s (`MEMORY USAGE` of the stream plus its dead-letter stream) |
| `ingest_stream_last_applied_timestamp_seconds` | gauge | `stream` | consumer |
| `ingest_stream_observed_at_clamped_total` | counter | `stream`, `schema` | the ingest-writer's guard helpers (`ingest_writer::observed`, spec §7.8): an observed time clamped to `now() + 2 min` |
| `ingest_stream_rows_total` | counter | `stream`, `schema`, `mode` (`shadow`, `apply`) | the ingest-writer's snapshot handlers: rows decoded and validated (shadow) or written (apply) |
| `ingest_stream_row_writes_total` | counter | `stream`, `schema`, `outcome` (`written`, `skipped`) | the ingest-writer's snapshot handlers: of the applied rows, those an upsert inserted or updated, and those it left alone (unchanged, or refused as older); `INGEST_WRITER_CHANGED_ROWS_ONLY` (plan 3a.9) moves unchanged rows to `skipped` |
| `ingest_stream_sink_rows_total` | counter | `stream`, `schema`, `sink` (`http`, `stream`) | the producers (poller-ldbws, full-coverage-consumer): rows the api accepted, or rows whose snapshot was fully XADDed |

`register_producer(stream)` / `register_consumer(stream)` (called by
`Producer::spawn` / `StreamConsumer::new`) register the alerting series at
0. A caller whose `encode`/`split_snapshot` returns `TooLarge` counts it
with `metrics::record_oversize(stream)`.

## Tests

- Unit and golden: `cargo test -p ingest-stream`.
- Redis-gated (CI's rust-db-test job; local valkey on 6379):
  `cargo test -p ingest-stream --test redis_stream -- --ignored --test-threads=1`.
  Each test uses a random key prefix and deletes its keys and ACL users.
