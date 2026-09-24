# TIPLOC-primary CRS crosswalk (closing `journey.rs::tiploc_key`'s gap 1)

## Context

`crates/api/src/data/journey.rs`'s `tiploc_key` doc comment (lines ~85-103)
documents a real, confirmed gap: `stanox_crs` (`crates/api/migrations/20260901150000_stanox_crs.sql`)
has `PRIMARY KEY (stanox)`, so `crates/schedule-reference`'s `parser::resolve`
can store at most ONE TIPLOC per STANOX. Two real, documented stations have
more than one real TIPLOC sharing one STANOX:

- Vauxhall: `VAUXHLM` ("Vauxhall Main Lines") and `VAUXHLW` ("Vauxhall
  Windsor lines"), both STANOX `87214`, both real CRS `VXH` (confirmed:
  `reference-data/stanox-crs.csv` line `87214,VXH`; `journey.rs`'s own
  existing characterization test at line ~1733 already asserts `VAUXHLW`
  resolves to `VXH`).
- Clapham Junction: `CLPHMJM` (main lines) and `CLPHMJW` (Windsor lines),
  both STANOX `87219`, both real CRS `CLJ` (confirmed:
  `reference-data/stanox-crs.csv` line `87219,CLJ`; the existing
  `all_14_real_ambiguous_stanox_values_resolve...` test in
  `crates/schedule-reference/src/parser.rs` already asserts `87219` resolves
  to `CLJ`).

Whichever TIPLOC `resolve`'s STANOX-grouping happens to keep as `stanox_crs`'s
one row for that STANOX resolves correctly everywhere in the app; the OTHER
TIPLOC has no CRS anywhere and silently fails to resolve, even on real,
confirmed production journeys (see `journey.rs`'s doc comment for train
`L82877`, SWR's Kingston loop, 2026-09-14).

### Investigation finding: no STANOX-inheritance policy is needed

`journey.rs`'s doc comment worries a naive "any TIPLOC sharing a STANOX
inherits that STANOX's one resolved CRS" policy would wrongly hand a
station's CRS to a same-STANOX junction TIPLOC (its own example: Waterloo's
CRS wrongly handed to a Waterloo-area junction TIPLOC). That worry is valid
against an inheritance-by-STANOX design -- but investigation shows Vauxhall
and Clapham Junction do not need inheritance at all:

