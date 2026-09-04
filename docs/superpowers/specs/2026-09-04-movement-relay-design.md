# Design: `movement-relay` — a Single Real Kafka Client, Fanned Out via Redis Streams

**Status: design proposal, not approved. Spec stage only — no implementation
plan, no code in this pass.**

## Why this exists, stated precisely

Two crates in this repo each independently open their own Kafka consumer
group against RDM's Train Movements product (`TRAIN_MVT_ALL_TOC`,
`pkc-z3p1v0.europe-west2.gcp.confluent.cloud:9092`):

- `crates/trust-consumer` holds the one REAL, RDM-issued group,
  `SC-c4d90f8e-c047-49b5-9892-6c9cda63e1eb` (a production Helm value
  override of `trustConsumer.kafka.consumerGroup` — the crate's own
  `kafka_consumer_group` default, `distant-signal-trust-consumer`
  (`crates/trust-consumer/src/config.rs:36`), is not what production runs).
- `crates/full-coverage-consumer` was built with its own, separate,
  made-up group id (`distant-signal-full-coverage-consumer`,
  `crates/full-coverage-consumer/src/config.rs:43`), reusing
  `trust-consumer`'s exact SASL credential but a different `group.id`
  (confirmed by reading `charts/distant-signal/templates/full-coverage-consumer-deployment.yaml:96-113`,
  which sources `KAFKA_SASL_USERNAME`/`KAFKA_SASL_PASSWORD` from the same
  `trustConsumer.kafka.existingSecret` keys `trust-consumer-deployment.yaml`
  uses, changing only `KAFKA_CONSUMER_GROUP`).

Deployed against the real cluster, that combination — same credential,
different group id — was rejected outright with `GroupAuthorizationFailed`.
The repo owner has since confirmed directly with RDM: **one consumer group
per account per API product, full stop.** This is a stronger, now-confirmed
version of a risk the prior design pass
(`docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`,
Decision 1) only *inferred* from Kafka's protocol ("SASL credentials
authenticate a connection, not a group membership... though this is
inferred, not independently confirmed against RDM's specific product
terms") — that inference is now empirically falsified by the real
`GroupAuthorizationFailed`, and every conclusion in this document is built
on the corrected, confirmed fact instead.

Two independent Kafka clients for this feed is therefore not viable as
built, ever, on this product. This document designs the fix the repo owner
asked for directly: a third service, `movement-relay`, becomes the sole
real Kafka client (holding `SC-c4d90f8e-...`), filters the feed down to
message types either downstream service could ever use, and republishes
into a Redis Streams queue with two consumer groups — one per downstream
service — with real backpressure/retention and crash-recovery semantics.
`trust-consumer` and `full-coverage-consumer` stop touching Kafka
altogether and become Redis Streams readers instead.

This is also a first-class fix for a second, independently-confirmed
production bug: `trust-consumer`'s own module doc
(`crates/trust-consumer/src/process.rs:32-45`) already documents that a
tracked train can get stuck `resolution_status: 'pending'` forever if the
Activation that would bind its `train_uid` isn't observed by the
*currently running* process before the resolving Movement — nothing ever
retries it. Confirmed live today: every service pod restarted around
17:35 UTC (an unrelated redeploy), and any train activated before that
restart, tracked/resolved after it, is now stuck permanently. A durable,
replayable queue that a restarted consumer can resume from its last
acknowledged position — not just a live fire-and-forget fan-out — is a
first-class goal of this design, not a nice-to-have.

Required reading, consumed in full before this document was written:
`crates/trust-schema/src/{schema,dedup,journey}.rs`,
`crates/trust-consumer/src/{process,feed/mod,feed/kafka,main,config,health}.rs`,
`crates/full-coverage-consumer/src/{main,config,feed/mod,feed/kafka}.rs`,
`crates/enricher/src/stream.rs` and its call sites in `crates/enricher/src/main.rs`,
`crates/api/src/data/queries.rs`'s `XADD` call and
`crates/api/src/data/train_tracking.rs`'s dedup upsert,
`charts/distant-signal/templates/{trust-consumer-deployment,full-coverage-consumer-deployment,aggregator-deployment,notifier-deployment,poller-deployments,schedulefeed-deployment,_helpers.tpl}.yaml`,
`charts/distant-signal/values.yaml`'s `redis:`/`trustConsumer:`/`fullCoverageConsumer:`
sections, and
`docs/superpowers/specs/2026-09-04-option-b-live-consumer-design.md`.

## Ground truth this document corrects

