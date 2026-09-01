# Design: Private Custom Lines and Tracked Trains

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md`
and `docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md`. No
implementation plan, no code or migration changes — this is a design
document only.

## Goal, and why this is a real reversal

Three changes, as stated by the repo owner:

1. Custom lines should be private to the user who created them (not
   publicly readable, as they currently are).
2. Remove the ability for a custom line to exist without a real owner
   (currently a real, permanently-supported state in this schema).
3. Tracked trains should also be private to the user who created them (not
   publicly readable, as they currently are).

**This reverses deliberate design decisions made earlier in this same
session, on purpose, not by oversight.** Three separate pieces of prior
work said the opposite, in writing, with reasoning:

- `crates/api/migrations/20260828100000_add_ownership.sql` added
  `custom_lines.user_id` as **nullable by design**, with its own header
  comment stating outright: *"NULL is deliberately NOT 'public, owned by
  nobody' for write access: a NULL-owned row stays readable... but is not
  editable or deletable by anyone... until an operator manually assigns a
  real owner."* It ships an "OPERATOR RUNBOOK" telling admins how to
  manually claim orphaned rows, and states leaving them unclaimed is
  *"safe... never destructive."* This spec proposes making the NULL state
  itself impossible, and readability conditional on real ownership — both
  reversed from that migration's own stated intent.
- `crates/api/src/routes/lines.rs`'s `get_line` handler was changed a few
  hours before this spec was written (same session) specifically to add an
  `isOwner` field, with an explicit comment explaining why the *read*
  itself was deliberately kept public: *"No separate catalogue-id check
  needed here... there's no distinct error message worth giving for
  'that's a catalogue line' on a read-only lookup the way there is for a
  rejected write."* The file's module doc says outright: *"Reads (`GET
  /lines`, `GET /lines/{id}`...) are unauthenticated."* This spec reverses
  that "reads stay public" half explicitly — see Decision 1.
- `docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` audited
  every auth-relevant surface in this app and explicitly classified a
  tracked train's live journey as **"Tier 1 — Public, no gate"**, reasoning
  that *"any tracked train is a public, shareable URL by design... This is
  the app's entire reason to exist and must never be gated."* This spec
  reverses that classification for tracked trains specifically — see
  Decision 6, and the tradeoff called out there.

None of this is being reversed lightly. Each section below states plainly
what was true before, what changes, and what is lost by changing it — most
concretely the "share a link to a friend to show them your delayed train"
use case, which this design does remove, per the repo owner's own
unambiguous instruction (see Decision 6).

### Clarifications received mid-investigation

Two points the repo owner clarified directly, resolving what would
otherwise be open questions in this doc:

1. **Custom-line creation must be gated behind login, full stop.**
   Investigation confirms this is *already* true today — see Decision 4 —
   so this clarification is satisfied by the existing code, not new work.
2. **Orphaned (NULL-owner) custom lines must be genuinely eliminated**, not
   merely made application-layer-inaccessible. This resolves Decision 2's
   migration-path question in favor of a real schema change
   (`user_id` becomes `NOT NULL`). The exact mechanics of what happens to
   any pre-existing NULL rows are still spelled out in full in Decision 2
   — sign-off on the *direction* doesn't remove the need to see precisely
   what the migration does to real data.

## Corrections: the investigation found a materially larger surface than the brief named

Following this repo's established "Corrections" precedent
(`2026-08-31-anonymous-user-ux-design.md`,
`2026-08-31-tracked-trains-list-design.md`): the brief's mandatory reads
named `lines.rs`'s `get_line` as *the* read to lock down for custom lines.
Direct inspection shows that endpoint is not the only place a custom
line's content is publicly readable — it is not even the endpoint that
serves a custom line's most substantive content. Locking down only
`get_line` would ship a change that *looks* like it makes custom lines
private while leaving their actual status/disruption data fully public.
This correction reshapes Decisions 1–3 below; it isn't a side note.

**What actually renders `/lines/[id]`'s content, traced end to end:**
`frontend/app/lines/[id]/page.tsx` calls four backend reads. Only one of
them is the endpoint the brief named:

| Frontend call | Backend route | Serves custom lines today? | Ownership-aware today? |
|---|---|---|---|
| `getLineStatus([id], true)` | `GET /Line/{ids}/Status` | **Yes** — this is the line's name, category, operators, and the actual disruption list (`RepresentativeInfo`/`IssueList`) rendered on the page. | **No.** No auth extractor at all. |
| `getCustomLine(id)` | `GET /public/lines/{id}` | Yes — only used for `isOwner` and edit-form prefill data (name/stations/etc., not status). | Partial — computes `isOwner` via `OptionalAuthenticatedUser`, but the read itself never rejects or filters. |
| `getAllLines()` | `GET /public/lines` | Yes — every custom line's name/operators/category appears in the bulk list unconditionally. | **No.** |
| `getLineDefinition(id)` | `GET /public/lines/{id}/definition` | Yes — stations/operators for the tooltip. | **No.** |

`GET /Line/{ids}/Status` is backed by `queries::line_status_for_ids`
(`crates/api/src/data/queries.rs`), a plain `SELECT ... FROM line_status
WHERE line_id = ANY($1)` with no join to `custom_lines` and no auth
extractor on its route handler (`crates/api/src/routes/line_status.rs`).
The `line_status` table itself is written by `crates/aggregator`, which
explicitly merges custom lines into the exact same pipeline as catalogue
lines (`aggregation::merge_custom_lines`, `crates/aggregator/src/main.rs`
line 93–94: *"the rest of the pipeline... treats them identically to
catalogue lines"*) and assigns them real `mode_name`s (in practice
`national-rail`, since custom lines are NR-headcode/CRS-based) — meaning
custom-line rows also appear, unfiltered, in `GET /Line/Mode/{modes}/Status`
(the bulk feed the home page and `/lines` All Lines table both read) and
in `GET /Line/{id}/Status/{from}/to/{to}` (the `/lines/[id]/history` page).
`GET /StopPoint/{crs}/Disruption` is the one exception — it builds its
candidate line list from `app.config.lines` (the static TOML catalogue
only), so custom lines never appear there; confirmed by reading
`get_stop_point_disruption`, no change needed for that route.

So: **fixing only `get_line` would leave a private custom line's real
name, category, operators, and live disruption status fully public and
enumerable by id**, both individually (`GET /Line/{id}/Status`) and in
bulk (`GET /Line/Mode/{modes}/Status`, `GET /public/lines`). Decisions
1–3 below cover the full set: `lines.rs`'s three routes and
`line_status.rs`'s three affected routes (all but `get_stop_point_disruption`).

The good news: this doesn't require touching `crates/aggregator` at all.
The aggregator has no concept of "who's asking" and shouldn't grow one —
it's a batch job computing one shared, real-world status per line,
independent of any viewer. The privacy boundary belongs entirely at
**read time**, in `crates/api`'s route handlers, the same place every
other ownership check in this codebase already lives (`custom_lines.rs`'s
`update_custom_line`/`delete_custom_line`, `train_tracking.rs`'s
`tracked_train_owner`). See Decision 3 for exactly how.

## Current relevant state, precisely (verified 2026-09-01)

- `custom_lines.user_id`: nullable, `REFERENCES users(id) ON DELETE
  CASCADE`, no `NOT NULL`. `insert_custom_line` (the only INSERT path)
  always sets a real `user_id` from the caller's `AuthenticatedUser` — no
  code path in this repo creates a new NULL-owner row. NULL rows can only
  exist as pre-ownership-retrofit legacy data.
- `create_line`/`update_line`/`delete_line` (`crates/api/src/routes/lines.rs`)
  already require `AuthenticatedUser` — confirmed by reading the handler
  signatures (`user: AuthenticatedUser` on all three). `update_custom_line`/
  `delete_custom_line` (`crates/api/src/data/custom_lines.rs`) already
  filter `WHERE id = $1 AND user_id = $2`, so a NULL-owned row can never be
  edited or deleted by anyone (a NULL never equals a real user id) —
  matches the migration's own stated intent for writes.
- `CustomLineForm.tsx` already has a working `needsLogin` 401-handling
  branch (added after the anonymous-user-ux-design audit flagged its
  absence) — anonymous/lapsed-session submission already shows a login
  prompt, not raw backend text. `DeleteLineButton` is gated behind the
  parent page's `isCustom && isOwner` check, so it's not rendered at all
  for a non-owner/anonymous viewer today.
- `tracked_trains.user_id` is `NOT NULL` from birth (`crates/api/migrations/20260828120000_train_tracking.sql`,
  confirmed by `docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md`'s
  own direct-inspection finding). **There is no orphaned-row problem for
  tracked trains at all** — every row has always had a real owner. The
  privacy change for tracked trains is a pure route/auth change, no schema
  migration needed.
- `get_by_tracking_id`/`get_by_uid_and_date` (`crates/api/src/routes/train.rs`)
  take no auth extractor of any kind — confirmed by reading both handler
  signatures. `tracked_train_owner(pool, tracking_id)` already exists
  (used today only by `post_ticket`/`get_tickets`) and is exactly the
  ownership check these two routes need.
- Ticket routes (`post_ticket`, `get_tickets`, `get_delay_repay_estimate`)
  are already fully private via `AuthenticatedUser` + `tracked_train_owner`/
  `get_ticket_owned`. **Nothing changes for these routes.** They already
  implement, precisely, the pattern this spec extends to the two read
  routes — see Decision 5.
- `get_delay_repay_estimate` calls `train_tracking::get_by_tracking_id`
  (the **data-layer function**, not the HTTP route) directly, inside a
  handler that has already independently verified ticket ownership via
  `get_ticket_owned`. This internal call bypasses HTTP entirely and needs
  no change — the auth gate this spec adds lives in the *route handler*
  `get_by_tracking_id`/`get_by_uid_and_date` in `train.rs`, not in the
  data-layer query functions of the same name in `train_tracking.rs`,
  which stay plain, auth-agnostic queries (consistent with this repo's
  existing convention: auth lives in route handlers/extractors, not data
  functions).
- The tracked-trains-list feature (`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md`,
  plan at `docs/superpowers/plans/2026-08-31-tracked-trains-list.md`) is
  **designed but not implemented** — confirmed, no `/Train/mine` route,
  query, or frontend page exists in the repo today (`grep -r "Train/mine"`
  returns nothing outside the plan/spec docs themselves). See Decision 7
  for how this design interacts with it once built.
- Existing 401-vs-404 convention in this codebase, both explicit in code
  comments: `AuthenticatedUser`'s `FromRequestParts` impl
  (`crates/api/src/auth.rs`) rejects with **401** + plain text (`"no
  session"` / `"session expired or unknown"`) when there's no valid
  session — this is the "you're not logged in at all" signal, uniform
  across every route that requires it, and reveals nothing about the
  requested resource (it fires identically whether or not the id exists).
  `tracked_train_owner`'s own doc comment states the second-tier rule
  explicitly: *"A mismatch or missing tracked train both map to the same
  `404` — never `403`"* — "exists but not yours" and "doesn't exist" are
  deliberately indistinguishable. This spec reuses both conventions
  verbatim rather than inventing new ones: **401 for "not logged in,"
  404 for "logged in but this isn't yours (or it doesn't exist, or nobody
  owns it) — never 403, anywhere in this design.**