- `crates/schedule-reference/src/parser.rs::resolve` already establishes
  that a TIPLOC's own `TI` record carries its own CRS directly, or (if
  blank) is completed from that SAME TIPLOC's own `MSN` `A` record
  (`parse_msn_a_lines`, whose doc comment notes CRS "is always populated in
  a real record"). This per-TIPLOC completion needs no STANOX at all --
  `resolve`'s STANOX-keyed grouping only exists to answer a DIFFERENT
  question ("what is the ONE CRS for this STANOX", needed by
  `trust-consumer` because TRUST movement messages carry STANOX, not
  TIPLOC) and, along the way, incidentally discards every TIPLOC at a
  STANOX except the one it picks as winner.
- Because Vauxhall's two TIPLOCs and Clapham Junction's two TIPLOCs each
  carry (directly or via their own MSN completion) the SAME real CRS as
  each other (`VXH`, `CLJ` respectively -- confirmed above), `resolve`'s
  STANOX-level ambiguity check never even flags STANOX `87214`/`87219` as
  ambiguous (`distinct.len() == 1` in both cases): it cleanly picks ONE of
  the two identically-correct candidates and discards the other. This is
  not a true CRS conflict needing a tiebreaker policy -- it is pure
  information loss caused by `stanox_crs`'s one-row-per-STANOX schema.
- The fix, therefore, is direct and needs no inheritance policy: build a
  TIPLOC-PRIMARY crosswalk straight from the same per-TIPLOC
  (`TI` record, completed from its OWN `MSN` record) data `resolve` already
  computes, but keep EVERY TIPLOC with a resolvable CRS as its own row,
  with NO STANOX-based grouping, tiebreaking or exclusion step at all. A
  junction TIPLOC that shares a STANOX with a real station but has neither
  its own `TI` CRS nor its own `MSN` record (`journey.rs`'s case 2:
  `SHCKLGJ`, `TWCKNMJ`, `NINELMJ`, `WLNDNJW`, `WATRLWC`, etc.) simply gets no
  row here either, exactly as today -- there is no STANOX-inheritance step
  to wrongly promote it.
- Real side benefit, not required but worth recording: several of the 14
  real ambiguous STANOX values `resolve` currently EXCLUDES ENTIRELY because
  two non-`X`-prefixed CRS candidates share one STANOX with no principled
  tiebreaker (e.g. `89428`: `ASHFKI`/`ASI` vs `ASHFKY`/`AFK`, both real,
  distinct, bookable stations) now resolve BOTH candidates correctly under
  the TIPLOC-primary crosswalk, since each TIPLOC's own CRS is directly
  known and neither needs to borrow the other's identity.
- Caveat, stated honestly: this sandbox has no live `timetable_full.zip`
  (kept out-of-repo, see `reference-data/stanox-crs.md`), so `VAUXHLW`'s and
  `CLPHMJW`'s exact real `TI`/`MSN` byte contents were not independently
  re-verified byte-for-byte in this pass -- the design instead relies on
  (a) their real CRS values being already independently confirmed in
  `reference-data/stanox-crs.csv` and in this codebase's own existing tests
  (cited above), and (b) `reference-data/line-catalogue-validation.md`'s own
  real, documented observation that some multi-TIPLOC stations'
  platform-specific TIPLOCs carry no CRS of their own on at least one other
  reference source (railwaycodes.org.uk) -- which is exactly the "blank
  `TI` CRS, completed from `MSN`" path `resolve_tiploc_crs` (this plan)
  already handles, same as `resolve` already does for `WATRLMN`. Task 1's
  tests exercise both the "own `TI` CRS populated" and "blank `TI` CRS,
  completed via own `MSN` record" shapes for both stations so the fix does
  not depend on which one turns out to be true in a live delivery.

### Design decision

Add a NEW table, `tiploc_crs` (`PRIMARY KEY (tiploc)`), populated by a NEW,
simpler parser function `resolve_tiploc_crs` that skips STANOX-grouping
entirely, alongside the EXISTING `stanox_crs` table/`resolve` function,
which stay completely unchanged (schema, upsert, and every existing test
fixture). `stanox_crs` keeps serving the callers that genuinely need
"exactly one CRS per STANOX" (`trust-consumer`'s STANOX-keyed reload
tied to TRUST movement messages). `tiploc_crs` becomes the canonical,
richer TIPLOC->CRS crosswalk for every "given a TIPLOC, what CRS" or "given
a CRS, which TIPLOC(s)" need inside `api` and `schedule-reference`.

Read-side integration is additive, not a replacement, to protect the large
number of existing tests across `crates/api` that seed ONLY `stanox_crs`
directly via raw SQL (`train.rs`, `trips.rs`, `trip_planning.rs`,
`reconciliation.rs`, `routes/stanox_crs.rs`): the three TIPLOC-crosswalk
read functions (`crs_for_tiploc`, `crs_for_tiplocs_batch`,
`list_stanox_crs_for_crs`) are changed to read the UNION of `tiploc_crs` and
`stanox_crs`, preferring a `tiploc_crs` row when a TIPLOC is present in
both. Every existing `stanox_crs`-only fixture keeps resolving exactly as
before (nothing lost); any TIPLOC that ONLY exists in the new, richer
`tiploc_crs` table (Vauxhall's/Clapham Junction's previously-dropped
sibling TIPLOC) now ALSO resolves (the actual fix). `trip_planning.rs`'s
`fetch_interchange_data` (which reads the whole `stanox_crs` table, not
these three query functions) is updated the same way: read the whole
`tiploc_crs` table too and let it override any `stanox_crs`-derived entry
for the same TIPLOC when building its in-memory maps.

`crates/schedule-reference` gets a second, parallel per-cycle publish:
alongside its existing `parser::resolve` -> `POST /private/stanox-crs`
publish (unchanged), it also runs the new `parser::resolve_tiploc_crs` on
the SAME already-parsed `ti_records`/`msn_crs`/`msn_change_time` and `POST`s
the result to a new `POST /private/tiploc-crs` endpoint (POST-only, reusing
the existing `internal_oauth_group_schedule_reference` writer credential,
same shape as `/schedule-network-departures`). `schedule-reference`'s own
in-process `tiploc_to_crs` maps (built 3 times in `main.rs`, currently all
from the STANOX-truncated `stanox_crs_records`) are switched to build from
this SAME new, richer `resolve_tiploc_crs` output computed locally in
`poll_once` -- no HTTP round-trip needed for that part, since
`schedule-reference` already holds the freshly-parsed CIF text in memory
each cycle.

## Global Constraints

- Do not change `stanox_crs`'s schema, `queries::upsert_stanox_crs`, or any
  existing test that seeds `stanox_crs` directly. Every one of those must
  keep passing unmodified.
- `common::StanoxCrsRecord`'s wire shape is unchanged (no new/removed
  fields, no renamed fields) -- it is a public, versioned wire contract with
  `#[serde(default)]` precedent for additive fields; do not touch it.
- New wire type: `common::TiplocCrsRecord` with fields in this exact order
  and these exact names: `tiploc: String`, `crs: String`,
  `station_name: String`, `stanox: String`, `source_sequence: i32`,
  `change_time_minutes: Option<i32>` (mirrors `StanoxCrsRecord`'s existing
  field set and the `#[serde(default)]` convention on
  `change_time_minutes`). Plain `#[derive(Debug, Clone, Serialize,
  Deserialize)]`, no `rename_all` (matches `StanoxCrsRecord`).
- New migration file:
  `crates/api/migrations/20260924130000_tiploc_crs.sql` (next available
  timestamp after `20260924120000_schedule_destination_departures_operator_atoc.sql`).
  Exact schema:
  ```sql
  CREATE TABLE tiploc_crs (
      tiploc TEXT PRIMARY KEY,
      crs TEXT NOT NULL,
      station_name TEXT NOT NULL,
      stanox TEXT NOT NULL,
      source_sequence INTEGER NOT NULL,
      change_time_minutes INTEGER,
      updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
  );

  CREATE INDEX tiploc_crs_crs_idx ON tiploc_crs (UPPER(crs));
  ```
  Give the migration a header comment in this file's own established style
  (see `20260901150000_stanox_crs.sql`) explaining it is the TIPLOC-primary
  sibling of `stanox_crs`, additive, and cross-referencing this plan file
  and `journey.rs::tiploc_key`'s doc comment.
