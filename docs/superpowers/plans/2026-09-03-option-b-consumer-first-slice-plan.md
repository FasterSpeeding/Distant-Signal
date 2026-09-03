# Plan: A Pure CIF SCHEDULE Query Library (`crates/schedule-query`) — Option B's First Safe Slice

**Status: implementation plan for a narrow, pre-approved slice only.** Gated
on `docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md`'s
verdict (read that first — it is not repeated here): this plan builds
**only** the piece that document found safe to build ahead of
`docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
reaching "go" — a pure, offline, no-network CIF SCHEDULE parsing/matching
library. It does **not** build any part of Option B's actual consumer
(no Kafka, no live TRUST data, no writes, no wiring into
`merge_full_coverage` or `trust-consumer`'s live matching). If any task
below is read as drifting toward those things, stop and re-read the scoping
doc's "Explicitly out of scope" section — that boundary is deliberate and
load-bearing, not an oversight to route around.

## What this plan produces

A new crate, `crates/schedule-query`, a **pure library** (`lib.rs`, no
`main.rs`, no binary shipped via Helm, no dependency on `tokio`, `reqwest`,
`sqlx`, or `rdkafka`) that:

1. Parses `BS`/`BX`/`LO`/`LI`/`CR`/`LT` records from a CIF SCHEDULE `MCA`
   extract's text (the same `RJTTF*MCA.txt` file `crates/schedule-reference`
   already reads for `TI` records — this crate reads the *other* record
   family from the same file, and does not duplicate or depend on
   `schedule-reference`'s own `TI`/`A` parsing).
2. Resolves STP overlays correctly: for a given `(UID, date)`, picks the
   schedule record whose date range and day-of-week bitmask cover that
   date, preferring the lowest-alphabetically STP indicator (`C` beats `N`
   beats `O` beats `P`) — matching the design spec's and findings doc's own
   independently-confirmed rule, including the `C` = "cancelled that day,
   no body" case.
3. Answers two read-only queries against an in-memory, pre-parsed index:
   `schedule_for_uid(uid, date) -> Option<ResolvedSchedule>` (the direct
   `train_uid` → booked-schedule bridge `trust-consumer/src/matching.rs`'s
   own module doc names as missing) and `schedules_touching(tiplocs,
   date) -> Vec<ResolvedSchedule>` (the line-population query a future
   full-coverage consumer would need).
4. Handles the two real parsing gotchas the validation findings doc already
   found the hard way, so nobody re-discovers them: the schedule-body
   TIPLOC field is fixed 7-character space-padded (`"EUSTON "`), and a `C`
   (STP=Cancelled) record has no `LO`/`LI`/`LT` body lines at all.

Nothing in this crate makes a network call, touches a database, or is
invoked from any of `trust-consumer`, `aggregator`, or `api`'s production
code paths. It is built, tested, and left unused — exactly the posture the
already-merged full-coverage presentation scaffolding already established
as acceptable in this repo, for the same underlying reason (a real producer
doesn't exist/isn't validated yet), applied to a lower, purer layer.

## Non-goals — binding for every task below

- **No I/O of any kind inside the library.** Every public function takes
  `&str` (already-read file content) or already-parsed structures in, and
  returns plain data out — mirroring
  `crates/schedule-reference/src/parser.rs`'s own established "parsing
  logic pure and testable separately from I/O" convention exactly, cited
  directly in that module's own doc comment.
- **No new Kafka consumer, HTTP route, database table, or migration.**
- **No wiring into `crates/trust-consumer`, `crates/aggregator`, or
  `crates/api`.** This crate is not added as a dependency of any of them in
  this pass. A future pass may propose that (see the scoping doc's "Explicitly
  out of scope" and Open Question 2) — not this one.
- **No change to `LineDefinition.full_coverage_enabled`, `merge_full_coverage`,
  or any file the already-merged full-coverage scaffolding touches.**
- **No CIF `AA` (Association) records, no freight-specific fields, no
  record type not already independently exercised against real data in the
  four validation sessions.** If a real fixture line uses a field this plan
  doesn't decode, leave it undecoded and note it in the field's own doc
  comment — do not guess at a schema for it.
- **Test fixtures are small, real, hand-extracted excerpts — not the full
  `timetable_full.zip`.** That file is untracked, ~711MB uncompressed, not
  present in this worktree, and per this repo's own established convention
  (every validation session streamed it via `unzip -p`, never extracted or
  committed it) stays out of version control. Fixtures for this plan are
  literal byte-for-byte real CIF lines already quoted, and therefore already
  legitimately citable, in
  `docs/superpowers/specs/2026-08-29-trust-schedule-delay-validation-findings.md`
  (e.g. the `C11052` STP=P/STP=C Bank Holiday pair, the `F26094`/`Q98537`
  replacement-UID bodies, the `C01370` multi-station WCML pattern) plus a
  handful of synthetic-but-byte-layout-correct lines for edge cases the real
  quotes don't happen to cover (e.g. a `BX` extension record, a `CR` change-
  en-route record) — synthetic lines must be clearly commented as such, not
  presented as real.

## Task 1: Crate scaffolding

- [ ] Create `crates/schedule-query/Cargo.toml` — a lib-only crate.
  Dependencies: `chrono` (date/day-of-week handling, matching the version
  already pinned in `crates/schedule-ingest/Cargo.toml`), `serde`
  (`derive` feature, for the public structs — needed if this crate's output
  is ever handed across a process boundary later, cheap to add now, costs
  nothing while unused). Deliberately **no** `common` dependency unless a
  concrete shared type is found worth reusing during implementation (check
  first; don't add it speculatively).
- [ ] Add `crates/schedule-query` to the workspace `Cargo.toml`'s
  `members` list (confirm the existing workspace manifest's exact
  shape/location before editing — read it, don't assume).
- [ ] `src/lib.rs`: module doc explaining this crate's scope in the same
  terms as this plan's "What this plan produces" section — a pure schedule
  index, no I/O, no production wiring, built as Option B's first safe
  slice per the scoping doc. Cite both this plan and the scoping doc by
  path.

## Task 2: Record types and fixed-width parsing

- [ ] `src/records.rs`: plain structs for the record shapes this crate
  decodes, each carrying a doc comment citing the exact byte-offset source
  (the design spec's and verification findings' own decoded layouts,
  re-verified against the real fixture bytes in Task 4's tests, not
  re-derived from memory):
  - `BasicSchedule { uid: String, stp_indicator: StpIndicator, date_from: NaiveDate, date_to: NaiveDate, days_of_week: [bool; 7], }` — from a `BS` line. `stp_indicator` as a real enum (`Permanent`, `Overlay`, `Cancellation`, `New`), not a bare `char`, with an explicit `Ord`/`PartialOrd` impl (or a `fn precedence(&self) -> u8`) encoding "lowest wins" directly, so the resolution logic in Task 3 reads as a `min_by_key` rather than hand-rolled comparison logic.
  - `CallingPoint { tiploc: String, kind: CallingPointKind, booked_arrival: Option<NaiveTime>, booked_departure: Option<NaiveTime>, is_half_minute_arrival: bool, is_half_minute_departure: bool }` where `CallingPointKind` is `Origin` (`LO`), `Intermediate` (`LI`), `Terminate` (`LT`) — from the corresponding body lines. The `H`-suffix half-minute marker (confirmed real in the findings doc's own quoted output, e.g. `MKC@0750H`) is captured as its own boolean, not silently dropped or rounded.
  - `RawSchedule { basic: BasicSchedule, calling_points: Vec<CallingPoint> }` — one `BS`(+`BX`)/`LO`/`LI`*/`LT` block, pre-STP-resolution. A `Cancellation`-indicator `RawSchedule` has an empty `calling_points` (per the findings doc's own confirmed "`C` meaning cancelled-that-day with no body at all").
- [ ] `src/parse.rs`: `pub fn parse_schedule_records(text: &str) -> Vec<RawSchedule>` — streams `text.lines()`, groups by the real CIF block structure (`BS` starts a block, optional `BX` extends it, `LO`/`LI`*/`LT` are its body, terminated implicitly by the next `BS`/`ZZ`), matching `RJTTF942MCA.txt`'s real observed shape (findings doc's own quoted `awk`-based grouping in its Task 3 section is a plain-text precedent for this same grouping logic, reimplemented properly here rather than as a shell one-liner). A line shorter than its record type's minimum real width, or an unrecognized record-type prefix, is skipped with a `tracing::debug!` (or a returned count of skipped lines, if this crate stays fully log-free — decide during implementation, matching whichever posture `schedule-reference/src/parser.rs` already established, since consistency with that sibling crate is worth more than a fresh decision here) — never a hard parse failure for one bad line, mirroring `parse_ti_lines`'s own documented "a single malformed line must not abort the whole extraction" posture exactly.
- [ ] **Field offsets to implement, decoded from real bytes already quoted in the findings doc's 2026-08-29 "Task 3" section** (`BSNC005732605172612060000001 PXX1S003101121194800 DMU    125      S A T        P`): UID at a fixed offset, Date Runs From/To as 6-digit `YYMMDD`, days-of-week as a 7-character bitmask, STP indicator as the record's final significant character (`P` in the quoted example). Confirm each offset against this exact quoted line plus at least one more real quoted line with a different STP indicator (the `C11052`/`260831` pair) before trusting it — do not assume the design spec's prose description of field positions is byte-exact without checking it against real bytes, per this repo's own "no invented API details" convention.

## Task 3: STP-overlay resolution

- [ ] `src/resolve.rs`: `pub fn resolve_for_date(raw: &[RawSchedule], uid: &str, date: NaiveDate) -> Option<ResolvedSchedule>` — filters `raw` to records matching `uid` whose date range and day-of-week bitmask cover `date`, then picks the one with lowest `StpIndicator` precedence (Task 2's `Ord` impl does the comparison). Returns `None` if no record covers that UID/date at all. Returns `Some(ResolvedSchedule { cancelled: true, calling_points: vec![] , .. })` (or an equivalent explicit "runs but empty" shape — decide the exact enum/struct split during implementation, but the *distinction* between "no schedule at all for this UID/date" and "a schedule exists and says cancelled" must be preserved, not collapsed to the same `None`) when the winning record's indicator is `Cancellation`.
- [ ] `pub fn schedules_touching(index: &ScheduleIndex, tiplocs: &[&str], date: NaiveDate) -> Vec<ResolvedSchedule>` — resolves every UID in the index for `date` (via `resolve_for_date`), keeps only the resolved (non-cancelled) results whose `calling_points` include at least one of `tiplocs`, comparing with the padding-normalization from Task 4. This is the line-population query.
- [ ] `pub struct ScheduleIndex` — a thin wrapper grouping `Vec<RawSchedule>` by `uid` (a `HashMap<String, Vec<RawSchedule>>`) built once via `ScheduleIndex::build(raw: Vec<RawSchedule>)`, so `schedule_for_uid`/`schedules_touching` aren't re-scanning a flat `Vec` on every call. `ScheduleIndex::from_text(text: &str) -> Self` composes Task 2's `parse_schedule_records` with `build` as the one convenience entry point most callers will actually use.

## Task 4: TIPLOC padding/matching helper

- [ ] `src/tiploc.rs`: `pub fn normalize_tiploc(raw: &str) -> &str` (or `String`, whichever avoids an unnecessary allocation at call sites — decide during implementation) — trims the fixed 7-character space-padding the findings doc's Task 5 section already found real and load-bearing (`"EUSTON "` vs. `lines/*.toml`'s bare `"EUSTON"`). Used internally by `schedules_touching`'s TIPLOC-membership check, and exported so a future caller matching this crate's output against `lines/*.toml`'s own `Station.tiploc` field doesn't have to rediscover the gotcha.
- [ ] Unit test asserting `normalize_tiploc("EUSTON ") == "EUSTON"` and `normalize_tiploc("EUSTON") == "EUSTON"` (idempotent on already-trimmed input, since callers may pass either shape).

## Task 5: Tests against real, quoted CIF fixture data

- [ ] `tests/fixtures/` (or `#[cfg(test)]` inline constants — prefer inline for a handful of short blocks, a `tests/fixtures/*.txt` file if the blocks grow unwieldy): real, byte-for-byte lines transcribed directly from the findings doc's own quoted output, each with a comment citing which findings-doc section it's quoted from:
  - The `C11052` STP=P base pattern plus its real `STP=C`/`260831` Bank Holiday cancellation override (2026-08-31/09-01 section) — tests that `resolve_for_date` picks the base pattern for an ordinary Tuesday and the cancellation for `2026-08-31` specifically.
  - The real `F26094` STP=N Bank Holiday replacement body (`LOEUSTON 1130 -> ... -> LIHTCHEND 1135/1136H -> ... -> LIBUSHEY 1148/1149`) — tests full `LO`/`LI`/`LT` body decoding including the half-minute marker, and cross-checked against the same session's real observed TRUST pin locations (`HRW` → `BSH`) as a sanity narrative in the test's own doc comment (not asserted programmatically against TRUST data — this crate has no TRUST dependency — just cited as corroboration).
  - The `C01370`/`C17755`/`C17798` WCML multi-station examples (2026-08-29 Task 3 section) — tests `schedules_touching` against WCML's real five sample TIPLOCs (`EUSTON`, `MKNSCEN`, `CREWE`, `PRSTON`, `CARLILE`) returns the expected UIDs.
  - A synthetic (clearly commented as such) minimal `BS`+`BX`+`LO`+`LT` two-point block for the smallest possible valid schedule, to pin down parsing without real-data noise.
  - A synthetic malformed line (too short, unrecognized record type) to confirm Task 2's "skip, don't abort" behavior.
- [ ] A test asserting `schedule_for_uid` on a UID/date combination *not* present in the fixture index returns `None`, not a panic or a default.
- [ ] Run `cargo test -p schedule-query` and confirm all tests pass before moving on — per `superpowers:test-driven-development`, write each test before (or alongside) the code it exercises, not after, for every task above.

## Task 6: A dev-only manual smoke-test binary (optional, not deployed)

- [ ] `examples/inspect.rs` (Cargo `examples/`, not `src/bin/` — never built into a container image, never referenced by any Helm chart or Dockerfile): reads a path from `std::env::args()`, calls `ScheduleIndex::from_text` on its content, and prints a `schedule_for_uid` or `schedules_touching` result for a UID/TIPLOC-list/date passed as further args. Purpose: lets a human manually re-run this crate against the real, local, untracked `timetable_full.zip` (via `unzip -p timetable_full.zip RJTTF942MCA.txt | cargo run -p schedule-query --example inspect -- <uid> <date>`) to sanity-check the library against the full real extract, the way every validation session's own throwaway scripts already did — without committing any of that real data or making this crate depend on the file being present for `cargo test` to pass. Explicitly labeled in its own top comment as a manual dev tool, not part of any deployed service.

## Task 7: Documentation and commit

- [ ] `crates/schedule-query/src/lib.rs`'s module doc (Task 1) finalized to
  reflect what actually got built, cross-linking both this plan and the
  scoping doc by path, and stating plainly, in the doc comment itself, that
  this crate is unused by any production binary as of this commit — mirroring
  the already-merged full-coverage scaffolding's own "kept honest about being
  inert" precedent (e.g. `LineDefinition.full_coverage_enabled`'s own doc
  comment: "nothing consumes this yet").
- [ ] `cargo fmt`, `cargo clippy -p schedule-query`, `cargo test -p
  schedule-query` all clean.
- [ ] Commit with a message stating plainly what this is and is not: a pure,
  offline CIF SCHEDULE query library, proven against real quoted CIF data,
  not wired into any production path, per
  `docs/superpowers/specs/2026-09-03-option-b-consumer-scoping.md`'s verdict.

## Explicitly out of scope (repeated from the scoping doc, binding here too)

- Wiring this crate into `trust-consumer`, `aggregator`, or `api`.
- Any Kafka consumer, HTTP route, or database schema.
- Populating `merge_full_coverage`'s production call site.
- CIF `AA` records or any record type not exercised by real fixture data.
- A performance/memory benchmark against the full 711MB extract (named as a
  real future need in the scoping doc's Open Questions, not performed here).

## Open questions / risks

1. **Whether `ResolvedSchedule`'s exact shape (a struct vs. an enum
   distinguishing "cancelled" from "not found") is the right one for a
   future consumer is a guess**, per the scoping doc's own Open Question 2
   — accepted as a bounded risk since the parsing/resolution logic
   underneath it is the expensive part and is reusable regardless.
2. ~~Byte offsets are pinned against the specific real lines quoted in the
   findings doc, not against the full extract~~ **Resolved 2026-09-03,
   post-merge**: ran Task 6's `examples/inspect.rs` against the real,
   local `timetable_full.zip` (`RJTTF948MCA.txt`, a later real delivery
   than the `RJTTF942MCA.txt` sample this crate's fixtures were quoted
   from). Clean parse across the entire file — 463,947 real `BS` records,
   234,941 distinct UIDs, zero parse errors/panics — and a `touching`
   query against the real WCML TIPLOCs
   (`EUSTON,MKNSCEN,CREWE,PRSTON,CARLILE`) on a real date returned 1227
   real, well-formed schedules with plausible calling-point sequences
   (e.g. `KGMRJCN` → `CARLCJN` → `CARLILE`, correct `Terminate` kind and
   arrival time). This confirms the byte offsets hold against the full
   real extract, not just the quoted fixture lines — the residual risk
   this open question named is closed.
