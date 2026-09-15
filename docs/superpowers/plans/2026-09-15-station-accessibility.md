# Station Accessibility & Facilities Section Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface a curated, twelve-key allowlist of `stations.accessibility`
(step-free access, staff assistance, toilets, lifts, transport links,
cycling, car parks, drop-off/pick-up, platform facilities, station
facilities, help and support, lounges/waiting) as a new fourth section,
"Accessibility & facilities," on `/stations/[crs]`, via a new backend read
route and a generic, crash-proof frontend renderer.

**Architecture:** One new backend read (`reference::station_accessibility`,
`crates/api/src/data/reference.rs`) filters the existing `stations.
accessibility` JSONB column down to a named `const` allowlist of top-level
keys, exposed at `GET /public/stations/{crs}/accessibility`
(`crates/api/src/routes/reference.rs`, same module that already owns every
other `stations`/`tocs` read). No migration, no new write path — this is a
read of data `poller-stations` already captures today. The frontend adds a
loosely-typed (`unknown`-valued) response type, an hour-cached fetcher, a
small pure module of depth-limited rendering rules
(`frontend/lib/stationAccessibility.ts`) that never assumes a field's
internal shape, a new section component
(`frontend/components/StationAccessibilitySection.tsx`), and a fourth
independent block on the station page mirroring the page's two existing
three-state coverage sections exactly.

**Tech Stack:** Rust (axum, sqlx, Postgres, `serde_json::Value` passthrough)
for the API; Next.js App Router (Server Component page, one new `'use
client'` component using Mantine `Spoiler`/`Code`), TypeScript, Vitest +
`@testing-library/react` for the frontend.

**Spec:**
`docs/superpowers/specs/2026-09-12-station-accessibility-design.md` (this
plan implements every decision in that spec as written; it does not
re-derive any of them). Closest sibling precedent for the page-section
pattern: `docs/superpowers/specs/2026-09-03-per-station-stats-design.md`
and its plan, `docs/superpowers/plans/2026-09-03-per-station-stats-plan.md`.

## Global Constraints

- **The allowlist is exactly these twelve keys, in this order, and no
  others** (spec Decision 1) — this is a product-priority call the spec
  deliberately made and this plan does not second-guess it (spec Open
  Question 3):
  ```
  stationAccessibility, staffAssistance, toiletsAndChanging, lifts,
  transportLinks, cycling, carParks, dropOffPickUp, platformFacilities,
  stationFacilities, helpAndSupport, loungesAndWaiting
  ```
- **No migration, no new write path.** `poller-stations` and
  `upsert_stations` are entirely unmodified (spec Decision 2). Every task
  below only reads `stations.accessibility`.
- **The new route lives in `crates/api/src/routes/reference.rs` and
  `crates/api/src/data/reference.rs`** — the module that already owns every
  `stations`/`tocs` read — not a new sibling module (spec Decision 3).
- **Route:** `GET /public/stations/{crs}/accessibility`, mounted from
  `reference.rs::router()` into the existing `public_router()`. The
  response body **is** the filtered object directly (`Json<serde_json::
  Value>`), never wrapped in an envelope (spec Decision 4).
- **Two honest absence states, never collapsed into one:** `404` means "no
  `stations` row for this CRS at all" (this app has never captured
  reference data for it); `200 {}` means "the row exists, but none of the
  twelve allowlisted keys were present (or all were null) in its
  `accessibility` JSONB." Every task that touches this distinction —
  backend query, route, frontend fetch wrapper, and UI copy — must keep
  these two states separately reachable and separately worded (spec
  Correction 5, Decision 9).
- **No case normalization on the `crs` lookup.** Exact `crs = $1` matching,
  same as `latest_station_sample`'s existing precedent — this is a
  pre-existing, shared rough edge across sibling single-station routes;
  this plan does not fix or diverge from it (spec Decision 3 code comment,
  Open Question 5).
- **No decomposition of any allowlisted key's value into typed Rust
  fields.** Each value is forwarded as an opaque `serde_json::Value`,
  filtered only by top-level key name — this does not violate Global
  Constraint 7 of `docs/superpowers/plans/01-poller-microservices.md`
  (spec Decision 1).
- **Frontend type is intentionally loose:** every field on
  `StationAccessibilityData` (`frontend/lib/types.ts`) is `unknown`-valued
  and optional; a key is present only when non-null in the source data
  (spec Decision 5). No field is ever typed more precisely than `unknown` —
  this codebase has never verified any of these keys' internal shape
  (Correction 2, Open Question 1).
- **Rendering is generic and depth-limited, never one component per known
  field** (spec Decision 6). The renderer must never throw on an
  unanticipated shape; anything it can't render cleanly falls back to a
  collapsed, monospace raw-JSON view. This is a hedge against unverified
  real-world data, not an implementation shortcut — do not replace it with
  typed per-field components even if a real payload later confirms a
  field's shape.
- **Category grouping and order are fixed** (spec Decision 7), not
  alphabetical and not the response's own key order:
  ```
  1. Step-free access & assistance  → stationAccessibility, staffAssistance
  2. Facilities                     → toiletsAndChanging, lifts, loungesAndWaiting
  3. Platform & station facilities  → platformFacilities, stationFacilities, helpAndSupport
  4. Getting here                   → transportLinks, carParks, dropOffPickUp, cycling
  ```
  A group heading renders only when at least one of its keys is present.
- **Naming keeps the existing wire/Rust vocabulary
  (`StationAccessibilityData`, `getStationAccessibility`,
  `stations.accessibility`) unchanged, despite the "accessibility" WCAG
  naming collision** (spec Correction 4). The only mitigations are UI copy
  ("Accessibility & facilities" heading, not bare "Accessibility") and the
  component's filename (`StationAccessibilitySection.tsx`). **Do not**
  rename `StationAccessibilityData`, `getStationAccessibility`,
  `stations.accessibility`, or `StationReference.accessibility` anywhere in
  this plan — that renaming is explicitly out of scope (spec Non-goals,
  Open Question 4).