## Decisions

### 1. `GET /public/lines/{id}` (`get_line`): require ownership, 401/404 per the existing convention

Switch `get_line` from `OptionalAuthenticatedUser` to `AuthenticatedUser`.
After fetching the row, treat "doesn't exist," "exists but `user_id` is
NULL (legacy orphan)," and "exists but owned by someone else" identically:
**404, `"custom line not found"`** — reusing the exact message
`update_line`/`delete_line` already use for "exists but not yours," so an
external observer gets no signal distinguishing any of the three cases.
No session at all → **401**, `"no session"`, via the extractor itself —
uniform with every other `AuthenticatedUser`-gated route, and reveals
nothing about whether `id` exists (it's the same response for every id).

`CustomLineDetail.isOwner` becomes vestigial once this ships: any 200
response is by construction from the real owner (everyone else gets 404
before the response body is ever built), so `is_owner`/`isOwner` would
always serialize as `true`. **Recommend removing the field entirely** at
implementation time as dead weight that could otherwise read as
meaningful when it no longer is — the frontend's `isCustom && isOwner`
gate on `/lines/[id]/page.tsx` also simplifies (see Decision 8), since a
non-owner now never reaches a `200` from this endpoint to compute
`isOwner` from in the first place.

### 2. `custom_lines.user_id`: real `NOT NULL` migration, with explicit mechanics for existing NULL rows

Per the repo owner's clarification, this is a genuine schema change, not
an application-layer workaround. Two things must both be true for a
migration that adds `NOT NULL` to succeed: no row can be left NULL, and
whatever happens to any real NULL rows found at migration time must be
stated in the migration itself, not decided implicitly.

**What's actually in the table today, live-deployment-wise:** the
original migration's own comment already establishes this is a
*"not needed on a fresh install"* concern — NULL rows can only exist on a
deployment that had `custom_lines` rows before the 2026-08-28 ownership
retrofit shipped and whose operator never ran that migration's own
documented runbook (`UPDATE custom_lines SET user_id = '<admin sub>'
WHERE user_id IS NULL;`). This app's own design posture (cited by the
same migration file, quoting the original custom-lines design doc) is a
*"single trusted personal instance"*-sized deployment — so the realistic
worst case is a small number of hand-authored rows, not a large orphan
population. That doesn't change the mechanics below; it just means the
blast radius, if any exists, is expected to be small.

