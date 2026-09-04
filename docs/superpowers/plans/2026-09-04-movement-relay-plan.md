# Plan: `movement-relay` — a Single Real Kafka Client, Fanned Out via Redis Streams — Implementation

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> **This plan has three parts, deliberately not the same kind of thing.**
> **Deploy A (Tasks 1–12)** and **Deploy C (Tasks 13–16)** are normal,
> buildable, testable, mergeable implementation work — plan and execute them
> exactly like any other task list in this repo, checkbox by checkbox,
> `cargo`/`helm template` verification at the end of each.
> **Deploy B, the runbook in its own section below, is NOT a task for an
> implementation agent to "complete."** It is a manual, one-time, watched
> production operation against a real, non-renewable RDM Kafka credential
> with no rehearsal environment (RDM enforces one consumer group per account
> per product — there is no second group to test the cutover against). If
> you are an agent executing this plan: **implement and merge Tasks 1–12,
> stop, and hand the Deploy B runbook to a human. Do not attempt to run,
> simulate, dry-run, or automate any command in the Deploy B section
> yourself, under any circumstance, including being asked to "just try it in
> a test namespace" — there is no test namespace this design doc's own
> "one group per account" constraint permits.** Tasks 13–16 (Deploy C) must
> not be merged until a human confirms Deploy B's B3 step has completed
> successfully in production — see Task 13's own gate note.

**Goal:** implement
`docs/superpowers/specs/2026-09-04-movement-relay-design.md` ("the design
doc") end to end. The design doc's Decisions section is authoritative for
every shape below; this plan does not repeat its reasoning, only the
concrete steps — except where the design doc explicitly poses an open
question as "a judgment call the implementation plan should make explicitly"
(its "Open questions / risks" section, items 3–5, and Decision 4's own list).
Those calls are made below, plainly, not left dangling a second time. See
"Judgment calls this plan makes" immediately below.

**Architecture:** one small additive function on the already-pure
`crates/trust-schema` (Task 1); one new shared library crate,
`crates/movement-feed`, consumed by both existing consumer crates as a
Redis Streams reader (Tasks 2–3); both `crates/trust-consumer` and
`crates/full-coverage-consumer` gain a config-selectable second
`MovementFeed` backend, Kafka kept as the default until Deploy B flips it
(Task 4); one new binary crate, `crates/movement-relay`, the sole real
Kafka client from Deploy B onward (Tasks 5–8); a new Helm Deployment +
values + secret wiring (Tasks 9–10); CI/local-dev wiring (Task 11); a full
verification pass (Task 12); then, only after a human has executed Deploy B
in production, Deploy C deletes the now-dead Kafka path from both existing
consumer crates (Tasks 13–16).

**Tech stack:** Rust (`rdkafka` for `movement-relay`'s one real Kafka
client, matching `trust-consumer`'s existing convention exactly; `redis`
0.27 with `tokio-comp`/`connection-manager` features for every Redis
Streams caller, matching `crates/api` and `crates/enricher`'s existing
convention; `axum` for every health endpoint, matching every existing
persistent-consumer crate).

**Design doc:**
`docs/superpowers/specs/2026-09-04-movement-relay-design.md`.

---

## Judgment calls this plan makes (read before Task 1)

The design doc flagged five things as genuine judgment calls for "the
implementation plan," not decided there. Resolved here, plainly:

1. **Deploy A/B/C sequencing (Decision 4).** Deploy A and Deploy C are
   ordinary implementation task lists (Tasks 1–12 and 13–16 below). Deploy B
   is a hand-executed runbook, not a task list — see the banner above and
   the runbook section. This split is the organizing structure of this
   entire plan, not a footnote.

2. **How B1 stops `trust-consumer`'s Kafka connection (design doc's Open
   Question 3): add `trustConsumer.replicaCount` as a first-class,
   values-driven chart knob (default `1`), not an out-of-band `kubectl
   scale`.** Reasoning: `trust-consumer-deployment.yaml:34` currently
   hardcodes `replicas: 1` as a YAML literal. An out-of-band `kubectl scale
   --replicas=0` works for exactly one manual moment, but the very next
   `helm upgrade` that doesn't also carry `--set
   trustConsumer.replicaCount=0` silently scales it back to 1 (Helm
   reconciles the Deployment spec back to whatever the chart template
   renders, which still says `replicas: 1` verbatim) — precisely during the
   B1→B2 window this design doc's own Open Question 2 already flags as
   risk-bearing if it runs long. A values-driven knob makes "trust-consumer
   is at zero replicas" an explicit, visible, `helm get values`-inspectable
   part of every command run during the cutover, not a fact that lives only
   in `kubectl`'s live cluster state and can be silently undone by the next
   unrelated `helm upgrade`. Same knob added for `fullCoverageConsumer` for
   symmetry (Task 10) — not load-bearing for B1 itself (Decision 6 confirms
   `full-coverage-consumer` never held real group membership to begin with),
   but a consistent mental model for an operator reading `values.yaml` next
   to `trustConsumer.replicaCount` is worth the two extra lines.

3. **Graceful Kafka shutdown / `LeaveGroup` on SIGTERM (design doc's Open
   Question 4): out of scope for this plan's code, handled instead as a
   timed pause in the Deploy B runbook.** Reasoning: `feed/kafka.rs` in
   both `trust-consumer` and `full-coverage-consumer` is deleted outright by
   Deploy C (Tasks 13–14) — writing and testing a
   `rdkafka::ConsumerContext`/graceful-close shutdown handler into code with
   a known, already-scheduled deletion date buys nothing beyond this one
   cutover, and the one real use of it (B1's stop) has a "good enough"
   fallback: a fixed, conservative wait after scale-to-zero before treating
   the group member as evicted (the runbook uses 90s, see its own reasoning
   below). A code change is riskier to get right on the one irreplaceable
   real run than a documented pause is — the pause has no failure mode
   beyond "waited a bit too long," where a shutdown-handler bug risks a
   silent non-departure on the one run that matters. `movement-relay`
   itself (the ongoing, permanent Kafka client from Deploy B onward) is a
   different case and is **not** exempted by this reasoning — see Task 7's
   own note on why `movement-relay`'s *own* future redeploys don't need
   this either (Decision 4's "same-service rollout" safety argument in the
   design doc already covers that case without a `LeaveGroup` handler).

4. **`movementRelay.enabled` toggle's lifespan (design doc's Open Question
   5): kept permanently, not scheduled for removal.** Reasoning: this
   chart already has two permanent, never-removed toggles of the identical
   shape — `scheduleFeed.enabled` and every `pollers.<name>.enabled` —
   neither of which was ever planned for removal once its feature
   stabilized. `movementRelay.enabled` follows that established precedent.
   A documented, permanent kill switch for the chart's one real Kafka
   client (a single point of failure for the whole movement-events
   pipeline, per the design doc's own Non-goals section on Redis's
   unhardened single-instance posture) is a reasonable thing to keep
   forever, not a temporary migration scaffold. No Deploy-C-or-later task
   removes it.

5. **(Not separately numbered in the design doc, but load-bearing for this
   plan's own Task 4/9 shape): how Deploy A ships a Redis-backed
   `MovementFeed` as "available but not yet wired to real traffic"
   without requiring a second code merge at cutover time.** Resolved as: a
   new config-selectable enum field, `movement_feed_backend: kafka |
   redis-stream`, added to both `trust-consumer` and `full-coverage-consumer`
   in Deploy A (Task 4), defaulting to `kafka` (zero change to today's
   production behavior when Deploy A merges). Deploy B's B3 step is then a
   pure Helm-values flip (`--set trustConsumer.movementFeed=redis-stream`)
   plus a redeploy — **not** a new code change at the moment of cutover.
   This is what makes B3 executable from a runbook command instead of
   requiring an implementation agent to write and merge code during the
   dangerous window.

---

## Non-goals — binding, same as the design doc's own, restated for the
## implementation stage

- **`trust-schema`'s existing parsing logic.** Unchanged beyond the one
  additive `confirmed_envelope_bodies` function (Task 1).
- **`process.rs`, `matching.rs`, `correlate.rs`, `station_correlate.rs`,
  `population.rs`, or any write path to `api`.** Every one of these
  consumes `MovementFeed::next_batch()`'s `Vec<String>` exactly as it always
  has — Decision 2's field-layout choice (raw `payload` string, unchanged
  shape) is what makes this true. No call site inside these files changes.
- **`full-coverage-consumer`'s own correlation-logic correctness, or
  flipping `LineDefinition.full_coverage_enabled` for any real line.**
  Unaffected; this plan only changes how events *reach* both consumers, not
  what either does with them.
- **Redis's own deployment/HA posture.** The design doc's own Non-goals
  flags this as a real, new operational cost this plan does not solve —
  `redis.enabled: true` stays a single, non-clustered `redis:7` instance
  with the resource/`maxmemory-policy` posture it already has today.
  Sizing/hardening it for `movement-events`'s real write volume is a
  separate, future piece of work.
- **A UI/dashboard for stream health.** Prometheus metrics only (Tasks 7,
  9).
- **Real peak-vs-average `TRAIN_MVT_ALL_TOC` message-rate research, or
  RDM's `retention.ms`.** Both remain open, pre-existing gaps this plan
  does not close (design doc Open Questions 1–2) — `MAXLEN` stays the
  design doc's proposed starting figure, `500,000`, until real production
  lag data suggests otherwise.
- **Automating, dry-running, or rehearsing Deploy B.** Stated once already
  in the banner above; repeated here because it's binding, not decorative.
- **RDM's own consumer-group visibility (design doc Open Question 7).**
  If RDM exposes a dashboard confirming group membership, the Deploy B
  runbook uses it as a corroborating check where noted; this plan does not
  invent or assume such a dashboard exists.

## Global Constraints

- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo test --workspace` (non-`#[ignore]`d tests) after
  every task. `movement-feed`'s Redis-backed tests are the **first**
  `#[ignore]`-gated tests in this repo that need a live Redis rather than a
  live Postgres — Task 3 adds a `redis:` service block to
  `.github/workflows/ci.yml`'s `rust-test` job (mirroring its existing
  `postgres:` service block exactly) and a `REDIS_URL`-gated `cargo test -p
  movement-feed -- --ignored --test-threads=1` step, run locally the same
  way: `REDIS_URL=redis://localhost:6379 cargo test -p movement-feed --
  --ignored --test-threads=1`.
- **New-crate conventions.** Every new/modified `Cargo.toml` below pins the
  **same** dependency versions this workspace already uses for that crate
  (copied from `crates/trust-consumer/Cargo.toml`,
  `crates/full-coverage-consumer/Cargo.toml`, `crates/enricher/Cargo.toml`,
  and `crates/api/Cargo.toml` at the exact versions quoted in this plan) —
  confirm they're still current at implementation time via `grep` against
  those files; don't silently downgrade or introduce a second version of a
  dependency already pinned elsewhere in the workspace.
- **Wire field naming.** Every Redis Streams field name and every new Helm
  value/env var uses the same snake_case-in-Rust /
  SCREAMING_SNAKE_CASE-in-env convention every existing crate in this repo
  already uses. No camelCase anywhere in this plan's new wire surface.
- **`rdkafka` stays a dependency of exactly the crates that need it, at
  each stage.** After Deploy A merges: `trust-consumer` and
  `full-coverage-consumer` **keep** `rdkafka` (their Kafka backend is still
  compiled in, just not selected by default) and `movement-relay` **gains**
  it. After Deploy C merges: `trust-consumer` and `full-coverage-consumer`
  **drop** `rdkafka` entirely; `movement-relay` is the sole `rdkafka`-
  dependent crate in the workspace. Task 13/14 make this drop explicit,
  including the CI/Dockerfile build-dependency cleanup that follows from
  it.
- **File scope.** New: `crates/movement-feed/`, `crates/movement-relay/`,
  `docker/movement-relay.Dockerfile`,
  `charts/distant-signal/templates/movement-relay-deployment.yaml`.
  Modified (Deploy A): `Cargo.toml` (workspace members),
  `crates/trust-schema/src/schema.rs`,
  `crates/trust-consumer/{Cargo.toml,src/{config.rs,main.rs,feed/mod.rs}}`
  (`feed/kafka.rs` untouched in Deploy A — only deleted in Deploy C),
  `crates/full-coverage-consumer/{Cargo.toml,src/{config.rs,main.rs,feed/mod.rs}}`
  (same), `charts/distant-signal/{values.yaml,templates/{_helpers.tpl,secret.yaml,trust-consumer-deployment.yaml,full-coverage-consumer-deployment.yaml}}`,
  `.github/workflows/{ci.yml,containers.yml}`, `docker-compose.yml`.
  Modified (Deploy C, gated — see Task 13): `crates/trust-consumer/{Cargo.toml,src/{config.rs,main.rs}}`
  (`feed/kafka.rs` deleted), `crates/full-coverage-consumer/` (same),
  `docker/{trust-consumer,full-coverage-consumer}.Dockerfile`,
  `charts/distant-signal/templates/{trust-consumer-deployment,full-coverage-consumer-deployment}.yaml`,
  `charts/distant-signal/values.yaml`, `docker-compose.yml`,
  `.github/workflows/ci.yml` (rdkafka build-dependency removal, if nothing
  else in the workspace still needs it — confirmed at Task 16, since
  `movement-relay` still does).
- **No `lines/*.toml` changes anywhere in this plan.**
- **No change to any confirmed `TrustMessage` field/shape.** `Activation`,
  `Movement`, `Cancellation`, `ChangeOfOrigin`, `ChangeOfIdentity`,
  `dedup::dedup_key`, and every function in `journey.rs` stay byte-for-byte
  unchanged.

---

# Deploy A — dormant infrastructure, normal review bar

## Task 1: `trust-schema` — `confirmed_envelope_bodies`

**Files:** modify `crates/trust-schema/src/schema.rs`.

Independent. First, small, additive. The **only** change to `trust-schema`
in this entire plan.

- [ ] **Step 1: Add the function**, directly after `parse_batch`
  (`schema.rs:124-132`), sharing its exact array/bare-object dispatch and
  reusing `parse_envelope`'s confirmed-`msg_type` list as the single source
  of truth (not a second, hand-copied list):

```rust
/// The `movement-relay` filtering primitive
/// (docs/superpowers/specs/2026-09-04-movement-relay-design.md Decision 1):
/// classifies each envelope in `raw` by `header.msg_type` alone against
/// the same five confirmed types `parse_envelope` already encodes, and
/// re-serializes each SURVIVING envelope's own `serde_json::Value`
/// verbatim, byte-faithful, even in the rare multi-envelope-array case.
///
/// Deliberately does NOT attempt to deserialize `body` into any typed
/// struct -- an envelope with a confirmed `msg_type` but a body that would
/// fail `parse_envelope`'s own typed deserialization (missing/malformed
/// fields) still survives here unchanged. That validation job stays where
/// it already lives, inside each downstream consumer's own `parse_batch`
/// call -- this function only ever looks at `header.msg_type`.
///
/// Shares `parse_batch`'s error behavior for a structurally malformed
/// payload (e.g. an envelope missing `header` entirely): that's a hard
/// `Err`, not a per-envelope skip, because `Vec<Envelope>`/`Envelope`
/// deserialization itself fails before per-envelope classification ever
/// runs -- same as `parse_batch` today.
pub fn confirmed_envelope_bodies(raw: &str) -> anyhow::Result<Vec<String>> {
    const CONFIRMED: [&str; 5] = ["0001", "0002", "0003", "0006", "0007"];

    let value: serde_json::Value = serde_json::from_str(raw)?;
    let envelopes: Vec<serde_json::Value> = if value.is_array() {
        serde_json::from_value(value)?
    } else {
        vec![value]
    };

    envelopes
        .into_iter()
        .filter_map(|envelope| {
            let msg_type = envelope.get("header")?.get("msg_type")?.as_str()?;
            if CONFIRMED.contains(&msg_type) {
                Some(serde_json::to_string(&envelope).map_err(anyhow::Error::from))
            } else {
                None
            }
        })
        .collect()
}
```

  (The structural `Envelope`/`Header` structs already declared above this
  function in `schema.rs` are reused for the initial parse/shape check via
  `Value` navigation rather than a second `serde_json::from_value::<Envelope>`
  pass, to avoid constructing a second `Envelope` just to throw its `body`
  away — `.get("header")?.get("msg_type")?.as_str()?` is deliberately a
  `Value`-level walk, not a typed deserialize, matching this function's own
  doc comment: "never deserializes into a typed struct.")