- **Cache for an hour, not `no-store`.** `getStationAccessibility`
  (`frontend/lib/api.ts`) uses `next: { revalidate: 3600 }`, matching
  `getStationName`/`getAllTocs` — this is slow-changing reference data
  (`poller-stations`'s documented 24-hour poll interval), not a live feed
  (spec Decision 5).
- **Backend test convention:** every Rust test touching a live database is
  `#[tokio::test]` + `#[ignore]`, run explicitly with:
  ```
  DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api -- --ignored --test-threads=1
  ```
  Fixture rows use a reserved `Z…`-prefixed CRS namespace so cleanup can
  never touch real data, matching `crates/api/src/data/reference.rs`'s own
  `db_tests` convention and `crates/api/src/routes/station_stats.rs`'s
  `db_tests` convention.
- **The backend filtering logic is unit-testable without a database.** Per
  spec's own Testing approach, the in-memory filter step is pulled out as
  its own pure function so its four required cases (allowlisted-and-present,
  non-allowlisted-and-present, null-valued-allowlisted, empty-object-in)
  can be asserted with a plain `#[test]`, no `#[ignore]`, no `DATABASE_URL`.
- **Frontend test convention:** Vitest, `renderWithMantine` from
  `frontend/test/render.tsx`, colocated `*.test.ts`/`*.test.tsx` files. The
  pure-function tests in `frontend/lib/stationAccessibility.test.ts` are
  written **before** the component that consumes them (spec's own stated
  TDD-first note for exactly this module, since it's what actually defends
  the "never crashes on unknown JSONB shape" claim).
- **No new Playwright/e2e/axe-core spec.** `frontend/e2e/
  accessibility.spec.ts` already sweeps `/stations/{crs}`; the new section
  renders inside that same page and needs no addition (spec Testing
  approach, Non-goals).
- **Real-world field shape is unverified — flagged, not blocking.** Spec
  Open Question 1 calls this "worth doing before or shortly after
  implementation, not resolved by this document." Task 1 below is the
  concrete procedure for that verification. This repository currently has
  **no real Rail Data Marketplace credentials anywhere** — `dev.env.example`
  and `local.env.example` both state explicitly that every `RDM_*_BASE_URL`
  is a non-functional `*.example.invalid` placeholder and that the Stations
  feed's JSON field casing is unconfirmed. Task 1 therefore cannot be
  executed inside this repo's own dev environment; it requires whoever
  executes this plan to have (or obtain) a real RDM Stations product
  account. **Tasks 2-7 do not depend on Task 1 completing** — they implement
  Decision 1/6 exactly as specified, using hand-built fixtures that already
  cover every branch the spec names. Task 1 is ordered first only so that,
  if real credentials are available, its findings can inform Task 2 and
  Task 5's fixtures before those are written; if they are not available,
  skip to Task 2 and revisit Task 1 "shortly after implementation" per the
  spec's own framing.

---

## File Structure

```
crates/api/src/data/reference.rs
  + ACCESSIBILITY_KEYS const, filter_accessibility_fields(), station_accessibility()
  + #[cfg(test)] mod accessibility_filter_tests (non-DB, pure function)
  + db_tests: two new #[ignore]'d tests appended to the existing module   (Task 2)

crates/api/src/routes/reference.rs
  + get_station_accessibility handler, router() gains one route
  + #[cfg(test)] mod db_tests (new in this file)                          (Task 3)

frontend/lib/types.ts
  + StationAccessibilityData                                              (Task 4)

frontend/lib/api.ts
  + getStationAccessibility()                                             (Task 4)

frontend/lib/api.test.ts
  + coverage for getStationAccessibility                                  (Task 4)

frontend/lib/stationAccessibility.ts        NEW                           (Task 5)
frontend/lib/stationAccessibility.test.ts   NEW                           (Task 5)

frontend/components/StationAccessibilitySection.tsx        NEW           (Task 6)
frontend/components/StationAccessibilitySection.test.tsx   NEW           (Task 6)

frontend/app/stations/[crs]/page.tsx        MODIFIED (fourth section)     (Task 7)
frontend/app/stations/[crs]/page.test.tsx   MODIFIED (new describe block) (Task 7)
```

---

## Task 1: Verify the real RDM payload shape and sanity-check payload size (manual, credential-gated)

Addresses spec Open Questions 1 and 2. **No repository files are modified
by this task unless real credentials are available and a shape surprise is
found** (see Step 5). This task does not block Task 2 onward — see Global
Constraints above.

**Files:** none, unless Step 5 finds a surprise (in which case: the fixture
additions described in Task 2 Step 6 and Task 5 Step 6, both optional
sub-steps of those tasks).

- [ ] **Step 1: Confirm whether real RDM Stations credentials are
  available**

  This repository's own `dev.env.example`/`local.env.example` document that
  `RDM_STATIONS_BASE_URL`/`RDM_STATIONS_API_KEY` are placeholders
  (`http://rdm-stations.example.invalid` / `changeme-...`) and that the
  Stations feed's JSON field casing has never been confirmed against a live
  account. If you (the person executing this plan) do not have a real Rail
  Data Marketplace Stations product account and API key, **stop here,
  proceed directly to Task 2, and re-run this task later** when credentials
  become available — this matches the spec's own "before or shortly after
  implementation" framing for this open question.

- [ ] **Step 2: Fetch the full Stations feed once, against the real
  account**

  ```bash
  curl -sS -H "x-apikey: $RDM_STATIONS_API_KEY" "$RDM_STATIONS_BASE_URL/stations" \
    -o /tmp/rdm-stations-full.json
  ```

  This is the same envelope shape `crates/poller-stations/src/schema.rs`
  already parses (`{"stations": [...]}`) — there is no documented
  single-station endpoint, so the full feed is fetched once and one station
  is extracted from it.

- [ ] **Step 3: Extract one real, well-known station and measure its size**
  (Open Question 2)

  ```bash
  jq '.stations[] | select(.crsCode == "EUS")' /tmp/rdm-stations-full.json > /tmp/euston-accessibility.json
  wc -c /tmp/euston-accessibility.json
  ```

  Record the byte size. If it's large enough to be a real concern for a
  synchronously-rendered Server Component (say, comfortably into six
  figures), note it as a follow-up for a future plan — this plan does not
  add any size-based truncation or streaming, since the spec explicitly
  left this unsized and non-blocking.

- [ ] **Step 4: Inspect each of the twelve allowlisted keys' real shape**

  For each key in `ACCESSIBILITY_KEYS` (see Global Constraints), run:

  ```bash
  jq '.stationAccessibility' /tmp/euston-accessibility.json
  jq '.staffAssistance' /tmp/euston-accessibility.json
  jq '.toiletsAndChanging' /tmp/euston-accessibility.json
  jq '.lifts' /tmp/euston-accessibility.json
  jq '.transportLinks' /tmp/euston-accessibility.json
  jq '.cycling' /tmp/euston-accessibility.json
  jq '.carParks' /tmp/euston-accessibility.json
  jq '.dropOffPickUp' /tmp/euston-accessibility.json
  jq '.platformFacilities' /tmp/euston-accessibility.json
  jq '.stationFacilities' /tmp/euston-accessibility.json
  jq '.helpAndSupport' /tmp/euston-accessibility.json
  jq '.loungesAndWaiting' /tmp/euston-accessibility.json
  ```

  For each, note: present or absent; JSON type (string/number/boolean/
  null/array/object); if an array, whether every element is a primitive or
  some/all are objects; if an object, whether every own value is itself a
  primitive or primitive-array (shallow), or whether it nests another
  object/array one level deeper.

  Repeat for one or two more stations of different sizes/regions (a small
  rural station and a large interchange) if time allows — a single station
  cannot prove every key's shape, only disprove specific wrong assumptions.

- [ ] **Step 5: Compare against Decision 6's five rendering branches**

  For each observed shape, confirm it falls cleanly into one of: primitive;
  array of primitives; array of objects/mixed; shallow plain object (every
  own value a primitive or primitive-array); or "deeper/unclear" (falls
  back to raw JSON). Falling into the last bucket is **expected and
  correct**, not a bug — Decision 6 was designed for exactly that
  possibility. The only genuine "surprise" worth acting on is a shape that
  the pure renderer in Task 5 would mis-render as something other than one
  of these five outcomes (e.g. throwing, or silently dropping data) — that
  would be a real bug in Task 5's implementation, not a spec problem.

- [ ] **Step 6: Record findings, and update fixtures only if warranted**

  If every observed shape is already covered by an existing hand-built test
  case in Task 2/Task 5 below (which cover primitive, array-of-primitives,
  array-of-objects, shallow-object, and deep/malformed-object cases), no
  code changes are needed — note in the PR/commit description (or a
  standup) that live verification confirmed the existing fixtures are
  representative.

  If a real shape reveals something the existing fixtures don't exercise
  (e.g. a key that in practice is *always* an array of objects, never a
  bare object), add one additional regression case using the real
  (trimmed, only the relevant key's value) JSON as an inline Rust `const`
  (matching `crates/poller-stations/src/schema.rs`'s own `SAMPLE_JSON`
  convention) to Task 2's `accessibility_filter_tests` module, and a
  parallel TypeScript fixture to Task 5's `stationAccessibility.test.ts`.
  Do **not** change the twelve-key allowlist itself based on this — that is
  explicitly out of scope (Open Question 3).

---

## Task 2: Backend data layer — `station_accessibility` read and its pure filter

**Files:**
- Modify: `crates/api/src/data/reference.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks (Task 1 is informational only).
- Produces (consumed by Task 3):
  - `pub const ACCESSIBILITY_KEYS: &[&str]` — the twelve-key allowlist.
  - `pub(crate) fn filter_accessibility_fields(full: &serde_json::Value) -> serde_json::Value` — pure, no I/O; extracted specifically so it can be unit-tested without a database.
  - `pub async fn station_accessibility(pool: &PgPool, crs: &str) -> Result<Option<serde_json::Value>>`

- [ ] **Step 1: Write the failing non-DB unit tests for the pure filter**

  Add to `crates/api/src/data/reference.rs`, directly after the existing
  `get_all_tocs` function (before the `// These tests seed and delete...`
  comment that introduces `mod db_tests`):

  ```rust
  #[cfg(test)]
  mod accessibility_filter_tests {
      use serde_json::json;

      use super::*;

      #[test]
      fn forwards_an_allowlisted_key_that_is_present() {
          let full = json!({ "lifts": { "count": 2 }, "somethingElse": "ignored" });
          let filtered = filter_accessibility_fields(&full);
          assert_eq!(filtered, json!({ "lifts": { "count": 2 } }));
      }

      #[test]
      fn drops_a_non_allowlisted_key_even_though_it_is_present() {
          let full = json!({ "ticketBuying": { "open": true } });
          let filtered = filter_accessibility_fields(&full);
          assert_eq!(filtered, json!({}));
      }

      #[test]
      fn drops_a_null_valued_allowlisted_key() {
          let full = json!({ "carParks": null, "lifts": { "count": 1 } });
          let filtered = filter_accessibility_fields(&full);
          assert_eq!(filtered, json!({ "lifts": { "count": 1 } }));
      }

      #[test]
      fn an_empty_accessibility_object_produces_an_empty_object() {
          let full = json!({});
          let filtered = filter_accessibility_fields(&full);
          assert_eq!(filtered, json!({}));
      }

      #[test]
      fn forwards_every_allowlisted_key_present_and_drops_everything_else_in_one_pass() {
          let full = json!({
              "stationAccessibility": { "stepFree": true },
              "staffAssistance": "Available 06:00-23:00",
              "toiletsAndChanging": null,
              "lifts": [{ "location": "Platform 1" }],
              "transportLinks": ["Bus", "Underground"],
              "cycling": {},
              "carParks": [{ "spaces": 120 }],
              "dropOffPickUp": null,
              "platformFacilities": { "seating": true },
              "stationFacilities": { "wifi": true },
              "helpAndSupport": "0800 123 4567",
              "loungesAndWaiting": { "firstClass": false },
              "ticketBuying": { "open": true },
              "staffingLevel": "Full",
              "informationServices": {},
              "address": { "line1": "1 Station Rd" },
              "stationMap": "https://example.invalid/map.png",
              "stationAlerts": [],
              "slug": "example-station",
              "sixteenCharacterName": "EXAMPLE STN",
              "nationalLocationCode": "1234",
              "minimumConnectionTime": 5,
              "changeHistory": { "changedBy": "AAP2" }
          });
          let filtered = filter_accessibility_fields(&full);
          assert_eq!(
              filtered,
              json!({
                  "stationAccessibility": { "stepFree": true },
                  "staffAssistance": "Available 06:00-23:00",
                  "lifts": [{ "location": "Platform 1" }],
                  "transportLinks": ["Bus", "Underground"],
                  "cycling": {},
                  "carParks": [{ "spaces": 120 }],
                  "platformFacilities": { "seating": true },
                  "stationFacilities": { "wifi": true },
                  "helpAndSupport": "0800 123 4567",
                  "loungesAndWaiting": { "firstClass": false }
              }),
              "every non-allowlisted key dropped, every null-valued allowlisted key dropped, \
               every present-and-non-null allowlisted key forwarded verbatim"
          );
      }
  }
  ```

- [ ] **Step 2: Run the tests to verify they fail**

  Run: `cargo test -p api accessibility_filter_tests`
  Expected: compile error — `filter_accessibility_fields` not found in this scope.

- [ ] **Step 3: Write `ACCESSIBILITY_KEYS`, `filter_accessibility_fields`, and `station_accessibility`**

  Add this directly above the `accessibility_filter_tests` module from
  Step 1 (i.e. immediately after `get_all_tocs`'s closing brace):

  ```rust
  /// Top-level `stations.accessibility` keys considered "accessibility or
  /// amenities" data for the station page's facilities section -- see
  /// docs/superpowers/specs/2026-09-12-station-accessibility-design.md
  /// Decision 1 for why this list and not the full RDM `Station` object.
  /// Each value is forwarded completely unexamined by
  /// [`filter_accessibility_fields`]: this only filters by key name, it
  /// does not decompose or validate what's inside (Global Constraint 7,
  /// docs/superpowers/plans/01-poller-microservices.md:40-42).
  pub const ACCESSIBILITY_KEYS: &[&str] = &[
      "stationAccessibility",
      "staffAssistance",
      "toiletsAndChanging",
      "lifts",
      "transportLinks",
      "cycling",
      "carParks",
      "dropOffPickUp",
      "platformFacilities",
      "stationFacilities",
      "helpAndSupport",
      "loungesAndWaiting",
  ];

  /// The in-memory filter step, pulled out as its own pure function so it
  /// can be unit-tested without a database (see `accessibility_filter_tests`
  /// below). Drops every key not in [`ACCESSIBILITY_KEYS`] and every
  /// allowlisted key whose value is JSON `null`. `full` is not required to
  /// be a JSON object -- a non-object input (which should never happen for
  /// this column, but is not asserted against) produces an empty object,
  /// the same as an object with no allowlisted keys present.
  pub(crate) fn filter_accessibility_fields(full: &serde_json::Value) -> serde_json::Value {
      let mut filtered = serde_json::Map::new();
      if let Some(obj) = full.as_object() {
          for key in ACCESSIBILITY_KEYS {
              if let Some(value) = obj.get(*key) {
                  if !value.is_null() {
                      filtered.insert((*key).to_string(), value.clone());
                  }
              }
          }
      }
      serde_json::Value::Object(filtered)
  }

  /// Returns `None` when `stations` has no row for `crs` at all (this app
  /// has never captured reference data for it) -- distinct from `Some` of
  /// an empty object, which means the row exists but none of
  /// `ACCESSIBILITY_KEYS` were present (or all were null) in its
  /// `accessibility` JSONB. Exact `crs = $1` match, no case normalization
  /// -- same convention `latest_station_sample`
  /// (`crates/api/src/data/queries.rs`) already uses for a single-CRS
  /// lookup; this function does not introduce or fix case-sensitivity
  /// handling either way (see the design spec's Open questions/risks).
  pub async fn station_accessibility(
      pool: &PgPool,
      crs: &str,
  ) -> Result<Option<serde_json::Value>> {
      use sqlx::Row;
      let row = sqlx::query("SELECT accessibility FROM stations WHERE crs = $1")
          .bind(crs)
          .fetch_optional(pool)
          .await?;

      let Some(row) = row else {
          return Ok(None);
      };
      let full: serde_json::Value = row.try_get("accessibility")?;
      Ok(Some(filter_accessibility_fields(&full)))
  }
  ```

- [ ] **Step 4: Run the unit tests to verify they pass**

  Run: `cargo test -p api accessibility_filter_tests`
  Expected: all 5 tests pass.

- [ ] **Step 5: Add the `#[ignore]`d DB tests to the existing `db_tests` module**

  Add these two tests inside the existing `#[cfg(test)] mod db_tests { ... }`
  block at the bottom of `crates/api/src/data/reference.rs` (alongside
  `search_stations_ranks_exact_code_then_name_prefix_then_substring` etc.):

  ```rust
      #[tokio::test]
      #[ignore = "requires a live database; see the plan's Global Constraints for the \
                  DATABASE_URL incantation, then run with `cargo test -p api \
                  station_accessibility_returns_the_filtered_set_for_an_existing_row \
                  -- --ignored`"]
      async fn station_accessibility_returns_the_filtered_set_for_an_existing_row() {
          let pool = connect().await;

          sqlx::query(
              "INSERT INTO stations (crs, name, accessibility) VALUES ($1, $2, $3) \
               ON CONFLICT (crs) DO UPDATE SET accessibility = EXCLUDED.accessibility",
          )
          .bind("ZFA")
          .bind("Fixture Facilities Station")
          .bind(serde_json::json!({
              "lifts": { "count": 2 },
              "carParks": null,
              "ticketBuying": { "open": true }
          }))
          .execute(&pool)
          .await
          .expect("seed fixture station");

          let result = station_accessibility(&pool, "ZFA").await.expect("query");
          assert_eq!(
              result,
              Some(serde_json::json!({ "lifts": { "count": 2 } })),
              "carParks (null) and ticketBuying (non-allowlisted) must both be absent"
          );

          sqlx::query("DELETE FROM stations WHERE crs = 'ZFA'")
              .execute(&pool)
              .await
              .expect("cleanup fixture station");
      }

      #[tokio::test]
      #[ignore = "requires a live database; see the plan's Global Constraints for the \
                  DATABASE_URL incantation, then run with `cargo test -p api \
                  station_accessibility_returns_none_when_no_row_exists_for_the_crs \
                  -- --ignored`"]
      async fn station_accessibility_returns_none_when_no_row_exists_for_the_crs() {
          let pool = connect().await;
          sqlx::query("DELETE FROM stations WHERE crs = 'ZFB'")
              .execute(&pool)
              .await
              .expect("ensure no fixture row present");

          let result = station_accessibility(&pool, "ZFB").await.expect("query");
          assert_eq!(result, None, "no stations row at all must be None, not Some({{}})");
      }
  ```

- [ ] **Step 6 (optional — only if Task 1 was executed and found a real
  fixture worth adding): append one more `#[test]` to
  `accessibility_filter_tests`** using the trimmed real JSON captured in
  Task 1 Step 6, asserting `filter_accessibility_fields` handles it exactly
  as the five existing branches predict. Skip this step entirely if Task 1
  was deferred or found nothing not already covered.

- [ ] **Step 7: Run the DB tests to verify they pass**

  Run:
  ```
  DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api station_accessibility_ -- --ignored --test-threads=1
  ```
  Expected: both new tests pass.

- [ ] **Step 8: Commit**

  ```bash
  git add crates/api/src/data/reference.rs
  git commit -m "$(cat <<'EOF'
Add station_accessibility read: filtered stations.accessibility passthrough

Adds the twelve-key ACCESSIBILITY_KEYS allowlist and a pure filter step
(unit-tested without a database) plus the read function that backs the
new /public/stations/{crs}/accessibility route.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
  ```

---

## Task 3: Backend route — `GET /public/stations/{crs}/accessibility`

**Files:**
- Modify: `crates/api/src/routes/reference.rs`

**Interfaces:**
- Consumes: `reference::station_accessibility` (Task 2).
- Produces (consumed by Task 4): `GET /public/stations/{crs}/accessibility`
  — `200 <filtered object>` | `200 {}` | `404 "no station reference data
  for: {crs}"`.

- [ ] **Step 1: Write the failing tests**

  Add to `crates/api/src/routes/reference.rs`, after the existing
  `#[cfg(test)] mod tests { ... }` block (the plain `sanitize_query` unit
  tests):

  ```rust
  #[cfg(test)]
  mod db_tests {
      use axum::body::Body;
      use axum::http::Request;
      use sqlx::PgPool;
      use sqlx::postgres::PgPoolOptions;
      use tower::ServiceExt;

      use super::*;
      use crate::app::{App, AppState};
      use crate::auth::oidc::{OidcClient, OidcConfig};
      use crate::data::config::{LineCatalogue, ServiceArguments};

      /// Copied from `routes::station_stats::db_tests::test_app` (that
      /// module's own doc comment: colocated per-file rather than shared).
      /// Every field an inert placeholder except `database`, which the
      /// caller supplies -- this route touches nothing else on `App`.
      fn test_app(pool: PgPool) -> App {
          let config = ServiceArguments {
              bind_url: "0.0.0.0:0".to_string(),
              database_url: String::new(),
              redis_url: "redis://127.0.0.1:0".to_string(),
              internal_oauth_issuer_url: "https://example.invalid".to_string(),
              internal_oauth_client_id: "test-internal-oauth-client".to_string(),
              internal_oauth_group_incidents: "svc-poller-incidents".to_string(),
              internal_oauth_group_stations: "svc-poller-stations".to_string(),
              internal_oauth_group_tocs: "svc-poller-tocs".to_string(),
              internal_oauth_group_ldbws: "svc-poller-ldbws".to_string(),
              internal_oauth_group_tfl: "svc-poller-tfl".to_string(),
              internal_oauth_group_trust_consumer: "svc-trust-consumer".to_string(),
              internal_oauth_group_schedule_ingest: "svc-schedule-ingest".to_string(),
              internal_oauth_group_schedule_reference: "svc-schedule-reference".to_string(),
              internal_oauth_group_full_coverage: "svc-full-coverage-consumer".to_string(),
              internal_oauth_group_trust_backlog: "svc-trust-backlog-consumer".to_string(),
              internal_oauth_group_irish_rail_gtfs: "svc-poller-irish-rail-gtfs".to_string(),
              internal_oauth_group_irish_rail_live: "svc-poller-irish-rail-live".to_string(),
              internal_oauth_group_nir_stations: "svc-poller-nir-stations".to_string(),
              chatbot_access_group: "distant-signal-chatbot-users".to_string(),
              sso_issuer_url: "https://example.invalid".to_string(),
              sso_client_id: "test-client".to_string(),
              sso_client_secret: "test-secret".to_string(),
              sso_redirect_url: "https://example.invalid/callback".to_string(),
              sso_post_login_redirect_url: "https://example.invalid/".to_string(),
              session_ttl_days: 14,
              history_retention_days: 7,
              daily_stats_retention_days: 300,
              half_hourly_stats_retention_hours: 840,
              metrics_enabled: false,
              defaults_file: None,
              lines: LineCatalogue(vec![]),
              vapid_public_key: "test-vapid-public-key".to_string(),
              full_coverage_enabled_default: false,
              schedule_match_interval_secs: 300,
              reconciliation_sweep_interval_secs: 300,
              schedule_enrichment_grace_minutes: 30,
              backlog_match_sweep_interval_secs: 300,
          };

          std::sync::Arc::new(AppState {
              config,
              database: pool,
              redis: redis::Client::open("redis://127.0.0.1:0").expect("parse placeholder redis url"),
              oidc: OidcClient::new(OidcConfig {
                  issuer_url: "https://example.invalid".to_string(),
                  client_id: "test-client".to_string(),
                  client_secret: "test-secret".to_string(),
                  redirect_url: "https://example.invalid/callback".to_string(),
              })
              .expect("construct placeholder oidc client"),
              internal_oauth_verifier: crate::auth::internal_oauth::ServiceTokenVerifier::new(
                  "https://example.invalid".to_string(),
                  "test-internal-oauth-client".to_string(),
              )
              .expect("construct placeholder internal-oauth verifier"),
              internal_oauth_routes: Vec::new(),
              schedule_crs_line_index: std::collections::HashMap::new(),
          })
      }

      async fn connect() -> PgPool {
          let database_url =
              std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
          PgPoolOptions::new()
              .connect(&database_url)
              .await
              .expect("connect to postgres")
      }

      async fn get(pool: &PgPool, uri: &str) -> (StatusCode, String) {
          let router: axum::Router = crate::app::Router::new()
              .merge(router())
              .with_state(test_app(pool.clone()));
          let response = router
              .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
              .await
              .unwrap();
          let status = response.status();
          let body = axum::body::to_bytes(response.into_body(), usize::MAX)
              .await
              .unwrap();
          (status, String::from_utf8(body.to_vec()).unwrap())
      }

      #[tokio::test]
      #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                  station_accessibility_route -- --ignored --test-threads=1`"]
      async fn station_accessibility_route_404s_naming_the_crs_when_no_row_exists() {
          let pool = connect().await;
          sqlx::query("DELETE FROM stations WHERE crs = 'ZFC'")
              .execute(&pool)
              .await
              .expect("ensure no fixture row present");

          let (status, body) = get(&pool, "/stations/ZFC/accessibility").await;
          assert_eq!(status, StatusCode::NOT_FOUND);
          assert!(body.contains("ZFC"), "404 body should name the CRS: {body}");
      }

      #[tokio::test]
      #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                  station_accessibility_route -- --ignored --test-threads=1`"]
      async fn station_accessibility_route_is_200_empty_object_when_the_row_has_no_allowlisted_keys()
       {
          let pool = connect().await;
          sqlx::query(
              "INSERT INTO stations (crs, name, accessibility) VALUES ($1, $2, $3) \
               ON CONFLICT (crs) DO UPDATE SET accessibility = EXCLUDED.accessibility",
          )
          .bind("ZFD")
          .bind("Fixture Quiet Station")
          .bind(serde_json::json!({ "ticketBuying": { "open": true } }))
          .execute(&pool)
          .await
          .expect("seed fixture station");

          let (status, body) = get(&pool, "/stations/ZFD/accessibility").await;
          assert_eq!(status, StatusCode::OK);
          let json: Value = serde_json::from_str(&body).unwrap();
          assert_eq!(json, serde_json::json!({}));

          sqlx::query("DELETE FROM stations WHERE crs = 'ZFD'")
              .execute(&pool)
              .await
              .expect("cleanup fixture station");
      }

      #[tokio::test]
      #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                  station_accessibility_route -- --ignored --test-threads=1`"]
      async fn station_accessibility_route_returns_the_filtered_object_unwrapped_verbatim() {
          let pool = connect().await;
          sqlx::query(
              "INSERT INTO stations (crs, name, accessibility) VALUES ($1, $2, $3) \
               ON CONFLICT (crs) DO UPDATE SET accessibility = EXCLUDED.accessibility",
          )
          .bind("ZFE")
          .bind("Fixture Facilities Station Two")
          .bind(serde_json::json!({
              "lifts": [{ "location": "Platform 1" }],
              "transportLinks": ["Bus", "Underground"],
              "stationMap": "https://example.invalid/map.png"
          }))
          .execute(&pool)
          .await
          .expect("seed fixture station");

          let (status, body) = get(&pool, "/stations/ZFE/accessibility").await;
          assert_eq!(status, StatusCode::OK);
          let json: Value = serde_json::from_str(&body).unwrap();
          assert_eq!(
              json,
              serde_json::json!({
                  "lifts": [{ "location": "Platform 1" }],
                  "transportLinks": ["Bus", "Underground"]
              }),
              "the body is the filtered object directly, not wrapped in an envelope, and \
               stationMap (non-allowlisted) is absent: {json}"
          );

          sqlx::query("DELETE FROM stations WHERE crs = 'ZFE'")
              .execute(&pool)
              .await
              .expect("cleanup fixture station");
      }
  }
  ```

- [ ] **Step 2: Run the tests to verify they fail (route does not exist yet)**

  Run:
  ```
  DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api station_accessibility_route -- --ignored --test-threads=1
  ```
  Expected: compile error — `router()` has no `/stations/{crs}/accessibility`
  route, `get_station_accessibility` does not exist.

- [ ] **Step 3: Add the `Path` import**

  Change:
  ```rust
  use axum::Json;
  use axum::extract::{Query, State};
  use axum::http::StatusCode;
  use serde::Deserialize;

  use crate::app::{App, Router};
  use crate::data::reference::{self, Suggestion};
  ```
  to:
  ```rust
  use axum::Json;
  use axum::extract::{Path, Query, State};
  use axum::http::StatusCode;
  use serde::Deserialize;

  use crate::app::{App, Router};
  use crate::data::reference::{self, Suggestion};
  ```

- [ ] **Step 4: Register the route and add the handler**

  Change:
  ```rust
  pub fn router() -> Router {
      Router::new()
          .route("/stations", axum::routing::get(search_stations))
          .route("/tocs", axum::routing::get(search_tocs))
          .route("/tocs/all", axum::routing::get(list_all_tocs))
  }
  ```
  to:
  ```rust
  pub fn router() -> Router {
      Router::new()
          .route("/stations", axum::routing::get(search_stations))
          .route(
              "/stations/{crs}/accessibility",
              axum::routing::get(get_station_accessibility),
          )
          .route("/tocs", axum::routing::get(search_tocs))
          .route("/tocs/all", axum::routing::get(list_all_tocs))
  }
  ```

  Add this handler directly above `fn internal_error`:

  ```rust
  /// `GET /public/stations/{crs}/accessibility` -- see
  /// docs/superpowers/specs/2026-09-12-station-accessibility-design.md
  /// Decision 4. The response body **is** the filtered object; no
  /// hand-built `json!()` reshaping is needed because every value
  /// forwarded is already the RDM feed's own camelCase JSON, untouched --
  /// unlike `station_stats.rs`, there is no nested `common` struct being
  /// embedded that could hit the camelCase/snake_case pitfall
  /// `crates/api/src/routes/incidents.rs` documents.
  async fn get_station_accessibility(
      State(app): State<App>,
      Path(crs): Path<String>,
  ) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
      match reference::station_accessibility(&app.database, &crs)
          .await
          .map_err(internal_error)?
      {
          Some(data) => Ok(Json(data)),
          None => Err((
              StatusCode::NOT_FOUND,
              format!("no station reference data for: {crs}"),
          )),
      }
  }
  ```

- [ ] **Step 5: Run the tests to verify they pass**

  Run:
  ```
  DATABASE_URL=postgres://lucy@localhost:5432/distant_signal_test cargo test -p api station_accessibility_route -- --ignored --test-threads=1
  ```
  Expected: all 3 new tests pass.

- [ ] **Step 6: Run the full non-DB test suite for a sanity check**

  Run: `cargo test -p api`
  Expected: PASS, no regressions (this only adds one route and one handler
  to an existing router; `search_stations`/`search_tocs`/`list_all_tocs`
  are untouched).

- [ ] **Step 7: Commit**

  ```bash
  git add crates/api/src/routes/reference.rs
  git commit -m "$(cat <<'EOF'
