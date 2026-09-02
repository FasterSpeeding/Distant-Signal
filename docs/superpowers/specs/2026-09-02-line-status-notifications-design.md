# Design: Line-Status Notifications

**Status: design proposal, not approved.** Written to the same rigor as
`docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md` (whose
Tier 1/2/3 policy and `LoginPromptModal`/`useNeedsLogin` primitives this
spec's own gating decisions build on directly, not reinvent) and
`docs/superpowers/specs/2026-09-01-pwa-manifest-design.md` (the most
recent PWA-adjacent spec, and the reason this feature is even reachable on
iOS — see Decision 1). No implementation plan is included; that is a
separate, later step in this repo's process.

## Goal

Notify a signed-in user when a line they've pinned has a new incident, a
severity-tier status change (e.g. Good Service → Minor Delays → Severe
Delays), or when a train they're tracking is cancelled or newly,
meaningfully delayed — primarily via background push to the installed PWA
on mobile, but designed to work well on desktop too, not as a mobile-only
afterthought.

## Prerequisite check: the sibling service-worker spec

**As of this session, `docs/superpowers/specs/2026-09-02-pwa-service-worker-design.md`
does not exist on `main`** — confirmed by listing
`docs/superpowers/specs/` (re-checked at the start of this session, after
an earlier interruption, specifically for this) and finding no file
matching `*service-worker*` or `*line-status-notif*` anywhere in that
directory or in `git log --oneline --all` for either name. The most
recent PWA-adjacent spec present is
`docs/superpowers/specs/2026-09-01-pwa-manifest-design.md`, which
explicitly ships manifest + icons + `viewport.themeColor` **with no
service worker** (its own Explicitly out of scope: *"Any service worker,
offline caching, or push notifications — carried forward unchanged from
the research doc's own explicit out-of-scope list"*) and whose
`docs/superpowers/specs/2026-09-01-pwa-support-research.md` predecessor
names push notifications as needing *"a real backend, a subscription
model, and a server-side trigger for what would even push a
notification"* — i.e. exactly this document.

**This is a hard, unresolved dependency, not a soft one.** Web Push
delivery to a closed or backgrounded tab is only possible via
`ServiceWorkerRegistration.showNotification()` from inside a `push` event
handler running in an active service worker — there is no other browser
API that delivers a notification without one. This spec is written
**assuming a service worker will exist** (per the concurrently-commissioned
sibling effort) and does not block on it landing first, but every piece of
this design that touches the service worker is written as a **contract**
(a payload shape, a small number of required event-handler behaviours) the
eventual SW must satisfy — not as a redesign of the SW's own scope,
caching strategy, or registration mechanism, which stays entirely that
spec's job. See Decision 1 and the Architecture section's "SW push-handler
contract" box for exactly what's specified vs. deliberately left open, and
Open questions/risks #1 for the concrete risk if the two designs' payload
assumptions ever diverge.

## Current relevant state (verified 2026-09-02, this session)

### 1. The real trigger source: `line_status_history`, written from two independent call sites

`crates/aggregator/src/queries.rs`'s `write_line_status` (lines 260–311)
is the aggregator's poll-cycle write: it upserts `line_status` on every
cycle (line ~279) but only inserts a `line_status_history` row **if the
statuses actually changed** since the last cycle (line 274–276, 300–308):

```rust
let changed = match &existing {
    None => true,
    Some(existing) => normalize_for_diff(existing) != normalize_for_diff(&statuses_json),
};
...
if changed {
    sqlx::query(
        "INSERT INTO line_status_history (line_id, statuses, computed_at) VALUES ($1, $2, NOW())",
    )
    ...
}
```

`normalize_for_diff` (lines 163–190) strips three specifically-identified
sources of every-cycle churn before comparing — `validity.from_date` on
non-incident-derived statuses, `sample_stats`/`sample_availability` (both
recomputed from live LDBWS samples every poll), and the `" (live samples
show: ...)"` reason-text suffix `escalate_from_sample_stats` appends on
escalation — so `line_status_history` already represents "the aggregator's
own considered judgement that something real changed," not "a poll cycle
ran." This is the real signal to hook into; it is **not** the same as "the
severity tier changed" (see Decision 2 — the strip list does not remove
the rest of `reason`, so a same-tier reason-text edit still produces a
history row).

**A second, independent writer exists for TfL lines.**
`crates/api/src/data/queries.rs`'s `upsert_tfl_line_status` (lines
333–382) is called from `crates/api/src/routes/ingest.rs`'s
`post_tfl_line_status` handler (line 152, module doc lines 6–9: *"the odd
one out: its batch is already-computed... targets `line_status`/
`line_status_history` directly"*). It runs its own, structurally identical
but textually separate `tfl_statuses_changed` diff-and-insert against
`line_status_history` (lines 371–379). **Any design that hooks into
`write_line_status` alone would silently miss every TfL line
(`tfl-victoria`, `tfl-central`, etc.) — a real, pinnable line category**
(`crates/api/src/routes/preferences.rs`'s `get_preferences` handler
explicitly folds in `queries::tfl_line_summaries` alongside static and
custom lines when resolving what a user's `pinned_lines` rows actually
refer to, with a comment noting a TfL pin would otherwise "silently
[drop]... on every read"). This asymmetry between the two writers is the
central architectural fact driving Decision 3 below.

`line_status_history.id` is `BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY
KEY` (`crates/api/migrations/20260510023522_initial.sql:89`), globally
monotonic across every writer and every line — a clean watermark cursor
for a poll-based consumer, confirmed by direct read of that migration.

### 2. `common::Severity`/`severity_rank`: a coarse 5-tier scale, distinct from the raw enum's `Ord`

`crates/common/src/lib.rs:108–131`'s `severity_rank` maps every
`Severity` variant to one of five buckets — `0` (good/no-issues), `1`
(informational/special/closed-overnight), `2` (planned), `3`
(mild — reduced service, minor delays, recovering, change of frequency),
`4` (severe — closed, suspended, severe delays, rail replacement, part
closed, diverted, not running) — deliberately mirroring
`frontend/lib/severity.ts`'s `GROUP_RANK` so both ends of the stack agree.
Its own doc comment (lines 92–101) explains why this exists at all:
**`Severity`'s derived `Ord` sorts by TfL-mirrored discriminant value, not
true severity** — `Diverted = 21` and `PartClosed = 11` are numerically
high (`Ord`-"mild") but genuinely severe, while `GoodService = 10` sits
mid-range. `common::LineStatusReport::worst_severity()` (lines 362–370)
uses raw `.min()` over the enum, **not** `severity_rank` — a real,
pre-existing inconsistency between the two "how bad is this line" helpers
already in the codebase, not something this spec introduces. **This
spec's own severity-transition logic uses `severity_rank` exclusively,
never `worst_severity()`**, precisely because the discriminant-vs-rank gap
`severity_rank`'s own doc comment describes would otherwise make, e.g., a
`Diverted` line register as an *improvement* over `MinorDelays`. See
Decision 2.

### 3. Pinning: `pinned_lines`, per-user, no per-line notification flag today

`crates/api/src/data/preferences.rs` (read in full): `list_pinned_line_ids`
(lines 7–13) and `replace_pinned_lines` (lines 41–53) are the entire
surface. Schema (`crates/api/migrations/20260710090000_preferences.sql`
plus the ownership retrofit in `20260828100000_add_ownership.sql`):

```sql
CREATE TABLE pinned_lines (
    line_id    TEXT        PRIMARY KEY,  -- retrofitted below
    pinned_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
-- 20260828100000_add_ownership.sql:
ALTER TABLE pinned_lines
    ADD COLUMN user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    DROP CONSTRAINT pinned_lines_pkey,
    ADD PRIMARY KEY (user_id, line_id);
```

`(user_id, line_id)` composite primary key, `replace_pinned_lines`
delete-all-then-insert-all in one transaction per PUT. No third column
exists anywhere — pinning is a flat, boolean "this line is on my home
page" membership, nothing per-line beyond `pinned_at`. This is the actual
shape of "the existing pinned-lines concept" the task asks to reuse or not
— see Decision 3.

### 4. Tracked trains: a real, separate per-train state pipeline — but no incident-to-train matching exists anywhere

`crates/api/src/data/train_tracking.rs` (read in full) and
`crates/api/migrations/20260828120000_train_tracking.sql` (read in full)
define three tables: `tracked_trains` (one row per pin, `user_id NOT NULL
REFERENCES users(id)`), `train_movement_events` (append-only,
`id BIGSERIAL PRIMARY KEY`, one row per TRUST message, deduped on
`(tracked_train_id, dedup_key)`), `train_current_state` (one denormalized
row per tracked train, upserted on every event). `upsert_train_event`
(`train_tracking.rs:219–283`) is called from exactly one place —
`crates/api/src/routes/ingest.rs:169`'s `post_train_movement_event`
handler, which `crates/trust-consumer` POSTs to — a single write path,
unlike `line_status_history`'s two.

`crates/trust-consumer/src/journey.rs` (read in full) computes the actual
transitions: `apply_movement` (lines 28–46) sets `status = "en_route"` and
a `delay_minutes` derived from TRUST's `variation_status` (`"ON TIME"`/
`"EARLY"` clamp to `0`, `"LATE"` is filled in by the caller with a real
minute count); `apply_cancellation` (line 48–50) is a real, discrete
transition to `status = "cancelled"`. These are genuine, already-computed
per-train signals.

**A repo-wide grep this session (`affects.*tracked`, `tracked.*affect`,
`incident.*train`, `train.*incident` across every `.rs` file, excluding
tests/strings) found zero matches.** There is no code anywhere that
associates a `LineStatus`/incident with a specific tracked train's route
— nothing plays the role `crates/aggregator/src/matcher.rs` plays for
lines. "An incident affecting a tracked train," read literally, is not a
signal this codebase computes today; building it would mean a second,
train-shaped `matcher.rs` (matching a tracked train's origin/destination/
operator against currently-active incidents) — a genuinely new piece of
matching logic, not a reuse of an existing one. See Decision 4 for how
this spec scopes tracked-train notifications around what *does* already
exist (`status`/`delay_minutes` transitions) instead.

### 5. Anonymous-UX and login-prompt conventions this spec must follow, not reinvent

`docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md`'s three-
tier policy (§Policy) and `docs/superpowers/specs/2026-09-02-modal-login-prompt-design.md`'s
`LoginPromptModal`/`useNeedsLogin()` primitives (`frontend/components/
useNeedsLogin.ts`, confirmed present) are the established, current
mechanism for every Tier-2 "public entry, gated completion" control in
this app — `PinToggle`, `CustomLineForm`, `TrackTrainForm`,
`TicketEntryForm` all use it, per the modal-login-prompt spec's own
twelve-site inventory. Pinning specifically is Tier 2: `PinToggle.tsx`
renders for every visitor and shows `LoginPromptModal` (body: *"Log in to
pin this {kind}."*) only on a real `401` from the `PUT`. A
"notifications" control layered on top of pinning should follow the exact
same shape, not invent a fourth login-prompt pattern — see Decision 6.

### 6. Backend/deployment stack this design must fit

`crates/api/Cargo.toml` pins `reqwest = "0.12"` **on purpose** — its own
comment (lines 20–27) explains `oauth2`/`openidconnect`'s
`AsyncHttpClient` trait is only implemented for `reqwest` 0.12's
`Client`, so anything in the `api` crate pulling a different major would
break `auth::oidc`'s `request_async` call with an unsatisfied-trait-bound
error. `crates/common/Cargo.toml` is on `reqwest = "0.13.4"` instead — the
two crates already disagree, deliberately. This matters directly for
Decision 5's crate choice, below.

Web Push delivery needs VAPID keys and per-service-worker-registration
push subscriptions stored server-side — verified via web search this
session (see Decision 5 for the specific crate and its own HTTP-client
independence from `reqwest`, which sidesteps the version split above
entirely rather than adding a third opinion to it).

Every Rust service in this workspace already builds against
`rust:1.88-bookworm` (`docker/api.Dockerfile:41`, comment lines 4–19
explaining the shared rustc floor) and the `api` runtime image already
requires OpenSSL indirectly via `sqlx`'s `tls-native-tls` feature
(`docker/api.Dockerfile:77–79`'s own comment: *"sqlx's tls-native-tls
feature verifies the Postgres connection's cert... against the system
store"*) — an OpenSSL build/link requirement is not new to this
workspace.

Deployment already has a generic app-secret template
(`charts/distant-signal/templates/secret.yaml`, confirmed present
alongside per-service `*-deployment.yaml` files, e.g.
`aggregator-deployment.yaml`) that every service's `Deployment` already
reads env vars from — the same mechanism used for `DATABASE_URL` and the
OIDC client secret today.

## Decisions

### 1. Platform matrix: design around real, uneven browser/OS support — mobile gets the headline win, desktop gets a real but different one

Verified this session (web search, cross-checked against
`2026-09-01-pwa-support-research.md:187–195`'s own iOS finding):

| Platform | Install required? | What the visitor actually gets |
|---|---|---|
| **iOS Safari (and iOS Chrome/Edge, all WebKit-backed)** | **Yes — Home Screen add is mandatory.** Safari: iOS **16.4+** (confirmed exact version, both by this session's web search and the pre-existing research doc's own citation, `2026-09-01-pwa-support-research.md:187–188`). iOS Chrome/Edge: 16.6+ (same WebKit engine, per this session's search). A normal Safari tab — installed or not, this app already ships `display: standalone` via the merged manifest — **cannot receive push at all**, on any iOS browser. | Full background delivery, but **only after** the visitor has added the PWA to their home screen — the exact install flow `2026-09-01-pwa-manifest-design.md` already ships icons/manifest for. |
| **Android Chrome/Firefox** | **No.** A plain browser tab can call `Notification.requestPermission()` + `PushManager.subscribe()` and receive background push with no install step at all. | Full background delivery either way; installing to the home screen additionally gives an app-shaped icon/notification-shade grouping, but isn't required for push itself to work. |
| **Desktop Chrome/Firefox/Edge** | **No.** Same as Android — native support in a normal tab, no install. Safari on **macOS Ventura+ (Safari 16+)** also supports it natively, confirmed this session — Apple's desktop and mobile Safari diverged on this exact requirement (desktop never required installation; iOS always has). | Background delivery while the browser process is running (exact "browser fully quit" behaviour is OS/browser-background-service-dependent and out of this spec's control) — foreground-tab-open delivery always works regardless. A visitor who never installs the PWA at all still gets real, working push on desktop. |

**This is not a uniform "mobile-good, desktop-degraded" split — the real
fault line is iOS specifically, not mobile in general.** An Android
visitor gets the full background experience from a bare browser tab,
matching desktop; only iOS (Safari, Chrome, or Edge, all WebKit) requires
the install step this app's manifest work already supports. Framing this
as "primarily targeted at mobile users using the installed PWA" (the
task's own framing) is accurate for the **iOS** audience specifically —
this app's manifest spec (`2026-09-01-pwa-manifest-design.md`'s own
Corrections/relationship section) already treats iOS as this app's
primary PWA-install audience — but the feature must not assume every
mobile visitor needs to install first, and must not assume desktop
visitors get a lesser experience than mobile: on Android and every
desktop browser, a bare open tab is enough to subscribe and receive
background push, so the "enable notifications" control (Decision 6) is
never conditioned on install state, only on browser support
(`'serviceWorker' in navigator && 'PushManager' in window`, the standard
capability check) and, transitively, on whatever the sibling SW spec
actually registers.

**A visitor who never grants permission, or is on an unsupported/
non-installed-and-required (iOS) combination, gets nothing new — the
existing foreground `AutoRefresh` 30-second poll (already documented as
this app's only freshness mechanism, `2026-09-01-pwa-manifest-design.md`'s
Architecture section: "No change anywhere to `AutoRefresh.tsx`...") is
and remains their entire freshness channel.** This spec adds a background
channel on top of that for opted-in visitors; it does not attempt to
replace or replicate the foreground experience for anyone else — see
Explicitly out of scope.

### 2. Change filter: severity-tier transitions only, via `severity_rank` — not "every `line_status_history` row"

**Chosen: a `line_status_history` row only becomes a notification
candidate when the line's computed worst `severity_rank` (0–4, per
Current relevant state §2) differs from the previous history row's worst
`severity_rank` for that same `line_id`.** "Worst" is computed the same
way `LineStatusReport::worst_severity()` intends — the single worst
concurrent status on a line — but via `severity_rank`, not
`worst_severity()`'s raw `Ord::min()`, for the exact reason `severity_rank`'s
own doc comment gives (Current relevant state §2): the raw enum would
misrank `Diverted`/`PartClosed` as mild.

**Considered and rejected: notify on every `line_status_history` insert.**
`normalize_for_diff` (Current relevant state §1) already strips the
noisiest per-cycle churn, but it does **not** strip a genuine reason-text
edit at the same severity (e.g. an incident's estimated-resolution time
shifting, or wording tweaks as more detail becomes available) — those
still produce real history rows today, confirmed by reading
`normalize_entry_for_diff` (`queries.rs:175–190`), which only touches
`validity.from_date`, `sample_stats`, `sample_availability`, and one
specific reason-text suffix. Notifying on every such row would mean a
user gets pushed every time an already-known "Severe Delays" incident's
description is re-worded, with no change to what actually matters to a
rider (whether their line is running). `severity_rank`'s 5-bucket
coarseness is a feature here, not a loss of information — it already
absorbs same-tier text/detail churn as a side effect of the bucket being
coarser than raw `reason` text, without this spec needing its own
separate "is this reason meaningfully different" heuristic.

**Considered and rejected: notify on the raw 15-value `Severity` changing**
(not bucketed through `severity_rank`) — e.g. `MinorDelays` →
`ReducedService`, both rank 3. Rejected for the same reason `severity_rank`
exists at all: two mild-tier values swapping is not a meaningfully
different rider-facing outcome, and — separate from the "is this useful"
argument — comparing raw discriminants directly would reintroduce exactly
the `Ord`-vs-true-severity mismatch `severity_rank`'s own doc comment
warns about.

### 3. Trigger mechanism: a new, dedicated poll-based service reading `line_status_history` by watermark — not a hook inside either existing writer

**Chosen: a new binary (illustratively `crates/notifier`, mirroring this
workspace's existing per-concern service split — `aggregator`,
`enricher`, `trust-consumer` are all separate poll-loop binaries already,
per `Cargo.toml`'s `[workspace] members`) that polls `line_status_history`
for rows with `id` greater than a persisted watermark, on its own
interval, independently of both `write_line_status` and
`upsert_tfl_line_status`.**

**Considered and rejected: hook directly into `write_line_status`**
(return the `changed` flag it already computes, call a notify function
inline from `run_cycle`). Rejected because Current relevant state §1's
central finding — `upsert_tfl_line_status` is a **second, independent**
writer, in a different crate (`api`, not `aggregator`), with its own
separate diff — means this alone would silently exclude every TfL line. A
correct version of this approach would need the identical hook added
**twice**, in two different crates, with the two implementations kept in
sync forever — the same "two call sites can drift" shape the codebase has
already hit once for this pair (`tfl_statuses_changed` and
`normalize_for_diff` are already two independently-maintained diff
functions for structurally the same table).

**Considered and rejected: a Postgres `LISTEN`/`NOTIFY` trigger on
`line_status_history` inserts**, pushed to a long-lived listener process.
Would solve the two-writers problem (a trigger fires regardless of which
statement inserted the row) with lower latency than polling, but this
workspace has no existing `LISTEN`/`NOTIFY` usage anywhere (grepped) — it
would be a wholly new operational pattern (a persistent DB connection
held open outside sqlx's normal pool-and-query usage, reconnect/backoff
handling for a dropped listen connection) introduced for a feature whose
own trigger source (a line's status changing) is not latency-sensitive at
human-perceptible timescales — a push notification arriving 30–60 seconds
after the aggregator's own poll cycle computed the change is
indistinguishable in practice from one arriving instantly, since the
aggregator's own `poll_interval_secs` is already the dominant source of
lag long before any notifier adds its own. **Rejected as unjustified
complexity for a latency win nobody would notice**, not because the
mechanism is wrong in the abstract.

**A watermark-based poll over both signal tables (`line_status_history`
for lines, `train_movement_events` for tracked trains — Decision 4) is
the chosen mechanism, sharing one new small cursor table**:

```sql
CREATE TABLE notifier_cursor (
    name             TEXT   PRIMARY KEY,  -- 'line_status_history' | 'train_movement_events'
    last_processed_id BIGINT NOT NULL DEFAULT 0
);
```

The cursor is an **efficiency** mechanism, not a correctness-critical one
— rescanning already-processed rows would be caught anyway by
Decision 5's per-`(user_id, line_id)`/`(user_id, tracked_train_id)`
notification-state idempotency check (a row this cycle whose computed
severity/status already matches the last-*notified* state is a no-op
regardless of whether the cursor already skipped it). This distinction
matters for how much durability the cursor itself needs: unlike
`crates/aggregator/src/dedup.rs`'s `SeenServiceLedger` (explicitly
in-memory and restart-scoped, per `main.rs:49–54`'s own comment, because
its worst failure mode is a bounded, self-correcting over-count), losing
this cursor on a crash would at worst cause one extra full-table rescan on
restart, not a duplicate notification — so a plain persisted row (updated
every cycle after processing) is sufficient; no additional durability
design is needed beyond "write it after each successful cycle."

**One necessary correctness guard, found by tracing `write_line_status`'s
own logic (Current relevant state §1): a line's very first-ever
`line_status_history` row must never itself trigger a notification.**
`write_line_status`'s `changed` is unconditionally `true` when `existing`
is `None` (line 275) — meaning a brand-new deployment's very first
aggregation cycle writes a history row for **every** line simultaneously,
with nothing to diff against. The notifier's own severity-transition
comparison (Decision 2) needs a **previous** history row to compare
against by construction — a line_id seen for the first time in this
poller's own scan has no prior row to compare its severity against and
must be skipped, not treated as "changed from nothing." This is not a
new gap this spec introduces; it is a real, already-latent property of
`write_line_status`'s own `None => true` branch that a naive
notifier — one that (incorrectly) treated "a history row exists" as "a
notification is due" — would turn into a first-run notification storm to
every pinner of every line. Explicitly guarded against in Decision 2/5's
combined logic: "no prior row for this `line_id`" is a distinct outcome
from "no severity change," handled as a silent skip in both cases.

### 4. Scope: both pinned-line and tracked-train notifications — but tracked-train notifications cover the train's own resolved status, not "an incident affecting it"

**Both are in scope.** Current relevant state §4 already establishes why
this is not "reuse the same matching logic twice": pinned-line
notifications key off `line_status_history` (an existing, computed,
per-line severity signal); tracked-train notifications key off
`train_movement_events`/`train_current_state` (an existing, computed,
per-train status/delay signal) — two genuinely different, already-existing
data sources, not one signal applied to two audiences.

**What's covered for a tracked train**: a transition to `status =
'cancelled'` (`journey.rs`'s `apply_cancellation`, a real discrete event —
`crates/trust-consumer/src/process.rs:710`'s own test confirms this status
value is actually written), and `delay_minutes` crossing a fixed threshold
(illustratively 15 minutes, matching the kind of delay that actually
changes a rider's plans — not independently researched or user-tested,
same "reasonable round number, revisit with real usage" posture
`train_tracking.rs`'s own `MAX_PIN_AGE`/`MINE_LIST_LIMIT` constants
already take). Detected the same way as line-status transitions
(Decision 3): poll `train_movement_events` by watermark, join to
`tracked_trains.user_id`, compare the new row's derived `status`/
`delay_minutes` against a persisted per-`(user_id, tracked_train_id)`
last-notified state (mirroring Decision 5 exactly, for trains instead of
lines).

**What's explicitly NOT covered — "an incident affecting a tracked
train," read as "a Knowledgebase incident whose scope includes this
train's route":** this would need new matching logic — a train-shaped
counterpart to `crates/aggregator/src/matcher.rs`'s existing
segment/operator/route matching, but keyed on a specific train's origin/
destination/calling points rather than a whole line. Nothing like this
exists today (Current relevant state §4's repo-wide grep), and building
it is a real, separate design problem (CIF-derived calling-point lists
aren't available to `trust-consumer` at all per that crate's own Global
Constraints, referenced in `journey.rs:32–38`'s comment about why
`next_calling_point` is never populated ahead of time) — not a small
extension of this spec's trigger-and-deliver mechanism. **This spec
argues the train's own resolved `status`/`delay_minutes` is in fact the
more precise, more actionable signal for someone tracking one specific
service anyway** — "your train is cancelled" from TRUST's own
authoritative feed is a stronger, more specific fact than "an incident
whose text vaguely matches your route was filed," so scoping to the
existing signal isn't a lesser fallback, it's the better-grounded of the
two for this specific audience.

### 5. Subscription model: reuse `pinned_lines` as scope, a new `push_subscriptions` table as the per-device delivery gate — no new "notification preferences" concept

**Chosen: a line/train notifies a user if and only if (a) it's in that
user's `pinned_lines`/`tracked_trains` and (b) that user has at least one
live row in a new `push_subscriptions` table.** No new per-line
notification-preference table.

```sql
CREATE TABLE push_subscriptions (
    id           BIGINT      GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    user_id      TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    endpoint     TEXT        NOT NULL UNIQUE,  -- the Push API's own dedup key
    p256dh       TEXT        NOT NULL,
    auth         TEXT        NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX push_subscriptions_user_id ON push_subscriptions (user_id);

CREATE TABLE line_notification_state (
    user_id                    TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    line_id                    TEXT        NOT NULL,
    last_notified_severity_rank SMALLINT   NOT NULL,
    last_notified_at            TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, line_id)
);

CREATE TABLE train_notification_state (
    user_id             TEXT        NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tracked_train_id    BIGINT      NOT NULL REFERENCES tracked_trains(id) ON DELETE CASCADE,
    last_notified_status TEXT       NOT NULL,
    last_notified_delay_minutes INTEGER,
    last_notified_at     TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (user_id, tracked_train_id)
);
```

Three separate tables, not one polymorphic `notification_state(target_type,
target_id)` table — deliberately matching this schema's own existing
convention of separate tables per concept rather than a shared polymorphic
one (`pinned_lines`/`pinned_stations`, `custom_lines`/`tracked_trains`,
never a merged "pinned things" table anywhere in this codebase).

**Why reuse `pinned_lines` rather than a separate opt-in table**: the
task itself asks this to be weighed, and the concrete alternative — a
`notification_preferences(user_id, line_id, enabled)` table, independently
toggled from pinning — was considered and rejected. Two reasons: (1)
`push_subscriptions`'s own existence (Decision 6's "Enable notifications"
control, itself a real browser-permission grant) is already a second,
meaningful gate beyond pinning — a third toggle on top of "pinned" and
"push enabled on this device" would be redundant friction for what this
app's `DESIGN.md` already characterizes as a "single trusted personal
instance"-sized deployment, not a multi-tenant product where fine-grained
per-line muting is a proven need. (2) It would create exactly the
preference-drift problem this app has already had to fix once elsewhere —
`crates/api/src/routes/preferences.rs:114–115`'s comment on TfL pins
silently dropping on read is a live example of two representations of
"what's pinned" (the stored ids vs. the catalogue that validates them)
falling out of sync; a separate notification-scope list next to
`pinned_lines` would be a second, independently-maintained copy of "which
lines this user cares about," open to the identical class of drift (pin a
line, forget to also toggle notifications for it; unpin a line, forget to
untoggle). **Unpinning a line is, by construction, also "unsubscribing
from notifications for it"** — the join is recomputed fresh every
notifier cycle directly against `pinned_lines`, so there is nothing to
keep in sync.

Fan-out is per-user, not per-subscription-row for the rate-limit decision:
`line_notification_state`/`train_notification_state` are keyed on
`(user_id, ...)`, not `(push_subscription_id, ...)`, so a user with two
devices (phone + desktop) shares one cooldown state and both devices are
pushed together when a notification does fire — not staggered or
independently rate-limited per device.

### 6. Frontend surface: one new Tier-2 control, not a new page

**Chosen: a single, global "Enable notifications" toggle** (illustratively
placed on the home page alongside the pinned-lines section, since it's a
direct extension of that existing UI, not a standalone concept needing
its own route — no `/settings` page exists anywhere in `frontend/app`
today, confirmed by listing the directory, so this avoids inventing one
for a single control). It performs, client-side: check
`'serviceWorker' in navigator && 'PushManager' in window` (standard
capability probe — deliberately not an install-state check, per Decision
1); if supported, `navigator.serviceWorker.ready` (resolves once
*whatever* SW the sibling spec registers is active — this call makes no
assumption about that SW's file location or scope) then
`registration.pushManager.subscribe({ userVisibleOnly: true,
applicationServerKey: <fetched from a new, unauthenticated GET
/public/notifications/vapid-public-key> })`, then `POST
/public/notifications/subscribe` with the resulting `PushSubscription`'s
`endpoint`/`keys.p256dh`/`keys.auth`, authenticated the same way every
other Tier-2 write in this app is (`AuthenticatedUser`, 401 on no
session).

**Follows the exact established Tier-2 shape, not a new one**: renders
unconditionally for every visitor (anonymous included — "advertise the
feature," the same reasoning `2026-09-02-modal-login-prompt-design.md`'s
Decision 6 already gives for reclassifying "My Trains & Tickets" the same
way); on a `401` from the subscribe POST, shows `LoginPromptModal` (body:
*"Log in to enable notifications."*), using `useNeedsLogin()` — the
shared hook, not a hand-rolled fifth variant, per that spec's own
Decision 5 "adopt the shared hook while touching this anyway" posture.

**Not a per-line control.** Because Decision 5 reuses `pinned_lines` as
scope directly, there is no per-line "notify me about this one"
affordance to add next to each pin star — enabling notifications once
covers every currently- and future-pinned line and tracked train
automatically. This is a direct consequence of Decision 5, not an
independent UI simplification.

## Architecture

```
                              ┌─────────────────────────────┐
                              │   aggregator (existing)      │
                              │   write_line_status()        │
                              │   -> line_status_history      │
                              └──────────────┬───────────────┘
                                             │
                              ┌──────────────┴───────────────┐
                              │   api (existing)              │
                              │   upsert_tfl_line_status()    │
                              │   -> line_status_history       │  (TfL lines,
                              └──────────────┬───────────────┘   separate writer)
                                             │
     ┌───────────────────────────────────────┴──────────────────────────────┐
     │                                                                       │
     ▼                                                                       ▼
┌─────────────────────────┐                                    ┌───────────────────────────┐
│ trust-consumer (existing)│                                    │  line_status_history table │
│  -> /private/... ingest  │                                    │  (id BIGINT IDENTITY, the  │
│  -> upsert_train_event() │                                    │   shared watermark cursor) │
│  -> train_movement_events│                                    └──────────────┬─────────────┘
└──────────────┬───────────┘                                                   │
               │                                                               │
               ▼                                                               ▼
     ┌─────────────────────────────────────────────────────────────────────────────┐
     │  notifier (NEW binary, its own poll cycle, like aggregator/enricher)         │
     │                                                                              │
     │  1. Read notifier_cursor for both tables' last_processed_id                  │
     │  2. For each new line_status_history row (Decision 3):                      │
     │       compute worst severity_rank; compare to the PRECEDING history row     │
     │       for this line_id (skip if no preceding row — Decision 3's guard)      │
     │       skip if severity_rank unchanged (Decision 2)                          │
     │  3. For each new train_movement_events row (Decision 4):                    │
     │       derive status/delay_minutes; skip if unchanged from prior event       │
     │  4. For each surviving candidate:                                           │
     │       JOIN pinned_lines / tracked_trains -> candidate user_id(s)            │
     │       JOIN line_notification_state / train_notification_state:             │
     │         - severity got WORSE (rank increased)      -> notify immediately   │
     │         - severity got BETTER or lateral            -> notify only if       │
     │           last_notified_at older than cooldown (default 20 min)             │
     │       skip if unchanged from this user's own last-NOTIFIED state            │
     │         (independent of watermark position — Decision 3's idempotency note) │
     │  5. For each notify-worthy (user_id, target): JOIN push_subscriptions       │
     │       -> zero or more (endpoint, p256dh, auth) rows                        │
     │  6. Build payload { title, body, url, tag } (see SW contract, below)        │
     │  7. Send via web-push crate + VAPID (Decision 5's Current-relevant-state    │
     │       crate finding), one HTTP POST per subscription endpoint               │
     │  8. On 404/410 from the push service: DELETE the push_subscriptions row     │
     │       (Error handling, below)                                              │
     │  9. On success: upsert line_notification_state/train_notification_state    │
     │  10. Advance notifier_cursor                                                │
     └──────────────────────────────────┬─────────────────────────────────────────┘
                                         │  HTTPS POST (VAPID-signed, RFC8188-encrypted)
                                         ▼
                          ┌───────────────────────────────┐
                          │  Browser's push service         │
                          │  (FCM / Mozilla autopush / etc.) │
                          └────────────────┬────────────────┘
                                           │  (background, even if the app is closed —
                                           │   Decision 1's platform matrix governs whether
                                           │   this reaches the device at all)
                                           ▼
                     ┌─────────────────────────────────────────────┐
                     │  SW PUSH-HANDLER CONTRACT (this spec's       │
                     │  requirement on the sibling SW spec, not     │
                     │  its own new infrastructure):                │
                     │                                              │
                     │  self.addEventListener('push', event => {   │
                     │    const { title, body, url, tag } =        │
                     │      event.data.json();                     │
                     │    // Skip if a focused client already shows │
                     │    // this page (AutoRefresh already covers  │
                     │    // it within 30s) -- clients.matchAll()   │
                     │    // check, standard pattern, not new infra │
                     │    registration.showNotification(title,      │
                     │      { body, tag, data: { url } });          │
                     │  });                                        │
                     │  self.addEventListener('notificationclick',  │
                     │    event => clients.openWindow(              │
                     │      event.notification.data.url));         │
                     └─────────────────────────────────────────────┘
```

## Error handling

- **Expired/invalid push subscriptions.** The Web Push protocol's own
  contract: a push service returns `404` or `410 Gone` on send when a
  subscription is no longer valid (uninstalled PWA, revoked permission,
  cleared browser data). The `notifier`'s send step (Architecture, step
  8) treats either status as "delete this `push_subscriptions` row" —
  self-healing, no separate expiry/TTL sweep needed, mirroring
  `crates/api/src/data/users.rs:insert_login_state`'s own "every write
  takes out its own trash" posture for a different table.
- **Transient delivery failures** (5xx from the push service, network
  timeout). No retry queue — a bounded number of immediate retries (e.g.
  2, with a short backoff) inside the single send attempt, then log and
  move on, same posture every polling service in this workspace already
  takes for a failed cycle (`crates/aggregator/src/main.rs:75–77`: *"if
  let Err(err) = result { tracing::error!(...); will retry next
  interval }"*) — no dead-letter queue exists anywhere in this codebase
  for anything, and this app's "single trusted personal instance" scale
  (`DESIGN.md`) doesn't justify introducing the first one here. A
  transient failure on a real severity change is not silently lost
  forever either: `line_notification_state`/`train_notification_state`
  are only updated on send success, so a subsequent poll cycle that finds
  the target's *current* state still differs from `last_notified_*`
  will retry the notification on its own, without needing an explicit
  retry queue.
- **VAPID key misconfiguration / missing keys at startup.** The
  `notifier` binary should fail fast at startup (refuse to start,
  matching `crates/api/src/main.rs`'s existing posture of failing loudly
  on a missing required config value) rather than silently no-op every
  cycle — a silently-broken notifier would be indistinguishable from "no
  one has any notify-worthy changes," a much worse failure mode than a
  crash-looping pod an operator notices immediately.
- **The cold-start / first-ever-history-row storm** (Decision 3's guard)
  is handled by construction — "no preceding row" is a distinct,
  explicitly-skipped case, not merely "the severity check happening to
  find no difference."
- **A user with `push_subscriptions` rows but zero `pinned_lines`/
  `tracked_trains`** never matches the JOIN in step 4 — no special-casing
  needed, this falls out of the query shape itself.

## Testing

Following this repo's established convention (pure logic tested directly,
without a database; DB round-trips as `#[ignore]`d integration tests per
`crates/api/src/data/users.rs`'s own precedent; frontend components tested
with the existing `renderWithMantine`/Vitest setup):

- **Severity-transition and rate-limit decision logic** (Decisions 2/3/5)
  as pure, synchronous functions — `(previous_severity_rank,
  new_severity_rank, last_notified_severity_rank, last_notified_at, now)
  -> NotifyDecision { Skip, NotifyNow, NotifyIfCooldownElapsed }` — the
  same "pull the actual decision into a pure function, test it without
  I/O" shape `crates/aggregator/src/main.rs`'s own
  `lines_with_sample_coverage` and `crates/api/src/data/train_tracking.rs`'s
  `validate_pin` already use. Cases to cover explicitly: no prior row
  (skip, Decision 3's guard); same-rank change (skip, Decision 2);
  escalation during an active cooldown (notify immediately, Decision 5's
  bypass); de-escalation during an active cooldown (skip until cooldown
  elapses); a genuinely new incident with no prior line history at all.
- **`notifier_cursor` advancement and idempotency**: an `#[ignore]`d
  DB-backed test asserting a second poll cycle over an unchanged
  `line_status_history`/`train_movement_events` produces zero sends
  (mirrors `crates/aggregator/src/queries.rs`'s own existing test
  asserting "a stable status across two cycles still writes exactly one
  history row" — this spec's equivalent is "exactly one *notification*
  row, not one per poll").
- **`push_subscriptions` CRUD and the 404/410 self-cleanup path**:
  `#[ignore]`d DB tests, same shape as `users.rs`'s
  `session_round_trip_creates_looks_up_and_deletes`.
- **`web-push` crate's own send path**: not unit-testable meaningfully
  without a real or mocked push endpoint — this spec does not propose
  simulating the push service's HTTP contract; a manual, real-device
  smoke test (subscribe from an actual browser, trigger a real severity
  change or a manual test-send route, confirm the OS notification
  appears) is the only thing that actually verifies the encryption/VAPID
  path end-to-end, the same honest "some of this is manual, and this
  spec says so" posture `2026-09-01-pwa-manifest-design.md`'s own Testing
  section takes for install-flow verification.
- **Frontend "Enable notifications" control**: `useNeedsLogin`/
  `LoginPromptModal` rendering on a mocked `401`, following
  `2026-09-02-modal-login-prompt-design.md`'s own Testing section
  pattern for `PinToggle`/`TrackTrainForm` exactly; a `PushManager`/
  `serviceWorker` capability-check branch (unsupported browser renders
  nothing or a disabled state, not a broken button) — mocked, since
  jsdom has no real Push API.

## Explicitly out of scope

- **The service worker itself** — its registration, scope, caching
  strategy, and every concern `2026-09-01-pwa-support-research.md`
  already scoped to that future spec. This document only specifies the
  `push`/`notificationclick` handler *contract* (Architecture, above) as
  a requirement the eventual SW must satisfy.
- **"An incident affecting a tracked train"** in the literal
  route-matching sense (Decision 4) — no matching logic like
  `crates/aggregator/src/matcher.rs` exists for trains, and building one
  is a separate design problem, not an extension of this spec's
  trigger-and-deliver mechanism.
- **Per-line notification muting/preferences finer than "pinned or not"**
  (Decision 5) — no `notification_preferences` table; unpin is the only
  "stop notifying me about this" mechanism this spec provides.
- **Email or any non-push delivery channel.** Grepped this session for
  any existing email-sending capability (`lettre`, `smtp`, `sendgrid`,
  `mailgun`) across every `Cargo.toml` and `.rs` file in the workspace —
  none exists. `users.email` is stored today only for display (per
  `crates/api/src/data/users.rs`'s own doc comment on `verified_email`),
  never for sending anything. Adding a first email-sending dependency
  purely as a notification fallback is a materially bigger, separate
  piece of new infrastructure than this spec's scope.
- **In-tab toast notifications built from client-side diffing of
  `AutoRefresh`'s polled data**, as a second, push-independent delivery
  path for an already-open, already-foregrounded tab. The existing
  30-second poll already surfaces a changed status to a visibly open tab
  within one refresh cycle; the SW push-handler contract's own
  focused-client check (Architecture) is the one piece of "don't
  double-notify an open tab" logic this spec asks for, not a second
  notification UI.
- **Retry queues / dead-letter handling for failed sends** beyond the
  bounded in-attempt retry described in Error handling — no such
  mechanism exists anywhere in this codebase today for any feature.
- **Multi-tenant rate-limit tuning, A/B-testing cooldown windows, or any
  configurability of the 20-minute cooldown / 15-minute delay threshold
  beyond a single static config value** — same "reasonable round number,
  not researched or load-tested, revisit with real usage" posture this
  codebase already applies to comparable constants
  (`train_tracking.rs`'s `MAX_PIN_AGE`, `MINE_LIST_LIMIT`).
- **A dedicated `/settings` page.** Decision 6 places the one new control
  on the existing home page rather than inventing a new route for it.

## Open questions/risks

1. **Hard dependency on the sibling service-worker spec, unresolved as of
   this writing.** This entire feature is inert without a registered
   service worker capable of handling `push` events — confirmed not to
   exist yet by checking `docs/superpowers/specs/` for
   `2026-09-02-pwa-service-worker-design.md` (or any similarly-named
   file) both at the start of this session and again after an earlier
   interruption to this same task. The real risk if the two designs
   proceed independently: the SW spec might choose a payload contract, a
   notification-grouping (`tag`) convention, or a `notificationclick`
   navigation behaviour that doesn't match what this spec's Architecture
   section assumes — the payload shape (`{ title, body, url, tag }`) and
   the two handler behaviours specified here should be treated as this
   spec's request of that implementation, to be reconciled explicitly
   once both are being implemented, not assumed to land correctly by
   coincidence.
2. **`web-push` crate maturity and OpenSSL build requirement**, not
   independently verified beyond this session's web search. The crate
   (`pimeys/rust-web-push`, or an actively-maintained fork) implements
   VAPID + RFC8188 via Mozilla's `ece` crate and does **not** depend on
   `reqwest` at all (default HTTP client is `isahc`, with a
   `hyper-client` feature alternative) — this was checked specifically
   because `api`'s existing `reqwest = "0.12"` pin (Current relevant
   state §6) would have been a real integration risk if `web-push` also
   depended on `reqwest`, and it doesn't. Its own docs describe it as
   "still in active development... breaking changes in accordance with
   semver" — worth a fresh check of its current release/maintenance
   status at implementation time, not treated as settled by this design
   pass.
3. **iOS's real-world install rate is unknown.** This app's whole
   PWA-install push (`2026-09-01-pwa-manifest-design.md`) exists to make
   the install step possible at all, but nothing in this codebase
   measures how many real visitors actually complete it (no analytics
   infrastructure exists, per that spec's own Decision 1 finding) — so
   this design cannot say how much of its "primary, mobile" audience
   (Decision 1) will actually ever be reachable via push versus staying
   on the foreground-only fallback indefinitely.
4. **The 20-minute cooldown / escalation-bypass policy (Decision 5) is a
   reasoned first cut, not user-tested.** An escalation that then
   immediately de-escalates (e.g. briefly misreported Severe Delays,
   corrected within a minute) would still fire one immediate
   "got worse" notification under this policy, since the bypass check
   only looks at the direction of the specific transition being
   evaluated, not a longer window of stability first. A debounce that
   waits N minutes before notifying *any* transition (trading immediacy
   for fewer false-alarm escalations) was considered and set aside as
   more complex for a benefit that's speculative without real incident-
   correction-rate data from this app's own feeds.
5. **`push_subscriptions.endpoint UNIQUE` with no explicit re-ownership
   handling specified here.** If the same browser subscribes while
   logged in as user A, then later while logged in as user B (shared
   device, or a user logging into a different account), the endpoint is
   the same string but should now belong to B, not A. This spec's schema
   allows an `ON CONFLICT (endpoint) DO UPDATE SET user_id = EXCLUDED.
   user_id, ...` upsert to handle this correctly, but the exact
   route-handler logic for `POST /public/notifications/subscribe` isn't
   spelled out further here — a real, small decision left for
   implementation, not expected to be contentious.
