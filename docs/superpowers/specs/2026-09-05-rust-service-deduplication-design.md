# Rust Service Deduplication — Design Spec

**Status: design spec, not an approved plan.** No implementation, no code
in this pass.

## What was asked for

A review agent audited all 18 crates in this workspace for code
duplication and returned an 11-item findings list, ranked into three
tiers. The repo owner approved a spec→plan→implement chain for the six
"worth fixing" items (#1–#6) and explicitly rejected the five "don't fix"
items (#7–#11, Tier 3) — those are out of scope here and are not
re-litigated below.

This document re-verifies the review's load-bearing claims against the
current repo state (some drift is expected — the review ran some time
ago), then makes the concrete architectural calls a plan needs before
Tasks can be written: where shared code lives, exactly what moves where,
what per-crate differences must survive the move, and what order to do it
in. Per this repo's citation discipline, every claim below is cited to
`file:line` as re-checked in this pass, not copied from the review
findings uncritically.

---

## 0. Verification summary

All Tier 1/2 claims were re-read against the current tree. Line numbers
have drifted slightly from the review's citations in most files (typically
by 0–5 lines — comments/const additions since the review ran), but every
byte-identical/near-identical claim holds. Two things the review stated
loosely turned out to matter a lot for this spec's Decision 1 and Decision
5 below, and are flagged where relevant:

- The review's "either feature-gate axum or make a new crate" framing
  undersold how different `#1`/`#2` (clap + tokio, needed by every
  consumer already) are from `#3` (axum, needed by only 3 of 14) — see
  Decision 1.
- Two fields inside the review's "byte-identical" `#2` config block —
  `metrics_port` and `kafka_consumer_group` — are **not** actually
  identical across crates: they carry genuinely different default values,
  one of them (`kafka_consumer_group` on `movement-relay`) by explicit,
  documented design. See §3.2.

---

## 1. Decision: no new "service-common" crate. Extend `common` for #1/#2/#4; one new narrow crate for #3 (and #5's new home)

**The call: `common` gains `tokio` (promoted from dev- to a real
dependency, `time` feature only) and `clap` (`derive`, `env` features) as
ordinary dependencies, and hosts the code for #1, #2, and #4 directly. A
new crate — this document calls it `service-health`, name TBD at
implementation time — is added to the workspace for #3's shared
health/readiness module, and also becomes the new home for #5's
`ActiveFeed`/`MovementFeed` machinery once #3 lands (see §3.5). `common`
does not gain `axum`, not even behind a feature flag.**

### Census: who actually depends on `common` today

```
grep -rl '^common' crates/*/Cargo.toml
```

14 of the 18 crates: `api`, `aggregator`, `schedule-ingest`, `enricher`,
`poller-tocs`, `full-coverage-consumer`, `schedule-reference`,
`movement-relay`, `notifier`, `poller-incidents`, `poller-ldbws`,
`poller-stations`, `poller-tfl`, `trust-consumer`. The 4 that don't:
`common` itself, `movement-feed`, `schedule-query`, `trust-schema`.

Checked each non-dependent's own `Cargo.toml` for whether it would object
to gaining `clap`/`tokio`/`axum` transitively — moot, because none of them
would gain anything (they don't depend on `common`), but worth confirming
they're deliberately lean, not accidentally so:

- `crates/schedule-query/Cargo.toml:6-7` — a comment states this
  explicitly: *"Deliberately lib-only: no `main.rs`/`[[bin]]`, no
  dependency on `tokio`, `reqwest`, `sqlx`, or `rdkafka`."*
- `crates/trust-schema/Cargo.toml` — `anyhow`, `serde`, `serde_json`,
  `sha2`, `tracing` only.
- `crates/movement-feed/Cargo.toml:6-9` — `anyhow`, `async-trait`,
  `redis`, `tracing`; `tokio` appears only as a `[dev-dependencies]` /
  `test-util`-feature dependency (`crates/movement-feed/Cargo.toml:11-24`),
  not a real one.

**Every one of the 14 real `common` dependents already declares `clap`
(`derive`, `env`) and `tokio` directly in its own `Cargo.toml`** (spot
checked: `crates/api/Cargo.toml`, `crates/aggregator/Cargo.toml`,
`crates/enricher/Cargo.toml`, `crates/notifier/Cargo.toml`,
`crates/movement-relay/Cargo.toml`, and by construction the 5 pollers +
`schedule-ingest`/`schedule-reference`/`trust-consumer`/
`full-coverage-consumer`, all of which use `#[derive(Parser)]` in their
own `config.rs` and `#[tokio::main]` in their own `main.rs`). Promoting
`common`'s existing `tokio` dev-dependency to a real one (adding the
`time` feature it needs for `tokio::time::interval_at`/`Instant`) and
adding `clap` as a real dependency costs **zero** consumers anything they
don't already pay: no crate in this workspace that depends on `common`
today lacks either.

**`axum` is different.** Only 3 of the 14 (`trust-consumer`,
`full-coverage-consumer`, `movement-relay`) need it. And `common` has an
explicit, documented anti-axum stance today —
`crates/common/src/metrics.rs:1-9` states this module serves "the six
binaries that have no HTTP server of their own" and its own doc for
`install` (`crates/common/src/metrics.rs:51-56`) spells out *why*: "No
`axum` dependency: `metrics-exporter-prometheus`'s `with_http_listener`
spins up its own minimal `hyper`-based listener, so this doesn't pull a
web framework into six crates that have never needed one." That's a
deliberate design decision already in the codebase, not an accident to
route around.

Two facts make even a *feature-gated* `axum` in `common` a real question,
not a slam dunk, worth resolving explicitly rather than deferring:

1. **Docker builds are per-binary**, not per-workspace —
   `docker/poller-incidents.Dockerfile:52-56` runs
   `cargo build --release --bin poller-incidents`, not
   `--workspace`. Feature resolution for a `-p`/`--bin`-scoped build only
   pulls in features actually requested by that binary's own dependency
   closure, so a `common/service-health` feature enabled only by
   `trust-consumer`/`full-coverage-consumer`/`movement-relay`'s own
   `Cargo.toml` would never compile `axum` into any of the 5 pollers',
   `aggregator`'s, `enricher`'s, or `notifier`'s production image, feature
   flag or not.
2. **But CI does build/lint the whole workspace at once**:
   `.github/workflows/ci.yml:216-217`/`:219-220` run
   `cargo build --workspace` / `cargo test --workspace`, and the `clippy`
   job passes `--all-features` (`.github/workflows/ci.yml:98`). A
   feature-gated `axum` dependency on `common` would still get compiled in
   that single shared build graph — though this is moot either way,
   because `trust-consumer`, `full-coverage-consumer`, and
   `movement-relay` already declare `axum = "0.8.9"` directly today
   (`crates/trust-consumer/Cargo.toml`,
   `crates/full-coverage-consumer/Cargo.toml`,
   `crates/movement-relay/Cargo.toml`), so `axum` is already part of that
   same CI build graph regardless of what `common` does.

So the *technical* cost of a feature-gated `axum`-in-`common` is close to
zero either way. The decision not to do it is about legibility and
precedent, not compile time: `common`'s own module docs make a point of
being the one place a web framework was kept out of on purpose, and this
workspace already has a working precedent for narrow, single-purpose
shared crates instead of one grab-bag (`movement-feed`, `trust-schema`,
`schedule-query` are all small and scoped to one concern). A new
`service-health` crate:

- Keeps `common`'s own stated architecture intact with no caveats needed
  ("why does `common` list `axum` as optional?" is a question nobody has
  to answer).
- Is the natural new home for #5's `ActiveFeed` too (§3.5) — `ActiveFeed`
  is tied to its crate today *only* through `health::ConnectionState`
  (`crates/trust-consumer/src/main.rs:51`,
  `crates/full-coverage-consumer/src/main.rs:65`); once `ConnectionState`
  lives in a shared crate, `ActiveFeed` can live right next to it rather
  than needing a second new crate of its own.
- Costs exactly one new workspace member, not a broad "service-common"
  that would duplicate a chunk of what `common` already does.

**Net effect: one new crate added to `[workspace] members` in the root
`Cargo.toml`, not a "service-common" absorbing #1/#2/#3/#4 together.**

---

## 2. Scope table

| Finding | Moves to | New/changed dependency | Consumers |
|---|---|---|---|
| #1 poller loop | `common::poller_loop` (new module) | `common` gains real `tokio` (`time`) | poller-incidents, poller-stations, poller-tocs, poller-ldbws, poller-tfl |
| #2a `InternalOAuthArgs` + `token_cache()` | `common` (new module, see §7 Open Questions) | `common` gains real `clap` | 9: the 5 pollers + schedule-ingest, schedule-reference, trust-consumer, full-coverage-consumer |
| #2b `MetricsArgs` (metrics_enabled only) | same new `common` module | (same `clap` addition) | same 9 |
| #2c `KafkaConnectionArgs` (5 fields, no `kafka_consumer_group`) | same new `common` module | (same `clap` addition) | trust-consumer, full-coverage-consumer, movement-relay |
| #3 `ConnectionState`/`spawn`/`spawn_with_state`/`set_connected` | new `service-health` crate | new workspace member; depends on `common` (for `metric_name`) | trust-consumer, full-coverage-consumer, movement-relay |
| #4 `LineCatalogue` + `parse_lines` | `common` | none (only needs `anyhow`+`std::path`, both already present) | aggregator, api, full-coverage-consumer, schedule-reference |
| #5 `ActiveFeed`/`MovementFeed` impl/`check_gap`/`MovementFeedBackend` | `movement-feed` crate | `movement-feed` gains real `async-trait` (already has it) + a dependency on `service-health` | trust-consumer, full-coverage-consumer |
| #6 `get_json<T>`/`post_json<T>` | `common::ingest` | none (module already has `reqwest`/`serde`) | poller-ldbws, trust-consumer, full-coverage-consumer, schedule-ingest, schedule-reference |

---

## 3. Per-item detail: what's real, what moves, what must be preserved

### 3.1 #1 — poller `main()` loop scaffolding

Re-verified against the current tree: `poller-incidents/src/main.rs:28-87`
and `poller-tocs/src/main.rs:28-87` (the `main` function) differ in
exactly two label strings (`"poller" => "incidents"` at line 73/78 vs.
`"poller" => "tocs"`). `poller-stations/src/main.rs:28-87` and the
equivalent block in `poller-ldbws/src/main.rs:57-116` and
`poller-tfl/src/main.rs:75-141` are the same shape (line numbers shift a
little in the latter two because of extra module-level doc comments and
constants above `main`, not because the block itself differs). This
confirms the review's core claim; the exact line ranges it cited (28-90)
are off by ~3 lines from current `main()`, which ends at line 87 in
`poller-incidents`.

**Genuine differences, confirmed to live outside the extracted block:**

- **`poller-tfl`'s pre-flight check** (`require_non_empty_key`,
  `crates/poller-tfl/src/main.rs:89`) runs *before* `Config::parse()`'s
  result is used for anything else — specifically before the
  `metrics_enabled` check at `crates/poller-tfl/src/main.rs:93` — i.e.
  strictly before the scaffolding block even starts, not interleaved with
  it. It only needs `config.tfl_app_key`, which a shared loop helper never
  touches.
