# Design: A Schedule-Feed-Derived STANOX->CRS Reference Table

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(closest recent precedent for this repo's design-doc convention) and
directly turns
`docs/superpowers/specs/2026-09-01-stanox-crs-live-reference-data-research.md`
("the research doc")'s headline recommendation into a concrete design. No
code, migration, or parser was written to produce this document — every
claim below is grounded in a file this session actually read, or in a
byte-for-byte inspection of the real `timetable_full.zip` CIF sample this
session performed directly (not merely re-cited from prior research).

## Goal

Replace the current static, hand-regenerated, never-updated
`reference-data/stanox-crs.csv` (loaded once at `trust-consumer` startup,
baked into its Docker image) with a table that refreshes on a real cadence,
sourced from data this app already receives daily — without inventing a
new feed dependency, a new licence, or a new architectural pattern this
codebase doesn't already have a precedent for. Concretely:

1. Design where CIF `MCA`/`MSN` parsing happens, relative to the already-
   shipped, deliberately narrow `crates/schedule-ingest` pipeline.
2. Design the resulting schema, and work out whether the current CSV's
   14-way STANOX ambiguity problem survives the switch (it does — see
   Decision 2).
3. Design how `crates/trust-consumer` picks the new table up, building on
   its own existing `reference_reload_secs` pattern.
4. Confirm the cadence this design can honestly promise.
5. Answer, explicitly, the user's second question: is it worth designing
   full monthly-timetable ingestion (not just STANOX/CRS) now, alongside
   this — see Decision 5.

## Current relevant state (verified this session)

### `schedule-ingest`'s real, narrow scope

`crates/schedule-ingest/src/main.rs:1-11`'s own module doc states its job
precisely: "watches a locally mounted directory for a pushed CIF SCHEDULE
feed delivery from Network Rail/RDG, verifies completeness against the
delivery's own manifest, and forwards completed sequences to the `api`
crate's ingestion endpoint." Read in full this session — `main.rs`,
`manifest.rs`, `scan.rs`, `config.rs` — this holds precisely:

- `manifest.rs:53-89` (`parse`) only extracts a `Sequence:` number and a
  flat list of filenames from `RJTTF<nnn>DAT.txt`. It never opens any of
  the 8 listed files (`ZTR`/`REJ`/`SET`/`FLF`/`MCA`/`MSN`/`ALF`/`TSI`,
  confirmed by the test fixture at `main.rs:490-501`) to read their
  content.
- `scan.rs`'s `StabilityTracker`/`scan_incoming` only ever call
  `std::fs::read_dir`/`metadata` — mtime and byte length, never file
  content.
- `main.rs::run_scan_cycle` (144-296) moves the whole verified-complete
  delivery into `storage_dir/<sequence>/` via `std::fs::rename` (261-267)
  and POSTs only `{sequence, ingested_at, files: [{name, bytes}]}`
  (`ScheduleFeedIngestRequest`, `main.rs:335-346`) to
  `api`'s `/private/schedule-feed-ingests`. `bytes` is the size already
  observed by the stability check, "not a re-stat" (comment at 258-259) —
  reinforcing that this loop treats every file as an opaque blob.
- `crates/api/migrations/20260901130000_schedule_feed_ingests.sql`
  confirms the table this POST lands in stores only `sequence`,
  `ingested_at`, and a `files JSONB` array — no parsed row-level content,
  matching `crates/api/src/data/queries.rs:539-544`'s
  `insert_schedule_feed_ingest` (`ON CONFLICT (sequence) DO NOTHING`, a
  metadata write, not an upsert-by-content).
- `crates/api/src/routes/ingest.rs:185-189`'s own comment draws the
  distinction explicitly: this is "one row per delivery sequence... unlike
  the other ingest routes [which forward] a per-poll-cycle batch of
  reference data."

Adding TI/A-record content parsing to this loop is genuinely new work, not
a small tweak — see Decision 1.

### The real CIF file formats, independently reconfirmed against `timetable_full.zip` this session

`timetable_full.zip` (76MB compressed / 711,352,325B uncompressed, 9
files, `Generated: 28/08/2026` per its own banner lines) sits at this
repo's root (`/workspaces/github-com-fasterspeeding-network-rail-status/timetable_full.zip`
— note: **not** present inside this git worktree, since it's untracked;
read directly from the main checkout's copy this session, streamed via
`unzip -p`, never extracted to disk). This session independently
re-derived, rather than only re-citing, the facts below:

**`RJTTF942MCA.txt` (707,743,886B) record-type tally**, a full pass over
every line (`unzip -p ... | awk '{print substr($0,1,2)}' | sort | uniq
-c`), reproduces
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-inference-timetable-verification.md`'s
own tally exactly:

```
6803900 LI     488798 BS     407636 BX
 407636 LO      407636 LT      97363 CR
  12085 TI        5967 AA          1 HD
      1 ZZ
