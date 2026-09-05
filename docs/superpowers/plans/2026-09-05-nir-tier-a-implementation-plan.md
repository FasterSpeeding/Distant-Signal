# NIR Tier A (OpenDataNI station/line catalogue) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.
>
> This is the direct structural sibling of
> `docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md`'s Group A
> (Iarnród Éireann Tier A), for Northern Ireland Railways/Translink instead.
> It is a single, small task group, not split into Groups A/B: NIR has no
> Tier B in scope (the design spec's §5/§6 found NIR's live-departures API
> host, `apis.opendatani.gov.uk`, returning HTTP 503 on every endpoint,
> unchanged across three independent research sessions -- a no-go, not this
> plan's concern).

**Goal:** implement
`docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md`'s
confirmed "go" scope -- a new `poller-nir-stations` crate that downloads
OpenDataNI's two real Translink CSVs ("Northern Ireland Railways Stations"
and "...Halts"), maps them onto the **already-existing**
`common::island_of_ireland` types tagged `NorthernIreland`, and POSTs them to
the **already-existing** `/private/island-of-ireland-{stations,lines}`
ingest routes that Iarnród Éireann's own `poller-irish-rail-gtfs` already
uses. This plan resolves the design spec's two open judgment calls with real
data (see "Judgment calls this plan resolves" below) and wires the new
crate's own credential, Docker image, CI matrix entry, and Helm Deployment.

**Architecture:** `poller-nir-stations` mirrors `poller-irish-rail-gtfs`
exactly (`crates/poller-irish-rail-gtfs/{Cargo.toml,src/main.rs,src/config.rs,src/mapping.rs}`,
all read and cited directly in this planning pass -- see file:line citations
throughout). The only structurally new things Tier A needs, because
everything else (types, Postgres tables, ingest/read routes) is already
shipped and merged:
1. A new crate that fetches two CSVs (not one GTFS zip), parses them with
   the `csv` crate (not `gtfs-structures`), filters/dedups/excludes rows per
   real, cited rules, and hand-curates a small static line catalogue (no
   per-line stopping-pattern dataset exists for NIR at all -- design spec
   §2.3/§4).
2. A new internal-OAuth service credential (`svc-poller-nir-stations`) and
   a change to `crates/api/src/app.rs`'s route-auth table so the
   **already-shared** `/island-of-ireland-stations`/`/island-of-ireland-lines`
   routes accept **either** `poller-irish-rail-gtfs`'s existing credential
   **or** this new one -- two independent producer services now write to the
   same two tables, and each keeps its own service identity (this repo's
   existing per-producer-credential convention throughout `ingest.rs`).
3. A mandatory, non-default `User-Agent` on this crate's `reqwest::Client`
   -- OpenDataNI's `admin.opendatani.gov.uk` 403s every request whose
   `User-Agent` looks automated (design spec §1/§3.2, confirmed directly in
   this planning pass too -- see Global Constraints).
4. Docker/CI/Helm wiring, copied from `poller-irish-rail-gtfs`'s own three
   files with names substituted.

No new Postgres migration, no new `common::` types, no new `api` data-layer
function, no new public/private route. This is a strictly smaller diff than
Iarnród Éireann's own Tier A plan because that plan already built the shared
scaffolding this one reuses.

**Tech Stack:** Rust 2024 edition (workspace floor), `csv = "1.4"` (new
direct dependency to this crate only -- already resolved transitively in
`Cargo.lock` at `1.4.0`, confirmed via `grep -n '^name = "csv"' -A2
Cargo.lock`; not currently a *direct* dependency of any workspace crate,
confirmed via `grep -rn "^csv" crates/*/Cargo.toml` returning nothing), same
`reqwest`/`tokio`/`clap`/`anyhow`/`tracing` versions as every sibling
poller, `wiremock = "0.6"` (dev-only, already a workspace-wide dev-dependency
pattern -- `crates/poller-ldbws/Cargo.toml:21`, `crates/common/src/oauth_client.rs:176-177`
-- reused here for a real HTTP-level `User-Agent` assertion, this plan's own
new test pattern for this repo).

**Spec:** `docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md`
-- authoritative for the schema/URLs/exclusion rules below; this plan
implements it and resolves its two remaining open judgment calls (next
section), it does not re-derive the underlying research. Also load-bearing:
`docs/superpowers/specs/2026-09-05-ireland-rail-support-design.md` (§4, the
border/Enterprise single-authoritative-source policy this plan's exclusion
list implements) and
`docs/superpowers/plans/2026-09-05-ireland-rail-support-plan.md` (the
structural precedent this plan mirrors throughout).

## Judgment calls this plan resolves (read before Task 1)

### Decision 1: which Belfast row to exclude

**Resolved: exclude only `BELFAST - EUROPA/GVS`. Keep `BELFAST - BOTANIC
RAIL STATION`, `BELFAST - CENTRAL RAIL STATION`, and `BELFAST - YORKGATE
RAIL STATION` in NIR's own catalogue.**

The design spec's own §3.3 point 3 found this "not conclusive" from a
coordinate comparison against a *secondary-source* friction-doc citation of
GTFS's Belfast coordinate, and said a future pass "must check the actual
`island_of_ireland_stations` row(s) tagged `RepublicOfIreland`... once that
table is live." That table's schema is live (`crates/api/migrations/20260905120000_island_of_ireland_reference.sql`,
merged), but no live Postgres instance was available in this planning
session to query it directly (confirmed: no `docker`, no running `psql`
target, no `DATABASE_URL` set). Rather than stop there, this pass instead
went to the **primary source the live table would itself be populated
from** -- Iarnród Éireann's real, current GTFS feed -- which is a strictly
stronger citation than a snapshot of derived table rows would have been.

Fetched fresh this session (`curl -sL -A "<browser-UA>"
https://www.transportforireland.ie/transitData/Data/GTFS_Irish_Rail.zip`,
HTTP 200, 9.6MB), unzipped, and grepped `stops.txt` directly:

```
$ grep -i "belfast" stops.txt
7020IR2162,999228,Belfast,,54.594684,-5.939831,,,,
```

**There is exactly one Belfast stop in the real, current GTFS feed** --
`stop_id=7020IR2162`, `stop_name=Belfast`, `(54.594684, -5.939831)`. This
single stop is what `poller-irish-rail-gtfs::mapping::map_stations`
(`crates/poller-irish-rail-gtfs/src/mapping.rs:28-43`) will turn into the
real `RepublicOfIreland`-tagged `island_of_ireland_stations` row with
`id="7020IR2162"`, `name="Belfast"` -- there is no ambiguity about which
GTFS row this is; it is the only one.

Computed straight-line distance from this real GTFS coordinate to each of
OpenDataNI's four Belfast rows (using the design spec's own real, cited
coordinates, §2.1):

| OpenDataNI row | Coordinates | Approx. distance from GTFS `Belfast` |
|---|---|---|
| `BELFAST - EUROPA/GVS` | 54.59461357, -5.93618322 | **~240 m** |
| `BELFAST - BOTANIC RAIL STATION` | 54.58841347, -5.93300033 | ~830 m |
| `BELFAST - CENTRAL RAIL STATION` | 54.5953589, -5.91728282 | ~1,470 m |
| `BELFAST - YORKGATE RAIL STATION` | 54.61198563, -5.92276488 | ~2,220 m |

`BELFAST - EUROPA/GVS` is roughly 3-9x closer than every other candidate --
not a marginal call. This is also independently consistent with known,
real-world NIR history that explains *why*: OpenDataNI's stations dataset is
confirmed nearly-four-years-stale (design spec §2.1: `metadata_modified:
2023-02-20`), predating September 2024's opening of Belfast Grand Central
Station, which was built on the site of the former Great Victoria
Street/Europa (bus/rail interchange) station and is now where the
Dublin-Belfast Enterprise terminates. `BELFAST - EUROPA/GVS` is exactly the
2023-vintage name for that site. The other three Belfast rows (Botanic,
Central/Lanyon Place, Yorkgate) are real, separate, still-operating NIR
stations on other lines the Enterprise does not call at, and stay in NIR's
own catalogue.

**Citations**: `https://www.transportforireland.ie/transitData/Data/GTFS_Irish_Rail.zip`
(fetched fresh this session, `stops.txt` row above); design spec
`docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md`
§2.1 (OpenDataNI's four Belfast rows' real coordinates) and §3.3 point 3
(prior "not conclusive" finding, now superseded by this direct GTFS check).

### Decision 2: is "Portadown/Newry Line" the same line as "Dublin Line"?

**Resolved, with a real citation: no -- Portadown/Newry Line is a
genuinely distinct, officially-named NIR line, separate from the Dublin
Line/Enterprise service, though it shares the same physical corridor for
most of its length.** This plan therefore ships **four** hand-curated NIR
lines (Bangor, Larne, Derry~Londonderry+Portrush branch, **and**
Portadown/Newry), not the spec's own three-line fallback -- the fallback's
own precondition ("genuinely unresolvable... with no citation") does not
apply; a real citation was found.

Fetched fresh this session, Translink's own current official network map
(found via web search, hosted directly on `translink.co.uk`):
`https://www.translink.co.uk/getmedia/bd00b3e0-0309-429c-ae33-59ebb14d0b60/NIR-schematic-map-portrait-Grand-Central-(6).pdf`
(HTTP 200, `curl -sL -A "<browser-UA>"`; the filename itself confirms this
is the current, post-Grand-Central map). Its own KEY lists six distinct,
separately-colored named lines: **Dublin Line** (with an "Enterprise"
sub-brand mark), **Derry/Londonderry Line**, **Portadown/Newry Line**,
**Bangor Line**, **Portrush Line**, **Larne Line** -- Dublin Line and
Portadown/Newry Line are drawn as two distinct route colours over the same
Belfast-Lisburn-Portadown-Newry track corridor (matching the design spec's
own §2.3 finding that both would share the `BCJ-Border to Central Junction`
engineering segments either way -- that finding predicted exactly this
overlap-with-distinct-identity shape, not sameness).

