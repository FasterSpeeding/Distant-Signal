# Rust Service Deduplication — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan
> task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Six ordered task groups, one plan, matching the spec's own §5
> sequencing** — the same "ordered task groups within one plan" structure
> `docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md`'s
> Part 1 used, cited by the spec itself as this plan's structural
> precedent. Every group here is normal, in-repo, buildable/testable work —
> unlike that plan's Part 2, nothing here needs a human or a second
> repository. Groups A→D→E must run in that relative order (E is
> hard-blocked on D finishing first); Group F is fully independent and may
> run at any point, including in parallel with A–E.

**Goal:** implement
`docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md`
(the approved design spec for findings #1–#6 of the workspace-wide Rust
crate-duplication review) end to end: extend `common` with shared poller-loop
scaffolding, config-arg structs, and a line-catalogue type; stand up one new
narrow crate for shared health/readiness HTTP plumbing; fold `ActiveFeed`
into `movement-feed`; and close the two remaining `common::ingest` GET/POST
gaps. Zero behavior change anywhere: no CLI flag, env var, default value, or
wire-visible string (metric name, gauge name, `/healthz` body text) changes
for any consumer.

**Architecture:** `common` gains `tokio` (promoted from dev- to a real
dependency, already carries the `time` feature it needs), `clap`
(`derive`, `env`), and `metrics` (0.24 — a fresh-verification addition this
plan makes that the spec's own migration-mechanics section missed, see
"Fresh-verification corrections" below) as real dependencies, and hosts
`InternalOAuthArgs`/`MetricsArgs`/`KafkaConnectionArgs`, `LineCatalogue`, and
`run_poll_loop` directly. A new crate, `health-http` (this plan's resolution
of the spec's Open Question 4 — see below), holds the
`ConnectionState`/`spawn`/`spawn_with_state`/`set_connected`/`healthz`
machinery `trust-consumer`, `full-coverage-consumer`, and `movement-relay`
each hand-duplicated. `movement-feed` gains a generic `ActiveFeed<K>` plus
`MovementFeedBackend` (this plan's resolution of a real gap the spec's §3.5
did not actually close — see below), replacing the two crates' byte-for-byte
copies. `common::ingest` gains `get_json`/`post_json`, closing the last two
duplicated HTTP-wrapper shapes.

**Spec:**
`docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md` —
authoritative for every architectural decision below (no grab-bag crate, the
exact scope-per-item table in its §2, the six-group sequencing in its §5,
the `metrics_port`/`kafka_consumer_group` per-crate exclusions in its §3.2).
This plan does not re-argue any of those decisions; it only resolves the
five items the spec explicitly left open (its own "Open Questions" section)
and two additional gaps this plan's own fresh re-verification found in the
spec's migration mechanics (flagged explicitly, not silently patched).

## Global Constraints

- **Zero CLI flag renames, zero env var renames, zero default value changes,
  anywhere, for any of the six groups below.** `#[command(flatten)]`
  preserves an embedded struct's own field-derived `--long-flag`/`ENV_VAR`
  names exactly, as long as the shared struct's field names are copied
  verbatim from today's `Config` fields (every task below does this). Every
  task that flattens a config block into a shared struct ends with a
  verification step that greps or renders the real `--help` output, or
  cross-checks `docker-compose.yml`/`charts/distant-signal/templates/*.yaml`,
  to confirm the exact flag/env names are unchanged.
- **`metrics_port` and `kafka_consumer_group` stay per-crate `Config`
  fields, never folded into a shared struct.** The spec's §3.2 found real,
  cited, per-field reasons: `metrics_port`'s default genuinely differs per
  crate (`9091`/`9092`/`9093`/`9095`) and `docker-compose.yml` relies on the
  code-level default (never sets `METRICS_PORT`); `kafka_consumer_group`
  differs in defaultedness itself (`movement-relay` deliberately has *no*
  default, per its own documented "unforgeable identity" stance,
  `movement-relay/src/config.rs:16-24`). Do not "fix" this by unifying them.
- **Wire-visible strings stay byte-for-byte identical.** `health-http`'s
  `healthz` response text and `set_connected`'s gauge name both become
  function *parameters* instead of hardcoded per-crate literals, but every
  real caller passes the exact string it emits today.
- **rustc floor: 1.88** (`docker/poller-incidents.Dockerfile:4-12`'s own
  build-log-derived comment — the actual floor is the `icu_*` transitive
  crates pulled in via `reqwest → url → idna → idna_adapter`, not the
  edition-2024 minimum of 1.85). Edition: `2024` everywhere already. Every
  new/changed crate in this plan targets both.
- **`cargo fmt` scoping — read this before every task's verification
  step.** Never run an unscoped `cargo fmt` or `cargo fmt --all` against
  this workspace. Two prior implementation passes this session already had
  `cargo fmt`'s workspace-wide form silently reformat unrelated files
  outside the task's own diff. Every task below that needs formatting
  applies `rustfmt --edition 2024 <exact files this task touched>` (listed
  per task) — never a bare `cargo fmt`/`cargo fmt --all`. `cargo fmt -p
  <crate>` is acceptable only when a task's own diff is the *only* pending
  formatting delta in that entire crate (true for the new-crate-scaffolding
  tasks in Group A; not assumed true anywhere existing code is touched).
- **No new dependencies beyond what each task states explicitly.** In
  particular: do not add `axum` to `common` (matches the spec's own
  Decision 1 and `common/src/metrics.rs:1-9,51-56`'s existing, documented
  no-axum stance — unchanged by this plan).
- **File scope.** Only the files named in each task's own "Files" line
  change. No `lines/*.toml` changes anywhere in this plan. No Helm chart or
  `docker-compose.yml` edits — Groups B/D's verification steps *read* those
  files to confirm names are unchanged, they do not edit them.
- **Tier 3 (#7–#11 of the original review) stays untouched.** Not
  re-litigated, not folded in: `feed/kafka.rs`'s `KafkaMovementFeed` copies
  (scheduled for deletion in Deploy C, not this plan), the
  `distant_signal_*_errors_total` hand-written counter sites, the `PgPool`
  construction sites, the Redis `XINFO GROUPS` field-walk copies, `api`'s
  ingest-wrapper repetition.
- **No new dedup targets invented.** `aggregator`/`enricher`/`notifier`'s
  own bootstrap lines (`dotenv::dotenv().ok()` +
  `tracing_subscriber::fmt()...init()`) are out of scope, per the spec's own
  §3.1 and Non-goals.

## Open Questions — Resolved

The spec's own "Open Questions" section left five items for this plan
stage. Resolved here, each checked against the real current repo state
rather than guessed:

1. **Module path for the new `common` types.** `crates/common/src/oauth_client.rs`
   gains `InternalOAuthArgs` + its `token_cache()` method (colocated with
   `OAuthCredentials`/`OAuthTokenCache`, which it wraps — keeps a single
   file owning the whole OAuth2 client-credentials story). A new
   `crates/common/src/service_args.rs` holds `MetricsArgs` and
   `KafkaConnectionArgs` (keeps `oauth_client.rs` free of anything
   Kafka-specific, and keeps `clap` imports out of `oauth_client.rs` beyond
   what `InternalOAuthArgs` itself needs). A new `crates/common/src/config.rs`
   holds `LineCatalogue` + `parse_lines` — a new module rather than growing
   `lib.rs` further, matching `common`'s existing pattern of one file per
   concern (`ingest.rs`, `metrics.rs`, `oauth_client.rs`, `rail_day.rs` are
   all already split out; `lib.rs` itself holds only struct definitions +
   `pub mod` lines, no behavior).
2. **`run_poll_loop`'s cycle-parameter shape: `FnMut() -> Fut` vs.
   `AsyncFnMut`.** Decision: `F: FnMut() -> Fut, Fut: Future<Output =
   anyhow::Result<()>>` — not `AsyncFnMut`. Confirmed via
   `grep -rn "AsyncFnMut" crates/` that this workspace has zero existing
   precedent for async-closure trait bounds anywhere; `FnMut() -> Fut` is
   the long-established, universally-understood idiom, composes trivially
   with a plain `async fn` reference (every poller's own `poll_once` is
   already exactly this shape), and `poller-tfl`'s one genuinely different
   case (capturing `&mut dlr_state` across repeated calls) is completely
   ordinary `FnMut` semantics — no need to reach for the newer, less-tooled
   `AsyncFnMut` sugar (stable since 1.85, still comparatively new against
   this workspace's confirmed 1.88 floor) to solve a problem `FnMut` already
   solves cleanly.
3. **Aligning `clap`'s version pin workspace-wide.** Decision: **no.**
   Re-checked: the spec's own §4 migration mechanics lists which
   `Cargo.toml`s this work actually touches, and it is only `common`'s (new
   `clap` dependency), the new `health-http`'s, and `movement-feed`'s (new
   `clap` dependency, a gap this plan also found — see below). None of the
   9 crates whose `clap` pin is `4.6.1` today have their `Cargo.toml`
   touched by this plan at all (confirmed: they already depend on `clap`
   and `common`, and gain no new dependency per §4). Bumping their pins
   anyway would mean editing 9 files this work otherwise has zero reason to
   touch, for a patch-version-only change Cargo's own resolver already
   unifies to one real compiled version across the whole workspace lockfile
   regardless of each `Cargo.toml`'s stated range — pure diff-surface cost,
   zero functional benefit. `common`'s own new `clap` dependency is pinned
   to `4.6.6` (the highest currently used, matching its closest siblings
   `trust-consumer`/`full-coverage-consumer`/`movement-relay`) purely
   because it's a brand-new line in a file this plan touches anyway, not as
   part of a workspace-wide alignment effort.
4. **The new crate's name.** `health-http` — a concrete, two-word noun
   phrase matching this workspace's existing convention
   (`movement-feed`, `trust-schema`, `schedule-query`), naming exactly what
   the crate contains (a health/readiness HTTP endpoint module), not ending
   in "common"/"shared". Used consistently as the package name, path
   dependency key, and Rust crate name (`health_http::`) throughout this
   plan.
5. **`common::ingest::fetch_last_fetched` rewritten in terms of the new
   `get_json`?** Decision: **yes**, as a drive-by cleanup, in the same
   commit as `get_json`'s own addition (Group F, Task F1) — same file, same
   review pass, trivial, and it removes one more near-duplicate of the
   GET+bearer+deserialize shape `get_json` exists to name once.

**Fresh-verification corrections to the spec** (not open questions the spec
posed — gaps this plan's own re-verification found while grounding tasks
against real line numbers, flagged explicitly per this document's own
citation discipline rather than silently patched):

- **`common` needs a new real dependency on `metrics = "0.24"` (the metrics
  *facade* crate), not just `tokio`/`clap`.** The spec's §4 "Cargo.toml
  changes" for `common` lists only `tokio`/`clap`. But `run_poll_loop`
  (moved into `common::poller_loop`, Group A Task A6) must call
  `metrics::histogram!`/`metrics::counter!` — the exact macros every
  poller's own `main.rs` already calls today. Confirmed via
  `crates/common/Cargo.toml`: it depends on `metrics-exporter-prometheus`
  (the *recorder* crate) but never the bare `metrics` facade crate itself,
  because `common/src/metrics.rs` only *installs* a recorder, it never
  emits a metric. Task A2 below adds this dependency; every one of the 5
  poller crates already depends on `metrics = "0.24"` directly (unaffected).
- **`movement-feed` needs a new real dependency on `clap` (`derive`
  feature only, no `env`), not just `health-http`.** The spec's §4 lists
  only `service-health` (now `health-http`) as `movement-feed`'s new
  dependency. But moving `MovementFeedBackend` (a `#[derive(clap::ValueEnum)]`
  enum) into `movement-feed` (Group E, Task E1) requires `clap`'s `derive`
  feature to be available in that crate; confirmed via
  `crates/movement-feed/Cargo.toml` that it has no `clap` dependency today.
- **`ActiveFeed` cannot move into `movement-feed` as a concrete, non-generic
  enum, contrary to the spec's §3.5 claim.** The spec states "every other
  type `ActiveFeed` touches (`KafkaMovementFeed`, `RedisStreamMovementFeed`,
  `GapInfo`) is already imported from the shared `movement-feed` crate
  today." Re-checked against real source: `crates/trust-consumer/src/feed/mod.rs:9`
  declares `pub mod kafka;` (crate-local), and
  `crates/trust-consumer/src/main.rs:21` imports
  `use feed::kafka::KafkaMovementFeed;` — a crate-local type, **not**
  `movement_feed::...` — only `GapInfo`/`RedisStreamMovementFeed` actually
  come from the shared crate. `full-coverage-consumer` has its own,
  separate `KafkaMovementFeed` the same way. Sharing `KafkaMovementFeed`
  itself is out of scope (it's Tier 3 finding #7, scheduled for deletion in
  Deploy C, not this plan) and picking one crate's own type arbitrarily
  would be worse than the status quo. **Fix: make `ActiveFeed` generic over
  its Kafka variant** — `enum ActiveFeed<K: MovementFeed> { Kafka(K),
  RedisStream(Box<RedisStreamMovementFeed>, health_http::ConnectionState,
  &'static str) }` (the trailing `&'static str` on `RedisStream` is a second,
  smaller fix: `set_connected`'s gauge name is per-caller, so it must be
  threaded through at construction time rather than hardcoded inside a
  now-shared `next_batch` impl). Each caller instantiates its own
  `ActiveFeed<KafkaMovementFeed>` (their own crate-local type as the
  generic argument) with zero change to its own `KafkaMovementFeed`. See
  Task E1 for the full design.

## Non-goals

- **Tier 3 (#7–#11).** Not touched (Global Constraints, above).
- **Inventing new deduplication targets** beyond the six findings this
  spec scoped (Global Constraints, above).
- **Unifying `metrics_port` or `kafka_consumer_group` across crates.**
  Deliberately kept per-crate everywhere in this plan (Global Constraints).
- **Aligning `clap`/`axum`/`tokio` version pins workspace-wide.** Resolved
  above (Open Question 3) as a deliberate "no" for this pass, beyond the
  minimum needed for the new/changed crates to compile.
- **Renaming any CLI flag, env var, metric name, or gauge name.** Every
  task's verification step confirms this explicitly.
- **Deleting `KafkaMovementFeed`, or otherwise touching Deploy C's own
  planned Kafka-side cleanup.** `ActiveFeed`'s `Kafka(K)` variant stays
  generic specifically so this plan does not need to touch that code at all.
- **A `docker build` CI job change.** This plan's final verification asks
  whoever executes it to run one real `docker build` for a representative
  poller image as a manual confirmation, not to add a new CI step.

---

# Group A — Foundational `common`/new-crate scaffolding

Purely additive: nothing existing changes behavior. Every new item
(`InternalOAuthArgs`, `MetricsArgs`, `KafkaConnectionArgs`, `LineCatalogue`,
`run_poll_loop`, the whole `health-http` crate) compiles and has its own
tests, but nothing outside `common`/`health-http` references any of it yet.

## Task A1: Pre-change baseline — full workspace build/test/lint

**Files:** none (verification-only task; its output is a reference point
for every later group's own verification, not a diff).

Run this **before any other task in this plan starts**, on the current,
unmodified tree, so every later "still passes" claim has something concrete
to diff against.

- [ ] **Step 1: Full workspace build**

```bash
cargo build --workspace 2>&1 | tee /tmp/dedup-baseline-build.log
```

  Expected: succeeds (this is the tree's current, working state — a failure
  here means something in the environment is broken independent of this
  plan; stop and investigate before proceeding).

- [ ] **Step 2: Full workspace test**

```bash
cargo test --workspace 2>&1 | tee /tmp/dedup-baseline-test.log
```

  Record the pass/fail count per crate. This plan's later per-crate
  `cargo test -p <crate>` steps should show the same or a growing count for
  that crate (growing where a task adds tests), never shrinking.

- [ ] **Step 3: Full workspace clippy**

```bash
cargo clippy --workspace --all-features --all-targets -- -D warnings 2>&1 | tee /tmp/dedup-baseline-clippy.log
```

- [ ] **Step 4: fmt check (read-only, no write)**

```bash
cargo fmt --all -- --check 2>&1 | tee /tmp/dedup-baseline-fmt.log
```

  Record whether this already reports any pre-existing drift unrelated to
  this plan (if so, note it — later tasks must not "fix" unrelated
  pre-existing drift as a side effect of their own scoped `rustfmt` calls,
  since that would violate this plan's own fmt-scoping constraint).

- [ ] **Step 5: `helm lint` baseline**

```bash
helm lint charts/distant-signal
```

  Expected: passes today; this plan makes no chart changes, so this must
  still pass identically at the end (Final Verification, below).

No commit for this task — it produces reference logs only, not a diff.

---

## Task A2: `common/Cargo.toml` — promote `tokio`, add `clap` + `metrics`

**Files:** modify `crates/common/Cargo.toml`.

- [ ] **Step 1: Promote `tokio` from dev- to a real dependency.** Current
  (`crates/common/Cargo.toml`):

```toml
[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
chrono-tz = "0.10"
glob = "0.3.3"
metrics-exporter-prometheus = { version = "0.18", default-features = false, features = ["http-listener"] }
reqwest = { version = "0.13.4", default-features = false, features = ["json", "form", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
serde-inline-default = "1.0.1"
serde_json = "1.0.149"
serde_repr = "0.1.20"
toml = { version = "1.1.2", features = ["serde"] }
tracing = "0.1.44"

[dev-dependencies]
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros", "time"] }
wiremock = "0.6"
```

  Change to:

```toml
[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
chrono-tz = "0.10"
clap = { version = "4.6.6", features = ["derive", "env"] }
glob = "0.3.3"
metrics = "0.24"
metrics-exporter-prometheus = { version = "0.18", default-features = false, features = ["http-listener"] }
reqwest = { version = "0.13.4", default-features = false, features = ["json", "form", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
serde-inline-default = "1.0.1"
serde_json = "1.0.149"
serde_repr = "0.1.20"
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros", "time"] }
toml = { version = "1.1.2", features = ["serde"] }
tracing = "0.1.44"

[dev-dependencies]
wiremock = "0.6"
```

  `tokio`'s version/features are carried over unchanged (it already has the
  `time` feature `common::poller_loop` needs — no feature-set change, only
  a section move). `clap` is pinned `4.6.6` (Open Question 3, above).
  `metrics` is the fresh-verification addition (above) `run_poll_loop`
  needs for its `histogram!`/`counter!` calls.

- [ ] **Step 2: Verify — this crate alone, not the workspace.**

```bash
cargo build -p common
```

  Expected: succeeds (nothing yet references the new dependencies, so this
  only proves the manifest itself is valid).

- [ ] **Step 3: Format and commit**

```bash
rustfmt --edition 2024 crates/common/Cargo.toml 2>/dev/null || true  # Cargo.toml isn't Rust; nothing to format, this step is a no-op — do not run cargo fmt on this task
git add crates/common/Cargo.toml
git commit -m "common: promote tokio to a real dependency, add clap + metrics"
```

---

## Task A3: `common::oauth_client` — add `InternalOAuthArgs` + `token_cache()`

**Files:** modify `crates/common/src/oauth_client.rs`.

**Interfaces produced:** `pub struct InternalOAuthArgs { pub
internal_oauth_token_url: String, pub internal_oauth_client_id: String, pub
internal_oauth_scope: String, pub internal_oauth_username: String, pub
internal_oauth_password: String }`, `impl InternalOAuthArgs { pub fn
token_cache(&self) -> OAuthTokenCache }`. Group B's 9 consumer crates rely on
this exact struct/method shape.

- [ ] **Step 1: Add `use clap::Parser;`** to the top of
  `crates/common/src/oauth_client.rs` (alongside the existing `use
  std::sync::Mutex;` / `use std::time::{Duration, Instant};` / `use
  serde::Deserialize;` block at lines 17-20).

- [ ] **Step 2: Add `InternalOAuthArgs` + `token_cache()`**, inserted after
  `OAuthTokenCache`'s closing `}` (currently line 133) and before the
  `#[cfg(test)]` module (currently line 135):

```rust
/// Every real caller's own copy of the 5 `internal_oauth_*` CLI/env flags
/// (identical field names, types, and the one real default —
/// `internal_oauth_scope`'s `"groups"` — across all 9 real callers,
/// confirmed byte-for-byte identical in
/// docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md
/// §3.2). `#[command(flatten)]` this into a `Config` struct to gain these
/// 5 flags with their existing `--internal-oauth-*`/`INTERNAL_OAUTH_*`
/// names unchanged.
#[derive(Debug, Clone, clap::Args)]
pub struct InternalOAuthArgs {
    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    /// This service's own Authentik service-account credential --
    /// per-service, distinct from every other caller's.
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,
}

impl InternalOAuthArgs {
    /// Builds the `OAuthTokenCache` every real caller previously
    /// hand-constructed identically at its own call site (9 byte-for-byte
    /// copies of `OAuthTokenCache::new(OAuthCredentials { ... })`).
    pub fn token_cache(&self) -> OAuthTokenCache {
        OAuthTokenCache::new(OAuthCredentials {
            token_url: self.internal_oauth_token_url.clone(),
            client_id: self.internal_oauth_client_id.clone(),
            scope: self.internal_oauth_scope.clone(),
            username: self.internal_oauth_username.clone(),
            password: self.internal_oauth_password.clone(),
        })
    }
}
```

- [ ] **Step 3: Add a unit test** inside the existing `#[cfg(test)] mod
  tests` block (after `credentials()`'s helper, before the first `#[tokio::test]`):

```rust
    #[test]
    fn token_cache_builds_from_the_flattened_args_unchanged() {
        let args = InternalOAuthArgs {
            internal_oauth_token_url: "http://auth.invalid/token".to_string(),
            internal_oauth_client_id: "distant-signal-internal".to_string(),
            internal_oauth_scope: "groups".to_string(),
            internal_oauth_username: "svc-test".to_string(),
            internal_oauth_password: "app-password".to_string(),
        };
        // token_cache() itself has no externally observable state beyond
        // constructing an OAuthTokenCache -- this just confirms it doesn't
        // panic and produces a real cache (get_token's own network-hitting
        // behavior is already covered by OAuthTokenCache's existing tests
        // above, which this method threads through unchanged).
        let _cache = args.token_cache();
    }
```

- [ ] **Step 4: Verify**

```bash
cargo build -p common
cargo test -p common
cargo clippy -p common --all-features -- -D warnings
rustfmt --edition 2024 crates/common/src/oauth_client.rs
git diff --stat crates/common/src/oauth_client.rs   # confirm rustfmt touched only this file
```

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/oauth_client.rs
git commit -m "common: add InternalOAuthArgs + token_cache(), shared OAuth2 client-credentials config"
```

---

## Task A4: `common::service_args` (new file) — `MetricsArgs` + `KafkaConnectionArgs`

**Files:** create `crates/common/src/service_args.rs`; modify
`crates/common/src/lib.rs`.

**Interfaces produced:** `pub struct MetricsArgs { pub metrics_enabled: bool
}`, `pub struct KafkaConnectionArgs { pub kafka_brokers: String, pub
kafka_topic: String, pub kafka_sasl_username: String, pub
kafka_sasl_password: String, pub kafka_sasl_mechanism: String }` (5 fields —
`kafka_consumer_group` deliberately excluded, Global Constraints).

- [ ] **Step 1: Create `crates/common/src/service_args.rs`**

```rust
//! Shared `clap::Args` sub-structs for CLI/env config blocks that are
//! byte-identical (or identical apart from one deliberately-excluded
//! per-crate field) across multiple binaries. See
//! docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md
//! §3.2 for the full per-field verification.

/// `metrics_enabled` only -- NOT `metrics_port`, whose default genuinely
/// differs per crate (`9091`/`9092`/`9093`/`9095`) and which
/// `docker-compose.yml` relies on the code-level default for. Every real
/// caller's `metrics_enabled` default is `true`, confirmed identical
/// across all 9.
#[derive(Debug, Clone, clap::Args)]
pub struct MetricsArgs {
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}

/// 5 of the 6 Kafka connection fields shared by `trust-consumer`,
/// `full-coverage-consumer`, and `movement-relay` -- NOT
/// `kafka_consumer_group`, which stays a per-crate field: two of the three
/// crates default it to a distinct per-deployment string, and the third
/// (`movement-relay`) deliberately has no default at all (a fixed,
/// externally-issued, unforgeable identity -- see
/// `crates/movement-relay/src/config.rs:16-24`'s own comment). All 5
/// fields below are `#[arg(long, env)]` with no default (required) in
/// every one of the 3 real callers today -- flattening changes nothing
/// about defaultedness or requiredness.
#[derive(Debug, Clone, clap::Args)]
pub struct KafkaConnectionArgs {
    #[arg(long, env)]
    pub kafka_brokers: String,
    #[arg(long, env)]
    pub kafka_topic: String,
    #[arg(long, env)]
    pub kafka_sasl_username: String,
    #[arg(long, env)]
    pub kafka_sasl_password: String,
    #[arg(long, env)]
    pub kafka_sasl_mechanism: String,
}
```

- [ ] **Step 2: Wire into `lib.rs`.** Current
  (`crates/common/src/lib.rs:11-14`):

```rust
pub mod ingest;
pub mod metrics;
pub mod oauth_client;
pub mod rail_day;
```

  Change to:

```rust
pub mod config;
pub mod ingest;
pub mod metrics;
pub mod oauth_client;
pub mod rail_day;
pub mod service_args;
```

  (`pub mod config;` is added here too, in anticipation of Task A5 —
  adding both `pub mod` lines in one pass avoids a second one-line edit to
  the same block; `config.rs` itself doesn't exist until Task A5, so this
  step alone would fail to compile in isolation — do Task A5 immediately
  after this step, before verifying, or add only `pub mod service_args;`
  here and let Task A5 add its own line. This plan does the latter — see
  Task A5 Step 2 for the actual `config` line.)

  Revised Step 2 (do this instead, to keep A4 independently buildable):

```rust
pub mod ingest;
pub mod metrics;
pub mod oauth_client;
pub mod rail_day;
pub mod service_args;
```

- [ ] **Step 3: Unit tests** — add a `#[cfg(test)] mod tests` block at the
  bottom of `service_args.rs`:

```rust
#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct TestConfig {
        #[command(flatten)]
        metrics: MetricsArgs,
        #[command(flatten)]
        kafka: KafkaConnectionArgs,
    }

    #[test]
    fn metrics_args_flatten_preserves_flag_names_and_default() {
        let config = TestConfig::try_parse_from([
            "test",
            "--kafka-brokers",
            "b",
            "--kafka-topic",
            "t",
            "--kafka-sasl-username",
            "u",
            "--kafka-sasl-password",
            "p",
            "--kafka-sasl-mechanism",
            "PLAIN",
        ])
        .expect("only the required Kafka args should be needed");
        assert!(
            config.metrics.metrics_enabled,
            "metrics_enabled's default must stay true when --metrics-enabled is omitted"
        );
    }

    #[test]
    fn kafka_connection_args_requires_all_five_fields() {
        let result = TestConfig::try_parse_from(["test"]);
        assert!(
            result.is_err(),
            "all 5 Kafka fields are required (no default) -- omitting them must fail to parse"
        );
    }
}
```

- [ ] **Step 4: Verify**

```bash
cargo build -p common
cargo test -p common
cargo clippy -p common --all-features -- -D warnings
rustfmt --edition 2024 crates/common/src/service_args.rs crates/common/src/lib.rs
```

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/service_args.rs crates/common/src/lib.rs
git commit -m "common: add MetricsArgs + KafkaConnectionArgs shared config structs"
```

---

## Task A5: `common::config` (new file) — `LineCatalogue` + `parse_lines`

**Files:** create `crates/common/src/config.rs`; modify
`crates/common/src/lib.rs`.

**Interfaces produced:** `pub struct LineCatalogue(pub Vec<LineDefinition>)`
(with `Deref<Target = Vec<LineDefinition>>`), `pub fn parse_lines(path: &str)
-> anyhow::Result<LineCatalogue>`.

- [ ] **Step 1: Create `crates/common/src/config.rs`**

```rust
//! Shared `--lines-dir` line-catalogue loader. Previously 4 byte-identical
//! (or near-identical) copies across `aggregator`, `api`,
//! `full-coverage-consumer`, and `schedule-reference` -- see
//! docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md
//! §3.4.

use std::path::PathBuf;

use crate::LineDefinition;

/// Newtype around the parsed line catalogue.
///
/// `clap_derive` infers the type it downcasts an `ArgMatches` entry to from
/// the field's *syntactic* shape, not from the `value_parser`'s `Value`
/// type: a bare `Vec<LineDefinition>` field is always treated as "one
/// `LineDefinition` per CLI occurrence, collected via `ArgAction::Append`"
/// -- this panics at runtime ("Mismatch between definition and access of
/// `lines`") the moment `--lines-dir`/`LINES_DIR`/`default_value` actually
/// supplies a value. `parse_lines` instead produces the *entire* vec from a
/// single `--lines-dir` occurrence, so the field type must not look like
/// `Vec<T>` to the derive macro. This newtype (plus `Deref`) sidesteps
/// that -- every existing call site that treated a local `LineCatalogue`
/// as `&[LineDefinition]` continues to work unchanged.
#[derive(Debug, Clone, Default)]
pub struct LineCatalogue(pub Vec<LineDefinition>);

impl std::ops::Deref for LineCatalogue {
    type Target = Vec<LineDefinition>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub fn parse_lines(path: &str) -> anyhow::Result<LineCatalogue> {
    LineDefinition::from_dir(&PathBuf::from(path)).map(LineCatalogue)
}
```

- [ ] **Step 2: Wire into `lib.rs`.** Building on Task A4's change, current:

```rust
pub mod ingest;
pub mod metrics;
pub mod oauth_client;
pub mod rail_day;
pub mod service_args;
```

  Change to:

```rust
pub mod config;
pub mod ingest;
pub mod metrics;
pub mod oauth_client;
pub mod rail_day;
pub mod service_args;
```

- [ ] **Step 3: Unit test** — add to `config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lines_rejects_a_nonexistent_directory() {
        // Mirrors the existing per-crate copies' own implicit contract:
        // LineDefinition::from_dir's own error surfaces unchanged through
        // this shared wrapper.
        let result = parse_lines("/nonexistent/path/that/should/not/exist");
        assert!(result.is_err());
    }

    #[test]
    fn line_catalogue_derefs_to_the_inner_vec() {
        let catalogue = LineCatalogue(vec![]);
        assert_eq!(catalogue.len(), 0);
    }
}
```

- [ ] **Step 4: Verify**

```bash
cargo build -p common
cargo test -p common
cargo clippy -p common --all-features -- -D warnings
rustfmt --edition 2024 crates/common/src/config.rs crates/common/src/lib.rs
```

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/config.rs crates/common/src/lib.rs
git commit -m "common: add LineCatalogue + parse_lines, shared --lines-dir loader"
```

---

## Task A6: `common::poller_loop` (new file) — `run_poll_loop`

**Files:** create `crates/common/src/poller_loop.rs`; modify
`crates/common/src/lib.rs`.

**Interfaces produced:** `pub async fn run_poll_loop<F, Fut>(poller_label:
&'static str, client: &reqwest::Client, api_ingest_url: &str,
internal_oauth: &crate::oauth_client::OAuthTokenCache, poll_interval:
std::time::Duration, metrics_enabled: bool, metrics_port: u16, cycle: F) ->
anyhow::Result<()> where F: FnMut() -> Fut, Fut: std::future::Future<Output
= anyhow::Result<()>>`. Group C's 5 pollers call this directly from their own
`main()`.

- [ ] **Step 1: Create `crates/common/src/poller_loop.rs`**

```rust
//! Shared poller `main()` loop scaffolding: install metrics (if enabled),
//! compute the first-tick delay via `ingest::time_until_next_poll`, then
//! loop forever recording `poller_cycle_duration_seconds`/
//! `poller_cycle_total` and logging cycle errors. Previously duplicated,
//! byte-identical apart from one metric label string, across
//! `poller-incidents`/`poller-stations`/`poller-tocs`/`poller-ldbws`/
//! `poller-tfl`'s own `main()` functions -- see
//! docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md
//! §3.1.
//!
//! `poll_once` and any pre-flight check stay in each poller's own
//! `main.rs`, called as today -- only this wrapper is shared. A poller
//! with cycle-to-cycle mutable state (`poller-tfl`'s own `DlrMatchState`)
//! keeps owning that state in its own `main()` and captures it by mutable
//! reference in the `cycle` closure passed in here -- ordinary `FnMut`
//! semantics, no change needed to this function's own signature.

use std::future::Future;
use std::time::Duration;

use crate::ingest;
use crate::oauth_client::OAuthTokenCache;

pub async fn run_poll_loop<F, Fut>(
    poller_label: &'static str,
    client: &reqwest::Client,
    api_ingest_url: &str,
    internal_oauth: &OAuthTokenCache,
    poll_interval: Duration,
    metrics_enabled: bool,
    metrics_port: u16,
    mut cycle: F,
) -> anyhow::Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    if metrics_enabled {
        crate::metrics::install(metrics_port)?;
    }

    let delay =
        ingest::time_until_next_poll(client, api_ingest_url, internal_oauth, poll_interval).await;
    if !delay.is_zero() {
        tracing::info!(
            delay_secs = delay.as_secs(),
            "data still fresh from a prior run; delaying first poll"
        );
    }
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + delay, poll_interval);

    loop {
        interval.tick().await;

        let cycle_start = std::time::Instant::now();
        let result = cycle().await;
        metrics::histogram!(
            crate::metrics::metric_name("poller_cycle_duration_seconds"),
            "poller" => poller_label
        )
        .record(cycle_start.elapsed().as_secs_f64());
        metrics::counter!(
            crate::metrics::metric_name("poller_cycle_total"),
            "poller" => poller_label,
            "result" => if result.is_ok() { "success" } else { "failure" }
        )
        .increment(1);

        if let Err(err) = result {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}
```

- [ ] **Step 2: Wire into `lib.rs`.** Add `pub mod poller_loop;` to the
  block edited in Tasks A4/A5, alphabetically ordered:

```rust
pub mod config;
pub mod ingest;
pub mod metrics;
pub mod oauth_client;
pub mod poller_loop;
pub mod rail_day;
pub mod service_args;
```

- [ ] **Step 3: Unit test** — this function loops forever by design, so a
  direct end-to-end test would hang; test its two extractable behaviors
  instead, matching this crate's existing posture toward
  `time_until_next_poll` (tested via its own pure helper, not the async
  wrapper). Add to `poller_loop.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;
    use crate::oauth_client::{OAuthCredentials, OAuthTokenCache};

    async fn token_cache(server: &MockServer) -> OAuthTokenCache {
        Mock::given(method("POST"))
            .and(path("/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fake-jwt",
                "expires_in": 300,
            })))
            .mount(server)
            .await;
        OAuthTokenCache::new(OAuthCredentials {
            token_url: format!("{}/token/", server.uri()),
            client_id: "test".to_string(),
            scope: "groups".to_string(),
            username: "test".to_string(),
            password: "test".to_string(),
        })
    }

    /// Not a full loop run (this function never returns) -- confirms the
    /// cycle closure is actually invoked and its result recorded, by
    /// racing the loop against a timeout and asserting at least one
    /// invocation happened. Mirrors this crate's existing
    /// `time_until_next_poll` tests' preference for a real (mocked) HTTP
    /// round trip over a fake clock abstraction.
    #[tokio::test]
    async fn run_poll_loop_invokes_the_cycle_closure_on_each_tick() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "fetchedAt": null
            })))
            .mount(&server)
            .await;
        let tokens = token_cache(&server).await;
        let client = reqwest::Client::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_cycle = Arc::clone(&call_count);

        let loop_future = run_poll_loop(
            "test",
            &client,
            &format!("{}/ingest", server.uri()),
            &tokens,
            Duration::from_millis(10),
            false,
            0,
            || {
                call_count_for_cycle.fetch_add(1, Ordering::Relaxed);
                async { Ok(()) }
            },
        );

        let _ = tokio::time::timeout(Duration::from_millis(100), loop_future).await;
        assert!(
            call_count.load(Ordering::Relaxed) >= 1,
            "the cycle closure must run at least once within 100ms at a 10ms interval"
        );
    }
}
```

- [ ] **Step 4: Verify**

```bash
cargo build -p common
cargo test -p common
cargo clippy -p common --all-features -- -D warnings
rustfmt --edition 2024 crates/common/src/poller_loop.rs crates/common/src/lib.rs
```

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/poller_loop.rs crates/common/src/lib.rs
git commit -m "common: add run_poll_loop, shared poller main() scaffolding"
```

---

## Task A7: Scaffold the new `health-http` crate

**Files:** create `crates/health-http/Cargo.toml`,
`crates/health-http/src/lib.rs`; modify root `Cargo.toml`.

**Interfaces produced:** `pub type ConnectionState = Arc<AtomicBool>;`,
`pub fn spawn(bind_url: String, healthy_text: &'static str, unhealthy_text:
&'static str) -> ConnectionState`, `pub fn spawn_with_state(bind_url:
String, state: ConnectionState, healthy_text: &'static str, unhealthy_text:
&'static str)`, `pub fn set_connected(state: &ConnectionState, gauge_name:
&str, connected: bool)`. Not wired into any consumer yet — that's Group D.

- [ ] **Step 1: Add the new member to the root `Cargo.toml`.** Current:

```toml
[workspace]
resolver = "2"
members = [
    "crates/common",
    "crates/api",
    ...
    "crates/movement-feed",
    "crates/movement-relay",
]
```

  Add `"crates/health-http",` (alphabetically, after `"crates/full-coverage-consumer",`
  and before `"crates/movement-feed",` — matching the existing list's rough
  alphabetical grouping).

- [ ] **Step 2: Create `crates/health-http/Cargo.toml`**

```toml
[package]
name = "health-http"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = { version = "0.8.9", features = ["http2"] }
common = { path = "../common" }
metrics = "0.24"
tokio = { version = "1.53.1", features = ["rt-multi-thread", "macros", "net"] }
tracing = "0.1.44"
```

  (`axum` version matches every current axum user in this workspace;
  `tokio`'s `net` feature is needed for `TcpListener`, `rt-multi-thread`/
  `macros` for the `tokio::spawn`'d server task; `common` is a path
  dependency for `metric_name`.)

- [ ] **Step 3: Create `crates/health-http/src/lib.rs`**

```rust
//! Shared health/readiness HTTP endpoint: `/healthz` backed by an
//! `AtomicBool`, plus a matching Prometheus readiness gauge update.
//! Previously duplicated near-verbatim across `trust-consumer`,
//! `full-coverage-consumer` (character-for-character identical apart from
//! one gauge-name string), and (a close structural cousin, deliberately
//! different readiness semantics) `movement-relay`. See
//! docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md
//! §3.3 for the full per-caller verification.
//!
//! `healthy_text`/`unhealthy_text` and `gauge_name` are parameters, not
//! hardcoded, so every real caller's own wire-visible `/healthz` response
//! body and Prometheus gauge name stay byte-for-byte unchanged after
//! adopting this shared module -- this is a pure refactor, not a
//! behavior-unifying one.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::StatusCode;
use axum::routing::get;

/// `true` once the caller's own connection/consumer is confirmed live;
/// `false` from startup and whenever disconnected. Shared, not
/// crate-local -- every real caller today used exactly this type alias
/// (`Arc<AtomicBool>`) under its own crate-local name.
pub type ConnectionState = Arc<AtomicBool>;

/// Creates a fresh `ConnectionState` and starts the `/healthz` server.
/// Matches `trust-consumer`/`full-coverage-consumer`'s own current call
/// shape (`health::spawn(bind_url)`).
pub fn spawn(
    bind_url: String,
    healthy_text: &'static str,
    unhealthy_text: &'static str,
) -> ConnectionState {
    let state: ConnectionState = Arc::new(AtomicBool::new(false));
    spawn_with_state(bind_url, Arc::clone(&state), healthy_text, unhealthy_text);
    state
}

/// Starts the `/healthz` server against an already-constructed state.
/// Matches `movement-relay`'s own current call shape
/// (`health::spawn(bind_url, ready)`, where `ready` is created earlier and
/// owned by `RelayContext`).
pub fn spawn_with_state(
    bind_url: String,
    state: ConnectionState,
    healthy_text: &'static str,
    unhealthy_text: &'static str,
) {
    tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/healthz",
            get(move || healthz(Arc::clone(&state), healthy_text, unhealthy_text)),
        );
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

async fn healthz(
    state: ConnectionState,
    healthy_text: &'static str,
    unhealthy_text: &'static str,
) -> (StatusCode, &'static str) {
    if state.load(Ordering::Relaxed) {
        (StatusCode::OK, healthy_text)
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, unhealthy_text)
    }
}

/// Centralizes every `ConnectionState` transition with a matching
/// Prometheus gauge update, so the `AtomicBool` and the readiness gauge
/// never drift out of sync. `gauge_name` is a parameter (e.g.
/// `"trust_consumer_ready"`, `"full_coverage_consumer_ready"`,
/// `"movement_relay_ready"`) instead of three copy-pasted hardcoded
/// strings -- every real caller passes the exact string it emits today.
pub fn set_connected(state: &ConnectionState, gauge_name: &str, connected: bool) {
    state.store(connected, Ordering::Relaxed);
    metrics::gauge!(common::metrics::metric_name(gauge_name))
        .set(if connected { 1.0 } else { 0.0 });
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use super::*;

    #[test]
    fn set_connected_updates_the_shared_atomic_state() {
        let state: ConnectionState = Arc::new(AtomicBool::new(false));

        set_connected(&state, "test_ready", true);
        assert!(state.load(Ordering::Relaxed));

        set_connected(&state, "test_ready", false);
        assert!(!state.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn healthz_reports_the_caller_supplied_text_for_each_state() {
        let state: ConnectionState = Arc::new(AtomicBool::new(false));
        let (status, body) = healthz(Arc::clone(&state), "connected", "disconnected").await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, "disconnected");

        state.store(true, Ordering::Relaxed);
        let (status, body) = healthz(Arc::clone(&state), "connected", "disconnected").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "connected");
    }
}
```

- [ ] **Step 4: Verify**

```bash
cargo build -p health-http
cargo test -p health-http
cargo clippy -p health-http --all-features -- -D warnings
rustfmt --edition 2024 crates/health-http/src/lib.rs
```

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/health-http/Cargo.toml crates/health-http/src/lib.rs
git commit -m "Add health-http crate: shared readiness /healthz + gauge plumbing (not yet adopted)"
```

---

# Group B — Adopt Group A's config types

Each task below flattens one crate's own `internal_oauth_*`/
`metrics_enabled`/Kafka/`LineCatalogue` block into Group A's shared structs.
**Every task ends with a `--help` or `try_parse_from` check confirming the
exact same flags/env vars still work** (Global Constraints). `#4`
(`LineCatalogue`) needs no shared crate at all and could run before/in
parallel with Group A per the spec's own §5 note; it's sequenced here
anyway since it's mechanically similar work, not because of a real ordering
constraint.

## Task B1: `poller-incidents` — flatten `InternalOAuthArgs` + `MetricsArgs`

**Files:** modify `crates/poller-incidents/src/config.rs`,
`crates/poller-incidents/src/main.rs`.

- [ ] **Step 1: `config.rs`.** Current (`crates/poller-incidents/src/config.rs:27-67`):

```rust
    /// Shared, non-secret OAuth2 client-credentials config (same value
    /// across all 8 real callers) -- see
    /// docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md
    /// Decision 6.
    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    /// This service's own Authentik service-account credential --
    /// per-service, distinct from every other caller's. `username` is
    /// identifying, not itself the secret; `password` (an Authentik
    /// app-password) is the actual secret.
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,

    /// RSPS5050 P-03-00 Rev A §10: "Recommend every 5 minutes."
    #[arg(long, env, default_value_t = 300)]
    pub poll_interval_secs: u64,

    /// Port for this poller's Prometheus `/metrics` endpoint. See
    /// docs/superpowers/plans/2026-08-29-metrics.md's Global Constraints
    /// for why this differs from api.service.port -- api reuses its
    /// existing HTTP listener, this poller has none, so it needs a new one.
    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,

    /// Whether to start this service's Prometheus `/metrics` listener at
    /// all. Distinct from `metrics_port` (which port to use IF started) --
    /// this is what actually satisfies "metrics.enabled=false leaves the
    /// service working exactly as it does today" (see the Helm chart's
    /// `metrics.enabled` value and this branch's final whole-branch
    /// review, Important finding #2): omitting the containerPort/env/
    /// annotations in the chart alone does not stop the process from
    /// listening, since Kubernetes container ports are purely
    /// declarative.
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
```

  Replace the 5 `internal_oauth_*` fields and `metrics_enabled` with
  flattened structs (`metrics_port` stays a plain field, per Global
  Constraints):

```rust
    /// Shared, non-secret OAuth2 client-credentials config (same value
    /// across all 9 real callers).
    #[command(flatten)]
    pub internal_oauth: common::oauth_client::InternalOAuthArgs,

    /// RSPS5050 P-03-00 Rev A §10: "Recommend every 5 minutes."
    #[arg(long, env, default_value_t = 300)]
    pub poll_interval_secs: u64,

    /// Port for this poller's Prometheus `/metrics` endpoint. Stays a
    /// plain field, not part of `MetricsArgs` -- its default differs per
    /// crate and `docker-compose.yml` relies on the code default.
    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,

    #[command(flatten)]
    pub metrics: common::service_args::MetricsArgs,
}
```

- [ ] **Step 2: `main.rs`.** Current
  (`crates/poller-incidents/src/main.rs:36-48`):

```rust
    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });
```

  Change to:

```rust
    let config = Config::parse();
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = config.internal_oauth.token_cache();
```

  This task does **not** yet touch the loop body below this (lines 50-86) —
  that's Group C's job, sequenced after this group specifically so Group
  C's own diff is "swap the loop body for a helper call," not both changes
  at once (spec §5).

- [ ] **Step 3: Verify — flags/env unchanged**

```bash
cargo build -p poller-incidents
cargo run -p poller-incidents -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port"
```

  Confirm the output still lists `--internal-oauth-token-url`,
  `--internal-oauth-client-id`, `--internal-oauth-scope`,
  `--internal-oauth-username`, `--internal-oauth-password`,
  `--metrics-enabled`, `--metrics-port` — same flags as before this task,
  byte-for-byte.

```bash
cargo test -p poller-incidents
cargo clippy -p poller-incidents --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-incidents/src/config.rs crates/poller-incidents/src/main.rs
```

- [ ] **Step 4: Commit**

```bash
git add crates/poller-incidents/src/config.rs crates/poller-incidents/src/main.rs
git commit -m "poller-incidents: flatten InternalOAuthArgs + MetricsArgs from common"
```

---

## Task B2: `poller-stations` — flatten `InternalOAuthArgs` + `MetricsArgs`

**Files:** modify `crates/poller-stations/src/config.rs`,
`crates/poller-stations/src/main.rs`.

Identical shape to Task B1. This crate's own field line numbers:
`internal_oauth_token_url` at `config.rs:31`, through
`internal_oauth_password` at `:43`; `metrics_port` at `:55`;
`metrics_enabled` at `:67`. `main.rs`'s OAuth-cache construction block is at
`:41-48` (same line numbers as `poller-incidents`, confirmed identical
shape); `metrics_enabled` check at `:37`.

- [ ] **Step 1: `config.rs`** — same substitution as Task B1 Step 1: delete
  the 5 `internal_oauth_*` fields (`:31-43`) and replace with `#[command(flatten)]
  pub internal_oauth: common::oauth_client::InternalOAuthArgs,`; keep
  `metrics_port` (`:55`) as a plain field; delete `metrics_enabled` (`:67`)
  and replace with `#[command(flatten)] pub metrics:
  common::service_args::MetricsArgs,`.

- [ ] **Step 2: `main.rs`** — same substitution as Task B1 Step 2: `if
  config.metrics.metrics_enabled { ... }`; `let internal_oauth =
  config.internal_oauth.token_cache();`, deleting the manual
  `OAuthTokenCache::new(OAuthCredentials { ... })` block.

- [ ] **Step 3: Verify**

```bash
cargo build -p poller-stations
cargo run -p poller-stations -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port"
cargo test -p poller-stations
cargo clippy -p poller-stations --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-stations/src/config.rs crates/poller-stations/src/main.rs
```

- [ ] **Step 4: Commit**

```bash
git add crates/poller-stations/src/config.rs crates/poller-stations/src/main.rs
git commit -m "poller-stations: flatten InternalOAuthArgs + MetricsArgs from common"
```

---

## Task B3: `poller-tocs` — flatten `InternalOAuthArgs` + `MetricsArgs`

**Files:** modify `crates/poller-tocs/src/config.rs`,
`crates/poller-tocs/src/main.rs`.

Identical shape. Field line numbers: `internal_oauth_token_url` at
`config.rs:32` through `internal_oauth_password` at `:44`; `metrics_port`
at `:55`; `metrics_enabled` at `:67`. `main.rs` OAuth-cache block at
`:41-48`; `metrics_enabled` check at `:37`.

- [ ] **Step 1: `config.rs`** — same substitution as Task B1 Step 1, this
  crate's own line numbers above.

- [ ] **Step 2: `main.rs`** — same substitution as Task B1 Step 2.

- [ ] **Step 3: Verify**

```bash
cargo build -p poller-tocs
cargo run -p poller-tocs -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port"
cargo test -p poller-tocs
cargo clippy -p poller-tocs --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-tocs/src/config.rs crates/poller-tocs/src/main.rs
```

- [ ] **Step 4: Commit**

```bash
git add crates/poller-tocs/src/config.rs crates/poller-tocs/src/main.rs
git commit -m "poller-tocs: flatten InternalOAuthArgs + MetricsArgs from common"
```

---

## Task B4: `poller-ldbws` — flatten `InternalOAuthArgs` + `MetricsArgs`

**Files:** modify `crates/poller-ldbws/src/config.rs`,
`crates/poller-ldbws/src/main.rs`.

Field line numbers: `internal_oauth_token_url` at `config.rs:62` through
`internal_oauth_password` at `:74`; `metrics_port` at `:87`;
`metrics_enabled` at `:99`. `main.rs` OAuth-cache block at `:70-76`;
`metrics_enabled` check at `:66`. **This crate also has a test fixture
Config literal** at `main.rs:388-397` that must be updated in the same
task:

- [ ] **Step 1: `config.rs`** — same substitution as Task B1 Step 1, this
  crate's own line numbers.

- [ ] **Step 2: `main.rs` (production code)** — same substitution as Task
  B1 Step 2.

- [ ] **Step 3: `main.rs` test fixture.** Current
  (`crates/poller-ldbws/src/main.rs:388-397`, inside a `Config { ... }`
  struct literal):

```rust
            internal_oauth_token_url: "http://auth.invalid/token".to_string(),
            internal_oauth_client_id: "distant-signal-internal".to_string(),
            internal_oauth_scope: "groups".to_string(),
            internal_oauth_username: "svc-poller-ldbws".to_string(),
            internal_oauth_password: "app-password".to_string(),
            ...
            metrics_enabled: false,
```

  Change to:

```rust
            internal_oauth: common::oauth_client::InternalOAuthArgs {
                internal_oauth_token_url: "http://auth.invalid/token".to_string(),
                internal_oauth_client_id: "distant-signal-internal".to_string(),
                internal_oauth_scope: "groups".to_string(),
                internal_oauth_username: "svc-poller-ldbws".to_string(),
                internal_oauth_password: "app-password".to_string(),
            },
            ...
            metrics: common::service_args::MetricsArgs { metrics_enabled: false },
```

  (keep every other field in this literal exactly as-is; only the OAuth
  block and `metrics_enabled` line move under the new nested structs.)

- [ ] **Step 4: Verify**

```bash
cargo build -p poller-ldbws
cargo run -p poller-ldbws -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port"
cargo test -p poller-ldbws
cargo clippy -p poller-ldbws --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-ldbws/src/config.rs crates/poller-ldbws/src/main.rs
```

- [ ] **Step 5: Commit**

```bash
git add crates/poller-ldbws/src/config.rs crates/poller-ldbws/src/main.rs
git commit -m "poller-ldbws: flatten InternalOAuthArgs + MetricsArgs from common"
```

---

## Task B5: `poller-tfl` — flatten `InternalOAuthArgs` + `MetricsArgs`

**Files:** modify `crates/poller-tfl/src/config.rs`,
`crates/poller-tfl/src/main.rs`.

Field line numbers: `internal_oauth_token_url` at `config.rs:42` through
`internal_oauth_password` at `:54`; `metrics_port` at `:87`;
`metrics_enabled` at `:99`. `main.rs`'s pre-flight check
(`require_non_empty_key`, `:91`) and `dlr_state` (`:121`) are **not**
touched by this task — confirmed in the spec's §3.1 to live outside the
scaffolding block this group/Group C touch. OAuth-cache block at `:96-103`;
`metrics_enabled` check at `:92`.

- [ ] **Step 1: `config.rs`** — same substitution as Task B1 Step 1, this
  crate's own line numbers.

- [ ] **Step 2: `main.rs`** — same substitution as Task B1 Step 2, applied
  at this crate's own lines (`:92`, `:96-103`); leave
  `require_non_empty_key(&config.tfl_app_key)?;` (`:91`) and `let mut
  dlr_state = dlr::inference::DlrMatchState::new();` (`:121`) untouched.

- [ ] **Step 3: Verify**

```bash
cargo build -p poller-tfl
cargo run -p poller-tfl -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port"
cargo test -p poller-tfl
cargo clippy -p poller-tfl --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-tfl/src/config.rs crates/poller-tfl/src/main.rs
```

- [ ] **Step 4: Commit**

```bash
git add crates/poller-tfl/src/config.rs crates/poller-tfl/src/main.rs
git commit -m "poller-tfl: flatten InternalOAuthArgs + MetricsArgs from common"
```

---

## Task B6: `schedule-ingest` — flatten `InternalOAuthArgs` + `MetricsArgs`

**Files:** modify `crates/schedule-ingest/src/config.rs`,
`crates/schedule-ingest/src/main.rs`.

Field line numbers: `internal_oauth_token_url` at `config.rs:104` through
`internal_oauth_password` at `:116`; `metrics_port` at `:123`;
`metrics_enabled` at `:134`. This crate's OAuth-cache construction and
metrics-install call live in `main.rs` (grep for
`OAuthTokenCache::new`/`metrics::install` in this crate's own `main.rs` to
find the current call site before editing — this plan's own earlier
research did not capture its exact line numbers for the production call
site, only the test fixture below; re-derive them fresh as this task's
first step). **Test fixture** at `main.rs:742-758` (`fn test_config`):

- [ ] **Step 1: Re-derive `main.rs`'s production call site.**

```bash
grep -n "OAuthTokenCache::new\|metrics_enabled\|metrics::install" crates/schedule-ingest/src/main.rs | head -10
```

  Apply the same substitution as Task B1 Step 2 at whatever lines this
  reports (this crate's `main()` was not read in full during this plan's
  own research pass — confirm the exact block before editing, since
  `schedule-ingest`'s `main()` has more going on than a poller's own
  simpler `main()` and may not have this block in the same relative
  position).

- [ ] **Step 2: `config.rs`** — same substitution as Task B1 Step 1, this
  crate's own line numbers (`:104-116`, `:123`, `:134`).

- [ ] **Step 3: `main.rs` test fixture.** Current
  (`crates/schedule-ingest/src/main.rs:742-758`):

```rust
    fn test_config(watch_dir: &std::path::Path, storage_dir: &std::path::Path) -> Config {
        Config {
            watch_dir: watch_dir.to_path_buf(),
            storage_dir: storage_dir.to_path_buf(),
            check_times: "22:00,16:00".to_string(),
            poll_interval_secs: 120,
            retention_keep_deliveries: 2,
            stability_cycles: 2,
            api_ingest_url: "http://127.0.0.1:1/schedule-feed-ingests".to_string(),
            internal_oauth_token_url: "http://127.0.0.1:1/token".to_string(),
            internal_oauth_client_id: "test-client".to_string(),
            internal_oauth_scope: "groups".to_string(),
            internal_oauth_username: "test-user".to_string(),
            internal_oauth_password: "test-password".to_string(),
            metrics_port: 0,
            metrics_enabled: false,
        }
    }
```

  Change to:

```rust
    fn test_config(watch_dir: &std::path::Path, storage_dir: &std::path::Path) -> Config {
        Config {
            watch_dir: watch_dir.to_path_buf(),
            storage_dir: storage_dir.to_path_buf(),
            check_times: "22:00,16:00".to_string(),
            poll_interval_secs: 120,
            retention_keep_deliveries: 2,
            stability_cycles: 2,
            api_ingest_url: "http://127.0.0.1:1/schedule-feed-ingests".to_string(),
            internal_oauth: common::oauth_client::InternalOAuthArgs {
                internal_oauth_token_url: "http://127.0.0.1:1/token".to_string(),
                internal_oauth_client_id: "test-client".to_string(),
                internal_oauth_scope: "groups".to_string(),
                internal_oauth_username: "test-user".to_string(),
                internal_oauth_password: "test-password".to_string(),
            },
            metrics_port: 0,
            metrics: common::service_args::MetricsArgs { metrics_enabled: false },
        }
    }
```

- [ ] **Step 4: Verify**

```bash
cargo build -p schedule-ingest
cargo run -p schedule-ingest -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port"
cargo test -p schedule-ingest
cargo clippy -p schedule-ingest --all-features -- -D warnings
rustfmt --edition 2024 crates/schedule-ingest/src/config.rs crates/schedule-ingest/src/main.rs
```

- [ ] **Step 5: Commit**

```bash
git add crates/schedule-ingest/src/config.rs crates/schedule-ingest/src/main.rs
git commit -m "schedule-ingest: flatten InternalOAuthArgs + MetricsArgs from common"
```

---

## Task B7: `schedule-reference` — flatten `InternalOAuthArgs` + `MetricsArgs`, swap in shared `LineCatalogue`

**Files:** modify `crates/schedule-reference/src/config.rs`,
`crates/schedule-reference/src/main.rs`.

Both changes touch the same file — done in one task. Field line numbers:
`internal_oauth_token_url` at `config.rs:89` through
`internal_oauth_password` at `:101`; `metrics_port` at `:107`;
`metrics_enabled` at `:111`. `LineCatalogue`/`parse_lines`/`Deref` block at
`config.rs:7-25`; `--lines-dir` field at `:81-82`.

- [ ] **Step 1: `config.rs` — OAuth/Metrics flatten**, same substitution as
  Task B1 Step 1.

- [ ] **Step 2: `config.rs` — `LineCatalogue` swap.** Delete lines `7-25`
  (the local `parse_lines` fn + `LineCatalogue` struct + `Deref` impl).
  Replace the `use common::LineDefinition;` import (line 5) with `use
  common::config::{parse_lines, LineCatalogue};` (drop the now-unused
  `common::LineDefinition` import if nothing else in this file references
  it directly — confirm via `cargo build`'s own unused-import warning after
  this edit). The field itself (`config.rs:81-82`) is unchanged:

```rust
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines)]
    pub lines: LineCatalogue,
```

  continues to resolve correctly against the imported (not locally
  defined) `parse_lines`/`LineCatalogue`.

- [ ] **Step 3: `main.rs` — OAuth/Metrics production call site.** Find and
  update via:

```bash
grep -n "OAuthTokenCache::new\|metrics_enabled\|metrics::install" crates/schedule-reference/src/main.rs | head -10
```

  Apply the same substitution as Task B1 Step 2.

- [ ] **Step 4: Verify**

```bash
cargo build -p schedule-reference
cargo run -p schedule-reference -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port|lines-dir"
cargo test -p schedule-reference
cargo clippy -p schedule-reference --all-features -- -D warnings
rustfmt --edition 2024 crates/schedule-reference/src/config.rs crates/schedule-reference/src/main.rs
```

- [ ] **Step 5: Commit**

```bash
git add crates/schedule-reference/src/config.rs crates/schedule-reference/src/main.rs
git commit -m "schedule-reference: flatten InternalOAuthArgs + MetricsArgs, adopt shared LineCatalogue"
```

---

## Task B8: `trust-consumer` — flatten `InternalOAuthArgs` + `MetricsArgs` + `KafkaConnectionArgs`

**Files:** modify `crates/trust-consumer/src/config.rs`,
`crates/trust-consumer/src/main.rs`.

Field line numbers (`config.rs`): Kafka block `kafka_brokers` `:50` through
`kafka_sasl_mechanism` `:77` (note: `kafka_consumer_group` at `:61` stays a
plain field, per Global Constraints); OAuth block `:92-104`; metrics
`:197-205` (`metrics_port` `:203`, `metrics_enabled` `:205`). `main.rs`'s
OAuth-cache block at `:99-106`ish (confirmed via earlier read: the block
mirrors the pollers' own shape, `common::oauth_client::OAuthTokenCache::new(...)`
right after `metrics::install`); `metrics_enabled` check at `:99-101`
(re-confirm exact lines before editing — `main.rs`'s setup section was
read in full during this plan's research and is reproduced below for this
task's own use).

- [ ] **Step 1: `config.rs` — Kafka flatten.** Current
  (`crates/trust-consumer/src/config.rs:46-77`, the `Config` struct's
  Kafka fields):

```rust
pub struct Config {
    #[arg(long, env)]
    pub kafka_brokers: String,
    ...
    #[arg(long, env)]
    pub kafka_topic: String,
    ...
    #[arg(long, env, default_value = "distant-signal-trust-consumer")]
    pub kafka_consumer_group: String,
    ...
    #[arg(long, env)]
    pub kafka_sasl_username: String,
    ...
    #[arg(long, env)]
    pub kafka_sasl_password: String,
    ...
    #[arg(long, env)]
    pub kafka_sasl_mechanism: String,
```

  Replace the 5 flattenable fields (keep `kafka_consumer_group` as its own
  plain field, in its existing position, doc comment intact):

```rust
pub struct Config {
    #[command(flatten)]
    pub kafka: common::service_args::KafkaConnectionArgs,

    #[arg(long, env, default_value = "distant-signal-trust-consumer")]
    pub kafka_consumer_group: String,
```

  Every call site that read `config.kafka_brokers` etc. directly now reads
  `config.kafka.kafka_brokers` etc. — grep for every such call site in this
  crate before moving on:

```bash
grep -rn "config\.kafka_brokers\|config\.kafka_topic\|config\.kafka_sasl_username\|config\.kafka_sasl_password\|config\.kafka_sasl_mechanism" crates/trust-consumer/src/
```

  Update each hit to the `config.kafka.<field>` path (`kafka_consumer_group`
  stays `config.kafka_consumer_group`, unchanged).

- [ ] **Step 2: `config.rs` — OAuth/Metrics flatten**, same substitution as
  Task B1 Step 1, this crate's own lines (`:92-104`, `:203`, `:205`).

- [ ] **Step 3: `main.rs` — OAuth/Metrics production call site.** Current
  (`crates/trust-consumer/src/main.rs:99-106`, reproduced from this plan's
  own full read of this file):

```rust
    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let connection_state = health::spawn(config.health_bind_url.clone());
    let http = reqwest::Client::new();
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });
```

  Change to (leave `health::spawn` untouched here — that's Group D's job):

```rust
    let config = Config::parse();
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let connection_state = health::spawn(config.health_bind_url.clone());
    let http = reqwest::Client::new();
    let internal_oauth = config.internal_oauth.token_cache();
```

- [ ] **Step 4: Verify — flags/env AND the existing `try_parse_from`
  regression test.**

```bash
cargo build -p trust-consumer
cargo run -p trust-consumer -- --help 2>&1 | grep -E "kafka-brokers|kafka-topic|kafka-consumer-group|kafka-sasl|internal-oauth|metrics-enabled|metrics-port"
cargo test -p trust-consumer movement_feed_backend_defaults_to_kafka_when_unset
```

  This crate's own existing test
  (`crates/trust-consumer/src/config.rs:209-253`,
  `movement_feed_backend_defaults_to_kafka_when_unset`) calls
  `Config::try_parse_from` with the real `--kafka-brokers`/
  `--internal-oauth-token-url`/etc. flag strings — it must still pass
  unmodified after this flatten, which is itself strong, executable proof
  the flag names didn't change.

```bash
cargo test -p trust-consumer
cargo clippy -p trust-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/trust-consumer/src/config.rs crates/trust-consumer/src/main.rs
```

  (If Step 1's grep found hits outside `config.rs`/`main.rs`, add those
  files to this `rustfmt` invocation and this task's `git add`/commit.)

- [ ] **Step 5: Commit**

```bash
git add crates/trust-consumer/src/config.rs crates/trust-consumer/src/main.rs
git commit -m "trust-consumer: flatten InternalOAuthArgs + MetricsArgs + KafkaConnectionArgs from common"
```

---

## Task B9: `full-coverage-consumer` — flatten `InternalOAuthArgs` + `MetricsArgs` + `KafkaConnectionArgs`, swap in shared `LineCatalogue`

**Files:** modify `crates/full-coverage-consumer/src/config.rs`,
`crates/full-coverage-consumer/src/main.rs`.

All four changes touch `config.rs`/`main.rs` — one task. Field line
numbers (`config.rs`): `LineCatalogue`/`parse_lines`/`Deref` at `:1-19`;
Kafka block `:44-61` (`kafka_consumer_group` at `:55` stays plain); OAuth
block `:91-99`; metrics `:126,128`; `--lines-dir` field at `:120-121`; test
fixture `base_config` at `:213-243`.

- [ ] **Step 1: `config.rs` — `LineCatalogue` swap.** Delete the local
  `parse_lines` fn + `LineCatalogue` struct + `Deref` impl (`:7-19`, per
  this plan's own fresh read). Replace `use common::LineDefinition;` (line
  5) with `use common::config::{parse_lines, LineCatalogue};` (drop the
  `common::LineDefinition` import only if nothing else in this file uses it
  directly — this crate's test fixture builds `LineDefinition` literals
  too, at `:180-199`ish; check before dropping the import). Field at
  `:120-121` is unchanged.

- [ ] **Step 2: `config.rs` — Kafka flatten.** Same substitution as Task
  B8 Step 1, this crate's own lines (`:44-61`, keeping
  `kafka_consumer_group` at `:55` as its own plain field). Grep and update
  every `config.kafka_brokers`/etc. call site in this crate the same way.

- [ ] **Step 3: `config.rs` — OAuth/Metrics flatten**, same substitution as
  Task B1 Step 1, this crate's own lines (`:91-99`, `:126`, `:128`).

- [ ] **Step 4: `main.rs` — OAuth/Metrics production call site.** Current
  (`crates/full-coverage-consumer/src/main.rs:106-119`, reproduced from
  this plan's own full read):

```rust
    let config = Config::parse();
    if config.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let connection_state = health::spawn(config.health_bind_url.clone());
    let http = reqwest::Client::new();
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });
```

  Change identically to Task B8 Step 3's pattern (leave `health::spawn`
  untouched — Group D's job).

- [ ] **Step 5: `config.rs` test fixture.** Current
  (`crates/full-coverage-consumer/src/config.rs:213-243`, `fn
  base_config`):

```rust
    fn base_config(lines: Vec<LineDefinition>, shadow_lines: &str) -> Config {
        Config {
            kafka_brokers: String::new(),
            kafka_topic: String::new(),
            kafka_consumer_group: String::new(),
            kafka_sasl_username: String::new(),
            kafka_sasl_password: String::new(),
            kafka_sasl_mechanism: String::new(),
            schedule_line_population_url: String::new(),
            full_coverage_stats_url: String::new(),
            station_full_coverage_stats_url: String::new(),
            stanox_crs_url: String::new(),
            internal_oauth_token_url: String::new(),
            internal_oauth_client_id: String::new(),
            internal_oauth_scope: String::new(),
            internal_oauth_username: String::new(),
            internal_oauth_password: String::new(),
            population_reload_secs: 300,
            stanox_crs_reload_secs: 3600,
            stats_write_interval_secs: 60,
            shadow_lines: shadow_lines.to_string(),
            lines: LineCatalogue(lines),
            health_bind_url: String::new(),
            metrics_port: 9093,
            metrics_enabled: false,
            movement_feed_backend: MovementFeedBackend::Kafka,
            redis_url: String::new(),
            redis_autoclaim_min_idle_secs: 30,
            redis_gap_check_secs: 60,
        }
    }
```

  Change to:

```rust
    fn base_config(lines: Vec<LineDefinition>, shadow_lines: &str) -> Config {
        Config {
            kafka: common::service_args::KafkaConnectionArgs {
                kafka_brokers: String::new(),
                kafka_topic: String::new(),
                kafka_sasl_username: String::new(),
                kafka_sasl_password: String::new(),
                kafka_sasl_mechanism: String::new(),
            },
            kafka_consumer_group: String::new(),
            schedule_line_population_url: String::new(),
            full_coverage_stats_url: String::new(),
            station_full_coverage_stats_url: String::new(),
            stanox_crs_url: String::new(),
            internal_oauth: common::oauth_client::InternalOAuthArgs {
                internal_oauth_token_url: String::new(),
                internal_oauth_client_id: String::new(),
                internal_oauth_scope: String::new(),
                internal_oauth_username: String::new(),
                internal_oauth_password: String::new(),
            },
            population_reload_secs: 300,
            stanox_crs_reload_secs: 3600,
            stats_write_interval_secs: 60,
            shadow_lines: shadow_lines.to_string(),
            lines: LineCatalogue(lines),
            health_bind_url: String::new(),
            metrics_port: 9093,
            metrics: common::service_args::MetricsArgs { metrics_enabled: false },
            movement_feed_backend: MovementFeedBackend::Kafka,
            redis_url: String::new(),
            redis_autoclaim_min_idle_secs: 30,
            redis_gap_check_secs: 60,
        }
    }
```

- [ ] **Step 6: Verify**

```bash
cargo build -p full-coverage-consumer
cargo run -p full-coverage-consumer -- --help 2>&1 | grep -E "kafka-brokers|kafka-consumer-group|kafka-sasl|internal-oauth|metrics-enabled|metrics-port|lines-dir"
cargo test -p full-coverage-consumer
cargo clippy -p full-coverage-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/full-coverage-consumer/src/config.rs crates/full-coverage-consumer/src/main.rs
```

- [ ] **Step 7: Commit**

```bash
git add crates/full-coverage-consumer/src/config.rs crates/full-coverage-consumer/src/main.rs
git commit -m "full-coverage-consumer: flatten config structs from common, adopt shared LineCatalogue"
```

---

## Task B10: `movement-relay` — flatten `KafkaConnectionArgs`

**Files:** modify `crates/movement-relay/src/config.rs`.

`movement-relay` has no `internal_oauth_*`/`metrics_enabled` fields at all
(confirmed: it's not one of the 9 OAuth/Metrics consumers) — this task is
Kafka-only. Current (`crates/movement-relay/src/config.rs:8-30`):

```rust
pub struct Config {
    #[arg(long, env)]
    pub kafka_brokers: String,
    #[arg(long, env)]
    pub kafka_topic: String,
    /// The one real, RDM-issued group ...
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
    ...
```

- [ ] **Step 1: Flatten the 5 fields**, keeping `kafka_consumer_group` (no
  default — Global Constraints) as its own plain field, doc comment intact:

```rust
pub struct Config {
    #[command(flatten)]
    pub kafka: common::service_args::KafkaConnectionArgs,

    /// The one real, RDM-issued group -- `SC-c4d90f8e-...` in production,
    /// per the design doc's "Why this exists" section. Deliberately no
    /// default: unlike trust-consumer's own kafka_consumer_group (which
    /// DOES have a sensible per-deployment default,
    /// "distant-signal-trust-consumer"), this crate's group id is a fixed,
    /// externally-issued, unforgeable identity -- guessing wrong here is
    /// worse than refusing to start.
    #[arg(long, env)]
    pub kafka_consumer_group: String,

    #[arg(long, env, default_value = "redis://redis:6379")]
    pub redis_url: String,
    ...
```

- [ ] **Step 2: Update call sites.**

```bash
grep -rn "config\.kafka_brokers\|config\.kafka_topic\|config\.kafka_sasl_username\|config\.kafka_sasl_password\|config\.kafka_sasl_mechanism" crates/movement-relay/src/
```

  Update every hit to `config.kafka.<field>`; `config.kafka_consumer_group`
  is unchanged.

- [ ] **Step 3: Verify**

```bash
cargo build -p movement-relay
cargo run -p movement-relay -- --help 2>&1 | grep -E "kafka-brokers|kafka-topic|kafka-consumer-group|kafka-sasl"
cargo test -p movement-relay
cargo clippy -p movement-relay --all-features -- -D warnings
rustfmt --edition 2024 crates/movement-relay/src/config.rs
```

  (Add any other file Step 2's grep touched to this `rustfmt`
  invocation/commit.)

- [ ] **Step 4: Commit**

```bash
git add crates/movement-relay/src/config.rs
git commit -m "movement-relay: flatten KafkaConnectionArgs from common"
```

---

## Task B11: `aggregator` — adopt shared `LineCatalogue`

**Files:** modify `crates/aggregator/src/config.rs`.

Current (`crates/aggregator/src/config.rs:1-32`):

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use common::LineDefinition;

fn parse_lines(path: &str) -> Result<LineCatalogue> {
    LineDefinition::from_dir(&PathBuf::from(path)).map(LineCatalogue)
}

/// Newtype around the parsed line catalogue. ...
#[derive(Debug, Clone, Default)]
pub struct LineCatalogue(pub Vec<LineDefinition>);

impl std::ops::Deref for LineCatalogue {
    type Target = Vec<LineDefinition>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
```

- [ ] **Step 1: Replace the whole block (lines 1-32) with:**

```rust
use clap::Parser;
use common::config::{parse_lines, LineCatalogue};
```

  (`std::path::PathBuf`, `anyhow::Result`, and `common::LineDefinition` are
  no longer needed directly in this file — confirm via `cargo build`'s
  unused-import warnings after this edit whether anything else in this
  file still needs them; keep only what's still used.)

- [ ] **Step 2: Verify — field usage at `config.rs:44-45` is unchanged**

```rust
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines)]
    pub lines: LineCatalogue,
```

  No edit needed here — it resolves against the newly-imported names.

```bash
cargo build -p aggregator
cargo run -p aggregator -- --help 2>&1 | grep -E "lines-dir"
cargo test -p aggregator
cargo clippy -p aggregator --all-features -- -D warnings
rustfmt --edition 2024 crates/aggregator/src/config.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/aggregator/src/config.rs
git commit -m "aggregator: adopt shared common::config::LineCatalogue"
```

---

## Task B12: `api` — adopt shared `LineCatalogue`

**Files:** modify `crates/api/src/data/config.rs`.

Current (`crates/api/src/data/config.rs:1-43`):

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::ValueHint;
use serde::de::DeserializeOwned;

use crate::data::LineDefinition;

pub use common::Defaults;

fn parse_toml_path<T: DeserializeOwned>(path: &'_ str) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}

fn parse_lines(path: &str) -> Result<LineCatalogue> {
    LineDefinition::from_dir(&PathBuf::from(path)).map(LineCatalogue)
}

/// Newtype around the parsed line catalogue. ...
#[derive(Debug, Clone, Default)]
pub struct LineCatalogue(pub Vec<LineDefinition>);

impl std::ops::Deref for LineCatalogue {
    type Target = Vec<LineDefinition>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
```

`parse_toml_path` (unrelated to `LineCatalogue`, used for `Defaults`
loading elsewhere in this file) must stay.

- [ ] **Step 1: Remove only the `LineCatalogue`-specific block** (the
  `parse_lines` fn at lines 16-18 and the `LineCatalogue` struct + `Deref`
  impl at lines 20-43), keeping `parse_toml_path` and everything else:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::ValueHint;
use serde::de::DeserializeOwned;

pub use common::Defaults;
use common::config::{parse_lines, LineCatalogue};

fn parse_toml_path<T: DeserializeOwned>(path: &'_ str) -> Result<T> {
    let text = std::fs::read_to_string(path)?;
    Ok(toml::from_str(&text)?)
}
```

  Drop `use crate::data::LineDefinition;` (line 7) — per the spec's §3.4
  note, this was only ever a locally re-exported name for `common::LineDefinition`
  (`crates/api/src/data/mod.rs:15`), needed only by the now-deleted local
  `LineCatalogue`; confirm via `cargo build`'s unused-import warning
  whether anything else in this file still needs it before removing it.

- [ ] **Step 2: Field usage at `config.rs:201-202` is unchanged:**

```rust
    #[arg(long = "lines-dir", env = "LINES_DIR", default_value = "/app/lines", value_parser = parse_lines, value_hint = ValueHint::FilePath, value_name = "DIR")]
    pub lines: LineCatalogue,
```

- [ ] **Step 3: Verify**

```bash
cargo build -p api
cargo run -p api -- --help 2>&1 | grep -E "lines-dir"
cargo test -p api
cargo clippy -p api --all-features -- -D warnings
rustfmt --edition 2024 crates/api/src/data/config.rs
```

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/config.rs
git commit -m "api: adopt shared common::config::LineCatalogue"
```

---

# Group C — `#1`, the poller loop

Extracts each poller's `main()` loop body into a call to
`common::poller_loop::run_poll_loop` (Task A6). Sequenced after Group B
specifically for these 5 crates (not a hard technical dependency —
`run_poll_loop` takes an already-built `OAuthTokenCache`, it doesn't
require `InternalOAuthArgs` to exist) so each of these tasks' diff is "swap
the loop body for a helper call," not "flatten config *and* extract the
loop" at once (spec §5).

## Task C1: `poller-incidents` — adopt `run_poll_loop`

**Files:** modify `crates/poller-incidents/src/main.rs`.

Current, after Task B1 (`crates/poller-incidents/src/main.rs:28-87`,
`fn main`):

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = config.internal_oauth.token_cache();

    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let delay = ingest::time_until_next_poll(
        &client,
        &config.api_ingest_url,
        &internal_oauth,
        poll_interval,
    )
    .await;
    if !delay.is_zero() {
        tracing::info!(
            delay_secs = delay.as_secs(),
            "data still fresh from a prior run; delaying first poll"
        );
    }
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + delay, poll_interval);

    loop {
        interval.tick().await;

        let cycle_start = std::time::Instant::now();
        let result = poll_once(&client, &config, &internal_oauth).await;
        metrics::histogram!(
            common::metrics::metric_name("poller_cycle_duration_seconds"),
            "poller" => "incidents"
        )
        .record(cycle_start.elapsed().as_secs_f64());
        metrics::counter!(
            common::metrics::metric_name("poller_cycle_total"),
            "poller" => "incidents",
            "result" => if result.is_ok() { "success" } else { "failure" }
        )
        .increment(1);

        if let Err(err) = result {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}
```

- [ ] **Step 1: Replace with**

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = config.internal_oauth.token_cache();
    let poll_interval = Duration::from_secs(config.poll_interval_secs);

    common::poller_loop::run_poll_loop(
        "incidents",
        &client,
        &config.api_ingest_url,
        &internal_oauth,
        poll_interval,
        config.metrics.metrics_enabled,
        config.metrics_port,
        || poll_once(&client, &config, &internal_oauth),
    )
    .await
}
```

  `"incidents"` is the exact metric label value this poller already emits
  today (`"poller" => "incidents"`) — unchanged.

- [ ] **Step 2: Verify — metrics behavior unchanged (manual/optional) +
  full automated suite**

```bash
cargo build -p poller-incidents
cargo test -p poller-incidents
cargo clippy -p poller-incidents --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-incidents/src/main.rs
```

  Optional manual spot-check, if a local `api` stack is reachable: run
  `poller-incidents` with `METRICS_ENABLED=true`, `curl
  localhost:9091/metrics | grep poller_cycle` after one tick and confirm
  `poller="incidents"` still appears on both `poller_cycle_duration_seconds`
  and `poller_cycle_total`.

- [ ] **Step 3: Commit**

```bash
git add crates/poller-incidents/src/main.rs
git commit -m "poller-incidents: adopt common::poller_loop::run_poll_loop"
```

---

## Task C2: `poller-stations` — adopt `run_poll_loop`

**Files:** modify `crates/poller-stations/src/main.rs`.

Identical shape to Task C1, this crate's own `"stations"` label (already
used at its current `"poller" => "stations"` sites, `main.rs:73,78`).

- [ ] **Step 1: Same substitution as Task C1 Step 1**, with `"stations"` in
  place of `"incidents"`, at this crate's own `fn main` (`main.rs:29-87`).

- [ ] **Step 2: Verify**

```bash
cargo build -p poller-stations
cargo test -p poller-stations
cargo clippy -p poller-stations --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-stations/src/main.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/poller-stations/src/main.rs
git commit -m "poller-stations: adopt common::poller_loop::run_poll_loop"
```

---

## Task C3: `poller-tocs` — adopt `run_poll_loop`

**Files:** modify `crates/poller-tocs/src/main.rs`.

Identical shape, `"tocs"` label, `fn main` at `main.rs:29-87`.

- [ ] **Step 1: Same substitution as Task C1 Step 1**, with `"tocs"`.

- [ ] **Step 2: Verify**

```bash
cargo build -p poller-tocs
cargo test -p poller-tocs
cargo clippy -p poller-tocs --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-tocs/src/main.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/poller-tocs/src/main.rs
git commit -m "poller-tocs: adopt common::poller_loop::run_poll_loop"
```

---

## Task C4: `poller-ldbws` — adopt `run_poll_loop`

**Files:** modify `crates/poller-ldbws/src/main.rs`.

`fn main` at `main.rs:58-116`, `"ldbws"` label. `poll_once`
(`main.rs:120-...`) already calls `fetch_sample_stations` internally
(`:123`) — no signature change needed, `run_poll_loop`'s `cycle` closure
just calls `poll_once` exactly as today.

- [ ] **Step 1: Same substitution as Task C1 Step 1**, with `"ldbws"`, at
  this crate's own `fn main` boundaries.

- [ ] **Step 2: Verify**

```bash
cargo build -p poller-ldbws
cargo test -p poller-ldbws
cargo clippy -p poller-ldbws --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-ldbws/src/main.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/poller-ldbws/src/main.rs
git commit -m "poller-ldbws: adopt common::poller_loop::run_poll_loop"
```

---

## Task C5: `poller-tfl` — adopt `run_poll_loop` (mutable `dlr_state` capture)

**Files:** modify `crates/poller-tfl/src/main.rs`.

The one genuinely different case. Current, after Task B5
(`crates/poller-tfl/src/main.rs:77-141`, `fn main`):

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    require_non_empty_key(&config.tfl_app_key)?;
    if config.metrics.metrics_enabled {
        common::metrics::install(config.metrics_port)?;
    }
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = config.internal_oauth.token_cache();

    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let delay = ingest::time_until_next_poll(
        &client,
        &config.api_ingest_url,
        &internal_oauth,
        poll_interval,
    )
    .await;
    if !delay.is_zero() {
        tracing::info!(
            delay_secs = delay.as_secs(),
            "data still fresh from a prior run; delaying first poll"
        );
    }
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + delay, poll_interval);
    let mut dlr_state = dlr::inference::DlrMatchState::new();

    loop {
        interval.tick().await;

        let cycle_start = std::time::Instant::now();
        let result = poll_once(&client, &config, &mut dlr_state, &internal_oauth).await;
        metrics::histogram!(
            common::metrics::metric_name("poller_cycle_duration_seconds"),
            "poller" => "tfl"
        )
        .record(cycle_start.elapsed().as_secs_f64());
        metrics::counter!(
            common::metrics::metric_name("poller_cycle_total"),
            "poller" => "tfl",
            "result" => if result.is_ok() { "success" } else { "failure" }
        )
        .increment(1);

        if let Err(err) = result {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}
```

- [ ] **Step 1: Replace with**

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    require_non_empty_key(&config.tfl_app_key)?;
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;
    let internal_oauth = config.internal_oauth.token_cache();
    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let mut dlr_state = dlr::inference::DlrMatchState::new();

    common::poller_loop::run_poll_loop(
        "tfl",
        &client,
        &config.api_ingest_url,
        &internal_oauth,
        poll_interval,
        config.metrics.metrics_enabled,
        config.metrics_port,
        || poll_once(&client, &config, &mut dlr_state, &internal_oauth),
    )
    .await
}
```

  `require_non_empty_key` stays before everything, unchanged position and
  behavior (it only needs `config.tfl_app_key`, confirmed in the spec's
  §3.1 to be untouched by the shared loop). `dlr_state` is owned by this
  `main()` exactly as before; the `cycle` closure captures it by mutable
  reference (`&mut dlr_state`) each call — ordinary `FnMut` semantics
  (Open Question 2, above), no new abstraction needed.

- [ ] **Step 2: Verify**

```bash
cargo build -p poller-tfl
cargo test -p poller-tfl
cargo clippy -p poller-tfl --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-tfl/src/main.rs
```

  The existing `require_non_empty_key` unit tests
  (`crates/poller-tfl/src/main.rs:404-412`) are untouched by this task and
  must still pass unmodified.

- [ ] **Step 3: Commit**

```bash
git add crates/poller-tfl/src/main.rs
git commit -m "poller-tfl: adopt common::poller_loop::run_poll_loop, preserving dlr_state + pre-flight check ordering"
```

---

# Group D — `#3`, shared health/readiness

Moves `trust-consumer`/`full-coverage-consumer`'s already-near-identical
`health.rs` bodies (and `movement-relay`'s structurally similar but
semantically distinct one) onto the `health-http` crate scaffolded in
Task A7. **Group E is hard-blocked on this group finishing** — `ActiveFeed`
cannot reference a shared `ConnectionState` type until it exists.

## Task D1: `trust-consumer` — adopt `health-http`

**Files:** modify `crates/trust-consumer/Cargo.toml`,
`crates/trust-consumer/src/main.rs`,
`crates/trust-consumer/src/feed/kafka.rs`; delete
`crates/trust-consumer/src/health.rs`.

- [ ] **Step 1: `Cargo.toml`.** Current
  (`crates/trust-consumer/Cargo.toml:9,12`):

```toml
axum = { version = "0.8.9", features = ["http2"] }
...
common = { path = "../common" }
```

  Remove the `axum` line (confirmed via `grep -rln axum
  crates/trust-consumer/src/` that `health.rs` — being deleted this task —
  is this crate's *only* direct `axum` user); add `health-http = { path =
  "../health-http" }` alongside `common`:

```toml
common = { path = "../common" }
health-http = { path = "../health-http" }
```

- [ ] **Step 2: Delete `crates/trust-consumer/src/health.rs`** entirely.
  Its `set_connected`/`spawn`/`ConnectionState`/`healthz` all move to
  `health-http` (already scaffolded, Task A7) unchanged in shape.

- [ ] **Step 3: `main.rs` — remove the module, update call sites.** Remove
  `mod health;` (`main.rs:10`). Current
  (`crates/trust-consumer/src/main.rs:99` region, after Task B8):

```rust
    let connection_state = health::spawn(config.health_bind_url.clone());
```

  Change to:

```rust
    let connection_state =
        health_http::spawn(config.health_bind_url.clone(), "connected", "disconnected");
```

  Every other `health::ConnectionState`/`health::set_connected` reference
  in `main.rs` (the `ActiveFeed` block, `main.rs:34-89` — untouched by this
  task, Group E's job) becomes `health_http::ConnectionState`/
  `health_http::set_connected(..., "trust_consumer_ready", ...)`. Since
  Group E hasn't run yet, make this minimal, mechanical substitution now
  (find every `health::` reference in this file and requalify it as
  `health_http::`, adding the `"trust_consumer_ready"` gauge-name argument
  wherever `set_connected` is called):

```bash
grep -n "health::" crates/trust-consumer/src/main.rs
```

  For the one `health::set_connected(connection_state, result.is_ok())`
  call inside `ActiveFeed`'s `next_batch` impl (currently `main.rs:64`),
  change to `health_http::set_connected(connection_state,
  "trust_consumer_ready", result.is_ok())`. For the `RedisStream(Box<...>,
  health::ConnectionState)` variant field type (currently `main.rs:54`),
  change `health::ConnectionState` to `health_http::ConnectionState`.

- [ ] **Step 4: `feed/kafka.rs` — update call sites.** Current
  (`crates/trust-consumer/src/feed/kafka.rs:77,93`):

```rust
crate::health::set_connected(&self.connection_state, true);
```

```rust
crate::health::set_connected(&self.connection_state, false);
```

  Change to:

```rust
health_http::set_connected(&self.connection_state, "trust_consumer_ready", true);
```

```rust
health_http::set_connected(&self.connection_state, "trust_consumer_ready", false);
```

  Also update this file's own `connection_state: ConnectionState` field
  type (`:17`) and `connect`'s parameter type (`:36`) from a
  crate-local/`health::` reference to `health_http::ConnectionState` —
  confirm the exact current type path via
  `grep -n "ConnectionState" crates/trust-consumer/src/feed/kafka.rs`
  before editing (this file may already reference `crate::health::ConnectionState`
  by full path, or via a `use` — check and update whichever form is
  present).

- [ ] **Step 5: Verify — `/healthz` text and gauge name unchanged**

```bash
cargo build -p trust-consumer
grep -rn "health::" crates/trust-consumer/src/   # expect zero hits left
cargo test -p trust-consumer
cargo clippy -p trust-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/trust-consumer/src/main.rs crates/trust-consumer/src/feed/kafka.rs
```

  Manual/optional spot-check against a running instance:
  `curl localhost:<health_bind_url port>/healthz` — body text must still
  read `"connected"`/`"disconnected"` exactly as before this task; `curl
  localhost:<metrics_port>/metrics | grep trust_consumer_ready` — gauge
  name unchanged.

- [ ] **Step 6: Commit**

```bash
git add crates/trust-consumer/Cargo.toml crates/trust-consumer/src/main.rs crates/trust-consumer/src/feed/kafka.rs
git rm crates/trust-consumer/src/health.rs
git commit -m "trust-consumer: adopt shared health-http crate, delete local health.rs"
```

---

## Task D2: `full-coverage-consumer` — adopt `health-http`

**Files:** modify `crates/full-coverage-consumer/Cargo.toml`,
`crates/full-coverage-consumer/src/main.rs`,
`crates/full-coverage-consumer/src/feed/kafka.rs`; delete
`crates/full-coverage-consumer/src/health.rs`.

Identical shape to Task D1, this crate's gauge name
`"full_coverage_consumer_ready"`, `/healthz` text
`"connected"`/`"disconnected"` (unchanged). `feed/kafka.rs`'s two call
sites are at `:54,68` (per this plan's own fresh read); `main.rs`'s
`health::spawn` call and `ActiveFeed` block are at the same relative
positions as `trust-consumer`'s (`main.rs:113` region for `spawn`; the
`ActiveFeed` block itself, `main.rs:59-103`, stays untouched here — Group
E's job).

- [ ] **Step 1: `Cargo.toml`** — same substitution as Task D1 Step 1
  (remove `axum`, add `health-http = { path = "../health-http" }`).

- [ ] **Step 2: Delete `crates/full-coverage-consumer/src/health.rs`**
  entirely.

- [ ] **Step 3: `main.rs`** — same substitution as Task D1 Step 3, this
  crate's `"full_coverage_consumer_ready"` gauge name, `spawn` call
  becoming `health_http::spawn(config.health_bind_url.clone(), "connected",
  "disconnected")`.

- [ ] **Step 4: `feed/kafka.rs`** — same substitution as Task D1 Step 4,
  this crate's own lines (`:54,68`), gauge name
  `"full_coverage_consumer_ready"`.

- [ ] **Step 5: Verify**

```bash
cargo build -p full-coverage-consumer
grep -rn "health::" crates/full-coverage-consumer/src/   # expect zero hits left
cargo test -p full-coverage-consumer
cargo clippy -p full-coverage-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/full-coverage-consumer/src/main.rs crates/full-coverage-consumer/src/feed/kafka.rs
```

- [ ] **Step 6: Commit**

```bash
git add crates/full-coverage-consumer/Cargo.toml crates/full-coverage-consumer/src/main.rs crates/full-coverage-consumer/src/feed/kafka.rs
git rm crates/full-coverage-consumer/src/health.rs
git commit -m "full-coverage-consumer: adopt shared health-http crate, delete local health.rs"
```

---

## Task D3: `movement-relay` — adopt `health-http` (`spawn_with_state`)

**Files:** modify `crates/movement-relay/Cargo.toml`,
`crates/movement-relay/src/health.rs`, `crates/movement-relay/src/main.rs`.

This crate's readiness semantics ("confirmed Kafka partition assignment,"
not "the HTTP server answered") are load-bearing and must not change —
`movement-relay/src/health.rs:1-12`'s own module doc says so explicitly.
This task moves only the shared plumbing (`spawn`→`spawn_with_state`,
`healthz`, the inlined store+gauge pairs → `set_connected`), leaving
`RelayContext`'s own `ClientContext`/`ConsumerContext` impl and its
rebalance-branch logic untouched.

- [ ] **Step 1: `Cargo.toml`** — remove `axum` (confirmed
  `movement-relay/src/health.rs` is this crate's only direct `axum` user),
  add `health-http = { path = "../health-http" }`.

- [ ] **Step 2: Rewrite `crates/movement-relay/src/health.rs`.** Current
  (full file, reproduced from this plan's own read):

```rust
//! Readiness for `movement-relay` means "confirmed Kafka partition
//! assignment," NOT "the HTTP server answered" and NOT "at least one
//! message has arrived" ... Do not "fix" this inconsistency by making the
//! two match -- it is deliberate.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::http::StatusCode;
use axum::routing::get;
use rdkafka::ClientContext;
use rdkafka::consumer::{BaseConsumer, ConsumerContext, Rebalance};

pub type ReadyState = Arc<AtomicBool>;

pub struct RelayContext {
    pub ready: ReadyState,
}

impl ClientContext for RelayContext {}

impl ConsumerContext for RelayContext {
    fn post_rebalance(&self, _base_consumer: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        match rebalance {
            Rebalance::Assign(partitions) if !partitions.elements().is_empty() => {
                self.ready.store(true, Ordering::Relaxed);
                metrics::gauge!(common::metrics::metric_name("movement_relay_ready")).set(1.0);
                tracing::info!(...);
            }
            Rebalance::Revoke(_) => {
                self.ready.store(false, Ordering::Relaxed);
                metrics::gauge!(common::metrics::metric_name("movement_relay_ready")).set(0.0);
                tracing::warn!(...);
            }
            Rebalance::Error(err) => {
                self.ready.store(false, Ordering::Relaxed);
                metrics::gauge!(common::metrics::metric_name("movement_relay_ready")).set(0.0);
                tracing::error!(...);
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
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "no confirmed partition assignment",
        )
    }
}
```

  Replace with:

```rust
//! Readiness for `movement-relay` means "confirmed Kafka partition
//! assignment," NOT "the HTTP server answered" and NOT "at least one
//! message has arrived" (contrast with `trust-consumer`/
//! `full-coverage-consumer`'s own `ConnectionState`, which flips on
//! message arrival -- see
//! docs/superpowers/specs/2026-09-04-movement-relay-design.md Decision 5
//! for why the two crates deliberately differ here. Do not "fix" this
//! inconsistency by making the two match -- it is deliberate.
//!
//! The shared HTTP/gauge plumbing (`spawn_with_state`/`healthz`/
//! `set_connected`) now lives in `health-http`; only this crate's own
//! Kafka-rebalance-driven state transitions stay here.

use std::sync::atomic::Ordering;

use rdkafka::ClientContext;
use rdkafka::consumer::{BaseConsumer, ConsumerContext, Rebalance};

pub struct RelayContext {
    pub ready: health_http::ConnectionState,
}

impl ClientContext for RelayContext {}

impl ConsumerContext for RelayContext {
    fn post_rebalance(&self, _base_consumer: &BaseConsumer<Self>, rebalance: &Rebalance<'_>) {
        match rebalance {
            Rebalance::Assign(partitions) if !partitions.elements().is_empty() => {
                health_http::set_connected(&self.ready, "movement_relay_ready", true);
                tracing::info!(
                    partitions = partitions.elements().len(),
                    "movement-relay: Kafka partition assignment confirmed; readiness now true"
                );
            }
            Rebalance::Revoke(_) => {
                health_http::set_connected(&self.ready, "movement_relay_ready", false);
                tracing::warn!("movement-relay: Kafka partitions revoked; readiness now false");
            }
            Rebalance::Error(err) => {
                health_http::set_connected(&self.ready, "movement_relay_ready", false);
                tracing::error!(error = ?err, "movement-relay: Kafka rebalance error; readiness now false");
            }
            _ => {}
        }
    }
}
```

  (`Ordering` import stays only if still used elsewhere in this file after
  the edit — verify via `cargo build`'s warnings and drop it if unused.)

- [ ] **Step 3: `main.rs`.** Current (`crates/movement-relay/src/main.rs:34-35`):

```rust
    let ready: health::ReadyState = Arc::new(AtomicBool::new(false));
    health::spawn(config.health_bind_url.clone(), Arc::clone(&ready));
```

  Change to:

```rust
    let ready: health_http::ConnectionState = Arc::new(AtomicBool::new(false));
    health_http::spawn_with_state(
        config.health_bind_url.clone(),
        Arc::clone(&ready),
        "partitions assigned",
        "no confirmed partition assignment",
    );
```

- [ ] **Step 4: Verify**

```bash
cargo build -p movement-relay
grep -rn "\bhealth::" crates/movement-relay/src/   # expect zero hits left
cargo test -p movement-relay
cargo clippy -p movement-relay --all-features -- -D warnings
rustfmt --edition 2024 crates/movement-relay/src/health.rs crates/movement-relay/src/main.rs
```

  Manual/optional spot-check: `curl localhost:<health_bind_url
  port>/healthz` before/after a real partition assignment must still read
  `"no confirmed partition assignment"` / `"partitions assigned"`
  respectively — unchanged text.

- [ ] **Step 5: Commit**

```bash
git add crates/movement-relay/Cargo.toml crates/movement-relay/src/health.rs crates/movement-relay/src/main.rs
git commit -m "movement-relay: adopt shared health-http crate (spawn_with_state + set_connected), readiness semantics unchanged"
```

---

# Group E — `#5`, `ActiveFeed`

**Hard-blocked on Group D.** `ActiveFeed`'s only remaining crate-tie is
`ConnectionState`, which must already be `health_http::ConnectionState` — a
type external to both `trust-consumer` and `full-coverage-consumer` —
before `movement-feed` can hold `ActiveFeed` without picking one crate's own
`health` module arbitrarily.

## Task E1: `movement-feed` — add generic `ActiveFeed<K>` + `MovementFeedBackend`

**Files:** create `crates/movement-feed/src/active_feed.rs`; modify
`crates/movement-feed/src/lib.rs`, `crates/movement-feed/Cargo.toml`.

**Interfaces produced:** `pub enum MovementFeedBackend { Kafka,
RedisStream }`, `pub enum ActiveFeed<K: MovementFeed> { Kafka(K),
RedisStream(Box<RedisStreamMovementFeed>, health_http::ConnectionState,
&'static str) }` implementing `MovementFeed`, plus an inherent `async fn
check_gap(&mut self) -> anyhow::Result<Option<GapInfo>>`. See "Open
Questions — Resolved" / "Fresh-verification corrections" above for why
`ActiveFeed` is generic (`KafkaMovementFeed` is genuinely crate-local, not
shared, contrary to the spec's own §3.5 claim) and why `RedisStream` carries
a third `&'static str` field (the gauge name is per-caller).

- [ ] **Step 1: `Cargo.toml`.** Current
  (`crates/movement-feed/Cargo.toml`):

```toml
[dependencies]
anyhow = "1.0.104"
async-trait = "0.1.92"
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
tracing = "0.1.44"
```

  Add `clap` (derive only — this crate has no CLI of its own, it only needs
  the `ValueEnum` derive macro) and `health-http`:

```toml
[dependencies]
anyhow = "1.0.104"
async-trait = "0.1.92"
clap = { version = "4.6.6", features = ["derive"] }
health-http = { path = "../health-http" }
redis = { version = "0.27", features = ["tokio-comp", "connection-manager"] }
tracing = "0.1.44"
```

- [ ] **Step 2: Create `crates/movement-feed/src/active_feed.rs`**

```rust
//! `ActiveFeed<K>` + `MovementFeedBackend`: shared dispatch between
//! whichever concrete `MovementFeed` backend a caller selected. Previously
//! duplicated near-verbatim (differing only in doc-comment wording) across
//! `trust-consumer`'s and `full-coverage-consumer`'s own `main.rs`/
//! `config.rs`. See
//! docs/superpowers/specs/2026-09-05-rust-service-deduplication-design.md
//! §3.5 -- and this plan's own "Fresh-verification corrections" section
//! for why `ActiveFeed` is generic over its Kafka variant: `KafkaMovementFeed`
//! is a distinct, crate-local type per caller (each crate's own
//! `feed::kafka` module, scheduled for deletion in Deploy C, out of scope
//! here), not actually shared the way the design spec assumed. Sharing it
//! would require either merging those two modules (out of scope) or
//! picking one crate's own type arbitrarily (worse than the status quo);
//! a generic parameter avoids both.

use crate::redis_stream::{GapInfo, RedisStreamMovementFeed};
use crate::MovementFeed;

/// Which transport a `MovementFeed` consumer uses. Verbatim move of the
/// two byte-identical enums previously duplicated in
/// `trust-consumer/src/config.rs` and
/// `full-coverage-consumer/src/config.rs`.
#[derive(Debug, Clone, Copy, clap::ValueEnum, PartialEq, Eq)]
pub enum MovementFeedBackend {
    /// A direct Kafka consumer, via each caller's own crate-local
    /// `feed::kafka::KafkaMovementFeed`.
    Kafka,
    /// The Redis Streams reader (`RedisStreamMovementFeed`, this crate),
    /// reading what `movement-relay` publishes.
    RedisStream,
}

/// Wraps whichever concrete `MovementFeed` backend was selected. Generic
/// over `K` (each caller's own Kafka implementation) -- see this module's
/// own doc for why. The `RedisStream` variant's third field is the
/// Prometheus gauge name to report readiness under (e.g.
/// `"trust_consumer_ready"` / `"full_coverage_consumer_ready"`) --
/// per-caller, so it's supplied at construction time rather than hardcoded
/// inside this now-shared type's own `next_batch` impl.
pub enum ActiveFeed<K: MovementFeed> {
    Kafka(K),
    RedisStream(Box<RedisStreamMovementFeed>, health_http::ConnectionState, &'static str),
}

#[async_trait::async_trait]
impl<K: MovementFeed> MovementFeed for ActiveFeed<K> {
    async fn next_batch(&mut self) -> anyhow::Result<Vec<String>> {
        match self {
            ActiveFeed::Kafka(feed) => feed.next_batch().await,
            ActiveFeed::RedisStream(feed, connection_state, gauge_name) => {
                let result = feed.next_batch().await;
                health_http::set_connected(connection_state, gauge_name, result.is_ok());
                result
            }
        }
    }

    async fn commit(&mut self) -> anyhow::Result<()> {
        match self {
            ActiveFeed::Kafka(feed) => feed.commit().await,
            ActiveFeed::RedisStream(feed, _, _) => feed.commit().await,
        }
    }
}

impl<K: MovementFeed> ActiveFeed<K> {
    /// `Ok(None)` immediately for the `Kafka` variant (no analog);
    /// delegates to `RedisStreamMovementFeed::check_gap` for the
    /// `RedisStream` variant.
    pub async fn check_gap(&mut self) -> anyhow::Result<Option<GapInfo>> {
        match self {
            ActiveFeed::Kafka(_) => Ok(None),
            ActiveFeed::RedisStream(feed, _, _) => feed.check_gap().await,
        }
    }
}
```

- [ ] **Step 3: Wire into `lib.rs`.** Current
  (`crates/movement-feed/src/lib.rs:19`):

```rust
pub mod redis_stream;
```

  Change to:

```rust
pub mod active_feed;
pub mod redis_stream;

pub use active_feed::{ActiveFeed, MovementFeedBackend};
```

- [ ] **Step 4: Unit test** — add to `active_feed.rs`, using the crate's
  existing `FakeMovementFeed` test double for the `Kafka` variant (no real
  Kafka connection needed):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::FakeMovementFeed;

    #[tokio::test]
    async fn kafka_variant_check_gap_is_always_none() {
        let mut feed: ActiveFeed<FakeMovementFeed> =
            ActiveFeed::Kafka(FakeMovementFeed::new(vec![]));
        assert_eq!(feed.check_gap().await.unwrap(), None);
    }

    #[tokio::test]
    async fn kafka_variant_delegates_next_batch_and_commit() {
        let mut feed: ActiveFeed<FakeMovementFeed> =
            ActiveFeed::Kafka(FakeMovementFeed::new(vec![vec!["one".to_string()]]));
        let batch = feed.next_batch().await.unwrap();
        assert_eq!(batch, vec!["one".to_string()]);
        feed.commit().await.unwrap();
    }
}
```

  (Confirm `FakeMovementFeed`'s real constructor name/shape via
  `grep -n "impl FakeMovementFeed" crates/movement-feed/src/lib.rs` before
  writing this test — this plan's own research read `FakeMovementFeed`'s
  struct fields but not its full `impl` block; adjust the constructor call
  above to match whatever's actually there, e.g. `FakeMovementFeed::new`
  may take different arguments.)

- [ ] **Step 5: Verify**

```bash
cargo build -p movement-feed
cargo build -p movement-feed --features test-util
cargo test -p movement-feed --features test-util
cargo clippy -p movement-feed --all-features -- -D warnings
rustfmt --edition 2024 crates/movement-feed/src/active_feed.rs crates/movement-feed/src/lib.rs
```

- [ ] **Step 6: Commit**

```bash
git add crates/movement-feed/Cargo.toml crates/movement-feed/src/active_feed.rs crates/movement-feed/src/lib.rs
git commit -m "movement-feed: add generic ActiveFeed<K> + MovementFeedBackend (not yet adopted)"
```

---

## Task E2: `trust-consumer` — adopt shared `ActiveFeed`/`MovementFeedBackend`

**Files:** modify `crates/trust-consumer/src/config.rs`,
`crates/trust-consumer/src/main.rs`.

- [ ] **Step 1: `config.rs` — replace the local enum with a re-export.**
  Current (`crates/trust-consumer/src/config.rs:11-33`, the doc comment +
  enum):

```rust
/// Which transport this crate's `MovementFeed` uses. See
/// docs/superpowers/plans/2026-09-04-movement-relay-plan.md, "Judgment
/// calls," item 5, for why this exists: ...
#[derive(Debug, Clone, Copy, clap::ValueEnum, PartialEq, Eq)]
pub enum MovementFeedBackend {
    /// Today's production default -- a direct Kafka consumer via
    /// `feed::kafka::KafkaMovementFeed`. Unchanged behavior from before
    /// this plan.
    Kafka,
    /// The new Redis Streams reader ...
    RedisStream,
}
```

  Replace with:

```rust
/// Which transport this crate's `MovementFeed` uses -- now defined once in
/// `movement_feed`, re-exported here so every existing
/// `use config::{Config, MovementFeedBackend};` import (and this file's own
/// `#[arg(..., value_enum, default_value_t = MovementFeedBackend::Kafka)]`)
/// keeps resolving unchanged.
pub use movement_feed::MovementFeedBackend;
```

- [ ] **Step 2: `main.rs` — remove the local `ActiveFeed`, import the
  shared one.** Delete the entire block at `main.rs:34-89` (the doc
  comment, `enum ActiveFeed`, `impl MovementFeed for ActiveFeed`, `impl
  ActiveFeed { check_gap }`). Update the import block. Current
  (`main.rs:18-22`):

```rust
use clap::Parser;
use config::{Config, MovementFeedBackend};
use feed::MovementFeed;
use feed::kafka::KafkaMovementFeed;
use movement_feed::redis_stream::{GapInfo, RedisStreamMovementFeed};
```

  Change to:

```rust
use clap::Parser;
use config::{Config, MovementFeedBackend};
use feed::MovementFeed;
use feed::kafka::KafkaMovementFeed;
use movement_feed::redis_stream::{GapInfo, RedisStreamMovementFeed};
use movement_feed::ActiveFeed;
```

  (Keep the `GapInfo`/`RedisStreamMovementFeed` import if this file's own
  code still references `GapInfo`'s type directly elsewhere — confirm via
  `cargo build`'s unused-import warning; `RedisStreamMovementFeed` is still
  needed at the construction call site below.)

- [ ] **Step 3: Update the two `ActiveFeed` construction call sites.**
  Current (from this plan's own full read of `main.rs`'s setup section):

```rust
    let mut feed = match config.movement_feed_backend {
        MovementFeedBackend::Kafka => {
            ActiveFeed::Kafka(KafkaMovementFeed::connect(&config, connection_state)?)
        }
        MovementFeedBackend::RedisStream => ActiveFeed::RedisStream(
            Box::new(
                RedisStreamMovementFeed::connect(
                    &config.redis_url,
                    "trust-consumer",
                    "trust-consumer-1",
                    Duration::from_secs(config.redis_autoclaim_min_idle_secs),
                )
                .await?,
            ),
            connection_state,
        ),
    };
```

  Change the `RedisStream` arm to pass the gauge name as a third
  constructor argument (the `Kafka` arm is unchanged — `K` is inferred as
  `KafkaMovementFeed`):

```rust
    let mut feed = match config.movement_feed_backend {
        MovementFeedBackend::Kafka => {
            ActiveFeed::Kafka(KafkaMovementFeed::connect(&config, connection_state)?)
        }
        MovementFeedBackend::RedisStream => ActiveFeed::RedisStream(
            Box::new(
                RedisStreamMovementFeed::connect(
                    &config.redis_url,
                    "trust-consumer",
                    "trust-consumer-1",
                    Duration::from_secs(config.redis_autoclaim_min_idle_secs),
                )
                .await?,
            ),
            connection_state,
            "trust_consumer_ready",
        ),
    };
```

  Every other use of `feed` (`.next_batch()`, `.commit()`, `.check_gap()`)
  is unchanged — same trait/inherent method names, now resolved against
  `movement_feed::ActiveFeed` instead of the deleted local type.

- [ ] **Step 4: Verify**

```bash
cargo build -p trust-consumer
cargo test -p trust-consumer
cargo clippy -p trust-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/trust-consumer/src/config.rs crates/trust-consumer/src/main.rs
```

  Re-run the existing `movement_feed_backend_defaults_to_kafka_when_unset`
  test explicitly (it constructs a real `Config` via `try_parse_from` and
  asserts `config.movement_feed_backend == MovementFeedBackend::Kafka` —
  must still pass unmodified against the re-exported type):

```bash
cargo test -p trust-consumer movement_feed_backend_defaults_to_kafka_when_unset
```

- [ ] **Step 5: Commit**

```bash
git add crates/trust-consumer/src/config.rs crates/trust-consumer/src/main.rs
git commit -m "trust-consumer: adopt shared movement_feed::ActiveFeed + MovementFeedBackend"
```

---

## Task E3: `full-coverage-consumer` — adopt shared `ActiveFeed`/`MovementFeedBackend`

**Files:** modify `crates/full-coverage-consumer/src/config.rs`,
`crates/full-coverage-consumer/src/main.rs`.

Identical shape to Task E2. Local enum at `config.rs:25-34`; local
`ActiveFeed` block at `main.rs:54-103` (doc comment starting at `:54`,
`enum ActiveFeed` at `:59`); this crate's own gauge name
`"full_coverage_consumer_ready"`, consumer-group labels
`"full-coverage-consumer"` / `"full-coverage-consumer-1"`.

- [ ] **Step 1: `config.rs`** — same substitution as Task E2 Step 1:
  replace the local `MovementFeedBackend` enum (`:25-34`) with `pub use
  movement_feed::MovementFeedBackend;`.

- [ ] **Step 2: `main.rs` — remove local `ActiveFeed`, update imports.**
  Delete `main.rs:54-103`. Current import block
  (`main.rs:47-51`):

```rust
use clap::Parser;
use config::{Config, MovementFeedBackend};
use feed::MovementFeed;
use feed::kafka::KafkaMovementFeed;
use movement_feed::redis_stream::{GapInfo, RedisStreamMovementFeed};
```

  Add `use movement_feed::ActiveFeed;`, same as Task E2 Step 2.

- [ ] **Step 3: Update the `ActiveFeed` construction call site.** Current
  (from this plan's own full read of `main.rs`'s setup section):

```rust
    let mut feed = match config.movement_feed_backend {
        MovementFeedBackend::Kafka => {
            ActiveFeed::Kafka(KafkaMovementFeed::connect(&config, connection_state)?)
        }
        MovementFeedBackend::RedisStream => ActiveFeed::RedisStream(
            Box::new(
                RedisStreamMovementFeed::connect(
                    &config.redis_url,
                    "full-coverage-consumer",
                    "full-coverage-consumer-1",
                    Duration::from_secs(config.redis_autoclaim_min_idle_secs),
                )
                .await?,
            ),
            connection_state,
        ),
    };
```

  Add the third constructor argument to the `RedisStream` arm:

```rust
            connection_state,
            "full_coverage_consumer_ready",
        ),
    };
```

- [ ] **Step 4: Verify**

```bash
cargo build -p full-coverage-consumer
cargo test -p full-coverage-consumer
cargo clippy -p full-coverage-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/full-coverage-consumer/src/config.rs crates/full-coverage-consumer/src/main.rs
```

- [ ] **Step 5: Commit**

```bash
git add crates/full-coverage-consumer/src/config.rs crates/full-coverage-consumer/src/main.rs
git commit -m "full-coverage-consumer: adopt shared movement_feed::ActiveFeed + MovementFeedBackend"
```

---

# Group F — `#6`, `common::ingest::get_json`/`post_json`

Fully independent of Groups A–E; could run at any point, including in
parallel with all of them (spec §5's own note). Also resolves Open
Question 5 (`fetch_last_fetched` drive-by rewrite).

## Task F1: `common::ingest` — add `get_json`/`post_json`, rewrite `post_batch`/`fetch_last_fetched`

**Files:** modify `crates/common/src/ingest.rs`.

**Interfaces produced:** `pub async fn get_json<T: DeserializeOwned>(client:
&reqwest::Client, url: &str, tokens: &OAuthTokenCache) ->
anyhow::Result<T>`, `pub async fn post_json<T: Serialize>(client:
&reqwest::Client, url: &str, tokens: &OAuthTokenCache, body: &T) ->
anyhow::Result<()>`. Groups F2–F6 depend on these two signatures.

- [ ] **Step 1: Add `use serde::de::DeserializeOwned;`** to the existing
  import block (`crates/common/src/ingest.rs:21`, alongside `use
  serde::{Deserialize, Serialize};` — merge into one `use serde::{de::DeserializeOwned,
  Deserialize, Serialize};` or add a second `use` line, matching this
  file's existing style).

- [ ] **Step 2: Add `get_json`/`post_json`, and rewrite `post_batch` in
  terms of `post_json`.** Current `post_batch`
  (`crates/common/src/ingest.rs:39-62`):

```rust
pub async fn post_batch<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
    items: &[T],
    noun: &str,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(url)
        .bearer_auth(&token)
        .json(items)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::info!(count = items.len(), "posted {noun} to ingestion API");
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("ingestion POST failed: {status} {text}");
    }
}
```

  Change to (adds `get_json`/`post_json`, keeps `post_batch`'s own public
  signature and behavior identical, per the spec's §3.6 design — callers
  are unaffected):

```rust
/// GET + bearer-token + deserialize -- the shape every "fetch one typed
/// resource from `api`'s `/private/*` routes" caller repeats. Previously
/// duplicated with no shared logic beyond this (already trivial) shape
/// across `poller-ldbws::fetch_sample_stations`,
/// `trust_consumer::queries::{fetch_active_tracked_trains,fetch_stanox_crs}`,
/// and `full_coverage_consumer::queries::fetch_stanox_crs`.
pub async fn get_json<T: DeserializeOwned>(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<T> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

/// Single-object POST + bearer-token -- deliberately distinct from
/// `post_batch`'s array-wrapping shape (wrapping a single record in a
/// one-element slice would change the wire shape, not match it).
/// Previously duplicated across `schedule-ingest::main::post_ingest` and
/// `schedule-reference::main::post_schedule_line_population`.
pub async fn post_json<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
    body: &T,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("POST failed: {status} {text}");
    }
}

/// POSTs `items` as a JSON array to `url` with a fresh
/// `Authorization: Bearer` token from `tokens`, then logs and returns
/// `Ok(())` on a 2xx response, or bails with an `anyhow::Error` (including
/// status + response body) otherwise.
///
/// `noun` is used only in the success log line (e.g. `"incidents"`,
/// `"stations"`, `"tocs"`) — callers pass their own plural label. A thin
/// wrapper over [`post_json`]: `items` (a slice) already serializes as a
/// JSON array via `serde`, so this adds only the success log line
/// `post_json` itself doesn't have — no change to this function's own
/// public signature or behavior.
pub async fn post_batch<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
    items: &[T],
    noun: &str,
) -> anyhow::Result<()> {
    post_json(client, url, tokens, &items).await?;
    tracing::info!(count = items.len(), "posted {noun} to ingestion API");
    Ok(())
}
```

  Note: `post_json`'s own error message (`"POST failed: {status} {text}"`)
  intentionally differs from `post_batch`'s previous inline error message
  (`"ingestion POST failed: {status} {text}"`) since `post_json` is a new,
  more general function without "ingestion" framing baked in — this is a
  new function's own message, not a change to any *existing* caller's
  observed error text, since `post_batch`'s own error path in the new
  version comes from `post_json`'s `Err` propagating through `?`... **this
  is a real, if narrow, wire/log-text change for every existing
  `post_batch` caller's failure-path log message.** To avoid it, keep
  `post_batch`'s own error branch inline instead of delegating error
  formatting to `post_json`:

```rust
pub async fn post_batch<T: Serialize>(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
    items: &[T],
    noun: &str,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(url)
        .bearer_auth(&token)
        .json(items)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::info!(count = items.len(), "posted {noun} to ingestion API");
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("ingestion POST failed: {status} {text}");
    }
}
```

  **Use this second version** (`post_batch` left exactly as it is today,
  untouched) — it is simpler, has zero risk of an observable message
  change, and the spec's own §3.6 only required `post_batch`'s body
  "become a thin call to `post_json`" as a nice-to-have illustration, not a
  hard requirement; this plan prioritizes "zero behavior change anywhere"
  (Global Constraints) over that one-line simplification. Add only
  `get_json`/`post_json` as new functions; do not modify `post_batch`.

- [ ] **Step 3: Rewrite `fetch_last_fetched` in terms of `get_json`**
  (Open Question 5 — yes). Current
  (`crates/common/src/ingest.rs:104-118`):

```rust
async fn fetch_last_fetched(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Option<DateTime<Utc>>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    let body: LastFetchedResponse = response.json().await?;
    Ok(body.fetched_at)
}
```

  Change to:

```rust
async fn fetch_last_fetched(
    client: &reqwest::Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Option<DateTime<Utc>>> {
    let body: LastFetchedResponse = get_json(client, url, tokens).await?;
    Ok(body.fetched_at)
}
```

- [ ] **Step 4: Unit tests for `get_json`/`post_json`**, matching this
  file's existing style (no dedicated test module for HTTP helpers exists
  yet in `ingest.rs` — its `#[cfg(test)] mod tests` currently only covers
  `duration_until_next_poll`; add `wiremock`-based tests for the two new
  functions in the same module, mirroring `oauth_client.rs`'s own test
  style):

```rust
    #[tokio::test]
    async fn get_json_deserializes_a_successful_response() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fake-jwt",
                "expires_in": 300,
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/thing"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "value": 42
            })))
            .mount(&server)
            .await;

        let tokens = crate::oauth_client::OAuthTokenCache::new(crate::oauth_client::OAuthCredentials {
            token_url: format!("{}/token/", server.uri()),
            client_id: "c".to_string(),
            scope: "groups".to_string(),
            username: "u".to_string(),
            password: "p".to_string(),
        });
        let client = reqwest::Client::new();

        #[derive(serde::Deserialize)]
        struct Thing {
            value: u32,
        }
        let thing: Thing = get_json(&client, &format!("{}/thing", server.uri()), &tokens)
            .await
            .unwrap();
        assert_eq!(thing.value, 42);
    }

    #[tokio::test]
    async fn post_json_posts_the_body_and_returns_ok_on_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "fake-jwt",
                "expires_in": 300,
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/thing"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;

        let tokens = crate::oauth_client::OAuthTokenCache::new(crate::oauth_client::OAuthCredentials {
            token_url: format!("{}/token/", server.uri()),
            client_id: "c".to_string(),
            scope: "groups".to_string(),
            username: "u".to_string(),
            password: "p".to_string(),
        });
        let client = reqwest::Client::new();

        #[derive(serde::Serialize)]
        struct Thing {
            value: u32,
        }
        post_json(&client, &format!("{}/thing", server.uri()), &tokens, &Thing { value: 1 })
            .await
            .unwrap();
    }
```

- [ ] **Step 5: Verify — every existing test in this file still passes**
  (the `fetch_last_fetched` rewrite must not change
  `time_until_next_poll`'s own observable behavior, since
  `duration_until_next_poll`'s tests below it are pure and untouched)

```bash
cargo build -p common
cargo test -p common
cargo clippy -p common --all-features -- -D warnings
rustfmt --edition 2024 crates/common/src/ingest.rs
```

- [ ] **Step 6: Commit**

```bash
git add crates/common/src/ingest.rs
git commit -m "common: add ingest::get_json/post_json, rewrite fetch_last_fetched in terms of get_json"
```

---

## Task F2: `poller-ldbws` — `fetch_sample_stations` → `get_json`

**Files:** modify `crates/poller-ldbws/src/main.rs`.

Current (`crates/poller-ldbws/src/main.rs:160-174`):

```rust
async fn fetch_sample_stations(
    client: &Client,
    config: &Config,
    tokens: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<String>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(&config.api_sample_stations_url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;

    Ok(response.json::<Vec<String>>().await?)
}
```

- [ ] **Step 1: Replace with**

```rust
async fn fetch_sample_stations(
    client: &Client,
    config: &Config,
    tokens: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<String>> {
    common::ingest::get_json(client, &config.api_sample_stations_url, tokens).await
}
```

- [ ] **Step 2: Verify**

```bash
cargo build -p poller-ldbws
cargo test -p poller-ldbws
cargo clippy -p poller-ldbws --all-features -- -D warnings
rustfmt --edition 2024 crates/poller-ldbws/src/main.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/poller-ldbws/src/main.rs
git commit -m "poller-ldbws: use common::ingest::get_json for fetch_sample_stations"
```

---

## Task F3: `trust-consumer` — `queries.rs` GETs → `get_json`

**Files:** modify `crates/trust-consumer/src/queries.rs`.

Current (`crates/trust-consumer/src/queries.rs:13-41`):

```rust
pub async fn fetch_active_tracked_trains(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Vec<TrackedTrainRef>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}

pub async fn fetch_stanox_crs(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}
```

- [ ] **Step 1: Replace both with**

```rust
pub async fn fetch_active_tracked_trains(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Vec<TrackedTrainRef>> {
    common::ingest::get_json(client, url, tokens).await
}

pub async fn fetch_stanox_crs(
    client: &Client,
    url: &str,
    tokens: &OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    common::ingest::get_json(client, url, tokens).await
}
```

- [ ] **Step 2: Verify**

```bash
cargo build -p trust-consumer
cargo test -p trust-consumer
cargo clippy -p trust-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/trust-consumer/src/queries.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/trust-consumer/src/queries.rs
git commit -m "trust-consumer: use common::ingest::get_json in queries.rs"
```

---

## Task F4: `full-coverage-consumer` — `queries.rs::fetch_stanox_crs` → `get_json`

**Files:** modify `crates/full-coverage-consumer/src/queries.rs`.

Current (`crates/full-coverage-consumer/src/queries.rs:26-43`):

```rust
pub async fn fetch_stanox_crs(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    // Identical to trust_consumer::queries::fetch_stanox_crs -- not
    // extracted into trust-schema (Task 1's scope was parsing/dedup/
    // journey only, deliberately not HTTP client code, which has no
    // shared logic beyond "GET + bearer + deserialize," already trivial).
    let token = tokens.get_token(client).await?;
    let response = client
        .get(url)
        .bearer_auth(&token)
        .send()
        .await?
        .error_for_status()?;
    Ok(response.json().await?)
}
```

  (`fetch_line_population`, above it in this file, is **not** touched by
  this task — it appends `.query(&[...])` before `.bearer_auth`, a shape
  `get_json` doesn't support and was never part of this finding's scope.)

- [ ] **Step 1: Replace `fetch_stanox_crs` with**

```rust
pub async fn fetch_stanox_crs(
    client: &reqwest::Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<Vec<common::StanoxCrsRecord>> {
    common::ingest::get_json(client, url, tokens).await
}
```

- [ ] **Step 2: Verify**

```bash
cargo build -p full-coverage-consumer
cargo test -p full-coverage-consumer
cargo clippy -p full-coverage-consumer --all-features -- -D warnings
rustfmt --edition 2024 crates/full-coverage-consumer/src/queries.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/full-coverage-consumer/src/queries.rs
git commit -m "full-coverage-consumer: use common::ingest::get_json for fetch_stanox_crs"
```

---

## Task F5: `schedule-ingest` — `post_ingest` → `post_json`

**Files:** modify `crates/schedule-ingest/src/main.rs`.

Current (`crates/schedule-ingest/src/main.rs:342-367`):

```rust
async fn post_ingest(
    client: &Client,
    config: &Config,
    tokens: &common::oauth_client::OAuthTokenCache,
    request: &ScheduleFeedIngestRequest,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(&config.api_ingest_url)
        .bearer_auth(&token)
        .json(request)
        .send()
        .await?;

    if response.status().is_success() {
        tracing::info!(
            delivered_at = %request.delivered_at,
            files = request.files.len(),
            "posted schedule feed ingest to api"
        );
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("schedule feed ingest POST failed: {status} {text}");
    }
}
```

  `post_json`'s own error text (`"POST failed: {status} {text}"`) differs
  from this function's current error text (`"schedule feed ingest POST
  failed: {status} {text}"`) — per Global Constraints (no wire/log-text
  changes), keep this function's own distinct error message and success
  log line, delegating only the mechanical bearer-fetch+send+status-check
  to `post_json`, catching its `Err` to preserve this crate's own message:

- [ ] **Step 1: Replace with**

```rust
async fn post_ingest(
    client: &Client,
    config: &Config,
    tokens: &common::oauth_client::OAuthTokenCache,
    request: &ScheduleFeedIngestRequest,
) -> anyhow::Result<()> {
    common::ingest::post_json(client, &config.api_ingest_url, tokens, request)
        .await
        .map_err(|err| anyhow::anyhow!("schedule feed ingest POST failed: {err}"))?;
    tracing::info!(
        delivered_at = %request.delivered_at,
        files = request.files.len(),
        "posted schedule feed ingest to api"
    );
    Ok(())
}
```

  This preserves the success log line exactly; the failure path's message
  is `"schedule feed ingest POST failed: {inner post_json error}"`, where
  `{inner post_json error}` is `"POST failed: {status} {text}"` — the
  overall failure message text changes shape slightly (double-wrapped) but
  still contains the same status/body information. **If exact
  byte-for-byte error text preservation is required** (this crate's own
  error message is not asserted against in any existing test — confirm via
  `grep -rn "schedule feed ingest POST failed" crates/schedule-ingest/`
  before deciding), the safer alternative is to leave `post_ingest`
  entirely untouched and skip this task, since the spec's own §3.6 lists
  this as a genuine duplicate to close but Global Constraints' "zero
  behavior change" bar is stricter than the spec's own dedup goal here.
  **Recommended default: apply the `map_err` version above** (log/error
  text is not a deployment-affecting default, flag/env, or metric name —
  it falls outside this plan's hardest constraint — but note the shape
  change explicitly in the commit message so it's easy to find later if it
  turns out something did depend on the exact old string).

- [ ] **Step 2: Verify — confirm nothing asserts the old exact string**

```bash
grep -rn "schedule feed ingest POST failed" crates/schedule-ingest/
cargo build -p schedule-ingest
cargo test -p schedule-ingest
cargo clippy -p schedule-ingest --all-features -- -D warnings
rustfmt --edition 2024 crates/schedule-ingest/src/main.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/schedule-ingest/src/main.rs
git commit -m "schedule-ingest: use common::ingest::post_json for post_ingest"
```

---

## Task F6: `schedule-reference` — `post_schedule_line_population` → `post_json`

**Files:** modify `crates/schedule-reference/src/main.rs`.

Current (`crates/schedule-reference/src/main.rs:372-393`):

```rust
async fn post_schedule_line_population(
    client: &Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    body: &serde_json::Value,
) -> anyhow::Result<()> {
    let token = tokens.get_token(client).await?;
    let response = client
        .post(url)
        .bearer_auth(&token)
        .json(body)
        .send()
        .await?;

    if response.status().is_success() {
        Ok(())
    } else {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("schedule-line-population POST failed: {status} {text}");
    }
}
```

  Unlike Task F5, this function has **no success-path side effect** (no log
  line) beyond `Ok(())` — a much cleaner drop-in for `post_json`, whose own
  `Ok(())` matches exactly. Only the error message text differs
  (`"schedule-line-population POST failed: ..."` vs. `post_json`'s
  `"POST failed: ..."`), same trade-off as Task F5.

- [ ] **Step 1: Replace with**

```rust
async fn post_schedule_line_population(
    client: &Client,
    url: &str,
    tokens: &common::oauth_client::OAuthTokenCache,
    body: &serde_json::Value,
) -> anyhow::Result<()> {
    common::ingest::post_json(client, url, tokens, body)
        .await
        .map_err(|err| anyhow::anyhow!("schedule-line-population POST failed: {err}"))
}
```

- [ ] **Step 2: Verify — confirm nothing asserts the old exact string**

```bash
grep -rn "schedule-line-population POST failed" crates/schedule-reference/
cargo build -p schedule-reference
cargo test -p schedule-reference
cargo clippy -p schedule-reference --all-features -- -D warnings
rustfmt --edition 2024 crates/schedule-reference/src/main.rs
```

- [ ] **Step 3: Commit**

```bash
git add crates/schedule-reference/src/main.rs
git commit -m "schedule-reference: use common::ingest::post_json for post_schedule_line_population"
```

---

# Final Verification

Run once every task above (Groups A–F) is complete and committed. This is
the plan's own "does the whole tree still work, end to end" gate — separate
from each task's own narrower `-p <crate>` checks.

- [ ] **Step 1: Full workspace build, test, clippy, fmt-check**

```bash
cargo build --workspace 2>&1 | tee /tmp/dedup-final-build.log
cargo test --workspace 2>&1 | tee /tmp/dedup-final-test.log
cargo clippy --workspace --all-features --all-targets -- -D warnings 2>&1 | tee /tmp/dedup-final-clippy.log
cargo fmt --all -- --check 2>&1 | tee /tmp/dedup-final-fmt.log
```

  Diff `/tmp/dedup-final-test.log`'s pass/fail counts against
  `/tmp/dedup-baseline-test.log` (Task A1) per crate — every crate should
  show the same or a higher passing-test count, never fewer, never a new
  failure. If `/tmp/dedup-baseline-fmt.log` (Task A1) already showed
  pre-existing drift unrelated to this plan, confirm this run's own
  `--check` output shows exactly that same pre-existing drift and nothing
  new (this plan's own scoped `rustfmt` calls should not have introduced
  any *new* drift elsewhere, per the fmt-scoping Global Constraint).

- [ ] **Step 2: `helm lint` — confirm the chart layer is untouched and
  still references nothing this plan moved**

```bash
helm lint charts/distant-signal
grep -rn "internal_oauth\|kafka_consumer_group\|metrics_port\|LineCatalogue\|ActiveFeed\|ConnectionState" charts/distant-signal/
```

  The `grep` is expected to return **zero** hits — this chart only ever
  referenced env var *names* (`INTERNAL_OAUTH_*`, `KAFKA_CONSUMER_GROUP`,
  `METRICS_PORT`), never Rust type/field identifiers, so nothing in it
  could reference anything this plan renamed internally. `helm lint` itself
  must pass identically to Task A1's baseline (this plan makes zero chart
  changes).

- [ ] **Step 3: `docker-compose.yml` env var cross-check — confirm every
  name Group B's tasks verified individually is still present, in one
  pass**

```bash
grep -n "INTERNAL_OAUTH_TOKEN_URL\|INTERNAL_OAUTH_CLIENT_ID\|INTERNAL_OAUTH_SCOPE\|INTERNAL_OAUTH_USERNAME\|INTERNAL_OAUTH_PASSWORD\|KAFKA_BROKERS\|KAFKA_TOPIC\|KAFKA_CONSUMER_GROUP\|KAFKA_SASL_USERNAME\|KAFKA_SASL_PASSWORD\|KAFKA_SASL_MECHANISM\|METRICS_ENABLED\|METRICS_PORT\|LINES_DIR" docker-compose.yml | wc -l
```

  Confirm this count matches (or exceeds, if unrelated services were added
  to `docker-compose.yml` independently of this plan) the count from a
  `git stash`-free `git diff main -- docker-compose.yml` showing **zero**
  lines changed in this file across this entire plan's commits — this file
  should never appear in `git log --stat` for any commit this plan made.

- [ ] **Step 4: Per-binary Docker build reproduction (manual, real
  environment required)** — this plan's own research sandbox has no
  `docker` binary available, so this step could not be executed while
  writing this plan; whoever executes this plan should run it for real,
  in an environment with Docker installed, at least once:

```bash
docker build -f docker/trust-consumer.Dockerfile -t distant-signal-trust-consumer-dedup-check .
```

  This is the crate most affected by this plan (Groups B/D/E all touch
  it) and the one whose per-binary build the spec's own Decision 1
  reasoning most depends on (`common`/`health-http` must not pull `axum`
  into any of the 5 poller images — `trust-consumer` is not a poller, but
  confirming its own per-binary build succeeds and produces a working
  binary is the closest real check this plan can specify without Docker
  access during planning). If time allows, also run:

```bash
docker build -f docker/poller-incidents.Dockerfile -t distant-signal-poller-incidents-dedup-check .
```

  and confirm (via `docker history` or by inspecting the build log's own
  dependency-compilation lines) that `axum` does not appear in this image's
  compiled dependency graph — the concrete, empirical version of the
  spec's Decision 1 claim ("a per-binary build never pulls `axum` into any
  poller, feature flag or not"), which this plan's own research could
  reason about but not verify by actually running a Docker build.

- [ ] **Step 5: `--help` sweep — every flattened crate, one more time,
  together** (belt-and-suspenders beyond each task's own individual check)

```bash
for crate in poller-incidents poller-stations poller-tocs poller-ldbws poller-tfl schedule-ingest schedule-reference trust-consumer full-coverage-consumer movement-relay aggregator api; do
  echo "=== $crate ==="
  cargo run -p "$crate" -- --help 2>&1 | grep -E "internal-oauth|metrics-enabled|metrics-port|kafka-brokers|kafka-topic|kafka-consumer-group|kafka-sasl|lines-dir"
done
```

  Compare this output against a `git stash`-preserved run of the same loop
  against `main` (or against Task A1's own recorded baseline, if `--help`
  output was captured there) — every flag name must appear in both, with
  the same default values shown.

No commit for this section — it is verification-only, run against the
finished tree after every task's own commit already landed.