```

**`TI` records carry STANOX and (mostly) CRS together**, at the byte
offsets `reference-data/stanox-crs.md:66-75` documents (`44..49` STANOX,
`53..56` CRS) — reconfirmed directly against real lines this session:

```
TIEUSTON 00144400NLONDON EUSTON             724102893EUSLONDON EUSTON
TIWATRLMN16559801RLONDON WATERLOO           87212   0
TIVICTRIA00542600PLONDON VICTORIA           87201   0VICLONDON VICTORIA
TIVICTRCR48542662MVICTORIA CARRIAGE ROAD    87201   0XVR
```

`WATRLMN` (the TIPLOC real Waterloo schedules actually use, per the
verification doc's Claim 2) has STANOX `87212` but a **blank** CRS field —
matching that doc's own finding exactly.

**`RJTTF942MSN.txt`'s `A` records fill exactly this gap, and only this
gap** — this session decoded the real byte layout directly (not assumed
from documentation, since the canonical wiki source 403's on fetch, per
`reference-data/stanox-crs.md:77-81`), by cross-referencing 19 real
records where a subsidiary/interchange CRS genuinely differs from the
station's primary CRS (e.g. Glasgow Central Low Level `GCL` vs. parent
`GLC`; Abbey Wood Elizabeth-line platform `ABX` vs. parent `ABW`):

| Bytes | Field |
|---|---|
| `0..1` | Record type, `"A"` |
| `1..5` | spaces |
| `5..35` | Station name (30, space-padded) |
| `35..36` | CATE (interchange status digit) |
| `36..43` | TIPLOC (7, space-padded) |
| `43..46` | Subsidiary CRS (3) |
| `46..49` | spaces (reserved/unused in every one of 3,301 real records checked) |
| `49..52` | **CRS (3) — always populated in every real `A` record checked** |
| `52..57` | Easting (OS grid ref) |
| `57..58` | Estimated-position flag |
| `58..63` | Northing (OS grid ref) |
| `63..65` | Change-date |
| (rest) | trailing spaces, padding every record to a fixed 82 bytes |

3,301 real `A` records were parsed this session (excluding the one
`FILE-SPEC=05` header pseudo-record) — none has a blank primary CRS field,
and **none carries a STANOX field anywhere** (confirmed by inspecting the
full byte layout: only OS Easting/Northing appear where a STANOX might be
expected, matching `reference-data/stanox-crs.md:42-48`'s own earlier
finding that MSN is *not* a STANOX source). `WATRLMN`'s `A` record
(`A    LONDON WATERLOO               3WATRLMNWAT   WAT15312 6179815`)
resolves CRS `WAT` for the exact TIPLOC `TI` left blank.

**Conclusion, directly answering one of the "what to design" questions**:
`MCA` and `MSN` are *both* required for the STANOX/CRS table specifically —
`MCA`'s `TI` records are the *only* source of STANOX in the whole bundle;
`MSN`'s `A` records are needed only to fill the CRS gap `TI` leaves for
some real, passenger-relevant TIPLOCs (Waterloo among them). Neither file
alone is sufficient. This is a narrow, bounded read of two record types
(`TI`, 12,085 lines; `A`, 3,302 lines) — 15,387 lines total, 0.18% of
`MCA`'s 8,631,021 lines — not a parse of the file's substantive content
(`BS`/`BX`/`LO`/`LI`/`LT`/`CR`, the 8,610,939 lines that describe actual
train schedules). See Decision 5 for why that distinction matters.

**Every daily delivery is a full refresh, not an incremental delta** —
confirmed by `docs/superpowers/specs/2026-08-30-schedule-feed-ingress-design.md:258-275`,
independently re-read this session: `MCA` is documented and confirmed as
"Full CIF refresh file containing all timetable details," and `MSN` is
"always a full-refresh-only file" — this app's licensed product is
literally named "Timetable - Full Refresh - Daily," not an update-only
product (the `RJTTCnnn.CFA` incremental-update file that would carry
partial data doesn't exist in this delivery shape at all). **This resolves
the research doc's own Open Question 3** ("whether MCA/MSN extraction...
would need to run on every daily delivery... or only on full-refresh
deliveries") definitively: every delivery is a full refresh, so every
delivery's `TI`/`A` extraction is a complete, standalone, from-scratch
snapshot — never a merge against a prior day's partial state.

### STANOX ambiguity is a property of the raw data, not an artifact of the CSV or of `stations.json`

This session parsed every real `TI` line in `RJTTF942MCA.txt` directly
(a streamed Python pass, `awk`-equivalent) and independently reproduced
the *exact same* 14 ambiguous STANOX values `reference-data/stanox-crs.md:96-113`
already documents by hand, with the identical 5-unresolvable/9-resolvable
split:

- **941 `TI` records have no STANOX** (blank or `"00000"`) — matches
  `stanox-crs.md:92` exactly.
- **3,129 distinct STANOX values have at least one CRS** — matches
  `stanox-crs.md:96` exactly.
- **14 STANOX map to more than one CRS**, this session's own direct parse:
  `30120` (PRE/XPU), `31510` (MCV/XVS), `40320` (CTR/XCZ), `52215`
  (SDI/SFA), `86441` (BOG/XBN), `86935` (PFT/POO), `86981` (WEY/XWJ),
  `87201` (VIC/XVR), `87219` (CLJ/XCP), `87261` (WIM/XWD), `87981`
  (XBP/XMP), `88486` (SAY/XSQ), `89428` (AFK/ASI), `89530` (EBD/EBF).
  Applying the CSV's own documented "prefer the sole non-`X`-prefixed
  candidate" policy resolves exactly the same 9
  (`BOG`,`CLJ`,`CTR`,`MCV`,`PRE`,`SAY`,`VIC`,`WEY`,`WIM`) and leaves
  exactly the same 5 genuinely irresolvable
  (`52215`,`86935`,`87981`,`89428`,`89530`) that
  `reference-data/stanox-crs.md:107-113` names — the resulting resolved
  count is `3129 - 14 + 9 = 3124`, matching the current CSV's own row
  count exactly.

This directly answers a question this design must not hand-wave: **moving
to a schedule-feed-derived source does not make the ambiguity go away**.
It's a structural fact about the underlying CIF data (one physical STANOX
area genuinely serving multiple TIPLOCs/CRS, per `stanox-crs.md:99-104`'s
own explanation), reproduced identically whether read from the hand-curated
CSV, from `stations.json` (per the research doc's own finding, cited
below), or read fresh from `MCA` itself, as this session just did. The
disambiguation policy has to be **reimplemented as real parsing logic** in
whatever produces the live table — see Decision 2.

### `trust-consumer`'s current load-once architecture

`crates/trust-consumer/src/stanox_crs.rs` (read in full): `StanoxCrsTable`
is a plain `HashMap<String, String>`, built by `from_file`/`parse`
(66-132), looked up via `stanox_to_crs` (143-153, with zero-pad retry for
short input). `crates/trust-consumer/src/config.rs:99-107`'s
`--stanox-crs-file`/`STANOX_CRS_FILE` flag uses a `clap` `value_parser`
that calls `StanoxCrsTable::from_file` **at CLI-parse time** — the table
is embedded directly as a field of the immutable `Config` struct built once
in `main` (`crates/trust-consumer/src/main.rs:34`,
`let config = Config::parse();`). There is no code path anywhere in this
crate that re-reads any STANOX source after startup today.

`trust-consumer` **already has** a working periodic-reload mechanism for a
different piece of state, directly reusable as a pattern:
`main.rs:40-78` maintains `reload_interval`/`last_reference_reload`
(`Duration::from_secs(config.reference_reload_secs)`, default `60`, per
`config.rs:68-72`) and, once per tick, calls
`queries::fetch_active_tracked_trains` over HTTP against `api`, then
atomically swaps the result into `reference`/`state` via
`process::apply_reference_reload` (`main.rs:64`) — logging and continuing
on failure (`main.rs:74-77`), never crashing the loop. This crate remains
entirely DB-free today (confirmed: no `sqlx`/`postgres`/`PgPool` anywhere
in `crates/trust-consumer`) — it talks only to Kafka and to `api` over
HTTP.

### The existing `poller-* -> /private/X -> Postgres` and single-Pod multi-container precedents

`crates/poller-stations/src/main.rs` (read in full): a `tokio::time::
interval` loop, `common::ingest::time_until_next_poll` for restart-safe
delay, `common::ingest::post_batch` (`common/src/ingest.rs:35`) to POST a
parsed `Vec<T>` to `api`'s `/private/stations`, matching
`crates/api/src/routes/ingest.rs:28-50`'s consistent
"`GET` last-fetched + `POST` batch" route-pair shape (documented in that
file's own module doc, lines 1-15) and its upsert-by-primary-key query
convention (e.g. `insert_schedule_feed_ingest`'s `ON CONFLICT ... DO
NOTHING`/`DO UPDATE` pattern at `queries.rs:539-559`).
`charts/distant-signal/templates/poller-deployments.yaml` confirms **no
Kubernetes `CronJob` exists anywhere in this chart** — every recurring job
is a long-lived Deployment with an in-process timer.

`charts/distant-signal/templates/schedulefeed-deployment.yaml:1-8`'s own
comment is directly load-bearing for this design's Decision 1: "The first
MULTI-CONTAINER Deployment in this chart... pairs an SFTP receiver (the
`sftp` container...) with this app's own verifier/uploader (the `ingest`
container, `crates/schedule-ingest`) sharing one PVC." Two containers,
one Pod, one shared `data` volume (`volumeMounts` at lines 164-166 and
225-227, same `mountPath: /data/schedule-feed` in both containers) — this
pattern is already proven, already deployed, and already survived a real
incident (the `fsGroup`/`chown` permission-denied fix visible in this
repo's own recent commit history, `b5cbdfe`).

## Decisions

### 1. Where parsing happens: a new sibling container in the existing multi-container schedule-feed Pod — not inline in `schedule-ingest`'s own loop, and not a file upload to `api`

Three real shapes were weighed:

**(a) Inline in `schedule-ingest`'s own `run_scan_cycle`.** Rejected.
`schedule-ingest`'s own module doc and its design doc
(`docs/superpowers/specs/2026-09-01-schedule-feed-push-design.md:14-16`,
cited directly by the research doc) go out of their way to state this
crate's scope narrowly and to explicitly disclaim touching
`trust-consumer`'s STANOX/CRS gap. That's a real, already-made boundary
decision, not an oversight to route around. Concretely, adding TI/A
parsing here means: (1) a genuinely new capability class for this crate —
domain-specific CIF-content interpretation and a disambiguation policy,
which is a different kind of work from RSPS5046 delivery-completeness
verification; (2) a multi-second-to-tens-of-seconds full-file byte scan
(707MB — 5.8s wall time for a full record-type tally, per the verification
doc's own `awk` benchmark, independently plausible from this session's
equivalent scans) glued onto a loop whose current `run_scan_cycle` is fast
metadata/rename work, coupling an unrelated latency profile onto the
check-time-gated scan cadence; (3) turning this crate's current "one
record per delivery" POST shape (`ingest.rs:185-189`'s own comment
distinguishing it from every other poller) into a second, structurally
different "batch of ~3,100+ parsed rows" POST shape in the same binary.
None of this is impossible, but it works directly against a boundary this
crate's own authors already drew on purpose.

**(b) `schedule-ingest` uploads raw `MCA`/`MSN` file content to `api` for
parsing there.** Rejected outright. This would mean streaming up to 707MB
over HTTP into `api`, which has no precedent anywhere in this codebase —
every existing ingest route is a small JSON body (`ingest.rs`'s own module
doc: "Each poller POSTs a `Vec<T>` snapshot"). It also contradicts the
already-established "files live on the shared PVC, only metadata crosses
HTTP" design `schedule-ingest`/`schedule-sftp` already embody.

**(c) A new sibling container in the same Pod, sharing the same PVC
read-only, following the `poller-* -> /private/X -> Postgres` shape on
its output side.** **Chosen.** This is a third container alongside the
already-precedented `sftp`/`ingest` pair in
`schedulefeed-deployment.yaml` — not a new Deployment, not a new PVC. It
mounts the same `data` volume at `/data/schedule-feed` (read-only —
`readOnly: true` on its `volumeMount`, a pattern already used elsewhere in
this chart) and, once `schedule-ingest`'s own `ingest` container has moved
a verified-complete delivery into `storage_dir/<sequence>/`, reads
`RJTTF<sequence>MCA.txt` and `RJTTF<sequence>MSN.txt` directly off that
already-local disk — no network transfer of the 707MB file at all, since
it's already sitting on the volume this container already mounts. It
extracts only `TI` and `A` lines (a streamed, prefix-filtered read — see
Current relevant state above on why this is a bounded 15,387-line
extraction, not a full-file parse), applies the disambiguation policy in
code (Decision 2), and `POST`s the resolved rows to a new
`/private/stanox-crs` endpoint on `api` via `common::ingest::post_batch`,
matching `poller-stations`' exact shape.

Naming: a new crate, e.g. `crates/schedule-reference` (working name; not
`poller-corpus`, since it is not CORPUS — the research doc's own
recommendation #1 already rejects CORPUS as the source). This keeps this
repo's consistent "one crate, one concern" pattern (every poller is its
own crate; `schedule-ingest` itself is a separate crate from `api` despite
sharing a Pod with a third-party image) rather than growing
`schedule-ingest`'s own binary a second, unrelated responsibility.

**How it learns a new sequence has landed, without a new `api` write
surface**: `GET /private/schedule-feed-ingests` today returns only
`{fetched_at}` (`LastFetchedResponse`,
`queries.rs:531-537`/`last_schedule_feed_fetch`) — no sequence number, the
same gap `schedule-ingest`'s own `main.rs:13-32` module doc already
names and works around by tracking `last_ingested_sequence` **in-memory
only**, treating a restart as an acceptable, honestly-scoped limitation
rather than a hard requirement to fix first. This design reuses exactly
that same posture: the new crate scans `storage_dir`'s immediate numeric
subdirectories itself (the same technique `schedule-ingest`'s own
`prune_old_sequences`, `main.rs:445-484`, already uses to find the highest
sequence — a new, small, independently-written function, not shared code
across the crate boundary, consistent with this repo not sharing binary
internals beyond the `common` crate), picks the highest one that contains
both an `MCA` and `MSN` file, and reprocesses it if it differs from the
last sequence *this process* has already parsed (in-memory, reset on
restart). This is actually a **more benign** version of
`schedule-ingest`'s own equivalent gap: because the `api`-side write is an
upsert keyed by `stanox` (Decision 2), a restart-induced reprocess of an
already-seen sequence is a harmless no-op, not a false gap-detection log
line — there is no data-loss risk analogous to `schedule-ingest`'s own
`Gap` classification concern.

### 2. Schema, and the disambiguation policy as real code

New Postgres table `stanox_crs`:

| Column | Type | Notes |
|---|---|---|
| `stanox` | `text primary key` | 5-digit, zero-padded, matches the current CSV's shape |
| `crs` | `text not null` | 3-letter, uppercase |
| `tiploc` | `text` | The TIPLOC the resolved `(stanox, crs)` pair came from. Not present in today's CSV, but explicitly anticipated by `stanox_crs.rs`'s own module doc (lines 42-51: "adding a future column (e.g. `tiploc`) means adding one more named lookup and one more field") and by the research doc's own finding that `common::Station.tiploc` already has a real, if partial, consumer (`crates/common/src/lib.rs:403`) |
| `station_name` | `text` | The `TI`/`A` record's own name field, for human debugging/auditing of the live table (the CSV has no equivalent today, since it's small enough to cross-reference by hand) |
| `source_sequence` | `integer` | Which `schedule_feed_ingests.sequence` this row was last (re)derived from — new, since a live source benefits from provenance the static file never needed |
| `updated_at` | `timestamptz not null` | When this row was last (re)written |

**Ingestion semantics: replace-on-write, not append**. Since every daily
delivery is a full refresh (Current relevant state, above), each
successful parse produces the *complete* current STANOX/CRS table, not a
delta. The new crate's `POST /private/stanox-crs` upserts by `stanox`
(`ON CONFLICT (stanox) DO UPDATE`, matching this repo's own established
upsert convention) — this naturally self-heals a STANOX whose CRS
assignment changed between deliveries (a renamed/decommissioned TIPLOC,
the exact staleness risk the research doc flagged the static file as
having zero protection against) and requires no separate "delete rows
absent from today's delivery" step, since every delivery re-asserts every
row it still resolves.

**Disambiguation policy — reimplemented as code, not inherited for free**:
the parsing crate groups `TI` records by `stanox`, and for each STANOX with
more than one distinct `crs`:
- If exactly one candidate CRS is not `X`-prefixed and the other(s) are,
  keep the non-`X` one — the exact policy
  `reference-data/stanox-crs.md:104-106` documents by hand, reimplemented
  as a real conditional in this new crate rather than a human judgment
  call.
- Otherwise (two-plus genuine non-`X` candidates, or two-plus `X`-prefixed
  candidates, with no principled tiebreaker) — exclude the STANOX entirely
  from the table, exactly matching the current CSV's own treatment of the
  5 genuinely irresolvable cases. A caller-side miss for one of these five
  is indistinguishable from any other unmapped STANOX, matching
  `stanox_crs.rs`'s own documented "a lookup miss is the honest 'we don't
  know' case" posture (module doc, lines 27-35).
- A `TI` record's blank CRS is filled from `MSN`'s `A` record for the same
  TIPLOC before this grouping runs (per the WATRLMN case above) — so the
  ambiguity check operates on the CRS-completed set, matching the current
  hand-curated table's own effective behavior (its provenance doc treats
  MSN-sourced completion as part of the same extraction pass, not a
  separate correction step).

This policy needs its own unit tests against fixture `TI`/`A` lines
mirroring the real ones this session decoded (see Testing).

**Relationship to `reference-data/stanox-crs.csv`**: additive, not a
replacement of the file itself — see Decision 3 for the consumption-side
transition. This design does not propose deleting the CSV or its loader
code.

### 3. Consumption in `trust-consumer`: a second periodic HTTP reload, mirroring the existing `reference_reload_secs` pattern; the CSV stays as the startup fallback

Per the research doc's own recommendation (#3) and directly extending the
already-proven pattern at `main.rs:40-78`:

- `Config` gains a new `stanox_crs_reload_secs` field (its own default,
  not reusing `reference_reload_secs`'s `60` — see Decision 4 on why a
  much coarser interval is the honest choice here) and, separately, keeps
  the existing `--stanox-crs-file`/`STANOX_CRS_FILE` startup load
  **completely unchanged** — this remains the value used until the first
  successful live reload, and the value silently kept if the live source
  is ever empty or unreachable (e.g. local dev with no `schedule-feed`
  pipeline deployed at all).
- The table itself moves from a plain field embedded once in the immutable
  `Config` (today's shape) to a shared, swappable cell (e.g. `Arc<ArcSwap<
  StanoxCrsTable>>` or an `Arc<tokio::sync::RwLock<StanoxCrsTable>>`) that
  `process::run_once`/`run_cycle` read through instead of a bare
  `&StanoxCrsTable` — `main.rs:130`'s existing `stanox_crs: &stanox_crs::
  StanoxCrsTable` parameter becomes a reference into this cell (or a
  cheap cloned snapshot read once per cycle, matching how `reference`/
  `state` are already handled as mutable loop-owned values swapped by
  `apply_reference_reload`).
- The main loop's existing reload block (`main.rs:52-78`) gains a sibling
  block, same shape: on `stanox_crs_reload_interval.elapsed() >=
  stanox_crs_reload_secs`, `GET /private/stanox-crs`, parse the response
  into a fresh `StanoxCrsTable`, and swap it into the cell — logging and
  continuing on failure (`tracing::error!(...); // retrying next cycle`),
  never crashing the loop, exactly matching the existing tracked-trains
  reload's resilience posture.
