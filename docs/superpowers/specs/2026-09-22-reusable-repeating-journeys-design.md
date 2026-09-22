# Reusable & Repeating Journeys — Design Investigation

This is a design spec, not an implementation — no code was written to
produce it. It investigates a natural next step after journey tracking
(`docs/superpowers/specs/2026-09-22-journey-tracking-design.md`, all four
phases implemented on branch `integration-preview`, not yet on `main`):
letting a user avoid re-entering the same journey's criteria every time
they travel it again. Citations are file:line against
`integration-preview`'s actual shipped code (read via the checked-out
worktree at `/home/coder/Distant-Signal/.integration-worktree`, branch
`integration-preview` at `9e0efc45`), not the design docs or plans alone —
flagged **Speculative** anywhere I couldn't verify directly.

**A second naming collision, flagged up front, same posture as the parent
spec's own front-matter warning.** The parent spec already had to
disambiguate "Journey" (new, the multi-leg tracked thing) from the older,
narrower `journey.rs`/`JourneyTimeline` (a single train's calling-point
list). This document adds a *third* sense: a **template** — a saved shape
a user can stamp out new journeys from, either on demand or on a
schedule — which is deliberately **not** a `journeys` row itself (a
template has no legs bound to real trains, no `service_date`, and
typically outlives any one journey stamped from it). Recommend a distinct
new noun rather than overloading "journey" a third time — this doc uses
**"journey template"** (Rust type `JourneyTemplate`/`JourneyTemplateLeg`,
table `journey_templates`/`journey_template_legs`, route prefix
`/JourneyTemplates`) throughout, and calls the naming choice out again in
§7's open questions for explicit product-owner sign-off, exactly as the
parent spec did for its own collision.

---

## 0. What already exists (baseline)

### 0.1 The shipped journey schema — every field is instance-shaped, not template-shaped

Confirmed against the actual migrations on `integration-preview`
(`crates/api/migrations/20260922090000_journeys.sql`,
`20260922100000_journeys_historical_migration.sql`,
`20260922110000_group_journeys.sql`,
`20260922130000_journey_leg_notification_state.sql`):

```
journeys
  id          BIGSERIAL PRIMARY KEY
  user_id     TEXT NOT NULL REFERENCES users(id)
  custom_name TEXT
  created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()

journey_legs
  id                     BIGSERIAL PRIMARY KEY
  journey_id             BIGINT NOT NULL REFERENCES journeys(id) ON DELETE CASCADE
  leg_order              INT NOT NULL                     -- UNIQUE (journey_id, leg_order)
  origin_crs             TEXT                             -- nullable (knownTrain-mode legs)
  destination_crs        TEXT                             -- nullable
  service_date            DATE NOT NULL                   -- ONE concrete calendar day, always
  depart_after/before     TIME                             -- nullable window bounds
  arrive_after/before     TIME                             -- nullable window bounds
  train_subscription_id  BIGINT REFERENCES train_subscriptions(id) ON DELETE SET NULL
  match_mode             TEXT NOT NULL DEFAULT 'unmatched'
                          CHECK (match_mode IN ('unmatched', 'manual', 'auto'))
  created_at             TIMESTAMPTZ NOT NULL DEFAULT NOW()
```

The load-bearing fact for this whole document: **`journey_legs.service_date`
is `NOT NULL DATE`.** Every leg — matched or not, window-search or
knownTrain — is permanently anchored to exactly one calendar day
(`crates/api/src/data/journeys.rs:55`, and every leg-creation function,
e.g. `create_journey_with_window_leg` at `journeys.rs:396-419`, takes a
single `service_date: NaiveDate` parameter, never a range or a weekday
set). There is **no field anywhere in this schema that can express "every
weekday"** — not a day-of-week bitmask, not a `valid_from`/`valid_until`
pair, nothing. This is the concrete gap the task brief predicted, now
confirmed by reading the actual column, not inferred from the design doc.

### 0.2 `match_mode = 'auto'` — confirmed present in the `CHECK` constraint, absent from every code path