- **`poller-tfl`'s `dlr_state`** (`crates/poller-tfl/src/main.rs:121`,
  `let mut dlr_state = dlr::inference::DlrMatchState::new();`) is
  long-lived, mutable, cycle-to-cycle state that `poll_once` needs
  (`crates/poller-tfl/src/main.rs:124`,
  `poll_once(&client, &config, &mut dlr_state, &internal_oauth)`). This is
  the one genuine signature difference among the 5 pollers' `poll_once`
  calls.
- **`poller-ldbws`'s extra fetch** (`fetch_sample_stations`,
  `crates/poller-ldbws/src/main.rs:160-174`) is called from inside its own
  `poll_once` (`crates/poller-ldbws/src/main.rs:123`), not from `main`'s
  loop body — confirmed it never touches the scaffolding being extracted.

**Design for `common::poller_loop`:** a function (name TBD, e.g.
`run_poll_loop`) taking the poller's label, an already-built
`reqwest::Client`, the `api_ingest_url`, an already-constructed
`OAuthTokenCache` (built via §3.2's `token_cache()`), the poll interval,
`metrics_enabled`/`metrics_port` as plain values (not a struct — see
§3.2 for why), and a repeatedly-callable async cycle function. Internally
it does exactly what the 5 `main()`s do today: install metrics if
enabled, call `ingest::time_until_next_poll`, log the "still fresh" delay,
build the `interval_at`, then loop forever recording
`poller_cycle_duration_seconds`/`poller_cycle_total` under the given label
and logging cycle errors. `poll_once` and any pre-flight check stay in
each poller's own `main.rs`, called as today; only the wrapper is shared.
`poller-tfl`'s pre-flight check runs before the helper is called (it needs
nothing the helper constructs). `poller-tfl`'s `dlr_state` is handled by
having its own `main` own the `DlrMatchState` and capture it by mutable
reference in the cycle closure passed to the helper — no change to
`poll_once`'s own signature needed. Every metric label value currently
emitted (`"incidents"`, `"stations"`, `"tocs"`, `"ldbws"`, `"tfl"`) is
passed straight through unchanged.

