# TfL Line Status Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Transport for London line status — Underground, DLR, London Overground, Elizabeth line and Tram — as a second data source alongside National Rail, so those 20 lines appear on `/lines`, on the dashboard, on their own detail and history pages, and in the TfL-shaped read API, using the architecture that already exists.

**Architecture:** A fifth poller crate (`crates/poller-tfl`) fetches `GET https://api.tfl.gov.uk/Line/Mode/{modes}/Status` every 300s, maps each line to the existing `common::LineStatusReport` wire type, and POSTs the batch to a new `/private/tfl-line-status` ingest endpoint. Unlike the National Rail path, nothing is inferred: TfL publishes line status directly, so the `api` crate writes it straight into `line_status`/`line_status_history` and the `aggregator` is never involved. The two writers are kept apart by a new `line_status.source` column ('aggregator' | 'tfl') and by a `tfl-` prefix on every TfL line id. Everything downstream — `/Line/{ids}/Status`, `/Line/{id}/Status/{from}/to/{to}`, the frontend's tables, cards, issue list and history page — is already mode-agnostic and picks the new lines up once the mode gate on `/Line/Mode/{mode}/Status` is relaxed and the line list learns about them.

**Tech Stack:** Rust 2024 edition (axum 0.8, sqlx 0.8 with runtime-checked queries, reqwest 0.13, clap 4 with `env`, chrono, serde/serde_repr), PostgreSQL 16 with `sqlx::migrate!` migrations, Next.js 16 App Router + Mantine v9 + Vitest on the frontend, docker compose + a Helm chart for deployment.

**Spec:** None. There is no separate design doc for this feature — the decisions, their evidence and their rejected alternatives live in Global Constraints below, the same convention `docs/superpowers/plans/2026-08-22-ux-review-fixes.md` uses.

## Global Constraints

- **Rationale lives here, not in a separate spec file.** Every non-obvious choice below was checked against the real source in this repo or against the live TfL API on 2026-08-22, and the check is recorded with it. Where a task makes a call that a reviewer could reasonably make differently, it carries a **Decision** note.

- **v1 is line status only.** No station/stop-level TfL data (arrivals, per-stop disruptions), no journey planning, no `bus`/`river-bus`/`cable-car`/`national-rail`-via-TfL modes. `tram` **is** in scope. The hard reason for the line-only scope: this app's `stations.crs` column is `CHAR(3)` (`crates/api/migrations/20260510023522_initial.sql`) and the station route is `/stations/[crs]`, while TfL StopPoints are Naptan ids like `940GZZLUABC`. Nothing in this plan touches `stations`, `station_samples`, `/StopPoint/{crs}/Disruption`, or the `lines/*.toml` catalogue's station lists.

- **The operator is the literal string `"TfL"` for every ingested line** — tube, DLR, Overground, Elizabeth line and tram alike. There is no per-line ATOC-style operator code to derive, unlike National Rail. It is defined once as `common::TFL_OPERATOR` (Task 1) and used by both the poller and the `api` crate.

- **Every TfL line id is namespaced `tfl-`.** This is not cosmetic. `line_status.line_id` is the PRIMARY KEY, and TfL's tube line id is `northern` while `lines/northern.toml` line 1 is `id = "northern"` — two different railways, one primary key. Verified against the live feed on 2026-08-22: the 20 in-scope lines are `bakerloo, central, circle, district, dlr, elizabeth, hammersmith-city, jubilee, liberty, lioness, metropolitan, mildmay, northern, piccadilly, suffragette, tram, victoria, waterloo-city, weaver, windrush`. `northern` is the only collision today; the prefix removes the whole class of them, and it also makes `/lines/tfl-victoria` self-describing. The prefix is `common::TFL_LINE_ID_PREFIX` (Task 1) — never hand-written.

- **Historical data comes from this app's own archive, never from TfL.** TfL's Unified API has no historical/archive endpoint — only current status plus planned works up to ~12 weeks ahead. `/Line/{id}/Status/{from}/to/{to}` already reads `line_status_history` (`crates/api/src/data/queries.rs::line_status_history_for_range`) and is line-id-generic, so TfL history works the moment the ingest starts appending history rows, and only covers the period since this feature shipped. Do not add a "fetch history from TfL" path; it does not exist.