Grepping the full `integration-preview` tree for `match_mode`/`'auto'`
(`crates/api/src/data/journeys.rs`, `crates/api/src/routes/journeys.rs`,
`crates/notifier/`) turns up the literal string `'auto'` in exactly one
place: the migration's own `CHECK (match_mode IN ('unmatched', 'manual',
'auto'))` (`20260922090000_journeys.sql`). Every function that *writes* a
`match_mode` value writes either `'unmatched'` (leg created with an open
window, `journeys.rs:412`) or `'manual'` (leg created from a known train,
or committed/re-committed via `set_leg_train_subscription`,
`journeys.rs:494`) — never `'auto'`. Every route-level test that asserts
on `match_mode` asserts `"manual"` or `"unmatched"`
(`crates/api/src/routes/journeys.rs:1638`,
`crates/api/src/data/journeys.rs:830,866,957,1153-1154,1337,1407`). The
Phase 1 plan's own module doc comment says this in as many words: `'auto'`
"stays a … [reserved value], not currently reachable from any code this
plan adds" (`docs/superpowers/plans/2026-09-22-journey-tracking-phase1-single-leg-migration-plan.md:218-220`).
**`'auto'` is schema-reserved, not implemented** — exactly as the task
brief described, now confirmed by reading the code rather than assuming
the plan matched what shipped.

### 0.3 No template/preset/recurrence concept exists anywhere in this codebase

A repo-wide search (`crates/`, `frontend/`, `docs/superpowers/specs/`) for
`template|preset|recurring|recurrence|repeat|schedule_rule|cron|rrule`
(case-insensitive) turns up **zero** product-facing hits — every match is
either test-fixture noise (`wiremock::ResponseTemplate`), the aggregator's
own *incident*-recurrence handling (`crates/aggregator/src/aggregation.rs`'s
`has_recurring_schedule`, which detects a recurring *service-disruption
notice* in NR's incident feed — a read-only classification of upstream
data, nothing a user creates or owns), or plain-English comments using
"repeat" as an ordinary word. **There is no "saved search," "preset," or
"template" pattern anywhere in this app to model from — this would be the
first of its kind.** The closest analogue is the single-train tracking
`custom_name`/rename feature (`rename_tracked_train`,
`crates/api/src/data/train_tracking.rs:1371`) — but renaming an *existing*
tracked row is not the same operation as spawning a *new* one from a
saved shape; it establishes only that this app is comfortable with a
free-text label a user attaches to a personal resource, nothing about
reuse-to-create.

### 0.4 The periodic-sweep pattern is the real precedent for "runs on its own schedule," not any cron/CronJob infra

No Kubernetes `CronJob` or OS-level cron exists anywhere in this repo's
`charts/`/`docker/` (checked directly — no hits). Every "do this
periodically without a human click" behavior in this codebase today is a
`tokio::time::interval` inside an already-long-running binary:
`train_tracking.rs`'s `list_pending_pins_for_schedule_match`/
`list_pending_pins_for_backlog_match` sweeps (consumed by whatever process
runs them — **Speculative**: not traced further, out of this
investigation's scope) and, more directly relevant, `crates/notifier`'s
own `main.rs` `tokio::select!` loop (§0.5 below) which already runs three
independent interval-driven branches in one process. **This is the
concrete precedent for "materialize today's occurrences of every active
recurring journey template" (§3): a fourth branch in an existing
interval-driven loop (most naturally `notifier`, since it already reads
`journeys`/`journey_legs` and already owns the "per-cycle DB sweep, best
effort, retried next cycle" idiom), not a new deployable, not new
infrastructure.**

### 0.5 Notifications — confirmed shape: escalation-only, per-row dedup, silent on match/commit

Read `crates/notifier/src/decision.rs` and
`crates/notifier/src/queries.rs` directly (not just the parent spec's
description of them, since notification behavior is the crux of this
doc's rolling-journey analysis):

- **`decide_train_notification(previous_rank, new_rank)`**
  (`decision.rs:83-89`) and **`decide_skip_notification(was_skipped,
  is_skipped)`** (`decision.rs:102-108`) are both escalation-only, no
  cooldown, no cold-start guard — confirmed by their own test names:
  `train_escalation_notifies_deescalation_does_not` (`decision.rs:203-209`),
  `a_newly_tracked_already_delayed_train_does_notify_once`
  (`decision.rs:212-217`), `a_leg_already_skipped_on_first_ever_check_notifies_once`
  (`decision.rs:245-250`).
- Dedup state is **per-row, not per-user-globally**:
  `train_notification_state` keys on `(user_id, tracked_train_id)`
  (parent spec §0.3), `journey_leg_notification_state` keys on `(user_id,
  journey_leg_id)` (`20260922130000_journey_leg_notification_state.sql`,
  PK line). Both are only ever upserted **after** a send succeeds
  (`queries.rs:333-358` for trains; the skip-state migration's own header
  comment restates the same discipline for legs).
- **Critically: nothing fires a notification on a leg being *matched* or
  *committed*, today, for either `'manual'` or a hypothetical `'auto'`
  leg.** `set_leg_train_subscription` (`journeys.rs:465-494`, the single
  function both first-pick and "Change train" re-pick call) is a plain
  `UPDATE`, with no notification side effect anywhere in its call graph —
  confirmed by grepping `crates/notifier/` for any read of
  `journey_legs.match_mode` or `.train_subscription_id` transition: the
  only two things the notifier polls per committed leg are (a) the bound
  `trains_id`'s delay/cancellation state via the *existing*,
  journey-unaware `candidates_for_trains_id` (parent spec §0.3, unchanged
  by journeys at all), and (b) `list_committed_legs_for_today`
  (`queries.rs:487-527`) feeding `skip_check.rs`'s Darwin-sample lookup.
  **"A leg just got a train" is not a notification class that exists
  today.** This matters directly for §4 below: it means "don't spam a
  fresh 'matched!' push every weekday" isn't something new to prevent —
  it's something to deliberately *not add* when `'auto'` is finally
  implemented, preserving a silence this app already has.
- **`journey_leg_for_train_subscription`** (`queries.rs:390-416`) is the
  one journey-aware notification building block that exists: given a
  `train_subscription_id`, it looks up `(journey_id, journey_name,
  leg_order, total_legs)` so the payload builder can say "Leg 2 of
  'Weekend in Edinburgh'…" — **confirmed implemented**, not just planned
  (return type at `queries.rs:363-364`, query at `queries.rs:395-416`).
- **Unmatched legs are invisible to the notifier by construction.** Both
  `candidates_for_trains_id` (fan-out keyed on a real `trains_id`) and
  `list_committed_legs_for_today` (`queries.rs:487-527`, whose own query
  — **Speculative** on the exact `WHERE`, not re-read line-by-line here,
  but its name and its only caller `skip_check`'s doc comment both
  confirm it) require a bound `train_subscription_id` to exist at all. A
  leg that never got matched — whether because no human picked one, or
  (§3) because an `'auto'` sweep found zero candidates for today's
  window — currently generates **zero** signal to the user, ever. This is
  a real, load-bearing gap for §4.2's proposed new notification class.

### 0.6 Group sharing — confirmed instance-scoped, no template-transitive path exists

`group_journeys` (`20260922110000_group_journeys.sql`) is `PRIMARY KEY
(group_id, journey_id)` — a grant against one concrete `journeys.id`.
`journey_readable_by` (`crates/api/src/data/journeys.rs:632-657`, tested
at `journeys.rs:1429-1592` — owner-always-readable, fellow-member-readable,
stranger-excluded, departed-member-loses-access, all four confirmed by
name) resolves "owner OR shared-into-a-group-I'm-in" for exactly one
`journey_id` at a time. **There is no concept today of a grant that
"follows" a journey that doesn't exist yet** — which is precisely the
problem a recurring journey creates, since (per §3's recommended
materialization model) each day mints a **new** `journeys.id`. This is
investigated in depth in §5.

### 0.7 No UK bank-holiday / non-trading-day calendar exists anywhere

Checked for any holiday-calendar concept (`bank_holiday`, `BankHoliday`,
`public_holiday`) across `crates/`: the only hits are TfL DLR timetable
parsing and a schedule-reference test fixture — both about *how National
Rail's own CIF schedule data encodes non-standard-day running*, not a
product-facing "skip this recurring journey on holidays" feature. If a
recurring journey's day-of-week rule should also respect bank holidays
(a real commuter expectation — "don't try to match my commute on Christmas
Day"), **that data does not exist in this app today** and would need
either a new small reference table or a deliberate non-goal. Flagged in
§7.

---

## 1. Disambiguating "reusable" vs. "rolling/repeating"

The brief is right that the product owner's phrasing doesn't distinguish
these, and right that the answer isn't obviously "these are the same
thing." Having read the actual schema and notifier code, here is the
determination:

**They are two different *trigger mechanisms* for the same underlying new
concept — a saved journey shape — and should share one data model, but
ship as clearly separable features with different UX and different
product risk.**

- **"Reusable"** = a *user-triggered* restart. The user looks at a past or
  current journey and says "do this again" *right now*, once, for a
  specific new date they pick. No schedule, no automation, no unattended
  matching decision — the user is present and will pick a date (and, if
  window-search, a candidate) the same way they do for any journey today.
  This is **low risk and cheap**: it needs no new automatic-matching
  policy decision (§2.3's deferred question stays deferred), no new
  notification class, and arguably no new schema at all for its simplest
  form (§4.1).
- **"Rolling/repeating"** = a *system-triggered*, unattended restart on a
  cadence, with (for the `'auto'`-mode case) the system also picking the
  candidate without the user present. This **does** require resolving the
  deferred auto-commit rule (§2.3 of the parent spec), **does** require a
  new recurrence concept (§0.1's confirmed gap), and **does** require new
  notification thinking (§4.2) — a user who is not looking at the app that
  morning must still find out if their commute failed to auto-match.

The unifying insight: both are "stamp a new `journeys` row from a saved
shape." A **journey template** (new entity, §2) is that saved shape. What
differs is only *who/what pulls the trigger and how often*:

| | Reusable | Rolling |
|---|---|---|
| Trigger | User clicks a button | A scheduled sweep, unattended |
| Cadence | Once, on demand | Every day matching a recurrence rule |
| Candidate pick | User picks (same as today) | System picks (`'auto'`, §3) — or still user-picked if the template is deliberately left `'manual'` (a "remind me every weekday to pick my train" hybrid, see §3.3) |
| New schema needed | Arguably none for an MVP (§4.1) | A recurrence rule, an active/paused flag, an end condition (§2) |
| New notification class | None | Yes — "today's auto-match failed" (§4.2) |

Recommend treating **"reusable" as the cheap, low-risk Phase 1** of this
feature area (§8), shippable independently of any recurrence work, and
**"rolling" as the substantial follow-on** that reuses the same template
entity but adds recurrence, the deferred auto-commit decision, and new
notification logic on top.

---

## 2. Data model

### 2.1 The reusable-only path needs no new table

The simplest version of "reusable" requires **zero backend schema
change**. `GET /Journeys/{id}` already returns every field needed to
pre-fill a new journey's creation form (`journeys.rs`'s
`JourneyDetailResponse`, confirmed shape at `routes/journeys.rs:594-621`
carrying `match_mode`, and the leg row's own `origin_crs`/
`destination_crs`/`depart_after`/`depart_before`/`arrive_after`/
`arrive_before`, §0.1). A **"Track this journey again" button** on the
journey detail page (`frontend/app/journeys/[id]/page.tsx`) can read the
current journey's own already-fetched data client-side and route to
`/journeys/new` with those fields pre-filled in local component state —
no new API surface, no new table, a pure frontend affordance analogous to
how `TrackThisTrainButton.tsx` already posts straight to a known-identity
creation route. **This is real, shippable value with no data-model risk**
and should not be gated on the recurrence work below (§8, Phase A).

The limitation: it only offers "repeat the journey I'm looking at right
now," not "manage a durable, named, reusable shape independent of any one
instance" (e.g. "my client-site trip," reusable six months from now after
every instance of it has long since scrolled off `/journeys/mine`). If the
product owner wants the latter — a genuinely persistent, independently
manageable saved shape — that needs §2.2's new entity even without any
recurrence.

### 2.2 The template entity (needed for durable "reusable" and required for "rolling")

```sql
CREATE TABLE journey_templates (
    id                  BIGSERIAL PRIMARY KEY,
    user_id             TEXT NOT NULL REFERENCES users(id),
    custom_name         TEXT,
    -- NULL = a one-shot template: exists only to be duplicated on demand
    -- (the durable form of "reusable", §2.1's persistent sibling).
    -- Non-NULL = a rolling journey: which days this template auto-
    -- materializes on. Stored as a bitmask (Mon=1 .. Sun=64) rather than a
    -- day-name array -- cheap to query ("is today's weekday bit set"),
    -- and avoids yet another free-text enum in this codebase.
    days_of_week        SMALLINT,               -- NULL, or 1..127
    active              BOOLEAN NOT NULL DEFAULT TRUE,   -- pause without deleting (§2.4)
    starts_on           DATE,                    -- NULL = "from whenever created"
    ends_on             DATE,                    -- NULL = "until I say stop" (§7 open question: is that the ONLY end condition, or also an occurrence count?)
    default_match_mode  TEXT NOT NULL DEFAULT 'manual'
                         CHECK (default_match_mode IN ('manual', 'auto')),
    auto_commit_rule    TEXT                     -- NULL unless default_match_mode='auto'; see §3.1
                         CHECK (auto_commit_rule IN ('earliest', 'nearest_to_now')),
    created_at          TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX journey_templates_user_id ON journey_templates (user_id);
-- Drives the daily materialization sweep (§3): "every active template
-- whose days_of_week bit for today is set, and today is within
-- [starts_on, ends_on]". A partial index on active=true is worth
-- considering once real row counts exist -- not asserted here as
-- necessary, since journey_templates is expected to be small per user
-- (no evidence otherwise).

CREATE TABLE journey_template_legs (
    id                  BIGSERIAL PRIMARY KEY,
    template_id         BIGINT NOT NULL REFERENCES journey_templates(id) ON DELETE CASCADE,
    leg_order           INT NOT NULL,
    origin_crs          TEXT,
    destination_crs     TEXT,
    -- No service_date -- a template leg is date-less by definition; the
    -- materialized journey_legs row gets today's (or the target) date at
    -- stamping time (§3.2).
    depart_after        TIME,
    depart_before        TIME,
    arrive_after         TIME,
    arrive_before         TIME,
    UNIQUE (template_id, leg_order)
);
CREATE INDEX journey_template_legs_template_id ON journey_template_legs (template_id);

-- Lineage: which template (if any) produced a given journey. Nullable and
-- ON DELETE SET NULL so deleting a template never cascades into deleting
-- journeys it already produced -- matches this codebase's established
-- "a deleted parent orphans its children's foreign key, never deletes
-- them" posture (journey_legs.train_subscription_id's own ON DELETE SET
-- NULL is the direct precedent, 20260922090000_journeys.sql).
ALTER TABLE journeys ADD COLUMN source_template_id BIGINT
    REFERENCES journey_templates(id) ON DELETE SET NULL;
CREATE INDEX journeys_source_template_id ON journeys (source_template_id);
```

**Why a new table pair, not folding onto `journeys`/`journey_legs`
themselves** — the same reasoning the parent spec used to justify
`journey_legs` not folding onto `train_subscriptions` (§1.1 there): a
template leg has no `service_date` (`journey_legs.service_date` is `NOT
NULL` and every existing reader — the migration, every leg-creation
function, the notifier's `list_committed_legs_for_today` — assumes a
concrete date exists), no `train_subscription_id` (a template is never
itself bound to a real train), and needs fields (`days_of_week`, `active`,
`ends_on`) that would be meaningless `NULL`s on every ordinary, non-template
journey. Keeping templates on their own tables means **zero changes** to
`journeys`/`journey_legs` beyond the one additive, nullable
`source_template_id` lineage column — every existing route, the notifier,
tickets, and `group_journeys` sharing keep working byte-for-byte.

**Why one `journey_templates` row (not one per leg) mirrors `journeys`
exactly, matching a real product need**: a multi-leg commute ("drive to
the station, train to the interchange, train to the office") needs its
whole shape reusable/recurring as one unit, not leg-by-leg — same reason
`journeys` groups legs today.

### 2.3 `default_match_mode`/`auto_commit_rule` finally gives `'auto'` a real meaning — but only at the template level

This is the resolution this document proposes for the parent spec's
deferred Open Question #1 (§2.3 there): **`journey_legs.match_mode =
'auto'` should mean "this leg's `train_subscription_id` was set by the
daily materialization sweep using its template's `auto_commit_rule`, not
by a human clicking a candidate."** It is set once, at materialization
time (§3.2), not re-evaluated continuously — an already-auto-matched
leg behaves identically to a `'manual'` one from that point on (same
notifier fan-out, same "Change train" re-pick affordance, since the
persisted window is unaffected). This keeps `'auto'` scoped to *how a
leg got its first candidate*, never a standing "keep re-deciding this
leg forever" mode — which avoids reopening the ambiguity the parent spec's
Open Question #1 flagged about "which candidate wins" needing to be
re-litigated on every poll cycle; it's decided exactly once, when the
day's occurrence is minted.

### 2.4 Pause/resume and "active until" — confirmed as new concepts, no precedent to reuse

`active` (§2.2) is the pause switch — set `false` to stop materializing
new occurrences without deleting the template or its history (its past
`journeys` rows, `notification_state` rows, and tickets are untouched,
matching this app's consistent "orphan, don't cascade-delete, a personal
resource's history" posture: `train_subscriptions` has no pruning at all
per the parent spec §0.1, and `journey_legs.train_subscription_id`'s `ON
DELETE SET NULL` is the same instinct one level up). `ends_on` is a
one-time, date-known-in-advance stop (e.g. "I know this contract ends
15 December"); `active=false` is the "until I say stop, and I don't know
when that is yet" toggle. Both are genuinely new — no existing table in
this app has an active/paused flag on a personal resource (`train_subscriptions`
has no such concept; a tracked train is either tracked or deleted, nothing
in between). Recommend surfacing both in the UI (§6) as a single "Pause"
button (toggles `active`) plus an optional "End on" date field, rather
than making the user understand two separate mechanisms.

### 2.5 Retention — a genuinely new problem this feature introduces

The parent spec's own baseline (§0.1 there) already flags that
`train_subscriptions` has **no retention/pruning job** and grows
unbounded, and treats that as acceptable because growth is driven by
manual user action (one row per click). **A rolling journey breaks that
assumption**: a single active daily-weekday template silently produces
~260 new `journeys` rows a year, each with its own `journey_legs`,
`journey_leg_notification_state`, and (if the user attaches tickets)
`tracked_train_tickets` rows, with **zero further clicks from the user**.
`MINE_LIST_LIMIT = 100` (parent spec §0.1, `train_tracking.rs:46`) already
caps one *response's* size, but a `/journeys/mine` list dominated by 260+
auto-generated same-shape rows a year, forever, for one recurring commute,
is a materially different scaling and UX problem than today's
manually-created rows. This is flagged explicitly as an open question
(§7) rather than resolved here — retention policy (archive after N days?
collapse into a single "history" view keyed by template rather than
listing every occurrence individually?) is a product decision, not
something this investigation should default silently.

---

## 3. The materialization sweep ("rolling" mechanism)

### 3.1 Where it runs

Per §0.4's precedent, this is a fourth branch in `crates/notifier`'s
existing `main.rs` `tokio::select!` loop (`main.rs:53-`), on its own
interval (e.g. `template_materialize_poll_interval_secs`, checked hourly
— materialization only needs to actually *act* once a day per template,
but polling hourly is cheap and matches this crate's existing
multi-interval pattern rather than needing wall-clock-aware scheduling
logic). Each tick: `SELECT` every `journey_templates` row where `active
= true`, today's weekday bit is set in `days_of_week`, today is within
`[starts_on, ends_on]`, and **no `journeys` row already exists for
`(source_template_id, today)`** (the idempotency guard — the same shape
as `candidates_for_trains_id`'s own "a second poll over an unchanged
table finds no new candidates" test discipline,
`crates/notifier/src/queries.rs:635`, applied here to "don't double-mint
today's occurrence if the sweep runs twice before midnight rolls over").

Placing this in `notifier` (which already depends on the DB layout
`journeys`/`journey_legs` live in) rather than `crates/api` avoids adding
scheduling logic to the request-serving process; it costs nothing that
isn't already true of `notifier`'s existing design (best-effort, retried
next cycle on failure, no user-facing latency requirement).

### 3.2 What one materialization does

For each due template: `INSERT INTO journeys (user_id, custom_name,
source_template_id, ...)` (one new row, `custom_name` inherited from the
template, or template name + date if the product wants the list to
visually distinguish occurrences — a frontend/copy decision, not a schema
one), then for each `journey_template_legs` row, `INSERT INTO journey_legs
(journey_id, leg_order, origin_crs, destination_crs, service_date,
depart_after, depart_before, arrive_after, arrive_before, match_mode)`
with `service_date = today` and `match_mode` seeded from the template's
`default_match_mode`. If `default_match_mode = 'manual'`, materialization
stops there — an `'unmatched'` leg sits waiting for the user to pick a
candidate the same way any window-search leg does today (this is the
"remind me every weekday to pick my train, but don't guess for me" hybrid
named in §3.3). If `'auto'`, materialization immediately runs the
existing candidate query (parent spec §2.1's extended
`search_schedule_calling_point_departures`) against the leg's
origin/destination/windows for today's `service_date` and commits per
`auto_commit_rule`:

- **`'earliest'`**: the first candidate the query returns for the window,
  committed the moment scheduling data exists for it — this is the
  parent spec's own named option (a) (§2.3 there), "closest analogue to
  today's point-in-time pin behavior."
- **`'nearest_to_now'`**: **Speculative on exact mechanics** — meaningful
  only if materialization runs close to the actual travel time rather than
  hours ahead (the parent spec's own option (b) framing, "re-decided daily
  for a recurring commute-style journey"); if materialization instead runs
  once overnight for the whole day ahead, "nearest to now" and "earliest"
  degenerate to the same answer, since "now" at 4am is before every
  candidate in an 08:00 window. Recommend **defaulting to `'earliest'`
  only** for the first shipped version of `'auto'` and treating
  `'nearest_to_now'` as a stretch option gated on deciding *when*
  materialization actually runs (§7).

If the candidate query returns **zero** candidates for today (a genuine
Sunday-engineering-works gap in an otherwise-Monday-Friday commute, a
timetable change that removed the usual service, etc.), the leg is left
`'unmatched'` rather than the whole materialization failing — this is the
one case that needs a *new* notification class (§4.2), since today's
notifier has no way to tell a user "your recurring thing didn't work
today" at all (§0.5's confirmed gap).

### 3.3 A named hybrid worth calling out explicitly to the product owner

`default_match_mode = 'manual'` with a non-NULL `days_of_week` is a real,
useful third mode this data model supports for free: **"remind me, don't
guess for me."** Every weekday morning, an unmatched leg with today's
window appears, the user gets to see it and (optionally, §4.2) gets
nudged if they haven't picked by some cutoff — but the system never
silently commits a candidate on the user's behalf. This is materially
lower-risk than full `'auto'` (no wrong-guess to correct, no new "which
train did the system silently pick for me" trust question) and might be
the right *default* for a first ship of rolling journeys, with true
`'auto'` as an opt-in escalation once the product owner is comfortable
with the silent-commit behavior. Recommend surfacing this explicitly as a
three-way choice in the UI (§6), not a boolean "make this recurring"
toggle that implies full automation.

---

## 4. Notifications

### 4.1 Escalation notifications need no new logic — the existing per-occurrence dedup already prevents "spam"

Because §3.2 mints a **new** `journey_legs` row (and thus a fresh,
un-populated `journey_leg_notification_state`/`train_notification_state`
row) for every day's occurrence, the existing escalation-only,
per-row-keyed dedup (§0.5) already does exactly the right thing with zero
changes: each new day's leg starts at rank 0/not-skipped, so a genuine
delay or skip on *that* day's specific working notifies once, the same
way a freshly-tracked train does today — never a `false→true→false→true`
repeat-notify loop for the *same* transition, because it isn't the same
row. **The literal thing the task brief worried about — "a fresh
'matched!' notification every single weekday" — cannot happen with
today's logic even if `'auto'` is wired up exactly as designed, because
no notification fires on match/commit at all (§0.5)**, and that should
stay true. Recommend explicitly **not** adding a "your leg was
auto-matched" push notification class — silence-on-successful-match is
the existing, correct behavior; the interesting new information is
*failure* to match, not success.

### 4.2 One genuinely new notification class is worth adding: "today's occurrence needs attention"

This is the concrete gap §0.5 and §3.2 both point at: an unmatched leg
(whether `'manual'`-mode-by-design, per §3.3, or `'auto'`-mode-that-found-
nothing) is **invisible** to today's notifier — `candidates_for_trains_id`
and `list_committed_legs_for_today` both require a bound `trains_id`
(§0.5). For a rolling journey specifically, this silence is a real product
problem in a way it isn't for a one-off journey: a user who manually
creates an unmatched window-search leg is, by definition, sitting there
about to pick a candidate; a user whose recurring commute silently failed
to auto-match at 4am has no reason to open the app before their normal
departure time and may simply miss their train. Recommend a new decision
path, structurally parallel to `decide_skip_notification`
(`decision.rs:102-108`) and using the same `journey_leg_notification_state`
table shape (a new boolean column or a sibling table, implementation
detail): fire once, escalation-only ("this leg is still unmatched as of
[some cutoff, e.g. 90 minutes before the earliest `depart_before` bound]"),
never re-fire for the same leg once sent, same discipline as every other
`*_notification_state` table in this app.

### 4.3 Audience for a rolling journey's notifications stays the parent spec's own open question, unresolved by this doc

The parent spec left "does a group a journey is shared into also get push
notifications" open (§5.3/§9.3 there), recommending owner-only for Phase
1. This document adds no new reason to revisit that — the new §4.2
notification class should follow whatever the eventual answer to the
parent question is, not fork its own separate audience rule.

---

## 5. Interaction with group sharing (Phase 4, already shipped)

Confirmed in §0.6: `group_journeys` grants are scoped to one concrete
`journeys.id`, and `journey_readable_by` resolves membership for exactly
that id. A recurring journey mints a **new** `journeys.id` every
materialized day (§3.2) — so a naive re-use of `group_journeys` as-is
would mean a group's access silently expires the moment tomorrow's
occurrence is minted, unless something re-shares it. **This is a real gap
this feature must resolve, not inherit unexamined**, and the task brief is
right to ask about it explicitly.

**Recommend: sharing targets the template, and read access resolves
transitively to whichever occurrence is "current."** Concretely:

```sql
CREATE TABLE group_journey_templates (
    group_id    TEXT   NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    template_id BIGINT NOT NULL REFERENCES journey_templates(id) ON DELETE CASCADE,
    added_by    TEXT   NOT NULL REFERENCES users(id),
    added_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, template_id)
);
```

— a direct structural copy of `group_journeys` (§0.6), same ownership/
unshare/view permission model as the parent spec's §6 (share requires
owning the template; unshare is sharer-or-manager; view is any member,
read-only). `journey_readable_by` (`journeys.rs:632-657`) gains a third
disjunct alongside its existing "owner" and "shared via `group_journeys`"
checks: "caller is a member of a group with a `group_journey_templates`
grant for `journeys.source_template_id`." This means:

- Sharing a *template* automatically extends to every occurrence it
  produces, past and future, with **no daily re-share step and no
  scheduler-side write to any sharing table** — materialization (§3.2)
  stays a pure `journeys`/`journey_legs` insert, unaware of sharing
  entirely, exactly the same separation of concerns the parent spec
  already draws between `journeys` and `group_journeys` (two independent
  tables, one references the other, neither writer needs to know about
  the other).
- A **one-off** journey (no `source_template_id`, or a template-less
  "reusable, but not recurring" duplication per §2.1) keeps using the
  existing, unmodified `group_journeys` — sharing "just today's specific
  instance" is still the right and only option for a non-recurring
  journey, and this is not a behavior change for anything shipped today.
- **What this deliberately does NOT do**: it does not let a group member
  reach back into *past* occurrences the template produced before the
  share was created, unless the past `journeys` row is independently
  shared via `group_journeys` too — template-sharing is forward-looking
  by construction (a grant dated `added_at` naturally only ever applies to
  occurrences that exist at read time, but nothing stops a future
  occurrence pre-dating `added_at` from also being covered, since the
  membership check is "does a grant currently exist," not "did it exist on
  the occurrence's own date" — worth a product decision, flagged in §7,
  though the recommended default of "yes, template sharing covers every
  occurrence at read time, past or future" matches `group_trains`'
  existing "any current member sees the full shared list" posture,
  §0.4 of the parent spec).

This answers the brief's explicit question — **sharing a recurring
journey shares the recurrence** (the template), not merely "today's
specific instance" — while leaving the existing one-off sharing path
(§0.6) completely untouched.

---

## 6. UX for creating and managing a reusable or recurring journey

**Entry points**, cheapest-first:

1. **"Track this journey again"** (§2.1) — a button on an existing
   journey's detail page (`frontend/app/journeys/[id]/page.tsx`, next to
   the existing `ShareJourneyButton`/`AddJourneyLegButton` header controls,
   §0.6's own file confirms this header already composes several owner-only
   action buttons in one row) that pre-fills `/journeys/new` from the
   current journey's own already-fetched leg data. No template entity
   touched. Ships independently (§8, Phase A).
2. **"Make this a template" / "Save as reusable"** — promotes an existing
   journey's shape into a durable `journey_templates` row (§2.1's
   "durable" case), independent of any recurrence — this is the button
   for "my occasional client-site trip," used every few weeks, not daily.
3. **A `/journeys/templates` list** (mirroring `/journeys/mine`'s existing
   list-plus-detail shape) — each row shows the template's name, its
   recurrence summary if any ("Weekdays" / "One-off, reuse on demand"),
   active/paused state, and a "Run now" button (the manual-trigger
   equivalent of §3.2, for a non-recurring template, or for topping up a
   recurring one on an extra day). Detail page: edit origin/destination/
   windows per leg (same `TimeFilterInput`/`TrainSearchForm` before/after
   UI the parent spec's §0.5 already established as this app's window-entry
   convention — reused verbatim, not reinvented), the three-way match-mode
   choice from §3.3 (manual reminder / auto-earliest / auto-nearest-to-now),
   day-of-week picker, start/end date, and the Pause toggle (§2.4).
4. **"Make this recurring"** toggle, reachable from a template's own
   detail page (or inline when creating one) — turns `days_of_week` from
   `NULL` to a real bitmask. Recommend **against** a single global
   "recurring journey" creation flow that's disconnected from the
   template concept — collapsing straight from "track a journey" to "set
   up a recurring commute" in one form skips past the cheap, low-risk
   reusable step (§2.1/§8 Phase A) that most users likely want first and
   that ships with far less product risk.

**A `/journeys/mine` list question, flagged for design not resolved
here**: once a rolling template is producing ~5 new `journeys` rows a
week, should `/journeys/mine` show every occurrence individually (today's
list behavior, unmodified) or collapse same-template occurrences under
one expandable row ("Weekday commute — today: on time, delayed 20m
Tuesday, …")? This is directly downstream of §2.5's retention question —
recommend not committing to a list redesign until that's settled, since a
collapsed view would need the underlying row-growth problem addressed
either way.

---

## 7. Open questions for the product owner

1. **Which "reusable" does the product owner actually want** — the cheap,
   template-less "duplicate what I'm looking at, once" button (§2.1,
   ships fast, no schema change), the durable, independently-manageable
   "saved shape" template entity (§2.2, without any recurrence), or is the
   real ask always about the *recurring* case and "reusable" was just
   imprecise language for it? This materially changes Phase A's scope
   (§8) — recommend confirming before starting any implementation, even
   though §2.1 is cheap enough to build speculatively.
2. **Auto-commit rule** (parent spec's own deferred Open Question #1,
   inherited here): ship `'earliest'` only (§3.2's recommendation), or
   is `'nearest_to_now'` important enough to also resolve *when*
   materialization runs (§3.2's dependency) before shipping `'auto'` at
   all?
3. **Occurrence materialization model** — confirm the recommended "one
   fresh `journeys` row per day, kept forever" (§2.5, §3.2) is actually
   wanted, versus a lighter-weight alternative this document did **not**
   recommend but should name for completeness: mutating one long-lived
   `journeys` row's single leg in place each day (no per-day history, no
   new row growth, but breaks per-occurrence ticket attachment and
   Delay-Repay tracking, and breaks the escalation-dedup reasoning in
   §4.1, since a mutated-in-place leg's notification state would need
   deliberate resetting at rollover instead of getting it for free from a
   fresh row). Recommend confirming this trade-off explicitly rather than
   assuming.
4. **Retention** (§2.5) — is unbounded per-occurrence row growth
   acceptable (matches this app's existing `train_subscriptions` posture),
   or does a rolling journey's ~260-rows/year rate finally justify a
   pruning/archival job this app has never needed before?
5. **Bank holidays / one-off skips** (§0.7, §2.4) — should a recurring
   journey's day-of-week rule also exclude UK bank holidays (data that
   doesn't exist in this app today and would need sourcing), and
   separately, does a user need a lightweight "skip tomorrow only"
   snooze distinct from pausing the whole template (§2.4's `active` flag
   is all-or-nothing; a single-day skip is a different, smaller
   mechanism not designed here)?
6. **Template sharing retroactivity** (§5's final bullet) — does sharing
   a template into a group expose every occurrence that template has
   *ever* produced (this document's recommended default, matching
   `group_trains`' existing "any current member sees everything shared"
   posture) or only occurrences from the share date forward?
7. **Naming** (front matter) — "journey template," or does the product
   owner prefer different product-facing language (e.g. "Recurring
   journey," "Commute") that this document's Rust/SQL symbol names should
   then follow, to avoid a naming mismatch between the UI and the code the
   way the parent spec's own "Journey" collision already came close to?
8. **Notification cutoff for §4.2** — "still unmatched as of X before the
   earliest window bound" needs a concrete X (parent spec's own
   `train_delay_threshold_minutes`-style configurable constant is the
   precedent, `crates/notifier/src/config.rs` — **Speculative**, not
   re-read here, but named as the existing pattern to extend) — is a
   fixed default (e.g. 60 minutes) acceptable for Phase 1, or does this
   need to be per-template configurable from day one?

---

## 8. Proposed phased approach

Modeled on the parent spec's own §9 phasing (small, individually-shippable
phases; each stands alone; nothing in a later phase blocks an earlier
one shipping).

**Phase A — "Track this journey again" (reusable, no new schema).**
Frontend-only: a button on the journey detail page that pre-fills
`/journeys/new` from the current journey's already-fetched data (§2.1).
No backend change at all. **Complexity: low.** Ships independently of
everything else in this document and resolves the "reusable" half of
Open Question #1 empirically — real usage of this button is itself useful
signal for whether the durable-template entity (Phase B) is worth
building at all.

**Phase B — Durable journey templates, on-demand only (no recurrence
yet).** Backend: `journey_templates`/`journey_template_legs` (§2.2, minus
`days_of_week`/`active`/`ends_on`/`default_match_mode`/`auto_commit_rule`
— or with those columns present but unused/always-NULL, since adding them
in Phase B avoids a second migration in Phase C), `journeys.source_template_id`
lineage column; `POST /JourneyTemplates` (create/promote-from-existing),
`GET /JourneyTemplates/mine`, `POST /JourneyTemplates/{id}/materialize`
(the manual "Run now" trigger, §6 item 3 — literally §3.2's logic with no
scheduler, called synchronously from a button click). Frontend:
`/journeys/templates` list + detail + edit (§6). **Complexity: low-medium**
— mechanically similar to the parent spec's own Phase 1 (a new table pair,
CRUD routes, a list+detail page), no new automatic-matching or
notification logic yet.

**Phase C — Recurrence + the materialization sweep + `'auto'` matching.**
Backend: `days_of_week`/`active`/`starts_on`/`ends_on`/
`default_match_mode`/`auto_commit_rule` columns actually used; the
`notifier` sweep branch (§3.1-3.2); resolves the parent spec's Open
Question #1 by finally implementing `'auto'` exactly as scoped in §2.3
above. Frontend: day-of-week picker, three-way match-mode choice (§3.3),
Pause toggle. **Complexity: medium-high** — this is the phase with
genuinely new automated-decision logic (which candidate wins, unattended)
and the phase most worth a product-owner check-in mid-build given how
central Open Questions #2-3 are to its shape.

**Phase D — The new "unmatched occurrence" notification class (§4.2).**
Backend: new decision function + state table (mirrors
`decide_skip_notification`'s shape exactly, §4.2), a new `notifier`
interval branch reading materialized-but-still-`'unmatched'` legs whose
window has nearly elapsed. **Complexity: low-medium**, structurally a
near-copy of Phase 3 of the parent spec (station-skip detection) —
same "new decision function, new state table, new interval branch,
reused delivery layer" shape. Sequenced after Phase C since it has
nothing to detect until `'auto'`-mode legs can actually fail to match.

**Phase E — Template group sharing.** Backend: `group_journey_templates`
(§5), the `journey_readable_by` third disjunct. Frontend: share button on
a template's own detail page, mirroring `ShareJourneyButton.tsx`.
**Complexity: low** — the smallest phase, a near-verbatim structural copy
of already-shipped Phase 4 group-sharing code (§0.6), plus one new
`OR` clause in one existing authorization function.

**Not phased separately, explicitly out of scope for all of the above:**
bank-holiday exclusion (§7 Q5), a single-day snooze distinct from pausing
(§7 Q5), any retention/archival job (§7 Q4, needs its own product decision
before any schema commitment), and a collapsed/grouped `/journeys/mine`
list view (§6's flagged list-design question) — each is independently
addable once its own open question is resolved, none blocks Phases A-E.
