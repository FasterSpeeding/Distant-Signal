# STANOX->CRS Reference Data: Live Refresh & Coverage Extension — Research

**Status: research/survey only, not an approved design.** Written to the
same rigor as `docs/superpowers/specs/2026-08-31-other-uk-transit-networks-research.md`
(structural template — a research doc that evaluates plausibility, cites
what it could and couldn't confirm, and reaches a recommendation without
being an implementation plan) and cross-checked against this repo's own
prior, already-executed research on the same file
(`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md`'s
"Claim 3", `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`'s
Task 1). No code, CSV, or Dockerfile in this repo was modified to produce
this document.

## Problem being researched

`crates/trust-consumer/src/stanox_crs.rs` translates TRUST Train Movements
STANOX codes to National Rail CRS codes using `reference-data/stanox-crs.csv`,
a **checked-in, static, 2-column, 3124-row file**, loaded once at process
startup and baked into the `trust-consumer` Docker image at build time. It
has never been regenerated since it was first created. Two related changes
were asked to be evaluated, not implemented:

1. Replace the static file with a regularly (the ask specified "daily")
   refreshed cached resource, so this data can't go stale between manual
   file edits / image rebuilds.
2. Extend coverage/schema using `stations.json` (an extracted CORPUS-shaped
   file bundled in `train-mcp.zip`, present at the repo root), which has
   more fields (`crs, name, tiploc, stanox, nlc`) and ~600 more rows than
   the current CSV.

A specific lead was flagged for investigation: this session's own
licence-compliance work found a real, signed Rail Data Marketplace
licence for **"NWR CORPUS"** — Network Rail's standard STANOX/TIPLOC/
CRS/NLC cross-reference dataset — already held by this app, which looked
like a strong candidate as an authoritative upstream source for both
asks.

## Method

Direct reading of the current implementation
(`crates/trust-consumer/src/stanox_crs.rs`, `config.rs`, `process.rs`,
`docker/trust-consumer.Dockerfile`, `reference-data/stanox-crs.md`); `git
log --follow` against the data file for its real update history; a
`grep -rni corpus` sweep of `crates/`, `docs/`, and `charts/`; direct
reading of this repo's own prior CORPUS/CIF research
(`docs/superpowers/specs/2026-08-29-trust-schedule-delay-*.md`,
`docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md`,
`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md`) and the
already-committed `crates/schedule-ingest` crate + `charts/distant-signal/
templates/schedulefeed-*.yaml` it ships with; a Python script parsing and
cross-referencing `stations.json` against `reference-data/stanox-crs.csv`
directly (row counts, key overlap, per-key CRS agreement); and direct
reading of the two closest existing "poll an upstream feed into Postgres"
precedents, `crates/poller-stations` and (by file listing/line-count only)
`crates/poller-tocs`. No web fetches were performed — every claim below is
grounded in a file this session actually read, or in the CORPUS/Darwin
licence terms the task brief stated were already confirmed this session
and should be treated as fact.

## Current mechanism, in full

### Loading and lookup

`StanoxCrsTable::from_file` (`crates/trust-consumer/src/stanox_crs.rs:72-76`)
reads `reference-data/stanox-crs.csv` and calls `parse` (`stanox_crs.rs:93-132`),
a hand-rolled, header-name-driven (not positional) CSV parser chosen
deliberately over the `csv` crate because "STANOX and CRS values are short,
fixed-shape alphanumeric codes with no embedded commas/quoting to worry
about" (`stanox_crs.rs:88-92`). `parse` fails loudly — `bail!`s — on an
empty stanox/crs, a duplicate stanox, or a missing required column
(`stanox_crs.rs:120-128`), matching this codebase's "config load fails
fast at startup" posture cited against `common::LineDefinition::from_file`.

Lookup (`stanox_crs.rs:143-153`, `stanox_to_crs`) trims the input, tries an
exact match, and — defensively, since real TRUST STANOX are always
5-digit zero-padded — retries with leading-zero padding for a short
numeric input. A miss returns `None`, never an error.

Wiring: `crates/trust-consumer/src/config.rs:99-107` exposes
`--stanox-crs-file` / `STANOX_CRS_FILE` (default
`/app/reference-data/stanox-crs.csv`) via a `clap` `value_parser` that
calls `StanoxCrsTable::from_file` **at CLI-parse time**, i.e. once, at
process startup, before `main` does anything else
(`crates/trust-consumer/src/main.rs:34`, `let config = Config::parse();`).
`docker/trust-consumer.Dockerfile:59` (`COPY --chown=trust-consumer:trust-consumer
reference-data/ /app/reference-data/`) bakes the file into the image at
build time. **There is no code path anywhere in `trust-consumer` that
re-reads this file, or any other STANOX->CRS source, after startup** — the
table lives for the whole process lifetime as a field of the immutable
`Config` struct built once in `main`.

