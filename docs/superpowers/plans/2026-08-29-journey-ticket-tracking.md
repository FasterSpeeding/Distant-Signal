# Journey Ticket Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user attach a ticket record (operator, ticket type, origin/destination) to an already-tracked train they own — via three ascending-effort ingestion tiers: manual entry (the durable v1 backbone), best-effort `.pkpass` (Apple Wallet) auto-fill, and best-effort PDF e-ticket auto-fill — all funneling through the same review-before-save write path. Once a ticket exists against a *resolved* tracked train, derive a plain-language Delay Repay eligibility estimate from that train's own TRUST-sourced delay data, that only ever links out to the operator's own claim form.

**Architecture:** No new crate, no new persistent connection — this feature is pure request/response CRUD plus a small set of pure functions, fitting entirely inside `crates/api`, extending the `/Train/...` route family and `tracked_trains` data model that `docs/superpowers/plans/2026-08-28-train-tracking.md` already shipped (verified merged: `crates/api/src/data/train_tracking.rs`, `crates/api/src/routes/train.rs`, `tracked_trains`/`train_movement_events`/`train_current_state` all exist and are live). A new `tracked_train_tickets` table (Task 1) hangs off `tracked_trains` and `users` (both already merged, per `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`). Manual entry (Tasks 2–3) is the *only* path in this whole feature that ever writes to `tracked_train_tickets`; `.pkpass` (Tasks 6–7) and PDF (Tasks 8–9) uploads are read-only parse-and-preview routes whose output must go through a second, separate client request to that same manual-entry endpoint before anything is saved. A Delay Repay eligibility estimator (Task 4) is a pure, database-pool-free function reachable only from a single read-only `GET` route (Task 5); no task in this plan, and no future task without a fresh design review, may wire it into anything that submits, confirms, or asserts a claim.

**Tech Stack:** Rust/sqlx/axum (`crates/api`, no new crate), PostgreSQL, `zip` (new dependency, reading `.pkpass` ZIP containers), `pdf-extract` (new dependency, PDF text-layer extraction), `regex` (new direct dependency of `crates/api`; already resolved transitively in the workspace lockfile via another crate, so no new major-version surprise expected), axum's `multipart` Cargo feature (new feature flag on the existing `axum` dependency).

**Spec:** `docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md` — read in full before starting; this plan does not restate its research, only its resulting decisions, and resolves several things the design doc left as open implementation-level choices (flagged inline below wherever this plan makes such a call).

## Crate-landscape research (this plan's own pass, re-confirming the design doc's citations)

Checked directly against crates.io as of this plan's writing (2026-08-29), all three actively maintained, no stale citations found:

- **`zip`**: max stable `8.6.0`, last published 2026-08-11. A read-only, deflate-only build needs `default-features = false, features = ["deflate"]` (the crate's own defaults additionally pull in `aes-crypto`/`bzip2`/`deflate64`/`lzma`/`ppmd`/`time`/`xz`/`zstd`, none of which this feature needs for reading a plain, unencrypted `.pkpass` ZIP). See Task 6.
- **`pdf-extract`**: `0.12.0`, last published 2026-06-25, 4.4M+ downloads since 2018 — the design doc's citation holds, and its download count over `pdfsink-rs` (a newer, much lower-download alternative also checked in this pass — 8.8k downloads, created April 2026) is why this plan uses `pdf-extract` directly rather than the newer higher-level crate the design doc mentioned only as one option among several. See Task 8.
- **`lopdf`**: `0.44.0`, last published 2026-07-10 — also actively maintained, but this plan does not add it as a dependency: `pdf-extract`'s `extract_text_from_mem` alone is sufficient for this feature's "native text layer, not OCR" scope: `lopdf`'s lower-level object-model API would only be needed for something this plan doesn't do (rewriting/generating PDFs, or hand-rolling text extraction). Noted here so a future task doesn't re-research this.
- **`regex`**: `1.13.1` already resolved in the workspace `Cargo.lock` (pulled in transitively by an existing dependency) — adding it as a direct dependency of `crates/api` (Task 8) cannot introduce a new major version into the workspace.

## Global Constraints

- **No new configuration.** Unlike train-tracking (RDM/Kafka credentials) or user-accounts-sso (OIDC client secrets), this feature acquires no new external data-access relationship and needs no new env vars — the design doc's Verdict is explicit that this is the point.
- **Migration ordering.** Timestamp-prefixed SQL under `crates/api/migrations/`; the latest existing file is `20260828120000_train_tracking.sql`. This plan's migration is `20260829090000_journey_ticket_tracking.sql`, and depends on both `tracked_trains` (`20260828120000_train_tracking.sql`) and `users` (`20260828090000_user_accounts.sql`) — both already applied in this repo as of this plan's writing, so there is no cross-plan ordering note needed here (unlike those two plans' mutual note at their own tops).
- **The never-assert-proof-of-travel / never-auto-submit rule, enforced structurally, not just by convention.** This is the design's central safety rule, drawn directly from the Delay Repay Sniper precedent (design doc Research summary §5). This plan enforces it four separate ways, all load-bearing:
  1. `estimate_delay_repay`/`claim_url_for` (Task 4) are pure functions — no `PgPool` parameter, no I/O capability of any kind. They *cannot* record "a claim was made," "a claim was submitted," or anything resembling claim state, even by an accidental future edit, because they have no way to write anywhere.
  2. The only route in this entire plan that touches Delay Repay data is `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay` (Task 5) — a read-only `GET`. There is no `POST`/`PUT`/`DELETE` route anywhere in this plan for anything Delay-Repay-shaped.
  3. The route's response type carries a non-optional `disclaimer: String` and a non-optional `claimUrl: String`, always populated — including when no percentage estimate could be computed — so a caller can never receive a bare number with no caveat and no link attached (see Task 5).
  4. **Any future task that wires this estimator's output into a write path requires a fresh design-doc pass.** Do not extend this plan or a follow-up plan to add claim submission without one — that is exactly the boundary this whole feature exists to respect.
- **Review-before-save, enforced structurally, not just documented.** `.pkpass`/PDF upload routes (Tasks 7, 9) perform **zero database writes** — this is a literal, checkable property: grep each route handler for `sqlx::query`/`INSERT` and confirm there is none. The *only* `INSERT` path into `tracked_train_tickets` anywhere in the codebase is `train_tracking::create_ticket` (Task 2), called only by `POST /Train/{trackingId}/tickets` (Task 3). An upload route returns a `PartialTicket` preview; turning that preview into a saved row requires a second, separate client request to the manual-entry endpoint, carrying whatever the user reviewed/edited, plus an honest `source` tag identifying which tier produced it.
- **Legal/privacy schema audit — a hard constraint, not just Task 1 prose.** `tracked_train_tickets` may never gain a column for payment/price data, any barcode payload (raw or decoded), any ITSO data, passenger name, or the uploaded file itself (design doc's Data model: "Deliberately not stored"). Task 1 explicitly diffs its own column list against this list before being marked done.
- **Ownership model: ticket routes are session-gated and owner-scoped; train-state reads stay public.** Every ticket route (create, list, delay-repay estimate — Tasks 3, 5) requires `AuthenticatedUser` and is scoped to the caller's own rows, returning `404` (never `403`) for "exists but not yours" — matching `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s existing convention (`custom_lines`' `update_custom_line`) and `docs/superpowers/plans/2026-08-28-train-tracking.md`'s Task 3 (pin creation, also session-gated). This is a deliberate *difference* from that same plan's Task 5 (`GET /Train/{trackingId}`), which stays public/unscoped on the reasoning that a train's live position is a public transit fact — a ticket record is a materially more personal claim ("I personally had a ticket for this") than that, so it does not inherit the shareable-link posture. `.pkpass`/PDF upload routes (Tasks 7, 9) still require `AuthenticatedUser` (no anonymous file-parsing endpoint) but do **not** need a `tracked_trains` ownership check — they read and write no `tracked_train_id`-scoped row at all; the tracking id in their URL path is context for the client's follow-up confirm request, not a resource the upload route itself touches.
- **CRS fields stay free `TEXT`, validated at the app layer — and that validation is the actual review-before-save mechanism, not just a UI nicety.** Neither `.pkpass` nor PDF extraction can ever recover a real CRS code (neither format publishes one — both give station *names*, e.g. "Kings Cross"). `validate_ticket_entry` (Task 2) enforces the same 3-letter-CRS format check `train_tracking::validate_pin` already enforces for `TrackPinRequest.origin_crs`. That check is what actually *forces* a human to correct a pre-filled station name into a real code before a row can ever be saved — the concrete mechanism behind "review before save," not merely a documented convention.
- **No barcode/ITSO decoding, anywhere, full stop** — restated from the design doc's Non-goals as a hard constraint every task must respect, not just Tasks 6–9's own framing.
- **Operator/scheme/claim-URL data is a small, explicitly-sourced starter table, not an invented catalogue.** Task 4's `DR30_OPERATORS`/`CLAIM_URLS` tables cover only the three operators this plan's own research pass could verify against each operator's live Delay Repay page (LNER, CrossCountry, ScotRail — see Task 4). An operator not in that table still gets a DR15-percentage estimate (per the design doc's own cited "most operators use DR15" finding — a general research finding, not a per-operator invention) but always the National Rail generic compensation page as its `claim_url`, never a fabricated operator-specific URL. Expanding this table with more individually-verified operators is real, valuable follow-up work, explicitly out of scope here — do not backfill it with unverified guesses to make the feature look more complete than its research supports.
- **File upload hygiene (Tasks 6–9).** Every upload route is bounded by `DefaultBodyLimit::max(8 * 1024 * 1024)` (8 MiB — generous for a boarding pass or e-ticket PDF, bounded against abuse) layered onto `train::router()`. `parse_pkpass` bounds every ZIP-entry read it performs to guard against a zip-bomb-style small-file/huge-decompressed-content mismatch. `parse_pdf`'s call into the third-party `pdf_extract` crate is wrapped in `std::panic::catch_unwind`, since it parses untrusted, potentially-malformed input via code this app doesn't control, and a panic inside it must not take the whole request handler down. Neither the uploaded `.pkpass` nor the uploaded PDF is ever persisted past the request — processed transiently in memory only, matching the design doc's Data model.
- **`crates/aggregator` and `crates/trust-consumer` are untouched by every task in this plan** — this is per-user ticket metadata plus a pair of pure functions, not aggregation or feed-ingestion input.
- **Wire-type convention, matching `docs/superpowers/plans/2026-08-28-train-tracking.md`'s own Global Constraints exactly:** request-body types shared as `common` wire types (`TicketEntryRequest`, alongside the existing `TrackPinRequest`) use plain `snake_case` field names; response types composed purely for one route's own JSON output (`TrackedTrainTicket`, `PartialTicket`, `DelayRepayEstimateResponse`) are defined directly in `crates/api` and use `#[serde(rename_all = "camelCase")]`, matching `TrackedTrainState`'s existing precedent.
- **New dependencies:** `zip` (Task 6), `pdf-extract` and `regex` (Task 8). `axum`'s `multipart` feature is enabled on the existing dependency (Task 7).

---

### Task 1: Database schema migration

**Files:**
- Create: `crates/api/migrations/20260829090000_journey_ticket_tracking.sql`

**Interfaces:**
- Produces: `tracked_train_tickets` table. Consumed by Task 2 (`create_ticket`/`list_tickets_for_tracked_train`/`get_ticket_owned`/`tracked_train_owner`), Task 5 (`delay-repay` route reads `operator`).
- **Depends on:** `tracked_trains` (`docs/superpowers/plans/2026-08-28-train-tracking.md`'s Task 1) and `users` (`docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 1) — both already merged and applied in this repo.

- [ ] **Step 1: Write the migration**

Create `crates/api/migrations/20260829090000_journey_ticket_tracking.sql`:

```sql
-- -------------------------------------------------------------------------
-- Journey ticket tracking: a user-entered (or best-effort auto-filled,
-- always user-reviewed-before-save) record that they had a ticket for a
-- specific tracked train, plus the data needed to derive a Delay Repay
-- eligibility estimate against that train's own TRUST-sourced delay data.
-- See docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md
-- and docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md.
--
-- LEGAL/PRIVACY AUDIT (see this plan's Global Constraints): this table
-- deliberately stores ONLY operator, ticket_type, origin_crs,
-- destination_crs, source, and timestamps/ownership. It must NEVER gain a
-- column for payment/price data, any barcode payload (raw or decoded), any
-- ITSO data, passenger name, or the uploaded .pkpass/PDF file itself.
-- Diff any future migration touching this table against this list before
-- merging it.
--
-- source: provenance, extending DESIGN.md's dataQuality philosophy (see
-- DESIGN.md's Data quality section) of never collapsing inferred data into
-- an unlabelled value. 'manual' is the only trustworthy-by-construction
-- source; 'pkpass-semantics' / 'pkpass-heuristic' / 'pdf-heuristic' are all
-- pre-fills the user reviewed and explicitly confirmed via a manual-entry
-- POST before this row existed -- confirmation, not the parse itself, is
-- what makes the row trustworthy. See this plan's Task 2/3.
-- -------------------------------------------------------------------------

CREATE TABLE tracked_train_tickets (
    id BIGSERIAL PRIMARY KEY,
    tracked_train_id BIGINT NOT NULL REFERENCES tracked_trains(id) ON DELETE CASCADE,

    -- Redundant with tracked_trains.user_id by construction (a ticket's
    -- owner is always the same user who owns the tracked train it's
    -- attached to -- see Task 2's create_ticket, which only ever writes
    -- user_id from the caller after Task 3's ownership check on
    -- tracked_train_id passes). Kept explicit so every ownership check on
    -- this table filters directly (WHERE user_id = $n) without a join.
    user_id TEXT NOT NULL REFERENCES users(id),

    operator     TEXT,  -- free text or a known operator code; not
                         -- validated against a hard catalogue in v1.
    ticket_type  TEXT,  -- e.g. "single", "return", "season", "advance" --
                         -- user-entered or auto-filled, never parsed from
                         -- a barcode.
    origin_crs       TEXT,
    destination_crs  TEXT,

    source TEXT NOT NULL DEFAULT 'manual'
        CHECK (source IN ('manual', 'pkpass-semantics', 'pkpass-heuristic', 'pdf-heuristic')),

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX tracked_train_tickets_tracked_train ON tracked_train_tickets (tracked_train_id);

-- Supports every ticket route's ownership-scoped query (Task 2) filtering
-- directly on user_id, per this table's own header comment above.
CREATE INDEX tracked_train_tickets_user_id ON tracked_train_tickets (user_id);
```

- [ ] **Step 2: Run the API crate test suite to confirm nothing broke**

Run: `cargo test -p api`
Expected: PASS — one new, unreferenced table; no existing query touches it.

- [ ] **Step 3: Commit**

```bash
git add crates/api/migrations/20260829090000_journey_ticket_tracking.sql
git commit -m "Add tracked_train_tickets table"
```

---

### Task 2: Manual-entry data layer and wire type

**Files:**
- Modify: `crates/common/src/lib.rs`
- Modify: `crates/api/src/data/train_tracking.rs`

**Interfaces:**
- Produces: `common::TicketEntryRequest`. `fn validate_ticket_entry(entry: &TicketEntryRequest) -> Result<(), String>` (pure, unit-tested). `async fn tracked_train_owner(pool, tracking_id) -> Result<Option<String>>`. `async fn create_ticket(pool, tracked_train_id, entry, user_id) -> Result<i64>`. `async fn list_tickets_for_tracked_train(pool, tracking_id, user_id) -> Result<Vec<TrackedTrainTicket>>`. `async fn get_ticket_owned(pool, ticket_id, user_id) -> Result<Option<TrackedTrainTicket>>`. `struct TrackedTrainTicket`.
- Consumed by: Task 3 (all of the above, from the routes), Task 5 (`get_ticket_owned`, `TrackedTrainTicket.operator`).

- [ ] **Step 1: Add `TicketEntryRequest` to `common`**

Add to `crates/common/src/lib.rs`, near `TrackPinRequest`:

```rust
/// Manual ticket-entry payload for `POST /Train/{trackingId}/tickets`
/// (`crates/api/src/routes/train.rs`) -- the durable v1 backbone every
/// ingestion tier ultimately funnels through (see
/// docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md's
/// Architecture section). `source` defaults to "manual"; a `.pkpass`/PDF
/// upload preview (Tasks 6-9) is turned into a saved row by the client
/// re-submitting this same request shape with `source` set to whichever
/// tier produced the reviewed data ("pkpass-semantics" / "pkpass-heuristic"
/// / "pdf-heuristic") -- there is no separate "confirm upload" endpoint;
/// this is the only write path, deliberately.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketEntryRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_crs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_crs: Option<String>,
    #[serde(default = "default_ticket_source")]
    pub source: String,
}

fn default_ticket_source() -> String {
    "manual".to_string()
}
```

- [ ] **Step 2: Confirm the workspace still builds**

Run: `cargo build --workspace`
Expected: PASS — additive, unused by anything yet.

- [ ] **Step 3: Write the failing tests for `validate_ticket_entry`**

Add to `crates/api/src/data/train_tracking.rs` (below the existing `validate_pin`/`create_pin`, above `TrackedTrainRow`):

```rust
use common::TicketEntryRequest;

/// Every allowed value of `tracked_train_tickets.source` -- kept in one
/// place (this constant, not repeated string literals) so this app-layer
/// check and the migration's own CHECK constraint (Task 1) can't silently
/// drift apart; if they ever do, the DB constraint is the backstop.
const TICKET_SOURCES: [&str; 4] = ["manual", "pkpass-semantics", "pkpass-heuristic", "pdf-heuristic"];

/// This is the actual mechanism behind "review before save" for the
/// `.pkpass`/PDF ingestion tiers (Tasks 6-9), not merely a data-quality
/// nicety: neither of those formats can ever recover a real CRS code (both
/// only ever give station NAMES, e.g. "Kings Cross" -- see
/// `crates/api/src/data/ticket_extraction.rs`'s module doc). Rejecting a
/// non-3-letter value here means a `PartialTicket` preview resubmitted
/// unedited is *guaranteed* to fail this check, forcing a human to correct
/// it into a real code before anything is ever saved.
pub fn validate_ticket_entry(entry: &TicketEntryRequest) -> Result<(), String> {
    if !TICKET_SOURCES.contains(&entry.source.as_str()) {
        return Err(format!("source must be one of {TICKET_SOURCES:?}"));
    }
    if let Some(crs) = &entry.origin_crs
        && crs.len() != 3
    {
        return Err("origin_crs must be a 3-letter CRS code".to_string());
    }
    if let Some(crs) = &entry.destination_crs
        && crs.len() != 3
    {
        return Err("destination_crs must be a 3-letter CRS code".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod ticket_entry_tests {
    use super::*;

    fn entry(origin_crs: Option<&str>, source: &str) -> TicketEntryRequest {
        TicketEntryRequest {
            operator: Some("LNER".to_string()),
            ticket_type: Some("single".to_string()),
            origin_crs: origin_crs.map(str::to_string),
            destination_crs: Some("EDB".to_string()),
            source: source.to_string(),
        }
    }

    #[test]
    fn a_well_formed_manual_entry_is_valid() {
        assert!(validate_ticket_entry(&entry(Some("KGX"), "manual")).is_ok());
    }

    #[test]
    fn missing_optional_fields_are_valid() {
        let entry = TicketEntryRequest {
            operator: None,
            ticket_type: None,
            origin_crs: None,
            destination_crs: None,
            source: "manual".to_string(),
        };
        assert!(validate_ticket_entry(&entry).is_ok());
    }

    #[test]
    fn a_station_name_instead_of_a_crs_code_is_rejected() {
        // Exactly the "Kings Cross" vs "KGX" case this check exists for --
        // see this function's doc comment.
        assert!(validate_ticket_entry(&entry(Some("Kings Cross"), "manual")).is_err());
    }

    #[test]
    fn every_declared_source_is_accepted() {
        for source in TICKET_SOURCES {
            assert!(validate_ticket_entry(&entry(Some("KGX"), source)).is_ok(), "{source} should be valid");
        }
    }

    #[test]
    fn an_unknown_source_is_rejected() {
        assert!(validate_ticket_entry(&entry(Some("KGX"), "barcode-decoded")).is_err());
    }
}
```

- [ ] **Step 2: Run the tests to confirm they pass**

Run: `cargo test -p api ticket_entry_tests`
Expected: PASS — implementation and tests written together (same posture `validate_pin`'s own tests used).

- [ ] **Step 3: Add the query functions and `TrackedTrainTicket`**

Add to `crates/api/src/data/train_tracking.rs`, below the tests above:

```rust
/// Returns the owning `user_id` for a tracked train, or `None` if no such
/// tracked train exists. `POST /Train/{trackingId}/tickets` (Task 3) uses
/// this to answer "does this tracked train exist AND belong to the caller"
/// before creating a ticket against it (there's no existing ticket row yet
/// to filter by, unlike the read paths below). A mismatch or missing
/// tracked train both map to the same `404` at the route layer -- never
/// `403` -- matching `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s
/// existing "exists but not yours" convention.
pub async fn tracked_train_owner(pool: &PgPool, tracking_id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT user_id FROM tracked_trains WHERE id = $1").bind(tracking_id).fetch_optional(pool).await?;
    Ok(row.map(|(id,)| id))
}

pub async fn create_ticket(
    pool: &PgPool,
    tracked_train_id: i64,
    entry: &TicketEntryRequest,
    user_id: &str,
) -> anyhow::Result<i64> {
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO tracked_train_tickets \
            (tracked_train_id, user_id, operator, ticket_type, origin_crs, destination_crs, source) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING id",
    )
    .bind(tracked_train_id)
    .bind(user_id)
    .bind(&entry.operator)
    .bind(&entry.ticket_type)
    .bind(&entry.origin_crs)
    .bind(&entry.destination_crs)
    .bind(&entry.source)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// The public read-model for a ticket, returned directly as JSON by
/// `GET /Train/{trackingId}/tickets` (Task 3). Never leaks `user_id` --
/// same posture as `TrackedTrainState`.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackedTrainTicket {
    pub id: i64,
    pub tracked_train_id: i64,
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

const TICKET_SELECT: &str = "\
    SELECT id, tracked_train_id, operator, ticket_type, origin_crs, destination_crs, source, created_at \
    FROM tracked_train_tickets";

/// Filters directly on `(tracked_train_id, user_id)` -- no join needed,
/// per this table's own ownership-redundancy design (see Task 1's migration
/// comment). A caller who doesn't own `tracking_id` gets an empty list,
/// identical to "you own it but have no tickets yet" -- Task 3's route
/// additionally checks `tracked_train_owner` first so the two cases are
/// distinguished at the HTTP layer (404 vs 200 []).
pub async fn list_tickets_for_tracked_train(
    pool: &PgPool,
    tracking_id: i64,
    user_id: &str,
) -> anyhow::Result<Vec<TrackedTrainTicket>> {
    let rows = sqlx::query_as::<_, TrackedTrainTicket>(&format!(
        "{TICKET_SELECT} WHERE tracked_train_id = $1 AND user_id = $2 ORDER BY created_at"
    ))
    .bind(tracking_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Used by the Delay Repay estimate route (Task 5), which needs a single
/// ticket by its own id, still scoped to the caller.
pub async fn get_ticket_owned(pool: &PgPool, ticket_id: i64, user_id: &str) -> anyhow::Result<Option<TrackedTrainTicket>> {
    let row = sqlx::query_as::<_, TrackedTrainTicket>(&format!("{TICKET_SELECT} WHERE id = $1 AND user_id = $2"))
        .bind(ticket_id)
        .bind(user_id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}
```

- [ ] **Step 4: Confirm the crate builds and the full test suite passes**

Run: `cargo build -p api && cargo test -p api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/common/src/lib.rs crates/api/src/data/train_tracking.rs
git commit -m "Add manual ticket-entry data layer and TicketEntryRequest wire type"
```

---

### Task 3: Manual-entry routes — `POST`/`GET /Train/{trackingId}/tickets`

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `POST /Train/{trackingId}/tickets` (session-gated, ownership-checked). `GET /Train/{trackingId}/tickets` (session-gated, ownership-checked). Consumed by: the eventual frontend ticket form (out of scope here), and Tasks 7/9's upload-preview flow, whose output is expected to be resubmitted here.

- [ ] **Step 1: Add the two routes**

Extend `crates/api/src/routes/train.rs`'s `router()`:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id))
        .route("/Train/by-uid/{train_uid}/{date}", axum::routing::get(get_by_uid_and_date))
        .route("/Train/{tracking_id}/tickets", axum::routing::post(post_ticket).get(get_tickets))
}
```

Add the handlers and response types:

```rust
use common::TicketEntryRequest;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TicketCreatedResponse {
    ticket_id: i64,
}

async fn post_ticket(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
    Json(entry): Json<TicketEntryRequest>,
) -> Result<Json<TicketCreatedResponse>, (StatusCode, String)> {
    train_tracking::validate_ticket_entry(&entry).map_err(|msg| (StatusCode::BAD_REQUEST, msg))?;

    match train_tracking::tracked_train_owner(&app.database, tracking_id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }

    let ticket_id = train_tracking::create_ticket(&app.database, tracking_id, &entry, &user.id)
        .await
        .map_err(internal_error("create ticket"))?;

    Ok(Json(TicketCreatedResponse { ticket_id }))
}

async fn get_tickets(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(tracking_id): Path<i64>,
) -> Result<Json<Vec<train_tracking::TrackedTrainTicket>>, (StatusCode, String)> {
    match train_tracking::tracked_train_owner(&app.database, tracking_id).await.map_err(internal_error("check tracked train ownership"))? {
        Some(owner) if owner == user.id => {}
        _ => return Err((StatusCode::NOT_FOUND, "no tracked train with that id".to_string())),
    }

    let tickets = train_tracking::list_tickets_for_tracked_train(&app.database, tracking_id, &user.id)
        .await
        .map_err(internal_error("list tickets"))?;
    Ok(Json(tickets))
}
```

(The ownership check is duplicated across both handlers rather than factored into a shared helper at this point in the file — Task 5 adds a third call site with a slightly different shape (loading a single ticket, not a tracked-train row); factor all three into one helper there once the real shape of "needs it" is settled, rather than guessing the right abstraction now.)

- [ ] **Step 2: Confirm the workspace builds and tests pass**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 3: Manually verify against a live dev stack**

```bash
docker compose --env-file dev.env up --build -d api postgres
psql "$DATABASE_URL" -c "INSERT INTO users (id) VALUES ('TEST-USER') ON CONFLICT DO NOTHING"
psql "$DATABASE_URL" -c "INSERT INTO sessions (id, user_id, expires_at) VALUES ('$(echo -n manual-test-token | sha256sum | cut -d' ' -f1)', 'TEST-USER', NOW() + INTERVAL '1 hour')"
curl -s -X POST http://localhost:8080/Train/track \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"service_date":"2026-08-29","origin_crs":"KGX","scheduled_departure":"2026-08-29T18:32:00Z"}'
# note the returned trackingId, then:
curl -s -X POST http://localhost:8080/Train/1/tickets \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"operator":"LNER","ticket_type":"single","origin_crs":"KGX","destination_crs":"EDB"}'
curl -s http://localhost:8080/Train/1/tickets -H "Cookie: nr_session=manual-test-token"
curl -s http://localhost:8080/Train/1/tickets   # no cookie
```

Expected: ticket creation returns `{"ticketId":1}`; the authenticated `GET` returns a one-element array with `"source":"manual"`; the unauthenticated `GET` returns `401`. Clean up: `psql "$DATABASE_URL" -c "DELETE FROM tracked_trains WHERE id = 1; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"` (cascades to `tracked_train_tickets`).

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Add POST and GET /Train/{trackingId}/tickets manual entry routes"
```

---

### Task 4: Delay Repay eligibility estimator

**Files:**
- Create: `crates/api/src/data/delay_repay_rules.rs`
- Modify: `crates/api/src/data/mod.rs`

**Interfaces:**
- Produces: `fn estimate_delay_repay(operator: &str, delay_minutes: i32) -> Option<DelayRepayEstimate>` (pure, unit-tested). `fn claim_url_for(operator: &str) -> &'static str` (pure, unit-tested, never returns `None` — always resolves to at least the generic National Rail page). `struct DelayRepayEstimate`. `const GENERIC_CLAIM_URL: &str`.
- Consumed by: Task 5 (the one route allowed to call either function — see this plan's Global Constraints).

This is the task where this plan's central safety rule has to hold. Re-read this plan's Global Constraints' "never-assert-proof-of-travel" bullet before writing a line of this file.

- [ ] **Step 1: Write the failing tests**

Create `crates/api/src/data/delay_repay_rules.rs`:

```rust
//! A small, explicitly-maintained-in-this-repo Delay Repay ruleset. There
//! is no official API to sync this against (see
//! docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
//! Research summary §4) -- every value here is compiled from each
//! operator's own live compensation page, cited below, not guessed at.
//!
//! STRUCTURAL SAFETY NOTE, not just a comment: every function in this file
//! is pure (no `PgPool`, no I/O of any kind) and is called from exactly one
//! place in the whole codebase -- `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`
//! (crates/api/src/routes/train.rs, Task 5), a read-only route. This file
//! must never gain a function that writes anywhere, and no future change
//! anywhere in this codebase may wire either function below into a write
//! path without a fresh design-doc pass -- see this plan's Global
//! Constraints. This app estimates eligibility and links out; it never
//! submits a claim or asserts proof of travel, full stop.

use serde::Serialize;

/// A rough eligibility estimate for a Delay Repay claim -- never a
/// guarantee, never proof of travel, never a claim itself. `disclaimer` is
/// intentionally NOT optional: every estimate this function returns
/// carries its own caveat text baked in, so a caller serializing this type
/// cannot accidentally display a bare percentage with no caveat attached.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayRepayEstimate {
    pub scheme: &'static str, // "DR15" | "DR30"
    pub band_minutes: i32,    // the threshold band this delay fell into
    pub percentage: u8,       // rough percentage-of-fare estimate
    pub disclaimer: &'static str,
}

const DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a guarantee of \
    compensation and not proof you travelled. Always verify eligibility and submit any claim \
    directly with the operator -- this app never submits a claim on your behalf.";

/// Operators verified (in this plan's own research pass, cross-checking
/// docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
/// citation) to still run the older Delay Repay 30 scheme, which has no
/// 15-29 minute band at all -- confirmed against each operator's own live
/// page as of 2026-08-29:
///   - LNER: 30+ minutes (delayrepay.lner.co.uk)
///   - CrossCountry: 30+ minutes (delayrepay.crosscountrytrains.co.uk)
///   - ScotRail: 30+ minutes (scotrail.co.uk/plan-your-journey/our-delay-repay-guarantee)
/// Matched case-insensitively as a substring of the ticket's free-text
/// `operator` field (not a hard ATOC-code catalogue -- see this plan's
/// Global Constraints and the design doc's Open Question 6, which this
/// plan does not resolve). An operator NOT in this list is assumed DR15,
/// per the design doc's own cited "most operators use DR15" finding --
/// see `estimate_delay_repay` below.
const DR30_OPERATORS: &[&str] = &["lner", "crosscountry", "scotrail"];

/// Verified, operator-specific claim pages for the same three DR30
/// operators above (found alongside their scheme during this plan's
/// research pass). Every other operator falls back to `GENERIC_CLAIM_URL`
/// -- deliberately not filled in with unverified guesses. See this plan's
/// Global Constraints.
const CLAIM_URLS: &[(&str, &str)] = &[
    ("lner", "https://delayrepay.lner.co.uk/delayrepayV2/"),
    ("crosscountry", "https://delayrepay.crosscountrytrains.co.uk/"),
    ("scotrail", "https://www.scotrail.co.uk/plan-your-journey/our-delay-repay-guarantee"),
];

/// National Rail's own compensation page -- confirmed real and accurate by
/// the design doc's own research (Research summary §4): it "directs
/// passengers to claim directly from your train company." The universal
/// fallback for any operator not in `CLAIM_URLS`, so this route never
/// returns a claim link that goes nowhere real.
pub const GENERIC_CLAIM_URL: &str = "https://www.nationalrail.co.uk/help-and-assistance/compensation-and-refunds/";

/// Returns `None` if `delay_minutes` doesn't clear the relevant scheme's
/// lowest band (e.g. a 20-minute delay on a DR30 operator) -- there is
/// nothing positive to estimate, and the route (Task 5) still surfaces the
/// disclaimer and a claim link regardless of whether this returns `Some`.
pub fn estimate_delay_repay(operator: &str, delay_minutes: i32) -> Option<DelayRepayEstimate> {
    let operator_lower = operator.to_lowercase();
    let scheme_is_dr30 = DR30_OPERATORS.iter().any(|op| operator_lower.contains(op));

    let (scheme, band) = if scheme_is_dr30 {
        ("DR30", dr30_band(delay_minutes)?)
    } else {
        ("DR15", dr15_band(delay_minutes)?)
    };

    Some(DelayRepayEstimate { scheme, band_minutes: band.0, percentage: band.1, disclaimer: DISCLAIMER })
}

fn dr15_band(delay_minutes: i32) -> Option<(i32, u8)> {
    match delay_minutes {
        d if d >= 60 => Some((60, 100)),
        d if d >= 30 => Some((30, 50)),
        d if d >= 15 => Some((15, 25)),
        _ => None,
    }
}

fn dr30_band(delay_minutes: i32) -> Option<(i32, u8)> {
    match delay_minutes {
        d if d >= 60 => Some((60, 100)),
        d if d >= 30 => Some((30, 50)),
        _ => None,
    }
}

/// Never returns `None` -- every caller gets somewhere real to go, even for
/// an operator this table has no specific page for. See `GENERIC_CLAIM_URL`.
pub fn claim_url_for(operator: &str) -> &'static str {
    let operator_lower = operator.to_lowercase();
    CLAIM_URLS
        .iter()
        .find(|(op, _)| operator_lower.contains(op))
        .map(|(_, url)| *url)
        .unwrap_or(GENERIC_CLAIM_URL)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dr15_band_edges() {
        assert_eq!(estimate_delay_repay("Southeastern", 14), None);
        assert_eq!(estimate_delay_repay("Southeastern", 15).unwrap().percentage, 25);
        assert_eq!(estimate_delay_repay("Southeastern", 29).unwrap().percentage, 25);
        assert_eq!(estimate_delay_repay("Southeastern", 30).unwrap().percentage, 50);
        assert_eq!(estimate_delay_repay("Southeastern", 59).unwrap().percentage, 50);
        assert_eq!(estimate_delay_repay("Southeastern", 60).unwrap().percentage, 100);
        assert_eq!(estimate_delay_repay("Southeastern", 30).unwrap().scheme, "DR15");
    }

    #[test]
    fn dr30_band_edges_have_no_fifteen_minute_band() {
        assert_eq!(estimate_delay_repay("LNER", 15), None);
        assert_eq!(estimate_delay_repay("LNER", 29), None);
        assert_eq!(estimate_delay_repay("LNER", 30).unwrap().percentage, 50);
        assert_eq!(estimate_delay_repay("LNER", 59).unwrap().percentage, 50);
        assert_eq!(estimate_delay_repay("LNER", 60).unwrap().percentage, 100);
        assert_eq!(estimate_delay_repay("LNER", 30).unwrap().scheme, "DR30");
    }

    #[test]
    fn dr30_operator_matching_is_case_insensitive_and_substring_based() {
        assert_eq!(estimate_delay_repay("ScotRail", 30).unwrap().scheme, "DR30");
        assert_eq!(estimate_delay_repay("scotrail", 30).unwrap().scheme, "DR30");
        assert_eq!(estimate_delay_repay("Abellio ScotRail", 30).unwrap().scheme, "DR30");
    }

    #[test]
    fn every_estimate_carries_the_disclaimer() {
        let estimate = estimate_delay_repay("LNER", 60).unwrap();
        assert_eq!(estimate.disclaimer, DISCLAIMER);
    }

    #[test]
    fn known_operators_get_their_own_claim_page() {
        assert_eq!(claim_url_for("LNER"), "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert_eq!(claim_url_for("CrossCountry"), "https://delayrepay.crosscountrytrains.co.uk/");
    }

    #[test]
    fn an_unlisted_operator_still_gets_a_real_link_never_none() {
        assert_eq!(claim_url_for("Some Operator Not In Our Table"), GENERIC_CLAIM_URL);
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test -p api delay_repay_rules`
Expected: PASS.

- [ ] **Step 3: Wire the module in**

Add `pub mod delay_repay_rules;` to `crates/api/src/data/mod.rs`.

- [ ] **Step 4: Confirm the crate builds and the full test suite passes**

Run: `cargo build -p api && cargo test -p api`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/delay_repay_rules.rs crates/api/src/data/mod.rs
git commit -m "Add Delay Repay eligibility estimator"
```

---

### Task 5: Delay Repay estimate route — `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay`

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay` (session-gated, ownership-checked). Consumed by: the eventual frontend (out of scope here).

- [ ] **Step 1: Add the route**

Extend `crates/api/src/routes/train.rs`'s `router()`:

```rust
        .route("/Train/{tracking_id}/tickets/{ticket_id}/delay-repay", axum::routing::get(get_delay_repay_estimate))
```

Add the handler:

```rust
use crate::data::delay_repay_rules;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct DelayRepayEstimateResponse {
    delay_minutes: Option<i32>,
    estimate: Option<delay_repay_rules::DelayRepayEstimate>,
    // Always populated, independent of whether `estimate` is `Some` --
    // this route must never leave a caller with a bare percentage and no
    // caveat, or with nowhere real to go. See this plan's Global
    // Constraints.
    claim_url: String,
    disclaimer: &'static str,
}

const DELAY_REPAY_ROUTE_DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a \
    guarantee of compensation and not proof you travelled. This app never submits a claim on your \
    behalf -- verify eligibility and claim directly from the operator using the link above.";

async fn get_delay_repay_estimate(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path((tracking_id, ticket_id)): Path<(i64, i64)>,
) -> Result<Json<DelayRepayEstimateResponse>, (StatusCode, String)> {
    let ticket = train_tracking::get_ticket_owned(&app.database, ticket_id, &user.id)
        .await
        .map_err(internal_error("read ticket"))?
        .filter(|t| t.tracked_train_id == tracking_id)
        .ok_or((StatusCode::NOT_FOUND, "no ticket with that id for that tracked train".to_string()))?;

    let state = train_tracking::get_by_tracking_id(&app.database, tracking_id)
        .await
        .map_err(internal_error("read tracked train state"))?
        .ok_or((StatusCode::NOT_FOUND, "no tracked train with that id".to_string()))?;

    let estimate = match (ticket.operator.as_deref(), state.delay_minutes) {
        (Some(operator), Some(delay_minutes)) => delay_repay_rules::estimate_delay_repay(operator, delay_minutes),
        _ => None,
    };
    let claim_url = ticket.operator.as_deref().map(delay_repay_rules::claim_url_for).unwrap_or(delay_repay_rules::GENERIC_CLAIM_URL);

    Ok(Json(DelayRepayEstimateResponse {
        delay_minutes: state.delay_minutes,
        estimate,
        claim_url: claim_url.to_string(),
        disclaimer: DELAY_REPAY_ROUTE_DISCLAIMER,
    }))
}
```

Note: this handler does not call `train_tracking::tracked_train_owner` at all — `get_ticket_owned` already scopes by `user_id`, and the ticket row's own `tracked_train_id` is compared against the path's `tracking_id` (`.filter(...)`) to reject a ticket id that's real but doesn't belong under this tracking id. That's a tighter, single-query-per-resource check than Task 3's two-step pattern, since there's no "create against an id that might not be yours yet" case here — everything being checked already exists.

- [ ] **Step 2: Confirm the workspace builds and tests pass**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 3: Manually verify — including that the estimate is genuinely read-only**

```bash
# Using the tracked train + ticket created in Task 3's manual verification
# (re-run those steps first if they were already cleaned up), then:
psql "$DATABASE_URL" -c "INSERT INTO train_current_state (tracked_train_id, status, delay_minutes, updated_at) VALUES (1, 'en_route', 35, NOW()) ON CONFLICT (tracked_train_id) DO UPDATE SET delay_minutes = 35"
curl -s http://localhost:8080/Train/1/tickets/1/delay-repay -H "Cookie: nr_session=manual-test-token"
```

Expected: `{"delayMinutes":35,"estimate":{"scheme":"DR15","bandMinutes":30,"percentage":50,"disclaimer":"..."},"claimUrl":"...","disclaimer":"..."}` (LNER's own ticket from Task 3 would actually resolve `DR30` at 35 minutes — no estimate at all, since 35 < LNER's 30-minute *and* the delay here clears it: recompute by hand against Task 4's `dr30_band` before asserting the exact JSON — the point of this step is confirming the route wires the two pieces together correctly, not re-deriving the eligibility table by curl). Confirm via `grep -n "INSERT\|UPDATE\|DELETE" crates/api/src/routes/train.rs` that `get_delay_repay_estimate` itself contains none of those — the only mutation in this whole file remains `post_ticket`'s `create_ticket` call and `post_track`'s `create_pin` call. Clean up: `psql "$DATABASE_URL" -c "DELETE FROM tracked_trains WHERE id = 1; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"`.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Add GET /Train/{trackingId}/tickets/{ticketId}/delay-repay route"
```

---

### Task 6: `.pkpass` boarding-pass parsing

**Files:**
- Create: `crates/api/src/data/ticket_extraction.rs`
- Modify: `crates/api/src/data/mod.rs`
- Modify: `crates/api/Cargo.toml`

**Interfaces:**
- Produces: `struct PartialTicket`. `fn parse_pass_json(pass: &serde_json::Value) -> anyhow::Result<PartialTicket>` (pure, unit-tested). `fn parse_pkpass(bytes: &[u8]) -> anyhow::Result<PartialTicket>` (thin ZIP-reading wrapper around the above; not unit-tested beyond a round-trip smoke test, mirroring `auth::oidc::OidcClient`'s own untested-protocol-plumbing precedent from `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 4).
- Consumed by: Task 7 (the upload route).

- [ ] **Step 1: Add the `zip` dependency**

```bash
cd crates/api
cargo add zip@8 --no-default-features --features deflate
cd ../..
```

Expected: `crates/api/Cargo.toml` gains one dependency line; `Cargo.lock` updates. `zip 8.6.0` is the current stable release as of this plan's crate-landscape research (2026-08-29) — confirm nothing newer/breaking has shipped since if implementing this step much later. `--no-default-features --features deflate` matches this codebase's existing convention of trimming dependency feature sets to only what's used (see `reqwest`'s `--no-default-features --features json,native-tls,gzip` in this same crate) — a `.pkpass` is an ordinary, unencrypted ZIP, so `deflate` (the standard PKZIP compression method) is all reading one requires; the crate's own defaults additionally pull in AES encryption support, bzip2/lzma/ppmd/xz/zstd decompression, and a `time` dependency, none of which this feature needs.

- [ ] **Step 2: Write the failing tests for `parse_pass_json`**

Create `crates/api/src/data/ticket_extraction.rs`:

```rust
//! Best-effort, review-before-save auto-fill for ticket entry: reads
//! openly-documented file formats a user already has (Apple Wallet
//! `.pkpass`, PDF e-tickets) and returns a `PartialTicket` preview -- this
//! module and every function in it NEVER writes to the database (see
//! docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md's Global
//! Constraints on review-before-save) and NEVER decodes a barcode or
//! touches ITSO data, in either format (see the design doc's Non-goals).

use serde::Serialize;

/// What a `.pkpass`/PDF parse could recover -- the same fillable fields as
/// `common::TicketEntryRequest`, minus a user-chosen `source` (this is
/// fixed per parse path) plus a fixed `source` describing which one
/// produced it. `None` means "not found in this file, leave for the user
/// to fill in" -- never guessed at. This is exactly what a human sees on a
/// review-before-save form pre-filled from an upload; nothing here is ever
/// written to `tracked_train_tickets` directly -- see this module's own
/// doc comment.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PartialTicket {
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    /// Best-effort station identifier -- almost never a real CRS code in
    /// practice (neither `.pkpass` nor PDF extraction publishes one; both
    /// give station NAMES, e.g. "Kings Cross"). Deliberately NOT
    /// normalized here: `train_tracking::validate_ticket_entry`'s existing
    /// CRS-format check is what actually forces a human to correct this
    /// into a real code before it can be saved -- see this plan's Global
    /// Constraints.
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub source: &'static str,
}

/// Pure: given `pass.json`'s already-parsed content, returns a
/// `PartialTicket`, preferring Apple's standardised `semantics` dictionary
/// (`departureStationName`/`destinationStationName`) when present, falling
/// back to the positional `primaryFields` convention Apple's own PassKit
/// docs specify for a boarding/transit pass (exactly two entries:
/// departure, then arrival, in that order -- positional, not per-issuer
/// label-string matching, since the ordering is Apple's own convention,
/// not each issuer's choice) when it isn't. See
/// docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
/// Open Question 1: which real UK retailers populate `semantics` is
/// unconfirmed, so both paths are implemented, not just the optimistic
/// one -- obtain 1-2 real sample passes to confirm this split's real-world
/// hit rate before relying on it heavily.
pub fn parse_pass_json(pass: &serde_json::Value) -> anyhow::Result<PartialTicket> {
    let boarding_pass = pass.get("boardingPass").ok_or_else(|| anyhow::anyhow!("not a boardingPass-style pkpass"))?;
    let transit_type = boarding_pass.get("transitType").and_then(|v| v.as_str()).unwrap_or_default();
    anyhow::ensure!(transit_type == "PKTransitTypeTrain", "not a train boarding pass (transitType = {transit_type:?})");

    let operator = pass.get("organizationName").and_then(|v| v.as_str()).map(str::to_string);
    let semantics = boarding_pass.get("semantics");

    let (origin, destination, source) = if let Some((origin, destination)) = semantics.and_then(semantics_origin_destination) {
        (Some(origin), Some(destination), "pkpass-semantics")
    } else {
        let (origin, destination) = primary_fields_origin_destination(boarding_pass);
        (origin, destination, "pkpass-heuristic")
    };

    Ok(PartialTicket { operator, ticket_type: None, origin_crs: origin, destination_crs: destination, source })
}

fn semantics_origin_destination(semantics: &serde_json::Value) -> Option<(String, String)> {
    let origin = semantics.get("departureStationName").and_then(|v| v.as_str())?;
    let destination = semantics.get("destinationStationName").and_then(|v| v.as_str())?;
    Some((origin.to_string(), destination.to_string()))
}

/// Apple's PassKit docs specify a boarding-pass-style pass's
/// `primaryFields` array holds exactly two entries for a transit pass:
/// departure, then arrival, in that order. Returns `(None, None)` for
/// anything that doesn't match that exact two-field shape, rather than
/// guessing at which field is which.
fn primary_fields_origin_destination(boarding_pass: &serde_json::Value) -> (Option<String>, Option<String>) {
    let Some(fields) = boarding_pass.get("primaryFields").and_then(|v| v.as_array()) else {
        return (None, None);
    };
    match fields.as_slice() {
        [origin, destination] => (
            origin.get("value").and_then(|v| v.as_str()).map(str::to_string),
            destination.get("value").and_then(|v| v.as_str()).map(str::to_string),
        ),
        _ => (None, None),
    }
}

#[cfg(test)]
mod pass_json_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn semantics_present_is_preferred_and_labelled_accordingly() {
        let pass = json!({
            "organizationName": "LNER",
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [{"key":"origin","label":"FROM","value":"Kings Cross"}],
                "semantics": {
                    "departureStationName": "Kings Cross",
                    "destinationStationName": "Edinburgh"
                }
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(ticket.operator, Some("LNER".to_string()));
        assert_eq!(ticket.origin_crs, Some("Kings Cross".to_string()));
        assert_eq!(ticket.destination_crs, Some("Edinburgh".to_string()));
        assert_eq!(ticket.source, "pkpass-semantics");
    }

    #[test]
    fn semantics_absent_falls_back_to_the_two_field_primary_fields_heuristic() {
        let pass = json!({
            "organizationName": "Trainline",
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [
                    {"key":"origin","label":"FROM","value":"London Waterloo"},
                    {"key":"destination","label":"TO","value":"Woking"}
                ]
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(ticket.origin_crs, Some("London Waterloo".to_string()));
        assert_eq!(ticket.destination_crs, Some("Woking".to_string()));
        assert_eq!(ticket.source, "pkpass-heuristic");
    }

    #[test]
    fn a_primary_fields_array_of_the_wrong_length_yields_none_not_a_guess() {
        let pass = json!({
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "primaryFields": [{"key":"a","value":"1"}, {"key":"b","value":"2"}, {"key":"c","value":"3"}]
            }
        });
        let ticket = parse_pass_json(&pass).unwrap();
        assert_eq!(ticket.origin_crs, None);
        assert_eq!(ticket.destination_crs, None);
        assert_eq!(ticket.source, "pkpass-heuristic");
    }

    #[test]
    fn a_non_train_transit_type_is_rejected() {
        let pass = json!({"boardingPass": {"transitType": "PKTransitTypeAir"}});
        assert!(parse_pass_json(&pass).is_err());
    }

    #[test]
    fn a_pass_with_no_boarding_pass_at_all_is_rejected() {
        let pass = json!({"organizationName": "Not A Boarding Pass"});
        assert!(parse_pass_json(&pass).is_err());
    }

    #[test]
    fn ticket_type_is_never_guessed_at() {
        let pass = json!({"boardingPass": {"transitType": "PKTransitTypeTrain"}});
        assert_eq!(parse_pass_json(&pass).unwrap().ticket_type, None);
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p api pass_json_tests`
Expected: PASS.

- [ ] **Step 4: Add `parse_pkpass` and its round-trip test**

Add to `crates/api/src/data/ticket_extraction.rs`, above the `#[cfg(test)]` block:

```rust
use std::io::{Read, Write};

/// `pass.json` is plain-text JSON, and real ones are a few KB -- this
/// bounds every ZIP-entry read in this function against a zip-bomb-style
/// small-file/huge-decompressed-content mismatch (see this plan's Global
/// Constraints on file upload hygiene).
const MAX_ENTRY_BYTES: u64 = 1_000_000; // 1 MiB

/// Thin wrapper: unzips the `.pkpass` container, reads `pass.json`,
/// deserializes it, and hands off to `parse_pass_json` (the actual logic,
/// fully unit-tested above). Not unit-tested beyond the round-trip smoke
/// test below -- this function's own job (calling into the `zip` crate
/// correctly) is thin enough that `parse_pass_json`'s own tests carry the
/// real coverage, mirroring `auth::oidc::OidcClient`'s untested-plumbing
/// precedent.
pub fn parse_pkpass(bytes: &[u8]) -> anyhow::Result<PartialTicket> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|err| anyhow::anyhow!("not a valid .pkpass (zip) file: {err}"))?;
    let mut entry = archive
        .by_name("pass.json")
        .map_err(|err| anyhow::anyhow!("pass.json not found in .pkpass archive: {err}"))?;

    let mut buf = Vec::new();
    entry.by_ref().take(MAX_ENTRY_BYTES).read_to_end(&mut buf)?;

    let pass: serde_json::Value = serde_json::from_slice(&buf).map_err(|err| anyhow::anyhow!("pass.json is not valid JSON: {err}"))?;
    parse_pass_json(&pass)
}

#[cfg(test)]
mod parse_pkpass_tests {
    use super::*;

    fn build_pkpass(pass_json: &serde_json::Value) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            writer.start_file("pass.json", zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(pass_json.to_string().as_bytes()).unwrap();
            writer.finish().unwrap();
        }
        buf
    }

    #[test]
    fn a_well_formed_pkpass_round_trips_through_the_full_pipeline() {
        let pass = serde_json::json!({
            "organizationName": "LNER",
            "boardingPass": {
                "transitType": "PKTransitTypeTrain",
                "semantics": {"departureStationName": "Kings Cross", "destinationStationName": "Edinburgh"}
            }
        });
        let bytes = build_pkpass(&pass);
        let ticket = parse_pkpass(&bytes).unwrap();
        assert_eq!(ticket.operator, Some("LNER".to_string()));
        assert_eq!(ticket.source, "pkpass-semantics");
    }

    #[test]
    fn a_zip_with_no_pass_json_is_rejected() {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            writer.start_file("readme.txt", zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(b"not a pass").unwrap();
            writer.finish().unwrap();
        }
        assert!(parse_pkpass(&buf).is_err());
    }

    #[test]
    fn bytes_that_are_not_a_zip_at_all_are_rejected() {
        assert!(parse_pkpass(b"this is definitely not a zip file").is_err());
    }
}
```

Confirm the exact `zip` 8.x API surface used above (`ZipArchive::new`, `by_name`, `ZipWriter::new`/`start_file`/`SimpleFileOptions`/`finish`) against `cargo doc -p zip --open` while implementing this step — this plan was written without the ability to compile-check against the crate directly; the *shape* (read via `ZipArchive`, write via `ZipWriter` for the test fixture only) is what's being prescribed, exact method names are a compile-time detail to true up, same posture as `docs/superpowers/plans/2026-08-28-user-accounts-sso.md`'s Task 4 note on `openidconnect`.

- [ ] **Step 5: Wire the module in**

Add `pub mod ticket_extraction;` to `crates/api/src/data/mod.rs`.

- [ ] **Step 6: Run the full test suite**

Run: `cargo build -p api && cargo test -p api`
Expected: PASS once any method-name drift from Step 4's note is corrected.

- [ ] **Step 7: Commit**

```bash
git add crates/api/Cargo.toml crates/api/Cargo.lock crates/api/src/data/ticket_extraction.rs crates/api/src/data/mod.rs
git commit -m "Add .pkpass boarding-pass parsing"
```

---

### Task 7: `.pkpass` upload route — `POST /Train/{trackingId}/tickets/pkpass`

**Files:**
- Modify: `crates/api/Cargo.toml`
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `POST /Train/{trackingId}/tickets/pkpass` (multipart, session-gated, **review-before-save: returns a preview, writes nothing**). Consumed by: the eventual frontend upload flow (out of scope here), which is expected to let the user edit the returned `PartialTicket` and resubmit it to `POST /Train/{trackingId}/tickets` (Task 3).

- [ ] **Step 1: Enable axum's `multipart` feature**

In `crates/api/Cargo.toml`, change:

```toml
axum = { version = "0.8.9", features = ["http2", "tracing"] }
```

to:

```toml
axum = { version = "0.8.9", features = ["http2", "tracing", "multipart"] }
```

- [ ] **Step 2: Add the body-size limit and the route**

In `crates/api/src/routes/train.rs`, add the limit to `router()` and the new route:

```rust
use axum::extract::DefaultBodyLimit;

pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id))
        .route("/Train/by-uid/{train_uid}/{date}", axum::routing::get(get_by_uid_and_date))
        .route("/Train/{tracking_id}/tickets", axum::routing::post(post_ticket).get(get_tickets))
        .route("/Train/{tracking_id}/tickets/{ticket_id}/delay-repay", axum::routing::get(get_delay_repay_estimate))
        .route("/Train/{tracking_id}/tickets/pkpass", axum::routing::post(post_pkpass_upload))
        // 8 MiB: generous for a real boarding pass or e-ticket PDF (both
        // are typically tens of KB to low single-digit MB), bounded
        // against abuse. Applies to every route on this router, including
        // the small-JSON ones above -- harmless headroom for those, load-
        // bearing for the two upload routes (this one and Task 9's PDF
        // route). See this plan's Global Constraints on file upload
        // hygiene.
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
}
```

Add the handler:

```rust
use axum::extract::Multipart;

use crate::data::ticket_extraction;

/// `tracking_id` is accepted in the path for URL-shape consistency with
/// every other `/Train/{trackingId}/tickets/...` route, and `_user`
/// requires the caller be logged in (no anonymous file-parsing endpoint --
/// see this plan's Global Constraints), but neither is otherwise used:
/// this handler reads and writes no `tracked_train_id`-scoped row at all.
/// It parses an uploaded file and returns a preview; the tracking id only
/// matters to the client's later, separate confirm request
/// (`POST /Train/{trackingId}/tickets`, Task 3).
///
/// REVIEW-BEFORE-SAVE, structurally: this function contains no
/// `sqlx::query` call and touches no database handle -- there is nothing
/// in this file that could accidentally persist an unreviewed upload. See
/// this plan's Global Constraints.
async fn post_pkpass_upload(
    _user: AuthenticatedUser,
    Path(_tracking_id): Path<i64>,
    mut multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    let bytes = read_single_file_field(&mut multipart, "file").await?;
    ticket_extraction::parse_pkpass(&bytes)
        .map(Json)
        .map_err(|err| (StatusCode::UNPROCESSABLE_ENTITY, format!("could not read this as a train .pkpass: {err}")))
}

/// Shared by this route and Task 9's PDF upload route: reads the single
/// multipart field named `field_name` (expected to be `"file"` for both)
/// into memory and returns its raw bytes.
async fn read_single_file_field(multipart: &mut Multipart, field_name: &str) -> Result<Vec<u8>, (StatusCode, String)> {
    while let Some(field) = multipart.next_field().await.map_err(|err| (StatusCode::BAD_REQUEST, format!("malformed upload: {err}")))? {
        if field.name() == Some(field_name) {
            let bytes = field.bytes().await.map_err(|err| (StatusCode::BAD_REQUEST, format!("failed to read upload: {err}")))?;
            return Ok(bytes.to_vec());
        }
    }
    Err((StatusCode::BAD_REQUEST, format!("no '{field_name}' field in upload")))
}
```

- [ ] **Step 3: Confirm the workspace builds and tests pass**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 4: Confirm the review-before-save property, then manually verify**

```bash
grep -n "sqlx::query\|INSERT\|UPDATE\|DELETE" crates/api/src/routes/train.rs
```

Expected: the only matches are inside `post_ticket` (via `create_ticket`) and `post_track` (via `create_pin`) — `post_pkpass_upload`/`read_single_file_field` must not appear.

```bash
docker compose --env-file dev.env up --build -d api postgres
psql "$DATABASE_URL" -c "INSERT INTO users (id) VALUES ('TEST-USER') ON CONFLICT DO NOTHING"
psql "$DATABASE_URL" -c "INSERT INTO sessions (id, user_id, expires_at) VALUES ('$(echo -n manual-test-token | sha256sum | cut -d' ' -f1)', 'TEST-USER', NOW() + INTERVAL '1 hour')"
curl -s -X POST http://localhost:8080/Train/track \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"service_date":"2026-08-29","origin_crs":"KGX","scheduled_departure":"2026-08-29T18:32:00Z"}'
# note the returned trackingId, then, with a real .pkpass file if one is
# available (e.g. exported from a maintainer's own LNER/Trainline booking
# -- see the design doc's Open Question 1) or a hand-built one from
# Task 6's test fixture builder:
curl -s -X POST http://localhost:8080/Train/1/tickets/pkpass \
  -H "Cookie: nr_session=manual-test-token" -F "file=@sample.pkpass"
psql "$DATABASE_URL" -c "SELECT count(*) FROM tracked_train_tickets"
```

Expected: the upload returns a `PartialTicket` JSON preview; `tracked_train_tickets` still has zero rows (nothing was saved). If a real sample pass is available, this is also the point to resolve the design doc's Open Question 1 (whether `semantics` is actually populated by a real UK retailer) — note the result in a follow-up, it isn't blocking for this plan. Clean up: `psql "$DATABASE_URL" -c "DELETE FROM tracked_trains WHERE id = 1; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"`.

- [ ] **Step 5: Commit**

```bash
git add crates/api/Cargo.toml crates/api/src/routes/train.rs
git commit -m "Add POST /Train/{trackingId}/tickets/pkpass upload-and-preview route"
```

---

### Task 8: PDF e-ticket text extraction and parsing

**Files:**
- Modify: `crates/api/src/data/ticket_extraction.rs`
- Modify: `crates/api/Cargo.toml`

**Interfaces:**
- Produces: `fn parse_pdf_text(text: &str) -> PartialTicket` (pure, unit-tested). `fn parse_pdf(bytes: &[u8]) -> anyhow::Result<PartialTicket>` (thin wrapper around `pdf_extract::extract_text_from_mem`, not unit-tested beyond a magic-header smoke test, same posture as `parse_pkpass`).
- Consumed by: Task 9 (the upload route).

- [ ] **Step 1: Add the `pdf-extract` and `regex` dependencies**

```bash
cd crates/api
cargo add pdf-extract@0.12
cargo add regex@1
cd ../..
```

Expected: `crates/api/Cargo.toml` gains two dependency lines. `pdf-extract 0.12.0` is current as of this plan's crate-landscape research (2026-08-29; see this plan's own research section above for why it was chosen over `lopdf`/`pdfsink-rs`). `regex 1.13.1` is already resolved in the workspace `Cargo.lock` via another crate, so this should not pull in a new major version — confirm `cargo tree -p api -i regex` still shows one resolved version after this step, not two.

- [ ] **Step 2: Write the failing tests for `parse_pdf_text`**

Add to `crates/api/src/data/ticket_extraction.rs`, below `parse_pkpass` and its tests:

```rust
/// Pure: given a PDF's already-extracted raw text, applies a small,
/// explicitly per-retailer set of best-effort heuristics. No standardised
/// UK rail e-ticket PDF layout exists across retailers (see
/// docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md's
/// Research summary §3 and Open Question 2) -- this is a genuinely
/// fragile, lower-confidence tier than `.pkpass` parsing, by design; an
/// unmatched field is left `None` for manual completion, never guessed at.
pub fn parse_pdf_text(text: &str) -> PartialTicket {
    let operator = KNOWN_RETAILER_MARKERS.iter().find(|marker| text.contains(**marker)).map(|marker| marker.to_string());

    let (origin, destination) = ROUTE_PATTERN
        .captures(text)
        .map(|caps| (Some(caps[1].trim().to_string()), Some(caps[2].trim().to_string())))
        .unwrap_or((None, None));

    let text_lower = text.to_lowercase();
    let ticket_type = TICKET_TYPE_KEYWORDS.iter().find(|kw| text_lower.contains(&kw.to_lowercase())).map(|kw| kw.to_string());

    PartialTicket { operator, ticket_type, origin_crs: origin, destination_crs: destination, source: "pdf-heuristic" }
}

/// The "smallest possible set of known templates" the design doc's Open
/// Question 2 calls for -- LNER and Trainline only, per that same note.
/// Expanding this list is real follow-up work, not attempted here.
const KNOWN_RETAILER_MARKERS: &[&str] = &["LNER", "Trainline"];

const TICKET_TYPE_KEYWORDS: &[&str] =
    &["Anytime Day Single", "Off-Peak Day Single", "Off-Peak Day Return", "Advance Single", "Season", "Open Return"];

/// Matches the "<origin> to <destination>" shape the design doc's own
/// worked example uses ("18:32 London Waterloo to Woking, Off-Peak Day
/// Single") -- deliberately conservative (letters/spaces/apostrophes/
/// hyphens only) since this matches against unstructured extracted text
/// with no field boundaries at all. Confirm this against 1-2 real e-ticket
/// PDFs at implementation time (Open Question 2 flags real samples as
/// needed, same as `.pkpass`'s Open Question 1) and adjust -- this is a
/// starting point, not a pattern verified against real tickets.
static ROUTE_PATTERN: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| regex::Regex::new(r"([A-Za-z][A-Za-z '\-]+?)\s+to\s+([A-Za-z][A-Za-z '\-]+?)[,\.\n]").unwrap());

#[cfg(test)]
mod parse_pdf_text_tests {
    use super::*;

    #[test]
    fn matches_the_design_docs_own_worked_example() {
        let text = "LNER e-ticket\n18:32 London Waterloo to Woking, Off-Peak Day Single\nFare: withheld";
        let ticket = parse_pdf_text(text);
        assert_eq!(ticket.operator, Some("LNER".to_string()));
        assert_eq!(ticket.origin_crs, Some("London Waterloo".to_string()));
        assert_eq!(ticket.destination_crs, Some("Woking".to_string()));
        assert_eq!(ticket.ticket_type, Some("Off-Peak Day Single".to_string()));
        assert_eq!(ticket.source, "pdf-heuristic");
    }

    #[test]
    fn an_unrecognized_retailer_yields_no_operator_guess() {
        let ticket = parse_pdf_text("Some Other Retailer Ltd e-ticket, King's Cross to York, Anytime Day Single");
        assert_eq!(ticket.operator, None);
    }

    #[test]
    fn text_with_no_route_pattern_match_yields_no_stations() {
        let ticket = parse_pdf_text("LNER receipt: thank you for your purchase");
        assert_eq!(ticket.origin_crs, None);
        assert_eq!(ticket.destination_crs, None);
    }

    #[test]
    fn no_ticket_type_keyword_present_yields_none_not_a_guess() {
        let ticket = parse_pdf_text("Trainline: London Waterloo to Woking");
        assert_eq!(ticket.ticket_type, None);
    }
}
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p api parse_pdf_text_tests`
Expected: PASS.

- [ ] **Step 4: Add `parse_pdf`**

Add to `crates/api/src/data/ticket_extraction.rs`, above the `#[cfg(test)]` blocks:

```rust
/// Thin wrapper: validates the `%PDF-` magic header, extracts the native
/// text layer via the third-party `pdf_extract` crate, and hands off to
/// `parse_pdf_text` (the actual logic, fully unit-tested above).
///
/// `catch_unwind`: `pdf_extract` parses untrusted, potentially-malformed
/// input via code this app doesn't control; a panic inside it must fail
/// this one request, not take the whole handler down. See this plan's
/// Global Constraints on file upload hygiene.
pub fn parse_pdf(bytes: &[u8]) -> anyhow::Result<PartialTicket> {
    anyhow::ensure!(bytes.starts_with(b"%PDF-"), "not a PDF file (missing %PDF- header)");

    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(bytes))
        .map_err(|_| anyhow::anyhow!("PDF text extraction panicked"))?
        .map_err(|err| anyhow::anyhow!("failed to extract text from PDF: {err}"))?;

    Ok(parse_pdf_text(&text))
}

#[cfg(test)]
mod parse_pdf_tests {
    use super::*;

    #[test]
    fn bytes_without_the_pdf_magic_header_are_rejected_before_extraction_is_attempted() {
        assert!(parse_pdf(b"this is not a pdf").is_err());
    }
}
```

Confirm `pdf_extract::extract_text_from_mem`'s exact signature and error type against `cargo doc -p pdf-extract --open` while implementing this step (this plan's crate-landscape research confirmed the function exists in `0.12.0` but not its full signature) — same "shape prescribed, names to true up" posture as Task 6's `zip` note.

- [ ] **Step 5: Run the full test suite**

Run: `cargo build -p api && cargo test -p api`
Expected: PASS once any signature drift from Step 4's note is corrected.

- [ ] **Step 6: Commit**

```bash
git add crates/api/Cargo.toml crates/api/Cargo.lock crates/api/src/data/ticket_extraction.rs
git commit -m "Add PDF e-ticket text extraction and parsing"
```

---

### Task 9: PDF upload route — `POST /Train/{trackingId}/tickets/pdf`

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `POST /Train/{trackingId}/tickets/pdf` (multipart, session-gated, **review-before-save: returns a preview, writes nothing** — same contract as Task 7's `.pkpass` route). Consumed by: the eventual frontend upload flow (out of scope here).

- [ ] **Step 1: Add the route**

Extend `crates/api/src/routes/train.rs`'s `router()`:

```rust
        .route("/Train/{tracking_id}/tickets/pdf", axum::routing::post(post_pdf_upload))
```

Add the handler, reusing `read_single_file_field` from Task 7:

```rust
/// Same contract as `post_pkpass_upload` (Task 7) -- see that handler's
/// doc comment for why `_user`/`_tracking_id` are otherwise unused, and
/// the same REVIEW-BEFORE-SAVE note: no `sqlx::query` call, no database
/// handle, anywhere in this function.
async fn post_pdf_upload(
    _user: AuthenticatedUser,
    Path(_tracking_id): Path<i64>,
    mut multipart: Multipart,
) -> Result<Json<ticket_extraction::PartialTicket>, (StatusCode, String)> {
    let bytes = read_single_file_field(&mut multipart, "file").await?;
    ticket_extraction::parse_pdf(&bytes)
        .map(Json)
        .map_err(|err| (StatusCode::UNPROCESSABLE_ENTITY, format!("could not read this as a PDF e-ticket: {err}")))
}
```

- [ ] **Step 2: Confirm the workspace builds and tests pass**

Run: `cargo build --workspace && cargo test -p api`
Expected: PASS.

- [ ] **Step 3: Confirm review-before-save, then manually verify**

```bash
grep -n "sqlx::query\|INSERT\|UPDATE\|DELETE" crates/api/src/routes/train.rs
```

Expected: same result as Task 7's Step 4 — still only `post_ticket`/`post_track`.

```bash
curl -s -X POST http://localhost:8080/Train/1/tickets/pdf \
  -H "Cookie: nr_session=manual-test-token" -F "file=@sample-eticket.pdf"
psql "$DATABASE_URL" -c "SELECT count(*) FROM tracked_train_tickets"
```

(Reuse the tracked train + session from Task 7's manual verification, or re-create them first.) Expected: a `PartialTicket` preview back; `tracked_train_tickets` unchanged. If a real LNER/Trainline PDF e-ticket is available, use it here to sanity-check Task 8's regex/keyword heuristics against real formatting — not blocking for this plan if none is at hand. Clean up per Task 7's Step 4.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Add POST /Train/{trackingId}/tickets/pdf upload-and-preview route"
```

---

### Task 10: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Run the full Rust workspace test suite**

Run: `cargo test --workspace`
Expected: PASS, no regressions.

- [ ] **Step 2: Run `cargo clippy` across the workspace**

Run: `cargo clippy --workspace --all-targets`
Expected: no new warnings from this plan's changes.

- [ ] **Step 3: Re-confirm the structural safety properties by grep, not just by memory**

```bash
grep -rn "sqlx::query\|INSERT INTO\|UPDATE \|DELETE FROM" crates/api/src/data/delay_repay_rules.rs crates/api/src/data/ticket_extraction.rs
```

Expected: **no output at all** — neither file may contain any database call. This is the literal, checkable form of this plan's two central Global Constraints (never-auto-claim, review-before-save).

```bash
grep -rn "PgPool" crates/api/src/data/delay_repay_rules.rs
```

Expected: no output — `estimate_delay_repay`/`claim_url_for` take no database handle, so they structurally cannot become a write path even by accident.

- [ ] **Step 4: Bring up the full dev stack**

```bash
docker compose --env-file dev.env up --build -d
docker compose ps
```

Expected: every service healthy — unlike `docs/superpowers/plans/2026-08-28-train-tracking.md`'s own final task, this plan adds no new service with unmet external prerequisites, so there is no expected-unhealthy exception here.

- [ ] **Step 5: Manually verify the full ticket lifecycle end-to-end**

```bash
source dev.env
psql "$DATABASE_URL" -c "INSERT INTO users (id) VALUES ('TEST-USER') ON CONFLICT DO NOTHING"
psql "$DATABASE_URL" -c "INSERT INTO sessions (id, user_id, expires_at) VALUES ('$(echo -n manual-test-token | sha256sum | cut -d' ' -f1)', 'TEST-USER', NOW() + INTERVAL '1 hour')"
curl -s -X POST http://localhost:8080/Train/track \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"service_date":"2026-08-29","origin_crs":"KGX","scheduled_departure":"2026-08-29T18:32:00Z"}'
curl -s -X POST http://localhost:8080/Train/1/tickets \
  -H "Cookie: nr_session=manual-test-token" -H "Content-Type: application/json" \
  -d '{"operator":"LNER","ticket_type":"single","origin_crs":"KGX","destination_crs":"EDB","source":"manual"}'
psql "$DATABASE_URL" -c "INSERT INTO train_current_state (tracked_train_id, status, delay_minutes, updated_at) VALUES (1, 'en_route', 45, NOW()) ON CONFLICT (tracked_train_id) DO UPDATE SET delay_minutes = 45"
curl -s http://localhost:8080/Train/1/tickets -H "Cookie: nr_session=manual-test-token"
curl -s http://localhost:8080/Train/1/tickets/1/delay-repay -H "Cookie: nr_session=manual-test-token"
```

Expected: ticket creation succeeds; the list route shows the one ticket with `"source":"manual"`; the delay-repay route returns `{"delayMinutes":45,"estimate":{"scheme":"DR30","bandMinutes":30,"percentage":50,...},"claimUrl":"https://delayrepay.lner.co.uk/delayrepayV2/","disclaimer":"..."}` (LNER, 45 minutes → DR30's 30-minute band). Clean up: `psql "$DATABASE_URL" -c "DELETE FROM tracked_trains WHERE id = 1; DELETE FROM sessions WHERE user_id = 'TEST-USER'; DELETE FROM users WHERE id = 'TEST-USER'"`.

- [ ] **Step 6: Confirm no leftover uncommitted changes**

Run: `git status`
Expected: clean working tree.

---

## Follow-ups this plan intentionally leaves undone

Restated from this plan's own narrowing decisions above, collected in one place so they aren't lost:

- **Expanding `DR30_OPERATORS`/`CLAIM_URLS` (Task 4) beyond the three individually-verified operators.** Real, valuable work; requires per-operator verification against each TOC's own Passenger's Charter, not a bulk guess.
- **Resolving the design doc's Open Question 6** (whether operator identity should reuse `poller-tocs`' `TocReference`/ATOC-code catalogue instead of free-text substring matching). This plan's free-text approach is a deliberately narrower v1 slice.
- **Confirming the design doc's Open Question 1** (whether real UK retailers populate `.pkpass`'s `semantics` dictionary) against 1-2 real sample passes — flagged at Task 7's manual verification step, not blocking for this plan.
- **Confirming the design doc's Open Question 2** (PDF layout variability) against 1-2 real e-ticket PDFs — flagged at Task 9's manual verification step, not blocking for this plan.
- **Google Wallet support, frontend UI** — both explicitly out of scope per the design doc's own Non-goals; unresearched and undesigned here.