`docs/superpowers/specs/2026-08-28-train-tracking-design.md`'s research
(quoted in `crates/trust-schema/src/schema.rs:1-6`'s own module doc)
confirmed **five**, not four, real TRUST `msg_type`s, by name and field —
`crates/trust-schema/src/schema.rs:94-105` (`TrustMessage` enum) and the
match arms at `:135-151`:

| `msg_type` | Variant | Confirmed fields |
|---|---|---|
| `"0001"` | `Activation` | `train_id`, `train_uid`, `toc_id`, `train_service_code`, `schedule_wtt_id`, `schedule_start_date`, `schedule_end_date` |
| `"0002"` | `Cancellation` | `train_id`, `canx_timestamp`, `canx_reason_code`, `canx_type` |
| `"0003"` | `Movement` | `train_id`, `event_type`, `gbtt_timestamp`, `planned_timestamp`, `actual_timestamp`, `reporting_stanox`, `loc_stanox`, `toc_id`, `variation_status` |
| `"0006"` | `ChangeOfOrigin` | `train_id` |
| `"0007"` | `ChangeOfIdentity` | `train_id` |
| anything else (`"0005"`, `"0008"`, …) | `Unknown(msg_type)` | none — dropped by both consumers today, per `unconfirmed_msg_types_become_unknown_not_a_parse_error` |

(`ChangeOfOrigin` is `"0006"`, not `"0004"` as an earlier framing of this
task assumed — corrected here against the actual enum/match, not asserted
from memory.) All five confirmed types are acted on by `trust-consumer`
today (`process.rs:320-548`'s full `match`); `full-coverage-consumer`'s own
correlation is documented as needing effectively the same slice (per
`2026-09-04-option-b-live-consumer-design.md` Decision 2d, which reuses
`trust_schema::journey::apply_movement`/`apply_cancellation` directly). The
one real, safe filtering opportunity is dropping `Unknown` before either
consumer ever sees it — exactly what this document's relay does, and nothing
narrower, since both consumers need essentially the whole confirmed-type
slice.

## Decision 1: the new service

**`crates/movement-relay`**, a new binary crate, workspace member. End to
end, once per Kafka record:

1. Connect to RDM's Kafka broker with the one real credential and the one
   real group, `SC-c4d90f8e-...` (`rdkafka::StreamConsumer`, structurally
   the same `ClientConfig` `trust-consumer/src/feed/kafka.rs:37-60` already
   uses — `SASL_SSL`, `enable.auto.commit=false`,
   `enable.auto.offset.store=false`, explicit store-then-commit on
   confirmed downstream delivery).
2. For each raw record payload, classify every envelope inside it (normally
   exactly one bare `{header, body}` object — confirmed live, per
   `schema.rs`'s own module doc; the JSON-array-of-envelopes shape is
   handled defensively, never observed) by `header.msg_type` alone, against
   the confirmed five-type list above.
3. **Forward the surviving envelope's own raw bytes unchanged** — not a
   relay-typed re-encoding. See the rationale below; this is the one real
   design choice this section has to justify.
4. `XADD` each surviving envelope as its own Redis Stream entry.
5. Only commit the Kafka offset for a record once every surviving envelope
   it contained has been durably `XADD`ed (mirrors `trust-consumer`'s own
   "never commit on a failed downstream write" discipline,
   `main.rs::run_cycle`, `feed/kafka.rs`'s store-then-commit ordering).

**Raw passthrough, not a relay-typed re-encoding — justified against what
downstream actually needs to reconstruct.** The alternative considered was
publishing the already-parsed `TrustMessage` variant (JSON-serialized) so
`trust-consumer`/`full-coverage-consumer` wouldn't need to re-parse. Two
reasons against it:

- **Fidelity risk for no real gain.** `schema.rs`'s own structs already
  keep several `#[allow(dead_code)]` fields "for the same 'faithful port
  of the confirmed wire shape' reason" (`schema.rs:34-35`, `:51-53`,
  `:69-72`) — fields no consumer reads today but that are deliberately not
  discarded, because this repo's own convention (`schema.rs`'s header doc,
  "no invented API details") is to keep confirmed shape intact even when
  unused. Re-serializing through the typed struct is fine for every field
  *currently* modeled, but silently drops anything RDM's real payload
  carries that the struct doesn't declare (serde's default "ignore unknown
  fields" behavior) — a live payload has already disproven one shape
  assumption in this exact module twice (the header doc's own account of
  the array-vs-bare-object and multi-field discoveries). Raw passthrough
  can never lose a field neither consumer has modeled yet.
- **No parsing is actually eliminated by moving it earlier.** Both
  consumers already share `trust-schema` and already call
  `trust_schema::schema::parse_batch` themselves inside `process.rs::run_once`
  (`crates/trust-consumer/src/process.rs:308`). Publishing typed JSON would
  still require `movement-relay` to fully deserialize every body (to
  re-serialize it), so no parsing work is actually saved — it's only
  relocated, while adding real fidelity risk. Publishing raw bytes means
  `movement-relay` only ever needs to look at `header.msg_type` (a partial,
  cheap parse — see Decision 3), never the body, for every message it
  forwards.

The one, narrow, deliberate change requested — dropping `Unknown` before
either consumer sees it — is achieved with **zero change to body-shape
validation**: a confirmed-type envelope whose *body* fails to deserialize
still flows through the relay unchanged (the relay never inspects bodies)
and is still dropped exactly where it is today, inside each downstream
consumer's own `parse_batch`/`parse_envelope` call (`schema.rs:153-155`'s
existing warn-and-drop). That validation job does not move.

## Decision 2: Redis Streams design

**One stream, `movement-events`, two consumer groups — not per-message-type
streams.** Both downstream services need essentially the full confirmed-type
slice (Ground truth section above); there is no meaningful narrower split to
give either one, so multiple streams would only add operational surface
(two `MAXLEN` policies, two lag metrics, two things that can independently
drift) for no filtering benefit. This deliberately does **not** copy
`incident-text-changed`'s shape uncritically: that stream has exactly one
consumer group (`enricher`) and no `MAXLEN` at all
(`crates/enricher/src/stream.rs:10-33`, `crates/api/src/data/queries.rs:207-213`)
because it carries a few incident-text-change events a day, not ~630k/day.

- **Consumer groups**: `trust-consumer` and `full-coverage-consumer`, named
  literally (mirrors `enricher`'s `GROUP = "enricher"` constant,
  `stream.rs:11`). Each created idempotently via
  `XGROUP CREATE movement-events <group> $ MKSTREAM`, `BUSYGROUP` swallowed
  — verbatim the same pattern as `stream::ensure_group` (`stream.rs:14-33`).
