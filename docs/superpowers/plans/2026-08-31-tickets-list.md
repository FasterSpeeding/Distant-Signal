# Tickets List Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a logged-in user a standalone page listing every ticket they've attached across every train they've tracked, most-recently-added first, with an inline Delay Repay estimate rendered on every row — closing the gap `docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md`'s own "Explicitly out of scope" section flagged and deferred: today the only way to see a ticket is to already know a specific train's tracking id or (uid, date) and open that train's own detail page, where `TicketPanel` renders it. There is no cross-train ticket query, no nav-bar entry, and no standalone page.

**Architecture:** One new backend query/route (`crates/api`) that computes each row's Delay Repay estimate inline during the same query pass (no per-ticket follow-up call), one required mechanical refactor to a currently-private disclaimer constant so both the existing per-ticket route and this new list route read identical text, and one new frontend page + nav item + supporting `lib/api.ts`/`lib/types.ts` additions that reuse existing, already-reviewed ticket/Delay-Repay rendering rather than reinventing it.

```
crates/api/src/data/delay_repay_rules.rs   + pub const ROUTE_DISCLAIMER
                                              (hoisted out of routes/train.rs)
crates/api/src/routes/train.rs             build_delay_repay_response now
                                              reads delay_repay_rules::ROUTE_DISCLAIMER
                                              + GET /Train/tickets/mine
crates/api/src/data/train_tracking.rs      + TicketListRow, TicketListItem,
                                              build_ticket_list_item,
                                              list_tickets_for_user,
                                              MINE_TICKETS_LIMIT
        │ server-side fetch (read, cookie-fwd, no-store)
        ▼
frontend/lib/types.ts                      + TicketListItem
frontend/lib/api.ts                        + getMyTickets
frontend/components/TicketSummary.tsx      NEW -- extracted out of
                                              TicketPanel.tsx, reused by both
frontend/components/DelayRepayEstimate.tsx UNCHANGED -- reused as-is
frontend/app/track/tickets/page.tsx        NEW -- login nudge / empty state /
                                              list, per-row link + inline
                                              <DelayRepayEstimate>
frontend/app/layout.tsx                    + MyTicketsNavItem (NEW, own
                                              Suspense, own guarded
                                              getSession())
```

**Tech Stack:** Rust (axum, sqlx, `PgPool`) for the backend tasks; Next.js App Router + TypeScript + Mantine v9, Vitest + `@testing-library/react` + this repo's `renderWithMantine` helper (`frontend/test/render.tsx`) for the frontend tasks.

**Spec:** `docs/superpowers/specs/2026-08-31-tickets-list-design.md` — read in full before starting; this plan does not restate its research, only carries its decisions into concrete tasks. Cross-references below to "Decision N" / "Finding N" refer to that document.

**Status note:** every prerequisite this plan depends on is already live, not merely planned — confirmed by direct inspection while writing this plan (2026-09-01). `tracked_train_tickets.user_id` is a direct, indexed (`tracked_train_tickets_user_id`) column, confirmed in `crates/api/src/data/train_tracking.rs`'s `create_ticket`/`list_tickets_for_tracked_train`/`get_ticket_owned` (all filter `WHERE ... user_id = $n` with no join). `crates/api/src/routes/train.rs::router()` mounts `/Train/track` as a literal segment above the dynamic `/Train/{tracking_id}` (Finding 8's precedent for adding `/Train/tickets/mine` the same way), and `router_builds_without_panicking` already exists to catch a route-table conflict at `cargo test` time. `DELAY_REPAY_ROUTE_DISCLAIMER` is confirmed still private to `routes/train.rs` (line 67, used at line 145) — the hoist this plan's Task 1 performs has not happened yet. `TicketSummary` is confirmed still a private, unexported function inside `TicketPanel.tsx` (line 98) — the extraction this plan's Task 5 performs has not happened yet. `DelayRepayEstimate.tsx` is confirmed already its own exported component taking `{ response: DelayRepayEstimateResponse }` with no fetch of its own — directly reusable, no changes needed. Re-grepped the whole tree for `Train/mine`, `list_tracked_trains_for_user`, and `track/mine`: still zero matches outside spec/plan doc files — `docs/superpowers/plans/2026-08-31-tracked-trains-list.md` has not been implemented, so this plan cannot build (and does not build) the `/track/mine` ticket-count touch point the spec documents as future work. No `frontend/app/layout.tsx` test file exists yet (`layout.test.tsx` or equivalent) — Task 7's nav-item test is new coverage, not an extension of an existing file.

## Global Constraints