- [ ] **Step 2: Tests**, in `schema.rs`'s existing `#[cfg(test)] mod tests`:

  - `confirmed_envelope_bodies_keeps_confirmed_types_and_drops_unknown`: a
    3-envelope array (`0001` valid shape, `0005` unknown, `0003` valid
    shape) → returns exactly 2 strings, in original order, each re-parsing
    to the same `header.msg_type` as its source envelope.
  - `confirmed_envelope_bodies_does_not_filter_on_body_shape` — **the one
    the design doc's Decision 1 rationale exists to prove**: a single
    envelope, `msg_type: "0001"`, `body: {"not_the_right_shape": true}` →
    `confirmed_envelope_bodies` returns `Ok(vec![..])` with **one** entry
    (not dropped), while a parallel assertion in the same test confirms
    `parse_batch` on the identical input returns zero messages (dropped,
    per `parse_envelope`'s existing `.ok()`-based body validation) — the two
    functions' different behavior on the exact same malformed input is the
    point of this test, so both are asserted side by side, not just one.
  - `confirmed_envelope_bodies_on_a_bare_single_envelope_object`: the same
    non-array bare-`{header,body}` shape `parse_batch`'s own
    `parses_a_single_bare_envelope_object_not_wrapped_in_an_array` test
    covers → returns exactly one string.
  - `confirmed_envelope_bodies_errors_on_a_payload_missing_header_entirely`:
    `r#"{"not_an_envelope": true}"#` → `Err`, mirroring `parse_batch`'s own
    `a_malformed_payload_produces_a_specific_field_level_error` test's
    input verbatim (same input, same "hard error, not a silent empty
    result" expectation — note the error message here is a generic serde
    `Value`-walk failure, not necessarily naming `header` the way
    `parse_batch`'s typed-struct error does; assert on `is_err()`, not on
    message content, since this function's error path is structurally
    different from `parse_batch`'s).
  - `confirmed_envelope_bodies_is_byte_faithful`: an envelope carrying an
    extra, unmodeled field inside `body` (something none of `Activation`/
    `Movement`/etc.'s structs declare) → the returned string, re-parsed as
    `serde_json::Value`, still contains that extra field — proving nothing
    is silently dropped by round-tripping through `Value` rather than a
    typed struct.

- [ ] **Step 3: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p trust-schema --all-features
cargo test -p trust-schema
```

```bash
git add crates/trust-schema/src/schema.rs
git commit -m "trust-schema: add confirmed_envelope_bodies, the movement-relay filtering primitive"
```

---

## Task 2: `crates/movement-feed` — crate scaffolding, trait, `FakeMovementFeed`

**Files:** create `crates/movement-feed/{Cargo.toml,src/lib.rs}`; modify
workspace `Cargo.toml`.

Independent of Task 1. Hoists the trait + fake, verbatim in spirit, from the
two existing near-identical copies
(`crates/trust-consumer/src/feed/mod.rs`,
`crates/full-coverage-consumer/src/feed/mod.rs`) into one shared crate — one
copy, not two, per the design doc's Decision 3.

- [ ] **Step 1: `Cargo.toml`**

```toml
[package]
name = "movement-feed"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.104"
async-trait = "0.1.92"
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
tracing = "0.1.44"

[dev-dependencies]
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "test-util"] }
```

Add `"crates/movement-feed"` to the workspace `Cargo.toml`'s `members`
list, alongside `"crates/trust-schema"`.

- [ ] **Step 2: `src/lib.rs`** — trait + fake, doc comment updated to
  describe the transport-level meaning of `commit` for a Redis Streams
  backend (per the design doc's own note: "only the doc comment's
  description of what 'commit' does at the transport level changes"):

```rust
//! `MovementFeed`: the shared trait between `crates/trust-consumer` and
//! `crates/full-coverage-consumer`'s consume loops and their transport.
//! Historically each crate hand-duplicated this trait plus its own Kafka
//! implementation (`crates/trust-consumer/src/feed/{mod,kafka}.rs`,
//! `crates/full-coverage-consumer/src/feed/{mod,kafka}.rs`) -- that
//! duplication was justified while each crate's transport was genuinely
//! per-consumer (a different Kafka `group.id` each). It stopped being
//! justified once both became structurally identical Redis Streams
//! readers of the same `movement-events` stream, differing only in which
//! named consumer group they read as -- see
//! docs/superpowers/specs/2026-09-04-movement-relay-design.md Decision 3
//! and docs/superpowers/plans/2026-09-04-movement-relay-plan.md Task 2.
//!
//! `crates/movement-relay`'s own Kafka consume loop does NOT depend on
//! this crate -- it is a producer/publisher into `movement-events`, not a
//! `MovementFeed` implementer. This crate is consumed only by the two
//! downstream Redis Streams readers.

pub mod redis_stream;

use async_trait::async_trait;

#[async_trait]
pub trait MovementFeed: Send {
    /// Returns the next batch of raw JSON message-batch bodies (each
    /// element is one Redis Stream entry's `payload` field -- the
    /// surviving envelope's raw bytes, unchanged from what `movement-relay`
    /// `XADD`ed; per `trust_schema::schema::parse_batch`'s input shape,
    /// that's normally a single bare `{header, body}` envelope object) not
    /// yet acknowledged. An empty `Vec` means "nothing new right now," not
    /// an error.
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>>;

    /// Acknowledges (`XACK`s, for the real implementation) everything
    /// returned by the most recent `next_batch` call. Only called after
    /// every message in that batch has been successfully written through
    /// downstream -- same at-least-once framing this trait has always had
    /// under Kafka: a crash between `next_batch` and `commit` means the
    /// same batch is redelivered next time (via this consumer's own
    /// pending-entries list, replayed on the next startup -- see
    /// `redis_stream::RedisStreamMovementFeed`'s own doc), which the
    /// `dedup_key` path makes safe.
    ///
    /// A `commit` with nothing received since the last one is a no-op that
    /// still returns `Ok(())`.
    async fn commit(&mut self) -> anyhow::Result<()>;
}

/// Test double for `MovementFeed` -- verbatim in spirit from the two
/// pre-existing, now-deleted copies in `trust-consumer`/
/// `full-coverage-consumer`. `committed_count` only moves for a `commit`
/// that had something to confirm, so a test can assert "this failure path
/// did not advance the feed" and mean it.
#[cfg(any(test, feature = "test-util"))]
pub struct FakeMovementFeed {
    batches: std::collections::VecDeque<Vec<String>>,
    received_since_commit: bool,
    pub committed_count: usize,
}

#[cfg(any(test, feature = "test-util"))]
impl FakeMovementFeed {
    pub fn new(batches: Vec<Vec<String>>) -> Self {
        Self {
            batches: batches.into(),
            received_since_commit: false,
            committed_count: 0,
        }
    }
}

#[cfg(any(test, feature = "test-util"))]
#[async_trait]
impl MovementFeed for FakeMovementFeed {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        let batch = self.batches.pop_front().unwrap_or_default();
        if !batch.is_empty() {
            self.received_since_commit = true;
        }
        Ok(batch)
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        if !self.received_since_commit {
            return Ok(());
        }
        self.received_since_commit = false;
        self.committed_count += 1;
        Ok(())
    }
}
```

  **Note the `#[cfg(any(test, feature = "test-util"))]` gate, not bare
  `#[cfg(test)]`**: unlike the two original copies (each private to its own
  crate, `#[cfg(test)]` was correct there), `FakeMovementFeed` now needs to
  be usable from **both** `trust-consumer`'s and `full-coverage-consumer`'s
  own `#[cfg(test)]` modules, which are a different crate than
  `movement-feed` — a plain `#[cfg(test)]` item is not exported outside the
  crate that declares it, even under `cfg(test)`, because Cargo compiles
  each crate's own test harness separately and a dependency crate is always
  compiled in **non**-test mode when pulled in as a dependency. Add a
  `test-util` feature to `movement-feed`'s `Cargo.toml`
  (`[features]\ntest-util = []`) and have `trust-consumer`/
  `full-coverage-consumer`'s `Cargo.toml` depend on it as
  `movement-feed = { path = "../movement-feed", features = ["test-util"] }`
  under `[dev-dependencies]` **in addition to** the plain
  `[dependencies]` entry (needed for the real `RedisStreamMovementFeed` in
  non-test builds) — mirrors how this repo already handles a handful of
  cross-crate test-only re-exports (confirm the exact existing precedent
  for this shape at implementation time via `grep -rn 'feature = "test-util"'`
  across the workspace; if none exists yet, this is the first, and the
  reasoning above is the one to cite in the `Cargo.toml` comment).

- [ ] **Step 3: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p movement-feed --all-features
```

(No `movement-feed`-only tests yet — `redis_stream.rs` doesn't exist until
Task 3. A bare `cargo build -p movement-feed` confirming it compiles is
this step's actual bar.)

```bash
git add crates/movement-feed Cargo.toml
git commit -m "Add crates/movement-feed: shared MovementFeed trait + FakeMovementFeed, hoisted from trust-consumer/full-coverage-consumer"
```

---

## Task 3: `crates/movement-feed` — `RedisStreamMovementFeed`

**Files:** create `crates/movement-feed/src/redis_stream.rs`; modify
`crates/movement-feed/src/lib.rs` (add `pub mod redis_stream;` — already
added in Task 2's Step 2 above), `.github/workflows/ci.yml`.

Depends on Task 2. The one genuinely new, non-trivial piece of Redis logic
in this whole plan — startup-PEL-replay-then-`>`, ack-via-`XACK`, a
periodic `XAUTOCLAIM` sweep, and the two-layer gap detection the design
doc's Decision 2 spells out. Given the complexity, this needs real
integration-test coverage against a real Redis, not just a fake.

- [ ] **Step 1: Constants and construction**

```rust
//! `RedisStreamMovementFeed`: the real, production `MovementFeed`
//! implementation from Deploy B onward -- reads the `movement-events`
//! stream `movement-relay` writes to, as one of its two fixed consumer
//! groups (`trust-consumer` or `full-coverage-consumer`).
//! See docs/superpowers/specs/2026-09-04-movement-relay-design.md
//! Decision 2 for the full reasoning; this module implements it, it does
//! not re-argue it.

use std::time::Duration;

use async_trait::async_trait;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;

use crate::MovementFeed;

const STREAM: &str = "movement-events";

pub struct RedisStreamMovementFeed {
    conn: ConnectionManager,
    group: String,
    consumer: String,
    /// Startup replay of this consumer's own pending-entries list (`0`,
    /// not `>`) happens exactly once, before the first `>` read -- see
    /// `next_batch`'s own doc. `false` once that first replay pass has
    /// returned empty (fully drained).
    replaying_pel: bool,
    /// IDs returned by the most recent `next_batch` call, held until
    /// `commit` XACKs them or they're replaced by the next call -- same
    /// receive/confirm split `KafkaMovementFeed::last_received` already
    /// established, generalized to a `Vec` since one Redis Streams read
    /// can return more than one entry per call (unlike the Kafka feed,
    /// which only ever returned one message per `next_batch`).
    pending_ack: Vec<String>,
    last_autoclaim_sweep: tokio::time::Instant,
    autoclaim_min_idle: Duration,
}

impl RedisStreamMovementFeed {
    /// `group` is one of the two fixed literals (`"trust-consumer"` /
    /// `"full-coverage-consumer"`) -- see each crate's own `main.rs` call
    /// site (Task 4). `consumer` is a fixed per-deployment name (e.g.
    /// `"trust-consumer-1"`), matching `enricher::stream::CONSUMER`'s own
    /// one-fixed-name convention and this design's own
    /// single-replica constraint (design doc Decision 2).
    pub async fn connect(
        redis_url: &str,
        group: impl Into<String>,
        consumer: impl Into<String>,
        autoclaim_min_idle: Duration,
    ) -> anyhow::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let mut conn = client.get_connection_manager().await?;
        let group = group.into();

        ensure_group(&mut conn, &group).await?;

        Ok(Self {
            conn,
            group,
            consumer: consumer.into(),
            replaying_pel: true,
            pending_ack: Vec::new(),
            last_autoclaim_sweep: tokio::time::Instant::now() - autoclaim_min_idle,
            autoclaim_min_idle,
        })
    }
}

/// Idempotent group creation, `MKSTREAM`-backed -- verbatim in spirit from
/// `crates/enricher/src/stream.rs::ensure_group`, generalized over the
/// group name (this crate serves two different group names from one
/// implementation, unlike enricher's single hardcoded `GROUP`).
async fn ensure_group(conn: &mut ConnectionManager, group: &str) -> anyhow::Result<()> {
    let result: redis::RedisResult<()> = redis::cmd("XGROUP")
        .arg("CREATE")
        .arg(STREAM)
        .arg(group)
        .arg("$")
        .arg("MKSTREAM")
        .query_async(conn)
        .await;

    match result {
        Ok(()) => Ok(()),
        Err(err) if err.to_string().contains("BUSYGROUP") => Ok(()),
        Err(err) => Err(err.into()),
    }
}
```

- [ ] **Step 2: `next_batch` — startup PEL replay, then `>`**

```rust
#[async_trait]
impl MovementFeed for RedisStreamMovementFeed {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        // Periodic XAUTOCLAIM sweep, checked once per call -- cheap
        // (skips immediately if not due) and keeps this on the same
        // "checked every loop iteration" shape every existing multi-cadence
        // main.rs loop in this repo already uses, rather than a second
        // spawned task racing this one's own Redis connection.
        if self.last_autoclaim_sweep.elapsed() >= self.autoclaim_min_idle {
            self.reclaim_stale().await?;
            self.last_autoclaim_sweep = tokio::time::Instant::now();
        }

        let id_arg = if self.replaying_pel { "0" } else { ">" };
        let reply: redis::streams::StreamReadReply = self
            .conn
            .xread_options(
                &[STREAM],
                &[id_arg],
                &redis::streams::StreamReadOptions::default()
                    .group(&self.group, &self.consumer)
                    .count(100)
                    .block(if self.replaying_pel { 0 } else { 5000 }),
            )
            .await?;

        let entries: Vec<(String, String)> = reply
            .keys
            .into_iter()
            .flat_map(|k| k.ids)
            .filter_map(|entry| {
                let payload: String = entry
                    .map
                    .get("payload")
                    .and_then(|v| redis::from_redis_value(v).ok())?;
                Some((entry.id, payload))
            })
            .collect();