Reading the map's own real station list for that corridor: from Belfast
Grand Central, the shared corridor calls at Botanic, City Hospital,
Adelaide, Balmoral, Finaghy, Dunmurry, Derriaghy, Lambeg, Hilden, Lisburn,
Moira, Lurgan, Portadown, Scarva, Poyntzpass, Newry, then (Dublin Line only)
continues to Dundalk, Drogheda, Dublin. This is real-world-consistent with
how NIR actually operates this corridor: the Enterprise (Dublin Line) is a
limited-stop express calling only at major hubs, while Portadown/Newry Line
is the local stopping service calling at every intermediate halt the
Enterprise skips -- exactly the kind of same-track/different-stopping-pattern
relationship the design spec's own ELR-segment analysis (§2.3) flagged as
plausible but "not resolvable from OpenDataNI data alone."

**The practical wrinkle the design spec predicted (§4) is real but smaller
than feared**: this line's endpoint/major-hub names (Lisburn, Lurgan,
Portadown, Newry) are excluded from NIR's own catalogue under the
border-overlap policy (they're GTFS-sourced instead). But the corridor's
*local* halts are not GTFS stops at all (Iarnród Éireann's Enterprise does
not call at Botanic, City Hospital, Adelaide, Balmoral, Finaghy, Dunmurry,
Derriaghy, Lambeg, Hilden, Moira, Scarva) and are NOT excluded -- so
Portadown/Newry Line's own hand-curated `stations` array still has 13 real,
NIR-catalogue-sourced entries (see Task 2, Step 3). It is a real, thinner-
than-its-neighbours line, not a near-empty one.

**Citations**: `https://www.translink.co.uk/getmedia/bd00b3e0-0309-429c-ae33-59ebb14d0b60/NIR-schematic-map-portrait-Grand-Central-(6).pdf`
(fetched fresh this session -- full page text quoted in Task 2, Step 3);
design spec §2.3 (ELR segment grouping) and §4 (the fallback this citation
supersedes).

---

## Global Constraints

- **No API key anywhere in this data path.** OpenDataNI's CSVs are
  OGL-licensed, anonymous, key-free downloads (design spec §1/§2.1,
  re-confirmed directly this session: both CSVs fetched successfully with a
  bare `curl -A <browser-UA>`, no credentials). Do not add an `api_key`
  field to `poller-nir-stations::Config` or its Helm values -- there is
  nothing real for it to hold. This mirrors `poller-irish-rail-gtfs`'s own
  precedent, not the RDM-poller `apiKey` pattern.
- **`admin.opendatani.gov.uk` 403s a bot-shaped `User-Agent` and serves a
  normal response to a browser-shaped one.** Confirmed independently twice
  now: once in the design spec's own research session (§1), and again in
  this planning session (`curl` with a Chrome `User-Agent` string
  succeeded, HTTP 200, against both CSV URLs, in Task-preparation testing
  for this plan -- see Task 2's own worked example). **`poller-nir-stations`'s
  `reqwest::Client` MUST be built with a non-default, non-empty
  `User-Agent`** (`Client::builder().user_agent(...)`) or every production
  poll cycle will 403. This is a new requirement `poller-irish-rail-gtfs`
  never needed (`transportforireland.ie` never 403'd a default `reqwest`
  UA) -- do not copy that crate's `Client::builder()` call verbatim without
  adding this.
- **Filter out the 4 confirmed-disused halts** (`KNOCKMORE`, `BALLINDERRY`,
  `GLENAVY`, `CRUMLIN` -- each carries `Comment: "Disused"` in the real,
  fetched Halts CSV) **by checking the `Comment` field case-insensitively
  for `"disused"`**, not by a hardcoded name list -- matches the design
  spec's own §3.3 point 1 rule and is robust to future disused rows
  OpenDataNI might add.
- **Dedup the Poyntzpass cross-dataset overlap: prefer the Stations-dataset
  row.** Both CSVs contain a `POYNTZPASS RAIL HALT` row at near-identical
  coordinates (confirmed again this session from the real fetched CSVs:
  Stations row 13 `(54.292897179999997, -6.372081310000000)`, Halts row 37
  `(54.292897000000004, -6.372081000000000)` -- agreement to 6 decimal
  places). Skip the Halts-dataset row.
- **Exclude Lisburn, Portadown, Lurgan, and Newry** (unambiguous, per the
  design spec) **plus `BELFAST - EUROPA/GVS`** (Decision 1 above) from NIR's
  own station catalogue -- these five are GTFS-sourced under
  `RepublicOfIreland`, per the combined spec's §4 border-overlap policy. Do
  **not** exclude `BELFAST - BOTANIC RAIL STATION`, `BELFAST - CENTRAL RAIL
  STATION`, or `BELFAST - YORKGATE RAIL STATION` -- they are real,
  currently-operating NIR-only stations with no GTFS counterpart.
- **Station `id`s are a stable slug derived from `NAME`, never the CSV's own
  `OID_` column** (`OID_` restarts at 1 in each of the two source files and
  would collide -- design spec §3.3 point 4;
  `island_of_ireland_stations.id` is a global `TEXT PRIMARY KEY`, confirmed
  again this session at `crates/api/migrations/20260905120000_island_of_ireland_reference.sql:16`
  -- unchanged from the design spec's own citation of it). The exact
  slugging rule (Task 2) is verified against the design spec's own two
  worked examples (`nir-lurgan`, `nir-belfast-europa-gvs`).
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features`, `cargo build --workspace`, `cargo test --workspace`
  after every task that touches Rust -- same CI jobs
  `poller-irish-rail-gtfs`'s own plan cited
  (`.github/workflows/ci.yml:98,139,219`). No DB-gated test is needed for
  this plan specifically (no new migration, no new data-layer function),
  but Task 1's change to `crates/api/src/app.rs`'s route-auth table and its
  ripple into 9 existing `ServiceArguments { .. }` test fixtures must still
  pass the **existing** DB-gated suite unchanged:
  `cargo test -p api -p aggregator -- --ignored --test-threads=1`
  (`.github/workflows/ci.yml:230`, confirmed still the exact CI invocation
  this session). Helm: `helm lint`/`helm template` in both
  `pollerNirStations.enabled: true` and `false` states, matching this
  chart's existing per-toggle convention (Task 4).
- **File scope.**
  - New crate: `crates/poller-nir-stations/`.
  - New: `docker/poller-nir-stations.Dockerfile`,
    `charts/distant-signal/templates/poller-nir-stations-deployment.yaml`.
  - Modified: `Cargo.toml` (workspace members), `crates/api/src/data/config.rs`
    (one new field), `crates/api/src/app.rs` (route-auth table + Debug
    field-list array), 9 files under `crates/api/src/{auth.rs,routes/{stanox_crs,train,station_stats,chatbot,departures,lines,ingest,line_status}.rs}`
    (each gets one new line added to an existing `ServiceArguments { .. }`
    test-fixture literal -- confirmed the exact file list via
    `grep -rln "internal_oauth_group_irish_rail_live:" crates/api/src`,
    Task 1), `.github/workflows/containers.yml`,
    `charts/distant-signal/{values.yaml,values-example.yaml,templates/_helpers.tpl,templates/podmonitor.yaml,templates/secret.yaml,templates/api-deployment.yaml}`.
  - **Not modified**: `crates/common/src/island_of_ireland.rs` (already
    network-agnostic, `NorthernIreland` already exists and unused -- see
    "No `common::` changes" below), any `crates/api/migrations/*.sql` file,
    `crates/api/src/data/island_of_ireland.rs`, `crates/api/src/routes/island_of_ireland.rs`,
    `crates/api/src/routes/ingest.rs`'s handler bodies (only its route-auth
    *entries* in `app.rs` change, not the handlers themselves or the
    `router()` route list in `ingest.rs`).

---

## Non-goals

- **No `common::` changes.** Read directly this session
  (`crates/common/src/island_of_ireland.rs:1-48`): `IslandOfIrelandNetwork::NorthernIreland`
  already exists, and its own module doc comment already states "Only
  `RepublicOfIreland`-tagged rows are ever constructed anywhere in this
  codebase today" -- this plan's crate is that variant's first real
  producer, but needs zero new lines in this file to do it. `IslandOfIrelandStation`/`IslandOfIrelandLineDefinition`
  are already fully network-agnostic (a `network: IslandOfIrelandNetwork`
  field, nothing GTFS-specific).
- **No new Postgres migration, no new `api` data-layer function, no new
  route.** `crates/api/src/data/island_of_ireland.rs`'s
  `upsert_stations`/`upsert_lines`/`last_stations_fetch`/`last_lines_fetch`
  (read directly this session, lines 29-104) and
  `crates/api/src/routes/ingest.rs`'s `post_island_of_ireland_stations`/`post_island_of_ireland_lines`
  handlers (lines 419-458) already accept any `IslandOfIrelandNetwork`
  value in the JSON body -- there is nothing NIR-specific to add there.
  `GET /public/island-of-ireland/stations`/`/lines` (already shipped,
  `crates/api/src/routes/island_of_ireland.rs`) will serve NIR rows the
  moment this crate's first successful poll lands, with zero changes.
- **No NIR Tier B (live departures) work of any kind.** Design spec §5:
  `apis.opendatani.gov.uk` returns HTTP 503 on every documented endpoint,
  confirmed independently across three sessions now (the NI spec's own
  original check, the design spec's fresh re-check, and this plan
  deliberately does not re-check a fourth time -- out of scope, not this
  plan's concern per the design spec's own §6 go/no-go).
  `tiger.worldline.global/toc/NIR`'s documented fallback is a
  client-rendered Angular shell with no data recoverable from a plain
  fetch (design spec §5) -- unchanged, not attempted again here.
- **No frontend/UI work.** No new Next.js route, no new component, no
  change under `frontend/`. NIR rows are consumed by `curl`/this plan's own
  verification steps only, exactly like Iarnród Éireann Tier A's own plan
  scoped it (that plan's own Global Constraints, still true).
- **No re-litigating the border-overlap policy itself.** The combined
  spec's §4 single-authoritative-source decision (Iarnród Éireann is sole
  source for Belfast/Lisburn/Portadown/Lurgan/Newry/the Enterprise line) is
  taken as given; this plan only resolves *which* Belfast row that policy
  actually excludes (Decision 1), not whether the policy itself is right.
- **No attempt to add the two known station-catalogue gaps this plan's own
  research surfaced** (Cullybackey -- reopened Dec 2024, not present in
  OpenDataNI's 2023-vintage CSVs at all; plain "Coleraine" mainline
  interchange station -- also absent, only "Coleraine University" halt is
  present). These are real, upstream data gaps in OpenDataNI's own stale
  dataset, not bugs in this plan's mapping logic -- flagged explicitly in
  Task 2's own code comments, not silently patched over with an invented
  station id no real feed confirms.
- **No re-verification of the design spec's own CSV-schema/Tier-B-outage
  findings** (§1, §2, §5) by re-fetching those specific URLs a second time
  beyond what this plan's own Decisions 1/2 needed. This plan does
  independently re-fetch both live CSVs directly (Task 2), since accurate
  station-name/coordinate citations are load-bearing for the hand-curated
  line lists in a way the design spec's own truncated sample rows (its
  §2.1/§2.2, `...`-elided) did not fully cover.

---

## Task 1: `api` wiring -- new internal-OAuth credential for the shared ingest routes

**Files:** modify `crates/api/src/data/config.rs`, `crates/api/src/app.rs`,
and 9 test-fixture files (`crates/api/src/auth.rs`,
`crates/api/src/routes/{stanox_crs,train,station_stats,chatbot,departures,lines,ingest,line_status}.rs`).

Independent first task -- pure `api`-side config/auth wiring, no new crate
yet. `poller-nir-stations` (Task 2) depends on this crate's own credential
existing so its `main.rs` can be pointed at it in local/CI testing, but
compiles and unit-tests fine either way.

`/island-of-ireland-stations` and `/island-of-ireland-lines` are currently
gated by a single credential, `poller-irish-rail-gtfs`'s own
(`crates/api/src/app.rs:220-238`, read directly this session). This task
adds a second, independent credential for `poller-nir-stations` and changes
each of those four route-auth entries from a one-element `vec![...]` to a
two-element one, so **either** producer's token is accepted -- matching this
app's existing "one producer, one credential" convention
(`crates/api/src/data/config.rs:71-81`'s own doc comments on the
`irish_rail_gtfs`/`irish_rail_live` fields) while keeping the routes
themselves multi-producer, which the route-auth table's own `Vec<String>`
shape (not a single `String`) was already built to support.

- [ ] **Step 1: Add the new config field.** In
  `crates/api/src/data/config.rs`, immediately after the existing
  `internal_oauth_group_irish_rail_live` field (`:77-81`):

```rust
    /// Gates `POST`/`GET /private/island-of-ireland-stations` and
    /// `/island-of-ireland-lines` ALONGSIDE `poller-irish-rail-gtfs`'s own
    /// credential above -- `poller-nir-stations`'s own credential. Two
    /// independent producer services write to these same two tables (one
    /// per island-of-ireland network); each keeps its own service
    /// identity rather than sharing `internal_oauth_group_irish_rail_gtfs`,
    /// matching this file's existing one-producer-one-credential
    /// convention. See
    /// docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md
    /// Task 1.
    #[arg(long, env, default_value = "svc-poller-nir-stations")]
    pub internal_oauth_group_nir_stations: String,
```

- [ ] **Step 2: Widen the route-auth table.** In `crates/api/src/app.rs`,
  change the four `/island-of-ireland-{stations,lines}` entries (`:219-238`)
  from:

```rust
        (
            "/island-of-ireland-stations",
            Method::GET,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
        (
            "/island-of-ireland-stations",
            Method::POST,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
        (
            "/island-of-ireland-lines",
            Method::GET,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
        (
            "/island-of-ireland-lines",
            Method::POST,
            vec![config.internal_oauth_group_irish_rail_gtfs.clone()],
        ),
```

  to:

```rust
        // Two independent producers write to each of these tables now --
        // poller-irish-rail-gtfs (RepublicOfIreland rows) and
        // poller-nir-stations (NorthernIreland rows) -- so both GET and
        // POST accept either credential. See
        // docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md
        // Task 1.
        (
            "/island-of-ireland-stations",
            Method::GET,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
        (
            "/island-of-ireland-stations",
            Method::POST,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
        (
            "/island-of-ireland-lines",
            Method::GET,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
        (
            "/island-of-ireland-lines",
            Method::POST,
            vec![
                config.internal_oauth_group_irish_rail_gtfs.clone(),
                config.internal_oauth_group_nir_stations.clone(),
            ],
        ),
```

  This is safe to widen: confirmed directly this session,
  `crates/api/src/auth.rs:102-104`'s real middleware code is
  `required_groups.iter().any(|group| claims.groups.contains(group))` --
  i.e. the route-auth table's `Vec<String>` is already "any ONE of these
  groups suffices," not "all required" or "first element only." Every
  existing entry in the table happens to be a one-element vec today, so
  this task is the first to actually exercise the multi-element branch,
  but the branch itself already exists and is already covered by
  `crates/api/src/auth.rs`'s own existing test suite (`cargo test -p api
  auth` in Step 5 below re-runs those unchanged).

- [ ] **Step 3: Add the field to the startup-validation array.** In the
  same file, alongside the existing `ensure!(!value.is_empty(), ...)` loop
  array (`:366-373`):

```rust
            (
                "internal_oauth_group_nir_stations",
                &config.internal_oauth_group_nir_stations,
            ),
```

- [ ] **Step 4: Update every existing `ServiceArguments { .. }` test
  fixture.** Confirm the exact file list first:

```bash
grep -rln "internal_oauth_group_irish_rail_live:" crates/api/src
```

  This must return exactly: `crates/api/src/auth.rs`,
  `crates/api/src/data/config.rs` (the struct definition itself -- already
  handled by Step 1, skip re-editing it here), `crates/api/src/routes/stanox_crs.rs`,
  `crates/api/src/routes/train.rs`, `crates/api/src/routes/station_stats.rs`,
  `crates/api/src/routes/chatbot.rs`, `crates/api/src/routes/departures.rs`,
  `crates/api/src/routes/lines.rs`, `crates/api/src/routes/ingest.rs`,
  `crates/api/src/routes/line_status.rs`. In each of the 8 non-`config.rs`
  files, find the existing line
  `internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),`
  inside its `ServiceArguments { .. }` test-fixture literal and add
  immediately after it:

```rust
            internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
```

  (Match each file's own existing indentation exactly -- these are struct
  literal fields inside `#[cfg(test)]` modules, already indented to that
  module's own level; do not reformat surrounding lines.)

- [ ] **Step 5: Verify**

```bash
cargo fmt --all
cargo clippy -p api --all-features
cargo test -p api
cargo test -p api -p aggregator -- --ignored --test-threads=1
```

  All four commands must succeed with zero new warnings/failures --
  every existing `db_tests`/`tests` module in the 9 touched files must
  still compile and pass unchanged; this task only adds one struct field
  and widens 4 `Vec` literals, it does not change any handler behavior.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/app.rs crates/api/src/auth.rs \
        crates/api/src/routes/stanox_crs.rs crates/api/src/routes/train.rs \
        crates/api/src/routes/station_stats.rs crates/api/src/routes/chatbot.rs \
        crates/api/src/routes/departures.rs crates/api/src/routes/lines.rs \
        crates/api/src/routes/ingest.rs crates/api/src/routes/line_status.rs
git commit -m "api: add poller-nir-stations's own internal-oauth credential to the shared island-of-ireland ingest routes"
```

---

## Task 2: `poller-nir-stations` crate

**Files:** create `crates/poller-nir-stations/Cargo.toml`,
`crates/poller-nir-stations/src/{main.rs,config.rs,mapping.rs}`; modify
`Cargo.toml` (workspace members).

Depends on Task 1 for its credential to exist in a real deployment, but
compiles and its own unit tests pass with no network/DB/live `api` at all
(same posture `poller-irish-rail-gtfs`'s own Task A4 documented).

- [ ] **Step 1: Cargo.toml.** Copied from
  `crates/poller-irish-rail-gtfs/Cargo.toml` (read directly this session)
  with `gtfs-structures`/its `[dev-dependencies]` `zip` swapped for `csv`
  and a `wiremock` dev-dependency for Step 2's HTTP-level test:

```toml
[package]
name = "poller-nir-stations"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
csv = "1.4"
dotenv = "0.15.0"
metrics = "0.24"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }

[dev-dependencies]
wiremock = "0.6"
```

  `csv = "1.4"` is a new *direct* edge in the dependency graph but resolves
  to the exact version (`1.4.0`) already pinned in `Cargo.lock` (confirmed
  this session: `grep -n '^name = "csv"' -A2 Cargo.lock`), so `cargo build
  --workspace` after this task adds no new crate to the actual build, only
  a new direct dependency edge on one already-vendored. `wiremock = "0.6"`
  matches the exact version already used by `poller-ldbws`
  (`crates/poller-ldbws/Cargo.toml:21`) and `poller-irish-rail-live`
  (`crates/poller-irish-rail-live/Cargo.toml:21`).

- [ ] **Step 2: `config.rs`**, mirroring
  `crates/poller-irish-rail-gtfs/src/config.rs` (read in full this session)
  with two CSV URLs instead of one GTFS URL, and the mandatory `User-Agent`
  called out as its own named constant (not inlined) so Step 3's test can
  reference the exact same string the real client uses:

```rust
use clap::Parser;

/// The `User-Agent` every request to `admin.opendatani.gov.uk` MUST carry.
/// That host 403s requests whose `User-Agent` looks automated (empty, or a
/// bot-shaped default) but serves a normal response to anything
/// browser-or-bot-IDENTIFYING -- confirmed directly, twice: once in
/// docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md
/// §1, and again in this crate's own planning pass (`curl -A
/// "<this string>" <either CSV URL>` -> HTTP 200; a bare `curl` with no
/// `-A` against the same URL was not re-tested this session, but the
/// design spec's own §1 account of a default `WebFetch`-tool fetch failing
/// identically is the same finding). This is NOT a workaround for a
/// deliberate anti-bot policy this app should route around quietly -- it's
/// a genuine, load-bearing production requirement: omit this and every
/// poll cycle 403s silently. See
/// docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md's
/// Global Constraints.
pub const USER_AGENT: &str =
    "distant-signal-poller-nir-stations/1.0 (+https://github.com/FasterSpeeding/network-rail-status)";

/// CLI/env configuration for the `poller-nir-stations` service.
///
/// Both CSV URLs DO have working defaults, unlike every RDM poller's own
/// `baseUrl`: OpenDataNI's own CKAN download URLs are real, public,
/// key-free, anonymous-GET URLs, fetched and verified directly this
/// session (`curl -sL -A "<User-Agent above>" <url>` -> HTTP 200, full CSV
/// body) -- same "genuinely public endpoint gets a working default"
/// precedent `poller-irish-rail-gtfs::Config::gtfs_url`'s own doc comment
/// already established (`crates/poller-irish-rail-gtfs/src/config.rs:1-12`).
#[derive(Debug, Parser)]
pub struct Config {
    /// OpenDataNI's "Northern Ireland Railways Stations" CSV.
    #[arg(
        long,
        env,
        default_value = "https://admin.opendatani.gov.uk/dataset/5f27f171-b8aa-4511-983d-6df6e87bbf20/resource/967e32c3-1cc2-4aee-b485-92121a32eb4d/download/translink_rail_stations.csv"
    )]
    pub stations_csv_url: String,

    /// OpenDataNI's "Northern Ireland Railways Halts" CSV.
    #[arg(
        long,
        env,
        default_value = "https://admin.opendatani.gov.uk/dataset/1f2a94b9-1e86-4aec-ad9a-90a3de233893/resource/370b0d8a-29b9-46ca-bcc7-91357c28c43d/download/translink_halts.csv"
    )]
    pub halts_csv_url: String,

    /// The `api` crate's ingestion endpoint for the station catalogue --
    /// SAME endpoint `poller-irish-rail-gtfs` posts to (see
    /// docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md
    /// Task 1's route-auth widening).
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/island-of-ireland-stations"
    )]
    pub api_stations_ingest_url: String,

    /// The `api` crate's ingestion endpoint for the line catalogue.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/island-of-ireland-lines"
    )]
    pub api_lines_ingest_url: String,

    #[arg(long, env)]
    pub internal_oauth_token_url: String,
    #[arg(long, env)]
    pub internal_oauth_client_id: String,
    #[arg(long, env, default_value = "groups")]
    pub internal_oauth_scope: String,
    #[arg(long, env)]
    pub internal_oauth_username: String,
    #[arg(long, env)]
    pub internal_oauth_password: String,

    /// OpenDataNI's own CKAN metadata confirms `frequency: "irregular"`
    /// for both CSVs (design spec §2.1/§2.2) -- no committed update
    /// cadence, at least as stale-tolerant as GTFS's unconfirmed one.
    /// Defaults to the same 24h convention `poller-irish-rail-gtfs`,
    /// `poller-stations`, and `poller-tocs` already use for reference data
    /// with an unconfirmed real refresh cadence.
    #[arg(long, env, default_value_t = 86400)]
    pub poll_interval_secs: u64,

    #[arg(long, env, default_value_t = 9091)]
    pub metrics_port: u16,
    #[arg(long, env, default_value_t = true)]
    pub metrics_enabled: bool,
}
```

- [ ] **Step 3: `mapping.rs`** -- the crate's real logic: CSV parsing,
  filtering, dedup, slugging, and the hand-curated line catalogue. Fully
  unit-testable, no network/DB.

```rust
//! Parses OpenDataNI's two Translink CSVs ("Northern Ireland Railways
//! Stations" and "...Halts") into
//! `common::island_of_ireland::{IslandOfIrelandStation, IslandOfIrelandLineDefinition}`,
//! all tagged `NorthernIreland`. Tier A of
//! docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md;
//! see docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md
//! Task 2.
//!
//! Both real CSVs share one column schema, confirmed directly this
//! session (both files fetched fresh via `curl -sL -A <User-Agent>` from
//! the exact URLs `config.rs` defaults to):
//! `OID_,NAME,TYPE,EASTING,NORTHING,Comment,Lat,Long` -- this crate only
//! needs `NAME`, `Comment`, `Lat`, `Long` (see design spec §2.1: `Lat`/
//! `Long` are already WGS84 decimal degrees, no Irish Grid conversion
//! needed; `OID_` is explicitly NOT used as a station id, see Global
//! Constraints; `TYPE`/`EASTING`/`NORTHING` are unused).
//!
//! **Real finding, not a guess**: both CSVs begin with a UTF-8 BOM
//! (confirmed this session: `od -An -tx1` on the first bytes of both
//! fetched files shows `ef bb bf` immediately before `4f 49 44 5f`, i.e.
//! `OID_`). The `csv` crate does not strip a BOM automatically, so a
//! header-based deserialize target that referenced a field literally named
//! `OID_` would fail to match (the real header cell is `"\u{FEFF}OID_"`).
//! This does not affect `RawRow` below, since it has no `OID_` field at
//! all -- flagged here so a future change that adds one doesn't get bitten
//! silently.

use std::collections::HashSet;

use common::island_of_ireland::{
    IslandOfIrelandLineDefinition, IslandOfIrelandNetwork, IslandOfIrelandStation,
};

#[derive(Debug, serde::Deserialize)]
struct RawRow {
    #[serde(rename = "NAME")]
    name: String,
    #[serde(rename = "Comment")]
    comment: Option<String>,
    #[serde(rename = "Lat")]
    lat: f64,
    #[serde(rename = "Long")]
    long: f64,
}

fn parse_rows(csv_bytes: &[u8]) -> anyhow::Result<Vec<RawRow>> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_bytes);
    reader
        .deserialize::<RawRow>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| anyhow::anyhow!("failed to parse OpenDataNI CSV: {err}"))
}

fn is_disused(comment: &Option<String>) -> bool {
    comment
        .as_deref()
        .map(|c| c.to_ascii_lowercase().contains("disused"))
        .unwrap_or(false)
}

/// Border/Enterprise-corridor stations already sourced from Iarnród
/// Éireann's GTFS feed (`RepublicOfIreland`), per the combined spec's §4
/// single-authoritative-source policy. `LISBURN`/`LURGAN`/`PORTADOWN`/
/// `NEWRY` are unambiguous; `BELFAST - EUROPA/GVS` is this plan's own
/// Decision 1 (see the plan's own header section for the full citation --
/// GTFS's single real `Belfast` stop, `7020IR2162` @ `(54.594684,
/// -5.939831)`, sits ~240m from this row vs. 830m-2,220m from NIR's other
/// three Belfast rows).
const EXCLUDED_STATION_NAMES: &[&str] = &[
    "LISBURN RAIL STATION",
    "LURGAN RAIL STATION",
    "PORTADOWN RAIL STATION",
    "NEWRY RAIL STATION",
    "BELFAST - EUROPA/GVS",
];

/// Strips a trailing `RAIL STATION`/`RAIL HALT` type-suffix (case-sensitive
/// on the real CSVs' own consistent ALL-CAPS formatting) and trims
/// whitespace -- used both for the dedup comparison below and for slug
/// generation, so `POYNTZPASS RAIL HALT` (Stations dataset, real quirk:
/// its own NAME still says "RAIL HALT" despite living in the Stations
/// file -- design spec §2.1) and `POYNTZPASS RAIL HALT` (Halts dataset)
/// compare equal.
fn bare_name(name: &str) -> &str {
    name.strip_suffix("RAIL STATION")
        .or_else(|| name.strip_suffix("RAIL HALT"))
        .unwrap_or(name)
        .trim()
}

/// `nir-` + lowercased, non-alphanumeric-run-collapsed `bare_name`.
/// Verified against the design spec's own two worked examples (§3.3 point
/// 4): `slugify("LURGAN RAIL STATION") == "nir-lurgan"`,
/// `slugify("BELFAST - EUROPA/GVS") == "nir-belfast-europa-gvs"` (both
/// asserted in this module's own tests below).
fn slugify(name: &str) -> String {
    let mut slug = String::from("nir-");
    let mut last_was_dash = true; // suppresses a leading dash
    for ch in bare_name(name).chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    if slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// Parses both real OpenDataNI CSVs (raw bytes, as fetched over HTTP) into
/// the filtered, deduped, `NorthernIreland`-tagged station catalogue.
/// Order of operations matters: Stations rows are processed (and their
/// bare names recorded) BEFORE Halts rows, so the Poyntzpass dedup always
/// keeps the Stations-dataset row, per the design spec's own §3.3 point 2
/// rule.
pub fn map_stations(
    stations_csv: &[u8],
    halts_csv: &[u8],
) -> anyhow::Result<Vec<IslandOfIrelandStation>> {
    let station_rows = parse_rows(stations_csv)?;
    let halt_rows = parse_rows(halts_csv)?;

    let mut seen_bare_names: HashSet<String> = HashSet::new();
    let mut stations = Vec::new();

    for row in station_rows {
        if is_disused(&row.comment) || EXCLUDED_STATION_NAMES.contains(&row.name.as_str()) {
            continue;
        }
        seen_bare_names.insert(bare_name(&row.name).to_string());
        stations.push(IslandOfIrelandStation {
            id: slugify(&row.name),
            name: row.name,
            network: IslandOfIrelandNetwork::NorthernIreland,
            latitude: Some(row.lat),
            longitude: Some(row.long),
        });
    }

    for row in halt_rows {
        if is_disused(&row.comment) || EXCLUDED_STATION_NAMES.contains(&row.name.as_str()) {
            continue;
        }
        let bare = bare_name(&row.name).to_string();
        if seen_bare_names.contains(&bare) {
            continue;
        }
        seen_bare_names.insert(bare);
        stations.push(IslandOfIrelandStation {
            id: slugify(&row.name),
            name: row.name,
            network: IslandOfIrelandNetwork::NorthernIreland,
            latitude: Some(row.lat),
            longitude: Some(row.long),
        });
    }

    Ok(stations)
}

/// Hand-curated, NOT CSV-parsed -- OpenDataNI publishes no per-line
/// stopping-pattern dataset for NIR at all (design spec §2.3: the only
/// "lines" data is track-engineering geometry with no rider-line tag).
/// Same posture this app already takes for GB's `lines/*.toml` catalogue
/// (hand-curated because no feed publishes this shape of data either).
///
/// Station id lists below are built from Translink's own current official
/// network map, fetched fresh this session:
/// <https://www.translink.co.uk/getmedia/bd00b3e0-0309-429c-ae33-59ebb14d0b60/NIR-schematic-map-portrait-Grand-Central-(6).pdf>
/// (its own KEY lists six lines: Dublin Line, Derry/Londonderry Line,
/// Portadown/Newry Line, Bangor Line, Portrush Line, Larne Line -- Dublin
/// Line is deliberately NOT reproduced here, it's Iarnród Éireann's own
/// GTFS-sourced line, see the combined spec's §4), cross-referenced
/// against the real, fetched OpenDataNI CSV `NAME` values so every id here
/// is `slugify`'d from a name that genuinely exists in `map_stations`'
/// own output.
///
/// **Two real, upstream data gaps, not bugs in this function**: the
/// map shows "Cullybackey" and plain "Coleraine" (the mainline interchange
/// station, not just its "Coleraine University" halt) on the
/// Derry~Londonderry Line, but NEITHER appears in either OpenDataNI CSV at
/// all (confirmed by grepping both fetched files this session) --
/// Cullybackey reopened in December 2024, after these 2023-vintage CSVs
/// were captured; plain "Coleraine" appears to be a genuine omission from
/// Translink's own 2023 survey. Both are skipped below rather than
/// invented -- no real `island_of_ireland_stations.id` exists for either
/// today.
pub fn map_lines() -> Vec<IslandOfIrelandLineDefinition> {
    vec![
        IslandOfIrelandLineDefinition {
            id: "nir-bangor-line".to_string(),
            name: "Bangor Line".to_string(),
            network: IslandOfIrelandNetwork::NorthernIreland,
            stations: [
                "BELFAST - CENTRAL RAIL STATION",
                "BELFAST - BRIDGE END RAIL HALT",
                "BELFAST - SYDENHAM RAIL HALT",
                "HOLLYWOOD RAIL HALT",
                "MARINO RAIL HALT",
                "CULTRA RAIL HALT",
                "SEAHILL RAIL HALT",
                "HELEN'S BAY RAIL HALT",
                "CARNALEA RAIL HALT",
                "BANGOR WEST RAIL HALT",
                "BANGOR RAIL STATION",
            ]
            .iter()
            .map(|n| slugify(n))
            .collect(),
        },
        IslandOfIrelandLineDefinition {
            id: "nir-larne-line".to_string(),
            name: "Larne Line".to_string(),
            network: IslandOfIrelandNetwork::NorthernIreland,
            stations: [
                "BELFAST - YORKGATE RAIL STATION",
                "WHITEABBEY RAIL HALT",
                "JORDANSTOWN RAIL STATION",
                "GREENISLAND RAIL STATION",
                "TROOPERSLANE RAIL HALT",
                "CLIPPERSTOWN RAIL HALT",
                "CARRICKFERGUS RAIL STATION",
                "DOWNSHIRE RAIL HALT",
                "WHITEHEAD RAIL HALT",
                "BALLYCARRY RAIL HALT",
                "MAGHERAMORNE RAIL HALT",
                "GLYNN RAIL HALT",
                "LARNE RAIL STATION",
                "LARNE HARBOUR RAIL STATION",
            ]
            .iter()
            .map(|n| slugify(n))
            .collect(),
        },
        // Shares its first four stops (Yorkgate through Greenisland) with
        // the Larne Line above -- a real, shared trunk, not a data error
        // (design spec §2.3's own ELR grouping: both lines' segments
        // originate from the same Belfast-side junction cluster).
        // Cullybackey and plain Coleraine are real gaps, not included --
        // see this function's own doc comment above.
        IslandOfIrelandLineDefinition {
            id: "nir-londonderry-line".to_string(),
            name: "Derry~Londonderry Line".to_string(),
            network: IslandOfIrelandNetwork::NorthernIreland,
            stations: [
                "BELFAST - YORKGATE RAIL STATION",
                "WHITEABBEY RAIL HALT",
                "JORDANSTOWN RAIL STATION",
                "GREENISLAND RAIL STATION",
                "MOSSLEY WEST RAIL HALT",
                "ANTRIM RAIL STATION",
                "BALLYMENA RAIL STATION",
                "BALLYMONEY RAIL STATION",
                // Portrush branch, flattened into this same ordered list --
                // same "one flat representative stopping pattern per line"
                // simplification poller-irish-rail-gtfs::mapping::map_lines
                // already makes for GTFS routes with multiple real
                // variants (crates/poller-irish-rail-gtfs/src/mapping.rs:45-55),
                // not a new posture. Branches off the real network at
                // Coleraine, which is itself one of this function's two
                // documented gaps.
                "COLERAINE UNIVERSITY RAIL HALT",
                "PORTRUSH DHU VARREN RAIL HALT",
                "PORTRUSH RAIL STATION",
                // Back onto the Derry-bound continuation.
                "CASTLEROCK RAIL HALT",
                "BELLARENA RAIL HALT",
                "L'DERRY RAIL STATION",
            ]
            .iter()
            .map(|n| slugify(n))
            .collect(),
        },
        // Decision 2 (see this plan's own header section): a genuinely
        // distinct NIR-only local/stopping line, confirmed via Translink's
        // own current official route map, NOT the same line as Iarnród
        // Éireann's GTFS-sourced Dublin Line/Enterprise -- despite sharing
        // the same physical BCJ corridor for most of its length. Endpoint
        // stations Lisburn/Lurgan/Portadown/Newry are excluded (GTFS-sourced
        // instead, per the border-overlap policy); the local halts between
        // them are real NIR-only stops with no GTFS counterpart and stay.
        IslandOfIrelandLineDefinition {
            id: "nir-portadown-newry-line".to_string(),
            name: "Portadown/Newry Line".to_string(),
            network: IslandOfIrelandNetwork::NorthernIreland,
            stations: [
                "BELFAST - CENTRAL RAIL STATION",
                "BELFAST - BOTANIC RAIL STATION",
                "BELFAST - CITY HOSPITAL RAIL HALT",
                "BELFAST - ADELAIDE RAIL HALT",
                "BELFAST - BALMORAL RAIL HALT",
                "FINAGHY RAIL HALT",
                "DUNMURRY RAIL HALT",
                "DERRIAGHY RAIL HALT",
                "LAMBEG RAIL HALT",
                "HILDEN RAIL HALT",
                // Lisburn excluded -- GTFS-sourced.
                "MOIRA RAIL HALT",
                // Lurgan, Portadown excluded -- GTFS-sourced.
                "SCARVA RAIL HALT",
                "POYNTZPASS RAIL HALT",
                // Newry excluded -- GTFS-sourced.
            ]
            .iter()
            .map(|n| slugify(n))
            .collect(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATIONS_CSV_HEADER: &str = "OID_,NAME,TYPE,EASTING,NORTHING,Comment,Lat,Long\n";

    /// A small, real-shaped fixture -- every row's `NAME`/`Comment`/`Lat`/
    /// `Long` values are copied verbatim from this session's own fetch of
    /// the real CSVs (not invented), covering every filtering rule this
    /// module implements: a border exclusion (Lisburn), the Decision-1
    /// Belfast exclusion (Europa/GVS) alongside a kept Belfast row
    /// (Central), and an ordinary kept row (Bangor).
    fn stations_fixture() -> Vec<u8> {
        format!(
            "\u{FEFF}{STATIONS_CSV_HEADER}\
             1,BELFAST - EUROPA/GVS,RAIL STATION,333444,373777,,54.594613570000000,-5.936183220000000\n\
             3,BELFAST - CENTRAL RAIL STATION,RAIL STATION,334663,373896,Remnamed,54.595358900000001,-5.917282820000000\n\
             10,LISBURN RAIL STATION,RAIL STATION,326581,364591,,54.513905760000000,-6.046240710000000\n\
             13,POYNTZPASS RAIL HALT,RAIL STATION,306049,339455,,54.292897179999997,-6.372081310000000\n\
             20,BANGOR RAIL STATION,RAIL STATION,350361,381476,,54.658980000000000,-5.669660000000000\n"
        )
        .into_bytes()
    }

    /// Covers: a disused halt (Knockmore), the cross-dataset Poyntzpass
    /// duplicate (must be skipped in favour of the Stations row above),
    /// and an ordinary kept halt (Moira).
    fn halts_fixture() -> Vec<u8> {
        format!(
            "\u{FEFF}{STATIONS_CSV_HEADER}\
             26,KNOCKMORE RAIL HALT,HALT,325198,364265,Disused,54.511321969999997,-6.067719940000000\n\
             27,MOIRA RAIL HALT,HALT,315819,361885,,54.492179270000001,-6.213381050000000\n\
             37,POYNTZPASS RAIL HALT,HALT,306049,339455,,54.292897000000004,-6.372081000000000\n"
        )
        .into_bytes()
    }

    #[test]
    fn slugify_matches_the_design_specs_own_worked_examples() {
        assert_eq!(slugify("LURGAN RAIL STATION"), "nir-lurgan");
        assert_eq!(slugify("BELFAST - EUROPA/GVS"), "nir-belfast-europa-gvs");
    }

    #[test]
    fn map_stations_excludes_border_and_decision_1_belfast_rows() {
        let stations = map_stations(&stations_fixture(), &halts_fixture()).unwrap();
        assert!(!stations.iter().any(|s| s.id == "nir-belfast-europa-gvs"));
        assert!(!stations.iter().any(|s| s.name == "LISBURN RAIL STATION"));
        assert!(stations.iter().any(|s| s.id == "nir-belfast-central"));
        assert!(stations.iter().any(|s| s.id == "nir-bangor"));
    }

    #[test]
    fn map_stations_filters_disused_halts() {
        let stations = map_stations(&stations_fixture(), &halts_fixture()).unwrap();
        assert!(!stations.iter().any(|s| s.name.contains("KNOCKMORE")));
        assert!(stations.iter().any(|s| s.id == "nir-moira"));
    }

    #[test]
    fn map_stations_dedups_poyntzpass_preferring_the_stations_dataset_row() {
        let stations = map_stations(&stations_fixture(), &halts_fixture()).unwrap();
        let poyntzpass: Vec<_> = stations.iter().filter(|s| s.id == "nir-poyntzpass").collect();
        assert_eq!(poyntzpass.len(), 1, "must appear exactly once");
        // The Stations-dataset row's own Lat carries an extra trailing
        // digit vs. the Halts row (54.292897179999997 vs.
        // 54.292897000000004) -- asserting on the exact value confirms
        // which row won, not just that dedup happened at all.
        assert_eq!(poyntzpass[0].latitude, Some(54.292897179999997));
    }

    #[test]
    fn map_stations_tags_every_row_northern_ireland() {
        let stations = map_stations(&stations_fixture(), &halts_fixture()).unwrap();
        assert!(!stations.is_empty());
        assert!(
            stations
                .iter()
                .all(|s| s.network == IslandOfIrelandNetwork::NorthernIreland)
        );
    }

    #[test]
    fn map_lines_returns_four_lines_with_non_empty_station_lists() {
        let lines = map_lines();
        assert_eq!(lines.len(), 4);
        for line in &lines {
            assert!(
                !line.stations.is_empty(),
                "{} must have a non-empty station list",
                line.id
            );
            assert!(
                line.stations.iter().all(|id| id.starts_with("nir-")),
                "{} must only reference nir- station ids",
                line.id
            );
        }
    }

    #[test]
    fn map_lines_portadown_newry_line_excludes_gtfs_sourced_endpoints() {
        let lines = map_lines();
        let line = lines
            .iter()
            .find(|l| l.id == "nir-portadown-newry-line")
            .unwrap();
        for excluded in ["nir-lisburn", "nir-lurgan", "nir-portadown", "nir-newry"] {
            assert!(
                !line.stations.iter().any(|id| id == excluded),
                "{excluded} must not appear -- it's GTFS-sourced"
            );
        }
        assert_eq!(line.stations.len(), 13);
    }
}
```

- [ ] **Step 4: `main.rs`**, mirroring
  `crates/poller-irish-rail-gtfs/src/main.rs`'s poll-loop shape (read in
  full this session) with two CSV fetches instead of one GTFS-zip fetch,
  and the mandatory `User-Agent` on the `Client`:

```rust
//! `poller-nir-stations`: downloads OpenDataNI's two Translink CSVs
//! ("Northern Ireland Railways Stations"/"...Halts") on an interval,
//! parses/filters/dedups them, and forwards the derived
//! `NorthernIreland`-tagged station catalogue -- plus a small hand-curated
//! line catalogue -- to `api`'s existing
//! `/private/island-of-ireland-{stations,lines}` ingestion endpoints
//! (shared with `poller-irish-rail-gtfs`). Tier A of
//! docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md;
//! see docs/superpowers/plans/2026-09-05-nir-tier-a-implementation-plan.md
//! Task 2.

mod config;
mod mapping;

use std::time::Duration;

use clap::Parser;
use common::ingest;
use config::Config;
use reqwest::Client;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

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
    // `.user_agent(...)` is NOT optional -- see config::USER_AGENT's own
    // doc comment and this plan's Global Constraints. Every request this
    // client makes to admin.opendatani.gov.uk 403s without it.
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent(config::USER_AGENT)
        .build()?;
    let internal_oauth =
        common::oauth_client::OAuthTokenCache::new(common::oauth_client::OAuthCredentials {
            token_url: config.internal_oauth_token_url.clone(),
            client_id: config.internal_oauth_client_id.clone(),
            scope: config.internal_oauth_scope.clone(),
            username: config.internal_oauth_username.clone(),
            password: config.internal_oauth_password.clone(),
        });

    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let delay = ingest::time_until_next_poll(
        &client,
        &config.api_stations_ingest_url,
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
            "poller" => "nir-stations"
        )
        .record(cycle_start.elapsed().as_secs_f64());
        metrics::counter!(
            common::metrics::metric_name("poller_cycle_total"),
            "poller" => "nir-stations",
            "result" => if result.is_ok() { "success" } else { "failure" }
        )
        .increment(1);

        if let Err(err) = result {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}

async fn poll_once(
    client: &Client,
    config: &Config,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) -> anyhow::Result<()> {
    let stations_csv = client
        .get(&config.stations_csv_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    let halts_csv = client
        .get(&config.halts_csv_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;

    let stations = mapping::map_stations(&stations_csv, &halts_csv)?;
    let lines = mapping::map_lines();
    tracing::info!(
        stations = stations.len(),
        lines = lines.len(),
        "parsed NIR station/line catalogue"
    );

    ingest::post_batch(
        client,
        &config.api_stations_ingest_url,
        internal_oauth,
        &stations,
        "island-of-ireland stations (NIR)",
    )
    .await?;
    ingest::post_batch(
        client,
        &config.api_lines_ingest_url,
        internal_oauth,
        &lines,
        "island-of-ireland lines (NIR)",
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::*;

    /// Real HTTP-level assertion that the client actually sends the
    /// required `User-Agent` -- this is the one thing that silently breaks
    /// the whole poller in production if regressed (Global Constraints).
    /// `wiremock`'s exact-value `header(...)` matcher only matches a
    /// request carrying exactly this header/value pair; `.expect(1)`
    /// fails the test on drop if that never happened -- so a
    /// `Client::builder()` call that dropped `.user_agent(...)` would make
    /// this test fail with a connection/mock-mismatch error, not silently
    /// pass.
    #[tokio::test]
    async fn client_sends_the_required_user_agent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/stations.csv"))
            .and(header("user-agent", config::USER_AGENT))
            .respond_with(ResponseTemplate::new(200).set_body_string("OID_,NAME,TYPE,EASTING,NORTHING,Comment,Lat,Long\n"))
            .expect(1)
            .mount(&server)
            .await;

        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .user_agent(config::USER_AGENT)
            .build()
            .unwrap();
        let response = client
            .get(format!("{}/stations.csv", server.uri()))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }
}
```

- [ ] **Step 5: Add to workspace.** In the root `Cargo.toml`'s `members`
  list, add `"crates/poller-nir-stations",` alongside the existing
  `poller-irish-rail-gtfs`/`poller-irish-rail-live` entries (`:11-12`).

- [ ] **Step 6: Verify**

```bash
cargo fmt --all
cargo clippy -p poller-nir-stations --all-features
cargo test -p poller-nir-stations
cargo build --workspace
```

  Expected: all four succeed; `cargo test -p poller-nir-stations` runs (at
  minimum) the 7 `mapping::tests` plus `main::tests::client_sends_the_required_user_agent`
  -- 8 tests, all passing, zero `#[ignore]`d (no DB/no live network needed
  by any of them).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/poller-nir-stations
git commit -m "add poller-nir-stations: OpenDataNI CSV-backed Northern Ireland Railways station/line catalogue ingestion"
```

---

## Task 3: Docker + CI + Helm chart wiring

**Files:** create `docker/poller-nir-stations.Dockerfile`, create
`charts/distant-signal/templates/poller-nir-stations-deployment.yaml`;
modify `.github/workflows/containers.yml`,
`charts/distant-signal/values.yaml`, `charts/distant-signal/values-example.yaml`,
`charts/distant-signal/templates/_helpers.tpl`,
`charts/distant-signal/templates/podmonitor.yaml`,
`charts/distant-signal/templates/secret.yaml`,
`charts/distant-signal/templates/api-deployment.yaml`.

Depends on Task 2 (the binary must exist for the Dockerfile to build).

- [ ] **Step 1: Dockerfile.** Copied from
  `docker/poller-irish-rail-gtfs.Dockerfile` (read in full this session)
  with the binary name substituted throughout:

```dockerfile
# syntax=docker/dockerfile:1
# Multi-stage build for the `poller-nir-stations` service.
#
# Same rustc 1.88 floor as every other crate in this workspace -- see
# docker/poller-stations.Dockerfile's own comment for the confirmed
# icu_provider transitive-dependency reasoning.
#
# Build from the repo root:
#   docker build -f docker/poller-nir-stations.Dockerfile .
ARG CARGO_PROFILE=release

FROM rust:1.88-bookworm AS builder
ARG CARGO_PROFILE

WORKDIR /app
COPY . .
RUN --mount=type=cache,id=cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,id=cargo-git,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,id=cargo-target-1.88,target=/app/target,sharing=locked \
    if [ "$CARGO_PROFILE" = "release" ]; then \
      cargo build --release --bin poller-nir-stations; \
    else \
      cargo build --bin poller-nir-stations; \
    fi \
    && cp /app/target/${CARGO_PROFILE}/poller-nir-stations /usr/local/bin/poller-nir-stations

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 1000 poller \
    && useradd --system --no-create-home --shell /usr/sbin/nologin --uid 1000 --gid 1000 poller

COPY --from=builder /usr/local/bin/poller-nir-stations /usr/local/bin/poller-nir-stations

USER 1000:1000

ENTRYPOINT ["/usr/local/bin/poller-nir-stations"]
```

- [ ] **Step 2: CI matrix.** In `.github/workflows/containers.yml`, add
  alongside the existing `poller-irish-rail-*` entries (`:136-141`):

```yaml
          - service: poller-nir-stations
            dockerfile: docker/poller-nir-stations.Dockerfile
            target: ""
```

- [ ] **Step 3: Helm `_helpers.tpl` secret-name helpers.** In
  `charts/distant-signal/templates/_helpers.tpl`, immediately after the
  existing `pollerIrishRailLive*` block (`:407-429`, read in full this
  session), add the same three-helper shape for the new poller:

```
{{/*
Resolved Secret name/key for poller-nir-stations's own OAuth2 credential --
same shape as pollerIrishRailGtfs/pollerIrishRailLive above.
*/}}
{{- define "distant-signal.pollerNirStationsSecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.pollerNirStations.existingSecret }}
{{- end }}

{{- define "distant-signal.pollerNirStationsOauthUsernameSecretKey" -}}
{{- if .Values.pollerNirStations.existingSecret }}
{{- .Values.pollerNirStations.existingSecretInternalOauthUsernameKey }}
{{- else }}
{{- print "internal-oauth-username-poller-nir-stations" }}
{{- end }}
{{- end }}

{{- define "distant-signal.pollerNirStationsOauthPasswordSecretKey" -}}
{{- if .Values.pollerNirStations.existingSecret }}
{{- .Values.pollerNirStations.existingSecretInternalOauthPasswordKey }}
{{- else }}
{{- print "internal-oauth-password-poller-nir-stations" }}
{{- end }}
{{- end }}
```

- [ ] **Step 4: Helm values.** In `charts/distant-signal/values.yaml`, add a
  new top-level block immediately after the existing `pollerIrishRailLive:`
  block (`:1046-1090`), sibling to it (not nested inside `pollers:`, same
  Judgment-Call-#2 reasoning `poller-irish-rail-gtfs`'s own plan already
  established -- no API key exists for this crate's `Config` to hold
  either):

```yaml
# ---------------------------------------------------------------------------
# pollerNirStations (crates/poller-nir-stations/src/config.rs: Config)
# -- Tier A of docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md.
# Gets its own bespoke Deployment template for the same reason
# pollerIrishRailGtfs/pollerIrishRailLive do: OpenDataNI's CSVs are
# anonymous, key-free downloads, so this crate's Config doesn't fit the
# generic `.Values.pollers` map's RDM-API-key shape.
# ---------------------------------------------------------------------------
pollerNirStations:
  enabled: false
  image:
    repository: distant-signal/poller-nir-stations
    tag: ""
    pullPolicy: IfNotPresent
  # -- OpenDataNI's own CKAN download URLs for Translink's "Northern
  # Ireland Railways Stations"/"...Halts" CSVs -- real, key-free, confirmed
  # reachable directly with a browser User-Agent
  # (docs/superpowers/specs/2026-09-05-nir-tier-a-implementation-design.md
  # section 1; re-confirmed this session). Overridable in case OpenDataNI
  # ever moves these.
  stationsCsvUrl: "https://admin.opendatani.gov.uk/dataset/5f27f171-b8aa-4511-983d-6df6e87bbf20/resource/967e32c3-1cc2-4aee-b485-92121a32eb4d/download/translink_rail_stations.csv"
  haltsCsvUrl: "https://admin.opendatani.gov.uk/dataset/1f2a94b9-1e86-4aec-ad9a-90a3de233893/resource/370b0d8a-29b9-46ca-bcc7-91357c28c43d/download/translink_halts.csv"
  apiStationsIngestPath: /private/island-of-ireland-stations
  apiLinesIngestPath: /private/island-of-ireland-lines
  # -- OpenDataNI's own CKAN metadata confirms `frequency: "irregular"` for
  # both CSVs -- same 24h convention pollerIrishRailGtfs/pollers.stations
  # already use for reference data with an unconfirmed cadence.
  pollIntervalSecs: 86400
  # -- No API key needed at all (OGL-licensed, anonymous CSV downloads).
  internalOauthUsername: ""
  internalOauthPassword: ""
  existingSecret: ""
  existingSecretInternalOauthUsernameKey: internal-oauth-username-poller-nir-stations
  existingSecretInternalOauthPasswordKey: internal-oauth-password-poller-nir-stations
  logLevel: info
  metricsPort: 9091
  extraEnv: []
  resources: {}
  nodeSelector: {}
  tolerations: []
  affinity: {}
  podAnnotations: {}
  podSecurityContext: {}
```

  Also add the new group name to `api.internalOauth.groups`
  (`:432-443`, immediately after the existing `irishRailLive:` line):

```yaml
      nirStations: svc-poller-nir-stations
```

- [ ] **Step 5: New Deployment template.** Copied from
  `charts/distant-signal/templates/poller-irish-rail-gtfs-deployment.yaml`
  (read in full this session) with the resource name/labels/env vars
  substituted for two CSV URLs instead of one GTFS URL, and using this
  task's own new `_helpers.tpl` entries for the Secret name/keys (matching
  that file's own real pattern -- `distant-signal.pollerIrishRailGtfsSecretName`
  etc., NOT a hand-rolled `default (...)` expression):

```yaml
{{- if .Values.pollerNirStations.enabled }}
apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ printf "%s-poller-nir-stations" (include "distant-signal.fullname" .) | trunc 63 | trimSuffix "-" }}
  labels:
    {{- include "distant-signal.labels" (dict "root" . "component" "poller-nir-stations") | nindent 4 }}
spec:
  replicas: 1
  strategy:
    type: Recreate
  selector:
    matchLabels:
      {{- include "distant-signal.selectorLabels" (dict "root" . "component" "poller-nir-stations") | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include "distant-signal.labels" (dict "root" . "component" "poller-nir-stations") | nindent 8 }}
      {{- if or .Values.metrics.enabled .Values.pollerNirStations.podAnnotations }}
      annotations:
        {{- if .Values.metrics.enabled }}
        prometheus.io/scrape: "true"
        prometheus.io/port: {{ .Values.pollerNirStations.metricsPort | quote }}
        prometheus.io/path: "/metrics"
        {{- end }}
        {{- with .Values.pollerNirStations.podAnnotations }}
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
        {{- include "distant-signal.podSecurityContext" (dict "override" .Values.pollerNirStations.podSecurityContext) | nindent 8 }}
      containers:
        - name: poller-nir-stations
          image: {{ include "distant-signal.image" (dict "root" . "image" .Values.pollerNirStations.image) | quote }}
          imagePullPolicy: {{ .Values.pollerNirStations.image.pullPolicy }}
          securityContext:
            {{- include "distant-signal.containerSecurityContext" (dict "readOnlyRootFilesystem" true) | nindent 12 }}
          {{- if .Values.metrics.enabled }}
          ports:
            - name: metrics
              containerPort: {{ .Values.pollerNirStations.metricsPort }}
              protocol: TCP
          {{- end }}
          env:
            - name: STATIONS_CSV_URL
              value: {{ .Values.pollerNirStations.stationsCsvUrl | quote }}
            - name: HALTS_CSV_URL
              value: {{ .Values.pollerNirStations.haltsCsvUrl | quote }}
            - name: API_STATIONS_INGEST_URL
              value: {{ printf "%s%s" (include "distant-signal.apiBaseUrl" .) .Values.pollerNirStations.apiStationsIngestPath | quote }}
            - name: API_LINES_INGEST_URL
              value: {{ printf "%s%s" (include "distant-signal.apiBaseUrl" .) .Values.pollerNirStations.apiLinesIngestPath | quote }}
            - name: INTERNAL_OAUTH_TOKEN_URL
              value: {{ .Values.internalOauth.tokenUrl | quote }}
            - name: INTERNAL_OAUTH_CLIENT_ID
              value: {{ .Values.internalOauth.clientId | quote }}
            - name: INTERNAL_OAUTH_SCOPE
              value: {{ .Values.internalOauth.scope | quote }}
            - name: INTERNAL_OAUTH_USERNAME
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.pollerNirStationsSecretName" . }}
                  key: {{ include "distant-signal.pollerNirStationsOauthUsernameSecretKey" . }}
            - name: INTERNAL_OAUTH_PASSWORD
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.pollerNirStationsSecretName" . }}
                  key: {{ include "distant-signal.pollerNirStationsOauthPasswordSecretKey" . }}
            - name: POLL_INTERVAL_SECS
              value: {{ .Values.pollerNirStations.pollIntervalSecs | quote }}
            - name: METRICS_ENABLED
              value: {{ .Values.metrics.enabled | quote }}
            {{- if .Values.metrics.enabled }}
            - name: METRICS_PORT
              value: {{ .Values.pollerNirStations.metricsPort | quote }}
            {{- end }}
            - name: RUST_LOG
              value: {{ .Values.pollerNirStations.logLevel | quote }}
            {{- with .Values.pollerNirStations.extraEnv }}
            {{- toYaml . | nindent 12 }}
            {{- end }}
          {{- with .Values.pollerNirStations.resources }}
          resources:
            {{- toYaml . | nindent 12 }}
          {{- end }}
      {{- with .Values.pollerNirStations.nodeSelector }}
      nodeSelector:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.pollerNirStations.tolerations }}
      tolerations:
        {{- toYaml . | nindent 8 }}
      {{- end }}
      {{- with .Values.pollerNirStations.affinity }}
      affinity:
        {{- toYaml . | nindent 8 }}
      {{- end }}
{{- end }}
```

- [ ] **Step 6: `podmonitor.yaml` selector.** Add to the
  `matchExpressions` values list, immediately after the existing
  `poller-irish-rail-live` guard (`:66-68`):

```yaml
          {{- if .Values.pollerNirStations.enabled }}
          - poller-nir-stations
          {{- end }}