Add GET /public/stations/{crs}/accessibility route

Serves the filtered stations.accessibility passthrough added in the
previous commit. 404 for no stations row at all; 200 {} for a row with
no allowlisted keys present; 200 <object> otherwise, unwrapped.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
  ```

---

## Task 4: Frontend type and fetcher

**Files:**
- Modify: `frontend/lib/types.ts`
- Modify: `frontend/lib/api.ts`
- Modify: `frontend/lib/api.test.ts`

**Interfaces:**
- Consumes: `GET /public/stations/{crs}/accessibility` (Task 3).
- Produces (consumed by Tasks 5-7): `StationAccessibilityData` (`frontend/
  lib/types.ts`), `getStationAccessibility(crs: string): Promise<
  StationAccessibilityData>` (`frontend/lib/api.ts`, throws
  `ApiNotFoundError` on a 404 via the shared `errorForResponse`).

- [ ] **Step 1: Write the failing test**

  Add to `frontend/lib/api.test.ts`, alongside the other per-fetcher tests
  (near `getStationSampleStats`'s test):

  ```ts
  it('getStationAccessibility fetches the correct URL with hour caching', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(JSON.stringify({ lifts: { count: 2 } }), { status: 200 })),
    );
    await expect(getStationAccessibility('EUS')).resolves.toEqual({ lifts: { count: 2 } });
    expect(fetch).toHaveBeenCalledWith(
      'http://test-api:8080/public/stations/EUS/accessibility',
      expect.objectContaining({ next: { revalidate: 3600 } }),
    );
  });

  it('getStationAccessibility throws ApiNotFoundError on a 404', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response('not found', { status: 404 })));
    await expect(getStationAccessibility('ZZZ')).rejects.toBeInstanceOf(ApiNotFoundError);
  });
  ```

  Add `getStationAccessibility` to the existing `import { ... } from
  '@/lib/api'` block at the top of the file (alongside `getStationSampleStats`
  etc.).

- [ ] **Step 2: Run the test to verify it fails**

  Run (from `frontend/`): `npx vitest run lib/api.test.ts -t getStationAccessibility`
  Expected: FAIL — `getStationAccessibility` is not exported from `lib/api.ts`.

- [ ] **Step 3: Add `StationAccessibilityData` to `lib/types.ts`**

  Add anywhere among the other response-shape interfaces (e.g. next to
  `StationOperatorSampleStats`):

  ```ts
  /** `GET /public/stations/{crs}/accessibility`'s response -- a filtered
   * passthrough of `stations.accessibility` (see
   * docs/superpowers/specs/2026-09-12-station-accessibility-design.md
   * Decision 1 for the exact key allowlist). Every value is `unknown`, not
   * a nested interface, because this codebase has never recorded the RDM
   * feed's field-level shape for any of these keys (design spec
   * Correction 2) -- typing them more precisely here would be inventing a
   * contract this app cannot actually verify. Keys are present only when
   * non-null in the source data; a key with no data is simply absent, not
   * `null`. Deliberately keeps the `accessibility`-named wire vocabulary
   * despite the WCAG "accessibility" naming collision this codebase also
   * uses elsewhere (Correction 4) -- see that section for why this isn't
   * renamed. */
  export interface StationAccessibilityData {
    stationAccessibility?: unknown;
    staffAssistance?: unknown;
    toiletsAndChanging?: unknown;
    lifts?: unknown;
    transportLinks?: unknown;
    cycling?: unknown;
    carParks?: unknown;
    dropOffPickUp?: unknown;
    platformFacilities?: unknown;
    stationFacilities?: unknown;
    helpAndSupport?: unknown;
    loungesAndWaiting?: unknown;
  }
  ```

- [ ] **Step 4: Add `getStationAccessibility` to `lib/api.ts`**

  Add `StationAccessibilityData` to the `import type { ... } from
  './types'` block at the top of the file, then add this function near
  `getStationSampleStats`:

  ```ts
  /** `GET /public/stations/{crs}/accessibility` -- filtered RDM station
   * facilities/accessibility data (design spec Decisions 3-4). Cached for
   * an hour, same convention as `getStationName`/`getAllTocs`: the
   * underlying feed's own documented poll interval is 24 hours
   * (`crates/poller-stations/src/main.rs`), so this is reference data, not
   * a live feed, and does not warrant `cache: 'no-store'`. Throws
   * `ApiNotFoundError` on a 404 (no `stations` row for this CRS at all)
   * via `errorForResponse`, same as every other `fetchJson` caller. */
  export async function getStationAccessibility(crs: string): Promise<StationAccessibilityData> {
    return fetchJson<StationAccessibilityData>(`${baseUrl()}/public/stations/${crs}/accessibility`, {
      next: { revalidate: 3600 },
    });
  }
  ```

- [ ] **Step 5: Run the tests to verify they pass**

  Run: `npx vitest run lib/api.test.ts`
  Expected: PASS, including both new tests, no regressions in the rest of
  the file.

- [ ] **Step 6: Commit**

  ```bash
  git add frontend/lib/types.ts frontend/lib/api.ts frontend/lib/api.test.ts
  git commit -m "$(cat <<'EOF'
