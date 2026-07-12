# Edit/Delete Custom Lines — Design

Sub-project 2 of 3 (see also: `2026-07-11-operator-station-autocomplete-design.md`,
already shipped; dark-theme spec to follow once this one ships).
Independent of the other two.

## Goals

- Let a user edit an existing custom line's name, operators, stations,
  headcode prefixes, and destination CRS filter, reusing the existing
  create form rather than a second one.
- Let a user delete a custom line from the UI — the backend
  `DELETE /public/lines/{id}` endpoint already exists and works
  (`crates/api/src/routes/lines.rs`) but has no UI wired to it today.
- Surface both actions on the line detail page (`/lines/[id]`), gated on
  `source === 'custom'` — catalogue lines get neither.

## Non-goals

- No auth/ownership changes — same unauthenticated model as the rest of
  custom lines (see the original custom-lines spec's Non-goals).
- The custom line's `id` (slug) never changes on edit, even if the name
  does. It's derived once at creation
  (`crates/api/src/data/custom_lines.rs`'s `slugify`) and pinned-line
  references / bookmarked URLs depend on it staying stable. Renaming a
  line does not regenerate or move its id.
- No optimistic UI / partial-field (PATCH-style) updates. The edit form
  submits the same full-object shape `POST /lines` already accepts;
  `PUT /lines/{id}` is a full replace of the editable fields, matching
  how the form already works today (no per-field diffing).
- No inline edit/delete actions on the `/lines` list page — detail page
  only, per the placement decision below.

## Backend

`crates/api/src/data/custom_lines.rs` gains two functions, alongside the
existing `list_custom_lines`/`insert_custom_line`/`delete_custom_line`:

```rust
pub async fn get_custom_line(pool: &PgPool, id: &str) -> Result<Option<CustomLine>>
pub async fn update_custom_line(pool: &PgPool, id: &str, new: NewCustomLine) -> Result<Option<CustomLine>>
```

Both return `None` when no custom line has that `id` (not found — the
caller decides the HTTP status). `update_custom_line` does a plain
`UPDATE custom_lines SET name = $2, operators = $3, ... WHERE id = $1`
and returns the updated row, or `None` if the `WHERE` matched nothing. It
does **not** touch `id` or `created_at`.

`crates/api/src/routes/lines.rs`'s `/lines/{id}` route gains `GET` and
`PUT` alongside the existing `DELETE`:

- `GET /lines/{id}` → the full `CustomLine` (id, name, operators,
  stations, headcodePrefixes, destinationCrsFilter — camelCase on the
  wire, matching `CreateLineRequest`'s convention). 404 if `id` belongs to
  a catalogue line (mirrors `delete_line`'s existing catalogue-id
  rejection) or doesn't exist at all. This is a new capability — today's
  `GET /lines` only returns the trimmed `LineSummary` projection (no
  `stations`/`headcodePrefixes`/`destinationCrsFilter`), which isn't
  enough to pre-populate an edit form.
- `PUT /lines/{id}` → same body shape and validation as `POST /lines`
  (`CreateLineRequest`: non-empty name, ≥2 stations), applied via
  `update_custom_line`. 400 for a catalogue id (same check as
  `delete_line`), 404 if the id doesn't exist, 200 with the updated
  `LineSummary` on success.

## Frontend

`frontend/app/lines/CustomLineForm.tsx` gains an optional prop:

```ts
existingLine?: { id: string; name: string; operators: string[]; stations: string[]; headcodePrefixes: string[]; destinationCrsFilter: string[] }
```

When present: all form state initializes from it instead of empty
defaults, submission does `PUT /api/lines/{id}` instead of
`POST /api/lines`, the submit button reads "Save changes" instead of
"Create line", and on success it redirects to `/lines/{id}` (the line's
own detail page) instead of `/lines` (the list) — landing back where you
came from, not the browse page, since you were editing a specific line.

New page `frontend/app/lines/[id]/edit/page.tsx`: server component, calls
the new `GET /public/lines/{id}` (via a new `getCustomLine(id)` in
`frontend/lib/api.ts`) to fetch the existing record, 404s (via
`notFound()`) if it's not a custom line, and renders
`<CustomLineForm existingLine={...} />`.

`frontend/app/lines/[id]/page.tsx` (the detail page): when the fetched
line's `source === 'custom'`, render an "Edit" link
(`/lines/{id}/edit`) and a "Delete" button. Delete asks for confirmation
(Mantine `Modal` or a simple confirm affordance consistent with this
app's existing patterns — no native `window.confirm`, this is a Client
Component so a Mantine-based confirmation fits the rest of the UI), then
`DELETE /api/lines/{id}` (through the existing same-origin proxy, same
pattern as `PinToggle`'s writes) and redirects to `/lines` on success.

## Testing

- Rust: `update_custom_line`/`get_custom_line`'s SQL is constrained by the
  same reality documented in the autocomplete spec — no DB-integration
  harness exists in this workspace. Verify via manual `curl` against a
  running stack instead, same as that feature's Task 6.
- Frontend: no test file exists for `CustomLineForm` today (confirmed
  during the autocomplete feature's Task 4) and this spec doesn't add one
  either, consistent with that precedent — the edit-mode branch reuses
  the same untested form. New test coverage is added for the detail
  page's edit/delete buttons instead: gating logic (`source === 'custom'`
  shows them, `'catalogue'` shows neither) and the delete confirm-then-
  fetch flow, following `PinToggle.test.tsx`'s pattern for a Client
  Component that writes through the `/api/*` proxy.