- **Transition, concretely**: on a fresh deployment with the new
  `schedule-reference` crate not yet running (or `/private/stanox-crs`
  returning an empty table), `trust-consumer` behaves exactly as it does
  today — the CSV-derived table, loaded once at startup, is all it ever
  has. Once the new crate's first successful parse lands, the next
  `stanox_crs_reload_secs` tick swaps in the live table, and the CSV-
  derived one is only ever consulted again if a later reload fails and
  there is no previously-fetched live table yet (an edge case: startup
  race between `trust-consumer` and the new crate's first successful
  parse — resolved by the CSV fallback, not by blocking startup on the
  new crate).
- The CSV file, its loader (`stanox_crs.rs::from_file`), and
  `reference-data/stanox-crs.md`'s provenance documentation are **kept, not
  deleted** — same explicit posture the research doc's own recommendation
  #3 states ("this doc's recommendation is additive, not a removal of the
  existing safety net"). It remains: the committed, reviewable-in-a-diff
  default for local dev and any environment without the schedule-feed
  pipeline deployed; the last-known-good fallback if the live table is
  ever corrupted or unreachable at startup; and a human-readable snapshot
  useful for spot-checking the live table's output against, at least
  informally. Whether to periodically regenerate the checked-in CSV from
  the live table (an occasional manual audit, not automated) is left open
  — not designed here, not required.