**Recommended mechanics — reassign, don't delete:**

```sql
-- 1. A placeholder account, not a real login-capable user: `users.id` is
--    just a TEXT primary key with no format constraint (it's normally an
--    OIDC `sub`, but nothing enforces that), and `email`/`name` are both
--    nullable. No `sessions` row can ever reference this id except by an
--    operator manually crafting one — the normal OIDC login flow only
--    ever creates a session for a real `sub` returned by the SSO server.
--    So a line "owned" by this placeholder is, in practice, exactly as
--    unreachable through the app as a NULL-owned row is today: nobody can
--    authenticate as it.
INSERT INTO users (id, email, name)
VALUES ('legacy-unclaimed', NULL, 'Unclaimed legacy custom lines')
ON CONFLICT (id) DO NOTHING;

-- 2. Reassign, not delete, any surviving NULL rows.
UPDATE custom_lines SET user_id = 'legacy-unclaimed' WHERE user_id IS NULL;

-- 3. Now safe: no row can be NULL, so the constraint holds going forward.
ALTER TABLE custom_lines ALTER COLUMN user_id SET NOT NULL;
```

**Why reassign rather than delete:** this repo's own ownership-retrofit
migration already drew, in its own words, a hard line between
`custom_lines` (*"authored content, which IS carried forward"*) and
`pinned_lines`/`pinned_stations` (*"pure UI convenience state"*, safely
`TRUNCATE`d in that same migration). A blanket `DELETE FROM custom_lines
WHERE user_id IS NULL;` would apply the *pinned_lines* treatment to data
this codebase has already, explicitly, classified as the *other* kind —
destroying a real user's authored line definition (name, stations,
operators, headcode filters) with no way back. The placeholder-reassignment
path gets the schema guarantee the repo owner asked for (no row can ever
be NULL again) without deleting anything, and — since the placeholder
can't log in — it has the *exact same effective visibility* as a NULL row
does today under this spec's own new read rule (Decision 1): completely
inaccessible to every real visitor, owner or not, until a human operator
manually reassigns it. That's not a new operational step — it's the same
manual-claim runbook the original migration already documented, just
re-pointed at a concrete id (`'legacy-unclaimed'`) instead of `NULL`:

```sql
-- Updated runbook, same shape as before, new WHERE clause:
UPDATE custom_lines SET user_id = '<admin sub>' WHERE user_id = 'legacy-unclaimed';
```

**Alternative considered and rejected as the default: `DELETE FROM
custom_lines WHERE user_id IS NULL;`.** More final, and arguably
"cleaner" in the sense of not leaving inert rows around — but it is a
real, irreversible data-loss operation on user-authored content, contrary
to this codebase's own established distinction above. **If the repo
owner has verified, out-of-band, that no live deployment has any real
NULL-owner rows worth keeping (e.g. this is a from-scratch/dev-only
deployment, or the runbook was already run everywhere that matters), the
delete is simpler and equally valid — but that verification is a fact
about the live data this design doc cannot supply, so implementation
should confirm it explicitly (e.g. `SELECT count(*) FROM custom_lines
WHERE user_id IS NULL;` before writing the migration) rather than assume
either way.** This is flagged here, explicitly, one more time, precisely
because the mechanics must stay visible in the spec even though the
overall direction (eliminate the NULL state) is no longer an open
question.

### 3. `line_status.rs`: filter custom-line rows by ownership at read time, without gating catalogue/TfL content

Applies to `get_line_status` (`GET /Line/{ids}/Status`), `get_mode_status`
(`GET /Line/Mode/{modes}/Status`), and `get_line_status_history`
(`GET /Line/{id}/Status/{from}/to/{to}`). `get_stop_point_disruption` is
unaffected — confirmed above, it never surfaces custom lines.

