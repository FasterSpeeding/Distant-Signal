# Operator/Station Autocomplete — Design

Sub-project 1 of 3 (see also: edit-custom-lines, dark-theme — specs to
follow once this one ships). Independent of the other two; sequenced first
because it's the most self-contained.

## Goals

- Replace free-text operator/station code entry with type-ahead suggestions
  (matching on code or name) across every field that takes an operator or
  station code, so users don't need to already know ATOC/CRS codes by heart.
- Fields in scope: `CustomLineForm`'s Operators (`TagsInput`), Add station
  (`TextInput` → `Autocomplete`), Destination CRS filter (`TagsInput`); and
  `StationSearchForm`'s station lookup (`TextInput` → `Autocomplete`).

## Non-goals

- Headcode prefixes field stays plain free text — not a station/operator
  code, no reference data to suggest from.
- No new database indexes. `stations`/`tocs` are small (~2,500 / ~30 rows);
  a sequential `ILIKE` scan is fast enough for a single-instance personal
  app. Revisit only if this table grows by orders of magnitude.
- No fuzzy/typo-tolerant matching (no `pg_trgm`) — plain substring `ILIKE`
  on code or name is enough for a list this size.
- No auth/rate-limiting on the new endpoints — consistent with the rest of
  `/public/*` in this app's "single trusted personal instance" model.

## Backend

New read-only data-access module `crates/api/src/data/reference.rs`
(the existing `queries.rs` only has write-side `upsert_stations`/
`upsert_tocs` for ingest — no read functions for either table exist yet):

```rust
pub async fn search_stations(pool: &PgPool, q: &str, limit: i64) -> anyhow::Result<Vec<StationSuggestion>>
pub async fn search_tocs(pool: &PgPool, q: &str, limit: i64) -> anyhow::Result<Vec<TocSuggestion>>
```

Both use runtime-checked `sqlx::query_as` (not the `query_as!` macro
family), matching this codebase's established choice to avoid needing a
live DB or checked-in `.sqlx` cache at compile time (see `queries.rs`
module doc). Query shape:

```sql
SELECT crs AS code, name FROM stations
WHERE crs ILIKE $1 OR name ILIKE $1
ORDER BY name LIMIT $2
```

(`$1` bound to `'%' || q || '%'`; same shape for `tocs` with `atoc_code`.)

New route module `crates/api/src/routes/reference.rs`, following the
`lines.rs` pattern (`pub fn router() -> Router`, handlers take
`State(app)` + `Query`, return `Result<Json<T>, (StatusCode, String)>`,
errors funneled through a local `internal_error` helper):

- `GET /stations?q=<text>` → `Vec<StationSuggestion { code, name }>`
- `GET /tocs?q=<text>` → `Vec<TocSuggestion { code, name }>`

Both cap results at 20. Missing or empty `q` returns `[]` without querying
— this is a type-ahead endpoint, not a listing endpoint. Wired into
`public_router()` in `routes/mod.rs` alongside the existing merges.

## Frontend

`frontend/lib/api.ts` gains:

```ts
searchStations(q: string, signal?: AbortSignal): Promise<Suggestion[]>
searchTocs(q: string, signal?: AbortSignal): Promise<Suggestion[]>
```

New `frontend/lib/useSuggestions.ts` hook: takes a search function and the
current query string, debounces input by 250ms, issues the fetch with an
`AbortController` (aborting the previous in-flight request on every new
keystroke), and returns `{ suggestions: Suggestion[], loading: boolean }`.
Shared by all four fields so the debounce/abort logic exists once.

UI wiring splits by field cardinality, both using Mantine's built-in
`data` prop (`{value, label}[]`) rather than a custom dropdown:

- **Multi-value** (Operators, Destination CRS filter — both already
  `TagsInput`): wire the existing `onSearchChange` to the hook, feed
  `data={suggestions.map(s => ({value: s.code, label: `${s.code} — ${s.name}`}))}`.
  Typed text that matches nothing is still accepted as a free tag —
  preserves today's behavior for any code the reference tables don't have
  yet (data-completeness gaps are a known open item, see
  `.env.example`'s notes on unconfirmed RDM feeds).
- **Single-value** (`CustomLineForm`'s Add station field,
  `StationSearchForm`'s lookup): swap `TextInput` for Mantine
  `Autocomplete`, same `data` shape. Selecting an option sets the field's
  value to the code (`value`), matching today's stored/submitted shape;
  the "CODE — Name" label is dropdown-only display.

## Testing

- Rust: unit tests for `search_stations`/`search_tocs` against a test DB,
  following the existing test patterns in `queries.rs` (empty query,
  code-match, name-match, case-insensitivity, limit enforcement).
- Frontend: tests for `useSuggestions` (debounce timing, abort-on-rekey),
  and that selecting a suggestion in each of the four fields sets the
  right underlying value.