- New route: `POST /private/tiploc-crs`, POST-only (no GET pair -- same
  shape as `/schedule-network-departures`), body `Vec<common::TiplocCrsRecord>`,
  handler upserts via a new `queries::upsert_tiploc_crs`. Auth: only
  `config.internal_oauth_group_schedule_reference` may call it (mirror the
  `/schedule-network-departures` entry in `crates/api/src/app.rs` exactly,
  including its comment style referencing which existing entry it reuses
  the credential from).
- Do not rename or change the signature of `queries::crs_for_tiploc`,
  `queries::crs_for_tiplocs_batch`, or `queries::list_stanox_crs_for_crs` --
  only their SQL body changes (to the union query below). Every existing
  caller (`journey.rs`, `schedule_matching.rs`) needs zero code changes.
- Run `cargo fmt --all` and `cargo clippy --workspace --all-features
  --all-targets -- -D warnings` before every commit; both must be clean.
- Every DB-gated test in this plan follows this codebase's existing
  convention exactly: `#[tokio::test]` + `#[ignore = "requires a live
  database; run with \`cargo test -p api <test_name> -- --ignored
  --test-threads=1\`"]`, a `test_pool`/`connect` helper reading
  `DATABASE_URL`, and explicit fixture cleanup (`DELETE FROM ... WHERE
  ...`) at the end of the test, matching the exact style already used in
  the file being edited (`queries.rs`'s `stanox_crs_lookup_query_tests`
  module, `routes/stanox_crs.rs`'s `db_tests` module).
- No task may modify `crates/api/src/routes/train.rs`,
  `crates/api/src/routes/trips.rs`, or
  `crates/api/src/data/reconciliation.rs` -- the whole point of the
  union-read design is that those files' existing `stanox_crs`-only
  fixtures need no changes. If a task's own work appears to require
  touching one of those files, stop and treat that as a plan defect to
  flag, not something to route around silently.

## Task 1: `common` wire type + pure parser resolution (no I/O, no DB)

Add to `crates/common/src/lib.rs`, directly below `StanoxCrsRecord`:

```rust
/// One directly-resolved TIPLOC->CRS row: the TIPLOC-primary sibling of
/// [`StanoxCrsRecord`], as `crates/schedule-reference` derives it via
/// `parser::resolve_tiploc_crs` and POSTs to `api`'s
/// `/private/tiploc-crs`. Unlike `StanoxCrsRecord`, this crosswalk keeps
/// EVERY TIPLOC with its own resolvable CRS as its own row -- no
/// STANOX-based grouping or "one row per STANOX" exclusion -- so a real
/// station whose STANOX is shared by more than one genuine calling-point
/// TIPLOC (e.g. Vauxhall's `VAUXHLM`/`VAUXHLW`, both CRS `VXH`; Clapham
/// Junction's `CLPHMJM`/`CLPHMJW`, both CRS `CLJ`) has a row for EACH of
/// them, not just whichever one `StanoxCrsRecord`'s STANOX-level
/// disambiguation happened to keep. See
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md and
/// `crates/api/src/data/journey.rs`'s `tiploc_key` doc comment ("What this
/// does NOT fix" item 1) for the gap this closes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TiplocCrsRecord {
    pub tiploc: String,
    pub crs: String,
    pub station_name: String,
    pub stanox: String,
    pub source_sequence: i32,
    #[serde(default)]
    pub change_time_minutes: Option<i32>,
}
```

In `crates/schedule-reference/src/parser.rs`, add a new public struct
`TiplocCrsRow` and function `resolve_tiploc_crs`, placed directly after
`resolve` (keep `resolve` completely unchanged):

```rust
/// One directly-resolved TIPLOC->CRS row -- the TIPLOC-primary output
/// `resolve_tiploc_crs` produces. Same fields `ParsedRow` carries, minus
/// `stanox`'s role as a grouping key (it is still carried through, just
/// never grouped/deduplicated on).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TiplocCrsRow {
    pub tiploc: String,
    pub crs: String,
    pub station_name: String,
    pub stanox: String,
    pub change_time_minutes: Option<i32>,
}

/// Resolves a TIPLOC-PRIMARY CRS crosswalk: every `TI` record whose own CRS
/// is populated, or whose blank CRS is completed from `msn_crs_by_tiploc`
/// for that SAME TIPLOC (identical per-TIPLOC completion `resolve` already
/// does), becomes its own row -- with NO STANOX-based grouping,
/// tiebreaking, or exclusion step of any kind. This is deliberate, not a
/// simplification that drops a needed safeguard: see
/// docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md's
/// "Investigation finding" section for why a STANOX-inheritance policy is
/// NOT needed here, and why this cannot wrongly promote a same-STANOX
/// junction TIPLOC (case 2 of `crates/api/src/data/journey.rs`'s
/// `tiploc_key` doc comment) to a real station's identity: a junction
/// TIPLOC with neither its own `TI` CRS nor its own `MSN` record simply
/// produces no row here, exactly as it produces none in `resolve`.
///
/// Real example this exists to fix: Vauxhall's `VAUXHLM`/`VAUXHLW` (STANOX
/// `87214`, both CRS `VXH`) and Clapham Junction's `CLPHMJM`/`CLPHMJW`
/// (STANOX `87219`, both CRS `CLJ`) each get their own row here, unlike
/// `resolve`, which keeps only one TIPLOC per STANOX.
pub fn resolve_tiploc_crs(
    ti: &[TiRecord],
    msn_crs_by_tiploc: &HashMap<String, String>,
    msn_change_time_by_tiploc: &HashMap<String, i32>,
) -> Vec<TiplocCrsRow> {
    let mut by_tiploc: HashMap<String, TiplocCrsRow> = HashMap::new();

    for record in ti {
        let Some(stanox) = &record.stanox else {
            continue;
        };
        let crs = record
            .crs
            .clone()
            .or_else(|| msn_crs_by_tiploc.get(&record.tiploc).cloned());
        let Some(crs) = crs else { continue };

        by_tiploc.insert(
            record.tiploc.clone(),
            TiplocCrsRow {
                tiploc: record.tiploc.clone(),
                crs,
                station_name: record.station_name.clone(),
                stanox: stanox.clone(),
                change_time_minutes: msn_change_time_by_tiploc.get(&record.tiploc).copied(),
            },
        );
    }

    let mut rows: Vec<TiplocCrsRow> = by_tiploc.into_values().collect();
    rows.sort_by(|a, b| a.tiploc.cmp(&b.tiploc));
    rows
}
```

(`by_tiploc: HashMap` rather than pushing straight into a `Vec` guards
against two `TI` lines for the same TIPLOC in a malformed delivery --
last-one-wins, matching this module's existing "skip/degrade malformed
input, never hard-error" posture elsewhere in this same file.)

Add a new `#[cfg(test)] mod resolve_tiploc_crs_tests` in the same file,
reusing the existing `resolve_tests` module's `ti(...)` helper (move it to
be usable from both modules, e.g. by making it a `pub(super)` free function
in `resolve_tests` and `use`-ing it, OR duplicate the same 6-line helper
into the new module -- either is fine, prefer whichever keeps the diff
smaller). Required test cases (use these exact real TIPLOC/STANOX/CRS
values -- see this plan's Context section for why each is real, not
invented):

1. `vauxhall_both_real_tiplocs_resolve_to_the_same_real_crs_when_both_ti_records_carry_it_directly`:
   two `ti(...)` records, `("VAUXHLM", "VAUXHALL", "87214", "VXH")` and
   `("VAUXHLW", "VAUXHALL", "87214", "VXH")`. Assert `resolve_tiploc_crs`
   (empty MSN maps) returns exactly 2 rows, one per TIPLOC, both `crs ==
   "VXH"`. This is the core regression test for this plan's whole point:
   `resolve` (existing, unchanged) on the same input would return only 1
   row for STANOX `87214` -- assert that too, in the same test, as the
   explicit contrast (`assert_eq!(resolve(&ti_records, &HashMap::new(),
   &HashMap::new()).len(), 1)`), so this test fails loudly if a future
   change to `resolve` itself ever accidentally "fixes" this the wrong way.
2. `vauxhall_windsor_lines_still_resolves_when_its_own_ti_crs_is_blank_and_only_msn_completes_it`:
   same two TIPLOCs, but `VAUXHLW`'s `ti(...)` CRS is `""` (blank -- `ti()`
   already maps empty string to `None`, matching `resolve_tests`'
   convention), completed via
   `msn_crs_by_tiploc = HashMap::from([("VAUXHLW".to_string(),
   "VXH".to_string())])`. Assert both rows still resolve, both `crs ==
   "VXH"` -- proves the fix works whichever of the two real completion
   paths turns out to be true for this TIPLOC in a live delivery (this
   plan's Context section explains why that was not independently
   re-verified byte-for-byte in this pass).