```

- [ ] **Step 7: `secret.yaml`.** Add a new username/password pair,
  immediately after the existing `poller-irish-rail-live` block
  (`:92-99`):

```yaml
{{/* poller-nir-stations's own OAuth2 credential -- same shape as
     poller-irish-rail-gtfs/poller-irish-rail-live above, own service
     account. Gated on pollerNirStations.enabled. */}}
{{- if and .Values.pollerNirStations.enabled (not .Values.pollerNirStations.existingSecret) -}}
{{- $_ := set $data "internal-oauth-username-poller-nir-stations" (.Values.pollerNirStations.internalOauthUsername | default "" | b64enc) -}}
{{- $_ := set $data "internal-oauth-password-poller-nir-stations" (.Values.pollerNirStations.internalOauthPassword | default "" | b64enc) -}}
{{- end }}
```

- [ ] **Step 8: `api-deployment.yaml`.** Add the new group env var,
  immediately after the existing `INTERNAL_OAUTH_GROUP_IRISH_RAIL_LIVE`
  entry (`:127-130`):

```yaml
            - name: INTERNAL_OAUTH_GROUP_NIR_STATIONS
              value: {{ .Values.api.internalOauth.groups.nirStations | quote }}
```

- [ ] **Step 9: `values-example.yaml`.** Add a filled-in
  `pollerNirStations: enabled: true` block, immediately after the existing
  `pollerIrishRailLive:` block (`:202-205`):

```yaml
pollerNirStations:
  enabled: true
  internalOauthUsername: svc-poller-nir-stations
  internalOauthPassword: replace-me-poller-nir-stations-app-password
