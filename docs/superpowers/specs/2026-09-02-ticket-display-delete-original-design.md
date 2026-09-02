# Design: Ticket Display, Delete & Original-Document Access

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-tickets-list-design.md` (the direct
predecessor for everything ticket-list-shaped this spec touches) and
`docs/superpowers/specs/2026-09-01-tracked-trains-home-page-design.md`
(the most recent sibling spec in this same session, same conventions). No
implementation plan is included; that is a separate, later step in this
repo's process.

This spec covers three requested improvements of very different risk
profiles: (1) showing more of a ticket's already-stored fields regardless
of train-link status, (2) letting a user delete a ticket, and (3) opening
the original uploaded document or rendering a scannable QR code. The first
two are designed fully below. The third is **not** designed as requested —
see Decision 3, which is the load-bearing section of this document.

## Relationship to prior specs

`docs/superpowers/specs/2026-08-31-tickets-list-design.md`'s own
"Explicitly out of scope" section states plainly: *"Editing or deleting a
saved ticket. No `PUT`/`DELETE` route exists anywhere in the ticket family
today... this spec doesn't add one, and this list page is read-only."*
That remains true as of this writing (confirmed by re-reading
`crates/api/src/routes/train.rs`'s route table in full, `crates/api/src/routes/train.rs:26-62`
— no `DELETE` route exists for `/Train/tickets/...` anywhere; the only
`DELETE` in the ticket family's neighbourhood is
`DELETE /Train/{tracking_id}` (`crates/api/src/routes/train.rs:48`), which
deletes an entire tracked train, cascading its tickets as a side effect,
not a ticket on its own). This spec is the deliberate follow-up that closes
that gap (Decisions 2–3).

## Current relevant state (verified 2026-09-02)

**The ticket feature has grown since the tickets-list design spec was
written** — it is worth restating what exists today, since some of it
postdates that spec:

- `crates/api/src/routes/train.rs:26-62` (`router()`): the full current
  route table includes `POST /Train/tickets` (standalone ticket creation,
  no `tracked_train_id`), `GET /Train/tickets/mine` (cross-train list),
  `POST /Train/tickets/pkpass` / `POST /Train/tickets/pdf` (standalone
  upload-preview), `POST /Train/tickets/{ticket_id}/attach` (attaches a
  standalone ticket to a tracked train the caller owns), alongside the
  older `{tracking_id}`-scoped family (`POST`/`GET
  /Train/{tracking_id}/tickets`, `GET
  /Train/{tracking_id}/tickets/{ticket_id}/delay-repay`, upload routes).
  A ticket can now exist with `tracked_train_id: NULL` — the "regardless
  of whether a train has actually been linked" framing in the brief is not
  hypothetical; it is this app's real, shipped `TicketListItem`/
  `TrackedTrainTicket` shape (`frontend/lib/types.ts:287-296`,
  `frontend/lib/types.ts:398-418`).
- **`frontend/components/TicketSummary.tsx`** (read in full,
  `frontend/components/TicketSummary.tsx:12-28`): the shared row renderer,
  used by `TicketPanel.tsx:94` and `app/track/mine/page.tsx:159,194`. Its
  prop type is `Pick<TrackedTrainTicket | TicketListItem, 'operator' |
  'ticketType' | 'originCrs' | 'destinationCrs'>` — **only** four of the
  fields either wire shape carries. It renders `"{operator} — {ticketType}"`
  and, if either CRS is present, `"{origin} → {destination}"`. Nothing
  else.
- **What's already on the wire but not rendered.** Both
  `TrackedTrainTicket` (`frontend/lib/types.ts:287-296`) and
  `TicketListItem` (`frontend/lib/types.ts:398-418`) carry `source:
  TicketSource` and `createdAt: string` (RFC3339) as real, always-populated
  fields — confirmed against the Rust structs that produce them:
  `crates/api/src/data/train_tracking.rs:563-572` (`TrackedTrainTicket`,
  `pub source: String`, `pub created_at: DateTime<Utc>`, neither
  `Option`-wrapped) and `crates/api/src/data/train_tracking.rs:679-699`
  (`TicketListItem`, same two fields, same non-`Option` typing). Both are
  populated by the ticket's own six original columns, independent of
  `tracked_train_id`/attachment status — the migration's own table
  definition (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:36-49`)
  has `source` `NOT NULL DEFAULT 'manual'` and `created_at TIMESTAMPTZ NOT
  NULL DEFAULT NOW()` directly on `tracked_train_tickets`, with no
  dependency on the (nullable) `tracked_train_id` column. **Confirmed:
  these two fields are already correctly populated for a standalone
  (unattached) ticket** — nothing about them is train-link-dependent, so
  surfacing them requires no backend change and no special-casing for the
  unattached path.
