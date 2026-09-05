# Design: Custom Names for Tracked Trains and Tickets

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`
and `docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md` — no
implementation plan is included; that is a separate, later step in this
repo's process.

The ask: let a user set a custom display name for a tracked train and for a
ticket, so their `/track/mine` list stops looking like a wall of identical
route/date rows once they track more than one thing. A user who never
bothers to rename anything must still get something readable — a sane
default derived from origin, destination, and date/time — never a bare id.

## Current relevant state (verified 2026-09-05)

**Schema — no name column exists on either table today.**
`grep -n "ALTER TABLE tracked_train" crates/api/migrations/*.sql` turns up
exactly one hit, `20260901140000_standalone_tickets.sql:31`
(`ALTER TABLE tracked_train_tickets ALTER COLUMN tracked_train_id DROP NOT
NULL`) — nothing has ever touched either table to add a name/nickname
column. This is confirmed clean net-new territory, not a retrofit.

- `tracked_trains` (`crates/api/migrations/20260828120000_train_tracking.sql:39-58`):
  `id`, `user_id`, `service_date`, `pin_origin_crs`,
  `pin_scheduled_departure`, `pin_destination_crs`, `pin_operator`,
  `train_uid`, `train_id`, `resolution_status`, `tracked_at`,
  `resolved_at`. No free-text user-authored column exists here at all.
- `tracked_train_tickets` (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:26-38`,
  amended by `20260901140000_standalone_tickets.sql:31` to make
  `tracked_train_id` nullable): `id`, `tracked_train_id`, `user_id`,
  `operator`, `ticket_type`, `origin_crs`, `destination_crs`, `source`,
  `created_at`. `operator`/`ticket_type` are the closest existing things to
  free text, but both are described in their own column comments as
  "free text or a known operator code" / an enum-like set of examples, not
  a user-authored label field.

**The legal/privacy audit list — must be checked explicitly, not glossed.**
Both ticket migrations carry the same "LEGAL/PRIVACY AUDIT" comment
(`20260829090000_journey_ticket_tracking.sql:9-13`, carried forward
verbatim by `20260901140000_standalone_tickets.sql:16-22`): this table
"must NEVER gain a column for payment/price data, any barcode payload (raw
or decoded), any ITSO data, passenger name, or the uploaded .pkpass/PDF
file itself." See Decision 2 below — resolved explicitly, not skipped.

**Precedent for a free-text user-authored column: `custom_lines.name`.**
`crates/api/migrations/20260709100000_custom_lines.sql:24` —
`name TEXT NOT NULL` — no `CHECK` constraint, no length limit at the
database layer. The only app-layer validation anywhere in this codebase
for a `name`-shaped field is "not empty after trimming"
(`crates/api/src/routes/lines.rs:230` in `create_line`, `crates/api/src/routes/lines.rs:320`
in `update_line` — identical one-liner in both). Grepping
`crates/api/src` for `MAX.*NAME`, `name.len() >`, or any length-bounding
logic on a name field returns nothing. **There is no existing precedent in
this schema for a user-authored free-text column with a length cap** —
whatever bound this spec proposes (Decision 1) is a new precedent, not a
copy of an established pattern, and is called out as such.

**Existing rename precedent: `PUT /lines/{id}` → `update_custom_line`.**
This codebase already has exactly the update mechanism this ask needs, just
for a different table:
- Route: `crates/api/src/routes/lines.rs:28-36` mounts
  `.route("/lines/{id}", axum::routing::get(get_line).put(update_line).delete(delete_line))`.
- Handler: `crates/api/src/routes/lines.rs:308-353` (`update_line`) takes
  `AuthenticatedUser`, validates `req.name.trim().is_empty()`, then calls
  `custom_lines::update_custom_line`.
- Query: `crates/api/src/data/custom_lines.rs:181-216`
  (`update_custom_line`) — `UPDATE custom_lines SET name = $2, ... WHERE id
  = $1 AND user_id = $7`, ownership folded directly into the `WHERE`
  clause (never a separate ownership lookup followed by an unscoped
  write), returns `Option<CustomLine>` (`None` if the id doesn't exist or
  isn't the caller's — same 404-covers-both convention as every other
  mutation in this app).

By contrast, `tracked_trains`/`tracked_train_tickets` today support only
create + read + delete. `crates/api/src/data/train_tracking.rs` has no
`update_*`/`rename_*` function at all (confirmed by
`grep -n "fn update_\|fn rename_" crates/api/src/routes/*.rs crates/api/src/data/*.rs`
— the only hits are `custom_lines::update_custom_line` and the string
"renamed" inside doc comments/test fixtures). A new route is required; there
is nothing to extend.

**Existing ownership pattern for `tracked_trains`/tickets, to replicate.**
- `crates/api/src/data/train_tracking.rs:534-541` (`delete_tracked_train`):
  `DELETE FROM tracked_trains WHERE id = $1 AND user_id = $2`, returns
  `bool`.
- `crates/api/src/data/train_tracking.rs:622-631` (`tracked_train_owner`):
  used by ticket-creation routes to check "does this tracked train exist
  AND belong to the caller" before writing a dependent row.
- Ticket ownership is redundant-by-design directly on
  `tracked_train_tickets.user_id` (migration comment,
  `20260829090000_journey_ticket_tracking.sql:31-36`), so every ticket
  mutation filters `WHERE id = $1 AND user_id = $2` with no join — see
  `attach_ticket_to_tracked_train` (`crates/api/src/data/train_tracking.rs:681-698`).
- Every route in `crates/api/src/routes/train.rs` that mutates uses
  `AuthenticatedUser` (`crates/api/src/auth.rs:239-244`) and the universal
  "exists but not yours → 404, never 403" convention
  (`crates/api/src/routes/train.rs:1-13` module doc).

A new rename route for either table follows this exact shape: `Authenticated
User`, `WHERE id = $1 AND user_id = $2` folded into the `UPDATE`, 404 for
"doesn't exist or isn't yours."

**Frontend — where these are rendered today, and the existing default-label
logic to reuse.**
- `frontend/app/track/mine/page.tsx` is the single merged list for both
  tracked trains and tickets (`/track/mine`, "My Trains & Tickets") — see
  its own module doc (`frontend/app/track/mine/page.tsx:21-46`) explaining
  why trains and their attached tickets, plus unattached tickets, are one
  page, not two. This directly answers essential-context point 6: trains
  and tickets ARE shown together on one surface, nested (a train's card
  contains its attached tickets; unattached tickets get their own
  section below).
- **Tracked-train default label, already built and already exactly what
  the ask wants**: `TrackedTrainListRow`
  (`frontend/app/track/mine/page.tsx:121-187`) renders
  `routeLabel(train.pinOriginCrs, train.pinOriginName, train.pinDestinationCrs,
  train.pinDestinationName)` as the row's bold title
  (`frontend/app/track/mine/page.tsx:133-138,151`), with
  `formatDate(train.serviceDate)} · {formatTime(train.pinScheduledDeparture)}`
  underneath (`frontend/app/track/mine/page.tsx:155`).
  `TrainJourney.tsx:18-24` (the single-train detail page) builds the
  equivalent `pinSummary` the same way. Both call sites already compute,
  at render time, from data already present on the wire type, precisely the
  "origin → destination, date/time" string the ask describes as the sane
  default (e.g. "London Kings Cross (KGX) → Bristol Temple Meads (BRI), 10
  May 2026 · 14:32").
- The two helpers behind this: `frontend/lib/stationLabel.ts:19-28`
  (`routeLabel`) — `"A (AAA) → B (BBB)"`, or just the origin if there's no
  destination yet — and `frontend/lib/dateFormat.ts` (`formatDate`,
  `formatTime`) — both London-wall-clock, `Intl.DateTimeFormat`-backed,
  module-level constants (see that file's own header on why timezone must
  be pinned explicitly).
- **Ticket default label, already built**: `TicketSummary.tsx:57-63` (shared
  by `TicketPanel.tsx` and `app/track/mine/page.tsx`) renders
  `{ticket.operator ?? 'Ticket'}{ticket.ticketType ? " — ${ticketType}" :
  ""}`, then a route line if either CRS is present. This is a weaker
  default than the tracked-train one (no date/time — a ticket has none of
  its own; `TicketSummary`'s prop type doesn't even carry
  `pinScheduledDeparture`), but it is the existing fallback shape and this
  spec extends it rather than inventing new copy (Decision 3).
- Rename-adjacent UI precedent: `DeleteTrainButton.tsx` and
  `DeleteTicketButton.tsx` are small client components, each a `Button`
  that opens a `Modal`, calls `fetch('/api/Train/...', { method: ... })`
  through the same-origin proxy (`app/api/[...path]/route.ts`), handles a
  401 via the existing `useNeedsLogin`/`LoginLink` pattern, and refreshes
  or navigates on success. No inline-edit-in-place pattern (click text,
  edit in a place, save on blur) exists anywhere in this frontend today —
  every mutation this app has is a button that opens a modal or a
  full-page form. A rename control should follow that same convention, not
  invent a new interaction pattern this app doesn't otherwise use.
- `frontend/lib/types.ts:322-341` (`TrackedTrainState`),
  `:350-364` (`TrackedTrainListItem`), `:405-418` (`TrackedTrainTicket`),
  `:520-540` (`TicketListItem`) — the four wire shapes that would each gain
  a `customName: string | null` field.

## Decisions

### 1. Schema: one nullable `custom_name` column on each table, capped at 100 characters, app-layer validated only

Add:

```sql
-- crates/api/migrations/<new-timestamp>_custom_tracking_names.sql
ALTER TABLE tracked_trains ADD COLUMN custom_name TEXT;
ALTER TABLE tracked_train_tickets ADD COLUMN custom_name TEXT;
```

Both nullable, both no `DEFAULT` — `NULL` means "no custom name set, render
the computed default" (see Decision 3), which is the overwhelming common
case for a user who never bothers to rename anything. This mirrors the
`Option<String>`-typed nullable columns already all over both tables
(`pin_destination_crs`, `pin_operator`, `train_uid`, every ticket field
except `id`/`tracked_train_id`(now nullable too)/`user_id`/`source`/
`created_at`) rather than introducing a new "empty string means unset"
convention this schema doesn't otherwise use.

**Length cap: 100 characters, enforced in Rust (`validate_pin`-style
free function), not a database `CHECK` constraint.** Reasoning:

- No precedent in this schema uses a DB-level `CHECK` for string length on
  a free-text column (`custom_lines.name` has none — see Current relevant
  state above), and every existing validation of a similarly-shaped field
  (`origin_crs` in `validate_pin`, `name` in `update_line`/`create_line`)
  lives in the Rust route/data layer, returning the same
  `(StatusCode::BAD_REQUEST, String)` shape every other 400 in this API
  uses. A DB `CHECK` failure would instead surface as an opaque 500
  (`internal_error`, `crates/api/src/routes/train.rs:731-739` truncated in
  citation), which is strictly worse UX and inconsistent with this file's
  own established pattern of app-layer validation with human-readable
  messages.
- 100 characters is a new, un-researched round number — flagged the same
  way `MAX_PIN_AGE`/`MINE_LIST_LIMIT` are flagged in
  `crates/api/src/data/train_tracking.rs:14-38` as "reasonable-sounding,
  not researched or load-tested" — generous enough for something like "My
  commute — London Paddington to Bristol, before the 08:12 got binned last
  time" while still bounding abuse (a multi-KB paste into a display-name
  field). Trim + reject-if-empty-after-trim (so whitespace-only can't
  masquerade as "custom name set" and permanently hide the useful
  default), matching `validate_pin`'s own `origin_crs.trim().is_empty()`
  check (`crates/api/src/data/train_tracking.rs:48-50`).
- Empty string is normalized to `NULL` server-side on write (clearing a
  name is "unset the custom name, go back to the computed default," never
  "set the custom name to the empty string" — the latter has no useful
  meaning and would need its own fallback logic everywhere `customName` is
  read).

### 2. Privacy audit: a ticket's `custom_name` does not violate the LEGAL/PRIVACY AUDIT list — resolved explicitly

The audit list (`20260829090000_journey_ticket_tracking.sql:9-13`, carried
forward by `20260901140000_standalone_tickets.sql:16-22`) bans: payment/
price data, any barcode payload (raw or decoded), any ITSO data, **passenger
name**, or the uploaded `.pkpass`/PDF file itself.

**A user-chosen custom name for their own ticket entry is not "passenger
name," and this spec states why rather than assuming it:**

- "Passenger name" in that audit's context is PII *extracted from the
  ticket itself* — the traveller's identity as printed on a barcode, ITSO
  card, or `.pkpass`/PDF payload, i.e. data about a real person that this
  codebase has deliberately kept out of its schema because it never needs
  it and holding it is a liability with no product benefit. This is why
  the audit groups it with barcode/ITSO/payment data: all four are
  *extracted-from-the-source-document* categories, not user input.
- `custom_name` is the opposite in every relevant respect: it is typed by
  the tracking user themselves, describing *their own list entry* (e.g.
  "Mum's ticket to Leeds," "Commute — Tuesday," "Refund attempt #2"), never
  parsed or inferred from the uploaded document, and never populated by
  `ticket_extraction.rs` (which this spec does not touch — see Non-goals).
  It is exactly the same category of thing as `custom_lines.name` (Current
  relevant state above): a label the owning user picks for their own
  record, with no connection to any third party's identity.
- It is still worth being honest that a user COULD type a passenger's name
  into this field voluntarily (e.g. "Sarah's return ticket") — this is a
  real but unavoidable property of any free-text field a user controls
  (the same is already true of `operator`/`ticket_type`, both free text
  today per their own column comments). This is qualitatively different
  from the audited risk, which is about this system *extracting and
  storing* identity data from a document without being asked to — not
  about a user electing to type a name into a box they were told is a
  label for their own use. No other free-text field in this schema is held
  to a "user might voluntarily type something sensitive into it" standard
  either.
- **Conclusion: `tracked_train_tickets.custom_name` does not run afoul of
  the audit list.** The migration adding it should say so explicitly, in
  the same style as the two prior migrations' own audit comments, so a
  future reader auditing this table again doesn't have to re-derive this
  reasoning — see the migration sketch below.

Migration comment sketch (for the implementation plan, not written here):

```sql
-- LEGAL/PRIVACY AUDIT ADDENDUM (see 20260829090000_journey_ticket_tracking.sql's
-- and 20260901140000_standalone_tickets.sql's own audit comments): this
-- migration adds `custom_name`, a nullable, user-authored display label
-- the tracking user types for their OWN list entry (e.g. "Mum's ticket to
-- Leeds"). This is NOT "passenger name" in the sense that comment bans --
-- that ban targets PII *extracted from the ticket document itself*
-- (barcode/ITSO/pkpass payload), never anything a user types about their
-- own record. `custom_name` is never populated by `ticket_extraction.rs`
-- and carries no connection to a third party's identity. See
-- docs/superpowers/specs/2026-09-05-custom-tracking-names-design.md's
-- Decision 2 for the full reasoning. The audit list itself is unchanged --
-- this table still must never gain payment/price data, any barcode
-- payload, ITSO data, passenger name (as extracted from a document), or
-- the uploaded file itself.
ALTER TABLE tracked_train_tickets ADD COLUMN custom_name TEXT;
```

### 3. Default name: computed client-side at render time, never persisted as a literal fallback string

**Not stored server-side.** Reasoning, grounded in this codebase's own
existing pattern rather than asserted generically:

- `frontend/app/track/mine/page.tsx:133-138,151,155` and
  `frontend/components/TrainJourney.tsx:18-24` **already do exactly this**
  — both compute `routeLabel(...) + formatDate/formatTime(...)` fresh on
  every render, directly from fields already present on
  `TrackedTrainListItem`/`TrackedTrainState`, rather than the backend
  writing a pre-formatted summary string into the database at pin-creation
  time. This is the established precedent this spec is asked to match
  (essential-context point 5), and it already exists for the exact fields
  (origin, destination, service date, scheduled departure) the ask's own
  example ("London Paddington → Bristol Temple Meads, 14:32 5 Sep") needs.
- Storing a literal default server-side would need re-derivation logic
  anyway: `pin_origin_name`/`pin_destination_name` are joined in from
  `stations` at *read* time (`TRACKED_TRAIN_STATE_SELECT`,
  `crates/api/src/data/train_tracking.rs:408-417`, `LEFT JOIN stations ...
  ON so.crs = UPPER(...)`), specifically so a station whose reference data
  arrives *after* the pin was created still resolves to a real name
  instead of a bare CRS code (see that `SELECT`'s own comment,
  `crates/api/src/data/train_tracking.rs:397-407`, on why `LEFT JOIN`, not
  `JOIN`). A default name computed once at creation and stored as a
  literal would freeze "KGX" into the row forever if the station reference
  data hadn't loaded yet at that moment, permanently missing the backfill
  this join already provides for free. Client-side, render-time
  computation gets this backfill for free, every time, forever — no
  re-generation job, no "the default looks stale" bug class.
- It also avoids a real correctness trap: `pin_destination_crs` can be
  `NULL` at pin-creation time and get filled in later (pre-match pins
  genuinely have no destination yet — `TrainJourney.tsx:16-17`'s own
  comment). A stored-at-creation default would show "London Paddington"
  forever even after the destination becomes known; a render-time default
  updates automatically the next time the row is fetched.

**Where it's computed**: a new small helper, colocated with
`routeLabel`/`stationLabel` in `frontend/lib/stationLabel.ts` (or a
sibling `trackingName.ts` if the maintainers prefer not to grow that file
past its current single concern — a call for the implementation plan, not
this spec), something in the shape of:

```ts
// Tracked train: reuses routeLabel + formatDate/formatTime exactly as
// TrackedTrainListRow/TrainJourney already do — no new formatting logic.
export function trackedTrainDisplayName(train: {
  customName: string | null;
  pinOriginCrs: string;
  pinOriginName: string | null;
  pinDestinationCrs: string | null;
  pinDestinationName: string | null;
  serviceDate: string;
  pinScheduledDeparture?: string; // absent on TrackedTrainState today
}): string {
  if (train.customName) return train.customName;
  const route = routeLabel(train.pinOriginCrs, train.pinOriginName, train.pinDestinationCrs, train.pinDestinationName);
  const when = train.pinScheduledDeparture
    ? `${formatDate(train.serviceDate)} · ${formatTime(train.pinScheduledDeparture)}`
    : formatDate(train.serviceDate);
  return `${route}, ${when}`;
}
```

Note `TrackedTrainState` (the single-train detail page's wire shape) has no
`pinScheduledDeparture` field at all (`frontend/lib/types.ts:319-320`'s own
comment: "there is no `scheduledDeparture` field — the backend's read query
does not select `pin_scheduled_departure`, only `serviceDate`"), so the
helper must degrade to date-only there, exactly as `TrainJourney.tsx`'s
existing `pinSummary` already does — this is not a new gap introduced by
this feature, it already exists in the current default rendering.

**Ticket default** extends `TicketSummary.tsx:57-63`'s existing
`operator`/`ticketType`/route rendering rather than replacing it: if
`customName` is set, render it as the bold title in place of `{operator ??
'Ticket'}{ticketType...}`; the route sub-line stays exactly as-is either
way (a custom name replaces the "what is this" line, not the "where does
it go" line, since even a renamed ticket's route is still useful
at-a-glance information the operator/type line duplicates less than the
route line does).

### 4. API surface: two new `PATCH` routes, one per table, following the existing rename precedent exactly

No existing update route exists for either table (Current relevant state
above) — both are new. Modeled directly on `PUT /lines/{id}` →
`update_custom_line`, using `PATCH` rather than `PUT` since this is a
partial update of one field on an otherwise-immutable-by-this-route row
(unlike `update_custom_line`, which replaces every editable field of a
custom line at once — there's no equivalent "replace everything" concept
for a tracked train's pin data, which is written once at creation and then
only ever mutated by `trust-consumer`'s event pipeline).

```
PATCH /Train/{trackingId}/name
  Body:  { "customName": string | null }   // null/omitted-then-empty clears it
  Auth:  AuthenticatedUser, WHERE id = $1 AND user_id = $2 folded into UPDATE
  200:   { "customName": string | null }   // the normalized value actually stored
  400:   trimmed length > 100 chars (message carries no `_`, per
         validate_pin's own established convention,
         crates/api/src/data/train_tracking.rs:591-611's test)
  404:   doesn't exist, or isn't the caller's (never 403)

PATCH /Train/tickets/{ticketId}/name
  Body:  { "customName": string | null }
  Auth:  AuthenticatedUser, WHERE id = $1 AND user_id = $2 (no join needed --
         same ownership-redundancy this table already has for every other
         mutation, per its own migration comment)
  200/400/404: same shape as above
```

Routed as a sub-path (`/name`), not a bare `PATCH /Train/{trackingId}`
that could later grow to accept other partial-update fields — this mirrors
this router's own existing style of one narrow route per concern (compare
`/Train/{tracking_id}/tickets/{ticket_id}/delay-repay`,
`/Train/tickets/{ticket_id}/attach`) rather than a general-purpose PATCH
endpoint this codebase doesn't otherwise have anywhere.

Backend additions, mirroring `update_custom_line` exactly:

- `crates/api/src/data/train_tracking.rs`: `validate_custom_name(name: &str)
  -> Result<(), String>` (trim, empty-after-trim ok — that's "clear it",
  length check the only rejection), plus `rename_tracked_train(pool, id,
  user_id, custom_name: Option<&str>) -> anyhow::Result<bool>` (`UPDATE
  tracked_trains SET custom_name = $1 WHERE id = $2 AND user_id = $3`,
  `bool` return matching `delete_tracked_train`'s own shape) and
  `rename_ticket(pool, ticket_id, user_id, custom_name: Option<&str>) ->
  anyhow::Result<bool>` (same shape against `tracked_train_tickets`).
- `crates/api/src/routes/train.rs`: two new handlers, `patch_tracked_train_name`
  and `patch_ticket_name`, each `AuthenticatedUser` + the validate-then-
  write-then-404-if-no-rows-affected shape every other mutation in this
  file already uses.
- `TrackedTrainState`, `TrackedTrainListItem`, `TrackedTrainTicket`,
  `TicketListItem` (all four structs in `crates/api/src/data/train_tracking.rs`
  cited above) each gain `pub custom_name: Option<String>`, and their
  backing `SELECT`s (`TRACKED_TRAIN_STATE_SELECT`, `list_tracked_trains_for_user`'s
  query, `TICKET_SELECT`, `list_tickets_for_tracked_train`'s and the
  mine-list's query) each add `tt.custom_name` / `t.custom_name` to their
  column list. `TrackedTrainRef` (the poller-facing, trust-consumer-only
  shape) does NOT need it — that struct exists purely for movement-message
  matching and never reaches a user-facing surface.
- `frontend/lib/types.ts`: `customName: string | null` added to all four
  corresponding interfaces (`TrackedTrainState`, `TrackedTrainListItem`,
  `TrackedTrainTicket`, `TicketListItem`).
- `frontend/lib/api.ts`: two new thin wrapper functions,
  `renameTrackedTrain(trackingId, customName)` and `renameTicket(ticketId,
  customName)`, following the same same-origin-proxy `fetch('/api/Train/...',
  { method: 'PATCH', body: ... })` shape `DeleteTrainButton.tsx`/
  `DeleteTicketButton.tsx` already use directly inline (those two don't
  route through `lib/api.ts` at all today — a call for the implementation
  plan whether the new rename components follow that same inline-fetch
  precedent or centralize in `lib/api.ts`; either is consistent with
  something already in this codebase).

### 5. Frontend UI: a small "Rename" button next to each row's title, opening a modal with one text input — matches this app's own established mutation pattern, not an invented inline-edit pattern

Per Current relevant state above, this frontend has zero inline-edit-in-
place components anywhere (every mutation is a button → modal → fetch →
refresh/navigate, per `DeleteTrainButton.tsx`/`DeleteTicketButton.tsx`).
This spec follows that convention rather than introducing click-to-edit
text, for consistency and because it's already a proven, tested pattern in
this exact list.

Placement:

- **`TrackedTrainListRow`** (`frontend/app/track/mine/page.tsx:121-187`):
  add a small `RenameTrainButton` next to `RowStatusBadge` in the header
  `Group` (`frontend/app/track/mine/page.tsx:150-153`) — outside the
  `<Link>` wrapping the rest of the header (same reason `DeleteTrainButton`
  already lives outside that link on the single-train detail page: a
  button inside an anchor is invalid HTML, and this row's whole header is
  already a navigation link). Title text becomes
  `trackedTrainDisplayName(train)` (Decision 3) instead of the bare
  `routeLabel(...)` call at `frontend/app/track/mine/page.tsx:151`.
- **Single-train detail page** (`TrainJourney.tsx`, rendered by
  `frontend/app/train/by-id/[trackingId]/page.tsx` and
  `frontend/app/train/[uid]/[date]/page.tsx`): add the same
  `RenameTrainButton` near `DeleteTrainButton`, which those two page
  components already render alongside `<TrainJourney>` (grounds this in
  the existing page composition rather than pushing the button inside
  `TrainJourney.tsx` itself, which today takes only `state` as a prop and
  has no mutation concerns).
- **`TicketSummary`** (used by both `TicketPanel.tsx` and
  `app/track/mine/page.tsx`'s two call sites): add a `RenameTicketButton`
  next to `DeleteTicketButton`, which is already rendered as a sibling at
  every one of `TicketSummary`'s three current call sites
  (`frontend/app/track/mine/page.tsx:179,217`, `TicketPanel.tsx`) — the
  same "actions row below the summary" placement `DeleteTicketButton`
  already established, not a new location.
- No rename control on the creation forms (`TrackTrainForm.tsx`,
  `TicketEntryForm.tsx`) — a user naming something before it exists is a
  reasonable future enhancement (see Open Questions) but is out of scope
  for a v1 whose whole point is "give existing untitled rows a name," and
  adding it to two already-complex forms is a separable follow-up, not a
  blocker for the rename-after-the-fact case that actually motivates this
  request.

`RenameTrainButton`/`RenameTicketButton` (two small, near-identical
components, or one generic `RenameButton` parameterized by the fetch URL —
an implementation-plan call): `Button` (small, `variant="subtle"` or
similar to stay visually secondary next to `DeleteTrainButton`'s existing
`variant="outline" color="red"`) → `Modal` with a single `TextInput`
pre-filled with the current `customName` (or empty if unset, with
placeholder text showing what the computed default currently is, so the
user sees exactly what they're overriding) → `Save` posts
`PATCH .../name`, `Clear` (visible only when a custom name is currently
set) posts `{ customName: null }` → on success, `router.refresh()` (List
Server Component re-fetches, same pattern likely used elsewhere for non-
navigating success — `DeleteTicketButton`'s own doc, if it documents this,
should be checked at implementation time for the exact idiom already
established there) rather than navigating away, since renaming stays on
the same page. 401 handling via the existing `useNeedsLogin`/`LoginLink`
pattern, identical to `DeleteTrainButton.tsx:44-68`.

### 6. Ownership/auth: no new pattern, direct reuse of the existing one

Both new routes require `AuthenticatedUser` (`crates/api/src/auth.rs:239-244`)
and fold the ownership check directly into the `UPDATE ... WHERE id = $1
AND user_id = $2` clause, exactly like `delete_tracked_train`
(`crates/api/src/data/train_tracking.rs:534-541`) and `update_custom_line`
(`crates/api/src/data/custom_lines.rs:181-216`) — never a separate
ownership `SELECT` followed by an unscoped write, and never `403`: a
mismatch or missing row both 404, matching this app's universal
"exists-but-not-yours is indistinguishable from doesn't-exist" convention
stated in `crates/api/src/routes/train.rs`'s own module doc (lines 1-13)
and reiterated at nearly every mutation this spec cites above.

## Non-goals

- **Extraction never populates `custom_name`.** `ticket_extraction.rs`
  (`.pkpass`/PDF parsing) is untouched by this spec — a custom name is
  always and only user-typed, never inferred, matching Decision 2's
  privacy reasoning exactly (if extraction ever wrote to this column, the
  "never extracted from the document" half of that reasoning would stop
  being true).
- **No rename-at-creation-time UI** (`TrackTrainForm.tsx`/
  `TicketEntryForm.tsx` stay as they are) — see Decision 5's closing
  paragraph.
- **No search/filter by custom name** on `/track/mine` — that list has no
  search or filter mechanism of any kind today for any field; adding one
  is a separable feature, not implied by "let me name things."
- **No history of previous custom names** — a rename overwrites, is never
  versioned or logged.
- **No length-limit UI hint (character counter) mandated** — nice-to-have,
  not load-bearing; a call for the implementation plan.
- **No change to `TrackedTrainRef`** (the trust-consumer-facing poller
  shape) — it has no user-facing rendering need for a name.

## Open questions / risks

1. **Biggest open question: should the 100-character cap (Decision 1) be
   raised, and should it live in `crates/common` as a shared constant both
   the future frontend character-counter and the backend validator import,
   rather than being duplicated as a bare `100` in two places?** This
   spec picked 100 by the same "reasonable-sounding, not researched"
   methodology `MAX_PIN_AGE`/`MINE_LIST_LIMIT` were explicitly flagged
   with in their own doc comments — there is no usage data yet on what
   real users type into a field like this. Given this repo's own stated
   preference (per those two constants' comments) to revisit
   un-researched bounds once real usage exists rather than guess harder up
   front, 100 is a reasonable starting point to ship, but the
   single-source-of-truth question (one Rust constant referenced from both
   the validator and, if a character counter is ever added,
   `crates/common` so the frontend doesn't hardcode a second copy of the
   same number) should be settled explicitly in the implementation plan
   rather than left to whichever file happens to get written first.
2. Whether `PATCH` is the right verb given this codebase has zero existing
   `PATCH` routes anywhere (`grep -rn "axum::routing::patch"
   crates/api/src/routes/` turns up nothing) — every existing partial-ish
   update in this app is either a `PUT` that replaces a whole resource
   (`update_custom_line`) or a `POST` to a narrow action sub-path
   (`/attach`). `PATCH .../name` is the most semantically correct HTTP verb
   for "update exactly one field," but introduces a new verb to this
   router's vocabulary — a `POST .../name` sub-path would be more
   consistent with the router's existing style (every other narrow-action
   route here is `POST`, never `PUT`/`PATCH`, except the one `PUT
   /lines/{id}`). Worth a deliberate choice in the implementation plan,
   not an assumption.
3. Whether clearing a custom name (setting it back to `NULL`) needs its own
   distinct UI affordance beyond "submit an empty text input" — an empty
   submission silently reverting to the computed default might surprise a
   user who meant to just fix a typo and hit backspace-select-all by
   accident. A visible "Clear" action (Decision 5) mitigates this but
   wasn't validated against a real user.