These three routes serve a **mix** of catalogue/TfL lines (public,
unowned, must stay Tier 1 per `2026-08-31-anonymous-user-ux-design.md`'s
own reasoning — line/station status is *"this app's entire reason to
exist and must never be gated"*) and custom lines (now private) in the
same response. Gating the whole route behind `AuthenticatedUser` would
break the anonymous catalogue-browsing experience these routes primarily
exist for. Instead: add `OptionalAuthenticatedUser` (never rejects), fetch
rows as today, then for any row whose `line_id` has this app's own
`custom-` prefix (guaranteed — `custom_lines::slugify` always produces
`format!("custom-{slug}")`; no other id shape reaches this table), look up
its `user_id` and drop the row unless it matches the caller.

New small helper needed in `custom_lines.rs`, a bulk variant of the
ownership lookup `get_custom_line` already does one-at-a-time:

```rust
/// Owners for every custom-prefixed id in `ids`, for filtering a bulk
/// status response by ownership without an N+1 query per row. Catalogue/
/// TfL ids in `ids` simply won't match anything here and are left alone
/// by the caller.
pub async fn owners_for_ids(pool: &PgPool, ids: &[String]) -> Result<HashMap<String, Option<String>>>
```

Per-route treatment:

- **`get_mode_status`** (bulk, many lines, e.g. the home page's/`/lines`
  table's full feed): silently drop any custom-line row the caller doesn't
  own — never error the whole request. An anonymous visitor sees zero
  custom lines; a logged-in visitor sees only their own. This is list
  filtering, not a rejection, matching how `list_lines` (Decision 8) and
  every other "browse everything you're allowed to see" surface in this
  codebase behaves.
- **`get_line_status`** (few ids, the line detail page's own fetch):
  same filtering, applied before the existing `if rows.is_empty() { 404
  }` check — so a request whose *only* requested id is a custom line the
  caller doesn't own falls straight into the same, already-existing
  `"no matching line(s): {ids}"` 404 an unknown id already produces. No
  new status code, no new branch — the existing "unknown id" path already
  does exactly what "you can't see this" should do, and does it without
  distinguishing the two cases, consistent with the 404 convention
  established above.
- **`get_line_status_history`**: this route has no existence check at all
  today for *any* id — an unknown id already just returns an empty
  `history` array, no 404, no distinction from "known id, no data in this
  range." Extend that same non-distinguishing behavior: if `id` is a
  custom line the caller doesn't own, return the same empty array a truly
  unknown id already produces. This closes the leak (no real history data
  crosses the boundary) without adding a new response shape this route
  has never had.

This is the one place this spec's line-privacy design introduces genuinely
new backend logic beyond gating an existing extractor — flagged here as
the one piece worth extra implementation-time scrutiny (new helper
function, new filtering step in three handlers), unlike Decisions 1, 5,
and 6, which are closer to "swap the extractor, reuse the existing
ownership check."

### 4. Custom-line creation: already fully gated — confirmed, not changed

Per the repo owner's clarification, this needed explicit verification
rather than assumption. Confirmed by direct reading:

- `create_line` (`crates/api/src/routes/lines.rs`) takes `user:
  AuthenticatedUser` as a required extractor — an anonymous `POST
  /public/lines` already gets a bare `401` today, unconditionally. There
  is no code path, today, that creates a custom line without a real,
  authenticated caller.
- `insert_custom_line` (`crates/api/src/data/custom_lines.rs`) always
  binds `user_id` from that caller — there is no INSERT statement in this
  codebase that can produce a NULL-owner row. Every future custom line, by
  construction, has a real owner from birth; only pre-retrofit legacy rows
  can ever be NULL (Decision 2).
- `CustomLineForm.tsx` (frontend) already implements the `needsLogin`
  401-prompt pattern for both create and edit — this was already fixed
  (per `2026-08-31-anonymous-user-ux-design.md`'s Correction 3, and
  confirmed still present in the file today).

**No change is needed anywhere for creation-gating.** This clarification
is satisfied by existing code; it's recorded here so the "was this
already true, or does it need work" question has a citable, verified
answer rather than being left implicit.

### 5. Custom-line writes (`update_line`/`delete_line`): unaffected

Already `AuthenticatedUser` + `WHERE id = $1 AND user_id = $2`-scoped,
already 404 on any mismatch (never 403), already the pattern the rest of
this spec extends to reads. No change.

### 6. Tracked-train reads (`get_by_tracking_id`, `get_by_uid_and_date`): require ownership, same 401/404 convention — and the tradeoff this removes, stated plainly

Add `AuthenticatedUser` to both handlers. After fetching `state`, check
ownership via the **existing** `tracked_train_owner(pool, state.id)` (for
`get_by_uid_and_date`, this means fetching by uid/date first to learn the
row's `id`, then checking ownership — a two-step lookup, not a schema
change) and return **404**, reusing each route's own existing not-found
message (`"no tracked train with that id"` /
`"no resolved tracked train for that uid/date"`), for both "doesn't
exist" and "exists but isn't the caller's" — identical to the ticket
routes' own `tracked_train_owner`-based check, and to
`tracked_train_owner`'s own doc comment naming this exact convention. No
session → **401**, `"no session"`. This is the smallest of the three
surfaces to implement: reuse the extractor already used by
`post_track`/the ticket routes, reuse the ownership check already used by
the ticket routes, no new query needed.

**What this removes, explicitly, per the repo owner's own instruction:**
`2026-08-31-anonymous-user-ux-design.md` classified this exact public
readability as intentional and load-bearing — a tracked train's URL is
*"a public, shareable URL by design,"* meaning today a user can send
`/train/by-id/12345` (or the canonical `/train/{uid}/{date}` link) to a
friend to show them "look, my train's delayed," with no login required on
either end. **This design removes that capability entirely, with no
opt-in replacement.** The repo owner's literal instruction — *"ensure
that tracked trains are also private to the person who made them"* — reads
as unambiguous, no-exceptions private, and this spec implements it that
way rather than inventing a middle ground unprompted. A "make this
tracking link shareable" opt-in flag was considered as an alternative (a
per-pin boolean, defaulting closed, that would let a route bypass the
ownership check for that one pin) and is **not designed here**, on the
grounds that the instruction doesn't ask for it and a genuinely optional,
narrower feature shouldn't be smuggled into a "make this private" change
— but it's named here explicitly, not silently dropped, in case the repo
owner wants it as a distinct, later, opt-in feature once this ships. See
Open questions.

**Frontend implication — a real bug this spec must not ship into**:
`getTrackedTrainById`/`getTrackedTrainByUidAndDate` (`frontend/lib/api.ts`)
currently forward **no cookies at all** (confirmed: both call the shared
`fetchJson` with no `Cookie` header, unlike `getPreferences`/
`getTicketsForTrackedTrain`, which manually reattach
`(await cookies()).toString()`). If the backend route is gated without
this fix, **every request — including the real owner's own — arrives
looking anonymous and gets a bare 401**, since a Server Component's own
`fetch` never inherits the incoming request's cookies automatically.
**Both functions must add the same cookie-forwarding this app already
uses in three other places** (`getPreferences`, `getSession`,
`getTicketsForTrackedTrain`) as a precondition of this change working at
all, not an optional cleanup.

Both functions must also stop treating every non-2xx as one undifferentiated
`Error`/`ApiNotFoundError`. Add a distinct `ApiUnauthorizedError` (mirrors
`ApiNotFoundError`'s existing shape — `export class ApiUnauthorizedError
extends Error {}`) and have `errorForResponse` map `401` to it, so the two
`/train/...` page components can tell "not logged in at all" apart from
"doesn't exist / isn't yours":

```ts
export async function getTrackedTrainById(id: number): Promise<TrackedTrainState> {
  const url = `${baseUrl()}/Train/${id}`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<TrackedTrainState>;
}
// getTrackedTrainByUidAndDate: identical shape, same cookie-forwarding addition.
```

`frontend/app/train/by-id/[trackingId]/page.tsx` and
`frontend/app/train/[uid]/[date]/page.tsx` both already have a
`try { ... } catch (err) { if (err instanceof ApiNotFoundError)
notFound(); throw err; }` block. Extend both with a second branch:

```ts
} catch (err) {
  if (err instanceof ApiNotFoundError) notFound();
  if (err instanceof ApiUnauthorizedError) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Tracking Train {trackingId}</Title>
        <TextLink href="/api/auth/login" underline="always">
          Log in to view this tracked train
        </TextLink>
      </Stack>
    );
  }
  throw err;
}
```

This is a deliberate departure from the custom-line detail page's
collapse-401-into-404 choice (Decision 8) — and worth being explicit about
why the two pages differ: a custom line's detail page has other,
still-public sibling content one route below it in the same directory
(catalogue lines at other ids), so a blunt "not found" for a private one
reads naturally as "nothing here." A tracked-train page has no such
sibling context — it's a single-purpose page reached only by a specific
id, and *"log in — this might be yours"* is a more honest, more useful
message than a bare 404 for a visitor who genuinely owns the train but
whose session lapsed. Both choices follow the same underlying rule (never
leak *which* of "doesn't exist" vs "exists, not yours" is true — that
stays 404-collapsed in both designs); they differ only in whether "not
logged in at all" gets its own distinct, honest prompt, which this page
can afford and the custom-line page's simpler existing structure doesn't
already have wired up for it.

**`blend_darwin_eta` / live polling**: unaffected structurally — it runs
inside the same handler, after the ownership check passes, exactly as
before. The only real behavioral change is at the page-refresh boundary:
`AutoRefresh` (`app/layout.tsx`, `router.refresh()` every 30s) re-runs
these Server Component fetches on a live timer. If a viewer's session
expires mid-view, the *next* auto-refresh will now hit the 401 branch
above and the page will swap from live journey data to a login prompt —
a real, visible behavior change versus today (where the page just keeps
working, logged in or not). This is accepted as the correct consequence
of "private," not a bug to route around, but is worth naming so it isn't
mistaken for a fetch or caching regression during implementation.

### 7. Interaction with the (unimplemented) tracked-trains-list feature

`docs/superpowers/specs/2026-08-31-tracked-trains-list-design.md` designs
`GET /Train/mine` as already session-gated (`AuthenticatedUser`, no
ownership check needed beyond the extractor itself, since "list my own
trains" has no second party to be wrong about) and its list rows link to
`/train/{uid}/{date}` or `/train/by-id/{id}` for the *same authenticated
user's own* pins. **Nothing about this design changes once the two read
routes above require ownership** — a user clicking through from their own
list, as themselves, to their own train's detail page will always pass
the new ownership check trivially (same session, same `user.id`, and the
list only ever contains that user's own trains in the first place). No
revision to that spec is needed; this is recorded here only because the
brief asked whether the interaction needed reconciling, and it doesn't.

### 8. Frontend: `/lines/[id]/page.tsx`, `/lines/[id]/edit/page.tsx`, `getCustomLine`, `getAllLines`, `getLineDefinition`

**Collapse 401 into `ApiNotFoundError` for `getCustomLine` specifically**,
rather than adding a distinct `ApiUnauthorizedError` branch the way
Decision 6 does for tracked trains. Reasoning for the difference: on this
page, "not logged in" and "logged in but not the owner" already render
identically today (before this spec, both just see the same public read;
after it, both should just see the same 404) — there is no scenario on
`/lines/[id]` where "please log in, this might be yours" is worth a
distinct prompt the way it is on a single-purpose tracked-train page,
because the *default*, common case for this page is a public catalogue
line most visitors have no reason to think they own. Concretely:

```ts
export async function getCustomLine(id: string): Promise<CustomLineDetail> {
  const url = `${baseUrl()}/public/lines/${id}`;
  const cookieHeader = (await cookies()).toString();
  const response = await fetch(url, {
    cache: 'no-store',
    ...(cookieHeader ? { headers: { Cookie: cookieHeader } } : {}),
  });
  if (response.status === 401 || response.status === 404) {
    throw new ApiNotFoundError(`API request to ${url} failed: ${response.status}`);
  }
  if (!response.ok) throw errorForResponse(url, response);
  return response.json() as Promise<CustomLineDetail>;
}
```

**`getCustomLine` currently forwards no cookies at all** (same class of
gap as Decision 6's finding for the train functions) — this is a
precondition fix, not optional, or the real owner would also always see
404.

With this change, **`/lines/[id]/page.tsx` and `/lines/[id]/edit/page.tsx`
need no code changes at all** — both already have a
`catch (err) { if (err instanceof ApiNotFoundError) notFound(); throw
err; }` block (for `getLineStatus`/`getCustomLine` respectively), and
both errors now already collapse into that existing branch. The detail
page's `isCustom`/`isOwner` gating on the Edit/Delete buttons
(`isCustom && isOwner &&` around the `<Link>`/`<DeleteLineButton>`) also
simplifies for free: once `getCustomLine` 404s for any non-owner, the
whole page already 404s upstream at `getLineStatus` (Decision 3 — a
private custom line is now excluded from that response too, so
`rows.is_empty()` triggers first) before the Edit/Delete gating logic is
even reached. **Recommend deleting the now-dead `is_owner`/`isOwner`
field** (Decision 1) and simplifying that gate back to `isCustom &&`, once
implementation confirms no path can reach it as a non-owner.

**Real, deliberate UX tradeoff to flag, not silently accept:** a session
that expires between page-load and a later `AutoRefresh` cycle now turns
even the *real owner's own* custom line detail page into a bare Next.js
404, with no "log in again" prompt — unlike the equivalent tracked-train
case (Decision 6), which does get a distinct prompt. This is accepted
here as a reasonable, minor cost of reusing the existing error-handling
structure with zero new page code, but it's a real behavior difference
between the two surfaces this spec should not paper over. If this turns
out to matter in practice (e.g. custom-line sessions expiring mid-edit is
a common complaint), revisit by giving `/lines/[id]/page.tsx` the same
401-vs-404 split Decision 6 gives the train pages — not designed here,
flagged as the natural follow-up.

**`getAllLines()`, `getLineStatusForMode()`, `getLineStatus()`**: all
three currently forward no cookies (confirmed, none of `getAllLines`,
`getLineStatusForMode`, `getLineStatus` attach a `Cookie` header today).
All three need the same cookie-forwarding addition so a logged-in owner's
own custom lines actually appear in `/lines`' All Lines table and any
future "Right now" home-page widget (per
`2026-08-31-anonymous-user-ux-design.md`'s still-unimplemented proposal,
which reads `allReports` from exactly this call — it inherits this fix
automatically once `getLineStatusForMode` forwards cookies, no separate
change needed to that proposal).

**`list_lines`/`GET /public/lines`** (backend): add
`OptionalAuthenticatedUser`; replace the unconditional
`custom_lines::list_custom_lines(&app.database)` call with a
caller-scoped variant — `list_custom_lines_for_user(pool, user_id)` for an
authenticated caller (returns only that caller's own custom lines), or
skip the custom-line section entirely for an anonymous caller (empty,
same as today's *shape* for that case, just now also true for a logged-in
non-owner rather than only for anonymous visitors). Catalogue and TfL
entries are completely unaffected — no filtering, no auth requirement
change for those.

**`getLineDefinition(id)`/`GET /public/lines/{id}/definition`**
(tooltip data): gate the custom-line branch the same way as `get_line`
(Decision 1) — `OptionalAuthenticatedUser`, 404 for a custom id the caller
doesn't own, catalogue ids fully unaffected (the function's existing
catalogue-first branch already returns before ever touching
`custom_lines`). Lower stakes than the other changes in this section,
since `/lines/[id]/page.tsx` already swallows *any* error from this call
in a bare `try { ... } catch { /* swallowed */ }` for "nice-to-have
tooltip, don't break the page over it" — but still needs cookie-forwarding
added, or the real owner's own tooltip would silently vanish too.

## Architecture (net summary)

```
┌───────────────────────────────────────────────────────────────────────┐
│ frontend/lib/api.ts                                                     │
│   getCustomLine        + cookie fwd, 401+404 -> ApiNotFoundError        │
│   getAllLines          + cookie fwd (custom-line entries now scoped)    │
│   getLineDefinition    + cookie fwd                                     │
│   getLineStatus(ForMode) + cookie fwd (private custom lines filtered)   │
│   getTrackedTrainById/ByUidAndDate  + cookie fwd, NEW ApiUnauthorizedError│
│                                                                           │
│ frontend/app/lines/[id]/page.tsx, [id]/edit/page.tsx   -- NO code change│
│ frontend/app/train/by-id/[trackingId], [uid]/[date]    -- NEW 401 branch│
└───────────────────────────────────┬─────────────────────────────────────┘
                                     │ cookie-forwarded reads
                                     ▼