- **Existing provenance-badge precedent.** `frontend/components/IssueList.tsx:37-43`
  (`DATA_QUALITY_LABELS`, a `Record<LineStatus['dataQuality'], string>`)
  and `frontend/components/IssueList.tsx:367-369` (`<Badge variant="outline"
  size="sm" color="gray">{DATA_QUALITY_LABELS[status.dataQuality]}</Badge>`,
  with the surrounding comment: *"Explicit gray: without a `color`, Mantine
  falls back to theme.primaryColor... It's provenance, not brand"*) is the
  established, reviewed visual language this app already uses for exactly
  this kind of thing — a data-quality/provenance tag that must never read
  as branded or actionable. `tracked_train_tickets.source`'s own migration
  comment (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:17-23`)
  explicitly frames itself as "extending DESIGN.md's dataQuality
  philosophy... of never collapsing inferred data into an unlabelled
  value" — so this precedent is not just visually similar, it is the
  designed conceptual sibling of the `dataQuality` badges.
- **`app/track/mine/page.tsx`'s standalone-ticket path** (read in full):
  unattached tickets render via `UnattachedTicketRow`
  (`frontend/app/track/mine/page.tsx:180-212`), which calls `<TicketSummary
  ticket={ticket} />` (line 194) with the exact same `TicketListItem` shape
  attached tickets use — there is no separate, narrower type or rendering
  path for the unattached case. Whatever `TicketSummary` is changed to
  render, both call sites (`TicketPanel.tsx`'s attached tickets,
  `TrackedTrainListRow`'s attached tickets, `UnattachedTicketRow`'s
  standalone tickets) get it automatically, with no separate wiring.
- **No single-ticket delete route exists** (confirmed by the route table
  above). `crates/api/src/routes/train.rs:746` (inside
  `delete_user_and_dependents`, the bulk cascade tied to full account
  deletion) is the only place `DELETE FROM tracked_train_tickets` appears
  anywhere in the codebase (grepped `crates/`), and it is user-scoped, not
  ticket-scoped.
- **The existing delete precedent, read in full**:
  `frontend/components/DeleteTrainButton.tsx:1-95` — a `'use client'`
  component: confirm modal (`useDisclosure`), calls `fetch('/api/Train/{id}',
  { method: 'DELETE' })` through the same-origin proxy
  (`frontend/app/api/[...path]/route.ts`), handles `401` via the
  established `useNeedsLogin`/`LoginLink` pattern (never shows the raw
  backend rejection text), any other non-`ok` status as a generic error
  message, and on success calls `router.push('/track/mine')` — appropriate
  there because deleting the tracked train removes the entire subject of
  the page it's rendered on. `crates/api/src/routes/train.rs:325-337`
  (`delete_tracked_train`) is its backend counterpart: `AuthenticatedUser`,
  `train_tracking::delete_tracked_train(pool, id, user_id)`
  (`crates/api/src/data/train_tracking.rs:413-420`, `DELETE FROM
  tracked_trains WHERE id = $1 AND user_id = $2`, `rows_affected() > 0`
  decides `204` vs `404`) — the exact "exists-but-not-yours and
  doesn't-exist both 404, never 403" convention this whole app uses
  everywhere (also `crates/api/src/routes/lines.rs:354` `delete_line`,
  cited by `delete_tracked_train`'s own doc comment,
  `crates/api/src/routes/train.rs:315-316`).
- **The "stay on this page, re-fetch server data" precedent**, distinct
  from `DeleteTrainButton`'s "navigate away" one: `frontend/components/AttachTicketAction.tsx:41`
  and `frontend/components/PinToggle.tsx:99` both call `router.refresh()`
  after a successful mutation through the same `/api/Train/tickets/...`
  proxy prefix, rather than navigating anywhere — the right precedent for
  a component that deletes one row out of a list the user is still looking
  at, as opposed to deleting the entire resource the current page exists
  to show.
- **`get_ticket_owned`** (`crates/api/src/data/train_tracking.rs:601-608`):
  `SELECT ... WHERE id = $1 AND user_id = $2`, no join — confirms, as the
  tickets-list spec's own Finding 1 already established, that ticket
  ownership is a direct column (`tracked_train_tickets.user_id`,
  `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:36`),
  not transitive through the owning tracked train. A delete query can
  filter the same way, with no join.
- **Route-trie precedent for adding a new literal-adjacent path.**
  `crates/api/src/routes/train.rs:30-42`'s own comment and the
  `literal_route_wins_over_same_position_dynamic_route` test
  (`crates/api/src/routes/train.rs:565-590`, referenced) establish that a
  new literal segment under `/Train/tickets/...` safely coexists with the
  dynamic `/Train/{tracking_id}/...` branch at the same position — already
  exercised for `/Train/tickets/mine`, `/Train/tickets/pkpass`,
  `/Train/tickets/pdf`. `/Train/tickets/{ticket_id}` (Decision 2, below) is
  a new *dynamic* segment nested one level under the existing literal
  `tickets` segment — it does not compete with any of those literals (they
  terminate at that same depth; `{ticket_id}` is matched only when the
  second segment is neither `mine`, `pkpass`, nor `pdf`) and is a different
  route depth from the existing `/Train/tickets/{ticket_id}/attach`
  (three segments after `/Train/`, vs. two for the new route) — no
  conflict, same reasoning the tickets-list spec's own Finding 8 already
  used for `/Train/tickets/mine`.

## Decisions

### 1. Enhanced `TicketSummary`: surface `source` and `createdAt` — frontend-only, no backend change

**Chosen: widen `TicketSummary`'s prop type and rendering to include
`source` (as a provenance badge, styled after `IssueList.tsx`'s
`dataQuality` badge) and `createdAt` (as a dimmed "added on" line, via
`formatDateTime` from `frontend/lib/dateFormat.ts:23-27`), in addition to
the four fields it already renders.** No backend change of any kind is
needed — as established in Current relevant state, both fields are already
serialized on both wire shapes (`TrackedTrainTicket`, `TicketListItem`) and
already populated correctly for both attached and unattached tickets. This
is a pure, low-risk rendering change to one already-shared component,
automatically inherited by every one of its three call sites
(`TicketPanel.tsx`, `TrackedTrainListRow`'s attached-ticket block,
`UnattachedTicketRow`) with no separate wiring per call site.

**Two real alternatives were considered for what to add beyond `source`/
`createdAt`:**

- **`id` (the ticket's own database id).** Rejected as user-facing
  content — it's an internal identifier with no meaning to the person
  reading the row (unlike, say, a booking reference, which this app never
  stores per the migration's audited constraint). It is already available
  to code that needs it (`ticket.id`, used internally for
  `DeleteTicketButton`/`AttachTicketAction`'s own props) without being
  rendered as text.
- **`tracked_train_id`'s presence/absence itself, rendered as an explicit
  "unattached" badge on `TicketSummary`.** Considered, but rejected as
  redundant with existing page-level structure: `app/track/mine/page.tsx`
  already puts unattached tickets under their own `"Tickets not yet
  attached to a train"` heading (`frontend/app/track/mine/page.tsx:96`),
  and `TicketPanel.tsx`'s tickets are always attached by construction (it
  only ever renders results from `getTicketsForTrackedTrain`, which is
  scoped to one tracked train). Rendering a second, row-level signal for
  the same fact `TicketSummary` doesn't otherwise know how to display
  cleanly (it has no page-level context of which section it's in) would
  duplicate information already communicated structurally, for no real
  gain — not designed here.

**Concrete shape** (illustrative, not code to be pasted verbatim):

- Widen the prop type: `Pick<TrackedTrainTicket | TicketListItem,
  'operator' | 'ticketType' | 'originCrs' | 'destinationCrs' | 'source' |
  'createdAt'>`.
- Below the existing route line, add a `Group` containing:
  - A `<Badge variant="outline" size="sm" color="gray">` with a
    `SOURCE_LABELS` lookup (new, local `Record<TicketSource, string>`,
    same shape as `IssueList.tsx`'s `DATA_QUALITY_LABELS`) — e.g. `manual`
    → "Manual entry", `pkpass-semantics`/`pkpass-heuristic` → "From Wallet
    pass", `pdf-heuristic` → "From PDF". Exact wording is a naming detail,
    not load-bearing; flagged in Open questions.
  - `<Text size="xs" c="dimmed">Added {formatDateTime(ticket.createdAt)}</Text>`.

**Verification of the standalone (unattached) case, as the brief
explicitly required rather than assumed:** `UnattachedTicketRow`
(`frontend/app/track/mine/page.tsx:180-212`) passes the exact same
`TicketListItem` object into `<TicketSummary ticket={ticket} />` that
`TrackedTrainListRow`'s attached-ticket branch does
(`frontend/app/track/mine/page.tsx:159`) — same component, same prop
shape, same two fields populated identically regardless of
`trackedTrainId` being `null` or not (Current relevant state, above). No
divergent behaviour exists or needs to be added for the unattached path;
this was verified by reading the actual field population, not assumed
from the type signature alone.

### 2. Backend: `DELETE /Train/tickets/{ticketId}`, ownership-scoped, mirroring `delete_tracked_train`

**Chosen: a new route, `DELETE /Train/tickets/{ticket_id}`**, added to
`crates/api/src/routes/train.rs`'s existing router alongside its literal
siblings under `/Train/tickets/...`. This is the flat-route convention
this file already uses for the ticket family (`/Train/tickets`,
`/Train/tickets/mine`, `/Train/tickets/pkpass`, `/Train/tickets/pdf`,
`/Train/tickets/{ticket_id}/attach`) — a ticket-scoped action addressed
directly by ticket id, not nested under a `{tracking_id}` (correct: a
ticket may have no owning tracked train at all, per Current relevant
state, so a route that requires a `tracking_id` in its path cannot be the
general case).

**New data-layer function**, mirroring `delete_tracked_train`
(`crates/api/src/data/train_tracking.rs:413-420`) exactly:

```
pub async fn delete_ticket(pool: &PgPool, ticket_id: i64, user_id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM tracked_train_tickets WHERE id = $1 AND user_id = $2")
        .bind(ticket_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
```

No join needed for ownership, per `get_ticket_owned`'s own established
precedent (Current relevant state) — `tracked_train_tickets.user_id` is a
direct, indexed column (`tracked_train_tickets_user_id`,
`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:56`).
Nothing else needs deleting as a consequence: a ticket has no child rows
anywhere in the schema (it is a leaf in the FK graph — nothing
`REFERENCES tracked_train_tickets`), unlike a tracked train, which needed
`delete_tracked_train`'s own accompanying note about cascades.

**New route handler**, mirroring `delete_tracked_train`'s handler
(`crates/api/src/routes/train.rs:325-337`) exactly in shape:

```
async fn delete_ticket(
    State(app): State<App>,
    user: AuthenticatedUser,
    Path(ticket_id): Path<i64>,
) -> Result<StatusCode, (StatusCode, String)> {
    let deleted = train_tracking::delete_ticket(&app.database, ticket_id, &user.id)
        .await
        .map_err(internal_error("delete ticket"))?;
    if !deleted {
        return Err((StatusCode::NOT_FOUND, "no ticket with that id".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}
```

`404` for both "no such ticket" and "exists but belongs to someone else"
— indistinguishable at this layer, same universal convention as every
other ownership check in this app (never `403`). `204 No Content` on
success, matching `delete_tracked_train`/`delete_line`.

**Two real alternatives considered and rejected:**

- **A route nested under the owning tracked train**
  (`DELETE /Train/{tracking_id}/tickets/{ticket_id}`), mirroring the
  older `{tracking_id}`-scoped ticket family (`POST`/`GET
  /Train/{tracking_id}/tickets`). **Rejected**: this shape cannot express
  "delete a standalone (unattached) ticket," since it requires a
  `tracking_id` that a standalone ticket doesn't have. The flat
  `/Train/tickets/{ticket_id}` shape already chosen for
  `/Train/tickets/{ticket_id}/attach` handles both cases uniformly with
  one route, matching that sibling's own reasoning
  (`crates/api/src/routes/train.rs:144-154`'s doc comment: *"Ownership-scoped
  on BOTH sides"*, no dependency on a pre-existing tracking id on the
  ticket itself).
- **A soft-delete flag instead of a real `DELETE`.** Considered briefly
  and rejected: nothing else in this table's schema, this app's
  conventions, or the brief calls for retention of a deleted ticket —
  `tracked_train_tickets` has no existing `deleted_at`-shaped column, no
  other table in this app soft-deletes (`delete_line`,
  `delete_tracked_train`, and the account-cascade delete are all hard
  deletes), and the legal/privacy audit comment on this exact table
  (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:9-15`)
  argues, if anything, for less retention of this data over time, not
  more. A hard delete is the right default here, consistent with the rest
  of the app.

### 3. Frontend: `DeleteTicketButton`, modeled on `DeleteTrainButton`, refreshing in place rather than navigating away

**Chosen: a new client component, `frontend/components/DeleteTicketButton.tsx`**,
taking `{ ticketId: number }`, closely modeled on
`DeleteTrainButton.tsx`'s confirm-modal/fetch/error-handling shape (same
`useDisclosure` confirm modal, same distinct `aria-label="Confirm delete"`
naming rationale for the two same-text "Delete" buttons, same
`useNeedsLogin`/`LoginLink` `401` handling, same generic-error-message
fallback for any other non-`ok` status) — **but calling
`router.refresh()` on success, not `router.push(...)`.**

This is a deliberate divergence from `DeleteTrainButton`'s own pattern,
reasoned from Current relevant state: `DeleteTrainButton` navigates away
because deleting a tracked train removes the entire subject of the page
it's rendered on (a single-train detail page) — there is nothing left on
that page to show. Deleting a *ticket*, by contrast, always happens from
inside a list of other things (a train detail page's other tickets, or
`/track/mine`'s other trains/tickets) that remain valid and worth showing
after the delete. `router.refresh()` — the exact mechanism
`AttachTicketAction.tsx:41` and `PinToggle.tsx:99` already use for the
same "mutate one row, stay on this page, let the Server Component re-fetch
supply the new truth" shape — re-runs the enclosing Server Component
(`TicketPanel`, or `app/track/mine/page.tsx`), which naturally drops the
now-deleted ticket from its next render with no separate client-side
list-splicing logic needed.

**Wiring, at all three places a ticket currently renders**, as a sibling
action alongside `TicketSummary` (matching `UnattachedTicketRow`'s own
existing pattern of placing `AttachTicketAction` as a sibling, not folded
into `TicketSummary` itself — `TicketSummary`'s prop type stays
deliberately narrow per Decision 1's reasoning, and per its own existing
doc comment, `frontend/components/TicketSummary.tsx:9-11`):

- `TicketPanel.tsx` (`frontend/components/TicketPanel.tsx:89-98`): add
  `<DeleteTicketButton ticketId={ticket.id} />` inside each ticket's
  `Stack`, alongside `TicketSummary`/`DelayRepayEstimate`.
- `app/track/mine/page.tsx`'s `TrackedTrainListRow` (attached tickets,
  `frontend/app/track/mine/page.tsx:157-172`): same addition, inside the
  per-ticket `Stack`.
- `app/track/mine/page.tsx`'s `UnattachedTicketRow` (standalone tickets,
  `frontend/app/track/mine/page.tsx:191-211`): same addition, alongside
  the existing `AttachTicketAction`/track-a-new-train `Group`
  (`frontend/app/track/mine/page.tsx:203-208`) — this is the row the brief
  specifically calls out, and it needs no different treatment from the
  other two: `delete_ticket`'s ownership check is identical regardless of
  attachment status.

**Scope: delete is available for a ticket regardless of attachment
status.** Considered restricting delete to only unattached tickets (on the
theory that an attached ticket is "more committed" state) and rejected: a
ticket's attachment to a tracked train is not itself protected by any
constraint in this codebase (the tickets-list spec's own upload-first flow
already lets a ticket move freely from unattached to attached via
`POST /Train/tickets/{ticket_id}/attach` — with no reverse "detach"
operation existing either), and there is no real product reason a user
would be allowed to discard an unattached ticket but forbidden from
discarding one they later attached. The ownership-scoped `DELETE` route
(Decision 2) applies uniformly to both cases by construction, so
restricting the frontend affordance to only one of them would be an
arbitrary UI restriction not backed by any backend constraint.

**What happens to a deleted ticket's Delay Repay estimate:** nothing
lingers, because nothing is stored in the first place. `estimate`/
`claimUrl`/`disclaimer` are computed fresh on every read — either per-row,
inline, inside `list_tickets_for_user`'s query
(`crates/api/src/data/train_tracking.rs:705-732`'s `build_ticket_list_item`)
for the list views, or per-request by `GET
/Train/{trackingId}/tickets/{ticketId}/delay-repay`
(`crates/api/src/routes/train.rs:205-221`) for `TicketPanel`'s own eager
fetch. Once the ticket row is gone, both call paths simply can't find it
on their next invocation: the list query's `WHERE t.user_id = $1` no
longer matches it, and the per-ticket route 404s
(`get_ticket_owned` returns `None`). Since `router.refresh()` (above)
re-runs the enclosing Server Component, which re-issues exactly these
reads, the deleted ticket's entire row — summary, badge, and Delay Repay
block alike — simply stops being part of the next render. No orphaned
estimate, no stale cached percentage, no separate cleanup step required
anywhere.

## Decision 4 (Request #3): opening the original document / rendering a QR code — this conflicts directly with an audited privacy constraint and is not designed here

**This is the load-bearing section of this spec.** Request #3, as stated
("open the original PDF, or — for `.pkpass` tickets — render the QR code
for scanning"), conflicts directly with an explicit, already-audited
constraint in this codebase, on more than one independent axis. This
section states the conflict precisely, reasons through what (if anything)
is actually buildable within the existing constraint, and names this as a
decision point for the user/product owner rather than resolving it
unilaterally.

### The constraint, quoted precisely

`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:9-15`,
the table's own header comment:

> "LEGAL/PRIVACY AUDIT (see this plan's Global Constraints): this table
> deliberately stores ONLY operator, ticket_type, origin_crs,
> destination_crs, source, and timestamps/ownership. It must NEVER gain a
> column for payment/price data, any barcode payload (raw or decoded), any
> ITSO data, passenger name, or the uploaded .pkpass/PDF file itself. Diff
> any future migration touching this table against this list before
> merging it."

This is not casual commentary. The originating plan's own Global
Constraints section restates it as one of a short list of hard rules:
*"Legal/privacy schema audit — a hard constraint, not just Task 1
prose... No barcode/ITSO decoding, anywhere, full stop"*
(`docs/superpowers/plans/2026-08-29-journey-ticket-tracking.md:32,35`).
The design doc behind it independently confirms the file itself is never
retained past the initial parse: *"no file retention past transient
parsing"* is listed as one of five load-bearing constraints in its own
Legal/privacy assessment
(`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md:449-456`).

A second, independent source in this codebase treats the barcode/ITSO
half of this line as a hard boundary, not an oversight:
`crates/api/src/data/ticket_extraction.rs:1-7`'s own module doc states
this module *"NEVER writes to the database... and NEVER decodes a barcode
or touches ITSO data, in either format"* — and the design doc's own
Non-goals section explains why, in concrete, researched terms, not as a
stylistic choice: *"RSP-6/Aztec barcode decoding... no official public
spec exists, and the only working decoder found in this research pass is
built on reverse-engineered RSA keys obtained by decompiling ticket
inspector apps — a materially different, and much riskier, legal posture
than reading an openly-documented file format"* and *"ITSO smartcard
data... gated behind ITSO Ltd's own accreditation/membership process, not
a public spec or open API"*
(`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-design.md:84-93`).

### Where request #3 conflicts

Request #3 as literally stated requires two things, both explicitly
forbidden:

1. **"Open the original PDF"** requires the original uploaded file to
   still exist somewhere DS controls, to be re-served on demand. The
   constraint forbids exactly this: *"the uploaded .pkpass/PDF file
   itself"* is named in the same sentence as payment data and barcode
   payloads as something the schema must never gain a column for, and the
   design doc's Legal/privacy assessment independently states "no file
   retention past transient parsing" as one of five conditions the whole
   feature's lower-risk classification depends on.
2. **"Render the QR code for scanning" from a `.pkpass`** requires (a)
   the original `.pkpass` file (or at minimum its barcode payload) to
   still exist somewhere DS controls, which is the same violation as (1),
   plus (b) *decoding* that barcode payload to extract the data a QR
   renderer would encode — which `ticket_extraction.rs` explicitly never
   does today, and which the design doc's own Non-goals section rejected
   for this app specifically because no legitimate implementation path
   exists (the only real-world decoder found in that research pass
   depends on reverse-engineered keys from decompiled apps).

### What is, and isn't, actually buildable within the existing constraint

**"Open the original PDF" cannot mean "DS stores and re-serves the file"
without a conscious reversal of the constraint — and there is no narrower
version of that specific request that avoids the conflict.** The
constraint is not "store it briefly" or "store a redacted version" — it is
categorical: the file itself must never be retained past the initial
parse. `ticket_extraction.rs`'s own framing (*"this module... NEVER writes
to the database"*) confirms there is no code path today, anywhere, that
persists the upload past the single request that parses it
(`parse_pkpass`/`parse_pdf`,
`crates/api/src/data/ticket_extraction.rs:106-118,355-363`, both take
`&[u8]` and return a `PartialTicket` — nothing is written to disk or to
the database, and the bytes go out of scope when the request handler
returns). Concretely: by the time a user is looking at a saved ticket row
on `/track/mine` or a train's detail page — which is the only place a
"open original PDF" button could ever appear — the request that briefly
held the PDF's bytes in memory ended, potentially days or weeks earlier.
**There is no "original" left anywhere in this system to re-open.** This
isn't a missing feature; it's the direct, intended consequence of the
review-before-save architecture this feature was built around. Building
"open the original PDF" as requested would require adding file storage
specifically to defeat that property — not a narrower implementation of
the same request, a different, unreviewed decision.

**The QR-code half is even more clearly out of reach, on a second,
independent axis.** Even setting aside file storage entirely, rendering a
QR code requires *decoded barcode data* to encode into the QR image.
`ticket_extraction.rs` explicitly never decodes a barcode from a `.pkpass`
today (confirmed above), and the design doc's Non-goals section rejected
building that capability for this app specifically, based on real
research into how such decoding would have to be done (reverse-engineered
keys from decompiled third-party apps — not a legally comparable
posture to reading Apple's openly-published `.pkpass` container format
for the fields this app already extracts). So building "render the QR
code" would require **both** forbidden things at once: storing/retaining
the file or its barcode payload (already flagged above), **and** adding a
barcode-decoding capability this codebase's own design research
explicitly investigated and declined to build, for reasons independent of
data retention. There is no data anywhere in this system today — even
transiently, even mid-request — that represents a decoded barcode; there
is nothing to render as a QR code without first building the thing the
design doc already said no to.

**Is there any narrower version that doesn't require persisting the
forbidden data?** Honestly: no version of "open the original document" or
"show a scannable code" was found that avoids needing either the file
itself or its barcode payload to exist somewhere past the initial parse.
A QR code that encodes only the fields this table already stores
(operator, ticket type, origin/destination CRS) would not be a valid
ticket barcode — it would not scan as anything a conductor's reader
expects, and presenting it as if it might would be actively misleading, a
different and worse problem than simply not offering the feature. No
narrower, honest interpretation of request #3 was found that both (a)
delivers something recognisable as "open the original" or "show a QR
code" and (b) respects the constraint.

### This is a named decision point, not a design conclusion

Per the explicit instruction this spec was written under: this document
does **not** decide whether to build request #3. Two real options exist,
and choosing between them is a product/legal decision, not a design
decision:

- **(a) Request #3 genuinely cannot be built as stated without a
  deliberate, consciously-made reversal of the audited privacy
  constraint** — i.e., a decision to start retaining the uploaded file
  (and, for the QR-code half, to also start decoding barcode data, a
  second capability this app has never had). If that reversal is what the
  product wants, it needs the same kind of explicit, named sign-off this
  session's other legal-adjacent findings have been routed to — re-opening
  `crates/api/migrations/20260829090000_journey_ticket_tracking.sql`'s own
  audit line, the design doc's Legal/privacy assessment, and the Non-goals
  section's barcode-decoding rejection, all with eyes open, not as a side
  effect of implementing a UI button. This spec does not make that call.
- **(b) A narrower version that doesn't require persisting the forbidden
  data.** No such version was identified during this research pass (see
  above) — this spec is not aware of an implementation of "open the
  original" or "render a QR code" that stays inside the existing
  constraint and still delivers something real. If the user/product owner
  believes one exists that this research missed, that needs to be
  identified explicitly before any implementation planning starts; it is
  not something this spec can respons­ibly invent to appear more complete.

**No part of request #3 is designed, sketched, or implied as
implementation below.** This section is deliberately the entire scope of
this spec's treatment of it.

## Architecture

Decisions 1–3 only (Decision 4 adds nothing to build):

```
┌─────────────────────────────────────────────────────────────────────┐
│ frontend/                                                               │
│                                                                            │
│  components/TicketSummary.tsx     CHANGED -- widened Pick<...> to add   │
│                                      source/createdAt; renders a         │
│                                      provenance Badge + "Added ..." line │
│                                      (Decision 1, no backend change)     │
│                                                                            │
│  components/DeleteTicketButton.tsx  NEW -- modeled on DeleteTrainButton, │
│                                      DELETE /api/Train/tickets/{id},     │
│                                      router.refresh() on success         │
│                                      (Decision 3)                        │
│                                                                            │
│  components/TicketPanel.tsx          + <DeleteTicketButton>              │
│  app/track/mine/page.tsx             + <DeleteTicketButton> in both      │
│                                         TrackedTrainListRow (attached)   │
│                                         and UnattachedTicketRow          │
└──────────────────────────┬────────────────────────────────────────────────┘
     same-origin proxy     │  DELETE /api/Train/tickets/{ticketId}
     (frontend/app/api/    │  -> passthrough, no /public/ prefix
     [...path]/route.ts)   ▼  (existing /Train/... allowlist, unchanged)
                 ┌──────────────────────────────────────────────┐
                 │ crates/api                                       │
                 │  DELETE /Train/tickets/{ticket_id}   NEW route,  │
                 │    AuthenticatedUser-gated                       │
                 │    -> train_tracking::delete_ticket   NEW query, │
                 │       mirrors delete_tracked_train exactly       │
                 │       (Decision 2)                               │
                 └──────────────────────────────────────────────┘
```

## Error handling

- **`DELETE /Train/tickets/{ticketId}`**: bare `401` (no session, handled
  by the `AuthenticatedUser` extractor itself, before the handler runs);
  `404` for both "no such ticket" and "exists but belongs to someone
  else" (indistinguishable at this layer, universal app convention, never
  `403`); `204 No Content` on success. No other status is possible from
  this route — it has no request body to validate, so there is no `400`
  case.
- **`DeleteTicketButton`**: `401` is caught specifically and shown via the
  established `useNeedsLogin`/`LoginLink` pattern (never the raw backend
  rejection text) — same as `DeleteTrainButton`. Any other non-`ok` status
  (`404`, `5xx`, network failure) falls back to a generic error message
  shown inside the confirm modal, same as `DeleteTrainButton`. A `404`
  here can only really happen from a double-click/stale-render race (the
  button only ever renders for a ticket the enclosing Server Component
  just fetched and confirmed exists) — not treated as a distinguishable
  case from any other failure, same posture `DeleteTrainButton` already
  takes for its own narrow `401` race.
- **`TicketSummary`'s widened rendering** introduces no new error path —
  `source` and `createdAt` are both non-optional on both wire shapes
  (Current relevant state), so there is no `null`/missing-field case for
  this component to guard against, unlike `operator`/`ticketType`/the CRS
  fields, which are already optional and already handled.

## Testing

Following this repo's existing convention (colocated `*.test.tsx`/`*.test.ts`,
`renderWithMantine`, Vitest; Rust `#[cfg(test)]` modules colocated in the
same file):

- **`crates/api/src/data/train_tracking.rs`**: unit/integration tests for
  `delete_ticket` mirroring `delete_tracked_train`'s own coverage shape —
  owner deletes their own ticket (returns `true`, row gone), a different
  user's attempt (returns `false`), a nonexistent ticket id (returns
  `false`), and — new relative to `delete_tracked_train`, since a ticket
  can be standalone — deleting an *unattached* ticket succeeds identically
  to deleting an attached one (no special-casing on `tracked_train_id`).
- **`crates/api/src/routes/train.rs`**: route-level tests mirroring the
  existing `delete_tracked_train` test block
  (`crates/api/src/routes/train.rs:1129-1231`, the `TEST-DELETE-*` seeded
  cases) — `401` with no session, `404` for another user's ticket, `404`
  for a nonexistent ticket id, `204` for the real owner, and confirmation
  that a deleted ticket subsequently 404s from every other ticket-reading
  route (`GET /Train/tickets/mine`'s list no longer includes it; `GET
  .../tickets/{ticketId}/delay-repay` 404s), directly exercising the "no
  orphaned estimate" claim in Decision 3.
- **`components/TicketSummary.test.tsx`**: extend for the two new fields
  — a `source` badge renders with the correct label for each
  `TicketSource` variant, `createdAt` renders via `formatDateTime`, and
  existing assertions about the four original fields still pass unchanged
  (a pure addition, not a rewrite).
- **`components/DeleteTicketButton.test.tsx`**: mirroring
  `DeleteTrainButton.test.tsx`'s existing shape — confirm-modal
  open/cancel, successful delete calls `router.refresh()` (not
  `router.push`, the deliberate divergence from `DeleteTrainButton`),
  `401` shows the login prompt, other failures show the generic error
  text.
- **`app/track/mine/page.tsx` / `TicketPanel.tsx`**: render tests
  confirming `<DeleteTicketButton>` is present for both an attached and
  an unattached ticket row, wired with the correct `ticketId`.

## Explicitly out of scope

- **Everything in Decision 4 (request #3)** — no code, migration, or
  route is designed for opening the original document or rendering a QR
  code. This is the deliberate point of that section, not an omission.
- **Editing a saved ticket's fields.** Still no `PUT` route anywhere in
  the ticket family (unchanged from the tickets-list spec's own
  "Explicitly out of scope") — this spec adds `DELETE` only.
- **Bulk/multi-select delete.** One `DeleteTicketButton` per ticket row;
  no "select several tickets and delete them together" affordance is
  designed.
- **A "detach" operation** (moving an attached ticket back to
  unattached without deleting it). Not requested, and no route exists for
  it today; a user who wants to detach a ticket from the wrong train has
  no path other than deleting and re-adding it — noted as a real gap but
  not this spec's concern.
- **Undo/soft-delete/grace-period recovery** for a deleted ticket.
  Rejected as a real feature (Decision 2) in favour of a straightforward
  hard delete behind a confirm modal, consistent with every other delete
  in this app.
- **Any retention/pruning job for `tracked_train_tickets`.** Unchanged
  from the tickets-list spec's own scope — this spec adds a
  user-initiated delete, not an automated one.
- **Rewording/relabelling `TicketSource`'s existing four values**
  (`manual`/`pkpass-semantics`/`pkpass-heuristic`/`pdf-heuristic`)
  anywhere outside the new frontend display label map — the wire values
  themselves, and the `CHECK` constraint that enforces them
  (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:46-47`),
  are unchanged.

## Open questions / risks

1. **Request #3's disposition is fully unresolved and is this spec's
   central open item, not a minor footnote.** Decision 4 lays out why
   neither half of the literal request can be built inside the existing
   constraint, and that no narrower honest version was found either. This
   needs an explicit answer from the user/product owner — reverse the
   constraint deliberately (with the same weight of sign-off its original
   audit received), accept that this feature genuinely can't be delivered
   as asked, or identify a narrower interpretation this research pass
   missed. This spec takes no position on which.
2. **`SOURCE_LABELS`' exact wording (Decision 1)** is a naming detail, not
   researched against real user feedback — flagged as a reasonable
   starting point, not a final answer, same posture this session's other
   specs take for un-load-bearing copy choices.
3. **No "detach" operation exists**, so a user who wants to move a ticket
   from the wrong tracked train to the right one has to delete and
   re-enter it, losing the original `source`/`createdAt` provenance in
   the process (a freshly re-entered ticket gets `source: 'manual'` and a
   new `createdAt`, not the original values). Not designed here; flagged
   as a real, if minor, follow-up.
4. **`DeleteTicketButton`'s placement inside `TicketPanel.tsx`'s existing
   `Stack`/`Divider` layout** (`frontend/components/TicketPanel.tsx:89-97`)
   is a mechanical composition detail left to implementation — whether it
   sits inline with the route text, on its own line, or grouped with a
   future "detach" affordance is not decided here.