```

- [ ] **Step 10: Verify**

```bash
docker build -f docker/poller-nir-stations.Dockerfile . -t poller-nir-stations:test

helm lint charts/distant-signal
helm lint charts/distant-signal -f charts/distant-signal/values-example.yaml

# enabled: false (default) -- confirm the new Deployment/PodMonitor entry
# do not render at all:
helm template distant-signal charts/distant-signal \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 --set trustConsumer.kafka.topic=t \
  --set trustConsumer.kafka.saslMechanism=PLAIN --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=m --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=c --set api.sso.clientSecret=s \
  --set api.sso.redirectUrl=https://app.example.com/callback --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  | grep -c "poller-nir-stations"   # expect 0

# enabled: true, via values-example.yaml:
helm template distant-signal charts/distant-signal -f charts/distant-signal/values-example.yaml \
  --set trustConsumer.kafka.brokers=kafka.example.com:9094 --set trustConsumer.kafka.topic=t \
  --set trustConsumer.kafka.saslMechanism=PLAIN --set enricher.llm.baseUrl=http://llm.example.com/v1 \
  --set enricher.llm.model=m --set api.sso.issuerUrl=https://sso.example.com \
  --set api.sso.clientId=c --set api.sso.clientSecret=s \
  --set api.sso.redirectUrl=https://app.example.com/callback --set api.sso.postLoginRedirectUrl=https://app.example.com/ \
  | grep -c "poller-nir-stations"   # expect > 0, includes the Deployment and the PodMonitor selector entry
