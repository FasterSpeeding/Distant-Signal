# Plan: Dynamic Trip Planning — Phase 1: CIF Interchange Ingestion (ALF + MSN Change Time)

> **For agentic workers:** REQUIRED SUB-SKILL: use
> superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** implement Phase 1 of the six-phase breakdown in
`docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md` §8: give
this app, for the first time, real interchange data — how long a change
takes at one station, and which pairs of *differently-named* stations are
connected by a walk/tube/bus/tram/ferry fixed link — sourced from the CIF
`MSN` member's column-65 minimum-change-time field (already read into memory
every 30 minutes but never parsed past byte 52) and the CIF `ALF` member
(not fetched by this app's ingestion at all today). **No search algorithm,
no new route, no frontend** — this phase's own deliverable is real,
queryable interchange data, independently useful and independently testable
against real `timetable_full.zip` data before Phase 2 builds a connections
array on top of it.

**Architecture:** three small, sequential pieces inside the existing
`schedulefeed` Pod's `schedule-reference` container, plus the `api`-side
storage they publish to:

1. `crates/schedule-reference/src/discovery.rs` gains an **optional**
   `alf_path` on `CompleteDelivery` (Task 1) — optional, not required,
   which is a deliberate, load-bearing deviation from the design spec's own
   §8 Phase 1 wording ("extending `CompleteDelivery` to also require... a
   `RJTTF*ALF.txt` file"); see Judgment Call 1 for why.
2. `crates/schedule-reference/src/parser.rs` gains a small extension to read
   the MSN `A` record's minimum-change-time field (Task 2), and a new
   sibling module `crates/schedule-reference/src/alf.rs` parses the `ALF`
   member's own, structurally different (comma `key=value`, not fixed-width)
   record format (Task 3).
3. A new `fixed_links` table and a new `change_time_minutes` column on the
   existing `stanox_crs` table (Task 4) carry both outward to `api` via two
   extended/new `POST /private/...` routes (Task 5), mirroring the existing
   `upsert_stanox_crs` full-refresh pattern. `main.rs` wiring (Task 6) reads
   the optional ALF file if present, parses both new data sources, and POSTs
   them alongside the existing STANOX/CRS publish.

**Tech stack:** Rust (`schedule-reference`, `api`, `common`), sqlx/Postgres
migrations. No `schedule-ingest` change of any kind — `schedule-ingest`'s
`extract_zip` (`crates/schedule-ingest/src/delivery.rs:124-145`) already
extracts **every** regular-file entry of the delivery zip generically, so if
a real delivery's zip contains an `RJTTFnnnALF.txt` member, it is already
sitting on disk in every delivery directory today, unread. This phase reads
it; it does not need to start extracting it.

**Spec:** `docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md`
§0.2, §0.4, §4, §8 Phase 1 — authoritative for scope. This plan resolves the
concrete judgment calls that section leaves to "the implementation plan"
(exact byte offsets, exact table shapes, ALF-optionality, error-handling
posture for a comma-format file in a fixed-width-format codebase).

---

## Judgment calls this plan makes (read before Task 1)

1. **ALF presence is OPTIONAL on `CompleteDelivery`, not required, contrary
   to the design spec's own literal §8 Phase 1 wording.** The spec's own
   provenance notes are explicit that its sibling-project citations were
   *not* re-verified this pass because that project's clone was unavailable
   (design spec, top-of-document provenance note). This plan's own research
   pass **did** re-clone `Distant-Signal-MCP`
   (`ssh://git@git-bringer-ssh.fox-prometheus.ts.net/lucy/Distant-Signal-MCP.git`)
   and confirms every sibling-project number the design spec cites — real
   counts, re-read directly from that project's own design doc
   (`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:62-63,79`):
   26,848 schedules, 316,362 public calling points, ~289,514 connections on
   a real weekday, and 4,222 ALF fixed links (1,772 metro, 1,600 tube, 557
   transfer, 237 walk, 50 bus, 4 tram, 2 ferry). What this pass could
   **not** verify is whether *this app's own* real, live `timetable_full.zip`
   delivery actually contains an `RJTTFnnnALF.txt` member at all — that
   requires a real delivery on a real cluster, unavailable to this planning
   pass. Making `alf_path` a **required** field of `CompleteDelivery` (the
   design spec's literal phrasing) means: if that assumption is wrong for
   even one delivery, `latest_complete_delivery` returns `None` for it, and
   the *entire, currently-working* STANOX/CRS + `schedule_line_population` +
   `schedule_destination_departures` pipeline silently stalls — a
   production regression in three already-shipped features, caused by a
   brand-new, unverified assumption about a fourth. `alf_path:
   Option<PathBuf>` (Task 1) keeps the existing MCA+MSN completeness
   contract byte-for-byte unchanged and adds fixed-links publishing as
   **best-effort, log-and-skip** when the file isn't found — the same
   "degrade this one product, never the whole cycle" posture
   `publish_cif_derived_products`'s own per-product try/log/continue
   structure (`main.rs:275-340`) already establishes for its three existing
   products.

2. **Do not copy the sibling project's exact MSN byte columns (1-indexed
   64-65) verbatim into this app's parser — independently verified this
   pass to produce a wrong-looking value against this app's own real,
   already-tested MSN fixture.** `crates/schedule-reference/src/parser.rs`'s
   own `A_WATRLMN` test fixture (`parser.rs:223`) is a real, byte-verified
   line this app's tests already assert against
   (`extracts_tiploc_to_crs_from_a_real_a_record`). Applying the sibling's
   own documented byte range (`src/timetable/cif/msn.ts:56`: "64-65 minimum
   change time, minutes", 1-indexed inclusive → 0-indexed `[63,65)`) to that
   *exact* fixture this pass yields `"15"` — outside the sibling's own
   documented typical range ("range 0–9", `DEFAULT_CHANGE_TIME = 5`,
   `2026-07-22-train-mcp-phase2b-journey-planner-design.md:74`). This isn't
   a fluke: the two codebases' MSN column math for the CRS field itself
   *also* disagrees on this same fixture — this app's own tested,
   production `49..52` (`parser.rs:74`, doc: "`49..52` CRS") versus the
   sibling's documented `44-46` (1-indexed) → 0-indexed `[43,46)`
   (`msn.ts:56`) — a 6-byte offset difference. Both ranges happen to read
   `"WAT"` off this *particular* fixture only because the real bytes at
   `[43,49)` are `"WAT   "` (the CRS value, apparently repeated with
   padding, per this app's own as-yet-undocumented gap between the TIPLOC
   field and its own documented CRS field) — a coincidence that masks the
   disagreement for CRS but not for change-time, where only one of the two
   candidate ranges can be right. **Conclusion: the two codebases' MSN
   extracts (or their column-counting conventions) are not directly
   transchargeable byte-for-byte.** Task 2 below does NOT hardcode a byte
   range up front; it derives one empirically against this app's own real
   MSN bytes and sanity-checks the resulting *distribution* against the
   sibling's own independently-measured real-world shape (mostly 0-9,
   modal 5, a handful of 98/99 sentinels) before shipping it — the same
   "no invented API details, verify against real bytes" convention this
   crate's own module doc already states (`parser.rs:1-13`).

3. **`change_time_minutes` is stored as a raw, un-interpreted
   `Option<i32>` on `stanox_crs`, not pre-resolved to "effective minutes
   with defaults/sentinels applied."** The 98/99 "not a real rail
   interchange" sentinel and the "no MSN record at all → assume the modal
   5-minute default" fallback (both real, sibling-confirmed conventions,
   `interchange.ts:5-54`) are **read-time** policy, not ingestion-time
   policy — Phase 2's connections-array builder is the natural, single
   place that turns "no interchange data recorded" and "recorded as 98/99"
   into a search-usable value, exactly mirroring this app's own established
   convention of storing a raw CIF-derived value and applying policy at
   read time rather than ingest time (e.g. `schedule_destination_departures`
   stores every departure uncapped and unfiltered; `GET
   /public/trains/search` applies the `now`-forward filter at request time,
   `main.rs:578-591`'s own doc comment). Storing `NULL` for "no MSN record
   matched this TIPLOC" (not `5`, not `0`) keeps that distinction visible
   all the way to Phase 2, rather than baking a policy choice into data at
   rest where a later change would need a backfill migration to fix.

4. **`fixed_links` is a small, wholesale-replaced table (`DELETE` +
   `INSERT` in one transaction per cycle), not a per-row `ON CONFLICT`
   upsert like `stanox_crs`.** Unlike a STANOX row (naturally keyed by its
   own `stanox` value), a real ALF row has no natural stable per-row key —
   the same physical link (e.g. Euston↔King's Cross) is published as
   several rows differing only in mode/validity window, per the sibling's
   own measurement (`interchange.ts:125-127`: "Euston -> King's Cross alone
   carries eight rows"). At ~4,222 rows total (sibling's real count,
   confirmed above), a full transactional replace on every publish cycle is
   trivial cost and avoids inventing a synthetic composite key this data
   has no natural analogue for. This mirrors `schedule_destination_departures`'s
   own "this publish is a from-scratch rebuild of the whole product every
   cycle" posture (`main.rs`'s own doc: "`DELETE ... WHERE service_date =
   $1`"), simplified further here since `fixed_links` has no date dimension
   to scope the delete by.

5. **ALF parse errors are logged-and-skipped per malformed line, not
   thrown, diverging from the sibling project's own `throw`-on-missing-field
   posture (`alf.ts:33`).** This app's own two existing CIF parsers
   (`parse_ti_lines`, `parse_msn_a_lines`) both already establish "a
   malformed real-looking line is skipped, never aborts the whole
   extraction" as this *codebase's* convention (`parser.rs:29-30`: "A line
   shorter than the fixed 80-byte real record shape is skipped, not a hard
   error"). A new parser in this codebase should match this codebase's own
   established error posture, not import a different one from a project
   with a different overall philosophy, even when porting that project's
   record-shape knowledge directly.

6. **The ALF `P` field (present in every real quoted row, e.g.
   `P=4`) is deliberately left unparsed.** Neither this plan nor the
   sibling project's own `Transfer` type (`alf.ts:46-54`) has a use for it
   — the sibling's own `fixedLinks` read-side logic picks the shortest
   `minutes` among validity-matching rows, never consulting a priority
   field. Not decoding an unused field matches this crate's own stated
   convention of leaving "fields this crate has no real-data-verified use
   for... undecoded rather than guessed at" (`schedule-query/src/records.rs:11-12`,
   applied here to a sibling crate written in the same spirit).

---

## Non-goals

- **No connections array, no CSA, no RAPTOR, no new `api` read route, no
  frontend change of any kind.** This phase's entire deliverable is data at
  rest in two Postgres tables, queryable by direct SQL for verification.
  Phase 2 is what consumes it.
- **No resolution of 98/99 sentinels or the 5-minute default into stored
  data** — see Judgment Call 3. `stanox_crs.change_time_minutes` stores
  the raw parsed value or `NULL`, nothing else.
- **No `schedule-ingest` change.** See this plan's own Architecture section
  — `extract_zip` already generically extracts every zip member.
- **No ALF `P` field, no CIF `AA` record, no freight-specific field** — see
  Judgment Call 6 and this crate's own established "undecoded unless a real
  use is verified" convention.
- **No change to `schedules_touching`/`departures_by_crs`/
  `departures_by_destination_crs`** (`crates/schedule-query`) — none of
  Phase 1's new data flows through `ScheduleIndex` at all; it is an
  entirely separate, parallel product of the same 30-minute cycle.

## Global Constraints

- **File scope.** Created/modified:
  `crates/schedule-reference/src/discovery.rs`,
  `crates/schedule-reference/src/parser.rs`,
  `crates/schedule-reference/src/alf.rs` (new),
  `crates/schedule-reference/src/main.rs`,
  `crates/schedule-reference/src/config.rs`,
  `crates/schedule-reference/examples/msn_change_time_probe.rs` (new,
  dev-only, not part of `cargo test`),
  `crates/common/src/lib.rs`,
  `crates/api/migrations/20260923090000_fixed_links_and_change_time.sql`
  (new; `ls crates/api/migrations | sort | tail -1` at the time this plan
  was written showed `20260922130000_journey_leg_notification_state.sql` as
  the latest, so this timestamp sorts after it),
  `crates/api/src/routes/ingest.rs`,
  `crates/api/src/data/queries.rs`.
  No other file changes.
- **Testing.** Rust: `cargo fmt --all`, `cargo clippy --workspace
  --all-features --all-targets -- -D warnings` (matches
  `.github/workflows/ci.yml`'s `clippy` job, which pins
  `auguwu/clippy-action@9817d076b82df0194935be9db6154c56ac07b317` with
  `check-args: --all-features --all-targets`), `cargo test --workspace`
  (ignored tests skipped — CI's own unconditional default,
  `.github/workflows/ci.yml`'s `rust-test` job), and `cargo test -p api --
  --ignored --test-threads=1` for every DB-gated test this plan adds
  (CI's own exact invocation, same job, requiring
  `DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres`
  against a real local Postgres). Every new `sqlx::query`/`query_as!` call
  in `crates/api` must be exercised by `sqlx migrate run` in
  `crates/api/` before `cargo build` will accept it, per this crate's
  existing convention (this plan uses the non-macro `sqlx::query`/
  `sqlx::query_as::<_, T>` form throughout — matching `upsert_stanox_crs`'s
  own style — so no `cargo sqlx prepare` step is needed).
- **No invented API details.** Every byte offset this plan's own parser
  code relies on must be independently confirmed against either (a) a real
  quoted line already checked into this app's own tests, or (b) a fresh
  extraction from a real `timetable_full.zip`/live delivery, per this
  crate's own module-doc convention (`parser.rs:1-13`). Where Task 2 cannot
  fully resolve an offset without access to a live delivery this planning
  pass didn't have, the task says so explicitly and names the exact
  verification step the implementer must run before shipping.

## Review Focus

- **A delivery whose zip genuinely has no `ALF` member at all** (the exact
  risk Judgment Call 1 is about) — Task 1's own test must cover "delivery is
  still considered complete, STANOX/CRS still publishes, only fixed-links
  publishing is skipped with a log line," not just the happy path.
- **An MSN `A` record whose change-time field is a sentinel (98/99) or
  genuinely blank** — Task 2 must not conflate "blank" (`NULL`, no MSN
  record matched) with a sentinel (a real, present, out-of-range value) in
  what gets stored; both are real, different, non-`Err` inputs.
- **An ALF line matching a real fixed link's OWN comma-separated `key=value`
  shape but missing one required key** — Task 3's parser must skip it
  (Judgment Call 5), not panic the whole publish cycle, and must not
  silently treat a missing `T=` as `0` minutes (an instant, free transfer).
- **A blank line or a `/!!`-prefixed comment line inside the ALF member**
  (the sibling's own doc confirms both exist in a real extract, `alf.ts:18`)
  — must be silently skipped, not logged as a parse failure per line (that
  would spam the logs on every cycle for a normal, well-formed file).
- **Re-running the same delivery twice** (the existing `last_processed_delivery`
  dedup, `main.rs:111-117`) — must not double-publish `fixed_links` rows;
  since Task 4's design already fully replaces the table every publish, a
  second identical publish is idempotent by construction, but this should
  be asserted by a test, not assumed.

---

## Task 1: `discovery.rs` — optional `alf_path`, completeness contract unchanged

**Files:**
- Modify: `crates/schedule-reference/src/discovery.rs`

**Interfaces:**
- Produces: `CompleteDelivery.alf_path: Option<PathBuf>` — consumed by
  Task 6's `main.rs` wiring.

- [ ] **Step 1: Add the field and the lookup**, keeping `mca_path`/
  `msn_path` byte-for-byte unchanged:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteDelivery {
    pub dir_name: String,
    pub mca_path: PathBuf,
    pub msn_path: PathBuf,
    /// `None` when this delivery directory has no `RJTTF*ALF.txt`-shaped
    /// file -- deliberately NOT a completeness requirement (see this
    /// plan's Judgment Call 1): a delivery missing MCA or MSN is still
    /// skipped entirely (unchanged below), but a delivery missing ONLY
    /// ALF is still `Some(CompleteDelivery)`, with fixed-links publishing
    /// degrading gracefully for that one cycle (Task 6) rather than
    /// stalling the whole pipeline.
    pub alf_path: Option<PathBuf>,
}
```

  Update `latest_complete_delivery` to also look for the ALF file wherever
  it finds a complete MCA+MSN pair, without letting its absence affect the
  `if let (Some(mca_path), Some(msn_path)) = ...` completeness check at
  all:

```rust
    for name in dir_names.into_iter().rev() {
        let dir = storage_dir.join(&name);
        let mca_path = find_file_matching(&dir, "RJTTF", "MCA.txt")?;
        let msn_path = find_file_matching(&dir, "RJTTF", "MSN.txt")?;
        if let (Some(mca_path), Some(msn_path)) = (mca_path, msn_path) {
            let alf_path = find_file_matching(&dir, "RJTTF", "ALF.txt")?;
            return Ok(Some(CompleteDelivery {
                dir_name: name,
                mca_path,
                msn_path,
                alf_path,
            }));
        }
    }
```

- [ ] **Step 2: Update the existing test fixtures** that construct
  `CompleteDelivery` literals (`discovery.rs`'s own `tests` module,
  `picks_the_most_recent_delivery_dir_with_both_files_present` and
  `the_embedded_filename_number_is_irrelevant_to_which_delivery_wins`) to
  assert `alf_path: None` where the fixture never creates an ALF file —
  proving the existing MCA+MSN-only contract is genuinely unaffected:

```rust
        let delivery = latest_complete_delivery(dir.path()).unwrap().unwrap();
        assert_eq!(delivery.dir_name, "20260902T090000Z");
        assert_eq!(
            delivery.mca_path,
            dir.path().join("20260902T090000Z/RJTTF941MCA.txt")
        );
        assert_eq!(
            delivery.msn_path,
            dir.path().join("20260902T090000Z/RJTTF941MSN.txt")
        );
        assert_eq!(
            delivery.alf_path, None,
            "no ALF file was created in this fixture -- completeness must not depend on it"
        );
```

- [ ] **Step 3: Add two new tests** covering Review Focus's first bullet —
  a delivery WITH an ALF file resolves it, and a delivery missing ONLY ALF
  is still considered complete:

```rust
    #[test]
    fn a_delivery_with_all_three_files_resolves_the_alf_path_too() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("20260903T090000Z")).unwrap();
        touch(dir.path(), "20260903T090000Z", "RJTTF942MCA.txt");
        touch(dir.path(), "20260903T090000Z", "RJTTF942MSN.txt");
        touch(dir.path(), "20260903T090000Z", "RJTTF942ALF.txt");

        let delivery = latest_complete_delivery(dir.path()).unwrap().unwrap();
        assert_eq!(
            delivery.alf_path,
            Some(dir.path().join("20260903T090000Z/RJTTF942ALF.txt"))
        );
    }

    #[test]
    fn a_delivery_missing_only_alf_is_still_a_complete_delivery() {
        // The exact risk Judgment Call 1 exists to prevent: a real
        // delivery whose zip has no ALF member at all must not stall the
        // whole pipeline.
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("20260903T090000Z")).unwrap();
        touch(dir.path(), "20260903T090000Z", "RJTTF942MCA.txt");
        touch(dir.path(), "20260903T090000Z", "RJTTF942MSN.txt");
        // No ALF file at all.

        let delivery = latest_complete_delivery(dir.path()).unwrap().unwrap();
        assert_eq!(delivery.dir_name, "20260903T090000Z");
        assert_eq!(delivery.alf_path, None);
    }
```

- [ ] **Step 4: Run the tests**

```bash
cargo test -p schedule-reference discovery::
```

  Expected: all pass, including the two new ones.

- [ ] **Step 5: Commit**

```bash
git add crates/schedule-reference/src/discovery.rs
git commit -m "schedule-reference: locate an optional ALF file alongside each complete MCA+MSN delivery"
```

---

## Task 2: MSN minimum-change-time parsing, byte offset verified against real bytes first

**Files:**
- Create: `crates/schedule-reference/examples/msn_change_time_probe.rs`
  (dev-only human tool, not part of `cargo test` — mirrors
  `crates/schedule-query`'s own `examples/inspect.rs` precedent for
  "a human re-checks a byte offset against a real, full,
  untracked `timetable_full.zip` extract by hand", per that crate's own
  module doc, `schedule-query/src/lib.rs:70-73`)
- Modify: `crates/schedule-reference/src/parser.rs`

**Interfaces:**
- Produces: `parser::parse_msn_change_time_by_tiploc(text: &str) ->
  HashMap<String, i32>` (TIPLOC → raw parsed change-time integer, no
  default/sentinel interpretation — Judgment Call 3), and an extended
  `ParsedRow`/`resolve()` that attaches `change_time_minutes: Option<i32>`
  to each resolved STANOX row.
- Consumes: nothing new — same already-in-memory `a_text` `read_prefixed_lines`
  already produces (`main.rs:120`).

- [ ] **Step 1: Write the verification tool.** This does NOT hardcode a
  byte range up front — it takes a byte range as a CLI argument and reports
  the resulting value distribution, so the implementer can try the
  sibling's hypothesized `64..65` (0-indexed `[63,65)`) plus a few
  neighboring ranges against a REAL delivery's MSN file and see which one
  actually looks right, before any range is hardcoded into shipped code:

```rust
//! Dev-only tool: reports the value distribution a candidate byte range
//! produces when applied to every real `A` line of a live delivery's MSN
//! file -- NOT part of `cargo test` (see crate module doc / this plan's
//! Task 2). Run against a real `timetable_full.zip` extract's
//! `RJTTFnnnMSN.txt` before trusting ANY specific byte range in shipped
//! parser code -- this app's own `49..52` CRS field and the sibling
//! project's documented `44..46` CRS field disagree by 6 bytes on the
//! exact same real fixture (this plan's Judgment Call 2), so a byte range
//! copied from another codebase's documentation is not sufficient
//! evidence on its own.
//!
//! Usage: `cargo run -p schedule-reference --example msn_change_time_probe -- \
//!     /path/to/RJTTFnnnMSN.txt <start> <end>`
//! (0-indexed, half-open, e.g. `63 65` to try the sibling's own hypothesis).
//!
//! A byte range is a good candidate when the printed distribution looks
//! like the sibling project's own independently-measured real shape
//! (`docs/superpowers/specs/2026-07-22-train-mcp-phase2b-journey-planner-design.md:74`):
//! mostly single-digit values, a clear mode around 5, and a small number
//! (order of ten, not hundreds) of 98/99 outliers. A range producing
//! double-digit values across most stations, or a huge spread, is wrong.
use std::collections::BTreeMap;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let [_, path, start, end] = args.as_slice() else {
        anyhow::bail!("usage: msn_change_time_probe <msn-file-path> <start> <end>");
    };
    let start: usize = start.parse()?;
    let end: usize = end.parse()?;
    let text = std::fs::read_to_string(path)?;

    let mut histogram: BTreeMap<String, u32> = BTreeMap::new();
    let mut lines_checked = 0u32;
    for line in text.lines() {
        if !line.starts_with('A') || line.len() < end {
            continue;
        }
        // Same "reject the FILE-SPEC pseudo-record" guard as
        // parse_msn_a_lines uses on the TIPLOC field, applied here on the
        // CRS field instead, since this probe doesn't parse TIPLOC.
        let crs = line.get(49..52).unwrap_or("").trim();
        if crs.is_empty() {
            continue;
        }
        lines_checked += 1;
        let raw = line[start..end].trim().to_string();
        *histogram.entry(raw).or_insert(0) += 1;
    }

    println!("checked {lines_checked} real 'A' station lines from {path}");
    println!("value distribution for byte range [{start}, {end}):");
    for (value, count) in &histogram {
        println!("  {value:>6} : {count}");
    }
    Ok(())
}
```

- [ ] **Step 2: Run the probe against a real delivery** (the implementer
  must have access to a real `timetable_full.zip` extract or a live
  delivery directory to do this — this plan cannot complete this step
  itself, since no live delivery was available to this planning pass):

```bash
cargo run -p schedule-reference --example msn_change_time_probe -- \
    /path/to/real/RJTTFnnnMSN.txt 63 65
```

  Compare the printed distribution against the sibling project's own
  reported real shape (3,295 stations, 2,512 at `5`, range `0`-`9`, plus
  nine values at `98`/`99`). If `[63,65)` (the sibling's own hypothesis)
  does not look like that shape, try adjacent ranges (`[62,64)`, `[64,66)`,
  etc.) until one does. **Record the confirmed range in this file's own
  doc comment before proceeding to Step 3** — do not guess.

- [ ] **Step 3: Add `parse_msn_change_time_by_tiploc`** to `parser.rs`,
  using the byte range Step 2 confirmed (written here as `63..65` — replace
  with whatever Step 2 actually confirms if different):

```rust
/// TIPLOC -> raw minimum-change-time minutes, from every real `A` record in
/// `text` (same already-filtered `A`-prefixed text `parse_msn_a_lines`
/// reads, `main.rs`'s `read_prefixed_lines(&delivery.msn_path, "A")`).
///
/// Byte layout: `63..65` (0-indexed, half-open) -- CONFIRMED against a real
/// delivery's MSN file per this plan's own Task 2 Step 2, not copied from
/// the sibling `Distant-Signal-MCP` project's own documented `64-65`
/// (1-indexed) without independent verification: applying that project's
/// own range to THIS app's own already-tested `A_WATRLMN` fixture
/// (`parser.rs`'s existing `msn_tests` module) produces a value outside
/// its own documented typical range, and the two codebases' CRS-field byte
/// math for the exact same real fixture already disagrees by 6 bytes (see
/// this plan's Judgment Call 2) -- the two extracts are not directly
/// byte-transferable.
///
/// Returns the RAW parsed integer, with no default/sentinel interpretation
/// applied (deliberately -- see this plan's Judgment Call 3): `NULL`
/// downstream (this function simply omits the entry) means "no MSN record
/// matched this TIPLOC at all," a genuinely different fact from "this
/// TIPLOC's own recorded value happens to be a 98/99 sentinel" or "happens
/// to be the modal 5" -- both of which DO appear as real, present map
/// entries. A line whose change-time field is present but not a valid
/// non-negative integer is skipped for that one TIPLOC (same "skip
/// malformed, never abort the whole extraction" posture as
/// `parse_msn_a_lines`, not the sibling's own throw -- Judgment Call 5),
/// not a hard error.
pub fn parse_msn_change_time_by_tiploc(text: &str) -> HashMap<String, i32> {
    let mut map = HashMap::new();
    for line in text.lines() {
        if line.len() < 65 {
            continue;
        }
        let tiploc = line[36..43].trim();
        if tiploc.is_empty() || !tiploc.chars().all(|c| c.is_ascii_alphanumeric()) {
            continue; // catches the FILE-SPEC=05 header pseudo-record, same as parse_msn_a_lines
        }
        let raw = line[63..65].trim();
        let Ok(minutes) = raw.parse::<i32>() else {
            continue;
        };
        if minutes < 0 {
            continue;
        }
        map.insert(tiploc.to_string(), minutes);
    }
    map
}
```

- [ ] **Step 4: Extend `ParsedRow` and `resolve()`** to carry the value
  through, keyed by the same `record.tiploc` `resolve()` already uses to
  backfill a blank CRS:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRow {
    pub stanox: String,
    pub crs: String,
    pub tiploc: String,
    pub station_name: String,
    /// Raw minimum-change-time minutes from the MSN `A` record matching
    /// this row's own TIPLOC, or `None` if no MSN record matched it at
    /// all (a real, honest gap -- e.g. a junction-only TIPLOC with no
    /// passenger station record). See [`parse_msn_change_time_by_tiploc`]'s
    /// own doc comment for why this is never defaulted or sentinel-resolved
    /// here.
    pub change_time_minutes: Option<i32>,
}

pub fn resolve(
    ti: &[TiRecord],
    msn_crs_by_tiploc: &HashMap<String, String>,
    msn_change_time_by_tiploc: &HashMap<String, i32>,
) -> Vec<ParsedRow> {
    // ... unchanged body up to the row-construction point ...
    if let Some(winner) = winner {
        let (record, crs) = candidates
            .iter()
            .find(|(_, crs)| crs == winner)
            .expect("winner came from distinct");
        rows.push(ParsedRow {
            stanox,
            crs: crs.clone(),
            tiploc: record.tiploc.clone(),
            station_name: record.station_name.clone(),
            change_time_minutes: msn_change_time_by_tiploc.get(&record.tiploc).copied(),
        });
    }
```

  Update the one call site in `main.rs` (Task 6 does the full wiring; this
  step only needs `resolve`'s new third parameter added at its existing
  call site so the crate keeps compiling):

```rust
    let msn_crs = parser::parse_msn_a_lines(&a_text);
    let msn_change_time = parser::parse_msn_change_time_by_tiploc(&a_text);
    let rows = parser::resolve(&ti_records, &msn_crs, &msn_change_time);
```

- [ ] **Step 5: Update every existing `resolve()` test call site** in
  `parser.rs`'s own `resolve_tests` module to pass `&HashMap::new()` as the
  new third argument (none of the existing tests assert on change time, so
  an empty map is the correct, minimal fixture) — five call sites
  (`an_unambiguous_stanox_resolves_directly`,
  `a_blank_ti_crs_is_completed_from_the_msn_a_record_before_grouping`,
  `ambiguous_stanox_with_one_non_x_candidate_resolves_to_it`,
  `ambiguous_stanox_with_two_non_x_candidates_is_excluded_entirely`,
  `all_14_real_ambiguous_stanox_values_resolve_exactly_as_the_checked_in_csv_does`),
  and update each test's own `assert_eq!` on the resulting `Vec<ParsedRow>`
  to include `change_time_minutes: None,` in every expected struct literal.

- [ ] **Step 6: Add new tests for `parse_msn_change_time_by_tiploc` and
  `resolve`'s new field**, using the byte-verified constants already in
  this file (`A_WATRLMN`, `A_HEADER`) plus a new, clearly-labeled synthetic
  fixture for the sentinel case (labeled synthetic per this crate's own
  "quote real bytes when available, clearly mark anything else synthetic"
  convention, `records.rs:130-136`'s sibling precedent — a real 98/99
  station's exact byte-for-byte line was not available to this planning
  pass):

```rust
#[cfg(test)]
mod change_time_tests {
    use super::*;

    #[test]
    fn extracts_the_change_time_for_a_real_a_record() {
        let map = parse_msn_change_time_by_tiploc(A_WATRLMN);
        // Confirmed against a real delivery per this plan's Task 2 Step 2;
        // update this assertion if that step confirmed a different value
        // for this exact fixture line than whatever is written here.
        assert!(map.contains_key("WATRLMN"));
    }

    #[test]
    fn the_file_spec_header_pseudo_record_contributes_no_change_time() {
        let map = parse_msn_change_time_by_tiploc(A_HEADER);
        assert!(map.is_empty());
    }

    #[test]
    fn a_tiploc_with_no_msn_record_at_all_is_absent_not_zero() {
        let map = parse_msn_change_time_by_tiploc("");
        assert_eq!(map.get("ANYTPL"), None);
    }
}
```

- [ ] **Step 7: Run the tests**

```bash
cargo test -p schedule-reference parser::
```

  Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/schedule-reference/src/parser.rs crates/schedule-reference/examples/msn_change_time_probe.rs
git commit -m "schedule-reference: parse MSN minimum-change-time minutes, byte offset verified against real data"
```

---

## Task 3: `crates/schedule-reference/src/alf.rs` — ALF fixed-link parsing

**Files:**
- Create: `crates/schedule-reference/src/alf.rs`
- Modify: `crates/schedule-reference/src/main.rs` (add `mod alf;`)

**Interfaces:**
- Produces: `alf::parse_alf_lines(text: &str) -> Vec<alf::ParsedFixedLink>`,
  consumed by Task 6.

- [ ] **Step 1: Write the module.** The ALF member is a genuinely different
  format from MCA/MSN (comma-separated `key=value`, not fixed-width) — this
  module deliberately does not reuse `parser.rs`'s byte-slice helpers:

```rust
//! Pure CIF `ALF` (Additional Fixed Links) record parsing -- one
//! comma-separated `key=value` line per fixed link (a walk, tube, bus, tram
//! or ferry connection between two CRS codes), e.g.:
//!
//!     M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2359,P=4,R=0000001
//!
//! This is a genuinely different record shape from every other CIF member
//! this crate reads: `MCA`/`MSN` are fixed-width, byte-offset records
//! (`parser.rs`); `ALF` is comma-delimited key=value pairs, so this module
//! does not import or reuse `parser.rs`'s byte-slice helpers. Real line
//! shape and field meanings confirmed against the sibling `Distant-Signal-MCP`
//! project's own `src/timetable/cif/alf.ts` (re-cloned and independently
//! re-read for this plan's own research pass, not carried forward
//! unverified) -- see this plan's own header for the exact commit
//! provenance. `P` (a real field on every quoted row) is deliberately
//! never parsed -- see this plan's Judgment Call 6.
//!
//! No I/O here -- same "parsing logic pure and testable separately from
//! I/O" convention `parser.rs`'s own module doc establishes.

/// One parsed `ALF` fixed-link record. `from_crs`/`to_crs` are CRS codes
/// (NOT TIPLOCs -- a different identifier space from every other record
/// this crate parses, and a real source of join bugs if the two are
/// confused when Phase 2 consumes this data against a TIPLOC-keyed
/// connections array).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFixedLink {
    pub mode: String,
    pub from_crs: String,
    pub to_crs: String,
    pub minutes: i32,
    /// Raw `HHMM`, 4 ASCII digits, e.g. `"0001"`. Not parsed into a
    /// `NaiveTime` here -- Phase 2's read-side interchange logic is where
    /// this gets compared against a query's own clock time, matching this
    /// codebase's "store the raw CIF value, interpret at read time"
    /// convention (this plan's Judgment Call 3).
    pub valid_from: String,
    pub valid_to: String,
    /// Raw 7-character `'0'`/`'1'` bitmask, Monday-first -- the same
    /// day-of-week convention `schedule_query::records::BasicSchedule::days_of_week`
    /// already uses for the CIF `SCHEDULE` member's own days-run bitmask,
    /// stored here as text rather than `[bool; 7]` since nothing in this
    /// crate needs to inspect individual days at ingest time.
    pub days_mask: String,
}

/// Parses one `ALF` line. Returns `None` for a blank line or a `/!!`-prefixed
/// comment line (both real, confirmed present in a real extract) -- neither
/// is a parse failure worth logging on every cycle for a normal,
/// well-formed file. Returns `None` (logged by the caller, not this pure
/// function -- see `main.rs`'s wiring) for a line that has link-like shape
/// but is missing a required key, rather than panicking the whole
/// extraction -- this crate's own established "skip malformed, never abort"
/// convention (this plan's Judgment Call 5), diverging deliberately from
/// the sibling project's own `throw`-on-missing-field posture.
pub fn parse_alf_line(line: &str) -> Option<ParsedFixedLink> {
    let text = line.trim();
    if text.is_empty() || text.starts_with("/!!") {
        return None;
    }

    let mut values: std::collections::HashMap<&str, &str> = std::collections::HashMap::new();
    for pair in text.split(',') {
        if let Some((key, value)) = pair.split_once('=') {
            values.insert(key.trim(), value.trim());
        }
    }

    let mode = values.get("M")?.to_string();
    let from_crs = values.get("O")?.to_string();
    let to_crs = values.get("D")?.to_string();
    let minutes: i32 = values.get("T")?.parse().ok()?;
    if minutes < 0 {
        return None;
    }
    let valid_from = values.get("S")?.to_string();
    let valid_to = values.get("E")?.to_string();
    let days_mask = values.get("R")?.to_string();

    Some(ParsedFixedLink {
        mode,
        from_crs,
        to_crs,
        minutes,
        valid_from,
        valid_to,
        days_mask,
    })
}

/// Every successfully-parsed link in `text`, one call per already-read-into-memory
/// ALF file (mirrors `parser::parse_ti_lines`'s own "whole file as one
/// `&str` in, `Vec` out" shape). A malformed line simply contributes
/// nothing to the result -- see [`parse_alf_line`]'s own doc comment.
pub fn parse_alf_lines(text: &str) -> Vec<ParsedFixedLink> {
    text.lines().filter_map(parse_alf_line).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real quoted line, re-verified against the sibling project's own
    // `src/timetable/cif/alf.ts` module doc during this plan's own
    // research pass (that project's own doc states this line's provenance
    // as a real extract; not independently re-extracted from a live
    // delivery by this pass, since none was available -- see this plan's
    // own header note).
    const REAL_LINE: &str = "M=WALK,O=AFK,D=ASI,T=5,S=0001,E=2359,P=4,R=0000001";

    #[test]
    fn parses_a_real_fixed_link_line() {
        let link = parse_alf_line(REAL_LINE).expect("real line parses");
        assert_eq!(link.mode, "WALK");
        assert_eq!(link.from_crs, "AFK");
        assert_eq!(link.to_crs, "ASI");
        assert_eq!(link.minutes, 5);
        assert_eq!(link.valid_from, "0001");
        assert_eq!(link.valid_to, "2359");
        assert_eq!(link.days_mask, "0000001");
    }

    #[test]
    fn a_blank_line_is_skipped() {
        assert_eq!(parse_alf_line(""), None);
        assert_eq!(parse_alf_line("   "), None);
    }

    #[test]
    fn a_comment_line_is_skipped() {
        assert_eq!(parse_alf_line("/!! Sequence: 904"), None);
    }

    #[test]
    fn a_line_missing_a_required_field_is_skipped_not_a_panic() {
        assert_eq!(parse_alf_line("M=WALK,O=AFK,D=ASI,S=0001,E=2359,R=0000001"), None);
    }

    #[test]
    fn a_negative_transfer_time_is_rejected() {
        assert_eq!(
            parse_alf_line("M=WALK,O=AFK,D=ASI,T=-5,S=0001,E=2359,R=0000001"),
            None
        );
    }

    #[test]
    fn parse_alf_lines_skips_blanks_and_comments_and_keeps_real_links() {
        let text = format!("/!! Sequence: 904\n\n{REAL_LINE}\n");
        let links = parse_alf_lines(&text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].mode, "WALK");
    }
}
```

- [ ] **Step 2: Register the module** in `main.rs`:

```rust
mod alf;
mod config;
mod discovery;
mod parser;
```

- [ ] **Step 3: Run the tests**

```bash
cargo test -p schedule-reference alf::
```

  Expected: all 6 pass.

- [ ] **Step 4: Commit**

```bash
git add crates/schedule-reference/src/alf.rs crates/schedule-reference/src/main.rs
git commit -m "schedule-reference: add ALF fixed-link parsing (comma key=value format)"
```

---

## Task 4: Migration — `fixed_links` table + `stanox_crs.change_time_minutes`

**Files:**
- Create: `crates/api/migrations/20260923090000_fixed_links_and_change_time.sql`

- [ ] **Step 1: Write the migration**

```sql
-- -------------------------------------------------------------------------
-- Dynamic Trip Planning Phase 1: real interchange data.
-- docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §0.2,
-- §0.4, §4, §8 Phase 1; this plan's own Task 4.
--
-- Same-station changes: a nullable `change_time_minutes` on the existing
-- `stanox_crs` table (a per-TIPLOC/STANOX attribute, so it belongs on the
-- table already keyed by that identity, not a new table). NULL means "no
-- MSN record matched this TIPLOC at all" -- a real, different fact from a
-- present-but-sentinel (98/99) value or the modal 5 -- see this plan's
-- Judgment Call 3. No default/sentinel interpretation happens here or in
-- `schedule-reference`; Phase 2's connections-array builder is where that
-- policy lives.
--
-- Cross-station walking/tube/bus/tram/ferry transfers: a new `fixed_links`
-- table, CRS-keyed (ALF's own identifier space -- NOT TIPLOCs, see
-- `alf.rs`'s own module doc). Wholesale-replaced every publish cycle
-- (DELETE + INSERT in one transaction, see `queries::upsert_fixed_links`)
-- rather than per-row upserted, because a real ALF row has no natural
-- stable per-row key -- the same physical link legitimately appears as
-- several rows differing only in mode/validity window (this plan's
-- Judgment Call 4).
-- -------------------------------------------------------------------------

ALTER TABLE stanox_crs ADD COLUMN change_time_minutes INTEGER;

CREATE TABLE fixed_links (
    id                BIGSERIAL PRIMARY KEY,
    mode              TEXT NOT NULL,
    from_crs          TEXT NOT NULL,
    to_crs            TEXT NOT NULL,
    minutes           INTEGER NOT NULL,
    -- Raw "HHMM" (4 ASCII digits), not a TIME column -- see alf.rs's own
    -- doc comment on ParsedFixedLink::valid_from/valid_to for why this
    -- stays a raw string, matching this app's "store raw CIF value,
    -- interpret at read time" convention.
    valid_from        TEXT NOT NULL,
    valid_to          TEXT NOT NULL,
    -- Raw 7-char '0'/'1' bitmask, Monday-first, matching
    -- schedule_query::records::BasicSchedule::days_of_week's convention.
    days_mask         TEXT NOT NULL,
    source_sequence   INTEGER NOT NULL,
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX fixed_links_from_crs ON fixed_links (from_crs);
```

- [ ] **Step 2: Verify.** `sqlx` migrations in `crates/api` run
  automatically on `cargo test`/`cargo run` startup:

```bash
DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api --lib -- --list 2>&1 | tail -5
psql "$DATABASE_URL" -c "\d stanox_crs" | grep change_time_minutes
psql "$DATABASE_URL" -c "\d fixed_links"
```

  Expected: no migration error; `stanox_crs` shows the new nullable
  `integer` column; `fixed_links` exists with the shape above.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260923090000_fixed_links_and_change_time.sql
git commit -m "api: add fixed_links table and stanox_crs.change_time_minutes column"
```

---

## Task 5: `api` data layer + ingest routes

**Files:**
- Modify: `crates/common/src/lib.rs`
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/ingest.rs`

**Interfaces:**
- Produces: `common::FixedLinkRecord`, `queries::upsert_fixed_links`,
  `POST /private/fixed-links`; extends `common::StanoxCrsRecord` and
  `queries::upsert_stanox_crs`/`list_stanox_crs`/`list_stanox_crs_for_crs`
  with `change_time_minutes`.
- Consumes: Task 4's migration must already be applied.

- [ ] **Step 1: Add `FixedLinkRecord` to `crates/common/src/lib.rs`**, near
  `StanoxCrsRecord`:

```rust
/// One resolved CIF `ALF` fixed-link row, as published between
/// `crates/schedule-reference` (writer, `POST /private/fixed-links`) and
/// `crates/api` (reader/storage). `from_crs`/`to_crs` are CRS codes, not
/// TIPLOCs -- ALF's own identifier space (see
/// `crates/schedule-reference/src/alf.rs`'s module doc). See
/// docs/superpowers/specs/2026-09-22-dynamic-trip-planning-design.md §0.4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixedLinkRecord {
    pub mode: String,
    pub from_crs: String,
    pub to_crs: String,
    pub minutes: i32,
    /// Raw "HHMM", 4 ASCII digits -- see `alf::ParsedFixedLink`'s own doc
    /// comment for why this is not a parsed time type.
    pub valid_from: String,
    pub valid_to: String,
    /// Raw 7-char '0'/'1' bitmask, Monday-first.
    pub days_mask: String,
    pub source_sequence: i32,
}
```

  Extend `StanoxCrsRecord` with the new nullable field:

```rust
pub struct StanoxCrsRecord {
    pub stanox: String,
    pub crs: String,
    pub tiploc: String,
    pub station_name: String,
    pub source_sequence: i32,
    /// Raw minimum-change-time minutes from the matching MSN `A` record,
    /// or `None` if no MSN record matched this TIPLOC at all -- see
    /// docs/superpowers/plans/2026-09-22-dynamic-trip-planning-phase1-cif-interchange-ingestion-plan.md's
    /// Judgment Call 3 for why this is never defaulted/sentinel-resolved
    /// here. `#[serde(default)]` so a `stanox_crs` publish from a
    /// not-yet-upgraded `schedule-reference` build still deserializes (as
    /// `None`, the same "assume the older, narrower shape" posture
    /// `CallingPoint::day_offset`'s own `#[serde(default)]` establishes for
    /// an analogous additive field).
    #[serde(default)]
    pub change_time_minutes: Option<i32>,
}
```

- [ ] **Step 2: Extend `upsert_stanox_crs`/`list_stanox_crs`/
  `list_stanox_crs_for_crs`** in `crates/api/src/data/queries.rs` to carry
  the new column:

```rust
pub async fn upsert_stanox_crs(pool: &PgPool, records: &[common::StanoxCrsRecord]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for record in records {
        sqlx::query(
            r#"
            INSERT INTO stanox_crs (stanox, crs, tiploc, station_name, source_sequence, change_time_minutes, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (stanox) DO UPDATE SET
                crs                 = EXCLUDED.crs,
                tiploc              = EXCLUDED.tiploc,
                station_name        = EXCLUDED.station_name,
                source_sequence     = EXCLUDED.source_sequence,
                change_time_minutes = EXCLUDED.change_time_minutes,
                updated_at          = NOW()
            "#,
        )
        .bind(&record.stanox)
        .bind(&record.crs)
        .bind(&record.tiploc)
        .bind(&record.station_name)
        .bind(record.source_sequence)
        .bind(record.change_time_minutes)
        .execute(&mut *tx)
        .await?;

        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}
```

  `StanoxCrsRow` (the private `sqlx::FromRow` type `list_stanox_crs`/
  `list_stanox_crs_for_crs` select into before `.into()`-converting to
  `common::StanoxCrsRecord`) gains the same field, and both `SELECT`s add
  `change_time_minutes` to their column list — locate `StanoxCrsRow`'s
  definition (`grep -n "struct StanoxCrsRow" crates/api/src/data/queries.rs`)
  and its `From<StanoxCrsRow> for common::StanoxCrsRecord` impl, adding the
  field to both in the same mechanical way `custom_name` was threaded
  through `TrackedTrainState` in the custom-tracking-names plan's own
  Task 5 (same pattern: add to struct, add to `SELECT`, add to the mapping
  impl).

- [ ] **Step 3: Add `upsert_fixed_links`**, directly below
  `list_stanox_crs_for_crs`:

```rust
/// Wholesale-replaces `fixed_links` with `records` in one transaction --
/// see this plan's Judgment Call 4 for why this is a full replace, not a
/// per-row `ON CONFLICT` upsert like `upsert_stanox_crs`: a real ALF row
/// has no natural stable per-row key. At ~4,222 real rows (confirmed
/// against the sibling `Distant-Signal-MCP` project's own measurement,
/// see this plan's header), this is cheap on every ~30-minute publish
/// cycle. Called only when `schedule-reference` actually found an ALF
/// file this cycle (`routes::ingest::post_fixed_links`'s own caller,
/// `main.rs`'s `publish_fixed_links`) -- an absent ALF file means this
/// function is simply never called that cycle, leaving the previous
/// cycle's rows in place rather than deleting them with nothing to
/// replace them (this plan's Judgment Call 1's "degrade this one product,
/// never wipe it for no reason" posture).
pub async fn upsert_fixed_links(pool: &PgPool, records: &[common::FixedLinkRecord]) -> Result<u64> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM fixed_links").execute(&mut *tx).await?;

    let mut count = 0u64;
    for record in records {
        sqlx::query(
            r#"
            INSERT INTO fixed_links (mode, from_crs, to_crs, minutes, valid_from, valid_to, days_mask, source_sequence, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
            "#,
        )
        .bind(&record.mode)
        .bind(&record.from_crs)
        .bind(&record.to_crs)
        .bind(record.minutes)
        .bind(&record.valid_from)
        .bind(&record.valid_to)
        .bind(&record.days_mask)
        .bind(record.source_sequence)
        .execute(&mut *tx)
        .await?;
        count += 1;
    }

    tx.commit().await?;
    Ok(count)
}

/// Every `fixed_links` row whose `from_crs` matches `crs` -- Phase 2's own
/// read-side lookup shape (mirrors `list_stanox_crs_for_crs`'s own
/// `WHERE crs = $1` pattern). Case-insensitive, matching that function's
/// own `UPPER(...)` convention.
pub async fn list_fixed_links_from_crs(pool: &PgPool, crs: &str) -> Result<Vec<common::FixedLinkRecord>> {
    let rows = sqlx::query_as::<_, FixedLinkRow>(
        "SELECT mode, from_crs, to_crs, minutes, valid_from, valid_to, days_mask, source_sequence \
         FROM fixed_links WHERE UPPER(from_crs) = UPPER($1)",
    )
    .bind(crs)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(common::FixedLinkRecord::from).collect())
}

#[derive(Debug, sqlx::FromRow)]
struct FixedLinkRow {
    mode: String,
    from_crs: String,
    to_crs: String,
    minutes: i32,
    valid_from: String,
    valid_to: String,
    days_mask: String,
    source_sequence: i32,
}

impl From<FixedLinkRow> for common::FixedLinkRecord {
    fn from(row: FixedLinkRow) -> Self {
        Self {
            mode: row.mode,
            from_crs: row.from_crs,
            to_crs: row.to_crs,
            minutes: row.minutes,
            valid_from: row.valid_from,
            valid_to: row.valid_to,
            days_mask: row.days_mask,
            source_sequence: row.source_sequence,
        }
    }
}
```

- [ ] **Step 4: Add `POST /private/fixed-links`** in
  `crates/api/src/routes/ingest.rs`, directly below `post_stanox_crs`, and
  mount it in that file's `router()` alongside the existing
  `/private/stanox-crs` route (same internal-OAuth-gated pattern — check
  `AppState::internal_oauth_routes`/`build_internal_oauth_routes` for where
  `/private/stanox-crs`'s group entry is registered and add a matching
  entry for `/private/fixed-links` with the same group, since both are
  written by the same `schedule-reference` service identity):

```rust
/// `crates/schedule-reference`'s per-cycle fixed-links batch -- see
/// `queries::upsert_fixed_links`.
async fn post_fixed_links(
    State(app): State<App>,
    Json(records): Json<Vec<common::FixedLinkRecord>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_fixed_links(&app.database, &records)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

```rust
        .route("/private/fixed-links", axum::routing::post(post_fixed_links))
```

- [ ] **Step 5: Add DB-gated `#[ignore]`d tests** for `upsert_fixed_links`
  and `upsert_stanox_crs`'s new field, in `queries.rs`'s existing DB-test
  module:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_fixed_links -- --ignored --test-threads=1`"]
    async fn upsert_fixed_links_replaces_the_whole_table_each_call() {
        let pool = connect().await;
        let first = vec![common::FixedLinkRecord {
            mode: "TUBE".to_string(),
            from_crs: "EUS".to_string(),
            to_crs: "KGX".to_string(),
            minutes: 5,
            valid_from: "0500".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111100".to_string(),
            source_sequence: 1,
        }];
        upsert_fixed_links(&pool, &first).await.expect("first publish");
        let after_first = list_fixed_links_from_crs(&pool, "EUS").await.expect("read back");
        assert_eq!(after_first.len(), 1);

        let second = vec![common::FixedLinkRecord {
            mode: "TRANSFER".to_string(),
            from_crs: "EUS".to_string(),
            to_crs: "STP".to_string(),
            minutes: 15,
            valid_from: "0000".to_string(),
            valid_to: "2359".to_string(),
            days_mask: "1111111".to_string(),
            source_sequence: 2,
        }];
        upsert_fixed_links(&pool, &second).await.expect("second publish replaces");
        let after_second = list_fixed_links_from_crs(&pool, "EUS").await.expect("read back");
        assert_eq!(
            after_second.len(),
            1,
            "the first cycle's EUS->KGX row must be gone -- this is a full replace, not an upsert"
        );
        assert_eq!(after_second[0].to_crs, "STP");

        sqlx::query("DELETE FROM fixed_links").execute(&pool).await.ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api \
                upsert_stanox_crs_change_time -- --ignored --test-threads=1`"]
    async fn upsert_stanox_crs_change_time_minutes_round_trips_including_none() {
        let pool = connect().await;
        let records = vec![
            common::StanoxCrsRecord {
                stanox: "TEST-STANOX-WITH-CHANGE-TIME".to_string(),
                crs: "ZZZ".to_string(),
                tiploc: "ZZZTPL".to_string(),
                station_name: "TEST STATION".to_string(),
                source_sequence: 1,
                change_time_minutes: Some(5),
            },
            common::StanoxCrsRecord {
                stanox: "TEST-STANOX-NO-CHANGE-TIME".to_string(),
                crs: "YYY".to_string(),
                tiploc: "YYYTPL".to_string(),
                station_name: "TEST STATION 2".to_string(),
                source_sequence: 1,
                change_time_minutes: None,
            },
        ];
        upsert_stanox_crs(&pool, &records).await.expect("upsert");

        let all = list_stanox_crs(&pool).await.expect("read back");
        let with_time = all
            .iter()
            .find(|r| r.stanox == "TEST-STANOX-WITH-CHANGE-TIME")
            .expect("row present");
        assert_eq!(with_time.change_time_minutes, Some(5));
        let without_time = all
            .iter()
            .find(|r| r.stanox == "TEST-STANOX-NO-CHANGE-TIME")
            .expect("row present");
        assert_eq!(without_time.change_time_minutes, None);

        sqlx::query("DELETE FROM stanox_crs WHERE stanox LIKE 'TEST-STANOX-%'")
            .execute(&pool)
            .await
            .ok();
    }
```

- [ ] **Step 6: Verify**

```bash
cargo build -p api
cargo test -p api -- --ignored --test-threads=1
```

  Expected: builds clean, both new DB-gated tests pass.

- [ ] **Step 7: Commit**

```bash
git add crates/common/src/lib.rs crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs
git commit -m "api: add fixed_links storage and stanox_crs.change_time_minutes plumbing"
```

---

## Task 6: `schedule-reference` wiring — publish both new products

**Files:**
- Modify: `crates/schedule-reference/src/main.rs`
- Modify: `crates/schedule-reference/src/config.rs`

**Interfaces:**
- Consumes: Task 1's `alf_path`, Task 2's `parse_msn_change_time_by_tiploc`,
  Task 3's `alf::parse_alf_lines`, Task 5's `POST /private/fixed-links`.

- [ ] **Step 1: Add the new outbound URL to `config.rs`**, next to
  `schedule_line_population_url` (same `#[arg(env = "...", ...)]` shape
  every other outbound URL in this struct already uses):

```rust
    #[arg(env = "FIXED_LINKS_URL", long)]
    pub fixed_links_url: String,
```

- [ ] **Step 2: Wire `parse_msn_change_time_by_tiploc` into `poll_once`**,
  alongside the existing `msn_crs`/`resolve` call:

```rust
    let ti_records = parser::parse_ti_lines(&ti_text);
    let msn_crs = parser::parse_msn_a_lines(&a_text);
    let msn_change_time = parser::parse_msn_change_time_by_tiploc(&a_text);
    let rows = parser::resolve(&ti_records, &msn_crs, &msn_change_time);
```

- [ ] **Step 3: Add `publish_fixed_links`**, called from `poll_once`
  alongside the existing `common::ingest::post_batch(...)` for STANOX/CRS,
  gated on `delivery.alf_path` being `Some` (Judgment Call 1's degrade
  path):

```rust
    common::ingest::post_batch(
        client,
        &config.api_ingest_url,
        internal_oauth,
        &records,
        "stanox/crs rows",
    )
    .await?;

    *last_processed_delivery = Some(delivery.dir_name.clone());

    if let Some(alf_path) = &delivery.alf_path {
        publish_fixed_links(client, config, alf_path, internal_oauth, source_sequence).await;
    } else {
        tracing::warn!(
            delivery = %delivery.dir_name,
            "this delivery has no ALF file; fixed-links data was not refreshed this cycle \
             (previous cycle's rows, if any, remain in place)"
        );
    }

    publish_cif_derived_products(client, config, &delivery.mca_path, internal_oauth, &records)
        .await;
```

  and the function itself, near `publish_schedule_line_population`:

```rust
/// Reads and parses `alf_path`'s already-local, read-only-mounted ALF
/// member, and POSTs the result as one full-replace batch (see
/// `queries::upsert_fixed_links`'s own doc comment). Best-effort: a read or
/// parse failure here logs and returns, exactly like every other
/// `publish_*` function's own log-and-continue posture (`main.rs`'s
/// existing convention throughout) -- it never propagates as a hard
/// `poll_once` failure, since fixed-links data degrading for one cycle
/// must never take down the STANOX/CRS or schedule-population publishes
/// that already succeeded this same cycle.
async fn publish_fixed_links(
    client: &Client,
    config: &Config,
    alf_path: &std::path::Path,
    internal_oauth: &common::oauth_client::OAuthTokenCache,
    source_sequence: i32,
) {
    let text = match std::fs::read_to_string(alf_path) {
        Ok(text) => text,
        Err(err) => {
            tracing::error!(error = ?err, path = ?alf_path, "failed to read ALF file; skipping fixed-links publish this cycle");
            return;
        }
    };

    let records: Vec<common::FixedLinkRecord> = alf::parse_alf_lines(&text)
        .into_iter()
        .map(|link| common::FixedLinkRecord {
            mode: link.mode,
            from_crs: link.from_crs,
            to_crs: link.to_crs,
            minutes: link.minutes,
            valid_from: link.valid_from,
            valid_to: link.valid_to,
            days_mask: link.days_mask,
            source_sequence,
        })
        .collect();

    tracing::info!(count = records.len(), "parsed ALF fixed links");

    if let Err(err) =
        common::ingest::post_batch(client, &config.fixed_links_url, internal_oauth, &records, "fixed-link rows")
            .await
    {
        tracing::error!(error = ?err, "failed to publish fixed links; will retry next cycle");
    }
}
```

  `source_sequence` is already computed earlier in `poll_once`
  (`embedded_sequence_number(&delivery.mca_path).unwrap_or(0)`, `main.rs:141`)
  — pass that same value through rather than recomputing it.

- [ ] **Step 4: Add a chart/env-var entry** for `FIXED_LINKS_URL` in
  whatever `charts/distant-signal` values file already sets
  `SCHEDULE_LINE_POPULATION_URL`/`SCHEDULE_DESTINATION_DEPARTURES_URL` for
  the `schedule-reference` container (`grep -rn
  "SCHEDULE_LINE_POPULATION_URL" charts/distant-signal/` to find the
  template), pointing at `/private/fixed-links` on the same `api` base URL
  those already use.

- [ ] **Step 5: Verify**

```bash
cargo build -p schedule-reference
cargo test -p schedule-reference
cargo clippy -p schedule-reference --all-features --all-targets -- -D warnings
```

  Expected: builds and tests pass. There is no live-delivery integration
  test for `publish_fixed_links` itself in this crate (it makes a real
  HTTP POST) — this mirrors every other `publish_*` function in this file,
  none of which have one either; correctness here is proven by Task 3's
  parser tests plus Task 5's `api`-side DB-gated round-trip test.

- [ ] **Step 6: Commit**

```bash
git add crates/schedule-reference/src/main.rs crates/schedule-reference/src/config.rs charts/distant-signal
git commit -m "schedule-reference: publish ALF fixed links alongside the existing STANOX/CRS cycle"
```

---

## Self-review notes

- **Spec coverage**: §0.2's MSN column-65 field (Task 2), §0.2/§0.4's ALF
  ingestion gap (Tasks 1, 3, 6), §4's "same-CRS interchange... needs no new
  data source" and "cross-station... genuinely new CIF ingestion work" are
  both directly implemented. §8 Phase 1's own deliverable description
  ("real, queryable interchange data... before anything downstream depends
  on it") is met — Task 5's two new DB-gated tests are exactly that
  independent verification, runnable before Phase 2 exists.
- **Placeholder scan**: no TBD/TODO left in shipped code. Task 2's Step 2
  is the one place this plan cannot complete a value itself (no live CIF
  delivery available to this planning pass) — it is written as an explicit,
  concrete verification procedure with a defined pass/fail criterion (the
  sibling's own measured distribution shape), not a vague "figure this
  out later."
- **Type consistency**: `ParsedFixedLink`/`FixedLinkRecord`/`FixedLinkRow`
  all carry the same seven substantive fields end to end;
  `ParsedRow`/`StanoxCrsRecord`/`StanoxCrsRow` all gain `change_time_minutes`
  together.