- **No new database migration.** `tracked_train_tickets_user_id` already exists and covers this plan's new query's `WHERE t.user_id = $1` — do not add an index or a migration file.
- **Ownership check needs no join.** Per Finding 1, `tracked_train_tickets.user_id` is a direct column — `list_tickets_for_user`'s `WHERE t.user_id = $1` on `tracked_train_tickets` alone is the complete ownership filter. The joins to `tracked_trains`/`train_current_state` exist only to pull in train context for display (route, date, live delay) and to let `build_ticket_list_item` compute the Delay Repay estimate inline — not for ownership.
- **No per-row HTTP call for the Delay Repay estimate.** Per Finding 3/Decision 1, `estimate_delay_repay`/`claim_url_for` are pure functions needing only `operator`/`delay_minutes`, both already columns reachable in the one join. `build_ticket_list_item` computes every row's estimate in Rust, in the same request, zero additional DB round trips. Do not reuse `TicketPanel.tsx`'s per-ticket `Promise.all(...getDelayRepayEstimate...)` pattern here — that pattern is correct for `TicketPanel`'s own single-train scope, wrong for a cross-train list of up to 100 tickets.
- **The disclaimer hoist (Task 1) is a byte-identical move, not a rewrite.** `DELAY_REPAY_ROUTE_DISCLAIMER`'s text must not change by even a character when it becomes `delay_repay_rules::ROUTE_DISCLAIMER`. Both call sites (the existing per-ticket route's `build_delay_repay_response` and this plan's new `build_ticket_list_item`) must read the exact same constant — no second literal copy anywhere.
- **`TicketListItem`'s field list is fixed by the spec (Decision 1)** — implement exactly the fields shown in Task 2, in the order shown. The last four fields (`estimate`, `claimUrl`, `disclaimer`, plus `delayMinutes` already present) are deliberately shaped identically to `DelayRepayEstimateResponse` so a `TicketListItem` can be passed straight into `<DelayRepayEstimate response={...}>` with no adapter — do not rename or reshape these to "look more efficient."
- **Route is `GET /Train/tickets/mine`, a literal segment**, per Finding 8 — mounted alongside this router's other literals, safe against the existing dynamic `/Train/{tracking_id}/...` branch since no existing route has a literal `tickets` segment directly under `/Train/`. Do not mount it under the dynamic branch or give it a different path.
- **Route always returns `200` with a (possibly empty) array for any authenticated caller — never `404`.** There is no id in this route's path to be wrong about; the only two outcomes are 401 (no session, handled by the `AuthenticatedUser` extractor itself) and 200.
- **`MINE_TICKETS_LIMIT` (proposed `100`) with no pagination past it is inherited from the spec, not a re-decision point** — implement the cap as specified. Do not add pagination, "load more," or any filter/sort control (Decision 2) — a plain, single-order (`created_at DESC`) list is the whole of this plan's scope.
- **Frontend page path is fixed at `/track/tickets`**, per Decision 3 — a standalone page, not a section of `/track/mine` (which does not exist yet, per the Status note above) and not a `/track` tab. Do not touch `frontend/app/track/page.tsx` or `frontend/app/page.tsx` in this plan.
- **This page is read-only. No "add a ticket" affordance anywhere on it.** A ticket always needs a concrete `trackingId` in context, which a cross-train list doesn't supply one specific instance of — ticket creation stays reachable only from a specific tracked train's own detail page, unchanged by this plan.
- **`<DelayRepayEstimate>` is imported and used exactly as-is — no new props, no wrapper, no modifications to `frontend/components/DelayRepayEstimate.tsx`.** This is the literal reuse Finding 7 identifies: it is what makes the safety-critical rendering (full, verbatim disclaimer text; always present; never collapsed; the claim link's `target="_blank" rel="noopener noreferrer"` hygiene) automatically inherited rather than re-implemented. No task in this plan may touch that file.
- **`TicketSummary`'s extraction (Task 5) must not change its rendering.** `TicketPanel.tsx`'s existing tests must still pass unmodified after the extraction — this is a pure refactor (move + `export`), not new design.
- **Nav item is hidden entirely when logged out, not shown-with-a-prompt.** Per Decision 4/5, follow `AuthNavItem`/`DataFreshnessNavItem`'s exact shape in `frontend/app/layout.tsx`: own async Server Component, own `<Suspense fallback={null}>`, own `getSession().catch(() => ({ authenticated: false, id: null, email: null, name: null }))`-guarded call — never an unguarded `getSession()` call (the historical bug class already found and fixed once in `TicketPanel.tsx`).
- **The page itself does not call `getSession()`.** Per Decision 3/4, `getMyTickets()`'s own `null`-on-`401` return is the complete "not logged in" signal — there is no second party to disambiguate on a route with no id in its path, unlike `TicketPanel`'s owner-vs-not-owner distinction on a public page.
- **No new refresh mechanism.** The new page is an ordinary `no-store` Server Component read, covered by the existing global `AutoRefresh` (30s, mounted once in `app/layout.tsx`) — no per-route opt-out, no manual "check now" button.
- **Reads never go through the `/api/*` proxy.** `getMyTickets` is a server-only, cookie-forwarding read called from a Server Component, exactly like `getTicketsForTrackedTrain`/`getDelayRepayEstimate` — never called from a Client Component. This plan introduces no mutation and therefore needs no proxy change.
- **No backend changes outside `crates/api/src/data/delay_repay_rules.rs`, `crates/api/src/data/train_tracking.rs`, and `crates/api/src/routes/train.rs`.** No task may modify `crates/trust-consumer`, `crates/common`, or any migration file.
- **Out of scope, per the spec's own "Explicitly out of scope" section — no task may build any of these:** editing/deleting a saved ticket, any "add a ticket" entry point on this page, a per-row ticket-count indicator on `/track/mine` (that page doesn't exist yet — this is real future work belonging to whichever plan eventually implements it, not this one), any retention/pruning job for `tracked_train_tickets`, pagination/"load more" past `MINE_TICKETS_LIMIT`, filtering/search/sort controls, consolidating the repeated per-row disclaimer text into one page-level notice, fixing the `'completed'`-status gap in `crates/trust-consumer/src/journey.rs`, or any refresh mechanism faster than the existing global 30s `AutoRefresh`.
- **Open questions the spec deliberately left unresolved — no task in this plan should resolve them, only implement around them as specified:** `MINE_TICKETS_LIMIT`'s value (`100`, proposed for consistency with the trains-list spec's own `MINE_LIST_LIMIT`, not independently researched); the visual-weight tradeoff of up to 100 repeated full-text disclaimer blocks on one page; whether to eventually group tickets by their owning tracked train instead of a flat list; the exact future `/track/mine` ticket-count touch-point shape; whether the ticket's own `originCrs`/`destinationCrs` or the tracked train's pinned route is the more useful "route" to headline on a row (this plan implements the spec's own choice — the ticket's own route — as specified).
- **Testing convention:** Rust `#[cfg(test)]` modules colocated in the same file. Frontend: colocated `*.test.tsx`/`*.test.ts`, `@testing-library/react`, `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npm test` from `frontend/`). Every frontend task's verification step runs `npm test` and `npm run build` (both from `frontend/`) and requires both to pass with no new failures. Every backend task's verification step runs `cargo test -p api` and requires it to pass with no new failures.

---

### Task 1: Backend — hoist `DELAY_REPAY_ROUTE_DISCLAIMER` into `delay_repay_rules.rs` as `pub const ROUTE_DISCLAIMER`

**Files:**
- Modify: `crates/api/src/data/delay_repay_rules.rs`
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `pub const ROUTE_DISCLAIMER: &str` in `delay_repay_rules.rs`.
- Consumed by: `routes/train.rs`'s existing `build_delay_repay_response` (this task), and Task 2's new `build_ticket_list_item` (`train_tracking.rs`).

This is the required companion change Finding 4 identifies: `train_tracking.rs` (a `data` module) does not depend on `routes/train.rs` per this repo's existing layering, so it cannot reach a route-private const as-is. This task closes that gap mechanically, before Task 2 needs the constant. Byte-identical text — a move, not a wording change.

- [ ] **Step 1: Add `ROUTE_DISCLAIMER` to `delay_repay_rules.rs`**

In `crates/api/src/data/delay_repay_rules.rs`, add near the existing `DISCLAIMER` const (after it, so the two sibling constants sit together):

```rust
/// The route-level disclaimer rendered by every HTTP response that carries
/// a Delay Repay estimate -- textually DIFFERENT from `DISCLAIMER` above
/// (that one lives inside a non-null `DelayRepayEstimate.disclaimer`; this
/// one is the always-populated, top-level field on the response, present
/// even when `estimate` is `None`). Hoisted here (rather than staying
/// private to `routes/train.rs`) so both call sites that need it --
/// `routes/train.rs`'s `build_delay_repay_response` and
/// `train_tracking.rs`'s `build_ticket_list_item` -- read the exact same
/// string, closing a drift risk for a safety-critical, verbatim-required
/// piece of text (see `components/DelayRepayEstimate.tsx`'s own doc
/// comment: this string must render "in full, every time," never
/// paraphrased or shortened). Byte-identical to the const this replaced --
/// a mechanical move, not a wording change.
pub const ROUTE_DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a \
    guarantee of compensation and not proof you travelled. This app never submits a claim on your \
    behalf -- verify eligibility and claim directly from the operator using the link above.";
```

- [ ] **Step 2: Remove the private const from `routes/train.rs` and update its one call site**

In `crates/api/src/routes/train.rs`, delete:

```rust
const DELAY_REPAY_ROUTE_DISCLAIMER: &str = "This is a rough, community-sourced estimate, not a \
    guarantee of compensation and not proof you travelled. This app never submits a claim on your \
    behalf -- verify eligibility and claim directly from the operator using the link above.";
```

In `build_delay_repay_response`, change:

```rust
        disclaimer: DELAY_REPAY_ROUTE_DISCLAIMER,
```

to:

```rust
        disclaimer: delay_repay_rules::ROUTE_DISCLAIMER,
```

`delay_repay_rules` is already imported in this file's `use crate::data::{eta_blend, ticket_extraction, train_tracking, delay_repay_rules};` — no new import needed.

- [ ] **Step 3: Add a test asserting the two disclaimer constants stay distinct and non-empty**

Add to `crates/api/src/data/delay_repay_rules.rs`'s existing `#[cfg(test)] mod tests` block:

```rust
    #[test]
    fn route_disclaimer_is_distinct_from_the_per_estimate_disclaimer_and_non_empty() {
        // Two different strings by design -- see ROUTE_DISCLAIMER's own doc
        // comment and components/DelayRepayEstimate.tsx's doc comment for
        // why rendering both at once would read as inconsistent, not
        // doubly cautious.
        assert_ne!(ROUTE_DISCLAIMER, DISCLAIMER);
        assert!(!ROUTE_DISCLAIMER.is_empty());
    }
```

- [ ] **Step 4: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS, including the existing `dr30_operator_with_a_qualifying_delay_gets_a_specific_estimate_and_claim_url` and the other two `build_delay_repay_response` tests in `routes/train.rs` (unaffected — they assert `!response.disclaimer.is_empty()` or an operator-specific claim URL, not the literal disclaimer text itself, so the hoist doesn't touch their assertions).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/data/delay_repay_rules.rs crates/api/src/routes/train.rs
git commit -m "Hoist DELAY_REPAY_ROUTE_DISCLAIMER into delay_repay_rules as pub ROUTE_DISCLAIMER"
```

---

### Task 2: Backend — `TicketListItem` struct and `list_tickets_for_user` query

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`

**Interfaces:**
- Consumes: `delay_repay_rules::ROUTE_DISCLAIMER`, `delay_repay_rules::estimate_delay_repay`, `delay_repay_rules::claim_url_for`, `delay_repay_rules::GENERIC_CLAIM_URL` (all from Task 1 / already-existing).
- Produces: `pub struct TicketListItem` (public, `Serialize`, `camelCase` on the wire), `pub async fn list_tickets_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<Vec<TicketListItem>>`, `const MINE_TICKETS_LIMIT: i64`.
- Consumed by: Task 3 (`crates/api/src/routes/train.rs`'s new `GET /Train/tickets/mine` handler).

Depends on Task 1 (imports `delay_repay_rules::ROUTE_DISCLAIMER`).

- [ ] **Step 1: Add the `use` import for `delay_repay_rules`**

`train_tracking.rs` currently has no dependency on `delay_repay_rules`. Add to its existing import block at the top of the file:

```rust
use crate::data::delay_repay_rules;
```

(alongside the existing `use chrono::{DateTime, Utc};` / `use common::{...};` / `use serde::Serialize;` / `use sqlx::PgPool;` lines — matching `routes/train.rs`'s own `use crate::data::{..., delay_repay_rules};` style.)

- [ ] **Step 2: Add `MINE_TICKETS_LIMIT`, `TicketListRow`, and `TicketListItem`**

Add near the file's other constant (`MAX_PIN_AGE`) and after the existing `TrackedTrainTicket`/`TICKET_SELECT`/`list_tickets_for_tracked_train`/`get_ticket_owned` block (i.e. at the end of the file, alongside the ticket-family code it's the cross-train counterpart to — exact placement within the file is an implementation-time judgment call, not load-bearing):

```rust
/// Caps `list_tickets_for_user`'s response size. No retention/pruning job
/// exists anywhere in this codebase for `tracked_train_tickets` either
/// (grepped for `prune`/`retention`/`expire`/`DELETE FROM tracked_train_tickets`
/// -- only `ON DELETE CASCADE` and unrelated matches turned up), so this
/// table grows without bound for as long as a user keeps adding tickets,
/// and this cap is the only bound on one HTTP response. `100` matches
/// `MINE_LIST_LIMIT`'s proposed figure for the sibling tracked-trains list,
/// for consistency -- not independently researched or load-tested. See
/// docs/superpowers/specs/2026-08-31-tickets-list-design.md's Open
/// Question 1 (also: no pagination/"load more" is designed for what falls
/// past this cap).
const MINE_TICKETS_LIMIT: i64 = 100;

/// Physical columns selected by `list_tickets_for_user`'s query -- private,
/// exists only to satisfy `sqlx::FromRow`. `TicketListItem` (below) is the
/// public shape, built from this plus a pure computation -- same
/// two-struct pattern this file already uses for `TrackedTrainRow` /
/// `TrackedTrainRef`.
#[derive(Debug, Clone, sqlx::FromRow)]
struct TicketListRow {
    id: i64,
    tracked_train_id: i64,
    operator: Option<String>,
    ticket_type: Option<String>,
    origin_crs: Option<String>,
    destination_crs: Option<String>,
    source: String,
    created_at: DateTime<Utc>,
    service_date: chrono::NaiveDate,
    pin_origin_crs: String,
    pin_destination_crs: Option<String>,
    pin_scheduled_departure: DateTime<Utc>,
    resolution_status: String,
    train_uid: Option<String>,
    status: Option<String>,
    delay_minutes: Option<i32>,
}

/// A user's own tickets, across every tracked train they have -- the
/// cross-train counterpart to `TrackedTrainTicket` (which is scoped to one
/// tracked train). Carries the ticket's own six fields (unchanged from
/// `TrackedTrainTicket`) plus enough of the owning tracked train's context
/// (route, date, live delay) to make a row useful without clicking
/// through, plus a Delay Repay estimate computed inline -- see
/// `build_ticket_list_item` for why that's a pure computation, not a
/// second query per row. The last four fields are deliberately named and
/// shaped to match `DelayRepayEstimateResponse` exactly, field-for-field,
/// so the frontend can pass a `TicketListItem` straight into the
/// already-reviewed `<DelayRepayEstimate>` component with no adapter.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TicketListItem {
    pub id: i64,
    pub tracked_train_id: i64,
    pub operator: Option<String>,
    pub ticket_type: Option<String>,
    pub origin_crs: Option<String>,
    pub destination_crs: Option<String>,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub service_date: chrono::NaiveDate,
    pub pin_origin_crs: String,
    pub pin_destination_crs: Option<String>,
    pub pin_scheduled_departure: DateTime<Utc>,
    pub resolution_status: String,
    pub train_uid: Option<String>,
    pub status: Option<String>,
    pub delay_minutes: Option<i32>,
    pub estimate: Option<delay_repay_rules::DelayRepayEstimate>,
    pub claim_url: String,
    pub disclaimer: &'static str,
}
```

- [ ] **Step 3: Add `build_ticket_list_item`**

```rust
/// Mirrors `routes/train.rs`'s `build_delay_repay_response` exactly (same
/// `match (operator, delay_minutes)` shape), so the two independently
/// computed estimates for the same `(ticket, tracked train)` pair can
/// never disagree.
fn build_ticket_list_item(row: TicketListRow) -> TicketListItem {
    let estimate = match (row.operator.as_deref(), row.delay_minutes) {
        (Some(operator), Some(delay_minutes)) => delay_repay_rules::estimate_delay_repay(operator, delay_minutes),
        _ => None,
    };
    let claim_url = row.operator.as_deref().map(delay_repay_rules::claim_url_for).unwrap_or(delay_repay_rules::GENERIC_CLAIM_URL);

    TicketListItem {
        id: row.id,
        tracked_train_id: row.tracked_train_id,
        operator: row.operator,
        ticket_type: row.ticket_type,
        origin_crs: row.origin_crs,
        destination_crs: row.destination_crs,
        source: row.source,
        created_at: row.created_at,
        service_date: row.service_date,
        pin_origin_crs: row.pin_origin_crs,
        pin_destination_crs: row.pin_destination_crs,
        pin_scheduled_departure: row.pin_scheduled_departure,
        resolution_status: row.resolution_status,
        train_uid: row.train_uid,
        status: row.status,
        delay_minutes: row.delay_minutes,
        estimate,
        claim_url: claim_url.to_string(),
        disclaimer: delay_repay_rules::ROUTE_DISCLAIMER,
    }
}
```

- [ ] **Step 4: Add `list_tickets_for_user`**

```rust
/// A user's own tickets, across every tracked train they have,
/// most-recently-added first. No join needed for ownership (`WHERE
/// t.user_id = $1` on `tracked_train_tickets` alone, per this table's own
/// ownership-redundancy design -- Finding 1 of the design spec) -- the
/// joins to `tracked_trains`/`train_current_state` exist purely to pull in
/// enough train context for a useful row (route, date, live delay) and to
/// let `build_ticket_list_item` compute each row's Delay Repay estimate
/// inline, with no per-ticket follow-up query.
///
/// `JOIN` (not `LEFT JOIN`) to `tracked_trains`: every
/// `tracked_train_tickets` row has a `NOT NULL tracked_train_id REFERENCES
/// tracked_trains(id) ON DELETE CASCADE`, so a ticket can never outlive
/// (or predate) its parent row -- an inner join can't silently drop a
/// ticket here. `LEFT JOIN` to `train_current_state` matches every other
/// query in this file that reads it: a `pending`/just-resolved tracked
/// train legitimately has no `train_current_state` row yet.
pub async fn list_tickets_for_user(pool: &PgPool, user_id: &str) -> anyhow::Result<Vec<TicketListItem>> {
    let rows = sqlx::query_as::<_, TicketListRow>(
        "SELECT t.id, t.tracked_train_id, t.operator, t.ticket_type, t.origin_crs, t.destination_crs, \
                t.source, t.created_at, \
                tt.service_date, tt.pin_origin_crs, tt.pin_destination_crs, tt.pin_scheduled_departure, \
                tt.resolution_status, tt.train_uid, \
                cs.status, cs.delay_minutes \
         FROM tracked_train_tickets t \
         JOIN tracked_trains tt ON tt.id = t.tracked_train_id \
         LEFT JOIN train_current_state cs ON cs.tracked_train_id = tt.id \
         WHERE t.user_id = $1 \
         ORDER BY t.created_at DESC \
         LIMIT $2",
    )
    .bind(user_id)
    .bind(MINE_TICKETS_LIMIT)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(build_ticket_list_item).collect())
}
```

- [ ] **Step 5: Add a unit test for `build_ticket_list_item`**

Add a new `#[cfg(test)] mod ticket_list_tests` block (or extend an existing one in this file — the file already has `ticket_entry_tests` and a bare `tests` module; either is fine, keep it colocated per this file's convention):

```rust
#[cfg(test)]
mod ticket_list_tests {
    use super::*;

    fn row(operator: Option<&str>, delay_minutes: Option<i32>) -> TicketListRow {
        TicketListRow {
            id: 1,
            tracked_train_id: 1,
            operator: operator.map(str::to_string),
            ticket_type: Some("Off-Peak Day Single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "manual".to_string(),
            created_at: "2026-08-29T12:00:00Z".parse().unwrap(),
            service_date: "2026-08-29".parse().unwrap(),
            pin_origin_crs: "KGX".to_string(),
            pin_destination_crs: Some("EDB".to_string()),
            pin_scheduled_departure: "2026-08-29T09:00:00Z".parse().unwrap(),
            resolution_status: "resolved".to_string(),
            train_uid: Some("A12345".to_string()),
            status: Some("late".to_string()),
            delay_minutes,
        }
    }

    // Regression check that this mirrored implementation hasn't drifted
    // from routes/train.rs's build_delay_repay_response for the same
    // (operator, delay_minutes) pair -- see this function's own doc
    // comment for why the two must never disagree.
    #[test]
    fn matches_build_delay_repay_response_for_a_qualifying_dr30_delay() {
        let item = build_ticket_list_item(row(Some("LNER"), Some(45)));

        let estimate = item.estimate.expect("LNER + 45 minutes should clear the DR30 30-minute band");
        assert_eq!(estimate.scheme, "DR30");
        assert_eq!(estimate.percentage, 50);
        assert_eq!(item.claim_url, "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert_eq!(item.delay_minutes, Some(45));
    }

    #[test]
    fn no_operator_yields_no_estimate_but_still_a_real_claim_link_and_disclaimer() {
        let item = build_ticket_list_item(row(None, Some(45)));

        assert_eq!(item.estimate, None);
        assert_eq!(item.claim_url, delay_repay_rules::GENERIC_CLAIM_URL);
        assert_eq!(item.disclaimer, delay_repay_rules::ROUTE_DISCLAIMER);
    }

    #[test]
    fn no_delay_data_yields_no_estimate_but_claim_url_and_disclaimer_are_still_populated() {
        let item = build_ticket_list_item(row(Some("LNER"), None));

        assert_eq!(item.estimate, None);
        assert_eq!(item.delay_minutes, None);
        assert_eq!(item.claim_url, "https://delayrepay.lner.co.uk/delayrepayV2/");
        assert_eq!(item.disclaimer, delay_repay_rules::ROUTE_DISCLAIMER);
    }
}
```

`list_tickets_for_user` itself follows this file's existing convention of leaving query functions untested at the unit level — no other query function in this file has one either (confirm by checking `list_tickets_for_tracked_train`/`get_ticket_owned`/`create_pin` etc. before deciding otherwise; if this repo has since grown an integration-test harness for query functions, e.g. `#[sqlx::test]`, that's a candidate to add here too, but is not required by this task).