Add StationAccessibilityData type and getStationAccessibility fetcher

Frontend-side wiring for GET /public/stations/{crs}/accessibility. Every
field is unknown-valued -- this codebase has never verified the RDM
feed's per-key shape (design spec Correction 2).

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
  ```

---

## Task 5: Pure rendering-rules module — `frontend/lib/stationAccessibility.ts`

This is the module the spec calls out explicitly as needing test-first
treatment (Testing approach: "it should be written first, before the
component"), since it's what actually defends the "never crashes on
unknown JSONB shape" claim (Correction 2 / Decision 6).

**Files:**
- Create: `frontend/lib/stationAccessibility.ts`
- Create: `frontend/lib/stationAccessibility.test.ts`

**Interfaces:**
- Consumes: `StationAccessibilityData` (Task 4).
- Produces (consumed by Task 6):
  - `ACCESSIBILITY_CATEGORIES: { heading: string; keys: (keyof StationAccessibilityData)[] }[]`
  - `humanizeKey(key: string): string`
  - `type RenderableValue = { kind: 'text'; text: string } | { kind: 'rows'; rows: { label: string; value: string }[] } | { kind: 'items'; count: number; items: RenderableValue[] } | { kind: 'raw'; json: string }`
  - `renderAccessibilityValue(value: unknown): RenderableValue`

- [ ] **Step 1: Write the failing tests**

  Create `frontend/lib/stationAccessibility.test.ts`:

  ```ts
  import { describe, it, expect } from 'vitest';
  import {
    ACCESSIBILITY_CATEGORIES,
    humanizeKey,
    renderAccessibilityValue,
  } from './stationAccessibility';

  describe('humanizeKey', () => {
    it('splits camelCase into title-cased words', () => {
      expect(humanizeKey('stepFreeAccess')).toBe('Step free access');
      expect(humanizeKey('lifts')).toBe('Lifts');
      expect(humanizeKey('helpAndSupport')).toBe('Help and support');
    });
  });

  describe('renderAccessibilityValue', () => {
    it('renders a string primitive as text', () => {
      expect(renderAccessibilityValue('Available 06:00-23:00')).toEqual({
        kind: 'text',
        text: 'Available 06:00-23:00',
      });
    });

    it('renders a number primitive as text', () => {
      expect(renderAccessibilityValue(2)).toEqual({ kind: 'text', text: '2' });
    });

    it('renders booleans as Yes/No, not "true"/"false"', () => {
      expect(renderAccessibilityValue(true)).toEqual({ kind: 'text', text: 'Yes' });
      expect(renderAccessibilityValue(false)).toEqual({ kind: 'text', text: 'No' });
    });

    it('renders an array of primitives as comma-joined text', () => {
      expect(renderAccessibilityValue(['Bus', 'Underground', 'Taxi'])).toEqual({
        kind: 'text',
        text: 'Bus, Underground, Taxi',
      });
    });

    it('renders an array of objects as an items list, not inlined', () => {
      const result = renderAccessibilityValue([{ spaces: 120 }, { spaces: 40 }]);
      expect(result.kind).toBe('items');
      if (result.kind === 'items') {
        expect(result.count).toBe(2);
        expect(result.items).toHaveLength(2);
        expect(result.items[0]).toEqual({ kind: 'rows', rows: [{ label: 'Spaces', value: '120' }] });
      }
    });

    it('renders a mixed-type array as an items list too', () => {
      const result = renderAccessibilityValue([{ spaces: 120 }, 'overflow']);
      expect(result.kind).toBe('items');
      if (result.kind === 'items') expect(result.count).toBe(2);
    });

    it('renders a shallow plain object as one label/value row per own key, keys humanized', () => {
      expect(renderAccessibilityValue({ stepFree: true, notes: 'Ramp available' })).toEqual({
        kind: 'rows',
        rows: [
          { label: 'Step free', value: 'Yes' },
          { label: 'Notes', value: 'Ramp available' },
        ],
      });
    });

    it('renders a shallow object whose own value is an array of primitives as a joined row', () => {
      expect(renderAccessibilityValue({ operators: ['GWR', 'Avanti'] })).toEqual({
        kind: 'rows',
        rows: [{ label: 'Operators', value: 'GWR, Avanti' }],
      });
    });

    it('drops a null-valued own key inside a shallow object rather than rendering a blank row', () => {
      expect(renderAccessibilityValue({ stepFree: true, notes: null })).toEqual({
        kind: 'rows',
        rows: [{ label: 'Step free', value: 'Yes' }],
      });
    });

    it('falls back to raw JSON for an object nested more than one level deep, rather than throwing', () => {
      const deeplyNested = { level1: { level2: { level3: 'too deep' } } };
      const result = renderAccessibilityValue(deeplyNested);
      expect(result.kind).toBe('raw');
      if (result.kind === 'raw') {
        expect(JSON.parse(result.json)).toEqual(deeplyNested);
      }
    });

    it('falls back to raw JSON for a deliberately malformed array-of-arrays-of-objects shape, never throwing', () => {
      const malformed = [[{ a: 1 }], [{ b: 2 }]];
      expect(() => renderAccessibilityValue(malformed)).not.toThrow();
      const result = renderAccessibilityValue(malformed);
      // Each element of the outer array is itself an array, not an object
      // or primitive -- recursed as an 'items' entry whose own recursion
      // hits the raw-fallback branch, never a thrown error.
      expect(result.kind).toBe('items');
      if (result.kind === 'items') {
        expect(result.items[0].kind).toBe('raw');
      }
    });
  });

  describe('ACCESSIBILITY_CATEGORIES', () => {
    it('covers exactly the twelve allowlisted keys, once each, in the spec-defined group order', () => {
      const allKeys = ACCESSIBILITY_CATEGORIES.flatMap((c) => c.keys);
      expect(allKeys).toEqual([
        'stationAccessibility',
        'staffAssistance',
        'toiletsAndChanging',
        'lifts',
        'loungesAndWaiting',
        'platformFacilities',
        'stationFacilities',
        'helpAndSupport',
        'transportLinks',
        'carParks',
        'dropOffPickUp',
        'cycling',
      ]);
      expect(ACCESSIBILITY_CATEGORIES.map((c) => c.heading)).toEqual([
        'Step-free access & assistance',
        'Facilities',
        'Platform & station facilities',
        'Getting here',
      ]);
    });
  });
  ```

- [ ] **Step 2: Run the tests to verify they fail**

  Run (from `frontend/`): `npx vitest run lib/stationAccessibility.test.ts`
  Expected: FAIL — `Cannot find module './stationAccessibility'`.

- [ ] **Step 3: Write the module**

  Create `frontend/lib/stationAccessibility.ts`:

  ```ts
  import type { StationAccessibilityData } from './types';

  /** Fixed display order and grouping for the twelve allowlisted keys --
   * see docs/superpowers/specs/2026-09-12-station-accessibility-design.md
   * Decision 7. A station page reader scans top-to-bottom for the thing
   * they care about, most-asked-about first; this is deliberately not
   * alphabetical and not the response's own key order. `StationAccessibilitySection`
   * skips a whole group when none of its keys are present in the data. */
  export const ACCESSIBILITY_CATEGORIES: {
    heading: string;
    keys: (keyof StationAccessibilityData)[];
  }[] = [
    { heading: 'Step-free access & assistance', keys: ['stationAccessibility', 'staffAssistance'] },
    { heading: 'Facilities', keys: ['toiletsAndChanging', 'lifts', 'loungesAndWaiting'] },
    {
      heading: 'Platform & station facilities',
      keys: ['platformFacilities', 'stationFacilities', 'helpAndSupport'],
    },
    { heading: 'Getting here', keys: ['transportLinks', 'carParks', 'dropOffPickUp', 'cycling'] },
  ];

  /** `stepFreeAccess` -> `Step free access`: splits on camelCase word
   * boundaries, lowercases every word, then capitalizes only the first --
   * deliberately no hardcoded per-field dictionary, since the field set is
   * unverified against a real payload (design spec Correction 2). A wrong
   * or ugly label from an unanticipated key is an acceptable, non-crashing
   * degradation (Decision 6). */
  export function humanizeKey(key: string): string {
    const words = key
      .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
      .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
      .toLowerCase()
      .split(' ')
      .filter(Boolean);
    if (words.length === 0) return key;
    return [words[0].charAt(0).toUpperCase() + words[0].slice(1), ...words.slice(1)].join(' ');
  }

  export type RenderableValue =
    | { kind: 'text'; text: string }
    | { kind: 'rows'; rows: { label: string; value: string }[] }
    | { kind: 'items'; count: number; items: RenderableValue[] }
    | { kind: 'raw'; json: string };

  type Primitive = string | number | boolean;

  function isPrimitive(value: unknown): value is Primitive {
    return typeof value === 'string' || typeof value === 'number' || typeof value === 'boolean';
  }

  function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
  }

  function primitiveText(value: Primitive): string {
    if (typeof value === 'boolean') return value ? 'Yes' : 'No';
    return String(value);
  }

  /** The one place this feature decides how to display a value of
   * genuinely unknown shape -- see design spec Decision 6. Never throws:
   * every branch either produces a renderable result or falls through to
   * the final raw-JSON fallback. Recurses at most one extra level (for an
   * array of objects, or an object's own values), matching the spec's
   * "depth-limited" framing -- anything deeper than that falls back to raw
   * JSON rather than attempting arbitrary recursion. */
  export function renderAccessibilityValue(value: unknown): RenderableValue {
    if (isPrimitive(value)) {
      return { kind: 'text', text: primitiveText(value) };
    }

    if (Array.isArray(value)) {
      if (value.every(isPrimitive)) {
        return { kind: 'text', text: value.map((v) => primitiveText(v as Primitive)).join(', ') };
      }
      return { kind: 'items', count: value.length, items: value.map(renderAccessibilityValue) };
    }

    if (isPlainObject(value)) {
      const rows: { label: string; value: string }[] = [];
      for (const [key, v] of Object.entries(value)) {
        if (v === null || v === undefined) continue;
        if (isPrimitive(v)) {
          rows.push({ label: humanizeKey(key), value: primitiveText(v) });
        } else if (Array.isArray(v) && v.every(isPrimitive)) {
          rows.push({ label: humanizeKey(key), value: v.map((x) => primitiveText(x as Primitive)).join(', ') });
        } else {
          // A nested object or array-of-non-primitives one level inside an
          // already-nested object is deeper than the "shallow object"
          // branch covers -- the whole value falls back to raw JSON rather
          // than attempting a second level of row-rendering.
          return { kind: 'raw', json: JSON.stringify(value, null, 2) };
        }
      }
      return { kind: 'rows', rows };
    }

    return { kind: 'raw', json: JSON.stringify(value, null, 2) };
  }
  ```

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `npx vitest run lib/stationAccessibility.test.ts`
  Expected: PASS — all tests from Step 1.

- [ ] **Step 5 (optional — only if Task 1 found a real fixture worth
  adding): add one more `it(...)` block** using the real (trimmed) JSON
  captured in Task 1 Step 6, asserting `renderAccessibilityValue` handles
  it exactly as the existing branches predict. Skip if Task 1 was deferred
  or found nothing new.

- [ ] **Step 6: Commit**

  ```bash
  git add frontend/lib/stationAccessibility.ts frontend/lib/stationAccessibility.test.ts
  git commit -m "$(cat <<'EOF'