This helper is scoped to exactly these 5 crates. `aggregator`, `enricher`,
and `notifier` were checked
(`crates/aggregator/src/main.rs:1-60`,
`crates/enricher/src/main.rs:1-60`, `crates/notifier/src/main.rs:1-40`)
and do **not** share this shape — no `internal_oauth_*`, no
`ingest::time_until_next_poll`, no `poller_cycle_*` metrics; they're
DB-backed cycles with their own bespoke loops (`aggregator` and
`full-coverage-consumer`/`trust-consumer` even have their own *different*,
multi-cadence-in-one-loop shape, documented at
`crates/full-coverage-consumer/src/main.rs:10-16` as deliberately mirroring
`trust-consumer`'s own). Folding those in would be inventing a new
dedup target beyond what was reviewed — explicitly out of scope here.

### 3.2 #2 — `InternalOAuthArgs` / `MetricsArgs` / `KafkaConnectionArgs`

**The OAuth block is fully clean to flatten.** Re-checked
`internal_oauth_token_url`/`internal_oauth_client_id`/
`internal_oauth_scope`/`internal_oauth_username`/`internal_oauth_password`
across `poller-incidents/src/config.rs:31-44`,
`full-coverage-consumer/src/config.rs:90-99`,
`trust-consumer/src/config.rs:92-104`,
`schedule-reference/src/config.rs:89-101`,
`schedule-ingest/src/config.rs:104-116` (plus the other 3 pollers): field
names, types, doc-comment wording, defaultedness, and the one actual
default value (`internal_oauth_scope`'s `"groups"`) are identical
everywhere — zero discrepancy to design around. The paired
`OAuthTokenCache::new(OAuthCredentials{...})` construction
(`crates/poller-incidents/src/main.rs:41-48`, and 8 more byte-identical
copies) becomes a single `InternalOAuthArgs::token_cache(&self) ->
OAuthTokenCache` method, called as `config.internal_oauth.token_cache()`.

**`MetricsArgs` flattens `metrics_enabled` only — not `metrics_port`.**
`metrics_enabled`'s default is `true` everywhere, re-confirmed in all 9
crates (`poller-incidents/src/config.rs:67`,
`poller-ldbws/src/config.rs:99`, `poller-tfl/src/config.rs:99`,
`schedule-ingest/src/config.rs:134`, `schedule-reference/src/config.rs:111`,
`trust-consumer/src/config.rs:205`,
`full-coverage-consumer/src/config.rs:128`) — safe to unify. But
`metrics_port`'s *default value* genuinely differs per crate:
`poller-incidents`/`poller-stations`/`poller-tocs`/`poller-ldbws`/
`poller-tfl`/`schedule-ingest` all default to `9091`
(`poller-incidents/src/config.rs:55`, `poller-ldbws/src/config.rs:87`),
`schedule-reference/src/config.rs:107` defaults to `9092`,
`full-coverage-consumer/src/config.rs:126` defaults to `9093`,
`trust-consumer/src/config.rs:202` defaults to `9095`. `clap`'s
`#[command(flatten)]` splices in a sub-struct's fields with whatever
`default_value_t` that sub-struct declares — there's no per-embedding-site
override without generics or duplicate structs, neither of which is worth
it here. Checked whether these defaults are actually load-bearing:
`docker-compose.yml` never sets `METRICS_PORT` at all for any of these
services (`grep -n METRICS_PORT docker-compose.yml` — no hits), so local
dev relies entirely on the code-level default; the Helm chart, by
contrast, always renders `METRICS_PORT` explicitly
(`charts/distant-signal/templates/poller-deployments.yaml:83-85`,
values-driven, so it never reads the code default in Kubernetes). Since a
silently-changed default *would* be an observable (if likely harmless)
behavior change in `docker-compose` and this document's own mandate is to
avoid unflagged deployment-affecting changes, **`metrics_port` stays a
plain per-crate field, not part of the flattened struct.** This has no
effect on `run_poll_loop`'s design (§3.1) — it takes `metrics_enabled`/
`metrics_port` as two plain values regardless of which are flattened.

**`KafkaConnectionArgs` flattens 5 of 6 Kafka fields — not
`kafka_consumer_group`.** `kafka_brokers`/`kafka_topic`/
`kafka_sasl_username`/`kafka_sasl_password`/`kafka_sasl_mechanism` are all
declared `#[arg(long, env)]` with **no default** (required) in all three
crates — `trust-consumer/src/config.rs:47-77`,
`full-coverage-consumer/src/config.rs:50-61`,
`movement-relay/src/config.rs:12-27` — so flattening them changes nothing
about defaultedness or requiredness. `kafka_consumer_group` does not fit:
`trust-consumer` defaults it to `"distant-signal-trust-consumer"`
(`trust-consumer/src/config.rs:60`), `full-coverage-consumer` to
`"distant-signal-full-coverage-consumer"`
(`full-coverage-consumer/src/config.rs:54`), and `movement-relay` gives it
**no default at all**, by explicit design —
`movement-relay/src/config.rs:16-21`'s own comment: *"Deliberately no
default: unlike trust-consumer's own `kafka_consumer_group` (which DOES
have a sensible per-deployment default...), this crate's group id is a
fixed, externally-issued, unforgeable identity — guessing wrong here is
worse than refusing to start."* This is the repo's own documented reason
this field must not be unified; it stays a per-crate field.

**CLI flags and env vars are unaffected.** `clap`'s `#[command(flatten)]`
does not prefix or rename an embedded struct's fields — a field named
`internal_oauth_token_url` inside a flattened `InternalOAuthArgs` still
produces `--internal-oauth-token-url` / `INTERNAL_OAUTH_TOKEN_URL`, byte
for byte, as long as the shared struct's field names match today's Config
field names exactly (which this design requires). Cross-checked against
current deployment wiring: `docker-compose.yml:175-179` sets
`INTERNAL_OAUTH_TOKEN_URL`/`INTERNAL_OAUTH_CLIENT_ID`/
`INTERNAL_OAUTH_SCOPE`/`INTERNAL_OAUTH_USERNAME`/`INTERNAL_OAUTH_PASSWORD`
and `docker-compose.yml:382-397` sets `KAFKA_BROKERS`/`KAFKA_TOPIC`/
`KAFKA_CONSUMER_GROUP`/`KAFKA_SASL_USERNAME`/`KAFKA_SASL_PASSWORD`/
`KAFKA_SASL_MECHANISM`; `charts/distant-signal/templates/poller-deployments.yaml:81-108`
renders `METRICS_ENABLED`/`METRICS_PORT`/`INTERNAL_OAUTH_TOKEN_URL`/
`INTERNAL_OAUTH_CLIENT_ID`/`INTERNAL_OAUTH_SCOPE`/`INTERNAL_OAUTH_USERNAME`/
`INTERNAL_OAUTH_PASSWORD` under those exact names. **None of these names
change. No CLI flag or env var is renamed anywhere by this work**, and
(per the two exclusions above) no default value changes either.

### 3.3 #3 — shared health/readiness module

Re-confirmed near-verbatim: `spawn`
(`trust-consumer/src/health.rs:20-42` / `full-coverage-consumer/src/health.rs:25-47`)
and `healthz`
(`trust-consumer/src/health.rs:44-50` / `full-coverage-consumer/src/health.rs:49-55`)
are character-for-character identical; `full-coverage-consumer/src/health.rs:8`
literally says "Verbatim copy of `crates/trust-consumer/src/health.rs`".
Both declare `pub type ConnectionState = Arc<AtomicBool>;`
(`trust-consumer/src/health.rs:18`,
`full-coverage-consumer/src/health.rs:23`) — the same type alias, not
distinct types, confirming the review's point that the stated
justification for keeping two copies doesn't hold. `set_connected`
differs only in the gauge-name string literal
(`"trust_consumer_ready"` vs. `"full_coverage_consumer_ready"`,
`trust-consumer/src/health.rs:62` /
`full-coverage-consumer/src/health.rs:67`).

`movement-relay/src/health.rs:1-12`'s own module doc is explicit and
should be treated as load-bearing, not just documentation: *"readiness
for `movement-relay` means 'confirmed Kafka partition assignment,' NOT
'the HTTP server answered' and NOT 'at least one message has arrived'...
Do not 'fix' this inconsistency by making the two match — it is
deliberate."* Re-read `movement-relay/src/health.rs:60-74`'s `spawn` and
`:76-85`'s `healthz`: the `bind`/`serve`/error-handling body is the
*same* 15-line shape as `trust-consumer`'s, but (a) it takes a
pre-existing `ReadyState` rather than creating and returning one, because
the state is created earlier and owned by `RelayContext`
(`movement-relay/src/health.rs:22-30`), and (b) its `healthz` response
text differs ("partitions assigned" / "no confirmed partition assignment"
vs. "connected" / "disconnected").

**How `RelayContext`'s rebalance-driven flag-setting plugs into a shared
`set_connected`:** today, `RelayContext::post_rebalance`
(`movement-relay/src/health.rs:35-57`) inlines the atomic store *and* the
gauge update at all three branches (`Assign`, `Revoke`, `Error`) —
exactly the two operations `trust-consumer`/`full-coverage-consumer`'s
`set_connected` already bundles together
(`trust-consumer/src/health.rs:60-67`). The Kafka-specific parts —
`impl ClientContext`, `impl ConsumerContext`, matching on `Rebalance`
variants, and the `tracing::info!`/`warn!`/`error!` calls — are not
shareable and stay in `movement-relay`. Only the state-mutation body
(`self.ready.store(...)` + `metrics::gauge!(...).set(...)`) is replaced
by a call to the shared `set_connected(&self.ready, "movement_relay_ready",
true)` (and `false` on the other two branches). This is a clean,
minimal-touch integration: `RelayContext` keeps its own struct and trait
impls; only its three call sites change from two inlined statements to
one shared function call.

**Design for the new crate:**

- `pub type ConnectionState = Arc<AtomicBool>;` (name kept — every
  existing caller of `crates::health::ConnectionState` imports it by this
  name today).
- `pub fn spawn(bind_url: String) -> ConnectionState` — creates a fresh
  `AtomicBool`, matches `trust-consumer`/`full-coverage-consumer`'s
  current call shape exactly.
- `pub fn spawn_with_state(bind_url: String, state: ConnectionState)` —
  takes an already-constructed state and starts the same server task,
  matches `movement-relay`'s current call shape
  (`movement_relay::health::spawn(bind_url, ready)`) exactly.
- `pub fn set_connected(state: &ConnectionState, gauge_name: &str,
  connected: bool)` — the gauge name (`"trust_consumer_ready"`,
  `"full_coverage_consumer_ready"`, `"movement_relay_ready"`) is now a
  parameter instead of three copy-pasted hardcoded strings, but every
  caller passes the exact same string it emits today, so the actual
  metric names on the wire are unchanged.
- The `healthz` response body text: **recommend parameterizing this too**
  (two `&'static str` values, one per status) rather than picking one
  wording and applying it everywhere. `trust-consumer`/
  `full-coverage-consumer` keep "connected"/"disconnected";
  `movement-relay` keeps "partitions assigned"/"no confirmed partition
  assignment". This is a small, deliberate choice to make this a pure
  refactor with zero observable behavior change anywhere, including in
  the literal HTTP response body a liveness probe or a human `curl` would
  see — flagged explicitly here per this document's mandate to call out
  anything that could silently change deployed behavior, even something
  this minor.

### 3.4 #4 — `LineCatalogue` + `parse_lines`

Re-confirmed identical across all four sites:
`aggregator/src/config.rs:7-32`, `api/src/data/config.rs:16-42`,
`full-coverage-consumer/src/config.rs:7-24`,
`schedule-reference/src/config.rs:7-24`. Same newtype, same `Deref`, same
`parse_lines` free function, same ~12-line doc comment explaining the
`clap_derive` `Vec<T>` panic workaround. Confirmed these all wrap the
*same* type, not four independently-defined lookalikes: `api`'s copy
imports `crate::data::LineDefinition`
(`api/src/data/config.rs:7`), and `api/src/data/mod.rs:15` shows this is
itself `pub use common::{LineDefinition, Station};` — a re-export, not a
distinct type. All four `LineCatalogue`s are `Vec<common::LineDefinition>`
under the hood.

This needs no `clap`/`tokio`/`axum` — `parse_lines` and the newtype only
touch `anyhow`, `std::path::PathBuf`, and `common::LineDefinition`, all
already present in `common` today. **This is fully independent of
Decision 1's shared-crate question** and can move into `common` on its
own schedule, in parallel with everything else. Each of the 4 crates'
`config.rs` drops its own copy and does `use
common::config::LineCatalogue;` (or wherever it lands — see §7); `api`'s
one wrinkle is its doc comment referencing `crate::data::LineDefinition`
by its local re-exported name, which needs updating to reference
`common::LineDefinition` directly, a one-line, no-behavior-change edit.

### 3.5 #5 — `ActiveFeed` / `MovementFeed` impl / `check_gap` / `MovementFeedBackend`

Re-confirmed structurally identical:
`trust-consumer/src/main.rs:34-89` and
`full-coverage-consumer/src/main.rs:58-100` (line numbers drifted
slightly from the review's citation, content unchanged) — the `ActiveFeed`
enum, its `MovementFeed` trait impl, and its `check_gap` inherent method
differ only in doc-comment wording. `MovementFeedBackend`
(`trust-consumer/src/config.rs:17-32` /
`full-coverage-consumer/src/config.rs:31-35`, the latter's own comment
says "Verbatim copy") is the same two-variant `Kafka`/`RedisStream` enum
in both.

Confirmed the review's claim that the only thing tying `ActiveFeed` to its
crate is the health type: `RedisStream(Box<RedisStreamMovementFeed>,
health::ConnectionState)` appears in both
(`trust-consumer/src/main.rs:51`,
`full-coverage-consumer/src/main.rs:65`) — every other type `ActiveFeed`
touches (`KafkaMovementFeed`, `RedisStreamMovementFeed`, `GapInfo`) is
already imported from the shared `movement-feed` crate today
(`use movement_feed::redis_stream::{GapInfo, RedisStreamMovementFeed};` in
both files). Once `health::ConnectionState` is `service_health::
ConnectionState` (§3.3) — a type both crates will import from the same
shared, externally-addressable crate rather than each having its own
crate-local `health` module — `ActiveFeed` has zero remaining
crate-specific dependencies and can move into `movement-feed` wholesale,
parameterized by whatever's genuinely per-caller (there is none found
beyond construction-site values already passed in from each `Config`).

**This is why #5 is sequenced strictly after #3, not just "should be
done after" as a style preference**: `movement-feed` moving `ActiveFeed`
in requires a concrete, externally-shared `ConnectionState` type to exist
first. Doing #5 before #3 would mean `movement-feed` depending on
whichever crate's `health` module got picked arbitrarily (or duplicating
the type yet again), which is worse than the status quo.

