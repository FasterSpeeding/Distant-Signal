# Design: Should a Shared Internal-Services Authentik Group Exist?

**Status: design proposal / investigation, not approved.** This document
answers a question, not a plan — per this repo's own established
convention (see every design under `docs/superpowers/specs/`, none of
which contain a task list). No code is written or modified to produce it.

## Goal

`docs/superpowers/specs/2026-09-02-internal-service-oauth2-design.md`
(hereafter "the base design") gives every one of the 8 real internal
callers of `crates/api`'s `/private/*` routes its own dedicated Authentik
group (`svc-poller-incidents`, `svc-trust-consumer`, etc.), and
`build_internal_oauth_routes()` (`crates/api/src/app.rs:55–159`) lists,
per `(path, method)`, exactly which group(s) may call it. This session
also found and fixed a real gap in that table: `/stanox-crs` has two
legitimate callers on two different methods (`trust-consumer` `GET`,
`schedule-reference` `POST`), and a prior, unsplit version of the table
let either caller's token authorize both methods.

This document investigates whether that fix is a preview of a broader
need — whether it is worth adding a **second, shared** Authentik group
(e.g. `svc-internal`) that most/all 8 callers also belong to, gating
access to genuinely shared/common endpoints, while keeping the existing
per-service groups narrowly scoped to each service's own write/ingest
responsibilities. It also investigates a second, distinct variant raised
directly by the repo owner: a **read/write split**, where a shared group
grants `GET`-only access across *all* `/private/*` endpoints and each
service's own group grants write access to just the endpoints it needs
to write to. Both are evaluated against the real, current route table —
not hypothetically.

## Current relevant state (verified this session)

### The full internal-oauth route table today — 8 callers, 10 paths, 17 entries

`build_internal_oauth_routes()` (`crates/api/src/app.rs:60–158`), backed
by `crates/api/src/routes/ingest.rs:28–63` and
`crates/api/src/routes/samples.rs:16–17`:

| Path | Method | Required group | Caller |
|---|---|---|---|
| `/incidents` | GET | `svc-poller-incidents` | poller-incidents |
| `/incidents` | POST | `svc-poller-incidents` | poller-incidents |
| `/stations` | GET | `svc-poller-stations` | poller-stations |
| `/stations` | POST | `svc-poller-stations` | poller-stations |
| `/tocs` | GET | `svc-poller-tocs` | poller-tocs |
| `/tocs` | POST | `svc-poller-tocs` | poller-tocs |
| `/station-samples` | GET | `svc-poller-ldbws` | poller-ldbws |
| `/station-samples` | POST | `svc-poller-ldbws` | poller-ldbws |
| `/sample-stations` | GET | `svc-poller-ldbws` | poller-ldbws |
| `/tfl-line-status` | GET | `svc-poller-tfl` | poller-tfl |
| `/tfl-line-status` | POST | `svc-poller-tfl` | poller-tfl |
| `/train-events` | POST | `svc-trust-consumer` | trust-consumer |
| `/tracked-trains` | GET | `svc-trust-consumer` | trust-consumer |
| `/schedule-feed-ingests` | GET | `svc-schedule-ingest` | schedule-ingest |
| `/schedule-feed-ingests` | POST | `svc-schedule-ingest` | schedule-ingest |
| `/stanox-crs` | GET | `svc-trust-consumer` | trust-consumer |
| `/stanox-crs` | POST | `svc-schedule-reference` | schedule-reference |

`find crates -maxdepth 1 -type d` (this session) confirms 8 real callers
exist today (`schedule-reference` now exists as a crate, unlike at the
time the base design was written): the 5 RDM/TfL pollers, `trust-consumer`,
`schedule-ingest`, `schedule-reference`. `ServiceArguments`
(`crates/api/src/data/config.rs:77–92`) declares exactly one
`internal_oauth_group_*` field per caller, defaulting to the `svc-*` names
above; `charts/distant-signal/values.yaml:432–440`
(`api.internalOauth.groups`) mirrors the same 8-entry map.

