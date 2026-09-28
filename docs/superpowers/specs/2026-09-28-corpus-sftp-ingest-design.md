# CORPUS ingest over the existing SFTP push — design

Status: implemented on branch `wt-corpus-sftp-ingest` (off by default).
Date: 2026-09-28.

## Context

The user asked for Network Rail's CORPUS location reference data to come in
the same way as the CIF timetable: RDM pushes a file to our SFTP server. The
user said "it would be easiest to reuse the existing SFTP server to ingest this
data as well". The provider has been asked to start sending it to the SFTP
server that is already live, so files may arrive before this change deploys.

The provider sends two files, both named `CORPUSExtract`:

- **`CORPUSExtract.json.gz`**: real CORPUS. This is gzipped JSON of the form
  `{"TIPLOCDATA":[{NLC, STANOX, TIPLOC, 3ALPHA, UIC, NLCDESC, NLCDESC16}…]}`.
  The sample has 55,972 rows and is about 7 MB uncompressed. An absent value
  is a single space. `NLC` is a JSON number. Other extracts have carried
  `NLC` and `STANOX` as either numbers or strings; see
  `crates/line-catalogue-validator/src/regenerate.rs`.
- **`CORPUSExtract.csv.gz`**: this is **SMART berth data**, not CORPUS, and is
  deliberately ignored (user decision, 2026-09-28).

CORPUS is licensed as the RDM product "NWR CORPUS", with a **monthly** update
cadence (see `2026-09-03-schedule-feed-cadence-research.md`).

## How the existing SFTP push works

The sources are `charts/distant-signal/templates/schedulefeed-*.yaml`,
`crates/schedule-ingest`, and `2026-09-01-schedule-feed-push-design.md`.