3. `clapham_junction_both_real_tiplocs_resolve_to_the_same_real_crs`: same
   shape as case 1, using `("CLPHMJM", "CLAPHAM JUNCTION", "87219", "CLJ")`
   and `("CLPHMJW", "CLAPHAM JUNCTION", "87219", "CLJ")`. Assert 2 rows, both
   `CLJ`.
4. `a_junction_tiploc_sharing_a_stations_stanox_with_no_own_crs_anywhere_still_does_not_resolve`:
   proves no accidental STANOX-inheritance. Two records: a real station
   (`ti("WATRLMN", "LONDON WATERLOO", "87212", "WAT")`) and a same-STANOX
   junction with a blank `TI` CRS and NO matching MSN entry
   (`ti("WATRLWC", "WATERLOO WINDSOR JN", "87212", "")`, empty
   `msn_crs_by_tiploc`). Assert `resolve_tiploc_crs` returns exactly 1 row
   (`WATRLMN`/`WAT`) -- `WATRLWC` must be absent, not defaulted to `WAT`.
   Name this test to make its purpose unmistakable in a future diff (e.g.
   the name above), and reference `journey.rs`'s "Waterloo junction" worry
   in its doc comment/body comment.
5. `ambiguous_stanox_with_two_genuine_non_x_candidates_now_resolves_both_instead_of_neither`:
   the real, currently-excluded `resolve` case (`89428`:
   `ti("ASHFKI", "ASHFORD INT (PLATS 3-4)", "89428", "ASI")` and
   `ti("ASHFKY", "ASHFORD INTERNATIONAL", "89428", "AFK")`, copied from
   `resolve_tests::ambiguous_stanox_with_two_non_x_candidates_is_excluded_entirely`).
   Assert `resolve_tiploc_crs` returns 2 rows (`ASI` and `AFK`, one each),
   in explicit contrast with `resolve`'s own existing behaviour on the same
   input (still 0 rows, per the existing unchanged test) -- state this
   contrast in the test's doc comment as a real, positive side effect of
   this design, not a required behavior change to `resolve` itself.