**`/stanox-crs` is the only multi-caller path in the table today.** Every
other path has exactly one legitimate caller, on every method it exposes.
Critically, `/stanox-crs`'s two callers are split into two *separate*
table entries (one per method), each carrying exactly **one** required
group — not one entry with two groups. `crates/api/src/auth.rs:801–825`'s
own test, `every_production_route_entry_is_reachable_by_its_own_caller_on_its_own_method`,
asserts this directly: it iterates every entry in the real production
table and requires `groups.len() == 1` for each ("`the /stanox-crs`
split was the only multi-group entry that ever existed" — past tense,
referring to the pre-fix, unsplit version). **As of right now, no table
entry has more than one required group at all.** Adopting either variant
investigated below would be the first time that changes.

### The public route surface has no internal-oauth gate at all — and no candidate "everyone needs this" route exists there either

`crates/api/src/routes/mod.rs:21–53`'s `public_router()` merges `health`,
`freshness`, `history_retention`, `incidents` (public GET), `lines`,
`notifications`, `preferences`, `reference`, and `auth` — none of these
pass through `require_internal_oauth` (`mod.rs:60–64`'s `private_router()`
is the only thing that does, wrapping only `ingest::router()` and
`samples::router()`). Health checks, freshness data, and reference-data
reads are already public, unauthenticated endpoints today, not
internal-oauth-gated ones — so "a shared health-check/freshness/reference
read endpoint" is not a hypothetical this design needs to invent a group
for; it already exists, and already needs no internal-service credential
at all. There is no route anywhere in this codebase — public or private —
that all 8 internal callers currently need and don't already have.

### Enforcement already treats a route entry's required-group list as OR, and already supports N groups per entry with zero new logic

`require_internal_oauth` (`crates/api/src/auth.rs:53–111`) does a
two-phase lookup — path match, then method match — and then:

```rust
if !required_groups
    .iter()
    .any(|group| claims.groups.contains(group))
{
    ...  // 403
}
```

(`auth.rs:102–108`). This is already "the verified token's `groups` claim
must contain **any** of this entry's required groups" — the exact
semantic either shared-group variant needs. **Point 5's question is
answered directly: `Vec<(&'static str, Method, Vec<String>)>` and the
`.any()` check already structurally support "caller's own group OR a
shared group" per route, with zero change to `auth.rs`.** What would
change is `app.rs`'s `build_internal_oauth_routes()` (which routes' `Vec`
gets a second group added) and `data/config.rs` (a new
`internal_oauth_group_shared`-shaped field) — a config/data-shape change
to an existing, well-tested mechanism, not new authorization logic. The
one real code-adjacent cost: `auth.rs:801–825`'s own
`groups.len() == 1` assertion (see above) is a deliberate regression
guard against the exact pre-fix bug this session found, and would need
updating to allow >1 for whichever entries gain a second group — a
one-line change to a test's expectation, not a redesign of the test.

### GET vs. POST across the real table, and what each GET actually returns

Of the 17 entries above, **9 are GET, 8 are POST**. What the 9 GETs
return, checked directly against `crates/api/src/routes/ingest.rs` and
`samples.rs`:

- **6 are last-fetched timestamps only** — `GET /incidents`,
  `/stations`, `/tocs`, `/station-samples`, `/tfl-line-status`,
  `/schedule-feed-ingests` each return a single `LastFetchedResponse`
  (`ingest.rs:70–113`, `:222–229`) — one `DateTime`, nothing else. Not
  remotely sensitive; arguably closer in spirit to `public_router()`'s
  `freshness` endpoint than to a protected resource.
- **`GET /sample-stations`** (`samples.rs:19–28`) returns the list of
  station CRS codes to sample, derived from the (public) line catalogue
  plus custom lines. Low sensitivity — it's config-shaped, not user data.
- **`GET /tracked-trains`** (`ingest.rs:192–199`,
  `data/train_tracking.rs:208–220`) returns `TrackedTrainRef` rows
  (`crates/common/src/lib.rs:652–660`: `id`, `service_date`,
  `pin_origin_crs`, `pin_scheduled_departure`, `resolution_status`,
  `train_uid`, `train_id`) for every currently-active tracked train
  system-wide. **No `user_id` or any per-user identifying field is
  selected** — the query (`train_tracking.rs:209–216`) never joins to a
  user table. Aggregate operational data, not directly user-identifying,
  though it does reveal which trains are being tracked in aggregate.