        // The PEL replay pass (id `0`) returns however many pending
        // entries this consumer name left unacked last time -- possibly
        // zero (a clean prior shutdown, or a first-ever run). EITHER WAY
        // it only ever runs once: a `0`-id read that returns nothing still
        // means "no more of MY OWN old pending entries," not "no more
        // entries in the stream" (there could be plenty ahead of `>` from
        // other consumers' progress) -- switching to `>` after exactly one
        // empty (or non-empty) `0`-read is correct regardless of which.
        if self.replaying_pel {
            self.replaying_pel = false;
        }

        self.pending_ack = entries.iter().map(|(id, _)| id.clone()).collect();
        Ok(entries.into_iter().map(|(_, payload)| payload).collect())
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        if self.pending_ack.is_empty() {
            return Ok(());
        }
        let ids = std::mem::take(&mut self.pending_ack);
        let _: i64 = self.conn.xack(STREAM, &self.group, &ids).await?;
        Ok(())
    }
}
```

- [ ] **Step 3: `reclaim_stale` — `XAUTOCLAIM` sweep**, cursor-until-`"0-0"`
  loop shape copied from `enricher::stream::claim_stale`
  (`crates/enricher/src/stream.rs:88-126`), generalized over group name;
  claimed entries are simply returned to this consumer's own next
  `next_batch` read via the PEL (they become part of *this* consumer's
  pending list once claimed, so no separate "process reclaimed entries"
  path is needed — the next `next_batch` call's own `>`-mode read won't
  see them since they're not new, but the reclaim itself doesn't hand them
  back through `next_batch`'s return value either; **note this
  asymmetry explicitly**: reclaiming via `XAUTOCLAIM` re-assigns ownership
  but does not itself deliver the payload. Handle this the same way
  `redis_stream.rs`'s own `next_batch` PEL-replay does — after a claim, the
  claimed IDs are now this consumer's own pending entries, retrievable via
  a `0`-id read exactly like startup replay. **Concretely: `reclaim_stale`
  sets `self.replaying_pel = true` after a non-empty claim**, so the next
  `next_batch` call re-enters PEL-replay mode and picks the reclaimed
  entries up through the exact same code path startup replay already
  uses — no separate delivery mechanism needed):

```rust
async fn reclaim_stale(&mut self) -> anyhow::Result<()> {
    let mut cursor = "0-0".to_string();
    let mut claimed_any = false;
    loop {
        let reply: redis::streams::StreamAutoClaimReply = self
            .conn
            .xautoclaim_options(
                STREAM,
                &self.group,
                &self.consumer,
                self.autoclaim_min_idle.as_millis() as u64,
                cursor,
                redis::streams::StreamAutoClaimOptions::default().count(100),
            )
            .await?;

        if !reply.claimed.is_empty() {
            claimed_any = true;
        }
        if reply.next_stream_id == "0-0" {
            break;
        }
        cursor = reply.next_stream_id;
    }
    if claimed_any {
        // Re-enter PEL-replay mode so the next `next_batch` call picks
        // these up through the same `id = "0"` path startup replay uses --
        // see this function's own doc note above for why XAUTOCLAIM alone
        // doesn't deliver payloads.
        self.replaying_pel = true;
    }
    Ok(())
}
```

- [ ] **Step 4: Gap detection — `check_gap`**, the design doc's "definitive
  detection" mechanism (compare `last-delivered-id` against the stream's
  oldest retained entry). Returns an `Option<GapInfo>` for the caller
  (Task 4's `trust-consumer`/`full-coverage-consumer` main loop) to log and
  count with its own crate-specific metric name — kept generic here since
  the mechanism is identical for both callers, only the response differs
  (design doc Decision 2's own "both named explicitly" requirement is
  satisfied at the call site, Task 4, not here):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GapInfo {
    pub group_last_delivered_id: String,
    pub stream_first_entry_id: String,
}

impl RedisStreamMovementFeed {
    /// Compares this group's `last-delivered-id` (via `XINFO GROUPS`)
    /// against the stream's current oldest retained entry (via `XINFO
    /// STREAM`'s `first-entry`). `Some(GapInfo)` means entries between
    /// those two IDs were trimmed (`MAXLEN`) before this group ever read
    /// them -- a provable gap, not a suspicion. Call on the same cadence
    /// this crate's caller already reloads its other periodic state (see
    /// Task 4) -- cheap, two Redis round-trips, no new polling loop.
    pub async fn check_gap(&mut self) -> anyhow::Result<Option<GapInfo>> {
        let groups: Vec<redis::Value> = redis::cmd("XINFO")
            .arg("GROUPS")
            .arg(STREAM)
            .query_async(&mut self.conn)
            .await?;
        let Some(last_delivered_id) = find_group_field(&groups, &self.group, "last-delivered-id")?
        else {
            return Ok(None); // group doesn't exist yet -- nothing to compare.
        };

        let stream_info: Vec<redis::Value> = redis::cmd("XINFO")
            .arg("STREAM")
            .arg(STREAM)
            .query_async(&mut self.conn)
            .await?;
        let Some(first_entry_id) = find_stream_first_entry_id(&stream_info)? else {
            return Ok(None); // empty stream -- nothing trimmed yet.
        };

        if stream_id_less_than(&last_delivered_id, &first_entry_id) {
            Ok(Some(GapInfo {
                group_last_delivered_id: last_delivered_id,
                stream_first_entry_id: first_entry_id,
            }))
        } else {
            Ok(None)
        }
    }
}

/// Stream IDs are `<ms>-<seq>` pairs, monotonic and directly comparable as
/// a pair of integers (never as a bare string -- `"9-0" < "10-0"`
/// lexicographically is false but numerically true, so this must NOT be a
/// plain string `<` comparison).
fn stream_id_less_than(a: &str, b: &str) -> bool {
    fn parts(id: &str) -> (u64, u64) {
        let mut it = id.splitn(2, '-');
        let ms = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        let seq = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        (ms, seq)
    }
    parts(a) < parts(b)
}
```

  (`find_group_field`/`find_stream_first_entry_id` are small private
  helpers walking `XINFO`'s flat alternating-field-name/value reply shape —
  same parsing shape `enricher::stream::group_lag` already established for
  `XINFO GROUPS`; `XINFO STREAM`'s `first-entry` field is itself a 2-element
  array `[id, fields]`, so `find_stream_first_entry_id` extracts index `0`
  of that nested array. Implement directly against a real Redis's actual
  reply shape during Task 3's own test-writing pass — confirm the exact
  nesting via `redis-cli XINFO STREAM movement-events` against a local
  Redis with at least one entry, not assumed from documentation alone,
  per this repo's "no invented API details" convention.)

- [ ] **Step 5: `#[ignore]`-gated integration tests against a real Redis**

  New `#[cfg(test)] mod redis_tests` in `redis_stream.rs`, each function
  `#[ignore = "needs REDIS_URL"]`, connecting to
  `std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".into())`,
  using a uniquely-named stream/group per test run (e.g.
  `format!("movement-events-test-{}", uuid-or-timestamp)`, since this
  module's `STREAM`/group constants are fixed literals in production code —
  either add a test-only constructor that takes an override stream name, or
  `redis::cmd("DEL")` the fixed test keys at the start/end of each test;
  pick whichever this crate's own implementation makes cleaner, but every
  test must clean up after itself unconditionally, mirroring
  `station_stats.rs::db_tests`'s "delete the fixture row at the end" rule):

  - `startup_replay_delivers_a_prior_consumers_unacked_entry`: XADD one
    entry, read it via `next_batch` (PEL replay picks it up on a **fresh**
    `RedisStreamMovementFeed::connect` for the same group/consumer, since
    it was never acked) — WITHOUT acking, drop the feed, reconnect a new
    `RedisStreamMovementFeed` for the same `(group, consumer)`, call
    `next_batch` again → the same entry is delivered again (this is the
    direct regression test for the stuck-Activation bug the design doc's
    "Why this exists" section names as a first-class goal).
  - `commit_acks_only_after_being_called_not_on_receipt`: XADD one entry,
    `next_batch`, then (without calling `commit`) inspect the group's PEL
    directly (`XPENDING`) → the entry is still pending. Then call `commit`,
    re-check `XPENDING` → empty.
  - `xack_ordering_matches_kafka_feeds_never_commit_before_confirmed_rule`:
    mirrors `trust-consumer/src/main.rs`'s own
    `a_failed_post_does_not_commit_the_batch` test's *intent*, but proves
    it one layer lower — `next_batch` twice without ever calling `commit`
    in between → the second call does NOT re-deliver the first batch's
    entries via `>` (they're legitimately "delivered, not yet acked," which
    is correct — only a **new consumer/reconnect** replays via PEL, an
    **already-connected** feed must not re-deliver its own in-flight batch
    to itself). This test's job is specifically to confirm `next_batch`
    never accidentally reads its own undelivered entries twice from the
    live `>` cursor.
  - `xautoclaim_reclaims_an_entry_stuck_under_a_different_consumer_name`:
    XADD one entry, deliver it to consumer name `"dead-consumer"` (a
    throwaway `RedisStreamMovementFeed` connected with that name, read via
    `next_batch`, then dropped without acking — simulating a crashed pod
    that never restarts under the same name), then connect a **second**
    `RedisStreamMovementFeed` under a **different** consumer name
    (`"live-consumer"`) with a short `autoclaim_min_idle` (e.g.
    `Duration::from_millis(50)`, then sleep past it) → its own `next_batch`
    call (which internally checks/runs the sweep) reclaims and delivers the
    entry.
  - `check_gap_detects_a_trimmed_range_the_group_never_read`: XADD several
    entries, deliver+ack a few via `next_batch`/`commit` so the group's
    `last-delivered-id` advances, then `XADD ... MAXLEN 1 *` (force
    aggressive trimming past what the group has read) enough additional
    entries to trim away everything at or before the group's
    `last-delivered-id` → `check_gap()` returns `Some(GapInfo{..})` with the
    two IDs populated and `group_last_delivered_id` provably older than
    `stream_first_entry_id` (assert via the same `stream_id_less_than`
    logic, re-derived independently in the test rather than calling the
    private helper, to avoid the test validating itself against its own
    implementation).
  - `check_gap_reports_none_when_the_group_is_caught_up`: XADD and
    fully drain+ack via `next_batch`/`commit`, no trimming beyond what's
    been read → `check_gap()` returns `None`.

- [ ] **Step 6: CI wiring**

  `.github/workflows/ci.yml`'s `rust-test` job: add a `redis:` entry under
  `services:`, sibling to the existing `postgres:` block:

```yaml
      redis:
        image: redis:7
        ports:
          - 6379:6379
        options: >-
          --health-cmd "redis-cli ping"
          --health-interval 10s
          --health-timeout 5s
          --health-retries 5
```

  and, under `env:`, add `REDIS_URL: redis://localhost:6379`. After the
  existing `cargo test -p api -p aggregator -- --ignored --test-threads=1`
  step, add:

```yaml
      - name: cargo test -p movement-feed -- --ignored (Redis-backed tests)
        run: cargo test -p movement-feed -- --ignored --test-threads=1
```

- [ ] **Step 7: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p movement-feed --all-features
REDIS_URL=redis://localhost:6379 cargo test -p movement-feed -- --ignored --test-threads=1
```

```bash
git add crates/movement-feed/src/redis_stream.rs .github/workflows/ci.yml
git commit -m "movement-feed: add RedisStreamMovementFeed (PEL replay, XACK, XAUTOCLAIM sweep, gap detection)"
```

---

## Task 4: `trust-consumer` and `full-coverage-consumer` — dual-backend `MovementFeed`

**Files:** modify `crates/trust-consumer/{Cargo.toml,src/{config.rs,main.rs,feed/mod.rs}}`,
`crates/full-coverage-consumer/{Cargo.toml,src/{config.rs,main.rs,feed/mod.rs}}`.

Depends on Tasks 2–3. This is the task that makes Deploy B's B3 step a
config flip, not a code change — see "Judgment calls," item 5, above.

- [ ] **Step 1: `Cargo.toml` (both crates)**

  Add `movement-feed = { path = "../movement-feed" }` under
  `[dependencies]`; add `movement-feed = { path = "../movement-feed",
  features = ["test-util"] }` under `[dev-dependencies]` (per Task 2's own
  note on why `FakeMovementFeed` needs the feature-gated re-export). Add
  `clap`'s `derive` feature already covers `ValueEnum` — no new dependency
  needed for the backend enum (Step 2).

- [ ] **Step 2: `feed/mod.rs` (both crates) — shrink to a re-export**

```rust
//! Re-exports the shared `MovementFeed` trait/fake from `movement-feed`,
//! plus this crate's own Kafka implementation (`kafka.rs`, unchanged --
//! scheduled for deletion in Deploy C, see
//! docs/superpowers/plans/2026-09-04-movement-relay-plan.md Task 13/14,
//! NOT this task).

pub mod kafka;

pub use movement_feed::MovementFeed;
#[cfg(test)]
pub use movement_feed::FakeMovementFeed;
```

  `kafka.rs` itself is **untouched** in this task — it keeps working exactly
  as it does today, since `movement_feed_backend` (Step 3) defaults to
  `kafka` and this task must not change default production behavior.

- [ ] **Step 3: `config.rs` (both crates) — the backend toggle + Redis
      connection fields**

  Add, near the existing Kafka fields:

```rust
#[derive(Debug, Clone, Copy, clap::ValueEnum, PartialEq, Eq)]
pub enum MovementFeedBackend {
    /// Today's production default -- a direct Kafka consumer via
    /// `feed::kafka::KafkaMovementFeed`. Unchanged behavior from before
    /// this plan.
    Kafka,
    /// The new Redis Streams reader
    /// (`movement_feed::redis_stream::RedisStreamMovementFeed`), reading
    /// what `movement-relay` publishes. Selected only once
    /// docs/superpowers/specs/2026-09-04-movement-relay-design.md's
    /// Deploy B has moved the real Kafka credential over to
    /// `movement-relay` -- selecting this BEFORE that happens means this
    /// crate simply reads nothing (the stream/group exist but
    /// `movement-relay` was never enabled to publish into them), not a
    /// crash -- a safe, if useless, misconfiguration.
    RedisStream,
}

/// Which transport this crate's `MovementFeed` uses. See
/// docs/superpowers/plans/2026-09-04-movement-relay-plan.md, "Judgment
/// calls," item 5, for why this exists: Deploy A ships this defaulting to
/// `kafka` (zero behavior change); Deploy B's B3 step flips it to
/// `redis-stream` via a Helm value, with no new code merge at cutover
/// time.
#[arg(long, env, value_enum, default_value_t = MovementFeedBackend::Kafka)]
pub movement_feed_backend: MovementFeedBackend,

/// Only read when `movement_feed_backend = redis-stream`. Always
/// required regardless of the selected backend -- Redis is already an
/// always-on chart-level dependency (`redis.enabled: true`), so requiring
/// this unconditionally is simpler than making it conditionally required,
/// and costs nothing when unused under the `kafka` backend.
#[arg(long, env, default_value = "redis://redis:6379")]
pub redis_url: String,

/// How long an entry may sit unacked in this consumer's own pending-entries
/// list before `RedisStreamMovementFeed`'s periodic sweep reclaims it.
/// Sized small relative to `enricher`'s own `reclaimMinIdleSecs` (1000s) --
/// see docs/superpowers/specs/2026-09-04-movement-relay-design.md
/// Decision 2's own note: this crate's cycle latency (consume -> derive ->
/// POST to api -> ack) should be sub-second in the healthy case, unlike
/// enricher's slower LLM-call latency.
#[arg(long, env, default_value_t = 30)]
pub redis_autoclaim_min_idle_secs: u64,
```

  (`full-coverage-consumer`'s own copy of these three fields is identical
  in shape; its `Config`'s existing test fixture `base_config` (Step 4)
  needs the three new fields added, defaulted the same way Task 4's own
  `movement_feed_backend: MovementFeedBackend::Kafka` keeps every existing
  test's behavior unchanged.)

- [ ] **Step 4: `main.rs` (both crates) — construct the selected backend**

  `trust-consumer/src/main.rs`, replacing the single
  `KafkaMovementFeed::connect(&config, connection_state)?` call site:

```rust
let mut feed: Box<dyn MovementFeed> = match config.movement_feed_backend {
    config::MovementFeedBackend::Kafka => {
        Box::new(feed::kafka::KafkaMovementFeed::connect(&config, connection_state)?)
    }
    config::MovementFeedBackend::RedisStream => Box::new(
        movement_feed::redis_stream::RedisStreamMovementFeed::connect(
            &config.redis_url,
            "trust-consumer",
            "trust-consumer-1",
            Duration::from_secs(config.redis_autoclaim_min_idle_secs),
        )
        .await?,
    ),
};
```

  (`full-coverage-consumer`'s own call site is identical apart from the
  fixed group/consumer-name literals: `"full-coverage-consumer"` /
  `"full-coverage-consumer-1"`.) `run_cycle`'s existing generic bound,
  `F: MovementFeed`, already accepts `Box<dyn MovementFeed>` since
  `MovementFeed: Send` is already object-safe (no associated types, no
  `Self`-returning methods other than the already-`async_trait`-boxed
  ones) — confirm this compiles as-is at implementation time; if
  `async_trait`'s expansion needs an explicit `&mut Box<dyn MovementFeed>`
  deref somewhere `run_cycle` currently takes `&mut F` directly, that's a
  mechanical fix, not a design change.

  **Gap-check wiring** (both crates, only meaningful under the
  `RedisStream` backend — a no-op the loop skips entirely under `Kafka`):
  add a `redis_gap_check_interval` timer to each crate's existing
  multi-cadence loop (same shape as `reference_reload_secs`/
  `stanox_crs_reload_secs`), calling `RedisStreamMovementFeed::check_gap`
  **only reachable when the concrete type is known** — since `feed` is now
  a `Box<dyn MovementFeed>`, `check_gap` (a `RedisStreamMovementFeed`-only
  inherent method, not part of the `MovementFeed` trait — deliberately,
  since `KafkaMovementFeed` has no analog) isn't callable through the
  trait object. Resolve this by **not** boxing the feed as `Box<dyn
  MovementFeed>` for this purpose; instead, keep the two backend branches
  as a small local enum wrapping both concrete types
  (`enum ActiveFeed { Kafka(KafkaMovementFeed), RedisStream(RedisStreamMovementFeed) }`)
  implementing `MovementFeed` itself by delegating each method to whichever
  variant is active (a manual, mechanical `match` in each of `next_batch`/
  `commit`), with one additional inherent method,
  `async fn check_gap(&mut self) -> anyhow::Result<Option<movement_feed::redis_stream::GapInfo>>`,
  that returns `Ok(None)` immediately for the `Kafka` variant and delegates
  for the `RedisStream` variant. This is a few more lines than `Box<dyn
  MovementFeed>` but keeps the gap-check reachable without a downcast —
  prefer this shape over `Box<dyn Any>`-based downcasting, which this
  repo's existing code never does anywhere else.

  On `Some(gap)`, each crate logs and increments its **own**
  crate-specific metric, per the design doc's explicit "both services'
  exposure differs and both are named explicitly" requirement:
  - `trust-consumer`: `tracing::error!(last_delivered = %gap.group_last_delivered_id, new_first_entry = %gap.stream_first_entry_id, "movement-events stream gap detected: some events between these IDs were trimmed before trust-consumer ever read them -- any Activation/Movement in that range is silently lost, possibly stranding a pin in resolution_status='pending' forever")`
    + `metrics::counter!(common::metrics::metric_name("trust_consumer_stream_gap_detected_total")).increment(1)`.
    (`trust-consumer` doesn't currently install `common::metrics` at all —
    confirm via `grep -rn metrics:: crates/trust-consumer/src/` at
    implementation time; if it genuinely has no metrics installer today,
    add one, mirroring `full-coverage-consumer`'s own `config.metrics_enabled`
    + `common::metrics::install(config.metrics_port)?` pattern exactly,
    since this counter needs somewhere real to be scraped from — this is a
    real, additive piece of this task, not assumed already present.)
  - `full-coverage-consumer`: same log shape, its own message text noting
    the shadow-mode `SampleStats` accuracy risk (per the design doc's own
    wording), `metrics::counter!(common::metrics::metric_name("full_coverage_consumer_stream_gap_detected_total")).increment(1)`.

- [ ] **Step 5: Tests**

  - Both crates' existing `#[cfg(test)] mod tests` (in `main.rs`,
    exercising `run_cycle`/the main loop against `FakeMovementFeed`) —
    confirm they still pass unmodified in assertions after the
    `Box<dyn MovementFeed>`-or-`ActiveFeed`-enum change (only construction
    syntax changes, not test bodies).
  - `config.rs`: a unit test asserting `movement_feed_backend` defaults to
    `Kafka` when unset (parse a minimal arg list via `Config::parse_from`,
    confirm the field) — the concrete regression test for "Deploy A changes
    nothing about default production behavior."
  - A unit test for the `ActiveFeed` enum's `check_gap` delegation:
    construct the `Kafka` variant (with a throwaway/never-connected
    `KafkaMovementFeed` is awkward without a real broker — instead, test
    this at the type level via a small local trait object substitution, or
    simply confirm via code review + the `Kafka => Ok(None)` arm's
    triviality that this doesn't need a runtime test; note the decision
    either way in the PR, don't silently skip it without saying so).

- [ ] **Step 6: Verify and commit**

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test -p trust-consumer -p full-coverage-consumer
```

```bash
git add crates/trust-consumer crates/full-coverage-consumer
git commit -m "trust-consumer, full-coverage-consumer: add a config-selectable Redis Streams MovementFeed backend (defaults to kafka, unchanged behavior)"
```

---

## Task 5: `crates/movement-relay` — crate scaffolding

**Files:** create `crates/movement-relay/{Cargo.toml,src/{config.rs,health.rs}}`;
modify workspace `Cargo.toml`.

Depends on nothing from Tasks 1–4 directly (parallel-safe once Task 1
lands, since it needs `trust-schema`), but sequenced after them here for
narrative flow — implementers may run Tasks 5–8 in parallel with 2–4 if
convenient, both only converge at Task 9 (Helm).

- [ ] **Step 1: `Cargo.toml`**

```toml
[package]
name = "movement-relay"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.104"
async-trait = "0.1.92"
axum = { version = "0.8.9", features = ["http2"] }
clap = { version = "4.6.6", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
metrics = "0.24"
rdkafka = { version = "0.39.0", features = ["cmake-build", "ssl", "sasl"] }
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
serde_json = "1.0.151"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "time"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
trust-schema = { path = "../trust-schema" }

[dev-dependencies]
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "test-util"] }
```

  (No `reqwest`/`common::oauth_client` — `movement-relay` never calls
  `api` over HTTP, unlike every poller/consumer in this repo; it only
  speaks Kafka in and Redis out. No `serde`/`chrono` beyond what
  `serde_json`/`trust-schema` already pull transitively — confirm at
  implementation time whether a direct `serde` dep is needed for anything
  in `config.rs`, likely not.)

  Add `"crates/movement-relay"` to the workspace `Cargo.toml`'s `members`
  list.

- [ ] **Step 2: `config.rs`** — mirrors `trust-consumer/src/config.rs`'s
  shape for the Kafka fields (same GAP-flagged no-default posture for
  `kafka_topic`/`kafka_sasl_mechanism`, same doc-comment reasoning),
  substituting the API/OAuth/reference-reload fields (not needed here) for
  Redis output fields:

```rust
use clap::Parser;

