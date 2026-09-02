# Ticket Display Enhancements & Delete Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface a ticket's already-stored `source`/`createdAt` fields on
`TicketSummary.tsx` (both attached and standalone tickets, no backend
change), and let a user delete a single ticket via a new
`DELETE /Train/tickets/{ticketId}` route and a `DeleteTicketButton`
component wired into every place a ticket renders today.

**Architecture:**

```
crates/api/src/data/train_tracking.rs   + delete_ticket(pool, ticket_id, user_id) -> bool
                                           (mirrors delete_tracked_train exactly)
        │                                (Task 1)
        ▼
crates/api/src/routes/train.rs          + DELETE /Train/tickets/{ticket_id}
                                           router() gains one route; delete_ticket handler
                                           mirrors delete_tracked_train's handler exactly
                                         (Task 2)
        │
        │  same-origin proxy (frontend/app/api/[...path]/route.ts,
        │  unchanged -- already passes /Train/... straight through)
        ▼
frontend/components/TicketSummary.tsx   CHANGED -- widened Pick<...> to add
                                           source/createdAt; provenance Badge +
                                           "Added <date>" line (no backend dep)
                                         (Task 3, independent of Tasks 1-2/4)

frontend/components/DeleteTicketButton.tsx   NEW -- modeled on DeleteTrainButton.tsx,
                                                DELETE /api/Train/tickets/{id},
                                                router.refresh() on success (not push)
                                              (Task 4, independent of Tasks 1-3)
        │
        ▼
frontend/components/TicketPanel.tsx          + <DeleteTicketButton>
frontend/app/track/mine/page.tsx             + <DeleteTicketButton> in both
                                                TrackedTrainListRow (attached)
                                                and UnattachedTicketRow (standalone)
                                              (Task 5, depends on Tasks 2 & 4;
                                               benefits from Task 3 having landed)
```

**Tech Stack:** Rust (`crates/api` -- `axum`, `sqlx`, existing `AuthenticatedUser`
extractor; no new crate); Next.js App Router + TypeScript + Mantine v9
(`Modal`/`useDisclosure`/`Badge`, all already in use elsewhere; no new npm
package); the existing same-origin `/api/*` proxy
(`frontend/app/api/[...path]/route.ts`) already passes `/Train/...` requests
straight through with no `/public/` prefix -- confirmed unchanged and
sufficient for the new route (`route.ts:40`, `:56`).

**Spec:** `docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`
-- read in full before starting; this plan carries its Decisions 1-3 into
concrete tasks. Every citation below was independently re-verified against
this worktree's current source (fast-forwarded to `main` at commit
`b12f679` to pick up this spec, which had not yet reached this worktree's
branch), not trusted blind from the spec.

## Global Constraints

- **No new database migration, anywhere.** `tracked_train_tickets.source`
  and `.created_at` are already `NOT NULL` columns
  (`crates/api/migrations/20260829090000_journey_ticket_tracking.sql:46-49`),
  already serialized on both `TrackedTrainTicket`
  (`crates/api/src/data/train_tracking.rs:561-572`) and `TicketListItem`
  (`train_tracking.rs:677-699`), and already populated correctly for a
  standalone (unattached) ticket -- Task 3 is rendering-only. `delete_ticket`
  needs no schema change either: ownership is already a direct, indexed
  column (`tracked_train_tickets_user_id`,
  `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:56`),
  confirmed by `get_ticket_owned`'s own existing `WHERE id = $1 AND
  user_id = $2` (`train_tracking.rs:602`, no join). Do not create a
  migration file in any task.
- **No new dependency, in either ecosystem.** Every change in this plan
  reuses existing Rust crates (`axum`, `sqlx`, `anyhow`, `serde`) and
  existing frontend packages (`@mantine/core`, `@mantine/hooks`,
  `next/navigation`) -- no new Cargo crate, no new npm package.
- **Never `403`, anywhere in this plan.** The new route's ownership check
  answers only "deleted" (`true`) or "not deleted" (`false`, mapped to a
  bare `404`) -- "doesn't exist" and "exists but belongs to someone else"
  are indistinguishable at the data layer and stay that way at the HTTP
  layer, matching `delete_tracked_train`
  (`crates/api/src/routes/train.rs:325-337`) and `delete_line`
  (`crates/api/src/routes/lines.rs:354`, cited by `delete_tracked_train`'s
  own doc comment) exactly.
- **The new route is flat: `/Train/tickets/{ticket_id}`, never nested
  under a `{tracking_id}`.** A ticket can have `tracked_train_id: NULL`
  (a STANDALONE ticket,
  `crates/api/migrations/20260901140000_standalone_tickets.sql:31`) --
  a route shape requiring a `tracking_id` in its path cannot express
  deleting one, the same reasoning `post_attach_ticket`
  (`routes/train.rs:144-154`) already used for its own flat
  `/Train/tickets/{ticket_id}/attach` shape. **The new route's path
  parameter must be named `{ticket_id}`**, exactly matching the existing
  sibling `/Train/tickets/{ticket_id}/attach` (`routes/train.rs:47`) --
  matchit/axum panics at router-construction time if two routes at the
  same trie position use different parameter names for what looks like
  the same segment; reusing the existing name avoids that, and
  `router_builds_without_panicking` (`routes/train.rs:559-562`) is the
  regression check that would catch it if this were gotten wrong.
- **`TicketSummary`'s prop type stays a narrow `Pick<...>`, never the full
  wire type.** Widen the field list (add `'source' | 'createdAt'`), don't
  replace `Pick<TrackedTrainTicket | TicketListItem, ...>` with the whole
  union type -- per the component's own existing doc comment
  (`frontend/components/TicketSummary.tsx:9-11`) and the spec's Decision 1
  reasoning (keeps the component honest about exactly which fields it
  reads, from either wire shape).
- **`DeleteTicketButton` must call `router.refresh()`, never
  `router.push(...)`.** This is the one deliberate divergence from
  `DeleteTrainButton.tsx` this plan makes, reasoned through in the spec's
  Decision 3: deleting a ticket never removes the entire subject of the
  page it's rendered on (unlike deleting a tracked train), so the caller
  should stay put and let the enclosing Server Component's next fetch drop
  the row -- the exact mechanism `AttachTicketAction.tsx:41` and
  `PinToggle.tsx:99` already use for the same shape.
- **Feature #3 ("open the original PDF" / render a QR code from a
  `.pkpass`) is out of scope for every task in this plan.** Do not add any
  route, column, migration, or frontend affordance toward it in any task,
  not even as a stub or a "future work" placeholder beyond the one-line
  pointer in "Not in this plan," below.
- **Testing convention.** Rust: `#[cfg(test)]` modules colocated in the
  same file, run via `cargo test -p api`. Every new test that touches a
  real `PgPool` is `#[ignore = "requires a live database; ..."]`, matching
  every existing test in `routes/train.rs`'s `db_tests` module
  (e.g. `routes/train.rs:1154-1157`) and `data/custom_lines.rs`'s
  `db_tests` module (`custom_lines.rs:309-313`) -- run with `DATABASE_URL`
  set and `cargo test -p api <name> -- --ignored --test-threads=1`.
  Frontend: colocated `*.test.tsx`, `@testing-library/react`,
  `renderWithMantine` (`frontend/test/render.tsx`), Vitest (`npx vitest
  run` or `npm test`, from `frontend/`).