### 4. Cadence: daily, honestly — matching the licensed product, not an arbitrarily tighter poll

The research doc already established the licensing distinction and this
session re-read it directly: **NWR CORPUS is licensed at monthly cadence;
Darwin Timetable Files (the CIF SCHEDULE product `schedule-ingest` already
ingests) is licensed at daily cadence**
(`docs/superpowers/specs/2026-09-01-stanox-crs-live-reference-data-research.md:429-451`,
citing the real, signed RDM licences this app already holds). This
design's whole premise — deriving the table from `MCA`/`MSN`, not from a
separate CORPUS feed — is precisely what lets "daily" be an honest claim
rather than oversold against a monthly upstream, restated here as the
concrete conclusion the research doc's cadence analysis was building
toward.

Two distinct intervals, deliberately not conflated:

- **How often the new `schedule-reference` crate checks for a new
  sequence**: independent of the *underlying* daily cadence — matching the
  research doc's own observation that `schedule-ingest`'s check-time list
  and `poller-stations`' flat interval are both valid daily-cadence shapes
  picked to match their own delivery mechanism, not copied wholesale.
  Since this crate reads an already-local, already-verified-complete
  sequence directory (no network fetch of its own), a moderately frequent
  interval (candidate: every 30-60 minutes, matching `poller-stations`'
  own order of magnitude for "cheap to check, expensive-ish to skip") is
  reasonable — most checks find nothing new, since a fresh sequence only
  lands roughly once a day, but an idle check costs one directory listing.
