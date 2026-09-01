# Design: Tickets List

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md` (the
closest and most relevant structural precedent — same problem shape: "a
user-scoped list view is missing even though the underlying per-item data
and ownership model already exist," same team, same session, same
conventions) and `docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
(the reviewed precedent for every piece of ticket/Delay-Repay rendering
this spec reuses rather than reinvents). No implementation plan is
included; that is a separate, later step in this repo's process.

## Goal

Journey ticket tracking — attach a `.pkpass`/PDF/manual ticket to a
tracked train, see a Delay Repay estimate — is fully implemented and
merged. `TicketPanel` (`frontend/components/TicketPanel.tsx`) is genuinely
wired into both `frontend/app/train/by-id/[trackingId]/page.tsx` and
`frontend/app/train/[uid]/[date]/page.tsx`, confirmed by direct inspection
of both page files. But there is no standalone, discoverable place to see
"all the tickets I've attached across every train I've tracked" — a user
can only see their tickets by first navigating to a specific train's
detail page, which itself requires already knowing its tracking id or
UID+date. There is no nav-bar entry for tickets, and the backend has no
cross-train ticket query: `crates/api/src/data/train_tracking.rs`'s
`list_tickets_for_tracked_train` is scoped to one tracked train (`WHERE
tracked_train_id = $1 AND user_id = $2`), not one user across every
tracked train they have.

This is the exact same shape of gap
`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md` closed
for trains themselves, and that spec's own Goal section says so directly:
its "Explicitly out of scope" lists *"A 'my tickets across all tracked
trains' view... Different backend gap... already flagged as out of scope
by [the ticket-tracking-frontend spec]'s own 'Explicitly out of scope'
section. This spec is trains-only; tickets stay reachable only via each
train's own detail page's `TicketPanel`."* This spec is that deferred
follow-up, scoped to tickets.

## Corrections / findings from direct investigation

Following this repo's established "Corrections" precedent: things the
brief's framing left open (or got approximately right but not precisely)
that direct reading resolved, materially shaping the design below.