- **Consumer name**: one fixed name per group (e.g. `trust-consumer-1`,
  `full-coverage-consumer-1`, mirroring `stream.rs:12`'s `CONSUMER =
  "enricher-1"`) — consistent with both crates' own existing
  `replicas: 1` / one-fixed-group-membership constraint
  (`trust-consumer/src/config.rs:32-35`,
  `full-coverage-consumer-deployment.yaml:26-28`'s identical reasoning).
  No new horizontal-scaling assumption is introduced by this move.
- **Field layout**: one field, `payload` — the surviving envelope's raw
  JSON bytes, verbatim (Decision 1). A second, redundant field, `msg_type`,
  extracted from the same header, is included purely for cheap
  `redis-cli`/`XRANGE` introspection and future coarse filtering without a
  full JSON parse — genuinely redundant with what's inside `payload`, kept
  anyway because the cost is one small string per entry and the debugging
  value is real (this is exactly the kind of live-feed surprise this
  module has already been burned by twice).
- **`MovementFeed`'s contract is unchanged.** `next_batch`'s existing doc
  (`crates/trust-consumer/src/feed/mod.rs:14-19`) already says each element
  is "one raw Kafka record's payload... normally a single bare
  `{header, body}` envelope object." A Redis-Streams-backed implementation
  returns `payload` directly as that same `Vec<String>` element — `parse_batch`
  and every line of `process.rs::run_once`/`process_message` need **zero**
  changes. See Decision 3.

**Ordering and at-least-once.** Only `movement-relay` ever `XADD`s to this
stream (mirrors `api` being the sole `XADD`er of `incident-text-changed`,
`queries.rs:207`), so entries are in a single, total, monotonic order —
stronger than Kafka's own per-partition-only ordering. `XREADGROUP ...
STREAMS movement-events >` delivers only never-before-delivered entries;
each consumer only advances past an entry via `XACK`, called after (not
before) its own successful downstream write — `trust-consumer` acks only
after `post_train_events` to `api` succeeds, mirroring its existing
never-commit-on-a-failed-post rule (`main.rs::run_cycle:177-185`) exactly,
substituting `XACK` for `feed.commit()`.

**Redelivery is already safe — no new dedup work needed.**
`trust_schema::dedup::dedup_key` (SHA-256 of `train_id` + `msg_type` +
`event_type` + `loc_stanox` + `planned_timestamp`) is written into every
`TrainMovementEventMessage` and enforced server-side via
`ON CONFLICT (tracked_train_id, dedup_key) DO NOTHING`
(`crates/api/src/data/train_tracking.rs:317-323`). A Redis-Streams
redelivery — whether via a consumer re-reading its own pending-entries list
(Decision 2, restart case, below) or `XAUTOCLAIM` reclaiming a stuck entry
— redelivers the *exact same* `payload` bytes, so `dedup_key` computes
identically and the existing `ON CONFLICT` upsert absorbs it for free. This
is the same property the prior Kafka-based design relied on
(`process.rs:56-79`'s own module doc, "the dedup_key path... makes that
replay safe"), carried over unchanged.

**Backpressure/retention — the genuinely new piece.**

- **`XADD movement-events MAXLEN ~ N *`**, approximate trimming (`~`),
  matching Redis's own documented guidance that approximate trimming avoids
  exact trimming's O(entries-removed) cost on every write — relevant here
  since `movement-relay` writes at real feed volume.
- **Oldest-first eviction is a deliberate, explicit requirement, not an
  assumed default — stated here as such per the repo owner's own direction.**
  `XADD ... MAXLEN ~ N` always trims from the stream's head (its oldest
  retained entries) as it grows past `N`, which is exactly the wanted
  direction: stale old movement data is worse to keep than a gap, since
  both downstream consumers care about current train state, not a complete
  historical log — an old, superseded `Movement` for a train that has long
  since moved on is actively misleading if processed late, where a
  detected gap (below) is at least honestly flagged as missing. This falls
  out of Redis Streams' native trimming behavior with no extra code, but is
  recorded here as an intentional design choice this document is making,
  not a side effect nobody decided on.
- **Sizing against real, cited volume.** `docs/superpowers/specs/2026-08-28-train-tracking-design.md:400-406`:
  a real 9-day pipeline capture on the unfiltered national feed measured
  ~630k messages/day (~611k Movement, ~26.7k Activation, cancellations/
  change-of-origin/-identity in the low hundreds/day). Filtering `Unknown`
  removes a small remainder of that, so the stream should carry close to
  630k entries/day (~7.3/s average, materially bursty at peak — no real
  peak/trough figure exists in this repo yet, flagged as an open question
  below, not invented here). **Proposed `N = 500,000`** — roughly 19 hours
  of full-volume headroom at the average rate, chosen because both
  consumers are designed to process each batch essentially immediately (no
  intentional backlog — Kafka held that role before; this stream now does),
  so healthy lag should be seconds, and 19 hours is generous cover for a
  redeploy window that Decision 5's `maxUnavailable: 0` rollout should keep
  to low minutes, not hours. This is a starting figure, not empirically
  tuned against real production lag data — flagged as unresearched, same
  posture this repo already uses elsewhere for first-guess cadence
  constants (e.g. `trust-consumer/src/config.rs:126-128`'s own
  `stanox_crs_reload_secs` comment).
- **A consumer that falls behind past the trim window WILL lose entries it
  never read — by design, given oldest-first eviction above, not merely as
  an edge case under sustained load — so this must be a detected, handled
  case, not left as "this could happen."** Two layers, a leading indicator
  and a definitive one:
  - **Leading indicator (early warning, before any loss)**: expose
    `movement_relay_stream_lag{group}` (a `Prometheus` gauge, reusing
    `stream::group_lag`'s exact `XINFO GROUPS` → `lag` field query,
    `crates/enricher/src/stream.rs:128-165`, generalized to loop over both
    group names) and alert at a threshold meaningfully below `N` (e.g. 50%)
    so an operator is paged while there's still headroom to intervene, not
    after loss has already happened.
  - **Definitive detection (has loss already happened — a concrete,
    queryable answer, not a guess): compare the group's `last-delivered-id`
    against the stream's current oldest retained entry.** `XINFO GROUPS
    movement-events` reports each group's `last-delivered-id` (the highest
    ID it has been handed via `>`); `XINFO STREAM movement-events` reports
    `first-entry`'s ID (the oldest entry MAXLEN trimming has left behind).
    Stream IDs are monotonic `<ms>-<seq>` pairs and directly comparable. If
    `group.last-delivered-id < stream.first-entry.id`, entries between
    those two IDs were trimmed before this group ever read them — a
    provable gap, not a suspicion. (Separately, Redis 7's `lag` field
    itself is documented to report as indeterminate once trimming has
    outpaced a group's read position, since the entries-added/entries-read
    arithmetic it relies on desyncs from what the stream still holds —
    worth treating a `lag` that goes from a real number to unavailable as a
    corroborating signal, though this isn't independently re-verified
    against the exact `redis:7` image this chart pins, so the ID
    comparison above is the primary, load-bearing check, not this.)
    Run this check on the same cadence each consumer already reloads its
    other periodic state (`reference_reload_secs`-equivalent) — cheap, two
    more Redis calls on an existing timer, no new polling loop.
  - **What the affected service concretely does on a detected gap — stated
    plainly, not left unresolved.** There is no automated recovery: trimmed
    entries are gone, the same class of unrecoverable loss Kafka's own
    topic-level retention already exposes today (RDM's `retention.ms` for
    this topic is unconfirmed — a pre-existing gap this document doesn't
    close). The concrete, honest response is **detection and loud
    surfacing**, not silence: `tracing::error!` naming the last-delivered
    and new-first-entry IDs (so the approximate skipped range is visible in
    logs), plus a Prometheus counter
    (`trust_consumer_stream_gap_detected_total`/
    `full_coverage_consumer_stream_gap_detected_total`) an alert can key
    off. **The two services' exposure differs and both are named
    explicitly, not left as one glossed-over "affected state" note:**
    - `trust-consumer`: a gap can silently strand a pin exactly like the
      already-documented stuck-`pending`-forever bug
      (`process.rs:32-45`) — an Activation or its resolving Movement
      simply never arrives. There is no way to know *which* tracked trains
      were affected from the gap alone, so no targeted re-resolution is
      possible; the honest response is exactly what's described above
      (log + metric), which is a real improvement over today regardless —
      today this failure mode is undetected entirely, `process.rs`'s
      module doc only documents that it *can* happen, not that anything
      would ever notice. This document does not close the underlying gap
      (no retry/rebind mechanism exists, same as today); it makes the
      *event* of it happening observable, which it currently is not at all.
    - `full-coverage-consumer`: unlike a *redelivery* (already safe, its
      own `main.rs` module doc: "`DerivedState` fields are last-write-wins
      per event, not additive... redelivery of an already-processed batch
      would just re-derive the same state, not corrupt it"), a genuine
      *gap* means some population UIDs' real events are never seen at all
      for that window, which can bias its own shadow-mode `SampleStats`
      (e.g. inflating the "unconfirmed-by-window-close = cancelled" bucket
      the prior design doc's Decision 2d already flags as a real accuracy
      risk on its own). The concrete response: the same log+metric above,
      plus — since this consumer's whole purpose right now is producing a
      *trustworthy* shadow-mode comparison sample — treating any rail day
      during which a gap was detected as a day whose `SampleStats` should
      not be read as clean signal (a comment/log correlation is sufficient
      for this pass; a machine-readable "degraded" flag on the persisted
      row is a natural follow-up, not designed here).
- **PEL handling for a consumer that crashes mid-processing — two
  mechanisms, not one.** `XREADGROUP ... consumer STREAMS key >` only ever
  delivers entries never before delivered *to that consumer name*; entries
  already delivered to it but never `XACK`ed sit in its own
  pending-entries list, invisible to `>`. Since Decision 2 fixes one named
  consumer per group (matching the existing singleton-replica constraint),
  the direct, low-friction fix for "the pod that held this consumer name
  crashed or was redeployed" is: **on startup, before switching to `>`,
  read that consumer's own PEL first** (`XREADGROUP ... STREAMS
  movement-events 0` — id `0`, not `>` — replays whatever this exact
  consumer name left unacked last time). This is the direct mechanism that
  closes today's stuck-Activation bug: a restarted `trust-consumer` resumes
  from its last acknowledged position rather than silently skipping ahead.
  Layered on top, a periodic `XAUTOCLAIM` sweep — reusing
  `stream::claim_stale`'s exact cursor-until-`"0-0"` loop shape
  (`crates/enricher/src/stream.rs:88-126`), one instance per consumer
  group, `min_idle` sized generously against each service's own expected
  cycle latency (consume → derive → POST to `api` → ack should be
  sub-second in the healthy case; enricher's own `reclaimMinIdleSecs`
  default is sized against its slower LLM-call latency, so this service's
  own default should be smaller, not copied verbatim) — remains the
  general safety net for entries stuck under a genuinely dead consumer
  name, or any staleness the startup-PEL-replay step doesn't cover.

## Decision 3: the `MovementFeed` trait boundary

**Unify into a new shared crate, `crates/movement-feed`, consumed by both
`trust-consumer` and `full-coverage-consumer` — not folded into
`trust-schema`.** Today the two crates hand-duplicate `feed/mod.rs`
(`MovementFeed` trait, `FakeMovementFeed`) and `feed/kafka.rs`
(`KafkaMovementFeed`) almost verbatim — the full-coverage-consumer copy's
own header comment even names why: "this is genuinely per-consumer Kafka
plumbing, not shared logic Task 1's `trust-schema` extraction was ever
meant to cover" (`crates/full-coverage-consumer/src/feed/mod.rs:1-6`). That
reasoning held while each crate's Kafka plumbing was genuinely
per-consumer (different `group.id`). It no longer holds once both become
**structurally identical Redis Streams readers** — same stream, same
`XREADGROUP`/startup-PEL-replay/`XAUTOCLAIM` mechanics, differing only in
which named consumer group they read as. Continuing to hand-duplicate that
would violate this repo's own "one crate per concern" convention
(`DESIGN.md` §12, cited directly by the prior design doc's own Decision 1)
applied to genuinely shared logic, not just services.

**Not folded into `trust-schema` itself**, because that crate's own module
doc is explicit and load-bearing: "No I/O, no `tokio`, no `rdkafka`
dependency -- both real callers own their own Kafka plumbing; this crate
only understands the message bytes once they're already `&str`"
(`crates/trust-schema/src/lib.rs:9-11`). A Redis-Streams-performing trait
implementation is exactly the I/O that doc comment excludes; adding it
would make every future reader of `trust-schema` re-verify that boundary
still holds. A new sibling crate keeps `trust-schema` exactly as pure as it
is today (zero changes to it beyond the one small classification helper
Decision 1/this section needs — see below).

```
crates/trust-schema/           # UNCHANGED except one small addition:
  src/schema.rs                 # + pub fn confirmed_envelope_bodies(raw: &str)
                                 #   -> anyhow::Result<Vec<String>> -- shares
                                 #   parse_batch's array/bare-object dispatch
                                 #   and the SAME confirmed-msg_type list
                                 #   parse_envelope's match already encodes
                                 #   (single source of truth, not a second
                                 #   hand-copied list) -- classifies via
                                 #   serde_json::Value only (never
                                 #   deserializes into a typed struct), and
                                 #   re-serializes each surviving envelope
                                 #   Value verbatim, byte-faithful even in
                                 #   the rare multi-envelope-array case.
                                 #   Pure, no I/O -- consistent with the
                                 #   crate's existing boundary. movement-relay
                                 #   is this function's only real caller.

crates/movement-feed/          # NEW -- pure-Rust trait + two impls, no
  src/                          # trust-schema dependency (works on opaque
    lib.rs                      # strings, same as the trait already did).
    kafka... NOT here           # movement-relay's OWN Kafka consume loop
                                 # does NOT depend on this crate -- it is a
                                 # producer/publisher, not a MovementFeed
                                 # implementer. This crate is consumed only
                                 # by the two DOWNSTREAM Redis-Streams readers.
    redis_stream.rs              # RedisStreamMovementFeed: XREADGROUP with
                                  # startup-PEL-replay-then-`>`, ack() calls
                                  # XACK, a periodic XAUTOCLAIM sweep
                                  # (Decision 2). Constructed with
                                  # (stream, group, consumer) -- the only
                                  # per-caller difference.
    lib.rs (trait + FakeMovementFeed)  # MovementFeed trait signature
                                        # UNCHANGED (next_batch/commit names
                                        # kept, minimal churn to process.rs
                                        # call sites in both crates) --
                                        # only the doc comment's description
                                        # of what "commit" does at the
                                        # transport level changes (XACK
                                        # instead of Kafka offset store+commit).
                                        # FakeMovementFeed hoisted verbatim
                                        # from the two existing, near-identical
                                        # copies -- one copy, not two.

crates/trust-consumer/         # feed/kafka.rs DELETED. feed/mod.rs shrinks
                                # to `pub use movement_feed::{MovementFeed,
                                # FakeMovementFeed};` or is removed and
                                # main.rs imports movement_feed directly.
                                # process.rs, matching.rs, journey usage:
                                # UNCHANGED -- run_once's signature is
                                # generic over `F: MovementFeed` already
                                # (process.rs:287), so swapping the concrete
                                # type in main.rs is the only call-site change.

crates/full-coverage-consumer/ # same shape, same deletions.
```

**Why this is genuinely minimal churn to `process.rs`'s consume loop in
each crate**: `run_once<F: MovementFeed>` (`process.rs:287-318`) already
takes the trait, not a concrete type — it was built generic specifically so
`FakeMovementFeed` could stand in for tests (`feed/mod.rs:1-4`'s own doc).
Swapping `KafkaMovementFeed` for `movement_feed::RedisStreamMovementFeed`
at each crate's one construction site in `main.rs` (currently
`KafkaMovementFeed::connect(&config, connection_state)?`,
`trust-consumer/src/main.rs:43`) is the entire integration change; nothing
in `process.rs`, `matching.rs`, `stanox_crs.rs`, or the full-coverage
crate's `correlate.rs`/`station_correlate.rs`/`population.rs` needs to
change at all, because `trust_schema::schema::parse_batch` still receives
exactly the same shape of string it always did (Decision 2's field-layout
choice is what makes this true — a typed-JSON relay design would have
broken this).

## Decision 4: the credential cutover — highest risk, reasoned through, not hand-waved

**The core hazard: Kafka consumer-group rebalancing does not tolerate two
independent services sharing one group without a real coverage gap.**
Kafka assigns each `TopicPartition` to exactly one group member at a time.
If old `trust-consumer` (still on its direct-Kafka path) and new
`movement-relay` were simultaneously members of `SC-c4d90f8e-...`, a
rebalance would **split** the topic's partitions between them — this is
not hypothetical, it's the documented, intended behavior a Kafka consumer
group exists to provide (`trust-consumer/src/config.rs:32-35`'s own doc
already describes this exact mechanic for the *horizontal-scaling* case:
"multiple trust-consumer replicas sharing one group would each get a
subset of partitions"). During that split, **neither service sees the
whole feed**: `trust-consumer` (still writing directly to `api`, since it
hasn't been switched to Redis yet) would miss Activations/Movements on
whatever partitions `movement-relay` was assigned, and nothing compensates
for that loss, because nothing is reading `movement-relay`'s share back
into `trust-consumer` during that same window. This is a **real, new
instance of the exact stuck-Activation bug this whole redesign exists to
fix**, self-inflicted by a careless cutover — the one outcome this section
exists to rule out.

**A same-service future rollout (`movement-relay` v1 → v2) does not have
this problem, and is worth stating explicitly since it looks identical at
first glance.** Two members of the same group *momentarily* splitting
partitions during a clean rebalance is safe here specifically because both
members are the *same* service, publishing to the *same* downstream sink
(`movement-events`) — the union of what v1 and v2 publish during the split
still covers the whole topic, just from two publishers instead of one, then
converges to one once v1 is confirmed gone. This is categorically different
from the cross-service (old `trust-consumer` vs. new `movement-relay`)
case, where the two members have **different** downstream effects and only
one of them is actually wired to matter yet. **Consequence**: `movement-relay`'s
*own* future redeploys can safely use Decision 5's `maxUnavailable: 0`
new-before-old rollout — but the *first*, real credential handoff cannot,
and must be sequenced as a deliberate stop-then-start, not a rolling
update. This is worth calling out as a genuine payoff of the migration
too: once `trust-consumer`/`full-coverage-consumer` are Redis Streams
readers, Kafka rebalance semantics become `movement-relay`'s problem alone
— neither downstream consumer ever has to reason about a Kafka rebalance
again, and Redis Streams consumer groups have no partition-splitting analog
at all (every member independently reads unclaimed `>` entries; two members
of a Redis Streams group sharing the read load has no coverage-gap failure
mode the way a Kafka rebalance does), which makes `maxUnavailable: 0` a
strictly *safer* rollout policy for the two downstream consumers under this
design than it was under direct Kafka.

**Proposed sequencing — spans at least three logical deploy steps, not one
`helm upgrade`:**

1. **Merge/Deploy A (low risk, normal review).** Ship `movement-relay`'s
   code, `crates/movement-feed`, and the Helm Deployment template, gated by
   a `movementRelay.enabled` toggle (matching this chart's existing
   `pollers.*.enabled`/`scheduleFeed.enabled` convention) defaulting to
   `false`. `trust-consumer`/`full-coverage-consumer` gain the new
   Redis-backed `MovementFeed` implementation as available but not yet
   wired to real traffic. Nothing about production Kafka behavior changes.
   `movement-relay` can be exercised against a placeholder/dev credential
   or `FakeMovementFeed`-equivalent fixtures in this step — zero blast
   radius on the real group, and a real verification point exists (its
   `/healthz`, logs, and `XADD` counts against a non-production target)
   before the real credential is ever involved.
2. **Deploy B (the actual handoff — manual, sequenced, watched, not
   automated on a first run).**
   - **B1**: stop `trust-consumer`'s Kafka connection cleanly — see the
     open question below on how, since this isn't currently a supported
     one-flag operation in this chart.
   - **B2**: only once B1 is confirmed (no member on `SC-c4d90f8e-...`),
     deploy `movement-relay` with `movementRelay.enabled=true` and the real
     credential/group moved from `trustConsumer.kafka.*` values to
     `movementRelay.kafka.*`. Verify it is the group's sole member and is
     actively `XADD`ing (readiness reporting healthy, a log line or
     `movement_relay_stream_lag`/`movement_relay_events_published_total`
     metric moving) before proceeding — this is the real, human-checked
     verification gate the brief asked for, deliberately not skipped.
   - **B3**: only after B2 is confirmed healthy, redeploy `trust-consumer`
     and `full-coverage-consumer` pointed at their new Redis-backed
     `MovementFeed`. These two redeploys can safely use `maxUnavailable: 0`
     (Decision 5) — Redis Streams groups tolerate the overlap, per the
     reasoning above.
3. **Merge/Deploy C (cleanup, low risk, normal review + rollout).** Delete
   the now-dead `feed/kafka.rs` in both consumer crates, the Kafka-related
   Helm values/guard blocks (`trust-consumer-deployment.yaml:1-22`'s guard,
   `full-coverage-consumer-deployment.yaml:1-18`'s duplicate guard, both
   crates' `kafka_*` env vars), and `docker/trust-consumer.Dockerfile`'s/
   `docker/full-coverage-consumer.Dockerfile`'s now-unneeded
   `rdkafka`-only build dependencies if nothing else in either crate still
   needs them. This step is not time-pressured — B's dangerous window ends
   once B3 lands; C can wait for ordinary review.

**Open questions flagged as genuine judgment calls a human should confirm
before planning proceeds — this section, not glossed over:**

- **"Stop `trust-consumer`'s Kafka connection cleanly" (B1) is not
  currently a one-flag operation.** `trust-consumer-deployment.yaml:34`
  hardcodes `replicas: 1` as a YAML literal, not `{{ .Values.trustConsumer.replicaCount
  }}` — there is no values-driven way to scale it to zero today. The
  practical options are (a) an out-of-band `kubectl scale
  deploy/<name>-trust-consumer --replicas=0` for this one manual operation,
  not wired into the chart, or (b) adding a `trustConsumer.replicaCount`
  (or an explicit `trustConsumer.kafkaEnabled` toggle disabling
  `feed/kafka.rs`'s activation in-process) as a first-class, supported
  chart knob. Which is worth building versus doing once by hand is a real
  design decision the implementation plan should make explicitly, not
  infer from this document.
- **Neither crate calls a graceful `consumer.close()`/`LeaveGroup` today**
  (confirmed absent by grepping both crates and both `feed/kafka.rs`
  files) — a bare process termination (Kubernetes SIGTERM on scale-to-zero)
  relies on the broker's `session.timeout.ms` to evict the member, which
  is slower and less certain than an explicit clean departure. Worth
  fixing as part of B1's mechanism (a graceful shutdown handler calling
  `StreamConsumer`'s close path before the process exits) rather than
  assuming SIGTERM alone is fast/clean enough — flagged, not resolved
  here.
- **RDM's own `retention.ms`/replay window for `TRAIN_MVT_ALL_TOC` is
  unconfirmed.** If B1→B2's window is longer than that retention, some
  messages could already be gone from Kafka itself before `movement-relay`
  ever starts reading — a pre-existing risk this document doesn't invent
  but that makes "keep B1→B2 as short as operationally possible" a real
  constraint, not just a tidiness preference.
- **Whether this cutover should be treated as a one-time, closely-watched
  manual operation (repo owner present, tailing logs) versus something
  scripted for repeatability is a judgment call**, not decided here — given
  there is no sandbox/second group to rehearse against (RDM's one-group
  rule applies to this exact scenario too), the first real run carries
  irreducible risk regardless of tooling.

## Decision 5: rollout/readiness

**New Helm Deployment, `charts/distant-signal/templates/movement-relay-deployment.yaml`**,
structurally modeled on `trust-consumer-deployment.yaml` (`replicas: 1`;
`automountServiceAccountToken: false`; `podSecurityContext`/
`containerSecurityContext` via the exact same
`distant-signal.podSecurityContext`/`distant-signal.containerSecurityContext`
helpers every other Deployment uses, `_helpers.tpl:138-155`, with
`readOnlyRootFilesystem: true` matching `trust-consumer`'s own container),
with two deliberate differences from that template:

- **Explicit `strategy: { type: RollingUpdate, rollingUpdate: { maxUnavailable:
  0, maxSurge: 1 } }`.** Neither `trust-consumer-deployment.yaml` nor
  `full-coverage-consumer-deployment.yaml` declares a `strategy:` block
  today (Kubernetes' own default for a `Deployment` — 25%/25% — happens to
  round to the same numbers at `replicas: 1`, but that's an accident of the
  default's rounding, not a documented intent, and isn't safe to rely on
  if replicas ever changes). This chart already has a strong, explicit
  convention of stating `strategy:` deliberately —
  `aggregator-deployment.yaml:10-13`, `notifier-deployment.yaml:12-13`,
  `poller-deployments.yaml:27-28`, and `schedulefeed-deployment.yaml:24-25`
  all explicitly set `type: Recreate`, each with a one-line comment
  explaining why (a rolling update would briefly double-write, or
  double-send, or double-touch a shared PVC). `movement-relay` needs the
  opposite policy for the opposite-shaped reason — it must never have zero
  members ready before a new one takes over, per the repo owner's own
  request and Decision 4's "own future redeploys are safe under
  new-before-old" finding — so it gets its own explicit block, following
  the same convention of stating intent plainly rather than relying on an
  unstated default.
- **A readiness probe that means "joined the Kafka consumer group and is
  actively consuming," not just "the HTTP server answered."**
  `trust-consumer/src/health.rs`'s existing `ConnectionState` flips `true`
  "once the consumer has successfully polled at least one batch (or
  confirmed group membership)" (`health.rs:14-17`) — a reasonable liveness
  proxy, but a looser signal than this service specifically needs: it
  conflates "the group rebalance completed and partitions were assigned"
  with "the feed happened to produce a message since," which could report
  not-ready during a genuine lull even though group membership is
  perfectly fine. Since `movement-relay`'s readiness is the actual gate
  Decision 4's rollout safety depends on (whether the *new* pod has truly
  taken over group membership before the *old* one — the case that matters
  here — is torn down), this document recommends a tighter signal:
  `rdkafka`'s `ConsumerContext`/rebalance callback (`pre_rebalance`/
  `post_rebalance`) flips readiness true on a confirmed partition
  assignment, independent of whether a message has arrived yet. This is a
  deliberate improvement over `trust-consumer`'s existing looser proxy,
  not a copy of it — worth a comment at the call site explaining why the
  two differ, so a future reader doesn't "fix" the inconsistency by making
  them match.
- **Liveness probe reused verbatim** from `trust-consumer`'s own shape
  (`initialDelaySeconds: 30, periodSeconds: 15, failureThreshold: 6`,
  `trust-consumer-deployment.yaml:73-79`) — unrelated to the
  maxUnavailable concern, no reason to differ.
- **`movementRelay.enabled` toggle** (Decision 4, Merge A) is a deliberate,
  temporary affordance for the staged cutover, unlike `trust-consumer`'s
  own permanent no-toggle posture. Whether to keep it after the cutover
  completes (a documented "kill switch") or drop it once stable (matching
  `trust-consumer`'s own precedent of no toggle for a load-bearing,
  always-on service) is an implementation-time call, not decided here.

## Decision 6: `full-coverage-consumer`'s made-up group — migration/cleanup

**Non-issue, with one honest caveat.** `distant-signal-full-coverage-consumer`
never successfully joined — the real cluster rejected it with
`GroupAuthorizationFailed`, an ACL rejection at the `JoinGroup` request
stage. Kafka brokers only persist a consumer group's metadata once a
client successfully joins it at least once; an ACL-rejected join attempt
does not create group state to clean up. There is therefore almost
certainly nothing to delete on RDM's side for this specific group id, and
no data-continuity concern (it never held or processed a single real
message). **Caveat, stated honestly rather than asserted as fact**: this
conclusion is inferred from Kafka's own protocol ordering (ACL check
precedes group creation), not independently confirmed against RDM's
specific implementation or any dashboard RDM might expose for "consumer
groups registered against this subscription." If RDM exposes such a view,
a one-time manual check that `distant-signal-full-coverage-consumer` never
appears there is cheap and worth doing before calling this fully closed —
but there is no cleanup *task* to plan around by default. Separately: since
`full-coverage-consumer` never had its own independent SASL credential (it
always reused `trust-consumer`'s, per the Helm template read in the
"Why this exists" section), there is no second credential to revoke either
— Decision 4's Merge/Deploy C deleting its now-dead `feed/kafka.rs` and
Kafka env vars is the entire cleanup, and it was already planned there.

## Non-goals

- **`trust-schema`'s parsing logic itself.** Untouched beyond the one
  small, additive `pub fn confirmed_envelope_bodies` (Decision 3) —
  stated plainly as a real, if small, change, not hidden inside "unchanged."
  `schema::Activation`/`Movement`/`Cancellation`/`ChangeOfOrigin`/
  `ChangeOfIdentity`, `dedup::dedup_key`, and every function in `journey.rs`
  are byte-for-byte unchanged.
- **How `trust-consumer`/`full-coverage-consumer` derive or write their own
  downstream state once they have a message.** `process.rs::process_message`,
  `matching::resolve_origin_departure`, `correlate.rs`,
  `station_correlate.rs`, `population.rs`, and every write path to `api` —
  unchanged. This document only replaces the transport underneath
  `MovementFeed`, not anything above it.
- **`full-coverage-consumer`'s own correlation-logic correctness or
  value.** Shadow mode (`LineDefinition.full_coverage_enabled: false`
  everywhere) is unaffected — this document changes how events *reach* that
  consumer, not what it does with them once they arrive.
- **Flipping `full_coverage_enabled` for any real line, or any change to
  its default.**
- **The separately-broken `invalid_grant` OAuth credential issue affecting
  `schedule-reference`/`full-coverage-consumer`'s own internal API auth.**
  A distinct, already-identified problem with `api`'s internal
  service-to-service OAuth, unrelated to Kafka/Redis transport — explicitly
  out of scope here.
- **Redis's own deployment/HA posture.** `redis.enabled: true` today
  deploys a single, non-clustered `redis:7` instance with no explicit
  `resources` limit (`values.yaml:685-728`). This document introduces a
  new dependency both `trust-consumer` and `full-coverage-consumer` did not
  have before: today, Kafka (RDM-managed, external) is the shared upstream
  both depend on; after this change, this app's own single Redis instance
  becomes a new, self-hosted single point of failure sitting between
  `movement-relay` and both downstream consumers. Sizing/HA-hardening
  Redis for this new role (dedicated resource limits, `maxmemory-policy`
  interaction with `MAXLEN` trimming, whether the existing single instance
  is even the right shape once this stream's write volume lands on it) is
  flagged as a real, new operational cost this document does not solve —
  worth its own follow-up, not invented here.
- **A UI/dashboard for the new stream's health.** Prometheus metrics
  (`movement_relay_stream_lag`, publish counters) are proposed as the
  detection mechanism; a dashboard to visualize them is not designed here.

## Open questions / risks (collected)

1. Real peak-vs-average message rate on `TRAIN_MVT_ALL_TOC` is unresearched
   — the `MAXLEN` sizing above uses the one real average-volume figure this
   repo has (`2026-08-28-train-tracking-design.md`'s 9-day capture) with
   generous headroom, not a measured peak.
2. RDM's own Kafka topic-level retention (`retention.ms`) for this product
   is unconfirmed — relevant to how long Deploy B's B1→B2 window can safely
   be, and a pre-existing gap this document doesn't invent.
3. Whether `trustConsumer.replicaCount`/an explicit Kafka-enable toggle is
   worth adding to the chart as a first-class, supported mechanism for B1,
   versus doing the one real cutover by hand — a genuine implementation-time
   call (Decision 4).
4. Neither existing crate performs a graceful Kafka `LeaveGroup` on
   shutdown today — worth fixing as part of this work rather than assumed
   away (Decision 4).
5. Whether `movementRelay.enabled` should remain a permanent toggle or be
   removed once the cutover is stable (Decision 5).
6. Redis's own single-instance, unbounded-resources posture becomes a new
   real dependency this design introduces and does not harden (Non-goals).
7. Whether RDM exposes any visibility into registered consumer groups for
   this subscription, to positively confirm Decision 6's "nothing to clean
   up" conclusion rather than rely on inference from Kafka's protocol
   ordering.
