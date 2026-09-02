# Internal Service Accounts for `/private/*` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the single shared `X-Internal-Token` value every internal caller of `api`'s `/private/*` routes presents today with **per-service, identity-bearing credentials plus route-level scoping** — without changing the wire mechanism (still one bearer token in one header), without a database table, and without a flag-day cutover that could crash-loop a poller mid-rollout. Concretely: a static `InternalService` enum with a code-defined `allowed_prefixes()` table; a startup-built, in-memory token-hash → identity registry mirroring `AuthenticatedUser`'s existing session-lookup shape; `require_internal_token` rewritten to resolve an identity and check it against the requested route (`401` unknown token, `403` known-but-wrong-scope, unchanged `200`-eligible pass-through on a match); a bounded dual-acceptance window where the legacy shared token keeps working as an unscoped `InternalService::Legacy` identity, logged at `warn` on every use; per-service secret keys in the Helm chart and `docker-compose.yml`, following the `pollers.<name>.apiKey` / `scheduleFeed.sftp.password` "this app mints the credential" precedent; and lightweight per-request identity logging. **No poller/service crate's own Rust code changes anywhere in this plan** — `common::ingest::post_batch`/`fetch_last_fetched` and every `poller-*`/`trust-consumer`/`schedule-ingest` `config.rs` already take an opaque `internal_token: &str`; only *which value* each container is configured with changes, and that is a chart/compose-only edit (Tasks 5–6).

**Architecture:**

```
crates/api/src/auth.rs                     + InternalService enum, allowed_prefixes()
                                            + InternalServiceRegistry (token-hash -> identity)
                                            + classify_internal_request (pure, testable)
        │                                  (Task 1)
        ▼
crates/api/src/data/config.rs              + 7 new internal_token_<service> fields
crates/api/src/app.rs                      AppState::init builds the registry at startup
        │                                  (Task 2, depends on Task 1)
        ▼
crates/api/src/auth.rs::require_internal_token   rewritten: resolve -> classify -> 401/403/200
                                            constant_time_eq removed (dead code once this lands)
        │                                  (Task 3, depends on Tasks 1-2)
        ├──────────────────────┬───────────────────────────┐
        ▼                      ▼                            ▼
Task 4: Legacy identity   Task 7: per-request         (Tasks 5-6, parallel with 3/4/7,
  + warn-on-use log         identity logging            depend only on Task 2's field names)
        │                      │                                    │
        └──────────────────────┴───────────────┬────────────────────┘
                                                 ▼
                        charts/distant-signal/{secret.yaml,_helpers.tpl,values.yaml,
                          poller-deployments.yaml,trust-consumer-deployment.yaml,
                          schedulefeed-deployment.yaml}    (Task 5)
                        docker-compose.yml, dev.env.example, local.env.example  (Task 6)
                                                 │
                                                 ▼
                              Task 8: rollout runbook (operational, not code)
                                                 │
                                                 ▼
                              Task 9: end-to-end verification
```

