# Train Listing Page (`/trains`, Approach B v1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship `/trains` — a destination-first, whole-network, CIF-derived
train search page — alongside (never replacing) `/track`, plus a "Track this
train" CTA on the existing public `/train/[uid]/[date]` page, so a user can
find an arbitrary train by where it's going rather than only by a station
they already know, click through to its public status page, and start
tracking it in one action.

**Architecture:** A second grouping pass in `crates/schedule-query`
(`departures_by_destination_crs`, sibling to the already-shipped
`departures_by_crs`, run against the *same* transient, per-cycle
`ScheduleIndex`) buckets every non-cancelled schedule's departure-bearing
calling points by their schedule's **destination** CRS instead of by their
own origin CRS. `crates/schedule-reference` then **flattens** that grouping
— one JSON object per departure, each carrying its own `destination_crs`,
uncapped, for the *whole rail day* (`NaiveTime::MIN`, not
`london_local_time_now()`) — and publishes it once per CIF delivery to a new
`POST /private/schedule-destination-departures` ingest route. That route
wholesale-replaces the day in a new **flat, one-row-per-departure** table,
`schedule_destination_departures (service_date, destination_crs, scheduled,
train_uid, origin_crs)`, whose composite primary key is also the covering
index for the only query shape the read side needs, in exactly the order it
needs it — written with one `DELETE` by `service_date` plus one `UNNEST`
bulk insert in a single transaction, not a per-row loop (~377,000 rows per
day). A new public read route,
`GET /public/trains/search?destination=&origin=&from=&to=&limit=&after=`,
answers it with a bounded **index range scan** and a **keyset cursor** over
`(scheduled, train_uid, origin_crs)`, applying the `now`-forward filter at
*request* time (where the clock is actually correct) and returning the
envelope `{results, nextCursor}` in camelCase; the 404-vs-`200 []` split is
preserved by a cheap day-scoped existence probe. Because the flat table
accrues a full day of rows per service date rather than ~2,500 bucket rows,
a new `prune_schedule_destination_departures` job runs inside the
aggregator's existing prune cycle. On the frontend, a new client component
`TrainSearchForm` drives that route through the existing same-origin
`/api/*` proxy, renders each result with a link to `/train/{uid}/{today}`
and a shared `TrackThisTrainButton` that calls
`POST /Train/by-uid/{uid}/{date}/track`, and offers a "Load more" control
that re-requests with `after=` and **appends** — with `TrackTrainForm`'s
exact best-effort ticket-attach follow-up on `/trains`, and without it on
`/train/[uid]/[date]` (per the spec's own §5 exclusion).

**Tech Stack:** Rust (`axum`, `sqlx` runtime-checked `query`/`query_as` — no
`query!` macro, `anyhow`, `chrono`, `serde_json`, `reqwest`), PostgreSQL 16,
Next.js/React (Server Component pages + `'use client'` interactive
components, Mantine UI), Vitest + `@testing-library/react` +
`renderWithMantine`, `cargo test` with `#[tokio::test] #[ignore]`-gated
live-database tests.

**Spec:** `docs/superpowers/specs/2026-09-07-train-listing-page-design.md`
(read in full; every reference to "the design doc" below means this
document). Its §3 **Recommendation — Approach B** is a settled decision this
plan does not re-litigate: Approaches A and C are not built, not partially
built, and not kept as fallbacks anywhere in this plan.

**Spec addendum (supersedes part of the above):**
`docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md`
— also required reading, in full. Every reference to "the addendum" below
means this document. Where the two disagree on **publish / storage / query
mechanism**, the addendum wins; on everything else (the product decision,
§4's "`/track` is not replaced", §5's CTA, §6's exclusion list) the original
design doc stands unchanged.

---

> ## ⚠️ This plan was revised on 2026-09-07, after it had already begun
>
> **Read this before executing any task.** Tasks 1, 2, 4 and 5 of this plan
> are **not** what they were when it was first written, and Tasks 3, 6, 7
> and 10 have named changes. A thirteenth task was added.
>
> **Why:** this plan's own Task 1 — a controller-run gating diagnostic —
> was run for real against the live production CIF delivery and returned
> numbers the plan's own size check fails on. The busiest destination
> (London Waterloo) buckets **9,634** departure-bearing calling points for
> one day, with the next several busiest within the same order of magnitude,
> so **no value of `MAX_DEPARTURES_PER_DESTINATION` is defensible**: any cap
> that fits the payload truncates the busiest destinations to roughly the
> first two hours after the daily CIF delivery landed, which — because the
> publish fires once per *delivery*, not once per 30-minute *cycle* — means
> a search for "trains to Waterloo" at 18:00 honestly returns nothing. Task
> 1's own escape hatch ("stop and flag it back to the repo owner: the honest
> fix would be pagination…") was exercised, and its output is
> `docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md`,
> whose **Approach C is accepted**.
>
> **What that means concretely:** the per-destination JSONB-bucket-with-a-cap
> storage shape is replaced by a **flat, one-row-per-departure table**,
> published **uncapped for the whole day**, with the `now`-forward filter and
> pagination moved to the **read** side as an indexed range scan plus a
> keyset cursor. The cap is deleted, not re-valued. Every revised task below
> says so in its own header; the addendum's §5 table is the authoritative
> task-by-task disposition, and this plan now matches it.

---

## Decisions this plan resolves (the design doc's own §7 Open Questions)

The design doc closed with six open questions. This plan resolves the two
that actually block implementation, and records why the other four are
deliberately left alone.

1. **Open Question 1 — the destination-keyed bucket's cap/cardinality is
   unmeasured.** **Resolved, and then re-resolved.** Task 1's controller-run
   diagnostic was run for real against the live production CIF delivery, and
   the answer is that **there is no defensible cap at all**: the busiest
   destination buckets 9,634 departure-bearing calling points for one day and
   the next several busiest are within the same order of magnitude, so a cap
   that fits the payload truncates exactly the destinations a whole-network
   destination search exists to serve — and, because the publish fires once
   per CIF *delivery* rather than once per 30-minute *cycle*, it truncates
   them to "the first N after the delivery landed", not "the next N from
   now". The addendum's §1 and §3 work that through in full.

   The resolution is therefore **not a number but a shape change**:
   `MAX_DEPARTURES_PER_DESTINATION` is **deleted** (Task 4), the storage
   becomes a flat one-row-per-departure table published uncapped for the
   whole day (Task 2), and the `now`-forward filter plus pagination move to
   the read side as an indexed range scan with a keyset cursor (Tasks 5 and
   7). Nothing downstream hard-codes a cap any more.

   **Task 1 survives, re-scoped, and now gates nothing but a publish
   strategy.** It measures `Σ_d bucket_d` — the network-wide *sum* of
   departure-bearing calling points for a whole day from `00:00`, not the
   per-destination max — and multiplies by ~80 bytes. Its one remaining
   output is a binary: single-POST (the default, expected ~21MB) versus the
   addendum §3 chunked-publish fallback. It still mirrors the
   shared-train-identity plan's Task 6 precedent (a real, executable,
   controller-run measurement rather than an assumption), but it is **no
   longer a gate on Task 4** — no constant depends on it.

2. **Open Question 2 — is `train_tracking::create_subscription_for_train`
   idempotent for a second click by the same user on the same train?**
   **Resolved definitively: NO, it is not, and this was already known and
   proven in-repo.** Evidence, read directly:
   - `crates/api/src/data/train_tracking.rs:162-169` — the function's own doc
     comment states it verbatim: *"NOT idempotent by `(trains_id, user_id)`
     — every call inserts a new row, same as `create_pin`'s own
     long-established behavior for the legacy path… Calling this twice for
     the same train therefore creates two separate subscriptions, each
     independently rename/delete-able."*
   - `crates/api/src/data/train_tracking.rs:170-187` — the implementation is
     a bare `INSERT … SELECT … RETURNING id` with **no** `ON CONFLICT`, no
     existence check, no `WHERE NOT EXISTS`.
   - `crates/api/src/data/train_tracking.rs:3506-3556` — a live-database test,
     `create_subscription_for_train_called_twice_creates_two_separate_subscriptions`,
     asserts `assert_ne!(first_tracking_id, second_tracking_id)` and
     `assert_eq!(row_count, 2)`.
   - There is **no** uniqueness constraint backing it: the only index that
     ever guarded per-user train identity,
     `tracked_trains_resolved_identity`
     (`crates/api/migrations/20260828120000_train_tracking.sql:85`), was
     dropped outright by
     `crates/api/migrations/20260906140000_drop_legacy_columns.sql`, and
     `crates/api/migrations/20260907100000_rename_tracked_trains.sql` renamed
     the table to `train_subscriptions` with only three
     non-unique indexes carried over.

   **Task 8 therefore exists and is ordered before every task that wires a
   CTA** (Tasks 9-12). Its fix is **application-level, inside
   `create_subscription_for_train` only** — a single atomic
   existing-row-wins CTE — and deliberately **not** a `(user_id, trains_id)`
   unique index. That was considered and rejected on direct evidence: four
   separate, unrelated code paths already do a bare
   `UPDATE train_subscriptions SET trains_id = $2 WHERE id = $1`
   (`crates/api/src/data/schedule_matching.rs:131`,
   `crates/api/src/data/trust_event_backlog_match.rs:394` and `:504`,
   `crates/api/src/data/train_tracking.rs:684`), and `create_pin`
   (`train_tracking.rs:79-100`) has never deduplicated legacy pins at all —
   so a user with two legacy CRS+time pins for the same physical train is
   reachable *today*, and a global unique index would turn each of those four
   `UPDATE`s into a hard failure inside schedule matching, backlog matching,
   and live TRUST resolution. Task 8 states this tradeoff in the code, and
   Task 9 closes the remaining concurrent-double-submit window the
   application-level fix cannot close, the same way every other mutating
   control in this app already does: an in-flight `busy` state that disables
   the button.

3. **Open Question 3 (mixed-source rows), 4 (retention/pruning parity),
   5 (shareable filter state in the URL), 6 (same-day VSTP amendments).**
   - 3 needs no resolution: this plan's `/trains` is **CIF-derived only**
     (the design doc's own §3 Approach B v1 scope and §6's "no true
     whole-network live-board destination search"), so no result row has two
     possible sources and there is nothing to merge or disambiguate.
   - 4 **is resolved by a task — Task 13 — and this reverses the original
     plan's answer.** The original reasoning was: `schedule_network_departures`
     has no retention/pruning job anywhere (confirmed — no
     `prune_schedule_network_departures` exists in `crates/aggregator`, and
     `upsert_schedule_network_departures` wholesale-replaces per
     `(crs, service_date)` each cycle), so parity is achieved by doing
     nothing. That was sound for a ~2,500-row bucket table. It does **not**
     survive the addendum's flat shape, which accrues roughly **377,000 rows
     per service date, forever**, because the wholesale replace is now scoped
     to one `service_date` and nothing ever deletes yesterday's. Task 13
     therefore adds `prune_schedule_destination_departures` to
     `crates/aggregator/src/queries.rs`, modelled on the existing
     `prune_history`/`prune_trust_event_backlog` pair, with a **2-day**
     default retention. Task 2's migration header comment states this
     explicitly instead of the old "RETENTION: none, deliberately".
   - 5 **is** implemented, because it costs nothing here: Task 11's `/trains`
     page reads `?destination=`, `?origin=` and `?ticketId=` out of
     `searchParams` exactly the way `frontend/app/track/page.tsx:7,10-21`
     already reads `?origin=`/`?ticketId=`. This is a direct consequence of
     the §4 requirement that `/trains` be reachable via a `?ticketId=`
     deep link; extending the same mechanism to the two filter fields is one
     extra prop each.
   - 6 is genuinely out of this plan's scope — it is inherited, unchanged,
     from two prior shipped features, and nothing here makes it better or
     worse.

## Global Constraints

These are the design doc's §6 exclusions **plus the addendum's own §6
additions**, binding on **every** task below. If a task seems to need one of
these, stop and re-read the design doc and the addendum — it doesn't.

Nothing in the original §6 list assumed the JSONB-bucket storage shape, so
the revision did not have to remove anything here; it only had to add the
addendum's five further exclusions (the last five bullets below).

- **No change to `/Train/track`, `TrackPinRequest`, or `post_track`'s
  matching logic.** This plan only adds a new discovery surface and a new
  CTA; the legacy fallback path is untouched.
- **No CIF operator/`BX`/headcode decoding.** `schedule_query::records`
  carries no operator field and this plan does not add one. There is **no
  operator filter** on `/trains` — CIF rows have no operator field at all.
- **No browsing a date other than today.** Both the LDBWS and CIF sources
  stay "always now/today, server-side" (`get_station_schedule_departures`'s
  own posture, `crates/api/src/routes/departures.rs:71-88`). The new route
  computes `today` server-side and accepts no date parameter.
- **No resident, permanently-in-memory whole-network index.** The new
  grouping pass is transient and per-cycle, run against the same
  stack-local `ScheduleIndex` `publish_cif_derived_products` already builds
  once per cycle (`crates/schedule-reference/src/main.rs:194`) — never a
  second parse, never a cached index.
- **No synchronous request-time call from `api` into `schedule-reference`**
  (or any other batch poller). Publish-then-poll only, same as every
  existing cross-service link in this app.
- **No broadening of `poller-ldbws`'s ~286-station sampled set.**
- **No whole-network LDBWS (live-board) destination search.** CIF-derived
  only for this v1.
- **No merging of LDBWS and CIF-derived rows for the same train/station.**
- **No `ticketId` deep-link convention for `/train/[uid]/[date]`.** The
  `/trains` page's own "Track this train" action gets full ticket-attach
  parity (design doc §4); the `/train/[uid]/[date]` CTA gets **no**
  `attachTicketId` at all (design doc §5, explicit).
- **No pixel-level UI design.** Use plain, functional Mantine markup
  consistent with `frontend/components/TrackTrainForm.tsx`'s own structure
  (`Stack`/`Group`/`Autocomplete`/`ScrollArea`/`Button`/`Alert`). Every
  component in this plan ships complete, real markup — "left to design
  review" is not a valid step in any task.
- **`crates/api` uses runtime-checked `sqlx::query`/`sqlx::query_as`
  exclusively** — no `query!`/`query_as!`, no `.sqlx` query cache. Every new
  query in this plan follows that.
- **New JSON responses stay camelCase**, produced the same way every other
  hand-built response in this crate is — a `serde_json::json!()` field-by-field
  render function in `crates/api/src/render.rs`, not a
  `#[derive(Serialize)]` struct (see `schedule_departure_json`,
  `crates/api/src/render.rs:171-181`, the exact sibling to copy).
- **Backend live-database tests** follow this crate's existing convention
  exactly: a `#[cfg(test)] mod db_tests` block, a private `connect()` reading
  `DATABASE_URL` via `PgPoolOptions`, `#[tokio::test]` +
  `#[ignore = "requires a live database; …"]`, seed via raw `sqlx::query`
  INSERT, and an explicit `DELETE` cleanup at the end of the test body. Do
  **not** use `#[sqlx::test]` — it is not used anywhere in this crate.
  Fixture CRS codes come from the reserved `Z…` namespace and must not
  collide with ones already claimed (`ZQQ`/`ZQR`/`ZQS` by
  `routes/station_stats.rs`, `ZQT`/`ZQU`/`ZQV`/`ZQW`/`ZQX`/`ZQY`/`ZQZ`/`ZRA`
  by `routes/departures.rs` and `routes/ingest.rs`). This plan claims
  `ZRB`-`ZRF`.
- **Frontend tests** use Vitest + `@testing-library/react` +
  `renderWithMantine` (`frontend/test/render`), run via `npm test -- <file>`
  from the `frontend/` directory.
- **No cap of any kind on the destination data** — not in
  `schedule-query`, not in `schedule-reference`, not in the table. The
  addendum's whole argument is that no cap value is defensible;
  `MAX_DEPARTURES_PER_DESTINATION` does not exist in this plan any more and
  must not be reintroduced under another name. `MAX_DEPARTURES_PER_STATION
  = 10` in `crates/schedule-reference` belongs to the *other*, shipped
  product and is untouched.
- **No `COPY`-based or extension-based bulk loader.** An `UNNEST` insert is
  sufficient at ~377,000 rows and stays inside the
  runtime-checked-`sqlx`-only rule above (addendum §6).
- **No partitioning of `schedule_destination_departures` by
  `service_date`.** Named in the addendum's §7 item 5 as the standard
  mitigation if write pressure bites; explicitly not designed there and not
  built here.
- **No consolidating the two CIF-derived departure tables.** The new flat
  table would, uncapped, contain a superset of `schedule_network_departures`,
  so deriving one from the other is plausible — and is explicitly deferred
  (addendum §6 and §7 item 6). `schedule_network_departures`, its migration,
  its `MAX_DEPARTURES_PER_STATION = 10`, `GET
  /public/stations/{crs}/schedule-departures` and `TrackTrainForm`'s use of
  either are all untouched by every task below.
- **No fixing `schedule_network_departures`' own once-per-delivery
  `now`-forward staleness.** Found while grounding the addendum (§1.3),
  named there, and deliberately left alone: this plan's approach avoids
  inheriting it rather than repairing it.

---

## Task 1: Publish-volume diagnostic — measure `Σ_d bucket_d`, the whole-day network total

> **REVISED per the addendum's §5 row 1.** This task used to pick a cap.
> **It no longer does — there is no cap.** It measures one number, the
> network-wide **sum** of departure-bearing calling points for a whole rail
> day, and uses it to choose between a single POST and a chunked publish.
> It is **no longer a gate on Task 4**: no constant anywhere depends on its
> output. Tasks 2 through 13 can all begin before it has been run.

**This task deliberately does not follow the TDD template.** There is no
code to write and no commit to make: it is a real, executable measurement —
the same shape as the shared-train-identity plan's Task 6 ("Backfill
diagnostic — count the un-repointable edge case before Step B runs").

Its original framing (the design doc's own Open Question 1, "Approach B's
per-bucket cap and cardinality are unmeasured… This needs a real check
against `timetable_full.zip` before an implementation plan commits to a
specific cap") has been *answered*: the diagnostic was run, and the answer
was that no cap is defensible. See the addendum's §1.1 for the numbers and
§1.2 for the arithmetic correction that makes the uncapped whole-day publish
viable. What survives is the *sizing* half of the question, restated
correctly: the plan's original size check multiplied a worst-case bucket by
*every* destination, which is not the real bound, because
**every departure-bearing calling point is filed under exactly one
destination bucket** (`departures_by_destination_crs` takes
`resolved.calling_points.last()` once per schedule and uses that single
value as the key for all of that schedule's entries). So the bound is a
sum, not a product:

```
Σ_d bucket_d = total departure-bearing calling points network-wide, for the whole day
```

> **CONTROLLER-RUN. Do not delegate this task to an implementer subagent.**
> It reads a real CIF extract (and, in its fallback form, production
> database state). This session's standing rule is that production
> database/SSH access is performed by the primary controlling agent
> directly, never by a subagent working unsupervised.

**Files:** none created, none changed, nothing committed.

**Interfaces:**
- Produces: one number — `Σ_d bucket_d`, the whole-day network-wide total of
  departure-bearing calling points — and, from it, exactly one binary
  decision recorded in this plan's execution record: **single POST** (the
  default) or **chunked POST**. That decision is consumed by Task 4's
  publish function and, only in the chunked case, by Task 6's handler.
  Nothing else consumes anything from this task.

- [ ] **Step 1: Get a real CIF extract, locally, without touching production**

Preferred and sufficient. This diagnostic is **fully locally reproducible**
— it needs only a CIF `MCA` file, not the database and not SSH — so prefer
this form over the production fallback in Step 4:

```bash
# Either: a fresh timetable_full.zip pulled from the Network Rail SCHEDULE
# feed into the repo root (untracked; this is the same file
# crates/schedule-query/examples/inspect.rs's own module doc already
# documents as the manual re-check input).
ls -la timetable_full.zip

# Or: copy the current delivery's MCA file off the running schedulefeed
# PVC, which is exactly what schedule-reference itself reads
# (crates/schedule-reference/src/discovery.rs).
kubectl cp <namespace>/<schedulefeed-pod>:/data/schedule-feed/<timestamp>/RJTTF<n>MCA.txt ./RJTTFMCA.txt -c schedule-reference
```

- [ ] **Step 2: Run the destination histogram and take the SUM of its third column**

This needs no new Rust and no new binary. A `BS` record opens a schedule
block, `LO`/`LI` records are its departure-bearing calling points, and the
`LT` record's TIPLOC (bytes 3-9, 1-indexed, per
`crates/schedule-query/src/records.rs`'s own offset documentation) is its
destination. This is the **same command the original diagnostic ran** — only
the way its output is read has changed:

```bash
unzip -p timetable_full.zip 'RJTTF*MCA.txt' | awk '
  /^BS/ { dep = 0; next }
  /^LO/ { dep++; next }
  /^LI/ { dep++; next }
  /^LT/ { dest = substr($0, 3, 7); sub(/ +$/, "", dest);
          trains[dest]++; points[dest] += dep; next }
  END   { for (d in trains) printf "%s\t%d\t%d\n", d, trains[d], points[d] }
' | sort -k3 -rn | tee /tmp/dest-histogram.tsv | head -30

# THE NUMBER THIS TASK EXISTS FOR: the SUM of the third column, not its max.
awk -F'\t' '{ total += $3 } END { printf "sum_departure_points=%d\n", total }' \
  /tmp/dest-histogram.tsv
```

(If Step 1 produced a plain `RJTTFMCA.txt` instead of the zip, replace the
`unzip -p …` prefix with `cat ./RJTTFMCA.txt`.)

Columns are `TIPLOC`, `distinct schedule records terminating there`,
`total departure-bearing calling points bucketed under it`. **The third
column's SUM is `Σ_d bucket_d`** — the total number of rows the flat table
will hold for one service date, and the total number of JSON objects one
publish POSTs. The per-destination *maximum* (the old framing) is no longer
a decision input at all; keep the top-30 listing only as context for the
execution record.

If a real `resolve_for_date`-based count is preferred over the text scan,
`crates/schedule-query/examples/inspect.rs` is the existing entry point for
running this crate's real resolution against an extract; either form answers
the question, and the text scan is the cheaper one.

**Read the number correctly — it is an over-count, deliberately.** This awk
pass counts *every* schedule record in the file, across every date range and
days-of-week bitmask, with no STP-overlay resolution. The real per-day total,
after `resolve_for_date` picks one record per UID, is **strictly smaller**
(the addendum's §1.2 derives ~377,000 from the resolved 25,305 non-cancelled
schedules). Erring high is the correct direction for a payload-size
decision. Note that there is **no** `now`-forward shrinkage to subtract any
more: Task 4 now publishes from `NaiveTime::MIN`, the whole day.

- [ ] **Step 3: Record the number and decide single-POST versus chunked-POST**

Write `sum_departure_points` and the top-30 listing into this plan's
execution record (a comment on the tracking issue/PR, or appended to this
file's checklist). Then apply the size check — **which no longer has a
stop-and-flag branch; the escape hatch has already been exercised and the
addendum is its output**:

```
payload_bytes ≈ sum_departure_points × ~80 bytes
```

> **~80, not the addendum's ~55.** The addendum's §3 sketches a four-key
> row; Task 4's actual row carries `service_date` as a fifth key, because
> the ingest handler's first statement is a `DELETE ... WHERE service_date =
> $1` and `common::ingest::post_batch` posts a bare array with nowhere else
> to carry the day. That is ~26 more bytes per entry. Task 4's Interfaces
> block states the same figure and the same reason. Using the larger,
> correct number here only makes this gate *more* conservative.

- **If `payload_bytes` ≤ ~60MB (i.e. below ~750,000 entries): SINGLE
  POST.** This is the expected case and the plan's default. The addendum's
  derived estimate is ~377,000 entries, which at ~80 bytes is **~30MB**,
  against the private router's real limit,
  `DefaultBodyLimit::max(100 * 1024 * 1024)`
  (`crates/api/src/routes/mod.rs:86`) — ~3.3x headroom. Task 4 ships exactly
  as written and Task 6 needs no chunk semantics. Record "single POST" and
  move on.
- **If `payload_bytes` exceeds ~60MB (i.e. ~750,000+ entries): CHUNKED
  POST.** Do **not** stop. `common::ingest::post_batch` takes `&[T]`, so the
  fallback is zero new shared code — in Task 4's
  `publish_schedule_destination_departures`, replace the single
  `post_batch(…, &rows, …)` call with:

  ```rust
  // Chunked publish, adopted ONLY because Task 1's real measurement came
  // in above ~60MB for a single body. Each chunk is its own POST; the
  // per-service_date atomic replace therefore weakens from "one
  // transaction" to "converges once every chunk has landed", which is why
  // Task 6's handler must learn "the FIRST chunk of a publish clears the
  // day" semantics alongside this change. See the addendum's §3,
  // "Escape hatch if the revised measurement comes in high".
  for chunk in rows.chunks(50_000) {
      if let Err(err) = common::ingest::post_batch(
          client,
          &config.schedule_destination_departures_url,
          internal_oauth,
          chunk,
          "schedule-derived destination departures rows",
      )
      .await
      {
          tracing::error!(error = ?err, "failed to publish a chunk of schedule-derived destination departures; will retry next cycle");
          return;
      }
  }
  ```

  and implement Task 6's conditional note. Record "chunked POST" in the
  execution record so Task 6's implementer knows to apply it.

- [ ] **Step 4: Fallback ONLY if no CIF extract can be obtained**

If Step 1 genuinely cannot produce an extract, the weaker proxy is a
controller-run read of the already-published origin-keyed table, which at
least bounds the network-wide order of magnitude:

```bash
psql "$DATABASE_URL" -c "
  SELECT count(*) AS crs_rows,
         sum(jsonb_array_length(departures)) AS total_departures
  FROM schedule_network_departures
  WHERE service_date = CURRENT_DATE;"
```

`total_departures` here is capped at 10 per origin station, so it is a
**floor**, not the answer: it cannot see the calling points the cap already
dropped. Treat a result from this fallback as confirmation of the order of
magnitude only, and — because the derived ~30MB has ~3.3x headroom —
default to **single POST** unless the fallback itself already
implies more than ~750,000 entries. Run this yourself; do not hand it to a
subagent.

- [ ] **Step 5: No commit for this task**

Nothing in the repository changed. **No other task is blocked on this one.**
Tasks 2, 3 and 13 have no dependency on it whatsoever; Task 4 reads its
recorded decision only to pick between the single `post_batch` call it
already contains and the chunked loop in Step 3 above, and Task 6 reads it
only to decide whether to add first-chunk-clears-the-day semantics. If this
task has not been run when Task 4 is dispatched, Task 4 ships the
single-POST default and this task's result is applied afterwards as a small,
localized amendment.

---

## Task 2: Migration — `schedule_destination_departures` (flat, one row per departure)

> **REVISED per the addendum's §5 row 2.** Same file slot, **different
> DDL**: the five-column flat table with a composite primary key, no
> `departures JSONB` and no `updated_at`. The header comment's old
> "RETENTION: none, deliberately" paragraph was correct for a ~2,500-row
> bucket table and is false for this one; it is rewritten below to point at
> Task 13.

**Files:**
- Create: `crates/api/migrations/20260907130000_schedule_destination_departures.sql`
- Test: none of its own — this repo has no migration-testing framework; a
  migration is proven by the next Rust code that queries the new shape
  (Task 5's `db_tests`). Same convention as
  `20260904110000_schedule_network_departures.sql`, which has no test file
  either.

**Interfaces:**
- Consumes: nothing.
- Produces: table `schedule_destination_departures (service_date DATE NOT
  NULL, destination_crs TEXT NOT NULL, scheduled TIME NOT NULL, train_uid
  TEXT NOT NULL, origin_crs TEXT NOT NULL, PRIMARY KEY (service_date,
  destination_crs, scheduled, train_uid, origin_crs))`. Tasks 5, 6, 7 and 13
  all depend on this migration having applied. **Nothing else depends on
  this task, and this task depends on nothing** — it can be done first.

- [ ] **Step 1: Verify the table does not exist yet**

Run: `psql "$DATABASE_URL" -c "SELECT to_regclass('public.schedule_destination_departures');"`
Expected: prints an empty result (`NULL`) — there is something real to build.

- [ ] **Step 2: Write the migration**

```sql
-- ---------------------------------------------------------------------
-- ONE ROW PER DEPARTURE, not one row per destination bucket: every
-- CIF-SCHEDULE-derived, departure-bearing calling point of every
-- non-cancelled schedule TERMINATING at `destination_crs`, for one whole
-- rail day, UNCAPPED, published by schedule-reference once per CIF
-- delivery. Backs GET /public/trains/search.
--
-- The destination-keyed sibling of schedule_network_departures
-- (20260904110000_schedule_network_departures.sql), which is keyed by the
-- departure's OWN origin CRS. Both are produced from the SAME transient,
-- per-cycle ScheduleIndex in the same pass over the same delivery -- see
-- crates/schedule-reference/src/main.rs's publish_cif_derived_products --
-- so this table costs one extra grouping pass and one extra POST, not a
-- second parse and not a resident index. See
-- docs/superpowers/specs/2026-09-07-train-listing-page-design.md, Approach
-- B (the recommended one), and §0 point 5 for why an origin-keyed bucket
-- could not answer "which trains go to X".
--
-- WHY FLAT AND NOT A JSONB BUCKET, which is what this table was first
-- designed as. A destination bucket is enormous and its size is NOT
-- knowable in advance: London Waterloo buckets ~9,634 departure-bearing
-- calling points for one day, with the next several busiest destinations
-- within the same order of magnitude, so no per-destination cap is
-- defensible -- see
-- docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
-- (§1 for the measurement, §3 Approach C for this shape). Worse, the
-- publish fires once per CIF DELIVERY (roughly daily), not once per
-- 30-minute cycle, so any earliest-first cap freezes at delivery time and
-- is entirely in the past by the evening. Storing the rows instead of a
-- capped bucket removes the cap, and moves both the `now`-forward filter
-- and pagination to REQUEST time, where the clock is actually correct.
--
-- THE PRIMARY KEY IS THE POINT. (service_date, destination_crs, scheduled,
-- train_uid, origin_crs) is also the covering index for the only query
-- shape the read route needs, in exactly the order it needs it: equality
-- on the first two columns, a range scan on `scheduled`, and
-- (scheduled, train_uid, origin_crs) as a total order for a keyset cursor.
-- Waterloo therefore costs the same as Bootle Oriel Road: LIMIT + 1 index
-- entries touched, never the whole day. Do not add a second index without
-- a measured reason; do not reorder these columns.
--
-- `destination_crs` IS stored on every row here, unlike the bucket design
-- it replaces (where it was the key and therefore implicit). It is a real
-- column because it is a real filter predicate.
--
-- No `updated_at`: an ingest wholesale-replaces a whole service_date in one
-- transaction (DELETE by service_date, then one UNNEST bulk INSERT -- see
-- queries::upsert_schedule_destination_departures), so per-row write
-- timestamps would all be identical and carry no information the
-- service_date does not already carry.
--
-- RETENTION: REQUIRED, and pruned by the aggregator -- see
-- crates/aggregator/src/queries.rs's prune_schedule_destination_departures
-- and Config::schedule_destination_departures_retention_days (default 2).
-- This deliberately DIVERGES from schedule_network_departures, which has
-- no pruning job anywhere in this repo. That divergence is not an
-- oversight: the sibling's wholesale replace is scoped per (crs,
-- service_date) over ~2,500 CRS codes, so its steady-state size is
-- trivial, whereas THIS table accrues roughly 377,000 rows for every
-- service date it has ever seen and nothing would ever delete yesterday's.
-- Every read is scoped to today, computed server-side, so nothing reads a
-- past date and a 2-day window is ample -- 2 rather than 1 for safety
-- around the rail-day/midnight boundary and around a CIF delivery that
-- lands late. Design doc §7 Open Question 4, RE-resolved by the addendum's
-- §3 "Retention becomes required" (it reverses the original plan's
-- "retention: none, by parity" answer, which was sound only for the bucket
-- shape).
--
-- Partitioning by service_date is the standard mitigation if the once-daily
-- DELETE + bulk INSERT turns out to cost too much WAL or leave too much
-- bloat. It is NAMED here and deliberately not built -- addendum §7 item 5.
-- ---------------------------------------------------------------------

CREATE TABLE schedule_destination_departures (
    service_date    DATE NOT NULL,
    destination_crs TEXT NOT NULL,
    scheduled       TIME NOT NULL,
    train_uid       TEXT NOT NULL,
    origin_crs      TEXT NOT NULL,
    PRIMARY KEY (service_date, destination_crs, scheduled, train_uid, origin_crs)
);
```

- [ ] **Step 3: Apply the migration and verify it took**

The migration is applied automatically on `api`'s next startup via
`sqlx::migrate!`, or manually:

```bash
sqlx migrate run --source crates/api/migrations
psql "$DATABASE_URL" -c "SELECT to_regclass('public.schedule_destination_departures');"
```

Expected: prints `schedule_destination_departures`.

- [ ] **Step 4: Confirm nothing else broke**

Run: `cargo test -p api`
Expected: PASS — no existing test touches this table, so this is a pure
regression check that the migration file parses and the crate still builds.

- [ ] **Step 5: Commit**

```bash
git add crates/api/migrations/20260907130000_schedule_destination_departures.sql
git commit -m "Add schedule_destination_departures table for destination-first train search"
```

---

## Task 3: `schedule-query` — `departures_by_destination_crs`

> **KEPT, with exactly one correction, per the addendum's §5 row 3.** This
> function is shape-agnostic — it returns an uncapped, unsorted
> `HashMap<String, Vec<DestinationDeparture>>` and lets its caller decide
> everything else — so the signature, the `DestinationDeparture` record and
> **all seven tests in Step 1 survive verbatim**. The one change is in Step
> 4's doc comment: its closing sentence about the caller capping each bucket
> is false under the addendum and now cites it instead. The deliberate
> drop-vs-degrade asymmetry for an unresolved destination TIPLOC is
> unaffected and still right.
>
> Note that Task 4 will call this with `now = NaiveTime::MIN`, which makes
> the `if departure < now` filter a no-op. That is a *caller's* decision, as
> intended; do not remove the parameter or the filter — `departures_by_crs`
> keeps the same shape and a future caller may want a real boundary.

**Files:**
- Modify: `crates/schedule-query/src/records.rs` (add `DestinationDeparture`
  immediately after `ScheduleDeparture`, which ends at line 188)
- Modify: `crates/schedule-query/src/resolve.rs` (add
  `departures_by_destination_crs` immediately after `departures_by_crs`,
  which ends at line 216; add tests to the existing `#[cfg(test)] mod tests`)
- Modify: `crates/schedule-query/src/lib.rs` (add both names to the two
  existing `pub use` lists)

**Interfaces:**
- Consumes: existing `ScheduleIndex`, `resolve_for_date`, `normalize_tiploc`
  — all unchanged.
- Produces:
  `pub struct DestinationDeparture { pub uid: String, pub origin_crs: String,
  pub scheduled: chrono::NaiveTime }` (Serialize + Deserialize + Debug +
  Clone + PartialEq + Eq), and
  `pub fn departures_by_destination_crs(index: &ScheduleIndex, date:
  NaiveDate, now: NaiveTime, tiploc_to_crs: &HashMap<String, String>) ->
  HashMap<String, Vec<DestinationDeparture>>`. Both re-exported from
  `schedule_query`'s crate root. Consumed by Task 4.

- [ ] **Step 1: Write the failing tests**

Add these to the existing `#[cfg(test)] mod tests` block in
`crates/schedule-query/src/resolve.rs`, immediately after
`departures_by_crs_buckets_an_intermediate_calling_point_departure_under_its_own_crs`
(which ends at line 559). They reuse that module's existing `basic`,
`calling_point`, `calling_point_with_departure`, `tiploc_map`, `WEEKDAYS`
and `c11052_with_departures` helpers verbatim — nothing new to define.

```rust
    #[test]
    fn departures_by_destination_crs_buckets_an_origin_departure_under_the_schedules_destination() {
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point("CREWE  ", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(
            by_destination.len(),
            1,
            "the ONLY bucket key is the schedule's destination (CRE), never its origin"
        );
        assert!(
            !by_destination.contains_key("EUS"),
            "this function must not also bucket by origin -- that is departures_by_crs's job"
        );
        let crewe = &by_destination["CRE"];
        assert_eq!(crewe.len(), 1);
        assert_eq!(crewe[0].uid, "C11052");
        assert_eq!(crewe[0].origin_crs, "EUS");
        assert_eq!(
            crewe[0].scheduled,
            NaiveTime::from_hms_opt(8, 22, 0).unwrap()
        );
    }

    #[test]
    fn departures_by_destination_crs_buckets_every_departure_bearing_calling_point_under_one_destination() {
        // The load-bearing difference from departures_by_crs: a train from
        // EUSTON to MNCRPIC calling at CREWE contributes TWO entries to the
        // SAME (MAN) bucket -- "next train to Manchester from anywhere"
        // must find it whether the searcher is at Euston or at Crewe. This
        // is also exactly why this bucket's cardinality needed its own
        // sizing pass (Task 1) rather than reusing the origin-keyed cap.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
                calling_point("MNCRPIC", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE"), ("MNCRPIC", "MAN")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(by_destination.len(), 1, "one destination, one bucket");
        let manchester = &by_destination["MAN"];
        assert_eq!(manchester.len(), 2);
        let mut origins: Vec<&str> = manchester.iter().map(|d| d.origin_crs.as_str()).collect();
        origins.sort();
        assert_eq!(origins, vec!["CRE", "EUS"]);
    }

    #[test]
    fn departures_by_destination_crs_excludes_a_departure_already_before_now() {
        // Same `now`-forward posture as departures_by_crs (resolve.rs:193):
        // the 08:22 EUSTON departure is gone by 10:00, but the 10:05 CREWE
        // one is still ahead -- the bucket keeps only the latter.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
                calling_point("MNCRPIC", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE"), ("MNCRPIC", "MAN")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(by_destination["MAN"].len(), 1);
        assert_eq!(by_destination["MAN"][0].origin_crs, "CRE");
    }

    #[test]
    fn departures_by_destination_crs_excludes_a_cancelled_schedule_even_though_its_time_has_not_passed() {
        // Real UID/STP/date-range/days values (a base P pattern plus a real
        // STP=C override on 2026-08-31), reusing this module's own
        // c11052_with_departures fixture and its Bank Holiday cross-check.
        let index = ScheduleIndex::build(c11052_with_departures());
        let date = NaiveDate::from_ymd_opt(2026, 8, 31).unwrap(); // the cancelled date
        let now = NaiveTime::from_hms_opt(0, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);
        assert!(
            by_destination.is_empty(),
            "the STP=C override must suppress this date's bucket entirely"
        );
    }

    #[test]
    fn departures_by_destination_crs_drops_a_schedule_whose_destination_tiploc_is_unresolved() {
        // The asymmetry with departures_by_crs, and it is deliberate: THERE,
        // an unresolved destination degrades to `destination_crs: None` and
        // the row is still returned under its own origin. HERE the
        // destination IS the bucket key, so there is no honest bucket to
        // file this schedule under -- it is dropped entirely rather than
        // guessed at or filed under a fabricated key.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point("CREWE  ", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS")]); // CREWE deliberately absent

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);
        assert!(
            by_destination.is_empty(),
            "an unresolved DESTINATION tiploc drops the whole schedule -- there is no bucket key"
        );
    }

    #[test]
    fn departures_by_destination_crs_drops_only_the_calling_point_whose_own_tiploc_is_unresolved() {
        // Complementary to the test above: an unresolved INTERMEDIATE
        // tiploc drops just that one entry, not the schedule -- the
        // destination bucket still exists and still holds the resolvable
        // calling points. Same "drop, never fabricate" rule as
        // departures_by_crs (resolve.rs:196-198), applied per entry.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point_with_departure("CREWE  ", CallingPointKind::Intermediate, "10:05"),
                calling_point("MNCRPIC", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        // CREWE deliberately absent; EUSTON and MNCRPIC both resolve.
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("MNCRPIC", "MAN")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);

        assert_eq!(by_destination["MAN"].len(), 1);
        assert_eq!(by_destination["MAN"][0].origin_crs, "EUS");
    }

    #[test]
    fn departures_by_destination_crs_never_buckets_the_terminating_calling_point_itself() {
        // A Terminate calling point has no booked_departure by
        // construction (CallingPointKind::Terminate's own doc), so a train
        // must never appear as "departing from X" in X's own arrivals
        // bucket. Guards against a future refactor that starts reading
        // booked_arrival here.
        let raw = vec![RawSchedule {
            basic: basic(
                "C11052",
                StpIndicator::Permanent,
                "2026-05-18",
                "2026-12-11",
                WEEKDAYS,
            ),
            calling_points: vec![
                calling_point_with_departure("EUSTON ", CallingPointKind::Origin, "08:22"),
                calling_point("CREWE  ", CallingPointKind::Terminate),
            ],
        }];
        let index = ScheduleIndex::build(raw);
        let date = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
        let now = NaiveTime::from_hms_opt(8, 0, 0).unwrap();
        let tiploc_to_crs = tiploc_map(&[("EUSTON", "EUS"), ("CREWE", "CRE")]);

        let by_destination = departures_by_destination_crs(&index, date, now, &tiploc_to_crs);
        assert_eq!(by_destination["CRE"].len(), 1);
        assert_eq!(
            by_destination["CRE"][0].origin_crs, "EUS",
            "CRE must not appear as its own bucket's origin"
        );
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p schedule-query departures_by_destination_crs`
Expected: FAIL with a compile error — `departures_by_destination_crs` is not
defined and `DestinationDeparture` does not exist.

- [ ] **Step 3: Add the `DestinationDeparture` record**

In `crates/schedule-query/src/records.rs`, immediately after the
`ScheduleDeparture` struct (which ends at line 188):

```rust
/// One departure-bearing calling point of a schedule that TERMINATES at
/// some destination CRS, as bucketed by
/// [`crate::resolve::departures_by_destination_crs`]. The destination CRS
/// itself is deliberately absent from this struct: it is the bucket key
/// (identical for every entry in a bucket), exactly as the origin CRS is
/// the bucket key for [`ScheduleDeparture`]/`departures_by_crs`.
///
/// `origin_crs` means "the station this train departs FROM", which is the
/// calling point's own CRS -- an `Origin` calling point for the first
/// entry, an `Intermediate` one for every later entry of the same
/// schedule. It is NOT necessarily the schedule's own first station, and
/// is deliberately not the same concept as `trains.origin_crs` in
/// `crates/api`, which always is. A caller filtering by "origin" on the
/// train-search route (`GET /public/trains/search?origin=`) is asking
/// "departing from here", which is exactly this field.
///
/// `scheduled` is Europe/London LOCAL civil time, straight off the CIF
/// body, same as [`ScheduleDeparture::scheduled`] -- never UTC. See
/// `crates/schedule-reference/src/main.rs`'s `london_local_time_at` for
/// the one place that distinction is handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DestinationDeparture {
    pub uid: String,
    pub origin_crs: String,
    pub scheduled: NaiveTime,
}
```

- [ ] **Step 4: Add the grouping function**

In `crates/schedule-query/src/resolve.rs`, immediately after
`departures_by_crs` (which ends at line 216):

```rust
/// The destination-keyed sibling of [`departures_by_crs`]: every
/// non-cancelled, resolved schedule's `now`-forward, departure-bearing
/// calling points, bucketed by the CRS of that schedule's own TERMINATING
/// calling point rather than by each calling point's own CRS. Backs the
/// destination-first whole-network train search
/// (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B).
///
/// Runs against the SAME already-built, transient, per-cycle
/// [`ScheduleIndex`] as [`departures_by_crs`] -- one extra O(all UIDs)
/// resolve pass plus O(total calling points) bucketing per cycle, no second
/// parse and no resident index (that constraint is restated verbatim in the
/// design doc's §6).
///
/// Two deliberate asymmetries with [`departures_by_crs`], both about the
/// "drop, never fabricate" rule applied to a value that is now a bucket
/// KEY rather than a field:
///
/// * A schedule whose terminating TIPLOC has no `tiploc_to_crs` entry is
///   dropped **entirely** -- there is no honest bucket to file it under.
///   `departures_by_crs` can degrade the same case to
///   `destination_crs: None` because there the destination is only a
///   field; here it is the key.
/// * A calling point whose OWN TIPLOC has no `tiploc_to_crs` entry drops
///   just that entry, leaving the schedule's other entries in the bucket --
///   identical to `departures_by_crs`'s own per-calling-point drop.
///
/// **There is no cap, here or anywhere downstream.** This function returns
/// every matching calling point, unsorted, exactly like
/// `departures_by_crs`, and its caller
/// (`crates/schedule-reference/src/main.rs`'s
/// `schedule_destination_departures_rows`) merely flattens the result --
/// it does not sort, truncate, or bucket it. An earlier design capped each
/// bucket at a constant; that was measured and rejected, because the
/// busiest destination holds ~9,634 entries for one day and the next
/// several busiest are within the same order of magnitude, so no cap value
/// truncates honestly. See
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
/// (§1 for the measurement, §3 Approach C for what replaced it): the whole
/// day is published uncapped and the `now`-forward filter and pagination
/// happen at READ time instead, as an indexed range scan with a keyset
/// cursor. Do not reintroduce a cap in this function's caller.
pub fn departures_by_destination_crs(
    index: &ScheduleIndex,
    date: NaiveDate,
    now: NaiveTime,
    tiploc_to_crs: &HashMap<String, String>,
) -> HashMap<String, Vec<crate::records::DestinationDeparture>> {
    let mut by_destination: HashMap<String, Vec<crate::records::DestinationDeparture>> =
        HashMap::new();

    for uid in index.uids() {
        let Some(resolved) = index.schedule_for_uid(uid, date) else {
            continue;
        };
        if resolved.cancelled {
            continue;
        }
        let Some(destination_crs) = resolved
            .calling_points
            .last()
            .and_then(|last| tiploc_to_crs.get(normalize_tiploc(&last.tiploc)))
        else {
            continue;
        };
        for cp in &resolved.calling_points {
            let Some(departure) = cp.booked_departure else {
                continue;
            };
            if departure < now {
                continue;
            }
            let Some(origin_crs) = tiploc_to_crs.get(normalize_tiploc(&cp.tiploc)) else {
                continue;
            };
            by_destination
                .entry(destination_crs.clone())
                .or_default()
                .push(crate::records::DestinationDeparture {
                    uid: resolved.uid.clone(),
                    origin_crs: origin_crs.clone(),
                    scheduled: departure,
                });
        }
    }

    by_destination
}
```

- [ ] **Step 5: Export both names from the crate root**

In `crates/schedule-query/src/lib.rs`, extend the two existing `pub use`
lists (alphabetically, matching how they are already ordered):

```rust
pub use records::{
    BasicSchedule, CallingPoint, CallingPointKind, DestinationDeparture, LinePopulationEntry,
    RawSchedule, ScheduleDeparture, StpIndicator,
};
pub use resolve::{
    ResolvedSchedule, ScheduleIndex, departures_by_crs, departures_by_destination_crs, match_pin,
    resolve_for_date, schedules_touching,
};
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `cargo test -p schedule-query`
Expected: PASS — all seven new tests plus every pre-existing one (in
particular the six `departures_by_crs_*` tests must still pass **unchanged**;
this task adds a sibling function and must not have altered that one's
behaviour).

- [ ] **Step 7: Commit**

```bash
git add crates/schedule-query/src/records.rs crates/schedule-query/src/resolve.rs crates/schedule-query/src/lib.rs
git commit -m "Add departures_by_destination_crs, the destination-keyed grouping pass"
```

---

## Task 4: `schedule-reference` — publish the flat, uncapped, whole-day destination rows

> **REVISED per the addendum's §5 row 4, and NO LONGER GATED ON TASK 1.**
> The original version of this task was gated because its Step 3 hard-coded
> `MAX_DEPARTURES_PER_DESTINATION`. **That constant is deleted, not
> re-valued**, so nothing here depends on Task 1's number and this task can
> be dispatched immediately. Three things changed:
>
> 1. `schedule_destination_departures_rows` becomes a **flatten** — one JSON
>    object per departure, each carrying its own `destination_crs` — instead
>    of a sort + truncate + group-into-an-array.
> 2. The `now` argument passed to `departures_by_destination_crs` changes
>    from `london_local_time_now()` to `chrono::NaiveTime::MIN`: publish the
>    whole rail day, and filter `now`-forward at *read* time instead.
> 3. Its two cap-behaviour tests are deleted (they assert a cap that no
>    longer exists) and replaced by tests for the flatten.
>
> Steps 4, 5 and 6 — the `publish_cif_derived_products` wiring, the config
> field, and the Helm env var — are **unaffected** and carry through
> verbatim.
>
> Task 1's only remaining influence on this task is the choice between the
> single `post_batch` call written in Step 3 and the chunked loop in Task 1
> Step 3. **Default to the single call.** If Task 1 has been run and
> recorded "chunked POST", substitute that loop and also implement Task 6's
> conditional note.

**Files:**
- Modify: `crates/schedule-reference/src/config.rs` (add
  `schedule_destination_departures_url` after
  `schedule_network_departures_url`, which ends at line 55)
- Modify: `crates/schedule-reference/src/main.rs` (add the constant, the
  publish function and the pure row-shaping function after
  `schedule_network_departures_rows`, which ends at line 329; add one call
  in `publish_cif_derived_products`, currently ending at line 214; add tests
  to the existing `#[cfg(test)] mod poll_once_tests`)
- Modify: `charts/distant-signal/templates/schedulefeed-deployment.yaml:288-289`
  (add the new env var alongside `SCHEDULE_NETWORK_DEPARTURES_URL`)

**Interfaces:**
- Consumes: `schedule_query::departures_by_destination_crs(index, date, now,
  &tiploc_to_crs) -> HashMap<String, Vec<schedule_query::DestinationDeparture>>`
  (Task 3); existing `common::ingest::post_batch` and `Config`.
  **`london_local_time_now()` is no longer used by this task's publish** —
  it stays in the file for `publish_schedule_network_departures`, which is
  untouched.
- Produces: a batch-array POST to `config.schedule_destination_departures_url`
  whose elements are **one flat object per departure**:
  `{"service_date": "YYYY-MM-DD", "destination_crs": String, "scheduled":
  "HH:MM:SS", "train_uid": String, "origin_crs": String}` — the exact body
  shape Task 6's ingest route deserializes into
  `queries::ScheduleDestinationDeparturesRow` (Task 5), field-for-field.

> **Why `service_date` is on every row**, when the addendum's §3 sketch
> lists only four keys. The ingest side must know which service date a
> publish replaces, because its first statement is `DELETE FROM
> schedule_destination_departures WHERE service_date = $1` — and the route
> takes no path segment, no query parameter and no envelope to carry it
> (`common::ingest::post_batch` posts a bare array; changing that would
> change shared code used by four other publishers). Deriving it from the
> receiving service's own clock instead would reintroduce exactly the
> clock-coupling this design moved *out* of the publish. So it is a real
> per-row field, matching the table's own column.
>
> The honest cost: ~26 extra bytes per entry, so budget **~80 bytes per
> entry**, not the addendum's ~55. At the derived ~377,000 entries that is
> **~30MB**, still comfortably inside `DefaultBodyLimit::max(100 * 1024 *
> 1024)` (`crates/api/src/routes/mod.rs:86`) with ~3.3x headroom, and still
> under the ~60MB single-POST threshold. Task 1's Step 3 arithmetic uses
> the same ~80 figure.

- [ ] **Step 1: Write the failing tests**

Add to the existing `#[cfg(test)] mod poll_once_tests` in
`crates/schedule-reference/src/main.rs`, immediately after
`schedule_network_departures_rows_produces_one_row_per_crs_key` (which ends
at line 566):

```rust
    #[test]
    fn schedule_destination_departures_rows_produces_one_flat_row_per_departure_carrying_its_destination() {
        // The load-bearing shape assertion: this function FLATTENS. Two
        // destinations holding three departures between them produce THREE
        // rows, not two, and each row names its own destination rather than
        // inheriting it from a bucket key it no longer has.
        let mut by_destination = std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![
                schedule_query::DestinationDeparture {
                    uid: "U1".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap(),
                },
                schedule_query::DestinationDeparture {
                    uid: "U1".to_string(),
                    origin_crs: "CRE".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(10, 5, 0).unwrap(),
                },
            ],
        );
        by_destination.insert(
            "EDB".to_string(),
            vec![schedule_query::DestinationDeparture {
                uid: "U2".to_string(),
                origin_crs: "KGX".to_string(),
                scheduled: chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
            }],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let mut rows = schedule_destination_departures_rows(by_destination, today);
        // HashMap iteration order is unspecified; sort for a stable assert.
        rows.sort_by_key(|r| {
            (
                r["destination_crs"].as_str().unwrap().to_string(),
                r["scheduled"].as_str().unwrap().to_string(),
            )
        });

        assert_eq!(rows.len(), 3, "one row per DEPARTURE, not per destination");

        assert_eq!(
            rows[0],
            serde_json::json!({
                "service_date": "2026-09-07",
                "destination_crs": "EDB",
                "scheduled": "09:00:00",
                "train_uid": "U2",
                "origin_crs": "KGX",
            }),
            "exactly five keys, named exactly as the table's columns are"
        );

        // The same UID appears twice under MAN, once per departure-bearing
        // calling point -- that is the whole point of the grouping, and the
        // table's PK (which includes origin_crs) admits both.
        assert_eq!(rows[1]["destination_crs"], "MAN");
        assert_eq!(rows[1]["train_uid"], "U1");
        assert_eq!(rows[1]["origin_crs"], "EUS");
        assert_eq!(rows[1]["scheduled"], "08:22:00");
        assert_eq!(rows[2]["destination_crs"], "MAN");
        assert_eq!(rows[2]["train_uid"], "U1");
        assert_eq!(rows[2]["origin_crs"], "CRE");
        assert_eq!(rows[2]["scheduled"], "10:05:00");

        for row in &rows {
            assert!(
                row.get("departures").is_none(),
                "there is no nested departures array any more -- the shape is flat"
            );
            assert!(
                row.get("uid").is_none(),
                "the JSON key is train_uid (the column name), not DestinationDeparture::uid"
            );
        }
    }

    #[test]
    fn schedule_destination_departures_rows_is_uncapped_and_keeps_every_entry_of_a_huge_bucket() {
        // Regression guard against a reintroduced cap. The real busiest
        // destination holds ~9,634 entries for one day
        // (docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
        // §1.1), so 9,634 is used here deliberately rather than a round
        // number: if anyone ever reintroduces a truncate, this fails.
        let mut by_destination = std::collections::HashMap::new();
        let departures: Vec<schedule_query::DestinationDeparture> = (0..9_634u32)
            .map(|i| schedule_query::DestinationDeparture {
                uid: format!("U{i:05}"),
                origin_crs: if i % 2 == 0 { "EUS" } else { "CRE" }.to_string(),
                // Seconds since midnight, wrapped into a real 24h clock.
                scheduled: chrono::NaiveTime::from_num_seconds_from_midnight_opt(
                    i % 86_400,
                    0,
                )
                .unwrap(),
            })
            .collect();
        by_destination.insert("WAT".to_string(), departures);

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let rows = schedule_destination_departures_rows(by_destination, today);

        assert_eq!(
            rows.len(),
            9_634,
            "every entry must survive -- there is no cap, by design"
        );
    }

    #[test]
    fn schedule_destination_departures_rows_does_not_sort_and_does_not_need_to() {
        // Explicitly records that ordering is NOT this function's job any
        // more. The read route's ORDER BY rides the table's primary key
        // (queries::search_schedule_destination_departures), so a
        // publish-side sort would be pure wasted work over ~377,000 rows.
        // This test asserts the function is a faithful, order-preserving
        // flatten of each bucket rather than asserting a sort it must not do.
        let mut by_destination = std::collections::HashMap::new();
        by_destination.insert(
            "MAN".to_string(),
            vec![
                schedule_query::DestinationDeparture {
                    uid: "LATE".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(23, 0, 0).unwrap(),
                },
                schedule_query::DestinationDeparture {
                    uid: "EARLY".to_string(),
                    origin_crs: "EUS".to_string(),
                    scheduled: chrono::NaiveTime::from_hms_opt(1, 0, 0).unwrap(),
                },
            ],
        );

        let today = chrono::NaiveDate::from_ymd_opt(2026, 9, 7).unwrap();
        let rows = schedule_destination_departures_rows(by_destination, today);

        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0]["train_uid"], "LATE",
            "input order within a bucket is preserved verbatim; no sort happens here"
        );
        assert_eq!(rows[1]["train_uid"], "EARLY");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p schedule-reference schedule_destination_departures_rows`
Expected: FAIL with a compile error — `schedule_destination_departures_rows`
does not exist yet.

- [ ] **Step 3: Add the pure row-shaping function — and NO constant**

In `crates/schedule-reference/src/main.rs`, immediately after
`schedule_network_departures_rows` (which ends at line 329).

> **Do not add a `MAX_DEPARTURES_PER_DESTINATION`.** An earlier draft of
> this plan did, at 200; the measurement in the addendum's §1.1 killed it.
> `MAX_DEPARTURES_PER_STATION = 10` a few lines above belongs to
> `publish_schedule_network_departures` and is untouched — do not "restore
> symmetry" by adding a destination-side twin.

```rust
/// Pure JSON-shaping logic, split out of
/// `publish_schedule_destination_departures` purely so it is unit-testable
/// without a mock HTTP server -- same convention as
/// `schedule_network_departures_rows` directly above.
///
/// **A flatten, not a grouping.** Its sibling above emits one row per CRS
/// key with a capped, sorted `departures` array inside it; this one emits
/// one row per DEPARTURE, each carrying its own `destination_crs`, and
/// there is no array, no sort and no cap anywhere in it. The three
/// differences all have the same cause:
///
/// * **No cap**, because no cap value is defensible. London Waterloo
///   buckets ~9,634 departure-bearing calling points for a single day and
///   the next several busiest destinations are within the same order of
///   magnitude, so any cap truncates precisely the destinations a
///   whole-network destination search exists to serve. Worse, this publish
///   fires once per CIF DELIVERY (roughly daily), not once per 30-minute
///   cycle, so an earliest-first cap freezes at delivery time and is
///   entirely in the past by the evening. See
///   docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
///   §1 and §3.
/// * **No sort**, because ordering is the read side's job now:
///   `queries::search_schedule_destination_departures`'s `ORDER BY
///   scheduled, train_uid, origin_crs` rides the destination table's own
///   primary key. Sorting ~377,000 rows here would be wasted work.
/// * **One row per departure**, because the destination is no longer a
///   bucket key -- it is a column, and a filter predicate, on a flat table.
///
/// `service_date` is emitted on every row, unlike the four-key sketch in
/// the addendum's §3, because the ingest handler's first statement is a
/// `DELETE ... WHERE service_date = $1` and `common::ingest::post_batch`
/// posts a bare array with nowhere else to carry the day. Budget ~80 bytes
/// per entry when sizing the POST, not ~55.
fn schedule_destination_departures_rows(
    mut by_destination: std::collections::HashMap<
        String,
        Vec<schedule_query::DestinationDeparture>,
    >,
    today: chrono::NaiveDate,
) -> Vec<serde_json::Value> {
    by_destination
        .drain()
        .flat_map(|(destination_crs, departures)| {
            departures.into_iter().map(move |d| {
                serde_json::json!({
                    "service_date": today,
                    "destination_crs": destination_crs,
                    "scheduled": d.scheduled,
                    "train_uid": d.uid,
                    "origin_crs": d.origin_crs,
                })
            })
        })
        .collect()
}

/// The destination-keyed sibling of `publish_schedule_network_departures`
/// directly above: same one-batch-array POST shape, same `tiploc_to_crs`
/// map built from this cycle's already-resolved `stanox_crs_records`, same
/// log-and-continue error posture (a failed POST just means this delivery's
/// grouping is discarded and rebuilt when the next one lands). See
/// docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B, as revised by
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md,
/// Approach C.
///
/// **Two deliberate differences from the sibling, both easy to "fix" back
/// by mistake:**
///
/// 1. `now` is `chrono::NaiveTime::MIN`, NOT `london_local_time_now()`.
///    That is not an oversight and it is not a placeholder -- it publishes
///    the WHOLE rail day, on purpose. The sibling's publish-time
///    `now`-forward filter is evaluated exactly once per CIF delivery
///    (roughly daily -- `poll_once` returns early unless the delivery
///    directory changed, `main.rs:101-107`), so whatever the clock happened
///    to read when the delivery landed becomes the boundary for the rest of
///    the day. For a next-10-per-station board that is a tolerable
///    staleness; for a destination search it silently empties the busiest
///    destinations by evening. So this product publishes everything and
///    `GET /public/trains/search` applies `scheduled >= now` at REQUEST
///    time, where the clock is actually correct. If you change this back to
///    `london_local_time_now()`, you reintroduce that bug. See the
///    addendum's §1.3.
/// 2. The rows are flat and uncapped (see
///    `schedule_destination_departures_rows`), so this is a much larger
///    body than the sibling's: ~377,000 objects, ~30MB. That is inside
///    `DefaultBodyLimit::max(100 * 1024 * 1024)`
///    (`crates/api/src/routes/mod.rs:86`) with ~3.3x headroom. If a future
///    measurement pushes it past ~60MB, chunk it with
///    `for chunk in rows.chunks(50_000)` and teach the ingest handler
///    "the first chunk clears the day" -- addendum §3's documented
///    fallback, and Task 1 Step 3 of this plan.
async fn publish_schedule_destination_departures(
    client: &Client,
    config: &Config,
    index: &schedule_query::ScheduleIndex,
    today: chrono::NaiveDate,
    stanox_crs_records: &[common::StanoxCrsRecord],
    internal_oauth: &common::oauth_client::OAuthTokenCache,
) {
    let tiploc_to_crs: std::collections::HashMap<String, String> = stanox_crs_records
        .iter()
        .map(|r| {
            (
                schedule_query::normalize_tiploc(&r.tiploc).to_string(),
                r.crs.clone(),
            )
        })
        .collect();
    // Midnight, i.e. no publish-time `now`-forward filter at all -- see this
    // function's own doc comment, point 1. Deliberate; do not "fix".
    let now = chrono::NaiveTime::MIN;

    let by_destination =
        schedule_query::departures_by_destination_crs(index, today, now, &tiploc_to_crs);
    let rows = schedule_destination_departures_rows(by_destination, today);

    if let Err(err) = common::ingest::post_batch(
        client,
        &config.schedule_destination_departures_url,
        internal_oauth,
        &rows,
        "schedule-derived destination departures rows",
    )
    .await
    {
        tracing::error!(error = ?err, "failed to publish schedule-derived destination departures; will retry next cycle");
    }
}
```

- [ ] **Step 4: Wire the publish into the existing shared cycle**

In `publish_cif_derived_products` (`crates/schedule-reference/src/main.rs`,
currently ending at line 214), add the third publish call at the end of the
function body, after the existing `publish_schedule_network_departures` call:

```rust
    publish_schedule_line_population(client, config, &index, today, internal_oauth).await;
    publish_schedule_network_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
    // Third CIF-derived product off the SAME one-per-cycle ScheduleIndex
    // and the SAME `today` -- the design doc's Approach B is explicit that
    // this must not trigger a second parse or a resident index.
    publish_schedule_destination_departures(
        client,
        config,
        &index,
        today,
        stanox_crs_records,
        internal_oauth,
    )
    .await;
```

- [ ] **Step 5: Add the config field**

In `crates/schedule-reference/src/config.rs`, immediately after
`schedule_network_departures_url` (which ends at line 55):

```rust
    /// The `api` crate's ingestion endpoint for this service's fourth
    /// responsibility: the destination-keyed, CIF-derived whole-network
    /// train-search publish. See
    /// docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
    /// Approach B. POST-only, no GET pair -- same shape as
    /// `schedule_network_departures_url` directly above, and reusing the
    /// same `internal_oauth_group_schedule_reference` writer credential.
    #[arg(
        long,
        env,
        default_value = "http://api:8080/private/schedule-destination-departures"
    )]
    pub schedule_destination_departures_url: String,
```

- [ ] **Step 6: Add the Helm env var**

In `charts/distant-signal/templates/schedulefeed-deployment.yaml`,
immediately after the existing `SCHEDULE_NETWORK_DEPARTURES_URL` block
(lines 288-289):

```yaml
            # Train-listing-page plan: this crate's fourth responsibility,
            # the destination-keyed sibling of the publish directly above.
            - name: SCHEDULE_DESTINATION_DEPARTURES_URL
              value: {{ printf "%s/private/schedule-destination-departures" (include "distant-signal.apiBaseUrl" .) | quote }}
```

- [ ] **Step 7: Run the tests to verify they pass**

Run: `cargo test -p schedule-reference`
Expected: PASS — all three new tests plus every pre-existing one (the two
`schedule_network_departures_rows_*` tests in particular must still pass
unchanged).

Then run the chart lint so the template edit is proven syntactically valid:

```bash
helm lint charts/distant-signal
```

Expected: `1 chart(s) linted, 0 chart(s) failed`.

- [ ] **Step 8: Commit**

```bash
git add crates/schedule-reference/src/main.rs crates/schedule-reference/src/config.rs charts/distant-signal/templates/schedulefeed-deployment.yaml
git commit -m "Publish CIF-derived destination-keyed departures from schedule-reference"
```

---

## Task 5: `api` data layer — bulk upsert and keyset-paginated indexed search

> **REWRITTEN per the addendum's §5 row 5 and its §3.** Everything in this
> task changed shape:
>
> - `ScheduleDestinationDeparturesRow` becomes **flat scalar fields**, with
>   **no `serde_json::Value` anywhere**.
> - The upsert becomes, in **one transaction**, a `DELETE` by `service_date`
>   plus **one `UNNEST` bulk `INSERT`** — not the sibling's per-row
>   `INSERT ... ON CONFLICT` loop, which would be ~377,000 round trips.
> - The search becomes the addendum's **indexed range scan with a keyset
>   cursor**, not a `LEFT JOIN LATERAL jsonb_array_elements`.
> - The 404-vs-`200 []` split is preserved by a **day-scoped existence
>   probe**, which deliberately changes its semantics — see the note below.
>
> Every test in this task is rewritten around the flat shape; none of the
> original coverage is dropped, and four tests are added (keyset paging,
> last-page cursor, the `now` boundary, and an empty-batch guard).

**Files:**
- Modify: `crates/api/src/data/queries.rs` (add the two structs, the row
  struct, the upsert and the search function immediately after
  `latest_schedule_network_departures`, which ends at line 897; add a new
  test module at the end of the file)

**Interfaces:**
- Consumes: table `schedule_destination_departures` (Task 2).
- Produces:
  - `pub struct ScheduleDestinationDeparturesRow { pub service_date:
    chrono::NaiveDate, pub destination_crs: String, pub scheduled:
    chrono::NaiveTime, pub train_uid: String, pub origin_crs: String }`
    (`Debug + Clone + Deserialize`) — consumed by Task 6's ingest handler as
    a `Json<Vec<…>>` body, and matching Task 4's published JSON
    field-for-field.
  - `pub struct DestinationDepartureCursor { pub scheduled:
    chrono::NaiveTime, pub train_uid: String, pub origin_crs: String }`
    (`Debug + Clone + PartialEq + Eq`) — the keyset cursor, consumed by
    Task 7, which encodes and decodes it on the wire.
  - `pub struct DestinationDeparturePage { pub departures:
    Vec<serde_json::Value>, pub next_cursor:
    Option<DestinationDepartureCursor> }` (`Debug + Clone`).
  - `pub async fn upsert_schedule_destination_departures(pool: &PgPool, rows:
    &[ScheduleDestinationDeparturesRow]) -> Result<u64>` — consumed by
    Task 6.
  - `pub async fn search_schedule_destination_departures(pool: &PgPool,
    destination_crs: &str, service_date: chrono::NaiveDate, scheduled_from:
    chrono::NaiveTime, origin_crs: Option<&str>, to_time:
    Option<chrono::NaiveTime>, after: Option<&DestinationDepartureCursor>,
    limit: i64) -> Result<Option<DestinationDeparturePage>>` — consumed by
    Task 7.

> **The three outcomes, and how the return type keeps them apart.** The
> original `Option<Vec<…>>` distinguished two facts; this shape
> distinguishes the same two, plus pagination:
>
> | Outcome | Return | Route's response |
> |---|---|---|
> | No CIF publish has landed for `service_date` at all | `Ok(None)` | `404` |
> | Day published, filters matched nothing | `Ok(Some(page))` with `page.departures.is_empty()` | `200 {"results": [], "nextCursor": null}` |
> | Day published, rows found | `Ok(Some(page))` with rows, `next_cursor` set iff more remain | `200 {"results": [...], "nextCursor": "…"}` |
>
> **This deliberately changes the 404's meaning**, and the change must be a
> conscious call rather than a side effect (the addendum's §3 and §7 item 3
> argue it, and this plan accepts it). Under the old bucket shape the probe
> was per-destination, so an unknown destination CRS 404'd. Under a flat
> table an empty result set is empty either way, so the probe is scoped to
> the **day**: `404` now means *"we do not have today's timetable"*, and an
> unknown or train-less destination CRS returns `200` with an empty
> `results` array — *"we have today's timetable and nothing goes there"*.
> Those are genuinely different answers and this is the more honest split;
> it does, however, diverge from `get_station_schedule_departures`'s
> behaviour, which is unchanged and stays as it is.

> **Why `departures` stays `Vec<serde_json::Value>` when the storage is now
> typed.** So that Task 7's `render::destination_departure_json` and both of
> its tests survive **verbatim**, exactly as the addendum's §5 row 7
> requires. This function builds each element with `json!()` from the typed
> query result, in the same `{"uid", "origin_crs", "scheduled":
> "HH:MM:SS"}` shape the render function already reads — so the storage
> change stops at this file's boundary and the render layer never learns
> about it. The keys are `uid`/`origin_crs`/`scheduled` (the render
> function's contract), not `train_uid`/`origin_crs`/`scheduled` (the
> table's columns); the one-line mapping is in the code below.

- [ ] **Step 1: Write the failing tests**

Add this new module at the very end of `crates/api/src/data/queries.rs`:

> **Two conventions this module follows that are easy to get wrong.**
>
> 1. The pool helper is named **`test_pool()`**, not `connect()` — that is
>    this file's own local convention, shared by its three existing
>    `*_query_tests` modules (`incident_query_tests`,
>    `schedule_feed_ingest_query_tests`, `stanox_crs_lookup_query_tests`),
>    as `stanox_crs_lookup_query_tests`' own doc comment states explicitly.
>    Task 6 and Task 7's modules live in `routes/*.rs` and use `connect()`,
>    matching *their* files. Follow the file you are in.
> 2. **Every test gets its own `service_date`**, and the dates are in
>    **2099**. This is load-bearing, not fussiness. The existence probe is
>    scoped to the *day*, not to the destination, so two tests sharing a
>    `service_date` would see each other's rows and the
>    "nothing published" test would spuriously pass through to
>    `Some(empty)`. A far-future year additionally guarantees no collision
>    with real published data in a shared development database — which
>    matters now in a way it did not under the per-destination bucket, for
>    exactly the same reason.

```rust
/// Tested at the query level rather than through a route harness -- same
/// posture as this file's other `*_query_tests` modules. The real SQL here
/// (an indexed range scan over the destination table's own primary key,
/// three optional predicates, a keyset cursor, and the day-scoped existence
/// probe that keeps "no publish today" and "published but nothing matched"
/// distinguishable) is exactly what needs live-database coverage;
/// `routes/trains.rs`'s handler is a thin parse/render wrapper over it.
///
/// Every test here owns a distinct `service_date` in 2099, because the
/// existence probe is day-scoped: sharing a date between tests would let
/// one test's fixture answer another's "is anything published?" question,
/// and a real service date could let PRODUCTION data answer it.
#[cfg(test)]
mod schedule_destination_departures_query_tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    async fn test_pool() -> PgPool {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        PgPoolOptions::new()
            .connect(&database_url)
            .await
            .expect("connect to postgres")
    }

    /// A distinct, far-future fixture date per test. See this module's own
    /// doc comment for why both properties matter.
    fn fixture_date(day: u32) -> chrono::NaiveDate {
        chrono::NaiveDate::from_ymd_opt(2099, 1, day).expect("valid fixture date")
    }

    fn time(h: u32, m: u32) -> chrono::NaiveTime {
        chrono::NaiveTime::from_hms_opt(h, m, 0).expect("valid fixture time")
    }

    /// Midnight -- the lower bound that admits everything, used wherever a
    /// test is not itself about the `now`-forward boundary.
    fn any_time() -> chrono::NaiveTime {
        chrono::NaiveTime::MIN
    }

    async fn delete_day(pool: &PgPool, service_date: chrono::NaiveDate) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(service_date)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_destination_departures rows");
    }

    fn row(
        service_date: chrono::NaiveDate,
        destination_crs: &str,
        scheduled: chrono::NaiveTime,
        train_uid: &str,
        origin_crs: &str,
    ) -> ScheduleDestinationDeparturesRow {
        ScheduleDestinationDeparturesRow {
            service_date,
            destination_crs: destination_crs.to_string(),
            scheduled,
            train_uid: train_uid.to_string(),
            origin_crs: origin_crs.to_string(),
        }
    }

    /// Three trains to ZRD from two origins at three times -- enough to
    /// discriminate the origin filter, the time bounds and the ordering
    /// independently. The flat-shape equivalent of the original plan's
    /// single three-element JSONB bucket.
    fn fixture_rows(service_date: chrono::NaiveDate) -> Vec<ScheduleDestinationDeparturesRow> {
        vec![
            row(service_date, "ZRD", time(8, 22), "C10001", "EUS"),
            row(service_date, "ZRD", time(10, 5), "C10002", "CRE"),
            row(service_date, "ZRD", time(18, 40), "C10003", "EUS"),
        ]
    }

    async fn seed(pool: &PgPool, service_date: chrono::NaiveDate) {
        delete_day(pool, service_date).await;
        upsert_schedule_destination_departures(pool, &fixture_rows(service_date))
            .await
            .expect("seed fixture rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_wholesale_replaces_the_whole_service_date() {
        // The flat-shape successor to the bucket table's
        // "wholesale-replaces an existing row for the same key" test. The
        // unit of replacement is now the DAY, not one (destination_crs,
        // service_date) key -- a fresh delivery's grouping supersedes the
        // prior one entirely, including destinations that vanished from it.
        let pool = test_pool().await;
        let date = fixture_date(1);
        delete_day(&pool, date).await;

        let first = vec![
            row(date, "ZRB", time(8, 0), "OLD1", "EUS"),
            row(date, "ZRC", time(9, 0), "OLD2", "CRE"),
        ];
        let inserted = upsert_schedule_destination_departures(&pool, &first)
            .await
            .expect("first upsert");
        assert_eq!(inserted, 2);

        // The second publish drops ZRC entirely and changes ZRB's row.
        let second = vec![row(date, "ZRB", time(9, 30), "NEW1", "CRE")];
        upsert_schedule_destination_departures(&pool, &second)
            .await
            .expect("second upsert");

        let stored: Vec<(String, chrono::NaiveTime, String)> = sqlx::query_as(
            "SELECT destination_crs, scheduled, train_uid \
             FROM schedule_destination_departures WHERE service_date = $1 \
             ORDER BY destination_crs",
        )
        .bind(date)
        .fetch_all(&pool)
        .await
        .expect("read back");

        assert_eq!(
            stored.len(),
            1,
            "a fresh publish wholesale-replaces the whole service_date, never merges into it"
        );
        assert_eq!(stored[0].0, "ZRB");
        assert_eq!(stored[0].1, time(9, 30));
        assert_eq!(stored[0].2, "NEW1");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn upsert_with_an_empty_batch_does_not_wipe_the_day() {
        // Guards the one way a DELETE-then-INSERT upsert can destroy real
        // data that a per-row ON CONFLICT loop never could: a publish that
        // produced no rows (a parse failure upstream, an empty grouping)
        // must be a no-op, NOT "delete today's timetable".
        let pool = test_pool().await;
        let date = fixture_date(2);
        seed(&pool, date).await;

        let affected = upsert_schedule_destination_departures(&pool, &[])
            .await
            .expect("empty upsert");
        assert_eq!(affected, 0);

        let (remaining,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM schedule_destination_departures WHERE service_date = $1",
        )
        .bind(date)
        .fetch_one(&pool)
        .await
        .expect("count");
        assert_eq!(
            remaining, 3,
            "an empty batch must leave the day untouched, never clear it"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_with_nothing_published_for_the_day_is_none_not_an_empty_page() {
        // The whole reason for the Option: `None` becomes a 404 ("no CIF
        // publish has landed for today at all"), `Some(page)` with no rows
        // becomes a `200` with an empty `results` array ("we have today's
        // timetable and your filters matched nothing"). These are different
        // facts and must never collapse.
        let pool = test_pool().await;
        let date = fixture_date(3);
        delete_day(&pool, date).await;

        let result = search_schedule_destination_departures(
            &pool,
            "ZRC",
            date,
            any_time(),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");
        assert!(result.is_none());
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_with_the_day_published_but_no_matching_rows_is_some_and_empty() {
        let pool = test_pool().await;
        let date = fixture_date(4);
        seed(&pool, date).await;

        // No fixture row has this origin.
        let page = search_schedule_destination_departures(
            &pool,
            "ZRD",
            date,
            any_time(),
            Some("ZZZ"),
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day IS published");
        assert!(
            page.departures.is_empty(),
            "a published-but-unmatched day is Some(empty), never None"
        );
        assert!(page.next_cursor.is_none());

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_with_an_unknown_destination_on_a_published_day_is_some_and_empty() {
        // The deliberate semantic change the flat shape brings, pinned by a
        // test so nobody "restores" the old behaviour by accident: an
        // unknown destination CRS on a day that IS published is a `200`
        // with no results, NOT a 404. 404 now means "no timetable for
        // today at all". See the addendum's §3 and §7 item 3.
        let pool = test_pool().await;
        let date = fixture_date(5);
        seed(&pool, date).await;

        let page = search_schedule_destination_departures(
            &pool,
            "ZRF",
            date,
            any_time(),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published, even though this destination has no trains");
        assert!(page.departures.is_empty());

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_with_no_filters_returns_every_row_earliest_first_in_the_render_shape() {
        let pool = test_pool().await;
        let date = fixture_date(6);
        seed(&pool, date).await;

        let page = search_schedule_destination_departures(
            &pool,
            "ZRD",
            date,
            any_time(),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 3);
        assert_eq!(
            page.departures[0],
            serde_json::json!({
                "uid": "C10001",
                "origin_crs": "EUS",
                "scheduled": "08:22:00",
            }),
            "the element shape is exactly what render::destination_departure_json reads: \
             `uid` (not `train_uid`), and `scheduled` as HH:MM:SS"
        );
        assert_eq!(page.departures[1]["uid"], "C10002");
        assert_eq!(page.departures[2]["uid"], "C10003");
        assert!(
            page.next_cursor.is_none(),
            "the whole day fitted in one page, so there is no next cursor"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_filters_by_origin_crs() {
        let pool = test_pool().await;
        let date = fixture_date(7);
        seed(&pool, date).await;

        let page = search_schedule_destination_departures(
            &pool,
            "ZRD",
            date,
            any_time(),
            Some("CRE"),
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C10002");
        assert_eq!(page.departures[0]["origin_crs"], "CRE");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_filters_by_an_inclusive_time_range() {
        // Inclusive at BOTH ends, and discriminating about it: 10:05 is the
        // exact lower bound here and must be returned, while 08:22 (below
        // it) and 18:40 (above the upper bound) must not.
        let pool = test_pool().await;
        let date = fixture_date(8);
        seed(&pool, date).await;

        let page = search_schedule_destination_departures(
            &pool,
            "ZRD",
            date,
            time(10, 5),
            None,
            Some(time(12, 0)),
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C10002");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_excludes_rows_before_the_now_boundary() {
        // New under this shape, and the reason the shape exists: the
        // `now`-forward filter is applied HERE, at read time, not at
        // publish time. `scheduled_from` is the route's own
        // `max(now, from)`. At 11:00 only the 18:40 train remains.
        let pool = test_pool().await;
        let date = fixture_date(9);
        seed(&pool, date).await;

        let page = search_schedule_destination_departures(
            &pool,
            "ZRD",
            date,
            time(11, 0),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 1);
        assert_eq!(page.departures[0]["uid"], "C10003");

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_is_scoped_to_the_requested_service_date_only() {
        // Proves the "always today, server-side" scoping: a stale day's
        // rows must never leak through, and must not even make the
        // existence probe say "published".
        let pool = test_pool().await;
        let date = fixture_date(10);
        let yesterday = date - chrono::Duration::days(1);
        delete_day(&pool, date).await;
        delete_day(&pool, yesterday).await;

        upsert_schedule_destination_departures(
            &pool,
            &[row(yesterday, "ZRD", time(8, 0), "STALE", "EUS")],
        )
        .await
        .expect("seed a stale day");

        let result = search_schedule_destination_departures(
            &pool,
            "ZRD",
            date,
            any_time(),
            None,
            None,
            None,
            100,
        )
        .await
        .expect("search");
        assert!(
            result.is_none(),
            "yesterday's rows must not answer today's query, nor satisfy today's existence probe"
        );

        delete_day(&pool, yesterday).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_applies_the_limit_and_returns_a_cursor_for_the_rest() {
        let pool = test_pool().await;
        let date = fixture_date(11);
        seed(&pool, date).await;

        let page = search_schedule_destination_departures(
            &pool,
            "ZRD",
            date,
            any_time(),
            None,
            None,
            None,
            2,
        )
        .await
        .expect("search")
        .expect("the day is published");

        assert_eq!(page.departures.len(), 2, "limit caps the page");
        assert_eq!(
            page.departures[0]["uid"], "C10001",
            "the limit keeps the EARLIEST rows -- ORDER BY runs before LIMIT"
        );
        assert_eq!(
            page.next_cursor,
            Some(DestinationDepartureCursor {
                scheduled: time(10, 5),
                train_uid: "C10002".to_string(),
                origin_crs: "CRE".to_string(),
            }),
            "the cursor is the LAST row of this page, so the next page starts strictly after it"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_keyset_cursor_pages_through_the_day_without_gaps_or_repeats() {
        // The load-bearing pagination test. Pages of 1 through the three
        // fixture rows: each page must yield exactly the next row, in
        // order, and the final page must report no further cursor rather
        // than handing back a cursor that would yield nothing.
        let pool = test_pool().await;
        let date = fixture_date(12);
        seed(&pool, date).await;

        let mut seen: Vec<String> = Vec::new();
        let mut cursor: Option<DestinationDepartureCursor> = None;
        for _ in 0..5 {
            let page = search_schedule_destination_departures(
                &pool,
                "ZRD",
                date,
                any_time(),
                None,
                None,
                cursor.as_ref(),
                1,
            )
            .await
            .expect("search")
            .expect("the day is published");

            for departure in &page.departures {
                seen.push(departure["uid"].as_str().unwrap().to_string());
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        assert_eq!(
            seen,
            vec!["C10001", "C10002", "C10003"],
            "every row exactly once, in scheduled order, across three pages"
        );
        assert!(
            cursor.is_none(),
            "the last page must NOT hand back a cursor -- there is nothing after it"
        );

        delete_day(&pool, date).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn search_cursor_breaks_ties_on_train_uid_then_origin_crs() {
        // The reason the cursor is a three-part tuple and not just a time.
        // Three rows share one `scheduled`; a time-only cursor would either
        // skip two of them or loop forever. Paging one at a time must walk
        // all three exactly once, in (train_uid, origin_crs) order.
        let pool = test_pool().await;
        let date = fixture_date(13);
        delete_day(&pool, date).await;
        upsert_schedule_destination_departures(
            &pool,
            &[
                row(date, "ZRE", time(9, 0), "C20002", "CRE"),
                row(date, "ZRE", time(9, 0), "C20001", "EUS"),
                row(date, "ZRE", time(9, 0), "C20001", "CRE"),
            ],
        )
        .await
        .expect("seed tied rows");

        let mut seen: Vec<(String, String)> = Vec::new();
        let mut cursor: Option<DestinationDepartureCursor> = None;
        for _ in 0..5 {
            let page = search_schedule_destination_departures(
                &pool,
                "ZRE",
                date,
                any_time(),
                None,
                None,
                cursor.as_ref(),
                1,
            )
            .await
            .expect("search")
            .expect("the day is published");
            for departure in &page.departures {
                seen.push((
                    departure["uid"].as_str().unwrap().to_string(),
                    departure["origin_crs"].as_str().unwrap().to_string(),
                ));
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        assert_eq!(
            seen,
            vec![
                ("C20001".to_string(), "CRE".to_string()),
                ("C20001".to_string(), "EUS".to_string()),
                ("C20002".to_string(), "CRE".to_string()),
            ],
            "ties on `scheduled` are broken by train_uid then origin_crs, matching the PK's \
             own column order, and every tied row is visited exactly once"
        );

        delete_day(&pool, date).await;
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p api schedule_destination_departures -- --ignored`
Expected: FAIL with a compile error — none of
`ScheduleDestinationDeparturesRow`, `DestinationDepartureCursor`,
`DestinationDeparturePage`, `upsert_schedule_destination_departures` or
`search_schedule_destination_departures` is defined.

- [ ] **Step 3: Implement the two structs, the bulk upsert and the search**

In `crates/api/src/data/queries.rs`, immediately after
`latest_schedule_network_departures` (which ends at line 897):

```rust
/// One `POST /private/schedule-destination-departures` batch element -- one
/// DEPARTURE, not one destination bucket. Query-scoped, deserialized
/// straight off the request body by
/// `routes::ingest::post_schedule_destination_departures`. Defined here
/// (the data layer), not in `routes/ingest.rs`, so the data layer never
/// depends on a route-layer type -- same direction as every other
/// dependency between these two files.
///
/// Deliberately NOT shaped like `ScheduleNetworkDeparturesRow` above, which
/// carries an opaque `serde_json::Value` bucket. Every field here is a flat
/// scalar mapping one-to-one onto a column of
/// `schedule_destination_departures`, because the destination product needs
/// to be FILTERED and PAGINATED in SQL rather than stored and relayed
/// whole. See
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
/// §3 for why the bucket shape could not work here.
#[derive(Debug, Clone, Deserialize)]
pub struct ScheduleDestinationDeparturesRow {
    pub service_date: chrono::NaiveDate,
    pub destination_crs: String,
    pub scheduled: chrono::NaiveTime,
    pub train_uid: String,
    pub origin_crs: String,
}

/// An opaque-to-the-caller position in one destination's ordered results:
/// the last row of the page just returned. The next page is everything
/// strictly after it under `ORDER BY scheduled, train_uid, origin_crs`.
///
/// All three components are needed, not just `scheduled`: many trains share
/// a departure minute, so a time-only cursor would either skip the rest of
/// a tied group or return it forever. The tuple is exactly the trailing
/// three columns of `schedule_destination_departures`' primary key, in the
/// same order, so the comparison rides the index instead of re-sorting.
///
/// `routes::trains` encodes this onto the wire and parses it back; nothing
/// outside that module should construct one from user input without going
/// through that parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DestinationDepartureCursor {
    pub scheduled: chrono::NaiveTime,
    pub train_uid: String,
    pub origin_crs: String,
}

/// One page of destination-search results.
///
/// `departures` elements are deliberately `serde_json::Value` in the
/// `{"uid", "origin_crs", "scheduled": "HH:MM:SS"}` shape, NOT the typed
/// row: that is the exact element shape
/// `crate::render::destination_departure_json` already reads, so the
/// storage layer's move from a JSONB bucket to a flat table stops at this
/// function and the render layer is untouched.
///
/// `next_cursor` is `Some` only when there is genuinely at least one more
/// row -- the query fetches `limit + 1` to know that, rather than handing
/// back a cursor that would yield an empty page.
#[derive(Debug, Clone)]
pub struct DestinationDeparturePage {
    pub departures: Vec<serde_json::Value>,
    pub next_cursor: Option<DestinationDepartureCursor>,
}

/// Replaces one CIF delivery's worth of per-destination departures.
///
/// **Deliberately NOT shaped like `upsert_schedule_network_departures`
/// above.** That one loops a single-row `INSERT ... ON CONFLICT` per row
/// inside a transaction, which is correct for its ~2,500 rows and would be
/// ~377,000 round trips here. This is instead, in ONE transaction:
///
/// 1. `DELETE FROM schedule_destination_departures WHERE service_date =
///    ANY(...)` over the batch's distinct service dates, then
/// 2. one multi-row `INSERT ... SELECT * FROM UNNEST(...)`.
///
/// The pair preserves the same "wholesale replace, never merged" posture
/// both existing CIF-derived products document, just at day granularity
/// instead of per-key. `UNNEST` follows this crate's own established batch
/// pattern -- see `crate::data::trains::find_or_create_trains_batch` and
/// `mark_trains_resolved_batch` for the identical
/// build-parallel-Vecs-then-bind style. Five bind parameters regardless of
/// row count, so the 65,535-parameter protocol ceiling is not in play.
///
/// **An empty `rows` is a no-op, and that is load-bearing.** A publish that
/// produced nothing (an upstream parse failure, a delivery with no
/// schedules) must not be allowed to delete a service date's real
/// timetable. The per-row `ON CONFLICT` loop it replaces could not have
/// this bug; a DELETE-then-INSERT can, so it is guarded and tested
/// (`upsert_with_an_empty_batch_does_not_wipe_the_day`).
///
/// `ON CONFLICT DO NOTHING` on the insert: the primary key covers all five
/// columns, so a conflict can only mean the publisher emitted a
/// byte-identical duplicate. Dropping it silently is strictly better than
/// failing a ~377,000-row batch over one pathological schedule. The return
/// value is therefore rows actually INSERTED, which may be under
/// `rows.len()` in that case.
pub async fn upsert_schedule_destination_departures(
    pool: &PgPool,
    rows: &[ScheduleDestinationDeparturesRow],
) -> Result<u64> {
    if rows.is_empty() {
        return Ok(0);
    }

    let service_dates: Vec<chrono::NaiveDate> = rows.iter().map(|r| r.service_date).collect();
    let destination_crs: Vec<&str> = rows.iter().map(|r| r.destination_crs.as_str()).collect();
    let scheduled: Vec<chrono::NaiveTime> = rows.iter().map(|r| r.scheduled).collect();
    let train_uids: Vec<&str> = rows.iter().map(|r| r.train_uid.as_str()).collect();
    let origin_crs: Vec<&str> = rows.iter().map(|r| r.origin_crs.as_str()).collect();

    // Normally exactly one date. Handled as a set anyway so a batch that
    // straddles a rail-day boundary replaces both days rather than half of
    // one -- and so the DELETE can never be wider than what is being
    // written.
    let mut distinct_dates = service_dates.clone();
    distinct_dates.sort_unstable();
    distinct_dates.dedup();

    let mut tx = pool.begin().await?;

    sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = ANY($1::date[])")
        .bind(&distinct_dates)
        .execute(&mut *tx)
        .await?;

    let result = sqlx::query(
        "INSERT INTO schedule_destination_departures \
            (service_date, destination_crs, scheduled, train_uid, origin_crs) \
         SELECT * FROM UNNEST($1::date[], $2::text[], $3::time[], $4::text[], $5::text[]) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&service_dates)
    .bind(&destination_crs)
    .bind(&scheduled)
    .bind(&train_uids)
    .bind(&origin_crs)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(result.rows_affected())
}

/// Cheap, day-scoped existence probe backing the 404-versus-`200 []` split.
///
/// **Scoped to the DAY, not to the destination**, and that is a deliberate
/// semantic change from the bucket shape this replaces. Under a flat table
/// an empty result set is empty whether the destination is unknown or the
/// timetable is missing, so the only honest thing left to probe is whether
/// today's CIF publish landed at all. Consequently `404` now means "we do
/// not have today's timetable" and an unknown or train-less destination CRS
/// returns an empty `200`. See
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
/// §3 and §7 item 3 -- this diverges from
/// `routes::departures::get_station_schedule_departures`' own split, which
/// is unchanged.
///
/// One indexed lookup: `service_date` is the primary key's leading column.
async fn schedule_destination_departures_published_for(
    pool: &PgPool,
    service_date: chrono::NaiveDate,
) -> Result<bool> {
    let probe: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM schedule_destination_departures WHERE service_date = $1 LIMIT 1",
    )
    .bind(service_date)
    .fetch_optional(pool)
    .await?;
    Ok(probe.is_some())
}

/// The destination-first train search's one read: a bounded index range
/// scan over `schedule_destination_departures`' primary key, with a keyset
/// cursor.
///
/// The `(service_date, destination_crs, scheduled, train_uid, origin_crs)`
/// primary key is also this query's covering index, in exactly the order it
/// is used: equality on the first two columns, a range on `scheduled`, and
/// the trailing three as the total order the cursor rides. The worst real
/// case -- London Waterloo, no origin filter, ~8,145 matching rows -- touches
/// `limit + 1` index entries, not 8,145, so a busy destination costs the
/// same as a quiet one. **Do not** replace the row-comparison cursor with
/// `OFFSET`: an offset re-scans everything it skips, which is precisely the
/// cost this shape exists to avoid.
///
/// `scheduled_from` is an INCLUSIVE lower bound and is the caller's
/// already-combined `max(now, from)`. There is only one lower bound
/// parameter, deliberately: `now` is not optional (a departure that has
/// already gone is not a search result) and a caller-supplied `from` can
/// only narrow further, never reach back past it. `to_time` is an
/// INCLUSIVE upper bound. Both are real `NaiveTime`s compared against a
/// real `TIME` column -- the old shape's lexicographic `"HH:MM:SS"` string
/// comparison is gone along with the JSONB.
///
/// `Ok(None)` means no CIF publish has landed for `service_date` at all
/// (the caller maps that to a `404`). `Ok(Some(page))` with an empty
/// `page.departures` means the day IS published and the filters matched
/// nothing (a `200` with an empty `results` array). The probe only runs
/// when the main query came back empty, so the common case is one round
/// trip, not two.
#[allow(clippy::too_many_arguments)]
pub async fn search_schedule_destination_departures(
    pool: &PgPool,
    destination_crs: &str,
    service_date: chrono::NaiveDate,
    scheduled_from: chrono::NaiveTime,
    origin_crs: Option<&str>,
    to_time: Option<chrono::NaiveTime>,
    after: Option<&DestinationDepartureCursor>,
    limit: i64,
) -> Result<Option<DestinationDeparturePage>> {
    // One extra row is fetched purely to learn whether a next page exists,
    // so `next_cursor` is never handed back for an empty page.
    let fetch = limit.saturating_add(1);

    let rows: Vec<(String, String, chrono::NaiveTime)> = sqlx::query_as(
        r#"
        SELECT train_uid, origin_crs, scheduled
        FROM schedule_destination_departures
        WHERE service_date = $1
          AND destination_crs = $2
          AND scheduled >= $3
          AND ($4::text IS NULL OR origin_crs = $4)
          AND ($5::time IS NULL OR scheduled <= $5)
          AND ($6::time IS NULL
               OR (scheduled, train_uid, origin_crs) > ($6, $7, $8))
        ORDER BY scheduled, train_uid, origin_crs
        LIMIT $9
        "#,
    )
    .bind(service_date)
    .bind(destination_crs)
    .bind(scheduled_from)
    .bind(origin_crs)
    .bind(to_time)
    .bind(after.map(|c| c.scheduled))
    .bind(after.map(|c| c.train_uid.as_str()))
    .bind(after.map(|c| c.origin_crs.as_str()))
    .bind(fetch)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        // Only now is the probe worth a round trip -- and it is the ONLY
        // thing that separates a 404 from an empty 200.
        if !schedule_destination_departures_published_for(pool, service_date).await? {
            return Ok(None);
        }
        return Ok(Some(DestinationDeparturePage {
            departures: Vec::new(),
            next_cursor: None,
        }));
    }

    let has_more = rows.len() as i64 > limit;
    let page_rows = if has_more {
        &rows[..limit as usize]
    } else {
        &rows[..]
    };

    let next_cursor = if has_more {
        page_rows
            .last()
            .map(|(train_uid, origin_crs, scheduled)| DestinationDepartureCursor {
                scheduled: *scheduled,
                train_uid: train_uid.clone(),
                origin_crs: origin_crs.clone(),
            })
    } else {
        None
    };

    // Rendered into the SAME element shape the JSONB bucket used to store,
    // so `render::destination_departure_json` needs no change: `uid` (not
    // `train_uid`), and `scheduled` as a fixed-width "HH:MM:SS" string.
    let departures = page_rows
        .iter()
        .map(|(train_uid, origin_crs, scheduled)| {
            serde_json::json!({
                "uid": train_uid,
                "origin_crs": origin_crs,
                "scheduled": scheduled.format("%H:%M:%S").to_string(),
            })
        })
        .collect();

    Ok(Some(DestinationDeparturePage {
        departures,
        next_cursor,
    }))
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api schedule_destination_departures -- --ignored --test-threads=1 --nocapture`
Expected: PASS — all twelve tests in the new module.

- [ ] **Step 5: Run the full crate suite to confirm no regression**

Run: `cargo test -p api`
Expected: PASS — the new tests are `#[ignore]`d so they do not run here,
matching every other live-database test in this file.

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/data/queries.rs
git commit -m "Add flat-table upsert and keyset-paginated search for schedule_destination_departures"
```

---

## Task 6: `api` ingest route — `POST /private/schedule-destination-departures`

> **KEPT ALMOST ENTIRELY, per the addendum's §5 row 6.** The route path, the
> method, the `internal_oauth_group_schedule_reference` authorization, the
> `UpsertResponse` shape and the `app.rs` registration are **all unchanged**.
> The only change is that the body type now follows Task 5's flat row
> struct, so this task's `db_tests` seed and assertions change with it.
>
> **Conditional, and only if Task 1 says so:** *IF Task 1's real measurement
> requires chunking, this handler must additionally treat the FIRST chunk of
> a publish as the one that clears the day* — i.e. only the first POST in a
> batch of chunked POSTs performs the
> `DELETE FROM schedule_destination_departures WHERE service_date = $1`, and
> subsequent chunks in the same publish only INSERT. *Implement this only if
> Task 1's controller-run result requires it; the default (a single POST)
> needs no such handling.* Concretely, that would mean a query parameter or
> header on the request distinguishing first-chunk from continuation, and a
> second data-layer entry point next to
> `upsert_schedule_destination_departures` that skips the `DELETE` — do not
> build either speculatively. Task 1 Step 3 records which path was taken.

**Files:**
- Modify: `crates/api/src/routes/ingest.rs` (add the route to `router()`,
  currently at lines 32-95, next to the existing
  `/schedule-network-departures` entry at lines 88-91; add the handler after
  `post_schedule_network_departures`, which ends at line 447; add a
  `db_tests` test next to the existing
  `post_schedule_network_departures_upserts_the_row` test at line 1114)
- Modify: `crates/api/src/app.rs` (add the route/method/group triple to
  `build_internal_oauth_routes`, immediately after the
  `/schedule-network-departures` entry at lines 231-235)

**Interfaces:**
- Consumes: `queries::upsert_schedule_destination_departures(pool, rows) ->
  Result<u64>` and `queries::ScheduleDestinationDeparturesRow` (Task 5);
  the existing `UpsertResponse` struct in this file.
- Produces: `POST /private/schedule-destination-departures`, accepting
  `Json<Vec<ScheduleDestinationDeparturesRow>>` and returning
  `Json(UpsertResponse { upserted })`. Authorized by the **existing**
  `internal_oauth_group_schedule_reference` credential — no new group, no
  new secret, no chart change beyond Task 4's env var. Called by Task 4's
  `publish_schedule_destination_departures`.

- [ ] **Step 1: Write the failing test**

Add to the existing `db_tests` module in `crates/api/src/routes/ingest.rs`,
immediately after `post_schedule_network_departures_upserts_the_row` and its
sibling (the module's `delete_fixture`-style helper for that table is at
lines 1094-1101 — this test brings its own, for the new table):

```rust
    /// Day-scoped, like the upsert itself -- the unit of replacement for
    /// this table is a whole `service_date`, not one destination's rows.
    /// The fixture date is in 2099 for the same reason Task 5's are: it
    /// must not collide with a real published day in a shared development
    /// database. See `queries::schedule_destination_departures_query_tests`'
    /// own module doc comment.
    async fn delete_destination_departures_fixture(pool: &PgPool, service_date: &str) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1::date")
            .bind(service_date)
            .execute(pool)
            .await
            .expect("cleanup fixture schedule_destination_departures rows");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                post_schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn post_schedule_destination_departures_upserts_the_rows() {
        let pool = connect().await;
        delete_destination_departures_fixture(&pool, "2099-02-01").await;

        let router: axum::Router = crate::app::Router::new()
            .merge(router())
            .with_state(test_app(pool.clone()));
        // Flat: one JSON object per DEPARTURE, exactly as
        // schedule-reference's `schedule_destination_departures_rows`
        // emits them (Task 4) and exactly as
        // `queries::ScheduleDestinationDeparturesRow` deserializes them.
        // Two rows, so "one row per departure" is actually discriminated.
        let body = serde_json::json!([
            {
                "service_date": "2099-02-01",
                "destination_crs": "ZRB",
                "scheduled": "08:22:00",
                "train_uid": "C10001",
                "origin_crs": "EUS"
            },
            {
                "service_date": "2099-02-01",
                "destination_crs": "ZRB",
                "scheduled": "10:05:00",
                "train_uid": "C10002",
                "origin_crs": "CRE"
            }
        ]);
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/schedule-destination-departures")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let response_body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&response_body).unwrap();
        assert_eq!(json["upserted"], 2);

        let stored: Vec<(String, chrono::NaiveTime, String, String)> = sqlx::query_as(
            "SELECT destination_crs, scheduled, train_uid, origin_crs \
             FROM schedule_destination_departures \
             WHERE service_date = '2099-02-01' \
             ORDER BY scheduled",
        )
        .fetch_all(&pool)
        .await
        .expect("read back the upserted rows");

        assert_eq!(stored.len(), 2, "one stored row per posted departure");
        assert_eq!(stored[0].0, "ZRB");
        assert_eq!(
            stored[0].1,
            chrono::NaiveTime::from_hms_opt(8, 22, 0).unwrap()
        );
        assert_eq!(stored[0].2, "C10001");
        assert_eq!(stored[0].3, "EUS");
        assert_eq!(stored[1].2, "C10002");
        assert_eq!(stored[1].3, "CRE");

        delete_destination_departures_fixture(&pool, "2099-02-01").await;
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `DATABASE_URL=... cargo test -p api post_schedule_destination_departures -- --ignored`
Expected: FAIL — the route is not registered, so the request 404s (the
assertion on `StatusCode::OK` fails), or the crate fails to compile because
`upsert_schedule_destination_departures` is referenced by a handler that
does not exist yet.

- [ ] **Step 3: Register the route**

In `crates/api/src/routes/ingest.rs`'s `router()`, immediately after the
existing `/schedule-network-departures` entry (lines 88-91):

```rust
        .route(
            "/schedule-destination-departures",
            axum::routing::post(post_schedule_destination_departures),
        )
```

Extend that file's existing import of query-layer types to include the new
row struct (the same `use crate::data::queries::{…}` line that already
brings in `ScheduleNetworkDeparturesRow`):

```rust
use crate::data::queries::{ScheduleDestinationDeparturesRow, ScheduleNetworkDeparturesRow};
```

- [ ] **Step 4: Write the handler**

Immediately after `post_schedule_network_departures` (which ends at line
447):

```rust
/// `crates/schedule-reference`'s per-DELIVERY batch of CIF-derived
/// per-DESTINATION departures -- the destination-keyed sibling of
/// `post_schedule_network_departures` directly above, and the write side of
/// the destination-first train search
/// (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
/// Approach B, as revised by
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md,
/// Approach C). POST only: no service reads this table back over HTTP --
/// `api` serves it straight off Postgres via
/// `routes::trains::get_trains_search`.
///
/// The body is FLAT -- one element per departure, ~377,000 of them, ~30MB
/// -- not one element per destination with an array inside it. The whole
/// batch replaces its service date in one transaction inside
/// `upsert_schedule_destination_departures`; this handler adds no logic of
/// its own beyond that call, deliberately.
async fn post_schedule_destination_departures(
    State(app): State<App>,
    Json(rows): Json<Vec<ScheduleDestinationDeparturesRow>>,
) -> Result<Json<UpsertResponse>, (StatusCode, String)> {
    let upserted = queries::upsert_schedule_destination_departures(&app.database, &rows)
        .await
        .map_err(internal_error)?;
    Ok(Json(UpsertResponse { upserted }))
}
```

- [ ] **Step 5: Authorize the route for `schedule-reference`'s existing credential**

In `crates/api/src/app.rs`'s `build_internal_oauth_routes`, immediately after
the `/schedule-network-departures` entry (lines 231-235):

```rust
        // POST-only, same as /schedule-network-departures directly above,
        // and reusing schedule-reference's EXISTING writer credential --
        // the same one /stanox-crs, /schedule-line-population and
        // /schedule-network-departures already use. A fourth product from
        // the same producer is not a fourth identity.
        (
            "/schedule-destination-departures",
            Method::POST,
            vec![config.internal_oauth_group_schedule_reference.clone()],
        ),
```

- [ ] **Step 6: Run the tests to verify they pass**

Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api post_schedule_destination_departures -- --ignored --test-threads=1`
Expected: PASS.

Then: `cargo test -p api`
Expected: PASS, no regressions — in particular
`build_internal_oauth_routes`' own existing tests (if any assert the table's
length or contents) must be updated to account for exactly one new entry,
never rewritten to assert something weaker.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/routes/ingest.rs crates/api/src/app.rs
git commit -m "Add POST /private/schedule-destination-departures ingest route"
```

---

## Task 7: `api` public route — `GET /public/trains/search`

> **CHANGED ADDITIVELY, per the addendum's §5 row 7.** Everything this route
> already did, it still does. **Surviving verbatim, do not touch:** the route
> path; the required `destination` and optional `origin`/`from`/`to`
> parameters; the camelCase wire shape
> `{uid, scheduled: "HH:MM", originCrs, destinationCrs}`;
> `render.rs`'s `destination_departure_json`; and **both** of its render
> tests (Steps 1 and 3 of this task are unchanged from the original plan).
>
> **Three additions:**
> 1. a `limit` query parameter — default 50, hard maximum 200, clamped
>    server-side;
> 2. an opaque `after` cursor query parameter;
> 3. consequently, the response body becomes the envelope
>    `{results: [...], nextCursor: string | null}` instead of a bare JSON
>    array. That envelope is the only change that ripples to the frontend
>    (Task 10).
>
> The 404 keeps its status code but changes its *meaning*, inherited from
> Task 5's day-scoped existence probe: it now means "no CIF publish has
> landed for today at all", and an unknown destination CRS returns a `200`
> with an empty `results` array. Task 5's Interfaces block argues that
> change; this task's tests pin it.

**Files:**
- Create: `crates/api/src/routes/trains.rs`
- Modify: `crates/api/src/routes/mod.rs:7-24` (add `pub mod trains;`),
  `crates/api/src/routes/mod.rs:48-63` (add `.merge(trains::router())` to
  `public_router()`)
- Modify: `crates/api/src/render.rs` (add `destination_departure_json`
  immediately after `schedule_departure_json`, which ends at line 181; add a
  test to the existing `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: `queries::search_schedule_destination_departures(pool,
  destination_crs, service_date, scheduled_from, origin_crs, to_time, after,
  limit) -> Result<Option<queries::DestinationDeparturePage>>`, plus
  `queries::DestinationDepartureCursor` (Task 5).
- Produces:
  - `pub(crate) fn destination_departure_json(d: &serde_json::Value,
    destination_crs: &str) -> serde_json::Value`, rendering
    `{uid, scheduled: "HH:MM", originCrs, destinationCrs}` — **unchanged
    from the original plan**.
  - `GET /public/trains/search?destination={CRS}&origin={CRS}&from=HH:MM&to=HH:MM&limit={1..200}&after={cursor}`
    — `destination` required, the other five optional. `200` with
    `{"results": [ … ], "nextCursor": string | null}`; `400` on any
    malformed parameter, including a malformed `after`; `404` when no CIF
    publish has landed for today at all. Unauthenticated, like every other
    read in `public_router()`. Consumed by Task 10's `TrainSearchForm` via
    the same-origin `/api/*` proxy.

> **Cursor encoding, decided here.** `after` is
> **base64url-without-padding of `"HH:MM:SS|train_uid|origin_crs"`**.
>
> - *Why base64url and not the bare delimited string:* it makes the value
>   visibly opaque, so a client is not tempted to construct or mutate one by
>   hand and thereby depend on the ordering key — which is an internal
>   implementation detail of the table's primary key and must stay free to
>   change. `URL_SAFE_NO_PAD` needs no percent-encoding in a query string.
> - *Why this crate can:* `base64 = "0.22"` is already a direct dependency
>   of `crates/api`, and `base64::engine::general_purpose::URL_SAFE_NO_PAD`
>   is this crate's established engine (`crates/api/src/auth.rs:122-123`,
>   `crates/api/src/auth/internal_oauth.rs:25-26`). No new dependency.
> - *Why not a signed or encrypted token:* there is nothing to protect. The
>   cursor names a public timetable row on a public, unauthenticated route;
>   tampering with one can only reposition the reader within data they may
>   already read in full.
> - *Validation, and what a malformed cursor does:* **`400`**, matching this
>   route's existing posture for every other malformed input (`normalize_crs`
>   and `normalize_time` both already 400 rather than silently ignoring, for
>   the reason stated in their doc comments — a dropped filter returns MORE
>   rows than asked for, which reads as a broken search). A cursor fails
>   validation if it is not valid base64url, is not valid UTF-8, does not
>   split into exactly three `|`-separated parts, or its first part does not
>   parse as `%H:%M:%S`. Silently ignoring one would restart the user at
>   page 1 while their UI appended it as page 2, duplicating every row.

- [ ] **Step 1: Write the failing render test**

Add to the existing `#[cfg(test)] mod tests` in
`crates/api/src/render.rs`, immediately after
`schedule_departure_json_maps_snake_case_to_camel_case_and_trims_seconds`:

```rust
    #[test]
    fn destination_departure_json_maps_snake_case_to_camel_case_and_reattaches_the_destination() {
        let raw = serde_json::json!({
            "uid": "C11052",
            "origin_crs": "EUS",
            "scheduled": "08:22:00",
        });
        let json = destination_departure_json(&raw, "MAN");
        assert_eq!(
            json,
            serde_json::json!({
                "uid": "C11052",
                "scheduled": "08:22",
                "originCrs": "EUS",
                "destinationCrs": "MAN",
            })
        );
        assert!(
            json.get("origin_crs").is_none(),
            "no stray snake_case field"
        );
    }

    #[test]
    fn destination_departure_json_renders_a_missing_field_as_null_rather_than_omitting_it() {
        // Same defensive posture as schedule_departure_json: the stored
        // blob is opaque JSONB written by another service, so a missing key
        // must produce an explicit null rather than a differently-shaped
        // object the frontend's own row type would silently mis-parse.
        let json = destination_departure_json(&serde_json::json!({}), "MAN");
        assert!(json["uid"].is_null());
        assert!(json["originCrs"].is_null());
        assert!(json["scheduled"].is_null());
        assert_eq!(json["destinationCrs"], "MAN");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p api destination_departure_json`
Expected: FAIL with a compile error — `destination_departure_json` is not
defined.

- [ ] **Step 3: Implement the render function**

In `crates/api/src/render.rs`, immediately after `schedule_departure_json`
(which ends at line 181):

```rust
/// One `GET /public/trains/search` result row. Sibling of
/// `schedule_departure_json` directly above, same "hand-built camelCase
/// over an opaque JSONB element" convention, with two differences:
///
/// * it carries `originCrs` (the calling point this train departs FROM),
///   which the origin-keyed sibling doesn't need because there the origin
///   is the URL path segment; and
/// * `destinationCrs` is supplied by the CALLER, not read out of `d` --
///   it is the route's own required query parameter and is identical for
///   every row of one response, so the search query does not bother to
///   project it back out of the table (see
///   `queries::search_schedule_destination_departures`, whose `SELECT`
///   lists only `train_uid`, `origin_crs`, `scheduled`).
///
/// `scheduled` is trimmed from the stored `"HH:MM:SS"` to `"HH:MM"`,
/// identical to `schedule_departure_json`, so both sources hand the
/// frontend the same time shape.
pub(crate) fn destination_departure_json(d: &Value, destination_crs: &str) -> Value {
    let scheduled = d
        .get("scheduled")
        .and_then(Value::as_str)
        .map(|s| s.chars().take(5).collect::<String>());
    json!({
        "uid": d.get("uid").cloned().unwrap_or(Value::Null),
        "scheduled": scheduled,
        "originCrs": d.get("origin_crs").cloned().unwrap_or(Value::Null),
        "destinationCrs": destination_crs,
    })
}
```

- [ ] **Step 4: Write the route module**

Create `crates/api/src/routes/trains.rs`:

```rust
//! `GET /public/trains/search` -- destination-first, whole-network,
//! CIF-SCHEDULE-derived train search. Backs the `/trains` listing page
//! (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
//! Approach B).
//!
//! Named `trains` (plural), deliberately distinct from this crate's
//! `routes::train` (singular), which serves the authenticated, per-train
//! `/Train/...` family. This module is a public, unauthenticated READ over
//! published timetable data and shares no state, auth model or types with
//! that one.
//!
//! Reads `schedule_destination_departures` directly
//! (`queries::search_schedule_destination_departures`) as a bounded index
//! range scan with a keyset cursor. This is a publish-then-poll read of a
//! table `schedule-reference` writes when a CIF delivery lands -- never a
//! synchronous call into that service, per the design doc's §6.
//!
//! **v1 filter set, and why it stops here.** `destination` is required;
//! `origin` and the `from`/`to` time range are optional. There is
//! deliberately NO operator filter: the CIF SCHEDULE feed's operator field
//! is parsed-but-undecoded everywhere in this codebase, so a CIF-derived row
//! has no operator to filter on at all (design doc §1.3/§6). There is
//! deliberately NO date parameter: like `get_station_schedule_departures`,
//! this is "always today, server-side" (design doc §6).
//!
//! **This route owns the `now`-forward boundary**, which is the whole point
//! of the storage shape behind it. The publish stores the entire rail day
//! uncapped (`crates/schedule-reference`'s
//! `publish_schedule_destination_departures` passes `NaiveTime::MIN`),
//! because it fires once per CIF delivery -- roughly daily -- so a
//! publish-time filter would freeze at whatever the clock read when the
//! delivery landed. Evaluating `now` here means a search at 18:00 is
//! correct at 18:00. See
//! docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
//! §1.3 and §3.
//!
//! **Pagination is a keyset cursor, not an offset.** `limit` bounds one
//! page; `after` carries the last row of the previous page. Both exist
//! because there is no cap anywhere else in this pipeline: a busy
//! destination genuinely has thousands of trains in a day, and the honest
//! way to show them is a page at a time rather than a silent truncation.

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::app::{App, Router};
use crate::data::queries;
use crate::data::queries::DestinationDepartureCursor;
use crate::render::destination_departure_json;

/// Page size when the caller does not ask for one.
///
/// 50 rather than the old flat 100: this is now a PAGE, not the whole
/// answer, so it is sized for a first screenful plus room to scroll, and
/// "Load more" covers the rest at no extra cost (the next page is another
/// `LIMIT`-bounded index range scan, not a re-scan).
const DEFAULT_SEARCH_LIMIT: i64 = 50;

/// Hard ceiling on one page, clamped server-side rather than rejected.
///
/// **Why a ceiling exists at all,** now that nothing else in this pipeline
/// caps anything: this is an unauthenticated, unmetered public route over a
/// table with ~377,000 rows per day. Without a ceiling, `?limit=1000000`
/// is a free full-table scan and a ~20MB response for any anonymous
/// caller. 200 is four default pages -- generous for any real client,
/// including one that wants to render a whole morning at once.
///
/// **Why clamp rather than 400:** an over-large `limit` is not a malformed
/// input, it is an over-eager one, and the honest answer is "here are 200,
/// with a cursor for the rest" rather than an error. That is the opposite
/// call from `normalize_time`/`normalize_crs`, which DO 400 -- because
/// there, silently dropping a filter would return MORE rows than the caller
/// asked for, whereas clamping a limit only ever returns fewer, with a
/// cursor saying so. A `limit` that is zero, negative or unparseable IS
/// malformed and does 400.
const MAX_SEARCH_LIMIT: i64 = 200;

#[derive(Debug, Deserialize)]
struct TrainSearchParams {
    /// Required. A 3-letter CRS code; the search is keyed on it.
    destination: String,
    /// Optional. Matches the calling point a train departs FROM, which for
    /// a mid-route result is an intermediate station, not the schedule's
    /// own first station -- see `schedule_query::DestinationDeparture`'s
    /// own doc comment.
    origin: Option<String>,
    /// Optional, `"HH:MM"`, inclusive lower bound on scheduled departure.
    /// Narrows the `now`-forward window; it can never widen it backwards
    /// (a train that has already departed is not a search result).
    from: Option<String>,
    /// Optional, `"HH:MM"`, inclusive upper bound.
    to: Option<String>,
    /// Optional page size, 1..=`MAX_SEARCH_LIMIT`, defaulting to
    /// `DEFAULT_SEARCH_LIMIT`. Values above the maximum are clamped, not
    /// rejected; zero, negative and unparseable values are a `400`.
    ///
    /// Typed `Option<String>` rather than `Option<i64>` deliberately: with
    /// `Option<i64>`, `?limit=abc` fails inside axum's `Query` extractor
    /// and produces its generic deserialization error, which names neither
    /// the field nor the expectation. Parsing it here keeps every 400 on
    /// this route self-describing, exactly as `from`/`to` already are.
    limit: Option<String>,
    /// Optional opaque keyset cursor from a previous response's
    /// `nextCursor`. See `decode_cursor`.
    after: Option<String>,
}

pub fn router() -> Router {
    Router::new().route("/trains/search", axum::routing::get(get_trains_search))
}

/// Parses a caller-supplied `"HH:MM"` into a real `NaiveTime`, which is
/// what the query now compares against a real `TIME` column. (Under the
/// JSONB bucket this returned a `"HH:MM:SS"` string for a lexicographic
/// comparison; there is no text comparison left to line up with.)
///
/// `Err` (a 400) rather than silently ignoring an unparseable value: a
/// dropped filter would return MORE trains than asked for, which reads as a
/// broken search rather than a rejected input.
fn normalize_time(label: &str, raw: &str) -> Result<chrono::NaiveTime, (StatusCode, String)> {
    chrono::NaiveTime::parse_from_str(raw, "%H:%M").map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            format!("{label} must be a time of day in HH:MM form"),
        )
    })
}

/// Validates and uppercases a CRS code. Rejecting rather than passing a
/// malformed value through matters here because a non-CRS `destination`
/// would otherwise be reported as an ordinary empty result, which
/// misreports a caller error as a data gap.
fn normalize_crs(label: &str, raw: &str) -> Result<String, (StatusCode, String)> {
    let trimmed = raw.trim();
    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("{label} must be a 3-letter CRS code"),
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Parses and bounds the page size. Over-large values are CLAMPED to
/// `MAX_SEARCH_LIMIT`; zero, negative and unparseable values are a `400`.
/// See `MAX_SEARCH_LIMIT`'s own doc comment for why those two inputs are
/// treated differently.
fn normalize_limit(raw: Option<&str>) -> Result<i64, (StatusCode, String)> {
    let Some(raw) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(DEFAULT_SEARCH_LIMIT);
    };
    let parsed: i64 = raw.parse().map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        )
    })?;
    if parsed < 1 {
        return Err((
            StatusCode::BAD_REQUEST,
            "limit must be a positive whole number".to_string(),
        ));
    }
    Ok(parsed.min(MAX_SEARCH_LIMIT))
}

/// Renders a keyset cursor for the wire: base64url-without-padding of
/// `"HH:MM:SS|train_uid|origin_crs"`.
///
/// Base64 makes the value visibly OPAQUE. That is the point of encoding it
/// at all -- the three components are the trailing columns of
/// `schedule_destination_departures`' primary key, an internal detail that
/// must stay free to change, and a bare readable string invites a client to
/// build one by hand and depend on it. It is not a security measure and
/// deliberately is not signed: the cursor names a public timetable row on
/// an unauthenticated route, so tampering can only reposition a reader
/// within data they may already read in full.
///
/// `URL_SAFE_NO_PAD` is this crate's established engine
/// (`crates/api/src/auth.rs:122-123`), and needs no percent-encoding in a
/// query string.
fn encode_cursor(cursor: &DestinationDepartureCursor) -> String {
    URL_SAFE_NO_PAD.encode(format!(
        "{}|{}|{}",
        cursor.scheduled.format("%H:%M:%S"),
        cursor.train_uid,
        cursor.origin_crs
    ))
}

/// Inverse of `encode_cursor`. A malformed cursor is a `400`, never a
/// silently-ignored one: ignoring it would restart the caller at page 1
/// while their UI appended the result as page 2, duplicating every row on
/// screen. That is the same reasoning `normalize_time` gives for rejecting
/// an unparseable time rather than dropping the filter.
///
/// `train_uid` and `origin_crs` are passed through as-is rather than
/// validated further: they are compared for ordering only, so a nonsense
/// value yields an empty page rather than anything unsafe, and the query is
/// parameterized.
fn decode_cursor(raw: &str) -> Result<DestinationDepartureCursor, (StatusCode, String)> {
    let invalid = || {
        (
            StatusCode::BAD_REQUEST,
            "after must be a cursor returned by a previous search".to_string(),
        )
    };
    let bytes = URL_SAFE_NO_PAD.decode(raw).map_err(|_| invalid())?;
    let decoded = String::from_utf8(bytes).map_err(|_| invalid())?;
    let parts: Vec<&str> = decoded.split('|').collect();
    let [scheduled, train_uid, origin_crs] = parts.as_slice() else {
        return Err(invalid());
    };
    let scheduled =
        chrono::NaiveTime::parse_from_str(scheduled, "%H:%M:%S").map_err(|_| invalid())?;
    Ok(DestinationDepartureCursor {
        scheduled,
        train_uid: (*train_uid).to_string(),
        origin_crs: (*origin_crs).to_string(),
    })
}

async fn get_trains_search(
    State(app): State<App>,
    Query(params): Query<TrainSearchParams>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let destination = normalize_crs("destination", &params.destination)?;
    let origin = params
        .origin
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_crs("origin", s))
        .transpose()?;
    let from_time = params
        .from
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("from", s))
        .transpose()?;
    let to_time = params
        .to
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(|s| normalize_time("to", s))
        .transpose()?;
    let limit = normalize_limit(params.limit.as_deref())?;
    let after = params
        .after
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(decode_cursor)
        .transpose()?;

    // "Always today, server-side" -- no date parameter exists on this route
    // by design (design doc §6). Same posture and same expression as
    // `routes::departures::get_station_schedule_departures`.
    let today = chrono::Utc::now().date_naive();

    // The `now`-forward boundary, evaluated HERE rather than at publish
    // time -- see this module's own doc comment. `from` can only narrow it
    // further, never reach back past it, so the effective lower bound is
    // the later of the two.
    //
    // Europe/London LOCAL time, not UTC, and that is load-bearing: the
    // stored `scheduled` values are London local civil time straight off
    // the CIF body (`schedule_query::DestinationDeparture::scheduled`'s own
    // doc comment says so explicitly), so comparing a UTC time-of-day
    // against them would be an hour wrong every British Summer Time.
    // `chrono_tz` is already a direct dependency of this crate and
    // `chrono_tz::Europe::London` is already used in
    // `crate::data::eta_blend` for the same reason -- no new dependency,
    // and no hardcoded offset.
    let now = chrono::Utc::now()
        .with_timezone(&chrono_tz::Europe::London)
        .time();
    let scheduled_from = match from_time {
        Some(from) => std::cmp::max(now, from),
        None => now,
    };

    let Some(page) = queries::search_schedule_destination_departures(
        &app.database,
        &destination,
        today,
        scheduled_from,
        origin.as_deref(),
        to_time,
        after.as_ref(),
        limit,
    )
    .await
    .map_err(internal_error)?
    else {
        // 404 vs an empty `results` array is a real distinction here, not
        // pedantry -- but note WHICH distinction it now draws. Under the
        // flat table the probe is day-scoped, so this 404 means "no CIF
        // publish has landed for today at all", and an unknown or
        // train-less destination CRS gets a `200` with no results instead.
        // See the addendum's §3 and §7 item 3, and Task 5's Interfaces
        // block. The frontend renders different copy for each.
        return Err((
            StatusCode::NOT_FOUND,
            "no CIF-derived schedule data has been published for today".to_string(),
        ));
    };

    // An envelope, not a bare array, because a bare array has nowhere to
    // carry `nextCursor`. camelCase and hand-built with `json!()`, like
    // every other response in this crate.
    Ok(Json(json!({
        "results": page
            .departures
            .iter()
            .map(|row| destination_departure_json(row, &destination))
            .collect::<Vec<Value>>(),
        "nextCursor": page.next_cursor.as_ref().map(encode_cursor),
    })))
}

fn internal_error(err: anyhow::Error) -> (StatusCode, String) {
    tracing::error!(error = ?err, "train search query failed");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "query failed".to_string(),
    )
}
```

- [ ] **Step 5: Wire the module into `routes/mod.rs`**

Add the module declaration alphabetically among the existing ones (after
`pub mod train;`, line 24):

```rust
pub mod train;
pub mod trains;
```

And add it to `public_router()`'s merge chain:

```rust
        .merge(departures::router())
        .merge(stanox_crs::router())
        .merge(trains::router())
}
```

- [ ] **Step 6: Write the route-level failing tests**

Add a `db_tests` module at the end of `crates/api/src/routes/trains.rs`. The
`test_app` helper is copied verbatim from
`crates/api/src/routes/departures.rs:162-219` (that module's own doc comment
establishes the colocated-per-file convention; copy it exactly, including
every inert placeholder field, rather than importing it):

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

    /// Copied from `routes::departures::db_tests::test_app`, per that
    /// module's own doc comment ("colocated per-file rather than shared,
    /// until a third file needs it too"). Every field is an inert
    /// placeholder except `database`, which the caller supplies -- this
    /// route touches nothing else on `App`.
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

    /// Day-scoped, because the route's own existence probe is. Under the
    /// flat table there is no per-destination row to delete, and leaving
    /// another destination's rows behind for today would make the 404 test
    /// silently pass through to a `200`.
    async fn delete_today(pool: &PgPool) {
        sqlx::query("DELETE FROM schedule_destination_departures WHERE service_date = $1")
            .bind(chrono::Utc::now().date_naive())
            .execute(pool)
            .await
            .expect("cleanup today's schedule_destination_departures rows");
    }

    /// One time in the past and two strictly in the future, relative to the
    /// route's own London-local `now`.
    ///
    /// Computed at runtime, deliberately. This route applies its
    /// `now`-forward filter at REQUEST time -- that is the entire point of
    /// the storage shape behind it -- so a fixed wall-clock fixture like
    /// "08:22" would pass in the morning and silently return nothing in the
    /// afternoon. Anything asserting on visible rows must therefore be
    /// relative.
    fn relative_times() -> (chrono::NaiveTime, chrono::NaiveTime, chrono::NaiveTime) {
        let now = chrono::Utc::now()
            .with_timezone(&chrono_tz::Europe::London)
            .time();
        let past = chrono::NaiveTime::MIN;
        let (soon, soon_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(30));
        let (later, later_wrapped) = now.overflowing_add_signed(chrono::Duration::minutes(60));
        assert!(
            soon_wrapped == 0 && later_wrapped == 0 && now > past,
            "these tests need at least an hour before midnight and a moment after it; \
             re-run outside 23:00-00:01 Europe/London"
        );
        (past, soon, later)
    }

    /// Seeds today with: one already-departed row (which the route must
    /// hide), and two future rows from two different origins (which it must
    /// show, earliest first). The flat-shape successor to the original
    /// plan's two-element JSONB bucket.
    async fn seed_today(pool: &PgPool, destination_crs: &str) {
        delete_today(pool).await;
        let today = chrono::Utc::now().date_naive();
        let (past, soon, later) = relative_times();
        for (scheduled, train_uid, origin_crs) in [
            (past, "C10000", "EUS"),
            (soon, "C10001", "EUS"),
            (later, "C10002", "CRE"),
        ] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(today)
            .bind(destination_crs)
            .bind(scheduled)
            .bind(train_uid)
            .bind(origin_crs)
            .execute(pool)
            .await
            .expect("seed fixture row");
        }
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

    /// `results` out of the envelope, asserting the envelope's own shape on
    /// the way through so every test that reads rows also proves the body
    /// is not a bare array.
    fn results(body: &str) -> Vec<Value> {
        let json: Value = serde_json::from_str(body).unwrap();
        assert!(
            json.is_object() && json.get("results").is_some() && json.get("nextCursor").is_some(),
            "the body is an envelope with exactly `results` and `nextCursor`: {json}"
        );
        json["results"].as_array().cloned().unwrap()
    }

    fn next_cursor(body: &str) -> Option<String> {
        let json: Value = serde_json::from_str(body).unwrap();
        json["nextCursor"].as_str().map(str::to_string)
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_missing_destination_is_a_400() {
        let pool = connect().await;
        let (status, _) = get(&pool, "/trains/search").await;
        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "destination is required -- axum's Query extractor rejects the missing field"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_destination_is_a_400_not_a_404() {
        // The discriminating case: a caller error must not be reported as
        // a data gap.
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?destination=NOTACRS").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("destination"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_time_is_a_400() {
        let pool = connect().await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&from=half+past+eight").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("from"), "400 body should name the field: {body}");
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_malformed_after_cursor_is_a_400() {
        // A malformed cursor must NOT be silently ignored: ignoring it
        // restarts the caller at page 1 while their UI appends the result
        // as page 2, duplicating every row on screen. Two shapes are
        // checked -- not-base64 at all, and valid base64 whose payload has
        // the wrong number of parts.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, body) = get(&pool, "/trains/search?destination=ZRB&after=!!!not-base64!!!").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("after"), "400 body should name the field: {body}");

        // base64url of "nonsense" -- decodes cleanly, but is not a cursor.
        let (status, _) = get(&pool, "/trains/search?destination=ZRB&after=bm9uc2Vuc2U").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_rejects_a_zero_or_unparseable_limit_but_clamps_an_over_large_one() {
        // The asymmetry `MAX_SEARCH_LIMIT`'s doc comment argues, pinned:
        // an over-large limit is over-eager (clamp, and say so with a
        // cursor), a zero or unparseable one is malformed (400).
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, _) = get(&pool, "/trains/search?destination=ZRB&limit=0").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&limit=lots").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(body.contains("limit"), "400 body should name the field: {body}");

        let (status, body) = get(&pool, "/trains/search?destination=ZRB&limit=99999").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "an over-large limit is clamped to MAX_SEARCH_LIMIT, never rejected"
        );
        assert_eq!(results(&body).len(), 2, "the fixture only has two future rows");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_nothing_published_for_today_is_a_404() {
        // NOTE the changed meaning of this 404, and the changed assertion
        // that follows from it. Under the flat table the existence probe is
        // scoped to the DAY, not the destination, so this says "no CIF
        // publish has landed for today at all" and no longer names a CRS.
        // The companion test below pins the other half of that split.
        //
        // This test needs today's table to be genuinely empty. Run it
        // against the local docker-compose database, not one a real
        // `schedule-reference` has published into.
        let pool = connect().await;
        delete_today(&pool).await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(
            body.contains("today"),
            "the 404 is about today's publish, not about the destination: {body}"
        );
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_unknown_destination_on_a_published_day_is_200_and_empty() {
        // The other half of the changed split, and the reason it is a
        // deliberate call rather than an accident: once today's timetable
        // IS published, "nothing goes to ZRF" is a real answer, not a
        // missing one. Do not "restore" this to a 404.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRF").await;
        assert_eq!(status, StatusCode::OK);
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_published_day_with_no_matches_is_200_with_an_empty_results_array() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB&origin=ZZZ").await;
        assert_eq!(
            status,
            StatusCode::OK,
            "published-but-unmatched is a 200 with an empty results array, never a 404"
        );
        assert!(results(&body).is_empty());
        assert!(next_cursor(&body).is_none());
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_renders_camel_case_rows_with_trimmed_time_and_the_destination_attached() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=zrb").await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        let (_, soon, _) = relative_times();

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["uid"], "C10001");
        assert_eq!(
            rows[0]["scheduled"],
            soon.format("%H:%M").to_string(),
            "seconds trimmed"
        );
        assert_eq!(rows[0]["originCrs"], "EUS");
        assert_eq!(
            rows[0]["destinationCrs"], "ZRB",
            "the lowercase query param is normalized and re-attached uppercase"
        );
        assert!(rows[0].get("origin_crs").is_none(), "no stray snake_case field");

        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_hides_a_departure_that_has_already_gone() {
        // The `now`-forward filter, which now lives HERE rather than at
        // publish time. The fixture's 00:00 row is published and matches
        // every other predicate; it must not be returned.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        let uids: Vec<&str> = results(&body)
            .iter()
            .map(|r| r["uid"].as_str().unwrap())
            .collect();
        assert!(
            !uids.contains(&"C10000"),
            "an already-departed row must not be returned: {uids:?}"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_applies_origin_and_time_filters_together() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (_, soon, later) = relative_times();
        let uri = format!(
            "/trains/search?destination=ZRB&origin=CRE&from={}&to={}",
            soon.format("%H:%M"),
            later.format("%H:%M")
        );
        let (status, body) = get(&pool, &uri).await;
        assert_eq!(status, StatusCode::OK);
        let rows = results(&body);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["uid"], "C10002");
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_returns_a_null_next_cursor_when_the_page_is_the_last_one() {
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;
        let (status, body) = get(&pool, "/trains/search?destination=ZRB").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(results(&body).len(), 2);
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            json["nextCursor"],
            Value::Null,
            "nextCursor is explicit JSON null on the last page, never omitted -- the frontend \
             checks it to decide whether to render Load more"
        );
        delete_today(&pool).await;
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                trains_search -- --ignored --test-threads=1`"]
    async fn trains_search_paginates_with_a_cursor_and_after_continues_from_it() {
        // The end-to-end pagination contract Task 10's "Load more" button
        // depends on: page 1 returns a cursor, feeding that cursor back as
        // `after` returns the NEXT row (not a repeat, not a restart), and
        // the final page reports no cursor.
        let pool = connect().await;
        seed_today(&pool, "ZRB").await;

        let (status, first) = get(&pool, "/trains/search?destination=ZRB&limit=1").await;
        assert_eq!(status, StatusCode::OK);
        let first_rows = results(&first);
        assert_eq!(first_rows.len(), 1);
        assert_eq!(first_rows[0]["uid"], "C10001");
        let cursor = next_cursor(&first).expect("a second page exists, so a cursor is returned");

        let (status, second) = get(
            &pool,
            &format!("/trains/search?destination=ZRB&limit=1&after={cursor}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let second_rows = results(&second);
        assert_eq!(second_rows.len(), 1);
        assert_eq!(
            second_rows[0]["uid"], "C10002",
            "`after` must continue from the cursor, not restart at page 1"
        );
        assert!(
            next_cursor(&second).is_none(),
            "the last page must not hand back a cursor"
        );

        delete_today(&pool).await;
    }
}
```

> **Step 6 note on `test_app`:** it is copied, not shared, on purpose —
> `routes::departures::db_tests` is a private `#[cfg(test)]` module, so it
> cannot be imported, and that module's own doc comment already establishes
> the convention ("colocated per-file rather than shared, until a third file
> needs it too"). The version written above is verbatim from
> `crates/api/src/routes/departures.rs:162-219`. If `ServiceArguments` has
> gained a field since, the compiler will say so; add it with the same inert
> placeholder value `departures.rs`'s copy uses, and do not delete a field to
> make it build.

- [ ] **Step 7: Run the tests to verify they pass**

Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p api trains_search destination_departure_json -- --ignored --test-threads=1`
Expected: PASS — twelve route tests plus the two render tests. (The two
render tests are not `#[ignore]`d; run them separately with
`cargo test -p api destination_departure_json` if the combined invocation's
`--ignored` flag skips them.)

`--test-threads=1` is not optional here, and for a new reason: several of
these tests assert on the contents of *today* in a table whose existence
probe is day-scoped, so they would see each other's fixtures if run
concurrently. Run them against the local docker-compose database, not one a
real `schedule-reference` has published into.

- [ ] **Step 8: Manually verify against a running local stack**

```bash
docker compose up -d --build api

# The envelope, and the two new parameters.
curl -s "http://localhost:8080/public/trains/search?destination=MAN&limit=2" | jq .

# Feed the cursor back in: the second page must not repeat the first.
CURSOR=$(curl -s "http://localhost:8080/public/trains/search?destination=MAN&limit=2" | jq -r .nextCursor)
curl -s "http://localhost:8080/public/trains/search?destination=MAN&limit=2&after=$CURSOR" | jq .

curl -s -o /dev/null -w "%{http_code}\n" "http://localhost:8080/public/trains/search"
curl -s -o /dev/null -w "%{http_code}\n" "http://localhost:8080/public/trains/search?destination=MAN&after=nonsense"
curl -s -o /dev/null -w "%{http_code}\n" "http://localhost:8080/public/trains/search?destination=MAN&limit=0"
```

Expected: the first returns `{"results": [...], "nextCursor": "…"}` (or a
`404` if no CIF publish has landed for today yet — either confirms the route
is reachable); the second returns a *different* two rows; the last three all
`400`. Note that `?destination=ZZZ` is now a `200` with an empty `results`
array rather than a `404`, once today's data is published — that is the
deliberate semantic change, not a regression.

- [ ] **Step 9: Commit**

```bash
git add crates/api/src/routes/trains.rs crates/api/src/routes/mod.rs crates/api/src/render.rs
git commit -m "Add GET /public/trains/search, the destination-first train search route"
```

---

## Task 8: Make `create_subscription_for_train` idempotent per `(user_id, trains_id)`

> **This task must land before Tasks 9-12.** Every CTA in this plan calls
> `POST /Train/by-uid/{uid}/{date}/track`, which calls this function. Today,
> a user who clicks "Track this train" twice — a repeat click, a back-navigate
> and re-click, a second tab — silently gets two independent
> `train_subscriptions` rows, two entries in `/track/mine`, and two
> notification streams for one train. See this plan's "Decisions this plan
> resolves", item 2, for the file:line evidence that this is the current,
> documented, test-asserted behaviour.

**Files:**
- Modify: `crates/api/src/data/train_tracking.rs:162-187`
  (`create_subscription_for_train`'s doc comment and body)
- Test: `crates/api/src/data/train_tracking.rs:3497-3556` (invert the
  existing `create_subscription_for_train_called_twice_creates_two_separate_subscriptions`
  test; add two more to the same `db_tests` module)

**Interfaces:**
- Consumes: nothing new.
- Produces: `pub async fn create_subscription_for_train(pool: &PgPool,
  trains_id: i64, user_id: &str) -> anyhow::Result<i64>` — **unchanged
  signature**, changed semantics: it now returns the caller's *existing*
  subscription id for that `trains_id` when one exists, instead of creating a
  second row. No caller's code changes; `routes::train::post_track_by_uid`
  (`crates/api/src/routes/train.rs:691-707`) keeps working verbatim and now
  returns the same `trackingId` on a repeat call.

**Design note this task must not silently drop:** the fix is
**application-level, in this one function**, and deliberately **not** a
`UNIQUE (user_id, trains_id)` index. Four unrelated code paths already do a
bare `UPDATE train_subscriptions SET trains_id = $2 WHERE id = $1`
(`crates/api/src/data/schedule_matching.rs:131`,
`crates/api/src/data/trust_event_backlog_match.rs:394` and `:504`,
`crates/api/src/data/train_tracking.rs:684`), and `create_pin`
(`train_tracking.rs:79-100`) has never deduplicated legacy CRS+time pins —
so a user with two legacy pins that later resolve to the same physical train
is reachable today, and a global unique index would turn each of those four
`UPDATE`s into a hard runtime failure inside schedule matching, backlog
matching, and live TRUST resolution. That is a much larger, separately
reviewable change than this plan's scope. Do not add the index.

- [ ] **Step 1: Invert the existing test and add two more**

In `crates/api/src/data/train_tracking.rs`'s `db_tests` module, replace the
whole of
`create_subscription_for_train_called_twice_creates_two_separate_subscriptions`
(lines 3497-3556, doc comment included) with:

```rust
    /// Explicit idempotency check. This test previously asserted the
    /// OPPOSITE -- that two calls produced two rows -- which was an honest
    /// record of the behaviour at the time, not a requirement. The
    /// train-listing-page plan turned that behaviour into a real user-facing
    /// bug (a "Track this train" button a user can click twice), so the
    /// function was fixed and this test inverted alongside it. See
    /// docs/superpowers/plans/2026-09-07-train-listing-page-implementation-plan.md
    /// Task 8, which also records why a `UNIQUE (user_id, trains_id)` index
    /// was rejected in favour of this in-function fix.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_called_twice_returns_the_same_subscription \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_called_twice_returns_the_same_subscription() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-TRACK-TWICE";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-TWICE-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        let first_tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("first create_subscription_for_train call");
        let second_tracking_id = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("second create_subscription_for_train call, same trains_id and user_id");

        assert_eq!(
            first_tracking_id, second_tracking_id,
            "a repeat call for the same (trains_id, user_id) must return the EXISTING \
             subscription, not create a second one"
        );

        let (row_count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM train_subscriptions WHERE trains_id = $1 AND user_id = $2",
        )
        .bind(trains_id)
        .bind(user_id)
        .fetch_one(&pool)
        .await
        .expect("count subscriptions for this (trains_id, user_id) pair");
        assert_eq!(row_count, 1, "exactly one row must exist after two calls");

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(first_tracking_id)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }

    /// Discriminating counterpart: idempotency is scoped to ONE user. Two
    /// different users tracking the same physical train is this endpoint's
    /// own headline scenario (the whole point of the shared `trains` table)
    /// and must still produce two independent subscriptions.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_is_not_shared_between_users \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_is_not_shared_between_users() {
        let pool = connect().await;
        let user_a = "TEST-NR-PRIMARY-SHARED-A";
        let user_b = "TEST-NR-PRIMARY-SHARED-B";
        seed_user(&pool, user_a).await;
        seed_user(&pool, user_b).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-SHARED-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        let a = create_subscription_for_train(&pool, trains_id, user_a)
            .await
            .expect("user A subscribes");
        let b = create_subscription_for_train(&pool, trains_id, user_b)
            .await
            .expect("user B subscribes to the same train");

        assert_ne!(
            a, b,
            "two users tracking one train must still get two independent subscriptions"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id IN ($1, $2)")
            .bind(a)
            .bind(b)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_a).await;
        cleanup_user(&pool, user_b).await;
    }

    /// Guards the one thing the existing-row-wins CTE could plausibly get
    /// wrong: the returned id must be the row that actually exists, usable
    /// as a real tracking id, not a stale/duplicated value. Reads the row
    /// back through the same ownership query every `/Train/{trackingId}`
    /// route uses.
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p api \
                create_subscription_for_train_repeat_call_returns_a_usable_tracking_id \
                -- --ignored --test-threads=1`"]
    async fn create_subscription_for_train_repeat_call_returns_a_usable_tracking_id() {
        let pool = connect().await;
        let user_id = "TEST-NR-PRIMARY-USABLE-ID";
        seed_user(&pool, user_id).await;

        let trains_id = crate::data::trains::find_or_create_train(
            &pool,
            "TEST-NR-PRIMARY-USABLE-UID",
            "2026-09-07".parse().unwrap(),
        )
        .await
        .expect("seed a trains row");

        create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("first call");
        let returned = create_subscription_for_train(&pool, trains_id, user_id)
            .await
            .expect("second call");

        let owner = tracked_train_owner(&pool, returned)
            .await
            .expect("ownership lookup");
        assert_eq!(
            owner,
            Some(user_id.to_string()),
            "the returned id must resolve to a real, caller-owned subscription"
        );

        sqlx::query("DELETE FROM train_subscriptions WHERE id = $1")
            .bind(returned)
            .execute(&pool)
            .await
            .ok();
        sqlx::query("DELETE FROM trains WHERE id = $1")
            .bind(trains_id)
            .execute(&pool)
            .await
            .ok();
        cleanup_user(&pool, user_id).await;
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run:
`DATABASE_URL=... cargo test -p api create_subscription_for_train -- --ignored --test-threads=1`
Expected: FAIL —
`create_subscription_for_train_called_twice_returns_the_same_subscription`
fails on `assert_eq!(first_tracking_id, second_tracking_id)` (they differ
today) and on `assert_eq!(row_count, 1)` (it is 2 today).
`create_subscription_for_train_is_not_shared_between_users` passes already —
that is expected and correct; it is a regression guard for the fix, not a
new requirement.

- [ ] **Step 3: Replace the function body and its doc comment**

In `crates/api/src/data/train_tracking.rs`, replace lines 162-187 (the
"NOT idempotent by `(trains_id, user_id)`" paragraph through the end of the
function) with:

```rust
/// **Idempotent by `(user_id, trains_id)`.** A second call for the same
/// user and the same shared train returns that user's EXISTING subscription
/// id rather than inserting another row -- so a user who clicks "Track this
/// train" twice (a repeat click, a back-navigate, a second tab) ends up
/// with one subscription, one `/track/mine` entry and one notification
/// stream, and is navigated to the same `/train/by-id/{trackingId}` both
/// times.
///
/// Scoped to ONE user: two different users tracking the same physical train
/// still get two independent subscriptions, which is this endpoint's own
/// headline scenario and the entire reason the shared `trains` table
/// exists.
///
/// Deliberately NOT backed by a `UNIQUE (user_id, trains_id)` index, and
/// this is a considered rejection rather than an oversight. Four unrelated
/// paths already do a bare `UPDATE train_subscriptions SET trains_id = $2
/// WHERE id = $1` (`data/schedule_matching.rs`'s `attempt_schedule_match`,
/// `data/trust_event_backlog_match.rs` in two places, and this file's own
/// live-resolution write), and `create_pin` above has never deduplicated
/// legacy CRS+time pins -- so a user holding two legacy pins that later
/// resolve to the same physical train is reachable today, and a global
/// unique index would turn each of those `UPDATE`s into a hard failure
/// inside schedule matching, backlog matching and live TRUST resolution.
///
/// The honest residual limitation, stated rather than papered over: under
/// READ COMMITTED, two genuinely simultaneous in-flight calls can both
/// observe no existing row and both insert. This closes the ordinary
/// repeat-click case, not a true concurrent double-submit; the frontend
/// closes that one the way every other mutating control in this app does,
/// by disabling the button while its request is in flight
/// (`frontend/components/TrackThisTrainButton.tsx`).
///
/// See docs/superpowers/specs/2026-09-07-train-listing-page-design.md §7
/// Open Question 2, which flagged this function's behaviour as unverified,
/// and Task 8 of that spec's implementation plan, which verified it (it was
/// not idempotent) and fixed it.
pub async fn create_subscription_for_train(
    pool: &PgPool,
    trains_id: i64,
    user_id: &str,
) -> anyhow::Result<i64> {
    // One statement, not a SELECT-then-INSERT round trip: the `inserted`
    // CTE's `NOT EXISTS (SELECT 1 FROM existing)` guard means the INSERT
    // never fires when a subscription is already there, and the final
    // UNION ALL yields exactly one row either way -- so `fetch_one` still
    // errors (RowNotFound) for a `trains_id` that names no `trains` row,
    // exactly as the previous plain `INSERT ... SELECT` did.
    let row: (i64,) = sqlx::query_as(
        "WITH existing AS ( \
             SELECT id FROM train_subscriptions \
             WHERE user_id = $1 AND trains_id = $2 \
             ORDER BY id LIMIT 1 \
         ), \
         inserted AS ( \
             INSERT INTO train_subscriptions \
                 (user_id, trains_id, service_date, pin_origin_crs, pin_scheduled_departure, pin_destination_crs) \
             SELECT $1, tr.id, tr.service_date, tr.origin_crs, tr.scheduled_departure, tr.destination_crs \
             FROM trains tr \
             WHERE tr.id = $2 AND NOT EXISTS (SELECT 1 FROM existing) \
             RETURNING id \
         ) \
         SELECT id FROM existing UNION ALL SELECT id FROM inserted",
    )
    .bind(user_id)
    .bind(trains_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}
```

`ORDER BY id LIMIT 1` rather than a bare `SELECT id`: if pre-existing
duplicate rows are already in the database (they are reachable today, and
this task deliberately does not delete them), the oldest one wins,
deterministically, on every call.

- [ ] **Step 4: Run the tests to verify they pass**

Run:
`DATABASE_URL=... cargo test -p api create_subscription_for_train -- --ignored --test-threads=1`
Expected: PASS — all three tests, including the two pre-existing ones in
this module (`create_subscription_for_train_inherits_known_schedule_data`,
`create_subscription_for_train_allows_null_pins_for_a_bare_uid_with_no_schedule_data`,
`train_tracking.rs:3385` and `:3448`), which exercise a *first* call and
must be unaffected.

- [ ] **Step 5: Run the route-level tests that depend on this function**

Run:
`DATABASE_URL=... cargo test -p api post_track_by_uid nr_primary -- --ignored --test-threads=1`
Expected: PASS. If any test in `crates/api/src/routes/train.rs` asserts that
two `POST /Train/by-uid/…/track` calls produce two distinct `trackingId`s,
that assertion is now wrong for the same reason the `db_tests` one was —
invert it (same `assert_eq!`, same reasoning) rather than reverting this
task.

- [ ] **Step 6: Run the full crate suite**

Run: `cargo test -p api`
Expected: PASS, no regressions.

- [ ] **Step 7: Commit**

```bash
git add crates/api/src/data/train_tracking.rs
git commit -m "Make create_subscription_for_train idempotent per user and train"
```

---

## Task 9: `TrackThisTrainButton` — the shared "Track this train" CTA

**Files:**
- Create: `frontend/components/TrackThisTrainButton.tsx`
- Test: `frontend/components/TrackThisTrainButton.test.tsx`

**Interfaces:**
- Consumes: `POST /Train/by-uid/{uid}/{date}/track` (existing,
  `crates/api/src/routes/train.rs:691-707`, made idempotent by Task 8) and
  `POST /Train/tickets/{ticketId}/attach` (existing,
  `crates/api/src/routes/train.rs:247-276`), both through the same-origin
  `/api/*` proxy (`frontend/app/api/[...path]/route.ts`, whose
  `resolveTargetPath` already passes a `Train/...` path straight through
  with no prefix inserted); `useNeedsLogin` (existing,
  `frontend/components/useNeedsLogin.ts`); `LoginPromptModal` (existing).
- Produces: `export function TrackThisTrainButton({ uid, date,
  attachTicketId, size }: { uid: string; date: string; attachTicketId?:
  number; size?: 'xs' | 'sm' | 'md' }): JSX.Element` — consumed by Task 10
  (`/trains` rows, **with** `attachTicketId`) and Task 12
  (`/train/[uid]/[date]`, **without** it).

- [ ] **Step 1: Write the failing tests**

```typescript
// frontend/components/TrackThisTrainButton.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrackThisTrainButton } from './TrackThisTrainButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Routes a mocked `fetch` by URL, the same shape
 * `TrackTrainForm.test.tsx`'s own `mockFetchByUrl` helper uses: the
 * by-uid track call and the ticket-attach follow-up are configured
 * independently so a test can make one fail without the other. */
function mockFetchByUrl(
  options: { track?: () => Response; attach?: () => Response | Promise<Response> } = {},
) {
  const {
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    attach = () => new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url)) return Promise.resolve(attach());
    if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

describe('TrackThisTrainButton', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('POSTs to the by-uid track route with the uid and date from its props', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/C11052/2026-09-07/track',
        expect.objectContaining({ method: 'POST' }),
      );
    });
  });

  it('percent-encodes a path-like uid', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052/../mine" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/C11052%2F..%2Fmine/2026-09-07/track',
        expect.objectContaining({ method: 'POST' }),
      );
    });
  });

  it('navigates to the new tracking id on success', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('makes no ticket-attach call when attachTicketId is absent', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalled());
    const attachCalls = fetchMock.mock.calls.filter((args: unknown[]) =>
      String(args[0]).includes('/attach'),
    );
    expect(attachCalls).toHaveLength(0);
  });

  it('attaches the ticket after a successful track when attachTicketId is given', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({
          method: 'POST',
          body: JSON.stringify({ trackingId: 42 }),
        }),
      );
    });
  });

  // The parity requirement's real substance: a failed attach must not block
  // navigation, exactly as TrackTrainForm.tsx:321-335 already behaves.
  it('still navigates when the ticket-attach follow-up rejects', async () => {
    const fetchMock = mockFetchByUrl({ attach: () => Promise.reject(new Error('network blip')) });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('still navigates when the ticket-attach follow-up returns a 409', async () => {
    const fetchMock = mockFetchByUrl({
      attach: () => new Response('ticket is already attached to a tracked train', { status: 409 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/train/by-id/42'));
  });

  it('opens the login prompt and does not navigate on a 401', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('unauthorized', { status: 401 }) }));
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    expect(await screen.findByText('Log in to track this train.')).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('shows an error and does not navigate on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ track: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    expect(await screen.findByText("Couldn't track this train. Try again.")).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  // The concurrent-double-submit guard Task 8's in-function fix cannot
  // close on its own -- asserted, not assumed.
  it('disables itself while a request is in flight', async () => {
    let resolveTrack: (value: Response) => void = () => {};
    const pending = new Promise<Response>((resolve) => {
      resolveTrack = resolve;
    });
    vi.stubGlobal(
      'fetch',
      vi.fn((input: RequestInfo | URL) => {
        if (/\/track$/.test(String(input))) return pending;
        throw new Error(`unexpected fetch for ${String(input)}`);
      }),
    );
    renderWithMantine(<TrackThisTrainButton uid="C11052" date="2026-09-07" />);

    const button = screen.getByRole('button', { name: 'Track this train' });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole('button', { name: 'Tracking…' })).toBeDisabled());

    resolveTrack(new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }));
    await waitFor(() => expect(pushMock).toHaveBeenCalled());
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- TrackThisTrainButton.test.tsx`
Expected: FAIL — `./TrackThisTrainButton` does not exist.

- [ ] **Step 3: Write the component**

```tsx
// frontend/components/TrackThisTrainButton.tsx
'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Stack } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

/** The "Track this train" action for a train whose real CIF identity is
 * already known -- a `(train_uid, service_date)` pair. Calls
 * `POST /Train/by-uid/{uid}/{date}/track`
 * (`crates/api/src/routes/train.rs`'s `post_track_by_uid`, the NR-primary
 * tracking entry point), which takes NO request body at all: identity is
 * entirely in the path, so there is no form to fill in and nothing to
 * validate client-side. That is the whole difference from
 * `TrackTrainForm`'s legacy `POST /Train/track` flow, which has to guess an
 * identity from a CRS + time pin.
 *
 * Two call sites, deliberately different:
 * * `/trains` result rows pass `attachTicketId`, giving the listing page's
 *   action full parity with `/track?ticketId=...`'s existing flow
 *   (docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4).
 * * `/train/[uid]/[date]`'s page-level CTA does NOT -- that page has no
 *   `ticketId` query-param convention and inventing one is explicitly out
 *   of scope (§5 of the same doc).
 *
 * The ticket-attach follow-up mirrors `TrackTrainForm.tsx`'s own
 * (`handleSubmit`, the `attachTicketId !== undefined` block) exactly,
 * including swallowing every failure: tracking the train has ALREADY
 * succeeded by then, so a network blip or a `409 Conflict` (the ticket
 * having since been attached elsewhere) must not block navigation. The
 * ticket simply stays standalone and reattachable from the merged
 * trains/tickets list.
 *
 * Calls the same-origin `/api/*` proxy rather than `lib/api.ts` -- this is
 * a Client Component and cannot read the server-only `API_BASE_URL` env var
 * (same reasoning as `PinToggle` and `TrackTrainForm`).
 *
 * 401 handling is the shared `useNeedsLogin`/`LoginPromptModal` pattern
 * (`useNeedsLogin.ts`'s own doc comment names it). Unlike `TrackTrainForm`,
 * there is no typed input to protect on a 401 -- this control is a single
 * click, so it behaves like `PinToggle`: show the prompt, change nothing
 * else.
 *
 * `disabled={busy}` is load-bearing, not cosmetic:
 * `train_tracking::create_subscription_for_train` is idempotent per
 * `(user_id, trains_id)` as of this feature, but that in-function fix
 * cannot close a genuinely concurrent double-submit under READ COMMITTED.
 * Disabling the control while its request is in flight is what does. */
export function TrackThisTrainButton({
  uid,
  date,
  attachTicketId,
  size = 'sm',
}: {
  uid: string;
  date: string;
  attachTicketId?: number;
  size?: 'xs' | 'sm' | 'md';
}) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  async function track() {
    setBusy(true);
    needsLoginState.reset();
    setError(null);
    try {
      const response = await fetch(
        `/api/Train/by-uid/${encodeURIComponent(uid)}/${encodeURIComponent(date)}/track`,
        { method: 'POST' },
      );

      if (response.ok) {
        const result: { trackingId: number } = await response.json();
        if (attachTicketId !== undefined) {
          // Best-effort, exactly as TrackTrainForm does it -- see this
          // component's own doc comment.
          try {
            await fetch(`/api/Train/tickets/${attachTicketId}/attach`, {
              method: 'POST',
              headers: { 'Content-Type': 'application/json' },
              body: JSON.stringify({ trackingId: result.trackingId }),
            });
          } catch {
            // Deliberately swallowed.
          }
        }
        router.push(`/train/by-id/${result.trackingId}`);
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      setError("Couldn't track this train. Try again.");
    } catch {
      setError("Couldn't track this train. Try again.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Stack gap="xs">
      <Button size={size} onClick={track} disabled={busy}>
        {busy ? 'Tracking…' : 'Track this train'}
      </Button>
      {error && (
        <Alert color="red" title="Couldn't track this train">
          {error}
        </Alert>
      )}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to track this train.
      </LoginPromptModal>
    </Stack>
  );
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- TrackThisTrainButton.test.tsx`
Expected: PASS, all 11 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrackThisTrainButton.tsx frontend/components/TrackThisTrainButton.test.tsx
git commit -m "Add TrackThisTrainButton, the shared NR-primary track CTA"
```

---

## Task 10: `TrainSearchForm` — the filter form and result list

> **CHANGED MINIMALLY, and frontend-only, per the addendum's §5 row 10.**
> Everything in this task stands: both `Autocomplete`s,
> `searchStations`/`useSuggestions`, the row rendering, the
> `/train/{uid}/{today}` link, `TrackThisTrainButton` with `attachTicketId`,
> the `/track` manual fallback, and every existing test. Two additions:
>
> 1. the component reads the envelope `{results, nextCursor}` instead of a
>    bare array (Task 7); and
> 2. it renders a **"Load more"** control, shown only when `nextCursor` is
>    non-null, which re-requests with `after=` and **appends** to the
>    existing rows rather than replacing them.
>
> This is where the pagination burden honestly lands, and it costs a button.
> It is also the only place in the whole revision where a *user-visible*
> change appears — which is the point: nothing is silently truncated any
> more, so the UI has to offer the rest.

**Files:**
- Create: `frontend/components/TrainSearchForm.tsx`
- Test: `frontend/components/TrainSearchForm.test.tsx`

**Interfaces:**
- Consumes: `GET /public/trains/search` (Task 7) via the same-origin proxy
  path `/api/trains/search?…` (`resolveTargetPath` prefixes anything that
  isn't `Train/...` with `/public/`, and `proxy` forwards
  `req.nextUrl.search` verbatim, so the query string reaches the backend
  unchanged); `TrackThisTrainButton` (Task 9); `searchStations`
  (`frontend/lib/suggestions.ts:9-14`) and `useSuggestions`
  (`frontend/lib/useSuggestions.ts`), the same pair every station field in
  this app already uses; `TextLink`.
- Produces: `export function TrainSearchForm({ initialDestination,
  initialOrigin, attachTicketId }: { initialDestination?: string;
  initialOrigin?: string; attachTicketId?: number }): JSX.Element` —
  consumed by Task 11's `/trains` page.

- [ ] **Step 1: Write the failing tests**

```typescript
// frontend/components/TrainSearchForm.test.tsx
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TrainSearchForm } from './TrainSearchForm';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Builds a `GET /public/trains/search` response body. The route returns an
 * ENVELOPE, not a bare array (Task 7): `results` plus a `nextCursor` that
 * is an explicit `null` on the last page. Every test that stubs a search
 * response goes through this, so no test can accidentally assert against
 * the pre-pagination bare-array shape. */
function searchBody(
  rows: Array<{ uid: string; scheduled: string; originCrs: string; destinationCrs: string }>,
  nextCursor: string | null = null,
) {
  return JSON.stringify({ results: rows, nextCursor });
}

const PAGE_ONE = [
  { uid: 'C10001', scheduled: '08:22', originCrs: 'EUS', destinationCrs: 'MAN' },
  { uid: 'C10002', scheduled: '10:05', originCrs: 'CRE', destinationCrs: 'MAN' },
];
const PAGE_TWO = [
  { uid: 'C10003', scheduled: '11:40', originCrs: 'EUS', destinationCrs: 'MAN' },
];
const PAGE_THREE = [
  { uid: 'C10004', scheduled: '13:15', originCrs: 'CRE', destinationCrs: 'MAN' },
];

/** Routes a mocked `fetch` by URL: the search call, the station-suggestion
 * calls both Autocompletes fire, and the track/attach calls
 * `TrackThisTrainButton` makes. `search` defaults to two rows and no next
 * page, so most tests only override the branch they care about.
 *
 * `search` receives the request URL so a test can answer page 1 and page 2
 * differently -- which is exactly what "Load more" needs to be tested
 * honestly. */
function mockFetchByUrl(
  options: { search?: (url: string) => Response; track?: () => Response } = {},
) {
  const {
    search = () => new Response(searchBody(PAGE_ONE), { status: 200 }),
    track = () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
  } = options;
  return vi.fn((input: RequestInfo | URL) => {
    const url = String(input);
    if (url.startsWith('/api/trains/search')) return Promise.resolve(search(url));
    if (url.startsWith('/api/stations?')) return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    if (/\/api\/Train\/tickets\/\d+\/attach$/.test(url))
      return Promise.resolve(new Response(JSON.stringify({ ticketId: 7, trackedTrainId: 42 }), { status: 200 }));
    if (/\/api\/Train\/by-uid\/.+\/track$/.test(url)) return Promise.resolve(track());
    throw new Error(`unexpected fetch for ${url}`);
  });
}

function searchCallUrls(fetchMock: ReturnType<typeof vi.fn>): string[] {
  return fetchMock.mock.calls
    .map((args: unknown[]) => String(args[0]))
    .filter((url: string) => url.startsWith('/api/trains/search'));
}

function searchCallUrl(fetchMock: ReturnType<typeof vi.fn>): string {
  const urls = searchCallUrls(fetchMock);
  if (urls.length === 0) throw new Error('no /api/trains/search call recorded');
  return urls[0];
}

describe('TrainSearchForm', () => {
  beforeEach(() => {
    pushMock.mockClear();
  });

  it('does not search until a valid destination CRS is entered', () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('button', { name: 'Search' })).toBeDisabled();
    expect(
      screen.getByText('Enter a destination station above to search for trains.'),
    ).toBeInTheDocument();
  });

  it('sends only the destination when no optional filter is set', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() => expect(searchCallUrl(fetchMock)).toBe('/api/trains/search?destination=MAN'));
  });

  it('sends every optional filter it has, uppercased', async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="man" initialOrigin="eus" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(searchCallUrl(fetchMock)).toBe(
        '/api/trains/search?destination=MAN&origin=EUS&from=09%3A00&to=12%3A00',
      ),
    );
  });

  it('renders one row per result, with time, origin and destination', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN')).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN')).toBeInTheDocument();
  });

  it('links each row to the public train page for today', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const links = await screen.findAllByRole('link', { name: 'View live status' });
    const today = new Date().toISOString().slice(0, 10);
    expect(links[0]).toHaveAttribute('href', `/train/C10001/${today}`);
  });

  it('renders a Track this train action on every row', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    expect(buttons).toHaveLength(2);
  });

  it("passes attachTicketId through, so the row's track action attaches the ticket", async () => {
    const fetchMock = mockFetchByUrl();
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" attachTicketId={7} />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    const buttons = await screen.findAllByRole('button', { name: 'Track this train' });
    fireEvent.click(buttons[0]);

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/tickets/7/attach',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trackingId: 42 }) }),
      ),
    );
  });

  // The 404-vs-200-[] split the backend route draws deliberately (Task 7)
  // has to survive into the UI, or it was pointless.
  it('distinguishes "nothing published for this destination" from "no matches"', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('not found', { status: 404 }) }));
    renderWithMantine(<TrainSearchForm initialDestination="ZZZ" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(
        /No scheduled timetable data is available for that destination today/,
      ),
    ).toBeInTheDocument();
  });

  it('says so when the search succeeds but matches nothing', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({ search: () => new Response(searchBody([]), { status: 200 }) }),
    );
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText('No scheduled trains match those filters right now.'),
    ).toBeInTheDocument();
  });

  it('shows an error state on a 500', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl({ search: () => new Response('boom', { status: 500 }) }));
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText("Couldn't search for trains right now. Try again."),
    ).toBeInTheDocument();
  });

  // The honesty requirement carried over from TrackTrainForm's own CIF
  // branch: these rows are timetable data, not live running information,
  // and the UI must never imply otherwise.
  it('labels the results as scheduled timetable data, not live status', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(
      await screen.findByText(/scheduled timetable, not live running information/),
    ).toBeInTheDocument();
  });

  it('offers the manual /track fallback, carrying any ticketId through', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm attachTicketId={7} />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute(
      'href',
      '/track?ticketId=7',
    );
  });

  it('offers the manual /track fallback with no query string when there is no ticketId', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track');
  });

  it('renders no operator filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/Operator/i)).not.toBeInTheDocument();
  });

  it('renders no date filter at all', () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm />);

    expect(screen.queryByLabelText(/^Date/i)).not.toBeInTheDocument();
  });

  // ---- Pagination. There is no cap anywhere in the backend any more, so
  // a busy destination genuinely has more trains than one page; "Load more"
  // is how the user reaches them, and these four tests are the contract.

  it('does not offer Load more when the response has no nextCursor', async () => {
    vi.stubGlobal('fetch', mockFetchByUrl());
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByText('08:22 · EUS → MAN')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument();
  });

  it('offers Load more when the response carries a nextCursor', async () => {
    vi.stubGlobal(
      'fetch',
      mockFetchByUrl({
        search: () => new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
      }),
    );
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    expect(await screen.findByRole('button', { name: 'Load more' })).toBeInTheDocument();
  });

  it('appends the next page rather than replacing the rows, and sends after=', async () => {
    // The load-bearing assertion of the whole pagination change: APPEND.
    // A "Load more" that replaced the list would look like it worked while
    // silently losing page 1.
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=CURSOR1')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('11:40 · EUS → MAN')).toBeInTheDocument();
    expect(
      screen.getByText('08:22 · EUS → MAN'),
      'page 1 must still be on screen -- Load more appends, it does not replace',
    ).toBeInTheDocument();
    expect(screen.getByText('10:05 · CRE → MAN')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(2);
    expect(urls[0]).toBe('/api/trains/search?destination=MAN');
    expect(urls[1]).toBe('/api/trains/search?destination=MAN&after=CURSOR1');

    // Exhausted: the second response's nextCursor was null.
    await waitFor(() =>
      expect(screen.queryByRole('button', { name: 'Load more' })).not.toBeInTheDocument(),
    );
  });

  it('uses the NEW cursor on a second Load more, not the first one again', async () => {
    // Guards the specific bug an append-only implementation invites:
    // keeping the cursor from the original search in state and re-sending
    // it, which would fetch page 2 forever and duplicate its rows.
    const fetchMock = mockFetchByUrl({
      search: (url) => {
        if (url.includes('after=CURSOR2'))
          return new Response(searchBody(PAGE_THREE, null), { status: 200 });
        if (url.includes('after=CURSOR1'))
          return new Response(searchBody(PAGE_TWO, 'CURSOR2'), { status: 200 });
        return new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 });
      },
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN')).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    expect(await screen.findByText('13:15 · CRE → MAN')).toBeInTheDocument();

    const urls = searchCallUrls(fetchMock);
    expect(urls).toHaveLength(3);
    expect(urls[1]).toBe('/api/trains/search?destination=MAN&after=CURSOR1');
    expect(urls[2]).toBe(
      '/api/trains/search?destination=MAN&after=CURSOR2',
      'the second Load more must use the cursor from the SECOND response',
    );
    expect(screen.getAllByText('11:40 · EUS → MAN')).toHaveLength(1);
  });

  it('keeps the original filters on a Load more request', async () => {
    // The cursor is positional, not self-describing: dropping `origin`
    // or the time range on page 2 would silently widen the search
    // mid-scroll.
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="man" initialOrigin="eus" />);

    fireEvent.change(screen.getByLabelText('From (optional)'), { target: { value: '09:00' } });
    fireEvent.change(screen.getByLabelText('To (optional)'), { target: { value: '12:00' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));

    await waitFor(() => expect(searchCallUrls(fetchMock)).toHaveLength(2));
    expect(searchCallUrls(fetchMock)[1]).toBe(
      '/api/trains/search?destination=MAN&origin=EUS&from=09%3A00&to=12%3A00&after=CURSOR1',
    );
  });

  it('starts a fresh search over rather than appending to the previous one', async () => {
    // Pressing Search again after paginating must RESET, not append -- the
    // opposite of Load more. Same append-vs-replace bug, mirrored.
    const fetchMock = mockFetchByUrl({
      search: (url) =>
        url.includes('after=')
          ? new Response(searchBody(PAGE_TWO, null), { status: 200 })
          : new Response(searchBody(PAGE_ONE, 'CURSOR1'), { status: 200 }),
    });
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TrainSearchForm initialDestination="MAN" />);

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Load more' }));
    expect(await screen.findByText('11:40 · EUS → MAN')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Search' }));

    await waitFor(() =>
      expect(screen.queryByText('11:40 · EUS → MAN')).not.toBeInTheDocument(),
    );
    expect(screen.getByText('08:22 · EUS → MAN')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- TrainSearchForm.test.tsx`
Expected: FAIL — `./TrainSearchForm` does not exist.

- [ ] **Step 3: Write the component**

```tsx
// frontend/components/TrainSearchForm.tsx
'use client';

import { useState, type FormEvent } from 'react';
import { Alert, Autocomplete, Button, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import dayjs from 'dayjs';
import { TextLink } from './TextLink';
import { TrackThisTrainButton } from './TrackThisTrainButton';
import { searchStations } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

/** Wire shape of `GET /public/trains/search`
 * (`crates/api/src/render.rs::destination_departure_json`). Deliberately
 * NOT `ScheduleDepartureRow` from `TrackTrainForm.tsx`, even though the two
 * overlap: this one carries `originCrs` (the station a train departs FROM,
 * which for a mid-route result is an intermediate stop, not the schedule's
 * first station) and a non-nullable `destinationCrs` (it is the search key,
 * so a row can only exist if it resolved). Like every CIF-derived row in
 * this app it carries NO operator and NO live running status -- the CIF
 * SCHEDULE feed has neither (see
 * docs/superpowers/specs/2026-09-07-train-listing-page-design.md §1.3), and
 * this list never fabricates them. */
interface TrainSearchRow {
  uid: string;
  scheduled: string;
  originCrs: string;
  destinationCrs: string;
}

/** The envelope `GET /public/trains/search` returns. Not a bare array: it
 * has to carry `nextCursor`, because the backend publishes and stores the
 * whole day uncapped and hands it back a page at a time. `nextCursor` is an
 * explicit `null` on the last page, never omitted. */
interface TrainSearchResponse {
  results: TrainSearchRow[];
  nextCursor: string | null;
}

/** Exactly one of five mutually-exclusive states, checked top to bottom by
 * `resultsContent` below. `'unpublished'` and an empty `rows` array are
 * genuinely different facts and get different copy -- the backend route
 * draws that 404-vs-empty-results distinction on purpose (Task 7) and
 * collapsing it here would waste it.
 *
 * `nextCursor` lives INSIDE the success variant rather than in its own
 * `useState`, so it cannot survive a state transition it does not belong
 * to: a fresh search, an error, or an unpublished response all discard it
 * automatically, and there is no way to render "Load more" next to an error
 * or next to page 1 of a search that has since been re-run. */
type Results =
  | { rows: TrainSearchRow[]; nextCursor: string | null }
  | 'unpublished'
  | 'error'
  | null;

/** Destination-first, whole-network train search -- the `/trains` page's
 * one interactive component
 * (docs/superpowers/specs/2026-09-07-train-listing-page-design.md,
 * Approach B).
 *
 * Deliberately NOT a replacement for `TrackTrainForm`, and it does not try
 * to be: this searches published CIF timetable data by destination and
 * cannot see a train that isn't in it (a station missing from
 * `stanox_crs`, a same-day amendment landing after the last CIF delivery).
 * Note that "the result list was truncated" is NOT on that list any more:
 * nothing is capped, and everything past the first page is reachable with
 * Load more. `/track`'s manual-entry form
 * remains the honest fallback for exactly those gaps, and this component
 * links to it explicitly rather than pretending they don't exist -- §4 of
 * the design doc is a direct "no" on full replacement.
 *
 * Filter set, and why it stops here: Destination is required (it is the
 * server-side bucket key); Origin and a From/To time range are optional.
 * There is no Operator filter -- CIF rows carry no operator field at all,
 * so the filter could only ever match nothing. There is no Date filter --
 * both of this app's schedule sources are "today only, server-side". Both
 * are explicit §6 non-goals, not omissions to fill in later.
 *
 * Fetches through the same-origin `/api/*` proxy, like every other Client
 * Component in this app (`API_BASE_URL` is server-only). */
export function TrainSearchForm({
  initialDestination = '',
  initialOrigin = '',
  attachTicketId,
}: {
  initialDestination?: string;
  initialOrigin?: string;
  attachTicketId?: number;
}) {
  const [destinationCrs, setDestinationCrs] = useState(initialDestination);
  const [originCrs, setOriginCrs] = useState(initialOrigin);
  const [fromTime, setFromTime] = useState('');
  const [toTime, setToTime] = useState('');
  const [results, setResults] = useState<Results>(null);
  const [searching, setSearching] = useState(false);
  // Separate from `searching` on purpose: a "Load more" in flight must not
  // blank the rows already on screen the way `resultsContent`'s
  // `searching` branch does, and must not re-disable the Search button.
  const [loadingMore, setLoadingMore] = useState(false);

  const { suggestions: destinationSuggestions } = useSuggestions(destinationCrs, searchStations);
  const { suggestions: originSuggestions } = useSuggestions(originCrs, searchStations);

  const destinationValid = CRS_PATTERN.test(destinationCrs.trim());
  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const fromValid = fromTime.trim() === '' || TIME_PATTERN.test(fromTime.trim());
  const toValid = toTime.trim() === '' || TIME_PATTERN.test(toTime.trim());
  const canSearch = destinationValid && originValid && fromValid && toValid && !searching;

  // Computed once per render rather than once per row: every result links
  // to the same calendar date, because this search is always "today"
  // (server-side) -- same construction and same browser-local-date
  // assumption `TrackTrainForm`'s own CIF branch already makes.
  const today = dayjs().format('YYYY-MM-DD');

  // The manual fallback keeps a standalone ticket's id attached to the
  // journey, so a user who bounces from /trains to /track doesn't silently
  // lose the ticket they were trying to link -- same `?ticketId=` param
  // `app/track/page.tsx` already reads.
  const manualHref = attachTicketId !== undefined ? `/track?ticketId=${attachTicketId}` : '/track';

  /** The current filter set as query parameters. Shared by the initial
   * search and by "Load more" so that page 2 is unambiguously a
   * continuation of page 1's query -- the cursor is positional, not
   * self-describing, so dropping a filter here would silently widen the
   * search mid-scroll. */
  function searchParams() {
    const params = new URLSearchParams({ destination: destinationCrs.trim().toUpperCase() });
    if (originCrs.trim()) params.set('origin', originCrs.trim().toUpperCase());
    if (fromTime.trim()) params.set('from', fromTime.trim());
    if (toTime.trim()) params.set('to', toTime.trim());
    return params;
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    if (!canSearch) return;
    setSearching(true);
    try {
      const response = await fetch(`/api/trains/search?${searchParams().toString()}`);
      if (response.status === 404) {
        // A real, distinct fact, not an error: no CIF publish has landed
        // for today at all. Never collapsed into "no matches".
        setResults('unpublished');
        return;
      }
      if (!response.ok) {
        setResults('error');
        return;
      }
      const body: TrainSearchResponse = await response.json();
      // REPLACE, not append -- a fresh search starts over. The mirror of
      // `handleLoadMore` below, and the two must not be merged.
      setResults({ rows: body.results, nextCursor: body.nextCursor });
    } catch {
      setResults('error');
    } finally {
      setSearching(false);
    }
  }

  /** Fetches the next page and APPENDS it.
   *
   * Only reachable when `results` is a success state carrying a non-null
   * `nextCursor`, so it re-reads that state at call time rather than
   * trusting a captured value -- which is also what makes a second click
   * use the SECOND response's cursor rather than the first's.
   *
   * A failed "Load more" deliberately does NOT blow away the rows already
   * on screen: it leaves them, drops the cursor so the button disappears,
   * and lets the user re-run the search if they want. Replacing a good
   * partial list with a full-width error would be a worse outcome than
   * showing fewer trains. */
  async function handleLoadMore() {
    if (results === null || results === 'error' || results === 'unpublished') return;
    if (results.nextCursor === null || loadingMore) return;

    setLoadingMore(true);
    try {
      const params = searchParams();
      params.set('after', results.nextCursor);
      const response = await fetch(`/api/trains/search?${params.toString()}`);
      if (!response.ok) {
        setResults((current) =>
          current !== null && current !== 'error' && current !== 'unpublished'
            ? { rows: current.rows, nextCursor: null }
            : current,
        );
        return;
      }
      const body: TrainSearchResponse = await response.json();
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: [...current.rows, ...body.results], nextCursor: body.nextCursor }
          : current,
      );
    } catch {
      setResults((current) =>
        current !== null && current !== 'error' && current !== 'unpublished'
          ? { rows: current.rows, nextCursor: null }
          : current,
      );
    } finally {
      setLoadingMore(false);
    }
  }

  function resultsContent() {
    if (!destinationValid) {
      return (
        <Text size="sm" c="dimmed">
          Enter a destination station above to search for trains.
        </Text>
      );
    }
    if (searching) {
      return (
        <Text size="sm" c="dimmed">
          Searching…
        </Text>
      );
    }
    if (results === null) {
      return (
        <Text size="sm" c="dimmed">
          Press Search to find trains to this destination.
        </Text>
      );
    }
    if (results === 'error') {
      return (
        <Alert color="red" title="Search failed">
          Couldn&apos;t search for trains right now. Try again.
        </Alert>
      );
    }
    if (results === 'unpublished') {
      return (
        <Text size="sm" c="dimmed">
          No scheduled timetable data is available for that destination today — it may not be a
          station this feed covers, or today&apos;s timetable may not have been published yet.
        </Text>
      );
    }
    if (results.rows.length === 0) {
      return (
        <Text size="sm" c="dimmed">
          No scheduled trains match those filters right now.
        </Text>
      );
    }
    return (
      <>
        <Text size="sm" c="dimmed">
          These are from the scheduled timetable, not live running information, and may be up to 30
          minutes out of date. Open a train to see its live status.
        </Text>
        <ScrollArea mah={420} offsetScrollbars>
          <Stack gap="xs">
            {results.rows.map((row) => (
              <Group key={`${row.uid}-${row.originCrs}-${row.scheduled}`} justify="space-between" wrap="nowrap">
                <Text size="sm">
                  {row.scheduled} · {row.originCrs} → {row.destinationCrs}
                </Text>
                <Group gap="sm" wrap="nowrap">
                  {/* Always safe to render, unlike TrackTrainForm's LDBWS
                      branch: every row here carries a real CIF schedule UID
                      (it is the grouping pass's own key), which is exactly
                      what /train/[uid]/[date] is keyed on. */}
                  <TextLink href={`/train/${encodeURIComponent(row.uid)}/${today}`}>
                    View live status
                  </TextLink>
                  <TrackThisTrainButton
                    uid={row.uid}
                    date={today}
                    attachTicketId={attachTicketId}
                    size="xs"
                  />
                </Group>
              </Group>
            ))}
          </Stack>
        </ScrollArea>
        {/* Only when the server said there IS more. `nextCursor` is an
            explicit null on the last page, so this disappears on its own
            once the day is exhausted -- there is no separate "has more"
            flag to keep in sync. The plain "variant=default" Button is
            deliberate: the visual treatment of Load more is implementation
            /design-review, exactly as the rest of this page's styling is
            (addendum §6). */}
        {results.nextCursor !== null && (
          <Group>
            <Button
              variant="default"
              size="xs"
              onClick={handleLoadMore}
              disabled={loadingMore}
              loading={loadingMore}
            >
              Load more
            </Button>
          </Group>
        )}
      </>
    );
  }

  return (
    <Stack gap="md" component="form" onSubmit={handleSubmit}>
      <Autocomplete
        label="Destination station"
        placeholder="e.g. Manchester or MAN"
        value={destinationCrs}
        onChange={setDestinationCrs}
        data={destinationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = destinationSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={destinationCrs.length > 0 && !destinationValid ? 'Must be a 3-letter CRS code' : null}
        required
      />
      <Autocomplete
        label="Departing from (optional)"
        placeholder="e.g. Euston or EUS"
        description="Any station on the train's route, not just where it started."
        value={originCrs}
        onChange={setOriginCrs}
        data={originSuggestions.map((s) => ({ value: s.code, label: s.code }))}
        filter={({ options }) => options}
        renderOption={({ option }) => {
          const match = originSuggestions.find((s) => s.code === option.value);
          return match ? `${match.code} — ${match.name}` : option.value;
        }}
        error={originCrs.length > 0 && !originValid ? 'Must be a 3-letter CRS code' : null}
      />
      <Group grow align="flex-start">
        {/* Plain TextInputs, not @mantine/dates pickers: these are a
            time-of-day range on a fixed (today) date, which DateTimePicker
            cannot express without also asking for a date this feature
            deliberately does not accept. */}
        <TextInput
          label="From (optional)"
          placeholder="09:00"
          value={fromTime}
          onChange={(event) => setFromTime(event.currentTarget.value)}
          error={fromTime.length > 0 && !fromValid ? 'Must be a time like 09:00' : null}
        />
        <TextInput
          label="To (optional)"
          placeholder="12:00"
          value={toTime}
          onChange={(event) => setToTime(event.currentTarget.value)}
          error={toTime.length > 0 && !toValid ? 'Must be a time like 12:00' : null}
        />
      </Group>
      <Group>
        <Button type="submit" disabled={!canSearch}>
          {searching ? 'Searching…' : 'Search'}
        </Button>
      </Group>
      <Stack gap="xs" mih={72}>
        {resultsContent()}
      </Stack>
      <Text size="sm" c="dimmed">
        Can&apos;t find your train? <TextLink href={manualHref}>Track it manually</TextLink> by
        entering its origin station and departure time.
      </Text>
    </Stack>
  );
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- TrainSearchForm.test.tsx`
Expected: PASS, all 21 tests — the original 15 plus the six pagination
tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/TrainSearchForm.tsx frontend/components/TrainSearchForm.test.tsx
git commit -m "Add TrainSearchForm, the destination-first train search UI"
```

---

## Task 11: The `/trains` page and its nav entry

**Files:**
- Create: `frontend/app/trains/page.tsx`
- Test: `frontend/app/trains/page.test.tsx`
- Modify: `frontend/app/layout.tsx:225-227` (add the nav link)
- Test: `frontend/app/layout.test.tsx` (assert the new nav link)

**Interfaces:**
- Consumes: `TrainSearchForm` (Task 10).
- Produces: the route `/trains`, accepting `?destination=`, `?origin=` and
  `?ticketId=` query parameters. Linked from the global nav and from
  Task 12's page copy.

- [ ] **Step 1: Write the failing page tests**

```typescript
// frontend/app/trains/page.test.tsx
import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrainsPage from './page';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/trains',
  useSearchParams: () => new URLSearchParams(''),
}));

// The page mounts TrainSearchForm, whose suggestion hooks fire real
// fetches on mount for any pre-filled, valid CRS -- give every test an
// inert 200 so none of them depend on network behaviour.
vi.stubGlobal('fetch', vi.fn(async () => new Response('[]', { status: 200 })));

describe('TrainsPage', () => {
  it('renders the title and the search form', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({}) }));
    expect(screen.getByRole('heading', { name: 'Find a Train' })).toBeInTheDocument();
    expect(screen.getByLabelText('Destination station')).toBeInTheDocument();
  });

  it('pre-fills the destination and origin from the query string, uppercased', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ destination: 'man', origin: 'eus' }) }),
    );
    expect(screen.getByLabelText('Destination station')).toHaveValue('MAN');
    expect(screen.getByLabelText('Departing from (optional)')).toHaveValue('EUS');
  });

  it('uses the first value when a query param is repeated', async () => {
    renderWithMantine(
      await TrainsPage({ searchParams: Promise.resolve({ destination: ['MAN', 'EDB'] }) }),
    );
    expect(screen.getByLabelText('Destination station')).toHaveValue('MAN');
  });

  it('shows the ticket-attach explainer copy when arriving with a ticketId', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: '7' }) }));
    expect(
      screen.getByText(
        "Find the train your saved ticket is for — it'll be attached automatically once you track it.",
      ),
    ).toBeInTheDocument();
  });

  it('carries a valid ticketId through to the manual fallback link', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: '7' }) }));
    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute(
      'href',
      '/track?ticketId=7',
    );
  });

  // Same posture as app/track/page.tsx:16-21: a malformed value is treated
  // as absent rather than passed through as NaN.
  it('treats a non-numeric ticketId as absent', async () => {
    renderWithMantine(await TrainsPage({ searchParams: Promise.resolve({ ticketId: 'nope' }) }));
    expect(screen.getByRole('link', { name: 'Track it manually' })).toHaveAttribute('href', '/track');
    expect(
      screen.queryByText(/it'll be attached automatically once you track it/),
    ).not.toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- app/trains/page.test.tsx`
Expected: FAIL — `./page` does not exist.

- [ ] **Step 3: Write the page**

```tsx
// frontend/app/trains/page.tsx
import { Stack, Title, Text } from '@mantine/core';
import { TrainSearchForm } from '@/components/TrainSearchForm';

/** `/trains` -- the primary train-discovery surface
 * (docs/superpowers/specs/2026-09-07-train-listing-page-design.md §4).
 *
 * Ships ALONGSIDE `/track`, never replacing it: `/track`'s manual-entry
 * form is the honest fallback for every gap this search cannot close (a
 * station outside the CIF-derived data, a train past the per-destination
 * cap, a same-day amendment), and both `/stations/[crs]`'s "Track a train
 * from here" link and `TicketEntryForm`'s standalone-ticket flow still
 * point at it unchanged. See §4's explicit "do not delete or hide /track".
 *
 * Query params mirror `/track`'s own convention (`app/track/page.tsx`):
 * `?destination=`/`?origin=` pre-fill the filters (so a filtered search is
 * a shareable link -- the design doc's §7 Open Question 5, resolved
 * affirmatively because it costs one prop each), and `?ticketId=` carries a
 * standalone ticket through so the row-level "Track this train" action can
 * attach it, giving this page full parity with `/track?ticketId=...`. */
export default async function TrainsPage({
  searchParams,
}: {
  searchParams: Promise<{
    destination?: string | string[];
    origin?: string | string[];
    ticketId?: string | string[];
  }>;
}) {
  const { destination, origin, ticketId } = await searchParams;
  // Next.js supplies a `string[]` for a repeated query param (e.g.
  // `?destination=a&destination=b`) -- fall back to the first value rather
  // than letting `.toUpperCase()` throw on an array. Same handling as
  // `app/track/page.tsx:10-13`.
  const destinationParam = Array.isArray(destination) ? destination[0] : destination;
  const originParam = Array.isArray(origin) ? origin[0] : origin;
  const ticketIdParam = Array.isArray(ticketId) ? ticketId[0] : ticketId;
  const attachTicketId = ticketIdParam && /^\d+$/.test(ticketIdParam) ? Number(ticketIdParam) : undefined;

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Find a Train</Title>
      <Text c="dimmed">
        {attachTicketId !== undefined
          ? "Find the train your saved ticket is for — it'll be attached automatically once you track it."
          : 'Search the whole network by where a train is going. Open any result for its live status, or track it to get updates.'}
      </Text>
      <TrainSearchForm
        initialDestination={destinationParam?.toUpperCase()}
        initialOrigin={originParam?.toUpperCase()}
        attachTicketId={attachTicketId}
      />
    </Stack>
  );
}
```

- [ ] **Step 4: Run the page tests to verify they pass**

Run (from `frontend/`): `npm test -- app/trains/page.test.tsx`
Expected: PASS, all 6 tests.

- [ ] **Step 5: Write the failing nav test**

The nav's two static links (`All Lines`, `Station Lookup`) live directly
inside `RootLayout`, which is not exported and cannot be rendered in a unit
test — it awaits `getDataFreshness()` and returns a whole `<html>` tree.
`frontend/app/layout.test.tsx` already has an established pattern for
asserting things in that unrenderable part of the file: a source assertion
via the `readFileSync` it already imports on line 1 (see the existing
`expect(source).toMatch(/<Container\s+component="main"/)` case at line
75-77). Follow it exactly, as a new top-level `describe` appended to the
file:

```typescript
describe('the primary nav', () => {
  // Source assertion, not a render: these links live inside RootLayout
  // itself, which is unexported and awaits getDataFreshness() -- the same
  // reason the `<Container component="main">` case above is written this
  // way rather than rendered.
  it('links to the new train-search page', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<TextLink href="\/trains">Find a Train<\/TextLink>/);
  });

  // Regression guard: /trains is an ADDITION, not a replacement. The
  // design doc's §4 is an explicit "no" on removing or hiding /track, and
  // the two station/line entry points either side of the new link must
  // survive it.
  it('still links to the existing lines and stations pages', () => {
    const source = readFileSync('app/layout.tsx', 'utf8');
    expect(source).toMatch(/<TextLink href="\/lines">All Lines<\/TextLink>/);
    expect(source).toMatch(/<TextLink href="\/stations">Station Lookup<\/TextLink>/);
  });
});
```

- [ ] **Step 6: Run it to verify it fails**

Run (from `frontend/`): `npm test -- layout.test.tsx`
Expected: FAIL on the first new case — `app/layout.tsx`'s source contains no
`<TextLink href="/trains">Find a Train</TextLink>`. The second new case
(`still links to the existing lines and stations pages`) passes already;
that is correct and intended — it is a regression guard for Step 7, not a
new requirement.

- [ ] **Step 7: Add the nav link**

In `frontend/app/layout.tsx`, in the nav `<Group gap="lg">` (lines 225-227),
between the two existing static links and the tracked-trains item:

```tsx
                  <Group gap="lg">
                    <TextLink href="/lines">All Lines</TextLink>
                    <TextLink href="/stations">Station Lookup</TextLink>
                    {/* The primary train-discovery surface. `/track` is
                        still reachable (from here via /trains' own manual
                        fallback link, from /stations/[crs], and from
                        TicketEntryForm) but is no longer the first thing a
                        visitor is pointed at -- see
                        docs/superpowers/specs/2026-09-07-train-listing-page-design.md
                        §4. */}
                    <TextLink href="/trains">Find a Train</TextLink>
                    <TrackedTrainsNavItem />
```

- [ ] **Step 8: Run the tests and the build**

Run (from `frontend/`): `npm test -- layout.test.tsx` — Expected: PASS.
Run (from `frontend/`): `npm run build` — Expected: a clean build with no
TypeScript errors, and `/trains` listed in the route table.

- [ ] **Step 9: Commit**

```bash
git add frontend/app/trains/page.tsx frontend/app/trains/page.test.tsx frontend/app/layout.tsx frontend/app/layout.test.tsx
git commit -m "Add the /trains discovery page and its nav entry"
```

---

## Task 12: The `/train/[uid]/[date]` "Track this train" CTA

**Files:**
- Modify: `frontend/app/train/[uid]/[date]/page.tsx:99-111` (the returned
  JSX only — `toJourneyState` and the fetch/404 logic above it are
  untouched)
- Test: `frontend/app/train/[uid]/[date]/page.test.tsx`

**Interfaces:**
- Consumes: `TrackThisTrainButton` (Task 9), rendered **without**
  `attachTicketId`.
- Produces: nothing other tasks depend on. This is the last task.

**The one thing this task must not do:** pass an `attachTicketId`. The
design doc's §5 is explicit — `/train/[uid]/[date]` has no existing
`ticketId` query-param convention, and inventing one here is out of scope
(§6 restates it as a named non-goal). Only `/trains`' own row-level action
gets ticket-attach parity.

- [ ] **Step 1: Write the failing tests**

Append to the existing `describe` block in
`frontend/app/train/[uid]/[date]/page.test.tsx`. The file's existing
`next/navigation` mock already supplies `useRouter`, `usePathname` and
`useSearchParams`, which is everything `TrackThisTrainButton` and
`LoginPromptModal` need — no mock changes are required.

```typescript
  it('renders a Track this train button for every visitor', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.getByRole('button', { name: 'Track this train' })).toBeInTheDocument();
  });

  it('tracks by the uid and date from the URL, not from the response body', async () => {
    // Discriminating: the fixture's own trainUid deliberately differs from
    // the URL segment, so a component wired to the wrong source fails here.
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(
      publicTrainState({ trainUid: 'DIFFERENT' }),
    );
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() =>
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/Train/by-uid/W12345/2026-08-31/track',
        expect.objectContaining({ method: 'POST' }),
      ),
    );
  });

  // The spec's own §5 exclusion, asserted rather than assumed: this page's
  // CTA must never make a ticket-attach call, because this page has no
  // ticketId convention to source one from.
  it('makes no ticket-attach call after tracking', async () => {
    const fetchMock = vi.fn(
      async () => new Response(JSON.stringify({ trackingId: 42 }), { status: 200 }),
    );
    vi.stubGlobal('fetch', fetchMock);
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );

    fireEvent.click(screen.getByRole('button', { name: 'Track this train' }));

    await waitFor(() => expect(fetchMock).toHaveBeenCalled());
    const attachCalls = fetchMock.mock.calls.filter((args: unknown[]) =>
      String(args[0]).includes('/attach'),
    );
    expect(attachCalls).toHaveLength(0);
  });

  it('still renders no owner actions alongside the new CTA', async () => {
    // Regression guard on this page's whole reason for being read-only:
    // Rename/Delete/tickets all key on a train_subscriptions.id this
    // response does not carry (see the page's own doc comment). Adding a
    // track CTA must not have opened that door.
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.queryByRole('button', { name: /Rename/i })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Delete/i })).not.toBeInTheDocument();
  });

  it('points at the new /trains page for finding other trains', async () => {
    vi.mocked(api.getPublicTrainByUidAndDate).mockResolvedValue(publicTrainState());
    renderWithMantine(
      await TrackedTrainByUidPage({ params: Promise.resolve({ uid: 'W12345', date: '2026-08-31' }) }),
    );
    expect(screen.getByRole('link', { name: 'Find a train' })).toHaveAttribute('href', '/trains');
  });
```

Add `fireEvent` and `waitFor` to this file's existing
`@testing-library/react` import.

- [ ] **Step 2: Run the tests to verify they fail**

Run (from `frontend/`): `npm test -- app/train`
Expected: FAIL — no "Track this train" button is rendered and no link named
"Find a train" exists.

- [ ] **Step 3: Wire the CTA into the page**

In `frontend/app/train/[uid]/[date]/page.tsx`, add the import alongside the
existing component imports:

```tsx
import { TrackThisTrainButton } from '@/components/TrackThisTrainButton';
import { TextLink } from '@/components/TextLink';
```

Then replace the returned JSX (lines 99-111) with:

```tsx
  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Train {uid}</Title>
        <Group gap="sm">
          {/* Shown to EVERY visitor, logged in or not -- the shared
              "show the control to everyone, prompt on the real 401"
              posture `PinToggle`/`TrackTrainForm` already establish via
              useNeedsLogin/LoginPromptModal. No `attachTicketId`: this page
              has no `ticketId` query-param convention and inventing one is
              explicitly out of scope
              (docs/superpowers/specs/2026-09-07-train-listing-page-design.md
              §5/§6). Only /trains' own row action attaches tickets.

              A logged-in visitor who ALREADY tracks this train gets no
              special treatment, deliberately: `PublicTrainState` carries no
              "you already have a subscription" hint, by design (it is the
              shared, public train, with nothing per-subscriber on it).
              Clicking again is harmless -- `create_subscription_for_train`
              is idempotent per (user, train) and returns the existing
              subscription, so both clicks land on the same
              /train/by-id/{trackingId}. */}
          <TrackThisTrainButton uid={uid} date={date} />
          <ShareButton />
        </Group>
      </Group>
      <TrainJourney state={toJourneyState(train)} />
      <Text size="sm" c="dimmed">
        This is the public view of this service. Track it above to get updates, or{' '}
        <TextLink href="/trains">Find a train</TextLink> going somewhere else.
      </Text>
    </Stack>
  );
```

The closing sentence changed because it had to: it previously told the
visitor to go to `/track` "to get updates", which is now stale advice with a
Track button directly above it. It points at `/trains` (the new primary
discovery surface) for the *other* thing that sentence was doing — finding a
different train — and `/track` remains reachable from there via that page's
own manual-fallback link, unchanged.

- [ ] **Step 4: Run the tests to verify they pass**

Run (from `frontend/`): `npm test -- app/train`
Expected: PASS — the 5 new tests plus every pre-existing one in the file (in
particular the `notFound()` and journey-rendering tests, which this task does
not touch).

- [ ] **Step 5: Run the full frontend suite and build**

Run (from `frontend/`): `npm test` — Expected: PASS, no regressions in any
suite.
Run (from `frontend/`): `npm run build` — Expected: clean.

- [ ] **Step 6: Manually verify the whole feature end to end**

```bash
docker compose up -d --build
```

Then, in a browser:
1. Open `/trains`, type a destination (e.g. `MAN`), press Search. Expect a
   list of scheduled trains, each with "View live status" and "Track this
   train". If the list says no timetable data is available, check that
   `schedule-reference` has completed a cycle since Task 4 shipped
   (`docker compose logs schedule-reference | grep destination`).
2. Click "View live status" on a row → lands on `/train/{uid}/{today}`,
   which now shows a "Track this train" button.
3. Click it while logged out → the login prompt modal opens, nothing else
   changes.
4. Log in, click it → lands on `/train/by-id/{trackingId}`.
5. Go back to that train's public page and click it again → lands on the
   **same** `/train/by-id/{trackingId}`, and `/track/mine` shows **one**
   entry for it, not two. (This is Task 8's fix, observed end to end.)
6. Open `/trains?ticketId={a real standalone ticket id}` and track a row →
   the ticket appears attached to the new tracked train in `/track/mine`.

- [ ] **Step 7: Commit**

```bash
git add "frontend/app/train/[uid]/[date]/page.tsx" "frontend/app/train/[uid]/[date]/page.test.tsx"
git commit -m "Add the Track this train CTA to the public train page"
```

---

## Task 13: `prune_schedule_destination_departures` — retention for the flat table

> **NEW, per the addendum's §5 "NEW" row and its §3 "Retention becomes
> required" section.** This task did not exist in the original plan, and it
> exists now for exactly one reason: the addendum's storage change turns a
> ~2,500-row table into one that gains **~377,000 rows for every service
> date and never loses any**. The original plan resolved the design doc's
> Open Question 4 by inspection — the sibling `schedule_network_departures`
> has no pruning job, so the new table gets none either — and that reasoning
> was correct for a bucket table whose wholesale replace was scoped per
> `(crs, service_date)` over a bounded key space. It does not survive a flat
> table whose wholesale replace is scoped to one day.

> **ORDERING: this task depends ONLY on Task 2** (the table must exist). It
> is independent of Tasks 1, 3, 4, 5, 6, 7, 8, 9, 10, 11 and 12 — it shares
> no function, type, route or file with any of them — and may be executed at
> any time after Task 2, **including in parallel with the work on Tasks
> 3-12**. Nothing depends on it in turn.

**Files:**
- Modify: `crates/aggregator/src/queries.rs` (add
  `prune_schedule_destination_departures` immediately after
  `prune_trust_event_backlog`, which ends at line 511; add one test to the
  existing `#[cfg(test)] mod tests`, next to
  `prune_trust_event_backlog_deletes_only_rows_older_than_the_retention_window`
  at line 3106)
- Modify: `crates/aggregator/src/config.rs` (add
  `schedule_destination_departures_retention_days` immediately after
  `trains_retention_days`, which ends at line 108)
- Modify: `crates/aggregator/src/main.rs` (one new `run_cycle` parameter at
  line 185, one new argument at the call site at line 74, one new prune call
  beside the existing ones at line 269)
- Modify: `charts/distant-signal/templates/aggregator-deployment.yaml:91-92`
  (add the env var after `TRUST_EVENT_BACKLOG_RETENTION_DAYS`) and
  `charts/distant-signal/values.yaml:660` (add the value after
  `trustEventBacklogRetentionDays`)

**Interfaces:**
- Consumes: table `schedule_destination_departures` (Task 2).
- Produces: `pub async fn prune_schedule_destination_departures(pool:
  &PgPool, retention_days: i64) -> Result<u64>`, called once per aggregator
  cycle from `run_cycle`. Nothing else consumes it.

> **Retention default: 2 days.** The addendum's §7 item 4 leaves this as an
> explicit human call between 1 and 2 and this plan picks **2**, for the
> reasons it names:
>
> - Every read of this table is scoped to today, computed server-side, so
>   *nothing* reads a past date and the window only has to cover the
>   producer, not the consumer.
> - 1 day matches `trust_event_backlog_retention_days`, but that default
>   exists to enforce an **RDM licensing safeguard** for TRUST Train
>   Movements data — see `Config::trust_event_backlog_retention_days`' own
>   doc comment and the loud per-cycle warning `run_cycle` emits when it is
>   raised. **That reasoning does not apply here at all**: CIF SCHEDULE
>   timetable data is not under that clause, and copying the number would
>   copy a constraint that isn't real while dropping the safety margin that
>   is. Do **not** add a warning of that kind to this field.
> - 2 is safer at the two edges that actually bite: the rail day crossing
>   midnight (CIF schedules carry past-midnight calling points, and
>   `service_date` is a rail day, not a calendar day), and a CIF delivery
>   that lands late — where a 1-day window could delete the only published
>   day shortly before its replacement arrives.
>
> Two days of this table is roughly 750,000 rows and ~80-120MB including the
> index, which is unremarkable for Postgres to hold.

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in
`crates/aggregator/src/queries.rs`, immediately after
`prune_trust_event_backlog_deletes_only_rows_older_than_the_retention_window`
(which ends at line 3138). It mirrors that test's structure exactly — same
`#[ignore]` shape, same inline `PgPoolOptions` connect, same seed-two-rows /
assert-one-pruned / assert-the-other-survives / explicit-`DELETE`-cleanup
body — with the one necessary difference that this table is pruned by a
`DATE` column against `CURRENT_DATE`, not by a `TIMESTAMPTZ` against `NOW()`,
so the fixture dates are computed relative to today rather than hardcoded.

```rust
    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
                prune_schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn prune_schedule_destination_departures_deletes_only_rows_older_than_the_retention_window()
    {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.unwrap();

        // Relative to CURRENT_DATE, not hardcoded: the predicate is
        // `service_date < CURRENT_DATE - $1`, so a fixed date would flip
        // this test's meaning as the calendar moved.
        let today = chrono::Utc::now().date_naive();
        let stale = today - chrono::Duration::days(5);
        let fresh = today - chrono::Duration::days(1);

        for (service_date, train_uid) in [(stale, "TEST-PRUNE-OLD"), (fresh, "TEST-PRUNE-NEW")] {
            sqlx::query(
                "INSERT INTO schedule_destination_departures \
                    (service_date, destination_crs, scheduled, train_uid, origin_crs) \
                 VALUES ($1, 'ZRB', '08:00:00', $2, 'EUS')",
            )
            .bind(service_date)
            .bind(train_uid)
            .execute(&pool)
            .await
            .expect("seed fixture rows");
        }

        let pruned = prune_schedule_destination_departures(&pool, 2)
            .await
            .expect("prune");
        assert_eq!(
            pruned, 1,
            "only the 5-day-old row should be pruned at a 2-day retention"
        );

        let remaining: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM schedule_destination_departures \
             WHERE train_uid = 'TEST-PRUNE-NEW'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            remaining.0, 1,
            "yesterday's rows are inside a 2-day window and must survive"
        );

        sqlx::query(
            "DELETE FROM schedule_destination_departures \
             WHERE train_uid IN ('TEST-PRUNE-OLD', 'TEST-PRUNE-NEW')",
        )
        .execute(&pool)
        .await
        .ok();
    }

    #[tokio::test]
    #[ignore = "requires a live database; run with `DATABASE_URL=... cargo test -p aggregator \
                prune_schedule_destination_departures -- --ignored --test-threads=1`"]
    async fn prune_schedule_destination_departures_never_deletes_todays_rows() {
        // The discriminating case, and the one that would actually hurt: a
        // retention window is only ever allowed to reach into the PAST.
        // Deleting today's rows would blank the live search between one CIF
        // delivery and the next, which no retention value should ever be
        // able to do.
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set to run this test");
        let pool = PgPoolOptions::new().connect(&database_url).await.unwrap();

        let today = chrono::Utc::now().date_naive();
        sqlx::query(
            "INSERT INTO schedule_destination_departures \
                (service_date, destination_crs, scheduled, train_uid, origin_crs) \
             VALUES ($1, 'ZRB', '08:00:00', 'TEST-PRUNE-TODAY', 'EUS')",
        )
        .bind(today)
        .execute(&pool)
        .await
        .expect("seed today's fixture row");

        // Even at the most aggressive value this config field allows.
        prune_schedule_destination_departures(&pool, 0)
            .await
            .expect("prune");

        let remaining: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM schedule_destination_departures \
             WHERE train_uid = 'TEST-PRUNE-TODAY'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            remaining.0, 1,
            "today's rows must survive any retention value -- the predicate is strictly \
             `service_date < CURRENT_DATE - $1`, never `<=`"
        );

        sqlx::query(
            "DELETE FROM schedule_destination_departures WHERE train_uid = 'TEST-PRUNE-TODAY'",
        )
        .execute(&pool)
        .await
        .ok();
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p aggregator prune_schedule_destination_departures -- --ignored`
Expected: FAIL with a compile error — `prune_schedule_destination_departures`
is not defined.

- [ ] **Step 3: Add the prune function**

In `crates/aggregator/src/queries.rs`, immediately after
`prune_trust_event_backlog` (which ends at line 511):

```rust
/// Prunes `schedule_destination_departures` rows for service dates older
/// than `retention_days`.
///
/// **This table is the one CIF-derived published product that genuinely
/// needs pruning**, and that is a deliberate divergence from its sibling
/// `schedule_network_departures`, which has no pruning job anywhere in this
/// repo. The sibling's wholesale replace is scoped per `(crs,
/// service_date)` over ~2,500 CRS codes, so its steady-state size is
/// trivial. This one holds ONE ROW PER DEPARTURE -- roughly 377,000 rows
/// per service date -- and its wholesale replace is scoped to a single day,
/// so without this job every day the service has ever seen accumulates
/// forever. See
/// docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
/// §3, "Retention becomes required".
///
/// Nothing reads a past service date: every read of this table computes
/// `today` server-side (`crates/api/src/routes/trains.rs`), so the window
/// exists to protect the PRODUCER's edges, not a consumer -- see
/// `Config::schedule_destination_departures_retention_days` for why the
/// default is 2 rather than 1.
///
/// Modelled on `prune_history` and `prune_trust_event_backlog` directly
/// above, with the one difference that this table's age column is a `DATE`
/// (`service_date`, a rail day) rather than a `TIMESTAMPTZ`, so the
/// comparison is against `CURRENT_DATE`. The comparison is strictly `<`,
/// never `<=`: today's rows must survive any retention value, including 0.
///
/// A single unbatched `DELETE`, unlike `prune_trains` a little further down
/// -- which loops in `PRUNE_TRAINS_BATCH`-sized chunks. That is the right
/// call here and worth stating: this deletes at most one service date's
/// worth of rows per run once the window is steady, it runs against a table
/// nothing reads for past dates, and the aggregator's cycle is a forgiving
/// place to spend the time. If lock duration or WAL volume ever does bite,
/// the addendum's §7 item 5 names `service_date` partitioning with a
/// partition swap as the standard mitigation -- reach for that rather than
/// for a batching loop.
pub async fn prune_schedule_destination_departures(
    pool: &PgPool,
    retention_days: i64,
) -> Result<u64> {
    let result = sqlx::query(
        "DELETE FROM schedule_destination_departures \
         WHERE service_date < CURRENT_DATE - ($1 || ' days')::interval",
    )
    .bind(retention_days.to_string())
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}
```

- [ ] **Step 4: Add the config field**

In `crates/aggregator/src/config.rs`, immediately after
`trains_retention_days` (which ends at line 108):

```rust
    /// How long to keep `schedule_destination_departures` rows before
    /// pruning them, in whole service dates.
    ///
    /// **2, not 1**, and the difference matters. 1 would match
    /// `trust_event_backlog_retention_days` above, but that default exists
    /// to enforce an RDM licensing safeguard for TRUST Train Movements
    /// data -- a constraint that does not apply to CIF SCHEDULE timetable
    /// data at all. Copying the number would copy a restriction that isn't
    /// real here while giving up the margin that is: `service_date` is a
    /// RAIL day, which crosses midnight, and a CIF delivery can land late,
    /// so a 1-day window can delete the only published day shortly before
    /// its replacement arrives. 2 covers both edges.
    ///
    /// Nothing reads a past service date -- every read computes `today`
    /// server-side -- so this window protects the producer's edges, not a
    /// consumer, and there is no reason to raise it further. At ~377,000
    /// rows per day, 2 days is ~750,000 rows and ~80-120MB with the index.
    ///
    /// Unlike `trust_event_backlog_retention_days` there is deliberately NO
    /// warning emitted when this is configured higher: nothing legal is at
    /// stake, only disk.
    #[arg(long, env, default_value_t = 2)]
    pub schedule_destination_departures_retention_days: i64,
```

- [ ] **Step 5: Wire it into the existing prune cycle**

Three edits in `crates/aggregator/src/main.rs`, all of them mechanical.

First, the `run_cycle` signature (currently line 185, after
`trains_retention_days`) — note the function already carries
`#[allow(clippy::too_many_arguments)]`, so no new attribute is needed:

```rust
    trains_retention_days: i64,
    schedule_destination_departures_retention_days: i64,
```

Second, the call site inside the poll loop (currently line 74, after
`config.trains_retention_days`):

```rust
            config.trains_retention_days,
            config.schedule_destination_departures_retention_days,
```

Third, the prune call itself, immediately after the `prune_trains` block
(currently lines 269-273), matching that block's metric-emitting shape
exactly:

```rust
    // The CIF-derived destination-search table -- the one published product
    // in this repo that genuinely accrues (~377,000 rows per service date,
    // one row per departure) rather than wholesale-replacing a bounded key
    // space. See queries::prune_schedule_destination_departures' own doc
    // comment, and
    // docs/superpowers/specs/2026-09-07-train-listing-destination-search-sizing-design.md
    // §3.
    let schedule_destination_departures_pruned = queries::prune_schedule_destination_departures(
        pool,
        schedule_destination_departures_retention_days,
    )
    .await?;
    metrics::counter!(common::metrics::metric_name(
        "aggregator_schedule_destination_departures_rows_pruned_total"
    ))
    .increment(schedule_destination_departures_pruned);
```

- [ ] **Step 6: Add the chart env var and its value**

Three of the aggregator's four retention settings are already chart-exposed
rather than left on their compiled-in defaults, so this one is too.

In `charts/distant-signal/templates/aggregator-deployment.yaml`, immediately
after the `TRUST_EVENT_BACKLOG_RETENTION_DAYS` block (lines 91-92):

```yaml
            # Train-listing-page plan, Task 13. NOT a licensing safeguard,
            # unlike the setting directly above -- purely disk. See
            # Config::schedule_destination_departures_retention_days
            # (crates/aggregator/src/config.rs) for why the default is 2
            # rather than 1.
            - name: SCHEDULE_DESTINATION_DEPARTURES_RETENTION_DAYS
              value: {{ .Values.aggregator.scheduleDestinationDeparturesRetentionDays | quote }}
```

And in `charts/distant-signal/values.yaml`, immediately after
`trustEventBacklogRetentionDays: 1` (line 660):

```yaml
  # -- How long to keep schedule_destination_departures rows, in whole
  # service dates. This table holds one row per departure (~377,000 per
  # day) and its wholesale replace is scoped to a single service date, so
  # unlike schedule_network_departures it genuinely accrues and genuinely
  # needs pruning. 2 rather than 1 to stay safe across the rail day's
  # midnight boundary and a late CIF delivery; nothing reads a past
  # service date, so there is no reason to raise it.
  scheduleDestinationDeparturesRetentionDays: 2
```

- [ ] **Step 7: Run the tests to verify they pass**

Run:
`DATABASE_URL=postgres://postgres:postgres@localhost:5432/postgres cargo test -p aggregator prune_schedule_destination_departures -- --ignored --test-threads=1`
Expected: PASS — both new tests.

Then the full crate, to prove the `run_cycle` signature change compiles
everywhere it is called:

Run: `cargo test -p aggregator`
Expected: PASS, no regressions.

Then the chart lint, so the template edit is proven syntactically valid:

```bash
helm lint charts/distant-signal
```

Expected: `1 chart(s) linted, 0 chart(s) failed`.

- [ ] **Step 8: Commit**

```bash
git add crates/aggregator/src/queries.rs crates/aggregator/src/config.rs crates/aggregator/src/main.rs charts/distant-signal/templates/aggregator-deployment.yaml charts/distant-signal/values.yaml
git commit -m "Prune schedule_destination_departures on the aggregator's existing cycle"
```

---
## Self-review notes (re-performed on this plan after the 2026-09-07 revision)

This section replaces the original plan's self-review. It re-runs the same
four checks, now against **both** source documents, plus a fifth covering the
revision itself.

**0. Revision coverage — the addendum's §5 table, row by row.** All thirteen
tasks are accounted for and match it:

| Addendum §5 says | This plan now has |
|---|---|
| **1** changed, re-scoped: measures `Σ_d bucket_d`, no cap, chooses single vs. chunked POST, still controller-run, no commit, **do not stop** | Task 1, retitled "Publish-volume diagnostic"; Step 2 takes the **sum** of the histogram's third column; Step 3's gate is single-POST vs. chunked-POST with the `rows.chunks(50_000)` code written out; the stop-and-flag branch is gone; the header states it no longer gates Task 4 |
| **2** changed: the five-column flat table with the composite PK, no `departures JSONB`; the "RETENTION: none" paragraph rewritten to point at the new pruning task | Task 2, DDL copied verbatim from addendum §3; header comment rewritten, with a RETENTION paragraph naming `prune_schedule_destination_departures` and `Config::schedule_destination_departures_retention_days` |
| **3** kept, one correction: signature, record and all seven tests verbatim; the `MAX_DEPARTURES_PER_DESTINATION` sentence must cite the addendum instead | Task 3, header note stating exactly that; Steps 1-3 and 5-7 untouched; only Step 4's closing doc-comment paragraph rewritten |
| **4** changed: constant **deleted**; `schedule_destination_departures_rows` becomes a flatten; two cap tests go, the third becomes "one row per departure carrying its destination"; `now` → `NaiveTime::MIN`; **Steps 4, 5, 6 unaffected**; no longer gated on Task 1 | Task 4, all of it; Steps 4 (wiring), 5 (config field) and 6 (Helm env var) carried through unchanged; three replacement tests; an explicit "do not add a constant" callout |
| **5** changed: flat scalar fields, `DELETE`+`UNNEST` in one transaction, indexed range scan with keyset cursor, `after` in the signature, day-scoped existence probe, `db_tests` rewritten, `ZRB`-`ZRF` still holds | Task 5, rewritten end to end; twelve tests, all `ZRB`-`ZRF` |
| **6** kept, one conditional: path, method, authorization, `UpsertResponse`, `app.rs` registration unchanged; only the body type follows Task 5; first-chunk-clears-the-day **iff** Task 1 forces chunking | Task 6; Steps 3, 4 (bar its doc comment), 5 unchanged; the conditional is written as a conditional, with an explicit "do not build either speculatively" |
| **7** changed additively: path, `destination`/`origin`/`from`/`to`, the camelCase wire shape, `destination_departure_json` and **both** render tests verbatim; add `limit` (default 50, max 200) and an opaque `after`; response becomes `{results, nextCursor}` | Task 7; Steps 1-3 untouched; `limit`/`after`/envelope added in Steps 4 and 6 |
| **8, 9, 11, 12** unaffected | Untouched. Verified by `git diff` producing **no hunks** inside any of the four sections |
| **10** changed minimally, frontend-only: read `{results, nextCursor}`, render a "Load more" that re-requests with `after=` | Task 10; everything else carried through; six added tests |
| **NEW** `prune_schedule_destination_departures`, modelled on `prune_history`, config field, prune-cycle wiring, a `db_tests` test | Task 13 |

**1. Spec coverage.**

*Against the original design doc* — unchanged from the previous review
except where the addendum overrides, so only the deltas are restated here:

- **§2 Goal and scope.** Every verdict in the realistic-filter table still
  holds: Origin — yes, optional; Date — out of scope, no date parameter
  anywhere; Time-of-day range — the inclusive `from`/`to` pair, now real
  `NaiveTime` values against a real `TIME` column; Destination — Tasks 2-7;
  Operator — not built, with Task 10's `renders no operator filter at all`
  test still guarding the omission. The "live-status summary" bullet remains
  deliberately absent for the same reason as before.
- **§3 Approach B.** The *product* decision is untouched; its *mechanism*
  sentences ("published… into a new table, copy-adjacent to
  `schedule_network_departures`'s own migration and POST/GET route shape",
  and "a decision on the result cap's shape") are the two the addendum
  supersedes, and this plan now follows the addendum on both. Its "needs its
  own sizing pass, not an assumed reuse of `MAX_DEPARTURES_PER_STATION = 10`"
  is Task 1 — which **was run**, and whose answer was "no cap".
- **§4, §5.** Untouched by the revision; Tasks 8, 9, 11 and 12 are unchanged
  and the previous review's findings for them stand as written.
- **§6.** All ten bullets remain in Global Constraints and none is
  contradicted. Re-checked specifically against the new work: Task 5 reads
  Postgres only; Task 13 touches the aggregator's own prune cycle and no
  poller; no task adds a date parameter, decodes an operator, holds a
  resident index, or merges two sources.
- **§7 Open questions.** 1 is resolved by Task 1 and then *re-resolved* by
  the addendum (there is no cap); 2 by Task 8; 3 is void under a CIF-only
  v1; **4 is now resolved by Task 13 rather than by inspection, reversing
  the original answer**, and both "Decisions this plan resolves" item 3 and
  Task 2's migration header say so explicitly; 5 is Task 11; 6 is inherited
  and untouched.

*Against the addendum:*

- **§1.1 measurement, §1.2 the corrected `Σ` bound, §1.3 once-per-delivery.**
  All three are load-bearing in the plan rather than merely cited: §1.1 is
  the reason Task 4 deletes the constant and the reason Task 4's
  uncapped-bucket test uses 9,634 specifically; §1.2 is Task 1's whole
  re-scoping; §1.3 is why Task 4 passes `NaiveTime::MIN` and why Task 7 owns
  the `now` boundary.
- **§3 Approach C.** The DDL, the read query, the `UNNEST` upsert, the
  existence probe and the chunking fallback are all reproduced as real code.
- **§6 out of scope.** Five of its bullets were not in the original Global
  Constraints and are now added there verbatim in substance (no `COPY`
  loader, no partitioning, no consolidating the two tables, no fixing the
  sibling's staleness, no cap under any name).
- **§7 open questions/risks.** 1 → Task 1's gate. 2 (`NaiveTime::MIN` and
  midnight-crossing schedules) → named in Task 4's doc comment as "made more
  observable, not made worse", inherited unchanged from `departures_by_crs`.
  3 (the 404 semantics change) → **taken as an explicit approval**, argued in
  Task 5's Interfaces block and pinned by two tests in each of Tasks 5 and 7.
  4 (retention default) → decided: **2 days**, with the reasoning written
  into Task 13, the config field's doc comment and `values.yaml`. 5 (write
  pressure) → named in Task 2's migration comment and Task 13's function
  doc, with partitioning named as the mitigation and deliberately not built.
  6 (deriving the sibling table) → Global Constraints, explicitly deferred.
  7 (VSTP) → inherited, untouched.

**2. Placeholder scan.** No "TBD", "TODO", "implement later", "add
appropriate error handling", "similar to Task N", or "write tests for the
above" appears anywhere. Every code step contains literal code, including
every step this revision rewrote. Two notes:

- **The one value previously flagged as "decided outside the plan text" is
  gone.** The old review's closing paragraph defended
  `MAX_DEPARTURES_PER_DESTINATION = 200` as a real default with a gate. That
  constant no longer exists, and nothing has replaced it: there is now **no
  value anywhere in this plan that Task 1 decides**. Task 1 chooses between
  two fully-written code paths (a single `post_batch` call in Task 4 Step 3,
  or the chunked loop in Task 1 Step 3), which is a branch, not a blank.
- **Task 6's conditional is a conditional, not a placeholder.** It states
  the exact semantics to add, names where they would go, and says explicitly
  not to build them speculatively. The default path is complete without it.

Four things this pass found and fixed inline rather than reported:

- **The addendum's four-key publish row cannot work.** Its §3 sketches
  `{destination_crs, origin_crs, scheduled, train_uid}`, but the ingest
  handler's first statement is `DELETE ... WHERE service_date = $1` and
  `common::ingest::post_batch` posts a bare array with nowhere else to carry
  the day. Deriving it from the receiving service's clock would reintroduce
  the clock-coupling this whole design moves out of the publish. `service_date`
  is therefore a fifth per-row key, matching the table's own column — and the
  per-entry size budget rises from the addendum's ~55 bytes to **~80**, so
  Task 1's threshold is now ~750,000 entries rather than ~1,000,000, and the
  expected payload ~30MB rather than ~21MB. Still ~3.3x inside the router's
  100MB limit. Stated in Task 4's Interfaces block and Task 1 Step 3.
- **`legacy_backfill.rs` is not the `UNNEST` precedent.** The brief pointed
  at it; it has none. The real in-crate precedent is
  `crates/api/src/data/trains.rs`' `find_or_create_trains_batch` (`:50-75`)
  and `mark_trains_resolved_batch` (`:140-174`), whose
  build-parallel-`Vec`s-then-`.bind()` style Task 5 follows exactly.
  `legacy_backfill.rs` *is* cited correctly elsewhere — `prune_trains`' own
  `PRUNE_TRAINS_BATCH` doc comment names it as its batching precedent — which
  is probably where the confusion came from.
- **The day-scoped existence probe breaks test isolation, in two different
  ways.** Under the old per-destination bucket, tests could share a
  `service_date` and stay independent; they cannot now. Task 5's tests
  therefore each own a **distinct `service_date` in 2099** — far-future so a
  shared development database's real published data cannot answer their
  "is anything published?" question either — and Task 7's route tests, which
  are forced onto the real "today", delete the **whole day** rather than one
  destination's rows and carry an explicit note that they need a database
  `schedule-reference` has not published into. Both modules' doc comments
  state the reasoning.
- **Task 7's route tests could not use fixed wall-clock times.** With the
  `now`-forward filter moved to request time, a fixture at "08:22" passes in
  the morning and returns nothing in the afternoon. The tests now compute
  fixtures relative to London-local `now` via a `relative_times()` helper,
  with an explicit assertion telling the runner to re-run outside
  23:00-00:01. This is a direct, non-obvious consequence of the addendum's
  §1.3 that nothing in the addendum calls out.

One further correctness fix, worth separating because it is a bug rather
than a plan-mechanics issue: **Task 7 computes `now` in `Europe/London`, not
UTC.** The stored `scheduled` values are London local civil time straight
off the CIF body (`DestinationDeparture::scheduled`'s own doc comment says
so), so a UTC time-of-day comparison would be an hour wrong all summer.
`chrono-tz` is already a direct dependency of `crates/api` and
`chrono_tz::Europe::London` is already used in `data/eta_blend.rs` for the
same reason, so this costs nothing. `today` remains
`chrono::Utc::now().date_naive()`, unchanged and matching
`get_station_schedule_departures` — deliberately not widened here.

Two comments inside Task 7's *render* code describe its input as "an opaque
JSONB element". Those are left **verbatim**, as the addendum requires, and
they remain correct in the sense that matters: `destination_departure_json`
still receives a `serde_json::Value` whose schema it does not own and must
not assume, which is exactly what its defensive null-handling test exists to
prove. Only the value's *producer* changed.

**3. Type and signature consistency across all thirteen tasks.**
- `DestinationDeparture { uid, origin_crs, scheduled }` (Task 3) is what
  Task 4's flatten reads, emitting JSON keys
  `service_date`/`destination_crs`/`scheduled`/`train_uid`/`origin_crs` —
  which is exactly, field for field, Task 5's
  `ScheduleDestinationDeparturesRow`, which is what Task 6's handler
  deserializes and Task 6's test body posts. Note the deliberate rename at
  that boundary: `DestinationDeparture::uid` becomes the JSON/column
  `train_uid`, and Task 4's test asserts the absence of a stray `uid` key.
- Task 5's `search_schedule_destination_departures(pool, destination_crs,
  service_date, scheduled_from, origin_crs, to_time, after, limit) ->
  Result<Option<DestinationDeparturePage>>` is called with exactly that
  arity and those types by Task 7, whose `let … else` destructures the
  `Option` into the 404 branch.
- `to_time` is `Option<chrono::NaiveTime>` on both sides, and Task 7's
  `normalize_time` was changed to return `NaiveTime` rather than the old
  `String` so it does. `scheduled_from` is non-optional on both sides.
- `DestinationDepartureCursor { scheduled, train_uid, origin_crs }` (Task 5)
  is constructed by Task 5's query, asserted structurally by Task 5's
  `search_applies_the_limit_and_returns_a_cursor_for_the_rest`, and
  encoded/decoded by Task 7's `encode_cursor`/`decode_cursor` — which are
  exact inverses over the same three fields in the same order.
- `DestinationDeparturePage { departures, next_cursor }`'s `departures`
  elements are `{uid, origin_crs, scheduled: "HH:MM:SS"}`, which is exactly
  what Task 7's **unchanged** `destination_departure_json` reads and what
  Task 5's `search_with_no_filters_…` test asserts literally.
- `destination_departure_json(d, destination_crs)` emits
  `{uid, scheduled: "HH:MM", originCrs, destinationCrs}` — exactly Task 10's
  `TrainSearchRow` interface and exactly what Task 10's fixtures produce.
- The envelope `{results, nextCursor}` is produced by Task 7's handler,
  asserted by Task 7's `results()`/`next_cursor()` test helpers, and
  consumed by Task 10's `TrainSearchResponse` — same two field names, same
  `string | null` for the cursor, on both sides.
- `prune_schedule_destination_departures(pool, retention_days) ->
  Result<u64>` (Task 13) matches `prune_history`/`prune_trust_event_backlog`
  exactly, and its new `run_cycle` parameter is added in the same position
  in the signature and at the call site.
- `TrackThisTrainButton({ uid, date, attachTicketId?, size? })` (Task 9) is
  called by Task 10 with all four props and by Task 12 with two — unchanged.
- `TrainSearchForm({ initialDestination?, initialOrigin?, attachTicketId? })`
  (Task 10) is called by Task 11 with exactly those three — unchanged. The
  pagination state is entirely internal, so Task 11 needs no edit at all.
- `create_subscription_for_train(pool, trains_id, user_id) ->
  anyhow::Result<i64>` (Task 8) keeps its exact existing signature.
- Route path segments agree everywhere:
  `/private/schedule-destination-departures` (Task 6 route, Task 4 config
  default, Task 6 `app.rs` auth entry, Task 4 Helm env);
  `/public/trains/search` (Task 7) reaches the frontend as
  `/api/trains/search` through the proxy's `/public/` prefixing rule, which
  Task 10 uses verbatim including its new `after=` parameter.
- Env var and Helm value names agree: `SCHEDULE_DESTINATION_DEPARTURES_URL`
  ↔ `schedule_destination_departures_url` (Task 4);
  `SCHEDULE_DESTINATION_DEPARTURES_RETENTION_DAYS` ↔
  `schedule_destination_departures_retention_days` ↔
  `aggregator.scheduleDestinationDeparturesRetentionDays` (Task 13).

**4. Ordering check.** The revision loosened the graph rather than tightening
it:

- **Task 1 now gates nothing.** It was the plan's only hard gate; Task 4 no
  longer depends on its output for any value, only for a choice between two
  written code paths, and Task 4's default is complete without it. Tasks 2,
  3 and 13 never depended on it.
- Strict data dependencies: 2→5→6, 2→5→7, 2→13, 3→4.
- Task 8 gates Tasks 9-12 (stated in Task 8's own header and in "Decisions
  this plan resolves"). Task 9 gates Tasks 10 and 12. Task 10 gates Task 11.
- **Task 13 is a leaf on both sides**: it needs only Task 2 and nothing
  needs it, so it can run in parallel with all of Tasks 3-12.
- Tasks 2 and 3 are independent of each other and of everything else, so
  work can begin immediately on either.

**5. Global Constraints self-check.** Every new query uses runtime-checked
`sqlx::query`/`query_as` — including Task 5's `UNNEST` insert and Task 13's
`DELETE`, neither of which uses a `query!` macro or the `.sqlx` cache. Every
new JSON response goes through a hand-built `json!()`; Task 7's envelope is
built that way too rather than by deriving `Serialize` on a new struct. Every
live-DB test uses the `#[tokio::test] #[ignore]` + manual pool pattern with
explicit `DELETE` cleanup and no `#[sqlx::test]` — with the per-file naming
difference now called out where it bites (`test_pool()` in
`data/queries.rs`, `connect()` in `routes/*.rs` and
`aggregator/src/queries.rs`'s inline form). The fixture CRS codes claimed are
still exactly `ZRB`-`ZRF`, no new ones, and Task 13 reuses `ZRB`. No task
introduces a cap under any name; no task adds a `COPY` loader, a partition,
or a change to `schedule_network_departures`.