- **`GET /stanox-crs`** (`ingest.rs:258–265`) returns the full
  STANOX/CRS/TIPLOC reference table (`common::StanoxCrsRecord`,
  `lib.rs:682–691`) — reference data, the same category of thing
  `public_router()`'s own `reference` endpoint already serves publicly
  for other reference datasets.

**None of the 9 GET routes return secrets, credentials, or PII.** The
worst-case content behind any of them is aggregate operational metadata.

### Authentik-side mechanics: multi-group membership is native, not an obstacle

Per the base design's own investigation (Current relevant state there): a
service account is a `User` object in Authentik's data model, and the
`groups` scope-mapping expression (`request.user.ak_groups.all()`)
evaluates against however many groups that user belongs to — Authentik
users, service accounts included, are natively members of any number of
groups. Adding a second group membership to all 8 existing service
accounts is a normal, supported operation with no special-casing —
confirmed as a modeling question only, not a technical obstacle, matching
what point 4 of the task asked to verify.

## Decisions

### 1. Does a real shared-endpoint need exist today? No.

`/stanox-crs` is the only multi-caller path in the table, and it is
already correctly handled — two callers, two methods, two single-group
entries, no shared group involved or needed. No other route in the
private-route surface has more than one legitimate caller. No route
outside the private surface needs internal-oauth at all (public routes
already serve health/freshness/reference reads unauthenticated). **There
is no "every one of the 8 callers plausibly needs this specific route"
case anywhere in this codebase today.** A shared group gating "genuinely
shared common endpoints" (the first variant this document was asked to
investigate) would therefore be built with **no current route to attach
it to** — pure speculative infrastructure, not a fix for an observed gap.

### 2. The narrow "shared-common-endpoints" group variant: defer, don't build now

**What it would concretely simplify, if a shared endpoint existed**: today,
onboarding one new genuinely-shared route means enumerating all 8 groups
in that one route's `Vec<String>` by hand, and a future 9th service needs
a 9th manual addition to every such route. With a shared group, the route
is written once against one group name, and every current *and future*
service just needs that one group membership — no per-route enumeration,
no N-way fan-out per new shared route.

That is a real, legible simplification — **if and when a second
genuinely-shared route ever appears.** It does not exist today (Decision
1). Building the group now means:

- Provisioning an Authentik group with no route gated on it yet — dead
  configuration, not exercised by anything, until some future route
  opts in.
- Widening blast radius pre-emptively: every one of the 8 service
  accounts gains a second group membership, which — the moment even one
  route is ever added to it, possibly by someone who doesn't reread this
  document — grants every one of the 8 callers access to that route,
  whether or not that's actually the right scope for it. A group that
  already exists and already has 8 members is a much easier "just add
  this route to the group" decision to make casually later than
  provisioning a new group is; that ease cuts both ways.
- No corresponding benefit today, because nothing needs it today.

**Recommendation: do not build this now.** Revisit only when a second
real multi-caller (or "every caller needs this") route actually
materializes — at that point, the immediate, narrow fix (per the
`/stanox-crs` precedent: add the specific callers' specific groups to
that one route's `Vec<String>`, using the OR-based check that already
exists) is almost always sufficient by itself for one more route. A
shared group only earns its keep once *several* such routes exist or are
clearly coming, which is not the situation today.

### 3. The read/write split variant (raised by the repo owner): feasible and low-risk, but still not justified by a real need today

**The concrete proposal**: a shared group (e.g. `svc-internal-read`)
grants `GET` access across all `/private/*` routes; each service's own
existing group continues to gate its own `POST`/write routes exactly as
today.

**Feasibility, checked against the real table**: of the 17 entries, 9 are
GET and 8 are POST (Current relevant state, above). Converting to this
model is mechanically identical to the narrow variant — no `auth.rs`
change, since the `.any()` check already treats a route's required-group
list as OR (Current relevant state, above). Concretely it would mean: add
one new `ServiceArguments` field (`internal_oauth_group_internal_read`,
or similar), give it to all 8 service accounts in Authentik, and either
replace or extend the group list on each of the 9 GET entries in
`build_internal_oauth_routes()` to include it. `auth.rs:801–825`'s
`groups.len() == 1` test assertion needs updating for those 9 entries
either way (see Current relevant state).