- **`trust-consumer`'s `stanox_crs_reload_secs`**: also need not be daily
  itself — it can check more often than the data actually changes (e.g.
  hourly) without materially changing behavior, the same way
  `reference_reload_secs`'s existing `60`-second default is far tighter
  than tracked-train pins are created in practice, deliberately, so a
  newly-created pin is picked up promptly. For STANOX/CRS, "promptly" only
  matters relative to a roughly-daily-changing upstream, so a coarser
  default than `60` is the honest choice — exact figure flagged as an
  implementation-time, unresearched starting point (Open questions),
  matching this codebase's own established posture for figures like this
  (`MINE_LIST_LIMIT`, `MAX_PIN_AGE`, per the tracked-trains-home-page
  spec's own precedent for flagging unresearched constants honestly rather
  than pretending a chosen number is load-bearing).

### 5. The full monthly-timetable question: narrow scope now; full-timetable ingestion is real, named, and substantially larger — not designed here

The user asked directly whether ingesting the *full* monthly timetable
(not just STANOX/CRS reference data, but `MCA`'s actual schedule/service
records) is worth designing now, alongside or instead of this narrower
scope. Answered concretely, not deferred by default:

**What the narrow design (this document) touches**: `TI` (12,085 lines)
and `A` (3,302 lines) records only — 0.18% of `MCA`'s 8,631,021 total
lines. These are pure location-*reference* data: "what CRS does STANOX X
resolve to." They say nothing about which trains run, when, or between
where.