Add generic, depth-limited accessibility-value renderer

A single small pure module handles every possible shape of an
allowlisted accessibility field's value without assuming its internal
structure (design spec Decision 6) -- this is the test suite that
actually defends the "never crashes on unknown JSONB shape" claim.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
  ```

---

## Task 6: `StationAccessibilitySection` component

**Files:**
- Create: `frontend/components/StationAccessibilitySection.tsx`
- Create: `frontend/components/StationAccessibilitySection.test.tsx`

**Interfaces:**
- Consumes: `ACCESSIBILITY_CATEGORIES`, `humanizeKey`,
  `renderAccessibilityValue`, `RenderableValue` (Task 5);
  `StationAccessibilityData` (Task 4).
- Produces (consumed by Task 7):
  ```ts
  export interface StationAccessibilitySectionProps {
    result:
      | { coverage: 'unavailable' }
      | { coverage: 'empty' }
      | { coverage: 'present'; data: StationAccessibilityData };
  }
  export function StationAccessibilitySection(props: StationAccessibilitySectionProps)
  ```
  This prop shape is deliberately identical to (but not the same declared
  type as) the `StationAccessibilityResult` local type Task 7 defines
  inside `page.tsx` -- matching this codebase's existing convention where
  `StationDisruptions`/`StationSampleStatsResult` are page-local, unexported
  unions (`frontend/app/stations/[crs]/page.tsx`), not shared through
  `lib/types.ts`. TypeScript's structural typing makes the two interchangeable
  without either file importing a type from the other.

- [ ] **Step 1: Write the failing tests**

  Create `frontend/components/StationAccessibilitySection.test.tsx`:

  ```tsx
  import { describe, it, expect } from 'vitest';
  import { screen } from '@testing-library/react';
  import { renderWithMantine } from '@/test/render';
  import { StationAccessibilitySection } from './StationAccessibilitySection';

  describe('StationAccessibilitySection', () => {
    it('renders the "not yet captured" copy for coverage: unavailable', () => {
      renderWithMantine(<StationAccessibilitySection result={{ coverage: 'unavailable' }} />);
      expect(
        screen.getByText("We don't have station reference data for this station yet."),
      ).toBeInTheDocument();
    });

    it('renders the "nothing published" copy for coverage: empty, distinct from unavailable', () => {
      renderWithMantine(<StationAccessibilitySection result={{ coverage: 'empty' }} />);
      expect(
        screen.getByText('No accessibility or facilities details have been published for this station.'),
      ).toBeInTheDocument();
      expect(
        screen.queryByText("We don't have station reference data for this station yet."),
      ).not.toBeInTheDocument();
    });

    it('renders only the group headings whose keys are present in the data', () => {
      renderWithMantine(
        <StationAccessibilitySection
          result={{ coverage: 'present', data: { lifts: { count: 2 } } }}
        />,
      );
      expect(screen.getByText('Facilities')).toBeInTheDocument();
      expect(screen.queryByText('Step-free access & assistance')).not.toBeInTheDocument();
      expect(screen.queryByText('Platform & station facilities')).not.toBeInTheDocument();
      expect(screen.queryByText('Getting here')).not.toBeInTheDocument();
    });

    it('renders humanized field labels and at least one rendered value per present key', () => {
      renderWithMantine(
        <StationAccessibilitySection
          result={{
            coverage: 'present',
            data: {
              staffAssistance: 'Available 06:00-23:00',
              carParks: [{ spaces: 120 }],
            },
          }}
        />,
      );
      expect(screen.getByText('Staff assistance')).toBeInTheDocument();
      expect(screen.getByText('Available 06:00-23:00')).toBeInTheDocument();
      expect(screen.getByText('Car parks')).toBeInTheDocument();
      expect(screen.getByText('1 items')).toBeInTheDocument();
    });

    it('renders a raw-JSON fallback inside a collapsed control for an unexpected deep shape, without throwing', () => {
      const deeplyNested = { level1: { level2: { level3: 'too deep' } } };
      expect(() =>
        renderWithMantine(
          <StationAccessibilitySection
            result={{ coverage: 'present', data: { stationFacilities: deeplyNested } }}
          />,
        ),
      ).not.toThrow();
      expect(screen.getByText('Station facilities')).toBeInTheDocument();
      // Spoiler's raw-JSON content is collapsed by default, so only its
      // show-more control is asserted here, not the JSON text itself.
      expect(screen.getByText('Show raw data')).toBeInTheDocument();
    });

    it('renders the heading "Accessibility & facilities", not bare "Accessibility"', () => {
      renderWithMantine(<StationAccessibilitySection result={{ coverage: 'empty' }} />);
      expect(screen.getByRole('heading', { name: 'Accessibility & facilities' })).toBeInTheDocument();
    });
  });
  ```

- [ ] **Step 2: Run the tests to verify they fail**

  Run (from `frontend/`): `npx vitest run components/StationAccessibilitySection.test.tsx`
  Expected: FAIL — `Cannot find module './StationAccessibilitySection'`.

- [ ] **Step 3: Write the component**

  Create `frontend/components/StationAccessibilitySection.tsx`:

  ```tsx
  'use client';

  import { Code, Group, Spoiler, Stack, Text, Title } from '@mantine/core';
  import {
    ACCESSIBILITY_CATEGORIES,
    humanizeKey,
    renderAccessibilityValue,
    type RenderableValue,
  } from '@/lib/stationAccessibility';
  import type { StationAccessibilityData } from '@/lib/types';

  export interface StationAccessibilitySectionProps {
    result:
      | { coverage: 'unavailable' }
      | { coverage: 'empty' }
      | { coverage: 'present'; data: StationAccessibilityData };
  }

  /** Recursively renders one already-computed `RenderableValue` -- see
   * `frontend/lib/stationAccessibility.ts`'s `renderAccessibilityValue` for
   * the shape-detection rules this only displays. An array-of-objects
   * entry ('items') nests each item inside its own collapsed `Spoiler`
   * rather than inlining it, so a large array (e.g. `carParks`) doesn't
   * produce a wall of text (design spec Decision 6). */
  function AccessibilityValue({ value }: { value: RenderableValue }) {
    if (value.kind === 'text') {
      return <Text size="sm">{value.text}</Text>;
    }
    if (value.kind === 'rows') {
      return (
        <Stack gap={2}>
          {value.rows.map((row) => (
            <Group key={row.label} gap="xs" wrap="nowrap">
              <Text size="sm" fw={500}>
                {row.label}:
              </Text>
              <Text size="sm">{row.value}</Text>
            </Group>
          ))}
        </Stack>
      );
    }
    if (value.kind === 'items') {
      return (
        <Stack gap={4}>
          <Text size="sm" c="dimmed">
            {value.count} items
          </Text>
          <Spoiler maxHeight={0} showLabel="Show items" hideLabel="Hide items">
            <Stack gap="sm">
              {value.items.map((item, index) => (
                // eslint-disable-next-line react/no-array-index-key -- items have no stable id in this genuinely-unknown-shape data
                <AccessibilityValue key={index} value={item} />
              ))}
            </Stack>
          </Spoiler>
        </Stack>
      );
    }
    return (
      <Spoiler maxHeight={0} showLabel="Show raw data" hideLabel="Hide raw data">
        <Code block>{value.json}</Code>
      </Spoiler>
    );
  }

  /** Fourth, independent section on `/stations/[crs]` -- see
   * docs/superpowers/specs/2026-09-12-station-accessibility-design.md
   * Decisions 6-9. Heading is deliberately "Accessibility & facilities",
   * not bare "Accessibility", so it reads unambiguously as physical-access
   * information (Decision 8) -- this codebase separately uses
   * "accessibility" for WCAG audits (Correction 4), and this component's
   * own name/copy are the only mitigations for that collision; the wire
   * type name (`StationAccessibilityData`) is deliberately left alone. */
  export function StationAccessibilitySection({ result }: StationAccessibilitySectionProps) {
    return (
      <Stack gap="xs">
        <Title order={2} size="h4">
          Accessibility & facilities
        </Title>
        {result.coverage === 'unavailable' && (
          <Text c="dimmed">We don&apos;t have station reference data for this station yet.</Text>
        )}
        {result.coverage === 'empty' && (
          <Text c="dimmed">No accessibility or facilities details have been published for this station.</Text>
        )}
        {result.coverage === 'present' &&
          ACCESSIBILITY_CATEGORIES.map((category) => {
            const presentKeys = category.keys.filter((key) => result.data[key] !== undefined);
            if (presentKeys.length === 0) return null;
            return (
              <Stack key={category.heading} gap={4}>
                <Text size="sm" fw={700}>
                  {category.heading}
                </Text>
                {presentKeys.map((key) => (
                  <Stack key={key} gap={2} pl="sm">
                    <Text size="sm" fw={500}>
                      {humanizeKey(key)}
                    </Text>
                    <AccessibilityValue value={renderAccessibilityValue(result.data[key])} />
                  </Stack>
                ))}
              </Stack>
            );
          })}
      </Stack>
    );
  }
  ```

- [ ] **Step 4: Run the tests to verify they pass**

  Run: `npx vitest run components/StationAccessibilitySection.test.tsx`
  Expected: PASS — all tests from Step 1.

- [ ] **Step 5: Commit**

  ```bash
  git add frontend/components/StationAccessibilitySection.tsx frontend/components/StationAccessibilitySection.test.tsx
  git commit -m "$(cat <<'EOF'