- [ ] **Step 6: Compile-check**

Run (from repo root): `cargo check -p api`
Expected: PASS, no warnings about unused struct/function (both are consumed in Task 3, which should be done in the same work session or immediately after — if done strictly separately, expect a transient `dead_code` warning until Task 3 lands).

- [ ] **Step 7: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS, including the three new `ticket_list_tests` and every existing test in this file/crate.

- [ ] **Step 8: Commit**

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Add TicketListItem and list_tickets_for_user query"
```

---

### Task 3: Backend — `GET /Train/tickets/mine` route

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Produces: `.route("/Train/tickets/mine", axum::routing::get(get_my_tickets))` mounted in `router()`; `async fn get_my_tickets(...) -> Result<Json<Vec<train_tracking::TicketListItem>>, (StatusCode, String)>`.
- Consumes: `train_tracking::list_tickets_for_user` (Task 2).
- Consumed by: Task 4 (`frontend/lib/api.ts`'s `getMyTickets`, functionally — at end-to-end runtime, not at compile time).

Depends on Task 2 being complete (imports `train_tracking::TicketListItem`/`list_tickets_for_user`).

- [ ] **Step 1: Add the route to `router()`**

Add `/Train/tickets/mine` as a literal segment, per Finding 8. This router has no existing literal `tickets` segment directly under `/Train/` (every existing `tickets`-shaped route hangs off the dynamic `/Train/{tracking_id}` branch), so this introduces a new, non-conflicting branch — add it directly after the existing literal `/Train/track`, before the dynamic segment, matching this file's existing convention of listing literals first:

```rust
pub fn router() -> Router {
    Router::new()
        .route("/Train/track", axum::routing::post(post_track))
        .route("/Train/tickets/mine", axum::routing::get(get_my_tickets))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id))
        .route("/Train/by-uid/{train_uid}/{date}", axum::routing::get(get_by_uid_and_date))
        .route("/Train/{tracking_id}/tickets", axum::routing::post(post_ticket).get(get_tickets))
        .route("/Train/{tracking_id}/tickets/{ticket_id}/delay-repay", axum::routing::get(get_delay_repay_estimate))
        .route("/Train/{tracking_id}/tickets/pkpass", axum::routing::post(post_pkpass_upload))
        .route("/Train/{tracking_id}/tickets/pdf", axum::routing::post(post_pdf_upload))
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
}
```

- [ ] **Step 2: Add the handler**

Add near the other simple, single-purpose handlers (e.g. directly after `post_track`):

```rust
/// Always `200` with a (possibly empty) array for any authenticated
/// caller -- never `404`, matching `GET /Train/mine`'s own two-outcome
/// shape (if that route has landed by the time this is implemented) more
/// closely than the per-ticket routes' three-outcome ("exists but not
/// yours" -> 404) shape. There's no id in this route's path to be wrong
/// about: the only two real outcomes are "logged in, here's your list" and
/// "not logged in, bare 401" (handled by the `AuthenticatedUser` extractor
/// itself, before this function runs).
async fn get_my_tickets(
    State(app): State<App>,
    user: AuthenticatedUser,
) -> Result<Json<Vec<train_tracking::TicketListItem>>, (StatusCode, String)> {
    let tickets = train_tracking::list_tickets_for_user(&app.database, &user.id)
        .await
        .map_err(internal_error("list tickets"))?;
    Ok(Json(tickets))
}
```

Reuses the existing `internal_error` helper already defined at the bottom of this file — no new error-mapping helper needed.

- [ ] **Step 3: Run the crate's test suite**

Run (from repo root): `cargo test -p api`
Expected: PASS. `router_builds_without_panicking` (unmodified, but now exercising the widened `router()`) is the actual regression check for Finding 8's literal-segment concern — a route-table conflict would panic this test at construction time, not silently misroute at runtime.

- [ ] **Step 4: Run the full backend build**

Run (from repo root): `cargo build --workspace`
Expected: PASS, no new warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Add GET /Train/tickets/mine, session-gated via AuthenticatedUser"
```