/// CLI/env configuration for `movement-relay` -- the sole real Kafka
/// client against RDM's Train Movements product from Deploy B onward. See
/// docs/superpowers/specs/2026-09-04-movement-relay-design.md and
/// docs/superpowers/plans/2026-09-04-movement-relay-plan.md.
#[derive(Debug, Parser)]
pub struct Config {
    /// GAP: unconfirmed hostname until Deploy B's real credential is in
    /// hand -- same posture as trust-consumer/src/config.rs's own
    /// identical field.
    #[arg(long, env)]
    pub kafka_brokers: String,
    #[arg(long, env)]
    pub kafka_topic: String,
    /// The one real, RDM-issued group -- `SC-c4d90f8e-...` in production,
    /// per the design doc's "Why this exists" section. Deliberately no
    /// default: unlike trust-consumer's own kafka_consumer_group (which
    /// DOES have a sensible per-deployment default,
    /// "distant-signal-trust-consumer"), this crate's group id is a fixed,
    /// externally-issued, unforgeable identity -- guessing wrong here is
    /// worse than refusing to start.
    #[arg(long, env)]
    pub kafka_consumer_group: String,
    #[arg(long, env)]
    pub kafka_sasl_username: String,
    #[arg(long, env)]
    pub kafka_sasl_password: String,
    #[arg(long, env)]
    pub kafka_sasl_mechanism: String,

    #[arg(long, env, default_value = "redis://redis:6379")]
    pub redis_url: String,

    #[arg(long, env, default_value = "0.0.0.0:8083")]
    pub health_bind_url: String,
    #[arg(long, env, default_value_t = 9094)]
    pub metrics_port: u16,
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
    /// How often the leading-indicator lag gauge (Task 7) polls `XINFO
    /// GROUPS` for both downstream groups. UNRESEARCHED starting figure,
    /// same posture as every other first-guess cadence constant in this
    /// codebase (see trust-consumer/src/config.rs's own
    /// stanox_crs_reload_secs comment).
    #[arg(long, env, default_value_t = 30)]
    pub stream_lag_poll_secs: u64,
}
```

- [ ] **Step 3: `health.rs`** — deliberately **not** a verbatim copy of
  `trust-consumer/src/health.rs`. Per the design doc's Decision 5, this
  crate's readiness must mean "joined the Kafka consumer group and was
  assigned partitions," not merely "polled at least one message" — a
  looser signal that could report not-ready during a genuine feed lull.
  Implemented via `rdkafka`'s rebalance callback, not the message-arrival
  flag `trust-consumer`/`full-coverage-consumer` use:

```rust
//! Readiness for `movement-relay` means "confirmed Kafka partition
//! assignment," NOT "the HTTP server answered" and NOT "at least one
//! message has arrived" (contrast with `trust-consumer`/
//! `full-coverage-consumer`'s own `ConnectionState`, which flips on
//! message arrival -- see
//! docs/superpowers/specs/2026-09-04-movement-relay-design.md Decision 5
//! for why the two crates deliberately differ here: this crate's
//! readiness is the exact gate Deploy B's rollout safety depends on
//! (whether the NEW pod has truly taken over group membership before the
//! OLD one is torn down), which message-arrival alone doesn't prove during
//! a genuine lull. Do not "fix" this inconsistency by making the two
//! match -- it is deliberate.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::StatusCode;
use axum::routing::get;
use rdkafka::ClientContext;
use rdkafka::consumer::{ConsumerContext, Rebalance};

pub type ReadyState = Arc<AtomicBool>;

/// `rdkafka::ClientConfig::create_with_context` target -- flips `ready`
/// true on a non-empty partition assignment (`post_rebalance`'s `Assign`
/// variant), false on any revoke/error path, independent of whether a
/// message has arrived on the assigned partitions yet.
pub struct RelayContext {
    pub ready: ReadyState,
}

impl ClientContext for RelayContext {}

impl ConsumerContext for RelayContext {
    fn post_rebalance(&self, rebalance: &Rebalance) {
        match rebalance {
            Rebalance::Assign(partitions) if !partitions.elements().is_empty() => {
                self.ready.store(true, Ordering::Relaxed);
                tracing::info!(
                    partitions = partitions.elements().len(),
                    "movement-relay: Kafka partition assignment confirmed; readiness now true"
                );
            }
            Rebalance::Revoke(_) => {
                self.ready.store(false, Ordering::Relaxed);
                tracing::warn!("movement-relay: Kafka partitions revoked; readiness now false");
            }
            Rebalance::Error(err) => {
                self.ready.store(false, Ordering::Relaxed);
                tracing::error!(error = ?err, "movement-relay: Kafka rebalance error; readiness now false");
            }
            _ => {}
        }
    }
}

pub fn spawn(bind_url: String, ready: ReadyState) {
    tokio::spawn(async move {
        let app = axum::Router::new().route("/healthz", get(move || healthz(Arc::clone(&ready))));
        let listener = match tokio::net::TcpListener::bind(&bind_url).await {
            Ok(listener) => listener,
            Err(err) => {
                tracing::error!(error = ?err, bind_url, "failed to bind health endpoint");
                return;
            }
        };
        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!(error = ?err, "health endpoint server stopped");
        }
    });
}

async fn healthz(ready: ReadyState) -> (StatusCode, &'static str) {
    if ready.load(Ordering::Relaxed) {
        (StatusCode::OK, "partitions assigned")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "no confirmed partition assignment")
    }
}
```

  Liveness stays the simple always-answers-if-alive shape every other
  crate's `/healthz` already provides via the same endpoint (Task 9's Helm
  template points `livenessProbe` at the same `/healthz` route, per the
  design doc's Decision 5 — the liveness/readiness *split* here is at the
  Kubernetes probe-config level, not two different HTTP routes; confirm
  this is consistent with `trust-consumer-deployment.yaml`'s own existing
  pattern, which also points both probes at the same `/healthz`).

- [ ] **Step 4: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p movement-relay --all-features
```

```bash
git add crates/movement-relay Cargo.toml
git commit -m "Add crates/movement-relay scaffolding: config.rs, rebalance-based health.rs"
```

---

## Task 6: `crates/movement-relay` — raw Kafka source

**Files:** create `crates/movement-relay/src/kafka_source.rs`; modify
`crates/movement-relay/src/main.rs` (module declaration only, at this
stage).

Depends on Task 5. **Deliberately does not depend on `movement-feed`** —
per the design doc's own Decision 3 tree sketch ("`movement-relay`'s OWN
Kafka consume loop does NOT depend on this crate"), since `movement-feed`
is scoped to the two downstream Redis Streams *readers*, and
`movement-relay` is a raw-Kafka-payload consumer / Redis *producer*, a
structurally different role. This means `kafka_source.rs` is
**structurally near-identical to `trust-consumer/src/feed/kafka.rs`**,
duplicated rather than shared — a small, deliberate, spec-directed
exception to this repo's usual DRY instinct, justified by crate-boundary
purity (`movement-feed` staying Redis-only) rather than an oversight; worth
noting in the file's own doc comment so a future reader doesn't "fix" the
duplication by merging it into `movement-feed`. The duplicated shape is
temporary in one direction only: `trust-consumer`'s own copy is deleted in
Deploy C (Task 13); this crate's copy is permanent.