**What full-timetable ingestion would actually require**, grounded in
this session's own re-confirmed record counts and in
`docs/superpowers/specs/2026-09-01-train-mcp-integration-research.md`
(read in full this session):

- Parsing `BS`/`BX`/`LO`/`LI`/`CR`/`LT` — 8,610,939 of `MCA`'s 8,631,021
  lines, three orders of magnitude more than this design's `TI`+`A` scope.
- **STP overlay resolution** (`C`/`N`/`O`/`P` precedence per calendar
  date) — confirmed real and structurally significant this session
  (81,162 `C`-indicator cancellation-only records with no body at all,
  reconfirmed against the exact tally above) — a genuinely stateful
  algorithm, not a lookup.
- A materially larger schema: on the order of 400,000+ schedule rows and
  6.8 million calling-point rows nationwide, even before any per-day
  filtering — versus this design's ~3,100-row `stanox_crs` table.
- If the goal is "what runs between A and B" (`find_services`/
  `plan_journey`-shaped functionality), a journey-planning algorithm
  (RAPTOR or Connection Scan) on top of the parsed schedule data. This app
  has **zero** existing code toward any of this: the train-mcp research
  doc's own grep-verified finding states plainly, "There is no
  `schedules`/`calling_points` table, no TIPLOC-to-CRS timetable join,
  nothing queryable by 'what runs between A and B'"
  (`2026-09-01-train-mcp-integration-research.md:285-286`).