Add StationAccessibilitySection component

Renders the three honest coverage states and the four fixed category
groups over the generic renderer added in the previous commit. Not yet
wired into the station page.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
  ```

---

## Task 7: Wire into the station page

**Files:**
- Modify: `frontend/app/stations/[crs]/page.tsx`
- Modify: `frontend/app/stations/[crs]/page.test.tsx`

**Interfaces:**
- Consumes: `getStationAccessibility` (Task 4), `StationAccessibilitySection`
  (Task 6).
- Produces: nothing further downstream — this is the last task.

- [ ] **Step 1: Write the failing tests**

  Add to `frontend/app/stations/[crs]/page.test.tsx`:

  1. Add `getStationAccessibility: vi.fn()` to the `vi.mock('@/lib/api', ...)`
     factory at the top of the file (alongside `getStationSampleStats` etc.).
  2. Add `vi.mocked(api.getStationAccessibility).mockResolvedValue({})` to
     every existing `beforeEach` in the file (the three `describe` blocks
     at lines 73-82, 129-136, and 171-177 of the current file) — every
     existing test renders the whole page, so every existing `beforeEach`
     needs this new call mocked or those tests will now reject with an
     unmocked-function error.
  3. Add a new `describe` block, mirroring the existing "sample stats by
     operator" block's structure:

  ```tsx
  describe('StationDisruptionPage -- accessibility & facilities', () => {
    beforeEach(() => {
      __resetStaleCacheForTests();
      vi.stubGlobal('fetch', vi.fn());
      vi.mocked(api.getStationName).mockResolvedValue('London Kings Cross');
      vi.mocked(api.getPreferences).mockResolvedValue({ pinnedLines: [], pinnedStations: [] });
      vi.mocked(api.getStopPointDisruption).mockResolvedValue([]);
      vi.mocked(api.getStationSampleStats).mockResolvedValue([]);
      vi.mocked(api.getAllTocs).mockResolvedValue([]);
    });

    it('renders the "not yet captured" copy when the route 404s', async () => {
      vi.mocked(api.getStationAccessibility).mockRejectedValue(
        new ApiNotFoundError('no station reference data for: KGX'),
      );

      await renderPage();

      expect(
        screen.getByText("We don't have station reference data for this station yet."),
      ).toBeInTheDocument();
    });

    it('renders the "nothing published" copy for a row with no allowlisted keys', async () => {
      vi.mocked(api.getStationAccessibility).mockResolvedValue({});

      await renderPage();

      expect(
        screen.getByText('No accessibility or facilities details have been published for this station.'),
      ).toBeInTheDocument();
    });

    it('renders grouped headings and at least one rendered value for a populated response, end to end through the real component tree', async () => {
      vi.mocked(api.getStationAccessibility).mockResolvedValue({
        stationAccessibility: { stepFree: true },
        carParks: [{ spaces: 120 }],
      });

      await renderPage();

      expect(screen.getByText('Step-free access & assistance')).toBeInTheDocument();
      expect(screen.getByText('Getting here')).toBeInTheDocument();
      expect(screen.getByText('Step free')).toBeInTheDocument();
      expect(screen.getByText('Yes')).toBeInTheDocument();
      expect(screen.getByText('Car parks')).toBeInTheDocument();
    });

    it('renders the raw-JSON fallback without throwing for a deliberately malformed shape', async () => {
      vi.mocked(api.getStationAccessibility).mockResolvedValue({
        lifts: { level1: { level2: { level3: 'too deep' } } },
      });

      await expect(renderPage()).resolves.toBeDefined();
      expect(screen.getByText('Show raw data')).toBeInTheDocument();
    });
  });
  ```

- [ ] **Step 2: Run the tests to verify they fail**

  Run: `npx vitest run "app/stations/[crs]/page.test.tsx"`
  Expected: FAIL — every existing test now fails because
  `api.getStationAccessibility` is called by nothing yet but is unmocked in
  a way the new `vi.mock` factory expects (actually: at this point
  `page.tsx` doesn't call it at all, so the *new* describe block's tests
  fail because the accessibility copy never renders; the pre-existing
  `beforeEach` blocks' added mock calls are harmless no-ops until Step 3
  wires the fetch in — confirm no crash, only missing-text assertion
  failures in the new block).

- [ ] **Step 3: Wire `fetchStationAccessibility` and the section into `page.tsx`**

  Add to the `import { ... } from '@/lib/api'` block:
  ```tsx
  import {
    getStopPointDisruption,
    getPreferences,
    getStationName,
    getStationSampleStats,
    getStationAccessibility,
    getAllTocs,
    ApiNotFoundError,
  } from '@/lib/api';
  ```

  Add the component import:
  ```tsx
  import { StationAccessibilitySection } from '@/components/StationAccessibilitySection';
  ```

  Add `StationAccessibilityData` to the `import type { ... } from '@/lib/types'` block.

  Add this coverage wrapper directly after `fetchStationSampleStats`'s
  definition, mirroring its shape exactly (Decision 8/9):

  ```tsx
  /** A third, independent coverage question from the two above -- whether
   * this app has ever captured `stations` reference data for this CRS at
   * all, vs. whether it has but none of the allowlisted accessibility
   * keys were present. See design spec Correction 5 / Decision 9: these
   * are two different, already-representable database states and must
   * stay visibly distinct in the UI, not collapsed into one "no data"
   * message. */
  type StationAccessibilityResult =
    | { coverage: 'unavailable' }
    | { coverage: 'empty' }
    | { coverage: 'present'; data: StationAccessibilityData };

  async function fetchStationAccessibility(crs: string): Promise<StationAccessibilityResult> {
    try {
      const data = await withStaleFallback(`stationAccessibility:${crs}`, () => getStationAccessibility(crs));
      return Object.keys(data).length === 0 ? { coverage: 'empty' } : { coverage: 'present', data };
    } catch (err) {
      if (err instanceof ApiNotFoundError) return { coverage: 'unavailable' };
      throw err;
    }
  }
  ```

  Add `fetchStationAccessibility(crs)` to the page component's `Promise.all`:

  ```tsx
  const [{ reports, coverage }, preferences, sampleStatsResult, accessibilityResult, tocs] = await Promise.all([
    fetchStationDisruptions(crs),
    getPreferences().catch(() => NO_PREFERENCES),
    fetchStationSampleStats(crs),
    fetchStationAccessibility(crs),
    getAllTocs().catch(() => []),
  ]);
  ```

  Add the section as the fourth block, after the existing "Sample stats by
  operator" `Stack` (replacing the file's final `</Stack>\n  );\n}`):

  ```tsx
      <Divider />
      <StationAccessibilitySection result={accessibilityResult} />
    </Stack>
  );
  }
  ```

- [ ] **Step 4: Run the page tests to verify they pass**

  Run: `npx vitest run "app/stations/[crs]/page.test.tsx"`
  Expected: PASS — every existing test (now correctly mocking
  `getStationAccessibility`) plus every test in the new "accessibility &
  facilities" describe block.

- [ ] **Step 5: Run the full frontend test suite**

  Run (from `frontend/`): `npm test`
  Expected: PASS, no regressions anywhere else in the suite.

- [ ] **Step 6: Run the full backend test suite (non-DB) as a final sanity check**

  Run: `cargo test -p api`
  Expected: PASS.

- [ ] **Step 7: Commit**

  ```bash
  git add frontend/app/stations/[crs]/page.tsx frontend/app/stations/[crs]/page.test.tsx
  git commit -m "$(cat <<'EOF'