**Tech Stack:** Rust (`crates/api` only — `sha2`/`base64` already in-tree via `hash_session_token`'s existing use, no new dependency); Helm chart (`charts/distant-signal`, existing `randAlphaNum`/lookup-preserve pattern, no new chart dependency); `docker-compose.yml` / `.env.example` files. No new crate, no new Cargo dependency, no new npm package, no database migration, no frontend change.

**Spec:** `docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md` (669 lines, on `main` at commit `c727a72` — not yet present on this plan's own branch history; fetched via `git show main:docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md` for this plan's writing) — read in full before starting; this plan turns its Decisions into concrete tasks and does not re-litigate them. Cross-references below to "Decision N" refer to that document.

**Status note — every citation below independently re-verified against this worktree's actual current source, not trusted from the spec:**

- `crates/api/src/auth.rs`: module doc at lines 1–6 (matches spec exactly), `require_internal_token` at lines 20–36, `constant_time_eq` at lines 46–56, `hash_session_token` at lines 164–168, `AuthenticatedUser`/`get_session_with_user` flow at lines 176–198 (all confirmed as cited).
- `crates/api/src/routes/mod.rs`: `private_router()` at lines 58–75 (spec cited 58–75, confirmed exact), `.layer(middleware::from_fn_with_state(app, require_internal_token))` at line 62 (spec cited "62"). Its own doc comment (lines 53–57) matches the spec's quote exactly.
- `crates/api/src/routes/ingest.rs`: `router()` at lines 28–53. **Route line numbers have drifted from the spec's own citations** — this plan cites the corrected ones: `post_incidents` 101–109, `post_stations` 111–119, `post_station_samples` 121–129, `post_tocs` 131–139, `post_tfl_line_status` 148–156, `post_train_events` 160–170, `get_active_tracked_trains` 176–183, `get_schedule_feed_last_fetched`/`post_schedule_feed_ingest` 206–213/215–224 — these match the spec's table almost exactly (the spec's own citations for this file were accurate; independently re-confirmed here).
- `crates/api/src/routes/samples.rs`: `router()` at line 17, `get_sample_stations` at lines 20–25 — confirmed.
- `crates/api/src/app.rs`: `AppState::init()`'s `internal_token` non-empty guard at lines 65–72 (spec cited "65–72", confirmed exact), hand-rolled `Debug` impl at lines 47–56.
- `crates/api/src/data/config.rs`: `ServiceArguments` at lines 46–156, `#[derive(Debug, clap::Parser)]` at line 46, `internal_token` field at line 57.
- `crates/common/src/ingest.rs`: `INTERNAL_TOKEN_HEADER` at line 22, `post_batch` at lines 35–57, `fetch_last_fetched` at lines 99–112 — confirmed exact.
- All seven pollers/services confirmed to declare an identically-shaped `internal_token: String` field via `#[arg(long, env)]`: `crates/poller-ldbws/src/config.rs:45`, `crates/poller-stations/src/config.rs:29`, `crates/poller-incidents/src/config.rs:30`, `crates/poller-tocs/src/config.rs:30`, `crates/poller-tfl/src/config.rs:40`, `crates/trust-consumer/src/config.rs:66`, `crates/schedule-ingest/src/config.rs:56`.
- `crates/api/src/data/users.rs`: `grep -n "role\|permission\|scope\|group"` returns nothing, confirming the spec's "no roles vocabulary today" claim.
- `crates/api/src/routes/train.rs:9–11`'s 404-not-403 ownership-convention comment: quote independently re-confirmed verbatim.
- **Corrected finding, not in the spec:** the spec's "Current relevant state" section claims `docker-compose.yml` has **nine** `INTERNAL_TOKEN` occurrences ("nine occurrences of the same variable"). Direct count in this worktree: **eight** — `grep -n "INTERNAL_TOKEN" docker-compose.yml` returns lines 96 (`api`), 146 (`poller-incidents`), 167 (`poller-stations`), 191 (`poller-tocs`), 216 (`poller-ldbws`), 241 (`poller-tfl`), 322 (`trust-consumer`), 480 (`schedule-ingest`'s `ingest` container) — one `api` consumer plus the seven real callers, matching this plan's task count exactly. Task 6 below uses eight, not nine.
- **Corrected finding, not in the spec:** the spec repeatedly cites "Decision 6" (auditability/per-request identity logging) as if it were a numbered section (in Decision 3's own reasoning, in Decision 5, and three times in the Testing section) — but the spec's `## Decisions` section only numbers **1 through 5** (`### 1.` … `### 5. Rollout`); there is no `### 6.` heading anywhere in the 669-line document. The auditability content "Decision 6" points at is real (it's argued inline in Decision 3 and required by the Testing section) but was never written up as its own decision. This plan treats it as a real requirement — Task 7 below — but cites it as "the design's auditability requirement," not "Decision 6," since that heading does not exist in the spec as written.
- `charts/distant-signal/templates/secret.yaml`: internal-token block at lines 36–41 (spec cited "36–41", confirmed exact), per-poller `rdm-<name>-api-key` block at lines 54–62 (confirmed, **gated on `$poller.enabled`** — load-bearing for Task 5, see below).
- `charts/distant-signal/templates/_helpers.tpl`: `distant-signal.internalTokenSecretName`/`internalTokenSecretKey` at lines 186–196 (spec cited "185–195", off by one but same content), `distant-signal.pollerSecretName`/`pollerSecretKey` at lines 223–233.
- `charts/distant-signal/templates/poller-deployments.yaml`: single template, `range $name, $poller := .Values.pollers` at line 10, the `INTERNAL_TOKEN` env entry at lines 97–101, the sibling `RDM_API_KEY`-shaped entry (`pollerSecretName`/`pollerSecretKey`) at lines 92–96 — this plan's Task 5 extends this exact block.
- `charts/distant-signal/templates/api-deployment.yaml`: `INTERNAL_TOKEN` at lines 105–109, container name `api` at line 90.
- `charts/distant-signal/templates/trust-consumer-deployment.yaml`: `INTERNAL_TOKEN` at lines 103–107, container name `trust-consumer` at line 56.
- `charts/distant-signal/templates/schedulefeed-deployment.yaml`: `INTERNAL_TOKEN` at lines 207–211, inside the `ingest` container block (name at line 177) — the `schedulefeed` Pod's second of two containers (`sftp`, `ingest`), not a third container as this plan initially assumed from a loose reading; independently confirmed by reading the file.
- `charts/distant-signal/templates/schedulefeed-secret.yaml`: full file (89 lines) — the "this app mints the credential" precedent, `password` auto-generated via the identical `override -> preserve-existing -> randAlphaNum` chain at lines 70–73.
- `charts/distant-signal/values.yaml`: `secrets` block at lines 36–44, `pollers.incidents`/`.stations`/`.tfl`/`.tocs`/`.ldbws` blocks at lines 143–272 (each with its own `apiKey`/`existingSecret`/`existingSecretApiKeyKey` trio, confirmed identical shape across all five).
- `dev.env.example`: duplication-warning header at lines 8–14, `INTERNAL_TOKEN=changeme-shared-secret-local-dev-only` at line 120. `local.env.example`: matching `INTERNAL_TOKEN` at line 81.

## Global Constraints

- **No database table, no migration, anywhere in this plan.** Decision 2 explicitly rejects a DB-backed ACL/service-account table — the route-scoping table is a static Rust `match`/array in `crates/api/src/auth.rs`, and the credential lookup is an in-memory `HashMap` built once at `AppState::init()` from config, not a `sessions`-style DB table. No task creates a file under `crates/api/migrations/`.
- **The wire mechanism does not change.** Still one bearer token in the `X-Internal-Token` header (`common::ingest::INTERNAL_TOKEN_HEADER`, unchanged). `common::ingest::post_batch`/`fetch_last_fetched` (`crates/common/src/ingest.rs:35–57`, `:99–112`) are not touched by any task. **No `poller-*`/`trust-consumer`/`schedule-ingest` crate's Rust source is modified anywhere in this plan** — every one of the seven services already reads one opaque `internal_token: &str` from its own `config.rs`; only the chart/compose value it's handed changes (Tasks 5–6). If a future task ever needs to touch a poller's own code for this feature, that is a sign this plan's premise (Decision 1: "the minimal edit to the existing mechanism") has been violated.
- **Status codes, per Decision 3: unknown/invalid token → `401`; known, valid, wrong-scope token → `403`.** This is a deliberate departure from this app's own 404-for-ownership convention (`crates/api/src/routes/train.rs:9–11`) — do not "fix" a `403` in this feature back to `404` in review; the spec's Decision 3 argues at length why the ownership convention's reasoning (withholding a resource's existence from an untrusted human prober) does not apply to a trusted internal service hitting a publicly-visible, fixed route table.
- **The legacy shared token is not deleted or renamed by this plan.** `secrets.internalToken` / `INTERNAL_TOKEN` (chart key and env var name) keeps meaning exactly what it means today throughout every task here; it becomes the source for `InternalService::Legacy`, allowed every route, logged at `warn` on every use (Decision 5). Retiring it is explicitly **out of scope** for this plan (Task 8 describes the runbook that leads to a *future* plan doing so).
- **No new per-service Rust code path beyond the seven real `InternalService` variants plus `Legacy`.** `schedule-reference` does not exist in this codebase yet (confirmed: `find crates -maxdepth 1 -type d` lists no such crate) — no task speculatively adds a placeholder variant or route-prefix entry for it, per the spec's own "Explicitly out of scope."
- **No test in this plan touches a live database.** Every new type/function this plan adds (`InternalService`, `InternalServiceRegistry`, `classify_internal_request`) is pure or operates on in-memory state built from config — unlike `crates/api/src/data/queries.rs`'s DB-touching tests (`#[ignore = "requires a live database..."]`, e.g. `queries.rs:1024–1059`), nothing here needs that convention. Every test in this plan runs under a plain `cargo test -p api`.
- **No log-capture test dependency is introduced.** This codebase has no existing convention for asserting a `tracing::warn!`/`info!` call actually fired (confirmed: no `tracing-test`, no custom subscriber/`MockWriter` anywhere in the workspace). Rather than add one, Task 3 factors the auth decision into a pure `classify_internal_request` function that returns a plain enum; `require_internal_token` itself just matches on that enum to pick a status code and a tracing macro. Tests assert on the enum, never on log output — this is a deliberate design choice this plan makes (the spec leaves the exact mechanism open), not an implementation detail to improvise later.
- **`ServiceArguments` already derives `Debug` directly (`crates/api/src/data/config.rs:46`), and already carries three unredacted secrets that way (`internal_token`, `sso_client_secret`, `database_url` — per `app.rs`'s own comment, lines 34–37).** The seven new per-service token fields (Task 2) inherit this same pre-existing risk/mitigation posture — "never `tracing::debug!(?app.config, ...)`" is already the rule, not a new one this plan invents. No task adds a new mitigation for this; it is noted so a reviewer doesn't ask for one as new scope.
- **Helm per-service secret keys must render regardless of `pollers.<name>.enabled`.** The existing `rdm-<name>-api-key` block (`secret.yaml:54–62`) is gated on `{{- if and $poller.enabled (not $poller.existingSecret) -}}` — correct for that key (a random RDM key is meaningless when unused), but wrong for a per-service internal token: `api`'s own Deployment always renders and always needs a value for every `InternalService` variant to build its startup registry, even for a poller a given deployment doesn't run (exactly parallel to how the single `internal-token` value is already unconditionally generated today, independent of any poller's `enabled` flag). Task 5 renders the seven new keys in a **separate, unconditionally-iterating** block, gated only on `not $poller.existingSecret` (or the sibling non-poller `existingSecret`, for trust-consumer/schedule-ingest), never on `enabled`.
- **Parallelizable tasks:** Task 1 is foundational (Rust-only, no dependency). Task 2 depends on Task 1. Task 3 depends on Tasks 1–2. Tasks 4 and 7 both depend on Task 3 and both edit `require_internal_token`'s body — **not parallelizable with each other** (sequence 4 then 7, or merge by hand); either may run parallel with Tasks 5–6. Tasks 5 (Helm) and 6 (local dev) depend only on Task 2's field/env-var names being final, not on Tasks 3/4/7's Rust logic landing — dispatch in parallel with each other and with 3/4/7. Task 8 (rollout runbook) depends on 4, 5, and 6 all landing. Task 9 (end-to-end verification) depends on everything.

---

### Task 1: `InternalService` enum, static route-scoping table, and the token-hash registry (pure, no config wiring yet)

**Files:**
- Modify: `crates/api/src/auth.rs`

**Interfaces:**
- Produces: `pub enum InternalService { PollerIncidents, PollerStations, PollerTocs, PollerLdbws, PollerTfl, TrustConsumer, ScheduleIngest, Legacy }`, `impl InternalService { fn allows(&self, path: &str) -> bool }`, `pub struct InternalServiceRegistry(HashMap<String, InternalService>)` with `fn from_tokens(pairs: &[(InternalService, &str)]) -> Self` and `fn resolve(&self, token: &str) -> Option<InternalService>`.
- Consumed by: Task 2 (`AppState::init` builds the registry from config), Task 3 (`require_internal_token` calls `resolve`/`allows`).
- **Depends on:** nothing — foundational.

This is the code-defined route-scoping table Decision 2 calls for, built directly from the confirmed route surface (`crates/api/src/routes/ingest.rs`'s `router()`, `crates/api/src/routes/samples.rs`'s `router()`, cross-checked against each service's own `config.rs` default `*_url`, per the Status note above):

| Route prefix | `InternalService` |
|---|---|
| `/incidents` | `PollerIncidents` |
| `/stations` | `PollerStations` |
| `/tocs` | `PollerTocs` |
| `/sample-stations`, `/station-samples` | `PollerLdbws` (the one service with two allowed prefixes — Decision 2's stated reason the primitive is "a list of prefixes," not a single string) |
| `/tfl-line-status` | `PollerTfl` |
| `/train-events`, `/tracked-trains` | `TrustConsumer` |
| `/schedule-feed-ingests` | `ScheduleIngest` |

- [ ] **Step 1: Add `InternalService` and its scoping table**

Add near the top of `crates/api/src/auth.rs`, after the existing `use` statements:

```rust
/// One variant per legitimate caller of `private_router()`, plus `Legacy`
/// for the pre-migration shared token (Decision 5). A static, code-defined
/// table -- not a DB-backed ACL -- per Decision 2: the caller set is small,
/// changes only alongside a code deploy, and `require_internal_token` runs
/// on every `/private/*` request, including poller-ldbws's every-60s
/// station-sample POST, so this must stay a zero-I/O check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InternalService {
    PollerIncidents,
    PollerStations,
    PollerTocs,
    PollerLdbws,
    PollerTfl,
    TrustConsumer,
    ScheduleIngest,
    /// The pre-migration shared `X-Internal-Token` value (Decision 5).
    /// Allowed every route, unlike every other variant -- see `allows`.
    /// Retiring this variant (and the config field it's built from) is a
    /// separate, later plan; see this plan's Task 8.
    Legacy,
}

impl InternalService {
    /// The route prefixes this identity may reach, matched against
    /// `request.uri().path()` with a plain `starts_with` -- no
    /// `MatchedPath` extractor, matching this file's own hand-rolled
    /// posture for something this narrow (see `parse_cookie`'s doc
    /// comment). A route added to `ingest.rs`/`samples.rs` without a
    /// matching entry here is default-denied for every real service (see
    /// Step 3's regression test), including `Legacy` -- no, wait: Legacy
    /// bypasses this table entirely, see `allows` below.
    fn allowed_prefixes(&self) -> &'static [&'static str] {
        match self {
            InternalService::PollerIncidents => &["/incidents"],
            InternalService::PollerStations => &["/stations"],
            InternalService::PollerTocs => &["/tocs"],
            InternalService::PollerLdbws => &["/sample-stations", "/station-samples"],
            InternalService::PollerTfl => &["/tfl-line-status"],
            InternalService::TrustConsumer => &["/train-events", "/tracked-trains"],
            InternalService::ScheduleIngest => &["/schedule-feed-ingests"],
            // Legacy never consults this list -- see `allows`. Returning
            // an empty slice here (rather than every real prefix) means
            // this list never needs updating when a new service/route is
            // added; only `allows`'s one early-return needs to exist.
            InternalService::Legacy => &[],
        }
    }

    /// `Legacy` is allowed everywhere -- today's actual behavior,
    /// preserved for the Decision 5 transition window. Every other
    /// variant is checked against its own `allowed_prefixes`.
    pub fn allows(&self, path: &str) -> bool {
        if matches!(self, InternalService::Legacy) {
            return true;
        }
        self.allowed_prefixes().iter().any(|prefix| path.starts_with(prefix))
    }
}
```

- [ ] **Step 2: Add the token-hash registry**

Add below `InternalService`, reusing `hash_session_token` (already public in this file, generic over any token string -- Decision 1's own suggestion: "the existing `hash_session_token` function, or a sibling with the same shape"; this plan reuses it directly rather than duplicating it):

```rust
use std::collections::HashMap;

/// Opaque-token -> `InternalService` lookup, built once at startup from
/// config (`AppState::init`, Task 2) -- not a DB table (Decision 2).
/// Mirrors `AuthenticatedUser`'s own "hash the presented token, look up
/// the hash" shape (`hash_session_token` / `get_session_with_user`), just
/// against an in-memory map instead of the `sessions` table, since this
/// set is small and fixed at deploy time.
#[derive(Debug, Default)]
pub struct InternalServiceRegistry(HashMap<String, InternalService>);

impl InternalServiceRegistry {
    /// `pairs` is `(identity, raw token)` -- every entry's token is hashed
    /// before being stored, exactly like a session token, so a leaked
    /// startup panic message or `{:?}` dump of this struct never contains
    /// a raw credential. A duplicate raw token across two different
    /// identities silently lets the later entry win (last-write-wins on
    /// the same hash key) -- not guarded against here; Task 2's startup
    /// validation is the place that would catch an operator accidentally
    /// reusing one token for two services, if that's ever added.
    pub fn from_tokens(pairs: &[(InternalService, &str)]) -> Self {
        let mut map = HashMap::with_capacity(pairs.len());
        for (identity, token) in pairs {
            map.insert(hash_session_token(token), *identity);
        }
        InternalServiceRegistry(map)
    }

    pub fn resolve(&self, token: &str) -> Option<InternalService> {
        self.0.get(&hash_session_token(token)).copied()
    }
}
```

- [ ] **Step 3: Add unit tests**

Add a new `mod internal_service_tests` alongside the existing `#[cfg(test)] mod tests` (`auth.rs:213`):

```rust
#[cfg(test)]
mod internal_service_tests {
    use super::*;

    fn registry() -> InternalServiceRegistry {
        InternalServiceRegistry::from_tokens(&[
            (InternalService::PollerIncidents, "tok-incidents"),
            (InternalService::PollerStations, "tok-stations"),
            (InternalService::PollerTocs, "tok-tocs"),
            (InternalService::PollerLdbws, "tok-ldbws"),
            (InternalService::PollerTfl, "tok-tfl"),
            (InternalService::TrustConsumer, "tok-trust"),
            (InternalService::ScheduleIngest, "tok-schedule"),
            (InternalService::Legacy, "tok-legacy"),
        ])
    }

    #[test]
    fn a_known_token_resolves_to_its_identity() {
        assert_eq!(registry().resolve("tok-incidents"), Some(InternalService::PollerIncidents));
    }

    #[test]
    fn an_unknown_token_resolves_to_none() {
        assert_eq!(registry().resolve("not-a-real-token"), None);
    }

    #[test]
    fn an_empty_token_resolves_to_none_even_against_an_empty_registry() {
        // Mirrors auth.rs's existing empty_provided_against_real_token_does_not_match
        // (line 238) for the old single-token scheme -- an empty presented
        // token must never accidentally match anything, including a
        // registry with no entries at all.
        let empty = InternalServiceRegistry::default();
        assert_eq!(empty.resolve(""), None);
    }

    // Table-driven scope-enforcement test: every real (InternalService, route)
    // pair from this task's route table allows; at least one OTHER service's
    // identity against that same route is denied. Regression guard for "a
    // newly added /private/* route forgets to declare who may call it."
    #[test]
    fn each_service_is_allowed_only_its_own_routes() {
        let cases: &[(InternalService, &str)] = &[
            (InternalService::PollerIncidents, "/incidents"),
            (InternalService::PollerStations, "/stations"),
            (InternalService::PollerTocs, "/tocs"),
            (InternalService::PollerLdbws, "/sample-stations"),
            (InternalService::PollerLdbws, "/station-samples"),
            (InternalService::PollerTfl, "/tfl-line-status"),
            (InternalService::TrustConsumer, "/train-events"),
            (InternalService::TrustConsumer, "/tracked-trains"),
            (InternalService::ScheduleIngest, "/schedule-feed-ingests"),
        ];
        let all_services = [
            InternalService::PollerIncidents,
            InternalService::PollerStations,
            InternalService::PollerTocs,
            InternalService::PollerLdbws,
            InternalService::PollerTfl,
            InternalService::TrustConsumer,
            InternalService::ScheduleIngest,
        ];

        for (owner, route) in cases {
            assert!(owner.allows(route), "{owner:?} must be allowed on its own route {route}");
            for other in all_services.iter().filter(|s| *s != owner) {
                assert!(
                    !other.allows(route),
                    "{other:?} must NOT be allowed on {owner:?}'s route {route}"
                );
            }
        }
    }

    #[test]
    fn legacy_is_allowed_on_every_real_route() {
        for route in [
            "/incidents", "/stations", "/tocs", "/sample-stations", "/station-samples",
            "/tfl-line-status", "/train-events", "/tracked-trains", "/schedule-feed-ingests",
        ] {
            assert!(InternalService::Legacy.allows(route), "Legacy must be allowed on {route}");
        }
    }

    #[test]
    fn a_route_not_in_any_services_allowlist_is_denied_for_every_real_identity() {
        // Default-deny guard: a fabricated route present in nobody's
        // allowlist must never accidentally pass for a real (non-Legacy)
        // identity.
        let fabricated = "/some-future-route-nobody-declared";
        for service in [
            InternalService::PollerIncidents,
            InternalService::PollerStations,
            InternalService::PollerTocs,
            InternalService::PollerLdbws,
            InternalService::PollerTfl,
            InternalService::TrustConsumer,
            InternalService::ScheduleIngest,
        ] {
            assert!(!service.allows(fabricated), "{service:?} must not be allowed on {fabricated}");
        }
    }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test -p api internal_service_tests::`
Expected: PASS (7 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/auth.rs
git commit -m "Add InternalService enum, static route-scoping table, and token-hash registry"
```

---

### Task 2: Per-service token config fields + startup registry wiring

**Files:**
- Modify: `crates/api/src/data/config.rs`
- Modify: `crates/api/src/app.rs`

**Interfaces:**
- Produces: seven new `ServiceArguments` fields (below); `AppState.internal_services: auth::InternalServiceRegistry` (new field).
- Consumed by: Task 3 (`require_internal_token` reads `app.internal_services`).
- **Depends on:** Task 1 (`InternalService`/`InternalServiceRegistry` must exist).

Field/env-var naming (Open Question 1 in the spec leaves this unresolved; this plan makes the call: one explicit, named `clap` field per known service, mirroring `pollers.<name>.apiKey`'s one-field-per-known-poller chart shape rather than a dynamically-sized map — consistent with `internal_token`'s own existing single-field shape):

| Field | Env var |
|---|---|
| `internal_token_poller_incidents` | `INTERNAL_TOKEN_POLLER_INCIDENTS` |
| `internal_token_poller_stations` | `INTERNAL_TOKEN_POLLER_STATIONS` |
| `internal_token_poller_tocs` | `INTERNAL_TOKEN_POLLER_TOCS` |
| `internal_token_poller_ldbws` | `INTERNAL_TOKEN_POLLER_LDBWS` |
| `internal_token_poller_tfl` | `INTERNAL_TOKEN_POLLER_TFL` |
| `internal_token_trust_consumer` | `INTERNAL_TOKEN_TRUST_CONSUMER` |
| `internal_token_schedule_ingest` | `INTERNAL_TOKEN_SCHEDULE_INGEST` |

The existing `internal_token` field (`config.rs:57`) is untouched — it becomes `Legacy`'s source, not one of the seven.

**Why these seven can safely be `#[arg(long, env)]` (required, no default) exactly like `internal_token` already is, not optional:** the Helm chart (Task 5) auto-generates all seven per-service secret keys unconditionally in the same `Secret` object `internal-token` already lives in, as part of the *same* `helm upgrade`/chart version bump that ships the new `api` image — so by the time a new `api` pod boots, its own chart release has already rendered every key it needs, the same way `internal_token` itself is already guaranteed present today. This is a different concern from Decision 5's poller-rollout-ordering problem (which is about *poller* pods lagging behind on which *value* they send, not about whether the *value exists in the cluster at all*).

- [ ] **Step 1: Add the seven fields to `ServiceArguments`**

In `crates/api/src/data/config.rs`, immediately after `internal_token` (currently line 57):

```rust
    /// Per-service internal tokens (Decision 1/2,
    /// docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md).
    /// Each is this app's OWN minted credential for exactly one real
    /// caller -- see `auth::InternalService` for the identity each maps
    /// to and which /private/* routes it may reach. `internal_token`
    /// above stays wired to `auth::InternalService::Legacy` (allowed
    /// every route, Decision 5's dual-acceptance window), not replaced by
    /// these.
    #[arg(long, env)]
    pub internal_token_poller_incidents: String,
    #[arg(long, env)]
    pub internal_token_poller_stations: String,
    #[arg(long, env)]
    pub internal_token_poller_tocs: String,
    #[arg(long, env)]
    pub internal_token_poller_ldbws: String,
    #[arg(long, env)]
    pub internal_token_poller_tfl: String,
    #[arg(long, env)]
    pub internal_token_trust_consumer: String,
    #[arg(long, env)]
    pub internal_token_schedule_ingest: String,
```

- [ ] **Step 2: Wire the registry into `AppState`**

In `crates/api/src/app.rs`, add the field to `AppState` (after `oidc`, currently line 25):

```rust
    /// Startup-built token-hash -> identity lookup for `/private/*`
    /// (Decision 1/2). Built once in `init`, immutable afterward -- no
    /// runtime credential add/remove without a redeploy, per Decision 2's
    /// explicit rejection of a DB-backed, dynamically-editable table.
    pub internal_services: crate::auth::InternalServiceRegistry,
```

Add a placeholder to the hand-rolled `Debug` impl (currently lines 47–56), matching its existing "fixed placeholder, never a real dump" posture:

```rust
            .field("internal_services", &"InternalServiceRegistry { .. }")
```

In `AppState::init()`, after the existing `internal_token` non-empty guard (currently lines 65–72), add the seven new guards and build the registry:

```rust
        for (name, value) in [
            ("internal_token_poller_incidents", &config.internal_token_poller_incidents),
            ("internal_token_poller_stations", &config.internal_token_poller_stations),
            ("internal_token_poller_tocs", &config.internal_token_poller_tocs),
            ("internal_token_poller_ldbws", &config.internal_token_poller_ldbws),
            ("internal_token_poller_tfl", &config.internal_token_poller_tfl),
            ("internal_token_trust_consumer", &config.internal_token_trust_consumer),
            ("internal_token_schedule_ingest", &config.internal_token_schedule_ingest),
        ] {
            ensure!(!value.is_empty(), "{name} must not be empty (see --{}/{})", name.replace('_', "-"), name.to_uppercase());
        }

        let internal_services = crate::auth::InternalServiceRegistry::from_tokens(&[
            (crate::auth::InternalService::Legacy, config.internal_token.as_str()),
            (crate::auth::InternalService::PollerIncidents, config.internal_token_poller_incidents.as_str()),
            (crate::auth::InternalService::PollerStations, config.internal_token_poller_stations.as_str()),
            (crate::auth::InternalService::PollerTocs, config.internal_token_poller_tocs.as_str()),
            (crate::auth::InternalService::PollerLdbws, config.internal_token_poller_ldbws.as_str()),
            (crate::auth::InternalService::PollerTfl, config.internal_token_poller_tfl.as_str()),
            (crate::auth::InternalService::TrustConsumer, config.internal_token_trust_consumer.as_str()),
            (crate::auth::InternalService::ScheduleIngest, config.internal_token_schedule_ingest.as_str()),
        ]);
```

And add `internal_services` to the final `Ok(Arc::new(Self { .. }))` construction (currently lines 102–107).

- [ ] **Step 3: Build the whole workspace**

Run: `cargo build --workspace`
Expected: PASS — this only touches `crates/api`, but a full-workspace build confirms nothing downstream (there is nothing downstream of `api`'s own config) broke.

- [ ] **Step 4: Commit**

```bash
git add crates/api/src/data/config.rs crates/api/src/app.rs
git commit -m "Add per-service internal-token config fields and build the startup registry"
```

---

### Task 3: `require_internal_token` middleware rewrite (401 / 403, pure classification)

**Files:**
- Modify: `crates/api/src/auth.rs`

**Interfaces:**
- Produces: `pub(crate) enum InternalAuthOutcome { Unknown, Forbidden(InternalService), Allowed(InternalService) }`, `pub(crate) fn classify_internal_request(registry: &InternalServiceRegistry, token: &str, path: &str) -> InternalAuthOutcome` (pure — no logging, no I/O). `require_internal_token` rewritten to call it and translate the outcome into a status code (Task 4 and Task 7 each add one more concern to what `require_internal_token` does with a given outcome — see their own Steps).
- Consumed by: Task 4 (`Allowed(Legacy)` gets its own warn log), Task 7 (every `Allowed`/`Forbidden` gets a structured log line).
- **Depends on:** Tasks 1–2.

- [ ] **Step 1: Add the pure classification function**

Add to `crates/api/src/auth.rs`, near `InternalServiceRegistry`:

```rust
/// Outcome of resolving one `/private/*` request's presented token against
/// the requested path -- deliberately a plain, loggable-and-testable enum
/// rather than folding straight into a `StatusCode`, so the decision of
/// "was this request allowed, and by which identity" is unit-testable
/// without capturing `tracing` output (this codebase has no existing
/// log-capture test convention -- see this plan's Global Constraints).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InternalAuthOutcome {
    /// No identity resolved at all -- unchanged failure mode from today's
    /// single-token scheme. Always `401`.
    Unknown,
    /// A real identity resolved, but it isn't allowed on this path. Always
    /// `403` (Decision 3) -- deliberately NOT this app's usual
    /// 404-for-ownership convention; see `require_internal_token`'s own
    /// doc comment for why.
    Forbidden(InternalService),
    /// A real identity resolved and is allowed here. `next.run(...)`
    /// proceeds; the caller decides what (if anything) to log, since
    /// `InternalService::Legacy` gets an extra `warn` (Task 4) that a
    /// real per-service identity doesn't.
    Allowed(InternalService),
}

pub(crate) fn classify_internal_request(
    registry: &InternalServiceRegistry,
    token: &str,
    path: &str,
) -> InternalAuthOutcome {
    let Some(identity) = registry.resolve(token) else {
        return InternalAuthOutcome::Unknown;
    };
    if identity.allows(path) {
        InternalAuthOutcome::Allowed(identity)
    } else {
        InternalAuthOutcome::Forbidden(identity)
    }
}
```

- [ ] **Step 2: Rewrite `require_internal_token`**

Replace the current body (`auth.rs:20–36`):

```rust
/// `axum::middleware::from_fn` handler enforcing per-service internal
/// auth. Resolves the presented `X-Internal-Token` value against
/// `app.internal_services` (Decision 1/2), then checks the resolved
/// identity against the requested path (`InternalService::allows`).
///
/// Status codes, per Decision 3 -- a deliberate departure from this app's
/// usual 404-for-ownership convention (`crate::routes::train`'s module
/// doc): unknown token -> `401` (no identity resolved at all, unchanged
/// from today). Known, valid, WRONG-SCOPE token -> `403` (a real
/// credential, just not for this route) -- not `404`, because the thing
/// being protected is a fixed, publicly-visible route table (no existence
/// fact to withhold) and the caller is a trusted internal service (not an
/// untrusted human prober), so a `403` is diagnosable signal for a
/// misconfigured deployment, not an information leak.
pub async fn require_internal_token(
    State(app): State<App>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let provided = request
        .headers()
        .get(INTERNAL_TOKEN_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let path = request.uri().path().to_string();

    match classify_internal_request(&app.internal_services, provided, &path) {
        InternalAuthOutcome::Unknown => Err(StatusCode::UNAUTHORIZED),
        InternalAuthOutcome::Forbidden(identity) => {
            tracing::warn!(?identity, path, "internal request rejected: valid credential, wrong scope");
            Err(StatusCode::FORBIDDEN)
        }
        InternalAuthOutcome::Allowed(identity) => {
            // Task 4 adds a Legacy-specific warn here; Task 7 adds a
            // general per-request identity log for every Allowed case.
            Ok(next.run(request).await)
        }
    }
}
```

- [ ] **Step 3: Remove the now-dead `constant_time_eq` and its five tests**

Nothing calls `constant_time_eq` after Step 2 — `require_internal_token` no longer compares against one fixed string. Delete the function (`auth.rs:38–56`, including its doc comment) and its five tests in the existing `#[cfg(test)] mod tests` block: `equal_tokens_match`, `different_content_same_length_does_not_match`, `different_length_does_not_match`, `empty_tokens_match`, `empty_provided_against_real_token_does_not_match` (currently `auth.rs:217–240`). Also rewrite this file's module doc comment (currently lines 1–6, describing the old single-shared-secret mechanism) to describe the new per-service scheme:

```rust
//! Internal-auth gate for `private_router()`.
//!
//! Per-service bearer tokens (`X-Internal-Token`), resolved via a
//! startup-built hash lookup (`InternalServiceRegistry`) into an
//! `InternalService` identity, then checked against the requested route's
//! allowed prefixes. A legacy shared-token identity (`InternalService::Legacy`)
//! is allowed every route during a bounded rollout window -- see
//! docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md.
//! This is intentionally not a general auth framework -- just enough to
//! keep the ingestion endpoints reachable only by their own legitimate
//! caller.
```

- [ ] **Step 4: Add tests for `classify_internal_request`**

Alongside `internal_service_tests` (Task 1, Step 3):

```rust
    #[test]
    fn classify_returns_unknown_for_an_unresolvable_token() {
        let reg = registry();
        assert_eq!(
            classify_internal_request(&reg, "not-a-real-token", "/incidents"),
            InternalAuthOutcome::Unknown
        );
    }

    #[test]
    fn classify_returns_allowed_for_a_services_own_route() {
        let reg = registry();
        assert_eq!(
            classify_internal_request(&reg, "tok-incidents", "/incidents"),
            InternalAuthOutcome::Allowed(InternalService::PollerIncidents)
        );
    }

    #[test]
    fn classify_returns_forbidden_for_a_valid_token_on_another_services_route() {
        let reg = registry();
        assert_eq!(
            classify_internal_request(&reg, "tok-tfl", "/schedule-feed-ingests"),
            InternalAuthOutcome::Forbidden(InternalService::PollerTfl)
        );
    }

    #[test]
    fn classify_returns_allowed_for_legacy_on_any_route() {
        let reg = registry();
        assert_eq!(
            classify_internal_request(&reg, "tok-legacy", "/schedule-feed-ingests"),
            InternalAuthOutcome::Allowed(InternalService::Legacy)
        );
    }
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p api auth::`
Expected: PASS. `cargo test -p api` overall must still pass (confirms nothing else in the crate referenced `constant_time_eq`).

- [ ] **Step 6: Commit**

```bash
git add crates/api/src/auth.rs
git commit -m "Rewrite require_internal_token: per-service scope check, 401/403, pure classification"
```

---

### Task 4: Dual-acceptance — `Legacy` identity, warn-on-use logging, its own regression tests

**Files:**
- Modify: `crates/api/src/auth.rs`

**Interfaces:**
- Produces: `require_internal_token`'s `Allowed` arm gains a `Legacy`-specific `tracing::warn!`.
- Consumed by: Task 8 (the rollout runbook's "confirmed migrated" signal is this exact log line disappearing from `api`'s logs).
- **Depends on:** Task 3.

Decision 5's dual-acceptance window is already mechanically in place after Task 3 (`Legacy` resolves and `allows` every route) — what's left is the requirement the spec's Testing section states explicitly: "the legacy shared token still resolves and is still granted every route during the transition window, **and doing so is observably logged**... this is the mechanism the 'safe to retire' decision depends on, so it needs its own coverage, not just an assumption that logging happens." This task adds that logging and pins it with a test on the (already-testable, per Task 3's design) outcome enum — not on log capture.

- [ ] **Step 1: Add the Legacy-specific warn in `require_internal_token`'s `Allowed` arm**

Extend the `Allowed(identity)` match arm from Task 3, Step 2:

```rust
        InternalAuthOutcome::Allowed(identity) => {
            if matches!(identity, InternalService::Legacy) {
                tracing::warn!(
                    path,
                    "legacy shared X-Internal-Token used -- migrate this caller to its own per-service token \
                     (see docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md Decision 5)"
                );
            }
            Ok(next.run(request).await)
        }
```

- [ ] **Step 2: Add a helper so the "is this the Legacy path" branch is itself directly testable**

To avoid ever needing to assert on log output (Global Constraints), extract the boolean the `if` above is really deciding:

```rust
impl InternalAuthOutcome {
    /// Whether this outcome is the pre-migration shared-token identity
    /// succeeding -- the exact condition `require_internal_token` uses to
    /// decide whether to emit the Decision 5 migration-nudge warning.
    /// Pulled out as its own function so it's unit-testable without
    /// capturing `tracing` output.
    pub(crate) fn is_legacy_success(&self) -> bool {
        matches!(self, InternalAuthOutcome::Allowed(InternalService::Legacy))
    }
}
```

Update `require_internal_token`'s `Allowed` arm to call this instead of matching inline:

```rust
        InternalAuthOutcome::Allowed(identity) => {
            let outcome = InternalAuthOutcome::Allowed(identity);
            if outcome.is_legacy_success() {
                tracing::warn!(path, "legacy shared X-Internal-Token used -- migrate this caller to its own per-service token");
            }
            Ok(next.run(request).await)
        }
```

- [ ] **Step 3: Add the regression tests**

```rust
    #[test]
    fn legacy_success_is_flagged_for_the_migration_warning() {
        assert!(InternalAuthOutcome::Allowed(InternalService::Legacy).is_legacy_success());
    }

    #[test]
    fn a_real_per_service_success_is_not_flagged_as_legacy() {
        assert!(!InternalAuthOutcome::Allowed(InternalService::PollerIncidents).is_legacy_success());
    }

    #[test]
    fn forbidden_and_unknown_are_never_flagged_as_legacy_success() {
        assert!(!InternalAuthOutcome::Forbidden(InternalService::Legacy).is_legacy_success());
        assert!(!InternalAuthOutcome::Unknown.is_legacy_success());
    }
```

(The middle case can't actually occur — `Legacy.allows(_)` is always `true`, so `Forbidden(Legacy)` is unreachable in practice — but the test still pins `is_legacy_success`'s literal definition against a future refactor that might loosen `allows` for `Legacy`.)

- [ ] **Step 4: Run the tests**

Run: `cargo test -p api auth::`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/api/src/auth.rs
git commit -m "Warn-log every use of the legacy shared internal token (Decision 5 dual-acceptance)"
```

---

### Task 5: Helm chart — per-service secret keys + Deployment wiring for all 7 services

**Files:**
- Modify: `charts/distant-signal/templates/secret.yaml`
- Modify: `charts/distant-signal/templates/_helpers.tpl`
- Modify: `charts/distant-signal/values.yaml`
- Modify: `charts/distant-signal/templates/poller-deployments.yaml`
- Modify: `charts/distant-signal/templates/trust-consumer-deployment.yaml`
- Modify: `charts/distant-signal/templates/schedulefeed-deployment.yaml`
- Modify: `charts/distant-signal/templates/api-deployment.yaml`

**Interfaces:**
- Produces: seven new Secret keys (`internal-token-poller-incidents`, `-stations`, `-tocs`, `-ldbws`, `-tfl`, `internal-token-trust-consumer`, `internal-token-schedule-ingest`), auto-generated the same way `internal-token` is today; each of the 7 service Deployments/containers gets its own `secretKeyRef`; `api`'s Deployment gets all seven new env vars (Task 2's field names) alongside its unchanged `INTERNAL_TOKEN`.
- Consumed by: Task 8 (the rollout runbook operates on these values).
- **Depends on:** Task 2 (needs the final env-var names: `INTERNAL_TOKEN_POLLER_INCIDENTS`, etc.). Independent of Tasks 3/4/7's Rust logic.

**Why this is one coordinated task, not seven per-service tasks:** five of the seven services (`poller-incidents`, `-stations`, `-tocs`, `-ldbws`, `-tfl`) are rendered by a **single shared template** (`poller-deployments.yaml`'s one `range $name, $poller := .Values.pollers` loop, confirmed at line 10) — wiring in a per-poller `secretKeyRef` is inherently one edit to that one loop body, not five separate diffs; splitting it across five "tasks" would mean five agents editing the same lines of the same file. The remaining two (`trust-consumer`, `schedule-ingest`'s `ingest` container) are two more small, disjoint edits to their own already-separate Deployment templates. All seven land together as one mechanical, same-shape change across a handful of files — exactly the kind of task this repo's own plans (e.g. the STANOX plan's Task 7, wiring five/six Deployment objects' env blocks in one task) already treat as one unit, not N.

**Design call this task makes (Open Question 1 in the spec leaves the exact shape open):** per-poller tokens live inside each `pollers.<name>` values block (`internalToken` / `existingSecretInternalTokenKey`), directly parallel to the existing `apiKey` / `existingSecretApiKeyKey` pair already there — reusing the *same* `pollers.<name>.existingSecret` toggle and the *same* underlying Secret object apiKey already uses, just a second key within it. `trustConsumer` and `scheduleFeed.ingest` (not in the `pollers` map) get their own analogous, narrowly-scoped `existingSecret`/`existingSecretInternalTokenKey` pair each, mirroring `scheduleFeed.sftp.existingSecret`'s already-established "scoped to the one sub-block that needs it" pattern rather than a chart-wide toggle.

- [ ] **Step 1: Add values for the five pollers**

In `charts/distant-signal/values.yaml`, add to each of the five `pollers.<name>` blocks (e.g. `incidents`, currently lines 144–170), immediately after the existing `apiKey`/`existingSecret`/`existingSecretApiKeyKey` trio:

```yaml
    # -- This app's own minted credential for this poller to reach
    # /private/* (Decision 1/2 of the internal-service-accounts design).
    # Auto-generated like secrets.internalToken when empty -- unlike
    # apiKey above, a random value here is NOT meaningless, since this app
    # is both the issuer and the verifier.
    internalToken: ""
    existingSecretInternalTokenKey: internal-token-poller-incidents
```

(Repeat for `stations`, `tocs`, `ldbws`, `tfl`, each with its own `internal-token-poller-<name>` default key name.)

Add to `values.yaml`'s `trustConsumer` block:

```yaml
  # -- Set to read this app's own minted /private/* credential from a
  # pre-existing Secret instead of letting the chart generate one.
  existingSecret: ""
  internalToken: ""
  existingSecretInternalTokenKey: internal-token-trust-consumer
```

Add to `values.yaml`'s `scheduleFeed.ingest` block:

```yaml
    existingSecret: ""
    internalToken: ""
    existingSecretInternalTokenKey: internal-token-schedule-ingest
```

- [ ] **Step 2: Add the `_helpers.tpl` helpers**

Sibling to `distant-signal.pollerSecretName`/`pollerSecretKey` (`_helpers.tpl:223–233`):

```
{{/*
Resolved Secret name/key for one poller's OWN internal-token (distinct
from its RDM apiKey, same Secret object). Call as:
  {{ include "distant-signal.pollerSecretName" (dict "root" $ "poller" $p) }}
  {{ include "distant-signal.pollerInternalTokenSecretKey" (dict "root" $ "name" $name "poller" $p) }}
*/}}
{{- define "distant-signal.pollerInternalTokenSecretKey" -}}
{{- if .poller.existingSecret }}
{{- .poller.existingSecretInternalTokenKey }}
{{- else }}
{{- printf "internal-token-poller-%s" .name }}
{{- end }}
{{- end }}

{{/*
Resolved Secret name/key for trust-consumer's own internal-token.
*/}}
{{- define "distant-signal.trustConsumerInternalTokenSecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.trustConsumer.existingSecret }}
{{- end }}

{{- define "distant-signal.trustConsumerInternalTokenSecretKey" -}}
{{- if .Values.trustConsumer.existingSecret }}
{{- .Values.trustConsumer.existingSecretInternalTokenKey }}
{{- else }}
{{- print "internal-token-trust-consumer" }}
{{- end }}
{{- end }}

{{/*
Resolved Secret name/key for schedule-ingest's own internal-token.
*/}}
{{- define "distant-signal.scheduleIngestInternalTokenSecretName" -}}
{{- default (include "distant-signal.secretName" .) .Values.scheduleFeed.ingest.existingSecret }}
{{- end }}

{{- define "distant-signal.scheduleIngestInternalTokenSecretKey" -}}
{{- if .Values.scheduleFeed.ingest.existingSecret }}
{{- .Values.scheduleFeed.ingest.existingSecretInternalTokenKey }}
{{- else }}
{{- print "internal-token-schedule-ingest" }}
{{- end }}
{{- end }}
```

- [ ] **Step 3: Render the seven new keys in `secret.yaml`, unconditional on `enabled`**

Add a new block after the existing `rdm-<name>-api-key` block (`secret.yaml:54–62`) — deliberately **not** reusing that block's `and $poller.enabled` condition (see this task's Global-Constraints callout: `api`'s Deployment always needs a value for every service, whether or not this particular release runs that poller):

```
{{/* internal-token-poller-<name>: one per REAL caller, rendered
     regardless of pollers.<name>.enabled -- unlike rdm-<name>-api-key
     above, this app both mints and verifies this value, so it is never
     meaningless, and api's own Deployment (always rendered) needs a
     value for every service to build its startup registry even when a
     given release doesn't run that poller. Same override -> preserve ->
     randAlphaNum chain as internal-token itself. */}}
{{- range $name, $poller := .Values.pollers -}}
{{- if not $poller.existingSecret -}}
{{- $key := printf "internal-token-poller-%s" $name -}}
{{- $token := $poller.internalToken | default (get $existingData $key | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data $key ($token | b64enc) -}}
{{- end -}}
{{- end -}}

{{/* internal-token-trust-consumer / internal-token-schedule-ingest: same
     shape, same reasoning, for the two callers outside .Values.pollers. */}}
{{- if not .Values.trustConsumer.existingSecret -}}
{{- $tcToken := .Values.trustConsumer.internalToken | default (get $existingData "internal-token-trust-consumer" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "internal-token-trust-consumer" ($tcToken | b64enc) -}}
{{- end -}}

{{- if not .Values.scheduleFeed.ingest.existingSecret -}}
{{- $siToken := .Values.scheduleFeed.ingest.internalToken | default (get $existingData "internal-token-schedule-ingest" | b64dec) | default (randAlphaNum 32) -}}
{{- $_ := set $data "internal-token-schedule-ingest" ($siToken | b64enc) -}}
{{- end -}}
```

- [ ] **Step 4: Wire `poller-deployments.yaml`'s shared range loop**

In the existing `env:` block (`poller-deployments.yaml:72–110`), add one entry alongside the unchanged `INTERNAL_TOKEN` entry (lines 97–101) — this is the one edit that covers all five pollers, since it's inside the shared `range` loop:

```yaml
            - name: {{ printf "INTERNAL_TOKEN_POLLER_%s" ($name | upper) }}
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.pollerSecretName" (dict "root" $root "poller" $poller) }}
                  key: {{ include "distant-signal.pollerInternalTokenSecretKey" (dict "root" $root "name" $name "poller" $poller) }}
```

This entry is rendered into all five poller Deployments but is **inert on the poller side** — no poller's own `config.rs` reads `INTERNAL_TOKEN_POLLER_*` (Global Constraints: no poller code changes). It exists so `api`'s own env (below) has a matching name to copy the pattern from and so a future poller-side opt-in has zero chart work left to do; the poller's actual `INTERNAL_TOKEN` env var (unchanged) is what it authenticates with today and continues to.

- [ ] **Step 5: Wire `trust-consumer-deployment.yaml` and `schedulefeed-deployment.yaml`**

`trust-consumer-deployment.yaml`, alongside the unchanged `INTERNAL_TOKEN` entry (lines 103–107):

```yaml
            - name: INTERNAL_TOKEN_TRUST_CONSUMER
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.trustConsumerInternalTokenSecretName" . }}
                  key: {{ include "distant-signal.trustConsumerInternalTokenSecretKey" . }}
```

`schedulefeed-deployment.yaml`'s `ingest` container, alongside its unchanged `INTERNAL_TOKEN` entry (lines 207–211):

```yaml
            - name: INTERNAL_TOKEN_SCHEDULE_INGEST
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.scheduleIngestInternalTokenSecretName" . }}
                  key: {{ include "distant-signal.scheduleIngestInternalTokenSecretKey" . }}
```

- [ ] **Step 6: Wire all seven into `api-deployment.yaml`**

Alongside `api`'s own unchanged `INTERNAL_TOKEN` entry (`api-deployment.yaml:105–109`), add all seven — `api` is the one consumer that needs every value, to build its Task 2 registry:

```yaml
            - name: INTERNAL_TOKEN_POLLER_INCIDENTS
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.pollerSecretName" (dict "root" . "poller" .Values.pollers.incidents) }}
                  key: {{ include "distant-signal.pollerInternalTokenSecretKey" (dict "root" . "name" "incidents" "poller" .Values.pollers.incidents) }}
            # ...repeat for stations, tocs, ldbws, tfl (same shape, .Values.pollers.<name>)...
            - name: INTERNAL_TOKEN_TRUST_CONSUMER
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.trustConsumerInternalTokenSecretName" . }}
                  key: {{ include "distant-signal.trustConsumerInternalTokenSecretKey" . }}
            - name: INTERNAL_TOKEN_SCHEDULE_INGEST
              valueFrom:
                secretKeyRef:
                  name: {{ include "distant-signal.scheduleIngestInternalTokenSecretName" . }}
                  key: {{ include "distant-signal.scheduleIngestInternalTokenSecretKey" . }}
```

Note: `api-deployment.yaml` is not inside the `pollers` `range` loop, so each of the five poller entries here is written out individually (`.Values.pollers.incidents`, `.Values.pollers.stations`, etc.), unlike Step 4's single shared-loop edit.

- [ ] **Step 7: Render and inspect**

Run: `helm template charts/distant-signal --set pollers.incidents.enabled=true` (and again with `pollers.incidents.enabled=false`) from the repo root.
Expected: both renders succeed with no `nil pointer` / `wrong type` errors; `internal-token-poller-incidents` appears in the rendered `Secret` in **both** cases (confirming Step 3's "unconditional on `enabled`" fix); the `api` Deployment's env block contains all eight `INTERNAL_TOKEN*` entries; the `poller-incidents` Deployment (when enabled) contains both `INTERNAL_TOKEN` and the new (inert) `INTERNAL_TOKEN_POLLER_INCIDENTS`.

- [ ] **Step 8: Commit**

```bash
git add charts/distant-signal/templates/secret.yaml charts/distant-signal/templates/_helpers.tpl \
        charts/distant-signal/values.yaml charts/distant-signal/templates/poller-deployments.yaml \
        charts/distant-signal/templates/trust-consumer-deployment.yaml \
        charts/distant-signal/templates/schedulefeed-deployment.yaml \
        charts/distant-signal/templates/api-deployment.yaml
git commit -m "Add per-service internal-token secret keys and wire all 7 callers plus api"
```

---

### Task 6: Local dev — `docker-compose.yml` + `dev.env.example` + `local.env.example`

**Files:**
- Modify: `docker-compose.yml`
- Modify: `dev.env.example`
- Modify: `local.env.example`

**Interfaces:**
- Produces: eight new `${INTERNAL_TOKEN_*}` interpolations in `docker-compose.yml` (`api` gets all seven; each of the seven services gets its own, unchanged-name `INTERNAL_TOKEN` continuing to work as today, per Global Constraints); matching `INTERNAL_TOKEN_*` entries in both `.env.example` files.
- Consumed by: Task 9 (the manual docker-compose smoke test).
- **Depends on:** Task 2 (final env-var names). Independent of Tasks 3/4/5/7.

**Why one task, not seven:** confirmed in this plan's Status note, `docker-compose.yml` has exactly eight `INTERNAL_TOKEN` occurrences today (one per real consumer, api included) — adding the per-service equivalents is eight small, disjoint one-line edits to the *same* file, not seven independently-landable changes; splitting it would only fragment one coherent diff.

- [ ] **Step 1: Add `${INTERNAL_TOKEN_*}` to each service in `docker-compose.yml`**

`api`'s block (around line 96) gains all seven, alongside its unchanged `INTERNAL_TOKEN`:

```yaml
      INTERNAL_TOKEN_POLLER_INCIDENTS: ${INTERNAL_TOKEN_POLLER_INCIDENTS}
      INTERNAL_TOKEN_POLLER_STATIONS: ${INTERNAL_TOKEN_POLLER_STATIONS}
      INTERNAL_TOKEN_POLLER_TOCS: ${INTERNAL_TOKEN_POLLER_TOCS}
      INTERNAL_TOKEN_POLLER_LDBWS: ${INTERNAL_TOKEN_POLLER_LDBWS}
      INTERNAL_TOKEN_POLLER_TFL: ${INTERNAL_TOKEN_POLLER_TFL}
      INTERNAL_TOKEN_TRUST_CONSUMER: ${INTERNAL_TOKEN_TRUST_CONSUMER}
      INTERNAL_TOKEN_SCHEDULE_INGEST: ${INTERNAL_TOKEN_SCHEDULE_INGEST}
```

Each of the seven services (`poller-incidents` at line 146, `poller-stations` at 167, `poller-tocs` at 191, `poller-ldbws` at 216, `poller-tfl` at 241, `trust-consumer` at 322, `schedule-ingest` at 480) keeps its own existing `INTERNAL_TOKEN: ${INTERNAL_TOKEN}` line **unchanged** — Global Constraints: no poller/service reads a per-service-named env var; only `api`'s side needs to know the eight distinct values to build its registry.

- [ ] **Step 2: Add matching entries to `dev.env.example` and `local.env.example`**

`dev.env.example`, immediately after the existing `INTERNAL_TOKEN=...` line (line 120):

```
# Per-service internal tokens api uses to build its startup identity
# registry (crates/api/src/auth.rs::InternalServiceRegistry) — see
# docs/superpowers/specs/2026-09-01-internal-service-accounts-design.md.
# Each poller/service above keeps sending its unchanged INTERNAL_TOKEN;
# these seven exist only so api can recognize which value belongs to
# which caller. Distinct, arbitrary local-dev values are fine — nothing
# in this repo cross-checks them against anything but this file's own
# INTERNAL_TOKEN, which stays valid for every route during the transition
# window (Decision 5).
INTERNAL_TOKEN_POLLER_INCIDENTS=changeme-poller-incidents-local-dev-only
INTERNAL_TOKEN_POLLER_STATIONS=changeme-poller-stations-local-dev-only
INTERNAL_TOKEN_POLLER_TOCS=changeme-poller-tocs-local-dev-only
INTERNAL_TOKEN_POLLER_LDBWS=changeme-poller-ldbws-local-dev-only
INTERNAL_TOKEN_POLLER_TFL=changeme-poller-tfl-local-dev-only
INTERNAL_TOKEN_TRUST_CONSUMER=changeme-trust-consumer-local-dev-only
INTERNAL_TOKEN_SCHEDULE_INGEST=changeme-schedule-ingest-local-dev-only
```

Repeat verbatim (same variable names, `local.env`-flavored placeholder values) in `local.env.example`, immediately after its own `INTERNAL_TOKEN=` line (line 81) — this file and `dev.env.example` are already-documented as an accepted, self-contained duplication (`dev.env.example:8–14`'s own header); these seven lines extend that existing convention, they don't introduce a new one.

- [ ] **Step 3: Verify `docker compose config` renders cleanly**

Run: `docker compose -f docker-compose.yml config --quiet` with a local `dev.env` copied from the updated `dev.env.example`.
Expected: no error (confirms no YAML/interpolation typo).

- [ ] **Step 4: Commit**

```bash
git add docker-compose.yml dev.env.example local.env.example
git commit -m "Add per-service internal-token env vars for local dev (api-side only)"
```

---

### Task 7: General per-request identity logging (auditability)

**Files:**
- Modify: `crates/api/src/auth.rs`

**Interfaces:**
- Produces: every `InternalAuthOutcome::Allowed`/`Forbidden` case in `require_internal_token` logs the resolved identity, not just the `Forbidden` and `Legacy` cases Tasks 3–4 already cover.
- Consumed by: Task 8 (operators watching for both the Decision 5 legacy-warning and general call volume/identity mix).
- **Depends on:** Task 3. **Not parallelizable with Task 4** — both edit `require_internal_token`'s `Allowed`/`Forbidden` arms; land this after Task 4, or hand-merge if dispatched together.

The design's auditability requirement (referenced throughout the spec as "Decision 6," though — per this plan's Status note — no such numbered section actually exists in the spec) is "lightweight per-request identity logging as a near-free auditability win." Tasks 3–4 already log the two failure/transition cases (`403` and legacy-`200`); this task closes the gap for an ordinary successful per-service request, so every `/private/*` request's resolved identity is visible in `api`'s logs, not just the exceptional ones.

- [ ] **Step 1: Extend the `Allowed` arm with a general identity log**

```rust
        InternalAuthOutcome::Allowed(identity) => {
            let outcome = InternalAuthOutcome::Allowed(identity);
            if outcome.is_legacy_success() {
                tracing::warn!(path, "legacy shared X-Internal-Token used -- migrate this caller to its own per-service token");
            } else {
                tracing::debug!(?identity, path, "internal request authenticated");
            }
            Ok(next.run(request).await)
        }
```

`debug`, not `info`, for the ordinary case — this fires on every single `/private/*` request, including `poller-ldbws`'s every-60s station-sample POST (the same high-frequency route Decision 2 already flagged as the reason the scoping check itself must stay zero-I/O); `info`-level here would make this the loudest line in `api`'s log output for no operational benefit over `debug`. The `Forbidden` and legacy-`Allowed` cases stay at `warn` (Task 3/4) since those genuinely need attention.

- [ ] **Step 2: Add a regression test pinning the log-level choice's underlying condition**

There is nothing new to unit-test here beyond what Task 3/4 already cover (`classify_internal_request`'s outcome and `is_legacy_success` fully determine which branch fires) — this step is deliberately a no-op check, not a gap: run `cargo test -p api auth::` once more and confirm the full suite built in Tasks 1, 3, and 4 still passes unchanged, confirming this step's edit didn't alter any of `require_internal_token`'s decision logic, only its logging.

Run: `cargo test -p api auth::`
Expected: PASS, same test count as after Task 4.

- [ ] **Step 3: Commit**

```bash
git add crates/api/src/auth.rs
git commit -m "Log the resolved identity for every successfully authenticated internal request"
```

---

### Task 8: Rollout runbook — per-environment cutover for the 7 services (operational, not code)

**No files modified.** This task is a written operator runbook + a go/no-go checklist, not a code change — it exists because the user-facing question "does the 7-service migration need one task per service, or one coordinated task?" has a real, non-obvious answer once Decision 5's dual-acceptance window is factored in: **it needs neither.** There is no per-service *code* migration at all (Global Constraints: no poller/service crate is touched anywhere in this plan) — every "migration" step is either the one coordinated Helm/compose change (Tasks 5–6, already landed) or a value an operator sets in their own `values.yaml`/`.env`, and Decision 5's dual-acceptance means those seven values can be set **in any order, on any timeline, independently per service**, because the legacy token keeps every not-yet-migrated caller working throughout.

**Depends on:** Tasks 4, 5, 6.

- [ ] **Step 1: Ship the chart/compose changes with dual-acceptance already live**

Once Tasks 1–7 are merged and a chart version bump / `api` image bump ships, `api` accepts **both** the legacy `internal_token` value (still whatever `secrets.internalToken`/`INTERNAL_TOKEN` already is) and all seven new per-service values, simultaneously, for every route. No operator action is required at this point — every poller/service keeps sending its existing token via its unchanged `internal_token`/`INTERNAL_TOKEN` config, which still resolves to `Legacy` and is still allowed everywhere.

- [ ] **Step 2: (Optional, per-operator, any order) Point each poller/service's `INTERNAL_TOKEN` at its own new secret**

This is the actual "migration" — and it is a **chart values change**, not a code change, applied independently per service at the operator's own pace: e.g. set `pollers.incidents.existingSecret`/override to source `poller-incidents`'s `INTERNAL_TOKEN` env var from the new `internal-token-poller-incidents` key instead of the shared `internal-token` key (this requires one more small Helm change beyond Task 5's scope — Task 5 only *creates* the per-service keys and gives `api` visibility into them; it does **not** repoint each poller's own `INTERNAL_TOKEN` env var at its new key, since doing so immediately would remove the legacy fallback's whole *point* for that service the moment this chart version ships. That repointing is this task's Step 2, deliberately left as a separate, operator-timed values change — e.g. `poller-deployments.yaml`'s existing `INTERNAL_TOKEN` `secretKeyRef` (`poller-deployments.yaml:97–101`) would need to switch from `distant-signal.internalTokenSecretName`/`Key` to `distant-signal.pollerSecretName`/`distant-signal.pollerInternalTokenSecretKey` — **a follow-up chart task, not included in this plan**, since flipping the default for every operator in one release would itself be exactly the "flag-day cutover" Decision 5 rejects. This plan stops at "both are available"; a later plan makes the new one the default.)

Order does not matter across the seven services — `poller-tfl` can move to its own token while `trust-consumer` is still on the legacy one, for any length of time, with no cross-service coordination required, because scoping is per-token, not per-deployment-wave.

- [ ] **Step 3: Watch for the migration signal (Task 7's logs)**

The `tracing::warn!(path, "legacy shared X-Internal-Token used...")` line added in Task 4 is the concrete, observable "is anything still using the old token" signal Decision 5 calls for. An operator greps/dashboards on this line per-service (the `path` field narrows which route, though not which caller — a real limitation worth noting: at `403`/legacy-`warn` time there is no way to distinguish *which pod* sent a legacy request beyond the path it hit, since the shared token carries no per-pod identity by design). Its **absence** for a sustained period (an operational judgment call, not a number this plan fixes — Open Question 2 in the spec leaves this deliberately unresolved) is the signal that every real caller has moved off the legacy token.

- [ ] **Step 4: (Future, separate plan) Retire the legacy token**

Once Step 3's signal has been quiet long enough, a **follow-up plan** (not part of this one — Global Constraints: this plan does not delete `secrets.internalToken`) removes `InternalService::Legacy`, the `internal_token` config field, the `internal-token` chart secret key, and the corresponding `docker-compose.yml`/`.env.example` entries. Flag this explicitly rather than silently leaving it undone: this plan's Task 4 is deliberately a *permanent-until-retired* code path, not a TTL'd one, per Decision 5's own "not a permanent second code path... a real, bounded transition" — the boundedness is enforced by an operator decision (Step 3/4 here), not by any code in this plan.

---

### Task 9: End-to-end verification

**Files:** none modified — verification only.

**Depends on:** everything (Tasks 1–8).

- [ ] **Step 1: Full workspace build and test**

Run: `cargo build --workspace && cargo test --workspace`
Expected: PASS, including every new test added in Tasks 1, 3, and 4.

- [ ] **Step 2: Helm chart render sanity check**

Run, from the repo root:
```bash
helm template charts/distant-signal --set pollers.incidents.enabled=true --set pollers.stations.enabled=true \
  --set pollers.tocs.enabled=true --set pollers.ldbws.enabled=true --set pollers.tfl.enabled=true \
  --set trustConsumer.enabled=true --set scheduleFeed.enabled=true > /tmp/rendered-all-enabled.yaml
helm template charts/distant-signal > /tmp/rendered-defaults.yaml
```
Expected: both succeed with no errors. Grep `/tmp/rendered-defaults.yaml` for `internal-token-poller-incidents` (etc.) in the rendered `Secret` and confirm all seven new keys are present **even with every poller left at its default `enabled: false`** — the concrete check for this plan's Global-Constraints fix to the `rdm-*-api-key` gating pattern. Grep the `api` Deployment's `env:` block in both renders for all eight `INTERNAL_TOKEN*` names.

- [ ] **Step 3: Manual docker-compose smoke test — the four real outcomes**

With a local stack (`docker compose up api poller-incidents postgres redis` or equivalent, `dev.env` populated from Task 6's updated example), exercise `require_internal_token` directly against `api`'s `/private/incidents` route with `curl`:

1. **No/garbage token** → expect `401`. `curl -i -X GET http://localhost:8080/private/incidents` (no header) and again with `-H "X-Internal-Token: garbage"`.
2. **Correct per-service token, own route** → expect `200`/normal response. `curl -i -H "X-Internal-Token: $INTERNAL_TOKEN_POLLER_INCIDENTS" http://localhost:8080/private/incidents`.
3. **Correct per-service token, wrong route** → expect `403`. `curl -i -H "X-Internal-Token: $INTERNAL_TOKEN_POLLER_INCIDENTS" http://localhost:8080/private/schedule-feed-ingests`.
4. **Legacy shared token, any route** → expect `200`, plus a `legacy shared X-Internal-Token used` line in `api`'s container logs (`docker compose logs api | grep "legacy shared"`). `curl -i -H "X-Internal-Token: $INTERNAL_TOKEN" http://localhost:8080/private/schedule-feed-ingests`.

Expected: all four match, confirming Tasks 3–4's logic end to end against a real running `api`, not just unit tests.

- [ ] **Step 4: Confirm no poller/service crate was touched**

Run: `git diff --stat main -- crates/poller-incidents crates/poller-stations crates/poller-tocs crates/poller-ldbws crates/poller-tfl crates/trust-consumer crates/schedule-ingest`
Expected: empty output — the Global Constraints' central claim ("no poller/service crate's own Rust code changes anywhere in this plan") holds all the way through implementation, not just in this plan's stated intent.

- [ ] **Step 5: Final report**

Summarize, against this plan's own Global Constraints: no migration file added, no new Cargo/npm dependency, `403`/`401` split matches Decision 3, `constant_time_eq` removed as dead code, legacy token still works chart-wide, all seven new secret keys render unconditional on `enabled`, and Task 8's runbook — not this plan — owns the actual per-environment cutover and eventual legacy retirement.