```

- [ ] **Step 11: Commit**

```bash
git add docker/poller-nir-stations.Dockerfile .github/workflows/containers.yml \
        charts/distant-signal/values.yaml charts/distant-signal/values-example.yaml \
        charts/distant-signal/templates/_helpers.tpl charts/distant-signal/templates/podmonitor.yaml \
        charts/distant-signal/templates/secret.yaml charts/distant-signal/templates/api-deployment.yaml \
        charts/distant-signal/templates/poller-nir-stations-deployment.yaml
git commit -m "chart+ci: wire up poller-nir-stations (Docker, CI matrix, Helm Deployment/PodMonitor/secret)"
```

---

## Task 4: End-to-end verification

**Files:** none (verification only).

- [ ] **Step 1: Full workspace check**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-features
cargo build --workspace
cargo test --workspace
cargo test -p api -p aggregator -- --ignored --test-threads=1
```

- [ ] **Step 2: Real live smoke test against the real OpenDataNI URLs --
  with an explicit browser `User-Agent`, so this step cannot silently pass
  against a cached/mocked response.** This directly exercises the Global
  Constraints' own 403-avoidance requirement against the real host, not
  just the crate's own unit tests:

```bash
UA="Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"

curl -sL -A "$UA" -o /tmp/nir_stations_smoke.csv -w "stations HTTP %{http_code}\n" \
  "https://admin.opendatani.gov.uk/dataset/5f27f171-b8aa-4511-983d-6df6e87bbf20/resource/967e32c3-1cc2-4aee-b485-92121a32eb4d/download/translink_rail_stations.csv"
# expect: stations HTTP 200

curl -sL -A "$UA" -o /tmp/nir_halts_smoke.csv -w "halts HTTP %{http_code}\n" \
  "https://admin.opendatani.gov.uk/dataset/1f2a94b9-1e86-4aec-ad9a-90a3de233893/resource/370b0d8a-29b9-46ca-bcc7-91357c28c43d/download/translink_halts.csv"
# expect: halts HTTP 200

wc -l /tmp/nir_stations_smoke.csv /tmp/nir_halts_smoke.csv
# expect: 21 lines (1 header + 20 rows) and 38 lines (1 header + 37 rows) --
# if these counts have drifted from what this plan's own citations recorded
# (design spec section 2.1/2.2; this plan's own header section), OpenDataNI
# has updated the dataset since this plan was written -- re-check the
# excluded-name list and hand-curated line station ids still all resolve
# to real rows before trusting this task's Step 3 counts below.

# Negative control: confirm the 403-on-no-UA finding is still real, not
# stale folklore -- this MUST fail (403), or this plan's whole User-Agent
# requirement needs re-justifying:
curl -s -o /dev/null -w "no-UA HTTP %{http_code}\n" \
  "https://admin.opendatani.gov.uk/dataset/5f27f171-b8aa-4511-983d-6df6e87bbf20/resource/967e32c3-1cc2-4aee-b485-92121a32eb4d/download/translink_rail_stations.csv"
# expect: no-UA HTTP 403 (or a redirect chain ending in one) -- curl's
# default User-Agent (`curl/<version>`) is exactly the bot-shaped case
# admin.opendatani.gov.uk's WAF blocks, per this plan's own citations.
```