---

### Task 4: Frontend — `lib/types.ts` and `lib/api.ts` additions

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Produces: `TicketListItem` (type), `getMyTickets(): Promise<TicketListItem[] | null>`.
- Consumed by: Task 6 (`app/track/tickets/page.tsx`).

`TicketSource`, `DelayRepayEstimate`, `ResolutionStatus`, `JourneyStatus` are already defined in `frontend/lib/types.ts` — reused verbatim here, no new enum/union types needed.

- [ ] **Step 1: Add the type**

Add to `frontend/lib/types.ts`, after `DelayRepayEstimateResponse`:

```ts
/** `GET /Train/tickets/mine`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TicketListItem`, camelCase).
 * The last four fields are deliberately shaped identically to
 * `DelayRepayEstimateResponse` so a `TicketListItem` can be passed
 * straight into `<DelayRepayEstimate>` with no adapter -- see
 * docs/superpowers/specs/2026-08-31-tickets-list-design.md's Finding 7 /
 * Decision 1. */
export interface TicketListItem {
  id: number;
  trackedTrainId: number;
  operator: string | null;
  ticketType: string | null;
  originCrs: string | null;
  destinationCrs: string | null;
  source: TicketSource;
  createdAt: string; // RFC3339 -- list ordering key
  serviceDate: string; // "YYYY-MM-DD"
  pinOriginCrs: string;
  pinDestinationCrs: string | null;
  pinScheduledDeparture: string; // RFC3339
  resolutionStatus: ResolutionStatus;
  trainUid: string | null;
  status: JourneyStatus | null;
  delayMinutes: number | null;
  estimate: DelayRepayEstimate | null;
  claimUrl: string;
  disclaimer: string;
}
```

- [ ] **Step 2: Add `getMyTickets`**

Add `TicketListItem` to `frontend/lib/api.ts`'s existing `import type { ... } from './types';` list, then add after `getDelayRepayEstimate`:

```ts
/** `GET /Train/tickets/mine`. Returns `null` on `401` (not logged in) --
 * deliberately not `ApiNotFoundError`, matching `getTicketsForTrackedTrain`'s
 * precedent of treating "no session" as an expected, first-class outcome.
 * Unlike that function, there is no second, distinct 404-shaped outcome to
 * also collapse into `null` here -- no id in this route's path to be
 * wrong about, so a 401 from this one call is the complete signal (same
 * reasoning as `getTrackedTrainById`'s sibling list route, `getMyTrackedTrains`,
 * if that has landed). `app/track/tickets/page.tsx` does NOT need a
 * separate `getSession()` call the way `TicketPanel` does. */
export async function getMyTickets(): Promise<TicketListItem[] | null> {
  const url = `${baseUrl()}/Train/tickets/mine`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401) {
    return null;
  }
  if (!response.ok) {
    throw errorForResponse(url, response);
  }
  return response.json() as Promise<TicketListItem[]>;
}
```

- [ ] **Step 3: Add tests**

Add `getMyTickets` to `frontend/lib/api.test.ts`'s existing import list from `./api`, then add (copying the cookie-stubbing pattern the file's existing `getTicketsForTrackedTrain`/`getDelayRepayEstimate` tests already use — `incomingCookies`, `vi.stubGlobal('fetch', ...)`):

```ts
it('getMyTickets fetches the correct URL, forwarding cookies, with no caching', async () => {
  incomingCookies.header = 'distant_signal_session=abc123';
  vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
  await getMyTickets();
  expect(fetch).toHaveBeenCalledWith(
    'http://test-api:8080/Train/tickets/mine',
    expect.objectContaining({
      cache: 'no-store',
      headers: { Cookie: 'distant_signal_session=abc123' },
    }),
  );
});

it('getMyTickets returns null on a 401 (not logged in)', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('no session', { status: 401 })));
  await expect(getMyTickets()).resolves.toBeNull();
});

it('getMyTickets resolves an empty array as logged-in-with-no-tickets, not null', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));
  await expect(getMyTickets()).resolves.toEqual([]);
});

it('getMyTickets still throws on a non-401 failure', async () => {
  vi.stubGlobal('fetch', vi.fn(async () => new Response('server error', { status: 500 })));
  await expect(getMyTickets()).rejects.toThrow(/500/);
});
```

(Verify the exact base URL string used elsewhere in this test file — e.g. `http://test-api:8080` above is illustrative, copy whatever the existing `getTicketsForTrackedTrain`/`getDelayRepayEstimate` tests in this same file actually assert against, since it must match the test environment's `API_BASE_URL` setup exactly.)

- [ ] **Step 4: Run the test suite**

Run (from `frontend/`): `npm test -- api.test.ts`
Expected: all tests, including the four new ones, PASS.

- [ ] **Step 5: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
git commit -m "Add TicketListItem type and getMyTickets read function"
```

---

### Task 5: Frontend — extract `TicketSummary` out of `TicketPanel.tsx`

**Files:**
- Create: `frontend/components/TicketSummary.tsx`
- Modify: `frontend/components/TicketPanel.tsx`

**Interfaces:**
- Produces: `export function TicketSummary({ ticket }: { ticket: Pick<TicketListItem | TrackedTrainTicket, 'operator' | 'ticketType' | 'originCrs' | 'destinationCrs'> })` — an exported component, reusable by both `TicketPanel.tsx` and Task 6's new page.
- Consumed by: `TicketPanel.tsx` (this task, unchanged rendering), Task 6 (`app/track/tickets/page.tsx`'s `TicketListRow`).

This is the mechanical refactor Finding 6 identifies: `TicketSummary` is currently a private, unexported function inside `TicketPanel.tsx` (confirmed at line 98, `function TicketSummary({ ticket }: { ticket: TrackedTrainTicket }) { ... }`), so it can't be reused by a new page as-is. No rendering behavior changes — only its location and export visibility.

- [ ] **Step 1: Create `frontend/components/TicketSummary.tsx`**

Move the existing function verbatim, widening its prop type so it accepts either `TrackedTrainTicket` (its current caller) or `TicketListItem` (Task 6's new caller) — both share the four fields this component actually reads:

```tsx
import { Stack, Text } from '@mantine/core';
import type { TrackedTrainTicket, TicketListItem } from '@/lib/types';