**Does any caller's read need already span beyond its own domain?**
Checked directly against the table: **no**, with exactly one existing
exception — `trust-consumer` already reads `/stanox-crs`
(`schedule-reference`'s write domain), which is the base design's own
already-accepted precedent for one service legitimately reading another's
data. No other cross-domain read is exercised by any of the 5 pollers,
`trust-consumer`, `schedule-ingest`, or `schedule-reference` today. So
this variant, if adopted, would grant 8 services' worth of *new*, unused
read permissions — poller-incidents would gain the ability to `GET
/schedule-feed-ingests`'s timestamp, `GET /tracked-trains`,
`GET /stanox-crs`'s full table, etc. — none of which it has any present
need for.

**Blast-radius tradeoff, weighed concretely against what's actually
behind these 9 GET routes**: a compromised single service credential
today can only reach that one service's own narrow read+write surface
(2 routes typically, e.g. poller-incidents: `GET`/`POST /incidents`
only). Under this variant, a compromised credential's *write* surface is
unchanged (still gated by its own specific group), but its *read* surface
becomes all 9 GET routes system-wide. Per the content audit above, that
means: 6 timestamps, one station list, the active-tracked-trains table
(no PII), and the full STANOX/CRS reference table (already public-grade
reference data). **This is a real, broader default-allow than the narrow
variant — every compromised credential can now read operationally
everything, not just a scoped shared subset — but the actual sensitivity
of what's newly exposed is low across the board**, because nothing behind
these particular 9 GET routes is a secret, a credential, or PII. The
repo owner's framing ("read-everything is a broader default-allow,
distinct risk profile") is correct as a structural point even though the
current data behind it happens to be low-stakes — the risk is in what
future GET routes get added to `/private/*` and automatically inherit
this blanket grant, not in what's there today.

**Recommendation: feasible, low current risk, but still not worth
adopting now**, for a reason distinct from Decision 2's "no route to
attach it to": here the routes already exist, and the model *is*
immediately applicable — but doing so would grant seven-eighths of the
newly-opened read paths to services that have never needed them and have
articulated no need for them, purely because the blanket model is
convenient to reason about, not because any concrete workflow requires
it. That inverts this codebase's own stated posture from the base
design's Decision 3 (*"one group per real caller ... preserves the real
least-privilege property ... a compromised or misconfigured `poller-tfl`
pod cannot reach `/schedule-feed-ingests`"*) for the read direction with
no offsetting workflow benefit today. If a second genuine cross-service
read need ever materializes (a second `/stanox-crs`-shaped case), the
`/stanox-crs` precedent — add that one specific route to that one
specific other caller's allowed groups — remains the narrower, equally
easy fix, and does not require pre-committing every service to read
access on every other service's routes in order to solve one new
instance. A blanket read-all group is the more attractive choice only if
*several* independent cross-domain read needs emerge around the same
time (much like Decision 2's threshold for the narrow shared-group
variant) — not yet observed.

### 4. Authentik-side mechanics: confirmed, not an obstacle for either variant

Per Current relevant state: Authentik service accounts are `User` objects
and natively support membership in any number of groups; the `groups`
scope mapping surfaces however many the account belongs to. Adding a
second group to all 8 existing service accounts is a normal
group-membership operation — no new Authentik object type, no per-account
special configuration, purely operator time (out of scope for this
document to perform, as it was for the base design's own Decision-3-area
provisioning).

### 5. Code-side mechanics: no new logic for either variant, confirmed against the actual enforcement code and its test suite

`require_internal_oauth`'s `.any()` check (`auth.rs:102–108`) already
implements "caller's own group OR a second group" per route with zero
code change to the enforcement path itself. What *does* need touching for
either variant: `build_internal_oauth_routes()`'s route entries (which
routes' `Vec<String>` gains the shared/read group), one new
`ServiceArguments` config field (plus its chart/`values.yaml` mirror,
following the existing `internal_oauth_group_*` pattern at
`config.rs:77–92`/`values.yaml:432–440`), and
`auth.rs:801–825`'s `groups.len() == 1` regression-guard test, which
would need to allow >1 for whichever entries change. None of this is new
authorization logic — it is the same shape the `/stanox-crs` fix already
proved out this session, just applied to more entries.

### 6. Overall recommendation: defer both variants; keep the status quo

Neither variant is justified by a real, current need:

- The narrow shared-group variant has **no route to attach it to** —
  `/stanox-crs` is the only multi-caller route, and it's already
  correctly handled by the existing per-route multi-group mechanism with
  no shared group involved.
- The read/write split is mechanically ready and low-risk given today's
  GET payloads, but would grant seven-eighths of its newly-opened access
  to callers that have never needed it, purely for structural
  convenience, not a workflow requirement — a real (if currently
  low-stakes) widening of blast radius with no matching benefit today.

**Do this instead**: keep the current one-group-per-caller model, and
keep using the already-proven, already-tested per-route multi-group
mechanism (`Vec<String>` + `.any()`) exactly as `/stanox-crs` uses it
today, on a route-by-route basis, whenever a *second* real multi-caller
or cross-domain-read need actually shows up. That mechanism has zero
additional cost to invoke per instance (it's already load-bearing,
already tested) and keeps blast radius scoped to exactly the callers a
given route actually needs — never wider by default. Revisit a shared
group (of either shape) only if/when **several** such needs accumulate at
once, at which point the administrative savings (add-once-to-a-group vs.
enumerate-per-route) start to outweigh the wider blast radius; one
instance, or zero, does not clear that bar.

## Architecture

Not designed to implementation depth, per this document's own
investigation-only scope (Explicitly out of scope, below). If a future
decision revisits Decision 6 because a real trigger has appeared, the
shape to reach for is already fully specified by Decisions 2/3/4/5 above:
one new `ServiceArguments` group field, that group added to the specific
route(s) that motivated the change (narrow variant) or to every GET entry
(read/write-split variant), and a one-line update to
`auth.rs`'s `groups.len() == 1` assertion — no new middleware, no new
claim, no change to `require_internal_oauth` itself.

## Explicitly out of scope

- **Implementing either variant.** This is an investigation document; no
  code, config, or Authentik-side object was created or modified.
- **Choosing an exact shared-group name**, since neither variant is
  recommended for adoption now. If one is adopted later, it should follow
  the existing `svc-*` naming convention already established by the 8
  per-caller groups.
- **Re-litigating the base design's per-service-group model**, which this
  document affirms rather than revisits — Decision 6 explicitly keeps it.
- **The `/stanox-crs` fix itself**, already implemented and tested this
  session (`auth.rs`'s split-by-method entries and their regression
  tests) — cited here as precedent, not redesigned.
- **Any change to the public route surface** (`health`, `freshness`,
  `history_retention`, `reference`, etc.) — confirmed unauthenticated and
  out of internal-oauth's remit entirely; nothing here proposes gating
  them.
- **A 9th/10th future internal service's own onboarding steps** beyond
  noting, per Decision 2, how the shared-group model would reduce that
  service's onboarding cost if it existed.

## Open questions/risks

1. **What the actual trigger for revisiting Decision 6 looks like.** This
   document sets no numeric threshold beyond "several" cross-domain
   needs accumulating; a future revisit should look at the real count of
   multi-caller/cross-domain-read routes at that time rather than a
   number picked in the abstract here.
2. **Whether future `/private/*` routes might carry more sensitive GET
   payloads than today's 9** (e.g. a future route returning
   user-identifying data). The read/write-split variant's "low current
   risk" finding (Decision 3) is a fact about *today's* routes, not a
   permanent property — a future route added to that blanket-read group
   without re-auditing its sensitivity would inherit read access from
   every one of the 8 (or more) internal callers, whether or not that's
   appropriate for that specific route's data. Anyone reviewing a new
   `/private/*` GET route in the future should check what group(s) gate
   it explicitly, not assume the shared-read group (if ever built) is
   automatically appropriate.
3. **Whether Decision 1's "no shared-endpoint need exists" finding still
   holds once `schedule-reference` (now a real crate, unlike at the time
   the base design was written) grows a fuller route surface** — this
   document audited its one current route (`/stanox-crs` POST); if
   `schedule-reference` gains more routes with their own multi-caller
   shape, that's exactly the kind of accumulation Decision 6 says should
   trigger a revisit.