### How `process.rs` uses it

`crates/trust-consumer/src/process.rs:315-316` is the sole translation
call site: `movement.loc_stanox.as_deref().and_then(|stanox|
stanox_crs.stanox_to_crs(stanox))` produces `loc_crs: Option<String>`.
That `Option` then:

- Gates matching a Movement to a pin's expected origin departure —
  `loc_crs_for_match` (`process.rs:347`) is `?`-early-returned if `None`,
  and feeds `crate::matching::resolve_origin_departure`
  (`process.rs:366`).
- Is threaded into `crate::journey::apply_movement`
  (`process.rs:373`) as `loc_crs.as_deref()` — the module doc for
  `stanox_crs.rs:32-35` states the caller "already falls back to the raw
  STANOX for display" when this is `None`.
- Is persisted verbatim (`loc_crs`, `process.rs:412`) alongside the raw
  `loc_stanox` (`process.rs:411`) on the emitted train-movement-event
  record.

### What a lookup miss means today (confirmed by the test suite, not just the doc comment)

`stanox_crs.rs`'s own module doc (lines 27-35) and its tests distinguish
three miss categories, all collapsing to the same `None`, deliberately:

- **Genuinely non-passenger locations** — signals, junctions, sidings,
  depots — have no CRS at all. Of 12,085 real `TI` records in the
  2026-08-28 CIF extract this table was built from, 8,510 carry no CRS at
  all (`reference-data/stanox-crs.md:93-94`) — expected, not a data gap.
- **Deliberately excluded ambiguous STANOX** — 5 real STANOX values
  (`89428`, `87981`, `89530`, `86935`, `52215`) where two genuinely
  different, equally-valid CRS candidates share one physical STANOX with
  no principled way to prefer one — excluded entirely rather than guessed
  (`reference-data/stanox-crs.md:106-113`; test
  `an_ambiguous_excluded_stanox_translates_to_none`, `stanox_crs.rs:247-254`).
- **A genuinely unknown/malformed input** — `"00000"`, `"99999"`,
  non-numeric strings, empty strings (test
  `an_unknown_stanox_translates_to_none_not_a_panic`, `stanox_crs.rs:238-244`).

A fourth, *resolved-not-missed* case is also tested: 9 further STANOX
values are shared between a real passenger CRS and a non-passenger
`X`-prefixed pseudo-code (e.g. STANOX `87201` = both `VIC`
London Victoria and `XVR` Victoria Carriage Road); the table prefers the
non-`X` candidate (test `a_shared_stanox_resolves_to_the_real_passenger_crs_not_the_pseudo_code`,
`stanox_crs.rs:227-235`). **This disambiguation policy is hand-applied
provenance, not something the file format itself encodes** — any
replacement data source has to re-derive or re-apply the same policy, not
just supply more rows (see the `stations.json` comparison below, which
found this policy is *not* already applied in that file).

### Update history in practice: none, ever