┌───────────────────────────────────────────────────────────────────────┐
│ crates/api                                                               │
│  routes/lines.rs                                                        │
│   get_line               OptionalAuth -> AuthenticatedUser; 401/404     │
│   list_lines              + OptionalAuthenticatedUser; scoped custom set│
│   get_line_definition     + OptionalAuthenticatedUser; 404 for custom   │
│  routes/line_status.rs                                                  │
│   get_line_status         + OptionalAuthenticatedUser; filter custom    │
│   get_mode_status         + OptionalAuthenticatedUser; filter custom    │
│   get_line_status_history + OptionalAuthenticatedUser; empty for custom │
│   get_stop_point_disruption   UNCHANGED (never serves custom lines)     │
│  routes/train.rs                                                        │
│   get_by_tracking_id      (none) -> AuthenticatedUser + owner check     │
│   get_by_uid_and_date     (none) -> AuthenticatedUser + owner check     │
│   ticket routes            UNCHANGED (already private)                  │
│  data/custom_lines.rs    + owners_for_ids (NEW, bulk ownership lookup)  │
│  data/train_tracking.rs  UNCHANGED (tracked_train_owner already exists)│
│                                                                           │
│  migrations/  NEW: custom_lines.user_id -> NOT NULL, placeholder-owner  │
│               reassignment for any legacy NULL rows (Decision 2)        │
└───────────────────────────────────────────────────────────────────────┘
```

## Testing

Following this repo's existing convention (colocated `#[cfg(test)]`
modules in Rust; colocated `*.test.tsx`/Vitest in the frontend):