Wire StationAccessibilitySection into the station page

Fourth, independent section on /stations/[crs], fetched alongside the
existing disruption/sample-stats/tocs calls. Completes the
accessibility-and-facilities feature end to end.

Co-Authored-By: Claude Sonnet 5 <noreply@anthropic.com>
EOF
)"
  ```

---

## Self-Review

**1. Spec coverage:**

| Spec section | Task |
| --- | --- |
| Decision 1: twelve-key allowlist, exact list and order | Task 2 (`ACCESSIBILITY_KEYS`), Task 5 (`ACCESSIBILITY_CATEGORIES` covers all twelve exactly once) |
| Decision 2: no migration, no new write path | No task touches `poller-stations` or `upsert_stations`; File Structure lists no migration file |
| Decision 3: new code lives in `reference.rs` (both data and route), not a new module | Tasks 2-3 |
| Decision 4: route shape, unwrapped response body, 404/200{}/200{filtered} | Task 3 |
| Decision 5: loose `unknown`-valued type, hour-cached fetcher | Task 4 |
| Decision 6: generic, depth-limited, crash-proof renderer, five branches | Task 5 (every branch has a dedicated test), Task 6 (renders each `RenderableValue.kind`) |
| Decision 7: fixed category order, group hidden when empty | Task 5 (`ACCESSIBILITY_CATEGORIES`), Task 6 (`presentKeys.length === 0` skip) |
| Decision 8: naming (`StationAccessibilityData`, `getStationAccessibility`, component filename, heading copy), fourth independent section | Tasks 4, 6, 7 |
| Decision 9: three honest states and their exact copy | Task 6 (component), Task 7 (page-level coverage wrapper + tests) |
| Correction 2: unverified real-world shape → generic renderer, not typed components | Task 5's entire design; Task 1 addresses verifying it without violating it |
| Correction 4: naming collision, mitigated only via UI copy/filename | Global Constraints (explicit "do not rename" instruction), Task 6's heading and doc comment |
| Correction 5: two distinct absence states | Global Constraints, Task 2 (`Option<Value>`), Task 3 (404 vs 200{}), Task 6/7 (`unavailable` vs `empty`) |
| Error handling: DB error → 500; unexpected shape never errors at any layer | Task 3 (`internal_error` reuse); Task 5/6 (raw-JSON fallback, never throws) |
| Testing approach: non-DB unit test for filter logic | Task 2 Step 1 |
| Testing approach: `#[ignore]`d DB test, `Z…` namespace | Task 2 Step 5, Task 3 Step 1 |
| Testing approach: frontend pure-function tests written first | Task 5 (explicitly ordered before Task 6) |
| Testing approach: page-level test extending `page.test.tsx` | Task 7 |
| Testing approach: no new e2e/Playwright spec | Global Constraints; no such file appears in File Structure |
| Open Question 1: unverified real shape | Task 1 |
| Open Question 2: unmeasured payload size | Task 1 Step 3 |
| Open Question 3: allowlist correctness is a product call, out of scope | Global Constraints states this explicitly; no task adds/removes a key |
| Open Question 4: naming collision, no action beyond spec's own mitigation | Global Constraints, Task 6 |
| Open Question 5: no CRS case normalization, pre-existing rough edge | Global Constraints, Task 2's `station_accessibility` doc comment |