/** The "operator — ticket type" / "origin → destination" row renderer,
 * shared by `TicketPanel.tsx` (one tracked train's own tickets) and
 * `app/track/tickets/page.tsx` (a cross-train ticket list) -- extracted
 * out of `TicketPanel.tsx`, where it was previously a private,
 * unexported function, so both can reuse it rather than duplicating
 * ticket-row rendering. `Pick<...>` keeps the prop narrow: this component
 * only ever reads these four fields, from either wire shape. */
export function TicketSummary({
  ticket,
}: {
  ticket: Pick<TrackedTrainTicket | TicketListItem, 'operator' | 'ticketType' | 'originCrs' | 'destinationCrs'>;
}) {
  const route =
    ticket.originCrs || ticket.destinationCrs ? `${ticket.originCrs ?? '?'} → ${ticket.destinationCrs ?? '?'}` : null;
  return (
    <Stack gap={2}>
      <Text fw={500}>
        {ticket.operator ?? 'Ticket'}
        {ticket.ticketType ? ` — ${ticket.ticketType}` : ''}
      </Text>
      {route && <Text size="sm">{route}</Text>}
    </Stack>
  );
}
```

- [ ] **Step 2: Update `TicketPanel.tsx` to import it instead of defining it locally**

In `frontend/components/TicketPanel.tsx`, remove the local `function TicketSummary({ ticket }: { ticket: TrackedTrainTicket }) { ... }` definition (lines 98-110), and add to the top-of-file imports:

```tsx
import { TicketSummary } from './TicketSummary';
```

The existing `<TicketSummary ticket={ticket} />` call site inside `TicketPanel`'s render (currently line 89) needs no change — same component name, same prop shape, just imported instead of locally defined.

- [ ] **Step 3: Run `TicketPanel.tsx`'s existing tests unmodified**

Run (from `frontend/`): `npm test -- TicketPanel.test.tsx`
Expected: all existing tests in `frontend/components/TicketPanel.test.tsx` PASS with zero changes to that test file — this is the direct regression check that the extraction changed nothing about what `TicketPanel` renders (per this plan's Global Constraints).

- [ ] **Step 4: Write a render test for the extracted `TicketSummary` component**

Create `frontend/components/TicketSummary.test.tsx`:

```tsx
import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TicketSummary } from './TicketSummary';