- `crates/api/src/routes/lines.rs`: extend the existing `is_owner`
  unit-test suite's spirit into new coverage for the changed `get_line`
  behavior (401 for no session, 404 for non-owner/legacy-NULL/nonexistent,
  200 only for the real owner) — likely as integration-style tests given
  this needs a real `custom_lines` row and session, following whatever
  pattern this repo already uses for `AuthenticatedUser`-gated route tests
  elsewhere (none of the existing tests in this file exercise a live
  extractor path yet — this may be the first, so check
  `crates/api`'s existing integration test setup, if any, before assuming
  a shape).
- `crates/api/src/data/custom_lines.rs`: new `#[ignore]`d DB test for
  `owners_for_ids`, following the existing
  `get_custom_line_reports_the_owning_user_id_or_none_for_a_legacy_row`
  test's exact fixture/cleanup pattern in the same file.
- `crates/api/src/routes/train.rs`: extend `router_builds_without_panicking`
  coverage implicitly; add ownership-check coverage for
  `get_by_tracking_id`/`get_by_uid_and_date` mirroring the existing
  `tracked_train_owner`-based tests the ticket routes presumably already
  have (or, if none exist yet for those either, this is the first and
  should establish the pattern).
- `frontend/lib/api.test.ts` (or equivalent): `getCustomLine` collapsing
  401+404, `getTrackedTrainById`/`ByUidAndDate` distinguishing
  `ApiUnauthorizedError` from `ApiNotFoundError`, all four cookie-forwarding
  additions — mirroring `getTicketsForTrackedTrain`'s existing test shape
  for 401/404 handling.