- **Scale of the investment**: that same research doc characterizes the
  equivalent, already-built system (train-mcp's own CIF timetable store +
  RAPTOR/CSA planner — "26,848 schedules, 316,362 calling points, ~289,514
  connections on a measured weekday," from a ~600MB CIF extract) as
  described in train-mcp's *own* design doc as "roughly a month of work"
  (`2026-09-01-train-mcp-integration-research.md:256-268`), and concludes
  the gap for Distant Signal to build the equivalent is "substantively the
  same engineering investment [as] train-mcp's own Phase 2a/2b already
  represent, done once already in a different language, for a different
  codebase" (`train-mcp-integration-research.md:296-299`). This is not
  this design's own estimate — it's a directly-transferable, independently
  arrived-at sizing from a closely related piece of this session's prior
  research, cited rather than re-derived.

**Recommendation: narrow scope now (this document), full-timetable
ingestion named explicitly as a separate, much larger follow-on — not
bundled with this pass.** The two are genuinely separable pieces of the
same underlying files (reference data vs. schedule data), and nothing
about building the STANOX/CRS table forecloses a future full-timetable
project. If that project is ever pursued, this design's new
`schedule-reference` crate (which already streams `MCA` off the shared PVC
on every new delivery) is architecturally the natural place to extend
from — but that's a future decision, not something this design commits to
or scopes further. A full-timetable design, if pursued, needs its own
dedicated design pass (schema, STP-resolution approach, planner choice,
whether to mirror train-mcp's own SQLite-per-process model or this app's
Postgres-backed convention) — not sketched here beyond naming it.

## Architecture

```
 Darwin Timetable Files (DTD), daily full-refresh push, via SFTP
        │
        ▼
 ┌───────────────────────────── schedulefeed Pod (existing + new) ─────────────────────────┐
 │                                                                                            │
 │  ┌────────────┐        ┌──────────────────┐        ┌───────────────────────────────┐    │
 │  │   sftp     │  PVC   │      ingest       │  PVC   │   schedule-reference (NEW)      │    │
 │  │ container  │◄──────►│  container         │◄──────►│   container                    │    │
 │  │ (SFTPGo)   │  "data"│  crates/           │ "data" │   crates/schedule-reference      │    │
 │  │            │ volume │  schedule-ingest   │  volume│   (read-only mount)             │    │
 │  └────────────┘        │                    │        │                                  │    │
 │                         │ watch_dir ->       │        │ scans storage_dir's numeric      │    │
 │                         │ verify manifest -> │        │ subdirs for the highest complete  │    │
 │                         │ move to storage_dir│        │ sequence (MCA+MSN present),       │    │
 │                         │ /<sequence>/       │        │ reprocesses if new vs. last        │    │
 │                         └─────────┬──────────┘        │ in-memory-seen sequence            │    │
 │                                   │ POST                │                                  │    │
 │                                   │ {sequence,           │ streams RJTTF<n>MCA.txt for TI    │    │
 │                                   │  ingested_at,files}  │ lines (STANOX+CRS+TIPLOC),        │    │
 │                                   ▼                      │ RJTTF<n>MSN.txt for A lines        │    │
 │                    ┌───────────────────────────┐         │ (CRS completion for blank-CRS TI),  │    │
 │                    │ POST /private/             │         │ applies disambiguation policy       │    │
 │                    │ schedule-feed-ingests      │         │ in code                             │    │
 │                    └─────────────┬─────────────┘         └───────────────┬───────────────────┘    │
 └──────────────────────────────────┼───────────────────────────────────────┼────────────────────────┘
                                     │                                        │ POST /private/stanox-crs
                                     ▼                                        ▼ (batch upsert by stanox,
                     ┌───────────────────────────────────────────────────────┴──────┐  common::ingest::post_batch)
                     │                      api (Postgres)                          │
                     │  schedule_feed_ingests (sequence, ingested_at, files JSONB)   │
                     │  stanox_crs (stanox PK, crs, tiploc, station_name,            │
                     │              source_sequence, updated_at)                    │
                     └───────────────────────────────┬───────────────────────────────┘
                                                       │ GET /private/stanox-crs
                                                       ▼
                                        ┌───────────────────────────────────┐
                                        │            trust-consumer          │
                                        │  startup: --stanox-crs-file CSV    │
                                        │           (unchanged fallback)     │
                                        │  loop: existing reference_reload   │
                                        │        block (tracked trains) +    │
                                        │        NEW stanox_crs_reload block │
                                        │        (swap Arc<ArcSwap<Table>>)  │
                                        └───────────────────────────────────┘
```

## Error handling

- **`schedule-reference` finds a sequence directory missing one of
  `MCA`/`MSN`** (shouldn't happen given every delivery is a documented
  full refresh, per Current relevant state — but defensively): skip that
  sequence, log at `ERROR`, retry on the next check — mirroring
  `schedule-ingest`'s own "log and retry next cycle, never crash the
  process" posture for a bad manifest parse (`main.rs:211-217`).
- **A `TI`/`A` line fails to parse at the expected byte offsets** (a
  malformed or unexpectedly-short line): skip that single record, count it
  in a metric, do not fail the whole sequence's extraction — a partial,
  mostly-correct table beats no table, matching this design's overall
  "additive, fail open to the existing fallback" posture.
- **The disambiguation policy encounters a STANOX ambiguity shape it
  doesn't recognize** (e.g. three-plus distinct CRS candidates for one
  STANOX, never seen in the real 2026-08-28 sample but not provably
  impossible in a future delivery): treat as irresolvable, exclude the
  STANOX, log at `WARN` with the full candidate list — never guess.
- **`POST /private/stanox-crs` fails** (network error, `api` down): log
  and retry on the next check-time-equivalent tick, exactly matching
  `schedule-ingest`'s own `pending_post` retry pattern
  (`main.rs:162-174`) in spirit, though — unlike that pattern — no local
  file-move needs to be undone or retried here, since this crate never
  moves anything; a failed POST just means the already-computed in-memory
  table is discarded and rebuilt from the same on-disk files next cycle
  (cheap, since the files are still local and unchanged).
- **`trust-consumer`'s live reload fails** (network error, `api` down, or
  an empty/malformed response): log and keep serving whatever table is
  currently loaded (CSV-derived, or a previously-fetched live one) —
  identical resilience posture to the existing tracked-trains reload
  (`main.rs:74-77`). Never block the Kafka-consuming main loop on this.
- **`api`'s `stanox_crs` table is empty** (fresh environment, or the new
  crate has never successfully run): `GET /private/stanox-crs` returns an
  empty list; `trust-consumer` treats this the same as a fetch failure —
  keep the current (startup CSV, or last-known-good live) table rather
  than swapping in an empty one that would silently stop translating every
  STANOX.

## Testing

- **`schedule-reference` crate**: unit tests against real, hand-copied
  `TI`/`A` line fixtures mirroring the exact byte-verbatim lines this
  session decoded (Euston, WATRLMN's blank-CRS case, the Victoria/XVR
  ambiguous-but-resolved case, and at least one of the 5 genuinely
  irresolvable STANOX) — following `stanox_crs.rs`'s own existing
  precedent of quoting real bytes rather than synthetic fixtures
  (`stanox_crs.rs:185-200`'s `REAL_EUSTON`/`REAL_VICTORIA`/etc. constants).
  Assert: STANOX+CRS extraction from `TI`; CRS-completion from `MSN`'s `A`
  record for a blank-`TI`-CRS TIPLOC; the disambiguation policy resolving
  the 9 non-`X`-preferable cases and excluding the 5 irresolvable ones,
  using the exact 14 STANOX values this session's own scan found (listed
  in Current relevant state) as the fixture set, so the test doubles as a
  regression guard if a future CIF extract's ambiguity set ever differs.