- [ ] **Step 1: Trait + real implementation**

```rust
//! Raw Kafka source for movement-relay. Structurally close to
//! `trust-consumer/src/feed/kafka.rs` (same ClientConfig shape, same
//! store-then-commit offset discipline) but deliberately NOT shared via
//! `crates/movement-feed` -- see
//! docs/superpowers/plans/2026-09-04-movement-relay-plan.md Task 6 for why.
//! Returns RAW record payloads (unclassified) -- classification into
//! confirmed/unknown message types happens in `main.rs` via
//! `trust_schema::schema::confirmed_envelope_bodies`, not here.

use async_trait::async_trait;
use rdkafka::ClientConfig;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::Message;

use crate::config::Config;
use crate::health::{ReadyState, RelayContext};

#[async_trait]
pub trait RawKafkaSource: Send {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>>;
    async fn commit(&mut self) -> anyhow::Result<()>;
}

pub struct KafkaRawSource {
    consumer: StreamConsumer<RelayContext>,
    last_received: Option<(String, i32, i64)>,
}

impl KafkaRawSource {
    pub fn connect(config: &Config, ready: ReadyState) -> anyhow::Result<Self> {
        let context = RelayContext { ready };
        let consumer: StreamConsumer<RelayContext> = ClientConfig::new()
            .set("bootstrap.servers", &config.kafka_brokers)
            .set("group.id", &config.kafka_consumer_group)
            .set("security.protocol", "SASL_SSL")
            .set("sasl.mechanisms", &config.kafka_sasl_mechanism)
            .set("sasl.username", &config.kafka_sasl_username)
            .set("sasl.password", &config.kafka_sasl_password)
            .set("enable.auto.commit", "false")
            .set("enable.auto.offset.store", "false")
            .create_with_context(context)?;

        consumer.subscribe(&[&config.kafka_topic])?;

        Ok(Self { consumer, last_received: None })
    }
}

#[async_trait]
impl RawKafkaSource for KafkaRawSource {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        let message = self.consumer.recv().await?;
        let payload = message
            .payload()
            .ok_or_else(|| anyhow::anyhow!("empty Kafka message payload"))?;
        let batch = String::from_utf8_lossy(payload).into_owned();
        self.last_received = Some((message.topic().to_string(), message.partition(), message.offset()));
        Ok(vec![batch])
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        let Some((topic, partition, offset)) = self.last_received.as_ref() else {
            return Ok(());
        };
        self.consumer.store_offset(topic, *partition, *offset)?;
        self.consumer.commit_consumer_state(rdkafka::consumer::CommitMode::Async)?;
        self.last_received = None;
        Ok(())
    }
}
```

  (No `connection_state`-flavored `AtomicBool` flip here — readiness is
  entirely owned by `RelayContext`'s rebalance callback, Task 5, not by
  this module's `next_batch`/`Err` path the way `trust-consumer`'s
  `KafkaMovementFeed` does it. This is the one structural divergence from
  `trust-consumer/src/feed/kafka.rs` beyond the return-shape difference —
  worth its own one-line comment at the `connect` call site.)

- [ ] **Step 2: `FakeRawSource` test double**, same receive/confirm-split
  shape as `movement-feed::FakeMovementFeed`:

```rust
#[cfg(test)]
pub struct FakeRawSource {
    batches: std::collections::VecDeque<Vec<String>>,
    received_since_commit: bool,
    pub committed_count: usize,
}

#[cfg(test)]
impl FakeRawSource {
    pub fn new(batches: Vec<Vec<String>>) -> Self {
        Self { batches: batches.into(), received_since_commit: false, committed_count: 0 }
    }
}

#[cfg(test)]
#[async_trait]
impl RawKafkaSource for FakeRawSource {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        let batch = self.batches.pop_front().unwrap_or_default();
        if !batch.is_empty() {
            self.received_since_commit = true;
        }
        Ok(batch)
    }
    async fn commit(&mut self) -> anyhow::Result<()> {
        if !self.received_since_commit {
            return Ok(());
        }
        self.received_since_commit = false;
        self.committed_count += 1;
        Ok(())
    }
}
```

- [ ] **Step 3: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p movement-relay --all-features
```

```bash
git add crates/movement-relay/src/kafka_source.rs
git commit -m "movement-relay: add the raw Kafka source (KafkaRawSource/FakeRawSource)"
```

---

## Task 7: `crates/movement-relay` — consume → classify → `XADD` → commit loop

**Files:** create `crates/movement-relay/src/{event_sink.rs,main.rs}`.

Depends on Tasks 1 (`confirmed_envelope_bodies`), 5, 6. The core of this
crate.

- [ ] **Step 1: `event_sink.rs` — `EventSink` trait + real + fake**,
  mirroring `run_cycle`'s existing `F: MovementFeed, P: AsyncFnOnce`
  closure-injection pattern from `trust-consumer/src/main.rs`, applied to
  the producer side (a trait here rather than a bare closure, since a real
  implementation needs to hold a live `ConnectionManager` across calls,
  unlike a stateless HTTP POST closure):

```rust
//! `EventSink`: the one thing `movement-relay`'s main loop needs from
//! Redis -- XADD-ing surviving envelopes into `movement-events`. Kept as
//! a trait (not inlined into main.rs) so tests can substitute a
//! `FakeEventSink` -- this repo's established "no wiremock, use a fake
//! trait impl" convention (see e.g.
//! crates/trust-consumer/src/feed/mod.rs::FakeMovementFeed), applied here
//! on the producer side for the first time in this codebase.

use async_trait::async_trait;
use redis::AsyncCommands;
use redis::aio::ConnectionManager;

const STREAM: &str = "movement-events";
const MAXLEN: usize = 500_000; // see the design doc's Decision 2 sizing rationale.

#[async_trait]
pub trait EventSink: Send {
    /// XADDs one surviving envelope. `msg_type` is the redundant
    /// introspection field (Decision 2's field-layout choice);
    /// `payload` is the envelope's own raw JSON bytes, unchanged.
    async fn publish(&mut self, msg_type: &str, payload: &str) -> anyhow::Result<()>;
}

pub struct RedisEventSink {
    conn: ConnectionManager,
}

impl RedisEventSink {
    pub async fn connect(redis_url: &str) -> anyhow::Result<Self> {
        let client = redis::Client::open(redis_url)?;
        Ok(Self { conn: client.get_connection_manager().await? })
    }
}

#[async_trait]
impl EventSink for RedisEventSink {
    async fn publish(&mut self, msg_type: &str, payload: &str) -> anyhow::Result<()> {
        let _: String = redis::cmd("XADD")
            .arg(STREAM)
            .arg("MAXLEN")
            .arg("~")
            .arg(MAXLEN)
            .arg("*")
            .arg("payload")
            .arg(payload)
            .arg("msg_type")
            .arg(msg_type)
            .query_async(&mut self.conn)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[derive(Default)]
pub struct FakeEventSink {
    pub published: Vec<(String, String)>,
    pub fail_next: bool,
}

#[cfg(test)]
#[async_trait]
impl EventSink for FakeEventSink {
    async fn publish(&mut self, msg_type: &str, payload: &str) -> anyhow::Result<()> {
        if self.fail_next {
            self.fail_next = false;
            return Err(anyhow::anyhow!("simulated publish failure"));
        }
        self.published.push((msg_type.to_string(), payload.to_string()));
        Ok(())
    }
}
```

  (`msg_type` is re-derived cheaply from the same `Value` walk
  `confirmed_envelope_bodies` already did internally — rather than have
  `main.rs` re-parse each surviving payload string a second time just to
  extract it, change `confirmed_envelope_bodies`'s return type from
  `Vec<String>` to `Vec<(String, String)>` (`(msg_type, payload)` pairs).
  **This is a real, small deviation from Task 1's own signature as
  originally sketched — apply it there instead of re-parsing here**: go
  back and update Task 1's `confirmed_envelope_bodies` to return
  `anyhow::Result<Vec<(String, String)>>`, and update its own tests
  accordingly (`.0` for msg_type, `.1` for payload) before starting this
  task. Note this explicitly in Task 1's own commit if Task 7 is
  implemented after Task 1 has already merged with the `Vec<String>`
  signature — a follow-up commit adjusting it is fine, just don't leave
  two inconsistent shapes.)

- [ ] **Step 2: `main.rs` — the loop**

```rust
mod config;
mod event_sink;
mod health;
mod kafka_source;

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use clap::Parser;
use config::Config;
use event_sink::{EventSink, RedisEventSink};
use kafka_source::{KafkaRawSource, RawKafkaSource};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }

    let ready: health::ReadyState = Arc::new(AtomicBool::new(false));
    health::spawn(config.health_bind_url.clone(), Arc::clone(&ready));

    let mut source = KafkaRawSource::connect(&config, ready)?;
    let mut sink = RedisEventSink::connect(&config.redis_url).await?;

    tokio::spawn(stream_lag_loop(
        config.redis_url.clone(),
        Duration::from_secs(config.stream_lag_poll_secs),
    ));

    loop {
        match run_cycle(&mut source, &mut sink).await {
            Cycle::Committed => {}
            Cycle::Failed => tokio::time::sleep(ERROR_BACKOFF).await,
        }
    }
}

const ERROR_BACKOFF: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq, Eq)]
enum Cycle {
    Committed,
    Failed,
}

/// One consume -> classify -> XADD -> commit cycle. Only commits the Kafka
/// offset once EVERY surviving envelope from this record has been
/// durably XADDed -- mirrors trust-consumer's own never-commit-on-a-
/// failed-downstream-write discipline, substituting "every XADD in this
/// record succeeded" for "the HTTP POST succeeded".
async fn run_cycle<S, K>(source: &mut S, sink: &mut K) -> Cycle
where
    S: RawKafkaSource,
    K: EventSink,
{
    let batch = match source.next_batch().await {
        Ok(batch) => batch,
        Err(err) => {
            tracing::error!(error = ?err, "error receiving from Kafka");
            return Cycle::Failed;
        }
    };

    for raw in &batch {
        let envelopes = match trust_schema::schema::confirmed_envelope_bodies(raw) {
            Ok(envelopes) => envelopes,
            Err(err) => {
                tracing::error!(error = ?err, raw = %raw, "failed to classify Kafka record; not committing this record's offset");
                return Cycle::Failed;
            }
        };
        for (msg_type, payload) in &envelopes {
            if let Err(err) = sink.publish(msg_type, payload).await {
                tracing::error!(error = ?err, msg_type, "failed to XADD envelope; not committing this record's offset");
                return Cycle::Failed;
            }
            metrics::counter!(
                common::metrics::metric_name("movement_relay_events_published_total"),
                "msg_type" => msg_type.clone()
            )
            .increment(1);
        }
    }

    if let Err(err) = source.commit().await {
        tracing::error!(error = ?err, "failed to commit Kafka offset");
        return Cycle::Failed;
    }
    Cycle::Committed
}

/// Leading-indicator lag gauge (design doc Decision 2) -- polls `XINFO
/// GROUPS movement-events` for both downstream groups on its own timer,
/// independent of the main consume loop. Reuses the same `XINFO GROUPS`
/// field-walk shape `movement_feed::redis_stream::check_gap`'s own
/// `find_group_field` helper uses -- NOT re-exported from that crate
/// (`movement-relay` deliberately doesn't depend on `movement-feed`, Task
/// 6's own note) -- a small, independent copy here instead, or hand this
/// crate's own copy of `find_group_field` a private home in this file.
async fn stream_lag_loop(redis_url: String, interval: Duration) {
    let Ok(client) = redis::Client::open(redis_url) else {
        tracing::error!("stream_lag_loop: failed to build Redis client; lag gauge disabled");
        return;
    };
    let mut conn = match client.get_connection_manager().await {
        Ok(conn) => conn,
        Err(err) => {
            tracing::error!(error = ?err, "stream_lag_loop: failed to connect; lag gauge disabled");
            return;
        }
    };
    loop {
        tokio::time::sleep(interval).await;
        for group in ["trust-consumer", "full-coverage-consumer"] {
            match group_lag(&mut conn, group).await {
                Ok(Some(lag)) => {
                    metrics::gauge!(
                        common::metrics::metric_name("movement_relay_stream_lag"),
                        "group" => group
                    )
                    .set(lag as f64);
                }
                Ok(None) => {} // group doesn't exist yet -- nothing to report.
                Err(err) => {
                    tracing::warn!(error = ?err, group, "stream_lag_loop: failed to fetch XINFO GROUPS");
                }
            }
        }
    }
}

/// `XINFO GROUPS movement-events`'s `lag` field for one named group --
/// same reply-walk shape as `crates/enricher/src/stream.rs::group_lag`,
/// generalized over group name (this function serves two group names from
/// one binary; enricher's own copy only ever serves one, `"enricher"`).
async fn group_lag(conn: &mut redis::aio::ConnectionManager, group: &str) -> anyhow::Result<Option<i64>> {
    let reply: Vec<redis::Value> = redis::cmd("XINFO")
        .arg("GROUPS")
        .arg("movement-events")
        .query_async(conn)
        .await?;
    for entry in reply {
        let redis::Value::Array(fields) = entry else { continue };
        let mut name: Option<String> = None;
        let mut lag: Option<i64> = None;
        let mut it = fields.into_iter();
        while let (Some(k), Some(v)) = (it.next(), it.next()) {
            let k: String = redis::from_redis_value(&k)?;
            match k.as_str() {
                "name" => name = redis::from_redis_value(&v).ok(),
                "lag" => lag = redis::from_redis_value(&v).ok(),
                _ => {}
            }
        }
        if name.as_deref() == Some(group) {
            return Ok(lag);
        }
    }
    Ok(None)
}
```

- [ ] **Step 3: Tests** (in `main.rs`'s own `#[cfg(test)] mod tests`,
  against `FakeRawSource`/`FakeEventSink` — no real Kafka or Redis):

  - `a_batch_with_confirmed_and_unknown_types_publishes_only_confirmed`:
    one raw record containing a 2-envelope array (`0003` valid, `0005`
    unknown) → `sink.published.len() == 1`, its `msg_type == "0003"`.
  - `every_envelope_in_a_record_must_publish_before_the_offset_commits`:
    one raw record with 2 confirmed envelopes, `sink.fail_next = true`
    (fails the **second** publish) → `Cycle::Failed`,
    `source.committed_count == 0` — the direct regression test for "only
    commit once every surviving envelope has been durably XADDed," proving
    a partial-record failure does not commit (so the whole record,
    including the envelope that DID publish successfully just before the
    failure, is redelivered next time — a harmless duplicate XADD on
    retry, not a lost one, since Redis Streams entries aren't deduplicated
    at the transport level, only downstream via `dedup_key` once they
    reach a consumer).
  - `an_unclassifiable_record_does_not_commit`: a raw record that fails
    `confirmed_envelope_bodies` outright (missing `header`) →
    `Cycle::Failed`, `committed_count == 0`.
  - `a_clean_batch_commits_and_increments_the_publish_counter`: (metrics
    assertion optional/best-effort depending on whether `metrics::counter!`
    is easily observable in a unit test without a real recorder installed —
    if not, assert only on `sink.published`/`committed_count`, and note in
    the test's own comment that the counter increment is exercised but not
    independently asserted, matching how `full-coverage-consumer/src/main.rs`'s
    own existing tests already treat its `metrics::counter!` calls.)
  - `an_empty_poll_commits_nothing`: mirrors
    `trust-consumer::main::an_empty_poll_commits_nothing` exactly.