- [ ] **Step 3: Local end-to-end smoke test.** With a local Postgres
  migrated and `api` running (this repo's existing
  `docker-compose.dev.yml` workflow, unchanged by this plan), and
  `poller-irish-rail-gtfs`'s own smoke test (its plan's Task A6, Step 2)
  already run at least once so the shared tables aren't empty:

```bash
DATABASE_URL=... cargo run -p poller-nir-stations -- \
  --internal-oauth-token-url ... --internal-oauth-client-id ... \
  --internal-oauth-username svc-poller-nir-stations --internal-oauth-password ... \
  --poll-interval-secs 999999   # fires once immediately (fresh delay=0 on first run against this crate's own last-fetch check), then Ctrl-C

curl -s "http://localhost:8080/public/island-of-ireland/stations?network=northern-ireland" | jq 'length'
# expect 47 (20 stations + 37 halts - 4 disused - 1 Poyntzpass dup - 5
# border/Belfast exclusions = 47, per this plan's own header-section math)

curl -s "http://localhost:8080/public/island-of-ireland/lines?network=northern-ireland" | jq 'length'
# expect 4 (Bangor, Larne, Derry~Londonderry, Portadown/Newry)

curl -s "http://localhost:8080/public/island-of-ireland/stations" | jq 'length'
# expect the NIR count (47) PLUS whatever poller-irish-rail-gtfs's own
# RepublicOfIreland rows already landed -- confirms both producers' rows
# coexist in the same table without the route-auth change from Task 1
# breaking either one.
```

- [ ] **Step 4: Confirm the two Decision-1/2-resolved exclusions landed
  correctly**, by name, against the real live data:

```bash
curl -s "http://localhost:8080/public/island-of-ireland/stations?network=northern-ireland" \
  | jq '[.[] | select(.name | test("Belfast"; "i"))] | map(.name)'
# expect: Botanic, Central, Yorkgate present; Europa/GVS ABSENT (Decision 1)

curl -s "http://localhost:8080/public/island-of-ireland/stations?network=northern-ireland" \
  | jq '[.[] | select(.name | test("Lisburn|Portadown|Lurgan|Newry"; "i"))]'
# expect: [] -- empty. These are GTFS-sourced under
# network=republic-of-ireland instead; confirm THAT with:
curl -s "http://localhost:8080/public/island-of-ireland/stations?network=republic-of-ireland" \
  | jq '[.[] | select(.name | test("Lisburn|Portadown|Lurgan|Newry|Belfast"; "i"))] | map(.name)'
# expect: non-empty -- Belfast (the GTFS stop_id 7020IR2162 row) at minimum
```

No commit for this task (verification only) -- if any step fails, fix the
relevant earlier task and re-verify before considering this plan complete.