`git log --follow -- reference-data/stanox-crs.csv` returns exactly one
commit, `d2fe0f1` ("Move trust-consumer's hardcoded STANOX->CRS table into
an external, runtime-loaded config file") — the commit that *created* the
file. **It has never been regenerated or edited since.** There is no
CI job, cron, script, or Makefile target that regenerates it automatically
— the only regeneration procedure that exists is the fully manual,
human-run recipe documented in `reference-data/stanox-crs.md:126-136`
(`unzip -p timetable_full.zip RJTTF942MCA.txt | grep '^TI' > ti.txt`, then
hand-run the exclusion policy, then re-sort and rewrite the CSV). This
directly confirms the premise of the user's ask: today, the only way this
data gets fresher is a human noticing it's stale, manually re-running that
recipe against a fresh CIF extract, committing the result, and waiting for
the next image build/deploy.

## The CORPUS licensing angle — investigated concretely

`grep -rni corpus crates/ docs/ charts/` finds **zero CORPUS ingestion
code anywhere in this repository.** The only `crates/` hit is an unrelated
string in `crates/enricher/src/llm.rs:901` ("golden corpus" — an LLM-eval
term, nothing to do with rail reference data). Every other hit is prose in
this repo's own prior research/plan documents
(`docs/superpowers/specs/2026-08-29-trust-schedule-delay-*.md`,
`docs/superpowers/plans/2026-08-29-trust-schedule-delay-validation.md`,
`docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md`).
**If CORPUS is pursued as a direct integration, this is genuinely new
ingestion work — there is no existing wiring to extend.**

The licence itself, however, is real and already confirmed by this
session's earlier work
(`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md:72-90`,
Task 1, "VERIFIED — favorable, not an open question"): **"NWR CORPUS"**
(RDM product `P-9d26e657-26be-496b-b669-93b217d45859`), publisher Network
Rail, **OGL v3.0 — free**, permitted purpose "may be made freely available
or otherwise distributed to third parties," UK-only territory, **monthly**
update frequency. (A companion licence, "Darwin Timetable Files," product
`P-9ca6bc7e-62e1-44d6-b93a-1616f7d2caf8`, publisher Rail Delivery Group,
same OGL3/free terms, is **daily** cadence — this distinction matters
directly for the cadence recommendation below.) The untracked PDF
filenames visible in this worktree's `git status`
(`P-9d26e657-26be-496b-b669-93b217d45859.pdf`,
`P-9ca6bc7e-62e1-44d6-b93a-1616f7d2caf8.pdf`) match these product IDs
exactly, corroborating the finding.

### A closer, already-in-flight candidate pathway than raw CORPUS

This app already has real, committed, in-progress infrastructure for
ingesting the **CIF SCHEDULE / Darwin Timetable Files** product — the
*other* licence, the daily one — via SFTP push: `crates/schedule-ingest`
(module doc, `main.rs:1-33`) plus `charts/distant-signal/templates/
schedulefeed-{configmap,deployment,pvc,secret,service}.yaml` plus
`crates/api/migrations/20260901130000_schedule_feed_ingests.sql`. It
watches a locally-mounted directory a sibling self-hosted SFTPGo container
writes into (`config.rs:16-17`, default `/data/schedule-feed/incoming`),
waits for the delivery's own manifest (`RJTTF<nnn>DAT.txt`) and every file
it lists to be present and mtime/size-stable, then moves the completed
delivery to `storage_dir` and records `{sequence, ingested_at, files}` via
`POST /private/schedule-feed-ingests`. Its default `check_times`
(`config.rs:31`, `"22:00,22:30,23:00,23:30,00:00,00:30,01:00,01:30,16:00"`)
mirror RSPS5046's own documented overnight delivery window plus a 16:00
fallback — this is a genuinely daily-cadence pipeline, already running
against a real, already-licensed product.

Crucially, the files that pipeline receives (`RJTTF*MCA.txt`,
`RJTTF*MSN.txt`, confirmed by the test fixture list at `main.rs:490-501`)
are **the exact same file types** `reference-data/stanox-crs.csv` was
hand-derived from — this repo's own prior research already established
this precisely: `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md`'s
"Claim 3" (lines 200-266) found that `TI` records inside `MCA` carry
STANOX+TIPLOC+CRS together for most locations, `MSN`'s `A` records fill
the remaining blank-CRS gap (e.g. Waterloo's `WATRLMN` TIPLOC), and
concluded: "if this app ever ingests the CIF schedule feed at all... the
STANOX<->CRS mapping trust-consumer needs comes along for free as a
byproduct of the same file... CORPUS may still be worth having for extra
robustness/currency... but it is no longer obviously *necessary*."

**However, `schedule-ingest` does not currently do this.** Reading
`crates/api/migrations/20260901130000_schedule_feed_ingests.sql` confirms
the `schedule_feed_ingests` table only stores a per-delivery manifest —
`sequence`, `ingested_at`, and a `files JSONB` array of `{name, bytes}` —
**not** parsed row-level STANOX/TIPLOC/CRS content. Nothing in
`schedule-ingest`'s own code opens `MCA`/`MSN` and extracts rows; it moves
opaque files into `storage_dir` and stops. Its own design doc says this
explicitly, unprompted: "It also does not touch `crates/trust-consumer`
in any way... that crate's STANOX<->CRS matching gap is separate,
currently-in-flight work this document neither depends on nor blocks"
(`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md:14-16`).
**This is real, closer infrastructure than starting from zero, but it is
not a wiring change either** — parsing `MCA`/`MSN` into structured rows
and getting them somewhere `trust-consumer` can read is still unbuilt
work, on top of an already-built file-delivery pipeline.

### An unresolved, honestly-flagged gap: does CORPUS actually ride the same pipe?

`schedule-ingest`'s SFTP pipeline is specifically the **DTD** (Data
Transformation and Distribution service, owned by RDG) delivery channel
for the **Darwin Timetable Files** product — confirmed via RSPS5046
(cited in `docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md:82-99`),
a **Rail Delivery Group** product. **NWR CORPUS is published by Network
Rail**, a different publisher, and this session's prior research (the
Open Rail Data Wiki citation in
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md:197-207`)
describes it as available "via the same 'All Reference Data' topic on
both the legacy Network Rail Open Data platform and RDM" — language that
was never independently verified against a live delivery mechanism (the
wiki page 403'd on direct fetch every time it was tried this session, per
that same document's citation notes). **Whether CORPUS specifically would
arrive over the same DTD/SFTPGo pipe `schedule-ingest` already handles, or
a separate Network-Rail-hosted delivery channel with its own onboarding,
is genuinely unconfirmed** — this should not be assumed either way in any
follow-up design.

## `stations.json`'s real coverage delta — measured directly

`stations.json` (repo root) is a JSON array of 3,721 objects,
`{crs, name, tiploc, stanox, nlc}`. Directly parsing it and cross-joining
against `reference-data/stanox-crs.csv` on `stanox` (Python, this
session) found:

- **3,142 of 3,721 records carry a non-null `stanox`; 579 do not** (e.g.
  `{"crs":"ABF","name":"Ashurst Bald Face Stag P.H.",...,"stanox":null}`,
  `{"crs":"AER","name":"Aberaeron Alban Square",...,"stanox":null}`) —
  these read as heritage-line/preserved-railway/request-stop CRS codes
  that have an `nlc` but no operational STANOX at all, i.e. locations
  TRUST's STANOX-keyed Movement messages could never reference regardless
  of table coverage. Confirms the brief's caution: `stations.json`'s
  `stanox` field is **not** populated for every entry.
- **Distinct non-null STANOX values in `stations.json`: 3,128** (some
  STANOX repeat across multiple records — see below) — close to, not
  dramatically larger than, the CSV's 3,124 rows.
- **Overlap**: 3,120 STANOX values appear in both files. **4 STANOX
  present in the CSV are absent from `stations.json`'s `stanox` field
  entirely** (`40056`->`TRW`, `65943`->`BEW`, `81307`->`HBR`,
  `65941`->`XKD`) — a real, if small, coverage *regression*, not just a
  gap. **8 STANOX are present in `stations.json` but not the CSV.**
- **`stations.json` reproduces the exact same 14-way ambiguity the CSV's
  own provenance doc independently documented** (`reference-data/stanox-crs.md:97-113`
  says "14 STANOX values map to more than one" CRS) — this session's
  direct parse of the raw `stations.json` array found **the identical 14
  STANOX values** carry two conflicting `crs` entries within
  `stations.json` itself (e.g. `87201` appears as both `VIC` "London
  Victoria" and `XVR` "Victoria Carriage Road"; `89428` as both `AFK`
  "Ashford (Kent)" and `ASI` "Ashford Int"). This is strong independent
  corroboration that both files derive from the same underlying CIF/CORPUS
  data family — but it also means **`stations.json` does not apply any
  disambiguation policy of its own**. For the 9 of 14 cases where the CSV
  resolved to the real passenger CRS over an `X`-prefixed pseudo-code
  (`BOG`, `CLJ`, `CTR`, `MCV`, `PRE`, `SAY`, `VIC`, `WEY`, `WIM`),
  `stations.json` genuinely contains *both* rows — a naive "just build a
  `HashMap` from `stations.json`" would silently resolve to whichever
  entry happened to load last (array order, not policy), which is exactly
  the class of bug this codebase's CSV was hand-curated to avoid.
- Of the 5 STANOX the CSV excludes entirely as genuinely irresolvable
  (`89428`, `87981`, `89530`, `86935`, `52215`), `stations.json` likewise
  offers no resolution — it just contains both raw candidates (e.g.
  `52215` -> both `SDI` "Stratford Intl" and `SFA` "Stratford
  International," two near-identically-named entries with no obvious
  tiebreaker). `stations.json` is not a source of new information for
  these; it reproduces the same underlying ambiguity CORPUS-family data
  has.
- **Net genuinely new, unambiguous STANOX->CRS coverage `stations.json`
  offers over the current CSV**: 3 rows — `57121`->`WIR` (Wirksworth),
  `72354`->`ZOC` (labelled "Oxford Circus Lt" — plausibly a
  light-rail/Underground-interchange code bundled into the CORPUS extract;
  not independently confirmed as a real National Rail booking code this
  session), and `86030`->`XWQ` (Woking Down Yard, a non-passenger
  siding). This is a marginal gain, not a material one, for the specific
  STANOX->CRS lookup `trust-consumer` needs.

**Verdict on `stations.json` as a direct drop-in replacement**: not safe
without re-implementing the CSV's own disambiguation policy first, has a
handful of its own coverage gaps the current CSV doesn't have, and adds
only 3 genuinely new, unambiguous STANOX rows. Its real value-add is
elsewhere: the 579 no-STANOX records (irrelevant to this specific lookup,
but potentially useful for a broader CRS/name/TIPLOC/NLC station
catalogue — see `docs/superpowers/specs/2026-08-31-station-catalogue-completeness-research.md`
if that's ever pursued as a separate thread) and the `tiploc`/`nlc`
columns, which `stanox_crs.rs`'s own module doc (lines 42-51) already
anticipated as a natural future extension ("adding a future column (e.g.
`tiploc`) means adding one more named lookup and one more field"), and
which `common::Station.tiploc: Option<String>`
(`crates/common/src/lib.rs:403`) already has a real, if only partially
wired, consumer in the line catalogue (used for exactly this kind of
CRS-ambiguity fix — see the `swr-alton.toml` Farnham/Fareham correction
this session's own earlier work found and fixed).

## Architecture options for a periodic-refresh mechanism

### This repo's existing "poll upstream reference data" precedent

`crates/poller-stations` (and, per its matching file/line-count
structure, `crates/poller-tocs`) is the closest existing shape: a
standalone binary, deployed as a Kubernetes **Deployment** (`replicas: 1`,
`strategy: Recreate` — `charts/distant-signal/templates/poller-deployments.yaml:19-28`,
templated identically for all five pollers), running a `tokio::time::
interval` loop (`poller-stations/src/main.rs:42-69`) that fetches an RDM
HTTP JSON feed, parses it, and `POST`s the parsed rows to `api`'s
`/private/stations` endpoint via `common::ingest::post_batch`
(`poller-stations/src/main.rs:78-85`), which `api` upserts into a
Postgres `stations` table. **There is no Kubernetes `CronJob` anywhere in
this repo's Helm chart** (`grep -rn "CronJob" charts/` returns nothing) —
every recurring job in this app today is a long-lived process with an
in-process timer, not an externally-scheduled batch job. Cadence is a
plain `poll_interval_secs` config value, defaulting to **86400 (24h)** for
`poller-stations` specifically because "RSPS5050 P-03-00 Rev A §6:
'updated overnight; Poll frequency should only be once every 24 hours'"
(`poller-stations/src/config.rs:31-34`) — i.e. this app already runs a
genuinely daily-cadence poller today, and does so as a long-lived
Deployment, not a CronJob. `common::ingest::time_until_next_poll`
(`crates/common/src/ingest.rs:83`) additionally lets a fresh restart skip
an unnecessary immediate re-poll if `api`'s stored data is still within
the interval — a "don't poll needlessly on every restart" refinement this
app already has and a new poller would reasonably reuse.

`schedule-ingest`'s own scheduling shape is meaningfully different
(check-time list gated on a known overnight delivery window rather than a
flat interval, and passive file-watching rather than outbound HTTP
polling — because its upstream is SFTP push, not a pull-able REST
endpoint) — a reminder that "daily cadence" doesn't force one single
mechanical shape; it should be picked to match the actual delivery
mechanism of whichever upstream is chosen, not copied wholesale from
either existing precedent.

### The real architectural mismatch to resolve: file load vs. a queryable table

`trust-consumer` currently has **no database dependency at all** — a
targeted grep of `crates/trust-consumer/Cargo.toml` and every `.rs` file
in the crate for `sqlx`/`postgres`/`PgPool`/`DATABASE_URL` finds zero
hits. It talks to exactly two things: Kafka (the TRUST feed) and `api`'s
HTTP endpoints (`api_ingest_url`, `api_tracked_trains_url`). This matters:
unlike `poller-stations`'s data (which flows into `api`, into Postgres,
and stays there for `api` itself to query), `trust-consumer`'s STANOX->CRS
table is loaded once, in-process, from a **file**, and never touches a
database at all today.

Three real options, weighed against what this crate already does:

1. **Give `trust-consumer` a direct Postgres dependency**, querying a new
   `stanox_crs` table per lookup or caching it with a periodic re-query.
   Matches the `poller-stations`-fed-table shape most literally, but is a
   new capability class for this specific crate — it has deliberately
   stayed DB-free so far, talking to `api` over HTTP for everything else
   (including its own periodic reference-reload, see below). Also turns a
   currently pure, dependency-free `HashMap` lookup
   (`stanox_crs.rs:143-153`) into either a per-message DB round-trip (bad
   for a message stream this app's own volume research already flagged as
   ~611k Movement messages/day nationwide) or an in-process cache with its
   own staleness/reload logic to build from scratch.
2. **Keep the file-based load, but have something else regenerate the
   file on a schedule, and have `trust-consumer` re-read it.** This is
   architecturally closer to `schedule-ingest`'s own shape (watch/land a
   file, verify it, hand off) but as noted above, nothing currently
   parses `MCA`/`MSN` into a `stanox,crs`-shaped output file, and
   `trust-consumer` itself has zero code today that re-reads a file after
   startup — this would need a genuinely new reload mechanism, not reuse
   of an existing one.
3. **Add a second in-process periodic HTTP reload, mirroring
   `trust-consumer`'s own existing `reference_reload_secs` pattern.**
   This is the closest-fitting option, because `trust-consumer` **already
   does exactly this shape of thing**, today, for a different piece of
   state: `main.rs:40-78` maintains a `reload_interval`/
   `last_reference_reload` pair and, once per tick, calls
   `queries::fetch_active_tracked_trains` over HTTP against `api`, then
   atomically swaps the result into `reference`/`state` via
   `process::apply_reference_reload` (`main.rs:64`) — all inside the same
   loop that also drives Kafka consumption (`config.rs:68-72`'s own doc
   comment: "How often to reload the active-tracked-trains reference set
   from `api`"). A STANOX->CRS live table is a natural sibling to this: a
   new `stanox_crs_reload_secs`-style timer, a small `api` endpoint
   serving the current table (fed by whatever produces fresh rows —
   `poller-corpus` or an extended `schedule-ingest`), and an atomic swap
   of `config.stanox_crs`'s current `Config`-embedded, load-once value for
   a `Mutex`/`ArcSwap`-wrapped, periodically-refreshed one. This keeps
   `trust-consumer` DB-free (consistent with its design so far), reuses an
   already-proven in-crate pattern instead of inventing a new one, and
   only requires whatever populates `api`'s new endpoint (a
   `poller-corpus`-shaped crate, or an extended `schedule-ingest` that
   also parses `MCA`/`MSN`) to follow the *existing* `poller-*` ->
   `/private/X` -> Postgres shape on the producer side.

**This is the most codebase-consistent shape of the three**: it reuses
one already-proven pattern on the consumer side (`trust-consumer`'s own
reference-reload timer) and one already-proven pattern on the producer
side (`poller-*` -> `/private/X` -> Postgres), rather than introducing
either a new DB dependency in a currently DB-free crate or a new
file-reload mechanism that doesn't exist anywhere in this codebase today.

## Licensing implications for cadence — "daily" needs a caveat

The user's own ask specified "likely daily." That is achievable, **but
only via one specific route, not via CORPUS directly**:

- **NWR CORPUS's real, signed licence states a monthly update
  frequency** (`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md:80-84`).
  A design that ingests CORPUS as its own separate feed and claims "daily
  freshness" would be overselling what the licensed upstream actually
  republishes — CORPUS itself cannot honestly support daily refresh,
  regardless of how often this app's own poller checks it. A
  CORPUS-sourced design should target **monthly** cadence, matching the
  licence, not the user's stated "daily" preference.
- **Darwin Timetable Files (the CIF SCHEDULE product `schedule-ingest`
  already ingests) is licensed at daily cadence**, and its own delivery
  pipeline already runs on a real overnight-plus-16:00-fallback daily
  check schedule (`schedule-ingest/src/config.rs:31`). Since this repo's
  own prior research already established that `MCA`/`MSN` — files that
  pipeline already receives daily — carry the STANOX<->CRS mapping "for
  free" (see the Claim 3 discussion above), **a design that extracts
  STANOX<->CRS from the schedule feed's own daily deliveries, rather than
  from a separate CORPUS ingestion, is the only route that honestly
  supports the "daily" cadence the user asked for.**

**Recommendation on cadence: target daily, but source it from the
schedule feed's own `MCA`/`MSN` files (piggybacking on the already-daily,
already-partially-built `schedule-ingest` pipeline), not from CORPUS.** If
CORPUS is pursued anyway (e.g. for its cleaner/more-purpose-built
per-location record, or to independently corroborate the schedule-feed
extraction), monthly is the honest cadence to promise for that specific
source — daily polling of a monthly-updated upstream would just mean 29
of every 30 polls find nothing new.

## Cost/benefit framing

- **Keeping the status quo (static file, manual regen)** costs nothing
  further but leaves the real risk the user flagged unaddressed: this
  file has been regenerated exactly zero times since creation, and
  nothing catches staleness — a renamed/decommissioned TIPLOC or a
  changed CRS assignment would silently under-translate more STANOX over
  time, with the only symptom being more pins that "don't match" (per
  `stanox_crs.rs`'s own documented miss-handling), not an error.
- **Swapping in `stations.json` wholesale** is cheap to try but is a false
  economy: it reproduces the exact same 14-way ambiguity the current CSV
  already hand-resolved, offers only 3 genuinely new unambiguous rows, has
  4 STANOX gaps of its own the current file doesn't have, and would need
  the disambiguation policy re-implemented in code (not just swapped data)
  to be safe. Not recommended as a direct drop-in on its own merits.
- **Building a standalone `poller-corpus`** against the real NWR CORPUS
  licence is genuinely new work (zero existing ingestion code found), its
  delivery mechanism relative to `schedule-ingest`'s existing DTD/SFTPGo
  pipe is unconfirmed (different publisher), and its licensed cadence
  (monthly) undercuts the user's stated "daily" goal. Medium-to-high
  effort, cadence mismatch, real coverage upside over what's already
  proven inside this repo's own CIF sample is unclear (Claim 3's own
  conclusion: CORPUS "may still be worth having for extra
  robustness/currency... but it is no longer obviously *necessary*").
- **Extending `schedule-ingest` (or a small sibling) to parse `MCA`/`MSN`
  into structured STANOX/TIPLOC/CRS rows, feeding a new `api` endpoint,
  consumed by a new periodic reload timer in `trust-consumer` mirroring
  its existing `reference_reload_secs` pattern** is real, scoped,
  medium-effort new work — but it reuses an already-licensed (daily),
  already-partially-built (file arrival, stability-checking, manifest
  verification all already exist in `schedule-ingest`), already-proven
  (this exact extraction logic was already hand-run once, successfully,
  to produce the current CSV, and is fully documented step-by-step in
  `reference-data/stanox-crs.md`) pipeline. This is the only option that
  is both genuinely daily and does not require a wholly new feed
  dependency.

## Recommendation

Ranked:

1. **Do not adopt `stations.json` (or raw CORPUS) as a direct drop-in
   replacement for `reference-data/stanox-crs.csv`.** The coverage delta
   is marginal (3 genuinely new unambiguous STANOX out of 3,124), it
   reproduces the current file's hardest edge cases without resolving
   them, and it introduces its own small coverage gaps. If pursued at
   all, `stations.json`'s `tiploc`/`nlc` columns are more valuable as
   future schema extensions (per `stanox_crs.rs`'s own doc anticipating a
   `tiploc` column) than its `stanox`/`crs` pair is as a wholesale data
   replacement.
2. **The highest-leverage next step, if live refresh is pursued, is
   extending the already-in-flight `schedule-ingest` pipeline (or a small
   sibling service) to parse `RJTTF*MCA.txt`/`RJTTF*MSN.txt` into
   structured STANOX->TIPLOC->CRS rows, applying this file's own
   already-documented exclusion/disambiguation policy in code instead of
   by hand, and serving the result to `trust-consumer` via a new small
   `api` endpoint.** This reuses licensed (Darwin Timetable Files, daily),
   already-built (SFTP receipt, delivery-completeness verification,
   manifest parsing) infrastructure rather than starting a fourth feed
   dependency from zero, and is the only route that honestly supports the
   user's stated "daily" cadence preference.
3. **On the `trust-consumer` consumption side, extend its existing
   in-process reference-reload pattern (`main.rs:40-78`,
   `reference_reload_secs`) with a second periodic HTTP reload for the
   STANOX->CRS table**, rather than giving this currently DB-free crate a
   first-ever direct Postgres dependency, and rather than inventing a new
   file-reload mechanism that doesn't exist anywhere in this codebase
   today. Keep the static CSV file and `--stanox-crs-file` flag as the
   startup-time fallback/default for local dev and any environment
   without the new feed wired up — this doc's recommendation is additive,
   not a removal of the existing safety net.
4. **A pure CORPUS integration (its own `poller-corpus`, independent of
   the schedule feed) is a real, licensed, free option — but rank it
   below option 2, not above it**, specifically because (a) its delivery
   mechanism relative to the already-built DTD/SFTPGo pipe is unconfirmed
   and may require a wholly separate onboarding, and (b) its licensed
   monthly cadence cannot honestly deliver on "daily" freshness regardless
   of implementation quality. Worth revisiting specifically if the
   schedule-feed-derived extraction (option 2) turns out to have gaps
   CORPUS's own purpose-built format would close — a comparison this
   research could not run, since neither pipeline currently parses
   location-code rows out of its raw files.
5. **If cadence must be picked without further work, state it plainly as
   dependent on the source**: daily is realistic and honest only for a
   schedule-feed-derived design (option 2); monthly is the honest ceiling
   for anything sourced directly from NWR CORPUS (option 4). Do not ship
   a design that claims daily freshness against a monthly-licensed
   upstream.

## Open questions (explicit, not resolved here)

1. **Whether NWR CORPUS actually rides the same DTD/SFTPGo delivery
   channel `schedule-ingest` already receives, or a separate
   Network-Rail-hosted mechanism** — the Open Rail Data Wiki's "All
   Reference Data" topic language was never independently verified
   against a live delivery spec in this session (the wiki 403'd on every
   fetch attempt in the prior research this document cites).
2. **`stations.json`'s real upstream provenance and freshness** — it was
   extracted from `train-mcp.zip`'s bundled CORPUS reference data,
   confirmed present and directly read this session, but this session
   did not independently trace `train-mcp.zip` itself back to a dated
   CORPUS extract or confirm when it was generated — it may itself
   already be stale relative to whatever "live" CORPUS delivery would
   provide.
3. **Whether CIF SCHEDULE's `MCA`/`MSN` extraction, if built into
   `schedule-ingest`, would need to run on every daily delivery (cheap,
   incremental) or only on full-refresh deliveries** — `docs/
   superpowers/specs/2026-08-30-schedule-feed-ingress-design.md:265`
   notes at least one file "only exists in a *daily update* delivery, not
   a *full refresh* delivery," a distinction this research did not chase
   down for `MCA`/`MSN` specifically.
4. **The real National Rail status of STANOX `72354`/CRS `ZOC` ("Oxford
   Circus Lt")** found only in `stations.json` — plausibly a light-rail/
   Underground-interchange code bundled into a CORPUS extract rather than
   a genuine additional National Rail booking point; not independently
   confirmed either way this session.

## References

- `crates/trust-consumer/src/stanox_crs.rs` (full module — loader, parser,
  lookup, and test suite)
- `crates/trust-consumer/src/config.rs:89-107` (`--stanox-crs-file` wiring)
  and `crates/trust-consumer/src/main.rs:34-83` (startup load + existing
  reference-reload precedent)
- `crates/trust-consumer/src/process.rs:315-412` (translation call site
  and downstream use)
- `reference-data/stanox-crs.md` (full extraction methodology and
  exclusion policy)
- `docker/trust-consumer.Dockerfile:59` (build-time bake-in)
- `crates/poller-stations/src/main.rs` and `src/config.rs` (existing
  "poll an RDM reference feed into Postgres" precedent, including its
  24-hour default cadence and its Deployment-not-CronJob shape)
- `charts/distant-signal/templates/poller-deployments.yaml` (shared
  poller Deployment template; confirms no `CronJob` exists in this chart)
- `crates/schedule-ingest/src/main.rs` and `src/config.rs` (the
  already-in-flight, SFTP-push-fed, daily-cadence CIF SCHEDULE pipeline)
- `crates/api/migrations/20260901130000_schedule_feed_ingests.sql`
  (confirms today's schedule-feed table stores only a file manifest, not
  parsed reference rows)
- `docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md:14-16`
  (explicit disclaimer that this pipeline does not currently touch
  `trust-consumer`'s STANOX->CRS gap)
- `docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md`
  (RSPS5046 SFTP-pull/push findings, DTD vs. RDM distinction)
- `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md`
  "Claim 3" (200-266) (the prior, already-executed finding that `MCA`/
  `MSN` carry the STANOX<->CRS mapping without a separate CORPUS feed)
- `docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-design.md:197-280`
  (original CORPUS research: what it is, its historical "nightly" claim
  per the Open Rail Data Wiki, and the "three feeds" cost framing this
  document's Claim 3 later revised)
- `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md:72-90`
  (Task 1: the real, signed NWR CORPUS and Darwin Timetable Files RDM
  licences, their cadence, and their distribution terms)
- `stations.json` (repo root; read and parsed directly this session) and
  `reference-data/stanox-crs.csv` (the current, checked-in table) — direct
  comparison performed this session, findings summarized above
- `crates/common/src/lib.rs:403` (`Station.tiploc` — the existing, partial
  consumer of a TIPLOC-shaped field this document's schema-extension
  discussion references)