- [ ] **Step 4: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p movement-relay --all-features
cargo test -p movement-relay
```

```bash
git add crates/movement-relay/src/{event_sink.rs,main.rs}
git commit -m "movement-relay: consume -> classify -> XADD -> commit main loop, plus the leading-indicator lag gauge"
```

---

## Task 8: `docker/movement-relay.Dockerfile`

**Files:** create `docker/movement-relay.Dockerfile`.

Depends on Task 5 (crate must exist to build). Modeled directly on
`docker/trust-consumer.Dockerfile` — **this crate needs the same
`cmake`/`libssl-dev`/`pkg-config`/`libsasl2-dev`/`libcurl4-openssl-dev`
builder-stage packages and the same `libsasl2-2` runtime package**, for
the identical reason (`rdkafka`'s `cmake-build`/`ssl`/`sasl` features).
**This is the crate those packages "belong to" from Deploy C onward** — see
Task 16's own note on removing them from `trust-consumer.Dockerfile`/
`full-coverage-consumer.Dockerfile` once those crates drop `rdkafka`
entirely.

- [ ] **Step 1: Write the Dockerfile**, copying `trust-consumer.Dockerfile`
  verbatim in structure, substituting the binary name (`movement-relay`
  for `trust-consumer` throughout), dropping the `reference-data/` `COPY`
  (this crate has no static reference data to bake in — it never touches
  STANOX/CRS), and dropping the `curl` runtime package + `HEALTHCHECK`-style
  comment reasoning only if this crate's own container never needs a
  compose-level `curl`-based healthcheck (Task 11 decides this — if
  `docker-compose.yml`'s `movement-relay` service gets a `HEALTHCHECK`
  entry mirroring `trust-consumer`'s own, keep `curl` in the runtime
  image; if not, drop it. Default to keeping it for consistency with every
  other persistent-consumer Dockerfile in this repo, since Task 11 does
  add a compose healthcheck).

  Numeric `USER`/group: pick the next unused uid/gid in this repo's own
  sequence — confirm the highest currently used
  (`grep -rn 'groupadd --system --gid' docker/*.Dockerfile`) and take the
  next integer, following the same "pinned numeric, not symbolic" reasoning
  `trust-consumer.Dockerfile`'s own comment already documents.

- [ ] **Step 2: Build sanity check**

```bash
docker build -f docker/movement-relay.Dockerfile -t distant-signal/movement-relay:test .
```

- [ ] **Step 3: Commit**

```bash
git add docker/movement-relay.Dockerfile
git commit -m "Add docker/movement-relay.Dockerfile, modeled on trust-consumer.Dockerfile"
```

---

## Task 9: Helm — `movement-relay` Deployment, values, secrets

**Files:** create `charts/distant-signal/templates/movement-relay-deployment.yaml`;
modify `charts/distant-signal/{values.yaml,templates/{_helpers.tpl,secret.yaml}}`.

Depends on Task 8 (image must be buildable to sanely deploy, though
`helm template` itself doesn't need a real image to render).

- [ ] **Step 1: `values.yaml`** — new `movementRelay` block, own `kafka.*`
  sub-block (deliberately **not** reusing `trustConsumer.kafka.*` — see
  this task's own note below for why this is the one place this plan
  diverges from `full-coverage-consumer`'s existing "reuse trustConsumer's
  Kafka values" precedent), inserted after the existing `fullCoverageConsumer`
  block:

```yaml
# ---------------------------------------------------------------------------
# movementRelay -- the sole real Kafka client against RDM's Train Movements
# product from Deploy B onward (docs/superpowers/specs/2026-09-04-movement-relay-design.md).
# Off by default: `movementRelay.enabled: false` means this Deployment does
# not render at all, and neither trustConsumer nor fullCoverageConsumer's
# production Kafka behavior changes until a human executes the Deploy B
# runbook (docs/superpowers/plans/2026-09-04-movement-relay-plan.md).
#
# Kafka connection settings here are DELIBERATELY SEPARATE from
# trustConsumer.kafka.* (unlike fullCoverageConsumer's own values block,
# which reuses trustConsumer.kafka.* by design) -- Decision 4's own B2 step
# describes the real credential as MOVING from trustConsumer.kafka.* to
# movementRelay.kafka.* during the cutover, which only makes sense if these
# are genuinely separate value paths an operator sets independently, not
# the same one two components both read.
# ---------------------------------------------------------------------------
movementRelay:
  enabled: false
  image:
    repository: distant-signal/movement-relay
    tag: ""
    pullPolicy: IfNotPresent
  kafka:
    brokers: ""
    topic: ""
    consumerGroup: ""
    saslMechanism: ""
    saslUsername: ""
    saslPassword: ""
    existingSecret: ""
    existingSecretUsernameKey: kafka-sasl-username
    existingSecretPasswordKey: kafka-sasl-password
  streamLagPollSecs: 30
  healthPort: 8083
  metricsPort: 9094
  logLevel: info
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

  Also add, alongside the existing `trustConsumer.replicaCount`/
  `fullCoverageConsumer.replicaCount` (Task 10) and `movementFeed` toggle:
  nothing further here — those two fields live on `trustConsumer`/
  `fullCoverageConsumer`'s own blocks, not `movementRelay`'s (Task 10
  covers this).

- [ ] **Step 2: `_helpers.tpl`** — `movementRelaySecretName`, mirroring
  `trustConsumerSecretName`'s exact shape (`_helpers.tpl:281-283`):

```
{{- define "distant-signal.movementRelaySecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.movementRelay.kafka.existingSecret }}
{{- end }}
```

  (No `movementRelayOauthUsernameSecretKey`/`...PasswordSecretKey` needed —
  `movement-relay` has no internal-OAuth credential at all, unlike every
  other crate in this repo; it never calls `api`.)

- [ ] **Step 3: `secret.yaml`** — a second `kafka-sasl-username`/
  `kafka-sasl-password` pair, this time keyed off `movementRelay.kafka.*`
  rather than `trustConsumer.kafka.*`, under **different** secret data
  keys (since both may coexist in the same chart-rendered Secret during
  the Deploy A→B window — they must not collide):

```
{{- if not .Values.movementRelay.kafka.existingSecret -}}
{{- $_ := set $data "movement-relay-kafka-sasl-username" (.Values.movementRelay.kafka.saslUsername | default "" | b64enc) -}}
{{- $_ := set $data "movement-relay-kafka-sasl-password" (.Values.movementRelay.kafka.saslPassword | default "" | b64enc) -}}
{{- end -}}
```

  placed directly after the existing `trustConsumer`-keyed
  `kafka-sasl-username`/`kafka-sasl-password` block (`secret.yaml:125-134`),
  with a one-line comment cross-referencing why the key names differ
  (avoids the collision noted in Step 2's own reasoning).

- [ ] **Step 4: `templates/movement-relay-deployment.yaml`** — new file,
  modeled on `trust-consumer-deployment.yaml`'s overall shape, with the
  three deliberate differences the design doc's Decision 5 specifies:

```yaml
{{- if .Values.movementRelay.enabled }}
{{- if not .Values.movementRelay.kafka.brokers }}
{{- fail "movementRelay.kafka.brokers is empty. Set it to the RDM Train Movements Kafka broker address(es) once movementRelay.enabled=true -- see docs/superpowers/plans/2026-09-04-movement-relay-plan.md's Deploy B runbook." }}
{{- end }}
{{- if not .Values.movementRelay.kafka.topic }}
{{- fail "movementRelay.kafka.topic is empty. Set it to the confirmed RDM Train Movements Kafka topic name." }}
{{- end }}
{{- if not .Values.movementRelay.kafka.consumerGroup }}
{{- fail "movementRelay.kafka.consumerGroup is empty. Set it to the real, RDM-issued consumer group id (SC-... in production) -- see the design doc's 'Why this exists' section. Unlike trustConsumer.kafka.consumerGroup, this field has NO sensible default: guessing wrong here risks colliding with a real group id." }}
{{- end }}
{{- if not .Values.movementRelay.kafka.saslMechanism }}
{{- fail "movementRelay.kafka.saslMechanism is empty. Set it to the confirmed RDM Kafka product's SASL mechanism." }}
{{- end }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-movement-relay" (include "distant-signal.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "movement-relay") | nindent 4 }}
spec:
  replicas: 1
  # Explicit, unlike trust-consumer/full-coverage-consumer's own
  # undeclared (default-25%/25%) strategy: a rolling update here MUST NOT
  # have zero members ready before a new one takes over -- see the design
  # doc's Decision 5. This is the OPPOSITE intent from every Recreate-typed
  # Deployment in this chart (aggregator, notifier, poller-deployments,
  # schedulefeed), stated explicitly for the same reason they state theirs.
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 0
      maxSurge: 1
  selector:
    matchLabels:
      {{- include "distant-signal.selectorLabels" (dict "root" . "component" "movement-relay") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "distant-signal.labels" (dict "root" . "component" "movement-relay") | nindent 8 }}
      {{- if or .Values.metrics.enabled .Values.movementRelay.podAnnotations }}
      annotations:
        {{- if .Values.metrics.enabled }}
        prometheus.io/scrape: "true"
        prometheus.io/port: {{ .Values.movementRelay.metricsPort | quote }}
        prometheus.io/path: "/metrics"
        {{- end }}
        {{- with .Values.movementRelay.podAnnotations }}
        {{- toYaml . | nindent 8 }}
        {{- end }}
      {{- end }}
    spec:
      serviceAccountName: {{ include "distant-signal.serviceAccountName" . }}
      automountServiceAccountToken: false
      {{- with .Values.imagePullSecrets }}
      imagePullSecrets:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      securityContext:
        {{- include "distant-signal.podSecurityContext" (dict "override" .Values.movementRelay.podSecurityContext) | nindent 8 }}
      containers:
        - name: movement-relay
          image: {{ include "distant-signal.image" (dict "root" . "image" .Values.movementRelay.image) | quote }}
          imagePullPolicy: {{ .Values.movementRelay.image.pullPolicy }}
          securityContext:
            {{- include "distant-signal.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          ports:
            - name: health
              containerPort: {{ .Values.movementRelay.healthPort }}
              protocol: TCP
            {{- if .Values.metrics.enabled }}
            - name: metrics
              containerPort: {{ .Values.movementRelay.metricsPort }}
              protocol: TCP
            {{- end }}
          # Readiness means "confirmed Kafka partition assignment" (a
          # rebalance-callback-driven signal, crates/movement-relay/src/health.rs),
          # NOT "at least one message arrived" -- deliberately tighter than
          # trust-consumer's own probe. See that file's own doc comment.
          readinessProbe:
            httpGet:
              path: /healthz
              port: health
            initialDelaySeconds: 10
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: /healthz
              port: health
            initialDelaySeconds: 30
            periodSeconds: 15
            failureThreshold: 6
          env:
            - name: KAFKA_BROKERS
              value: {{ .Values.movementRelay.kafka.brokers | quote }}
            - name: KAFKA_TOPIC
              value: {{ .Values.movementRelay.kafka.topic | quote }}
            - name: KAFKA_CONSUMER_GROUP
              value: {{ .Values.movementRelay.kafka.consumerGroup | quote }}
            - name: KAFKA_SASL_MECHANISM
              value: {{ .Values.movementRelay.kafka.saslMechanism | quote }}
            - name: KAFKA_SASL_USERNAME
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.movementRelaySecretName" . }}
                  key: {{ .Values.movementRelay.kafka.existingSecretUsernameKey }}
            - name: KAFKA_SASL_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.movementRelaySecretName" . }}
                  key: {{ .Values.movementRelay.kafka.existingSecretPasswordKey }}
            - name: REDIS_URL
              value: {{ include "distant-signal.redisUrl" . | quote }}
            - name: STREAM_LAG_POLL_SECS
              value: {{ .Values.movementRelay.streamLagPollSecs | quote }}
            - name: HEALTH_BIND_URL
              value: {{ printf "0.0.0.0:%d" (int .Values.movementRelay.healthPort) | quote }}
            - name: METRICS_ENABLED
              value: {{ .Values.metrics.enabled | quote }}
            {{- if .Values.metrics.enabled }}
            - name: METRICS_PORT
              value: {{ .Values.movementRelay.metricsPort | quote }}
            {{- end }}
            - name: RUST_LOG
              value: {{ .Values.movementRelay.logLevel | quote }}
            {{- with .Values.movementRelay.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with .Values.movementRelay.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.movementRelay.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.movementRelay.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.movementRelay.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
```

  (`distant-signal.redisUrl` — confirm this helper already exists via
  `grep -n 'define "distant-signal.redisUrl"' charts/distant-signal/templates/_helpers.tpl`;
  if `api`/`enricher`'s Redis URL is instead inlined per-template rather
  than via a shared helper, follow whichever pattern is actually there —
  don't invent a helper that doesn't already exist without checking first.)

- [ ] **Step 5: Render-check**

```bash
helm template charts/distant-signal > /dev/null  # movementRelay.enabled=false -- must NOT render the Deployment or fail any guard.
helm template charts/distant-signal \
  --set movementRelay.enabled=true \
  --set movementRelay.kafka.brokers=kafka.example.invalid:9092 \
  --set movementRelay.kafka.topic=test-topic \
  --set movementRelay.kafka.consumerGroup=test-group \
  --set movementRelay.kafka.saslMechanism=PLAIN \
  > /dev/null  # must render cleanly with placeholder values.
grep -c "movement-relay" <(helm template charts/distant-signal) # expect 0 -- confirms the disabled-by-default guard actually works, not just "didn't error."
```

- [ ] **Step 6: Commit**

```bash
git add charts/distant-signal/templates/movement-relay-deployment.yaml \
        charts/distant-signal/templates/_helpers.tpl \
        charts/distant-signal/templates/secret.yaml \
        charts/distant-signal/values.yaml
git commit -m "Helm: add movement-relay Deployment (disabled by default), values, and secret wiring"
```

---

## Task 10: Helm — `trustConsumer`/`fullCoverageConsumer` `replicaCount` + `movementFeed` toggle

**Files:** modify `charts/distant-signal/{values.yaml,templates/{trust-consumer-deployment.yaml,full-coverage-consumer-deployment.yaml}}`.

Depends on Task 4 (the `movement_feed_backend`/`redis_url` config fields
must exist for these env vars to mean anything). Implements "Judgment
calls" items 2 and 5 above at the chart layer.

- [ ] **Step 1: `values.yaml`** — add to the existing `trustConsumer` block
  (near `retentionDays`):

```yaml
  # -- Values-driven replica count, added specifically to support the
  # Deploy B cutover runbook's B1 step (stop trust-consumer's Kafka
  # connection cleanly, by scaling to 0 via `helm upgrade`, not an
  # out-of-band `kubectl scale` a later `helm upgrade` could silently
  # undo). See docs/superpowers/plans/2026-09-04-movement-relay-plan.md,
  # "Judgment calls," item 2.
  replicaCount: 1
  # -- Which MovementFeed transport this deployment uses: "kafka" (today's
  # production default, unchanged) or "redis-stream" (reads what
  # movement-relay publishes -- only meaningful once movementRelay.enabled
  # and the real credential has moved over, see the Deploy B runbook's B3
  # step).
  movementFeed: kafka
```

  and, mirroring for `fullCoverageConsumer`:

```yaml
  replicaCount: 1
  movementFeed: kafka
```

- [ ] **Step 2: Deployment templates** — both files:
  `spec.replicas: 1` → `spec.replicas: {{ .Values.trustConsumer.replicaCount }}`
  (respectively `.Values.fullCoverageConsumer.replicaCount`); add, to each
  container's `env:` list:

```yaml
            - name: MOVEMENT_FEED_BACKEND
              value: {{ .Values.trustConsumer.movementFeed | quote }}
            - name: REDIS_URL
              value: {{ include "distant-signal.redisUrl" . | quote }}
            - name: REDIS_AUTOCLAIM_MIN_IDLE_SECS
              value: "30"
```

  (`full-coverage-consumer-deployment.yaml`'s own copy substitutes
  `.Values.fullCoverageConsumer.movementFeed`.) Update each file's existing
  Kafka `fail` guards (`trust-consumer-deployment.yaml:14-22`,
  `full-coverage-consumer-deployment.yaml:10-18`) to only fire when
  `.Values.trustConsumer.movementFeed == "kafka"` — an operator who has
  already flipped to `redis-stream` (post-Deploy-B) should not be forced
  to keep a real Kafka credential populated just to satisfy a guard for a
  transport this Deployment no longer uses:

```
{{- if eq .Values.trustConsumer.movementFeed "kafka" }}
{{- if not .Values.trustConsumer.kafka.brokers }}
{{- fail "..." }}
{{- end }}
{{- /* ...same for topic/saslMechanism... */ -}}
{{- end }}
```

- [ ] **Step 3: Render-check**

```bash
helm template charts/distant-signal --set trustConsumer.kafka.brokers=x --set trustConsumer.kafka.topic=x --set trustConsumer.kafka.saslMechanism=PLAIN --set fullCoverageConsumer.kafka.consumerGroup=x > /dev/null
helm template charts/distant-signal --set trustConsumer.movementFeed=redis-stream --set fullCoverageConsumer.movementFeed=redis-stream > /dev/null  # must render WITHOUT needing any Kafka value set.
helm template charts/distant-signal --set trustConsumer.replicaCount=0 --set trustConsumer.kafka.brokers=x --set trustConsumer.kafka.topic=x --set trustConsumer.kafka.saslMechanism=PLAIN | grep -A2 "name: .*trust-consumer$" | grep -q "replicas: 0"  # confirm the knob actually works.
```

- [ ] **Step 4: Commit**

```bash
git add charts/distant-signal/values.yaml \
        charts/distant-signal/templates/trust-consumer-deployment.yaml \
        charts/distant-signal/templates/full-coverage-consumer-deployment.yaml