- **Chart.** `scheduleFeed.enabled` (default false) renders one
  `schedulefeed` Deployment (Recreate, 1 replica) with three containers on
  one PVC mounted at `/data/schedule-feed`:
  - **`sftp`** (SFTPGo) listens on port 2022, exposed by a LoadBalancer
    Service. `scheduleFeed.sftp.allowedCidrs` feeds
    `loadBalancerSourceRanges` and the NetworkPolicy.
  - The `sftp` container has a single SFTP user, `scheduleFeed.sftp.username`
    (default `dtd-push`). That user authenticates by password or by public
    key, taken from a Secret. The user is created at startup by the
    entrypoint ConfigMap through `--loaddata-from`. Its home directory is
    `/data/schedule-feed/<destinationFolder>[/<folderPath>]` (default
    `incoming`) with `"/": ["*"]` permissions.
  - **`ingest`** (`crates/schedule-ingest`) scans that folder
    (`WATCH_DIR`) every `pollIntervalSecs` (120 s). A file becomes a
    candidate once its (mtime, size) has been unchanged for `stabilityCycles`
    (5) scans.
  - `ingest` extracts the newest stable zip atomically (temp dir, fsync,
    completion marker, rename) into `/data/schedule-feed/<YYYYMMDDTHHMMSSZ>/`
    and keeps `retentionKeepDeliveries` (3; separate from CORPUS's own 3) extracted deliveries.
  - `ingest` then POSTs `{delivered_at, ingested_at, files}` to api
    `POST /private/schedule-feed-ingests`, using its own internal OAuth
    credential (Authentik client-credentials, group `svc-schedule-ingest`).
    The zip itself stays in `incoming` and is overwritten by the next push.
  - **`reference`** (`crates/schedule-reference`) mounts the PVC read-only,
    parses the newest complete delivery, and POSTs derived products
    (`/private/stanox-crs`, `/private/tiploc-crs` and others) with its own
    credential (`svc-schedule-reference`).
- **api.** Every `/private/*` route is gated per (path, method) by
  `app::build_internal_oauth_routes` on an `INTERNAL_OAUTH_GROUP_*` value.
  Every such env var must be wired on the api container.
  `data::config::chart_env_wiring_tests` enforces that.

### The hazard found on the way

CIF detection took the newest `*.zip` in `incoming`, so a CORPUS or SMART
file pushed as a zip would have been extracted and published as the
timetable. This is fixed first, as a standalone commit:

- A file is a CIF candidate only if it matches `CIF_FILE_PATTERN` (default
  `timetable_full.zip`, DTD's exact delivery name; see decision 4) and does
  not match `CIF_EXCLUDE_PATTERN` (default `CORPUSExtract*`, whatever the
  extension).
- Patterns are comma-separated, case-insensitive `*` globs
  (`crates/schedule-ingest/src/pattern.rs`).
- Every other file keeps the existing one-time "stray file" warning.

## Design

### Distinguishing the file

The file is recognised by name only: `CORPUS_FILE_PATTERN`, default
`CORPUSExtract.json.gz` (the RDM delivery name). The content check is simply
that the file must parse as gzipped JSON with a `TIPLOCDATA` array. Anything
else is rejected. `CORPUSExtract.csv.gz` and any other file are strays.

### Ingest path: a sibling mode in `schedule-ingest`

The CORPUS path is a second, independent step in the same scan loop
(`crates/schedule-ingest/src/corpus.rs`). It reuses the CIF path's directory
scan and `StabilityTracker` primitives but keeps its own state. It runs under
the same SFTP user, the same `incoming` folder, the same container and the
same PVC. **It is off unless `CORPUS_INGEST_ENABLED=true`**
(`scheduleFeed.corpus.enabled`). While it is off, a CORPUS file is just a
stray, exactly as before.

Each cycle, with the flag on, the CORPUS step does the following:

1. It picks the newest stable file matching `CORPUS_FILE_PATTERN`.
2. It reads and gunzips the file, capped at `CORPUS_MAX_DECOMPRESSED_BYTES`
   (256 MiB; the real file is about 7 MB) as a gzip-bomb guard. It then
   parses `TIPLOCDATA`.
3. It normalises each value:
   - Values are trimmed, and a blank becomes NULL.
   - An NLC that is a number, or all digits, is left-padded to 6 digits.
   - A STANOX that is all digits is left-padded to 5 digits.
   - `3ALPHA` becomes `crs`.
4. It refuses a file with fewer than `CORPUS_MIN_ROWS` (10,000) rows, a row
   with no NLC, or a file that is not gzip or not the expected JSON. It counts
   `schedule_feed_corpus_rejected_total` and moves the file to
   `storage/corpus/rejected/`.
5. It POSTs `{delivered_at (file mtime), source_file, locations[]}` to
   `CORPUS_API_URL` (`/private/corpus-locations`).
6. On a 2xx response, it re-checks that the file's (mtime, size) is
   unchanged. If so, it moves the file (a rename on the same PVC) to
   `/data/schedule-feed/corpus/<YYYYMMDDTHHMMSSZ>-<name>` and keeps
   `CORPUS_RETENTION_KEEP` (3) files there and in `rejected/`. The re-check
   means a re-upload that starts mid-cycle is never moved away half-written.
   Files therefore never pile up in `incoming`.
7. If the POST fails, the file stays where it is and the whole step is
   retried next cycle. Loading the same file again is idempotent.
8. After a successful load, any older stable file that matches the pattern
   is archived without being loaded. This can only happen with a wildcard
   pattern. It means older data can never be loaded over newer data.

Memory use stays well within the `ingest` container's 256 Mi limit. At peak
the step holds the ~0.8 MB compressed file, ~7 MB of JSON, ~56k parsed rows
and the ~10 MB request body.

There is no in-memory dedup state that a restart can lose. A file that is
still in `incoming` has not been archived, so it is loaded again. If the move
itself fails after a successful load, the step remembers the (mtime, size) it
loaded, so it does not re-POST every cycle.

It records these metrics:

- `schedule_feed_corpus_last_load_delivered_at_seconds`
- `schedule_feed_corpus_rows`
- `schedule_feed_corpus_rejected_total`, which feeds the new
  `DistantSignalCorpusRejected` alert.

### api: `POST /private/corpus-locations`

This route has its own group, `INTERNAL_OAUTH_GROUP_CORPUS` (default
`svc-corpus-ingest`, chart value `api.internalOauth.groups.corpus`). The
`schedule-ingest` service account must be **added to that group** in
Authentik. There is no new credential: the same client-credentials user
carries both groups.

The route rejects an empty `locations` list or a blank `nlc` with 400. It
then runs `data::corpus::replace_corpus_locations` in one transaction:

1. `pg_advisory_xact_lock`, so concurrent loads serialise.
2. `DELETE FROM corpus_locations`.
3. One `INSERT … SELECT FROM UNNEST(…)`.
4. Upsert the `corpus_deliveries` marker row.

Readers see the old set or the new set, never a mixture. `DELETE` rather than
`TRUNCATE` means readers are never blocked. The monthly cost of about 56k dead
tuples is left to autovacuum.

### Schema

The migration is `20260928100000_corpus_locations.sql`. It is transactional
and starts with `SET LOCAL lock_timeout = '5s'`. It creates new tables only,
so the indexes on those new tables can live in the same file, which
`migration_index_locking` allows.

```sql
corpus_locations (
  id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,  -- no natural key:
  nlc TEXT NOT NULL,        -- 20 NLCs and 20 TIPLOCs repeat in the sample
  stanox TEXT, tiploc TEXT, crs TEXT, uic TEXT,
  nlc_desc TEXT, nlc_desc16 TEXT,
  delivered_at TIMESTAMPTZ NOT NULL, source_file TEXT NOT NULL,
  loaded_at TIMESTAMPTZ NOT NULL DEFAULT now())
-- partial indexes on tiploc, stanox, crs (WHERE NOT NULL); index on nlc

corpus_deliveries (            -- publish/freshness marker, one row per load
  delivered_at TIMESTAMPTZ PRIMARY KEY, source_file TEXT NOT NULL,
  row_count INTEGER NOT NULL, loaded_at TIMESTAMPTZ NOT NULL DEFAULT now())
```

Retention: `corpus_locations` holds only the current delivery. The marker
table grows by one tiny row a month. The archived files on the PVC are capped
at 3 processed and 3 rejected.

### Chart

- `scheduleFeed.corpus.enabled` (false), `filePattern`, `minRows`,
  `retentionKeep` and `maxDecompressedBytes` are wired to the `ingest`
  container. So are `scheduleFeed.ingest.cifFilePattern` and
  `cifExcludePattern`, and `CORPUS_API_URL`.
- `api.internalOauth.groups.corpus` is wired to the api container.
- There are no SFTP, Service or NetworkPolicy changes. The same user and
  folder are used, and `ingest` already reaches api.
- A `chart_env_wiring_tests` module in `schedule-ingest` asserts that every
  `CORPUS_*`/`CIF_*`/`*_URL` env var is set on the `ingest` container.

## Follow-up (2026-09-28): comparison, fallback, CSV regeneration

Built after this design, in the user's order:

1. **Comparison, read-only.** `api::data::corpus_comparison` compares
   CORPUS (through the shared conservative inference,
   `common::corpus_inference`) with `tiploc_crs`/`stanox_crs` and
   `stations`. Every load logs a summary and sets
   `distant_signal_api_corpus_comparison_{tiplocs,stanoxes}{outcome}`; the
   full report is `kubectl exec deploy/distant-signal-api -c api --
   corpus_compare [--full]`.
2. **Runtime fallback, off by default** (`api.corpusFallback.enabled`,
   `CORPUS_FALLBACK_ENABLED`). See `api::data::corpus_crosswalk`: the
   timetable crosswalk stays primary and wins every conflict; CORPUS fills
   TIPLOCs neither timetable table has and STANOXes the timetable does not
   know. Applied in the five `queries` lookups, so every api caller and the
   `GET /private/stanox-crs` consumers get it unchanged.
3. **`crs-tiploc.csv` from the loaded CORPUS**: a manual runbook in
   `reference-data/line-catalogue-validation.md` ("From the CORPUS the app
   has loaded"), plus `line-catalogue-validator
   --regenerate-crs-tiploc-from-db` (feature `db`).

The consumers below that are NOT covered: `schedule-reference`'s own
in-process CIF crosswalk (`crs_to_tiploc_map`, which decides which lines'
schedules are published) and names. No UI shows the timetable's terse
TPS names: station names come from the Knowledgebase `stations` table, and
the TIPLOC fallback lets more stops reach it. So no name substitution was
built; the comparison lists the name differences for a later decision.

## Consumers (not changed here)

- The **`stanox_crs` / `tiploc_crs` crosswalks** are currently derived from
  CIF by `schedule-reference`. CORPUS is the authoritative master list and
  could fill TIPLOCs or STANOXes that CIF never calls at, such as junctions
  and freight locations. It could also let a sub-CRS TIPLOC map to its
  parent station through a shared STANOX or NLC.
- **Station and location names**: `NLCDESC` gives display names for
  TIPLOC/STANOX-only locations, for example in TRUST event rendering and
  train detail.
- **The LDBWS delay-reason matcher's sub-CRS fix**
  (`ds-review/ldbws-delay-reason-on-train-detail-scope.md` §4.4). Its 18
  board-CRS ↔ TIPLOC-CRS alias pairs (PAD/PDX, STP/SPL and others) and its
  two TIPLOCs missing from `tiploc_crs` could come from CORPUS, by grouping
  TIPLOCs on NLC location or STANOX, instead of a static map.
- **`reference-data/crs-tiploc.csv` regeneration**: the
  `line-catalogue-validator --regenerate-crs-tiploc-from-corpus` input could
  be read from `corpus_locations` instead of a hand-downloaded file.
  `regenerate.rs` is being changed concurrently and is untouched here.
- **trust-consumer / trust-backlog-consumer** STANOX→CRS reloads.

## Decisions for the user

1. **Whether and when to replace the CIF-derived lookups with CORPUS.** This
   covers `tiploc_crs`, `stanox_crs` and the static CSV. It is not done here.
   The table only lands, so it can be compared with the CIF-derived data
   first.
2. **Enabling it.** Enable it in Ranma-Config with
   `scheduleFeed.corpus.enabled=true`, and add the schedule-ingest Authentik
   user to a `svc-corpus-ingest` group, or set
   `api.internalOauth.groups.corpus` to the group you create. Until both are
   done, a delivered file sits in `incoming` as a one-time stray warning. If
   only the flag is on, every cycle logs a 403 error.
3. **Ignoring the SMART file.** The provider's `CORPUSExtract.csv.gz` is SMART
   berth data, and is deliberately ignored.
4. **The CIF pattern default.** Decided 2026-09-28: locked to the exact
   `timetable_full.zip` name (the only name DTD has delivered under; 34 of 34
   ingests in prod's last 72 h). `CORPUSExtract*` stays excluded as a second
   guard. A renamed delivery would now be logged as a stray and hit the
   existing "no .zip delivery by the final check time" error, rather than
   being picked up silently.