- **The TfL severity scale needs five new `Severity` variants, and code 20 is an active collision.** Fetched live from `https://api.tfl.gov.uk/Line/Meta/Severity` on 2026-08-22: codes run 0–20, and the descriptions are **byte-identical across all five in-scope modes** (checked tube vs dlr vs overground vs elizabeth-line vs tram — zero differences). Codes 0–14 already match `common::Severity` one-for-one. Codes 15–20 do not: TfL 15 is `Diverted` (ours is 21), TfL 16–19 (`Not Running`, `Issues Reported`, `No Issues`, `Information`) have no equivalent at all, and **TfL 20 is `Service Closed` while our discriminant 20 is the NR extension `Recovering`**. This is live, not theoretical — at the time of capture 13 of the 20 lines were reporting severity 20. Renumbering is not an option (the discriminant is what `Serialize_repr` writes into the `line_status.statuses` JSONB, what `render.rs` emits as `statusSeverity`, and what `frontend/lib/severity.ts`'s table keys off), so Task 1 adds new variants at 22–26 and maps TfL's codes onto them explicitly.

- **Do not fetch `/Line/Meta/Severity` at runtime.** The brief suggested caching it rather than hardcoding the scale, because TfL documents it as mode-dependent. Checked: for the five modes in scope it is not — see above — and every `lineStatuses[]` entry already carries its own `statusSeverityDescription` inline, so a per-cycle meta request would buy nothing the response body doesn't already contain. Instead the mapping table is pinned by a test that transcribes the live table verbatim (Task 1), so a future change to TfL's scale shows up as a test failure to investigate rather than as silently mis-rendered status.

- **An unrecognised severity code is recorded, never dropped and never guessed.** `severity_from_tfl_code` returns `None` for anything outside 0–20; the poller maps that to `Severity::Information` (grey, "informational"), logs a warning, and puts TfL's own `statusSeverityDescription` in the `reason` so the user still reads what TfL said. Dropping the status would make a disrupted line render as Good Service (`frontend/lib/severity.ts::worstStatus` returns severity 10 for an empty status list), and guessing a colour for a code we do not understand is worse than saying "Information".

- **`line_status` rows get an owner column, because the aggregator currently deletes anything it does not recognise.** `crates/aggregator/src/queries.rs::prune_removed_lines` runs `DELETE FROM line_status WHERE NOT (line_id = ANY($1))` every cycle with only its own (static + custom) line ids. Without Task 2, every TfL row would be deleted within one aggregation cycle (5s in dev, 60s in prod). The new `source TEXT NOT NULL DEFAULT 'aggregator'` column scopes that DELETE and gives the TfL ingest its own prune.

- **Decision: TfL lines are NOT added to `lines/*.toml`.** The obvious move is a TOML file per TfL line, and it is wrong here for three reasons. (1) The aggregator loads that directory (`crates/aggregator/src/main.rs`, `LINES_DIR`) and builds a `LineStatusReport` for **every** line in it, so each cycle it would overwrite the freshly-ingested TfL status with a Good-Service fallback derived from zero incidents and zero samples. (2) A `LineDefinition` is 90% route topology — ordered CRS stations, segments, sample stations, match keywords, severity thresholds — and none of it applies to a feed that hands us the finished status. (3) It would go stale: TfL split "London Overground" into six named lines in 2024, and the ids are documented as able to drift. So `GET /public/lines` derives its TfL entries from the `line_status` rows the poller wrote (Task 7), which is authoritative by construction and self-heals when TfL renames something.

- **Poll cadence is 300s, matching `poller-incidents`.** TfL publishes no recommended interval for `/Line/Mode/.../Status`, and there is no push, no webhook and no confirmed ETag support. The registered free tier is documented as ~500 requests/minute but community reports say enforcement is inconsistent, so the poller does not assume a budget: it makes exactly one request per cycle and retries a 429 or 5xx up to three times with exponential backoff (Task 4).

- **Attribution is a licence obligation, not a nicety.** TfL's open data is published under a modified Open Government Licence v2.0 that requires "Powered by TfL Open Data" wherever the data is presented. Task 9 adds it. The licence's additional Ordnance Survey / Geomni attributions apply to TfL's *geographic* data (stop locations, maps) — none of which v1 ingests — so they are deliberately not included, and that reasoning is written into the component.

- **Testing conventions, verified against the repo.** Rust: pure functions are unit-tested in a `#[cfg(test)] mod tests` at the bottom of the same file (`crates/common/src/ingest.rs`, `crates/api/src/data/queries.rs`, `crates/poller-incidents/src/schema.rs`, `crates/api/src/render.rs` all do this); parsing is tested against a literal fixture string; anything that needs Postgres is a `#[tokio::test] #[ignore = "requires a live database; ..."]` (the only example is `crates/aggregator/src/queries.rs::load_incidents_excludes_cleared_rows`). Frontend: every file in `components/` and `lib/` has a `.test.ts(x)` sibling and is rendered through `frontend/test/render.tsx`'s `renderWithMantine`; **no file under `app/` has a component test** (the only `app/` test is `app/globals.test.ts`, which reads CSS text), so Server Component pages are verified by hand against the running stack.

- **Running the `#[ignore]`d database tests.** `docker-compose.yml` publishes no host port for postgres, so use the container's address on the compose network (Linux host):

  ```bash
  docker compose --env-file dev.env up -d postgres api
  set -a; . ./dev.env; set +a
  DB_IP=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' \
    "$(docker compose --env-file dev.env ps -q postgres)")
  export DATABASE_URL="postgres://$POSTGRES_USER:$POSTGRES_PASSWORD@$DB_IP:5432/$POSTGRES_DB"
  ```

  The `api` service must be up at least once first — it is what runs `sqlx::migrate!` (`crates/api/src/main.rs`).

- **Every command in this plan runs from the repo root** unless it says `cd frontend`. Rust tests are `cargo test -p <crate>`; there is no CI workflow and no justfile — the tests are the gate.

- **Commit after each task**, using the command given in the task's final step.

---

## Phase 0 — Shared vocabulary

### Task 1: Teach `Severity` the six TfL codes it does not know

**Files:**
- Modify: `crates/common/src/lib.rs`
- Modify: `frontend/lib/severity.ts`
- Modify: `frontend/lib/severity.test.ts`
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/components/IssueList.tsx`

**Interfaces:**
- Produces: `common::Severity::{ServiceClosed, NotRunning, IssuesReported, NoIssues, Information}`, `common::severity_from_tfl_code(code: u8) -> Option<Severity>`, `common::DataQuality::Tfl`, `common::TFL_OPERATOR: &str`, `common::TFL_LINE_ID_PREFIX: &str`. Consumed by Task 4 (the poller's mapping) and Task 7 (`/public/lines`).
- Produces: rows 22–26 in `frontend/lib/severity.ts`'s `SEVERITY_TABLE` and a `tfl` entry in `IssueList`'s `DATA_QUALITY_LABELS`. Consumed by every page that renders a `StatusBadge`.

The Rust and TypeScript halves are one task on purpose: `crates/common/src/lib.rs`'s existing `rank_matches_the_frontends_group_table` test asserts, row by row, that `severity_rank` agrees with `frontend/lib/severity.ts`'s `SEVERITY_TABLE` + `GROUP_RANK`. Adding variants on one side only leaves that test red by design.

The mapping, transcribed from a live `GET https://api.tfl.gov.uk/Line/Meta/Severity` on 2026-08-22 (identical for tube, dlr, overground, elizabeth-line and tram):

| TfL code | TfL description | maps to | our discriminant |
|---|---|---|---|
| 0–14 | (identical meanings) | the existing variants | 0–14 |
| 15 | Diverted | `Severity::Diverted` | 21 |
| 16 | Not Running | `Severity::NotRunning` | 23 (new) |
| 17 | Issues Reported | `Severity::IssuesReported` | 24 (new) |
| 18 | No Issues | `Severity::NoIssues` | 25 (new) |
| 19 | Information | `Severity::Information` | 26 (new) |
| 20 | Service Closed | `Severity::ServiceClosed` | 22 (new) |

**Decision — `Service Closed` is `informational` (grey), not `severe` (red).** It is the ordinary overnight state of the Underground: at the moment of capture, 13 of the 20 lines were reporting it. Painting the whole network red every night would be false alarm, and TfL's own site greys it. `Not Running` — the code for a service that is unexpectedly absent — stays `severe`.

- [ ] **Step 1: Write the failing Rust tests**

Append to `crates/common/src/lib.rs`:

```rust
#[cfg(test)]
mod tfl_severity_tests {
    use super::*;

    /// TfL's own `GET /Line/Meta/Severity` table, transcribed verbatim from
    /// a live fetch on 2026-08-22. The descriptions were identical for
    /// every mode this app ingests (tube, dlr, overground, elizabeth-line,
    /// tram) — checked all five, zero differences — which is why the
    /// mapping is a compile-time table here instead of a per-cycle request
    /// to that endpoint. If TfL extends or renumbers the scale, this test
    /// is what fails.
    const TFL_SEVERITY_TABLE: [(u8, &str); 21] = [
        (0, "Special Service"),
        (1, "Closed"),
        (2, "Suspended"),
        (3, "Part Suspended"),
        (4, "Planned Closure"),
        (5, "Part Closure"),
        (6, "Severe Delays"),
        (7, "Reduced Service"),
        (8, "Bus Service"),
        (9, "Minor Delays"),
        (10, "Good Service"),
        (11, "Part Closed"),
        (12, "Exit Only"),
        (13, "No Step Free Access"),
        (14, "Change of frequency"),
        (15, "Diverted"),
        (16, "Not Running"),
        (17, "Issues Reported"),
        (18, "No Issues"),
        (19, "Information"),
        (20, "Service Closed"),
    ];

    #[test]
    fn every_published_tfl_code_maps_to_a_severity() {
        for (code, description) in TFL_SEVERITY_TABLE {
            assert!(
                severity_from_tfl_code(code).is_some(),
                "TfL code {code} ({description}) has no mapping"
            );
        }
    }

    #[test]
    fn our_wording_matches_tfls_except_two_deliberate_rewordings() {
        for (code, tfl_description) in TFL_SEVERITY_TABLE {
            let ours = severity_from_tfl_code(code).unwrap().description();
            match code {
                // Pre-existing NR wording, unchanged by this feature: the
                // NR feed's equivalent is a rail replacement bus.
                8 => assert_eq!(ours, "Rail Replacement"),
                // Same words, our capitalisation.
                14 => assert_eq!(ours, "Change of Frequency"),
                _ => assert_eq!(ours, tfl_description, "code {code}"),
            }
        }
    }

    #[test]
    fn tfl_codes_above_14_do_not_collide_with_the_nr_extensions() {
        // The whole reason the new variants exist. Our 20 is the NR
        // extension `Recovering` and our 21 is `Diverted`; TfL's 20 is
        // "Service Closed" (which 13 of 20 lines were reporting at the time
        // of capture) and TfL's 15 is its Diverted. Mapping by raw number
        // would have shown "Recovering" all night, every night.
        assert_eq!(severity_from_tfl_code(20), Some(Severity::ServiceClosed));
        assert_ne!(severity_from_tfl_code(20), Some(Severity::Recovering));
        assert_eq!(severity_from_tfl_code(15), Some(Severity::Diverted));
        assert_eq!(Severity::Recovering as u8, 20);
        assert_eq!(Severity::Diverted as u8, 21);
    }

    #[test]
    fn an_unpublished_code_has_no_mapping() {
        // 21 is deliberately included: it is a valid discriminant on OUR
        // scale but not on TfL's, so a naive round-trip would "succeed".
        assert_eq!(severity_from_tfl_code(21), None);
        assert_eq!(severity_from_tfl_code(99), None);
    }

    #[test]
    fn service_closed_is_informational_and_not_running_is_severe() {
        // An overnight closure is the normal state of the Underground and
        // must not paint the network red; a service that is unexpectedly
        // absent must.
        assert_eq!(severity_rank(Severity::ServiceClosed), 1);
        assert_eq!(severity_rank(Severity::NotRunning), 4);
        assert_eq!(severity_rank(Severity::NoIssues), 0);
    }
}
```

In the same file, extend the existing `rank_matches_the_frontends_group_table` table (in `mod severity_rank_tests`) with the five new rows, immediately after `(Severity::Diverted, 4),`:

```rust
            (Severity::ServiceClosed, 1),
            (Severity::NotRunning, 4),
            (Severity::IssuesReported, 3),
            (Severity::NoIssues, 0),
            (Severity::Information, 1),
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p common`
Expected: FAILS TO COMPILE — `no variant named ServiceClosed found for enum Severity`, and `cannot find function severity_from_tfl_code`.

- [ ] **Step 3: Implement in `crates/common/src/lib.rs`**

Add the five variants to `Severity`, immediately after `Diverted = 21,`:

```rust
    /// TfL code 20. The line is shut for the night (or has not started for
    /// the day) — the ordinary overnight state of the Underground, not a
    /// fault. Deliberately NOT discriminant 20: that is already the NR
    /// extension `Recovering`, and renumbering would change the meaning of
    /// every `statusSeverity` already stored in `line_status.statuses` and
    /// rendered by `frontend/lib/severity.ts`.
    ServiceClosed = 22,
    /// TfL code 16. Unlike `ServiceClosed`, this is a service that should
    /// be running and is not.
    NotRunning = 23,
    /// TfL code 17.
    IssuesReported = 24,
    /// TfL code 18. TfL's own "everything is fine" wording for modes that
    /// don't use `Good Service`.
    NoIssues = 25,
    /// TfL code 19, and this crate's landing place for any future TfL code
    /// it has never heard of (see `severity_from_tfl_code`).
    Information = 26,
```

Add their descriptions to `Severity::description`, after the `Self::Diverted => "Diverted",` arm:

```rust
            Self::ServiceClosed => "Service Closed",
            Self::NotRunning => "Not Running",
            Self::IssuesReported => "Issues Reported",
            Self::NoIssues => "No Issues",
            Self::Information => "Information",
```

Extend `severity_rank`'s match arms (it is exhaustive on purpose — the compiler will demand this):

```rust
        Severity::GoodService | Severity::NoIssues => 0,
        Severity::SpecialService
        | Severity::ExitOnly
        | Severity::NoStepFree
        | Severity::ServiceClosed
        | Severity::Information => 1,
        Severity::PlannedClosure | Severity::PartClosure => 2,
        Severity::ReducedService
        | Severity::MinorDelays
        | Severity::ChangeOfFrequency
        | Severity::Recovering
        | Severity::IssuesReported => 3,
        Severity::Closed
        | Severity::Suspended
        | Severity::PartSuspended
        | Severity::SevereDelays
        | Severity::BusService
        | Severity::PartClosed
        | Severity::Diverted
        | Severity::NotRunning => 4,
```

Add the mapping function immediately after `severity_rank`:

```rust
/// Maps a TfL Unified API `statusSeverity` code to this app's `Severity`.
///
/// Codes 0–14 are the same scale in both systems (ours was modelled on
/// TfL's). 15–20 are not: TfL 15 is its own `Diverted` where ours is 21,
/// and TfL 20 is `Service Closed` where our 20 is the NR extension
/// `Recovering` — so a raw numeric passthrough would have mislabelled the
/// ordinary overnight closure of every Underground line as "Recovering".
///
/// `None` means TfL has published a code this table has never seen. Callers
/// must not drop the status (a line with no statuses renders as Good
/// Service) and must not guess a severity: `crates/poller-tfl` records it
/// as `Severity::Information` and carries TfL's own description through in
/// the reason text.
pub fn severity_from_tfl_code(code: u8) -> Option<Severity> {
    Some(match code {
        0 => Severity::SpecialService,
        1 => Severity::Closed,
        2 => Severity::Suspended,
        3 => Severity::PartSuspended,
        4 => Severity::PlannedClosure,
        5 => Severity::PartClosure,
        6 => Severity::SevereDelays,
        7 => Severity::ReducedService,
        8 => Severity::BusService,
        9 => Severity::MinorDelays,
        10 => Severity::GoodService,
        11 => Severity::PartClosed,
        12 => Severity::ExitOnly,
        13 => Severity::NoStepFree,
        14 => Severity::ChangeOfFrequency,
        15 => Severity::Diverted,
        16 => Severity::NotRunning,
        17 => Severity::IssuesReported,
        18 => Severity::NoIssues,
        19 => Severity::Information,
        20 => Severity::ServiceClosed,
        _ => return None,
    })
}

/// The `operators` entry every TfL-sourced line carries. TfL has no
/// per-line ATOC-style operator code the way National Rail does — tube,
/// DLR, Overground, Elizabeth line and tram are all "TfL" — so this is a
/// constant rather than anything derived from the feed.
pub const TFL_OPERATOR: &str = "TfL";

/// Prefix on every TfL line id. `line_status.line_id` is a primary key and
/// TfL's tube line id is `northern`, which is also the id in
/// `lines/northern.toml`; without this prefix the two railways would fight
/// over one row. Applied once, in `crates/poller-tfl`.
pub const TFL_LINE_ID_PREFIX: &str = "tfl-";
```

Add the `DataQuality` variant (the enum is `#[serde(rename_all = "kebab-case")]`, so this serialises as `"tfl"`):

```rust
    /// Published by TfL as line status, not inferred by this app from
    /// incidents or departure boards. The most authoritative quality there
    /// is for a TfL line, and deliberately not folded into
    /// `Knowledgebase` — that name means the National Rail RDM
    /// Knowledgebase feed specifically.
    Tfl,
```

- [ ] **Step 4: Run the Rust tests to verify they pass**

Run: `cargo test -p common`
Expected: PASS — including the pre-existing `rank_matches_the_frontends_group_table` with its five new rows.

- [ ] **Step 5: Write the failing frontend tests**

Add to `frontend/lib/severity.test.ts`:

```typescript
describe('TfL severity codes', () => {
  // The Rust half of this table lives in crates/common/src/lib.rs
  // (`severity_from_tfl_code` + `severity_rank`), and
  // `rank_matches_the_frontends_group_table` there asserts the two agree.
  it('labels the five TfL-only codes', () => {
    expect(severityLabel(22)).toBe('Service Closed');
    expect(severityLabel(23)).toBe('Not Running');
    expect(severityLabel(24)).toBe('Issues Reported');
    expect(severityLabel(25)).toBe('No Issues');
    expect(severityLabel(26)).toBe('Information');
  });

  it('greys out an overnight closure rather than painting it red', () => {
    // Service Closed is the ordinary state of the Underground at 02:00 —
    // 13 of 20 lines were reporting it when this was written. A red
    // network every night is a false alarm, not information.
    expect(severityColor(22)).toBe('gray');
    expect(severityColor(26)).toBe('gray');
  });

  it('keeps an unexpectedly absent service severe, and "no issues" good', () => {
    expect(severityColor(23)).toBe('red');
    expect(severityColor(24)).toBe('yellow');
    expect(severityColor(25)).toBe('green');
  });

  it('does not confuse TfL Service Closed with the NR Recovering extension', () => {
    expect(severityLabel(20)).toBe('Recovering');
    expect(severityLabel(22)).toBe('Service Closed');
  });
});
```

- [ ] **Step 6: Run the frontend test to verify it fails**

Run: `cd frontend && npx vitest run lib/severity.test.ts`
Expected: the 4 new tests FAIL (`severityLabel(22)` returns `'Unknown'`); the pre-existing ones PASS.

- [ ] **Step 7: Implement the frontend half**

In `frontend/lib/severity.ts`, add to `SEVERITY_TABLE` after the `21` row:

```typescript
  // TfL-only codes. Their numbers are this app's own discriminants (see
  // crates/common/src/lib.rs), not TfL's raw statusSeverity: TfL's 20 is
  // "Service Closed" but 20 was already taken by the NR "Recovering"
  // extension, so the poller remaps them on the way in.
  22: { label: 'Service Closed', group: 'informational' },
  23: { label: 'Not Running', group: 'severe' },
  24: { label: 'Issues Reported', group: 'mild' },
  25: { label: 'No Issues', group: 'good' },
  26: { label: 'Information', group: 'informational' },
```

In `frontend/lib/types.ts`, extend the `LineStatus.dataQuality` union:

```typescript
  dataQuality: 'knowledgebase' | 'ldbws-inferred' | 'trust-inferred' | 'planned' | 'tfl';
```

In `frontend/components/IssueList.tsx`, add the label (the `Record<LineStatus['dataQuality'], string>` type makes this mandatory, and the chip row that renders it is generated by `Object.entries(DATA_QUALITY_LABELS)`, so the filter chip appears for free):

```typescript
  planned: 'Planned',
  tfl: 'TfL',
```

- [ ] **Step 8: Run the whole frontend suite and the type-checker**

Run: `cd frontend && npm test && npx tsc --noEmit`
Expected: PASS — the suite, and no type errors from the widened `dataQuality` union.

- [ ] **Step 9: Commit**

```bash
git add crates/common/src/lib.rs frontend/lib/severity.ts frontend/lib/severity.test.ts frontend/lib/types.ts frontend/components/IssueList.tsx
git commit -m "Map TfL severity codes 15-20 onto their own Severity variants"
```

---

## Phase 1 — Storage and ingestion

### Task 2: Give every `line_status` row an owner

**Files:**
- Create: `crates/api/migrations/20260822120000_line_status_source.sql`
- Modify: `crates/aggregator/src/queries.rs`

**Interfaces:**
- Produces: the `line_status.source` column, with values `'aggregator'` and `'tfl'`. Consumed by Task 3 (the TfL upsert and its prune), Task 7 (`/public/lines`), and the freshness query.

Nothing about TfL works until this lands. `crates/aggregator/src/queries.rs::prune_removed_lines` runs `DELETE FROM line_status WHERE NOT (line_id = ANY($1))` on every cycle, and the ids it passes are its own static-catalogue-plus-custom-lines set — so a TfL row would survive at most one aggregation interval (5s under `dev.env`, 60s under `local.env`) before being deleted. The column also gives the TfL ingest a cheap "which rows are mine" filter for its own prune and for the freshness timestamp.

- [ ] **Step 1: Write the failing test**

Add to `crates/aggregator/src/queries.rs`'s `mod tests`:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p aggregator \
                prune_removed_lines_leaves_other_sources_alone -- --ignored`"]
    async fn prune_removed_lines_leaves_other_sources_alone() {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES \
                ('TEST-AGG', 'test aggregator line', 'national-rail', '{}', '[]', 'aggregator'), \
                ('TEST-TFL', 'test tfl line', 'tube', '{TfL}', '[]', 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        // An empty current-line set is the worst case: the aggregator has
        // nothing of its own left, so anything it does not own must still
        // survive.
        prune_removed_lines(&pool, &[]).await.expect("prune_removed_lines");

        let survivors: Vec<String> =
            sqlx::query_scalar("SELECT line_id FROM line_status WHERE line_id IN ('TEST-AGG', 'TEST-TFL')")
                .fetch_all(&pool)
                .await
                .expect("read survivors");

        sqlx::query("DELETE FROM line_status WHERE line_id IN ('TEST-AGG', 'TEST-TFL')")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        assert!(!survivors.contains(&"TEST-AGG".to_string()), "the aggregator's own stale row should go");
        assert!(survivors.contains(&"TEST-TFL".to_string()), "a TfL-owned row must not be collateral damage");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Bring the stack up and export `DATABASE_URL` exactly as Global Constraints describes, then run:

`cargo test -p aggregator prune_removed_lines_leaves_other_sources_alone -- --ignored`
Expected: FAIL — `column "source" of relation "line_status" does not exist` (the seed INSERT).

- [ ] **Step 3: Write the migration**

Create `crates/api/migrations/20260822120000_line_status_source.sql`:

```sql
-- -------------------------------------------------------------------------
-- line_status.source — which service owns this row.
--
-- Until now the aggregator was the only writer of line_status, so "every
-- row is mine" was a safe assumption and prune_removed_lines
-- (crates/aggregator/src/queries.rs) deletes any row whose line_id is not
-- in the aggregator's own static+custom line set. TfL line status arrives
-- already computed — TfL publishes status directly, so there is nothing for
-- the aggregator to infer — and is written straight into this table by the
-- api crate's /private/tfl-line-status ingest. Without an owner column that
-- prune would delete every TfL row on the next aggregation cycle (5s in
-- dev, 60s in prod).
--
--   'aggregator' — crates/aggregator, derived from incidents + LDBWS samples
--   'tfl'        — crates/api's /private/tfl-line-status, fed by
--                  crates/poller-tfl from TfL's Unified API
--
-- Free text rather than an enum type for the same reason mode_name is: a
-- new source is a code change, not a schema migration. The DEFAULT is what
-- back-fills the existing rows correctly — every row that exists when this
-- migration runs was written by the aggregator.
--
-- No index. This table is one row per line (tens of rows, ~20 of them TfL),
-- and every query that filters on source either also filters on the line_id
-- primary key or is a full scan by design; an index here would be cargo
-- cult.
-- -------------------------------------------------------------------------

ALTER TABLE line_status ADD COLUMN source TEXT NOT NULL DEFAULT 'aggregator';
```

- [ ] **Step 4: Scope the aggregator's writes and prune**

In `crates/aggregator/src/queries.rs`, change `prune_removed_lines`'s doc comment and query:

```rust
/// Deletes `line_status` rows for any `line_id` not in `current_line_ids`.
/// Called every cycle with the freshly-merged static+custom line set, so a
/// deleted custom line's last-known status is removed on the next cycle
/// rather than lingering forever (custom lines are the only way a line can
/// disappear between cycles — the static catalogue is fixed for the
/// process's lifetime).
///
/// Scoped to `source = 'aggregator'`: this crate is no longer the only
/// writer of `line_status`. TfL lines are written by the api crate's
/// `/private/tfl-line-status` ingest and are pruned by that endpoint
/// against its own batch — they are invisible to this crate's line set, so
/// an unscoped DELETE here would wipe them on the very next cycle.
pub async fn prune_removed_lines(pool: &PgPool, current_line_ids: &[String]) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM line_status WHERE source = 'aggregator' AND NOT (line_id = ANY($1))",
    )
    .bind(current_line_ids)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

And make `write_line_status`'s upsert claim ownership explicitly, so a row can never drift between owners unnoticed (the `tfl-` id prefix is what actually keeps the two id spaces apart — this is the belt to that pair of braces):

```rust
    sqlx::query(
        r#"
        INSERT INTO line_status (line_id, name, mode_name, operators, statuses, computed_at, source)
        VALUES ($1, $2, $3, $4, $5, NOW(), 'aggregator')
        ON CONFLICT (line_id) DO UPDATE SET
            name        = EXCLUDED.name,
            mode_name   = EXCLUDED.mode_name,
            operators   = EXCLUDED.operators,
            statuses    = EXCLUDED.statuses,
            computed_at = NOW(),
            source      = 'aggregator'
        "#,
    )
```

- [ ] **Step 5: Apply the migration and re-run the test**

```bash
docker compose --env-file dev.env up -d --build api
docker compose --env-file dev.env logs api | grep -i migrat
cargo test -p aggregator prune_removed_lines_leaves_other_sources_alone -- --ignored
```

Expected: the migration is applied on api start-up, and the test PASSES.

- [ ] **Step 6: Check nothing else regressed**

Run: `cargo test -p aggregator`
Expected: PASS (the ignored test is skipped in a plain run).

- [ ] **Step 7: Commit**

```bash
git add crates/api/migrations/20260822120000_line_status_source.sql crates/aggregator/src/queries.rs
git commit -m "Mark who owns each line_status row so the aggregator stops pruning other writers"
```

---

### Task 3: Ingest endpoint for TfL line status

**Files:**
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/ingest.rs`
- Modify: `crates/api/src/routes/freshness.rs`

**Interfaces:**
- Consumes: `line_status.source` (Task 2).
- Produces: `POST /private/tfl-line-status` accepting a JSON array of `common::LineStatusReport` (snake_case field names: `id`, `name`, `mode_name`, `operators`, `statuses`), and `GET /private/tfl-line-status` returning `{"fetchedAt": ...}`. Consumed by Task 4's poller.
- Produces: `queries::upsert_tfl_line_status`, `queries::last_tfl_line_status_fetch`, and a `tfl` field on `routes::freshness::DataFreshness`. The last is consumed by Task 8's frontend type.

The wire type is deliberately the existing `common::LineStatusReport` rather than a new TfL-shaped struct: it already carries exactly `id`/`name`/`mode_name`/`operators`/`statuses`, the poller has to do the TfL→domain mapping anyway (severity codes alone force it — Task 1), and doing it in the poller keeps the api crate free of any knowledge of TfL's JSON. Same split as `poller-incidents`, which maps RDM XML to `IncidentMessage` before posting.

The upsert mirrors `crates/aggregator/src/queries.rs::write_line_status` — upsert `line_status`, append to `line_status_history` only on a real change — with two differences. It needs no `normalize_for_diff` equivalent: TfL statuses carry no `sample_stats`, and their `from_date` comes from the feed (or, when a status has no validity period at all, from the line's own `modified` timestamp — see Task 4), so nothing in the JSON churns cycle to cycle. And it prunes its own batch, because a line that leaves the feed has no other way of disappearing.

- [ ] **Step 1: Write the failing test**

Add to `crates/api/src/data/queries.rs`'s `mod tests`:

```rust
    #[test]
    fn a_line_with_no_stored_row_is_always_changed() {
        assert!(tfl_statuses_changed(None, &serde_json::json!([])));
    }

    #[test]
    fn identical_statuses_are_not_changed() {
        let stored = serde_json::json!([{ "severity": 10, "reason": "Good Service" }]);
        let incoming = serde_json::json!([{ "severity": 10, "reason": "Good Service" }]);
        assert!(!tfl_statuses_changed(Some(&stored), &incoming));
    }

    #[test]
    fn a_new_severity_is_changed() {
        let stored = serde_json::json!([{ "severity": 10, "reason": "Good Service" }]);
        let incoming = serde_json::json!([{ "severity": 6, "reason": "Signal failure at Oxford Circus" }]);
        assert!(tfl_statuses_changed(Some(&stored), &incoming));
    }

    #[test]
    fn a_second_simultaneous_status_is_changed() {
        // TfL routinely reports several statuses on one line at once — a
        // planned closure alongside a live disruption. Gaining or losing
        // one is a change even if the first entry is untouched.
        let stored = serde_json::json!([{ "severity": 4, "reason": "Planned engineering work" }]);
        let incoming = serde_json::json!([
            { "severity": 4, "reason": "Planned engineering work" },
            { "severity": 6, "reason": "Signal failure at Oxford Circus" },
        ]);
        assert!(tfl_statuses_changed(Some(&stored), &incoming));
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p api tfl`
Expected: FAILS TO COMPILE — `cannot find function tfl_statuses_changed in this scope`.

- [ ] **Step 3: Implement the queries**

In `crates/api/src/data/queries.rs`, widen the `common` import:

```rust
use common::{IncidentMessage, LineStatusReport, StationReference, StationSample, TocReference};
```

and add, after `upsert_station_samples`:

```rust
/// Pure diff check, factored out of `upsert_tfl_line_status` so it's
/// testable without a database: a TfL line's statuses are "changed" if the
/// line is new to us, or if the incoming `statuses` JSON differs from what
/// is stored.
///
/// A plain comparison is enough here, unlike the aggregator's
/// `normalize_for_diff` (crates/aggregator/src/queries.rs), which has to
/// strip a freshly-stamped `from_date` and per-cycle `sample_stats` before
/// two unchanged cycles compare equal. Nothing in a TfL status is
/// recomputed by us: the severity, reason and validity period all come
/// from the feed verbatim, and a status with no validity period at all
/// falls back to the line's own `modified` timestamp rather than to
/// `Utc::now()` (see `crates/poller-tfl/src/schema.rs`), precisely so this
/// comparison stays stable across polls. If it ever stops being stable,
/// `line_status_history` grows a row every 300s per line and that is the
/// symptom to look for.
fn tfl_statuses_changed(existing: Option<&serde_json::Value>, incoming: &serde_json::Value) -> bool {
    match existing {
        None => true,
        Some(stored) => stored != incoming,
    }
}

/// Upserts a batch of TfL line-status reports into `line_status` (marked
/// `source = 'tfl'`), appending a `line_status_history` snapshot for each
/// line whose statuses actually changed, and deleting any TfL row missing
/// from this batch.
///
/// The whole batch is one transaction — unlike `upsert_incidents`, which
/// chunks to bound its lock-hold window, this is ~20 rows once every 300s.
///
/// An empty batch is a no-op rather than a mass delete: "TfL returned
/// nothing" is a fault, not an instruction to forget every line. The poller
/// refuses to post one either (belt and braces, since this is the side that
/// would do the damage).
pub async fn upsert_tfl_line_status(pool: &PgPool, reports: &[LineStatusReport]) -> Result<u64> {
    if reports.is_empty() {
        return Ok(0);
    }

    let mut tx = pool.begin().await?;
    let mut count = 0u64;

    for report in reports {
        let statuses_json = serde_json::to_value(&report.statuses)?;

        let existing: Option<serde_json::Value> =
            sqlx::query_scalar("SELECT statuses FROM line_status WHERE line_id = $1 AND source = 'tfl'")
                .bind(&report.id)
                .fetch_optional(&mut *tx)
                .await?;

        sqlx::query(
            r#"
            INSERT INTO line_status (line_id, name, mode_name, operators, statuses, computed_at, source)
            VALUES ($1, $2, $3, $4, $5, NOW(), 'tfl')
            ON CONFLICT (line_id) DO UPDATE SET
                name        = EXCLUDED.name,
                mode_name   = EXCLUDED.mode_name,
                operators   = EXCLUDED.operators,
                statuses    = EXCLUDED.statuses,
                computed_at = NOW(),
                source      = 'tfl'
            "#,
        )
        .bind(&report.id)
        .bind(&report.name)
        .bind(&report.mode_name)
        .bind(&report.operators)
        .bind(&statuses_json)
        .execute(&mut *tx)
        .await?;

        if tfl_statuses_changed(existing.as_ref(), &statuses_json) {
            sqlx::query(
                "INSERT INTO line_status_history (line_id, statuses, computed_at) VALUES ($1, $2, NOW())",
            )
            .bind(&report.id)
            .bind(&statuses_json)
            .execute(&mut *tx)
            .await?;
        }

        count += 1;
    }

    // A TfL line that leaves the feed (a renamed id, a withdrawn service)
    // has no other way of disappearing — `/public/lines` derives its TfL
    // entries from exactly these rows. The aggregator's
    // `prune_removed_lines` is the same idea from the other side of the
    // fence; each writer prunes only what it owns.
    let ids: Vec<&str> = reports.iter().map(|r| r.id.as_str()).collect();
    let pruned = sqlx::query("DELETE FROM line_status WHERE source = 'tfl' AND NOT (line_id = ANY($1))")
        .bind(&ids)
        .execute(&mut *tx)
        .await?
        .rows_affected();
    if pruned > 0 {
        tracing::info!(pruned, "removed TfL lines no longer present in the feed");
    }

    tx.commit().await?;
    Ok(count)
}

/// Timestamp of the most recent TfL line-status ingest, or `None` if none
/// has ever landed. Backs both `GET /private/tfl-line-status` (the poller's
/// startup freshness check) and the public `/public/freshness` endpoint.
pub async fn last_tfl_line_status_fetch(pool: &PgPool) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    let (computed_at,): (Option<chrono::DateTime<chrono::Utc>>,) =
        sqlx::query_as("SELECT MAX(computed_at) FROM line_status WHERE source = 'tfl'")
            .fetch_one(pool)
            .await?;
    Ok(computed_at)
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api`
Expected: PASS — the four new `tfl_statuses_changed` tests and everything already there.

- [ ] **Step 5: Wire up the route**

In `crates/api/src/routes/ingest.rs`, widen the import:

```rust
use common::{IncidentMessage, LineStatusReport, StationReference, StationSample, TocReference};
```

add the route to `router()`, after the `/station-samples` entry:

```rust
        .route(
            "/tfl-line-status",
            axum::routing::get(get_tfl_line_status_last_fetched).post(post_tfl_line_status),
        )
```

and add the two handlers next to their siblings:

```rust
async fn get_tfl_line_status_last_fetched(
    State(app): State<App>,
) -> Result<Json<LastFetchedResponse>, (StatusCode, String)> {
    let fetched_at = queries::last_tfl_line_status_fetch(&app.database)
        .await
        .map_err(internal_error)?;
    Ok(Json(LastFetchedResponse { fetched_at }))
}

/// Unlike the other four ingest routes, this one writes the aggregator's
/// output table directly. That is not a shortcut: TfL publishes finished
/// line status, so there is nothing for the aggregator to infer from
/// incidents or departure boards, and routing it through that crate would
/// mean inventing a second input table for data that is already in its
/// final shape. The two writers stay out of each other's way via
/// `line_status.source` and the `tfl-` line-id prefix.
async fn post_tfl_line_status(
    State(app): State<App>,
    Json(reports): Json<Vec<LineStatusReport>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_tfl_line_status(&app.database, &reports)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

Also extend that file's module doc — the sentence "Each poller POSTs a `Vec<T>` snapshot once per poll cycle; these handlers just deserialize the body ... and hand it to the matching upsert query" is still true, so only the route inventory needs updating; add after the first paragraph:

```rust
//! `/tfl-line-status` is the odd one out: its batch is already-computed
//! line status from TfL rather than raw upstream data, so its upsert
//! targets `line_status`/`line_status_history` directly (see
//! `queries::upsert_tfl_line_status`).
```

- [ ] **Step 6: Add TfL to the public freshness endpoint**

In `crates/api/src/routes/freshness.rs`, update the module doc's first line to say "how fresh the four data sources feeding the status API are (stations reference data, TOC reference data, the raw incidents feed, and the TfL line-status feed)", then:

```rust
#[derive(Debug, Serialize, PartialEq)]
pub struct DataFreshness {
    pub stations: Option<DateTime<Utc>>,
    pub tocs: Option<DateTime<Utc>>,
    pub incidents: Option<DateTime<Utc>>,
    /// When TfL line status last landed. Unlike its three siblings this is
    /// not a poller-fed raw table but the `computed_at` of the TfL-owned
    /// `line_status` rows themselves — for this source, ingest and
    /// computation are the same event.
    pub tfl: Option<DateTime<Utc>>,
}

async fn get_freshness(State(app): State<App>) -> Result<Json<DataFreshness>, (StatusCode, String)> {
    let (stations, tocs, incidents, tfl) = tokio::try_join!(
        queries::last_stations_fetch(&app.database),
        queries::last_tocs_fetch(&app.database),
        queries::last_incidents_fetch(&app.database),
        queries::last_tfl_line_status_fetch(&app.database),
    )
    .map_err(internal_error)?;
    Ok(Json(DataFreshness { stations, tocs, incidents, tfl }))
}
```

and fix its two unit tests, which construct the struct literally:

```rust
    #[test]
    fn serializes_missing_data_as_null() {
        let freshness = DataFreshness { stations: None, tocs: None, incidents: None, tfl: None };
        let json = serde_json::to_value(&freshness).unwrap();
        assert!(json["stations"].is_null());
        assert!(json["tocs"].is_null());
        assert!(json["incidents"].is_null());
        assert!(json["tfl"].is_null());
    }

    #[test]
    fn round_trips_a_present_timestamp() {
        let ts = Utc.with_ymd_and_hms(2026, 7, 15, 9, 0, 0).unwrap();
        let freshness = DataFreshness { stations: Some(ts), tocs: None, incidents: None, tfl: None };
        let json = serde_json::to_value(&freshness).unwrap();
        let roundtripped: DateTime<Utc> = json["stations"].as_str().unwrap().parse().unwrap();
        assert_eq!(roundtripped, ts);
    }
```

- [ ] **Step 7: Run the api tests**

Run: `cargo test -p api`
Expected: PASS.

- [ ] **Step 8: Verify the endpoint against the running stack**

```bash
docker compose --env-file dev.env up -d --build api
set -a; . ./dev.env; set +a

curl -s -X POST http://localhost:8080/private/tfl-line-status \
  -H "x-internal-token: $INTERNAL_TOKEN" -H 'content-type: application/json' \
  -d '[{"id":"tfl-victoria","name":"Victoria","mode_name":"tube","operators":["TfL"],
        "statuses":[{"severity":10,"reason":"Good Service",
                     "validity":{"from_date":"2026-08-22T06:00:00Z","to_date":null,"is_now":true},
                     "data_quality":"tfl"}]}]'
```

Expected: `{"upserted":1}`. Then check the read side and the history behaviour:

```bash
# The status is readable through the existing TfL-shaped endpoint.
curl -s http://localhost:8080/Line/tfl-victoria/Status | head -c 400

# The freshness endpoint now reports it.
curl -s http://localhost:8080/public/freshness

# The poller freshness check answers.
curl -s http://localhost:8080/private/tfl-line-status -H "x-internal-token: $INTERNAL_TOKEN"

# Re-post the SAME body: statuses are unchanged, so no new history row.
# Then post it with severity 6 and a different reason: one new history row.
docker compose --env-file dev.env exec -T postgres \
  psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
  -c "SELECT line_id, computed_at FROM line_status_history WHERE line_id = 'tfl-victoria' ORDER BY computed_at;"
```

Expected: `/Line/tfl-victoria/Status` returns one report with `"statusSeverity": 10` and `"dataQuality": "tfl"`; `/public/freshness` has a non-null `tfl`; the history table gains a row on the first post and on the changed post, but **not** on the identical re-post. Finally, post a batch that omits `tfl-victoria` but contains some other line and confirm the `tfl-victoria` row is pruned, then post an empty array `[]` and confirm nothing is deleted (`{"upserted":0}`).

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/data/queries.rs crates/api/src/routes/ingest.rs crates/api/src/routes/freshness.rs
git commit -m "Add the /private/tfl-line-status ingest endpoint"
```

---

## Phase 2 — The poller

### Task 4: `crates/poller-tfl`

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/poller-tfl/Cargo.toml`
- Create: `crates/poller-tfl/src/main.rs`
- Create: `crates/poller-tfl/src/config.rs`
- Create: `crates/poller-tfl/src/schema.rs`

**Interfaces:**
- Consumes: `common::{severity_from_tfl_code, DataQuality::Tfl, TFL_OPERATOR, TFL_LINE_ID_PREFIX, LineStatusReport, LineStatus, ValidityPeriod, Disruption}` (Task 1), `common::ingest::{post_batch, time_until_next_poll}`, and `POST /private/tfl-line-status` (Task 3).
- Produces: the `poller-tfl` binary. Consumed by Task 5's deployment wiring.

Same three-file shape as the four existing pollers: `config.rs` is a `clap::Parser` with `env` on every field, `schema.rs` owns the upstream wire format and its mapping (pure, fixture-tested), `main.rs` is the poll loop plus one HTTP fetch. Two things differ from the RDM pollers and both are TfL facts, not preferences: the auth header is `Ocp-Apim-Subscription-Key` instead of `x-apikey`, and the fetch retries a 429 or 5xx inside the cycle rather than waiting 300s, because TfL's documented ~500 req/min ceiling is reported to be enforced inconsistently.

The fixture below is a real `GET /Line/Mode/tram/Status` response captured on 2026-08-22, trimmed of the `routeSections`/`serviceTypes`/`crowding` tails. It is worth reading before writing the code: it shows a severity 20 ("Service Closed") in the wild, and it shows `"created": "0001-01-01T00:00:00"` on the status object — a timestamp with **no timezone**, which `chrono`'s serde impl rejects for `DateTime<Utc>`. That field is therefore deliberately not modelled; modelling it would fail the parse of every response TfL sends.

The crate is built in an order the compiler can actually follow: the boilerplate that has no dependency on the mapping (`Cargo.toml`, `config.rs`, `main.rs` and its own retry tests) lands first, then the schema is TDD'd against it. Writing the schema test first in a crate with no binary target would fail on "no targets specified", which is not a useful red.

- [ ] **Step 1: Create the crate skeleton**

Add `"crates/poller-tfl",` to the `members` list in the root `Cargo.toml`, after `"crates/poller-ldbws",`.

Create `crates/poller-tfl/Cargo.toml` (dependency versions copied from `crates/poller-stations/Cargo.toml`, the other JSON-parsing poller, plus `chrono` for the timestamps in TfL's payload):

```toml
[package]
name = "poller-tfl"
version = "0.1.0"
edition = "2024"

[dependencies]
anyhow = "1.0.102"
chrono = { version = "0.4.44", features = ["serde"] }
clap = { version = "4.6.1", features = ["derive", "env"] }
common = { path = "../common" }
dotenv = "0.15.0"
reqwest = { version = "0.13.4", default-features = false, features = ["json", "native-tls", "gzip"] }
serde = { version = "1.0.228", features = ["derive"] }
serde_json = "1.0.149"
tokio = { version = "1.52.3", features = ["rt-multi-thread", "macros"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }
```

- [ ] **Step 2: Write `config.rs`**

Create `crates/poller-tfl/src/config.rs`:

```rust
use clap::Parser;

/// CLI/env configuration for the `poller-tfl` service.
///
/// Unlike the four RDM pollers, `tfl_base_url` HAS a default: TfL's Unified
/// API is a published, stable, documented public endpoint, so there is no
/// "no confirmed endpoint path" gap to fail loudly over. The subscription
/// key still has none — an unset key must stop the process at startup
/// rather than have it poll anonymously and get rate-limited later.
#[derive(Debug, Parser)]
pub struct Config {
    /// TfL Unified API root, without a trailing path. The binary appends
    /// `/Line/Mode/{modes}/Status` itself.
    #[arg(long, env, default_value = "https://api.tfl.gov.uk")]
    pub tfl_base_url: String,

    /// TfL subscription key from api-portal.tfl.gov.uk, sent as the
    /// `Ocp-Apim-Subscription-Key` header (see `main.rs`).
    #[arg(long, env)]
    pub tfl_app_key: String,

    /// Comma-separated TfL modes to poll, passed straight through to TfL's
    /// own comma-separated `{modes}` path segment.
    ///
    /// `bus`, `river-bus`, `cable-car` and friends are deliberately absent
    /// — v1's scope is rail-like TfL modes. `national-rail` is absent for a
    /// different reason: this app already has four National Rail pollers
    /// and an aggregator producing far better status for it than TfL's
    /// summary view.
    #[arg(long, env, default_value = "tube,dlr,overground,elizabeth-line,tram")]
    pub tfl_modes: String,

    /// The `api` crate's ingestion endpoint for TfL line status.
    #[arg(long, env, default_value = "http://api:8080/private/tfl-line-status")]
    pub api_ingest_url: String,

    /// Shared secret sent via `X-Internal-Token` to reach the ingestion
    /// endpoint (see `crates/api/src/auth.rs`).
    #[arg(long, env)]
    pub internal_token: String,

    /// TfL publishes no recommended interval for the line-status endpoint,
    /// and offers no push, no webhook and no confirmed conditional-request
    /// support — polling is the only option. 300s mirrors
    /// `poller-incidents`, whose feed has a comparable update rhythm.
    #[arg(long, env, default_value_t = 300)]
    pub poll_interval_secs: u64,
}
```

- [ ] **Step 3: Write `main.rs`, with its retry tests**

Create `crates/poller-tfl/src/main.rs`:

```rust
//! `poller-tfl`: polls TfL's Unified API for line status across the modes
//! this app displays (tube, DLR, Overground, Elizabeth line, tram) and
//! forwards it to the `api` crate's `/private/tfl-line-status` endpoint.
//!
//! Unlike the four RDM pollers, what this one carries is already finished
//! line status — TfL publishes status directly, so nothing downstream has
//! to infer it from incidents or departure boards, and the aggregator is
//! not involved. `schema.rs` does the whole TfL→domain mapping (severity
//! codes above all) so the `api` crate never sees TfL's JSON.
//!
//! There is no historical endpoint on TfL's side. Everything this app can
//! ever show for "the Victoria line last Tuesday" is what this poller
//! wrote into `line_status_history` at the time.

mod config;
mod schema;

use std::time::Duration;

use chrono::Utc;
use clap::Parser;
use common::ingest;
use config::Config;
use reqwest::{Client, StatusCode};

/// TfL's subscription-key header. Not in `common::ingest` alongside
/// `RDM_AUTH_HEADER_NAME`: that constant is there because four pollers and
/// the api crate all have to agree on it, whereas this one has exactly one
/// consumer.
const TFL_AUTH_HEADER_NAME: &str = "Ocp-Apim-Subscription-Key";

/// Per-request timeout, matching the other pollers: a peer that accepts the
/// connection and never answers would otherwise hang the poll loop forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Attempts per poll cycle before giving up and waiting for the next tick.
/// TfL's registered free tier is documented at roughly 500 requests per
/// minute, but community reports say the enforcement is inconsistent — so
/// this poller does not assume a budget, it just backs off when told to.
const MAX_ATTEMPTS: u32 = 3;

/// Worth retrying inside the cycle: rate limiting and transient upstream
/// faults. A 4xx that is not 429 means this poller is wrong (bad key, bad
/// mode name) and retrying it just burns quota.
fn should_retry(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// 2s, 4s. Both delays plus two requests fit comfortably inside the 300s
/// poll interval, so a retrying cycle can never overlap the next one.
fn retry_delay(attempt: u32) -> Duration {
    Duration::from_secs(2u64.pow(attempt))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let config = Config::parse();
    let client = Client::builder().timeout(REQUEST_TIMEOUT).build()?;

    let poll_interval = Duration::from_secs(config.poll_interval_secs);
    let delay =
        ingest::time_until_next_poll(&client, &config.api_ingest_url, &config.internal_token, poll_interval).await;
    if !delay.is_zero() {
        tracing::info!(delay_secs = delay.as_secs(), "data still fresh from a prior run; delaying first poll");
    }
    let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + delay, poll_interval);

    loop {
        interval.tick().await;

        if let Err(err) = poll_once(&client, &config).await {
            tracing::error!(error = ?err, "poll cycle failed; will retry next interval");
        }
    }
}

async fn poll_once(client: &Client, config: &Config) -> anyhow::Result<()> {
    let body = fetch_status_json(client, config).await?;
    let reports = schema::parse_line_status(&body, Utc::now())?;

    // Never post an empty batch. The ingest endpoint prunes TfL rows that
    // are missing from the batch it receives, so an empty one would read as
    // "TfL has no lines any more" and blank the whole section. The api side
    // guards this too; this is the half that knows it is a fault.
    if reports.is_empty() {
        anyhow::bail!("TfL returned no lines for modes {}; refusing to post an empty batch", config.tfl_modes);
    }

    tracing::info!(count = reports.len(), "parsed line statuses from TfL");

    ingest::post_batch(
        client,
        &config.api_ingest_url,
        &config.internal_token,
        &reports,
        "TfL line statuses",
    )
    .await
}

async fn fetch_status_json(client: &Client, config: &Config) -> anyhow::Result<String> {
    let url = format!(
        "{}/Line/Mode/{}/Status",
        config.tfl_base_url.trim_end_matches('/'),
        config.tfl_modes
    );

    let mut attempt = 0;
    loop {
        let response = client
            .get(&url)
            .header(TFL_AUTH_HEADER_NAME, &config.tfl_app_key)
            .send()
            .await?;
        let status = response.status();

        if status.is_success() {
            return Ok(response.text().await?);
        }

        attempt += 1;
        if attempt >= MAX_ATTEMPTS || !should_retry(status) {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("TfL line-status fetch failed: {status} {body}");
        }

        let delay = retry_delay(attempt);
        tracing::warn!(%status, attempt, delay_secs = delay.as_secs(), "TfL line-status fetch failed; retrying");
        tokio::time::sleep(delay).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limiting_and_upstream_faults_are_retried() {
        assert!(should_retry(StatusCode::TOO_MANY_REQUESTS));
        assert!(should_retry(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(should_retry(StatusCode::BAD_GATEWAY));
    }

    #[test]
    fn our_own_mistakes_are_not_retried() {
        // A bad subscription key or a mode TfL doesn't know is not going to
        // fix itself two seconds later; retrying just spends quota.
        assert!(!should_retry(StatusCode::UNAUTHORIZED));
        assert!(!should_retry(StatusCode::FORBIDDEN));
        assert!(!should_retry(StatusCode::NOT_FOUND));
    }

    #[test]
    fn backoff_fits_inside_one_poll_interval() {
        assert_eq!(retry_delay(1), Duration::from_secs(2));
        assert_eq!(retry_delay(2), Duration::from_secs(4));
        let total: u64 = (1..MAX_ATTEMPTS).map(|attempt| retry_delay(attempt).as_secs()).sum();
        assert!(total < 300, "total backoff {total}s must not overrun the 300s poll interval");
    }
}
```

- [ ] **Step 4: Write the failing schema tests**

Create `crates/poller-tfl/src/schema.rs` containing **only** the test module below (the code it exercises comes in Step 6):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// A real `GET /Line/Mode/tram/Status` response, captured 2026-08-22
    /// and trimmed of its `routeSections`/`serviceTypes`/`crowding` tails.
    ///
    /// Note `"created": "0001-01-01T00:00:00"` on the status object: no
    /// timezone, so it is not modelled — a `DateTime<Utc>` field there
    /// would fail the parse of every response TfL sends. The `$type`
    /// members are TfL's .NET type tags and are ignored the same way.
    const TRAM_STATUS_JSON: &str = r#"[
      {
        "$type": "Tfl.Api.Presentation.Entities.Line, Tfl.Api.Presentation.Entities",
        "id": "tram",
        "name": "Tram",
        "modeName": "tram",
        "disruptions": [],
        "created": "2026-08-17T17:06:09.323Z",
        "modified": "2026-08-17T17:06:09.323Z",
        "lineStatuses": [
          {
            "$type": "Tfl.Api.Presentation.Entities.LineStatus, Tfl.Api.Presentation.Entities",
            "id": 0,
            "lineId": "tram",
            "statusSeverity": 20,
            "statusSeverityDescription": "Service Closed",
            "reason": "London Tramlink: Service will resume later this morning.",
            "created": "0001-01-01T00:00:00",
            "validityPeriods": [
              {
                "$type": "Tfl.Api.Presentation.Entities.ValidityPeriod, Tfl.Api.Presentation.Entities",
                "fromDate": "2026-08-22T01:46:28Z",
                "toDate": "2026-08-22T05:05:09Z",
                "isNow": true
              }
            ],
            "disruption": {
              "$type": "Tfl.Api.Presentation.Entities.Disruption, Tfl.Api.Presentation.Entities",
              "category": "RealTime",
              "categoryDescription": "RealTime",
              "description": "London Tramlink: Service will resume later this morning.",
              "affectedRoutes": [],
              "affectedStops": [],
              "closureText": "serviceClosed"
            }
          }
        ]
      }
    ]"#;

    fn now() -> DateTime<Utc> {
        "2026-08-22T03:00:00Z".parse().unwrap()
    }

    #[test]
    fn parses_a_real_response_and_maps_every_field() {
        let reports = parse_line_status(TRAM_STATUS_JSON, now()).expect("live capture should parse");
        assert_eq!(reports.len(), 1);
        let report = &reports[0];

        // Namespaced, because TfL's tube line id `northern` is also the id
        // of lines/northern.toml and line_status.line_id is a primary key.
        assert_eq!(report.id, "tfl-tram");
        assert_eq!(report.name, "Tram");
        assert_eq!(report.mode_name, "tram");
        assert_eq!(report.operators, vec!["TfL".to_string()]);
        assert_eq!(report.statuses.len(), 1);

        let status = &report.statuses[0];
        // TfL 20 is "Service Closed"; OUR 20 is the NR extension
        // `Recovering`. This assertion is the regression guard for that.
        assert_eq!(status.severity, Severity::ServiceClosed);
        assert_eq!(status.reason, "London Tramlink: Service will resume later this morning.");
        assert_eq!(status.validity.from_date, "2026-08-22T01:46:28Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(status.validity.to_date, Some("2026-08-22T05:05:09Z".parse::<DateTime<Utc>>().unwrap()));
        assert!(status.validity.is_now);
        assert!(matches!(status.data_quality, DataQuality::Tfl));
        assert!(status.sample_stats.is_none());

        let disruption = status.disruption.as_ref().expect("disruption should be carried through");
        assert_eq!(disruption.category, "RealTime");
        assert_eq!(disruption.description, "London Tramlink: Service will resume later this morning.");
        // v1 is line-status only: TfL's affectedStops are Naptan ids, which
        // this app's CHAR(3) CRS columns cannot hold.
        assert!(disruption.affected_stops.is_empty());
        assert!(disruption.affected_routes.is_empty());
        assert_eq!(disruption.source.as_deref(), Some("tfl-line-status-tfl-tram"));
    }

    #[test]
    fn keeps_every_simultaneous_status_on_a_line() {
        // TfL routinely reports a planned closure and a live disruption on
        // one line at the same time. Collapsing to "the worst" here would
        // throw away the other one before the frontend's issue list ever
        // sees it.
        let json = r#"[
          {
            "id": "victoria",
            "name": "Victoria",
            "modeName": "tube",
            "modified": "2026-08-22T02:00:00Z",
            "lineStatuses": [
              {
                "statusSeverity": 4,
                "statusSeverityDescription": "Planned Closure",
                "reason": "No service between Seven Sisters and Walthamstow Central",
                "validityPeriods": [
                  { "fromDate": "2026-08-22T00:00:00Z", "toDate": "2026-08-23T00:00:00Z", "isNow": true }
                ]
              },
              {
                "statusSeverity": 6,
                "statusSeverityDescription": "Severe Delays",
                "reason": "Signal failure at Oxford Circus",
                "validityPeriods": [
                  { "fromDate": "2026-08-22T02:30:00Z", "toDate": null, "isNow": true }
                ]
              }
            ]
          }
        ]"#;

        let reports = parse_line_status(json, now()).expect("should parse");
        assert_eq!(reports[0].id, "tfl-victoria");
        assert_eq!(reports[0].statuses.len(), 2);
        assert_eq!(reports[0].statuses[0].severity, Severity::PlannedClosure);
        assert_eq!(reports[0].statuses[1].severity, Severity::SevereDelays);
    }

    #[test]
    fn an_unknown_severity_code_is_recorded_as_information_not_dropped() {
        // Dropping it would leave the line with zero statuses, which the
        // frontend renders as Good Service — a fault reported as "fine" is
        // the worst possible failure mode here. Guessing a severity for a
        // code we have never seen is the second worst.
        let json = r#"[
          {
            "id": "dlr",
            "name": "DLR",
            "modeName": "dlr",
            "modified": "2026-08-22T02:00:00Z",
            "lineStatuses": [
              { "statusSeverity": 99, "statusSeverityDescription": "Partly Marvellous", "validityPeriods": [] }
            ]
          }
        ]"#;

        let reports = parse_line_status(json, now()).expect("should parse");
        let status = &reports[0].statuses[0];
        assert_eq!(status.severity, Severity::Information);
        assert_eq!(status.reason, "Partly Marvellous");
    }

    #[test]
    fn a_status_with_no_validity_period_falls_back_to_the_lines_modified_time() {
        // NOT to `now`: a fresh timestamp every cycle would make the
        // statuses JSON differ on every poll, and the api's
        // `tfl_statuses_changed` would then append a history row every
        // 300s for a line that never changed.
        let json = r#"[
          {
            "id": "central",
            "name": "Central",
            "modeName": "tube",
            "modified": "2026-08-22T02:00:00Z",
            "lineStatuses": [
              { "statusSeverity": 10, "statusSeverityDescription": "Good Service", "validityPeriods": [] }
            ]
          }
        ]"#;

        let reports = parse_line_status(json, now()).expect("should parse");
        let validity = &reports[0].statuses[0].validity;
        assert_eq!(validity.from_date, "2026-08-22T02:00:00Z".parse::<DateTime<Utc>>().unwrap());
        assert_eq!(validity.to_date, None);
        assert!(validity.is_now);
        // With no `reason`, TfL's own description is the only prose there is.
        assert_eq!(reports[0].statuses[0].reason, "Good Service");
    }

    #[test]
    fn select_validity_prefers_the_period_covering_now_over_one_that_has_ended() {
        let ended = TflValidityPeriod {
            from_date: "2026-08-21T00:00:00Z".parse().unwrap(),
            to_date: Some("2026-08-21T06:00:00Z".parse().unwrap()),
            is_now: false,
        };
        let current = TflValidityPeriod {
            from_date: "2026-08-22T02:00:00Z".parse().unwrap(),
            to_date: Some("2026-08-22T06:00:00Z".parse().unwrap()),
            is_now: false,
        };
        let chosen = select_validity(&[ended, current], now(), now());
        assert_eq!(chosen.from_date, "2026-08-22T02:00:00Z".parse::<DateTime<Utc>>().unwrap());
        // `isNow` was false on the wire but the window contains `now`, so
        // the stored flag says what it means — the same correction the
        // aggregator's `validity_for_output` makes for incidents.
        assert!(chosen.is_now);
    }

    #[test]
    fn select_validity_falls_back_to_the_earliest_future_period() {
        let later = TflValidityPeriod {
            from_date: "2026-09-01T00:00:00Z".parse().unwrap(),
            to_date: None,
            is_now: false,
        };
        let sooner = TflValidityPeriod {
            from_date: "2026-08-25T00:00:00Z".parse().unwrap(),
            to_date: None,
            is_now: false,
        };
        let chosen = select_validity(&[later, sooner], now(), now());
        assert_eq!(chosen.from_date, "2026-08-25T00:00:00Z".parse::<DateTime<Utc>>().unwrap());
        assert!(!chosen.is_now);
    }
}
```

- [ ] **Step 5: Run the tests to verify they fail**

Run: `cargo test -p poller-tfl`
Expected: FAILS TO COMPILE — `cannot find function parse_line_status in module schema` (from `main.rs`), plus `cannot find function parse_line_status`/`cannot find type TflValidityPeriod` in the test module itself.

- [ ] **Step 6: Implement `schema.rs`**

Prepend to `crates/poller-tfl/src/schema.rs`, above the test module:

```rust
//! TfL Unified API `GET /Line/Mode/{modes}/Status` JSON, and its mapping to
//! this app's `common::LineStatusReport`.
//!
//! Field names are transcribed from a live response captured on 2026-08-22
//! (see `TRAM_STATUS_JSON` in the tests), not from documentation. Three
//! facts about that payload drive the shape here:
//!
//! - A line carries a **list** of `lineStatuses`, and several can be live
//!   at once (a planned closure plus a live disruption). All of them are
//!   kept; `common::LineStatus` is already a per-status type and
//!   `line_status.statuses` is already an array.
//! - `LineStatus.created` is serialised as `"0001-01-01T00:00:00"` — no
//!   timezone — which `chrono`'s serde impl will not parse into a
//!   `DateTime<Utc>`. It is deliberately not modelled. The line-level
//!   `created`/`modified` are proper RFC 3339 with a `Z`.
//! - TfL's `statusSeverity` is its own 0–20 scale, which diverges from this
//!   app's `Severity` above 14 (TfL 20 is "Service Closed"; ours is the NR
//!   extension "Recovering"). Every code goes through
//!   `common::severity_from_tfl_code`; nothing is passed through raw.
//!
//! Everything stop-level (`affectedStops`, `affectedRoutes`) is dropped:
//! TfL identifies stops by Naptan id (`940GZZLUABC`) and this app's station
//! columns are `CHAR(3)` CRS codes. That is v1's scope line, not an
//! oversight.

use anyhow::Result;
use chrono::{DateTime, Utc};
use common::{
    DataQuality, Disruption, LineStatus, LineStatusReport, Severity, TFL_LINE_ID_PREFIX, TFL_OPERATOR,
    ValidityPeriod, severity_from_tfl_code,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflLine {
    pub id: String,
    pub name: String,
    pub mode_name: String,
    /// When TfL last touched this line's record. Used as the `from_date`
    /// for a status that carries no validity period of its own — a stable
    /// timestamp, unlike `Utc::now()`, so an unchanged line does not
    /// produce a fresh `line_status_history` row every 300s.
    #[serde(default)]
    pub modified: Option<DateTime<Utc>>,
    #[serde(default)]
    pub line_statuses: Vec<TflLineStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflLineStatus {
    pub status_severity: u8,
    #[serde(default)]
    pub status_severity_description: String,
    /// Absent on a healthy line — TfL sends no prose for Good Service.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub validity_periods: Vec<TflValidityPeriod>,
    #[serde(default)]
    pub disruption: Option<TflDisruption>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflValidityPeriod {
    pub from_date: DateTime<Utc>,
    #[serde(default)]
    pub to_date: Option<DateTime<Utc>>,
    #[serde(default)]
    pub is_now: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TflDisruption {
    /// `"RealTime"` | `"PlannedWork"` | `"Information"` in every observed
    /// response; `Option` only so a missing one cannot fail the whole poll.
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Parses a whole `/Line/Mode/{modes}/Status` response body.
///
/// `now` is a parameter rather than a `Utc::now()` call so the mapping is
/// deterministic under test — the same convention
/// `common::ingest::duration_until_next_poll` and the aggregator's
/// `next_rail_day_boundary` use.
pub fn parse_line_status(json: &str, now: DateTime<Utc>) -> Result<Vec<LineStatusReport>> {
    let lines: Vec<TflLine> = serde_json::from_str(json)?;
    Ok(lines.iter().map(|line| to_report(line, now)).collect())
}

fn to_report(line: &TflLine, now: DateTime<Utc>) -> LineStatusReport {
    let id = format!("{TFL_LINE_ID_PREFIX}{}", line.id);
    let fallback = line.modified.unwrap_or(now);
    LineStatusReport {
        id: id.clone(),
        name: line.name.clone(),
        mode_name: line.mode_name.clone(),
        // Hardcoded, not derived: TfL has no per-line operator code, and
        // every mode in scope is run by the same body.
        operators: vec![TFL_OPERATOR.to_string()],
        statuses: line
            .line_statuses
            .iter()
            .map(|status| map_status(status, now, fallback, &id))
            .collect(),
    }
}

fn map_status(
    status: &TflLineStatus,
    now: DateTime<Utc>,
    fallback: DateTime<Utc>,
    line_id: &str,
) -> LineStatus {
    let severity = severity_from_tfl_code(status.status_severity).unwrap_or_else(|| {
        tracing::warn!(
            line_id,
            code = status.status_severity,
            description = %status.status_severity_description,
            "unknown TfL statusSeverity code; recording it as Information rather than \
             guessing a severity or dropping the status"
        );
        Severity::Information
    });

    LineStatus {
        severity,
        reason: reason_text(status),
        validity: select_validity(&status.validity_periods, now, fallback),
        disruption: status.disruption.as_ref().map(|disruption| Disruption {
            category: disruption.category.clone().unwrap_or_default(),
            description: disruption.description.clone().unwrap_or_default(),
            // Naptan ids and TfL route objects have nowhere to go in a
            // CRS-shaped schema — see this module's doc comment.
            affected_stops: vec![],
            affected_routes: vec![],
            source: Some(format!("tfl-line-status-{line_id}")),
        }),
        data_quality: DataQuality::Tfl,
        // LDBWS-derived delay/cancellation counts. There is no TfL
        // equivalent and v1 does not sample TfL arrivals.
        sample_stats: None,
    }
}

/// TfL omits `reason` entirely on a healthy line, and for a severity code
/// this app has never seen the description is the only human-readable
/// signal there is — so the description is the fallback, and the result is
/// never an empty string.
fn reason_text(status: &TflLineStatus) -> String {
    match status.reason.as_deref().map(str::trim) {
        Some(reason) if !reason.is_empty() => reason.to_string(),
        _ => status.status_severity_description.clone(),
    }
}

fn period_covers_now(period: &TflValidityPeriod, now: DateTime<Utc>) -> bool {
    period.from_date <= now && period.to_date.is_none_or(|to| to >= now)
}

/// Collapses TfL's `validityPeriods[]` to the single `ValidityPeriod` that
/// `common::LineStatus` stores, preferring the period that is actually in
/// force: TfL's own `isNow`, else one whose window contains `now`, else the
/// earliest on record. With no periods at all it synthesises one starting
/// at `fallback` (the line's `modified` timestamp) and open-ended.
///
/// The returned `is_now` is recomputed rather than copied when the dates
/// say otherwise, so a status that is in force cannot arrive at the
/// frontend flagged `isNow: false` — the exact bug that made the National
/// Rail issue list bucket in-progress works as neither Active nor Upcoming.
pub fn select_validity(
    periods: &[TflValidityPeriod],
    now: DateTime<Utc>,
    fallback: DateTime<Utc>,
) -> ValidityPeriod {
    let chosen = periods
        .iter()
        .find(|period| period.is_now)
        .or_else(|| periods.iter().find(|period| period_covers_now(period, now)))
        .or_else(|| periods.iter().min_by_key(|period| period.from_date));

    match chosen {
        Some(period) => ValidityPeriod {
            from_date: period.from_date,
            to_date: period.to_date,
            is_now: period.is_now || period_covers_now(period, now),
        },
        None => ValidityPeriod { from_date: fallback, to_date: None, is_now: true },
    }
}
```

- [ ] **Step 7: Run the whole crate's tests**

Run: `cargo test -p poller-tfl`
Expected: PASS — the seven schema tests and the three retry tests.

- [ ] **Step 8: Verify against the live TfL API and the local stack**

The api must be running with the Task 3 endpoint. A subscription key is not required for a handful of unauthenticated requests, so this can be verified before one is issued — pass any placeholder:

```bash
docker compose --env-file dev.env up -d --build api
set -a; . ./dev.env; set +a

TFL_APP_KEY=unregistered-verification-run \
API_INGEST_URL=http://localhost:8080/private/tfl-line-status \
INTERNAL_TOKEN="$INTERNAL_TOKEN" \
RUST_LOG=info \
  cargo run -p poller-tfl
```

Expected log lines: `parsed line statuses from TfL count=20`, then `posted TfL line statuses to ingestion API count=20`. Then:

```bash
curl -s http://localhost:8080/Line/tfl-northern,tfl-tram,tfl-elizabeth/Status | head -c 600
```

Expected: three reports, ids `tfl-northern`/`tfl-tram`/`tfl-elizabeth`, `"operators": ["TfL"]`, `"dataQuality": "tfl"`, and — depending on the hour — a `"statusSeverity": 22` rendering as `Service Closed`, **never** 20/`Recovering`. Confirm the National Rail `northern` line is untouched: `curl -s http://localhost:8080/Line/northern/Status`.

Leave the poller running for two cycles (10 minutes) and confirm `line_status_history` does **not** grow for lines whose status did not change:

```bash
docker compose --env-file dev.env exec -T postgres psql -U "$POSTGRES_USER" -d "$POSTGRES_DB" \
  -c "SELECT line_id, count(*) FROM line_status_history WHERE line_id LIKE 'tfl-%' GROUP BY line_id ORDER BY 2 DESC LIMIT 5;"
```

Expected: one row per line, not one per poll. More than one for an unchanged line means the `from_date` fallback is churning — re-read `select_validity`.

- [ ] **Step 9: Commit**

```bash
git add Cargo.toml crates/poller-tfl
git commit -m "Add poller-tfl, polling TfL line status for tube, DLR, Overground, Elizabeth line and tram"
```

---

### Task 5: Deploy `poller-tfl` alongside the other five services

**Files:**
- Create: `docker/poller-tfl.Dockerfile`
- Modify: `docker-compose.yml`
- Modify: `docker-compose.dev.yml`
- Modify: `dev.env.example`
- Modify: `local.env.example`
- Modify: `charts/nr-status/values.yaml`
- Modify: `charts/nr-status/templates/poller-deployments.yaml`

**Interfaces:** Consumes `crates/poller-tfl`'s env vars (`TFL_BASE_URL`, `TFL_APP_KEY`, `TFL_MODES`, `API_INGEST_URL`, `INTERNAL_TOKEN`, `POLL_INTERVAL_SECS`). Produces no code interface.

The chart's poller template is a single loop over `.Values.pollers` that never branches on a poller's name — everything that differs lives in the map. TfL breaks that in exactly one place: the template hardcodes `RDM_API_KEY` as the env var the key is injected into, and this binary reads `TFL_APP_KEY`. One `default` on that line keeps the loop generic.

- [ ] **Step 1: Write the Dockerfile**

Create `docker/poller-tfl.Dockerfile` as a copy of `docker/poller-tocs.Dockerfile` with every `poller-tocs` replaced by `poller-tfl` (four occurrences in the build stage's `cargo build`/`cp`, one in the `COPY --from=builder`, one in the `ENTRYPOINT`, plus the header comment's example `docker build -f docker/poller-tfl.Dockerfile .`). Keep `FROM rust:1.86-bookworm` and the three cache mounts (`cargo-registry`, `cargo-git`, `cargo-target-1.86`) exactly as they are — this crate's dependency set is the same as the other pollers', so it belongs in the same 1.86 target cache, and mixing rustc versions in one cache id forces full recompiles.

- [ ] **Step 2: Add the compose service**

In `docker-compose.yml`, after the `poller-ldbws` block:

```yaml
  poller-tfl:
    build:
      context: .
      dockerfile: docker/poller-tfl.Dockerfile
      args:
        CARGO_PROFILE: release
    restart: unless-stopped
    depends_on:
      api:
        condition: service_healthy
    environment:
      # crates/poller-tfl/src/config.rs: Config. No endpoint GAP here,
      # unlike the four RDM pollers — TfL's Unified API is public and
      # documented, so the base URL has a real default.
      TFL_BASE_URL: ${TFL_BASE_URL:-https://api.tfl.gov.uk}
      TFL_APP_KEY: ${TFL_APP_KEY}
      TFL_MODES: ${TFL_MODES:-tube,dlr,overground,elizabeth-line,tram}
      INTERNAL_TOKEN: ${INTERNAL_TOKEN}
      # TfL publishes no recommended cadence and offers no push or
      # conditional requests; 300s mirrors poller-incidents.
      POLL_INTERVAL_SECS: ${POLL_INTERVAL_SECS_TFL:-300}
      RUST_LOG: ${RUST_LOG:-info}
      API_INGEST_URL: http://api:8080/private/tfl-line-status
```

In `docker-compose.dev.yml`, after the `poller-ldbws` block:

```yaml
  poller-tfl:
    build:
      args:
        CARGO_PROFILE: debug
    develop:
      watch:
        - action: rebuild
          path: ./crates/poller-tfl
        - action: rebuild
          path: ./crates/common
        - action: rebuild
          path: ./Cargo.toml
        - action: rebuild
          path: ./Cargo.lock
```

- [ ] **Step 3: Add the env-file entries**

In `local.env.example`, after the `poller-ldbws` section:

```bash
# ---------------------------------------------------------------------------
# poller-tfl (crates/poller-tfl/src/config.rs: Config)
# ---------------------------------------------------------------------------
# No GAP here: TfL's Unified API is public and documented, so the default in
# the binary is the real endpoint. Register at api-portal.tfl.gov.uk for a
# subscription key; the free tier is documented at ~500 requests/min, which
# one request every 300s is nowhere near.
TFL_BASE_URL=https://api.tfl.gov.uk
TFL_APP_KEY=changeme-tfl-subscription-key
# Line-status-only scope: no bus/river-bus/cable-car, and no national-rail
# (this app's own four pollers cover that far better than TfL's summary).
TFL_MODES=tube,dlr,overground,elizabeth-line,tram
# TfL publishes no recommended cadence; 300s mirrors poller-incidents.
POLL_INTERVAL_SECS_TFL=300
```

In `dev.env.example`, the same block but with the last two lines replaced by:

```bash
# TfL publishes no recommended cadence; 300s mirrors poller-incidents.
# Shortened here so the poll loop is actually visible in a dev session —
# one request per cycle is still a rounding error against the documented
# ~500/min. Never carry this value into local.env.
POLL_INTERVAL_SECS_TFL=60
```

- [ ] **Step 4: Add the Helm values and un-hardcode the key's env var**

In `charts/nr-status/templates/poller-deployments.yaml`, change the API-key env entry:

```yaml
            # RDM_API_KEY for the four Rail Data Marketplace pollers; tfl
            # reads TFL_APP_KEY instead, because TfL's key goes in its own
            # `Ocp-Apim-Subscription-Key` header rather than RDM's
            # `x-apikey`. Defaulted so the four existing entries in
            # .Values.pollers need no change.
            - name: {{ $poller.apiKeyEnvVar | default "RDM_API_KEY" }}
```

In `charts/nr-status/values.yaml`, add a `tfl` entry to the `pollers` map (Go templates iterate maps in sorted key order, so it renders between `stations` and `tocs`):

```yaml
  tfl:
    enabled: false
    image:
      repository: nr-status/poller-tfl
      tag: ""
      pullPolicy: IfNotPresent
    # -- TfL Unified API root. Unlike the RDM feeds this is a real, public,
    # documented endpoint, so it has a working default rather than "".
    baseUrl: "https://api.tfl.gov.uk"
    baseUrlEnvVar: TFL_BASE_URL
    # -- tfl only. TfL sends its subscription key in its own header, so the
    # binary reads TFL_APP_KEY where the RDM pollers read RDM_API_KEY.
    apiKeyEnvVar: TFL_APP_KEY
    ingestPath: /private/tfl-line-status
    # -- TfL publishes no recommended cadence for the line-status endpoint,
    # and offers no push or conditional requests. 300s mirrors incidents.
    pollIntervalSecs: 300
    # -- TfL subscription key from api-portal.tfl.gov.uk. Rendered into the
    # chart Secret as `rdm-tfl-api-key` when `existingSecret` is empty — the
    # `rdm-` prefix is the chart's uniform per-poller key naming
    # (nr-status.pollerSecretKey), not a claim that this is an RDM key.
    apiKey: ""
    existingSecret: ""
    existingSecretApiKeyKey: rdm-tfl-api-key
    logLevel: info
    extraEnv:
      - name: TFL_MODES
        value: "tube,dlr,overground,elizabeth-line,tram"
    resources: {}
    nodeSelector: {}
    tolerations: []
    affinity: {}
    podAnnotations: {}
    podSecurityContext: {}
```

- [ ] **Step 5: Verify the chart renders and the four existing pollers are unchanged**

```bash
helm template nr-status charts/nr-status --set pollers.tfl.enabled=true > /tmp/with-tfl.yaml
helm template nr-status charts/nr-status > /tmp/without-tfl.yaml
grep -c 'name: TFL_APP_KEY' /tmp/with-tfl.yaml     # expect 1
grep -c 'name: RDM_API_KEY' /tmp/with-tfl.yaml     # expect 0 (all four RDM pollers default to disabled)
helm lint charts/nr-status
```

Then confirm the defaulting did not disturb an enabled RDM poller:

```bash
helm template nr-status charts/nr-status \
  --set pollers.incidents.enabled=true --set pollers.incidents.baseUrl=https://example.invalid \
  | grep -A1 'name: RDM_API_KEY'
```

Expected: `helm lint` passes, the TfL deployment carries `TFL_APP_KEY` sourced from the `rdm-tfl-api-key` secret key plus a `TFL_MODES` entry, and the incidents deployment still carries `RDM_API_KEY`.

- [ ] **Step 6: Verify the compose service end to end**

```bash
cp dev.env.example dev.env   # if you don't already have one; fill in INTERNAL_TOKEN
docker compose --env-file dev.env up -d --build poller-tfl
docker compose --env-file dev.env logs -f poller-tfl
```

Expected: `parsed line statuses from TfL count=20` then `posted TfL line statuses to ingestion API count=20`, repeating on the interval. Restart the container and confirm it logs `data still fresh from a prior run; delaying first poll` rather than immediately re-fetching.

- [ ] **Step 7: Commit**

```bash
git add docker/poller-tfl.Dockerfile docker-compose.yml docker-compose.dev.yml dev.env.example local.env.example charts/nr-status/values.yaml charts/nr-status/templates/poller-deployments.yaml
git commit -m "Wire poller-tfl into compose, the env templates and the Helm chart"
```

---

## Phase 3 — The read path

### Task 6: Let `/Line/Mode/{modes}/Status` serve more than one mode

**Files:**
- Modify: `crates/api/src/routes/line_status.rs`
- Modify: `crates/api/src/data/queries.rs`

**Interfaces:**
- Produces: `GET /Line/Mode/{modes}/Status` accepting a comma-separated mode list drawn from `national-rail`, `tube`, `dlr`, `overground`, `elizabeth-line`, `tram`. Consumed by Task 8's frontend.
- Produces: `queries::line_status_for_modes(pool, modes: &[String])`, replacing `line_status_for_mode`.

`get_mode_status` currently rejects everything that is not `national-rail` with a 400, so no TfL line is reachable through it — and the frontend's two list pages (`/lines` and the dashboard) are built on exactly this endpoint. Comma-separated modes are not an invention: TfL's own `/Line/Mode/{modes}/Status` takes them, this API exists to mimic that URL scheme, and it is what lets `/lines` fetch every displayed line in one request instead of six.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `crates/api/src/routes/line_status.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_mode_still_works() {
        assert_eq!(parse_modes("national-rail").unwrap(), vec!["national-rail".to_string()]);
    }

    #[test]
    fn every_tfl_mode_this_app_ingests_is_accepted() {
        // The gate used to be `if mode != "national-rail" { 400 }`, which
        // made every ingested TfL line unreachable through this endpoint —
        // and it is the endpoint both frontend list pages are built on.
        let modes = parse_modes("tube,dlr,overground,elizabeth-line,tram").unwrap();
        assert_eq!(modes.len(), 5);
        assert!(modes.contains(&"elizabeth-line".to_string()));
    }

    #[test]
    fn whitespace_and_empty_segments_are_tolerated() {
        assert_eq!(parse_modes("tube, dlr,").unwrap(), vec!["tube".to_string(), "dlr".to_string()]);
    }

    #[test]
    fn an_unsupported_mode_is_named_in_the_error() {
        // `bus` and `river-bus` are real TfL modes this app deliberately
        // does not ingest, so "no results" would be a misleading answer.
        let err = parse_modes("tube,bus").unwrap_err();
        assert!(err.contains("bus"), "error should name the offending mode: {err}");
    }

    #[test]
    fn an_empty_mode_list_is_rejected_rather_than_matching_everything() {
        assert!(parse_modes("").is_err());
        assert!(parse_modes(",,").is_err());
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p api parse_modes`
Expected: FAILS TO COMPILE — `cannot find function parse_modes in this scope`.

- [ ] **Step 3: Implement**

In `crates/api/src/routes/line_status.rs`, add above `get_mode_status`:

```rust
/// Every mode this deployment has data for. `national-rail` is written by
/// the aggregator from Knowledgebase incidents and LDBWS samples; the other
/// five are written by `crates/poller-tfl` via `/private/tfl-line-status`.
///
/// The list is closed rather than "anything in the database" so that a
/// typo, or a real TfL mode this app deliberately does not ingest (`bus`,
/// `river-bus`, `cable-car`), gets a 400 that names the problem instead of
/// an empty array that reads as "no disruption anywhere".
const SUPPORTED_MODES: [&str; 6] = [
    "national-rail",
    "tube",
    "dlr",
    "overground",
    "elizabeth-line",
    "tram",
];

/// Splits and validates TfL's comma-separated `{modes}` path segment.
/// Comma-separated modes are TfL's own contract for this URL — mimicking it
/// is the whole point of these four endpoints — and it lets the frontend
/// fetch every displayed line in one request rather than six.
fn parse_modes(raw: &str) -> Result<Vec<String>, String> {
    let modes: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
        .map(str::to_string)
        .collect();

    if modes.is_empty() {
        return Err("no mode given".to_string());
    }
    if let Some(unsupported) = modes.iter().find(|mode| !SUPPORTED_MODES.contains(&mode.as_str())) {
        return Err(format!("unsupported mode: {unsupported}"));
    }
    Ok(modes)
}
```

and rewrite the handler:

```rust
async fn get_mode_status(
    State(app): State<App>,
    Path(modes): Path<String>,
    Query(query): Query<DetailQuery>,
) -> Result<Json<Vec<Value>>, (StatusCode, String)> {
    let modes = parse_modes(&modes).map_err(|message| (StatusCode::BAD_REQUEST, message))?;

    let rows = queries::line_status_for_modes(&app.database, &modes)
        .await
        .map_err(internal_error)?;

    Ok(Json(rows_to_json(rows, query.detail)))
}
```

In `crates/api/src/data/queries.rs`, replace `line_status_for_mode`:

```rust
/// Every line whose `mode_name` is in `modes`. Plural because TfL's
/// `/Line/Mode/{modes}/Status` takes a comma-separated list and this API
/// mimics its URL scheme — and because the frontend's list pages want
/// National Rail and the five TfL modes in one round trip.
pub async fn line_status_for_modes(pool: &PgPool, modes: &[String]) -> Result<Vec<LineStatusRow>> {
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name, operators, statuses, computed_at FROM line_status WHERE mode_name = ANY($1)",
    )
    .bind(modes)
    .fetch_all(pool)
    .await?;
    rows.into_iter().map(row_to_report).collect()
}
```

Also update the first line of `line_status.rs`'s module doc, which names the route: `//! The four TfL-shaped read endpoints: `/Line/Mode/{modes}/Status`, ...`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p api`
Expected: PASS — five new `parse_modes` tests, and no other caller of the renamed query (`line_status_for_mode` had exactly one, `get_mode_status`; if the build disagrees, fix the caller it names).

- [ ] **Step 5: Verify against the running stack**

```bash
docker compose --env-file dev.env up -d --build api
curl -s 'http://localhost:8080/Line/Mode/national-rail/Status' | python3 -c 'import json,sys; print(len(json.load(sys.stdin)))'
curl -s 'http://localhost:8080/Line/Mode/tube,dlr,overground,elizabeth-line,tram/Status' \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d), sorted({r["modeName"] for r in d}))'
curl -s -o /dev/null -w '%{http_code}\n' 'http://localhost:8080/Line/Mode/bus/Status'
```

Expected: the National Rail count is unchanged from before this task; the TfL query returns 20 reports across the five modes; `bus` returns `400`.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/routes/line_status.rs crates/api/src/data/queries.rs
git commit -m "Accept a comma-separated mode list, including the five TfL modes"
```

---

### Task 7: List TfL lines in `/public/lines`

**Files:**
- Modify: `crates/api/src/data/queries.rs`
- Modify: `crates/api/src/routes/lines.rs`

**Interfaces:**
- Produces: `queries::tfl_line_summaries(pool) -> Vec<TflLineSummaryRow>` and a third group of entries in `GET /public/lines`, each with `"source": "tfl"` and `category` set to the TfL mode name. Consumed by Task 8's frontend types.

`/lines` builds its table from `getAllLines()` and joins the status reports onto it by id, so a line missing from this endpoint never appears no matter how much status data exists for it. `/lines/{id}` also reads its "Category:" line from here.

**Decision — derived from `line_status`, not from `lines/*.toml`.** The full argument is in Global Constraints; the short version is that a TOML entry would be aggregated over (overwriting the ingested status with a Good-Service fallback), would duplicate what the feed already states authoritatively, and would go stale the next time TfL renames a line the way it split London Overground into six in 2024. The rows the poller writes are the same data with none of those problems. The cost is one extra query per `/public/lines` call, on a table with tens of rows.

- [ ] **Step 1: Write the failing test**

Add to `crates/api/src/data/queries.rs`'s `mod tests`:

```rust
    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                tfl_line_summaries_lists_only_tfl_owned_rows -- --ignored`"]
    async fn tfl_line_summaries_lists_only_tfl_owned_rows() {
        use sqlx::postgres::PgPoolOptions;

        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres");

        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) \
             VALUES \
                ('TEST-AGG', 'test aggregator line', 'national-rail', '{NT}', '[]', 'aggregator'), \
                ('TEST-TFL', 'test tfl line', 'tube', '{TfL}', '[]', 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let summaries = tfl_line_summaries(&pool).await.expect("tfl_line_summaries");

        sqlx::query("DELETE FROM line_status WHERE line_id IN ('TEST-AGG', 'TEST-TFL')")
            .execute(&pool)
            .await
            .expect("cleanup fixture rows");

        let ids: Vec<&str> = summaries.iter().map(|row| row.id.as_str()).collect();
        assert!(ids.contains(&"TEST-TFL"), "a TfL-owned row should be listed");
        assert!(!ids.contains(&"TEST-AGG"), "the catalogue already lists aggregator lines");

        let tfl = summaries.iter().find(|row| row.id == "TEST-TFL").unwrap();
        assert_eq!(tfl.mode_name, "tube");
        assert_eq!(tfl.name, "test tfl line");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Export `DATABASE_URL` as per Global Constraints, then run:

`cargo test -p api tfl_line_summaries_lists_only_tfl_owned_rows -- --ignored`
Expected: FAILS TO COMPILE — `cannot find function tfl_line_summaries`.

- [ ] **Step 3: Implement the query**

Add to `crates/api/src/data/queries.rs`, after `line_status_for_modes`:

```rust
/// The identity of one TfL line, for the `/public/lines` catalogue.
pub struct TflLineSummaryRow {
    pub id: String,
    pub name: String,
    pub mode_name: String,
}

/// TfL lines, derived from the rows `crates/poller-tfl` wrote rather than
/// from a hand-curated `lines/*.toml` entry.
///
/// A TOML entry would be wrong three ways: the aggregator loads that
/// directory and would overwrite each ingested TfL status with a
/// Good-Service fallback on its next cycle; a `LineDefinition` is mostly
/// route topology (ordered CRS stations, segments, sample stations,
/// keywords, thresholds) that a finished-status feed has no use for; and it
/// would drift out of date — TfL split "London Overground" into six named
/// lines in 2024. These rows are the feed's own answer, and
/// `upsert_tfl_line_status` prunes the ones that leave it.
pub async fn tfl_line_summaries(pool: &PgPool) -> Result<Vec<TflLineSummaryRow>> {
    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT line_id, name, mode_name FROM line_status WHERE source = 'tfl' ORDER BY name",
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(TflLineSummaryRow {
                id: row.try_get("line_id")?,
                name: row.try_get("name")?,
                mode_name: row.try_get("mode_name")?,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test -p api tfl_line_summaries_lists_only_tfl_owned_rows -- --ignored`
Expected: PASS.

- [ ] **Step 5: Add them to the endpoint**

In `crates/api/src/routes/lines.rs`, extend `list_lines` after the custom-line block:

```rust
    // TfL lines, from the rows crates/poller-tfl wrote — see
    // `queries::tfl_line_summaries` for why they are not catalogue TOML
    // files. `category` carries the TfL mode name (`tube`, `dlr`,
    // `overground`, `elizabeth-line`, `tram`), which is the honest answer
    // to "what kind of line is this" for a network with no `main-line` /
    // `commuter` / `regional` distinction, and is what the line detail
    // page renders as "Category:".
    let tfl = queries::tfl_line_summaries(&app.database)
        .await
        .map_err(internal_error)?;
    out.extend(tfl.into_iter().map(|line| LineSummary {
        id: line.id,
        name: line.name,
        category: line.mode_name,
        operators: vec![common::TFL_OPERATOR.to_string()],
        source: "tfl",
    }));

    Ok(Json(out))
```

and add the query import alongside the existing one:

```rust
use crate::data::{custom_lines::{self, NewCustomLine}, queries};
```

(replacing the current `use crate::data::custom_lines::{self, NewCustomLine};`).

- [ ] **Step 6: Verify against the running stack**

```bash
docker compose --env-file dev.env up -d --build api
curl -s http://localhost:8080/public/lines \
  | python3 -c 'import json,sys; d=json.load(sys.stdin); print(len(d)); print([l for l in d if l["source"]=="tfl"][:3])'
```

Expected: the catalogue lines are still there, plus 20 entries with `"source": "tfl"`, ids prefixed `tfl-`, `"operators": ["TfL"]` and a mode name in `category`. Confirm creating and deleting a custom line still works (`POST`/`DELETE /public/lines`), since this task edits that file.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/queries.rs crates/api/src/routes/lines.rs
git commit -m "List TfL lines in /public/lines, derived from the ingested rows"
```

---

## Phase 4 — Frontend

### Task 8: Show TfL lines on the dashboard and All Lines

**Files:**
- Create: `frontend/lib/modes.ts`
- Create: `frontend/lib/modes.test.ts`
- Modify: `frontend/lib/api.test.ts`
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/app/lines/page.tsx`
- Modify: `frontend/app/page.tsx`
- Modify: `frontend/app/layout.tsx`
- Modify: `frontend/components/DataFreshnessInfo.tsx`
- Modify: `frontend/components/DataFreshnessInfo.test.tsx`

**Interfaces:**
- Consumes: the comma-separated mode list from Task 6, `"source": "tfl"` from Task 7, and `DataFreshness.tfl` from Task 3.
- Produces: `DISPLAYED_MODES` and `DISPLAYED_MODES_PARAM` in `lib/modes.ts`.

Both list pages hardcode `getLineStatusForMode('national-rail')`, so TfL lines would show up in the All Lines table with an empty Status cell and never show up on the dashboard at all, even when pinned. Everything else on the frontend is already generic: `AllLinesTable` derives its operator filter from `line.operators` (so `TfL` appears in it for free), `StatusBadge` reads the severity table Task 1 extended, `IssueList` groups by `dataQuality` (Task 1 added the `TfL` chip), and `/lines/[id]`, `/lines/[id]/history` and `LineStatusCard` are all keyed on line id with no mode logic. Nothing else needs doing — no mode grouping, no icons, no per-mode sections.

- [ ] **Step 1: Write the failing tests**

Create `frontend/lib/modes.test.ts`:

```typescript
import { describe, it, expect } from 'vitest';
import { DISPLAYED_MODES, DISPLAYED_MODES_PARAM } from './modes';

describe('DISPLAYED_MODES', () => {
  it('covers National Rail and the five TfL modes this app ingests', () => {
    expect(DISPLAYED_MODES).toEqual([
      'national-rail',
      'tube',
      'dlr',
      'overground',
      'elizabeth-line',
      'tram',
    ]);
  });

  it('renders as the comma-separated path segment the API expects', () => {
    // Matches SUPPORTED_MODES in crates/api/src/routes/line_status.rs; a
    // mode missing from that list is a 400, not an empty result.
    expect(DISPLAYED_MODES_PARAM).toBe('national-rail,tube,dlr,overground,elizabeth-line,tram');
  });
});
```

Add to `frontend/lib/api.test.ts`, next to the existing `getLineStatusForMode` test:

```typescript
  it('getLineStatusForMode passes a comma-separated mode list through unescaped', async () => {
    await getLineStatusForMode('national-rail,tube,tram');
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/Line/Mode/national-rail,tube,tram/Status',
      expect.objectContaining({ cache: 'no-store' }),
    );
  });
```

In `frontend/components/DataFreshnessInfo.test.tsx`, add `tfl` to the fixture and assert its row:

```typescript
const freshness: DataFreshness = {
  stations: '2026-07-15T09:00:00Z',
  tocs: '2026-07-15T08:00:00Z',
  incidents: null,
  tfl: '2026-08-22T03:00:00Z',
};
```

```typescript
  it('shows a row for the TfL feed', async () => {
    renderWithMantine(<DataFreshnessInfo freshness={freshness} />);
    fireEvent.mouseEnter(screen.getByRole('button', { name: 'Data freshness' }));
    expect(await screen.findByText(/^TfL:/)).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd frontend && npx vitest run lib/modes.test.ts lib/api.test.ts components/DataFreshnessInfo.test.tsx`
Expected: `lib/modes.test.ts` FAILS to resolve (`./modes` does not exist); the new `DataFreshnessInfo` test FAILS (no TfL row); the new `api` test PASSES already (it is a regression guard — `getLineStatusForMode` interpolates its argument, and commas are legal in a path segment, so no encoding change is needed).

- [ ] **Step 3: Implement**

Create `frontend/lib/modes.ts`:

```typescript
/** Every mode whose lines this app displays.
 *
 * Mirrors `SUPPORTED_MODES` in `crates/api/src/routes/line_status.rs` —
 * that list is a closed set, so a mode missing from it comes back as a 400
 * rather than an empty array. `national-rail` is computed by the aggregator
 * from Knowledgebase incidents and LDBWS samples; the other five are
 * published by TfL and ingested wholesale by `crates/poller-tfl`.
 *
 * Kept as one constant rather than a per-page literal because both list
 * pages need the same set, and a page that quietly omits a mode looks
 * exactly like a mode with no disruptions. */
export const DISPLAYED_MODES = [
  'national-rail',
  'tube',
  'dlr',
  'overground',
  'elizabeth-line',
  'tram',
] as const;

/** The value to interpolate into `/Line/Mode/{modes}/Status`. TfL's own API
 * takes a comma-separated list here and this one mimics it, so all six
 * modes are one round trip. */
export const DISPLAYED_MODES_PARAM = DISPLAYED_MODES.join(',');
```

In `frontend/app/lines/page.tsx`:

```tsx
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
```
```tsx
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
```

In `frontend/app/page.tsx`:

```tsx
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
```
```tsx
  // Every displayed mode, not just national-rail: a pinned TfL line would
  // otherwise be silently missing from "Your Lines".
  const allReports = await getLineStatusForMode(DISPLAYED_MODES_PARAM);
```

In `frontend/lib/types.ts`:

```typescript
export interface LineSummary {
  id: string;
  name: string;
  category: string;
  operators: string[];
  source: 'catalogue' | 'custom' | 'tfl';
}
```
```typescript
export interface DataFreshness {
  stations: string | null;
  tocs: string | null;
  incidents: string | null;
  tfl: string | null;
}
```

In `frontend/components/DataFreshnessInfo.tsx`, add the row after Incidents:

```tsx
          {freshnessRow('TfL', freshness.tfl)}
```

and in `frontend/app/layout.tsx`, add the field to the all-null fallback (the layout has no error boundary above it, so this object is what a failed freshness fetch degrades to):

```tsx
  const freshness = await getDataFreshness().catch(() => ({
    stations: null,
    tocs: null,
    incidents: null,
    tfl: null,
  }));
```

- [ ] **Step 4: Run the whole frontend suite and the type-checker**

Run: `cd frontend && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Verify against the running stack**

```bash
docker compose --env-file dev.env up -d --build frontend
```

With `poller-tfl` running, check:
- `http://localhost:3000/lines` — the 20 TfL lines are in the table with a populated Status badge, an em dash in Avg Delay and Cancelled (no LDBWS samples exist for them, which is correct), and `TfL` appears in the operator filter and narrows the table to exactly those 20.
- `http://localhost:3000/lines/tfl-victoria` — name, `Category: tube`, `Operators: TfL`, a working issue list with a `TfL` source badge, no Edit/Delete buttons (it is not a custom line), and no line-definition tooltip (there is no station list — `getLineDefinition` 404s and the page already swallows that).
- `http://localhost:3000/lines/tfl-victoria/history` — renders; it will be sparse or empty until the poller has been running a while, which is expected and is the only history that can exist.
- Pin a TfL line, then reload `http://localhost:3000/` — it appears under "Your Lines".
- The nav's ⓘ tooltip shows a `TfL:` row with a recent timestamp.
- `http://localhost:3000/lines/northern` still shows the National Rail Northern line, unchanged.

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/modes.ts frontend/lib/modes.test.ts frontend/lib/api.test.ts frontend/lib/types.ts frontend/app/lines/page.tsx frontend/app/page.tsx frontend/app/layout.tsx frontend/components/DataFreshnessInfo.tsx frontend/components/DataFreshnessInfo.test.tsx
git commit -m "Show TfL lines on the dashboard and All Lines, and report their freshness"
```

---

### Task 9: "Powered by TfL Open Data"

**Files:**
- Create: `frontend/components/OpenDataAttribution.tsx`
- Create: `frontend/components/OpenDataAttribution.test.tsx`
- Modify: `frontend/app/layout.tsx`

**Interfaces:** None shared.

TfL's data is published under a modified Open Government Licence v2.0 whose attribution clause is not optional: any public-facing use has to carry "Powered by TfL Open Data". A component rather than three lines inline in `layout.tsx` for one reason — this repo tests everything in `components/` and nothing under `app/`, and a licence obligation deserves a test that fails if someone tidies the wording away.

- [ ] **Step 1: Write the failing test**

Create `frontend/components/OpenDataAttribution.test.tsx`:

```typescript
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { OpenDataAttribution } from './OpenDataAttribution';

describe('OpenDataAttribution', () => {
  it('carries TfL\'s required attribution verbatim', () => {
    // Not decoration: TfL's modified OGL v2.0 requires this exact phrase
    // wherever its open data is presented. Reworded, it stops being
    // attribution.
    renderWithMantine(<OpenDataAttribution />);
    expect(screen.getByText('Powered by TfL Open Data')).toBeInTheDocument();
  });

  it('is a landmark, so it is reachable rather than just visible', () => {
    const { container } = renderWithMantine(<OpenDataAttribution />);
    expect(container.querySelector('footer')).not.toBeNull();
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run components/OpenDataAttribution.test.tsx`
Expected: FAIL to resolve — `./OpenDataAttribution` does not exist.

- [ ] **Step 3: Implement**

Create `frontend/components/OpenDataAttribution.tsx`:

```tsx
import { Box, Text } from '@mantine/core';

/** Attribution for the third-party open data this app republishes.
 *
 * TfL publishes its Unified API data under a modified Open Government
 * Licence v2.0 whose attribution clause is a condition of use, not a
 * courtesy: "Powered by TfL Open Data" has to appear wherever the data is
 * presented. The wording is fixed — do not paraphrase it.
 *
 * The licence also asks for Ordnance Survey and Geomni attributions where
 * the data used is derived from theirs. That applies to TfL's *geographic*
 * data — StopPoint coordinates, maps, route geometry — and this app ingests
 * none of it: v1 is line status only (see
 * `docs/superpowers/plans/2026-08-22-tfl-line-status-integration.md`). If
 * stop-level TfL data is ever added, those two lines have to be added here
 * with it.
 *
 * A plain Server Component with no interactivity, rendered once by the root
 * layout so it is on every page. */
export function OpenDataAttribution() {
  return (
    <Box
      component="footer"
      p="md"
      style={{ borderTop: '1px solid var(--mantine-color-default-border)' }}
    >
      <Text size="xs" c="dimmed">
        Powered by TfL Open Data
      </Text>
    </Box>
  );
}
```

In `frontend/app/layout.tsx`, import it:

```tsx
import { OpenDataAttribution } from '@/components/OpenDataAttribution';
```

and render it as the last child of `MantineProvider`, after `{children}`:

```tsx
          {children}
          <OpenDataAttribution />
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend && npx vitest run components/OpenDataAttribution.test.tsx`
Expected: PASS.

- [ ] **Step 5: Run the full suite and the type-checker**

Run: `cd frontend && npm test && npx tsc --noEmit`
Expected: PASS.

- [ ] **Step 6: Verify against the running stack**

```bash
docker compose --env-file dev.env up -d --build frontend
```

Check at 1440px and at 390px that the attribution sits below the content on `/`, `/lines`, `/lines/tfl-victoria` and `/stations`, reads exactly "Powered by TfL Open Data", and does not overlap or push the page into horizontal scroll.

- [ ] **Step 7: Run the e2e suite as a final gate**

Run: `cd frontend && npm run test:e2e`
Expected: PASS, or a clear report of which specs need updating for the new footer and the extra lines in the All Lines table — do not leave a red e2e suite behind.

- [ ] **Step 8: Commit**

```bash
git add frontend/components/OpenDataAttribution.tsx frontend/components/OpenDataAttribution.test.tsx frontend/app/layout.tsx
git commit -m "Add TfL's required open-data attribution"
```

---

## Deliberately not in this plan

Recorded so they are not re-opened mid-execution.

- **Station/stop-level TfL data** — arrivals, per-stop disruptions, `/StopPoint/{naptan}/Disruption`. TfL identifies stops by Naptan id (`940GZZLUABC`); this app's `stations.crs` is `CHAR(3)` and its route is `/stations/[crs]`. Bridging that is a schema and routing change of its own, not a line-status feature.
- **Journey planning.** Never in scope.
- **`bus`, `river-bus`, `cable-car`, `cycle-hire`, `river-tour`, `road`.** Real TfL modes, deliberately not ingested. `parse_modes` rejects them by name so the omission reads as a decision rather than as "no disruptions".
- **`national-rail` via TfL.** TfL publishes it, but this app already produces far better National Rail status from four dedicated pollers, an incident matcher and departure-board sampling. Pulling TfL's summary view would put two writers on the same lines.
- **Fetching TfL history.** There is no such endpoint. `/lines/tfl-*/history` shows what this app recorded, starting the day the poller was deployed.
- **A `/Line/Meta/Severity` fetch in the poll loop.** Verified redundant — the table is identical across all five in-scope modes and every status carries its own description inline. `crates/common`'s `TFL_SEVERITY_TABLE` test is the tripwire if TfL changes the scale.
- **TfL line catalogue TOML files.** See Task 7's Decision.
- **Mode grouping, mode icons, or a TfL-only page.** The existing table, cards, detail page and history page render TfL lines correctly with no mode-specific work; adding chrome for it is a design change, not part of this integration.
- **`LineStatusReport::worst_severity` (`crates/common/src/lib.rs`)** takes the numeric `min()` of the discriminants, which was already inconsistent with `severity_rank` before this feature (it is why `severity_rank` exists) and is only used by aggregator tests. The new 22–26 variants do not make it worse — no TfL line ever mixes them with a National Rail status — and fixing it is a separate change with its own blast radius.

## Self-Review

Run before starting execution; recorded here so a reviewer can check the same things.

**1. Requirement coverage.** Every item in the brief maps to a task: new poller crate + config → Tasks 4/5; ingest endpoint + migration → Tasks 2/3; mode-gate relaxation → Task 6; the "line catalogue entries" slot → Task 7 (derived from `line_status` instead of TOML, with the Decision stated); aggregator/status-writing wiring → Tasks 2/3; attribution → Task 9. The hardcoded `"TfL"` operator is `common::TFL_OPERATOR` (Task 1), applied in Tasks 4 and 7. History-from-our-own-archive needs no new code and is verified in Tasks 3 and 8. The five in-scope modes appear in exactly three places, all of them agreeing: `SUPPORTED_MODES` (Task 6), `TFL_MODES` (Tasks 4/5) and `DISPLAYED_MODES` (Task 8), plus `national-rail` in the first and last.

**2. Placeholder scan.** No "TBD", no "add error handling", no "similar to Task N", no test described but not written. Every code step carries the actual code; every verification step carries the actual command and the expected output. The one place a task says "copy this file and rename" (Task 5's Dockerfile) names the file, the six substitutions and the two lines that must not change.

**3. Type consistency.** `severity_from_tfl_code` (defined Task 1, used Task 4) is `fn(u8) -> Option<Severity>` in both. `TFL_OPERATOR`/`TFL_LINE_ID_PREFIX` (Task 1) are `&str` consts, used in Task 4 via `.to_string()` and Task 7 via `.to_string()`. `upsert_tfl_line_status(&PgPool, &[LineStatusReport]) -> Result<u64>` (Task 3) matches the handler's `Json<Vec<LineStatusReport>>` and the poller's `post_batch(..., &reports, ...)` over `Vec<LineStatusReport>` (Task 4). `line_status_for_mode` → `line_status_for_modes(&PgPool, &[String])` is renamed in Task 6 with its single caller updated in the same task. `tfl_line_summaries` returns `Vec<TflLineSummaryRow>` with `id`/`name`/`mode_name`, which is exactly what Task 7's `list_lines` destructures. `DataFreshness` gains `tfl` on the Rust side (Task 3) and the TypeScript side (Task 8) with the same JSON key. `DataQuality::Tfl` serialises kebab-case as `"tfl"`, matching the TypeScript union member added in Task 1 and the `DATA_QUALITY_LABELS` key in the same task.