git commit -m "Helm: add trustConsumer/fullCoverageConsumer replicaCount and movementFeed toggle"
```

---

## Task 11: CI + local dev wiring

**Files:** modify `.github/workflows/containers.yml`, `docker-compose.yml`.

Depends on Task 8 (Dockerfile must exist).

- [ ] **Step 1: `containers.yml`** — add a matrix entry, alongside
  `full-coverage-consumer`'s:

```yaml
          - service: movement-relay
            dockerfile: docker/movement-relay.Dockerfile
            target: ""
```

- [ ] **Step 2: `docker-compose.yml`** — a new `movement-relay` service,
  same placeholder-`KAFKA_*`-env pattern `trust-consumer`'s own service
  already uses (real RDM values are GAP-flagged the same way), plus a
  `REDIS_URL` pointing at compose's existing `redis` service:

```yaml
  movement-relay:
    build:
      context: .
      dockerfile: docker/movement-relay.Dockerfile
      args:
        CARGO_PROFILE: release
    restart: unless-stopped
    depends_on:
      redis:
        condition: service_started
    environment:
      # crates/movement-relay/src/config.rs: Config. GAP: same
      # unconfirmed-against-a-live-RDM-catalogue placeholders as
      # trust-consumer's own KAFKA_* vars above -- see that service's own
      # comment.
      KAFKA_BROKERS: ${KAFKA_BROKERS}
      KAFKA_TOPIC: ${KAFKA_TOPIC}
      KAFKA_CONSUMER_GROUP: ${MOVEMENT_RELAY_KAFKA_CONSUMER_GROUP:?MOVEMENT_RELAY_KAFKA_CONSUMER_GROUP must be set -- this has no safe default, unlike KAFKA_CONSUMER_GROUP above}
      KAFKA_SASL_USERNAME: ${KAFKA_SASL_USERNAME}
      KAFKA_SASL_PASSWORD: ${KAFKA_SASL_PASSWORD}
      KAFKA_SASL_MECHANISM: ${KAFKA_SASL_MECHANISM}
      REDIS_URL: redis://redis:6379
      HEALTH_BIND_URL: 0.0.0.0:8083
      RUST_LOG: ${RUST_LOG:-info}
    healthcheck:
      test: ["CMD", "curl", "-f", "http://localhost:8083/healthz"]
      interval: 10s
      timeout: 3s
      retries: 5
      start_period: 15s
```

  (This service is **not** added to `trust-consumer`'s or
  `full-coverage-consumer`'s own `depends_on` in compose — those two keep
  reading Kafka directly by default in local dev too, same as production,
  unless a developer explicitly sets `MOVEMENT_FEED_BACKEND=redis-stream`
  in their own `.env`. Also add `REDIS_URL: redis://redis:6379` and
  `MOVEMENT_FEED_BACKEND: ${MOVEMENT_FEED_BACKEND:-kafka}` to both
  `trust-consumer`'s and `full-coverage-consumer`'s existing `environment:`
  blocks in this same step, so a developer CAN opt into exercising the new
  path locally without editing compose itself.)

- [ ] **Step 3: Verify and commit**

```bash
docker compose config > /dev/null  # syntax/interpolation sanity check only -- does not build images.
```

```bash
git add .github/workflows/containers.yml docker-compose.yml
git commit -m "CI/compose: add movement-relay service, wire REDIS_URL/MOVEMENT_FEED_BACKEND into trust-consumer and full-coverage-consumer"
```

---

## Task 12: Deploy A — full verification pass