6. `a_tiploc_with_no_stanox_at_all_still_does_not_resolve`: `ti("FOO", "n",
   "", "BAR")` (blank stanox -> `None` per `ti()`'s own mapping). Assert
   empty result -- matches `resolve`'s existing guard, carried over
   unchanged.

Run `cargo test -p schedule-reference` and `cargo fmt --all --check` /
`cargo clippy -p schedule-reference -p common --all-features --all-targets
-- -D warnings` before committing. This task touches no other crate and no
DB -- it is pure, fully unit-testable.

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary, concerns.

## Task 2: `tiploc_crs` table, upsert, and ingest route (schema + write path)

Depends on Task 1 (needs `common::TiplocCrsRecord`).

1. Add the migration file exactly as specified in this plan's Global
   Constraints (`crates/api/migrations/20260924130000_tiploc_crs.sql`).
2. In `crates/api/src/data/queries.rs`, add `upsert_tiploc_crs`, modeled
   directly on the existing `upsert_stanox_crs` (same transaction-per-batch
   shape, same `ON CONFLICT (tiploc) DO UPDATE SET ...` pattern for every
   column except `tiploc`/`updated_at`, same `Result<u64>` return of rows
   upserted). Also add `list_tiploc_crs(pool: &PgPool) ->
   Result<Vec<common::TiplocCrsRecord>>` (whole current table, ordered by
   `tiploc`, modeled on `list_stanox_crs`) -- Task 3 needs this for
   `trip_planning.rs`.
3. In `crates/api/src/routes/ingest.rs`: add `/tiploc-crs` to `router()`
   (POST-only: `.route("/tiploc-crs", axum::routing::post(post_tiploc_crs))`)
   and a `post_tiploc_crs` handler mirroring `post_stanox_crs` exactly
   (`Json<Vec<common::TiplocCrsRecord>>` in, `queries::upsert_tiploc_crs`,
   `Json<UpsertResponse>` out). Give it a doc comment mirroring
   `post_stanox_crs`'s own, pointing at
   `queries::upsert_tiploc_crs` and this plan.
4. In `crates/api/src/app.rs`: add the auth-route entry for `/tiploc-crs`
   POST, exactly as specified in this plan's Global Constraints (mirror the
   `/schedule-network-departures` entry, including a comment in that
   file's own established style citing which existing entry's credential
   this reuses and why no GET pair exists).
5. In `crates/api/src/auth.rs`: add DB-free auth tests for the new route
   mirroring the existing POST-only route test shape used for
   `/schedule-network-departures` or `/schedule-calling-points-full` in the
   same file (schedule-reference's token accepted on POST; every other
   service's token rejected on POST; no GET handler exists at all, so don't
   write a GET test). Follow this file's existing naming convention for
   these tests exactly (e.g.
   `schedule_references_token_is_accepted_on_post_tiploc_crs`).
6. DB-gated tests in `crates/api/src/data/queries.rs` (new module,
   `tiploc_crs_query_tests`, same shape/doc-comment convention as
   `stanox_crs_lookup_query_tests`): round-trip test for
   `upsert_tiploc_crs`/`list_tiploc_crs` proving TWO rows for the SAME
   STANOX with DIFFERENT tiplocs both persist and both come back (seed
   `common::TiplocCrsRecord { tiploc: "TEST-VAUXHLM", crs: "VXH", stanox:
   "TEST-87214", ... }` and `{ tiploc: "TEST-VAUXHLW", crs: "VXH", stanox:
   "TEST-87214", ... }`, assert `list_tiploc_crs` returns both, clean up
   both by tiploc at the end). This is the direct DB-level proof this
   plan's whole point (two TIPLOCs, one STANOX, both persisted) actually
   works against a real schema, independent of the `stanox_crs` table
   entirely.

Do not touch `queries::crs_for_tiploc`, `crs_for_tiplocs_batch`, or
`list_stanox_crs_for_crs` in this task -- that is Task 3, so this task's
diff is reviewable as "new table + write path" in isolation.

Run `cargo fmt --all --check`, `cargo clippy -p api --all-features
--all-targets -- -D warnings`, `cargo test -p api -- --skip
incident_search_query_tests` (non-DB tests), and report clearly whether
`DATABASE_URL` was available in this environment to also run the new
`#[ignore]`d DB-gated tests (`cargo test -p api -- --ignored
--test-threads=1 --skip incident_search_query_tests`, after `sqlx migrate
run --source crates/api/migrations` against that database) -- if a live
Postgres is reachable, actually run them and report real pass/fail output,
not just "should work."

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary (state explicitly whether DB-gated tests
ran against a live database or were only compiled), concerns.

## Task 3: redirect TIPLOC-crosswalk reads to the union of both tables