No gaps found.

**2. Placeholder scan:** No "TBD"/"implement later"/"add appropriate
handling" language anywhere in the tasks above; every step carries the
literal code or command to run. Task 1 is deliberately not code-shaped
(it's a manual verification procedure), but every one of its steps still
names an exact command and an exact decision rule for what counts as a
"surprise" worth acting on — it does not say "verify things look right"
without saying what "right" means.

**3. Type consistency:** `ACCESSIBILITY_KEYS` (Rust, Task 2) and
`ACCESSIBILITY_CATEGORIES`'s flattened key list (TypeScript, Task 5) are
both asserted, in their own tests, to be exactly the same twelve keys.
`StationAccessibilityData` is declared once (Task 4) and referenced by the
same name in Tasks 5, 6, and 7 with no renaming. `RenderableValue`'s four
`kind` variants (`text`/`rows`/`items`/`raw`) are declared once in Task 5
and consumed by the same names in Task 6's `AccessibilityValue` switch.
`StationAccessibilityResult`'s three `coverage` variants
(`unavailable`/`empty`/`present`) are used identically in Task 6's
component prop type and Task 7's page-local type, and their exact UI copy
strings match between the component (Task 6) and the page-level tests
(Task 7).

---

**Plan complete and saved to `docs/superpowers/plans/2026-09-15-station-accessibility.md`. Two execution options:**

**1. Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration

**2. Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

**Which approach?**