**Files:** none (verification only).

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-features -- -D warnings` (or this
  repo's actual clippy invocation — confirm the exact flags `lint.yml`
  uses via `grep -n clippy .github/workflows/*.yml` first, match it)
- [ ] `cargo test --workspace`
- [ ] `REDIS_URL=redis://localhost:6379 cargo test -p movement-feed --
  --ignored --test-threads=1` (needs a local Redis running —
  `docker run -d -p 6379:6379 redis:7` if none is already up)
- [ ] `helm template charts/distant-signal > /dev/null` (default values —
  `movementRelay.enabled=false`, `trustConsumer.movementFeed=kafka` — must
  render cleanly with the SAME set of `--set` overrides this chart already
  required before this plan, no new mandatory value introduced by Deploy
  A)
- [ ] `helm template charts/distant-signal --set movementRelay.enabled=true --set movementRelay.kafka.brokers=x --set movementRelay.kafka.topic=x --set movementRelay.kafka.consumerGroup=x --set movementRelay.kafka.saslMechanism=PLAIN --set trustConsumer.movementFeed=redis-stream --set fullCoverageConsumer.movementFeed=redis-stream > /dev/null`
  (the full Deploy-B-end-state render, checked for cleanliness ahead of
  time even though nobody runs it against a real cluster yet)
- [ ] `docker build -f docker/movement-relay.Dockerfile -t distant-signal/movement-relay:verify .`
- [ ] Grep confirmation: `grep -rn "LineDefinition\|full_coverage_enabled" crates/movement-relay crates/movement-feed`
  returns nothing — confirms this plan never touched line-catalogue/shadow-mode
  concerns, per its own Non-goals.

No commit for this task — it's a gate before declaring Deploy A done, not
a code change.

---

# Deploy B — the credential cutover runbook

**This section is not a task list for an implementation agent. It is
executed by a human, once, watching logs live, against production. Nothing
below is meant to be run, simulated, or "tested" by an automated agent —
see the banner at the top of this document.**

## Why this exists as a runbook, not code

RDM enforces exactly one consumer group per account per API product
(confirmed live — see the design doc's "Why this exists" section: a second
group id under the same credential was rejected outright with
`GroupAuthorizationFailed`). There is no sandbox, staging group, or second
credential to rehearse this exact operation against before running it for
real. The core hazard (design doc Decision 4): if the old `trust-consumer`
(still on its direct-Kafka path) and the new `movement-relay` are ever
simultaneously members of the real group `SC-c4d90f8e-...`, Kafka's
rebalance protocol will split the topic's partitions between them, and
**neither service sees the whole feed** for that window — a self-inflicted
recreation of the exact stuck-Activation bug this whole redesign exists to
fix. Every step below exists to make that split structurally impossible,
by never having both services attempt group membership at the same time.

## Pre-flight (before B1)

- [ ] Deploy A (Tasks 1–12) has been merged, deployed to production, and
  observed stable for a real period with `movementRelay.enabled=false` and
  `trustConsumer.movementFeed=kafka`/`fullCoverageConsumer.movementFeed=kafka`
  (today's unchanged behavior). Confirm via `kubectl get pods` that no
  `movement-relay` pod exists yet.
- [ ] `movement-relay`'s own code has been exercised against a
  placeholder/dev Kafka credential or a local `FakeRawSource`/`FakeEventSink`-backed
  test run (Task 7's own unit tests, plus a manual local run against a
  throwaway topic if one is available) — this is the "real verification
  point before the real credential is ever involved" the design doc's
  Decision 4 names for Deploy A.
- [ ] The real RDM credential (`SC-c4d90f8e-...` group id, SASL
  username/password, confirmed broker address, confirmed topic name,
  confirmed SASL mechanism) is in hand, ready to pass via `--set` (or a
  values file kept OUT of version control — never commit it).
- [ ] `helm get values <release> -o yaml > preflight-values-backup.yaml`
  — the exact rollback baseline for every step below.
- [ ] A terminal tailing `kubectl logs -f deploy/<release>-trust-consumer`
  and a second tailing `kubectl logs -f deploy/<release>-movement-relay`
  (once it exists) are both open and watched for the entire operation. If
  RDM exposes any dashboard/API listing active consumer-group members for
  this subscription (unconfirmed whether one exists — design doc Open
  Question 7), have it open too, as a corroborating check at each
  verification gate below — but do not treat its absence as a blocker; the
  log-based checks below are the primary, load-bearing verification either
  way.

## B1 — stop `trust-consumer`'s Kafka connection cleanly

1. ```
   helm upgrade <release> charts/distant-signal -f <your-standard-values-file> \
     --set trustConsumer.replicaCount=0
   ```
2. `kubectl get pods -l app.kubernetes.io/component=trust-consumer` →
   confirm zero pods within a normal pod-termination window (well under a
   minute for a graceful SIGTERM-based shutdown of this simple a process).
3. **Wait 90 seconds** after the pod is confirmed gone, before proceeding
   to B2. This plan deliberately does not implement a `LeaveGroup`-on-SIGTERM
   handler (see "Judgment calls," item 3, above) — this fixed wait is the
   accepted substitute, generous relative to `librdkafka`'s own default
   `session.timeout.ms` (45 seconds) so the broker has evicted the dead
   member via timeout well before B2 starts, even without an explicit
   departure message.
4. If RDM exposes consumer-group visibility, confirm zero active members
   on `SC-c4d90f8e-...` before proceeding. If not, proceed on the timing
   buffer alone — an accepted residual risk, named as such in the design
   doc's own Open Question 7.

**Rollback for B1** (safe at any point before B2 starts): `helm upgrade
<release> charts/distant-signal -f <your-standard-values-file> --set
trustConsumer.replicaCount=1`. Nothing else has changed yet; `trust-consumer`
resumes from its last committed Kafka offset exactly as before this
operation started.

## B2 — deploy `movement-relay` with the real credential; verify sole membership

1. ```
   helm upgrade <release> charts/distant-signal -f <your-standard-values-file> \
     --set trustConsumer.replicaCount=0 \
     --set movementRelay.enabled=true \
     --set movementRelay.kafka.brokers=<real-broker> \
     --set movementRelay.kafka.topic=<real-topic> \
     --set movementRelay.kafka.consumerGroup=SC-c4d90f8e-c047-49b5-9892-6c9cda63e1eb \
     --set movementRelay.kafka.saslMechanism=<real-mechanism> \
     --set movementRelay.kafka.saslUsername=<real-username> \
     --set movementRelay.kafka.saslPassword=<real-password>
   ```
   (Note `trustConsumer.replicaCount=0` is repeated in this same command
   deliberately — never rely on a value set by a previous `helm upgrade`
   persisting implicitly; state every value this operation depends on
   explicitly, every time, in every command in this runbook.)
2. Watch `movement-relay`'s logs for, in order: a successful broker
   connection, the `RelayContext::post_rebalance` "Kafka partition
   assignment confirmed" log line (Task 5's own log message), then either a
   real message flowing through (an `XADD`-driven
   `movement_relay_events_published_total` increment) or — if the feed is
   quiet at that exact moment — at minimum the readiness flip.
3. `kubectl get pods -l app.kubernetes.io/component=movement-relay` →
   `1/1 Ready` (confirms the readiness probe, Task 5/9, is passing —
   partition assignment confirmed, not merely "the pod started").
4. If RDM exposes consumer-group visibility, confirm exactly one member on
   `SC-c4d90f8e-...`, `movement-relay`'s own client id. If not, corroborate
   via a sustained (~5 minute) window of stable partition assignment in the
   logs — no repeated rebalance/`GroupAuthorizationFailed` churn.
5. Watch `movement_relay_stream_lag{group="trust-consumer"}` and
   `{group="full-coverage-consumer"}` (Prometheus, or `kubectl exec` into
   the pod and query `/metrics` directly if Prometheus scraping isn't yet
   wired to a dashboard) — expect these to **climb** during this step
   (nothing is reading yet), which is expected, not itself a failure. Watch
   that the climb rate stays well within the `MAXLEN=500,000`/~19-hour
   headroom the design doc's Decision 2 sizes for — if it's climbing
   faster than that budget implies, treat this as a signal to proceed to
   B3 promptly, not necessarily to abort.

**Rollback for B2** — this is the step Decision 4's whole cutover-safety
argument is built around; follow this exactly, not from memory:

```
helm upgrade <release> charts/distant-signal -f <your-standard-values-file> \
  --set trustConsumer.replicaCount=0 \
  --set movementRelay.enabled=false
```

Then **wait for `movement-relay`'s own pod to be confirmed gone** (same
`kubectl get pods` check as B1 step 2) **and wait the same 90-second
buffer** (same reasoning as B1 step 3 — `movement-relay` has no
`LeaveGroup` handler either) **before** running:

```
helm upgrade <release> charts/distant-signal -f <your-standard-values-file> \
  --set trustConsumer.replicaCount=1 \
  --set movementRelay.enabled=false
```

**Do not combine these two rollback commands into one, and do not scale
`trustConsumer.replicaCount` back to `1` in the same command that disables
`movementRelay`.** Doing so risks the exact two-member split this whole
runbook exists to prevent, just with the roles reversed from B1's own
hazard.

## B3 — cut `trust-consumer`/`full-coverage-consumer` over to Redis Streams

Only start this once B2's verification (steps 2–5 above) has been
confirmed stable.

1. ```
   helm upgrade <release> charts/distant-signal -f <your-standard-values-file> \
     --set movementRelay.enabled=true \
     --set movementRelay.kafka.brokers=<real-broker> \
     --set movementRelay.kafka.topic=<real-topic> \
     --set movementRelay.kafka.consumerGroup=SC-c4d90f8e-c047-49b5-9892-6c9cda63e1eb \
     --set movementRelay.kafka.saslMechanism=<real-mechanism> \
     --set movementRelay.kafka.saslUsername=<real-username> \
     --set movementRelay.kafka.saslPassword=<real-password> \
     --set trustConsumer.replicaCount=1 \
     --set trustConsumer.movementFeed=redis-stream \
     --set fullCoverageConsumer.movementFeed=redis-stream
   ```
2. Watch `trust-consumer`'s and `full-coverage-consumer`'s logs for:
   `RedisStreamMovementFeed` startup (its own `ensure_group`/PEL-replay
   entry, Task 3), then a first successful `XREADGROUP` delivery, events
   flowing into `process::run_once`/`correlate::apply_*`, and `commit`
   (`XACK`) calls succeeding.
3. `kubectl get pods -l app.kubernetes.io/component=trust-consumer` and
   the equivalent for `full-coverage-consumer` → both `1/1 Ready`.
4. Confirm `movement_relay_stream_lag{group=...}` for both groups is now
   trending **down** toward a small steady-state value, not still
   climbing from B2.
5. Spot-check one real end-to-end cycle if a live tracked train is
   available at the time: confirm an Activation and at least one Movement
   for it reach `api` and update `train_current_state` as expected — the
   same manual bar this repo already used when `trust-consumer` first went
   live (no new verification method invented here).
6. Once stable for an observation window the repo owner judges sufficient
   (no fixed duration prescribed here — this is a judgment call for the
   person running the operation, not a number this plan invents), Deploy B
   is complete.

**Rollback for B3 — read this before touching anything, this is the single
highest-risk rollback in this whole runbook.** Do **NOT** set
`trustConsumer.movementFeed=kafka` (or `fullCoverageConsumer.movementFeed=kafka`)
while `movementRelay.enabled=true`. Doing so puts `trust-consumer` back
onto a direct Kafka connection to `SC-c4d90f8e-...` **while `movement-relay`
is still a live member of that same group** — this is not a "revert to a
known-good prior state," it is a fresh instance of the exact
two-independent-members-split-partitions hazard Decision 4 exists to
prevent, just triggered from the opposite direction (a redundant Kafka
member being ADDED back, not two ever having coexisted before). The only
safe recovery paths from a B3 problem are:

- **Fix forward.** The hazard this runbook exists to prevent is
  specifically two *Kafka* clients on the same group — `trust-consumer`
  and `full-coverage-consumer` reading Redis Streams while
  `movement-relay` keeps running on Kafka has no such hazard (Redis
  Streams consumer groups have no partition-splitting failure mode at
  all, per the design doc's own Decision 4 closing paragraph). If B3's
  problem is confined to the Redis Streams read path (e.g. a bug in
  `RedisStreamMovementFeed`), debug and redeploy `trust-consumer`/
  `full-coverage-consumer` with `movementFeed=redis-stream` still set,
  leaving `movement-relay` and Redis exactly as they are.
- **Full unwind**, only if fixing forward isn't viable: repeat B2's own
  rollback procedure in full (disable `movementRelay`, confirm its pod
  is gone, wait 90 seconds, only then scale `trustConsumer.replicaCount`
  back to `1` with `movementFeed=kafka`) — treating B2 and B3 as one unit
  to unwind together, not B3 alone.

## Post-B verification checklist

- [ ] `trustConsumer.kafka.*`/no Kafka value under `fullCoverageConsumer`
  is read by anything live in production any longer — `movement-relay` is
  now the sole real Kafka client. Safe to schedule Deploy C.
- [ ] The stuck-`resolution_status='pending'`-forever bug class
  (`trust-consumer/src/process.rs:32-45`) is now durably retried on a
  `trust-consumer` restart, via `RedisStreamMovementFeed`'s startup PEL
  replay (Task 3's own regression test,
  `startup_replay_delivers_a_prior_consumers_unacked_entry`, covers the
  mechanism in isolation — this checklist item is a note that the
  mechanism is now live in production, not a claim that it was
  independently re-verified end-to-end against a real forced mid-activation
  crash, which this runbook does not attempt).
- [ ] Record the actual date/time Deploy B completed somewhere durable
  (a follow-up commit updating this plan doc's own checkboxes is fine) —
  Deploy C's own gate (Task 13) checks for this.

---

# Deploy C — cleanup, normal review bar, gated on Deploy B

**Do not merge Tasks 13–16 until a human confirms the Deploy B runbook's
B3 step has completed successfully in production and the "Post-B
verification checklist" above is checked off.** These tasks are written
now so they're ready to execute promptly once that gate clears — "not
time-pressured" per the design doc's own framing of Deploy C, but also not
blocked on anything code-related, only on the real-world event of B3
succeeding.

## Task 13: `trust-consumer` — delete the Kafka path

**Files:** delete `crates/trust-consumer/src/feed/kafka.rs`; modify
`crates/trust-consumer/{Cargo.toml,src/{config.rs,main.rs,feed/mod.rs}}`.

- [ ] **Step 1: Confirm the gate.** `grep -n movementFeed
  charts/distant-signal/values.yaml` shows `trustConsumer.movementFeed`
  currently set to (or overridden in production to) `redis-stream` — if
  this plan's own commit history / the operator directly confirms Deploy
  B's B3 succeeded, proceed; otherwise stop and wait.
- [ ] **Step 2:** `git rm crates/trust-consumer/src/feed/kafka.rs`.
- [ ] **Step 3: `config.rs`** — delete `MovementFeedBackend`, the
  `movement_feed_backend` field, and every `kafka_*` field
  (`kafka_brokers`, `kafka_topic`, `kafka_consumer_group`,
  `kafka_sasl_username`, `kafka_sasl_password`, `kafka_sasl_mechanism`).
  `redis_url`/`redis_autoclaim_min_idle_secs` stay, now unconditionally
  required/used.
- [ ] **Step 4: `main.rs`** — collapse the `ActiveFeed` enum (or `Box<dyn
  MovementFeed>` match, per however Task 4 was actually implemented) back
  to a single, direct
  `RedisStreamMovementFeed::connect(&config.redis_url, "trust-consumer",
  "trust-consumer-1", ...).await?` construction — no branching left. The
  gap-check call site (Task 4 Step 4) simplifies to a direct call, no
  longer routed through the enum's delegating inherent method.
- [ ] **Step 5: `feed/mod.rs`** — delete `pub mod kafka;`; keep the
  `pub use movement_feed::{MovementFeed, FakeMovementFeed};` re-export (or
  remove the file entirely and have `main.rs` import `movement_feed`
  directly — either is fine, pick whichever is less churn given how Task 4
  actually left this file).
- [ ] **Step 6: `Cargo.toml`** — remove `rdkafka`, `async-trait` (confirm
  first via `grep -rn async_trait crates/trust-consumer/src/` that nothing
  else in this crate still uses it directly — `MovementFeed`'s own
  `#[async_trait]` macro usage now lives entirely inside `movement-feed`,
  not here).
- [ ] **Step 7: Tests.** Existing `main.rs` tests (against
  `FakeMovementFeed`) — confirm they still pass with identical assertions
  (only construction-site code changed, per Task 4's own original
  design). Delete the now-meaningless "backend defaults to kafka" config
  test added in Task 4 Step 5.
- [ ] **Step 8: Verify and commit**

```bash
cargo fmt --all
cargo clippy -p trust-consumer --all-features
cargo test -p trust-consumer
```

```bash
git add -A crates/trust-consumer
git commit -m "trust-consumer: delete the dead Kafka MovementFeed path post-Deploy-B (Redis Streams is now the only transport)"
```

## Task 14: `full-coverage-consumer` — delete the Kafka path

**Files:** same shape as Task 13, applied to
`crates/full-coverage-consumer/`.

- [ ] Repeat every step of Task 13 against `full-coverage-consumer`'s own
  `feed/kafka.rs`, `config.rs`, `main.rs`, `feed/mod.rs`, `Cargo.toml`.
- [ ] Verify and commit, same bar as Task 13 Step 8, substituting `-p
  full-coverage-consumer`.

## Task 15: Dockerfiles, Helm guards, values, compose cleanup

**Files:** modify `docker/{trust-consumer,full-coverage-consumer}.Dockerfile`,
`charts/distant-signal/{values.yaml,templates/{trust-consumer-deployment,full-coverage-consumer-deployment}.yaml}`,
`docker-compose.yml`.

Depends on Tasks 13–14 (crates must have actually dropped `rdkafka` first).

- [ ] **Step 1: Dockerfiles.** Both `trust-consumer.Dockerfile` and
  `full-coverage-consumer.Dockerfile` drop the
  `cmake`/`libssl-dev`/`pkg-config`/`libsasl2-dev`/`libcurl4-openssl-dev`
  builder-stage packages and the `libsasl2-2` runtime package — confirm
  first, via `cargo tree -p trust-consumer | grep -i rdkafka` (expect no
  output), that nothing pulls `rdkafka` transitively any longer. Match the
  resulting shape to `aggregator.Dockerfile`'s own plain, pure-Rust
  multi-stage build (no special system packages) — that's the correct
  post-cleanup template to copy from, not `movement-relay.Dockerfile`
  (which still needs them).
- [ ] **Step 2: Helm — delete the dead Kafka guard blocks and values.**
  `trust-consumer-deployment.yaml`'s and
  `full-coverage-consumer-deployment.yaml`'s `{{- if eq
  .Values.trustConsumer.movementFeed "kafka" }}...{{- end }}`-wrapped
  guards (Task 10 Step 2) are deleted entirely, along with the `KAFKA_*`
  env entries in both files. `values.yaml`: delete
  `trustConsumer.kafka.*` and `fullCoverageConsumer.kafka.*` entirely
  (their real credential now lives only under `movementRelay.kafka.*`,
  which stays); delete `trustConsumer.movementFeed`/
  `fullCoverageConsumer.movementFeed` (no longer a real choice — always
  Redis Streams now). **Keep `trustConsumer.replicaCount`/
  `fullCoverageConsumer.replicaCount`** — per "Judgment calls," item 2's
  own framing, this is a generically useful knob now that it exists, not
  scheduled for removal.
- [ ] **Step 3: `secret.yaml`.** Delete the `trustConsumer.kafka.*`-keyed
  `kafka-sasl-username`/`kafka-sasl-password` block (`secret.yaml:125-134`
  originally) — the `movement-relay`-keyed pair added in Task 9 Step 3
  stays, now the only Kafka secret this chart ever renders.
- [ ] **Step 4: `docker-compose.yml`.** Remove `trust-consumer`'s and
  `full-coverage-consumer`'s `KAFKA_*`/`MOVEMENT_FEED_BACKEND` environment
  entries entirely (added in Task 11 Step 2); both now unconditionally
  read `REDIS_URL` only. `movement-relay`'s own service block (added in
  Task 11) is unchanged by this task.
- [ ] **Step 5: `.github/workflows/ci.yml`.** Confirm via `cargo tree
  --workspace | grep -i rdkafka` that `movement-relay` is now the
  **only** workspace member pulling `rdkafka` — the "Install rdkafka
  build dependencies" step stays (still needed for `movement-relay`), its
  comment updated to say so explicitly rather than naming
  `trust-consumer` as the reason (its own comment currently cites
  `trust-consumer/Cargo.toml`'s `rdkafka` dependency by name — that
  citation is now stale and should point at `movement-relay` instead).
- [ ] **Step 6: Render/build checks**

```bash
helm template charts/distant-signal > /dev/null
docker build -f docker/trust-consumer.Dockerfile -t distant-signal/trust-consumer:verify .
docker build -f docker/full-coverage-consumer.Dockerfile -t distant-signal/full-coverage-consumer:verify .
docker compose config > /dev/null
```

- [ ] **Step 7: Commit**

```bash
git add docker/trust-consumer.Dockerfile docker/full-coverage-consumer.Dockerfile \
        charts/distant-signal/values.yaml \
        charts/distant-signal/templates/{trust-consumer-deployment,full-coverage-consumer-deployment,secret}.yaml \
        docker-compose.yml .github/workflows/ci.yml
git commit -m "Deploy C cleanup: drop rdkafka build deps and Kafka Helm/compose wiring from trust-consumer and full-coverage-consumer"
```

## Task 16: Deploy C — final verification pass

**Files:** none.

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-features`
- [ ] `cargo test --workspace`
- [ ] `REDIS_URL=redis://localhost:6379 cargo test -p movement-feed --
  --ignored --test-threads=1`
- [ ] `DATABASE_URL=<url> cargo test -p api -p aggregator -- --ignored
  --test-threads=1` (confirms nothing in this cleanup accidentally broke
  an unrelated DB-backed test)
- [ ] `cargo tree --workspace | grep -i rdkafka` → exactly one crate,
  `movement-relay`, appears.
- [ ] `helm template charts/distant-signal > /dev/null` — confirm this now
  renders with **no** `--set` at all beyond whatever this chart already
  required before this entire plan started (no `trustConsumer.kafka.*`
  requirement left).
- [ ] Grep confirmation, same as Task 12: `grep -rn
  "LineDefinition\|full_coverage_enabled" crates/movement-relay
  crates/movement-feed` still returns nothing.
- [ ] Note in the PR/commit: `full-coverage-consumer`'s made-up group
  (`distant-signal-full-coverage-consumer`) cleanup, per the design doc's
  Decision 6 — confirmed to be a non-issue with no cleanup task, since it
  never successfully joined the real cluster (`GroupAuthorizationFailed`
  at the ACL-check stage, before any group state is persisted). No code
  task exists for this; this line is a documentation confirmation only.

No commit for this task — it's a gate confirming Deploy C is genuinely
complete.

---

## Open items a human should still decide, not resolved by this plan

- **The real `TRAIN_MVT_ALL_TOC` peak-vs-average message rate and RDM's
  own `retention.ms`** (design doc Open Questions 1–2) remain unresearched
  — `MAXLEN=500,000` is a starting figure this plan does not independently
  validate against real production lag data. Worth revisiting once Deploy
  B has run for a real period and `movement_relay_stream_lag` has real
  data behind it.
- **Redis's own single-instance, unhardened posture** (design doc
  Non-goals) is a new real dependency this plan introduces without
  hardening. Worth a dedicated follow-up once `movement-events`'s real
  write volume is observed in production.
- **Whether RDM exposes any consumer-group visibility** (design doc Open
  Question 7) is unconfirmed — the Deploy B runbook above treats this as
  an optional corroborating check, never a hard requirement, precisely
  because this plan doesn't know whether it exists.
- **The exact "observation window" length before declaring B3 stable**
  (Deploy B runbook, B3 step 6) is deliberately left to the operator's
  judgment at the time, not a fixed number invented here.
- **Whether `trust-consumer`'s existing `#[allow(dead_code)]`-flagged
  unused `Activation`/`Movement`/`Cancellation` fields (design doc's "Ground
  truth" section) are worth trimming now that a byte-faithful relay makes
  their preservation less obviously load-bearing** — explicitly not
  addressed by this plan; the design doc's own Non-goals already rules
  this out, restated here only so it isn't silently revisited during
  implementation.