- Sequence-selection logic (highest numeric subdirectory with both files
  present, reprocess-if-new-since-last-seen): unit tests mirroring
  `schedule-ingest/src/main.rs`'s own `prune_keeps_only_the_n_highest_
  numeric_subdirectories`-style tests (temp-dir-based, no real PVC needed).
- **`api`**: a test for `/private/stanox-crs`'s `POST` (batch upsert by
  `stanox`, confirming a re-POST with a changed `crs` for an existing
  `stanox` overwrites rather than errors or duplicates) and `GET` (returns
  the current full table), following the existing pattern for every other
  `/private/X` route pair in `ingest.rs`.
- **`trust-consumer`**: a test asserting the CSV-derived table is used
  when the live reload has never succeeded (startup, or every reload
  attempt has failed) and a test asserting a successful live reload
  replaces it for subsequent lookups — mirroring the existing
  `apply_reference_reload` test coverage's shape for the tracked-trains
  reload, applied to the new `stanox_crs` swap. A regression test
  confirming a *failed* live reload does not clear the currently-loaded
  table (guards the "fail open, never silently go blank" requirement in
  Error handling).

## Explicitly out of scope

- **Full monthly-timetable ingestion** (`BS`/`BX`/`LO`/`LI`/`CR`/`LT`
  parsing, STP resolution, a schedules/calling-points schema, a
  RAPTOR/CSA journey planner). Weighed and explicitly deferred in
  Decision 5 — named as a real, separate, comparably-large-to-train-mcp's-
  own-build follow-on, not designed further here.
- **CORPUS as an independent feed** (`poller-corpus`). Already
  investigated and ranked below this design's approach by the research
  doc (recommendation #4) — its delivery mechanism relative to the
  existing DTD/SFTPGo pipe is unconfirmed, and its licensed monthly
  cadence cannot honestly support "daily." Not revisited here; would only
  be worth reconsidering if this design's own extraction later turns out
  to have real gaps CORPUS's purpose-built format would close.
- **Deleting `reference-data/stanox-crs.csv` or its loader.** Decision 3
  keeps both as the startup fallback, indefinitely, not as a
  time-limited migration step.
- **Automating regeneration of the checked-in CSV from the live table.**
  Left as an optional, manual, occasional audit practice — not designed or
  required.
- **`stations.json`/`train-mcp.zip`-sourced CORPUS data as an input to
  this design.** The research doc's own direct comparison already found
  `stations.json` reproduces the same 14-way ambiguity without resolving
  it and offers only 3 genuinely new unambiguous rows over the current
  CSV — not revisited or reused here.
- **Any frontend or public API surface change.** `stanox_crs` is
  exclusively a `trust-consumer`-internal translation table, reached only
  via `/private/stanox-crs`; nothing in this design proposes exposing it
  publicly (unlike, e.g., `/public/freshness`, which this design does not
  propose extending either, though that would be a small, natural future
  addition — see Open questions).

## Open questions / risks

1. **`stanox_crs_reload_secs`'s and the new crate's own poll interval are
   both unresearched starting figures** (candidates named in Decision 4:
   hourly-ish and 30-60-minute-ish respectively), same posture this
   codebase already takes for `MINE_LIST_LIMIT`/`MAX_PIN_AGE`-shaped
   constants — worth revisiting once this is running against real
   deliveries.
2. **The exact swap mechanism for `Config.stanox_crs`** (`ArcSwap` vs.
   `tokio::sync::RwLock` vs. some other interior-mutability shape) is
   named but not settled — an implementation-time choice within
   Decision 3's stated constraint (DB-free, atomic swap, no blocking of
   the Kafka-consuming main loop), not decided here.
3. **Whether `/public/freshness` should gain a `stanox_crs` field**,
   mirroring its existing `schedule_feed` field
   (`crates/api/src/routes/freshness.rs:35-38`) — a small, natural
   addition once this table exists, but not required for this design's
   core goal and not designed further here.
4. **The 14-ambiguity/3,124-row figures are specific to the
   2026-08-28 extract** this session inspected. A future delivery could
   plausibly surface a different ambiguity count (a renamed TIPLOC, a
   newly-shared STANOX) — the disambiguation *policy* is designed to
   handle this generically (Decision 2), but the specific fixture-based
   regression tests (Testing, above) will need occasional updates if a
   real future delivery's ambiguity set genuinely differs, which is an
   expected maintenance cost of a live source, not a design flaw.
5. **This design does not resolve `schedule-ingest`'s own pre-existing
   "no persistent last-ingested-sequence" gap** (`main.rs:13-32`) — it
   deliberately reuses the same class of in-memory-only tracking for the
   new crate rather than fixing the underlying gap for both. If that gap
   is ever closed (e.g. `schedule_feed_ingests` gains a queryable "latest
   sequence" endpoint), the new crate could simplify to use it instead of
   its own filesystem scan — noted as a natural simplification, not
   required.
6. **Resource/security-context sizing for the new third container**
   (memory needed to hold ~3,100 in-flight rows plus a streaming read
   buffer — small; CPU for a single 707MB streamed pass — the existing
   `ingest` container's own `resources` block is a reasonable starting
   point to mirror) is not sized here; left to implementation.
