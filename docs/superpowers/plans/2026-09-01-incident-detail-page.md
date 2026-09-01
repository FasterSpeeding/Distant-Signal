# Incident Detail Page Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

## Blockers

**None.** Every prerequisite this plan depends on was directly re-inspected against the live codebase on 2026-09-01 (one day after the design spec was written) and is still true: the `incidents`/`incident_history` table shapes are unchanged, no route or query function for a single incident exists yet, and the exact two frontend render sites (`DisruptionDetail.tsx` embedded once inside `IssueList.tsx`, itself rendered by exactly `/lines/[id]` and `/stations/[crs]`) are unchanged. This plan is ready to implement today. See "Status note" below for the full inspection, including two genuine (non-blocking) corrections to the design spec's own claims that change how specific tasks must be written.

**Goal:** Give a user a standalone `/incidents/[id]` page showing one Knowledgebase-sourced incident's full detail — description, affected stations, every validity period, which lines currently report it, and its own change history — and make it actually reachable: a "View full incident details" link added to `DisruptionDetail.tsx`, the sole place a `Disruption` is rendered anywhere in this frontend today, so both real call sites (`/lines/[id]`, `/stations/[crs]`) get the link automatically. Closes the gap the design spec's own Goal section states: today an incident's detail only ever exists inline, re-rendered fresh (and identically) everywhere it appears, with no page a user can land on or share for one specific incident.

**Architecture:**

```
crates/api/src/data/queries.rs      + IncidentRow, incident_by_id,
                                       IncidentHistoryRow, incident_history_for_id,
                                       IncidentLineRefRow, lines_currently_reporting_incident

crates/api/src/routes/incidents.rs  NEW -- GET /public/incidents/{incidentId},
                                       hand-built camelCase JSON (to_incident_detail_json,
                                       following crates/api/src/render.rs's proven
                                       json!()-macro convention -- see Status note
                                       Correction A for why struct-derive Serialize
                                       is NOT safe to use here)
crates/api/src/routes/mod.rs        + pub mod incidents; .merge(incidents::router())
                                       in public_router()
        │ server-side fetch, no-store, no auth needed
        ▼
frontend/lib/sanitizeHtml.ts        NEW -- sanitizeDescription + DOMPurify hook,
                                       extracted verbatim out of DisruptionDetail.tsx
frontend/lib/incidents.ts           NEW -- incidentIdFromSource
frontend/lib/types.ts               + IncidentDetail, IncidentHistoryEntry, IncidentLineRef
frontend/lib/api.ts                 + getIncident
frontend/components/DisruptionDetail.tsx  MODIFIED -- imports sanitizeDescription
                                       instead of defining it; adds a conditional
                                       "View full incident details" link
frontend/app/incidents/[id]/page.tsx      NEW -- async Server Component,
                                       getIncident(id), notFound() on 404

frontend/components/IssueList.tsx, app/lines/[id]/page.tsx,
app/stations/[crs]/page.tsx         UNCHANGED -- reach the new page only through
                                       DisruptionDetail's new link (verified live
                                       today, see Status note)
```

**Tech Stack:** Rust (axum, sqlx, `PgPool`) for the backend tasks; Next.js App Router + TypeScript + Mantine v9, Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`) for the frontend tasks.

**Spec:** `docs/superpowers/specs/2026-08-31-incident-detail-page-design.md` — read in full before starting; this plan does not restate its research or its "Corrections to the brief" section, only carries its Decisions into concrete tasks. Cross-references below to "Decision N" / "Correction N" refer to that document.

**Status note:** every prerequisite this plan depends on was re-confirmed by direct inspection while writing this plan (2026-09-01, one day after the spec's own 2026-08-31 snapshot). Nothing has drifted in a way that blocks implementation, but two things the spec asserted turned out to need a sharper, more specific instruction than the spec itself gives — recorded here so tasks below can state them as hard requirements rather than leaving an implementer to rediscover them.

- **`incidents` table**: unchanged since the spec's own snapshot — grepped every migration filename after `20260822120000_line_status_source.sql` (`20260828090000_user_accounts.sql`, `20260828100000_add_ownership.sql`, `20260828120000_train_tracking.sql`, `20260829090000_journey_ticket_tracking.sql`) for `incidents`/`incident_history`/`line_status`; the only two hits are comment-only mentions of `incidents.incident_id` as a naming-convention example, not schema changes. The spec's quoted `incidents` column list is still accurate, confirmed directly against `upsert_incidents` in `crates/api/src/data/queries.rs`.
- **`incident_history` table**: confirmed accurate, but only after checking the full migration history, not just the current shape — the *initial* migration (`20260510023522_initial.sql`) actually created it with `severity_hint TEXT`, `valid_from`/`valid_to TIMESTAMPTZ` and **no** `is_cleared` column, which would have been a real blocker if still current. `20260706004003_reference_data.sql` (lines 74-80) drops those three columns and adds `priority INTEGER NOT NULL`, `validity_periods JSONB NOT NULL DEFAULT '[]'`, `is_cleared BOOLEAN NOT NULL DEFAULT FALSE` — the live shape now matches the spec's claim exactly. Recorded here because the spec's own "Current relevant state" section doesn't mention this earlier shape existed at all; a shallower check (reading only the latest migration touching the table) would have missed that the fix already landed.
- **No route or query for a single incident exists**: confirmed — `crates/api/src/routes/` has no `incidents.rs`, `crates/api/src/data/queries.rs` has no `incident_by_id`/`incident_history_for_id`/similar (grepped `incident`: every hit is inside `upsert_incidents`/`incident_changed`/`text_changed`, all internal to the upsert's own diff check). `crates/api/src/routes/mod.rs`'s `public_router()` still only merges `health`, `freshness`, `history_retention`, `lines`, `preferences`, `reference`, `auth` — no `incidents` module.
- **Frontend render-site inventory is unchanged and confirmed live**: `DisruptionDetail` is still embedded exactly once, in `frontend/components/IssueList.tsx` (line 384, `<DisruptionDetail disruption={status.disruption} />` inside `AccordionPanel`, only when `status.disruption` is truthy), and `IssueList` is still rendered by exactly `frontend/app/lines/[id]/page.tsx` and `frontend/app/stations/[crs]/page.tsx`. `frontend/app/page.tsx` (dashboard) still only fetches disruption data for a badge/summary, confirmed by grep — no `<IssueList` or `<DisruptionDetail` there. One new page has appeared since the spec was written, `frontend/app/track/mine/page.tsx` (the tracked-trains list has since shipped) — confirmed by grep it touches neither `Disruption` nor `IssueList` at all, so it doesn't add a third render site. `frontend/app/track/tickets/page.tsx` (a *different*, still-unimplemented plan) does not exist yet either. Net: the spec's "add the link once, inside `DisruptionDetail.tsx`, and both real pages get it for free" claim is still exactly true today.
- **`frontend/lib/incidents.ts` and `frontend/lib/sanitizeHtml.ts` do not exist yet** — confirmed by `ls`; both are genuinely new files, not renames of something already present.
- **Correction A (new finding, not in the spec) — the wire JSON is hand-built, not struct-derived, and this matters for how Task 2 must be written.** The spec's Correction 1/2 correctly describe `common::Disruption`/`AffectedRoute`/`ValidityPeriod` as having **no** `#[serde(rename_all)]`, so their Rust field names are literally `affected_stops`/`from_crs`/`to_crs`/`from_date` etc. What the spec does not say — and what matters for building `to_incident_detail_json` in Task 2 — is *how* the public API nonetheless returns these as camelCase (`affectedStops`, `{"from":..,"to":..}`, `fromDate`) today: **`crates/api/src/render.rs`'s `status_to_json` hand-builds the JSON with `serde_json::json!()`, field by field** (`"affectedRoutes": disruption.affected_routes.iter().map(|r| json!({"from": r.from_crs, "to": r.to_crs}))...`), entirely independent of the structs' own derived `Serialize` impl — confirmed by reading `render.rs` in full, and cross-checked against `frontend/lib/types.ts`'s `Disruption`/`AffectedRoute` interfaces (`affectedStops`, `{from, to}`), which match `render.rs`'s output, not the Rust struct's own field names. **If Task 2 instead defines `#[derive(Serialize)] #[serde(rename_all = "camelCase")]` structs that embed a `Vec<common::ValidityPeriod>` directly**, the outer struct's fields rename correctly but each `ValidityPeriod` *element* would still serialize as `from_date`/`to_date`/`is_now` — `rename_all` is not inherited by a nested type that doesn't also declare it, and `ValidityPeriod` doesn't. This would silently produce a response that is camelCase at the top level and snake_case inside every validity period and affected-route entry, breaking the frontend contract in Decision 2/the API/type contract section. Task 2 below builds the response with `serde_json::json!()` throughout, mirroring `render.rs` exactly, to avoid this.
- **Correction B (new finding, not in the spec) — `formatFullValidity` cannot be imported as-is, and `export const revalidate = 0` is not actually universal.** Two small, concrete corrections to Decision 6's wording:
  - `formatFullValidity` is a **file-private, unexported** function in `IssueList.tsx` (confirmed, no `export` keyword) with signature `(status: LineStatus, now: number) => string` — it cannot be imported by the new page as-is, and its signature doesn't fit a raw `ValidityPeriod` anyway. The new page needs its own small local formatting helper built from the *exported* `formatDateTime` (`frontend/lib/dateFormat.ts`), not an import of `formatFullValidity`. (Decision 6's own phrasing — "extended to render every entry" — already implies new code is needed here; this just makes explicit that it can't start from an import.)
  - `export const revalidate = 0` is **not** present on every dynamic page in this app, contrary to Decision 6's "same rationale as every other dynamic route" phrasing — confirmed by grep, it's present on `/lines/[id]/history/page.tsx`, `/track/mine/page.tsx`, and the dashboard `app/page.tsx`, but **absent** from both `/lines/[id]/page.tsx` and `/stations/[crs]/page.tsx` (the two pages structurally closest to the new one: a dynamic `[id]`-style segment reading one entity via a `no-store` fetch). Both still render dynamically without it. Task 3 below adds it anyway for explicitness/safety (harmless either way, and matches the majority convention), but an implementer should not be surprised if it turns out to be a no-op against this app's actual dynamic-rendering behavior.