Depends on Task 2 (needs `tiploc_crs` table + `list_tiploc_crs`).

1. In `crates/api/src/data/queries.rs`, change ONLY the SQL body (not the
   signature or return type) of these three functions to read the union of
   `tiploc_crs` and `stanox_crs`, preferring a `tiploc_crs` row when a
   TIPLOC exists in both:

   `crs_for_tiploc`:
   ```sql
   SELECT crs FROM (
       SELECT tiploc, crs FROM tiploc_crs
       UNION
       SELECT tiploc, crs FROM stanox_crs
   ) merged
   WHERE UPPER(TRIM(tiploc)) = UPPER($1)
   LIMIT 1
   ```
   (Binding and `.trim()` on the Rust side stay exactly as they are today.)

   `crs_for_tiplocs_batch`:
   ```sql
   SELECT DISTINCT UPPER(TRIM(tiploc)), UPPER(crs) FROM (
       SELECT tiploc, crs FROM tiploc_crs
       UNION
       SELECT tiploc, crs FROM stanox_crs
   ) merged
   WHERE UPPER(TRIM(tiploc)) = ANY($1)
   ```

   `list_stanox_crs_for_crs` (keep its existing name and
   `Vec<common::StanoxCrsRecord>` return type -- it is a generic "which rows
   cover this CRS" lookup, and `StanoxCrsRecord`'s shape already has every
   field a `tiploc_crs` row also has):
   ```sql
   SELECT DISTINCT ON (tiploc) tiploc, crs, station_name, stanox, source_sequence, change_time_minutes
   FROM (
       SELECT tiploc, crs, station_name, stanox, source_sequence, change_time_minutes, 1 AS priority
       FROM tiploc_crs WHERE UPPER(crs) = UPPER($1)
       UNION ALL
       SELECT tiploc, crs, station_name, stanox, source_sequence, change_time_minutes, 2 AS priority
       FROM stanox_crs WHERE UPPER(crs) = UPPER($1)
   ) merged
   ORDER BY tiploc, priority
   ```
   (`DISTINCT ON (tiploc) ... ORDER BY tiploc, priority` keeps the
   `tiploc_crs` row, priority 1, when the same TIPLOC appears in both
   tables, and still returns every distinct TIPLOC from either table
   otherwise. `StanoxCrsRow`'s `#[derive(sqlx::FromRow)]` already matches
   this column list; the `query_as::<_, StanoxCrsRow>` call site is
   unaffected.)

   Update each function's doc comment to state plainly that it now reads
   BOTH tables (a TIPLOC present in `stanox_crs` but not yet in
   `tiploc_crs` still resolves -- no existing caller/fixture breaks; a
   TIPLOC present ONLY in `tiploc_crs` -- e.g. Vauxhall's/Clapham
   Junction's previously-dropped sibling TIPLOC -- now ALSO resolves, which
   it could not before this plan), and reference this plan file.

2. In `crates/api/src/data/trip_planning.rs`'s `fetch_interchange_data`:
   after the existing loop over `stanox_rows` (unchanged), add a second
   loop over `crate::data::queries::list_tiploc_crs(pool).await?`, applying
   the SAME three writes (`change_time_by_tiploc`, `tiploc_to_crs`,
   `crs_to_tiplocs` with its existing dedup-by-contains guard) so a
   `tiploc_crs` row OVERRIDES/ADDS to whatever the `stanox_crs` loop already
   populated for that TIPLOC (run this loop AFTER the `stanox_rows` loop so
   `tiploc_crs` naturally wins on `insert` for `change_time_by_tiploc`/
   `tiploc_to_crs`; `crs_to_tiplocs`'s existing `if !siblings.contains(...)`
   guard already prevents duplicate entries either order). Update the
   function's doc comment to mention it now also reads `tiploc_crs` and
   why (link to this plan). Do not change `InterchangeData`'s shape or any
   other function in this file.

3. Update `crates/api/src/data/schedule_matching.rs`'s doc comment near
   `list_stanox_crs_for_crs`'s call site (~line 30) to reflect that the
   real TIPLOC(s) matched against now come from the union of `stanox_crs`
   and `tiploc_crs`, not `stanox_crs` alone. No code change needed there --
   `list_stanox_crs_for_crs`'s signature is unchanged.

4. Update the existing DB-gated tests in `queries.rs`'s
   `stanox_crs_lookup_query_tests` module ONLY if needed to keep passing
   (they should not need any change at all, since they only ever seed
   `stanox_crs` and the union still includes it -- if you find one needs a
   change to keep passing, treat that as a signal the union SQL above is
   wrong, not a reason to alter the test).