- **Parallelizable tasks:** Task 1 (backend data layer) and Task 3
  (frontend `TicketSummary`) are fully independent -- different
  ecosystems, no shared files -- and can be dispatched in parallel. Task 4
  (new `DeleteTicketButton` component) is also independent of Tasks 1-3 and
  can run in parallel with either. Task 2 depends on Task 1 (needs
  `train_tracking::delete_ticket` to exist). Task 5 depends on Task 2 (the
  real route) and Task 4 (the component) landing first; it also touches
  the same two files Task 3 touches (`TicketPanel.tsx`,
  `app/track/mine/page.tsx`'s ticket rows), so land Task 3 before Task 5 to
  avoid a merge conflict, even though there's no functional dependency
  between them. Task 6 (verification) runs last, after every other task.

---

### Task 1: Backend data layer -- `train_tracking::delete_ticket`

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs`

**Interfaces:**
- Produces: `pub async fn delete_ticket(pool: &PgPool, ticket_id: i64, user_id: &str) -> anyhow::Result<bool>`.
- Consumed by: Task 2's route handler (`crates/api/src/routes/train.rs`).
- **Depends on:** nothing -- this is the foundational task.

- [ ] **Step 1: Add `delete_ticket` to `crates/api/src/data/train_tracking.rs`**

Place it directly after `get_ticket_owned` (currently `train_tracking.rs:601-608`),
before the `MINE_TICKETS_LIMIT` constant (currently line 621) -- grouping it
with the other single-ticket operations (`create_ticket`,
`attach_ticket_to_tracked_train`, `list_tickets_for_tracked_train`,
`get_ticket_owned`) rather than the "list all tickets for user" section
below it:

```rust
/// Deletes a ticket by id, scoped to the caller's ownership -- mirrors
/// `delete_tracked_train`'s own `WHERE id = $1 AND user_id = $2` shape
/// exactly (`crates/api/src/data/train_tracking.rs:413-420`). No join
/// needed, per `get_ticket_owned`'s own established precedent just above:
/// `tracked_train_tickets.user_id` is a direct, indexed column
/// (`tracked_train_tickets_user_id`,
/// `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:56`),
/// not transitive through the owning tracked train. Applies identically
/// whether the ticket is attached (`tracked_train_id: Some(_)`) or
/// standalone (`tracked_train_id: None`, per
/// `crates/api/migrations/20260901140000_standalone_tickets.sql`) -- the
/// `WHERE` clause never references that column, so there is nothing to
/// special-case. Nothing else needs deleting as a consequence: unlike a
/// tracked train, a ticket is a leaf in the FK graph -- nothing
/// `REFERENCES tracked_train_tickets` anywhere in this schema. Returns
/// `true` if a row was deleted, `false` if no ticket with that id belongs
/// to this caller (doesn't exist, or belongs to someone else --
/// indistinguishable at this layer, same as every other ownership check
/// in this file; the route handler maps `false` to `404`, never `403`).
pub async fn delete_ticket(pool: &PgPool, ticket_id: i64, user_id: &str) -> anyhow::Result<bool> {
    let result = sqlx::query("DELETE FROM tracked_train_tickets WHERE id = $1 AND user_id = $2")
        .bind(ticket_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}
```

- [ ] **Step 2: Add a `db_tests` module with the four-outcome coverage**

`train_tracking.rs` has no existing `db_tests` module of its own (unlike
`data/custom_lines.rs`) -- add one at the end of the file, after the
existing `ticket_list_tests` module, following `custom_lines.rs`'s
`db_tests` pattern (`custom_lines.rs:305-402`: each test connects to
`PgPool` itself via a local helper, no shared fixture module imported
across files):

```rust
#[cfg(test)]
mod db_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn connect() -> PgPool {
        let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new().connect(&database_url).await.expect("connect to postgres")
    }

    async fn seed_user(pool: &PgPool, user_id: &str) {
        sqlx::query("INSERT INTO users (id, email, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING")
            .bind(user_id)
            .bind(format!("{user_id}@example.com"))
            .bind(user_id)
            .execute(pool)
            .await
            .expect("seed fixture user");
    }

    async fn cleanup_user(pool: &PgPool, user_id: &str) {
        sqlx::query("DELETE FROM tracked_train_tickets WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tickets");
        sqlx::query("DELETE FROM tracked_trains WHERE user_id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture tracked_trains");
        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user_id)
            .execute(pool)
            .await
            .expect("cleanup fixture user");
    }

    /// Minimal fixture row -- only the `NOT NULL` columns
    /// (`crates/api/migrations/20260828120000_train_tracking.sql:40-76`).
    async fn seed_tracked_train(pool: &PgPool, user_id: &str) -> i64 {
        let (id,): (i64,) = sqlx::query_as(
            "INSERT INTO tracked_trains (user_id, service_date, pin_origin_crs, pin_scheduled_departure) \
             VALUES ($1, $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind("2026-09-02".parse::<chrono::NaiveDate>().unwrap())
        .bind("KGX")
        .bind("2026-09-02T09:00:00Z".parse::<DateTime<Utc>>().unwrap())
        .fetch_one(pool)
        .await
        .expect("insert fixture tracked_trains row");
        id
    }

    fn fixture_entry() -> TicketEntryRequest {
        TicketEntryRequest {
            operator: Some("LNER".to_string()),
            ticket_type: Some("single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "manual".to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_the_owner_can_delete_their_own_attached_ticket() {
        let pool = connect().await;
        seed_user(&pool, "TEST-TICKET-DELETE-OWNER").await;
        let tracking_id = seed_tracked_train(&pool, "TEST-TICKET-DELETE-OWNER").await;
        let ticket_id = create_ticket(&pool, Some(tracking_id), &fixture_entry(), "TEST-TICKET-DELETE-OWNER")
            .await
            .expect("create fixture ticket");

        let deleted = delete_ticket(&pool, ticket_id, "TEST-TICKET-DELETE-OWNER").await.expect("delete ticket");
        assert!(deleted);

        let gone = get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-OWNER").await.expect("read ticket");
        assert!(gone.is_none(), "ticket row should be gone after the owner deletes it");

        cleanup_user(&pool, "TEST-TICKET-DELETE-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_non_owner_cannot_delete_it_and_the_row_survives() {
        let pool = connect().await;
        seed_user(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
        seed_user(&pool, "TEST-TICKET-DELETE-OTHER").await;
        let ticket_id = create_ticket(&pool, None, &fixture_entry(), "TEST-TICKET-DELETE-REAL-OWNER")
            .await
            .expect("create fixture ticket");

        let deleted = delete_ticket(&pool, ticket_id, "TEST-TICKET-DELETE-OTHER").await.expect("delete ticket");
        assert!(!deleted);

        let still_there = get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-REAL-OWNER").await.expect("read ticket");
        assert!(still_there.is_some(), "row should survive a non-owner's delete attempt");

        cleanup_user(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
        cleanup_user(&pool, "TEST-TICKET-DELETE-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_nonexistent_id_returns_false() {
        let pool = connect().await;
        let deleted = delete_ticket(&pool, 99999999, "TEST-TICKET-DELETE-NOBODY").await.expect("delete ticket");
        assert!(!deleted);
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_an_unattached_standalone_ticket_deletes_identically_to_an_attached_one() {
        let pool = connect().await;
        seed_user(&pool, "TEST-TICKET-DELETE-STANDALONE").await;
        // tracked_train_id: None -- a STANDALONE ticket. delete_ticket's
        // own WHERE clause never references this column, so this must
        // succeed identically to the attached case above.
        let ticket_id = create_ticket(&pool, None, &fixture_entry(), "TEST-TICKET-DELETE-STANDALONE")
            .await
            .expect("create fixture ticket");

        let deleted = delete_ticket(&pool, ticket_id, "TEST-TICKET-DELETE-STANDALONE").await.expect("delete ticket");
        assert!(deleted);

        let gone = get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-STANDALONE").await.expect("read ticket");
        assert!(gone.is_none());

        cleanup_user(&pool, "TEST-TICKET-DELETE-STANDALONE").await;
    }
}
```

`TicketEntryRequest` is already in scope via `use super::*;` (the file's
own top-level `use common::{TicketEntryRequest, TrackPinRequest, ...};`,
`train_tracking.rs:8`) -- no additional `use` needed for it. `DateTime`/
`Utc` are likewise already imported at the file's top level
(`train_tracking.rs:7`).

- [ ] **Step 3: Run the tests**

Run: `cargo build -p api` (compiles the new function and test module
without running the `#[ignore]`d tests). With a live `DATABASE_URL` set:
`cargo test -p api delete_ticket -- --ignored --test-threads=1`.
Expected: PASS (all four new tests).

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Add train_tracking::delete_ticket, mirroring delete_tracked_train"
```

---

### Task 2: Backend route -- `DELETE /Train/tickets/{ticketId}`

**Files:**
- Modify: `crates/api/src/routes/train.rs`

**Interfaces:**
- Consumes: `train_tracking::delete_ticket` (Task 1).
- Produces: `DELETE /Train/tickets/{ticket_id}` -- `401` (no session, via
  `AuthenticatedUser`), `404` with body `"no ticket with that id"` (unknown
  or not-yours), `204 No Content` on success.
- **Depends on:** Task 1.

- [ ] **Step 1: Add the `delete_ticket` route handler**

Place it directly after `post_attach_ticket` (currently `train.rs:155-187`),
before `get_tickets` (currently line 189) -- grouping it with the other
ticket-scoped write handlers:

```rust
/// `DELETE /Train/tickets/{ticketId}` -- mirrors `delete_tracked_train`
/// (below) exactly: same `AuthenticatedUser` + 404-for-unknown-or-not-yours
/// shape, same `204 No Content` on success. Ownership is folded directly
/// into `train_tracking::delete_ticket`'s own `WHERE id = $1 AND user_id =
/// $2` (no join, no separate ownership lookup first -- see that function's
/// doc comment). Deliberately flat (`/Train/tickets/{ticket_id}`, not
/// nested under a `{tracking_id}`), matching `post_attach_ticket`'s own
/// reasoning just above: a ticket may have no owning tracked train at all
/// (a STANDALONE ticket), so a route shape that requires a `tracking_id`
/// in its path cannot express deleting one. Applies uniformly regardless
/// of attachment status -- `tracked_train_tickets` has no child rows to
/// clean up either way (unlike `delete_tracked_train`, this is a leaf in
/// the FK graph).
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

- [ ] **Step 2: Register the route in `router()`**

In `router()` (currently `train.rs:26-62`), add the new route directly
after the existing `/Train/tickets/{ticket_id}/attach` line (currently
line 47), before `/Train/{tracking_id}` (currently line 48):

```rust
        .route("/Train/tickets/{ticket_id}/attach", axum::routing::post(post_attach_ticket))
        .route("/Train/tickets/{ticket_id}", axum::routing::delete(delete_ticket))
        .route("/Train/{tracking_id}", axum::routing::get(get_by_tracking_id).delete(delete_tracked_train))
```

Also extend the router's own doc comment (currently `train.rs:30-42`) with
one sentence naming the new route, matching that comment's existing style:
after the sentence ending "...is how a standalone ticket later gets a
`tracked_train_id`.", add: "`/Train/tickets/{ticket_id}` (`DELETE` only)
removes a ticket outright, regardless of attachment status -- see
`delete_ticket`'s own doc comment."

- [ ] **Step 3: Add route-level `db_tests` coverage**

In `routes/train.rs`'s existing `db_tests` module, add a new `seed_ticket`
helper and a `delete_ticket_request` helper (mirroring `delete_request`,
currently `train.rs:1135-1152`), then a `// --- delete_ticket ---` test
section, placed after the existing `// --- delete_tracked_train ---`
section (which currently ends at line 1232), before `// --- get_by_uid_and_date ---`
(currently line 1234):

```rust
    // --- delete_ticket (Decision 2 of
    // docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md) ---

    /// Seeds one `tracked_train_tickets` fixture row directly via
    /// `train_tracking::create_ticket` -- `tracking_id: None` creates a
    /// STANDALONE ticket, matching `create_ticket`'s own documented
    /// convention. Returns the new row's `id`.
    async fn seed_ticket(pool: &PgPool, user_id: &str, tracking_id: Option<i64>) -> i64 {
        let entry = common::TicketEntryRequest {
            operator: Some("LNER".to_string()),
            ticket_type: Some("Off-Peak Day Single".to_string()),
            origin_crs: Some("KGX".to_string()),
            destination_crs: Some("EDB".to_string()),
            source: "manual".to_string(),
        };
        crate::data::train_tracking::create_ticket(pool, tracking_id, &entry, user_id)
            .await
            .expect("insert fixture ticket")
    }

    /// Issues `DELETE /Train/tickets/{ticketId}`, optionally with a session
    /// cookie -- mirrors `delete_request` above, for the ticket-scoped
    /// sibling route.
    async fn delete_ticket_request(router: axum::Router, ticket_id: i64, raw_token: Option<&str>) -> (StatusCode, Value) {
        let mut builder = Request::builder().method("DELETE").uri(format!("/Train/tickets/{ticket_id}"));
        if let Some(token) = raw_token {
            builder = builder.header(header::COOKIE, format!("distant_signal_session={token}"));
        }
        let req = builder.body(Body::empty()).expect("build request");
        let response = router.oneshot(req).await.expect("oneshot request");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.expect("read body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|_| {
                Value::String(String::from_utf8(bytes.to_vec()).expect("body is valid utf8"))
            })
        };
        (status, value)
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_no_session_is_401() {
        let pool = connect().await;
        seed_session(&pool, "TEST-TICKET-DELETE-401-OWNER").await;
        let ticket_id = seed_ticket(&pool, "TEST-TICKET-DELETE-401-OWNER", None).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_ticket_request(router, ticket_id, None).await;

        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, Value::String("no session".to_string()));

        cleanup_user(&pool, "TEST-TICKET-DELETE-401-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_non_owner_session_gets_the_same_404_as_unknown_and_the_row_survives() {
        let pool = connect().await;
        seed_session(&pool, "TEST-TICKET-DELETE-OWNER").await;
        let other_token = seed_session(&pool, "TEST-TICKET-DELETE-OTHER").await;
        let ticket_id = seed_ticket(&pool, "TEST-TICKET-DELETE-OWNER", None).await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_ticket_request(router, ticket_id, Some(&other_token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no ticket with that id".to_string()));

        let still_there = crate::data::train_tracking::get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-OWNER")
            .await
            .expect("read ticket");
        assert!(still_there.is_some(), "row should survive a non-owner's delete attempt");

        cleanup_user(&pool, "TEST-TICKET-DELETE-OWNER").await;
        cleanup_user(&pool, "TEST-TICKET-DELETE-OTHER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_nonexistent_id_is_404_with_the_unchanged_message() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TICKET-DELETE-NOTFOUND").await;

        let router = test_router(test_app(pool.clone()));
        let (status, body) = delete_ticket_request(router, 99999999, Some(&token)).await;

        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no ticket with that id".to_string()));

        cleanup_user(&pool, "TEST-TICKET-DELETE-NOTFOUND").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_the_owner_can_delete_a_standalone_ticket() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
        // tracking_id: None -- confirms this route applies uniformly to a
        // STANDALONE ticket, not just an attached one.
        let ticket_id = seed_ticket(&pool, "TEST-TICKET-DELETE-REAL-OWNER", None).await;

        let router = test_router(test_app(pool.clone()));
        let (status, _body) = delete_ticket_request(router, ticket_id, Some(&token)).await;

        assert_eq!(status, StatusCode::NO_CONTENT);

        let gone = crate::data::train_tracking::get_ticket_owned(&pool, ticket_id, "TEST-TICKET-DELETE-REAL-OWNER")
            .await
            .expect("read ticket");
        assert!(gone.is_none(), "ticket row should be gone after the owner deletes it");

        cleanup_user(&pool, "TEST-TICKET-DELETE-REAL-OWNER").await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; see the plan's Global Constraints for the \
                DATABASE_URL incantation, then run with `cargo test -p api \
                delete_ticket -- --ignored --test-threads=1`"]
    async fn delete_ticket_a_deleted_ticket_disappears_from_every_other_ticket_reading_route() {
        let pool = connect().await;
        let token = seed_session(&pool, "TEST-TICKET-DELETE-CASCADE-READS").await;
        let tracking_id =
            seed_tracked_train(&pool, "TEST-TICKET-DELETE-CASCADE-READS", Some("D44444"), "2026-08-29".parse().unwrap()).await;
        let ticket_id = seed_ticket(&pool, "TEST-TICKET-DELETE-CASCADE-READS", Some(tracking_id)).await;

        let router = test_router(test_app(pool.clone()));
        let (status, _) = delete_ticket_request(router.clone(), ticket_id, Some(&token)).await;
        assert_eq!(status, StatusCode::NO_CONTENT);

        // The list route no longer includes it...
        let (status, tickets) = request(router.clone(), "/Train/tickets/mine".to_string(), Some(&token)).await;
        assert_eq!(status, StatusCode::OK);
        let rows = tickets.as_array().expect("array body");
        assert!(
            rows.iter().all(|r| r.get("id").and_then(Value::as_i64) != Some(ticket_id)),
            "deleted ticket should not appear in the mine list: {rows:?}"
        );

        // ...and the per-ticket delay-repay route 404s, same as any other
        // ticket that never existed -- proves Decision 3's "no orphaned
        // estimate" claim (both reads recompute fresh from the row on
        // every request; once the row is gone, there is nothing to find).
        let (status, body) = request(router, format!("/Train/{tracking_id}/tickets/{ticket_id}/delay-repay"), Some(&token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body, Value::String("no ticket with that id for that tracked train".to_string()));

        cleanup_user(&pool, "TEST-TICKET-DELETE-CASCADE-READS").await;
    }
```

- [ ] **Step 4: Run the tests**

Run: `cargo build -p api` (confirms `router_builds_without_panicking`,
`train.rs:559-562`, still passes -- the regression check for the
`{ticket_id}` parameter-name-collision risk called out in Global
Constraints). With a live `DATABASE_URL`:
`cargo test -p api delete_ticket -- --ignored --test-threads=1`.
Expected: PASS (all five new tests, plus the pre-existing
`router_builds_without_panicking` and
`literal_route_wins_over_same_position_dynamic_route`, unaffected).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/routes/train.rs
git commit -m "Add DELETE /Train/tickets/{ticketId} route"
```

---

### Task 3: Frontend -- `TicketSummary` provenance badge + added-date line

**Files:**
- Modify: `frontend/components/TicketSummary.tsx`
- Modify: `frontend/components/TicketSummary.test.tsx`

**Interfaces:**
- Produces: `TicketSummary`'s widened prop type,
  `Pick<TrackedTrainTicket | TicketListItem, 'operator' | 'ticketType' |
  'originCrs' | 'destinationCrs' | 'source' | 'createdAt'>`.
- Consumed by: this component's three existing call sites
  (`TicketPanel.tsx:94`, `app/track/mine/page.tsx:159,194`) automatically
  -- none of them need their own changes, since they already pass the full
  `TrackedTrainTicket`/`TicketListItem` object (both already carry
  `source`/`createdAt`, confirmed in Global Constraints).
- **Depends on:** nothing -- independent of every other task in this plan
  (pure frontend rendering change, no backend dependency).

- [ ] **Step 1: Write the failing tests first**

Add two new test cases to `frontend/components/TicketSummary.test.tsx`
(currently 34 lines, 4 existing tests unaffected by this change but their
fixtures need the two new required fields added -- see Step 3):

```tsx
  it('renders a provenance badge for the ticket source', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: null,
          originCrs: null,
          destinationCrs: null,
          source: 'pkpass-semantics',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.getByText('From Wallet pass')).toBeInTheDocument();
  });

  it('renders the added-on date via formatDateTime', () => {
    renderWithMantine(
      <TicketSummary
        ticket={{
          operator: 'LNER',
          ticketType: null,
          originCrs: null,
          destinationCrs: null,
          source: 'manual',
          createdAt: '2026-08-29T12:00:00Z',
        }}
      />,
    );
    expect(screen.getByText(/Added/)).toBeInTheDocument();
  });
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run TicketSummary` (from `frontend/`).
Expected: FAIL -- `screen.getByText('From Wallet pass')` and
`screen.getByText(/Added/)` not found; also TypeScript errors on the new
test object literals until `TicketSummary`'s prop type is widened.

- [ ] **Step 3: Widen `TicketSummary.tsx` and add the badge + date line**

Full new content:

```tsx
import { Badge, Group, Stack, Text } from '@mantine/core';
import type { TrackedTrainTicket, TicketListItem, TicketSource } from '@/lib/types';
import { formatDateTime } from '@/lib/dateFormat';

/** Provenance labels for `TicketSummary`'s badge -- styled after
 * `IssueList.tsx`'s `DATA_QUALITY_LABELS` (`components/IssueList.tsx:38-44`),
 * this feature's own conceptual sibling per
 * `crates/api/migrations/20260829090000_journey_ticket_tracking.sql:17-23`'s
 * own comment ("extending DESIGN.md's dataQuality philosophy"). Exact
 * wording is a naming detail, not load-bearing -- see
 * docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md's
 * Open Question 2. */
const SOURCE_LABELS: Record<TicketSource, string> = {
  manual: 'Manual entry',
  'pkpass-semantics': 'From Wallet pass',
  'pkpass-heuristic': 'From Wallet pass',
  'pdf-heuristic': 'From PDF',
};

/** The "operator — ticket type" / "origin → destination" row renderer,
 * shared by `TicketPanel.tsx` (one tracked train's own tickets) and the
 * merged `app/track/mine/page.tsx` (both a train's attached tickets and
 * its own standalone-tickets section) -- extracted out of `TicketPanel.tsx`,
 * where it was previously a private, unexported function, so both can
 * reuse it rather than duplicating ticket-row rendering. `Pick<...>` keeps
 * the prop narrow: this component only ever reads these six fields, from
 * either wire shape. `source`/`createdAt` are never `null`/`undefined` on
 * either wire shape (both are `NOT NULL` columns on `tracked_train_tickets`,
 * independent of `tracked_train_id`'s attachment status), so this
 * component needs no fallback rendering for either -- unlike
 * `operator`/`ticketType`/the CRS fields, which stay optional. */
export function TicketSummary({
  ticket,
}: {
  ticket: Pick<
    TrackedTrainTicket | TicketListItem,
    'operator' | 'ticketType' | 'originCrs' | 'destinationCrs' | 'source' | 'createdAt'
  >;
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
      <Group gap="xs">
        {/* Explicit gray, same rationale as IssueList.tsx's dataQuality
            badge (components/IssueList.tsx:366-372): without a `color`,
            Mantine falls back to theme.primaryColor, making this read as
            branded or interactive. It's provenance, not brand. */}
        <Badge variant="outline" size="sm" color="gray">
          {SOURCE_LABELS[ticket.source]}
        </Badge>
        <Text size="xs" c="dimmed">
          Added {formatDateTime(ticket.createdAt)}
        </Text>
      </Group>
    </Stack>
  );
}
```

- [ ] **Step 4: Fix the four existing tests' fixtures**

Add `source: 'manual', createdAt: '2026-08-29T12:00:00Z'` (or any fixed
RFC3339 string) to each of the four existing ticket object literals in
`TicketSummary.test.tsx` (currently lines 9, 16, 23, 30) -- these are now
required fields on the widened `Pick<...>` type, so `tsc` fails to compile
the test file until every literal supplies them. This is a pure addition
(no assertion in these four tests changes).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `npx vitest run TicketSummary` (from `frontend/`).
Expected: PASS (all 6 tests).

- [ ] **Step 6: Commit**

```bash
git add frontend/components/TicketSummary.tsx frontend/components/TicketSummary.test.tsx
git commit -m "Surface ticket source and added-on date on TicketSummary"
```

---

### Task 4: Frontend -- new `DeleteTicketButton` component

**Files:**
- Create: `frontend/components/DeleteTicketButton.tsx`
- Create: `frontend/components/DeleteTicketButton.test.tsx`

**Interfaces:**
- Produces: `DeleteTicketButton({ ticketId: number })`, a `'use client'`
  component.
- Consumed by: Task 5 (`TicketPanel.tsx`, `app/track/mine/page.tsx`).
- **Depends on:** nothing -- independent of Tasks 1-3 (a self-contained new
  component; its own tests mock `fetch` and `next/navigation`, so it needs
  neither the real backend route nor `TicketSummary`'s changes to be
  testable in isolation). Task 5 does need Task 2's real route to exist for
  the feature to work end-to-end once wired in.

- [ ] **Step 1: Write the failing tests first**

Full content of `frontend/components/DeleteTicketButton.test.tsx`,
modeled on `DeleteTrainButton.test.tsx` (currently 79 lines) but using the
`refreshMock` pattern `AttachTicketAction.test.tsx` already establishes
(currently lines 7-12) instead of `pushMock`:

```tsx
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DeleteTicketButton } from './DeleteTicketButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/track/mine',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('DeleteTicketButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not call DELETE until the confirmation modal is confirmed', () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('DELETEs the ticket and refreshes the page on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/Train/tickets/5', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('shows an error and does not refresh on a failed delete', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no ticket with that id', { status: 404 }));

    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(screen.getByText('no ticket with that id')).toBeInTheDocument();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  // This button only ever renders for a ticket the enclosing Server
  // Component just fetched, so a 401 here can only come from a session
  // that lapses between page load and this click -- same narrow race
  // DeleteTrainButton already reasons about for its own delete.
  it('a 401 shows a login prompt instead of the raw backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<DeleteTicketButton ticketId={5} />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    const loginLink = await screen.findByRole('link', { name: 'Log in to delete this ticket' });
    expect(loginLink).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrack%2Fmine');
    expect(screen.queryByText('no session')).not.toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `npx vitest run DeleteTicketButton` (from `frontend/`).
Expected: FAIL -- module `./DeleteTicketButton` does not exist yet.

- [ ] **Step 3: Write `DeleteTicketButton.tsx`**

Full content:

```tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Modal, Text, Group } from '@mantine/core';
import { useDisclosure } from '@mantine/hooks';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';

/** Deletes via the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * -- this is a Client Component and cannot reach the `api` service directly.
 * `/api/Train/tickets/{ticketId}` is passed straight through to the
 * backend's `DELETE /Train/tickets/{ticketId}`
 * (`crates/api/src/routes/train.rs::delete_ticket`) with no `/public/`
 * prefix inserted -- see that proxy's own `resolveTargetPath` comment.
 * Closely modeled on `DeleteTrainButton.tsx` (same confirm-modal shape,
 * same distinct `aria-label="Confirm delete"` naming rationale for the two
 * same-text "Delete" buttons, same `useNeedsLogin`/`LoginLink` `401`
 * handling, same generic-error-message fallback for any other non-`ok`
 * status) -- but calls `router.refresh()` on success, never
 * `router.push(...)`.
 *
 * This is a deliberate divergence from `DeleteTrainButton`: deleting a
 * tracked train removes the entire subject of the page it's rendered on,
 * so navigating away is correct there. Deleting a *ticket* always happens
 * from inside a list of other things (a train's other tickets, or
 * `/track/mine`'s other trains/tickets) that remain valid and worth
 * showing afterwards -- `router.refresh()` is the same mechanism
 * `AttachTicketAction.tsx:41` and `PinToggle.tsx:99` already use for this
 * "mutate one row, stay on this page" shape. It re-runs the enclosing
 * Server Component (`TicketPanel`, or `app/track/mine/page.tsx`), which
 * naturally drops the now-deleted ticket from its next render -- no
 * client-side list-splicing logic needed here.
 *
 * A `401` here can only really happen from a session that lapses between
 * page load and this click (every call site only ever renders this button
 * for a ticket the enclosing Server Component just fetched) -- same narrow
 * race `DeleteTrainButton` already reasons about. A `404` (a
 * double-click/stale-render race) is not treated as a distinguishable case
 * either, same posture.
 *
 * Known, accepted trade-off shared with every other per-row action in this
 * app (e.g. `PinToggle` on a list page): this component is rendered once
 * per ticket, each instance with its own independent `opened`/`deleting`
 * state, so nothing prevents a caller from having two different tickets'
 * confirm modals open at once. Not a new risk this component introduces --
 * no per-row action in this codebase guards against that today. */
export function DeleteTicketButton({ ticketId }: { ticketId: number }) {
  const router = useRouter();
  const [opened, { open, close }] = useDisclosure(false);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function handleDelete() {
    setDeleting(true);
    setError(null);
    needsLoginState.reset();
    try {
      const response = await fetch(`/api/Train/tickets/${ticketId}`, { method: 'DELETE' });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setDeleting(false);
        return;
      }
      router.refresh();
    } catch {
      setError('Request failed.');
      setDeleting(false);
    }
  }

  return (
    <>
      <Button variant="outline" color="red" size="xs" onClick={open}>
        Delete
      </Button>
      <Modal opened={opened} onClose={close} title="Delete this ticket?">
        <Text>This cannot be undone.</Text>
        {error && <Text c="red">{error}</Text>}
        {needsLoginState.needsLogin && (
          <LoginLink underline="always">Log in to delete this ticket</LoginLink>
        )}
        <Group justify="end" mt="md">
          <Button variant="default" onClick={close} disabled={deleting}>
            Cancel
          </Button>
          <Button color="red" onClick={handleDelete} loading={deleting} aria-label="Confirm delete">
            Delete
          </Button>
        </Group>
      </Modal>
    </>
  );
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `npx vitest run DeleteTicketButton` (from `frontend/`).
Expected: PASS (all 4 tests).

- [ ] **Step 5: Commit**

```bash
git add frontend/components/DeleteTicketButton.tsx frontend/components/DeleteTicketButton.test.tsx
git commit -m "Add DeleteTicketButton, modeled on DeleteTrainButton"
```

---

### Task 5: Frontend -- wire `DeleteTicketButton` into `TicketPanel.tsx` and `app/track/mine/page.tsx`

**Files:**
- Modify: `frontend/components/TicketPanel.tsx`
- Modify: `frontend/components/TicketPanel.test.tsx`
- Modify: `frontend/app/track/mine/page.tsx`
- Modify: `frontend/app/track/mine/page.test.tsx`

**Interfaces:**
- Consumes: `DeleteTicketButton` (Task 4), the real `DELETE
  /Train/tickets/{ticketId}` route (Task 2, for end-to-end correctness --
  the tests below mock `fetch`, so they don't need the live route).
- **Depends on:** Task 2 and Task 4. Land after Task 3 too, to avoid a
  merge conflict on `TicketPanel.tsx`/`app/track/mine/page.tsx` (no
  functional dependency between them).

- [ ] **Step 1: Wire `TicketPanel.tsx`**

Add the import and the button, inside the existing per-ticket `Stack`
(currently `TicketPanel.tsx:91-97`):

```tsx
import { TicketSummary } from './TicketSummary';
import { DeleteTicketButton } from './DeleteTicketButton';
```

```tsx
      {withEstimates.map(({ ticket, estimate }, index) => (
        <Stack key={ticket.id} gap="xs">
          {index > 0 && <Divider />}
          <TicketSummary ticket={ticket} />
          {estimate && <DelayRepayEstimate response={estimate} />}
          <DeleteTicketButton ticketId={ticket.id} />
        </Stack>
      ))}
```

- [ ] **Step 2: Extend `TicketPanel.test.tsx`**

`TicketPanel.test.tsx` currently imports `screen` only from
`@testing-library/react` (line 2) -- extend that import to add
`fireEvent, waitFor`. Extend the existing "200 with tickets" test
(currently lines 58-83) to assert the button renders, and add a new test
proving it's wired to the right ticket id:

```tsx
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
```

(add this line to the end of the existing "200 with tickets" test body).

```tsx
  it('clicking Delete for a ticket DELETEs that exact ticket id', async () => {
    vi.mocked(api.getSession).mockResolvedValue(session(true));
    vi.mocked(api.getTicketsForTrackedTrain).mockResolvedValue([
      {
        id: 7,
        trackedTrainId: 1,
        operator: 'LNER',
        ticketType: null,
        originCrs: null,
        destinationCrs: null,
        source: 'manual',
        createdAt: '2026-08-29T12:00:00Z',
      },
    ]);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));

    renderWithMantine(await TicketPanel({ trackingId: 1 }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/7', { method: 'DELETE' });
    });
    vi.unstubAllGlobals();
  });
```

- [ ] **Step 3: Wire `app/track/mine/page.tsx`**

Add the import at the top:

```tsx
import { DeleteTicketButton } from '@/components/DeleteTicketButton';
```

`TrackedTrainListRow`'s per-ticket block (currently lines 157-172): add
`<DeleteTicketButton ticketId={ticket.id} />` after `<DelayRepayEstimate .../>`:

```tsx
            {tickets.map((ticket) => (
              <Stack key={ticket.id} gap={4}>
                <TicketSummary ticket={ticket} />
                <DelayRepayEstimate
                  response={{
                    delayMinutes: ticket.delayMinutes,
                    estimate: ticket.estimate,
                    claimUrl: ticket.claimUrl,
                    disclaimer: ticket.disclaimer,
                  }}
                />
                <DeleteTicketButton ticketId={ticket.id} />
              </Stack>
            ))}
```

`UnattachedTicketRow` (currently lines 180-212): add it inside the
existing `Group` alongside `AttachTicketAction`/the track-a-new-train link
(currently lines 203-208):

```tsx
        <Group gap="lg" wrap="wrap" align="flex-end">
          <AttachTicketAction ticketId={ticket.id} trains={trains} />
          <TextLink href={`/track?${trackParams.toString()}`} underline="always">
            Track a new train for this ticket
          </TextLink>
          <DeleteTicketButton ticketId={ticket.id} />
        </Group>
```

- [ ] **Step 4: Extend `app/track/mine/page.test.tsx`**

Extend the existing "a train with an attached ticket" test (currently
lines 90-102) and "a standalone (unattached) ticket" test (currently
lines 124-152) each with:

```tsx
    expect(screen.getByRole('button', { name: 'Delete' })).toBeInTheDocument();
```

Add a new click-through test proving the attached-ticket row's button is
wired to the right id (this file already mocks `next/navigation`'s
`useRouter` with `refresh: vi.fn()`, currently line 17, so no additional
router mock is needed):

```tsx
  it('clicking Delete on an attached ticket row DELETEs that exact ticket id', async () => {
    vi.mocked(api.getMyTrackedTrains).mockResolvedValue([train()]);
    vi.mocked(api.getMyTickets).mockResolvedValue([ticket({ id: 9, trackedTrainId: 1 })]);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(null, { status: 204 })));

    renderWithMantine(await MyTrackedTrainsPage());
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetch).toHaveBeenCalledWith('/api/Train/tickets/9', { method: 'DELETE' });
    });
    vi.unstubAllGlobals();
  });
```

This file's imports (currently line 1-2) need `fireEvent, waitFor` added
to the `@testing-library/react` import.

- [ ] **Step 5: Run the tests**

Run: `npx vitest run TicketPanel app/track/mine` (from `frontend/`).
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/components/TicketPanel.tsx frontend/components/TicketPanel.test.tsx \
        frontend/app/track/mine/page.tsx frontend/app/track/mine/page.test.tsx
git commit -m "Wire DeleteTicketButton into TicketPanel and the /track/mine ticket rows"
```

---

### Task 6: Full-stack verification

**Files:** none (verification only -- no source changes in this task).

**Depends on:** Tasks 1-5, all landed.

- [ ] **Step 1: Rust workspace build and unit tests**

Run: `cargo build --workspace && cargo test --workspace`.
Expected: PASS. This does not exercise the `#[ignore]`d live-database
tests added in Tasks 1-2 -- those need Step 2.

- [ ] **Step 2: Rust live-database tests**

With `DATABASE_URL` set to a real Postgres instance carrying this app's
migrations (see this plan's Global Constraints for the convention; every
existing `#[ignore]`d test in this codebase uses the same incantation):

```bash
cargo test -p api delete_ticket -- --ignored --test-threads=1
```

Expected: PASS (9 tests total -- 4 from Task 1's `train_tracking.rs`
`db_tests`, 5 from Task 2's `routes/train.rs` `db_tests`).

- [ ] **Step 3: Frontend unit tests**

Run (from `frontend/`): `npx vitest run`.
Expected: PASS, including every new/modified test from Tasks 3-5.

- [ ] **Step 4: Frontend type check**

Run (from `frontend/`): `npx tsc --noEmit`.
Expected: PASS -- in particular, this is what would catch a missed
`source`/`createdAt` fixture update anywhere else in the frontend that
constructs a `TrackedTrainTicket`/`TicketListItem` object literal and
passes it to `TicketSummary` (Task 3's Global Constraints note: only three
call sites exist today, both already passing the full wire type, so none
should need a change -- this step is the safety net confirming that).

- [ ] **Step 5: Frontend production build**

Run (from `frontend/`): `npm run build`.
Expected: PASS (exercises the same project-wide type-check as Step 4,
plus static-generation eligibility for every route -- both
`app/track/mine/page.tsx` and any page rendering `TicketPanel` already
set `revalidate = 0` for reasons unrelated to this plan, so this should
be a clean pass-through with no new build-time surprises).

- [ ] **Step 6: Manual smoke check (optional but recommended)**

If a local dev environment with a seeded database is available: log in,
navigate to `/track/mine`, confirm the provenance badge and "Added ..."
line render on both an attached and a standalone ticket row, and confirm
clicking Delete on each removes it from the page without navigating away.

---

## Not in this plan

**Feature #3 ("open the original PDF" / render a QR code from a
`.pkpass`)** is not planned here in any form -- it is blocked on an
unresolved product/legal decision. See
`docs/superpowers/specs/2026-09-02-ticket-display-delete-original-design.md`'s
Open Questions #1 for the pending decision this needs before any
implementation planning can start.