- **Backend conventions confirmed for Task 1/2's exact shape**: `internal_error(err: anyhow::Error) -> (StatusCode, String)` is a small helper **duplicated per route file** (confirmed present, separately, in `lines.rs`, `line_status.rs`, `reference.rs`, `samples.rs`, `freshness.rs`, `ingest.rs`, `preferences.rs` — `train.rs`'s curried variant is the one outlier, not the convention to copy here); `incidents.rs` needs its own copy, matching `lines.rs`/`line_status.rs`'s plain form. The DB-integration test convention for a query function needing real Postgres is `#[tokio::test] #[ignore = "requires a live database; ..."]` connecting via `PgPoolOptions` against `std::env::var("DATABASE_URL")`, seeding/cleaning up fixture rows by hand — confirmed via `queries.rs`'s existing `tfl_line_summaries_lists_only_tfl_owned_rows` test, the closest precedent for a query this plan adds.

## Global Constraints

- **No new database migration.** `incidents`/`incident_history`/`line_status` all already have every column and index this feature needs (verified above and in the spec's Correction 1-3). Do not add a migration file, an index, or a column.
- **`GET /public/incidents/{incidentId}` is unauthenticated**, per the spec's "Public read-route convention" finding — every field it returns is already fully public today via `GET /Line/{ids}/Status?detail=true`/`GET /StopPoint/{crs}/Disruption`. Do not gate it behind `AuthenticatedUser`/`OptionalAuthenticatedUser`.
- **Always returns `404` for an unknown id, never a `200` with nulled-out fields** — mirrors `lines.rs::get_line_definition`'s `Option`/`None` → `(StatusCode::NOT_FOUND, "...")` pattern exactly.
- **No special-casing `is_cleared = true`.** A cleared incident is still a fully valid detail page (per Decision 2) — do not add a branch that 404s or hides content for a cleared incident.
- **The response JSON must be hand-built with `serde_json::json!()`, not derived via `#[serde(rename_all = "camelCase")]` on structs that embed `common::ValidityPeriod`/`common::AffectedRoute` directly.** Per Correction A above — this is the one hard technical constraint in this plan with a real, silent-failure-shaped consequence if skipped. Follow `crates/api/src/render.rs::status_to_json`'s pattern for rendering a `ValidityPeriod` as `{"fromDate": ..., "toDate": ..., "isNow": ...}`.
- **`lines_currently_reporting_incident` must query the JSONB `disruption.source` field inside `line_status.statuses` via `jsonb_array_elements` + a path comparison (`s -> 'disruption' ->> 'source' = $1`), never JSONB containment (`@>`) and never the unrelated top-level `line_status.source` column** — per the spec's Correction 2 and Decision 3. Both are real, easy-to-make-by-accident mistakes: `@>` requires full structural element match and would silently never match a real row; `line_status.source` is a same-named-but-unrelated column (`'aggregator' | 'tfl'`, which *service* wrote the row) sitting right next to the JSONB field this query actually needs.
- **`$1` bound to `lines_currently_reporting_incident` is the full reconstructed `knowledgebase-incident-{incidentId}` string, not the bare `incidentId`** — that's the literal value stored in the JSONB, per Decision 3.
- **No new index.** Per Decision 3, this table is tens of rows; a full scan plus unnest is the accepted cost, matching this codebase's own stated rationale for leaving the sibling `line_status.source` column unindexed. Do not add a GIN expression index.
- **`incidentIdFromSource` is the only place in the frontend that parses `Disruption.source`.** Per Decision 4/Correction 1, it must return `null` (not throw, not attempt a partial parse) for `null`, `undefined`, `'ldbws-sampling'`, and any `'tfl-line-status-{lineId}'` value — those are not incident-backed and must never produce a link. Do not duplicate this prefix-check logic anywhere else.
- **The link is added inside `DisruptionDetail.tsx` only** — per Decision 4, do not also add it to `IssueList.tsx` or to either page. The whole point of this placement is that both real call sites inherit it for free with zero changes to either.
- **`sanitizeDescription`'s extraction (Task 3) must not change its behavior.** Byte-for-byte the same `ALLOWED_TAGS`/`ALLOWED_ATTR`/`DOMPurify.addHook` registration, just moved and exported. `DisruptionDetail.test.tsx`'s existing sanitization-behavior assertions move to the new `sanitizeHtml.test.ts` (per the spec's Testing section) rather than being duplicated or dropped.
- **No "browse all incidents" page, no edit/write path on the new route, no NLP-extraction fields (`extracted_*`, `source_text_hash`) anywhere in the response, no retention/pruning job for `incidents`/`incident_history`.** All four are explicitly out of scope per the spec's "Explicitly out of scope" section — no task in this plan may add any of them.
- **`priority`'s display treatment on the page itself is left as an implementation-time call**, per the spec's Open Question 1 — it is still included in the API response (Task 2) since it's a real field with no other place to see it, but Task 4 (the page) is not required to give it a legend or special styling.
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same file; a query function that needs real Postgres gets an `#[ignore]`d `#[tokio::test]` per the Status note's confirmed convention. Frontend: colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`). Every frontend task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures. Every backend task's verification step runs `cargo test -p api` (and, where noted, the `--ignored` DB test if a live Postgres is available) and requires it to pass with no new failures.

---

### Task 1: Backend — `incident_by_id`, `incident_history_for_id`, `lines_currently_reporting_incident` queries

**Files:**
- Modify: `crates/api/src/data/queries.rs`

**Interfaces:**
- Produces: `pub struct IncidentRow`, `pub async fn incident_by_id(pool: &PgPool, incident_id: &str) -> Result<Option<IncidentRow>>`; `pub struct IncidentHistoryRow`, `pub async fn incident_history_for_id(pool: &PgPool, incident_id: &str) -> Result<Vec<IncidentHistoryRow>>`; `pub struct IncidentLineRefRow`, `pub async fn lines_currently_reporting_incident(pool: &PgPool, source: &str) -> Result<Vec<IncidentLineRefRow>>`.
- Consumed by: Task 2 (`crates/api/src/routes/incidents.rs`'s handler).

No dependency on any other task — this is pure data-layer addition to an existing file, following its established style (plain `sqlx::query`/`sqlx::query_as`, matching `line_status_for_ids`/`line_status_history_for_range` in the same file).

- [ ] **Step 1: Add `IncidentRow` and `incident_by_id`**

Place near the file's other row-struct/query pairs (e.g. after `line_status_history_for_range`, at the end of the file — exact placement is not load-bearing).

```rust
/// One row from `incidents`, by primary key. `validity_periods` is kept as
/// raw `serde_json::Value` here (not deserialized into
/// `Vec<common::ValidityPeriod>`) because the route layer needs to
/// re-render each period as camelCase JSON by hand anyway (see
/// `routes/incidents.rs`'s `to_incident_detail_json` and this plan's
/// Global Constraints) -- deserializing into the Rust struct and then
/// re-serializing through `serde_json::json!()` field-by-field would just
/// add a round trip with no benefit.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentRow {
    pub incident_id: String,
    pub summary: String,
    pub description: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    pub priority: i32,
    pub validity_periods: serde_json::Value,
    pub is_planned: bool,
    pub is_cleared: bool,
    pub first_seen_at: chrono::DateTime<chrono::Utc>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
}

/// `incident_id` is this table's primary key (see `upsert_incidents`'s own
/// `INSERT ... ON CONFLICT (incident_id)`), so this is a direct index
/// lookup -- no new index needed. Deliberately does not filter on
/// `is_cleared`: a cleared incident is still a real, fully valid detail
/// page (Decision 2 of the design spec).
pub async fn incident_by_id(pool: &PgPool, incident_id: &str) -> Result<Option<IncidentRow>> {
    let row = sqlx::query_as::<_, IncidentRow>(
        "SELECT incident_id, summary, description, operators, affected_stations, priority, \
                validity_periods, is_planned, is_cleared, first_seen_at, fetched_at \
         FROM incidents WHERE incident_id = $1",
    )
    .bind(incident_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 2: Add `IncidentHistoryRow` and `incident_history_for_id`**

```rust
/// One append-only snapshot from `incident_history`, per the same
/// raw-JSONB rationale as `IncidentRow` above.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentHistoryRow {
    pub summary: String,
    pub description: String,
    pub operators: Vec<String>,
    pub affected_stations: Vec<String>,
    pub priority: i32,
    pub validity_periods: serde_json::Value,
    pub is_planned: bool,
    pub is_cleared: bool,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

/// Newest-first, matching the `incident_history_id_time` index
/// (`(incident_id, recorded_at DESC)`, created in the initial migration)
/// exactly -- no new index needed.
pub async fn incident_history_for_id(pool: &PgPool, incident_id: &str) -> Result<Vec<IncidentHistoryRow>> {
    let rows = sqlx::query_as::<_, IncidentHistoryRow>(
        "SELECT summary, description, operators, affected_stations, priority, validity_periods, \
                is_planned, is_cleared, recorded_at \
         FROM incident_history WHERE incident_id = $1 ORDER BY recorded_at DESC",
    )
    .bind(incident_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 3: Add `IncidentLineRefRow` and `lines_currently_reporting_incident`**

This is the query the Global Constraints section above singles out: `jsonb_array_elements`, not `@>`; the JSONB `disruption.source` path, not the unrelated `line_status.source` column.

```rust
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IncidentLineRefRow {
    pub line_id: String,
    pub name: String,
}

/// Which lines currently carry a status whose `disruption.source` equals
/// `source` exactly (the full `knowledgebase-incident-{id}` string, not
/// the bare id -- that's the literal value stored in the JSONB, see
/// Decision 3 of the design spec). `jsonb_array_elements` unnests
/// `line_status.statuses` (one row per line, one array element per
/// simultaneous status) so each element's `disruption.source` can be
/// compared with a plain path expression. Deliberately NOT JSONB
/// containment (`s @> '{"disruption": {"source": "..."}}'`): Postgres
/// array/object containment requires a full structural match of every
/// key in the compared object, and a real stored status object also
/// carries `severity`/`reason`/`validity`/`dataQuality`, so `@>` would
/// silently match nothing -- see the design spec's Correction 2. Also NOT
/// `line_status.source` (a same-named, unrelated top-level column:
/// `'aggregator' | 'tfl'`, which *service* wrote the row -- added by
/// `20260822120000_line_status_source.sql`). No new index: this table is
/// tens of rows total, matching this repo's own stated rationale for
/// leaving `line_status.source` itself unindexed.
pub async fn lines_currently_reporting_incident(pool: &PgPool, source: &str) -> Result<Vec<IncidentLineRefRow>> {
    let rows = sqlx::query_as::<_, IncidentLineRefRow>(
        "SELECT DISTINCT line_status.line_id, line_status.name \
         FROM line_status, jsonb_array_elements(statuses) AS s \
         WHERE s -> 'disruption' ->> 'source' = $1 \
         ORDER BY line_status.name",
    )
    .bind(source)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
```

- [ ] **Step 4: Add DB-integration tests**

Add a new `#[cfg(test)] mod incident_query_tests` block (colocated, matching this file's convention), mirroring the existing `tfl_line_summaries_lists_only_tfl_owned_rows` test's shape exactly (manual `PgPoolOptions::connect`, seed, run, cleanup, assert):

```rust
#[cfg(test)]
mod incident_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres")
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api incident_by_id -- --ignored`"]
    async fn incident_by_id_finds_a_seeded_row_and_none_for_an_unknown_id() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO incidents (incident_id, summary, description, operators, affected_stations, priority) \
             VALUES ('TEST-INC-1', 'Signal failure', 'Delays expected', '{VT}', '{WOK}', 3) \
             ON CONFLICT (incident_id) DO UPDATE SET summary = EXCLUDED.summary",
        )
        .execute(&pool)
        .await
        .expect("seed fixture row");

        let found = incident_by_id(&pool, "TEST-INC-1").await.expect("query").expect("row should exist");
        assert_eq!(found.summary, "Signal failure");
        assert_eq!(found.affected_stations, vec!["WOK".to_string()]);

        let missing = incident_by_id(&pool, "TEST-INC-DOES-NOT-EXIST").await.expect("query");
        assert!(missing.is_none());

        sqlx::query("DELETE FROM incidents WHERE incident_id = 'TEST-INC-1'").execute(&pool).await.expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api incident_history_for_id -- --ignored`"]
    async fn incident_history_for_id_is_ordered_newest_first_and_empty_for_an_unknown_id() {
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO incident_history (incident_id, summary, description, operators, affected_stations, \
                                             priority, is_planned, recorded_at) \
             VALUES \
                ('TEST-INC-2', 'v1', 'd', '{}', '{}', 1, false, NOW() - INTERVAL '1 hour'), \
                ('TEST-INC-2', 'v2', 'd', '{}', '{}', 2, false, NOW())",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let history = incident_history_for_id(&pool, "TEST-INC-2").await.expect("query");
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].summary, "v2", "newest snapshot should be first");
        assert_eq!(history[1].summary, "v1");

        let empty = incident_history_for_id(&pool, "TEST-INC-DOES-NOT-EXIST").await.expect("query");
        assert!(empty.is_empty());

        sqlx::query("DELETE FROM incident_history WHERE incident_id = 'TEST-INC-2'").execute(&pool).await.expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `cargo test -p api lines_currently_reporting_incident -- --ignored`"]
    async fn lines_currently_reporting_incident_matches_only_the_exact_jsonb_source_string() {
        // The concrete regression test for Correction 2: this must match a
        // real `knowledgebase-incident-*` source and must NOT false-positive
        // against an `ldbws-sampling`/`tfl-line-status-*` row, nor against
        // the unrelated `line_status.source` COLUMN (set to 'tfl' on the
        // second fixture row here, deliberately, to prove the query reaches
        // into the JSONB and not that column).
        let pool = test_pool().await;
        sqlx::query(
            "INSERT INTO line_status (line_id, name, mode_name, operators, statuses, source) VALUES \
                ('TEST-LINE-A', 'Test Line A', 'national-rail', '{VT}', \
                 '[{\"severity\":9,\"reason\":\"x\",\"validity\":{\"from_date\":\"2026-01-01T00:00:00Z\",\"to_date\":null,\"is_now\":true}, \
                    \"data_quality\":\"knowledgebase\",\"disruption\":{\"category\":\"RealTime\",\"description\":\"x\", \
                    \"affected_stops\":[],\"affected_routes\":[],\"source\":\"knowledgebase-incident-TEST-INC-3\"}}]', \
                 'aggregator'), \
                ('TEST-LINE-B', 'Test Line B', 'tube', '{TfL}', \
                 '[{\"severity\":9,\"reason\":\"x\",\"validity\":{\"from_date\":\"2026-01-01T00:00:00Z\",\"to_date\":null,\"is_now\":true}, \
                    \"data_quality\":\"tfl\",\"disruption\":{\"category\":\"RealTime\",\"description\":\"x\", \
                    \"affected_stops\":[],\"affected_routes\":[],\"source\":\"tfl-line-status-TEST-LINE-B\"}}]', \
                 'tfl') \
             ON CONFLICT (line_id) DO UPDATE SET statuses = EXCLUDED.statuses, source = EXCLUDED.source",
        )
        .execute(&pool)
        .await
        .expect("seed fixture rows");

        let matches = lines_currently_reporting_incident(&pool, "knowledgebase-incident-TEST-INC-3")
            .await
            .expect("query");
        let ids: Vec<&str> = matches.iter().map(|r| r.line_id.as_str()).collect();
        assert!(ids.contains(&"TEST-LINE-A"));
        assert!(!ids.contains(&"TEST-LINE-B"), "must not match the unrelated tfl-line-status-* source");

        let no_match = lines_currently_reporting_incident(&pool, "ldbws-sampling").await.expect("query");
        assert!(
            no_match.iter().all(|r| r.line_id != "TEST-LINE-A" && r.line_id != "TEST-LINE-B"),
            "the shared 'ldbws-sampling' literal must never match a real incident lookup"
        );

        sqlx::query("DELETE FROM line_status WHERE line_id IN ('TEST-LINE-A', 'TEST-LINE-B')")
            .execute(&pool)
            .await
            .expect("cleanup");
    }
}
```

- [ ] **Step 5: Compile-check and run the crate's test suite**

Run (from repo root): `cargo check -p api`, then `cargo test -p api`.
Expected: PASS. The three new `#[ignore]`d tests are skipped by default (no local Postgres assumed) — that's expected; if a live `DATABASE_URL` is available, additionally run `cargo test -p api incident -- --ignored` and confirm all three pass.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add incident_by_id, incident_history_for_id, lines_currently_reporting_incident queries"
```

---

### Task 2: Backend — `GET /public/incidents/{incidentId}` route

**Files:**
- Create: `crates/api/src/routes/incidents.rs`
- Modify: `crates/api/src/routes/mod.rs`

**Interfaces:**
- Produces: `pub fn router() -> Router` mounting `GET /incidents/{incidentId}` (becomes `/public/incidents/{incidentId}` once merged into `public_router()`, which nests everything it returns under `/public` in `main.rs`); `to_incident_detail_json(incident: queries::IncidentRow, history: Vec<queries::IncidentHistoryRow>, lines: Vec<queries::IncidentLineRefRow>) -> serde_json::Value` (pure, unit-testable without a database, mirroring `render.rs::to_tfl_shape`'s own testable-without-DB shape).
- Consumes: Task 1's `queries::incident_by_id`/`incident_history_for_id`/`lines_currently_reporting_incident`.

Depends on Task 1 being complete (imports its query functions and row types).

- [ ] **Step 1: Write `crates/api/src/routes/incidents.rs`**

```rust
//! `GET /public/incidents/{incidentId}` -- a single Knowledgebase incident's
//! full detail: description, affected stations, every validity period,
//! which lines currently report it, and its own change history.
//! Unauthenticated, matching every other read in `public_router()` -- see
//! docs/superpowers/specs/2026-08-31-incident-detail-page-design.md's
//! "Public read-route convention" finding: every field this returns is
//! already fully public today via `GET /Line/{ids}/Status?detail=true`.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;

pub fn router() -> Router {
    Router::new().route("/incidents/{incidentId}", axum::routing::get(get_incident))
}

/// `knowledgebase-incident-{incidentId}` is the ONLY provenance-string
/// format that names a real `incidents` row -- see the design spec's
/// Correction 1. Reconstructing it here (rather than storing/returning the
/// bare incident_id as `disruption.source`) is what lets
/// `lines_currently_reporting_incident` reach into `line_status.statuses`'
/// JSONB and find this exact incident.
fn knowledgebase_source(incident_id: &str) -> String {
    format!("knowledgebase-incident-{incident_id}")
}

async fn get_incident(
    State(app): State<App>,
    Path(incident_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let Some(incident) = queries::incident_by_id(&app.database, &incident_id).await.map_err(internal_error)? else {
        return Err((StatusCode::NOT_FOUND, "incident not found".to_string()));
    };
    let history = queries::incident_history_for_id(&app.database, &incident_id).await.map_err(internal_error)?;
    let source = knowledgebase_source(&incident_id);
    let lines = queries::lines_currently_reporting_incident(&app.database, &source).await.map_err(internal_error)?;

    Ok(Json(to_incident_detail_json(incident, history, lines)))
}

/// Renders `serde_json::Value` field-by-field via `json!()`, exactly like
/// `crates/api/src/render.rs::status_to_json` -- deliberately NOT
/// `#[derive(Serialize)] #[serde(rename_all = "camelCase")]` on a struct
/// that embeds `validity_periods` directly, because `rename_all` is not
/// inherited into a nested type. See this plan's Status note Correction A
/// and Global Constraints for the concrete failure mode that would produce
/// (a response that's camelCase at the top level but snake_case inside
/// every validity period). Pure function, no I/O -- unit-testable without
/// a database, matching `to_tfl_shape`'s own testable shape in `render.rs`.
fn to_incident_detail_json(
    incident: queries::IncidentRow,
    history: Vec<queries::IncidentHistoryRow>,
    lines: Vec<queries::IncidentLineRefRow>,
) -> Value {
    json!({
        "incidentId": incident.incident_id,
        "summary": incident.summary,
        "description": incident.description,
        "operators": incident.operators,
        "affectedStations": incident.affected_stations,
        "priority": incident.priority,
        "validityPeriods": render_validity_periods(&incident.validity_periods),
        "isPlanned": incident.is_planned,
        "isCleared": incident.is_cleared,
        "firstSeenAt": incident.first_seen_at.to_rfc3339(),
        "fetchedAt": incident.fetched_at.to_rfc3339(),
        "currentlyAffectsLines": lines.iter().map(|l| json!({
            "id": l.line_id,
            "name": l.name,
        })).collect::<Vec<_>>(),
        "history": history.iter().map(|h| json!({
            "summary": h.summary,
            "description": h.description,
            "operators": h.operators,
            "affectedStations": h.affected_stations,
            "priority": h.priority,
            "validityPeriods": render_validity_periods(&h.validity_periods),
            "isPlanned": h.is_planned,
            "isCleared": h.is_cleared,
            "recordedAt": h.recorded_at.to_rfc3339(),
        })).collect::<Vec<_>>(),
    })
}

/// `validity_periods` comes back from `queries::incident_by_id`/
/// `incident_history_for_id` as raw `serde_json::Value` (the column's own
/// stored JSONB, snake-case field names -- `from_date`/`to_date`/`is_now`,
/// since `common::ValidityPeriod` has no `rename_all`). Deserializes into
/// the real Rust type first so a malformed row fails loudly via
/// `unwrap_or_default` -> empty array, rather than this function
/// re-implementing JSONB field access by hand.
fn render_validity_periods(raw: &Value) -> Value {
    let periods: Vec<common::ValidityPeriod> = serde_json::from_value(raw.clone()).unwrap_or_default();
    Value::Array(
        periods
            .into_iter()
            .map(|p| {
                json!({
                    "fromDate": p.from_date.to_rfc3339(),
                    "toDate": p.to_date.map(|d| d.to_rfc3339()),
                    "isNow": p.is_now,
                })
            })
            .collect(),
    )
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!("incident lookup failed: {err:#}");
    (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
}
```

**Implementation-time verification note:** confirm the exact `internal_error` body (logging call, message text) against another file in this directory (e.g. `lines.rs`) and match it precisely rather than inventing new wording — the snippet above is illustrative of the shape, not necessarily byte-identical to what's already there.

- [ ] **Step 2: Mount the router in `crates/api/src/routes/mod.rs`**

Add `pub mod incidents;` to the module list (alphabetically, after `history_retention`, before `ingest`) and `.merge(incidents::router())` inside `public_router()` (after `.merge(history_retention::router())`, before `.merge(lines::router())` — exact position among the merges is not load-bearing, none of the existing paths start with `/incidents`, so there is no conflict to worry about):

```rust
pub mod incidents;
```

```rust
    Router::new()
        .merge(health::router())
        .merge(freshness::router())
        .merge(history_retention::router())
        .merge(incidents::router())
        .merge(lines::router())
        .merge(preferences::router())
        .merge(reference::router())
        .merge(auth::router())
```

- [ ] **Step 3: Add unit tests for `to_incident_detail_json`**

Add a `#[cfg(test)] mod tests` block to `incidents.rs`, no database required (mirrors `render.rs`'s own DB-free test module):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn sample_incident() -> queries::IncidentRow {
        queries::IncidentRow {
            incident_id: "12345".to_string(),
            summary: "Signal failure at Woking".to_string(),
            description: "<p>Delays expected</p>".to_string(),
            operators: vec!["VT".to_string()],
            affected_stations: vec!["WOK".to_string(), "WAT".to_string()],
            priority: 3,
            validity_periods: serde_json::json!([
                {"from_date": "2026-08-30T09:00:00Z", "to_date": null, "is_now": true}
            ]),
            is_planned: false,
            is_cleared: false,
            first_seen_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            fetched_at: Utc.with_ymd_and_hms(2026, 8, 31, 10, 15, 0).unwrap(),
        }
    }

    #[test]
    fn renders_top_level_fields_as_camel_case() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        assert_eq!(json["incidentId"], "12345");
        assert_eq!(json["summary"], "Signal failure at Woking");
        assert_eq!(json["affectedStations"][0], "WOK");
        assert_eq!(json["isPlanned"], false);
        assert_eq!(json["isCleared"], false);
    }

    #[test]
    fn validity_periods_render_as_camel_case_not_snake_case() {
        // The direct regression test for Correction A -- proves this
        // function does not fall back to a derived Serialize impl that
        // would leak `from_date`/`to_date`/`is_now` through unrenamed.
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        let period = &json["validityPeriods"][0];
        assert_eq!(period["fromDate"], "2026-08-30T09:00:00+00:00");
        assert!(period["toDate"].is_null());
        assert_eq!(period["isNow"], true);
        assert!(period.get("from_date").is_none(), "must not leak the raw snake_case JSONB field name");
    }

    #[test]
    fn currently_affects_lines_is_empty_array_not_null_when_no_lines_match() {
        let json = to_incident_detail_json(sample_incident(), vec![], vec![]);
        assert!(json["currentlyAffectsLines"].is_array());
        assert_eq!(json["currentlyAffectsLines"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn history_renders_every_entry_newest_first_order_preserved() {
        let history = vec![
            queries::IncidentHistoryRow {
                summary: "v2".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 2,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 31, 9, 0, 0).unwrap(),
            },
            queries::IncidentHistoryRow {
                summary: "v1".to_string(),
                description: "d".to_string(),
                operators: vec![],
                affected_stations: vec![],
                priority: 1,
                validity_periods: serde_json::json!([]),
                is_planned: false,
                is_cleared: false,
                recorded_at: Utc.with_ymd_and_hms(2026, 8, 30, 9, 0, 0).unwrap(),
            },
        ];
        let json = to_incident_detail_json(sample_incident(), history, vec![]);
        assert_eq!(json["history"][0]["summary"], "v2");
        assert_eq!(json["history"][1]["summary"], "v1");
    }

    #[test]
    fn currently_affects_lines_renders_id_and_name() {
        let lines = vec![queries::IncidentLineRefRow { line_id: "south-western".to_string(), name: "South Western Main Line".to_string() }];
        let json = to_incident_detail_json(sample_incident(), vec![], lines);
        assert_eq!(json["currentlyAffectsLines"][0]["id"], "south-western");
        assert_eq!(json["currentlyAffectsLines"][0]["name"], "South Western Main Line");
    }

    #[test]
    fn knowledgebase_source_matches_the_exact_format_correction_1_verified() {
        assert_eq!(knowledgebase_source("12345"), "knowledgebase-incident-12345");
    }
}
```

- [ ] **Step 4: Run the crate's test suite and the full workspace build**

Run (from repo root): `cargo test -p api`, then `cargo build --workspace`.
Expected: both PASS, no new warnings. A route-table conflict (there shouldn't be one — no existing public route starts with `/incidents`) would surface as an axum panic the first time `public_router()` is actually constructed, i.e. at `cargo run`/server startup or in any test that boots the app; there is no existing test that constructs `public_router()` directly (confirmed — this is a pre-existing gap in this codebase's own test coverage, not something this task needs to newly solve), so a manual `cargo run -p api` + `curl localhost:.../public/health` sanity check is a reasonable extra confidence check here if time allows, though not required to consider this task done.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/incidents.rs crates/api/src/routes/mod.rs
git commit -m "Add GET /public/incidents/{incidentId}"
```

---

### Task 3: Frontend — extract `sanitizeDescription` into `frontend/lib/sanitizeHtml.ts`

**Files:**
- Create: `frontend/lib/sanitizeHtml.ts`
- Create: `frontend/lib/sanitizeHtml.test.ts`
- Modify: `frontend/components/DisruptionDetail.tsx`
- Modify: `frontend/components/DisruptionDetail.test.tsx`

**Interfaces:**
- Produces: `export function sanitizeDescription(html: string): string`.
- Consumed by: `DisruptionDetail.tsx` (this task), Task 4's new `app/incidents/[id]/page.tsx`.

Per Decision 5 — a pure move, no behavior change. No dependency on any other task; this can be done first or in parallel with backend work.

- [ ] **Step 1: Create `frontend/lib/sanitizeHtml.ts`**

Move verbatim out of `DisruptionDetail.tsx`: the `DOMPurify.addHook('afterSanitizeAttributes', ...)` registration, `ALLOWED_TAGS`, `ALLOWED_ATTR`, and the `sanitizeDescription` function body — byte-identical, just relocated and exported. ES modules are singletons, so the hook still registers exactly once regardless of how many modules import `sanitizeDescription`.

```ts
import DOMPurify from 'isomorphic-dompurify';

// Registered once at module load. `disruption.description` comes from the
// Darwin/Knowledgebase feed already fully HTML-entity-decoded by the time
// it reaches the frontend (see poller-incidents' quick_xml parsing) — it's
// real markup, not escaped/serialized XML needing re-parsing. DOMPurify's
// ALLOWED_ATTR strips `target`/`rel` by default since they're not in the
// allowlist below; this hook adds them back on every surviving `<a>` so
// external links don't inherit this page's window/referrer.
DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank');
    node.setAttribute('rel', 'noopener');
  }
});

const ALLOWED_TAGS = ['p', 'br', 'strong', 'b', 'em', 'i', 'ul', 'ol', 'li', 'a'];
const ALLOWED_ATTR = ['href'];

/** The single sanitizer for every incident/disruption description this app
 * renders as HTML — shared by `DisruptionDetail.tsx` (a line's/station's
 * inline issue list) and `app/incidents/[id]/page.tsx` (the incident's own
 * detail page), so both apply the exact same allowlist and the same
 * forced `target="_blank" rel="noopener"` link hardening. Extracted out of
 * `DisruptionDetail.tsx`, where this previously lived file-local — see
 * docs/superpowers/specs/2026-08-31-incident-detail-page-design.md
 * Decision 5. */
export function sanitizeDescription(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR });
}
```

- [ ] **Step 2: Update `DisruptionDetail.tsx` to import instead of define**

Remove the `DOMPurify` import, the `DOMPurify.addHook(...)` call, `ALLOWED_TAGS`, `ALLOWED_ATTR`, and the local `sanitizeDescription` function from `frontend/components/DisruptionDetail.tsx`; add:

```tsx
import { sanitizeDescription } from '@/lib/sanitizeHtml';
```

The existing `dangerouslySetInnerHTML={{ __html: sanitizeDescription(disruption.description) }}` call site needs no change — same function name, same signature.

- [ ] **Step 3: Move the sanitization-behavior tests out of `DisruptionDetail.test.tsx`**

Create `frontend/lib/sanitizeHtml.test.ts` containing the three tests that test `sanitizeDescription`'s own behavior, not anything `DisruptionDetail`-specific — move (don't duplicate) `'renders safe HTML tags as actual elements, not escaped text'`, `'strips script tags and event handler attributes'`, `'forces target=_blank and rel=noopener on links'` out of `DisruptionDetail.test.tsx`, rewritten to call `sanitizeDescription` directly rather than through a rendered component:

```ts
import { describe, it, expect } from 'vitest';
import { sanitizeDescription } from './sanitizeHtml';

describe('sanitizeDescription', () => {
  it('keeps safe HTML tags intact', () => {
    const result = sanitizeDescription('<p>Signal failure</p><br/><strong>at Woking</strong>');
    expect(result).toContain('<p>Signal failure</p>');
    expect(result).toContain('<strong>at Woking</strong>');
  });

  it('strips script tags and event handler attributes', () => {
    const result = sanitizeDescription('<p onclick="alert(1)">Safe text</p><script>alert(2)</script>');
    expect(result).not.toContain('<script>');
    expect(result).not.toContain('onclick');
    expect(result).toContain('Safe text');
  });

  it('forces target=_blank and rel=noopener on links', () => {
    const result = sanitizeDescription('<a href="https://example.com">More info</a>');
    expect(result).toContain('target="_blank"');
    expect(result).toContain('rel="noopener"');
  });
});
```

`DisruptionDetail.test.tsx` keeps its own remaining tests (description renders, affected stops render, affected route renders, source line renders/omits) — those genuinely test `DisruptionDetail`'s own rendering, not the sanitizer.

- [ ] **Step 4: Run the frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`.
Expected: both PASS. `DisruptionDetail.test.tsx`'s remaining tests must still pass unmodified in substance (only the three sanitizer-behavior tests moved out) — this is the direct check that the extraction changed nothing observable.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/sanitizeHtml.ts frontend/lib/sanitizeHtml.test.ts frontend/components/DisruptionDetail.tsx frontend/components/DisruptionDetail.test.tsx
git commit -m "Extract sanitizeDescription out of DisruptionDetail.tsx into lib/sanitizeHtml.ts"
```

---

### Task 4: Frontend — `incidentIdFromSource` and the "View full incident details" link

**Files:**
- Create: `frontend/lib/incidents.ts`
- Create: `frontend/lib/incidents.test.ts`
- Modify: `frontend/components/DisruptionDetail.tsx`
- Modify: `frontend/components/DisruptionDetail.test.tsx`
- Modify: `frontend/components/IssueList.test.tsx`

**Interfaces:**
- Produces: `export function incidentIdFromSource(source: string | null | undefined): string | null`.
- Consumed by: `DisruptionDetail.tsx` (this task).

This task is **the explicit "wiring" requirement** — per Decision 4, adding the link once inside `DisruptionDetail.tsx` is what reaches both real render sites (`/lines/[id]`, `/stations/[crs]`, both via `IssueList.tsx`, both confirmed still live and unchanged in the Status note above) with zero changes to `IssueList.tsx` or either page. No dependency on Task 3 at the import level, but both modify `DisruptionDetail.tsx`, so doing Task 3 first (as ordered here) avoids a self-inflicted merge conflict.

- [ ] **Step 1: Create `frontend/lib/incidents.ts`**

```ts
const KNOWLEDGEBASE_INCIDENT_PREFIX = 'knowledgebase-incident-';

/** The only place in this frontend that "parses" `Disruption.source` — see
 * docs/superpowers/specs/2026-08-31-incident-detail-page-design.md
 * Correction 1 for why this exact prefix, and why the LDBWS
 * ('ldbws-sampling', a shared literal constant, not an id) and TfL
 * ('tfl-line-status-{lineId}', keyed off a line id, not an incident id)
 * source values must NOT resolve to a link — neither names a real
 * `incidents` row, so there is nothing for `/incidents/[id]` to show for
 * either. */
export function incidentIdFromSource(source: string | null | undefined): string | null {
  if (!source || !source.startsWith(KNOWLEDGEBASE_INCIDENT_PREFIX)) return null;
  return source.slice(KNOWLEDGEBASE_INCIDENT_PREFIX.length);
}
```

- [ ] **Step 2: Add tests**

Create `frontend/lib/incidents.test.ts`:

```ts
import { describe, it, expect } from 'vitest';
import { incidentIdFromSource } from './incidents';

describe('incidentIdFromSource', () => {
  it('strips the known prefix and returns the raw incident id', () => {
    expect(incidentIdFromSource('knowledgebase-incident-12345')).toBe('12345');
  });

  it('returns null for null', () => {
    expect(incidentIdFromSource(null)).toBeNull();
  });

  it('returns null for undefined', () => {
    expect(incidentIdFromSource(undefined)).toBeNull();
  });

  it('returns null for the shared LDBWS-inferred literal constant', () => {
    expect(incidentIdFromSource('ldbws-sampling')).toBeNull();
  });

  it('returns null for a TfL line-keyed source, even though it superficially looks id-shaped', () => {
    expect(incidentIdFromSource('tfl-line-status-northern')).toBeNull();
  });

  it('returns null for an empty string', () => {
    expect(incidentIdFromSource('')).toBeNull();
  });
});
```

- [ ] **Step 3: Add the link to `DisruptionDetail.tsx`**

```tsx
import { incidentIdFromSource } from '@/lib/incidents';
import { TextLink } from './TextLink';
```

Compute at the top of the component body:

```tsx
const incidentId = incidentIdFromSource(disruption.source);
```

Add, immediately after the existing `{disruption.source && (<Text size="xs" c="dimmed">Source: {disruption.source}</Text>)}` block:

```tsx
{incidentId && (
  <TextLink href={`/incidents/${incidentId}`} underline="always">
    View full incident details
  </TextLink>
)}
```

`TextLink` (`frontend/components/TextLink.tsx`) has no `'use client'` of its own and is safe to use inside `DisruptionDetail.tsx`'s existing `'use client'` boundary.

- [ ] **Step 4: Add render tests to `DisruptionDetail.test.tsx`**

```tsx
it('renders a link to the incident detail page when source names a real incident', () => {
  renderWithMantine(<DisruptionDetail disruption={sample} />);
  const link = screen.getByRole('link', { name: 'View full incident details' });
  expect(link).toHaveAttribute('href', '/incidents/123');
});

it('renders no incident-detail link when source is the LDBWS-inferred literal', () => {
  renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: 'ldbws-sampling' }} />);
  expect(screen.queryByRole('link', { name: 'View full incident details' })).not.toBeInTheDocument();
});

it('renders no incident-detail link when source is a TfL line-keyed value', () => {
  renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: 'tfl-line-status-northern' }} />);
  expect(screen.queryByRole('link', { name: 'View full incident details' })).not.toBeInTheDocument();
});

it('renders no incident-detail link when source is null', () => {
  renderWithMantine(<DisruptionDetail disruption={{ ...sample, source: null }} />);
  expect(screen.queryByRole('link', { name: 'View full incident details' })).not.toBeInTheDocument();
});
```

(`sample.source` is already `'knowledgebase-incident-123'`, confirmed in the existing fixture — the first test above should resolve `href="/incidents/123"`.)

- [ ] **Step 5: Add one regression assertion in `IssueList.test.tsx` proving the link reaches the real render tree**

`IssueList.tsx` embeds `DisruptionDetail` directly — its own existing test file already renders a real disruption fixture (confirmed, `IssueList.test.tsx` line ~246) without mocking `DisruptionDetail`, so this is the concrete end-to-end proof that "the wiring reaches both real call sites" isn't just true in theory. Add, near that existing disruption fixture's test case (or as a new `it`):

```tsx
it('surfaces the "View full incident details" link when a status disruption is knowledgebase-sourced', () => {
  // ... render IssueList with a status whose disruption.source is
  // 'knowledgebase-incident-...' (reuse this file's existing disruption
  // fixture, which already uses that source value) ...
  // expand the relevant AccordionItem if IssueList's existing tests do so
  // for other assertions inside a panel, then:
  expect(screen.getByRole('link', { name: 'View full incident details' })).toBeInTheDocument();
});
```

**Implementation-time verification note:** match this file's existing pattern exactly for expanding an `Accordion` panel before asserting on its contents (if the existing tests already do this for other panel-body assertions — check how the file's other `status.disruption`-adjacent assertions reach into the panel) rather than inventing a new interaction pattern.

- [ ] **Step 6: Run the frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`.
Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/lib/incidents.ts frontend/lib/incidents.test.ts frontend/components/DisruptionDetail.tsx frontend/components/DisruptionDetail.test.tsx frontend/components/IssueList.test.tsx
git commit -m "Wire a 'View full incident details' link into DisruptionDetail"
```

---

### Task 5: Frontend — `lib/types.ts` and `lib/api.ts` additions

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: `IncidentLineRef`, `IncidentHistoryEntry`, `IncidentDetail` (types); `getIncident(incidentId: string): Promise<IncidentDetail>`.
- Consumed by: Task 6 (`app/incidents/[id]/page.tsx`).

No dependency on Task 2 at the type/contract level — the wire shape is already fully specified by the design spec's API/type contract section, confirmed consistent with Task 2's `to_incident_detail_json` above. A real end-to-end check (hitting a live backend rather than a mocked `fetch`) does need Task 2 done first.

- [ ] **Step 1: Add the types to `frontend/lib/types.ts`**

```ts
export interface IncidentLineRef {
  id: string;
  name: string;
}

export interface IncidentHistoryEntry {
  summary: string;
  description: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  validityPeriods: ValidityPeriod[];
  isPlanned: boolean;
  isCleared: boolean;
  recordedAt: string; // RFC3339
}

/** `GET /public/incidents/{incidentId}`'s response
 * (`crates/api/src/routes/incidents.rs`). `description` is raw HTML —
 * sanitize with `sanitizeDescription` (`frontend/lib/sanitizeHtml.ts`)
 * before rendering, same as `DisruptionDetail`. `currentlyAffectsLines`
 * is computed fresh per request — can be empty for a cleared or
 * no-longer-matched incident, which is a normal outcome, not an error. */
export interface IncidentDetail {
  incidentId: string;
  summary: string;
  description: string;
  operators: string[];
  affectedStations: string[];
  priority: number;
  validityPeriods: ValidityPeriod[];
  isPlanned: boolean;
  isCleared: boolean;
  firstSeenAt: string; // RFC3339
  fetchedAt: string; // RFC3339
  currentlyAffectsLines: IncidentLineRef[];
  history: IncidentHistoryEntry[];
}
```

Add these after the existing `Disruption`/`ValidityPeriod` interfaces — `ValidityPeriod` is already defined and reused verbatim, no new shape needed for it.

- [ ] **Step 2: Add `getIncident` to `frontend/lib/api.ts`**

```ts
/** `GET /public/incidents/{incidentId}`. Public, unauthenticated read — no
 * cookie forwarding needed, same plain `fetchJson` pattern as
 * `getLineDefinition`/`getCustomLine`. Throws `ApiNotFoundError` on a 404
 * (via `errorForResponse`, same as every other `fetchJson` caller) —
 * `app/incidents/[id]/page.tsx` catches it and calls `notFound()`,
 * identical to `/lines/[id]`'s existing pattern. */
export async function getIncident(incidentId: string): Promise<IncidentDetail> {
  return fetchJson<IncidentDetail>(`${baseUrl()}/public/incidents/${incidentId}`, {
    cache: 'no-store',
  });
}
```

Add `IncidentDetail` to this file's existing `import type { ... } from './types';` list.

- [ ] **Step 3: Add tests to `frontend/lib/api.test.ts`**

Add `getIncident` to the existing import list from `./api`, then add (matching this file's existing `getLineDefinition`/`getCustomLine` test style — no cookie stubbing needed, this is not a cookie-forwarding read):

```ts
it('getIncident fetches the correct URL with no caching', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response(JSON.stringify({ incidentId: '123' }), { status: 200 })));
  await getIncident('123');
  expect(fetch).toHaveBeenCalledWith('http://test-api:8080/public/incidents/123', expect.objectContaining({ cache: 'no-store' }));
});

it('getIncident throws ApiNotFoundError on a 404', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
  await expect(getIncident('does-not-exist')).rejects.toThrow(ApiNotFoundError);
});

it('getIncident still throws on a non-404 failure', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
  await expect(getIncident('123')).rejects.toThrow(/500/);
});
```

(Verify the exact base URL string this test file's `beforeEach` stubs — `http://test-api:8080` above matches the value confirmed directly in this file today; copy whatever's actually there rather than assuming.)

- [ ] **Step 4: Run the frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`.
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add IncidentDetail type and getIncident read function"
```

---

### Task 6: Frontend — `/incidents/[id]` page

**Files:**
- Create: `frontend/app/incidents/[id]/page.tsx`
- Create: `frontend/app/incidents/[id]/page.test.tsx`
- Create: `frontend/app/incidents/[id]/not-found.tsx`

**Interfaces:**
- Consumes: `getIncident` (Task 5), `sanitizeDescription` (Task 3), `TextLink` (existing), `formatDate`/`formatDateTime` (existing, `frontend/lib/dateFormat.ts`).
- Produces: default-exported async Server Component.
- Consumed by: users following the link Task 4 added (and anyone directly typing/sharing the URL).

Depends on Task 3 and Task 5 being complete.

- [ ] **Step 1: Write `frontend/app/incidents/[id]/not-found.tsx`**

Matches `/lines/[id]/not-found.tsx`'s established shape:

```tsx
import { Group, Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function IncidentNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Incident not found</Title>
      <Text c="dimmed">
        No incident matches that ID. It may have been mistyped, or this app may never have ingested it — there is
        no retention/prune job on incident records today, so this is indistinguishable from a very old, no-longer-tracked one.
      </Text>
      <Group gap="lg">
        <TextLink href="/" underline="always">
          Back to your dashboard
        </TextLink>
      </Group>
    </Stack>
  );
}
```

- [ ] **Step 2: Write `frontend/app/incidents/[id]/page.tsx`**

```tsx
import { notFound } from 'next/navigation';
import { Badge, Divider, Group, Stack, Text, Title } from '@mantine/core';
import { ApiNotFoundError, getIncident } from '@/lib/api';
import { sanitizeDescription } from '@/lib/sanitizeHtml';
import { TextLink } from '@/components/TextLink';
import { formatDateTime } from '@/lib/dateFormat';
import type { IncidentDetail, IncidentHistoryEntry, ValidityPeriod } from '@/lib/types';

// Same rationale as every dynamic `[param]` route in this app: without
// this, `next build` may try to prerender against a database that only
// exists on the compose network at runtime. (Note: `/lines/[id]/page.tsx`
// and `/stations/[crs]/page.tsx` — the two structurally closest existing
// pages — do NOT declare this explicitly and still render dynamically, so
// this may be a no-op in practice; added anyway for explicitness, matching
// `/lines/[id]/history/page.tsx`'s and the dashboard's convention.)
export const revalidate = 0;

function formatValidityPeriod(period: ValidityPeriod): string {
  const from = formatDateTime(period.fromDate);
  return period.toDate ? `${from} – ${formatDateTime(period.toDate)}` : `${from} – ongoing`;
}

/** Which of a history entry's fields differ from the entry immediately
 * after it in the (newest-first) list — a short textual diff summary
 * rather than a full field dump every time, since most consecutive
 * snapshots differ in only one or two fields. `older` is `undefined` for
 * the oldest entry (nothing to diff against — it's the incident's
 * first-seen snapshot). */
function describeChanges(entry: IncidentHistoryEntry, older: IncidentHistoryEntry | undefined): string {
  if (!older) return 'First seen';
  const changes: string[] = [];
  if (entry.summary !== older.summary) changes.push('summary changed');
  if (entry.description !== older.description) changes.push('description changed');
  if (entry.priority !== older.priority) changes.push(`priority changed from ${older.priority} to ${entry.priority}`);
  if (JSON.stringify(entry.validityPeriods) !== JSON.stringify(older.validityPeriods)) changes.push('validity changed');
  if (entry.isPlanned !== older.isPlanned) changes.push(`isPlanned changed to ${entry.isPlanned}`);
  if (entry.isCleared !== older.isCleared) changes.push(`isCleared changed to ${entry.isCleared}`);
  return changes.length > 0 ? changes.join(', ') : 'Re-confirmed, no change';
}

export default async function IncidentDetailPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;

  let incident: IncidentDetail;
  try {
    incident = await getIncident(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Group gap="sm">
        <Title order={1}>{incident.summary}</Title>
        <Badge color={incident.isPlanned ? 'blue' : 'orange'}>{incident.isPlanned ? 'Planned Work' : 'Real-Time'}</Badge>
      </Group>

      <div dangerouslySetInnerHTML={{ __html: sanitizeDescription(incident.description) }} />

      {incident.affectedStations.length > 0 && (
        <Group gap="xs">
          {incident.affectedStations.map((crs) => (
            <Badge key={crs} variant="outline" color="gray">
              {crs}
            </Badge>
          ))}
        </Group>
      )}

      <Stack gap={4}>
        <Text fw={500}>Validity</Text>
        {incident.validityPeriods.map((period, i) => (
          <Text key={i} size="sm" c="dimmed">
            {formatValidityPeriod(period)}
          </Text>
        ))}
      </Stack>

      <Divider />

      <Stack gap={4}>
        <Text fw={500}>Currently affects</Text>
        {incident.currentlyAffectsLines.length === 0 ? (
          <Text size="sm" c="dimmed">
            Not currently reported on any tracked line.
          </Text>
        ) : (
          <Group gap="md">
            {incident.currentlyAffectsLines.map((line) => (
              <TextLink key={line.id} href={`/lines/${line.id}`}>
                {line.name}
              </TextLink>
            ))}
          </Group>
        )}
      </Stack>

      <Divider />

      <Stack gap="xs">
        <Text fw={500}>History</Text>
        {incident.history.map((entry, i) => (
          <Stack key={i} gap={2}>
            <Text size="sm">{formatDateTime(entry.recordedAt)}</Text>
            <Text size="sm" c="dimmed">
              {describeChanges(entry, incident.history[i + 1])}
            </Text>
          </Stack>
        ))}
      </Stack>

      <Divider />

      <Stack gap={2}>
        <Text size="xs" c="dimmed">
          First seen: {formatDateTime(incident.firstSeenAt)}
        </Text>
        <Text size="xs" c="dimmed">
          Last fetched: {formatDateTime(incident.fetchedAt)}
        </Text>
      </Stack>
    </Stack>
  );
}
```

**Implementation-time note:** `describeChanges`/the history section's exact visual treatment is flagged by the design spec's Open Question 2 as a real, unresolved styling choice — the plain grouped list above follows Decision 6's recommendation (closer to `/lines/[id]/history`'s precedent than Mantine's unused `Timeline` component), but is not meant to be pixel-final; adjust freely as long as every history entry still renders with a timestamp and a change summary.

- [ ] **Step 3: Write `frontend/app/incidents/[id]/page.test.tsx`**

Mock `@/lib/api` and `next/navigation`, matching `/lines/[id]/page.test.tsx`'s established pattern:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import IncidentDetailPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError } from '@/lib/api';
import type { IncidentDetail } from '@/lib/types';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getIncident: vi.fn() };
});
vi.mock('next/navigation', () => ({ notFound: vi.fn(() => { throw new Error('NEXT_NOT_FOUND'); }) }));

function detail(overrides: Partial<IncidentDetail> = {}): IncidentDetail {
  return {
    incidentId: '12345',
    summary: 'Signal failure at Woking',
    description: '<p>Delays expected</p>',
    operators: ['VT'],
    affectedStations: ['WOK', 'WAT'],
    priority: 3,
    validityPeriods: [{ fromDate: '2026-08-30T09:00:00Z', toDate: null, isNow: true }],
    isPlanned: false,
    isCleared: false,
    firstSeenAt: '2026-08-30T09:00:00Z',
    fetchedAt: '2026-08-31T10:15:00Z',
    currentlyAffectsLines: [{ id: 'south-western', name: 'South Western Main Line' }],
    history: [
      {
        summary: 'Signal failure at Woking',
        description: '<p>Delays expected</p>',
        operators: ['VT'],
        affectedStations: ['WOK', 'WAT'],
        priority: 3,
        validityPeriods: [{ fromDate: '2026-08-30T09:00:00Z', toDate: null, isNow: true }],
        isPlanned: false,
        isCleared: false,
        recordedAt: '2026-08-30T09:00:00Z',
      },
    ],
    ...overrides,
  };
}

describe('IncidentDetailPage', () => {
  it('calls notFound() when getIncident throws ApiNotFoundError', async () => {
    vi.mocked(api.getIncident).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(IncidentDetailPage({ params: Promise.resolve({ id: 'does-not-exist' }) })).rejects.toThrow();
  });

  it('renders the summary, description, and affected stations', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText('Signal failure at Woking')).toBeInTheDocument();
    expect(screen.getByText('Delays expected')).toBeInTheDocument();
    expect(screen.getByText('WOK')).toBeInTheDocument();
  });

  it('renders a link to each currently-affected line', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByRole('link', { name: 'South Western Main Line' })).toHaveAttribute('href', '/lines/south-western');
  });

  it('renders the "not currently reported anywhere" empty state', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail({ currentlyAffectsLines: [] }));
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText('Not currently reported on any tracked line.')).toBeInTheDocument();
  });

  it('renders the history timeline with at least the first-seen entry', async () => {
    vi.mocked(api.getIncident).mockResolvedValue(detail());
    renderWithMantine(await IncidentDetailPage({ params: Promise.resolve({ id: '12345' }) }));
    expect(screen.getByText('First seen')).toBeInTheDocument();
  });
});
```

**Implementation-time verification note:** confirm the exact `notFound()`-mocking idiom this repo's other page tests use (`/lines/[id]/page.test.tsx` mocks `next/navigation` too — copy its exact shape rather than the illustrative one above) so this test's assertion style matches the established convention precisely.

- [ ] **Step 4: Run the frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`.
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/incidents
git commit -m "Add /incidents/[id] detail page"
```

---

## Sequencing notes

- Tasks 1 → 2 are backend-only and must run in that order: Task 2 imports Task 1's query functions/row types directly.
- Task 3 (extract `sanitizeDescription`) and Task 4 (wire the link) have no dependency on the backend tasks — both only touch existing, already-shipped frontend code (`DisruptionDetail.tsx` and its test file) and could be done first, in parallel with Tasks 1-2, if that suits the executor better. They are sequenced 3-then-4 here only because both modify `DisruptionDetail.tsx` and doing the sanitizer extraction first avoids a self-inflicted diff conflict with the link addition — there is no real dependency between them beyond that.
- Task 5 (types/api client) can be authored as soon as the wire contract is known — which it already is, from the design spec's API/type contract section — without Task 2 being merged. The honest end-to-end path (a real backend to hit rather than a mocked `fetch`) does need Task 2 done first.
- Task 6 depends on Task 3 (imports `sanitizeDescription`) and Task 5 (imports `getIncident`/`IncidentDetail`). It has no import-level dependency on Task 4, but Task 4 is what makes the page actually reachable from anywhere in the live UI — both should ship together for the feature to be end-to-end usable, even though nothing would fail to compile if they didn't.
- Overall recommended order: 1, 2, 3, 4, 5, 6 — matching the dependency chain above and this repo's own backend-then-frontend structure (`docs/superpowers/plans/2026-08-31-tickets-list.md`, this plan's closest structural precedent).

## Open questions carried forward from the spec (not resolved by this plan)

1. **`priority` (raw RDM integer, no documented "major"/"minor" meaning) is returned by the API (Task 2) but this plan does not resolve how, or whether, to give it a legend on the page itself** — Task 6 leaves it unsurfaced in the rendered page body (available in the API response for a future task to use), per the spec's own Open Question 1.
2. **The history timeline's visual treatment (Task 6, `describeChanges`) is a real, unresolved design choice**, per the spec's Open Question 2 — this plan implements a plain grouped list (Decision 6's recommendation) as a working default, explicitly flagged in Task 6 as not meant to be pixel-final.
3. **No retention/pruning job exists for `incidents`/`incident_history`** (spec's Open Question 3, also called out in this plan's Global Constraints as out of scope) — this plan's 404 behavior for a very old, never-ingested incident is indistinguishable from "never existed," which `frontend/app/incidents/[id]/not-found.tsx` (Task 6) says explicitly in its copy rather than pretending otherwise.
4. **`lines_currently_reporting_incident`'s full-scan-plus-unnest query (Task 1) has no measured cost yet** — reasoned to be cheap given `line_status`'s current size (spec's Open Question 4), not re-litigated by this plan.