describe('TicketSummary', () => {
  it('renders operator and ticket type', () => {
    renderWithMantine(
      <TicketSummary ticket={{ operator: 'LNER', ticketType: 'Off-Peak Day Single', originCrs: null, destinationCrs: null }} />,
    );
    expect(screen.getByText('LNER — Off-Peak Day Single')).toBeInTheDocument();
  });

  it('falls back to "Ticket" when operator is null', () => {
    renderWithMantine(
      <TicketSummary ticket={{ operator: null, ticketType: null, originCrs: null, destinationCrs: null }} />,
    );
    expect(screen.getByText('Ticket')).toBeInTheDocument();
  });

  it('renders the route when either origin or destination is present', () => {
    renderWithMantine(
      <TicketSummary ticket={{ operator: 'LNER', ticketType: null, originCrs: 'KGX', destinationCrs: 'EDB' }} />,
    );
    expect(screen.getByText('KGX → EDB')).toBeInTheDocument();
  });

  it('renders no route line when both origin and destination are null', () => {
    renderWithMantine(
      <TicketSummary ticket={{ operator: 'LNER', ticketType: null, originCrs: null, destinationCrs: null }} />,
    );
    expect(screen.queryByText(/→/)).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 5: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/TicketSummary.tsx frontend/components/TicketSummary.test.tsx frontend/components/TicketPanel.tsx
git commit -m "Extract TicketSummary out of TicketPanel.tsx into its own exported component"
```

---

### Task 6: Frontend — `/track/tickets` page

**Files:**
- Create: `frontend/app/track/tickets/page.tsx`
- Create: `frontend/app/track/tickets/page.test.tsx`

**Interfaces:**
- Consumes: `getMyTickets` (Task 4), `TicketSummary` (Task 5), `DelayRepayEstimate` (existing, unmodified).
- Produces: default-exported async Server Component rendering the three outcomes — login nudge (`null`), empty state (`[]`), populated list.
- Consumed by: Task 7 (`MyTicketsNavItem` links here).

Depends on Task 4 and Task 5 being complete.

Per Decision 3, the row link logic mirrors the trains-list spec's own precedent: `resolutionStatus === 'resolved' && trainUid` links to the canonical `/train/{trainUid}/{serviceDate}`; otherwise (`pending`/`unresolved`, or a defensive `resolved`-with-null-`trainUid`) links to `/train/by-id/{trackedTrainId}`.

- [ ] **Step 1: Write the page**

Create `frontend/app/track/tickets/page.tsx`:

```tsx
import { Divider, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyTickets } from '@/lib/api';
import { TextLink } from '@/components/TextLink';
import { TicketSummary } from '@/components/TicketSummary';
import { DelayRepayEstimate } from '@/components/DelayRepayEstimate';
import { formatDate, formatTime } from '@/lib/dateFormat';
import type { TicketListItem } from '@/lib/types';

/** `/track/tickets` -- every ticket a logged-in user has attached, across
 * every train they've tracked, most-recently-added first, per
 * docs/superpowers/specs/2026-08-31-tickets-list-design.md Decision 3.
 * Standalone rather than a section of `/track/mine`: that page does not
 * exist yet (see this plan's Status note), and per Decision 3 a
 * ticket-focused view answers a different question ("which of these is
 * worth an actual Delay Repay claim right now") than a train-focused one.
 * `getMyTickets()` returning `null` on a `401` is the COMPLETE "not logged
 * in" signal for this page -- there is no second party to disambiguate
 * (no id in this route's path that could belong to someone else), so no
 * separate `getSession()` call is needed here the way `TicketPanel` needs
 * one. This page has no "add a ticket" affordance of its own -- ticket
 * creation always needs a concrete `trackingId` in context, which this
 * cross-train list doesn't supply one specific instance of; the empty
 * state below links to `/track` (not `/track/mine`, which doesn't exist
 * yet) as the only entry point that exists today. */
export default async function MyTicketsPage() {
  const tickets = await getMyTickets();

  if (tickets === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>My Tickets</Title>
        <TextLink href="/api/auth/login" underline="always">
          Log in to see your tickets
        </TextLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>My Tickets</Title>
      {tickets.length === 0 ? (
        <Text c="dimmed">
          You haven&apos;t added any tickets yet. Track a train, then attach a ticket to it from that
          train&apos;s own page. <Link href="/track">Track a train</Link> to get started.
        </Text>
      ) : (
        <Stack gap="lg">
          {tickets.map((ticket, index) => (
            <Stack key={ticket.id} gap="xs">
              {index > 0 && <Divider />}
              <TicketListRow ticket={ticket} />
            </Stack>
          ))}
        </Stack>
      )}
    </Stack>
  );
}

function TicketListRow({ ticket }: { ticket: TicketListItem }) {
  // Canonical, shareable URL once resolved; the by-id detail route
  // otherwise -- same logic as the sibling tracked-trains list's own row
  // link, applied to a ticket's owning tracked train. The
  // `resolved`-with-null-`trainUid` fallback is defensive: the backend's
  // own resolution invariant means this shouldn't happen, but this
  // component doesn't assume it.
  const href =
    ticket.resolutionStatus === 'resolved' && ticket.trainUid
      ? `/train/${ticket.trainUid}/${ticket.serviceDate}`
      : `/train/by-id/${ticket.trackedTrainId}`;

  return (
    <Stack gap="xs">
      <Group justify="space-between" wrap="nowrap">
        <TicketSummary ticket={ticket} />
        <Link href={href}>
          {formatDate(ticket.serviceDate)} · {formatTime(ticket.pinScheduledDeparture)}
        </Link>
      </Group>
      {/* Imported and used exactly as-is, no new props, no wrapper -- this
          is the literal reuse the design spec's Finding 7 identifies, and
          it's what makes the safety-critical disclaimer rendering
          automatically inherited rather than re-implemented on this page.
          This page adds no new Delay Repay rendering logic of its own. */}
      <DelayRepayEstimate
        response={{
          delayMinutes: ticket.delayMinutes,
          estimate: ticket.estimate,
          claimUrl: ticket.claimUrl,
          disclaimer: ticket.disclaimer,
        }}
      />
    </Stack>
  );
}
```

**Implementation-time verification note:** confirm `formatDate`/`formatTime` (`frontend/lib/dateFormat.ts`) exist with these exact signatures — both are already used elsewhere (e.g. `TrainJourney.tsx`/`EtaBadge.tsx`), so this should be a straight reuse; adjust the calls if the actual signatures differ.

- [ ] **Step 2: Write the tests**

Create `frontend/app/track/tickets/page.test.tsx`, mocking `@/lib/api` and calling the async page function directly (the same "await the async Server Component, then render" technique `TicketPanel.test.tsx` established):

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import MyTicketsPage from './page';
import * as api from '@/lib/api';
import type { TicketListItem } from '@/lib/types';

vi.mock('@/lib/api');

function item(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: 'Off-Peak Day Single',
    originCrs: 'KGX',
    destinationCrs: 'EDB',
    source: 'manual',
    createdAt: '2026-08-31T12:00:00Z',
    serviceDate: '2026-08-31',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'EDB',
    pinScheduledDeparture: '2026-08-31T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'A12345',
    status: 'en_route',
    delayMinutes: 45,
    estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'x' },
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: 'This is a rough, community-sourced estimate...',
    ...overrides,
  };
}

describe('MyTicketsPage', () => {
  it('null (not logged in): shows a login nudge', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue(null);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByRole('link', { name: 'Log in to see your tickets' })).toHaveAttribute(
      'href',
      '/api/auth/login',
    );
  });

  it('empty array: shows the empty state with a working link to /track', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByText(/haven't added any tickets yet/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Track a train' })).toHaveAttribute('href', '/track');
  });

  it('resolved ticket with a trainUid: links to the canonical /train/{uid}/{date} URL', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([item()]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByRole('link', { name: /31 Aug 2026/ })).toHaveAttribute('href', '/train/A12345/2026-08-31');
  });

  it('pending ticket: links to the by-id detail route', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([
      item({ resolutionStatus: 'pending', trainUid: null, status: null, delayMinutes: null, estimate: null }),
    ]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByRole('link', { name: /31 Aug 2026/ })).toHaveAttribute('href', '/train/by-id/1');
  });

  it('every row renders its ticket summary and a DelayRepayEstimate block with the verbatim disclaimer', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([item()]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getByText(/LNER/)).toBeInTheDocument();
    expect(screen.getByText(/KGX → EDB/)).toBeInTheDocument();
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
    expect(screen.getByText('This is a rough, community-sourced estimate...')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /See how to claim from the operator/ })).toHaveAttribute(
      'href',
      'https://delayrepay.lner.co.uk/delayrepayV2/',
    );
  });

  it('multiple tickets: renders one DelayRepayEstimate block per row, not just the first', async () => {
    vi.mocked(api.getMyTickets).mockResolvedValue([
      item({ id: 1, operator: 'LNER' }),
      item({ id: 2, operator: 'CrossCountry', claimUrl: 'https://delayrepay.crosscountrytrains.co.uk/' }),
    ]);
    renderWithMantine(await MyTicketsPage());
    expect(screen.getAllByRole('link', { name: /See how to claim from the operator/ })).toHaveLength(2);
  });
});
```

(Adjust the exact `formatDate`/`getByRole('link', { name: /.../ })` text-matching once Step 1's `formatDate`/`formatTime` signatures are confirmed — the illustrative `/31 Aug 2026/` pattern above should be replaced with whatever this repo's actual date formatting produces, matching the convention `TrackedTrainListRow`'s own tests use if the sibling trains-list plan has landed.)

- [ ] **Step 3: Run the tests**

Run (from `frontend/`): `npm test -- track/tickets/page.test.tsx`
Expected: all six tests PASS.

- [ ] **Step 4: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add frontend/app/track/tickets/page.tsx frontend/app/track/tickets/page.test.tsx
git commit -m "Add /track/tickets page listing a user's own tickets with inline Delay Repay estimates"
```

---

### Task 7: Frontend — session-gated nav item

**Files:**
- Modify: `frontend/app/layout.tsx`

**Interfaces:**
- Produces: `MyTicketsNavItem` (new async Server Component, local to `layout.tsx`, matching `AuthNavItem`/`DataFreshnessNavItem`'s existing shape), wrapped in `<Suspense fallback={null}>`, placed in the nav `Group` next to the existing `<TextLink href="/track">Track a Train</TextLink>`.

Depends on Task 6 being complete (links to `/track/tickets`, which must exist for the link to be meaningful — though this task would still compile/test fine on its own if done first, since it's a plain `<TextLink>` with a hardcoded href).

Per Decision 5: hidden entirely (returns `null`) when logged out, not shown-with-a-login-prompt — a deliberate difference from `TicketPanel`'s in-page degrade pattern, because this is a full nav-bar entry point to a page whose entire content is private to the viewer, not a section of an already-public page.

- [ ] **Step 1: Re-read the current `AuthNavItem`/`DataFreshnessNavItem` block to confirm the exact surrounding structure**

Re-read `frontend/app/layout.tsx` (confirmed present as of 2026-09-01 at lines 26-55 for the two existing nav-item functions, lines 98-109 for where they're mounted in the nav `Group`) to get the precise `Group`/`Suspense` nesting and the `getSession()` guard shape to mirror exactly.

- [ ] **Step 2: Add `MyTicketsNavItem`**

Add a new function alongside `AuthNavItem`/`DataFreshnessNavItem` (same file, same pattern):

```tsx
// Same rationale as AuthNavItem/DataFreshnessNavItem: a separate async
// Server Component so <Suspense> can stream the session check in without
// blocking the rest of the shell. Renders nothing at all when logged out
// (Decision 5 of docs/superpowers/specs/2026-08-31-tickets-list-design.md)
// -- this is a full nav-bar entry point to a page whose entire content is
// private to the viewer, not a section of an already-public page (the
// TicketPanel pattern), so showing it to every visitor and having it
// always resolve to a login nudge would be dead weight for the common
// case of an anonymous visitor. Guarded with the same .catch() shape as
// AuthNavItem/DataFreshnessNavItem: a root layout has no route-level
// error.tsx, so an unguarded getSession() here could take down every
// page's nav bar on an auth glitch -- the same historical bug class
// already fixed once in TicketPanel.tsx, not repeated here.
async function MyTicketsNavItem() {
  const session = await getSession().catch(() => ({
    authenticated: false,
    id: null,
    email: null,
    name: null,
  }));
  if (!session.authenticated) {
    return null;
  }
  return <TextLink href="/track/tickets">My Tickets</TextLink>;
}
```

- [ ] **Step 3: Mount it in the nav `Group`**

Add, next to the existing `<TextLink href="/track">Track a Train</TextLink>`:

```tsx
<Suspense fallback={null}>
  <MyTicketsNavItem />
</Suspense>
```

No skeleton fallback (unlike `DataFreshnessNavItem`'s icon or `AuthNavItem`'s "Log in" text) — there's no harmless placeholder for a link whose very presence depends on the still-pending fetch; render nothing until it resolves rather than flash a link that might immediately disappear, per Decision 5. Relative ordering against `TrackedTrainsNavItem` (if the sibling trains-list plan's own nav item has also landed by the time this is implemented) is an implementation-time call, not load-bearing.

This is a **second** `getSession()` call on every page load (on top of `AuthNavItem`'s existing one) — accepted as harmless per Decision 5: Next.js's per-request `fetch` deduplication means identical `fetch` calls within one render pass share a single underlying network request. No new caching mechanism is built to avoid this.

- [ ] **Step 4: Write a render test for session-conditional nav rendering**

No `layout.test.tsx` or equivalent exists yet in this repo (confirmed by search while writing this plan) — this is new test surface, not an extension of an existing file. Create `frontend/app/layout.test.tsx`, using the same "await the async function directly, then render" technique used elsewhere in this plan (Task 6) and in `TicketPanel.test.tsx`:

```tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import * as api from '@/lib/api';

vi.mock('@/lib/api');

// AuthNavItem/DataFreshnessNavItem stay local/unexported, matching their
// existing convention -- but rendering through the default RootLayout
// export isn't a viable way to test MyTicketsNavItem in isolation
// (RootLayout is synchronous and doesn't await its own Suspense children
// in a unit-render, so a mocked getSession() rejection/resolution can't be
// observed that way). Simplest robust option: export `MyTicketsNavItem`
// from layout.tsx (adding `export` to its function declaration, Step 2
// above) purely for this test's `import` statement to reach it directly --
// mirroring how this plan's other async Server Component tests (Task 6,
// TicketPanel.test.tsx) call their exported target directly rather than
// rendering through a parent.
import { MyTicketsNavItem } from './layout';

describe('MyTicketsNavItem', () => {
  it('hides "My Tickets" when logged out', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    renderWithMantine(await MyTicketsNavItem());
    expect(screen.queryByRole('link', { name: 'My Tickets' })).not.toBeInTheDocument();
  });

  it('shows "My Tickets", pointing at /track/tickets, when logged in', async () => {
    vi.mocked(api.getSession).mockResolvedValue({ authenticated: true, id: 'user-1', email: null, name: null });
    renderWithMantine(await MyTicketsNavItem());
    expect(screen.getByRole('link', { name: 'My Tickets' })).toHaveAttribute('href', '/track/tickets');
  });

  it('degrades to hidden (not a thrown error) when getSession rejects', async () => {
    vi.mocked(api.getSession).mockRejectedValue(new Error('auth service unreachable'));
    renderWithMantine(await MyTicketsNavItem());
    expect(screen.queryByRole('link', { name: 'My Tickets' })).not.toBeInTheDocument();
  });
});
```

Add `export` to `MyTicketsNavItem`'s function declaration in Step 2 so this test file can import it directly (`AuthNavItem`/`DataFreshnessNavItem` themselves stay unexported/untested by this plan — only the function this plan adds needs to become reachable for its own new test).

- [ ] **Step 5: Run the tests**

Run (from `frontend/`): `npm test -- layout.test.tsx`
Expected: all three tests PASS.

- [ ] **Step 6: Run the full frontend test suite and build**

Run (from `frontend/`): `npm test && npm run build`
Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Add session-gated 'My Tickets' nav item"
```

---

## Sequencing notes

- Tasks 1 → 2 → 3 are backend-only and must run in that order: Task 2 imports Task 1's `ROUTE_DISCLAIMER`; Task 3 imports Task 2's `TicketListItem`/`list_tickets_for_user`.
- Task 4 (frontend types/api) can start once Task 3's route shape is known — it does not require Tasks 1-3 to actually be merged to write against, since the wire contract is already fully specified in the design spec, but the honest end-to-end path (a working `GET /Train/tickets/mine` to hit) requires Tasks 1-3 done first if anyone wants to manually verify Task 4 against a live backend rather than mocked `fetch`.
- Task 5 (extract `TicketSummary`) has no dependency on the backend tasks or on Task 4 — it only touches existing, already-shipped frontend code (`TicketPanel.tsx`) and could be done first, in parallel with Tasks 1-4, if that suits the executor better. It is sequenced here after Task 4 only to match this plan's overall backend-then-frontend narrative, not because of a real import dependency.
- Task 6 depends on both Task 4 (imports `getMyTickets`/`TicketListItem`) and Task 5 (imports the extracted `TicketSummary`).
- Task 7 depends on Task 6 only for the link target to be meaningful (`/track/tickets` should exist); it has no import-level dependency on Task 6 and could be built in parallel if needed.
- Overall recommended order: 1, 2, 3, 4, 5, 6, 7 — matching the dependency chain above and the backend-then-frontend structure `docs/superpowers/plans/2026-08-31-tracked-trains-list.md` (this plan's closest structural precedent) also uses.

## Open questions carried forward from the spec (not resolved by this plan)

1. **`MINE_TICKETS_LIMIT`'s proposed value (`100`, Task 2 Step 2) is chosen for consistency with the sibling trains-list spec's own `MINE_LIST_LIMIT`, not independently researched.** This plan implements it as specified — revisit once real usage data exists, per the design spec's Open Question 1.
2. **Up to 100 repeated, full-text Delay Repay disclaimer blocks could render on one fully-populated page (Decision 3's accepted visual-weight tradeoff).** This plan builds the page exactly as specified — every row's `<DelayRepayEstimate>` renders its own full, verbatim disclaimer, per this plan's own Global Constraints forbidding any change to that component's contract. Whether this reads as reassuring or repetitive at real scale is flagged by the spec as genuinely unknown until this ships, per the spec's Open Question 2 — not resolved by any task in this plan.
3. **Whether to eventually group tickets by their owning tracked train (a two-level train → ticket(s) list) instead of a flat, ticket-per-row list** is the spec's own Open Question 3, deliberately left open. Task 6 implements the flat list as specified.
4. **The forward-compatible `/track/mine` per-row ticket-count touch point (Decision 3) is sketched, not designed, and is not implemented by this plan at all** — per this plan's Global Constraints, it belongs to whichever future plan implements `/track/mine` (not yet built, per this plan's Status note), which would need a new join/column on that plan's own `TrackedTrainListItem`/`list_tracked_trains_for_user`.
5. **Headlining the ticket's own `originCrs`/`destinationCrs` as a row's "route," rather than the tracked train's pinned route, is the spec's own judgment call (Open Question 5)** — implemented as specified in Task 6's `TicketListRow` (via the reused `TicketSummary` component, which already reads the ticket's own `originCrs`/`destinationCrs`), not re-decided by this plan.