`movement-feed` gains one new dependency: `service-health` (for
`ConnectionState`/`set_connected`). It does not need `axum` itself for
this — only the type alias and the plain function, not the HTTP server —
though `axum` will be part of `movement-feed`'s compiled dependency graph
transitively either way, which is immaterial since its only two
current consumers (`trust-consumer`, `full-coverage-consumer`) already
depend on `axum` directly today.

### 3.6 #6 — `common::ingest::get_json<T>` / `post_json<T>`

Re-confirmed both gaps. **(a) GET + bearer + deserialize**, repeated with
no shared logic beyond what's already trivial:
`poller-ldbws/src/main.rs:160-174` (`fetch_sample_stations`),
`trust-consumer/src/queries.rs:12-24`
(`fetch_active_tracked_trains`) and `:26-37`
(`fetch_stanox_crs`), `full-coverage-consumer/src/queries.rs:24-37`
(`fetch_stanox_crs`, whose own comment at lines 25-27 says "Identical to
`trust_consumer::queries::fetch_stanox_crs`"). `common::ingest` itself
already has this exact shape once, privately:
`fetch_last_fetched` (`crates/common/src/ingest.rs:104-118`) is GET +
bearer + `error_for_status` + `.json().await?`, just returning a specific
field afterward. A `get_json<T: DeserializeOwned>` public function fits
right in next to it, and `fetch_last_fetched` can be rewritten in terms of
it as a bonus (not required, since `fetch_last_fetched` isn't itself one
of the review's cited duplicates — flagged as a nice-to-have, not part of
this item's required scope).

**(b) Single-object POST**, confirmed as genuinely distinct from
`post_batch`'s array-wrapping shape, and confirmed both existing call
sites already say so in their own comments:
`schedule-ingest/src/main.rs:342-364`'s `post_ingest` ("Deliberately
**not** `common::ingest::post_batch`: that helper always serializes
`items: &[T]` as a JSON array... Wrapping a single record in a
one-element slice would change the wire shape rather than match it") and
`schedule-reference/src/main.rs:370-389`'s
`post_schedule_line_population` ("A single-object POST (not a batch
array) -- `common::ingest::post_batch` serializes a slice as a JSON
array, which doesn't fit this route's body shape"). Both are otherwise
identical bearer-token-fetch/POST/status-check/error-text bodies.

**Design:** add `pub async fn get_json<T: DeserializeOwned>(client, url,
tokens) -> anyhow::Result<T>` and `pub async fn post_json<T: Serialize>
(client, url, tokens, body: &T) -> anyhow::Result<()>` to
`crates/common/src/ingest.rs`, next to `post_batch`. Per the review's own
suggestion, `post_batch`'s body becomes a thin call to `post_json` with
`items` (the slice itself, which already serializes as a JSON array via
`serde`) as the body — no change to `post_batch`'s public signature or
behavior, callers are unaffected. `get_json`/`post_json` are strictly
additive to a module whose own doc
(`crates/common/src/ingest.rs:1-16`) already frames itself as "single
source of truth for the POST-batch-and-log pattern every real caller
repeats" — this is the same pattern extended to two shapes it doesn't
cover yet, not a new architectural concern.

---

## 4. Migration mechanics

### Cargo.toml changes

- **Root `Cargo.toml`**: add `"crates/service-health"` to `[workspace]
  members`.
- **`crates/common/Cargo.toml`**: promote `tokio` from
  `[dev-dependencies]` to `[dependencies]` (features: whatever it has
  today, plus `time`); add `clap = { version = "4.6.x", features =
  ["derive", "env"] }` (pin matching the rest of the workspace, currently
  `4.6.1`–`4.6.6` across crates — worth aligning to one version as part of
  this work, flagged in §7).
- **New `crates/service-health/Cargo.toml`**: `axum` (`0.8.9`, matching
  every current axum user in this workspace), `tokio` (`rt-multi-thread`,
  `macros`, `net`), `common` (path dep, for `metric_name`), `metrics`,
  `tracing`.
- **`crates/movement-feed/Cargo.toml`**: add `service-health` (path dep).
- **The 5 poller crates + schedule-ingest + schedule-reference +
  trust-consumer + full-coverage-consumer**: no *new* dependencies needed
  — they already depend on `common`, and everything in §2 either lives in
  `common` (already a dependency) or (for trust-consumer/
  full-coverage-consumer) in `service-health`, which they gain a
  dependency on for #3 regardless.
- **`movement-relay/Cargo.toml`**: add `service-health` (path dep),
  replacing its own `crates/movement-relay/src/health.rs` module.

### CLI flags / env vars / default values

**No CLI flag or environment variable is renamed anywhere by this work.**
`#[command(flatten)]` preserves the embedded struct's own field-derived
`--long-flag` / `ENV_VAR` names exactly, and this design's shared structs
use field names identical to today's `Config` fields. Verified against
current wiring in `docker-compose.yml` and
`charts/distant-signal/templates/poller-deployments.yaml` (§3.2) — every
name checked there is unchanged.

**No default value changes either**, by design: the two fields whose
default genuinely differs per crate (`metrics_port`, `kafka_consumer_group`)
are deliberately excluded from the shared structs (§3.2) rather than
forced to a single value. If a future pass wants to unify those too,
that's a new decision with its own tradeoffs (e.g., whether
`kafka_consumer_group`'s per-deployment default is worth keeping at all
given `movement-relay`'s explicit no-default stance) — not something this
document's "pure refactor" framing should smuggle in.

**`service-health`'s `healthz` response body text** is the one place a
literal wire-visible string is being reorganized (from three
crate-local, hardcoded pairs to one shared function's parameters) — §3.3
recommends parameterizing rather than unifying specifically so the actual
text stays byte-for-byte the same per crate.

---

## 5. Sequencing

**One plan, ordered task groups — not multiple separate plans.** Every
group here is a normal, in-repo, independently buildable-and-testable
change (unlike, say, the Grafana plan's Part 2, which needed a human to
edit a different repository) — this matches the "Part 1" style of
`docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md`
(ordered task groups within one plan, each independently mergeable with
its own verification), not that plan's Part 1/Part 2 split, since nothing
here leaves this repository.

Revising the review's suggested order in two places, both explained below:

1. **Group A — foundational `common`/new-crate scaffolding.** Add
   `tokio`(`time`)/`clap` to `common`; add `InternalOAuthArgs`/
   `MetricsArgs`/`KafkaConnectionArgs`/`LineCatalogue` to `common`; scaffold
   the new `service-health` crate (types + functions, not yet wired into
   any consumer). Purely additive — nothing existing changes behavior yet.
2. **Group B — adopt Group A's config types.** Flatten
   `InternalOAuthArgs`/`MetricsArgs` into all 9 crates' `Config` (#2's
   OAuth/metrics half); flatten `KafkaConnectionArgs` into the 3
   Kafka-consuming crates (#2's Kafka half); replace all 4
   `LineCatalogue` copies with `common`'s (#4). **#4 does not need to wait
   for this group** — it has no dependency on Decision 1's shared-crate
   question at all (§3.4) and can land in parallel with Group A, or even
   before it, as its own single-commit change. It's placed in Group B
   here only because it's mechanically similar work, not because of a
   real ordering constraint.
3. **Group C — #1, the poller loop.** Extract `run_poll_loop` into
   `common::poller_loop`; adopt it in the 5 pollers. Sequenced after Group
   B specifically for the 5 pollers (not a hard technical dependency —
   `run_poll_loop` takes plain values/an already-built `OAuthTokenCache`,
   it doesn't require `InternalOAuthArgs` to exist — but both changes
   touch the same `main.rs`/`config.rs` pair in the same 5 files, and
   doing the config flatten first means Group C's diff is "swap the loop
   body for a helper call using the token cache that's already there,"
   not "flatten config *and* extract the loop in the same touch."
4. **Group D — #3, shared health/readiness.** Move
   `trust-consumer/src/health.rs`'s body into `service-health`; adopt in
   `trust-consumer` and `full-coverage-consumer`; then adopt in
   `movement-relay` (the `spawn_with_state` + `set_connected`-inside-
   `RelayContext` shape, §3.3), preserving its distinct readiness
   semantics untouched.
5. **Group E — #5, `ActiveFeed`.** Move `ActiveFeed`/`MovementFeed`
   impl/`check_gap`/`MovementFeedBackend` into `movement-feed`. **Strictly
   depends on Group D**, not just conventionally sequenced after it —
   `ActiveFeed`'s only remaining crate-tie is `health::ConnectionState`
   (§3.5), which must already be `service_health::ConnectionState` (a
   type external to both `trust-consumer` and `full-coverage-consumer`)
   before `movement-feed` can hold `ActiveFeed` without picking one
   crate's `health` module arbitrarily.
6. **Group F — #6, `get_json`/`post_json`.** Fully independent of Groups
   A–E; can run any time, including in parallel with all of them, exactly
   as the review itself noted.

This keeps the review's overall shape (#2/#4 mechanical first, #1 next,
#3 before #5, #6 anytime) but corrects two things: #4 is not actually
blocked on the shared-crate question the way #1/#2/#3 are (it needs no
shared crate at all), and #5's dependency on #3 is a hard technical
requirement (a type must exist somewhere shared before it can be
referenced from a new location), not a stylistic preference.

---

## Non-goals

- **Tier 3 (#7–#11).** Not touched, not re-argued: `feed/kafka.rs`'s
  `KafkaMovementFeed` copies (already scheduled for deletion in Deploy
  C), the 19 hand-written `distant_signal_*_errors_total` counter sites,
  the 4 `PgPool` construction sites, the 3 Redis `XINFO GROUPS` field-walk
  copies, and `api`'s thin ingest-wrapper repetition.
- **Inventing new deduplication targets.** `aggregator`/`enricher`/
  `notifier`'s own bootstrap lines (`dotenv::dotenv().ok()` +
  `tracing_subscriber::fmt()...init()`) are textually similar to the 5
  pollers' but were not part of the review's #1 finding and are not
  folded in here (§3.1).
- **Unifying `metrics_port` or `kafka_consumer_group` across crates.**
  Deliberately kept per-crate (§3.2, §4) — a real, cited reason exists for
  each (docker-compose's reliance on the code default for the former;
  `movement-relay`'s own documented "no default, unforgeable identity"
  stance for the latter).
- **Implementation of any kind.** No code, no `Cargo.toml` edits, no new
  crate scaffold — this is the design pass only.
- **Aligning the workspace's scattered `clap`/`axum`/`tokio` version pins**
  (`clap` alone ranges `4.6.1`–`4.6.6` across crates checked in this pass)
  beyond what's strictly needed for the new shared crate/module to compile
  against all its consumers. Flagged as a §7 open question, not decided
  here.
- **Renaming any CLI flag, env var, or metric name.** Confirmed none of
  this work requires it (§3.2, §4); if a future pass wants to, e.g.,
  unify `metrics_port` defaults, that's a separate, explicitly
  deployment-affecting decision.

---

## Open Questions

1. **Exact module path for the new `common` types.** This document treats
   `InternalOAuthArgs`/`MetricsArgs`/`KafkaConnectionArgs` and
   `LineCatalogue` as living in `common` but doesn't pin the exact
   module — candidates: extend `crates/common/src/oauth_client.rs` (for
   `InternalOAuthArgs`, since `OAuthCredentials`/`OAuthTokenCache` already
   live there) and add a new `crates/common/src/service_args.rs` for the
   clap-specific structs (keeping `oauth_client.rs` itself free of a
   `clap` import, matching its current zero-clap state), plus a
   `crates/common/src/config.rs` or extending `lib.rs` directly for
   `LineCatalogue` (since `LineDefinition` itself lives in `lib.rs`).
   Left for the plan stage.
2. **Exact generic/trait shape for `run_poll_loop`'s cycle parameter.**
   This document specifies the conceptual contract (§3.1) but not whether
   it's `F: FnMut() -> Fut, Fut: Future<Output = anyhow::Result<()>>` or
   an `AsyncFnMut` bound (stable as of the edition-2024/rustc-1.85+ this
   workspace already requires — confirmed floor is actually rustc 1.88,
   per `docker/poller-incidents.Dockerfile:4-12`'s own build-log-derived
   comment). Either compiles; left as an implementation-time call.
3. **Whether to also align `clap`'s version pin** across every crate this
   work touches (currently `4.6.1` in some, `4.6.6` in others) while
   already editing all of their `Cargo.toml`s. Not required for this work
   to compile (Cargo will resolve one shared version workspace-wide
   regardless), but touching 9+ `Cargo.toml` files anyway makes it a
   near-zero-marginal-cost moment to do it, if desired.
4. **The new crate's final name.** `service-health` is used throughout
   this document as a working name; something else (`readiness`,
   `health-http`) may fit this workspace's existing naming conventions
   (`movement-feed`, `trust-schema`, `schedule-query` — all
   noun-phrase, none end in a generic word like "common" or "shared")
   better. Left for the plan stage.
5. **Whether `fetch_last_fetched` (`crates/common/src/ingest.rs:104-118`)
   should be rewritten in terms of the new `get_json` (§3.6) as a
   drive-by cleanup.** Not required — it wasn't one of the review's cited
   duplicates — but it's the same shape and sits in the same file the new
   function lands in.

---

## References

- Review findings as given in this task's prompt (not a separate document
  in this repo — reproduced and re-verified inline throughout §0/§3).
- `crates/common/src/metrics.rs:1-9,51-56` — `common`'s existing,
  documented no-`axum` stance, load-bearing for Decision 1.
- `crates/common/src/ingest.rs:1-16,39-62,88-118` — `post_batch`/
  `time_until_next_poll`/`fetch_last_fetched`, the existing module #6
  extends.
- `crates/common/src/oauth_client.rs:1-34` — `OAuthCredentials`/
  `OAuthTokenCache`, which `InternalOAuthArgs::token_cache()` (§3.2)
  wraps.
- `docker/poller-incidents.Dockerfile:1-57` — per-binary Docker build
  shape and the rustc-1.88 floor, both load-bearing for Decision 1 and
  Open Question 2.
- `.github/workflows/ci.yml:98,216-233` — workspace-wide CI build/test/
  clippy shape, load-bearing for Decision 1's axum-cost analysis.
- `docker-compose.yml:105-114,175-179,200-204,228-232,257-261,286-290,
  377-397,400-404,431-455,460-461` — current env var wiring, load-bearing
  for §3.2/§4's "no renames" claim.
- `charts/distant-signal/templates/poller-deployments.yaml:75-108` —
  current Helm env var wiring, same purpose.
- `crates/trust-consumer/src/health.rs`,
  `crates/full-coverage-consumer/src/health.rs`,
  `crates/movement-relay/src/health.rs:1-85` — full text re-read for §3.3.
- `crates/trust-consumer/src/main.rs:1-110`,
  `crates/full-coverage-consumer/src/main.rs:1-110` — full text re-read
  for §3.5.
- `docs/superpowers/plans/2026-09-05-status-observability-grafana-plan.md` —
  structural precedent for "one plan, ordered task groups" cited in §5.