5. Add a NEW DB-gated test in `queries.rs` (same module or a new one,
   your call, matching this file's conventions) that is the actual
   end-to-end proof of this plan's fix at the query layer: seed ONLY
   `tiploc_crs` (via `upsert_tiploc_crs`, NOT `stanox_crs` at all) with
   `VAUXHLM`/`VXH`/stanox `TEST-VXH-STANOX` and
   `VAUXHLW`/`VXH`/stanox `TEST-VXH-STANOX` (use a clearly test-scoped
   TIPLOC/stanox prefix, e.g. `TEST-VAUXHLM`/`TEST-VAUXHLW`, to avoid ever
   colliding with a real delivery's rows in a shared test database), then
   assert:
   - `crs_for_tiplocs_batch(&pool, &["TEST-VAUXHLM".into(),
     "TEST-VAUXHLW".into()])` resolves BOTH to `"VXH"`.
   - `list_stanox_crs_for_crs(&pool, "VXH")` includes rows for BOTH
     `TEST-VAUXHLM` and `TEST-VAUXHLW`.
   Clean up both rows from `tiploc_crs` at the end.

Run `cargo fmt --all --check`, `cargo clippy -p api --all-features
--all-targets -- -D warnings`, `cargo test -p api -- --skip
incident_search_query_tests`, and (if `DATABASE_URL` is reachable) the full
`--ignored` DB-gated suite for `crates/api` -- report real pass/fail output
either way, and explicitly confirm the EXISTING `train.rs`/`trips.rs`/
`trip_planning.rs`/`reconciliation.rs` DB-gated tests that seed
`stanox_crs` directly still pass unmodified (run at least
`trip_planning`'s and `trip_planning_itinerary`'s `--ignored` tests
specifically, since this task edits `trip_planning.rs`).

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary (state explicitly whether DB-gated tests ran
against a live database), concerns.

## Task 4: `schedule-reference` publishes and consumes the new crosswalk

Depends on Task 1 (needs `resolve_tiploc_crs`/`common::TiplocCrsRecord`) and
Task 2 (needs `POST /private/tiploc-crs` to exist). Independent of Task 3.

1. In `crates/schedule-reference/src/config.rs`: add
   `tiploc_crs_url: String` following the exact pattern of the existing
   `fixed_links_url`/`schedule_calling_points_full_url` fields (own doc
   comment explaining this is the seventh/next responsibility, POST-only,
   reuses the same `internal_oauth_group_schedule_reference` writer
   credential, default value
   `"http://api:8080/private/tiploc-crs"`, env var name following this
   file's existing convention for the field name in SCREAMING_SNAKE_CASE).
2. In `crates/schedule-reference/src/main.rs`'s `poll_once`: directly after
   the existing `let rows = parser::resolve(&ti_records, &msn_crs,
   &msn_change_time);` line, add
   `let tiploc_rows = parser::resolve_tiploc_crs(&ti_records, &msn_crs, &msn_change_time);`
   and build+POST the corresponding `Vec<common::TiplocCrsRecord>` batch to
   `config.tiploc_crs_url`, using the SAME `source_sequence` already
   computed for the `stanox_crs` batch, via `common::ingest::post_batch`
   (mirror the existing `records`/`post_batch(... "stanox/crs rows")` call
   exactly, including its logging shape, but with noun `"tiploc/crs rows"`).
   This POST failing must NOT abort the cycle or prevent the existing
   `stanox_crs` POST/advance-`last_processed_delivery` logic from
   proceeding -- match this file's other "best-effort, log and continue"
   publishes (e.g. `publish_fixed_links`'s own posture) rather than the
   `stanox_crs` POST's own "only advance on success" posture, since
   `tiploc_crs` is a strict superset used for defense-in-depth, not the
   record advancing dedup state.
3. Change `publish_cif_derived_products`'s signature to ALSO take
   `tiploc_crs_records: &[common::TiplocCrsRecord]` (in addition to its
   existing `stanox_crs_records: &[common::StanoxCrsRecord]` parameter --
   do not remove that parameter, `log_new_unresolved_booked_tiplocs` still
   needs SOME map and either source is fine for it since it is a superset
   relationship; use your judgment on which to pass it, documented inline),
   and update its call site in `poll_once` to pass `&tiploc_rows` computed
   in step 2.
4. In the THREE `tiploc_to_crs` construction sites inside
   `crates/schedule-reference/src/main.rs` (the ones building
   `std::collections::HashMap<String, String>` by mapping
   `stanox_crs_records.iter().map(|r| (normalize_tiploc(&r.tiploc)...,
   r.crs.clone()))` -- currently around the `departures_by_crs` and
   `departures_by_destination_crs` call sites, i.e. the functions
   containing those two calls and taking `stanox_crs_records: &[common::StanoxCrsRecord]`
   as a parameter): change their parameter type to
   `tiploc_crs_records: &[common::TiplocCrsRecord]` and build the map from
   THAT instead (same `normalize_tiploc(&r.tiploc)... -> r.crs.clone()`
   shape, just a different source slice/type), updating their callers
   accordingly so `&tiploc_rows` (or the equivalent value threaded through
   from step 3) reaches them. `log_new_unresolved_booked_tiplocs` may keep
   using `stanox_crs_records` OR switch to `tiploc_crs_records` -- your
   call, but state which and why in your report (switching to
   `tiploc_crs_records` gives it visibility into the same superset the
   other two now use, which is the more consistent choice, but is not
   required by this plan).
   Update each touched function's doc comment to reflect the new parameter
   name/type and why (link to this plan).
5. Add/update unit tests in `crates/schedule-reference/src/main.rs`'s own
   test module for any of the three functions from step 4 that already
   have unit tests exercising their `tiploc_to_crs` construction (search
   for existing tests calling them) -- update the test fixtures to build
   `common::TiplocCrsRecord` values instead of `common::StanoxCrsRecord`
   wherever the changed parameter is exercised. Do not remove test
   coverage; adapt it to the new type.

Run `cargo fmt --all --check`, `cargo clippy -p schedule-reference
--all-features --all-targets -- -D warnings`, `cargo test -p
schedule-reference`.

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary, concerns.

## Task 5: close the doc-comment gap, regression tests in `journey.rs`, reference-data note

Depends on Tasks 1-4 conceptually (documents the now-real fix), but touches
entirely different files, so can be dispatched once Task 3 is complete
(needs the union-read behavior to describe accurately) without waiting on
Task 4.

1. In `crates/api/src/data/journey.rs`, rewrite `tiploc_key`'s doc comment
   item 1 (the "A real station reached via a line-group TIPLOC..." bullet,
   lines ~89-103) to state, in this codebase's own established
   "what/why/evidence" doc style: this gap is now closed by
   `tiploc_crs` (`crates/api/migrations/20260924130000_tiploc_crs.sql`) and
   `crates/schedule-reference::parser::resolve_tiploc_crs`; both Vauxhall
   TIPLOCs (`VAUXHLM`/`VAUXHLW`) and both Clapham Junction TIPLOCs
   (`CLPHMJM`/`CLPHMJW`) now resolve; cite this plan file
   (`docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md`) the
   same way other closed-then-superseded doc comments in this codebase cite
   their own closing design (e.g. how `stanox-crs.md`'s "This file's role
   since the live table" section cites the 2026-09-01 design doc). Do not
   delete the historical explanation of WHY the gap existed (the STANOX
   primary-key constraint) -- keep it as context for why the fix looks the
   way it does, same as this file's other doc comments preserve history
   rather than erasing it.
2. Replace the existing characterization test
   `a_line_group_tiploc_absent_from_the_stanox_keyed_crosswalk_is_still_unresolved`
   (~line 1718) with a regression test proving the fix, in the same test
   module, using the same `stops_from_calling_points`/`raw_cp` test
   helpers already in this file. Since `stops_from_calling_points` takes a
   plain `tiploc_to_crs: &HashMap<String, String>` (source-agnostic -- the
   real fix lives in how `queries::crs_for_tiplocs_batch` now BUILDS that
   map, which Task 3 already covers and tests at the query layer), this
   test's job is to prove that once such a map correctly contains BOTH
   real TIPLOCs (as it now can, post-fix), BOTH calling points resolve in
   the SAME journey -- something the OLD, STANOX-truncated map could never
   have contained simultaneously. Concretely:
   ```rust
   #[test]
   fn both_of_vauxhalls_real_tiplocs_resolve_when_the_crosswalk_holds_both() {
       // Regression test for tiploc_key's "What this does NOT fix" item 1
       // (now closed -- see docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md
       // and crates/schedule-reference::parser::resolve_tiploc_crs). Before
       // that fix, `tiploc_to_crs` could only ever contain ONE of
       // VAUXHLM/VAUXHLW at a time (stanox_crs's one-row-per-STANOX
       // limit) -- this test's whole point is that it now legitimately
       // contains BOTH, and both calling points on the same journey
       // resolve, exactly as they should on real train `L82877`.
       let service_date: NaiveDate = "2026-09-14".parse().unwrap();
       let tiploc_to_crs: HashMap<String, String> = [
           ("VAUXHLM".to_string(), "VXH".to_string()),
           ("VAUXHLW".to_string(), "VXH".to_string()),
       ]
       .into_iter()
       .collect();

       let stops = stops_from_calling_points(
           &[raw_cp("VAUXHLM"), raw_cp("VAUXHLW")],
           &tiploc_to_crs,
           service_date,
       );

       assert_eq!(stops[0].crs, Some("VXH".to_string()));
       assert_eq!(stops[1].crs, Some("VXH".to_string()));
   }
   ```
   Add a second test in the same shape for Clapham Junction
   (`CLPHMJM`/`CLPHMJW`, both `CLJ`). Keep
   `a_tiploc_with_no_crosswalk_row_still_degrades_to_none_rather_than_guessing`
   (the `SHCKLGJ` test directly below it) completely unchanged -- it is
   still valid and still needed.
3. In `reference-data/stanox-crs.md`, add a short new section (after "This
   file's role since the live table (2026-09-01)", before "A real
   discrepancy this transition surfaced") titled something like "A second,
   TIPLOC-primary table since 2026-09-24", explaining in 1-2 short
   paragraphs: `tiploc_crs` exists alongside the live `stanox_crs` table
   described above, is additive (does not replace it), and closes the
   specific real gap this file's own STANOX-grouping model cannot close on
   its own (a STANOX covering more than one genuine, differently-CRS'd-or-
   identically-CRS'd calling-point TIPLOC) -- cite Vauxhall/Clapham
   Junction and this plan file
   (`docs/superpowers/plans/2026-09-24-tiploc-crs-crosswalk-plan.md`).

Run `cargo fmt --all --check`, `cargo clippy -p api --all-features
--all-targets -- -D warnings`, `cargo test -p api -- --skip
incident_search_query_tests` and confirm the two new/updated `journey.rs`
tests pass (they are plain unit tests, no DB needed).

Report file contract: DONE / DONE_WITH_CONCERNS / NEEDS_CONTEXT / BLOCKED,
commits, one-line test summary, concerns.