- `frontend/app/train/by-id/[trackingId]/page.test.tsx` (new or extended):
  the new login-prompt branch renders for `ApiUnauthorizedError`,
  `notFound()` still fires for `ApiNotFoundError`.
- Migration: a real, applied-and-verified test of Decision 2's SQL against
  a fixture database containing at least one legacy NULL-owner row,
  confirming it survives as `'legacy-unclaimed'`-owned rather than being
  destroyed, and that the `NOT NULL` constraint then holds.

## Explicitly out of scope

- **A "make this tracking link shareable" opt-in.** Named and reasoned
  about in Decision 6, not designed — the repo owner's instruction reads
  as unconditional, and this would be new, separate feature surface
  (a per-pin flag, a UI to set it, a distinct unauthenticated read path
  gated by that flag) that wasn't asked for.
- **Any change to `crates/aggregator`.** The privacy boundary is entirely
  a read-time concern in `crates/api`; the aggregator keeps computing one
  shared status per line, owner-blind, exactly as today.
- **Any change to ticket routes** (`post_ticket`, `get_tickets`,
  `get_delay_repay_estimate`). Already fully private; confirmed, not
  touched.
- **Any change to `pinned_lines`/`pinned_stations` privacy.** Already
  fully user-scoped since the 2026-08-28 ownership retrofit (composite
  `(user_id, line_id)`/`(user_id, crs)` primary keys) — out of scope, not
  implicated by anything in this design.
- **A distinct "log in again" prompt for `/lines/[id]`'s session-expiry
  edge case.** Named as a real, deliberate tradeoff in Decision 8, not
  fixed — flagged as a reasonable follow-up if it matters in practice.
- **Retroactively notifying existing custom-line creators that their line
  is now private**, or any UI surfacing of the `legacy-unclaimed`
  placeholder state to real users. Out of scope — this is an operator-facing
  migration concern (the runbook), not a user-facing feature.
- **Implementing the tracked-trains-list feature itself.** Already fully
  designed elsewhere (`2026-08-31-tracked-trains-list-design.md`); this
  doc only confirms the interaction (Decision 7), doesn't touch that spec.

## Open questions / risks

1. **Whether a "shareable tracking link" opt-in is wanted as a later,
   separate feature.** Named explicitly in Decision 6 rather than assumed
   away — this design implements the literal, unconditional instruction,
   but the capability it removes was real and previously deliberate. Worth
   a direct, explicit confirmation from the repo owner before or after
   implementation, not a silent loss.
2. **The `custom_lines.user_id NOT NULL` migration's exact mechanics
   (Decision 2) still assume no live deployment needs the destructive
   `DELETE` alternative.** The reassign-to-placeholder path is recommended
   as the default and is non-destructive either way, but implementation
   should still run `SELECT count(*) FROM custom_lines WHERE user_id IS
   NULL;` against the real target database before writing the migration,
   simply to know whether this is a zero-row no-op or a real reassignment,
   and report that count back before the migration ships.
3. **No existing precedent in this codebase for an `AuthenticatedUser`-gated
   *read* route with integration-test coverage** (every existing
   `AuthenticatedUser` test surface found during this investigation is
   either a unit test of pure logic or untested at the route level) — the
   testing section above flags this; implementation may need to establish
   a new test-fixture pattern (a real session + user row) rather than
   extend an existing one.
4. **`get_line_status_history`'s "no distinction, just empty" treatment
   for a private custom line (Decision 3)** matches that route's own
   pre-existing behavior for *any* unknown id, but was never a deliberate
   design choice before now — it happened to fall out of the route never
   checking id validity at all. Worth a second look at implementation time
   to confirm this is still the right call once it's load-bearing for
   privacy specifically, not just an accident of an unrelated route never
   having needed a 404 before.