1. **Ticket ownership is a direct `user_id` column on
   `tracked_train_tickets`, not purely transitive through the owning
   tracked train — confirmed, not assumed.** The brief's framing
   ("presumably transitive through the owning tracked-train's `user_id`,
   not a direct `user_id` column on the ticket itself") had the arrow
   backwards. `crates/api/migrations/20260829090000_journey_ticket_tracking.sql`'s
   own header comment on the column states this explicitly: *"Redundant
   with tracked_trains.user_id by construction... Kept explicit so every
   ownership check on this table filters directly (WHERE user_id = $n)
   without a join."* `train_tracking::create_ticket` writes `user_id`
   directly from the caller (post-ownership-check, never from the request
   body); `list_tickets_for_tracked_train` and `get_ticket_owned` both
   filter `WHERE ... user_id = $n` with **no join to `tracked_trains` at
   all**. This matters directly for this spec's own query (Decision 1): a
   cross-train, user-scoped ticket list needs **no join for ownership** —
   `WHERE t.user_id = $1` on `tracked_train_tickets` alone is a complete,
   already-indexed (`tracked_train_tickets_user_id`) ownership filter. A
   join to `tracked_trains` (and `train_current_state`) is still needed,
   but only to pull in **train context for display** (route, date, live
   delay), not to establish ownership.
2. **`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md`
   (the trains list feature) is a design proposal only — it has not been
   implemented.** Grepped the whole tree for `Train/mine`,
   `list_tracked_trains_for_user`, and `track/mine`: zero matches anywhere
   outside that spec/plan's own doc files. `crates/api/src/routes/train.rs::router()`
   still has no `/Train/mine` route, `crates/api/src/data/train_tracking.rs`
   has no `list_tracked_trains_for_user`, and `frontend/app/track/` has
   only `page.tsx` (the search form), no `mine/` subdirectory. This
   directly shapes Decision 3 below: this spec cannot design "surface
   tickets as part of the tracked-trains list page" as its primary,
   load-bearing plan, because that page doesn't exist to surface anything
   on. It designs a standalone page that stands on its own, plus a
   documented (but explicitly out-of-scope-to-implement-here) touch point
   for if/when the trains list ships.
3. **`get_delay_repay_estimate`'s real per-call cost is two sequential,
   separately-scoped lookups** (`train_tracking::get_ticket_owned`, then
   `train_tracking::get_by_tracking_id`) **per `(trackingId, ticketId)`
   pair** — confirmed by reading `crates/api/src/routes/train.rs`'s
   handler directly. `TicketPanel.tsx` already accepts this as an N+1
   cost across a single train's own (expected-small) ticket count
   (`Promise.all(tickets.map(async (ticket) => ({ ticket, estimate: await
   getDelayRepayEstimate(trackingId, ticket.id) })))`, flagged as Open
   Question 1 of the ticket-tracking-frontend spec, "fine... likely fine
   in practice"). Naively reusing that same per-ticket-HTTP-call pattern
   for a cross-train list of up to 100 tickets (Decision 2) would multiply
   that N+1 cost by however many tracked trains a user has, tickets included
   — a real scaling concern the brief anticipated ("more backend work").
   **But this is avoidable, not inherent**: `delay_repay_rules::estimate_delay_repay(operator:
   &str, delay_minutes: i32)` and `claim_url_for(operator: &str)`
   (`crates/api/src/data/delay_repay_rules.rs`) are both pure functions —
   no `PgPool`, no I/O, confirmed by that file's own module doc ("every
   function in this file is pure"). Both only need `operator` (already a
   column on `tracked_train_tickets`) and `delay_minutes` (already a
   column on `train_current_state`, reachable via the same
   `tracked_train_id` join this spec's list query already needs for route/
   date context per Finding 1). **This means a single list query that
   joins `tracked_train_tickets` → `tracked_trains` → `train_current_state`
   can compute every row's Delay Repay estimate in Rust, in the same
   request, with zero additional database round trips** — cheaper than
   `TicketPanel`'s own existing per-ticket-fetch pattern, not more
   expensive. This is the deciding finding behind Decision 1's "inline,
   not link-through" call.
4. **The route-level disclaimer text lives in the wrong place to reuse
   safely.** `DELAY_REPAY_ROUTE_DISCLAIMER` (the top-level, unconditional
   disclaimer string — see `2026-08-29-journey-ticket-tracking-frontend-design.md`
   Decision 3 for why this exact string, not `delay_repay_rules::DISCLAIMER`,
   is the one that must render) is a private `const` inside
   `crates/api/src/routes/train.rs`, not exported from
   `crates/api/src/data/delay_repay_rules.rs` alongside its sibling
   `DISCLAIMER` const. Building this spec's new list-row mapping function
   in `train_tracking.rs` (a `data` module, which — correctly, per this
   repo's existing layering — does not depend on `routes/train.rs`) means
   it cannot reach that constant as-is. Duplicating the literal string in
   a second location would be a real drift risk for a
   safety-critical, verbatim-required string (see Decision 3 of the
   frontend spec: *"Do not paraphrase, shorten, or drop this string"*).
   Decision 1 below specifies hoisting it into `delay_repay_rules.rs` as a
   shared `pub const ROUTE_DISCLAIMER` — a mechanical move, not a wording
   change, and not a re-litigation of what the string says.
5. **No retention/pruning job exists anywhere in this codebase for
   `tracked_train_tickets` either.** Grepped `crates/` for `prune`,
   `retention`, `expire`, `DELETE FROM tracked_train_tickets` — the only
   hits are `ON DELETE CASCADE` (cleans up only if the parent
   `tracked_trains` row is ever deleted, which per the trains-list spec's
   own Finding 3 never happens) and unrelated matches in other crates
   (`aggregator`'s `line_status_history` pruning, `trust-consumer`'s
   in-memory activation pruning, session expiry — none touch this table).
   Same finding as the trains-list spec's Finding 3, for the sibling
   table: this table grows without bound for as long as a user keeps
   adding tickets, and a "list everything" query has no database-side
   cutoff to lean on. Shapes Decision 2 below the same way.
6. **`TicketSummary` (the "operator — ticket type" / "origin → destination"
   row renderer) is a private, unexported function inside
   `TicketPanel.tsx`**, not its own component — confirmed by reading the
   file in full (`function TicketSummary({ ticket }: ...)`, no `export`).
   It can't be reused by a new page as-is. The brief's instruction to
   "reuse/adapt these rather than reinventing ticket-row rendering from
   scratch" requires extracting it into its own small exported component
   first (Decision 3) — a minimal, mechanical refactor of existing,
   already-reviewed rendering, not new design.
7. **`DelayRepayEstimate.tsx` is already its own exported component**,
   taking a `{ response: DelayRepayEstimateResponse }` prop and containing
   zero fetch logic of its own (confirmed: "Pure presentational — takes an
   already-fetched response, no fetch of its own"). This one is
   **directly** reusable with no extraction step: this spec's new
   `TicketListItem` wire shape (Decision 1) deliberately carries the exact
   same four fields `DelayRepayEstimateResponse` has
   (`delayMinutes`/`estimate`/`claimUrl`/`disclaimer`), so a list row can
   pass `{ delayMinutes: item.delayMinutes, estimate: item.estimate,
   claimUrl: item.claimUrl, disclaimer: item.disclaimer }` straight into
   `<DelayRepayEstimate response={...} />` with no new rendering logic at
   all — see Decision 3's safety discussion for why this literal reuse,
   not a rewritten "compact" version, is the design.
8. **A new literal route segment, `/Train/tickets/mine`, is safe against
   this router's existing dynamic segments, by the same matchit
   literal-vs-dynamic precedent the trains-list spec's own Finding 5
   already established and relied on** (`/Train/track`, a literal,
   coexisting safely with `/Train/{tracking_id}`, a dynamic, at the same
   position). Every existing route that continues past `/Train/{tracking_id}/tickets/...`
   hangs off the **dynamic** `{tracking_id}` branch of the router's trie;
   no existing route has a **literal** `tickets` segment directly under
   `/Train/`. Adding `/Train/tickets/mine` introduces that literal branch
   for the first time, but it's a genuinely new, non-conflicting branch —
   not a case of two patterns racing for the same request shape the way
   Finding 5's `/Train/track` vs. `/Train/{tracking_id}` case was. The
   existing `router_builds_without_panicking` test (`crates/api/src/routes/train.rs`)
   is still the right place to catch a mistake here at `cargo test` time,
   same as Finding 5 already established.

## Current relevant state (verified 2026-09-01)

**Backend (`crates/api`)**, ticket routes mounted under `/Train/{tracking_id}/tickets/...`
on the root router, per `crates/api/src/routes/train.rs`:

- `create_ticket(pool, tracked_train_id, entry, user_id)` — writes `user_id`
  directly onto `tracked_train_tickets`, always from the authenticated
  caller post-ownership-check, never from the request body.
- `list_tickets_for_tracked_train(pool, tracking_id, user_id)` — filters
  `WHERE tracked_train_id = $1 AND user_id = $2` directly on
  `tracked_train_tickets`, no join. Scoped to **one** tracked train.
- `get_ticket_owned(pool, ticket_id, user_id)` — filters `WHERE id = $1
  AND user_id = $2`, also no join. Used by the Delay Repay route.
- `tracked_train_owner(pool, tracking_id)` — single-row ownership check on
  `tracked_trains` itself, used by `POST`/`GET .../tickets` to decide
  `404` vs. proceeding, per this app's "never `403`" convention.
- **No query exists today that answers "every ticket belonging to user
  X, across every tracked train they have."**
- `GET /Train/{trackingId}/tickets/{ticketId}/delay-repay` —
  session-gated, owner-scoped (`get_ticket_owned`, then a
  `tracked_train_id` match check, then `get_by_tracking_id` for the
  train's live `delay_minutes`), returns `DelayRepayEstimateResponse`
  (`delayMinutes`, `estimate`, `claimUrl`, `disclaimer` — the last two
  **always** populated, per that route's own doc comment: *"this route
  must never leave a caller with a bare percentage and no caveat, or with
  nowhere real to go"*). `build_delay_repay_response` (the pure response
  assembler, unit-tested in `routes/train.rs`'s own `#[cfg(test)]` module)
  is the exact logic this spec's new list-row mapper needs to mirror —
  see Decision 1.
- `crates/api/src/data/delay_repay_rules.rs` — pure, no I/O anywhere in
  the file (its own module doc's explicit "STRUCTURAL SAFETY NOTE").
  `estimate_delay_repay(operator: &str, delay_minutes: i32) ->
  Option<DelayRepayEstimate>` and `claim_url_for(operator: &str) ->
  &'static str` are both `pub`, already usable from anywhere in the crate.
  `GENERIC_CLAIM_URL` is `pub`. The route-level `DELAY_REPAY_ROUTE_DISCLAIMER`
  const, however, is private to `routes/train.rs` — see Finding 4.
- **Auth**: `AuthenticatedUser` (`crates/api/src/auth.rs`) — the same
  extractor every ownership-scoped route in this file already uses. Bare
  `401`, no body, on no/invalid session.
- **`tracked_train_tickets_user_id`** (`CREATE INDEX ... ON
  tracked_train_tickets (user_id)`, same migration) already exists and
  covers this spec's new query's `WHERE t.user_id = $1` — no new index
  needed.

**Frontend**: no `/track/tickets`-shaped (or equivalent) route exists in
`frontend/app/` today. `frontend/lib/api.ts` has `getTicketsForTrackedTrain`
and `getDelayRepayEstimate`, both scoped to one `trackingId`, both
returning `null` on `401` **or** `404` collapsed together (a deliberate
choice for `TicketPanel`'s own use, per that spec's Decision 1 — not
reusable verbatim here, see Decision 3). `frontend/components/TicketPanel.tsx`
and `frontend/components/DelayRepayEstimate.tsx` are the reviewed
rendering precedent (Findings 6–7 above cover exactly what's directly
reusable vs. needs extracting first).

**`frontend/app/layout.tsx`**'s nav bar (verified, lines 22–113): a flat
`<Group gap="lg">` of `TextLink`s and small async Server Components, each
wrapped in its own `<Suspense>` — `DataFreshnessNavItem` and `AuthNavItem`
are the two existing precedents for "a nav item whose content depends on
its own per-request fetch." `AuthNavItem`'s `getSession().catch(() => ({
authenticated: false, id: null, email: null, name: null }))` guard is the
established, reviewed shape — the exact guard this spec's own new nav item
(Decision 4) follows, and the shape `TicketPanel.tsx`'s own history
(per the ticket-tracking-frontend spec's brief) already found missing and
fixed once, elsewhere.

## Decisions

### 1. Backend: a new list-item shape and query that computes Delay Repay inline — not a per-ticket call

**New private row struct**, `TicketListRow`, plus a **new public wire
struct**, `TicketListItem`, both in `crates/api/src/data/train_tracking.rs`
— mirroring the existing `TrackedTrainRow → TrackedTrainRef` two-struct
pattern in this same file (a `sqlx::FromRow` row for exactly what the
query selects, mapped into a richer public struct that also carries
computed fields `FromRow` can't derive over):

```rust
/// Physical columns selected by `list_tickets_for_user`'s query --
/// private, exists only to satisfy `sqlx::FromRow`. `TicketListItem`
/// (below) is the public shape, built from this plus a pure computation
/// -- same two-struct pattern this file already uses for
/// `TrackedTrainRow` / `TrackedTrainRef`.
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
/// cross-train counterpart to `TrackedTrainTicket` (which is scoped to
/// one tracked train). Carries the ticket's own six fields (unchanged
/// from `TrackedTrainTicket`) plus enough of the owning tracked train's
/// context (route, date, live delay) to make a row useful without
/// clicking through (Finding 1's join is for THIS, not for ownership),
/// plus a Delay Repay estimate computed inline -- see this struct's own
/// build function for why that's a pure computation, not a second query
/// per row (Finding 3). The last four fields are deliberately named and
/// shaped to match `DelayRepayEstimateResponse` exactly, field-for-field,
/// so the frontend can pass a `TicketListItem` straight into the already-
/// reviewed `<DelayRepayEstimate>` component with no adapter (Finding 7).
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

**Build function**, mirroring `build_delay_repay_response`'s own logic in
`routes/train.rs` exactly (same `match (operator, delay_minutes)` shape),
so the two independently-computed estimates for the same
`(ticket, tracked train)` pair can never disagree:

```rust
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

**Required companion change (Finding 4): hoist `DELAY_REPAY_ROUTE_DISCLAIMER`
out of `routes/train.rs` into `delay_repay_rules.rs`** as `pub const
ROUTE_DISCLAIMER: &str = "..."` (byte-identical text — this is a move, not
a rewrite), and update `routes/train.rs`'s existing
`build_delay_repay_response` to reference `delay_repay_rules::ROUTE_DISCLAIMER`
instead of its own private const. This is the only change this spec makes
to already-shipped, reviewed code, and it changes zero behavior — it
exists solely so both call sites (the existing per-ticket route and this
spec's new list query) read the *same* string from one place, closing the
drift risk Finding 4 identifies rather than accepting it. (An optional,
slightly deeper refactor — factoring the shared `match (operator,
delay_minutes) { ... }` arm itself into one function both
`build_delay_repay_response` and `build_ticket_list_item` call — is left
as an implementation-time judgment call; not load-bearing for this spec,
since duplicating four lines of already-tested match logic is a much
smaller risk than duplicating a safety-critical string literal.)

**New query function**:

```rust
/// Caps `list_tickets_for_user`'s response size -- same reasoning as
/// `MINE_LIST_LIMIT` in this file (Finding 5: no retention/pruning job
/// exists for `tracked_train_tickets` either), a starting, unresearched
/// figure, not load-tested. See Open Questions.
const MINE_TICKETS_LIMIT: i64 = 100;

/// A user's own tickets, across every tracked train they have,
/// most-recently-added first. No join needed for ownership (`WHERE
/// t.user_id = $1` on `tracked_train_tickets` alone, per this table's own
/// ownership-redundancy design -- Finding 1) -- the joins to
/// `tracked_trains`/`train_current_state` exist purely to pull in enough
/// train context for a useful row (route, date, live delay) and to let
/// `build_ticket_list_item` compute each row's Delay Repay estimate
/// inline, with no per-ticket follow-up query (Finding 3).
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

`JOIN` (not `LEFT JOIN`) to `tracked_trains`: every `tracked_train_tickets`
row has a `NOT NULL tracked_train_id REFERENCES tracked_trains(id) ON
DELETE CASCADE`, so a ticket can never outlive (or predate) its parent
row — an inner join can't silently drop a ticket here. `LEFT JOIN` to
`train_current_state` is kept, matching every other query in this file
that reads it: a `pending`/just-resolved tracked train legitimately has no
`train_current_state` row yet.

**New route**: `GET /Train/tickets/mine`, in `crates/api/src/routes/train.rs`,
`AuthenticatedUser`-gated, same always-`200`-with-a-(possibly-empty)-array-
or-bare-`401` two-outcome shape as the trains-list spec's own `GET
/Train/mine` (no id in the path to be wrong about, so there's no third,
ownership-`404` outcome the way the per-train ticket routes have):

```rust
.route("/Train/tickets/mine", axum::routing::get(get_my_tickets))
```

```rust
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

Mounted as a literal, alongside this router's other literals (`/Train/track`,
and `/Train/mine` if the trains-list spec has landed by the time this is
implemented) — see Finding 8 for why this doesn't conflict with the
existing dynamic `/Train/{tracking_id}/...` branch.

### 2. Scope: every ticket a user has ever added, most-recently-added first, capped — no filter

Mirrors the trains-list spec's own Decision 2 reasoning closely, applied
to the sibling table: no retention/pruning job exists for
`tracked_train_tickets` (Finding 5), so the cap (`MINE_TICKETS_LIMIT`,
proposed `100`, matching the trains-list spec's own `MINE_LIST_LIMIT`
figure for consistency — see Open Questions) is the only bound on an
otherwise-unbounded response, not a claim that older tickets stop
mattering. No status- or eligibility-based filter (e.g. "only tickets with
a positive Delay Repay estimate") is designed here — nothing in the brief
or the existing backend suggests users asked for one, and per the trains-
list spec's own reasoning, a filter that isn't genuinely requested adds
complexity without a validated need; a plain, single-order list is the
whole of v1, same posture.

**Ordering: the ticket's own `created_at DESC`** (most recently *attached*
ticket first), not the owning tracked train's `service_date` or
`pin_scheduled_departure`. Same reasoning as the trains-list spec's
Decision 2 rejection of schedule-based ordering: a ticket attached five
minutes ago, for a train that's delayed right now, is far more likely to
be what the user opened this page to check on than one attached weeks ago
for a trip that already happened or hasn't happened yet. `created_at DESC`
needs no interpretation of `resolutionStatus`/`status` to be a single,
unambiguous "what did I do most recently" signal.

### 3. Frontend: a new standalone page at `/track/tickets` — not (only) a section of the still-unimplemented tracked-trains list

The brief asked for a real investigation of both placements. Per Finding
2, `/track/mine` (the trains list) **does not exist** — grepped and
confirmed zero implementation anywhere in the tree. This rules out
"surface tickets as part of that page" as this spec's primary,
load-bearing design: there is no page to surface anything on, and this
spec cannot respon­sibly make its own deliverable depend on a separate,
still-unapproved design landing first.

Independent of that landing status, a standalone ticket list is also the
better fit for what a ticket-focused view is actually for. A tracked-train
row answers "what's this specific train doing"; a ticket row answers a
different question — "which of these have I actually got a ticket for,
and is any of them worth an actual Delay Repay claim right now" — closer
in spirit to a personal "things to potentially act on" list than to a
live-status board. Folding it into a per-train list would mean either
showing the full Delay Repay block only for trains that happen to have a
ticket (an inconsistent row shape within one list) or omitting it there
entirely and still needing a separate surface for it — which is this page.

**Decision: build a standalone page, `frontend/app/track/tickets/page.tsx`,**
under the same `/track` segment as the existing search-form entry point
and the (proposed, not yet built) `/track/mine`, following that already-
proposed path convention for consistency rather than inventing a
different shape (e.g. `/tickets`) for what is structurally the same kind
of page.

**Forward-compatible touch point, not part of this spec's own deliverable:**
when/if `/track/mine` is eventually built, that page's own per-row shape
could add a compact "N ticket(s)" indicator, linking to **that same
train's own detail page** (where `TicketPanel` already renders the full
ticket + estimate block for that train) — not to this new cross-train
page, and not to a filtered view of it. Reasoning: `/track/mine`'s job is
"which of my trains needs my attention," and the answer to "does this one
have a ticket" is already fully, safely rendered on that train's own
detail page — sending the click there reuses an already-reviewed
rendering surface instead of building filtering machinery for a payoff
("show me just this one train's tickets, but on the cross-train page")
that the train's own detail page already provides for free. This would
need a ticket-count column/join added to that spec's own
`TrackedTrainListItem`/`list_tracked_trains_for_user` — real, but small,
future work belonging to whichever plan eventually implements `/track/mine`,
not this one; see Explicitly out of scope.

**`frontend/app/track/tickets/page.tsx`** (async Server Component):

```tsx
export default async function MyTicketsPage() {
  // getMyTickets() returning null on 401 is the COMPLETE "not logged in"
  // signal for this page -- same reasoning as the trains-list spec's own
  // Decision 3: there is no second party to disambiguate (no id in this
  // route's path that could belong to someone else), so no separate
  // getSession() call is needed here the way TicketPanel needs one.
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
```

The empty-state copy is deliberately explicit that ticket-adding only
happens from an already-tracked train's own page — this page is
read-only, there is no "add a ticket" entry point on it (a ticket always
needs a `trackingId` in context, which this cross-train list doesn't
provide one specific instance of). Linking to `/track` (the only entry
point that exists today) rather than `/track/mine` is a real, current-state
choice — worth revisiting once/if `/track/mine` ships, since it would let
a user reach an *already*-tracked train in one click rather than
re-searching.

`TicketListRow` (new component, or inlined — implementation-time
judgment call), reusing rather than reinventing (Findings 6–7):

```tsx
function TicketListRow({ ticket }: { ticket: TicketListItem }) {
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

`TicketSummary` is extracted out of `TicketPanel.tsx` into its own
exported component (Finding 6 — a mechanical refactor: `export function
TicketSummary({ ticket }: { ticket: Pick<TicketListItem, 'operator' |
'ticketType' | 'originCrs' | 'destinationCrs'> })` or equivalent, reused
unmodified by both `TicketPanel.tsx` and this new page). `DelayRepayEstimate`
is imported and used **exactly as-is**, no new props, no wrapper — this is
the literal reuse Finding 7 identifies, and it is what makes the
safety-critical rendering rules (full, verbatim disclaimer text; always
present; never collapsed; the claim link's `target="_blank" rel="noopener
noreferrer"` external-link hygiene) automatically inherited rather than
re-implemented on this page. **This spec adds no new Delay Repay rendering
logic of its own anywhere.**

**Accepted visual-weight tradeoff, stated plainly:** per Decision 3 of the
ticket-tracking-frontend spec, the disclaimer must render "in full, every
time," never collapsed behind a toggle. With up to `MINE_TICKETS_LIMIT`
(proposed 100) rows, a fully populated page could render up to 100
repeated full-disclaimer blocks. This is the direct, deliberate cost of
preserving the safety property exactly as already specified, not an
oversight of this spec — flagged as an Open Question below in case real
usage shows it needs revisiting (e.g., a future pass consolidating
identical disclaimer text once per page instead of once per row), but not
solved here, since doing so would mean touching the already-reviewed
`DelayRepayEstimate` component's own contract, which is out of scope for
this spec per the brief ("do not re-litigate the core ticket-tracking
feature's own already-settled decisions").

### 4. Auth/session handling: reuse the guarded pattern, no unguarded `getSession()` anywhere new

Two places this spec adds a `getSession()`-shaped check, both following
the already-established, already-guarded shape (`app/layout.tsx`'s
`AuthNavItem`/`DataFreshnessNavItem`, **not** the historical unguarded
call the brief flags as previously found and fixed in `TicketPanel.tsx`):

- **The page itself does not call `getSession()` at all** — per Decision
  3's inline reasoning, `getMyTickets()`'s own `null`-on-`401` return is
  the complete signal, mirroring the trains-list spec's own Decision 3
  ("this page does NOT need a separate `getSession()` call... there is no
  second party here at all").
- **The new nav item (Decision 5) does call `getSession()`**, guarded
  identically to `AuthNavItem`/`DataFreshnessNavItem`:
  `getSession().catch(() => ({ authenticated: false, id: null, email:
  null, name: null }))`. A root layout has no route-level `error.tsx`, so
  an unguarded call here would take down every page's nav bar on an auth
  glitch — the exact bug class the brief calls out as already found and
  fixed once in this codebase.

### 5. Nav integration: a second session-gated nav item, hidden entirely when logged out

**Decision: add `MyTicketsNavItem`, following `AuthNavItem`/`DataFreshnessNavItem`'s
exact shape** (own async Server Component, own `<Suspense fallback={null}>`,
own guarded `getSession()` call), same reasoning as the trains-list spec's
own Decision 4: this is a full nav-bar entry point to a page whose entire
content is private to the viewer, so it should not exist in the DOM at all
for an anonymous visitor (unlike `TicketPanel`'s own in-page degrade
pattern, appropriate there because it's a section of an already-public
page — not the case here).

```tsx
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

...wrapped in `<Suspense fallback={null}>`, placed in the nav `Group`
alongside `<TextLink href="/track">Track a Train</TextLink>` (and
alongside the trains-list spec's own `TrackedTrainsNavItem`, if that spec
has landed by the time this one is implemented — relative ordering
between the two is an implementation-time call, not load-bearing).

This is a **third** `getSession()` call on every page load if the
trains-list spec's own nav item has also landed (on top of `AuthNavItem`'s
existing one) — accepted as harmless for the same reason the trains-list
spec's own Decision 4 already accepted its second one: Next.js's
per-request `fetch` deduplication means identical `fetch` calls within one
render pass share a single underlying network request. No new caching
mechanism is built to avoid this.

### 6. Data refresh: reuse `AutoRefresh`, same as every other dynamic page

No new refresh mechanism. `getMyTickets()` is an ordinary `no-store`
Server Component read, covered by the existing global `AutoRefresh` (30s,
mounted once in `app/layout.tsx`), same accepted tension every other
dynamic page in this app already carries (a page with only long-finished
journeys on it keeps re-fetching every 30s regardless).

## API/type contract

Hand-written, matching this repo's existing convention of not generating
frontend types from Rust source:

```ts
// frontend/lib/types.ts additions

/** `GET /Train/tickets/mine`'s per-item response shape
 * (`crates/api/src/data/train_tracking.rs`'s `TicketListItem`, camelCase).
 * The last four fields are deliberately shaped identically to
 * `DelayRepayEstimateResponse` so a `TicketListItem` can be passed
 * straight into `<DelayRepayEstimate>` with no adapter -- see the design
 * spec's Finding 7 / Decision 1. */
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

(`TicketSource`, `DelayRepayEstimate`, `ResolutionStatus`, `JourneyStatus`
are all already defined in `frontend/lib/types.ts` — reused verbatim, no
new enum/union types needed.)

```ts
// frontend/lib/api.ts addition -- per-user, session-gated read, same
// cookie-forwarding pattern getSession()/getPreferences()/
// getTicketsForTrackedTrain() already use.

/** `GET /Train/tickets/mine`. Returns `null` on `401` (not logged in) --
 * deliberately not `ApiNotFoundError`, matching `getTicketsForTrackedTrain`'s
 * precedent of treating "no session" as an expected, first-class outcome.
 * Unlike that function, there is no second, distinct 404-shaped outcome to
 * also collapse into `null` here -- no id in this route's path to be
 * wrong about, so a 401 from this one call is the complete signal (same
 * reasoning as the trains-list spec's own `getMyTrackedTrains`). */
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

No proxy (`app/api/[...path]/route.ts`) changes needed: `GET /Train/tickets/mine`
is a server-side-only read, like every other `lib/api.ts` function —
never called from a Client Component, so it never goes through the
browser-facing proxy. Already covered path-wise even before any allowlist
check, since it sits under the already-passed-through `/Train/...` prefix.

## Architecture

```
┌────────────────────────────────────────────────────────────────────────┐
│ frontend/ (Next.js App Router)                                          │
│                                                                            │
│  app/track/tickets/page.tsx     NEW -- async Server Comp, cookie-fwd     │
│                                    GET /Train/tickets/mine, login nudge / │
│                                    empty state / list, per-row link +     │
│                                    inline <DelayRepayEstimate>            │
│                                                                            │
│  components/TicketSummary.tsx   NEW -- extracted out of TicketPanel.tsx  │
│                                    (Finding 6), reused by both            │
│  components/DelayRepayEstimate.tsx  UNCHANGED -- reused as-is (Finding 7)│
│                                                                            │
│  app/layout.tsx                  + MyTicketsNavItem (NEW, own Suspense,  │
│                                    own guarded getSession())              │
│                                                                            │
│  lib/api.ts    + getMyTickets                                            │
│  lib/types.ts  + TicketListItem                                          │
└──────────────────────────┬────────────────────────────────────────────────┘
     server-side fetch     │
     (read, cookie-fwd,    │
     no-store)             ▼
                 ┌──────────────────────────────────────────────┐
                 │ api crate                                       │
                 │  GET /Train/tickets/mine   NEW -- AuthenticatedUser-│
                 │    gated -> train_tracking::list_tickets_for_user│
                 │       (NEW query + TicketListItem, NEW)          │
                 │  delay_repay_rules.rs: DELAY_REPAY_ROUTE_DISCLAIMER│
                 │    hoisted from routes/train.rs -> pub ROUTE_DISCLAIMER│
                 │    (mechanical move, both call sites updated)   │
                 └──────────────────────────────────────────────┘
```

## Error handling

- `getMyTickets()`'s `401` branch is not an error path — an expected,
  common, first-class outcome (anonymous visitor, or a session that
  lapsed since page load), rendered as the login nudge.
- Any other non-ok status (5xx, network failure) throws via the shared
  `errorForResponse`, falling through to the existing root `app/error.tsx`
  — no segment-specific `error.tsx`, matching every other page with no
  bespoke error boundary today.
- `MyTicketsNavItem`'s `getSession()` call is guarded with `.catch()`,
  degrading to "link hidden" on any auth-check glitch rather than taking
  down the nav bar for every visitor.
- No new upload/mutation surface is introduced by this spec at all — pure
  read/list feature, same as the trains-list spec. None of the
  multipart/file-upload error handling from the ticket-tracking spec is
  relevant here; this page has no "add a ticket" affordance of its own
  (Decision 3).
- `estimate`/`claimUrl`/`disclaimer` are computed server-side, in Rust,
  before serialization — there is no frontend code path where a row can
  have an `estimate` without a `claimUrl`/`disclaimer` (the same backend
  invariant `build_delay_repay_response` already guarantees for the
  per-ticket route, preserved here since `build_ticket_list_item` mirrors
  its logic — Decision 1).

## Testing

Following this repo's existing convention (colocated `*.test.tsx`,
`renderWithMantine`, Vitest; Rust `#[cfg(test)]` modules colocated in the
same file):

- `crates/api/src/data/train_tracking.rs`: `build_ticket_list_item` is
  pure (no `PgPool`) and directly unit-testable, mirroring
  `routes/train.rs`'s own `build_delay_repay_response` test shape —
  construct a `TicketListRow`, assert the resulting `TicketListItem`'s
  `estimate`/`claimUrl`/`disclaimer` match what `build_delay_repay_response`
  would independently compute for the same `(operator, delay_minutes)`
  pair (a real regression check that the two mirrored implementations
  haven't drifted). `list_tickets_for_user` itself follows this file's
  existing convention of leaving query functions untested at the unit
  level (no other query function in this file has one either).
- `crates/api/src/routes/train.rs`: extend `router_builds_without_panicking`
  coverage implicitly by adding the route (Finding 8's regression check).
- `crates/api/src/data/delay_repay_rules.rs`: existing tests for
  `estimate_delay_repay`/`claim_url_for` are unaffected by hoisting
  `ROUTE_DISCLAIMER` into this file; add one trivial test asserting
  `ROUTE_DISCLAIMER != DISCLAIMER` (they are, and must stay, two distinct
  strings — see Finding 4 and the frontend spec's own Decision 3) and that
  `ROUTE_DISCLAIMER` is non-empty.
- `lib/api.ts`: unit tests for `getMyTickets` — `401` → `null`, `200 []` →
  `[]` (not `null`), `200` with items → resolves normally, non-`401`
  failure → throws — mirroring `getMyTrackedTrains`'s test shape from the
  trains-list spec.
- `components/TicketSummary.tsx` (post-extraction): existing rendering
  behavior preserved — a render test confirming `TicketPanel.tsx`'s own
  existing tests still pass unmodified after the extraction (a pure
  refactor should not change what either file renders).
- `app/track/tickets/page.tsx`: render tests for the three real outcomes —
  `null` (login nudge), `[]` (empty state, working link to `/track`), and
  a populated list (each row's `TicketSummary` content, the correct
  canonical-vs-by-id link per `resolutionStatus`/`trainUid`, and a
  `<DelayRepayEstimate>` block present for every row, verbatim-rendering
  its `disclaimer` — the direct regression check that this page never
  drops the safety-critical text for any row, mirroring the ticket-
  tracking-frontend spec's own Delay Repay render-test convention).
- `app/layout.tsx`'s `MyTicketsNavItem`: render test confirming the link
  is absent when logged out and present, pointing at `/track/tickets`,
  when logged in — mirroring `AuthStatus.test.tsx`'s existing shape.

## Explicitly out of scope for this spec

- **Editing or deleting a saved ticket.** No `PUT`/`DELETE` route exists
  anywhere in the ticket family today (unchanged from the ticket-tracking-
  frontend spec's own "Explicitly out of scope") — this spec doesn't add
  one, and this list page is read-only.
- **Any "add a ticket" entry point on this page.** Ticket creation is,
  and stays, only reachable from a specific tracked train's own detail
  page via `TicketEntryForm` (already-shipped, already-reviewed) — a
  ticket always needs a concrete `trackingId` in context, which a
  cross-train list doesn't supply one specific instance of. Not a gap;
  a deliberate boundary.
- **A per-row ticket-count indicator on `/track/mine`.** Per Decision 3,
  this is real, sensible future work, but it belongs to whichever plan
  eventually implements the (currently unimplemented) trains-list spec,
  not this one — it would need a new join/column on that spec's own
  `TrackedTrainListItem`/`list_tracked_trains_for_user`, which this spec
  does not touch.
- **Any retention/pruning job for `tracked_train_tickets`.** Per Finding
  5, none exists today, and this spec doesn't add one — `MINE_TICKETS_LIMIT`
  bounds one response, it does not delete or archive anything.
- **Pagination / "load more" past the cap.** Not designed here — see Open
  Questions. A user with more than `MINE_TICKETS_LIMIT` tickets simply
  can't reach the oldest ones through this page; their individual tracked
  trains' own pages (if the tracking id/URL is still known) remain
  unaffected.
- **Filtering/search/sort controls** (e.g. "only tickets with a positive
  estimate," "only this operator"). No real, requested need surfaced
  during this research pass — a plain, single-order list is the whole of
  v1, per Decision 2.
- **Consolidating the repeated full-disclaimer text into a single
  page-level notice instead of one per row.** Flagged as the direct cost
  of Decision 3's literal-reuse approach; not solved here since it would
  mean touching `DelayRepayEstimate.tsx`'s own already-reviewed contract,
  out of scope per the brief.
- **Fixing the underlying `'completed'`-status gap** in
  `crates/trust-consumer/src/journey.rs` — same out-of-scope item the
  trains-list spec already carries forward from the train-tracking-
  frontend spec; not this spec's concern either.
- **Real-time updates faster than the existing global 30s `AutoRefresh`.**
  No new refresh mechanism, per Decision 6.

## Open questions / risks

1. **`MINE_TICKETS_LIMIT`'s proposed value (`100`) is chosen for
   consistency with the trains-list spec's own `MINE_LIST_LIMIT`, not
   independently researched.** In practice, ticket count per user is
   likely much smaller than tracked-train count (not every tracked train
   gets a ticket attached), so `100` may be generously high rather than a
   real constraint for most users — but, same as the trains-list spec's
   own Open Question 1, this codebase has no real usage data yet to size
   either number against. Revisit once real usage exists.
2. **The accepted visual-weight tradeoff (Decision 3): up to 100 repeated,
   full-text Delay Repay disclaimer blocks on one page.** Flagged
   explicitly rather than solved — the safety requirement (full text,
   every row, never collapsed) is non-negotiable per the already-settled
   ticket-tracking-frontend spec, but whether that reads as reassuring or
   repetitive at real scale is genuinely unknown until this ships and
   gets used. If it turns out to be a real problem, the fix belongs to a
   fresh pass on `DelayRepayEstimate.tsx`'s own contract (e.g., a
   page-level "every estimate below carries the same disclaimer" framing
   sentence plus per-row short text) — not something this spec resolves
   unilaterally, since it would be re-litigating an already-settled
   safety decision.
3. **Whether to eventually group tickets by their owning tracked train
   (a two-level list: train → its ticket(s)) instead of a flat,
   ticket-per-row list** is a real UI judgment call this spec makes
   narrowly (flat list, per Decision 3) but doesn't claim is definitively
   right for every usage pattern — e.g., a user who bought return tickets
   and tracks both legs separately might prefer seeing them grouped.
   Revisit once real usage exists, same posture the trains-list spec's own
   Open Question 3 already took for its own row-shape judgment call.
4. **The forward-compatible `/track/mine` per-row ticket-count touch point
   (Decision 3) is sketched, not designed.** Exact shape (a bare count? a
   count plus "possible compensation" flag if any attached ticket has a
   non-null `estimate`?) is left to whichever future plan actually
   implements `/track/mine` — this spec only establishes that linking
   through to the train's own page, not to this new page, is the right
   target once that work happens.
5. **Headlining the ticket's own `originCrs`/`destinationCrs` (when
   present) as the row's "route," rather than the tracked train's
   `pinOriginCrs`/`pinDestinationCrs`, is this spec's own judgment call,**
   on the reasoning that a ticket documents what the passenger is actually
   entitled to travel, which is the more relevant fact for a ticket-
   focused list. Both are included on the wire (`TicketListItem` carries
   both), so this is a rendering choice, not a data-availability
   constraint — worth revisiting if real usage shows the pin's route is
   what users actually look for first on this particular page.
